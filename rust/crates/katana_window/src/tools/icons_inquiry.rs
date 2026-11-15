//! Icons of the Distance, Area, ID Point, Angle, List tools (see tool_icons). What is measured is
//! drawn in the neutral tone - the points picked, the outline, the arms - and the measurement in
//! the accent: the span a distance covers, the area filled, the angle's arc. They read at 16 px,
//! so no mark is closer than a stroke to the next.

use super::tool_icons::{STROKE, ToolInk, arc, circle, polyline, pt};

pub fn paint_inquiry_icon(tool_id: &str, ink: &mut ToolInk<'_>) -> bool {
    if tool_id == "inquiry.distance" {
        // Two picked points and the span between them, with the witness ticks a dimension has
        // at its ends.
        ink.line(5.0, 17.0, 19.0, 7.0, true, STROKE);
        ink.line(3.3, 14.6, 6.7, 19.4, true, 1.2);
        ink.line(17.3, 4.6, 20.7, 9.4, true, 1.2);
        ink.node(5.0, 17.0, false);
        ink.node(19.0, 7.0, false);
        return true;
    }
    if tool_id == "inquiry.area" {
        // A four-sided lot, filled: the area is the inside.
        let lot = polyline(
            &[pt(4.0, 18.0), pt(7.0, 6.0), pt(19.0, 4.0), pt(20.0, 17.0)],
            true,
        );
        ink.fill(&lot, true, 110);
        ink.stroke(&lot, false, STROKE);
        return true;
    }
    if tool_id == "inquiry.id" {
        // A point in the crosshairs: where it is is the answer.
        ink.stroke(&circle(12.0, 12.0, 6.5), false, STROKE);
        ink.line(12.0, 2.5, 12.0, 8.0, false, STROKE);
        ink.line(12.0, 16.0, 12.0, 21.5, false, STROKE);
        ink.line(2.5, 12.0, 8.0, 12.0, false, STROKE);
        ink.line(16.0, 12.0, 21.5, 12.0, false, STROKE);
        ink.dot(12.0, 12.0, 2.2, true);
        return true;
    }
    if tool_id == "inquiry.angle" {
        // Two arms from a vertex at (4, 19), and the angle between them: the lower arm runs
        // east, the upper one up at 40 degrees on paper, and the arc of radius 9 sweeps between.
        ink.line(4.0, 19.0, 21.0, 19.0, false, STROKE);
        ink.line(4.0, 19.0, 17.8, 7.4, false, STROKE);
        ink.stroke(&arc(4.0, 19.0, 9.0, 0.0, 40.0), true, STROKE);
        ink.node(4.0, 19.0, false);
        return true;
    }
    if tool_id == "inquiry.list" {
        // A list: lines of text, each with its bullet.
        for y in [6.0, 12.0, 18.0] {
            ink.dot(5.0, y, 1.6, true);
            ink.line(9.0, y, 20.0, y, false, STROKE);
        }
        return true;
    }
    false
}
