#pragma once

// What a reduction and adjustment did to the data, as structured values.
//
// The report is the answer to "what happened to my data?": every raw value,
// every correction applied to it and how big it was, what came out, what was
// thrown away and why, and how well it all fitted. It is data first and text
// second, so that the same report renders as plain text (the job's log), as
// HTML (the wizard's report page, a saved report) and is testable field by
// field without parsing either.
//
// Units are the model's: metres and radians. The renderers convert to
// millimetres and degrees-minutes-seconds for people.

#include <cstddef>
#include <optional>
#include <string>
#include <vector>

#include "katana/survey/data_model.hpp"
#include "katana/survey/error_propagation.hpp"
#include "katana/survey/reduction_settings.hpp"

namespace katana::survey {

// A sentence for a person, and the record it is about when there is one.
struct ReportMessage {
    std::string text;
    SourceRecord source{};

    friend bool operator==(const ReportMessage&, const ReportMessage&) = default;
};

// ---- Input -------------------------------------------------------------------------

// What was read. Filled by the CALLER (katana::survey cannot see the format
// registry) and copied into the report unchanged - see ReductionContext::input.
struct ReportInput {
    std::string fileName;
    std::vector<std::string> siblingFiles{}; // other files the reader used, by name
    std::string formatId;
    std::string formatName; // "Leica GSI-16"
    std::string parserVersion;
    std::size_t recordsRead = 0;
    std::size_t recordsSkipped = 0;
    std::vector<ReportMessage> warnings{};  // the parser's
    std::vector<std::string> notCarried{};  // what the format did not carry

    friend bool operator==(const ReportInput&, const ReportInput&) = default;
};

// ---- Setups ------------------------------------------------------------------------

struct ReportSetup {
    std::string stationId;
    std::string pointId;
    double instrumentHeight = 0.0;
    std::string backsightPointId{};
    // Grid azimuth to the backsight from the known coordinates.
    std::optional<double> backsightAzimuth{};
    // The mean circle reading on the backsight.
    std::optional<double> backsightReading{};
    // Added to every circle reading of this setup to give a grid azimuth.
    std::optional<double> orientationCorrection{};
    // Backsight check: measured minus computed, where both exist.
    std::optional<double> backsightDistanceDifference{}; // metres, horizontal
    std::optional<double> backsightHeightDifference{};   // metres
    InstrumentSettings instrument{};
    SourceRecord source{};

    friend bool operator==(const ReportSetup&, const ReportSetup&) = default;
};

// ---- Observations -------------------------------------------------------------------

enum class CorrectionKind {
    PrismConstant,
    Atmospheric,
    FaceMean,            // the mean of a face-left / face-right pair
    SlopeToHorizontal,
    CurvatureRefraction,
    HeightReduction,     // to the ellipsoid or the geoid
    GridScale,
    CombinedFactor,
    Orientation,         // circle reading to grid azimuth
    InstrumentAndTargetHeight,
    Other,
};

[[nodiscard]] const char* toString(CorrectionKind kind);

// One correction applied to one value.
struct AppliedCorrection {
    CorrectionKind kind = CorrectionKind::Other;
    // The change it made, in the unit of the value (metres or radians):
    // value after = value before + amount.
    double amount = 0.0;
    // For a proportional correction, the factor applied (1 + ppm * 1e-6, a
    // scale factor); absent for an additive one.
    std::optional<double> factor{};
    // Where the number came from, in words: "k = 0.13", "+12.4 ppm from
    // 21.0 C and 1013.2 hPa", "instrument already applied -34.4 mm".
    std::string note{};

    friend bool operator==(const AppliedCorrection&, const AppliedCorrection&) = default;
};

// One raw value and what became of it.
struct ReportObservation {
    std::string stationId{};  // empty for an observation belonging to no setup
    std::string kind;         // observationKindName() of the raw observation
    std::string from;
    std::string to;
    Pointing pointing{};
    bool angular = false;     // raw / reduced are radians when true, metres otherwise
    double raw = 0.0;
    std::vector<AppliedCorrection> corrections{}; // in the order applied
    std::optional<double> reduced{};              // absent when rejected before reduction
    bool rejected = false;
    std::string rejectionReason{}; // "outside the face tolerance", "flagged by Baarda, w = 4.1"
    SourceRecord source{};

    friend bool operator==(const ReportObservation&, const ReportObservation&) = default;
};

// One face-left / face-right pair and its check.
struct FacePairCheck {
    std::string stationId;
    std::string targetId;
    std::size_t leftPointing = 0;
    std::size_t rightPointing = 0;
    // Spreads as FaceTolerances defines them; absent for a quantity the pair
    // did not observe on both faces.
    std::optional<double> horizontalSpread{};
    std::optional<double> zenithSpread{};
    std::optional<double> distanceSpread{};
    bool withinTolerance = true;
    SourceRecord left{};
    SourceRecord right{};

    friend bool operator==(const FacePairCheck&, const FacePairCheck&) = default;
};

// ---- Misclosures --------------------------------------------------------------------

struct MisclosureReport {
    std::string name;                       // traverse or loop name
    std::optional<double> angular{};        // radians
    std::optional<double> angularAllowable{};
    std::optional<double> northing{};       // metres, computed - known
    std::optional<double> easting{};
    std::optional<double> linear{};         // hypot(northing, easting)
    double length = 0.0;                    // metres, total traverse length
    std::optional<double> precisionRatio{}; // N of "1 : N" = length / linear
    std::optional<double> height{};         // metres, levelling or trig height misclosure
    std::optional<bool> withinTolerance{};  // absent when no tolerance applies

    friend bool operator==(const MisclosureReport&, const MisclosureReport&) = default;
};

// ---- Adjustment ---------------------------------------------------------------------

struct ReportResidual {
    std::string observation; // "slope distance 101 -> 102"
    SourceRecord source{};
    bool angular = false;
    double residual = 0.0;   // v = adjusted - observed
    double sigma = 0.0;      // a-priori
    double redundancyNumber = 0.0;
    std::optional<double> standardised{}; // w (Baarda) or tau
    bool flagged = false;
    bool rejected = false;

    friend bool operator==(const ReportResidual&, const ReportResidual&) = default;
};

struct ReportGlobalTest {
    double statistic = 0.0;
    double lowerCritical = 0.0;
    double upperCritical = 0.0;
    double significanceLevel = 0.0;
    bool passed = false;

    friend bool operator==(const ReportGlobalTest&, const ReportGlobalTest&) = default;
};

struct ReportEllipse {
    std::string pointId;
    ErrorEllipse standard{};     // one sigma
    double confidenceScale = 0.0; // multiply `standard` by this for the confidence level

    friend bool operator==(const ReportEllipse& a, const ReportEllipse& b)
    {
        return a.pointId == b.pointId && a.standard.semiMajor == b.standard.semiMajor &&
               a.standard.semiMinor == b.standard.semiMinor &&
               a.standard.orientation == b.standard.orientation &&
               a.confidenceScale == b.confidenceScale;
    }
};

// One adjustment run (horizontal and levels are two).
struct AdjustmentReport {
    std::string method; // "network least squares (horizontal)", "traverse, Bowditch"
    std::size_t unknowns = 0;
    std::size_t observations = 0;
    std::size_t redundancy = 0;
    std::optional<double> varianceFactor{}; // a posteriori; absent at redundancy 0
    std::optional<ReportGlobalTest> globalTest{};
    std::size_t iterations = 0;
    std::vector<ReportResidual> residuals{};
    std::vector<std::string> flaggedOutliers{};  // observation labels
    std::vector<std::string> rejectedOutliers{}; // auto-rejected, in rejection order
    std::vector<ReportEllipse> ellipses{};

    friend bool operator==(const AdjustmentReport&, const AdjustmentReport&) = default;
};

// ---- Coordinates --------------------------------------------------------------------

// How a coordinate came out of the reduction.
enum class ComputationMethod {
    Control,              // held: it went in as control
    Radiation,            // computed from one setup, not adjusted
    TraverseBowditch,
    TraverseTransit,
    TraverseLeastSquares,
    NetworkLeastSquares,
    Gnss,                 // a GNSS position converted to grid
};

[[nodiscard]] const char* toString(ComputationMethod method);

struct CoordinateReport {
    std::string pointId;
    double northing = 0.0;
    double easting = 0.0;
    std::optional<double> elevation{};
    std::optional<double> sigmaNorthing{};
    std::optional<double> sigmaEasting{};
    std::optional<double> sigmaElevation{};
    ComputationMethod method = ComputationMethod::Radiation;
    // Against the previous run of the same job (ReductionContext::previous);
    // absent on the first run and for a point the previous run did not have.
    std::optional<double> shiftNorthing{};
    std::optional<double> shiftEasting{};
    std::optional<double> shiftElevation{};

    friend bool operator==(const CoordinateReport&, const CoordinateReport&) = default;
};

// ---- The report ---------------------------------------------------------------------

struct ReductionReport {
    std::string createdUtc{}; // ISO 8601, supplied by the caller
    ReportInput input{};
    ReductionSettings settings{}; // exactly what was used
    std::vector<ReportSetup> setups{};
    std::vector<ReportObservation> observations{};
    std::vector<FacePairCheck> facePairs{};
    std::vector<MisclosureReport> misclosures{};
    std::vector<AdjustmentReport> adjustments{};
    std::vector<CoordinateReport> coordinates{};
    std::vector<ReportMessage> warnings{}; // the reduction's own

    friend bool operator==(const ReductionReport&, const ReductionReport&) = default;
};

// Plain text, for the job's log and a terminal: sections in the order above,
// angles in degrees-minutes-seconds, lengths in metres to the millimetre.
[[nodiscard]] std::string renderText(const ReductionReport& report);

// A self-contained HTML document (inline CSS, no scripts, no external
// resources) for the wizard's report page and a saved report. Every string
// from the file is escaped: a point id is user data.
[[nodiscard]] std::string renderHtml(const ReductionReport& report);

} // namespace katana::survey
