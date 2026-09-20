#include "katana/interop/import.hpp"

#include <algorithm>
#include <cctype>
#include <cmath>
#include <limits>
#include <string>

namespace katana::interop {
namespace {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::entity::Entity;
using katana::geometry::Box2;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;
using katana::geometry::Vec2;

std::string lowerExtension(const std::filesystem::path& path)
{
    std::string extension = path.extension().string();
    if (!extension.empty() && extension.front() == '.') {
        extension.erase(extension.begin());
    }
    std::transform(extension.begin(), extension.end(), extension.begin(),
                   [](unsigned char ch) { return static_cast<char>(std::tolower(ch)); });
    return extension;
}

bool contains(const std::vector<std::string>& values, const std::string& value)
{
    return std::find(values.begin(), values.end(), value) != values.end();
}

// Converts a source layer name into something usable as a Katana layer name.
// Keeps it recognisable rather than mangling it: only characters that would be
// confusing in a layer list are replaced.
std::string sanitizeLayerName(std::string name)
{
    for (char& ch : name) {
        const auto value = static_cast<unsigned char>(ch);
        if (std::iscntrl(value) != 0 || ch == '\n' || ch == '\r' || ch == '\t') {
            ch = ' ';
        }
    }
    // Trim
    const auto first = name.find_first_not_of(' ');
    if (first == std::string::npos) {
        return "IMPORT";
    }
    const auto last = name.find_last_not_of(' ');
    return name.substr(first, last - first + 1);
}

Point2 shifted(const katana::gis::GeoPoint& point, const std::optional<Vec2>& origin)
{
    if (origin.has_value()) {
        return Point2(point.x - origin->x, point.y - origin->y);
    }
    return Point2(point.x, point.y);
}

// Drops consecutive duplicates, which shapefiles are full of and which would
// make a zero-length segment the model then rejects.
std::vector<Point2> distinctPoints(const std::vector<katana::gis::GeoPoint>& source,
                                   const std::optional<Vec2>& origin)
{
    std::vector<Point2> points;
    points.reserve(source.size());
    for (const katana::gis::GeoPoint& raw : source) {
        if (!std::isfinite(raw.x) || !std::isfinite(raw.y)) {
            continue;
        }
        const Point2 point = shifted(raw, origin);
        if (!points.empty() && points.back() == point) {
            continue;
        }
        points.push_back(point);
    }
    return points;
}

void copyAttributes(const katana::gis::VectorFeature& feature, Entity& entity)
{
    for (const auto& [key, value] : feature.attributes) {
        entity.properties.emplace(key, value);
    }
}

} // namespace

const char* toString(SourceKind kind)
{
    switch (kind) {
    case SourceKind::Vector:
        return "vector";
    case SourceKind::Raster:
        return "raster";
    case SourceKind::PointCloud:
        return "point cloud";
    case SourceKind::Unknown:
        break;
    }
    return "unknown";
}

std::vector<std::string> vectorExtensions()
{
    return {"shp", "geojson", "json", "gpkg", "kml", "gml", "dxf", "tab", "sqlite"};
}

std::vector<std::string> rasterExtensions()
{
    return {"tif", "tiff", "geotiff", "img", "vrt", "png", "jpg", "jpeg", "asc", "dem", "bil",
            "ecw", "jp2"};
}

std::vector<std::string> pointCloudExtensions()
{
    return {"las", "laz", "copc", "bpf", "ply", "pcd", "e57"};
}

SourceKind kindForPath(const std::filesystem::path& path)
{
    const std::string extension = lowerExtension(path);
    if (extension.empty()) {
        return SourceKind::Unknown;
    }
    // Point cloud first: PDAL and GDAL both claim .ply, and a .ply in a survey
    // context is a point cloud far more often than it is a vector layer.
    if (contains(pointCloudExtensions(), extension)) {
        return SourceKind::PointCloud;
    }
    if (contains(rasterExtensions(), extension)) {
        return SourceKind::Raster;
    }
    if (contains(vectorExtensions(), extension)) {
        return SourceKind::Vector;
    }
    return SourceKind::Unknown;
}

// ---- vector ---------------------------------------------------------------

Result<VectorImportResult> importVector(const std::filesystem::path& path,
                                        const VectorImportOptions& options)
{
    auto dataset = katana::gis::GdalDataset::open(path);
    if (!dataset.ok()) {
        return dataset.error();
    }
    if (!(*dataset)->hasVector()) {
        return makeError(ErrorCode::InvalidArgument, "file holds no vector layers",
                         path.string());
    }

    auto layers = (*dataset)->vectorLayers();
    if (!layers.ok()) {
        return layers.error();
    }
    if (layers->empty()) {
        return makeError(ErrorCode::InvalidArgument, "file holds no vector layers",
                         path.string());
    }

    const int layerCount = static_cast<int>(layers->size());
    if (options.sourceLayerIndex >= layerCount) {
        return makeError(ErrorCode::InvalidArgument, "vector layer index is out of range",
                         "requested " + std::to_string(options.sourceLayerIndex) + " of " +
                             std::to_string(layerCount));
    }

    const int firstLayer = options.sourceLayerIndex < 0 ? 0 : options.sourceLayerIndex;
    const int lastLayer = options.sourceLayerIndex < 0 ? layerCount - 1 : options.sourceLayerIndex;

    VectorImportResult result;
    result.projectionWkt = (*layers)[static_cast<std::size_t>(firstLayer)].projectionWkt;

    const std::string sourceName = path.filename().string();
    std::uint64_t remaining = options.maxFeatures;

    for (int index = firstLayer; index <= lastLayer; ++index) {
        const auto& info = (*layers)[static_cast<std::size_t>(index)];

        const std::string targetLayer =
            options.targetLayer.empty()
                ? sanitizeLayerName(info.name.empty() ? path.stem().string() : info.name)
                : options.targetLayer;
        if (!contains(result.layersNeeded, targetLayer)) {
            result.layersNeeded.push_back(targetLayer);
        }

        if (!info.projectionWkt.empty() && !result.projectionWkt.empty() &&
            info.projectionWkt != result.projectionWkt) {
            result.warnings.push_back("layer '" + info.name +
                                      "' declares a different coordinate reference system from "
                                      "the first layer; coordinates are imported unchanged");
        }

        auto features = (*dataset)->readFeatures(index, remaining);
        if (!features.ok()) {
            return features.error();
        }

        for (const katana::gis::VectorFeature& feature : *features) {
            ++result.featuresRead;

            auto makeEntity = [&](katana::entity::Geometry geometry) {
                Entity entity;
                entity.geometry = std::move(geometry);
                entity.layer = targetLayer;
                if (options.attributesAsProperties) {
                    copyAttributes(feature, entity);
                }
                entity.metadata.emplace("source.file", sourceName);
                entity.metadata.emplace("source.layer", info.name);
                result.entities.push_back(std::move(entity));
            };

            switch (feature.geometry.kind) {
            case katana::gis::GeometryKind::Point: {
                if (feature.geometry.parts.empty() || feature.geometry.parts.front().empty()) {
                    ++result.featuresSkipped;
                    break;
                }
                const Point2 position =
                    shifted(feature.geometry.parts.front().front(), options.originShift);
                if (!std::isfinite(position.x) || !std::isfinite(position.y)) {
                    ++result.featuresSkipped;
                    break;
                }
                result.bounds.expand(position);
                makeEntity(katana::entity::PointGeometry{position});
                break;
            }
            case katana::gis::GeometryKind::LineString: {
                const std::vector<Point2> points =
                    distinctPoints(feature.geometry.parts.front(), options.originShift);
                if (points.size() < 2) {
                    ++result.featuresSkipped;
                    break;
                }
                for (const Point2& point : points) {
                    result.bounds.expand(point);
                }
                // Two points is a line in every CAD program; anything longer is
                // a polyline. Importing a two-point line as a polyline would
                // make it unfilletable and odd to edit.
                if (points.size() == 2) {
                    makeEntity(Segment2{points.front(), points.back()});
                } else {
                    Polyline2 polyline;
                    polyline.vertices = points;
                    polyline.closed = false;
                    makeEntity(std::move(polyline));
                }
                break;
            }
            case katana::gis::GeometryKind::Polygon: {
                // The entity model has no polygon-with-holes type, so each ring
                // becomes its own closed polyline. The ring's role is recorded
                // in metadata so the information is not simply lost.
                bool wroteAnyRing = false;
                for (std::size_t ring = 0; ring < feature.geometry.parts.size(); ++ring) {
                    std::vector<Point2> points =
                        distinctPoints(feature.geometry.parts[ring], options.originShift);
                    // A closed ring repeats its first point; the Polyline2
                    // `closed` flag expresses that instead.
                    if (points.size() > 1 && points.front() == points.back()) {
                        points.pop_back();
                    }
                    if (points.size() < 3) {
                        continue;
                    }
                    for (const Point2& point : points) {
                        result.bounds.expand(point);
                    }
                    Polyline2 polyline;
                    polyline.vertices = std::move(points);
                    polyline.closed = true;

                    Entity entity;
                    entity.geometry = std::move(polyline);
                    entity.layer = targetLayer;
                    if (options.attributesAsProperties) {
                        copyAttributes(feature, entity);
                    }
                    entity.metadata.emplace("source.file", sourceName);
                    entity.metadata.emplace("source.layer", info.name);
                    entity.metadata.emplace("source.ring",
                                            std::string(ring == 0 ? "exterior" : "hole"));
                    result.entities.push_back(std::move(entity));
                    wroteAnyRing = true;
                }
                if (!wroteAnyRing) {
                    ++result.featuresSkipped;
                }
                break;
            }
            case katana::gis::GeometryKind::Unknown:
                ++result.featuresSkipped;
                break;
            }

            if (options.maxFeatures != 0 && result.featuresRead >= options.maxFeatures) {
                break;
            }
        }

        if (options.maxFeatures != 0) {
            if (result.featuresRead >= options.maxFeatures) {
                if (index < lastLayer) {
                    result.warnings.push_back(
                        "the feature limit was reached; later layers were not read");
                }
                break;
            }
            remaining = options.maxFeatures - result.featuresRead;
        }
    }

    if (result.entities.empty()) {
        return makeError(ErrorCode::InvalidArgument,
                         "no geometry in this file could be imported",
                         std::to_string(result.featuresRead) + " features read, " +
                             std::to_string(result.featuresSkipped) + " skipped");
    }
    if (result.featuresSkipped > 0) {
        result.warnings.push_back(std::to_string(result.featuresSkipped) +
                                  " features were skipped: empty or degenerate geometry");
    }
    return result;
}

// ---- raster ---------------------------------------------------------------

Result<RasterOverlay> importRaster(const std::filesystem::path& path,
                                   const RasterImportOptions& options)
{
    auto dataset = katana::gis::GdalDataset::open(path);
    if (!dataset.ok()) {
        return dataset.error();
    }
    if (!(*dataset)->hasRaster()) {
        return makeError(ErrorCode::InvalidArgument, "file holds no raster bands", path.string());
    }

    auto image = (*dataset)->readImage(options.maxPixels);
    if (!image.ok()) {
        return image.error();
    }

    RasterOverlay overlay;
    overlay.name = options.name.empty() ? path.stem().string() : options.name;
    overlay.source = path;
    overlay.width = image->width;
    overlay.height = image->height;
    overlay.rgba = std::move(image->rgba);
    overlay.geotransform = image->geotransform;
    overlay.hasGeotransform = image->hasGeotransform;
    overlay.projectionWkt = image->projectionWkt;
    return overlay;
}

// ---- point cloud ----------------------------------------------------------

Result<PointCloudLayer> importPointCloud(const std::filesystem::path& path,
                                         const PointCloudImportOptions& options)
{
    const katana::pointcloud::PointCloudEngine engine;

    // Read the header first so the decimation step is chosen from the real
    // point count. Opening a 400-million-point file then costs the same as
    // opening a small one, which is what Phase 17 asks for.
    auto header = engine.readHeader(path);
    if (!header.ok()) {
        return header.error();
    }

    katana::pointcloud::PointCloudReadOptions readOptions;
    readOptions.classification = options.classification;
    readOptions.clip = options.clip;
    readOptions.decimationStep = katana::pointcloud::PointCloudEngine::decimationForBudget(
        header->pointCount, options.budget);
    // A ceiling as well as a step: a file whose header understates its count
    // (or a classification filter that changes the density) must still not blow
    // the budget.
    readOptions.maxPoints = options.budget;

    auto cloud = engine.read(path, readOptions);
    if (!cloud.ok()) {
        return cloud.error();
    }
    if (cloud->points.empty()) {
        return makeError(ErrorCode::InvalidArgument,
                         "no points survived the import filters", path.string());
    }

    PointCloudLayer layer;
    layer.name = options.name.empty() ? path.stem().string() : options.name;
    layer.source = path;
    layer.points = std::move(cloud->points);
    layer.bounds = cloud->bounds;
    layer.projectionWkt = cloud->projectionWkt;
    layer.sourcePointCount =
        std::max<std::uint64_t>(header->pointCount, layer.points.size());
    layer.decimationStep = readOptions.decimationStep;
    return layer;
}

Result<std::vector<Point2>> groundPointsXY(const PointCloudLayer& cloud)
{
    constexpr std::uint8_t kAsprsGround = 2;
    std::vector<Point2> points;
    points.reserve(cloud.points.size() / 2 + 1);
    for (const auto& point : cloud.points) {
        if (point.classification == kAsprsGround) {
            points.emplace_back(point.x, point.y);
        }
    }
    if (points.empty()) {
        return makeError(ErrorCode::InvalidArgument,
                         "this cloud carries no points classified as ground (ASPRS class 2)",
                         cloud.name);
    }
    return points;
}

} // namespace katana::interop
