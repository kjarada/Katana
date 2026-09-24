// Icons of the Divide, Measure, Lengthen, Reverse, Match Properties, Select
// Similar and Quick Select tools (see tool_icons.hpp): one file for the four
// small families written together (draw_divide, modify_length,
// modify_properties and select_tools in src/katana_cad/tools). The object is
// in the neutral tone, what the tool makes or does in the accent.

#include "tools/tool_icons.hpp"

namespace katana::qt::tools {

namespace {

// An arrow head at (x, y) pointing along +x (dx = 1) or -x (dx = -1).
void head(const ToolInk& ink, double x, double y, double dx, bool accent)
{
    ink.stroke(polyline({{x - 3.5 * dx, y - 3.5}, {x, y}, {x - 3.5 * dx, y + 3.5}}), accent);
}

} // namespace

bool paintDrawDivideIcon(std::string_view toolId, const ToolInk& ink)
{
    if (toolId == "draw.divide") {
        // A line cut into four equal parts: its ends and the three points
        // at its quarters, 3 + 18 * k / 4.
        ink.line(3, 14, 21, 14);
        ink.node(3, 14);
        ink.node(21, 14);
        for (const double x : {7.5, 12.0, 16.5}) {
            ink.line(x, 9, x, 19, true, 1.4);
        }
        return true;
    }
    if (toolId == "draw.measure") {
        // A line with a mark every 5 from its left end, and the last stretch
        // short of a full step - measuring stops before the end.
        ink.line(3, 15, 21, 15);
        ink.node(3, 15);
        for (const double x : {8.0, 13.0, 18.0}) {
            ink.line(x, 10, x, 20, true, 1.4);
        }
        ink.line(3, 6, 8, 6, true, 1.1);
        ink.line(3, 4, 3, 8, true, 1.1);
        ink.line(8, 4, 8, 8, true, 1.1);
        return true;
    }
    return false;
}

bool paintModifyLengthIcon(std::string_view toolId, const ToolInk& ink)
{
    if (toolId == "modify.lengthen") {
        // A line and the length added at its end, arrowed.
        ink.line(3, 12, 12, 12);
        ink.node(3, 12);
        ink.line(12, 12, 20.5, 12, true);
        head(ink, 21, 12, 1, true);
        return true;
    }
    if (toolId == "modify.reverse") {
        // The same line run the other way: the old direction neutral above,
        // the new one in the accent below.
        ink.line(4, 8, 19, 8);
        head(ink, 20, 8, 1, false);
        ink.line(5, 16, 20, 16, true);
        head(ink, 4, 16, -1, true);
        return true;
    }
    return false;
}

bool paintPropertyIcon(std::string_view toolId, const ToolInk& ink)
{
    if (toolId == "modify.match_properties") {
        // A brush: its handle neutral, its bristles carrying the accent onto
        // the stroke it paints.
        ink.line(19, 3, 12.5, 11.5, false, 2.4);
        ink.fill(polyline({{10, 10}, {14.5, 13.5}, {10, 19}, {5.5, 15.5}}, true), true);
        ink.line(4, 21, 20, 21, true);
        return true;
    }
    return false;
}

bool paintSelectIcon(std::string_view toolId, const ToolInk& ink)
{
    if (toolId == "select.similar") {
        // One circle picked (neutral) and the two like it found (accent).
        ink.stroke(circle(6, 17, 3.2));
        ink.dashed(rectangle(1.5, 12.5, 9, 9));
        ink.stroke(circle(17, 17, 3.2), true);
        ink.stroke(circle(12, 6.5, 3.2), true);
        return true;
    }
    if (toolId == "select.quick") {
        // A funnel: everything goes in at the top, what meets the condition
        // comes out at the bottom.
        ink.stroke(polyline({{3, 4}, {21, 4}, {14, 12}, {14, 19}, {10, 21}, {10, 12}}, true));
        ink.line(12, 13, 12, 22.5, true, 1.4);
        ink.dot(12, 8, 1.6, true);
        return true;
    }
    return false;
}

} // namespace katana::qt::tools
