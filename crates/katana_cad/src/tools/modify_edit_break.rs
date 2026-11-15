//! Break and Break at Point (src/katana_cad/tools/modify_edit_break.cpp).
//! Break takes the object at the point it is picked - the first break point,
//! as in AutoCAD, or [First point] to give it exactly - and a second point,
//! and removes what lies between; typing @0,0 for the second point breaks at
//! the first without a gap. Break at Point splits a line, arc or open
//! polyline in two at one point; a circle or closed polyline has no ends, so
//! one point cannot split it in two, and it is refused when picked. Each is
//! ONE command: the object keeps its id for the first piece and the second
//! piece is new.

use katana_core::{Error, ErrorCode, Result};
use katana_entity::entity::INVALID_ENTITY_ID;
use katana_entity::entity_geometry::distance_to;
use katana_entity::{EntityId, EntityType, Geometry};
use katana_geometry::polyline_vertices::{sub_path, wrapping_path};
use katana_geometry::{Arc2, CurvePolyline2, Point2, Polyline2, Segment2};
use katana_math::cpp;
use katana_math::numerics::{TWO_PI, normalize_angle, tolerance};

use crate::drawing::vertex_editing::write_polyline;
use crate::interactive_tool::{InteractiveTool, ToolContext, ToolInput, ToolStep};
use crate::tool_feedback::ToolFeedback;
use crate::tools::modify_edit_support::{
    EditSession, PolylinePath, ToolPtr, as_sentence, base_entity, base_point, carry_heights,
    is_option, kind_name, no_drawing, refusal_to_edit, with_article,
};

#[derive(Default)]
struct Broken {
    pieces: Vec<Geometry>,
    /// For the preview.
    removed: Vec<Geometry>,
}

// The point of `geometry` nearest `p`; `p` itself when it is on it already,
// so a snapped point is kept to the last bit rather than re-projected.
fn onto(geometry: &Geometry, p: Point2) -> Point2 {
    if distance_to(geometry, p) <= tolerance::GEOMETRIC {
        return p;
    }
    match geometry {
        Geometry::Line(segment) => segment.closest_point(p),
        Geometry::Arc(arc) => arc.closest_point(p),
        Geometry::Circle(circle) => circle.closest_point(p),
        Geometry::Polyline(polyline) => polyline.closest_point(p).unwrap_or(p),
        Geometry::CurvePolyline(curved) => curved.closest_point(p).unwrap_or(p),
        _ => p,
    }
}

// Breaks an open curve measured by fractions or stations from 0 to `length`:
// `piece(from, to)` makes the part between two of them.
fn break_open(
    first: f64,
    second: f64,
    length: f64,
    kind: &str,
    piece: impl Fn(f64, f64) -> Geometry,
) -> Result<Broken> {
    let low = cpp::min(first, second);
    let high = cpp::max(first, second);
    let mut out = Broken::default();
    if high - low <= tolerance::GEOMETRIC {
        if low <= tolerance::GEOMETRIC || low >= length - tolerance::GEOMETRIC {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                format!(
                    "The break point is at an end of the {kind}, so there is nothing to split."
                ),
            ));
        }
        out.pieces = vec![piece(0.0, low), piece(low, length)];
        return Ok(out);
    }
    if low <= tolerance::GEOMETRIC && high >= length - tolerance::GEOMETRIC {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!("The two points take in the whole {kind}; use Erase to remove it."),
        ));
    }
    if low > tolerance::GEOMETRIC {
        out.pieces.push(piece(0.0, low));
    }
    if high < length - tolerance::GEOMETRIC {
        out.pieces.push(piece(high, length));
    }
    out.removed.push(piece(low, high));
    Ok(out)
}

// A polyline piece with the break points exactly as given where it ends at
// one.
fn exact_polyline(mut piece: Polyline2, from: f64, to: f64, ends: [(f64, Point2); 2]) -> Geometry {
    for (station, point) in ends {
        if (from - station).abs() <= tolerance::GEOMETRIC
            && let Some(first) = piece.vertices.first_mut()
        {
            *first = point;
        }
        if (to - station).abs() <= tolerance::GEOMETRIC
            && let Some(last) = piece.vertices.last_mut()
        {
            *last = point;
        }
    }
    Geometry::Polyline(piece)
}

// The same for a curve polyline piece.
fn exact_curved(
    mut piece: CurvePolyline2,
    from: f64,
    to: f64,
    ends: [(f64, Point2); 2],
) -> Geometry {
    for (station, point) in ends {
        if (from - station).abs() <= tolerance::GEOMETRIC
            && let Some(first) = piece.vertices.first_mut()
        {
            first.position = point;
        }
        if (to - station).abs() <= tolerance::GEOMETRIC
            && let Some(last) = piece.vertices.last_mut()
        {
            last.position = point;
        }
    }
    Geometry::CurvePolyline(piece)
}

// `a` and `b` are on the geometry already (onto()).
fn break_geometry(geometry: &Geometry, a: Point2, b: Point2) -> Result<Broken> {
    let kind = kind_name(geometry);
    match geometry {
        Geometry::Line(segment) => {
            // Measured in length, so "no gap" and "at an end" mean the same
            // distance on every line; the break points themselves go in as
            // given.
            let length = segment.length();
            let ta = segment.parameter_of(a) * length;
            let tb = segment.parameter_of(b) * length;
            let at = |s: f64| {
                if (s - ta).abs() <= tolerance::GEOMETRIC {
                    return a;
                }
                if (s - tb).abs() <= tolerance::GEOMETRIC {
                    return b;
                }
                if s <= 0.0 { segment.start } else { segment.end }
            };
            break_open(ta, tb, length, kind, |from, to| {
                Geometry::Line(Segment2::new(at(from), at(to)))
            })
        }
        Geometry::Arc(arc) => {
            let length = arc.length();
            let sa = arc.parameter_of_angle((a - arc.center).angle()) * length;
            let sb = arc.parameter_of_angle((b - arc.center).angle()) * length;
            break_open(sa, sb, length, kind, |from, to| {
                Geometry::Arc(Arc2::new(
                    arc.center,
                    arc.radius,
                    arc.start_angle + arc.sweep * (from / length),
                    arc.sweep * ((to - from) / length),
                ))
            })
        }
        Geometry::Circle(circle) => {
            // AutoCAD's rule: the part removed runs counter-clockwise from the
            // first point to the second.
            let from = normalize_angle((a - circle.center).angle());
            let to = normalize_angle((b - circle.center).angle());
            let gap = normalize_angle(to - from);
            if gap * circle.radius <= tolerance::GEOMETRIC
                || (TWO_PI - gap) * circle.radius <= tolerance::GEOMETRIC
            {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    "A circle cannot be broken at a single point; pick two points.",
                ));
            }
            Ok(Broken {
                pieces: vec![Geometry::Arc(Arc2::new(
                    circle.center,
                    circle.radius,
                    to,
                    TWO_PI - gap,
                ))],
                removed: vec![Geometry::Arc(Arc2::new(
                    circle.center,
                    circle.radius,
                    from,
                    gap,
                ))],
            })
        }
        Geometry::Polyline(polyline) => {
            let path = PolylinePath::new(polyline.clone());
            let sa = path.station_of(a);
            let sb = path.station_of(b);
            // The break points exactly as given where a piece ends at one.
            let ends = [(sa, a), (sb, b)];
            if !path.closed() {
                return break_open(sa, sb, path.length(), kind, |from, to| {
                    exact_polyline(path.between(from, to), from, to, ends)
                });
            }
            // Closed: the part removed runs from the first point to the second
            // in the direction the polyline was drawn, and what is left is one
            // open polyline from the second point round to the first.
            if (sa - sb).abs() <= tolerance::GEOMETRIC
                || ((sa - sb).abs() - path.length()).abs() <= tolerance::GEOMETRIC
            {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    "A closed polyline cannot be broken at a single point; pick two points.",
                ));
            }
            Ok(if sa < sb {
                Broken {
                    pieces: vec![exact_polyline(path.wrapping(sb, sa), sb, sa, ends)],
                    removed: vec![exact_polyline(path.between(sa, sb), sa, sb, ends)],
                }
            } else {
                Broken {
                    pieces: vec![exact_polyline(path.between(sb, sa), sb, sa, ends)],
                    removed: vec![exact_polyline(path.wrapping(sa, sb), sa, sb, ends)],
                }
            })
        }
        Geometry::CurvePolyline(curved) => {
            // As a straight polyline, measured along its arcs (geometry's
            // sub_path); the pieces are curve polylines, stored by the drawing
            // system's rule when the tool makes them entities.
            let sa = curved.nearest(a).map_or(0.0, |nearest| nearest.station);
            let sb = curved.nearest(b).map_or(0.0, |nearest| nearest.station);
            let length = curved.length();
            let ends = [(sa, a), (sb, b)];
            if !curved.closed {
                return break_open(sa, sb, length, kind, |from, to| {
                    exact_curved(sub_path(curved, from, to), from, to, ends)
                });
            }
            if (sa - sb).abs() <= tolerance::GEOMETRIC
                || ((sa - sb).abs() - length).abs() <= tolerance::GEOMETRIC
            {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    "A closed polyline cannot be broken at a single point; pick two points.",
                ));
            }
            Ok(if sa < sb {
                Broken {
                    pieces: vec![exact_curved(wrapping_path(curved, sb, sa), sb, sa, ends)],
                    removed: vec![exact_curved(sub_path(curved, sa, sb), sa, sb, ends)],
                }
            } else {
                Broken {
                    pieces: vec![exact_curved(sub_path(curved, sb, sa), sb, sa, ends)],
                    removed: vec![exact_curved(wrapping_path(curved, sa, sb), sa, sb, ends)],
                }
            })
        }
        _ => Err(Error::new(
            ErrorCode::Unsupported,
            format!("A {kind} cannot be broken; pick a line, arc, circle or polyline."),
        )),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Object,
    First,
    Second,
}

struct BreakTool {
    at_point: bool,
    session: EditSession,
    step: Step,
    target: EntityId,
    first: Point2,
}

impl BreakTool {
    fn new(at_point: bool) -> Self {
        Self {
            at_point,
            session: EditSession::new(),
            step: Step::Object,
            target: INVALID_ENTITY_ID,
            first: Point2::default(),
        }
    }
}

impl InteractiveTool for BreakTool {
    fn prompt(&self, _cx: &ToolContext<'_>) -> String {
        String::from(match self.step {
            Step::Object => "Select object to break",
            Step::First => "Specify first break point",
            Step::Second => {
                if self.at_point {
                    "Specify break point"
                } else {
                    "Specify second break point or [First point]"
                }
            }
        })
    }

    fn expects(&self, _cx: &ToolContext<'_>) -> ToolInput {
        if self.step == Step::Object {
            ToolInput::Entity
        } else {
            ToolInput::Point
        }
    }

    fn entity(&mut self, cx: &ToolContext<'_>, id: EntityId, at: Point2) -> ToolStep {
        if self.step != Step::Object {
            return base_entity(cx, self.expects(cx), id, at);
        }
        let Some(document) = cx.document else {
            return no_drawing();
        };
        if let Some(refusal) = refusal_to_edit(document, id) {
            return ToolStep::rejected(refusal);
        }
        let Some(entity) = self.session.original(document, id) else {
            return ToolStep::rejected("That object is no longer in the drawing.");
        };
        let entity_type = entity.r#type();
        if entity_type != EntityType::Line
            && entity_type != EntityType::Arc
            && entity_type != EntityType::Circle
            && entity_type != EntityType::Polyline
            && entity_type != EntityType::CurvePolyline
        {
            return ToolStep::rejected(format!(
                "A {} cannot be broken; pick a line, arc, circle or polyline.",
                kind_name(&entity.geometry)
            ));
        }
        let closed = match &entity.geometry {
            Geometry::Polyline(polyline) => polyline.closed,
            Geometry::CurvePolyline(curved) => curved.closed,
            _ => false,
        };
        if self.at_point && (entity_type == EntityType::Circle || closed) {
            // Refused at the pick: every point after it would be refused as
            // "one point", and a one-point tool cannot take a second.
            return ToolStep::rejected(as_sentence(&format!(
                "{} has no ends, so one point cannot split it; use Break with two points",
                with_article(kind_name(&entity.geometry))
            )));
        }
        self.target = id;
        self.first = onto(&entity.geometry, at);
        self.step = Step::Second;
        ToolStep::next("")
    }

    fn point(&mut self, cx: &ToolContext<'_>, at: Point2) -> ToolStep {
        if self.step == Step::Object {
            return base_point(cx, self.expects(cx), at);
        }
        let Some(document) = cx.document else {
            return no_drawing();
        };
        let Some(original) = self.session.original(document, self.target) else {
            return ToolStep::rejected("That object is no longer in the drawing.");
        };
        if self.step == Step::First {
            self.first = onto(&original.geometry, at);
            self.step = Step::Second;
            return ToolStep::next("");
        }
        let second = onto(&original.geometry, at);
        let broken = match break_geometry(
            &original.geometry,
            if self.at_point { second } else { self.first },
            second,
        ) {
            Ok(broken) => broken,
            Err(error) => return ToolStep::rejected(error.message),
        };
        let count = broken.pieces.len();
        let mut pieces = Vec::new();
        for geometry in broken.pieces {
            if let Geometry::CurvePolyline(curved) = &geometry {
                // Its heights are in the geometry; stored as the simplest
                // kind.
                match write_polyline(original.clone(), curved) {
                    Ok(piece) => pieces.push(piece),
                    Err(error) => return ToolStep::rejected(as_sentence(&error.message)),
                }
                continue;
            }
            let mut piece = original.clone();
            piece.geometry = geometry;
            carry_heights(original, &mut piece);
            pieces.push(piece);
        }
        self.session.begin();
        if let Err(error) = self.session.replace(self.target, pieces) {
            return ToolStep::rejected(error.message);
        }
        ToolStep::done(
            Some(self.session.commit("BREAK")),
            if count == 2 {
                "Broken in two."
            } else {
                "Broken."
            },
            false,
        )
    }

    fn value(&mut self, cx: &ToolContext<'_>, text: &str) -> ToolStep {
        if is_option(text, "Undo", "U") {
            return self.undo(cx);
        }
        if self.step == Step::Second && !self.at_point && is_option(text, "First", "F") {
            self.step = Step::First;
            return ToolStep::next("");
        }
        ToolStep::rejected(format!("'{text}' is not an option here."))
    }

    fn undo(&mut self, _cx: &ToolContext<'_>) -> ToolStep {
        match self.step {
            Step::Object => ToolStep::rejected("Nothing to undo."),
            Step::First => {
                self.step = Step::Second;
                ToolStep::next("")
            }
            Step::Second => {
                self.step = Step::Object;
                ToolStep::next("")
            }
        }
    }

    fn preview(&self, cx: &ToolContext<'_>, cursor: Point2) -> ToolFeedback {
        let mut feedback = ToolFeedback::default();
        if self.step == Step::Object {
            return feedback;
        }
        let Some(original) = cx
            .document
            .and_then(|document| self.session.original(document, self.target))
        else {
            return feedback;
        };
        let geometry = &original.geometry;
        let here = onto(geometry, cursor);
        if self.step == Step::First || self.at_point {
            feedback.markers.push(here);
            return feedback;
        }
        feedback.markers = vec![self.first, here];
        if let Ok(broken) = break_geometry(geometry, self.first, here) {
            feedback.shapes = broken.removed;
        }
        feedback
    }

    // The first break point, so "@0,0" breaks there and "@2<0" measures from
    // it.
    fn last_point(&self, _cx: &ToolContext<'_>) -> Option<Point2> {
        if self.step == Step::Second && !self.at_point {
            Some(self.first)
        } else {
            None
        }
    }
}

pub fn make_break_tool() -> ToolPtr {
    Box::new(BreakTool::new(false))
}

pub fn make_break_at_point_tool() -> ToolPtr {
    Box::new(BreakTool::new(true))
}
