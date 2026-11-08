//! Annotation drawn out as plain shapes for a file that has no such entities
//! (include/katana/cad/annotation/export_annotation.hpp; docs/annotation.md,
//! "Exchange").
//!
//! A DXF file has no label - a label's words and place are worked out for a
//! view, by the placer - and the DXF writer draws only an aligned dimension
//! and a bare leader itself. The file can still carry what the sheet shows:
//! the lines, the arrowheads, the callout frames and the text, at the scale
//! the drawing is annotated at. That drawing is cad's (the placer, the
//! dimension and leader builders), which the DXF crate may not see (the
//! layering rule), so a front end draws it here and hands it to the writer
//! (dxf's ExportOptions::drawn): one drawing of an annotation, whether it goes
//! to the screen, a sheet or a file.
//!
//! The shapes are the plain kinds every format has: a stroke is a Segment2 or
//! an open Polyline2, a filled arrowhead, a dot and a callout frame a CLOSED
//! Polyline2 (the model cannot fill one; the outline is the shape), and each
//! line of text a single-line, bottom-left TextGeometry at its model height.
//! A mask is not drawn: a file's reader has no ground to paint it in.

use std::collections::BTreeMap;

use katana_entity::{DimensionKind, EntityId, Geometry, LeaderContext, Model, TextGeometry};
use katana_geometry::{Point2, Polyline2, Segment2};
use katana_math::numerics::tolerance;

use crate::annotation::drawing::{Drawing, TextMeasure};
use crate::annotation::label_layout::{
    LabelLayoutOptions, label_entities, label_keep_out, layout_labels,
};
use crate::annotation::leader_draw::build_leader;
use crate::dimension_draw::{DimensionDrawing, build_dimension, resolve_dimension_style};

// As much of the keep-out as the plan view gathers (the plan painter's
// MAXIMUM_LINEWORK), so a file is judged as the screen is.
const KEEP_OUT_LIMIT: usize = 200_000;

/// By annotation entity: the shapes it is drawn as. An entry with no shapes
/// is an annotation that draws nothing at this scale - a label the placer
/// found no room for, or one whose target is gone.
pub type DrawnAnnotation = BTreeMap<EntityId, Vec<Geometry>>;

// A path as one shape: a segment for two points, a polyline for more, and
// nothing for one that has no length.
fn append_path(path: &[Point2], closed: bool, out: &mut Vec<Geometry>) {
    if path.len() < 2 {
        return;
    }
    if path.len() == 2 && !closed {
        if path[0].distance_to(path[1]) > tolerance::GEOMETRIC {
            out.push(Geometry::Line(Segment2::new(path[0], path[1])));
        }
        return;
    }
    let polyline = Polyline2::new(path.to_vec(), closed && path.len() > 2);
    if polyline.length() > tolerance::GEOMETRIC {
        out.push(Geometry::Polyline(polyline));
    }
}

fn append_text(text: &str, at: Point2, height: f64, rotation: f64, out: &mut Vec<Geometry>) {
    if text.is_empty() || !(height > 0.0) {
        return;
    }
    out.push(Geometry::Text(TextGeometry {
        position: at,
        text: String::from(text),
        height,
        rotation,
        ..TextGeometry::default()
    }));
}

/// A drawing's pieces as plain shapes (above). (The C++ `plainShapes`
/// overload of a Drawing; plain_dimension_shapes is the other.)
pub fn plain_shapes(drawing: &Drawing) -> Vec<Geometry> {
    let mut out = Vec::new();
    for stroke in &drawing.strokes {
        append_path(stroke, false, &mut out);
    }
    for outline in &drawing.outlines {
        append_path(outline, true, &mut out);
    }
    for fill in &drawing.fills {
        append_path(fill, true, &mut out);
    }
    for run in &drawing.texts {
        append_text(&run.text, run.origin, run.height, run.rotation, &mut out);
    }
    out
}

/// A dimension's drawing as plain shapes. (The C++ `plainShapes` overload of
/// a DimensionDrawing.)
pub fn plain_dimension_shapes(drawing: &DimensionDrawing) -> Vec<Geometry> {
    let mut out = Vec::new();
    for line in &drawing.extension_lines {
        append_path(&[line.start, line.end], false, &mut out);
    }
    if drawing.has_dimension_line {
        append_path(
            &[drawing.dimension_line.start, drawing.dimension_line.end],
            false,
            &mut out,
        );
    }
    for curve in &drawing.curves {
        append_path(curve, false, &mut out);
    }
    for stroke in &drawing.arrow_strokes {
        append_path(&[stroke.start, stroke.end], false, &mut out);
    }
    for fill in &drawing.arrow_fills {
        append_path(fill, true, &mut out);
    }
    append_text(
        &drawing.text,
        drawing.text_anchor,
        drawing.text_height,
        drawing.text_rotation,
        &mut out,
    );
    out
}

/// Every label, leader and dimension of a kind other than Aligned in
/// `model`, drawn at 1 : `scale` as the plan view draws them: labels placed
/// together, kept out of each other and of the drawing (label_keep_out), in
/// label id order. Aligned dimensions are left to the writer, which has
/// always drawn them. `measure` is the width the text is taken to have; the
/// estimated one (the C++ default) is what a file with no fonts is judged by.
/// A smart leader's note reads the drawing's surfaces and alignments through
/// `leaders` (leader_presets.rs, leader_context), as the plan view's does.
pub fn draw_annotation_for_export(
    model: &Model,
    scale: f64,
    measure: &TextMeasure,
    leaders: &LeaderContext,
) -> DrawnAnnotation {
    let mut drawn = DrawnAnnotation::new();
    model.entities.for_each(|entity| match &entity.geometry {
        Geometry::Leader(leader) => {
            drawn.insert(
                entity.id,
                plain_shapes(&build_leader(model, leader, scale, measure, leaders)),
            );
        }
        Geometry::Dimension(dimension) if dimension.kind != DimensionKind::Aligned => {
            drawn.insert(
                entity.id,
                plain_dimension_shapes(&build_dimension(
                    dimension,
                    resolve_dimension_style(model, entity),
                    scale,
                )),
            );
        }
        Geometry::Label(_) => {
            // Nothing unless the placer puts it somewhere.
            drawn.entry(entity.id).or_default();
        }
        _ => {}
    });

    // The labels, placed together as the plan view places them.
    let options = LabelLayoutOptions {
        scale,
        measure: measure.clone(),
        linework: label_keep_out(model, scale, measure, KEEP_OUT_LIMIT, leaders),
        ..LabelLayoutOptions::default()
    };
    let layout = layout_labels(model, &label_entities(model), &options);
    for placed in [&layout.placed, &layout.marks_only] {
        for piece in placed {
            let shapes = plain_shapes(&piece.drawing);
            drawn.entry(piece.label).or_default().extend(shapes);
        }
    }
    drawn
}
