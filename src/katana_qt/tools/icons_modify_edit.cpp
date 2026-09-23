// Icons of the Trim, Extend, Offset, Fillet, Chamfer, Break, Break at Point,
// Join and Explode tools (see tool_icons.hpp). Each shows the object in the
// neutral tone and what the tool does to it in the accent: the part a trim
// takes away, the length an extend adds, the copy an offset makes, the arc a
// fillet puts in the corner. They are drawn to be read at 16 px, so every
// mark is at least a stroke apart from the next.

#include "tools/tool_icons.hpp"

#include <QPainter>

namespace katana::qt::tools {

namespace {

// A corner the fillet and chamfer icons round off: two neutral lines stopping
// short of it, and the corner they used to make, dashed.
void corner(const ToolInk& ink)
{
    ink.line(5, 21, 5, 12);
    ink.line(12, 5, 21, 5);
    ink.dashed(polyline({{5, 12}, {5, 5}, {12, 5}}));
}

} // namespace

bool paintModifyEditIcon(std::string_view toolId, const ToolInk& ink)
{
    if (toolId == "modify.trim") {
        // A cutting edge, the line it crosses, and the end beyond it going,
        // as two accent dashes. Drawn as two strokes rather than with a dash
        // pattern: a pattern's dashes are a pixel long at 16 px and blur into
        // a faint line, and a cross marking the cut blurred into a blob.
        ink.line(13, 3, 13, 21);
        ink.line(3, 12, 13, 12);
        ink.line(15.5, 12, 17.5, 12, true);
        ink.line(20, 12, 22, 12, true);
        return true;
    }
    if (toolId == "modify.extend") {
        // A boundary, a line, and its new length reaching the boundary.
        ink.line(20, 3, 20, 21);
        ink.line(3, 12, 10, 12);
        ink.line(10, 12, 18.5, 12, true);
        ink.stroke(polyline({{14.5, 8}, {18.5, 12}, {14.5, 16}}), true);
        ink.node(3, 12);
        return true;
    }
    if (toolId == "modify.offset") {
        // A bent line and its parallel copy 5 inside it: the vertical moves
        // to x = 9, the horizontal to y = 9, and the diagonal x + y = 15
        // moves 5 along its normal to x + y = 15 + 5 * sqrt(2), 22.07.
        ink.stroke(polyline({{4, 21}, {4, 11}, {11, 4}, {21, 4}}));
        ink.stroke(polyline({{9, 21}, {9, 13.07}, {13.07, 9}, {21, 9}}), true);
        return true;
    }
    if (toolId == "modify.fillet") {
        // Radius 7 about (12,12): tangent to x = 5 at (5,12), to y = 5 at (12,5).
        corner(ink);
        ink.stroke(arc(12, 12, 7, 180, -90), true);
        return true;
    }
    if (toolId == "modify.chamfer") {
        corner(ink);
        ink.line(5, 12, 12, 5, true);
        return true;
    }
    if (toolId == "modify.break") {
        // A line with a gap cut out of it, the two break points marked. The
        // gap is left empty: dashed, it read as an unbroken line at 16 px.
        ink.line(3, 20, 8.5, 15.5);
        ink.line(15.5, 8.5, 21, 4);
        ink.node(8.5, 15.5, true);
        ink.node(15.5, 8.5, true);
        return true;
    }
    if (toolId == "modify.break_at_point") {
        // One line, one break point: the pieces meet there, marked across.
        ink.line(3, 20, 21, 4);
        ink.line(9, 7, 15, 17, true);
        ink.node(12, 12, true);
        return true;
    }
    if (toolId == "modify.join") {
        // Two lines brought together at one vertex, which is the new part.
        ink.line(3, 19, 10.5, 8.5);
        ink.line(13.5, 8.5, 21, 19);
        ink.stroke(polyline({{10.5, 8.5}, {12, 6.5}, {13.5, 8.5}}), true);
        ink.dot(12, 6.5, 2.2, true);
        ink.node(3, 19);
        ink.node(21, 19);
        return true;
    }
    if (toolId == "modify.explode") {
        // A rectangle's four sides pulled apart, with the burst between them.
        ink.line(8, 4, 16, 4);
        ink.line(8, 20, 16, 20);
        ink.line(4, 8, 4, 16);
        ink.line(20, 8, 20, 16);
        ink.line(9.5, 9.5, 6.5, 6.5, true);
        ink.line(14.5, 9.5, 17.5, 6.5, true);
        ink.line(9.5, 14.5, 6.5, 17.5, true);
        ink.line(14.5, 14.5, 17.5, 17.5, true);
        return true;
    }
    return false;
}

} // namespace katana::qt::tools
