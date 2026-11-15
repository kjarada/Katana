//! The Text Style and Label Style managers (Format > Text Styles..., Format > Label Styles and
//! Rules...; docs/annotation.md "In the window"). The port of annotation/annotation_managers.
//!
//! Each is a list of the table's items and a form for the chosen one. They change nothing
//! themselves: every button builds the verb line a person would type - TEXTSTYLE, LABELSTYLE,
//! AUTOLABEL - and runs it through the window's one executor (host::Host::run), so an edit is
//! echoed in the command log, undone as a typed line is, and refused in the verb's own words in
//! the dialog's problem line. Apply sends only the fields the form changed, as one line (one undo
//! step), and nothing when none did. The dialogs hold no copy of the tables: every refresh reads
//! the document, so an undo or a verb typed while one is open shows at once.
//!
//! Object names, for the headless driver and the tests:
//!
//! ```text
//! textStyleManagerDialog
//!   textStyleList, textStyleNewName (blank: the fresh name its placeholder shows),
//!   textStyleNew, textStyleDelete, textStyleApply, textStyleProblem; the form: textStyleFont,
//!   textStylePaperHeight, textStyleWidthFactor, textStyleOblique, textStyleBold,
//!   textStyleItalic, textStyleColour, textStyleMask, textStyleMaskMargin, textStyleReadable,
//!   textStyleLineSpacing, textStyleSample
//! labelStyleManagerDialog, its tabs labelStyleTabs
//!   Label Styles: labelStyleList, labelStyleNewName, labelStyleNewKind, labelStyleNew,
//!   labelStyleDelete, labelStyleDefaults, labelStyleApply; the form: labelStyleKind,
//!   labelStyleTemplate, labelTemplateInsert (the values a template of the kind may use, each with
//!   its steps: an action "labelValue:VALUE" or "labelValue:VALUE:STEP" puts {VALUE:STEP} at the
//!   cursor), labelStyleTemplateCheck, labelStyleTextStyle, labelStylePaperHeight,
//!   labelStylePlacement, labelStyleOrientation, labelStyleOffset, labelStyleLeader,
//!   labelStyleDisplace, labelStylePriority, labelStyleMarker, labelStyleMarkerSize,
//!   labelStyleMinimumLength, labelStyleInterval, labelStyleTickInterval, labelStyleTickLength
//!   Auto-Label Rules: labelRuleTable (Rule, Label style, Layer, Code, Type, Labels go on,
//!   Enabled - a check box that switches the rule - and Labels, from the last run or preview);
//!   the form labelRuleName, labelRuleStyle, labelRuleLayer, labelRuleCode, labelRuleType,
//!   labelRuleLabelLayer, labelRuleEnabled; labelRuleAdd, labelRuleUpdate, labelRuleDelete,
//!   labelRulePreview, labelRuleRun, labelRuleClear and labelRuleReport. Run, Preview and Clear
//!   act on the rules selected in the table, every enabled rule when none is.
//! labelStyleProblem   the last refusal, under both tabs
//! ```
//!
//! What is the C++'s and not here. The Insert Value menu is a table of `Action` rows
//! (`LabelStyleManagerDialog::insert_menu`), drawn under the labelTemplateInsert button and looked
//! up by name without a frame, as every menu of the window is. Its values go in at the field's
//! cursor as the C++'s did - after the last value put in, else at the end of the template, and over
//! the NAME a {prop.NAME} leaves selected; where a person clicks the caret into the middle of the
//! field, egui keeps that caret in its own state and the menu puts the value at the end. A text
//! style's sample is shown in the window's own face, bold and italic as the style is.

use std::collections::{BTreeMap, BTreeSet};

use egui::{Context, RichText, Ui};
use katana_cad::{CommandInterpreter, Document, DocumentChange};
use katana_core::error::make_error;
use katana_core::{Error, ErrorCode, Result};
use katana_entity::entity::to_string;
use katana_entity::label_text::{check_label_template, label_value_names};
use katana_entity::{
    Color, LabelKind, LabelMarker, LabelOrientation, LabelPlacement, LabelStyle, TextStyle,
};
use katana_math::numerics::{DEG_TO_RAD, RAD_TO_DEG};

use super::dialog_support::{Followed, LineCursor, TableShown, show_dialog, spin_value, table};
use super::form_widgets::{button, check, combo, describe, edit, problem_line};
use crate::command_word::{annotation_text_word, command_word, exact_number};
use crate::drive::action::{Action, Menu, MenuItem, action_item};
use crate::drive::controls::{self, Choice, LineOptions, SpinRange};
use crate::drive::registry::{ControlKind, ControlRecord};
use crate::host::Host;
use crate::shell::DialogFrame;

/// The rule table's columns.
const RULE_NAME: usize = 0;
const RULE_ENABLED: usize = 6;
const RULE_LABELS: usize = 7;
const RULE_HEADERS: [&str; 8] = [
    "Rule",
    "Label style",
    "Layer",
    "Code",
    "Type",
    "Labels go on",
    "Enabled",
    "Labels",
];
// The Labels column is the last of the headers, its cell the last of each row (refresh).
const _: () = assert!(RULE_LABELS + 1 == RULE_HEADERS.len());

/// The steps of the template language (entity's label_text, and HELP's list): the Insert Value
/// menu offers each of them under every value it applies to, which check_label_template decides -
/// so the menu cannot offer a step the template would refuse. One decimal of a second for dms.N
/// and three places for .Nf and +.Nf stand for the rest.
const STEPS: [&str; 23] = [
    "m", "mm", "km", "ft", "m2", "ha", "km2", "ac", "deg", "rad", "gon", "dms", "dms.1", "dm",
    "qb", "ch", ".0f", ".1f", ".2f", ".3f", "+.3f", "upper", "lower",
];

/// The values every kind has, before the kind's own (LABELSTYLE VALUES).
const COMMON_VALUES: [&str; 5] = ["id", "layer", "code", "point", "description"];

/// The entity types a rule may name (entity's EntityType) and an alignment, for a chainage style.
const RULE_TYPES: [&str; 9] = [
    "Point",
    "Line",
    "Arc",
    "Polyline",
    "Circle",
    "Text",
    "Dimension",
    "Leader",
    "Alignment",
];
const ANY_TYPE: &str = "Any";

const KINDS: [LabelKind; 5] = [
    LabelKind::Point,
    LabelKind::Segment,
    LabelKind::Arc,
    LabelKind::Area,
    LabelKind::Chainage,
];
const PLACEMENTS: [LabelPlacement; 7] = [
    LabelPlacement::Auto,
    LabelPlacement::Above,
    LabelPlacement::Below,
    LabelPlacement::Along,
    LabelPlacement::Centroid,
    LabelPlacement::Right,
    LabelPlacement::Left,
];
const ORIENTATIONS: [LabelOrientation; 2] =
    [LabelOrientation::Aligned, LabelOrientation::Horizontal];
const MARKERS: [LabelMarker; 4] = [
    LabelMarker::None,
    LabelMarker::Cross,
    LabelMarker::Dot,
    LabelMarker::Circle,
];

// The text style form's boxes.
const TEXT_PAPER: SpinRange = SpinRange::new(0.0, 100.0, 0.5, 2).with_suffix(" mm");
const TEXT_WIDTH: SpinRange = SpinRange::new(0.05, 100.0, 0.05, 2);
const TEXT_OBLIQUE: SpinRange = SpinRange::new(-84.9, 84.9, 1.0, 1).with_suffix(" deg");
const TEXT_MARGIN: SpinRange = SpinRange::new(0.0, 20.0, 0.25, 2).with_suffix(" mm");
const TEXT_SPACING: SpinRange = SpinRange::new(0.25, 4.0, 0.1, 2);

// The label style form's boxes.
const LABEL_PAPER: SpinRange = SpinRange::new(0.0, 100.0, 0.5, 2).with_suffix(" mm");
const LABEL_OFFSET: SpinRange = SpinRange::new(0.0, 100.0, 0.25, 2).with_suffix(" mm");
const LABEL_MARKER_SIZE: SpinRange = SpinRange::new(0.0, 50.0, 0.25, 2).with_suffix(" mm");
const LABEL_MINIMUM_LENGTH: SpinRange = SpinRange::new(0.0, 500.0, 1.0, 1).with_suffix(" mm");
const LABEL_INTERVAL: SpinRange = SpinRange::new(0.001, 1.0e6, 10.0, 3);
const LABEL_TICK: SpinRange = SpinRange::new(0.0, 1.0e6, 5.0, 3);
const LABEL_TICK_LENGTH: SpinRange = SpinRange::new(0.0, 50.0, 0.25, 2).with_suffix(" mm");
/// QSpinBox's range for a label's priority.
const PRIORITY_LOWEST: i64 = -1000;
const PRIORITY_HIGHEST: i64 = 1000;

/// "\n" in a line edit is a line break in the stored template, as on the command line.
fn shown_template(text: &str) -> String {
    text.replace('\n', "\\n")
}

fn stored_template(text: &str) -> String {
    text.replace("\\n", "\n")
}

/// A unique name: "base", else "base 2", "base 3"...
fn fresh_name_in(contains: impl Fn(&str) -> bool, base: &str) -> String {
    let mut name = String::from(base);
    let mut n = 2;
    while contains(&name) {
        name = format!("{base} {n}");
        n += 1;
    }
    name
}

fn on_off(on: bool) -> &'static str {
    if on { "on" } else { "off" }
}

/// `key=after` written exactly, when `after` is not `before`: only what the form changed.
fn push_number(options: &mut Vec<String>, key: &str, before: f64, after: f64) {
    if after != before {
        options.push(format!("{key}={}", exact_number(after)));
    }
}

/// `key=after`, when `after` is not `before`.
fn push_named(options: &mut Vec<String>, key: &str, before: &str, after: &str) {
    if after != before {
        options.push(format!("{key}={after}"));
    }
}

/// The rows of a list of names, one cell each.
fn name_rows(names: &[String]) -> Vec<Vec<String>> {
    names.iter().map(|name| vec![name.clone()]).collect()
}

/// Runs `line` through `host`; the refusal, or why the line could not be built, goes to
/// `problem`. The reply comes back through `reply` when asked for.
fn run_line(
    command_line: bool,
    host: &mut dyn Host,
    problem: &mut String,
    line: Result<String>,
    reply: Option<&mut String>,
) -> bool {
    let line = match line {
        Ok(line) => line,
        Err(error) => {
            *problem = describe(&Err(error));
            return false;
        }
    };
    if !command_line {
        *problem = String::from("there is no command line to run the line on");
        return false;
    }
    let outcome = host.run(&line);
    *problem = if outcome.ok {
        String::new()
    } else if outcome.error.is_empty() {
        format!("refused: {line}")
    } else {
        outcome.error.clone()
    };
    if let Some(reply) = reply {
        *reply = outcome.reply;
    }
    outcome.ok
}

// ---- text styles ------------------------------------------------------------------------------

/// What a click in the text style manager asked for, done after the frame drew it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TextPressed {
    New,
    Delete,
    Apply,
}

/// Format > Text Styles... (see the module comment).
pub struct TextStyleManagerDialog {
    frame: DialogFrame,
    /// False for a dialog given no command line (the C++'s empty runner).
    command_line: bool,
    followed: Followed,
    names: Vec<String>,
    current: Option<usize>,
    new_name: String,
    new_name_placeholder: String,
    font: String,
    paper: f64,
    width: f64,
    oblique: f64,
    bold: bool,
    italic: bool,
    colour: String,
    mask: bool,
    margin: f64,
    readable: bool,
    spacing: f64,
    problem: String,
    shown: String,
    pressed: Option<TextPressed>,
}

impl TextStyleManagerDialog {
    /// The manager over `document`, showing Standard; its lines run through the host it is drawn
    /// with.
    pub fn new(document: &Document) -> Self {
        let mut dialog = Self {
            frame: DialogFrame::new("textStyleManagerDialog", "Text Styles"),
            command_line: true,
            followed: Followed::new(document),
            names: Vec::new(),
            current: None,
            new_name: String::new(),
            new_name_placeholder: String::new(),
            font: String::new(),
            paper: 0.0,
            width: 1.0,
            oblique: 0.0,
            bold: false,
            italic: false,
            colour: String::new(),
            mask: false,
            margin: 0.0,
            readable: false,
            spacing: 1.0,
            problem: String::new(),
            shown: String::new(),
            pressed: None,
        };
        dialog.refresh(document);
        dialog
    }

    /// The same manager given no command line to run on.
    pub fn without_command_line(mut self) -> Self {
        self.command_line = false;
        self
    }

    /// What the drawing did since the dialog last looked, seen now (dialog_support).
    fn look(&mut self, document: &Document) {
        if self
            .followed
            .take()
            .has(DocumentChange::ANNOTATION_STYLES | DocumentChange::REPLACED)
        {
            self.refresh(document);
        }
    }

    fn current_name(&self) -> String {
        self.current
            .and_then(|at| self.names.get(at))
            .cloned()
            .unwrap_or_default()
    }

    fn fresh_name(document: &Document) -> String {
        let styles = &document.model().text_styles;
        fresh_name_in(|name| styles.contains(name), "Text Style")
    }

    fn refresh(&mut self, document: &Document) {
        let keep = if self.shown.is_empty() {
            String::from(katana_entity::annotation::DEFAULT_TEXT_STYLE_NAME)
        } else {
            self.shown.clone()
        };
        self.names.clear();
        document.model().text_styles.for_each(|style| {
            self.names.push(style.name.clone());
        });
        self.current = None;
        self.new_name_placeholder = Self::fresh_name(document);
        self.select_style(document, &keep);
    }

    /// Selects a style by name and shows it in the form.
    pub fn select(&mut self, document: &Document, name: &str) {
        self.look(document);
        self.select_style(document, name);
    }

    fn select_style(&mut self, document: &Document, name: &str) {
        let target = if self.names.iter().any(|listed| listed == name) {
            String::from(name)
        } else {
            String::from(katana_entity::annotation::DEFAULT_TEXT_STYLE_NAME)
        };
        if let Some(at) = self.names.iter().position(|listed| *listed == target) {
            self.current = Some(at);
        }
        if let Some(style) = document.model().text_styles.find(&target) {
            let style = style.clone();
            self.show_style(&style);
        }
    }

    fn show_style(&mut self, style: &TextStyle) {
        self.shown.clone_from(&style.name);
        self.font.clone_from(&style.font_family);
        self.paper = spin_value(&TEXT_PAPER, style.paper_height);
        self.width = spin_value(&TEXT_WIDTH, style.width_factor);
        self.oblique = spin_value(&TEXT_OBLIQUE, style.oblique * RAD_TO_DEG);
        self.bold = style.bold;
        self.italic = style.italic;
        self.colour = style.color.map(Color::to_hex).unwrap_or_default();
        self.mask = style.mask;
        self.margin = spin_value(&TEXT_MARGIN, style.mask_margin);
        self.readable = style.readable;
        self.spacing = spin_value(&TEXT_SPACING, style.line_spacing);
    }

    /// The form's style, as Apply would store it.
    pub fn form_style(&self, document: &Document) -> TextStyle {
        let mut style = document
            .model()
            .text_styles
            .find(&self.shown)
            .cloned()
            .unwrap_or_default();
        style.name.clone_from(&self.shown);
        style.font_family = String::from(self.font.trim());
        style.paper_height = self.paper;
        style.width_factor = self.width;
        style.oblique = self.oblique * DEG_TO_RAD;
        style.bold = self.bold;
        style.italic = self.italic;
        let colour = self.colour.trim();
        style.color = None;
        if !colour.is_empty()
            && !colour.eq_ignore_ascii_case("bylayer")
            && let Ok(parsed) = Color::from_hex(colour)
        {
            style.color = Some(parsed);
        }
        style.mask = self.mask;
        style.mask_margin = self.margin;
        style.readable = self.readable;
        style.line_spacing = self.spacing;
        style
    }

    /// The line Apply runs: empty when the form changes nothing.
    pub fn apply_line(&self, document: &Document) -> Result<String> {
        let Some(stored) = document.model().text_styles.find(&self.shown) else {
            return Err(Error::new(ErrorCode::NotFound, "choose a text style first"));
        };
        let colour = self.colour.trim();
        if !colour.is_empty()
            && !colour.eq_ignore_ascii_case("bylayer")
            && Color::from_hex(colour).is_err()
        {
            return Err(make_error(
                ErrorCode::ParseFailure,
                "colour must be ByLayer or #RRGGBB",
                colour,
            ));
        }
        let form = self.form_style(document);
        // TEXTSTYLE SET's keys (annotation_verbs, the text style keys), only for what the form
        // changed.
        let mut options = Vec::new();
        if form.font_family != stored.font_family {
            options.push(format!("font={}", command_word(&form.font_family, "Font")?));
        }
        if form.paper_height != stored.paper_height {
            options.push(format!("paper={}", exact_number(form.paper_height)));
        }
        if form.width_factor != stored.width_factor {
            options.push(format!("width={}", exact_number(form.width_factor)));
        }
        if form.oblique != stored.oblique {
            // In degrees, as the box shows it and the verb takes it.
            options.push(format!("oblique={}", exact_number(self.oblique)));
        }
        if form.bold != stored.bold {
            options.push(format!("bold={}", on_off(form.bold)));
        }
        if form.italic != stored.italic {
            options.push(format!("italic={}", on_off(form.italic)));
        }
        if form.color != stored.color {
            options.push(format!(
                "colour={}",
                form.color
                    .map_or_else(|| String::from("bylayer"), Color::to_hex)
            ));
        }
        if form.mask != stored.mask {
            options.push(format!("mask={}", on_off(form.mask)));
        }
        if form.mask_margin != stored.mask_margin {
            options.push(format!("margin={}", exact_number(form.mask_margin)));
        }
        if form.readable != stored.readable {
            options.push(format!("readable={}", on_off(form.readable)));
        }
        if form.line_spacing != stored.line_spacing {
            options.push(format!("spacing={}", exact_number(form.line_spacing)));
        }
        if options.is_empty() {
            return Ok(String::new());
        }
        let name = command_word(&stored.name, "Style")?;
        Ok(format!("TEXTSTYLE SET {name} {}", options.join(" ")))
    }

    /// What the last refused action said; empty after one that worked.
    pub fn problem(&self) -> &str {
        &self.problem
    }

    fn run(&mut self, host: &mut dyn Host, line: Result<String>) -> bool {
        let done = run_line(self.command_line, host, &mut self.problem, line, None);
        self.look(host.document());
        done
    }

    /// Apply: the line of what the form changed, run; nothing, and true, when it changed nothing.
    pub fn apply(&mut self, host: &mut dyn Host) -> bool {
        self.look(host.document());
        let line = self.apply_line(host.document());
        if let Ok(line) = &line
            && line.is_empty()
        {
            self.problem.clear();
            return true; // unchanged: no step
        }
        self.run(host, line)
    }

    /// New: a text style named from textStyleNewName, or the fresh name its placeholder shows.
    pub fn add_style(&mut self, host: &mut dyn Host) -> bool {
        self.look(host.document());
        let typed = self.new_name.trim();
        let name = if typed.is_empty() {
            Self::fresh_name(host.document())
        } else {
            String::from(typed)
        };
        let written = match command_word(&name, "Name") {
            Ok(written) => written,
            Err(error) => return self.run(host, Err(error)),
        };
        if !self.run(host, Ok(format!("TEXTSTYLE NEW {written}"))) {
            return false;
        }
        self.new_name.clear();
        self.select_style(host.document(), &name);
        true
    }

    /// Delete: the style chosen in the list.
    pub fn delete_style(&mut self, host: &mut dyn Host) -> bool {
        self.look(host.document());
        let written = match command_word(&self.current_name(), "Style") {
            Ok(written) => written,
            Err(error) => return self.run(host, Err(error)),
        };
        if !self.run(host, Ok(format!("TEXTSTYLE DELETE {written}"))) {
            return false;
        }
        self.shown.clear();
        self.refresh(host.document());
        true
    }

    /// Draws the dialog for this frame; false once its title bar's close button is clicked.
    pub fn ui(&mut self, ctx: &Context, host: &mut dyn Host) -> bool {
        self.look(host.document());
        let mut frame = std::mem::replace(
            &mut self.frame,
            DialogFrame::new("textStyleManagerDialog", "Text Styles"),
        );
        let closed = show_dialog(&mut frame, ctx, self, host, Self::body, Self::footer);
        self.frame = frame;
        match self.pressed.take() {
            Some(TextPressed::New) => {
                self.add_style(host);
            }
            Some(TextPressed::Delete) => {
                self.delete_style(host);
            }
            Some(TextPressed::Apply) => {
                self.apply(host);
            }
            None => {}
        }
        !closed
    }

    fn body(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                let rows = name_rows(&self.names);
                if controls::rows(ui, "textStyleList", &rows, &mut self.current).changed() {
                    let name = self.current_name();
                    if let Some(style) = host.document().model().text_styles.find(&name) {
                        let style = style.clone();
                        self.show_style(&style);
                    }
                }
                ui.horizontal(|ui| {
                    edit(
                        ui,
                        "textStyleNewName",
                        &mut self.new_name,
                        &self.new_name_placeholder,
                    )
                    .on_hover_text("The name New gives; blank for the one shown");
                    if button(ui, "textStyleNew", "New")
                        .on_hover_text("A new text style of the name beside it (TEXTSTYLE NEW)")
                        .clicked()
                    {
                        self.pressed = Some(TextPressed::New);
                    }
                });
            });
            ui.vertical(|ui| {
                egui::Grid::new("textStyleForm")
                    .num_columns(2)
                    .show(ui, |ui| {
                        ui.label("Font");
                        edit(ui, "textStyleFont", &mut self.font, "the plain face");
                        ui.end_row();
                        let boxes = [
                            (
                                "Height on paper (0: the text's own)",
                                "textStylePaperHeight",
                                &mut self.paper,
                                TEXT_PAPER,
                            ),
                            (
                                "Width factor",
                                "textStyleWidthFactor",
                                &mut self.width,
                                TEXT_WIDTH,
                            ),
                            (
                                "Oblique",
                                "textStyleOblique",
                                &mut self.oblique,
                                TEXT_OBLIQUE,
                            ),
                        ];
                        for (title, name, value, range) in boxes {
                            ui.label(title);
                            controls::spin(ui, name, value, range);
                            ui.end_row();
                        }
                        check(ui, "textStyleBold", "Bold", &mut self.bold);
                        check(ui, "textStyleItalic", "Italic", &mut self.italic);
                        ui.end_row();
                        ui.label("Colour");
                        edit(
                            ui,
                            "textStyleColour",
                            &mut self.colour,
                            "ByLayer, or #RRGGBB",
                        );
                        ui.end_row();
                        check(ui, "textStyleMask", "Background mask", &mut self.mask);
                        controls::spin(ui, "textStyleMaskMargin", &mut self.margin, TEXT_MARGIN);
                        ui.end_row();
                        check(
                            ui,
                            "textStyleReadable",
                            "Keep readable (turn upside-down text)",
                            &mut self.readable,
                        );
                        ui.end_row();
                        ui.label("Line spacing");
                        controls::spin(ui, "textStyleLineSpacing", &mut self.spacing, TEXT_SPACING);
                        ui.end_row();
                    });
                let mut sample = RichText::new("AaBb 123.456");
                if self.bold {
                    sample = sample.strong();
                }
                if self.italic {
                    sample = sample.italics();
                }
                controls::label_as(
                    ui,
                    "textStyleSample",
                    "AaBb 123.456",
                    egui::Label::new(sample),
                );
            });
        });
    }

    fn footer(&mut self, ui: &mut Ui, _host: &mut dyn Host) {
        problem_line(ui, "textStyleProblem", &self.problem);
        ui.horizontal(|ui| {
            if button(ui, "textStyleDelete", "Delete").clicked() {
                self.pressed = Some(TextPressed::Delete);
            }
            if button(ui, "textStyleApply", "Apply").clicked() {
                self.pressed = Some(TextPressed::Apply);
            }
        });
    }
}

// ---- label styles and rules -------------------------------------------------------------------

/// What a click in the label style manager asked for, done after the frame drew it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LabelPressed {
    New,
    Delete,
    Defaults,
    Apply,
    RuleAdd,
    RuleUpdate,
    RuleDelete,
    Preview,
    Run,
    Clear,
}

/// Format > Label Styles and Rules... (see the module comment).
pub struct LabelStyleManagerDialog {
    frame: DialogFrame,
    /// False for a dialog given no command line (the C++'s empty runner).
    command_line: bool,
    followed: Followed,
    tab: usize,
    names: Vec<String>,
    current: Option<usize>,
    new_name: String,
    new_name_placeholder: String,
    new_kind: LabelKind,
    kind: LabelKind,
    template: String,
    template_cursor: LineCursor,
    insert_menu: Menu,
    text_styles: Vec<String>,
    text_style: Choice,
    paper: f64,
    placement: LabelPlacement,
    orientation: LabelOrientation,
    offset: f64,
    leader: bool,
    displace: bool,
    priority: i64,
    marker: LabelMarker,
    marker_size: f64,
    minimum_length: f64,
    interval: f64,
    tick: f64,
    tick_length: f64,
    template_check: String,
    problem: String,
    shown: String,

    rule_rows: Vec<Vec<String>>,
    rule_enabled_states: Vec<bool>,
    rule_current: Option<usize>,
    /// The rules chosen in the table, by name.
    chosen: BTreeSet<String>,
    rule_name: String,
    rule_styles: Vec<String>,
    rule_style: Choice,
    rule_layer: String,
    rule_code: String,
    rule_types: Vec<String>,
    rule_type: Choice,
    rule_label_layer: String,
    rule_enabled: bool,
    run_report: String,
    /// Labels per rule from the last run or preview, for the Labels column.
    label_counts: BTreeMap<String, String>,
    /// The table's Enabled box clicked: the rule and what it asks for, run as its own line at the
    /// next look - after the click's frame, as the C++ queued it past the box's own signal.
    queued_enabled: Option<(String, bool)>,
    pressed: Option<LabelPressed>,
}

impl LabelStyleManagerDialog {
    /// The manager over `document`; its lines run through the host it is drawn with.
    pub fn new(document: &Document) -> Self {
        let rule_types: Vec<String> = std::iter::once(ANY_TYPE)
            .chain(RULE_TYPES)
            .map(String::from)
            .collect();
        let mut rule_type = Choice::editable("");
        rule_type.choose(0, &rule_types);
        let mut dialog = Self {
            frame: DialogFrame::new("labelStyleManagerDialog", "Label Styles and Rules"),
            command_line: true,
            followed: Followed::new(document),
            tab: 0,
            names: Vec::new(),
            current: None,
            new_name: String::new(),
            new_name_placeholder: String::new(),
            new_kind: LabelKind::Point,
            kind: LabelKind::Point,
            template: String::new(),
            template_cursor: LineCursor::default(),
            insert_menu: Menu::new("labelTemplateInsertMenu", "Insert Value"),
            text_styles: Vec::new(),
            text_style: Choice::default(),
            paper: 0.0,
            placement: LabelPlacement::Auto,
            orientation: LabelOrientation::Aligned,
            offset: 0.0,
            leader: false,
            displace: false,
            priority: 0,
            marker: LabelMarker::None,
            marker_size: 0.0,
            minimum_length: 0.0,
            interval: LABEL_INTERVAL.minimum,
            tick: 0.0,
            tick_length: 0.0,
            template_check: String::new(),
            problem: String::new(),
            shown: String::new(),
            rule_rows: Vec::new(),
            rule_enabled_states: Vec::new(),
            rule_current: None,
            chosen: BTreeSet::new(),
            rule_name: String::new(),
            rule_styles: Vec::new(),
            rule_style: Choice::default(),
            rule_layer: String::new(),
            rule_code: String::new(),
            rule_types,
            rule_type,
            rule_label_layer: String::new(),
            rule_enabled: true,
            run_report: String::new(),
            label_counts: BTreeMap::new(),
            queued_enabled: None,
            pressed: None,
        };
        dialog.refresh(document);
        dialog
    }

    /// The same manager given no command line to run on.
    pub fn without_command_line(mut self) -> Self {
        self.command_line = false;
        self
    }

    /// What the drawing did since the dialog last looked, seen now (dialog_support).
    fn look(&mut self, document: &Document) {
        if self
            .followed
            .take()
            .has(DocumentChange::ANNOTATION_STYLES | DocumentChange::REPLACED)
        {
            self.refresh(document);
        }
    }

    fn current_name(&self) -> String {
        self.current
            .and_then(|at| self.names.get(at))
            .cloned()
            .unwrap_or_default()
    }

    fn fresh_name(document: &Document) -> String {
        let styles = &document.model().label_styles;
        fresh_name_in(|name| styles.contains(name), "Label Style")
    }

    fn refresh(&mut self, document: &Document) {
        let model = document.model();
        let keep = self.shown.clone();
        let rule_style = self.rule_style.current_text(&self.rule_styles);
        self.names = model.label_styles.names();
        self.current = None;
        self.rule_styles = self.names.clone();
        // A combo refilled has its first item chosen, and keeps the one it had where it can.
        self.rule_style = match self.rule_styles.iter().position(|name| *name == rule_style) {
            Some(at) => Choice::at(at),
            None if !self.rule_styles.is_empty() => Choice::at(0),
            None => Choice::default(),
        };
        self.new_name_placeholder = Self::fresh_name(document);
        self.text_styles.clear();
        model.text_styles.for_each(|style| {
            self.text_styles.push(style.name.clone());
        });
        self.text_style = if self.text_styles.is_empty() {
            Choice::default()
        } else {
            Choice::at(0)
        };
        // The chosen rules stay chosen through a rebuild, so a Run after an Enabled click runs
        // what was chosen.
        let chosen: BTreeSet<String> = self.chosen_rules().into_iter().collect();
        self.rule_rows.clear();
        self.rule_enabled_states.clear();
        self.rule_current = None;
        model.label_rules.for_each(|rule| {
            self.rule_rows.push(vec![
                rule.name.clone(),
                rule.label_style.clone(),
                rule.layer.clone(),
                rule.code.clone(),
                if rule.entity_type.is_empty() {
                    String::from(ANY_TYPE)
                } else {
                    rule.entity_type.clone()
                },
                rule.label_layer.clone(),
                String::from(if rule.enabled { "yes" } else { "no" }),
                self.label_counts
                    .get(&rule.name)
                    .cloned()
                    .unwrap_or_default(),
            ]);
            self.rule_enabled_states.push(rule.enabled);
        });
        self.chosen = self
            .rule_rows
            .iter()
            .map(|row| row[RULE_NAME].clone())
            .filter(|name| chosen.contains(name))
            .collect();
        if !keep.is_empty() {
            self.select_style(document, &keep);
        } else if let Some(first) = self.names.first().cloned() {
            self.select_style(document, &first);
        }
    }

    /// Selects a style by name and shows it in the form.
    pub fn select(&mut self, document: &Document, name: &str) {
        self.look(document);
        self.select_style(document, name);
    }

    fn select_style(&mut self, document: &Document, name: &str) {
        let Some(at) = self.names.iter().position(|listed| listed == name) else {
            self.shown.clear();
            return;
        };
        self.current = Some(at);
        if let Some(style) = document.model().label_styles.find(name) {
            let style = style.clone();
            self.show_style(&style);
        }
    }

    fn show_style(&mut self, style: &LabelStyle) {
        self.shown.clone_from(&style.name);
        self.kind = style.kind;
        self.template = shown_template(&style.text);
        self.template_cursor.reset();
        let text_style = if style.text_style.is_empty() {
            katana_entity::annotation::DEFAULT_TEXT_STYLE_NAME
        } else {
            style.text_style.as_str()
        };
        self.text_style = Choice {
            index: self.text_styles.iter().position(|name| name == text_style),
            ..Choice::default()
        };
        self.paper = spin_value(&LABEL_PAPER, style.paper_height);
        self.placement = style.placement;
        self.orientation = style.orientation;
        self.offset = spin_value(&LABEL_OFFSET, style.offset);
        self.leader = style.leader;
        self.displace = style.displace;
        self.priority = i64::from(style.priority).clamp(PRIORITY_LOWEST, PRIORITY_HIGHEST);
        self.marker = style.marker;
        self.marker_size = spin_value(&LABEL_MARKER_SIZE, style.marker_size);
        self.minimum_length = spin_value(&LABEL_MINIMUM_LENGTH, style.minimum_length);
        self.interval = spin_value(&LABEL_INTERVAL, style.interval);
        self.tick = spin_value(&LABEL_TICK, style.tick_interval);
        self.tick_length = spin_value(&LABEL_TICK_LENGTH, style.tick_length);
        self.check_template();
        self.fill_insert_menu();
    }

    fn show_rule(&mut self, document: &Document, name: &str) {
        let Some(rule) = document.model().label_rules.find(name) else {
            return;
        };
        self.rule_name.clone_from(&rule.name);
        self.rule_style = Choice {
            index: self
                .rule_styles
                .iter()
                .position(|style| *style == rule.label_style),
            ..Choice::default()
        };
        self.rule_layer.clone_from(&rule.layer);
        self.rule_code.clone_from(&rule.code);
        let entity_type = if rule.entity_type.is_empty() {
            String::from(ANY_TYPE)
        } else {
            rule.entity_type.clone()
        };
        let found = self.rule_types.iter().position(|kind| *kind == entity_type);
        match found {
            Some(at) => self.rule_type.choose(at, &self.rule_types),
            None => self.rule_type.text = entity_type,
        }
        self.rule_label_layer.clone_from(&rule.label_layer);
        self.rule_enabled = rule.enabled;
    }

    fn check_template(&mut self) {
        self.template_check =
            match check_label_template(&stored_template(&self.template), self.kind) {
                Ok(()) => String::from("template: valid"),
                Err(error) => describe(&Err(error)),
            };
    }

    /// The Insert Value menu: what the kind may say, each with the steps check_label_template
    /// takes after it - the menu offers only what the template would accept - then any property
    /// of the target, by name.
    fn fill_insert_menu(&mut self) {
        let kind = self.kind;
        let valid = |field: &str| check_label_template(&format!("{{{field}}}"), kind).is_ok();
        let mut menu = Menu::new("labelTemplateInsertMenu", "Insert Value");
        let values: Vec<&str> = COMMON_VALUES
            .iter()
            .copied()
            .chain(label_value_names(kind))
            .collect();
        for value in values {
            if !valid(value) {
                continue;
            }
            let mut sub = Menu::new(value, value).action(Action::new(
                &format!("labelValue:{value}"),
                &format!("{{{value}}}"),
            ));
            for step in STEPS {
                let field = format!("{value}:{step}");
                if !valid(&field) {
                    continue;
                }
                sub = sub.action(Action::new(
                    &format!("labelValue:{field}"),
                    &format!("{{{field}}}"),
                ));
            }
            menu = menu.submenu(sub);
        }
        menu = menu.action(Action::new("labelValue:prop.NAME", "{prop.NAME}"));
        self.insert_menu = menu;
    }

    /// The Insert Value menu's rows, each action named "labelValue:" and the field it puts in.
    pub fn insert_menu(&self) -> &Menu {
        &self.insert_menu
    }

    /// The Insert Value action `name` triggered, as a click on it does; false for no such action.
    pub fn trigger_insert(&mut self, name: &str) -> bool {
        let found = self
            .insert_menu
            .actions()
            .iter()
            .any(|action| action.name == name);
        match name.strip_prefix("labelValue:") {
            Some(field) if found => {
                let field = String::from(field);
                self.insert_value(&field);
                true
            }
            _ => false,
        }
    }

    /// Puts "{field}" - "value" or "value:step" - at the template's cursor, as the Insert Value
    /// menu does; "prop.NAME" leaves NAME selected to type over.
    pub fn insert_value(&mut self, field: &str) {
        self.template_cursor
            .insert(&mut self.template, &format!("{{{field}}}"));
        if field == "prop.NAME" {
            // "NAME" is the property's to type: leave it selected.
            if let Some(cursor) = self.template_cursor.cursor {
                let end = cursor - 1;
                self.template_cursor.selection = Some((end - 4, end));
            }
        }
        self.check_template();
    }

    /// The template field's selected text (QLineEdit::selectedText); empty for none.
    pub fn template_selected_text(&self) -> String {
        self.template_cursor.selected(&self.template)
    }

    /// `text` typed into the template field where its cursor is, over its selection
    /// (QLineEdit::insert).
    pub fn type_in_template(&mut self, text: &str) {
        self.template_cursor.insert(&mut self.template, text);
        self.check_template();
    }

    /// The form's style, as Apply would store it.
    pub fn form_style(&self, document: &Document) -> LabelStyle {
        let mut style = document
            .model()
            .label_styles
            .find(&self.shown)
            .cloned()
            .unwrap_or_default();
        style.name.clone_from(&self.shown);
        style.kind = self.kind;
        style.text = stored_template(&self.template);
        let text_style = self.text_style.current_text(&self.text_styles);
        style.text_style = if text_style == katana_entity::annotation::DEFAULT_TEXT_STYLE_NAME {
            String::new()
        } else {
            text_style
        };
        style.paper_height = self.paper;
        style.placement = self.placement;
        style.orientation = self.orientation;
        style.offset = self.offset;
        style.leader = self.leader;
        style.displace = self.displace;
        style.priority = self.priority as i32;
        style.marker = self.marker;
        style.marker_size = self.marker_size;
        style.minimum_length = self.minimum_length;
        style.interval = self.interval;
        style.tick_interval = self.tick;
        style.tick_length = self.tick_length;
        style
    }

    /// The line Apply runs: empty when the form changes nothing.
    pub fn apply_line(&self, document: &Document) -> Result<String> {
        let Some(stored) = document.model().label_styles.find(&self.shown) else {
            return Err(Error::new(
                ErrorCode::NotFound,
                "choose a label style first",
            ));
        };
        let form = self.form_style(document);
        // LABELSTYLE SET's keys (annotation_verbs, the label style keys), only for what the form
        // changed.
        let mut options = Vec::new();
        if form.text != stored.text {
            options.push(format!(
                "text={}",
                annotation_text_word(&form.text, "Text")?
            ));
        }
        if form.text_style != stored.text_style {
            options.push(format!(
                "textstyle={}",
                command_word(&form.text_style, "Text style")?
            ));
        }
        push_number(
            &mut options,
            "paper",
            stored.paper_height,
            form.paper_height,
        );
        push_named(
            &mut options,
            "placement",
            to_string(stored.placement),
            to_string(form.placement),
        );
        push_named(
            &mut options,
            "orientation",
            to_string(stored.orientation),
            to_string(form.orientation),
        );
        push_number(&mut options, "offset", stored.offset, form.offset);
        if form.leader != stored.leader {
            options.push(format!("leader={}", on_off(form.leader)));
        }
        if form.displace != stored.displace {
            options.push(format!("displace={}", on_off(form.displace)));
        }
        if form.priority != stored.priority {
            options.push(format!("priority={}", form.priority));
        }
        push_named(
            &mut options,
            "marker",
            to_string(stored.marker),
            to_string(form.marker),
        );
        push_number(
            &mut options,
            "markersize",
            stored.marker_size,
            form.marker_size,
        );
        push_number(
            &mut options,
            "minlength",
            stored.minimum_length,
            form.minimum_length,
        );
        push_number(&mut options, "interval", stored.interval, form.interval);
        push_number(
            &mut options,
            "tick",
            stored.tick_interval,
            form.tick_interval,
        );
        push_number(
            &mut options,
            "ticklength",
            stored.tick_length,
            form.tick_length,
        );
        if options.is_empty() {
            return Ok(String::new());
        }
        let name = command_word(&stored.name, "Style")?;
        Ok(format!("LABELSTYLE SET {name} {}", options.join(" ")))
    }

    /// What the last refused action said; empty after one that worked.
    pub fn problem(&self) -> &str {
        &self.problem
    }

    /// What the last rule run, preview or clear did, as AUTOLABEL replies.
    pub fn run_report(&self) -> &str {
        &self.run_report
    }

    /// The rules selected in the table, in its order; empty for none.
    pub fn chosen_rules(&self) -> Vec<String> {
        self.rule_rows
            .iter()
            .map(|row| &row[RULE_NAME])
            .filter(|name| self.chosen.contains(*name))
            .cloned()
            .collect()
    }

    /// The rules `names` chosen in the table, and only they, as clicks there choose them (none:
    /// the table's selection cleared). A name the table does not list is passed over.
    pub fn choose_rules(&mut self, names: &[&str]) {
        self.chosen = self
            .rule_rows
            .iter()
            .map(|row| row[RULE_NAME].clone())
            .filter(|name| names.contains(&name.as_str()))
            .collect();
    }

    /// The table's Enabled box of row `row` set to `enabled`, as a click on it does: the rule is
    /// switched by a line of its own, run at the dialog's next frame.
    pub fn tick_rule_enabled(&mut self, row: usize, enabled: bool) {
        if let Some(cells) = self.rule_rows.get(row) {
            self.queued_enabled = Some((cells[RULE_NAME].clone(), enabled));
        }
    }

    fn run(&mut self, host: &mut dyn Host, line: Result<String>) -> bool {
        let done = run_line(self.command_line, host, &mut self.problem, line, None);
        self.look(host.document());
        done
    }

    /// Apply: the line of what the form changed, run; nothing, and true, when it changed nothing.
    pub fn apply(&mut self, host: &mut dyn Host) -> bool {
        self.look(host.document());
        let line = self.apply_line(host.document());
        if let Ok(line) = &line
            && line.is_empty()
        {
            self.problem.clear();
            return true; // unchanged: no step
        }
        self.run(host, line)
    }

    /// New: a label style named from labelStyleNewName (or the fresh name), of the kind
    /// labelStyleNewKind shows.
    pub fn add_style(&mut self, host: &mut dyn Host) -> bool {
        self.look(host.document());
        let typed = self.new_name.trim();
        let name = if typed.is_empty() {
            Self::fresh_name(host.document())
        } else {
            String::from(typed)
        };
        let written = match command_word(&name, "Name") {
            Ok(written) => written,
            Err(error) => return self.run(host, Err(error)),
        };
        // The kind's standard template comes with it, as LABELSTYLE NEW gives.
        let kind = to_string(self.new_kind);
        if !self.run(host, Ok(format!("LABELSTYLE NEW {written} kind={kind}"))) {
            return false;
        }
        self.new_name.clear();
        self.select_style(host.document(), &name);
        true
    }

    /// Delete: the style chosen in the list.
    pub fn delete_style(&mut self, host: &mut dyn Host) -> bool {
        self.look(host.document());
        let written = match command_word(&self.current_name(), "Style") {
            Ok(written) => written,
            Err(error) => return self.run(host, Err(error)),
        };
        if !self.run(host, Ok(format!("LABELSTYLE DELETE {written}"))) {
            return false;
        }
        self.shown.clear();
        self.refresh(host.document());
        true
    }

    /// Add Standard Set: LABELSTYLE DEFAULTS.
    pub fn add_defaults(&mut self, host: &mut dyn Host) -> bool {
        self.look(host.document());
        self.run(host, Ok(String::from("LABELSTYLE DEFAULTS")))
    }

    /// The rule form's options: a filter left blank is "any", sent blank ("") only where it
    /// replaces one the rule has (`every_field`, for RULE SET).
    fn rule_options(&self, every_field: bool) -> Result<String> {
        let mut options = Vec::new();
        let mut rule_type = String::from(self.rule_type.current_text(&self.rule_types).trim());
        if rule_type.eq_ignore_ascii_case(ANY_TYPE) {
            rule_type.clear();
        }
        let fields = [
            ("style", self.rule_style.current_text(&self.rule_styles)),
            ("layer", String::from(self.rule_layer.trim())),
            ("code", String::from(self.rule_code.trim())),
            ("type", rule_type),
            ("labellayer", String::from(self.rule_label_layer.trim())),
        ];
        for (key, value) in fields {
            if value.is_empty() && !every_field {
                continue;
            }
            options.push(format!("{key}={}", command_word(&value, key)?));
        }
        options.push(format!("enabled={}", on_off(self.rule_enabled)));
        Ok(options.join(" "))
    }

    /// Add Rule: a rule made from its form.
    pub fn add_rule(&mut self, host: &mut dyn Host) -> bool {
        self.look(host.document());
        let name = match command_word(self.rule_name.trim(), "Rule") {
            Ok(name) => name,
            Err(error) => return self.run(host, Err(error)),
        };
        let options = match self.rule_options(false) {
            Ok(options) => options,
            Err(error) => return self.run(host, Err(error)),
        };
        if !self.run(host, Ok(format!("AUTOLABEL RULE ADD {name} {options}"))) {
            return false;
        }
        self.rule_name.clear();
        true
    }

    /// Update Rule: the form stored over the rule of its name.
    pub fn update_rule(&mut self, host: &mut dyn Host) -> bool {
        self.look(host.document());
        let name = match command_word(self.rule_name.trim(), "Rule") {
            Ok(name) => name,
            Err(error) => return self.run(host, Err(error)),
        };
        let options = match self.rule_options(true) {
            Ok(options) => options,
            Err(error) => return self.run(host, Err(error)),
        };
        self.run(host, Ok(format!("AUTOLABEL RULE SET {name} {options}")))
    }

    /// Delete Rule: the rule of the table's current row.
    pub fn delete_rule(&mut self, host: &mut dyn Host) -> bool {
        self.look(host.document());
        let Some(row) = self.rule_current.and_then(|at| self.rule_rows.get(at)) else {
            self.problem = String::from("choose the rule to delete in the table");
            return false;
        };
        let name = match command_word(&row[RULE_NAME], "Rule") {
            Ok(name) => name,
            Err(error) => return self.run(host, Err(error)),
        };
        self.run(host, Ok(format!("AUTOLABEL RULE DELETE {name}")))
    }

    /// The rule `name` switched on or off, by a line of its own.
    pub fn set_rule_enabled(&mut self, host: &mut dyn Host, name: &str, enabled: bool) -> bool {
        self.look(host.document());
        let written = match command_word(name, "Rule") {
            Ok(written) => written,
            Err(error) => return self.run(host, Err(error)),
        };
        let done = self.run(
            host,
            Ok(format!(
                "AUTOLABEL RULE SET {written} enabled={}",
                on_off(enabled)
            )),
        );
        if !done {
            self.refresh(host.document()); // the box shows what the rule still is
        }
        done
    }

    /// RUN, PREVIEW or CLEAR of the chosen rules; the reply's counts go to the report and its
    /// "rule=NAME labels=N" lines to the Labels column.
    fn rule_verb(&mut self, host: &mut dyn Host, action: &str) -> bool {
        let mut line = format!("AUTOLABEL {action}");
        for rule in self.chosen_rules() {
            match command_word(&rule, "Rule") {
                Ok(written) => {
                    line.push(' ');
                    line.push_str(&written);
                }
                Err(error) => return self.run(host, Err(error)),
            }
        }
        let mut reply = String::new();
        let done = run_line(
            self.command_line,
            host,
            &mut self.problem,
            Ok(line),
            Some(&mut reply),
        );
        self.look(host.document());
        if !done {
            return false;
        }
        // "autolabel created=1 kept=0 removed=0 skipped=0" (or "preview ...", or CLEAR's
        // "removed=1"), then "rule=NAME labels=N" a rule: the counts for the report, the rules'
        // for the Labels column.
        let mut summary = Vec::new();
        for reply_line in reply.split('\n').filter(|line| !line.is_empty()) {
            if !reply_line.starts_with("rule=") {
                summary.push(reply_line);
                continue;
            }
            let Ok(words) = CommandInterpreter::tokenize(reply_line) else {
                continue;
            };
            let mut rule = String::new();
            let mut labels = String::new();
            for field in words {
                if let Some(name) = field.strip_prefix("rule=") {
                    rule = String::from(name);
                } else if let Some(count) = field.strip_prefix("labels=") {
                    labels = String::from(count);
                }
            }
            if !rule.is_empty() {
                self.label_counts.insert(rule, labels);
            }
        }
        self.run_report = summary.join(" ");
        self.refresh(host.document());
        true
    }

    /// Run Rules: the chosen rules, every enabled one when none is chosen, as one step.
    pub fn run_rules(&mut self, host: &mut dyn Host) -> bool {
        self.look(host.document());
        self.rule_verb(host, "RUN")
    }

    /// Preview: what running the chosen rules would do, changing nothing.
    pub fn preview_rules(&mut self, host: &mut dyn Host) -> bool {
        self.look(host.document());
        self.rule_verb(host, "PREVIEW")
    }

    /// Remove Rule Labels: the labels the chosen rules made - every rule's when none is chosen.
    pub fn clear_rules(&mut self, host: &mut dyn Host) -> bool {
        self.look(host.document());
        // What CLEAR removes it removes by rule; the Labels column no longer says how many those
        // rules have.
        let chosen = self.chosen_rules();
        let cleared: Vec<String> = if chosen.is_empty() {
            self.label_counts.keys().cloned().collect()
        } else {
            chosen
        };
        for rule in cleared {
            self.label_counts.remove(&rule);
        }
        self.rule_verb(host, "CLEAR")
    }

    /// Draws the dialog for this frame; false once its title bar's close button is clicked.
    pub fn ui(&mut self, ctx: &Context, host: &mut dyn Host) -> bool {
        self.look(host.document());
        if let Some((name, enabled)) = self.queued_enabled.take() {
            self.set_rule_enabled(host, &name, enabled);
        }
        let mut frame = std::mem::replace(
            &mut self.frame,
            DialogFrame::new("labelStyleManagerDialog", "Label Styles and Rules"),
        );
        let closed = show_dialog(&mut frame, ctx, self, host, Self::body, Self::footer);
        self.frame = frame;
        match self.pressed.take() {
            Some(LabelPressed::New) => {
                self.add_style(host);
            }
            Some(LabelPressed::Delete) => {
                self.delete_style(host);
            }
            Some(LabelPressed::Defaults) => {
                self.add_defaults(host);
            }
            Some(LabelPressed::Apply) => {
                self.apply(host);
            }
            Some(LabelPressed::RuleAdd) => {
                self.add_rule(host);
            }
            Some(LabelPressed::RuleUpdate) => {
                self.update_rule(host);
            }
            Some(LabelPressed::RuleDelete) => {
                self.delete_rule(host);
            }
            Some(LabelPressed::Preview) => {
                self.preview_rules(host);
            }
            Some(LabelPressed::Run) => {
                self.run_rules(host);
            }
            Some(LabelPressed::Clear) => {
                self.clear_rules(host);
            }
            None => {}
        }
        !closed
    }

    fn body(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        controls::tabs(
            ui,
            "labelStyleTabs",
            &["Label Styles", "Auto-Label Rules"],
            &mut self.tab,
        );
        ui.separator();
        if self.tab == 0 {
            self.styles_tab(ui, host);
        } else {
            self.rules_tab(ui, host);
        }
    }

    fn styles_tab(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                let rows = name_rows(&self.names);
                if controls::rows(ui, "labelStyleList", &rows, &mut self.current).changed() {
                    let name = self.current_name();
                    if let Some(style) = host.document().model().label_styles.find(&name) {
                        let style = style.clone();
                        self.show_style(&style);
                    }
                }
                edit(
                    ui,
                    "labelStyleNewName",
                    &mut self.new_name,
                    &self.new_name_placeholder,
                )
                .on_hover_text("The name New gives; blank for the one shown");
                ui.horizontal(|ui| {
                    combo(ui, "labelStyleNewKind", &KINDS, &mut self.new_kind)
                        .on_hover_text("What the new style labels; fixed once it is made");
                    if button(ui, "labelStyleNew", "New")
                        .on_hover_text(
                            "A new label style of the name and kind beside it (LABELSTYLE NEW)",
                        )
                        .clicked()
                    {
                        self.pressed = Some(LabelPressed::New);
                    }
                });
            });
            ui.vertical(|ui| {
                self.style_form(ui);
                ui.horizontal(|ui| {
                    if button(ui, "labelStyleDelete", "Delete").clicked() {
                        self.pressed = Some(LabelPressed::Delete);
                    }
                    if button(ui, "labelStyleDefaults", "Add Standard Set").clicked() {
                        self.pressed = Some(LabelPressed::Defaults);
                    }
                    if button(ui, "labelStyleApply", "Apply").clicked() {
                        self.pressed = Some(LabelPressed::Apply);
                    }
                });
            });
        });
    }

    fn style_form(&mut self, ui: &mut Ui) {
        let mut inserted: Option<String> = None;
        egui::Grid::new("labelStyleForm")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label("Kind");
                ui.add_enabled_ui(false, |ui| {
                    // Fixed once made; New asks for it.
                    combo(ui, "labelStyleKind", &KINDS, &mut self.kind);
                });
                ui.end_row();

                ui.label("Template");
                ui.horizontal(|ui| {
                    if controls::line_with(
                        ui,
                        "labelStyleTemplate",
                        &mut self.template,
                        LineOptions {
                            placeholder: "{bearing:dms} {distance:.3f}",
                            ..LineOptions::default()
                        },
                    )
                    .changed()
                    {
                        self.template_cursor.reset();
                        self.check_template();
                    }
                    let menu = &self.insert_menu;
                    let shown = ui.menu_button("Insert Value", |ui| {
                        for item in &menu.items {
                            match item {
                                MenuItem::Menu(sub) => {
                                    ui.menu_button(sub.title.as_str(), |ui| {
                                        for action in sub.actions() {
                                            if action_item(ui, action).clicked() {
                                                inserted = Some(action.name.clone());
                                                ui.close();
                                            }
                                        }
                                    });
                                }
                                MenuItem::Action(action) => {
                                    if action_item(ui, action).clicked() {
                                        inserted = Some(action.name.clone());
                                        ui.close();
                                    }
                                }
                                MenuItem::Section(_) | MenuItem::Separator => {}
                            }
                        }
                    });
                    let mut record = ControlRecord::new("labelTemplateInsert", ControlKind::Button);
                    record.text = String::from("Insert Value");
                    controls::custom(
                        ui,
                        &shown.response.on_hover_text(
                            "The values a template of this kind may use, each with the steps \
                             that apply to it (LABELSTYLE VALUES); the choice goes in at the \
                             cursor",
                        ),
                        record,
                    );
                });
                ui.end_row();

                ui.label("");
                controls::label(ui, "labelStyleTemplateCheck", &self.template_check);
                ui.end_row();

                ui.label("Text style");
                controls::choice(
                    ui,
                    "labelStyleTextStyle",
                    &self.text_styles,
                    &mut self.text_style,
                );
                ui.end_row();

                ui.label("Height on paper (0: the text style's)");
                controls::spin(ui, "labelStylePaperHeight", &mut self.paper, LABEL_PAPER);
                ui.end_row();
                ui.label("Placement");
                combo(ui, "labelStylePlacement", &PLACEMENTS, &mut self.placement);
                ui.end_row();
                ui.label("Orientation");
                combo(
                    ui,
                    "labelStyleOrientation",
                    &ORIENTATIONS,
                    &mut self.orientation,
                );
                ui.end_row();
                ui.label("Offset");
                controls::spin(ui, "labelStyleOffset", &mut self.offset, LABEL_OFFSET);
                ui.end_row();
                check(
                    ui,
                    "labelStyleLeader",
                    "Leader when displaced",
                    &mut self.leader,
                );
                check(
                    ui,
                    "labelStyleDisplace",
                    "May move to make room",
                    &mut self.displace,
                );
                ui.end_row();
                ui.label("Priority");
                controls::whole(
                    ui,
                    "labelStylePriority",
                    &mut self.priority,
                    PRIORITY_LOWEST..=PRIORITY_HIGHEST,
                );
                ui.end_row();
                ui.label("Marker");
                combo(ui, "labelStyleMarker", &MARKERS, &mut self.marker);
                ui.end_row();
                let boxes = [
                    (
                        "Marker size",
                        "labelStyleMarkerSize",
                        &mut self.marker_size,
                        LABEL_MARKER_SIZE,
                    ),
                    (
                        "Shortest segment labelled",
                        "labelStyleMinimumLength",
                        &mut self.minimum_length,
                        LABEL_MINIMUM_LENGTH,
                    ),
                    (
                        "Chainage label interval",
                        "labelStyleInterval",
                        &mut self.interval,
                        LABEL_INTERVAL,
                    ),
                    (
                        "Chainage tick interval",
                        "labelStyleTickInterval",
                        &mut self.tick,
                        LABEL_TICK,
                    ),
                    (
                        "Tick length",
                        "labelStyleTickLength",
                        &mut self.tick_length,
                        LABEL_TICK_LENGTH,
                    ),
                ];
                for (title, name, value, range) in boxes {
                    ui.label(title);
                    controls::spin(ui, name, value, range);
                    ui.end_row();
                }
            });
        if let Some(name) = inserted {
            self.trigger_insert(&name);
        }
    }

    fn rules_tab(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        let highlighted: Vec<bool> = self
            .rule_rows
            .iter()
            .map(|row| self.chosen.contains(&row[RULE_NAME]))
            .collect();
        let shown: TableShown = table(
            ui,
            "labelRuleTable",
            &RULE_HEADERS,
            &self.rule_rows,
            self.rule_current,
            &highlighted,
            Some((RULE_ENABLED, self.rule_enabled_states.as_slice())),
        );
        if let Some(at) = shown.chosen {
            let name = self.rule_rows[at][RULE_NAME].clone();
            if shown.adding {
                if !self.chosen.remove(&name) {
                    self.chosen.insert(name.clone());
                }
            } else {
                self.chosen = BTreeSet::from([name.clone()]);
            }
            // A row chosen shows its rule in the form, to edit and Update.
            if self.rule_current != Some(at) {
                self.rule_current = Some(at);
                self.show_rule(host.document(), &name);
            }
        }
        // The Enabled box switches the rule, as its own line - run at the next frame, once the
        // table is drawn: the rule's change rebuilds the table.
        if let Some((row, on)) = shown.checked {
            self.tick_rule_enabled(row, on);
        }
        ui.horizontal_wrapped(|ui| {
            edit(ui, "labelRuleName", &mut self.rule_name, "name");
            controls::choice(
                ui,
                "labelRuleStyle",
                &self.rule_styles,
                &mut self.rule_style,
            );
            edit(
                ui,
                "labelRuleLayer",
                &mut self.rule_layer,
                "layer glob, e.g. cadastre*",
            );
            edit(
                ui,
                "labelRuleCode",
                &mut self.rule_code,
                "code glob, e.g. BDY*",
            );
            controls::choice(ui, "labelRuleType", &self.rule_types, &mut self.rule_type)
                .on_hover_text(
                    "The kind of entity labelled - a glob, so Poly* is allowed - or Any for \
                     every one the style labels",
                );
            edit(
                ui,
                "labelRuleLabelLayer",
                &mut self.rule_label_layer,
                "the target's layer",
            );
            check(ui, "labelRuleEnabled", "Enabled", &mut self.rule_enabled);
        });
        ui.horizontal(|ui| {
            if button(ui, "labelRuleAdd", "Add Rule").clicked() {
                self.pressed = Some(LabelPressed::RuleAdd);
            }
            if button(ui, "labelRuleUpdate", "Update Rule")
                .on_hover_text("Store the form over the rule of its name (AUTOLABEL RULE SET)")
                .clicked()
            {
                self.pressed = Some(LabelPressed::RuleUpdate);
            }
            if button(ui, "labelRuleDelete", "Delete Rule").clicked() {
                self.pressed = Some(LabelPressed::RuleDelete);
            }
        });
        controls::label(ui, "labelRuleReport", &self.run_report);
        ui.horizontal(|ui| {
            if button(ui, "labelRuleClear", "Remove Rule Labels")
                .on_hover_text(
                    "Remove the labels the chosen rules made - every rule's when none is chosen \
                     (AUTOLABEL CLEAR)",
                )
                .clicked()
            {
                self.pressed = Some(LabelPressed::Clear);
            }
            if button(ui, "labelRulePreview", "Preview")
                .on_hover_text(
                    "What running the chosen rules would do, changing nothing (AUTOLABEL PREVIEW)",
                )
                .clicked()
            {
                self.pressed = Some(LabelPressed::Preview);
            }
            if button(ui, "labelRuleRun", "Run Rules")
                .on_hover_text(
                    "Label what the chosen rules match - every enabled rule when none is chosen - \
                     as one step (AUTOLABEL RUN)",
                )
                .clicked()
            {
                self.pressed = Some(LabelPressed::Run);
            }
        });
    }

    fn footer(&mut self, ui: &mut Ui, _host: &mut dyn Host) {
        problem_line(ui, "labelStyleProblem", &self.problem);
    }
}
