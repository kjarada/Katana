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
    // Band 1's no-data value, when the file declares one. Absent is not a
    // value: a DEM without one has every pixel meaningful.
    std::optional<double> noDataValue;
};

// One band at full precision on a regular sub-grid: sample (i, j) is SOURCE
// pixel (i * stride, j * stride), exactly as the file holds it - no
// resampling, so an elevation is a surveyed post and not an average of
// neighbours, some of which may be no-data sentinels. Row major, top row first.
struct RasterSamples {
    int columns = 0;
    int rows = 0;
    int stride = 1;
    std::vector<double> values; // columns * rows
    // The band's own no-data value, when it declares one. The caller decides
    // what a no-data sample means; this layer only reports it.
    std::optional<double> noDataValue;
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
    double z = 0.0; // meaningful only in a geometry whose hasZ is true
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
    // Whether the geometry carries heights: read from the file's own geometry,
    // and on writing, whether a 3D geometry is written. Without it a 2D source
    // read as z = 0 everywhere, indistinguishable from real heights at the
    // datum, and every export wrote a 3D geometry with z = 0 (audit IO-01).
    // Absent is not zero.
    bool hasZ = false;
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
    // Any driver GDAL can write, including those that can only COPY a
    // finished dataset rather than create one (AAIGrid): writeRaster builds
    // those in memory first and copies, so the caller need not know which
    // kind a format is.
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

    // The short name of the driver that opened the dataset: "GTiff",
    // "ESRI Shapefile", "GPKG". For display and for bug reports.
    [[nodiscard]] std::string driverName() const;

    // Band values as doubles, for analysis (elevation, for instance). The whole
    // band is materialised, so prefer readImage() for display.
    [[nodiscard]] katana::core::Result<std::vector<double>> readBand(int bandIndex) const;

    // Band values on every `stride`-th pixel of every `stride`-th row, read
    // one source row at a time so a 2 GB DEM never needs to be resident: the
    // memory is one row plus the samples kept. InvalidArgument for a band out
    // of range or a stride below 1.
    [[nodiscard]] katana::core::Result<RasterSamples> readBandSampled(int bandIndex,
                                                                      int stride) const;

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

    // The same for a raster written by writeRaster: .tif/.tiff -> GTiff,
    // .asc -> AAIGrid, .img -> HFA. Unsupported, naming the extension, for
    // anything else.
    [[nodiscard]] static katana::core::Result<std::string>
    rasterDriverForPath(const std::filesystem::path& path);

  private:
    GdalDataset() = default;

    void* dataset_ = nullptr;
};

// True for formats that store exactly ONE geometry type per layer, so a mixed
// collection cannot be written in a single file. Shapefile is the notable case:
// its header fixes the shape type, and GDAL only discovers the mismatch on the
// first feature of a different type - by which point a partial file exists.
[[nodiscard]] bool driverHoldsOneGeometryType(const std::string& driver);

// True for a format whose layers have a fixed set of fields and accept no
// others (DXF). writeVector still writes such a file - geometry, and any
// attribute the format already has a field for - but everything else is
// dropped, and a caller should say so rather than let it pass unremarked.
[[nodiscard]] bool driverHasFixedFields(const std::string& driver);

// True for formats whose specification fixes the coordinate reference system, so
// any coordinates written are reinterpreted as that CRS regardless of what the
// data actually is. RFC 7946 pins GeoJSON to WGS 84 longitude/latitude.
[[nodiscard]] bool driverAssumesWgs84(const std::string& driver);

// A coordinate system for a person to read: its name, and its authority code
// where GDAL can identify one - "GDA94 / MGA zone 56 (EPSG:28356)". Parsed by
// GDAL's own OGRSpatialReference, which reads WKT1, WKT2 and PROJJSON alike,
// rather than by picking at the text. Empty for an empty WKT; a WKT GDAL
// cannot parse is described as such, never as blank, because "no CRS" and "a
// CRS we could not read" are different things to the person importing it.
[[nodiscard]] std::string describeCrs(const std::string& wkt);

// The GDAL version string, for the about box and for bug reports.
[[nodiscard]] std::string gdalVersion();

} // namespace katana::gis
