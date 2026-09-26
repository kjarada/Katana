#pragma once

// What a file holds, read WITHOUT importing it (PLAN.MD Phase 20) - the
// gdalinfo / pdal info a person needs before choosing what to import and how:
// which layer of a GeoPackage and what fields it has, whether a DEM is
// georeferenced and what its no-data value is, how many points a LAS holds
// and whether it is COPC.
//
// One description, used by the desktop application's import dialogs, by the
// INFO verb's records (src/katana_app/geo/info_verb.cpp) and, through INFO,
// by GIS > Dataset Information, so none of them can disagree about what a
// file is. A GDAL dataset is described from GDAL's own info JSON - `raster
// info` and `vector info` through the geoprocessing bridge
// (gis/processing.hpp) - kept verbatim beside the description, so INFO ...
// JSON and the records are one reading. GDAL opens a /vsi path or a URL
// itself, so those are described too. A point cloud is PDAL's.

#include <cstdint>
#include <filesystem>
#include <optional>
#include <string>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/geometry/primitives2d.hpp"
#include "katana/interop/import.hpp"

namespace katana::interop {

// One band of a raster. Absent is not zero: a band with no no-data value, or
// whose statistics were never computed, says so.
struct BandDescription {
    int band = 0;               // 1-based, as GDAL numbers them
    std::string dataType;       // GDAL's name: Byte, Int16, Float32 ...
    std::string colour;         // GDAL's colour interpretation: Gray, Red, Undefined ...
    std::optional<double> noData;
    // From statistics the file carries, or computed when asked
    // (DescribeOptions::statistics).
    std::optional<double> min, max, mean, stdDev;
    // Each overview's width and height, largest first.
    std::vector<std::pair<int, int>> overviews;
};

struct RasterDescription {
    int width = 0;
    int height = 0;
    int bandCount = 0;
    bool georeferenced = false;
    // World units per pixel along the image's own columns and rows: the
    // lengths of the geotransform's column and row vectors, so a rotated
    // image reports its true pixel size. Meaningful only when georeferenced.
    double pixelWidth = 0.0;
    double pixelHeight = 0.0;
    // All four corners through the full affine. Empty when not georeferenced:
    // GDAL's default transform would put it at the origin, which is a guess.
    katana::geometry::Box2 bounds;
    std::optional<double> noDataValue; // band 1's
    std::vector<BandDescription> bands{};
};

// One attribute field of a vector layer, as GDAL types it.
struct FieldDescription {
    std::string name;
    std::string type;    // GDAL's: String, Integer, Integer64, Real, Date, DateTime ...
    std::string subtype; // Boolean, Int16, Float32, JSON, UUID; empty for none
    int width = 0;       // 0 when the format sets none
    int precision = 0;
};

struct VectorLayerDescription {
    std::string name;
    std::uint64_t featureCount = 0;
    std::string geometryType; // GDAL's spelling, e.g. "Polygon"
    std::string crs;          // gis::describeCrs of the layer's own CRS; empty when none
    katana::geometry::Box2 extent{}; // empty when the layer has none
    std::vector<FieldDescription> fields{};
};

// A dataset inside a container (a netCDF variable, a GeoPackage raster
// table): what `IMPORT ... subdataset=` names.
struct SubdatasetDescription {
    std::string name;        // what GDAL opens it by: NETCDF:"f.nc":elevation
    std::string description; // for a person
};

struct PointCloudDescription {
    std::uint64_t pointCount = 0;
    katana::pointcloud::PointCloudBounds bounds;
    bool hasColor = false;
    // A Cloud Optimised Point Cloud, which can answer a resolution query
    // (PointCloudImportOptions::resolution) instead of being decimated.
    bool copc = false;
};

struct SourceDescription {
    std::filesystem::path path;
    SourceKind kind = SourceKind::Unknown;
    // GDAL's driver short name ("GTiff", "GPKG"), or the PDAL reader
    // ("readers.las", "readers.copc") for a point cloud.
    std::string driver;
    // gis::describeCrs of the dataset's CRS; empty when the file declares none.
    std::string crs;
    std::string projectionWkt;
    // A GeoPackage can hold rasters and vector layers at once, so these are
    // filled from what the dataset actually contains, not from its kind.
    std::optional<RasterDescription> raster;
    std::vector<VectorLayerDescription> vectorLayers;
    std::vector<SubdatasetDescription> subdatasets{};
    std::optional<PointCloudDescription> pointCloud;
    // GDAL's own info JSON, verbatim, for what it described: `raster info`,
    // `vector info` and - DescribeOptions::multidim, for a multidimensional
    // format - `mdim info`. Empty for what it is not.
    std::string rasterJson{}, vectorJson{}, multidimJson{};
};

struct DescribeOptions {
    // Every band's minimum, maximum, mean and standard deviation, computed
    // from every pixel - no .aux.xml is written beside the file for it (the
    // bridge's rule, docs/geoprocessing.md "Sidecars").
    bool statistics = false;
    // Only this vector layer; empty for every layer. NotFound-like failure
    // from GDAL for a layer the file does not have.
    std::string layer;
    // A multidimensional format's `mdim info` JSON as well.
    bool multidim = false;
    // GDAL is asked whatever the extension: INFO says what GDAL reads (a
    // netCDF, a FlatGeobuf), where an import dialog asks only of what
    // Katana imports.
    bool anyFormat = false;
};

// NotFound for a missing file (a /vsi path or a URL is GDAL's to find);
// Unsupported for an extension no importer claims (unless
// DescribeOptions::anyFormat), and for a 12d archive,
// which is described by importing it (its header says nothing its contents do
// not); FileImportFailure when GDAL or PDAL cannot read it.
[[nodiscard]] katana::core::Result<SourceDescription>
describeSource(const std::filesystem::path& path, const DescribeOptions& options = {});

// Whether `path` names something GDAL opens rather than a file on this disk:
// a /vsi path (/vsizip/, /vsicurl/ ...) or a URL.
[[nodiscard]] bool isVirtualPath(const std::filesystem::path& path);

// A multi-line summary for a person, in a fixed order: kind and driver, the
// CRS (or that there is none), then the raster, the layers or the cloud.
// Coordinates are printed to 3 decimals - the millimetre, the resolution
// survey coordinates are quoted to - and counts with thousands separators.
[[nodiscard]] std::string formatDescription(const SourceDescription& description);

} // namespace katana::interop
