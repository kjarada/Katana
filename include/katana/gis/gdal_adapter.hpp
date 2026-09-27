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
    // The circular arcs in a curved line or ring (a file's CircularString,
    // CompoundCurve or CurvePolygon), one list per part: each index i says
    // that parts[p][i], [i + 1] and [i + 2] are an arc's start, a point on
    // it and its end; the part is straight everywhere else. Empty for
    // straight geometry, which is all a reader gives unless it is asked to
    // keep arcs (VectorReadOptions::keepArcs). The middle point is NOT a
    // vertex: whoever reads a geometry with arcs draws them or makes them
    // chords, never a polyline through it, which would cut every curve.
    std::vector<std::vector<std::size_t>> arcs;

    [[nodiscard]] bool hasArcs() const
    {
        for (const auto& part : arcs) {
            if (!part.empty()) {
                return true;
            }
        }
        return false;
    }
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

// How a layer's features are read by GdalDataset::readTable.
struct VectorReadOptions {
    // 0 reads them all; otherwise at most this many of the file's features.
    std::uint64_t maxFeatures = 0;
    // A curve is kept as its arcs (VectorGeometry::arcs), for whoever turns
    // them into Katana's arcs and circles and makes the rest chords by
    // Katana's one rule - IMPORT. Otherwise every curve is chords at GDAL's
    // own step (OGR_ARC_STEPSIZE, 4 degrees), counted in the report. This
    // layer cannot make them by the rule EXPORT uses: katana_io sees only
    // core, not geometry.
    bool keepArcs = false;
    // OGR's attribute filter (OGRLayer::SetAttributeFilter): the WHERE clause
    // of an OGR SQL statement, "kind = 'lot'", read by the driver - which
    // may hand it to the database it reads, a GeoPackage's SQLite. Empty
    // reads every feature. IMPORT's where= (docs/interop.md, "Import options").
    std::string attributeFilter;
    // Only the features whose geometry meets this box - minX, minY, maxX,
    // maxY in the layer's own coordinates - by OGRLayer::SetSpatialFilterRect,
    // so a driver with a spatial index (GeoPackage, FlatGeobuf, a shapefile's
    // .qix) reads only those rather than every feature being read and then
    // thrown away. GDAL's test is the envelope's: a feature whose box meets
    // the box is kept. IMPORT's scope.
    std::optional<std::array<double, 4>> spatialFilter;
};

// What a read could not carry, said rather than dropped.
struct VectorReadReport {
    std::uint64_t featuresRead = 0; // features of the file read, with or without geometry
    // Geometry left out, by what it was: "tin", "polyhedral surface",
    // "unsupported <GDAL's type name>", "nested too deep". A feature
    // without geometry is kept, with its fields, and counted as "no geometry".
    std::map<std::string, std::uint64_t> skipped;
    std::uint64_t curvesMadeChords = 0;
    // GDAL's own warnings, raised while this read ran on this thread.
    std::vector<std::string> warnings;
};

// What a write did beside writing what it was given.
struct VectorWriteReport {
    std::uint64_t featuresWritten = 0;
    // Features with no geometry the format can hold (a line of one point).
    std::uint64_t featuresSkipped = 0;
    // Fields a format with a fixed set of fields (DXF) has nowhere to put.
    std::vector<std::string> fieldsNotWritten;
    // GDAL's own warnings (a shapefile shortening a field's name), and what
    // Katana changed on the way (coordinates converted to longitude and
    // latitude for a format that holds nothing else).
    std::vector<std::string> warnings;
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
    // Empty means "the writer for the path's name" (vectorDriverForPath):
    // .shp, .geojson, .gpkg, .kml and .kmz (LIBKML), .fgb, .parquet, .gpx,
    // .shp.zip ... An explicit name overrides it.
    std::string driver;
    std::string layerName = "katana";
    // The coordinate system of a table that names none. A format that holds
    // only longitude and latitude on WGS 84 (KML, KMZ, GPX) is written in
    // them, converted from this; given none, such a write is refused with
    // InvalidCRS rather than written without its geometry.
    std::string projectionWkt;
    // KEY=VALUE, the driver's own options, over the ones Katana chooses for a
    // format (CSV's geometry column, a MapInfo table's bounds): a key given
    // here wins.
    std::vector<std::string> creationOptions;
    std::vector<std::string> layerCreationOptions;
    // Add the tables as new layers of the file at `path` when there is one
    // (a GeoPackage of several layers, written one EXPORT at a time) rather
    // than replacing it; a file that is not there is created. EXPORT's
    // append (docs/interop.md, "Export options").
    bool append = false;
};

// ---- dataset --------------------------------------------------------------

// The typed tables of processing.hpp, which includes this header: the one
// representation of features with their fields, for the algorithms and for
// files alike.
namespace processing {
struct FeatureTable;
struct FeatureSet;
} // namespace processing

class GdalDataset {
  public:
    // Opens a raster or vector dataset: a file, a folder GDAL reads (a file
    // geodatabase), a /vsi path, a URL, or a driver's connection string
    // (formats.hpp, isVirtualPath). An archive GDAL does not open as it is -
    // a .zip of a shapefile, a .tar.gz of a GeoTIFF - is opened by its
    // inside: as a folder, or its one dataset. NotFound for a local path that
    // does not exist; FileImportFailure, with GDAL's own message as context,
    // when GDAL cannot open it; InvalidArgument, naming them, for an archive
    // of several datasets.
    [[nodiscard]] static katana::core::Result<std::unique_ptr<GdalDataset>>
    open(const std::filesystem::path& path);
    // The same with the driver's open options, KEY=VALUE.
    [[nodiscard]] static katana::core::Result<std::unique_ptr<GdalDataset>>
    open(const std::filesystem::path& path, const std::vector<std::string>& openOptions);

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
    // The same with `band` (1-based) alone, shown as grey stretched over its
    // own range - or through its colour table when it has one - whatever
    // the file says its bands are: IMPORT's band=, for a multispectral
    // image's near infrared or a stack of grids. 0 is readImage(maxPixels).
    // InvalidArgument naming the count for a band the file does not have.
    [[nodiscard]] katana::core::Result<RasterImage> readImage(int maxPixels, int band) const;

    [[nodiscard]] katana::core::Result<std::vector<VectorLayerInfo>> vectorLayers() const;

    // One layer as a typed table: its fields as the file declares them
    // (integers, reals, booleans, text; dates as ISO 8601 text), and each
    // feature's geometry as its simple parts, a multi-geometry's members in
    // one feature. The table carries the layer's name and coordinate system.
    // What the read could not carry - a TIN, a curve made chords - is counted
    // in `report`, with GDAL's warnings. InvalidArgument for a layer index out
    // of range. Callers include katana/gis/processing.hpp.
    [[nodiscard]] katana::core::Result<processing::FeatureTable>
    readTable(int layerIndex, const VectorReadOptions& options = {},
              VectorReadReport* report = nullptr) const;

    // The rows of an SQL statement run on the dataset (GDALDataset::
    // ExecuteSQL) as a typed table, as readTable reads a layer: `dialect`
    // is "OGRSQL", "SQLITE" or "" for the driver's own (a GeoPackage's is
    // SQLite). options.spatialFilter is handed to ExecuteSQL as its spatial
    // filter, options.attributeFilter set on the result. InvalidArgument,
    // with GDAL's message, for a statement GDAL refuses, and for one that
    // gives no rows as a layer (an UPDATE): IMPORT reads, it never writes to
    // the file it imports. IMPORT's sql= (docs/interop.md, "Import options").
    [[nodiscard]] katana::core::Result<processing::FeatureTable>
    readSql(const std::string& statement, const std::string& dialect,
            const VectorReadOptions& options = {}, VectorReadReport* report = nullptr) const;

    // Features of one layer, with every value as text. maxFeatures == 0 reads
    // them all. Geometries are flattened: a MultiPolygon feature yields one
    // VectorFeature per polygon, each carrying a copy of the attributes. The
    // same read as readTable, with the values written out as text.
    [[nodiscard]] katana::core::Result<std::vector<VectorFeature>>
    readFeatures(int layerIndex, std::uint64_t maxFeatures = 0) const;
    // The same, saying what it could not carry.
    [[nodiscard]] katana::core::Result<std::vector<VectorFeature>>
    readFeatures(int layerIndex, std::uint64_t maxFeatures, VectorReadReport& report) const;

    // ---- writing ----------------------------------------------------------

    [[nodiscard]] static katana::core::Status writeRaster(const std::filesystem::path& path,
                                                          const RasterExportOptions& options,
                                                          const std::vector<double>& values);

    // Every value as a text field; writeTables with one table of them.
    [[nodiscard]] static katana::core::Status writeVector(const std::filesystem::path& path,
                                                          const std::vector<VectorFeature>& features,
                                                          const VectorExportOptions& options);

    // Typed tables to a file, a layer per table, replacing the file - or,
    // with options.append, added to it: refused, before anything is written,
    // with InvalidArgument for a layer name the file has already and
    // Unsupported for a format that adds no layers to a file, and on a
    // failure part-way the layers this call made are deleted, the file's own
    // left as they were. A table's field named OGR_STYLE is also each
    // feature's OGR style string (a text's LABEL), which KML, DXF and
    // MapInfo draw by. A
    // single table's layer is named `options.layerName` (when given), several
    // are named after their tables. Fields keep their types where the format
    // has them, and the next type it has where it does not (an Integer64 that
    // fits is an Integer in KML). The format's own needs are Katana's to
    // meet, not the caller's (docs/interop.md, "Fidelity"):
    //   - KML, KMZ and GPX hold longitude and latitude on WGS 84 only: the
    //     coordinates are converted, and a table with no coordinate system is
    //     refused with InvalidCRS before anything is written;
    //   - CSV is written with its geometry (WKT; X, Y and Z for points) and
    //     the .csvt that keeps its field types;
    //   - a MapInfo table gets bounds from the data, since its default
    //     bounds quantise survey coordinates to about a centimetre;
    //   - a DXF writes areas as closed polylines, not solid hatches.
    // A failure GDAL raises while writing a feature fails the write - KML
    // reports "Export of geometry to KML failed" and writes the placemark
    // without geometry - and whatever was written is removed.
    [[nodiscard]] static katana::core::Result<VectorWriteReport>
    writeTables(const std::filesystem::path& path, const processing::FeatureSet& set,
                const VectorExportOptions& options);

    // The driver a vector path is written with - gis::vectorWriterFor, from
    // GDAL's registry and its own choice for the name (formats.hpp) - or an
    // error naming the extension when no writer claims it. Exposed so a UI
    // can reject an unsupported extension before the user fills in a whole
    // export dialog.
    [[nodiscard]] static katana::core::Result<std::string>
    vectorDriverForPath(const std::filesystem::path& path);

    // The same for a raster written by writeRaster: .tif/.tiff -> GTiff,
    // .asc -> AAIGrid, .img -> HFA. Unsupported, naming the extension, for
    // anything else. Kept a table on purpose, not the registry: writeRaster
    // writes a DEM's Float64 heights, and most raster writers GDAL has (PNG,
    // JPEG, GIF) hold 8- or 16-bit integers.
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

// True for formats that can hold nothing but longitude and latitude on WGS 84
// (KML, KMZ, GPX): writeTables converts to them, and refuses a table whose
// coordinate system it does not know. GDAL's own KML writer, handed a
// projected coordinate, writes the placemark without its geometry and
// succeeds.
[[nodiscard]] bool driverHoldsOnlyLonLat(const std::string& driver);

// True for a format of points and lines only, one layer of each (GPX's
// waypoints and routes): an area goes to it as the closed line around it.
[[nodiscard]] bool driverHoldsNoAreas(const std::string& driver);

// A coordinate system for a person to read: its name, and its authority code
// where GDAL can identify one - "GDA94 / MGA zone 56 (EPSG:28356)". Parsed by
// GDAL's own OGRSpatialReference, which reads WKT1, WKT2 and PROJJSON alike,
// rather than by picking at the text. Empty for an empty WKT; a WKT GDAL
// cannot parse is described as such, never as blank, because "no CRS" and "a
// CRS we could not read" are different things to the person importing it.
[[nodiscard]] std::string describeCrs(const std::string& wkt);

// Whether two coordinate systems are one, as GDAL's OGRSpatialReference::
// IsSame judges it, not by their text: a .prj's naming of a system and
// EPSG's definition of it are the same, and so are a geographic 3D system
// and its 2D one (GDAL declares a GeoJSON with heights EPSG:4979), and the
// axis order data is mapped to is not compared. Nothing when either is empty
// or cannot be read: nothing is known to differ.
[[nodiscard]] std::optional<bool> sameCrs(const std::string& a, const std::string& b);

// The GDAL version string, for the about box and for bug reports.
[[nodiscard]] std::string gdalVersion();

} // namespace katana::gis
