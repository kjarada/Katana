//! Making dimensions of every kind from the points a person or an agent gives
//! (docs/annotation.md, "Dimensions";
//! include/katana/cad/annotation/dimension_build.hpp,
//! src/katana_cad/annotation/dimension_build.cpp): the arithmetic DIM LINEAR,
//! DIM ANGULAR, DIM BASELINE and the rest share with the interactive tool, so
//! that a dimension typed and a dimension drawn come out alike.
//!
//! Each point may be ANCHORED to another entity's point (entity's anchor.rs);
//! the dimension then carries the reference and follows that entity through
//! every edit (associative.rs).

use katana_commands::{ChangeSet, ChangeSetCommand, CommandPtr};
use katana_core::error::make_error;
use katana_core::{Error, ErrorCode, Result};
use katana_entity::anchor::{describe, resolve_anchor};
use katana_entity::{
    AnchorPoint, AnchorRef, DimensionGeometry, DimensionKind, Entity, EntityId, Geometry, Model,
    validate,
};
use katana_geometry::{Point2, Segment2, Vec2};
use katana_math::cpp;
use katana_math::numerics::{PI, normalize_angle, tolerance};

use crate::dimension_draw::{dimension_style_at_scale, resolve_dimension_style};

/// A point, and the entity point it came from when it came from one.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AnchoredPoint {
    pub point: Point2,
    pub r#ref: AnchorRef,
}

fn checked(dimension: DimensionGeometry) -> Result<DimensionGeometry> {
    validate(&Geometry::Dimension(dimension.clone()))?;
    Ok(dimension)
}

fn unit(angle: f64) -> Vec2 {
    Vec2::new(angle.cos(), angle.sin())
}

fn id_context(id: EntityId) -> String {
    format!("id={id}")
}

/// An anchored point from `reference` resolved against the model; NotFound
/// when the entity is gone, InvalidArgument when it offers no such point.
pub fn anchored_point(model: &Model, reference: &AnchorRef) -> Result<AnchoredPoint> {
    let Some(entity) = model.entities.find(reference.entity) else {
        return Err(make_error(
            ErrorCode::NotFound,
            "entity does not exist",
            id_context(reference.entity),
        ));
    };
    let Some(point) = resolve_anchor(entity, reference) else {
        // Every point OF an entity is read here - a label's, a leader's tip,
        // VERTEX INSERT's #id.sN - so the sentence names no one use: "to
        // dimension to" misled an agent inserting at a segment that was not.
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "that entity has no such point",
            format!("{} {}", id_context(reference.entity), describe(reference)),
        ));
    };
    Ok(AnchoredPoint {
        point,
        r#ref: *reference,
    })
}

/// The point an entity is referred to by when no point is named: a point's or
/// a text's position, a line's or a polyline's start, an arc's or a circle's
/// centre.
pub fn default_anchor(geometry: &Geometry) -> AnchorPoint {
    match geometry {
        Geometry::Arc(_) | Geometry::Circle(_) | Geometry::Ellipse(_) => AnchorPoint::Centre,
        Geometry::Line(_)
        | Geometry::Polyline(_)
        | Geometry::CurvePolyline(_)
        | Geometry::Spline(_)
        | Geometry::Leader(_) => AnchorPoint::Start,
        _ => AnchorPoint::Position,
    }
}

/// Aligned: |b - a|, the dimension line through `at`.
pub fn aligned_dimension(
    a: &AnchoredPoint,
    b: &AnchoredPoint,
    at: Point2,
) -> Result<DimensionGeometry> {
    let span = b.point - a.point;
    if !(span.length() > tolerance::GEOMETRIC) {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the two points coincide",
        ));
    }
    checked(DimensionGeometry {
        start: a.point,
        end: b.point,
        start_ref: a.r#ref,
        end_ref: b.r#ref,
        offset: (at - a.point).dot(span.normalized().perpendicular()),
        ..DimensionGeometry::default()
    })
}

/// Linear along `angle` (radians); with no angle, horizontal or vertical as
/// `at` is dragged: a line through `at` above or below the two points gives a
/// horizontal dimension, one beside them a vertical one - how every CAD
/// program's linear dimension reads the cursor.
pub fn linear_dimension(
    a: &AnchoredPoint,
    b: &AnchoredPoint,
    at: Point2,
    angle: Option<f64>,
) -> Result<DimensionGeometry> {
    let direction = match angle {
        Some(angle) => angle,
        None => {
            // Where `at` is beyond the box of the two points: further above or
            // below it than beside it reads as a horizontal dimension.
            let middle = (a.point + b.point) * 0.5;
            let half_width = 0.5 * (b.point.x - a.point.x).abs();
            let half_height = 0.5 * (b.point.y - a.point.y).abs();
            let beside = cpp::max(0.0, (at.x - middle.x).abs() - half_width);
            let above = cpp::max(0.0, (at.y - middle.y).abs() - half_height);
            let horizontal = above >= beside;
            let mut direction = if horizontal { 0.0 } else { 0.5 * PI };
            // Nothing to measure that way: the other way is what was meant.
            if (b.point - a.point).dot(unit(direction)).abs() <= tolerance::GEOMETRIC {
                direction = if horizontal { 0.5 * PI } else { 0.0 };
            }
            direction
        }
    };
    let along = unit(direction);
    if !((b.point - a.point).dot(along).abs() > tolerance::GEOMETRIC) {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the two points are level along the dimension's direction",
        ));
    }
    checked(DimensionGeometry {
        kind: DimensionKind::Linear,
        angle: direction,
        start: a.point,
        end: b.point,
        start_ref: a.r#ref,
        end_ref: b.r#ref,
        offset: (at - a.point).dot(along.perpendicular()),
        ..DimensionGeometry::default()
    })
}

/// Angular at `vertex` between the rays through `first` and `second`,
/// measuring the side `at` is on (the arc through it); with no `at`, the
/// counter-clockwise angle from first to second on the shorter ray's radius.
pub fn angular_dimension(
    vertex: &AnchoredPoint,
    first: &AnchoredPoint,
    second: &AnchoredPoint,
    at: Option<Point2>,
) -> Result<DimensionGeometry> {
    let mut dimension = DimensionGeometry {
        kind: DimensionKind::Angular,
        vertex: vertex.point,
        vertex_ref: vertex.r#ref,
        start: first.point,
        start_ref: first.r#ref,
        end: second.point,
        end_ref: second.r#ref,
        ..DimensionGeometry::default()
    };
    if let Some(at) = at {
        let to_at = at - vertex.point;
        dimension.offset = to_at.length();
        // The side `at` is on: when the arc through it is not in the
        // counter-clockwise sweep from first to second, the rays swap and
        // the other angle is measured.
        let sweep = dimension.measurement();
        let at_angle = normalize_angle(to_at.angle() - (first.point - vertex.point).angle());
        if at_angle > sweep {
            std::mem::swap(&mut dimension.start, &mut dimension.end);
            std::mem::swap(&mut dimension.start_ref, &mut dimension.end_ref);
        }
    }
    checked(dimension)
}

/// Angular between two line entities: the vertex is where they meet
/// (extended), each ray runs to the line's end on the side `at` is, and the
/// dimension follows both lines - the vertex is worked out again from them
/// whenever they move. InvalidArgument for parallel lines.
pub fn angular_between_lines(
    model: &Model,
    first: EntityId,
    second: EntityId,
    at: Point2,
) -> Result<DimensionGeometry> {
    let line_of = |id: EntityId| -> Result<Segment2> {
        let Some(entity) = model.entities.find(id) else {
            return Err(make_error(
                ErrorCode::NotFound,
                "entity does not exist",
                id_context(id),
            ));
        };
        match &entity.geometry {
            Geometry::Line(line) => Ok(*line),
            _ => Err(make_error(
                ErrorCode::InvalidArgument,
                "an angle between lines needs two lines",
                id_context(id),
            )),
        }
    };
    // Both looked up before either is judged, as the C++ does.
    let a = line_of(first);
    let b = line_of(second);
    let a = a?;
    let b = b?;
    let da = a.end - a.start;
    let db = b.end - b.start;
    let denominator = da.cross(db);
    if denominator.abs() <= tolerance::ANGULAR * da.length() * db.length() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the lines are parallel: there is no angle",
        ));
    }
    let t = (b.start - a.start).cross(db) / denominator;
    let vertex = a.start + da * t;
    // Each ray runs to the line's end on the side `at` is: the end whose
    // direction from the vertex is nearer the direction to `at`.
    let ray_end = |line: &Segment2, id: EntityId| {
        let to_at = at - vertex;
        let start_score = (line.start - vertex).dot(to_at);
        let end_score = (line.end - vertex).dot(to_at);
        let use_end = end_score >= start_score;
        AnchoredPoint {
            point: if use_end { line.end } else { line.start },
            r#ref: AnchorRef {
                entity: id,
                point: if use_end {
                    AnchorPoint::End
                } else {
                    AnchorPoint::Start
                },
                ..AnchorRef::default()
            },
        }
    };
    angular_dimension(
        &AnchoredPoint {
            point: vertex,
            r#ref: AnchorRef::default(),
        },
        &ray_end(&a, first),
        &ray_end(&b, second),
        Some(at),
    )
}

/// Radius or diameter of a circle or an arc entity, through `at` (its
/// direction from the centre; the text sits beyond the curve by as much as
/// `at` is outside it). Follows the curve's centre and radius.
pub fn radial_dimension(
    model: &Model,
    curve: EntityId,
    at: Point2,
    diameter: bool,
) -> Result<DimensionGeometry> {
    let Some(entity) = model.entities.find(curve) else {
        return Err(make_error(
            ErrorCode::NotFound,
            "entity does not exist",
            id_context(curve),
        ));
    };
    let (centre, radius) = match &entity.geometry {
        Geometry::Circle(circle) => (circle.center, circle.radius),
        Geometry::Arc(arc) => (arc.center, arc.radius),
        _ => {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "a radius or diameter needs a circle or an arc",
                id_context(curve),
            ));
        }
    };
    let to_at = at - centre;
    let direction = if to_at.length() > tolerance::GEOMETRIC {
        to_at.normalized()
    } else {
        Vec2::new(1.0, 0.0)
    };
    checked(DimensionGeometry {
        kind: if diameter {
            DimensionKind::Diameter
        } else {
            DimensionKind::Radius
        },
        vertex: centre,
        vertex_ref: AnchorRef {
            entity: curve,
            point: AnchorPoint::Centre,
            ..AnchorRef::default()
        },
        start: centre + direction * radius,
        offset: cpp::max(0.0, to_at.length() - radius),
        ..DimensionGeometry::default()
    })
}

/// Ordinate of `feature` from `datum`, its leader ending at `at`. With no
/// axis given, a leader more vertical than horizontal gives the X ordinate
/// (the value written up or down the sheet is an easting) and otherwise the
/// Y, as AutoCAD's DIMORDINATE decides.
pub fn ordinate_dimension(
    datum: &AnchoredPoint,
    feature: &AnchoredPoint,
    at: Point2,
    x_axis: Option<bool>,
) -> Result<DimensionGeometry> {
    let leader = at - feature.point;
    let x = x_axis.unwrap_or(leader.y.abs() > leader.x.abs());
    checked(DimensionGeometry {
        kind: if x {
            DimensionKind::OrdinateX
        } else {
            DimensionKind::OrdinateY
        },
        vertex: datum.point,
        vertex_ref: datum.r#ref,
        start: feature.point,
        start_ref: feature.r#ref,
        end: at,
        ..DimensionGeometry::default()
    })
}

/// Baseline chain: from `base`'s first point to each of `points`, each
/// dimension line `spacing` further out than the one before (base first). For
/// Aligned and Linear bases.
pub fn baseline_dimensions(
    base: &DimensionGeometry,
    points: &[AnchoredPoint],
    spacing: f64,
) -> Result<Vec<DimensionGeometry>> {
    if base.kind != DimensionKind::Aligned && base.kind != DimensionKind::Linear {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "a baseline chain continues an aligned or linear dimension",
        ));
    }
    if !spacing.is_finite() || !(spacing > 0.0) {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the baseline spacing must be positive",
        ));
    }
    let mut chain = Vec::with_capacity(points.len());
    let outward = if base.offset >= 0.0 { 1.0 } else { -1.0 };
    let mut offset = base.offset;
    for point in points {
        offset += outward * spacing;
        let mut next = base.clone();
        next.end = point.point;
        next.end_ref = point.r#ref;
        next.text_override.clear();
        next.offset = offset;
        chain.push(checked(next)?);
    }
    Ok(chain)
}

/// Continued chain: end to end, from `base`'s second point, every dimension
/// line on base's. For Aligned and Linear bases.
pub fn continued_dimensions(
    base: &DimensionGeometry,
    points: &[AnchoredPoint],
) -> Result<Vec<DimensionGeometry>> {
    if base.kind != DimensionKind::Aligned && base.kind != DimensionKind::Linear {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "a continued chain continues an aligned or linear dimension",
        ));
    }
    let mut chain = Vec::with_capacity(points.len());
    let mut previous = base.clone();
    for point in points {
        let mut next = previous.clone();
        next.text_override.clear();
        next.start = previous.end;
        next.start_ref = previous.end_ref;
        next.end = point.point;
        next.end_ref = point.r#ref;
        if base.kind == DimensionKind::Linear {
            // On the previous dimension's line: that line is the points X
            // with (X - start) . n = offset, so from the new start it is at
            // offset - (new start - old start) . n.
            let normal = unit(base.angle).perpendicular();
            next.offset = previous.offset - (next.start - previous.start).dot(normal);
        } else {
            // An aligned chain turns with each span; each keeps the offset.
            next.offset = previous.offset;
        }
        let made = checked(next)?;
        chain.push(made.clone());
        previous = made;
    }
    Ok(chain)
}

/// Which chain dimension_chain makes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DimensionChain {
    #[default]
    Baseline,
    Continued,
}

/// The spacing a baseline chain from dimension entity `base` takes when none
/// is given: a text height and a half of the dimension style `base` is drawn
/// in, at 1 : `scale` - AutoCAD's metric DIMDLI of 3.75 against a DIMTXT of
/// 2.5.
pub fn baseline_spacing(model: &Model, base: &Entity, scale: f64) -> f64 {
    let style = dimension_style_at_scale(resolve_dimension_style(model, base), scale);
    1.5 * style.text_height
}

/// DIM BASELINE's and DIM CONTINUE's command, which the Baseline and Continue
/// Dimension tools make too, so the two cannot differ: the chain from the
/// aligned or linear dimension `base` through `points` (baseline_dimensions,
/// continued_dimensions) as ONE command, DIM_BASELINE or DIM_CONTINUE, each
/// dimension on the base's layer and in its style and colour. A baseline
/// chain's lines are `spacing` apart, or baseline_spacing's at 1 : `scale`
/// when none is given; a continued chain has no spacing. InvalidArgument when
/// `base` is not a dimension or there are no points, and as the chain's
/// builder refuses.
pub fn dimension_chain(
    model: &Model,
    base: EntityId,
    points: &[AnchoredPoint],
    kind: DimensionChain,
    spacing: Option<f64>,
    scale: f64,
) -> Result<CommandPtr> {
    let found = model.entities.find(base);
    let Some((entity, dimension)) = found.and_then(|entity| match &entity.geometry {
        Geometry::Dimension(dimension) => Some((entity, dimension)),
        _ => None,
    }) else {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "that entity is not a dimension",
            id_context(base),
        ));
    };
    if points.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "a chain needs at least one more point",
        ));
    }
    let chain = match kind {
        DimensionChain::Baseline => baseline_dimensions(
            dimension,
            points,
            spacing.unwrap_or_else(|| baseline_spacing(model, entity, scale)),
        )?,
        DimensionChain::Continued => continued_dimensions(dimension, points)?,
    };
    let mut changes = ChangeSet::default();
    for geometry in chain {
        changes.add.push(Entity {
            geometry: Geometry::Dimension(geometry),
            layer: entity.layer.clone(),
            style: entity.style.clone(),
            color: entity.color,
            ..Entity::default()
        });
    }
    let name = match kind {
        DimensionChain::Baseline => "DIM_BASELINE",
        DimensionChain::Continued => "DIM_CONTINUE",
    };
    Ok(Box::new(ChangeSetCommand::new(
        name,
        move |_context| Ok(changes.clone()),
        false,
    )))
}
