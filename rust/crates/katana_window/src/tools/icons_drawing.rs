//! Icons of the drawing system's tools (see tool_icons): Draw > Vertices, and the draw tools the
//! drawing system added.
//!
//! The vertex tools share one picture - a three-vertex polyline in neutral with its vertices as
//! nodes - and each marks in the accent what it does to it: the vertex it adds, removes or moves,
//! the run it straightens, the heights it sets. At 16 px the family reads as one and the accent
//! tells the tools apart.

use crate::canvas::{Path, RectF};

use super::tool_icons::{STROKE, ToolInk, arc, polyline, pt, rectangle};

/// The family's polyline: (3,18) (10,7) (21,14), vertices as nodes.
fn base_polyline(ink: &mut ToolInk<'_>, nodes: bool) {
    ink.stroke(
        &polyline(&[pt(3.0, 18.0), pt(10.0, 7.0), pt(21.0, 14.0)], false),
        false,
        STROKE,
    );
    if nodes {
        ink.node(3.0, 18.0, false);
        ink.node(10.0, 7.0, false);
        ink.node(21.0, 14.0, false);
    }
}

fn plus(ink: &mut ToolInk<'_>, x: f64, y: f64) {
    ink.line(x - 2.5, y, x + 2.5, y, true, 1.4);
    ink.line(x, y - 2.5, x, y + 2.5, true, 1.4);
}

fn cross(ink: &mut ToolInk<'_>, x: f64, y: f64) {
    ink.line(x - 2.2, y - 2.2, x + 2.2, y + 2.2, true, 1.4);
    ink.line(x - 2.2, y + 2.2, x + 2.2, y - 2.2, true, 1.4);
}

pub fn paint_drawing_icon(tool_id: &str, ink: &mut ToolInk<'_>) -> bool {
    if tool_id == "draw.vertex.insert" {
        base_polyline(ink, true);
        ink.node(15.5, 10.5, true);
        plus(ink, 18.0, 4.5);
        return true;
    }
    if tool_id == "draw.vertex.delete" {
        ink.stroke(
            &polyline(&[pt(3.0, 18.0), pt(21.0, 14.0)], false),
            false,
            STROKE,
        );
        ink.dashed(
            &polyline(&[pt(3.0, 18.0), pt(10.0, 7.0), pt(21.0, 14.0)], false),
            false,
        );
        ink.node(3.0, 18.0, false);
        ink.node(21.0, 14.0, false);
        cross(ink, 10.0, 7.0);
        return true;
    }
    if tool_id == "draw.vertex.move" {
        ink.dashed(
            &polyline(&[pt(3.0, 18.0), pt(10.0, 7.0), pt(21.0, 14.0)], false),
            false,
        );
        ink.stroke(
            &polyline(&[pt(3.0, 18.0), pt(14.0, 4.0), pt(21.0, 14.0)], false),
            true,
            STROKE,
        );
        ink.node(3.0, 18.0, false);
        ink.node(21.0, 14.0, false);
        ink.node(14.0, 4.0, true);
        return true;
    }
    if tool_id == "draw.vertex.edit" {
        base_polyline(ink, true);
        // A table beside it: the Vertices panel.
        ink.stroke(&rectangle(13.0, 16.0, 9.0, 6.0), true, 1.1);
        ink.line(13.0, 19.0, 22.0, 19.0, true, 1.0);
        ink.line(16.5, 16.0, 16.5, 22.0, true, 1.0);
        return true;
    }
    if tool_id == "draw.vertex.straighten" {
        ink.dashed(
            &polyline(
                &[pt(3.0, 16.0), pt(8.0, 8.0), pt(13.0, 17.0), pt(18.0, 9.0)],
                false,
            ),
            false,
        );
        ink.line(3.0, 16.0, 21.0, 12.0, true, STROKE);
        ink.node(3.0, 16.0, false);
        ink.node(21.0, 12.0, false);
        return true;
    }
    if tool_id == "draw.vertex.weed" {
        ink.dashed(
            &polyline(
                &[
                    pt(3.0, 14.0),
                    pt(6.0, 12.5),
                    pt(9.0, 13.5),
                    pt(12.0, 12.0),
                    pt(15.0, 13.0),
                    pt(18.0, 11.5),
                    pt(21.0, 12.0),
                ],
                false,
            ),
            false,
        );
        ink.stroke(
            &polyline(&[pt(3.0, 14.0), pt(21.0, 12.0)], false),
            true,
            STROKE,
        );
        ink.node(3.0, 14.0, false);
        ink.node(21.0, 12.0, false);
        return true;
    }
    if tool_id == "draw.vertex.densify" {
        ink.stroke(
            &polyline(&[pt(3.0, 16.0), pt(21.0, 8.0)], false),
            false,
            STROKE,
        );
        for k in 1..5_i32 {
            ink.node(
                3.0 + 18.0 * f64::from(k) / 5.0,
                16.0 - 8.0 * f64::from(k) / 5.0,
                true,
            );
        }
        ink.node(3.0, 16.0, false);
        ink.node(21.0, 8.0, false);
        return true;
    }
    if tool_id == "draw.vertex.close" {
        base_polyline(ink, true);
        ink.line(21.0, 14.0, 3.0, 18.0, true, STROKE);
        return true;
    }
    if tool_id == "draw.vertex.start" {
        ink.stroke(
            &polyline(
                &[pt(4.0, 18.0), pt(10.0, 5.0), pt(20.0, 8.0), pt(19.0, 19.0)],
                true,
            ),
            false,
            STROKE,
        );
        ink.node(10.0, 5.0, false);
        ink.node(20.0, 8.0, false);
        ink.node(19.0, 19.0, false);
        ink.dot(4.0, 18.0, 2.6, true);
        return true;
    }
    if tool_id == "draw.vertex.height" {
        base_polyline(ink, true);
        ink.line(10.0, 7.0, 10.0, 21.0, true, 1.1);
        ink.label(15.5, 20.0, 7.5, "Z", true);
        return true;
    }
    if tool_id == "draw.vertex.interpolate" {
        ink.stroke(
            &polyline(&[pt(3.0, 19.0), pt(21.0, 7.0)], false),
            false,
            STROKE,
        );
        ink.node(3.0, 19.0, false);
        ink.node(21.0, 7.0, false);
        ink.dot(9.0, 15.0, 1.9, true);
        ink.dot(15.0, 11.0, 1.9, true);
        return true;
    }
    if tool_id == "draw.vertex.grade" {
        ink.stroke(
            &polyline(&[pt(3.0, 19.0), pt(21.0, 7.0)], false),
            true,
            STROKE,
        );
        ink.node(3.0, 19.0, false);
        ink.node(21.0, 7.0, false);
        ink.line(3.0, 19.0, 21.0, 19.0, false, 1.0);
        ink.line(21.0, 19.0, 21.0, 7.0, false, 1.0);
        return true;
    }
    if tool_id == "draw.vertex.arc" {
        ink.dashed(&polyline(&[pt(4.0, 16.0), pt(20.0, 16.0)], false), false);
        ink.stroke(&arc(12.0, 16.0, 8.0, 0.0, 180.0), true, STROKE);
        ink.node(4.0, 16.0, false);
        ink.node(20.0, 16.0, false);
        return true;
    }
    if tool_id == "draw.vertex.line" {
        ink.dashed(&arc(12.0, 16.0, 8.0, 0.0, 180.0), false);
        ink.line(4.0, 16.0, 20.0, 16.0, true, STROKE);
        ink.node(4.0, 16.0, false);
        ink.node(20.0, 16.0, false);
        return true;
    }
    if tool_id == "draw.vertex.fillet" {
        ink.stroke(
            &polyline(&[pt(3.0, 20.0), pt(3.0, 11.0)], false),
            false,
            STROKE,
        );
        ink.stroke(
            &polyline(&[pt(12.0, 4.0), pt(21.0, 4.0)], false),
            false,
            STROKE,
        );
        ink.stroke(&arc(12.0, 11.0, 9.0, 90.0, 90.0), true, STROKE);
        ink.dashed(
            &polyline(&[pt(3.0, 11.0), pt(3.0, 4.0), pt(12.0, 4.0)], false),
            false,
        );
        return true;
    }
    if tool_id == "draw.vertex.chamfer" {
        ink.stroke(
            &polyline(&[pt(3.0, 20.0), pt(3.0, 10.0)], false),
            false,
            STROKE,
        );
        ink.stroke(
            &polyline(&[pt(10.0, 4.0), pt(21.0, 4.0)], false),
            false,
            STROKE,
        );
        ink.line(3.0, 10.0, 10.0, 4.0, true, STROKE);
        ink.dashed(
            &polyline(&[pt(3.0, 10.0), pt(3.0, 4.0), pt(10.0, 4.0)], false),
            false,
        );
        return true;
    }
    if tool_id == "draw.vertex.merge" {
        ink.stroke(
            &polyline(&[pt(3.0, 18.0), pt(11.0, 9.0), pt(21.0, 14.0)], false),
            false,
            STROKE,
        );
        ink.node(3.0, 18.0, false);
        ink.node(21.0, 14.0, false);
        ink.dot(11.0, 9.0, 2.8, true);
        ink.node(9.5, 10.5, false);
        ink.node(12.5, 8.5, false);
        return true;
    }
    if tool_id == "draw.vertex.grid" {
        for x in [4.0, 12.0, 20.0] {
            for y in [4.0, 12.0, 20.0] {
                ink.dot(x, y, 0.8, false);
            }
        }
        ink.stroke(
            &polyline(&[pt(4.0, 20.0), pt(12.0, 4.0), pt(20.0, 12.0)], false),
            true,
            STROKE,
        );
        ink.node(4.0, 20.0, true);
        ink.node(12.0, 4.0, true);
        ink.node(20.0, 12.0, true);
        return true;
    }
    // ---- the draw tools ----
    if tool_id == "draw.polyline3d" {
        ink.stroke(
            &polyline(&[pt(3.0, 19.0), pt(10.0, 11.0), pt(21.0, 14.0)], false),
            true,
            STROKE,
        );
        for (x, y) in [(3.0, 19.0), (10.0, 11.0), (21.0, 14.0)] {
            ink.line(x, y, x, y - 7.0, false, 1.0);
            ink.node(x, y, false);
        }
        return true;
    }
    if tool_id == "draw.spline" {
        let mut path = Path::new();
        path.move_to(pt(3.0, 17.0));
        path.cubic_to(pt(8.0, 2.0), pt(14.0, 24.0), pt(21.0, 7.0));
        ink.stroke(&path, true, STROKE);
        ink.node(3.0, 17.0, false);
        ink.node(12.0, 13.0, false);
        ink.node(21.0, 7.0, false);
        return true;
    }
    if tool_id == "draw.ellipse"
        || tool_id == "draw.ellipse.centre"
        || tool_id == "draw.ellipse.arc"
    {
        let mut path = Path::new();
        let bounds = RectF::new(2.0, 6.0, 20.0, 12.0);
        if tool_id == "draw.ellipse.arc" {
            path.arc_move_to(bounds, 0.0);
            path.arc_to(bounds, 0.0, 200.0);
            ink.dashed(&polyline(&[pt(12.0, 12.0), pt(22.0, 12.0)], false), false);
        } else {
            path.add_ellipse(bounds);
        }
        ink.stroke(&path, true, STROKE);
        if tool_id == "draw.ellipse.centre" {
            ink.node(12.0, 12.0, false);
            ink.node(22.0, 12.0, false);
        } else {
            ink.node(2.0, 12.0, false);
            ink.node(22.0, 12.0, false);
        }
        return true;
    }
    if tool_id == "draw.xline" {
        ink.line(1.0, 20.0, 23.0, 4.0, true, STROKE);
        ink.node(9.5, 13.8, false);
        return true;
    }
    if tool_id == "draw.ray" {
        ink.line(5.0, 18.0, 23.0, 5.0, true, STROKE);
        ink.node(5.0, 18.0, false);
        return true;
    }
    if tool_id == "draw.dline" {
        ink.stroke(
            &polyline(&[pt(3.0, 16.0), pt(12.0, 7.0), pt(21.0, 12.0)], false),
            true,
            STROKE,
        );
        ink.stroke(
            &polyline(&[pt(3.0, 20.0), pt(12.0, 11.0), pt(21.0, 16.0)], false),
            true,
            STROKE,
        );
        ink.dashed(
            &polyline(&[pt(3.0, 18.0), pt(12.0, 9.0), pt(21.0, 14.0)], false),
            false,
        );
        return true;
    }
    if tool_id == "draw.sketch" {
        let mut path = Path::new();
        path.move_to(pt(3.0, 15.0));
        path.cubic_to(pt(6.0, 6.0), pt(8.0, 20.0), pt(11.0, 12.0));
        path.cubic_to(pt(13.0, 6.0), pt(16.0, 18.0), pt(21.0, 9.0));
        ink.stroke(&path, true, STROKE);
        return true;
    }
    if tool_id == "draw.revcloud" {
        let mut path = Path::new();
        path.move_to(pt(4.0, 8.0));
        for (x, y) in [
            (10.0, 5.0),
            (16.0, 5.0),
            (21.0, 9.0),
            (21.0, 15.0),
            (16.0, 19.0),
            (10.0, 19.0),
            (4.0, 15.0),
            (4.0, 8.0),
        ] {
            let from = path.current_position();
            let to = pt(x, y);
            // (from + to) / 2.0: QPointF divided coordinate by coordinate.
            let sum = from + to;
            let mid = pt(sum.x / 2.0, sum.y / 2.0);
            let out = pt(-(to.y - from.y) * 0.35, (to.x - from.x) * 0.35);
            path.quad_to(mid - out, to);
        }
        ink.stroke(&path, true, STROKE);
        return true;
    }
    if tool_id == "draw.circle.ttt" {
        // Three tangent lines round the circle they touch.
        let mut circle = Path::new();
        circle.add_ellipse(RectF::new(7.0, 8.0, 10.0, 10.0));
        ink.stroke(&circle, true, STROKE);
        ink.line(2.0, 18.0, 22.0, 18.0, false, 1.0);
        ink.line(4.0, 5.0, 13.0, 21.0, false, 1.0);
        ink.line(20.0, 5.0, 11.0, 21.0, false, 1.0);
        return true;
    }
    if tool_id == "draw.arc.sed" {
        // An arc between its ends, with the direction it leaves its start.
        let mut path = Path::new();
        path.move_to(pt(4.0, 19.0));
        path.quad_to(pt(4.0, 5.0), pt(20.0, 7.0));
        ink.stroke(&path, true, STROKE);
        ink.dashed(&polyline(&[pt(4.0, 19.0), pt(4.0, 5.0)], false), false);
        ink.node(4.0, 19.0, false);
        ink.node(20.0, 7.0, false);
        return true;
    }
    false
}
