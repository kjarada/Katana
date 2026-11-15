//! Circle and Arc in their construction variants (tools/families.rs;
//! src/katana_cad/tools/draw_curves.cpp).
//!
//! Two state machines, one per curve, each able to run every construction of
//! its curve. The catalogue lists each construction as its own tool - a menu
//! entry and an icon per variant, as a CAD user finds them - and every entry
//! starts the same machine in a different construction. The plain CIRCLE and
//! ARC verbs start the general form, which also takes the other constructions
//! as options at its first prompts, as AutoCAD's commands do; the variants
//! start in theirs and offer only what belongs to it.
//!
//! DIRECTION. An arc these tools make always runs counter-clockwise from its
//! start angle to its end angle: a positive Arc2::sweep, with the start angle
//! in [0, 2*pi). A 3-point arc picked clockwise is the same set of points and
//! is stored reversed, from the third point to the first. One direction for
//! every arc the tools make is the convention DXF and AutoCAD keep, and it lets
//! every construction that is not 3-point state its meaning in one phrase:
//! "counter-clockwise from the start".
//!
//! Both tools restart after each curve, as a CAD user expects of Circle and
//! Arc; Esc ends them.

use katana_commands::{EntityAttributes, create_arc, create_circle};
use katana_core::text::{equals_ignoring_case, parse_finite_double};
use katana_core::{Error, ErrorCode, Result};
use katana_entity::{EntityId, Geometry};
use katana_geometry::{Arc2, Circle2, Point2, Segment2, Triangle2, Vec2};
use katana_math::cpp;
use katana_math::numerics::{DEG_TO_RAD, RAD_TO_DEG, TWO_PI, normalize_angle, tolerance};

use crate::document::Document;
use crate::interactive_tool::{
    InteractiveTool, ToolCatalog, ToolContext, ToolFactory, ToolInfo, ToolInput, ToolStep,
};
use crate::survey_tools::c_fixed;
use crate::tool_feedback::ToolFeedback;
use crate::tools::draw_lines::{DefaultAnswers, factory};
use crate::tools::families::Report;
use crate::utilities::utility_data::trimmed_text;

// ---- typed input -------------------------------------------------------------------

// An option keyword from a prompt: its capitalised short form or the whole
// word, in any case ("d", "D", "Diameter"). An empty `word` is none (the C++
// default).
fn is_option(text: &str, short_form: &str, word: &str) -> bool {
    let typed = trimmed_text(text).as_bytes();
    equals_ignoring_case(typed, short_form.as_bytes())
        || (!word.is_empty() && equals_ignoring_case(typed, word.as_bytes()))
}

// The U of LINE: a typed Undo steps back exactly as the Undo button does.
fn is_undo(text: &str) -> bool {
    is_option(text, "U", "UNDO")
}

fn typed_number(text: &str) -> Option<f64> {
    parse_finite_double(trimmed_text(text))
}

// A length or an angle for the command log: three decimals, trailing zeros
// dropped, so a radius of 5 reads "5" and one of 12.5 reads "12.5". The C++
// std::format("{:.3f}") keeps the sign of a value that rounds to zero, hence
// c_fixed and the "-0" rule.
fn figure(value: f64) -> String {
    let mut text = c_fixed(value, 3);
    while text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    if text == "-0" {
        String::from("0")
    } else {
        text
    }
}

// "Specify radius" + ["Diameter", "Undo"] -> "Specify radius or [Diameter/Undo]".
fn with_options(what: &str, options: &[&str]) -> String {
    if options.is_empty() {
        return String::from(what);
    }
    format!("{what} or [{}]", options.join("/"))
}

fn coincide(a: Point2, b: Point2) -> bool {
    a.distance_to(b) <= tolerance::GEOMETRIC
}

fn midway(a: Point2, b: Point2) -> Point2 {
    (a + b) * 0.5
}

// ---- arcs --------------------------------------------------------------------------

// The same arc running counter-clockwise, its start angle in [0, 2*pi): the one
// form every arc these tools make is stored in (the module doc says why).
fn counter_clockwise(mut arc: Arc2) -> Arc2 {
    if arc.sweep < 0.0 {
        arc = arc.reversed();
    }
    arc.start_angle = normalize_angle(arc.start_angle);
    arc
}

// The arc about `centre` that starts at `start` and runs counter-clockwise to
// the direction of `towards`. `towards` only aims the end, as in AutoCAD: it
// need not lie on the arc, so a user can pick the end anywhere on its ray.
fn arc_from_centre(centre: Point2, start: Point2, towards: Point2) -> Result<Arc2> {
    let radius = centre.distance_to(start);
    if !(radius > tolerance::GEOMETRIC) {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "the start is on the centre, so the arc would have no radius",
        ));
    }
    if coincide(towards, centre) {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "the end point is on the centre, so it gives the arc no direction to end in",
        ));
    }
    let start_angle = (start - centre).angle();
    let sweep = normalize_angle((towards - centre).angle() - start_angle);
    // A sweep that rounds to nothing is the end aimed along the start: the arc
    // would be a point. A full turn is a circle, which Circle draws.
    if !(sweep * radius > tolerance::GEOMETRIC)
        || !((TWO_PI - sweep) * radius > tolerance::GEOMETRIC)
    {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "the end is in the start's direction from the centre, so the arc would be empty; \
             draw a circle for a full turn",
        ));
    }
    Ok(Arc2::new(
        centre,
        radius,
        normalize_angle(start_angle),
        sweep,
    ))
}

// The arc about `centre` from `start` through `degrees`, counter-clockwise
// when positive and clockwise when negative (stored reversed, so still
// counter-clockwise from its own start).
fn arc_from_angle(centre: Point2, start: Point2, degrees: f64) -> Result<Arc2> {
    let radius = centre.distance_to(start);
    if !(radius > tolerance::GEOMETRIC) {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "the start is on the centre, so the arc would have no radius",
        ));
    }
    if !(degrees.abs() * DEG_TO_RAD * radius > tolerance::GEOMETRIC) {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "an included angle of zero draws nothing; give the angle in degrees",
        ));
    }
    if degrees.abs() >= 360.0 {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "an arc turns through less than 360 degrees; draw a circle for a full turn",
        ));
    }
    Ok(counter_clockwise(Arc2::new(
        centre,
        radius,
        (start - centre).angle(),
        degrees * DEG_TO_RAD,
    )))
}

// The centre of an arc from `start` to `end` of signed `radius`. Every centre
// of an arc through both ends lies on their perpendicular bisector; a POSITIVE
// radius puts it on the left of start->end, where the counter-clockwise arc
// from start to end is the minor one, and a NEGATIVE radius on the right,
// where that arc is the major one. So the sign alone chooses the arc, and the
// arc is still counter-clockwise from start to end.
fn centre_for_radius(start: Point2, end: Point2, radius: f64) -> Result<Point2> {
    let half = start.distance_to(end) / 2.0;
    if !(radius.abs() > tolerance::GEOMETRIC) {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "an arc's radius must not be zero",
        ));
    }
    if radius.abs() < half - tolerance::GEOMETRIC {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            format!(
                "a radius of {} is less than half the distance between the ends ({}), so no arc \
                 of that radius joins them",
                figure(radius.abs()),
                figure(half)
            ),
        ));
    }
    let left = (end - start).normalized().perpendicular();
    // max(): a radius within tolerance of the half chord is the semicircle,
    // and must not become the square root of a tiny negative number.
    let rise = cpp::max(0.0, radius * radius - half * half).sqrt();
    let side = if radius > 0.0 { 1.0 } else { -1.0 };
    Ok(midway(start, end) + left * (rise * side))
}

// The counter-clockwise arc from `start` to `end` about `centre`, for a
// centre already on their bisector.
fn arc_between(centre: Point2, start: Point2, end: Point2) -> Arc2 {
    let start_angle = (start - centre).angle();
    let sweep = normalize_angle((end - centre).angle() - start_angle);
    Arc2::new(
        centre,
        centre.distance_to(start),
        normalize_angle(start_angle),
        sweep,
    )
}

// Where a picked point puts the centre of an arc from `start` to `end`: the
// point of their perpendicular bisector nearest it. The side it is on chooses
// the arc exactly as a radius's sign does (centre_for_radius).
fn centre_near(start: Point2, end: Point2, picked: Point2) -> Point2 {
    let middle = midway(start, end);
    let left = (end - start).normalized().perpendicular();
    middle + left * (picked - middle).dot(left)
}

// ---- tangent circles ---------------------------------------------------------------

// A straight line the circle is to touch: a line entity, or the segment of a
// polyline nearest where it was picked.
#[derive(Clone, Copy, Debug)]
struct TangentLine {
    id: EntityId,
    /// The polyline segment; 0 for a line entity.
    segment: usize,
    line: Segment2,
    pick: Point2,
}

// The corner two picked lines make: where they cross, and the unit direction
// of each ARM - the half of the line on the side it was picked.
#[derive(Clone, Copy, Debug)]
struct Corner {
    apex: Point2,
    first: Vec2,
    second: Vec2,
}

fn corner_of(a: &TangentLine, b: &TangentLine) -> Result<Corner> {
    let da = a.line.delta().normalized();
    let db = b.line.delta().normalized();
    let sine = da.cross(db);
    if sine.abs() <= tolerance::ANGULAR {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "the two lines are parallel, so there is no corner between them for a circle to sit \
             in",
        ));
    }
    let along = (b.line.start - a.line.start).cross(db) / sine;
    let apex = a.line.start + da * along;
    let ta = (a.pick - apex).dot(da);
    let tb = (b.pick - apex).dot(db);
    if ta.abs() <= tolerance::GEOMETRIC || tb.abs() <= tolerance::GEOMETRIC {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "a line was picked where the two cross, which does not say which corner the circle \
             goes in; pick each line on the side the circle should touch",
        ));
    }
    Ok(Corner {
        apex,
        first: da * (if ta > 0.0 { 1.0 } else { -1.0 }),
        second: db * (if tb > 0.0 { 1.0 } else { -1.0 }),
    })
}

// The circle of `radius` touching both lines, in the corner whose arms were
// picked.
//
// Two crossing lines make four corners, and a circle of a given radius fits
// in each, so there are four answers. The one taken is the circle in the
// corner between the arm of each line on the side it was picked - the corner
// a user means by picking near it, the reading Fillet gives its picks. Its
// centre is on that corner's bisector: c = apex + (u1 + u2) r / |u1 x u2|,
// since c - apex = a u1 + b u2 is r from the line along u1 exactly when
// |b (u1 x u2)| = r, and likewise a, so a = b = r / |sin| (u1, u2 unit).
// The circle may touch a line beyond the end of its segment: it is tangent
// to the LINE, as AutoCAD's Ttr is.
fn circle_in_corner(corner: &Corner, radius: f64) -> Circle2 {
    let sine = corner.first.cross(corner.second).abs();
    Circle2::new(
        corner.apex + (corner.first + corner.second) * (radius / sine),
        radius,
    )
}

// The line of entity `id` near `at`, or why it is not one.
fn tangent_line_of(document: Option<&Document>, id: EntityId, at: Point2) -> Result<TangentLine> {
    let Some(entity) = document.and_then(|document| document.model().entities.find(id)) else {
        return Err(Error::new(
            ErrorCode::NotFound,
            "that entity is not in the drawing",
        ));
    };
    let mut out = TangentLine {
        id,
        segment: 0,
        line: Segment2::default(),
        pick: at,
    };
    match &entity.geometry {
        Geometry::Line(line) => out.line = *line,
        Geometry::Polyline(polyline) => {
            // The segment the pick is nearest, skipping zero-length ones,
            // which have no direction to be tangent to.
            let mut best = 0.0;
            let mut found = false;
            for i in 0..polyline.segment_count() {
                let segment = polyline.segment(i);
                if segment.is_degenerate() {
                    continue;
                }
                let distance = segment.distance_to(at);
                if !found || distance < best {
                    best = distance;
                    found = true;
                    out.segment = i;
                    out.line = segment;
                }
            }
            if !found {
                return Err(Error::new(
                    ErrorCode::InvalidGeometry,
                    "that polyline has no length",
                ));
            }
        }
        _ => {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "the circle is fitted to straight lines; pick a line or a polyline",
            ));
        }
    }
    if out.line.is_degenerate() {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "that line has no length, so it has no direction to touch",
        ));
    }
    Ok(out)
}

// ---- Circle ------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum CircleMode {
    #[default]
    CentreRadius,
    CentreDiameter,
    TwoPoint,
    ThreePoint,
    TangentTangentRadius,
}

#[derive(Clone, Debug, Default)]
struct CircleState {
    mode: CircleMode,
    points: Vec<Point2>,
    tangents: Vec<TangentLine>,
}

struct CircleTool {
    attributes: EntityAttributes,
    /// The CIRCLE verb's form, which offers the other constructions as options
    /// at its first prompt.
    general: bool,
    state: CircleState,
    history: Vec<CircleState>,
}

impl CircleTool {
    fn new(attributes: &EntityAttributes, mode: CircleMode, general: bool) -> Self {
        Self {
            attributes: attributes.clone(),
            general,
            state: CircleState {
                mode,
                ..CircleState::default()
            },
            history: Vec::new(),
        }
    }

    fn undo_option(&self) -> Vec<&'static str> {
        self.with_undo(Vec::new())
    }

    fn with_undo(&self, mut options: Vec<&'static str>) -> Vec<&'static str> {
        if !self.history.is_empty() {
            options.push("Undo");
        }
        options
    }

    fn centred(&self) -> bool {
        self.state.mode == CircleMode::CentreRadius || self.state.mode == CircleMode::CentreDiameter
    }

    fn measure_name(&self) -> &'static str {
        if self.state.mode == CircleMode::CentreDiameter {
            "diameter"
        } else {
            "radius"
        }
    }

    // Every accepted input is recorded, so Undo steps back over an option as
    // readily as over a point.
    fn accept(&mut self, next: CircleState) -> ToolStep {
        let before = std::mem::replace(&mut self.state, next);
        self.history.push(before);
        ToolStep::next("")
    }

    fn accept_point(&mut self, at: Point2) -> ToolStep {
        let mut next = self.state.clone();
        next.points.push(at);
        self.accept(next)
    }

    fn switch_to(&mut self, mode: CircleMode) -> ToolStep {
        let next = CircleState {
            mode,
            ..self.state.clone()
        };
        self.accept(next)
    }

    // `zero_why`: the refusal for a circle of no size, when this input can
    // produce one.
    fn finish(&self, circle: Circle2, zero_why: &str) -> ToolStep {
        if !(circle.radius > tolerance::GEOMETRIC) || !circle.radius.is_finite() {
            return ToolStep::rejected(if zero_why.is_empty() {
                String::from("the circle would have no radius")
            } else {
                format!("{zero_why}, so the circle would have no radius")
            });
        }
        ToolStep::done(
            Some(create_circle(
                circle.center,
                circle.radius,
                self.attributes.clone(),
            )),
            format!("circle, radius {}", figure(circle.radius)),
            true,
        )
    }
}

impl InteractiveTool for CircleTool {
    fn prompt(&self, _cx: &ToolContext<'_>) -> String {
        let n = self.state.points.len();
        match self.state.mode {
            CircleMode::CentreRadius | CircleMode::CentreDiameter => {
                if n == 0 {
                    let options = if self.general {
                        vec!["3P", "2P", "Ttr"]
                    } else {
                        self.undo_option()
                    };
                    return with_options("Specify centre point", &options);
                }
                if self.state.mode == CircleMode::CentreRadius {
                    with_options("Specify radius", &self.with_undo(vec!["Diameter"]))
                } else {
                    with_options("Specify diameter", &self.with_undo(vec!["Radius"]))
                }
            }
            CircleMode::TwoPoint => with_options(
                if n == 0 {
                    "Specify first end of the circle's diameter"
                } else {
                    "Specify second end of the circle's diameter"
                },
                &self.undo_option(),
            ),
            CircleMode::ThreePoint => with_options(
                match n {
                    0 => "Specify first point on the circle",
                    1 => "Specify second point on the circle",
                    _ => "Specify third point on the circle",
                },
                &self.undo_option(),
            ),
            CircleMode::TangentTangentRadius => with_options(
                match self.state.tangents.len() {
                    0 => "Select the first line to touch",
                    1 => "Select the second line to touch",
                    _ => "Specify radius of circle",
                },
                &self.undo_option(),
            ),
        }
    }

    fn expects(&self, _cx: &ToolContext<'_>) -> ToolInput {
        if self.state.mode == CircleMode::TangentTangentRadius {
            return if self.state.tangents.len() < 2 {
                ToolInput::Entity
            } else {
                ToolInput::Value
            };
        }
        ToolInput::Point
    }

    fn point(&mut self, _cx: &ToolContext<'_>, at: Point2) -> ToolStep {
        let points = self.state.points.clone();
        match self.state.mode {
            CircleMode::CentreRadius | CircleMode::CentreDiameter => {
                let Some(&centre) = points.first() else {
                    return self.accept_point(at);
                };
                // A picked point is on the circumference for a radius, and at
                // the far end of the diameter for a diameter, as the rubber
                // band shows.
                let length = centre.distance_to(at);
                let radius = if self.state.mode == CircleMode::CentreRadius {
                    length
                } else {
                    length / 2.0
                };
                self.finish(Circle2::new(centre, radius), "the point is on the centre")
            }
            CircleMode::TwoPoint => {
                let Some(&first) = points.first() else {
                    return self.accept_point(at);
                };
                if coincide(first, at) {
                    return ToolStep::rejected(
                        "the second end is on the first, so the circle would have no diameter",
                    );
                }
                self.finish(
                    Circle2::new(midway(first, at), first.distance_to(at) / 2.0),
                    "",
                )
            }
            CircleMode::ThreePoint => {
                if points.iter().any(|&earlier| coincide(earlier, at)) {
                    return ToolStep::rejected(
                        "that point is on one already picked; a circle needs three separate \
                         points",
                    );
                }
                if points.len() < 2 {
                    return self.accept_point(at);
                }
                let Some(circle) = Triangle2::new(points[0], points[1], at).circumcircle() else {
                    return ToolStep::rejected(
                        "the three points are in a line, so no circle passes through them",
                    );
                };
                self.finish(circle, "")
            }
            CircleMode::TangentTangentRadius => {
                ToolStep::rejected(if self.state.tangents.len() < 2 {
                    "select a line in the drawing: the circle is fitted to two lines, not placed at \
                 a point"
                } else {
                    "type the radius as a number"
                })
            }
        }
    }

    fn entity(&mut self, cx: &ToolContext<'_>, id: EntityId, at: Point2) -> ToolStep {
        if self.state.mode != CircleMode::TangentTangentRadius || self.state.tangents.len() >= 2 {
            return DefaultAnswers(self.expects(cx)).entity(cx, id, at);
        }
        let picked = match tangent_line_of(cx.document, id, at) {
            Ok(picked) => picked,
            Err(error) => return ToolStep::rejected(error.message),
        };
        if let Some(first) = self.state.tangents.first() {
            if first.id == picked.id && first.segment == picked.segment {
                return ToolStep::rejected("that line is already picked; pick the other line");
            }
            if let Err(error) = corner_of(first, &picked) {
                return ToolStep::rejected(error.message);
            }
        }
        let mut next = self.state.clone();
        next.tangents.push(picked);
        self.accept(next)
    }

    fn value(&mut self, cx: &ToolContext<'_>, text: &str) -> ToolStep {
        if is_undo(text) {
            return self.undo(cx);
        }
        let n = self.state.points.len();
        let centred = self.centred();
        if centred && n == 0 && self.general {
            if is_option(text, "3P", "") {
                return self.switch_to(CircleMode::ThreePoint);
            }
            if is_option(text, "2P", "") {
                return self.switch_to(CircleMode::TwoPoint);
            }
            if is_option(text, "T", "TTR") {
                return self.switch_to(CircleMode::TangentTangentRadius);
            }
        }
        if centred && n == 1 {
            if self.state.mode == CircleMode::CentreRadius && is_option(text, "D", "DIAMETER") {
                return self.switch_to(CircleMode::CentreDiameter);
            }
            if self.state.mode == CircleMode::CentreDiameter && is_option(text, "R", "RADIUS") {
                return self.switch_to(CircleMode::CentreRadius);
            }
            let Some(number) = typed_number(text) else {
                return ToolStep::rejected(format!(
                    "'{text}' is not a {}; type a number, pick a point on the circle, or {}",
                    self.measure_name(),
                    if self.state.mode == CircleMode::CentreRadius {
                        "D"
                    } else {
                        "R"
                    }
                ));
            };
            if !(number > 0.0) {
                return ToolStep::rejected(format!(
                    "a {} must be greater than zero",
                    self.measure_name()
                ));
            }
            let radius = if self.state.mode == CircleMode::CentreRadius {
                number
            } else {
                number / 2.0
            };
            return self.finish(Circle2::new(self.state.points[0], radius), "");
        }
        if self.state.mode == CircleMode::TangentTangentRadius && self.state.tangents.len() == 2 {
            let Some(number) = typed_number(text) else {
                return ToolStep::rejected(format!("'{text}' is not a radius; type a number"));
            };
            if !(number > 0.0) {
                return ToolStep::rejected("a radius must be greater than zero");
            }
            return match corner_of(&self.state.tangents[0], &self.state.tangents[1]) {
                Err(error) => ToolStep::rejected(error.message),
                Ok(corner) => self.finish(circle_in_corner(&corner, number), ""),
            };
        }
        if self.state.mode == CircleMode::TangentTangentRadius {
            return ToolStep::rejected("select a line in the drawing for the circle to touch");
        }
        ToolStep::rejected(format!(
            "'{text}' is not a point; type one as x,y or pick it{}",
            if centred && n == 0 && self.general {
                ", or type 3P, 2P or Ttr"
            } else {
                ""
            }
        ))
    }

    fn undo(&mut self, cx: &ToolContext<'_>) -> ToolStep {
        let Some(before) = self.history.pop() else {
            return DefaultAnswers(self.expects(cx)).undo(cx);
        };
        self.state = before;
        ToolStep::next("")
    }

    fn preview(&self, _cx: &ToolContext<'_>, cursor: Point2) -> ToolFeedback {
        let mut feedback = ToolFeedback::default();
        let points = &self.state.points;
        match self.state.mode {
            CircleMode::CentreRadius | CircleMode::CentreDiameter => {
                if points.len() == 1 {
                    let length = points[0].distance_to(cursor);
                    let radius = if self.state.mode == CircleMode::CentreRadius {
                        length
                    } else {
                        length / 2.0
                    };
                    feedback
                        .shapes
                        .push(Geometry::Circle(Circle2::new(points[0], radius)));
                    feedback
                        .shapes
                        .push(Geometry::Line(Segment2::new(points[0], cursor)));
                    feedback.markers.push(points[0]);
                }
            }
            CircleMode::TwoPoint => {
                if points.len() == 1 {
                    feedback.shapes.push(Geometry::Circle(Circle2::new(
                        midway(points[0], cursor),
                        points[0].distance_to(cursor) / 2.0,
                    )));
                    feedback
                        .shapes
                        .push(Geometry::Line(Segment2::new(points[0], cursor)));
                    feedback.markers.push(points[0]);
                }
            }
            CircleMode::ThreePoint => {
                if points.len() == 1 {
                    feedback
                        .shapes
                        .push(Geometry::Line(Segment2::new(points[0], cursor)));
                } else if points.len() == 2 {
                    match Triangle2::new(points[0], points[1], cursor).circumcircle() {
                        Some(circle) => feedback.shapes.push(Geometry::Circle(circle)),
                        None => feedback
                            .shapes
                            .push(Geometry::Line(Segment2::new(points[1], cursor))),
                    }
                }
                feedback.markers = points.clone();
            }
            CircleMode::TangentTangentRadius => {
                for line in &self.state.tangents {
                    feedback.markers.push(line.line.closest_point(line.pick));
                }
                if self.state.tangents.len() == 2
                    && let Ok(corner) = corner_of(&self.state.tangents[0], &self.state.tangents[1])
                {
                    feedback.markers.push(corner.apex);
                }
            }
        }
        feedback
    }

    fn last_point(&self, _cx: &ToolContext<'_>) -> Option<Point2> {
        self.state.points.last().copied()
    }
}

// ---- Arc ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum ArcMode {
    #[default]
    ThreePoint,
    StartCentreEnd,
    CentreStartEnd,
    StartEndRadius,
}

#[derive(Clone, Debug, Default)]
struct ArcState {
    mode: ArcMode,
    points: Vec<Point2>,
    /// Start-centre-end: the included angle is being typed instead of an end
    /// point being picked (the Angle option).
    by_angle: bool,
}

struct ArcTool {
    attributes: EntityAttributes,
    /// The ARC verb's form, which offers the other constructions as options at
    /// its first two prompts.
    general: bool,
    state: ArcState,
    history: Vec<ArcState>,
}

impl ArcTool {
    fn new(attributes: &EntityAttributes, mode: ArcMode, general: bool) -> Self {
        Self {
            attributes: attributes.clone(),
            general,
            state: ArcState {
                mode,
                ..ArcState::default()
            },
            history: Vec::new(),
        }
    }

    fn with_undo(&self, mut options: Vec<&'static str>) -> Vec<&'static str> {
        if !self.history.is_empty() {
            options.push("Undo");
        }
        options
    }

    fn end_prompt(&self) -> String {
        if self.state.by_angle {
            return with_options(
                "Specify included angle in degrees (negative for clockwise)",
                &self.with_undo(Vec::new()),
            );
        }
        with_options(
            "Specify end point of arc, counter-clockwise from the start",
            &self.with_undo(vec!["Angle"]),
        )
    }

    // The centre and the start, in whichever order the construction took them.
    fn centre(&self) -> Point2 {
        self.state.points[if self.state.mode == ArcMode::StartCentreEnd {
            1
        } else {
            0
        }]
    }

    fn start(&self) -> Point2 {
        self.state.points[if self.state.mode == ArcMode::StartCentreEnd {
            0
        } else {
            1
        }]
    }

    fn accept(&mut self, next: ArcState) -> ToolStep {
        let before = std::mem::replace(&mut self.state, next);
        self.history.push(before);
        ToolStep::next("")
    }

    fn accept_point(&mut self, at: Point2) -> ToolStep {
        let mut next = self.state.clone();
        next.points.push(at);
        self.accept(next)
    }

    fn switch_to(&mut self, mode: ArcMode) -> ToolStep {
        let next = ArcState {
            mode,
            ..self.state.clone()
        };
        self.accept(next)
    }

    fn finish(&self, arc: Arc2) -> ToolStep {
        // The model refuses these too; refusing here keeps the tool where it
        // is, with a sentence, instead of failing the command.
        if !(arc.radius > tolerance::GEOMETRIC)
            || !arc.radius.is_finite()
            || !(arc.length() > tolerance::GEOMETRIC)
        {
            return ToolStep::rejected("the arc would have no length");
        }
        ToolStep::done(
            Some(create_arc(arc, self.attributes.clone())),
            format!(
                "arc, radius {}, {} degrees",
                figure(arc.radius),
                figure(arc.sweep * RAD_TO_DEG)
            ),
            true,
        )
    }
}

impl InteractiveTool for ArcTool {
    fn prompt(&self, _cx: &ToolContext<'_>) -> String {
        let n = self.state.points.len();
        match self.state.mode {
            ArcMode::ThreePoint => {
                if n == 0 {
                    let options = if self.general {
                        vec!["Centre"]
                    } else {
                        Vec::new()
                    };
                    return with_options("Specify start point of arc", &options);
                }
                if n == 1 {
                    let options = if self.general {
                        self.with_undo(vec!["Centre", "End"])
                    } else {
                        self.with_undo(Vec::new())
                    };
                    return with_options("Specify second point of arc", &options);
                }
                with_options("Specify end point of arc", &self.with_undo(Vec::new()))
            }
            ArcMode::StartCentreEnd => {
                if n == 0 {
                    return with_options("Specify start point of arc", &self.with_undo(Vec::new()));
                }
                if n == 1 {
                    return with_options(
                        "Specify centre point of arc",
                        &self.with_undo(Vec::new()),
                    );
                }
                self.end_prompt()
            }
            ArcMode::CentreStartEnd => {
                if n == 0 {
                    return with_options(
                        "Specify centre point of arc",
                        &self.with_undo(Vec::new()),
                    );
                }
                if n == 1 {
                    return with_options("Specify start point of arc", &self.with_undo(Vec::new()));
                }
                self.end_prompt()
            }
            ArcMode::StartEndRadius => {
                if n == 0 {
                    return with_options("Specify start point of arc", &self.with_undo(Vec::new()));
                }
                if n == 1 {
                    return with_options("Specify end point of arc", &self.with_undo(Vec::new()));
                }
                with_options(
                    "Specify radius of arc (negative for the major arc) or pick its centre",
                    &self.with_undo(Vec::new()),
                )
            }
        }
    }

    fn expects(&self, _cx: &ToolContext<'_>) -> ToolInput {
        if self.state.by_angle {
            ToolInput::Value
        } else {
            ToolInput::Point
        }
    }

    fn point(&mut self, _cx: &ToolContext<'_>, at: Point2) -> ToolStep {
        if self.state.by_angle {
            return ToolStep::rejected("type the included angle in degrees");
        }
        let points = self.state.points.clone();
        match self.state.mode {
            ArcMode::ThreePoint => {
                if points.iter().any(|&earlier| coincide(earlier, at)) {
                    return ToolStep::rejected(
                        "that point is on one already picked; an arc through three points needs \
                         three separate points",
                    );
                }
                if points.len() < 2 {
                    return self.accept_point(at);
                }
                let Some(arc) = Arc2::through_points(points[0], points[1], at) else {
                    return ToolStep::rejected(
                        "the three points are in a line, so no arc passes through them",
                    );
                };
                self.finish(counter_clockwise(arc))
            }
            ArcMode::StartCentreEnd | ArcMode::CentreStartEnd => {
                if points.len() == 1 && coincide(points[0], at) {
                    return ToolStep::rejected(
                        "the start and the centre coincide, so the arc would have no radius",
                    );
                }
                if points.len() < 2 {
                    return self.accept_point(at);
                }
                match arc_from_centre(self.centre(), self.start(), at) {
                    Err(error) => ToolStep::rejected(error.message),
                    Ok(arc) => self.finish(arc),
                }
            }
            ArcMode::StartEndRadius => {
                if points.len() == 1 && coincide(points[0], at) {
                    return ToolStep::rejected(
                        "the end is on the start, so there is no arc between them",
                    );
                }
                if points.len() < 2 {
                    return self.accept_point(at);
                }
                self.finish(arc_between(
                    centre_near(points[0], points[1], at),
                    points[0],
                    points[1],
                ))
            }
        }
    }

    fn value(&mut self, cx: &ToolContext<'_>, text: &str) -> ToolStep {
        if is_undo(text) {
            return self.undo(cx);
        }
        let n = self.state.points.len();
        if self.state.mode == ArcMode::ThreePoint
            && self.general
            && n == 0
            && is_option(text, "C", "CENTRE")
        {
            return self.switch_to(ArcMode::CentreStartEnd);
        }
        if self.state.mode == ArcMode::ThreePoint && self.general && n == 1 {
            if is_option(text, "C", "CENTRE") {
                return self.switch_to(ArcMode::StartCentreEnd);
            }
            if is_option(text, "E", "END") {
                return self.switch_to(ArcMode::StartEndRadius);
            }
        }
        let centred = self.state.mode == ArcMode::StartCentreEnd
            || self.state.mode == ArcMode::CentreStartEnd;
        if centred && n == 2 && !self.state.by_angle && is_option(text, "A", "ANGLE") {
            let next = ArcState {
                by_angle: true,
                ..self.state.clone()
            };
            return self.accept(next);
        }
        if self.state.by_angle {
            let Some(degrees) = typed_number(text) else {
                return ToolStep::rejected(format!("'{text}' is not an angle; type it in degrees"));
            };
            return match arc_from_angle(self.centre(), self.start(), degrees) {
                Err(error) => ToolStep::rejected(error.message),
                Ok(arc) => self.finish(arc),
            };
        }
        if self.state.mode == ArcMode::StartEndRadius && n == 2 {
            let Some(radius) = typed_number(text) else {
                return ToolStep::rejected(format!(
                    "'{text}' is not a radius; type a number or pick the centre"
                ));
            };
            let (start, end) = (self.state.points[0], self.state.points[1]);
            return match centre_for_radius(start, end, radius) {
                Err(error) => ToolStep::rejected(error.message),
                Ok(middle) => self.finish(arc_between(middle, start, end)),
            };
        }
        ToolStep::rejected(format!(
            "'{text}' is not a point; type one as x,y or pick it"
        ))
    }

    fn undo(&mut self, cx: &ToolContext<'_>) -> ToolStep {
        let Some(before) = self.history.pop() else {
            return DefaultAnswers(self.expects(cx)).undo(cx);
        };
        self.state = before;
        ToolStep::next("")
    }

    fn preview(&self, _cx: &ToolContext<'_>, cursor: Point2) -> ToolFeedback {
        let points = &self.state.points;
        let mut feedback = ToolFeedback {
            markers: points.clone(),
            ..ToolFeedback::default()
        };
        if points.is_empty() || self.state.by_angle {
            return feedback;
        }
        match self.state.mode {
            ArcMode::ThreePoint => {
                let through = if points.len() == 2 {
                    Arc2::through_points(points[0], points[1], cursor)
                } else {
                    None
                };
                match through {
                    Some(arc) => feedback.shapes.push(Geometry::Arc(counter_clockwise(arc))),
                    None => feedback.shapes.push(Geometry::Line(Segment2::new(
                        points[points.len() - 1],
                        cursor,
                    ))),
                }
            }
            ArcMode::StartCentreEnd | ArcMode::CentreStartEnd => {
                if points.len() == 2 {
                    if let Ok(arc) = arc_from_centre(self.centre(), self.start(), cursor) {
                        feedback.shapes.push(Geometry::Arc(arc));
                    }
                    feedback
                        .shapes
                        .push(Geometry::Line(Segment2::new(self.centre(), cursor)));
                } else {
                    feedback
                        .shapes
                        .push(Geometry::Line(Segment2::new(points[0], cursor)));
                }
            }
            ArcMode::StartEndRadius => {
                if points.len() == 2 {
                    let middle = centre_near(points[0], points[1], cursor);
                    feedback
                        .shapes
                        .push(Geometry::Arc(arc_between(middle, points[0], points[1])));
                    feedback.markers.push(middle);
                } else {
                    feedback
                        .shapes
                        .push(Geometry::Line(Segment2::new(points[0], cursor)));
                }
            }
        }
        feedback
    }

    fn last_point(&self, _cx: &ToolContext<'_>) -> Option<Point2> {
        self.state.points.last().copied()
    }
}

// ---- the catalogue -----------------------------------------------------------------

fn curve_tool(
    id: &str,
    name: &str,
    order: i32,
    aliases: &[&str],
    tip: &str,
    make: ToolFactory,
) -> ToolInfo {
    ToolInfo {
        id: String::from(id),
        name: String::from(name),
        category: String::from("Draw"),
        group: String::from("Curves"),
        order,
        aliases: aliases.iter().map(|&alias| String::from(alias)).collect(),
        tip: String::from(tip),
        make: Some(make),
        ..ToolInfo::default()
    }
}

fn circle_tool(mode: CircleMode, general: bool) -> ToolFactory {
    factory(move |_, start| -> Box<dyn InteractiveTool> {
        Box::new(CircleTool::new(&start.attributes, mode, general))
    })
}

fn arc_tool(mode: ArcMode, general: bool) -> ToolFactory {
    factory(move |_, start| -> Box<dyn InteractiveTool> {
        Box::new(ArcTool::new(&start.attributes, mode, general))
    })
}

/// Circle and Arc in their construction variants (the C++ addDrawCurveTools).
pub fn add_draw_curve_tools(catalog: &mut ToolCatalog, report: &mut Report<'_>) {
    // CIRCLE and ARC mean here what they mean to the command interpreter
    // (centre and radius; start, a point on the arc, end), and what they mean
    // in AutoCAD. The variants have no AutoCAD verb of their own - there they
    // are options of CIRCLE and ARC, as they are of the general tools here.
    report(catalog.add(curve_tool(
        "draw.circle",
        "Circle, Centre Radius",
        10,
        &["CIRCLE", "C"],
        "Draws a circle from its centre and radius; type D for a diameter, or 3P, 2P or Ttr for \
         the other constructions.",
        circle_tool(CircleMode::CentreRadius, true),
    )));
    report(catalog.add(curve_tool(
        "draw.circle.diameter",
        "Circle, Centre Diameter",
        20,
        &[],
        "Draws a circle from its centre and diameter.",
        circle_tool(CircleMode::CentreDiameter, false),
    )));
    report(catalog.add(curve_tool(
        "draw.circle.2p",
        "Circle, 2 Points",
        30,
        &[],
        "Draws a circle on two points at the ends of its diameter.",
        circle_tool(CircleMode::TwoPoint, false),
    )));
    report(catalog.add(curve_tool(
        "draw.circle.3p",
        "Circle, 3 Points",
        40,
        &[],
        "Draws the circle through three points.",
        circle_tool(CircleMode::ThreePoint, false),
    )));
    report(catalog.add(curve_tool(
        "draw.circle.ttr",
        "Circle, Tangent Tangent Radius",
        50,
        &[],
        "Draws a circle of a given radius touching two lines, in the corner whose sides you pick.",
        circle_tool(CircleMode::TangentTangentRadius, false),
    )));

    report(catalog.add(curve_tool(
        "draw.arc",
        "Arc, 3 Points",
        60,
        &["ARC", "A"],
        "Draws an arc through its start, a second point and its end; type C or E for the centre \
         and radius constructions.",
        arc_tool(ArcMode::ThreePoint, true),
    )));
    report(catalog.add(curve_tool(
        "draw.arc.sce",
        "Arc, Start Centre End",
        70,
        &[],
        "Draws an arc counter-clockwise about a centre, from a start point to the direction of an \
         end point or through an angle.",
        arc_tool(ArcMode::StartCentreEnd, false),
    )));
    report(catalog.add(curve_tool(
        "draw.arc.cse",
        "Arc, Centre Start End",
        80,
        &[],
        "Draws an arc about a centre picked first, counter-clockwise from a start point to the \
         direction of an end point or through an angle.",
        arc_tool(ArcMode::CentreStartEnd, false),
    )));
    report(catalog.add(curve_tool(
        "draw.arc.ser",
        "Arc, Start End Radius",
        90,
        &[],
        "Draws an arc counter-clockwise between two ends with a given radius: positive for the \
         minor arc, negative for the major one.",
        arc_tool(ArcMode::StartEndRadius, false),
    )));
}
