//! Balloon (src/katana_cad/tools/annotate_balloon.cpp): a numbered circle
//! callout at the end of a leader, as the BALLOON verb makes one - the item
//! numbers of a schedule or a detail's parts.
//!
//! ```text
//!   Specify the balloon's arrow point or [Number/Style/Paper]     the tip
//!   Specify next point or [Number/Style/Paper/Undo]               a bend, or
//!                                                                 where the
//!                                                                 circle sits
//!   Specify next point or [Number/Style/Paper/Undo] <done>        more bends,
//!                                                                 or Enter
//! ```
//!
//! The number is one more than the highest balloon's in the drawing
//! (annotation::leader_edit::next_balloon_number, the verb's own rule), or
//! what Number was told; Style and Paper are its text style and height on
//! paper, BALLOON's style= and paper=, which the tool once had no way to
//! give. The balloon is ONE leader entity with a circle callout and the
//! verb's sizes - `BALLOON p p [p...] n= style= paper=` typed and this tool
//! given the same points and options make the same entity - and a tip
//! snapped to an entity's end, middle, centre or vertex follows it, as
//! BALLOON #id.end does. The options are for the balloon being drawn, as the
//! Leader's are.
//!
//! The C++ refuses a number that is not UTF-8; a Rust line is a `&str`, which
//! always is.

use katana_commands::EntityAttributes;
use katana_entity::annotation::DEFAULT_TEXT_STYLE_NAME;
use katana_entity::{AnchorRef, CalloutShape, Geometry, LeaderGeometry};
use katana_geometry::Point2;

use crate::annotation::leader_edit::next_balloon_number;
use crate::interactive_tool::{InteractiveTool, ToolContext, ToolInput, ToolStart, ToolStep};
use crate::tool_feedback::ToolFeedback;
use crate::tools::annotate_common::{
    coincident, create_all, format_number, is_option, new_entity, text_style_named, trimmed,
    typed_number,
};

// What the typed line is for, when it is not a point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Asking {
    Nothing,
    Number,
    Style,
    Paper,
}

struct BalloonTool {
    attributes: EntityAttributes,
    vertices: Vec<Point2>,
    tip_ref: AnchorRef,
    number: Option<String>,
    style: Option<String>,
    // mm
    paper: Option<f64>,
    asking: Asking,
}

impl BalloonTool {
    fn place(&mut self, at: Point2, anchor: AnchorRef) -> ToolStep {
        if self.asking != Asking::Nothing {
            return ToolStep::rejected(
                "type what the prompt asks, or press Enter to keep what it shows",
            );
        }
        if self
            .vertices
            .last()
            .is_some_and(|&last| coincident(last, at))
        {
            return ToolStep::rejected(
                "that point is on the previous one; each leader segment needs a length",
            );
        }
        if self.vertices.is_empty() {
            self.tip_ref = anchor; // only the tip follows what it points at
        }
        self.vertices.push(at);
        ToolStep::next("")
    }

    // What Number was told, else the drawing's next by the verb's own rule;
    // None only when the drawing's highest balloon has no next.
    fn number(&self, cx: &ToolContext<'_>) -> Option<String> {
        if let Some(number) = &self.number {
            return Some(number.clone());
        }
        let Some(document) = cx.document else {
            return Some(String::from("1"));
        };
        next_balloon_number(document.model())
            .ok()
            .map(|next| format!("{next}"))
    }

    // The verb's balloon: a leader with the entity's default sizes, a circle
    // callout and the number as its note, in the style and at the height on
    // paper the options gave.
    fn balloon_through(&self, cx: &ToolContext<'_>, vertices: Vec<Point2>) -> LeaderGeometry {
        LeaderGeometry {
            vertices,
            tip_ref: self.tip_ref,
            callout: CalloutShape::Circle,
            text: self.number(cx).unwrap_or_default(),
            style: self.style.clone().unwrap_or_default(),
            paper_height: self.paper.unwrap_or(LeaderGeometry::default().paper_height),
            ..LeaderGeometry::default()
        }
    }
}

impl InteractiveTool for BalloonTool {
    fn prompt(&self, cx: &ToolContext<'_>) -> String {
        match self.asking {
            Asking::Number => {
                return format!(
                    "Enter the balloon's number <{}>",
                    self.number(cx).unwrap_or_else(|| String::from("?"))
                );
            }
            Asking::Style => {
                return format!(
                    "Enter text style name <{}>",
                    self.style.as_deref().unwrap_or(DEFAULT_TEXT_STYLE_NAME)
                );
            }
            Asking::Paper => {
                // Unset, the leader's 0: its text style's height, as the
                // verb's.
                return format!(
                    "Enter the number's height on paper, mm <{}>",
                    match self.paper {
                        Some(paper) => format_number(paper),
                        None => String::from("the style's"),
                    }
                );
            }
            Asking::Nothing => {}
        }
        String::from(if self.vertices.is_empty() {
            "Specify the balloon's arrow point or [Number/Style/Paper]"
        } else if self.vertices.len() < 2 {
            "Specify next point or [Number/Style/Paper/Undo]"
        } else {
            "Specify next point or [Number/Style/Paper/Undo] <done>"
        })
    }

    fn expects(&self, _cx: &ToolContext<'_>) -> ToolInput {
        if self.asking != Asking::Nothing {
            ToolInput::Value
        } else {
            ToolInput::Point
        }
    }

    fn point(&mut self, _cx: &ToolContext<'_>, at: Point2) -> ToolStep {
        self.place(at, AnchorRef::default())
    }

    fn anchored_point(
        &mut self,
        _cx: &ToolContext<'_>,
        at: Point2,
        anchor: &AnchorRef,
    ) -> ToolStep {
        self.place(at, *anchor)
    }

    fn value(&mut self, cx: &ToolContext<'_>, typed: &str) -> ToolStep {
        let text = trimmed(typed);
        match self.asking {
            Asking::Number => {
                if text.is_empty() {
                    return self.enter(cx);
                }
                self.number = Some(String::from(text));
                self.asking = Asking::Nothing;
                return ToolStep::next("");
            }
            Asking::Style => {
                if text.is_empty() {
                    return self.enter(cx);
                }
                return match text_style_named(cx.document, text) {
                    Err(error) => ToolStep::rejected(error.message),
                    Ok(found) => {
                        self.style = Some(found);
                        self.asking = Asking::Nothing;
                        ToolStep::next("")
                    }
                };
            }
            Asking::Paper => {
                if text.is_empty() {
                    return self.enter(cx);
                }
                let Some(height) = typed_number(text).filter(|&height| height > 0.0) else {
                    return ToolStep::rejected(
                        "the number's height on paper is millimetres greater than 0",
                    );
                };
                self.paper = Some(height);
                self.asking = Asking::Nothing;
                return ToolStep::next("");
            }
            Asking::Nothing => {}
        }
        if !self.vertices.is_empty() && is_option(typed, "Undo", 1) {
            return self.undo(cx);
        }
        if is_option(typed, "Number", 1) {
            self.asking = Asking::Number;
            return ToolStep::next("");
        }
        if is_option(typed, "Style", 1) {
            self.asking = Asking::Style;
            return ToolStep::next("");
        }
        if is_option(typed, "Paper", 1) {
            self.asking = Asking::Paper;
            return ToolStep::next("");
        }
        ToolStep::rejected(if self.vertices.is_empty() {
            "click the point the arrow touches, or type N, S or P for the number, its style or \
             its height on paper"
        } else {
            "click the next point, type N, S or P for the number, its style or its height on \
             paper, or press Enter to finish"
        })
    }

    fn enter(&mut self, cx: &ToolContext<'_>) -> ToolStep {
        if self.asking != Asking::Nothing {
            self.asking = Asking::Nothing; // keeps what the prompt showed
            return ToolStep::next("");
        }
        if self.vertices.is_empty() {
            return ToolStep::done(None, "", false);
        }
        if self.vertices.len() < 2 {
            return ToolStep::rejected("a balloon needs a second point, where its circle sits");
        }
        if self.number(cx).is_none() {
            return ToolStep::rejected(
                "the highest balloon is numbered as high as a number goes, so has no next: type \
                 N and give one, or BALLOON RENUMBER first",
            );
        }
        let balloon = self.balloon_through(cx, self.vertices.clone());
        let message = format!("balloon {}", balloon.text);
        let entities = vec![new_entity(Geometry::Leader(balloon), &self.attributes)];
        ToolStep::done(Some(create_all("CREATE_BALLOON", entities)), message, true)
    }

    fn undo(&mut self, _cx: &ToolContext<'_>) -> ToolStep {
        if self.asking != Asking::Nothing {
            self.asking = Asking::Nothing;
            return ToolStep::next("");
        }
        if self.vertices.pop().is_none() {
            return ToolStep::rejected("nothing to undo: no point has been given");
        }
        if self.vertices.is_empty() {
            self.tip_ref = AnchorRef::default();
        }
        ToolStep::next("")
    }

    fn preview(&self, cx: &ToolContext<'_>, cursor: Point2) -> ToolFeedback {
        let mut feedback = ToolFeedback::default();
        let Some(&last) = self.vertices.last() else {
            return feedback;
        };
        let mut vertices = self.vertices.clone();
        if !coincident(last, cursor) {
            vertices.push(cursor);
        }
        if vertices.len() >= 2 {
            feedback
                .shapes
                .push(Geometry::Leader(self.balloon_through(cx, vertices)));
        }
        feedback.markers.push(last);
        feedback
    }

    fn last_point(&self, _cx: &ToolContext<'_>) -> Option<Point2> {
        self.vertices.last().copied()
    }
}

/// The Balloon tool.
pub(crate) fn make_balloon_tool(
    _cx: &ToolContext<'_>,
    start: &ToolStart,
) -> Box<dyn InteractiveTool> {
    Box::new(BalloonTool {
        attributes: start.attributes.clone(),
        vertices: Vec::new(),
        tip_ref: AnchorRef::default(),
        number: None,
        style: None,
        paper: None,
        asking: Asking::Nothing,
    })
}
