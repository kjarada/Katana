// What the drawing contributes to a reduction (cad/survey_job.hpp,
// reductionContextFor): its survey points, for control taken from the
// drawing, and what its coordinate system knows through katana_geodesy.
//
// katana::survey may not see geodesy (tools/check_layering.cmake), so these
// arrive as functions; this file is where cad, which may, builds them.

#include "katana/cad/survey_job.hpp"

#include <charconv>
#include <memory>
#include <optional>
#include <string>
#include <string_view>

#include "katana/cad/survey_points.hpp"
#include "katana/geodesy/coordinate.hpp"
#include "katana/geodesy/coordinate_reference_system.hpp"
#include "katana/geodesy/grid_factors.hpp"

namespace katana::cad {

namespace {

using katana::core::makeError;
using katana::core::Result;
namespace geodesy = katana::geodesy;

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

} // namespace

Result<katana::survey::ReductionContext> reductionContextFor(const Document& document)
{
    katana::survey::ReductionContext context;

    // Every survey point, by the import's own property names - the same list
    // the Point Manager shows, so "take CP1 from the drawing" means the CP1
    // the person can see. The reduction takes the first of an id the drawing
    // has twice; the wizard is where that ambiguity is shown.
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
