// Road templates as data (tests/cad/road/test_road_template.cpp;
// include/katana/cad/road/road_template.hpp): the standard ones, the checks, a
// kerb's shape worked by hand, and the JSON the project keeps them in.

use crate::{must, near};
use katana_cad::corridor::Assembly;
use katana_cad::road::road_template::{
    Component, ComponentKind, KerbFacing, KerbShape, RoadTemplate, RoadTemplateSet, ShapeColumn,
    Side, check_template, default_component, kerb_facing_from, kerb_shape, kerb_width, make_course,
    road_templates_from_json, road_templates_to_json, standard_templates, template_from_assembly,
};
use katana_core::{ErrorCode, Status};
use katana_entity::Color;
use katana_entity::entity::to_string;

fn component(kind: ComponentKind, side: Side, name: &str) -> Component {
    let mut made = default_component(kind, side);
    made.name = String::from(name);
    made
}

fn two_lanes() -> RoadTemplate {
    RoadTemplate {
        name: String::from("two lanes"),
        components: vec![
            component(ComponentKind::Lane, Side::Left, "lane"),
            component(ComponentKind::Batter, Side::Left, "batter"),
            component(ComponentKind::Lane, Side::Right, "lane"),
            component(ComponentKind::Batter, Side::Right, "batter"),
        ],
        ..RoadTemplate::default()
    }
}

fn column(out: f64, top: f64, bottom: f64) -> ShapeColumn {
    ShapeColumn { out, top, bottom }
}

fn expect_columns(shape: &[ShapeColumn], expected: &[ShapeColumn]) {
    for (i, (got, wanted)) in shape.iter().zip(expected).enumerate() {
        assert!((got.out - wanted.out).abs() <= 1e-15, "{i}");
        assert!((got.top - wanted.top).abs() <= 1e-15, "{i}");
        assert!((got.bottom - wanted.bottom).abs() <= 1e-15, "{i}");
    }
}

// The template's refusal: it must refuse.
#[track_caller]
fn refusal(status: Status) -> katana_core::Error {
    status.expect_err("the template was accepted")
}

mod road_template {
    use super::*;

    #[test]
    fn the_standard_templates_are_valid_and_each_is_the_mirror_of_itself() {
        let templates = standard_templates();
        assert_eq!(templates.len(), 3);
        assert_eq!(templates[0].name, "rural two-lane");
        assert_eq!(templates[1].name, "urban street");
        assert_eq!(templates[2].name, "divided road");
        for each in &templates {
            must(check_template(each));
            let left = each.side(Side::Left);
            let right = each.side(Side::Right);
            assert_eq!(left.len(), right.len(), "{}", each.name);
            for (i, (l, r)) in left.iter().zip(&right).enumerate() {
                let mut mirrored = (*r).clone();
                mirrored.side = Side::Left;
                assert_eq!(**l, mirrored, "{} component {i}", each.name);
            }
            // Every standard template ends each side at the ground.
            assert_eq!(
                left[left.len() - 1].kind,
                ComponentKind::Batter,
                "{}",
                each.name
            );
        }
    }

    #[test]
    fn a_course_takes_its_colour_from_its_material() {
        assert_eq!(
            make_course("AC14", 0.05, "asphalt", "PAVEMENT").colour,
            Color::new(60, 60, 60, 255)
        );
        assert_eq!(
            make_course("slab", 0.1, "Concrete", "PAVEMENT").colour,
            Color::new(200, 200, 190, 255)
        );
        assert_eq!(
            make_course("base", 0.2, "crushed rock", "PAVEMENT").colour,
            Color::new(170, 145, 105, 255)
        );
        assert_eq!(
            make_course("x", 0.2, "", "PAVEMENT").colour,
            Color::new(150, 150, 150, 255)
        );
        assert_eq!(make_course("x", 0.2, "", "PAVEMENT").ifc_type, "PAVEMENT");
    }

    #[test]
    fn a_barrier_kerb_is_its_face_battered_one_in_six_then_its_top() {
        // 150 mm face battered 1 in 6 is 25 mm out; a 150 mm top; its base 150
        // mm under the lip, level.
        let kerb = default_component(ComponentKind::Kerb, Side::Left);
        let shape = kerb_shape(&kerb);
        assert_eq!(shape.len(), 3);
        assert_eq!(shape[0], column(0.0, 0.0, -0.15));
        near(shape[1].out, 0.025, 1e-15);
        near(shape[1].top, 0.15, 1e-15);
        near(shape[2].out, 0.175, 1e-15);
        near(shape[2].top, 0.15, 1e-15);
        for each in &shape {
            near(each.bottom, -0.15, 1e-15);
        }
        near(kerb_width(&kerb), 0.175, 1e-15);
    }

    #[test]
    fn a_kerb_and_gutter_falls_to_its_face_and_keeps_its_depth_under_the_channel() {
        // A 300 mm gutter falling 6 % reaches its invert 18 mm down; the 150 mm
        // face then rises from there, battered 25 mm, to a 150 mm top. The base
        // is 150 mm under the lip, and under the invert from the invert on.
        let mut kerb = default_component(ComponentKind::Kerb, Side::Left);
        kerb.shape = KerbShape::KerbAndGutter;
        let shape = kerb_shape(&kerb);
        assert_eq!(shape.len(), 4);
        near(shape[1].out, 0.300, 1e-15);
        near(shape[1].top, -0.018, 1e-15);
        near(shape[1].bottom, -0.168, 1e-15);
        near(shape[2].out, 0.325, 1e-15);
        near(shape[2].top, 0.132, 1e-15);
        near(shape[3].out, 0.475, 1e-15);
        near(shape[3].top, 0.132, 1e-15);
        near(shape[0].bottom, -0.15, 1e-15);
        near(kerb_width(&kerb), 0.475, 1e-15);
    }

    #[test]
    fn a_mountable_kerbs_face_runs_one_and_a_half_times_its_height() {
        let mut kerb = default_component(ComponentKind::Kerb, Side::Left);
        kerb.shape = KerbShape::Mountable;
        kerb.height = 0.1;
        kerb.width = 0.1;
        let shape = kerb_shape(&kerb);
        assert_eq!(shape.len(), 3);
        near(shape[1].out, 0.15, 1e-15);
        near(shape[1].top, 0.1, 1e-15);
        near(kerb_width(&kerb), 0.25, 1e-15);
    }

    #[test]
    fn a_kerb_facing_outward_is_the_same_kerb_from_its_lip_back_to_its_back() {
        // A mountable kerb 100 mm high with a 150 mm top and 150 mm base depth:
        // the face runs 1.5 x 0.1 = 0.15 m, so 0.3 m in all. Facing outward it
        // starts at its back at level 0, the top runs 0.15 m flat, and the face
        // falls 0.1 m over the next 0.15 m to the lip; the base is 0.15 m under
        // the lip at -0.1, so -0.25 throughout.
        let mut kerb = default_component(ComponentKind::Kerb, Side::Left);
        kerb.shape = KerbShape::Mountable;
        kerb.height = 0.1;
        kerb.faces = KerbFacing::Outward;
        let shape = kerb_shape(&kerb);
        assert_eq!(shape.len(), 3);
        expect_columns(
            &shape,
            &[
                column(0.0, 0.0, -0.25),
                column(0.15, 0.0, -0.25),
                column(0.30, -0.1, -0.25),
            ],
        );
        near(kerb_width(&kerb), 0.3, 1e-15);
        // The same kerb facing inward is as wide.
        kerb.faces = KerbFacing::Inward;
        near(kerb_width(&kerb), 0.3, 1e-15);

        // A barrier: a 150 mm face battered 1 in 6 is 25 mm out; 150 mm of top
        // first, then down 0.15 over 0.025 to the lip; its base 0.15 under the
        // lip.
        let mut barrier = default_component(ComponentKind::Kerb, Side::Left);
        barrier.faces = KerbFacing::Outward;
        let fall = kerb_shape(&barrier);
        assert_eq!(fall.len(), 3);
        near(fall[1].out, 0.15, 1e-15);
        near(fall[1].top, 0.0, 1e-15);
        near(fall[2].out, 0.175, 1e-15);
        near(fall[2].top, -0.15, 1e-15);
        for each in &fall {
            near(each.bottom, -0.30, 1e-15);
        }

        // Kerb and gutter, 300 mm at 6 % (the invert 18 mm below the lip), a 150
        // mm face: facing inward the lip is 0, the invert -0.018, the top 0.132.
        // Facing outward everything is measured from that top: the face crest
        // 0.15 out at 0, the invert at 0.175 and -0.15, the lip at 0.475 and
        // -0.132 (the channel rises 18 mm back to it); the base 0.15 under the
        // channel throughout: -0.30 to the invert, -0.282 under the lip.
        let mut channel = default_component(ComponentKind::Kerb, Side::Left);
        channel.shape = KerbShape::KerbAndGutter;
        channel.faces = KerbFacing::Outward;
        let gutter = kerb_shape(&channel);
        assert_eq!(gutter.len(), 4);
        expect_columns(
            &gutter,
            &[
                column(0.0, 0.0, -0.30),
                column(0.15, 0.0, -0.30),
                column(0.175, -0.15, -0.30),
                column(0.475, -0.132, -0.282),
            ],
        );
    }

    #[test]
    fn only_an_edge_strip_of_the_kerbs_can_be_superelevated_and_no_component_may_be_named_earthworks()
     {
        let mut kerbs = two_lanes();
        let mut kerb = default_component(ComponentKind::Kerb, Side::Left);
        kerb.superelevated = true;
        kerbs.components.insert(1, kerb);
        for shape in [
            KerbShape::Barrier,
            KerbShape::Mountable,
            KerbShape::KerbAndGutter,
        ] {
            kerbs.components[1].shape = shape;
            let refused = refusal(check_template(&kerbs));
            assert_eq!(
                refused.code,
                ErrorCode::InvalidArgument,
                "{}",
                to_string(shape)
            );
            assert!(
                refused.message.contains("only an edge strip has"),
                "{}",
                refused.message
            );
            assert!(refused.message.contains("left:kerb"));
        }
        kerbs.components[1].shape = KerbShape::EdgeStrip;
        assert!(check_template(&kerbs).is_ok());

        // The corridor's earthworks objects have ids of their own, in any case.
        for name in ["earthworks", "Earthworks", "EARTHWORKS"] {
            let mut named = two_lanes();
            named.components[0].name = String::from(name);
            let refused = refusal(check_template(&named));
            assert!(
                refused.message.contains("earthworks is the name"),
                "{name}: {}",
                refused.message
            );
        }
        let mut close = two_lanes();
        close.components[0].name = String::from("earthwork");
        assert!(check_template(&close).is_ok());
    }

    #[test]
    fn the_divided_roads_median_kerb_falls_to_the_carriageway_and_an_edge_strip_takes_no_facing() {
        let templates = standard_templates();
        let divided = &templates[2];
        assert_eq!(divided.name, "divided road");
        for side in [Side::Left, Side::Right] {
            let kerb = divided
                .find(side, "kerb")
                .expect("the divided road has a kerb");
            assert_eq!(divided.components[kerb].faces, KerbFacing::Outward);
        }
        // The other standard templates' kerbs are on the carriageway's outer
        // edge.
        for each in &templates[1].components {
            if each.kind == ComponentKind::Kerb {
                assert_eq!(each.faces, KerbFacing::Inward);
            }
        }
        // A flush strip has no face to turn.
        let mut strip = two_lanes();
        let mut edge = default_component(ComponentKind::Kerb, Side::Left);
        edge.shape = KerbShape::EdgeStrip;
        edge.faces = KerbFacing::Outward;
        strip.components.insert(1, edge);
        let refused = refusal(check_template(&strip));
        assert_eq!(refused.code, ErrorCode::InvalidArgument);
        assert!(refused.message.contains("no face"), "{}", refused.message);
        assert!(refused.message.contains("left:kerb"));
        strip.components[1].faces = KerbFacing::Inward;
        assert!(check_template(&strip).is_ok());
        assert_eq!(to_string(KerbFacing::Outward), "outward");
        assert_eq!(kerb_facing_from("INWARD"), Some(KerbFacing::Inward));
        assert!(kerb_facing_from("sideways").is_none());
    }

    #[test]
    fn an_edge_strip_is_flush_at_its_crossfall() {
        let mut strip = default_component(ComponentKind::Kerb, Side::Left);
        strip.shape = KerbShape::EdgeStrip;
        strip.width = 0.3;
        strip.crossfall = -2.0;
        let shape = kerb_shape(&strip);
        assert_eq!(shape.len(), 2);
        near(shape[1].top, -0.006, 1e-15); // 300 mm at -2 %
        near(shape[1].bottom, -0.156, 1e-15);
    }

    #[test]
    fn check_refuses_what_cannot_be_built_naming_the_component() {
        assert!(check_template(&two_lanes()).is_ok());

        let mut twice = two_lanes();
        twice
            .components
            .insert(0, component(ComponentKind::Lane, Side::Left, "LANE"));
        let duplicate = refusal(check_template(&twice));
        assert_eq!(duplicate.code, ErrorCode::InvalidArgument);
        // The second of the two is the one refused, named as it is written.
        assert!(duplicate.message.contains("left:lane"));

        let mut outside = two_lanes();
        outside
            .components
            .insert(2, component(ComponentKind::Verge, Side::Left, "verge"));
        assert!(
            refusal(check_template(&outside))
                .message
                .contains("outside the batter")
        );

        let mut narrow = two_lanes();
        narrow.components[0].width = 0.0;
        assert_eq!(
            refusal(check_template(&narrow)).code,
            ErrorCode::InvalidArgument
        );

        let mut not_a_number = two_lanes();
        not_a_number.components[0].crossfall = f64::NAN;
        assert!(
            refusal(check_template(&not_a_number))
                .message
                .contains("finite")
        );

        let mut steep = two_lanes();
        steep.components[0].crossfall = 101.0;
        assert!(check_template(&steep).is_err());

        let mut coursed_kerb = two_lanes();
        coursed_kerb
            .components
            .insert(1, component(ComponentKind::Kerb, Side::Left, "kerb"));
        coursed_kerb.components[1].courses = vec![make_course("x", 0.1, "concrete", "PAVEMENT")];
        assert!(
            refusal(check_template(&coursed_kerb))
                .message
                .contains("no courses")
        );

        let mut bad_word = two_lanes();
        bad_word.components[0].name = String::from("lane one");
        assert!(check_template(&bad_word).is_err());

        let mut half_bench = two_lanes();
        half_bench.components[1].bench_height = 1.0;
        assert!(
            refusal(check_template(&half_bench))
                .message
                .contains("bench")
        );

        let mut thin_course = two_lanes();
        thin_course.components[0].courses = vec![make_course("AC", 0.0, "asphalt", "PAVEMENT")];
        assert!(
            refusal(check_template(&thin_course))
                .message
                .contains("thickness")
        );

        let mut slash = two_lanes();
        slash.name = String::from("a/b");
        assert!(check_template(&slash).is_err());
    }

    #[test]
    fn a_bench_too_small_to_build_is_refused_naming_the_batter_and_the_numbers() {
        // Batters at 2 and 4 run per rise, 50 m reach.
        let benched = |height: f64, width: f64| {
            let mut made = two_lanes();
            for each in &mut made.components {
                if each.kind == ComponentKind::Batter {
                    each.bench_height = height;
                    each.bench_width = width;
                }
            }
            made
        };
        // A bench every 0.125 m of rise at the flatter 1 in 2 is 0.25 m of
        // slope, and a 0.25 m flat: a bench every 0.5 m, 50 / 0.5 = 100 across
        // the 50 m - exactly the most. All binary fractions, so exact.
        assert!(check_template(&benched(0.125, 0.25)).is_ok());
        // A hair less flat is 50 / 0.4999 = 100.02 benches: over.
        let over = refusal(check_template(&benched(0.125, 0.2499)));
        assert_eq!(over.code, ErrorCode::InvalidArgument);
        assert!(over.message.contains("left:batter"), "{}", over.message);
        assert!(over.message.contains("100 benches"), "{}", over.message);
        assert!(over.message.contains("0.2499"), "{}", over.message);
        // A real batter's bench - 5 m of rise, a 3 m flat - and none at all.
        assert!(check_template(&benched(5.0, 3.0)).is_ok());
        assert!(check_template(&benched(0.0, 0.0)).is_ok());
        // The millimetre typo of the report, and a value so small that adding
        // it to a running distance changed nothing and the bench loop never
        // ended.
        assert!(check_template(&benched(0.001, 0.001)).is_err());
        assert!(check_template(&benched(1e-300, 1e-300)).is_err());
        // A denormal: the period underflows.
        assert!(check_template(&benched(1e-320, 1e-320)).is_err());
        // A short reach has few benches: a 0.05 m bench (period 0.05 x 2 + 0.05
        // = 0.15 m) over a 50 m reach is 333, over a 5 m reach 33.
        assert!(check_template(&benched(0.05, 0.05)).is_err());
        let mut short_reach = benched(0.05, 0.05);
        for each in &mut short_reach.components {
            if each.kind == ComponentKind::Batter {
                each.maximum_width = 5.0;
            }
        }
        assert!(check_template(&short_reach).is_ok());
    }

    #[test]
    fn an_enormous_length_is_refused_and_the_limit_itself_is_accepted() {
        // 1000 m: the most; the half-metre sampling across a section is linear
        // in a width, and past 4.5e15 m it never moved.
        let with = |change: &dyn Fn(&mut RoadTemplate)| {
            let mut made = two_lanes();
            change(&mut made);
            check_template(&made)
        };
        assert!(with(&|t: &mut RoadTemplate| t.components[0].width = 1000.0).is_ok());
        for huge in [1000.001, 1e6, 1e16, 4.6e15] {
            let lane = refusal(with(&|t: &mut RoadTemplate| t.components[0].width = huge));
            assert_eq!(lane.code, ErrorCode::InvalidArgument, "{huge}");
            assert!(lane.message.contains("at most 1000 m"), "{}", lane.message);
            assert!(lane.message.contains("left:lane"));
        }
        assert!(with(&|t: &mut RoadTemplate| t.components[1].maximum_width = 5000.0).is_err());
        assert!(with(&|t: &mut RoadTemplate| t.components[1].maximum_width = 1000.0).is_ok());
        assert!(
            with(&|t: &mut RoadTemplate| {
                t.components[0].courses = vec![make_course("slab", 2000.0, "concrete", "PAVEMENT")];
            })
            .is_err()
        );
        // A kerb's dimensions and a drain's.
        assert!(
            with(&|t: &mut RoadTemplate| {
                let mut kerb = default_component(ComponentKind::Kerb, Side::Left);
                kerb.width = 1e9;
                t.components.insert(1, kerb);
            })
            .is_err()
        );
        assert!(
            with(&|t: &mut RoadTemplate| {
                let mut drain = default_component(ComponentKind::Drain, Side::Left);
                drain.depth = 1e9;
                t.components.insert(1, drain);
            })
            .is_err()
        );
    }

    #[test]
    fn an_empty_template_is_accepted_to_be_filled_in() {
        let empty = RoadTemplate {
            name: String::from("new"),
            ..RoadTemplate::default()
        };
        assert!(check_template(&empty).is_ok());
    }

    #[test]
    fn the_assembly_is_a_lane_and_a_batter_each_side_with_its_crossfall_made_outward_percent() {
        let assembly = Assembly {
            half_width: 4.0,
            crossfall: 0.025, // falls away: -2.5 % outward
            cut_batter: 1.5,
            fill_batter: 3.0,
            maximum_batter_width: 30.0,
        };
        let made = template_from_assembly(&assembly);
        must(check_template(&made));
        for side in [Side::Left, Side::Right] {
            let components = made.side(side);
            assert_eq!(components.len(), 2);
            assert_eq!(components[0].kind, ComponentKind::Lane);
            assert_eq!(components[0].width, 4.0);
            near(components[0].crossfall, -2.5, 1e-12);
            assert!(components[0].courses.is_empty());
            assert!(!components[0].superelevated);
            assert_eq!(components[1].kind, ComponentKind::Batter);
            assert_eq!(components[1].cut_slope, 1.5);
            assert_eq!(components[1].fill_slope, 3.0);
            assert_eq!(components[1].maximum_width, 30.0);
        }
    }

    #[test]
    fn the_json_round_trips_every_value_exactly() {
        let mut set = RoadTemplateSet {
            templates: standard_templates(),
        };
        // Values with no short decimal form, to prove the doubles are exact.
        set.templates[0].components[0].width = 0.1 + 0.2;
        set.templates[0].components[0].courses[0].thickness = 1.0 / 3.0;
        set.templates[0].description = String::from("line one, \"quoted\"");
        let last = set.templates[1]
            .components
            .last_mut()
            .expect("the urban street has components");
        last.bench_height = 1.25;
        last.bench_width = 2.5;
        let json = must(road_templates_to_json(&set));
        let read = must(road_templates_from_json(&json));
        assert_eq!(read, set);
        assert!(json.contains("\"format\":\"katana-road-templates\""));
    }

    #[test]
    fn a_kerbs_facing_is_kept_in_the_json_and_an_older_file_without_one_reads_inward() {
        let set = RoadTemplateSet {
            templates: standard_templates(),
        };
        let json = must(road_templates_to_json(&set));
        assert!(json.contains("\"faces\":\"outward\""));
        assert!(json.contains("\"faces\":\"inward\""));
        let read = must(road_templates_from_json(&json));
        assert_eq!(read, set);

        // A file written before kerbs had a facing: its kerb reads as it always
        // did.
        let older = must(road_templates_from_json(
            r#"{"format":"katana-road-templates","version":1,"templates":[{"name":"t","components":[{"name":"kerb","kind":"kerb","side":"left","shape":"mountable","width":0.15,"height":0.1,"depth":0.15}]}]}"#,
        ));
        assert_eq!(older.templates[0].components[0].faces, KerbFacing::Inward);
        // A word that is neither is a parse failure naming it.
        let bad = road_templates_from_json(
            r#"{"format":"katana-road-templates","version":1,"templates":[{"name":"t","components":[{"name":"kerb","kind":"kerb","side":"left","faces":"sideways"}]}]}"#,
        )
        .expect_err("sideways is no facing");
        assert_eq!(bad.code, ErrorCode::ParseFailure);
    }

    #[test]
    fn the_json_of_a_newer_version_is_refused_as_unsupported() {
        let read = road_templates_from_json(
            r#"{"format":"katana-road-templates","version":99,"templates":[]}"#,
        )
        .expect_err("version 99 is newer");
        assert_eq!(read.code, ErrorCode::Unsupported);
    }

    #[test]
    fn text_that_is_not_a_template_set_is_a_parse_failure() {
        assert_eq!(
            road_templates_from_json("not json")
                .expect_err("not JSON")
                .code,
            ErrorCode::ParseFailure
        );
        assert_eq!(
            road_templates_from_json(r#"{"format":"katana-sheets","version":1}"#)
                .expect_err("sheets")
                .code,
            ErrorCode::ParseFailure
        );
        assert_eq!(
            road_templates_from_json(
                r#"{"format":"katana-road-templates","version":1,"templates":[{"name":"t","components":[{"name":"x","kind":"tunnel","side":"left"}]}]}"#
            )
            .expect_err("a tunnel is no component")
            .code,
            ErrorCode::ParseFailure
        );
        // Two templates of one name, ignoring case.
        assert_eq!(
            road_templates_from_json(
                r#"{"format":"katana-road-templates","version":1,"templates":[{"name":"T","components":[]},{"name":"t","components":[]}]}"#
            )
            .expect_err("two of one name")
            .code,
            ErrorCode::ParseFailure
        );
    }

    #[test]
    fn an_empty_set_is_written_and_read_back() {
        let json = must(road_templates_to_json(&RoadTemplateSet::default()));
        let read = must(road_templates_from_json(&json));
        assert!(read.templates.is_empty());
    }
}
