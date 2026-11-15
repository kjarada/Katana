//! Text and Multiline Text (src/katana_cad/tools/annotate_text.cpp):
//! single-line text, line after line, as AutoCAD's DTEXT places it, and one
//! text of several lines, as its MTEXT makes one.
//!
//! ```text
//!   Specify start point of text or [Style/Justify/Paper]
//!                                                a point, or an option:
//!     Style    a text style's name, or . for none
//!     Justify  TL TC TR ML MC MR BL BC BR (entity.rs, TextJustify)
//!     Paper    a height on paper in mm; 0 for a model height
//!   Specify height or [Undo] <2.5>               a number, a point (its distance
//!                                                from the start), or Enter;
//!                                                not asked of a paper-sized text
//!   Specify rotation angle of text or [Undo] <0> degrees counter-clockwise from
//!                                                east, a point (the direction to
//!                                                it), or Enter
//!   Enter text                                   typed; each line goes below
//!                                                the last; an empty line ends
//! ```
//!
//! All the lines of one use are ONE command, so one undo removes the whole
//! text - as one DTEXT command undoes as one in AutoCAD. Text makes an entity
//! a line; Multiline Text makes ONE entity of every line, broken by '\n', as
//! MTEXT p "a\nb" does.
//!
//! The style, justification and paper height are the TEXT verb's options
//! (docs/annotation.md), and a text is made from them by the rule the verb
//! uses (annotation::text_layout::fit_model_height): a paper-sized one - a
//! paper height, or a style with one - is drawn at its paper size at every
//! scale, so its model height is worked out for the drawing's annotation
//! scale and no height is asked for.
//!
//! The defaults come from the drawing: the newest text in it gives the
//! height, rotation, style, justification and paper height offered, and Enter
//! at Text's first prompt continues directly below it (DTEXT's own shortcut).
//! Reading them from the drawing rather than from memory in the tool means
//! they follow the drawing across sessions and through undo, as TEXTSIZE does
//! in a DWG, and a test sees the same default whatever ran before it. With no
//! text in the drawing the height is 2.5, the metric default of AutoCAD's
//! TEXTSIZE and of this model's TextGeometry. Multiline Text is paper-sized
//! unless told otherwise, as MTEXT is: where the newest text would leave it
//! in model units it starts in the Standard style.
//!
//! The C++ refuses a typed line that is not UTF-8 at the text prompt; a Rust
//! line is a `&str`, which always is, so that refusal has nothing to refuse.

use katana_commands::EntityAttributes;
use katana_entity::annotation::{DEFAULT_ANNOTATION_SCALE, DEFAULT_TEXT_STYLE_NAME};
use katana_entity::entity::{text_justify_from_string, to_string};
use katana_entity::{Geometry, TextGeometry, TextJustify};
use katana_geometry::{Point2, Segment2, Vec2};
use katana_math::numerics::{DEG_TO_RAD, RAD_TO_DEG, tolerance};

use crate::annotation::text_layout::{fit_model_height, is_paper_sized};
use crate::interactive_tool::{InteractiveTool, ToolContext, ToolInput, ToolStart, ToolStep};
use crate::tool_feedback::ToolFeedback;
use crate::tools::annotate_common::{
    coincident, create_all, default_point, format_number, is_option, line_spacing, new_entity,
    trimmed, typed_number,
};

const DEFAULT_HEIGHT: f64 = 2.5;

#[derive(Clone, Debug, PartialEq)]
struct TypedLine {
    position: Point2,
    text: String,
}

// Text: an entity a line. Multiline: one entity of all the lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Lines,
    Block,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Start,
    Style,
    Justify,
    Paper,
    Height,
    Rotation,
    Content,
}

struct TextTool {
    mode: Mode,
    attributes: EntityAttributes,
    scale: f64,
    newest: Option<TextGeometry>,
    default_height: f64,
    default_rotation: f64,

    // The options, offered at the first prompt.
    style: String,
    paper: f64,
    justify: TextJustify,

    step: Step,
    start: Point2,
    height: f64,
    rotation: f64,
    // Entered by Enter at the first prompt, so Undo goes back there rather
    // than to a rotation prompt the user never saw.
    continued: bool,
    lines: Vec<TypedLine>,
}

impl TextTool {
    fn new(cx: &ToolContext<'_>, start: &ToolStart, mode: Mode) -> Self {
        let mut scale = DEFAULT_ANNOTATION_SCALE;
        let mut newest: Option<TextGeometry> = None;
        if let Some(document) = cx.document {
            scale = document.annotation_scale();
            // Ascending id order, so the last one seen is the newest.
            document.model().entities.for_each(|entity| {
                if let Geometry::Text(text) = &entity.geometry {
                    newest = Some(text.clone());
                }
            });
        }
        let mut tool = Self {
            mode,
            attributes: start.attributes.clone(),
            scale,
            newest,
            default_height: DEFAULT_HEIGHT,
            default_rotation: 0.0,
            style: String::new(),
            paper: 0.0,
            justify: TextJustify::BottomLeft,
            step: Step::Start,
            start: Point2::default(),
            height: DEFAULT_HEIGHT,
            rotation: 0.0,
            continued: false,
            lines: Vec::new(),
        };
        if let Some(newest) = &tool.newest {
            tool.default_height = newest.height;
            tool.default_rotation = newest.rotation;
            tool.style = newest.style.clone();
            tool.paper = newest.paper_height;
            tool.justify = newest.justify;
        }
        if mode == Mode::Block && !tool.paper_sized(cx) {
            tool.style = String::from(DEFAULT_TEXT_STYLE_NAME);
            tool.paper = 0.0;
        }
        tool
    }

    // Down the page in the text's own frame: a rotated text's next line is
    // below it as the text reads, not below it on the drawing.
    fn below(rotation: f64) -> Vec2 {
        Vec2::new(0.0, -1.0).rotated(rotation)
    }

    fn next_position(&self) -> Point2 {
        match self.lines.last() {
            Some(last) if self.mode != Mode::Block => {
                last.position + Self::below(self.rotation) * line_spacing(self.height)
            }
            _ => self.start,
        }
    }

    // The text the choices make at `at`, sized by the verb's rule.
    fn made(&self, cx: &ToolContext<'_>, at: Point2, text: String) -> TextGeometry {
        let mut geometry = TextGeometry {
            style: self.style.clone(),
            paper_height: self.paper,
            justify: self.justify,
            ..TextGeometry::new(at, text, self.height, self.rotation)
        };
        if let Some(document) = cx.document {
            fit_model_height(document.model(), self.scale, &mut geometry);
        }
        geometry
    }

    fn block_text(&self, cx: &ToolContext<'_>) -> TextGeometry {
        let mut joined = String::new();
        for line in &self.lines {
            if !joined.is_empty() {
                joined.push('\n');
            }
            joined.push_str(&line.text);
        }
        self.made(cx, self.start, joined)
    }

    // Whether the text as chosen is paper-sized: a paper height, or a style
    // with one.
    fn paper_sized(&self, cx: &ToolContext<'_>) -> bool {
        if self.paper > 0.0 {
            return true;
        }
        let Some(document) = cx.document else {
            return false;
        };
        if self.style.is_empty() {
            return false;
        }
        let probe = TextGeometry {
            style: self.style.clone(),
            ..TextGeometry::default()
        };
        is_paper_sized(document.model(), &probe)
    }

    // A paper-sized text's model height at the drawing's annotation scale.
    fn paper_model_height(&self, cx: &ToolContext<'_>) -> f64 {
        self.made(cx, self.start, String::new()).height
    }

    // What the options are now, for the first prompt: "style Notes, MC, 3.5 mm".
    fn settings(&self) -> String {
        let style = if self.style.is_empty() {
            "none"
        } else {
            self.style.as_str()
        };
        let mut text = format!("style {style}, {}", to_string(self.justify));
        if self.paper > 0.0 {
            text.push_str(&format!(", {} mm on paper", format_number(self.paper)));
        }
        text
    }

    fn choose_style(&mut self, cx: &ToolContext<'_>, typed: &str) -> ToolStep {
        let name = trimmed(typed);
        if name == "." {
            self.style.clear();
        } else if cx
            .document
            .is_some_and(|document| document.model().text_styles.contains(name))
        {
            self.style = String::from(name);
        } else {
            return ToolStep::rejected(format!(
                "there is no text style '{name}'; type a style's name, or . for none"
            ));
        }
        self.step = Step::Start;
        ToolStep::next("")
    }
}

impl InteractiveTool for TextTool {
    fn prompt(&self, _cx: &ToolContext<'_>) -> String {
        match self.step {
            Step::Start => {
                let mut text = String::from(if self.mode == Mode::Block {
                    "Specify insertion point of the text or [Style/Justify/Paper]"
                } else {
                    "Specify start point of text or [Style/Justify/Paper]"
                });
                if self.mode == Mode::Lines && self.newest.is_some() {
                    text.push_str(", or press Enter to continue below the last text");
                }
                format!("{text} <{}>", self.settings())
            }
            Step::Style => format!(
                "Enter a text style name, or . for none <{}>",
                if self.style.is_empty() {
                    "."
                } else {
                    self.style.as_str()
                }
            ),
            Step::Justify => format!(
                "Enter a justification [TL/TC/TR/ML/MC/MR/BL/BC/BR] <{}>",
                to_string(self.justify)
            ),
            Step::Paper => format!(
                "Specify the height on paper in mm, or 0 for a model height <{}>",
                format_number(self.paper)
            ),
            Step::Height => format!(
                "Specify height or [Undo] <{}>",
                format_number(self.default_height)
            ),
            Step::Rotation => format!(
                "Specify rotation angle of text or [Undo] <{}>",
                format_number(self.default_rotation * RAD_TO_DEG)
            ),
            Step::Content => {
                if self.mode == Mode::Block {
                    return String::from(if self.lines.is_empty() {
                        "Enter the first line of text"
                    } else {
                        "Enter the next line, or press Enter to finish the text"
                    });
                }
                String::from(if self.lines.is_empty() {
                    "Enter text"
                } else {
                    "Enter the next line of text, or press Enter to finish"
                })
            }
        }
    }

    fn expects(&self, _cx: &ToolContext<'_>) -> ToolInput {
        // The text itself is a Value so that "Road, north side" stays text
        // rather than being read as a malformed point; so is a style's name.
        match self.step {
            Step::Style | Step::Justify | Step::Paper | Step::Content => ToolInput::Value,
            Step::Start | Step::Height | Step::Rotation => ToolInput::Point,
        }
    }

    fn point(&mut self, cx: &ToolContext<'_>, at: Point2) -> ToolStep {
        match self.step {
            Step::Start => {
                self.start = at;
                // A paper-sized text has its height from the paper: nothing
                // to ask.
                if self.paper_sized(cx) {
                    self.height = self.paper_model_height(cx);
                    self.step = Step::Rotation;
                } else {
                    self.step = Step::Height;
                }
                ToolStep::next("")
            }
            Step::Height => {
                // A picked height is the distance from the start point, as in
                // AutoCAD: the user shows how tall the letters are.
                if coincident(self.start, at) {
                    return ToolStep::rejected(
                        "that point is on the start point, which gives a height of zero",
                    );
                }
                self.height = self.start.distance_to(at);
                self.step = Step::Rotation;
                ToolStep::next("")
            }
            Step::Rotation => {
                if coincident(self.start, at) {
                    return ToolStep::rejected(
                        "that point is on the start point, so it gives no direction for the text",
                    );
                }
                self.rotation = (at - self.start).angle();
                self.step = Step::Content;
                ToolStep::next("")
            }
            Step::Style | Step::Justify | Step::Paper | Step::Content => {
                default_point(cx, self.expects(cx), at)
            }
        }
    }

    fn value(&mut self, cx: &ToolContext<'_>, text: &str) -> ToolStep {
        match self.step {
            Step::Start => {
                if is_option(text, "Undo", 1) {
                    return self.undo(cx);
                }
                if is_option(text, "Style", 1) {
                    self.step = Step::Style;
                    return ToolStep::next("");
                }
                if is_option(text, "Justify", 1) {
                    self.step = Step::Justify;
                    return ToolStep::next("");
                }
                if is_option(text, "Paper", 1) {
                    self.step = Step::Paper;
                    return ToolStep::next("");
                }
                ToolStep::rejected(format!(
                    "'{text}' is not a point; click the start point or type it as x,y (or S, J \
                     or P for the style, justification or paper height)"
                ))
            }
            Step::Style => self.choose_style(cx, text),
            Step::Justify => {
                let Ok(justify) = text_justify_from_string(text) else {
                    return ToolStep::rejected(format!(
                        "'{text}' is not a justification: TL, TC, TR, ML, MC, MR, BL, BC or BR"
                    ));
                };
                self.justify = justify;
                self.step = Step::Start;
                ToolStep::next("")
            }
            Step::Paper => {
                let Some(paper) = typed_number(text).filter(|&paper| !(paper < 0.0)) else {
                    return ToolStep::rejected(
                        "the height on paper is a number of millimetres, or 0 for a model height",
                    );
                };
                self.paper = paper;
                self.step = Step::Start;
                ToolStep::next("")
            }
            Step::Height => {
                if is_option(text, "Undo", 1) {
                    return self.undo(cx);
                }
                let Some(height) = typed_number(text) else {
                    return ToolStep::rejected("the height must be a number, such as 2.5");
                };
                if !(height > tolerance::GEOMETRIC) {
                    return ToolStep::rejected("a text height must be greater than zero");
                }
                self.height = height;
                self.step = Step::Rotation;
                ToolStep::next("")
            }
            Step::Rotation => {
                if is_option(text, "Undo", 1) {
                    return self.undo(cx);
                }
                let Some(degrees) = typed_number(text) else {
                    return ToolStep::rejected(
                        "the rotation must be an angle in degrees, counter-clockwise from east",
                    );
                };
                self.rotation = degrees * DEG_TO_RAD;
                self.step = Step::Content;
                ToolStep::next("")
            }
            Step::Content => {
                // The router trims, so a line of blanks arrives empty and
                // ends the text as an empty line does.
                if text.is_empty() {
                    return self.enter(cx);
                }
                let position = self.next_position();
                self.lines.push(TypedLine {
                    position,
                    text: String::from(text),
                });
                ToolStep::next("")
            }
        }
    }

    fn enter(&mut self, cx: &ToolContext<'_>) -> ToolStep {
        match self.step {
            Step::Start => {
                let Some(newest) = self.newest.clone().filter(|_| self.mode != Mode::Block) else {
                    return ToolStep::done(None, "", false);
                };
                // DTEXT's Enter at the first prompt: carry on below the
                // newest text, in its height and rotation, without asking
                // again.
                self.height = if self.paper_sized(cx) {
                    self.paper_model_height(cx)
                } else {
                    newest.height
                };
                self.rotation = newest.rotation;
                self.start =
                    newest.position + Self::below(self.rotation) * line_spacing(self.height);
                self.continued = true;
                self.step = Step::Content;
                return ToolStep::next("");
            }
            Step::Style | Step::Justify | Step::Paper => {
                // Enter keeps the choice shown in the prompt.
                self.step = Step::Start;
                return ToolStep::next("");
            }
            Step::Height => {
                self.height = self.default_height;
                self.step = Step::Rotation;
                return ToolStep::next("");
            }
            Step::Rotation => {
                self.rotation = self.default_rotation;
                self.step = Step::Content;
                return ToolStep::next("");
            }
            Step::Content => {}
        }
        if self.lines.is_empty() {
            // Enter with nothing typed ends the tool, so a second Enter is
            // how a user leaves Text without reaching for Esc.
            return ToolStep::done(None, "", false);
        }
        let count = self.lines.len();
        if self.mode == Mode::Block {
            let entities = vec![new_entity(
                Geometry::Text(self.block_text(cx)),
                &self.attributes,
            )];
            let message = if count == 1 {
                String::from("a text of 1 line")
            } else {
                format!("a text of {count} lines")
            };
            return ToolStep::done(Some(create_all("CREATE_TEXT", entities)), message, true);
        }
        let entities = self
            .lines
            .iter()
            .map(|line| {
                new_entity(
                    Geometry::Text(self.made(cx, line.position, line.text.clone())),
                    &self.attributes,
                )
            })
            .collect();
        let message = if count == 1 {
            String::from("1 line of text")
        } else {
            format!("{count} lines of text")
        };
        ToolStep::done(Some(create_all("CREATE_TEXT", entities)), message, true)
    }

    fn undo(&mut self, cx: &ToolContext<'_>) -> ToolStep {
        match self.step {
            Step::Start => ToolStep::rejected("nothing to undo: no start point has been given"),
            Step::Style | Step::Justify | Step::Paper | Step::Height => {
                self.step = Step::Start;
                ToolStep::next("")
            }
            Step::Rotation => {
                // A paper-sized text was never asked its height.
                self.step = if self.paper_sized(cx) {
                    Step::Start
                } else {
                    Step::Height
                };
                ToolStep::next("")
            }
            Step::Content => {
                if self.lines.pop().is_none() {
                    if self.continued {
                        self.continued = false;
                        self.step = Step::Start;
                    } else {
                        self.step = Step::Rotation;
                    }
                }
                ToolStep::next("")
            }
        }
    }

    fn preview(&self, cx: &ToolContext<'_>, cursor: Point2) -> ToolFeedback {
        let mut feedback = ToolFeedback::default();
        match self.step {
            Step::Start | Step::Style | Step::Justify | Step::Paper => {}
            Step::Height | Step::Rotation => {
                // The rubber band the height or the direction is measured
                // along.
                feedback.markers.push(self.start);
                if !coincident(self.start, cursor) {
                    feedback
                        .shapes
                        .push(Geometry::Line(Segment2::new(self.start, cursor)));
                }
            }
            Step::Content => {
                // The lines typed so far, which are not in the drawing until
                // the text is finished, and where the next one will start -
                // for a multiline text, where its block is anchored.
                if self.mode == Mode::Block {
                    if !self.lines.is_empty() {
                        feedback.shapes.push(Geometry::Text(self.block_text(cx)));
                    }
                    feedback.markers.push(self.start);
                } else {
                    for line in &self.lines {
                        feedback.shapes.push(Geometry::Text(self.made(
                            cx,
                            line.position,
                            line.text.clone(),
                        )));
                    }
                    feedback.markers.push(self.next_position());
                }
            }
        }
        feedback
    }

    fn last_point(&self, _cx: &ToolContext<'_>) -> Option<Point2> {
        match self.step {
            Step::Start | Step::Style | Step::Justify | Step::Paper => None,
            Step::Height | Step::Rotation | Step::Content => Some(self.start),
        }
    }
}

/// The Text tool: an entity a line.
pub(crate) fn make_text_tool(cx: &ToolContext<'_>, start: &ToolStart) -> Box<dyn InteractiveTool> {
    Box::new(TextTool::new(cx, start, Mode::Lines))
}

/// The Multiline Text tool: the Text tool's options and prompts, its lines
/// made ONE entity.
pub(crate) fn make_multiline_text_tool(
    cx: &ToolContext<'_>,
    start: &ToolStart,
) -> Box<dyn InteractiveTool> {
    Box::new(TextTool::new(cx, start, Mode::Block))
}
