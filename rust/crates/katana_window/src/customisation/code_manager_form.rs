//! The survey code manager's rule form: one page per kind of section, reading a rule in and
//! writing a rule out, with lint at every edit. The form edits the BUFFER only, and only through
//! the buttons: a table's own change never edits anything (docs/cad.md). The port of
//! customisation/code_manager_form.
//!
//! Every page is drawn, the ones behind offstage, so a driver fills a field of a section that is
//! not the one shown, as it did on a hidden page of the C++'s stack.

use egui::{RichText, Ui};
use katana_cad::Document;
use katana_cad::code_edit::{format_attribute_lines, parse_attribute_lines};
use katana_cad::code_table::{
    BuiltInSymbolTest, ColourLookup, LintKind, LintSeverity, lint_survey_rule,
};
use katana_cad::colour_lookup::colour_lookup;
use katana_cad::style_catalogue::NameRole;
use katana_core::error::make_error;
use katana_core::text::{format_exact_real, parse_finite_double, trimmed};
use katana_core::{Error, ErrorCode, Result, Status};
use katana_entity::entity::to_string;
use katana_entity::tables::is_built_in_symbol_name;
use katana_entity::{
    SurveyAttribute, SurveyBreakline, SurveyPipe, SurveyRule, SurveySection, validate,
};

use super::code_manager::{Event, REFUSAL_COLOUR, SurveyCodeManagerDialog};
use super::code_manager_support::ColourField;
use super::codes_seams::{NamePicker, UNDEFINED_NAME_COLOUR};
use super::definition_editor::index_choice;
use crate::drive::controls::{self, Choice, LineOptions, TextOptions, custom};
use crate::drive::registry::{ControlKind, ControlRecord};
use crate::host::Host;
use crate::survey::survey_dialogs::offstage;

/// The sections the form offers, in the Section list's order.
const SECTIONS: [SurveySection; 9] = [
    SurveySection::Map,
    SurveySection::VertexSymbol,
    SurveySection::VertexTextStyle,
    SurveySection::Pipe,
    SurveySection::VertexPipe,
    SurveySection::SegmentPipe,
    SurveySection::StringAttribute,
    SurveySection::VertexAttribute,
    SurveySection::Tinable,
];

const MAP_PAGE: usize = 0;
const SYMBOL_PAGE: usize = 1;
const TEXT_PAGE: usize = 2;
const PIPE_PAGE: usize = 3;
const ATTRIBUTES_PAGE: usize = 4;
const TINABLE_PAGE: usize = 5;
const PAGE_COUNT: usize = 6;

fn page_for(section: SurveySection) -> usize {
    match section {
        SurveySection::Map => MAP_PAGE,
        SurveySection::VertexSymbol => SYMBOL_PAGE,
        SurveySection::VertexTextStyle => TEXT_PAGE,
        SurveySection::Pipe | SurveySection::VertexPipe | SurveySection::SegmentPipe => PIPE_PAGE,
        SurveySection::StringAttribute | SurveySection::VertexAttribute => ATTRIBUTES_PAGE,
        SurveySection::Tinable => TINABLE_PAGE,
    }
}

fn pipe_of(rule: &mut SurveyRule) -> &mut Option<SurveyPipe> {
    match rule.section {
        SurveySection::VertexPipe => &mut rule.vertex_pipe,
        SurveySection::SegmentPipe => &mut rule.segment_pipe,
        _ => &mut rule.pipe,
    }
}

fn attributes_of(rule: &mut SurveyRule) -> &mut Vec<SurveyAttribute> {
    if rule.section == SurveySection::VertexAttribute {
        &mut rule.vertex_attributes
    } else {
        &mut rule.attributes
    }
}

fn number_text(value: f64) -> String {
    if value == 0.0 {
        String::new()
    } else {
        format_exact_real(value)
    }
}

/// core's trimmed, of text: the ASCII blanks at its ends taken off.
pub(super) fn ascii_trimmed(text: &str) -> &str {
    std::str::from_utf8(trimmed(text.as_bytes())).unwrap_or(text)
}

/// A number field: empty is "not given", which the map keeps as 0.
fn number(field: &str, what: &str) -> Result<f64> {
    let typed = ascii_trimmed(field);
    if typed.is_empty() {
        return Ok(0.0);
    }
    parse_finite_double(typed).ok_or_else(|| {
        Error::new(
            ErrorCode::InvalidArgument,
            format!("{what} \"{typed}\" is not a number"),
        )
    })
}

/// The choices of the three editable boxes; a typed value is kept in its own case.
const TEXT_TYPES: [&str; 3] = ["", "paper", "world"];
const PIPE_JUSTIFIES: [&str; 4] = ["", "Obvert", "Invert", "Centre"];
const PIPE_SHAPES: [&str; 3] = ["", "diameter", "culvert"];

fn items_of(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| String::from(*item)).collect()
}

/// A box holding text as written, listed or not.
fn set_choice(choice: &mut Choice, items: &[&str], value: &str) {
    choice.index = items.iter().position(|item| *item == value);
    choice.text = String::from(value);
}

/// How the form's issues are shown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Tone {
    #[default]
    Plain,
    Warning,
    Refusal,
}

/// What the rule form holds: its fields as they are shown, and what follows from them.
pub(super) struct RuleForm {
    pub(super) title: String,
    pub(super) key: String,
    pub(super) section: usize,
    pub(super) comment: String,
    // feature: where the code goes and how its line is drawn.
    pub(super) model: String,
    pub(super) model_status: String,
    pub(super) model_bad: bool,
    pub(super) colour: ColourField,
    pub(super) breakline: usize,
    pub(super) linestyle: NamePicker,
    pub(super) weight: String,
    pub(super) group: String,
    // symbol: the symbol at each point.
    pub(super) symbol: NamePicker,
    pub(super) symbol_colour: ColourField,
    pub(super) symbol_size: String,
    pub(super) symbol_rotation: String,
    pub(super) symbol_offset: String,
    pub(super) symbol_raise: String,
    pub(super) hide: usize,
    // text: the fields a person sets; the rest are kept.
    pub(super) text_style: String,
    pub(super) text_colour: ColourField,
    pub(super) text_type: Choice,
    pub(super) text_size: String,
    pub(super) text_justify_x: String,
    pub(super) text_justify_y: String,
    // The three pipe sections.
    pub(super) pipe_justify: Choice,
    pub(super) pipe_shape: Choice,
    pub(super) pipe_size1: String,
    pub(super) pipe_size2: String,
    pub(super) pipe_active: bool,
    // The two attribute sections, and surface.
    pub(super) attributes: String,
    pub(super) tinable: usize,
    pub(super) issues: String,
    pub(super) issues_tone: Tone,
    /// The form holds a rule of the buffer: Save, Duplicate and Delete can be pressed.
    pub(super) indexed: bool,
    pub(super) up: bool,
    pub(super) down: bool,
    /// The linestyle and symbol names the form was last loaded with. Once the drawing has gone
    /// the pickers cannot be loaded (they read its library), so form_rule takes these instead of
    /// what the pickers still hold - the names of whichever rule was loaded before the drawing
    /// closed.
    pub(super) loaded_linestyle: String,
    pub(super) loaded_symbol: String,
    pub(super) pickers_loaded: bool,
}

impl RuleForm {
    pub(super) fn new(document: Option<&Document>) -> Self {
        Self {
            title: String::new(),
            key: String::new(),
            section: 0,
            comment: String::new(),
            model: String::new(),
            model_status: String::new(),
            model_bad: false,
            colour: ColourField::new(document),
            breakline: 0,
            linestyle: NamePicker::new(NameRole::Linetype, false),
            weight: String::new(),
            group: String::new(),
            symbol: NamePicker::new(NameRole::Symbol, false),
            symbol_colour: ColourField::new(document),
            symbol_size: String::new(),
            symbol_rotation: String::new(),
            symbol_offset: String::new(),
            symbol_raise: String::new(),
            hide: 0,
            text_style: String::new(),
            text_colour: ColourField::new(document),
            text_type: Choice::editable(""),
            text_size: String::new(),
            text_justify_x: String::new(),
            text_justify_y: String::new(),
            pipe_justify: Choice::editable(""),
            pipe_shape: Choice::editable(""),
            pipe_size1: String::new(),
            pipe_size2: String::new(),
            pipe_active: false,
            attributes: String::new(),
            tinable: 0,
            issues: String::new(),
            issues_tone: Tone::Plain,
            indexed: false,
            up: false,
            down: false,
            loaded_linestyle: String::new(),
            loaded_symbol: String::new(),
            pickers_loaded: true,
        }
    }

    /// The names the fields list, filled again: the customisation's colours or the library have
    /// changed. What each field shows is kept.
    pub(super) fn refresh_names(&mut self, document: &Document) {
        for field in [
            &mut self.colour,
            &mut self.symbol_colour,
            &mut self.text_colour,
        ] {
            field.refresh(Some(document));
        }
        self.linestyle.refresh(document);
        self.symbol.refresh(document);
    }

    fn the_section(&self) -> SurveySection {
        SECTIONS[self.section.min(SECTIONS.len() - 1)]
    }
}

/// A labelled field of a page: true when it was edited.
fn field(ui: &mut Ui, label: &str, name: &str, value: &mut String, tip: &str, hint: &str) -> bool {
    ui.horizontal(|ui| {
        ui.label(label);
        let mut response = controls::line_with(
            ui,
            name,
            value,
            LineOptions {
                placeholder: hint,
                ..LineOptions::default()
            },
        );
        if !tip.is_empty() {
            response = response.on_hover_text(tip);
        }
        response.changed()
    })
    .inner
}

/// A labelled box of fixed choices, by its row.
fn pick(ui: &mut Ui, label: &str, name: &str, items: &[&str], index: &mut usize) -> bool {
    ui.horizontal(|ui| {
        ui.label(label);
        index_choice(ui, name, &items_of(items), index)
    })
    .inner
}

/// A labelled box whose text may be typed.
fn typed(ui: &mut Ui, label: &str, name: &str, items: &[&str], chosen: &mut Choice) -> bool {
    ui.horizontal(|ui| {
        ui.label(label);
        controls::choice(ui, name, &items_of(items), chosen).changed()
    })
    .inner
}

/// One page of the form; true when a field of it was edited.
fn page_ui(form: &mut RuleForm, page: usize, ui: &mut Ui) -> bool {
    let mut edited = false;
    match page {
        MAP_PAGE => {
            edited |= field(
                ui,
                "Layer",
                "ruleModel",
                &mut form.model,
                "The layer the code's entities go on",
                "",
            );
            let mut status = RichText::new(form.model_status.as_str());
            if form.model_bad {
                status = status.color(REFUSAL_COLOUR);
            }
            controls::label_as(
                ui,
                "ruleModelStatus",
                &form.model_status,
                egui::Label::new(status).wrap(),
            );
            ui.horizontal(|ui| {
                ui.label("Colour");
                edited |= form.colour.ui(ui, "ruleColour").changed();
            });
            edited |= pick(
                ui,
                "Line / point",
                "ruleBreakline",
                &["(not set)", "Line", "Point"],
                &mut form.breakline,
            );
            ui.horizontal(|ui| {
                ui.label("Linestyle");
                ui.add_enabled_ui(form.pickers_loaded, |ui| {
                    edited |= form.linestyle.ui(ui, "ruleLinestyle");
                });
            });
            edited |= field(ui, "Weight", "ruleWeight", &mut form.weight, "", "");
            edited |= field(ui, "Group", "ruleGroup", &mut form.group, "", "");
        }
        SYMBOL_PAGE => {
            ui.horizontal(|ui| {
                ui.label("Symbol");
                ui.add_enabled_ui(form.pickers_loaded, |ui| {
                    edited |= form.symbol.ui(ui, "ruleSymbol");
                });
            });
            ui.horizontal(|ui| {
                ui.label("Colour");
                edited |= form.symbol_colour.ui(ui, "ruleSymbolColour").changed();
            });
            edited |= field(
                ui,
                "Size",
                "ruleSymbolSize",
                &mut form.symbol_size,
                "",
                "the definition's own size",
            );
            edited |= field(
                ui,
                "Rotation",
                "ruleSymbolRotation",
                &mut form.symbol_rotation,
                "",
                "",
            );
            edited |= field(
                ui,
                "Offset",
                "ruleSymbolOffset",
                &mut form.symbol_offset,
                "",
                "",
            );
            edited |= field(
                ui,
                "Raise",
                "ruleSymbolRaise",
                &mut form.symbol_raise,
                "",
                "",
            );
            edited |= pick(
                ui,
                "Hide the line",
                "ruleHide",
                &["(not set)", "no", "yes"],
                &mut form.hide,
            );
        }
        TEXT_PAGE => {
            edited |= field(
                ui,
                "Text style",
                "ruleTextStyle",
                &mut form.text_style,
                "",
                "",
            );
            ui.horizontal(|ui| {
                ui.label("Colour");
                edited |= form.text_colour.ui(ui, "ruleTextColour").changed();
            });
            edited |= typed(
                ui,
                "Size type",
                "ruleTextType",
                &TEXT_TYPES,
                &mut form.text_type,
            );
            edited |= field(ui, "Size", "ruleTextSize", &mut form.text_size, "", "");
            edited |= field(
                ui,
                "Justify across",
                "ruleTextJustifyX",
                &mut form.text_justify_x,
                "",
                "",
            );
            edited |= field(
                ui,
                "Justify up",
                "ruleTextJustifyY",
                &mut form.text_justify_y,
                "",
                "",
            );
        }
        PIPE_PAGE => {
            edited |= typed(
                ui,
                "Justify",
                "rulePipeJustify",
                &PIPE_JUSTIFIES,
                &mut form.pipe_justify,
            );
            edited |= typed(
                ui,
                "Shape",
                "rulePipeShape",
                &PIPE_SHAPES,
                &mut form.pipe_shape,
            );
            edited |= field(
                ui,
                "Size 1",
                "rulePipeSize1",
                &mut form.pipe_size1,
                "A number, or another attribute's name: $PipeDiameter",
                "",
            );
            edited |= field(ui, "Size 2", "rulePipeSize2", &mut form.pipe_size2, "", "");
            edited |= controls::check(ui, "rulePipeActive", "Active", &mut form.pipe_active)
                .changed();
        }
        ATTRIBUTES_PAGE => {
            ui.label("One attribute a line: <type> <name> = <value>, the type text or integer");
            edited |= controls::text_with(
                ui,
                "ruleAttributes",
                &mut form.attributes,
                TextOptions::default(),
            )
            .changed();
        }
        _ => {
            edited |= pick(
                ui,
                "Goes into a surface",
                "ruleTinable",
                &["(not set)", "yes", "no"],
                &mut form.tinable,
            );
        }
    }
    edited
}

impl SurveyCodeManagerDialog {
    pub(super) fn load_form(
        &mut self,
        document: Option<&Document>,
        rule: &SurveyRule,
        index: Option<usize>,
    ) {
        self.form_index = index;
        let form = &mut self.form;
        form.title = match index {
            Some(index) => format!("Rule #{index}"),
            None => String::from("New rule (not in the map)"),
        };
        form.key = rule.key.clone();
        form.section = SECTIONS
            .iter()
            .position(|section| *section == rule.section)
            .unwrap_or(0);
        form.comment = rule.comment.clone();

        form.model = rule.model.clone();
        form.colour.set(&rule.colour);
        form.breakline = match rule.breakline {
            None => 0,
            Some(SurveyBreakline::Line) => 1,
            Some(SurveyBreakline::Point) => 2,
        };
        // The pickers read the Document's library for what they list and draw, so once the
        // drawing has closed they cannot be loaded. Then each shows the rule's name as plain text
        // and is disabled - what it shows is what form_rule writes, from loaded_linestyle and
        // loaded_symbol, and a name typed there could be neither checked nor kept.
        let symbol = rule.symbol.clone().unwrap_or_default();
        form.pickers_loaded = document.is_some();
        form.loaded_linestyle = rule.linestyle.clone();
        form.loaded_symbol = symbol.style.clone();
        match document {
            Some(document) => {
                form.linestyle.set_current_name(document, &rule.linestyle);
                form.symbol.set_current_name(document, &symbol.style);
            }
            None => {
                form.linestyle.set_edit_text(&rule.linestyle);
                form.symbol.set_edit_text(&symbol.style);
            }
        }
        form.weight = rule.weight.clone();
        form.group = rule.group.clone();

        form.symbol_colour.set(&symbol.colour);
        form.symbol_size = number_text(symbol.size);
        form.symbol_rotation = number_text(symbol.rotation);
        form.symbol_offset = number_text(symbol.offset);
        form.symbol_raise = number_text(symbol.raise);
        form.hide = match rule.hide {
            None => 0,
            Some(false) => 1,
            Some(true) => 2,
        };

        let style = rule.text_style.clone().unwrap_or_default();
        form.text_style = style.textstyle.clone();
        form.text_colour.set(&style.colour);
        set_choice(&mut form.text_type, &TEXT_TYPES, &style.r#type);
        form.text_size = number_text(style.size);
        form.text_justify_x = style.justify_x.clone();
        form.text_justify_y = style.justify_y.clone();

        let mut copy = rule.clone();
        let pipe = pipe_of(&mut copy).clone().unwrap_or_default();
        set_choice(&mut form.pipe_justify, &PIPE_JUSTIFIES, &pipe.justify);
        set_choice(&mut form.pipe_shape, &PIPE_SHAPES, &pipe.shape);
        form.pipe_size1 = pipe.size1.clone();
        form.pipe_size2 = pipe.size2.clone();
        form.pipe_active = pipe.active;

        form.attributes = format_attribute_lines(attributes_of(&mut copy));
        form.tinable = match rule.tinable {
            None => 0,
            Some(true) => 1,
            Some(false) => 2,
        };

        form.indexed = index.is_some();
        form.up = index.is_some_and(|index| index > 0);
        form.down = index.is_some_and(|index| index + 1 < self.buffer.size());
        self.update_form_issues(document);
    }

    /// What the form holds, as a rule - InvalidArgument for a field that does not parse (a size
    /// that is not a number, an attribute line).
    pub fn form_rule(&self) -> Result<SurveyRule> {
        let form = &self.form;
        let section = form.the_section();
        // Start from the rule being edited, so that what the form does not show (a text style's
        // slant, a pipe rule's attributes) is kept - unless the section changed, when the old
        // section's fields would be ones the new section is not about (the lint reports a field
        // outside its section).
        let mut rule = self
            .form_index
            .and_then(|index| self.buffer.at(index).ok())
            .filter(|current| current.section == section)
            .unwrap_or_default();
        rule.section = section;
        rule.key = form.key.clone();
        rule.comment = form.comment.clone();

        match page_for(section) {
            MAP_PAGE => {
                rule.model = form.model.clone();
                rule.colour = form.colour.text();
                rule.breakline = match form.breakline {
                    1 => Some(SurveyBreakline::Line),
                    2 => Some(SurveyBreakline::Point),
                    _ => None,
                };
                rule.linestyle = if form.pickers_loaded {
                    form.linestyle.current_name()
                } else {
                    form.loaded_linestyle.clone()
                };
                rule.weight = form.weight.clone();
                rule.group = form.group.clone();
            }
            SYMBOL_PAGE => {
                let mut symbol = rule.symbol.clone().unwrap_or_default();
                symbol.style = if form.pickers_loaded {
                    form.symbol.current_name()
                } else {
                    form.loaded_symbol.clone()
                };
                symbol.colour = form.symbol_colour.text();
                symbol.size = number(&form.symbol_size, "the size")?;
                symbol.rotation = number(&form.symbol_rotation, "the rotation")?;
                symbol.offset = number(&form.symbol_offset, "the offset")?;
                symbol.raise = number(&form.symbol_raise, "the raise")?;
                rule.symbol = Some(symbol);
                rule.hide = match form.hide {
                    0 => None,
                    hide => Some(hide == 2),
                };
            }
            TEXT_PAGE => {
                let mut style = rule.text_style.clone().unwrap_or_default();
                style.textstyle = form.text_style.clone();
                style.colour = form.text_colour.text();
                style.r#type = form.text_type.text.clone();
                style.size = number(&form.text_size, "the text size")?;
                style.justify_x = form.text_justify_x.clone();
                style.justify_y = form.text_justify_y.clone();
                rule.text_style = Some(style);
            }
            PIPE_PAGE => {
                let mut pipe = pipe_of(&mut rule).clone().unwrap_or_default();
                pipe.justify = form.pipe_justify.text.clone();
                pipe.shape = form.pipe_shape.text.clone();
                pipe.size1 = form.pipe_size1.clone();
                pipe.size2 = form.pipe_size2.clone();
                pipe.active = form.pipe_active;
                *pipe_of(&mut rule) = Some(pipe);
            }
            ATTRIBUTES_PAGE => {
                *attributes_of(&mut rule) = parse_attribute_lines(&form.attributes)?;
            }
            _ => {
                rule.tinable = match form.tinable {
                    0 => None,
                    tinable => Some(tinable == 1),
                };
            }
        }
        Ok(rule)
    }

    pub(super) fn update_form_issues(&mut self, document: Option<&Document>) {
        let rule = match self.form_rule() {
            Ok(rule) => rule,
            Err(error) => {
                self.form.issues = error.describe();
                self.form.issues_tone = Tone::Refusal;
                return;
            }
        };
        let index = self.form_index.unwrap_or(self.buffer.size());
        let issues = match document {
            None => Vec::new(),
            Some(document) => {
                let colours = colour_lookup(document);
                let colour_of: &ColourLookup<'_> = &colours;
                let built_in: &BuiltInSymbolTest<'_> = &is_built_in_symbol_name;
                lint_survey_rule(
                    &rule,
                    index,
                    document.style_library(),
                    Some(colour_of),
                    Some(built_in),
                )
            }
        };

        // The layer, checked as the entity tables check it (lint's InvalidLayerPath), and whether
        // applying the code will make it.
        let mut layer_status = String::new();
        let mut layer_bad = false;
        for issue in &issues {
            if issue.kind == LintKind::InvalidLayerPath {
                layer_status = issue.message.clone();
                layer_bad = true;
            }
        }
        if !layer_bad {
            layer_status = String::from(if rule.model.is_empty() {
                "No layer: the code's entities stay on theirs."
            } else if document.is_some_and(|document| document.model().layers.contains(&rule.model))
            {
                "A valid layer the drawing has."
            } else {
                "A valid layer, created when codes are applied."
            });
        }
        self.form.model_status = layer_status;
        self.form.model_bad = layer_bad;

        let mut lines = Vec::new();
        let mut any_error = false;
        if let Err(error) = validate(&rule) {
            lines.push(error.describe());
            any_error = true;
        }
        for issue in &issues {
            any_error = any_error || issue.severity == LintSeverity::Error;
            lines.push(format!("{}: {}", to_string(issue.severity), issue.message));
        }
        self.form.issues_tone = if any_error {
            Tone::Refusal
        } else if lines.is_empty() {
            Tone::Plain
        } else {
            Tone::Warning
        };
        self.form.issues = if lines.is_empty() {
            String::from("No issues with this rule.")
        } else {
            lines.join("\n")
        };
    }

    /// What one of the form's buttons does; a refusal is said under the form and in the log.
    pub(super) fn form_pressed(&mut self, host: &mut dyn Host, event: &Event) {
        let document = self.live_document(host.document());
        let nothing_selected = |what: &str| -> Error {
            make_error(
                ErrorCode::InvalidState,
                format!("no rule is selected to {what}"),
                "",
            )
        };
        let status: Status = match event {
            Event::FormNew => {
                self.tree_current = None;
                self.load_form(document, &SurveyRule::default(), None);
                Ok(())
            }
            Event::FormAdd => self
                .form_rule()
                .and_then(|rule| self.add_rule_in(document, rule)),
            Event::FormUpdate => self.form_rule().and_then(|rule| match self.form_index {
                Some(index) => self.replace_rule_in(document, index, rule),
                None => Err(nothing_selected("save over")),
            }),
            Event::FormDuplicate => match self.form_index {
                Some(index) => self.duplicate_rule_in(document, index),
                None => Err(nothing_selected("duplicate")),
            },
            Event::FormDelete => match self.form_index {
                Some(index) => self.remove_rule_in(document, index),
                None => Err(nothing_selected("delete")),
            },
            Event::FormUp => match self.form_index {
                Some(index) => self.move_rule_in(document, index, -1),
                None => Err(nothing_selected("move")),
            },
            Event::FormDown => match self.form_index {
                Some(index) => self.move_rule_in(document, index, 1),
                None => Err(nothing_selected("move")),
            },
            _ => Ok(()),
        };
        if let Err(error) = status {
            self.form.issues = error.describe();
            self.form.issues_tone = Tone::Refusal;
            self.log(error.describe(), true);
        }
    }

    pub(super) fn rule_form_ui(&mut self, ui: &mut Ui) {
        let mut edited = false;
        let drawn = ui.vertical(|ui| {
            let form = &mut self.form;
            controls::label_as(
                ui,
                "ruleFormTitle",
                &form.title,
                egui::Label::new(RichText::new(form.title.as_str()).strong()),
            );
            edited |= field(
                ui,
                "Key",
                "ruleKey",
                &mut form.key,
                "An exact code (PABB) or a prefix ending in * (WM*); matching is exact, letter \
                 case included",
                "",
            );
            let words: Vec<&str> = SECTIONS.iter().map(|section| to_string(*section)).collect();
            if pick(ui, "Section", "ruleSection", &words, &mut form.section) {
                self.events.push(Event::SectionChosen);
            }
            edited |= field(ui, "Description", "ruleComment", &mut form.comment, "", "");

            let front = page_for(form.the_section());
            let pages = ui.vertical(|ui| {
                for page in 0..PAGE_COUNT {
                    if page == front {
                        edited |= page_ui(form, page, ui);
                    } else {
                        let salt = format!("rulePage{page}");
                        edited |= offstage(ui, &salt, |ui| page_ui(form, page, ui));
                    }
                }
            });
            custom(
                ui,
                &pages.response,
                ControlRecord::new("rulePages", ControlKind::Other),
            );

            let mut issues = RichText::new(form.issues.as_str());
            match form.issues_tone {
                Tone::Plain => {}
                Tone::Warning => issues = issues.color(UNDEFINED_NAME_COLOUR.to_egui()),
                Tone::Refusal => issues = issues.color(REFUSAL_COLOUR),
            }
            controls::label_as(
                ui,
                "ruleIssues",
                &form.issues,
                egui::Label::new(issues).wrap(),
            );

            let events = &mut self.events;
            ui.horizontal(|ui| {
                if controls::button(ui, "ruleNew", "New").clicked() {
                    events.push(Event::FormNew);
                }
                if controls::button(ui, "ruleAdd", "Add")
                    .on_hover_text("Add the form's rule after every other rule")
                    .clicked()
                {
                    events.push(Event::FormAdd);
                }
                ui.add_enabled_ui(form.indexed, |ui| {
                    if controls::button(ui, "ruleUpdate", "Save")
                        .on_hover_text("Replace the selected rule with the form's")
                        .clicked()
                    {
                        events.push(Event::FormUpdate);
                    }
                    if controls::button(ui, "ruleDuplicate", "Duplicate").clicked() {
                        events.push(Event::FormDuplicate);
                    }
                });
            });
            ui.horizontal(|ui| {
                ui.add_enabled_ui(form.indexed, |ui| {
                    if controls::button(ui, "ruleDelete", "Delete").clicked() {
                        events.push(Event::FormDelete);
                    }
                });
                ui.add_enabled_ui(form.up, |ui| {
                    if controls::button(ui, "ruleUp", "Move Up")
                        .on_hover_text("Earlier: among rules of one key, the earlier wins")
                        .clicked()
                    {
                        events.push(Event::FormUp);
                    }
                });
                ui.add_enabled_ui(form.down, |ui| {
                    if controls::button(ui, "ruleDown", "Move Down").clicked() {
                        events.push(Event::FormDown);
                    }
                });
            });
        });
        custom(
            ui,
            &drawn.response,
            ControlRecord::new("ruleForm", ControlKind::Other),
        );
        // Live checking: every field re-lints the form's rule. Nothing is written to the buffer
        // until a button says so.
        if edited {
            self.events.push(Event::FormEdited);
        }
    }
}
