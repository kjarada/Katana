//! Edit > Select by ID (src/katana_window/src/select_by_id_dialog.rs): test_plan_context_menu's
//! SelectById and SelectByIdDialog suites (the plan view's shortcut menu is VP's).
//!
//! The runner here is the window's in miniature: each line is kept, then run by the interpreter on
//! the same drawing, and what it answered comes back as the window's runner returns it.

use std::cell::RefCell;
use std::rc::Rc;

use katana_core::ErrorCode;
use katana_entity::EntityId;
use katana_window::drive::Harness;
use katana_window::host::{Answer, Host, HostParts, RecordingHost, VerbOutcome};
use katana_window::select_by_id_dialog::{SelectByIdDialog, parse_entity_ids, select_by_id_line};

mod select_by_id {
    use super::*;

    #[test]
    fn ids_are_read_across_commas_and_blanks_with_or_without_a_hash_each_once() {
        let ids = parse_entity_ids(" 3, 5  #7,3\t12 ")
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(ids, [3, 5, 7, 12]);
    }

    #[test]
    fn a_word_that_is_not_an_id_is_refused_by_name_and_no_ids_is_refused() {
        for text in ["3, x5", "3 0", "#", "1.5", "-2"] {
            let error = parse_entity_ids(text).expect_err(text);
            assert_eq!(error.code, ErrorCode::InvalidArgument, "{text}");
        }
        assert!(
            parse_entity_ids("3, x5")
                .expect_err("x5 is no id")
                .describe()
                .contains("x5")
        );
        for text in ["", "  ", ", ,"] {
            let error = parse_entity_ids(text).expect_err(text);
            assert!(error.describe().contains("no id"), "{text}");
        }
    }

    #[test]
    fn adding_names_the_current_selection_first_and_each_id_once() {
        assert_eq!(select_by_id_line(&[5, 2], &[2, 9], true), "SELECT 2 9 5");
        assert_eq!(select_by_id_line(&[5, 2], &[2, 9], false), "SELECT 5 2");
        assert_eq!(select_by_id_line(&[5], &[], true), "SELECT 5");
    }
}

/// A drawing, its interpreter, the runner's lines, and the dialog with what it asked to frame.
struct DialogFixture {
    host: RecordingHost,
    dialog: SelectByIdDialog,
    harness: Harness,
    /// Each on_selected, with its zoom.
    selected: Rc<RefCell<Vec<bool>>>,
}

impl DialogFixture {
    fn new() -> Self {
        // The window's runner: the line kept (the host's lines), run by the interpreter on the
        // drawing, its reply trimmed.
        let host = RecordingHost::new().answering(Answer::Custom(Box::new(
            |parts: &mut HostParts, line: &str| match parts
                .interpreter
                .run(&mut parts.document, line)
            {
                Ok(reply) => VerbOutcome::new(true, reply.trim(), ""),
                Err(error) => VerbOutcome::new(false, "", &error.describe()),
            },
        )));
        let selected = Rc::new(RefCell::new(Vec::new()));
        let mut dialog = SelectByIdDialog::new();
        let told = Rc::clone(&selected);
        dialog.on_selected = Some(Box::new(move |zoom: bool, _host: &mut dyn Host| {
            told.borrow_mut().push(zoom);
        }));
        let mut harness = Harness::new();
        harness.target("selectByIdDialog");
        let mut fixture = Self {
            host,
            dialog,
            harness,
            selected,
        };
        fixture.draw();
        fixture
    }

    fn draw(&mut self) {
        let Self {
            host,
            dialog,
            harness,
            ..
        } = self;
        harness
            .show(|ui| dialog.ui(ui, &mut *host))
            .expect("the dialog is drawn");
    }

    /// A line run on the drawing directly, not through the runner.
    fn run(&mut self, line: &str) {
        let parts = &mut self.host.parts;
        parts
            .interpreter
            .run(&mut parts.document, line)
            .unwrap_or_else(|error| panic!("{line}: {}", error.describe()));
    }

    fn select(&mut self, ids: &str) {
        let Self {
            host,
            dialog,
            harness,
            ..
        } = self;
        harness
            .fill("selectByIdIds", ids)
            .press("selectByIdSelect")
            .run(|ui| dialog.ui(ui, &mut *host))
            .expect("the ids are typed and Select pressed");
    }

    fn fill(&mut self, name: &str, text: &str) {
        let Self {
            host,
            dialog,
            harness,
            ..
        } = self;
        harness
            .fill(name, text)
            .run(|ui| dialog.ui(ui, &mut *host))
            .expect("the dialog is filled");
    }

    fn status(&self) -> String {
        self.harness.report("selectByIdStatus").expect("the status")
    }

    fn selection(&self) -> Vec<EntityId> {
        self.host.parts.document.selection().ids()
    }
}

mod select_by_id_dialog {
    use super::*;

    #[test]
    fn select_runs_select_through_the_runner_and_asks_for_what_it_selected_to_be_framed() {
        let mut f = DialogFixture::new();
        assert_eq!(f.dialog.object_name(), "selectByIdDialog");
        f.run("POINT 1,1");
        f.run("POINT 2,2");
        f.run("POINT 3,3");
        assert_eq!(
            f.harness.control("selectByIdZoom").expect("Zoom").checked,
            Some(true),
            "framing is the default"
        );
        f.select("#2, 3");
        assert_eq!(f.host.lines(), ["SELECT 2 3"]);
        assert_eq!(f.selection(), [2, 3]);
        assert_eq!(f.status(), "2 selected");
        assert_eq!(*f.selected.borrow(), [true]);

        // Added to, without framing.
        f.fill("selectByIdAdd", "on");
        f.fill("selectByIdZoom", "off");
        f.select("1");
        assert_eq!(
            f.host.lines().last().map(String::as_str),
            Some("SELECT 2 3 1")
        );
        assert_eq!(f.host.parts.document.selection().size(), 3);
        assert_eq!(*f.selected.borrow(), [true, false]);
    }

    #[test]
    fn an_id_select_refuses_is_said_and_the_selection_is_left_alone() {
        let mut f = DialogFixture::new();
        f.run("POINT 1,1");
        f.run("LAYER NEW hidden");
        f.run("POINT 2,2");
        f.run("SELECT 2");
        f.run("CHLAYER hidden");
        f.run("LAYER HIDE hidden");
        f.run("SELECT 1");

        f.select("1 99");
        assert!(
            f.status().contains("entity does not exist"),
            "{}",
            f.status()
        );
        assert!(f.status().contains("99"), "{}", f.status());
        f.select("2");
        assert!(f.status().contains("hidden or locked"), "{}", f.status());
        // Not an id at all: refused before anything runs.
        f.select("kerb");
        assert!(f.status().contains("kerb"), "{}", f.status());
        assert_eq!(f.host.lines(), ["SELECT 1 99", "SELECT 2"]);
        assert_eq!(f.selection(), [1]);
        assert!(f.selected.borrow().is_empty());
    }
}
