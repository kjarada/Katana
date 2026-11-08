// Grading a feature line to the ground (tests/cad/test_grading.cpp).

use crate::near;
use katana_cad::grading::grade_to_surface;
use katana_cad::{FeatureLine, Grading, GradingSide, GradingSlopes};
use katana_core::{ErrorCode, Result};
use katana_geometry::{Point2, Point3, Polyline2};
use katana_math::cpp;
use katana_terrain::TinSurface;

// A plane z = a + b x + c y over the square [-100, 100]^2: two triangles, so
// every elevation query is exact and every batter meets it at a point that
// can be worked by hand.
fn plane(a: f64, b: f64, c: f64) -> TinSurface {
    let z = |x: f64, y: f64| a + b * x + c * y;
    TinSurface::create(
        vec![
            Point3::new(-100.0, -100.0, z(-100.0, -100.0)),
            Point3::new(100.0, -100.0, z(100.0, -100.0)),
            Point3::new(100.0, 100.0, z(100.0, 100.0)),
            Point3::new(-100.0, 100.0, z(-100.0, 100.0)),
        ],
        vec![[0, 1, 2], [0, 2, 3]],
        Vec::new(),
    )
    .expect("two counter-clockwise triangles make a surface")
}

// The 10 x 10 pad centred on the origin.
fn square() -> Polyline2 {
    Polyline2::new(
        vec![
            Point2::new(-5.0, -5.0),
            Point2::new(5.0, -5.0),
            Point2::new(5.0, 5.0),
            Point2::new(-5.0, 5.0),
        ],
        true,
    )
}

fn slopes(cut: f64, fill: f64) -> GradingSlopes {
    GradingSlopes {
        cut_batter: cut,
        fill_batter: fill,
        interval: 2.5,
        ..GradingSlopes::default()
    }
}

// Volume of a frustum: h/3 (A1 + A2 + sqrt(A1 A2)). A pad's batters on flat
// ground make one, with the pad on top and the daylight square underneath.
fn frustum(height: f64, top_side: f64, base_side: f64) -> f64 {
    let a1 = top_side * top_side;
    let a2 = base_side * base_side;
    height / 3.0 * (a1 + a2 + (a1 * a2).sqrt())
}

#[track_caller]
fn graded(result: Result<Grading>) -> Grading {
    result.unwrap_or_else(|error| panic!("{}", error.describe()))
}

mod grading {
    use super::*;

    #[test]
    fn a_filled_pad_on_flat_ground_daylights_at_the_slope_times_the_height_with_mitred_corners() {
        // Pad at 2 m over ground at 0 with a 1 in 2 fill batter: the batter
        // runs 4 m out from every edge, so the daylight line is the 18 x 18
        // square, and its corners are the pad's corners pushed out 4 m in BOTH
        // axes - (-9, -9) - where the two batter planes meet, not 4 m along
        // the diagonal.
        let ground = plane(0.0, 0.0, 0.0);
        let graded = graded(grade_to_surface(
            &FeatureLine::pad(&square(), 2.0),
            &slopes(1.0, 2.0),
            &ground,
        ));
        assert_eq!(graded.missed, 0);
        // 4 vertices plus 3 interior samples on each 10 m edge at 2.5 m.
        assert_eq!(graded.samples, 16);
        assert_eq!(graded.daylights.len(), 16);
        for point in &graded.daylights {
            assert!(!point.cut);
            near(point.daylight.z, 0.0, 1e-9);
            // Every daylight point is 4 m out in the axis perpendicular to
            // its edge; the corners are 4 m out in both.
            near(
                cpp::max(point.daylight.x.abs(), point.daylight.y.abs()),
                9.0,
                1e-6,
            );
        }
        near(graded.daylights[0].daylight.x, -9.0, 1e-6);
        near(graded.daylights[0].daylight.y, -9.0, 1e-6);
        assert!(graded.bounded_by_daylight);
        assert!(graded.daylight_line.closed);
        near(graded.daylight_line.area(), 18.0 * 18.0, 1e-6);

        // The earthwork is a frustum 2 m high, 10 m square on top, 18 m
        // below: 2/3 (100 + 324 + 180) = 402.667 m^3, all fill.
        near(graded.fill, frustum(2.0, 10.0, 18.0), 1e-6);
        near(graded.cut, 0.0, 1e-9);
        near(graded.plan_area, 18.0 * 18.0, 1e-6);
        near(graded.surface.plan_area(), 18.0 * 18.0, 1e-6);
    }

    #[test]
    fn a_cut_pad_uses_the_cut_batter_and_the_corner_is_still_mitred() {
        // Pad at -2 with a 1 in 1 cut batter: 2 m out, a 14 x 14 daylight
        // square, and the cut is the frustum 2/3 (100 + 196 + 140) = 290.667
        // m^3. A 1 in 3 FILL batter must play no part.
        let ground = plane(0.0, 0.0, 0.0);
        let graded = graded(grade_to_surface(
            &FeatureLine::pad(&square(), -2.0),
            &slopes(1.0, 3.0),
            &ground,
        ));
        for point in &graded.daylights {
            assert!(point.cut);
            near(
                cpp::max(point.daylight.x.abs(), point.daylight.y.abs()),
                7.0,
                1e-6,
            );
        }
        near(graded.cut, frustum(2.0, 10.0, 14.0), 1e-6);
        near(graded.fill, 0.0, 1e-9);
    }

    #[test]
    fn on_sloping_ground_each_side_daylights_where_its_own_batter_meets_the_ground() {
        // Ground z = 0.1 x. The pad at 0.5 is AT GRADE along its east edge
        // (x = 5, ground 0.5), in fill along the west (ground -0.5). West:
        // batter 0.5 - d/2 meets ground 0.1 (-5 - d) = -0.5 - 0.1 d when
        // 1 = 0.4 d, d = 2.5: daylight at x = -7.5, z = -0.75. North and south
        // edges are in fill by 0.5 - 0.1 x, thinning to nothing at the east
        // end.
        let ground = plane(0.0, 0.1, 0.0);
        let graded = graded(grade_to_surface(
            &FeatureLine::pad(&square(), 0.5),
            &slopes(1.0, 2.0),
            &ground,
        ));
        assert_eq!(graded.missed, 0);
        let mut west = 0;
        let mut east = 0;
        for point in &graded.daylights {
            let on_west_edge = (point.origin.x + 5.0).abs() < 1e-9 && point.origin.y.abs() < 5.0;
            let on_east_edge = (point.origin.x - 5.0).abs() < 1e-9 && point.origin.y.abs() < 5.0;
            if on_west_edge {
                west += 1;
                near(point.daylight.x, -7.5, 1e-6);
                near(point.daylight.z, -0.75, 1e-6);
                assert!(!point.cut);
            }
            if on_east_edge {
                east += 1;
                near(point.daylight.x, 5.0, 1e-6); // at grade: the daylight is the edge
            }
            // Every daylight point lies on the ground, and on the batter from
            // its origin.
            near(point.daylight.z, 0.1 * point.daylight.x, 1e-6);
        }
        assert_eq!(west, 3);
        assert_eq!(east, 3);
        assert!(graded.fill > 0.0);
        near(graded.cut, 0.0, 1e-6);
    }

    #[test]
    fn an_open_feature_line_is_graded_on_the_chosen_side_only() {
        // A line along the x axis at 1 m, graded to its left (+y) at 1 in 2 on
        // flat ground: the daylight is the parallel line y = 2, and the strip
        // between them is a prism 10 long, 2 wide, 1 high, cut diagonally:
        // 10 m^3.
        let line = FeatureLine {
            vertices: vec![Point3::new(0.0, 0.0, 1.0), Point3::new(10.0, 0.0, 1.0)],
            closed: false,
        };
        let mut left = slopes(1.0, 2.0);
        left.side = GradingSide::Left;
        let ground = plane(0.0, 0.0, 0.0);
        let graded = graded(grade_to_surface(&line, &left, &ground));
        assert_eq!(graded.samples, 5);
        for point in &graded.daylights {
            near(point.daylight.y, 2.0, 1e-6);
            near(point.daylight.x, point.origin.x, 1e-9);
        }
        assert!(!graded.daylight_line.closed);
        assert!(graded.bounded_by_daylight);
        near(graded.fill, 10.0, 1e-6);
        near(graded.plan_area, 20.0, 1e-6);

        left.side = GradingSide::Right;
        let right = grade_to_surface(&line, &left, &ground).expect("graded to the right");
        for point in &right.daylights {
            near(point.daylight.y, -2.0, 1e-6);
        }
    }

    #[test]
    fn a_clockwise_pad_is_graded_outward_all_the_same() {
        let mut clockwise = square();
        clockwise.vertices.reverse();
        let ground = plane(0.0, 0.0, 0.0);
        let graded = graded(grade_to_surface(
            &FeatureLine::pad(&clockwise, 2.0),
            &slopes(1.0, 2.0),
            &ground,
        ));
        near(graded.daylight_line.area(), 18.0 * 18.0, 1e-6);
        near(graded.fill, frustum(2.0, 10.0, 18.0), 1e-6);
    }

    #[test]
    fn a_feature_line_with_varying_heights_grades_each_sample_from_its_own_elevation() {
        // An east-west line rising from 1 to 3 m over 10 m, graded left at 1
        // in 1 on flat ground: the daylight width grows with the height, 1 m
        // at the west end, 3 m at the east, and 2 m at the middle sample.
        let line = FeatureLine {
            vertices: vec![Point3::new(0.0, 0.0, 1.0), Point3::new(10.0, 0.0, 3.0)],
            closed: false,
        };
        let mut s = slopes(1.0, 1.0);
        s.interval = 5.0;
        let graded = graded(grade_to_surface(&line, &s, &plane(0.0, 0.0, 0.0)));
        assert_eq!(graded.daylights.len(), 3);
        near(graded.daylights[0].daylight.y, 1.0, 1e-6);
        near(graded.daylights[1].daylight.y, 2.0, 1e-6);
        near(graded.daylights[2].daylight.y, 3.0, 1e-6);
    }

    #[test]
    fn samples_that_find_no_ground_are_counted_and_the_rest_still_grade() {
        // The pad straddles the east edge of the ground (x = 100): its east
        // side and its corners' mitres run off the surface and are missed;
        // the rest reaches the ground and the surface is bounded by the hull
        // instead.
        let pad = Polyline2::new(
            vec![
                Point2::new(90.0, -5.0),
                Point2::new(105.0, -5.0),
                Point2::new(105.0, 5.0),
                Point2::new(90.0, 5.0),
            ],
            true,
        );
        let graded = graded(grade_to_surface(
            &FeatureLine::pad(&pad, 2.0),
            &slopes(1.0, 2.0),
            &plane(0.0, 0.0, 0.0),
        ));
        assert!(graded.missed > 0);
        assert!(graded.missed < graded.samples);
        assert!(!graded.bounded_by_daylight);
        assert!(!graded.surface.empty());
    }

    #[test]
    fn a_batter_that_never_meets_the_ground_within_the_width_is_a_miss() {
        // Ground z = x: falling at 1 in 1 to the west, rising at 1 in 1 to the
        // east. The pad at 0 is in fill on its west edge, where a 1 in 2
        // batter falls slower than the ground and never catches it; in cut on
        // its east edge, where a 1 in 1 batter rises exactly as fast as the
        // ground and stays 5 m below it for ever. Both edges' 3 interior
        // samples miss, and all 4 corners, whose mitres head west or east: 10
        // misses. The north and south edges run across the slope and daylight
        // normally.
        let ground = plane(0.0, 1.0, 0.0);
        let mut s = slopes(1.0, 2.0);
        s.maximum_width = 20.0;
        let graded = graded(grade_to_surface(
            &FeatureLine::pad(&square(), 0.0),
            &s,
            &ground,
        ));
        for point in &graded.daylights {
            assert!(
                point.origin.x > -5.0 + 1e-9,
                "nothing on the west edge should have daylighted"
            );
            assert!(point.origin.x < 5.0 - 1e-9, "nor on the east");
        }
        assert_eq!(graded.missed, 10);
        assert_eq!(graded.daylights.len(), 6);
        assert!(!graded.bounded_by_daylight);
    }

    #[test]
    fn refuses_what_cannot_be_graded() {
        let ground = plane(0.0, 0.0, 0.0);
        let pad = FeatureLine::pad(&square(), 2.0);
        let code = |line: &FeatureLine, given: &GradingSlopes| {
            grade_to_surface(line, given, &ground)
                .expect_err("refused")
                .code
        };

        let mut bad = slopes(0.0, 2.0);
        assert_eq!(code(&pad, &bad), ErrorCode::InvalidArgument);
        bad = slopes(1.0, -2.0);
        assert_eq!(code(&pad, &bad), ErrorCode::InvalidArgument);
        bad = slopes(1.0, 2.0);
        bad.interval = 0.0;
        assert_eq!(code(&pad, &bad), ErrorCode::InvalidArgument);
        bad = slopes(1.0, 2.0);
        bad.maximum_width = f64::NAN;
        assert_eq!(code(&pad, &bad), ErrorCode::InvalidArgument);

        let two = FeatureLine {
            vertices: vec![Point3::new(0.0, 0.0, 1.0), Point3::new(1.0, 0.0, 1.0)],
            closed: true,
        };
        assert_eq!(code(&two, &slopes(1.0, 2.0)), ErrorCode::InvalidArgument);
        let one = FeatureLine {
            vertices: vec![Point3::new(0.0, 0.0, 1.0)],
            closed: false,
        };
        assert_eq!(code(&one, &slopes(1.0, 2.0)), ErrorCode::InvalidArgument);
        let mut infinite = pad.clone();
        infinite.vertices[0].z = f64::INFINITY;
        assert_eq!(
            code(&infinite, &slopes(1.0, 2.0)),
            ErrorCode::InvalidArgument
        );

        // Entirely off the surface: nothing reaches the ground.
        let away = Polyline2::new(
            vec![
                Point2::new(500.0, 500.0),
                Point2::new(510.0, 500.0),
                Point2::new(510.0, 510.0),
            ],
            true,
        );
        let off = grade_to_surface(&FeatureLine::pad(&away, 2.0), &slopes(1.0, 2.0), &ground);
        assert_eq!(
            off.expect_err("nothing reaches the ground").code,
            ErrorCode::InvalidGeometry
        );
    }
}
