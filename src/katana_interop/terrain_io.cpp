#include "katana/interop/terrain_io.hpp"

#include <algorithm>
#include <cmath>
#include <limits>
#include <system_error>

#include "katana/core/text.hpp"
#include "katana/gis/gdal_adapter.hpp"

namespace katana::interop {
namespace {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::geometry::Point2;
using katana::geometry::Point3;

// Samples a stride keeps along one side: ceil(extent / stride), in 64 bits.
std::int64_t samplesAlong(std::int64_t extent, std::int64_t stride)
{
    return (extent + stride - 1) / stride;
}

std::uint64_t samplesFor(int width, int height, std::int64_t stride)
{
    return static_cast<std::uint64_t>(samplesAlong(width, stride)) *
           static_cast<std::uint64_t>(samplesAlong(height, stride));
}

// The conventional Float32 no-data value of Esri and GDAL DEMs, -FLT_MAX.
// Unlike the other common sentinel, -9999, it cannot be a real elevation on
// any body a surveyor works on - the Challenger Deep is about -10 935 m - so a
// surface whose ground truly lies at -9999 is not silently punched full of
// holes by a reader. It is exact in Float32 as well as Float64, so it survives
// a reader that loads the band as Float32 (GDAL's AAIGrid does by default).
constexpr double kSurfaceNoData = std::numeric_limits<float>::lowest();

} // namespace

// ---- DEM -> elevations ------------------------------------------------------

int strideForCap(int width, int height, std::size_t maxPoints)
{
    if (width <= 0 || height <= 0) {
        return 1; // nothing to sample; any stride samples all of it
    }
    const std::int64_t longest = std::max(width, height);
    if (maxPoints == 0) {
        // No stride brings a grid to zero samples. The coarsest meaningful one
        // - a single sample - is the closest answer; callers refuse a zero cap
        // before asking (readRasterElevations does).
        return static_cast<int>(longest);
    }
    const auto cap = static_cast<std::uint64_t>(maxPoints);
    if (samplesFor(width, height, 1) <= cap) {
        return 1;
    }
    // The sample count ceil(w/s) * ceil(h/s) never increases with s, so the
    // smallest s meeting the cap is found by bisection between a stride known
    // to be too small and one known to be enough. The lower bound is the
    // continuous estimate: ceil(w/s) * ceil(h/s) >= w*h / s^2, so no s below
    // sqrt(w*h / cap) can meet the cap - one less than its floor is safely
    // below, whatever the rounding of the square root. The upper bound is the
    // longer side, where one sample remains. Stepping up from the estimate one
    // stride at a time would also be exact, but not quick: on a one-pixel
    // strip the ceilings put the answer far above the estimate - 1 x 10^9
    // pixels capped at 10 is a stride of 10^8 against an estimate of 10^4.
    //
    // The audit (QT-24) found the GUI taking floor(sqrt(ceil(w*h / cap))),
    // which the floor can leave at 1 with the grid three times over the cap.
    const double estimate = std::sqrt(static_cast<double>(width) * static_cast<double>(height) /
                                      static_cast<double>(cap));
    std::int64_t tooSmall = std::max<std::int64_t>(0, static_cast<std::int64_t>(estimate) - 1);
    std::int64_t enough = longest;
    // Invariant: samplesFor(tooSmall) > cap (or tooSmall == 0), samplesFor(enough) <= cap.
    while (enough - tooSmall > 1) {
        const std::int64_t middle = tooSmall + (enough - tooSmall) / 2;
        if (samplesFor(width, height, middle) <= cap) {
            enough = middle;
        } else {
            tooSmall = middle;
        }
    }
    return static_cast<int>(enough);
}

Result<RasterElevations> readRasterElevations(const std::filesystem::path& path,
                                              const RasterElevationOptions& options)
{
    std::error_code existsError;
    if (!std::filesystem::exists(path, existsError)) {
        return makeError(ErrorCode::NotFound, "file does not exist", path.string());
    }
    if (options.maxPoints == 0) {
        return makeError(ErrorCode::InvalidArgument,
                         "a DEM cannot be read into zero points; the cap must be at least 1",
                         path.string());
    }

    auto dataset = katana::gis::GdalDataset::open(path);
    if (!dataset.ok()) {
        return dataset.error();
    }
    if (!(*dataset)->hasRaster()) {
        return makeError(ErrorCode::InvalidArgument, "file holds no raster bands", path.string());
    }
    auto info = (*dataset)->rasterInfo();
    if (!info.ok()) {
        return info.error();
    }
    if (!info->hasGeotransform) {
        // GDAL's default transform would put pixel (i, j) at (i, j) in world
        // units, and a surface built there is in the wrong place with nothing
        // to say so.
        return makeError(ErrorCode::InvalidArgument,
                         "this raster is not georeferenced, so its pixels have no ground position",
                         path.string());
    }
    if (options.band < 1 || options.band > info->bandCount) {
        return makeError(ErrorCode::InvalidArgument, "raster band index is out of range",
                         "requested " + std::to_string(options.band) + " of " +
                             std::to_string(info->bandCount) + " in " + path.string());
    }

    const int stride = strideForCap(info->width, info->height, options.maxPoints);
    auto samples = (*dataset)->readBandSampled(options.band, stride);
    if (!samples.ok()) {
        return samples.error();
    }

    RasterElevations result;
    result.stride = stride;
    result.projectionWkt = info->projectionWkt;
    result.sampled = static_cast<std::uint64_t>(samples->columns) *
                     static_cast<std::uint64_t>(samples->rows);
    result.points.reserve(static_cast<std::size_t>(result.sampled));

    const std::array<double, 6>& gt = info->geotransform;
    for (int j = 0; j < samples->rows; ++j) {
        for (int i = 0; i < samples->columns; ++i) {
            const double value = samples->values[static_cast<std::size_t>(j) *
                                                     static_cast<std::size_t>(samples->columns) +
                                                 static_cast<std::size_t>(i)];
            if (!std::isfinite(value) ||
                (samples->noDataValue.has_value() && value == *samples->noDataValue)) {
                ++result.noData;
                continue;
            }
            // The CENTRE of the source pixel: a DEM's value is the ground at
            // the middle of its cell (the "pixel is area" convention GDAL
            // reports every geotransform in), and the corner would shift the
            // whole surface half a pixel north-west.
            const double px = static_cast<double>(i) * stride + 0.5;
            const double py = static_cast<double>(j) * stride + 0.5;
            result.points.emplace_back(gt[0] + px * gt[1] + py * gt[2],
                                       gt[3] + px * gt[4] + py * gt[5], value);
        }
    }
    return result;
}

// ---- point cloud -> surface points -----------------------------------------

CloudSurfacePoints surfacePoints(const PointCloudLayer& cloud)
{
    CloudSurfacePoints result;
    const auto ground = static_cast<std::size_t>(
        std::count_if(cloud.points.begin(), cloud.points.end(),
                      [](const katana::pointcloud::PointCloudPoint& point) {
                          return point.classification == kAsprsGround;
                      }));
    if (ground == 0) {
        // Nothing is classified as ground: an unclassified cloud, not a cloud
        // with no ground. Every return is used and the caller told so, rather
        // than refusing - which would leave photogrammetry and terrestrial
        // scans with no way to become a surface at all.
        result.points.reserve(cloud.points.size());
        for (const auto& point : cloud.points) {
            result.points.emplace_back(point.x, point.y, point.z);
        }
        return result;
    }
    result.groundOnly = true;
    result.points.reserve(ground);
    for (const auto& point : cloud.points) {
        if (point.classification == kAsprsGround) {
            result.points.emplace_back(point.x, point.y, point.z);
        } else {
            ++result.excluded;
        }
    }
    return result;
}

// ---- surface -> DEM -----------------------------------------------------------

Result<SurfaceRasterResult> exportSurfaceRaster(const katana::terrain::TinSurface& surface,
                                                const std::filesystem::path& path,
                                                const SurfaceRasterOptions& options)
{
    if (surface.empty()) {
        return makeError(ErrorCode::InvalidArgument, "the surface has no triangles to export",
                         path.string());
    }
    if (!(options.cellSize > 0.0) || !std::isfinite(options.cellSize)) {
        return makeError(ErrorCode::InvalidArgument,
                         "the cell size must be a positive, finite length",
                         katana::core::formatExactReal(options.cellSize));
    }

    std::string driver = options.driver;
    if (driver.empty()) {
        auto inferred = katana::gis::GdalDataset::rasterDriverForPath(path);
        if (!inferred.ok()) {
            return inferred.error();
        }
        driver = *inferred;
    }

    const katana::geometry::Box2& bounds = surface.bounds();
    // Counted in doubles first: a cell size of a micrometre over a 10 km site
    // is 10^20 cells, which no integer type here would hold.
    const double columnsWanted = std::max(1.0, std::ceil(bounds.width() / options.cellSize));
    const double rowsWanted = std::max(1.0, std::ceil(bounds.height() / options.cellSize));
    const double cellsWanted = columnsWanted * rowsWanted;
    if (cellsWanted > static_cast<double>(options.maxCells) ||
        columnsWanted > static_cast<double>(std::numeric_limits<int>::max()) ||
        rowsWanted > static_cast<double>(std::numeric_limits<int>::max())) {
        // Named in full: "too many cells" leaves the user guessing how much
        // larger a cell has to be.
        return makeError(ErrorCode::InvalidArgument,
                         "a " + katana::core::formatExactReal(options.cellSize) +
                             " cell over this surface is " +
                             katana::core::formatExactReal(cellsWanted) + " cells (" +
                             katana::core::formatExactReal(columnsWanted) + " x " +
                             katana::core::formatExactReal(rowsWanted) + "), more than the " +
                             std::to_string(options.maxCells) +
                             " an export holds; choose a larger cell",
                         path.string());
    }

    SurfaceRasterResult result;
    result.columns = static_cast<int>(columnsWanted);
    result.rows = static_cast<int>(rowsWanted);
    result.noDataValue = kSurfaceNoData;
    result.driver = driver;
    // North-up, top-left corner at (min x, max y): row 0 is the northern edge.
    result.geotransform = {bounds.min.x, options.cellSize, 0.0, bounds.max.y, 0.0,
                           -options.cellSize};

    const auto columns = static_cast<std::size_t>(result.columns);
    std::vector<double> values(columns * static_cast<std::size_t>(result.rows), kSurfaceNoData);
    // One row of positions at a time: the grid of values has to be whole for
    // GDAL, but a grid of Point2 beside it would double the memory for
    // nothing, and a row is still thousands of positions for elevationsAt to
    // share across threads.
    std::vector<Point2> positions(columns);
    for (int row = 0; row < result.rows; ++row) {
        const double y = bounds.max.y - (static_cast<double>(row) + 0.5) * options.cellSize;
        for (std::size_t column = 0; column < columns; ++column) {
            positions[column] =
                Point2(bounds.min.x + (static_cast<double>(column) + 0.5) * options.cellSize, y);
        }
        const std::vector<std::optional<double>> elevations = surface.elevationsAt(positions);
        double* out = values.data() + static_cast<std::size_t>(row) * columns;
        for (std::size_t column = 0; column < columns; ++column) {
            if (elevations[column].has_value()) {
                out[column] = *elevations[column];
                ++result.cellsWithData;
            }
        }
    }

    katana::gis::RasterExportOptions raster;
    raster.width = result.columns;
    raster.height = result.rows;
    raster.driver = driver;
    raster.projectionWkt = options.projectionWkt;
    raster.geotransform = result.geotransform;
    raster.noDataValue = kSurfaceNoData;
    const auto written = katana::gis::GdalDataset::writeRaster(path, raster, values);
    if (!written.ok()) {
        return written.error();
    }
    return result;
}

double suggestedCellSize(const katana::geometry::Box2& bounds, int targetCells)
{
    const double longer = std::max(bounds.width(), bounds.height());
    if (bounds.empty() || !(longer > 0.0) || !std::isfinite(longer) || targetCells < 1) {
        return 1.0;
    }
    const double raw = longer / static_cast<double>(targetCells);

    // value x 10^n. A power of ten is exact in a double up to 10^22, and
    // multiplying or dividing by one rounds once, so 5 x 10^-1 comes out as
    // the double nearest 0.5 rather than as 5 times an inexact 0.1.
    const auto scaled = [](double value, int n) {
        return n >= 0 ? value * std::pow(10.0, n) : value / std::pow(10.0, -n);
    };

    // raw = mantissa x 10^exponent with the mantissa in [1, 10). log10 can
    // land a hair below an exact power of ten, so the split is corrected by
    // comparison rather than trusted.
    int exponent = static_cast<int>(std::floor(std::log10(raw)));
    double mantissa = scaled(raw, -exponent);
    if (mantissa >= 10.0) {
        ++exponent;
        mantissa = scaled(raw, -exponent);
    } else if (mantissa < 1.0) {
        --exponent;
        mantissa = scaled(raw, -exponent);
    }

    // Nearest of 1, 2, 5 and 10 on a LOGARITHMIC scale, the scale on which a
    // cell size is judged (twice as fine is as far off as twice as coarse).
    // Its boundaries are the geometric means of neighbours: sqrt(2), sqrt(10)
    // and sqrt(50). On a linear scale 1.45 would round to 1, 3.3 to 2 and 7.3
    // to 5, although each is nearer by ratio to the step above it.
    double step = 10.0;
    if (mantissa < std::sqrt(2.0)) {
        step = 1.0;
    } else if (mantissa < std::sqrt(10.0)) {
        step = 2.0;
    } else if (mantissa < std::sqrt(50.0)) {
        step = 5.0;
    }
    return scaled(step, exponent);
}

} // namespace katana::interop
