#pragma once

// Tiled, multithreaded software rasteriser (PLAN.MD Phases 15 and 19).
//
// WHY A SOFTWARE RASTERISER AT ALL, when the plan names Vulkan. Two reasons,
// both of which outlive it:
//
//  * It is testable. Rule 5 says test before optimisation, and a GPU frame
//    cannot be asserted on in CI - there is no dependable headless GL or Vulkan
//    device on the build machines. Every pixel here is a unit test, so the
//    clipping, the fill rule and the depth test are pinned down BEFORE any of it
//    is reimplemented against a driver.
//  * It fixes the architecture. Everything above it talks to DrawList and
//    Camera, never to this class, so the Vulkan backend is a second
//    implementation rather than a rewrite of the application.
//
// It is not a placeholder that does the obvious slow thing. The structure is
// the one a tile-based GPU uses:
//
//   1. TRANSFORM   every vertex once, in parallel, into clip space. Primitives
//                  index vertices, so a vertex shared by twenty triangles is
//                  transformed once - which is the entire reason DrawList is
//                  indexed.
//   2. SETUP+BIN   clip against the near plane, project to pixels, widen lines
//                  into quads, then record each primitive in the list of every
//                  64x64 screen tile its bounding box touches. Done in parallel
//                  over a FIXED number of primitive chunks.
//   3. RASTERISE   one task per tile. A tile owns its pixels exclusively, so
//                  there is no lock, no atomic and no false sharing in the
//                  inner loop, and the colour and depth a tile touches stay in
//                  L2 for the whole tile.
//
// DETERMINISM (Rule 7). The chunk count in stage 2 is a function of the
// primitive count alone, never of the number of cores, so the order in which a
// tile visits its primitives is identical on every machine. With a
// strictly-less depth test that makes the frame reproducible bit for bit -
// asserted by a test that renders the same scene with a 1-thread pool and with
// the shared pool and compares the buffers.
//
// SCRATCH is owned by the Rasterizer and reused, so a viewport redrawing at
// 60 Hz allocates on the first frame and never again (PLAN.MD section 33).
// That is also why this is a class and not a free function.

#include <cstddef>
#include <cstdint>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/render/camera.hpp"
#include "katana/render/draw_list.hpp"
#include "katana/render/framebuffer.hpp"

namespace katana::core {
class TaskPool;
}

namespace katana::render {

struct RenderOptions {
    Rgba background = rgba(30, 30, 34);
    // Discards clockwise-in-screen-space triangles. Off by default: a survey
    // surface is legitimately inspected from below, and a TIN whose triangles
    // are all counter-clockwise in plan is clockwise the moment you look up at
    // it from underneath.
    bool backfaceCull = false;
    // Clears colour and depth first. Off lets a caller compose several passes.
    bool clear = true;
    // null uses TaskPool::shared(). A single-threaded pool renders the same
    // pixels; pass one to make a test independent of the machine.
    katana::core::TaskPool* pool = nullptr;
};

struct RenderStats {
    std::size_t vertices = 0;
    std::size_t trianglesSubmitted = 0;
    std::size_t trianglesRasterised = 0; // survived near-clip, cull and the viewport
    std::size_t linesSubmitted = 0;
    std::size_t pointsSubmitted = 0;
    std::size_t fragments = 0;   // pixels that passed the depth test and were written
    std::size_t binEntries = 0;  // primitive-in-tile records; > 1 per primitive when it spans
    std::size_t tiles = 0;
};

class Rasterizer {
  public:
    Rasterizer() = default;

    // Draws `list` through `camera` into `target`.
    //
    // Fails with InvalidArgument when the target is empty or its size does not
    // match the camera's viewport - a mismatch silently renders the wrong
    // framing, which is exactly the kind of quiet wrongness section 36 forbids.
    [[nodiscard]] katana::core::Result<RenderStats>
    render(const DrawList& list, const Camera& camera, Framebuffer& target,
           const RenderOptions& options = {});

  private:
    // A vertex after the model-view-projection, before the perspective divide.
    struct ClipVertex {
        float x = 0.0f;
        float y = 0.0f;
        float z = 0.0f;
        float w = 0.0f;
        Rgba color = 0;
    };

    // A triangle ready to rasterise: pixel coordinates, NDC depth, and 1/w for
    // perspective-correct colour. Floats throughout - screen space needs
    // nothing more, and halving the working set is what keeps a tile in cache.
    struct ScreenTriangle {
        float x[3]{};
        float y[3]{};
        float z[3]{};       // NDC depth, already in [0, 1] and screen-linear
        float invW[3]{};
        Rgba color[3]{};
        float depthBias = 0.0f;
    };

    struct ScreenPoint {
        float x = 0.0f;
        float y = 0.0f;
        float z = 0.0f;
        float half = 0.5f; // half the square's side, in pixels
        Rgba color = 0;
    };

    void transformVertices(const DrawList& list, const Camera& camera,
                           katana::core::TaskPool& pool);
    void buildScreenPrimitives(const DrawList& list, const Framebuffer& target,
                               const RenderOptions& options, katana::core::TaskPool& pool);
    void binPrimitives(const Framebuffer& target, katana::core::TaskPool& pool);
    void rasteriseTiles(Framebuffer& target, katana::core::TaskPool& pool);

    // Stage 1 output.
    std::vector<ClipVertex> clip_;

    // Stage 2 output, one bucket per primitive chunk so the bucket a primitive
    // lands in - and therefore the order a tile sees it in - depends only on
    // its index.
    struct Chunk {
        std::vector<ScreenTriangle> triangles;
        std::vector<ScreenPoint> points;
        // tileBins[t] holds indices into `triangles` (below kPointTag) and into
        // `points` (with kPointTag set), in submission order.
        std::vector<std::vector<std::uint32_t>> tileBins;
        RenderStats stats;
    };
    static constexpr std::uint32_t kPointTag = 0x8000'0000u;

    std::vector<Chunk> chunks_;
    std::vector<RenderStats> tileStats_;
};

} // namespace katana::render
