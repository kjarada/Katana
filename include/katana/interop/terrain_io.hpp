#pragma once

// Terrain to and from GDAL and PDAL data (PLAN.MD Phases 14, 17 and 20): the
// elevations of a DEM, the ground of a point cloud, and a surface written out
// as a DEM.
//
// Here rather than in katana_terrain because terrain may not see the readers
// (tools/check_layering.cmake), and rather than in the desktop application
// because the CLI and the tests need the same conversions - a policy such as
// "a surface from a cloud is built from its ground returns" must have one
// definition, not one per front end.

#include <array>
#include <cstdint>
#include <filesystem>
#include <optional>
#include <stop_token>
#include <string>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/geometry/primitives2d.hpp"
#include "katana/geometry/primitives3d.hpp"
#include "katana/interop/reference_data.hpp"
#include "katana/terrain/tin_surface.hpp"

namespace katana::interop {

// ---- DEM -> elevations ------------------------------------------------------

struct RasterElevationOptions {
    // At most this many points are returned. The source is sampled on a
    // regular stride chosen so that the whole extent is covered at or under
    // the cap - never truncated to its first rows. 0 is refused.
    std::size_t maxPoints = 400'000;
    int band = 1;
    // Only the pixels of this window of the ground, so the cap is spent on
    // the site rather than on the whole sheet a DEM was delivered as. Cut
    // by GDAL (`raster clip --bbox`) before the stride is chosen; a window
    // reaching past the raster takes what the raster has there.
    std::optional<katana::geometry::Box2> area;
};

struct RasterElevations {
    // One point per kept sample, at the CENTRE of its source pixel, carrying
    // the band's value unchanged. Georeferencing is the file's own.
    std::vector<katana::geometry::Point3> points;
    int stride = 1;              // every stride-th pixel of every stride-th row
    std::uint64_t sampled = 0;   // pixels looked at
    std::uint64_t noData = 0;    // of those, no-data or non-finite and left out
    std::string projectionWkt;
};

// The elevations of a DEM band as points, for the TIN builder: the true band
// values through GDAL, not the 8-bit display copy a RasterOverlay holds (audit
// QT-23). NotFound for a missing file; InvalidArgument for a raster with no
// georeferencing (its pixels have no ground position, and inventing one would
// build a surface in the wrong place), a band out of range, or a zero cap;
// FileImportFailure when GDAL cannot read it.
[[nodiscard]] katana::core::Result<RasterElevations>
readRasterElevations(const std::filesystem::path& path, const RasterElevationOptions& options = {});

// The stride that brings a width x height grid to at most maxPoints samples:
// the smallest s with ceil(width/s) * ceil(height/s) <= maxPoints. Exposed so
// the cap can be tested at its edges without a file (audit QT-24).
[[nodiscard]] int strideForCap(int width, int height, std::size_t maxPoints);

// ---- point cloud -> surface points -----------------------------------------

// ASPRS LAS 1.4 R15 table 17: class 2 is Ground.
inline constexpr std::uint8_t kAsprsGround = 2;

struct CloudSurfacePoints {
    std::vector<katana::geometry::Point3> points;
    // True when the points are the cloud's ground returns alone. False when
    // the cloud carries no class-2 point - unclassified photogrammetry, a
    // terrestrial scan - and every return was taken, so trees and roofs are
    // part of the surface. The caller must SAY which it was: a canopy surface
    // presented as ground is the silent failure audit QT-10 found.
    bool groundOnly = false;
    // Returns left out because they are classified as something else.
    std::uint64_t excluded = 0;
};

// The one policy for a surface from a cloud: its ground returns when it has
// any, otherwise every return, flagged.
[[nodiscard]] CloudSurfacePoints surfacePoints(const PointCloudLayer& cloud);

// The returns of the ASPRS classes asked for, whatever the cloud holds - a
// person who knows the classes of their data says so (SURFACE FROM CLOUD
// classes=2,8). groundOnly is true when the classes are exactly {2};
// `excluded` counts the returns of every other class. An empty list is the
// policy above.
[[nodiscard]] CloudSurfacePoints surfacePoints(const PointCloudLayer& cloud,
                                               const std::vector<std::uint8_t>& classes);

// ---- surface -> DEM -----------------------------------------------------------

struct SurfaceRasterOptions {
    // World units per cell, square. Must be positive and finite.
    double cellSize = 1.0;
    // Empty: from the path's extension (gis::GdalDataset::rasterDriverForPath),
    // or COG when `cog` is set.
    std::string driver;
    std::string projectionWkt;
    // Largest grid written, columns x rows. The whole grid is held as doubles
    // before GDAL writes it, so this is 200 MB; a finer grid is refused with
    // InvalidArgument naming the cell count, rather than exhausting memory.
    std::uint64_t maxCells = 25'000'000;
    // What each cell is stored as: Float32 (the default) or Float64. Float32
    // is the DEM convention and half the size; near 1000 m its step is 6e-5
    // m, far finer than any survey that made the surface. The no-data value
    // is exact in both.
    std::string dataType = "Float32";
    // A Cloud Optimised GeoTIFF, written by GDAL's COG driver: tiled,
    // compressed and with overviews, readable a window at a time over HTTP.
    bool cog = false;
    // KEY=VALUE, GDAL's creation options for the driver, over Katana's
    // defaults for it: a GeoTIFF is tiled and DEFLATE-compressed with the
    // floating-point predictor (PREDICTOR=3), a COG DEFLATE with PREDICTOR=YES.
    std::vector<std::string> creationOptions;
    // A file already at the path is replaced only when this says so; else
    // AlreadyExists, and the file is left as it was.
    bool overwrite = false;
};

struct SurfaceRasterResult {
    int columns = 0;
    int rows = 0;
    std::uint64_t cellsWithData = 0;
    // Written into every cell whose centre is off the surface or in a hole,
    // and declared as the band's no-data value.
    double noDataValue = 0.0;
    std::string driver;
    std::string dataType;
    std::array<double, 6> geotransform{0.0, 1.0, 0.0, 0.0, 0.0, -1.0};
};

// Each cell's value is the surface's elevation at the cell's CENTRE, by the
// TIN's own linear interpolation (TinSurface::elevationsAt) - the value a
// surveyor would read off the surface there, not an average over the cell.
// The grid is north-up with its top-left corner at the surface's (min x,
// max y). The grid is written by GDAL's own `raster convert`
// (gis/processing.hpp), so every driver that can write a raster - those that
// only copy a finished dataset, as AAIGrid, included - and every creation
// option it takes are there. InvalidArgument for an empty surface, a bad cell
// size, too many cells or a data type other than Float32 and Float64;
// Unsupported for an extension no raster driver claims; AlreadyExists for a
// file in the way without `overwrite`; InvalidState "cancelled" when `stop`
// is requested (no file is left); what GDAL refused otherwise.
[[nodiscard]] katana::core::Result<SurfaceRasterResult>
exportSurfaceRaster(const katana::terrain::TinSurface& surface, const std::filesystem::path& path,
                    const SurfaceRasterOptions& options = {}, const std::stop_token& stop = {});

// A cell size for `bounds` of about `targetCells` along its longer side,
// rounded to 1, 2 or 5 x 10^n so the grid lines fall on round coordinates.
// 1.0 for an empty or zero-sized box.
[[nodiscard]] double suggestedCellSize(const katana::geometry::Box2& bounds, int targetCells = 1000);

} // namespace katana::interop
