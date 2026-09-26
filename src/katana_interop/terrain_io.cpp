#include "katana/interop/terrain_io.hpp"

#include <algorithm>
#include <cmath>
#include <limits>
#include <set>
#include <system_error>
#include <utility>

#include "katana/core/text.hpp"
#include "katana/gis/gdal_adapter.hpp"
#include "katana/gis/processing.hpp"
#include "katana/interop/geo/raster_products.hpp"

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

std::string utf8Of(const std::filesystem::path& path)
{
    const std::u8string text = path.generic_u8string();
    return std::string(text.begin(), text.end());
}

std::filesystem::path pathOf(const std::string& utf8)
{
    return std::filesystem::path(std::u8string(utf8.begin(), utf8.end()));
}

// The window `area` of the raster at `path`, cut by GDAL into a file of its
// own in the temporary folder: the reader then samples the cut as a whole,
// so the stride is chosen for the site and not for the sheet. Each call has
// its own file (the bridge names them), which the caller removes.
Result<std::filesystem::path> cutWindow(const std::filesystem::path& path,
                                        const katana::geometry::Box2& area)
{
    namespace gp = katana::gis::processing;
    gp::RunRequest clip;
    clip.path = {"raster", "clip"};
    clip.values.emplace_back("input", gp::DatasetValue(gp::DatasetPath{utf8Of(path), {}, {}}));
    const auto real = [](double value) { return katana::core::formatExactReal(value); };
    clip.tokens = {"--bbox=" + real(area.min.x) + "," + real(area.min.y) + "," +
                       real(area.max.x) + "," + real(area.max.y),
                   // A window reaching past the raster takes what is there;
                   // the rest reads as no-data and is left out, counted.
                   "--allow-bbox-outside-source"};
    // Every cell to a file: the cut of a large DEM is as large as the site,
    // and the reader streams a file a row at a time.
    clip.maxMemoryCells = 0;
    auto cut = gp::run(clip);
    if (!cut) {
        return makeError(cut.error().code, cut.error().message, path.string());
    }
    if (!cut->file) {
        return makeError(ErrorCode::Internal, "GDAL's clip kept no raster", path.string());
    }
    return pathOf(*cut->file);
}

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
    if (options.area) {
        const katana::geometry::Box2& area = *options.area;
        if (area.empty() || !(area.width() > 0.0) || !(area.height() > 0.0)) {
            return makeError(ErrorCode::InvalidArgument,
                             "the window of a raster to read needs a width and a height",
                             path.string());
        }
        dataset->reset(); // the cut opens the file itself
        auto cut = cutWindow(path, area);
        if (!cut) {
            return cut.error();
        }
        RasterElevationOptions whole = options;
        whole.area.reset();
        auto read = readRasterElevations(*cut, whole);
        std::error_code removed;
        std::filesystem::remove(*cut, removed);
        return read;
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

CloudSurfacePoints surfacePoints(const PointCloudLayer& cloud,
                                 const std::vector<std::uint8_t>& classes)
{
    if (classes.empty()) {
        return surfacePoints(cloud);
    }
    const std::set<std::uint8_t> wanted(classes.begin(), classes.end());
    CloudSurfacePoints result;
    result.groundOnly = wanted == std::set<std::uint8_t>{kAsprsGround};
    for (const auto& point : cloud.points) {
        if (wanted.contains(point.classification)) {
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
                                                const SurfaceRasterOptions& options,
                                                const std::stop_token& stop)
{
    namespace gp = katana::gis::processing;
    if (surface.empty()) {
        return makeError(ErrorCode::InvalidArgument, "the surface has no triangles to export",
                         path.string());
    }
    if (!(options.cellSize > 0.0) || !std::isfinite(options.cellSize)) {
        return makeError(ErrorCode::InvalidArgument,
                         "the cell size must be a positive, finite length",
                         katana::core::formatExactReal(options.cellSize));
    }
    if (options.dataType != "Float32" && options.dataType != "Float64") {
        return makeError(ErrorCode::InvalidArgument, "a DEM is written as Float32 or Float64",
                         options.dataType);
    }

    std::string driver = options.cog ? std::string("COG") : options.driver;
    if (driver.empty()) {
        auto inferred = katana::gis::GdalDataset::rasterDriverForPath(path);
        if (!inferred.ok()) {
            return inferred.error();
        }
        driver = *inferred;
    }
    std::error_code existsError;
    if (!options.overwrite && std::filesystem::exists(path, existsError)) {
        return makeError(ErrorCode::AlreadyExists,
                         "a file is already there; replacing it has to be asked for",
                         path.string());
    }

    // The one sampler (geo::surfaceGrid), so a DEM written here and the grid
    // a geoprocessing run reads from the same surface are the same grid.
    auto grid = geo::surfaceGrid(surface, options.cellSize, options.maxCells);
    if (!grid) {
        return makeError(grid.error().code, grid.error().message, path.string());
    }
    SurfaceRasterResult result;
    result.columns = grid->info.width;
    result.rows = grid->info.height;
    result.noDataValue = geo::kGridNoData;
    result.driver = driver;
    result.dataType = options.dataType;
    result.geotransform = grid->info.geotransform;
    const std::vector<double>& values = grid->bands.front();
    result.cellsWithData = static_cast<std::uint64_t>(
        std::ranges::count_if(values, [](double value) { return value != geo::kGridNoData; }));
    // The grid is made in the type asked for, so GDAL rounds each double to
    // the nearest Float32 once, as it fills the grid, and the file holds that.
    grid->dataType = options.dataType;
    grid->info.projectionWkt = options.projectionWkt;

    // Katana's defaults for the driver, then the caller's options over them
    // by key: a DEM is read a window at a time, so tiled, and floating-point
    // heights compress better with the floating-point predictor.
    std::vector<std::pair<std::string, std::string>> creation;
    if (driver == "GTiff") {
        creation = {{"TILED", "YES"}, {"COMPRESS", "DEFLATE"}, {"PREDICTOR", "3"}};
    } else if (driver == "COG") {
        creation = {{"COMPRESS", "DEFLATE"}, {"PREDICTOR", "YES"}};
    }
    for (const std::string& option : options.creationOptions) {
        const std::size_t equals = option.find('=');
        if (equals == std::string::npos || equals == 0) {
            return makeError(ErrorCode::InvalidArgument, "a creation option is KEY=VALUE", option);
        }
        const std::string key = katana::core::lowered(option.substr(0, equals));
        std::erase_if(creation, [&](const std::pair<std::string, std::string>& kept) {
            return katana::core::lowered(kept.first) == key;
        });
        creation.emplace_back(option.substr(0, equals), option.substr(equals + 1));
    }

    gp::RunRequest request;
    request.path = {"raster", "convert"};
    request.values.emplace_back("input", gp::DatasetValue(std::move(*grid)));
    request.outputTo = gp::OutputTo::File;
    request.outputPath = utf8Of(path);
    request.outputFormat = driver;
    for (const auto& [key, value] : creation) {
        request.creationOptions.push_back(key + "=" + value);
    }
    request.overwrite = options.overwrite;
    auto written = gp::run(request, stop);
    if (!written) {
        return makeError(written.error().code, written.error().message, path.string());
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
