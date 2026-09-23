// Icons of the Text and dimensions tools (see tool_icons.hpp).
//
// The accent is what the tool makes - the letter, the dimension line and its
// arrows, the leader - and the neutral tone is what it is made against: the
// start point and baseline, the measured points and their extension lines,
// the note. The two dimensions share one language - two points, extension
// lines, an arrowed line - so they read as a pair and differ only in how the
// line lies.
// Arrowheads are filled triangles rather than strokes: at 16 px two strokes
// meeting at a point blur into a blob, where a solid wedge still reads as an
// arrow.

#include <cmath>

#include "tools/tool_icons.hpp"

namespace katana::qt::tools {

namespace {

// Thinner than the dimension line, as on a drawing, but not the 1.1 of a
// construction line: a diagonal that thin anti-aliases away at 16 px.
constexpr double kExtensionWidth = 1.3;

// A filled arrowhead with its point at (x, y), pointing along (dx, dy), which
// need not be a unit vector.
void arrowhead(const ToolInk& ink, double x, double y, double dx, double dy)
{
    // Measured at 16 px: shorter than this and the head reads as a thicker
    // end of the line rather than an arrow.
    constexpr double kLength = 5.0;
    constexpr double kHalfWidth = 2.2;
    const double norm = std::hypot(dx, dy);
    const double ux = dx / norm;
    const double uy = dy / norm;
    const double backX = x - ux * kLength;
    const double backY = y - uy * kLength;
    ink.fill(polyline({{x, y},
                       {backX - uy * kHalfWidth, backY + ux * kHalfWidth},
                       {backX + uy * kHalfWidth, backY - ux * kHalfWidth}},
                      true),
             true);
}

void paintText(const ToolInk& ink)
{
    // A capital A, drawn as strokes rather than set in a font, so it is the
    // same letter on every machine and stays sharp at 16 px. It stands on the
    // baseline from the start point the tool asks for first. (A text cursor
    // beside the letter was tried and read as "AI".)
    ink.stroke(polyline({{5, 17.5}, {12, 3}, {19, 17.5}}), true);
    ink.line(7.9, 12, 16.1, 12, true);
    ink.line(4, 21, 21, 21, false, 1.1);
    ink.node(4, 21);
}

void paintLinearDimension(const ToolInk& ink)
{
    // Two points at different heights; the dimension measures only across.
    ink.line(4, 18.5, 4, 4.5, false, kExtensionWidth);
    ink.line(20, 13.5, 20, 4.5, false, kExtensionWidth);
    ink.node(4, 20.5);
    ink.node(20, 15.5);
    ink.line(8, 8, 16, 8, true);
    arrowhead(ink, 4, 8, -1, 0);
    arrowhead(ink, 20, 8, 1, 0);
}

void paintAlignedDimension(const ToolInk& ink)
{
    // Two points on a diagonal and the dimension parallel to them, offset up
    // and to the left along the normal (-1, -1) / sqrt 2. No line joins the
    // points: with one, the four sides closed into a box and read as a
    // rotated rectangle at 16 px.
    constexpr double kOffset = 8.0;
    const double shift = kOffset / std::sqrt(2.0);
    const double ax = 8.5;
    const double ay = 21.0;
    const double bx = 21.0;
    const double by = 8.5;
    ink.node(ax, ay);
    ink.node(bx, by);
    // Extension lines from just off each point to just past the dimension line.
    const double gap = 2.0 / std::sqrt(2.0);
    const double beyond = 1.2 / std::sqrt(2.0);
    ink.line(ax - gap, ay - gap, ax - shift - beyond, ay - shift - beyond, false, kExtensionWidth);
    ink.line(bx - gap, by - gap, bx - shift - beyond, by - shift - beyond, false, kExtensionWidth);
    const double dax = ax - shift;
    const double day = ay - shift;
    const double dbx = bx - shift;
    const double dby = by - shift;
    // The line stops short of the tips, which the arrowheads fill.
    const double inset = 4.0 / std::sqrt(2.0);
    ink.line(dax + inset, day - inset, dbx - inset, dby + inset, true);
    arrowhead(ink, dax, day, -1, 1);
    arrowhead(ink, dbx, dby, 1, -1);
}

void paintLeader(const ToolInk& ink)
{
    // The arrow touches a feature at the bottom left; the leader rises to a
    // short landing and the note stands beside it.
    constexpr double kTipX = 4.0;
    constexpr double kTipY = 20.5;
    constexpr double kBendX = 11.0;
    constexpr double kBendY = 9.0;
    const double dx = kTipX - kBendX;
    const double dy = kTipY - kBendY;
    const double length = std::hypot(dx, dy);
    // The line ends inside the arrowhead, so no gap shows between them.
    const double reach = (length - 3.5) / length;
    ink.stroke(polyline({{kBendX + dx * reach, kBendY + dy * reach}, {kBendX, kBendY}, {13.5, 9}}),
               true);
    arrowhead(ink, kTipX, kTipY, dx, dy);
    // Two lines of it, centred on the landing as the tool places a note; a
    // third merged the three into one block at 16 px.
    ink.line(16, 7, 21.5, 7, false, 1.6);
    ink.line(16, 11, 20, 11, false, 1.6);
}

} // namespace

bool paintAnnotateIcon(std::string_view toolId, const ToolInk& ink)
{
    if (toolId == "annotate.text") {
        paintText(ink);
    } else if (toolId == "annotate.dimlinear") {
        paintLinearDimension(ink);
    } else if (toolId == "annotate.dimaligned") {
        paintAlignedDimension(ink);
    } else if (toolId == "annotate.leader") {
        paintLeader(ink);
    } else {
        return false;
    }
    return true;
}

} // namespace katana::qt::tools
