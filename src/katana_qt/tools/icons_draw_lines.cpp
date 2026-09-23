// Icons of the Point, Line, Polyline, Rectangle, Polygon tools (see tool_icons.hpp).
//
// Point, Line, Polyline and Rectangle are the drawings the toolbar already
// has for them (icons.cpp), so the tools look the same when the catalogue's
// menus and toolbars replace the hand-made actions: what is drawn in the
// accent, the points the user gives as neutral nodes. Polygon is drawn as
// Circle is - the shape in the accent, its centre and a radius in the neutral
// tone - with the radius going off to a vertex, so the icon reads as "centre
// and vertex", the tool's default way of drawing one. Rectangle marks only
// the two opposite corners it is drawn from, where icons.cpp marks all four.

#include <array>

#include "tools/tool_icons.hpp"

namespace katana::qt::tools {

bool paintDrawLineIcon(std::string_view toolId, const ToolInk& ink)
{
    if (toolId == "draw.point") {
        ink.line(12, 3, 12, 8);
        ink.line(12, 16, 12, 21);
        ink.line(3, 12, 8, 12);
        ink.line(16, 12, 21, 12);
        ink.dot(12, 12, 2.6, true);
        return true;
    }
    if (toolId == "draw.line") {
        ink.line(5, 19, 19, 5, true);
        ink.node(5, 19);
        ink.node(19, 5);
        return true;
    }
    if (toolId == "draw.polyline") {
        const std::array<QPointF, 4> vertices = {QPointF(4, 18), QPointF(9, 8), QPointF(15, 15),
                                                 QPointF(20, 5)};
        ink.stroke(polyline({vertices[0], vertices[1], vertices[2], vertices[3]}), true);
        for (const QPointF& p : vertices) {
            ink.node(p.x(), p.y());
        }
        return true;
    }
    if (toolId == "draw.rectangle") {
        ink.stroke(rectangle(4, 6, 16, 12), true);
        // The two corners the tool asks for, not all four: a rectangle is
        // given by opposite corners.
        ink.node(4, 18);
        ink.node(20, 6);
        return true;
    }
    if (toolId == "draw.polygon") {
        // A pentagon of radius 9 about (12, 13), point up: vertices at 90, 18,
        // -54, -126 and 162 degrees on paper (9 cos 18 = 8.56, 9 sin 18 = 2.78,
        // 9 cos 54 = 5.29, 9 sin 54 = 7.28). Five sides rather than six: at
        // 16 px a hexagon rounds off into Circle's icon, a pentagon keeps its
        // corners. The centre sits low because a pentagon's bulk does.
        const QPointF top(12, 4);
        const QPointF right(20.56, 10.22);
        const QPointF lowerRight(17.29, 20.28);
        const QPointF lowerLeft(6.71, 20.28);
        const QPointF left(3.44, 10.22);
        ink.stroke(polyline({top, right, lowerRight, lowerLeft, left}, true), true);
        ink.line(12, 13, right.x(), right.y(), false, 1.1);
        ink.node(12, 13);
        ink.node(right.x(), right.y());
        return true;
    }
    return false;
}

} // namespace katana::qt::tools
