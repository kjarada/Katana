//! Icons of the Circle and Arc tools in their construction variants (see tool_icons).
//!
//! As in icons, the curve the tool draws is in the accent and what the user picks to construct
//! it - a centre, a point on it, the lines it touches - is neutral. The variants of one curve
//! share its shape, so they read as one family in the menu, and differ only in the picks: at
//! 16 px a centre node, a diameter or three points on the rim is what tells them apart.

use crate::canvas::PointF;

use super::tool_icons::{STROKE, ToolInk, arc, circle, polyline, pt, rectangle};

/// The point at `degrees` (counter-clockwise on paper) on the circle about (cx, cy): the
/// painter's y points down, so sine is subtracted.
fn on_circle(cx: f64, cy: f64, radius: f64, degrees: f64) -> PointF {
    // The C++ writes pi as 3.14159265358979323846, which is this double.
    let radians = degrees * std::f64::consts::PI / 180.0;
    pt(cx + radius * radians.cos(), cy - radius * radians.sin())
}

/// A filled arrowhead with its tip at `tip`, pointing from `from`: the head of a radius
/// dimension, as the drawing's own dimensions draw one.
fn arrowhead(ink: &mut ToolInk<'_>, from: PointF, tip: PointF) {
    let along = tip - from;
    let length = along.x.hypot(along.y);
    // QPointF / qreal divides each coordinate.
    let ahead = pt(along.x / length, along.y / length);
    let side = pt(-ahead.y, ahead.x);
    ink.fill(
        &polyline(
            &[
                tip,
                tip - ahead * 3.6 + side * 1.6,
                tip - ahead * 3.6 - side * 1.6,
            ],
            true,
        ),
        false,
        255,
    );
}

/// A picked point ON the curve: a node a size larger than ToolInk::node, which at 16 px is two
/// pixels and vanishes into the curve's own stroke.
fn pick(ink: &mut ToolInk<'_>, x: f64, y: f64) {
    ink.fill(&rectangle(x - 2.1, y - 2.1, 4.2, 4.2), false, 255);
}

pub fn paint_draw_curve_icon(tool_id: &str, ink: &mut ToolInk<'_>) -> bool {
    if tool_id == "draw.circle" {
        // The application's Circle: a radius at 45 degrees from a centre node.
        ink.stroke(&circle(12.0, 12.0, 8.0), true, STROKE);
        ink.line(12.0, 12.0, 17.66, 6.34, false, 1.1);
        ink.node(12.0, 12.0, false);
        ink.node(17.66, 6.34, false);
        return true;
    }
    if tool_id == "draw.circle.diameter" {
        // The radius carried through the centre to the far side.
        ink.stroke(&circle(12.0, 12.0, 8.0), true, STROKE);
        ink.line(6.34, 17.66, 17.66, 6.34, false, 1.1);
        ink.node(12.0, 12.0, false);
        ink.node(6.34, 17.66, false);
        ink.node(17.66, 6.34, false);
        return true;
    }
    if tool_id == "draw.circle.2p" {
        // Two picks on the rim, opposite; no centre, which is never picked.
        ink.stroke(&circle(12.0, 12.0, 8.0), true, STROKE);
        pick(ink, 4.0, 12.0);
        pick(ink, 20.0, 12.0);
        return true;
    }
    if tool_id == "draw.circle.3p" {
        ink.stroke(&circle(12.0, 12.0, 8.0), true, STROKE);
        for degrees in [90.0, 210.0, 330.0] {
            let p = on_circle(12.0, 12.0, 8.0, degrees);
            pick(ink, p.x, p.y);
        }
        return true;
    }
    if tool_id == "draw.circle.ttr" {
        // Two lines meeting in a corner, and the circle sitting in it touching both; the touch
        // points are the picks.
        ink.line(4.0, 3.0, 4.0, 21.0, false, STROKE);
        ink.line(3.0, 20.0, 21.0, 20.0, false, STROKE);
        ink.stroke(&circle(11.5, 12.5, 7.5), true, STROKE);
        pick(ink, 4.0, 12.5);
        pick(ink, 11.5, 20.0);
        return true;
    }
    if tool_id == "draw.arc" {
        // The application's Arc as a true arc through the same three nodes, (4, 18), (12, 8.5)
        // and (20, 18): chord 16, rise 9.5, so its radius is (8^2 + 9.5^2) / (2 * 9.5) = 8.12
        // about (12, 16.62), and it runs from 189.8 degrees clockwise on paper to -9.8.
        ink.stroke(&arc(12.0, 16.62, 8.12, 189.8, -199.6), true, STROKE);
        pick(ink, 4.0, 18.0);
        pick(ink, 12.0, 8.5);
        pick(ink, 20.0, 18.0);
        return true;
    }
    if tool_id == "draw.arc.sce" || tool_id == "draw.arc.cse" {
        // A quarter arc about a picked centre, with its radii: the start on the right, the end
        // above, counter-clockwise between them. The first pick of each is the stronger mark -
        // the start as a dot for start-centre-end, the centre as a cross for centre-start-end -
        // since the two differ only in the order of the picks.
        let centre_first = tool_id == "draw.arc.cse";
        ink.stroke(&arc(6.0, 18.0, 13.0, 0.0, 90.0), true, STROKE);
        ink.line(6.0, 18.0, 19.0, 18.0, false, 1.1);
        ink.line(6.0, 18.0, 6.0, 5.0, false, 1.1);
        ink.node(6.0, 5.0, false);
        if centre_first {
            ink.line(6.0, 14.2, 6.0, 21.8, false, 1.7);
            ink.line(2.2, 18.0, 9.8, 18.0, false, 1.7);
            ink.node(19.0, 18.0, false);
        } else {
            ink.node(6.0, 18.0, false);
            ink.dot(19.0, 18.0, 2.5, false);
        }
        return true;
    }
    if tool_id == "draw.arc.ser" {
        // The two ends, picked, and a radius dimension from the centre to the arc: the value
        // typed is the radius. The centre is not marked - it is never picked, and a dot there
        // made the icon read as a dial. The arc is about (12, 20), radius 10, from 30 to 150
        // degrees; the ends are at y = 20 - 5.
        ink.stroke(&arc(12.0, 20.0, 10.0, 30.0, 120.0), true, STROKE);
        pick(ink, 20.66, 15.0);
        pick(ink, 3.34, 15.0);
        let on_arc = on_circle(12.0, 20.0, 10.0, 65.0);
        ink.line(12.0, 20.0, on_arc.x, on_arc.y, false, 1.1);
        arrowhead(ink, pt(12.0, 20.0), on_arc);
        return true;
    }
    false
}
