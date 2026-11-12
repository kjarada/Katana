//! Survey > Coordinate Geometry > Parcel Report... (survey_parcel_dialog.hpp/.cpp; docs/survey.md,
//! "The Survey menu"): a closed polyline's courses, area and deed wording, and bearing and
//! distance labels on it.
//!
//! NOTHING IS COMPUTED HERE. The courses, the summary and the legal description come from
//! katana_cad::parcel - parcel_report, format_parcel_report and legal_description, the functions
//! PARCEL id and PARCEL id LEGAL print from - so the dialog and the command line say the same
//! thing. The labels change the drawing, so they are not made here either: Label Courses writes
//! the line PARCEL <id> LABEL <height> and hands it to the window's one executor (`Host::run`),
//! which echoes it, keeps it in the history and makes the labels one undo step, exactly as if
//! typed.
//!
//! A survey tool dialog like the menu's others: non-modal, kept by the survey workbench, Use
//! Selection reading the selection when pressed. The report pane below the tabs shows what
//! PARCEL id prints, and the log gets it too.
//!
//! Object names (the headless --dialog surveyParcelReport, --fill and --press drive it by these):
//!
//! ```text
//! surveyParcelReportDialog   the dialog (the action surveyParcelReport + "Dialog")
//! parcel                     the parcel: a closed polyline's entity id
//! useSelection               fill parcel from the one selected closed polyline
//! compute                    the report: the pane, the log and both tabs
//! parcelTabs                 Courses, Legal Description
//! parcelCourses              course, from E, from N, bearing, distance
//! parcelSummary              area in m2 and ha, perimeter, centroid, direction drawn
//! parcelCopy                 the courses to the clipboard, tab separated
//! parcelCsv                  Save CSV... (no file dialog in a headless run)
//! parcelName                 the name the legal description opens with
//! parcelLegalText            the legal description (read only)
//! parcelLegalSave            Save... as a .txt (no file dialog in a headless run)
//! labelHeight                the label text height, model units (2.5)
//! label                      PARCEL <id> LABEL <height> through the executor
//! ```

use std::path::Path;

use egui::Ui;
use katana_cad::Document;
use katana_cad::parcel::{
    ParcelReport, format_parcel_report, format_parcel_summary, legal_description,
    parcel_courses_csv, parcel_report,
};
use katana_core::error::make_error;
use katana_core::path_text::path_to_utf8;
use katana_core::text::{format_general, parse_integer};
use katana_core::{Error, ErrorCode, Result, Status};
use katana_entity::{EntityId, Geometry};

use super::survey_dialogs::{
    SurveyToolFrame, ToolButton, coloured_label, field, form_row, frame_views, offstage,
    save_file_dialog,
};
use crate::drive::{self, LineOptions, TextOptions};
use crate::format::qt_fixed;
use crate::host::Host;
use crate::theme::palette;

/// The label height PARCEL id LABEL takes when it is given none, so the dialog and the verb start
/// from the same text.
const DEFAULT_LABEL_HEIGHT: f64 = 2.5;

/// The courses table's columns.
const COURSE_COLUMNS: [&str; 5] = ["Course", "From E", "From N", "Bearing", "Distance"];

/// An entity id as the command line takes it: 12 or #12.
fn entity_id(typed: &str) -> Result<EntityId> {
    let text = typed.trim();
    let digits = text.strip_prefix('#').unwrap_or(text);
    match parse_integer(digits) {
        Some(number) if number > 0 => Ok(number as EntityId),
        _ => Err(make_error(
            ErrorCode::InvalidArgument,
            "Parcel: type the id of a closed polyline, or Use Selection",
            text,
        )),
    }
}

fn write_text(path: &Path, text: &str) -> Status {
    std::fs::write(path, text).map_err(|error| {
        make_error(
            ErrorCode::FileExportFailure,
            format!(
                "cannot write {}",
                String::from_utf8_lossy(&path_to_utf8(path))
            ),
            format!("{error}"),
        )
    })
}

/// What the dialog draws besides the shared pane.
#[derive(Clone, Debug, PartialEq)]
struct ParcelForm {
    parcel: String,
    label_height: String,
    tab: usize,
    courses: Vec<Vec<String>>,
    course: Option<usize>,
    summary: String,
    name: String,
    name_placeholder: String,
    legal: String,
    /// A button of the tabs pressed this frame.
    pressed: Option<&'static str>,
    name_changed: bool,
}

/// The Parcel Report dialog (the C++ SurveyParcelDialog).
pub struct SurveyParcelDialog {
    pub tool: SurveyToolFrame,
    form: ParcelForm,
    /// Whether the window's executor is there: a test may build the dialog without it, and the
    /// dialog then says it cannot label (the C++'s empty CommandRunner).
    executor: bool,
    /// The last report computed, and the entity it is of.
    report: Option<ParcelReport>,
    report_of: EntityId,
}

impl SurveyParcelDialog {
    /// `executor` false: a dialog with no command line to run PARCEL LABEL on.
    pub fn new(executor: bool) -> Self {
        Self {
            tool: SurveyToolFrame::new(
                "surveyParcelReportDialog",
                "Parcel Report",
                "The courses of a closed polyline as quadrant bearings (whole seconds) and \
                 horizontal distances, its area, perimeter and centroid, and the wording a deed \
                 carries - what PARCEL id and PARCEL id LEGAL print. Label Courses puts a bearing \
                 and distance on every course and the area at the centroid, on the current \
                 layer, as one undo step.",
            ),
            form: ParcelForm {
                parcel: String::new(),
                label_height: format_general(DEFAULT_LABEL_HEIGHT, 6),
                tab: 0,
                courses: Vec::new(),
                course: None,
                summary: String::new(),
                name: String::new(),
                name_placeholder: String::from("Parcel <id>"),
                legal: String::new(),
                pressed: None,
                name_changed: false,
            },
            executor,
            report: None,
            report_of: 0,
        }
    }

    pub fn show(&mut self) {
        self.tool.open = true;
    }

    pub fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        self.form.pressed = None;
        self.form.name_changed = false;
        // Each filler goes in at the left in the C++, so the last added is first: Use Selection,
        // then Label Courses.
        let pressed = self.tool.show(
            ui,
            &mut self.form,
            Self::draw_form,
            &[
                ToolButton::new("useSelection", "Use Selection"),
                ToolButton::new("label", "Label Courses"),
            ],
            &[ToolButton::new("compute", "Compute")],
        );
        if self.form.name_changed {
            self.show_legal();
        }
        match self.form.pressed {
            Some("parcelCopy") => self.copy(host),
            Some("parcelCsv") => {
                let suggested = format!("parcel_{}_courses.csv", self.report_of);
                self.save(
                    host,
                    "Save Parcel Courses",
                    &suggested,
                    ("CSV", "csv"),
                    true,
                );
            }
            Some("parcelLegalSave") => {
                let suggested = format!("parcel_{}_legal.txt", self.report_of);
                self.save(
                    host,
                    "Save Legal Description",
                    &suggested,
                    ("Text", "txt"),
                    false,
                );
            }
            _ => {}
        }
        match pressed {
            Some("useSelection") => {
                self.tool.pane.message.clear();
                let status = self.use_selection(host.document());
                self.tool.filler_done(host, status);
            }
            Some("label") => {
                self.tool.pane.message.clear();
                let status = self.label(host);
                self.tool.filler_done(host, status);
            }
            Some("compute") => {
                let report = self.compute(host.document());
                self.tool.verb_done(host, report);
            }
            _ => {}
        }
    }

    fn draw_form(ui: &mut Ui, form: &mut ParcelForm) {
        form_row(ui, "Parcel:", |ui| {
            field(
                ui,
                "parcel",
                &mut form.parcel,
                "entity id",
                "The id of a closed polyline, as INFO and LIST give it",
            )
        });
        form_row(ui, "Label height:", |ui| {
            field(
                ui,
                "labelHeight",
                &mut form.label_height,
                "2.5",
                "The labels' text height, in model units",
            )
        });
        drive::tabs(
            ui,
            "parcelTabs",
            &["Courses", "Legal Description"],
            &mut form.tab,
        );
        // The page not in front is still there, as a QTabWidget's is: what it holds can be filled,
        // pressed and read.
        if form.tab == 0 {
            Self::courses_page(ui, form);
            offstage(ui, "parcelLegal", |ui| Self::legal_page(ui, form));
        } else {
            offstage(ui, "parcelCourses", |ui| Self::courses_page(ui, form));
            Self::legal_page(ui, form);
        }
    }

    fn courses_page(ui: &mut Ui, form: &mut ParcelForm) {
        ui.weak(COURSE_COLUMNS.join(" | "));
        drive::rows(ui, "parcelCourses", &form.courses, &mut form.course);
        coloured_label(
            ui,
            "parcelSummary",
            &form.summary,
            palette::text().to_egui(),
        );
        ui.horizontal(|ui| {
            if drive::button(ui, "parcelCopy", "Copy").clicked() {
                form.pressed = Some("parcelCopy");
            }
            if drive::button(ui, "parcelCsv", "Save CSV...").clicked() {
                form.pressed = Some("parcelCsv");
            }
        });
    }

    fn legal_page(ui: &mut Ui, form: &mut ParcelForm) {
        form_row(ui, "Name:", |ui| {
            let placeholder = form.name_placeholder.clone();
            if drive::line_with(
                ui,
                "parcelName",
                &mut form.name,
                LineOptions {
                    placeholder: &placeholder,
                    ..LineOptions::default()
                },
            )
            .on_hover_text("The name the description opens with, as PARCEL id LEGAL name takes it")
            .changed()
            {
                form.name_changed = true;
            }
        });
        drive::text_with(
            ui,
            "parcelLegalText",
            &mut form.legal,
            TextOptions {
                read_only: true,
                rows: 8,
                monospace: false,
            },
        );
        if drive::button(ui, "parcelLegalSave", "Save...").clicked() {
            form.pressed = Some("parcelLegalSave");
        }
    }

    fn use_selection(&mut self, document: &Document) -> Status {
        let ids = document.selection().ids();
        let closed = ids.len() == 1
            && document
                .model()
                .entities
                .find(ids[0])
                .is_some_and(|entity| {
                    matches!(&entity.geometry, Geometry::Polyline(boundary) if boundary.closed)
                });
        if !closed {
            return Err(make_error(
                ErrorCode::InvalidState,
                "select the one closed polyline to report",
                format!("{} selected", ids.len()),
            ));
        }
        self.form.parcel = format!("{}", ids[0]);
        Ok(())
    }

    fn compute(&mut self, document: &Document) -> Result<String> {
        let id = entity_id(&self.form.parcel)?;
        let Some(entity) = document.model().entities.find(id) else {
            return Err(make_error(
                ErrorCode::NotFound,
                "no entity with that id",
                format!("{id}"),
            ));
        };
        // The verb's own refusals, in its own words.
        let boundary = match &entity.geometry {
            Geometry::CurvePolyline(_) => {
                return Err(make_error(
                    ErrorCode::Unsupported,
                    "a parcel with arc courses is not reported yet; its courses are bearings and \
                     distances of straight sides",
                    format!("{id}"),
                ));
            }
            Geometry::Polyline(boundary) => boundary,
            _ => {
                return Err(make_error(
                    ErrorCode::InvalidGeometry,
                    "a parcel must be a closed polyline",
                    format!("{id}"),
                ));
            }
        };
        let report = parcel_report(boundary)?;
        self.form.courses = report
            .courses
            .iter()
            .enumerate()
            .map(|(at, course)| {
                vec![
                    format!("{}", at + 1),
                    qt_fixed(course.from.x, 3),
                    qt_fixed(course.from.y, 3),
                    course.bearing.clone(),
                    qt_fixed(course.distance, 3),
                ]
            })
            .collect();
        self.form.course = None;
        self.form.summary = format_parcel_summary(&report);
        self.form.name_placeholder = format!("Parcel {id}");
        let text = format_parcel_report(&report);
        self.report = Some(report);
        self.report_of = id;
        self.show_legal();
        Ok(text)
    }

    fn show_legal(&mut self) {
        let Some(report) = &self.report else {
            self.form.legal.clear();
            return;
        };
        // PARCEL id LEGAL's default name when none is typed.
        let typed = self.form.name.trim();
        let name = if typed.is_empty() {
            format!("Parcel {}", self.report_of)
        } else {
            String::from(typed)
        };
        self.form.legal = legal_description(report, &name);
    }

    fn label(&mut self, host: &mut dyn Host) -> Status {
        let id = entity_id(&self.form.parcel)?;
        let height = self.form.label_height.trim();
        if height.is_empty() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "type the label height",
            ));
        }
        if !self.executor {
            return Err(Error::new(
                ErrorCode::InvalidState,
                "this dialog has no command line to run PARCEL LABEL on",
            ));
        }
        let line = format!("PARCEL {id} LABEL {height}");
        let outcome = host.run(&line);
        if !outcome.ok {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                if outcome.error.is_empty() {
                    format!("{line} was refused")
                } else {
                    outcome.error
                },
            ));
        }
        let note = outcome
            .reply
            .split('\n')
            .rfind(|reply| !reply.is_empty())
            .map_or_else(|| line.clone(), String::from);
        self.tool.set_note(&note);
        frame_views(host);
        Ok(())
    }

    fn copy(&mut self, host: &mut dyn Host) {
        let mut lines = vec![COURSE_COLUMNS.join("\t")];
        lines.extend(self.form.courses.iter().map(|row| row.join("\t")));
        let text = lines.join("\n") + "\n";
        if let Err(error) = host.clipboard().set_text(&text) {
            self.tool.show_error(host, &error);
            return;
        }
        self.tool
            .set_note(&format!("{} courses copied.", self.form.courses.len()));
    }

    fn save(
        &mut self,
        host: &mut dyn Host,
        title: &str,
        suggested: &str,
        filter: (&str, &str),
        courses: bool,
    ) {
        if self.report.is_none() {
            let error = Error::new(ErrorCode::InvalidState, "compute the report first");
            self.tool.show_error(host, &error);
            return;
        }
        if host.headless() {
            let error = Error::new(
                ErrorCode::InvalidState,
                "a headless session opens no file dialog; PARCEL id and PARCEL id LEGAL give the \
                 same text",
            );
            self.tool.show_error(host, &error);
            return;
        }
        let extensions = [filter.1];
        let Some(path) = save_file_dialog(title, suggested, &[(filter.0, &extensions[..])]) else {
            return;
        };
        let written = if courses {
            self.save_courses_csv(&path)
        } else {
            self.save_legal_text(&path)
        };
        match written {
            Err(error) => self.tool.show_error(host, &error),
            Ok(()) => self.tool.set_note(&format!(
                "Saved to {}.",
                String::from_utf8_lossy(&path_to_utf8(&path))
            )),
        }
    }

    /// What Save CSV writes, to `path`, without asking where. InvalidState before a report has
    /// been computed.
    pub fn save_courses_csv(&self, path: &Path) -> Status {
        let Some(report) = &self.report else {
            return Err(Error::new(
                ErrorCode::InvalidState,
                "compute the report first",
            ));
        };
        write_text(path, &parcel_courses_csv(report))
    }

    /// What the legal Save writes, to `path`, without asking where. InvalidState before a report
    /// has been computed.
    pub fn save_legal_text(&self, path: &Path) -> Status {
        if self.report.is_none() {
            return Err(Error::new(
                ErrorCode::InvalidState,
                "compute the report first",
            ));
        }
        write_text(path, &format!("{}\n", self.form.legal))
    }
}
