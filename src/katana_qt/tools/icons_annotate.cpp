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
#include <numbers>

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

void paintMultilineText(const ToolInk& ink)
{
    // Text's capital A, smaller, over two lines of text inside the frame of
    // one block: several lines that are one thing.
    ink.stroke(rectangle(3, 3, 18, 18), false, 1.1);
    ink.stroke(polyline({{6, 11}, {9.5, 5}, {13, 11}}), true);
    ink.line(7.5, 8.8, 11.5, 8.8, true);
    ink.line(6, 14.5, 18, 14.5, false, 1.4);
    ink.line(6, 18, 15, 18, false, 1.4);
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

void paintAngularDimension(const ToolInk& ink)
{
    // Two lines from a vertex at the bottom left, one level and one rising
    // steeply, and the arc between them with an arrow at each end. The angle
    // is wide and the arc large so that the arc still shows between its two
    // arrowheads at 16 px.
    constexpr double kVertexX = 3.0;
    constexpr double kVertexY = 21.0;
    constexpr double kRiseX = 13.0;
    constexpr double kRiseY = 3.0;
    ink.line(kVertexX, kVertexY, 21.5, kVertexY, false, kExtensionWidth);
    ink.line(kVertexX, kVertexY, kRiseX, kRiseY, false, kExtensionWidth);
    ink.node(kVertexX, kVertexY);
    // Radius 15 from the level line (0 degrees) up to the rising one, at
    // atan2(18, 10) = 60.9 degrees on paper; the arc itself stops 17 degrees
    // short of each line, where the arrowheads take over.
    constexpr double kRadius = 15.0;
    const double rising = std::atan2(kVertexY - kRiseY, kRiseX - kVertexX);
    const double risingDegrees = rising * 180.0 / std::numbers::pi;
    ink.stroke(arc(kVertexX, kVertexY, kRadius, 17.0, risingDegrees - 34.0), true);
    arrowhead(ink, kVertexX + kRadius, kVertexY, 0, 1);
    arrowhead(ink, kVertexX + kRadius * std::cos(rising), kVertexY - kRadius * std::sin(rising),
              -std::sin(rising), -std::cos(rising));
}

void paintRadialDimension(const ToolInk& ink, bool diameter)
{
    // A circle, and the dimension line from its centre (radius) or across it
    // (diameter) with an arrow on the curve.
    constexpr double kCentreX = 11.0;
    constexpr double kCentreY = 13.0;
    constexpr double kRadius = 8.0;
    ink.stroke(circle(kCentreX, kCentreY, kRadius), false, kExtensionWidth);
    const double ux = std::cos(0.7);
    const double uy = -std::sin(0.7);
    const double tipX = kCentreX + ux * kRadius;
    const double tipY = kCentreY + uy * kRadius;
    if (diameter) {
        const double backX = kCentreX - ux * kRadius;
        const double backY = kCentreY - uy * kRadius;
        ink.line(backX + ux * 4.0, backY + uy * 4.0, tipX - ux * 4.0, tipY - uy * 4.0, true);
        arrowhead(ink, backX, backY, -ux, -uy);
    } else {
        ink.node(kCentreX, kCentreY);
        ink.line(kCentreX, kCentreY, tipX - ux * 4.0, tipY - uy * 4.0, true);
    }
    arrowhead(ink, tipX, tipY, ux, uy);
}

void paintOrdinateDimension(const ToolInk& ink)
{
    // The datum's two axes at the bottom left, a feature, and its leader
    // rising to where the value is written.
    ink.line(3, 21, 21, 21, false, 1.1);
    ink.line(3, 21, 3, 3, false, 1.1);
    ink.node(3, 21);
    ink.node(13, 16);
    ink.stroke(polyline({{13, 13.5}, {13, 9}, {16, 6}}), true);
    ink.line(16, 6, 21, 6, true);
}

} // namespace

bool paintAnnotateIcon(std::string_view toolId, const ToolInk& ink)
{
    if (toolId == "annotate.text") {
        paintText(ink);
    } else if (toolId == "annotate.mtext") {
        paintMultilineText(ink);
    } else if (toolId == "annotate.dimlinear") {
        paintLinearDimension(ink);
    } else if (toolId == "annotate.dimaligned") {
        paintAlignedDimension(ink);
    } else if (toolId == "annotate.dimangular") {
        paintAngularDimension(ink);
    } else if (toolId == "annotate.dimradius") {
        paintRadialDimension(ink, false);
    } else if (toolId == "annotate.dimdiameter") {
        paintRadialDimension(ink, true);
    } else if (toolId == "annotate.dimordinate") {
        paintOrdinateDimension(ink);
    } else if (toolId == "annotate.leader") {
        paintLeader(ink);
    } else {
        return false;
    }
    return true;
}

} // namespace katana::qt::tools
