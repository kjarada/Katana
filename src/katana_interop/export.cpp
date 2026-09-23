#include "katana/interop/export.hpp"

#include <algorithm>
#include <cmath>
#include <variant>

#include "katana/gis/gdal_adapter.hpp"
#include "katana/math/numerics.hpp"
#include "katana/pointcloud/point_cloud_engine.hpp"

namespace katana::interop {
namespace {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;
using katana::entity::Entity;
using katana::geometry::Arc2;
using katana::geometry::Circle2;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;
using katana::geometry::Vec2;

katana::gis::GeoPoint toGeo(const Point2& point, const std::optional<Vec2>& origin)
{
    if (origin.has_value()) {
        return katana::gis::GeoPoint{point.x + origin->x, point.y + origin->y, 0.0};
    }
    return katana::gis::GeoPoint{point.x, point.y, 0.0};
}

// Number of chords needed so a circular arc of `radius` spanning `sweep` never
// deviates from the polyline by more than `tolerance`.
//
// The sagitta of a chord subtending an angle phi is r * (1 - cos(phi/2)), so the
// largest admissible phi is 2 * acos(1 - tolerance / r). When the tolerance is
// at or beyond the radius the whole arc is within tolerance of a single chord
// and the formula degenerates, so that case is handled separately rather than
// left to produce a NaN.
int chordCount(double radius, double sweep, double tolerance)
{
    const double absSweep = std::abs(sweep);
    if (!(radius > 0.0) || !(absSweep > 0.0)) {
        return 1;
    }
    if (!(tolerance > 0.0)) {
        return 64; // a caller that asked for zero error gets a fine default
    }
    if (tolerance >= radius) {
        return 1;
    }
    const double maxAngle = 2.0 * std::acos(1.0 - tolerance / radius);
    if (!(maxAngle > 0.0) || !std::isfinite(maxAngle)) {
        return 4096;
    }
    const auto count = static_cast<int>(std::ceil(absSweep / maxAngle));
    return std::clamp(count, 1, 4096);
}

std::vector<katana::gis::GeoPoint> tessellateArc(const Arc2& arc, double tolerance,
                                                 const std::optional<Vec2>& origin)
{
    const int segments = chordCount(arc.radius, arc.sweep, tolerance);
    std::vector<katana::gis::GeoPoint> points;
    points.reserve(static_cast<std::size_t>(segments) + 1);
    for (int i = 0; i <= segments; ++i) {
        const double t = static_cast<double>(i) / static_cast<double>(segments);
        points.push_back(toGeo(arc.pointAt(t), origin));
    }
    return points;
}

std::vector<katana::gis::GeoPoint> tessellateCircle(const Circle2& circle, double tolerance,
                                                    const std::optional<Vec2>& origin)
{
    const int segments = std::max(3, chordCount(circle.radius, katana::math::kTwoPi, tolerance));
    std::vector<katana::gis::GeoPoint> points;
    points.reserve(static_cast<std::size_t>(segments));
    for (int i = 0; i < segments; ++i) {
        const double angle =
            katana::math::kTwoPi * static_cast<double>(i) / static_cast<double>(segments);
        points.push_back(toGeo(circle.pointAtAngle(angle), origin));
    }
    return points;
}

std::string propertyToString(const katana::entity::PropertyValue& value)
{
    return std::visit(
        [](const auto& held) -> std::string {
            using Held = std::decay_t<decltype(held)>;
            if constexpr (std::is_same_v<Held, std::string>) {
                return held;
            } else if constexpr (std::is_same_v<Held, bool>) {
                return held ? "true" : "false";
            } else if constexpr (std::is_same_v<Held, std::int64_t>) {
                return std::to_string(held);
            } else {
                // Round trip exactly: shortest representation that reads back
                // as the same double.
                char buffer[32];
                const int written = std::snprintf(buffer, sizeof(buffer), "%.17g",
                                                  static_cast<double>(held));
                return written > 0 ? std::string(buffer, static_cast<std::size_t>(written))
                                   : std::string();
            }
        },
        value);
}

} // namespace

std::vector<FormatChoice> vectorExportFormats()
{
    return {
        {"ESRI Shapefile", "shp"}, {"GeoJSON", "geojson"}, {"GeoPackage", "gpkg"},
        {"Google Earth KML", "kml"}, {"AutoCAD DXF", "dxf"}, {"GML", "gml"},
    };
}

Result<VectorExportResult> exportVector(const katana::entity::Model& model,
                                        const std::filesystem::path& path,
                                        const VectorExportOptions& options)
{
    // Fail on an unsupported extension BEFORE walking the model, so a mistyped
    // filename costs nothing and the error names the extension.
    std::string driver = options.driver;
    if (driver.empty()) {
        auto inferred = katana::gis::GdalDataset::vectorDriverForPath(path);
        if (!inferred.ok()) {
            return inferred.error();
        }
        driver = *inferred;
    }

    VectorExportResult result;
    result.driver = driver;

    const bool filterByEntity = !options.entities.empty();
    const bool filterByLayer = !options.layers.empty();

    std::vector<katana::gis::VectorFeature> features;
    std::size_t textSkipped = 0;
    std::size_t dimensionSkipped = 0;
    std::size_t incompleteHeights = 0;
    std::size_t slopingArcs = 0;

    const auto append = [&](const Entity& entity) {
        if (filterByLayer && std::find(options.layers.begin(), options.layers.end(),
                                       entity.layer) == options.layers.end()) {
            return;
        }

        katana::gis::VectorGeometry geometry;
        bool supported = true;
        // One height per vertex written, from the properties every importer
        // writes (entity.hpp). A 3D geometry needs one at EVERY vertex, and a
        // missing one is never written as 0.
        std::vector<std::optional<double>> heights;
        const auto uniformHeight = [&](std::size_t written, std::optional<double> z) {
            heights.assign(written, z);
        };

        std::visit(
            [&](const auto& held) {
                using Held = std::decay_t<decltype(held)>;
                if constexpr (std::is_same_v<Held, katana::entity::PointGeometry>) {
                    geometry.kind = katana::gis::GeometryKind::Point;
                    geometry.parts.push_back({toGeo(held.position, options.originShift)});
                    heights = katana::entity::heightsOf(entity.properties, 1);
                } else if constexpr (std::is_same_v<Held, Segment2>) {
                    geometry.kind = katana::gis::GeometryKind::LineString;
                    geometry.parts.push_back({toGeo(held.start, options.originShift),
                                              toGeo(held.end, options.originShift)});
                    heights = katana::entity::heightsOf(entity.properties, 2);
                } else if constexpr (std::is_same_v<Held, Arc2>) {
                    geometry.kind = katana::gis::GeometryKind::LineString;
                    geometry.parts.push_back(
                        tessellateArc(held, options.curveTolerance, options.originShift));
                    // An arc carries a height at each end and nowhere between
                    // (the 12d archive, the only source of one). Level, every
                    // chord point has it; sloping, a height between the ends
                    // would be invented, so the arc goes in plan.
                    const auto ends = katana::entity::heightsOf(entity.properties, 2);
                    if (ends[0] && ends[1] && *ends[0] == *ends[1]) {
                        uniformHeight(geometry.parts.front().size(), ends[0]);
                    } else if (ends[0] || ends[1]) {
                        ++slopingArcs;
                    }
                } else if constexpr (std::is_same_v<Held, Polyline2>) {
                    std::vector<katana::gis::GeoPoint> points;
                    points.reserve(held.vertices.size() + 1);
                    for (const Point2& vertex : held.vertices) {
                        points.push_back(toGeo(vertex, options.originShift));
                    }
                    heights = katana::entity::heightsOf(entity.properties, held.vertices.size());
                    if (held.closed && points.size() >= 3) {
                        geometry.kind = katana::gis::GeometryKind::Polygon;
                    } else {
                        geometry.kind = katana::gis::GeometryKind::LineString;
                    }
                    geometry.parts.push_back(std::move(points));
                } else if constexpr (std::is_same_v<Held, Circle2>) {
                    geometry.kind = katana::gis::GeometryKind::Polygon;
                    geometry.parts.push_back(
                        tessellateCircle(held, options.curveTolerance, options.originShift));
                    uniformHeight(geometry.parts.front().size(),
                                  katana::entity::heightsOf(entity.properties, 1)[0]);
                } else if constexpr (std::is_same_v<Held, katana::entity::TextGeometry>) {
                    // A label has no geometry counterpart in these formats.
                    // Exporting its anchor as a point would invent a feature
                    // the drawing does not contain, so it is skipped and
                    // counted instead.
                    supported = false;
                    ++textSkipped;
                } else if constexpr (std::is_same_v<Held, katana::entity::DimensionGeometry>) {
                    // A dimension is annotation over a measurement, not a
                    // feature; the formats here have nowhere to put it.
                    supported = false;
                    ++dimensionSkipped;
                } else {
                    // This was a catch-all that counted anything unhandled as a
                    // skipped DIMENSION - so a geometry kind added later would
                    // be dropped from the export AND reported to the user with a
                    // fluent, confident, wrong sentence. Whoever adds a kind
                    // must decide what the export does with it.
                    static_assert(false, "vector export has no case for this geometry kind");
                }
            },
            entity.geometry);

        if (!supported) {
            ++result.entitiesSkipped;
            return;
        }
        if (geometry.parts.empty() || geometry.parts.front().empty()) {
            ++result.entitiesSkipped;
            return;
        }

        const bool anyHeight = std::any_of(heights.begin(), heights.end(),
                                           [](const auto& z) { return z.has_value(); });
        const bool everyHeight =
            anyHeight && heights.size() == geometry.parts.front().size() &&
            std::all_of(heights.begin(), heights.end(), [](const auto& z) { return z.has_value(); });
        if (everyHeight) {
            for (std::size_t i = 0; i < heights.size(); ++i) {
                geometry.parts.front()[i].z = *heights[i];
            }
            geometry.hasZ = true;
        } else if (anyHeight) {
            ++incompleteHeights; // in plan, its heights kept as attributes below
        }

        katana::gis::VectorFeature feature;
        feature.geometry = std::move(geometry);
        feature.attributes.emplace("katana_id", std::to_string(entity.id));
        feature.attributes.emplace("layer", entity.layer);
        if (options.propertiesAsAttributes) {
            for (const auto& [key, value] : entity.properties) {
                feature.attributes.emplace(key, propertyToString(value));
            }
        }
        features.push_back(std::move(feature));
    };

    if (filterByEntity) {
        for (const katana::entity::EntityId id : options.entities) {
            if (const Entity* entity = model.entities.find(id)) {
                append(*entity);
            } else {
                ++result.entitiesSkipped;
            }
        }
    } else {
        model.entities.forEach(append);
    }

    if (features.empty()) {
        return makeError(ErrorCode::InvalidArgument,
                         "nothing to export: no entity matched, or none has a geometry this "
                         "format can represent",
                         std::to_string(result.entitiesSkipped) + " skipped");
    }

    // Refuse a mixed collection for a single-type format BEFORE writing
    // anything. GDAL would otherwise accept the first feature, fix the layer's
    // shape type from it, and fail on the first feature of another kind - after
    // a partial file exists.
    if (katana::gis::driverHoldsOneGeometryType(driver)) {
        const auto kindOf = [](const katana::gis::VectorFeature& feature) {
            return feature.geometry.kind;
        };
        const katana::gis::GeometryKind first = kindOf(features.front());
        const bool mixed = std::any_of(features.begin(), features.end(),
                                       [&](const katana::gis::VectorFeature& feature) {
                                           return kindOf(feature) != first;
                                       });
        if (mixed) {
            const auto count = [&](katana::gis::GeometryKind kind) {
                return std::count_if(features.begin(), features.end(),
                                     [&](const katana::gis::VectorFeature& feature) {
                                         return kindOf(feature) == kind;
                                     });
            };
            return makeError(
                ErrorCode::Unsupported,
                driver + " stores one geometry type per file, but this selection mixes them",
                std::to_string(count(katana::gis::GeometryKind::Point)) + " points, " +
                    std::to_string(count(katana::gis::GeometryKind::LineString)) + " lines, " +
                    std::to_string(count(katana::gis::GeometryKind::Polygon)) +
                    " polygons. Export to GeoPackage or GeoJSON, which hold all three, or "
                    "select one kind at a time.");
        }
    }

    // Heights written into a geometry are dropped from its attributes, where
    // they would only repeat it. The exception is a format whose layer is
    // either 2D or 3D (a shapefile): a 3D layer has no way to say "no height",
    // so the drawing's heightless entities would be written at 0. When some
    // entities have heights and some do not, such a layer is written in plan
    // and every height stays an attribute - reported, not decided silently.
    const std::size_t withHeights = static_cast<std::size_t>(
        std::count_if(features.begin(), features.end(),
                      [](const katana::gis::VectorFeature& feature) { return feature.geometry.hasZ; }));
    const bool inPlan = withHeights > 0 && withHeights < features.size() &&
                        katana::gis::driverHoldsOneGeometryType(driver);
    for (katana::gis::VectorFeature& feature : features) {
        if (inPlan) {
            feature.geometry.hasZ = false;
        } else if (feature.geometry.hasZ) {
            feature.attributes.erase(std::string(katana::entity::kElevationProperty));
            feature.attributes.erase(std::string(katana::entity::kElevationsProperty));
        }
    }

    katana::gis::VectorExportOptions gdalOptions;
    gdalOptions.driver = driver;
    gdalOptions.layerName = options.layerName;
    gdalOptions.projectionWkt = options.projectionWkt;

    const Status status = katana::gis::GdalDataset::writeVector(path, features, gdalOptions);
    if (!status.ok()) {
        return status.error();
    }

    result.featuresWritten = features.size();
    if (katana::gis::driverHasFixedFields(driver)) {
        result.warnings.push_back(
            driver + " has a fixed set of fields: every entity keeps its geometry and its "
                     "layer, but entity ids and properties were not written. Use GeoPackage "
                     "to keep them.");
    }
    if (options.projectionWkt.empty() && katana::gis::driverAssumesWgs84(driver)) {
        // RFC 7946 defines GeoJSON coordinates as WGS 84 longitude/latitude, so
        // readers will label these as lat/lon whatever they really are. Saying
        // so is the difference between a known limitation and a silent lie.
        result.warnings.push_back(
            "GeoJSON always declares WGS 84 longitude/latitude; these coordinates were "
            "written unchanged and will be read back as degrees. Use GeoPackage to keep a "
            "projected coordinate system.");
    }
    if (textSkipped > 0) {
        result.warnings.push_back(std::to_string(textSkipped) +
                                  " text entities were skipped: this format has no text geometry");
    }
    if (dimensionSkipped > 0) {
        result.warnings.push_back(
            std::to_string(dimensionSkipped) +
            " dimension entities were skipped: this format has no dimension geometry");
    }
    if (inPlan) {
        result.warnings.push_back(
            std::to_string(withHeights) + " of " + std::to_string(features.size()) +
            " entities have heights, but a " + driver +
            " layer is either all 2D or all 3D and a 3D one cannot say \"no height\": the "
            "layer was written in plan and the heights are in the elevation attributes. "
            "Export to GeoPackage or GeoJSON to keep them in the geometry.");
    }
    if (incompleteHeights > 0) {
        result.warnings.push_back(
            std::to_string(incompleteHeights) +
            " entities have heights at only some of their vertices and were written in plan, "
            "their heights kept as attributes: a 3D geometry needs a height at every vertex");
    }
    if (slopingArcs > 0) {
        result.warnings.push_back(
            std::to_string(slopingArcs) +
            " arcs have different heights at their two ends and were written in plan, their "
            "heights kept as attributes: a height between the ends of an arc is not recorded");
    }
    return result;
}

Status exportPointCloud(const PointCloudLayer& cloud, const std::filesystem::path& path)
{
    if (cloud.points.empty()) {
        return makeError(ErrorCode::InvalidArgument, "point cloud layer holds no points",
                         cloud.name);
    }

    katana::pointcloud::PointCloud out;
    out.points = cloud.points;
    out.bounds = cloud.bounds;
    out.projectionWkt = cloud.projectionWkt;
    out.sourcePointCount = cloud.sourcePointCount;

    const katana::pointcloud::PointCloudEngine engine;
    return engine.write(path, out);
}

std::vector<FormatChoice> rasterExportFormats()
{
    // Exactly the extensions gis::GdalDataset::rasterDriverForPath maps, so
    // nothing offered here can then be refused by name.
    return {
        {"GeoTIFF", "tif"},
        {"Esri ASCII grid", "asc"},
        {"Erdas Imagine", "img"},
    };
}

std::vector<FormatChoice> pointCloudExportFormats()
{
    return {
        {"LAS", "las"},
        {"LAZ (compressed LAS)", "laz"},
    };
}

} // namespace katana::interop
