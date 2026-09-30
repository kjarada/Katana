// What the drawing contributes to a reduction (cad/survey_job.hpp,
// reductionContextFor): its survey points, for control taken from the
// drawing, and what its coordinate system knows through katana_geodesy. And
// the reduction a job on the drawing runs (reduceForDrawing), which checks
// the control held from those points before the reduction takes it.
//
// katana::survey may not see geodesy (tools/check_layering.cmake), so these
// arrive as functions; this file is where cad, which may, builds them.

#include "katana/cad/survey_job.hpp"

#include <charconv>
#include <cmath>
#include <format>
#include <memory>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/cad/survey_points.hpp"
#include "katana/geodesy/coordinate.hpp"
#include "katana/geodesy/coordinate_reference_system.hpp"
#include "katana/geodesy/grid_factors.hpp"
#include "katana/math/numerics.hpp"

namespace katana::cad {

namespace {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
namespace geodesy = katana::geodesy;
namespace survey = katana::survey;

// The projected system a grid scale factor is taken from: the system itself,
// or the horizontal part of a compound "EPSG:h+v" (the form a drawing records
// for grid plus height datum). A compound given any other way yields none,
// because this build has no way to split it; the reduction then refuses the
// grid scale "from the projection" by name rather than assume 1.0.
std::optional<geodesy::CoordinateReferenceSystem>
projectedPart(const geodesy::CoordinateReferenceSystem& crs, std::string_view text)
{
    if (crs.kind() == geodesy::CrsKind::Projected) {
        return crs;
    }
    if (crs.kind() != geodesy::CrsKind::Compound || !crs.isProjected()) {
        return std::nullopt;
    }
    constexpr std::string_view kEpsg = "EPSG:";
    if (!text.starts_with(kEpsg)) {
        return std::nullopt;
    }
    text.remove_prefix(kEpsg.size());
    int code = 0;
    const auto [end, error] = std::from_chars(text.data(), text.data() + text.size(), code);
    if (error != std::errc{} || end == text.data() + text.size() || *end != '+') {
        return std::nullopt;
    }
    auto horizontal = geodesy::CoordinateReferenceSystem::fromEpsg(code);
    if (!horizontal || horizontal->kind() != geodesy::CrsKind::Projected) {
        return std::nullopt;
    }
    return std::move(*horizontal);
}

// "E 500000.0000 N 5000000.0000 Z 100.0000"; "(no height)" where the point
// has none, which is not a height of zero. To a tenth of a millimetre, the
// resolution of math::tolerance::kCoordinate by which sameMark tells two
// marks apart: millimetres, as a coordinate is usually written, could show
// two marks 0.3 mm apart as one place.
std::string placeOf(const survey::SurveyPoint& point)
{
    return std::format("E {:.4f} N {:.4f}", point.easting, point.northing) +
           (point.elevation ? std::format(" Z {:.4f}", *point.elevation)
                            : std::string(" (no height)"));
}

// One ground mark: closer than math::tolerance::kCoordinate across, and in
// height too - where one has a height and the other none they are not, as a
// height held from one would not be held from the other.
bool sameMark(const survey::SurveyPoint& a, const survey::SurveyPoint& b)
{
    constexpr double kSameMark = katana::math::tolerance::kCoordinate;
    if (!(std::hypot(a.northing - b.northing, a.easting - b.easting) < kSameMark)) {
        return false;
    }
    if (a.elevation.has_value() != b.elevation.has_value()) {
        return false;
    }
    return !a.elevation || std::abs(*a.elevation - *b.elevation) < kSameMark;
}

// "A", "A and B", "A, B and C".
std::string listed(const std::vector<std::string>& items)
{
    std::string text;
    for (std::size_t i = 0; i < items.size(); ++i) {
        text += (i == 0 ? "" : i + 1 == items.size() ? " and " : ", ") + items[i];
    }
    return text;
}

} // namespace

Result<survey::ReductionOutcome> reduceForDrawing(const survey::SurveyProject& raw,
                                                  const survey::ReductionSettings& settings,
                                                  const survey::ReductionContext& context)
{
    std::vector<survey::ReportMessage> notes;
    for (const survey::ControlSelection& selection : settings.control) {
        if (selection.origin != survey::ControlOrigin::Drawing) {
            continue;
        }
        const std::string& id = selection.point.pointId;
        std::vector<const survey::SurveyPoint*> onDrawing;
        for (const survey::SurveyPoint& point : context.drawingPoints) {
            if (point.id == id) {
                onDrawing.push_back(&point);
            }
        }
        if (onDrawing.empty()) {
            continue; // the reduction refuses it, naming it
        }
        // The reduction holds the first of an id, and which is first is only
        // the order the points were drawn in: two marks of one id would put
        // the job on either with nothing to say which. Points that are one
        // mark give the same job whichever is taken.
        const survey::SurveyPoint& held = *onDrawing.front();
        std::vector<std::string> places;
        bool oneMark = true;
        for (const survey::SurveyPoint* point : onDrawing) {
            places.push_back(placeOf(*point));
            oneMark = oneMark && sameMark(*point, held);
        }
        if (!oneMark) {
            return makeError(ErrorCode::InvalidArgument,
                             "Control point " + id + " is on the drawing " +
                                 std::to_string(onDrawing.size()) + " times, at " +
                                 listed(places) +
                                 ", so which one to hold is not known; rename or delete all "
                                 "but one.");
        }
        const survey::SurveyPoint* inFile = nullptr;
        for (const survey::SurveyPoint& point : raw.points) {
            if (point.id == id) {
                inFile = &point;
                break;
            }
        }
        bool named = inFile != nullptr;
        for (const survey::UnpositionedPoint& point : raw.unpositionedPoints) {
            named = named || point.id == id;
        }
        // A file read by surveyio names every point an observation, a setup
        // or a feature refers to in one of these two lists
        // (survey::validateProject), so an id in neither is one nothing in the
        // file measures.
        if (!named) {
            notes.push_back({"Control point " + id +
                                 " is held from the drawing, but the file names no point " + id +
                                 ", so holding it changes nothing.",
                             {}});
        } else if (inFile != nullptr && !sameMark(held, *inFile)) {
            notes.push_back({"Control point " + id + " is held where the drawing has it, " +
                                 placeOf(held) + ", not where the file gives it, " +
                                 placeOf(*inFile) + ".",
                             inFile->source});
        }
    }
    auto outcome = survey::reduceAndAdjust(raw, settings, context);
    if (!outcome) {
        return outcome.error();
    }
    std::vector<survey::ReportMessage>& warnings = outcome->report.warnings;
    warnings.insert(warnings.begin(), notes.begin(), notes.end());
    return outcome;
}

Result<katana::survey::ReductionContext> reductionContextFor(const Document& document)
{
    katana::survey::ReductionContext context;

    // Every survey point, by the import's own property names - the same list
    // the Point Manager shows, so "take CP1 from the drawing" means the CP1
    // the person can see. An id the drawing has twice stays in twice: the
    // reduction would hold the first, so reduceForDrawing refuses to hold an
    // id whose points are not one mark, naming each - in the wizard and on
    // the SURVEY IMPORT line alike.
    const std::vector<DrawingSurveyPoint> points = drawingSurveyPoints(document);
    context.drawingPoints.reserve(points.size());
    for (const DrawingSurveyPoint& point : points) {
        context.drawingPoints.push_back(toSurveyPoint(point));
    }

    const std::string& system = document.metadata().coordinateSystem;
    if (system.empty()) {
        return context; // a local drawing: nothing a projection could answer
    }
    auto crs = geodesy::CoordinateReferenceSystem::fromUserInput(system);
    if (!crs) {
        // Not left empty quietly: a drawing that SAYS it is on a grid and
        // cannot be read would otherwise reduce as if it were local.
        return makeError(crs.error().code,
                         "the drawing's coordinate system \"" + system +
                             "\" cannot be read, so its grid scale factor is unknown: " +
                             crs.error().message,
                         crs.error().context);
    }

    if (auto projected = projectedPart(*crs, system)) {
        auto calculator = geodesy::GridFactorCalculator::create(*projected);
        if (!calculator) {
            return calculator.error();
        }
        // Shared, because a std::function must be copyable and a calculator
        // (a PROJ context) is not. Not thread-safe, like the calculator: a
        // reduction runs on one thread.
        auto shared = std::make_shared<geodesy::GridFactorCalculator>(std::move(*calculator));
        // The model is metres; the projection's coordinates are in its own
        // unit (geodesy/coordinate.hpp), so a feet grid is given feet.
        const double unit = projected->horizontalUnitToSi();
        context.gridScaleFactor = [shared, unit](double northing, double easting,
                                                 double /*ellipsoidalHeight*/)
            -> std::optional<double> {
            // The height does not enter: the point scale factor is the
            // projection's, at the point's place on the ellipsoid. Taking a
            // distance to the ellipsoid is the height reduction's job.
            auto factors = shared->at(geodesy::ProjectedCoordinate{easting / unit, northing / unit});
            if (!factors) {
                return std::nullopt; // outside the projection: the reduction says so
            }
            return factors->pointScaleFactor();
        };
    }

    // Left EMPTY, on purpose, until katana_geodesy can answer them:
    //
    // geoidSeparation - geodesy has no geoid model, and the PROJ installed
    //   here has no geoid grid, so there is nothing to ask. The reduction
    //   refuses a height reduction to the ellipsoid, naming it.
    // geocentricToGrid / geodeticToGrid - a GNSS value carries its own
    //   reference frame (GnssGlobalPositionObservation::referenceFrame) and
    //   these functions are not told it. Converting as if it were the
    //   drawing's datum would move a WGS 84 position by as much as the frames
    //   differ (1.8 m between WGS 84 and GDA2020) with nothing to say so.
    return context;
}

} // namespace katana::cad
