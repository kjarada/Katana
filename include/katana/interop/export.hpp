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

    // ---- EXPORT's options (docs/interop.md, "Export options") ----
    // The coordinate system the file is written in (anything gis::crsToWkt
    // reads): the coordinates, in projectionWkt's system, are moved into it
    // on the way out. Empty writes them as they are. InvalidCRS when it is
    // set and projectionWkt is empty: there is nothing to move from.
    std::string targetCrs;
    // One file layer per drawing layer, named after it, in the order the
    // layers are first met (split=layer). Refused for a format of points and
    // lines only (GPX), whose layers are its own.
    bool splitByLayer = false;
    // The layers added to the file when it exists (a GeoPackage), not the
    // file replaced (gis::VectorExportOptions::append).
    bool append = false;
    // The driver's own options, KEY=VALUE (co=, lco=). The caller checks
    // them against the driver's lists (gis::checkOptions).
    std::vector<std::string> creationOptions;
    std::vector<std::string> layerCreationOptions;
    // A text entity written as a point at its insertion point, with fields
    // text, text_height (model units) and text_rotation (degrees,
    // anticlockwise) and an OGR_STYLE LABEL that KML, DXF and MapInfo draw
    // (text=points). Off, text is left out and counted, as it always was.
    bool textAsPoints = false;
};

struct VectorExportResult {
    std::uint64_t featuresWritten = 0;
    std::uint64_t entitiesSkipped = 0;
    // Text entities written as points (textAsPoints).
    std::uint64_t textsWritten = 0;
    // The file's layers this export wrote, in order.
    std::vector<std::string> layers;
    std::string driver;
    std::vector<std::string> warnings;
    // The coordinate system the file's coordinates are in: the project's,
    // crs=<code>'s, WGS 84 for KML, KMZ and GPX, which hold nothing else, or
    // none (empty) for entities IMPORT moved from their file's coordinates.
    std::string projectionWkt;
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

// Every vector format this GDAL writes, from its registry, the common ones
// first; each extension is written by the driver EXPORT picks for it
// (gis::vectorSaveChoices, docs/interop.md "Formats").
[[nodiscard]] std::vector<FormatChoice> vectorExportFormats();
// What exportSurfaceRaster (terrain_io.hpp) writes: GeoTIFF, Esri ASCII grid,
// Erdas Imagine.
[[nodiscard]] std::vector<FormatChoice> rasterExportFormats();
// What exportPointCloud writes: LAS and LAZ. COPC is a conversion of a FILE,
// not an export of the sample held in memory - see
// PointCloudEngine::convertToCopc - so it is not offered here.
[[nodiscard]] std::vector<FormatChoice> pointCloudExportFormats();

} // namespace katana::interop
