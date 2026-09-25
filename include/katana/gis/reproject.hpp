#pragma once

// Coordinate transformation and raster warping through GDAL/OGR, behind a
// Katana interface (Rule 4).
//
// The imports of local files never reproject: a file's declared CRS is read and
// reported, and mixing systems is the user's responsibility (docs/interop.md,
// "No reprojection"). Data fetched from a web service is different in kind -
// the person did not choose the service's CRS, every service offers its own,
// and a web map in EPSG:3857 is useless on an MGA drawing until it is moved -
// so the online import (katana::interop, online_fetch.hpp) brings each result
// into the project's CRS, and this is what it does it with.
//
// AXIS ORDER. Every function here takes and returns coordinates in the
// "traditional GIS order": x is easting or longitude, y northing or latitude,
// whatever the authority says - OGR's OAMS_TRADITIONAL_GIS_ORDER. EPSG:4326
// is officially latitude first, and WMS 1.3.0, WFS 2.0 and WMTS honour that,
// which is the classic way to put Sydney in the Indian Ocean; `crsAxisIsYX`
// is how a request builder asks whether it must swap, and nothing else in the
// program ever sees a swapped pair.
//
// Threading: each call builds its own OGR objects, so calls on different
// threads are independent; a warp runs on the calling thread.

#include <array>
#include <cstdint>
#include <filesystem>
#include <functional>
#include <optional>
#include <stop_token>
#include <string>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/gis/gdal_adapter.hpp"

namespace katana::gis {

// An axis-aligned box in one CRS, traditional GIS order.
struct CrsBox {
    double minX = 0.0;
    double minY = 0.0;
    double maxX = 0.0;
    double maxY = 0.0;

    [[nodiscard]] double width() const { return maxX - minX; }
    [[nodiscard]] double height() const { return maxY - minY; }
    [[nodiscard]] bool valid() const { return maxX > minX && maxY > minY; }
};

// The CRS as WKT2, from anything GDAL's SetFromUserInput reads with its
// network and file access disabled ("EPSG:7856", "EPSG:4326", a WKT, PROJJSON,
// "urn:ogc:def:crs:EPSG::3857", "CRS84"). InvalidCRS for text it cannot read.
[[nodiscard]] katana::core::Result<std::string> crsToWkt(const std::string& crs);

// True when the CRS's authority defines its first axis as northing or
// latitude - EPSG:4326, EPSG:4283, EPSG:7844 and most geographic systems, and
// a few projected ones - so a protocol that follows the authority (WMS 1.3.0,
// WFS 2.0, WMTS TopLeftCorner) must be given (y, x). False for CRS84 and
// every easting-first projection. InvalidCRS for an unreadable CRS.
[[nodiscard]] katana::core::Result<bool> crsAxisIsYX(const std::string& crs);

[[nodiscard]] katana::core::Result<bool> crsIsGeographic(const std::string& crs);

// The EPSG code GDAL identifies for the CRS, when it can.
[[nodiscard]] std::optional<int> crsEpsgCode(const std::string& crs);

// `box` from one CRS to another, densified along its edges (21 points per
// edge, GDAL's own default) so a box whose edges curve in the target is still
// enclosed - a straight-edged box in MGA is not straight in Web Mercator.
[[nodiscard]] katana::core::Result<CrsBox> transformBox(const CrsBox& box,
                                                        const std::string& fromCrs,
                                                        const std::string& toCrs);

// One point, for tests and for reporting where something landed.
[[nodiscard]] katana::core::Result<std::array<double, 2>>
transformPoint(double x, double y, const std::string& fromCrs, const std::string& toCrs);

// Features moved from one CRS to another, vertex by vertex, heights untouched.
// A vertex PROJ cannot transform (outside the target's domain) fails the whole
// call with InvalidCRS naming the feature, rather than leaving it where it was.
[[nodiscard]] katana::core::Result<std::vector<VectorFeature>>
reprojectFeatures(std::vector<VectorFeature> features, const std::string& fromCrs,
                  const std::string& toCrs);

// What a raster is, read from its header only: for a COG over /vsicurl/ that
// is one or two range requests, not the image. For discovering what a pasted
// URL offers.
struct RasterProbe {
    int width = 0;
    int height = 0;
    int bandCount = 0;
    std::string dataType; // GDAL's name: "Byte", "Float32"
    std::string crsWkt;
    std::optional<CrsBox> lonLatBounds; // when the raster is georeferenced
};
[[nodiscard]] katana::core::Result<RasterProbe> probeRaster(const std::string& path,
                                                            const std::string& userAgent = {},
                                                            int timeoutSeconds = 60);

// Writes the features of every layer of `input` clipped to `box` (in the
// input's own CRS) to a GeoPackage at `output` - GDAL's VectorTranslate with
// -clipsrc, so a line or polygon crossing the box is cut at its edge rather
// than kept whole. For a file that covers the world (Natural Earth's
// coastline is one feature per landmass): without the clip an area around
// Sydney imported the whole east coast of Australia.
[[nodiscard]] katana::core::Status clipVectorFile(const std::filesystem::path& input,
                                                  const CrsBox& box,
                                                  const std::filesystem::path& output);

// ---- warping ----------------------------------------------------------------

// A raster to be warped. Either a dataset that carries its own georeferencing
// (a GeoTIFF, a COG over /vsicurl/), or an image a service returned for a box
// it was asked for, in which case `bounds` and `crs` say where it lies.
struct WarpSource {
    std::string path; // a file path, or a GDAL virtual path (/vsicurl/https://...)
    std::optional<CrsBox> bounds;
    std::string crs; // with bounds: the CRS they are in
};

struct WarpOptions {
    std::string targetCrs;
    CrsBox targetBounds;     // in targetCrs
    double resolution = 1.0; // target units per pixel, both axes
    // Elevation: one Float32 band, no-data carried through, bilinear. Imagery:
    // RGBA bytes, cubic, the alpha band marking where no source covered.
    bool elevation = false;
    // Refused before any pixel is read when the output would exceed this many
    // pixels, with a message saying how to shrink the request.
    std::uint64_t maxPixels = 64ull * 1024 * 1024;
    // Seconds per HTTP request for a /vsicurl/ source, and the User-Agent,
    // set for this thread only while the warp runs.
    int timeoutSeconds = 120;
    std::string userAgent;
    // For /vsicurl/ sources that 404 (a Copernicus tile over the ocean), skip
    // the source rather than fail; at least one source must open.
    bool skipMissingSources = false;
};

struct WarpResult {
    int width = 0;
    int height = 0;
    int sourcesUsed = 0;
    std::vector<std::string> warnings;
};

// Warps and mosaics `sources` into a GeoTIFF at `output` covering
// options.targetBounds at options.resolution in options.targetCrs. Progress
// in [0, 1] is reported through `progress`, and `stop` cancels between GDAL's
// chunks (InvalidState, "cancelled", and the partial file removed).
[[nodiscard]] katana::core::Result<WarpResult>
warpToGeoTiff(const std::vector<WarpSource>& sources, const WarpOptions& options,
              const std::filesystem::path& output, const std::stop_token& stop = {},
              const std::function<void(double)>& progress = {});

// A virtual raster (GDAL VRT, written to `output`) stacking three single-band
// rasters - Sentinel-2's red, green and blue reflectance, 0 to about 10000 -
// as one 8-bit RGB image, each band's `low`..`high` scaled onto 1..255 (a
// value outside it clamped to the end, never to 0) and 0 kept for no-data:
// the true-colour composite a person expects to see. No
// pixel is read here; the VRT is a warp source like any other, so only the
// part of each band the warp needs is ever fetched.
[[nodiscard]] katana::core::Status buildTrueColourVrt(const std::vector<std::string>& bandPaths,
                                                      double low, double high,
                                                      const std::filesystem::path& output,
                                                      const std::string& userAgent = {},
                                                      int timeoutSeconds = 120);

} // namespace katana::gis
