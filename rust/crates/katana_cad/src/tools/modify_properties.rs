//! Match Properties (src/katana_cad/tools/modify_properties.cpp; see
//! families.rs): AutoCAD's MATCHPROP. Pick the object whose properties are
//! wanted, then the objects to give them to, and Enter.
//!
//! An entity here carries three presentation properties of its own - its
//! layer, its style and its colour - and those are what is copied. A linetype
//! and a line weight are not an entity's own: they come from its style, or
//! from its layer when it is ByLayer (entity.rs, tables.rs), so copying the
//! style and the layer copies them, and nothing else could. [Settings]
//! chooses which of the three go across, and is remembered for the next use,
//! as AutoCAD remembers its settings. Everything matched is ONE command, so
//! one undo puts every target back.

use std::cell::RefCell;
use std::rc::Rc;

use katana_commands::{ChangeSet, ChangeSetCommand};
use katana_entity::{Entity, EntityId};
use katana_geometry::Point2;

use crate::interactive_tool::{
    InteractiveTool, ToolCatalog, ToolContext, ToolInfo, ToolInput, ToolStart, ToolStep,
};
use crate::tools::everyday_support::{SelectionStep, counted};
use crate::tools::families::Report;
use crate::tools::modify_edit_support::{
    ToolPtr, base_entity, is_option, refusal_to_edit, tool_factory,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MatchSettings {
    layer: bool,
    colour: bool,
    style: bool,
}

impl Default for MatchSettings {
    fn default() -> Self {
        Self {
            layer: true,
            colour: true,
            style: true,
        }
    }
}

impl MatchSettings {
    // "layer, colour and style", as the prompt and the report say it.
    fn words(&self) -> String {
        let mut parts = Vec::new();
        if self.layer {
            parts.push("layer");
        }
        if self.colour {
            parts.push("colour");
        }
        if self.style {
            parts.push("style");
        }
        let mut text = String::new();
        for (i, part) in parts.iter().enumerate() {
            text.push_str(if i == 0 {
                ""
            } else if i + 1 == parts.len() {
                " and "
            } else {
                ", "
            });
            text.push_str(part);
        }
        text
    }
}

thread_local! {
    // The settings the catalogue entry shares between uses (the C++
    // shared_ptr its factory captures); the program runs its tools on one
    // thread, so they are that thread's (cad_plan.md section 7, risk 4).
    static MATCH_SETTINGS: Rc<RefCell<MatchSettings>> =
        Rc::new(RefCell::new(MatchSettings::default()));
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Source,
    Targets,
    Settings,
}

struct MatchPropertiesTool {
    selection: SelectionStep,
    settings: Rc<RefCell<MatchSettings>>,
    step: Step,
    source: Entity,
}

impl MatchPropertiesTool {
    fn choose_settings(&mut self, text: &str) -> ToolStep {
        let mut chosen = MatchSettings {
            layer: false,
            colour: false,
            style: false,
        };
        let words = text.replace(',', " ");
        let mut any = false;
        for word in words.split(' ').filter(|word| !word.is_empty()) {
            any = true;
            if is_option(word, "Layer", "L") {
                chosen.layer = true;
            } else if is_option(word, "Colour", "C") || is_option(word, "Color", "C") {
                chosen.colour = true;
            } else if is_option(word, "Style", "S") {
                chosen.style = true;
            } else if is_option(word, "All", "A") {
                chosen = MatchSettings::default();
            } else {
                return ToolStep::rejected(format!(
                    "'{word}' is not a property here: Layer, Colour, Style or All"
                ));
            }
        }
        if !any || !(chosen.layer || chosen.colour || chosen.style) {
            return ToolStep::rejected("name at least one of Layer, Colour and Style");
        }
        *self.settings.borrow_mut() = chosen;
        self.step = Step::Targets;
        ToolStep::next(format!("Copying {}", chosen.words()))
    }

    fn apply(&mut self, cx: &ToolContext<'_>) -> ToolStep {
        let mut ids = self.selection.chosen(cx.document);
        ids.retain(|&id| id != self.source.id);
        let nothing = "nothing is selected: select the objects to match, then press Enter";
        if ids.is_empty() {
            return ToolStep::rejected(nothing);
        }
        // With no document no pick was taken, so the selection was empty.
        let Some(document) = cx.document else {
            return ToolStep::rejected(nothing);
        };
        let settings = *self.settings.borrow();
        if settings.layer && refusal_to_edit(document, self.source.id).is_some() {
            // Moving entities onto a locked layer is refused by the command;
            // said here, before anything is picked in vain.
            return ToolStep::rejected(format!(
                "the source's layer '{}' is locked, so nothing can be moved onto it",
                self.source.layer
            ));
        }
        let mut changes = ChangeSet::default();
        let mut locked = 0;
        for &id in &ids {
            let Some(target) = document.model().entities.find(id) else {
                continue;
            };
            if refusal_to_edit(document, id).is_some() {
                locked += 1;
                continue;
            }
            let mut matched = target.clone();
            if settings.layer {
                matched.layer = self.source.layer.clone();
            }
            if settings.style {
                matched.style = self.source.style.clone();
            }
            if settings.colour {
                matched.color = self.source.color;
            }
            if matched != *target {
                changes.modify.push(matched);
            }
        }
        let note = if locked == 0 {
            String::new()
        } else {
            format!("; {locked} on a locked layer left as it is")
        };
        if changes.modify.is_empty() {
            return ToolStep::done(
                None,
                format!(
                    "Nothing to change: the selected objects already have the source's {}{note}",
                    settings.words()
                ),
                false,
            );
        }
        let count = changes.modify.len();
        let command = Box::new(ChangeSetCommand::new(
            "MATCHPROP",
            move |_context| Ok(changes.clone()),
            false,
        ));
        ToolStep::done(
            Some(command),
            format!(
                "Matched {} on {}{note}",
                settings.words(),
                counted(count, "object", "objects")
            ),
            false,
        )
    }
}

impl InteractiveTool for MatchPropertiesTool {
    fn prompt(&self, _cx: &ToolContext<'_>) -> String {
        match self.step {
            Step::Source => String::from("Select the source object"),
            Step::Targets => {
                String::from("Select destination objects or [Settings], then press Enter")
            }
            Step::Settings => format!(
                "Properties to copy: any of [Layer/Colour/Style], or All <{}>",
                self.settings.borrow().words()
            ),
        }
    }

    fn expects(&self, _cx: &ToolContext<'_>) -> ToolInput {
        match self.step {
            Step::Source => ToolInput::Entity,
            Step::Targets => ToolInput::Selection,
            Step::Settings => ToolInput::Value,
        }
    }

    fn entity(&mut self, cx: &ToolContext<'_>, id: EntityId, at: Point2) -> ToolStep {
        if self.step == Step::Targets {
            return self.selection.pick(cx.document, id);
        }
        if self.step != Step::Source {
            return base_entity(cx, self.expects(cx), id, at);
        }
        let Some(entity) = cx
            .document
            .and_then(|document| document.model().entities.find(id))
        else {
            return ToolStep::rejected("that entity is not in the drawing");
        };
        self.source = entity.clone();
        self.step = Step::Targets;
        ToolStep::next(format!(
            "Source: layer {}, style {}, colour {}",
            self.source.layer,
            if self.source.style.is_empty() {
                "ByLayer"
            } else {
                self.source.style.as_str()
            },
            match self.source.color {
                Some(color) => color.to_hex(),
                None => String::from("ByLayer"),
            }
        ))
    }

    fn value(&mut self, cx: &ToolContext<'_>, text: &str) -> ToolStep {
        if self.step == Step::Settings {
            return self.choose_settings(text);
        }
        if self.step == Step::Targets {
            if is_option(text, "Settings", "S") {
                self.step = Step::Settings;
                return ToolStep::next("");
            }
            if is_option(text, "All", "A") {
                return self.selection.all(cx.document);
            }
        }
        ToolStep::rejected(format!("'{text}' is not an option here"))
    }

    fn enter(&mut self, cx: &ToolContext<'_>) -> ToolStep {
        match self.step {
            Step::Source => ToolStep::done(None, "", false),
            Step::Settings => {
                self.step = Step::Targets; // the settings stay as they are
                ToolStep::next("")
            }
            Step::Targets => self.apply(cx),
        }
    }

    fn undo(&mut self, _cx: &ToolContext<'_>) -> ToolStep {
        match self.step {
            Step::Settings => {
                self.step = Step::Targets;
                ToolStep::next("")
            }
            Step::Targets => {
                if !self.selection.undo() {
                    self.step = Step::Source;
                }
                ToolStep::next("")
            }
            Step::Source => ToolStep::rejected("nothing to undo in this tool"),
        }
    }
}

fn match_properties_tool(_cx: &ToolContext<'_>, _start: &ToolStart) -> ToolPtr {
    Box::new(MatchPropertiesTool {
        selection: SelectionStep::new(),
        settings: MATCH_SETTINGS.with(Rc::clone),
        step: Step::Source,
        source: Entity::default(),
    })
}

pub fn add_property_tools(catalog: &mut ToolCatalog, report: &mut Report<'_>) {
    let info = ToolInfo {
        id: String::from("modify.match_properties"),
        name: String::from("Match Properties"),
        category: String::from("Modify"),
        group: String::from("Properties"),
        order: 10,
        aliases: vec![
            String::from("MATCHPROP"),
            String::from("MA"),
            String::from("PAINTER"),
        ],
        tip: String::from(
            "Gives objects the layer, colour and style of a source object - and with the style \
             and the layer their linetype and weight; S chooses which are copied.",
        ),
        make: Some(tool_factory(match_properties_tool)),
        ..ToolInfo::default()
    };
    report(catalog.add(info));
}
