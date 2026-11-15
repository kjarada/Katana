//! The twins of tests/qt_widgets/survey/test_parcel_dialog.cpp: Survey > Coordinate Geometry >
//! Parcel Report (katana_window::survey::survey_parcel_dialog), driven by its object names as a
//! person drives it.
//!
//! The report is katana_cad's parcel_report and its words format_parcel_report's and
//! legal_description's, which cad's own tests pin on the same rectangle by hand; what is pinned
//! here is that the dialog shows exactly those, and that Label Courses is the line PARCEL <id>
//! LABEL <height> run through the executor - here the real interpreter on the same drawing - as
//! one undo step.

use katana_cad::parcel::{ParcelReport, legal_description, parcel_courses_csv, parcel_report};
use katana_commands::create_polyline;
use katana_entity::{EntityId, Geometry};
use katana_geometry::{Point2, Polyline2};
use katana_window::RecordingHost;
use katana_window::drive::Harness;
use katana_window::host::Host;
use katana_window::survey::survey_parcel_dialog::SurveyParcelDialog;

use crate::cells;
use crate::window_support::{Scratch, interpreter_host};

const DIALOG: &str = "surveyParcelReportDialog";

/// The 100 by 50 rectangle of cad's parcel tests, drawn counter-clockwise from the origin, as the
/// drawing's only entity; the dialog shown, its runner the interpreter on the same drawing (or
/// none).
struct Fixture {
    harness: Harness,
    host: RecordingHost,
    dialog: SurveyParcelDialog,
    id: EntityId,
}

impl Fixture {
    fn new(with_runner: bool, headless: bool) -> Self {
        let mut host = interpreter_host(katana_cad::Document::new(None));
        host.parts.headless = headless;
        let boundary = Polyline2 {
            vertices: vec![
                Point2::new(0.0, 0.0),
                Point2::new(100.0, 0.0),
                Point2::new(100.0, 50.0),
                Point2::new(0.0, 50.0),
            ],
            closed: true,
        };
        let attributes = host.document().current_attributes();
        host.document_mut()
            .execute(create_polyline(boundary, attributes))
            .expect("the rectangle is drawn");
        let id = *host
            .document()
            .model()
            .entities
            .ids()
            .last()
            .expect("an entity");
        let mut dialog = SurveyParcelDialog::new(with_runner);
        dialog.show();
        let mut fixture = Self {
            harness: Harness::new(),
            host,
            dialog,
            id,
        };
        fixture.harness.target(DIALOG);
        fixture.run();
        fixture
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

    fn message(&self) -> String {
        self.report("message")
    }

    fn parcel_report(&self) -> ParcelReport {
        let entity = self
            .host
            .document()
            .model()
            .entities
            .find(self.id)
            .expect("the rectangle");
        let Geometry::Polyline(boundary) = &entity.geometry else {
            panic!("the rectangle is a polyline");
        };
        parcel_report(boundary).expect("a report")
    }

    fn entities(&self) -> usize {
        self.host.document().model().entities.size()
    }

    /// What the interpreter prints for `line` on the same drawing.
    fn verb(&mut self, line: &str) -> String {
        let parts = &mut self.host.parts;
        parts
            .interpreter
            .run(&mut parts.document, line)
            .unwrap_or_else(|error| panic!("{}", error.describe()))
    }
}

mod parcel_dialog {
    use super::*;

    #[test]
    fn use_selection_takes_the_selected_closed_polyline_and_refuses_anything_else() {
        let mut f = Fixture::new(true, false);
        f.click("useSelection");
        assert!(
            f.message().contains("select the one closed polyline"),
            "{}",
            f.message()
        );
        let id = f.id;
        f.host.document_mut().selection_mut().set(vec![id]);
        f.click("useSelection");
        assert_eq!(f.report("parcel"), format!("{id}"));
    }

    #[test]
    fn compute_shows_the_courses_summary_and_report_the_parcel_verb_prints() {
        let mut f = Fixture::new(true, false);
        let id = f.id;
        f.fill("parcel", &format!("{id}"));
        f.click("compute");
        let report = f.parcel_report();

        let courses = f
            .harness
            .control("parcelCourses")
            .expect("the courses table");
        assert_eq!(courses.items.len(), 4);
        let first = cells(&courses.items[0]);
        let second = cells(&courses.items[1]);
        assert_eq!(first[0], "1");
        assert_eq!(first[3], report.courses[0].bearing);
        assert_eq!(first[4], "100.000");
        assert_eq!(second[1], "100.000"); // from (100, 0)
        assert_eq!(second[4], "50.000");
        assert_eq!(
            f.report("parcelSummary"),
            katana_cad::parcel::format_parcel_summary(&report)
        );
        // The pane and the log carry what PARCEL id prints, word for word.
        let verb = f.verb(&format!("PARCEL {id}"));
        assert_eq!(f.report("result"), verb);
        let (last, _) = f.host.logged().last().cloned().expect("something logged");
        assert_eq!(last, verb);
    }

    #[test]
    fn the_legal_description_is_parcellega_ls_with_the_name_typed() {
        let mut f = Fixture::new(true, false);
        let id = f.id;
        f.fill("parcel", &format!("#{id}"));
        f.click("compute");
        // No name typed: the verb's default, "Parcel <id>".
        let legal = f.verb(&format!("PARCEL {id} LEGAL"));
        assert_eq!(f.report("parcelLegalText"), legal);
        f.fill("parcelName", "Lot7");
        let named = f.verb(&format!("PARCEL {id} LEGAL Lot7"));
        assert_eq!(f.report("parcelLegalText"), named);
    }

    #[test]
    fn label_courses_runs_the_parcel_label_line_as_one_undo_step_and_names_the_layer() {
        let mut f = Fixture::new(true, false);
        let id = f.id;
        f.fill("parcel", &format!("{id}"));
        f.fill("labelHeight", "2");
        let before = f.entities();
        let undo = f.host.document().history().undo_count();
        f.click("label");
        assert_eq!(f.host.lines(), [format!("PARCEL {id} LABEL 2")]);
        // Four courses and the area.
        assert_eq!(f.entities(), before + 5);
        assert_eq!(f.host.document().history().undo_count(), undo + 1);
        assert_eq!(
            f.message(),
            "5 labels created on layer 0, the current layer"
        );
        f.host.document_mut().undo().expect("undone");
        assert_eq!(f.entities(), before);
    }

    #[test]
    fn what_is_not_a_parcel_is_refused_in_the_verbs_words() {
        let mut f = Fixture::new(true, false);
        f.click("compute");
        assert!(
            f.message().contains("type the id of a closed polyline"),
            "{}",
            f.message()
        );
        f.verb("LINE 0,0 10,0");
        let line = *f
            .host
            .document()
            .model()
            .entities
            .ids()
            .last()
            .expect("the line");
        f.fill("parcel", &format!("{line}"));
        f.click("compute");
        assert!(
            f.message().contains("a parcel must be a closed polyline"),
            "{}",
            f.message()
        );
        f.fill("parcel", "999999");
        f.click("compute");
        assert!(
            f.message().contains("no entity with that id"),
            "{}",
            f.message()
        );
        // A refused label runs its line and says why; nothing is added.
        let before = f.entities();
        let id = f.id;
        f.fill("parcel", &format!("{id}"));
        f.fill("labelHeight", "0");
        f.click("label");
        assert!(
            f.message().contains("height must be positive"),
            "{}",
            f.message()
        );
        assert_eq!(f.entities(), before);
    }

    #[test]
    fn without_an_executor_it_says_it_cannot_label_and_changes_nothing() {
        let mut f = Fixture::new(false, false);
        let id = f.id;
        f.fill("parcel", &format!("{id}"));
        let before = f.entities();
        f.click("label");
        assert!(f.message().contains("no command line"), "{}", f.message());
        assert_eq!(f.entities(), before);
    }

    #[test]
    fn the_csv_and_the_legal_text_are_saved_as_the_functions_write_them() {
        let mut f = Fixture::new(true, false);
        let folder =
            Scratch::new("the_csv_and_the_legal_text_are_saved_as_the_functions_write_them");
        // Nothing to save before a report.
        assert!(
            f.dialog
                .save_courses_csv(&folder.path("early.csv"))
                .is_err()
        );
        let id = f.id;
        f.fill("parcel", &format!("{id}"));
        f.click("compute");
        let report = f.parcel_report();

        let csv = folder.path("courses.csv");
        f.dialog.save_courses_csv(&csv).expect("saved");
        assert_eq!(
            std::fs::read_to_string(csv).expect("read back"),
            parcel_courses_csv(&report)
        );

        let legal = folder.path("legal.txt");
        f.dialog.save_legal_text(&legal).expect("saved");
        assert_eq!(
            std::fs::read_to_string(legal).expect("read back"),
            legal_description(&report, &format!("Parcel {id}")) + "\n"
        );

        f.click("parcelCopy");
        let copied = f.host.clipboard().text().expect("text copied");
        assert!(
            copied.starts_with("Course\tFrom E\tFrom N\tBearing\tDistance\n1\t0.000\t0.000\t"),
            "{copied}"
        );
        assert_eq!(copied.matches('\n').count(), 5);
    }

    #[test]
    fn saving_in_a_headless_session_opens_no_file_dialog() {
        let mut f = Fixture::new(true, true);
        let id = f.id;
        f.fill("parcel", &format!("{id}"));
        f.click("compute");
        f.click("parcelCsv");
        assert!(f.message().contains("headless"), "{}", f.message());
        f.click("parcelLegalSave");
        assert!(f.message().contains("headless"), "{}", f.message());
    }
}
