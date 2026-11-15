//! The drawing system's draw tools (tools/families.rs;
//! src/katana_cad/tools/draw_professional.cpp): the shapes a survey and civil
//! drafting package draws beyond lines and circles - the 3D polyline, the
//! spline, the ellipse and elliptical arc, construction lines and rays, the
//! double line, freehand sketch and the revision cloud (docs/drawing.md, "Draw
//! tools"). The Polyline tool's arc segments are in draw_lines.rs, beside the
//! tool they extend.
//!
//! Each follows the drafting habits of the commands of the same names -
//! prompts, option letters, Undo and Enter - and hands the document ONE
//! command, so one undo takes back the whole shape. What they make is stored
//! by the drawing system's rule (docs/drawing.md, "Which kind a polyline is"):
//! a straight 3D string is a Polyline2 with its heights in the elevation
//! properties, exactly as an imported one.
//!
//! The remembered settings (the current 3D polyline height, the double line's
//! width, the revision cloud's arc length) are process-wide statics in the
//! C++; the program uses them on one thread and the tests run on several, so
//! they are thread_local here (the cad port's plan, section 7, risk 4).

use std::any::Any;
use std::cell::{Cell, RefCell};

use katana_commands::EntityAttributes;
use katana_core::Result;
use katana_core::text::{
    equals_ignoring_case, format_exact_real, parse_finite_double, parse_integer,
};
use katana_entity::{Entity, EntityId, Geometry, Model, validate};
use katana_geometry::curves2d::{arc_from_bulge, bulge_from_sweep};
use katana_geometry::polyline_vertices::{nearest_segment, weed};
use katana_geometry::{
    Arc2, Circle2, CurvePolyline2, CurveSegment, CurveVertex, Ellipse2, Line2, Point2, Polyline2,
    Rectangle2, Segment2, Spline2, Vec2,
};
use katana_math::numerics::{DEG_TO_RAD, HALF_PI, normalize_angle, tolerance};

use crate::document::Document;
use crate::drawing::draw_shapes::{
    construction_segment, create_construction, create_drawn, double_line_sides, drawn_entity,
};
use crate::drawing::vertex_editing::{height_at_point, read_polyline, write_polyline};
use crate::interactive_tool::{
    InteractiveTool, ToolCatalog, ToolContext, ToolFactory, ToolInfo, ToolInput, ToolStep, view_of,
};
use crate::tool_feedback::ToolFeedback;
use crate::tools::draw_lines::factory;
use crate::tools::families::{Report, keep_work_on_escape};
use crate::utilities::utility_data::trimmed_text;

fn is_option(text: &str, keyword: &str) -> bool {
    let typed = trimmed_text(text);
    !typed.is_empty()
        && typed.len() <= keyword.len()
        && equals_ignoring_case(typed.as_bytes(), &keyword.as_bytes()[..typed.len()])
}

fn coincide(a: Point2, b: Point2) -> bool {
    a.distance_to(b) <= tolerance::GEOMETRIC
}

fn quoted(text: &str) -> String {
    format!("'{}'", trimmed_text(text))
}

fn number(value: f64) -> String {
    format_exact_real(value)
}

// A polyline entity stored as the simplest kind that holds it.
fn polyline_entity(polyline: &CurvePolyline2, attributes: &EntityAttributes) -> Result<Entity> {
    write_polyline(drawn_entity(Geometry::default(), attributes), polyline)
}

// ---- 3D Polyline ---------------------------------------------------------------------------
//
// A polyline whose every vertex may have a height: typed as x,y,z, taken from
// what the point snaps to (a surveyed point, a string's vertex, a line's end
// that has one), or the current height set with [Height]. A vertex with none
// has none - never zero.

thread_local! {
    // The current height, remembered by the program across the tool's
    // restarts.
    static CURRENT_POLYLINE_HEIGHT: Cell<Option<f64>> = const { Cell::new(None) };
}

struct Polyline3dTool {
    attributes: EntityAttributes,
    vertices: Vec<CurveVertex>,
    current: Option<f64>,
    asking_height: bool,
}

impl Polyline3dTool {
    fn new(attributes: &EntityAttributes) -> Self {
        Self {
            attributes: attributes.clone(),
            vertices: Vec::new(),
            current: CURRENT_POLYLINE_HEIGHT.get(),
            asking_height: false,
        }
    }

    fn add(&mut self, at: Point2, height: Option<f64>) -> ToolStep {
        if self
            .vertices
            .last()
            .is_some_and(|last| coincide(at, last.position))
        {
            return ToolStep::rejected("the point is where the last vertex is");
        }
        self.vertices.push(CurveVertex {
            position: at,
            bulge: 0.0,
            height,
        });
        ToolStep::next(match height {
            Some(height) => format!("height {}", number(height)),
            None => String::from("no height"),
        })
    }

    fn finish(&mut self, closed: bool) -> ToolStep {
        let polyline = CurvePolyline2 {
            vertices: self.vertices.clone(),
            closed,
        };
        if closed && polyline.vertices.len() < 3 {
            return ToolStep::rejected("a closed polyline needs three vertices");
        }
        let entity = match polyline_entity(&polyline, &self.attributes) {
            Ok(entity) => entity,
            Err(error) => return ToolStep::rejected(error.message),
        };
        let count = self.vertices.len();
        self.vertices.clear();
        ToolStep::done(
            Some(create_drawn("CREATE_POLYLINE_3D", vec![entity])),
            format!("3D polyline of {count} vertices"),
            true,
        )
    }
}

impl InteractiveTool for Polyline3dTool {
    fn prompt(&self, _cx: &ToolContext<'_>) -> String {
        if self.asking_height {
            return String::from("Specify the height for the next vertices, or [None]");
        }
        let height = match self.current {
            Some(current) => format!(" <height {}>", number(current)),
            None => String::from(" <no height>"),
        };
        if self.vertices.is_empty() {
            return format!("Specify start point (x,y,z) or [Height]{height}");
        }
        format!(
            "Specify next point (x,y,z) or [Height/{}Undo]{height}",
            if self.vertices.len() >= 3 {
                "Close/"
            } else {
                ""
            }
        )
    }

    fn expects(&self, _cx: &ToolContext<'_>) -> ToolInput {
        if self.asking_height {
            ToolInput::Value
        } else {
            ToolInput::Point
        }
    }

    fn point(&mut self, cx: &ToolContext<'_>, at: Point2) -> ToolStep {
        let mut height = self.current;
        // The view's own hidden layers: a snapped point's height is not read
        // through them.
        if let Some(document) = cx.document
            && let Some(found) = height_at_point(document, at, 1.0e-6, view_of(cx))
        {
            height = Some(found);
        }
        self.add(at, height)
    }

    fn point3d(&mut self, _cx: &ToolContext<'_>, at: Point2, z: f64) -> ToolStep {
        self.add(at, Some(z))
    }

    fn takes_heights(&self, _cx: &ToolContext<'_>) -> bool {
        true
    }

    // The dz of a typed @dx,dy,dz is the new vertex's height, as the PLINE3D
    // verb reads it (InteractiveTool::dz_is_a_change, left at its default):
    // one line, one polyline, in the window and through katana_cli.

    fn value(&mut self, cx: &ToolContext<'_>, text: &str) -> ToolStep {
        if self.asking_height {
            self.asking_height = false;
            if is_option(text, "None") {
                self.current = None;
                CURRENT_POLYLINE_HEIGHT.set(self.current);
                return ToolStep::next("new vertices have no height");
            }
            let Some(z) = parse_finite_double(trimmed_text(text)) else {
                self.asking_height = true;
                return ToolStep::rejected(format!(
                    "{} is not a height; type a number or None",
                    quoted(text)
                ));
            };
            self.current = Some(z);
            CURRENT_POLYLINE_HEIGHT.set(self.current);
            return ToolStep::next(format!("new vertices at height {}", number(z)));
        }
        if is_option(text, "Height") {
            self.asking_height = true;
            return ToolStep::next("");
        }
        if !self.vertices.is_empty() && is_option(text, "Close") {
            return self.finish(true);
        }
        if is_option(text, "Undo") {
            return self.undo(cx);
        }
        ToolStep::rejected(format!(
            "{} is not a point or an option; type x,y or x,y,z, H for the height, C to close or U \
             to undo",
            quoted(text)
        ))
    }

    fn enter(&mut self, _cx: &ToolContext<'_>) -> ToolStep {
        if self.asking_height {
            self.asking_height = false;
            return ToolStep::next("");
        }
        if self.vertices.len() < 2 {
            return ToolStep::done(None, "", false);
        }
        self.finish(false)
    }

    fn cancel(&mut self, cx: &ToolContext<'_>) -> ToolStep {
        keep_work_on_escape(self, cx)
    }

    fn undo(&mut self, _cx: &ToolContext<'_>) -> ToolStep {
        if self.asking_height {
            self.asking_height = false;
            return ToolStep::next("");
        }
        if self.vertices.pop().is_none() {
            return ToolStep::rejected("nothing to undo; no vertex has been given yet");
        }
        ToolStep::next("")
    }

    fn preview(&self, _cx: &ToolContext<'_>, cursor: Point2) -> ToolFeedback {
        let mut feedback = ToolFeedback::default();
        let mut points = Vec::new();
        for vertex in &self.vertices {
            points.push(vertex.position);
            feedback.markers.push(vertex.position);
        }
        if self
            .vertices
            .last()
            .is_some_and(|last| !coincide(cursor, last.position))
        {
            points.push(cursor);
        }
        if points.len() >= 2 {
            feedback
                .shapes
                .push(Geometry::Polyline(Polyline2::new(points, false)));
        }
        feedback
    }

    fn last_point(&self, _cx: &ToolContext<'_>) -> Option<Point2> {
        self.vertices.last().map(|vertex| vertex.position)
    }
}

// ---- Spline -----------------------------------------------------------------------------------
//
// Through the points given (Fit, the default) or with them as its control
// polygon (Control); [Degree] sets the degree, 3 unless changed.

struct SplineTool {
    attributes: EntityAttributes,
    points: Vec<Point2>,
    degree: i32,
    control: bool,
    asking_degree: bool,
}

impl SplineTool {
    fn new(attributes: &EntityAttributes) -> Self {
        Self {
            attributes: attributes.clone(),
            points: Vec::new(),
            degree: 3,
            control: false,
            asking_degree: false,
        }
    }

    fn build(&self, points: &[Point2]) -> Result<Spline2> {
        if self.control {
            Spline2::from_control_points(points.to_vec(), self.degree)
        } else {
            Spline2::through_points(points.to_vec(), self.degree)
        }
    }
}

impl InteractiveTool for SplineTool {
    fn prompt(&self, _cx: &ToolContext<'_>) -> String {
        if self.asking_degree {
            return format!("Specify the degree (1 to 10) <{}>", self.degree);
        }
        let mode = if self.control {
            "control point"
        } else {
            "fit point"
        };
        if self.points.is_empty() {
            return format!(
                "Specify first {mode} or [{}/Degree]",
                if self.control { "Fit" } else { "Control" }
            );
        }
        format!("Specify next {mode} or [Undo], Enter to finish")
    }

    fn expects(&self, _cx: &ToolContext<'_>) -> ToolInput {
        if self.asking_degree {
            ToolInput::Value
        } else {
            ToolInput::Point
        }
    }

    fn point(&mut self, _cx: &ToolContext<'_>, at: Point2) -> ToolStep {
        if self.points.last().is_some_and(|&last| coincide(at, last)) {
            return ToolStep::rejected("the point is where the last one is");
        }
        self.points.push(at);
        ToolStep::next("")
    }

    fn value(&mut self, cx: &ToolContext<'_>, text: &str) -> ToolStep {
        if self.asking_degree {
            let degree =
                parse_integer(trimmed_text(text)).filter(|degree| (1..=10).contains(degree));
            let Some(degree) = degree else {
                return ToolStep::rejected(format!(
                    "{} is not a degree from 1 to 10",
                    quoted(text)
                ));
            };
            // From 1 to 10, so it fits.
            self.degree = degree as i32;
            self.asking_degree = false;
            return ToolStep::next("");
        }
        if self.points.is_empty() && is_option(text, "Control") {
            self.control = true;
            return ToolStep::next("");
        }
        if self.points.is_empty() && is_option(text, "Fit") {
            self.control = false;
            return ToolStep::next("");
        }
        if is_option(text, "Degree") {
            self.asking_degree = true;
            return ToolStep::next("");
        }
        if is_option(text, "Undo") {
            return self.undo(cx);
        }
        ToolStep::rejected(format!("{} is not a point or an option", quoted(text)))
    }

    fn enter(&mut self, _cx: &ToolContext<'_>) -> ToolStep {
        if self.asking_degree {
            self.asking_degree = false;
            return ToolStep::next("");
        }
        if self.points.len() < 2 {
            return ToolStep::done(None, "", false);
        }
        let spline = match self.build(&self.points) {
            Ok(spline) => spline,
            Err(error) => return ToolStep::rejected(error.message),
        };
        let count = self.points.len();
        self.points.clear();
        ToolStep::done(
            Some(create_drawn(
                "CREATE_SPLINE",
                vec![drawn_entity(Geometry::Spline(spline), &self.attributes)],
            )),
            format!("spline through {count} points"),
            true,
        )
    }

    fn cancel(&mut self, cx: &ToolContext<'_>) -> ToolStep {
        keep_work_on_escape(self, cx)
    }

    fn undo(&mut self, _cx: &ToolContext<'_>) -> ToolStep {
        if self.asking_degree {
            self.asking_degree = false;
            return ToolStep::next("");
        }
        if self.points.pop().is_none() {
            return ToolStep::rejected("nothing to undo");
        }
        ToolStep::next("")
    }

    fn preview(&self, _cx: &ToolContext<'_>, cursor: Point2) -> ToolFeedback {
        let mut feedback = ToolFeedback {
            markers: self.points.clone(),
            ..ToolFeedback::default()
        };
        let mut points = self.points.clone();
        if points.last().is_some_and(|&last| !coincide(last, cursor)) {
            points.push(cursor);
        }
        if points.len() >= 2 {
            if let Ok(spline) = self.build(&points) {
                feedback.shapes.push(Geometry::Spline(spline));
            }
            if self.control {
                feedback
                    .shapes
                    .push(Geometry::Polyline(Polyline2::new(points, false)));
            }
        }
        feedback
    }

    fn last_point(&self, _cx: &ToolContext<'_>) -> Option<Point2> {
        self.points.last().copied()
    }
}

// ---- Ellipse, and the elliptical arc -----------------------------------------------------------
//
// Axis and End: the two ends of one axis, then the other axis's half length
// (a point - its distance from the first axis - or a typed number). Centre:
// the centre, one axis's end, the other half length. Arc: an ellipse as Axis
// and End, then the start and end, each a point (its direction from the
// centre) or an angle typed in degrees of the ellipse's own parameter.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EllipseVariant {
    AxisEnd,
    Centre,
    Arc,
}

struct EllipseTool {
    attributes: EntityAttributes,
    variant: EllipseVariant,
    points: Vec<Point2>,
    ellipse: Option<Ellipse2>,
    start: Option<f64>,
}

impl EllipseTool {
    fn new(attributes: &EntityAttributes, variant: EllipseVariant) -> Self {
        Self {
            attributes: attributes.clone(),
            variant,
            points: Vec::new(),
            ellipse: None,
            start: None,
        }
    }

    fn axis(&self) -> (Point2, Point2) {
        if self.variant == EllipseVariant::Centre {
            return (self.points[0], self.points[1]);
        }
        ((self.points[0] + self.points[1]) * 0.5, self.points[1])
    }

    fn other(&mut self, radius: f64) -> ToolStep {
        let (centre, axis_end) = self.axis();
        let Some(shape) = Ellipse2::from_axes(centre, axis_end, radius) else {
            return ToolStep::rejected("the other axis needs a length greater than zero");
        };
        if self.variant != EllipseVariant::Arc {
            return self.make(shape, "ellipse");
        }
        self.ellipse = Some(shape);
        ToolStep::next("")
    }

    // `ellipse` is the one the arc is on: the C++ reads *ellipse_, which is
    // set whenever this is reached.
    fn arc_end(&mut self, ellipse: Ellipse2, parameter: f64) -> ToolStep {
        let Some(start) = self.start else {
            self.start = Some(parameter);
            return ToolStep::next("");
        };
        let mut shape = ellipse;
        shape.start_parameter = start;
        shape.sweep = normalize_angle(parameter - start);
        if !(shape.sweep > tolerance::ANGULAR) {
            return ToolStep::rejected("the arc's end is its start; pick another end");
        }
        self.make(shape, "elliptical arc")
    }

    fn make(&mut self, shape: Ellipse2, what: &str) -> ToolStep {
        self.points.clear();
        self.ellipse = None;
        self.start = None;
        ToolStep::done(
            Some(create_drawn(
                "CREATE_ELLIPSE",
                vec![drawn_entity(Geometry::Ellipse(shape), &self.attributes)],
            )),
            what,
            true,
        )
    }
}

impl InteractiveTool for EllipseTool {
    fn prompt(&self, _cx: &ToolContext<'_>) -> String {
        match self.points.len() {
            0 => {
                return String::from(if self.variant == EllipseVariant::Centre {
                    "Specify centre of ellipse"
                } else {
                    "Specify axis endpoint of ellipse"
                });
            }
            1 => {
                return String::from(if self.variant == EllipseVariant::Centre {
                    "Specify endpoint of axis"
                } else {
                    "Specify other endpoint of axis"
                });
            }
            _ => {}
        }
        if self.ellipse.is_none() {
            return String::from("Specify distance to other axis (a point or a length)");
        }
        String::from(if self.start.is_some() {
            "Specify end of arc (a point or an angle)"
        } else {
            "Specify start of arc (a point or an angle)"
        })
    }

    fn expects(&self, _cx: &ToolContext<'_>) -> ToolInput {
        ToolInput::Point
    }

    fn point(&mut self, _cx: &ToolContext<'_>, at: Point2) -> ToolStep {
        if self.points.len() < 2 {
            if self.points.last().is_some_and(|&last| coincide(at, last)) {
                return ToolStep::rejected("an axis needs two different points");
            }
            self.points.push(at);
            return ToolStep::next("");
        }
        let Some(ellipse) = self.ellipse else {
            let (centre, axis_end) = self.axis();
            let line = Line2::through(centre, axis_end);
            return self.other(line.map_or(0.0, |line| line.distance_to(at)));
        };
        self.arc_end(ellipse, ellipse.parameter_towards(at))
    }

    fn value(&mut self, cx: &ToolContext<'_>, text: &str) -> ToolStep {
        if is_option(text, "Undo") {
            return self.undo(cx);
        }
        let typed = parse_finite_double(trimmed_text(text));
        if self.points.len() == 2
            && self.ellipse.is_none()
            && let Some(typed) = typed
        {
            return self.other(typed);
        }
        if let Some(ellipse) = self.ellipse
            && let Some(typed) = typed
        {
            return self.arc_end(ellipse, typed * DEG_TO_RAD);
        }
        ToolStep::rejected(format!("{} is not a point or a number here", quoted(text)))
    }

    fn undo(&mut self, _cx: &ToolContext<'_>) -> ToolStep {
        if self.start.is_some() {
            self.start = None;
        } else if self.ellipse.is_some() {
            self.ellipse = None;
        } else if self.points.pop().is_none() {
            return ToolStep::rejected("nothing to undo");
        }
        ToolStep::next("")
    }

    fn preview(&self, _cx: &ToolContext<'_>, cursor: Point2) -> ToolFeedback {
        let mut feedback = ToolFeedback {
            markers: self.points.clone(),
            ..ToolFeedback::default()
        };
        if self.points.len() == 1 {
            feedback
                .shapes
                .push(Geometry::Line(Segment2::new(self.points[0], cursor)));
        } else if self.points.len() == 2 && self.ellipse.is_none() {
            let (centre, axis_end) = self.axis();
            if let Some(line) = Line2::through(centre, axis_end)
                && let Some(shape) = Ellipse2::from_axes(centre, axis_end, line.distance_to(cursor))
            {
                feedback.shapes.push(Geometry::Ellipse(shape));
            }
        } else if let Some(ellipse) = self.ellipse {
            let mut shape = ellipse;
            let t = ellipse.parameter_towards(cursor);
            if let Some(start) = self.start {
                shape.start_parameter = start;
                shape.sweep = normalize_angle(t - start);
                if shape.sweep > tolerance::ANGULAR {
                    feedback.shapes.push(Geometry::Ellipse(shape));
                }
            } else {
                feedback.shapes.push(Geometry::Ellipse(shape));
                feedback.shapes.push(Geometry::Line(Segment2::new(
                    shape.center,
                    shape.point_at_parameter(t),
                )));
            }
        }
        feedback
    }

    fn last_point(&self, _cx: &ToolContext<'_>) -> Option<Point2> {
        self.points.last().copied()
    }
}

// ---- Construction Line and Ray -----------------------------------------------------------------
//
// A base point, then through points, each making one line (both ways) or ray
// (away from the base), until Enter - as XLINE and RAY do. [Hor] and [Ver]
// make horizontal and vertical ones through each point picked. They go on
// the construction layer (drawing/construction.rs), made if the drawing has
// none, which is never plotted.

struct ConstructionTool {
    attributes: EntityAttributes,
    ray: bool,
    base: Option<Point2>,
    fixed: Option<f64>,
    lines: Vec<Segment2>,
}

impl ConstructionTool {
    fn new(attributes: &EntityAttributes, ray: bool) -> Self {
        Self {
            attributes: attributes.clone(),
            ray,
            base: None,
            fixed: None,
            lines: Vec::new(),
        }
    }

    fn line(&self, through: Point2, direction: Vec2) -> Segment2 {
        construction_segment(through, direction, self.ray)
    }

    fn make(&mut self, cx: &ToolContext<'_>) -> ToolStep {
        let none = Model::default();
        let model = cx.document.map_or(&none, |document| document.model());
        let transaction =
            create_construction(model, self.attributes.clone(), &self.lines, self.ray);
        let count = self.lines.len();
        self.lines.clear();
        ToolStep::done(
            Some(transaction),
            format!(
                "{count}{}{}",
                if self.ray {
                    " ray"
                } else {
                    " construction line"
                },
                if count == 1 { "" } else { "s" }
            ),
            false,
        )
    }
}

impl InteractiveTool for ConstructionTool {
    fn prompt(&self, _cx: &ToolContext<'_>) -> String {
        if self.base.is_none()
            && let Some(fixed) = self.fixed
        {
            return format!(
                "Specify a point for the {} line, Enter to finish",
                if fixed == 0.0 {
                    "horizontal"
                } else {
                    "vertical"
                }
            );
        }
        if self.base.is_none() {
            return String::from(if self.ray {
                "Specify start point"
            } else {
                "Specify a point or [Hor/Ver]"
            });
        }
        String::from("Specify through point, Enter to finish")
    }

    fn expects(&self, _cx: &ToolContext<'_>) -> ToolInput {
        ToolInput::Point
    }

    fn point(&mut self, _cx: &ToolContext<'_>, at: Point2) -> ToolStep {
        if let Some(fixed) = self.fixed {
            let made = self.line(at, Vec2::new(fixed.cos(), fixed.sin()));
            self.lines.push(made);
            return ToolStep::next("");
        }
        let Some(base) = self.base else {
            self.base = Some(at);
            return ToolStep::next("");
        };
        if coincide(at, base) {
            return ToolStep::rejected("the through point is the base point");
        }
        let made = self.line(base, (at - base).normalized());
        self.lines.push(made);
        ToolStep::next("")
    }

    fn value(&mut self, _cx: &ToolContext<'_>, text: &str) -> ToolStep {
        if !self.ray && self.base.is_none() && is_option(text, "Hor") {
            self.fixed = Some(0.0);
            return ToolStep::next("");
        }
        if !self.ray && self.base.is_none() && is_option(text, "Ver") {
            self.fixed = Some(HALF_PI);
            return ToolStep::next("");
        }
        ToolStep::rejected(format!("{} is not a point or an option", quoted(text)))
    }

    // Every line made since the tool started, as ONE command.
    fn enter(&mut self, cx: &ToolContext<'_>) -> ToolStep {
        if self.lines.is_empty() {
            return ToolStep::done(None, "", false);
        }
        self.make(cx)
    }

    fn cancel(&mut self, cx: &ToolContext<'_>) -> ToolStep {
        keep_work_on_escape(self, cx)
    }

    fn undo(&mut self, _cx: &ToolContext<'_>) -> ToolStep {
        if self.lines.pop().is_some() {
            return ToolStep::next("");
        }
        if self.base.take().is_some() {
            return ToolStep::next("");
        }
        if self.fixed.take().is_some() {
            return ToolStep::next("");
        }
        ToolStep::rejected("nothing to undo")
    }

    fn preview(&self, _cx: &ToolContext<'_>, cursor: Point2) -> ToolFeedback {
        let mut feedback = ToolFeedback::default();
        for made in &self.lines {
            feedback.shapes.push(Geometry::Line(*made));
        }
        if let Some(fixed) = self.fixed {
            let direction = Vec2::new(fixed.cos(), fixed.sin());
            feedback
                .shapes
                .push(Geometry::Line(self.line(cursor, direction)));
        } else if let Some(base) = self.base
            && !coincide(cursor, base)
        {
            feedback.markers.push(base);
            feedback.shapes.push(Geometry::Line(
                self.line(base, (cursor - base).normalized()),
            ));
        }
        feedback
    }

    fn last_point(&self, _cx: &ToolContext<'_>) -> Option<Point2> {
        self.base
    }
}

// ---- Double Line --------------------------------------------------------------------------------
//
// A path picked as a polyline is, drawn as the two polylines either side of
// it at half the width; [Width] sets the width (remembered). Enter or Close
// finishes.

thread_local! {
    static DOUBLE_LINE_WIDTH: Cell<f64> = const { Cell::new(1.0) };
}

fn sides(path: &Polyline2) -> Vec<Polyline2> {
    double_line_sides(path, DOUBLE_LINE_WIDTH.get())
}

struct DoubleLineTool {
    attributes: EntityAttributes,
    points: Vec<Point2>,
    asking_width: bool,
}

impl DoubleLineTool {
    fn new(attributes: &EntityAttributes) -> Self {
        Self {
            attributes: attributes.clone(),
            points: Vec::new(),
            asking_width: false,
        }
    }

    fn finish(&mut self, closed: bool) -> ToolStep {
        let made = sides(&Polyline2::new(self.points.clone(), closed));
        if made.len() != 2 {
            return ToolStep::rejected("the path is too tight for a double line of this width");
        }
        let entities = made
            .into_iter()
            .map(|side| drawn_entity(Geometry::Polyline(side), &self.attributes))
            .collect();
        self.points.clear();
        ToolStep::done(
            Some(create_drawn("CREATE_DOUBLE_LINE", entities)),
            format!("double line of width {}", number(DOUBLE_LINE_WIDTH.get())),
            true,
        )
    }
}

impl InteractiveTool for DoubleLineTool {
    fn prompt(&self, _cx: &ToolContext<'_>) -> String {
        if self.asking_width {
            return format!("Specify the width <{}>", number(DOUBLE_LINE_WIDTH.get()));
        }
        format!(
            "{} or [Width{}/Undo] <width {}>",
            if self.points.is_empty() {
                "Specify start point"
            } else {
                "Specify next point"
            },
            if self.points.len() >= 3 { "/Close" } else { "" },
            number(DOUBLE_LINE_WIDTH.get())
        )
    }

    fn expects(&self, _cx: &ToolContext<'_>) -> ToolInput {
        if self.asking_width {
            ToolInput::Value
        } else {
            ToolInput::Point
        }
    }

    fn point(&mut self, _cx: &ToolContext<'_>, at: Point2) -> ToolStep {
        if self.points.last().is_some_and(|&last| coincide(at, last)) {
            return ToolStep::rejected("the point is where the last one is");
        }
        self.points.push(at);
        ToolStep::next("")
    }

    fn value(&mut self, cx: &ToolContext<'_>, text: &str) -> ToolStep {
        if self.asking_width {
            let width = parse_finite_double(trimmed_text(text))
                .filter(|&width| width > tolerance::GEOMETRIC);
            let Some(width) = width else {
                return ToolStep::rejected(format!(
                    "{} is not a width greater than zero",
                    quoted(text)
                ));
            };
            DOUBLE_LINE_WIDTH.set(width);
            self.asking_width = false;
            return ToolStep::next("");
        }
        if is_option(text, "Width") {
            self.asking_width = true;
            return ToolStep::next("");
        }
        if self.points.len() >= 3 && is_option(text, "Close") {
            return self.finish(true);
        }
        if is_option(text, "Undo") {
            return self.undo(cx);
        }
        ToolStep::rejected(format!("{} is not a point or an option", quoted(text)))
    }

    fn enter(&mut self, _cx: &ToolContext<'_>) -> ToolStep {
        if self.asking_width {
            self.asking_width = false;
            return ToolStep::next("");
        }
        if self.points.len() < 2 {
            return ToolStep::done(None, "", false);
        }
        self.finish(false)
    }

    fn cancel(&mut self, cx: &ToolContext<'_>) -> ToolStep {
        keep_work_on_escape(self, cx)
    }

    fn undo(&mut self, _cx: &ToolContext<'_>) -> ToolStep {
        if self.asking_width {
            self.asking_width = false;
            return ToolStep::next("");
        }
        if self.points.pop().is_none() {
            return ToolStep::rejected("nothing to undo");
        }
        ToolStep::next("")
    }

    fn preview(&self, _cx: &ToolContext<'_>, cursor: Point2) -> ToolFeedback {
        let mut feedback = ToolFeedback::default();
        let mut path = self.points.clone();
        if path.last().is_some_and(|&last| !coincide(last, cursor)) {
            path.push(cursor);
        }
        for side in sides(&Polyline2::new(path, false)) {
            feedback.shapes.push(Geometry::Polyline(side));
        }
        feedback.markers = self.points.clone();
        feedback
    }

    fn last_point(&self, _cx: &ToolContext<'_>) -> Option<Point2> {
        self.points.last().copied()
    }
}

// ---- Freehand Sketch ----------------------------------------------------------------------------
//
// A click puts the pen down and the next lifts it; between them the cursor's
// path is recorded, and each stroke is weeded to the tolerance ([Tolerance],
// by default a quarter of the view's pick aperture, so a sketch is as fine as
// the zoom it was drawn at). Enter keeps the strokes, one polyline each.
//
// The path is recorded from preview(), which the view calls with the cursor
// on every move: the tool interface has no move event of its own, and
// preview is the one call that sees the cursor between clicks. The recording
// is the tool's own state, so it is a RefCell there (the C++'s mutable).

struct SketchTool {
    attributes: EntityAttributes,
    tolerance: f64,
    pen_down: bool,
    asking_tolerance: bool,
    stroke: RefCell<Vec<Point2>>,
    strokes: Vec<Vec<Point2>>,
}

impl SketchTool {
    fn new(cx: &ToolContext<'_>, attributes: &EntityAttributes) -> Self {
        Self {
            attributes: attributes.clone(),
            tolerance: if cx.pick_tolerance > 0.0 {
                cx.pick_tolerance * 0.25
            } else {
                0.01
            },
            pen_down: false,
            asking_tolerance: false,
            stroke: RefCell::new(Vec::new()),
            strokes: Vec::new(),
        }
    }

    // For the tests, which have no mouse: the pen moving through `at`.
    fn record_move(&self, at: Point2) {
        self.record(at);
    }

    fn record(&self, at: Point2) {
        let mut stroke = self.stroke.borrow_mut();
        if stroke
            .last()
            .is_none_or(|last| last.distance_to(at) > tolerance::GEOMETRIC)
        {
            stroke.push(at);
        }
    }
}

impl InteractiveTool for SketchTool {
    fn prompt(&self, _cx: &ToolContext<'_>) -> String {
        if self.asking_tolerance {
            return format!("Specify the sketch tolerance <{}>", number(self.tolerance));
        }
        String::from(if self.pen_down {
            "Move to sketch; click to lift the pen"
        } else {
            "Click to put the pen down, or [Tolerance]; Enter keeps the sketch"
        })
    }

    fn expects(&self, _cx: &ToolContext<'_>) -> ToolInput {
        if self.asking_tolerance {
            ToolInput::Value
        } else {
            ToolInput::Point
        }
    }

    fn point(&mut self, _cx: &ToolContext<'_>, at: Point2) -> ToolStep {
        if !self.pen_down {
            self.pen_down = true;
            *self.stroke.get_mut() = vec![at];
            return ToolStep::next("");
        }
        self.record(at);
        self.pen_down = false;
        let stroke = std::mem::take(self.stroke.get_mut());
        if stroke.len() >= 2 {
            self.strokes.push(stroke);
        }
        ToolStep::next("")
    }

    fn value(&mut self, _cx: &ToolContext<'_>, text: &str) -> ToolStep {
        if self.asking_tolerance {
            let typed = parse_finite_double(trimmed_text(text)).filter(|&typed| typed > 0.0);
            let Some(typed) = typed else {
                return ToolStep::rejected(format!(
                    "{} is not a tolerance greater than zero",
                    quoted(text)
                ));
            };
            self.tolerance = typed;
            self.asking_tolerance = false;
            return ToolStep::next("");
        }
        if is_option(text, "Tolerance") {
            self.asking_tolerance = true;
            return ToolStep::next("");
        }
        ToolStep::rejected(format!("{} is not an option here", quoted(text)))
    }

    fn enter(&mut self, _cx: &ToolContext<'_>) -> ToolStep {
        if self.asking_tolerance {
            self.asking_tolerance = false;
            return ToolStep::next("");
        }
        let stroke = std::mem::take(self.stroke.get_mut());
        if self.pen_down && stroke.len() >= 2 {
            self.strokes.push(stroke);
        }
        self.pen_down = false;
        let mut made = Vec::new();
        for stroke in &self.strokes {
            if let Ok(weeded) = weed(
                &CurvePolyline2::from_points(stroke, false),
                self.tolerance,
                &[],
            ) && weeded.vertices.len() >= 2
            {
                let line = Geometry::Polyline(Polyline2::new(weeded.positions(), false));
                if validate(&line).is_ok() {
                    made.push(drawn_entity(line, &self.attributes));
                }
            }
        }
        self.strokes.clear();
        if made.is_empty() {
            return ToolStep::done(None, "", false);
        }
        let count = made.len();
        ToolStep::done(
            Some(create_drawn("CREATE_SKETCH", made)),
            format!("{count}{}", if count == 1 { " stroke" } else { " strokes" }),
            false,
        )
    }

    fn cancel(&mut self, cx: &ToolContext<'_>) -> ToolStep {
        keep_work_on_escape(self, cx)
    }

    fn undo(&mut self, _cx: &ToolContext<'_>) -> ToolStep {
        if self.pen_down {
            self.pen_down = false;
            self.stroke.get_mut().clear();
            return ToolStep::next("");
        }
        if self.strokes.pop().is_none() {
            return ToolStep::rejected("nothing to undo");
        }
        ToolStep::next("")
    }

    fn preview(&self, _cx: &ToolContext<'_>, cursor: Point2) -> ToolFeedback {
        if self.pen_down {
            self.record(cursor);
        }
        let mut feedback = ToolFeedback::default();
        for stroke in &self.strokes {
            feedback
                .shapes
                .push(Geometry::Polyline(Polyline2::new(stroke.clone(), false)));
        }
        let stroke = self.stroke.borrow();
        if stroke.len() >= 2 {
            feedback
                .shapes
                .push(Geometry::Polyline(Polyline2::new(stroke.clone(), false)));
        }
        feedback
    }

    fn last_point(&self, _cx: &ToolContext<'_>) -> Option<Point2> {
        self.stroke.borrow().last().copied()
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn Any> {
        Some(self)
    }
}

// ---- Revision Cloud -----------------------------------------------------------------------------
//
// The outline picked point by point (or a [Rectangle] by two corners), then
// drawn as a closed curve polyline of arcs about [Arc] length long, bulging
// outwards. Enter or Close finishes.

thread_local! {
    static CLOUD_ARC_LENGTH: Cell<f64> = const { Cell::new(1.0) };
}

fn cloud_around(outline: &[Point2], arc_length: f64) -> CurvePolyline2 {
    let mut ring = CurvePolyline2 {
        vertices: Vec::new(),
        closed: true,
    };
    let polygon = Polyline2::new(outline.to_vec(), true);
    // Outwards is to the right of a counter-clockwise ring, which a positive
    // bulge bulges to; to the left of a clockwise one.
    let bulge = if polygon.signed_area() >= 0.0 {
        0.6
    } else {
        -0.6
    };
    for (i, &a) in outline.iter().enumerate() {
        let b = outline[(i + 1) % outline.len()];
        let length = a.distance_to(b);
        // A length that is not negative, so the cast only truncates what the
        // C++'s static_cast<size_t> truncates.
        let pieces = ((length / arc_length).round() as usize).max(1);
        for k in 0..pieces {
            ring.vertices.push(CurveVertex {
                position: a + (b - a) * (k as f64 / pieces as f64),
                bulge,
                height: None,
            });
        }
    }
    ring
}

struct RevisionCloudTool {
    attributes: EntityAttributes,
    points: Vec<Point2>,
    rectangle: bool,
    asking_length: bool,
}

impl RevisionCloudTool {
    fn new(attributes: &EntityAttributes) -> Self {
        Self {
            attributes: attributes.clone(),
            points: Vec::new(),
            rectangle: false,
            asking_length: false,
        }
    }

    fn finish(&mut self) -> ToolStep {
        let cloud = cloud_around(&self.points, CLOUD_ARC_LENGTH.get());
        self.points.clear();
        self.rectangle = false;
        let arcs = cloud.vertices.len();
        let geometry = Geometry::CurvePolyline(cloud);
        if let Err(error) = validate(&geometry) {
            return ToolStep::rejected(error.message);
        }
        ToolStep::done(
            Some(create_drawn(
                "CREATE_REVISION_CLOUD",
                vec![drawn_entity(geometry, &self.attributes)],
            )),
            format!("revision cloud of {arcs} arcs"),
            true,
        )
    }
}

impl InteractiveTool for RevisionCloudTool {
    fn prompt(&self, _cx: &ToolContext<'_>) -> String {
        if self.asking_length {
            return format!(
                "Specify the arc length <{}>",
                number(CLOUD_ARC_LENGTH.get())
            );
        }
        if self.rectangle {
            return String::from(if self.points.is_empty() {
                "Specify first corner"
            } else {
                "Specify other corner"
            });
        }
        format!(
            "{}{}",
            if self.points.is_empty() {
                "Specify start point or [Arc/Rectangle]"
            } else {
                "Specify next point or [Arc/Undo]"
            },
            if self.points.len() >= 3 {
                ", Enter or C to close"
            } else {
                ""
            }
        )
    }

    fn expects(&self, _cx: &ToolContext<'_>) -> ToolInput {
        if self.asking_length {
            ToolInput::Value
        } else {
            ToolInput::Point
        }
    }

    fn point(&mut self, _cx: &ToolContext<'_>, at: Point2) -> ToolStep {
        if self.points.last().is_some_and(|&last| coincide(at, last)) {
            return ToolStep::rejected("the point is where the last one is");
        }
        if self.rectangle && !self.points.is_empty() {
            let corners = Rectangle2::from_corners(self.points[0], at);
            if !(corners.width > tolerance::GEOMETRIC && corners.height > tolerance::GEOMETRIC) {
                return ToolStep::rejected("the corners are in line");
            }
            self.points = corners.corners().to_vec();
            return self.finish();
        }
        self.points.push(at);
        ToolStep::next("")
    }

    fn value(&mut self, cx: &ToolContext<'_>, text: &str) -> ToolStep {
        if self.asking_length {
            let length = parse_finite_double(trimmed_text(text))
                .filter(|&length| length > tolerance::GEOMETRIC);
            let Some(length) = length else {
                return ToolStep::rejected(format!(
                    "{} is not a length greater than zero",
                    quoted(text)
                ));
            };
            CLOUD_ARC_LENGTH.set(length);
            self.asking_length = false;
            return ToolStep::next("");
        }
        if is_option(text, "Arc") {
            self.asking_length = true;
            return ToolStep::next("");
        }
        if self.points.is_empty() && is_option(text, "Rectangle") {
            self.rectangle = true;
            return ToolStep::next("");
        }
        if self.points.len() >= 3 && is_option(text, "Close") {
            return self.finish();
        }
        if is_option(text, "Undo") {
            return self.undo(cx);
        }
        ToolStep::rejected(format!("{} is not a point or an option", quoted(text)))
    }

    fn enter(&mut self, _cx: &ToolContext<'_>) -> ToolStep {
        if self.asking_length {
            self.asking_length = false;
            return ToolStep::next("");
        }
        if self.points.len() >= 3 {
            return self.finish();
        }
        ToolStep::done(None, "", false)
    }

    fn undo(&mut self, _cx: &ToolContext<'_>) -> ToolStep {
        if self.asking_length {
            self.asking_length = false;
        } else if !self.points.is_empty() {
            self.points.pop();
        } else if self.rectangle {
            self.rectangle = false;
        } else {
            return ToolStep::rejected("nothing to undo");
        }
        ToolStep::next("")
    }

    fn preview(&self, _cx: &ToolContext<'_>, cursor: Point2) -> ToolFeedback {
        let mut feedback = ToolFeedback {
            markers: self.points.clone(),
            ..ToolFeedback::default()
        };
        let mut outline = self.points.clone();
        if self.rectangle && self.points.len() == 1 {
            outline = Rectangle2::from_corners(self.points[0], cursor)
                .corners()
                .to_vec();
        } else if outline.last().is_some_and(|&last| !coincide(last, cursor)) {
            outline.push(cursor);
        }
        if outline.len() >= 3 {
            feedback.shapes.push(Geometry::CurvePolyline(cloud_around(
                &outline,
                CLOUD_ARC_LENGTH.get(),
            )));
        } else if outline.len() == 2 {
            feedback
                .shapes
                .push(Geometry::Line(Segment2::new(outline[0], outline[1])));
        }
        feedback
    }

    fn last_point(&self, _cx: &ToolContext<'_>) -> Option<Point2> {
        self.points.last().copied()
    }
}

// ---- Circle, Tangent Tangent Tangent; Arc, Start End Direction -------------------------------------
//
// The two constructions the Circle and Arc families lacked, named into those
// families so the menus put them in the same submenus.
//
// TTT: three lines picked (lines, or the straight segment of a polyline
// nearest the pick); of the four circles touching all three (the triangle's
// incircle and its three excircles) the one whose touching points lie
// nearest the three picks.

fn picked_line(document: Option<&Document>, id: EntityId, at: Point2) -> Option<Segment2> {
    let entity = document?.model().entities.find(id)?;
    if let Geometry::Line(line) = &entity.geometry {
        return Some(*line);
    }
    let polyline = read_polyline(entity)?;
    let segment = nearest_segment(&polyline, at)?;
    match polyline.segment(segment) {
        CurveSegment::Segment(straight) => Some(straight),
        CurveSegment::Arc(_) => None,
    }
}

type Matrix3 = [[f64; 3]; 3];

fn determinant(a: &Matrix3) -> f64 {
    a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1])
        - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
        + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0])
}

#[allow(
    clippy::needless_range_loop,
    reason = "Cramer's rule indexes the rows and columns of its matrices, as the C++ does."
)]
fn tangent_to_three(lines: &[Segment2; 3], picks: &[Point2; 3]) -> Option<Circle2> {
    let mut best = None;
    let mut best_score = f64::INFINITY;
    for signs in 0..8 {
        // n_i . c + k_i = s_i r for unit normals n_i: three equations in
        // (cx, cy, r), solved by Cramer's rule.
        let mut m: Matrix3 = [[0.0; 3]; 3];
        let mut rhs = [0.0; 3];
        for i in 0..3 {
            let n = lines[i].delta().normalized().perpendicular();
            let k = -n.dot(lines[i].start);
            let s = if ((signs >> i) & 1) != 0 { 1.0 } else { -1.0 };
            m[i][0] = n.x;
            m[i][1] = n.y;
            m[i][2] = -s;
            rhs[i] = -k;
        }
        let d = determinant(&m);
        if d.abs() < 1e-12 {
            continue;
        }
        let mut solution = [0.0; 3];
        for column in 0..3 {
            let mut replaced: Matrix3 = [[0.0; 3]; 3];
            for i in 0..3 {
                for j in 0..3 {
                    replaced[i][j] = if j == column { rhs[i] } else { m[i][j] };
                }
            }
            solution[column] = determinant(&replaced) / d;
        }
        let r = solution[2];
        if !(r > tolerance::GEOMETRIC) {
            continue;
        }
        let c = Point2::new(solution[0], solution[1]);
        let mut score = 0.0;
        for i in 0..3 {
            let line = Line2 {
                origin: lines[i].start,
                direction: lines[i].delta(),
            };
            score += line.closest_point(c).distance_to(picks[i]);
        }
        if score < best_score {
            best_score = score;
            best = Some(Circle2::new(c, r));
        }
    }
    best
}

struct CircleTangentsTool {
    attributes: EntityAttributes,
    lines: Vec<Segment2>,
    picks: Vec<Point2>,
}

impl CircleTangentsTool {
    fn new(attributes: &EntityAttributes) -> Self {
        Self {
            attributes: attributes.clone(),
            lines: Vec::new(),
            picks: Vec::new(),
        }
    }
}

impl InteractiveTool for CircleTangentsTool {
    fn prompt(&self, _cx: &ToolContext<'_>) -> String {
        const ORDINAL: [&str; 3] = ["first", "second", "third"];
        format!(
            "Select the {} line the circle touches",
            ORDINAL[self.lines.len().min(2)]
        )
    }

    fn expects(&self, _cx: &ToolContext<'_>) -> ToolInput {
        ToolInput::Entity
    }

    fn entity(&mut self, cx: &ToolContext<'_>, id: EntityId, at: Point2) -> ToolStep {
        let Some(line) = picked_line(cx.document, id, at).filter(|line| !line.is_degenerate())
        else {
            return ToolStep::rejected("pick a line, or a straight segment of a polyline");
        };
        self.lines.push(line);
        self.picks.push(at);
        if self.lines.len() < 3 {
            return ToolStep::next("");
        }
        let circle = tangent_to_three(
            &[self.lines[0], self.lines[1], self.lines[2]],
            &[self.picks[0], self.picks[1], self.picks[2]],
        );
        self.lines.clear();
        self.picks.clear();
        let Some(circle) = circle else {
            return ToolStep::rejected("no circle touches those three lines (two are parallel?)");
        };
        ToolStep::done(
            Some(create_drawn(
                "CREATE_CIRCLE",
                vec![drawn_entity(Geometry::Circle(circle), &self.attributes)],
            )),
            format!("circle of radius {}", number(circle.radius)),
            true,
        )
    }

    fn undo(&mut self, _cx: &ToolContext<'_>) -> ToolStep {
        if self.lines.pop().is_none() {
            return ToolStep::rejected("nothing to undo");
        }
        self.picks.pop();
        ToolStep::next("")
    }
}

// Start End Direction: the start, the end, then the direction the arc leaves
// its start - a point it heads towards, or an angle typed in degrees
// counter-clockwise from east.
struct ArcDirectionTool {
    attributes: EntityAttributes,
    points: Vec<Point2>,
}

impl ArcDirectionTool {
    fn new(attributes: &EntityAttributes) -> Self {
        Self {
            attributes: attributes.clone(),
            points: Vec::new(),
        }
    }

    fn arc_for(&self, direction: f64) -> Option<Arc2> {
        let tangent = Vec2::new(direction.cos(), direction.sin());
        let chord = self.points[1] - self.points[0];
        let half = tangent.cross(chord).atan2(tangent.dot(chord));
        arc_from_bulge(self.points[0], self.points[1], bulge_from_sweep(2.0 * half))
    }

    fn make(&mut self, direction: f64) -> ToolStep {
        let Some(arc) = self.arc_for(direction) else {
            return ToolStep::rejected(
                "that direction points straight at the end; there is no arc",
            );
        };
        self.points.clear();
        ToolStep::done(
            Some(create_drawn(
                "CREATE_ARC",
                vec![drawn_entity(Geometry::Arc(arc), &self.attributes)],
            )),
            "arc",
            true,
        )
    }
}

impl InteractiveTool for ArcDirectionTool {
    fn prompt(&self, _cx: &ToolContext<'_>) -> String {
        String::from(match self.points.len() {
            0 => "Specify start point of arc",
            1 => "Specify end point of arc",
            _ => "Specify tangent direction at the start (a point or an angle)",
        })
    }

    fn expects(&self, _cx: &ToolContext<'_>) -> ToolInput {
        ToolInput::Point
    }

    fn point(&mut self, _cx: &ToolContext<'_>, at: Point2) -> ToolStep {
        if self.points.last().is_some_and(|&last| coincide(at, last)) {
            return ToolStep::rejected("the point is where the last one is");
        }
        if self.points.len() < 2 {
            self.points.push(at);
            return ToolStep::next("");
        }
        self.make((at - self.points[0]).angle())
    }

    fn value(&mut self, _cx: &ToolContext<'_>, text: &str) -> ToolStep {
        let degrees = parse_finite_double(trimmed_text(text));
        if self.points.len() == 2
            && let Some(degrees) = degrees
        {
            return self.make(degrees * DEG_TO_RAD);
        }
        ToolStep::rejected(format!("{} is not taken here", quoted(text)))
    }

    fn undo(&mut self, _cx: &ToolContext<'_>) -> ToolStep {
        if self.points.pop().is_none() {
            return ToolStep::rejected("nothing to undo");
        }
        ToolStep::next("")
    }

    fn preview(&self, _cx: &ToolContext<'_>, cursor: Point2) -> ToolFeedback {
        let mut feedback = ToolFeedback {
            markers: self.points.clone(),
            ..ToolFeedback::default()
        };
        if self.points.len() == 1 {
            feedback
                .shapes
                .push(Geometry::Line(Segment2::new(self.points[0], cursor)));
        } else if self.points.len() == 2
            && !coincide(cursor, self.points[0])
            && let Some(arc) = self.arc_for((cursor - self.points[0]).angle())
        {
            feedback.shapes.push(Geometry::Arc(arc));
        }
        feedback
    }

    fn last_point(&self, _cx: &ToolContext<'_>) -> Option<Point2> {
        self.points.last().copied()
    }
}

// ---- registration ---------------------------------------------------------------------------------

fn info(
    id: &str,
    name: &str,
    group: &str,
    order: i32,
    aliases: &[&str],
    tip: &str,
    make: ToolFactory,
) -> ToolInfo {
    ToolInfo {
        id: String::from(id),
        name: String::from(name),
        category: String::from("Draw"),
        group: String::from(group),
        order,
        aliases: aliases.iter().map(|&alias| String::from(alias)).collect(),
        tip: String::from(tip),
        make: Some(make),
        ..ToolInfo::default()
    }
}

/// For the tests, which have no mouse: the Sketch tool's pen moving through
/// `at` (a no-op for any other tool). It reaches the tool through
/// InteractiveTool::as_any_mut, where the C++ used dynamic_cast.
pub fn sketch_move_for_tests(tool: &mut dyn InteractiveTool, at: Point2) {
    if let Some(sketch) = tool
        .as_any_mut()
        .and_then(|any| any.downcast_mut::<SketchTool>())
    {
        sketch.record_move(at);
    }
}

/// The drawing system's draw tools (the C++ addDrawProfessionalTools).
pub fn add_draw_professional_tools(catalog: &mut ToolCatalog, report: &mut Report<'_>) {
    report(catalog.add(info(
        "draw.polyline3d",
        "3D Polyline",
        "Lines",
        35,
        &["PLINE3D", "3DPOLY", "3DPOLYLINE"],
        "Draws a polyline with a height at each vertex: typed as x,y,z, taken from what the point \
         snaps to, or the current height set with H.",
        factory(|_, start| -> Box<dyn InteractiveTool> {
            Box::new(Polyline3dTool::new(&start.attributes))
        }),
    )));
    report(catalog.add(info(
        "draw.spline",
        "Spline",
        "Curves",
        70,
        &["SPLINE", "SPL"],
        "Draws a spline through the points you give, or with them as its control points (C); \
         Enter finishes.",
        factory(|_, start| -> Box<dyn InteractiveTool> {
            Box::new(SplineTool::new(&start.attributes))
        }),
    )));
    report(catalog.add(info(
        "draw.ellipse",
        "Ellipse, Axis and End",
        "Curves",
        80,
        &["ELLIPSE", "EL"],
        "Draws an ellipse from the two ends of one axis and the other axis's half length.",
        factory(|_, start| -> Box<dyn InteractiveTool> {
            Box::new(EllipseTool::new(&start.attributes, EllipseVariant::AxisEnd))
        }),
    )));
    report(catalog.add(info(
        "draw.ellipse.centre",
        "Ellipse, Centre",
        "Curves",
        81,
        &[],
        "Draws an ellipse from its centre, one axis's end and the other axis's half length.",
        factory(|_, start| -> Box<dyn InteractiveTool> {
            Box::new(EllipseTool::new(&start.attributes, EllipseVariant::Centre))
        }),
    )));
    report(catalog.add(info(
        "draw.ellipse.arc",
        "Ellipse, Arc",
        "Curves",
        82,
        &["ELLIPSEARC"],
        "Draws an elliptical arc: an ellipse by its axis, then the arc's start and end.",
        factory(|_, start| -> Box<dyn InteractiveTool> {
            Box::new(EllipseTool::new(&start.attributes, EllipseVariant::Arc))
        }),
    )));
    report(catalog.add(info(
        "draw.xline",
        "Construction Line",
        "Lines",
        60,
        &["XLINE", "XL"],
        "Draws construction lines through a base point, on the construction layer, which is \
         never plotted; H and V make them horizontal or vertical.",
        factory(|_, start| -> Box<dyn InteractiveTool> {
            Box::new(ConstructionTool::new(&start.attributes, false))
        }),
    )));
    report(catalog.add(info(
        "draw.ray",
        "Ray",
        "Lines",
        61,
        &["RAY"],
        "Draws rays from a start point through each point you pick, on the construction layer, \
         which is never plotted.",
        factory(|_, start| -> Box<dyn InteractiveTool> {
            Box::new(ConstructionTool::new(&start.attributes, true))
        }),
    )));
    report(catalog.add(info(
        "draw.dline",
        "Double Line",
        "Lines",
        70,
        &["DLINE", "DL"],
        "Draws both sides of a path at a width (W sets it) as two polylines.",
        factory(|_, start| -> Box<dyn InteractiveTool> {
            Box::new(DoubleLineTool::new(&start.attributes))
        }),
    )));
    report(catalog.add(info(
        "draw.sketch",
        "Freehand Sketch",
        "Lines",
        80,
        &["SKETCH"],
        "Records the cursor's path between two clicks and keeps it, weeded to a tolerance, as \
         polylines when you press Enter.",
        factory(|cx, start| -> Box<dyn InteractiveTool> {
            Box::new(SketchTool::new(cx, &start.attributes))
        }),
    )));
    report(catalog.add(info(
        "draw.circle.ttt",
        "Circle, Tangent Tangent Tangent",
        "Curves",
        55,
        &[],
        "Draws the circle touching three lines, the one whose touching points are nearest your \
         picks.",
        factory(|_, start| -> Box<dyn InteractiveTool> {
            Box::new(CircleTangentsTool::new(&start.attributes))
        }),
    )));
    report(catalog.add(info(
        "draw.arc.sed",
        "Arc, Start End Direction",
        "Curves",
        95,
        &[],
        "Draws an arc from its start to its end leaving the start in a direction you give.",
        factory(|_, start| -> Box<dyn InteractiveTool> {
            Box::new(ArcDirectionTool::new(&start.attributes))
        }),
    )));
    report(catalog.add(info(
        "draw.revcloud",
        "Revision Cloud",
        "Lines",
        90,
        &["REVCLOUD"],
        "Draws a cloud of arcs round an outline you pick, or round a rectangle (R), to mark a \
         change.",
        factory(|_, start| -> Box<dyn InteractiveTool> {
            Box::new(RevisionCloudTool::new(&start.attributes))
        }),
    )));
}
