//! tests/geo/test_surface_input.cpp: what a surface built from the drawing is
//! built from (katana_cad::geo::surface_input; docs/terrain.md "Surfaces on
//! every front end"): the rules the window's Surface From Drawing kept, moved
//! to katana_cad so katana_cli, katana_mcp and the window share them, with
//! the one change - an entity with no height is left out, not put on the
//! datum.

mod surface_input {
    use katana_cad::geo::surface_input::surface_input;
    use katana_cad::selection::is_drawn;
    use katana_cad::{CommandInterpreter, Document, NO_LAYER_OVERRIDES};
    use katana_entity::entity::{ELEVATION_PROPERTY, ELEVATIONS_PROPERTY, heights_of, set_heights};
    use katana_entity::{Entity, EntityId, Geometry, Model, PointGeometry};
    use katana_geometry::{Point2, Point3, Polyline2, Segment2};
    use katana_terrain::{Breakline, TinInput};

    use crate::{Scratch, copy_tree, draw};

    fn point(x: f64, y: f64, z: Option<f64>) -> Entity {
        let mut entity = Entity {
            geometry: Geometry::Point(PointGeometry {
                position: Point2::new(x, y),
            }),
            ..Entity::default()
        };
        set_heights(&mut entity.properties, &[z]);
        entity
    }

    fn polyline(vertices: Vec<Point2>, closed: bool, heights: &[Option<f64>]) -> Entity {
        let mut entity = Entity {
            geometry: Geometry::Polyline(Polyline2 { vertices, closed }),
            ..Entity::default()
        };
        set_heights(&mut entity.properties, heights);
        entity
    }

    fn p(x: f64, y: f64) -> Point2 {
        Point2::new(x, y)
    }

    // The window's Surface From Drawing as it was before the rules moved
    // (src/katana_qt/main_window.cpp, 2026-09-26), kept verbatim as the
    // oracle: what the moved rules must give wherever the entities carry
    // heights. It read every drawn entity; an entity with no height property
    // at all went to the datum.
    fn window_rules(model: &Model) -> TinInput {
        let mut input = TinInput::default();
        model.entities.for_each(|entity| {
            if !is_drawn(model, entity, &NO_LAYER_OVERRIDES) {
                return;
            }
            let carries_heights = entity.properties.contains_key(ELEVATION_PROPERTY)
                || entity.properties.contains_key(ELEVATIONS_PROPERTY);
            let height_at = |heights: &[Option<f64>], index: usize| -> Option<f64> {
                if !carries_heights {
                    return Some(0.0);
                }
                heights[index]
            };
            match &entity.geometry {
                Geometry::Point(at) => {
                    let heights = heights_of(&entity.properties, 1);
                    if let Some(z) = height_at(&heights, 0) {
                        input
                            .points
                            .push(Point3::new(at.position.x, at.position.y, z));
                    }
                }
                Geometry::Polyline(line) => {
                    let heights = heights_of(&entity.properties, line.vertices.len());
                    let mut breakline = Breakline::default();
                    let flush = |breakline: &mut Breakline, input: &mut TinInput| {
                        if breakline.vertices.len() >= 2 {
                            input.breaklines.push(breakline.clone());
                        }
                        breakline.vertices.clear();
                    };
                    let mut whole = true;
                    for (i, vertex) in line.vertices.iter().enumerate() {
                        let Some(z) = height_at(&heights, i) else {
                            whole = false;
                            flush(&mut breakline, &mut input);
                            continue;
                        };
                        breakline.vertices.push(Point3::new(vertex.x, vertex.y, z));
                        input.points.push(Point3::new(vertex.x, vertex.y, z));
                    }
                    breakline.closed = line.closed && whole;
                    flush(&mut breakline, &mut input);
                }
                _ => {}
            }
        });
        input
    }

    fn every_id(model: &Model) -> Vec<EntityId> {
        let mut ids = Vec::new();
        model.entities.for_each(|entity| ids.push(entity.id));
        ids
    }

    #[track_caller]
    fn expect_same_points(a: &[Point3], b: &[Point3]) {
        assert_eq!(a.len(), b.len());
        for (i, (left, right)) in a.iter().zip(b).enumerate() {
            assert_eq!(left.x, right.x, "{i}");
            assert_eq!(left.y, right.y, "{i}");
            assert_eq!(left.z, right.z, "{i}");
        }
    }

    #[test]
    fn four_levelled_points_are_four_survey_points_and_no_breakline() {
        let mut document = Document::default();
        let ids = draw(
            &mut document,
            vec![
                point(0.0, 0.0, Some(100.0)),
                point(10.0, 0.0, Some(101.0)),
                point(10.0, 10.0, Some(102.0)),
                point(0.0, 10.0, Some(101.0)),
            ],
        );
        let input = surface_input(document.model(), &ids);
        assert_eq!(input.input.points.len(), 4);
        assert_eq!(input.input.points[2].z, 102.0);
        assert!(input.input.breaklines.is_empty());
        assert_eq!(input.used, 4);
        assert_eq!(input.heightless_vertices, 0);
        assert!(input.skipped.is_empty());
    }

    #[test]
    fn a_heightless_point_is_left_out_and_counted() {
        let mut document = Document::default();
        let ids = draw(
            &mut document,
            vec![
                point(0.0, 0.0, Some(100.0)),
                point(10.0, 0.0, Some(101.0)),
                point(10.0, 10.0, Some(102.0)),
                point(5.0, 5.0, None),
            ],
        );
        let input = surface_input(document.model(), &ids);
        assert_eq!(input.input.points.len(), 3);
        assert_eq!(input.used, 3);
        assert_eq!(input.heightless_vertices, 1);
        assert_eq!(input.skipped.get("heightless"), Some(&1));
        for kept in &input.input.points {
            assert!(
                !(kept.x == 5.0 && kept.y == 5.0),
                "the heightless point is not at z = 0"
            );
        }
    }

    #[test]
    fn a_null_vertex_breaks_the_breakline_and_a_ring_that_lost_one_is_not_closed() {
        let mut document = Document::default();
        // A ring of four with its second vertex unsurveyed: the first vertex
        // is a run of one (no breakline), the third and fourth a run of two,
        // and the ring does not close across the gap.
        let ids = draw(
            &mut document,
            vec![polyline(
                vec![p(0.0, 0.0), p(10.0, 0.0), p(10.0, 10.0), p(0.0, 10.0)],
                true,
                &[Some(1.0), None, Some(3.0), Some(4.0)],
            )],
        );
        let input = surface_input(document.model(), &ids);
        assert_eq!(input.input.points.len(), 3);
        assert_eq!(input.heightless_vertices, 1);
        assert_eq!(input.input.breaklines.len(), 1);
        assert_eq!(input.input.breaklines[0].vertices.len(), 2);
        assert!(!input.input.breaklines[0].closed);
        assert_eq!(input.used, 1);
    }

    #[test]
    fn a_whole_levelled_ring_stays_closed() {
        let mut document = Document::default();
        let ids = draw(
            &mut document,
            vec![polyline(
                vec![p(0.0, 0.0), p(10.0, 0.0), p(10.0, 10.0)],
                true,
                &[Some(1.0), Some(2.0), Some(3.0)],
            )],
        );
        let input = surface_input(document.model(), &ids);
        assert_eq!(input.input.breaklines.len(), 1);
        assert!(input.input.breaklines[0].closed);
        assert_eq!(input.input.breaklines[0].vertices.len(), 3);
    }

    #[test]
    fn a_line_is_a_breakline_through_its_two_ends() {
        let mut document = Document::default();
        let mut line = Entity {
            geometry: Geometry::Line(Segment2 {
                start: p(0.0, 0.0),
                end: p(10.0, 0.0),
            }),
            ..Entity::default()
        };
        set_heights(&mut line.properties, &[Some(5.0), Some(6.0)]);
        let ids = draw(&mut document, vec![line]);
        let input = surface_input(document.model(), &ids);
        assert_eq!(input.input.breaklines.len(), 1);
        assert_eq!(input.input.breaklines[0].vertices[1].z, 6.0);
        assert_eq!(input.input.points.len(), 2);
    }

    #[test]
    fn what_no_surface_is_built_from_is_skipped_by_its_type_and_counted() {
        let mut document = Document::default();
        let mut interpreter = CommandInterpreter::default();
        for line in ["TEXT 0,0 2.5 \"LOT 7\"", "CIRCLE 5,5 2", "CIRCLE 9,9 1"] {
            if let Err(error) = interpreter.run(&mut document, line) {
                panic!("{line} -> {}", error.describe());
            }
        }
        let input = surface_input(document.model(), &every_id(document.model()));
        assert!(input.input.points.is_empty());
        assert_eq!(input.skipped.get("text"), Some(&1));
        assert_eq!(input.skipped.get("circle"), Some(&2));
        assert_eq!(input.used, 0);
    }

    #[test]
    fn on_levelled_data_the_rules_give_exactly_what_the_window_gave() {
        let mut document = Document::default();
        draw(
            &mut document,
            vec![
                point(0.0, 0.0, Some(100.0)),
                point(20.0, 0.0, Some(101.5)),
                point(20.0, 20.0, Some(99.25)),
                polyline(
                    vec![p(2.0, 2.0), p(8.0, 2.0), p(8.0, 8.0)],
                    false,
                    &[Some(100.1), Some(100.2), Some(100.3)],
                ),
                polyline(
                    vec![p(12.0, 12.0), p(18.0, 12.0), p(18.0, 18.0), p(12.0, 18.0)],
                    true,
                    &[Some(99.0), Some(99.5), None, Some(99.75)],
                ),
                polyline(
                    vec![p(3.0, 15.0), p(9.0, 15.0), p(9.0, 19.0)],
                    true,
                    &[Some(98.0), Some(98.0), Some(98.0)],
                ),
            ],
        );
        let oracle = window_rules(document.model());
        let input = surface_input(document.model(), &every_id(document.model()));
        expect_same_points(&input.input.points, &oracle.points);
        assert_eq!(input.input.breaklines.len(), oracle.breaklines.len());
        for (i, (made, expected)) in input
            .input
            .breaklines
            .iter()
            .zip(&oracle.breaklines)
            .enumerate()
        {
            expect_same_points(&made.vertices, &expected.vertices);
            assert_eq!(made.closed, expected.closed, "{i}");
        }
        // By hand: 3 levelled points, then 3 + 3 + 3 levelled vertices; 3
        // breaklines (the ring that lost a vertex splits in two runs, of 2 and
        // 1). A point with no height at all is where the rules part (the next
        // test).
        assert_eq!(input.input.points.len(), 12);
        assert_eq!(input.input.breaklines.len(), 3);
    }

    #[test]
    fn on_the_site_plan_the_heightless_are_left_out_where_the_window_put_them_on_the_datum() {
        // samples/site_plan is a 2D drawing: no entity carries a height. The
        // window made its three polylines' 13 vertices (5 + 4 + 4, LIST) a
        // flat surface at z = 0; the rules now leave them out and count them,
        // with the six lines' 12 ends. Counted by hand from LIST: 3 polylines
        // and 6 lines heightless, 1 arc, 3 circles, 4 texts, 3 dimensions.
        let scratch = Scratch::new("site-plan");
        let copy = scratch.path.join("katana-surface-input-site-plan");
        let samples =
            std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../samples/gis"));
        if let Err(error) = copy_tree(&samples.join("..").join("site_plan"), &copy) {
            panic!("{error}");
        }
        let mut document = Document::default();
        let mut interpreter = CommandInterpreter::default();
        let open = format!("OPEN \"{}\"", copy.display());
        if let Err(error) = interpreter.run(&mut document, &open) {
            panic!("{open} -> {}", error.describe());
        }
        assert_eq!(document.model().entities.size(), 20);

        let oracle = window_rules(document.model());
        assert_eq!(oracle.points.len(), 13);
        assert_eq!(oracle.breaklines.len(), 3);
        for datum in &oracle.points {
            assert_eq!(datum.z, 0.0);
        }

        let input = surface_input(document.model(), &every_id(document.model()));
        assert!(input.input.points.is_empty());
        assert!(input.input.breaklines.is_empty());
        assert_eq!(input.heightless_vertices, 25);
        assert_eq!(input.skipped.get("heightless"), Some(&9));
        assert_eq!(input.skipped.get("arc"), Some(&1));
        assert_eq!(input.skipped.get("circle"), Some(&3));
        assert_eq!(input.skipped.get("text"), Some(&4));
        assert_eq!(input.skipped.get("dimension"), Some(&3));
    }
}
