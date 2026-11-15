//! Twin of tests/survey/test_cogo.cpp.

mod survey_support;

mod survey_cogo {
    use crate::survey_support::{PROPERTY_ITERATIONS, Random, double_eq, error_code, near};
    use katana_core::ErrorCode;
    use katana_math::numerics::{DEG_TO_RAD, HALF_PI, PI, TWO_PI};
    use katana_survey::Coordinate2;
    use katana_survey::cogo::{
        forward, horizontal_distance, inverse, polygon_area, polygon_centroid, polygon_perimeter,
        polygon_signed_area, trigonometric_height_difference,
    };

    const NAN: f64 = f64::NAN;

    // Spacing of doubles at a UTM northing of 5e6 is 2^-30 ~ 9.3e-10 m. A
    // position computed there cannot be better than a few of these.
    const UTM_ROUNDING: f64 = 4.0 * 9.4e-10;

    fn c(northing: f64, easting: f64) -> Coordinate2 {
        Coordinate2 { northing, easting }
    }

    // L-shaped parcel, (northing, easting). In map axes (x = E, y = N):
    //   (0,0) (6,0) (6,2) (2,2) (2,5) (0,5), counter-clockwise.
    // Split into [0,6]x[0,2] (area 12, centroid x=3, y=1) and [0,2]x[2,5] (area
    // 6, centroid x=1, y=3.5):
    //   area      = 18
    //   perimeter = 6 + 2 + 4 + 3 + 2 + 5 = 22
    //   centroid  x = (12*3 + 6*1) / 18 = 7/3,  y = (12*1 + 6*3.5) / 18 = 11/6
    fn l_shape() -> Vec<Coordinate2> {
        vec![
            c(0.0, 0.0),
            c(0.0, 6.0),
            c(2.0, 6.0),
            c(2.0, 2.0),
            c(5.0, 2.0),
            c(5.0, 0.0),
        ]
    }

    fn translated(mut vertices: Vec<Coordinate2>, d_n: f64, d_e: f64) -> Vec<Coordinate2> {
        for v in &mut vertices {
            v.northing += d_n;
            v.easting += d_e;
        }
        vertices
    }

    // Random star-shaped (hence simple) polygon around `centre`, listed
    // counter-clockwise in map axes (x = easting, y = northing).
    //
    // For vertices at polar angle t_i and radius r_i about a common centre the
    // shoelace sum is sum_i r_i r_{i+1} sin(t_{i+1} - t_i), so the ring is
    // counter-clockwise exactly when every angular gap lies in (0, pi).
    // Independent uniform angles, merely sorted, do not give that: a few draws
    // clustered in one half-plane leave a gap above pi whose negative term can
    // outweigh the rest. One jittered angle per equal sector does: consecutive
    // vertices are (2 pi / count) (1 + u_{i+1} - u_i) apart with the jitters u
    // in [0, 1/2), which is within (pi / count, 3 pi / count], and
    // 3 pi / count <= pi for count >= 3.
    fn random_polygon(random: &mut Random, centre: Coordinate2) -> Vec<Coordinate2> {
        let count = random.integer(3, 12);
        let sector = TWO_PI / f64::from(count);
        let mut vertices = Vec::new();
        for i in 0..count {
            let angle = sector * (f64::from(i) + random.real(0.0, 0.5));
            let radius = random.real(20.0, 300.0);
            vertices.push(c(
                centre.northing + radius * angle.sin(),
                centre.easting + radius * angle.cos(),
            ));
        }
        vertices
    }

    // ---- inverse -------------------------------------------------------------

    #[test]
    fn inverse_cardinal_and_diagonal_directions() {
        let origin = c(1000.0, 2000.0);
        // Azimuth is clockwise from north: east is 90°, south 180°, west 270°.
        let cases = [
            (100.0, 0.0, 0.0, 100.0),
            (0.0, 100.0, 90.0, 100.0),
            (-100.0, 0.0, 180.0, 100.0),
            (0.0, -50.0, 270.0, 50.0),
            (100.0, 100.0, 45.0, 100.0 * 2.0_f64.sqrt()),
            (-100.0, 100.0, 135.0, 100.0 * 2.0_f64.sqrt()),
            (-100.0, -100.0, 225.0, 100.0 * 2.0_f64.sqrt()),
            (100.0, -100.0, 315.0, 100.0 * 2.0_f64.sqrt()),
        ];
        for (d_n, d_e, azimuth_degrees, distance) in cases {
            let result = inverse(origin, c(origin.northing + d_n, origin.easting + d_e)).unwrap();
            near(result.azimuth, azimuth_degrees * DEG_TO_RAD, 1e-15);
            near(result.distance, distance, 1e-12);
        }
    }

    #[test]
    fn inverse_three_four_five() {
        // dN = 3, dE = 4: distance 5, azimuth = angle whose cosine is 3/5.
        let result = inverse(c(1000.0, 2000.0), c(1003.0, 2004.0)).unwrap();
        double_eq(result.distance, 5.0);
        near(result.azimuth, 0.6_f64.acos(), 1e-15);
        // The reverse line differs by exactly 180°.
        let reverse = inverse(c(1003.0, 2004.0), c(1000.0, 2000.0)).unwrap();
        near(reverse.azimuth, 0.6_f64.acos() + PI, 1e-15);
    }

    #[test]
    fn inverse_azimuth_stays_below_two_pi() {
        // A hair west of north must give an azimuth just under 2*pi, never 2*pi
        // or < 0.
        let result = inverse(c(0.0, 0.0), c(1000.0, -1e-9)).unwrap();
        assert!(result.azimuth < TWO_PI);
        assert!(result.azimuth > TWO_PI - 1e-11);
    }

    #[test]
    fn inverse_of_coincident_points_is_an_error() {
        let p = c(5000000.0, 500000.0);
        assert_eq!(error_code(inverse(p, p)), ErrorCode::InvalidArgument);

        // 0.05 mm apart: the same ground mark under the 0.1 mm coordinate tolerance.
        assert!(inverse(c(0.0, 0.0), c(5e-5, 0.0)).is_err());
        // 1 mm apart is a legitimate, if short, line.
        assert!(inverse(c(0.0, 0.0), c(1e-3, 0.0)).is_ok());

        assert!(inverse(c(NAN, 0.0), c(1.0, 1.0)).is_err());
        assert!(inverse(c(0.0, 0.0), c(1.0, f64::INFINITY)).is_err());
    }

    // ---- forward -------------------------------------------------------------

    #[test]
    fn forward_known_geometry() {
        // az 30°, d 200: dN = 200 cos 30° = 100 sqrt(3), dE = 200 sin 30° = 100.
        let p = forward(c(1000.0, 2000.0), 30.0 * DEG_TO_RAD, 200.0).unwrap();
        near(p.northing, 1000.0 + 100.0 * 3.0_f64.sqrt(), 1e-12);
        near(p.easting, 2100.0, 1e-12);

        // Zero distance stays put; an azimuth beyond a full turn is accepted.
        assert_eq!(forward(c(7.0, 8.0), 1.0, 0.0).unwrap(), c(7.0, 8.0));
        let wrapped = forward(c(0.0, 0.0), TWO_PI + HALF_PI, 10.0).unwrap();
        near(wrapped.northing, 0.0, 1e-14);
        near(wrapped.easting, 10.0, 1e-14);
    }

    #[test]
    fn forward_rejects_bad_input() {
        assert_eq!(
            error_code(forward(c(0.0, 0.0), 0.0, -1.0)),
            ErrorCode::InvalidArgument
        );
        assert!(forward(c(0.0, 0.0), NAN, 1.0).is_err());
        assert!(forward(c(0.0, 0.0), 0.0, NAN).is_err());
        assert!(forward(c(NAN, 0.0), 0.0, 1.0).is_err());
    }

    #[test]
    fn property_forward_of_inverse_returns_the_target() {
        let mut random = Random::default();
        for _ in 0..PROPERTY_ITERATIONS {
            let a = c(
                5000000.0 + random.real(-5000.0, 5000.0),
                500000.0 + random.real(-5000.0, 5000.0),
            );
            let b = c(
                a.northing + random.real(-2000.0, 2000.0),
                a.easting + random.real(-2000.0, 2000.0),
            );
            let Ok(line) = inverse(a, b) else {
                // coincident draw
                continue;
            };
            assert!(line.azimuth >= 0.0);
            assert!(line.azimuth < TWO_PI);
            let back = forward(a, line.azimuth, line.distance).unwrap();
            near(back.northing, b.northing, UTM_ROUNDING);
            near(back.easting, b.easting, UTM_ROUNDING);
        }
    }

    #[test]
    fn property_inverse_of_forward_returns_azimuth_and_distance() {
        let mut random = Random::default();
        for _ in 0..PROPERTY_ITERATIONS {
            let a = c(random.real(-1000.0, 1000.0), random.real(-1000.0, 1000.0));
            let azimuth = random.real(0.0, TWO_PI);
            let distance = random.real(1.0, 2000.0);
            let b = forward(a, azimuth, distance).unwrap();
            let line = inverse(a, b).unwrap();
            near(line.distance, distance, 1e-9);
            // Compared through sine and cosine so that 0 and 2*pi agree.
            near((line.azimuth - azimuth).sin(), 0.0, 1e-10);
            near((line.azimuth - azimuth).cos(), 1.0, 1e-10);
        }
    }

    // ---- polygons ------------------------------------------------------------

    #[test]
    fn triangle_area_and_orientation() {
        // Map axes: (0,0) -> (E4,N0) -> (E0,N3): counter-clockwise, legs 4 and 3.
        let ccw = [c(0.0, 0.0), c(0.0, 4.0), c(3.0, 0.0)];
        double_eq(polygon_signed_area(&ccw).unwrap(), 6.0);
        double_eq(polygon_area(&ccw).unwrap(), 6.0);
        // 3 + 4 + 5
        double_eq(polygon_perimeter(&ccw).unwrap(), 12.0);

        let cw = [c(3.0, 0.0), c(0.0, 4.0), c(0.0, 0.0)];
        double_eq(polygon_signed_area(&cw).unwrap(), -6.0);
        double_eq(polygon_area(&cw).unwrap(), 6.0);

        // Centroid of a triangle is the mean of its vertices: N = 1, E = 4/3.
        let centroid = polygon_centroid(&ccw).unwrap();
        near(centroid.northing, 1.0, 1e-15);
        near(centroid.easting, 4.0 / 3.0, 1e-15);
    }

    #[test]
    fn l_shaped_parcel_hand_computed() {
        let parcel = l_shape();
        double_eq(polygon_area(&parcel).unwrap(), 18.0);
        double_eq(polygon_signed_area(&parcel).unwrap(), 18.0);
        double_eq(polygon_perimeter(&parcel).unwrap(), 22.0);
        let centroid = polygon_centroid(&parcel).unwrap();
        near(centroid.easting, 7.0 / 3.0, 1e-14);
        near(centroid.northing, 11.0 / 6.0, 1e-14);

        // An explicit closing vertex adds a zero-length edge and changes nothing.
        let mut closed = parcel.clone();
        closed.push(parcel[0]);
        double_eq(polygon_area(&closed).unwrap(), 18.0);
        double_eq(polygon_perimeter(&closed).unwrap(), 22.0);
        near(polygon_centroid(&closed).unwrap().easting, 7.0 / 3.0, 1e-14);
    }

    #[test]
    fn regression_parcel_at_utm_magnitude_keeps_full_precision() {
        // Vertices in units of 1/1024 m so that every coordinate, and every
        // translation by the UTM offset, is an exact double. The reference area
        // is computed in exact 64-bit integer arithmetic: no floating point
        // involved.
        const UNIT: i64 = 1024;
        let local: [(i64, i64); 5] = [
            (0, 0),
            (13 * UNIT + 517, 142 * UNIT + 333),
            (97 * UNIT + 259, 171 * UNIT + 801),
            (121 * UNIT + 77, 64 * UNIT + 1001),
            (60 * UNIT + 5, -22 * UNIT - 611),
        ];
        // in (1/1024 m)^2, x = easting, y = northing
        let mut twice_area_units: i64 = 0;
        for (i, a) in local.iter().enumerate() {
            let b = local[(i + 1) % local.len()];
            twice_area_units += a.1 * b.0 - b.1 * a.0;
        }
        let exact_area = (twice_area_units as f64).abs() / (2.0 * (UNIT * UNIT) as f64);
        // a parcel of ~1.3 ha, sanity of the fixture
        assert!(exact_area > 10000.0);

        // N ~ 5e6
        let origin_northing: i64 = 5000000 * UNIT + 777;
        // E ~ 5e5
        let origin_easting: i64 = 500000 * UNIT + 333;
        let utm: Vec<Coordinate2> = local
            .iter()
            .map(|v| {
                c(
                    (origin_northing + v.0) as f64 / UNIT as f64,
                    (origin_easting + v.1) as f64 / UNIT as f64,
                )
            })
            .collect();

        let area = polygon_area(&utm).unwrap();
        // Coordinate differences are exact and their products fit in 53 bits, so
        // the local-origin evaluation is exact here.
        near(area, exact_area, 1e-9);

        // The textbook shoelace on the raw coordinates multiplies numbers of ~5e5
        // and ~5e6: each product (~2.5e12) rounds at ~5e-4 m^2. This is the
        // failure the local origin exists to prevent; if it ever "passes", the
        // fixture is too easy.
        let mut naive_twice = 0.0;
        for (i, a) in utm.iter().enumerate() {
            let b = utm[(i + 1) % utm.len()];
            naive_twice += a.easting * b.northing - b.easting * a.northing;
        }
        let naive_error = (0.5 * naive_twice.abs() - exact_area).abs();
        assert!(naive_error > 1e-6);
        assert!((area - exact_area).abs() < naive_error);

        // Centroid: the local and the UTM parcel differ by the exact offset.
        let small: Vec<Coordinate2> = local
            .iter()
            .map(|v| c(v.0 as f64 / UNIT as f64, v.1 as f64 / UNIT as f64))
            .collect();
        let small_centroid = polygon_centroid(&small).unwrap();
        let utm_centroid = polygon_centroid(&utm).unwrap();
        near(
            utm_centroid.northing - origin_northing as f64 / UNIT as f64,
            small_centroid.northing,
            UTM_ROUNDING,
        );
        near(
            utm_centroid.easting - origin_easting as f64 / UNIT as f64,
            small_centroid.easting,
            UTM_ROUNDING,
        );
        near(
            polygon_perimeter(&utm).unwrap(),
            polygon_perimeter(&small).unwrap(),
            1e-9,
        );
    }

    #[test]
    fn polygon_rejects_bad_input() {
        let two = [c(0.0, 0.0), c(1.0, 1.0)];
        assert_eq!(error_code(polygon_area(&two)), ErrorCode::InvalidArgument);
        assert!(polygon_perimeter(&two).is_err());
        assert!(polygon_centroid(&two).is_err());
        assert!(polygon_area(&[]).is_err());

        let bad = [c(0.0, 0.0), c(1.0, NAN), c(2.0, 0.0)];
        assert!(polygon_area(&bad).is_err());
        assert!(polygon_signed_area(&bad).is_err());
    }

    #[test]
    fn collinear_polygon_has_zero_area_and_no_centroid() {
        let line = [c(0.0, 0.0), c(10.0, 10.0), c(20.0, 20.0), c(5.0, 5.0)];
        double_eq(polygon_area(&line).unwrap(), 0.0);
        assert_eq!(
            error_code(polygon_centroid(&line)),
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn property_area_invariant_under_rotation_of_the_vertex_list() {
        let mut random = Random::default();
        for _ in 0..PROPERTY_ITERATIONS {
            let mut polygon = random_polygon(&mut random, c(5000000.0, 500000.0));
            let area = polygon_signed_area(&polygon).unwrap();
            let perimeter = polygon_perimeter(&polygon).unwrap();
            let centroid = polygon_centroid(&polygon).unwrap();
            // generated counter-clockwise
            assert!(area > 0.0);

            polygon.rotate_left(random.integer(1, 2) as usize);
            // Only the local origin changes, i.e. the rounding of ~1e5 m^2 products.
            near(polygon_signed_area(&polygon).unwrap(), area, 1e-9 * area);
            near(polygon_perimeter(&polygon).unwrap(), perimeter, 1e-9);
            near(
                polygon_centroid(&polygon).unwrap().northing,
                centroid.northing,
                UTM_ROUNDING,
            );
            near(
                polygon_centroid(&polygon).unwrap().easting,
                centroid.easting,
                UTM_ROUNDING,
            );
        }
    }

    #[test]
    fn property_reversal_flips_the_sign_only() {
        let mut random = Random::default();
        for _ in 0..PROPERTY_ITERATIONS {
            let mut polygon = random_polygon(&mut random, c(1000.0, 1000.0));
            let signed_area = polygon_signed_area(&polygon).unwrap();
            let area = polygon_area(&polygon).unwrap();
            let centroid = polygon_centroid(&polygon).unwrap();

            polygon.reverse();
            near(
                polygon_signed_area(&polygon).unwrap(),
                -signed_area,
                1e-9 * area,
            );
            near(polygon_area(&polygon).unwrap(), area, 1e-9 * area);
            near(
                polygon_centroid(&polygon).unwrap().northing,
                centroid.northing,
                1e-9,
            );
            near(
                polygon_centroid(&polygon).unwrap().easting,
                centroid.easting,
                1e-9,
            );
        }
    }

    #[test]
    fn property_area_invariant_under_translation() {
        let mut random = Random::default();
        for _ in 0..PROPERTY_ITERATIONS {
            let polygon = random_polygon(&mut random, c(0.0, 0.0));
            let area = polygon_area(&polygon).unwrap();
            let perimeter = polygon_perimeter(&polygon).unwrap();

            // Integer metre offsets of UTM size. Adding them rounds each vertex by
            // up to half an ulp at 5e6 (4.7e-10 m); shifting every vertex of a
            // polygon by at most e changes its area by at most perimeter * e.
            let d_n = 5000000.0 + f64::from(random.integer(-100000, 100000));
            let d_e = 500000.0 + f64::from(random.integer(-100000, 100000));
            let vertex_count = polygon.len();
            let moved = translated(polygon, d_n, d_e);
            near(polygon_area(&moved).unwrap(), area, perimeter * 1e-9);
            near(
                polygon_perimeter(&moved).unwrap(),
                perimeter,
                vertex_count as f64 * 2e-9,
            );
        }
    }

    // ---- slope reductions ----------------------------------------------------

    #[test]
    fn slope_reductions() {
        // 100 m at zenith 60°: horizontal 100 sin 60° = 50 sqrt(3), rise 100 cos 60° = 50.
        let zenith = 60.0 * DEG_TO_RAD;
        near(
            horizontal_distance(100.0, zenith).unwrap(),
            50.0 * 3.0_f64.sqrt(),
            1e-12,
        );
        // + instrument 1.5 - target 1.2
        near(
            trigonometric_height_difference(100.0, zenith, 1.5, 1.2).unwrap(),
            50.3,
            1e-12,
        );
        // A level sight has no rise; looking down (zenith > 90°) is negative.
        near(
            trigonometric_height_difference(100.0, HALF_PI, 0.0, 0.0).unwrap(),
            0.0,
            1e-13,
        );
        near(
            trigonometric_height_difference(100.0, 120.0 * DEG_TO_RAD, 0.0, 0.0).unwrap(),
            -50.0,
            1e-12,
        );

        assert!(horizontal_distance(-1.0, zenith).is_err());
        assert!(horizontal_distance(100.0, -0.1).is_err());
        assert!(horizontal_distance(100.0, PI + 0.1).is_err());
        assert!(trigonometric_height_difference(100.0, zenith, NAN, 0.0).is_err());
    }
}
