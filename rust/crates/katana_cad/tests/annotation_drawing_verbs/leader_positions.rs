//! tests/cad/annotation/test_leader_positions.cpp: a leader's points and frame
//! edited through LEADER SET, the grips and leader_edit's
//! move_leader_vertices - one operation, one undo step, an attached tip kept
//! attached.

use katana_cad::annotation::leader_edit::{LeaderVertexMove, move_leader_vertices};
use katana_cad::drawing::grips::{apply_grip_drag, grips_of};
use katana_cad::{GripDrag, GripKind};
use katana_entity::{AnchorRef, CalloutShape, Entity, Geometry, LeaderGeometry};
use katana_geometry::Point2;
use katana_math::PI;

use crate::{Session, double_eq, pt};

// One hot point: vertex `index` to `position`.
fn to(index: usize, position: Point2) -> LeaderVertexMove {
    LeaderVertexMove { index, position }
}

mod leader_positions {
    use super::*;

    #[test]
    fn a_point_and_its_frame_change_together_in_one_undo_step() {
        let mut s = Session::default();
        s.run("LEADER 0,0 5,5 10,5 text=Note");
        let original = s.entity(1).geometry;
        let before = s.steps();
        s.run("LEADER SET 1 vertex=1 at=7,8 callout=box landingangle=180 paper=3");
        let leader = s.leader(1);
        assert_eq!(leader.vertices, [pt(0.0, 0.0), pt(7.0, 8.0), pt(10.0, 5.0)]);
        assert_eq!(leader.callout, CalloutShape::Box);
        assert_eq!(leader.landing_angle, Some(PI));
        assert_eq!(leader.text, "Note");
        double_eq(leader.paper_height, 3.0);
        assert_eq!(s.steps(), before + 1);
        assert!(s.document.undo().is_ok());
        assert_eq!(s.entity(1).geometry, original);
        assert!(s.document.redo().is_ok());
        s.run("LEADER SET 1 landingangle=auto");
        assert!(s.leader(1).landing_angle.is_none());
    }

    #[test]
    fn note_and_tip_coordinates_are_editable_and_an_unchanged_point_adds_no_step() {
        let mut s = Session::default();
        s.run("LEADER 0,0 5,5 10,5 text=Note");
        s.run("LEADER SET 1 at=12,7");
        s.run("LEADER SET 1 vertex=0 at=2,3");
        assert_eq!(
            s.leader(1).vertices,
            [pt(2.0, 3.0), pt(5.0, 5.0), pt(12.0, 7.0)]
        );
        let before = s.steps();
        s.run("LEADER SET 1 vertex=1 at=5,5");
        assert_eq!(s.steps(), before);
    }

    #[test]
    fn invalid_point_edits_and_directions_leave_the_whole_leader_unchanged() {
        let mut s = Session::default();
        s.run("LEADER 0,0 10,5 text=Note");
        let before = s.entity(1);
        let steps = s.steps();
        for options in [
            "vertex=-1 at=2,3",
            "vertex=2 at=2,3",
            "vertex=nope at=2,3",
            "vertex=0",
            "vertex=0 at=nan,3",
            "vertex=1 at=0,0",
            "vertex=0 at=2,3 tip=4,5",
            "landingangle=nan",
            "landingangle=inf",
            "landingangle=wrong",
        ] {
            assert!(
                s.refused(&format!("LEADER SET 1 callout=box {options}")),
                "{options}"
            );
            assert_eq!(s.entity(1), before);
            assert_eq!(s.steps(), steps);
        }
    }

    #[test]
    fn frame_edits_take_the_shared_scope_but_absolute_point_edits_require_one_leader() {
        let mut s = Session::default();
        s.run("LEADER 0,0 10,5 text=A");
        s.run("LEADER 0,10 10,15 text=B");
        s.run("LINE 0,0 10,0");
        let before = s.steps();
        let frames = s.run("LEADER SET DRAWING callout=box WHERE TYPE=leader");
        assert!(frames.contains("matched=2"), "{frames}");
        assert_eq!(s.steps(), before + 1);
        assert_eq!(s.leader(1).callout, CalloutShape::Box);
        assert_eq!(s.leader(2).callout, CalloutShape::Box);
        assert!(s.refused("LEADER SET DRAWING vertex=1 at=20,20"));
        let empty = s.run("LEADER SET DRAWING vertex=1 at=20,20 WHERE LAYER=missing");
        assert!(empty.contains("matched=0"), "{empty}");
        assert_eq!(s.steps(), before + 1);
    }

    #[test]
    fn grips_and_the_verb_use_the_same_vertex_edit_and_keep_an_attached_tip_attached() {
        let mut leader = LeaderGeometry {
            vertices: vec![pt(0.0, 0.0), pt(5.0, 5.0), pt(10.0, 5.0)],
            text: String::from("Note"),
            callout: CalloutShape::Box,
            landing_angle: Some(PI),
            ..LeaderGeometry::default()
        };
        let mut entity = Entity {
            id: 7,
            geometry: Geometry::Leader(leader.clone()),
            ..Entity::default()
        };
        let grips = grips_of(&entity);
        assert_eq!(grips.len(), 3);
        assert_eq!(grips[0].kind, GripKind::LeaderVertex);
        let drag = GripDrag {
            grabbed: grips[1],
            target: pt(6.0, 9.0),
            also_hot: Vec::new(),
            insert_vertex: false,
        };
        let moved = match apply_grip_drag(&entity, &drag) {
            Ok(moved) => moved,
            Err(error) => panic!("{}", error.describe()),
        };
        let Ok(direct) = move_leader_vertices(&leader, &[to(1, pt(6.0, 9.0))]) else {
            panic!("the verb's edit was refused");
        };
        assert_eq!(moved.geometry, Geometry::Leader(direct.clone()));
        assert_eq!(direct.landing_angle, leader.landing_angle);
        assert_eq!(direct.callout, leader.callout);

        leader.tip_ref = AnchorRef {
            entity: 42,
            ..AnchorRef::default()
        };
        entity.geometry = Geometry::Leader(leader.clone());
        let attached = grips_of(&entity);
        assert_eq!(attached.len(), 2);
        assert_eq!(attached[0].index, 1);
        assert!(move_leader_vertices(&leader, &[to(0, pt(1.0, 1.0))]).is_err());
        let Ok(note) = move_leader_vertices(&leader, &[to(2, pt(15.0, 8.0))]) else {
            panic!("the note's end was refused");
        };
        assert_eq!(note.tip_ref, leader.tip_ref);
    }

    #[test]
    fn multiple_hot_points_move_atomically_and_invalid_coordinates_are_refused() {
        let leader = LeaderGeometry {
            vertices: vec![pt(0.0, 0.0), pt(1.0, 0.0), pt(2.0, 0.0)],
            text: String::from("Note"),
            ..LeaderGeometry::default()
        };
        let moved = match move_leader_vertices(
            &leader,
            &[
                to(0, pt(1.0, 0.0)),
                to(1, pt(2.0, 0.0)),
                to(2, pt(3.0, 0.0)),
            ],
        ) {
            Ok(moved) => moved,
            Err(error) => panic!("{}", error.describe()),
        };
        assert_eq!(moved.vertices, [pt(1.0, 0.0), pt(2.0, 0.0), pt(3.0, 0.0)]);
        assert!(move_leader_vertices(&leader, &[to(3, pt(2.0, 3.0))]).is_err());
        assert!(
            move_leader_vertices(&leader, &[to(1, pt(2.0, 3.0)), to(1, pt(4.0, 5.0))]).is_err()
        );
        assert!(move_leader_vertices(&leader, &[to(1, pt(f64::INFINITY, 0.0))]).is_err());
        assert!(
            move_leader_vertices(&leader, &[to(0, pt(-1e308, 0.0)), to(2, pt(1e308, 0.0))])
                .is_err()
        );
    }
}
