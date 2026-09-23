// Icons of the Circle and Arc tools in their construction variants (see
// tool_icons.hpp).
//
// As in icons.cpp, the curve the tool draws is in the accent and what the user
// picks to construct it - a centre, a point on it, the lines it touches - is
// neutral. The variants of one curve share its shape, so they read as one
// family in the menu, and differ only in the picks: at 16 px a centre node, a
// diameter or three points on the rim is what tells them apart.

#include <cmath>

#include "tools/tool_icons.hpp"

namespace katana::qt::tools {

namespace {

// The point at `degrees` (counter-clockwise on paper) on the circle about
// (cx, cy): the painter's y points down, so sine is subtracted.
QPointF onCircle(double cx, double cy, double radius, double degrees)
{
    const double radians = degrees * 3.14159265358979323846 / 180.0;
    return {cx + radius * std::cos(radians), cy - radius * std::sin(radians)};
}

// A filled arrowhead with its tip at `tip`, pointing from `from`: the head of
// a radius dimension, as the drawing's own dimensions draw one.
void arrowhead(const ToolInk& ink, const QPointF& from, const QPointF& tip)
{
    const QPointF along = tip - from;
    const double length = std::hypot(along.x(), along.y());
    const QPointF ahead = along / length;
    const QPointF side(-ahead.y(), ahead.x());
    ink.fill(polyline({tip, tip - ahead * 3.6 + side * 1.6, tip - ahead * 3.6 - side * 1.6}, true));
}

// A picked point ON the curve: a node a size larger than ToolInk::node, which
// at 16 px is two pixels and vanishes into the curve's own stroke.
void pick(const ToolInk& ink, double x, double y)
{
    ink.fill(rectangle(x - 2.1, y - 2.1, 4.2, 4.2));
}

} // namespace

bool paintDrawCurveIcon(std::string_view toolId, const ToolInk& ink)
{
    if (toolId == "draw.circle") {
        // The application's Circle: a radius at 45 degrees from a centre node.
        ink.stroke(circle(12, 12, 8), true);
        ink.line(12, 12, 17.66, 6.34, false, 1.1);
        ink.node(12, 12);
        ink.node(17.66, 6.34);
        return true;
    }
    if (toolId == "draw.circle.diameter") {
        // The radius carried through the centre to the far side.
        ink.stroke(circle(12, 12, 8), true);
        ink.line(6.34, 17.66, 17.66, 6.34, false, 1.1);
        ink.node(12, 12);
        ink.node(6.34, 17.66);
        ink.node(17.66, 6.34);
        return true;
    }
    if (toolId == "draw.circle.2p") {
        // Two picks on the rim, opposite; no centre, which is never picked.
        ink.stroke(circle(12, 12, 8), true);
        pick(ink, 4, 12);
        pick(ink, 20, 12);
        return true;
    }
    if (toolId == "draw.circle.3p") {
        ink.stroke(circle(12, 12, 8), true);
        for (const double degrees : {90.0, 210.0, 330.0}) {
            const QPointF p = onCircle(12, 12, 8, degrees);
            pick(ink, p.x(), p.y());
        }
        return true;
    }
    if (toolId == "draw.circle.ttr") {
        // Two lines meeting in a corner, and the circle sitting in it touching
        // both; the touch points are the picks.
        ink.line(4, 3, 4, 21);
        ink.line(3, 20, 21, 20);
        ink.stroke(circle(11.5, 12.5, 7.5), true);
        pick(ink, 4, 12.5);
        pick(ink, 11.5, 20);
        return true;
    }
    if (toolId == "draw.arc") {
        // The application's Arc as a true arc through the same three nodes,
        // (4, 18), (12, 8.5) and (20, 18): chord 16, rise 9.5, so its radius is
        // (8^2 + 9.5^2) / (2 * 9.5) = 8.12 about (12, 16.62), and it runs from
        // 189.8 degrees clockwise on paper to -9.8.
        ink.stroke(arc(12, 16.62, 8.12, 189.8, -199.6), true);
        pick(ink, 4, 18);
        pick(ink, 12, 8.5);
        pick(ink, 20, 18);
        return true;
    }
    if (toolId == "draw.arc.sce" || toolId == "draw.arc.cse") {
        // A quarter arc about a picked centre, with its radii: the start on
        // the right, the end above, counter-clockwise between them. The first
        // pick of each is the stronger mark - the start as a dot for
        // start-centre-end, the centre as a cross for centre-start-end - since
        // the two differ only in the order of the picks.
        const bool centreFirst = toolId == "draw.arc.cse";
        ink.stroke(arc(6, 18, 13, 0, 90), true);
        ink.line(6, 18, 19, 18, false, 1.1);
        ink.line(6, 18, 6, 5, false, 1.1);
        ink.node(6, 5);
        if (centreFirst) {
            ink.line(6, 14.2, 6, 21.8, false, 1.7);
            ink.line(2.2, 18, 9.8, 18, false, 1.7);
            ink.node(19, 18);
        } else {
            ink.node(6, 18);
            ink.dot(19, 18, 2.5);
        }
        return true;
    }
    if (toolId == "draw.arc.ser") {
        // The two ends, picked, and a radius dimension from the centre to the
        // arc: the value typed is the radius. The centre is not marked - it is
        // never picked, and a dot there made the icon read as a dial. The arc is about (12, 20),
        // radius 10, from 30 to 150 degrees; the ends are at y = 20 - 5.
        ink.stroke(arc(12, 20, 10, 30, 120), true);
        pick(ink, 20.66, 15);
        pick(ink, 3.34, 15);
        const QPointF onArc = onCircle(12, 20, 10, 65);
        ink.line(12, 20, onArc.x(), onArc.y(), false, 1.1);
        arrowhead(ink, QPointF(12, 20), onArc);
        return true;
    }
    return false;
}

} // namespace katana::qt::tools
