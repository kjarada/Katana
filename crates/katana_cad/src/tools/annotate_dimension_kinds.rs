//! Angular, radius, diameter and ordinate dimensions
//! (src/katana_cad/tools/annotate_dimension_kinds.cpp), as AutoCAD's
//! DIMANGULAR, DIMRADIUS, DIMDIAMETER and DIMORDINATE take them:
//!
//! ```text
//!   Angular   Select the first line, or press Enter to specify the vertex
//!               Select the second line                 two lines picked, or
//!               Specify angle vertex                   three points given
//!               Specify first angle endpoint
//!               Specify second angle endpoint
//!             Specify dimension arc line location      the side measured
//!   Radius    Select an arc or circle
//!             Specify dimension line location          its direction and reach
//!   Diameter  the same
//!   Ordinate  Specify feature location or [Datum]
//!             Specify leader endpoint or [Xdatum/Ydatum]
//! ```
//!
//! Each makes the dimension the DIM verb makes (DIM ANGULAR, RADIUS,
//! DIAMETER, ORDINATE), through the same builders
//! (annotation::dimension_build), so a tool and a typed verb cannot disagree
//! about what was measured: one DimensionGeometry of its kind, drawn by
//! dimension_draw in the layer's dimension style, added by ONE command. A
//! dimension made from picked entities - two lines, an arc or a circle -
//! follows them when they are edited (the associative update), and so does
//! one whose points were snapped to an entity's end, middle, centre or vertex
//! (anchored_point), as DIM's #id points do; one made from typed or plainly
//! clicked points measures those points.
//!
//! The ordinate's datum is the drawing's origin, as AutoCAD's is the UCS
//! origin, until Datum gives another - DIM ORDINATE ... datum=x,y. The tool
//! starts again after each dimension, and measures from the datum of the
//! drawing's newest ordinate dimension: the datum given is kept for the rest
//! of the run, and the drawing, not the program, keeps it, as it keeps the
//! height the Text tool offers.

use katana_commands::{EntityAttributes, create_dimension};
use katana_core::{Error, ErrorCode, Result};
use katana_entity::{AnchorRef, DimensionGeometry, DimensionKind, EntityId, Geometry, Model};
use katana_geometry::{Point2, Segment2};
use katana_math::numerics::RAD_TO_DEG;

use crate::annotation::dimension_build::{
    AnchoredPoint, angular_between_lines, angular_dimension, ordinate_dimension, radial_dimension,
};
use crate::interactive_tool::{InteractiveTool, ToolContext, ToolInput, ToolStart, ToolStep};
use crate::tool_feedback::ToolFeedback;
use crate::tools::annotate_common::{
    coincident, default_enter, default_entity, default_point, format_number, is_option,
};

// What the finished dimension says it measured, as the other dimension tools
// do: "angular dimension measuring 90 degrees", "radius dimension measuring
// 3".
fn measured_message(dimension: &DimensionGeometry) -> String {
    let value = dimension.measurement();
    match dimension.kind {
        DimensionKind::Angular => format!(
            "angular dimension measuring {} degrees",
            format_number(value * RAD_TO_DEG)
        ),
        DimensionKind::Radius => format!("radius dimension measuring {}", format_number(value)),
        DimensionKind::Diameter => {
            format!("diameter dimension measuring {}", format_number(value))
        }
        DimensionKind::OrdinateX => {
            format!("X ordinate dimension measuring {}", format_number(value))
        }
        DimensionKind::OrdinateY => {
            format!("Y ordinate dimension measuring {}", format_number(value))
        }
        DimensionKind::Aligned | DimensionKind::Linear => {
            format!("dimension measuring {}", format_number(value))
        }
    }
}

fn finished(dimension: Result<DimensionGeometry>, attributes: &EntityAttributes) -> ToolStep {
    match dimension {
        Err(error) => ToolStep::rejected(error.message),
        Ok(dimension) => {
            let message = measured_message(&dimension);
            ToolStep::done(
                Some(create_dimension(dimension, attributes.clone())),
                message,
                true,
            )
        }
    }
}

// The drawing a picked entity's dimension is measured in. The C++ reads its
// document without asking - a pick needs one, so a tool with picks has one -
// and here the missing one is said rather than assumed.
fn model_of<'a>(cx: &ToolContext<'a>) -> Result<&'a Model> {
    match cx.document {
        Some(document) => Ok(document.model()),
        None => Err(Error::new(
            ErrorCode::InvalidState,
            "there is no drawing to dimension",
        )),
    }
}

// ---- Angular ------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AngularStep {
    FirstLine,
    SecondLine,
    Vertex,
    FirstPoint,
    SecondPoint,
    Location,
}

struct AngularDimensionTool {
    attributes: EntityAttributes,
    step: AngularStep,
    first_line: EntityId,
    second_line: EntityId,
    by_lines: bool,
    // Each with the entity point it was snapped to, if any.
    vertex: AnchoredPoint,
    first: AnchoredPoint,
    second: AnchoredPoint,
    // Set only while anchored_point hands a snapped point to point().
    pending: AnchorRef,
}

impl AngularDimensionTool {
    fn new(start: &ToolStart) -> Self {
        Self {
            attributes: start.attributes.clone(),
            step: AngularStep::FirstLine,
            first_line: 0,
            second_line: 0,
            by_lines: false,
            vertex: AnchoredPoint::default(),
            first: AnchoredPoint::default(),
            second: AnchoredPoint::default(),
            pending: AnchorRef::default(),
        }
    }

    fn place(&self, cx: &ToolContext<'_>, at: Point2) -> Result<DimensionGeometry> {
        if self.by_lines {
            return angular_between_lines(model_of(cx)?, self.first_line, self.second_line, at);
        }
        angular_dimension(&self.vertex, &self.first, &self.second, Some(at))
    }

    fn anchored(&self, at: Point2) -> AnchoredPoint {
        AnchoredPoint {
            point: at,
            r#ref: self.pending,
        }
    }
}

impl InteractiveTool for AngularDimensionTool {
    fn prompt(&self, _cx: &ToolContext<'_>) -> String {
        String::from(match self.step {
            AngularStep::FirstLine => "Select the first line, or press Enter to specify the vertex",
            AngularStep::SecondLine => "Select the second line or [Undo]",
            AngularStep::Vertex => "Specify angle vertex or [Undo]",
            AngularStep::FirstPoint => "Specify first angle endpoint or [Undo]",
            AngularStep::SecondPoint => "Specify second angle endpoint or [Undo]",
            AngularStep::Location => "Specify dimension arc line location or [Undo]",
        })
    }

    fn expects(&self, _cx: &ToolContext<'_>) -> ToolInput {
        if self.step == AngularStep::FirstLine || self.step == AngularStep::SecondLine {
            ToolInput::Entity
        } else {
            ToolInput::Point
        }
    }

    fn entity(&mut self, cx: &ToolContext<'_>, id: EntityId, at: Point2) -> ToolStep {
        if self.step != AngularStep::FirstLine && self.step != AngularStep::SecondLine {
            return default_entity(cx, self.expects(cx), id, at);
        }
        let is_line = cx
            .document
            .and_then(|document| document.model().entities.find(id))
            .is_some_and(|picked| matches!(picked.geometry, Geometry::Line(_)));
        if !is_line {
            return ToolStep::rejected(
                "pick a line; press Enter to give the vertex and two points instead",
            );
        }
        if self.step == AngularStep::FirstLine {
            self.first_line = id;
            self.step = AngularStep::SecondLine;
            return ToolStep::next("");
        }
        if id == self.first_line {
            return ToolStep::rejected("that is the first line; pick the other one");
        }
        self.second_line = id;
        self.by_lines = true;
        self.step = AngularStep::Location;
        ToolStep::next("")
    }

    fn point(&mut self, cx: &ToolContext<'_>, at: Point2) -> ToolStep {
        match self.step {
            AngularStep::Vertex => {
                self.vertex = self.anchored(at);
                self.step = AngularStep::FirstPoint;
                ToolStep::next("")
            }
            AngularStep::FirstPoint => {
                if coincident(self.vertex.point, at) {
                    return ToolStep::rejected(
                        "that point is the vertex; an angle's arm needs a length",
                    );
                }
                self.first = self.anchored(at);
                self.step = AngularStep::SecondPoint;
                ToolStep::next("")
            }
            AngularStep::SecondPoint => {
                if coincident(self.vertex.point, at) {
                    return ToolStep::rejected(
                        "that point is the vertex; an angle's arm needs a length",
                    );
                }
                self.second = self.anchored(at);
                self.by_lines = false;
                self.step = AngularStep::Location;
                ToolStep::next("")
            }
            AngularStep::Location => finished(self.place(cx, at), &self.attributes),
            AngularStep::FirstLine | AngularStep::SecondLine => {
                default_point(cx, self.expects(cx), at)
            }
        }
    }

    fn anchored_point(&mut self, cx: &ToolContext<'_>, at: Point2, anchor: &AnchorRef) -> ToolStep {
        self.pending = *anchor;
        let step = self.point(cx, at);
        self.pending = AnchorRef::default();
        step
    }

    fn value(&mut self, cx: &ToolContext<'_>, text: &str) -> ToolStep {
        if is_option(text, "Undo", 1) {
            return self.undo(cx);
        }
        ToolStep::rejected(
            if self.step == AngularStep::FirstLine || self.step == AngularStep::SecondLine {
                "pick a line in the drawing"
            } else {
                "click the point or type it as x,y"
            },
        )
    }

    fn enter(&mut self, cx: &ToolContext<'_>) -> ToolStep {
        if self.step == AngularStep::FirstLine {
            self.step = AngularStep::Vertex;
            return ToolStep::next("");
        }
        if self.step == AngularStep::Vertex {
            self.step = AngularStep::FirstLine;
            return ToolStep::next("");
        }
        ToolStep::rejected(if self.expects(cx) == ToolInput::Entity {
            "pick the second line"
        } else {
            "specify the point asked for"
        })
    }

    fn undo(&mut self, _cx: &ToolContext<'_>) -> ToolStep {
        self.step = match self.step {
            AngularStep::FirstLine => {
                return ToolStep::rejected("nothing to undo: nothing has been picked");
            }
            AngularStep::SecondLine | AngularStep::Vertex => AngularStep::FirstLine,
            AngularStep::FirstPoint => AngularStep::Vertex,
            AngularStep::SecondPoint => AngularStep::FirstPoint,
            AngularStep::Location => {
                if self.by_lines {
                    AngularStep::SecondLine
                } else {
                    AngularStep::SecondPoint
                }
            }
        };
        ToolStep::next("")
    }

    fn preview(&self, cx: &ToolContext<'_>, cursor: Point2) -> ToolFeedback {
        let mut feedback = ToolFeedback::default();
        match self.step {
            AngularStep::FirstPoint => {
                feedback.markers.push(self.vertex.point);
                if !coincident(self.vertex.point, cursor) {
                    feedback
                        .shapes
                        .push(Geometry::Line(Segment2::new(self.vertex.point, cursor)));
                }
            }
            AngularStep::SecondPoint => {
                feedback.markers.push(self.vertex.point);
                feedback.shapes.push(Geometry::Line(Segment2::new(
                    self.vertex.point,
                    self.first.point,
                )));
                if !coincident(self.vertex.point, cursor) {
                    feedback
                        .shapes
                        .push(Geometry::Line(Segment2::new(self.vertex.point, cursor)));
                }
            }
            AngularStep::Location => {
                if let Ok(dimension) = self.place(cx, cursor) {
                    feedback.shapes.push(Geometry::Dimension(dimension));
                }
            }
            AngularStep::FirstLine | AngularStep::SecondLine | AngularStep::Vertex => {}
        }
        feedback
    }

    fn last_point(&self, _cx: &ToolContext<'_>) -> Option<Point2> {
        match self.step {
            AngularStep::FirstPoint => Some(self.vertex.point),
            AngularStep::SecondPoint => Some(self.first.point),
            AngularStep::Location => (!self.by_lines).then_some(self.second.point),
            AngularStep::FirstLine | AngularStep::SecondLine | AngularStep::Vertex => None,
        }
    }
}

// ---- Radius and diameter ---------------------------------------------------------------

struct RadialDimensionTool {
    attributes: EntityAttributes,
    diameter: bool,
    curve: EntityId,
}

impl RadialDimensionTool {
    fn place(&self, cx: &ToolContext<'_>, at: Point2) -> Result<DimensionGeometry> {
        radial_dimension(model_of(cx)?, self.curve, at, self.diameter)
    }
}

impl InteractiveTool for RadialDimensionTool {
    fn prompt(&self, _cx: &ToolContext<'_>) -> String {
        String::from(if self.curve == 0 {
            "Select an arc or circle"
        } else {
            "Specify dimension line location or [Undo]"
        })
    }

    fn expects(&self, _cx: &ToolContext<'_>) -> ToolInput {
        if self.curve == 0 {
            ToolInput::Entity
        } else {
            ToolInput::Point
        }
    }

    fn entity(&mut self, cx: &ToolContext<'_>, id: EntityId, at: Point2) -> ToolStep {
        if self.curve != 0 {
            return default_entity(cx, self.expects(cx), id, at);
        }
        let is_round = cx
            .document
            .and_then(|document| document.model().entities.find(id))
            .is_some_and(|picked| {
                matches!(picked.geometry, Geometry::Arc(_) | Geometry::Circle(_))
            });
        if !is_round {
            return ToolStep::rejected("pick an arc or a circle");
        }
        self.curve = id;
        ToolStep::next("")
    }

    fn point(&mut self, cx: &ToolContext<'_>, at: Point2) -> ToolStep {
        if self.curve == 0 {
            return default_point(cx, self.expects(cx), at);
        }
        finished(self.place(cx, at), &self.attributes)
    }

    fn value(&mut self, cx: &ToolContext<'_>, text: &str) -> ToolStep {
        if is_option(text, "Undo", 1) {
            return self.undo(cx);
        }
        ToolStep::rejected(if self.curve == 0 {
            "pick an arc or a circle in the drawing"
        } else {
            "click where the dimension line goes"
        })
    }

    fn undo(&mut self, _cx: &ToolContext<'_>) -> ToolStep {
        if self.curve == 0 {
            return ToolStep::rejected("nothing to undo: nothing has been picked");
        }
        self.curve = 0;
        ToolStep::next("")
    }

    fn preview(&self, cx: &ToolContext<'_>, cursor: Point2) -> ToolFeedback {
        let mut feedback = ToolFeedback::default();
        if self.curve != 0
            && let Ok(dimension) = self.place(cx, cursor)
        {
            feedback.shapes.push(Geometry::Dimension(dimension));
        }
        feedback
    }
}

// ---- Ordinate --------------------------------------------------------------------------

// The datum of the drawing's newest ordinate dimension, with the entity point
// it follows; the drawing's origin when there is none.
fn newest_ordinate_datum(model: &Model) -> AnchoredPoint {
    let mut datum = AnchoredPoint {
        point: Point2::new(0.0, 0.0),
        r#ref: AnchorRef::default(),
    };
    model.entities.for_each(|entity| {
        if let Geometry::Dimension(dimension) = &entity.geometry
            && (dimension.kind == DimensionKind::OrdinateX
                || dimension.kind == DimensionKind::OrdinateY)
        {
            datum = AnchoredPoint {
                point: dimension.vertex,
                r#ref: dimension.vertex_ref,
            };
        }
    });
    datum
}

fn point_text(point: Point2) -> String {
    format!("{},{}", format_number(point.x), format_number(point.y))
}

struct OrdinateDimensionTool {
    attributes: EntityAttributes,
    datum: AnchoredPoint,
    asking_datum: bool,
    feature: Option<AnchoredPoint>,
    // Some(true): X; None: from the leader's direction.
    axis: Option<bool>,
    // Set only while anchored_point hands a snapped point to point().
    pending: AnchorRef,
}

impl OrdinateDimensionTool {
    fn new(cx: &ToolContext<'_>, start: &ToolStart) -> Self {
        Self {
            attributes: start.attributes.clone(),
            datum: match cx.document {
                Some(document) => newest_ordinate_datum(document.model()),
                None => AnchoredPoint {
                    point: Point2::new(0.0, 0.0),
                    r#ref: AnchorRef::default(),
                },
            },
            asking_datum: false,
            feature: None,
            axis: None,
            pending: AnchorRef::default(),
        }
    }

    fn place(&self, feature: &AnchoredPoint, at: Point2) -> Result<DimensionGeometry> {
        ordinate_dimension(&self.datum, feature, at, self.axis)
    }
}

impl InteractiveTool for OrdinateDimensionTool {
    fn prompt(&self, _cx: &ToolContext<'_>) -> String {
        if self.asking_datum {
            return format!("Specify the datum point <{}>", point_text(self.datum.point));
        }
        if self.feature.is_none() {
            // The datum is said while it is not the origin, since nothing on
            // screen shows what the ordinates are measured from.
            let origin =
                self.datum.point == Point2::new(0.0, 0.0) && !self.datum.r#ref.associated();
            return if origin {
                String::from("Specify feature location or [Datum]")
            } else {
                format!(
                    "Specify feature location or [Datum] (datum {})",
                    point_text(self.datum.point)
                )
            };
        }
        if let Some(axis) = self.axis {
            return format!(
                "Specify leader endpoint of the {} ordinate or [Xdatum/Ydatum/Undo]",
                if axis { "X" } else { "Y" }
            );
        }
        String::from("Specify leader endpoint or [Xdatum/Ydatum/Undo]")
    }

    fn expects(&self, _cx: &ToolContext<'_>) -> ToolInput {
        ToolInput::Point
    }

    fn point(&mut self, _cx: &ToolContext<'_>, at: Point2) -> ToolStep {
        let here = AnchoredPoint {
            point: at,
            r#ref: self.pending,
        };
        if self.asking_datum {
            self.datum = here;
            self.asking_datum = false;
            return ToolStep::next("");
        }
        let Some(feature) = self.feature else {
            self.feature = Some(here);
            return ToolStep::next("");
        };
        finished(self.place(&feature, at), &self.attributes)
    }

    // A feature or a datum snapped to an entity's point follows it, as DIM
    // ORDINATE #id.end ... datum=#id.centre does; the leader's end does not.
    fn anchored_point(&mut self, cx: &ToolContext<'_>, at: Point2, anchor: &AnchorRef) -> ToolStep {
        self.pending = *anchor;
        let step = self.point(cx, at);
        self.pending = AnchorRef::default();
        step
    }

    fn value(&mut self, cx: &ToolContext<'_>, text: &str) -> ToolStep {
        if is_option(text, "Undo", 1) {
            return self.undo(cx);
        }
        if self.asking_datum {
            return ToolStep::rejected(
                "click the datum or type it as x,y, or press Enter to keep it",
            );
        }
        if self.feature.is_none() && is_option(text, "Datum", 1) {
            self.asking_datum = true;
            return ToolStep::next("");
        }
        if self.feature.is_some() && is_option(text, "Xdatum", 1) {
            self.axis = Some(true);
            return ToolStep::next("");
        }
        if self.feature.is_some() && is_option(text, "Ydatum", 1) {
            self.axis = Some(false);
            return ToolStep::next("");
        }
        ToolStep::rejected(if self.feature.is_none() {
            "click the feature or type it as x,y, or type D for the datum"
        } else {
            "click the leader's end, or type X or Y to choose the ordinate"
        })
    }

    fn enter(&mut self, cx: &ToolContext<'_>) -> ToolStep {
        if self.asking_datum {
            self.asking_datum = false; // the datum shown is kept
            return ToolStep::next("");
        }
        default_enter(cx)
    }

    fn undo(&mut self, _cx: &ToolContext<'_>) -> ToolStep {
        if self.asking_datum {
            self.asking_datum = false;
            return ToolStep::next("");
        }
        if self.axis.take().is_some() {
            return ToolStep::next("");
        }
        if self.feature.take().is_some() {
            return ToolStep::next("");
        }
        ToolStep::rejected("nothing to undo: no feature has been given")
    }

    fn preview(&self, _cx: &ToolContext<'_>, cursor: Point2) -> ToolFeedback {
        let mut feedback = ToolFeedback::default();
        if let Some(feature) = self.feature.filter(|_| !self.asking_datum) {
            feedback.markers.push(feature.point);
            if let Ok(dimension) = self.place(&feature, cursor) {
                feedback.shapes.push(Geometry::Dimension(dimension));
            }
        }
        feedback
    }

    fn last_point(&self, _cx: &ToolContext<'_>) -> Option<Point2> {
        self.feature.map(|feature| feature.point)
    }
}

/// The Angular Dimension tool.
pub(crate) fn make_angular_dimension_tool(
    _cx: &ToolContext<'_>,
    start: &ToolStart,
) -> Box<dyn InteractiveTool> {
    Box::new(AngularDimensionTool::new(start))
}

/// The Radius Dimension tool.
pub(crate) fn make_radius_dimension_tool(
    _cx: &ToolContext<'_>,
    start: &ToolStart,
) -> Box<dyn InteractiveTool> {
    Box::new(RadialDimensionTool {
        attributes: start.attributes.clone(),
        diameter: false,
        curve: 0,
    })
}

/// The Diameter Dimension tool.
pub(crate) fn make_diameter_dimension_tool(
    _cx: &ToolContext<'_>,
    start: &ToolStart,
) -> Box<dyn InteractiveTool> {
    Box::new(RadialDimensionTool {
        attributes: start.attributes.clone(),
        diameter: true,
        curve: 0,
    })
}

/// The Ordinate Dimension tool.
pub(crate) fn make_ordinate_dimension_tool(
    cx: &ToolContext<'_>,
    start: &ToolStart,
) -> Box<dyn InteractiveTool> {
    Box::new(OrdinateDimensionTool::new(cx, start))
}
