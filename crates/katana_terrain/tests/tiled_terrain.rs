//! Twin of tests/terrain/test_tiled_terrain.cpp.

#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

pub mod support;

use katana_core::ErrorCode;
use katana_geometry::{Point3, Polyline2};
use katana_terrain::{Breakline, TiledTerrain, TiledTerrainOptions, TinInput};
use support::*;

/// Rough but smooth ground over [0, extent]^2, plus the four corners so that the
/// data extent is exactly the square and the tile grid is predictable.
fn rough_ground(extent: f64, count: i32, random: &mut Random) -> Vec<Point3> {
    let elevation = |x: f64, y: f64| 60.0 + 8.0 * (0.02 * x).sin() * (0.017 * y).cos() + 0.01 * x;
    let mut points: Vec<Point3> = Vec::new();
    for corner in [
        p2(0.0, 0.0),
        p2(extent, 0.0),
        p2(extent, extent),
        p2(0.0, extent),
    ] {
        points.push(p3(corner.x, corner.y, elevation(corner.x, corner.y)));
    }
    for _ in 0..count {
        let x = random.real(0.0, extent);
        let y = random.real(0.0, extent);
        points.push(p3(x, y, elevation(x, y)));
    }
    points
}

fn input_from(points: Vec<Point3>) -> TinInput {
    TinInput {
        points,
        ..TinInput::default()
    }
}

fn options(tile_size: f64, buffer_width: f64) -> TiledTerrainOptions {
    TiledTerrainOptions {
        tile_size,
        buffer_width,
        ..TiledTerrainOptions::default()
    }
}

fn create(input: TinInput, options: &TiledTerrainOptions) -> TiledTerrain {
    TiledTerrain::create(input, options).unwrap_or_else(|error| panic!("{}", error.describe()))
}

fn code_of(input: TinInput, options: &TiledTerrainOptions) -> Option<ErrorCode> {
    TiledTerrain::create(input, options)
        .err()
        .map(|error| error.code)
}

// ---- the tile grid ------------------------------------------------------------------------

mod tiled_terrain_grid {
    use super::*;

    #[test]
    fn geometry_of_the_grid_follows_the_data_extent() {
        let mut random = Random::default();
        let terrain = create(
            input_from(rough_ground(100.0, 200, &mut random)),
            &options(25.0, 5.0),
        );

        // ceil(100/25) = 4 tiles each way.
        assert_eq!(terrain.columns(), 4);
        assert_eq!(terrain.rows(), 4);
        assert_eq!(terrain.tile_count(), 16);
        assert_eq!(terrain.point_count(), 204);
        assert_eq!(terrain.bounds().min, p2(0.0, 0.0));
        assert_eq!(terrain.bounds().max, p2(100.0, 100.0));

        assert_eq!(terrain.tile_bounds(0).min, p2(0.0, 0.0));
        assert_eq!(terrain.tile_bounds(0).max, p2(25.0, 25.0));
        // Tile 9 = row 2, column 1.
        assert_eq!(terrain.tile_bounds(9).min, p2(25.0, 50.0));
        assert_eq!(terrain.tile_bounds(9).max, p2(50.0, 75.0));
        assert_eq!(terrain.tile_bounds(15).min, p2(75.0, 75.0));
        assert_eq!(terrain.tile_bounds(15).max, p2(100.0, 100.0));
        assert_eq!(terrain.buffered_tile_bounds(0).min, p2(-5.0, -5.0));
        assert_eq!(terrain.buffered_tile_bounds(0).max, p2(30.0, 30.0));

        assert_eq!(terrain.tile_at(p2(10.0, 10.0)), Some(0));
        assert_eq!(terrain.tile_at(p2(30.0, 60.0)), Some(9)); // column 1, row 2
        assert_eq!(terrain.tile_at(p2(0.0, 0.0)), Some(0));
        assert_eq!(terrain.tile_at(p2(100.0, 100.0)), Some(15)); // the far corner belongs to the last tile
        assert!(terrain.tile_at(p2(-0.5, 50.0)).is_none());
        assert!(terrain.tile_at(p2(50.0, 100.5)).is_none());
        assert!(terrain.tile_at(p2(f64::NAN, 5.0)).is_none());

        let bad =
            |call: &dyn Fn()| std::panic::catch_unwind(std::panic::AssertUnwindSafe(call)).is_err();
        assert!(
            bad(&|| {
                let _ = terrain.tile_bounds(16);
            }),
            "std::out_of_range"
        );
        assert!(
            bad(&|| {
                let _ = terrain.buffered_tile_bounds(16);
            }),
            "std::out_of_range"
        );
    }
}

// ---- agreement with a monolithic TIN --------------------------------------------------------

mod tiled_terrain {
    use super::*;

    #[test]
    fn a_tile_that_sees_every_point_reproduces_the_monolithic_tin_exactly() {
        // With a buffer wider than the data, every tile triangulates the whole point
        // set in input order, which is precisely what build_tin() was given directly.
        // The surfaces must therefore be bitwise identical, arrays and all.
        let mut random = Random::default();
        let points = rough_ground(100.0, 400, &mut random);
        let monolithic = build_from_points(points.clone());
        assert!(!monolithic.empty());

        let mut terrain = create(input_from(points), &options(25.0, 500.0));
        assert_eq!(terrain.tile_count(), 16);
        assert!(terrain.build_all().is_ok());

        for tile in 0..terrain.tile_count() {
            assert!(terrain.is_built(tile));
            let surface = terrain.tile_surface(tile).expect("built");
            assert!(*surface == monolithic, "tile {tile}");
        }

        // And then every in-bounds answer is certified, because no circumcircle can
        // reach data the tile did not see.
        let mut queries = Random::new(31);
        for _ in 0..300 {
            let x = queries.real(0.0, 100.0);
            let y = queries.real(0.0, 100.0);
            let p = p2(x, y);
            let sample = terrain
                .sample_at(p)
                .unwrap_or_else(|error| panic!("{}", error.describe()));
            let expected = monolithic.elevation_at(p);
            assert_eq!(sample.elevation.is_some(), expected.is_some(), "{p:?}");
            if let (Some(actual), Some(expected)) = (sample.elevation, expected) {
                double_eq(actual, expected);
                assert!(sample.certified, "{p:?}");
            }
        }
    }

    #[test]
    fn certified_samples_equal_the_monolithic_answer() {
        // The contract of TileSample::certified. A buffer of ~3 point spacings over
        // 100 m tiles leaves the interior certified and the hull-adjacent long thin
        // triangles uncertified; wherever the flag is set, the tiled answer must be
        // the monolithic one.
        let mut random = Random::default();
        let points = rough_ground(400.0, 3000, &mut random);
        let monolithic = build_from_points(points.clone());
        assert!(!monolithic.empty());

        let mut terrain = create(input_from(points.clone()), &options(100.0, 20.0));
        assert_eq!(terrain.tile_count(), 16);
        assert!(terrain.build_all().is_ok());

        // The same grid also goes to a terrain with no buffer at all, whose tiles
        // cannot see across their own borders.
        let mut bare = create(input_from(points), &options(100.0, 0.0));
        assert!(bare.build_all().is_ok());

        let mut certified = 0;
        let mut certified_unbuffered = 0;
        let mut checked = 0;
        for row in 0..40 {
            for column in 0..40 {
                let p = p2(10.0 * f64::from(column) + 5.0, 10.0 * f64::from(row) + 5.0);
                let expected = monolithic.elevation_at(p);
                let sample = terrain
                    .sample_at(p)
                    .unwrap_or_else(|error| panic!("{}", error.describe()));
                checked += 1;
                if let (true, Some(actual)) = (sample.certified, sample.elevation) {
                    certified += 1;
                    // Same three vertices, but reached through a different triangle
                    // rotation, so only the rounding of the barycentric weights
                    // differs: ~1e-13 on a 400 m extent with 16 m of relief.
                    near(actual, expected.expect("monolithic answer"), 1e-9);
                }
                let plain = bare
                    .sample_at(p)
                    .unwrap_or_else(|error| panic!("{}", error.describe()));
                if let (true, Some(actual)) = (plain.certified, plain.elevation) {
                    certified_unbuffered += 1;
                    near(actual, expected.expect("monolithic answer"), 1e-9);
                }
            }
        }
        assert_eq!(checked, 1600);
        // Not vacuous in either direction. With a buffer of ~3 point spacings nearly
        // every interior query certifies. With no buffer a query within one
        // circumradius of an internal tile border cannot: its triangle's circumcircle
        // has to reach across a border the tile never looked past. Three internal
        // borders each way cross this grid, so the loss runs into the hundreds.
        assert!(certified > 1200, "{certified}");
        assert!(certified_unbuffered < certified);
        assert!(certified_unbuffered < 1300, "{certified_unbuffered}");
    }

    #[test]
    fn a_plane_is_reproduced_exactly_by_every_tile_certified_or_not() {
        // Any triangulation of points on a plane interpolates that plane, so tiling
        // cannot change the answer even where the certificate cannot be given.
        let mut random = Random::default();
        let plane = Plane::new(0.03, -0.045, 210.0);
        let points = plane_points(&plane, 300.0, 1200, &mut random);

        // deliberately no buffer: tiles share no points at all
        let mut terrain = create(input_from(points), &options(75.0, 0.0));
        assert_eq!(terrain.tile_count(), 16);
        assert!(terrain.build_all().is_ok());

        let mut queries = Random::new(41);
        let mut answered = 0;
        for _ in 0..1500 {
            let x = queries.real(0.0, 300.0);
            let y = queries.real(0.0, 300.0);
            let p = p2(x, y);
            let elevation = terrain
                .elevation_at(p)
                .unwrap_or_else(|error| panic!("{}", error.describe()));
            let Some(elevation) = elevation else {
                continue; // outside this tile's own convex hull
            };
            answered += 1;
            // Linear interpolation of a linear function: only the rounding of the
            // barycentric weights, on elevations around 210 m.
            near(elevation, plane.at(p.x, p.y), 1e-9);
        }
        assert!(answered > 1000);
    }

    #[test]
    fn compute_tile_is_a_pure_function_of_the_tile() {
        // compute_tile() takes &self and must not depend on what has been built
        // before, so repeating it - and doing it after the tile was stored and
        // evicted - has to give a bitwise identical surface.
        let mut random = Random::default();
        let mut terrain = create(
            input_from(rough_ground(180.0, 600, &mut random)),
            &options(60.0, 10.0),
        );
        assert_eq!(terrain.tile_count(), 9);

        for tile in 0..terrain.tile_count() {
            let first = terrain
                .compute_tile(tile)
                .unwrap_or_else(|error| panic!("{}", error.describe()));
            let second = terrain
                .compute_tile(tile)
                .unwrap_or_else(|error| panic!("{}", error.describe()));
            assert!(first == second, "tile {tile}");

            assert!(terrain.build_tile(tile).is_ok());
            let stored = terrain.tile_surface(tile).expect("built");
            assert!(*stored == first, "tile {tile}");
            assert_eq!(stored.plan_area(), first.plan_area()); // bitwise
        }
    }

    // ---- building, eviction and the state a query needs -----------------------------------------

    #[test]
    fn queries_never_build_and_say_when_the_tile_is_missing() {
        let mut random = Random::default();
        let mut terrain = create(
            input_from(rough_ground(100.0, 300, &mut random)),
            &options(50.0, 8.0),
        );
        assert_eq!(terrain.tile_count(), 4);

        let inside = p2(25.0, 25.0);
        assert_eq!(terrain.tile_at(inside), Some(0));
        assert!(!terrain.is_built(0));
        assert!(terrain.tile_surface(0).is_none());
        assert_eq!(
            terrain.sample_at(inside).err().map(|error| error.code),
            Some(ErrorCode::InvalidState)
        );
        assert_eq!(
            terrain.elevation_at(inside).err().map(|error| error.code),
            Some(ErrorCode::InvalidState)
        );

        assert!(terrain.ensure_tile_at(inside).is_ok());
        assert!(terrain.is_built(0));
        let sample = terrain
            .sample_at(inside)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        let level = sample.elevation.expect("a level");

        // Building again is a no-op, and the stored surface survives it.
        assert!(terrain.build_tile(0).is_ok());
        assert!(terrain.is_built(0));

        terrain.evict_tile(0);
        assert!(!terrain.is_built(0));
        assert!(terrain.tile_surface(0).is_none());
        assert_eq!(
            terrain.sample_at(inside).err().map(|error| error.code),
            Some(ErrorCode::InvalidState)
        );
        // Rebuilding restores exactly the same surface.
        assert!(terrain.build_tile(0).is_ok());
        let again = terrain
            .sample_at(inside)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(again.elevation, Some(level)); // bitwise

        // Outside the data extent there is certainly no surface, and saying so needs
        // no tile at all.
        let outside = terrain
            .sample_at(p2(-10.0, 50.0))
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(outside.elevation.is_none());
        assert!(outside.certified);
        assert!(terrain.ensure_tile_at(p2(-10.0, 50.0)).is_ok()); // nothing to build

        assert_eq!(
            terrain.build_tile(4).err().map(|error| error.code),
            Some(ErrorCode::InvalidArgument)
        );
        terrain.evict_tile(4); // out of range: ignored, not a crash
        assert!(!terrain.is_built(4));
        assert!(terrain.tile_surface(4).is_none());
    }

    #[test]
    fn a_tile_with_too_few_points_is_an_empty_surface_not_an_error() {
        // Two clusters in opposite corners leave the middle tiles without enough
        // points to triangulate. That is terrain with a gap, not a failure.
        let mut points: Vec<Point3> = Vec::new();
        let mut random = Random::default();
        for _ in 0..40 {
            let x = random.real(0.0, 10.0);
            let y = random.real(0.0, 10.0);
            points.push(p3(x, y, 5.0));
            let x = random.real(90.0, 100.0);
            let y = random.real(90.0, 100.0);
            points.push(p3(x, y, 9.0));
        }
        points.push(p3(0.0, 0.0, 5.0));
        points.push(p3(100.0, 100.0, 9.0));

        let mut terrain = create(input_from(points), &options(25.0, 1.0));
        assert_eq!(terrain.tile_count(), 16);
        assert!(terrain.build_all().is_ok());

        let middle = 5; // row 1, column 1: [25, 50]^2, no points nearby
        assert!(terrain.is_built(middle));
        assert!(terrain.tile_surface(middle).expect("built").empty());

        let sample = terrain
            .sample_at(p2(37.5, 37.5))
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(sample.elevation.is_none());
        assert!(!sample.certified); // inside the extent, so nothing is proved

        let corner = terrain
            .sample_at(p2(5.0, 5.0))
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        double_eq(corner.elevation.expect("a level"), 5.0);
    }
}

// ---- input validation -----------------------------------------------------------------------

mod tiled_terrain_errors {
    use super::*;

    #[test]
    fn rejects_bad_options_and_unsupported_input() {
        let nan = f64::NAN;
        let inf = f64::INFINITY;
        let mut random = Random::default();
        let points = rough_ground(100.0, 50, &mut random);

        let good = options(25.0, 5.0);

        for bad in [0.0, -1.0, nan, inf] {
            let options = TiledTerrainOptions {
                tile_size: bad,
                ..good
            };
            assert_eq!(
                code_of(input_from(points.clone()), &options),
                Some(ErrorCode::InvalidArgument)
            );
        }
        for bad in [-1.0, nan, inf] {
            let options = TiledTerrainOptions {
                buffer_width: bad,
                ..good
            };
            assert_eq!(
                code_of(input_from(points.clone()), &options),
                Some(ErrorCode::InvalidArgument)
            );
        }

        // 100 m of extent at 0.1 m tiles is 1000 x 1000 tiles, past MAX_TILE_COUNT.
        let tiny = TiledTerrainOptions {
            tile_size: 0.1,
            ..good
        };
        assert_eq!(
            code_of(input_from(points.clone()), &tiny),
            Some(ErrorCode::InvalidArgument)
        );

        let mut with_breakline = input_from(points.clone());
        with_breakline.breaklines.push(Breakline {
            vertices: vec![p3(0.0, 0.0, 0.0), p3(50.0, 50.0, 1.0)],
            closed: false,
        });
        assert_eq!(code_of(with_breakline, &good), Some(ErrorCode::Unsupported));

        let mut with_boundary = input_from(points.clone());
        with_boundary.boundary =
            Polyline2::new(vec![p2(1.0, 1.0), p2(90.0, 1.0), p2(90.0, 90.0)], true);
        assert_eq!(code_of(with_boundary, &good), Some(ErrorCode::Unsupported));

        let mut with_hole = input_from(points.clone());
        with_hole.holes.push(Polyline2::new(
            vec![p2(40.0, 40.0), p2(60.0, 40.0), p2(60.0, 60.0)],
            true,
        ));
        assert_eq!(code_of(with_hole, &good), Some(ErrorCode::Unsupported));

        let too_few = input_from(vec![p3(0.0, 0.0, 0.0), p3(1.0, 0.0, 0.0)]);
        assert_eq!(
            code_of(too_few, &good),
            Some(ErrorCode::TriangulationFailure)
        );
        assert_eq!(
            code_of(TinInput::default(), &good),
            Some(ErrorCode::TriangulationFailure)
        );

        let mut non_finite = input_from(points);
        non_finite.points.push(p3(10.0, nan, 3.0));
        let failure = TiledTerrain::create(non_finite, &good).expect_err("refused");
        assert_eq!(failure.code, ErrorCode::InvalidArgument);
        assert!(failure.context.contains("point="));
    }

    #[test]
    fn degenerate_extents_still_give_one_tile() {
        // All points on one line: the extent has zero height, which must round up to a
        // single row rather than to zero rows. The tile itself cannot be triangulated.
        let mut input = TinInput::default();
        for i in 0..10 {
            input.points.push(p3(f64::from(i), 0.0, 1.0));
        }
        let mut terrain = create(input, &options(4.0, 0.0));
        assert_eq!(terrain.columns(), 3); // ceil(9/4)
        assert_eq!(terrain.rows(), 1);
        assert_eq!(terrain.tile_count(), 3);
        assert!(terrain.build_all().is_ok());
        for tile in 0..terrain.tile_count() {
            assert!(
                terrain.tile_surface(tile).expect("built").empty(),
                "tile {tile}"
            );
        }
    }
}

// ---- build_all builds the tiles across the cores ----------------------------

mod tiled_terrain_build {
    use super::*;

    #[test]
    fn building_every_tile_at_once_gives_the_same_surfaces_as_building_them_in_turn() {
        // build_all() hands the tiles to the task pool; build_tile() in a loop is the
        // serial run. A tile's triangulation depends on its own buffered points and
        // nothing else, so the two must agree vertex for vertex and triangle for
        // triangle - not merely in elevation.
        let mut random = Random::new(20260923);
        let options = options(40.0, 8.0);
        let points = rough_ground(200.0, 4000, &mut random);

        let mut parallel = create(input_from(points.clone()), &options);
        let mut serial = create(input_from(points), &options);

        assert!(
            parallel.tile_count() > 8,
            "one tile would share nothing out"
        );
        assert!(parallel.build_all().is_ok());
        for tile in 0..serial.tile_count() {
            assert!(serial.build_tile(tile).is_ok(), "tile {tile}");
        }

        assert_eq!(parallel.tile_count(), serial.tile_count());
        let mut triangles_seen = 0;
        for tile in 0..parallel.tile_count() {
            let a = parallel.tile_surface(tile).expect("built");
            let b = serial.tile_surface(tile).expect("built");
            assert_eq!(a.vertices().len(), b.vertices().len(), "tile {tile}");
            assert_eq!(a.triangle_count(), b.triangle_count(), "tile {tile}");
            for (v, (p, q)) in a.vertices().iter().zip(b.vertices().iter()).enumerate() {
                assert_eq!(p.x, q.x, "tile {tile} vertex {v}");
                assert_eq!(p.y, q.y, "tile {tile} vertex {v}");
                assert_eq!(p.z, q.z, "tile {tile} vertex {v}");
            }
            for (t, (p, q)) in a.triangles().iter().zip(b.triangles().iter()).enumerate() {
                assert_eq!(p, q, "tile {tile} triangle {t}");
            }
            triangles_seen += a.triangle_count();
        }
        assert!(
            triangles_seen > 1000,
            "the comparison must have had something to compare"
        );
    }
}
