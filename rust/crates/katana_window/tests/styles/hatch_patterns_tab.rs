//! The twins of tests/qt_widgets/customisation/test_hatch_patterns_tab.cpp: the Hatch Patterns tab
//! of Format > Styles and Linetypes, driven by its object names as a person drives it, and judged
//! by the lines it hands the executor and by the Document. The executor is the real interpreter on
//! the same drawing, where the window's command line sends a HATCH line, so each line is also
//! known to do what it says.
//!
//! A cell of the family grid, which the C++ set the text of, is typed into through the tab's
//! `set_family_cell`.

use katana_entity::HatchPattern;
use katana_window::customisation::{CustomisationContext, HatchPatternsTab};
use katana_window::host::RecordingHost;
use katana_window::style_manager::StyleManagerDialog;

use crate::support::{Bench, interpreter_host, run};

/// A drawing with "brick" (45 degrees every 0.25) hatching the layer "site", on which a closed
/// polyline is drawn, and the tab over it.
fn fixture() -> Bench<HatchPatternsTab> {
    let mut host = interpreter_host();
    for line in [
        "HATCH NEW brick 45 0.25",
        "LAYER NEW site",
        "LAYER HATCH site brick",
        "LAYER SET site",
        "PLINE 0,0 10,0 10,10 0,10 CLOSE",
        "LAYER SET 0",
    ] {
        run(&mut host, line);
    }
    Bench::new(host, |document| {
        HatchPatternsTab::new(&CustomisationContext::with_runner(), document)
    })
}

fn pattern(bench: &Bench<HatchPatternsTab>, name: &str) -> Option<HatchPattern> {
    bench.document().model().hatch_patterns.find(name).cloned()
}

/// Chooses a pattern in the table, and the frames after it.
fn select(bench: &mut Bench<HatchPatternsTab>, name: &str) -> bool {
    let chosen = bench.dialog.select_pattern(bench.host.document(), name);
    bench.draw();
    chosen
}

/// A cell of the family grid typed into, and the frames after it.
fn type_cell(bench: &mut Bench<HatchPatternsTab>, row: usize, column: usize, text: &str) {
    bench
        .dialog
        .set_family_cell(bench.host.document(), row, column, text);
    bench.draw();
}

fn family_cell(bench: &Bench<HatchPatternsTab>, row: usize, column: usize) -> String {
    bench.dialog.families()[row][column].clone()
}

mod hatch_patterns_tab {
    use super::*;

    #[test]
    fn the_table_lists_every_pattern_with_its_kind_and_what_uses_it() {
        let bench = fixture();
        assert_eq!(bench.rows("hatchPatternTable"), 2);
        assert_eq!(bench.row("hatchPatternTable", 0)[0], "brick");
        assert_eq!(bench.row("hatchPatternTable", 0)[1], "1 family");
        assert!(
            bench.row("hatchPatternTable", 0)[2].starts_with("used by 1 layer"),
            "{}",
            bench.row("hatchPatternTable", 0)[2]
        );
        assert_eq!(bench.row("hatchPatternTable", 1)[0], "none");
        assert_eq!(bench.row("hatchPatternTable", 1)[1], "draws nothing");
        // Nothing chosen yet: nothing to edit.
        assert!(!bench.enabled("hatchPatternEditor"));
    }

    #[test]
    fn an_edit_is_saved_as_one_hatch_set_line_and_one_undo_step() {
        let mut bench = fixture();
        assert!(select(&mut bench, "brick"));
        assert_eq!(family_cell(&bench, 0, 0), "45");
        assert_eq!(family_cell(&bench, 0, 1), "0.25");
        assert!(!bench.enabled("hatchPatternSave"));

        type_cell(&mut bench, 0, 1, "0.5");
        // The grid's own change runs nothing.
        assert!(bench.lines().is_empty());
        bench.press("hatchFamilyAdd");
        type_cell(&mut bench, 1, 0, "135");
        type_cell(&mut bench, 1, 1, "0.5");
        let before = bench.steps();
        bench.press("hatchPatternSave");
        assert_eq!(bench.last_line(), "HATCH SET brick 45 0.5 135 0.5");
        let brick = pattern(&bench, "brick").expect("brick is there");
        assert_eq!(brick.families.len(), 2);
        assert_eq!(brick.families[0].spacing, 0.5);
        assert_eq!(bench.steps(), before + 1);
        assert_eq!(
            bench.text("hatchPatternStatus"),
            "hatch pattern brick updated (2 families)"
        );
        assert_eq!(bench.row("hatchPatternTable", 0)[1], "2 families");
        assert!(!bench.enabled("hatchPatternSave"));
    }

    #[test]
    fn solid_is_saved_as_hatch_set_solid_and_revert_puts_the_drawings_back() {
        let mut bench = fixture();
        select(&mut bench, "brick");
        bench.fill("hatchPatternSolid", "on");
        assert!(!bench.enabled("hatchPatternFamilies"));
        bench.press("hatchPatternRevert");
        assert!(!bench.checked("hatchPatternSolid"));
        assert!(bench.lines().is_empty());
        bench.fill("hatchPatternSolid", "on");
        bench.press("hatchPatternSave");
        assert_eq!(bench.last_line(), "HATCH SET brick SOLID");
        assert!(pattern(&bench, "brick").expect("brick is there").solid);
    }

    #[test]
    fn a_family_with_one_number_is_refused_before_anything_runs() {
        let mut bench = fixture();
        select(&mut bench, "brick");
        bench.press("hatchFamilyAdd");
        type_cell(&mut bench, 1, 0, "90");
        bench.press("hatchPatternSave");
        assert!(bench.lines().is_empty());
        assert_eq!(
            bench.text("hatchPatternStatus"),
            "family 2 needs an angle and a spacing"
        );
    }

    #[test]
    fn unsaved_edits_survive_a_reload_that_left_the_pattern_alone() {
        let mut bench = fixture();
        select(&mut bench, "brick");
        type_cell(&mut bench, 0, 1, "2");
        run(&mut bench.host, "LAYER NEW other");
        bench.draw();
        assert_eq!(family_cell(&bench, 0, 1), "2");
        // Changed under it, the editor shows the drawing's pattern again.
        run(&mut bench.host, "HATCH SET brick 45 0.75");
        bench.draw();
        assert_eq!(family_cell(&bench, 0, 1), "0.75");
    }

    #[test]
    fn new_new_solid_and_duplicate_run_their_lines_under_the_typed_name() {
        let mut bench = fixture();
        select(&mut bench, "brick");
        // New takes the families in the grid.
        type_cell(&mut bench, 0, 0, "30");
        bench.fill("hatchPatternName", "slope");
        bench.press("hatchPatternNew");
        assert_eq!(bench.last_line(), "HATCH NEW slope 30 0.25");
        assert!(pattern(&bench, "slope").is_some());
        assert_eq!(bench.dialog.current_pattern(), "slope");

        bench.fill("hatchPatternName", "fill");
        bench.press("hatchPatternNewSolid");
        assert_eq!(bench.last_line(), "HATCH SOLID fill");
        assert!(pattern(&bench, "fill").is_some());

        // Duplicate takes the chosen pattern as the drawing holds it, in the degrees it was typed
        // in.
        select(&mut bench, "slope");
        bench.fill("hatchPatternName", "slope copy");
        bench.press("hatchPatternDuplicate");
        assert_eq!(bench.last_line(), "HATCH NEW \"slope copy\" 30 0.25");
        let copy = pattern(&bench, "slope copy").expect("the copy is there");
        assert_eq!(
            copy.families,
            pattern(&bench, "slope").expect("slope is there").families
        );
    }

    #[test]
    fn delete_is_refused_while_a_layer_names_the_pattern_and_purge_takes_the_unused() {
        let mut bench = fixture();
        run(&mut bench.host, "HATCH SOLID fill");
        bench.draw();
        select(&mut bench, "brick");
        bench.press("hatchPatternDelete");
        assert_eq!(bench.last_line(), "HATCH DELETE brick");
        assert!(pattern(&bench, "brick").is_some());
        assert!(
            bench.text("hatchPatternStatus").contains("site"),
            "{}",
            bench.text("hatchPatternStatus")
        );

        bench.press("hatchPatternPurge");
        assert_eq!(bench.last_line(), "PURGE HATCHES");
        assert!(pattern(&bench, "fill").is_none());
        assert!(pattern(&bench, "brick").is_some());
        assert!(pattern(&bench, "none").is_some());
    }

    #[test]
    fn none_is_neither_edited_nor_deleted() {
        let mut bench = fixture();
        select(&mut bench, "none");
        assert!(!bench.enabled("hatchPatternEditor"));
        assert!(!bench.enabled("hatchPatternDelete"));
        assert!(!bench.enabled("hatchPatternDuplicate"));
    }

    #[test]
    fn select_users_selects_what_is_hatched_with_the_pattern() {
        let mut bench = fixture();
        select(&mut bench, "brick");
        bench.press("hatchPatternSelectUsers");
        let ids = bench.document().selection().ids();
        assert_eq!(ids.len(), 1);
        let entity = bench.document().model().entities.find(ids[0]);
        assert_eq!(entity.expect("the polyline is there").layer, "site");
        assert_eq!(bench.text("hatchPatternStatus"), "1 entity selected.");
    }

    #[test]
    fn new_style_using_this_makes_a_style_that_hatches_with_it_in_one_step() {
        let mut bench = fixture();
        select(&mut bench, "brick");
        bench.fill("hatchPatternName", "brick paving");
        let before = bench.steps();
        bench.press("hatchPatternNewStyle");
        assert_eq!(
            bench.last_line(),
            "STYLE NEW \"brick paving\" HATCH brick"
        );
        let made = bench.document().model().styles.find("brick paving");
        assert_eq!(made.expect("the style is made").hatch_pattern, "brick");
        assert_eq!(bench.steps(), before + 1);
    }

    #[test]
    fn without_an_executor_it_says_so_and_changes_nothing() {
        let mut bench = Bench::new(RecordingHost::new(), |document| {
            HatchPatternsTab::new(&CustomisationContext::default(), document)
        });
        bench.fill("hatchPatternName", "fill");
        bench.press("hatchPatternNewSolid");
        assert!(
            bench
                .text("hatchPatternStatus")
                .starts_with("Nothing here"),
            "{}",
            bench.text("hatchPatternStatus")
        );
        assert!(bench.lines().is_empty());
        assert!(pattern(&bench, "fill").is_none());
    }

    #[test]
    fn the_styles_and_linetypes_manager_carries_the_tab() {
        let Bench { host, .. } = fixture();
        let manager = Bench::new(host, |document| {
            StyleManagerDialog::new(&CustomisationContext::with_runner(), document)
        });
        assert_eq!(
            manager.items("managerTabs"),
            ["Styles", "Linetypes", "Hatch Patterns", "Diagnostics"]
        );
        assert!(manager.has("hatchPatternsPage"));
    }
}
