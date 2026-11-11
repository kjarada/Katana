//! The named controls' own cases (drive::controls, drive::registry, drive::mailbox): each kind
//! filled, pressed and reported as main.cpp's --fill, --press and --report do, refused in the
//! C++'s words when it is disabled or hidden or cannot take the act, an act for no control or a
//! closed target refused, and a name drawn twice in one target refused.

use egui::Ui;
use katana_window::drive::{self, Choice, Harness, SpinRange, simplified};
use katana_window::shell::DialogFrame;

mod named_controls {
    use super::*;

    /// One of every kind, in a dialog named testDialog.
    struct Form {
        line: String,
        text: String,
        on: bool,
        choice: Choice,
        editable: Choice,
        number: f64,
        count: i64,
        row: Option<usize>,
        tab: usize,
        clicks: usize,
        radio_clicks: usize,
        disabled_line: String,
        hidden_clicks: usize,
        /// A second control named "button" in the same target.
        duplicate: bool,
    }

    impl Default for Form {
        fn default() -> Self {
            Self {
                line: String::new(),
                text: String::new(),
                on: false,
                choice: Choice::at(0),
                editable: Choice::editable("alpha"),
                number: 1.0,
                count: 1,
                row: None,
                tab: 0,
                clicks: 0,
                radio_clicks: 0,
                disabled_line: String::new(),
                hidden_clicks: 0,
                duplicate: false,
            }
        }
    }

    fn items() -> Vec<String> {
        vec![String::from("alpha"), String::from("beta")]
    }

    fn rows() -> Vec<Vec<String>> {
        vec![
            vec![String::from("first"), String::from("1")],
            vec![String::from("second"), String::from("2")],
        ]
    }

    fn draw_form(ui: &mut Ui, frame: &mut DialogFrame, form: &mut Form) {
        let ctx = ui.ctx().clone();
        frame.show_body(&ctx, form, |ui, form| {
            let items = items();
            let rows = rows();
            drive::line(ui, "line", &mut form.line);
            drive::text(ui, "text", &mut form.text);
            drive::check(ui, "check", "Check", &mut form.on);
            if drive::radio(ui, "radio", "Radio", false).clicked() {
                form.radio_clicks += 1;
            }
            drive::choice(ui, "choice", &items, &mut form.choice);
            drive::choice(ui, "editable", &items, &mut form.editable);
            drive::spin(
                ui,
                "spin",
                &mut form.number,
                SpinRange::new(0.0, 10.0, 0.1, 2),
            );
            drive::whole(ui, "whole", &mut form.count, 1..=5);
            drive::rows(ui, "rows", &rows, &mut form.row);
            drive::tabs(ui, "tabs", &["&General", "Linetypes"], &mut form.tab);
            drive::label(ui, "label", "A <b>label</b> as written");
            if drive::button(ui, "button", "&Apply").clicked() {
                form.clicks += 1;
            }
            drive::toggle(ui, "toggle", "Toggle", true);
            ui.add_enabled_ui(false, |ui| {
                drive::line(ui, "disabledLine", &mut form.disabled_line);
                drive::button(ui, "disabledButton", "Off");
            });
            ui.scope(|ui| {
                ui.set_invisible();
                if drive::button(ui, "hiddenButton", "Hidden").clicked() {
                    form.hidden_clicks += 1;
                }
            });
            if form.duplicate {
                drive::button(ui, "button", "Again");
            }
        });
    }

    struct Bench {
        harness: Harness,
        frame: DialogFrame,
        form: Form,
    }

    impl Bench {
        fn new() -> Self {
            let mut bench = Self {
                harness: Harness::new(),
                frame: DialogFrame::new("testDialog", "Test"),
                form: Form::default(),
            };
            bench.run().expect("the form is drawn");
            bench.harness.target("testDialog");
            bench
        }

        fn run(&mut self) -> Result<(), String> {
            let (frame, form) = (&mut self.frame, &mut self.form);
            self.harness.run(|ui| draw_form(ui, frame, form))
        }

        fn fill(&mut self, name: &str, text: &str) -> Result<(), String> {
            self.harness.fill(name, text);
            self.run()
        }

        fn press(&mut self, name: &str) -> Result<(), String> {
            self.harness.press(name);
            self.run()
        }
    }

    #[test]
    fn each_kind_takes_a_fill_as_a_person_would() {
        let mut bench = Bench::new();
        bench.fill("line", "x").expect("a field is typed into");
        assert_eq!(bench.form.line, "x");
        // A backslash and an n is a line break, so a field book fits on a command line.
        bench
            .fill("text", "a\\nb")
            .expect("a text box is typed into");
        assert_eq!(bench.form.text, "a\nb");
        bench.fill("check", "on").expect("a check box is ticked");
        assert!(bench.form.on);
        bench.fill("choice", "beta").expect("an item is chosen");
        assert_eq!(bench.form.choice.index, Some(1));
        // An editable choice takes a name it does not list, as typing does.
        bench
            .fill("editable", "gamma")
            .expect("an editable choice takes a new name");
        assert_eq!(bench.form.editable.text, "gamma");
        // Rounded to the box's two decimals as it shows them: 0.705 is the double just under it.
        bench.fill("spin", "0.705").expect("a number is set");
        assert_eq!(bench.form.number, 0.70);
        bench.fill("whole", " 3 ").expect("a whole number is set");
        assert_eq!(bench.form.count, 3);
        bench
            .fill("rows", "second")
            .expect("a row is chosen by its text");
        assert_eq!(bench.form.row, Some(1));
        bench
            .fill("tabs", "Linetypes")
            .expect("a tab is brought to the front");
        assert_eq!(bench.form.tab, 1);
        bench
            .fill("tabs", "General")
            .expect("a title's '&' is not counted");
        assert_eq!(bench.form.tab, 0);
    }

    #[test]
    fn a_fill_a_control_cannot_take_is_refused_in_the_cpps_words() {
        let mut bench = Bench::new();
        let refused = |bench: &mut Bench, name: &str, text: &str| {
            bench.fill(name, text).expect_err("the fill is refused")
        };
        assert_eq!(
            refused(&mut bench, "choice", "gamma"),
            "--fill: choice has no choice 'gamma'"
        );
        assert_eq!(
            refused(&mut bench, "spin", "11"),
            "--fill: spin takes a number from 0 to 10, not '11'"
        );
        assert_eq!(
            refused(&mut bench, "whole", "9"),
            "--fill: whole takes a whole number from 1 to 5, not '9'"
        );
        assert_eq!(
            refused(&mut bench, "tabs", "Styles"),
            "--fill: tabs has no tab 'Styles'"
        );
        assert_eq!(
            refused(&mut bench, "rows", "third"),
            "--fill: rows lists no 'third'"
        );
        assert_eq!(
            refused(&mut bench, "check", "maybe"),
            "--fill: the dialog has no field check that takes 'maybe'"
        );
        assert_eq!(
            refused(&mut bench, "button", "x"),
            "--fill: the dialog has no field button that takes 'x'"
        );
        assert_eq!(
            refused(&mut bench, "nothing", "y"),
            "--fill: the dialog has no field nothing that takes 'y'"
        );
    }

    #[test]
    fn a_press_clicks_a_button_once_and_toggles_a_check_box() {
        let mut bench = Bench::new();
        bench.press("button").expect("the button is pressed");
        assert_eq!(bench.form.clicks, 1);
        bench.press("button").expect("the button is pressed again");
        assert_eq!(bench.form.clicks, 2);
        bench.press("check").expect("the check box is clicked");
        assert!(bench.form.on);
        bench.press("radio").expect("the radio button is clicked");
        assert_eq!(bench.form.radio_clicks, 1);
        assert_eq!(
            bench.press("line").expect_err("a field is no button"),
            "--press: testDialog has no button line"
        );
        assert_eq!(
            bench.press("nothing").expect_err("there is no such button"),
            "--press: testDialog has no button nothing"
        );
    }

    #[test]
    fn a_disabled_or_hidden_control_refuses_the_act_as_the_cpp_refuses_it() {
        let mut bench = Bench::new();
        assert_eq!(
            bench
                .fill("disabledLine", "x")
                .expect_err("a disabled field"),
            "--fill: testDialog's field disabledLine is disabled"
        );
        assert_eq!(
            bench
                .press("disabledButton")
                .expect_err("a disabled button"),
            "--press: testDialog's button disabledButton is disabled"
        );
        assert_eq!(
            bench.press("hiddenButton").expect_err("a hidden button"),
            "--press: testDialog's button hiddenButton is hidden"
        );
        assert_eq!(bench.form.hidden_clicks, 0);
        // A press refusal names the target as the step named it.
        bench.harness.target_said_as("testDialog", "testAction");
        assert_eq!(
            bench
                .press("disabledButton")
                .expect_err("a disabled button"),
            "--press: testAction's button disabledButton is disabled"
        );
    }

    #[test]
    fn each_control_reports_what_it_shows() {
        let mut bench = Bench::new();
        bench.fill("line", "x").expect("a field is typed into");
        assert_eq!(
            bench.harness.report("label").as_deref(),
            Ok("A <b>label</b> as written")
        );
        assert_eq!(bench.harness.report("line").as_deref(), Ok("x"));
        assert_eq!(
            bench.harness.report("rows").as_deref(),
            Ok("first | 1 ; second | 2")
        );
        assert_eq!(bench.harness.report("button").as_deref(), Ok("Apply"));
        assert_eq!(
            bench.harness.report("toggle").as_deref(),
            Ok("Toggle, checked")
        );
        assert_eq!(
            bench.harness.report("check").as_deref(),
            Ok("Check, unchecked")
        );
        assert_eq!(
            bench.harness.report("choice"),
            Err(String::from(
                "--report: there is no label, field, text, list, button, view, action or menu choice"
            ))
        );
        assert_eq!(simplified("  two\n  lines  "), "two lines");
    }

    #[test]
    fn a_name_used_twice_in_one_target_is_refused() {
        let mut bench = Bench::new();
        bench.form.duplicate = true;
        let refusal = bench.run().expect_err("two controls named button");
        assert!(
            refusal.contains("the name button is drawn twice in testDialog"),
            "{refusal}"
        );
    }

    #[test]
    fn an_act_for_a_target_that_is_not_drawn_is_refused_as_closed() {
        let mut bench = Bench::new();
        bench.harness.target("goneDialog");
        assert_eq!(
            bench.press("close").expect_err("the target is not drawn"),
            "--press close: goneDialog has closed"
        );
    }

    #[test]
    fn the_same_name_in_two_targets_is_two_controls() {
        let mut harness = Harness::new();
        let mut first = DialogFrame::new("firstDialog", "First");
        let mut second = DialogFrame::new("secondDialog", "Second");
        let mut presses = (0, 0);
        let mut draw = |ui: &mut Ui| {
            let ctx = ui.ctx().clone();
            first.show_body(&ctx, &mut presses.0, |ui, count| {
                if drive::button(ui, "close", "Close").clicked() {
                    *count += 1;
                }
            });
            second.show_body(&ctx, &mut presses.1, |ui, count| {
                if drive::button(ui, "close", "Close").clicked() {
                    *count += 1;
                }
            });
        };
        harness.show(&mut draw).expect("both dialogs are drawn");
        harness
            .target("secondDialog")
            .press("close")
            .run(&mut draw)
            .expect("the second dialog's close is pressed");
        let drawn = harness.snapshot();
        assert!(drawn.control("firstDialog", "close").is_some());
        assert!(drawn.control("secondDialog", "close").is_some());
        assert_eq!(presses, (0, 1));
    }
}
