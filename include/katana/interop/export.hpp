#pragma once

// Export: the internal domain model -> external files (PLAN.MD Phase 20).
//
// The inverse of import.hpp. The model is read, never mutated.

#include <cstdint>
#include <filesystem>
#include <optional>
#include <string>
#include <vector>

#include "katana/entity/model.hpp"
#include "katana/geometry/primitives2d.hpp"
#include "katana/interop/reference_data.hpp"

namespace katana::interop {

struct VectorExportOptions {
    // Empty exports the whole model; otherwise only these entities.
    std::vector<katana::entity::EntityId> entities;
    // Empty exports every layer; otherwise only entities on these layers.
    std::vector<std::string> layers;

    std::string layerName = "katana";
    // Empty: inferred from the path extension.
    std::string driver;
    // The drawing's coordinate system (anything GDAL reads: "EPSG:7856", WKT),
    // written into the file. A format that holds only longitude and latitude
    // (KML, KMZ, GPX) is written in them, converted from this; without it
    // such an export is refused with InvalidCRS, since GDAL would write its
    // placemarks without their geometry and report success.
    std::string projectionWkt;

    // Added back to every coordinate, undoing an import's originShift.
    std::optional<katana::geometry::Vec2> originShift;

    // Write entity properties as feature attributes.
    bool propertiesAsAttributes = true;
    // Curves have no exact representation in most vector formats, so arcs and
    // circles are written as polylines. This is the sagitta of the chords used,
    // in model units: the largest distance the polyline may deviate from the
    // true curve. Stated explicitly rather than hidden, because it is a lossy
    // conversion and the user is entitled to choose its accuracy.
    double curveTolerance = 0.001;
};

struct VectorExportResult {
    std::uint64_t featuresWritten = 0;
    std::uint64_t entitiesSkipped = 0;
    std::string driver;
    std::vector<std::string> warnings;
};

// Entity types with no vector-format counterpart (text, dimensions) are skipped
// and counted, never silently dropped. The entities go through the one
// conversion to features (interop/geo/drawing_dataset.hpp): properties keep
// their types, a ring IMPORT tagged as a hole is written as its area's hole,
// and the file is written by GdalDataset::writeTables, which meets each
// format's needs (docs/interop.md, "Fidelity").
[[nodiscard]] katana::core::Result<VectorExportResult>
exportVector(const katana::entity::Model& model, const std::filesystem::path& path,
             const VectorExportOptions& options = {});

[[nodiscard]] katana::core::Status exportPointCloud(const PointCloudLayer& cloud,
                                                    const std::filesystem::path& path);

// Formats offered in the save dialog, as (description, extension) pairs.
struct FormatChoice {
    std::string description;
    std::string extension; // without the dot
};

[[nodiscard]] std::vector<FormatChoice> vectorExportFormats();
// What exportSurfaceRaster (terrain_io.hpp) writes: GeoTIFF, Esri ASCII grid,
// Erdas Imagine.
[[nodiscard]] std::vector<FormatChoice> rasterExportFormats();
// What exportPointCloud writes: LAS and LAZ. COPC is a conversion of a FILE,
// not an export of the sample held in memory - see
// PointCloudEngine::convertToCopc - so it is not offered here.
[[nodiscard]] std::vector<FormatChoice> pointCloudExportFormats();

} // namespace katana::interop
