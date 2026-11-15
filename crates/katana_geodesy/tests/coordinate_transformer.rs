//! The twins of tests/geodesy/test_coordinate_transformer.cpp.
//!
//! Provenance of expected values in this file
//!   [identity]  exact mathematical property of the CRS definition
//!   [oracle]    formula coded inside the tests (tests/support/mod.rs)
//!   [published] worked example of an authority, quoted with its printed precision
//!   [self]      PROJ self-consistency only - says nothing about absolute truth
//! Every assertion on a coordinate value carries one of these tags.
//!
//! GeodesyTransformer.MovedFromTransformerReportsInvalidState has no twin: a
//! Rust value cannot be used after it is moved, so the moved-from transformer
//! it calls does not compile.

#![allow(
    clippy::excessive_precision,
    clippy::approx_constant,
    reason = "Preserve decimal constants and expected values copied verbatim from the C++ oracle."
)]

pub mod support;

use katana_core::ErrorCode;
use katana_core::text::format_exact_real;
use katana_geodesy::{
    Coordinate, CoordinateReferenceSystem, CoordinateTransformer, GeographicCoordinate,
    GeographicExtent, LengthUnit, ProjectedCoordinate, TransformerOptions, UNSPECIFIED_EPOCH,
};
use katana_math::Vec3;
use katana_math::numerics::tolerance;

use support::{
    COORDINATE_AS_DEGREES, GridPosition, PROPERTY_ITERATIONS, Random, TransverseMercator, WGS84_A,
    WGS84_INVERSE_F, assert_err, assert_ok, dms, geodetic_to_geocentric, ground_separation_metres,
    half_unit, kruger_forward, make_transformer, make_transformer_with, near, near_in, same_bits,
};

const WGS84: i32 = 4326;
const UTM30N: i32 = 32630;

fn utm_zone30() -> TransverseMercator {
    TransverseMercator {
        central_meridian_deg: -3.0, // zone 30 spans 6 W .. 0
        ..TransverseMercator::default()
    }
}

// The C++'s GeographicCoordinate{latitude, longitude}: height 0.
fn geographic(latitude: f64, longitude: f64) -> GeographicCoordinate {
    GeographicCoordinate::new(latitude, longitude, 0.0)
}

// The C++'s ProjectedCoordinate{easting, northing}: height 0.
fn projected(easting: f64, northing: f64) -> ProjectedCoordinate {
    ProjectedCoordinate::new(easting, northing, 0.0)
}

// The C++'s Coordinate{x, y}: z 0, no epoch.
fn coordinate(x: f64, y: f64) -> Coordinate {
    Coordinate::new(x, y, 0.0)
}

mod geodesy_transformer {
    use super::*;

    // ---- regression: known control values --------------------------------------------

    #[test]
    fn utm30_n_central_meridian_maps_to_false_easting() {
        // [identity] On the central meridian eta = 0, hence E = false easting
        // exactly; on the equator xi = 0, hence N = 0 exactly.
        let mut transformer = assert_ok(make_transformer(WGS84, UTM30N));
        let origin =
            assert_ok(transformer.forward_as::<ProjectedCoordinate, _>(geographic(0.0, -3.0)));
        near(origin.easting, 500000.0, tolerance::GEOMETRIC);
        near(origin.northing, 0.0, tolerance::GEOMETRIC);

        for latitude in [-80.0, -33.5, 12.25, 47.0, 84.0] {
            let on_meridian = assert_ok(
                transformer.forward_as::<ProjectedCoordinate, _>(geographic(latitude, -3.0)),
            );
            near_in(
                on_meridian.easting,
                500000.0,
                tolerance::GEOMETRIC,
                &format!("latitude {latitude:?}"),
            );
        }
    }

    #[test]
    fn utm30_n_equator_and_meridian_symmetry() {
        let mut transformer = assert_ok(make_transformer(WGS84, UTM30N));
        let mut random = Random::default();
        for _ in 0..PROPERTY_ITERATIONS {
            let latitude = random.real(0.0, 84.0);
            let offset = random.real(0.0, 3.0);

            // [identity] Points of the equator have northing 0.
            let equator = assert_ok(
                transformer.forward_as::<ProjectedCoordinate, _>(geographic(0.0, -3.0 + offset)),
            );
            near(equator.northing, 0.0, tolerance::GEOMETRIC);

            // [identity] Mirror images about the central meridian: eastings
            // symmetric about 500 000, equal northings.
            let east = transformer
                .forward_as::<ProjectedCoordinate, _>(geographic(latitude, -3.0 + offset));
            let west = transformer
                .forward_as::<ProjectedCoordinate, _>(geographic(latitude, -3.0 - offset));
            let east = assert_ok(east);
            let west = assert_ok(west);
            near(
                east.easting - 500000.0,
                500000.0 - west.easting,
                tolerance::GEOMETRIC,
            );
            near(east.northing, west.northing, tolerance::GEOMETRIC);

            // [identity] Mirror images about the equator (false northing is 0
            // here).
            let south = assert_ok(
                transformer
                    .forward_as::<ProjectedCoordinate, _>(geographic(-latitude, -3.0 + offset)),
            );
            near(south.northing, -east.northing, tolerance::GEOMETRIC);
            near(south.easting, east.easting, tolerance::GEOMETRIC);
        }
    }

    #[test]
    fn wgs84_origin_in_utm30_n_matches_krueger_series() {
        // The point the removed placeholder got wrong: it "transformed" (0 N,
        // 0 E) to easting 0. The truth is 3 degrees east of the zone's central
        // meridian.
        let mut transformer = assert_ok(make_transformer(WGS84, UTM30N));
        let grid =
            assert_ok(transformer.forward_as::<ProjectedCoordinate, _>(geographic(0.0, 0.0)));

        // [published] 833 978.557 m, quoted to the millimetre.
        near(grid.easting, 833978.557, half_unit(0.001));
        near(grid.northing, 0.0, tolerance::GEOMETRIC); // [identity]

        // [oracle] Krueger series evaluated here, independent of PROJ. First
        // the oracle itself is pinned to the published millimetre, then the
        // library is compared with the oracle far below the millimetre.
        let expected: GridPosition = kruger_forward(&utm_zone30(), 0.0, 0.0);
        near(expected.easting, 833978.557, half_unit(0.001));
        near(grid.easting, expected.easting, tolerance::GEOMETRIC);
        near(grid.northing, expected.northing, tolerance::GEOMETRIC);
    }

    #[test]
    fn ordnance_survey_worked_example() {
        // [published] Ordnance Survey, "A guide to coordinate systems in Great
        // Britain", worked example of the National Grid projection (Airy 1830):
        //   52 39' 27.2531" N, 1 43' 4.5177" E  ->  E 651 409.903 m, N 313 177.270 m.
        // OSGB36 geographic -> British National Grid is a pure map projection,
        // so no datum transformation (and no grid file) is involved.
        let mut transformer = assert_ok(make_transformer(4277, 27700));
        assert!(!transformer.operation().is_ballpark);
        let position = geographic(dms(52.0, 39.0, 27.2531), dms(1.0, 43.0, 4.5177));
        let grid = assert_ok(transformer.forward_as::<ProjectedCoordinate, _>(position));
        near(grid.easting, 651409.903, half_unit(0.001));
        near(grid.northing, 313177.270, half_unit(0.001));

        // And back: the published latitude/longitude carry 0.0001" = 2.8e-8
        // degrees.
        let back = assert_ok(
            transformer.inverse_as::<GeographicCoordinate, _>(projected(651409.903, 313177.270)),
        );
        near(back.latitude, position.latitude, half_unit(0.0001 / 3600.0));
        near(
            back.longitude,
            position.longitude,
            half_unit(0.0001 / 3600.0),
        );
    }

    #[test]
    fn epsg_guidance_note_lambert_conformal_two_parallels_in_us_survey_feet() {
        // [published] IOGP Guidance Note 7-2, Lambert Conic Conformal (2SP)
        // example, NAD27 / Texas South Central (EPSG:32040), results in US
        // SURVEY FEET:
        //   28 30' N, 96 00' W  ->  E 2 963 503.91 ftUS, N 254 759.80 ftUS.
        // Had the library converted with the international foot the easting
        // would be off by 5.9 ft - the unit handling is what this example pins
        // down.
        let mut transformer = assert_ok(make_transformer(4267, 32040));
        assert_eq!(
            transformer.target().length_unit(),
            Some(LengthUnit::UsSurveyFoot)
        );
        let grid =
            assert_ok(transformer.forward_as::<ProjectedCoordinate, _>(geographic(28.5, -96.0)));
        near(grid.easting, 2963503.91, half_unit(0.01));
        near(grid.northing, 254759.80, half_unit(0.01));
    }

    #[test]
    fn epsg_guidance_note_lambert_conformal_one_parallel() {
        // [published] IOGP Guidance Note 7-2, Lambert Conic Conformal (1SP)
        // example, JAD69 / Jamaica National Grid (EPSG:24200):
        //   17 55' 55.80" N, 76 56' 37.26" W  ->  E 255 966.58 m, N 142 493.51 m.
        let mut transformer = assert_ok(make_transformer(4242, 24200));
        let position = geographic(dms(17.0, 55.0, 55.80), -dms(76.0, 56.0, 37.26));
        let grid = assert_ok(transformer.forward_as::<ProjectedCoordinate, _>(position));
        near(grid.easting, 255966.58, half_unit(0.01));
        near(grid.northing, 142493.51, half_unit(0.01));
    }

    #[test]
    fn epsg_guidance_note_oblique_stereographic() {
        // [published] IOGP Guidance Note 7-2, Oblique Stereographic example,
        // Amersfoort / RD New (EPSG:28992):  53 N, 6 E -> E 196 105.283 m,
        // N 557 057.739 m.
        let mut transformer = assert_ok(make_transformer(4289, 28992));
        let grid =
            assert_ok(transformer.forward_as::<ProjectedCoordinate, _>(geographic(53.0, 6.0)));
        near(grid.easting, 196105.283, half_unit(0.001));
        near(grid.northing, 557057.739, half_unit(0.001));
    }

    #[test]
    fn epsg_guidance_note_geographic_to_geocentric() {
        // [published] IOGP Guidance Note 7-2, geographic/geocentric conversion,
        // WGS 84:
        //   53 48' 33.820" N, 2 07' 46.380" E, h = 73.0 m
        //   ->  X 3 771 793.968 m, Y 140 253.342 m, Z 5 124 304.349 m.
        let mut transformer = assert_ok(make_transformer(4979, 4978));
        let latitude = dms(53.0, 48.0, 33.820);
        let longitude = dms(2.0, 7.0, 46.380);
        // Geocentric has no named type: generic Coordinate, x = longitude first.
        let xyz = assert_ok(transformer.forward(Coordinate::new(longitude, latitude, 73.0)));
        near(xyz.x, 3771793.968, half_unit(0.001));
        near(xyz.y, 140253.342, half_unit(0.001));
        near(xyz.z, 5124304.349, half_unit(0.001));

        // [oracle] The closed form agrees with the published digits too.
        let expected = geodetic_to_geocentric(WGS84_A, WGS84_INVERSE_F, latitude, longitude, 73.0);
        near(expected.x, 3771793.968, half_unit(0.001));
        near(xyz.x, expected.x, tolerance::GEOMETRIC);
        near(xyz.y, expected.y, tolerance::GEOMETRIC);
        near(xyz.z, expected.z, tolerance::GEOMETRIC);
    }

    // ---- the axis-order convention ---------------------------------------------------------

    #[test]
    fn generic_coordinate_is_longitude_first_whatever_the_authority_says() {
        // EPSG:4326 declares latitude first (see
        // GeodesyCrs.AxesAreReportedInAuthorityOrder). Katana's Coordinate is
        // nevertheless ALWAYS x = longitude, y = latitude.
        // [identity] (lat 40, lon -3) lies on the central meridian: E = 500 000.
        // With the axes swapped the input would mean (lat -3, lon 40), 43
        // degrees off the zone, and the easting would be millions of metres
        // away.
        let mut transformer = assert_ok(make_transformer(WGS84, UTM30N));
        let generic = assert_ok(transformer.forward(coordinate(-3.0, 40.0))); // x = lon, y = lat
        near(generic.x, 500000.0, tolerance::GEOMETRIC);
        assert!(generic.y > 4.4e6); // 40 degrees of meridian arc: ~4 430 km
        assert!(generic.y < 4.5e6);

        // The named type says the same thing without any order to remember,
        // and the two paths agree bit for bit.
        let named = assert_ok(transformer.forward_as::<ProjectedCoordinate, _>(
            GeographicCoordinate {
                latitude: 40.0,
                longitude: -3.0,
                height: 0.0,
            },
        ));
        assert!(same_bits(named.easting, generic.x));
        assert!(same_bits(named.northing, generic.y));

        // Inverse: x = longitude comes back first.
        let back = assert_ok(transformer.inverse(generic));
        near(back.x, -3.0, COORDINATE_AS_DEGREES);
        near(back.y, 40.0, COORDINATE_AS_DEGREES);
    }

    #[test]
    fn projected_coordinate_is_easting_first_for_northing_first_authorities() {
        // EPSG:3035 (ETRS89-extended / LAEA Europe) declares its axes northing,
        // easting.
        let crs = assert_ok(CoordinateReferenceSystem::from_epsg(3035));
        assert_eq!(crs.axes().len(), 2);
        assert_eq!(crs.axes()[0].direction, "north");

        // [identity] The projection centre (52 N, 10 E) maps to the false
        // origin, E 4 321 000, N 3 210 000 - two values that cannot be
        // confused.
        let mut transformer = assert_ok(make_transformer(4258, 3035));
        let named =
            assert_ok(transformer.forward_as::<ProjectedCoordinate, _>(geographic(52.0, 10.0)));
        near(named.easting, 4321000.0, tolerance::GEOMETRIC);
        near(named.northing, 3210000.0, tolerance::GEOMETRIC);

        let generic = assert_ok(transformer.forward(coordinate(10.0, 52.0)));
        near(generic.x, 4321000.0, tolerance::GEOMETRIC); // x is the EASTING
        near(generic.y, 3210000.0, tolerance::GEOMETRIC);
    }

    #[test]
    fn named_geographic_coordinates_are_degrees_even_for_grad_based_crs() {
        // NTF (Paris), EPSG:4807, measures angles in grads from the Paris
        // meridian.
        // [identity] Lambert zone II (EPSG:27572) has its origin at 52 grad =
        // 46.8 degrees on the Paris meridian, false origin E 600 000,
        // N 2 200 000.
        let mut transformer = assert_ok(make_transformer(4807, 27572));
        assert_eq!(transformer.source().horizontal_unit_name(), "grad");

        let named =
            assert_ok(transformer.forward_as::<ProjectedCoordinate, _>(geographic(46.8, 0.0)));
        near(named.easting, 600000.0, tolerance::GEOMETRIC);
        near(named.northing, 2200000.0, tolerance::GEOMETRIC);

        // The generic escape hatch carries the CRS's own unit: 52 grads.
        let generic = assert_ok(transformer.forward(coordinate(0.0, 52.0)));
        near(generic.x, 600000.0, tolerance::GEOMETRIC);
        near(generic.y, 2200000.0, tolerance::GEOMETRIC);

        let back = assert_ok(transformer.inverse_as::<GeographicCoordinate, _>(named));
        near(back.latitude, 46.8, COORDINATE_AS_DEGREES); // degrees, not grads
        near(back.longitude, 0.0, COORDINATE_AS_DEGREES);
    }

    #[test]
    fn named_types_must_match_the_crs_kind() {
        let mut transformer = assert_ok(make_transformer(WGS84, UTM30N));

        // The source CRS is geographic: a ProjectedCoordinate cannot be
        // forwarded.
        let wrong_source =
            assert_err(transformer.forward_as::<ProjectedCoordinate, _>(projected(500000.0, 0.0)));
        assert_eq!(wrong_source.code, ErrorCode::InvalidArgument);

        // The target CRS is projected: the result cannot be a
        // GeographicCoordinate.
        let wrong_target =
            assert_err(transformer.forward_as::<GeographicCoordinate, _>(geographic(40.0, -3.0)));
        assert_eq!(wrong_target.code, ErrorCode::InvalidArgument);

        // For the inverse the roles swap.
        assert!(
            transformer
                .inverse_as::<ProjectedCoordinate, _>(geographic(40.0, -3.0))
                .is_err()
        );
        assert!(
            transformer
                .inverse_as::<GeographicCoordinate, _>(projected(500000.0, 4.4e6))
                .is_ok()
        );
    }

    // ---- failures are errors, never numbers ----------------------------------------------

    #[test]
    fn latitude_beyond_the_pole_is_rejected() {
        let mut transformer = assert_ok(make_transformer(WGS84, UTM30N));

        let named =
            assert_err(transformer.forward_as::<ProjectedCoordinate, _>(geographic(91.0, -3.0)));
        assert_eq!(named.code, ErrorCode::InvalidArgument);

        // The generic path relies on PROJ, whose message is passed on.
        let generic = assert_err(transformer.forward(coordinate(-3.0, 91.0)));
        assert_eq!(generic.code, ErrorCode::InvalidArgument);
        assert!(generic.context.contains("PROJ"), "{}", generic.describe());

        // The transformer is still usable afterwards.
        assert!(transformer.forward(coordinate(-3.0, 40.0)).is_ok());
    }

    #[test]
    fn non_finite_input_is_rejected() {
        let mut transformer = assert_ok(make_transformer(WGS84, UTM30N));
        let nan = f64::NAN;
        let inf = f64::INFINITY;
        for bad in [
            coordinate(nan, 40.0),
            coordinate(-3.0, inf),
            Coordinate::new(-3.0, 40.0, nan),
            Coordinate {
                x: -3.0,
                y: 40.0,
                z: 0.0,
                t: nan,
            },
            Coordinate {
                x: -3.0,
                y: 40.0,
                z: 0.0,
                t: -inf,
            },
        ] {
            let error = assert_err(transformer.forward(bad));
            assert_eq!(error.code, ErrorCode::InvalidArgument);
        }
        assert!(
            transformer
                .forward_as::<ProjectedCoordinate, _>(geographic(nan, -3.0))
                .is_err()
        );
        assert!(
            transformer
                .inverse_as::<GeographicCoordinate, _>(projected(inf, 0.0))
                .is_err()
        );
    }

    #[test]
    fn coordinates_outside_the_projection_domain_are_rejected() {
        // Inverse Transverse Mercator far outside any possible grid value.
        let mut utm = assert_ok(make_transformer(WGS84, UTM30N));
        for absurd in [
            projected(1e9, 1e9),
            projected(1e12, 0.0),
            projected(5e7, 1e6),
        ] {
            let result = utm.inverse_as::<GeographicCoordinate, _>(absurd);
            assert!(result.is_err(), "{:?}", absurd.easting);
            assert_eq!(assert_err(result).code, ErrorCode::InvalidArgument);
        }

        // A northern Lambert cone has no image of the south pole.
        let mut lambert = assert_ok(make_transformer(4267, 32040));
        let pole =
            assert_err(lambert.forward_as::<ProjectedCoordinate, _>(geographic(-90.0, -96.0)));
        assert_eq!(pole.code, ErrorCode::InvalidArgument);
        assert!(!pole.context.is_empty());
    }

    #[test]
    fn local_engineering_crs_cannot_be_transformed_by_proj() {
        let site = assert_ok(CoordinateReferenceSystem::local_engineering(
            "Site",
            LengthUnit::Metre,
        ));
        let utm = assert_ok(CoordinateReferenceSystem::from_epsg(UTM30N));
        let options = TransformerOptions::default();
        for result in [
            CoordinateTransformer::create(&site, &utm, &options).is_ok(),
            CoordinateTransformer::create(&utm, &site, &options).is_ok(),
        ] {
            assert!(!result);
        }
        let failed = assert_err(CoordinateTransformer::create(&site, &utm, &options));
        assert_eq!(failed.code, ErrorCode::Unsupported);
        assert!(failed.message.contains("site calibration"));
    }

    #[test]
    fn invalid_options_are_rejected() {
        let source = assert_ok(CoordinateReferenceSystem::from_epsg(WGS84));
        let target = assert_ok(CoordinateReferenceSystem::from_epsg(UTM30N));

        let upside_down = TransformerOptions {
            // south > north
            area_of_interest: Some(GeographicExtent {
                west: -6.0,
                south: 60.0,
                east: 0.0,
                north: 50.0,
            }),
            ..TransformerOptions::default()
        };
        assert_eq!(
            assert_err(CoordinateTransformer::create(
                &source,
                &target,
                &upside_down
            ))
            .code,
            ErrorCode::InvalidArgument
        );

        let negative_accuracy = TransformerOptions {
            required_accuracy_metres: Some(-1.0),
            ..TransformerOptions::default()
        };
        assert_eq!(
            assert_err(CoordinateTransformer::create(
                &source,
                &target,
                &negative_accuracy
            ))
            .code,
            ErrorCode::InvalidArgument
        );

        let bad_epoch = TransformerOptions {
            coordinate_epoch: Some(f64::NAN),
            ..TransformerOptions::default()
        };
        assert_eq!(
            assert_err(CoordinateTransformer::create(&source, &target, &bad_epoch)).code,
            ErrorCode::InvalidArgument
        );
    }

    // ---- batch ------------------------------------------------------------------------------

    #[test]
    fn batch_equals_per_point_bitwise() {
        let mut transformer = assert_ok(make_transformer(WGS84, UTM30N));
        let mut random = Random::default();
        let mut points = Vec::new();
        for _ in 0..2000 {
            points.push(Coordinate::new(
                random.real(-9.0, 3.0),
                random.real(-80.0, 84.0),
                random.real(-100.0, 4000.0),
            ));
        }

        let mut batch = points.clone();
        let forward_report = assert_ok(transformer.forward_batch(&mut batch));
        assert_eq!(forward_report.succeeded, points.len());
        assert_eq!(forward_report.failed, 0);
        assert!(forward_report.all_succeeded());
        for (index, (point, batched)) in points.iter().zip(&batch).enumerate() {
            let single = assert_ok(transformer.forward(*point));
            assert!(
                same_bits(single.x, batched.x)
                    && same_bits(single.y, batched.y)
                    && same_bits(single.z, batched.z)
                    && same_bits(single.t, batched.t),
                "forward, point {index}"
            );
        }

        let mut inverse_batch = batch.clone();
        let inverse_report = assert_ok(transformer.inverse_batch(&mut inverse_batch));
        assert!(inverse_report.all_succeeded());
        for (index, (point, inverted)) in batch.iter().zip(&inverse_batch).enumerate() {
            let single = assert_ok(transformer.inverse(*point));
            assert!(
                same_bits(single.x, inverted.x)
                    && same_bits(single.y, inverted.y)
                    && same_bits(single.z, inverted.z),
                "inverse, point {index}"
            );
        }
    }

    #[test]
    fn named_batch_equals_named_per_point_bitwise() {
        let mut transformer = assert_ok(make_transformer(4267, 32040)); // result in US survey feet
        let mut random = Random::default();
        let mut positions = Vec::new();
        for _ in 0..1000 {
            positions.push(GeographicCoordinate::new(
                random.real(27.0, 31.0),
                random.real(-101.0, -94.0),
                random.real(0.0, 500.0),
            ));
        }
        let mut grid = vec![ProjectedCoordinate::default(); positions.len()];
        let report = assert_ok(
            transformer.forward_as_batch::<ProjectedCoordinate, GeographicCoordinate>(
                &positions, &mut grid,
            ),
        );
        assert!(report.all_succeeded());
        for (index, (position, projected_point)) in positions.iter().zip(&grid).enumerate() {
            let single = assert_ok(transformer.forward_as::<ProjectedCoordinate, _>(*position));
            assert!(
                same_bits(single.easting, projected_point.easting)
                    && same_bits(single.northing, projected_point.northing)
                    && same_bits(single.height, projected_point.height),
                "point {index}"
            );
        }

        let mut back = vec![GeographicCoordinate::default(); grid.len()];
        let inverse_report = assert_ok(
            transformer
                .inverse_as_batch::<GeographicCoordinate, ProjectedCoordinate>(&grid, &mut back),
        );
        assert!(inverse_report.all_succeeded());
        for (returned, position) in back.iter().zip(&positions) {
            near(returned.latitude, position.latitude, COORDINATE_AS_DEGREES);
            near(
                returned.longitude,
                position.longitude,
                COORDINATE_AS_DEGREES,
            );
            assert_eq!(returned.height, position.height); // 2D operation: passed through
        }

        let mut too_small = [ProjectedCoordinate::default(); 3];
        let mismatch = assert_err(
            transformer.forward_as_batch::<ProjectedCoordinate, GeographicCoordinate>(
                &positions,
                &mut too_small,
            ),
        );
        assert_eq!(mismatch.code, ErrorCode::InvalidArgument);
    }

    #[test]
    fn batch_marks_failed_points_as_na_n_and_counts_them() {
        let mut transformer = assert_ok(make_transformer(WGS84, UTM30N));
        let mut points = [
            Coordinate::new(-3.0, 40.0, 10.0),
            Coordinate::new(-3.0, 95.0, 10.0), // latitude beyond the pole
            Coordinate::new(-2.0, 41.0, 20.0),
            Coordinate::new(f64::NAN, 41.0, 20.0), // not a number to begin with
            Coordinate::new(-1.0, 42.0, 30.0),
        ];
        let report = assert_ok(transformer.forward_batch(&mut points));
        assert_eq!(report.succeeded, 3);
        assert_eq!(report.failed, 2);
        assert_eq!(report.first_failed_index, 1);
        assert!(!report.all_succeeded());

        for index in [1, 3] {
            assert!(points[index].x.is_nan());
            assert!(points[index].y.is_nan());
            assert!(points[index].z.is_nan());
        }
        // A failure does not disturb its neighbours.
        near(points[0].x, 500000.0, tolerance::GEOMETRIC); // [identity]
        let third = assert_ok(transformer.forward(Coordinate::new(-1.0, 42.0, 30.0)));
        assert!(same_bits(points[4].x, third.x) && same_bits(points[4].y, third.y));

        // Named batch: same policy.
        let positions = [
            geographic(40.0, -3.0),
            geographic(95.0, -3.0),
            geographic(41.0, -2.0),
        ];
        let mut grid = [ProjectedCoordinate::default(); 3];
        let named_report = assert_ok(
            transformer.forward_as_batch::<ProjectedCoordinate, GeographicCoordinate>(
                &positions, &mut grid,
            ),
        );
        assert_eq!(named_report.succeeded, 2);
        assert_eq!(named_report.failed, 1);
        assert_eq!(named_report.first_failed_index, 1);
        assert!(grid[1].easting.is_nan() && grid[1].northing.is_nan() && grid[1].height.is_nan());
        near(grid[0].easting, 500000.0, tolerance::GEOMETRIC);
    }

    #[test]
    fn empty_batch_is_a_no_op() {
        let mut transformer = assert_ok(make_transformer(WGS84, UTM30N));
        let mut none: Vec<Coordinate> = Vec::new();
        let report = assert_ok(transformer.forward_batch(&mut none));
        assert_eq!(report.succeeded, 0);
        assert_eq!(report.failed, 0);
    }

    // ---- ownership and threading ----------------------------------------------------------

    #[test]
    fn clone_is_independent_and_identical() {
        let mut original = assert_ok(make_transformer(WGS84, 27700));
        let mut clone = assert_ok(original.clone());
        assert_eq!(clone.operation().name, original.operation().name);
        assert_eq!(
            clone.operation().proj_pipeline,
            original.operation().proj_pipeline
        );
        assert_eq!(clone.source(), original.source());
        assert_eq!(clone.target(), original.target());

        let point = Coordinate::new(-1.5, 52.5, 100.0);
        let a = assert_ok(original.forward(point));
        let b = assert_ok(clone.forward(point));
        assert!(same_bits(a.x, b.x) && same_bits(a.y, b.y) && same_bits(a.z, b.z));
    }

    #[test]
    fn one_transformer_per_thread() {
        // The documented model: clone() per thread, nothing shared. Four
        // threads transform the same points concurrently and must reproduce
        // the reference result bit for bit (deterministic computation).
        let mut reference = assert_ok(make_transformer(WGS84, UTM30N));
        let mut random = Random::default();
        let mut points = Vec::new();
        for _ in 0..5000 {
            points.push(Coordinate::new(
                random.real(-6.0, 0.0),
                random.real(0.0, 84.0),
                0.0,
            ));
        }
        let mut expected = points.clone();
        assert_ok(reference.forward_batch(&mut expected));

        const THREADS: usize = 4;
        let mut clones = Vec::new();
        for _ in 0..THREADS {
            clones.push(assert_ok(reference.clone()));
        }
        let mut results: Vec<Vec<Coordinate>> = (0..THREADS).map(|_| points.clone()).collect();
        let failures: Vec<i64> = std::thread::scope(|scope| {
            let handles: Vec<_> = clones
                .iter_mut()
                .zip(results.iter_mut())
                .map(|(clone, result)| {
                    scope.spawn(move || match clone.forward_batch(result) {
                        Ok(report) => report.failed as i64,
                        Err(_) => -1,
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap_or(-1))
                .collect()
        });
        for (thread, (result, failure)) in results.iter().zip(&failures).enumerate() {
            assert_eq!(*failure, 0);
            let identical = result
                .iter()
                .zip(&expected)
                .all(|(got, want)| same_bits(got.x, want.x) && same_bits(got.y, want.y));
            assert!(identical, "thread {thread}");
        }
    }
}

mod geodesy_transformer_property {
    use super::*;

    #[test]
    fn utm30_n_agrees_with_krueger_series_across_the_zone() {
        // [oracle] The whole zone, and well beyond its 3 degree half-width, to
        // 0.1 micrometre. The two implementations share no code: PROJ uses the
        // Engsager-Poder formulation, the oracle Karney's.
        let mut transformer = assert_ok(make_transformer(WGS84, UTM30N));
        let mut random = Random::default();
        let mut worst: f64 = 0.0;
        for _ in 0..PROPERTY_ITERATIONS {
            let latitude = random.real(-80.0, 84.0);
            let longitude = random.real(-9.0, 3.0);
            let grid = assert_ok(
                transformer.forward_as::<ProjectedCoordinate, _>(geographic(latitude, longitude)),
            );
            // The EPSG definition of zone 30N has false northing 0 on both
            // hemispheres.
            let expected = kruger_forward(&utm_zone30(), latitude, longitude);
            let at = format!("lat {latitude:?} lon {longitude:?}");
            near_in(grid.easting, expected.easting, tolerance::GEOMETRIC, &at);
            near_in(grid.northing, expected.northing, tolerance::GEOMETRIC, &at);
            worst = worst
                .max((grid.easting - expected.easting).abs())
                .max((grid.northing - expected.northing).abs());
        }
        println!("worst_difference_metres={}", format_exact_real(worst));
    }

    #[test]
    fn geocentric_matches_closed_form_and_round_trips() {
        let mut transformer = assert_ok(make_transformer(4979, 4978));
        let mut random = Random::default();
        for _ in 0..PROPERTY_ITERATIONS {
            let latitude = random.real(-89.9, 89.9);
            let longitude = random.real(-180.0, 180.0);
            let height = random.real(-500.0, 9000.0);

            // [oracle] X = (N + h) cos(phi) cos(lambda) ... coded in the test
            // support.
            let expected =
                geodetic_to_geocentric(WGS84_A, WGS84_INVERSE_F, latitude, longitude, height);
            let xyz = assert_ok(transformer.forward(Coordinate::new(longitude, latitude, height)));
            near(xyz.x, expected.x, tolerance::GEOMETRIC);
            near(xyz.y, expected.y, tolerance::GEOMETRIC);
            near(xyz.z, expected.z, tolerance::GEOMETRIC);

            // [identity] inverse(forward(p)) == p. The inverse is iterative (no
            // closed form exists), so this also checks its convergence.
            let back = assert_ok(transformer.inverse(xyz));
            near(back.x, longitude, COORDINATE_AS_DEGREES);
            near(back.y, latitude, COORDINATE_AS_DEGREES);
            near(back.z, height, tolerance::COORDINATE);
        }
    }

    // ---- round trips --------------------------------------------------------------------------

    #[test]
    #[allow(
        clippy::assertions_on_constants,
        reason = "The C++ asserts the ordering of the two tolerances the case relies on."
    )]
    fn projected_round_trip_is_far_below_the_coordinate_tolerance() {
        // transform(inverse_transform(x)) ~ x. The requirement is
        // tolerance::COORDINATE (0.1 mm). What the arithmetic can deliver is
        // much better: the series are accurate to nanometres and a UTM
        // northing resolves to 1.9e-9 m (1 ulp at 1e7), so grid -> geographic
        // -> grid is asserted at tolerance::GEOMETRIC (0.1 micrometre, ~50
        // ulp) - three orders tighter.
        assert!(tolerance::GEOMETRIC < tolerance::COORDINATE);
        let mut transformer = assert_ok(make_transformer(WGS84, UTM30N));
        let mut random = Random::default();
        let mut worst: f64 = 0.0;
        for _ in 0..PROPERTY_ITERATIONS {
            let grid = ProjectedCoordinate::new(
                random.real(166000.0, 834000.0),
                random.real(0.0, 9300000.0),
                random.real(0.0, 3000.0),
            );
            let geographic_point =
                assert_ok(transformer.inverse_as::<GeographicCoordinate, _>(grid));
            let back =
                assert_ok(transformer.forward_as::<ProjectedCoordinate, _>(geographic_point));
            near(back.easting, grid.easting, tolerance::GEOMETRIC);
            near(back.northing, grid.northing, tolerance::GEOMETRIC);
            assert_eq!(back.height, grid.height);
            worst = worst
                .max((back.easting - grid.easting).abs())
                .max((back.northing - grid.northing).abs());
        }
        println!("worst_round_trip_metres={}", format_exact_real(worst));
    }

    #[test]
    fn geographic_round_trip_is_below_one_nanodegree() {
        // geographic -> projected -> geographic, for several projection
        // methods.
        struct Case {
            geographic: i32,
            projected: i32,
            south: f64,
            north: f64,
            west: f64,
            east: f64,
        }
        let cases = [
            // Transverse Mercator
            Case {
                geographic: 4326,
                projected: 32630,
                south: -80.0,
                north: 84.0,
                west: -9.0,
                east: 3.0,
            },
            // Lambert Conformal Conic 2SP, ftUS
            Case {
                geographic: 4267,
                projected: 32040,
                south: 25.0,
                north: 33.0,
                west: -104.0,
                east: -92.0,
            },
            // Oblique Stereographic
            Case {
                geographic: 4289,
                projected: 28992,
                south: 50.5,
                north: 53.7,
                west: 3.2,
                east: 7.3,
            },
            // Transverse Mercator, Airy
            Case {
                geographic: 4277,
                projected: 27700,
                south: 49.8,
                north: 60.9,
                west: -8.0,
                east: 1.8,
            },
        ];
        for case in &cases {
            let mut transformer = assert_ok(make_transformer(case.geographic, case.projected));
            let mut random = Random::default();
            for _ in 0..PROPERTY_ITERATIONS {
                let latitude = random.real(case.south, case.north);
                let longitude = random.real(case.west, case.east);
                let position = geographic(latitude, longitude);
                let grid = assert_ok(transformer.forward_as::<ProjectedCoordinate, _>(position));
                let back = assert_ok(transformer.inverse_as::<GeographicCoordinate, _>(grid));
                let at = format!("EPSG:{}", case.projected);
                near_in(back.latitude, position.latitude, COORDINATE_AS_DEGREES, &at);
                near_in(
                    back.longitude,
                    position.longitude,
                    COORDINATE_AS_DEGREES,
                    &at,
                );
            }
        }
    }
}

// ---- datum transformations --------------------------------------------------------------------
// What is asserted here holds for ANY installation: with or without the
// optional datum-shift grids (none are installed on the reference machine,
// whose share/proj holds only proj.db and the legacy init files).

mod geodesy_transformer_datum {
    use super::*;

    #[test]
    fn same_datum_conversion_is_not_a_transformation() {
        let transformer = assert_ok(make_transformer(WGS84, UTM30N));
        let operation = transformer.operation();
        assert!(!operation.name.is_empty());
        assert!(operation.is_usable);
        assert!(!operation.is_ballpark);
        assert!(!operation.requires_epoch);
        assert!(operation.grids.is_empty());
        assert!(transformer.missing_grids().is_empty());
        assert!(
            operation.proj_pipeline.contains("utm"),
            "{}",
            operation.proj_pipeline
        );
        // A map projection is exact by definition: declared accuracy 0.
        assert_eq!(operation.accuracy_metres, Some(0.0));
    }

    #[test]
    fn wgs84_to_british_national_grid_reports_its_operation() {
        let options = TransformerOptions {
            // England and Wales
            area_of_interest: Some(GeographicExtent {
                west: -6.0,
                south: 50.0,
                east: 2.0,
                north: 56.0,
            }),
            ..TransformerOptions::default()
        };
        let mut transformer = assert_ok(make_transformer_with(WGS84, 27700, &options));

        let operation = transformer.operation().clone();
        let trace = format!("selected: {}", operation.name);
        assert!(operation.name.contains("OSGB36"), "{trace}");
        assert!(operation.is_usable, "{trace}");
        assert!(!operation.is_ballpark, "{trace}"); // ballpark is refused by default
        assert!(!operation.proj_pipeline.is_empty(), "{trace}");
        // EPSG publishes 1 m for the OSTN15 grid and 2 m for the national
        // Helmert.
        let accuracy = operation
            .accuracy_metres
            .unwrap_or_else(|| panic!("{trace}: no declared accuracy"));
        assert!(accuracy > 0.0, "{trace}");
        assert!(accuracy <= 2.0, "{trace}");
        assert!(transformer.candidates().len() >= 2, "{trace}");

        // Whatever was selected, every grid it uses is present ...
        for grid in &operation.grids {
            assert!(grid.available, "{trace}: {}", grid.name);
        }
        // ... and the caller is told when a better, grid-based candidate was
        // skipped.
        let grid_based = !operation.grids.is_empty();
        if grid_based {
            assert!(transformer.missing_grids().is_empty(), "{trace}");
        } else {
            assert!(
                !transformer.missing_grids().is_empty(),
                "{trace}: the 1 m OSTN15 candidate needs a grid that is not installed"
            );
            for grid in transformer.missing_grids() {
                assert!(!grid.available, "{trace}");
                assert!(!grid.name.is_empty(), "{trace}");
            }
        }

        // A real datum shift was applied. [independent bound] OSGB36 and WGS
        // 84 differ by roughly 100-140 m on the ground across Great Britain; a
        // null (ballpark) transformation would give 0 m, a wrong-sign one
        // ~250 m.
        let position = geographic(dms(52.0, 39.0, 27.2531), dms(1.0, 43.0, 4.5177));
        let shifted = assert_ok(transformer.forward_as::<ProjectedCoordinate, _>(position));
        let mut projection_only = assert_ok(make_transformer(4277, 27700));
        let unshifted = assert_ok(projection_only.forward_as::<ProjectedCoordinate, _>(position));
        let shift =
            (shifted.easting - unshifted.easting).hypot(shifted.northing - unshifted.northing);
        assert!(shift > 50.0, "{trace}");
        assert!(shift < 200.0, "{trace}");

        // Round trip through the datum transformation. This is NOT an
        // identity, and no implementation can make it one: both CRSs are
        // two-dimensional, so the operation saves the input ellipsoidal
        // height, runs the three-dimensional Helmert and restores that same
        // height afterwards (+proj=push/pop +v_3). The forward pass hands the
        // Helmert h = 0 and discards the OSGB36 height it produces - about
        // -45 m, since the Airy 1830 ellipsoid as placed by OSGB36 lies that
        // far above the WGS 84 ellipsoid over Great Britain - and the reverse
        // pass hands it h = 0 again.
        //
        // [independent bound] displacing a point by h_o along the ellipsoid
        // normal before the reverse Helmert moves it horizontally by
        // |h_o| * (|dn| + |th|), where dn is the angle between the two datums'
        // normals at the point (the 134 m datum shift over R = 6.37e6 m, i.e.
        // 2.1e-5 rad) and th the 0.88 arc-second total rotation of EPSG:1314
        // (4.3e-6 rad). With |h_o| <= 56 m anywhere in Great Britain that is
        // at most 1.4 mm, so 2 mm is a bound only a discarded height can sit
        // under: the smallest error of the datum shift itself would be tens
        // of metres.
        const HEIGHTLESS_DATUM_ROUND_TRIP_METRES: f64 = 2e-3;
        let back = assert_ok(transformer.inverse_as::<GeographicCoordinate, _>(shifted));
        let residual = ground_separation_metres(&back, &position);
        assert!(residual < HEIGHTLESS_DATUM_ROUND_TRIP_METRES, "{trace}");
        println!(
            "datum_round_trip_residual_metres={}",
            format_exact_real(residual)
        );
    }

    #[test]
    fn required_accuracy_filters_candidates() {
        let options = TransformerOptions {
            area_of_interest: Some(GeographicExtent {
                west: -6.0,
                south: 50.0,
                east: 2.0,
                north: 56.0,
            }),
            required_accuracy_metres: Some(1.5), // only the OSTN15 grid (1 m) qualifies
            ..TransformerOptions::default()
        };
        match make_transformer_with(WGS84, 27700, &options) {
            Ok(transformer) => {
                // The grid is installed here.
                let accuracy = transformer
                    .operation()
                    .accuracy_metres
                    .expect("a declared accuracy");
                assert!(accuracy <= 1.5);
                assert!(!transformer.operation().grids.is_empty());
            }
            Err(error) => {
                // It is not: refusing beats silently delivering 2 m.
                assert_eq!(error.code, ErrorCode::Unsupported);
                assert!(
                    error.context.contains("missing grid"),
                    "{}",
                    error.describe()
                );
            }
        }
    }

    #[test]
    fn nad27_to_nad83_never_falls_back_to_ballpark_silently() {
        let conus = TransformerOptions {
            // Texas
            area_of_interest: Some(GeographicExtent {
                west: -100.0,
                south: 28.0,
                east: -94.0,
                north: 32.0,
            }),
            ..TransformerOptions::default()
        };
        let strict = make_transformer_with(4267, 4269, &conus);

        let lenient = TransformerOptions {
            allow_ballpark: true,
            ..conus.clone()
        };
        // A ballpark operation always exists.
        let mut permissive = assert_ok(make_transformer_with(4267, 4269, &lenient));

        match strict {
            Ok(mut strict) => {
                // NADCON grids are installed: a genuine, accuracy-rated
                // transformation.
                assert!(!strict.operation().is_ballpark);
                assert!(!strict.operation().grids.is_empty());
                let accuracy = strict
                    .operation()
                    .accuracy_metres
                    .expect("a declared accuracy");
                assert!(accuracy <= 2.0);
                let moved = assert_ok(
                    strict.forward_as::<GeographicCoordinate, _>(geographic(30.0, -97.0)),
                );
                // [independent bound] the NAD27 -> NAD83 shift in Texas is
                // tens of metres.
                assert!((moved.longitude + 97.0).abs() + (moved.latitude - 30.0).abs() > 1e-5);
            }
            Err(error) => {
                // No grids (the reference machine): the default is to REFUSE,
                // naming them.
                assert_eq!(error.code, ErrorCode::Unsupported);
                assert!(
                    error.context.contains("missing grid"),
                    "{}",
                    error.describe()
                );
                assert!(error.context.contains("allowBallpark"));

                // Only an explicit opt-in yields the ballpark operation, and it
                // is labelled.
                let operation = permissive.operation().clone();
                assert!(operation.is_ballpark);
                assert_eq!(operation.accuracy_metres, None); // unknown, not "0"
                assert!(!permissive.missing_grids().is_empty());
                for grid in permissive.missing_grids() {
                    assert!(!grid.available);
                }
                // [self] What "ballpark" means: the numbers pass through
                // unshifted, although the true shift is tens of metres. Hence
                // the refusal above.
                let unmoved = assert_ok(
                    permissive.forward_as::<GeographicCoordinate, _>(geographic(30.0, -97.0)),
                );
                near(unmoved.latitude, 30.0, COORDINATE_AS_DEGREES);
                near(unmoved.longitude, -97.0, COORDINATE_AS_DEGREES);
            }
        }
    }

    #[test]
    fn time_dependent_operation_demands_an_epoch() {
        // ITRF2014 -> ETRF2000 (geocentric, EPSG:7789 -> EPSG:7930) is a
        // 14-parameter time-dependent Helmert transformation: Europe moves
        // ~2.5 cm per year in ITRF.
        let mut transformer = assert_ok(make_transformer(7789, 7930));
        assert!(transformer.operation().requires_epoch);

        let without_epoch = Coordinate::new(3771793.968, 140253.342, 5124304.349);
        assert_eq!(without_epoch.t, UNSPECIFIED_EPOCH);
        let refused = transformer.forward(without_epoch);
        assert!(
            refused.is_err(),
            "PROJ would silently assume the reference epoch"
        );
        let refused = assert_err(refused);
        assert_eq!(refused.code, ErrorCode::InvalidArgument);
        assert!(refused.message.contains("epoch"));

        let at2010 = Coordinate {
            t: 2010.0,
            ..without_epoch
        };
        let at2020 = Coordinate {
            t: 2020.0,
            ..without_epoch
        };
        let result2010 = transformer.forward(at2010);
        let result2020 = transformer.forward(at2020);
        let result2010 = assert_ok(result2010);
        let result2020 = assert_ok(result2020);
        // [independent bound] ten years of Eurasian plate motion at about
        // 2.5 cm/yr.
        let drift = Vec3::new(
            result2020.x - result2010.x,
            result2020.y - result2010.y,
            result2020.z - result2010.z,
        )
        .length();
        assert!(drift > 0.15);
        assert!(drift < 0.35);

        // The batch path refuses the same point and transforms the others.
        let mut batch = [at2010, without_epoch, at2020];
        let report = assert_ok(transformer.forward_batch(&mut batch));
        assert_eq!(report.succeeded, 2);
        assert_eq!(report.failed, 1);
        assert_eq!(report.first_failed_index, 1);
        assert!(batch[1].x.is_nan());
        assert!(same_bits(batch[0].x, result2010.x) && same_bits(batch[2].x, result2020.x));

        // A default epoch in the options serves coordinates that carry none.
        let options = TransformerOptions {
            coordinate_epoch: Some(2020.0),
            ..TransformerOptions::default()
        };
        let mut with_default = assert_ok(make_transformer_with(7789, 7930, &options));
        let defaulted = assert_ok(with_default.forward(without_epoch));
        assert!(
            same_bits(defaulted.x, result2020.x)
                && same_bits(defaulted.y, result2020.y)
                && same_bits(defaulted.z, result2020.z)
        );
    }
}
