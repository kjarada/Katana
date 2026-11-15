//! tests/geo/test_grid_verb.cpp: RASTER GRID, surveyed points to a DEM
//! (src/katana_app/geo/grid_verbs.rs, docs/terrain.md "Gridding points to a
//! DEM"), through the executor, as the window runs it. Every expected value is
//! worked by hand from the plane the points are surveyed on, z = 100 + x/10 +
//! y/20. Its RasterGridContract suite is step 5's, and GridSession, which
//! drives the MCP server, is the MCP's (step 6, agent C).

mod grid_verb {
    use std::sync::atomic::AtomicBool;

    use katana_app::geo::dem_support::read_band_one_from_file;
    use katana_app::geo::replies::{Record, parse_records};
    use katana_app::geo::{handles, help_text};
    use katana_commands::create_entities;
    use katana_core::ErrorCode;
    use katana_entity::entity::set_heights;
    use katana_entity::{Entity, Geometry, PointGeometry, PropertyValue};
    use katana_geometry::Point2;
    use katana_interop::RasterRole;
    use katana_interop::geo::GRID_NO_DATA;

    use crate::geo_fixture::{GeoFixture, holds, must, near, refused};
    use crate::raster_support::double_eq;

    fn plane(x: f64, y: f64) -> f64 {
        100.0 + x / 10.0 + y / 20.0
    }

    fn spot(x: f64, y: f64, z: Option<f64>) -> Entity {
        let mut entity = Entity {
            geometry: Geometry::Point(PointGeometry {
                position: Point2::new(x, y),
            }),
            ..Entity::default()
        };
        if let Some(z) = z {
            set_heights(&mut entity.properties, &[Some(z)]);
        }
        entity
    }

    /// The C++ fixture's: its scratch folder named after the test.
    fn fixture(name: &str) -> GeoFixture {
        GeoFixture::new("katana-grid-verb-", name)
    }

    fn draw(fixture: &mut GeoFixture, entities: Vec<Entity>) {
        must(fixture.document.execute(create_entities(entities)));
    }

    /// Points every `step` metres over [x0, x1] x [y0, y1], surveyed on the
    /// plane, stepped as the C++'s loops step them.
    fn lattice(fixture: &mut GeoFixture, (x0, y0): (f64, f64), (x1, y1): (f64, f64), step: f64) {
        let mut points = Vec::new();
        let mut x = x0;
        while x <= x1 + 1e-9 {
            let mut y = y0;
            while y <= y1 + 1e-9 {
                points.push(spot(x, y, Some(plane(x, y))));
                y += step;
            }
            x += step;
        }
        draw(fixture, points);
    }

    /// The value of the last reference raster's cell holding (x, y).
    fn value_at(fixture: &GeoFixture, x: f64, y: f64) -> f64 {
        let raster = fixture
            .reference
            .rasters()
            .last()
            .expect("a reference raster");
        let band = must(read_band_one_from_file(&raster.source));
        let g = band.geotransform;
        let column = ((x - g[0]) / g[1]).floor() as i32;
        let row = ((y - g[3]) / g[5]).floor() as i32;
        assert!((0..band.width).contains(&column) && (0..band.height).contains(&row));
        band.values[row as usize * band.width as usize + column as usize]
    }

    fn record<'r>(records: &'r [Record], kind: &str) -> Option<&'r Record> {
        records.iter().find(|found| found.kind == kind)
    }

    // ---- the grid -----------------------------------------------------------------------------------

    #[test]
    fn linear_grid_of_points_on_a_plane_reproduces_the_plane() {
        // Points every 10 m over 0..100 on the plane. Linear interpolation
        // over a triangulation reproduces a plane exactly, so every cell
        // centre holds the plane's value there - (52.5, 47.5): 100 + 5.25 +
        // 2.375 = 107.625 - to Float64 rounding (1e-9; the output is Float64).
        let mut fixture = fixture("LinearGridOfPointsOnAPlaneReproducesThePlane");
        lattice(&mut fixture, (0.0, 0.0), (100.0, 100.0), 10.0);
        let reply = must(fixture.run("RASTER GRID DRAWING method=linear cell=5"));
        let records = parse_records(&reply);
        let grid = record(&records, "grid").unwrap_or_else(|| panic!("{reply}"));
        assert_eq!(grid.get("method"), Some("linear"));
        assert_eq!(grid.get("algorithm"), Some("vector grid linear"));
        assert_eq!(grid.get("z"), Some("geometry"));
        assert_eq!(grid.get("cell"), Some("5"));
        assert_eq!(grid.get("extent"), Some("0,0,100,100"));
        assert_eq!(grid.get("size"), Some("20x20"));
        let scope = record(&records, "scope").expect("a scope record");
        assert_eq!(scope.get("matched"), Some("121"));
        assert_eq!(scope.get("used"), Some("121"));
        assert_eq!(scope.get("points"), Some("121"));
        let output = record(&records, "output").expect("an output record");
        assert_eq!(output.get("target"), Some("reference"));
        assert_eq!(output.get("name"), Some("dem"));
        assert_eq!(output.get("raster"), Some("20x20"));
        assert_eq!(fixture.reference.rasters().len(), 1);
        let first = &fixture.reference.rasters()[0];
        assert_eq!(first.role, RasterRole::Derived);
        assert_eq!(first.derivation, "RASTER GRID DRAWING method=linear cell=5");

        near(value_at(&fixture, 52.5, 47.5), 107.625, 1e-9);
        let band = must(read_band_one_from_file(
            &fixture.reference.rasters()[0].source,
        ));
        for row in 0..band.height {
            for column in 0..band.width {
                let x = band.geotransform[0] + (f64::from(column) + 0.5) * band.geotransform[1];
                let y = band.geotransform[3] + (f64::from(row) + 0.5) * band.geotransform[5];
                near(
                    band.values[row as usize * band.width as usize + column as usize],
                    plane(x, y),
                    1e-9,
                );
            }
        }
        // The no-data value is declared: the surfaces' own, never GDAL's 0.
        assert_eq!(band.no_data, Some(GRID_NO_DATA));
    }

    #[test]
    fn a_heightless_point_is_excluded_and_counted_not_read_as_zero() {
        // A point with no height where the cell (52.5, 47.5) is: read as z = 0
        // it would pull that cell from 107.625 towards the datum.
        let mut fixture = fixture("AHeightlessPointIsExcludedAndCountedNotReadAsZero");
        lattice(&mut fixture, (0.0, 0.0), (100.0, 100.0), 10.0);
        draw(&mut fixture, vec![spot(52.5, 47.5, None)]);
        let reply = must(fixture.run("RASTER GRID DRAWING cell=5"));
        let records = parse_records(&reply);
        let scope = record(&records, "scope").unwrap_or_else(|| panic!("{reply}"));
        assert_eq!(scope.get("matched"), Some("122"));
        assert_eq!(scope.get("used"), Some("121"));
        assert_eq!(scope.get("skipped.heightless"), Some("1"));
        near(value_at(&fixture, 52.5, 47.5), 107.625, 1e-9);
    }

    #[test]
    fn the_gdal_verb_leaves_a_heightless_point_out_of_a_grid_that_reads_z() {
        // Three points at z = 10 and one with no height at (10, 10). Every
        // surveyed height is 10, so every cell of a nearest-neighbour grid is
        // 10; the heightless point, read as z = 0, made the cell nearest it 0.
        let mut fixture = fixture("TheGdalVerbLeavesAHeightlessPointOutOfAGridThatReadsZ");
        draw(
            &mut fixture,
            vec![
                spot(0.0, 0.0, Some(10.0)),
                spot(10.0, 0.0, Some(10.0)),
                spot(0.0, 10.0, Some(10.0)),
                spot(10.0, 10.0, None),
            ],
        );
        let reply = must(
            fixture.run("GDAL vector grid nearest --extent 0,0,10,10 --size 2,2 FROM DRAWING"),
        );
        let records = parse_records(&reply);
        let scope = record(&records, "scope").unwrap_or_else(|| panic!("{reply}"));
        assert_eq!(scope.get("used"), Some("3"));
        assert_eq!(scope.get("skipped.heightless"), Some("1"));
        for (x, y) in [(2.5, 2.5), (7.5, 2.5), (2.5, 7.5), (7.5, 7.5)] {
            assert_eq!(value_at(&fixture, x, y), 10.0, "{x},{y}");
        }
        // Buffering reads no Z: the heightless point is given to it.
        let buffered = must(fixture.run("GDAL vector buffer --distance 1 FROM DRAWING"));
        let buffer_records = parse_records(&buffered);
        let taken = record(&buffer_records, "scope").expect("a scope record");
        assert_eq!(taken.get("used"), Some("4"));
    }

    #[test]
    fn z_from_a_property_is_used() {
        // The heights are in the property rl, not in the geometry. One point
        // has no rl and one has a word there: both are left out and counted,
        // where GDAL would read either as 0.
        let mut fixture = fixture("ZFromAPropertyIsUsed");
        let mut points = Vec::new();
        let mut x = 0.0;
        while x <= 100.0 {
            let mut y = 0.0;
            while y <= 100.0 {
                let mut entity = spot(x, y, None);
                entity
                    .properties
                    .insert(String::from("rl"), PropertyValue::Real(plane(x, y)));
                points.push(entity);
                y += 10.0;
            }
            x += 10.0;
        }
        points.push(spot(52.5, 47.5, None));
        let mut worded = spot(57.5, 47.5, None);
        worded.properties.insert(
            String::from("rl"),
            PropertyValue::Text(String::from("not surveyed")),
        );
        points.push(worded);
        draw(&mut fixture, points);

        let reply = must(fixture.run("RASTER GRID DRAWING z=rl cell=5 NAME ground"));
        let records = parse_records(&reply);
        let grid = record(&records, "grid").unwrap_or_else(|| panic!("{reply}"));
        assert_eq!(grid.get("z"), Some("rl"));
        let scope = record(&records, "scope").expect("a scope record");
        assert_eq!(scope.get("used"), Some("121"));
        assert_eq!(scope.get("skipped.no_z"), Some("2"));
        assert_eq!(
            record(&records, "output")
                .expect("an output record")
                .get("name"),
            Some("ground")
        );
        near(value_at(&fixture, 52.5, 47.5), 107.625, 1e-9);
        near(value_at(&fixture, 57.5, 47.5), plane(57.5, 47.5), 1e-9);
    }

    #[test]
    fn the_extent_defaults_to_the_scope_bounds() {
        // Points over 10..90 x 20..80 at a 5 m cell: exactly 16 x 12 cells
        // from (10, 20), since the bounds already fall on whole cells.
        let mut fixture = fixture("TheExtentDefaultsToTheScopeBounds");
        lattice(&mut fixture, (10.0, 20.0), (90.0, 80.0), 10.0);
        let reply = must(fixture.run("RASTER GRID DRAWING cell=5"));
        let records = parse_records(&reply);
        let grid = record(&records, "grid").unwrap_or_else(|| panic!("{reply}"));
        assert_eq!(grid.get("extent"), Some("10,20,90,80"));
        assert_eq!(grid.get("size"), Some("16x12"));
        let band = must(read_band_one_from_file(
            &fixture
                .reference
                .rasters()
                .last()
                .expect("the grid's raster")
                .source,
        ));
        double_eq(band.geotransform[0], 10.0);
        double_eq(band.geotransform[3], 80.0);
        double_eq(band.geotransform[1], 5.0);
        double_eq(band.geotransform[5], -5.0);
    }

    #[test]
    fn bounds_off_the_cell_grow_outwards_to_whole_cells() {
        // Points over 12..92 x 22..82: outwards to the 5 m cells round them,
        // 10..95 x 20..85 - 17 x 13 cells, each exactly 5 m (GDAL itself
        // would stretch the cells to fit the bounds).
        let mut fixture = fixture("BoundsOffTheCellGrowOutwardsToWholeCells");
        lattice(&mut fixture, (12.0, 22.0), (92.0, 82.0), 10.0);
        let reply = must(fixture.run("RASTER GRID DRAWING cell=5"));
        let records = parse_records(&reply);
        let grid = record(&records, "grid").unwrap_or_else(|| panic!("{reply}"));
        assert_eq!(grid.get("extent"), Some("10,20,95,85"));
        assert_eq!(grid.get("size"), Some("17x13"));
        let band = must(read_band_one_from_file(
            &fixture
                .reference
                .rasters()
                .last()
                .expect("the grid's raster")
                .source,
        ));
        double_eq(band.geotransform[1], 5.0);
    }

    #[test]
    fn bounds_on_the_cell_at_an_mga_northing_stay_on_it() {
        // Four points 1 m apart at easting 330000.1 and northing 6250000.3
        // (MGA zone 56 magnitudes), gridded at 0.1 m: the bounds lie on 0.1 m
        // lines, so the grid is 10 x 10 cells from (330000.1, 6250000.3). In
        // binary64, 6250000.3 / 0.1 is 62500002.99999999 (one ulp there is
        // 7.5e-9), so a floor that allows 1e-9 of a cell put the grid's foot
        // at 6250000.2: 11 rows, the extra one holding no datum.
        let mut fixture = fixture("BoundsOnTheCellAtAnMgaNorthingStayOnIt");
        let mut points = Vec::new();
        for x in [330000.1, 330001.1] {
            for y in [6250000.3, 6250001.3] {
                points.push(spot(x, y, Some(100.0)));
            }
        }
        draw(&mut fixture, points);
        let reply = must(fixture.run("RASTER GRID DRAWING cell=0.1"));
        let records = parse_records(&reply);
        assert_eq!(
            record(&records, "grid")
                .unwrap_or_else(|| panic!("{reply}"))
                .get("size"),
            Some("10x10"),
            "{reply}"
        );
        let band = must(read_band_one_from_file(
            &fixture
                .reference
                .rasters()
                .last()
                .expect("the grid's raster")
                .source,
        ));
        // Within 1e-6 m: k x 0.1 in binary64 is off the decimal by an ulp or
        // two (1e-9 m here), far below the 0.1 m a wrong row would be.
        near(band.geotransform[0], 330000.1, 1e-6);
        near(
            band.geotransform[3] + 10.0 * band.geotransform[5],
            6250000.3,
            1e-6,
        );
    }

    #[test]
    fn a_given_extent_keeps_its_corner_and_size_takes_it_as_it_is() {
        let mut fixture = fixture("AGivenExtentKeepsItsCornerAndSizeTakesItAsItIs");
        lattice(&mut fixture, (0.0, 0.0), (100.0, 100.0), 10.0);
        // 0..99 at 5 m is 19.8 cells: 20, so 0..100.
        let reply = must(fixture.run("RASTER GRID DRAWING cell=5 extent=0,0,99,99"));
        let records = parse_records(&reply);
        assert_eq!(
            record(&records, "grid")
                .unwrap_or_else(|| panic!("{reply}"))
                .get("extent"),
            Some("0,0,100,100")
        );
        // 10 x 10 cells over 0..100: 10 m each.
        let reply =
            must(fixture.run("RASTER GRID DRAWING size=10x10 extent=0,0,100,100 NAME coarse"));
        let records = parse_records(&reply);
        assert_eq!(
            record(&records, "grid")
                .unwrap_or_else(|| panic!("{reply}"))
                .get("cell"),
            Some("10")
        );
        near(value_at(&fixture, 55.0, 45.0), plane(55.0, 45.0), 1e-9);
    }

    #[test]
    fn inverse_distance_takes_power_and_radius() {
        // Every 10 m cell's centre is 7.07 m from the four lattice points
        // round it and 15.8 m from the next; a radius of 8 m takes the four,
        // equally weighted, and the mean of a plane at four symmetric corners
        // is the plane at the centre: (55, 45) holds 100 + 5.5 + 2.25 =
        // 107.75.
        let mut fixture = fixture("InverseDistanceTakesPowerAndRadius");
        lattice(&mut fixture, (0.0, 0.0), (100.0, 100.0), 10.0);
        let reply =
            must(fixture.run("RASTER GRID DRAWING method=invdist power=2 radius=8 cell=10"));
        let records = parse_records(&reply);
        assert_eq!(
            record(&records, "grid")
                .unwrap_or_else(|| panic!("{reply}"))
                .get("method"),
            Some("invdist")
        );
        near(value_at(&fixture, 55.0, 45.0), 107.75, 1e-9);
    }

    #[test]
    fn an_empty_scope_is_reported() {
        let mut fixture = fixture("AnEmptyScopeIsReported");
        lattice(&mut fixture, (0.0, 0.0), (100.0, 100.0), 10.0);
        let reply = must(fixture.run("RASTER GRID AREA 1000,1000,1010,1010 cell=5"));
        let records = parse_records(&reply);
        assert_eq!(
            record(&records, "grid")
                .unwrap_or_else(|| panic!("{reply}"))
                .get("ran"),
            Some("no")
        );
        assert_eq!(
            record(&records, "scope")
                .expect("a scope record")
                .get("matched"),
            Some("0")
        );
        assert!(fixture.reference.rasters().is_empty());
    }

    #[test]
    fn points_with_no_height_at_all_are_reported_and_nothing_runs() {
        let mut fixture = fixture("PointsWithNoHeightAtAllAreReportedAndNothingRuns");
        draw(
            &mut fixture,
            vec![
                spot(0.0, 0.0, None),
                spot(10.0, 0.0, None),
                spot(0.0, 10.0, None),
            ],
        );
        let reply = must(fixture.run("RASTER GRID DRAWING cell=5"));
        let records = parse_records(&reply);
        assert_eq!(
            record(&records, "grid")
                .unwrap_or_else(|| panic!("{reply}"))
                .get("ran"),
            Some("no")
        );
        assert_eq!(
            record(&records, "scope")
                .expect("a scope record")
                .get("skipped.heightless"),
            Some("3")
        );
        assert!(fixture.reference.rasters().is_empty());
    }

    #[test]
    fn what_the_verb_cannot_do_is_refused_before_anything_runs() {
        let mut fixture = fixture("WhatTheVerbCannotDoIsRefusedBeforeAnythingRuns");
        lattice(&mut fixture, (0.0, 0.0), (20.0, 20.0), 10.0);
        let mut refuses = |line: &str, code: ErrorCode, says: &str| {
            let error = refused(fixture.run(line), line);
            assert_eq!(error.code, code, "{line}");
            holds(&error.describe(), says);
        };
        // The methods are the catalogue's, and the refusal lists them.
        refuses(
            "RASTER GRID DRAWING method=kriging",
            ErrorCode::InvalidArgument,
            "linear",
        );
        // power is for the inverse-distance methods only.
        refuses(
            "RASTER GRID DRAWING method=linear power=2",
            ErrorCode::InvalidArgument,
            "invdist",
        );
        refuses(
            "RASTER GRID DRAWING cell=5 size=10x10",
            ErrorCode::InvalidArgument,
            "give one",
        );
        refuses(
            "RASTER GRID DRAWING cell=0",
            ErrorCode::InvalidArgument,
            "positive",
        );
        refuses(
            "RASTER GRID DRAWING cell=5 cell=2",
            ErrorCode::InvalidArgument,
            "once",
        );
        refuses(
            "RASTER GRID DRAWING extent=0,0,0,10",
            ErrorCode::InvalidArgument,
            "no area",
        );
        refuses(
            "RASTER GRID DRAWING TO LAYER terrain/dem",
            ErrorCode::Unsupported,
            "raster",
        );
        refuses(
            "RASTER GRID DRAWING bogus",
            ErrorCode::InvalidArgument,
            "bogus",
        );
        // A grid of more than 25 million cells, and the cell that would fit.
        refuses(
            "RASTER GRID DRAWING cell=0.001",
            ErrorCode::InvalidArgument,
            "0.004",
        );
        assert!(fixture.reference.rasters().is_empty());
    }

    #[test]
    fn a_where_filter_and_the_options_may_come_in_any_order() {
        // The lattice on the layer spots, and a stray point at the datum on 0
        // where the cell (52.5, 47.5) is. WHERE LAYER=spots leaves the stray
        // out, so the cell holds the plane's 107.625; and method= after the
        // filter is the verb's option, not a condition.
        let mut fixture = fixture("AWhereFilterAndTheOptionsMayComeInAnyOrder");
        fixture.type_line("LAYER NEW spots");
        let mut points = Vec::new();
        let mut x = 0.0;
        while x <= 100.0 {
            let mut y = 0.0;
            while y <= 100.0 {
                let mut entity = spot(x, y, Some(plane(x, y)));
                entity.layer = String::from("spots");
                points.push(entity);
                y += 10.0;
            }
            x += 10.0;
        }
        draw(&mut fixture, points);
        draw(&mut fixture, vec![spot(52.5, 47.5, Some(0.0))]);
        let reply = must(fixture.run("RASTER GRID cell=5 DRAWING WHERE LAYER=spots method=linear"));
        let records = parse_records(&reply);
        assert_eq!(
            record(&records, "grid")
                .unwrap_or_else(|| panic!("{reply}"))
                .get("method"),
            Some("linear")
        );
        assert_eq!(
            record(&records, "scope")
                .expect("a scope record")
                .get("matched"),
            Some("121")
        );
        near(value_at(&fixture, 52.5, 47.5), 107.625, 1e-9);
    }

    #[test]
    fn preview_changes_nothing_and_a_file_needs_overwrite_to_be_replaced() {
        let mut fixture = fixture("PreviewChangesNothingAndAFileNeedsOverwriteToBeReplaced");
        lattice(&mut fixture, (0.0, 0.0), (100.0, 100.0), 10.0);
        let preview = must(fixture.run("RASTER GRID DRAWING cell=5 PREVIEW"));
        holds(&preview, "preview valid=yes changed=no");
        assert!(fixture.reference.rasters().is_empty());

        let _ = std::fs::create_dir_all(fixture.scratch.path());
        let file = fixture.scratch.file("dem.tif");
        let written = must(fixture.run(&format!("RASTER GRID DRAWING cell=5 TO FILE \"{file}\"")));
        holds(&written, "kind=file target=file");
        assert!(fixture.scratch.path().join("dem.tif").exists());
        let again = refused(
            fixture.run(&format!("RASTER GRID DRAWING cell=5 TO FILE \"{file}\"")),
            "a file in the way",
        );
        assert_eq!(again.code, ErrorCode::AlreadyExists);
        must(fixture.run(&format!(
            "RASTER GRID DRAWING cell=5 TO FILE \"{file}\" OVERWRITE"
        )));
    }

    #[test]
    fn a_cancelled_grid_adds_nothing() {
        let mut fixture = fixture("ACancelledGridAddsNothing");
        lattice(&mut fixture, (0.0, 0.0), (100.0, 100.0), 10.0);
        let prepared = must(fixture.prepare("RASTER GRID DRAWING cell=5"));
        assert!(prepared.reply.is_none());
        let stop = AtomicBool::new(true);
        let work = prepared.work.expect("a run has work");
        let error = refused(work(Some(&stop), None), "a stopped grid");
        assert_eq!(error.code, ErrorCode::InvalidState);
        assert!(fixture.reference.rasters().is_empty());
    }

    #[test]
    fn the_verb_is_in_the_help_every_front_end_shows() {
        assert!(handles("RASTER GRID DRAWING"));
        assert!(handles("raster grid"));
        assert!(!handles("RASTER NOSUCH"));
        holds(&help_text(), "RASTER GRID");
    }
}
