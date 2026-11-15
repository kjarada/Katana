//! The twins of tests/qt_widgets/annotation/test_leader_manager_positions.cpp: a leader's tip,
//! bends and note end typed as exact coordinates in the Leaders manager's Position group, sent as
//! one LEADER SET with the rest of the form and never rounded.

use katana_entity::CalloutShape;
use katana_geometry::Point2;
use katana_window::annotation::leader_manager::{LeaderManagerContext, LeaderManagerDialog};
use katana_window::host::RecordingHost;

use crate::support::{Bench, interpreter_host, leader, run};

fn manager(host: RecordingHost) -> Bench<LeaderManagerDialog> {
    Bench::new(host, |document| {
        LeaderManagerDialog::new(document, LeaderManagerContext::default())
    })
}

mod leader_manager_positions {
    use super::*;

    #[test]
    fn the_note_position_and_box_use_one_shared_command_without_rounding() {
        let mut host = interpreter_host();
        run(&mut host, "LEADER 0,0 5,5 10,5 text=Note");
        let mut bench = manager(host);
        assert!(bench.dialog.show_leader(bench.host.document(), 1));
        bench.draw();
        // The third point, the note end, chosen.
        assert_eq!(bench.value("leaderPositionPoint"), "Note end");
        assert_eq!(bench.value("leaderPositionX"), "10");
        assert_eq!(bench.value("leaderPositionY"), "5");
        bench.fill("leaderPositionX", "306005.123456789");
        bench.fill("leaderPositionY", "6270008.987654321");
        bench.fill("leaderCallout", "box");
        bench.fill("leaderLandingAngle", "180");
        let before = bench.steps();
        assert!(
            bench.dialog.apply(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        assert_eq!(bench.lines().len(), 1);
        assert!(
            bench.lines()[0].contains("vertex=2"),
            "{}",
            bench.lines()[0]
        );
        assert!(bench.lines()[0].contains("callout=box"));
        let shape = leader(&bench.host, 1);
        assert_eq!(
            shape.vertices.last().copied(),
            Some(Point2::new(306005.123456789, 6270008.987654321))
        );
        assert_eq!(shape.vertices[0], Point2::new(0.0, 0.0));
        assert_eq!(shape.callout, CalloutShape::Box);
        assert_eq!(bench.steps(), before + 1);
        bench.document_mut().undo().expect("the undo is done");
        bench.draw();
        assert_eq!(bench.value("leaderPositionX"), "10");
        assert_eq!(bench.value("leaderPositionY"), "5");
    }

    #[test]
    fn choosing_a_point_is_not_an_edit_and_invalid_input_runs_no_line() {
        let mut host = interpreter_host();
        run(&mut host, "LEADER 0,0 5,5 10,5 text=Note");
        let mut bench = manager(host);
        assert!(bench.dialog.show_leader(bench.host.document(), 1));
        bench.draw();
        bench.fill("leaderPositionPoint", "Bend 1");
        assert_eq!(bench.value("leaderPositionX"), "5");
        assert!(bench.dialog.apply(&mut bench.host));
        assert!(bench.lines().is_empty());
        bench.fill("leaderPositionX", "nan");
        assert!(!bench.dialog.apply(&mut bench.host));
        assert!(!bench.dialog.problem().is_empty());
        assert!(bench.lines().is_empty());
        bench.fill("leaderPositionX", "7.125");
        bench.fill("leaderPositionY", "9.5");
        assert!(
            bench.dialog.apply(&mut bench.host),
            "{}",
            bench.dialog.problem()
        );
        assert_eq!(leader(&bench.host, 1).vertices[1], Point2::new(7.125, 9.5));
    }

    #[test]
    fn an_attached_tip_is_shown_but_cannot_be_accidentally_detached_by_coordinates() {
        let mut host = interpreter_host();
        run(&mut host, "POINT 0,0");
        run(&mut host, "LEADER #1 10,5 text=Note");
        let mut bench = manager(host);
        assert!(bench.dialog.show_leader(bench.host.document(), 2));
        bench.draw();
        bench.fill("leaderPositionPoint", "Arrow tip");
        assert!(!bench.enabled("leaderPositionX"));
        assert!(!bench.enabled("leaderPositionY"));
        bench.fill("leaderPositionPoint", "Note end");
        assert!(bench.enabled("leaderPositionX"));
        assert!(bench.enabled("leaderPositionY"));
    }
}
