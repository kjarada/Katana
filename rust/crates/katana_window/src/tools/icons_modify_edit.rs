//! Icons of the Trim, Extend, Offset, Fillet, Chamfer, Break, Break at Point, Join and Explode
//! tools (see tool_icons). Each shows the object in the neutral tone and what the tool does to it
//! in the accent: the part a trim takes away, the length an extend adds, the copy an offset
//! makes, the arc a fillet puts in the corner. They are drawn to be read at 16 px, so every mark
//! is at least a stroke apart from the next.

use super::tool_icons::{STROKE, ToolInk, arc, polyline, pt};

/// A corner the fillet and chamfer icons round off: two neutral lines stopping short of it, and
/// the corner they used to make, dashed.
fn corner(ink: &mut ToolInk<'_>) {
    ink.line(5.0, 21.0, 5.0, 12.0, false, STROKE);
    ink.line(12.0, 5.0, 21.0, 5.0, false, STROKE);
    ink.dashed(
        &polyline(&[pt(5.0, 12.0), pt(5.0, 5.0), pt(12.0, 5.0)], false),
        false,
    );
}

pub fn paint_modify_edit_icon(tool_id: &str, ink: &mut ToolInk<'_>) -> bool {
    if tool_id == "modify.trim" {
        // A cutting edge, the line it crosses, and the end beyond it going, as two accent
        // dashes. Drawn as two strokes rather than with a dash pattern: a pattern's dashes are a
        // pixel long at 16 px and blur into a faint line, and a cross marking the cut blurred
        // into a blob.
        ink.line(13.0, 3.0, 13.0, 21.0, false, STROKE);
        ink.line(3.0, 12.0, 13.0, 12.0, false, STROKE);
        ink.line(15.5, 12.0, 17.5, 12.0, true, STROKE);
        ink.line(20.0, 12.0, 22.0, 12.0, true, STROKE);
        return true;
    }
    if tool_id == "modify.extend" {
        // A boundary, a line, and its new length reaching the boundary.
        ink.line(20.0, 3.0, 20.0, 21.0, false, STROKE);
        ink.line(3.0, 12.0, 10.0, 12.0, false, STROKE);
        ink.line(10.0, 12.0, 18.5, 12.0, true, STROKE);
        ink.stroke(
            &polyline(&[pt(14.5, 8.0), pt(18.5, 12.0), pt(14.5, 16.0)], false),
            true,
            STROKE,
        );
        ink.node(3.0, 12.0, false);
        return true;
    }
    if tool_id == "modify.offset" {
        // A bent line and its parallel copy 5 inside it: the vertical moves to x = 9, the
        // horizontal to y = 9, and the diagonal x + y = 15 moves 5 along its normal to
        // x + y = 15 + 5 * sqrt(2), 22.07.
        ink.stroke(
            &polyline(
                &[pt(4.0, 21.0), pt(4.0, 11.0), pt(11.0, 4.0), pt(21.0, 4.0)],
                false,
            ),
            false,
            STROKE,
        );
        ink.stroke(
            &polyline(
                &[pt(9.0, 21.0), pt(9.0, 13.07), pt(13.07, 9.0), pt(21.0, 9.0)],
                false,
            ),
            true,
            STROKE,
        );
        return true;
    }
    if tool_id == "modify.fillet" {
        // Radius 7 about (12,12): tangent to x = 5 at (5,12), to y = 5 at (12,5).
        corner(ink);
        ink.stroke(&arc(12.0, 12.0, 7.0, 180.0, -90.0), true, STROKE);
        return true;
    }
    if tool_id == "modify.chamfer" {
        corner(ink);
        ink.line(5.0, 12.0, 12.0, 5.0, true, STROKE);
        return true;
    }
    if tool_id == "modify.break" {
        // A line with a gap cut out of it, the two break points marked. The gap is left empty:
        // dashed, it read as an unbroken line at 16 px.
        ink.line(3.0, 20.0, 8.5, 15.5, false, STROKE);
        ink.line(15.5, 8.5, 21.0, 4.0, false, STROKE);
        ink.node(8.5, 15.5, true);
        ink.node(15.5, 8.5, true);
        return true;
    }
    if tool_id == "modify.break_at_point" {
        // One line, one break point: the pieces meet there, marked across.
        ink.line(3.0, 20.0, 21.0, 4.0, false, STROKE);
        ink.line(9.0, 7.0, 15.0, 17.0, true, STROKE);
        ink.node(12.0, 12.0, true);
        return true;
    }
    if tool_id == "modify.join" {
        // Two lines brought together at one vertex, which is the new part.
        ink.line(3.0, 19.0, 10.5, 8.5, false, STROKE);
        ink.line(13.5, 8.5, 21.0, 19.0, false, STROKE);
        ink.stroke(
            &polyline(&[pt(10.5, 8.5), pt(12.0, 6.5), pt(13.5, 8.5)], false),
            true,
            STROKE,
        );
        ink.dot(12.0, 6.5, 2.2, true);
        ink.node(3.0, 19.0, false);
        ink.node(21.0, 19.0, false);
        return true;
    }
    if tool_id == "modify.explode" {
        // A rectangle's four sides pulled apart, with the burst between them.
        ink.line(8.0, 4.0, 16.0, 4.0, false, STROKE);
        ink.line(8.0, 20.0, 16.0, 20.0, false, STROKE);
        ink.line(4.0, 8.0, 4.0, 16.0, false, STROKE);
        ink.line(20.0, 8.0, 20.0, 16.0, false, STROKE);
        ink.line(9.5, 9.5, 6.5, 6.5, true, STROKE);
        ink.line(14.5, 9.5, 17.5, 6.5, true, STROKE);
        ink.line(9.5, 14.5, 6.5, 17.5, true, STROKE);
        ink.line(14.5, 14.5, 17.5, 17.5, true, STROKE);
        return true;
    }
    false
}
