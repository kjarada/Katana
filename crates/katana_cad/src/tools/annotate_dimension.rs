//! Aligned and linear dimensions (src/katana_cad/tools/annotate_dimension.cpp),
//! as AutoCAD's DIMALIGNED and DIMLINEAR take them:
//!
//! ```text
//!   Specify first extension line origin or <select object>
//!                                        a point, or Enter to pick a line or a
//!                                        polyline segment instead
//!   Specify second extension line origin or [Undo]
//!   Specify dimension line location ...  a point; T gives the text; Linear
//!                                        also takes H, V or R (Rotated: a
//!                                        typed angle, or two points on the
//!                                        line)
//! ```
//!
//! ALIGNED is a DimensionGeometry of the Aligned kind, made by
//! annotation::dimension_build::aligned_dimension as DIM ALIGNED makes it:
//! |end - start|, its dimension line parallel to start->end and displaced by
//! `offset` to its LEFT (negative: to its right). The offset is the signed
//! distance of the picked location from the line through the origins, or a
//! typed number.
//!
//! LINEAR is one of two things, and the model can hold both:
//!
//! * The Linear kind (DimensionKind::Linear, DIM LINEAR's, made by
//!   dimension_build::linear_dimension), measuring along a direction from the
//!   two origins themselves. The tool makes it for a Rotated direction, for a
//!   dimension line BETWEEN the two origins' levels (each extension line then
//!   leaves its own origin, in opposite directions, as AutoCAD's DIMLINEAR
//!   draws them), and whenever an origin was snapped to an entity's end,
//!   middle, centre or vertex - the only kind that can follow its origins,
//!   since its start and end ARE them.
//! * Otherwise the PROJECTION this tool has always stored: an aligned
//!   dimension of the origins projected onto the chosen axis. A horizontal
//!   dimension of (x1, y1) and (x2, y2) placed at height yd is stored as start
//!   (x1, yb), end (x2, yb) - so it measures |x2 - x1| - with yb the level of
//!   whichever origin is FARTHER from the dimension line. That choice decides
//!   the extension lines, because the model draws both from its start and end
//!   towards the dimension line, each starting DIMEXO (extension_offset) clear
//!   of its point. The farther origin's extension line is exactly AutoCAD's,
//!   with the origin in the DIMEXO gap at its foot; the nearer origin's starts
//!   DIMEXO beyond the farther one's level. When the two levels differ by more
//!   than DIMEXO it runs THROUGH its origin and on past it, away from the
//!   dimension line, by the difference less DIMEXO; when they differ by less,
//!   it starts beyond its origin, which sits in a gap of DIMEXO less the
//!   difference. Every origin therefore lies on the line of its own extension
//!   line. Kept for placements it can draw because every drawing made before
//!   the Linear kind (2026-09-25) holds it, and a plain click still makes what
//!   it made then.
//!
//! Angular, radius, diameter and ordinate dimensions are in
//! annotate_dimension_kinds; baseline and continued chains in
//! annotate_dimension_chain. The C++ refuses dimension text that is not UTF-8
//! at the text prompt; a Rust line is a `&str`, which always is.

use katana_commands::{EntityAttributes, create_dimension};
use katana_core::{Error, ErrorCode, Result};
use katana_entity::{AnchorRef, DimensionGeometry, EntityId, Geometry};
use katana_geometry::{Point2, Polyline2, Segment2};
use katana_math::cpp;
use katana_math::numerics::{DEG_TO_RAD, PI, RAD_TO_DEG, tolerance};

use crate::annotation::dimension_build::{AnchoredPoint, aligned_dimension, linear_dimension};
use crate::interactive_tool::{InteractiveTool, ToolContext, ToolInput, ToolStart, ToolStep};
use crate::tool_feedback::ToolFeedback;
use crate::tools::annotate_common::{
    coincident, default_entity, default_point, format_number, is_option, typed_number,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Aligned,
    Linear,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Orientation {
    Horizontal,
    Vertical,
}

// What the dimension line prompt has been told so far. Each option typed
// there pushes the state it replaces, so Undo takes back the last option
// first and only then the second origin.
#[derive(Clone, Debug, Default, PartialEq)]
struct LocationState {
    // Empty: the measured distance.
    text_override: String,
    // Linear only; None: from the location.
    orientation: Option<Orientation>,
    // Linear only: Rotated, radians from east.
    angle: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    First,
    Object,
    Second,
    Location,
    Text,
    Angle,
    AngleSecond,
}

struct DimensionTool {
    attributes: EntityAttributes,
    kind: Kind,

    step: Step,
    first: Point2,
    second: Point2,
    // The entity points the origins were snapped to; unassociated when they
    // were clicked, typed or taken from a picked line.
    first_ref: AnchorRef,
    second_ref: AnchorRef,
    // Set only while anchored_point hands a snapped point to point().
    pending: AnchorRef,
    // Rotated's first point, when the angle is given by two.
    angle_from: Point2,
    // The origins came from a picked line, so Undo at the dimension line goes
    // back to picking rather than to a second-origin prompt never shown.
    picked: bool,
    state: LocationState,
    history: Vec<LocationState>,
}

// The segment of `polyline` nearest `at`; the first of equals.
fn nearest_segment(polyline: &Polyline2, at: Point2) -> Option<Segment2> {
    let mut nearest = None;
    let mut best = f64::INFINITY;
    for i in 0..polyline.segment_count() {
        let segment = polyline.segment(i);
        let distance = segment.distance_to(at);
        if distance < best {
            best = distance;
            nearest = Some(segment);
        }
    }
    nearest
}

impl DimensionTool {
    fn new(start: &ToolStart, kind: Kind) -> Self {
        Self {
            attributes: start.attributes.clone(),
            kind,
            step: Step::First,
            first: Point2::default(),
            second: Point2::default(),
            first_ref: AnchorRef::default(),
            second_ref: AnchorRef::default(),
            pending: AnchorRef::default(),
            angle_from: Point2::default(),
            picked: false,
            state: LocationState::default(),
            history: Vec::new(),
        }
    }

    fn remember(&mut self) {
        self.history.push(self.state.clone());
    }

    fn first_origin(&self) -> AnchoredPoint {
        AnchoredPoint {
            point: self.first,
            r#ref: self.first_ref,
        }
    }

    fn second_origin(&self) -> AnchoredPoint {
        AnchoredPoint {
            point: self.second,
            r#ref: self.second_ref,
        }
    }

    fn anchored(&self) -> bool {
        self.first_ref.associated() || self.second_ref.associated()
    }

    // Rotated: the direction is `angle`, which H and V give up.
    fn rotate(&mut self, angle: f64) -> ToolStep {
        self.remember();
        self.state.angle = Some(angle);
        self.state.orientation = None;
        self.step = Step::Location;
        ToolStep::next("")
    }

    fn location_value(&mut self, cx: &ToolContext<'_>, text: &str) -> ToolStep {
        if is_option(text, "Undo", 1) {
            return self.undo(cx);
        }
        if is_option(text, "Text", 1) {
            self.step = Step::Text;
            return ToolStep::next("");
        }
        if self.kind == Kind::Linear {
            if is_option(text, "Horizontal", 1) || is_option(text, "Vertical", 1) {
                self.remember();
                self.state.orientation = Some(if is_option(text, "Horizontal", 1) {
                    Orientation::Horizontal
                } else {
                    Orientation::Vertical
                });
                self.state.angle = None;
                return ToolStep::next("");
            }
            if is_option(text, "Rotated", 1) {
                self.step = Step::Angle;
                return ToolStep::next("");
            }
            return ToolStep::rejected(
                "place the dimension line with a point, or type T, H, V or R",
            );
        }
        // Aligned: a typed number is the offset itself, for a dimension line
        // at an exact distance - the interpreter's DIM takes the same.
        let Some(offset) = typed_number(text) else {
            return ToolStep::rejected(
                "place the dimension line with a point, type its offset (positive to the left of \
                 first to second), or type T",
            );
        };
        let dimension = DimensionGeometry {
            start_ref: self.first_ref,
            end_ref: self.second_ref,
            ..DimensionGeometry::new(
                self.first,
                self.second,
                offset,
                self.state.text_override.clone(),
            )
        };
        self.finish(&dimension)
    }

    fn place(&self, location: Point2) -> Result<DimensionGeometry> {
        let made = match self.kind {
            Kind::Aligned => {
                aligned_dimension(&self.first_origin(), &self.second_origin(), location)
            }
            Kind::Linear => self.place_linear(location),
        };
        made.map(|mut dimension| {
            dimension.text_override = self.state.text_override.clone();
            dimension
        })
    }

    fn place_linear(&self, location: Point2) -> Result<DimensionGeometry> {
        if let Some(angle) = self.state.angle {
            return linear_dimension(
                &self.first_origin(),
                &self.second_origin(),
                location,
                Some(angle),
            );
        }
        let (first, second) = (self.first, self.second);
        // Whether each orientation measures anything: level origins have no
        // vertical distance, one above the other no horizontal one.
        let measures_across = (second.x - first.x).abs() > tolerance::GEOMETRIC;
        let measures_up = (second.y - first.y).abs() > tolerance::GEOMETRIC;

        let orientation = match self.state.orientation {
            Some(orientation) => orientation,
            None => {
                // As DIMLINEAR decides: a line placed above or below the
                // origins measures across (horizontal), one beside them
                // measures up (vertical). Off a corner, the direction the
                // cursor is farther out wins; an exact diagonal is
                // horizontal, so the choice is never left to rounding.
                let out_x = cpp::max3(
                    0.0,
                    cpp::min(first.x, second.x) - location.x,
                    location.x - cpp::max(first.x, second.x),
                );
                let out_y = cpp::max3(
                    0.0,
                    cpp::min(first.y, second.y) - location.y,
                    location.y - cpp::max(first.y, second.y),
                );
                if out_x == 0.0 && out_y == 0.0 {
                    return Err(Error::new(
                        ErrorCode::InvalidArgument,
                        "the dimension line is within the rectangle the two origins span, so it \
                         could be horizontal or vertical; place it above, below or beside them, \
                         or type H or V",
                    ));
                }
                let mut chosen = if out_y >= out_x {
                    Orientation::Horizontal
                } else {
                    Orientation::Vertical
                };
                // Off the corner of a level pair - past the end of a
                // horizontal edge, the commonest thing dimensioned - "farther
                // out" can pick the orientation that measures nothing
                // although the line is also above or below the pair. Unless
                // the user forced one, the orientation that does measure wins
                // wherever the line is outside the origins on its side; with
                // neither, the refusal below says why.
                if chosen == Orientation::Vertical && !measures_up && measures_across && out_y > 0.0
                {
                    chosen = Orientation::Horizontal;
                } else if chosen == Orientation::Horizontal
                    && !measures_across
                    && measures_up
                    && out_x > 0.0
                {
                    chosen = Orientation::Vertical;
                }
                chosen
            }
        };
        let horizontal = orientation == Orientation::Horizontal;

        // `across` is the coordinate the dimension line is placed at.
        let across = |p: Point2| if horizontal { p.y } else { p.x };

        let measures = if horizontal {
            measures_across
        } else {
            measures_up
        };
        if !measures {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                if horizontal {
                    "the two origins are one above the other, so a horizontal dimension would \
                     measure nothing; place the line beside them or type V"
                } else {
                    "the two origins are level, so a vertical dimension would measure nothing; \
                     place the line above or below them or type H"
                },
            ));
        }
        let first_out = across(first) - across(location);
        let second_out = across(second) - across(location);
        let between = (first_out > tolerance::GEOMETRIC && second_out < -tolerance::GEOMETRIC)
            || (first_out < -tolerance::GEOMETRIC && second_out > tolerance::GEOMETRIC);
        // What the projection cannot be (the module doc): a line whose
        // extension lines leave in opposite directions, or origins that
        // follow an entity.
        if between || self.anchored() {
            return linear_dimension(
                &self.first_origin(),
                &self.second_origin(),
                location,
                Some(if horizontal { 0.0 } else { 0.5 * PI }),
            );
        }
        // The farther origin's level (the module doc). A tie keeps the first
        // origin's, so the choice is never left to rounding.
        let base = if first_out.abs() >= second_out.abs() {
            across(first)
        } else {
            across(second)
        };
        let start = if horizontal {
            Point2::new(first.x, base)
        } else {
            Point2::new(base, first.y)
        };
        let end = if horizontal {
            Point2::new(second.x, base)
        } else {
            Point2::new(base, second.y)
        };
        let normal = (end - start).normalized().perpendicular();
        Ok(DimensionGeometry::new(
            start,
            end,
            (location - start).dot(normal),
            self.state.text_override.clone(),
        ))
    }

    fn finish(&self, dimension: &DimensionGeometry) -> ToolStep {
        let message = format!(
            "{} dimension measuring {}",
            if self.kind == Kind::Aligned {
                "aligned"
            } else {
                "linear"
            },
            format_number(dimension.measurement())
        );
        ToolStep::done(
            Some(create_dimension(dimension.clone(), self.attributes.clone())),
            message,
            true,
        )
    }
}

impl InteractiveTool for DimensionTool {
    fn prompt(&self, _cx: &ToolContext<'_>) -> String {
        match self.step {
            Step::First => String::from("Specify first extension line origin or <select object>"),
            Step::Object => String::from("Select a line or a polyline segment to dimension"),
            Step::Second => String::from("Specify second extension line origin or [Undo]"),
            Step::Location => {
                if self.kind == Kind::Aligned {
                    return String::from(
                        "Specify dimension line location or its offset, or [Text/Undo]",
                    );
                }
                // Once H, V or R is typed the prompt says so, since nothing
                // else on screen shows that the cursor no longer chooses.
                if let Some(angle) = self.state.angle {
                    return format!(
                        "Specify location of the dimension line rotated {} degrees or \
                         [Text/Horizontal/Vertical/Rotated/Undo]",
                        format_number(angle * RAD_TO_DEG)
                    );
                }
                if let Some(orientation) = self.state.orientation {
                    return format!(
                        "Specify location of the {} dimension line or \
                         [Text/Horizontal/Vertical/Rotated/Undo]",
                        if orientation == Orientation::Horizontal {
                            "horizontal"
                        } else {
                            "vertical"
                        }
                    );
                }
                String::from(
                    "Specify dimension line location or [Text/Horizontal/Vertical/Rotated/Undo]",
                )
            }
            Step::Text => {
                String::from("Enter dimension text, or press Enter for the measured distance")
            }
            Step::Angle => format!(
                "Specify angle of dimension line or two points on it <{}>",
                format_number(self.state.angle.unwrap_or(0.0) * RAD_TO_DEG)
            ),
            Step::AngleSecond => String::from("Specify second point"),
        }
    }

    fn expects(&self, _cx: &ToolContext<'_>) -> ToolInput {
        match self.step {
            Step::Object => ToolInput::Entity,
            Step::Text => ToolInput::Value,
            Step::First | Step::Second | Step::Location | Step::Angle | Step::AngleSecond => {
                ToolInput::Point
            }
        }
    }

    fn point(&mut self, cx: &ToolContext<'_>, at: Point2) -> ToolStep {
        match self.step {
            Step::First => {
                self.first = at;
                self.first_ref = self.pending;
                self.step = Step::Second;
                ToolStep::next("")
            }
            Step::Second => {
                if coincident(self.first, at) {
                    return ToolStep::rejected(
                        "the second origin is on the first; a dimension needs two distinct points",
                    );
                }
                self.second = at;
                self.second_ref = self.pending;
                self.picked = false;
                self.step = Step::Location;
                ToolStep::next("")
            }
            Step::Location => match self.place(at) {
                Ok(dimension) => self.finish(&dimension),
                Err(error) => ToolStep::rejected(error.message),
            },
            Step::Angle => {
                self.angle_from = at;
                self.step = Step::AngleSecond;
                ToolStep::next("")
            }
            Step::AngleSecond => {
                if coincident(self.angle_from, at) {
                    return ToolStep::rejected(
                        "the second point is on the first; two points give a direction only when \
                         they are apart",
                    );
                }
                self.rotate((at - self.angle_from).angle())
            }
            Step::Object | Step::Text => default_point(cx, self.expects(cx), at),
        }
    }

    // An origin snapped to an entity's point keeps its reference, so the
    // dimension follows the entity (DIM ALIGNED #id.end does the same).
    fn anchored_point(&mut self, cx: &ToolContext<'_>, at: Point2, anchor: &AnchorRef) -> ToolStep {
        self.pending = *anchor;
        let step = self.point(cx, at);
        self.pending = AnchorRef::default();
        step
    }

    fn entity(&mut self, cx: &ToolContext<'_>, id: EntityId, at: Point2) -> ToolStep {
        if self.step != Step::Object {
            return default_entity(cx, self.expects(cx), id, at);
        }
        let Some(picked) = cx
            .document
            .and_then(|document| document.model().entities.find(id))
        else {
            return ToolStep::rejected("that entity is not in the drawing");
        };
        let measured = match &picked.geometry {
            Geometry::Line(line) => Some(*line),
            Geometry::Polyline(polyline) => nearest_segment(polyline, at),
            _ => {
                return ToolStep::rejected(
                    "only a line or a polyline segment can be dimensioned by picking it; pick \
                     another, or press Enter to pick the two origins instead",
                );
            }
        };
        let Some(measured) = measured.filter(|segment| !coincident(segment.start, segment.end))
        else {
            return ToolStep::rejected("that segment has no length to dimension");
        };
        self.first = measured.start;
        self.second = measured.end;
        self.first_ref = AnchorRef::default();
        self.second_ref = AnchorRef::default();
        self.picked = true;
        self.step = Step::Location;
        ToolStep::next("")
    }

    fn value(&mut self, cx: &ToolContext<'_>, text: &str) -> ToolStep {
        match self.step {
            Step::First => {
                if is_option(text, "Undo", 1) {
                    return self.undo(cx);
                }
                ToolStep::rejected(
                    "click the first origin or type it as x,y, or press Enter to pick a line \
                     instead",
                )
            }
            Step::Object => ToolStep::rejected(
                "pick a line or a polyline segment in the drawing, or press Enter to pick the two \
                 origins instead",
            ),
            Step::Second => {
                if is_option(text, "Undo", 1) {
                    return self.undo(cx);
                }
                ToolStep::rejected("click the second origin or type it as x,y")
            }
            Step::Location => self.location_value(cx, text),
            Step::Text => {
                if text.is_empty() {
                    return self.enter(cx);
                }
                self.remember();
                self.state.text_override = String::from(text);
                self.step = Step::Location;
                ToolStep::next("")
            }
            Step::Angle => {
                if is_option(text, "Undo", 1) {
                    return self.undo(cx);
                }
                let Some(degrees) = typed_number(text) else {
                    return ToolStep::rejected(
                        "type the angle in degrees counter-clockwise from east, or click two \
                         points on the dimension line",
                    );
                };
                self.rotate(degrees * DEG_TO_RAD)
            }
            Step::AngleSecond => {
                if is_option(text, "Undo", 1) {
                    return self.undo(cx);
                }
                ToolStep::rejected("click the second point on the dimension line")
            }
        }
    }

    fn enter(&mut self, _cx: &ToolContext<'_>) -> ToolStep {
        match self.step {
            Step::First => {
                self.step = Step::Object;
                ToolStep::next("")
            }
            Step::Object => {
                self.step = Step::First;
                ToolStep::next("")
            }
            Step::Second => ToolStep::rejected("specify the second origin"),
            Step::Location => ToolStep::rejected("specify where the dimension line goes"),
            Step::Text => {
                // Enter at the text prompt keeps (or goes back to) the
                // measured distance, as AutoCAD's <measured> default does.
                if !self.state.text_override.is_empty() {
                    self.remember();
                    self.state.text_override.clear();
                }
                self.step = Step::Location;
                ToolStep::next("")
            }
            // The angle offered, as AutoCAD's <0> is taken by Enter.
            Step::Angle => self.rotate(self.state.angle.unwrap_or(0.0)),
            Step::AngleSecond => ToolStep::rejected("click the second point on the dimension line"),
        }
    }

    fn undo(&mut self, _cx: &ToolContext<'_>) -> ToolStep {
        match self.step {
            Step::First => ToolStep::rejected("nothing to undo: no origin has been given"),
            Step::Object | Step::Second => {
                self.step = Step::First;
                ToolStep::next("")
            }
            Step::Location => {
                match self.history.pop() {
                    Some(state) => self.state = state,
                    None => {
                        self.step = if self.picked {
                            Step::Object
                        } else {
                            Step::Second
                        };
                    }
                }
                ToolStep::next("")
            }
            Step::Text | Step::Angle => {
                self.step = Step::Location;
                ToolStep::next("")
            }
            Step::AngleSecond => {
                self.step = Step::Angle;
                ToolStep::next("")
            }
        }
    }

    fn preview(&self, _cx: &ToolContext<'_>, cursor: Point2) -> ToolFeedback {
        let mut feedback = ToolFeedback::default();
        match self.step {
            Step::First | Step::Object => {}
            Step::Second => {
                feedback.markers.push(self.first);
                if !coincident(self.first, cursor) {
                    feedback
                        .shapes
                        .push(Geometry::Line(Segment2::new(self.first, cursor)));
                }
            }
            Step::Location | Step::Text | Step::Angle => {
                feedback.markers.push(self.first);
                feedback.markers.push(self.second);
                // Where the cursor cannot place a dimension (inside the box
                // of a linear dimension's origins), only the origins are
                // marked: drawing a dimension there would promise one the
                // click will refuse.
                if self.step != Step::Angle
                    && let Ok(dimension) = self.place(cursor)
                {
                    feedback.shapes.push(Geometry::Dimension(dimension));
                }
            }
            Step::AngleSecond => {
                feedback.markers.push(self.angle_from);
                if !coincident(self.angle_from, cursor) {
                    feedback
                        .shapes
                        .push(Geometry::Line(Segment2::new(self.angle_from, cursor)));
                }
            }
        }
        feedback
    }

    fn last_point(&self, _cx: &ToolContext<'_>) -> Option<Point2> {
        match self.step {
            Step::Second => Some(self.first),
            Step::Location | Step::Text | Step::Angle => Some(self.second),
            Step::AngleSecond => Some(self.angle_from),
            Step::First | Step::Object => None,
        }
    }
}

/// The Aligned Dimension tool.
pub(crate) fn make_aligned_dimension_tool(
    _cx: &ToolContext<'_>,
    start: &ToolStart,
) -> Box<dyn InteractiveTool> {
    Box::new(DimensionTool::new(start, Kind::Aligned))
}

/// The Linear Dimension tool.
pub(crate) fn make_linear_dimension_tool(
    _cx: &ToolContext<'_>,
    start: &ToolStart,
) -> Box<dyn InteractiveTool> {
    Box::new(DimensionTool::new(start, Kind::Linear))
}
