//! The program's own cases (main.cpp's port, drive::run): the switches read in main.cpp's order,
//! each value refused with exit 2, and whole runs of the program over the small test window - a
//! screenshot of a dialog and of the window, a script batch, a refused customisation, the writes,
//! the checks and the desktop's hand-over - with what each printed.

use std::path::Path;

use katana_app::console::Captured;
use katana_cad::PaperSize;
use katana_window::canvas::read_png;
use katana_window::drive::desktop::TITLE;
use katana_window::drive::run::{Switches, read_switches, run};
use katana_window::drive::steps::{Step, StepKind};

use crate::test_window::{Log, TestWindow, new_log};
use crate::window_support::Scratch;

mod program {
    use super::*;

    fn arguments(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| String::from(*word)).collect()
    }

    fn known(format: &str) -> bool {
        ["pdf", "pdfs", "png", "tiff"].contains(&format)
    }

    fn read(words: &[&str]) -> (Result<Switches, i32>, String) {
        let mut console = Captured::default();
        let switches = read_switches(&arguments(words), known, &mut console);
        (switches, console.err)
    }

    /// A whole run of the program over a test window, which opens no desktop: its exit code, what
    /// it printed, and the window's log.
    fn run_headless(words: &[&str]) -> (i32, Captured, Log) {
        let log = new_log();
        let mut console = Captured::default();
        let code = run(
            &arguments(words),
            &mut console,
            || TestWindow::new(&log),
            |_| panic!("a run nobody watches opens no desktop"),
        );
        (code, console, log)
    }

    fn logged(log: &Log, what: &str) -> bool {
        log.borrow().iter().any(|line| line == what)
    }

    fn path_text(path: &Path) -> String {
        String::from(path.to_str().expect("the scratch folder's path is UTF-8"))
    }

    #[test]
    fn the_switches_are_read_in_main_cpps_order() {
        let (switches, said) = read(&[
            "katana",
            "project",
            "--customise",
            "a.json",
            "b.json",
            "--screenshot",
            "out.png",
            "--dialog",
            "openTest",
            "--fill",
            "field=N 45 E",
            "--enter",
            "--survey-dialog",
            "surveyInverse",
            "--attributes",
            "12",
            "--paper",
            "a2",
            "--portrait",
            "--dpi",
            "150",
            "--scale",
            "500",
            "--action",
            "formatPurge",
            "data.dxf",
        ]);
        let switches = switches.expect("every value is taken");
        assert_eq!(said, "");
        assert_eq!(switches.inputs, ["project", "data.dxf"]);
        assert_eq!(switches.customisation, ["a.json", "b.json"]);
        assert_eq!(switches.screenshot.as_deref(), Some("out.png"));
        assert_eq!(
            switches.steps,
            vec![
                Step::new(StepKind::Dialog, "openTest"),
                Step::new(StepKind::Fill, "field=N 45 E"),
                Step::enter(),
                Step::new(StepKind::Dialog, "surveyInverse"),
            ]
        );
        assert!(switches.attribute_manager);
        assert_eq!(switches.attribute_entity, 12);
        assert_eq!(switches.settings.paper, PaperSize::A2);
        assert!(!switches.settings.landscape);
        assert_eq!(switches.settings.dpi, 150.0);
        assert!(switches.dpi_given);
        assert!(!switches.fit);
        assert_eq!(switches.settings.scale_denominator, 500.0);
        assert_eq!(switches.actions, ["formatPurge"]);
        assert!(switches.headless());
        assert!(!switches.writes_only());
        assert!(!switches.script_batch());
        assert_eq!(
            switches.step_kinds(),
            ["--dialog", "--fill", "--command", "--dialog"]
        );
    }

    #[test]
    fn an_attributes_switch_takes_an_id_only_when_one_follows() {
        let (switches, _) = read(&["katana", "--attributes", "--style-manager"]);
        let switches = switches.expect("taken");
        assert!(switches.attribute_manager);
        assert_eq!(switches.attribute_entity, 0);
        assert!(switches.style_manager);
        let (switches, _) = read(&["katana", "--attributes", "0"]);
        let switches = switches.expect("taken");
        assert_eq!(switches.attribute_entity, 0);
        assert_eq!(switches.inputs, ["0"]);
    }

    #[test]
    fn a_script_without_a_screenshot_is_a_batch_and_with_one_is_a_step() {
        let (switches, _) = read(&["katana", "--script", "a.kcs"]);
        let switches = switches.expect("taken");
        assert!(switches.script_batch());
        assert!(switches.headless());
        let (switches, _) = read(&["katana", "--script", "a.kcs", "--screenshot", "out.png"]);
        assert!(!switches.expect("taken").script_batch());
        let (switches, _) = read(&["katana", "--command", "LIST"]);
        assert!(!switches.expect("taken").headless());
    }

    #[test]
    fn a_value_a_switch_cannot_take_ends_the_run_with_2_saying_why() {
        let cases: [(&[&str], &str); 8] = [
            (
                &["katana", "--format", "doc"],
                "--format must be one of pdf, pdfs, png, tiff (not 'doc')\n",
            ),
            (
                &["katana", "--plot-style", "blue"],
                "--plot-style must be one of colour, grey, mono (not 'blue')\n",
            ),
            (
                &["katana", "--line-weight-scale", "9"],
                "--line-weight-scale must be a number from 0.1 to 5 (not '9')\n",
            ),
            (
                &["katana", "--line-weight-scale", "thick"],
                "--line-weight-scale must be a number from 0.1 to 5 (not 'thick')\n",
            ),
            (
                &["katana", "--paper", "B5"],
                "--paper must be one of A0, A1, A2, A3, A4\n",
            ),
            (&["katana", "--plot"], "--plot needs an output path\n"),
            (
                &["katana", "--plot-sheets", ""],
                "--plot-sheets needs an output path\n",
            ),
            (
                &["katana", "--update-health", "../elsewhere"],
                "--update-health needs the transaction id the update helper gives it\n",
            ),
        ];
        for (words, sentence) in cases {
            let (switches, said) = read(words);
            assert_eq!(switches, Err(2), "{words:?}");
            assert_eq!(said, sentence, "{words:?}");
        }
        let (switches, said) = read(&["katana", "--sheets-json"]);
        assert_eq!(switches, Err(2));
        assert_eq!(
            said,
            "--sheets-json needs an output path, or - for stdout\n"
        );
        // The style's other spellings are cad's, and taken.
        let (switches, _) = read(&[
            "katana",
            "--plot-style",
            "greyscale",
            "--line-weight-scale",
            "0.7",
        ]);
        let switches = switches.expect("taken");
        assert_eq!(switches.line_weight_scale, Some(0.7));
        assert_eq!(switches.settings.line_weight_scale, 0.7);
    }

    #[test]
    fn a_screenshot_run_is_headless_drives_its_steps_and_writes_the_target() {
        let scratch = Scratch::new("drive-program-dialog-shot");
        let shot = path_text(&scratch.path("dialog.png"));
        let (code, console, log) = run_headless(&[
            "katana",
            "--dialog",
            "openTest",
            "--fill",
            "testField=hello",
            "--press",
            "testRun",
            "--screenshot",
            &shot,
        ]);
        assert_eq!(code, 0, "{}", console.err);
        assert_eq!(
            console.err,
            "--dialog openTest opened testDialog \"Test Dialog\"\n"
        );
        assert!(logged(&log, "headless"));
        assert!(logged(&log, "loadDefaultCustomisation"));
        assert!(logged(&log, "show"));
        assert!(!logged(&log, "restoreSession"));
        assert!(!logged(&log, "startUpdates"));
        let picture = read_png(Path::new(&shot)).expect("the screenshot is a PNG");
        // The dialog, not the window.
        assert!(picture.width() > 0 && picture.width() < 1360);
        assert!(picture.height() > 0 && picture.height() < 860);
    }

    #[test]
    fn a_screenshot_run_with_no_steps_writes_the_whole_headless_window() {
        let scratch = Scratch::new("drive-program-window-shot");
        let shot = path_text(&scratch.path("window.png"));
        let (code, console, _) = run_headless(&["katana", "--screenshot", &shot]);
        assert_eq!(code, 0, "{}", console.err);
        let bytes = std::fs::read(&shot).expect("the screenshot is written");
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
        let picture = read_png(Path::new(&shot)).expect("a PNG");
        assert_eq!((picture.width(), picture.height()), (1360, 860));
    }

    #[test]
    fn a_screenshot_run_needs_a_path_and_a_target_a_step_closed_leaves_the_window() {
        let (code, console, _) = run_headless(&["katana", "--screenshot", ""]);
        assert_eq!(code, 2);
        assert_eq!(console.err, "--screenshot needs an output path\n");
        let scratch = Scratch::new("drive-program-closed-shot");
        let shot = path_text(&scratch.path("closed.png"));
        let (code, console, _) = run_headless(&[
            "katana",
            "--dialog",
            "openTest",
            "--press",
            "testClose",
            "--screenshot",
            &shot,
        ]);
        assert_eq!(code, 0, "{}", console.err);
        assert!(
            console
                .err
                .ends_with("openTest has closed: the window is grabbed\n"),
            "{}",
            console.err
        );
        let picture = read_png(Path::new(&shot)).expect("a PNG");
        assert_eq!((picture.width(), picture.height()), (1360, 860));
    }

    #[test]
    fn a_refused_step_ends_a_screenshot_run_with_1_and_writes_nothing() {
        let scratch = Scratch::new("drive-program-refused-shot");
        let shot = scratch.path("refused.png");
        let (code, console, _) = run_headless(&[
            "katana",
            "--dialog",
            "openTest",
            "--press",
            "testLocked",
            "--screenshot",
            &path_text(&shot),
        ]);
        assert_eq!(code, 1);
        assert!(
            console
                .err
                .ends_with("--press: openTest's button testLocked is disabled\n"),
            "{}",
            console.err
        );
        assert!(!shot.exists());
    }

    #[test]
    fn the_checks_say_what_they_held_and_a_size_they_cannot_take_fails_the_run() {
        let scratch = Scratch::new("drive-program-checks");
        let shot = path_text(&scratch.path("checks.png"));
        let (code, console, _) = run_headless(&[
            "katana",
            "--check-session-restore",
            "--check-shortcuts",
            "--check-menus",
            "--check-toolbar-fit",
            "1366x768,1920x1080",
            "--check-toolbars",
            "--screenshot",
            &shot,
        ]);
        assert_eq!(code, 0, "{}", console.err);
        assert_eq!(
            console.err,
            "session restore: held\n\
             shortcuts: 12 key sequences, each reaching one thing\n\
             menus: 7 items, each with an icon and a status tip\n\
             toolbar fit 1366x768: held\n\
             toolbar fit 1920x1080: held\n\
             toolbar button EditToolBar > editUndo: sign clear of its icon\n\
             toolbars and title bars: 1 buttons with a menu, each drawing its sign clear of its icon\n"
        );
        for sizes in ["100x50", "1366x", "", ","] {
            let (code, console, _) = run_headless(&[
                "katana",
                "--check-toolbar-fit",
                sizes,
                "--screenshot",
                &shot,
            ]);
            if sizes.is_empty() {
                // An empty value is no check at all, as main.cpp's isEmpty() skips it.
                assert_eq!(code, 0, "{}", console.err);
                continue;
            }
            assert_eq!(code, 1, "{sizes}");
            let said = if sizes == "," {
                String::from("--check-toolbar-fit needs sizes like 1366x768, a comma between\n")
            } else {
                format!(
                    "--check-toolbar-fit needs sizes like 1366x768, a comma between (not '{sizes}')\n"
                )
            };
            assert_eq!(console.err, said);
        }
    }

    #[test]
    fn the_managers_and_the_gis_dialogs_are_grabbed_and_a_file_with_none_fails_the_run() {
        let scratch = Scratch::new("drive-program-managers");
        let shot = path_text(&scratch.path("manager.png"));
        let (code, console, log) =
            run_headless(&["katana", "--attributes", "12", "--screenshot", &shot]);
        assert_eq!(code, 0, "{}", console.err);
        assert!(logged(&log, "selectOnly 12"));
        assert!(logged(&log, "attributeManager"));
        let (code, _, log) = run_headless(&["katana", "--style-manager", "--screenshot", &shot]);
        assert_eq!(code, 0);
        assert!(logged(&log, "styleManager"));
        let (code, console, _) =
            run_headless(&["katana", "--dataset-info", "none", "--screenshot", &shot]);
        assert_eq!(code, 1);
        assert_eq!(console.err, "no dialog for none\n");
        let (code, console, _) = run_headless(&[
            "katana",
            "--import-options",
            "roads.gpkg",
            "--fill",
            "testField=kind=road",
            "--screenshot",
            &shot,
        ]);
        assert_eq!(code, 0, "{}", console.err);
        let (code, console, _) =
            run_headless(&["katana", "--toggle-layer", "missing", "--screenshot", &shot]);
        assert_eq!(code, 1);
        assert_eq!(
            console.err,
            "--toggle-layer failed: NotFound: no such layer [missing]\n"
        );
    }

    #[test]
    fn a_script_batch_is_headless_and_a_line_it_refuses_fails_it_with_1() {
        let (code, console, log) = run_headless(&["katana", "--script", "ok.kcs"]);
        assert_eq!(code, 0, "{}", console.err);
        assert!(logged(&log, "headless"));
        assert!(logged(&log, "run SCRIPT ok.kcs"));
        let (code, console, _) = run_headless(&["katana", "--script", "BAD.kcs"]);
        assert_eq!(code, 1);
        assert_eq!(
            console.err,
            "--script BAD.kcs stopped at a line that was refused\n--script BAD.kcs was refused\n"
        );
    }

    #[test]
    fn a_refused_customise_fails_a_batch_and_leaves_a_screenshot_run_going() {
        let (code, console, log) = run_headless(&[
            "katana",
            "--customise",
            "/fixtures/BAD.customisation.json",
            "--script",
            "ok.kcs",
        ]);
        assert_eq!(code, 1);
        assert_eq!(
            console.err,
            "--customise /fixtures/BAD.customisation.json was refused\n"
        );
        assert!(logged(
            &log,
            "run CUSTOMISE /fixtures/BAD.customisation.json"
        ));
        assert!(!logged(&log, "run SCRIPT ok.kcs"));
        let scratch = Scratch::new("drive-program-customise");
        let shot = path_text(&scratch.path("customise.png"));
        let (code, console, _) = run_headless(&[
            "katana",
            "--customise",
            "/fixtures/BAD.customisation.json",
            "--screenshot",
            &shot,
        ]);
        assert_eq!(code, 0, "{}", console.err);
        // A path a line cannot carry loads nothing, and says why.
        let (code, console, _) = run_headless(&[
            "katana",
            "--customise",
            "/fixtures/a\"b.json",
            "--script",
            "ok.kcs",
        ]);
        assert_eq!(code, 1);
        assert!(
            console
                .err
                .ends_with("--customise /fixtures/a\"b.json was refused\n"),
            "{}",
            console.err
        );
    }

    #[test]
    fn a_run_that_writes_runs_its_lines_first_and_a_refused_one_fails_it() {
        let (code, console, log) =
            run_headless(&["katana", "--command", "BAD", "--plot", "out.pdf"]);
        assert_eq!(code, 1);
        assert_eq!(console.err, "--command BAD was refused\n");
        assert!(!log.borrow().iter().any(|line| line.starts_with("plot ")));
        let (code, console, log) = run_headless(&[
            "katana",
            "--command",
            "LIST",
            "--run-line",
            "VIEWS",
            "--plot",
            "out.pdf",
            "--scale",
            "500",
        ]);
        assert_eq!(code, 0, "{}", console.err);
        assert_eq!(
            console.err,
            "--run-line VIEWS: ok=yes\n  reply: done\n  reply: VIEWS\n"
        );
        assert!(logged(&log, "> LIST"));
        assert!(logged(&log, "plot out.pdf fit=false"));
        let (code, console, _) = run_headless(&["katana", "--plot-sheets", "sheets"]);
        assert_eq!(code, 0, "{}", console.err);
        let written = if cfg!(windows) {
            "sheets\\sheet1.pdf\n"
        } else {
            "sheets/sheet1.pdf\n"
        };
        assert_eq!(console.out, written);
        let (code, console, _) = run_headless(&["katana", "--sheets-json", "-"]);
        assert_eq!(code, 0, "{}", console.err);
        assert!(console.out.ends_with('\n') && console.out.len() > 1);
    }

    #[test]
    fn a_window_a_person_uses_is_started_and_handed_to_the_desktop() {
        let scratch = Scratch::new("drive-program-desktop");
        let folder = scratch.folder("project");
        let log = new_log();
        let mut console = Captured::default();
        let mut handed = false;
        let code = run(
            &arguments(&["katana", &path_text(&folder), "roads.dxf"]),
            &mut console,
            || TestWindow::new(&log),
            |desktop| {
                handed = true;
                assert!(desktop.window().shown);
                let viewport = desktop.viewport();
                assert_eq!(viewport.title.as_deref(), Some(TITLE));
                assert_eq!(viewport.inner_size, Some(egui::vec2(1360.0, 860.0)));
                assert!(viewport.icon.is_some());
                7
            },
        );
        assert!(handed);
        assert_eq!(code, 7, "the event loop's code is the run's");
        assert_eq!(console.err, "");
        let log = log.borrow();
        let expected = [
            String::from("watched"),
            String::from("loadDefaultCustomisation"),
            String::from("restoreSession"),
            String::from("show"),
            String::from("startUpdates"),
            format!("openProject {}", folder.display()),
            String::from("importPath roads.dxf"),
        ];
        assert_eq!(*log, expected);
    }

    #[test]
    fn a_window_given_nothing_to_open_shows_its_start_screen() {
        let log = new_log();
        let mut console = Captured::default();
        let code = run(
            &arguments(&["katana"]),
            &mut console,
            || TestWindow::new(&log),
            |_| 0,
        );
        assert_eq!(code, 0);
        assert!(logged(&log, "showStartScreenIfWanted"));
    }
}
