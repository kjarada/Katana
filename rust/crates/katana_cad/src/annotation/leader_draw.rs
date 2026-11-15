//! A leader and its callout, drawn (include/katana/cad/annotation/leader_draw.hpp;
//! docs/annotation.md, "Leaders and callouts").
//!
//! The line runs through the leader's vertices from the arrow at its tip;
//! from the last vertex a horizontal LANDING of `landing` paper millimetres
//! runs away from the line (to the right when the last stretch heads right or
//! straight up or down, to the left when it heads left), and the note is set
//! beyond it, its middle on the landing's line: left-justified off a
//! right-going landing, right-justified off a left-going one, as AutoCAD's
//! MLEADER sets it. A Box callout frames the note with a margin of a third of
//! its height; a Circle callout (a numbered balloon) centres the note in a
//! circle that clears it by the same margin, and the landing ends on the
//! circle. Every size is paper millimetres at 1 : scale.
//!
//! The note is entity's leader_note: a plain leader's text, or a smart
//! leader's template filled from the entity its tip is on as the model is now
//! (docs/annotation.md, "Smart leaders"), with the survey code looked for
//! where survey coding looks (code_property_candidates) and the drawing's
//! surfaces and alignments read through `context` (leader_presets.rs,
//! leader_context). A smart leader with nothing to say is drawn as a bare
//! leader.

use std::sync::OnceLock;

use katana_entity::annotation::{DEFAULT_TEXT_STYLE_NAME, annotation_model_size};
use katana_entity::leader_values::leader_note;
use katana_entity::{ArrowHead, CalloutShape, LeaderContext, LeaderGeometry, Model, TextJustify};
use katana_geometry::{Box2, Point2, Vec2};
use katana_math::cpp;
use katana_math::numerics::tolerance;

use crate::annotation::drawing::{DEFAULT_CIRCLE_SEGMENTS, Drawing, TextMeasure, circle_polygon};
use crate::annotation::text_layout::{
    Readable, layout_text, readable_rotation, resolve_text_appearance,
};
use crate::survey_coding::code_property_candidates;

// A closed head is three times as long as it is wide - dimension_draw's
// proportion (ARROW_WIDTH_FRACTION there), so a leader's arrow and a
// dimension's match.
const ARROW_WIDTH_FRACTION: f64 = 1.0 / 3.0;
// A callout's margin around its note, as a fraction of the text height.
const CALLOUT_MARGIN: f64 = 1.0 / 3.0;
// The gap between the landing's end and the note, as a fraction of the text
// height: AutoCAD's MLEADER default landing gap is 2 units against 4 of text.
const LANDING_GAP: f64 = 0.5;

/// code_property_candidates() as the entity layer takes them, made once: a
/// leader's note and a label's pieces are made on every paint.
pub(crate) fn code_properties() -> &'static [String] {
    static CODES: OnceLock<Vec<String>> = OnceLock::new();
    CODES.get_or_init(|| {
        code_property_candidates()
            .iter()
            .map(|&code| String::from(code))
            .collect()
    })
}

/// The arrowhead `head` of `size` model units at `tip` pointing along `along`
/// (a unit vector, away from the line): the shapes dimension_draw draws, as
/// strokes and fills. Exposed for the tests.
pub fn append_arrowhead(
    drawing: &mut Drawing,
    head: ArrowHead,
    tip: Point2,
    along: Vec2,
    size: f64,
) {
    if !(size > tolerance::GEOMETRIC) {
        return;
    }
    let normal = along.perpendicular();
    let back = tip - along * size;
    let half = normal * (size * ARROW_WIDTH_FRACTION);
    match head {
        ArrowHead::None => {}
        ArrowHead::Tick => {
            let direction = (along + normal).normalized();
            drawing.strokes.push(vec![
                tip - direction * (size * 0.5),
                tip + direction * (size * 0.5),
            ]);
        }
        ArrowHead::Open => drawing.strokes.push(vec![back + half, tip, back - half]),
        ArrowHead::ClosedFilled => drawing.fills.push(vec![tip, back + half, back - half]),
        ArrowHead::Dot => drawing.fills.push(circle_polygon(tip, size * 0.25, 16)),
    }
}

/// The leader drawn at 1 : `scale` (the C++ default context is
/// `&LeaderContext::default()`: no surface or alignment is read).
pub fn build_leader(
    model: &Model,
    leader: &LeaderGeometry,
    scale: f64,
    measure: &TextMeasure,
    context: &LeaderContext,
) -> Drawing {
    let mut drawing = Drawing::default();
    let v = &leader.vertices;
    if v.len() < 2 {
        return drawing;
    }
    let style_name = if leader.style.is_empty() {
        DEFAULT_TEXT_STYLE_NAME
    } else {
        leader.style.as_str()
    };
    let mut appearance = resolve_text_appearance(
        model,
        style_name,
        leader.paper_height,
        annotation_model_size(2.5, scale),
        scale,
    );
    appearance.readable = false;
    drawing.colour = appearance.colour;
    let height = appearance.height;

    // The line, from the arrow's back when the head is closed so the line's
    // end does not show through its point.
    let mut line: Vec<Point2> = v.clone();
    let first = v[1] - v[0];
    let first_length = first.length();
    let arrow = annotation_model_size(leader.arrow_size, scale);
    if first_length > tolerance::GEOMETRIC {
        let along = -first / first_length;
        append_arrowhead(
            &mut drawing,
            leader.arrow,
            v[0],
            along,
            cpp::min(arrow, 0.5 * first_length),
        );
    }

    // The landing's direction: away from the line's last stretch.
    let last = v[v.len() - 1] - v[v.len() - 2];
    let sign = if last.x < -tolerance::GEOMETRIC * cpp::max(1.0, last.length()) {
        -1.0
    } else {
        1.0
    };
    let landing_direction = match leader.landing_angle {
        Some(angle) => Vec2::new(angle.cos(), angle.sin()),
        None => Vec2::new(sign, 0.0),
    };
    let text_direction = match leader.landing_angle {
        Some(angle) => readable_rotation(angle, TextJustify::MiddleLeft),
        None => Readable {
            rotation: 0.0,
            justify: if sign > 0.0 {
                TextJustify::MiddleLeft
            } else {
                TextJustify::MiddleRight
            },
            turned: false,
        },
    };
    let landing = annotation_model_size(leader.landing, scale);
    let mut end = v[v.len() - 1];
    if landing > 0.0 {
        end += landing_direction * landing;
        line.push(end);
    }

    // A smart leader's note is read off its target now (docs/annotation.md,
    // "Smart leaders"); one with nothing to say is drawn bare.
    let note = leader_note(model, leader, code_properties(), context);
    if !note.is_empty() {
        let gap = LANDING_GAP * height;
        let margin = CALLOUT_MARGIN * height;
        if leader.callout == CalloutShape::Circle {
            // Measure first, then centre the note in the circle that clears it.
            let probe = layout_text(
                &note,
                Point2::new(0.0, 0.0),
                text_direction.rotation,
                TextJustify::MiddleCentre,
                &appearance,
                measure,
            );
            let mut radius = 0.0;
            for text_box in &probe.text_boxes {
                for corner in text_box {
                    radius = cpp::max(radius, Vec2::new(corner.x, corner.y).length());
                }
            }
            radius += margin;
            let centre = end + landing_direction * radius;
            let mut note_drawing = layout_text(
                &note,
                centre,
                text_direction.rotation,
                TextJustify::MiddleCentre,
                &appearance,
                measure,
            );
            note_drawing
                .masks
                .push(circle_polygon(centre, radius, DEFAULT_CIRCLE_SEGMENTS));
            note_drawing
                .outlines
                .push(circle_polygon(centre, radius, DEFAULT_CIRCLE_SEGMENTS));
            drawing.append(&note_drawing);
        } else {
            let reach = if leader.callout == CalloutShape::Box {
                gap + margin
            } else {
                gap
            };
            let at = end + landing_direction * reach;
            let mut note_drawing = layout_text(
                &note,
                at,
                text_direction.rotation,
                text_direction.justify,
                &appearance,
                measure,
            );
            if leader.callout == CalloutShape::Box && !note_drawing.text_boxes.is_empty() {
                let along = Vec2::new(text_direction.rotation.cos(), text_direction.rotation.sin());
                let up = along.perpendicular();
                let mut local = Box2::default();
                for &p in &note_drawing.text_boxes[0] {
                    let delta = p - at;
                    local.expand(Point2::new(delta.dot(along), delta.dot(up)));
                }
                let framed = local.inflated(margin);
                let corner = |x: f64, y: f64| at + along * x + up * y;
                let frame = vec![
                    corner(framed.min.x, framed.min.y),
                    corner(framed.max.x, framed.min.y),
                    corner(framed.max.x, framed.max.y),
                    corner(framed.min.x, framed.max.y),
                ];
                note_drawing.masks.push(frame.clone());
                note_drawing.outlines.push(frame);
            }
            drawing.append(&note_drawing);
        }
    }
    drawing.strokes.insert(0, line);
    drawing.update_extent();
    drawing
}
