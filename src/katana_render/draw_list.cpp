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

katana::math::AABB DrawList::bounds() const
{
    katana::math::AABB box;
    const auto include = [&](VertexIndex index) {
        if (index < positions.size() && positions[index].isFinite()) {
            box.expand(positions[index]);
        }
    };
    for (const DrawTriangle& triangle : triangles) {
        include(triangle.a);
        include(triangle.b);
        include(triangle.c);
    }
    for (const DrawLine& line : lines) {
        include(line.a);
        include(line.b);
    }
    for (const DrawPoint& point : points) {
        include(point.a);
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
