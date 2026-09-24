#include "gpu_scene.hpp"

#include <algorithm>

#include "scene_origin.hpp"

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
    const katana::math::AABB bounds = list.bounds();
    packDrawList(list, chooseSceneOrigin(bounds), out);
    out.bounds = bounds;
}

void packDrawList(const DrawList& list, const katana::math::Vec3& origin, GpuSceneData& out)
{
    out.clear();
    out.origin = origin;
    out.bounds = list.bounds();

    const std::size_t count = std::min(list.positions.size(), list.colors.size());
    out.vertices.resize(count);
    for (std::size_t i = 0; i < count; ++i) {
        const auto p = relativePosition(list.positions[i], origin);
        out.vertices[i] = GpuVertex{p[0], p[1], p[2], list.colors[i]};
    }

    out.triangleIndices.reserve(list.triangles.size() * 3);
    for (const auto& triangle : list.triangles) {
        if (triangle.a >= count || triangle.b >= count || triangle.c >= count) {
            continue;
        }
        out.triangleIndices.push_back(triangle.a);
        out.triangleIndices.push_back(triangle.b);
        out.triangleIndices.push_back(triangle.c);
    }

    out.lines.reserve(list.lines.size());
    for (const auto& line : list.lines) {
        if (line.a >= count || line.b >= count) {
            continue;
        }
        const GpuVertex& a = out.vertices[line.a];
        const GpuVertex& b = out.vertices[line.b];
        const float width = std::max(line.width, 1.0f);
        out.lines.push_back(GpuLine{GpuLineEnd{a.x, a.y, a.z, a.color, width},
                                    GpuLineEnd{b.x, b.y, b.z, b.color, width}});
    }

    out.points.reserve(list.points.size());
    for (const auto& point : list.points) {
        if (point.a >= count) {
            continue;
        }
        const GpuVertex& v = out.vertices[point.a];
        out.points.push_back(GpuPoint{v.x, v.y, v.z, v.color, std::max(point.size, 1.0f), 0.0f});
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
