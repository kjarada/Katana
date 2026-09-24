#pragma once

// A DrawList packed the way the GPU reads it (docs/gpu.md, "Architecture").
//
// This is the CPU half of an upload, and it has no Qt in it: the packing is
// plain arithmetic that can be tested without a device, and the renderer then
// copies these arrays into GPU buffers byte for byte.
//
// WHAT CHANGES FROM THE DRAW LIST
//
//   * Positions become float offsets from `origin`, which stays in double
//     (scene_origin.hpp has the precision rule). 16 bytes a vertex instead of
//     the draw list's 28 (three doubles and a colour).
//   * Triangles keep sharing vertices through a 32-bit index buffer.
//   * Lines and points become INSTANCES: each carries its own endpoint
//     positions and colours, and the vertex shader widens it into a screen-
//     space quad. The software path widens every line into two triangles on
//     the CPU (render::Rasterizer, buildScreenPrimitives); here that work - and
//     the antialiasing - happens per pixel on the GPU.
//   * A line's or point's depthBias is NOT carried. It is a constant in NDC
//     depth, sized for the software path's standard-Z buffer, and applied to
//     a reversed-Z float buffer it would pull a line metres towards the eye
//     (the x-ray defect, map_view3d). The GPU pushes filled triangles back by a
//     slope-scaled polygon offset instead (gpu_renderer.cpp, kFillSlopeBias),
//     which is what lets an edge lying in a surface win without letting a line
//     behind a building show through it.
//
// Indices the draw list holds that point past its vertices are dropped here,
// as the software path skips them, so a malformed list cannot make the GPU
// read outside a buffer.

#include <cstddef>
#include <cstdint>
#include <vector>

#include "katana/math/primitives.hpp"
#include "katana/render/draw_list.hpp"

namespace katana::qt::gpu {

// One vertex: position relative to the scene origin and the draw list's
// 0xAARRGGBB colour, which on a little-endian machine is the bytes B, G, R, A
// - the shaders swizzle it back.
struct GpuVertex {
    float x = 0.0f;
    float y = 0.0f;
    float z = 0.0f;
    std::uint32_t color = 0;
};
static_assert(sizeof(GpuVertex) == 16);

struct GpuLine {
    float ax = 0.0f;
    float ay = 0.0f;
    float az = 0.0f;
    std::uint32_t colorA = 0;
    float bx = 0.0f;
    float by = 0.0f;
    float bz = 0.0f;
    std::uint32_t colorB = 0;
    float width = 1.0f; // logical pixels, as DrawLine::width
    float unused = 0.0f;
};
static_assert(sizeof(GpuLine) == 40);

struct GpuPoint {
    float x = 0.0f;
    float y = 0.0f;
    float z = 0.0f;
    std::uint32_t color = 0;
    float size = 1.0f; // logical pixels, as DrawPoint::size: a size x size square
    float unused = 0.0f;
};
static_assert(sizeof(GpuPoint) == 24);

struct GpuSceneData {
    // Kept in double: every position below is relative to this.
    katana::math::Vec3 origin{0.0, 0.0, 0.0};
    // What the draw list's primitives reference, in world coordinates.
    katana::math::AABB bounds;

    std::vector<GpuVertex> vertices;
    std::vector<std::uint32_t> triangleIndices; // three per triangle
    std::vector<GpuLine> lines;
    std::vector<GpuPoint> points;

    void clear();
    [[nodiscard]] std::size_t triangleCount() const { return triangleIndices.size() / 3; }
    [[nodiscard]] bool empty() const
    {
        return triangleIndices.empty() && lines.empty() && points.empty();
    }
    // Bytes the GPU buffers will hold, for the stats line and the docs.
    [[nodiscard]] std::size_t byteSize() const;
};

// Packs `list` into `out` (cleared first, capacity kept), relative to the
// centre of the list's bounds.
void packDrawList(const katana::render::DrawList& list, GpuSceneData& out);

// Same, relative to a given origin. For tests that need to prove the origin is
// what makes large coordinates safe - the renderer always uses the centre.
void packDrawList(const katana::render::DrawList& list, const katana::math::Vec3& origin,
                  GpuSceneData& out);

// ---- point clouds ---------------------------------------------------------------

// One cloud point: 16 bytes against the 40 of pointcloud::PointCloudPoint.
struct GpuCloudPoint {
    float x = 0.0f;
    float y = 0.0f;
    float z = 0.0f;
    std::uint32_t color = 0;
};
static_assert(sizeof(GpuCloudPoint) == 16);

struct PointCloudData {
    katana::math::Vec3 origin{0.0, 0.0, 0.0};
    std::vector<GpuCloudPoint> points;
    // How many of the source points were offered; points.size() is what the
    // budget let through.
    std::size_t sourceCount = 0;

    void clear();
};

// Packs at most `budget` of the given points, relative to `origin`, taking an
// evenly spread subset when there are more: every k-th point, k chosen so the
// budget is not exceeded. Even spacing through the SOURCE ORDER is the right
// thinning for a file read in acquisition or tile order (a LAS scan line is
// spatially coherent); a budget of 0 means no limit. `colors` is parallel to
// `positions`, or empty for a single `fallbackColor`.
void packPointCloud(const std::vector<katana::math::Vec3>& positions,
                    const std::vector<katana::render::Rgba>& colors,
                    katana::render::Rgba fallbackColor, const katana::math::Vec3& origin,
                    std::size_t budget, PointCloudData& out);

} // namespace katana::qt::gpu
