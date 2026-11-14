//! Format > Global Modify...: a "Change" panel, Quick Select and Properties in one non-modal
//! window over the drawing (the owner's request of 2026-09-25; the logic is cad's
//! plan_global_modify, and this dialog only states it). The port of
//! customisation/global_modify_dialog.
//!
//! ```text
//! Apply to   the selection | a view (the views open in the workspace, with "only what is on
//!            screen" for a plan view) | the checked layers (with or without their sublayers) |
//!            the whole drawing
//! Filter     types, layer and style patterns, colour, a property and its value, text, drawn
//!            only - each optional (these two are the shared ScopeFilterWidget, which every
//!            dialog on drawing data shows; its controls keep the names they had here)
//! Modify     three tabs: Entities (layer, colour, style, shown, symbol, text height, set and
//!            remove a property), Layers (colour, linetype, weight, hatch, dimension style,
//!            shown, locked) and Styles (colour, linetype, weight, hatch, symbol, symbol size).
//!            A field is changed only when its box is ticked: an unticked field is left as each
//!            entity, layer or style has it.
//! ```
//!
//! The summary under the form is the plan's own (GlobalModifyPlan::summary), refreshed as the form
//! is edited and as the drawing changes, so what Apply will do - and what it will leave alone and
//! why - is read before it is pressed. Apply is ONE undo step. Select Matches runs the SELECT line
//! the scope and filter say (SELECT DRAWING WHERE TYPE=leader) through the window's executor and
//! shows what it selected in the active plan view. Each type box says how many of its kind the
//! scope and the rest of the filter take ("Leader (774)"), and with nothing selected the dialog
//! opens on the whole drawing, so a kind is found by ticking it.
//!
//! The rules of docs/desktop.md, "The rules a dialog or panel follows": every control has an
//! object name ("globalModify..."); a colour is typed as #RRGGBB or picked; nothing is asked in a
//! modal box; the Document is watched through a DocumentWatcher, the last member, so the dialog
//! may outlive it and then does nothing.
//!
//! What is the C++'s and not here. The preview after an edit waited 150 ms of quiet on a timer;
//! here it is worked out at the end of the frame that saw the edit. Choose... opened Qt's colour
//! dialog; here it shows egui's colour button beside the field. A size box at 0 showed "own size"
//! in place of the number. The tabs' pages and the Modify group had object names of their own;
//! only the controls in them are named here.

use std::rc::Rc;

use egui::{Context, Ui};
use katana_cad::document::Document;
use katana_cad::global_modify::{
    GlobalModify, ModifyFilter, ModifyScope, SymbolModify, match_entities, plan_global_modify,
};
use katana_cad::scope_verbs::count_by_type;
use katana_cad::style_catalogue::{linetype_choices, symbol_choices};
use katana_core::error::make_error;
use katana_core::text::{
    equals_ignoring_case, lowered, parse_finite_double, parse_integer, trimmed,
};
use katana_core::{Error, ErrorCode, Result};
use katana_entity::{Color, PropertyValue};

use super::customisation_context::CustomisationContext;
use super::document_watcher::DocumentWatcher;
use super::scope_filter_widget::{ScopeChoice, ScopeFilterView, ScopeFilterWidget};
use crate::annotation::dialog_support::show_dialog;
use crate::drive::controls::{self, Choice, LineOptions, SpinRange};
use crate::host::Host;
use crate::shell::DialogFrame;
use crate::survey::survey_dialogs::offstage;

const DIALOG: &str = "globalModifyDialog";
const TITLE: &str = "Global Modify";
const BY_LAYER: &str = "ByLayer";
/// The colour an error is said in (#c0392b), as the summary's stylesheet gave it.
const ERROR_COLOUR: egui::Color32 = egui::Color32::from_rgb(0xc0, 0x39, 0x2b);

/// One open view, as the View scope offers it: the shared widget's.
pub type GlobalModifyView = ScopeFilterView;

/// The workspace's open views, asked whenever the view list or a View scope is needed.
pub type GlobalModifyViews = Rc<dyn Fn() -> Vec<GlobalModifyView>>;

/// "#RRGGBB" or, when `by_layer` is allowed, ByLayer (an inner None).
fn read_colour(value: &str, field: &str, by_layer: bool) -> Result<Option<Color>> {
    let typed = value.trim();
    if by_layer && equals_ignoring_case(typed.as_bytes(), BY_LAYER.as_bytes()) {
        return Ok(None);
    }
    match Color::from_hex(typed) {
        Ok(colour) => Ok(Some(colour)),
        Err(_) => Err(make_error(
            ErrorCode::InvalidArgument,
            format!(
                "{field}: type #RRGGBB{}",
                if by_layer { " or ByLayer" } else { "" }
            ),
            typed,
        )),
    }
}

/// An editable choice of names: what it lists, and what is typed or chosen in it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct NameBox {
    items: Vec<String>,
    choice: Choice,
}

impl NameBox {
    fn new() -> Self {
        Self {
            items: Vec::new(),
            choice: Choice::editable(""),
        }
    }

    /// Puts `names` in the box, keeping what was typed or chosen. The first item is the box's
    /// current one afterwards, whatever it reads, as a list refilled chooses its first.
    fn refill(&mut self, names: Vec<String>) {
        self.items = names;
        self.choice.index = if self.items.is_empty() { None } else { Some(0) };
    }

    /// What the box reads, trimmed.
    fn text(&self) -> String {
        String::from(self.choice.text.trim())
    }

    /// Whether the box reads its first item, chosen: "(document default)", "(plain mark)".
    fn reads_first(&self) -> bool {
        self.items
            .first()
            .is_none_or(|first| self.choice.text == *first)
    }

    fn ui(&mut self, ui: &mut Ui, name: &str) -> bool {
        controls::choice(ui, name, &self.items, &mut self.choice).changed()
    }
}

/// A choice of two: "Shown" or "Hidden", "Locked" or "Unlocked". True is the first.
#[derive(Clone, Debug, PartialEq, Eq)]
struct YesNo {
    items: Vec<String>,
    index: usize,
}

impl YesNo {
    fn new(yes: &str, no: &str) -> Self {
        Self {
            items: vec![String::from(yes), String::from(no)],
            index: 0,
        }
    }

    fn yes(&self) -> bool {
        self.index == 0
    }

    fn ui(&mut self, ui: &mut Ui, name: &str) -> bool {
        let mut choice = Choice::at(self.index);
        let changed = controls::choice(ui, name, &self.items, &mut choice).changed();
        if let Some(index) = choice.index {
            self.index = index;
        }
        changed
    }
}

/// The three parts of the form.
struct Request {
    scope: ModifyScope,
    filter: ModifyFilter,
    change: GlobalModify,
}

fn match_count(count: usize, one: &str, many: &str) -> String {
    format!("{count}{}", if count == 1 { one } else { many })
}

/// A size box: from 0 up, to `decimals` places.
fn size_range(maximum: f64, decimals: usize) -> SpinRange {
    SpinRange::new(0.0, maximum, 1.0, decimals)
}

/// See the module comment.
pub struct GlobalModifyDialog {
    context: CustomisationContext,
    frame: DialogFrame,
    /// Apply to, and Only those that match: the shared controls.
    scope_filter: ScopeFilterWidget,
    tab: usize,

    // Modify > Entities
    set_layer: bool,
    layer: NameBox,
    set_colour: bool,
    colour: String,
    set_style: bool,
    style: NameBox,
    set_visible: bool,
    visible: YesNo,
    set_symbol: bool,
    symbol: NameBox,
    symbol_size: f64,
    set_text_height: bool,
    text_height: f64,
    set_property: bool,
    property_key: String,
    property_value: String,
    property_type: usize,
    remove_property: bool,
    remove_key: String,

    // Modify > Layers
    set_layer_colour: bool,
    layer_colour: String,
    set_layer_linetype: bool,
    layer_linetype: NameBox,
    set_layer_weight: bool,
    layer_weight: f64,
    set_layer_hatch: bool,
    layer_hatch: NameBox,
    set_layer_dim_style: bool,
    layer_dim_style: NameBox,
    set_layer_visible: bool,
    layer_visible: YesNo,
    set_layer_locked: bool,
    layer_locked: YesNo,

    // Modify > Styles
    set_style_colour: bool,
    style_colour: String,
    set_style_linetype: bool,
    style_linetype: NameBox,
    set_style_weight: bool,
    style_weight: f64,
    set_style_hatch: bool,
    style_hatch: NameBox,
    set_style_symbol: bool,
    style_symbol: NameBox,
    set_style_symbol_size: bool,
    style_symbol_size: f64,
    set_style_symbol_scale: bool,
    style_symbol_scale: f64,

    summary: String,
    summary_is_error: bool,
    /// The colour field whose colour button is shown, by its object name.
    picking: Option<&'static str>,
    /// An edit was made this frame: the preview is worked out at its end.
    preview_due: bool,
    /// The dialog has been drawn once: the rule for what it opens on has run.
    shown: bool,
    closing: bool,

    // Last, so it goes first: no delivery reaches a half-dropped form.
    watcher: DocumentWatcher,
}

impl GlobalModifyDialog {
    pub fn new(context: &CustomisationContext, document: &Document) -> Self {
        let mut dialog = Self {
            context: context.clone(),
            frame: DialogFrame::new(DIALOG, TITLE),
            scope_filter: ScopeFilterWidget::new("globalModify"),
            tab: 0,
            set_layer: false,
            layer: NameBox::new(),
            set_colour: false,
            colour: String::new(),
            set_style: false,
            style: NameBox::new(),
            set_visible: false,
            visible: YesNo::new("Shown", "Hidden"),
            set_symbol: false,
            symbol: NameBox::new(),
            symbol_size: 0.0,
            set_text_height: false,
            text_height: 2.5,
            set_property: false,
            property_key: String::new(),
            property_value: String::new(),
            property_type: 0,
            remove_property: false,
            remove_key: String::new(),
            set_layer_colour: false,
            layer_colour: String::new(),
            set_layer_linetype: false,
            layer_linetype: NameBox::new(),
            set_layer_weight: false,
            layer_weight: 0.25,
            set_layer_hatch: false,
            layer_hatch: NameBox::new(),
            set_layer_dim_style: false,
            layer_dim_style: NameBox::new(),
            set_layer_visible: false,
            layer_visible: YesNo::new("Shown", "Hidden"),
            set_layer_locked: false,
            layer_locked: YesNo::new("Locked", "Unlocked"),
            set_style_colour: false,
            style_colour: String::new(),
            set_style_linetype: false,
            style_linetype: NameBox::new(),
            set_style_weight: false,
            style_weight: 0.25,
            set_style_hatch: false,
            style_hatch: NameBox::new(),
            set_style_symbol: false,
            style_symbol: NameBox::new(),
            set_style_symbol_size: false,
            style_symbol_size: 0.0,
            set_style_symbol_scale: false,
            style_symbol_scale: 2.0,
            summary: String::new(),
            summary_is_error: false,
            picking: None,
            preview_due: false,
            shown: false,
            closing: false,
            watcher: DocumentWatcher::new(document),
        };
        dialog.reload(document);
        dialog.say(
            "Choose what to apply to, tick what to change, then Preview or Apply.",
            false,
        );
        dialog
    }

    fn live(&self) -> bool {
        self.watcher.document_alive()
    }

    fn say(&mut self, message: &str, is_error: bool) {
        self.summary = String::from(message);
        self.summary_is_error = is_error;
    }

    /// The workspace's open views, asked whenever the view list or a View scope is needed. Never
    /// given (a test, or no workspace): the View scope offers one "Whole drawing view" hiding
    /// nothing. Call `reload` after.
    pub fn set_views(&mut self, views: GlobalModifyViews) {
        self.scope_filter.views = Some(Box::new(move || views()));
    }

    /// The shared Apply to and Only those that match controls.
    pub fn scope_filter(&self) -> &ScopeFilterWidget {
        &self.scope_filter
    }

    pub fn scope_filter_mut(&mut self) -> &mut ScopeFilterWidget {
        &mut self.scope_filter
    }

    /// Refills the lists - layers, styles, linetypes, symbols, views - keeping what is checked and
    /// typed. The watcher's delivery calls it; a caller that needs it at once may too.
    pub fn reload(&mut self, document: &Document) {
        if !self.live() {
            return;
        }
        let model = document.model();

        // The layers and the views, keeping what is ticked and chosen.
        self.scope_filter.reload(model);

        self.layer.refill(model.layers.names());

        let mut style_names = vec![String::from(BY_LAYER)];
        style_names.extend(model.styles.names());
        self.style.refill(style_names);

        let symbols: Vec<String> = symbol_choices(document)
            .into_iter()
            .map(|entry| entry.name)
            .collect();
        self.symbol.refill(symbols.clone());
        let mut style_symbols = vec![String::from("(plain mark)")];
        style_symbols.extend(symbols);
        self.style_symbol.refill(style_symbols);

        self.layer_linetype.refill(
            linetype_choices(document, false)
                .into_iter()
                .map(|entry| entry.name)
                .collect(),
        );
        self.style_linetype.refill(
            linetype_choices(document, true)
                .into_iter()
                .map(|entry| entry.name)
                .collect(),
        );

        let hatches = model.hatch_patterns.names();
        self.layer_hatch.refill(hatches.clone());
        let mut style_hatches = vec![String::from(BY_LAYER)];
        style_hatches.extend(hatches);
        self.style_hatch.refill(style_hatches);

        let mut dim_styles = vec![String::from("(document default)")];
        dim_styles.extend(model.dimension_styles.names());
        self.layer_dim_style.refill(dim_styles);
    }

    pub fn summary_text(&self) -> &str {
        &self.summary
    }

    // ---- what the form says. Fails naming the field for text that does not read - a colour that
    // is not #RRGGBB or ByLayer, a view that has closed.

    pub fn scope(&self) -> Result<ModifyScope> {
        self.scope_filter.scope()
    }

    pub fn filter(&self) -> Result<ModifyFilter> {
        self.scope_filter.filter()
    }

    pub fn modification(&self) -> Result<GlobalModify> {
        let mut change = GlobalModify::default();
        let entities = &mut change.entities;
        if self.set_layer {
            entities.layer = Some(self.layer.text());
        }
        if self.set_colour {
            entities.colour = Some(read_colour(&self.colour, "the colour", true)?);
        }
        if self.set_style {
            let name = self.style.text();
            entities.style = Some(
                if equals_ignoring_case(name.as_bytes(), BY_LAYER.as_bytes()) {
                    String::new()
                } else {
                    name
                },
            );
        }
        if self.set_visible {
            entities.visible = Some(self.visible.yes());
        }
        if self.set_symbol {
            entities.symbol = Some(SymbolModify {
                name: self.symbol.text(),
                size: self.symbol_size,
            });
        }
        if self.set_text_height {
            entities.text_height = Some(self.text_height);
        }
        if self.set_property {
            let key = String::from(self.property_key.trim());
            let value = self.property_value.as_str();
            let refused = |what: &str| -> Error {
                make_error(
                    ErrorCode::InvalidArgument,
                    format!("the property's value: {what}"),
                    value,
                )
            };
            let typed = match self.property_type {
                1 => PropertyValue::Integer(
                    parse_integer(trimmed(value.as_bytes()))
                        .ok_or_else(|| refused("not an integer"))?,
                ),
                2 => PropertyValue::Real(
                    parse_finite_double(trimmed(value.as_bytes()))
                        .ok_or_else(|| refused("not a number"))?,
                ),
                3 => {
                    let folded = lowered(trimmed(value.as_bytes()));
                    if folded != b"true" && folded != b"false" {
                        return Err(refused("true or false"));
                    }
                    PropertyValue::Boolean(folded == b"true")
                }
                _ => PropertyValue::Text(String::from(value)),
            };
            entities.set_properties.push((key, typed));
        }
        if self.remove_property {
            entities
                .remove_properties
                .push(String::from(self.remove_key.trim()));
        }

        let layers = &mut change.layers;
        if self.set_layer_colour {
            // Never None: a layer's colour cannot be ByLayer, which read_colour then refuses.
            layers.colour = read_colour(&self.layer_colour, "the layers' colour", false)?;
        }
        if self.set_layer_linetype {
            layers.linetype = Some(self.layer_linetype.text());
        }
        if self.set_layer_weight {
            layers.line_weight = Some(self.layer_weight);
        }
        if self.set_layer_hatch {
            layers.hatch_pattern = Some(self.layer_hatch.text());
        }
        if self.set_layer_dim_style {
            layers.dimension_style = Some(
                if self.layer_dim_style.choice.index == Some(0)
                    && self.layer_dim_style.reads_first()
                {
                    String::new()
                } else {
                    self.layer_dim_style.text()
                },
            );
        }
        if self.set_layer_visible {
            layers.visible = Some(self.layer_visible.yes());
        }
        if self.set_layer_locked {
            layers.locked = Some(self.layer_locked.yes());
        }

        let styles = &mut change.styles;
        if self.set_style_colour {
            styles.colour = Some(read_colour(
                &self.style_colour,
                "the styles' colour",
                true,
            )?);
        }
        if self.set_style_linetype {
            styles.linetype = Some(self.style_linetype.text());
        }
        if self.set_style_weight {
            styles.line_weight = Some(self.style_weight);
        }
        if self.set_style_hatch {
            let name = self.style_hatch.text();
            styles.hatch_pattern = Some(
                if equals_ignoring_case(name.as_bytes(), BY_LAYER.as_bytes()) {
                    String::new()
                } else {
                    name
                },
            );
        }
        if self.set_style_symbol {
            styles.symbol = Some(if self.style_symbol.reads_first() {
                String::new()
            } else {
                self.style_symbol.text()
            });
        }
        if self.set_style_symbol_size {
            styles.symbol_size = Some(self.style_symbol_size);
        }
        if self.set_style_symbol_scale {
            styles.symbol_scale = Some(self.style_symbol_scale);
        }
        Ok(change)
    }

    /// The three parts of the form, or the first that does not read.
    fn read_request(&self) -> Result<Request> {
        Ok(Request {
            scope: self.scope()?,
            filter: self.filter()?,
            change: self.modification()?,
        })
    }

    /// Beside each type box, how many of that kind the scope and the REST of the filter take:
    /// what ticking that box would take. Counted by cad's count_by_type, as SELECT's types= is;
    /// no numbers while the scope or the filter does not read.
    fn count_types(&mut self, document: &Document) {
        let (Ok(scope), Ok(mut filter)) = (self.scope_filter.scope(), self.scope_filter.filter())
        else {
            self.scope_filter.hide_type_counts();
            return;
        };
        filter.types.clear();
        match match_entities(document, &scope, &filter) {
            Ok(matched) => self
                .scope_filter
                .show_type_counts(&count_by_type(document.model(), &matched)),
            Err(_) => self.scope_filter.hide_type_counts(),
        }
    }

    // ---- each shows its outcome in the summary and the log, and returns whether it did what it
    // says: preview whether the request is valid, apply whether the drawing changed,
    // select_matches whether anything was selected.

    /// `document` is the Document the dialog was made with; nothing is read once it has gone.
    pub fn preview(&mut self, document: &Document) -> bool {
        if !self.live() {
            return false;
        }
        self.count_types(document);
        let request = match self.read_request() {
            Ok(request) => request,
            Err(error) => {
                self.say(&error.describe(), true);
                return false;
            }
        };
        if request.change.empty() {
            return match match_entities(document, &request.scope, &request.filter) {
                Ok(matched) => {
                    self.say(
                        &format!(
                            "{}. Tick a field to change.",
                            match_count(matched.len(), " entity matches", " entities match")
                        ),
                        false,
                    );
                    true
                }
                Err(error) => {
                    self.say(&error.describe(), true);
                    false
                }
            };
        }
        match plan_global_modify(document, &request.scope, &request.filter, &request.change) {
            Ok(plan) => {
                self.say(&plan.summary(), false);
                true
            }
            Err(error) => {
                self.say(&error.describe(), true);
                false
            }
        }
    }

    fn refuse(&mut self, host: &mut dyn Host, error: &Error) -> bool {
        let message = error.describe();
        self.say(&message, true);
        host.log(&format!("Global Modify: {message}"), true);
        false
    }

    pub fn apply(&mut self, host: &mut dyn Host) -> bool {
        if !self.live() {
            return false;
        }
        self.preview_due = false;
        let request = match self.read_request() {
            Ok(request) => request,
            Err(error) => return self.refuse(host, &error),
        };
        let planned = plan_global_modify(
            host.document(),
            &request.scope,
            &request.filter,
            &request.change,
        );
        let mut plan = match planned {
            Ok(plan) => plan,
            Err(error) => return self.refuse(host, &error),
        };
        let summary = plan.summary();
        let Some(command) = plan.command.take() else {
            self.say(&summary, false);
            host.log(&format!("Global Modify: {summary}"), false);
            return false;
        };
        if let Err(error) = host.document_mut().execute(command) {
            return self.refuse(host, &error);
        }
        self.say(
            &format!("Done: {summary} One Undo puts it all back."),
            false,
        );
        host.log(
            &format!("Global Modify: {summary} (one Undo restores it)"),
            false,
        );
        true
    }

    /// Needs the context's runner: with none it says the line to type and selects nothing.
    pub fn select_matches(&mut self, host: &mut dyn Host) -> bool {
        if !self.live() {
            return false;
        }
        // The line a person would type - SELECT DRAWING WHERE TYPE=leader - run by the window's
        // one executor, so it is in the log and the history as a typed line is, and selects by
        // SELECT's rule: what is hidden or locked is counted and left out.
        let words = match self.scope_filter.verb_words() {
            Ok(words) => words,
            Err(error) => {
                self.say(&error.describe(), true);
                return false;
            }
        };
        let line = format!("SELECT {words}");
        if !self.context.has_runner {
            self.say(&format!("Nothing runs the line here: type {line}"), true);
            return false;
        }
        let outcome = host.run(&line);
        if !outcome.ok {
            self.say(&outcome.error, true);
            return false;
        }
        let ids = host.document().selection().ids();
        // Framed in the active plan view, when the window has one.
        if !ids.is_empty()
            && let Some(select_and_show) = self.context.select_and_show.clone()
        {
            select_and_show(host, &ids);
        }
        let mut said = match_count(ids.len(), " entity selected.", " entities selected.");
        // The reply is SELECT's record; what it could not select is its key.
        for field in outcome.reply.split_whitespace() {
            if let Some(count) = field.strip_prefix("unselectable=") {
                said.push_str(&format!(" {count} hidden or on a locked layer left out."));
            }
        }
        self.say(&said, false);
        !ids.is_empty()
    }

    // ---- the frame -----------------------------------------------------------------------------

    /// Draws the dialog for this frame - the drawing's changes since the last first; false once it
    /// is closed, by its Close button or its title bar's.
    pub fn ui(&mut self, ctx: &Context, host: &mut dyn Host) -> bool {
        if !self.shown {
            self.shown = true;
            // Quick Select's rule: opened with nothing selected, "Selected entities" takes
            // nothing, and a type ticked to select by (the owner's "select elements by their type
            // such as selecting leaders", 2026-11-03) found none. So it opens on the whole drawing
            // then. Only as it opens: a scope chosen while it is open is never changed under the
            // person.
            if self.live()
                && host.document().selection().empty()
                && self.scope_filter.choice() == ScopeChoice::Selection
            {
                self.scope_filter.set_choice(ScopeChoice::Drawing);
            }
            self.preview_due = true;
        }
        if let Some(changes) = self.watcher.deliver(host.document()) {
            if changes.model || changes.library {
                self.reload(host.document());
            }
            if changes.model || changes.selection {
                // What matches, and what it would change, may differ now.
                self.preview(host.document());
            }
        }
        let mut frame = std::mem::replace(&mut self.frame, DialogFrame::new(DIALOG, TITLE));
        let mut closed = show_dialog(&mut frame, ctx, self, host, Self::body, Self::footer);
        self.frame = frame;
        closed |= std::mem::take(&mut self.closing);
        // A preview after the edits of one frame, not one per control.
        if std::mem::take(&mut self.preview_due) {
            self.preview(host.document());
        }
        !closed
    }

    fn body(&mut self, ui: &mut Ui, _host: &mut dyn Host) {
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                if self.scope_filter.ui(ui) {
                    self.preview_due = true;
                }
            });
            ui.group(|ui| {
                ui.vertical(|ui| {
                    ui.strong("Modify");
                    ui.add(
                        egui::Label::new(
                            "Tick a field to change it; an unticked field is left as each one \
                             has it.",
                        )
                        .wrap(),
                    );
                    controls::tabs(
                        ui,
                        "globalModifyTabs",
                        &["Entities", "Their Layers", "Their Styles"],
                        &mut self.tab,
                    );
                    // A page not in front is still there, as a QTabWidget's is: what it holds can
                    // be ticked, filled and read.
                    let pages: [(&str, fn(&mut Self, &mut Ui)); 3] = [
                        ("globalModifyEntitiesTab", Self::entities_tab),
                        ("globalModifyLayersTab", Self::layers_tab),
                        ("globalModifyStylesTab", Self::styles_tab),
                    ];
                    for (at, (name, page)) in pages.into_iter().enumerate() {
                        if at == self.tab {
                            page(self, ui);
                        } else {
                            offstage(ui, name, |ui| page(self, ui));
                        }
                    }
                });
            });
        });
    }

    fn footer(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        let mut said = egui::RichText::new(self.summary.as_str());
        if self.summary_is_error {
            said = said.color(ERROR_COLOUR);
        }
        controls::label_as(
            ui,
            "globalModifySummary",
            &self.summary,
            egui::Label::new(said).wrap(),
        );
        ui.horizontal(|ui| {
            if controls::button(ui, "globalModifyPreview", "Preview")
                .on_hover_text("Say what Apply would change, changing nothing")
                .clicked()
            {
                self.preview(host.document());
            }
            if controls::button(ui, "globalModifySelect", "Select Matches")
                .on_hover_text("Select what the scope and the filter take, and show it")
                .clicked()
            {
                self.select_matches(host);
            }
            if controls::button(ui, "globalModifyApply", "Apply")
                .on_hover_text("Make the change: one Undo puts everything back")
                .clicked()
            {
                self.apply(host);
            }
            if controls::button(ui, "globalModifyClose", "Close").clicked() {
                self.closing = true;
            }
        });
    }

    /// The tick in front of a field, which enables its editors.
    fn tick(ui: &mut Ui, name: &str, label: &str, on: &mut bool, due: &mut bool) {
        if controls::check(ui, name, label, on).changed() {
            *due = true;
        }
    }

    fn line(ui: &mut Ui, name: &str, value: &mut String, placeholder: &str, due: &mut bool) {
        if controls::line_with(
            ui,
            name,
            value,
            LineOptions {
                placeholder,
                ..LineOptions::default()
            },
        )
        .changed()
        {
            *due = true;
        }
    }

    /// A colour typed as text, with a Choose... button beside it showing a colour to pick.
    fn colour_row(
        ui: &mut Ui,
        name: &'static str,
        choose: &str,
        value: &mut String,
        by_layer: bool,
        picking: &mut Option<&'static str>,
        due: &mut bool,
    ) {
        ui.horizontal(|ui| {
            let placeholder = if by_layer {
                "#RRGGBB or ByLayer"
            } else {
                "#RRGGBB"
            };
            Self::line(ui, name, value, placeholder, due);
            if controls::button(ui, choose, "Choose...").clicked() {
                *picking = if *picking == Some(name) {
                    None
                } else {
                    Some(name)
                };
            }
            if *picking == Some(name) {
                let current = read_colour(value, "colour", true)
                    .ok()
                    .flatten()
                    .unwrap_or_default();
                let mut rgb = [current.r, current.g, current.b];
                if ui.color_edit_button_srgb(&mut rgb).changed() {
                    *value = format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2]);
                    *due = true;
                }
            }
        });
    }

    fn entities_tab(&mut self, ui: &mut Ui) {
        let mut due = false;
        egui::Grid::new("globalModifyEntitiesTab")
            .num_columns(2)
            .show(ui, |ui| {
                Self::tick(
                    ui,
                    "globalModifySetLayer",
                    "Layer",
                    &mut self.set_layer,
                    &mut due,
                );
                ui.add_enabled_ui(self.set_layer, |ui| {
                    due |= self.layer.ui(ui, "globalModifyLayer");
                });
                ui.end_row();

                Self::tick(
                    ui,
                    "globalModifySetColour",
                    "Colour",
                    &mut self.set_colour,
                    &mut due,
                );
                ui.add_enabled_ui(self.set_colour, |ui| {
                    Self::colour_row(
                        ui,
                        "globalModifyColour",
                        "globalModifyColourChoose",
                        &mut self.colour,
                        true,
                        &mut self.picking,
                        &mut due,
                    );
                });
                ui.end_row();

                Self::tick(
                    ui,
                    "globalModifySetStyle",
                    "Style",
                    &mut self.set_style,
                    &mut due,
                );
                ui.add_enabled_ui(self.set_style, |ui| {
                    due |= self.style.ui(ui, "globalModifyStyle");
                });
                ui.end_row();

                Self::tick(
                    ui,
                    "globalModifySetVisible",
                    "Shown",
                    &mut self.set_visible,
                    &mut due,
                );
                ui.add_enabled_ui(self.set_visible, |ui| {
                    due |= self.visible.ui(ui, "globalModifyVisible");
                });
                ui.end_row();

                Self::tick(
                    ui,
                    "globalModifySetSymbol",
                    "Symbol (points)",
                    &mut self.set_symbol,
                    &mut due,
                );
                ui.add_enabled_ui(self.set_symbol, |ui| {
                    ui.horizontal(|ui| {
                        due |= self.symbol.ui(ui, "globalModifySymbol");
                        due |= controls::spin(
                            ui,
                            "globalModifySymbolSize",
                            &mut self.symbol_size,
                            size_range(1e6, 3),
                        )
                        .on_hover_text("Width in model units; 0 is the symbol's own size")
                        .changed();
                    });
                });
                ui.end_row();

                Self::tick(
                    ui,
                    "globalModifySetTextHeight",
                    "Text height",
                    &mut self.set_text_height,
                    &mut due,
                );
                ui.add_enabled_ui(self.set_text_height, |ui| {
                    due |= controls::spin(
                        ui,
                        "globalModifyTextHeight",
                        &mut self.text_height,
                        size_range(1e6, 3),
                    )
                    .changed();
                });
                ui.end_row();

                Self::tick(
                    ui,
                    "globalModifySetProperty",
                    "Set property",
                    &mut self.set_property,
                    &mut due,
                );
                ui.add_enabled_ui(self.set_property, |ui| {
                    ui.horizontal(|ui| {
                        Self::line(
                            ui,
                            "globalModifyPropertyKey",
                            &mut self.property_key,
                            "key",
                            &mut due,
                        );
                        Self::line(
                            ui,
                            "globalModifyPropertyValue",
                            &mut self.property_value,
                            "value",
                            &mut due,
                        );
                        let kinds: Vec<String> = ["text", "integer", "real", "boolean"]
                            .iter()
                            .map(|kind| String::from(*kind))
                            .collect();
                        let mut kind = Choice::at(self.property_type);
                        if controls::choice(ui, "globalModifyPropertyType", &kinds, &mut kind)
                            .changed()
                        {
                            due = true;
                        }
                        if let Some(index) = kind.index {
                            self.property_type = index;
                        }
                    });
                });
                ui.end_row();

                Self::tick(
                    ui,
                    "globalModifyRemoveProperty",
                    "Remove property",
                    &mut self.remove_property,
                    &mut due,
                );
                ui.add_enabled_ui(self.remove_property, |ui| {
                    Self::line(
                        ui,
                        "globalModifyRemoveKey",
                        &mut self.remove_key,
                        "key",
                        &mut due,
                    );
                });
                ui.end_row();
            });
        self.preview_due |= due;
    }

    fn layers_tab(&mut self, ui: &mut Ui) {
        let mut due = false;
        ui.add(
            egui::Label::new(
                "The layers the matched entities are on - or, when applying to layers, the \
                 checked layers themselves.",
            )
            .wrap(),
        );
        egui::Grid::new("globalModifyLayersTab")
            .num_columns(2)
            .show(ui, |ui| {
                Self::tick(
                    ui,
                    "globalModifySetLayerColour",
                    "Colour",
                    &mut self.set_layer_colour,
                    &mut due,
                );
                ui.add_enabled_ui(self.set_layer_colour, |ui| {
                    Self::colour_row(
                        ui,
                        "globalModifyLayerColour",
                        "globalModifyLayerColourChoose",
                        &mut self.layer_colour,
                        false,
                        &mut self.picking,
                        &mut due,
                    );
                });
                ui.end_row();

                Self::tick(
                    ui,
                    "globalModifySetLayerLinetype",
                    "Linetype",
                    &mut self.set_layer_linetype,
                    &mut due,
                );
                ui.add_enabled_ui(self.set_layer_linetype, |ui| {
                    due |= self.layer_linetype.ui(ui, "globalModifyLayerLinetype");
                });
                ui.end_row();

                Self::tick(
                    ui,
                    "globalModifySetLayerWeight",
                    "Line weight",
                    &mut self.set_layer_weight,
                    &mut due,
                );
                ui.add_enabled_ui(self.set_layer_weight, |ui| {
                    due |= controls::spin(
                        ui,
                        "globalModifyLayerWeight",
                        &mut self.layer_weight,
                        size_range(10.0, 2).with_suffix(" mm"),
                    )
                    .changed();
                });
                ui.end_row();

                Self::tick(
                    ui,
                    "globalModifySetLayerHatch",
                    "Hatch",
                    &mut self.set_layer_hatch,
                    &mut due,
                );
                ui.add_enabled_ui(self.set_layer_hatch, |ui| {
                    due |= self.layer_hatch.ui(ui, "globalModifyLayerHatch");
                });
                ui.end_row();

                Self::tick(
                    ui,
                    "globalModifySetLayerDimStyle",
                    "Dimension style",
                    &mut self.set_layer_dim_style,
                    &mut due,
                );
                ui.add_enabled_ui(self.set_layer_dim_style, |ui| {
                    due |= self.layer_dim_style.ui(ui, "globalModifyLayerDimStyle");
                });
                ui.end_row();

                Self::tick(
                    ui,
                    "globalModifySetLayerVisible",
                    "Shown",
                    &mut self.set_layer_visible,
                    &mut due,
                );
                ui.add_enabled_ui(self.set_layer_visible, |ui| {
                    due |= self.layer_visible.ui(ui, "globalModifyLayerVisible");
                });
                ui.end_row();

                if controls::check(
                    ui,
                    "globalModifySetLayerLocked",
                    "Locked",
                    &mut self.set_layer_locked,
                )
                .on_hover_text(
                    "An unlock comes first, so what is on the layer can be changed; a lock comes \
                     last",
                )
                .changed()
                {
                    due = true;
                }
                ui.add_enabled_ui(self.set_layer_locked, |ui| {
                    due |= self.layer_locked.ui(ui, "globalModifyLayerLocked");
                });
                ui.end_row();
            });
        self.preview_due |= due;
    }

    fn styles_tab(&mut self, ui: &mut Ui) {
        let mut due = false;
        ui.add(
            egui::Label::new(
                "The styles the matched entities wear. A style is shared: everything wearing it \
                 is redrawn, in the scope or not, and the summary counts those.",
            )
            .wrap(),
        );
        egui::Grid::new("globalModifyStylesTab")
            .num_columns(2)
            .show(ui, |ui| {
                Self::tick(
                    ui,
                    "globalModifySetStyleColour",
                    "Colour",
                    &mut self.set_style_colour,
                    &mut due,
                );
                ui.add_enabled_ui(self.set_style_colour, |ui| {
                    Self::colour_row(
                        ui,
                        "globalModifyStyleColour",
                        "globalModifyStyleColourChoose",
                        &mut self.style_colour,
                        true,
                        &mut self.picking,
                        &mut due,
                    );
                });
                ui.end_row();

                Self::tick(
                    ui,
                    "globalModifySetStyleLinetype",
                    "Linetype",
                    &mut self.set_style_linetype,
                    &mut due,
                );
                ui.add_enabled_ui(self.set_style_linetype, |ui| {
                    due |= self.style_linetype.ui(ui, "globalModifyStyleLinetype");
                });
                ui.end_row();

                Self::tick(
                    ui,
                    "globalModifySetStyleWeight",
                    "Line weight",
                    &mut self.set_style_weight,
                    &mut due,
                );
                ui.add_enabled_ui(self.set_style_weight, |ui| {
                    due |= controls::spin(
                        ui,
                        "globalModifyStyleWeight",
                        &mut self.style_weight,
                        size_range(10.0, 2).with_suffix(" mm"),
                    )
                    .changed();
                });
                ui.end_row();

                Self::tick(
                    ui,
                    "globalModifySetStyleHatch",
                    "Hatch",
                    &mut self.set_style_hatch,
                    &mut due,
                );
                ui.add_enabled_ui(self.set_style_hatch, |ui| {
                    due |= self.style_hatch.ui(ui, "globalModifyStyleHatch");
                });
                ui.end_row();

                Self::tick(
                    ui,
                    "globalModifySetStyleSymbol",
                    "Symbol",
                    &mut self.set_style_symbol,
                    &mut due,
                );
                ui.add_enabled_ui(self.set_style_symbol, |ui| {
                    due |= self.style_symbol.ui(ui, "globalModifyStyleSymbol");
                });
                ui.end_row();

                Self::tick(
                    ui,
                    "globalModifySetStyleSymbolSize",
                    "Symbol size",
                    &mut self.set_style_symbol_size,
                    &mut due,
                );
                ui.add_enabled_ui(self.set_style_symbol_size, |ui| {
                    due |= controls::spin(
                        ui,
                        "globalModifyStyleSymbolSize",
                        &mut self.style_symbol_size,
                        size_range(1e6, 3),
                    )
                    .changed();
                });
                ui.end_row();

                // A factor on each style's own number (StyleModify::symbol_scale): 2 doubles every
                // symbol, 0.5 halves them. 1000 is only the box's reach; MODIFY ... SET
                // STYLE.SYMBOLSCALE= takes any factor above 0.
                Self::tick(
                    ui,
                    "globalModifySetStyleSymbolScale",
                    "Scale symbol size by",
                    &mut self.set_style_symbol_scale,
                    &mut due,
                );
                ui.add_enabled_ui(self.set_style_symbol_scale, |ui| {
                    due |= controls::spin(
                        ui,
                        "globalModifyStyleSymbolScale",
                        &mut self.style_symbol_scale,
                        SpinRange::new(0.001, 1000.0, 1.0, 3),
                    )
                    .on_hover_text(
                        "Multiplies each style's symbol size: 2 doubles them, 0.5 halves them. A \
                         symbol at its own size has no size to multiply and is left as it is",
                    )
                    .changed();
                });
                ui.end_row();
            });
        self.preview_due |= due;
    }
}
