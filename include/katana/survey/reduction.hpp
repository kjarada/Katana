#pragma once

// Reduction and adjustment of raw field observations: the step between what a
// parser read (surveyio) and the coordinates that go on the drawing (cad).
//
//   raw SurveyProject + ReductionSettings + ReductionContext
//       -> reduceAndAdjust
//       -> ReductionOutcome { reduced project, computed points, report }
//
// The pipeline, in the order the corrections are applied to one pointing:
//   1. prism constant                (PrismConstantPolicy)
//   2. atmospheric ppm               (AtmosphericCorrection - Auto only where the
//                                     instrument did not apply it)
//   3. face left / face right mean   (FaceHandling, with FaceTolerances checked)
//   4. slope to horizontal, with curvature and refraction (k, R)
//   5. height reduction              (to the ellipsoid or the geoid)
//   6. grid scale or combined factor
//   7. orientation of circle readings to grid azimuths from the backsight
//   8. coordinates: radiation, a traverse, or a network adjustment
//      (the existing survey::adjustHorizontalNetwork / adjustLevelNetwork /
//      computeTraverse / adjustTraverse - Eigen-based, not re-implemented)
// Every step writes what it did into the report.
//
// katana::survey may not see geodesy (tools/check_layering.cmake), so
// everything that needs a projection or a geoid comes in through
// ReductionContext as functions, which cad builds from katana_geodesy.
//
// Pure: no state, the inputs are not modified, identical inputs give bitwise
// identical outputs (the report's time stamp is supplied, not read).

#include <functional>
#include <optional>
#include <string>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/survey/data_model.hpp"
#include "katana/survey/reduction_report.hpp"
#include "katana/survey/reduction_settings.hpp"

namespace katana::survey {

// A position in the drawing's grid system.
struct GridPosition {
    double northing = 0.0;
    double easting = 0.0;
    double ellipsoidalHeight = 0.0; // metres above the ellipsoid

    friend bool operator==(const GridPosition&, const GridPosition&) = default;
};

// One coordinate the reduction produced.
struct ComputedPoint {
    std::string id;
    double northing = 0.0;
    double easting = 0.0;
    std::optional<double> elevation{};
    // Absent where nothing gave the point a precision (radiation without an
    // adjustment); zero for a held component of control.
    std::optional<double> sigmaNorthing{};
    std::optional<double> sigmaEasting{};
    std::optional<double> sigmaElevation{};
    ComputationMethod method = ComputationMethod::Radiation;

    friend bool operator==(const ComputedPoint&, const ComputedPoint&) = default;
};

// What the caller supplies besides the data and the settings.
//
// Every function may be empty. An empty function is a statement that the
// drawing cannot answer the question, and the reduction then refuses the
// setting that needs it (GridScale::FromProjection with no gridScaleFactor is
// an InvalidArgument naming the setting) rather than quietly using 1.0.
struct ReductionContext {
    // Points already on the drawing, by id. The reduction holds the ones the
    // settings name as ControlOrigin::Drawing; the rest are ignored.
    std::vector<SurveyPoint> drawingPoints{};
    // The point scale factor of the drawing's projection at a grid position.
    std::function<std::optional<double>(double northing, double easting,
                                        double ellipsoidalHeight)>
        gridScaleFactor{};
    // Geoid separation N (ellipsoidal height = orthometric height + N), metres.
    std::function<std::optional<double>(double northing, double easting)> geoidSeparation{};
    // GNSS global values into the drawing's grid.
    std::function<std::optional<GridPosition>(const GeocentricCoordinate&)> geocentricToGrid{};
    std::function<std::optional<GridPosition>(const GeodeticCoordinate&)> geodeticToGrid{};
    // Copied into ReductionReport::input unchanged.
    ReportInput input{};
    // The coordinates of the previous run of the same job, for the report's
    // shifts when re-adjusting. Empty on a first import.
    std::vector<ComputedPoint> previous{};
    // Stamped on the report: the caller's clock, so a test is deterministic.
    std::string createdUtc{};
};

struct ReductionOutcome {
    // The raw project with its observations reduced (horizontal distances,
    // oriented directions, face means) and its computed points added as
    // SurveyPoints with CoordinateSource::Calculated. What cad draws.
    SurveyProject reduced;
    std::vector<ComputedPoint> points;
    ReductionReport report;
};

// Reduces `raw` under `settings`.
//
// Errors are for input that cannot be reduced at all, in a sentence a
// surveyor can act on: InvalidArgument for settings that fail
// validateReductionSettings or need a context function that is empty;
// NotFound for control naming a point neither the file nor the drawing has;
// AdjustmentFailure from the adjustment (no control, a datum defect,
// divergence), with its message. A single bad observation is NOT an error:
// it is rejected, and the report says which and why.
[[nodiscard]] katana::core::Result<ReductionOutcome>
reduceAndAdjust(const SurveyProject& raw, const ReductionSettings& settings,
                const ReductionContext& context);

} // namespace katana::survey
