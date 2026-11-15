//! The Survey menu's point tools: Export Survey Points, the Point Manager dock and the Point
//! Report (survey_points_ui.hpp/.cpp).
//!
//! As with every survey dialog, nothing survey-specific is computed here: the points come from
//! katana_cad's drawing_survey_points / survey_points_among, the file is written by surveyio's
//! delimited writer, the report text by format_point_report and point_report_csv.
//!
//! Object names (the headless --dialog and --survey-dock switches use them):
//!
//! ```text
//! surveyExportDialog       fields  file scope template preset columns delimiter header quoting
//!                                  comment decimals unit
//!                          buttons browse export
//! SurveyPointsDock         fields  filter
//!                          shown   points (the table) status
//! surveyPointReportDialog  shown   scope report
//!                          buttons refresh copy saveCsv close
//! ```

use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::path::PathBuf;

use egui::{Context, Response, Ui};
use katana_cad::SurveyImportOptions;
use katana_cad::survey_points::{
    DrawingSurveyPoint, drawing_survey_points, format_point_report, point_report_csv,
    survey_points_among, to_survey_point,
};
use katana_core::error::make_error;
use katana_core::path_text::{path_from_utf8, path_to_utf8};
use katana_core::text::parse_integer;
use katana_core::{Error, ErrorCode, Result, Status};
use katana_entity::EntityId;
use katana_geometry::{Box2, Point2};
use katana_survey::LinearUnit;
use katana_survey::data_model::to_string as unit_name;
use katana_surveyio::{
    ColumnRole, DelimitedExportOptions, DelimitedLayout, Delimiter, MAXIMUM_EXPORT_DECIMALS,
    Quoting, all_column_presets, column_preset_named, layout_template, parse_layout_template,
    preset_columns, template_letter, to_string as surveyio_name, write_delimited_points,
};

use super::survey_dialogs::{
    SurveyToolFrame, ToolButton, choice_of, coloured_label, field, form_row, muted, no_file_dialog,
    save_file_dialog,
};
use super::survey_templates::{TemplateChoice, saved_layout_templates};
use crate::drive::mailbox::{lists_no, no_button};
use crate::drive::{self, Act, Choice, ControlKind, ControlRecord, TextOptions};
use crate::format::qt_fixed;
use crate::host::Host;
use crate::shell::DialogFrame;
use crate::theme::palette;

/// The units a file is written in, in the Write In choice's order.
const UNITS: [LinearUnit; 4] = [
    LinearUnit::Metres,
    LinearUnit::Feet,
    LinearUnit::UsSurveyFeet,
    LinearUnit::Links,
];

/// The delimiters, in the Delimiter choice's order.
const DELIMITERS: [Delimiter; 4] = [
    Delimiter::Comma,
    Delimiter::Tab,
    Delimiter::Semicolon,
    Delimiter::Whitespace,
];

/// The quotes, in the Quotes choice's order.
const QUOTINGS: [Quoting; 2] = [Quoting::DoubleQuote, Quoting::None];

/// The margin round a point the Point Manager zooms to, in drawing units (metres): a view of
/// about 20 m across shows the point and its neighbours on a detail survey. Policy, not a
/// measurement.
const ZOOM_HALF_WIDTH: f64 = 10.0;

/// A file's name, for a sentence: QFileInfo::fileName.
pub(crate) fn file_name_of(path: &str) -> String {
    path_from_utf8(path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// All or nothing: a half-written point file is worse than none, because it reads without an
/// error (the C++'s QSaveFile: written beside, then put in place).
fn write_file(path: &str, bytes: &[u8]) -> Status {
    let target = path_from_utf8(path);
    let mut beside = target.clone().into_os_string();
    beside.push(".katana-saving");
    let beside = PathBuf::from(beside);
    let written = std::fs::write(&beside, bytes).and_then(|()| std::fs::rename(&beside, target));
    if let Err(error) = written {
        let _ = std::fs::remove_file(beside);
        return Err(make_error(
            ErrorCode::FileExportFailure,
            "the file could not be written",
            format!("{path}: {error}"),
        ));
    }
    Ok(())
}

/// The columns of a layout as a template spells them: "P,N,E,Z,D".
pub(crate) fn column_letters(columns: &[ColumnRole]) -> String {
    columns
        .iter()
        .map(|role| String::from(template_letter(*role)))
        .collect::<Vec<_>>()
        .join(",")
}

fn names_of<T: Copy + katana_surveyio::Named>(values: &[T]) -> Vec<&'static str> {
    values.iter().map(|value| surveyio_name(*value)).collect()
}

// ---- export ----------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
struct ExportForm {
    file: String,
    scope: Choice,
    template: TemplateChoice,
    /// The saved templates, read for this frame.
    templates: Vec<(String, String)>,
    /// The template the person chose this frame, to apply.
    template_chosen: Option<String>,
    preset: Choice,
    preset_chosen: bool,
    columns: String,
    delimiter: Choice,
    header: bool,
    quoting: Choice,
    comment: String,
    decimals: String,
    unit: Choice,
}

/// Survey > Export Survey Points... (surveyExportDialog).
pub struct SurveyExportDialog {
    pub tool: SurveyToolFrame,
    form: ExportForm,
}

impl Default for SurveyExportDialog {
    fn default() -> Self {
        Self::new()
    }
}

impl SurveyExportDialog {
    pub fn new() -> Self {
        Self {
            tool: SurveyToolFrame::new(
                "surveyExportDialog",
                "Export Survey Points",
                "Writes the drawing's survey points - point entities carrying a point number - \
                 to a delimited text file with the layout below. A point with no elevation is \
                 written with an empty field, never 0. Drawing coordinates are taken as metres \
                 (what the import writes); another unit converts on the way out.",
            ),
            form: ExportForm {
                file: String::new(),
                scope: Choice::at(0),
                template: TemplateChoice::new(),
                templates: Vec::new(),
                template_chosen: None,
                preset: Choice::at(0),
                preset_chosen: false,
                columns: String::from("P,E,N,Z,C,D"),
                delimiter: Choice::at(0),
                header: true,
                quoting: Choice::at(0),
                comment: String::new(),
                decimals: String::from("3"),
                unit: Choice::at(0),
            },
        }
    }

    pub fn show(&mut self) {
        self.tool.open = true;
    }

    pub fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        if !self.tool.open {
            return;
        }
        // Read at every frame: a template saved or deleted in the import wizard - while this
        // dialog is open or hidden - is offered here at once, or no longer.
        self.form.templates = saved_layout_templates(host.settings());
        self.form.template_chosen = None;
        self.form.preset_chosen = false;
        let pressed = self.tool.show(
            ui,
            &mut self.form,
            Self::draw_form,
            &[ToolButton::new("browse", "Browse...")],
            &[ToolButton::new("export", "Export")],
        );
        if let Some(text) = self.form.template_chosen.take() {
            match parse_layout_template(&text) {
                Err(error) => self.tool.set_note(&error.describe()),
                Ok(layout) => self.apply_layout(&layout),
            }
        }
        if self.form.preset_chosen
            && let Some(index) = self.form.preset.index.filter(|index| *index > 0)
            && let Some(preset) = all_column_presets().get(index - 1)
            && let Ok(preset) = column_preset_named(surveyio_name(*preset))
        {
            self.form.columns = column_letters(&preset_columns(preset));
        }
        match pressed {
            Some("browse") => {
                self.tool.pane.message.clear();
                let status = self.browse(host);
                self.tool.filler_done(host, status);
            }
            Some("export") => {
                let report = self.export_points(host);
                self.tool.verb_done(host, report);
            }
            _ => {}
        }
    }

    fn draw_form(ui: &mut Ui, form: &mut ExportForm) {
        form_row(ui, "File:", |ui| {
            field(
                ui,
                "file",
                &mut form.file,
                "the file to write",
                "The file to write; it is replaced whole",
            )
        });
        form_row(ui, "Points:", |ui| {
            choice_of(
                ui,
                "scope",
                &["All survey points", "Selected survey points"],
                &mut form.scope,
            )
            .on_hover_text("Which survey points to write")
        });
        form_row(ui, "Saved template:", |ui| {
            let (_, chosen) = form.template.ui(ui, "template", &form.templates);
            form.template_chosen = chosen;
        });
        let mut presets = vec!["(presets)"];
        presets.extend(names_of(all_column_presets()));
        form_row(ui, "Preset:", |ui| {
            form.preset_chosen = choice_of(ui, "preset", &presets, &mut form.preset)
                .on_hover_text("A column order named by its letters")
                .changed();
        });
        form_row(ui, "Columns:", |ui| {
            field(
                ui,
                "columns",
                &mut form.columns,
                "P,E,N,Z,D",
                "One letter per column: P point id, N northing, E easting, Z elevation, C code, \
                 D description, - an empty column",
            )
        });
        form_row(ui, "Delimiter:", |ui| {
            choice_of(ui, "delimiter", &names_of(&DELIMITERS), &mut form.delimiter)
                .on_hover_text("What separates the fields")
        });
        drive::check(
            ui,
            "header",
            "A header line naming the columns",
            &mut form.header,
        )
        .on_hover_text("With it, the import wizard reads the column order back without asking");
        form_row(ui, "Quotes:", |ui| {
            choice_of(ui, "quoting", &names_of(&QUOTINGS), &mut form.quoting)
                .on_hover_text("Double quotes round a field that needs them (RFC 4180), or none")
        });
        form_row(ui, "Comment prefix:", |ui| {
            field(
                ui,
                "comment",
                &mut form.comment,
                "none",
                "A field that would start a line with it is quoted",
            )
        });
        form_row(ui, "Decimals:", |ui| {
            field(
                ui,
                "decimals",
                &mut form.decimals,
                "0 to 12",
                "Places after the point",
            )
        });
        let units: Vec<&str> = UNITS.iter().map(|unit| unit_name(*unit)).collect();
        form_row(ui, "Write in:", |ui| {
            choice_of(ui, "unit", &units, &mut form.unit)
                .on_hover_text("The unit the coordinates are written in")
        });
    }

    fn apply_layout(&mut self, layout: &DelimitedLayout) {
        let form = &mut self.form;
        form.columns = column_letters(&layout.columns);
        form.delimiter = Choice::at(
            DELIMITERS
                .iter()
                .position(|delimiter| *delimiter == layout.delimiter)
                .unwrap_or(0),
        );
        form.header = layout.header_lines != 0;
        form.quoting = Choice::at(
            QUOTINGS
                .iter()
                .position(|quoting| *quoting == layout.quoting)
                .unwrap_or(0),
        );
        form.comment.clone_from(&layout.comment_prefix);
    }

    fn browse(&mut self, host: &mut dyn Host) -> Status {
        if host.headless() {
            return Err(no_file_dialog());
        }
        if let Some(path) = save_file_dialog(
            "Export Survey Points",
            &self.form.file,
            &[("CSV", &["csv"]), ("Text", &["txt"]), ("All", &["*"])],
        ) {
            self.form.file = String::from_utf8_lossy(&path_to_utf8(&path)).into_owned();
        }
        Ok(())
    }

    fn export_points(&self, host: &mut dyn Host) -> Result<String> {
        let form = &self.form;
        let path = form.file.trim();
        if path.is_empty() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "choose the file to write",
            ));
        }
        // surveyio's own template text, read by its own parser: the export takes exactly the
        // layouts a saved template may hold.
        let mut text = format!(
            "{};delimiter={};header={};quote={}",
            form.columns.trim(),
            surveyio_name(DELIMITERS[form.delimiter.index.unwrap_or(0).min(3)]),
            if form.header { "1" } else { "0" },
            surveyio_name(QUOTINGS[form.quoting.index.unwrap_or(0).min(1)])
        );
        if !form.comment.trim().is_empty() {
            text.push_str(";comment=");
            text.push_str(form.comment.trim());
        }
        let layout = parse_layout_template(&text)?;
        let decimals = match parse_integer(form.decimals.trim()) {
            Some(decimals) if (0..=i64::from(MAXIMUM_EXPORT_DECIMALS)).contains(&decimals) => {
                decimals
            }
            _ => {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "decimals are a whole number from 0 to 12",
                    form.decimals.as_str(),
                ));
            }
        };

        let document = host.document();
        let selected = form.scope.index == Some(1);
        let mut not_points = 0;
        let points: Vec<DrawingSurveyPoint> = if selected {
            let ids = document.selection().ids();
            if ids.is_empty() {
                return Err(Error::new(
                    ErrorCode::InvalidState,
                    "nothing is selected - select survey points, or write all of them",
                ));
            }
            let pick = survey_points_among(document, &ids, &SurveyImportOptions::default());
            not_points = pick.not_survey_points;
            pick.points
        } else {
            drawing_survey_points(document, &SurveyImportOptions::default())
        };
        if points.is_empty() {
            return Err(Error::new(
                ErrorCode::InvalidState,
                if selected {
                    "none of the selection is a survey point (a point entity with a point number)"
                } else {
                    "the drawing has no survey points (point entities with a point number)"
                },
            ));
        }
        let without_elevation = points
            .iter()
            .filter(|point| point.elevation.is_none())
            .count();
        let survey_points: Vec<_> = points.iter().map(to_survey_point).collect();
        let options = DelimitedExportOptions {
            decimals: decimals as i32,
            unit: UNITS[form.unit.index.unwrap_or(0).min(UNITS.len() - 1)],
        };
        let bytes = write_delimited_points(&survey_points, &layout, &options)?;
        write_file(path, bytes.as_bytes())?;
        let mut report = format!(
            "Exported {} point(s) to {} - layout {}, {decimals} decimals, in {}.",
            points.len(),
            file_name_of(path),
            layout_template(&layout),
            unit_name(options.unit)
        );
        if without_elevation != 0 {
            report.push_str(&format!(
                "\n{without_elevation} point(s) have no elevation and were written with an \
                 empty field."
            ));
        }
        if not_points != 0 {
            report.push_str(&format!(
                "\n{not_points} selected entities are not survey points and were not written."
            ));
        }
        Ok(report)
    }
}

// ---- the Point Manager -------------------------------------------------------------------------

/// The table's columns.
const POINT_COLUMNS: [&str; 7] = [
    "Point",
    "Code",
    "Easting",
    "Northing",
    "Elevation",
    "Description",
    "Source file",
];

/// A point id as a person reads a list of them: 2 before 10, and "CP2" before "CP10" (a
/// collator's numeric mode), where a plain text sort puts 10, 11 and 12 between 1 and 2.
fn natural_order(a: &str, b: &str) -> Ordering {
    fn runs(text: &str) -> Vec<(bool, String)> {
        let mut runs: Vec<(bool, String)> = Vec::new();
        for character in text.chars() {
            let digit = character.is_ascii_digit();
            match runs.last_mut() {
                Some((was_digit, run)) if *was_digit == digit => run.push(character),
                _ => runs.push((digit, String::from(character))),
            }
        }
        runs
    }
    let (left, right) = (runs(a), runs(b));
    for ((left_digit, left_run), (right_digit, right_run)) in left.iter().zip(&right) {
        let order = if *left_digit && *right_digit {
            let left_value = left_run.trim_start_matches('0');
            let right_value = right_run.trim_start_matches('0');
            left_value
                .len()
                .cmp(&right_value.len())
                .then_with(|| left_value.cmp(right_value))
        } else {
            left_run
                .to_lowercase()
                .cmp(&right_run.to_lowercase())
                .then_with(|| left_run.cmp(right_run))
        };
        if order != Ordering::Equal {
            return order;
        }
    }
    left.len().cmp(&right.len())
}

/// A number in a cell that sorts as a number: a blank (an absent elevation) before every number
/// rather than as zero.
fn number_order(a: Option<f64>, b: Option<f64>) -> Ordering {
    match (a, b) {
        (Some(a), Some(b)) => a.partial_cmp(&b).unwrap_or(Ordering::Equal),
        (None, Some(_)) => Ordering::Less,
        (Some(_), None) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn fixed3(value: f64) -> String {
    qt_fixed(value, 3)
}

fn cells_of(point: &DrawingSurveyPoint) -> Vec<String> {
    vec![
        point.id.clone(),
        point.code.clone(),
        fixed3(point.easting),
        fixed3(point.northing),
        point.elevation.map(fixed3).unwrap_or_default(),
        point.description.clone(),
        point.source_file.clone(),
    ]
}

/// Survey > Point Manager: the drawing's survey points in a table - filter box, sortable columns,
/// rows selecting their entities, a double-click zooming to the point. READ-ONLY. Editing would
/// need each change to go through a cad command (move the point entity, rewrite its number, code
/// or height property) so that it is undoable, and a rule for an edited id that another point
/// already has - the import's ExistingPointPolicy question asked of one cell.
///
/// The C++ rebuilt the table from a Document listener on the event loop; here the table is read
/// again when the drawing's model revision moves, and a selection - which moves no revision - is
/// only mirrored onto the rows, at every frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SurveyPointsDock {
    filter: String,
    points: Vec<DrawingSurveyPoint>,
    /// `points` has been read at least once.
    listed: bool,
    revision: Option<u64>,
    /// The rows in the order shown: indices into `points`.
    order: Vec<usize>,
    /// The column sorted by and whether ascending; None: the drawing's own order (the order the
    /// points were imported in), until a header is clicked.
    sort: Option<(usize, bool)>,
    status: String,
}

impl SurveyPointsDock {
    pub fn new() -> Self {
        Self::default()
    }

    /// Points listed, and shown under the filter: "12 points, 3 shown".
    pub fn status(&self) -> &str {
        &self.status
    }

    fn sort_rows(&mut self) {
        self.order = (0..self.points.len()).collect();
        let Some((column, ascending)) = self.sort else {
            return;
        };
        let points = &self.points;
        let compare = |a: &usize, b: &usize| {
            let (a, b) = (&points[*a], &points[*b]);
            match column {
                0 => natural_order(&a.id, &b.id),
                1 => a.code.cmp(&b.code),
                2 => number_order(Some(a.easting), Some(b.easting)),
                3 => number_order(Some(a.northing), Some(b.northing)),
                4 => number_order(a.elevation, b.elevation),
                5 => a.description.cmp(&b.description),
                _ => a.source_file.cmp(&b.source_file),
            }
        };
        // Stable, as the table's own sort is: equal rows keep the drawing's order.
        if ascending {
            self.order.sort_by(compare);
        } else {
            self.order.sort_by(|a, b| compare(b, a));
        }
    }

    fn matches(&self, point: &DrawingSurveyPoint) -> bool {
        let needle = self.filter.trim().to_lowercase();
        needle.is_empty()
            || [&point.id, &point.code, &point.description]
                .iter()
                .any(|cell| cell.to_lowercase().contains(&needle))
    }

    /// Draws the dock's content (its target, title and chrome are the window's).
    pub fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        let document = host.document();
        let revision = document.model_revision();
        if self.revision != Some(revision) {
            self.revision = Some(revision);
            let points = drawing_survey_points(document, &SurveyImportOptions::default());
            if !self.listed || points != self.points {
                self.points = points;
                self.listed = true;
                self.sort_rows();
            }
        }
        let selected: BTreeSet<EntityId> = self
            .points
            .iter()
            .filter(|point| document.selection().contains(point.entity))
            .map(|point| point.entity)
            .collect();
        let shown_count = self
            .points
            .iter()
            .filter(|point| self.matches(point))
            .count();
        self.status = format!("{} points, {shown_count} shown", self.points.len());

        let rows: Vec<Vec<String>> = self
            .order
            .iter()
            .map(|at| cells_of(&self.points[*at]))
            .collect();
        let mut chosen: Option<BTreeSet<EntityId>> = None;
        let mut zoom: Option<usize> = None;
        let mut sort_by: Option<usize> = None;

        // An act a driver posted to the table: a fill selects the row whose first cell is the
        // text, alone, as a click on it does (fillField's ClearAndSelect of whole rows).
        if let Some(taken) = drive::take(ui, "points") {
            match &taken.posted.act {
                Act::Fill(text) => match rows.iter().position(|row| row[0] == *text) {
                    Some(row) => {
                        chosen = Some(BTreeSet::from([self.points[self.order[row]].entity]));
                        drive::settle(ui, &taken, Ok(()));
                    }
                    None => drive::settle(ui, &taken, Err(lists_no("points", text))),
                },
                Act::Press => drive::settle(ui, &taken, Err(no_button(&taken.posted))),
                Act::ScrollTo => drive::settle(ui, &taken, Ok(())),
            }
        }

        ui.horizontal(|ui| {
            field(
                ui,
                "filter",
                &mut self.filter,
                "Filter by point, code or description",
                "",
            );
            drive::label(ui, "status", &self.status);
        });
        let table = egui::ScrollArea::both()
            .id_salt("surveyPointsTable")
            .max_height(ui.available_height().max(120.0))
            .show(ui, |ui| {
                egui::Grid::new("surveyPointsGrid")
                    .striped(true)
                    .show(ui, |ui| {
                        for (column, title) in POINT_COLUMNS.iter().enumerate() {
                            let marked = match self.sort {
                                Some((sorted, true)) if sorted == column => {
                                    format!("{title} \u{25b2}")
                                }
                                Some((sorted, false)) if sorted == column => {
                                    format!("{title} \u{25bc}")
                                }
                                _ => String::from(*title),
                            };
                            if ui.button(marked).clicked() {
                                sort_by = Some(column);
                            }
                        }
                        ui.end_row();
                        let extend =
                            ui.input(|input| input.modifiers.command || input.modifiers.ctrl);
                        for (row, cells) in rows.iter().enumerate() {
                            let at = self.order[row];
                            let point = &self.points[at];
                            if !self.matches(point) {
                                continue;
                            }
                            let is_selected = selected.contains(&point.entity);
                            for cell in cells {
                                let response: Response = ui.selectable_label(is_selected, cell);
                                if response.double_clicked() {
                                    zoom = Some(at);
                                }
                                if response.clicked() {
                                    let mut ids = if extend {
                                        selected.clone()
                                    } else {
                                        BTreeSet::new()
                                    };
                                    if extend && is_selected {
                                        ids.remove(&point.entity);
                                    } else {
                                        ids.insert(point.entity);
                                    }
                                    chosen = Some(ids);
                                }
                            }
                            ui.end_row();
                        }
                    });
            });
        let mut record = ControlRecord::new("points", ControlKind::Rows);
        record.items = rows.iter().map(|row| row.join(" | ")).collect();
        record.value = self
            .order
            .iter()
            .map(|at| &self.points[*at])
            .find(|point| selected.contains(&point.entity))
            .map(|point| point.id.clone())
            .unwrap_or_default();
        let response = ui.interact(
            table.inner_rect,
            ui.id().with("surveyPointsRecord"),
            egui::Sense::hover(),
        );
        drive::custom(ui, &response, record);
        muted(ui, "Read-only: edit a point with the drawing's tools.");

        if let Some(column) = sort_by {
            self.sort = Some(match self.sort {
                Some((sorted, ascending)) if sorted == column => (column, !ascending),
                _ => (column, true),
            });
            self.sort_rows();
        }
        if let Some(ids) = chosen {
            let document = host.document_mut();
            document.selection_mut().set(ids.into_iter().collect());
            document.notify_selection_changed();
        }
        if let Some(at) = zoom {
            let point = &self.points[at];
            let (east, north) = (point.easting, point.northing);
            if let Some(views) = host.views() {
                views.zoom_to(&Box2::new(
                    Point2::new(east - ZOOM_HALF_WIDTH, north - ZOOM_HALF_WIDTH),
                    Point2::new(east + ZOOM_HALF_WIDTH, north + ZOOM_HALF_WIDTH),
                ));
            }
        }
    }
}

// ---- the Point Report --------------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
struct ReportForm {
    scope: String,
    report: String,
    pressed: Option<&'static str>,
}

/// Survey > Point Report...: the points - the selected survey points when any are selected, all
/// of them otherwise - as a table of text, with Copy and Save As CSV. Non-modal, rebuilt each
/// time it is shown or refreshed.
pub struct SurveyPointReportDialog {
    frame: DialogFrame,
    open: bool,
    /// Shown since it was last refreshed (the C++'s showEvent).
    refresh_pending: bool,
    form: ReportForm,
    points: Vec<DrawingSurveyPoint>,
}

impl Default for SurveyPointReportDialog {
    fn default() -> Self {
        Self::new()
    }
}

impl SurveyPointReportDialog {
    pub fn new() -> Self {
        Self {
            frame: DialogFrame::new("surveyPointReportDialog", "Point Report"),
            open: false,
            refresh_pending: false,
            form: ReportForm::default(),
            points: Vec::new(),
        }
    }

    /// Shows the dialog, which refreshes itself as it is shown.
    pub fn show(&mut self) {
        self.open = true;
        self.refresh_pending = true;
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        if !self.open {
            return;
        }
        if self.refresh_pending {
            self.refresh_pending = false;
            self.refresh(host);
        }
        let ctx: Context = ui.ctx().clone();
        self.form.pressed = None;
        let shown = self.frame.show(
            &ctx,
            &mut self.form,
            |ui, form| {
                muted(
                    ui,
                    "The selected survey points, or every survey point when none is selected. \
                     Coordinates to 3 decimals; a blank elevation is a point with none.",
                );
                coloured_label(ui, "scope", &form.scope, palette::text().to_egui());
                drive::text_with(
                    ui,
                    "report",
                    &mut form.report,
                    TextOptions {
                        read_only: true,
                        rows: 16,
                        monospace: true,
                    },
                );
            },
            |ui, form| {
                ui.horizontal(|ui| {
                    for (name, text) in [
                        ("refresh", "Refresh"),
                        ("copy", "Copy"),
                        ("saveCsv", "Save As CSV..."),
                        ("close", "Close"),
                    ] {
                        if drive::button(ui, name, text).clicked() {
                            form.pressed = Some(name);
                        }
                    }
                });
            },
        );
        if shown.closed {
            self.open = false;
        }
        match self.form.pressed {
            Some("refresh") => self.refresh(host),
            Some("copy") => {
                let report = self.form.report.clone();
                if let Err(error) = host.clipboard().set_text(&report) {
                    host.log(&format!("Point Report: {}", error.describe()), true);
                }
            }
            Some("saveCsv") => self.save(host),
            Some("close") => self.open = false,
            _ => {}
        }
    }

    fn refresh(&mut self, host: &mut dyn Host) {
        let document = host.document();
        let ids = document.selection().ids();
        let pick = survey_points_among(document, &ids, &SurveyImportOptions::default());
        let selected = !pick.points.is_empty();
        self.points = if selected {
            pick.points
        } else {
            drawing_survey_points(document, &SurveyImportOptions::default())
        };
        self.form.scope = if selected {
            format!("The {} selected survey point(s).", self.points.len())
        } else {
            format!("All {} survey point(s) in the drawing.", self.points.len())
        };
        self.form.report = format_point_report(&self.points, 3);
        host.log(
            &format!("Point Report: {} point(s)", self.points.len()),
            false,
        );
    }

    fn save(&mut self, host: &mut dyn Host) {
        if host.headless() {
            host.log(
                &format!("Point Report: {}", no_file_dialog().describe()),
                true,
            );
            return;
        }
        let Some(path) = save_file_dialog("Save Point Report", "", &[("CSV", &["csv"])]) else {
            return;
        };
        let path = String::from_utf8_lossy(&path_to_utf8(&path)).into_owned();
        if let Err(error) = write_file(&path, point_report_csv(&self.points, 3).as_bytes()) {
            host.log(&format!("Point Report: {}", error.describe()), true);
            return;
        }
        host.log(
            &format!(
                "Point Report: {} point(s) saved to {}",
                self.points.len(),
                file_name_of(&path)
            ),
            false,
        );
    }
}

#[cfg(test)]
mod tests {
    use std::cmp::Ordering;

    use super::{natural_order, number_order};

    #[test]
    fn point_ids_sort_as_a_person_reads_them_and_a_blank_before_every_number() {
        assert_eq!(natural_order("2", "10"), Ordering::Less);
        assert_eq!(natural_order("CP2", "CP10"), Ordering::Less);
        assert_eq!(natural_order("CP10", "CP10"), Ordering::Equal);
        assert_eq!(number_order(None, Some(-5.0)), Ordering::Less);
    }
}
