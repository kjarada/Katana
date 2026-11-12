//! The twins of tests/qt_widgets/survey/test_survey_jobs_dialog.cpp: Survey > Survey Jobs, driven
//! by its object names - a job imported with a network adjustment is edited, previewed (the new
//! report beside the old, each point's shift listed), applied as one undo step and undone; its
//! report is exported as HTML, text and PDF; it is removed after a confirmation on the page; and
//! the list reads a job's last adjustment back from its report.

use katana_cad::linework_verbs::survey_import_finish;
use katana_cad::survey_job::{
    ImportSurveyJobCommand, SurveyJobImport, default_reduction, reduction_context_for,
};
use katana_cad::{Document, SurveyImportOptions};
use katana_commands::delete_entities;
use katana_entity::survey_map::{SurveyBreakline, SurveyMap, SurveyRule};
use katana_entity::{Color, Entity, Geometry, PropertyValue};
use katana_survey::reduction_report::{
    AdjustmentReport, ReductionReport, ReportGlobalTest, ReportResidual, ResectionReport,
    render_text,
};
use katana_survey::{
    AdjustmentMethod, ControlConstraint, ControlOrigin, ControlPoint, ControlSelection,
};
use katana_surveyio::{ReadOptions, format_registry, read_survey};
use katana_window::RecordingHost;
use katana_window::drive::Harness;
use katana_window::survey::reduction_report_view::display_report;
use katana_window::survey::survey_job_support::{adjustment_from_report_text, report_input_for};
use katana_window::survey::survey_jobs_dialog::{SurveyJobsDialog, viewable_report_html};

use crate::window_support::Scratch;
use crate::{cells, logged, path_text, survey_ui_data};

const DIALOG: &str = "surveyJobsDialog";

/// network_metres.rw5 imported as a job: a setup on A backsighting B and a setup on B
/// backsighting A, each shooting P and Q, with A and B held fixed in a horizontal network - what
/// the wizard does, through the same command. `coded`: the points are coded as they are drawn, as
/// SURVEY IMPORT ... CODES on LINEWORK off codes them (survey_import_finish), so the job
/// remembers that it was and a re-adjustment codes what it draws.
fn import_network_job(document: &mut Document, coded: bool) -> String {
    let name = "network_metres.rw5";
    let bytes = std::fs::read(survey_ui_data(name)).expect("the fixture");
    let read = read_survey(
        format_registry(),
        "tds-rw5",
        &bytes,
        name,
        &ReadOptions::default(),
    )
    .unwrap_or_else(|error| panic!("{}", error.describe()));
    let import_options = SurveyImportOptions::default();
    let mut request = SurveyJobImport {
        raw: read.project.clone(),
        import_options: import_options.clone(),
        ..SurveyJobImport::default()
    };
    request.job.name = String::from(name);
    request.job.format_id = read.format_id.clone();
    request.job.parser_version = read.parser_version.clone();
    request.job.source_file_name = String::from(name);
    request.job.source_bytes = bytes;
    request.job.settings.method = AdjustmentMethod::Network;
    request.job.settings.control = vec![
        ControlSelection {
            point: ControlPoint::fixed3d("A"),
            origin: ControlOrigin::File,
        },
        ControlSelection {
            point: ControlPoint::fixed3d("B"),
            origin: ControlOrigin::File,
        },
    ];
    request.job.layer = import_options.layer;
    request.context = reduction_context_for(document).expect("a context");
    request.context.input = report_input_for(&read, name);
    request.context.created_utc = String::from("2026-09-24T10:00:00Z");
    if coded {
        request.finish = survey_import_finish(document, Some(true), Some(false)).options;
    }
    let command = ImportSurveyJobCommand::new(document, request, Some(default_reduction()));
    let results = command.results();
    document
        .execute(Box::new(command))
        .unwrap_or_else(|error| panic!("{}", error.describe()));
    results.job_id()
}

/// The drawn point whose number (the property "point", as an import writes it) is `number`.
fn point_numbered(document: &Document, number: &str) -> Option<Entity> {
    let mut found = None;
    document.model().entities.for_each(|entity| {
        if !matches!(entity.geometry, Geometry::Point(_)) {
            return;
        }
        if let Some(PropertyValue::Text(text)) = entity.properties.get("point")
            && text == number
        {
            found = Some(entity.clone());
        }
    });
    found
}

struct Session {
    harness: Harness,
    host: RecordingHost,
    dialog: SurveyJobsDialog,
    job_id: String,
}

impl Session {
    fn new() -> Self {
        let mut document = Document::new(None);
        let job_id = import_network_job(&mut document, false);
        Self::over(document, job_id)
    }

    fn over(document: Document, job_id: String) -> Self {
        let mut dialog = SurveyJobsDialog::new();
        dialog.show();
        let mut session = Self {
            harness: Harness::new(),
            host: RecordingHost::with_document(document),
            dialog,
            job_id,
        };
        session.harness.target(DIALOG);
        session.run();
        session
    }

    fn run(&mut self) {
        let dialog = &mut self.dialog;
        let host = &mut self.host;
        if let Err(error) = self.harness.run(|ui| dialog.ui(ui, &mut *host)) {
            panic!("{error}");
        }
    }

    fn fill(&mut self, name: &str, text: &str) {
        self.harness.fill(name, text);
        self.run();
    }

    fn click(&mut self, name: &str) {
        self.harness.press(name);
        self.run();
    }

    fn report(&self, name: &str) -> String {
        self.harness
            .report(name)
            .unwrap_or_else(|error| panic!("{error}"))
    }

    fn rows(&self, name: &str) -> Vec<Vec<String>> {
        self.harness
            .control(name)
            .unwrap_or_else(|| panic!("no {name}"))
            .items
            .iter()
            .map(|row| cells(row.as_str()))
            .collect()
    }

    fn logged(&self, pattern: &str) -> bool {
        logged(&self.host, pattern)
    }

    fn document(&self) -> &Document {
        self.host.document()
    }
}

fn global_test(statistic: f64, lower: f64, upper: f64) -> ReportGlobalTest {
    ReportGlobalTest {
        statistic,
        lower_critical: lower,
        upper_critical: upper,
        significance_level: 0.05,
        passed: true,
    }
}

mod survey_jobs_dialog {
    use super::*;

    #[test]
    fn the_list_shows_each_job_with_its_method_points_and_last_adjustment() {
        let session = Session::new();
        let rows = session.rows("jobs");
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row[0], session.job_id);
        assert_eq!(row[1], "network_metres.rw5");
        assert_eq!(row[4], "network, horizontal");
        assert_eq!(row[5], "4"); // A, B, P, Q
        // The variance factor and global test as the job's own report states them.
        let job = &session.document().survey_jobs()[0];
        let last = adjustment_from_report_text(&job.report_text);
        let variance = last.variance_factor.expect("a variance factor");
        assert_eq!(row[6], variance);
        assert_eq!(
            row[7],
            last.global_test.unwrap_or_else(|| String::from("-"))
        );
    }

    #[test]
    fn edit_preview_apply_is_one_undo_step_and_the_new_report_is_shown_beside_the_old() {
        let mut session = Session::new();
        let before = session.document().survey_jobs()[0].clone();
        session.click("editAdjustment");
        // B no longer fixed but weighted 20 mm: the network is free to move it.
        session.fill("controlPick", "B");
        session.fill("controlHorizontal", "weighted");
        session.fill("controlSigmaHorizontal", "20");
        session.click("addControl");
        session.click("previewAdjustment");

        assert_eq!(
            session.rows("shifts").len(),
            4,
            "a shift for each of A, B, P and Q"
        );
        let summary = session.report("shiftSummary");
        assert!(summary.contains("against the job's last run"), "{summary}");
        assert!(session.harness.control("newReportSections").is_some());
        assert!(session.harness.control("oldReportSections").is_some());
        // The preview changes nothing yet.
        assert_eq!(session.document().survey_jobs()[0], before);

        session.click("applyAdjustment");
        let after = session.document().survey_jobs()[0].clone();
        assert_eq!(after.settings.control.len(), 2);
        assert_eq!(
            after.settings.control[1].point.northing.constraint,
            ControlConstraint::Weighted
        );
        assert_eq!(after.settings.control[1].point.northing.sigma, 0.020);
        assert_ne!(after.report_text, before.report_text);
        assert!(
            session.logged(&format!("Re-adjusted survey job {}", session.job_id)),
            "{:?}",
            session.host.logged().last()
        );

        // One undo puts the settings, the report and the points back.
        session.host.document_mut().undo().expect("undone");
        assert_eq!(session.document().survey_jobs()[0], before);
        session.run();
        session.run();
        let last = adjustment_from_report_text(&before.report_text);
        assert_eq!(
            session.rows("jobs")[0][6],
            last.variance_factor.unwrap_or_else(|| String::from("-"))
        );
    }

    #[test]
    fn the_report_is_exported_as_html_text_and_pdf() {
        let mut session = Session::new();
        let folder = Scratch::new("the_report_is_exported_as_html_text_and_pdf");
        let job = session.document().survey_jobs()[0].clone();
        session.fill("exportFile", &path_text(&folder.path("report.html")));
        session.click("exportReport");
        assert_eq!(
            std::fs::read_to_string(folder.path("report.html")).expect("written"),
            job.report_html
        );
        // No extension: the format chosen beside it names one.
        session.fill("exportFile", &path_text(&folder.path("report")));
        session.fill("exportFormat", "plain text");
        session.click("exportReport");
        assert_eq!(
            std::fs::read_to_string(folder.path("report.txt")).expect("written"),
            job.report_text
        );
        session.fill("exportFile", &path_text(&folder.path("report.pdf")));
        session.click("exportReport");
        let pdf = std::fs::read(folder.path("report.pdf")).expect("written");
        assert!(
            pdf.starts_with(b"%PDF-"),
            "{}",
            String::from_utf8_lossy(&pdf[..pdf.len().min(16)])
        );
        assert!(session.logged("as PDF to report.pdf"));
    }

    #[test]
    fn removing_a_job_is_asked_on_the_page_and_is_one_undo_step() {
        let mut session = Session::new();
        let entities = session.document().survey_jobs()[0].placed_points.len();
        session.click("removeJob");
        let question = session
            .harness
            .control("removeQuestion")
            .expect("the question");
        assert!(question.visible);
        assert!(question.text.contains(&session.job_id));
        // Keep It changes nothing.
        session.click("cancelRemove");
        assert!(
            !session
                .harness
                .control("removeQuestion")
                .expect("the question")
                .visible
        );
        assert_eq!(session.document().survey_jobs().len(), 1);

        session.click("removeJob");
        session.fill("removeWithPoints", "on");
        session.click("confirmRemove");
        assert!(session.document().survey_jobs().is_empty());
        assert!(session.logged(&format!(
            "Removed survey job {} (network_metres.rw5) with its points",
            session.job_id
        )));
        session.run();
        assert!(session.rows("jobs").is_empty());
        session.host.document_mut().undo().expect("undone");
        assert_eq!(session.document().survey_jobs().len(), 1);
        assert_eq!(
            session.document().survey_jobs()[0].placed_points.len(),
            entities
        );
    }

    #[test]
    fn the_last_adjustment_is_read_back_from_the_reports_own_text() {
        // A report with one adjustment, rendered by the report's own renderer: variance factor
        // 1.25 (exact in binary) prints to three places as 1.250, and 0.5 as 0.500; the global
        // test as passed or FAILED.
        let adjustment = AdjustmentReport {
            method: String::from("network least squares (horizontal)"),
            redundancy: 4,
            variance_factor: Some(1.25),
            global_test: Some(global_test(5.0, 0.484, 11.143)),
            ..AdjustmentReport::default()
        };
        let mut report = ReductionReport {
            adjustments: vec![adjustment.clone()],
            ..ReductionReport::default()
        };
        let line = adjustment_from_report_text(&render_text(&report));
        assert_eq!(line.variance_factor.as_deref(), Some("1.250"));
        assert_eq!(line.global_test.as_deref(), Some("passed"));

        if let Some(test) = &mut report.adjustments[0].global_test {
            test.passed = false;
        }
        let mut second = adjustment;
        second.variance_factor = Some(0.5);
        report.adjustments.push(second);
        let two = adjustment_from_report_text(&render_text(&report));
        assert_eq!(two.variance_factor.as_deref(), Some("1.250 / 0.500"));
        assert_eq!(two.global_test.as_deref(), Some("FAILED / passed"));
        // Radiation: nothing to read back.
        let none = adjustment_from_report_text(&render_text(&ReductionReport::default()));
        assert!(none.variance_factor.is_none());
        assert!(none.global_test.is_none());
    }

    #[test]
    fn a_resections_least_squares_is_not_read_back_as_the_jobs_adjustment() {
        // A job reduced by radiation whose reduction resected one setup: the resection's variance
        // factor and global test are its own, not the job's last adjustment, so the list reads
        // none back; and the screen's cut of the report keeps the resection, with its residuals.
        let resection = ResectionReport {
            station_id: String::from("S1"),
            point_id: String::from("R"),
            targets: vec![String::from("A"), String::from("B"), String::from("C")],
            horizontal: AdjustmentReport {
                method: String::from("resection at S1 (horizontal)"),
                redundancy: 3,
                variance_factor: Some(1.25),
                global_test: Some(global_test(3.75, 0.216, 9.348)),
                residuals: vec![ReportResidual::default(); 6],
                ..AdjustmentReport::default()
            },
            ..ResectionReport::default()
        };
        let report = ReductionReport {
            resections: vec![resection],
            ..ReductionReport::default()
        };
        let text = render_text(&report);
        assert!(text.contains("resection at S1 (horizontal)"));
        let line = adjustment_from_report_text(&text);
        assert!(line.variance_factor.is_none());
        assert!(line.global_test.is_none());

        let shown = display_report(&report, 4);
        assert_eq!(shown.resections.len(), 1);
        assert_eq!(shown.resections[0].horizontal.residuals.len(), 6);
    }

    #[test]
    fn a_stored_report_too_long_to_lay_out_is_cut_at_a_row_and_says_so() {
        let mut html = String::from("<html><body><table><tbody>");
        for _ in 0..1000 {
            html.push_str("<tr><td>row</td></tr>");
        }
        html.push_str("</tbody></table></body></html>");
        let cut = viewable_report_html(&html, 1000);
        assert!(cut.len() < 1400);
        // Cut after a whole row, then closed, with a line saying what to do.
        let end = cut.find("</tbody>").expect("closed");
        assert_eq!(&cut[end - 5..end], "</tr>");
        assert!(cut.contains("export it"));
        assert_eq!(viewable_report_html(&html, html.len()), html);
    }

    /// The dialog re-adjusts as the import reduces (reduce_for_drawing): B held from the drawing
    /// where the drawing has it twice - the job's own B, and another FORWARD put at the origin -
    /// would sit the job on whichever was drawn first, so the preview refuses, naming both, and
    /// the job is as it was.
    #[test]
    fn its_preview_refuses_to_hold_a_point_the_drawing_has_at_two_places() {
        let mut session = Session::new();
        {
            let parts = &mut session.host.parts;
            parts
                .interpreter
                .run(&mut parts.document, "FORWARD 0,-1,0 0 1 0 B")
                .unwrap_or_else(|error| panic!("{}", error.describe()));
        }
        let before = session.document().survey_jobs()[0].clone();
        session.click("editAdjustment");
        session.fill("controlFrom", "the drawing");
        session.fill("controlPick", "B");
        session.click("addControl");
        session.click("previewAdjustment");
        let message = session.report("message");
        assert!(
            message.contains("Control point B is on the drawing 2 times, at "),
            "{message}"
        );
        assert!(
            message.contains(" and E 0.0000 N 0.0000 Z 0.0000, so which one to hold is not known"),
            "{message}"
        );
        assert_eq!(session.document().survey_jobs()[0], before);
    }

    /// A job imported CODED is coded again where its re-adjustment draws a point: here P,
    /// deleted by hand and drawn again because Apply was told to overwrite hand edits.
    /// network_metres.rw5 shoots P and Q with the code PEG, and the one rule gives PEG a layer,
    /// a linestyle and a colour BY NAME - "red", a standard name. By hand: the import makes ONE
    /// style for the two, named after the linestyle and coloured #FF0000. A point drawn again
    /// must wear that style, not a second one, "Peg Line (red)".
    #[test]
    fn a_point_its_readjustment_draws_again_wears_the_style_its_neighbours_wear() {
        let mut document = Document::new(None);
        let mut map = SurveyMap::new();
        map.add(SurveyRule {
            key: String::from("PEG*"),
            model: String::from("SURVEY PEG"),
            linestyle: String::from("Peg Line"),
            colour: String::from("red"),
            breakline: Some(SurveyBreakline::Line),
            ..SurveyRule::default()
        })
        .expect("the rule is taken");
        document.set_survey_map(map);
        let job_id = import_network_job(&mut document, true);

        let style = document
            .model()
            .styles
            .find("Peg Line")
            .expect("the import coded its points");
        assert_eq!(
            style.color,
            Some(Color {
                r: 255,
                g: 0,
                b: 0,
                a: 255
            })
        );
        assert_eq!(document.model().styles.all().len(), 1);
        for number in ["P", "Q"] {
            let point = point_numbered(&document, number).expect("the point");
            assert_eq!(point.layer, "SURVEY PEG", "{number}");
            assert_eq!(point.style, "Peg Line", "{number}");
        }
        let first = point_numbered(&document, "P").expect("P").id;
        document
            .execute(delete_entities(vec![first]))
            .expect("deleted");
        assert!(point_numbered(&document, "P").is_none());

        let mut session = Session::over(document, job_id);
        session.click("editAdjustment");
        session.fill("handEdits", "overwrite them with the new coordinates");
        session.click("previewAdjustment");
        session.click("applyAdjustment");

        let again = point_numbered(session.document(), "P").unwrap_or_else(|| {
            panic!("{:?}", session.host.logged().last());
        });
        assert_ne!(again.id, first, "a point drawn now, not the one deleted");
        assert_eq!(again.layer, "SURVEY PEG");
        assert_eq!(again.style, "Peg Line");
        let styles: Vec<String> = session
            .document()
            .model()
            .styles
            .all()
            .into_iter()
            .map(|each| each.name)
            .collect();
        assert_eq!(styles, ["Peg Line"]);
    }
}
