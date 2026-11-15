//! Point, Line, Polyline, Rectangle, Polygon (tools/families.rs;
//! src/katana_cad/tools/draw_lines.cpp): the Draw menu's Lines group, the
//! tools that make straight-edged geometry.
//!
//! They follow AutoCAD's commands of the same names, because that is what a
//! CAD user's hands already know: the same prompts, the same option letters,
//! Close and Undo inside LINE and PLINE, Enter to finish, @dx,dy and
//! @distance<angle measured from the last point. Each one hands the document a
//! single command, so one undo takes back a whole chain of lines rather than
//! its last segment.
//!
//! The entity a tool makes and the ONE command that adds several are
//! draw_shapes' (drawn_entity, create_drawn), which the C++ restated here as
//! makeEntity and createAll.

use std::cell::Cell;
use std::sync::Arc;

use katana_commands::{EntityAttributes, create_line, create_point, create_polyline};
use katana_core::text::{
    equals_ignoring_case, format_exact_real, parse_finite_double, parse_integer,
};
use katana_core::{Error, ErrorCode, Result};
use katana_entity::entity::set_heights;
use katana_entity::{Geometry, PointGeometry};
use katana_geometry::curves2d::bulge_through;
use katana_geometry::{CurvePolyline2, CurveVertex, Point2, Polyline2, Rectangle2, Segment2, Vec2};
use katana_math::numerics::{PI, TWO_PI, tolerance};

use crate::drawing::draw_shapes::{create_drawn, drawn_entity, tangent_bulge};
use crate::interactive_tool::{
    InteractiveTool, ToolCatalog, ToolContext, ToolFactory, ToolInfo, ToolInput, ToolStart,
    ToolStep,
};
use crate::tool_feedback::ToolFeedback;
use crate::tools::families::{Report, keep_work_on_escape};
use crate::utilities::utility_data::trimmed_text;

// ---- shared by the families' ports ---------------------------------------------------

/// The trait's own answers, for a tool that answers a step as InteractiveTool's
/// default does: the C++'s qualified call `InteractiveTool::point(at)` from an
/// override. A Rust override cannot call the default it replaced, so this
/// stand-in, which overrides nothing but expects(), answers for it - with the
/// default's own words, not a copy of them. Give it what the tool expects now.
pub(crate) struct DefaultAnswers(pub ToolInput);

impl InteractiveTool for DefaultAnswers {
    fn prompt(&self, _cx: &ToolContext<'_>) -> String {
        String::new()
    }

    fn expects(&self, _cx: &ToolContext<'_>) -> ToolInput {
        self.0
    }
}

/// A tool's factory (ToolInfo::make) from a closure, so the closure's
/// arguments are inferred from the factory's signature for every lifetime.
pub(crate) fn factory<Make>(make: Make) -> ToolFactory
where
    Make: Fn(&ToolContext<'_>, &ToolStart) -> Box<dyn InteractiveTool> + Send + Sync + 'static,
{
    Arc::new(make)
}

// ---- shared by the tools -----------------------------------------------------------

// An option typed at a prompt: the whole keyword or any leading part of it, in
// any case, as AutoCAD takes "C", "Cl" or "close" for [Close]. Every prompt
// here offers options with different first letters, so one letter is never
// ambiguous.
fn is_option(text: &str, keyword: &str) -> bool {
    let typed = trimmed_text(text);
    !typed.is_empty()
        && typed.len() <= keyword.len()
        && equals_ignoring_case(typed.as_bytes(), &keyword.as_bytes()[..typed.len()])
}

// The geometry kernel's own test for "the same point" (primitives2d), so a
// tool never accepts a point that would make geometry validation refuse the
// command it returns.
fn coincide(a: Point2, b: Point2) -> bool {
    a.distance_to(b) <= tolerance::GEOMETRIC
}

fn quoted(text: &str) -> String {
    format!("'{}'", trimmed_text(text))
}

// A typed length (a radius, a rectangle's side): a finite number greater than
// the geometric tolerance, or a sentence saying why not.
fn parse_length(text: &str, what: &str) -> Result<f64> {
    let Some(value) = parse_finite_double(trimmed_text(text)) else {
        return Err(Error::new(
            ErrorCode::ParseFailure,
            format!(
                "{} is not a number; type the {what} as a number",
                quoted(text)
            ),
        ));
    };
    if !(value > tolerance::GEOMETRIC) {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!("the {what} must be greater than zero"),
        ));
    }
    Ok(value)
}

// Why typed text was not taken at a Line or Polyline prompt, naming what
// would be: the relative forms only once there is a point to measure them
// from, and Close only once the prompt offers it.
fn not_a_chain_point(text: &str, started: bool, can_close: bool) -> ToolStep {
    let mut why = format!("{} is not a point or an option; type ", quoted(text));
    why.push_str(if started {
        "x,y, @dx,dy or @distance<angle"
    } else {
        "the point as x,y"
    });
    if can_close {
        why.push_str(", or C to close");
    }
    ToolStep::rejected(why)
}

// ---- Point -------------------------------------------------------------------------

thread_local! {
    // A point with a height (the drawing system's, docs/drawing.md): typed as
    // x,y,z, or placed at the current height set with [Height], which the
    // program remembers as it remembers a fillet radius (the tool restarts
    // after every point, so the tool itself cannot keep it); its height goes
    // in the elevation property every survey point already carries. The C++
    // keeps it process-wide; the program uses it on one thread, and tests run
    // on several (the cad port's plan, section 7, risk 4).
    static CURRENT_POINT_HEIGHT: Cell<Option<f64>> = const { Cell::new(None) };
}

struct PointTool {
    attributes: EntityAttributes,
    height: Option<f64>,
    asking_height: bool,
}

impl PointTool {
    fn new(start: &ToolStart) -> Self {
        Self {
            attributes: start.attributes.clone(),
            height: CURRENT_POINT_HEIGHT.get(),
            asking_height: false,
        }
    }

    fn place(&self, at: Point2, height: Option<f64>) -> ToolStep {
        // One command per point, so each is its own undo, and a restart so the
        // tool goes on placing points until Esc - survey marks come in runs.
        let Some(height) = height else {
            return ToolStep::done(Some(create_point(at, self.attributes.clone())), "", true);
        };
        let mut point = drawn_entity(
            Geometry::Point(PointGeometry { position: at }),
            &self.attributes,
        );
        set_heights(&mut point.properties, &[Some(height)]);
        ToolStep::done(Some(create_drawn("CREATE_POINT", vec![point])), "", true)
    }
}

impl InteractiveTool for PointTool {
    fn prompt(&self, _cx: &ToolContext<'_>) -> String {
        if self.asking_height {
            return String::from("Specify the height for the next points, or [None]");
        }
        match self.height {
            Some(height) => format!(
                "Specify a point (x,y,z) or [Height] <height {}>",
                format_exact_real(height)
            ),
            None => String::from("Specify a point (x,y,z) or [Height]"),
        }
    }

    fn expects(&self, _cx: &ToolContext<'_>) -> ToolInput {
        if self.asking_height {
            ToolInput::Value
        } else {
            ToolInput::Point
        }
    }

    fn point(&mut self, _cx: &ToolContext<'_>, at: Point2) -> ToolStep {
        self.place(at, self.height)
    }

    fn point3d(&mut self, _cx: &ToolContext<'_>, at: Point2, z: f64) -> ToolStep {
        self.place(at, Some(z))
    }

    fn takes_heights(&self, _cx: &ToolContext<'_>) -> bool {
        true
    }

    fn value(&mut self, _cx: &ToolContext<'_>, text: &str) -> ToolStep {
        if self.asking_height {
            self.asking_height = false;
            if is_option(text, "None") {
                self.height = None;
                CURRENT_POINT_HEIGHT.set(self.height);
                return ToolStep::next("");
            }
            let Some(z) = parse_finite_double(trimmed_text(text)) else {
                self.asking_height = true;
                return ToolStep::rejected(format!(
                    "{} is not a height; type a number or None",
                    quoted(text)
                ));
            };
            self.height = Some(z);
            CURRENT_POINT_HEIGHT.set(self.height);
            return ToolStep::next("");
        }
        if is_option(text, "Height") {
            self.asking_height = true;
            return ToolStep::next("");
        }
        ToolStep::rejected(format!(
            "{} is not a point; type it as x,y or x,y,z, or H for a height",
            quoted(text)
        ))
    }

    fn preview(&self, _cx: &ToolContext<'_>, cursor: Point2) -> ToolFeedback {
        let mut feedback = ToolFeedback::default();
        feedback
            .shapes
            .push(Geometry::Point(PointGeometry { position: cursor }));
        feedback
    }
}

// ---- Line --------------------------------------------------------------------------
//
// A chain of SEPARATE lines, one per pair of consecutive points, as LINE
// draws; Polyline is the tool for one connected entity. The chain is kept
// here until Enter or Close and then created by one command.

struct LineTool {
    attributes: EntityAttributes,
    points: Vec<Point2>,
}

impl LineTool {
    fn new(start: &ToolStart) -> Self {
        Self {
            attributes: start.attributes.clone(),
            points: Vec::new(),
        }
    }

    fn can_close(&self) -> bool {
        self.points.len() >= 3
    }

    fn close(&mut self) -> ToolStep {
        if !self.can_close() {
            return ToolStep::rejected(
                "Close needs two lines first; there is nothing to close back to yet",
            );
        }
        let first = self.points[0];
        if self
            .points
            .last()
            .is_some_and(|&last| coincide(last, first))
        {
            return ToolStep::rejected(
                "the chain already ends at its first point; press Enter to finish",
            );
        }
        self.points.push(first);
        self.finish()
    }

    fn finish(&mut self) -> ToolStep {
        let count = self.points.len() - 1;
        let command = if count == 1 {
            create_line(self.points[0], self.points[1], self.attributes.clone())
        } else {
            let lines = self
                .points
                .windows(2)
                .map(|pair| {
                    drawn_entity(
                        Geometry::Line(Segment2::new(pair[0], pair[1])),
                        &self.attributes,
                    )
                })
                .collect();
            // The interpreter's LINE names its chain the same way.
            create_drawn("CREATE_LINES", lines)
        };
        self.points.clear();
        let message = if count == 1 {
            String::from("1 line")
        } else {
            format!("{count} lines")
        };
        ToolStep::done(Some(command), message, true)
    }
}

impl InteractiveTool for LineTool {
    fn prompt(&self, _cx: &ToolContext<'_>) -> String {
        if self.points.is_empty() {
            return String::from("Specify first point");
        }
        // Close is offered once there are two segments to close: with one,
        // closing would draw the same line back again.
        String::from(if self.can_close() {
            "Specify next point or [Close/Undo]"
        } else {
            "Specify next point or [Undo]"
        })
    }

    fn expects(&self, _cx: &ToolContext<'_>) -> ToolInput {
        ToolInput::Point
    }

    fn point(&mut self, _cx: &ToolContext<'_>, at: Point2) -> ToolStep {
        if self.points.last().is_some_and(|&last| coincide(at, last)) {
            return ToolStep::rejected(
                "the point is where the last one is; a line needs two different points",
            );
        }
        self.points.push(at);
        ToolStep::next("")
    }

    fn value(&mut self, cx: &ToolContext<'_>, text: &str) -> ToolStep {
        if !self.points.is_empty() && is_option(text, "Close") {
            return self.close();
        }
        if is_option(text, "Undo") {
            return self.undo(cx);
        }
        not_a_chain_point(text, !self.points.is_empty(), self.can_close())
    }

    fn enter(&mut self, _cx: &ToolContext<'_>) -> ToolStep {
        if self.points.len() < 2 {
            // No segment yet: Enter ends the tool, as it ends LINE.
            return ToolStep::done(None, "", false);
        }
        self.finish()
    }

    // Esc keeps the chain drawn so far, as AutoCAD keeps a LINE's segments.
    fn cancel(&mut self, cx: &ToolContext<'_>) -> ToolStep {
        keep_work_on_escape(self, cx)
    }

    fn undo(&mut self, _cx: &ToolContext<'_>) -> ToolStep {
        if self.points.pop().is_none() {
            return ToolStep::rejected("nothing to undo; no point has been given yet");
        }
        ToolStep::next("")
    }

    fn preview(&self, _cx: &ToolContext<'_>, cursor: Point2) -> ToolFeedback {
        let mut feedback = ToolFeedback::default();
        // The segments given so far are not in the drawing until the chain is
        // finished, so the preview draws them as well as the rubber band.
        for pair in self.points.windows(2) {
            feedback
                .shapes
                .push(Geometry::Line(Segment2::new(pair[0], pair[1])));
        }
        if let Some(&last) = self.points.last()
            && !coincide(cursor, last)
        {
            feedback
                .shapes
                .push(Geometry::Line(Segment2::new(last, cursor)));
        }
        feedback.markers = self.points.clone();
        feedback
    }

    fn last_point(&self, _cx: &ToolContext<'_>) -> Option<Point2> {
        self.points.last().copied()
    }
}

// ---- Polyline ----------------------------------------------------------------------
//
// PLINE with its segment modes (the drawing system's, docs/drawing.md): in
// Line mode a point is the end of a straight segment; in Arc mode it is the
// end of an arc TANGENT to the segment before (to the east for the first),
// as a CAD user draws a road or a kerb, and [Second] makes the next arc pass
// through a point instead, then end at the next. Close, Undo and Enter as
// before. A polyline with any arc is a CurvePolyline2; one with none is the
// Polyline2 it always was.

struct PolylineTool {
    attributes: EntityAttributes,
    vertices: Vec<CurveVertex>,
    /// The mode each vertex was given in.
    history: Vec<bool>,
    arc: bool,
    second_pending: bool,
    through: Option<Point2>,
}

impl PolylineTool {
    fn new(start: &ToolStart) -> Self {
        Self {
            attributes: start.attributes.clone(),
            vertices: Vec::new(),
            history: Vec::new(),
            arc: false,
            second_pending: false,
            through: None,
        }
    }

    // A closed polyline needs three vertices; with two, Close would only
    // double the one segment back on itself.
    fn can_close(&self) -> bool {
        self.vertices.len() >= 3
    }

    // The bulge of the arc from the last vertex to `to`, tangent to the
    // segment before it (draw_shapes' tangent_bulge, shared with PLINE ...
    // ARC).
    fn tangent_bulge(&self, to: Point2) -> f64 {
        let path = CurvePolyline2 {
            vertices: self.vertices.clone(),
            closed: false,
        };
        tangent_bulge(&path, to)
    }

    fn close(&mut self) -> ToolStep {
        if !self.can_close() {
            return ToolStep::rejected("Close needs three vertices; give another point first");
        }
        let first = self.vertices[0].position;
        let last = self.vertices[self.vertices.len() - 1].position;
        // A user who snapped back onto the start and then chose Close means
        // one closed shape, not a closed shape with a doubled first vertex.
        // Checked on a copy, so a refusal leaves the chain as it was.
        if coincide(last, first) {
            if self.vertices.len() - 1 < 3 {
                return ToolStep::rejected(
                    "a closed polyline needs three different vertices; give another point first",
                );
            }
            self.vertices.pop();
            self.history.pop();
            if let Some(back) = self.vertices.last_mut() {
                back.bulge = 0.0;
            }
        } else if self.arc {
            // In Arc mode the closing segment is a tangent arc too.
            let bulge = self.tangent_bulge(first);
            if let Some(back) = self.vertices.last_mut() {
                back.bulge = bulge;
            }
        }
        self.finish(true)
    }

    fn finish(&mut self, closed: bool) -> ToolStep {
        let mut shape = CurvePolyline2 {
            vertices: std::mem::take(&mut self.vertices),
            closed,
        };
        if !closed && let Some(back) = shape.vertices.last_mut() {
            back.bulge = 0.0;
        }
        self.history.clear();
        self.through = None;
        self.second_pending = false;
        let count = shape.vertices.len();
        let command = if shape.has_arcs() {
            create_drawn(
                "CREATE_POLYLINE",
                vec![drawn_entity(
                    Geometry::CurvePolyline(shape),
                    &self.attributes,
                )],
            )
        } else {
            create_polyline(
                Polyline2::new(shape.positions(), closed),
                self.attributes.clone(),
            )
        };
        ToolStep::done(
            Some(command),
            format!(
                "{}{count} vertices",
                if closed {
                    "closed polyline of "
                } else {
                    "polyline of "
                }
            ),
            true,
        )
    }
}

impl InteractiveTool for PolylineTool {
    fn prompt(&self, _cx: &ToolContext<'_>) -> String {
        if self.vertices.is_empty() {
            return String::from("Specify start point");
        }
        if self.through.is_some() {
            return String::from("Specify end point of arc");
        }
        let mut options = String::from(if self.arc { "Line/Second" } else { "Arc" });
        if self.can_close() {
            options.push_str("/Close");
        }
        options.push_str("/Undo");
        format!(
            "{} or [{options}]",
            if self.arc {
                "Specify end point of arc"
            } else {
                "Specify next point"
            }
        )
    }

    fn expects(&self, _cx: &ToolContext<'_>) -> ToolInput {
        ToolInput::Point
    }

    fn point(&mut self, _cx: &ToolContext<'_>, at: Point2) -> ToolStep {
        let last = self.vertices.last().map(|vertex| vertex.position);
        if last.is_some_and(|last| coincide(at, last)) {
            return ToolStep::rejected(
                "the point is where the last vertex is; the next vertex must be somewhere else",
            );
        }
        if self.arc
            && let Some(last) = last
        {
            if self.second_pending {
                self.through = Some(at);
                self.second_pending = false;
                return ToolStep::next("");
            }
            let bulge = match self.through.take() {
                Some(through) => bulge_through(last, through, at),
                None => self.tangent_bulge(at),
            };
            if let Some(back) = self.vertices.last_mut() {
                back.bulge = bulge;
            }
        }
        self.vertices.push(CurveVertex {
            position: at,
            bulge: 0.0,
            height: None,
        });
        self.history.push(self.arc);
        ToolStep::next("")
    }

    fn value(&mut self, cx: &ToolContext<'_>, text: &str) -> ToolStep {
        if !self.vertices.is_empty() && is_option(text, "Close") {
            return self.close();
        }
        if is_option(text, "Undo") {
            return self.undo(cx);
        }
        if is_option(text, "Arc") {
            self.arc = true;
            return ToolStep::next("");
        }
        if is_option(text, "Line") {
            self.arc = false;
            self.through = None;
            self.second_pending = false;
            return ToolStep::next("");
        }
        if self.arc && is_option(text, "Second") && !self.vertices.is_empty() {
            self.second_pending = true;
            return ToolStep::next("");
        }
        not_a_chain_point(text, !self.vertices.is_empty(), self.can_close())
    }

    fn enter(&mut self, _cx: &ToolContext<'_>) -> ToolStep {
        if self.vertices.len() < 2 {
            return ToolStep::done(None, "", false);
        }
        self.finish(false)
    }

    // Esc keeps the chain drawn so far, as AutoCAD keeps a LINE's segments.
    fn cancel(&mut self, cx: &ToolContext<'_>) -> ToolStep {
        keep_work_on_escape(self, cx)
    }

    fn undo(&mut self, _cx: &ToolContext<'_>) -> ToolStep {
        if self.second_pending || self.through.is_some() {
            self.second_pending = false;
            self.through = None;
            return ToolStep::next("");
        }
        if self.vertices.pop().is_none() {
            return ToolStep::rejected("nothing to undo; no vertex has been given yet");
        }
        self.history.pop();
        if let Some(back) = self.vertices.last_mut() {
            back.bulge = 0.0; // the segment it started is gone
        }
        ToolStep::next("")
    }

    fn preview(&self, _cx: &ToolContext<'_>, cursor: Point2) -> ToolFeedback {
        let mut feedback = ToolFeedback::default();
        for vertex in &self.vertices {
            feedback.markers.push(vertex.position);
        }
        let Some(last) = self.vertices.last().map(|vertex| vertex.position) else {
            return feedback;
        };
        let mut shape = CurvePolyline2 {
            vertices: self.vertices.clone(),
            closed: false,
        };
        if let Some(through) = self.through {
            feedback.markers.push(through);
        }
        if !coincide(cursor, last) && !self.second_pending {
            if self.arc {
                let bulge = match self.through {
                    Some(through) => bulge_through(last, through, cursor),
                    None => self.tangent_bulge(cursor),
                };
                if let Some(back) = shape.vertices.last_mut() {
                    back.bulge = bulge;
                }
            }
            shape.vertices.push(CurveVertex {
                position: cursor,
                bulge: 0.0,
                height: None,
            });
        }
        if shape.vertices.len() >= 2 {
            if shape.has_arcs() {
                feedback.shapes.push(Geometry::CurvePolyline(shape));
            } else {
                feedback
                    .shapes
                    .push(Geometry::Polyline(Polyline2::new(shape.positions(), false)));
            }
        }
        feedback
    }

    fn last_point(&self, _cx: &ToolContext<'_>) -> Option<Point2> {
        self.vertices.last().map(|vertex| vertex.position)
    }
}

// ---- Rectangle ---------------------------------------------------------------------
//
// Two opposite corners, as the interpreter's RECT takes them, so the second
// is also typed as @width,height from the first. [Dimensions] is RECTANG's
// other way: type the length along x and the width along y, then click on
// the side of the first corner the rectangle goes.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RectangleStep {
    FirstCorner,
    OtherCorner,
    Length,
    Width,
    Side,
}

struct RectangleTool {
    attributes: EntityAttributes,
    step: RectangleStep,
    corner: Point2,
    length: f64,
    width: f64,
}

impl RectangleTool {
    fn new(start: &ToolStart) -> Self {
        Self {
            attributes: start.attributes.clone(),
            step: RectangleStep::FirstCorner,
            corner: Point2::default(),
            length: 0.0,
            width: 0.0,
        }
    }

    fn has_area(rectangle: &Rectangle2) -> bool {
        rectangle.width > tolerance::GEOMETRIC && rectangle.height > tolerance::GEOMETRIC
    }

    // The typed length x width rectangle at the first corner, on the side of
    // it that `towards` is. On the corner's own x or y the side is taken as
    // up or right, the same as Enter.
    fn sized(&self, towards: Point2) -> Rectangle2 {
        let size = Vec2::new(
            if towards.x < self.corner.x {
                -self.length
            } else {
                self.length
            },
            if towards.y < self.corner.y {
                -self.width
            } else {
                self.width
            },
        );
        Rectangle2::from_corners(self.corner, self.corner + size)
    }

    fn make(&mut self, rectangle: Rectangle2) -> ToolStep {
        if !Self::has_area(&rectangle) {
            return ToolStep::rejected(
                "the corners are in line; a rectangle needs both a width and a height",
            );
        }
        // Rectangle2's outline, as the interpreter's RECT makes it:
        // counter-clockwise from the lower left corner.
        self.step = RectangleStep::FirstCorner;
        ToolStep::done(
            Some(create_polyline(
                rectangle.to_polyline(),
                self.attributes.clone(),
            )),
            "rectangle",
            true,
        )
    }
}

impl InteractiveTool for RectangleTool {
    fn prompt(&self, _cx: &ToolContext<'_>) -> String {
        String::from(match self.step {
            RectangleStep::FirstCorner => "Specify first corner point",
            RectangleStep::OtherCorner => "Specify other corner point or [Dimensions/Undo]",
            RectangleStep::Length => "Specify length of the rectangle along x or [Undo]",
            RectangleStep::Width => "Specify width of the rectangle along y or [Undo]",
            RectangleStep::Side => {
                "Specify a point on the side the rectangle goes or [Undo] <up and right>"
            }
        })
    }

    fn expects(&self, _cx: &ToolContext<'_>) -> ToolInput {
        if self.step == RectangleStep::Length || self.step == RectangleStep::Width {
            ToolInput::Value
        } else {
            ToolInput::Point
        }
    }

    fn point(&mut self, cx: &ToolContext<'_>, at: Point2) -> ToolStep {
        match self.step {
            RectangleStep::FirstCorner => {
                self.corner = at;
                self.step = RectangleStep::OtherCorner;
                ToolStep::next("")
            }
            RectangleStep::OtherCorner => self.make(Rectangle2::from_corners(self.corner, at)),
            RectangleStep::Side => {
                let rectangle = self.sized(at);
                self.make(rectangle)
            }
            RectangleStep::Length | RectangleStep::Width => {
                DefaultAnswers(self.expects(cx)).point(cx, at)
            }
        }
    }

    fn value(&mut self, cx: &ToolContext<'_>, text: &str) -> ToolStep {
        if is_option(text, "Undo") {
            return self.undo(cx);
        }
        match self.step {
            RectangleStep::FirstCorner => ToolStep::rejected(format!(
                "{} is not a point; type the corner as x,y",
                quoted(text)
            )),
            RectangleStep::OtherCorner => {
                if is_option(text, "Dimensions") {
                    self.step = RectangleStep::Length;
                    return ToolStep::next("");
                }
                ToolStep::rejected(format!(
                    "{} is not a point; type the other corner as x,y or as @width,height from \
                     the first, or D to give the sizes",
                    quoted(text)
                ))
            }
            RectangleStep::Length => match parse_length(text, "length") {
                Err(error) => ToolStep::rejected(error.message),
                Ok(length) => {
                    self.length = length;
                    self.step = RectangleStep::Width;
                    ToolStep::next("")
                }
            },
            RectangleStep::Width => match parse_length(text, "width") {
                Err(error) => ToolStep::rejected(error.message),
                Ok(width) => {
                    self.width = width;
                    self.step = RectangleStep::Side;
                    ToolStep::next("")
                }
            },
            RectangleStep::Side => ToolStep::rejected(format!(
                "{} is not a point; click on the side the rectangle goes, or press Enter for up \
                 and right",
                quoted(text)
            )),
        }
    }

    fn enter(&mut self, _cx: &ToolContext<'_>) -> ToolStep {
        if self.step == RectangleStep::Side {
            // The default side, as the prompt says: the first corner is the
            // lower left one.
            return self.make(Rectangle2 {
                origin: self.corner,
                width: self.length,
                height: self.width,
            });
        }
        if self.step == RectangleStep::Length || self.step == RectangleStep::Width {
            return ToolStep::rejected(format!(
                "type the {} of the rectangle, or U to go back",
                if self.step == RectangleStep::Length {
                    "length"
                } else {
                    "width"
                }
            ));
        }
        ToolStep::done(None, "", false)
    }

    fn undo(&mut self, _cx: &ToolContext<'_>) -> ToolStep {
        self.step = match self.step {
            RectangleStep::FirstCorner => {
                return ToolStep::rejected("nothing to undo; no corner has been given yet");
            }
            RectangleStep::OtherCorner => RectangleStep::FirstCorner,
            RectangleStep::Length => RectangleStep::OtherCorner,
            RectangleStep::Width => RectangleStep::Length,
            RectangleStep::Side => RectangleStep::Width,
        };
        ToolStep::next("")
    }

    fn preview(&self, _cx: &ToolContext<'_>, cursor: Point2) -> ToolFeedback {
        let mut feedback = ToolFeedback::default();
        if self.step == RectangleStep::FirstCorner {
            return feedback;
        }
        feedback.markers.push(self.corner);
        if self.step == RectangleStep::OtherCorner {
            let rectangle = Rectangle2::from_corners(self.corner, cursor);
            if Self::has_area(&rectangle) {
                feedback
                    .shapes
                    .push(Geometry::Polyline(rectangle.to_polyline()));
            } else if !coincide(self.corner, cursor) {
                // In line with the corner: the rectangle has collapsed to its
                // one visible side, which is still worth seeing.
                feedback
                    .shapes
                    .push(Geometry::Line(Segment2::new(self.corner, cursor)));
            }
        } else if self.step == RectangleStep::Side {
            feedback
                .shapes
                .push(Geometry::Polyline(self.sized(cursor).to_polyline()));
        }
        feedback
    }

    fn last_point(&self, _cx: &ToolContext<'_>) -> Option<Point2> {
        if self.step == RectangleStep::FirstCorner {
            None
        } else {
            Some(self.corner)
        }
    }
}

// ---- Polygon -----------------------------------------------------------------------
//
// A regular polygon, as POLYGON draws it: the number of sides, then either
// the centre and a radius or [Edge] and the two ends of one side. The radius
// is to a vertex when the polygon is Inscribed in the circle (the default)
// and to the midpoint of a side when it is Circumscribed about it; a picked
// radius point is ON the polygon either way - the first vertex, or the middle
// of the first side - so it can be snapped to something that the polygon
// must touch. A typed radius draws the bottom side level, as AutoCAD does.
//
// Unlike POLYGON, the number of sides has no <4> default taken by Enter. The
// tool restarts after each polygon, as the other tools here do, so its first
// prompt is where a user lands when done; Enter there has to end the tool,
// as it does in Point, Line, Polyline and Rectangle, or a right-click to
// leave would arm the next two clicks to draw a square.

const MINIMUM_SIDES: i32 = 3;
const MAXIMUM_SIDES: i32 = 1024;

// The vertices, counter-clockwise, of the regular `sides`-gon about `centre`
// whose first vertex is at centre + to_first. Each is the first rotated about
// the centre, rather than the previous one rotated again, so rounding does not
// accumulate around a 1024-gon, and the first is exactly the point it was
// derived from.
fn regular_polygon(centre: Point2, to_first: Vec2, sides: i32) -> Vec<Point2> {
    let mut vertices = Vec::with_capacity(usize::try_from(sides).unwrap_or_default());
    let step = TWO_PI / f64::from(sides);
    vertices.push(centre + to_first);
    for k in 1..sides {
        vertices.push(centre + to_first.rotated(step * f64::from(k)));
    }
    vertices
}

// The same polygon given the centre-to-midpoint vector of its first side: the
// first vertex is half a side's turn clockwise of it and 1/cos(pi/n) farther
// out (the circumradius of a polygon whose apothem is |to_midpoint|).
fn first_vertex_from_midpoint(to_midpoint: Vec2, sides: i32) -> Vec2 {
    let half = PI / f64::from(sides);
    to_midpoint.rotated(-half) * (1.0 / half.cos())
}

// The polygon on the LEFT of the directed side a->b, counter-clockwise from a:
// each side is the first turned by one exterior angle more than the last.
fn polygon_on_edge(a: Point2, b: Point2, sides: i32) -> Vec<Point2> {
    let mut vertices = Vec::with_capacity(usize::try_from(sides).unwrap_or_default());
    let edge = b - a;
    let step = TWO_PI / f64::from(sides);
    vertices.push(a);
    vertices.push(b);
    let mut last = b;
    for k in 1..sides - 1 {
        last += edge.rotated(step * f64::from(k));
        vertices.push(last);
    }
    vertices
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PolygonStep {
    Sides,
    Centre,
    Radius,
    EdgeStart,
    EdgeEnd,
}

struct PolygonTool {
    attributes: EntityAttributes,
    step: PolygonStep,
    /// Always typed before a step that uses it.
    sides: i32,
    inscribed: bool,
    /// The mode before each Inscribed or Circumscribed typed at the radius
    /// prompt, latest last; Undo pops them before it gives up the centre.
    modes_before: Vec<bool>,
    centre: Point2,
    edge_start: Point2,
}

impl PolygonTool {
    fn new(start: &ToolStart) -> Self {
        Self {
            attributes: start.attributes.clone(),
            step: PolygonStep::Sides,
            sides: MINIMUM_SIDES,
            inscribed: true,
            modes_before: Vec::new(),
            centre: Point2::default(),
            edge_start: Point2::default(),
        }
    }

    // From the centre to the first vertex, given the centre-to-radius-point
    // vector: that point IS the first vertex when inscribed, and the middle of
    // the first side when circumscribed.
    fn to_first_vertex(&self, to_point: Vec2) -> Vec2 {
        if self.inscribed {
            to_point
        } else {
            first_vertex_from_midpoint(to_point, self.sides)
        }
    }

    // Every I or C typed counts as an input for Undo, even one that leaves
    // the mode as it was: Undo then takes back what was typed, rather than
    // skipping it and throwing away the centre.
    fn set_inscribed(&mut self, inscribed: bool) -> ToolStep {
        self.modes_before.push(self.inscribed);
        self.inscribed = inscribed;
        ToolStep::next("")
    }

    fn make(&mut self, vertices: Vec<Point2>) -> ToolStep {
        let sides = self.sides;
        self.step = PolygonStep::Sides;
        self.modes_before.clear();
        ToolStep::done(
            Some(create_polyline(
                Polyline2::new(vertices, true),
                self.attributes.clone(),
            )),
            format!("polygon of {sides} sides"),
            true,
        )
    }
}

impl InteractiveTool for PolygonTool {
    fn prompt(&self, _cx: &ToolContext<'_>) -> String {
        String::from(match self.step {
            PolygonStep::Sides => "Enter number of sides",
            PolygonStep::Centre => "Specify centre of polygon or [Edge/Undo]",
            PolygonStep::Radius => {
                if self.inscribed {
                    "Specify a vertex or type the radius, or [Circumscribed/Undo]"
                } else {
                    "Specify the middle of a side or type the radius, or [Inscribed/Undo]"
                }
            }
            PolygonStep::EdgeStart => "Specify first endpoint of edge or [Undo]",
            PolygonStep::EdgeEnd => "Specify second endpoint of edge or [Undo]",
        })
    }

    fn expects(&self, _cx: &ToolContext<'_>) -> ToolInput {
        if self.step == PolygonStep::Sides {
            ToolInput::Value
        } else {
            ToolInput::Point
        }
    }

    fn point(&mut self, cx: &ToolContext<'_>, at: Point2) -> ToolStep {
        match self.step {
            PolygonStep::Sides => DefaultAnswers(self.expects(cx)).point(cx, at),
            PolygonStep::Centre => {
                self.centre = at;
                self.step = PolygonStep::Radius;
                ToolStep::next("")
            }
            PolygonStep::Radius => {
                if coincide(at, self.centre) {
                    return ToolStep::rejected(
                        "the point is at the centre; the polygon needs a radius greater than zero",
                    );
                }
                let vertices = regular_polygon(
                    self.centre,
                    self.to_first_vertex(at - self.centre),
                    self.sides,
                );
                self.make(vertices)
            }
            PolygonStep::EdgeStart => {
                self.edge_start = at;
                self.step = PolygonStep::EdgeEnd;
                ToolStep::next("")
            }
            PolygonStep::EdgeEnd => {
                if coincide(at, self.edge_start) {
                    return ToolStep::rejected(
                        "the endpoints are the same point; an edge needs two different points",
                    );
                }
                let vertices = polygon_on_edge(self.edge_start, at, self.sides);
                self.make(vertices)
            }
        }
    }

    fn value(&mut self, cx: &ToolContext<'_>, text: &str) -> ToolStep {
        if is_option(text, "Undo") {
            return self.undo(cx);
        }
        match self.step {
            PolygonStep::Sides => {
                let sides = parse_integer(trimmed_text(text));
                let Some(sides) = sides.filter(|sides| {
                    (i64::from(MINIMUM_SIDES)..=i64::from(MAXIMUM_SIDES)).contains(sides)
                }) else {
                    return ToolStep::rejected(format!(
                        "{} is not a number of sides; a polygon has a whole number of sides \
                         from 3 to 1024",
                        quoted(text)
                    ));
                };
                // From 3 to 1024, so it fits.
                self.sides = sides as i32;
                self.step = PolygonStep::Centre;
                ToolStep::next("")
            }
            PolygonStep::Centre => {
                if is_option(text, "Edge") {
                    self.step = PolygonStep::EdgeStart;
                    return ToolStep::next("");
                }
                ToolStep::rejected(format!(
                    "{} is not a point; type the centre as x,y, or E to draw the polygon on an \
                     edge",
                    quoted(text)
                ))
            }
            PolygonStep::Radius => {
                if is_option(text, "Inscribed") {
                    return self.set_inscribed(true);
                }
                if is_option(text, "Circumscribed") {
                    return self.set_inscribed(false);
                }
                let radius = match parse_length(text, "radius") {
                    Ok(radius) => radius,
                    Err(error) => return ToolStep::rejected(error.message),
                };
                // The bottom side level: its midpoint straight below the centre.
                let to_bottom = Vec2::new(0.0, -radius);
                let half = PI / f64::from(self.sides);
                let to_first = if self.inscribed {
                    to_bottom.rotated(-half)
                } else {
                    first_vertex_from_midpoint(to_bottom, self.sides)
                };
                let vertices = regular_polygon(self.centre, to_first, self.sides);
                self.make(vertices)
            }
            PolygonStep::EdgeStart | PolygonStep::EdgeEnd => ToolStep::rejected(format!(
                "{} is not a point; type the endpoint as x,y",
                quoted(text)
            )),
        }
    }

    fn enter(&mut self, _cx: &ToolContext<'_>) -> ToolStep {
        // At the sides prompt nothing is in progress, so this ends the tool
        // (see above); anywhere else it abandons the polygon, as Esc does.
        ToolStep::done(None, "", false)
    }

    fn undo(&mut self, _cx: &ToolContext<'_>) -> ToolStep {
        match self.step {
            PolygonStep::Sides => {
                return ToolStep::rejected(
                    "nothing to undo; the number of sides has not been given",
                );
            }
            PolygonStep::Centre => self.step = PolygonStep::Sides,
            PolygonStep::Radius => {
                // An Inscribed or Circumscribed typed since the centre is a
                // later input than the centre, so it is the one taken back.
                match self.modes_before.pop() {
                    Some(mode) => self.inscribed = mode,
                    None => self.step = PolygonStep::Centre,
                }
            }
            PolygonStep::EdgeStart => self.step = PolygonStep::Centre,
            PolygonStep::EdgeEnd => self.step = PolygonStep::EdgeStart,
        }
        ToolStep::next("")
    }

    fn preview(&self, _cx: &ToolContext<'_>, cursor: Point2) -> ToolFeedback {
        let mut feedback = ToolFeedback::default();
        if self.step == PolygonStep::Radius {
            feedback.markers.push(self.centre);
            if !coincide(cursor, self.centre) {
                feedback.shapes.push(Geometry::Polyline(Polyline2::new(
                    regular_polygon(
                        self.centre,
                        self.to_first_vertex(cursor - self.centre),
                        self.sides,
                    ),
                    true,
                )));
            }
        } else if self.step == PolygonStep::EdgeEnd {
            feedback.markers.push(self.edge_start);
            if !coincide(cursor, self.edge_start) {
                feedback.shapes.push(Geometry::Polyline(Polyline2::new(
                    polygon_on_edge(self.edge_start, cursor, self.sides),
                    true,
                )));
            }
        }
        feedback
    }

    fn last_point(&self, _cx: &ToolContext<'_>) -> Option<Point2> {
        match self.step {
            PolygonStep::Radius => Some(self.centre),
            PolygonStep::EdgeEnd => Some(self.edge_start),
            PolygonStep::Sides | PolygonStep::Centre | PolygonStep::EdgeStart => None,
        }
    }
}

// ---- registration ------------------------------------------------------------------

fn info(
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
        group: String::from("Lines"),
        order,
        aliases: aliases.iter().map(|&alias| String::from(alias)).collect(),
        tip: String::from(tip),
        make: Some(make),
        ..ToolInfo::default()
    }
}

/// Point, Line, Polyline, Rectangle, Polygon (the C++ addDrawLineTools).
pub fn add_draw_line_tools(catalog: &mut ToolCatalog, report: &mut Report<'_>) {
    // The aliases are AutoCAD's, plus the interpreter's own spellings of the
    // same verbs (RECT, RECTANGLE, POLYLINE; command_interpreter), so a word
    // typed at the command line means one thing whichever reads it.
    report(catalog.add(info(
        "draw.point",
        "Point",
        10,
        &["POINT", "PO"],
        "Places a point at each click or typed x,y until you press Esc.",
        factory(|_, start| -> Box<dyn InteractiveTool> { Box::new(PointTool::new(start)) }),
    )));
    report(catalog.add(info(
        "draw.line",
        "Line",
        20,
        &["LINE", "L"],
        "Draws a chain of separate lines point by point; C closes it, U takes back a point and \
         Enter finishes.",
        factory(|_, start| -> Box<dyn InteractiveTool> { Box::new(LineTool::new(start)) }),
    )));
    report(catalog.add(info(
        "draw.polyline",
        "Polyline",
        30,
        &["PLINE", "PL", "POLYLINE"],
        "Draws one polyline through the points you give; C closes it, U takes back a vertex and \
         Enter finishes.",
        factory(|_, start| -> Box<dyn InteractiveTool> { Box::new(PolylineTool::new(start)) }),
    )));
    report(catalog.add(info(
        "draw.rectangle",
        "Rectangle",
        40,
        &["RECTANG", "REC", "RECT", "RECTANGLE"],
        "Draws a rectangle from two opposite corners, or from one corner and @width,height or \
         its typed sizes.",
        factory(|_, start| -> Box<dyn InteractiveTool> { Box::new(RectangleTool::new(start)) }),
    )));
    report(catalog.add(info(
        "draw.polygon",
        "Polygon",
        50,
        &["POLYGON", "POL"],
        "Draws a regular polygon from its number of sides and either its centre and radius or \
         one of its edges.",
        factory(|_, start| -> Box<dyn InteractiveTool> { Box::new(PolygonTool::new(start)) }),
    )));
}
