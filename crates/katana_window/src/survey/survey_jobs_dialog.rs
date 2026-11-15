//! Survey > Survey Jobs...: the field files imported as survey jobs, and going back to one to
//! change how it was reduced and adjusted (survey_jobs_dialog.hpp/.cpp).
//!
//! ```text
//! the list       every job with its format, file, when it was imported, its method, its points
//!                and its last adjustment's variance factor and global test (read back from its
//!                stored report).
//! View Report    the job's last reduction report (reduction_report_view).
//! Edit           the job's settings in the same options the import wizard showed
//! Adjustment     (reduction_options_widget). Preview reads the job's STORED bytes again through
//!                surveyio - the folder they came from is not needed - reduces them with the new
//!                settings and shows the new report beside the old one, with every point's shift
//!                from the job's last run listed and the ones that move highlighted. Apply runs
//!                cad's ReadjustSurveyJobCommand: one undo step. What to do with the job's points
//!                the person has changed by hand is asked on the page, beside Apply - never in a
//!                box, so a headless run is never stopped by one.
//! Export Report  the job's report as HTML (as kept), plain text, or PDF, to the file named
//!                beside it.
//! Remove Job     asks on the page, with or without the job's points, then cad's
//!                RemoveSurveyJobCommand: one undo step.
//! ```
//!
//! NON-MODAL and one of it: the workbench keeps it. The list follows the document - an undo, a
//! job imported meanwhile - read again whenever the drawing's survey jobs generation moves (the
//! C++'s listener refreshed it on the event loop).
//!
//! As the C++ does, Apply and Remove execute their cad command on the drawing themselves - one
//! undo step each - rather than handing a verb line to the executor.
//!
//! Reading and reducing a large job run on a thread of their own (survey_task).
//!
//! Object names (an interface: the headless --dialog, --fill and --press):
//!
//! ```text
//! dialog   surveyJobsDialog
//! list     jobs (column 0 is the job id: --fill jobs=job-1 chooses it)
//! buttons  viewReport editAdjustment exportReport browseExport removeJob confirmRemove
//!          cancelRemove previewAdjustment applyAdjustment close cancelTask
//! fields   exportFormat exportFile removeWithPoints handEdits and the options
//!          (reduction_options_widget)
//! shown    jobSummary removeQuestion handEditsFound shifts shiftSummary jobReport oldReport
//!          newReport (each with its Sections, Browser and Summary) task taskStatus message
//! ```

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use egui::{Context, Ui};
use katana_cad::linework_verbs::survey_import_finish;
use katana_cad::survey_job::{
    HandEditPolicy, ReadjustSurveyJobCommand, RemoveSurveyJobCommand, SurveyJob, SurveyJobContext,
    SurveyJobReadjustment, reduce_for_drawing, reduction_context_for,
};
use katana_cad::survey_points::drawing_survey_points;
use katana_cad::{Document, SurveyImportOptions};
use katana_core::civil_date::now_utc;
use katana_core::{Error, Result};
use katana_entity::EntityId;
use katana_survey::reduction::{ComputedPoint, ReductionOutcome};
use katana_survey::reduction_report::{ComputationMethod, ReductionReport, render_html};
use katana_survey::{ReductionSettings, SurveyPoint, SurveyProject};
use katana_surveyio::{
    ReadOptions, ReadResult, SiblingFile, format_registry, read_survey, siblings_in_memory,
};

use super::reduction_options_widget::ReductionOptionsWidget;
use super::reduction_report_view::{ReductionReportView, display_report};
use super::survey_dialogs::{
    choice_of, coloured_label, field, message_colour, muted, no_file_dialog, save_file_dialog,
};
use super::survey_job_support::{
    BACKGROUND_OBSERVATIONS, BACKGROUND_READ_BYTES, PrecomputedReduction, REPORT_ROWS_ON_SCREEN,
    ReportFileFormat, adjustment_from_report_text, adjustment_summary, cached_job_reader,
    export_report, method_text, precomputed_reduction, rejected_observations, report_format_of,
    report_input_for, survey_size,
};
use super::survey_points_ui::file_name_of;
use super::survey_task::SurveyTaskBar;
use crate::drive::{self, Choice};
use crate::format::qt_fixed;
use crate::host::Host;
use crate::shell::DialogFrame;
use crate::theme::palette;

/// Bytes of a stored report the view lays out; the export writes it whole.
const VIEWED_REPORT_BYTES: usize = 4 << 20;

/// A shift a person notices on a drawing: 1 mm. (The C++ also drew a shift of 10 mm or more - one
/// they must look at - in the error colour, cell by cell; the rows control draws no colour of its
/// own per cell, so the shifts table marks none: docs/rust.md, "Not done".)
const SHIFT_NOTICED: f64 = 0.001;

/// What Apply does with the points changed by hand, in the choice's order.
const HAND_EDITS: [(&str, HandEditPolicy); 2] = [
    ("keep the points changed by hand", HandEditPolicy::Keep),
    (
        "overwrite them with the new coordinates",
        HandEditPolicy::Overwrite,
    ),
];

/// The export's formats, in the choice's order, with the extension each adds.
const EXPORT_FORMATS: [(&str, ReportFileFormat, &str); 3] = [
    ("HTML", ReportFileFormat::Html, ".html"),
    ("plain text", ReportFileFormat::Text, ".txt"),
    ("PDF", ReportFileFormat::Pdf, ".pdf"),
];

/// "2026-09-24 10:15" from an ISO 8601 time; the text as it is otherwise.
fn short_time(iso: &str) -> String {
    if iso.len() >= 16
        && iso.as_bytes()[10] == b'T'
        && let (Some(date), Some(time)) = (iso.get(..10), iso.get(11..16))
    {
        return format!(
            "{date} {time}{}",
            if iso.ends_with('Z') { " UTC" } else { "" }
        );
    }
    String::from(iso)
}

/// As ReadjustSurveyJobCommand fills it: the job's points as it last placed them, not where they
/// are now.
fn previous_of(job: &SurveyJob) -> Vec<ComputedPoint> {
    job.placed_points
        .iter()
        .map(|placed| ComputedPoint {
            id: placed.point_id.clone(),
            northing: placed.northing,
            easting: placed.easting,
            elevation: placed.elevation,
            ..ComputedPoint::default()
        })
        .collect()
}

/// The HTML a view may lay out in reasonable time: a stored report longer than `max_bytes` is cut
/// at the end of a table row and closed, with a line saying the whole report is in the export.
pub fn viewable_report_html(html: &str, max_bytes: usize) -> String {
    if html.len() <= max_bytes {
        return String::from(html);
    }
    let head = &html.as_bytes()[..max_bytes];
    let cut = head
        .windows(5)
        .rposition(|window| window == b"</tr>")
        .map_or(max_bytes, |row| row + 5);
    // A cut inside a character is moved back to its start.
    let mut cut = cut;
    while !html.is_char_boundary(cut) {
        cut -= 1;
    }
    format!(
        "{}</tbody></table></section>\n<p class=\"empty\">The report goes on past what this view \
         shows: export it (Export Report) to read it whole.</p></body></html>\n",
        &html[..cut]
    )
}

/// What a thread of the dialog's hands back.
#[allow(
    clippy::large_enum_variant,
    reason = "one value a task, moved once from its thread to the dialog: a box would only add an allocation"
)]
enum JobsWork {
    /// The job's stored bytes read again: the read, the project taken out of it.
    Read {
        job_id: String,
        ticket: u64,
        result: Result<(Arc<ReadResult>, Arc<SurveyProject>)>,
    },
    /// A preview's reduction, and its report rendered for the screen.
    Reduce {
        job_id: String,
        revision: u64,
        settings: ReductionSettings,
        previous: Vec<ComputedPoint>,
        drawing_points: Vec<SurveyPoint>,
        then_apply: bool,
        result: Result<(Arc<ReductionOutcome>, String)>,
    },
}

/// What the dialog draws.
struct JobsForm {
    jobs: Vec<Vec<String>>,
    current: Option<usize>,
    job_summary: String,
    export_format: Choice,
    export_file: String,
    remove_shown: bool,
    remove_question: String,
    remove_with_points: bool,
    remove_with_points_text: String,
    /// 0: the job's report; 1: the edit page.
    page: usize,
    job_report: ReductionReportView,
    options: ReductionOptionsWidget,
    hand_edits: Choice,
    hand_edits_found: String,
    hand_edits_accent: bool,
    shift_summary: String,
    shift_accent: bool,
    shifts: Vec<Vec<String>>,
    shift: Option<usize>,
    old_report: ReductionReportView,
    new_report: ReductionReportView,
    task: SurveyTaskBar<JobsWork>,
    task_cancelled: bool,
    message: String,
    message_is_error: bool,
    pressed: Vec<&'static str>,
    job_changed: bool,
    options_changed: bool,
}

/// The Survey Jobs dialog (the C++ SurveyJobsDialog).
pub struct SurveyJobsDialog {
    frame: DialogFrame,
    open: bool,
    form: JobsForm,
    /// The job being edited: its id, and its stored bytes read again - the read and the project
    /// taken out of it, shared with the threads that reduce it.
    editing_job: String,
    read: Option<Arc<ReadResult>>,
    raw: Option<Arc<SurveyProject>>,
    /// The last preview and what it was made from.
    outcome: Option<Arc<ReductionOutcome>>,
    outcome_settings: ReductionSettings,
    outcome_previous: Vec<ComputedPoint>,
    outcome_drawing_points: Vec<SurveyPoint>,
    outcome_revision: u64,
    read_ticket: u64,
    shown_generation: Option<u64>,
}

impl Default for SurveyJobsDialog {
    fn default() -> Self {
        Self::new()
    }
}

impl SurveyJobsDialog {
    pub fn new() -> Self {
        Self {
            frame: DialogFrame::new("surveyJobsDialog", "Survey Jobs"),
            open: false,
            form: JobsForm {
                jobs: Vec::new(),
                current: None,
                job_summary: String::new(),
                export_format: Choice::at(0),
                export_file: String::new(),
                remove_shown: false,
                remove_question: String::new(),
                remove_with_points: false,
                remove_with_points_text: String::new(),
                page: 0,
                job_report: ReductionReportView::new("jobReport"),
                options: ReductionOptionsWidget::new(),
                hand_edits: Choice::at(0),
                hand_edits_found: String::new(),
                hand_edits_accent: false,
                shift_summary: String::from("Press Preview to see how far each point would move."),
                shift_accent: false,
                shifts: Vec::new(),
                shift: None,
                old_report: ReductionReportView::new("oldReport"),
                new_report: ReductionReportView::new("newReport"),
                task: SurveyTaskBar::new(),
                task_cancelled: false,
                message: String::new(),
                message_is_error: false,
                pressed: Vec::new(),
                job_changed: false,
                options_changed: false,
            },
            editing_job: String::new(),
            read: None,
            raw: None,
            outcome: None,
            outcome_settings: ReductionSettings::default(),
            outcome_previous: Vec::new(),
            outcome_drawing_points: Vec::new(),
            outcome_revision: 0,
            read_ticket: 0,
            shown_generation: None,
        }
    }

    pub fn show(&mut self) {
        self.open = true;
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        if !self.open {
            return;
        }
        let ctx = ui.ctx().clone();
        self.refresh(host);
        if let Some(work) = self.form.task.poll() {
            self.apply_work(host, &ctx, work);
        }
        self.form.pressed.clear();
        self.form.job_changed = false;
        self.form.options_changed = false;
        self.form.task_cancelled = false;
        let shown = self
            .frame
            .show(&ctx, &mut self.form, Self::draw_body, |ui, form| {
                ui.horizontal(|ui| {
                    if drive::button(ui, "close", "Close")
                        .on_hover_text("Close; the jobs stay with the drawing")
                        .clicked()
                    {
                        form.pressed.push("close");
                    }
                });
            });
        if shown.closed {
            self.open = false;
        }
        if self.form.task_cancelled {
            self.show_message("Cancelled: nothing was changed.");
        }
        if self.form.job_changed {
            self.job_chosen(host);
        }
        if self.form.options_changed && self.outcome.is_some() && !self.preview_is_current(host) {
            self.form
                .new_report
                .show_note("The settings have changed since this preview: press Preview again.");
        }
        let pressed = std::mem::take(&mut self.form.pressed);
        for name in pressed {
            match name {
                "viewReport" => self.view_report(host),
                "editAdjustment" => self.edit_adjustment(host, &ctx),
                "removeJob" => self.ask_remove(host),
                "confirmRemove" => self.confirm_remove(host),
                "cancelRemove" => self.form.remove_shown = false,
                "exportReport" => self.export(host),
                "browseExport" => self.browse(host),
                "previewAdjustment" => self.preview(host, &ctx, false),
                "applyAdjustment" => self.apply(host, &ctx),
                "close" => self.open = false,
                _ => {}
            }
        }
    }

    fn draw_body(ui: &mut Ui, form: &mut JobsForm) {
        muted(
            ui,
            "The field files imported as survey jobs. A job keeps its file as it was read, so it \
             can be reduced and adjusted again with other settings: Edit Adjustment, Preview, \
             Apply - one undo step.",
        );
        ui.weak("Job | File | Format | Imported | Method | Points | Variance factor | Global test");
        egui::ScrollArea::vertical()
            .id_salt("surveyJobsList")
            .max_height(120.0)
            .show(ui, |ui| {
                if drive::rows(ui, "jobs", &form.jobs, &mut form.current).changed() {
                    form.job_changed = true;
                }
            });
        coloured_label(
            ui,
            "jobSummary",
            &form.job_summary,
            palette::text_muted().to_egui(),
        );
        let busy = form.task.busy();
        ui.horizontal(|ui| {
            let buttons = [
                (
                    "viewReport",
                    "View Report",
                    "The job's last reduction report",
                ),
                (
                    "editAdjustment",
                    "Edit Adjustment",
                    "Change the job's reduction and adjustment and re-run it",
                ),
                (
                    "removeJob",
                    "Remove Job...",
                    "Take the job off the list, with or without its points",
                ),
            ];
            for (name, text, tip) in buttons {
                if drive::button(ui, name, text).on_hover_text(tip).clicked() {
                    form.pressed.push(name);
                }
            }
            ui.add_space(16.0);
            let formats: Vec<&str> = EXPORT_FORMATS.iter().map(|(label, _, _)| *label).collect();
            choice_of(ui, "exportFormat", &formats, &mut form.export_format)
                .on_hover_text("The format, when the file name has no extension saying it");
            field(
                ui,
                "exportFile",
                &mut form.export_file,
                "file to export the report to",
                "",
            );
            if drive::button(ui, "browseExport", "...")
                .on_hover_text("Choose the file")
                .clicked()
            {
                form.pressed.push("browseExport");
            }
            if drive::button(ui, "exportReport", "Export Report")
                .on_hover_text("Write the job's report: HTML as kept, plain text, or PDF")
                .clicked()
            {
                form.pressed.push("exportReport");
            }
        });
        // Removal is confirmed here, on the page: a box would stop a headless run.
        let mut remove_row = |ui: &mut Ui| {
            ui.horizontal(|ui| {
                coloured_label(
                    ui,
                    "removeQuestion",
                    &form.remove_question,
                    palette::error().to_egui(),
                );
                drive::check(
                    ui,
                    "removeWithPoints",
                    &form.remove_with_points_text,
                    &mut form.remove_with_points,
                );
                if drive::button(ui, "confirmRemove", "Remove")
                    .on_hover_text("Remove the job")
                    .clicked()
                {
                    form.pressed.push("confirmRemove");
                }
                if drive::button(ui, "cancelRemove", "Keep It")
                    .on_hover_text("Leave the job as it is")
                    .clicked()
                {
                    form.pressed.push("cancelRemove");
                }
            });
        };
        if form.remove_shown {
            remove_row(ui);
        } else {
            super::survey_dialogs::hidden(ui, "removeRow", remove_row);
        }
        if form.page == 0 {
            form.job_report.ui(ui);
            super::survey_dialogs::offstage(ui, "surveyJobsEdit", |ui| {
                Self::edit_page(ui, form, busy)
            });
        } else {
            super::survey_dialogs::offstage(ui, "surveyJobsReport", |ui| form.job_report.ui(ui));
            Self::edit_page(ui, form, busy);
        }
        if form.task.ui(ui) {
            form.task_cancelled = true;
        }
        coloured_label(
            ui,
            "message",
            &form.message,
            message_colour(form.message_is_error),
        );
    }

    fn edit_page(ui: &mut Ui, form: &mut JobsForm, busy: bool) {
        ui.horizontal_top(|ui| {
            egui::ScrollArea::vertical()
                .id_salt("surveyJobsOptions")
                .max_height(480.0)
                .show(ui, |ui| {
                    ui.set_width(430.0);
                    if form.options.ui(ui) {
                        form.options_changed = true;
                    }
                });
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled_ui(!busy, |ui| {
                            drive::button(ui, "previewAdjustment", "Preview").on_hover_text(
                                "Read the stored file again, reduce and adjust it with these \
                                 settings, and show the new report beside the last one",
                            )
                        })
                        .inner
                        .clicked()
                    {
                        form.pressed.push("previewAdjustment");
                    }
                    ui.label("Points changed by hand:");
                    let policies: Vec<&str> = HAND_EDITS.iter().map(|(label, _)| *label).collect();
                    choice_of(ui, "handEdits", &policies, &mut form.hand_edits).on_hover_text(
                        "What Apply does with the job's points moved, re-levelled or deleted by \
                         hand since the job placed them",
                    );
                    if ui
                        .add_enabled_ui(!busy, |ui| {
                            drive::button(ui, "applyAdjustment", "Apply").on_hover_text(
                                "Re-adjust the job with these settings and move its points: one \
                                 undo step",
                            )
                        })
                        .inner
                        .clicked()
                    {
                        form.pressed.push("applyAdjustment");
                    }
                });
                let accent = palette::accent().to_egui();
                let muted_colour = palette::text_muted().to_egui();
                coloured_label(
                    ui,
                    "handEditsFound",
                    &form.hand_edits_found,
                    if form.hand_edits_accent {
                        accent
                    } else {
                        muted_colour
                    },
                );
                coloured_label(
                    ui,
                    "shiftSummary",
                    &form.shift_summary,
                    if form.shift_accent {
                        accent
                    } else {
                        muted_colour
                    },
                );
                ui.weak(
                    "Point | Shift N (mm) | Shift E (mm) | Shift H (mm) | Horizontal (mm) | In \
                     the drawing",
                );
                egui::ScrollArea::vertical()
                    .id_salt("surveyJobsShifts")
                    .max_height(150.0)
                    .show(ui, |ui| {
                        drive::rows(ui, "shifts", &form.shifts, &mut form.shift);
                    });
                ui.columns(2, |columns| {
                    muted(&mut columns[0], "The job's last run");
                    form.old_report.ui(&mut columns[0]);
                    muted(&mut columns[1], "This preview");
                    form.new_report.ui(&mut columns[1]);
                });
            });
        });
    }

    fn show_error(&mut self, host: &mut dyn Host, error: &Error) {
        let mut text = error.message.clone();
        if !error.context.is_empty() {
            text.push_str(" - ");
            text.push_str(&error.context);
        }
        self.form.message = text;
        self.form.message_is_error = true;
        host.log(&format!("Survey Jobs: {}", error.describe()), true);
    }

    fn show_message(&mut self, text: &str) {
        self.form.message = String::from(text);
        self.form.message_is_error = false;
    }

    /// The list as the document's jobs are now, when they have changed since it was last read.
    fn refresh(&mut self, host: &mut dyn Host) {
        let document = host.document();
        let generation = document.survey_jobs_generation();
        if self.shown_generation == Some(generation) {
            return;
        }
        self.shown_generation = Some(generation);
        let chosen = self
            .form
            .current
            .and_then(|row| self.form.jobs.get(row))
            .and_then(|row| row.first())
            .cloned();
        self.form.jobs = document
            .survey_jobs()
            .iter()
            .map(|job| {
                let format = format_registry().find(&job.format_id).map_or_else(
                    |_| job.format_id.clone(),
                    |descriptor| descriptor.human_name,
                );
                let last = adjustment_from_report_text(&job.report_text);
                vec![
                    job.id.clone(),
                    job.source_file_name.clone(),
                    format,
                    short_time(&job.imported_utc),
                    method_text(&job.settings),
                    format!("{}", job.placed_points.len()),
                    last.variance_factor.unwrap_or_else(|| String::from("-")),
                    last.global_test.unwrap_or_else(|| String::from("-")),
                ]
            })
            .collect();
        let again = chosen
            .and_then(|id| self.form.jobs.iter().position(|row| row[0] == id))
            .or_else(|| self.form.jobs.len().checked_sub(1));
        self.form.current = again;
        // A job being edited that is gone (an undo of its import) ends the edit.
        if !self.editing_job.is_empty() && document.find_survey_job(&self.editing_job).is_none() {
            self.editing_job.clear();
            self.read = None;
            self.raw = None;
            self.outcome = None;
            self.form.page = 0;
        }
        self.job_chosen(host);
    }

    fn current_job<'a>(&self, document: &'a Document) -> Option<&'a SurveyJob> {
        let id = self.form.jobs.get(self.form.current?)?.first()?;
        document.find_survey_job(id)
    }

    fn job_chosen(&mut self, host: &mut dyn Host) {
        let document = host.document();
        let Some(job) = self.current_job(document) else {
            self.form.job_summary = String::from(if document.survey_jobs().is_empty() {
                "No survey jobs yet: Survey > Import Survey Points reads a field file as one."
            } else {
                "Choose a job."
            });
            self.form.job_report.show_note("No job chosen.");
            return;
        };
        let siblings: Vec<&str> = job
            .sibling_files
            .iter()
            .map(|file| file.name.as_str())
            .collect();
        self.form.job_summary = format!(
            "{}: {} on layer {}, {} point(s); parser {}; last reduced {}{}.",
            job.id,
            job.name,
            job.layer,
            job.placed_points.len(),
            job.parser_version,
            short_time(&job.report_created_utc),
            if siblings.is_empty() {
                String::new()
            } else {
                format!("; with {}", siblings.join(", "))
            }
        );
        if self.form.page == 0 {
            let html = viewable_report_html(&job.report_html, VIEWED_REPORT_BYTES);
            self.form.job_report.set_report_html(&html);
        }
    }

    fn view_report(&mut self, host: &mut dyn Host) {
        let Some(html) = self
            .current_job(host.document())
            .map(|job| viewable_report_html(&job.report_html, VIEWED_REPORT_BYTES))
        else {
            self.show_message("Choose a job first.");
            return;
        };
        self.form.page = 0;
        self.form.job_report.set_report_html(&html);
    }

    fn edit_adjustment(&mut self, host: &mut dyn Host, ctx: &Context) {
        let Some(job) = self.current_job(host.document()).cloned() else {
            self.show_message("Choose a job first.");
            return;
        };
        self.editing_job = job.id.clone();
        self.read = None;
        self.raw = None;
        self.outcome = None;
        self.form.page = 1;
        self.form.options.set_settings(&job.settings);
        self.form
            .old_report
            .set_report_html(&viewable_report_html(&job.report_html, VIEWED_REPORT_BYTES));
        self.form
            .new_report
            .show_note("Press Preview to reduce and adjust the job with these settings.");
        self.form.shifts.clear();
        self.form.shift = None;
        self.form.shift_summary =
            String::from("Press Preview to see how far each point would move.");
        self.form.shift_accent = false;
        self.show_hand_edits(host.document(), &job);
        match reduction_context_for(host.document()) {
            Ok(context) => self
                .form
                .options
                .set_drawing_points(&context.drawing_points),
            Err(error) => self.show_error(host, &error),
        }

        // The stored bytes, read again as the re-adjustment will read them. The thread gets
        // copies: the job itself is the document's, which an undo may change while it runs.
        let job_id = job.id.clone();
        let format_id = job.format_id.clone();
        let name = job.source_file_name.clone();
        let size = job.source_bytes.len() as u64;
        let bytes = job.source_bytes;
        let files: Vec<SiblingFile> = job
            .sibling_files
            .into_iter()
            .map(|file| SiblingFile {
                name: file.name,
                bytes: file.bytes,
            })
            .collect();
        self.read_ticket += 1;
        let ticket = self.read_ticket;
        let started = self.form.task.run(
            &format!("Reading the stored {name} again"),
            size > BACKGROUND_READ_BYTES,
            ctx,
            move || {
                let options = ReadOptions {
                    siblings: Some(siblings_in_memory(files)),
                    ..ReadOptions::default()
                };
                let result = read_survey(format_registry(), &format_id, &bytes, &name, &options)
                    .map(|mut read| {
                        let raw = Arc::new(std::mem::take(&mut read.project));
                        (Arc::new(read), raw)
                    });
                JobsWork::Read {
                    job_id,
                    ticket,
                    result,
                }
            },
        );
        if let Some(work) = started {
            self.apply_work(host, ctx, work);
        }
    }

    fn apply_work(&mut self, host: &mut dyn Host, ctx: &Context, work: JobsWork) {
        match work {
            JobsWork::Read {
                job_id,
                ticket,
                result,
            } => match result {
                Err(error) => self.show_error(host, &error),
                Ok((read, raw)) => {
                    if ticket != self.read_ticket || self.editing_job != job_id {
                        return;
                    }
                    self.form.options.set_project(&raw);
                    self.show_message(&format!(
                        "Read the job's stored file again with parser {}: {} record(s), {} \
                         warning(s).",
                        read.parser_version,
                        read.records_read,
                        read.warnings.len()
                    ));
                    self.read = Some(read);
                    self.raw = Some(raw);
                }
            },
            JobsWork::Reduce {
                job_id,
                revision,
                settings,
                previous,
                drawing_points,
                then_apply,
                result,
            } => match result {
                Err(error) => {
                    self.form.new_report.show_note(&format!(
                        "The reduction could not run: {}",
                        error.describe()
                    ));
                    self.show_error(host, &error);
                }
                Ok((outcome, html)) => {
                    if self.editing_job != job_id {
                        return;
                    }
                    self.outcome_settings = settings;
                    self.outcome_previous = previous;
                    self.outcome_drawing_points = drawing_points;
                    self.outcome_revision = revision;
                    self.form.new_report.set_report_html(&html);
                    self.show_shifts(host.document(), &outcome.report);
                    self.show_message(&format!(
                        "Preview: {} point(s); {}; {} observation(s) rejected.",
                        outcome.points.len(),
                        adjustment_summary(&outcome.report),
                        rejected_observations(&outcome.report)
                    ));
                    self.outcome = Some(outcome);
                    if then_apply {
                        self.apply(host, ctx);
                    }
                }
            },
        }
    }

    fn context_for(&self, document: &Document, job: &SurveyJob) -> Result<SurveyJobContext> {
        let mut context = reduction_context_for(document)?;
        if let Some(read) = &self.read {
            context.input = report_input_for(read, &job.source_file_name);
        }
        context.created_utc = now_utc();
        context.previous = previous_of(job);
        Ok(context)
    }

    fn preview_is_current(&self, host: &mut dyn Host) -> bool {
        let document = host.document();
        let Some(job) = document.find_survey_job(&self.editing_job) else {
            return false;
        };
        if self.outcome.is_none()
            || self.outcome_revision != document.model_revision()
            || self.outcome_previous != previous_of(job)
        {
            return false;
        }
        self.form
            .options
            .settings()
            .is_ok_and(|settings| settings == self.outcome_settings)
    }

    fn preview(&mut self, host: &mut dyn Host, ctx: &Context, then_apply: bool) {
        let Some(job) = host.document().find_survey_job(&self.editing_job).cloned() else {
            self.show_message("Choose a job and press Edit Adjustment first.");
            return;
        };
        let Some(raw) = self.raw.clone() else {
            self.show_message("The job's stored file is still being read.");
            return;
        };
        let settings = match self.form.options.settings() {
            Ok(settings) => settings,
            Err(error) => {
                self.show_error(host, &error);
                return;
            }
        };
        let context = match self.context_for(host.document(), &job) {
            Ok(context) => context,
            Err(error) => {
                self.show_error(host, &error);
                return;
            }
        };
        let size = survey_size(&raw);
        let job_id = job.id.clone();
        let revision = host.document().model_revision();
        let previous = context.previous.clone();
        let drawing_points = context.drawing_points.clone();
        self.form.new_report.show_note("Reducing and adjusting...");
        let started = self.form.task.run(
            &format!(
                "Reducing and adjusting {size} observation(s) and point(s) of {}",
                job.source_file_name
            ),
            size > BACKGROUND_OBSERVATIONS,
            ctx,
            move || {
                let result = reduce_for_drawing(&raw, &settings, &context.to_reduction_context())
                    .map(|outcome| {
                        let html =
                            render_html(&display_report(&outcome.report, REPORT_ROWS_ON_SCREEN));
                        (Arc::new(outcome), html)
                    });
                JobsWork::Reduce {
                    job_id,
                    revision,
                    settings,
                    previous,
                    drawing_points,
                    then_apply,
                    result,
                }
            },
        );
        if let Some(work) = started {
            self.apply_work(host, ctx, work);
        }
    }

    fn apply(&mut self, host: &mut dyn Host, ctx: &Context) {
        let Some(job) = host.document().find_survey_job(&self.editing_job).cloned() else {
            self.show_message("Choose a job and press Edit Adjustment first.");
            return;
        };
        // Apply is what the preview showed: without a current one, it is run first (on its own
        // thread when large) and Apply follows it.
        if !self.preview_is_current(host) {
            self.preview(host, ctx, true);
            return;
        }
        let context = match self.context_for(host.document(), &job) {
            Ok(context) => context,
            Err(error) => {
                self.show_error(host, &error);
                return;
            }
        };
        let policy = HAND_EDITS[self.form.hand_edits.index.unwrap_or(0).min(1)].1;
        let request = SurveyJobReadjustment {
            job_id: job.id.clone(),
            settings: self.outcome_settings.clone(),
            context,
            hand_edits: policy,
            parser_version: self
                .read
                .as_ref()
                .map(|read| read.parser_version.clone())
                .unwrap_or_default(),
            // HOW a job that was imported coded or strung is finished again: as an import into
            // this drawing is (survey_import_finish) - colours through the Document's resolver,
            // the customisation's control codes, strings by point number. WHETHER is the job's
            // own, as it was imported, and is not read from this. Left empty, a point this run
            // draws for the first time was given a second style, "<name> (<colour>)", beside the
            // one its neighbours wear: a colour name with no lookup has no RGB.
            finish: survey_import_finish(host.document(), None, None).options,
        };
        let command = ReadjustSurveyJobCommand::new(
            host.document(),
            request,
            Some(cached_job_reader(job.id.clone(), self.raw.clone())),
            Some(precomputed_reduction(PrecomputedReduction {
                outcome: self.outcome.clone(),
                settings: self.outcome_settings.clone(),
                previous: self.outcome_previous.clone(),
                drawing_points: self.outcome_drawing_points.clone(),
            })),
        );
        let results = command.results();
        if let Err(error) = host.document_mut().execute(Box::new(command)) {
            self.show_error(host, &error);
            return;
        }
        // The command is on the undo stack now, and its results say what it did.
        let changes = results.changes();
        let summary = results
            .report()
            .map_or_else(|| String::from("-"), |report| adjustment_summary(&report));
        host.log(
            &format!(
                "Re-adjusted survey job {} ({}): {summary}; {} point(s) moved, {} created, {} \
                 removed, {} changed by hand and {} deleted by hand ({}), {} not drawn. One \
                 command - Undo puts the job and its points back.",
                job.id,
                job.source_file_name,
                changes.moved.len(),
                changes.created.len(),
                changes.removed.len(),
                changes.edited_by_hand.len(),
                changes.deleted_by_hand.len(),
                if policy == HandEditPolicy::Keep {
                    "kept"
                } else {
                    "overwritten"
                },
                changes.not_drawn.len()
            ),
            false,
        );
        self.show_message(
            "Applied: the job's points and report are the preview's now. Undo puts them back.",
        );
        if let Some(after) = host.document().find_survey_job(&job.id).cloned() {
            self.form.old_report.set_report_html(&viewable_report_html(
                &after.report_html,
                VIEWED_REPORT_BYTES,
            ));
            self.show_hand_edits(host.document(), &after);
        }
        self.outcome = None;
        self.form
            .new_report
            .show_note("Applied. Change the settings and press Preview to try others.");
    }

    /// The job's points as the drawing has them now, against where the job placed them: what
    /// Apply will ask about.
    fn show_hand_edits(&mut self, document: &Document, job: &SurveyJob) {
        let drawing = drawing_survey_points(document, &SurveyImportOptions::default());
        let by_entity: HashMap<EntityId, usize> = drawing
            .iter()
            .enumerate()
            .map(|(at, point)| (point.entity, at))
            .collect();
        let mut moved: Vec<&str> = Vec::new();
        let mut deleted: Vec<&str> = Vec::new();
        for placed in &job.placed_points {
            let Some(&at) = by_entity.get(&placed.entity) else {
                deleted.push(&placed.point_id);
                continue;
            };
            let now = &drawing[at];
            if now.northing != placed.northing
                || now.easting != placed.easting
                || now.elevation != placed.elevation
            {
                moved.push(&placed.point_id);
            }
        }
        let listed = |ids: &[&str]| {
            if ids.len() <= 8 {
                ids.join(", ")
            } else {
                format!("{}, ...", ids[..8].join(", "))
            }
        };
        if moved.is_empty() && deleted.is_empty() {
            self.form.hand_edits_found = String::from(
                "None of the job's points has been changed by hand since it placed them.",
            );
            self.form.hand_edits_accent = false;
            return;
        }
        let mut text = String::from("Changed by hand since the job placed them: ");
        if !moved.is_empty() {
            text.push_str(&format!(
                "{} moved or re-levelled ({})",
                moved.len(),
                listed(&moved[..])
            ));
        }
        if !deleted.is_empty() {
            text.push_str(&format!(
                "{}{} deleted ({})",
                if moved.is_empty() { "" } else { "; " },
                deleted.len(),
                listed(&deleted[..])
            ));
        }
        text.push_str(". Choose beside Apply whether the edits stand.");
        self.form.hand_edits_found = text;
        self.form.hand_edits_accent = true;
    }

    fn show_shifts(&mut self, document: &Document, report: &ReductionReport) {
        let mut rows: Vec<(usize, f64)> = Vec::new();
        let mut fresh = 0_usize;
        for (at, point) in report.coordinates.iter().enumerate() {
            if point.shift_northing.is_none()
                && point.shift_easting.is_none()
                && point.shift_elevation.is_none()
            {
                fresh += 1;
                continue;
            }
            rows.push((
                at,
                point
                    .shift_northing
                    .unwrap_or(0.0)
                    .hypot(point.shift_easting.unwrap_or(0.0)),
            ));
        }
        let worst = |row: &(usize, f64)| {
            katana_math::cpp::max(
                row.1,
                report.coordinates[row.0]
                    .shift_elevation
                    .unwrap_or(0.0)
                    .abs(),
            )
        };
        // The largest first: what a person needs to see is what moves.
        rows.sort_by(|a, b| worst(b).partial_cmp(&worst(a)).unwrap_or(Ordering::Equal));
        let mut gone = 0_usize;
        if let Some(job) = document.find_survey_job(&self.editing_job) {
            let computed: HashSet<&str> = report
                .coordinates
                .iter()
                .map(|point| point.point_id.as_str())
                .collect();
            gone = job
                .placed_points
                .iter()
                .filter(|placed| !computed.contains(placed.point_id.as_str()))
                .count();
        }
        let mm = |metres: Option<f64>| {
            metres.map_or_else(|| String::from("-"), |metres| qt_fixed(metres * 1000.0, 1))
        };
        let mut noticed = 0_usize;
        let mut largest = 0.0_f64;
        let mut largest_id = String::new();
        let mut held_control = 0_usize;
        let mut shifts = Vec::with_capacity(rows.len());
        for row in &rows {
            let point = &report.coordinates[row.0];
            let row_worst = worst(row);
            // A control point is drawn where it is held, not where a weighted adjustment puts it
            // (the reduced project keeps its known coordinates), so Apply leaves it; the adjusted
            // value is the report's.
            let control = point.method == ComputationMethod::Control;
            if control && row_worst >= SHIFT_NOTICED {
                held_control += 1;
            }
            shifts.push(vec![
                point.point_id.clone(),
                mm(point.shift_northing),
                mm(point.shift_easting),
                mm(point.shift_elevation),
                qt_fixed(row.1 * 1000.0, 1),
                String::from(if control { "stays: control" } else { "moves" }),
            ]);
            if row_worst >= SHIFT_NOTICED {
                noticed += 1;
            }
            if row_worst > largest {
                largest = row_worst;
                largest_id.clone_from(&point.point_id);
            }
        }
        self.form.shifts = shifts;
        self.form.shift = None;
        let mut text = format!(
            "{} point(s) against the job's last run: {noticed} move by 1 mm or more",
            rows.len()
        );
        if noticed > 0 {
            text.push_str(&format!(
                " (the most {} mm, {largest_id})",
                qt_fixed(largest * 1000.0, 1)
            ));
        }
        text.push_str(&format!("; {fresh} new; {gone} no longer computed."));
        if held_control > 0 {
            text.push_str(&format!(
                " {held_control} of them control: its adjusted value is in the report, and its \
                 point stays where it is held."
            ));
        }
        self.form.shift_summary = text;
        self.form.shift_accent = noticed > 0;
    }

    fn export(&mut self, host: &mut dyn Host) {
        let Some(job) = self.current_job(host.document()).cloned() else {
            self.show_message("Choose a job first.");
            return;
        };
        let mut path = String::from(self.form.export_file.trim());
        let mut format = report_format_of(&path);
        if format.is_none() && !path.is_empty() {
            // No extension that says it: the format chosen beside, and its extension added.
            let (_, chosen, extension) =
                EXPORT_FORMATS[self.form.export_format.index.unwrap_or(0).min(2)];
            format = Some(chosen);
            path.push_str(extension);
        }
        let format = format.unwrap_or(ReportFileFormat::Html);
        if let Err(error) = export_report(&job, &path, format) {
            self.show_error(host, &error);
            return;
        }
        let format_name = EXPORT_FORMATS
            .iter()
            .find(|(_, each, _)| *each == format)
            .map_or("HTML", |(name, _, _)| *name);
        host.log(
            &format!(
                "Exported the reduction report of survey job {} ({}) as {format_name} to {}",
                job.id,
                job.source_file_name,
                file_name_of(&path)
            ),
            false,
        );
        self.show_message(&format!("Exported to {path}."));
    }

    fn browse(&mut self, host: &mut dyn Host) {
        if host.headless() {
            self.show_error(host, &no_file_dialog());
            return;
        }
        if let Some(path) = save_file_dialog(
            "Export Reduction Report",
            &self.form.export_file,
            &[
                ("HTML", &["html"]),
                ("Plain text", &["txt"]),
                ("PDF", &["pdf"]),
            ],
        ) {
            self.form.export_file =
                String::from_utf8_lossy(&katana_core::path_text::path_to_utf8(&path)).into_owned();
        }
    }

    fn ask_remove(&mut self, host: &mut dyn Host) {
        let Some(job) = self.current_job(host.document()) else {
            self.show_message("Choose a job first.");
            return;
        };
        self.form.remove_question = format!(
            "Remove survey job {} ({})? Its stored file, settings and report go with it; Undo \
             brings them back.",
            job.id, job.source_file_name
        );
        self.form.remove_with_points_text = format!(
            "and its {} point(s) on the drawing",
            job.placed_points.len()
        );
        self.form.remove_with_points = false;
        self.form.remove_shown = true;
    }

    fn confirm_remove(&mut self, host: &mut dyn Host) {
        let Some(job) = self.current_job(host.document()) else {
            return;
        };
        if !self.form.remove_shown {
            return;
        }
        let id = job.id.clone();
        let file = job.source_file_name.clone();
        let with_points = self.form.remove_with_points;
        self.form.remove_shown = false;
        let command = RemoveSurveyJobCommand::new(host.document(), id.as_str(), with_points);
        if let Err(error) = host.document_mut().execute(Box::new(command)) {
            self.show_error(host, &error);
            return;
        }
        host.log(
            &format!(
                "Removed survey job {id} ({file}){}. One command - Undo puts it back.",
                if with_points {
                    " with its points"
                } else {
                    "; its points stay as ordinary survey points"
                }
            ),
            false,
        );
    }
}
