#pragma once

// Import: external files -> the internal domain model (PLAN.MD Phase 20,
// "All importers should convert external data into the internal domain model").
//
// Nothing here writes to a Model. Vector import produces plain Entity values and
// the caller wraps them in commands::createEntities, so an import is one
// validated, atomic, undoable command like every other change. Raster and point
// cloud import produce reference data, which is not undoable by design - see
// reference_data.hpp for why.

#include <cstdint>
#include <filesystem>
#include <optional>
#include <string>
#include <vector>

#include "katana/entity/entity.hpp"
#include "katana/geometry/primitives2d.hpp"
#include "katana/interop/reference_data.hpp"

namespace katana::interop {

enum class SourceKind { Unknown, Vector, Raster, PointCloud };

[[nodiscard]] const char* toString(SourceKind kind);

// Classifies by extension alone - no file is opened. Used to route a path to
// the right importer and to build file dialog filters. An extension GDAL and
// PDAL both claim (there are a few) resolves to the more specific of the two.
[[nodiscard]] SourceKind kindForPath(const std::filesystem::path& path);

// Extensions offered in the open dialog, without the leading dot.
[[nodiscard]] std::vector<std::string> vectorExtensions();
[[nodiscard]] std::vector<std::string> rasterExtensions();
[[nodiscard]] std::vector<std::string> pointCloudExtensions();

// ---- vector ---------------------------------------------------------------

struct VectorImportOptions {
    // Katana layer the imported entities are placed on. Empty means "one layer
    // per source layer, named after it", which is what a multi-layer GeoPackage
    // should do.
    std::string targetLayer;
    // A feature attribute that names the layer each entity belongs on,
    // matched case-insensitively; used only when `targetLayer` is empty.
    //
    // A DXF is ONE source layer to GDAL ("entities"), and the CAD layer of
    // each entity arrives as its `Layer` attribute - so without this a
    // 50 000-entity drawing lands on a single layer called "entities" and the
    // layer tree is useless. Katana's own exports write the same information
    // as `layer`, which the case-insensitive match also picks up, so a round
    // trip through GeoJSON or GeoPackage keeps its layers too. A feature
    // without the attribute, or with a blank one, falls back to the source
    // layer's name. Empty disables the lookup.
    std::string layerAttribute = "layer";
    // -1 imports every layer in the dataset.
    int sourceLayerIndex = -1;
    std::uint64_t maxFeatures = 0;
    // Copy feature attributes onto the entities as string properties.
    bool attributesAsProperties = true;
    // Subtracted from every coordinate. Survey data often sits at coordinates
    // where a double has only ~0.1 mm of resolution left; shifting to a local
    // origin restores precision for downstream editing. Recorded in the result
    // so the same shift can be undone on export.
    std::optional<katana::geometry::Vec2> originShift;
};

// Whether imported data sits so far from the drawing it is joining that the two
// cannot usefully be seen together (PLAN.MD Phase 20).
//
// This is the survey case, not a corner case. A DXF or GeoPackage in a
// projected CRS carries coordinates like (255440, 7410850); a drawing started
// from scratch sits near the origin. Merging them silently produces a drawing
// whose extents span seven million metres, in which the original content is a
// dot smaller than a pixel - and nothing says so, which is the kind of quiet
// wrongness PLAN.MD section 36 forbids.
struct PlacementAdvice {
    // True when one of the two would be invisible at a zoom that shows both.
    bool farApart = false;
    // Subtract from every imported coordinate to bring it alongside the
    // drawing. Zero when they already share a neighbourhood.
    katana::geometry::Vec2 suggestedShift{};
    // Distance between the two bounding boxes, 0 when they overlap.
    double separation = 0.0;
    // Ready to show. Empty when there is nothing worth saying.
    std::string message;
};

// `existing` may be empty, which is the common case of importing into a new
// drawing; the advice is then never "far apart", because there is nothing for
// the data to be far FROM.
[[nodiscard]] PlacementAdvice advisePlacement(const katana::geometry::Box2& existing,
                                              const katana::geometry::Box2& incoming);

struct VectorImportResult {
    std::vector<katana::entity::Entity> entities;
    // Katana layers the entities reference, in first-use order. The caller must
    // create any that do not exist yet, BEFORE executing the create command, or
    // the model will reject entities on an unknown layer.
    std::vector<std::string> layersNeeded;

    katana::geometry::Box2 bounds;
    std::uint64_t featuresRead = 0;
    std::uint64_t featuresSkipped = 0;
    std::string projectionWkt;
    // Non-fatal problems: unsupported geometry types, empty geometries,
    // attributes that could not be represented. Never silently dropped
    // (PLAN.MD section 36).
    std::vector<std::string> warnings;
};

[[nodiscard]] katana::core::Result<VectorImportResult>
importVector(const std::filesystem::path& path, const VectorImportOptions& options = {});

// ---- raster ---------------------------------------------------------------

struct RasterImportOptions {
    // Neither dimension of the decimated display copy exceeds this. 4096 keeps
    // a full-screen overlay sharp while bounding memory at 64 MB of RGBA.
    int maxPixels = 4096;
    std::string name; // empty: the file stem
};

[[nodiscard]] katana::core::Result<RasterOverlay>
importRaster(const std::filesystem::path& path, const RasterImportOptions& options = {});

// ---- point cloud ----------------------------------------------------------

struct PointCloudImportOptions {
    // Points to keep. The importer reads the header first and picks a
    // decimation step that meets this budget, so opening a billion-point file
    // costs the same as opening a small one.
    std::uint64_t budget = 2'000'000;
    std::optional<std::uint8_t> classification;
    std::optional<katana::pointcloud::PointCloudBounds> clip;
    std::string name; // empty: the file stem
};

[[nodiscard]] katana::core::Result<PointCloudLayer>
importPointCloud(const std::filesystem::path& path, const PointCloudImportOptions& options = {});

// ---- point cloud -> terrain ----------------------------------------------

// Ground points (ASPRS class 2) as survey positions, ready for the TIN builder.
// Returns InvalidArgument when the cloud carries no class 2 points at all,
// because silently building a surface from vegetation returns would be worse
// than refusing.
[[nodiscard]] katana::core::Result<std::vector<katana::geometry::Point2>>
groundPointsXY(const PointCloudLayer& cloud);

} // namespace katana::interop
