#include "katana/geometry/point_splat.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <cstring>

#include "katana/core/cpu_features.hpp"
#include "katana/core/task_pool.hpp"

#include "simd/splat_kernels.hpp"

namespace katana::geometry {

namespace {

// The kernel from this many points. Measured with BM_ProjectToPixels
// (bench_simd.cpp): see docs/performance.md.
constexpr std::size_t kProjectMinimum = 8;

// Points projected at a time into a buffer on the stack: 4 KB of pixel
// coordinates, which stay in L1 between the projection and the scatter.
constexpr std::size_t kBlock = 512;

// Below this many points drawn, one band on the calling thread: handing the
// pool a job costs more than the splat.
constexpr std::size_t kParallelMinimum = 32'768;

// Bands are at least this many rows, so a band is not mostly the tiles it
// shares with its neighbours.
constexpr int kMinimumBandRows = 16;

// The deepest level of detail: 4^8 cells, far more than a tile's points.
constexpr int kMaximumLevel = 8;

void projectScalar(const PixelProjection& p, const float* xs, const float* ys, std::size_t count,
                   std::int32_t* px, std::int32_t* py)
{
    for (std::size_t i = 0; i < count; ++i) {
        const double sx = p.halfWidth + (static_cast<double>(xs[i]) - p.centreX) * p.scale;
        const double sy = p.halfHeight - (static_cast<double>(ys[i]) - p.centreY) * p.scale;
        if (sx > -1.0e6 && sx < 1.0e6 && sy > -1.0e6 && sy < 1.0e6) {
            px[i] = static_cast<std::int32_t>(sx);
            py[i] = static_cast<std::int32_t>(sy);
        } else {
            px[i] = kOffImage;
            py[i] = kOffImage;
        }
    }
}

void project(const PixelProjection& p, const float* xs, const float* ys, std::size_t count,
             std::int32_t* px, std::int32_t* py)
{
#if defined(KATANA_HAVE_AVX2_KERNELS)
    if (count >= kProjectMinimum &&
        katana::core::activeSimdLevel() == katana::core::SimdLevel::Avx2) {
        static_assert(sizeof(PixelProjection) == 5 * sizeof(double));
        katana_avx2_project_to_pixels(&p.halfWidth, xs, ys, count, px, py);
        return;
    }
#endif
    projectScalar(p, xs, ys, count, px, py);
}

// Orders one tile's points coarse to fine. Level L takes, in each cell of a
// 2^L x 2^L grid over the tile, the first point in the order given that no
// coarser level took; points no level takes come last. So the first few
// points of a tile are spread over all of it, whatever order the file had
// them in. The tile's points are contiguous (fx, fy, n); `order` receives
// their positions in the new order.
void orderByLevel(const float* fx, const float* fy, std::size_t n, double x0, double y0,
                  double width, double height, std::vector<std::uint32_t>& order)
{
    order.resize(n);
    for (std::size_t k = 0; k < n; ++k) {
        order[k] = static_cast<std::uint32_t>(k);
    }
    if (n <= 1) {
        return;
    }
    int deepest = 0;
    while (deepest < kMaximumLevel && (std::size_t{1} << (2 * deepest)) < n) {
        ++deepest;
    }
    // Each point's cell at the deepest level, once; a coarser level's cell is
    // that shifted down.
    const int cells = 1 << deepest;
    const auto cellOf = [cells](double offset, double extent) {
        const double t = extent > 0.0 ? offset / extent : 0.0;
        return static_cast<std::uint32_t>(std::clamp(static_cast<int>(t * cells), 0, cells - 1));
    };
    std::vector<std::uint32_t> cx(n);
    std::vector<std::uint32_t> cy(n);
    for (std::size_t k = 0; k < n; ++k) {
        cx[k] = cellOf(static_cast<double>(fx[k]) - x0, width);
        cy[k] = cellOf(static_cast<double>(fy[k]) - y0, height);
    }
    std::vector<std::uint8_t> level(n, static_cast<std::uint8_t>(deepest + 1));
    std::vector<std::uint8_t> taken;
    std::size_t left = n;
    for (int l = 0; l <= deepest && left > 0; ++l) {
        const int shift = deepest - l;
        taken.assign(std::size_t{1} << (2 * l), 0);
        for (std::size_t k = 0; k < n; ++k) {
            if (level[k] <= deepest) {
                continue;
            }
            std::uint8_t& cell = taken[(static_cast<std::size_t>(cy[k] >> shift) << l) +
                                       static_cast<std::size_t>(cx[k] >> shift)];
            if (cell == 0) {
                cell = 1;
                level[k] = static_cast<std::uint8_t>(l);
                --left;
            }
        }
    }
    // Stable by level: within a level the order given, which is the file's.
    std::array<std::uint32_t, kMaximumLevel + 3> start{};
    for (std::size_t k = 0; k < n; ++k) {
        ++start[level[k] + 1U];
    }
    for (std::size_t l = 1; l < start.size(); ++l) {
        start[l] += start[l - 1];
    }
    for (std::size_t k = 0; k < n; ++k) {
        order[start[level[k]]++] = static_cast<std::uint32_t>(k);
    }
}

} // namespace

void projectToPixels(const PixelProjection& projection, std::span<const float> xs,
                     std::span<const float> ys, std::span<std::int32_t> px,
                     std::span<std::int32_t> py)
{
    const std::size_t count = std::min({xs.size(), ys.size(), px.size(), py.size()});
    project(projection, xs.data(), ys.data(), count, px.data(), py.data());
}

SplatCloud buildSplatCloud(const double* x, const double* y, std::size_t strideBytes,
                           std::size_t count, const std::uint32_t* colours,
                           katana::core::TaskPool& pool)
{
    SplatCloud cloud;
    cloud.sourceCount = count;
    const auto xAt = [&](std::size_t i) {
        double v = 0.0;
        std::memcpy(&v, reinterpret_cast<const unsigned char*>(x) + i * strideBytes, sizeof v);
        return v;
    };
    const auto yAt = [&](std::size_t i) {
        double v = 0.0;
        std::memcpy(&v, reinterpret_cast<const unsigned char*>(y) + i * strideBytes, sizeof v);
        return v;
    };

    Box2 bounds;
    for (std::size_t i = 0; i < count; ++i) {
        const double px = xAt(i);
        const double py = yAt(i);
        if (std::isfinite(px) && std::isfinite(py)) {
            bounds.expand(Point2(px, py));
        }
    }
    if (bounds.empty()) {
        return cloud;
    }
    cloud.origin = Point2(0.5 * (bounds.min.x + bounds.max.x), 0.5 * (bounds.min.y + bounds.max.y));

    // Offsets in the file's order, and which points they are.
    std::vector<float> fx;
    std::vector<float> fy;
    std::vector<std::uint32_t> source;
    fx.reserve(count);
    fy.reserve(count);
    source.reserve(count);
    float minX = std::numeric_limits<float>::infinity();
    float minY = minX;
    float maxX = -minX;
    float maxY = -minX;
    for (std::size_t i = 0; i < count; ++i) {
        const double px = xAt(i);
        const double py = yAt(i);
        if (!std::isfinite(px) || !std::isfinite(py)) {
            continue;
        }
        const auto ox = static_cast<float>(px - cloud.origin.x);
        const auto oy = static_cast<float>(py - cloud.origin.y);
        if (!std::isfinite(ox) || !std::isfinite(oy)) {
            continue;
        }
        fx.push_back(ox);
        fy.push_back(oy);
        source.push_back(static_cast<std::uint32_t>(i));
        minX = std::min(minX, ox);
        minY = std::min(minY, oy);
        maxX = std::max(maxX, ox);
        maxY = std::max(maxY, oy);
    }
    const std::size_t n = fx.size();
    if (n == 0) {
        return cloud;
    }

    // A grid of about n / kSplatTilePoints tiles, roughly square on the ground
    // whatever the cloud's shape, so a corridor scan is cut along its length.
    const double width = static_cast<double>(maxX) - static_cast<double>(minX);
    const double height = static_cast<double>(maxY) - static_cast<double>(minY);
    const double wanted = std::max(1.0, static_cast<double>(n) / static_cast<double>(kSplatTilePoints));
    int columns = 1;
    int rows = 1;
    if (width > 0.0 && height > 0.0) {
        columns = std::clamp(static_cast<int>(std::lround(std::sqrt(wanted * width / height))), 1, 4096);
        rows = std::clamp(static_cast<int>(std::lround(wanted / columns)), 1, 4096);
    } else if (width > 0.0) {
        columns = std::clamp(static_cast<int>(std::lround(wanted)), 1, 4096);
    } else if (height > 0.0) {
        rows = std::clamp(static_cast<int>(std::lround(wanted)), 1, 4096);
    }
    const double tileWidth = width / columns;
    const double tileHeight = height / rows;
    const auto tileOf = [&](std::size_t k) {
        const auto index = [](double offset, double extent, int cells) {
            const double t = extent > 0.0 ? offset / extent : 0.0;
            return std::clamp(static_cast<int>(t * cells), 0, cells - 1);
        };
        const int column = index(static_cast<double>(fx[k]) - minX, width, columns);
        // Row 0 at the top (largest y), as the image's rows run.
        const int row = rows - 1 - index(static_cast<double>(fy[k]) - minY, height, rows);
        return static_cast<std::size_t>(row) * static_cast<std::size_t>(columns) +
               static_cast<std::size_t>(column);
    };

    // Counting sort by tile, keeping the file's order within a tile.
    const std::size_t tileCount = static_cast<std::size_t>(columns) * static_cast<std::size_t>(rows);
    std::vector<std::uint32_t> start(tileCount + 1, 0);
    std::vector<std::uint32_t> tileOfPoint(n);
    for (std::size_t k = 0; k < n; ++k) {
        tileOfPoint[k] = static_cast<std::uint32_t>(tileOf(k));
        ++start[tileOfPoint[k] + 1];
    }
    for (std::size_t t = 0; t < tileCount; ++t) {
        start[t + 1] += start[t];
    }
    // The points gathered tile by tile, each tile in the file's order: the
    // level-of-detail pass then reads each tile's points contiguously.
    std::vector<float> gx(n);
    std::vector<float> gy(n);
    std::vector<std::uint32_t> gsource(n);
    {
        std::vector<std::uint32_t> next(start.begin(), start.end() - 1);
        for (std::size_t k = 0; k < n; ++k) {
            const std::uint32_t at = next[tileOfPoint[k]]++;
            gx[at] = fx[k];
            gy[at] = fy[k];
            gsource[at] = source[k];
        }
    }

    cloud.xs.resize(n);
    cloud.ys.resize(n);
    cloud.colours.resize(n);
    std::vector<SplatTile> tiles(tileCount);
    // Tiles are independent and each writes only its own range, so the copy
    // is the same whatever the number of threads.
    pool.parallelRanges(0, tileCount, 16, [&](std::size_t firstTile, std::size_t lastTile) {
        std::vector<std::uint32_t> order;
        for (std::size_t t = firstTile; t < lastTile; ++t) {
            const std::uint32_t begin = start[t];
            const std::uint32_t end = start[t + 1];
            SplatTile& tile = tiles[t];
            tile.begin = begin;
            tile.count = end - begin;
            if (begin == end) {
                continue;
            }
            const auto row = static_cast<int>(t / static_cast<std::size_t>(columns));
            const auto column = static_cast<int>(t % static_cast<std::size_t>(columns));
            orderByLevel(gx.data() + begin, gy.data() + begin, end - begin,
                         static_cast<double>(minX) + column * tileWidth,
                         static_cast<double>(minY) + (rows - 1 - row) * tileHeight, tileWidth,
                         tileHeight, order);
            tile.minX = std::numeric_limits<float>::infinity();
            tile.minY = tile.minX;
            tile.maxX = -tile.minX;
            tile.maxY = -tile.minX;
            for (std::uint32_t k = 0; k < tile.count; ++k) {
                const std::uint32_t from = begin + order[k];
                const float ox = gx[from];
                const float oy = gy[from];
                cloud.xs[begin + k] = ox;
                cloud.ys[begin + k] = oy;
                cloud.colours[begin + k] = colours[gsource[from]];
                tile.minX = std::min(tile.minX, ox);
                tile.minY = std::min(tile.minY, oy);
                tile.maxX = std::max(tile.maxX, ox);
                tile.maxY = std::max(tile.maxY, oy);
            }
        }
    });
    for (const SplatTile& tile : tiles) {
        if (tile.count > 0) {
            cloud.tiles.push_back(tile);
        }
    }
    return cloud;
}

SplatStats splatCloud(const SplatCloud& cloud, const SplatView& view, std::uint32_t* pixels,
                      std::size_t stride, katana::core::TaskPool& pool)
{
    SplatStats stats;
    if (cloud.size() == 0 || view.width <= 0 || view.height <= 0 || !(view.scale > 0.0) ||
        !std::isfinite(view.scale)) {
        return stats;
    }
    const PixelProjection p{0.5 * view.width, 0.5 * view.height, view.centre.x - cloud.origin.x,
                            view.centre.y - cloud.origin.y, view.scale};
    const int radius = std::max(0, view.radius);
    const int width = view.width;
    const int height = view.height;

    // The tiles that reach the image, and the rows their points can touch. A
    // point is drawn when its pixel is inside the image, -1 < sx < width and
    // -1 < sy < height; projecting a tile's bounds with the points' own
    // arithmetic bounds every point's sx and sy, so no tile skipped here could
    // have drawn anything.
    struct Visible {
        std::uint32_t tile;
        std::uint32_t draw;
        int rowLo;
        int rowHi; // inclusive
    };
    std::vector<Visible> visible;
    int rowBegin = height;
    int rowEnd = 0;
    for (std::size_t t = 0; t < cloud.tiles.size(); ++t) {
        const SplatTile& tile = cloud.tiles[t];
        const double sxLo = p.halfWidth + (static_cast<double>(tile.minX) - p.centreX) * p.scale;
        const double sxHi = p.halfWidth + (static_cast<double>(tile.maxX) - p.centreX) * p.scale;
        const double syLo = p.halfHeight - (static_cast<double>(tile.maxY) - p.centreY) * p.scale;
        const double syHi = p.halfHeight - (static_cast<double>(tile.minY) - p.centreY) * p.scale;
        if (!(sxHi > -1.0 && sxLo < width && syHi > -1.0 && syLo < height)) {
            continue;
        }
        const int pyLo = syLo <= 0.0 ? 0 : static_cast<int>(syLo);
        const int pyHi = syHi >= height ? height - 1 : static_cast<int>(syHi);
        const int rowLo = std::max(0, pyLo - radius);
        const int rowHi = std::min(height - 1, pyHi + radius);
        visible.push_back({static_cast<std::uint32_t>(t), tile.count, rowLo, rowHi});
        stats.pointsInView += tile.count;
        rowBegin = std::min(rowBegin, rowLo);
        rowEnd = std::max(rowEnd, rowHi + 1);
    }
    if (visible.empty()) {
        return stats;
    }
    // Over budget: every tile draws the same share of its points, rounded up,
    // its coarse levels first.
    if (stats.pointsInView > view.pointBudget) {
        const auto budget = static_cast<unsigned long long>(view.pointBudget);
        const auto total = static_cast<unsigned long long>(stats.pointsInView);
        for (Visible& v : visible) {
            v.draw = static_cast<std::uint32_t>((v.draw * budget + total - 1) / total);
        }
    }
    for (const Visible& v : visible) {
        stats.pointsDrawn += v.draw;
    }
    stats.rowBegin = rowBegin;
    stats.rowEnd = rowEnd;

    const auto band = [&](int lo, int hi) {
        for (int row = lo; row < hi; ++row) {
            std::memset(pixels + static_cast<std::size_t>(row) * stride, 0,
                        static_cast<std::size_t>(width) * sizeof(std::uint32_t));
        }
        std::array<std::int32_t, kBlock> px{};
        std::array<std::int32_t, kBlock> py{};
        for (const Visible& v : visible) {
            if (v.rowHi < lo || v.rowLo >= hi) {
                continue;
            }
            const SplatTile& tile = cloud.tiles[v.tile];
            for (std::uint32_t first = 0; first < v.draw; first += kBlock) {
                const std::size_t n = std::min<std::size_t>(kBlock, v.draw - first);
                const std::size_t base = tile.begin + first;
                project(p, cloud.xs.data() + base, cloud.ys.data() + base, n, px.data(), py.data());
                const std::uint32_t* colour = cloud.colours.data() + base;
                if (radius == 0) {
                    for (std::size_t j = 0; j < n; ++j) {
                        const std::int32_t y = py[j];
                        const std::int32_t x = px[j];
                        if (y >= lo && y < hi && x >= 0 && x < width) {
                            pixels[static_cast<std::size_t>(y) * stride + static_cast<std::size_t>(x)] =
                                colour[j];
                        }
                    }
                    continue;
                }
                for (std::size_t j = 0; j < n; ++j) {
                    const std::int32_t y = py[j];
                    const std::int32_t x = px[j];
                    // The centre pixel must be on the image, as it always had
                    // to be; then the square, clipped to the image and band.
                    if (x < 0 || x >= width || y < 0 || y >= height || y + radius < lo ||
                        y - radius >= hi) {
                        continue;
                    }
                    const int y0 = std::max(lo, y - radius);
                    const int y1 = std::min(hi - 1, y + radius);
                    const int x0 = std::max(0, x - radius);
                    const int x1 = std::min(width - 1, x + radius);
                    for (int yy = y0; yy <= y1; ++yy) {
                        std::uint32_t* line = pixels + static_cast<std::size_t>(yy) * stride;
                        for (int xx = x0; xx <= x1; ++xx) {
                            line[xx] = colour[j];
                        }
                    }
                }
            }
        }
    };

    const int rows = rowEnd - rowBegin;
    const std::size_t threads = stats.pointsDrawn < kParallelMinimum ? 1 : pool.concurrency();
    const auto bands = static_cast<std::size_t>(
        std::max(1, std::min(static_cast<int>(threads), rows / kMinimumBandRows)));
    if (bands == 1) {
        band(rowBegin, rowEnd);
        return stats;
    }
    pool.parallelFor(0, bands, [&](std::size_t b) {
        const auto lo = rowBegin + static_cast<int>(static_cast<std::size_t>(rows) * b / bands);
        const auto hi = rowBegin + static_cast<int>(static_cast<std::size_t>(rows) * (b + 1) / bands);
        band(lo, hi);
    });
    return stats;
}

} // namespace katana::geometry
