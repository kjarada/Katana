#include "katana/interop/import.hpp"

#include "katana/interop/archive12d.hpp"

#include <sstream>

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

bool equalsIgnoringCase(const std::string& a, const std::string& b)
{
    return a.size() == b.size() &&
           std::equal(a.begin(), a.end(), b.begin(), [](char x, char y) {
               return std::tolower(static_cast<unsigned char>(x)) ==
                      std::tolower(static_cast<unsigned char>(y));
           });
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
    case SourceKind::Archive12d:
        return "12d archive";
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
    if (contains(archive12dExtensions(), extension)) {
        return SourceKind::Archive12d;
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

            // The layer THIS feature belongs on: its own layer attribute when
            // it has one and the caller has not said "put everything here".
            std::string entityLayer = targetLayer;
            if (options.targetLayer.empty() && !options.layerAttribute.empty()) {
                for (const auto& [name, value] : feature.attributes) {
                    if (equalsIgnoringCase(name, options.layerAttribute)) {
                        const std::string named = sanitizeLayerName(value);
                        // sanitizeLayerName turns a blank into "IMPORT"; a
                        // blank attribute means "no layer given", not that.
                        if (value.find_first_not_of(" \t\r\n") != std::string::npos) {
                            entityLayer = named;
                        }
                        break;
                    }
                }
            }
            if (!contains(result.layersNeeded, entityLayer)) {
                result.layersNeeded.push_back(entityLayer);
            }

            auto makeEntity = [&](katana::entity::Geometry geometry) {
                Entity entity;
                entity.geometry = std::move(geometry);
                entity.layer = entityLayer;
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
                    entity.layer = entityLayer;
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


PlacementAdvice advisePlacement(const katana::geometry::Box2& existing,
                                const katana::geometry::Box2& incoming)
{
    PlacementAdvice advice;
    if (existing.empty() || incoming.empty()) {
        return advice; // nothing to be far from
    }

    const auto diagonal = [](const katana::geometry::Box2& box) {
        return std::hypot(box.width(), box.height());
    };
    katana::geometry::Box2 combined = existing;
    combined.expand(incoming);

    const double together = diagonal(combined);
    if (!(together > 0.0) || !std::isfinite(together)) {
        return advice;
    }

    // Gap between the boxes, zero where they overlap.
    const double gapX = std::max({existing.min.x - incoming.max.x,
                                  incoming.min.x - existing.max.x, 0.0});
    const double gapY = std::max({existing.min.y - incoming.max.y,
                                  incoming.min.y - existing.max.y, 0.0});
    advice.separation = std::hypot(gapX, gapY);

    // The criterion is the symptom, not an arbitrary distance: a part is lost
    // when showing both at once shrinks it below roughly one percent of the
    // view, which is the point at which it stops being visible at all. A
    // distance threshold in metres would be wrong for a site plan and wrong
    // again for a national grid.
    //
    // But the loss must be the PLACEMENT's doing. This used to report any part
    // under 1% of the combined view, which correctly placed data that is merely
    // small always is - a single control point has no extent at all, a 10 m
    // detail inside a 2 km site is 0.5% of it - and the default button then
    // moved it to the drawing's corner (audit IO-02). So a part counts only if,
    // with the gap closed, it would plainly be visible beside the other: its
    // diagonal over kVisibleBeside of the other's, twice the invisibility line.
    // Being lost at 1% of the combined view then needs a combined view more
    // than twice the larger part's, which only a real gap produces. With the
    // margin at 1x, a part barely over the line at the other's own zoom was
    // "lost" once it overhung it by a hair: a 20 m shed 80 m off the corner of
    // a 2 km site was reported far apart (the corner case in the tests).
    //
    // The larger part always clears the margin, so a drawing hidden by a
    // distant import is reported however small the import is, one far-away
    // point included. A part with no extent is drawn as a marker at any zoom
    // and is never lost by its size.
    constexpr double kInvisibleFraction = 0.01;
    constexpr double kVisibleBeside = 2.0 * kInvisibleFraction;
    const double existingSize = diagonal(existing);
    const double incomingSize = diagonal(incoming);
    const auto lostByPlacement = [&](double size, double otherSize) {
        return size > 0.0 && size <= together * kInvisibleFraction &&
               size > otherSize * kVisibleBeside;
    };
    if (!lostByPlacement(existingSize, incomingSize) &&
        !lostByPlacement(incomingSize, existingSize)) {
        return advice;
    }

    advice.farApart = true;
    // Bring the incoming data's lower-left corner to the drawing's, which keeps
    // the imported geometry's own shape and internal coordinates exactly as they
    // were and moves it as one piece.
    advice.suggestedShift =
        katana::geometry::Vec2(incoming.min.x - existing.min.x, incoming.min.y - existing.min.y);

    std::ostringstream out;
    out.precision(1);
    out << std::fixed << "the imported data sits about " << advice.separation
        << " units from the existing drawing, so at a zoom that shows both, one of them is "
           "smaller than a pixel";
    advice.message = out.str();
    return advice;
}

} // namespace katana::interop
