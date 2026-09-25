#include "katana/render/draw_list.hpp"

#include <cmath>

namespace katana::render {

void DrawList::clear()
{
    // clear() keeps capacity; the vectors are members of a Rasterizer-adjacent
    // object that is reused every frame, which is the point (section 33).
    positions.clear();
    colors.clear();
    triangles.clear();
    lines.clear();
    points.clear();
}

VertexIndex DrawList::addVertex(const Point3& position, Rgba color)
{
    const auto index = static_cast<VertexIndex>(positions.size());
    positions.push_back(position);
    colors.push_back(color);
    return index;
}

void DrawList::addTriangle(VertexIndex a, VertexIndex b, VertexIndex c)
{
    triangles.push_back(DrawTriangle{a, b, c});
}

void DrawList::addLine(VertexIndex a, VertexIndex b, float width, float depthBias)
{
    lines.push_back(DrawLine{a, b, width, depthBias});
}

void DrawList::addPoint(VertexIndex a, float size, float depthBias)
{
    points.push_back(DrawPoint{a, size, depthBias});
}

void DrawList::addSegment(const Point3& from, const Point3& to, Rgba color, float width,
                          float depthBias)
{
    const VertexIndex a = addVertex(from, color);
    const VertexIndex b = addVertex(to, color);
    addLine(a, b, width, depthBias);
}

namespace {

// The reference order: every use of every vertex, primitive by primitive.
// What bounds() falls back to when the order decides the result.
katana::math::AABB boundsInPrimitiveOrder(const DrawList& list)
{
    katana::math::AABB box;
    const auto include = [&](VertexIndex index) {
        if (index < list.positions.size() && list.positions[index].isFinite()) {
            box.expand(list.positions[index]);
        }
    };
    for (const DrawTriangle& triangle : list.triangles) {
        include(triangle.a);
        include(triangle.b);
        include(triangle.c);
    }
    for (const DrawLine& line : list.lines) {
        include(line.a);
        include(line.b);
    }
    for (const DrawPoint& point : list.points) {
        include(point.a);
    }
    return box;
}

} // namespace

katana::math::AABB DrawList::bounds() const
{
    std::vector<std::uint8_t> used;
    return bounds(used);
}

katana::math::AABB DrawList::bounds(std::vector<std::uint8_t>& used) const
{
    const std::size_t count = positions.size();
    used.assign(count, 0);
    const auto use = [&used, count](VertexIndex index) {
        if (index < count) {
            used[index] = 1;
        }
    };
    for (const DrawTriangle& triangle : triangles) {
        use(triangle.a);
        use(triangle.b);
        use(triangle.c);
    }
    for (const DrawLine& line : lines) {
        use(line.a);
        use(line.b);
    }
    for (const DrawPoint& point : points) {
        use(point.a);
    }
    katana::math::AABB box;
    for (std::size_t i = 0; i < count; ++i) {
        if (used[i] != 0 && positions[i].isFinite()) {
            box.expand(positions[i]);
        }
    }
    if (box.min.x == 0.0 || box.min.y == 0.0 || box.min.z == 0.0 || box.max.x == 0.0 ||
        box.max.y == 0.0 || box.max.z == 0.0) {
        return boundsInPrimitiveOrder(*this);
    }
    return box;
}

bool DrawList::allFinite() const
{
    for (const Point3& position : positions) {
        if (!position.isFinite()) {
            return false;
        }
    }
    return true;
}

} // namespace katana::render
