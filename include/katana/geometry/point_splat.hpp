#pragma once

// Point clouds drawn as pixels: the plan view's cloud splat, without Qt.
//
// A cloud is prepared once into a display copy (buildSplatCloud) and splatted
// into a 32-bit image every frame (splatCloud). The copy is what makes a frame
// cheap:
//
// - FLOAT OFFSETS FROM A DOUBLE ORIGIN, as structure-of-arrays: 12 bytes a
//   point (x, y, colour) where the stored cloud has 40, and x and y side by
//   side for the AVX2 projection. The origin is the middle of the cloud, so
//   a float keeps a 2 km cloud to about 0.06 mm; the projection itself is in
//   double. The stored cloud is not changed.
// - TILES: a grid over the cloud, each tile's points contiguous, so a frame
//   skips tiles off the image and each image band visits only the tiles that
//   reach it.
// - LEVEL OF DETAIL within each tile: the tile's points ordered coarse to fine
//   (the first point in each cell of a 1x1, 2x2, 4x4 ... grid, then the rest),
//   so any prefix of a tile is an even thinning of it. That is what the point
//   budget draws when more points are in view than it allows.
//
// WHICH POINT WINS A PIXEL. Where points overlap, the one later in the display
// copy's order is on top: tile by tile, and within a tile finer levels over
// coarser, then later points over earlier. The image is split into bands of
// rows, one TaskPool task each; a band writes only its own rows and visits
// points in that same order, so every pixel sees the same sequence of writes
// whatever the number of threads (Rule 7). The old splat drew in the file's
// order; see docs/performance.md for what the new order and the float
// offsets change, and by how much.

#include <cstddef>
#include <cstdint>
#include <limits>
#include <span>
#include <vector>

#include "katana/geometry/primitives2d.hpp"

namespace katana::core {
class TaskPool;
}

namespace katana::geometry {

// One tile of a display copy: points [begin, begin + count), and the bounds of
// their STORED float offsets. The projection is monotone in each coordinate,
// so projecting the bounds bounds every point's pixel exactly.
struct SplatTile {
    std::uint32_t begin = 0;
    std::uint32_t count = 0;
    float minX = 0.0F;
    float minY = 0.0F;
    float maxX = 0.0F;
    float maxY = 0.0F;
};

struct SplatCloud {
    Point2 origin;
    std::vector<float> xs; // offsets from origin, in display order
    std::vector<float> ys;
    std::vector<std::uint32_t> colours; // written to the image as they are
    std::vector<SplatTile> tiles;
    std::size_t sourceCount = 0; // points given, including any left out

    [[nodiscard]] std::size_t size() const { return xs.size(); }
};

// About this many points a tile. Small enough that a tile rarely straddles
// two bands (each band re-projects the tiles it shares), large enough that a
// frame's per-tile work is noise beside the points'.
inline constexpr std::size_t kSplatTilePoints = 1024;

// The display copy of `count` points whose x and y are read from `x` and `y`,
// stepping `strideBytes` from one point to the next (an array of structs is
// read in place), and whose colours are colours[i]. A point whose x or y is
// not finite, or whose offset from the origin is beyond a float, cannot be
// drawn and is left out. The tiles are ordered across `pool`; the copy is the
// same at every thread count.
[[nodiscard]] SplatCloud buildSplatCloud(const double* x, const double* y,
                                         std::size_t strideBytes, std::size_t count,
                                         const std::uint32_t* colours,
                                         katana::core::TaskPool& pool);

// Points projected a frame when more than this many lie in tiles on the image:
// then each such tile draws the same share of its points, coarse levels first.
// Measured in docs/performance.md: about what a frame can splat in 25-30 ms,
// and more points than a 1600 x 1000 view has pixels.
inline constexpr std::size_t kSplatPointBudget = 4'000'000;

struct SplatView {
    Point2 centre;       // in model units, drawn at the image's centre
    double scale = 1.0;  // pixels per model unit, y up in the model and down in the image
    int width = 0;
    int height = 0;
    int radius = 0;      // each point a (2 radius + 1)-pixel square, clipped to the image
    std::size_t pointBudget = kSplatPointBudget;
};

struct SplatStats {
    std::size_t pointsInView = 0; // in the tiles that reach the image
    std::size_t pointsDrawn = 0;  // projected: all of those, or the budget's share
    // The rows cleared and drawn; rows outside are left as they were, so an
    // image kept between frames need only be composited over this range.
    int rowBegin = 0;
    int rowEnd = 0;
};

// Clears rows [rowBegin, rowEnd) of `pixels` (stride in pixels) to zero and
// splats the cloud into them: a point whose pixel is inside the image gets
// its colour, and so do the pixels of its square. The same image at every
// thread count and SIMD level.
SplatStats splatCloud(const SplatCloud& cloud, const SplatView& view, std::uint32_t* pixels,
                      std::size_t stride, katana::core::TaskPool& pool);

// ---- the projection, on its own for tests and benchmarks ----------------

// A pixel coordinate that is not one: off any image this code draws.
inline constexpr std::int32_t kOffImage = std::numeric_limits<std::int32_t>::min();

struct PixelProjection {
    double halfWidth = 0.0;
    double halfHeight = 0.0;
    double centreX = 0.0; // the view's centre as an offset from the cloud's origin
    double centreY = 0.0;
    double scale = 1.0;
};

// For each i: sx = halfWidth + (xs[i] - centreX) * scale and
// sy = halfHeight - (ys[i] - centreY) * scale, in double; px[i], py[i] are them
// truncated towards zero when both lie strictly inside (-1e6, 1e6), and both
// kOffImage otherwise, NaN included. (Converting a coordinate outside int
// range is undefined behaviour, and panning a map-grid cloud when zoomed in
// produces exactly such values.) The AVX2 kernel from kProjectMinimum points,
// bit-identical to the loop.
void projectToPixels(const PixelProjection& projection, std::span<const float> xs,
                     std::span<const float> ys, std::span<std::int32_t> px,
                     std::span<std::int32_t> py);

} // namespace katana::geometry
