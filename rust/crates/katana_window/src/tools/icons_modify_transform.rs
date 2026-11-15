//! Icons of the Move, Copy, Rotate, Scale, Mirror, Stretch, Array and Erase tools (see
//! tool_icons).
//!
//! Neutral is the drawing as it was, the accent what the tool makes of it: the copies of an
//! array, the mirror image, the turn of Rotate. Move, Copy and Erase keep the shapes icons gave
//! the actions they replace, so a user who knew the old toolbar finds the same buttons.

use std::f64::consts::PI;

use crate::canvas::Path;

use super::tool_icons::{STROKE, ToolInk, arc, polyline, pt, rectangle};

/// A chevron arrowhead with its tip at (x, y), pointing along `degrees` counter-clockwise from
/// east as on paper (the painter's y points down).
fn arrow_head(ink: &mut ToolInk<'_>, x: f64, y: f64, degrees: f64, size: f64) {
    let radians = degrees * PI / 180.0;
    let along = pt(radians.cos(), -radians.sin());
    let across = pt(-along.y, along.x);
    let tip = pt(x, y);
    let back = tip - along * size;
    ink.stroke(
        &polyline(&[back + across * size, tip, back - across * size], false),
        true,
        STROKE,
    );
}

fn paint_move(ink: &mut ToolInk<'_>) {
    ink.line(12.0, 4.0, 12.0, 20.0, false, STROKE);
    ink.line(4.0, 12.0, 20.0, 12.0, false, STROKE);
    ink.stroke(
        &polyline(&[pt(9.0, 6.0), pt(12.0, 3.0), pt(15.0, 6.0)], false),
        true,
        STROKE,
    );
    ink.stroke(
        &polyline(&[pt(9.0, 18.0), pt(12.0, 21.0), pt(15.0, 18.0)], false),
        true,
        STROKE,
    );
    ink.stroke(
        &polyline(&[pt(6.0, 9.0), pt(3.0, 12.0), pt(6.0, 15.0)], false),
        true,
        STROKE,
    );
    ink.stroke(
        &polyline(&[pt(18.0, 9.0), pt(21.0, 12.0), pt(18.0, 15.0)], false),
        true,
        STROKE,
    );
}

fn paint_copy(ink: &mut ToolInk<'_>) {
    ink.stroke(
        &polyline(
            &[
                pt(8.5, 15.0),
                pt(4.0, 15.0),
                pt(4.0, 4.0),
                pt(15.0, 4.0),
                pt(15.0, 8.5),
            ],
            false,
        ),
        false,
        STROKE,
    );
    ink.stroke(&rectangle(9.0, 9.0, 11.0, 11.0), true, STROKE);
}

/// A square given a small turn, inside the curved arrow that turned it; the arrow leaves a gap at
/// the right, where its head is. The square is kept small so that at 16 px the arrow, not the
/// square, is what reads.
fn paint_rotate(ink: &mut ToolInk<'_>) {
    let mut square = Path::new();
    let half = 3.6;
    let turn = 25.0 * PI / 180.0;
    for corner in 0..4_i32 {
        let angle = turn + f64::from(corner) * PI / 2.0 + PI / 4.0;
        let p = pt(
            12.0 + half * 2.0_f64.sqrt() * angle.cos(),
            12.0 - half * 2.0_f64.sqrt() * angle.sin(),
        );
        if corner == 0 {
            square.move_to(p);
        } else {
            square.line_to(p);
        }
    }
    square.close_subpath();
    ink.stroke(&square, false, STROKE);
    // From 60 degrees, counter-clockwise round to 330: the head is at 330, pointing along the
    // turn (330 + 90 = 60 degrees).
    const RADIUS: f64 = 9.2;
    ink.stroke(&arc(12.0, 12.0, RADIUS, 60.0, 270.0), true, STROKE);
    let end = 330.0 * PI / 180.0;
    arrow_head(
        ink,
        12.0 + RADIUS * end.cos(),
        12.0 - RADIUS * end.sin(),
        60.0,
        3.2,
    );
}

/// A small square grown from its corner into a large one along the arrow.
fn paint_scale(ink: &mut ToolInk<'_>) {
    ink.stroke(&rectangle(4.0, 4.0, 16.0, 16.0), true, STROKE);
    ink.stroke(&rectangle(4.0, 13.0, 7.0, 7.0), false, STROKE);
    ink.line(11.0, 13.0, 17.0, 7.0, true, STROKE);
    arrow_head(ink, 17.5, 6.5, 45.0, 3.0);
    ink.node(4.0, 20.0, false);
}

/// A flag and its mirror image either side of the dashed mirror line.
fn paint_mirror(ink: &mut ToolInk<'_>) {
    ink.stroke(
        &polyline(&[pt(3.5, 19.0), pt(9.5, 19.0), pt(9.5, 6.0)], true),
        false,
        STROKE,
    );
    ink.stroke(
        &polyline(&[pt(20.5, 19.0), pt(14.5, 19.0), pt(14.5, 6.0)], true),
        true,
        STROKE,
    );
    ink.dashed(&polyline(&[pt(12.0, 2.5), pt(12.0, 21.5)], false), false);
}

/// A rectangle whose right-hand side has been pulled out: the moved edge and its corners in the
/// accent, the arrow showing which way.
fn paint_stretch(ink: &mut ToolInk<'_>) {
    ink.stroke(
        &polyline(
            &[pt(20.0, 7.0), pt(3.0, 7.0), pt(3.0, 17.0), pt(20.0, 17.0)],
            false,
        ),
        false,
        STROKE,
    );
    ink.line(20.0, 7.0, 20.0, 17.0, true, STROKE);
    ink.node(20.0, 7.0, true);
    ink.node(20.0, 17.0, true);
    ink.line(8.0, 12.0, 15.5, 12.0, true, STROKE);
    arrow_head(ink, 16.0, 12.0, 0.0, 2.8);
}

/// Three rows of three: the original, bottom left, and its eight copies.
fn paint_array_rectangular(ink: &mut ToolInk<'_>) {
    for row in 0..3_i32 {
        for column in 0..3_i32 {
            let original = row == 2 && column == 0;
            ink.fill(
                &rectangle(
                    3.5 + 6.5 * f64::from(column),
                    3.5 + 6.5 * f64::from(row),
                    4.5,
                    4.5,
                ),
                !original,
                255,
            );
        }
    }
}

/// Six items round a centre: the original, due east, and five copies. No circle joins them:
/// drawn dashed between the dots it filled the ring in at 16 px and the items were lost in it.
fn paint_array_polar(ink: &mut ToolInk<'_>) {
    for item in 0..6_i32 {
        let angle = f64::from(item) * PI / 3.0;
        ink.dot(
            12.0 + 8.0 * angle.cos(),
            12.0 - 8.0 * angle.sin(),
            2.4,
            item != 0,
        );
    }
    ink.node(12.0, 12.0, false);
}

fn paint_erase(ink: &mut ToolInk<'_>) {
    ink.line(4.0, 7.0, 20.0, 7.0, false, STROKE);
    ink.stroke(
        &polyline(
            &[pt(9.0, 7.0), pt(9.0, 4.5), pt(15.0, 4.5), pt(15.0, 7.0)],
            false,
        ),
        false,
        STROKE,
    );
    ink.stroke(
        &polyline(
            &[pt(6.0, 7.0), pt(7.0, 21.0), pt(17.0, 21.0), pt(18.0, 7.0)],
            false,
        ),
        false,
        STROKE,
    );
    ink.line(10.0, 11.0, 10.0, 17.0, true, STROKE);
    ink.line(14.0, 11.0, 14.0, 17.0, true, STROKE);
}

pub fn paint_modify_transform_icon(tool_id: &str, ink: &mut ToolInk<'_>) -> bool {
    match tool_id {
        "modify.move" => paint_move(ink),
        "modify.copy" => paint_copy(ink),
        "modify.rotate" => paint_rotate(ink),
        "modify.scale" => paint_scale(ink),
        "modify.mirror" => paint_mirror(ink),
        "modify.stretch" => paint_stretch(ink),
        "modify.array_rectangular" => paint_array_rectangular(ink),
        "modify.array_polar" => paint_array_polar(ink),
        "modify.erase" => paint_erase(ink),
        _ => return false,
    }
    true
}
