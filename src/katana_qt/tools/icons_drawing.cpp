// Icons of the drawing system's tools (see tool_icons.hpp): Draw > Vertices,
// and the draw tools the drawing system added.
//
// The vertex tools share one picture - a three-vertex polyline in neutral
// with its vertices as nodes - and each marks in the accent what it does to
// it: the vertex it adds, removes or moves, the run it straightens, the
// heights it sets. At 16 px the family reads as one and the accent tells
// the tools apart.

#include <cmath>

#include "tools/tool_icons.hpp"

namespace katana::qt::tools {

namespace {

// The family's polyline: (3,18) (10,7) (21,14), vertices as nodes.
void basePolyline(const ToolInk& ink, bool nodes = true)
{
    ink.stroke(polyline({{3, 18}, {10, 7}, {21, 14}}));
    if (nodes) {
        ink.node(3, 18);
        ink.node(10, 7);
        ink.node(21, 14);
    }
}

void plus(const ToolInk& ink, double x, double y)
{
    ink.line(x - 2.5, y, x + 2.5, y, true, 1.4);
    ink.line(x, y - 2.5, x, y + 2.5, true, 1.4);
}

void cross(const ToolInk& ink, double x, double y)
{
    ink.line(x - 2.2, y - 2.2, x + 2.2, y + 2.2, true, 1.4);
    ink.line(x - 2.2, y + 2.2, x + 2.2, y - 2.2, true, 1.4);
}

} // namespace

bool paintDrawingIcon(std::string_view toolId, const ToolInk& ink)
{
    if (toolId == "draw.vertex.insert") {
        basePolyline(ink);
        ink.node(15.5, 10.5, true);
        plus(ink, 18, 4.5);
        return true;
    }
    if (toolId == "draw.vertex.delete") {
        ink.stroke(polyline({{3, 18}, {21, 14}}));
        ink.dashed(polyline({{3, 18}, {10, 7}, {21, 14}}));
        ink.node(3, 18);
        ink.node(21, 14);
        cross(ink, 10, 7);
        return true;
    }
    if (toolId == "draw.vertex.move") {
        ink.dashed(polyline({{3, 18}, {10, 7}, {21, 14}}));
        ink.stroke(polyline({{3, 18}, {14, 4}, {21, 14}}), true);
        ink.node(3, 18);
        ink.node(21, 14);
        ink.node(14, 4, true);
        return true;
    }
    if (toolId == "draw.vertex.edit") {
        basePolyline(ink);
        // A table beside it: the Vertices panel.
        ink.stroke(rectangle(13, 16, 9, 6), true, 1.1);
        ink.line(13, 19, 22, 19, true, 1.0);
        ink.line(16.5, 16, 16.5, 22, true, 1.0);
        return true;
    }
    if (toolId == "draw.vertex.straighten") {
        ink.dashed(polyline({{3, 16}, {8, 8}, {13, 17}, {18, 9}}));
        ink.line(3, 16, 21, 12, true);
        ink.node(3, 16);
        ink.node(21, 12);
        return true;
    }
    if (toolId == "draw.vertex.weed") {
        ink.dashed(polyline({{3, 14}, {6, 12.5}, {9, 13.5}, {12, 12}, {15, 13}, {18, 11.5}, {21, 12}}));
        ink.stroke(polyline({{3, 14}, {21, 12}}), true);
        ink.node(3, 14);
        ink.node(21, 12);
        return true;
    }
    if (toolId == "draw.vertex.densify") {
        ink.stroke(polyline({{3, 16}, {21, 8}}));
        for (int k = 1; k < 5; ++k) {
            ink.node(3 + 18.0 * k / 5.0, 16 - 8.0 * k / 5.0, true);
        }
        ink.node(3, 16);
        ink.node(21, 8);
        return true;
    }
    if (toolId == "draw.vertex.close") {
        basePolyline(ink);
        ink.line(21, 14, 3, 18, true);
        return true;
    }
    if (toolId == "draw.vertex.start") {
        ink.stroke(polyline({{4, 18}, {10, 5}, {20, 8}, {19, 19}}, true));
        ink.node(10, 5);
        ink.node(20, 8);
        ink.node(19, 19);
        ink.dot(4, 18, 2.6, true);
        return true;
    }
    if (toolId == "draw.vertex.height") {
        basePolyline(ink);
        ink.line(10, 7, 10, 21, true, 1.1);
        ink.label(15.5, 20, 7.5, "Z", true);
        return true;
    }
    if (toolId == "draw.vertex.interpolate") {
        ink.stroke(polyline({{3, 19}, {21, 7}}));
        ink.node(3, 19);
        ink.node(21, 7);
        ink.dot(9, 15, 1.9, true);
        ink.dot(15, 11, 1.9, true);
        return true;
    }
    if (toolId == "draw.vertex.grade") {
        ink.stroke(polyline({{3, 19}, {21, 7}}), true);
        ink.node(3, 19);
        ink.node(21, 7);
        ink.line(3, 19, 21, 19, false, 1.0);
        ink.line(21, 19, 21, 7, false, 1.0);
        return true;
    }
    if (toolId == "draw.vertex.arc") {
        ink.dashed(polyline({{4, 16}, {20, 16}}));
        ink.stroke(arc(12, 16, 8, 0, 180), true);
        ink.node(4, 16);
        ink.node(20, 16);
        return true;
    }
    if (toolId == "draw.vertex.line") {
        ink.dashed(arc(12, 16, 8, 0, 180));
        ink.line(4, 16, 20, 16, true);
        ink.node(4, 16);
        ink.node(20, 16);
        return true;
    }
    if (toolId == "draw.vertex.fillet") {
        ink.stroke(polyline({{3, 20}, {3, 11}}));
        ink.stroke(polyline({{12, 4}, {21, 4}}));
        ink.stroke(arc(12, 11, 9, 90, 90), true);
        ink.dashed(polyline({{3, 11}, {3, 4}, {12, 4}}));
        return true;
    }
    if (toolId == "draw.vertex.chamfer") {
        ink.stroke(polyline({{3, 20}, {3, 10}}));
        ink.stroke(polyline({{10, 4}, {21, 4}}));
        ink.line(3, 10, 10, 4, true);
        ink.dashed(polyline({{3, 10}, {3, 4}, {10, 4}}));
        return true;
    }
    if (toolId == "draw.vertex.merge") {
        ink.stroke(polyline({{3, 18}, {11, 9}, {21, 14}}));
        ink.node(3, 18);
        ink.node(21, 14);
        ink.dot(11, 9, 2.8, true);
        ink.node(9.5, 10.5);
        ink.node(12.5, 8.5);
        return true;
    }
    if (toolId == "draw.vertex.grid") {
        for (double x : {4.0, 12.0, 20.0}) {
            for (double y : {4.0, 12.0, 20.0}) {
                ink.dot(x, y, 0.8);
            }
        }
        ink.stroke(polyline({{4, 20}, {12, 4}, {20, 12}}), true);
        ink.node(4, 20, true);
        ink.node(12, 4, true);
        ink.node(20, 12, true);
        return true;
    }
    return false;
}

} // namespace katana::qt::tools
