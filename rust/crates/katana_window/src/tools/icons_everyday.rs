//! Icons of the Divide, Measure, Lengthen, Reverse, Match Properties, Select Similar and Quick
//! Select tools (see tool_icons): one file for the four small families written together
//! (draw_divide, modify_length, modify_properties and select_tools in katana_cad's tools). The
//! object is in the neutral tone, what the tool makes or does in the accent.

use super::tool_icons::{STROKE, ToolInk, circle, polyline, pt, rectangle};

/// An arrow head at (x, y) pointing along +x (dx = 1) or -x (dx = -1).
fn head(ink: &mut ToolInk<'_>, x: f64, y: f64, dx: f64, accent: bool) {
    ink.stroke(
        &polyline(
            &[
                pt(x - 3.5 * dx, y - 3.5),
                pt(x, y),
                pt(x - 3.5 * dx, y + 3.5),
            ],
            false,
        ),
        accent,
        STROKE,
    );
}

pub fn paint_draw_divide_icon(tool_id: &str, ink: &mut ToolInk<'_>) -> bool {
    if tool_id == "draw.divide" {
        // A line cut into four equal parts: its ends and the three points at its quarters,
        // 3 + 18 * k / 4.
        ink.line(3.0, 14.0, 21.0, 14.0, false, STROKE);
        ink.node(3.0, 14.0, false);
        ink.node(21.0, 14.0, false);
        for x in [7.5, 12.0, 16.5] {
            ink.line(x, 9.0, x, 19.0, true, 1.4);
        }
        return true;
    }
    if tool_id == "draw.measure" {
        // A line with a mark every 5 from its left end, and the last stretch short of a full
        // step - measuring stops before the end.
        ink.line(3.0, 15.0, 21.0, 15.0, false, STROKE);
        ink.node(3.0, 15.0, false);
        for x in [8.0, 13.0, 18.0] {
            ink.line(x, 10.0, x, 20.0, true, 1.4);
        }
        ink.line(3.0, 6.0, 8.0, 6.0, true, 1.1);
        ink.line(3.0, 4.0, 3.0, 8.0, true, 1.1);
        ink.line(8.0, 4.0, 8.0, 8.0, true, 1.1);
        return true;
    }
    false
}

pub fn paint_modify_length_icon(tool_id: &str, ink: &mut ToolInk<'_>) -> bool {
    if tool_id == "modify.lengthen" {
        // A line and the length added at its end, arrowed.
        ink.line(3.0, 12.0, 12.0, 12.0, false, STROKE);
        ink.node(3.0, 12.0, false);
        ink.line(12.0, 12.0, 20.5, 12.0, true, STROKE);
        head(ink, 21.0, 12.0, 1.0, true);
        return true;
    }
    if tool_id == "modify.reverse" {
        // The same line run the other way: the old direction neutral above, the new one in the
        // accent below.
        ink.line(4.0, 8.0, 19.0, 8.0, false, STROKE);
        head(ink, 20.0, 8.0, 1.0, false);
        ink.line(5.0, 16.0, 20.0, 16.0, true, STROKE);
        head(ink, 4.0, 16.0, -1.0, true);
        return true;
    }
    false
}

pub fn paint_property_icon(tool_id: &str, ink: &mut ToolInk<'_>) -> bool {
    if tool_id == "modify.match_properties" {
        // A brush: its handle neutral, its bristles carrying the accent onto the stroke it
        // paints.
        ink.line(19.0, 3.0, 12.5, 11.5, false, 2.4);
        ink.fill(
            &polyline(
                &[
                    pt(10.0, 10.0),
                    pt(14.5, 13.5),
                    pt(10.0, 19.0),
                    pt(5.5, 15.5),
                ],
                true,
            ),
            true,
            255,
        );
        ink.line(4.0, 21.0, 20.0, 21.0, true, STROKE);
        return true;
    }
    false
}

pub fn paint_select_icon(tool_id: &str, ink: &mut ToolInk<'_>) -> bool {
    if tool_id == "select.similar" {
        // One circle picked (neutral) and the two like it found (accent).
        ink.stroke(&circle(6.0, 17.0, 3.2), false, STROKE);
        ink.dashed(&rectangle(1.5, 12.5, 9.0, 9.0), false);
        ink.stroke(&circle(17.0, 17.0, 3.2), true, STROKE);
        ink.stroke(&circle(12.0, 6.5, 3.2), true, STROKE);
        return true;
    }
    if tool_id == "select.quick" {
        // A funnel: everything goes in at the top, what meets the condition comes out at the
        // bottom.
        ink.stroke(
            &polyline(
                &[
                    pt(3.0, 4.0),
                    pt(21.0, 4.0),
                    pt(14.0, 12.0),
                    pt(14.0, 19.0),
                    pt(10.0, 21.0),
                    pt(10.0, 12.0),
                ],
                true,
            ),
            false,
            STROKE,
        );
        ink.line(12.0, 13.0, 12.0, 22.5, true, 1.4);
        ink.dot(12.0, 8.0, 1.6, true);
        return true;
    }
    false
}
