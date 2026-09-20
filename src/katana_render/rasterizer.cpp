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

} // namespace

// ---- stage 1: transform ---------------------------------------------------------

void Rasterizer::transformVertices(const DrawList& list, const Camera& camera, TaskPool& pool)
{
    const katana::math::Mat4 mvp = camera.viewProjection();
    clip_.resize(list.positions.size());

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

// The near plane in clip space is w > 0 combined with z >= 0; the one that
// actually breaks the perspective divide is w, so that is what is clipped
// against. kMinW keeps the reciprocal finite for a vertex sitting exactly on
// the eye plane.
constexpr float kMinW = 1.0e-6f;

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

    const auto project = [&viewport](const ClipVertex& v) {
        ProjectedVertex out;
        const float invW = 1.0f / std::max(v.w, kMinW);
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

        // Appends a screen triangle after clipping it against the near plane.
        // A triangle crossing the plane becomes one or two; one entirely behind
        // it disappears.
        const auto emitTriangle = [&](const std::array<ClipVertex, 3>& v, float depthBias) {
            std::array<ClipVertex, 4> inside{};
            std::size_t insideCount = 0;
            for (std::size_t i = 0; i < 3; ++i) {
                const ClipVertex& current = v[i];
                const ClipVertex& next = v[(i + 1) % 3];
                const bool currentIn = current.w > kMinW;
                const bool nextIn = next.w > kMinW;
                if (currentIn) {
                    inside[insideCount++] = current;
                }
                if (currentIn != nextIn) {
                    // Split where w crosses kMinW. Sutherland-Hodgman against
                    // the single plane that the divide cannot survive.
                    const float t = (kMinW - current.w) / (next.w - current.w);
                    ClipVertex cut;
                    cut.x = current.x + (next.x - current.x) * t;
                    cut.y = current.y + (next.y - current.y) * t;
                    cut.z = current.z + (next.z - current.z) * t;
                    cut.w = kMinW;
                    cut.color = lerpColor(current.color, next.color, t);
                    inside[insideCount++] = cut;
                }
            }
            if (insideCount < 3) {
                return;
            }
            for (std::size_t i = 1; i + 1 < insideCount; ++i) {
                const ProjectedVertex p0 = project(inside[0]);
                const ProjectedVertex p1 = project(inside[i]);
                const ProjectedVertex p2 = project(inside[i + 1]);

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

        for (std::size_t index = first; index < last; ++index) {
            if (index < list.triangles.size()) {
                const DrawTriangle& t = list.triangles[index];
                if (t.a >= clip_.size() || t.b >= clip_.size() || t.c >= clip_.size()) {
                    continue;
                }
                ++chunk.stats.trianglesSubmitted;
                emitTriangle({clip_[t.a], clip_[t.b], clip_[t.c]}, 0.0f);
                continue;
            }
            const std::size_t lineIndex = index - list.triangles.size();
            if (lineIndex < list.lines.size()) {
                const DrawLine& line = list.lines[lineIndex];
                if (line.a >= clip_.size() || line.b >= clip_.size()) {
                    continue;
                }
                ++chunk.stats.linesSubmitted;

                // Widen in SCREEN space so a line keeps its pixel thickness at
                // any depth, then reuse the triangle path: one tested
                // rasteriser, correct depth, correct clipping.
                ClipVertex a = clip_[line.a];
                ClipVertex b = clip_[line.b];
                if (a.w <= kMinW && b.w <= kMinW) {
                    continue;
                }
                if (a.w <= kMinW || b.w <= kMinW) {
                    ClipVertex& behind = a.w <= kMinW ? a : b;
                    const ClipVertex& front = a.w <= kMinW ? b : a;
                    const float t = (kMinW - behind.w) / (front.w - behind.w);
                    ClipVertex cut;
                    cut.x = behind.x + (front.x - behind.x) * t;
                    cut.y = behind.y + (front.y - behind.y) * t;
                    cut.z = behind.z + (front.z - behind.z) * t;
                    cut.w = kMinW;
                    cut.color = lerpColor(behind.color, front.color, t);
                    behind = cut;
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
            const ClipVertex& v = clip_[point.a];
            if (v.w <= kMinW) {
                continue;
            }
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
            const int x0 = std::max(0, static_cast<int>(std::floor(minX)) / Framebuffer::kTileSize);
            const int y0 = std::max(0, static_cast<int>(std::floor(minY)) / Framebuffer::kTileSize);
            const int x1 = std::min(tilesAcross - 1,
                                    static_cast<int>(std::floor(maxX)) / Framebuffer::kTileSize);
            const int y1 = std::min(tilesDown - 1,
                                    static_cast<int>(std::floor(maxY)) / Framebuffer::kTileSize);
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
                    const int x0 = std::max(rect.x0, static_cast<int>(std::floor(p.x - p.half)));
                    const int x1 = std::min(rect.x1 - 1, static_cast<int>(std::floor(p.x + p.half)));
                    const int y0 = std::max(rect.y0, static_cast<int>(std::floor(p.y - p.half)));
                    const int y1 = std::min(rect.y1 - 1, static_cast<int>(std::floor(p.y + p.half)));
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
                const int minX = std::max(rect.x0,
                                          static_cast<int>(std::floor(std::min({t.x[0], t.x[1], t.x[2]}))));
                const int maxX = std::min(rect.x1 - 1,
                                          static_cast<int>(std::floor(std::max({t.x[0], t.x[1], t.x[2]}))));
                const int minY = std::max(rect.y0,
                                          static_cast<int>(std::floor(std::min({t.y[0], t.y[1], t.y[2]}))));
                const int maxY = std::min(rect.y1 - 1,
                                          static_cast<int>(std::floor(std::max({t.y[0], t.y[1], t.y[2]}))));
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
