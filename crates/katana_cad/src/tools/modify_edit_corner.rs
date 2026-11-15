//! Fillet and Chamfer (src/katana_cad/tools/modify_edit_corner.cpp): two
//! lines, and the corner between them rounded by an arc or cut by a bevel.
//! One tool with two modes, as they differ only in what goes into the corner
//! and the values that size it. Each finishes after one corner, as
//! AutoCAD's do; [Multiple] keeps going until Enter, as one command.
//!
//! Each line keeps the side the user PICKED it on. geometry's fillet keeps
//! the end of each line farther from the corner, which is the same thing for
//! lines that stop short of each other or just overshoot, but not for two
//! lines that cross: there the pick is the only thing that says which
//! quarter to keep, so the line is first cut at the corner on the picked
//! side.

use katana_core::{Error, ErrorCode, Result};
use katana_entity::entity::{INVALID_ENTITY_ID, set_heights};
use katana_entity::{Entity, EntityId, Geometry};
use katana_geometry::editing::{chamfer, fillet};
use katana_geometry::intersection::intersect;
use katana_geometry::{IntersectionKind, Line2, Point2, Segment2};
use katana_math::numerics::tolerance;

use crate::document::Document;
use crate::interactive_tool::{InteractiveTool, ToolContext, ToolInput, ToolStep};
use crate::selection::{SelectionFilter, pick_entity};
use crate::tool_feedback::ToolFeedback;
use crate::tools::families::keep_work_on_escape;
use crate::tools::modify_edit_support::{
    Defaults, EditSession, ToolPtr, as_sentence, base_entity, carry_heights, format_number,
    height_at, height_vertices, is_option, kind_name, no_drawing, parse_number, refusal_to_edit,
    with_article,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Fillet,
    Chamfer,
}

// The line a segment lies on.
fn carrier(line: &Segment2) -> Line2 {
    Line2 {
        origin: line.start,
        direction: line.delta(),
    }
}

// The part of `line` on the picked side of `corner`: all of it when the
// corner is not inside it.
fn picked_side(line: &Segment2, corner: Point2, pick: Point2) -> Segment2 {
    let carrier = carrier(line);
    let at_corner = carrier.parameter_of(corner);
    let tiny = tolerance::GEOMETRIC / line.length();
    if at_corner <= tiny || at_corner >= 1.0 - tiny {
        return *line;
    }
    if carrier.parameter_of(pick) < at_corner {
        Segment2::new(line.start, corner)
    } else {
        Segment2::new(corner, line.end)
    }
}

struct Corner {
    /// The first line, rebuilt.
    first: Entity,
    /// The second line, rebuilt.
    second: Entity,
    /// The fillet arc or chamfer bevel.
    joiner: Option<Entity>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    First,
    Second,
    Radius,
    FirstDistance,
    SecondDistance,
}

struct CornerTool {
    defaults: Defaults,
    kind: Kind,
    session: EditSession,
    step: Step,
    multiple: bool,
    first: EntityId,
    first_pick: Point2,
    /// The first chamfer distance, while the second is asked.
    typed_first: f64,
}

impl CornerTool {
    fn new(defaults: Defaults, kind: Kind) -> Self {
        Self {
            defaults,
            kind,
            session: EditSession::new(),
            step: Step::First,
            multiple: false,
            first: INVALID_ENTITY_ID,
            first_pick: Point2::default(),
            typed_first: 0.0,
        }
    }

    fn settings(&self) -> String {
        let defaults = self.defaults.borrow();
        if self.kind == Kind::Fillet {
            return format!("radius {}", format_number(defaults.fillet_radius));
        }
        format!(
            "distances {}, {}",
            format_number(defaults.chamfer_first),
            format_number(defaults.chamfer_second)
        )
    }

    fn choose_distance(&mut self, distance: f64) -> ToolStep {
        if self.step == Step::FirstDistance {
            self.typed_first = distance;
            self.step = Step::SecondDistance;
            return ToolStep::next("");
        }
        let mut defaults = self.defaults.borrow_mut();
        defaults.chamfer_first = self.typed_first;
        defaults.chamfer_second = distance;
        drop(defaults);
        self.step = Step::First;
        ToolStep::next("")
    }

    fn finish(&self) -> ToolStep {
        let count = self.session.operations();
        let filleting = self.kind == Kind::Fillet;
        let defaults = self.defaults.borrow();
        let mut message = String::from(if filleting { "Filleted " } else { "Chamfered " });
        message.push_str(&if count == 1 {
            String::from("1 corner")
        } else {
            format!("{count} corners")
        });
        message.push_str(&if filleting {
            format!(" with radius {}.", format_number(defaults.fillet_radius))
        } else {
            format!(
                " at {} and {}.",
                format_number(defaults.chamfer_first),
                format_number(defaults.chamfer_second)
            )
        });
        ToolStep::done(
            Some(
                self.session
                    .commit(if filleting { "FILLET" } else { "CHAMFER" }),
            ),
            message,
            false,
        )
    }

    // The corner of the first line and `id` picked at `at`, without making
    // it.
    fn attempt(&self, document: &Document, id: EntityId, at: Point2) -> Result<Corner> {
        let first_pieces = self.session.pieces(document, self.first);
        let second_pieces = self.session.pieces(document, id);
        if first_pieces.len() != 1 || second_pieces.len() != 1 {
            return Err(Error::new(ErrorCode::InvalidState, "Pick two lines."));
        }
        let a = &first_pieces[0];
        let b = &second_pieces[0];
        let (Geometry::Line(line_a), Geometry::Line(line_b)) = (&a.geometry, &b.geometry) else {
            return Err(Error::new(ErrorCode::Unsupported, "Pick two lines."));
        };
        let fillet_kind = self.kind == Kind::Fillet;
        let meet = intersect(&carrier(line_a), &carrier(line_b));
        if meet.kind != IntersectionKind::Points {
            return Err(Error::new(
                ErrorCode::InvalidGeometry,
                format!(
                    "The lines are parallel, so there is no corner to {}",
                    if fillet_kind { "fillet." } else { "chamfer." }
                ),
            ));
        }
        let kept_a = picked_side(line_a, meet.points[0], self.first_pick);
        let kept_b = picked_side(line_b, meet.points[0], at);

        let mut corner = Corner {
            first: a.clone(),
            second: b.clone(),
            joiner: None,
        };
        let defaults = self.defaults.borrow();
        if fillet_kind {
            let made = fillet(&kept_a, &kept_b, defaults.fillet_radius).map_err(|error| {
                Error::new(
                    ErrorCode::InvalidGeometry,
                    if error.message.contains("too large") {
                        format!(
                            "A radius of {} is too large for these lines.",
                            format_number(defaults.fillet_radius)
                        )
                    } else {
                        as_sentence(&error.message)
                    },
                )
            })?;
            corner.first.geometry = Geometry::Line(made.first);
            corner.second.geometry = Geometry::Line(made.second);
            if let Some(arc) = made.arc {
                // As fillet_entities: the arc wears the first line's
                // attributes.
                let mut joiner = a.clone();
                joiner.geometry = Geometry::Arc(arc);
                corner.joiner = Some(joiner);
            }
        } else {
            let made = chamfer(
                &kept_a,
                &kept_b,
                defaults.chamfer_first,
                defaults.chamfer_second,
            )
            .map_err(|error| {
                Error::new(
                    ErrorCode::InvalidGeometry,
                    if error.message.contains("leaves no segment") {
                        String::from("The chamfer distances are too large for these lines.")
                    } else {
                        as_sentence(&error.message)
                    },
                )
            })?;
            corner.first.geometry = Geometry::Line(made.first);
            corner.second.geometry = Geometry::Line(made.second);
            if let Some(bevel) = made.bevel {
                let mut joiner = a.clone();
                joiner.geometry = Geometry::Line(bevel);
                corner.joiner = Some(joiner);
            }
        }
        carry_heights(a, &mut corner.first);
        carry_heights(b, &mut corner.second);
        if let Some(joiner) = &mut corner.joiner {
            // The joiner starts on the first line and ends on the second, so
            // each end takes its height from the line it touches.
            let ends = height_vertices(&joiner.geometry);
            if let (Some(&start), Some(&end)) = (ends.first(), ends.last()) {
                set_heights(
                    &mut joiner.properties,
                    &[height_at(a, start), height_at(b, end)],
                );
            }
        }
        Ok(corner)
    }
}

impl InteractiveTool for CornerTool {
    fn prompt(&self, _cx: &ToolContext<'_>) -> String {
        let fillet_kind = self.kind == Kind::Fillet;
        match self.step {
            Step::First => {
                let mut options = String::from(if fillet_kind { "Radius" } else { "Distance" });
                options.push_str(if self.multiple {
                    if self.session.operations() > 0 {
                        "/Undo"
                    } else {
                        ""
                    }
                } else {
                    "/Multiple"
                });
                format!("Select first line or [{options}] ({})", self.settings())
            }
            Step::Second => format!("Select second line or [Undo] ({})", self.settings()),
            Step::Radius => format!(
                "Specify fillet radius <{}>",
                format_number(self.defaults.borrow().fillet_radius)
            ),
            Step::FirstDistance => format!(
                "Specify first chamfer distance <{}>",
                format_number(self.defaults.borrow().chamfer_first)
            ),
            Step::SecondDistance => format!(
                "Specify second chamfer distance <{}>",
                format_number(self.typed_first)
            ),
        }
    }

    fn expects(&self, _cx: &ToolContext<'_>) -> ToolInput {
        if self.step == Step::First || self.step == Step::Second {
            ToolInput::Entity
        } else {
            ToolInput::Value
        }
    }

    fn entity(&mut self, cx: &ToolContext<'_>, id: EntityId, at: Point2) -> ToolStep {
        if self.step != Step::First && self.step != Step::Second {
            return base_entity(cx, self.expects(cx), id, at);
        }
        let Some(document) = cx.document else {
            return no_drawing();
        };
        if let Some(refusal) = refusal_to_edit(document, id) {
            return ToolStep::rejected(refusal);
        }
        let pieces = self.session.pieces(document, id);
        if pieces.len() != 1 || !matches!(pieces[0].geometry, Geometry::Line(_)) {
            let kind = match pieces.first() {
                None => "object",
                Some(piece) => kind_name(&piece.geometry),
            };
            let verb = if self.kind == Kind::Fillet {
                "filleted"
            } else {
                "chamfered"
            };
            if kind == "polyline" || kind == "closed polyline" {
                return ToolStep::rejected(format!(
                    "Only lines can be {verb}; explode the polyline into lines first."
                ));
            }
            return ToolStep::rejected(format!(
                "Only lines can be {verb}, and that is {}.",
                with_article(kind)
            ));
        }
        if self.step == Step::First {
            self.first = id;
            self.first_pick = at;
            self.step = Step::Second;
            return ToolStep::next("");
        }
        if id == self.first {
            return ToolStep::rejected("Pick a different line for the second.");
        }
        let corner = match self.attempt(document, id, at) {
            Ok(corner) => corner,
            Err(error) => return ToolStep::rejected(error.message),
        };
        self.session.begin();
        let mut taken = self.session.replace(self.first, vec![corner.first]);
        if taken.is_ok() {
            taken = self.session.replace(id, vec![corner.second]);
        }
        if taken.is_ok()
            && let Some(joiner) = corner.joiner
        {
            taken = self.session.add(joiner);
        }
        if let Err(error) = taken {
            return ToolStep::rejected(error.message);
        }
        self.step = Step::First;
        if self.multiple {
            return ToolStep::next("");
        }
        self.finish()
    }

    fn value(&mut self, cx: &ToolContext<'_>, text: &str) -> ToolStep {
        if is_option(text, "Undo", "U") {
            return self.undo(cx);
        }
        let fillet_kind = self.kind == Kind::Fillet;
        match self.step {
            Step::First => {
                if fillet_kind && is_option(text, "Radius", "R") {
                    self.step = Step::Radius;
                    return ToolStep::next("");
                }
                if !fillet_kind && is_option(text, "Distance", "D") {
                    self.step = Step::FirstDistance;
                    return ToolStep::next("");
                }
                if !self.multiple && is_option(text, "Multiple", "M") {
                    self.multiple = true;
                    return ToolStep::next(format!(
                        "Multiple: {} corners until Enter.",
                        if fillet_kind { "fillet" } else { "chamfer" }
                    ));
                }
                // A number here is the radius or the first distance, typed
                // without the option letter first.
                if let Some(number) = parse_number(text) {
                    if number < 0.0 {
                        return ToolStep::rejected(if fillet_kind {
                            "The fillet radius cannot be negative."
                        } else {
                            "A chamfer distance cannot be negative."
                        });
                    }
                    if fillet_kind {
                        self.defaults.borrow_mut().fillet_radius = number;
                        return ToolStep::next("");
                    }
                    self.typed_first = number;
                    self.step = Step::SecondDistance;
                    return ToolStep::next("");
                }
                ToolStep::rejected(if fillet_kind {
                    "Pick the first line, or type R to set the radius."
                } else {
                    "Pick the first line, or type D to set the distances."
                })
            }
            Step::Second => {
                ToolStep::rejected("Pick the second line, or type U to pick the first again.")
            }
            Step::Radius => {
                let Some(radius) = parse_number(text) else {
                    return ToolStep::rejected("Type the fillet radius as a number.");
                };
                if radius < 0.0 {
                    return ToolStep::rejected("The fillet radius cannot be negative.");
                }
                self.defaults.borrow_mut().fillet_radius = radius;
                self.step = Step::First;
                ToolStep::next("")
            }
            Step::FirstDistance | Step::SecondDistance => {
                let Some(distance) = parse_number(text) else {
                    return ToolStep::rejected("Type the chamfer distance as a number.");
                };
                if distance < 0.0 {
                    return ToolStep::rejected("A chamfer distance cannot be negative.");
                }
                self.choose_distance(distance)
            }
        }
    }

    // Esc keeps what the session has done: its Enter only ever commits.
    fn cancel(&mut self, cx: &ToolContext<'_>) -> ToolStep {
        keep_work_on_escape(self, cx)
    }

    fn enter(&mut self, _cx: &ToolContext<'_>) -> ToolStep {
        match self.step {
            Step::Radius => {
                self.step = Step::First; // keep the radius
                return ToolStep::next("");
            }
            Step::FirstDistance => {
                let distance = self.defaults.borrow().chamfer_first;
                return self.choose_distance(distance);
            }
            Step::SecondDistance => return self.choose_distance(self.typed_first),
            Step::First | Step::Second => {}
        }
        if self.session.operations() == 0 {
            return ToolStep::done(None, "", false);
        }
        self.finish()
    }

    fn undo(&mut self, _cx: &ToolContext<'_>) -> ToolStep {
        match self.step {
            Step::Second => {
                self.step = Step::First;
                ToolStep::next("")
            }
            Step::First => {
                if self.session.undo() {
                    return ToolStep::next(if self.kind == Kind::Fillet {
                        "Undid the last fillet."
                    } else {
                        "Undid the last chamfer."
                    });
                }
                ToolStep::rejected("Nothing to undo.")
            }
            Step::Radius | Step::FirstDistance => {
                self.step = Step::First;
                ToolStep::next("")
            }
            Step::SecondDistance => {
                self.step = Step::FirstDistance;
                ToolStep::next("")
            }
        }
    }

    fn preview(&self, cx: &ToolContext<'_>, cursor: Point2) -> ToolFeedback {
        let mut feedback = ToolFeedback {
            shapes: self.session.shapes(),
            ..ToolFeedback::default()
        };
        if self.step != Step::Second {
            return feedback;
        }
        let Some(document) = cx.document else {
            return feedback;
        };
        // The corner a click on the line under the cursor would make.
        let under = pick_entity(
            document.model(),
            cursor,
            cx.pick_tolerance,
            &SelectionFilter::default(),
            Some(document.spatial_index()),
        );
        if let Some(under) = under
            && under != self.first
            && let Ok(corner) = self.attempt(document, under, cursor)
        {
            feedback.shapes.push(corner.first.geometry);
            feedback.shapes.push(corner.second.geometry);
            if let Some(joiner) = corner.joiner {
                feedback.shapes.push(joiner.geometry);
            }
        }
        feedback
    }
}

pub fn make_fillet_tool(defaults: Defaults) -> ToolPtr {
    Box::new(CornerTool::new(defaults, Kind::Fillet))
}

pub fn make_chamfer_tool(defaults: Defaults) -> ToolPtr {
    Box::new(CornerTool::new(defaults, Kind::Chamfer))
}
