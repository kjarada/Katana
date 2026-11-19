//! Survey > Export Survey Points (katana_window::survey::survey_points_ui::SurveyExportDialog):
//! its Export builds the SURVEY EXPORT line its fields say and hands it to the window's command
//! executor, as every dialog does (CLAUDE.md section 1: one code path, logged), where it wrote the
//! file itself until 2026-11-19. The executor here is the session's own survey verbs
//! (katana_app::survey_verbs::run_survey_line) on the host's drawing, which is where the window's
//! command line sends a SURVEY line; the file read back is what the verb wrote.

use katana_app::survey_verbs::run_survey_line;
use katana_cad::Document;
use katana_window::RecordingHost;
use katana_window::drive::Harness;
use katana_window::host::VerbOutcome;
use katana_window::host::recording::{Answer, HostParts};
use katana_window::survey::survey_points_ui::SurveyExportDialog;

use crate::window_support::Scratch;

const DIALOG: &str = "surveyExportDialog";

/// The dialog shown over a drawing of two survey points - 1 at (100, 200, 10), 2 at (110, 205.5)
/// with no height - imported by the line's own LAYOUT, its executor the session's survey verbs.
struct Fixture {
    harness: Harness,
    host: RecordingHost,
    dialog: SurveyExportDialog,
}

impl Fixture {
    fn new(scratch: &Scratch) -> Self {
        let mut document = Document::new(None);
        let points = scratch.file("points.csv", "1,100,200,10\n2,110,205.5,\n");
        let line = format!(
            "SURVEY IMPORT \"{}\" LAYOUT \"P,E,N,Z;delimiter=comma;header=0\" UNIT metres",
            points.to_string_lossy()
        );
        run_survey_line(&mut document, &line).expect("the two points are imported");
        let host = RecordingHost::with_document(document).answering(Answer::Custom(Box::new(
            |parts: &mut HostParts, line: &str| match run_survey_line(&mut parts.document, line) {
                Ok(reply) => VerbOutcome::new(true, &reply, ""),
                Err(error) => VerbOutcome::new(false, "", &error.describe()),
            },
        )));
        let mut dialog = SurveyExportDialog::new();
        dialog.show();
        let mut fixture = Self {
            harness: Harness::new(),
            host,
            dialog,
        };
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
        self.harness.target(DIALOG).fill(name, text);
        self.run();
    }

    fn press(&mut self, name: &str) {
        self.harness.target(DIALOG).press(name);
        self.run();
    }
}

mod survey_export_dialog {
    use super::*;

    #[test]
    fn export_hands_the_survey_export_line_to_the_executor_and_says_what_it_wrote() {
        let scratch = Scratch::new("survey_export_dialog_line");
        let mut f = Fixture::new(&scratch);
        let out = scratch.path("out.csv");
        let path = out.to_string_lossy().into_owned();
        f.fill("file", &path);
        f.fill("columns", "P,E,N,Z");
        f.fill("decimals", "2");
        f.press("export");

        // The line a person would type: the dialog's choices as the verb's words, one for one
        // (docs/survey.md, "Point files on the command line"). The scratch folder has no blank,
        // so the path is one word as it is.
        assert!(!path.contains(' '), "{path}");
        assert_eq!(
            f.host.lines(),
            [format!(
                "SURVEY EXPORT {path} LAYOUT \"P,E,N,Z;delimiter=comma;header=1;quote=double\" \
                 DECIMALS 2 UNIT metres"
            )]
        );
        // The file is the verb's: its rows after the header, worked by hand - two places, the
        // missing height an empty field, never 0.
        let written = std::fs::read_to_string(&out).expect("the file is written");
        let rows: Vec<&str> = written.lines().skip(1).collect();
        assert_eq!(rows, ["1,100.00,200.00,10.00", "2,110.00,205.50,"]);
        // Said in the dialog's sentence, from the verb's reply.
        let said = f.dialog.tool.pane.result.clone();
        assert!(said.starts_with("Exported 2 points to out.csv - layout "), "{said}");
        assert!(said.contains(", 2 decimals, in "), "{said}");
        assert!(
            said.contains("1 point has no elevation and was written with an empty field."),
            "{said}"
        );
    }

    #[test]
    fn selected_points_are_the_selection_scope_and_an_empty_selection_runs_nothing() {
        let scratch = Scratch::new("survey_export_dialog_selection");
        let mut f = Fixture::new(&scratch);
        let out = scratch.path("selected.csv");
        f.fill("file", &out.to_string_lossy());
        f.fill("scope", "Selected survey points");
        let line = f.dialog.export_line().expect("a line");
        assert!(line.ends_with(" UNIT metres SELECTION"), "{line}");
        // Nothing is selected: refused in the dialog's words before any line runs.
        f.press("export");
        assert!(f.host.lines().is_empty());
        assert!(f.dialog.tool.pane.message.contains("nothing is selected"));
        assert!(!out.exists());
    }

    #[test]
    fn a_form_the_line_cannot_carry_is_refused_and_runs_nothing() {
        let scratch = Scratch::new("survey_export_dialog_refused");
        let mut f = Fixture::new(&scratch);
        // No file.
        f.press("export");
        assert!(f.dialog.tool.pane.message.contains("choose the file to write"));
        // Decimals out of range.
        f.fill("file", &scratch.path("x.csv").to_string_lossy());
        f.fill("decimals", "13");
        f.press("export");
        assert!(f.dialog.tool.pane.message.contains("from 0 to 12"));
        assert!(f.host.lines().is_empty());
    }
}
