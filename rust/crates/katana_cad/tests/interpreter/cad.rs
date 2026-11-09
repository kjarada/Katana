//! tests/cad/test_cad.cpp: the view transform, selection and picking,
//! snapping, the Document, and the command interpreter.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use katana_cad::selection::{pick_entity, pick_in_box};
use katana_cad::snapping::{ALL_SNAP_MODES, DEFAULT_SNAP_MODES, snap};
use katana_cad::view_transform::{DEFAULT_FIT_MARGIN, grid_spacing};
use katana_cad::{
    BoxSelectionMode, CommandInterpreter, Document, ImportPlacement, ImportPlacementMode,
    ListenerHandle, SelectionFilter, SnapMode, SnapModes, SnapRequest, ViewTransform,
};
use katana_commands::{
    EntityAttributes, create_arc, create_circle, create_layer, create_line, create_point,
    delete_entities, set_entity_visible, update_layer,
};
use katana_core::ErrorCode;
use katana_entity::{
    DimensionGeometry, Entity, EntityId, EntityType, Geometry, Layer, LineStyle, Model,
    PropertyValue, Stroke, StrokeOp, StyleLibrary, SurveyMap, TextGeometry,
};
use katana_geometry::{Arc2, Box2, Circle2, Point2, Polyline2, Segment2, Vec2};
use katana_math::HALF_PI;
use katana_math::vec2::nearly_equal;

use crate::property::Random;
use crate::{ScratchDirectory, Session, double_eq, must_create, near};

fn layer(name: &str) -> Layer {
    Layer {
        name: String::from(name),
        ..Layer::default()
    }
}

fn on_layer(name: &str) -> EntityAttributes {
    EntityAttributes {
        layer: String::from(name),
        ..EntityAttributes::default()
    }
}

fn nearly(a: Point2, b: Point2) -> bool {
    nearly_equal(a, b, katana_math::numerics::tolerance::RELATIVE)
}

fn segment(entity: &Entity) -> Segment2 {
    match &entity.geometry {
        Geometry::Line(segment) => *segment,
        other => panic!("not a line: {other:?}"),
    }
}

fn circle(entity: &Entity) -> Circle2 {
    match &entity.geometry {
        Geometry::Circle(circle) => *circle,
        other => panic!("not a circle: {other:?}"),
    }
}

fn arc(entity: &Entity) -> Arc2 {
    match &entity.geometry {
        Geometry::Arc(arc) => *arc,
        other => panic!("not an arc: {other:?}"),
    }
}

fn polyline(entity: &Entity) -> Polyline2 {
    match &entity.geometry {
        Geometry::Polyline(polyline) => polyline.clone(),
        other => panic!("not a polyline: {other:?}"),
    }
}

fn text(entity: &Entity) -> TextGeometry {
    match &entity.geometry {
        Geometry::Text(text) => text.clone(),
        other => panic!("not a text: {other:?}"),
    }
}

fn dimension(entity: &Entity) -> DimensionGeometry {
    match &entity.geometry {
        Geometry::Dimension(dimension) => dimension.clone(),
        other => panic!("not a dimension: {other:?}"),
    }
}

fn real_of(value: Option<&PropertyValue>) -> f64 {
    match value {
        Some(PropertyValue::Real(real)) => *real,
        other => panic!("not a real: {other:?}"),
    }
}

fn integer_of(value: Option<&PropertyValue>) -> i64 {
    match value {
        Some(PropertyValue::Integer(integer)) => *integer,
        other => panic!("not an integer: {other:?}"),
    }
}

fn text_of(value: Option<&PropertyValue>) -> String {
    match value {
        Some(PropertyValue::Text(text)) => text.clone(),
        other => panic!("not a text: {other:?}"),
    }
}

// ---- ViewTransform -------------------------------------------------------------

mod cad_view_transform {
    use super::*;

    #[test]
    fn maps_model_space_to_pixels_with_y_flipped() {
        let mut view = ViewTransform::default();
        view.resize(800.0, 600.0);
        view.center = Point2::new(100.0, 50.0);
        view.scale = 2.0;

        assert_eq!(
            view.world_to_screen(Point2::new(100.0, 50.0)),
            Point2::new(400.0, 300.0)
        ); // centre
        assert_eq!(
            view.world_to_screen(Point2::new(110.0, 60.0)),
            Point2::new(420.0, 280.0)
        ); // +y is up on screen
        assert_eq!(
            view.screen_to_world(Point2::new(420.0, 280.0)),
            Point2::new(110.0, 60.0)
        );
        double_eq(view.pixels_to_world(10.0), 5.0);

        let visible = view.visible_world_bounds();
        assert_eq!(visible.min, Point2::new(-100.0, -100.0));
        assert_eq!(visible.max, Point2::new(300.0, 200.0));
    }

    #[test]
    fn panning_follows_the_cursor() {
        let mut view = ViewTransform::default();
        view.resize(800.0, 600.0);
        view.scale = 4.0;
        let grabbed = view.screen_to_world(Point2::new(200.0, 200.0));
        view.pan_by_pixels(40.0, -20.0); // drag right and up
        assert!(nearly(
            view.screen_to_world(Point2::new(240.0, 180.0)),
            grabbed
        ));
    }

    #[test]
    fn zoom_keeps_the_point_under_the_cursor_fixed() {
        let mut random = Random::default();
        let mut view = ViewTransform::default();
        view.resize(1280.0, 720.0);
        view.center = Point2::new(500_000.0, 5_000_000.0); // survey magnitudes
        view.scale = 3.0;
        // Model coordinates near 5e6 are only representable to one ulp
        // (~9.3e-10), which is ulp * scale pixels on screen. A few such
        // roundings are the best any implementation can do, so the bound
        // scales with the zoom level.
        let ulp = f64::from_bits(5_000_000.0_f64.to_bits() + 1) - 5_000_000.0;
        for _ in 0..200 {
            let cursor = Point2::new(random.real(0.0, 1280.0), random.real(0.0, 720.0));
            let anchor = view.screen_to_world(cursor);
            view.zoom_at(cursor, random.real(0.5, 2.0));
            let after = view.world_to_screen(anchor);
            let bound = 1e-9 + 8.0 * ulp * view.scale;
            near(after.x, cursor.x, bound);
            near(after.y, cursor.y, bound);
        }
    }

    #[test]
    fn zoom_is_clamped_and_rejects_nonsense_factors() {
        let mut view = ViewTransform::default();
        view.resize(800.0, 600.0);
        for _ in 0..200 {
            view.zoom_at(Point2::new(400.0, 300.0), 10.0);
        }
        double_eq(view.scale, ViewTransform::MAXIMUM_SCALE);
        for _ in 0..400 {
            view.zoom_at(Point2::new(400.0, 300.0), 0.1);
        }
        double_eq(view.scale, ViewTransform::MINIMUM_SCALE);

        let before = view.scale;
        view.zoom_at(Point2::new(0.0, 0.0), 0.0);
        view.zoom_at(Point2::new(0.0, 0.0), -3.0);
        view.zoom_at(Point2::new(0.0, 0.0), f64::NAN);
        double_eq(view.scale, before);
    }

    #[test]
    fn fit_shows_the_whole_drawing() {
        let mut view = ViewTransform::default();
        view.resize(1000.0, 500.0);
        let square = Box2::new(Point2::new(0.0, 0.0), Point2::new(100.0, 100.0));
        view.fit(&square, 0.0);
        assert_eq!(view.center, Point2::new(50.0, 50.0));
        double_eq(view.scale, 5.0); // limited by the viewport height
        assert!(view.visible_world_bounds().contains_box(square));

        view.fit(&square, 0.1);
        double_eq(view.scale, 4.0); // 10% margin on each side

        view.fit(&Box2::default(), 0.05); // empty drawing
        assert_eq!(view.center, Point2::new(0.0, 0.0));
        double_eq(view.scale, 1.0);

        view.scale = 7.0;
        let mut single = Box2::default();
        single.expand(Point2::new(3.0, 4.0));
        view.fit(&single, DEFAULT_FIT_MARGIN); // a lone point: centre on it, keep the zoom
        assert_eq!(view.center, Point2::new(3.0, 4.0));
        double_eq(view.scale, 7.0);

        // A box that HAS extent but is far too small to fill the window at
        // maximum zoom. Fitting it clamps to MAXIMUM_SCALE; it must not be
        // mistaken for the lone point above and leave the zoom alone, or Zoom
        // Extents on a tiny drawing would leave it invisible.
        view.scale = 7.0;
        let tiny = Box2::new(Point2::new(10.0, 10.0), Point2::new(10.000001, 10.000001));
        view.fit(&tiny, 0.0);
        double_eq(view.scale, ViewTransform::MAXIMUM_SCALE);
        assert!(view.visible_world_bounds().contains_box(tiny));
    }

    #[test]
    fn grid_spacing_follows_a125_sequence() {
        double_eq(grid_spacing(1.0, 12.0), 20.0); // needs >= 12 units
        double_eq(grid_spacing(12.0, 12.0), 1.0); // needs >= 1 unit
        double_eq(grid_spacing(10.0, 12.0), 2.0); // needs >= 1.2
        double_eq(grid_spacing(4.0, 12.0), 5.0); // needs >= 3
        near(grid_spacing(1000.0, 12.0), 0.02, 1e-15); // needs >= 0.012
        double_eq(grid_spacing(0.0, 12.0), 1.0); // defensive default
        let mut scale = 1e-4;
        while scale < 1e4 {
            assert!(
                grid_spacing(scale, 12.0) * scale >= 12.0 * (1.0 - 1e-12),
                "{scale}"
            );
            assert!(
                grid_spacing(scale, 12.0) * scale <= 30.0 * (1.0 + 1e-12),
                "{scale}"
            );
            scale *= 1.7;
        }
    }
}

// ---- selection & picking --------------------------------------------------------

mod cad_selection {
    use super::*;

    #[test]
    fn set_operations_and_pruning() {
        let mut document = Document::default();
        let a = must_create(
            &mut document,
            create_point(Point2::new(0.0, 0.0), EntityAttributes::default()),
        );
        let b = must_create(
            &mut document,
            create_point(Point2::new(1.0, 1.0), EntityAttributes::default()),
        );

        let selection = document.selection_mut();
        selection.set(vec![b, a, b]);
        assert_eq!(selection.ids(), vec![a, b]); // ordered, de-duplicated
        selection.toggle(a);
        assert!(!selection.contains(a));
        selection.toggle(a);
        assert!(selection.contains(a));

        assert!(document.execute(delete_entities(vec![a])).is_ok());
        assert_eq!(document.selection().ids(), vec![b]); // deleted entities leave the selection
        assert!(document.undo().is_ok());
        assert_eq!(document.selection().ids(), vec![b]); // and do not sneak back in
    }
}

mod cad_picking {
    use super::*;

    #[test]
    fn picks_the_nearest_entity_within_tolerance() {
        let mut document = Document::default();
        let line = must_create(
            &mut document,
            create_line(
                Point2::new(0.0, 0.0),
                Point2::new(10.0, 0.0),
                EntityAttributes::default(),
            ),
        );
        let circle = must_create(
            &mut document,
            create_circle(Point2::new(5.0, 5.0), 2.0, EntityAttributes::default()),
        );
        let model = document.model();
        let all = SelectionFilter::default();

        assert_eq!(
            pick_entity(model, Point2::new(5.0, 0.2), 0.5, &all, None),
            Some(line)
        );
        assert_eq!(
            pick_entity(model, Point2::new(5.0, 2.9), 0.5, &all, None),
            Some(circle)
        ); // near the circumference
        assert!(pick_entity(model, Point2::new(5.0, 5.0), 0.5, &all, None).is_none()); // the inside is not the curve
        assert!(pick_entity(model, Point2::new(50.0, 50.0), 0.5, &all, None).is_none());

        let mut circles_only = SelectionFilter::default();
        circles_only.types.insert(EntityType::Circle);
        assert!(pick_entity(model, Point2::new(5.0, 0.2), 0.5, &circles_only, None).is_none());
    }

    #[test]
    fn coincident_entities_pick_the_one_drawn_last() {
        let mut document = Document::default();
        must_create(
            &mut document,
            create_line(
                Point2::new(0.0, 0.0),
                Point2::new(10.0, 0.0),
                EntityAttributes::default(),
            ),
        );
        let on_top = must_create(
            &mut document,
            create_line(
                Point2::new(0.0, 0.0),
                Point2::new(10.0, 0.0),
                EntityAttributes::default(),
            ),
        );
        assert_eq!(
            pick_entity(
                document.model(),
                Point2::new(5.0, 0.0),
                0.5,
                &SelectionFilter::default(),
                None
            ),
            Some(on_top)
        );
    }

    #[test]
    fn hidden_and_locked_entities_cannot_be_picked() {
        let mut document = Document::default();
        assert!(document.execute(create_layer(layer("Hidden"))).is_ok());
        assert!(document.execute(create_layer(layer("Locked"))).is_ok());
        must_create(
            &mut document,
            create_line(
                Point2::new(0.0, 0.0),
                Point2::new(10.0, 0.0),
                on_layer("Hidden"),
            ),
        );
        must_create(
            &mut document,
            create_line(
                Point2::new(0.0, 5.0),
                Point2::new(10.0, 5.0),
                on_layer("Locked"),
            ),
        );
        let invisible = must_create(
            &mut document,
            create_line(
                Point2::new(0.0, 9.0),
                Point2::new(10.0, 9.0),
                EntityAttributes::default(),
            ),
        );

        let mut hidden = document.model().layers.find("Hidden").cloned().unwrap();
        hidden.visible = false;
        let mut locked = document.model().layers.find("Locked").cloned().unwrap();
        locked.locked = true;
        assert!(document.execute(update_layer(hidden)).is_ok());
        assert!(document.execute(update_layer(locked)).is_ok());
        assert!(
            document
                .execute(set_entity_visible(vec![invisible], false))
                .is_ok()
        );

        let model = document.model();
        let all = SelectionFilter::default();
        assert!(pick_entity(model, Point2::new(5.0, 0.0), 0.5, &all, None).is_none());
        assert!(pick_entity(model, Point2::new(5.0, 5.0), 0.5, &all, None).is_none());
        assert!(pick_entity(model, Point2::new(5.0, 9.0), 0.5, &all, None).is_none());
        assert!(
            pick_in_box(
                model,
                Box2::new(Point2::new(-1.0, -1.0), Point2::new(11.0, 11.0)),
                BoxSelectionMode::Crossing,
                &all,
                None
            )
            .is_empty()
        );
    }

    #[test]
    fn window_needs_full_containment_crossing_only_contact() {
        let mut document = Document::default();
        let inside = must_create(
            &mut document,
            create_line(
                Point2::new(1.0, 1.0),
                Point2::new(4.0, 4.0),
                EntityAttributes::default(),
            ),
        );
        let crossing = must_create(
            &mut document,
            create_line(
                Point2::new(3.0, 3.0),
                Point2::new(20.0, 3.0),
                EntityAttributes::default(),
            ),
        );
        let _big_circle = must_create(
            &mut document,
            create_circle(Point2::new(2.5, 2.5), 50.0, EntityAttributes::default()),
        );
        let cuts_corner = must_create(
            &mut document,
            create_circle(Point2::new(0.0, 0.0), 1.0, EntityAttributes::default()),
        );
        must_create(
            &mut document,
            create_line(
                Point2::new(30.0, 30.0),
                Point2::new(40.0, 40.0),
                EntityAttributes::default(),
            ),
        ); // far away
        let model = document.model();
        let all = SelectionFilter::default();
        let bounds = Box2::new(Point2::new(0.0, 0.0), Point2::new(5.0, 5.0));

        assert_eq!(
            pick_in_box(model, bounds, BoxSelectionMode::Window, &all, None),
            vec![inside]
        );
        // The big circle's bounding box overlaps the selection box, but its
        // curve never comes near it: crossing selection must not grab it.
        assert_eq!(
            pick_in_box(model, bounds, BoxSelectionMode::Crossing, &all, None),
            vec![inside, crossing, cuts_corner]
        );
        assert!(
            pick_in_box(
                model,
                Box2::default(),
                BoxSelectionMode::Crossing,
                &all,
                None
            )
            .is_empty()
        );
    }
}

// ---- snapping ---------------------------------------------------------------------

fn request(cursor: Point2, modes: SnapModes, aperture: f64) -> SnapRequest<'static> {
    SnapRequest {
        cursor,
        modes,
        aperture,
        ..SnapRequest::default()
    }
}

mod cad_snapping {
    use super::*;

    #[test]
    fn endpoint_midpoint_center_intersection() {
        let mut document = Document::default();
        must_create(
            &mut document,
            create_line(
                Point2::new(0.0, 0.0),
                Point2::new(10.0, 0.0),
                EntityAttributes::default(),
            ),
        );
        must_create(
            &mut document,
            create_line(
                Point2::new(4.0, -5.0),
                Point2::new(4.0, 5.0),
                EntityAttributes::default(),
            ),
        );
        must_create(
            &mut document,
            create_circle(Point2::new(20.0, 20.0), 3.0, EntityAttributes::default()),
        );
        let model = document.model();

        let endpoint = snap(
            model,
            &request(Point2::new(9.8, 0.2), ALL_SNAP_MODES, 0.5),
            None,
        )
        .expect("an endpoint");
        assert_eq!(endpoint.mode, SnapMode::Endpoint);
        assert_eq!(endpoint.point, Point2::new(10.0, 0.0));

        let midpoint = snap(
            model,
            &request(Point2::new(5.2, 0.1), DEFAULT_SNAP_MODES, 0.5),
            None,
        )
        .expect("a midpoint");
        assert_eq!(midpoint.mode, SnapMode::Midpoint);
        assert_eq!(midpoint.point, Point2::new(5.0, 0.0));

        let intersection = snap(
            model,
            &request(Point2::new(4.1, 0.1), DEFAULT_SNAP_MODES, 0.5),
            None,
        )
        .expect("an intersection");
        assert_eq!(intersection.mode, SnapMode::Intersection);
        assert!(nearly(intersection.point, Point2::new(4.0, 0.0)));

        // Hovering the circumference offers the centre.
        let center = snap(
            model,
            &request(Point2::new(23.1, 20.0), DEFAULT_SNAP_MODES, 0.5),
            None,
        )
        .expect("a centre");
        assert_eq!(center.mode, SnapMode::Center);
        assert_eq!(center.point, Point2::new(20.0, 20.0));

        assert!(
            snap(
                model,
                &request(Point2::new(50.0, 50.0), DEFAULT_SNAP_MODES, 0.5),
                None
            )
            .is_none()
        );
    }

    #[test]
    fn exact_snaps_beat_nearest_and_nearest_beats_grid() {
        let mut document = Document::default();
        must_create(
            &mut document,
            create_line(
                Point2::new(0.0, 0.0),
                Point2::new(10.0, 0.0),
                EntityAttributes::default(),
            ),
        );
        let model = document.model();

        let mut r = request(Point2::new(2.3, 0.2), ALL_SNAP_MODES, 0.5);
        r.grid_spacing = 1.0;
        let nearest = snap(model, &r, None).expect("a nearest point"); // no endpoint or midpoint within 0.5
        assert_eq!(nearest.mode, SnapMode::Nearest);
        assert!(nearly(nearest.point, Point2::new(2.3, 0.0)));

        r.cursor = Point2::new(0.3, 0.2); // now the endpoint is in range too
        assert_eq!(
            snap(model, &r, None).map(|hit| hit.mode),
            Some(SnapMode::Endpoint)
        );

        r.cursor = Point2::new(2.3, 7.4); // nothing nearby: fall through to the grid
        let grid = snap(model, &r, None).expect("a grid point");
        assert_eq!(grid.mode, SnapMode::Grid);
        assert_eq!(grid.point, Point2::new(2.0, 7.0));

        r.modes = SnapMode::Endpoint | SnapMode::Midpoint; // grid not enabled
        assert!(snap(model, &r, None).is_none());
    }

    #[test]
    fn perpendicular_and_tangent_need_a_start_point() {
        let mut document = Document::default();
        must_create(
            &mut document,
            create_line(
                Point2::new(0.0, 0.0),
                Point2::new(10.0, 0.0),
                EntityAttributes::default(),
            ),
        );
        must_create(
            &mut document,
            create_circle(Point2::new(20.0, 0.0), 5.0, EntityAttributes::default()),
        );
        let model = document.model();

        let mut r = request(
            Point2::new(3.1, 0.1),
            SnapMode::Perpendicular | SnapMode::Tangent,
            0.5,
        );
        assert!(snap(model, &r, None).is_none()); // no `from`: these modes cannot apply

        r.from = Some(Point2::new(3.0, 8.0));
        let foot = snap(model, &r, None).expect("a perpendicular foot");
        assert_eq!(foot.mode, SnapMode::Perpendicular);
        assert!(nearly(foot.point, Point2::new(3.0, 0.0)));

        // From (20, 13): tangent points satisfy |CT| = 5 and CT . FT = 0.
        r.from = Some(Point2::new(20.0, 13.0));
        let tangent_y = 25.0 / 13.0; // r^2 / d above the centre
        let tangent_x = (25.0_f64 - tangent_y * tangent_y).sqrt();
        r.cursor = Point2::new(20.0 + tangent_x + 0.1, tangent_y);
        let tangent = snap(model, &r, None).expect("a tangent point");
        assert_eq!(tangent.mode, SnapMode::Tangent);
        assert!(nearly_equal(
            tangent.point,
            Point2::new(20.0 + tangent_x, tangent_y),
            1e-9
        ));
        let radius: Vec2 = tangent.point - Point2::new(20.0, 0.0);
        let tangent_line: Vec2 = tangent.point - Point2::new(20.0, 13.0);
        near(radius.dot(tangent_line), 0.0, 1e-9);

        r.from = Some(Point2::new(21.0, 0.0)); // inside the circle: no tangent exists
        r.modes = SnapMode::Tangent as SnapModes;
        assert!(snap(model, &r, None).is_none());
    }

    #[test]
    fn arc_snaps_respect_the_sweep_and_find_the_off_extent_centre() {
        let mut document = Document::default();
        // A shallow arc high on a big circle: its centre is far outside its
        // own extents.
        must_create(
            &mut document,
            create_arc(
                Arc2 {
                    center: Point2::new(0.0, 0.0),
                    radius: 100.0,
                    start_angle: HALF_PI - 0.05,
                    sweep: 0.1,
                },
                EntityAttributes::default(),
            ),
        );
        let model = document.model();

        let center = snap(
            model,
            &request(Point2::new(0.1, 0.1), DEFAULT_SNAP_MODES, 0.5),
            None,
        )
        .expect("the centre");
        assert_eq!(center.mode, SnapMode::Center);

        let mid = snap(
            model,
            &request(Point2::new(0.1, 100.1), DEFAULT_SNAP_MODES, 0.5),
            None,
        )
        .expect("the midpoint");
        assert_eq!(mid.mode, SnapMode::Midpoint);
        assert!(nearly_equal(mid.point, Point2::new(0.0, 100.0), 1e-9));

        // The perpendicular foot on the far side of the circle is not on the
        // arc.
        let mut r = request(
            Point2::new(0.0, -100.0),
            SnapMode::Perpendicular as SnapModes,
            0.5,
        );
        r.from = Some(Point2::new(0.0, 50.0));
        assert!(snap(model, &r, None).is_none());
    }

    // Snapping culls a polyline's segments against the cursor's reach before
    // the pairwise intersection loop. That loop is quadratic in the candidate
    // count, so a 3000-vertex surveyed string used to cost 247 ms per mouse
    // move in Release - against PLAN.MD section 32's 16 ms budget - purely
    // because the entity-level filter admits the whole polyline whenever any
    // part of it is near the cursor.
    //
    // The cull must not change ANY result, which is what these assert. It is
    // exact because every candidate is accepted only within `aperture` of the
    // cursor and lies on the curve that produced it, so a segment whose
    // bounding box misses the reach box cannot carry an acceptable point.
    #[test]
    fn culling_long_polylines_does_not_change_what_is_found() {
        let mut model = Model::default();
        // A long polyline crossed by a short one, intersecting at (50, 0).
        let path = Polyline2::new(
            (0..=200)
                .map(|i| Point2::new(f64::from(i) * 0.5, 0.0))
                .collect(),
            false,
        );
        assert!(
            model
                .entities
                .add(Entity {
                    geometry: Geometry::Polyline(path),
                    ..Entity::default()
                })
                .is_ok()
        );
        assert!(
            model
                .entities
                .add(Entity {
                    geometry: Geometry::Line(Segment2::new(
                        Point2::new(50.0, -5.0),
                        Point2::new(50.0, 5.0)
                    )),
                    ..Entity::default()
                })
                .is_ok()
        );

        // The intersection is still found, even though it sits in the middle
        // of a polyline whose far ends are nowhere near the cursor.
        let mut r = request(
            Point2::new(50.2, 0.2),
            SnapMode::Intersection as SnapModes,
            1.0,
        );
        let hit = snap(&model, &r, None).expect("the cull removed a real intersection");
        assert_eq!(hit.mode, SnapMode::Intersection);
        assert!(nearly(hit.point, Point2::new(50.0, 0.0)));

        // Nearest reads the same culled list, so it must still land on the
        // curve.
        r.modes = SnapMode::Nearest as SnapModes;
        r.cursor = Point2::new(20.3, 0.4);
        let nearest = snap(&model, &r, None).expect("a nearest point");
        assert_eq!(nearest.mode, SnapMode::Nearest);
        near(nearest.point.y, 0.0, 1e-12);
        near(nearest.point.x, 20.3, 1e-12);

        // And a cursor genuinely far from everything still snaps to nothing.
        r.cursor = Point2::new(50.0, 40.0);
        r.modes = DEFAULT_SNAP_MODES;
        assert!(snap(&model, &r, None).is_none());
    }
}

// ---- Document -----------------------------------------------------------------------

mod cad_document {
    use super::*;

    #[test]
    fn current_layer_drives_new_entities_and_survives_undo() {
        let mut document = Document::default();
        assert!(document.execute(create_layer(layer("Survey"))).is_ok());
        assert!(document.set_current_layer("Survey").is_ok());
        assert_eq!(document.current_attributes().layer, "Survey");
        assert_eq!(
            document
                .set_current_layer("Missing")
                .expect_err("no such layer")
                .code,
            ErrorCode::NotFound
        );

        assert!(document.undo().is_ok()); // the layer no longer exists
        assert_eq!(document.current_layer(), "0");
    }

    #[test]
    fn notifies_listeners_of_model_selection_and_layer_changes() {
        let mut document = Document::default();
        let notifications = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&notifications);
        let _listener = document.add_listener(move || {
            counted.fetch_add(1, Ordering::SeqCst);
        });

        must_create(
            &mut document,
            create_point(Point2::new(0.0, 0.0), EntityAttributes::default()),
        );
        assert_eq!(notifications.load(Ordering::SeqCst), 1);
        document.selection_mut().set(vec![1]);
        document.notify_selection_changed();
        assert_eq!(notifications.load(Ordering::SeqCst), 2);
        assert!(
            document
                .execute(create_circle(
                    Point2::new(0.0, 0.0),
                    -1.0,
                    EntityAttributes::default()
                ))
                .is_err()
        );
        assert_eq!(notifications.load(Ordering::SeqCst), 2); // failed commands change nothing
        assert!(document.undo().is_ok());
        assert_eq!(notifications.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn a_listener_whose_handle_has_died_is_never_called_again() {
        // The bug this guards: a viewport widget registered [this]{
        // update(); }, was replaced by a layout change, and the next command
        // called into the freed widget. With the handle gone, the
        // registration is gone.
        let mut document = Document::default();
        let calls = Arc::new(AtomicUsize::new(0));
        {
            let counted = Arc::clone(&calls);
            let _listener = document.add_listener(move || {
                counted.fetch_add(1, Ordering::SeqCst);
            });
            must_create(
                &mut document,
                create_point(Point2::new(0.0, 0.0), EntityAttributes::default()),
            );
            assert_eq!(calls.load(Ordering::SeqCst), 1);
        }
        must_create(
            &mut document,
            create_point(Point2::new(1.0, 0.0), EntityAttributes::default()),
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        // Moved, the registration follows the handle.
        let mut kept;
        {
            let counted = Arc::clone(&calls);
            let listener = document.add_listener(move || {
                counted.fetch_add(1, Ordering::SeqCst);
            });
            kept = listener;
        }
        must_create(
            &mut document,
            create_point(Point2::new(2.0, 0.0), EntityAttributes::default()),
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert!(kept.active());
        kept.reset();
        assert!(!kept.active());
        must_create(
            &mut document,
            create_point(Point2::new(3.0, 0.0), EntityAttributes::default()),
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn a_handle_that_outlives_its_document_does_nothing_when_it_dies() {
        // Qt deletes child widgets after the window's own members - the
        // Document among them - are gone, so the widget's handle dies last.
        let mut orphan;
        {
            let document = Document::default();
            orphan = document.add_listener(|| {});
            assert!(orphan.active());
        }
        assert!(!orphan.active());
        orphan.reset(); // must not touch the dead registry
    }

    #[test]
    fn a_listener_may_end_another_registration_while_notifications_run() {
        let mut document = Document::default();
        let second = Arc::new(AtomicUsize::new(0));
        let second_handle: Arc<Mutex<Option<ListenerHandle>>> = Arc::new(Mutex::new(None));
        let ended = Arc::clone(&second_handle);
        let _first = document.add_listener(move || {
            // Taken out before it is dropped: its drop ends the registration.
            let handle = ended.lock().unwrap().take();
            drop(handle);
        });
        let counted = Arc::clone(&second);
        *second_handle.lock().unwrap() = Some(document.add_listener(move || {
            counted.fetch_add(1, Ordering::SeqCst);
        }));
        // The first listener removes the second before it runs; the loop must
        // survive the list changing under it, and honour the removal.
        must_create(
            &mut document,
            create_point(Point2::new(0.0, 0.0), EntityAttributes::default()),
        );
        assert_eq!(second.load(Ordering::SeqCst), 0);
        must_create(
            &mut document,
            create_point(Point2::new(1.0, 0.0), EntityAttributes::default()),
        );
        assert_eq!(second.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn save_reopen_and_modified_flag() {
        // Its own directory, removed by name: the tests run at the same time,
        // and a remove of a directory they SHARED deleted the other's project
        // out from under it.
        let scratch = ScratchDirectory::new("document");
        let directory = scratch.path.join("document.katana");
        {
            let mut document = Document::default();
            assert!(!document.is_modified());
            assert_eq!(
                document.save().expect_err("nowhere to save yet").code,
                ErrorCode::InvalidState
            );

            assert!(document.execute(create_layer(layer("Roads"))).is_ok());
            must_create(
                &mut document,
                create_line(
                    Point2::new(0.0, 0.0),
                    Point2::new(10.0, 10.0),
                    on_layer("Roads"),
                ),
            );
            assert!(document.is_modified());

            let saved = document.save_as(&directory);
            assert!(saved.is_ok(), "{saved:?}");
            assert!(!document.is_modified());
            assert_eq!(document.metadata().name, "document");
            assert_eq!(
                document
                    .save_as(&directory)
                    .expect_err("already there")
                    .code,
                ErrorCode::AlreadyExists
            );

            must_create(
                &mut document,
                create_circle(Point2::new(5.0, 5.0), 2.0, EntityAttributes::default()),
            );
            assert!(document.is_modified());
            assert!(document.save().is_ok());
            assert!(!document.is_modified());
            assert!(document.undo().is_ok());
            assert!(document.is_modified()); // differs from what is on disk
        }
        {
            let mut reopened = Document::default();
            must_create(
                &mut reopened,
                create_point(Point2::new(99.0, 99.0), EntityAttributes::default()),
            ); // replaced by open()
            let opened = reopened.open(&directory);
            assert!(opened.is_ok(), "{opened:?}");
            assert_eq!(reopened.model().entities.size(), 2);
            assert!(reopened.model().layers.contains("Roads"));
            assert!(!reopened.is_modified());
            assert!(!reopened.history().can_undo()); // history does not cross documents

            // Editing keeps working after a reload (the stack was rebuilt).
            let id = must_create(
                &mut reopened,
                create_point(Point2::new(1.0, 1.0), EntityAttributes::default()),
            );
            assert_eq!(id, 3); // ids continue after the saved ones
            assert!(reopened.undo().is_ok());

            assert!(reopened.open(&scratch.path.join("missing.katana")).is_err());
            assert_eq!(reopened.model().entities.size(), 2); // a failed open changes nothing

            reopened.new_document();
            assert!(reopened.model().entities.empty());
            assert!(!reopened.has_project());
        }
    }

    #[test]
    fn replacing_the_library_or_the_map_moves_its_own_generation_only() {
        // A cache of thumbnails keyed on the library must go stale when the
        // library is replaced, and must NOT when only the map is - or an edit
        // to a code would repaint eight hundred symbols.
        let mut document = Document::default();
        let library = document.library_generation();
        let map = document.survey_map_generation();

        document.set_style_library(StyleLibrary::default());
        assert_eq!(document.library_generation(), library + 1);
        assert_eq!(document.survey_map_generation(), map);

        document.set_survey_map(SurveyMap::default());
        assert_eq!(document.library_generation(), library + 1);
        assert_eq!(document.survey_map_generation(), map + 1);

        // An edit to the drawing is neither.
        let mut session = Session::default();
        let before = session.document.library_generation();
        session.ok("STYLE NEW s");
        assert_eq!(session.document.library_generation(), before);
    }

    #[test]
    fn the_model_revision_counts_every_step_and_every_new_or_opened_drawing_and_nothing_else() {
        // Worked by hand, one line at a time: a command, its undo and its
        // redo are +1 each (3); a refused command, a selection, the current
        // layer, a library, a map, a save and the metadata are +0; a new
        // drawing is +1 (4); an open is +1 (5) - even of the same empty
        // project, which is the case the history's counts and the table sizes
        // could not see.
        let scratch = ScratchDirectory::new("revision");
        let directory = scratch.path.join("revision.katana");
        let mut document = Document::default();
        assert_eq!(document.model_revision(), 0);
        assert!(document.execute(create_layer(layer("Roads"))).is_ok());
        assert_eq!(document.model_revision(), 1);
        assert!(document.execute(create_layer(layer("Roads"))).is_err());
        assert_eq!(
            document.model_revision(),
            1,
            "a refused command changed nothing"
        );
        assert!(document.undo().is_ok());
        assert_eq!(document.model_revision(), 2);
        assert!(document.redo().is_ok());
        assert_eq!(document.model_revision(), 3);

        document.notify_selection_changed();
        assert!(document.set_current_layer("Roads").is_ok());
        document.set_style_library(StyleLibrary::default());
        document.set_survey_map(SurveyMap::default());
        assert_eq!(document.model_revision(), 3);

        document.new_document();
        assert_eq!(document.model_revision(), 4);
        assert!(document.save_as(&directory).is_ok());
        let mut metadata = document.metadata().clone();
        metadata.name = String::from("renamed");
        document.set_metadata(metadata);
        assert_eq!(document.model_revision(), 4);
        let opened = document.open(&directory);
        assert!(opened.is_ok(), "{opened:?}");
        assert_eq!(document.model_revision(), 5);
        assert!(document.open(&scratch.path.join("missing.katana")).is_err());
        assert_eq!(
            document.model_revision(),
            5,
            "a failed open changes nothing"
        );
        // The project's database is open until the Document goes, and Windows
        // will not delete an open file: the document is dropped before the
        // scratch directory.
        drop(document);
    }
}

// ---- CommandInterpreter ---------------------------------------------------------------

mod cad_interpreter {
    use super::*;

    #[test]
    fn draws_with_absolute_relative_and_polar_points() {
        let mut s = Session::default();
        s.ok("line 0,0 10,0");
        assert_eq!(
            segment(&s.entity(1)),
            Segment2::new(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0))
        );

        s.ok("L 10,0 @0,5"); // alias + relative to the last point
        assert_eq!(segment(&s.entity(2)).end, Point2::new(10.0, 5.0));

        s.ok("LINE 0,0 @10<90"); // polar: 10 units at 90 degrees
        assert!(nearly(segment(&s.entity(3)).end, Point2::new(0.0, 10.0)));

        s.ok("CIRCLE 5,5 2.5");
        double_eq(circle(&s.entity(4)).radius, 2.5);

        s.ok("ARC 1,0 0,1 -1,0");
        near(arc(&s.entity(5)).radius, 1.0, 1e-12);

        s.ok("PLINE 0,0 4,0 4,3 CLOSE");
        double_eq(polyline(&s.entity(6)).area(), 6.0);

        s.ok("RECT 10,10 0,0");
        double_eq(polyline(&s.entity(7)).area(), 100.0);

        s.ok("TEXT 1,1 2.5 \"Bench mark A\"");
        assert_eq!(text(&s.entity(8)).text, "Bench mark A");

        s.ok("DIM 0,0 10,0 2");
        double_eq(dimension(&s.entity(9)).measurement(), 10.0);
        s.ok("POINT 3,4");
        assert_eq!(s.document.model().entities.size(), 10);
    }

    #[test]
    fn chained_line_is_one_undo_step() {
        let mut s = Session::default();
        assert_eq!(s.ok("LINE 0,0 10,0 10,10 0,10"), "3 lines created");
        assert_eq!(s.document.model().entities.size(), 3);
        s.ok("U");
        assert!(s.document.model().entities.empty());
    }

    #[test]
    fn rejects_malformed_input_without_side_effects() {
        let mut s = Session::default();
        assert_eq!(s.fails("FROBNICATE 1 2"), ErrorCode::ParseFailure);
        assert_eq!(s.fails("LINE 0,0"), ErrorCode::InvalidArgument); // usage
        assert_eq!(s.fails("LINE 0,0 ten,0"), ErrorCode::ParseFailure); // not a number
        assert_eq!(s.fails("LINE 0,0 1,5x"), ErrorCode::ParseFailure); // trailing junk
        assert_eq!(s.fails("LINE 0,0 inf,0"), ErrorCode::ParseFailure); // non-finite
        assert_eq!(s.fails("CIRCLE 0,0 -5"), ErrorCode::InvalidGeometry); // validated by the model
        assert_eq!(s.fails("LINE 3,3 3,3"), ErrorCode::InvalidGeometry); // zero length
        assert_eq!(s.fails("ARC 0,0 1,1 2,2"), ErrorCode::InvalidGeometry); // collinear
        assert_eq!(
            s.fails("TEXT 0,0 2.5 \"unterminated"),
            ErrorCode::ParseFailure
        );
        // An empty quoted argument. Title casing the type name starts at the
        // second character, so an empty string used to hand std::transform a
        // first that is past its last and write off the end of the buffer
        // (access violation).
        assert_eq!(s.fails("SELECT TYPE \"\""), ErrorCode::ParseFailure);
        assert_eq!(s.fails("SELECT LAYER \"\""), ErrorCode::NotFound);
        assert_eq!(s.fails("MOVE 1,1"), ErrorCode::InvalidState); // nothing selected
        assert!(s.document.model().entities.empty());
        assert!(!s.document.history().can_undo());
        assert_eq!(s.ok(""), "");
        assert_eq!(s.ok("   "), "");
    }

    #[test]
    fn relative_point_needs_a_previous_point() {
        let mut s = Session::default();
        assert_eq!(s.fails("LINE @1,1 5,5"), ErrorCode::InvalidState);
        assert_eq!(s.fails("LINE 0,0 5<45"), ErrorCode::ParseFailure); // polar must be relative
    }

    #[test]
    fn select_and_transform() {
        let mut s = Session::default();
        s.ok("LINE 0,0 10,0");
        s.ok("CIRCLE 5,5 1");
        assert_eq!(s.ok("SELECT ALL"), "2 selected");
        assert_eq!(s.ok("MOVE 100,200"), "2 moved");
        assert_eq!(segment(&s.entity(1)).start, Point2::new(100.0, 200.0));

        assert_eq!(s.ok("SELECT TYPE circle"), "1 selected");
        s.ok("COPY 10,0");
        assert_eq!(s.document.model().entities.size(), 3);

        assert_eq!(s.ok("SELECT 1"), "1 selected");
        s.ok("ROTATE 100,200 90");
        assert!(nearly(segment(&s.entity(1)).end, Point2::new(100.0, 210.0)));
        s.ok("SCALE 100,200 2");
        assert!(nearly(segment(&s.entity(1)).end, Point2::new(100.0, 220.0)));
        s.ok("MIRROR 0,0 0,1 KEEP");
        assert_eq!(s.document.model().entities.size(), 4);
        s.ok("ARRAY 2 2 50,50");
        assert_eq!(s.document.model().entities.size(), 7);

        s.ok("SELECT ALL");
        assert_eq!(s.ok("ERASE"), "7 erased");
        assert!(s.document.selection().empty());
        assert_eq!(s.ok("UNDO 3"), "3 undone");
        assert_eq!(s.ok("REDO 99"), "3 redone"); // stops when history runs out
        assert_eq!(s.fails("SELECT 999"), ErrorCode::NotFound);
    }

    #[test]
    fn editing_commands() {
        let mut s = Session::default();
        s.ok("LINE 0,0 10,0"); // 1
        s.ok("LINE 3,-2 3,2"); // 2
        s.ok("LINE 7,-2 7,2"); // 3
        s.ok("TRIM 1 5,0 2 3"); // removes the middle of line 1
        assert_eq!(
            segment(&s.entity(1)),
            Segment2::new(Point2::new(0.0, 0.0), Point2::new(3.0, 0.0))
        );
        assert_eq!(s.document.model().entities.size(), 4);

        s.ok("LINE 20,0 24,0"); // 5
        s.ok("LINE 30,-5 30,5"); // 6
        s.ok("EXTEND 5 24,0 6");
        assert_eq!(segment(&s.entity(5)).end, Point2::new(30.0, 0.0));

        s.ok("OFFSET 5 2 25,9");
        assert_eq!(
            segment(&s.entity(7)),
            Segment2::new(Point2::new(20.0, 2.0), Point2::new(30.0, 2.0))
        );

        s.ok("LINE 50,0 40,0"); // 8
        s.ok("LINE 40,0 40,10"); // 9
        s.ok("FILLET 8 9 2");
        assert_eq!(s.entity(10).r#type(), EntityType::Arc);
        s.ok("U");
        assert!(!s.document.model().entities.contains(10));
        s.ok("CHAMFER 8 9 1"); // single distance applies to both sides
        // The bevel is entity 11, not 10: the undone fillet arc retired id 10
        // for good.
        assert!(s.document.model().entities.contains(11));
        assert_eq!(
            segment(&s.entity(11)),
            Segment2::new(Point2::new(41.0, 0.0), Point2::new(40.0, 1.0))
        );

        assert_eq!(s.fails("FILLET 8 9 5000"), ErrorCode::InvalidGeometry);
        assert_eq!(s.fails("TRIM 1 1,0 3"), ErrorCode::InvalidGeometry); // cutter 3 no longer crosses
        assert_eq!(s.fails("OFFSET abc 2 0,0"), ErrorCode::ParseFailure);
    }

    #[test]
    fn layers_and_attributes() {
        let mut s = Session::default();
        s.ok("LAYER NEW Survey #FF0000");
        s.ok("LAYER SET Survey");
        s.ok("POINT 1,1");
        assert_eq!(s.entity(1).layer, "Survey");
        assert_eq!(
            s.document
                .model()
                .layers
                .find("Survey")
                .map(|layer| layer.color.to_hex()),
            Some(String::from("#FF0000"))
        );

        let listing = s.ok("LAYER LIST");
        assert!(listing.contains("* Survey"), "{listing}"); // current layer is starred
        assert!(listing.contains("(1 entities)"), "{listing}");

        assert_eq!(s.fails("LAYER LOCK Survey"), ErrorCode::CommandRejected); // current layer
        assert_eq!(s.fails("LAYER DELETE Survey"), ErrorCode::CommandRejected); // in use
        assert_eq!(s.fails("LAYER NEW Survey"), ErrorCode::AlreadyExists);
        assert_eq!(s.fails("LAYER NEW Bad #GGGGGG"), ErrorCode::ParseFailure);
        assert_eq!(s.fails("LAYER SET Nowhere"), ErrorCode::NotFound);

        s.ok("SELECT ALL");
        s.ok("CHLAYER 0");
        s.ok("COLOR #00FF00");
        s.ok("PROP SET elevation 101.25");
        s.ok("PROP SET code IP");
        s.ok("PROP SET order 2");
        s.ok("PROP SET verified true");
        let entity = s.entity(1);
        let properties = &entity.properties;
        double_eq(real_of(properties.get("elevation")), 101.25);
        assert_eq!(text_of(properties.get("code")), "IP");
        assert_eq!(integer_of(properties.get("order")), 2);
        assert_eq!(
            properties.get("verified"),
            Some(&PropertyValue::Boolean(true))
        );
        assert_eq!(
            entity.color.map(|color| color.to_hex()),
            Some(String::from("#00FF00"))
        );
        s.ok("COLOR BYLAYER");
        assert!(s.entity(1).color.is_none());

        s.ok("LAYER SET 0");
        s.ok("LAYER HIDE Survey");
        assert_eq!(
            s.document
                .model()
                .layers
                .find("Survey")
                .map(|layer| layer.visible),
            Some(false)
        );
        s.ok("LAYER DELETE Survey"); // empty now
        let info = s.ok("INFO 1");
        assert!(info.contains("elevation = 101.25"), "{info}");
        assert!(s.ok("LIST").contains("1 entities"));
    }

    #[test]
    fn save_and_open_round_trip_through_the_command_line() {
        // Its own directory: see CadDocument.SaveReopenAndModifiedFlag.
        let scratch = ScratchDirectory::new("cli");
        let directory = scratch.path.join("cli.katana");
        let generic = |path: &std::path::Path| path.to_string_lossy().replace('\\', "/");
        let quoted = format!("\"{}\"", generic(&directory));
        {
            let mut s = Session::default();
            s.ok("RECT 0,0 30,20");
            assert_eq!(s.fails("SAVE"), ErrorCode::InvalidState);
            s.ok(&format!("SAVE {quoted}"));
            s.ok("CIRCLE 15,10 5");
            s.ok("SAVE");
        }
        {
            let mut s = Session::default();
            s.ok(&format!("OPEN {quoted}"));
            assert_eq!(s.document.model().entities.size(), 2);
            s.ok("NEW");
            assert!(s.document.model().entities.empty());
            assert_eq!(
                s.fails(&format!("OPEN \"{}\"", generic(&scratch.path.join("nope")))),
                ErrorCode::NotFound
            );
        }
    }

    #[test]
    fn info_takes_an_id_written_plain_or_as_an_anchored_point_names_it() {
        let mut s = Session::default();
        s.ok("RECT 0,0 10,5");
        // A 10 x 5 rectangle: perimeter 30, area 50.
        let plain = s.ok("INFO 1");
        assert_eq!(
            plain,
            "1  Polyline  layer=0  vertices=4  closed  length=30  area=50"
        );
        assert_eq!(s.ok("INFO #1"), plain);
        assert_eq!(s.fails("INFO #2"), ErrorCode::NotFound);
        assert_eq!(s.fails("INFO #"), ErrorCode::ParseFailure);
        assert_eq!(s.fails("INFO 0"), ErrorCode::ParseFailure);
        assert_eq!(s.fails("INFO 1 2"), ErrorCode::InvalidArgument);
    }

    #[test]
    fn select_takes_ids_written_plain_or_as_info_takes_them() {
        let mut s = Session::default();
        s.ok("POINT 1,1");
        s.ok("POINT 2,2");
        s.ok("POINT 3,3");
        // The ids Select by ID and a DIM line give, #n among them.
        assert_eq!(s.ok("SELECT #1 3"), "2 selected");
        assert_eq!(s.document.selection().ids(), vec![1, 3]);
        assert_eq!(s.fails("SELECT #4"), ErrorCode::NotFound);
        assert_eq!(s.fails("SELECT #"), ErrorCode::ParseFailure);
        assert_eq!(s.fails("SELECT ##2"), ErrorCode::ParseFailure);
        // A refused line leaves the selection as it was.
        assert_eq!(s.document.selection().ids(), vec![1, 3]);
    }

    #[test]
    fn an_entity_id_is_a_positive_whole_number_plain_or_after_a_hash() {
        for word in ["1", "12", "#12", "18446744073709551615"] {
            assert!(CommandInterpreter::is_entity_id(word), "{word}");
        }
        // What a front end must still read as a file: anything else a path
        // can be.
        for word in [
            "",
            "#",
            "0",
            "#0",
            "-3",
            "1.5",
            "12a",
            "##12",
            "site.las",
            "18446744073709551616",
        ] {
            assert!(!CommandInterpreter::is_entity_id(word), "{word}");
        }
    }

    #[test]
    fn an_import_argument_loses_one_pair_of_quotes_and_a_final_unquoted_local() {
        let read = |rest: &str| -> (String, bool) {
            let argument = CommandInterpreter::import_argument(rest);
            assert!(argument.is_ok(), "{rest}");
            argument.map_or_else(
                |_| (String::new(), false),
                |argument| {
                    (
                        argument.path,
                        argument.placement.mode == ImportPlacementMode::Local,
                    )
                },
            )
        };
        let pair = |path: &str, local: bool| (String::from(path), local);
        assert_eq!(read(" site.dxf"), pair("site.dxf", false));
        assert_eq!(read(" site.dxf LOCAL"), pair("site.dxf", true));
        assert_eq!(read(" site.dxf local "), pair("site.dxf", true));
        // What katana_import sends: the path quoted, LOCAL after it. The quotes
        // were once left on, and every such import looked for
        // "\"C:/a b.geojson\"".
        assert_eq!(
            read(" \"C:/Survey Data/parcels.geojson\" LOCAL"),
            pair("C:/Survey Data/parcels.geojson", true)
        );
        assert_eq!(
            read("\t\"C:/Survey Data/parcels.geojson\"\tLOCAL"),
            pair("C:/Survey Data/parcels.geojson", true)
        );
        // A path with blanks may still be typed bare.
        assert_eq!(
            read(" C:/Survey Data/parcels.geojson"),
            pair("C:/Survey Data/parcels.geojson", false)
        );
        // LOCAL inside the quotes is the file's name, and a word ending in
        // LOCAL is not the keyword.
        assert_eq!(read(" \"yard LOCAL\""), pair("yard LOCAL", false));
        assert_eq!(read(" GLOCAL"), pair("GLOCAL", false));
        assert_eq!(read(" LOCAL"), pair("LOCAL", false));
        // Nothing: a usage error for the front end to give.
        assert_eq!(read(""), pair("", false));
        assert_eq!(read("   "), pair("", false));
        assert_eq!(read(" \"\" LOCAL"), pair("", true));
    }

    #[test]
    fn an_import_argument_takes_alongside_or_an_offset_as_its_last_word_and_refuses_a_bad_offset() {
        let along = CommandInterpreter::import_argument(" \"C:/Survey Data/site.dxf\" alongside")
            .expect("alongside");
        assert_eq!(along.path, "C:/Survey Data/site.dxf");
        assert_eq!(
            along.placement,
            ImportPlacement {
                mode: ImportPlacementMode::Alongside,
                offset: Vec2::default(),
            }
        );

        let moved =
            CommandInterpreter::import_argument(" site.dxf OFFSET=-12.5,300").expect("an offset");
        assert_eq!(moved.path, "site.dxf");
        assert_eq!(
            moved.placement,
            ImportPlacement {
                mode: ImportPlacementMode::Offset,
                offset: Vec2::new(-12.5, 300.0),
            }
        );

        // The word alone is a file of that name, and inside the quotes it is
        // the file's too.
        for rest in [" ALONGSIDE", " OFFSET=1,2", " \"yard OFFSET=1,2\""] {
            let file = CommandInterpreter::import_argument(rest).expect(rest);
            assert_eq!(file.placement.mode, ImportPlacementMode::Keep, "{rest}");
        }
        // A mistyped offset is refused, never read as part of the file's name.
        for rest in [
            " site.dxf OFFSET=",
            " site.dxf OFFSET=1",
            " site.dxf offset=1,x",
            " site.dxf OFFSET=1,2,3",
            " site.dxf OFFSET=inf,0",
            " site.dxf OFFSET=1;2",
        ] {
            let refused = CommandInterpreter::import_argument(rest).expect_err(rest);
            assert_eq!(refused.code, ErrorCode::InvalidArgument, "{rest}");
        }
    }

    #[test]
    fn keeps_a_history_and_has_help() {
        let mut s = Session::default();
        s.ok("POINT 1,1");
        let _ = s.interpreter.run(&mut s.document, "NONSENSE");
        assert_eq!(
            s.interpreter.history(),
            &[String::from("POINT 1,1"), String::from("NONSENSE")]
        );
        assert!(s.ok("HELP").contains("FILLET"));
        assert!(s.ok("?").contains("FILLET"));
    }

    // Regression. The interpreter remembers the last point so that "@10,10"
    // can be relative to it. It cannot see the document being replaced behind
    // its back, so after File > New the GUI used to resolve a relative point
    // against the DISCARDED drawing's last point - silently placing geometry
    // somewhere the user never indicated, with no error to show for it.
    #[test]
    fn relative_point_state_is_forgotten_when_the_document_is_replaced() {
        let mut s = Session::default();
        s.ok("LINE 100,100 110,100"); // last point is now 110,100

        // Without a reset the next relative point would resolve against
        // 110,100.
        s.document.new_document();
        s.interpreter.reset_point_state();

        assert_eq!(
            s.fails("LINE @10,10 @5,0"),
            ErrorCode::InvalidState,
            "a relative point with no previous point must be refused, not invented"
        );

        // An absolute point re-establishes the reference, and relative works
        // again.
        s.ok("LINE 0,0 @10,0");
        // Not a hard-coded id: the counter is not reset by new_document(),
        // because ids are never reused - so the new line does NOT start again
        // at 1.
        let ids = s.document.model().entities.ids();
        assert_eq!(ids.len(), 1);
        assert_eq!(segment(&s.entity(ids[0])).end, Point2::new(10.0, 0.0));
    }

    #[test]
    fn linetypes_can_be_defined_listed_and_attached_to_a_layer() {
        // The whole point of the verb: without it a pattern can be defined in
        // the library and never reached from the application, which is
        // finished work in the tests and unfinished work to the user.
        let mut session = Session::default();
        assert!(session.ok("LINETYPE LIST").contains("continuous"));

        session.ok("LINETYPE NEW fence 1 -0.5");
        let listed = session.ok("LINETYPE LIST");
        assert!(listed.contains("fence"), "{listed}");
        assert!(listed.contains("period=1.5"), "{listed}");

        session.ok("LAYER NEW boundary");
        session.ok("LAYER LTYPE boundary fence");
        assert_eq!(
            session
                .document
                .model()
                .layers
                .find("boundary")
                .map(|layer| layer.linetype.clone()),
            Some(String::from("fence"))
        );

        // A layer still using it blocks deletion, naming the layer so it can
        // be found rather than just refusing.
        let refused = session
            .interpreter
            .run(&mut session.document, "LINETYPE DELETE fence")
            .expect_err("in use");
        assert!(
            refused.describe().contains("boundary"),
            "{}",
            refused.describe()
        );
    }

    #[test]
    fn a_linetype_with_an_odd_number_of_lengths_is_refused_with_a_useful_reason() {
        // "must end with a gap" is true and unhelpful; the actual mistake is
        // forgetting the gap after the last dash.
        let mut session = Session::default();
        let refused = session
            .interpreter
            .run(&mut session.document, "LINETYPE NEW bad 1 -0.5 1")
            .expect_err("odd");
        assert!(
            refused.describe().contains("pairs"),
            "{}",
            refused.describe()
        );

        // And the DXF rules still apply underneath.
        assert_eq!(
            session.fails("LINETYPE NEW bad -1 0.5"),
            ErrorCode::InvalidArgument,
            "must not start with a gap"
        );
        session.fails("LINETYPE DELETE continuous");
        session.fails("LAYER LTYPE 0 no-such-pattern");
    }

    #[test]
    fn open_reports_how_many_entities_it_actually_opened() {
        // The count used to be built as a sibling ARGUMENT to
        // document_.open() in the same call, and C++ does not order function
        // arguments - so it was read before the open ran and every OPEN
        // reported "(0 entities)" however large the project was.
        let scratch = ScratchDirectory::new("open-count");
        let directory = scratch.path.join("katana_open_count_test");
        let path = directory.to_string_lossy().into_owned();
        {
            let mut session = Session::default();
            session.ok("LINE 0,0 10,0");
            session.ok("LINE 0,5 10,5");
            session.ok("CIRCLE 5,5 2");
            session.ok(&format!("SAVE {path}"));
        }

        let mut reopened = Session::default();
        let reply = reopened.ok(&format!("OPEN {path}"));
        assert!(reply.contains("(3 entities)"), "reported: {reply}");
        assert_eq!(reopened.document.model().entities.size(), 3);
        drop(reopened);
    }

    #[test]
    fn dimension_styles_can_be_defined_tuned_and_attached_to_a_layer() {
        let mut session = Session::default();
        assert!(session.ok("DIMSTYLE LIST").contains("Standard"));

        session.ok("DIMSTYLE NEW site");
        session.ok("DIMSTYLE SET site DECIMALS 1");
        session.ok("DIMSTYLE SET site SCALE 1000");
        session.ok("DIMSTYLE SET site SUFFIX mm");
        session.ok("DIMSTYLE SET site HEAD Dot");

        let style = session
            .document
            .model()
            .dimension_styles
            .find("site")
            .cloned()
            .expect("site");
        assert_eq!(style.decimals, 1);
        double_eq(style.unit_scale, 1000.0);
        assert_eq!(style.arrow_head, katana_entity::ArrowHead::Dot);

        // LIST shows what a ten-unit dimension would READ as, which is the
        // question anyone setting a style is actually asking.
        let listed = session.ok("DIMSTYLE LIST");
        assert!(listed.contains("10000.0mm"), "{listed}");

        session.ok("LAYER NEW dims");
        session.ok("LAYER DIMSTYLE dims site");
        assert_eq!(
            session
                .document
                .model()
                .layers
                .find("dims")
                .map(|layer| layer.dimension_style.clone()),
            Some(String::from("site"))
        );

        // A layer still using it blocks deletion, naming the layer.
        let refused = session
            .interpreter
            .run(&mut session.document, "DIMSTYLE DELETE site")
            .expect_err("in use");
        assert!(
            refused.describe().contains("dims"),
            "{}",
            refused.describe()
        );
    }

    #[test]
    fn dimension_style_rejects_nonsense_with_its_own_reason() {
        let mut session = Session::default();
        session.ok("DIMSTYLE NEW site");

        assert_eq!(
            session.fails("DIMSTYLE SET site TEXT 0"),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            session.fails("DIMSTYLE SET site DECIMALS 99"),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            session.fails("DIMSTYLE SET site HEAD Diamond"),
            ErrorCode::ParseFailure
        );
        assert_eq!(
            session.fails("DIMSTYLE SET site NOSUCHFIELD 1"),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            session.fails("DIMSTYLE SET missing TEXT 1"),
            ErrorCode::NotFound
        );
        session.fails("DIMSTYLE DELETE Standard");
        session.fails("LAYER DIMSTYLE 0 no-such-style");

        // A refused change must leave the stored style alone.
        double_eq(
            session
                .document
                .model()
                .dimension_styles
                .find("site")
                .map_or(f64::NAN, |style| style.text_height),
            2.5,
        );
    }

    #[test]
    fn hatch_patterns_can_be_defined_and_attached_to_a_layer() {
        let mut session = Session::default();
        // Only the built-in is there to begin with, so a drawing is unhatched
        // until someone says otherwise.
        assert!(session.ok("HATCH LIST").contains("none"));

        session.ok("HATCH NEW brick 0 0.25");
        session.ok("HATCH SOLID concrete");
        session.ok("LAYER NEW paving");
        session.ok("LAYER HATCH paving brick");

        let model = session.document.model();
        let pattern = model.hatch_patterns.find("brick").expect("brick");
        assert_eq!(pattern.families.len(), 1);
        double_eq(pattern.families[0].spacing, 0.25);
        double_eq(pattern.families[0].angle, 0.0);
        assert!(!pattern.solid);

        assert!(
            model
                .hatch_patterns
                .find("concrete")
                .expect("concrete")
                .solid
        );
        assert_eq!(
            model.layers.find("paving").expect("paving").hatch_pattern,
            "brick"
        );
    }

    #[test]
    fn hatch_angles_are_typed_in_degrees_and_stored_in_radians() {
        // Every other angle the interpreter takes is in degrees, because that
        // is what a drafter types. The model keeps one unit. 45 degrees is
        // pi/4 exactly as a mathematical quantity, so this is a real
        // conversion check and not a restatement of whatever the parser
        // produced.
        let mut session = Session::default();
        session.ok("HATCH NEW cross 45 0.5 135 0.5");

        let pattern = session
            .document
            .model()
            .hatch_patterns
            .find("cross")
            .cloned()
            .expect("cross");
        assert_eq!(pattern.families.len(), 2);
        near(pattern.families[0].angle, (-1.0_f64).acos() / 4.0, 1e-12);
        near(
            pattern.families[1].angle,
            3.0 * (-1.0_f64).acos() / 4.0,
            1e-12,
        );

        // And LIST puts them back in degrees, so what is typed is what is
        // read.
        let listed = session.ok("HATCH LIST");
        assert!(listed.contains("45 deg"), "{listed}");
        assert!(listed.contains("135 deg"), "{listed}");
    }

    #[test]
    fn hatch_rejects_nonsense_with_its_own_reason() {
        let mut session = Session::default();
        // An odd number of arguments means an angle with no spacing, which
        // would otherwise be read as a spacing of whatever came next.
        assert_eq!(
            session.fails("HATCH NEW lopsided 45"),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            session.fails("HATCH NEW zero 0 0"),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            session.fails("HATCH NEW negative 0 -1"),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            session.fails("HATCH DELETE none"),
            ErrorCode::CommandRejected
        );
        assert!(session.document.model().hatch_patterns.contains("none"));
        assert!(!session.document.model().hatch_patterns.contains("lopsided"));
    }

    #[test]
    fn deleting_a_hatch_pattern_a_layer_still_uses_is_refused_and_names_the_layer() {
        // Allowing it would leave the layer naming a pattern that is gone, and
        // the drawing would quietly stop being hatched with nothing to say
        // why.
        let mut session = Session::default();
        session.ok("HATCH NEW brick 0 0.25");
        session.ok("LAYER NEW paving");
        session.ok("LAYER HATCH paving brick");

        let refused = session
            .interpreter
            .run(&mut session.document, "HATCH DELETE brick")
            .expect_err("in use");
        assert!(
            refused.describe().contains("paving"),
            "{}",
            refused.describe()
        );
        assert!(session.document.model().hatch_patterns.contains("brick"));

        // Detach it and the deletion goes through.
        session.ok("LAYER HATCH paving none");
        session.ok("HATCH DELETE brick");
        assert!(!session.document.model().hatch_patterns.contains("brick"));
    }

    #[test]
    fn creating_a_hatch_pattern_is_undoable() {
        let mut session = Session::default();
        session.ok("HATCH NEW brick 0 0.25");
        assert!(session.document.model().hatch_patterns.contains("brick"));
        session.ok("UNDO");
        assert!(!session.document.model().hatch_patterns.contains("brick"));
        session.ok("REDO");
        assert!(session.document.model().hatch_patterns.contains("brick"));
    }

    // ---- styles -----------------------------------------------------------------------

    #[test]
    fn styles_can_be_defined_tuned_applied_and_listed() {
        let mut session = Session::default();
        assert_eq!(session.ok("STYLE LIST"), "no styles");
        session.ok("LINETYPE NEW fence 1 -0.5");
        session.ok("STYLE NEW Kerb");
        session.ok("STYLE SET Kerb linetype fence");
        session.ok("STYLE SET Kerb weight 0.5");
        session.ok("STYLE SET Kerb colour #FF8000");
        session.ok("STYLE SET Kerb symbol cross");
        session.ok("STYLE SET Kerb symbolsize 1.5");
        session.ok("STYLE SET Kerb description from the library, colour shade 48");

        let kerb = session
            .document
            .model()
            .styles
            .find("Kerb")
            .cloned()
            .expect("Kerb");
        assert_eq!(kerb.linetype, "fence");
        double_eq(kerb.line_weight, 0.5);
        assert_eq!(
            kerb.color.map(|color| color.to_hex()),
            Some(String::from("#FF8000"))
        );
        assert_eq!(kerb.symbol, "cross");
        double_eq(kerb.symbol_size, 1.5);
        assert_eq!(kerb.description, "from the library, colour shade 48");
        let listed = session.ok("STYLE LIST");
        assert!(listed.contains("Kerb"));
        assert!(listed.contains("symbol=cross@1.5"), "{listed}");
        assert!(session.ok("STYLE SYMBOLS").contains("manhole"));

        session.ok("POINT 1,1");
        session.ok("SELECT ALL");
        session.ok("STYLE APPLY Kerb");
        let ids = session.document.model().entities.ids();
        assert_eq!(ids.len(), 1);
        assert_eq!(session.entity(ids[0]).style, "Kerb");
        session.ok("STYLE APPLY -");
        assert!(session.entity(ids[0]).style.is_empty());
    }

    #[test]
    fn properties_are_listed_typed_renamed_and_removed() {
        // PLAN.MD 20.2 slice 5: what the attribute manager does, without the
        // GUI.
        let mut s = Session::default();
        s.ok("POINT 1,1");
        s.ok("SELECT ALL");

        // The type is guessed from the value, unless it is stated - and a
        // feature code like "2" must be able to stay text.
        s.ok("PROP SET Level 31.25");
        s.ok("PROP SET Code 2 text");
        s.ok("PROP SET Order 2");
        assert_eq!(text_of(s.entity(1).properties.get("Code")), "2");
        assert_eq!(integer_of(s.entity(1).properties.get("Order")), 2);
        double_eq(real_of(s.entity(1).properties.get("Level")), 31.25);
        assert_eq!(s.fails("PROP SET Bad x integer"), ErrorCode::ParseFailure);
        assert_eq!(s.fails("PROP SET Bad 1 colour"), ErrorCode::InvalidArgument);

        let listed = s.ok("PROP LIST");
        assert!(listed.contains("Code=2 (text)"), "{listed}");
        assert!(listed.contains("Order=2 (integer)"), "{listed}");
        assert!(listed.contains("Level=31.25 (real)"), "{listed}");

        // Rename keeps the value and its type; it is one undoable step.
        s.ok("PROP RENAME Level RL");
        assert!(!s.entity(1).properties.contains_key("Level"));
        double_eq(real_of(s.entity(1).properties.get("RL")), 31.25);
        s.ok("UNDO");
        double_eq(real_of(s.entity(1).properties.get("Level")), 31.25);
        s.ok("REDO");

        // Renaming onto a name the entity already has would drop one of the
        // two values, so it is refused rather than guessed.
        assert_eq!(s.fails("PROP RENAME RL Code"), ErrorCode::AlreadyExists);
        assert_eq!(s.fails("PROP RENAME Nope x"), ErrorCode::NotFound);
        double_eq(real_of(s.entity(1).properties.get("RL")), 31.25); // a refused rename changes nothing

        s.ok("PROP DELETE Code");
        assert!(!s.entity(1).properties.contains_key("Code"));
        assert_eq!(s.fails("PROP DELETE Code"), ErrorCode::NotFound);
        s.ok("UNDO");
        assert_eq!(text_of(s.entity(1).properties.get("Code")), "2");
    }

    #[test]
    fn renaming_a_style_or_a_linetype_takes_everything_that_named_it_along() {
        // What an archive import leaves to tidy: a style named after a
        // linestyle and worn by the points, and a linetype named by a layer
        // and a style.
        let mut session = Session::default();
        session.ok("LINETYPE NEW fence 1 -0.5");
        // Quoted, because a library style name has spaces and dropping the
        // rest of one silently is what the arity check below now refuses.
        session.ok("STYLE NEW \"TOPO Natural Surface Point\"");
        assert_eq!(
            session.fails("STYLE NEW TOPO Natural Surface Point"),
            ErrorCode::InvalidArgument
        );
        session.ok("STYLE SET \"TOPO Natural Surface Point\" linetype fence");
        session.ok("LAYER NEW survey");
        session.ok("LAYER LTYPE survey fence");
        session.ok("POINT 1,1");
        session.ok("SELECT ALL");
        session.ok("STYLE APPLY \"TOPO Natural Surface Point\"");
        let ids = session.document.model().entities.ids();
        assert_eq!(ids.len(), 1);

        session.ok("STYLE RENAME \"TOPO Natural Surface Point\" Ground");
        assert_eq!(session.entity(ids[0]).style, "Ground");
        assert!(
            !session
                .document
                .model()
                .styles
                .contains("TOPO Natural Surface Point")
        );

        session.ok("LINETYPE RENAME fence \"post and rail\"");
        let model = session.document.model();
        assert_eq!(
            model.layers.find("survey").expect("survey").linetype,
            "post and rail"
        );
        assert_eq!(
            model.styles.find("Ground").expect("Ground").linetype,
            "post and rail"
        );

        // One undo step each, entities and holders included.
        session.ok("UNDO");
        assert_eq!(
            session
                .document
                .model()
                .layers
                .find("survey")
                .expect("survey")
                .linetype,
            "fence"
        );
        session.ok("UNDO");
        assert_eq!(session.entity(ids[0]).style, "TOPO Natural Surface Point");

        assert_eq!(session.fails("STYLE RENAME nosuch x"), ErrorCode::NotFound);
        assert_eq!(
            session.fails("LINETYPE RENAME continuous solid"),
            ErrorCode::CommandRejected
        );
    }

    #[test]
    fn style_rejects_nonsense_with_its_own_reason() {
        let mut session = Session::default();
        session.ok("STYLE NEW s");
        assert_eq!(session.fails("STYLE NEW s"), ErrorCode::AlreadyExists);
        assert_eq!(
            session.fails("STYLE SET s linetype nosuch"),
            ErrorCode::NotFound
        );
        assert_eq!(
            session.fails("STYLE SET s hatch nosuch"),
            ErrorCode::NotFound
        );
        assert_eq!(
            session.fails("STYLE SET s symbol blob"),
            ErrorCode::NotFound,
            "a symbol name is no longer a closed set, but one nothing defines does not exist - \
             reported the way `hatch nosuch` on the line above is"
        );
        assert_eq!(
            session.fails("STYLE SET s symbolsize -1"),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            session.fails("STYLE SET s weight -0.5"),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            session.fails("STYLE SET s colour notacolour"),
            ErrorCode::ParseFailure,
            "a colour that does not parse is a parse failure, as LAYER NEW reports it"
        );
        assert_eq!(
            session.fails("STYLE SET nosuch weight 1"),
            ErrorCode::NotFound
        );
        assert_eq!(
            session.fails("STYLE APPLY s"),
            ErrorCode::InvalidState,
            "nothing selected"
        );
        assert_eq!(session.fails("STYLE DELETE nosuch"), ErrorCode::NotFound);
    }

    #[test]
    fn a_style_can_name_a_symbol_from_the_loaded_library_but_not_one_from_nowhere() {
        // The point of PLAN.MD 20.3: a customisation brings hundreds of
        // symbols, so the sixteen built-in names stopped being the whole set.
        let mut session = Session::default();
        session.ok("STYLE NEW s");
        assert_eq!(
            session.fails("STYLE SET s symbol TEST Bollard"),
            ErrorCode::NotFound,
            "no library is loaded yet"
        );

        let mut library = StyleLibrary::default();
        let bollard = LineStyle {
            name: String::from("TEST Bollard"),
            at_vertices: true,
            strokes: vec![Stroke {
                op: StrokeOp::Move,
                point: Point2::new(0.0, 0.0),
                ..Stroke::default()
            }],
            ..LineStyle::default()
        };
        assert!(library.add(bollard).is_ok());
        session.document.set_style_library(library);

        session.ok("STYLE SET s symbol TEST Bollard");
        assert_eq!(
            session.document.model().styles.find("s").expect("s").symbol,
            "TEST Bollard",
            "the whole name, spaces and all"
        );
        assert!(session.document.definition_for("TEST Bollard").is_some());
        assert!(session.document.definition_for("nothing").is_none());

        // A built-in still works with no library at all.
        session.ok("STYLE SET s symbol circle");
        assert_eq!(
            session.document.model().styles.find("s").expect("s").symbol,
            "circle"
        );
    }

    #[test]
    fn deleting_a_style_an_entity_still_uses_is_refused_and_names_the_entity() {
        // An entity left naming a deleted style would draw ByLayer with
        // nothing to say why.
        let mut session = Session::default();
        session.ok("STYLE NEW Kerb");
        session.ok("POINT 1,1");
        session.ok("SELECT ALL");
        session.ok("STYLE APPLY Kerb");
        let refused = session
            .interpreter
            .run(&mut session.document, "STYLE DELETE Kerb")
            .expect_err("in use");
        assert_eq!(refused.code, ErrorCode::CommandRejected);
        assert!(
            refused.describe().contains("id=1"),
            "{}",
            refused.describe()
        );
        session.ok("STYLE APPLY -");
        session.ok("STYLE DELETE Kerb");
        assert!(!session.document.model().styles.contains("Kerb"));
    }

    #[test]
    fn style_edits_are_undoable_and_the_other_tables_still_are() {
        // The table commands share one implementation now; each table's undo
        // is still checked, so a change to the template cannot lose one of
        // them.
        let mut session = Session::default();
        let weight = |session: &Session| {
            session
                .document
                .model()
                .styles
                .find("s")
                .map_or(f64::NAN, |style| style.line_weight)
        };
        session.ok("STYLE NEW s");
        session.ok("STYLE SET s weight 0.7");
        session.ok("UNDO");
        double_eq(weight(&session), 0.25);
        session.ok("UNDO");
        assert!(!session.document.model().styles.contains("s"));
        session.ok("REDO");
        session.ok("REDO");
        double_eq(weight(&session), 0.7);
        session.ok("STYLE DELETE s");
        assert!(!session.document.model().styles.contains("s"));
        session.ok("UNDO");
        assert!(session.document.model().styles.contains("s"));
        // Undo of a deletion restores the item as it was.
        double_eq(weight(&session), 0.7);

        session.ok("LINETYPE NEW fence 1 -0.5");
        session.ok("LINETYPE DELETE fence");
        session.ok("UNDO");
        assert!(session.document.model().linetypes.contains("fence"));
        session.ok("DIMSTYLE NEW site");
        session.ok("UNDO");
        assert!(!session.document.model().dimension_styles.contains("site"));
        assert_eq!(
            session.fails("DIMSTYLE DELETE Standard"),
            ErrorCode::CommandRejected
        );
        assert_eq!(
            session.fails("LINETYPE DELETE continuous"),
            ErrorCode::CommandRejected
        );
        assert_eq!(
            session.fails("HATCH DELETE none"),
            ErrorCode::CommandRejected
        );
    }

    // ---- the style and linetype managers' verbs ------------------------------------------

    // A library as a customisation load gives one: a linestyle, a vertex
    // symbol, and a NON-vertex definition its customisation lists as a symbol
    // - the kind most symbols in use are (docs/survey_coding.md). `symbol` is
    // that listing (LineStyle::symbol); the two symbols were once symbols
    // because their source's name held the word.
    fn load_manager_library(document: &mut Document) {
        let mut library = StyleLibrary::default();
        for (name, at_vertices, source, symbol) in [
            ("TEST Water Main", false, "linestyles_test.4d", false),
            ("TEST Bollard", true, "symbols_test.4d", true),
            ("TEST Manhole Cover", false, "symbols_test.4d", true),
        ] {
            let style = LineStyle {
                name: String::from(name),
                at_vertices,
                source: String::from(source),
                symbol,
                strokes: vec![Stroke {
                    op: StrokeOp::Draw,
                    point: Point2::new(1.0, 0.0),
                    ..Stroke::default()
                }],
                ..LineStyle::default()
            };
            assert!(library.add(style).is_ok());
        }
        document.set_style_library(library);
    }

    #[test]
    fn a_style_or_layer_linetype_may_name_a_library_linestyle_and_a_style_may_say_by_layer() {
        // audit CAD-06: the model and the viewport take a library linestyle
        // name, and STYLE SET refused one - so a user could get one only by
        // import.
        let mut session = Session::default();
        load_manager_library(&mut session.document);
        session.ok("STYLE NEW s");
        session.ok("STYLE SET s linetype TEST Water Main");
        let linetype = |session: &Session| {
            session
                .document
                .model()
                .styles
                .find("s")
                .expect("s")
                .linetype
                .clone()
        };
        assert_eq!(
            linetype(&session),
            "TEST Water Main",
            "the rest of the line, unquoted, as SYMBOL takes it"
        );
        session.ok("STYLE SET s linetype bylayer");
        assert_eq!(
            linetype(&session),
            "ByLayer",
            "any case, stored in the one spelling"
        );
        assert_eq!(
            session.fails("STYLE SET s linetype TEST Bollard"),
            ErrorCode::InvalidArgument,
            "a vertex symbol is never a line pattern; the reply says to give it as the symbol"
        );
        assert_eq!(
            session.fails("STYLE SET s linetype nosuch"),
            ErrorCode::NotFound
        );

        session.ok("LAYER NEW services");
        session.ok("LAYER LTYPE services TEST Water Main");
        assert_eq!(
            session
                .document
                .model()
                .layers
                .find("services")
                .expect("services")
                .linetype,
            "TEST Water Main"
        );
        assert_eq!(
            session.fails("LAYER LTYPE services ByLayer"),
            ErrorCode::InvalidArgument,
            "a layer is what ByLayer inherits from"
        );
        assert_eq!(
            session.fails("LAYER LTYPE services nosuch"),
            ErrorCode::NotFound
        );
    }

    #[test]
    fn style_symbols_lists_the_catalogues_symbols_and_filters_them() {
        let mut session = Session::default();
        load_manager_library(&mut session.document);
        let all = session.ok("STYLE SYMBOLS");
        // The built-ins, the vertex symbol, and the non-vertex one listed as a
        // symbol; not the linestyle.
        assert!(all.contains("  manhole  (built-in)"), "{all}");
        assert!(all.contains("  TEST Bollard  (library)"), "{all}");
        assert!(all.contains("  TEST Manhole Cover  (library)"), "{all}");
        assert!(!all.contains("TEST Water Main"), "{all}");

        // Filtered, case folded: "manhole" and "TEST Manhole Cover", in that
        // order (folded, "manhole" < "test manhole cover").
        assert_eq!(
            session.ok("STYLE SYMBOLS MANHOLE"),
            "  manhole  (built-in)\n  TEST Manhole Cover  (library)"
        );
        assert_eq!(session.ok("STYLE SYMBOLS zzz"), "no symbol matches");
    }

    #[test]
    fn style_usage_counts_the_entities_wearing_each_style() {
        let mut session = Session::default();
        session.ok("STYLE NEW Kerb");
        session.ok("STYLE NEW Spare");
        session.ok("POINT 1,1");
        session.ok("POINT 2,2");
        session.ok("SELECT ALL");
        session.ok("STYLE APPLY Kerb");
        // Kerb: the two points, 1 and 2. Spare: none.
        assert_eq!(
            session.ok("STYLE USAGE"),
            "  Kerb  2 entities\n  Spare  0 entities"
        );
        assert_eq!(
            session.ok("STYLE USAGE Kerb"),
            "style Kerb: used by 2 entities, e.g. id=1"
        );
        assert_eq!(session.ok("STYLE USAGE Spare"), "style Spare: unused");
        assert_eq!(session.fails("STYLE USAGE nosuch"), ErrorCode::NotFound);
    }

    #[test]
    fn style_and_linetype_merge_repoint_every_holder_as_one_undo_step() {
        let mut session = Session::default();
        session.ok("STYLE NEW Kerb");
        session.ok("STYLE NEW \"Kerb 2\"");
        session.ok("POINT 1,1");
        session.ok("SELECT ALL");
        session.ok("STYLE APPLY \"Kerb 2\"");
        let ids = session.document.model().entities.ids();
        assert_eq!(ids.len(), 1);
        assert_eq!(
            session.ok("STYLE MERGE \"Kerb 2\" Kerb"),
            "style Kerb 2 merged into Kerb (1 entity moved)"
        );
        assert_eq!(session.entity(ids[0]).style, "Kerb");
        assert!(!session.document.model().styles.contains("Kerb 2"));
        session.ok("UNDO");
        assert_eq!(session.entity(ids[0]).style, "Kerb 2");
        assert_eq!(
            session.fails("STYLE MERGE Kerb nosuch"),
            ErrorCode::NotFound
        );

        session.ok("LINETYPE NEW fence 1 -0.5");
        session.ok("LINETYPE NEW rail 2 -1");
        session.ok("LAYER NEW survey");
        session.ok("LAYER LTYPE survey rail");
        session.ok("LINETYPE MERGE rail fence");
        let survey_linetype = |session: &Session| {
            session
                .document
                .model()
                .layers
                .find("survey")
                .expect("survey")
                .linetype
                .clone()
        };
        assert_eq!(survey_linetype(&session), "fence");
        assert!(!session.document.model().linetypes.contains("rail"));
        session.ok("UNDO");
        assert_eq!(survey_linetype(&session), "rail");
        assert_eq!(
            session.fails("LINETYPE MERGE continuous fence"),
            ErrorCode::CommandRejected
        );
    }

    #[test]
    fn purge_deletes_what_nothing_uses_as_one_undo_step_and_keeps_the_current_style() {
        let mut session = Session::default();
        session.ok("LINETYPE NEW fence 1 -0.5");
        session.ok("STYLE NEW Post");
        session.ok("STYLE SET Post linetype fence");
        session.ok("STYLE NEW Spare");
        session.ok("STYLE NEW Next");
        session.ok("STYLE CURRENT Next");
        // Post and Spare go, fence with Post; Next is kept, being current.
        assert_eq!(
            session.ok("PURGE STYLES"),
            "purged 2 styles, 0 linetypes and 0 hatch patterns (one UNDO restores them)\n  \
             styles: Post, Spare",
            "STYLES alone purges no linetype, not even one a purged style freed"
        );
        session.ok("UNDO");
        assert!(session.document.model().styles.contains("Post"));
        assert_eq!(
            session.ok("PURGE"),
            "purged 2 styles, 1 linetype and 0 hatch patterns (one UNDO restores them)\n  \
             styles: Post, Spare\n  linetypes: fence"
        );
        assert!(session.document.model().styles.contains("Next"));
        assert_eq!(
            session.ok("PURGE"),
            "nothing to purge: everything is used",
            "and no empty undo step"
        );
        session.ok("UNDO");
        assert!(session.document.model().styles.contains("Post"));
        assert!(session.document.model().linetypes.contains("fence"));
        assert_eq!(
            session.fails("PURGE EVERYTHING"),
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn style_current_sets_what_new_work_is_drawn_in() {
        let mut session = Session::default();
        assert_eq!(session.ok("STYLE CURRENT"), "current style is ByLayer");
        session.ok("STYLE NEW Kerb");
        assert_eq!(session.ok("STYLE CURRENT Kerb"), "current style is Kerb");
        assert_eq!(session.document.current_attributes().style, "Kerb");
        assert_eq!(session.fails("STYLE CURRENT nosuch"), ErrorCode::NotFound);
        assert_eq!(session.ok("STYLE CURRENT -"), "current style is ByLayer");
        assert_eq!(session.ok("STYLE CURRENT Kerb"), "current style is Kerb");
        assert_eq!(
            session.ok("STYLE CURRENT ByLayer"),
            "current style is ByLayer"
        );
    }

    #[test]
    fn setting_a_style_field_to_what_it_already_is_is_no_undo_step() {
        let mut session = Session::default();
        session.ok("STYLE NEW s");
        session.ok("STYLE SET s weight 0.5");
        let steps = session.document.history().undo_count();
        assert_eq!(session.ok("STYLE SET s weight 0.5"), "style s unchanged");
        assert_eq!(session.document.history().undo_count(), steps);
        assert!(!session.document.history().can_redo());
    }

    // ---- alignments -----------------------------------------------------------------------

    #[test]
    fn an_alignment_is_defined_by_its_p_is_and_solved_on_demand() {
        let mut session = Session::default();
        assert!(
            session.ok("ALIGN LIST").is_empty(),
            "nothing is defined in a new document"
        );

        // Three PIs with no curves: a 200 m kinked centreline.
        session.ok("ALIGN NEW road 0,0 100,0 100,100");
        assert_eq!(
            session
                .document
                .model()
                .alignments
                .find("road")
                .expect("road")
                .horizontal
                .pis
                .len(),
            3
        );
        let listed = session.ok("ALIGN LIST");
        assert!(listed.contains("length 200.000"), "{listed}");

        // Round the corner: T = R tan(45 deg) = 50, so the length becomes
        // 100 + 50 pi / 2 = 178.540.
        session.ok("ALIGN SET road 1 50");
        let listed = session.ok("ALIGN LIST");
        assert!(listed.contains("length 178.540"), "{listed}");

        session.ok("ALIGN START road 1000");
        let listed = session.ok("ALIGN LIST");
        assert!(listed.contains("1000.000 to 1178.540"), "{listed}");
    }

    #[test]
    fn alignment_p_is_can_be_appended_with_their_curve_data() {
        let mut session = Session::default();
        session.ok("ALIGN NEW road 0,0 400,0");
        // Extend past the corner with a 30 degree turn, R = 300, 90 m
        // spirals.
        session.ok("ALIGN SET road 1 300 90 90");
        session.ok("ALIGN PI road 659.808,150");
        let road = session
            .document
            .model()
            .alignments
            .find("road")
            .cloned()
            .expect("road");
        assert_eq!(road.horizontal.pis.len(), 3);
        double_eq(road.horizontal.pis[1].radius, 300.0);
        double_eq(road.horizontal.pis[1].spiral_in, 90.0);
        double_eq(road.horizontal.pis[1].spiral_out, 90.0);
    }

    #[test]
    fn a_station_table_includes_every_key_station_not_only_the_interval() {
        // A setting-out table that skipped the TS, SC, CS and ST would be
        // useless in the field: those are the points that get pegged.
        let mut session = Session::default();
        session.ok("ALIGN NEW road 0,0 100,0 100,100");
        session.ok("ALIGN SET road 1 50");
        let table = session.ok("ALIGN STATIONS road 25");
        // TS at 50 (on the interval anyway), ST at 128.540 (not on it).
        assert!(table.contains("128.540"), "{table}");
        assert!(table.contains("178.540"), "{table}");
        // Tangent rows say so; arc rows give the radius.
        assert!(table.contains("straight"), "{table}");
        assert!(table.contains("50.000"), "{table}");
        assert_eq!(
            session.fails("ALIGN STATIONS road 0"),
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn an_alignment_that_cannot_be_built_is_refused_naming_the_pi() {
        let mut session = Session::default();
        session.ok("ALIGN NEW road 0,0 400,0 659.808,150");
        // 500 m spirals on R = 300 use more deflection than a 30 degree
        // corner has.
        let refused = session
            .interpreter
            .run(&mut session.document, "ALIGN SET road 1 300 500 500")
            .expect_err("too much spiral");
        assert_eq!(refused.code, ErrorCode::InvalidGeometry);
        assert!(
            refused.describe().contains("PI 1"),
            "{}",
            refused.describe()
        );
        // The model kept the last good definition.
        double_eq(
            session
                .document
                .model()
                .alignments
                .find("road")
                .expect("road")
                .horizontal
                .pis[1]
                .spiral_in,
            0.0,
        );

        // One PI is a malformed command, and the verb says so before the
        // solver is asked - InvalidArgument from the usage guard, not
        // InvalidGeometry. The solver's own "at least two PIs" refusal is
        // reached through the API and is tested in the geometry suite.
        assert_eq!(
            session.fails("ALIGN NEW lonely 0,0"),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            session.fails("ALIGN SET road 7 50"),
            ErrorCode::InvalidArgument
        );
        assert_eq!(session.fails("ALIGN PI nosuch 1,1"), ErrorCode::NotFound);
    }

    #[test]
    fn alignment_edits_are_undoable() {
        let mut session = Session::default();
        let radius = |session: &Session| {
            session
                .document
                .model()
                .alignments
                .find("road")
                .expect("road")
                .horizontal
                .pis[1]
                .radius
        };
        session.ok("ALIGN NEW road 0,0 100,0 100,100");
        session.ok("ALIGN SET road 1 50");
        double_eq(radius(&session), 50.0);
        session.ok("UNDO");
        double_eq(radius(&session), 0.0);
        session.ok("UNDO");
        assert!(session.document.model().alignments.find("road").is_none());
        session.ok("REDO");
        assert!(session.document.model().alignments.find("road").is_some());
        session.ok("ALIGN DELETE road");
        assert!(session.document.model().alignments.find("road").is_none());
        session.ok("UNDO");
        assert!(session.document.model().alignments.find("road").is_some());
    }

    // ---- design profiles --------------------------------------------------------------

    #[test]
    fn a_design_profile_is_attached_by_pv_is_and_reported_with_its_low_point() {
        let mut session = Session::default();
        session.ok("ALIGN NEW road 0,0 400,0 400,400");
        session.ok("ALIGN SET road 1 50");
        assert!(
            session
                .ok("ALIGN PROFILE road")
                .contains("no design profile")
        );

        // A profile with one PVI cannot be built, so it cannot be grown from
        // nothing: PVI on an undesigned alignment is refused and says what to
        // do.
        assert_eq!(
            session.fails("ALIGN PVI road 0 16"),
            ErrorCode::InvalidState
        );

        // The textbook sag: -3% into +2% over 100 m, low point at 110 @ 13.6.
        session.ok("ALIGN DESIGN road 0,16 100,13,100 300,17");
        let road = session
            .document
            .model()
            .alignments
            .find("road")
            .cloned()
            .expect("road");
        assert_eq!(
            road.vertical.as_ref().map(|profile| profile.pvis.len()),
            Some(3)
        );

        let report = session.ok("ALIGN PROFILE road");
        assert!(report.contains("low point"), "{report}");
        assert!(report.contains("110.000"), "{report}");
        assert!(report.contains("13.600"), "{report}");
        assert!(report.contains("curve"), "{report}");
    }

    #[test]
    fn a_profile_that_cannot_be_built_is_refused_naming_the_pvi_and_keeps_the_last_good_one() {
        let mut session = Session::default();
        let pvis = |session: &Session| {
            session
                .document
                .model()
                .alignments
                .find("road")
                .and_then(|road| road.vertical.as_ref())
                .map_or(0, |profile| profile.pvis.len())
        };
        session.ok("ALIGN NEW road 0,0 400,0 400,400");
        session.ok("ALIGN DESIGN road 0,16 100,13,100 300,17");
        // A PVI behind the last one: stations must increase.
        let refused = session
            .interpreter
            .run(&mut session.document, "ALIGN PVI road 50 20")
            .expect_err("behind the last");
        assert_eq!(refused.code, ErrorCode::InvalidGeometry);
        assert!(
            refused.describe().contains("PVI 3"),
            "{}",
            refused.describe()
        );
        assert_eq!(pvis(&session), 3);
        assert_eq!(
            session.fails("ALIGN PVI road 400"),
            ErrorCode::InvalidArgument
        ); // no elevation
        // DESIGN with a single PVI is a malformed command, and a curve on the
        // last PVI is a definition that cannot be built - refused by the
        // model, naming it.
        assert_eq!(
            session.fails("ALIGN DESIGN road 0,16"),
            ErrorCode::InvalidArgument
        );
        let at_end = session
            .interpreter
            .run(&mut session.document, "ALIGN DESIGN road 0,16 300,17,50")
            .expect_err("a curve at the end");
        assert_eq!(at_end.code, ErrorCode::InvalidGeometry);
        assert!(at_end.describe().contains("PVI 1"), "{}", at_end.describe());
        // Appending works once a profile exists.
        session.ok("ALIGN PVI road 400 18");
        assert_eq!(pvis(&session), 4);
    }

    #[test]
    fn clearing_a_profile_is_undoable() {
        let mut session = Session::default();
        let profile = |session: &Session| {
            session
                .document
                .model()
                .alignments
                .find("road")
                .and_then(|road| road.vertical.as_ref())
                .map(|profile| profile.pvis.len())
        };
        session.ok("ALIGN NEW road 0,0 400,0 400,400");
        session.ok("ALIGN DESIGN road 0,16 300,17");
        assert!(profile(&session).is_some());
        session.ok("ALIGN CLEARPROFILE road");
        assert!(profile(&session).is_none());
        session.ok("UNDO");
        assert_eq!(profile(&session), Some(2));
    }

    // ---- parcels ----------------------------------------------------------------------

    // The id of the only entity in the document, for verbs that take one.
    fn only_entity_id(document: &Document) -> EntityId {
        let ids = document.model().entities.ids();
        assert_eq!(ids.len(), 1);
        ids.first().copied().unwrap_or_default()
    }

    fn count(document: &Document) -> usize {
        let mut count = 0;
        document.model().entities.for_each(|_| count += 1);
        count
    }

    #[test]
    fn a_parcel_is_reported_labelled_and_described_from_a_closed_polyline() {
        let mut session = Session::default();
        session.ok("PLINE 0,0 100,0 100,50 0,50 CLOSE");
        let id = only_entity_id(&session.document);

        let report = session.ok(&format!("PARCEL {id}"));
        assert!(report.contains("5000.000"), "{report}");
        assert!(report.contains("N 90"), "{report}");
        assert!(report.contains("counter-clockwise"), "{report}");

        let legal = session.ok(&format!("PARCEL {id} LEGAL Lot7"));
        assert!(legal.contains("Lot7"), "{legal}");
        assert!(legal.contains("thence"), "{legal}");
        assert!(legal.contains("0.5000 ha"), "{legal}");

        // Four course labels and the area, as ordinary text entities, in ONE
        // undo step.
        let before = count(&session.document);
        session.ok(&format!("PARCEL {id} LABEL 2"));
        assert_eq!(count(&session.document), before + 5);
        session.ok("UNDO");
        assert_eq!(
            count(&session.document),
            before,
            "the five labels are one undo step"
        );
    }

    #[test]
    fn parcel_refuses_an_open_line_and_a_missing_id() {
        let mut session = Session::default();
        session.ok("LINE 0,0 10,0");
        let id = only_entity_id(&session.document);
        assert_eq!(
            session.fails(&format!("PARCEL {id}")),
            ErrorCode::InvalidGeometry
        );
        assert_eq!(session.fails("PARCEL 999999"), ErrorCode::NotFound);
        assert_eq!(session.fails("PARCEL"), ErrorCode::InvalidArgument);
    }
}

// ---- import placement ------------------------------------------------------------------

mod import_placement {
    use super::*;
    use katana_cad::import_placement::{
        parse_placement_word, placement_word, resolve_import_shift,
    };

    fn placement(mode: ImportPlacementMode, offset: Vec2) -> ImportPlacement {
        ImportPlacement { mode, offset }
    }

    #[test]
    fn a_word_and_the_placement_it_names_read_back_as_each_other() {
        assert_eq!(placement_word(&ImportPlacement::default()), "");
        assert_eq!(
            placement_word(&placement(ImportPlacementMode::Local, Vec2::default())),
            "LOCAL"
        );
        assert_eq!(
            placement_word(&placement(ImportPlacementMode::Alongside, Vec2::default())),
            "ALONGSIDE"
        );
        assert_eq!(
            placement_word(&placement(
                ImportPlacementMode::Offset,
                Vec2::new(10.0, -20.5)
            )),
            "OFFSET=10,-20.5"
        );
        // Each offset written so it reads back exactly: survey coordinates
        // keep their millimetres, and a tenth is not 0.1000000000000000055.
        for written in [
            placement(ImportPlacementMode::Local, Vec2::default()),
            placement(ImportPlacementMode::Alongside, Vec2::default()),
            placement(ImportPlacementMode::Offset, Vec2::new(0.1, -7_410_850.125)),
            placement(ImportPlacementMode::Offset, Vec2::new(1e-7, 0.0)),
        ] {
            let read = parse_placement_word(&placement_word(&written)).expect("a placement");
            assert_eq!(read, Some(written), "{}", placement_word(&written));
        }
        let not_one = parse_placement_word("site.dxf").expect("no placement");
        assert!(not_one.is_none());
    }

    #[test]
    fn local_puts_the_lower_left_corner_at_the_origin() {
        // samples/gis/parcels.geojson spans (180, 0) to (365, 165).
        let parcels = Box2::new(Point2::new(180.0, 0.0), Point2::new(365.0, 165.0));
        let local = resolve_import_shift(
            &placement(ImportPlacementMode::Local, Vec2::default()),
            &Box2::default(),
            &parcels,
        );
        assert_eq!(local.shift, Some(Vec2::new(180.0, 0.0))); // subtracted: (0,0) to (185,165)
        assert_eq!(
            local.said,
            "LOCAL: moved as one piece by -180.000,0.000, so its lower-left corner sits at 0,0."
        );
    }

    #[test]
    fn alongside_puts_the_corner_on_the_drawings_and_into_an_empty_drawing_moves_nothing() {
        let parcels = Box2::new(Point2::new(180.0, 0.0), Point2::new(365.0, 165.0));
        let drawing = Box2::new(Point2::new(1000.0, 2000.0), Point2::new(1010.0, 2005.0));
        let along = resolve_import_shift(
            &placement(ImportPlacementMode::Alongside, Vec2::default()),
            &drawing,
            &parcels,
        );
        // Subtracted from (180, 0) it leaves the drawing's corner, (1000,
        // 2000).
        assert_eq!(along.shift, Some(Vec2::new(-820.0, -2000.0)));
        assert_eq!(
            along.said,
            "ALONGSIDE: moved as one piece by 820.000,2000.000, so its lower-left corner sits on \
             the drawing's, at 1000.000,2000.000."
        );

        let empty = resolve_import_shift(
            &placement(ImportPlacementMode::Alongside, Vec2::default()),
            &Box2::default(),
            &parcels,
        );
        assert!(empty.shift.is_none());
        assert!(
            empty.said.contains("the drawing is empty"),
            "{}",
            empty.said
        );
    }

    #[test]
    fn an_offset_moves_by_what_it_says_and_keep_or_no_data_moves_nothing() {
        let parcels = Box2::new(Point2::new(180.0, 0.0), Point2::new(365.0, 165.0));
        let moved = resolve_import_shift(
            &placement(ImportPlacementMode::Offset, Vec2::new(10.0, -20.5)),
            &Box2::default(),
            &parcels,
        );
        assert_eq!(moved.shift, Some(Vec2::new(-10.0, 20.5)));
        assert_eq!(moved.said, "OFFSET: moved as one piece by 10.000,-20.500.");

        let kept = resolve_import_shift(&ImportPlacement::default(), &Box2::default(), &parcels);
        assert!(kept.shift.is_none());
        assert!(kept.said.is_empty());
        // A file holding nothing with a position has nothing to move, whatever
        // was asked; said, so an empty import is not taken for a moved one.
        let nothing = resolve_import_shift(
            &placement(ImportPlacementMode::Local, Vec2::default()),
            &Box2::default(),
            &Box2::default(),
        );
        assert!(nothing.shift.is_none());
        assert!(nothing.said.contains("nothing to move"), "{}", nothing.said);
    }
}
