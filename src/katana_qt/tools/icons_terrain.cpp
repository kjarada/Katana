// Icons of Terrain > Edit Surface's tools (see tool_icons.hpp;
// src/katana_cad/tools/terrain_edit.cpp, docs/surface_editing.md). Each
// draws the same small TIN in the neutral tone - a quadrilateral of four
// triangles round a centre vertex - and what the tool does to it in the
// accent, so the nine read as one family on the Surface Edit toolbar.

#include "tools/tool_icons.hpp"

namespace katana::qt::tools {

namespace {

// The corners of the TIN and its centre, on the 24-unit grid.
constexpr QPointF kCorners[4] = {{3, 19}, {6, 5}, {19, 4}, {21, 18}};
constexpr QPointF kCentre{12, 12};

void drawTin(const ToolInk& ink, bool withCentre = true)
{
    ink.stroke(polyline({kCorners[0], kCorners[1], kCorners[2], kCorners[3]}, true));
    if (withCentre) {
        for (const QPointF& corner : kCorners) {
            ink.line(corner.x(), corner.y(), kCentre.x(), kCentre.y());
        }
    }
}

} // namespace

bool paintTerrainEditIcon(std::string_view toolId, const ToolInk& ink)
{
    if (toolId == "terrain.edit.deletetriangle") {
        // The lower triangle, filled and struck out.
        drawTin(ink);
        ink.fill(polyline({kCorners[0], kCorners[3], kCentre}, true), true, 120);
        ink.line(9, 18, 15, 14, true);
        return true;
    }
    if (toolId == "terrain.edit.flip") {
        // A quadrilateral, its old diagonal dashed and the new one in the accent.
        ink.stroke(polyline({{3, 12}, {12, 3}, {21, 12}, {12, 21}}, true));
        ink.dashed(polyline({{3, 12}, {21, 12}}));
        ink.line(12, 3, 12, 21, true);
        return true;
    }
    if (toolId == "terrain.edit.addpoint") {
        // The TIN with no centre yet, and the new vertex with its plus.
        drawTin(ink, false);
        ink.dot(12, 12, 2.4, true);
        ink.line(12, 6.5, 12, 9, true, 1.4);
        ink.line(12, 15, 12, 17.5, true, 1.4);
        ink.line(6.5, 12, 9, 12, true, 1.4);
        ink.line(15, 12, 17.5, 12, true, 1.4);
        return true;
    }
    if (toolId == "terrain.edit.deletepoint") {
        // The centre vertex, crossed out.
        drawTin(ink);
        ink.line(8.5, 8.5, 15.5, 15.5, true, 2.0);
        ink.line(15.5, 8.5, 8.5, 15.5, true, 2.0);
        return true;
    }
    if (toolId == "terrain.edit.movepoint") {
        // The vertex where it was, and an arrow in the accent to where it goes.
        drawTin(ink, false);
        ink.dot(8.5, 13.5, 2.0);
        ink.line(11, 12.8, 17.5, 10.2, true, 1.8);
        ink.line(17.5, 10.2, 14.4, 9.6, true, 1.8);
        ink.line(17.5, 10.2, 15.6, 12.7, true, 1.8);
        return true;
    }
    if (toolId == "terrain.edit.deleteinside") {
        // A dashed polygon in the accent, the TIN's lines outside it only.
        drawTin(ink, false);
        ink.dashed(polyline({{8, 8}, {16, 8}, {16, 16}, {8, 16}}, true), true);
        return true;
    }
    if (toolId == "terrain.edit.breakline") {
        // A bold line across the TIN.
        drawTin(ink);
        ink.line(3, 13, 21, 9, true, 2.6);
        return true;
    }
    if (toolId == "terrain.edit.boundary") {
        // The TIN inside a closed outline in the accent.
        ink.stroke(polyline({{6, 17}, {9, 7}, {17, 7}, {18, 16}}, true));
        ink.line(9, 7, 18, 16);
        ink.stroke(rectangle(3, 4, 18, 16), true);
        return true;
    }
    if (toolId == "terrain.edit.hole") {
        // The TIN with a void cut in it, the void's rim in the accent.
        drawTin(ink, false);
        ink.fill(rectangle(9, 9, 6, 6), true, 70);
        ink.stroke(rectangle(9, 9, 6, 6), true);
        return true;
    }
    return false;
}

} // namespace katana::qt::tools
