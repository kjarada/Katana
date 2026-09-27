#pragma once

// Rasters in and out of a geoprocessing run (docs/geoprocessing.md,
// "Bindings"): a TIN surface as the grid an algorithm reads, where a derived
// raster file is kept, and such a file as the reference raster the drawing
// shows.

#include <cstdint>
#include <filesystem>
#include <limits>
#include <optional>
#include <string>
#include <string_view>

#include "katana/core/error.hpp"
#include "katana/gis/processing.hpp"
#include "katana/interop/reference_data.hpp"
#include "katana/terrain/tin_surface.hpp"

namespace katana::interop::geo {

// The no-data value of a grid sampled from a surface - a cell whose centre is
// off the surface or in a hole - and of a DEM exportSurfaceRaster writes: the
// conventional Float32 no-data value of Esri and GDAL DEMs, -FLT_MAX. Unlike
// the other common sentinel, -9999, it cannot be a real elevation on any body
// a surveyor works on - the Challenger Deep is about -10 935 m - so ground that
// truly lies at -9999 is not silently punched full of holes. It is exact in
// Float32 as well as Float64, so it survives a reader or an algorithm that
// works in Float32 (GDAL's AAIGrid reads so by default).
inline constexpr double kGridNoData = std::numeric_limits<float>::lowest();

// The surface sampled at the CENTRE of each `cell`-square cell by the TIN's
// own linear interpolation, north-up, its top-left corner at the surface's
// (min x, max y) - the grid exportSurfaceRaster writes, from the same code.
// InvalidArgument for an empty surface, a cell that is not positive and
// finite, or more than `maxCells` cells (named in full, with the cell to use
// instead).
[[nodiscard]] katana::core::Result<katana::gis::processing::RasterGrid>
surfaceGrid(const katana::terrain::TinSurface& surface, double cell,
            std::uint64_t maxCells = 25'000'000);

// Where a derived raster called `name` is kept: <project>/cache/gdal/<name>.tif
// when the drawing has a project directory, else <scratch>/<name>.tif (a reply
// then says persisted=no). The name is made safe for a file: anything but
// letters, digits, '.', '-' and '_' becomes '_'. Nothing is created.
[[nodiscard]] std::filesystem::path
derivedRasterPath(const std::optional<std::filesystem::path>& projectDirectory,
                  const std::filesystem::path& scratch, std::string_view name);

// `file`, read as a reference raster (interop::importRaster) with role
// Derived and the line that made it. Its band facts are left for the
// reference-layer work that shows them.
[[nodiscard]] katana::core::Result<RasterOverlay>
derivedOverlay(const std::filesystem::path& file, std::string name, std::string derivation);

} // namespace katana::interop::geo
