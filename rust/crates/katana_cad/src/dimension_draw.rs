//! Turning a dimension and its style into plain geometry (PLAN.MD Phase 09;
//! include/katana/cad/dimension_draw.hpp).
//!
//! One definition, used by the 2D viewport, the 3D scene builder and the
//! extent calculation, so a dimension cannot look different in two views or be
//! culled at a size it is not drawn at.
//!
//! EVERYTHING HERE IS IN MODEL UNITS. The viewport previously drew a 5-pixel
//! tick and a 12-pixel label, which look right on screen and plot at whatever
//! size the paper happens to give them - a dimension is part of the drawing,
//! so its text has to keep its size relative to the geometry it annotates.
//!
//! The extent this returns covers the text, the arrows and the extension
//! overshoot. entity's bounding_box for a Dimension covers only the measured
//! points and the dimension line, which is smaller than what is drawn, so
//! using it to cull culls a dimension whose label is still on screen.

use std::sync::OnceLock;

use katana_entity::annotation::annotation_model_size;
use katana_entity::dimension_text::dimension_label;
use katana_entity::label_text::format_dms;
use katana_entity::tables::DEFAULT_DIMENSION_STYLE_NAME;
use katana_entity::text_block::character_count;
use katana_entity::{ArrowHead, DimensionGeometry, DimensionKind, DimensionStyle, Entity, Model};
use katana_geometry::{Arc2, Box2, Point2, Segment2, Vec2};
use katana_math::cpp;
use katana_math::numerics::{HALF_PI, PI, TWO_PI, tolerance};

#[derive(Clone, Debug, PartialEq)]
pub struct DimensionDrawing {
    /// The two extension lines, from just off each measured point to just past
    /// the dimension line.
    pub extension_lines: Vec<Segment2>,
    pub dimension_line: Segment2,
    /// False for the kinds that have no straight dimension line - an angular
    /// dimension's is its arc, in `curves` - so a consumer does not draw a
    /// zero-length one.
    pub has_dimension_line: bool,
    /// Polylines: an angular dimension's arc, chorded to the drawing's own
    /// accuracy, and a radial or ordinate dimension's leader.
    pub curves: Vec<Vec<Point2>>,
    /// Arrow strokes, for the tick and open heads.
    pub arrow_strokes: Vec<Segment2>,
    /// Closed arrow outlines, one polygon per end, for the filled and dot
    /// heads. A renderer that cannot fill draws them as closed polylines; the
    /// shape is still correct, only hollow.
    pub arrow_fills: Vec<Vec<Point2>>,

    pub text: String,
    /// Left end of the text baseline.
    pub text_anchor: Point2,
    pub text_height: f64,
    /// Radians, counter-clockwise.
    pub text_rotation: f64,

    /// Everything above, including the text. What a viewport should cull
    /// against.
    pub extent: Box2,
}

impl Default for DimensionDrawing {
    fn default() -> Self {
        Self {
            extension_lines: Vec::new(),
            dimension_line: Segment2::default(),
            has_dimension_line: true,
            curves: Vec::new(),
            arrow_strokes: Vec::new(),
            arrow_fills: Vec::new(),
            text: String::new(),
            text_anchor: Point2::default(),
            text_height: 0.0,
            text_rotation: 0.0,
            extent: Box2::default(),
        }
    }
}

impl DimensionDrawing {
    pub fn empty(&self) -> bool {
        self.text.is_empty() && self.extension_lines.is_empty()
    }
}

/// Advance per character as a fraction of the text height. The viewport draws
/// text with a simple stroke font whose glyphs are about six tenths as wide as
/// they are tall; the same figure is used to centre the label and to size its
/// extent, so the two cannot disagree.
const ADVANCE_PER_CHARACTER: f64 = 0.6;

/// A closed arrowhead is three times as long as it is wide, which is the
/// proportion DIMASZ implies and what reads as an arrow rather than a wedge.
const ARROW_WIDTH_FRACTION: f64 = 1.0 / 3.0;

/// Sides of the polygon approximating a dot head. Twelve is smooth at any size
/// a dimension arrow is drawn, and cheap.
const DOT_SIDES: i32 = 12;

/// Sides of the polygon an angular dimension's arc is chorded into per full
/// turn: 128 keeps the chord's sagitta under a hundredth of a percent of the
/// radius, invisible at any size a dimension arc is drawn.
const ARC_CHORDS_PER_TURN: i32 = 128;

fn append_arrow(
    drawing: &mut DimensionDrawing,
    tip: Point2,
    along: Vec2,
    normal: Vec2,
    style: &DimensionStyle,
) {
    let size = style.arrow_size;
    match style.arrow_head {
        ArrowHead::None => {}
        ArrowHead::Tick => {
            // The surveyor's oblique tick: a stroke at 45 degrees through the
            // point, extending equally either side.
            let direction = (along + normal).normalized();
            drawing.arrow_strokes.push(Segment2::new(
                tip - direction * (size * 0.5),
                tip + direction * (size * 0.5),
            ));
        }
        ArrowHead::Open => {
            let back = tip - along * size;
            let half = normal * (size * ARROW_WIDTH_FRACTION);
            drawing.arrow_strokes.push(Segment2::new(tip, back + half));
            drawing.arrow_strokes.push(Segment2::new(tip, back - half));
        }
        ArrowHead::ClosedFilled => {
            let back = tip - along * size;
            let half = normal * (size * ARROW_WIDTH_FRACTION);
            drawing
                .arrow_fills
                .push(vec![tip, back + half, back - half]);
        }
        ArrowHead::Dot => {
            let radius = size * 0.25;
            let mut circle = Vec::with_capacity(DOT_SIDES as usize);
            for i in 0..DOT_SIDES {
                let angle = TWO_PI * f64::from(i) / f64::from(DOT_SIDES);
                circle.push(Point2::new(
                    tip.x + angle.cos() * radius,
                    tip.y + angle.sin() * radius,
                ));
            }
            drawing.arrow_fills.push(circle);
        }
    }
}

/// The label's width by the same estimate the extent uses, counted in
/// characters rather than bytes so the degree and diameter signs count once.
fn label_width(text: &str, height: f64) -> f64 {
    height * ADVANCE_PER_CHARACTER * character_count(text) as f64
}

/// A direction turned to read left to right: the flip every label makes when
/// its line runs from right to left.
fn readable(along: Vec2) -> Vec2 {
    let angle = along.angle();
    if angle > HALF_PI || angle < -HALF_PI {
        -along
    } else {
        along
    }
}

/// Places the label centred on `middle`, `gap` off the line on the side
/// `outward` points to, running along `along` (turned to read), and records
/// the text box in the extent.
fn place_label(
    drawing: &mut DimensionDrawing,
    middle: Point2,
    along: Vec2,
    outward: Vec2,
    style: &DimensionStyle,
) {
    let text_along = readable(along);
    drawing.text_rotation = text_along.angle();
    let width = label_width(&drawing.text, style.text_height);
    // Above the line when `outward` is the text's up, below it (by its own
    // height) when it is the text's down, so the gap is the gap either way.
    let up = text_along.perpendicular();
    let lift = if outward.dot(up) >= 0.0 {
        style.text_gap
    } else {
        -(style.text_gap + style.text_height)
    };
    drawing.text_anchor = middle + up * lift - text_along * (width * 0.5);
    expand_by_text(drawing, text_along, up, width, style.text_height);
}

/// Both corners of the text box and the two between, so a rotated label is
/// covered whichever way it runs.
fn expand_by_text(
    drawing: &mut DimensionDrawing,
    text_along: Vec2,
    up: Vec2,
    width: f64,
    height: f64,
) {
    let anchor = drawing.text_anchor;
    drawing.extent.expand(anchor);
    drawing.extent.expand(anchor + text_along * width);
    drawing.extent.expand(anchor + up * height);
    drawing
        .extent
        .expand(anchor + text_along * width + up * height);
}

fn expand_by_pieces(drawing: &mut DimensionDrawing) {
    let mut extent = drawing.extent;
    for segment in &drawing.extension_lines {
        extent.expand(segment.start);
        extent.expand(segment.end);
    }
    if drawing.has_dimension_line {
        extent.expand(drawing.dimension_line.start);
        extent.expand(drawing.dimension_line.end);
    }
    for curve in &drawing.curves {
        for &point in curve {
            extent.expand(point);
        }
    }
    for stroke in &drawing.arrow_strokes {
        extent.expand(stroke.start);
        extent.expand(stroke.end);
    }
    for fill in &drawing.arrow_fills {
        for &point in fill {
            extent.expand(point);
        }
    }
    drawing.extent = extent;
}

/// An extension line from the measured point `from` to `foot` on the
/// dimension line, gapped at the point and running past the line. `fallback`
/// is the direction when the two coincide (a point already on the line).
fn extension_line(from: Point2, foot: Point2, fallback: Vec2, style: &DimensionStyle) -> Segment2 {
    let towards = foot - from;
    let direction = if towards.length() > tolerance::GEOMETRIC {
        towards.normalized()
    } else {
        fallback
    };
    Segment2::new(
        from + direction * style.extension_offset,
        foot + direction * style.extension_beyond,
    )
}

/// The Aligned kind: the construction every dimension had before the kinds.
fn build_aligned(dimension: &DimensionGeometry, style: &DimensionStyle) -> DimensionDrawing {
    let mut drawing = DimensionDrawing::default();

    let span = dimension.end - dimension.start;
    let length = span.length();
    if !(length > tolerance::GEOMETRIC) || !dimension.offset.is_finite() {
        // No direction to offset along. Returning nothing beats inventing an
        // orientation and drawing a dimension pointing somewhere arbitrary.
        return drawing;
    }
    let along = span / length;
    let normal = along.perpendicular();

    // Which side the dimension line sits on. Everything that reaches "away
    // from the measured points" is expressed through this, so a negative
    // offset mirrors the whole construction rather than turning it inside out.
    let side = if dimension.offset >= 0.0 { 1.0 } else { -1.0 };
    let outward = normal * side;

    let a = dimension.start + normal * dimension.offset;
    let b = dimension.end + normal * dimension.offset;
    drawing.dimension_line = Segment2::new(a, b);

    // DIMEXO leaves a gap at the measured point so the extension line does not
    // touch the feature; DIMEXE runs it past the dimension line.
    drawing.extension_lines.push(Segment2::new(
        dimension.start + outward * style.extension_offset,
        a + outward * style.extension_beyond,
    ));
    drawing.extension_lines.push(Segment2::new(
        dimension.end + outward * style.extension_offset,
        b + outward * style.extension_beyond,
    ));

    // Arrows point OUTWARD from the dimension line's ends, which is what they
    // do when the text fits between them.
    append_arrow(&mut drawing, a, -along, normal, style);
    append_arrow(&mut drawing, b, along, normal, style);

    drawing.text = dimension_text(dimension, style);
    drawing.text_height = style.text_height;

    // Rotated with the dimension, and flipped when that would leave it upside
    // down: a label is read left to right whichever way the dimension runs.
    let mut rotation = along.angle();
    let mut text_along = along;
    if rotation > HALF_PI || rotation < -HALF_PI {
        rotation += PI;
        if rotation > PI {
            rotation -= TWO_PI;
        }
        text_along = -along;
    }
    drawing.text_rotation = rotation;

    // By the text's BYTES, as the C++ counts them (std::string::size); the
    // other kinds count characters (label_width).
    let width = style.text_height * ADVANCE_PER_CHARACTER * drawing.text.len() as f64;
    let middle = (a + b) * 0.5;
    // Sat off the dimension line by DIMGAP, on the side away from the measured
    // points, and backed up half its width so it reads centred.
    drawing.text_anchor = middle + outward * style.text_gap - text_along * (width * 0.5);

    // The extent covers what is DRAWN, not what is measured.
    drawing.extent.expand(dimension.start);
    drawing.extent.expand(dimension.end);
    expand_by_pieces(&mut drawing);
    let up = Vec2::new(-text_along.y, text_along.x);
    expand_by_text(&mut drawing, text_along, up, width, style.text_height);

    drawing
}

/// Linear: horizontal, vertical or rotated. The dimension line is parallel to
/// the measuring direction, `offset` to its left through start; each measured
/// point is carried to it by its own extension line.
fn build_linear(dimension: &DimensionGeometry, style: &DimensionStyle) -> DimensionDrawing {
    let mut drawing = DimensionDrawing::default();
    let along = Vec2::new(dimension.angle.cos(), dimension.angle.sin());
    let normal = along.perpendicular();
    let measured = (dimension.end - dimension.start).dot(along);
    if !(measured.abs() > tolerance::GEOMETRIC) {
        return drawing;
    }
    let a = dimension.start + normal * dimension.offset;
    let b = a + along * measured;
    drawing.dimension_line = Segment2::new(a, b);
    let outward = normal * if dimension.offset >= 0.0 { 1.0 } else { -1.0 };
    drawing
        .extension_lines
        .push(extension_line(dimension.start, a, outward, style));
    drawing
        .extension_lines
        .push(extension_line(dimension.end, b, outward, style));
    let line_along = (b - a).normalized();
    append_arrow(
        &mut drawing,
        a,
        -line_along,
        line_along.perpendicular(),
        style,
    );
    append_arrow(
        &mut drawing,
        b,
        line_along,
        line_along.perpendicular(),
        style,
    );
    drawing.text = dimension_text(dimension, style);
    drawing.text_height = style.text_height;
    drawing.extent.expand(dimension.start);
    drawing.extent.expand(dimension.end);
    expand_by_pieces(&mut drawing);
    place_label(&mut drawing, (a + b) * 0.5, line_along, outward, style);
    drawing
}

/// Angular: an arc about the vertex from the start ray counter-clockwise to
/// the end ray, extension lines along the rays out to the arc when it lies
/// beyond them, arrows along the arc at its ends, the value outside it.
fn build_angular(dimension: &DimensionGeometry, style: &DimensionStyle) -> DimensionDrawing {
    let mut drawing = DimensionDrawing {
        has_dimension_line: false,
        ..DimensionDrawing::default()
    };
    let first = dimension.start - dimension.vertex;
    let second = dimension.end - dimension.vertex;
    let sweep = dimension.measurement();
    if !(first.length() > tolerance::GEOMETRIC)
        || !(second.length() > tolerance::GEOMETRIC)
        || !(sweep > tolerance::ANGULAR)
    {
        return drawing;
    }
    let radius = if dimension.offset > tolerance::GEOMETRIC {
        dimension.offset
    } else {
        cpp::min(first.length(), second.length())
    };
    let arc = Arc2::new(dimension.vertex, radius, first.angle(), sweep);
    let chords = std::cmp::max(
        8,
        (f64::from(ARC_CHORDS_PER_TURN) * sweep / TWO_PI).ceil() as i32,
    );
    let mut curve = Vec::with_capacity(chords as usize + 1);
    for i in 0..=chords {
        curve.push(arc.point_at(f64::from(i) / f64::from(chords)));
    }
    let arc_start = curve[0];
    let arc_end = curve[curve.len() - 1];
    drawing.curves.push(curve);
    for (ray, on_arc) in [(first, arc_start), (second, arc_end)] {
        let measured = dimension.vertex + ray;
        // The ray reaches the arc already when the arc is inside its length.
        if radius > ray.length() + tolerance::GEOMETRIC {
            drawing
                .extension_lines
                .push(extension_line(measured, on_arc, ray.normalized(), style));
        }
    }
    let radial_start = (arc_start - dimension.vertex).normalized();
    let radial_end = (arc_end - dimension.vertex).normalized();
    // Arrows point out of the arc along its tangents.
    append_arrow(
        &mut drawing,
        arc_start,
        -radial_start.perpendicular(),
        radial_start,
        style,
    );
    append_arrow(
        &mut drawing,
        arc_end,
        radial_end.perpendicular(),
        radial_end,
        style,
    );
    drawing.text = dimension_text(dimension, style);
    drawing.text_height = style.text_height;
    let middle = arc.point_at(0.5);
    let radial_middle = (middle - dimension.vertex).normalized();
    expand_by_pieces(&mut drawing);
    place_label(
        &mut drawing,
        middle,
        radial_middle.perpendicular(),
        radial_middle,
        style,
    );
    drawing
}

/// Radius and diameter: a line through the centre and the point on the curve,
/// continued `offset` beyond it to the value; arrows at the curve pointing
/// out to it.
fn build_radial(dimension: &DimensionGeometry, style: &DimensionStyle) -> DimensionDrawing {
    let mut drawing = DimensionDrawing {
        has_dimension_line: false,
        ..DimensionDrawing::default()
    };
    let radial = dimension.start - dimension.vertex;
    let radius = radial.length();
    if !(radius > tolerance::GEOMETRIC) {
        return drawing;
    }
    let out = radial / radius;
    let diameter = dimension.kind == DimensionKind::Diameter;
    let from = if diameter {
        dimension.vertex - radial
    } else {
        dimension.vertex
    };
    let to = dimension.start + out * cpp::max(dimension.offset, 0.0);
    drawing.curves.push(vec![from, to]);
    append_arrow(
        &mut drawing,
        dimension.start,
        out,
        out.perpendicular(),
        style,
    );
    if diameter {
        append_arrow(&mut drawing, from, -out, out.perpendicular(), style);
    }
    drawing.text = dimension_text(dimension, style);
    drawing.text_height = style.text_height;
    expand_by_pieces(&mut drawing);
    // The value along the part of the line outside the curve, or about the
    // point on the curve when the line stops there.
    let middle = if dimension.offset > tolerance::GEOMETRIC {
        dimension.start + out * (0.5 * dimension.offset)
    } else {
        dimension.vertex + radial * 0.5
    };
    place_label(
        &mut drawing,
        middle,
        out,
        readable(out).perpendicular(),
        style,
    );
    drawing
}

/// Ordinate: a leader from the feature to where the value is written, the
/// value along the leader's last stretch.
fn build_ordinate(dimension: &DimensionGeometry, style: &DimensionStyle) -> DimensionDrawing {
    let mut drawing = DimensionDrawing {
        has_dimension_line: false,
        ..DimensionDrawing::default()
    };
    let leader = dimension.end - dimension.start;
    let length = leader.length();
    if !(length > tolerance::GEOMETRIC) {
        return drawing;
    }
    let along = leader / length;
    let gap = cpp::min(style.extension_offset, 0.5 * length);
    drawing
        .curves
        .push(vec![dimension.start + along * gap, dimension.end]);
    drawing.text = dimension_text(dimension, style);
    drawing.text_height = style.text_height;
    expand_by_pieces(&mut drawing);
    // Beyond the leader's end, reading along it.
    let text_along = readable(along);
    let width = label_width(&drawing.text, style.text_height);
    drawing.text_rotation = text_along.angle();
    let base = dimension.end + along * style.text_gap;
    // From the end outwards: a text that reads back along the leader is set
    // from its far end.
    let start = if text_along.dot(along) >= 0.0 {
        base
    } else {
        base + along * width
    };
    drawing.text_anchor = start - text_along.perpendicular() * (0.5 * style.text_height);
    let up = text_along.perpendicular();
    expand_by_text(&mut drawing, text_along, up, width, style.text_height);
    drawing
}

/// `style` with its sizes in model units at 1 : `scale`: itself for a
/// model-unit style, its paper millimetres x scale / 1000 for a paper-sized
/// one.
pub fn dimension_style_at_scale(style: &DimensionStyle, scale: f64) -> DimensionStyle {
    if !style.paper_sized {
        return style.clone();
    }
    let mut sized = style.clone();
    for size in [
        &mut sized.text_height,
        &mut sized.text_gap,
        &mut sized.extension_offset,
        &mut sized.extension_beyond,
        &mut sized.arrow_size,
    ] {
        *size = annotation_model_size(*size, scale);
    }
    sized
}

/// The text a dimension shows: an override verbatim, else the measured value
/// formatted by the style - lengths by format_measurement, an angle in
/// degrees, minutes and seconds, a radius with "R" and a diameter with the
/// diameter sign before it when the style has no prefix of its own.
pub fn dimension_text(dimension: &DimensionGeometry, style: &DimensionStyle) -> String {
    if !dimension.text_override.is_empty() {
        return dimension.text_override.clone();
    }
    match dimension.kind {
        DimensionKind::Angular => {
            // Degrees, minutes and whole seconds: the form an angle on a survey
            // drawing is read in. The style's unit scale and rounding are for
            // lengths and do not apply.
            format!(
                "{}{}{}",
                style.prefix,
                format_dms(dimension.measurement(), 0),
                style.suffix
            )
        }
        DimensionKind::Radius | DimensionKind::Diameter => {
            let mut text = dimension_label(dimension.measurement(), "", style);
            if style.prefix.is_empty() {
                // ISO 129-1 writes a radius "R" and a diameter with the
                // diameter sign; U+00D8 is the form every face has.
                text.insert_str(
                    0,
                    if dimension.kind == DimensionKind::Radius {
                        "R"
                    } else {
                        "\u{D8}"
                    },
                );
            }
            text
        }
        DimensionKind::Aligned
        | DimensionKind::Linear
        | DimensionKind::OrdinateX
        | DimensionKind::OrdinateY => dimension_label(dimension.measurement(), "", style),
    }
}

/// Builds the drawing for `dimension` under `style`, for every kind (entity's
/// DimensionKind says what each measures and where its line goes).
///
/// `scale` is the annotation scale the drawing is for, 1 : scale. It matters
/// only to a paper-sized style (DimensionStyle::paper_sized), whose sizes are
/// millimetres on paper and are drawn at size x scale / 1000 model units; a
/// model-unit style draws the same at every scale. The C++ default is
/// DEFAULT_ANNOTATION_SCALE, which is what the spatial index and a caller with
/// no view use.
///
/// A degenerate dimension - the two measured points coincident, so there is
/// no direction to offset along - produces an empty drawing rather than a
/// division by zero or an arbitrary orientation. The model rejects such a
/// dimension on the way in; this is the renderer refusing to guess if one ever
/// arrives.
pub fn build_dimension(
    dimension: &DimensionGeometry,
    style: &DimensionStyle,
    scale: f64,
) -> DimensionDrawing {
    let sized = dimension_style_at_scale(style, scale);
    match dimension.kind {
        DimensionKind::Aligned => build_aligned(dimension, &sized),
        DimensionKind::Linear => build_linear(dimension, &sized),
        DimensionKind::Angular => build_angular(dimension, &sized),
        DimensionKind::Radius | DimensionKind::Diameter => build_radial(dimension, &sized),
        DimensionKind::OrdinateX | DimensionKind::OrdinateY => build_ordinate(dimension, &sized),
    }
}

/// The style a dimension resolves to: its layer's named style, else the
/// document default. Follows the same ByLayer shape as entity's
/// resolve_display, and returns the default rather than None so that a
/// renderer always has something to draw with.
pub fn resolve_dimension_style<'a>(model: &'a Model, entity: &Entity) -> &'a DimensionStyle {
    // Never absent: a renderer with no style has nothing to draw with, and a
    // missing style is not a reason to omit a dimension from the drawing.
    static FALLBACK: OnceLock<DimensionStyle> = OnceLock::new();

    // A layer naming a style that is not there resolves to the default, the
    // same way a layer naming a missing linetype draws solid: the drawing
    // still appears, in the document's default style.
    if let Some(layer) = model.layers.find(&entity.layer)
        && !layer.dimension_style.is_empty()
        && let Some(named) = model.dimension_styles.find(&layer.dimension_style)
    {
        return named;
    }
    if let Some(standard) = model.dimension_styles.find(DEFAULT_DIMENSION_STYLE_NAME) {
        return standard;
    }
    FALLBACK.get_or_init(DimensionStyle::default)
}
