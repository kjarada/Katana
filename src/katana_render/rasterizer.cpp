#include "katana/render/rasterizer.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <string>

#include "katana/core/task_pool.hpp"

namespace katana::render {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::TaskPool;

namespace {

// Chunking is deliberately a function of the workload and NOT of the core
// count: the bucket a primitive lands in decides the order a tile visits it,
// and a frame that depends on how many cores the machine has is not
// reproducible (Rule 7). 4096 primitives is large enough that the per-chunk
// bookkeeping disappears and small enough to keep every core fed on the sizes
// that matter.
constexpr std::size_t kPrimitivesPerChunk = 4096;
constexpr std::size_t kMaxChunks = 64;

[[nodiscard]] std::size_t chunkCountFor(std::size_t primitives)
{
    if (primitives == 0) {
        return 0;
    }
    const std::size_t wanted = (primitives + kPrimitivesPerChunk - 1) / kPrimitivesPerChunk;
    return std::min(wanted, kMaxChunks);
}

// Interpolates a clip-space vertex towards `b` by t. Colour is interpolated in
// clip space, which is where the near-plane split happens; the perspective
// correction that matters happens later, per pixel.
[[nodiscard]] Rgba lerpColor(Rgba a, Rgba b, float t)
{
    const auto mix = [t](std::uint8_t lo, std::uint8_t hi) {
        return static_cast<std::uint8_t>(static_cast<float>(lo) +
                                         (static_cast<float>(hi) - static_cast<float>(lo)) * t +
                                         0.5f);
    };
    return (static_cast<Rgba>(mix(alphaOf(a), alphaOf(b))) << 24) |
           (static_cast<Rgba>(mix(redOf(a), redOf(b))) << 16) |
           (static_cast<Rgba>(mix(greenOf(a), greenOf(b))) << 8) |
           static_cast<Rgba>(mix(blueOf(a), blueOf(b)));
}

// Clipping happens in clip space, before the divide, against five planes. A
// vertex is inside where every planeDistance() below is >= 0.
//
// The NEAR plane is clip z >= 0. With the Vulkan depth range camera.cpp builds
// (z/w = 0 at the near plane), that is exactly w >= near for a perspective
// projection and z_eye <= -near for an orthographic one, and setDepthRange()
// refuses near <= 0, so every kept vertex has w > 0 and the divide is safe.
// This used to clip at w > 1e-6 instead, which kept the reciprocal finite and
// nothing else: a vertex cut there projects with 1/w = 1e6 to a screen
// coordinate beyond INT_MAX, and the float-to-int conversion in the binning is
// then undefined. On x86-64 it yields INT_MIN, so a triangle or line crossing
// the eye plane lost its VISIBLE part as well, in any view taller than about
// 213 px - the ground below a perspective eye (audit of 2026-09-23).
//
// The four GUARD-BAND planes |x| <= kGuardBand * w and |y| <= kGuardBand * w
// bound every projected coordinate to within half a viewport of the image, so
// no screen coordinate can overflow whatever the input, and the edge functions
// keep the precision of a viewport-sized triangle. The band is wider than the
// image (1) for two reasons: a point or a wide line centred just outside the
// image still reaches into it and must not be rejected, and a triangle that
// only just overhangs the image takes the pass-through path rather than being
// cut. Wider buys nothing and costs precision: an edge function's error is
// about the triangle's extent in pixels times 2^-24, a few ten-thousandths of a
// pixel at this band on a 2000 px image and over a pixel at ten thousand
// viewports.
//
// Where a triangle or line does cross a plane, the cut is computed in DOUBLE.
// The inputs are floats, so every plane distance is exact in double; in float,
// 2 + (-3.1e8) loses the 2, the parameter of the cut comes out at exactly 0.5,
// and the cut vertex lands at the image centre instead of on the band. A cut
// vertex is small even when its endpoints are enormous, so rounding it back to
// float at the end costs nothing.
//
// The far plane is not clipped: nothing numerical goes wrong beyond it, and the
// per-pixel depth test already rejects depth > 1.
constexpr float kGuardBand = 2.0f;
constexpr std::size_t kClipPlanes = 5;

// Signed distance of a clip-space vertex from `plane`, >= 0 inside. Double for
// the reason just given; the inputs are the float clip coordinates, so the
// stage-1 classification and a stage-2 cut agree on which side a vertex is.
[[nodiscard]] double planeDistance(float x, float y, float z, float w, std::size_t plane)
{
    const double band = kGuardBand * static_cast<double>(w);
    switch (plane) {
    case 0:
        return static_cast<double>(z);
    case 1:
        return band + static_cast<double>(x);
    case 2:
        return band - static_cast<double>(x);
    case 3:
        return band + static_cast<double>(y);
    default:
        return band - static_cast<double>(y);
    }
}

// Bit p set when the vertex is outside plane p. A NaN distance counts as
// outside, so a vertex with a NaN in it is never passed through unclipped.
[[nodiscard]] std::uint8_t clipCodeOf(float x, float y, float z, float w)
{
    unsigned code = 0;
    for (std::size_t plane = 0; plane < kClipPlanes; ++plane) {
        if (!(planeDistance(x, y, z, w, plane) >= 0.0)) {
            code |= 1u << plane;
        }
    }
    return static_cast<std::uint8_t>(code);
}

} // namespace

// ---- stage 1: transform ---------------------------------------------------------

void Rasterizer::transformVertices(const DrawList& list, const Camera& camera, TaskPool& pool)
{
    const katana::math::Mat4 mvp = camera.viewProjection();
    clip_.resize(list.positions.size());
    clipCodes_.resize(list.positions.size());

    // Read straight out of the SoA position array. Nothing else is touched, so
    // this streams at memory bandwidth rather than striding over colours.
    const std::size_t count = list.positions.size();
    pool.parallelRanges(0, count, 4096, [&](std::size_t lo, std::size_t hi) {
        for (std::size_t i = lo; i < hi; ++i) {
            const Point3& p = list.positions[i];
            const katana::math::Vec4 c = mvp * katana::math::Vec4(p.x, p.y, p.z, 1.0);
            ClipVertex& out = clip_[i];
            out.x = static_cast<float>(c.x);
            out.y = static_cast<float>(c.y);
            out.z = static_cast<float>(c.z);
            out.w = static_cast<float>(c.w);
            out.color = i < list.colors.size() ? list.colors[i] : rgba(255, 255, 255);
            clipCodes_[i] = clipCodeOf(out.x, out.y, out.z, out.w);
        }
    });
}

// ---- stage 2: clip, project, widen, bin ----------------------------------------

namespace {

struct Viewport {
    float halfWidth = 0.0f;
    float halfHeight = 0.0f;
    float width = 0.0f;
    float height = 0.0f;
};

struct ProjectedVertex {
    float x = 0.0f;
    float y = 0.0f;
    float z = 0.0f;
    float invW = 0.0f;
    Rgba color = 0;
};

// Floor of a screen coordinate, clamped to [lo, hi] BEFORE the conversion to
// int, because converting a float outside int's range is undefined behaviour.
// NaN clamps to lo. After clipping every coordinate is inside the guard band,
// so this is the last line of defence rather than the mechanism - except for
// a point's half-size, which comes straight from the draw list.
[[nodiscard]] int pixelFloor(float value, int lo, int hi)
{
    const float f = std::floor(value);
    if (!(f >= static_cast<float>(lo))) {
        return lo;
    }
    if (f >= static_cast<float>(hi)) {
        return hi;
    }
    return static_cast<int>(f);
}

} // namespace

void Rasterizer::buildScreenPrimitives(const DrawList& list, const Framebuffer& target,
                                       const RenderOptions& options, TaskPool& pool)
{
    const std::size_t primitiveCount =
        list.triangles.size() + list.lines.size() + list.points.size();
    const std::size_t chunkCount = chunkCountFor(primitiveCount);

    chunks_.resize(chunkCount);
    for (Chunk& chunk : chunks_) {
        chunk.triangles.clear();
        chunk.points.clear();
        chunk.stats = RenderStats{};
    }
    if (chunkCount == 0) {
        return;
    }

    Viewport viewport;
    viewport.width = static_cast<float>(target.width());
    viewport.height = static_cast<float>(target.height());
    viewport.halfWidth = viewport.width * 0.5f;
    viewport.halfHeight = viewport.height * 0.5f;

    // The clipping helpers are lambdas only because ClipVertex is private to
    // the class; the planes, and why the arithmetic is double, are described at
    // kGuardBand.
    const auto distance = [](const ClipVertex& v, std::size_t plane) {
        return planeDistance(v.x, v.y, v.z, v.w, plane);
    };
    const auto isFinite = [](const ClipVertex& v) {
        return std::isfinite(v.x) && std::isfinite(v.y) && std::isfinite(v.z) &&
               std::isfinite(v.w);
    };
    // Clip space is where interpolation is linear, so a cut vertex is a lerp.
    const auto lerpClip = [](const ClipVertex& a, const ClipVertex& b, double t) {
        const auto mix = [t](float from, float to) {
            const double lo = static_cast<double>(from);
            return static_cast<float>(lo + (static_cast<double>(to) - lo) * t);
        };
        ClipVertex out;
        out.x = mix(a.x, b.x);
        out.y = mix(a.y, b.y);
        out.z = mix(a.z, b.z);
        out.w = mix(a.w, b.w);
        out.color = lerpColor(a.color, b.color, static_cast<float>(t));
        return out;
    };

    const auto project = [&viewport](const ClipVertex& v) {
        // Only ever given a vertex that survived clipping, so w >= near > 0.
        ProjectedVertex out;
        const float invW = 1.0f / v.w;
        out.x = (v.x * invW * 0.5f + 0.5f) * viewport.width;
        out.y = (0.5f - v.y * invW * 0.5f) * viewport.height; // screen +y is down
        out.z = v.z * invW;
        out.invW = invW;
        out.color = v.color;
        return out;
    };

    const bool cull = options.backfaceCull;

    // The bin step needs each chunk's own triangle/point index, so the chunk
    // boundaries are over the combined primitive stream: triangles first, then
    // lines, then points, each chunk taking a contiguous slice.
    const std::size_t perChunk = (primitiveCount + chunkCount - 1) / chunkCount;

    pool.parallelFor(0, chunkCount, [&](std::size_t chunkIndex) {
        Chunk& chunk = chunks_[chunkIndex];
        const std::size_t first = chunkIndex * perChunk;
        const std::size_t last = std::min(first + perChunk, primitiveCount);
        if (first >= last) {
            return;
        }

        // Fans a convex polygon whose every vertex is inside all five planes
        // into screen triangles.
        const auto emitPolygon = [&](const ClipVertex* polygon, std::size_t count,
                                     float depthBias) {
            for (std::size_t i = 1; i + 1 < count; ++i) {
                const ProjectedVertex p0 = project(polygon[0]);
                const ProjectedVertex p1 = project(polygon[i]);
                const ProjectedVertex p2 = project(polygon[i + 1]);

                const float area = (p1.x - p0.x) * (p2.y - p0.y) - (p2.x - p0.x) * (p1.y - p0.y);
                if (!(std::abs(area) > 0.0f)) {
                    continue; // zero area or NaN: nothing to fill
                }
                if (cull && area >= 0.0f) {
                    // Screen +y points down, so a counter-clockwise world
                    // triangle has a NEGATIVE screen-space area. Cull the other
                    // sign.
                    continue;
                }
                ScreenTriangle screen;
                screen.x[0] = p0.x;
                screen.y[0] = p0.y;
                screen.z[0] = p0.z;
                screen.invW[0] = p0.invW;
                screen.color[0] = p0.color;
                screen.x[1] = p1.x;
                screen.y[1] = p1.y;
                screen.z[1] = p1.z;
                screen.invW[1] = p1.invW;
                screen.color[1] = p1.color;
                screen.x[2] = p2.x;
                screen.y[2] = p2.y;
                screen.z[2] = p2.z;
                screen.invW[2] = p2.invW;
                screen.color[2] = p2.color;
                screen.depthBias = depthBias;
                chunk.triangles.push_back(screen);
                ++chunk.stats.trianglesRasterised;
            }
        };

        // A triangle that crosses at least one plane: Sutherland-Hodgman, one
        // plane at a time, into a convex polygon of up to eight vertices (each
        // plane adds at most one), then fanned.
        const auto emitClippedTriangle = [&](const std::array<ClipVertex, 3>& v, float depthBias) {
            // A non-finite coordinate has no position to clip, and a cut
            // interpolated from one is NaN: draw nothing rather than part.
            if (!isFinite(v[0]) || !isFinite(v[1]) || !isFinite(v[2])) {
                return;
            }
            std::array<ClipVertex, 3 + kClipPlanes> polygon{v[0], v[1], v[2]};
            std::array<ClipVertex, 3 + kClipPlanes> scratch{};
            std::size_t count = 3;
            // NEAR FIRST: once every vertex has z >= 0 every w is positive, so
            // each later cut interpolates between vertices the divide survives.
            for (std::size_t plane = 0; plane < kClipPlanes && count >= 3; ++plane) {
                std::size_t kept = 0;
                for (std::size_t i = 0; i < count; ++i) {
                    const ClipVertex& current = polygon[i];
                    const ClipVertex& next = polygon[(i + 1) % count];
                    const double dc = distance(current, plane);
                    const double dn = distance(next, plane);
                    const bool currentIn = dc >= 0.0;
                    const bool nextIn = dn >= 0.0;
                    if (currentIn) {
                        scratch[kept++] = current;
                    }
                    if (currentIn != nextIn) {
                        // dc and dn have opposite signs, so the denominator is
                        // never zero and t is in [0, 1].
                        ClipVertex cutVertex = lerpClip(current, next, dc / (dc - dn));
                        if (plane == 0) {
                            cutVertex.z = 0.0f; // on the near plane exactly: depth 0
                        }
                        scratch[kept++] = cutVertex;
                    }
                }
                std::copy_n(scratch.begin(), kept, polygon.begin());
                count = kept;
            }
            if (count >= 3) {
                emitPolygon(polygon.data(), count, depthBias);
            }
        };

        for (std::size_t index = first; index < last; ++index) {
            if (index < list.triangles.size()) {
                const DrawTriangle& t = list.triangles[index];
                if (t.a >= clip_.size() || t.b >= clip_.size() || t.c >= clip_.size()) {
                    continue;
                }
                ++chunk.stats.trianglesSubmitted;
                const unsigned ca = clipCodes_[t.a];
                const unsigned cb = clipCodes_[t.b];
                const unsigned cc = clipCodes_[t.c];
                if ((ca & cb & cc) != 0u) {
                    continue; // all three outside one plane: nothing of it can show
                }
                const std::array<ClipVertex, 3> v{clip_[t.a], clip_[t.b], clip_[t.c]};
                if ((ca | cb | cc) == 0u) {
                    emitPolygon(v.data(), 3, 0.0f); // the common case: nothing to cut
                } else {
                    emitClippedTriangle(v, 0.0f);
                }
                continue;
            }
            const std::size_t lineIndex = index - list.triangles.size();
            if (lineIndex < list.lines.size()) {
                const DrawLine& line = list.lines[lineIndex];
                if (line.a >= clip_.size() || line.b >= clip_.size()) {
                    continue;
                }
                ++chunk.stats.linesSubmitted;

                // Clip the segment against the same five planes (Liang-Barsky:
                // the kept part is the parameter interval [t0, t1] every plane
                // agrees on), then widen in SCREEN space so a line keeps its
                // pixel thickness at any depth, and reuse the triangle path:
                // one tested rasteriser, correct depth, correct clipping.
                const unsigned ca = clipCodes_[line.a];
                const unsigned cb = clipCodes_[line.b];
                if ((ca & cb) != 0u) {
                    continue; // both ends outside one plane
                }
                const ClipVertex& a0 = clip_[line.a];
                const ClipVertex& b0 = clip_[line.b];
                ClipVertex a = a0;
                ClipVertex b = b0;
                if ((ca | cb) != 0u) {
                    if (!isFinite(a0) || !isFinite(b0)) {
                        continue;
                    }
                    double t0 = 0.0;
                    double t1 = 1.0;
                    bool visible = true;
                    bool nearCutsA = false;
                    bool nearCutsB = false;
                    for (std::size_t plane = 0; plane < kClipPlanes && visible; ++plane) {
                        const double da = distance(a0, plane);
                        const double db = distance(b0, plane);
                        if (da < 0.0 && db < 0.0) {
                            visible = false;
                        } else if (da < 0.0) {
                            const double t = da / (da - db); // entering
                            if (t > t0) {
                                t0 = t;
                                nearCutsA = plane == 0;
                            }
                        } else if (db < 0.0) {
                            const double t = da / (da - db); // leaving
                            if (t < t1) {
                                t1 = t;
                                nearCutsB = plane == 0;
                            }
                        }
                    }
                    if (!visible || !(t0 <= t1)) {
                        continue;
                    }
                    a = lerpClip(a0, b0, t0);
                    b = lerpClip(a0, b0, t1);
                    // As for a triangle: an end cut by the near plane is ON it,
                    // depth exactly 0, not a rounding error either side.
                    if (nearCutsA) {
                        a.z = 0.0f;
                    }
                    if (nearCutsB) {
                        b.z = 0.0f;
                    }
                }
                const ProjectedVertex p0 = project(a);
                const ProjectedVertex p1 = project(b);
                float dx = p1.x - p0.x;
                float dy = p1.y - p0.y;
                const float length = std::sqrt(dx * dx + dy * dy);
                const float half = std::max(line.width, 1.0f) * 0.5f;
                if (!(length > 0.0f)) {
                    // Seen exactly end-on there is no direction to widen along;
                    // any one will do, because the square cap below turns it
                    // into a square centred on the point.
                    dx = 1.0f;
                    dy = 0.0f;
                } else {
                    dx /= length;
                    dy /= length;
                }
                const float nx = -dy * half;
                const float ny = dx * half;
                // SQUARE CAPS, extending the quad by half a width past each
                // end. Not cosmetic: without them a segment shorter than a
                // pixel encloses no pixel CENTRE and draws nothing at all, so a
                // densely surveyed string - where every segment is sub-pixel
                // once you zoom out - disappears entirely instead of drawing as
                // a line. With them the shortest possible segment still covers
                // its own width, and a vertical pipe viewed in plan reads as a
                // dot rather than vanishing.
                const float ex = dx * half;
                const float ey = dy * half;

                // Depth and 1/w come from the endpoint the corner belongs
                // to, so a widened line keeps the depth of the line itself.
                const auto corner = [](const ProjectedVertex& p, float sx, float sy) {
                    ProjectedVertex out = p;
                    out.x = p.x + sx;
                    out.y = p.y + sy;
                    return out;
                };
                const ProjectedVertex q0 = corner(p0, nx - ex, ny - ey);
                const ProjectedVertex q1 = corner(p0, -nx - ex, -ny - ey);
                const ProjectedVertex q2 = corner(p1, -nx + ex, -ny + ey);
                const ProjectedVertex q3 = corner(p1, nx + ex, ny + ey);

                const auto pushQuadTriangle = [&](const ProjectedVertex& v0,
                                                  const ProjectedVertex& v1,
                                                  const ProjectedVertex& v2) {
                    const float area =
                        (v1.x - v0.x) * (v2.y - v0.y) - (v2.x - v0.x) * (v1.y - v0.y);
                    if (!(std::abs(area) > 0.0f)) {
                        return;
                    }
                    ScreenTriangle screen;
                    const std::array<const ProjectedVertex*, 3> vs{&v0, &v1, &v2};
                    for (std::size_t k = 0; k < 3; ++k) {
                        screen.x[k] = vs[k]->x;
                        screen.y[k] = vs[k]->y;
                        screen.z[k] = vs[k]->z;
                        screen.invW[k] = vs[k]->invW;
                        screen.color[k] = vs[k]->color;
                    }
                    screen.depthBias = line.depthBias;
                    chunk.triangles.push_back(screen);
                    ++chunk.stats.trianglesRasterised;
                };
                // Never culled: a widened line has no meaningful facing.
                pushQuadTriangle(q0, q1, q2);
                pushQuadTriangle(q0, q2, q3);
                continue;
            }

            const std::size_t pointIndex = lineIndex - list.lines.size();
            if (pointIndex >= list.points.size()) {
                continue;
            }
            const DrawPoint& point = list.points[pointIndex];
            if (point.a >= clip_.size()) {
                continue;
            }
            ++chunk.stats.pointsSubmitted;
            if (clipCodes_[point.a] != 0u) {
                continue;
            }
            const ClipVertex& v = clip_[point.a];
            const ProjectedVertex p = project(v);
            ScreenPoint screen;
            screen.x = p.x;
            screen.y = p.y;
            screen.z = p.z - point.depthBias;
            screen.half = std::max(point.size, 1.0f) * 0.5f;
            screen.color = p.color;
            chunk.points.push_back(screen);
        }
    });

    binPrimitives(target, pool);
}

void Rasterizer::binPrimitives(const Framebuffer& target, TaskPool& pool)
{
    const int tilesAcross = target.tilesAcross();
    const int tilesDown = target.tilesDown();
    if (tilesAcross <= 0 || tilesDown <= 0) {
        return;
    }

    // Per chunk, so no two threads ever append to the same bin.
    const std::size_t tiles = target.tileCount();
    pool.parallelFor(0, chunks_.size(), [&](std::size_t chunkIndex) {
        Chunk& chunk = chunks_[chunkIndex];
        // Each chunk owns its bins, so resizing and clearing them here keeps
        // that cost parallel instead of serial in the caller.
        chunk.tileBins.resize(tiles);
        for (auto& bin : chunk.tileBins) {
            bin.clear();
        }

        const auto binBox = [&](float minX, float minY, float maxX, float maxY,
                                std::uint32_t tag) {
            if (!(maxX >= 0.0f) || !(maxY >= 0.0f) ||
                minX >= static_cast<float>(target.width()) ||
                minY >= static_cast<float>(target.height())) {
                return; // entirely off screen (the !(>=) form also rejects NaN)
            }
            const int x0 = pixelFloor(minX, 0, target.width() - 1) / Framebuffer::kTileSize;
            const int y0 = pixelFloor(minY, 0, target.height() - 1) / Framebuffer::kTileSize;
            const int x1 = pixelFloor(maxX, 0, target.width() - 1) / Framebuffer::kTileSize;
            const int y1 = pixelFloor(maxY, 0, target.height() - 1) / Framebuffer::kTileSize;
            for (int ty = y0; ty <= y1; ++ty) {
                for (int tx = x0; tx <= x1; ++tx) {
                    chunk.tileBins[static_cast<std::size_t>(ty) *
                                       static_cast<std::size_t>(tilesAcross) +
                                   static_cast<std::size_t>(tx)]
                        .push_back(tag);
                    ++chunk.stats.binEntries;
                }
            }
        };

        for (std::size_t i = 0; i < chunk.triangles.size(); ++i) {
            const ScreenTriangle& t = chunk.triangles[i];
            const float minX = std::min({t.x[0], t.x[1], t.x[2]});
            const float maxX = std::max({t.x[0], t.x[1], t.x[2]});
            const float minY = std::min({t.y[0], t.y[1], t.y[2]});
            const float maxY = std::max({t.y[0], t.y[1], t.y[2]});
            binBox(minX, minY, maxX, maxY, static_cast<std::uint32_t>(i));
        }
        for (std::size_t i = 0; i < chunk.points.size(); ++i) {
            const ScreenPoint& p = chunk.points[i];
            binBox(p.x - p.half, p.y - p.half, p.x + p.half, p.y + p.half,
                   static_cast<std::uint32_t>(i) | kPointTag);
        }
    });
}

// ---- stage 3: rasterise ---------------------------------------------------------

void Rasterizer::rasteriseTiles(Framebuffer& target, TaskPool& pool)
{
    const std::size_t tiles = target.tileCount();
    tileStats_.assign(tiles, RenderStats{});

    Rgba* const colorBase = target.color().data();
    float* const depthBase = target.depth().data();
    const int stride = target.width();

    pool.parallelFor(0, tiles, [&](std::size_t tileIndex) {
        const TileRect rect = target.tile(tileIndex);
        if (rect.empty()) {
            return;
        }
        RenderStats& stats = tileStats_[tileIndex];

        // Chunks in index order, primitives in index order within a chunk: the
        // visit order is a pure function of the draw list, so ties at equal
        // depth always resolve the same way (Rule 7).
        for (const Chunk& chunk : chunks_) {
            for (const std::uint32_t tag : chunk.tileBins[tileIndex]) {
                if ((tag & kPointTag) != 0u) {
                    const ScreenPoint& p = chunk.points[tag & ~kPointTag];
                    const int x0 = pixelFloor(p.x - p.half, rect.x0, rect.x1);
                    const int x1 = pixelFloor(p.x + p.half, rect.x0 - 1, rect.x1 - 1);
                    const int y0 = pixelFloor(p.y - p.half, rect.y0, rect.y1);
                    const int y1 = pixelFloor(p.y + p.half, rect.y0 - 1, rect.y1 - 1);
                    for (int y = y0; y <= y1; ++y) {
                        Rgba* row = colorBase + static_cast<std::size_t>(y) *
                                                    static_cast<std::size_t>(stride);
                        float* depthRow = depthBase + static_cast<std::size_t>(y) *
                                                          static_cast<std::size_t>(stride);
                        for (int x = x0; x <= x1; ++x) {
                            if (p.z < depthRow[x] && p.z >= 0.0f && p.z <= 1.0f) {
                                depthRow[x] = p.z;
                                row[x] = p.color;
                                ++stats.fragments;
                            }
                        }
                    }
                    continue;
                }

                const ScreenTriangle& t = chunk.triangles[tag];
                const int minX =
                    pixelFloor(std::min({t.x[0], t.x[1], t.x[2]}), rect.x0, rect.x1);
                const int maxX =
                    pixelFloor(std::max({t.x[0], t.x[1], t.x[2]}), rect.x0 - 1, rect.x1 - 1);
                const int minY =
                    pixelFloor(std::min({t.y[0], t.y[1], t.y[2]}), rect.y0, rect.y1);
                const int maxY =
                    pixelFloor(std::max({t.y[0], t.y[1], t.y[2]}), rect.y0 - 1, rect.y1 - 1);
                if (minX > maxX || minY > maxY) {
                    continue;
                }

                const float area = (t.x[1] - t.x[0]) * (t.y[2] - t.y[0]) -
                                   (t.x[2] - t.x[0]) * (t.y[1] - t.y[0]);
                if (!(std::abs(area) > 0.0f)) {
                    continue;
                }
                const float invArea = 1.0f / area;

                for (int y = minY; y <= maxY; ++y) {
                    const float py = static_cast<float>(y) + 0.5f;
                    Rgba* row =
                        colorBase + static_cast<std::size_t>(y) * static_cast<std::size_t>(stride);
                    float* depthRow =
                        depthBase + static_cast<std::size_t>(y) * static_cast<std::size_t>(stride);
                    for (int x = minX; x <= maxX; ++x) {
                        const float px = static_cast<float>(x) + 0.5f;

                        // No top-left rule: a pixel exactly on a shared edge
                        // is covered by both triangles rather than by exactly
                        // one. With an opaque, strictly-less depth test that
                        // costs a redundant write and changes no pixel, so the
                        // extra branches are not paid for. It would have to be
                        // added before any blended pass.
                        //
                        // Barycentrics from edge functions. Normalising by the
                        // signed area makes the sign test independent of
                        // winding, so a triangle is filled whichever way round
                        // it is - the caller opted in or out of culling long
                        // before this point.
                        const float w0 = ((t.x[1] - px) * (t.y[2] - py) -
                                          (t.x[2] - px) * (t.y[1] - py)) *
                                         invArea;
                        const float w1 = ((t.x[2] - px) * (t.y[0] - py) -
                                          (t.x[0] - px) * (t.y[2] - py)) *
                                         invArea;
                        const float w2 = 1.0f - w0 - w1;
                        if (w0 < 0.0f || w1 < 0.0f || w2 < 0.0f) {
                            continue;
                        }

                        const float depth =
                            w0 * t.z[0] + w1 * t.z[1] + w2 * t.z[2] - t.depthBias;
                        if (!(depth >= 0.0f) || depth > 1.0f || !(depth < depthRow[x])) {
                            continue;
                        }

                        // Perspective-correct colour: interpolate c/w and 1/w
                        // and divide. Under an orthographic projection every
                        // invW is equal and this reduces to the linear case.
                        const float invW = w0 * t.invW[0] + w1 * t.invW[1] + w2 * t.invW[2];
                        Rgba color;
                        if (invW > 0.0f) {
                            const float s = 1.0f / invW;
                            const auto channel = [&](int shift) {
                                const float c =
                                    (w0 * static_cast<float>((t.color[0] >> shift) & 0xFFu) *
                                         t.invW[0] +
                                     w1 * static_cast<float>((t.color[1] >> shift) & 0xFFu) *
                                         t.invW[1] +
                                     w2 * static_cast<float>((t.color[2] >> shift) & 0xFFu) *
                                         t.invW[2]) *
                                    s;
                                return static_cast<Rgba>(
                                    static_cast<std::uint8_t>(std::clamp(c, 0.0f, 255.0f) + 0.5f));
                            };
                            color = (channel(24) << 24) | (channel(16) << 16) |
                                    (channel(8) << 8) | channel(0);
                        } else {
                            color = t.color[0];
                        }

                        depthRow[x] = depth;
                        row[x] = color;
                        ++stats.fragments;
                    }
                }
            }
        }
    });
}

// ---- entry point ----------------------------------------------------------------

Result<RenderStats> Rasterizer::render(const DrawList& list, const Camera& camera,
                                       Framebuffer& target, const RenderOptions& options)
{
    if (target.empty()) {
        return makeError(ErrorCode::InvalidArgument, "render target is empty");
    }
    if (camera.viewportWidth() != target.width() || camera.viewportHeight() != target.height()) {
        return makeError(ErrorCode::InvalidArgument,
                         "camera viewport does not match the render target",
                         std::to_string(camera.viewportWidth()) + "x" +
                             std::to_string(camera.viewportHeight()) + " vs " +
                             std::to_string(target.width()) + "x" +
                             std::to_string(target.height()));
    }

    TaskPool& pool = options.pool != nullptr ? *options.pool : TaskPool::shared();

    if (options.clear) {
        // Parallel over tiles: on a 4K target this is 33 MB of writes, which is
        // worth splitting even though it is only a memset.
        Rgba* const colorBase = target.color().data();
        float* const depthBase = target.depth().data();
        const int stride = target.width();
        pool.parallelFor(0, target.tileCount(), [&](std::size_t tileIndex) {
            const TileRect rect = target.tile(tileIndex);
            for (int y = rect.y0; y < rect.y1; ++y) {
                const std::size_t row =
                    static_cast<std::size_t>(y) * static_cast<std::size_t>(stride);
                std::fill(colorBase + row + rect.x0, colorBase + row + rect.x1,
                          options.background);
                std::fill(depthBase + row + rect.x0, depthBase + row + rect.x1, 1.0f);
            }
        });
    }

    transformVertices(list, camera, pool);
    buildScreenPrimitives(list, target, options, pool);
    rasteriseTiles(target, pool);

    RenderStats total;
    total.vertices = list.positions.size();
    total.tiles = target.tileCount();
    for (const Chunk& chunk : chunks_) {
        total.trianglesSubmitted += chunk.stats.trianglesSubmitted;
        total.trianglesRasterised += chunk.stats.trianglesRasterised;
        total.linesSubmitted += chunk.stats.linesSubmitted;
        total.pointsSubmitted += chunk.stats.pointsSubmitted;
        total.binEntries += chunk.stats.binEntries;
    }
    for (const RenderStats& tile : tileStats_) {
        total.fragments += tile.fragments;
    }
    return total;
}

} // namespace katana::render
