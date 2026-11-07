//! The twin of tests/entity/annotation/test_annotation_model.cpp: the
//! annotation additions to the entity layer (docs/annotation.md) - the three
//! tables, the text block arithmetic, the Label and Leader kinds, the
//! annotation members of Text and Dimension, anchors and label values.
//!
//! Expected values are worked out by hand in the comments beside them.

pub mod support;

use katana_entity::anchor::{describe, resolve_anchor};
use katana_entity::annotation::{
    DEFAULT_ANNOTATION_SCALE, DEFAULT_TEXT_STYLE_NAME, annotation_model_size, glob_matches,
};
use katana_entity::entity_geometry::{bounding_box, distance_to, transformed};
use katana_entity::geometry_blob::{
    BLOB_VERSION, BLOB_VERSION_ANNOTATION, geometry_from_blob, geometry_to_blob,
};
use katana_entity::label_text::format_label;
use katana_entity::label_values::label_pieces;
use katana_entity::serialization::{geometry_from_json, geometry_to_json};
use katana_entity::text_block::{
    LINE_PITCH, character_count, line_origin, text_block_extent, text_lines,
};
use katana_entity::{
    Alignment, AnchorPoint, AnchorRef, ArrowHead, CalloutShape, DimensionGeometry, DimensionKind,
    Entity, EntityId, Geometry, LabelGeometry, LabelKind, LabelRule, LabelStyle, LeaderGeometry,
    Model, PointGeometry, PropertyValue, TextGeometry, TextJustify, TextStyle, TextStyleDatabase,
    validate,
};
use katana_geometry::{AlignmentPi, Arc2, Circle2, Point2, Polyline2, Segment2, Vec2};
use katana_math::Mat3;
use katana_math::numerics::{DEG_TO_RAD, PI};
use support::{double_eq, near};

fn p(x: f64, y: f64) -> Point2 {
    Point2::new(x, y)
}

fn entity_of(id: EntityId, geometry: Geometry) -> Entity {
    Entity {
        id,
        geometry,
        ..Entity::default()
    }
}

fn anchor(entity: u64, point: AnchorPoint, index: u32) -> AnchorRef {
    AnchorRef {
        entity,
        point,
        index,
        parameter: 0.0,
    }
}

fn label_on(target: u64, style: &str) -> LabelGeometry {
    LabelGeometry {
        target,
        style: String::from(style),
        ..LabelGeometry::default()
    }
}

fn style_of(name: &str, kind: LabelKind, text: &str) -> LabelStyle {
    LabelStyle {
        name: String::from(name),
        kind,
        text: String::from(text),
        ..LabelStyle::default()
    }
}

// ---- the one conversion and the tables -----------------------------------------------

mod annotation_scale {
    use super::*;

    #[test]
    fn paper_millimetres_become_model_units_at_the_scale() {
        // 2.5 mm at 1:200 is 0.5 m on the ground; at 1:1000, 2.5 m; at 1:2000,
        // 5 m.
        double_eq(annotation_model_size(2.5, 200.0), 0.5);
        double_eq(annotation_model_size(2.5, 1000.0), 2.5);
        double_eq(annotation_model_size(2.5, 2000.0), 5.0);
        // The scale at which a 2.5 mm text is TextGeometry's default 2.5 units.
        double_eq(DEFAULT_ANNOTATION_SCALE, 1000.0);
    }
}

mod text_styles {
    use super::*;

    #[test]
    fn standard_is_built_in_and_the_rules_are_each_said() {
        let mut styles = TextStyleDatabase::new();
        assert!(styles.contains(DEFAULT_TEXT_STYLE_NAME));
        assert!(styles.remove(DEFAULT_TEXT_STYLE_NAME).is_err());

        let road = TextStyle {
            name: String::from("Road"),
            paper_height: 3.5,
            width_factor: 0.8,
            oblique: 15.0 * DEG_TO_RAD,
            ..TextStyle::default()
        };
        assert!(styles.add(road.clone()).is_ok());

        let mut bad = road;
        bad.name = String::from("flat");
        bad.width_factor = 0.0;
        assert!(validate(&bad).is_err(), "a width factor of 0 draws nothing");
        bad.width_factor = 1.0;
        bad.oblique = 85.0 * DEG_TO_RAD;
        assert!(validate(&bad).is_err(), "85 degrees of slant is a line");
        bad.oblique = 0.0;
        bad.line_spacing = 0.1;
        assert!(validate(&bad).is_err(), "below MTEXT's 0.25");
        bad.line_spacing = 1.0;
        bad.paper_height = -1.0;
        assert!(validate(&bad).is_err());
    }
}

mod label_styles {
    use super::*;

    #[test]
    fn the_template_is_checked_on_the_way_in() {
        let mut style = style_of(
            "Bearing",
            LabelKind::Segment,
            "{bearing:dms} {distance:.3f}",
        );
        assert!(validate(&style).is_ok());
        style.text = String::from("{area}");
        let refused = validate(&style).unwrap_err();
        assert!(refused.context.contains("Bearing"), "names the style");
        style.text = String::from("{bearing}");
        style.kind = LabelKind::Chainage;
        assert!(validate(&style).is_err(), "a chainage label has no bearing");
        style.text = String::from("{chainage:ch}");
        style.interval = 0.0;
        assert!(
            validate(&style).is_err(),
            "a chainage interval must be positive"
        );
    }
}

mod label_rules {
    use super::*;

    #[test]
    fn need_a_style_and_a_known_type() {
        let mut rule = LabelRule {
            name: String::from("boundaries"),
            ..LabelRule::default()
        };
        assert!(validate(&rule).is_err(), "no style");
        rule.label_style = String::from("Bearing");
        assert!(validate(&rule).is_ok());
        // Not a kind there is ("Spline" was, until the drawing system added one).
        rule.entity_type = String::from("Hyperbola");
        assert!(validate(&rule).is_err());
        rule.entity_type = String::from("Polyline");
        assert!(validate(&rule).is_ok());
    }
}

mod globs {
    use super::*;

    #[test]
    fn match_stars_and_question_marks_without_case() {
        assert!(glob_matches("", "anything"));
        assert!(glob_matches("EP*", "ep12"));
        assert!(glob_matches("*BDY*", "lot-bdy-3"));
        assert!(glob_matches("T?", "TB"));
        assert!(!glob_matches("T?", "TBX"));
        assert!(glob_matches("survey/*", "survey/points"));
        assert!(!glob_matches("survey/*", "design/survey"));
        assert!(glob_matches("a*b*c", "aXXbYYc"));
        assert!(!glob_matches("a*b*c", "aXXbYY"));
    }
}

mod label_values {
    use super::*;

    #[test]
    fn the_code_is_the_first_of_the_names_given_that_the_target_carries() {
        let mut model = Model::new();
        let mut point = entity_of(1, Geometry::Point(PointGeometry::default()));
        point
            .metadata
            .insert(String::from("feature"), PropertyValue::from("TREE"));
        assert!(model.entities.insert(point.clone()).is_ok());
        let style = style_of("code", LabelKind::Point, "{code}");
        let label = label_on(1, "code");
        assert_eq!(
            format_label(
                &style.text,
                &label_pieces(&model, &label, &style, &[])[0].values
            ),
            "",
            "no names given: no code"
        );
        let names = [String::from("code"), String::from("feature")];
        assert_eq!(
            format_label(
                &style.text,
                &label_pieces(&model, &label, &style, &names)[0].values
            ),
            "TREE",
            "found in the metadata"
        );
        point
            .properties
            .insert(String::from("code"), PropertyValue::from("EP"));
        assert!(model.entities.replace(point).is_ok());
        assert_eq!(
            format_label(
                &style.text,
                &label_pieces(&model, &label, &style, &names)[0].values
            ),
            "EP",
            "the first name wins"
        );
    }

    #[test]
    fn a_point_has_its_level_only_when_it_has_one() {
        let mut model = Model::new();
        let mut point = entity_of(
            1,
            Geometry::Point(PointGeometry {
                position: p(300000.5, 6250000.25),
            }),
        );
        point
            .properties
            .insert(String::from("point"), PropertyValue::Integer(1001));
        assert!(model.entities.insert(point.clone()).is_ok());
        let style = style_of("pt", LabelKind::Point, "{point}\nRL {z:.3f}\n{easting:.3f}");
        let label = label_on(1, "pt");
        let pieces = label_pieces(&model, &label, &style, &[]);
        assert_eq!(pieces.len(), 1);
        assert_eq!(
            format_label(&style.text, &pieces[0].values),
            "1001\n300000.500",
            "no RL line: the point has no level, and absent is not zero"
        );

        let mut levelled = point;
        levelled
            .properties
            .insert(String::from("elevation"), PropertyValue::Real(31.25));
        assert!(model.entities.replace(levelled).is_ok());
        let pieces = label_pieces(&model, &label, &style, &[]);
        assert_eq!(
            format_label(&style.text, &pieces[0].values),
            "1001\nRL 31.250\n300000.500"
        );
    }

    #[test]
    fn each_segment_of_a_polyline_is_a_piece() {
        let mut model = Model::new();
        // A 3-4-5 triangle's two legs and hypotenuse, closed: (0,0)->(3,0)
        // east 90, (3,0)->(3,4) north 0, (3,4)->(0,0) 216 52' 12".
        let mut lot = entity_of(
            1,
            Geometry::Polyline(Polyline2::new(
                vec![p(0.0, 0.0), p(3.0, 0.0), p(3.0, 4.0)],
                true,
            )),
        );
        lot.properties
            .insert(String::from("elevations"), PropertyValue::from("10 10 12"));
        assert!(model.entities.insert(lot).is_ok());
        let style = style_of("bd", LabelKind::Segment, "{bearing:dms} {distance:.3f}");
        let pieces = label_pieces(&model, &label_on(1, "bd"), &style, &[]);
        assert_eq!(pieces.len(), 3);
        assert_eq!(
            format_label(&style.text, &pieces[0].values),
            "90\u{b0}00'00\" 3.000"
        );
        assert_eq!(
            format_label(&style.text, &pieces[1].values),
            "0\u{b0}00'00\" 4.000"
        );
        assert_eq!(
            format_label(&style.text, &pieces[2].values),
            "216\u{b0}52'12\" 5.000"
        );
        // The grade of the second leg: 2 m up over 4 m is 50 per cent.
        assert_eq!(format_label("{grade:.1f}%", &pieces[1].values), "50.0%");
        assert_eq!(pieces[1].anchor, p(3.0, 2.0), "the middle of the segment");

        let one = label_pieces(
            &model,
            &LabelGeometry {
                part: 2,
                ..label_on(1, "bd")
            },
            &style,
            &[],
        );
        assert_eq!(one.len(), 1);
        assert_eq!(
            format_label(&style.text, &one[0].values),
            "216\u{b0}52'12\" 5.000"
        );
    }

    #[test]
    fn an_arc_has_its_curve_data() {
        let mut model = Model::new();
        // Radius 10, a quarter turn: length 15.708, chord 10 sqrt 2 = 14.142,
        // tangent R tan 45 = 10.
        assert!(
            model
                .entities
                .insert(entity_of(
                    1,
                    Geometry::Arc(Arc2::new(p(0.0, 0.0), 10.0, 0.0, 0.5 * PI))
                ))
                .is_ok()
        );
        let style = style_of(
            "arc",
            LabelKind::Arc,
            "R{radius:.1f} L{length:.3f} C{chord:.3f} T{tangent:.3f} D{delta:dms}",
        );
        let pieces = label_pieces(&model, &label_on(1, "arc"), &style, &[]);
        assert_eq!(pieces.len(), 1);
        assert_eq!(
            format_label(&style.text, &pieces[0].values),
            "R10.0 L15.708 C14.142 T10.000 D90\u{b0}00'00\""
        );
    }

    #[test]
    fn an_area_is_labelled_inside_it_even_when_its_centroid_is_not() {
        let mut model = Model::new();
        // A thin C whose centroid is outside.
        let c = Polyline2::new(
            vec![
                p(0.0, 0.0),
                p(30.0, 0.0),
                p(30.0, 2.0),
                p(2.0, 2.0),
                p(2.0, 28.0),
                p(30.0, 28.0),
                p(30.0, 30.0),
                p(0.0, 30.0),
            ],
            true,
        );
        assert!(
            !c.contains(c.centroid().unwrap()),
            "the premise: the C's centroid is in its mouth"
        );
        assert!(
            model
                .entities
                .insert(entity_of(1, Geometry::Polyline(c.clone())))
                .is_ok()
        );
        let style = style_of("area", LabelKind::Area, "{area:m2:.1f}\n{area:ha:.4f} ha");
        let pieces = label_pieces(&model, &label_on(1, "area"), &style, &[]);
        assert_eq!(pieces.len(), 1);
        assert!(c.contains(pieces[0].anchor));
        // 30 x 30 = 900 less the mouth 28 x 26 = 728: 172 m2.
        assert_eq!(
            format_label(&style.text, &pieces[0].values),
            "172.0\n0.0172 ha"
        );
    }

    #[test]
    fn chainages_are_counted_not_accumulated() {
        let mut model = Model::new();
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
        assert!(model.alignments.add(road).is_ok());
        let style = LabelStyle {
            interval: 20.0,
            tick_interval: 10.0,
            ..style_of("ch", LabelKind::Chainage, "{chainage:ch}")
        };
        let label = LabelGeometry {
            alignment: String::from("road"),
            style: String::from("ch"),
            ..LabelGeometry::default()
        };
        let pieces = label_pieces(&model, &label, &style, &[]);
        // 0, 10, ... 100: eleven marks, six of them labelled.
        assert_eq!(pieces.len(), 11);
        let labelled = pieces.iter().filter(|piece| !piece.tick_only).count();
        assert_eq!(labelled, 6);
        assert_eq!(format_label(&style.text, &pieces[10].values), "0+100.000");
        assert_eq!(pieces[10].anchor, p(100.0, 0.0));
    }

    #[test]
    fn a_style_that_cannot_label_the_target_makes_nothing() {
        let mut model = Model::new();
        assert!(
            model
                .entities
                .insert(entity_of(1, Geometry::Point(PointGeometry::default())))
                .is_ok()
        );
        let style = style_of("bd", LabelKind::Segment, "{distance}");
        assert!(label_pieces(&model, &label_on(1, "bd"), &style, &[]).is_empty());
        assert!(
            label_pieces(&model, &label_on(99, "bd"), &style, &[]).is_empty(),
            "a target that is gone"
        );
    }
}

// ---- text blocks -----------------------------------------------------------------------

mod text_block {
    use super::*;

    #[test]
    fn the_nine_points_place_the_block() {
        // Two lines at height 2, pitch 5/3 x 2 = 10/3: the block is 2 + 10/3 =
        // 16/3 tall, from the top of the first line to the last baseline. 10
        // wide.
        let h = 2.0;
        let block = h + LINE_PITCH * h;
        let bl = text_block_extent(10.0, 2, h, 1.0, TextJustify::BottomLeft);
        double_eq(bl.left, 0.0);
        double_eq(bl.bottom, 0.0);
        double_eq(bl.top, block);
        let mc = text_block_extent(10.0, 2, h, 1.0, TextJustify::MiddleCentre);
        double_eq(mc.left, -5.0);
        double_eq(mc.right, 5.0);
        double_eq(mc.bottom, -0.5 * block);
        let tr = text_block_extent(10.0, 2, h, 1.0, TextJustify::TopRight);
        double_eq(tr.right, 0.0);
        double_eq(tr.top, 0.0);
        // The first line's baseline is a height below the top; a narrower
        // second line is centred in a centred block.
        let first = line_origin(&mc, 0, 10.0, h, 1.0, TextJustify::MiddleCentre);
        double_eq(first.y, mc.top - h);
        let second = line_origin(&mc, 1, 4.0, h, 1.0, TextJustify::MiddleCentre);
        double_eq(second.x, -2.0);
        double_eq(second.y, mc.bottom);
    }

    #[test]
    fn a_one_line_baseline_left_text_has_the_box_it_always_had() {
        // The box before justification existed: the baseline from the
        // position, 0.6 h per character long, and h up. "ABCD" at 2.5: 6 x 2.5.
        let text = Geometry::Text(TextGeometry::new(p(10.0, 20.0), "ABCD", 2.5, 0.0));
        let bounds = bounding_box(&text);
        double_eq(bounds.min.x, 10.0);
        double_eq(bounds.min.y, 20.0);
        double_eq(bounds.max.x, 16.0);
        double_eq(bounds.max.y, 22.5);
        // Picked by its baseline.
        double_eq(distance_to(&text, p(13.0, 20.0)), 0.0);
    }

    #[test]
    fn characters_are_counted_not_bytes() {
        assert_eq!(character_count("Caf\u{e9}"), 4);
        // 3, 6, the degree sign, 5, 2 and the minute mark.
        assert_eq!(character_count("36\u{b0}52'"), 6);
        assert_eq!(
            text_lines("a\nbc\n").len(),
            3,
            "a trailing break is a trailing empty line"
        );
        assert_eq!(text_lines("").len(), 1);
    }
}

// ---- the new kinds and members ---------------------------------------------------------

mod annotation_geometry {
    use super::*;

    #[test]
    fn dimension_kinds_measure_what_they_say() {
        let mut linear = DimensionGeometry {
            kind: DimensionKind::Linear,
            start: p(0.0, 0.0),
            end: p(3.0, 4.0),
            ..DimensionGeometry::default()
        };
        double_eq(linear.measurement(), 3.0); // horizontal: the x difference
        linear.angle = 0.5 * PI;
        near(linear.measurement(), 4.0, 1e-12); // vertical: the y difference

        let mut angular = DimensionGeometry {
            kind: DimensionKind::Angular,
            vertex: p(0.0, 0.0),
            start: p(1.0, 0.0),
            end: p(0.0, 1.0),
            ..DimensionGeometry::default()
        };
        near(angular.measurement(), 0.5 * PI, 1e-15);
        std::mem::swap(&mut angular.start, &mut angular.end);
        near(angular.measurement(), 1.5 * PI, 1e-15); // counter-clockwise from start

        let mut radius = DimensionGeometry {
            kind: DimensionKind::Radius,
            vertex: p(5.0, 5.0),
            start: p(8.0, 9.0),
            ..DimensionGeometry::default()
        };
        double_eq(radius.measurement(), 5.0);
        radius.kind = DimensionKind::Diameter;
        double_eq(radius.measurement(), 10.0);

        let mut ordinate = DimensionGeometry {
            kind: DimensionKind::OrdinateX,
            vertex: p(100.0, 200.0),
            start: p(90.0, 230.0),
            end: p(90.0, 240.0),
            ..DimensionGeometry::default()
        };
        double_eq(ordinate.measurement(), -10.0); // west of the datum is negative
        ordinate.kind = DimensionKind::OrdinateY;
        double_eq(ordinate.measurement(), 30.0);
        assert!(validate(&Geometry::Dimension(ordinate)).is_ok());
    }

    #[test]
    fn labels_and_leaders_validate() {
        let mut label = LabelGeometry {
            target: 3,
            style: String::from("S"),
            anchor: p(1.0, 2.0),
            ..LabelGeometry::default()
        };
        assert!(validate(&Geometry::Label(label.clone())).is_ok());
        label.alignment = String::from("road");
        assert!(
            validate(&Geometry::Label(label.clone())).is_err(),
            "a target AND an alignment"
        );
        label.alignment.clear();
        label.style.clear();
        assert!(validate(&Geometry::Label(label)).is_err(), "no style");

        let mut leader = LeaderGeometry {
            vertices: vec![p(0.0, 0.0)],
            ..LeaderGeometry::default()
        };
        assert!(
            validate(&Geometry::Leader(leader.clone())).is_err(),
            "one vertex"
        );
        leader.vertices.push(p(0.0, 0.0));
        assert!(
            validate(&Geometry::Leader(leader.clone())).is_err(),
            "no length"
        );
        leader.vertices[1] = p(3.0, 4.0);
        assert!(validate(&Geometry::Leader(leader.clone())).is_ok());
        leader.arrow_size = -1.0;
        assert!(validate(&Geometry::Leader(leader)).is_err());
    }

    #[test]
    fn transforms_move_the_points_and_keep_the_paper_sizes() {
        let shift = Mat3::translation(Vec2::new(10.0, 20.0));
        let leader = Geometry::Leader(LeaderGeometry {
            vertices: vec![p(0.0, 0.0), p(3.0, 4.0)],
            arrow_size: 2.5,
            ..LeaderGeometry::default()
        });
        let moved = transformed(&leader, &(Mat3::scaling(2.0, 2.0) * shift)).unwrap();
        let Geometry::Leader(out) = &moved else {
            panic!("not a leader")
        };
        assert_eq!(out.vertices[1], p(26.0, 48.0));
        // Paper millimetres do not scale with the model.
        double_eq(out.arrow_size, 2.5);

        // A mirrored angular dimension measures the same angle: its rays swap.
        let angular = Geometry::Dimension(DimensionGeometry {
            kind: DimensionKind::Angular,
            vertex: p(0.0, 0.0),
            start: p(1.0, 0.0),
            end: p(0.0, 1.0),
            start_ref: anchor(5, AnchorPoint::End, 0),
            ..DimensionGeometry::default()
        });
        let mirrored = transformed(&angular, &Mat3::scaling(-1.0, 1.0)).unwrap();
        let Geometry::Dimension(m) = &mirrored else {
            panic!("not a dimension")
        };
        near(m.measurement(), 0.5 * PI, 1e-12);
        assert_eq!(m.end_ref.entity, 5, "the reference goes with its point");
    }

    #[test]
    fn json_and_blob_round_trip_every_new_member() {
        let text = TextGeometry {
            style: String::from("Road"),
            paper_height: 3.5,
            justify: TextJustify::MiddleCentre,
            ..TextGeometry::new(p(1.0, 2.0), "two\nlines", 1.25, 0.3)
        };
        let dimension = DimensionGeometry {
            kind: DimensionKind::Angular,
            start: p(10.0, 0.0),
            end: p(0.0, 10.0),
            offset: 4.0,
            vertex_ref: anchor(9, AnchorPoint::SegmentMid, 2),
            ..DimensionGeometry::default()
        };
        let label = LabelGeometry {
            target: 4,
            part: 2,
            style: String::from("Bearing"),
            anchor: p(5.0, 6.0),
            position: Some(p(7.0, 8.0)),
            text_override: String::from("X"),
            rule: String::from("bdy"),
            ..LabelGeometry::default()
        };
        let leader = LeaderGeometry {
            vertices: vec![p(0.0, 0.0), p(3.0, 4.0), p(6.0, 4.0)],
            text: String::from("PIT 12\nIL 10.5"),
            arrow: ArrowHead::Dot,
            callout: CalloutShape::Box,
            style: String::from("Road"),
            paper_height: 3.0,
            arrow_size: 2.0,
            landing: 0.0,
            tip_ref: anchor(3, AnchorPoint::Vertex, 7),
            ..LeaderGeometry::default()
        };
        for geometry in [
            Geometry::Text(text),
            Geometry::Dimension(dimension),
            Geometry::Label(label),
            Geometry::Leader(leader),
        ] {
            let json = geometry_to_json(&geometry).unwrap();
            let back = geometry_from_json(&json).unwrap_or_else(|_| panic!("{json}"));
            assert_eq!(back, geometry, "{json}");

            let blob = geometry_to_blob(&geometry).unwrap();
            assert_eq!(blob[0], BLOB_VERSION_ANNOTATION);
            let from_blob =
                geometry_from_blob(&blob).unwrap_or_else(|error| panic!("{}", error.describe()));
            assert_eq!(from_blob, geometry);
        }
    }

    #[test]
    fn explicit_leader_landings_survive_storage_and_follow_rotations_and_mirrors() {
        let mut leader = LeaderGeometry {
            vertices: vec![p(0.0, 0.0), p(3.0, 4.0)],
            text: String::from("Note"),
            landing_angle: Some(0.0),
            ..LeaderGeometry::default()
        };
        assert!(validate(&Geometry::Leader(leader.clone())).is_ok());
        let turned =
            transformed(&Geometry::Leader(leader.clone()), &Mat3::rotation(PI / 2.0)).unwrap();
        let Geometry::Leader(turned) = turned else {
            panic!("not a leader")
        };
        near(turned.landing_angle.unwrap(), PI / 2.0, 1e-12);
        let mirrored =
            transformed(&Geometry::Leader(leader.clone()), &Mat3::scaling(-1.0, 1.0)).unwrap();
        let Geometry::Leader(mirrored) = mirrored else {
            panic!("not a leader")
        };
        near(mirrored.landing_angle.unwrap().abs(), PI, 1e-12);

        let json = geometry_to_json(&Geometry::Leader(leader.clone())).unwrap();
        assert!(json.contains("\"landingAngle\":0"), "{json}");
        let back = geometry_from_json(&json).unwrap();
        assert_eq!(back, Geometry::Leader(leader.clone()));
        leader.landing_angle = Some(f64::INFINITY);
        assert!(validate(&Geometry::Leader(leader.clone())).is_err());
        leader.landing_angle = Some(f64::NAN);
        assert!(validate(&Geometry::Leader(leader)).is_err());
    }

    #[test]
    fn an_old_text_or_dimension_is_stored_exactly_as_before() {
        // Byte for byte the version-1 layout: 2 header bytes, the point (16),
        // height and rotation (16), and the string (4 + 4 bytes).
        let text = Geometry::Text(TextGeometry::new(p(1.0, 2.0), "ABCD", 2.5, 0.0));
        let blob = geometry_to_blob(&text).unwrap();
        assert_eq!(blob[0], BLOB_VERSION);
        assert_eq!(blob.len(), 2 + 16 + 16 + 4 + 4);
        let json = geometry_to_json(&text).unwrap();
        assert!(!json.contains("justify"), "no new member in the old JSON");

        let dimension =
            Geometry::Dimension(DimensionGeometry::new(p(0.0, 0.0), p(10.0, 0.0), 2.0, ""));
        let dimension_blob = geometry_to_blob(&dimension).unwrap();
        assert_eq!(dimension_blob[0], BLOB_VERSION);
    }

    #[test]
    fn a_version_two_blob_cut_short_is_refused() {
        let leader = Geometry::Leader(LeaderGeometry {
            vertices: vec![p(0.0, 0.0), p(3.0, 4.0)],
            text: String::from("A"),
            ..LeaderGeometry::default()
        });
        let mut blob = geometry_to_blob(&leader).unwrap();
        blob.pop();
        assert!(geometry_from_blob(&blob).is_err());
        // A label in a version-1 blob cannot have been written by anything.
        let mut label = geometry_to_blob(&Geometry::Label(label_on(1, "S"))).unwrap();
        label[0] = BLOB_VERSION;
        assert!(geometry_from_blob(&label).is_err());
    }
}

// ---- anchors ----------------------------------------------------------------------------

mod anchors {
    use super::*;

    #[test]
    fn each_kind_offers_its_points() {
        let line = entity_of(1, Geometry::Line(Segment2::new(p(0.0, 0.0), p(10.0, 0.0))));
        assert_eq!(
            resolve_anchor(&line, &anchor(1, AnchorPoint::End, 0)),
            Some(p(10.0, 0.0))
        );
        assert_eq!(
            resolve_anchor(&line, &anchor(1, AnchorPoint::Mid, 0)),
            Some(p(5.0, 0.0))
        );
        assert!(
            resolve_anchor(&line, &anchor(1, AnchorPoint::Centre, 0)).is_none(),
            "a line has no centre"
        );

        let lot = entity_of(
            2,
            Geometry::Polyline(Polyline2::new(
                vec![p(0.0, 0.0), p(10.0, 0.0), p(10.0, 10.0), p(0.0, 10.0)],
                true,
            )),
        );
        assert_eq!(
            resolve_anchor(&lot, &anchor(2, AnchorPoint::Vertex, 2)),
            Some(p(10.0, 10.0))
        );
        assert_eq!(
            resolve_anchor(&lot, &anchor(2, AnchorPoint::SegmentMid, 3)),
            Some(p(0.0, 5.0)),
            "the closing segment of a closed polyline is the last"
        );
        assert!(resolve_anchor(&lot, &anchor(2, AnchorPoint::Vertex, 4)).is_none());

        let circle = entity_of(3, Geometry::Circle(Circle2::new(p(4.0, 4.0), 2.0)));
        assert_eq!(
            resolve_anchor(&circle, &anchor(3, AnchorPoint::Centre, 0)),
            Some(p(4.0, 4.0))
        );
        assert_eq!(describe(&anchor(2, AnchorPoint::Vertex, 2)), "vertex 2");
    }
}
