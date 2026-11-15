//! The twins of tests/qt_widgets/customisation/test_shared_filter_bar.cpp: the search field and
//! exclusive chips above every manager's table, found by object name as a test or the headless
//! driver finds them.

use katana_window::customisation::{FilterBar, FilterBarChanges};
use katana_window::drive::{ControlKind, Harness};

mod filter_bar {
    use super::*;

    /// Frames of the bar until idle; every change it reported, in order.
    fn draw(harness: &mut Harness, bar: &mut FilterBar) -> Vec<FilterBarChanges> {
        let mut reported = Vec::new();
        harness
            .run(|ui| {
                let changes = bar.ui(ui);
                if changes.any() {
                    reported.push(changes);
                }
            })
            .expect("the bar is drawn");
        reported
    }

    #[test]
    fn the_search_field_and_each_chip_are_found_by_their_object_names() {
        let mut bar = FilterBar::new(&["All", "Used", "Unused", "Missing"]);
        let mut harness = Harness::new();
        draw(&mut harness, &mut bar);
        let field = harness.control("filterText").expect("filterText");
        assert_eq!(field.kind, ControlKind::Line);
        for name in ["filterAll", "filterUsed", "filterUnused", "filterMissing"] {
            let chip = harness.control(name).unwrap_or_else(|| panic!("{name}"));
            assert_eq!(chip.kind, ControlKind::Toggle, "{name}");
        }
        // Spaces and punctuation are not part of a name.
        assert_eq!(
            FilterBar::chip_object_name("Not used (3)"),
            "filterNotused3"
        );
    }

    #[test]
    fn the_chips_are_exclusive_the_first_starts_checked_and_a_click_reports_its_index() {
        let mut bar = FilterBar::new(&["All", "Used", "Unused"]);
        let mut harness = Harness::new();
        let mut reported: Vec<usize> = Vec::new();
        draw(&mut harness, &mut bar);
        assert_eq!(bar.chip(), Some(0));

        harness.press("filterUnused");
        for changes in draw(&mut harness, &mut bar) {
            reported.extend(changes.chips);
        }
        assert_eq!(bar.chip(), Some(2));
        assert_eq!(bar.chip_label(), "Unused");
        assert_eq!(
            harness.control("filterAll").expect("filterAll").checked,
            Some(false)
        );
        // Once, for the chip turned on - not again for the one turned off.
        assert_eq!(reported, [2]);

        // Clicking the checked chip leaves it checked: there is always a filter.
        harness.press("filterUnused");
        for changes in draw(&mut harness, &mut bar) {
            reported.extend(changes.chips);
        }
        assert_eq!(bar.chip(), Some(2));
        assert_eq!(reported, [2]);

        bar.set_chip(1);
        reported.extend(bar.take_changes().chips);
        assert_eq!(reported, [2, 1]);
        bar.set_chip(9); // out of range: ignored
        assert_eq!(bar.chip(), Some(1));
    }

    #[test]
    fn typing_reports_the_text_as_typed_with_its_case_kept() {
        let mut bar = FilterBar::new(&["All"]);
        let mut harness = Harness::new();
        draw(&mut harness, &mut bar);
        harness.fill("filterText", "KeRb");
        let reported = draw(&mut harness, &mut bar);
        assert_eq!(
            reported.last().and_then(|changes| changes.text.clone()),
            Some(String::from("KeRb"))
        );
        assert_eq!(bar.text(), "KeRb");
    }

    #[test]
    fn a_relabelled_chip_keeps_the_object_name_of_its_first_label() {
        let mut bar = FilterBar::new(&["All", "Missing"]);
        bar.set_chip_label(1, "Missing (3)");
        let mut harness = Harness::new();
        draw(&mut harness, &mut bar);
        let missing = harness.control("filterMissing").expect("filterMissing");
        assert_eq!(missing.text, "Missing (3)");
    }
}
