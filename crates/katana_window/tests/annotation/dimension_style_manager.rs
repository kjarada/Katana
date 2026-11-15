//! The twins of tests/qt_widgets/annotation/test_dimension_style_manager.cpp: the Dimension Styles
//! manager (Format > Dimension Styles...), driven by object name as a person drives it and checked
//! by the lines it runs: every edit is a DIMSTYLE line through the executor, and each button is
//! ONE undo step. The runner is the interpreter itself, so a line is checked by what the verb makes
//! of it, not by what the dialog meant.

use katana_entity::{ArrowHead, DimensionStyle};
use katana_window::annotation::{AnnotationWorkbench, DimensionStyleManagerDialog};
use katana_window::drive::{Harness, WorkbenchHost};
use katana_window::host::Host;

use crate::support::{Bench, Dialog, interpreter_host, run};

impl Dialog for DimensionStyleManagerDialog {
    const NAME: &'static str = "dimensionStyleManagerDialog";
    fn draw(&mut self, ctx: &egui::Context, host: &mut dyn Host) -> bool {
        self.ui(ctx, host)
    }
}

fn stored(bench: &Bench<DimensionStyleManagerDialog>, name: &str) -> DimensionStyle {
    bench
        .document()
        .model()
        .dimension_styles
        .find(name)
        .unwrap_or_else(|| panic!("no style {name}"))
        .clone()
}

/// EXPECT_DOUBLE_EQ: within four units in the last place.
fn double_eq(a: f64, b: f64) -> bool {
    if a == b {
        return true;
    }
    if a.is_nan() || b.is_nan() || a.is_sign_negative() != b.is_sign_negative() {
        return false;
    }
    a.to_bits().abs_diff(b.to_bits()) <= 4
}

mod dimension_style_manager {
    use super::*;

    fn manager() -> Bench<DimensionStyleManagerDialog> {
        Bench::new(interpreter_host(), DimensionStyleManagerDialog::new)
    }

    #[test]
    fn every_field_is_driven_by_name_and_apply_is_one_line_and_one_step() {
        let mut bench = manager();
        assert_eq!(bench.dialog.name(), "dimensionStyleManagerDialog");
        assert_eq!(bench.dialog.current(), "Standard");

        bench.fill("dimStyleText", "3.5");
        bench.fill("dimStyleGap", "0.8");
        bench.fill("dimStylePrefix", "L= ");
        bench.fill("dimStyleSuffix", "m");
        bench.fill("dimStyleExtOff", "1");
        bench.fill("dimStyleExtBeyond", "2");
        bench.fill("dimStyleArrow", "3");
        bench.fill("dimStyleHead", "Tick");
        bench.fill("dimStyleScale", "1000");
        bench.fill("dimStyleDecimals", "1");
        bench.fill("dimStyleRound", "0.5");
        bench.fill("dimStyleTrim", "on");
        bench.fill("dimStylePaper", "on");

        let before = bench.steps();
        assert!(
            bench.dialog.apply(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        assert_eq!(bench.lines().len(), 1);
        assert_eq!(
            bench.lines()[0],
            "DIMSTYLE SET Standard TEXT 3.5 GAP 0.8 EXTOFF 1 EXTBEYOND 2 ARROW 3 HEAD Tick SCALE \
             1000 DECIMALS 1 ROUND 0.5 PREFIX \"L= \" SUFFIX m TRIM on PAPER on"
        );
        assert_eq!(bench.steps(), before + 1, "thirteen fields, one step");
        let standard = stored(&bench, "Standard");
        assert!(double_eq(standard.text_height, 3.5));
        assert!(double_eq(standard.text_gap, 0.8));
        assert_eq!(standard.prefix, "L= ");
        assert_eq!(standard.suffix, "m");
        assert!(double_eq(standard.extension_offset, 1.0));
        assert!(double_eq(standard.extension_beyond, 2.0));
        assert!(double_eq(standard.arrow_size, 3.0));
        assert_eq!(standard.arrow_head, ArrowHead::Tick);
        assert!(double_eq(standard.unit_scale, 1000.0));
        assert_eq!(standard.decimals, 1);
        assert!(double_eq(standard.round_to, 0.5));
        assert!(standard.suppress_trailing_zeros);
        assert!(standard.paper_sized);

        assert!(bench.dialog.apply(&mut bench.host), "an unchanged form");
        assert_eq!(bench.lines().len(), 1, "runs no line");
        assert_eq!(bench.steps(), before + 1, "and makes no step");

        // One undo takes every field back, and the form follows.
        bench.document_mut().undo().expect("the undo is done");
        assert_eq!(stored(&bench, "Standard"), DimensionStyle::default());
        bench.draw();
        assert_eq!(bench.value("dimStyleText"), "2.5");
    }

    #[test]
    fn new_and_duplicate_name_the_style_and_each_is_one_step() {
        let mut bench = manager();
        assert_eq!(
            bench.dialog.new_name_placeholder(),
            "Dimension Style",
            "a blank name gives the fresh one shown"
        );

        bench.fill("dimStyleNewName", "site");
        assert!(
            bench.dialog.new_style(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        assert_eq!(bench.lines().last().map(String::as_str), Some("DIMSTYLE NEW site"));
        assert_eq!(bench.dialog.current(), "site");
        bench.draw();
        assert!(bench.value("dimStyleNewName").is_empty());

        // Duplicate makes a style of what the form shows, its edits included, in one step: NEW
        // with the fields that differ from a new style's.
        bench.fill("dimStyleText", "5");
        bench.fill("dimStylePaper", "on");
        bench.fill("dimStyleNewName", "site wide");
        let before = bench.steps();
        assert!(
            bench.dialog.duplicate_style(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        assert_eq!(
            bench.lines().last().map(String::as_str),
            Some("DIMSTYLE NEW \"site wide\" TEXT 5 PAPER on")
        );
        assert_eq!(bench.steps(), before + 1);
        assert!(double_eq(stored(&bench, "site wide").text_height, 5.0));
        assert!(stored(&bench, "site wide").paper_sized);
        assert!(
            double_eq(stored(&bench, "site").text_height, 2.5),
            "the original is untouched"
        );
        assert_eq!(bench.dialog.current(), "site wide");

        // Blank names count on.
        bench.draw();
        assert!(bench.dialog.new_style(&mut bench.host));
        assert!(bench.dialog.new_style(&mut bench.host));
        assert!(
            bench
                .document()
                .model()
                .dimension_styles
                .contains("Dimension Style")
        );
        assert!(
            bench
                .document()
                .model()
                .dimension_styles
                .contains("Dimension Style 2")
        );

        // A name a line cannot carry is refused before anything runs.
        let lines = bench.lines().len();
        bench.fill("dimStyleNewName", "5\" arrows");
        assert!(!bench.dialog.new_style(&mut bench.host));
        assert_eq!(bench.lines().len(), lines);
        assert!(
            bench.dialog.problem().contains("double quote"),
            "{}",
            bench.dialog.problem()
        );
    }

    #[test]
    fn the_list_says_who_uses_each_style_and_delete_is_refused_while_one_does() {
        let mut host = interpreter_host();
        run(&mut host, "DIMSTYLE NEW site");
        run(&mut host, "DIMSTYLE NEW spare");
        run(&mut host, "LAYER NEW dims");
        run(&mut host, "LAYER DIMSTYLE dims site");
        let mut bench = Bench::new(host, DimensionStyleManagerDialog::new);
        assert_eq!(bench.rows("dimStyleList"), 3);
        let rows: Vec<String> = (0..3)
            .map(|row| bench.row("dimStyleList", row).join(": "))
            .collect();
        assert_eq!(
            rows,
            [
                "Standard: no layer (the default)",
                "site: 1 layer",
                "spare: no layer"
            ]
        );

        bench.dialog.select(bench.host.document(), "site");
        let before = bench.steps();
        assert!(!bench.dialog.delete_style(&mut bench.host));
        assert!(
            bench.dialog.problem().contains("dims"),
            "{}",
            bench.dialog.problem()
        );
        assert!(bench.document().model().dimension_styles.contains("site"));
        assert_eq!(bench.steps(), before);

        // A click on a row shows that style.
        bench.fill("dimStyleList", "spare");
        assert_eq!(bench.dialog.current(), "spare");
        assert!(
            bench.dialog.delete_style(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        assert_eq!(
            bench.lines().last().map(String::as_str),
            Some("DIMSTYLE DELETE spare")
        );
        assert!(!bench.document().model().dimension_styles.contains("spare"));
        assert_eq!(bench.dialog.current(), "Standard");
        assert!(bench.dialog.problem().is_empty());
    }

    #[test]
    fn the_preview_reads_the_form_and_the_sample_is_drawn() {
        let mut bench = manager();
        assert_eq!(bench.dialog.preview(), "a dimension of 10 reads as 10.000");
        let standard = bench.dialog.sample();
        assert!(!standard.is_null());
        // Some ink: the painter drew the dimension on the transparent sample.
        let mut inked = 0;
        for y in 0..standard.height() {
            for x in 0..standard.width() {
                if standard.pixel(x, y).a > 0 {
                    inked += 1;
                }
            }
        }
        assert!(inked > 50, "{inked}");

        // A drawing in metres dimensioned in millimetres, as DIMSTYLE LIST reads it
        // (CadInterpreter.DimensionStylesCanBeDefinedTunedAndAttachedToALayer).
        bench.fill("dimStyleScale", "1000");
        bench.fill("dimStyleDecimals", "1");
        bench.fill("dimStyleSuffix", "mm");
        assert_eq!(bench.dialog.preview(), "a dimension of 10 reads as 10000.0mm");

        // A field that does not read says which, and Apply runs nothing.
        bench.fill("dimStyleText", "tall");
        assert!(
            bench
                .dialog
                .preview()
                .contains("text height must be a number"),
            "{}",
            bench.dialog.preview()
        );
        assert!(!bench.dialog.apply(&mut bench.host));
        assert!(bench.lines().is_empty());
        assert!(bench.dialog.problem().contains("tall"));
        // One the verb refuses is refused in the verb's words.
        bench.fill("dimStyleText", "0");
        assert!(!bench.dialog.apply(&mut bench.host));
        assert_eq!(bench.lines().len(), 1);
        assert!(
            bench
                .dialog
                .problem()
                .contains("text height must be finite"),
            "{}",
            bench.dialog.problem()
        );
    }

    #[test]
    fn revert_drops_the_edits_and_a_change_made_elsewhere_shows_at_once() {
        let mut host = interpreter_host();
        run(&mut host, "DIMSTYLE NEW site");
        let mut bench = Bench::new(host, DimensionStyleManagerDialog::new);
        bench.dialog.select(bench.host.document(), "site");
        bench.draw();
        bench.fill("dimStyleText", "9");
        bench.dialog.revert(bench.host.document());
        bench.draw();
        assert_eq!(bench.value("dimStyleText"), "2.5");

        // An edit is kept through a change that leaves its style alone (a layer typed
        // elsewhere), and replaced by one that changes it.
        bench.fill("dimStyleText", "9");
        bench.run("LAYER NEW other");
        bench.draw();
        assert_eq!(bench.value("dimStyleText"), "9");
        bench.run("DIMSTYLE SET site TEXT 4 HEAD Dot");
        bench.draw();
        assert_eq!(bench.value("dimStyleText"), "4");
        assert_eq!(bench.value("dimStyleHead"), "Dot");
    }

    #[test]
    fn the_format_menu_opens_it_by_name() {
        let mut host = interpreter_host();
        let mut window = WorkbenchHost::new(AnnotationWorkbench::new(host.document()));
        let action = window
            .menus()
            .iter()
            .flat_map(|menu| menu.actions().into_iter().cloned().collect::<Vec<_>>())
            .find(|action| action.name == "formatDimensionStyles")
            .expect("the Format menu has the item");
        assert_eq!(
            action.dialog.as_deref(),
            Some("dimensionStyleManagerDialog"),
            "what the headless --dialog opens it by"
        );
        window
            .trigger(&mut host, "formatDimensionStyles")
            .expect("the item is triggered");
        let mut harness = Harness::new();
        harness
            .run(|ui| window.ui(ui, &mut host))
            .expect("the window is drawn");
        assert!(
            harness
                .target_record("dimensionStyleManagerDialog")
                .is_some()
        );
        // Its lines go through the executor the window lends it.
        harness
            .target("dimensionStyleManagerDialog")
            .fill("dimStyleNewName", "site");
        harness
            .run(|ui| window.ui(ui, &mut host))
            .expect("the name is typed");
        let dialog = window
            .workbench
            .dimension_style_manager()
            .expect("the dialog is made");
        assert_eq!(dialog.name(), "dimensionStyleManagerDialog");
        assert!(dialog.new_style(&mut host), "{}", dialog.problem());
        assert_eq!(host.lines(), ["DIMSTYLE NEW site"]);
    }
}
