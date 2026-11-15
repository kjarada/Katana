// One section of a road template (tests/cad/road/test_road_section.cpp;
// include/katana/cad/road/road_section.hpp), against offsets, levels and areas
// worked by hand from the template's widths, crossfalls and depths. The ground
// is a plane, which the TIN reproduces exactly, so the only tolerance is the
// daylight's bisection.

use crate::road_fixtures::{batters_alone, lane_template, level, plane_ground, straight};
use crate::{must, near};
use katana_cad::road::corridor_model::{
    Corridor, Crossfalls, SuperelevationPoint, superelevation_at,
};
use katana_cad::road::road_section::{
    SectionPoint, road_section, section_stations, section_stations_along, template_section,
};
use katana_cad::road::road_template::{
    ComponentKind, KerbShape, RoadTemplate, Side, default_component, make_course,
    standard_templates,
};
use katana_core::ErrorCode;
use katana_geometry::alignment::solve_alignment;
use katana_geometry::{AlignmentPi, HorizontalAlignment, Point2, SolvedAlignment};
use katana_math::cpp;

// Lane 3.5 m at -3 % (AC14 50 mm over a 200 mm base), shoulder 1.5 m at -4 %
// (a 50 mm seal), a barrier kerb as it comes, a 1.5 m footpath rising 2.5 %;
// both sides alike, no batter.
fn street() -> RoadTemplate {
    let mut made = RoadTemplate {
        name: String::from("street"),
        ..RoadTemplate::default()
    };
    for side in [Side::Left, Side::Right] {
        let mut lane = default_component(ComponentKind::Lane, side);
        lane.width = 3.5;
        lane.crossfall = -3.0;
        lane.courses = vec![
            make_course("AC14", 0.05, "asphalt", "PAVEMENT"),
            make_course("base", 0.2, "rock", "PAVEMENT"),
        ];
        let mut shoulder = default_component(ComponentKind::Shoulder, side);
        shoulder.width = 1.5;
        shoulder.crossfall = -4.0;
        shoulder.courses = vec![make_course("seal", 0.05, "asphalt", "PAVEMENT")];
        let kerb = default_component(ComponentKind::Kerb, side);
        let mut path = default_component(ComponentKind::Footpath, side);
        path.width = 1.5;
        path.crossfall = 2.5;
        path.courses.clear();
        made.components.extend([lane, shoulder, kerb, path]);
    }
    made
}

// (0,0) to (100,0) to (100,100), the corner rounded at 50 m - with a spiral of
// `spiral` metres into and out of the arc when it is given (the C++ defaults it
// to none): a quarter turn to the left, tangents of 100 m less the curve's own.
fn quarter_turn(spiral: f64) -> SolvedAlignment {
    let definition = HorizontalAlignment {
        pis: vec![
            AlignmentPi {
                point: Point2::new(0.0, 0.0),
                ..AlignmentPi::default()
            },
            AlignmentPi {
                point: Point2::new(100.0, 0.0),
                radius: 50.0,
                spiral_in: spiral,
                spiral_out: spiral,
            },
            AlignmentPi {
                point: Point2::new(100.0, 100.0),
                ..AlignmentPi::default()
            },
        ],
        ..HorizontalAlignment::default()
    };
    must(solve_alignment(&definition))
}

// How far the alignment turns between two stations, radians: the difference of
// its directions there, taken the short way round. Worked from the directions,
// not from the curvature section_stations_along reads, so it is an independent
// measure of what a step does.
fn turn_between(alignment: &SolvedAlignment, from: f64, to: f64) -> f64 {
    let difference = alignment
        .direction_at_station(to)
        .expect("on the alignment")
        - alignment
            .direction_at_station(from)
            .expect("on the alignment");
    difference.sin().atan2(difference.cos()).abs()
}

fn point_coded<'a>(line: &'a [SectionPoint], code: &str) -> Option<&'a SectionPoint> {
    line.iter().find(|point| point.code == code)
}

#[track_caller]
fn expect_point(line: &[SectionPoint], code: &str, offset: f64, level: f64) {
    let Some(point) = point_coded(line, code) else {
        panic!("no point {code}");
    };
    assert!(
        (point.offset - offset).abs() <= 1e-12,
        "{code}: offset {} against {offset}",
        point.offset
    );
    assert!(
        (point.level - level).abs() <= 1e-12,
        "{code}: level {} against {level}",
        point.level
    );
}

mod road_section {
    use super::*;

    #[test]
    fn a_street_is_set_out_at_its_hand_worked_offsets_and_levels() {
        // At design level 10: the lane falls 3.5 x 3 % = 0.105 to 9.895; the
        // shoulder 1.5 x 4 % = 0.06 more to 9.835 at 5.0; the kerb's face rises
        // 0.15 to 9.985 over 0.025 (battered 1 in 6), its top runs 0.15 to
        // 5.175; the footpath rises 1.5 x 2.5 % = 0.0375 to 10.0225 at 6.675.
        let section = must(road_section(
            &street(),
            &straight(200.0),
            &level(10.0, 0.0, 200.0),
            None,
            100.0,
            None,
        ));
        let top = &section.top;
        assert_eq!(top.len(), 11); // the centreline and five points a side
        expect_point(top, "CL", 0.0, 10.0);
        expect_point(top, "left/lane/outer", 3.5, 9.895);
        expect_point(top, "left/shoulder/outer", 5.0, 9.835);
        expect_point(top, "left/kerb/face", 5.025, 9.985);
        expect_point(top, "left/kerb/back", 5.175, 9.985);
        expect_point(top, "left/footpath/outer", 6.675, 10.0225);
        expect_point(top, "right/lane/outer", -3.5, 9.895);
        expect_point(top, "right/footpath/outer", -6.675, 10.0225);
        // Left to right.
        assert!(top.windows(2).all(|pair| pair[1].offset <= pair[0].offset));
        assert_eq!(top[0].code, "left/footpath/outer");
        assert_eq!(top[top.len() - 1].code, "right/footpath/outer");
        near(section.design_level, 10.0, 1e-12);
        near(section.centre.x, 100.0, 1e-12);
        // No batter: there is nothing to reach, and no ground to measure
        // against.
        assert!(section.left_reached);
        assert!(section.right_reached);
        assert!(!section.complete);
        assert!(section.ground.is_empty());
        assert_eq!(section.cut_area, 0.0);
    }

    #[test]
    fn the_divided_roads_lanes_sit_under_the_median_and_its_kerb_faces_the_traffic() {
        // The divided road on a level road at 0: the median 2 m wide at 0 % (its
        // top is the profile), then the mountable kerb facing outward: 0.15 m of
        // top at level 0, the face falling 0.1 m over 1.5 x 0.1 = 0.15 m to the
        // lip at 2.3, -0.1. The carriageway starts at the lip: lane 1 3.5 m at
        // -3 % falls 0.105 to 5.8 at -0.205, lane 2 to 9.3 at -0.31, the 2.5 m
        // shoulder 0.075 more to 11.8 at -0.385. (Facing the other way the kerb
        // rose to 0.1 and the lanes started 100 mm ABOVE the profile, with a
        // drop to the median.)
        let mut divided = RoadTemplate::default();
        for each in standard_templates() {
            if each.name == "divided road" {
                divided = each;
            }
        }
        assert_eq!(divided.name, "divided road");
        let section = must(template_section(&divided));
        assert_eq!(section.top.len(), 13); // the centreline and six points a side
        for sign in [1.0, -1.0] {
            let side = if sign > 0.0 { "left" } else { "right" };
            expect_point(
                &section.top,
                &format!("{side}/median/outer"),
                sign * 2.0,
                0.0,
            );
            expect_point(&section.top, &format!("{side}/kerb/face"), sign * 2.15, 0.0);
            expect_point(&section.top, &format!("{side}/kerb/lip"), sign * 2.3, -0.1);
            expect_point(
                &section.top,
                &format!("{side}/lane1/outer"),
                sign * 5.8,
                -0.205,
            );
            expect_point(
                &section.top,
                &format!("{side}/lane2/outer"),
                sign * 9.3,
                -0.31,
            );
            expect_point(
                &section.top,
                &format!("{side}/shoulder/outer"),
                sign * 11.8,
                -0.385,
            );
        }
        // The back is the median's edge.
        assert!(point_coded(&section.top, "left/kerb/back").is_none());
        // The kerb's base is 0.15 under its lip at -0.1: -0.25 throughout, and
        // it is one solid of three columns that ends where the lane starts.
        let kerb = section
            .solids
            .iter()
            .find(|solid| solid.key == "left/kerb")
            .expect("the left kerb is a solid");
        assert_eq!(kerb.columns.len(), 3);
        near(kerb.columns[0].offset, 2.0, 1e-12);
        near(kerb.columns[kerb.columns.len() - 1].offset, 2.3, 1e-12);
        for each in &kerb.columns {
            near(each.bottom, -0.25, 1e-12);
        }
    }

    #[test]
    fn each_course_and_kerb_is_a_solid_under_its_component() {
        let section = must(road_section(
            &street(),
            &straight(200.0),
            &level(10.0, 0.0, 200.0),
            None,
            100.0,
            None,
        ));
        // Lane AC14 and base, shoulder seal, the kerb: four a side.
        assert_eq!(section.solids.len(), 8);
        let solid = |key: &str| section.solids.iter().find(|each| each.key == key);
        let wearing = solid("left/lane/AC14").expect("the wearing course");
        assert_eq!(wearing.columns.len(), 2);
        near(wearing.columns[0].top, 10.0, 1e-12);
        near(wearing.columns[0].bottom, 9.95, 1e-12);
        near(wearing.columns[1].offset, 3.5, 1e-12);
        near(wearing.columns[1].top, 9.895, 1e-12);
        near(wearing.columns[1].bottom, 9.845, 1e-12);
        let base = solid("right/lane/base").expect("the base course");
        near(base.columns[1].offset, -3.5, 1e-12);
        near(base.columns[1].top, 9.845, 1e-12);
        near(base.columns[1].bottom, 9.645, 1e-12);
        assert!(base.course.is_some());
        let kerb = solid("left/kerb").expect("the kerb");
        assert!(kerb.course.is_none());
        assert_eq!(kerb.columns.len(), 3);
        for each in &kerb.columns {
            near(each.bottom, 9.835 - 0.15, 1e-12); // 150 mm under the lip
        }
    }

    #[test]
    fn the_formation_steps_a_millimetre_where_the_depth_changes_and_stays_a_function_of_offset() {
        // Under the lane the courses are 250 mm deep (9.75 at the centreline,
        // 9.645 at its edge), under the shoulder 50 mm (9.845 at its inner edge,
        // 9.785 at its outer), under the kerb its base 9.685, under the footpath
        // nothing (its top). At each change the shallower corner moves FACE_RUN
        // (1 mm) into its own component.
        let section = must(road_section(
            &street(),
            &straight(200.0),
            &level(10.0, 0.0, 200.0),
            None,
            100.0,
            None,
        ));
        let mut left: Vec<(f64, f64)> = section
            .formation
            .iter()
            .filter(|point| point.offset >= 0.0)
            .map(|point| (point.offset, point.level))
            .collect();
        left.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
        let expected: [(f64, f64); 9] = [
            (0.0, 9.75),
            (3.5, 9.645),
            (3.501, 9.845),
            (4.999, 9.785),
            (5.0, 9.685),
            (5.025, 9.685),
            (5.175, 9.685),
            (5.176, 9.985),
            (6.675, 10.0225),
        ];
        assert_eq!(left.len(), expected.len());
        for (i, (got, wanted)) in left.iter().zip(&expected).enumerate() {
            assert!((got.0 - wanted.0).abs() <= 1e-12, "{i}");
            assert!((got.1 - wanted.1).abs() <= 1e-12, "{i}");
        }
        // A function of offset all the way across: strictly falling left to
        // right.
        for (i, pair) in section.formation.windows(2).enumerate() {
            assert!(pair[0].offset > pair[1].offset, "{i}");
        }
    }

    #[test]
    fn superelevation_at_the_midpoint_of_its_transition_is_halfway_between_its_ends() {
        // From normal crown at 50 (-3, -3) to full superelevation at 100 (+3,
        // -3): at 75 the left lane is level, the right still falls 3 %.
        let corridor = Corridor {
            superelevation: vec![
                SuperelevationPoint {
                    station: 50.0,
                    left: -3.0,
                    right: -3.0,
                },
                SuperelevationPoint {
                    station: 100.0,
                    left: 3.0,
                    right: -3.0,
                },
            ],
            ..Corridor::default()
        };
        let halfway = superelevation_at(&corridor, 75.0).expect("75 is in the table");
        near(halfway.left, 0.0, 1e-15);
        near(halfway.right, -3.0, 1e-15);
        assert_eq!(
            superelevation_at(&corridor, 50.0),
            Some(Crossfalls {
                left: -3.0,
                right: -3.0
            })
        );
        assert_eq!(
            superelevation_at(&corridor, 100.0),
            Some(Crossfalls {
                left: 3.0,
                right: -3.0
            })
        );
        assert!(superelevation_at(&corridor, 49.0).is_none());
        assert!(superelevation_at(&corridor, 101.0).is_none());

        let section = must(road_section(
            &street(),
            &straight(200.0),
            &level(10.0, 0.0, 200.0),
            None,
            75.0,
            Some(halfway),
        ));
        expect_point(&section.top, "left/lane/outer", 3.5, 10.0);
        expect_point(&section.top, "right/lane/outer", -3.5, 9.895);
        // The shoulder is superelevated too: level on the left.
        expect_point(&section.top, "left/shoulder/outer", 5.0, 10.0);
        // A component that is not superelevated keeps its own: the footpath.
        expect_point(&section.top, "left/footpath/outer", 6.675, 10.15 + 0.0375);
    }

    #[test]
    fn an_edge_strip_and_a_drain_between_superelevated_components_follow_the_table() {
        // A lane 3.5 m, a flush edge strip 0.15 m, a shoulder 1.5 m, each side;
        // lane and shoulder superelevated by default. The table at the station
        // is (+6, -6) outward percent. Left: the lane rises 3.5 x 6 % = 0.21 to
        // 10.21, the strip - when it is superelevated too - 0.15 x 6 % = 0.009
        // to 10.219, the shoulder 1.5 x 6 % = 0.09 to 10.309. Right: 9.79, then
        // 0.009 down to 9.781, 0.09 more to 9.691. A strip left at its own 0 %
        // was flat at 10.21, and the shoulder's edge 9 mm low at 10.300.
        let stripped = |superelevated: bool| {
            let mut made = RoadTemplate {
                name: String::from("strip"),
                ..RoadTemplate::default()
            };
            for side in [Side::Left, Side::Right] {
                let mut lane = default_component(ComponentKind::Lane, side);
                lane.width = 3.5;
                lane.crossfall = -3.0;
                let mut strip = default_component(ComponentKind::Kerb, side);
                strip.shape = KerbShape::EdgeStrip;
                strip.crossfall = 0.0;
                strip.superelevated = superelevated;
                let mut shoulder = default_component(ComponentKind::Shoulder, side);
                shoulder.width = 1.5;
                shoulder.crossfall = -3.0;
                made.components.extend([lane, strip, shoulder]);
            }
            made
        };
        let table = Crossfalls {
            left: 6.0,
            right: -6.0,
        };
        let follows = must(road_section(
            &stripped(true),
            &straight(200.0),
            &level(10.0, 0.0, 200.0),
            None,
            100.0,
            Some(table),
        ));
        expect_point(&follows.top, "left/lane/outer", 3.5, 10.21);
        expect_point(&follows.top, "left/kerb/outer", 3.65, 10.219);
        expect_point(&follows.top, "left/shoulder/outer", 5.15, 10.309);
        expect_point(&follows.top, "right/lane/outer", -3.5, 9.79);
        expect_point(&follows.top, "right/kerb/outer", -3.65, 9.781);
        expect_point(&follows.top, "right/shoulder/outer", -5.15, 9.691);
        let flat = must(road_section(
            &stripped(false),
            &straight(200.0),
            &level(10.0, 0.0, 200.0),
            None,
            100.0,
            Some(table),
        ));
        expect_point(&flat.top, "left/kerb/outer", 3.65, 10.21);
        expect_point(&flat.top, "left/shoulder/outer", 5.15, 10.30);

        // A table drain 2 m wide with its invert 0.5 m down: its outer edge
        // rises with the table when it is superelevated: 2 x 6 % = 0.12 above
        // the lane's edge (10.21 + 0.12 = 10.33), and stays level with it
        // (10.21) when it is not.
        let drained = |superelevated: bool| {
            let mut lane = default_component(ComponentKind::Lane, Side::Left);
            lane.width = 3.5;
            lane.crossfall = -3.0;
            let mut drain = default_component(ComponentKind::Drain, Side::Left);
            drain.superelevated = superelevated;
            RoadTemplate {
                name: String::from("drained"),
                components: vec![lane, drain],
                ..RoadTemplate::default()
            }
        };
        let raised = must(road_section(
            &drained(true),
            &straight(200.0),
            &level(10.0, 0.0, 200.0),
            None,
            100.0,
            Some(table),
        ));
        expect_point(&raised.top, "left/drain/invert", 4.5, 9.71);
        expect_point(&raised.top, "left/drain/outer", 5.5, 10.33);
        let level0 = must(road_section(
            &drained(false),
            &straight(200.0),
            &level(10.0, 0.0, 200.0),
            None,
            100.0,
            Some(table),
        ));
        expect_point(&level0.top, "left/drain/outer", 5.5, 10.21);
        // With no table at the station nothing is superelevated: the own
        // crossfall.
        let none = must(road_section(
            &stripped(true),
            &straight(200.0),
            &level(10.0, 0.0, 200.0),
            None,
            100.0,
            None,
        ));
        expect_point(&none.top, "left/kerb/outer", 3.65, 9.895);
    }

    #[test]
    fn a_cut_batter_daylights_on_a_plane_at_the_hand_worked_offset() {
        // A level design 1 m under flat ground, 8 m wide, 2:1 batters: daylight
        // 2 m out at +-6, the textbook trapezoid (12 + 8) / 2 = 10 m2 of cut.
        let ground = plane_ground(0.0, 0.0, 0.0, 100.0);
        let section = must(road_section(
            &lane_template(4.0, 0.0, Vec::new(), 2.0, 2.0),
            &straight(200.0),
            &level(-1.0, 0.0, 200.0),
            Some(&ground),
            100.0,
            None,
        ));
        assert!(section.complete);
        expect_point(&section.top, "left/batter/daylight", 6.0, 0.0);
        expect_point(&section.top, "right/batter/daylight", -6.0, 0.0);
        near(section.cut_area, 10.0, 1e-9);
        near(section.fill_area, 0.0, 1e-9);
        near(section.left_cut, 5.0, 1e-9);
        near(section.right_cut, 5.0, 1e-9);
    }

    #[test]
    fn a_fill_batter_mirrors_the_cut() {
        let ground = plane_ground(0.0, 0.0, 0.0, 100.0);
        let section = must(road_section(
            &lane_template(4.0, 0.0, Vec::new(), 2.0, 2.0),
            &straight(200.0),
            &level(1.0, 0.0, 200.0),
            Some(&ground),
            100.0,
            None,
        ));
        expect_point(&section.top, "left/batter/daylight", 6.0, 0.0);
        near(section.fill_area, 10.0, 1e-9);
        near(section.cut_area, 0.0, 1e-9);
    }

    #[test]
    fn on_sidelong_ground_each_side_daylights_at_its_own_width_and_keeps_its_own_area() {
        // Ground rising 10 % to the left (z = 0.1 y), the design level at -1, 4
        // m each side, 2:1 cut: the left batter meets the ground 3.5 m out (at
        // 7.5, z 0.75), the right 1 m out (at -5, z -0.5). Integrated by hand:
        // the left side is 4.8 under the lane plus 2.45 under the batter, 7.25
        // m2; the right 3.2 plus 0.3, 3.5 m2; 10.75 in all.
        let ground = plane_ground(0.0, 0.1, 0.0, 100.0);
        let section = must(road_section(
            &lane_template(4.0, 0.0, Vec::new(), 2.0, 2.0),
            &straight(200.0),
            &level(-1.0, 0.0, 200.0),
            Some(&ground),
            100.0,
            None,
        ));
        expect_point(&section.top, "left/batter/daylight", 7.5, 0.75);
        expect_point(&section.top, "right/batter/daylight", -5.0, -0.5);
        near(section.left_cut, 7.25, 1e-9);
        near(section.right_cut, 3.5, 1e-9);
        near(section.cut_area, 10.75, 1e-9);
    }

    #[test]
    fn the_pavement_box_is_cut_and_the_batter_starts_at_the_finished_edge() {
        // The same cut with 300 mm of courses: the batter still starts at the
        // finished edge (4, -1) and daylights at 6, and the formation is 0.3
        // lower under the lane. Each side, by hand: 1.3 deep over 3.999 m
        // (5.1987), the 1 mm step from -1.3 up to -1 (0.001 x 1.15 = 0.00115),
        // the batter's triangle (1.0): 6.19985 m2; 12.3997 both.
        let ground = plane_ground(0.0, 0.0, 0.0, 100.0);
        let section = must(road_section(
            &lane_template(
                4.0,
                0.0,
                vec![
                    make_course("AC", 0.1, "asphalt", "PAVEMENT"),
                    make_course("base", 0.2, "rock", "PAVEMENT"),
                ],
                2.0,
                2.0,
            ),
            &straight(200.0),
            &level(-1.0, 0.0, 200.0),
            Some(&ground),
            100.0,
            None,
        ));
        expect_point(&section.top, "left/batter/daylight", 6.0, 0.0);
        near(section.left_cut, 6.19985, 1e-9);
        near(section.cut_area, 12.3997, 1e-9);
    }

    #[test]
    fn a_side_of_a_batter_alone_starts_its_formation_at_the_hinge_and_measures_the_triangle_under_it()
     {
        // Fill: ground at 9, design 10, a 1 in 4 fill batter. From the hinge on
        // the centreline (0, 10) it falls 1 over 4 m to the ground: daylight at
        // +-4, level 9. Each side is the triangle 0.5 x 4 x 1 = 2 m2 between the
        // batter and the ground; 4 m2 in all, and nothing in cut.
        let low = plane_ground(0.0, 0.0, 9.0, 100.0);
        let fill = must(road_section(
            &batters_alone(2.0, 4.0),
            &straight(200.0),
            &level(10.0, 0.0, 200.0),
            Some(&low),
            100.0,
            None,
        ));
        assert!(fill.complete);
        expect_point(&fill.top, "left/batter/daylight", 4.0, 9.0);
        expect_point(&fill.top, "right/batter/daylight", -4.0, 9.0);
        near(fill.fill_area, 4.0, 1e-9);
        near(fill.left_fill, 2.0, 1e-9);
        near(fill.right_fill, 2.0, 1e-9);
        near(fill.cut_area, 0.0, 1e-9);
        // The formation is the batters' own line, through the hinge: the two
        // hinges are one point at the centreline.
        assert_eq!(fill.formation.len(), 3);
        near(fill.formation[0].offset, 4.0, 1e-9);
        near(fill.formation[1].offset, 0.0, 1e-12);
        near(fill.formation[1].level, 10.0, 1e-12);
        near(fill.formation[2].offset, -4.0, 1e-9);

        // Cut: ground at 11, a 1 in 2 cut batter climbs 1 over 2 m: daylight at
        // +-2, level 11, a triangle 0.5 x 2 x 1 = 1 m2 a side.
        let high = plane_ground(0.0, 0.0, 11.0, 100.0);
        let cut = must(road_section(
            &batters_alone(2.0, 4.0),
            &straight(200.0),
            &level(10.0, 0.0, 200.0),
            Some(&high),
            100.0,
            None,
        ));
        expect_point(&cut.top, "left/batter/daylight", 2.0, 11.0);
        near(cut.cut_area, 2.0, 1e-9);
        near(cut.left_cut, 1.0, 1e-9);
        near(cut.right_cut, 1.0, 1e-9);
        near(cut.fill_area, 0.0, 1e-9);

        // At grade the batter has no area to measure and the section is
        // complete.
        let level10 = plane_ground(0.0, 0.0, 10.0, 100.0);
        let flat = must(road_section(
            &batters_alone(2.0, 4.0),
            &straight(200.0),
            &level(10.0, 0.0, 200.0),
            Some(&level10),
            100.0,
            None,
        ));
        assert!(flat.complete);
        near(flat.cut_area, 0.0, 1e-9);
        near(flat.fill_area, 0.0, 1e-9);

        // No ground at all: the formation is the hinge alone, and nothing
        // breaks.
        let bare = must(road_section(
            &batters_alone(2.0, 4.0),
            &straight(200.0),
            &level(10.0, 0.0, 200.0),
            None,
            100.0,
            None,
        ));
        assert!(!bare.complete);
        assert_eq!(bare.formation.len(), 1);
        near(bare.formation[0].level, 10.0, 1e-12);
    }

    #[test]
    fn a_batter_alone_beside_a_coursed_lane_keeps_its_own_hinge_and_its_own_area() {
        // Left: a 4 m lane at 0 % with 0.1 + 0.2 m of courses, then a 1 in 4
        // fill batter; right: the batter alone. Ground at 9, design 10.
        // Left formation: 9.7 under the lane to 3.999 (the corner stepped 1 mm
        // back), up to the hinge (4, 10), the batter down to the ground at 8:
        // 3.999 x 0.7 = 2.7993, the 1 mm step 0.001 x 0.85 = 0.00085, the
        // triangle 0.5 x 4 x 1 = 2: 4.80015 m2.
        // Right: the hinge (0, 10) is higher than the lane's 9.7 at the
        // centreline, so it is the shallower corner and steps 1 mm out to
        // -0.001: the step 0.001 x 0.85 = 0.00085, the batter 0.5 x 3.999 x 1 =
        // 1.9995: 2.00035 m2. Taking the right batter from the lane's f0 (0,
        // 9.7) to its daylight (-4, 9) instead - the line without the hinge -
        // gives 0.5 x 4 x 0.7 = 1.4 m2, 0.6 m2 short.
        let mut lane = default_component(ComponentKind::Lane, Side::Left);
        lane.width = 4.0;
        lane.crossfall = 0.0;
        lane.courses = vec![
            make_course("AC", 0.1, "asphalt", "PAVEMENT"),
            make_course("base", 0.2, "rock", "PAVEMENT"),
        ];
        let mut left_batter = default_component(ComponentKind::Batter, Side::Left);
        left_batter.fill_slope = 4.0;
        let mut right_batter = default_component(ComponentKind::Batter, Side::Right);
        right_batter.fill_slope = 4.0;
        let made = RoadTemplate {
            name: String::from("ramp"),
            components: vec![lane, left_batter, right_batter],
            ..RoadTemplate::default()
        };
        let ground = plane_ground(0.0, 0.0, 9.0, 100.0);
        let section = must(road_section(
            &made,
            &straight(200.0),
            &level(10.0, 0.0, 200.0),
            Some(&ground),
            100.0,
            None,
        ));
        assert!(section.complete);
        expect_point(&section.top, "left/batter/daylight", 8.0, 9.0);
        expect_point(&section.top, "right/batter/daylight", -4.0, 9.0);
        near(section.left_fill, 4.80015, 1e-9);
        near(section.right_fill, 2.00035, 1e-9);
        near(section.fill_area, 6.8005, 1e-9);
        near(section.cut_area, 0.0, 1e-9);
        // The formation keeps its function of offset: strictly falling across.
        for (i, pair) in section.formation.windows(2).enumerate() {
            assert!(pair[0].offset > pair[1].offset, "{i}");
        }
    }

    #[test]
    fn a_benched_batter_has_its_benches_and_daylights_where_they_lead_it() {
        // 3 m of cut, a 2:1 batter benched 2 m wide every 1 m of rise: up 1 over
        // 2 m to -2 at 6, a bench to 8, up to -1 at 10, a bench to 12, up to the
        // ground at 14.
        let mut benched = lane_template(4.0, 0.0, Vec::new(), 2.0, 2.0);
        for each in &mut benched.components {
            if each.kind == ComponentKind::Batter {
                each.bench_height = 1.0;
                each.bench_width = 2.0;
            }
        }
        let ground = plane_ground(0.0, 0.0, 0.0, 100.0);
        let section = must(road_section(
            &benched,
            &straight(200.0),
            &level(-3.0, 0.0, 200.0),
            Some(&ground),
            100.0,
            None,
        ));
        expect_point(&section.top, "left/batter/bench1-in", 6.0, -2.0);
        expect_point(&section.top, "left/batter/bench1-out", 8.0, -2.0);
        expect_point(&section.top, "left/batter/bench2-in", 10.0, -1.0);
        expect_point(&section.top, "left/batter/bench2-out", 12.0, -1.0);
        let foot = point_coded(&section.top, "left/batter/daylight").expect("the daylight");
        near(foot.offset, 14.0, 1e-9);
        near(foot.level, 0.0, 1e-9);
    }

    #[test]
    fn a_template_with_a_millimetre_bench_is_refused_before_a_section_is_built() {
        // A 1 mm bench every 1 mm of rise over a 50 m reach would be about 17
        // 000 benches a section (the cost grew with the square of that);
        // 1e-300 never ended. Both are refused by the check every section
        // starts with.
        for tiny in [0.001, 1e-300] {
            let mut benched = lane_template(4.0, 0.0, Vec::new(), 2.0, 2.0);
            for each in &mut benched.components {
                if each.kind == ComponentKind::Batter {
                    each.bench_height = tiny;
                    each.bench_width = tiny;
                }
            }
            let ground = plane_ground(0.0, 0.0, 0.0, 100.0);
            let refused = road_section(
                &benched,
                &straight(200.0),
                &level(-3.0, 0.0, 200.0),
                Some(&ground),
                100.0,
                None,
            )
            .expect_err("the bench is refused");
            assert_eq!(refused.code, ErrorCode::InvalidArgument, "{tiny}");
            assert!(refused.message.contains("benches"), "{}", refused.message);
        }
    }

    #[test]
    fn a_batter_that_meets_no_ground_leaves_its_side_unreached() {
        // Ground only 3 m either side: the batters need 6 m.
        let narrow = plane_ground(0.0, 0.0, 0.0, 3.0);
        let off_the_edge = must(road_section(
            &lane_template(4.0, 0.0, Vec::new(), 2.0, 2.0),
            &straight(200.0),
            &level(-1.0, 0.0, 200.0),
            Some(&narrow),
            100.0,
            None,
        ));
        assert!(!off_the_edge.left_reached);
        assert!(!off_the_edge.complete);
        assert_eq!(off_the_edge.cut_area, 0.0);
        // Its pavement is still there: the hinge is the outermost point.
        assert_eq!(off_the_edge.top[0].code, "left/lane/outer");

        // No surface at all: the same, every section.
        let none = must(road_section(
            &lane_template(4.0, 0.0, Vec::new(), 2.0, 2.0),
            &straight(200.0),
            &level(-1.0, 0.0, 200.0),
            None,
            100.0,
            None,
        ));
        assert!(!none.right_reached);
        assert!(none.ground.is_empty());
    }

    #[test]
    fn a_batter_limit_that_is_not_a_multiple_of_a_step_still_finds_a_daylight_inside_it() {
        // A 3.5 m lane at -3 % from design 10 ends at the hinge (3.5, 9.895). On
        // ground at 9 the 1 in 4 fill batter meets it 0.895 x 4 = 3.58 m out, at
        // offset 7.08: inside a limit of 3.7 m, but past the last half-metre step
        // (3.5) the march used to try before it gave up.
        let with_limit = |limit: f64| {
            let mut made = lane_template(3.5, -3.0, Vec::new(), 2.0, 4.0);
            for each in &mut made.components {
                if each.kind == ComponentKind::Batter {
                    each.maximum_width = limit;
                }
            }
            made
        };
        let ground = plane_ground(0.0, 0.0, 9.0, 100.0);
        let inside = must(road_section(
            &with_limit(3.7),
            &straight(200.0),
            &level(10.0, 0.0, 200.0),
            Some(&ground),
            100.0,
            None,
        ));
        assert!(inside.complete);
        expect_point(&inside.top, "left/batter/daylight", 7.08, 9.0);
        expect_point(&inside.top, "right/batter/daylight", -7.08, 9.0);
        // Fill a side, between the ground (9) and the finished line: under the
        // lane a trapezoid of heights 1 and 0.895 over 3.5 m, 3.5 x (1 + 0.895)
        // / 2 = 3.31625; under the batter a triangle 0.5 x 3.58 x 0.895 =
        // 1.60205: 4.9183 m2.
        near(inside.left_fill, 3.31625 + 1.60205, 1e-9);
        // A limit short of the crossing (3.5 m < 3.58 m) leaves the side
        // unreached, as it always did.
        let short = must(road_section(
            &with_limit(3.5),
            &straight(200.0),
            &level(10.0, 0.0, 200.0),
            Some(&ground),
            100.0,
            None,
        ));
        assert!(!short.left_reached);
        assert!(!short.complete);
        assert_eq!(short.cut_area, 0.0);

        // A limit under one step: ground at 9.85, the batter meets it 0.045 x 4
        // = 0.18 m out (offset 3.68) - inside 0.3 m, outside 0.1 m.
        let high = plane_ground(0.0, 0.0, 9.85, 100.0);
        let brief = must(road_section(
            &with_limit(0.3),
            &straight(200.0),
            &level(10.0, 0.0, 200.0),
            Some(&high),
            100.0,
            None,
        ));
        assert!(brief.complete);
        expect_point(&brief.top, "left/batter/daylight", 3.68, 9.85);
        let too_brief = must(road_section(
            &with_limit(0.1),
            &straight(200.0),
            &level(10.0, 0.0, 200.0),
            Some(&high),
            100.0,
            None,
        ));
        assert!(!too_brief.left_reached);
    }

    #[test]
    fn an_empty_template_is_the_centreline_alone() {
        let empty = RoadTemplate {
            name: String::from("empty"),
            ..RoadTemplate::default()
        };
        let section = must(road_section(
            &empty,
            &straight(200.0),
            &level(5.0, 0.0, 200.0),
            None,
            10.0,
            None,
        ));
        assert_eq!(section.top.len(), 1);
        assert_eq!(section.top[0].code, "CL");
        assert!(section.solids.is_empty());
    }

    #[test]
    fn refuses_a_station_off_the_alignment_or_profile_and_a_template_that_cannot_be_built() {
        assert_eq!(
            road_section(
                &street(),
                &straight(200.0),
                &level(10.0, 0.0, 200.0),
                None,
                250.0,
                None
            )
            .expect_err("off the alignment")
            .code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            road_section(
                &street(),
                &straight(200.0),
                &level(10.0, 0.0, 50.0),
                None,
                100.0,
                None
            )
            .expect_err("off the profile")
            .code,
            ErrorCode::InvalidArgument
        );
        let mut broken = street();
        broken.components[0].width = -1.0;
        assert_eq!(
            road_section(
                &broken,
                &straight(200.0),
                &level(10.0, 0.0, 200.0),
                None,
                100.0,
                None
            )
            .expect_err("a negative width")
            .code,
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn stations_are_the_interval_the_ends_and_the_keys_each_once() {
        // 0 to 100 every 30, with keys at 45, at the end, outside, and within a
        // tenth of a micrometre of the start (made one with it).
        let stations = section_stations(0.0, 100.0, 30.0, &[45.0, 100.0, -5.0, 1e-8]);
        assert_eq!(stations, vec![0.0, 30.0, 45.0, 60.0, 90.0, 100.0]);
        assert_eq!(
            section_stations(0.0, 20.0, 7.0, &[]),
            vec![0.0, 7.0, 14.0, 20.0]
        );
        assert!(section_stations(0.0, 20.0, 0.0, &[]).is_empty());
        assert!(section_stations(20.0, 0.0, 5.0, &[]).is_empty());
    }

    #[test]
    fn stations_inside_a_curve_are_cut_until_no_step_turns_through_more_than_the_tolerances_angle()
    {
        // The quarter turn at 50 m: the arc runs from station 50 to 50 + 25 pi =
        // 128.5398 (50 x pi/2). Every 10 m with the keys 0, 50, 128.5398,
        // 178.5398 the plain stations are 0, 10, ..., 120, 128.5398, 130, ...,
        // 170, 178.5398: 20. A tolerance of 0.001 allows a step to turn through
        // sqrt(6 x 0.001) = 0.07746 radians, 3.873 m on R = 50 m. A 10 m gap
        // turns through 10/50 = 0.2: 0.2/0.07746 = 2.58, three parts; the last
        // gap on the arc, 8.5398 m, turns 0.1708: 2.2, three parts. The arc
        // holds seven gaps of 10 m and that one: 8 x 2 = 16 stations more, 36.
        let alignment = quarter_turn(0.0);
        let keys = alignment.key_stations();
        let plain = section_stations(0.0, alignment.end_station(), 10.0, &keys);
        assert_eq!(plain.len(), 20);
        let cut = must(section_stations_along(
            0.0,
            alignment.end_station(),
            10.0,
            &keys,
            &alignment,
            0.001,
        ));
        assert_eq!(cut.len(), 36);
        // Every plain station is still there, ascending and each once.
        for station in &plain {
            assert!(cut.contains(station), "{station}");
        }
        assert!(
            cut.windows(2).all(|pair| pair[0] < pair[1]),
            "not strictly ascending"
        );
        // The first gap in thirds: 50, 50 + 10/3, 50 + 20/3, 60.
        let first = cut
            .iter()
            .position(|&station| station == 50.0)
            .expect("50 is a key station");
        assert!(cut.len() - first >= 4);
        near(cut[first + 1], 50.0 + 10.0 / 3.0, 1e-12);
        near(cut[first + 2], 50.0 + 20.0 / 3.0, 1e-12);
        assert_eq!(cut[first + 3], 60.0);
        // The property, from the directions: no step turns through more than
        // the angle - and the plain list does, which shows the cases are
        // reached.
        let angle = (6.0_f64 * 0.001).sqrt();
        let mut plain_worst = 0.0;
        for pair in plain.windows(2) {
            plain_worst = cpp::max(plain_worst, turn_between(&alignment, pair[0], pair[1]));
        }
        assert!(
            plain_worst > angle,
            "the plain stations already satisfied it: nothing was tested"
        );
        for pair in cut.windows(2) {
            assert!(
                turn_between(&alignment, pair[0], pair[1]) <= angle * (1.0 + 1e-9),
                "{} to {}",
                pair[0],
                pair[1]
            );
        }
    }

    #[test]
    fn stations_on_a_spiral_are_held_to_the_sharper_end_of_each_gap() {
        // A spiral of 40 m into and out of the arc: its curvature rises from
        // nothing to 1/50 linearly, so a gap on it turns through the mean of its
        // ends' curvatures times its length, at most the sharper end's times it,
        // which is what is held to the angle. Measured from the directions.
        let alignment = quarter_turn(40.0);
        let keys = alignment.key_stations();
        assert!(
            keys.len() >= 6,
            "a spiral in and out of an arc: TS SC CS ST"
        );
        let angle = (6.0_f64 * 0.001).sqrt();
        let mut worst = 0.0;
        for interval in [10.0, 3.0] {
            let plain = section_stations(0.0, alignment.end_station(), interval, &keys);
            for pair in plain.windows(2) {
                worst = cpp::max(worst, turn_between(&alignment, pair[0], pair[1]));
            }
            let cut = must(section_stations_along(
                0.0,
                alignment.end_station(),
                interval,
                &keys,
                &alignment,
                0.001,
            ));
            // 3 m on the arc is 0.06 radians, under the 0.0775 allowed: nothing
            // to add. 10 m is 0.2 on the arc, and the spirals' sharper ends are
            // cut too.
            if interval == 3.0 {
                assert_eq!(cut, plain);
            } else {
                assert!(cut.len() > plain.len());
            }
            for station in &plain {
                assert!(cut.contains(station), "{station}");
            }
            for pair in cut.windows(2) {
                assert!(
                    turn_between(&alignment, pair[0], pair[1]) <= angle * (1.0 + 1e-9),
                    "interval {interval}: {} to {}",
                    pair[0],
                    pair[1]
                );
            }
        }
        // The plain list at 10 m has a step that turns too far, so the property
        // above is not satisfied by luck.
        assert!(worst > angle);
    }

    #[test]
    fn a_tangent_adds_no_stations_and_no_tolerance_or_an_unusable_one_adds_none() {
        // The straight, and a curve with the tolerance off (0), negative, not a
        // number or infinite: the plain list, as section_stations has always
        // given.
        let straight_line = straight(100.0);
        let plain = section_stations(0.0, 100.0, 30.0, &[45.0]);
        assert_eq!(
            must(section_stations_along(
                0.0,
                100.0,
                30.0,
                &[45.0],
                &straight_line,
                0.001
            )),
            plain
        );
        let curve = quarter_turn(0.0);
        let curve_plain = section_stations(0.0, curve.end_station(), 10.0, &curve.key_stations());
        for tolerance in [0.0, -0.001, f64::NAN, f64::INFINITY] {
            let same = must(section_stations_along(
                0.0,
                curve.end_station(),
                10.0,
                &curve.key_stations(),
                &curve,
                tolerance,
            ));
            assert_eq!(same, curve_plain, "{tolerance}");
        }
        // No length, and no interval: nothing to cut.
        assert_eq!(
            must(section_stations_along(5.0, 5.0, 10.0, &[], &curve, 0.001)),
            vec![5.0]
        );
        assert!(must(section_stations_along(5.0, 0.0, 10.0, &[], &curve, 0.001)).is_empty());
        assert!(must(section_stations_along(0.0, 50.0, 0.0, &[], &curve, 0.001)).is_empty());
        // An interval longer than the road: the two ends, and the gap between
        // them cut where the alignment curves.
        let wide = must(section_stations_along(
            0.0,
            curve.end_station(),
            1e9,
            &[],
            &curve,
            0.001,
        ));
        assert_eq!(wide[0], 0.0);
        assert_eq!(wide[wide.len() - 1], curve.end_station());
        assert!(wide.len() > 2);
    }

    #[test]
    fn a_tolerances_stations_beyond_the_limit_are_refused_before_one_is_made() {
        // 1e-12 allows a step of sqrt(6e-12) = 2.4e-6 radians, 1.2e-4 m on R =
        // 50: 650 000 sections round the arc alone, over the 100 000 a corridor
        // makes. And 1e-300, which asks for more parts than a usize holds, is
        // refused the same way and not wrapped round.
        let curve = quarter_turn(0.0);
        for tolerance in [1e-12, 1e-300] {
            let refused = section_stations_along(
                0.0,
                curve.end_station(),
                10.0,
                &curve.key_stations(),
                &curve,
                tolerance,
            )
            .expect_err("too many stations");
            assert_eq!(refused.code, ErrorCode::InvalidArgument, "{tolerance}");
            assert!(refused.message.contains("100000"), "{}", refused.message);
        }
        // The straight has nothing to cut at any tolerance.
        assert!(section_stations_along(0.0, 100.0, 10.0, &[], &straight(100.0), 1e-300).is_ok());
    }
}
