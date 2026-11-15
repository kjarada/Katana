//! Geometry-generic operations on entities: validation, extents, picking
//! distance and similarity transforms.

use katana_core::error::{Error, ErrorCode, Result, Status, make_error};
use katana_core::text::format_exact_real;
use katana_geometry::{
    Arc2, Box2, Circle2, CurvePolyline2, Ellipse2, Point2, Polyline2, Segment2, Spline2, Vec2,
};
use katana_math::Mat3;
use katana_math::numerics::{TWO_PI, nearly_equal, tolerance};

use crate::entity::{
    AnchorPoint, AnchorRef, DimensionGeometry, DimensionKind, Geometry, LabelGeometry,
    LeaderGeometry, PointGeometry, TextGeometry, to_string,
};
use crate::leader_values::check_leader_template;
use crate::text_block::estimated_text_corners;
use katana_math::cpp;

/// What can be validated: the C++ `validate` overload set (a geometry here,
/// the table records in tables.rs and annotation.rs). Fails with the reason.
pub trait Validate {
    fn validate(&self) -> Status;
}

/// The C++ `validate(x)`, whichever record `x` is.
pub fn validate<T: Validate + ?Sized>(value: &T) -> Status {
    value.validate()
}

fn require_finite(point: Point2, what: &str) -> Status {
    if !point.is_finite() {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            format!("{what} is not finite"),
        ));
    }
    Ok(())
}

fn require_positive(value: f64, what: &str) -> Status {
    if !(value.is_finite() && value > tolerance::GEOMETRIC) {
        return Err(make_error(
            ErrorCode::InvalidGeometry,
            format!("{what} must be positive"),
            format_exact_real(value),
        ));
    }
    Ok(())
}

/// A reference's point is one there is, and only Along has a parameter: a
/// fraction of the way along, finite and in [0, 1]. Every other point is
/// exactly 0 there, so an anchor that needs no parameter is written as it
/// always was (geometry_blob.rs). The C++ also refused a point past the last
/// enumerator, which an AnchorPoint cannot hold.
fn validate_anchor(reference: &AnchorRef) -> Status {
    if reference.point == AnchorPoint::Along {
        // The inclusive range refuses a NaN as the C++'s two comparisons do.
        if !(reference.parameter.is_finite() && (0.0..=1.0).contains(&reference.parameter)) {
            return Err(make_error(
                ErrorCode::InvalidGeometry,
                "an anchor along an entity is a fraction from 0 to 1",
                format_exact_real(reference.parameter),
            ));
        }
    } else if reference.parameter != 0.0 {
        return Err(make_error(
            ErrorCode::InvalidGeometry,
            "only an anchor along an entity has a fraction",
            to_string(reference.point),
        ));
    }
    Ok(())
}

fn validate_point(point: &PointGeometry) -> Status {
    require_finite(point.position, "point position")
}

fn validate_segment(segment: &Segment2) -> Status {
    require_finite(segment.start, "line start")?;
    require_finite(segment.end, "line end")?;
    if segment.is_degenerate() {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "line has zero length",
        ));
    }
    Ok(())
}

fn validate_arc(arc: &Arc2) -> Status {
    require_finite(arc.center, "arc centre")?;
    require_positive(arc.radius, "arc radius")?;
    if !arc.start_angle.is_finite() || !arc.sweep.is_finite() {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "arc angles are not finite",
        ));
    }
    if arc.sweep.abs() > TWO_PI + tolerance::ANGULAR {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "arc sweeps more than a full turn",
        ));
    }
    if !(arc.length() > tolerance::GEOMETRIC) {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "arc has zero length",
        ));
    }
    Ok(())
}

fn validate_polyline(polyline: &Polyline2) -> Status {
    if polyline.vertices.len() < 2 {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "polyline needs at least two vertices",
        ));
    }
    for vertex in &polyline.vertices {
        require_finite(*vertex, "polyline vertex")?;
    }
    if !(polyline.length() > tolerance::GEOMETRIC) {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "polyline has zero length",
        ));
    }
    Ok(())
}

fn validate_circle(circle: &Circle2) -> Status {
    require_finite(circle.center, "circle centre")?;
    require_positive(circle.radius, "circle radius")
}

/// (The C++ also refused a text or a style name that is not UTF-8, and a
/// justification past the nine, which a `String` and a TextJustify cannot be.)
fn validate_text(text: &TextGeometry) -> Status {
    require_finite(text.position, "text position")?;
    if !text.rotation.is_finite() {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "text rotation is not finite",
        ));
    }
    if text.text.is_empty() {
        return Err(Error::new(ErrorCode::InvalidGeometry, "text is empty"));
    }
    if !(text.paper_height.is_finite() && text.paper_height >= 0.0) {
        return Err(make_error(
            ErrorCode::InvalidGeometry,
            "text paper height must be zero or positive",
            format_exact_real(text.paper_height),
        ));
    }
    require_positive(text.height, "text height")
}

/// (The C++ also refused a kind past OrdinateY, which a DimensionKind cannot
/// be.)
fn validate_dimension(dimension: &DimensionGeometry) -> Status {
    require_finite(dimension.start, "dimension start")?;
    require_finite(dimension.end, "dimension end")?;
    require_finite(dimension.vertex, "dimension vertex")?;
    if !dimension.offset.is_finite() || !dimension.angle.is_finite() {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "dimension offset or angle is not finite",
        ));
    }
    for reference in [
        &dimension.start_ref,
        &dimension.end_ref,
        &dimension.vertex_ref,
    ] {
        validate_anchor(reference)?;
    }
    match dimension.kind {
        DimensionKind::Aligned
        | DimensionKind::Linear
        | DimensionKind::Radius
        | DimensionKind::Diameter => {
            if !(dimension.measurement() > tolerance::GEOMETRIC) {
                return Err(Error::new(
                    ErrorCode::InvalidGeometry,
                    "dimension measures zero length",
                ));
            }
        }
        DimensionKind::Angular => {
            if !(dimension.start.distance_to(dimension.vertex) > tolerance::GEOMETRIC)
                || !(dimension.end.distance_to(dimension.vertex) > tolerance::GEOMETRIC)
            {
                return Err(Error::new(
                    ErrorCode::InvalidGeometry,
                    "an angular dimension's rays need a length",
                ));
            }
            if !(dimension.measurement() > tolerance::ANGULAR) {
                return Err(Error::new(
                    ErrorCode::InvalidGeometry,
                    "dimension measures zero angle",
                ));
            }
        }
        DimensionKind::OrdinateX | DimensionKind::OrdinateY => {
            // A feature ON the datum measures 0, which is a value like any
            // other; what an ordinate needs is a leader to put the value at.
            if !(dimension.start.distance_to(dimension.end) > tolerance::GEOMETRIC) {
                return Err(Error::new(
                    ErrorCode::InvalidGeometry,
                    "an ordinate dimension's leader has zero length",
                ));
            }
        }
    }
    Ok(())
}

/// (The C++ also refused text that is not UTF-8.)
fn validate_label(label: &LabelGeometry) -> Status {
    if label.style.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "a label needs a label style",
        ));
    }
    if (label.target == 0) == label.alignment.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "a label labels exactly one entity or one alignment",
        ));
    }
    if label.part < -1 {
        return Err(make_error(
            ErrorCode::InvalidGeometry,
            "a label's part is -1 or a piece index",
            format!("{}", label.part),
        ));
    }
    require_finite(label.anchor, "label anchor")?;
    if let Some(position) = label.position {
        return require_finite(position, "label position");
    }
    Ok(())
}

/// (The C++ also refused text that is not UTF-8, and an arrow or callout past
/// the last enumerator, which a `String` and the enumerations cannot be.)
fn validate_leader(leader: &LeaderGeometry) -> Status {
    if leader.vertices.len() < 2 {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "a leader needs at least two vertices",
        ));
    }
    for vertex in &leader.vertices {
        require_finite(*vertex, "leader vertex")?;
    }
    if !(Polyline2::new(leader.vertices.clone(), false).length() > tolerance::GEOMETRIC) {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "leader has zero length",
        ));
    }
    for size in [leader.paper_height, leader.arrow_size, leader.landing] {
        if !(size.is_finite() && size >= 0.0) {
            return Err(make_error(
                ErrorCode::InvalidGeometry,
                "a leader's sizes must be zero or positive",
                format_exact_real(size),
            ));
        }
    }
    if leader.landing_angle.is_some_and(|angle| !angle.is_finite()) {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "a leader's landing angle must be finite",
        ));
    }
    validate_anchor(&leader.tip_ref)?;
    // A smart leader (docs/annotation.md, "Smart leaders"): its note is read
    // off the entity its tip names, from ONE template - its own or its label
    // style's.
    if leader.fields && !leader.label_style.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "a leader's note is its own template or its label style's, not both",
        ));
    }
    if !leader.label_style.is_empty() && !leader.text.is_empty() {
        return Err(make_error(
            ErrorCode::InvalidGeometry,
            "a leader in a label style takes its note from the style; it has no text of its own",
            leader.label_style.as_str(),
        ));
    }
    if (leader.fields || !leader.label_style.is_empty()) && !leader.tip_ref.associated() {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "a smart leader reads the entity its tip is on, and its tip names none",
        ));
    }
    if leader.fields {
        if leader.text.is_empty() {
            return Err(Error::new(
                ErrorCode::InvalidGeometry,
                "a smart leader's template is empty",
            ));
        }
        if let Err(error) = check_leader_template(&leader.text) {
            return Err(make_error(
                ErrorCode::InvalidGeometry,
                error.message,
                error.context,
            ));
        }
    }
    Ok(())
}

/// A polyline with arcs may close on two vertices - two arcs make a round
/// shape - so, like Polyline2, only the vertex count of an open walk and a
/// length are required.
fn validate_curve_polyline(polyline: &CurvePolyline2) -> Status {
    if polyline.vertices.len() < 2 {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "polyline needs at least two vertices",
        ));
    }
    for vertex in &polyline.vertices {
        require_finite(vertex.position, "polyline vertex")?;
        if !vertex.bulge.is_finite() {
            return Err(Error::new(
                ErrorCode::InvalidGeometry,
                "polyline bulge is not finite",
            ));
        }
        if vertex.height.is_some_and(|height| !height.is_finite()) {
            return Err(Error::new(
                ErrorCode::InvalidGeometry,
                "polyline height is not finite",
            ));
        }
    }
    if !(polyline.length() > tolerance::GEOMETRIC) {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "polyline has zero length",
        ));
    }
    Ok(())
}

fn validate_ellipse(ellipse: &Ellipse2) -> Status {
    require_finite(ellipse.center, "ellipse centre")?;
    if !ellipse.major_axis.is_finite() || !(ellipse.major_radius() > tolerance::GEOMETRIC) {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "ellipse major axis must be positive",
        ));
    }
    if !(ellipse.ratio.is_finite()
        && ellipse.ratio * ellipse.major_radius() > tolerance::GEOMETRIC
        && ellipse.ratio <= 1.0 + tolerance::RELATIVE)
    {
        return Err(make_error(
            ErrorCode::InvalidGeometry,
            "ellipse ratio must be greater than zero and at most 1",
            format_exact_real(ellipse.ratio),
        ));
    }
    if !ellipse.start_parameter.is_finite()
        || !ellipse.sweep.is_finite()
        || !(ellipse.sweep > tolerance::ANGULAR)
        || ellipse.sweep > TWO_PI + tolerance::ANGULAR
    {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "ellipse sweep must be more than zero and at most a full turn",
        ));
    }
    Ok(())
}

fn validate_spline(spline: &Spline2) -> Status {
    spline.check_structure()?;
    for point in &spline.control_points {
        require_finite(*point, "spline control point")?;
    }
    for point in &spline.fit_points {
        require_finite(*point, "spline fit point")?;
    }
    if !(spline.bounding_box().width() + spline.bounding_box().height() > tolerance::GEOMETRIC) {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "spline has zero length",
        ));
    }
    Ok(())
}

/// Rejects geometry that must never enter the model: non-finite coordinates,
/// non-positive radii or text heights, arcs sweeping more than a full turn,
/// polylines with fewer than two vertices.
impl Validate for Geometry {
    fn validate(&self) -> Status {
        match self {
            Geometry::Point(point) => validate_point(point),
            Geometry::Line(segment) => validate_segment(segment),
            Geometry::Arc(arc) => validate_arc(arc),
            Geometry::Polyline(polyline) => validate_polyline(polyline),
            Geometry::Circle(circle) => validate_circle(circle),
            Geometry::Text(text) => validate_text(text),
            Geometry::Dimension(dimension) => validate_dimension(dimension),
            Geometry::Label(label) => validate_label(label),
            Geometry::Leader(leader) => validate_leader(leader),
            Geometry::CurvePolyline(polyline) => validate_curve_polyline(polyline),
            Geometry::Ellipse(ellipse) => validate_ellipse(ellipse),
            Geometry::Spline(spline) => validate_spline(spline),
        }
    }
}

/// The lines a dimension draws its measurement along, for its box and for
/// picking: the dimension line (the arc of an angular one, chorded), the
/// radial line, or an ordinate's leader. Not what the dimension painter draws
/// - no extension lines, arrows or text.
fn dimension_lines(dimension: &DimensionGeometry) -> Vec<Segment2> {
    let along = Vec2::new(dimension.angle.cos(), dimension.angle.sin());
    match dimension.kind {
        DimensionKind::Aligned => {
            let shift = (dimension.end - dimension.start)
                .normalized()
                .perpendicular()
                * dimension.offset;
            vec![Segment2::new(
                dimension.start + shift,
                dimension.end + shift,
            )]
        }
        DimensionKind::Linear => {
            let first = dimension.start + along.perpendicular() * dimension.offset;
            vec![Segment2::new(
                first,
                first + along * (dimension.end - dimension.start).dot(along),
            )]
        }
        DimensionKind::Angular => {
            let radius = if dimension.offset > tolerance::GEOMETRIC {
                dimension.offset
            } else {
                cpp::min(
                    dimension.start.distance_to(dimension.vertex),
                    dimension.end.distance_to(dimension.vertex),
                )
            };
            let from = (dimension.start - dimension.vertex).angle();
            let arc = Arc2::new(dimension.vertex, radius, from, dimension.measurement());
            const CHORDS: i32 = 32;
            (0..CHORDS)
                .map(|i| {
                    Segment2::new(
                        arc.point_at(f64::from(i) / f64::from(CHORDS)),
                        arc.point_at(f64::from(i + 1) / f64::from(CHORDS)),
                    )
                })
                .collect()
        }
        DimensionKind::Radius | DimensionKind::Diameter => {
            let out = (dimension.start - dimension.vertex).normalized();
            let from = if dimension.kind == DimensionKind::Diameter {
                dimension.vertex - (dimension.start - dimension.vertex)
            } else {
                dimension.vertex
            };
            vec![Segment2::new(
                from,
                dimension.start + out * cpp::max(dimension.offset, 0.0),
            )]
        }
        DimensionKind::OrdinateX | DimensionKind::OrdinateY => {
            vec![Segment2::new(dimension.start, dimension.end)]
        }
    }
}

/// Text extents are approximated (0.6 * height per character) because the
/// entity layer has no font metrics; renderers refine this.
pub fn bounding_box(geometry: &Geometry) -> Box2 {
    match geometry {
        Geometry::Point(point) => {
            let mut bounds = Box2::default();
            bounds.expand(point.position);
            bounds
        }
        Geometry::Line(segment) => segment.bounding_box(),
        Geometry::Arc(arc) => arc.bounding_box(),
        Geometry::Polyline(polyline) => polyline.bounding_box(),
        Geometry::Circle(circle) => circle.bounding_box(),
        Geometry::Text(text) => {
            let mut bounds = Box2::default();
            for corner in estimated_text_corners(text) {
                bounds.expand(corner);
            }
            bounds
        }
        Geometry::Dimension(dimension) => {
            let mut bounds = Box2::default();
            for line in dimension_lines(dimension) {
                bounds.expand(line.start);
                bounds.expand(line.end);
            }
            bounds.expand(dimension.start);
            bounds.expand(dimension.end);
            if dimension.uses_vertex() {
                bounds.expand(dimension.vertex);
            }
            bounds
        }
        // Only the anchor and a dragged position: the text is worked out where
        // it is drawn, and the painters cull labels by their targets.
        Geometry::Label(label) => {
            let mut bounds = Box2::default();
            bounds.expand(label.anchor);
            if let Some(position) = label.position {
                bounds.expand(position);
            }
            bounds
        }
        // The line only: the note's size is in paper millimetres, so the model
        // box it covers depends on the scale it is looked at.
        Geometry::Leader(leader) => Polyline2::new(leader.vertices.clone(), false).bounding_box(),
        Geometry::CurvePolyline(polyline) => polyline.bounding_box(),
        Geometry::Ellipse(ellipse) => ellipse.bounding_box(),
        Geometry::Spline(spline) => spline.bounding_box(),
    }
}

/// Distance from `point` to the drawn geometry (curves, not filled areas).
/// Text and dimensions measure to their baseline / dimension line.
pub fn distance_to(geometry: &Geometry, point: Point2) -> f64 {
    match geometry {
        Geometry::Point(shape) => shape.position.distance_to(point),
        Geometry::Line(segment) => segment.distance_to(point),
        Geometry::Arc(arc) => arc.distance_to(point),
        Geometry::Polyline(polyline) => polyline.distance_to(point).unwrap_or(f64::INFINITY),
        Geometry::Circle(circle) => circle.distance_to(point),
        // To the block's bottom edge - for one line justified BottomLeft, the
        // baseline, which is what a text was always picked by.
        Geometry::Text(text) => {
            let corners = estimated_text_corners(text);
            Segment2::new(corners[0], corners[1]).distance_to(point)
        }
        Geometry::Dimension(dimension) => {
            let mut nearest = f64::INFINITY;
            for line in dimension_lines(dimension) {
                nearest = cpp::min(nearest, line.distance_to(point));
            }
            nearest
        }
        Geometry::Label(label) => {
            let mut nearest = label.anchor.distance_to(point);
            if let Some(position) = label.position {
                nearest = cpp::min(nearest, position.distance_to(point));
            }
            nearest
        }
        Geometry::Leader(leader) => Polyline2::new(leader.vertices.clone(), false)
            .distance_to(point)
            .unwrap_or(f64::INFINITY),
        Geometry::CurvePolyline(polyline) => polyline.distance_to(point).unwrap_or(f64::INFINITY),
        Geometry::Ellipse(ellipse) => ellipse.distance_to(point),
        Geometry::Spline(spline) => spline.distance_to(point),
    }
}

struct Similarity {
    matrix: Mat3,
    scale: f64,
    mirrored: bool,
}

impl Similarity {
    fn point(&self, point: Point2) -> Point2 {
        self.matrix.transform_point(point)
    }
    /// Direction angle after the transform.
    fn angle(&self, radians: f64) -> f64 {
        self.matrix
            .transform_vector(Vec2::new(radians.cos(), radians.sin()))
            .angle()
    }
}

fn as_similarity(m: &Mat3) -> Result<Similarity> {
    let affine = m[(2, 0)] == 0.0 && m[(2, 1)] == 0.0 && m[(2, 2)] == 1.0;
    let column_x = Vec2::new(m[(0, 0)], m[(1, 0)]);
    let column_y = Vec2::new(m[(0, 1)], m[(1, 1)]);
    let scale_x = column_x.length();
    let scale_y = column_y.length();
    let finite = column_x.is_finite()
        && column_y.is_finite()
        && m[(0, 2)].is_finite()
        && m[(1, 2)].is_finite();
    if !affine
        || !finite
        || !(scale_x > 0.0)
        || !nearly_equal(scale_x, scale_y, tolerance::RELATIVE)
        || column_x.dot(column_y).abs() > tolerance::RELATIVE * scale_x * scale_y
    {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "transform must be a similarity (no shear or non-uniform scale)",
        ));
    }
    Ok(Similarity {
        matrix: *m,
        scale: scale_x,
        mirrored: column_x.cross(column_y) < 0.0,
    })
}

fn transform_geometry(geometry: &Geometry, s: &Similarity) -> Geometry {
    match geometry {
        Geometry::Point(point) => Geometry::Point(PointGeometry {
            position: s.point(point.position),
        }),
        Geometry::Line(segment) => {
            Geometry::Line(Segment2::new(s.point(segment.start), s.point(segment.end)))
        }
        Geometry::Arc(arc) => Geometry::Arc(Arc2::new(
            s.point(arc.center),
            arc.radius * s.scale,
            s.angle(arc.start_angle),
            if s.mirrored { -arc.sweep } else { arc.sweep },
        )),
        Geometry::Polyline(polyline) => {
            let mut result = polyline.clone();
            for vertex in &mut result.vertices {
                *vertex = s.point(*vertex);
            }
            Geometry::Polyline(result)
        }
        Geometry::Circle(circle) => Geometry::Circle(Circle2::new(
            s.point(circle.center),
            circle.radius * s.scale,
        )),
        Geometry::Text(text) => {
            let mut result = text.clone();
            result.position = s.point(text.position);
            result.height = text.height * s.scale;
            result.rotation = s.angle(text.rotation);
            Geometry::Text(result)
        }
        Geometry::Dimension(dimension) => {
            let mut result = dimension.clone();
            result.start = s.point(dimension.start);
            result.end = s.point(dimension.end);
            result.vertex = s.point(dimension.vertex);
            result.angle = s.angle(dimension.angle);
            match dimension.kind {
                DimensionKind::Aligned | DimensionKind::Linear => {
                    // The dimension line is on the other side of its direction
                    // once mirrored, so its signed offset changes sign.
                    result.offset =
                        dimension.offset * s.scale * if s.mirrored { -1.0 } else { 1.0 };
                }
                DimensionKind::Angular => {
                    result.offset = dimension.offset * s.scale;
                    // A mirror turns the counter-clockwise sweep from start to
                    // end clockwise: swapping the rays measures the same angle.
                    if s.mirrored {
                        std::mem::swap(&mut result.start, &mut result.end);
                        std::mem::swap(&mut result.start_ref, &mut result.end_ref);
                    }
                }
                DimensionKind::Radius
                | DimensionKind::Diameter
                | DimensionKind::OrdinateX
                | DimensionKind::OrdinateY => {
                    result.offset = dimension.offset * s.scale;
                }
            }
            Geometry::Dimension(result)
        }
        Geometry::Label(label) => {
            let mut result = label.clone();
            result.anchor = s.point(label.anchor);
            if let Some(position) = label.position {
                result.position = Some(s.point(position));
            }
            Geometry::Label(result)
        }
        // The sizes are paper millimetres and stay as they are at any scale.
        Geometry::Leader(leader) => {
            let mut result = leader.clone();
            for vertex in &mut result.vertices {
                *vertex = s.point(*vertex);
            }
            if let Some(angle) = result.landing_angle {
                result.landing_angle = Some(s.angle(angle));
            }
            Geometry::Leader(result)
        }
        // Heights are elevations, not plan distances: a plan scale leaves them
        // alone. A mirror turns every arc the other way.
        Geometry::CurvePolyline(polyline) => {
            let mut result = polyline.clone();
            for vertex in &mut result.vertices {
                vertex.position = s.point(vertex.position);
                if s.mirrored {
                    vertex.bulge = -vertex.bulge;
                }
            }
            Geometry::CurvePolyline(result)
        }
        // Mirrored, the point at t lands where the image's point at -t is (the
        // minor axis flips relative to the major), so the arc is re-expressed
        // as starting at -(start + sweep) rather than stored as a negative
        // sweep.
        Geometry::Ellipse(ellipse) => {
            let mut result = *ellipse;
            result.center = s.point(ellipse.center);
            result.major_axis = s.matrix.transform_vector(ellipse.major_axis);
            if s.mirrored {
                result.start_parameter = -(ellipse.start_parameter + ellipse.sweep);
            }
            Geometry::Ellipse(result)
        }
        Geometry::Spline(spline) => {
            let mut result = spline.clone();
            for point in &mut result.control_points {
                *point = s.point(*point);
            }
            for point in &mut result.fit_points {
                *point = s.point(*point);
            }
            Geometry::Spline(result)
        }
    }
}

/// Applies a 2D similarity transform (translation, rotation, uniform scale,
/// mirror). Returns InvalidArgument for non-similarity matrices (shear or
/// non-uniform scale), which would turn circles into ellipses. Mirrors reverse
/// arc direction and dimension side; text is repositioned but never rendered
/// back to front.
pub fn transformed(geometry: &Geometry, transform: &Mat3) -> Result<Geometry> {
    let similarity = as_similarity(transform)?;
    let result = transform_geometry(geometry, &similarity);
    // E.g. scaled below the geometric tolerance.
    validate(&result)?;
    Ok(result)
}
