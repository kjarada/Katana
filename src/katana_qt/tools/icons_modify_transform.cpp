// Icons of the Move, Copy, Rotate, Scale, Mirror, Stretch, Array and Erase
// tools (see tool_icons.hpp).
//
// Neutral is the drawing as it was, the accent what the tool makes of it: the
// copies of an array, the mirror image, the turn of Rotate. Move, Copy and
// Erase keep the shapes icons.cpp gave the actions they replace, so a user
// who knew the old toolbar finds the same buttons.

#include <cmath>
#include <numbers>

#include "tools/tool_icons.hpp"

namespace katana::qt::tools {

namespace {

// A chevron arrowhead with its tip at (x, y), pointing along `degrees`
// counter-clockwise from east as on paper (the painter's y points down).
void arrowHead(const ToolInk& ink, double x, double y, double degrees, double size = 3.2)
{
    const double radians = degrees * std::numbers::pi / 180.0;
    const QPointF along(std::cos(radians), -std::sin(radians));
    const QPointF across(-along.y(), along.x());
    const QPointF tip(x, y);
    const QPointF back = tip - along * size;
    ink.stroke(polyline({back + across * size, tip, back - across * size}), true);
}

void paintMove(const ToolInk& ink)
{
    ink.line(12, 4, 12, 20);
    ink.line(4, 12, 20, 12);
    ink.stroke(polyline({{9, 6}, {12, 3}, {15, 6}}), true);
    ink.stroke(polyline({{9, 18}, {12, 21}, {15, 18}}), true);
    ink.stroke(polyline({{6, 9}, {3, 12}, {6, 15}}), true);
    ink.stroke(polyline({{18, 9}, {21, 12}, {18, 15}}), true);
}

void paintCopy(const ToolInk& ink)
{
    ink.stroke(polyline({{8.5, 15}, {4, 15}, {4, 4}, {15, 4}, {15, 8.5}}));
    ink.stroke(rectangle(9, 9, 11, 11), true);
}

// A square given a small turn, inside the curved arrow that turned it; the
// arrow leaves a gap at the right, where its head is. The square is kept
// small so that at 16 px the arrow, not the square, is what reads.
void paintRotate(const ToolInk& ink)
{
    QPainterPath square;
    const double half = 3.6;
    const double turn = 25.0 * std::numbers::pi / 180.0;
    for (int corner = 0; corner < 4; ++corner) {
        const double angle = turn + corner * std::numbers::pi / 2.0 + std::numbers::pi / 4.0;
        const QPointF p(12 + half * std::sqrt(2.0) * std::cos(angle),
                        12 - half * std::sqrt(2.0) * std::sin(angle));
        if (corner == 0) {
            square.moveTo(p);
        } else {
            square.lineTo(p);
        }
    }
    square.closeSubpath();
    ink.stroke(square);
    // From 60 degrees, counter-clockwise round to 330: the head is at 330,
    // pointing along the turn (330 + 90 = 60 degrees).
    constexpr double kRadius = 9.2;
    ink.stroke(arc(12, 12, kRadius, 60, 270), true);
    const double end = 330.0 * std::numbers::pi / 180.0;
    arrowHead(ink, 12 + kRadius * std::cos(end), 12 - kRadius * std::sin(end), 60.0);
}

// A small square grown from its corner into a large one along the arrow.
void paintScale(const ToolInk& ink)
{
    ink.stroke(rectangle(4, 4, 16, 16), true);
    ink.stroke(rectangle(4, 13, 7, 7));
    ink.line(11, 13, 17, 7, true);
    arrowHead(ink, 17.5, 6.5, 45.0, 3.0);
    ink.node(4, 20);
}

// A flag and its mirror image either side of the dashed mirror line.
void paintMirror(const ToolInk& ink)
{
    ink.stroke(polyline({{3.5, 19}, {9.5, 19}, {9.5, 6}}, true));
    ink.stroke(polyline({{20.5, 19}, {14.5, 19}, {14.5, 6}}, true), true);
    ink.dashed(polyline({{12, 2.5}, {12, 21.5}}));
}

// A rectangle whose right-hand side has been pulled out: the moved edge and
// its corners in the accent, the arrow showing which way.
void paintStretch(const ToolInk& ink)
{
    ink.stroke(polyline({{20, 7}, {3, 7}, {3, 17}, {20, 17}}));
    ink.line(20, 7, 20, 17, true);
    ink.node(20, 7, true);
    ink.node(20, 17, true);
    ink.line(8, 12, 15.5, 12, true);
    arrowHead(ink, 16, 12, 0.0, 2.8);
}

// Three rows of three: the original, bottom left, and its eight copies.
void paintArrayRectangular(const ToolInk& ink)
{
    for (int row = 0; row < 3; ++row) {
        for (int column = 0; column < 3; ++column) {
            const bool original = row == 2 && column == 0;
            ink.fill(rectangle(3.5 + 6.5 * column, 3.5 + 6.5 * row, 4.5, 4.5), !original);
        }
    }
}

// Six items round a centre: the original, due east, and five copies. No
// circle joins them: drawn dashed between the dots it filled the ring in at
// 16 px and the items were lost in it.
void paintArrayPolar(const ToolInk& ink)
{
    for (int item = 0; item < 6; ++item) {
        const double angle = item * std::numbers::pi / 3.0;
        ink.dot(12 + 8.0 * std::cos(angle), 12 - 8.0 * std::sin(angle), 2.4, item != 0);
    }
    ink.node(12, 12);
}

void paintErase(const ToolInk& ink)
{
    ink.line(4, 7, 20, 7);
    ink.stroke(polyline({{9, 7}, {9, 4.5}, {15, 4.5}, {15, 7}}));
    ink.stroke(polyline({{6, 7}, {7, 21}, {17, 21}, {18, 7}}));
    ink.line(10, 11, 10, 17, true);
    ink.line(14, 11, 14, 17, true);
}

} // namespace

bool paintModifyTransformIcon(std::string_view toolId, const ToolInk& ink)
{
    if (toolId == "modify.move") {
        paintMove(ink);
    } else if (toolId == "modify.copy") {
        paintCopy(ink);
    } else if (toolId == "modify.rotate") {
        paintRotate(ink);
    } else if (toolId == "modify.scale") {
        paintScale(ink);
    } else if (toolId == "modify.mirror") {
        paintMirror(ink);
    } else if (toolId == "modify.stretch") {
        paintStretch(ink);
    } else if (toolId == "modify.array_rectangular") {
        paintArrayRectangular(ink);
    } else if (toolId == "modify.array_polar") {
        paintArrayPolar(ink);
    } else if (toolId == "modify.erase") {
        paintErase(ink);
    } else {
        return false;
    }
    return true;
}

} // namespace katana::qt::tools
