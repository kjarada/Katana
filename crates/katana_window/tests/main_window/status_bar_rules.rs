//! The status bar's rules (main_window::panels, `status_rule`): the short rules that part the tray
//! of minimised items from the message, and the readouts at the right from one another.

use egui::Rect;
use katana_window::MainWindow;
use katana_window::drive::headless::Headless;
use katana_window::drive::registry::Snapshot;
use katana_window::main_window::panels;

mod status_bar_rules {
    use super::*;

    const RULE: &str = "StatusBarRule";

    /// The window drawn until it is idle, an error in its status bar so that the message has
    /// words to stand beside the tray's rule.
    fn drawn(window: &mut MainWindow) -> Snapshot {
        window.exec.log.log("NotFound: no layer nothing", true);
        let mut headless = Headless::new();
        headless
            .settle(window)
            .unwrap_or_else(|problem| panic!("{problem}"));
        headless.snapshot()
    }

    /// The clear space at the left and at the right of the rule `name`: to the nearest thing
    /// drawn in its row that shows - not another rule, not a readout showing nothing (no width),
    /// not a part that is hidden, and not a row of a panel above that its panel scrolls out of
    /// sight (its rectangle may reach down to the bar; what its clip leaves of it does not).
    fn gaps(frame: &Snapshot, name: &str) -> (f32, f32) {
        let rule = frame
            .control_anywhere(name)
            .unwrap_or_else(|| panic!("the status bar has a rule {name}"))
            .rect;
        let beside: Vec<Rect> = frame
            .controls
            .iter()
            .filter(|control| !control.name.starts_with(RULE) && control.visible)
            .map(|control| control.rect.intersect(control.clip))
            .filter(|seen| {
                seen.width() > 0.5 && seen.y_range().contains(rule.center().y)
            })
            .collect();
        let left = beside
            .iter()
            .filter(|rect| rect.max.x <= rule.min.x + 0.5)
            .map(|rect| rule.min.x - rect.max.x)
            .fold(f32::INFINITY, f32::min);
        let right = beside
            .iter()
            .filter(|rect| rect.min.x >= rule.max.x - 0.5)
            .map(|rect| rect.min.x - rule.max.x)
            .fold(f32::INFINITY, f32::min);
        (left, right)
    }

    fn assert_even(frame: &Snapshot, name: &str) {
        let (left, right) = gaps(frame, name);
        assert!(
            left.is_finite() && right.is_finite(),
            "{name} stands between two things: {left} at its left, {right} at its right"
        );
        assert!(
            (left - right).abs() <= 1.0,
            "{name} has {left} points at its left and {right} at its right"
        );
        // The one gap, a row's group gap (8 points, theme::style::GROUP_GAP).
        assert_eq!(panels::STATUS_RULE_GAP, 8.0);
        assert!(
            (left - 8.0).abs() <= 1.0 && (right - 8.0).abs() <= 1.0,
            "{name} has {left} and {right} points either side, not the gap of 8"
        );
    }

    // The ninth look audit: 16 and 16, 8 and 9, and 16 and 8 points either side of the readouts'
    // rules - a readout showing nothing still took a gap, and the CRS button's padding stood
    // between its words and its rule.
    #[test]
    fn each_rule_between_the_readouts_has_the_same_gap_on_both_sides() {
        let mut window = MainWindow::new();
        let frame = drawn(&mut window);
        assert!(
            frame.control_anywhere("StatusBarRule0").is_none(),
            "nothing is minimised, so there is no tray and no rule after it"
        );
        for name in ["StatusBarRule1", "StatusBarRule2", "StatusBarRule3"] {
            assert_even(&frame, name);
        }
        // The rules are in the order their names say, from the left.
        let x = |name: &str| frame.control_anywhere(name).expect("drawn").rect.min.x;
        assert!(x("StatusBarRule1") < x("StatusBarRule2"));
        assert!(x("StatusBarRule2") < x("StatusBarRule3"));
        // Each stands between the readouts it parts: the CRS and the counts, the counts and the
        // layer, the layer and the coordinates.
        let readout = |name: &str| frame.control_anywhere(name).expect("drawn").rect;
        assert!(readout("statusProjectCrs").max.x <= x("StatusBarRule1"));
        assert!(readout("statusSelectionCount").min.x >= x("StatusBarRule1"));
        assert!(readout("statusSelectionCount").max.x <= x("StatusBarRule2"));
        assert!(readout("statusLayer").min.x >= x("StatusBarRule2"));
        assert!(readout("statusLayer").max.x <= x("StatusBarRule3"));
        assert!(readout("statusCoordinates").min.x >= x("StatusBarRule3"));
    }

    // The ninth look audit: 8 points between the tray and its rule, 16 between the rule and the
    // message.
    #[test]
    fn the_rule_after_the_tray_has_the_same_gap_to_the_tray_and_to_the_message() {
        let mut window = MainWindow::new();
        // A frame first: the views' chrome, which holds the tray, is installed when the workspace
        // is first drawn, and a view minimised before that is not minimised at all.
        let _ = drawn(&mut window);
        let id = window.exec.workspace.view_ids()[0];
        window.exec.workspace.minimise_view(id);
        assert!(window.exec.workspace.is_minimised(id), "the premise: the view is minimised");
        let frame = drawn(&mut window);
        assert!(
            frame.target("MinimisedToolBar").is_some(),
            "the premise: the minimised view has a button in the tray"
        );
        let message = frame
            .control_anywhere("statusMessage")
            .expect("the message is on record");
        assert!(
            message.rect.width() > 0.5,
            "the premise: the bar shows a message ({:?})",
            message.text
        );
        for name in ["StatusBarRule0", "StatusBarRule1", "StatusBarRule2", "StatusBarRule3"] {
            assert_even(&frame, name);
        }
        // It is the tray's last button at the rule's left and the message at its right.
        let rule = frame.control_anywhere("StatusBarRule0").expect("drawn").rect;
        let tray = frame.target("MinimisedToolBar").expect("drawn").rect;
        assert!((rule.min.x - tray.max.x - 8.0).abs() <= 1.0);
        assert!((message.rect.min.x - rule.max.x - 8.0).abs() <= 1.0);
    }
}
