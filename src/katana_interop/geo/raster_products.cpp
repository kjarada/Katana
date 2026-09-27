// Rasters in and out of a geoprocessing run (raster_products.hpp).

#include "katana/interop/geo/raster_products.hpp"

#include <algorithm>
#include <cmath>
#include <limits>
#include <utility>
#include <vector>

#include "katana/core/text.hpp"
#include "katana/interop/import.hpp"

namespace katana::interop::geo {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::geometry::Point2;

Result<katana::gis::processing::RasterGrid>
surfaceGrid(const katana::terrain::TinSurface& surface, double cell, std::uint64_t maxCells)
{
    if (surface.empty()) {
        return makeError(ErrorCode::InvalidArgument, "the surface has no triangles");
    }
    if (!(cell > 0.0) || !std::isfinite(cell)) {
        return makeError(ErrorCode::InvalidArgument,
                         "the cell size must be a positive, finite length",
                         katana::core::formatExactReal(cell));
    }
    const katana::geometry::Box2& bounds = surface.bounds();
    // Counted in doubles first: a cell of a micrometre over a 10 km site is
    // 10^20 cells, which no integer type here would hold.
    const double columnsWanted = std::max(1.0, std::ceil(bounds.width() / cell));
    const double rowsWanted = std::max(1.0, std::ceil(bounds.height() / cell));
    const double cellsWanted = columnsWanted * rowsWanted;
    if (cellsWanted > static_cast<double>(maxCells) ||
        columnsWanted > static_cast<double>(std::numeric_limits<int>::max()) ||
        rowsWanted > static_cast<double>(std::numeric_limits<int>::max())) {
        // Named in full: "too many cells" leaves the user guessing how much
        // larger a cell has to be.
        return makeError(ErrorCode::InvalidArgument,
                         "a " + katana::core::formatExactReal(cell) +
                             " cell over this surface is " +
                             katana::core::formatExactReal(cellsWanted) + " cells (" +
                             katana::core::formatExactReal(columnsWanted) + " x " +
                             katana::core::formatExactReal(rowsWanted) + "), more than the " +
                             std::to_string(maxCells) + " allowed; choose a larger cell");
    }

    katana::gis::processing::RasterGrid grid;
    grid.info.width = static_cast<int>(columnsWanted);
    grid.info.height = static_cast<int>(rowsWanted);
    grid.info.bandCount = 1;
    // North-up, top-left corner at (min x, max y): row 0 is the northern edge.
    grid.info.geotransform = {bounds.min.x, cell, 0.0, bounds.max.y, 0.0, -cell};
    grid.info.hasGeotransform = true;
    grid.info.noDataValue = kGridNoData;
    grid.dataType = "Float64";
    grid.noData = {kGridNoData};

    const auto columns = static_cast<std::size_t>(grid.info.width);
    std::vector<double> values(columns * static_cast<std::size_t>(grid.info.height), kGridNoData);
    // One row of positions at a time: the grid of values has to be whole for
    // GDAL, but a grid of Point2 beside it would double the memory for
    // nothing, and a row is still thousands of positions for elevationsAt to
    // share across threads.
    std::vector<Point2> positions(columns);
    for (int row = 0; row < grid.info.height; ++row) {
        const double y = bounds.max.y - (static_cast<double>(row) + 0.5) * cell;
        for (std::size_t column = 0; column < columns; ++column) {
            positions[column] = Point2(bounds.min.x + (static_cast<double>(column) + 0.5) * cell, y);
        }
        const std::vector<std::optional<double>> elevations = surface.elevationsAt(positions);
        double* out = values.data() + static_cast<std::size_t>(row) * columns;
        for (std::size_t column = 0; column < columns; ++column) {
            if (elevations[column].has_value()) {
                out[column] = *elevations[column];
            }
        }
    }
    grid.bands.push_back(std::move(values));
    return grid;
}

std::filesystem::path derivedRasterPath(const std::optional<std::filesystem::path>& projectDirectory,
                                        const std::filesystem::path& scratch, std::string_view name)
{
    std::string safe;
    for (const char c : name) {
        const bool plain = (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') ||
                           (c >= '0' && c <= '9') || c == '.' || c == '-' || c == '_';
        safe += plain ? c : '_';
    }
    // A name of dots alone would be the folder or its parent.
    if (safe.empty() || safe.find_first_not_of('.') == std::string::npos) {
        safe = "raster";
    }
    const std::filesystem::path folder =
        projectDirectory ? *projectDirectory / "cache" / "gdal" : scratch;
    return folder / (safe + ".tif");
}

Result<RasterOverlay> derivedOverlay(const std::filesystem::path& file, std::string name,
                                     std::string derivation)
{
    RasterImportOptions options;
    options.name = std::move(name);
    auto overlay = importRaster(file, options);
    if (!overlay) {
        return overlay.error();
    }
    overlay->role = RasterRole::Derived;
    overlay->derivation = std::move(derivation);
    return overlay;
}

} // namespace katana::interop::geo
