//! The survey code manager's other four tabs: the codes a drawing carries, what is wrong with the
//! rules being edited, applying the codes, and turning coded points into lines. Each shows a cad
//! report. On the two that change the drawing, Preview plans and never executes, and Execute hands
//! the line the tab shows - CODE ..., LINEWORK ... - to the window's executor: one undo step,
//! logged and undone as a typed line (code_manager). The port of customisation/code_manager_tabs.

use std::sync::Arc;

use egui::{RichText, Ui};
use katana_cad::Document;
use katana_cad::code_table::{
    BuiltInSymbolTest, ColourLookup, LintSeverity, code_census_of, format_coding_report,
    lint_survey_map,
};
use katana_cad::colour_lookup::colour_lookup;
use katana_cad::global_modify::match_entities;
use katana_cad::linework::{LineworkOptions, LineworkOrder};
use katana_cad::linework_verbs::{LineworkWords, plan_linework};
use katana_cad::survey_coding::{
    ColourOf, SurveyCodingOptions, SurveyCodingReport, apply_survey_codes,
    code_property_candidates, find_code_property, survey_code_of,
};
use katana_core::text::{lowered, read_reply_record};
use katana_core::{Error, ErrorCode, Result};
use katana_entity::colour_names::resolve_colour;
use katana_entity::entity::to_string;
use katana_entity::linework_codes::{LineworkCodes, linework_code_members};
use katana_entity::tables::is_built_in_symbol_name;
use katana_entity::{Color, EntityId, SurveyMatchKind, validate};

use super::code_manager::{
    CensusRow, Event, Jump, REFUSAL_COLOUR, SurveyCodeManagerDialog, match_class,
};
use super::code_manager_form::ascii_trimmed;
use super::codes_seams::{UNDEFINED_NAME_COLOUR, linework_control_label};
use super::definition_editor::index_choice;
use super::filter_bar::{FilterBar, FilterBarChanges};
use super::scope_filter_widget::{ScopeChoice, ScopeFilterWidget, ViewsFn};
use crate::annotation::dialog_support::table;
use crate::command_word::{command_word, exact_number};
use crate::drive::controls::{self, Choice, LineOptions, SpinRange, TextOptions};
use crate::drive::registry::{TargetKind, TargetSpec, begin_target, end_target};
use crate::host::Host;
use crate::survey::survey_dialogs::{hidden, offstage};

/// How far a curve's chords may stray from the arc, in millimetres for a drawing in metres.
const CHORD: SpinRange = SpinRange::new(0.1, 1000.0, 1.0, 1).with_suffix(" mm");

const COMMAND_TIP: &str = "The line Execute hands to the command line - typed there, or given to \
                           a script or an agent, it does the same";

/// The chord length the Linework tab starts on, in millimetres: the one process_linework itself
/// starts on (LineworkOptions, 0.005 of a drawing in metres), so the tab adds CHORD to its line
/// only for a length a person set.
fn default_chord_millimetres() -> f64 {
    LineworkOptions::default().chord_tolerance * 1000.0
}

/// Whether `text`, with its ASCII letters lowered, holds `lowered_needle`.
fn holds(text: &str, lowered_needle: &[u8]) -> bool {
    if lowered_needle.is_empty() {
        return true;
    }
    lowered(text)
        .windows(lowered_needle.len())
        .any(|window| window == lowered_needle)
}

/// The line in its field, or - where the controls cannot be said on a line - why not, as the
/// field's placeholder.
fn show_line(text: &mut String, hint: &mut String, line: Result<String>) {
    match line {
        Ok(line) => {
            *text = line;
            hint.clear();
        }
        Err(error) => {
            text.clear();
            *hint = format!("nothing to run yet: {}", error.describe());
        }
    }
}

/// What a tab's scope and filter take NOW: cad's match_entities over the shared widget's reading
/// of its controls, as Global Modify's preview resolves them. Fails as the widget does (no layer
/// ticked, a view that has closed), and with NotFound for a layer the drawing lacks.
fn scope_takes(document: Option<&Document>, scope: &ScopeFilterWidget) -> Result<Vec<EntityId>> {
    let Some(document) = document else {
        return Err(Error::new(
            ErrorCode::InvalidState,
            "the drawing this manager edits is closed",
        ));
    };
    let place = scope.scope()?;
    let which = scope.filter()?;
    match_entities(document, &place, &which)
}

/// The scope widget of a tab on drawing data: the whole drawing to begin with, as the tabs began
/// before they had these controls - codes are applied, and a survey strung, to all of it far more
/// often than to what happens to be selected.
fn make_scope(prefix: &str) -> ScopeFilterWidget {
    let mut scope = ScopeFilterWidget::new(prefix);
    scope.set_choice(ScopeChoice::Drawing);
    scope
}

/// A property chooser's first item is "find it" (the empty name - apply_survey_codes then finds
/// it), then the names codes are usually kept under; any other may be typed.
fn property_items() -> Vec<String> {
    let mut items = vec![String::new()];
    items.extend(
        code_property_candidates()
            .iter()
            .map(|candidate| String::from(*candidate)),
    );
    items
}

/// A filter bar that is a target of its own inside the dialog (code_manager's module comment).
fn nested_filter(ui: &mut Ui, name: &str, bar: &mut FilterBar) -> FilterBarChanges {
    begin_target(ui, TargetSpec::new(name, TargetKind::Panel, ""));
    let drawn = ui.vertical(|ui| bar.ui(ui));
    end_target(ui, drawn.response.rect);
    drawn.inner
}

/// A read-only field showing the line a tab's Execute runs: what is run is never a surprise, and
/// it can be copied to a script or handed to an agent.
fn command_field(ui: &mut Ui, name: &str, text: &str, hint: &str) {
    let mut shown = String::from(text);
    controls::line_with(
        ui,
        name,
        &mut shown,
        LineOptions {
            placeholder: hint,
            read_only: true,
            ..LineOptions::default()
        },
    )
    .on_hover_text(COMMAND_TIP);
}

/// A note shown only while the buffer has unapplied edits.
fn dirty_note(ui: &mut Ui, name: &str, text: &str, edited: bool) {
    let note = |ui: &mut Ui| {
        controls::label_as(
            ui,
            name,
            text,
            egui::Label::new(RichText::new(text).color(UNDEFINED_NAME_COLOUR.to_egui())).wrap(),
        );
    };
    if edited {
        note(ui);
    } else {
        hidden(ui, name, note);
    }
}

/// What the four tabs hold.
pub(super) struct TabState {
    property_items: Vec<String>,
    // codes in drawing
    census_scope: ScopeFilterWidget,
    census_command: String,
    census_command_hint: String,
    census_property: Choice,
    census_filter: FilterBar,
    census_rows: Vec<Vec<String>>,
    census_selected: String,
    census_summary: String,
    // issues
    issues_filter: FilterBar,
    issues_rows: Vec<Vec<String>>,
    issue_rules: Vec<usize>,
    issues_summary: String,
    pub(super) issues_title: String,
    // apply codes
    apply_scope: ScopeFilterWidget,
    apply_property: Choice,
    apply_command: String,
    apply_command_hint: String,
    apply_report: String,
    apply_rows: Vec<Vec<String>>,
    // linework
    linework_scope: ScopeFilterWidget,
    linework_fields: Vec<String>,
    linework_codes_status: String,
    linework_codes_bad: bool,
    linework_codes_use: bool,
    linework_order: usize,
    linework_chord: f64,
    linework_property: Choice,
    linework_command: String,
    linework_command_hint: String,
    linework_summary: String,
    linework_strings: Vec<Vec<String>>,
    linework_unplaced: Vec<Vec<String>>,
    linework_notes: Vec<Vec<String>>,
    linework_report_tab: usize,
}

impl TabState {
    pub(super) fn new() -> Self {
        let mut census_filter = FilterBar::new(&["All", "Matched", "Fallback only", "Unmatched"]);
        census_filter.set_placeholder_text("Search codes and layers");
        let mut issues_filter = FilterBar::new(&["All", "Errors", "Warnings"]);
        issues_filter.set_placeholder_text("Search keys and messages");
        Self {
            property_items: property_items(),
            census_scope: make_scope("census"),
            census_command: String::new(),
            census_command_hint: String::new(),
            census_property: Choice::editable(""),
            census_filter,
            census_rows: Vec::new(),
            census_selected: String::new(),
            census_summary: String::new(),
            issues_filter,
            issues_rows: Vec::new(),
            issue_rules: Vec::new(),
            issues_summary: String::new(),
            issues_title: String::from("Issues"),
            apply_scope: make_scope("apply"),
            apply_property: Choice::editable(""),
            apply_command: String::new(),
            apply_command_hint: String::new(),
            apply_report: String::new(),
            apply_rows: Vec::new(),
            linework_scope: make_scope("linework"),
            linework_fields: vec![String::new(); linework_code_members().len()],
            linework_codes_status: String::new(),
            linework_codes_bad: false,
            linework_codes_use: true,
            linework_order: 0,
            linework_chord: default_chord_millimetres(),
            linework_property: Choice::editable(""),
            linework_command: String::new(),
            linework_command_hint: String::new(),
            linework_summary: String::new(),
            linework_strings: Vec::new(),
            linework_unplaced: Vec::new(),
            linework_notes: Vec::new(),
            linework_report_tab: 0,
        }
    }
}

impl SurveyCodeManagerDialog {
    // ---- what the three tabs on drawing data share --------------------------------------------

    /// The scopes' views are the dialog's own, asked afresh each time a list is needed.
    pub(super) fn install_views(&mut self) {
        for scope in [
            &mut self.tabs.census_scope,
            &mut self.tabs.apply_scope,
            &mut self.tabs.linework_scope,
        ] {
            scope.views = self.views.clone().map(|views| -> ViewsFn {
                Box::new(move || views())
            });
        }
    }

    /// The Codes in Drawing tab's scope and filter controls.
    pub fn census_scope_mut(&mut self) -> &mut ScopeFilterWidget {
        &mut self.tabs.census_scope
    }

    /// The Apply Codes tab's scope and filter controls.
    pub fn apply_scope_mut(&mut self) -> &mut ScopeFilterWidget {
        &mut self.tabs.apply_scope
    }

    /// The Linework tab's scope and filter controls.
    pub fn linework_scope_mut(&mut self) -> &mut ScopeFilterWidget {
        &mut self.tabs.linework_scope
    }

    /// The layer and view lists of the three scopes, refilled; what is ticked is kept.
    pub(super) fn reload_scopes(&mut self, document: Option<&Document>) {
        for scope in [
            &mut self.tabs.census_scope,
            &mut self.tabs.apply_scope,
            &mut self.tabs.linework_scope,
        ] {
            match document {
                Some(document) => scope.reload(document.model()),
                None => scope.reload_views(),
            }
        }
    }

    /// The two lines shown again (applyCommand, lineworkCommand).
    pub(super) fn show_lines(&mut self) {
        let codes = self.codes_line();
        show_line(
            &mut self.tabs.apply_command,
            &mut self.tabs.apply_command_hint,
            codes,
        );
        let linework = self.linework_line();
        show_line(
            &mut self.tabs.linework_command,
            &mut self.tabs.linework_command_hint,
            linework,
        );
    }

    /// A tab's line: the verb, the scope's words, and PROPERTY when one is typed.
    fn line_with_property(verb: &str, scope: &ScopeFilterWidget, property: &str) -> Result<String> {
        let mut line = format!("{verb} {}", scope.verb_words()?);
        let property = property.trim();
        if !property.is_empty() {
            line.push_str(" PROPERTY ");
            line.push_str(&command_word(property, "Code property")?);
        }
        Ok(line)
    }

    // ---- codes in drawing ----------------------------------------------------------------------

    fn census_property(&self) -> String {
        String::from(ascii_trimmed(&self.tabs.census_property.text))
    }

    /// The census tab's line, CODE CENSUS <scope> [PROPERTY p]: what its Run hands to the
    /// executor, and what the table is counted by.
    fn census_line(&self) -> Result<String> {
        Self::line_with_property(
            "CODE CENSUS",
            &self.tabs.census_scope,
            &self.tabs.census_property.text,
        )
    }

    pub(super) fn refresh_census(&mut self, document: Option<&Document>) {
        self.census.clear();
        self.census_ids.clear();
        self.census_problem.clear();
        let line = self.census_line();
        show_line(
            &mut self.tabs.census_command,
            &mut self.tabs.census_command_hint,
            line,
        );
        if let Some(found) = document {
            // Counted here, not read from the verb's reply: the table is classed against the
            // rules being EDITED, which the verb (it reads the drawing's map) cannot be asked.
            // What it counts and what the scope takes are the verb's own (cad's code_census over
            // match_entities).
            match scope_takes(document, &self.tabs.census_scope) {
                Err(error) => self.census_problem = error.describe(),
                Ok(taken) => {
                    let census = code_census_of(found, &taken, &self.census_property());
                    self.census_ids = taken;
                    self.census_found_property = census.property;
                    self.census = census
                        .codes
                        .into_iter()
                        .map(|row| CensusRow {
                            code: row.code,
                            entities: row.entities,
                            kind: row.kind,
                            matched: row.matched,
                            model: row.model,
                        })
                        .collect();
                }
            }
        }
        self.filter_census();
    }

    fn run_census_line(&mut self, host: &mut dyn Host) {
        if !self.alive() {
            return;
        }
        let line = match self.census_line() {
            Ok(line) => line,
            Err(error) => {
                self.log(error.describe(), true);
                return;
            }
        };
        // A refused line is in the log already, where the executor ran it; only a manager with no
        // executor has yet to say so.
        if let Err(said) = self.run_line(host, &line)
            && !self.context.command_line
        {
            self.log(said, true);
        }
    }

    pub(super) fn filter_census(&mut self) {
        // Classed against the BUFFER, not the Document's map: the census is how a person checks
        // the rules they are writing against the codes the drawing actually carries, before
        // applying them.
        let mut entities = 0;
        let mut matched = 0;
        let mut fallback = 0;
        let mut unmatched = 0;
        for row in &mut self.census {
            let found = self.buffer.lookup(&row.code);
            row.kind = found.kind;
            row.matched = found.matched();
            row.model = found.resolved.model;
            entities += row.entities;
            if row.matched {
                matched += 1;
            } else if row.kind == SurveyMatchKind::FallbackOnly {
                fallback += 1;
            } else {
                unmatched += 1;
            }
        }
        let tabs = &mut self.tabs;
        tabs.census_filter
            .set_chip_label(1, &format!("Matched ({matched})"));
        tabs.census_filter
            .set_chip_label(2, &format!("Fallback only ({fallback})"));
        tabs.census_filter
            .set_chip_label(3, &format!("Unmatched ({unmatched})"));

        let filter = lowered(tabs.census_filter.text());
        let chip = tabs.census_filter.chip().unwrap_or(0);
        tabs.census_rows.clear();
        for row in &self.census {
            let is_fallback = !row.matched && row.kind == SurveyMatchKind::FallbackOnly;
            if (chip == 1 && !row.matched)
                || (chip == 2 && !is_fallback)
                || (chip == 3 && (row.matched || is_fallback))
            {
                continue;
            }
            if !holds(&row.code, &filter) && !holds(&row.model, &filter) {
                continue;
            }
            tabs.census_rows.push(vec![
                row.code.clone(),
                format!("{}", row.entities),
                String::from(match_class(row.kind, row.matched)),
                row.model.clone(),
            ]);
        }
        // The code chosen stays chosen while it is still listed.
        if !tabs
            .census_rows
            .iter()
            .any(|row| row[0] == tabs.census_selected)
        {
            tabs.census_selected.clear();
        }
        tabs.census_summary = if self.census_problem.is_empty() {
            format!(
                "{} entities matched the scope; {entities} carry a code under \"{}\": {} distinct \
                 codes - {matched} matched, {fallback} fallback only, {unmatched} unmatched by the \
                 rules being edited.",
                self.census_ids.len(),
                self.census_found_property,
                self.census.len()
            )
        } else {
            self.census_problem.clone()
        };
    }

    /// The census table's rows as they are shown: code, entities, class, layer.
    pub fn census_rows(&self) -> &[Vec<String>] {
        &self.tabs.census_rows
    }

    /// Makes the row of `code` the census table's current one, as a click on it does. False for a
    /// code the table does not show.
    pub fn select_census_code(&mut self, code: &str) -> bool {
        let listed = self.tabs.census_rows.iter().any(|row| row[0] == code);
        if listed {
            self.tabs.census_selected = String::from(code);
        }
        listed
    }

    /// A double-click on row `row` of the census: what the code gets is shown, as Test a Code
    /// would - at the next frame.
    pub fn activate_census_row(&mut self, row: usize) {
        if let Some(code) = self.tabs.census_rows.get(row).and_then(|row| row.first()) {
            self.pending = Some(Jump::TestCode(code.clone()));
        }
    }

    fn select_entities_with_code(&mut self, host: &mut dyn Host) {
        let code = self.tabs.census_selected.clone();
        if !self.alive() || code.is_empty() {
            self.log("Choose a code in the list first.", true);
            return;
        }
        // Among what the scope took, which is what the count above is of.
        let model = host.document().model();
        let ids: Vec<EntityId> = self
            .census_ids
            .iter()
            .copied()
            .filter(|&id| {
                model.entities.find(id).is_some_and(|entity| {
                    survey_code_of(entity, &self.census_found_property) == Some(code.as_str())
                })
            })
            .collect();
        match self.context.select_and_show.clone() {
            Some(select_and_show) => select_and_show(host, &ids),
            None => {
                host.document_mut().selection_mut().set(ids.clone());
                host.document().notify_selection_changed();
            }
        }
        self.log(
            format!("Selected the {} entities coded {code}.", ids.len()),
            false,
        );
    }

    // ---- issues --------------------------------------------------------------------------------

    pub(super) fn rebuild_issues(&mut self, document: Option<&Document>) {
        self.issues = match document {
            None => Vec::new(),
            Some(document) => {
                let colours = colour_lookup(document);
                let colour_of: &ColourLookup<'_> = &colours;
                let built_in: &BuiltInSymbolTest<'_> = &is_built_in_symbol_name;
                lint_survey_map(
                    &self.buffer,
                    document.style_library(),
                    Some(colour_of),
                    Some(built_in),
                )
            }
        };
        let errors = self
            .issues
            .iter()
            .filter(|issue| issue.severity == LintSeverity::Error)
            .count();
        let warnings = self.issues.len() - errors;
        let tabs = &mut self.tabs;
        tabs.issues_filter
            .set_chip_label(1, &format!("Errors ({errors})"));
        tabs.issues_filter
            .set_chip_label(2, &format!("Warnings ({warnings})"));

        let chip = tabs.issues_filter.chip().unwrap_or(0);
        let filter = lowered(tabs.issues_filter.text());
        tabs.issues_rows.clear();
        tabs.issue_rules.clear();
        for issue in &self.issues {
            let is_error = issue.severity == LintSeverity::Error;
            if (chip == 1 && !is_error) || (chip == 2 && is_error) {
                continue;
            }
            if !holds(&issue.key, &filter) && !holds(&issue.message, &filter) {
                continue;
            }
            tabs.issues_rows.push(vec![
                String::from(to_string(issue.severity)),
                format!("#{}", issue.rule),
                issue.key.clone(),
                String::from(to_string(issue.section)),
                String::from(to_string(issue.kind)),
                issue.message.clone(),
            ]);
            tabs.issue_rules.push(issue.rule);
        }
        tabs.issues_summary = format!(
            "{errors} errors and {warnings} warnings in the {} rules being edited.",
            self.buffer.size()
        );
        tabs.issues_title = if self.issues.is_empty() {
            String::from("Issues")
        } else {
            format!("Issues ({})", self.issues.len())
        };
    }

    /// The issues table's rows as they are shown: severity, rule, key, section, problem, message.
    pub fn issue_rows(&self) -> &[Vec<String>] {
        &self.tabs.issues_rows
    }

    /// A double-click on row `row` of the issues: its rule is opened in the form - at the next
    /// frame, as every jump from a list is.
    pub fn activate_issue(&mut self, row: usize) {
        if let Some(&rule) = self.tabs.issue_rules.get(row) {
            self.pending = Some(Jump::Rule(rule));
        }
    }

    // ---- apply codes ---------------------------------------------------------------------------

    fn coding_options(&self, document: Option<&Document>) -> SurveyCodingOptions {
        let mut options = SurveyCodingOptions {
            property: String::from(ascii_trimmed(&self.tabs.apply_property.text)),
            ..SurveyCodingOptions::default()
        };
        // The drawing's own resolver: its customisation's colours, then the standard names - what
        // the CODE line resolves a colour name by, so the preview shows the style the line will
        // make.
        if let Some(document) = document {
            let colours = document.customisation_state().colours.clone();
            let colour_of: ColourOf =
                Arc::new(move |name: &str| -> Option<Color> { resolve_colour(&colours, name) });
            options.colour_of = Some(colour_of);
        }
        options
    }

    /// The line the Apply Codes tab's Execute runs, as the controls stand; an error names the
    /// control that cannot be said on a line.
    fn codes_line(&self) -> Result<String> {
        Self::line_with_property(
            "CODE",
            &self.tabs.apply_scope,
            &self.tabs.apply_property.text,
        )
    }

    /// The preview fills the panes and says, with the log, why it could not; true when it planned.
    pub(super) fn preview_codes(&mut self, document: Option<&Document>) -> bool {
        self.tabs.apply_rows.clear();
        let Some(found) = document else {
            self.tabs.apply_report = String::from("The drawing is closed.");
            return false;
        };
        let taken = match scope_takes(document, &self.tabs.apply_scope) {
            Ok(taken) => taken,
            Err(error) => {
                self.tabs.apply_report = error.describe();
                self.log(error.describe(), true);
                return false;
            }
        };
        let mut options = self.coding_options(document);
        let mut report = SurveyCodingReport::default();
        let mut something = false;
        if taken.is_empty() {
            // A scope that took nothing is said, and is NOT handed on: to apply_survey_codes an
            // empty list is every entity in the drawing.
            report.property = if options.property.is_empty() {
                find_code_property(found.model(), &[])
            } else {
                options.property.clone()
            };
        } else {
            options.ids = taken.clone();
            match apply_survey_codes(found, &options, Some(&mut report)) {
                Ok(command) => something = command.is_some(),
                Err(error) => {
                    self.tabs.apply_report = error.describe();
                    self.log(error.describe(), true);
                    return false;
                }
            }
        }
        for row in &report.codes {
            self.tabs.apply_rows.push(vec![
                row.code.clone(),
                format!("{}", row.entities),
                String::from(match_class(row.kind, row.matched)),
                if row.layer.is_empty() {
                    String::from("(stays)")
                } else {
                    format!("{} -> {}", row.layers_from.join(", "), row.layer)
                },
                if row.style.is_empty() {
                    String::new()
                } else {
                    format!("{} ({})", row.style, to_string(row.style_outcome))
                },
                format!("{}", row.changed),
            ]);
        }
        // What the scope took first, as the line's own reply begins.
        let mut shown = format!("{} matched.\n", taken.len());
        shown.push_str(&format_coding_report(&report));
        shown.push_str(if something {
            "\nPreview only: nothing has changed. Execute runs the line above as one undoable \
             step."
        } else {
            "\nNothing to change."
        });
        self.tabs.apply_report = shown;
        true
    }

    fn execute_codes(&mut self, host: &mut dyn Host) {
        if !self.alive() {
            return;
        }
        let line = match self.codes_line() {
            Ok(line) => line,
            Err(error) => {
                self.tabs.apply_report = error.describe();
                self.log(error.describe(), true);
                return;
            }
        };
        // What is about to be done, row by row: the line's reply has the totals, not which code
        // went where. A preview that could not plan has said why, in the pane and the log; the
        // line would be refused for the same reason, so it is not run to say it twice.
        let document = self.live_document(host.document());
        if !self.preview_codes(document) {
            return;
        }
        match self.run_line(host, &line) {
            // In the verb's own words, which are in the command log as well: what the scope took,
            // the report, and whether anything was applied.
            Ok(outcome) => self.tabs.apply_report = outcome.reply,
            Err(said) => {
                self.tabs.apply_report = said.clone();
                // A refused line is in the log already, where the executor ran it; only a manager
                // with no executor has yet to say so.
                if !self.context.command_line {
                    self.log(said, true);
                }
            }
        }
    }

    /// The Apply Codes tab's report pane.
    pub fn apply_report(&self) -> &str {
        &self.tabs.apply_report
    }

    // ---- linework ------------------------------------------------------------------------------

    /// One of the seven fields set; a change is checked as typing is.
    fn set_linework_field(&mut self, at: usize, text: &str) {
        if let Some(field) = self.tabs.linework_fields.get_mut(at)
            && field.as_str() != text
        {
            *field = String::from(text);
            self.linework_codes_typed();
        }
    }

    fn linework_codes_typed(&mut self) {
        let valid = validate(&self.typed_linework_codes());
        self.tabs.linework_codes_status = match &valid {
            Ok(()) => String::from("Valid."),
            Err(error) => error.describe(),
        };
        self.tabs.linework_codes_bad = valid.is_err();
        self.tabs.linework_codes_use = valid.is_ok();
    }

    /// The seven fields from the customisation's control codes, which are the Document's; the
    /// usual spellings for a manager whose drawing has gone.
    pub(super) fn load_linework_codes(&mut self, document: Option<&Document>) {
        self.shown_linework = document.map_or_else(LineworkCodes::default, |document| {
            document.customisation_state().linework.clone()
        });
        let shown = self.shown_linework.clone();
        for (at, member) in linework_code_members().iter().enumerate() {
            self.set_linework_field(at, member.spelling(&shown));
        }
    }

    fn typed_linework_codes(&self) -> LineworkCodes {
        // The fields stand in the order the controls are declared in (entity's
        // linework_code_members), which is the order they were built in.
        let mut typed = LineworkCodes::default();
        for (member, field) in linework_code_members()
            .iter()
            .zip(&self.tabs.linework_fields)
        {
            *member.spelling_mut(&mut typed) = String::from(ascii_trimmed(field));
        }
        typed
    }

    /// CUSTOMISE SET with the control codes typed that are not the customisation's already; an
    /// empty line when all seven are.
    fn linework_codes_line(&self, document: Option<&Document>) -> Result<String> {
        let Some(document) = document else {
            return Err(Error::new(
                ErrorCode::InvalidState,
                "the drawing this manager edits is closed",
            ));
        };
        let typed = self.typed_linework_codes();
        validate(&typed)?;
        let held = &document.customisation_state().linework;
        let mut line = String::new();
        for member in linework_code_members() {
            let spelling = member.spelling(&typed);
            if spelling == member.spelling(held) {
                continue; // only what changes: the line then says what was changed
            }
            // A blank is refused above; a double quote cannot be written on a line at all, and
            // SET would read the word cut short at it.
            if spelling.contains('"') {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    "a control code cannot hold a double quote: a command line has no way to say \
                     one",
                )
                .with_context(spelling));
            }
            // An empty spelling is "linework.start=": it switches the control off.
            line.push_str(&format!(
                " linework.{}={spelling}",
                String::from_utf8_lossy(&lowered(member.name))
            ));
        }
        Ok(if line.is_empty() {
            line
        } else {
            format!("CUSTOMISE SET{line}")
        })
    }

    fn use_linework_codes(&mut self, host: &mut dyn Host) {
        let line = match self.linework_codes_line(self.live_document(host.document())) {
            Ok(line) => line,
            Err(error) => {
                self.tabs.linework_codes_status = error.describe();
                self.log(error.describe(), true);
                return;
            }
        };
        if line.is_empty() {
            self.tabs.linework_codes_status =
                String::from("These are the customisation's control codes already.");
            return;
        }
        if let Err(said) = self.run_line(host, &line) {
            self.tabs.linework_codes_status = said.clone();
            if !self.context.command_line {
                self.log(said, true);
            }
            return;
        }
        // The Document holds them now, and the fields are its: taken at once, rather than when
        // the watcher next delivers, so the tab never shows codes between the two.
        let document = self.live_document(host.document());
        self.load_linework_codes(document);
        self.tabs.linework_codes_status =
            String::from("Set: these are the customisation's control codes now.");
    }

    /// What the Linework line says beyond its scope: the property, the order and the chord
    /// length, each only when it is not the verb's default.
    fn linework_words(&self) -> LineworkWords {
        let mut words = LineworkWords {
            property: String::from(ascii_trimmed(&self.tabs.linework_property.text)),
            order: if self.tabs.linework_order == 1 {
                LineworkOrder::EntityOrder
            } else {
                LineworkOrder::PointNumber
            },
            chord: None,
        };
        // Millimetres on the form, the drawing's units - metres - on the line. Said only when it
        // is not the length the verb starts on, to a tenth of a millimetre, which is as fine as
        // the box is set.
        if (self.tabs.linework_chord - default_chord_millimetres()).abs() >= 0.05 {
            words.chord = Some(self.tabs.linework_chord / 1000.0);
        }
        words
    }

    fn linework_line(&self) -> Result<String> {
        let scope = self.tabs.linework_scope.verb_words()?;
        let words = self.linework_words();
        let mut line = format!("LINEWORK {scope}");
        if !words.property.is_empty() {
            line.push_str(" PROPERTY ");
            line.push_str(&command_word(&words.property, "Code property")?);
        }
        if words.order == LineworkOrder::EntityOrder {
            line.push_str(" ORDER entity");
        }
        if let Some(chord) = words.chord {
            line.push_str(" CHORD ");
            line.push_str(&exact_number(chord));
        }
        Ok(line)
    }

    pub(super) fn preview_linework(&mut self, document: Option<&Document>) -> bool {
        self.tabs.linework_strings.clear();
        self.tabs.linework_unplaced.clear();
        self.tabs.linework_notes.clear();
        let Some(found) = document else {
            self.tabs.linework_summary = String::from("The drawing is closed.");
            return false;
        };
        let taken = match scope_takes(document, &self.tabs.linework_scope) {
            Ok(taken) => taken,
            Err(error) => {
                self.tabs.linework_summary = error.describe();
                self.log(error.describe(), true);
                return false;
            }
        };
        // The verb's own plan (cad's plan_linework), so what is listed here is what the line then
        // draws: the customisation's control codes and colours, no point removed, the points
        // their survey job has strung left out, and a line the drawing already holds not drawn
        // again.
        let plan = match plan_linework(found, &taken, &self.linework_words()) {
            Ok(plan) => plan,
            Err(error) => {
                self.tabs.linework_summary = error.describe();
                self.log(error.describe(), true);
                return false;
            }
        };
        let report = &plan.planned.report;
        for line in &report.strings {
            self.tabs.linework_strings.push(vec![
                format!("{}{}", line.name, if line.join { " (join)" } else { "" }),
                line.key.clone(),
                format!("{}", line.point_numbers.len()),
                String::from(if line.closed { "closed" } else { "" }),
                if line.curves == 0 {
                    String::new()
                } else {
                    format!("{}", line.curves)
                },
                line.layer.clone(),
            ]);
        }
        for point in &report.unplaced {
            self.tabs.linework_unplaced.push(vec![
                point.point_number.clone(),
                point.code.clone(),
                String::from(to_string(point.reason)),
            ]);
        }
        for note in &report.notes {
            self.tabs.linework_notes.push(vec![
                note.point_number.clone(),
                String::from(to_string(note.kind)),
                note.detail.clone(),
            ]);
        }
        // What the scope took first, as the line's own reply begins; then what was left out, each
        // only when there is something to say.
        let mut summary = format!("{} matched. ", taken.len());
        summary.push_str(&if plan.planned.command.is_none() {
            format!(
                "Nothing to build: {} points looked at, {} not placed.",
                report.considered,
                report.unplaced.len()
            )
        } else {
            format!(
                "Preview only: {} lines from {} points; {} points not placed; {} notes. Execute \
                 builds them as one undoable step.",
                report.strings.len(),
                report.considered,
                report.unplaced.len(),
                report.notes.len()
            )
        });
        if !plan.strung_by_their_job.is_empty() {
            summary.push_str(&format!(
                " {} points are left to the lines their survey job drew.",
                plan.strung_by_their_job.len()
            ));
        }
        if !report.already_drawn.is_empty() {
            summary.push_str(&format!(
                " {} lines are in the drawing already and are not drawn again.",
                report.already_drawn.len()
            ));
        }
        self.tabs.linework_summary = summary;
        true
    }

    fn execute_linework(&mut self, host: &mut dyn Host) {
        if !self.alive() {
            return;
        }
        let line = match self.linework_line() {
            Ok(line) => line,
            Err(error) => {
                self.tabs.linework_summary = error.describe();
                self.log(error.describe(), true);
                return;
            }
        };
        // The lines about to be drawn, point by point: the reply counts them. A preview that
        // could not plan has said why; the line is not run to say it twice.
        let document = self.live_document(host.document());
        if !self.preview_linework(document) {
            return;
        }
        let outcome = match self.run_line(host, &line) {
            Ok(outcome) => outcome,
            Err(said) => {
                self.tabs.linework_summary = said.clone();
                if !self.context.command_line {
                    self.log(said, true);
                }
                return;
            }
        };
        // From the verb's own first record - "linework scope=... lines=<n> ..." - and never from
        // the plan above alone: what is said to have been drawn is what the line says it drew.
        let first = outcome.reply.split('\n').next().unwrap_or_default();
        let lines = read_reply_record(first)
            .and_then(|record| record.value("lines").map(String::from));
        self.tabs.linework_summary = match lines.as_deref() {
            None => format!("Executed. {first}"),
            Some("0") => format!(
                "Executed: no line was drawn, so nothing was added to the undo history. {first}"
            ),
            Some(lines) => {
                format!("Executed as one undoable step: {lines} lines drawn. {first}")
            }
        };
    }

    /// The Linework tab's three report tables: the lines, the points not placed, the notes.
    pub fn linework_strings(&self) -> &[Vec<String>] {
        &self.tabs.linework_strings
    }

    // ---- what the frame's drawing asked for ----------------------------------------------------

    pub(super) fn react_further(&mut self, host: &mut dyn Host, event: Event) {
        match event {
            Event::FormEdited | Event::SectionChosen => {
                let document = self.live_document(host.document());
                self.update_form_issues(document);
            }
            Event::FormNew
            | Event::FormAdd
            | Event::FormUpdate
            | Event::FormDuplicate
            | Event::FormDelete
            | Event::FormUp
            | Event::FormDown => self.form_pressed(host, &event),
            Event::CensusChanged => {
                let document = self.live_document(host.document());
                self.refresh_census(document);
            }
            Event::CensusFilter => self.filter_census(),
            Event::CensusChosen(row) => {
                if let Some(code) = self.tabs.census_rows.get(row).and_then(|row| row.first()) {
                    self.tabs.census_selected = code.clone();
                }
            }
            Event::CensusActivated(row) => self.activate_census_row(row),
            Event::CensusSelect => self.select_entities_with_code(host),
            Event::CensusNewRule => {
                let code = self.tabs.census_selected.clone();
                if code.is_empty() {
                    self.log("Choose a code in the list first.", true);
                } else {
                    let document = self.live_document(host.document());
                    self.new_rule_from_code_in(document, &code);
                }
            }
            Event::CensusRun => self.run_census_line(host),
            Event::IssuesFilter => {
                let document = self.live_document(host.document());
                self.rebuild_issues(document);
            }
            Event::IssueActivated(row) => self.activate_issue(row),
            Event::LinesChanged => self.show_lines(),
            Event::ApplyPreview => {
                let document = self.live_document(host.document());
                self.preview_codes(document);
            }
            Event::ApplyExecute => self.execute_codes(host),
            Event::LineworkCodeTyped => self.linework_codes_typed(),
            Event::LineworkUse => self.use_linework_codes(host),
            Event::LineworkDefaults => {
                let standard = LineworkCodes::default();
                for (at, member) in linework_code_members().iter().enumerate() {
                    self.set_linework_field(at, member.spelling(&standard));
                }
            }
            Event::LineworkPreview => {
                let document = self.live_document(host.document());
                self.preview_linework(document);
            }
            Event::LineworkExecute => self.execute_linework(host),
            // The frame's own and the Code Table's are code_manager's.
            _ => {}
        }
    }

    // ---- the tabs' pages -----------------------------------------------------------------------

    pub(super) fn census_tab(&mut self, ui: &mut Ui) {
        let tabs = &mut self.tabs;
        let events = &mut self.events;
        ui.horizontal_top(|ui| {
            // The shared "Apply to" and "Only those that match" controls: the census counts the
            // codes of what they take, as CODE CENSUS <scope> does.
            ui.vertical(|ui| {
                if tabs.census_scope.ui(ui) {
                    events.push(Event::CensusChanged);
                }
            });
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label("Code property");
                    if controls::choice(
                        ui,
                        "censusProperty",
                        &tabs.property_items,
                        &mut tabs.census_property,
                    )
                    .changed()
                    {
                        events.push(Event::CensusChanged);
                    }
                });
                if nested_filter(ui, "censusFilter", &mut tabs.census_filter).any() {
                    events.push(Event::CensusFilter);
                }
                let current = tabs
                    .census_rows
                    .iter()
                    .position(|row| row[0] == tabs.census_selected);
                let shown = table(
                    ui,
                    "censusTable",
                    &["Code", "Entities", "Class", "Layer"],
                    &tabs.census_rows,
                    current,
                    &[],
                    None,
                );
                if let Some(row) = shown.chosen {
                    events.push(Event::CensusChosen(row));
                }
                // A double-click shows what the code gets, as Test a Code would.
                if let Some(row) = shown.double_clicked {
                    events.push(Event::CensusActivated(row));
                }
                controls::label_as(
                    ui,
                    "censusSummary",
                    &tabs.census_summary,
                    egui::Label::new(tabs.census_summary.as_str()).wrap(),
                );
                command_field(
                    ui,
                    "censusCommand",
                    &tabs.census_command,
                    &tabs.census_command_hint,
                );
                ui.horizontal(|ui| {
                    if controls::button(ui, "censusSelect", "Select Entities With Code").clicked()
                    {
                        events.push(Event::CensusSelect);
                    }
                    if controls::button(ui, "censusNewRule", "New Rule From Code").clicked() {
                        events.push(Event::CensusNewRule);
                    }
                    if controls::button(ui, "censusRun", "Run Line")
                        .on_hover_text(
                            "Hand the line above to the command line: it answers with the same \
                             census, counted against the drawing's own survey map",
                        )
                        .clicked()
                    {
                        events.push(Event::CensusRun);
                    }
                    if controls::button(ui, "censusRefresh", "Count Again").clicked() {
                        events.push(Event::CensusChanged);
                    }
                });
            });
        });
    }

    pub(super) fn issues_tab(&mut self, ui: &mut Ui) {
        let tabs = &mut self.tabs;
        if nested_filter(ui, "issuesFilter", &mut tabs.issues_filter).any() {
            self.events.push(Event::IssuesFilter);
        }
        let shown = table(
            ui,
            "issuesTable",
            &["Severity", "Rule", "Key", "Section", "Problem", "Message"],
            &tabs.issues_rows,
            None,
            &[],
            None,
        );
        if let Some(row) = shown.double_clicked {
            self.events.push(Event::IssueActivated(row));
        }
        controls::label(ui, "issuesSummary", &tabs.issues_summary);
    }

    pub(super) fn apply_tab(&mut self, ui: &mut Ui) {
        let edited = self.dirty();
        let tabs = &mut self.tabs;
        let events = &mut self.events;
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                if tabs.apply_scope.ui(ui) {
                    events.push(Event::LinesChanged);
                }
            });
            ui.vertical(|ui| {
                ui.group(|ui| {
                    ui.label("Apply the drawing's survey map");
                    ui.horizontal(|ui| {
                        ui.label("Code property");
                        if controls::choice(
                            ui,
                            "applyProperty",
                            &tabs.property_items,
                            &mut tabs.apply_property,
                        )
                        .changed()
                        {
                            events.push(Event::LinesChanged);
                        }
                    });
                });
                dirty_note(
                    ui,
                    "applyDirtyNote",
                    "The rules being edited are not applied yet: codes are applied with the \
                     drawing's survey map. Apply the edits first to use them.",
                    edited,
                );
                command_field(
                    ui,
                    "applyCommand",
                    &tabs.apply_command,
                    &tabs.apply_command_hint,
                );
                ui.horizontal(|ui| {
                    if controls::button(ui, "applyPreview", "Preview")
                        .on_hover_text(
                            "Work out what applying the codes would do, changing nothing",
                        )
                        .clicked()
                    {
                        events.push(Event::ApplyPreview);
                    }
                    if controls::button(ui, "applyExecute", "Execute")
                        .on_hover_text(
                            "Run the line above: the codes applied as one undoable step",
                        )
                        .clicked()
                    {
                        events.push(Event::ApplyExecute);
                    }
                });
                table(
                    ui,
                    "applyRows",
                    &["Code", "Entities", "Class", "Layer", "Style", "Changed"],
                    &tabs.apply_rows,
                    None,
                    &[],
                    None,
                );
                let mut report = tabs.apply_report.clone();
                controls::text_with(
                    ui,
                    "applyReport",
                    &mut report,
                    TextOptions {
                        read_only: true,
                        rows: 6,
                        monospace: false,
                    },
                );
            });
        });
    }

    pub(super) fn linework_tab(&mut self, ui: &mut Ui) {
        let edited = self.dirty();
        let tabs = &mut self.tabs;
        let events = &mut self.events;
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                if tabs.linework_scope.ui(ui) {
                    events.push(Event::LinesChanged);
                }
            });
            ui.vertical(|ui| {
                ui.horizontal_top(|ui| {
                    ui.group(|ui| {
                        ui.vertical(|ui| {
                            ui.label("Control codes");
                            // One field a control, in the order the controls are declared in:
                            // the fields are read back by that order. The object name is
                            // "linework" and the control's name capitalised (arcStart:
                            // lineworkArcStart).
                            for (member, field) in linework_code_members()
                                .iter()
                                .zip(&mut tabs.linework_fields)
                            {
                                let mut letters = member.name.chars();
                                let name = match letters.next() {
                                    Some(initial) => format!(
                                        "linework{}{}",
                                        initial.to_uppercase(),
                                        letters.as_str()
                                    ),
                                    None => String::from("linework"),
                                };
                                ui.horizontal(|ui| {
                                    ui.label(linework_control_label(member.name));
                                    if controls::line(ui, &name, field)
                                        .on_hover_text(
                                            "Typed after the code, matched ignoring case; empty \
                                             switches it off",
                                        )
                                        .changed()
                                    {
                                        events.push(Event::LineworkCodeTyped);
                                    }
                                });
                            }
                            let mut status = RichText::new(tabs.linework_codes_status.as_str());
                            if tabs.linework_codes_bad {
                                status = status.color(REFUSAL_COLOUR);
                            }
                            controls::label_as(
                                ui,
                                "lineworkCodesStatus",
                                &tabs.linework_codes_status,
                                egui::Label::new(status).wrap(),
                            );
                            ui.horizontal(|ui| {
                                ui.add_enabled_ui(tabs.linework_codes_use, |ui| {
                                    if controls::button(ui, "lineworkCodesUse", "Use These Codes")
                                        .on_hover_text(
                                            "Make these the customisation's control codes (the \
                                             line CUSTOMISE SET linework...): every import and \
                                             every LINEWORK then reads them",
                                        )
                                        .clicked()
                                    {
                                        events.push(Event::LineworkUse);
                                    }
                                });
                                if controls::button(ui, "lineworkCodesDefaults", "Defaults")
                                    .on_hover_text(
                                        "Fill in the usual spellings; Use These Codes then sets \
                                         them",
                                    )
                                    .clicked()
                                {
                                    events.push(Event::LineworkDefaults);
                                }
                            });
                        });
                    });
                    ui.group(|ui| {
                        ui.vertical(|ui| {
                            ui.label("Options");
                            ui.horizontal(|ui| {
                                ui.label("Code property");
                                if controls::choice(
                                    ui,
                                    "lineworkProperty",
                                    &tabs.property_items,
                                    &mut tabs.linework_property,
                                )
                                .changed()
                                {
                                    events.push(Event::LinesChanged);
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.label("Join in order");
                                let orders = [
                                    String::from("By point number"),
                                    String::from("In the order observed"),
                                ];
                                if index_choice(
                                    ui,
                                    "lineworkOrder",
                                    &orders,
                                    &mut tabs.linework_order,
                                ) {
                                    events.push(Event::LinesChanged);
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.label("Chord tolerance");
                                if controls::spin(
                                    ui,
                                    "lineworkChordTolerance",
                                    &mut tabs.linework_chord,
                                    CHORD,
                                )
                                .on_hover_text(
                                    "How far a curve's chords may stray from the arc, for a \
                                     drawing in metres",
                                )
                                .changed()
                                {
                                    events.push(Event::LinesChanged);
                                }
                            });
                        });
                    });
                });
                dirty_note(
                    ui,
                    "lineworkDirtyNote",
                    "The rules being edited are not applied yet: lines are made with the \
                     drawing's survey map.",
                    edited,
                );
                command_field(
                    ui,
                    "lineworkCommand",
                    &tabs.linework_command,
                    &tabs.linework_command_hint,
                );
                ui.horizontal(|ui| {
                    if controls::button(ui, "lineworkPreview", "Preview")
                        .on_hover_text("Work out the lines, changing nothing")
                        .clicked()
                    {
                        events.push(Event::LineworkPreview);
                    }
                    if controls::button(ui, "lineworkExecute", "Execute")
                        .on_hover_text(
                            "Run the line above: the lines drawn as one undoable step",
                        )
                        .clicked()
                    {
                        events.push(Event::LineworkExecute);
                    }
                    controls::label_as(
                        ui,
                        "lineworkSummary",
                        &tabs.linework_summary,
                        egui::Label::new(tabs.linework_summary.as_str()).wrap(),
                    );
                });
                controls::tabs(
                    ui,
                    "lineworkReport",
                    &["Lines", "Points not placed", "Notes"],
                    &mut tabs.linework_report_tab,
                );
                let reports: [(&str, &[&str], &Vec<Vec<String>>); 3] = [
                    (
                        "lineworkStrings",
                        &["String", "Key", "Points", "Closed", "Curves", "Layer"],
                        &tabs.linework_strings,
                    ),
                    (
                        "lineworkUnplaced",
                        &["Point", "Code", "Why not placed"],
                        &tabs.linework_unplaced,
                    ),
                    (
                        "lineworkNotes",
                        &["Point", "Note", "Detail"],
                        &tabs.linework_notes,
                    ),
                ];
                for (at, (name, headers, rows)) in reports.into_iter().enumerate() {
                    if at == tabs.linework_report_tab {
                        table(ui, name, headers, rows, None, &[], None);
                    } else {
                        offstage(ui, name, |ui| {
                            table(ui, name, headers, rows, None, &[], None);
                        });
                    }
                }
            });
        });
    }
}
