//! Offset (src/katana_cad/tools/modify_edit_offset.cpp): a distance (typed,
//! measured between two points, or Through a point), then an object and the
//! side to put the copy on - as many as wanted, Enter to finish. [Multiple]
//! keeps offsetting the copy just made, which is how a set of parallel lines
//! is drawn in one go.

use katana_core::{Error, ErrorCode, Result};
use katana_entity::entity::{heights_of, set_heights};
use katana_entity::{Entity, EntityId, EntityType, Geometry};
use katana_geometry::editing::{offset, offset_towards};
use katana_geometry::{Curve2, CurveSegment, Line2, Point2, Polyline2, Segment2};
use katana_math::numerics::tolerance;

use crate::interactive_tool::{InteractiveTool, ToolContext, ToolInput, ToolStep};
use crate::tool_feedback::ToolFeedback;
use crate::tools::families::keep_work_on_escape;
use crate::tools::modify_edit_support::{
    Defaults, EditSession, ToolPtr, as_curve, as_geometry, as_sentence, base_entity, base_point,
    drawable, format_number, height_vertices, is_option, kind_name, no_drawing, parse_number,
    refusal_to_edit,
};

// The line a segment lies on.
fn carrier(segment: &Segment2) -> Line2 {
    Line2 {
        origin: segment.start,
        direction: segment.delta(),
    }
}

// The segment of `polyline` nearest to `p` that has a direction, or None.
fn nearest_segment(polyline: &Polyline2, p: Point2) -> Option<Segment2> {
    let mut best = None;
    let mut best_distance = f64::INFINITY;
    for i in 0..polyline.segment_count() {
        let segment = polyline.segment(i);
        let d = segment.distance_to(p);
        if !segment.is_degenerate() && d < best_distance {
            best_distance = d;
            best = Some(segment);
        }
    }
    best
}

// How far `side` is from `source` the way an offset measures it - from the
// line a segment lies on, from an arc's or circle's circumference - so that a
// Through offset passes exactly through the point.
fn through_distance(source: &Geometry, side: Point2) -> Option<f64> {
    match source {
        Geometry::Line(segment) => Some(carrier(segment).distance_to(side)),
        Geometry::Arc(arc) => Some((arc.center.distance_to(side) - arc.radius).abs()),
        Geometry::Circle(circle) => Some((circle.center.distance_to(side) - circle.radius).abs()),
        Geometry::Polyline(polyline) => {
            nearest_segment(polyline, side).map(|segment| carrier(&segment).distance_to(side))
        }
        Geometry::CurvePolyline(curved) => {
            let near = curved.nearest(side)?;
            Some(match curved.segment(near.segment) {
                CurveSegment::Segment(line) => carrier(&line).distance_to(side),
                CurveSegment::Arc(arc) => (arc.center.distance_to(side) - arc.radius).abs(),
            })
        }
        _ => None,
    }
}

// True when every side of `moved` still runs the way its side of `source`
// does. geometry's offset mitres each vertex and is only right while the
// distance is below the polyline's local feature size (editing.rs): beyond it
// the moved sides pass each other, and a mitre lands past the far end of a
// short side, which then runs backwards - an inward offset of an 8x8 square
// by 5 comes out as a 2x2 square drawn the other way round, when no such
// offset exists. Each side of the result lies on its source side's line
// moved sideways, so it can only run the same way, shrink to nothing or run
// back. geometry's offset works from the polyline without repeated vertices,
// so its sides are compared with those.
fn keeps_its_sides(source: &Polyline2, moved: &Polyline2) -> bool {
    let clean = source.without_duplicate_vertices();
    if clean.segment_count() != moved.segment_count() {
        return false;
    }
    for i in 0..clean.segment_count() {
        let side = moved.segment(i);
        if side.is_degenerate() || side.delta().dot(clean.segment(i).delta()) <= 0.0 {
            return false;
        }
    }
    true
}

// The offset of `source` by `distance` towards `side`. A polyline takes the
// side of its segment nearest the point, as offset_entity decides it.
fn offset_geometry(source: &Geometry, distance: f64, side: Point2) -> Result<Geometry> {
    match source {
        Geometry::Polyline(polyline) => {
            let Some(segment) = nearest_segment(polyline, side) else {
                return Err(Error::new(
                    ErrorCode::InvalidGeometry,
                    "That polyline has no length to offset.",
                ));
            };
            let sign = if carrier(&segment).signed_distance_to(side) >= 0.0 {
                1.0
            } else {
                -1.0
            };
            let moved = offset(polyline, sign * distance).map_err(|error| {
                Error::new(ErrorCode::InvalidGeometry, as_sentence(&error.message))
            })?;
            if !keeps_its_sides(polyline, &moved) {
                return Err(Error::new(
                    ErrorCode::InvalidGeometry,
                    format!(
                        "An offset of {} is too large for that {}: a side of the copy would \
                         turn back on itself or shrink to nothing.",
                        format_number(distance),
                        kind_name(source)
                    ),
                ));
            }
            Ok(Geometry::Polyline(moved))
        }
        Geometry::CurvePolyline(curved) => {
            // The side of the nearest piece decides: left of a line, inside
            // a counter-clockwise arc (geometry's offset's positive side).
            let Some(near) = curved.nearest(side) else {
                return Err(Error::new(
                    ErrorCode::InvalidGeometry,
                    "That polyline has no length to offset.",
                ));
            };
            let sign = match curved.segment(near.segment) {
                CurveSegment::Segment(line) => {
                    if carrier(&line).signed_distance_to(side) >= 0.0 {
                        1.0
                    } else {
                        -1.0
                    }
                }
                CurveSegment::Arc(arc) => {
                    if (side.distance_to(arc.center) < arc.radius) == (arc.sweep > 0.0) {
                        1.0
                    } else {
                        -1.0
                    }
                }
            };
            let moved = offset(curved, sign * distance).map_err(|error| {
                Error::new(ErrorCode::InvalidGeometry, as_sentence(&error.message))
            })?;
            Ok(Geometry::CurvePolyline(moved))
        }
        _ => {
            let Some(curve) = as_curve(source) else {
                return Err(Error::new(
                    ErrorCode::Unsupported,
                    "Only lines, arcs, circles and polylines offset.",
                ));
            };
            match offset_towards(&curve, distance, side) {
                Ok(moved) => Ok(as_geometry(&moved)),
                Err(error) => {
                    if matches!(curve, Curve2::Circle(_) | Curve2::Arc(_)) {
                        return Err(Error::new(
                            ErrorCode::InvalidGeometry,
                            format!(
                                "The distance is not smaller than the radius, so there is no {} \
                                 on that side.",
                                kind_name(source)
                            ),
                        ));
                    }
                    Err(Error::new(
                        ErrorCode::InvalidGeometry,
                        as_sentence(&error.message),
                    ))
                }
            }
        }
    }
}

// An offset copies its source, as offset_entity and AutoCAD's default
// (Layer = Source) make it: the same layer, style, colour and properties. Its
// vertices are the source's moved sideways, so each keeps its height - unless
// the offset dropped a repeated vertex, when only a level string's single
// height still describes it.
fn offset_copy(source: &Entity, geometry: Geometry) -> Entity {
    let mut copy = source.clone();
    copy.geometry = geometry;
    let before = height_vertices(&source.geometry).len();
    let after = height_vertices(&copy.geometry).len();
    if before != after {
        let heights = heights_of(&source.properties, before);
        let level = !heights.is_empty() && heights.iter().all(|height| *height == heights[0]);
        let height = if level { heights[0] } else { None };
        set_heights(&mut copy.properties, &vec![height; after]);
    }
    copy
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Distance,
    SecondPoint,
    Source,
    Side,
}

struct OffsetTool {
    defaults: Defaults,
    session: EditSession,
    step: Step,
    distance: f64,
    through: bool,
    multiple: bool,
    first: Option<Point2>,
    /// The object being offset and, in Multiple, every copy made from it
    /// since: the last is the next source.
    chain: Vec<Entity>,
}

impl OffsetTool {
    fn choose_distance(&mut self, distance: f64) -> ToolStep {
        self.distance = distance;
        self.through = false;
        self.defaults.borrow_mut().offset_distance = Some(distance);
        self.first = None;
        self.step = Step::Source;
        ToolStep::next("")
    }

    fn attempt(&self, side: Point2) -> Result<Entity> {
        let Some(source) = self.chain.last() else {
            return Err(Error::new(
                ErrorCode::InvalidState,
                "There is no object to offset.",
            ));
        };
        let mut distance = self.distance;
        if self.through {
            let measured = through_distance(&source.geometry, side);
            let Some(measured) = measured.filter(|&measured| measured > tolerance::GEOMETRIC)
            else {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    format!(
                        "The through point lies on the {}; pick a point off it.",
                        kind_name(&source.geometry)
                    ),
                ));
            };
            distance = measured;
        }
        let moved = offset_geometry(&source.geometry, distance, side)?;
        // Refused here rather than at Enter, where the drawing would refuse
        // the session's one command and every offset made in it with this.
        drawable(&moved)?;
        Ok(offset_copy(source, moved))
    }

    fn offset_towards(&mut self, side: Point2) -> ToolStep {
        let made = match self.attempt(side) {
            Ok(made) => made,
            Err(error) => return ToolStep::rejected(error.message),
        };
        self.session.begin();
        if let Err(error) = self.session.add(made.clone()) {
            return ToolStep::rejected(error.message);
        }
        if self.multiple {
            self.chain.push(made);
            return ToolStep::next("");
        }
        self.chain.clear();
        self.step = Step::Source;
        ToolStep::next("")
    }
}

impl InteractiveTool for OffsetTool {
    fn prompt(&self, _cx: &ToolContext<'_>) -> String {
        match self.step {
            Step::Distance => format!(
                "Specify offset distance or [Through] <{}>",
                match self.defaults.borrow().offset_distance {
                    Some(distance) => format_number(distance),
                    None => String::from("Through"),
                }
            ),
            Step::SecondPoint => String::from("Specify second point"),
            Step::Source => String::from("Select object to offset or [Exit/Undo]"),
            Step::Side => String::from(match (self.multiple, self.through) {
                (true, true) => "Specify through point for the next offset or [Undo]",
                (true, false) => "Specify point on side for the next offset or [Undo]",
                (false, true) => "Specify through point or [Multiple/Undo]",
                (false, false) => "Specify point on side to offset or [Multiple/Undo]",
            }),
        }
    }

    fn expects(&self, _cx: &ToolContext<'_>) -> ToolInput {
        if self.step == Step::Source {
            ToolInput::Entity
        } else {
            ToolInput::Point
        }
    }

    fn point(&mut self, cx: &ToolContext<'_>, at: Point2) -> ToolStep {
        match self.step {
            Step::Distance => {
                // A distance measured between two points, as AutoCAD allows.
                self.first = Some(at);
                self.step = Step::SecondPoint;
                ToolStep::next("")
            }
            Step::SecondPoint => {
                let distance = self.first.unwrap_or_default().distance_to(at);
                if !(distance > tolerance::GEOMETRIC) {
                    return ToolStep::rejected(
                        "The two points are the same; the offset distance must be greater than \
                         zero.",
                    );
                }
                self.choose_distance(distance)
            }
            Step::Side => self.offset_towards(at),
            Step::Source => base_point(cx, self.expects(cx), at),
        }
    }

    fn entity(&mut self, cx: &ToolContext<'_>, id: EntityId, at: Point2) -> ToolStep {
        if self.step != Step::Source {
            return base_entity(cx, self.expects(cx), id, at);
        }
        let Some(document) = cx.document else {
            return no_drawing();
        };
        if let Some(refusal) = refusal_to_edit(document, id) {
            // Offsetting only reads its source, but the copy lands on the
            // source's layer, which is locked.
            return ToolStep::rejected(refusal);
        }
        let Some(source) = self.session.original(document, id) else {
            return ToolStep::rejected("That object is no longer in the drawing.");
        };
        let entity_type = source.r#type();
        if entity_type != EntityType::Line
            && entity_type != EntityType::Arc
            && entity_type != EntityType::Circle
            && entity_type != EntityType::Polyline
            && entity_type != EntityType::CurvePolyline
        {
            return ToolStep::rejected(format!(
                "A {} cannot be offset; pick a line, arc, circle or polyline.",
                kind_name(&source.geometry)
            ));
        }
        self.chain = vec![source.clone()];
        self.step = Step::Side;
        ToolStep::next("")
    }

    fn value(&mut self, cx: &ToolContext<'_>, text: &str) -> ToolStep {
        if is_option(text, "Undo", "U") {
            return self.undo(cx);
        }
        if self.step == Step::Distance {
            if is_option(text, "Through", "T") {
                self.through = true;
                self.defaults.borrow_mut().offset_distance = None;
                self.step = Step::Source;
                return ToolStep::next("");
            }
            let Some(distance) = parse_number(text) else {
                return ToolStep::rejected(
                    "Type the offset distance, or T to offset through a point.",
                );
            };
            if !(distance > tolerance::GEOMETRIC) {
                return ToolStep::rejected("The offset distance must be greater than zero.");
            }
            return self.choose_distance(distance);
        }
        if self.step == Step::Side && !self.multiple && is_option(text, "Multiple", "M") {
            self.multiple = true;
            return ToolStep::next("Multiple: each point offsets the last offset again.");
        }
        if self.step == Step::Source && is_option(text, "Exit", "E") {
            return self.enter(cx);
        }
        ToolStep::rejected(format!("'{text}' is not an option here."))
    }

    // Esc keeps what the session has done: its Enter only ever commits.
    fn cancel(&mut self, cx: &ToolContext<'_>) -> ToolStep {
        keep_work_on_escape(self, cx)
    }

    fn enter(&mut self, _cx: &ToolContext<'_>) -> ToolStep {
        if self.step == Step::Distance {
            // Enter takes the default: the last distance, or Through.
            let remembered = self.defaults.borrow().offset_distance;
            if let Some(distance) = remembered {
                return self.choose_distance(distance);
            }
            self.through = true;
            self.step = Step::Source;
            return ToolStep::next("");
        }
        if self.step == Step::SecondPoint {
            return ToolStep::rejected("Specify the second point of the offset distance.");
        }
        let count = self.session.added().len();
        if count == 0 {
            return ToolStep::done(None, "", false);
        }
        ToolStep::done(
            Some(self.session.commit("OFFSET")),
            format!(
                "Made {count}{}",
                if count == 1 { " offset." } else { " offsets." }
            ),
            false,
        )
    }

    fn undo(&mut self, _cx: &ToolContext<'_>) -> ToolStep {
        match self.step {
            Step::Distance => ToolStep::rejected("Nothing to undo yet."),
            Step::SecondPoint => {
                self.first = None;
                self.step = Step::Distance;
                ToolStep::next("")
            }
            Step::Source => {
                if self.session.undo() {
                    return ToolStep::next("Undid the last offset.");
                }
                self.step = Step::Distance;
                self.through = false;
                ToolStep::next("")
            }
            Step::Side => {
                if self.chain.len() > 1 {
                    // In Multiple: take back the last copy; the one before it
                    // is the source again.
                    self.session.undo();
                    self.chain.pop();
                    return ToolStep::next("Undid the last offset.");
                }
                self.chain.clear();
                self.multiple = false;
                self.step = Step::Source;
                ToolStep::next("")
            }
        }
    }

    fn preview(&self, _cx: &ToolContext<'_>, cursor: Point2) -> ToolFeedback {
        let mut feedback = ToolFeedback {
            shapes: self.session.shapes(),
            ..ToolFeedback::default()
        };
        if self.step == Step::SecondPoint
            && let Some(first) = self.first
        {
            feedback
                .shapes
                .push(Geometry::Line(Segment2::new(first, cursor)));
            feedback.markers.push(first);
        }
        if self.step == Step::Side
            && let Ok(made) = self.attempt(cursor)
        {
            feedback.shapes.push(made.geometry);
        }
        feedback
    }

    fn last_point(&self, _cx: &ToolContext<'_>) -> Option<Point2> {
        if self.step == Step::SecondPoint {
            self.first
        } else {
            None
        }
    }
}

pub fn make_offset_tool(defaults: Defaults) -> ToolPtr {
    Box::new(OffsetTool {
        defaults,
        session: EditSession::new(),
        step: Step::Distance,
        distance: 0.0,
        through: false,
        multiple: false,
        first: None,
        chain: Vec::new(),
    })
}
