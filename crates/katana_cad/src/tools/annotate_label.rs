//! Label Objects (src/katana_cad/tools/annotate_label.cpp): labels placed by
//! hand, as the LABEL verb places them.
//!
//! ```text
//!   Select objects to label or [Style/Part/Text], then press Enter
//!                                  picks, or the selection the tool started
//!                                  with; Enter with nothing chosen ends it
//!   Specify text location or [Style/Part/Text/Undo] <automatic>
//!                                  a point pins the text there (LABEL's at=);
//!                                  Enter leaves it to the placer
//! ```
//!
//! Style names the label style, Part a polyline's segment (from 0; Enter for
//! every segment) and Text the label's own words in place of its style's.
//! The style offered first is the one the drawing's newest hand-placed label
//! wears - the last one used, remembered by the drawing rather than by the
//! program, so it is the same after a reopen, as the Text tool offers the
//! newest text's height - when it can label what was chosen, and otherwise
//! the first style by name that can.
//!
//! The labels are made by annotation::auto_label::create_labels, the LABEL
//! verb's own door, as ONE command: "LABEL 12 14 style=... part=... at=x,y
//! text=..." typed and this tool given the same answers make the same
//! entities. Several objects cannot share one place, so with more than one
//! the location prompt takes only Enter. When the labels are made the
//! selection is cleared, so the tool starts again asking for the next objects
//! rather than labelling the same ones twice at the next Enter.
//!
//! The C++ refuses a label text that is not UTF-8; a Rust line is a `&str`,
//! which always is.

use katana_core::text::equals_ignoring_case;
use katana_entity::entity::to_string;
use katana_entity::label_values::labels;
use katana_entity::{Entity, EntityId, Geometry, Model};
use katana_geometry::Point2;

use crate::annotation::auto_label::{LabelRequest, create_labels};
use crate::interactive_tool::{InteractiveTool, ToolContext, ToolInput, ToolStart, ToolStep};
use crate::tool_feedback::ToolFeedback;
use crate::tools::annotate_common::{
    default_entity, default_point, is_option, trimmed, typed_number,
};

// The style of the newest label placed by hand whose style still exists;
// empty when there is none. The entities are visited in id order, which is
// the order they were made.
fn newest_hand_label_style(model: &Model) -> String {
    let mut style = String::new();
    model.entities.for_each(|entity| {
        if let Geometry::Label(label) = &entity.geometry
            && label.rule.is_empty()
            && model.label_styles.contains(&label.style)
        {
            style = label.style.clone();
        }
    });
    style
}

fn style_labels(model: &Model, name: &str, target: &Entity) -> bool {
    model
        .label_styles
        .find(name)
        .is_some_and(|style| labels(style.kind, &target.geometry))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Select,
    Location,
    Style,
    Part,
    Text,
}

// What the options have been told. Each option pushes the settings it
// replaces, so Undo at the location takes back the last option first.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Settings {
    // Empty: the one offered (current_style).
    style: String,
    part: i32,
    text: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            style: String::new(),
            part: -1,
            text: String::new(),
        }
    }
}

struct LabelTool {
    step: Step,
    return_to: Step,
    // The tool asked for the objects (there was no selection when it
    // started), so Undo at the location goes back to asking.
    asked: bool,
    picked: Vec<EntityId>,
    pick_history: Vec<Vec<EntityId>>,
    targets: Vec<EntityId>,
    settings: Settings,
    history: Vec<Settings>,
}

impl LabelTool {
    fn new(start: &ToolStart) -> Self {
        let mut tool = Self {
            step: Step::Select,
            return_to: Step::Select,
            asked: false,
            picked: Vec::new(),
            pick_history: Vec::new(),
            targets: Vec::new(),
            settings: Settings::default(),
            history: Vec::new(),
        };
        if start.selection.is_empty() {
            tool.step = Step::Select;
            tool.asked = true;
        } else {
            tool.targets = start.selection.clone();
            tool.step = Step::Location;
        }
        tool
    }

    fn ask(&mut self, option: Step) -> ToolStep {
        self.return_to = self.step;
        self.step = option;
        ToolStep::next("")
    }

    fn remember(&mut self) {
        self.history.push(self.settings.clone());
    }

    // The style the labels will be made in: the one typed, else the newest
    // hand-placed label's when it can label the first object, else the first
    // by name that can. Empty when the drawing has none.
    fn current_style(&self, cx: &ToolContext<'_>) -> String {
        let Some(document) = cx.document.filter(|_| self.settings.style.is_empty()) else {
            return self.settings.style.clone();
        };
        let model = document.model();
        let first = self.targets.first().and_then(|&id| model.entities.find(id));
        let newest = newest_hand_label_style(model);
        let Some(first) = first else {
            if !newest.is_empty() {
                return newest;
            }
            return model
                .label_styles
                .names()
                .into_iter()
                .next()
                .unwrap_or_default();
        };
        if !newest.is_empty() && style_labels(model, &newest, first) {
            return newest;
        }
        for name in model.label_styles.names() {
            if style_labels(model, &name, first) {
                return name;
            }
        }
        String::new()
    }

    fn style_value(&mut self, cx: &ToolContext<'_>, typed: &str) -> ToolStep {
        let text = trimmed(typed);
        if text.is_empty() {
            return self.enter(cx);
        }
        let Some(document) = cx.document else {
            return ToolStep::rejected("there is no drawing to take the style from");
        };
        let model = document.model();
        // As typed, else the one style of that name in another case.
        let mut found = String::new();
        if model.label_styles.contains(text) {
            found = String::from(text);
        } else {
            for name in model.label_styles.names() {
                if equals_ignoring_case(name.as_bytes(), text.as_bytes()) {
                    found = name;
                    break;
                }
            }
        }
        let Some(style) = model.label_styles.find(&found) else {
            let names = model.label_styles.names().join(", ");
            return ToolStep::rejected(format!(
                "there is no label style \"{text}\"{}",
                if names.is_empty() {
                    String::from("; LABELSTYLE DEFAULTS adds the standard set")
                } else {
                    format!("; the drawing has {names}")
                }
            ));
        };
        // A style that cannot label what was chosen is refused now, in the
        // words LABEL would use, rather than at the end.
        for &id in &self.targets {
            if let Some(target) = model.entities.find(id)
                && !labels(style.kind, &target.geometry)
            {
                return ToolStep::rejected(format!(
                    "a {} label style cannot label a {} (id={id})",
                    to_string(style.kind),
                    to_string(target.r#type())
                ));
            }
        }
        self.remember();
        self.settings.style = found;
        self.step = self.return_to;
        ToolStep::next("")
    }

    fn part_value(&mut self, cx: &ToolContext<'_>, typed: &str) -> ToolStep {
        let text = trimmed(typed);
        if text.is_empty() {
            return self.enter(cx);
        }
        let mut part: i32 = -1;
        if !is_option(text, "All", 1) {
            let Some(number) = typed_number(text).filter(|&number| {
                !(number < 0.0 || number != number.floor() || number > 1_000_000_000.0)
            }) else {
                return ToolStep::rejected(
                    "type the segment's number, 0 for the first, or A for every segment",
                );
            };
            // A whole number from 0 to 10^9, which an i32 holds.
            part = number as i32;
        }
        self.remember();
        self.settings.part = part;
        self.step = self.return_to;
        ToolStep::next("")
    }

    fn choose_targets(&mut self, cx: &ToolContext<'_>) -> ToolStep {
        let mut chosen = self.picked.clone();
        if let Some(document) = cx.document {
            chosen.extend(document.selection().ids());
            chosen.retain(|&id| document.model().entities.contains(id));
        }
        chosen.sort_unstable();
        chosen.dedup();
        if chosen.is_empty() {
            // Nothing chosen: the tool ends, as Text ends at an empty first
            // line - the way out of a tool that starts itself again.
            return ToolStep::done(None, "", false);
        }
        self.targets = chosen;
        self.step = Step::Location;
        let style = self.current_style(cx);
        let count = self.targets.len();
        ToolStep::next(format!(
            "{count}{} to label{}",
            if count == 1 { " object" } else { " objects" },
            if style.is_empty() {
                String::new()
            } else {
                format!(" in style {style}")
            }
        ))
    }

    fn finish(&self, cx: &ToolContext<'_>, position: Option<Point2>) -> ToolStep {
        let Some(document) = cx.document else {
            return ToolStep::rejected("there is no drawing to label");
        };
        let style = self.current_style(cx);
        if style.is_empty() {
            return ToolStep::rejected(
                "the drawing has no label style that can label this; add one in Format > Label \
                 Styles and Rules (LABELSTYLE DEFAULTS adds the standard set)",
            );
        }
        let requests: Vec<LabelRequest> = self
            .targets
            .iter()
            .map(|&id| LabelRequest {
                target: id,
                style: style.clone(),
                part: self.settings.part,
                position,
                text_override: self.settings.text.clone(),
                ..LabelRequest::default()
            })
            .collect();
        let built = match create_labels(document.model(), &requests) {
            Ok(built) => built,
            Err(error) => {
                let context = trimmed(&error.context);
                return ToolStep::rejected(format!(
                    "{}{}",
                    error.message,
                    if context.is_empty() {
                        String::new()
                    } else {
                        format!(" ({context})")
                    }
                ));
            }
        };
        let count = requests.len();
        let mut step = ToolStep::done(
            Some(built),
            format!(
                "{} in style {style}",
                if count == 1 {
                    String::from("1 label")
                } else {
                    format!("{count} labels")
                }
            ),
            true,
        );
        // Labelled: not asked for again at the next Enter.
        step.selection = Some(Vec::new());
        step
    }
}

impl InteractiveTool for LabelTool {
    fn prompt(&self, cx: &ToolContext<'_>) -> String {
        match self.step {
            Step::Select => {
                String::from("Select objects to label or [Style/Part/Text], then press Enter")
            }
            Step::Location => {
                if self.targets.len() == 1 {
                    return String::from(
                        "Specify text location or [Style/Part/Text/Undo] <automatic>",
                    );
                }
                format!(
                    "Press Enter to label the {} objects, each placed automatically, or \
                     [Style/Part/Text/Undo]",
                    self.targets.len()
                )
            }
            Step::Style => {
                let style = self.current_style(cx);
                if style.is_empty() {
                    String::from(
                        "Enter label style name (the drawing has none that fits: LABELSTYLE \
                         DEFAULTS adds the standard set)",
                    )
                } else {
                    format!("Enter label style name <{style}>")
                }
            }
            Step::Part => format!(
                "Enter the polyline segment to label, 0 for the first, or [All] <{}>",
                if self.settings.part < 0 {
                    String::from("all")
                } else {
                    format!("{}", self.settings.part)
                }
            ),
            Step::Text => {
                if self.settings.text.is_empty() {
                    String::from("Enter the label's own text, or press Enter for its style's")
                } else {
                    format!(
                        "Enter the label's own text, or press Enter for its style's <{}>",
                        self.settings.text
                    )
                }
            }
        }
    }

    fn expects(&self, _cx: &ToolContext<'_>) -> ToolInput {
        match self.step {
            Step::Select => ToolInput::Selection,
            Step::Location => ToolInput::Point,
            Step::Style | Step::Part | Step::Text => ToolInput::Value,
        }
    }

    fn point(&mut self, cx: &ToolContext<'_>, at: Point2) -> ToolStep {
        match self.step {
            Step::Select => {
                ToolStep::rejected("select the objects to label first, then press Enter")
            }
            Step::Location => {
                if self.targets.len() != 1 {
                    return ToolStep::rejected(format!(
                        "{} labels cannot share one place; press Enter to place each \
                         automatically",
                        self.targets.len()
                    ));
                }
                self.finish(cx, Some(at))
            }
            Step::Style | Step::Part | Step::Text => default_point(cx, self.expects(cx), at),
        }
    }

    fn entity(&mut self, cx: &ToolContext<'_>, id: EntityId, at: Point2) -> ToolStep {
        if self.step != Step::Select {
            return default_entity(cx, self.expects(cx), id, at);
        }
        if !cx
            .document
            .is_some_and(|document| document.model().entities.contains(id))
        {
            return ToolStep::rejected("that entity is not in the drawing");
        }
        if self.picked.contains(&id) {
            return ToolStep::rejected("that entity is already chosen");
        }
        self.pick_history.push(self.picked.clone());
        self.picked.push(id);
        ToolStep::next("")
    }

    fn value(&mut self, cx: &ToolContext<'_>, text: &str) -> ToolStep {
        match self.step {
            Step::Select | Step::Location => {
                if self.step == Step::Location && is_option(text, "Undo", 1) {
                    return self.undo(cx);
                }
                if is_option(text, "Style", 1) {
                    return self.ask(Step::Style);
                }
                if is_option(text, "Part", 1) {
                    return self.ask(Step::Part);
                }
                if is_option(text, "Text", 1) {
                    return self.ask(Step::Text);
                }
                ToolStep::rejected(if self.step == Step::Select {
                    "pick the objects to label, or type S, P or T"
                } else {
                    "click where the text goes, press Enter to place it automatically, or type \
                     S, P or T"
                })
            }
            Step::Style => self.style_value(cx, text),
            Step::Part => self.part_value(cx, text),
            Step::Text => {
                if text.is_empty() {
                    return self.enter(cx);
                }
                self.remember();
                self.settings.text = String::from(text);
                self.step = self.return_to;
                ToolStep::next("")
            }
        }
    }

    fn enter(&mut self, cx: &ToolContext<'_>) -> ToolStep {
        match self.step {
            Step::Select => self.choose_targets(cx),
            Step::Location => self.finish(cx, None),
            Step::Style | Step::Part => {
                self.step = self.return_to;
                ToolStep::next("")
            }
            Step::Text => {
                // Back to the style's own words, as the dimension tools' text
                // prompt goes back to the measured value.
                if !self.settings.text.is_empty() {
                    self.remember();
                    self.settings.text.clear();
                }
                self.step = self.return_to;
                ToolStep::next("")
            }
        }
    }

    fn undo(&mut self, _cx: &ToolContext<'_>) -> ToolStep {
        match self.step {
            Step::Select => match self.pick_history.pop() {
                Some(previous) => {
                    self.picked = previous;
                    ToolStep::next("")
                }
                None => ToolStep::rejected("nothing to undo: nothing has been picked"),
            },
            Step::Location => {
                if let Some(previous) = self.history.pop() {
                    self.settings = previous;
                    return ToolStep::next("");
                }
                if self.asked {
                    // Back to choosing, with the picks kept.
                    self.targets.clear();
                    self.step = Step::Select;
                    return ToolStep::next("");
                }
                ToolStep::rejected("nothing to undo in this tool")
            }
            Step::Style | Step::Part | Step::Text => {
                self.step = self.return_to;
                ToolStep::next("")
            }
        }
    }

    fn preview(&self, _cx: &ToolContext<'_>, cursor: Point2) -> ToolFeedback {
        let mut feedback = ToolFeedback::default();
        if self.step == Step::Location && self.targets.len() == 1 {
            feedback.markers.push(cursor);
        }
        feedback
    }
}

/// The Label Objects tool.
pub(crate) fn make_label_tool(
    _cx: &ToolContext<'_>,
    start: &ToolStart,
) -> Box<dyn InteractiveTool> {
    Box::new(LabelTool::new(start))
}
