// The label placer (annotation/label_layout.rs, docs/annotation.md "The
// placer"; tests/cad/annotation/test_label_layout.cpp): candidates in the
// style's order, overlap refused, displaced labels with leaders, suppression
// counted, the order fixed so the result does not depend on the input's
// order, and the scale deciding what fits.

use std::sync::Arc;

use katana_cad::annotation::drawing::{TextFace, TextMeasure, estimated_measure};
use katana_cad::annotation::label_layout::{
    LabelLayout, LabelLayoutOptions, LabelPieceRef, append_dimension_keep_out, convex_overlap,
    label_entities, label_keep_out, layout_labels,
};
use katana_cad::dimension_draw::{build_dimension, resolve_dimension_style};
use katana_entity::annotation::DEFAULT_ANNOTATION_SCALE;
use katana_entity::text_block::character_count;
use katana_entity::{
    Alignment, DimensionGeometry, Entity, EntityId, Geometry, LabelGeometry, LabelKind,
    LabelPlacement, LabelStyle, LeaderContext, Model, PointGeometry, PropertyValue, TextGeometry,
};
use katana_geometry::{AlignmentPi, Box2, Point2, Polyline2, Segment2};
use katana_math::numerics::PI;

use crate::{Mt19937, near, ok};

fn half_per_character() -> TextMeasure {
    Arc::new(|line: &str, _face: &TextFace| 0.5 * character_count(line) as f64)
}

// A model with a point-number style and the points given, each labelled.
struct Scene {
    model: Model,
    labels: Vec<EntityId>,
}

fn points_labelled(points: &[Point2], displace: bool, priority: i32) -> Scene {
    let mut scene = Scene {
        model: Model::default(),
        labels: Vec::new(),
    };
    ok(scene.model.label_styles.add(LabelStyle {
        name: String::from("pt"),
        kind: LabelKind::Point,
        text: String::from("{point}"),
        displace,
        priority,
        ..LabelStyle::default()
    }));
    let mut next: EntityId = 1;
    for (i, &at) in points.iter().enumerate() {
        let mut point = Entity {
            id: next,
            geometry: Geometry::Point(PointGeometry { position: at }),
            ..Entity::default()
        };
        next += 1;
        point.properties.insert(
            String::from("point"),
            PropertyValue::Text(format!("P{}", i + 1)),
        );
        let target = point.id;
        ok(scene.model.entities.insert(point));
        let label = Entity {
            id: next,
            geometry: Geometry::Label(LabelGeometry {
                target,
                style: String::from("pt"),
                anchor: at,
                ..LabelGeometry::default()
            }),
            ..Entity::default()
        };
        next += 1;
        scene.labels.push(label.id);
        ok(scene.model.entities.insert(label));
    }
    scene
}

fn at(scale: f64) -> LabelLayoutOptions {
    LabelLayoutOptions {
        scale,
        measure: half_per_character(),
        ..LabelLayoutOptions::default()
    }
}

fn boxes(layout: &LabelLayout) -> Vec<Vec<Point2>> {
    layout
        .placed
        .iter()
        .map(|placed| placed.drawing.text_boxes[0].clone())
        .collect()
}

fn layout_of(model: &Model, options: &LabelLayoutOptions) -> LabelLayout {
    layout_labels(model, &label_entities(model), options)
}

fn set_label_style(model: &mut Model, id: EntityId, change: impl FnOnce(&mut LabelGeometry)) {
    let mut entity = model.entities.find(id).expect("the label").clone();
    if let Geometry::Label(label) = &mut entity.geometry {
        change(label);
    }
    ok(model.entities.replace(entity));
}

fn p(x: f64, y: f64) -> Point2 {
    Point2::new(x, y)
}

mod label_layout {
    use super::*;

    #[test]
    fn the_separating_axis_test_is_exact() {
        let square = [p(0.0, 0.0), p(2.0, 0.0), p(2.0, 2.0), p(0.0, 2.0)];
        let apart = [p(3.0, 0.0), p(5.0, 0.0), p(5.0, 2.0), p(3.0, 2.0)];
        let touching = [p(2.0, 0.0), p(4.0, 0.0), p(4.0, 2.0), p(2.0, 2.0)];
        // A diamond whose box overlaps the square's but which misses it.
        let diamond = [p(3.0, 2.9), p(3.95, 3.85), p(3.0, 4.8), p(2.05, 3.85)];
        let inside = [p(0.5, 0.5), p(1.0, 0.5), p(1.0, 1.0)];
        assert!(!convex_overlap(&square, &apart));
        assert!(
            !convex_overlap(&square, &touching),
            "touching edges are apart"
        );
        assert!(!convex_overlap(&square, &diamond));
        assert!(convex_overlap(&square, &inside));
    }

    #[test]
    fn two_labels_that_would_collide_are_kept_apart() {
        // Two points 1 m apart. At 1:1000 a 2.5 mm label is 2.5 m tall: their
        // upper-right places overlap, so the second takes another place.
        let scene = points_labelled(&[p(0.0, 0.0), p(1.0, 0.0)], true, 0);
        let layout = layout_of(&scene.model, &at(1000.0));
        assert_eq!(layout.placed.len(), 2);
        assert_eq!(layout.suppressed, 0);
        assert_eq!(
            layout.placed[0].candidate, 0,
            "the first placed takes its first place"
        );
        assert_ne!(layout.placed[1].candidate, 0);
        let all = boxes(&layout);
        assert!(!convex_overlap(&all[0], &all[1]));

        // With avoidance off, both take their first place - and overlap.
        let off = LabelLayoutOptions {
            avoid_collisions: false,
            ..at(1000.0)
        };
        let blind = layout_of(&scene.model, &off);
        assert!(convex_overlap(
            &blind.placed[0].drawing.text_boxes[0],
            &blind.placed[1].drawing.text_boxes[0]
        ));
    }

    #[test]
    fn the_scale_decides_what_fits() {
        // The same two points 10 m apart: at 1:200 the labels are 0.5 m tall
        // and both sit at their first place; at 1:5000 they are 12.5 m and the
        // second must move.
        let scene = points_labelled(&[p(0.0, 0.0), p(10.0, 0.0)], true, 0);
        let large = layout_of(&scene.model, &at(200.0));
        assert_eq!(large.placed.len(), 2);
        assert_eq!(large.placed[1].candidate, 0);
        let small = layout_of(&scene.model, &at(5000.0));
        assert!(small.placed.len() < 2 || small.placed[1].candidate != 0);
    }

    #[test]
    fn with_no_room_a_label_is_suppressed_and_counted() {
        // Five points at one place, a style that may not move its labels: the
        // first is placed, the other four have no room.
        let scene = points_labelled(&[p(0.0, 0.0); 5], false, 0);
        let layout = layout_of(&scene.model, &at(1000.0));
        assert_eq!(layout.considered, 5);
        assert_eq!(layout.placed.len(), 1);
        assert_eq!(layout.suppressed, 4);
        // Which: the labels are 2, 4, 6, 8 and 10, placed in id order, so the
        // first is placed and the rest are named, one piece each.
        assert_eq!(layout.placed[0].label, scene.labels[0]);
        let named: Vec<LabelPieceRef> = scene.labels[1..]
            .iter()
            .map(|&label| LabelPieceRef { label, piece: 0 })
            .collect();
        assert_eq!(layout.suppressed_pieces, named);
    }

    #[test]
    fn a_displaced_label_gets_a_leader_back() {
        // A crowd of twelve at one point: once the eight places around it are
        // taken, labels move out to the displaced rings, each with a leader
        // from the point to its text.
        let scene = points_labelled(&[p(0.0, 0.0); 12], true, 0);
        let layout = layout_of(&scene.model, &at(1000.0));
        assert!(layout.displaced > 0);
        for placed in &layout.placed {
            if placed.displaced {
                assert!(!placed.drawing.strokes.is_empty());
                assert_eq!(
                    placed.drawing.strokes[0][0],
                    p(0.0, 0.0),
                    "the leader starts at the point labelled"
                );
            }
        }
        let all = boxes(&layout);
        for (i, a) in all.iter().enumerate() {
            for (j, b) in all.iter().enumerate().skip(i + 1) {
                assert!(!convex_overlap(a, b), "{i} and {j}");
            }
        }
    }

    #[test]
    fn the_result_does_not_depend_on_the_order_labels_arrive_in() {
        let mut generator = Mt19937::new(20260925);
        let mut points = Vec::new();
        for _ in 0..40 {
            // GCC evaluates emplace_back's two arguments right to left: the
            // y is drawn first.
            let y = generator.real(0.0, 30.0);
            let x = generator.real(0.0, 30.0);
            points.push(p(x, y));
        }
        let scene = points_labelled(&points, true, 0);
        let mut labels = label_entities(&scene.model);
        let first = layout_labels(&scene.model, &labels, &at(1000.0));
        // A Fisher-Yates shuffle from the same engine: the case holds for
        // every order, so libstdc++'s own permutation is not needed.
        for i in (1..labels.len()).rev() {
            let j = (generator.next_u32() as usize) % (i + 1);
            labels.swap(i, j);
        }
        let second = layout_labels(&scene.model, &labels, &at(1000.0));
        assert_eq!(first.placed.len(), second.placed.len());
        assert_eq!(first.suppressed, second.suppressed);
        for (a, b) in first.placed.iter().zip(&second.placed) {
            assert_eq!(a.label, b.label);
            assert_eq!(a.candidate, b.candidate);
            assert_eq!(a.drawing.texts[0].origin, b.drawing.texts[0].origin);
        }
    }

    #[test]
    fn a_higher_priority_is_placed_first_and_keeps_its_place() {
        let mut scene = points_labelled(&[p(0.0, 0.0), p(1.0, 0.0)], true, 0);
        // The second point's label gets its own style of higher priority.
        let mut urgent = scene.model.label_styles.find("pt").expect("pt").clone();
        urgent.name = String::from("urgent");
        urgent.priority = 10;
        ok(scene.model.label_styles.add(urgent));
        set_label_style(&mut scene.model, scene.labels[1], |label| {
            label.style = String::from("urgent");
        });
        let layout = layout_of(&scene.model, &at(1000.0));
        assert_eq!(layout.placed.len(), 2);
        assert_eq!(
            layout.placed[0].label, scene.labels[1],
            "the urgent one first"
        );
        assert_eq!(layout.placed[0].candidate, 0);
    }

    #[test]
    fn text_keeps_off_the_lines_it_is_given() {
        let scene = points_labelled(&[p(0.0, 0.0)], true, 0);
        let mut options = at(1000.0);
        // A line through where the upper-right label would go.
        options
            .linework
            .push(Segment2::new(p(1.0, 2.0), p(6.0, 2.0)));
        let layout = layout_of(&scene.model, &options);
        assert_eq!(layout.placed.len(), 1);
        assert_ne!(layout.placed[0].candidate, 0);
    }

    #[test]
    fn a_dragged_label_is_where_it_was_put() {
        let mut scene = points_labelled(&[p(0.0, 0.0), p(0.0, 0.0)], true, 0);
        set_label_style(&mut scene.model, scene.labels[1], |label| {
            label.position = Some(p(1.0, 1.0));
        });
        let layout = layout_of(&scene.model, &at(1000.0));
        assert_eq!(layout.placed.len(), 2);
        assert_eq!(
            layout.placed[0].label, scene.labels[1],
            "a dragged label is placed first"
        );
        let mut placed = Box2::default();
        for &corner in &layout.placed[0].drawing.text_boxes[0] {
            placed.expand(corner);
        }
        assert!(placed.contains(p(1.0, 1.0)), "centred where it was put");
    }

    #[test]
    fn segment_labels_sit_beside_their_segment_reading_along_it() {
        let mut model = Model::default();
        ok(model.label_styles.add(LabelStyle {
            name: String::from("bd"),
            kind: LabelKind::Segment,
            text: String::from("{distance:.1f}"),
            ..LabelStyle::default()
        }));
        ok(model.entities.insert(Entity {
            id: 1,
            geometry: Geometry::Line(Segment2::new(p(0.0, 0.0), p(0.0, 100.0))), // north
            ..Entity::default()
        }));
        ok(model.entities.insert(Entity {
            id: 2,
            geometry: Geometry::Label(LabelGeometry {
                target: 1,
                style: String::from("bd"),
                anchor: p(0.0, 50.0),
                ..LabelGeometry::default()
            }),
            ..Entity::default()
        }));
        let layout = layout_of(&model, &at(1000.0));
        assert_eq!(layout.placed.len(), 1);
        let run = &layout.placed[0].drawing.texts[0];
        assert_eq!(run.text, "100.0");
        // Reads up the line.
        near(run.rotation, 0.5 * PI, 1e-12);
        // Above a line running north is its left: west of it.
        assert!(run.origin.x < 0.0);
    }

    #[test]
    fn chainage_ticks_are_drawn_at_every_mark() {
        let mut model = Model::default();
        let mut road = Alignment {
            name: String::from("road"),
            ..Alignment::default()
        };
        road.horizontal.pis = vec![
            AlignmentPi {
                point: p(0.0, 0.0),
                ..AlignmentPi::default()
            },
            AlignmentPi {
                point: p(100.0, 0.0),
                ..AlignmentPi::default()
            },
        ];
        ok(model.alignments.add(road));
        ok(model.label_styles.add(LabelStyle {
            name: String::from("ch"),
            kind: LabelKind::Chainage,
            text: String::from("{chainage:ch}"),
            interval: 50.0,
            tick_interval: 10.0,
            ..LabelStyle::default()
        }));
        ok(model.entities.insert(Entity {
            id: 1,
            geometry: Geometry::Label(LabelGeometry {
                alignment: String::from("road"),
                style: String::from("ch"),
                ..LabelGeometry::default()
            }),
            ..Entity::default()
        }));
        let layout = layout_of(&model, &at(1000.0));
        // 0, 50, 100 labelled (each with its tick); 10..40 and 60..90 ticks
        // only.
        assert_eq!(layout.placed.len(), 3);
        assert_eq!(layout.marks_only.len(), 8);
        assert_eq!(layout.placed[1].drawing.texts[0].text, "0+050.000");
    }

    #[test]
    fn a_label_whose_target_is_gone_is_counted_not_placed() {
        let mut scene = points_labelled(&[p(0.0, 0.0)], true, 0);
        assert!(scene.model.entities.remove(1).is_ok());
        let layout = layout_of(&scene.model, &at(1000.0));
        assert!(layout.placed.is_empty());
        assert_eq!(layout.orphaned, 1);
    }

    #[test]
    fn a_note_is_kept_out_of_even_when_the_label_would_fit_inside_it() {
        // A point with a large note over its upper right: the label's first
        // place lies wholly inside the note's box, crossing none of its edges
        // - the box's diagonals are what catch it.
        let mut scene = points_labelled(&[p(0.0, 0.0)], true, 0);
        ok(scene.model.entities.insert(Entity {
            id: 100,
            geometry: Geometry::Text(TextGeometry {
                position: p(0.5, 0.5),
                text: String::from("NOTE"),
                height: 6.0,
                ..TextGeometry::default()
            }),
            ..Entity::default()
        }));

        let mut options = at(1000.0);
        let blind = layout_of(&scene.model, &options);
        assert_eq!(blind.placed.len(), 1);
        assert_eq!(blind.placed[0].candidate, 0, "nothing to keep out of");

        options.linework = label_keep_out(
            &scene.model,
            options.scale,
            &options.measure,
            1000,
            &LeaderContext::default(),
        );
        assert!(!options.linework.is_empty());
        let layout = layout_of(&scene.model, &options);
        assert_eq!(layout.placed.len(), 1);
        assert_ne!(layout.placed[0].candidate, 0);
    }

    #[test]
    fn the_keep_out_holds_a_dimensions_lines_and_figures() {
        let mut model = Model::default();
        let dimension = Entity {
            id: 1,
            geometry: Geometry::Dimension(DimensionGeometry {
                start: p(0.0, 0.0),
                end: p(40.0, 0.0),
                offset: 5.0,
                ..DimensionGeometry::default()
            }),
            ..Entity::default()
        };
        ok(model.entities.insert(dimension.clone()));
        // Its drawing, and what it keeps out: two extension lines, the
        // dimension line, and the figures' box - four edges and two diagonals.
        let Geometry::Dimension(measured) = &dimension.geometry else {
            panic!("a dimension");
        };
        let drawing = build_dimension(
            measured,
            resolve_dimension_style(&model, &dimension),
            DEFAULT_ANNOTATION_SCALE,
        );
        let mut keep_out = Vec::new();
        append_dimension_keep_out(&drawing, &mut keep_out, 1000);
        assert_eq!(keep_out.len(), 9);
        let context = LeaderContext::default();
        assert_eq!(
            label_keep_out(&model, 1000.0, &estimated_measure(), 1000, &context).len(),
            9
        );
        // The limit is a limit.
        keep_out.clear();
        append_dimension_keep_out(&drawing, &mut keep_out, 4);
        assert_eq!(keep_out.len(), 4);
        // A hidden entity is in nobody's way.
        let mut hidden = model.entities.find(1).expect("the dimension").clone();
        hidden.visible = false;
        ok(model.entities.replace(hidden));
        assert!(label_keep_out(&model, 1000.0, &estimated_measure(), 1000, &context).is_empty());
    }

    #[test]
    fn an_area_label_moves_off_a_line_before_it_takes_a_mask() {
        let mut model = Model::default();
        let style = LabelStyle {
            name: String::from("lot"),
            kind: LabelKind::Area,
            text: String::from("{area:m2:.0f}"),
            placement: LabelPlacement::Centroid,
            ..LabelStyle::default()
        };
        ok(model.label_styles.add(style.clone()));
        ok(model.entities.insert(Entity {
            id: 1,
            geometry: Geometry::Polyline(Polyline2::new(
                vec![p(0.0, 0.0), p(40.0, 0.0), p(40.0, 25.0), p(0.0, 25.0)],
                true,
            )),
            ..Entity::default()
        }));
        // A kerb across the lot, through its middle.
        ok(model.entities.insert(Entity {
            id: 2,
            geometry: Geometry::Line(Segment2::new(p(0.0, 12.5), p(40.0, 12.5))),
            ..Entity::default()
        }));
        ok(model.entities.insert(Entity {
            id: 3,
            geometry: Geometry::Label(LabelGeometry {
                target: 1,
                style: String::from("lot"),
                anchor: p(20.0, 12.5),
                ..LabelGeometry::default()
            }),
            ..Entity::default()
        }));

        let mut options = at(200.0);
        options.linework = label_keep_out(
            &model,
            options.scale,
            &options.measure,
            1000,
            &LeaderContext::default(),
        );
        let moved = layout_of(&model, &options);
        assert_eq!(moved.placed.len(), 1);
        assert_ne!(moved.placed[0].candidate, 0, "stepped off the kerb");
        assert!(
            moved.placed[0].drawing.masks.is_empty(),
            "clear, so nothing hidden"
        );

        // A style that may not move its text keeps the centre, over a mask.
        let fixed = LabelStyle {
            displace: false,
            ..style
        };
        ok(model.label_styles.update(&fixed));
        let masked = layout_of(&model, &options);
        assert_eq!(masked.placed.len(), 1);
        assert_eq!(masked.placed[0].candidate, 0);
        assert!(!masked.placed[0].drawing.masks.is_empty());
    }
}
