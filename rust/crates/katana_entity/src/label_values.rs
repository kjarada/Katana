//! What a label says, and about which piece of which geometry: a label and
//! its style turned into pieces - one per point, segment, arc, area or
//! chainage mark - each with its template values and the geometry the placer
//! places it against (docs/annotation.md, "Labels").
//!
//! In the entity layer, next to the template language, because the values
//! are the model's own numbers and every consumer - the painters through the
//! placer, the command line's LABEL LIST, an exporter writing labels as text -
//! must read the same ones.

use katana_geometry::alignment::solve_alignment;
use katana_geometry::curves2d::CURVE_CHORD_TOLERANCE;
use katana_geometry::{Arc2, CurvePolyline2, CurveSegment, Point2, Vec2};
use katana_math::numerics::{PI, TWO_PI, tolerance};

use crate::anchor::inside_point;
use crate::annotation::{LabelKind, LabelStyle};
use crate::entity::{Entity, Geometry, LabelGeometry, heights_of};
use crate::label_text::{LabelQuantity, LabelValue, LabelValues};
use crate::model::Model;
use crate::tables::Alignment;
use crate::value_parts::{add_arc_values, add_common_values, add_segment_values};
use katana_math::cpp;

/// The C++ `LabelPiece::Shape`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LabelPieceShape {
    /// A point: the anchor is the point.
    #[default]
    Point,
    /// A straight piece from `from` to `to`; the anchor is its middle.
    Segment,
    /// `arc`; the anchor is its middle.
    Arc,
    /// A closed figure; the anchor is a point inside it.
    Area,
    /// A chainage on an alignment; the anchor is on the line.
    Station,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LabelPiece {
    pub shape: LabelPieceShape,
    pub values: LabelValues,
    pub anchor: Point2,
    /// The direction of the line at the anchor, radians counter-clockwise from
    /// east: a segment's, an arc's tangent (in its direction of sweep), the
    /// alignment's. 0 for a point and an area.
    pub direction: f64,
    /// The piece's length in model units: a segment's, an arc's; 0 otherwise.
    pub length: f64,
    pub from: Point2,
    pub to: Point2,
    pub arc: Arc2,
    /// A chainage tick with no text (LabelStyle::tick_interval between labels).
    pub tick_only: bool,
}

fn point_piece(entity: &Entity, position: Point2, codes: &[String]) -> LabelPiece {
    let mut piece = LabelPiece {
        shape: LabelPieceShape::Point,
        anchor: position,
        ..LabelPiece::default()
    };
    add_common_values(entity, codes, &mut piece.values);
    for (name, value) in [
        ("x", position.x),
        ("easting", position.x),
        ("y", position.y),
        ("northing", position.y),
    ] {
        piece.values.insert(
            String::from(name),
            LabelValue::of(LabelQuantity::Length, value),
        );
    }
    if let Some(z) = heights_of(&entity.properties, 1)[0] {
        piece
            .values
            .insert(String::from("z"), LabelValue::of(LabelQuantity::Number, z));
        piece
            .values
            .insert(String::from("rl"), LabelValue::of(LabelQuantity::Number, z));
    }
    piece
}

fn segment_piece(
    entity: &Entity,
    from: Point2,
    to: Point2,
    index: usize,
    z_from: Option<f64>,
    z_to: Option<f64>,
    codes: &[String],
) -> LabelPiece {
    let along = to - from;
    let mut piece = LabelPiece {
        shape: LabelPieceShape::Segment,
        from,
        to,
        anchor: from + (to - from) * 0.5,
        direction: along.angle(),
        length: along.length(),
        ..LabelPiece::default()
    };
    add_common_values(entity, codes, &mut piece.values);
    add_segment_values(&mut piece.values, from, to, index, z_from, z_to);
    piece
}

fn arc_piece(entity: &Entity, arc: &Arc2, codes: &[String]) -> LabelPiece {
    let anchor = arc.point_at(0.5);
    let radial = anchor - arc.center;
    // The tangent in the direction of sweep.
    let tangent = if arc.sweep >= 0.0 {
        radial.perpendicular()
    } else {
        -radial.perpendicular()
    };
    let mut piece = LabelPiece {
        shape: LabelPieceShape::Arc,
        arc: *arc,
        anchor,
        direction: tangent.angle(),
        length: arc.length(),
        from: arc.point_at(0.0),
        to: arc.point_at(1.0),
        ..LabelPiece::default()
    };
    add_common_values(entity, codes, &mut piece.values);
    add_arc_values(&mut piece.values, arc);
    piece
}

fn area_piece(
    entity: &Entity,
    inside: Point2,
    area: f64,
    perimeter: f64,
    codes: &[String],
) -> LabelPiece {
    let mut piece = LabelPiece {
        shape: LabelPieceShape::Area,
        anchor: inside,
        ..LabelPiece::default()
    };
    add_common_values(entity, codes, &mut piece.values);
    for (name, value) in [
        ("area", LabelValue::of(LabelQuantity::Area, area)),
        (
            "perimeter",
            LabelValue::of(LabelQuantity::Length, perimeter),
        ),
        ("x", LabelValue::of(LabelQuantity::Length, inside.x)),
        ("y", LabelValue::of(LabelQuantity::Length, inside.y)),
    ] {
        piece.values.insert(String::from(name), value);
    }
    piece
}

/// A chainage label's pieces: a mark at every tick and every label station
/// from the first multiple of the tick interval at or after the start, through
/// the end; the label stations carry text.
fn chainage_pieces(alignment: &Alignment, style: &LabelStyle) -> Vec<LabelPiece> {
    let mut pieces = Vec::new();
    let Ok(solved) = solve_alignment(&alignment.horizontal) else {
        return pieces;
    };
    let start = solved.start_station();
    let end = solved.end_station();
    let interval = style.interval;
    let step = if style.tick_interval > tolerance::GEOMETRIC {
        cpp::min(style.tick_interval, interval)
    } else {
        interval
    };
    // Stations are counted as whole multiples of the step, never
    // accumulated, so the 500th tick of a 10 km road is exactly at 5000 and
    // not wherever 500 additions of 10.0 land. A station that is a label
    // station within a part in a million of the interval is one - 10 x 0.1 is
    // not exactly 1.
    const MAXIMUM_MARKS: usize = 100_000;
    let first_index = (start / step - 1e-9).ceil();
    let mut k = first_index;
    loop {
        let station = k * step;
        if station > end + tolerance::GEOMETRIC || pieces.len() >= MAXIMUM_MARKS {
            break;
        }
        let at = cpp::clamp(station, start, end);
        let point = solved.point_at_station(at);
        let direction = solved.direction_at_station(at);
        if let (Some(point), Some(direction)) = (point, direction) {
            let ratio = station / interval;
            let mut piece = LabelPiece {
                shape: LabelPieceShape::Station,
                anchor: point,
                direction,
                tick_only: (ratio - ratio.round()).abs() > 1e-6,
                ..LabelPiece::default()
            };
            for (name, value) in [
                ("chainage", LabelValue::of(LabelQuantity::Chainage, station)),
                ("x", LabelValue::of(LabelQuantity::Length, point.x)),
                ("y", LabelValue::of(LabelQuantity::Length, point.y)),
                ("alignment", LabelValue::of_text(alignment.name.clone())),
            ] {
                piece.values.insert(String::from(name), value);
            }
            pieces.push(piece);
        }
        k += 1.0;
    }
    pieces
}

/// The segments of a curve polyline a label of `kind` labels: every one for a
/// segment label (an arc segment as an arc, a straight one as a segment), the
/// arc segments for an arc label; only `part` when it names one.
fn curve_segments_labelled(curve: &CurvePolyline2, kind: LabelKind, part: i32) -> Vec<usize> {
    let mut out = Vec::new();
    for i in 0..curve.segment_count() {
        if (part >= 0 && part as usize != i)
            || (kind == LabelKind::Arc && !curve.is_arc(i))
            || curve.segment_length(i) <= tolerance::GEOMETRIC
        {
            continue;
        }
        out.push(i);
    }
    out
}

/// Whether a label style of `kind` can label `geometry` at all.
pub fn labels(kind: LabelKind, geometry: &Geometry) -> bool {
    // A curve polyline is labelled as a polyline is - its arc segments as
    // arcs - so a lot drawn with a curved frontage labels as a straight one
    // did.
    match kind {
        LabelKind::Point => matches!(geometry, Geometry::Point(_)),
        LabelKind::Segment => match geometry {
            Geometry::Line(_) | Geometry::Polyline(_) => true,
            Geometry::CurvePolyline(curve) => curve.segment_count() > 0,
            _ => false,
        },
        LabelKind::Arc => match geometry {
            Geometry::Arc(_) | Geometry::Circle(_) => true,
            Geometry::CurvePolyline(curve) => curve.has_arcs(),
            _ => false,
        },
        LabelKind::Area => match geometry {
            Geometry::Polyline(polyline) => polyline.closed && polyline.vertices.len() >= 3,
            Geometry::CurvePolyline(curve) => curve.closed && curve.vertices.len() >= 3,
            Geometry::Circle(_) => true,
            _ => false,
        },
        LabelKind::Chainage => false,
    }
}

/// The pieces of a label of `style` on `target`, a target that is not (yet) a
/// label entity.
pub fn label_pieces_for(
    model: &Model,
    target: &Entity,
    part: i32,
    style: &LabelStyle,
    codes: &[String],
) -> Vec<LabelPiece> {
    // The entity is all a piece needs; the model is for a chainage's alignment.
    let _ = model;
    let mut pieces = Vec::new();
    if !labels(style.kind, &target.geometry) {
        return pieces;
    }
    match style.kind {
        LabelKind::Point => {
            if let Geometry::Point(point) = &target.geometry {
                pieces.push(point_piece(target, point.position, codes));
            }
        }
        LabelKind::Segment | LabelKind::Arc => {
            if let Geometry::CurvePolyline(curve) = &target.geometry {
                let v = &curve.vertices;
                for i in curve_segments_labelled(curve, style.kind, part) {
                    let j = curve.segment_end(i);
                    match curve.segment(i) {
                        CurveSegment::Arc(arc) => pieces.push(arc_piece(target, &arc, codes)),
                        CurveSegment::Segment(_) => pieces.push(segment_piece(
                            target,
                            v[i].position,
                            v[j].position,
                            i,
                            v[i].height,
                            v[j].height,
                            codes,
                        )),
                    }
                }
                return pieces;
            }
            if style.kind == LabelKind::Arc {
                match &target.geometry {
                    Geometry::Arc(arc) => pieces.push(arc_piece(target, arc, codes)),
                    Geometry::Circle(circle) => {
                        // A circle is labelled at its top, as a full-turn arc
                        // from north.
                        pieces.push(arc_piece(
                            target,
                            &Arc2::new(circle.center, circle.radius, -0.5 * PI, TWO_PI),
                            codes,
                        ));
                    }
                    _ => {}
                }
                return pieces;
            }
            match &target.geometry {
                Geometry::Line(line) => {
                    let z = heights_of(&target.properties, 2);
                    pieces.push(segment_piece(
                        target, line.start, line.end, 0, z[0], z[1], codes,
                    ));
                }
                Geometry::Polyline(polyline) => {
                    let v = &polyline.vertices;
                    // Wrapping as the C++ size_t does: a polyline with no
                    // vertex cannot reach the model (docs/rust.md "Not done").
                    let segments = if polyline.closed {
                        v.len()
                    } else {
                        v.len().wrapping_sub(1)
                    };
                    let z = heights_of(&target.properties, v.len());
                    for i in 0..segments {
                        if part >= 0 && part as usize != i {
                            continue;
                        }
                        let j = (i + 1) % v.len();
                        if v[i].distance_to(v[j]) <= tolerance::GEOMETRIC {
                            continue; // a repeated vertex has no direction to label
                        }
                        pieces.push(segment_piece(target, v[i], v[j], i, z[i], z[j], codes));
                    }
                }
                _ => {}
            }
        }
        LabelKind::Area => match &target.geometry {
            Geometry::Polyline(polyline) => pieces.push(area_piece(
                target,
                inside_point(polyline),
                polyline.area(),
                polyline.length(),
                codes,
            )),
            // The area and perimeter exact, arcs included; only where the
            // label stands comes from the chords.
            Geometry::CurvePolyline(curve) => pieces.push(area_piece(
                target,
                inside_point(&curve.to_polyline(CURVE_CHORD_TOLERANCE)),
                curve.area(),
                curve.length(),
                codes,
            )),
            Geometry::Circle(circle) => pieces.push(area_piece(
                target,
                circle.center,
                circle.area(),
                circle.perimeter(),
                codes,
            )),
            _ => {}
        },
        LabelKind::Chainage => {}
    }
    pieces
}

/// The pieces `label` makes under `style` from the model as it is now. Empty
/// when the target is gone, is not a kind of entity the style's kind labels,
/// or has no such part - the painter then draws nothing for it and counts it,
/// rather than inventing a place.
///
/// Every piece also has id, layer and, when the target carries them, point,
/// description and prop.NAME for each property, and `code`: the first of
/// `codes` the target carries, in its properties or its metadata. A chainage
/// label has only its own values.
pub fn label_pieces(
    model: &Model,
    label: &LabelGeometry,
    style: &LabelStyle,
    codes: &[String],
) -> Vec<LabelPiece> {
    if style.kind == LabelKind::Chainage {
        if label.alignment.is_empty() {
            return Vec::new();
        }
        return match model.alignments.find(&label.alignment) {
            Some(alignment) => chainage_pieces(alignment, style),
            None => Vec::new(),
        };
    }
    match model.entities.find(label.target) {
        Some(target) => label_pieces_for(model, target, label.part, style, codes),
        None => Vec::new(),
    }
}

/// Where a new label of `style` on `target` attaches (LabelGeometry::anchor):
/// the first piece's anchor, or the target's first point when there is none.
pub fn label_anchor(model: &Model, label: &LabelGeometry, style: &LabelStyle) -> Option<Point2> {
    // Worked out directly rather than from label_pieces: the associative
    // update asks this of every label after every command, and a piece
    // carries a map of values it does not need.
    if style.kind == LabelKind::Chainage {
        let alignment = model.alignments.find(&label.alignment)?;
        return alignment.horizontal.pis.first().map(|pi| pi.point);
    }
    let target = model.entities.find(label.target)?;
    if !labels(style.kind, &target.geometry) {
        return None;
    }
    let geometry = &target.geometry;
    if let Geometry::CurvePolyline(curve) = geometry {
        if style.kind == LabelKind::Area {
            return Some(inside_point(&curve.to_polyline(CURVE_CHORD_TOLERANCE)));
        }
        let segments = curve_segments_labelled(curve, style.kind, label.part);
        let first = *segments.first()?;
        return Some(curve.segment(first).point_at(0.5));
    }
    match style.kind {
        LabelKind::Point => match geometry {
            Geometry::Point(point) => Some(point.position),
            _ => None,
        },
        LabelKind::Segment => match geometry {
            Geometry::Line(line) => Some(line.start + (line.end - line.start) * 0.5),
            Geometry::Polyline(polyline) => {
                let v = &polyline.vertices;
                let segments = if polyline.closed {
                    v.len()
                } else {
                    v.len().wrapping_sub(1)
                };
                let i = if label.part >= 0 {
                    label.part as usize
                } else {
                    0
                };
                if i >= segments {
                    return None;
                }
                Some(v[i] + (v[(i + 1) % v.len()] - v[i]) * 0.5)
            }
            _ => None,
        },
        LabelKind::Arc => match geometry {
            Geometry::Arc(arc) => Some(arc.point_at(0.5)),
            Geometry::Circle(circle) => Some(circle.center + Vec2::new(0.0, circle.radius)),
            _ => None,
        },
        LabelKind::Area => match geometry {
            Geometry::Polyline(polyline) => Some(inside_point(polyline)),
            Geometry::Circle(circle) => Some(circle.center),
            _ => None,
        },
        LabelKind::Chainage => None,
    }
}
