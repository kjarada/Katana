#include "katana/interop/import.hpp"

#include "katana/interop/archive12d.hpp"

#include <sstream>

#include <algorithm>
#include <cmath>
#include <limits>
#include <map>
#include <string>
#include <system_error>
#include <variant>

#include "katana/core/text.hpp"
#include "katana/entity/entity_geometry.hpp"
#include "katana/gis/formats.hpp"
#include "katana/gis/processing.hpp"
#include "katana/interop/geo/drawing_dataset.hpp"

namespace katana::interop {
namespace {

namespace geo = katana::interop::geo;
namespace gp = katana::gis::processing;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::entity::Entity;

std::string lowerExtension(const std::filesystem::path& path)
{
    std::string extension = path.extension().string();
    if (!extension.empty() && extension.front() == '.') {
        extension.erase(extension.begin());
    }
    return katana::core::lowered(extension); // ASCII only, whatever the locale
}

bool contains(const std::vector<std::string>& values, const std::string& value)
{
    return std::find(values.begin(), values.end(), value) != values.end();
}

// The extensions GDAL reads data of one kind from, less those another
// importer takes by its extension first (kindForPath): PDAL's point clouds -
// GDAL claims .e57 for the images inside one - and the .12da archives.
std::vector<std::string> gdalExtensions(katana::gis::DataKind kind)
{
    std::vector<std::string> extensions;
    for (std::string& extension : katana::gis::readableExtensions(kind)) {
        if (!contains(pointCloudExtensions(), extension) &&
            !contains(archive12dExtensions(), extension)) {
            extensions.push_back(std::move(extension));
        }
    }
    return extensions;
}

// Where a name alone must decide - a URL, a file not written yet - an
// extension only raster readers claim is a raster, one only vector readers
// claim is vector data. One claimed by both is vector data (a GeoPackage's
// features are drawing data, its tiles a backdrop), except for the formats
// that are chiefly imagery or grids.
SourceKind kindByExtension(const std::string& extension)
{
    if (extension.empty()) {
        return SourceKind::Unknown;
    }
    constexpr const char* kChieflyRaster[] = {"mbtiles", "pdf", "jp2", "j2k", "nc",
                                              "pix",     "fits", "bag"};
    bool raster = false;
    bool vector = false;
    for (const katana::gis::Format& format : katana::gis::formats()) {
        if (contains(format.extensions, extension)) {
            raster = raster || format.readRaster;
            vector = vector || format.readVector;
        }
    }
    if (raster && vector) {
        return std::ranges::any_of(kChieflyRaster,
                                   [&extension](const char* name) { return extension == name; })
                   ? SourceKind::Raster
                   : SourceKind::Vector;
    }
    if (raster) {
        return SourceKind::Raster;
    }
    return vector ? SourceKind::Vector : SourceKind::Unknown;
}

// Converts a source layer name into something usable as a Katana layer name.
// Keeps it recognisable rather than mangling it: only characters that would be
// confusing in a layer list are replaced.
std::string sanitizeLayerName(std::string name)
{
    for (char& ch : name) {
        // The ASCII control characters, tab and newlines among them, and DEL.
        // Tested by value rather than with std::iscntrl, whose answer for a
        // byte above 0x7F depends on the process locale.
        const auto value = static_cast<unsigned char>(ch);
        if (value < 0x20 || value == 0x7F) {
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

// A field's value as a property: absent is no property, not an empty one.
std::optional<katana::entity::PropertyValue> propertyOf(const gp::FieldValue& value)
{
    return std::visit(
        [](const auto& held) -> std::optional<katana::entity::PropertyValue> {
            using Held = std::decay_t<decltype(held)>;
            if constexpr (std::is_same_v<Held, std::monostate>) {
                return std::nullopt;
            } else {
                return katana::entity::PropertyValue(held);
            }
        },
        value);
}

// The box around a feature's points, in the file's own coordinates: what the
// online import's area is tested against.
katana::gis::CrsBox boxOf(const gp::Feature& feature)
{
    katana::gis::CrsBox box{std::numeric_limits<double>::infinity(),
                            std::numeric_limits<double>::infinity(),
                            -std::numeric_limits<double>::infinity(),
                            -std::numeric_limits<double>::infinity()};
    for (const katana::gis::VectorGeometry& part : feature.parts) {
        for (const auto& ring : part.parts) {
            for (const katana::gis::GeoPoint& point : ring) {
                box.minX = std::min(box.minX, point.x);
                box.minY = std::min(box.minY, point.y);
                box.maxX = std::max(box.maxX, point.x);
                box.maxY = std::max(box.maxY, point.y);
            }
        }
    }
    return box;
}

// Every part of every feature moved from one CRS to another, by the one
// reprojection (gis::reprojectFeatures), heights untouched. An arc's three
// points move with the rest; a conformal projection keeps an arc that small
// round to far below a millimetre, and no web service sends arcs.
katana::core::Status reprojectTable(gp::FeatureTable& table, const std::string& from,
                                    const std::string& to)
{
    std::vector<katana::gis::VectorFeature> geometries;
    for (const gp::Feature& feature : table.features) {
        for (const katana::gis::VectorGeometry& part : feature.parts) {
            geometries.push_back(katana::gis::VectorFeature{part, {}});
        }
    }
    if (geometries.empty()) {
        return {};
    }
    auto moved = katana::gis::reprojectFeatures(std::move(geometries), from, to);
    if (!moved) {
        return moved.error();
    }
    std::size_t next = 0;
    for (gp::Feature& feature : table.features) {
        for (katana::gis::VectorGeometry& part : feature.parts) {
            part = std::move((*moved)[next++].geometry);
        }
    }
    return {};
}

// "1 TIN", "3 polyhedral surfaces": what a read left out, for a warning.
std::string skippedGeometry(const std::string& what, std::uint64_t count)
{
    if (what == "tin") {
        return std::to_string(count) + (count == 1 ? " TIN" : " TINs");
    }
    return std::to_string(count) + " " + what + (count == 1 ? "" : " geometries");
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
    return gdalExtensions(katana::gis::DataKind::Vector);
}

std::vector<std::string> rasterExtensions()
{
    return gdalExtensions(katana::gis::DataKind::Raster);
}

std::vector<std::string> archiveExtensions()
{
    return {"zip", "tar", "tgz", "gz"};
}

std::vector<std::string> pointCloudExtensions()
{
    return {"las", "laz", "copc", "bpf", "ply", "pcd", "e57"};
}

SourceKind kindForPath(const std::filesystem::path& path)
{
    const std::string extension = lowerExtension(path);
    if (contains(archive12dExtensions(), extension)) {
        return SourceKind::Archive12d;
    }
    // Point cloud first: PDAL and GDAL both claim .e57 (GDAL, the images in
    // one), and a .ply in a survey context is a point cloud far more often
    // than it is anything else.
    if (contains(pointCloudExtensions(), extension)) {
        return SourceKind::PointCloud;
    }
    // What the file holds, where GDAL can look at it: a GeoPackage of raster
    // tiles is a raster, a .zip of a shapefile vector data, a file whose
    // extension no reader claims whatever GDAL finds in it. A web address is
    // looked at only when its name says nothing: a look costs a round trip.
    const bool remote = katana::gis::isRemotePath(path);
    const SourceKind named = kindByExtension(extension);
    std::error_code ignored;
    const bool local = !remote && (katana::gis::isVirtualPath(path) ||
                                   std::filesystem::exists(path, ignored));
    if (local || (remote && named == SourceKind::Unknown)) {
        if (const auto content = katana::gis::identifyContent(path); content.ok()) {
            if (content->raster != content->vector) {
                return content->raster ? SourceKind::Raster : SourceKind::Vector;
            }
            if (content->raster) {
                // Both: the name's leaning, vector data where it has none.
                return named == SourceKind::Raster ? SourceKind::Raster : SourceKind::Vector;
            }
        }
    }
    // A file not there yet, or one GDAL cannot open: by its name, so the
    // import that follows fails with GDAL's own reason, not "no importer".
    return named;
}

// ---- vector ---------------------------------------------------------------

Result<VectorImportResult> importVector(const std::filesystem::path& path,
                                        const VectorImportOptions& options)
{
    if (!(options.curveTolerance > 0.0) || !std::isfinite(options.curveTolerance)) {
        return makeError(ErrorCode::InvalidArgument, "the curve tolerance must be positive",
                         katana::core::formatExactReal(options.curveTolerance));
    }
    auto dataset = katana::gis::GdalDataset::open(path);
    if (!dataset.ok()) {
        return dataset.error();
    }
    // A CSV's geometry columns (WKT, or X, Y and Z) are its geometry, not
    // fields as well: kept, each entity would carry its own coordinates as a
    // property that goes stale the moment it is moved. Z is a height when the
    // file says X and Y are coordinates (its .csvt, as EXPORT writes one).
    if ((*dataset)->driverName() == "CSV") {
        dataset = katana::gis::GdalDataset::open(path, {"KEEP_GEOM_COLUMNS=NO",
                                                        "Z_POSSIBLE_NAMES=Z"});
        if (!dataset.ok()) {
            return dataset.error();
        }
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
    if (!options.targetCrs.empty()) {
        // Every coordinate below is in the target, so that is what the
        // result declares.
        auto target = katana::gis::crsToWkt(options.targetCrs);
        if (!target) {
            return target.error();
        }
        result.projectionWkt = std::move(*target);
    }

    const std::string sourceName = path.filename().string();
    std::uint64_t remaining = options.maxFeatures;
    std::size_t heightAttributesReplaced = 0;
    geo::FeaturePieceCounts counts;
    std::map<std::string, std::uint64_t> skippedGeometries;
    const geo::FeaturePieceOptions pieceOptions{options.originShift, options.curveTolerance};

    // A source's Z becomes the heights every other part of Katana reads
    // (entity.hpp). A feature without one gets no height property at all.
    const auto applyHeights = [&](Entity& entity, const std::vector<std::optional<double>>& heights) {
        if (std::none_of(heights.begin(), heights.end(),
                         [](const auto& z) { return z.has_value(); })) {
            // A 2D feature's "elevation" attribute that is a number IS its
            // height: it is how exportVector writes the heights a 2D layer
            // cannot hold in its geometry (a shapefile mixing entities with
            // and without them), so a round trip keeps them. A typed field
            // arrives as a number already; a format without types (a CSV
            // without its .csvt) gives text, read here. The list form,
            // "elevations", is text anyway.
            const auto found = entity.properties.find(katana::entity::kElevationProperty);
            if (found != entity.properties.end()) {
                if (const auto* text = std::get_if<std::string>(&found->second)) {
                    if (const auto z =
                            katana::core::parseFiniteDouble(katana::core::trimmed(*text))) {
                        found->second = *z;
                    }
                }
            }
            return;
        }
        // An ATTRIBUTE named "elevation" is taken by a height here: those two
        // names are what the rest of the application reads a height from.
        if (entity.properties.contains(katana::entity::kElevationProperty) ||
            entity.properties.contains(katana::entity::kElevationsProperty)) {
            ++heightAttributesReplaced;
        }
        katana::entity::setHeights(entity.properties, heights);
    };

    // GDAL reads a GPX as five layers, two of which - route_points and
    // track_points - are the vertices of its routes and tracks again, one
    // point each: imported with the rest, every line came with a point on
    // each of its vertices. They are left out of a whole-file import, and
    // said; named by index, either is imported as asked.
    const bool gpx = (*dataset)->driverName() == "GPX";
    std::vector<std::string> restated;

    for (int index = firstLayer; index <= lastLayer; ++index) {
        const auto& info = (*layers)[static_cast<std::size_t>(index)];
        if (gpx && options.sourceLayerIndex < 0 &&
            (info.name == "route_points" || info.name == "track_points")) {
            if (info.featureCount > 0) {
                restated.push_back(info.name);
            }
            continue;
        }

        const std::string targetLayer =
            options.targetLayer.empty()
                ? sanitizeLayerName(info.name.empty() ? path.stem().string() : info.name)
                : options.targetLayer;
        if (options.targetCrs.empty() && !info.projectionWkt.empty() &&
            !result.projectionWkt.empty() && info.projectionWkt != result.projectionWkt) {
            result.warnings.push_back("layer '" + info.name +
                                      "' declares a different coordinate reference system from "
                                      "the first layer; coordinates are imported unchanged");
        }

        // The one read (gdal_adapter.hpp): typed fields, and curves kept as
        // arcs for featurePieces to make Katana's arcs and circles of.
        katana::gis::VectorReadOptions readOptions;
        readOptions.maxFeatures = remaining;
        readOptions.keepArcs = true;
        katana::gis::VectorReadReport report;
        auto table = (*dataset)->readTable(index, readOptions, &report);
        if (!table.ok()) {
            return table.error();
        }
        for (const auto& [what, count] : report.skipped) {
            if (what != "no geometry") {
                skippedGeometries[what] += count;
            }
        }
        for (std::string& warning : report.warnings) {
            result.warnings.push_back(std::move(warning));
        }
        if (options.sourceFilter) {
            const katana::gis::CrsBox& area = *options.sourceFilter;
            std::erase_if(table->features, [&](const gp::Feature& feature) {
                const katana::gis::CrsBox box = boxOf(feature);
                return box.maxX < area.minX || box.minX > area.maxX || box.maxY < area.minY ||
                       box.minY > area.maxY;
            });
        }
        if (!options.targetCrs.empty() && !table->features.empty()) {
            const std::string& source =
                info.projectionWkt.empty() ? options.assumedSourceCrs : info.projectionWkt;
            if (source.empty()) {
                return makeError(ErrorCode::InvalidCRS,
                                 "the layer declares no coordinate system, so it cannot be moved "
                                 "into the project's",
                                 info.name);
            }
            if (auto moved = reprojectTable(*table, source, options.targetCrs); !moved) {
                return moved.error();
            }
        }

        // The field that names each entity's layer: `layer` by default, which
        // is what Katana's own exports write and, matched case-insensitively,
        // DXF's `Layer` too.
        std::optional<std::size_t> layerField;
        if (options.targetLayer.empty() && !options.layerAttribute.empty()) {
            for (std::size_t f = 0; f < table->fields.size(); ++f) {
                if (katana::core::equalsIgnoringCase(table->fields[f].name,
                                                     options.layerAttribute)) {
                    layerField = f;
                    break;
                }
            }
        }

        std::uint64_t number = 0;
        for (const gp::Feature& feature : table->features) {
            ++result.featuresRead;
            ++number;

            // The layer THIS feature belongs on: its own layer attribute when
            // it has one and the caller has not said "put everything here".
            std::string entityLayer = targetLayer;
            if (layerField && *layerField < feature.values.size()) {
                if (const auto value = propertyOf(feature.values[*layerField])) {
                    const std::string named = katana::entity::toString(*value);
                    // sanitizeLayerName turns a blank into "IMPORT"; a blank
                    // attribute means "no layer given", not that.
                    if (named.find_first_not_of(" \t\r\n") != std::string::npos) {
                        entityLayer = sanitizeLayerName(named);
                    }
                }
            }

            katana::entity::PropertyMap properties;
            if (options.attributesAsProperties) {
                for (std::size_t f = 0; f < table->fields.size() && f < feature.values.size();
                     ++f) {
                    if (auto value = propertyOf(feature.values[f])) {
                        properties.emplace(table->fields[f].name, std::move(*value));
                    }
                }
            }

            // Which feature a ring came from, so a hole joins its own exterior
            // when the drawing is read as features again (drawing_dataset.hpp):
            // the file, the layer, the feature and the part.
            const std::string featureKey =
                sourceName + "|" + info.name + "#" + std::to_string(number);
            bool wroteAny = false;
            for (std::size_t p = 0; p < feature.parts.size(); ++p) {
                std::vector<geo::FeaturePiece> pieces =
                    geo::featurePieces(feature.parts[p], pieceOptions, counts);
                for (geo::FeaturePiece& piece : pieces) {
                    Entity entity;
                    entity.geometry = std::move(piece.geometry);
                    entity.layer = entityLayer;
                    entity.properties = properties;
                    applyHeights(entity, piece.heights);
                    entity.metadata.emplace("source.file", sourceName);
                    entity.metadata.emplace("source.layer", info.name);
                    if (piece.ring) {
                        // The entity model has no polygon-with-holes type, so
                        // each ring is its own closed polyline, and its role
                        // is recorded so the area can be put together again.
                        entity.metadata.emplace("source.ring", *piece.ring);
                        entity.metadata.emplace("source.part",
                                                featureKey + "." + std::to_string(p + 1));
                    }
                    result.bounds.expand(katana::entity::boundingBox(entity.geometry));
                    if (!contains(result.layersNeeded, entityLayer)) {
                        result.layersNeeded.push_back(entityLayer);
                    }
                    result.entities.push_back(std::move(entity));
                    wroteAny = true;
                }
            }
            if (!wroteAny) {
                ++result.featuresSkipped;
            }
        }

        if (options.maxFeatures != 0) {
            if (report.featuresRead >= remaining) {
                if (index < lastLayer) {
                    result.warnings.push_back(
                        "the feature limit was reached; later layers were not read");
                }
                break;
            }
            remaining -= report.featuresRead;
        }
    }

    for (const auto& [what, count] : skippedGeometries) {
        result.skipped[what] += count;
    }
    if (result.entities.empty()) {
        return makeError(ErrorCode::InvalidArgument,
                         "no geometry in this file could be imported",
                         std::to_string(result.featuresRead) + " features read, " +
                             std::to_string(result.featuresSkipped) + " skipped");
    }
    if (result.featuresSkipped > 0) {
        result.warnings.push_back(std::to_string(result.featuresSkipped) +
                                  " features were skipped: empty or degenerate geometry, or only "
                                  "geometry Katana has no entity for");
    }
    for (const auto& [what, count] : skippedGeometries) {
        result.warnings.push_back(
            skippedGeometry(what, count) + " left out: " +
            (what == "tin" || what == "polyhedral surface"
                 ? std::string("Katana has no entity that is a surface of faces")
                 : std::string("Katana has no entity for it")));
    }
    if (counts.curvesMadeChords > 0) {
        result.warnings.push_back(
            std::to_string(counts.curvesMadeChords) +
            " arcs within curves that are not one arc or circle were made chords within " +
            katana::core::formatExactReal(options.curveTolerance) +
            " units: Katana has no entity for a line of several arcs");
    }
    if (counts.heightsLost > 0) {
        result.warnings.push_back(
            std::to_string(counts.heightsLost) +
            " vertices were dropped for standing on the vertex before them in plan, and their "
            "different heights went with them: a vertical step cannot be drawn in plan");
    }
    if (heightAttributesReplaced > 0) {
        result.warnings.push_back(
            std::to_string(heightAttributesReplaced) +
            " features had an attribute named \"elevation\" or \"elevations\", which is "
            "where Katana keeps a height; the geometry's own Z replaced it");
    }
    for (const std::string& name : restated) {
        result.warnings.push_back(
            "GPX layer '" + name + "' left out: its points are the vertices of the file's " +
            (name == "route_points" ? "routes" : "tracks") +
            " again, one point each; import that layer by its index for them");
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
    if (options.resolution.has_value()) {
        // The COPC octree is the level of detail: decimating its answer as
        // well would thin a sample that is already even, by a step sized for
        // the WHOLE file. A plain LAS is refused by the engine, not quietly
        // read whole (PointCloudReadOptions::resolution).
        readOptions.resolution = options.resolution;
        readOptions.decimationStep = 1;
    } else {
        readOptions.decimationStep = katana::pointcloud::PointCloudEngine::decimationForBudget(
            header->pointCount, options.budget);
    }
    // A ceiling as well as a step: a file whose header understates its count
    // (or a classification filter that changes the density, or a resolution
    // finer than the budget allows) must still not blow the budget.
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
