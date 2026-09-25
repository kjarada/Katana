#include "gpu_scene.hpp"

#include <algorithm>

#include "katana/core/cpu_features.hpp"
#include "scene_origin.hpp"

#if defined(KATANA_HAVE_AVX2_KERNELS)
#include "simd/pack_kernels.hpp"
#endif

namespace katana::qt::gpu {

using katana::render::DrawList;

void GpuSceneData::clear()
{
    origin = katana::math::Vec3(0.0, 0.0, 0.0);
    bounds = katana::math::AABB{};
    vertices.clear();
    triangleIndices.clear();
    lines.clear();
    points.clear();
}

std::size_t GpuSceneData::byteSize() const
{
    return vertices.size() * sizeof(GpuVertex) + triangleIndices.size() * sizeof(std::uint32_t) +
           lines.size() * sizeof(GpuLine) + points.size() * sizeof(GpuPoint);
}

void packDrawList(const DrawList& list, GpuSceneData& out)
{
    // Bounded once, for the origin and the box both.
    const katana::math::AABB bounds = list.bounds();
    packDrawList(list, chooseSceneOrigin(bounds), bounds, out);
}

void packDrawList(const DrawList& list, const katana::math::Vec3& origin, GpuSceneData& out)
{
    packDrawList(list, origin, list.bounds(), out);
}

namespace {

// The draw list's positions, as the kernel reads them: x, y, z doubles a
// vertex, one vertex after another.
static_assert(sizeof(katana::render::Point3) == 3 * sizeof(double));

void packVertices(const DrawList& list, std::size_t count, const katana::math::Vec3& origin,
                  std::vector<GpuVertex>& out)
{
    out.resize(count);
#if defined(KATANA_HAVE_AVX2_KERNELS)
    // Any length but none: the kernel has no set-up to repay, and at four
    // vertices it already packs as fast as the loop (docs/gpu.md, "Packing").
    // An empty list has no first position to point it at.
    if (count > 0 && katana::core::activeSimdLevel() == katana::core::SimdLevel::Avx2) {
        const double at[3] = {origin.x, origin.y, origin.z};
        katana_avx2_pack_vertices(&list.positions.front().x, list.colors.data(), count, at,
                                  out.data());
        return;
    }
#endif
    for (std::size_t i = 0; i < count; ++i) {
        const auto p = relativePosition(list.positions[i], origin);
        out[i] = GpuVertex{p[0], p[1], p[2], list.colors[i]};
    }
}

} // namespace

void packDrawList(const DrawList& list, const katana::math::Vec3& origin,
                  const katana::math::AABB& bounds, GpuSceneData& out)
{
    out.clear();
    out.origin = origin;
    out.bounds = bounds;

    const std::size_t count = std::min(list.positions.size(), list.colors.size());
    packVertices(list, count, origin, out.vertices);

    // The indices are sized for every triangle, written in place and cut back
    // to those kept: sizing plain integers is one memset, and three push_backs
    // a triangle made the whole pack 14% slower (BM_GpuPack, docs/gpu.md
    // "Packing"). The lines are the other way round and stay appended: a
    // line's width defaults to 1, so sizing the array writes every line once
    // before the loop writes it again, and that made the pack 24% slower. The
    // points, whose size defaults to 1 the same way, are appended too.
    out.triangleIndices.resize(list.triangles.size() * 3);
    std::uint32_t* index = out.triangleIndices.data();
    for (const auto& triangle : list.triangles) {
        if (triangle.a >= count || triangle.b >= count || triangle.c >= count) {
            continue;
        }
        index[0] = triangle.a;
        index[1] = triangle.b;
        index[2] = triangle.c;
        index += 3;
    }
    out.triangleIndices.resize(static_cast<std::size_t>(index - out.triangleIndices.data()));

    out.lines.reserve(list.lines.size());
    for (const auto& source : list.lines) {
        if (source.a >= count || source.b >= count) {
            continue;
        }
        const GpuVertex& a = out.vertices[source.a];
        const GpuVertex& b = out.vertices[source.b];
        const float width = std::max(source.width, 1.0f);
        out.lines.push_back(GpuLine{GpuLineEnd{a.x, a.y, a.z, a.color, width},
                                    GpuLineEnd{b.x, b.y, b.z, b.color, width}});
    }

    out.points.reserve(list.points.size());
    for (const auto& source : list.points) {
        if (source.a >= count) {
            continue;
        }
        const GpuVertex& v = out.vertices[source.a];
        out.points.push_back(GpuPoint{v.x, v.y, v.z, v.color, std::max(source.size, 1.0f), 0.0f});
    }
}

void PointCloudData::clear()
{
    origin = katana::math::Vec3(0.0, 0.0, 0.0);
    points.clear();
    sourceCount = 0;
}

namespace {

// Every stride-th of `count` points, the stride the smallest whole number that
// keeps the count within the budget: ceil(count / budget).
template <typename PositionOf, typename ColorOf>
void packThinned(std::size_t count, PositionOf positionOf, ColorOf colorOf,
                 const katana::math::Vec3& origin, std::size_t budget, PointCloudData& out)
{
    out.clear();
    out.origin = origin;
    out.sourceCount = count;
    std::size_t stride = 1;
    if (budget > 0 && count > budget) {
        stride = (count + budget - 1) / budget;
    }
    out.points.reserve(count / stride + 1);
    for (std::size_t i = 0; i < count; i += stride) {
        const katana::math::Vec3 position = positionOf(i);
        if (!position.isFinite()) {
            continue;
        }
        const auto p = relativePosition(position, origin);
        out.points.push_back(GpuCloudPoint{p[0], p[1], p[2], colorOf(i)});
    }
}

} // namespace

void packPointCloud(const std::vector<katana::math::Vec3>& positions,
                    const std::vector<katana::render::Rgba>& colors,
                    katana::render::Rgba fallbackColor, const katana::math::Vec3& origin,
                    std::size_t budget, PointCloudData& out)
{
    const bool colored = colors.size() == positions.size();
    packThinned(
        positions.size(), [&](std::size_t i) { return positions[i]; },
        [&](std::size_t i) { return colored ? colors[i] : fallbackColor; }, origin, budget, out);
}

void packPointCloud(const katana::pointcloud::PointCloud& cloud,
                    katana::render::Rgba fallbackColor, std::size_t budget, PointCloudData& out)
{
    const auto& points = cloud.points;
    katana::math::AABB box;
    if (!cloud.bounds.empty()) {
        box.expand(katana::math::Vec3(cloud.bounds.minX, cloud.bounds.minY, cloud.bounds.minZ));
        box.expand(katana::math::Vec3(cloud.bounds.maxX, cloud.bounds.maxY, cloud.bounds.maxZ));
    } else {
        for (const auto& point : points) {
            const katana::math::Vec3 position(point.x, point.y, point.z);
            if (position.isFinite()) { // a stray infinity would put the origin nowhere
                box.expand(position);
            }
        }
    }
    packThinned(
        points.size(),
        [&](std::size_t i) { return katana::math::Vec3(points[i].x, points[i].y, points[i].z); },
        [&](std::size_t i) {
            const auto& point = points[i];
            return point.hasColor ? katana::render::rgba(point.red, point.green, point.blue)
                                  : fallbackColor;
        },
        chooseSceneOrigin(box), budget, out);
}

} // namespace katana::qt::gpu
