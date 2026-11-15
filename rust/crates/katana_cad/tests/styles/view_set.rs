// The open views of the workspace (PLAN.MD 47; tests/cad/test_view_set.cpp).
//
// ViewSet is the model the dock workspace is a picture of: which views are
// open, under which never-reused id, which one is active, what each is called
// and what camera it starts with. Every expectation is worked out from the
// module's contract - ids by counting, "most recent" by replaying the clicks,
// camera directions from the definition of the standard views in render's
// camera - not read back from a run.
//
// The C++ holds a reference to a view's state across the set's changes, as
// its widget does; Rust lends none across them, so these reach a view again
// by its id, and compare addresses where the C++ compared pointers.

use std::collections::{BTreeMap, BTreeSet};

use katana_cad::view_set::{
    MAXIMUM_PAPER_STYLE_SCALE, NO_VIEW, restore_view_paper_scales, set_view_paper_scales,
    validate_paper_style_scale, view_paper_scales, view_paper_scales_from_json,
    view_paper_scales_to_json,
};
use katana_cad::{Document, Section, ViewId, ViewKind, ViewPaperScales, ViewSet, ViewState};
use katana_commands::{EntityAttributes, create_line};
use katana_core::ErrorCode;
use katana_entity::Geometry;
use katana_geometry::{Point2, Point3};
use katana_math::Vec3;
use katana_render::{Camera, Projection};

use crate::{ScratchDirectory, must, near};

fn ids_of(set: &ViewSet) -> Vec<ViewId> {
    set.views().iter().map(|view| view.id).collect()
}

fn view(set: &ViewSet, id: ViewId) -> &ViewState {
    set.find(id).expect("the view is open")
}

fn view_mut(set: &mut ViewSet, id: ViewId) -> &mut ViewState {
    set.find_mut(id).expect("the view is open")
}

// Where the camera looks, against a direction worked out by hand.
#[track_caller]
fn expect_looking(camera: &Camera, expected: Vec3, tolerance: f64) {
    let forward = camera.forward();
    near(forward.x, expected.x, tolerance);
    near(forward.y, expected.y, tolerance);
    near(forward.z, expected.z, tolerance);
}

// An orbit no standard view produces, so "untouched" is distinguishable from
// "reset to the same place".
fn orbit_somewhere_odd(camera: &mut Camera) {
    camera.set_projection(Projection::Perspective);
    camera.set_orientation(1.0, 0.3);
    camera.set_distance(42.0);
    camera.set_target(Point3::new(1.0, 2.0, 3.0));
}

#[track_caller]
fn expect_same_camera(actual: &Camera, expected: &Camera) {
    assert_eq!(actual.projection(), expected.projection());
    assert_eq!(actual.azimuth(), expected.azimuth());
    assert_eq!(actual.elevation(), expected.elevation());
    assert_eq!(actual.distance(), expected.distance());
    assert_eq!(actual.target().x, expected.target().x);
    assert_eq!(actual.target().y, expected.target().y);
    assert_eq!(actual.target().z, expected.target().z);
}

// The directions render's camera defines. Front looks north (from -Y). An
// isometric view from the south-west looks north-east and down at the
// isometric elevation atan(1/sqrt 2), which is exactly the angle that makes
// all three components equal: (1, 1, -1) / sqrt 3.
fn looking_north() -> Vec3 {
    Vec3::new(0.0, 1.0, 0.0)
}

fn looking_north_east_and_down() -> Vec3 {
    let third = 1.0 / 3.0_f64.sqrt();
    Vec3::new(third, third, -third)
}

fn looking_down() -> Vec3 {
    Vec3::new(0.0, 0.0, -1.0)
}

// Top cannot look exactly down: the camera clamps elevation "just inside the
// poles" so that up never becomes parallel to the view direction. The guard
// in camera.rs is 1e-3 rad, which leaves the x-y components at sin(1e-3),
// about 1e-3; 2e-3 bounds that without depending on its exact value.
const POLE_TOLERANCE: f64 = 2.0e-3;
// cos and sin of multiples of pi/4 in doubles: a few ulps.
const DIRECTION_TOLERANCE: f64 = 1.0e-12;

fn scales(entries: &[(ViewId, Option<f64>)]) -> ViewPaperScales {
    entries.iter().copied().collect::<BTreeMap<_, _>>()
}

// The link keeps plan views showing the same centre at the same scale. Every
// number in the linked-view cases is worked by hand from ViewTransform's
// definitions: a view W by H pixels centred on c at s pixels a unit puts
// screen (x, y) at world (c.x + (x - W/2) / s, c.y - (y - H/2) / s), and
// zoom_at(p, k) keeps the world point under p where it was while the scale
// goes to k s.

// A plan view W x H pixels, centred on (x, y) at `scale`, framed as a widget
// that has painted it would be.
fn framed_plan(set: &mut ViewSet, width: f64, height: f64, x: f64, y: f64, scale: f64) -> ViewId {
    let added = set.add(ViewKind::Plan);
    added.plan.resize(width, height);
    added.plan.center = Point2::new(x, y);
    added.plan.scale = scale;
    added.plan_framed = true;
    added.id
}

#[track_caller]
fn expect_showing(set: &ViewSet, id: ViewId, x: f64, y: f64, scale: f64) {
    let found = view(set, id);
    assert_eq!(found.plan.center.x, x, "view {id}");
    assert_eq!(found.plan.center.y, y, "view {id}");
    assert_eq!(found.plan.scale, scale, "view {id}");
}

mod view_set {
    use super::*;

    // ---- ids and the active view --------------------------------------------------------

    #[test]
    fn an_empty_set_has_no_active_view() {
        let set = ViewSet::default();
        assert!(set.empty());
        assert_eq!(set.size(), 0);
        assert_eq!(set.active_id(), NO_VIEW);
        assert!(set.active().is_none());
        assert!(set.views().is_empty());
    }

    #[test]
    fn paper_style_scale_starts_automatic_and_survives_a_change_of_view_kind() {
        let mut set = ViewSet::default();
        let (id, centre, zoom) = {
            let added = set.add(ViewKind::Plan);
            assert!(added.paper_style_scale.is_none());
            added.paper_style_scale = Some(500.0);
            (added.id, added.plan.center, added.plan.scale)
        };
        assert!(set.set_kind(id, ViewKind::Model3D).is_ok());
        assert!(set.set_kind(id, ViewKind::Plan).is_ok());
        let found = view(&set, id);
        assert_eq!(found.paper_style_scale, Some(500.0));
        assert_eq!(found.plan.center, centre);
        assert_eq!(found.plan.scale, zoom);
    }

    #[test]
    fn paper_style_scales_round_trip_in_versioned_json_including_automatic_and_fractional_scales() {
        let given = scales(&[(1, Some(500.125)), (3, None)]);
        let json = view_paper_scales_to_json(&given).expect("written");
        assert!(json.contains("\"version\":1"));
        let read = view_paper_scales_from_json(&json).expect("read back");
        assert_eq!(read, given);
        let empty = view_paper_scales_from_json(r#"{"version":1,"views":[]}"#).expect("read");
        assert!(empty.is_empty());
    }

    #[test]
    fn paper_style_scale_rejects_nonpositive_nonfinite_unrepresentable_and_enormous_values() {
        // The last is the smallest positive double (denorm_min).
        for value in [
            0.0,
            -1.0,
            f64::INFINITY,
            f64::NAN,
            1.0e300,
            f64::from_bits(1),
        ] {
            assert!(validate_paper_style_scale(Some(value)).is_err(), "{value}");
            assert!(
                view_paper_scales_to_json(&scales(&[(1, Some(value))])).is_err(),
                "{value}"
            );
        }
        assert!(validate_paper_style_scale(None).is_ok());
        assert!(validate_paper_style_scale(Some(0.5)).is_ok());
        assert!(validate_paper_style_scale(Some(MAXIMUM_PAPER_STYLE_SCALE)).is_ok());
        assert!(validate_paper_style_scale(Some(MAXIMUM_PAPER_STYLE_SCALE + 1.0)).is_err());
        assert!(view_paper_scales_to_json(&scales(&[(0, Some(500.0))])).is_err());
    }

    #[test]
    fn invalid_or_newer_paper_style_scale_json_is_refused_rather_than_reinterpreted() {
        for text in [
            "",
            "{}",
            "[]",
            r#"{"version":1,"views":{}}"#,
            r#"{"version":1,"views":[{"view":0,"paper_scale":500}]}"#,
            r#"{"version":1,"views":[{"view":-1,"paper_scale":500}]}"#,
            r#"{"version":1,"views":[{"view":1.5,"paper_scale":500}]}"#,
            r#"{"version":1,"views":[{"view":4294967296,"paper_scale":500}]}"#,
            r#"{"version":1,"views":[{"view":1,"paper_scale":0}]}"#,
            r#"{"version":1,"views":[{"view":1,"paper_scale":1e300}]}"#,
            r#"{"version":1,"views":[{"view":1,"paper_scale":"500"}]}"#,
            r#"{"version":1,"views":[{"view":1},{"view":1}]}"#,
        ] {
            assert!(view_paper_scales_from_json(text).is_err(), "{text}");
        }
        let newer = view_paper_scales_from_json(r#"{"version":2,"views":[]}"#)
            .expect_err("a newer version");
        assert_eq!(newer.code, ErrorCode::Unsupported);
    }

    #[test]
    fn paper_style_scale_is_one_undo_step_and_does_not_change_geometry_annotation_or_other_metadata()
     {
        let mut document = Document::default();
        let mut metadata = document.metadata().clone();
        metadata.description = String::from("keep this");
        metadata
            .unknown_keys
            .insert(String::from("unrelated"), String::from("unchanged"));
        document.set_metadata(metadata.clone());
        must(
            &mut document,
            create_line(
                Point2::new(-4.0, 2.0),
                Point2::new(7.0, 3.0),
                EntityAttributes::default(),
            ),
        );
        let annotation = document.annotation_scale();
        let given = scales(&[(1, Some(250.5)), (2, None)]);
        assert!(set_view_paper_scales(&mut document, &given).is_ok());
        assert_eq!(document.history().undo_count(), 2);
        assert!(set_view_paper_scales(&mut document, &given).is_ok());
        assert_eq!(
            document.history().undo_count(),
            2,
            "an unchanged setting is not an edit"
        );
        assert_eq!(document.model().entities.size(), 1);
        let entity = document.model().entities.find(1).expect("the line");
        let Geometry::Line(line) = &entity.geometry else {
            panic!("a line");
        };
        assert_eq!(line.start, Point2::new(-4.0, 2.0));
        assert_eq!(line.end, Point2::new(7.0, 3.0));
        assert_eq!(document.annotation_scale(), annotation);
        assert_eq!(document.metadata().description, "keep this");
        assert_eq!(document.metadata().unknown_keys["unrelated"], "unchanged");
        assert!(document.undo().is_ok());
        assert_eq!(*document.metadata(), metadata);
        assert!(document.redo().is_ok());
        let after = view_paper_scales(&document).expect("read");
        assert_eq!(after, given);
    }

    #[test]
    fn paper_style_scale_saved_with_the_project_restores_and_undo_back_to_a_save_is_clean() {
        let scratch = ScratchDirectory::new("view-paper-scale-roundtrip");
        let path = scratch.path.join("view-paper-scale-roundtrip.katana");
        let mut document = Document::default();
        assert!(set_view_paper_scales(&mut document, &scales(&[(1, Some(500.125))])).is_ok());
        assert!(document.save_as(&path).is_ok());
        assert!(!document.is_modified());
        assert!(set_view_paper_scales(&mut document, &scales(&[(1, Some(1000.0))])).is_ok());
        assert!(document.undo().is_ok());
        assert!(!document.is_modified());
        let mut reopened = Document::default();
        assert!(reopened.open(&path).is_ok());
        let kept = view_paper_scales(&reopened).expect("read");
        assert_eq!(kept.get(&1).copied().flatten(), Some(500.125));
        let mut views = ViewSet::default();
        let id = {
            let added = views.add(ViewKind::Plan);
            added.plan.scale = 8.0;
            added.id
        };
        let restored = restore_view_paper_scales(&reopened, &mut views).expect("restored");
        assert_eq!(restored, vec![1]);
        assert_eq!(view(&views, id).paper_style_scale, Some(500.125));
        assert_eq!(view(&views, id).plan.scale, 8.0);
    }

    #[test]
    fn damaged_or_newer_saved_paper_scales_are_kept_and_edits_are_refused() {
        for text in ["not JSON", r#"{"version":2,"views":[]}"#] {
            let mut document = Document::default();
            let mut metadata = document.metadata().clone();
            metadata
                .unknown_keys
                .insert(String::from("view_paper_scales"), String::from(text));
            document.set_metadata(metadata.clone());
            assert!(set_view_paper_scales(&mut document, &scales(&[(1, Some(500.0))])).is_err());
            assert_eq!(*document.metadata(), metadata);
            assert_eq!(document.history().undo_count(), 0);
        }
    }

    #[test]
    fn ids_count_up_from_one_and_are_never_reused_after_a_remove() {
        // A dock that outlives its view by a queued event must not find a
        // different view under the same id.
        let mut set = ViewSet::default();
        assert_eq!(set.add(ViewKind::Plan).id, 1);
        assert_eq!(set.add(ViewKind::Model3D).id, 2);
        assert!(set.remove(2).is_ok());
        assert_eq!(set.add(ViewKind::Model3D).id, 3, "2 is gone, not free");
        assert!(set.remove(1).is_ok());
        assert!(set.remove(3).is_ok());
        assert!(set.empty());
        assert_eq!(
            set.add(ViewKind::Plan).id,
            4,
            "not even once the set is empty"
        );
    }

    #[test]
    fn the_first_view_added_becomes_active_and_later_ones_do_not() {
        let mut set = ViewSet::default();
        let first = set.add(ViewKind::Plan).id;
        assert_eq!(set.active_id(), first);
        assert_eq!(set.active().map(|active| active.id), Some(first));

        set.add(ViewKind::Model3D);
        set.add(ViewKind::Section);
        assert_eq!(
            set.active_id(),
            first,
            "opening a view and activating it are separate"
        );
    }

    #[test]
    fn activating_an_unknown_view_is_not_found_and_changes_nothing() {
        let mut set = ViewSet::default();
        set.add(ViewKind::Plan);
        let second = set.add(ViewKind::Model3D).id;
        assert!(set.activate(second).is_ok());

        for unknown in [99, NO_VIEW] {
            let refused = set.activate(unknown).expect_err("unknown");
            assert_eq!(refused.code, ErrorCode::NotFound);
            assert_eq!(
                set.active_id(),
                second,
                "no other view may be activated instead"
            );
        }
    }

    #[test]
    fn removing_an_unknown_view_is_not_found_and_changes_nothing() {
        let mut set = ViewSet::default();
        set.add(ViewKind::Plan);
        set.add(ViewKind::Model3D);
        for unknown in [99, NO_VIEW] {
            let refused = set.remove(unknown).expect_err("unknown");
            assert_eq!(refused.code, ErrorCode::NotFound);
        }
        assert_eq!(set.size(), 2);
        assert_eq!(set.active_id(), 1);
    }

    #[test]
    fn removing_the_active_view_activates_the_most_recently_active_of_the_rest() {
        // Clicks: 1 (active on opening), 3, 2, 4. The order of recent activity
        // is therefore 1 < 3 < 2 < 4, and each removal of the active view must
        // fall back one step down that list.
        let mut set = ViewSet::default();
        for _ in 0..4 {
            set.add(ViewKind::Plan);
        }
        assert!(set.activate(3).is_ok());
        assert!(set.activate(2).is_ok());
        assert!(set.activate(4).is_ok());

        assert!(set.remove(4).is_ok());
        assert_eq!(set.active_id(), 2);
        assert!(set.remove(2).is_ok());
        assert_eq!(set.active_id(), 3);
        assert!(set.remove(3).is_ok());
        assert_eq!(set.active_id(), 1);
        assert!(set.remove(1).is_ok());
        assert_eq!(set.active_id(), NO_VIEW, "never an id that is not open");
        assert!(set.active().is_none());
    }

    #[test]
    fn removing_a_view_that_is_not_active_leaves_the_active_view_alone() {
        let mut set = ViewSet::default();
        set.add(ViewKind::Plan);
        set.add(ViewKind::Model3D);
        set.add(ViewKind::Section);
        assert!(set.activate(2).is_ok());
        assert!(set.remove(3).is_ok());
        assert_eq!(set.active_id(), 2);
        assert!(set.remove(1).is_ok());
        assert_eq!(set.active_id(), 2);
    }

    // ---- restore ------------------------------------------------------------------------------

    #[test]
    fn restoring_a_view_keeps_its_id_and_moves_the_counter_past_it() {
        let mut set = ViewSet::default();
        let restored: *const ViewState = {
            let restored = set.restore(10, ViewKind::Model3D).expect("restored");
            assert_eq!(restored.id, 10);
            assert_eq!(restored.kind, ViewKind::Model3D);
            restored
        };
        assert!(std::ptr::eq(view(&set, 10), restored));
        assert_eq!(
            set.active_id(),
            10,
            "the first view into an empty set, like add()"
        );

        assert_eq!(set.add(ViewKind::Plan).id, 11);
    }

    #[test]
    fn restoring_an_id_below_the_counter_does_not_lower_it() {
        let mut set = ViewSet::default();
        set.add(ViewKind::Plan);
        set.add(ViewKind::Plan);
        set.add(ViewKind::Plan);
        assert!(set.remove(2).is_ok());

        let restored = set.restore(2, ViewKind::Section).expect("restored");
        assert_eq!(restored.id, 2);
        assert_eq!(
            set.add(ViewKind::Plan).id,
            4,
            "3 was issued; the next is 4, not 3"
        );
    }

    #[test]
    fn restoring_the_no_view_id_or_an_open_id_is_refused() {
        let mut set = ViewSet::default();
        set.add(ViewKind::Plan);

        let zero = set
            .restore(NO_VIEW, ViewKind::Plan)
            .expect_err("no view id");
        assert_eq!(zero.code, ErrorCode::InvalidArgument);

        let open = set.restore(1, ViewKind::Model3D).expect_err("open");
        assert_eq!(open.code, ErrorCode::AlreadyExists);
        assert_eq!(
            view(&set, 1).kind,
            ViewKind::Plan,
            "the open view is not replaced"
        );

        assert_eq!(set.size(), 1);
        assert_eq!(
            set.add(ViewKind::Plan).id,
            2,
            "a refused restore moves no counter"
        );
    }

    // ---- take ---------------------------------------------------------------------------------

    #[test]
    fn taking_a_view_hands_over_its_state_and_forgets_it() {
        let mut set = ViewSet::default();
        set.add(ViewKind::Plan);
        let second: *const ViewState = {
            let added = set.add(ViewKind::Model3D);
            assert!(added.layers.hide("survey"));
            added
        };
        assert!(set.activate(2).is_ok());

        let taken = set.take(2).expect("taken");
        assert!(
            std::ptr::eq(&*taken, second),
            "the same object, which a closing widget still holds"
        );
        assert_eq!(taken.id, 2);
        assert!(taken.layers.hides_directly("survey"));

        assert!(set.find(2).is_none(), "unfindable at once");
        assert_eq!(set.active_id(), 1, "and no longer active");
        assert_eq!(set.size(), 1);
        assert_eq!(set.count(ViewKind::Model3D), 0);

        assert!(set.take(2).is_none(), "a second take finds nothing");
        assert!(set.take(99).is_none());
        assert_eq!(
            set.add(ViewKind::Plan).id,
            3,
            "a taken id is not reused either"
        );
    }

    // ---- kind and camera ------------------------------------------------------------------------

    #[test]
    fn a_new_view_starts_with_the_camera_of_its_kind() {
        let mut set = ViewSet::default();
        let plan = set.add(ViewKind::Plan).camera;
        assert_eq!(plan.projection(), Projection::Orthographic);
        expect_looking(&plan, looking_down(), POLE_TOLERANCE);

        let model = set.add(ViewKind::Model3D).camera;
        assert_eq!(model.projection(), Projection::Perspective);
        expect_looking(&model, looking_north_east_and_down(), DIRECTION_TOLERANCE);

        // Orthographic, because a side view is read off with a scale rule.
        let elevation = set.add(ViewKind::Elevation).camera;
        assert_eq!(elevation.projection(), Projection::Orthographic);
        expect_looking(&elevation, looking_north(), DIRECTION_TOLERANCE);

        // A section draws in its own station/elevation transform and leaves the
        // camera as a default-constructed one.
        let section = set.add(ViewKind::Section).camera;
        expect_same_camera(&section, &Camera::default());
    }

    #[test]
    fn setting_the_kind_of_an_unknown_view_is_not_found_and_changes_nothing() {
        let mut set = ViewSet::default();
        set.add(ViewKind::Plan);
        let refused = set.set_kind(7, ViewKind::Model3D).expect_err("unknown");
        assert_eq!(refused.code, ErrorCode::NotFound);
        assert_eq!(set.count(ViewKind::Model3D), 0);
    }

    #[test]
    fn choosing_the_kind_a_view_already_has_leaves_its_camera_alone() {
        // "A 3D view's orbit is not reset by choosing 3D again."
        let mut set = ViewSet::default();
        let id = set.add(ViewKind::Model3D).id;
        orbit_somewhere_odd(&mut view_mut(&mut set, id).camera);
        let orbited = view(&set, id).camera;

        assert!(set.set_kind(id, ViewKind::Model3D).is_ok());
        expect_same_camera(&view(&set, id).camera, &orbited);
        assert_eq!(view(&set, id).number, 1);
    }

    #[test]
    fn changing_kind_gives_the_camera_that_kinds_starting_view() {
        let mut set = ViewSet::default();
        let id = set.add(ViewKind::Plan).id;

        assert!(set.set_kind(id, ViewKind::Model3D).is_ok());
        assert_eq!(view(&set, id).kind, ViewKind::Model3D);
        assert_eq!(view(&set, id).camera.projection(), Projection::Perspective);
        expect_looking(
            &view(&set, id).camera,
            looking_north_east_and_down(),
            DIRECTION_TOLERANCE,
        );

        orbit_somewhere_odd(&mut view_mut(&mut set, id).camera);
        assert!(set.set_kind(id, ViewKind::Elevation).is_ok());
        assert_eq!(view(&set, id).camera.projection(), Projection::Orthographic);
        expect_looking(&view(&set, id).camera, looking_north(), DIRECTION_TOLERANCE);

        orbit_somewhere_odd(&mut view_mut(&mut set, id).camera);
        assert!(set.set_kind(id, ViewKind::Plan).is_ok());
        assert_eq!(view(&set, id).camera.projection(), Projection::Orthographic);
        expect_looking(&view(&set, id).camera, looking_down(), POLE_TOLERANCE);
    }

    #[test]
    fn turning_a_view_into_a_section_leaves_its_camera_alone() {
        let mut set = ViewSet::default();
        let id = set.add(ViewKind::Model3D).id;
        orbit_somewhere_odd(&mut view_mut(&mut set, id).camera);
        let orbited = view(&set, id).camera;

        assert!(set.set_kind(id, ViewKind::Section).is_ok());
        assert_eq!(view(&set, id).kind, ViewKind::Section);
        expect_same_camera(&view(&set, id).camera, &orbited);
    }

    #[test]
    fn a_three_d_view_turned_into_a_section_and_back_keeps_its_orbit() {
        // configure_camera's contract (viewport_layout.rs): "A Section leaves
        // the camera alone, so switching back to the model kind the camera was
        // set up for restores what it had." Leaving it alone on the way in is
        // only half of that; coming back must not then reset it to the starting
        // view of the kind it already had.
        let mut set = ViewSet::default();
        let id = set.add(ViewKind::Model3D).id;
        orbit_somewhere_odd(&mut view_mut(&mut set, id).camera);
        let orbited = view(&set, id).camera;

        assert!(set.set_kind(id, ViewKind::Section).is_ok());
        assert!(set.set_kind(id, ViewKind::Model3D).is_ok());
        expect_same_camera(&view(&set, id).camera, &orbited);

        // The same for an elevation view, whose camera is a different start.
        let side = set.add(ViewKind::Elevation).id;
        view_mut(&mut set, side).camera.set_orientation(-1.2, 0.1);
        let turned = view(&set, side).camera;
        assert!(set.set_kind(side, ViewKind::Section).is_ok());
        assert!(set.set_kind(side, ViewKind::Elevation).is_ok());
        expect_same_camera(&view(&set, side).camera, &turned);
    }

    #[test]
    fn a_camera_pointed_afresh_is_no_longer_framed_and_one_left_alone_still_is() {
        // ViewState::camera_framed: "set_kind clears it whenever it points the
        // camera afresh". The 3D widget is rebuilt on every change of kind and
        // frames at its first paint unless this is set, so it must survive
        // exactly the changes that leave the camera alone - a trip through a
        // section, choosing the same kind - and no other.
        let mut set = ViewSet::default();
        let id = set.add(ViewKind::Model3D).id;
        assert!(!view(&set, id).camera_framed); // a new view has framed nothing yet

        view_mut(&mut set, id).camera_framed = true; // what the 3D widget's zoom extents records
        assert!(set.set_kind(id, ViewKind::Model3D).is_ok());
        assert!(view(&set, id).camera_framed);
        assert!(set.set_kind(id, ViewKind::Section).is_ok());
        assert!(view(&set, id).camera_framed);
        assert!(set.set_kind(id, ViewKind::Model3D).is_ok());
        assert!(view(&set, id).camera_framed);

        // 3D -> Elevation points the camera at the front: a frame from the 3D
        // orbit says nothing about what the elevation shows.
        assert!(set.set_kind(id, ViewKind::Elevation).is_ok());
        assert!(!view(&set, id).camera_framed);

        // Through a section to a DIFFERENT model kind: configured, so cleared.
        view_mut(&mut set, id).camera_framed = true;
        assert!(set.set_kind(id, ViewKind::Section).is_ok());
        assert!(set.set_kind(id, ViewKind::Model3D).is_ok());
        assert!(!view(&set, id).camera_framed);
    }

    #[test]
    fn a_section_turned_into_a_model_view_starts_from_that_kinds_view() {
        // Opened as a section, the camera was never pointed anywhere, so
        // arriving at a model kind must configure it.
        let mut set = ViewSet::default();
        let id = set.add(ViewKind::Section).id;
        assert!(set.set_kind(id, ViewKind::Model3D).is_ok());
        assert_eq!(view(&set, id).camera.projection(), Projection::Perspective);
        expect_looking(
            &view(&set, id).camera,
            looking_north_east_and_down(),
            DIRECTION_TOLERANCE,
        );

        // And a model view that went through a section to a DIFFERENT model
        // kind gets that kind's start, not the camera it had before.
        orbit_somewhere_odd(&mut view_mut(&mut set, id).camera);
        assert!(set.set_kind(id, ViewKind::Section).is_ok());
        assert!(set.set_kind(id, ViewKind::Elevation).is_ok());
        assert_eq!(view(&set, id).camera.projection(), Projection::Orthographic);
        expect_looking(&view(&set, id).camera, looking_north(), DIRECTION_TOLERANCE);
    }

    #[test]
    fn changing_kind_keeps_the_plan_zoom_the_layers_and_the_section() {
        // "Everything below survives a change of kind: switching a view to 3D
        // and back keeps its plan zoom, its hidden layers and its section."
        let mut set = ViewSet::default();
        let id = {
            let added = set.add(ViewKind::Plan);
            added.plan.center = Point2::new(512.0, -256.0);
            added.plan.scale = 0.125;
            added.plan_framed = true;
            assert!(added.layers.hide("survey"));
            added.hidden_references = BTreeSet::from([7, 9]);
            added.section = Some(Section {
                length: 12.5,
                ..Section::default()
            });
            added.section_exaggeration = 4.0;
            added.id
        };

        for through in [ViewKind::Model3D, ViewKind::Section, ViewKind::Elevation] {
            assert!(set.set_kind(id, through).is_ok());
            assert!(set.set_kind(id, ViewKind::Plan).is_ok());
            let found = view(&set, id);
            assert_eq!(found.plan.center, Point2::new(512.0, -256.0));
            assert_eq!(found.plan.scale, 0.125);
            assert!(found.plan_framed);
            assert!(found.layers.hides_directly("survey"));
            assert_eq!(found.hidden_references, BTreeSet::from([7, 9]));
            assert_eq!(
                found.section.as_ref().map(|section| section.length),
                Some(12.5)
            );
            assert_eq!(found.section_exaggeration, 4.0);
        }
    }

    // ---- numbering and titles ---------------------------------------------------------------------

    #[test]
    fn each_kind_is_numbered_with_the_lowest_number_no_open_view_of_that_kind_holds() {
        let mut set = ViewSet::default();
        assert_eq!(set.add(ViewKind::Plan).number, 1);
        assert_eq!(set.add(ViewKind::Plan).number, 2);
        assert_eq!(
            set.add(ViewKind::Model3D).number,
            1,
            "numbers count per kind"
        );
        assert_eq!(set.add(ViewKind::Plan).number, 3);

        assert!(set.remove(1).is_ok()); // "Plan 1"
        assert_eq!(
            set.add(ViewKind::Plan).number,
            1,
            "the lowest free, not a session counter"
        );
        assert_eq!(set.add(ViewKind::Plan).number, 4);
        assert_eq!(set.add(ViewKind::Section).number, 1);
        assert_eq!(
            view(&set, 2).number,
            2,
            "closing Plan 1 does not renumber Plan 2"
        );
    }

    #[test]
    fn changing_kind_renumbers_the_view_within_its_new_kind() {
        let mut set = ViewSet::default();
        let plan1 = set.add(ViewKind::Plan).id;
        let plan2 = set.add(ViewKind::Plan).id;
        let model1 = set.add(ViewKind::Model3D).id;

        assert!(set.set_kind(plan2, ViewKind::Model3D).is_ok());
        assert_eq!(view(&set, plan2).number, 2, "3D 1 is taken");
        assert_eq!(ViewSet::title(view(&set, plan2)), "3D 2");

        assert!(set.set_kind(model1, ViewKind::Plan).is_ok());
        assert_eq!(
            view(&set, model1).number,
            2,
            "Plan 1 is still open; Plan 2 became a 3D view"
        );

        assert!(set.set_kind(plan1, ViewKind::Elevation).is_ok());
        assert_eq!(ViewSet::title(view(&set, plan1)), "Elevation 1");

        // Choosing the kind it already has renumbers nothing.
        assert!(set.set_kind(plan2, ViewKind::Model3D).is_ok());
        assert_eq!(view(&set, plan2).number, 2);
    }

    #[test]
    fn a_title_is_the_kind_and_the_number() {
        let titled = |kind: ViewKind, number: i32| {
            ViewSet::title(&ViewState {
                kind,
                number,
                ..ViewState::default()
            })
        };
        assert_eq!(titled(ViewKind::Plan, 1), "Plan 1");
        assert_eq!(titled(ViewKind::Model3D, 2), "3D 2");
        assert_eq!(titled(ViewKind::Section, 1), "Section 1");
        assert_eq!(titled(ViewKind::Elevation, 12), "Elevation 12");
    }

    // ---- most_recent -------------------------------------------------------------------------------

    #[test]
    fn most_recent_is_the_active_view_when_it_is_of_that_kind() {
        let mut set = ViewSet::default();
        set.add(ViewKind::Model3D);
        let second = set.add(ViewKind::Model3D).id;
        assert!(set.activate(second).is_ok());
        assert_eq!(
            set.most_recent(ViewKind::Model3D).map(|recent| recent.id),
            Some(second)
        );
    }

    #[test]
    fn most_recent_is_the_last_active_of_that_kind_after_the_user_clicked_another_kind() {
        // Plot, F9 and the Standard Views must reach the 3D view the user last
        // touched even after they clicked back into the plan.
        let mut set = ViewSet::default();
        let plan = set.add(ViewKind::Plan).id;
        set.add(ViewKind::Model3D);
        let touched = set.add(ViewKind::Model3D).id;
        assert!(set.activate(touched).is_ok());
        assert!(set.activate(plan).is_ok());

        assert_eq!(
            set.most_recent(ViewKind::Model3D).map(|recent| recent.id),
            Some(touched),
            "not the first-opened one"
        );
        assert_eq!(
            set.most_recent(ViewKind::Plan).map(|recent| recent.id),
            Some(plan)
        );
    }

    #[test]
    fn most_recent_falls_back_to_the_first_opened_when_none_of_that_kind_was_ever_active() {
        let mut set = ViewSet::default();
        set.add(ViewKind::Plan); // active
        let first = set.add(ViewKind::Model3D).id;
        let second = set.add(ViewKind::Model3D).id;
        let third = set.add(ViewKind::Model3D).id;

        assert_eq!(
            set.most_recent(ViewKind::Model3D).map(|recent| recent.id),
            Some(first)
        );
        assert!(set.remove(first).is_ok());
        assert_eq!(
            set.most_recent(ViewKind::Model3D).map(|recent| recent.id),
            Some(second)
        );

        // And one activation outranks every never-activated view.
        assert!(set.activate(third).is_ok());
        assert!(set.activate(1).is_ok());
        assert_eq!(
            set.most_recent(ViewKind::Model3D).map(|recent| recent.id),
            Some(third)
        );
    }

    #[test]
    fn most_recent_is_null_when_no_view_of_that_kind_is_open() {
        let mut set = ViewSet::default();
        assert!(set.most_recent(ViewKind::Plan).is_none());
        let only = set.add(ViewKind::Model3D).id;
        assert!(set.most_recent(ViewKind::Section).is_none());
        assert!(set.remove(only).is_ok());
        assert!(set.most_recent(ViewKind::Model3D).is_none());
    }

    #[test]
    fn most_recent_follows_a_view_that_changed_kind() {
        let mut set = ViewSet::default();
        set.add(ViewKind::Plan);
        let changing = set.add(ViewKind::Model3D).id;
        assert!(set.set_kind(changing, ViewKind::Section).is_ok());
        assert!(set.most_recent(ViewKind::Model3D).is_none());
        assert_eq!(
            set.most_recent(ViewKind::Section).map(|recent| recent.id),
            Some(changing)
        );
    }

    // ---- listing ---------------------------------------------------------------------------------

    #[test]
    fn count_counts_the_open_views_of_one_kind() {
        let mut set = ViewSet::default();
        assert_eq!(set.count(ViewKind::Plan), 0);
        set.add(ViewKind::Plan);
        set.add(ViewKind::Model3D);
        let plan = set.add(ViewKind::Plan).id;
        assert_eq!(set.count(ViewKind::Plan), 2);
        assert_eq!(set.count(ViewKind::Model3D), 1);
        assert_eq!(set.count(ViewKind::Elevation), 0);
        assert!(set.set_kind(plan, ViewKind::Elevation).is_ok());
        assert_eq!(set.count(ViewKind::Plan), 1);
        assert_eq!(set.count(ViewKind::Elevation), 1);
        assert_eq!(set.size(), 3);
    }

    #[test]
    fn views_are_listed_in_the_order_they_were_opened() {
        let mut set = ViewSet::default();
        set.add(ViewKind::Plan);
        set.add(ViewKind::Model3D);
        assert!(set.restore(10, ViewKind::Section).is_ok());
        set.add(ViewKind::Plan);
        assert_eq!(ids_of(&set), vec![1, 2, 10, 11]);

        assert!(set.remove(2).is_ok());
        assert_eq!(ids_of(&set), vec![1, 10, 11]);
        assert!(set.activate(11).is_ok());
        assert_eq!(ids_of(&set), vec![1, 10, 11], "activation does not reorder");
    }

    #[test]
    fn a_views_state_stays_at_one_address_whatever_else_opens_or_closes() {
        // A widget holds its view's state for as long as it lives; the state
        // must not move under it. Enough views are opened to force the
        // container to grow several times over.
        let mut set = ViewSet::default();
        let plan: *const ViewState = {
            let added = set.add(ViewKind::Plan);
            assert!(added.layers.hide("survey"));
            added
        };
        let model: *const ViewState = set.add(ViewKind::Model3D);

        let mut others = Vec::new();
        for i in 0..64 {
            let kind = if i % 2 == 0 {
                ViewKind::Section
            } else {
                ViewKind::Elevation
            };
            others.push(set.add(kind).id);
        }
        for &id in others.iter().step_by(3) {
            assert!(set.remove(id).is_ok());
        }
        assert!(set.set_kind(2, ViewKind::Elevation).is_ok());

        assert!(std::ptr::eq(view(&set, 1), plan));
        assert!(std::ptr::eq(view(&set, 2), model));
        assert!(view(&set, 1).layers.hides_directly("survey"));
        assert_eq!(view(&set, 2).kind, ViewKind::Elevation);
    }

    // ---- linked views (view_link.rs) -------------------------------------------------------

    #[test]
    fn linked_plan_views_take_the_centre_and_scale_of_the_view_that_moved_and_keep_their_own_size()
    {
        let mut set = ViewSet::default();
        let a = framed_plan(&mut set, 300.0, 200.0, 10.0, 20.0, 4.0);
        let b = framed_plan(&mut set, 600.0, 400.0, 0.0, 0.0, 1.0);
        assert!(set.link(&[a, b], Some(a)).is_ok());

        // A zoomed 2x at its top-left pixel (0, 0). By hand: the anchor there
        // is (10 + (0 - 150) / 4, 20 - (0 - 100) / 4) = (-27.5, 45); at scale 8
        // the centre that keeps it at (0, 0) is (-27.5 + 150 / 8, 45 - 100 / 8)
        // = (-8.75, 32.5). Quarters and eighths: exact in binary, so compared
        // equal.
        view_mut(&mut set, a)
            .plan
            .zoom_at(Point2::new(0.0, 0.0), 2.0);
        expect_showing(&set, a, -8.75, 32.5, 8.0);
        set.note_moved(a);
        assert_eq!(set.follow(a), vec![b]);

        expect_showing(&set, b, -8.75, 32.5, 8.0);
        // Its own size: twice as wide, it shows twice as much about the same
        // centre.
        assert_eq!(view(&set, b).plan.width_pixels, 600.0);
        assert_eq!(view(&set, b).plan.height_pixels, 400.0);
    }

    #[test]
    fn a_joining_view_comes_to_the_view_the_user_moved_last() {
        // The Link button sends TO = link_leader_for(the view clicked). The
        // user zoomed the design view A and then linked the two in either
        // order: the as-built view B comes to A both times, and the zoom is
        // never lost.
        for design_first in [true, false] {
            let mut set = ViewSet::default();
            let a = framed_plan(&mut set, 300.0, 200.0, 100.0, 100.0, 3.0);
            let b = framed_plan(&mut set, 300.0, 200.0, 0.0, 0.0, 1.0);
            set.note_moved(a);
            let (first, second) = if design_first { (a, b) } else { (b, a) };

            let leader = set.link_leader_for(first);
            let waiting = set.link(&[first], Some(leader)).expect("linked");
            assert!(waiting.moved.is_empty(), "a link of one moves nothing");
            let leader = set.link_leader_for(second);
            let joined = set.link(&[second], Some(leader)).expect("linked");

            let how = if design_first {
                "design first"
            } else {
                "as-built first"
            };
            assert_eq!(joined.leader, a, "{how}");
            assert_eq!(joined.moved, vec![b]);
            expect_showing(&set, a, 100.0, 100.0, 3.0);
            expect_showing(&set, b, 100.0, 100.0, 3.0);
        }
    }

    #[test]
    fn two_views_nobody_moved_follow_the_first_listed() {
        let mut set = ViewSet::default();
        let one = framed_plan(&mut set, 300.0, 200.0, 1.0, 2.0, 3.0);
        let two = framed_plan(&mut set, 300.0, 200.0, 4.0, 5.0, 6.0);
        // No TO and no link yet: the first id listed leads, whatever its
        // number.
        let change = set.link(&[two, one], None).expect("linked");
        assert_eq!(change.leader, two);
        assert_eq!(change.linked, vec![one, two], "creation order");
        assert_eq!(change.moved, vec![one]);
        expect_showing(&set, one, 4.0, 5.0, 6.0);
        // Nobody moved either, so the Link button of a third view would name
        // the link's lowest id.
        let third = framed_plan(&mut set, 10.0, 10.0, 0.0, 0.0, 1.0);
        assert_eq!(set.link_leader_for(third), one);
    }

    #[test]
    fn link_to_names_the_view_the_others_come_to() {
        let mut set = ViewSet::default();
        let one = framed_plan(&mut set, 300.0, 200.0, 1.0, 2.0, 3.0);
        let two = framed_plan(&mut set, 300.0, 200.0, 4.0, 5.0, 6.0);
        let three = framed_plan(&mut set, 300.0, 200.0, 7.0, 8.0, 9.0);
        assert!(set.link(&[one, two], None).is_ok());
        expect_showing(&set, two, 1.0, 2.0, 3.0);

        // Three joins and leads: the members already linked come to it too.
        let change = set.link(&[three], Some(three)).expect("linked");
        assert_eq!(change.leader, three);
        assert_eq!(change.moved, vec![one, two]);
        expect_showing(&set, one, 7.0, 8.0, 9.0);
        expect_showing(&set, two, 7.0, 8.0, 9.0);

        // Without TO, a view joining a link comes to the link's lowest id.
        let four = framed_plan(&mut set, 300.0, 200.0, -1.0, -1.0, 1.0);
        view_mut(&mut set, one).plan.center = Point2::new(70.0, 80.0); // moved, not yet followed
        let joined = set.link(&[four], None).expect("linked");
        assert_eq!(joined.leader, one);
        expect_showing(&set, four, 70.0, 80.0, 9.0);
    }

    #[test]
    fn a_link_refuses_a_view_not_open_or_a_leader_outside_it_and_changes_nothing() {
        let mut set = ViewSet::default();
        let one = framed_plan(&mut set, 300.0, 200.0, 1.0, 2.0, 3.0);
        let two = framed_plan(&mut set, 300.0, 200.0, 4.0, 5.0, 6.0);
        let three = framed_plan(&mut set, 300.0, 200.0, 7.0, 8.0, 9.0);

        let unknown = set.link(&[one, 99], None).expect_err("99 is not open");
        assert_eq!(unknown.code, ErrorCode::NotFound);
        assert!(
            unknown.message.contains("no view 99 is open"),
            "{}",
            unknown.message
        );

        let outside = set
            .link(&[one, two], Some(three))
            .expect_err("three is neither");
        assert_eq!(outside.code, ErrorCode::InvalidArgument);
        assert!(
            outside.message.contains("view 3 is neither"),
            "{}",
            outside.message
        );

        let none = set.link(&[], None).expect_err("nothing to link");
        assert_eq!(none.code, ErrorCode::InvalidArgument);

        assert!(set.linked_views().is_empty());
        expect_showing(&set, one, 1.0, 2.0, 3.0);
        expect_showing(&set, two, 4.0, 5.0, 6.0);

        let unlink_unknown = set.unlink(&[42]).expect_err("42 is not open");
        assert_eq!(unlink_unknown.code, ErrorCode::NotFound);
    }

    #[test]
    fn a_link_of_one_waits_and_an_unlink_that_leaves_one_dissolves_it() {
        let mut set = ViewSet::default();
        let one = framed_plan(&mut set, 300.0, 200.0, 1.0, 2.0, 3.0);
        let two = framed_plan(&mut set, 300.0, 200.0, 4.0, 5.0, 6.0);
        let three = framed_plan(&mut set, 300.0, 200.0, 7.0, 8.0, 9.0);

        let waiting = set.link(&[one], None).expect("linked");
        assert_eq!(waiting.linked, vec![one]);
        assert!(waiting.moved.is_empty());
        // Unlinking a view that is not linked is no error, and leaves the
        // waiting link waiting.
        let nothing = set.unlink(&[three]).expect("no error");
        assert!(nothing.is_empty());
        assert_eq!(set.linked_views(), vec![one]);

        assert!(set.link(&[two], None).is_ok());
        expect_showing(&set, two, 1.0, 2.0, 3.0);

        // Two leaves; one is left alone, and a link of one it did not ask for
        // is no link: both have left.
        let left = set.unlink(&[two]).expect("unlinked");
        assert_eq!(left, vec![one, two]);
        assert!(set.linked_views().is_empty());
        assert!(!view(&set, one).linked);
    }

    #[test]
    fn a3_d_or_section_view_cannot_join_and_the_refusal_names_its_kind() {
        let mut set = ViewSet::default();
        let plan = framed_plan(&mut set, 300.0, 200.0, 1.0, 2.0, 3.0);
        let model = set.add(ViewKind::Model3D).id;
        let section = set.add(ViewKind::Section).id;

        let refused_3d = set.link(&[plan, model], None).expect_err("a 3D view");
        assert_eq!(refused_3d.code, ErrorCode::InvalidArgument);
        assert_eq!(refused_3d.message, "only plan views link: view 2 is 3D");

        let refused_section = set.link(&[section], None).expect_err("a section");
        assert_eq!(
            refused_section.message,
            "only plan views link: view 3 is Section"
        );
        // Refused whole: the plan view listed with the 3D one did not join.
        assert!(set.linked_views().is_empty());
    }

    #[test]
    fn changing_a_linked_views_kind_takes_it_out_of_the_link() {
        let mut set = ViewSet::default();
        let one = framed_plan(&mut set, 300.0, 200.0, 1.0, 2.0, 3.0);
        let two = framed_plan(&mut set, 300.0, 200.0, 4.0, 5.0, 6.0);
        let three = framed_plan(&mut set, 300.0, 200.0, 7.0, 8.0, 9.0);
        assert!(set.link(&[one, two, three], None).is_ok());

        assert!(set.set_kind(two, ViewKind::Model3D).is_ok());
        assert!(!view(&set, two).linked);
        assert_eq!(set.linked_views(), vec![one, three]);

        // Back to plan it stays out: joining is always asked for.
        assert!(set.set_kind(two, ViewKind::Plan).is_ok());
        assert!(!view(&set, two).linked);

        // This change leaves one member, which is no link.
        assert!(set.set_kind(three, ViewKind::Section).is_ok());
        assert!(set.linked_views().is_empty());
    }

    #[test]
    fn closing_a_linked_view_leaves_the_others_linked() {
        let mut set = ViewSet::default();
        let one = framed_plan(&mut set, 300.0, 200.0, 1.0, 2.0, 3.0);
        let two = framed_plan(&mut set, 300.0, 200.0, 4.0, 5.0, 6.0);
        let three = framed_plan(&mut set, 300.0, 200.0, 7.0, 8.0, 9.0);
        assert!(set.link(&[one, two, three], None).is_ok());

        assert!(set.remove(two).is_ok());
        assert_eq!(set.linked_views(), vec![one, three]);

        // A state handed over by take() is out of the link as well, and so is
        // the view it leaves alone.
        let taken = set.take(three).expect("taken");
        assert!(!taken.linked);
        assert!(set.linked_views().is_empty());
        assert!(!view(&set, one).linked);
    }

    #[test]
    fn follow_returns_only_the_linked_views_it_changed() {
        let mut set = ViewSet::default();
        let one = framed_plan(&mut set, 300.0, 200.0, 1.0, 2.0, 3.0);
        let two = framed_plan(&mut set, 300.0, 200.0, 4.0, 5.0, 6.0);
        let three = framed_plan(&mut set, 300.0, 200.0, 7.0, 8.0, 9.0);
        assert!(set.link(&[one, three], None).is_ok());

        {
            let leader = view_mut(&mut set, one);
            leader.plan.center = Point2::new(-3.0, -4.0);
            leader.plan.scale = 0.5;
        }
        assert_eq!(set.follow(one), vec![three]);
        expect_showing(&set, three, -3.0, -4.0, 0.5);
        expect_showing(&set, two, 4.0, 5.0, 6.0); // not linked: untouched

        // A view outside the link moves nobody, and neither does one not open.
        assert!(set.follow(two).is_empty());
        assert!(set.follow(99).is_empty());
        expect_showing(&set, one, -3.0, -4.0, 0.5);
    }

    #[test]
    fn unlinking_moves_nothing() {
        let mut set = ViewSet::default();
        let one = framed_plan(&mut set, 300.0, 200.0, 1.0, 2.0, 3.0);
        let two = framed_plan(&mut set, 300.0, 200.0, 4.0, 5.0, 6.0);
        assert!(set.link(&[one, two], None).is_ok());
        expect_showing(&set, two, 1.0, 2.0, 3.0);

        view_mut(&mut set, one).plan.center = Point2::new(50.0, 60.0);
        assert!(set.unlink(&[one]).is_ok());
        expect_showing(&set, two, 1.0, 2.0, 3.0);
        expect_showing(&set, one, 50.0, 60.0, 3.0);
        assert!(set.unlink_all().is_empty(), "nothing was left to unlink");
    }

    #[test]
    fn an_unframed_leader_moves_nobody_until_it_is_framed() {
        let mut set = ViewSet::default();
        let fresh = set.add(ViewKind::Plan).id; // opened, never painted
        let shown = framed_plan(&mut set, 300.0, 200.0, 4.0, 5.0, 6.0);

        let change = set.link(&[fresh, shown], None).expect("linked");
        assert_eq!(change.leader, fresh);
        assert!(change.moved.is_empty());
        expect_showing(&set, shown, 4.0, 5.0, 6.0);

        // Its first paint frames it; the widget reports that as a move.
        {
            let painted = view_mut(&mut set, fresh);
            painted.plan.center = Point2::new(9.0, 9.0);
            painted.plan.scale = 7.0;
            painted.plan_framed = true;
        }
        assert_eq!(set.follow(fresh), vec![shown]);
        expect_showing(&set, shown, 9.0, 9.0, 7.0);
    }

    #[test]
    fn the_view_moved_most_recently_leads_and_note_moved_ignores_a_view_not_open() {
        let mut set = ViewSet::default();
        let one = framed_plan(&mut set, 300.0, 200.0, 1.0, 2.0, 3.0);
        let two = framed_plan(&mut set, 300.0, 200.0, 4.0, 5.0, 6.0);
        assert_eq!(
            set.link_leader_for(two),
            two,
            "no link and no move: the view clicked"
        );
        set.note_moved(one);
        set.note_moved(two);
        set.note_moved(99);
        // No link: only the view asked about counts.
        assert_eq!(set.link_leader_for(one), one);
        assert!(set.link(&[two], None).is_ok());
        assert_eq!(
            set.link_leader_for(one),
            two,
            "two, the member, was moved after one"
        );
        set.note_moved(one);
        assert_eq!(set.link_leader_for(one), one);
        assert!(view(&set, one).last_moved > view(&set, two).last_moved);
    }
}
