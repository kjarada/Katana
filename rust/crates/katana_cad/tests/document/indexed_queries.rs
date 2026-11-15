// The spatial index must not change an answer (PLAN.MD Phase 18, Rule 6;
// tests/cad/test_indexed_queries.cpp).
//
// An index is an optimisation, and PLAN.MD's rule for an optimisation is that
// it may make things faster and may not make them different. Every test here
// runs the same query twice - once scanning, once through the index - and
// requires the results to be equal. If they ever diverge, the index is wrong,
// not the scan.
//
// The C++ draws a call's arguments from its generator in an order the
// language leaves open; GCC evaluates them right to left, so a point
// `Point2(position(random), position(random))` draws its y first, and so does
// every point here.

use crate::{Mt19937, double_eq};
use katana_cad::selection::{pick_entity, pick_in_box};
use katana_cad::snapping::{DEFAULT_SNAP_MODES, snap};
use katana_cad::spatial_query::query_extents;
use katana_cad::{
    BoxSelectionMode, Document, LayerOverrides, SelectionFilter, SnapMode, SnapModes, SnapRequest,
};
use katana_commands::{
    create_arc, create_circle, create_dimension, create_layer, create_line, create_point,
    create_polyline, delete_entities, move_entities, set_entity_visible,
};
use katana_entity::entity::INVALID_ENTITY_ID;
use katana_entity::entity_geometry::bounding_box;
use katana_entity::{DimensionGeometry, EntityId, Layer};
use katana_geometry::{Arc2, Box2, Point2, Polyline2, Vec2};

/// A drawing with every geometry kind, scattered and overlapping, so the
/// comparison is not over one easy shape. Fixed seed: a failure repeats.
/// `layers`, when given, deals the entities round those layers in turn (they
/// must exist); the geometry drawn is the same either way.
fn populate(document: &mut Document, count: i32, layers: &[&str]) {
    let mut random = Mt19937::new(20260920);
    for i in 0..count {
        let x = random.real(-200.0, 200.0);
        let y = random.real(-200.0, 200.0);
        let mut attributes = document.current_attributes();
        if !layers.is_empty() {
            attributes.layer = String::from(layers[i as usize % layers.len()]);
        }
        let status = match i % 5 {
            0 => {
                let dy = random.real(0.5, 12.0);
                let dx = random.real(0.5, 12.0);
                document.execute(create_line(
                    Point2::new(x, y),
                    Point2::new(x + dx, y + dy),
                    attributes,
                ))
            }
            1 => {
                let radius = random.real(0.5, 12.0);
                document.execute(create_circle(Point2::new(x, y), radius, attributes))
            }
            2 => {
                // A shallow arc: its centre is FAR outside its own bounding
                // box, which is exactly the case that breaks a naive index.
                let arc = Arc2::new(Point2::new(x, y), random.real(0.5, 12.0) * 8.0, 0.0, 0.25);
                document.execute(create_arc(arc, attributes))
            }
            3 => {
                let vertices = (0..5)
                    .map(|v| Point2::new(x + f64::from(v) * 1.3, y + f64::from(v + i).sin() * 2.0))
                    .collect();
                document.execute(create_polyline(Polyline2::new(vertices, false), attributes))
            }
            _ => document.execute(create_point(Point2::new(x, y), attributes)),
        };
        assert!(status.is_ok(), "{status:?}");
    }
}

fn request_at<'a>(cursor: Point2) -> SnapRequest<'a> {
    SnapRequest {
        cursor,
        aperture: 3.0,
        modes: DEFAULT_SNAP_MODES,
        ..SnapRequest::default()
    }
}

/// Point2(position(random), position(random)), y drawn first.
fn point_from(random: &mut Mt19937, low: f64, high: f64) -> Point2 {
    let y = random.real(low, high);
    let x = random.real(low, high);
    Point2::new(x, y)
}

fn everything<'a>() -> SelectionFilter<'a> {
    SelectionFilter::default()
}

// ---- a per-view layer override on both paths (PLAN.MD 47) ------------------------------

/// Nested layers, with siblings that share a TEXT prefix ("roads 2",
/// "roadside") so the whole-segment rule decides some of the hides.
const VIEW_LAYERS: [&str; 9] = [
    "0",
    "roads",
    "roads/kerb",
    "roads/kerb/top",
    "roads 2",
    "roadside",
    "survey/points",
    "survey/strings",
    "design/surface/tin1",
];

/// What a random view may hide: layers, pure ancestors ("survey", "design",
/// "design/surface") and a name no layer has.
const HIDEABLE: [&str; 10] = [
    "roads",
    "roads/kerb",
    "roads/kerb/top",
    "roads 2",
    "roadside",
    "survey",
    "survey/points",
    "design",
    "design/surface",
    "no/such/layer",
];

fn create_view_layers(document: &mut Document) {
    for name in VIEW_LAYERS {
        if !document.model().layers.contains(name) {
            let layer = Layer {
                name: String::from(name),
                ..Layer::default()
            };
            assert!(document.execute(create_layer(layer)).is_ok(), "{name}");
        }
    }
}

/// One to three hides, never the whole drawing: the property is about a view
/// that shows some things and not others.
fn random_view(random: &mut Mt19937) -> LayerOverrides {
    let mut view = LayerOverrides::default();
    let how_many = 1 + random.below(3);
    for _ in 0..how_many {
        view.hide(HIDEABLE[random.below(HIDEABLE.len() as u32) as usize]);
    }
    view
}

fn hidden_in(document: &Document, view: &LayerOverrides, id: EntityId) -> bool {
    document
        .model()
        .entities
        .find(id)
        .is_some_and(|entity| view.hides(&entity.layer))
}

mod indexed_queries {
    use super::*;

    #[test]
    fn the_index_tracks_the_document_as_it_is_drawn() {
        let mut document = Document::default();
        assert_eq!(document.spatial_index().size(), 0);

        populate(&mut document, 40, &[]);
        // Points have no extent but are still indexed; every entity should be
        // in.
        assert_eq!(
            document.spatial_index().size(),
            document.model().entities.size()
        );
    }

    #[test]
    fn picking_gives_the_same_answer_with_and_without_the_index() {
        let mut document = Document::default();
        populate(&mut document, 300, &[]);

        let mut random = Mt19937::new(5);
        let mut hits = 0;
        for trial in 0..600 {
            let point = point_from(&mut random, -220.0, 220.0);
            for tolerance in [0.25, 2.0, 15.0] {
                let scanned = pick_entity(document.model(), point, tolerance, &everything(), None);
                let indexed = pick_entity(
                    document.model(),
                    point,
                    tolerance,
                    &everything(),
                    Some(document.spatial_index()),
                );
                assert_eq!(scanned, indexed, "trial {trial} tolerance {tolerance}");
                if scanned.is_some() {
                    hits += 1;
                }
            }
        }
        // A comparison that never picked anything would pass while indexing
        // nothing at all.
        assert!(hits > 100, "the trials must actually hit entities");
    }

    #[test]
    fn snapping_gives_the_same_answer_with_and_without_the_index() {
        let mut document = Document::default();
        populate(&mut document, 250, &[]);

        let mut random = Mt19937::new(9);
        let mut hits = 0;
        for trial in 0..500 {
            let request = request_at(point_from(&mut random, -220.0, 220.0));
            let scanned = snap(document.model(), &request, None);
            let indexed = snap(document.model(), &request, Some(document.spatial_index()));

            assert_eq!(scanned.is_some(), indexed.is_some(), "trial {trial}");
            let (Some(scanned), Some(indexed)) = (scanned, indexed) else {
                continue;
            };
            hits += 1;
            assert_eq!(scanned.mode, indexed.mode, "trial {trial}");
            assert_eq!(scanned.entity, indexed.entity, "trial {trial}");
            double_eq(scanned.point.x, indexed.point.x);
            double_eq(scanned.point.y, indexed.point.y);
        }
        assert!(hits > 50, "the trials must actually snap to something");
    }

    #[test]
    fn an_arc_centre_is_still_snappable_through_the_index() {
        // THE case the indexed box has to be wider than the bounding box for.
        // A shallow arc's centre is a long way outside the arc's own extent,
        // so an index built on plain bounding boxes rejects the arc before the
        // centre snap is ever considered, and centre snap silently stops
        // working.
        let mut document = Document::default();
        let arc = Arc2::new(Point2::new(0.0, 0.0), 100.0, 0.0, 0.2); // a sliver far from its centre
        let attributes = document.current_attributes();
        assert!(document.execute(create_arc(arc, attributes)).is_ok());

        let request = SnapRequest {
            cursor: Point2::new(0.0, 0.0), // the centre
            aperture: 1.0,
            modes: SnapMode::Center as SnapModes,
            ..SnapRequest::default()
        };

        let scanned = snap(document.model(), &request, None);
        let indexed = snap(document.model(), &request, Some(document.spatial_index()));

        let scanned = scanned.expect("the scan must find the arc centre");
        let indexed = indexed.expect("so must the index");
        assert_eq!(scanned.mode, indexed.mode);
        assert_eq!(scanned.entity, indexed.entity);
    }

    #[test]
    fn clicking_an_arc_centre_still_does_not_select_the_arc() {
        // The other half of the same decision: the indexed box is widened for
        // snapping, and picking must NOT inherit that generosity.
        let mut document = Document::default();
        let arc = Arc2::new(Point2::new(0.0, 0.0), 100.0, 0.0, 0.2);
        let attributes = document.current_attributes();
        assert!(document.execute(create_arc(arc, attributes)).is_ok());

        let origin = Point2::new(0.0, 0.0);
        assert!(pick_entity(document.model(), origin, 1.0, &everything(), None).is_none());
        assert!(
            pick_entity(
                document.model(),
                origin,
                1.0,
                &everything(),
                Some(document.spatial_index())
            )
            .is_none()
        );
    }

    #[test]
    fn box_selection_gives_the_same_answer_with_and_without_the_index() {
        let mut document = Document::default();
        populate(&mut document, 300, &[]);

        let mut random = Mt19937::new(13);
        let mut non_empty = 0;
        for trial in 0..300 {
            let x = random.real(-220.0, 220.0);
            let y = random.real(-220.0, 220.0);
            let height = random.real(1.0, 90.0);
            let width = random.real(1.0, 90.0);
            let window = Box2::new(Point2::new(x, y), Point2::new(x + width, y + height));
            for mode in [BoxSelectionMode::Window, BoxSelectionMode::Crossing] {
                let scanned = pick_in_box(document.model(), window, mode, &everything(), None);
                let indexed = pick_in_box(
                    document.model(),
                    window,
                    mode,
                    &everything(),
                    Some(document.spatial_index()),
                );
                assert_eq!(scanned, indexed, "trial {trial}");
                if !scanned.is_empty() {
                    non_empty += 1;
                }
            }
        }
        assert!(
            non_empty > 100,
            "the windows must actually contain entities"
        );
    }

    #[test]
    fn the_index_survives_editing_moving_and_deleting() {
        let mut document = Document::default();
        populate(&mut document, 120, &[]);

        let mut random = Mt19937::new(21);
        for round in 0..25 {
            let ids = document.model().entities.ids();
            assert!(!ids.is_empty());

            // Move a few.
            let moving: Vec<EntityId> = ids.iter().copied().take(5).collect();
            let dy = random.real(-200.0, 200.0) * 0.1;
            let dx = random.real(-200.0, 200.0) * 0.1;
            assert!(
                document
                    .execute(move_entities(moving, Vec2::new(dx, dy)))
                    .is_ok()
            );

            // Delete one.
            if ids.len() > 10 {
                assert!(
                    document
                        .execute(delete_entities(vec![ids[ids.len() / 2]]))
                        .is_ok()
                );
            }

            // Draw one: the second end drawn before the first.
            let end = point_from(&mut random, -200.0, 200.0);
            let start = point_from(&mut random, -200.0, 200.0);
            let attributes = document.current_attributes();
            assert!(
                document
                    .execute(create_line(start, end, attributes))
                    .is_ok()
            );

            assert_eq!(
                document.spatial_index().size(),
                document.model().entities.size(),
                "round {round}"
            );

            // And the answers still agree.
            for trial in 0..30 {
                let point = point_from(&mut random, -200.0, 200.0);
                assert_eq!(
                    pick_entity(document.model(), point, 5.0, &everything(), None),
                    pick_entity(
                        document.model(),
                        point,
                        5.0,
                        &everything(),
                        Some(document.spatial_index())
                    ),
                    "round {round} trial {trial}"
                );
            }
        }
    }

    #[test]
    fn the_index_survives_undo_and_redo() {
        // Undo restores entities through the same command machinery, so the
        // index has to be put back with them. A stale index here is the
        // classic way an optimisation silently corrupts a session.
        let mut document = Document::default();
        populate(&mut document, 60, &[]);

        let before = document.model().entities.ids();
        assert!(before.len() >= 10);

        let doomed: Vec<EntityId> = before.iter().copied().take(8).collect();
        assert!(document.execute(delete_entities(doomed)).is_ok());
        assert_eq!(
            document.spatial_index().size(),
            document.model().entities.size()
        );

        assert!(document.undo().is_ok());
        assert_eq!(
            document.spatial_index().size(),
            document.model().entities.size(),
            "undo must restore the index with the entities"
        );
        assert_eq!(document.model().entities.ids(), before);

        assert!(document.redo().is_ok());
        assert_eq!(
            document.spatial_index().size(),
            document.model().entities.size()
        );

        assert!(document.undo().is_ok());
        // The real assertion: queries agree again after the round trip.
        let mut random = Mt19937::new(3);
        for trial in 0..200 {
            let point = point_from(&mut random, -200.0, 200.0);
            assert_eq!(
                pick_entity(document.model(), point, 6.0, &everything(), None),
                pick_entity(
                    document.model(),
                    point,
                    6.0,
                    &everything(),
                    Some(document.spatial_index())
                ),
                "trial {trial}"
            );
        }
    }

    #[test]
    fn starting_a_new_document_empties_the_index() {
        let mut document = Document::default();
        populate(&mut document, 50, &[]);
        assert!(document.spatial_index().size() > 0);

        document.new_document();
        assert_eq!(
            document.spatial_index().size(),
            0,
            "a stale index would make the new drawing pick entities that are gone"
        );
        assert!(
            pick_entity(
                document.model(),
                Point2::new(0.0, 0.0),
                1e6,
                &everything(),
                Some(document.spatial_index())
            )
            .is_none()
        );
    }

    #[test]
    fn hidden_and_locked_entities_are_filtered_the_same_way_on_both_paths() {
        // The index knows nothing about visibility; the exact test does. Both
        // paths must therefore still exclude the same entities.
        let mut document = Document::default();
        populate(&mut document, 40, &[]);
        let ids = document.model().entities.ids();
        assert!(ids.len() >= 10);

        let hidden: Vec<EntityId> = ids.iter().copied().take(10).collect();
        assert!(
            document
                .execute(set_entity_visible(hidden.clone(), false))
                .is_ok()
        );

        let mut random = Mt19937::new(17);
        for trial in 0..300 {
            let point = point_from(&mut random, -200.0, 200.0);
            let scanned = pick_entity(document.model(), point, 8.0, &everything(), None);
            let indexed = pick_entity(
                document.model(),
                point,
                8.0,
                &everything(),
                Some(document.spatial_index()),
            );
            assert_eq!(scanned, indexed, "trial {trial}");
            if let Some(scanned) = scanned {
                assert!(!hidden.contains(&scanned), "a hidden entity was picked");
            }
        }
    }

    #[test]
    fn both_sides_of_the_scan_crossover_give_the_same_answer() {
        // for_each_candidate uses the index for a narrow query and falls back
        // to the ordered scan for a wide one, because asking an index for
        // everything costs more than walking the model once (spatial_query).
        // The threshold changes WHICH path runs, so the answers on either side
        // of it have to be identical or the drawing would change as the user
        // zoomed.
        let mut document = Document::default();
        populate(&mut document, 400, &[]);

        let extent = document.spatial_index().bounds();
        assert!(!extent.empty());
        let middle = extent.center();

        // Window half-widths sweeping from far below the threshold to far
        // above, so both paths are exercised and the boundary is crossed.
        for share in [0.001, 0.01, 0.1, 0.3, 0.34, 0.35, 0.36, 0.5, 0.9, 1.0, 4.0] {
            let half = 0.5 * extent.width() * f64::sqrt(share);
            let window = Box2::new(
                Point2::new(middle.x - half, middle.y - half),
                Point2::new(middle.x + half, middle.y + half),
            );

            for mode in [BoxSelectionMode::Window, BoxSelectionMode::Crossing] {
                assert_eq!(
                    pick_in_box(document.model(), window, mode, &everything(), None),
                    pick_in_box(
                        document.model(),
                        window,
                        mode,
                        &everything(),
                        Some(document.spatial_index())
                    ),
                    "area share {share}"
                );
            }
            let tolerance = half;
            assert_eq!(
                pick_entity(document.model(), middle, tolerance, &everything(), None),
                pick_entity(
                    document.model(),
                    middle,
                    tolerance,
                    &everything(),
                    Some(document.spatial_index())
                ),
                "area share {share}"
            );
        }
    }

    #[test]
    fn a_query_entirely_off_the_drawing_is_empty_on_both_paths() {
        let mut document = Document::default();
        populate(&mut document, 100, &[]);

        let elsewhere = Point2::new(1.0e6, 1.0e6);
        assert!(pick_entity(document.model(), elsewhere, 1.0, &everything(), None).is_none());
        assert!(
            pick_entity(
                document.model(),
                elsewhere,
                1.0,
                &everything(),
                Some(document.spatial_index())
            )
            .is_none()
        );

        let far = Box2::new(
            Point2::new(1.0e6, 1.0e6),
            Point2::new(1.0e6 + 50.0, 1.0e6 + 50.0),
        );
        assert!(
            pick_in_box(
                document.model(),
                far,
                BoxSelectionMode::Crossing,
                &everything(),
                None
            )
            .is_empty()
        );
        assert!(
            pick_in_box(
                document.model(),
                far,
                BoxSelectionMode::Crossing,
                &everything(),
                Some(document.spatial_index())
            )
            .is_empty()
        );
    }

    #[test]
    fn a_dimension_is_found_by_its_label_not_only_by_its_measured_line() {
        // The broad phase culls against what a dimension DRAWS. Its label,
        // arrows and extension overshoot sit outside the measured points, so
        // culling against the plain bounding box drops a dimension whose text
        // is still on screen - a glitch at the edge of the view that looks
        // like a redraw fault.
        let mut document = Document::default();

        let dimension = DimensionGeometry {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(10.0, 0.0),
            offset: 2.0,
            ..DimensionGeometry::default()
        };
        let attributes = document.current_attributes();
        assert!(
            document
                .execute(create_dimension(dimension, attributes))
                .is_ok()
        );

        let id = *document
            .model()
            .entities
            .ids()
            .last()
            .expect("the dimension");
        let entity = document.model().entities.find(id).expect("the dimension");
        let plain = bounding_box(&entity.geometry);
        let indexed = query_extents(document.model(), entity);

        assert!(
            indexed.max.y > plain.max.y,
            "the drawn extent must exceed the measured one, or this test proves nothing"
        );

        // A window that touches only the drawn part - above the dimension
        // line, where the label sits - must still find it, on both paths.
        let label_only = Box2::new(
            Point2::new(4.0, plain.max.y + 0.05),
            Point2::new(6.0, indexed.max.y - 0.01),
        );
        assert!(!label_only.empty());

        let scanned = pick_in_box(
            document.model(),
            label_only,
            BoxSelectionMode::Crossing,
            &everything(),
            None,
        );
        let via_index = pick_in_box(
            document.model(),
            label_only,
            BoxSelectionMode::Crossing,
            &everything(),
            Some(document.spatial_index()),
        );
        assert_eq!(
            scanned, via_index,
            "both paths must agree about a dimension's extent"
        );
    }

    #[test]
    fn a_per_view_override_filters_the_same_entities_on_both_paths() {
        // The index knows nothing about layers; the per-view test runs in the
        // exact phase. Both paths must therefore drop exactly the same
        // entities, and neither may ever return one the view hides.
        //
        // The "changed" counts are the cases where the override actually
        // altered the answer - the document rule alone would have returned
        // something the view hides. Without enough of those, the comparison
        // would also pass with the view ignored on both paths.
        let mut document = Document::default();
        create_view_layers(&mut document);
        populate(&mut document, 400, &VIEW_LAYERS);

        let mut random = Mt19937::new(47);
        let index = Some(document.spatial_index());
        let model = document.model();

        let mut picks_changed = 0;
        let mut windows_changed = 0;
        let mut snaps_changed = 0;
        for trial in 0..400 {
            let view = random_view(&mut random);
            let in_view = SelectionFilter {
                view: Some(&view),
                ..SelectionFilter::default()
            };

            // Picking.
            let point = point_from(&mut random, -220.0, 220.0);
            for tolerance in [2.0, 15.0] {
                let scanned = pick_entity(model, point, tolerance, &in_view, None);
                let indexed = pick_entity(model, point, tolerance, &in_view, index);
                assert_eq!(scanned, indexed, "trial {trial} tolerance {tolerance}");
                if let Some(scanned) = scanned {
                    assert!(!hidden_in(&document, &view, scanned), "trial {trial}");
                }
                let unfiltered = pick_entity(model, point, tolerance, &everything(), index);
                if unfiltered.is_some_and(|id| hidden_in(&document, &view, id)) {
                    picks_changed += 1;
                }
            }

            // Box selection.
            let x = random.real(-220.0, 220.0);
            let y = random.real(-220.0, 220.0);
            let height = random.real(1.0, 90.0);
            let width = random.real(1.0, 90.0);
            let window = Box2::new(Point2::new(x, y), Point2::new(x + width, y + height));
            for mode in [BoxSelectionMode::Window, BoxSelectionMode::Crossing] {
                let scanned = pick_in_box(model, window, mode, &in_view, None);
                let indexed = pick_in_box(model, window, mode, &in_view, index);
                assert_eq!(scanned, indexed, "trial {trial}");
                for &id in &scanned {
                    assert!(!hidden_in(&document, &view, id), "trial {trial}");
                }
                let unfiltered = pick_in_box(model, window, mode, &everything(), index);
                if unfiltered.iter().any(|&id| hidden_in(&document, &view, id)) {
                    windows_changed += 1;
                }
            }

            // Snapping, at the default aperture and at a wide one: a random
            // cursor at 3 units snaps too rarely on its own to exercise the
            // view.
            let cursor = point_from(&mut random, -220.0, 220.0);
            for aperture in [3.0, 10.0] {
                let mut request = request_at(cursor);
                request.aperture = aperture;
                request.view = Some(&view);
                let scanned = snap(model, &request, None);
                let indexed = snap(model, &request, index);
                assert_eq!(scanned.is_some(), indexed.is_some(), "trial {trial}");
                if let (Some(scanned), Some(indexed)) = (scanned, indexed) {
                    assert_eq!(scanned.mode, indexed.mode, "trial {trial}");
                    assert_eq!(scanned.entity, indexed.entity, "trial {trial}");
                    double_eq(scanned.point.x, indexed.point.x);
                    double_eq(scanned.point.y, indexed.point.y);
                    if scanned.entity != INVALID_ENTITY_ID {
                        assert!(
                            !hidden_in(&document, &view, scanned.entity),
                            "trial {trial}"
                        );
                    }
                }
                request.view = None;
                let unfiltered = snap(model, &request, index);
                if unfiltered.is_some_and(|found| {
                    found.entity != INVALID_ENTITY_ID && hidden_in(&document, &view, found.entity)
                }) {
                    snaps_changed += 1;
                }
            }
        }

        // Floors, not expectations. With these seeds the C++ run reaches 94
        // changed picks, 373 windows and 76 snaps out of 800 of each; a floor
        // of about half leaves room for a change to the generator while still
        // failing a run that stopped reaching the case.
        assert!(picks_changed > 45, "picks the override actually changed");
        assert!(
            windows_changed > 180,
            "windows the override actually changed"
        );
        assert!(snaps_changed > 35, "snaps the override actually changed");
    }
}
