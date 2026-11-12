//! The step engine's own cases: every sigil of check_screenshot.cmake's `-DDRIVE` read as the
//! switch it stands for, each step taken against the small test window, and each refusal in
//! main.cpp's words.

use katana_app::console::Captured;
use katana_window::drive::WINDOW;
use katana_window::drive::headless::Headless;
use katana_window::drive::steps::{Step, StepEngine, StepKind, drive_steps};

use crate::test_window::{Log, TestWindow, new_log};

mod step_engine {
    use super::*;

    /// The window, drawn once, and an engine with no target yet.
    struct Bench {
        log: Log,
        window: TestWindow,
        headless: Headless,
        engine: StepEngine,
        console: Captured,
    }

    impl Bench {
        fn new() -> Self {
            let log = new_log();
            let mut window = TestWindow::new(&log);
            let mut headless = Headless::new();
            headless.settle(&mut window).expect("the window is drawn");
            Self {
                log,
                window,
                headless,
                engine: StepEngine::new(),
                console: Captured::default(),
            }
        }

        /// The `-DDRIVE` step `drive` taken; whether the run goes on.
        fn step(&mut self, drive: &str) -> bool {
            self.take(&Step::from_drive(drive))
        }

        fn take(&mut self, step: &Step) -> bool {
            self.engine.run(
                step,
                &mut self.window,
                &mut self.headless,
                &mut self.console,
            )
        }

        /// What the steps printed, and forgets it.
        fn said(&mut self) -> String {
            std::mem::take(&mut self.console.err)
        }

        fn logged(&self, what: &str) -> bool {
            self.log.borrow().iter().any(|line| line == what)
        }
    }

    #[test]
    fn every_sigil_is_read_as_the_switch_it_stands_for() {
        let cases = [
            ("@formatStyles", StepKind::Dialog, "formatStyles"),
            (
                "#surveyPointManager",
                StepKind::SurveyDock,
                "surveyPointManager",
            ),
            ("%PropertiesDock", StepKind::Panel, "PropertiesDock"),
            ("field=N 45 E", StepKind::Fill, "field=N 45 E"),
            ("!surveyRun", StepKind::Press, "surveyRun"),
            (">LINE 0,0 5,5", StepKind::Command, "LINE 0,0 5,5"),
            (">", StepKind::Command, ""),
            ("<VIEWS", StepKind::RunLine, "VIEWS"),
            ("?FrameStatsLabel", StepKind::Report, "FrameStatsLabel"),
            ("*formatPurge", StepKind::Trigger, "formatPurge"),
            ("^out.dxf", StepKind::ExportOptions, "out.dxf"),
            ("~110.3,50", StepKind::Hover, "110.3,50"),
            ("~-5,7", StepKind::Hover, "-5,7"),
            (
                "~RenderView2 160,330 10",
                StepKind::Wheel,
                "RenderView2 160,330 10",
            ),
            ("+3,4,shift", StepKind::Click, "3,4,shift"),
        ];
        for (drive, kind, text) in cases {
            assert_eq!(Step::from_drive(drive), Step::new(kind, text), "{drive}");
        }
        assert_eq!(
            drive_steps("@surveyInverse|surveyFrom=1|!surveyRun"),
            vec![
                Step::new(StepKind::Dialog, "surveyInverse"),
                Step::new(StepKind::Fill, "surveyFrom=1"),
                Step::new(StepKind::Press, "surveyRun"),
            ]
        );
        assert_eq!(Step::enter(), Step::new(StepKind::Command, ""));
        assert_eq!(
            StepKind::from_switch("--survey-dialog"),
            Some(StepKind::Dialog)
        );
        assert_eq!(StepKind::from_switch("--hover"), Some(StepKind::Hover));
        assert_eq!(StepKind::from_switch("--screenshot"), None);
        assert_eq!(StepKind::Wheel.switch(), "--wheel");
        assert_eq!(StepKind::ExportOptions.switch(), "--export-options");
    }

    #[test]
    fn a_dialog_opens_through_its_action_and_says_what_opened() {
        let mut bench = Bench::new();
        assert!(bench.step("@openTest"));
        assert_eq!(
            bench.said(),
            "--dialog openTest opened testDialog \"Test Dialog\"\n"
        );
        assert_eq!(bench.engine.target(), Some("testDialog"));
        assert_eq!(bench.engine.said_as(), "openTest");
        assert!(bench.logged("trigger openTest"));
    }

    #[test]
    fn a_dialog_asked_by_its_own_name_opens_through_the_action_that_names_it() {
        let mut bench = Bench::new();
        assert!(bench.step("@testDialog"));
        assert_eq!(
            bench.said(),
            "--dialog testDialog opened testDialog \"Test Dialog\"\n"
        );
        assert!(bench.logged("trigger openTest"));
    }

    #[test]
    fn a_dialog_no_action_opens_is_refused() {
        let mut bench = Bench::new();
        assert!(!bench.step("@nothing"));
        assert_eq!(
            bench.said(),
            "--dialog nothing failed: NotFound: no menu item or tool has that name [nothing]\n"
        );
        // An action that opens a dock opens no dialog.
        assert!(!bench.step("@showDock"));
        assert_eq!(
            bench.said(),
            "--dialog: showDock opened no dialog named TestDock\n"
        );
    }

    #[test]
    fn fills_and_presses_reach_the_targets_controls_by_name() {
        let mut bench = Bench::new();
        assert!(bench.step("@openTest"));
        assert!(bench.step("testField=hello world"));
        assert!(bench.step("testAgree=on"));
        assert!(bench.step("testChoice=Blue"));
        assert!(bench.step("!testRun"));
        assert_eq!(bench.window.dialog.field, "hello world");
        assert!(bench.window.dialog.agree);
        assert_eq!(bench.window.dialog.choice.index, Some(1));
        assert_eq!(bench.window.dialog.runs, 1);
        assert_eq!(
            bench.said(),
            "--dialog openTest opened testDialog \"Test Dialog\"\n"
        );
    }

    #[test]
    fn each_refusal_of_a_fill_or_a_press_is_main_cpps_sentence() {
        let mut bench = Bench::new();
        assert!(bench.step("@openTest"));
        bench.said();
        let refusals = [
            (
                "testLockedField=x",
                "--fill: testDialog's field testLockedField is disabled\n",
            ),
            (
                "!testLocked",
                "--press: openTest's button testLocked is disabled\n",
            ),
            (
                "!testHidden",
                "--press: openTest's button testHidden is hidden\n",
            ),
            ("!nope", "--press: openTest has no button nope\n"),
            (
                "nope=x",
                "--fill: the dialog has no field nope that takes 'x'\n",
            ),
            (
                "testChoice=Green",
                "--fill: testChoice has no choice 'Green'\n",
            ),
            (
                "testAgree=maybe",
                "--fill: the dialog has no field testAgree that takes 'maybe'\n",
            ),
            ("=x", "--fill needs FIELD=TEXT, not =x\n"),
            ("noequals", "--fill needs FIELD=TEXT, not noequals\n"),
        ];
        for (drive, sentence) in refusals {
            assert!(!bench.step(drive), "{drive} was taken");
            assert_eq!(bench.said(), sentence, "{drive}");
        }
    }

    #[test]
    fn a_fill_or_a_press_before_any_target_is_refused() {
        let mut bench = Bench::new();
        assert!(!bench.step("field=x"));
        assert_eq!(bench.said(), "--fill field=x comes before any --dialog\n");
        assert!(!bench.step("!button"));
        assert_eq!(bench.said(), "--press button comes before any --dialog\n");
    }

    #[test]
    fn a_step_for_a_dialog_that_has_closed_is_refused_and_the_window_is_grabbed() {
        let mut bench = Bench::new();
        assert!(bench.step("@openTest"));
        assert!(bench.step("!testClose"));
        bench.said();
        assert!(!bench.step("testField=x"));
        assert_eq!(bench.said(), "--fill testField=x: openTest has closed\n");
        let frame = bench.headless.snapshot();
        assert_eq!(bench.engine.finish(&frame, &mut bench.console), WINDOW);
        assert_eq!(bench.said(), "openTest has closed: the window is grabbed\n");
    }

    #[test]
    fn steps_that_made_no_target_leave_the_window_grabbed_and_say_nothing() {
        let mut bench = Bench::new();
        assert!(bench.step(">LIST"));
        let frame = bench.headless.snapshot();
        assert_eq!(bench.engine.finish(&frame, &mut bench.console), WINDOW);
        assert_eq!(bench.said(), "");
    }

    #[test]
    fn a_report_says_what_a_control_an_action_a_menu_or_a_view_shows() {
        let mut bench = Bench::new();
        let reports = [
            ("?statusLabel", "statusLabel: ready now\n"),
            ("?gridToggle", "gridToggle: Grid, unchecked\n"),
            ("?testMenu", "testMenu: Test: Item [Does a thing] ; More\n"),
            ("?testSubMenu", "testSubMenu: More: Deep\n"),
            ("?PlanView1", "PlanView1: drawn=3 ghosts=0 grips=0\n"),
        ];
        for (drive, said) in reports {
            assert!(bench.step(drive), "{drive}");
            assert_eq!(bench.said(), said, "{drive}");
        }
        assert!(bench.step("@openTest"));
        bench.said();
        // In the target, as written and simplified; a check box with whether it is checked; the
        // window's own controls with a dialog the target.
        assert!(bench.step("?testNote"));
        assert_eq!(bench.said(), "testNote: a <b>note</b> as written\n");
        assert!(bench.step("?testAgree"));
        assert_eq!(bench.said(), "testAgree: Agree, unchecked\n");
        assert!(bench.step("?statusLabel"));
        assert_eq!(bench.said(), "statusLabel: ready now\n");
        // A choice is no label, field, list or button to report, as in the C++.
        for name in ["testChoice", "nothing"] {
            assert!(!bench.step(&format!("?{name}")));
            assert_eq!(
                bench.said(),
                format!(
                    "--report: there is no label, field, text, list, button, view, action or menu {name}\n"
                )
            );
        }
    }

    #[test]
    fn the_wheel_turns_notch_by_notch_over_a_control_named_as_a_report_finds_it() {
        let mut bench = Bench::new();
        assert!(bench.step("~PlanView1 10,20 2"));
        assert_eq!(bench.said(), "--wheel PlanView1: 2 notches at 10,20\n");
        assert!(bench.step("~PlanView1 10.5,20 -1"));
        assert_eq!(bench.said(), "--wheel PlanView1: -1 notches at 10.5,20\n");
        assert_eq!(bench.window.wheel, vec![1.0, 1.0, -1.0]);
        let refusals = [
            (
                "~PlanView1 10,20",
                "--wheel: usage: --wheel \"NAME X,Y NOTCHES\", not \"PlanView1 10,20\"\n",
            ),
            (
                "~PlanView1 1,1 0",
                "--wheel: usage: --wheel \"NAME X,Y NOTCHES\", not \"PlanView1 1,1 0\"\n",
            ),
            (
                "~PlanView1 1;1 2",
                "--wheel: usage: --wheel \"NAME X,Y NOTCHES\", not \"PlanView1 1;1 2\"\n",
            ),
            (
                "~PlanView1 500,20 1",
                "--wheel: 500,20 is outside PlanView1, 200 x 100\n",
            ),
            ("~Nothing 1,1 1", "--wheel: there is no widget Nothing\n"),
        ];
        for (drive, sentence) in refusals {
            assert!(!bench.step(drive), "{drive}");
            assert_eq!(bench.said(), sentence, "{drive}");
        }
        assert_eq!(bench.window.wheel.len(), 3);
    }

    #[test]
    fn the_pointer_steps_print_the_record_the_window_answers() {
        let mut bench = Bench::new();
        assert!(bench.step("~1.5,2"));
        assert_eq!(bench.said(), "pointer: action=hover x=1.5 y=2 tool=none\n");
        assert!(bench.step("+3,4,shift"));
        assert_eq!(bench.said(), "pointer: action=click x=3 y=4 tool=none\n");
        assert!(!bench.step("~1.5"));
        assert_eq!(
            bench.said(),
            "--hover 1.5: ParseFailure: the pointer needs x,y[,shift|ctrl] [1.5]\n"
        );
        assert!(!bench.step("+9"));
        assert_eq!(
            bench.said(),
            "--click 9: ParseFailure: the pointer needs x,y[,shift|ctrl] [9]\n"
        );
    }

    #[test]
    fn a_panel_is_a_menu_opened_a_dock_or_an_open_window_of_its_own() {
        let mut bench = Bench::new();
        assert!(!bench.step("%nothing"));
        assert_eq!(
            bench.said(),
            "--panel: the window has no dock, toolbar, menu or open popup nothing\n"
        );
        assert!(bench.step("*showDock"));
        assert!(bench.step("%TestDock"));
        assert_eq!(bench.engine.target(), Some("TestDock"));
        assert!(bench.step("?status"));
        assert_eq!(bench.said(), "status: 3 points, 3 shown\n");
        assert!(bench.step("%testMenu"));
        assert!(bench.window.menu_open);
        assert!(bench.step("!testMenuItem"));
        assert!(!bench.window.menu_open);
        // A dialog another dialog's button opened is a panel once it is shown.
        assert!(bench.step("*openTest"));
        assert!(bench.step("%testDialog"));
        assert!(bench.step("testField=from a panel"));
        assert_eq!(bench.window.dialog.field, "from a panel");
    }

    #[test]
    fn a_survey_dock_is_shown_made_the_target_and_says_its_status_at_the_end() {
        let mut bench = Bench::new();
        assert!(bench.step("#showDock"));
        assert_eq!(bench.engine.target(), Some("TestDock"));
        assert_eq!(bench.engine.docks(), ["TestDock"]);
        let frame = bench.headless.snapshot();
        assert_eq!(bench.engine.finish(&frame, &mut bench.console), "TestDock");
        assert_eq!(bench.said(), "TestDock: 3 points, 3 shown\n");
        assert!(!bench.step("#plainAction"));
        assert_eq!(
            bench.said(),
            "--survey-dock: no action plainAction that names a dock\n"
        );
        assert!(!bench.step("#openTest"));
        assert_eq!(
            bench.said(),
            "--survey-dock: openTest showed no dock named testDialog\n"
        );
    }

    #[test]
    fn a_command_and_a_run_line_reach_the_window_and_a_run_line_says_what_it_got() {
        let mut bench = Bench::new();
        assert!(bench.step(">LIST"));
        assert!(bench.step(">"));
        assert!(bench.logged("> LIST"));
        assert!(bench.logged("> "));
        assert!(bench.step("<LIST"));
        assert_eq!(
            bench.said(),
            "--run-line LIST: ok=yes\n  reply: done\n  reply: LIST\n"
        );
        // Refused, it is said as the dialog would show it, and the steps go on.
        assert!(bench.step("<BAD"));
        assert_eq!(
            bench.said(),
            "--run-line BAD: ok=no\n  error: refused BAD\n"
        );
    }

    #[test]
    fn a_script_runs_by_the_script_verb_and_one_that_stops_ends_the_run() {
        let mut bench = Bench::new();
        assert!(bench.take(&Step::new(StepKind::Script, "ok.kcs")));
        assert!(bench.logged("run SCRIPT ok.kcs"));
        assert_eq!(bench.said(), "");
        assert!(!bench.take(&Step::new(StepKind::Script, "BAD.kcs")));
        assert_eq!(
            bench.said(),
            "--script BAD.kcs stopped at a line that was refused\n"
        );
    }

    #[test]
    fn a_trigger_runs_a_menu_item_in_its_turn_and_one_refused_ends_the_run() {
        let mut bench = Bench::new();
        assert!(bench.step("*gridToggle"));
        assert!(bench.step("?gridToggle"));
        assert_eq!(bench.said(), "gridToggle: Grid, checked\n");
        assert!(!bench.step("*disabledAction"));
        assert_eq!(
            bench.said(),
            "--trigger disabledAction failed: InvalidState: that menu item is disabled [disabledAction]\n"
        );
    }

    #[test]
    fn the_export_options_open_in_their_turn_as_the_target_by_their_own_name() {
        let mut bench = Bench::new();
        assert!(bench.step("^out.dxf"));
        assert_eq!(
            bench.said(),
            "--export-options out.dxf: opened vectorExportDialog\n"
        );
        assert_eq!(bench.engine.target(), Some("testDialog"));
        assert_eq!(bench.engine.said_as(), "testDialog");
        assert!(!bench.step("!nope"));
        assert_eq!(bench.said(), "--press: testDialog has no button nope\n");
    }

    #[test]
    fn a_window_that_never_comes_idle_fails_the_step_saying_so() {
        let mut bench = Bench::new();
        bench.window.never_idle = true;
        bench.headless.set_most_frames(5);
        assert!(!bench.step(">LIST"));
        assert_eq!(bench.said(), "the window did not settle in 5 frames\n");
    }

    #[test]
    fn a_name_drawn_twice_in_one_target_fails_the_step() {
        let mut bench = Bench::new();
        bench.window.draw_twice = true;
        assert!(!bench.step(">LIST"));
        assert_eq!(
            bench.said(),
            "the name twice is drawn twice in the window: a driver could not tell the two apart\n"
        );
    }

    #[test]
    fn a_job_running_is_waited_for_and_its_frames_are_not_counted() {
        let mut bench = Bench::new();
        bench.window.busy_frames.set(20);
        bench.headless.set_most_frames(5);
        assert!(bench.step(">LIST"));
        assert_eq!(bench.window.busy_frames.get(), 0);
    }
}
