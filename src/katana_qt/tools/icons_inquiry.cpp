// Icons of the Distance, Area, ID Point, Angle, List tools (see
// tool_icons.hpp). What is measured is drawn in the neutral tone - the points
// picked, the outline, the arms - and the measurement in the accent: the
// span a distance covers, the area filled, the angle's arc. They read at
// 16 px, so no mark is closer than a stroke to the next.

#include "tools/tool_icons.hpp"

namespace katana::qt::tools {

bool paintInquiryIcon(std::string_view toolId, const ToolInk& ink)
{
    if (toolId == "inquiry.distance") {
        // Two picked points and the span between them, with the witness
        // ticks a dimension has at its ends.
        ink.line(5, 17, 19, 7, true);
        ink.line(3.3, 14.6, 6.7, 19.4, true, 1.2);
        ink.line(17.3, 4.6, 20.7, 9.4, true, 1.2);
        ink.node(5, 17);
        ink.node(19, 7);
        return true;
    }
    if (toolId == "inquiry.area") {
        // A four-sided lot, filled: the area is the inside.
        const QPainterPath lot = polyline({{4, 18}, {7, 6}, {19, 4}, {20, 17}}, true);
        ink.fill(lot, true, 110);
        ink.stroke(lot);
        return true;
    }
    if (toolId == "inquiry.id") {
        // A point in the crosshairs: where it is is the answer.
        ink.stroke(circle(12, 12, 6.5));
        ink.line(12, 2.5, 12, 8);
        ink.line(12, 16, 12, 21.5);
        ink.line(2.5, 12, 8, 12);
        ink.line(16, 12, 21.5, 12);
        ink.dot(12, 12, 2.2, true);
        return true;
    }
    if (toolId == "inquiry.angle") {
        // Two arms from a vertex at (4, 19), and the angle between them: the
        // lower arm runs east, the upper one up at 40 degrees on paper, and
        // the arc of radius 9 sweeps between.
        ink.line(4, 19, 21, 19);
        ink.line(4, 19, 17.8, 7.4);
        ink.stroke(arc(4, 19, 9, 0, 40), true);
        ink.node(4, 19);
        return true;
    }
    if (toolId == "inquiry.list") {
        // A list: lines of text, each with its bullet.
        for (const double y : {6.0, 12.0, 18.0}) {
            ink.dot(5, y, 1.6, true);
            ink.line(9, y, 20, y);
        }
        return true;
    }
    return false;
}

} // namespace katana::qt::tools
