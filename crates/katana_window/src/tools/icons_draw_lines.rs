//! Icons of the Point, Line, Polyline, Rectangle, Polygon tools (see tool_icons).
//!
//! Point, Line, Polyline and Rectangle are the drawings the toolbar already has for them (icons),
//! so the tools look the same when the catalogue's menus and toolbars replace the hand-made
//! actions: what is drawn in the accent, the points the user gives as neutral nodes. Polygon is
//! drawn as Circle is - the shape in the accent, its centre and a radius in the neutral tone -
//! with the radius going off to a vertex, so the icon reads as "centre and vertex", the tool's
//! default way of drawing one. Rectangle marks only the two opposite corners it is drawn from,
//! where icons marks all four.

use super::tool_icons::{STROKE, ToolInk, polyline, pt, rectangle};

pub fn paint_draw_line_icon(tool_id: &str, ink: &mut ToolInk<'_>) -> bool {
    if tool_id == "draw.point" {
        ink.line(12.0, 3.0, 12.0, 8.0, false, STROKE);
        ink.line(12.0, 16.0, 12.0, 21.0, false, STROKE);
        ink.line(3.0, 12.0, 8.0, 12.0, false, STROKE);
        ink.line(16.0, 12.0, 21.0, 12.0, false, STROKE);
        ink.dot(12.0, 12.0, 2.6, true);
        return true;
    }
    if tool_id == "draw.line" {
        ink.line(5.0, 19.0, 19.0, 5.0, true, STROKE);
        ink.node(5.0, 19.0, false);
        ink.node(19.0, 5.0, false);
        return true;
    }
    if tool_id == "draw.polyline" {
        let vertices = [pt(4.0, 18.0), pt(9.0, 8.0), pt(15.0, 15.0), pt(20.0, 5.0)];
        ink.stroke(&polyline(&vertices, false), true, STROKE);
        for p in vertices {
            ink.node(p.x, p.y, false);
        }
        return true;
    }
    if tool_id == "draw.rectangle" {
        ink.stroke(&rectangle(4.0, 6.0, 16.0, 12.0), true, STROKE);
        // The two corners the tool asks for, not all four: a rectangle is given by opposite
        // corners.
        ink.node(4.0, 18.0, false);
        ink.node(20.0, 6.0, false);
        return true;
    }
    if tool_id == "draw.polygon" {
        // A pentagon of radius 9 about (12, 13), point up: vertices at 90, 18, -54, -126 and 162
        // degrees on paper (9 cos 18 = 8.56, 9 sin 18 = 2.78, 9 cos 54 = 5.29, 9 sin 54 = 7.28).
        // Five sides rather than six: at 16 px a hexagon rounds off into Circle's icon, a
        // pentagon keeps its corners. The centre sits low because a pentagon's bulk does.
        let top = pt(12.0, 4.0);
        let right = pt(20.56, 10.22);
        let lower_right = pt(17.29, 20.28);
        let lower_left = pt(6.71, 20.28);
        let left = pt(3.44, 10.22);
        ink.stroke(
            &polyline(&[top, right, lower_right, lower_left, left], true),
            true,
            STROKE,
        );
        ink.line(12.0, 13.0, right.x, right.y, false, 1.1);
        ink.node(12.0, 13.0, false);
        ink.node(right.x, right.y, false);
        return true;
    }
    false
}
