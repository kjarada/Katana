#pragma once

// GDAL raster and vector access, behind a Katana interface (Rule 4).
//
// Nothing here names a GDAL type: geometry comes back as plain coordinate
// arrays and rasters as 8-bit RGBA, so GDAL stays inside gdal_adapter.cpp and
// the rest of the program cannot accidentally depend on it.
//
// This layer is deliberately format-agnostic and domain-agnostic. It does not
// know what an entity is; converting to and from the domain model is the job of
// katana::interop (PLAN.MD Phase 20: "All importers should convert external
// data into the internal domain model").
//
// Threading: GDAL's driver manager is process-global and is registered once,
// under a mutex. A GdalDataset itself is not thread-safe; use one per thread.

#include <array>
#include <cstdint>
#include <filesystem>
#include <map>
#include <memory>
#include <optional>
#include <string>
#include <vector>

#include "katana/core/error.hpp"

namespace katana::gis {

// ---- raster ---------------------------------------------------------------

struct RasterInfo {
    int width = 0;
    int height = 0;
    int bandCount = 0;
    std::string projectionWkt;
    // Affine pixel -> world mapping, GDAL order:
    //   world.x = gt[0] + px*gt[1] + py*gt[2]
    //   world.y = gt[3] + px*gt[4] + py*gt[5]
    std::array<double, 6> geotransform{0.0, 1.0, 0.0, 0.0, 0.0, -1.0};
    // False when the file carries no georeferencing, in which case the
    // geotransform above is GDAL's default and places the image at the origin
    // one world unit per pixel. Callers that need real coordinates must check.
    bool hasGeotransform = false;
};

// A decimated 8-bit RGBA copy of a raster, ready to hand to a painter. Decimated
// on read by GDAL itself, so a 2 GB GeoTIFF never needs to be fully resident.
struct RasterImage {
    int width = 0;
    int height = 0;
    std::vector<std::uint8_t> rgba; // width * height * 4, row major, top row first
    // Geotransform of THIS image, already adjusted for the decimation, so it
    // maps the decimated pixel grid to world coordinates directly.
    std::array<double, 6> geotransform{0.0, 1.0, 0.0, 0.0, 0.0, -1.0};
    bool hasGeotransform = false;
    std::string projectionWkt;
};

// ---- vector ---------------------------------------------------------------

struct GeoPoint {
    double x = 0.0;
    double y = 0.0;
    double z = 0.0;
};

enum class GeometryKind {
    Unknown,
    Point,
    LineString,
    Polygon, // parts[0] is the exterior ring; any further parts are holes
};

// One geometry as coordinate arrays. A multi-geometry is delivered as several
// VectorGeometry values (one per member) rather than a nested structure, which
// keeps the consumer simple: every geometry has exactly one kind.
struct VectorGeometry {
    GeometryKind kind = GeometryKind::Unknown;
    std::vector<std::vector<GeoPoint>> parts;
};

struct VectorFeature {
    VectorGeometry geometry;
    // Ordered so that exported field order is deterministic (Rule 7).
    std::map<std::string, std::string> attributes;
};

struct VectorLayerInfo {
    std::string name;
    std::uint64_t featureCount = 0;
    std::string projectionWkt;
    std::string geometryType; // GDAL's spelling, e.g. "Polygon", for display
};

// ---- export options -------------------------------------------------------

struct RasterExportOptions {
    int width = 0;
    int height = 0;
    std::string driver = "GTiff";
    std::string projectionWkt;
    std::array<double, 6> geotransform{0.0, 1.0, 0.0, 0.0, 0.0, -1.0};
    std::optional<double> noDataValue;
};

struct VectorExportOptions {
    // Empty means "infer from the path's extension" - .shp, .geojson, .gpkg,
    // .kml, .gml, .dxf. An explicit name overrides the extension.
    std::string driver;
    std::string layerName = "katana";
    std::string projectionWkt;
};

// ---- dataset --------------------------------------------------------------

class GdalDataset {
  public:
    // Opens a raster or vector dataset. Returns InvalidArgument when GDAL
    // cannot open the path, with GDAL's own message as context.
    [[nodiscard]] static katana::core::Result<std::unique_ptr<GdalDataset>>
    open(const std::filesystem::path& path);

    ~GdalDataset();

    GdalDataset(const GdalDataset&) = delete;
    GdalDataset& operator=(const GdalDataset&) = delete;
    GdalDataset(GdalDataset&&) = delete;
    GdalDataset& operator=(GdalDataset&&) = delete;

    [[nodiscard]] bool hasRaster() const;
    [[nodiscard]] bool hasVector() const;

    [[nodiscard]] katana::core::Result<RasterInfo> rasterInfo() const;

    // Band values as doubles, for analysis (elevation, for instance). The whole
    // band is materialised, so prefer readImage() for display.
    [[nodiscard]] katana::core::Result<std::vector<double>> readBand(int bandIndex) const;

    // A display copy, decimated so neither dimension exceeds maxPixels. Uses
    // the band colour interpretation: RGB(A) where present, a grey ramp
    // otherwise, and the palette for paletted rasters. Pixels equal to the
    // band's no-data value become transparent.
    [[nodiscard]] katana::core::Result<RasterImage> readImage(int maxPixels) const;

    [[nodiscard]] katana::core::Result<std::vector<VectorLayerInfo>> vectorLayers() const;

    // Features of one layer. maxFeatures == 0 reads them all. Geometries are
    // flattened: a MultiPolygon feature yields one VectorFeature per polygon,
    // each carrying a copy of the attributes.
    [[nodiscard]] katana::core::Result<std::vector<VectorFeature>>
    readFeatures(int layerIndex, std::uint64_t maxFeatures = 0) const;

    // ---- writing ----------------------------------------------------------

    [[nodiscard]] static katana::core::Status writeRaster(const std::filesystem::path& path,
                                                          const RasterExportOptions& options,
                                                          const std::vector<double>& values);

    [[nodiscard]] static katana::core::Status writeVector(const std::filesystem::path& path,
                                                          const std::vector<VectorFeature>& features,
                                                          const VectorExportOptions& options);

    // The driver GDAL would use for this path, or an error naming the extension
    // when none is registered for it. Exposed so a UI can reject an unsupported
    // extension before the user fills in a whole export dialog.
    [[nodiscard]] static katana::core::Result<std::string>
    vectorDriverForPath(const std::filesystem::path& path);

  private:
    GdalDataset() = default;

    void* dataset_ = nullptr;
};

// True for formats that store exactly ONE geometry type per layer, so a mixed
// collection cannot be written in a single file. Shapefile is the notable case:
// its header fixes the shape type, and GDAL only discovers the mismatch on the
// first feature of a different type - by which point a partial file exists.
[[nodiscard]] bool driverHoldsOneGeometryType(const std::string& driver);

// True for formats whose specification fixes the coordinate reference system, so
// any coordinates written are reinterpreted as that CRS regardless of what the
// data actually is. RFC 7946 pins GeoJSON to WGS 84 longitude/latitude.
// True for a format whose layers have a fixed set of fields and accept no
// others (DXF). writeVector still writes such a file - geometry, and any
// attribute the format already has a field for - but everything else is
// dropped, and a caller should say so rather than let it pass unremarked.
[[nodiscard]] bool driverHasFixedFields(const std::string& driver);
[[nodiscard]] bool driverAssumesWgs84(const std::string& driver);

// The GDAL version string, for the about box and for bug reports.
[[nodiscard]] std::string gdalVersion();

} // namespace katana::gis
