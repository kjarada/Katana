#include "katana/interop/export.hpp"

#include <algorithm>
#include <cmath>
#include <cstddef>
#include <variant>

#include "katana/gis/formats.hpp"
#include "katana/gis/gdal_adapter.hpp"
#include "katana/gis/processing.hpp"
#include "katana/interop/geo/drawing_dataset.hpp"
#include "katana/pointcloud/point_cloud_engine.hpp"

namespace katana::interop {
namespace {

namespace geo = katana::interop::geo;
namespace gp = katana::gis::processing;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;
using katana::entity::Entity;

} // namespace

std::vector<FormatChoice> vectorExportFormats()
{
    // Every writer of vector layers this GDAL has, the common ones first
    // (gis::vectorSaveChoices): six hand-picked entries used to hide
    // FlatGeobuf, GeoParquet, KMZ, GPX and CSV, which EXPORT could write.
    std::vector<FormatChoice> choices;
    for (const katana::gis::SaveChoice& choice : katana::gis::vectorSaveChoices()) {
        choices.push_back(FormatChoice{choice.description, choice.extension});
    }
    return choices;
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

    // The entities named, or the whole drawing, on the layers named.
    const bool filterByLayer = !options.layers.empty();
    const auto onLayer = [&](const Entity& entity) {
        return !filterByLayer ||
               std::find(options.layers.begin(), options.layers.end(), entity.layer) !=
                   options.layers.end();
    };
    std::vector<katana::entity::EntityId> ids;
    if (!options.entities.empty()) {
        for (const katana::entity::EntityId id : options.entities) {
            const Entity* entity = model.entities.find(id);
            // One the drawing does not hold is counted missing by the
            // conversion.
            if (entity == nullptr || onLayer(*entity)) {
                ids.push_back(id);
            }
        }
    } else {
        model.entities.forEach([&](const Entity& entity) {
            if (onLayer(entity)) {
                ids.push_back(entity.id);
            }
        });
    }

    // The ONE conversion of entities to features (drawing_dataset.hpp): the
    // one EXPORT and every algorithm read the drawing through, so a lot with
    // a hole is written as one polygon with a hole - 9600 m2, not a lot of
    // 10000 and a separate "hole" of 400 - and a property keeps its type.
    geo::DrawingDatasetOptions conversion;
    conversion.curveTolerance = options.curveTolerance;
    conversion.crsWkt = options.projectionWkt;
    conversion.styleFields = false;
    conversion.properties = options.propertiesAsAttributes;
    // GPX holds a layer of points and one of lines, and no areas: a closed
    // shape goes to it as the closed line around it.
    const bool noAreas = katana::gis::driverHoldsNoAreas(driver);
    conversion.closedAsPolygons = !noAreas;
    conversion.oneTable = !noAreas;
    conversion.tableName = options.layerName;
    auto dataset = geo::drawingDataset(model, ids, conversion);
    if (!dataset) {
        return dataset.error();
    }
    const geo::DrawingDatasetStats& stats = dataset->stats;
    gp::FeatureSet& set = dataset->set;
    const auto skippedAs = [&stats](const char* reason) -> std::size_t {
        const auto found = stats.skipped.find(reason);
        return found == stats.skipped.end() ? 0 : found->second;
    };
    const std::size_t textSkipped = skippedAs("text");
    const std::size_t dimensionSkipped = skippedAs("dimension");
    const std::size_t annotationSkipped = skippedAs("label") + skippedAs("leader");
    for (const auto& [reason, count] : stats.skipped) {
        result.entitiesSkipped += count;
    }

    std::size_t featureCount = 0;
    std::size_t points = 0, lines = 0, polygons = 0;
    for (const gp::FeatureTable& table : set.tables) {
        for (const gp::Feature& feature : table.features) {
            ++featureCount;
            const katana::gis::GeometryKind kind =
                feature.parts.empty() ? katana::gis::GeometryKind::Unknown
                                      : feature.parts.front().kind;
            points += kind == katana::gis::GeometryKind::Point ? 1u : 0u;
            lines += kind == katana::gis::GeometryKind::LineString ? 1u : 0u;
            polygons += kind == katana::gis::GeometryKind::Polygon ? 1u : 0u;
        }
    }
    if (featureCount == 0) {
        return makeError(ErrorCode::InvalidArgument,
                         "nothing to export: no entity matched, or none has a geometry this "
                         "format can represent",
                         std::to_string(result.entitiesSkipped) + " skipped");
    }

    // Refuse a mixed collection for a single-type format BEFORE writing
    // anything. GDAL would otherwise accept the first feature, fix the layer's
    // shape type from it, and fail on the first feature of another kind - after
    // a partial file exists.
    if (katana::gis::driverHoldsOneGeometryType(driver) &&
        (points != 0) + (lines != 0) + (polygons != 0) > 1) {
        return makeError(
            ErrorCode::Unsupported,
            driver + " stores one geometry type per file, but this selection mixes them",
            std::to_string(points) + " points, " + std::to_string(lines) + " lines, " +
                std::to_string(polygons) +
                " polygons. Export to GeoPackage or GeoJSON, which hold all three, or "
                "select one kind at a time.");
    }

    if (options.originShift) {
        // Added back: the file gets the coordinates the import shifted.
        for (gp::FeatureTable& table : set.tables) {
            for (gp::Feature& feature : table.features) {
                for (katana::gis::VectorGeometry& part : feature.parts) {
                    for (auto& ring : part.parts) {
                        for (katana::gis::GeoPoint& point : ring) {
                            point.x += options.originShift->x;
                            point.y += options.originShift->y;
                        }
                    }
                }
            }
        }
    }

    // Heights written into a geometry are dropped from its attributes, where
    // they would only repeat it. The exception is a format whose layer is
    // either 2D or 3D (a shapefile): a 3D layer has no way to say "no height",
    // so the drawing's heightless entities would be written at 0. When some
    // entities have heights and some do not, such a layer is written in plan
    // and every height stays an attribute - reported, not decided silently.
    std::size_t withHeights = 0;
    for (const gp::FeatureTable& table : set.tables) {
        for (const gp::Feature& feature : table.features) {
            withHeights += !feature.parts.empty() && feature.parts.front().hasZ ? 1u : 0u;
        }
    }
    const bool inPlan = withHeights > 0 && withHeights < featureCount &&
                        katana::gis::driverHoldsOneGeometryType(driver);
    for (gp::FeatureTable& table : set.tables) {
        if (inPlan) {
            table.hasZ = false;
        }
        std::vector<std::size_t> heightFields;
        for (std::size_t f = 0; f < table.fields.size(); ++f) {
            if (table.fields[f].name == katana::entity::kElevationProperty ||
                table.fields[f].name == katana::entity::kElevationsProperty) {
                heightFields.push_back(f);
            }
        }
        for (gp::Feature& feature : table.features) {
            for (katana::gis::VectorGeometry& part : feature.parts) {
                if (inPlan) {
                    part.hasZ = false;
                }
            }
            const bool heighted = !feature.parts.empty() && feature.parts.front().hasZ;
            for (const std::size_t f : heightFields) {
                if (heighted && f < feature.values.size()) {
                    feature.values[f] = std::monostate{};
                }
            }
        }
        // A height field no feature needs any more is not written at all.
        for (auto f = heightFields.rbegin(); f != heightFields.rend(); ++f) {
            const bool used = std::ranges::any_of(table.features, [&](const gp::Feature& feature) {
                return *f < feature.values.size() &&
                       !std::holds_alternative<std::monostate>(feature.values[*f]);
            });
            if (used) {
                continue;
            }
            table.fields.erase(table.fields.begin() + static_cast<std::ptrdiff_t>(*f));
            for (gp::Feature& feature : table.features) {
                if (*f < feature.values.size()) {
                    feature.values.erase(feature.values.begin() + static_cast<std::ptrdiff_t>(*f));
                }
            }
        }
    }

    katana::gis::VectorExportOptions gdalOptions;
    gdalOptions.driver = driver;
    gdalOptions.layerName = options.layerName;
    gdalOptions.projectionWkt = options.projectionWkt;

    auto written = katana::gis::GdalDataset::writeTables(path, set, gdalOptions);
    if (!written.ok()) {
        return written.error();
    }

    result.featuresWritten = written->featuresWritten;
    result.entitiesSkipped += written->featuresSkipped;
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
    if (annotationSkipped > 0) {
        result.warnings.push_back(
            std::to_string(annotationSkipped) +
            " labels and leaders were skipped: this format has no annotation geometry");
    }
    if (inPlan) {
        result.warnings.push_back(
            std::to_string(withHeights) + " of " + std::to_string(featureCount) +
            " entities have heights, but a " + driver +
            " layer is either all 2D or all 3D and a 3D one cannot say \"no height\": the "
            "layer was written in plan and the heights are in the elevation attributes. "
            "Export to GeoPackage or GeoJSON to keep them in the geometry.");
    }
    // What the conversion could not carry (a height at only some vertices, a
    // property of several types) and what the writer said (GDAL's warnings,
    // a conversion to longitude and latitude).
    for (const std::string& warning : stats.warnings) {
        result.warnings.push_back(warning);
    }
    for (std::string& warning : written->warnings) {
        result.warnings.push_back(std::move(warning));
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
