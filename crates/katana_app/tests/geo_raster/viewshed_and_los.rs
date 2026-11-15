//! tests/geo/test_viewshed_and_los.cpp: RASTER VIEWSHED and LOS
//! (src/katana_app/geo/viewshed_verbs.rs; docs/terrain.md "Viewshed and line
//! of sight"). Its ViewshedContract and LineOfSight suites are step 5's and
//! step 2's.
//!
//! Every fixture is flat ground at 0 with, where there is one, a wall: the
//! hand-worked shadow is then similar triangles. Curvature is switched off
//! (curvature=none) wherever a shadow's edge is worked by hand, since GDAL
//! lowers the ground by 0.85714 d^2 / 12 741 994 m otherwise - 7e-6 m at 10
//! m, but not zero, and the expectation must be the true one.
//!
//! The wall fixture: 60 x 31 cells of 1 m from (0,0), 0 everywhere but the
//! column x = 20..21 (centre 20.5), which is 1 m high. An observer at the
//! centre (0.5, 15.5) with an eye 1.7 m up sees over the wall top, 1 m high
//! 20 m away; the sight grazing it falls 0.7 m in 20 m and meets the ground at
//! 20 x 1.7 / 0.7 = 48.571 m. So along the observer's row the cells 21 to 48 m
//! out (centres 21.5 to 48.5) are hidden, and those 49 m out and more (49.5
//! on) are seen again. The margins either side are 0.02 m (48 m out: 1.7 -
//! 0.035 x 48 = 0.02 above the ground) and 0.015 m (49 m: 0.015 below), far
//! beyond rounding. GDAL computes the cells on the observer's own row from the
//! cell before them on that row, so the row is exact.

mod viewshed {
    use std::sync::atomic::AtomicBool;

    use katana_cad::Document;
    use katana_commands::create_entities;
    use katana_core::ErrorCode;
    use katana_core::text::{parse_finite_double, parse_integer};
    use katana_entity::{Entity, Geometry, PointGeometry};
    use katana_geometry::{Point2, Segment2};
    use katana_gis::gdal_adapter::same_crs;
    use katana_gis::reproject::crs_to_wkt;
    use katana_gis::{GdalDataset, RasterInfo};
    use katana_interop::RasterRole;

    use crate::geo_fixture::{GeoFixture, TempDir, holds, must, near, refused};
    use crate::raster_support::{ascii_grid, is_empty_dir, records};

    /// The C++ fixture's: its scratch folder named after the test.
    fn fixture(name: &str) -> GeoFixture {
        GeoFixture::new("katana-viewshed-", name)
    }

    /// The C++ fixture's second folder, for the files a case writes.
    fn files(name: &str) -> TempDir {
        TempDir::new("katana-viewshed-", &format!("{name}-files"))
    }

    /// An ESRI ASCII grid of 1 m cells from (0,0), height(column, row) at
    /// each cell centre, row 0 the northern (top) row: whole numbers, as the
    /// C++'s heights are.
    fn grid(
        folder: &TempDir,
        name: &str,
        columns: i32,
        rows: i32,
        height: impl Fn(i32, i32) -> i32,
    ) -> String {
        ascii_grid(folder, name, columns, rows, |column, row| {
            format!("{}", height(column, row))
        })
    }

    fn wall(files: &TempDir) -> String {
        grid(files, "wall.asc", 60, 31, |column, _| {
            i32::from(column == 20)
        })
    }

    /// The last viewshed kept.
    struct Kept {
        info: RasterInfo,
        values: Vec<f64>,
    }

    impl Kept {
        /// Whether the cell whose centre is (x, y) is seen; none when the
        /// raster does not reach it.
        fn seen(&self, x: f64, y: f64) -> Option<bool> {
            let gt = &self.info.geotransform;
            let column = ((x - gt[0]) / gt[1]).floor() as i64;
            let row = ((y - gt[3]) / gt[5]).floor() as i64;
            if column < 0
                || row < 0
                || column >= i64::from(self.info.width)
                || row >= i64::from(self.info.height)
            {
                return None;
            }
            Some(self.values[(row * i64::from(self.info.width) + column) as usize] == 1.0)
        }
    }

    fn kept(fixture: &GeoFixture) -> Kept {
        let raster = fixture
            .reference
            .rasters()
            .last()
            .expect("a viewshed kept as a reference raster");
        let opened = must(GdalDataset::open(&raster.source));
        Kept {
            info: must(opened.raster_info()),
            values: must(opened.read_band(1)),
        }
    }

    fn point(x: f64, y: f64) -> Entity {
        Entity {
            geometry: Geometry::Point(PointGeometry {
                position: Point2::new(x, y),
            }),
            ..Entity::default()
        }
    }

    // ---- RASTER VIEWSHED --------------------------------------------------------------------------

    #[test]
    fn on_flat_ground_every_cell_within_max_distance_is_visible() {
        // Flat ground seen from 1.7 m up: each cell further out lies at a
        // steeper angle below the eye than any nearer one, so nothing hides
        // anything, and every cell within max= is seen. The edge at 10 m is
        // GDAL's (whether a cell 10 m out counts as within); the assertions
        // keep a metre clear of it.
        let name = "OnFlatGroundEveryCellWithinMaxDistanceIsVisible";
        let mut fixture = fixture(name);
        let files = files(name);
        let flat = grid(&files, "flat.asc", 41, 41, |_, _| 10);
        let reply = fixture.ok(&format!(
            "RASTER VIEWSHED FILE \"{flat}\" OBSERVER 20.5,20.5 max=10 curvature=none"
        ));
        let view = kept(&fixture);
        assert!(view.info.width > 0, "{reply}");
        let mut near_cells = 0;
        for row in 0..41 {
            for column in 0..41 {
                let x = f64::from(column) + 0.5;
                let y = f64::from(row) + 0.5;
                let d = (x - 20.5).hypot(y - 20.5);
                if d <= 9.0 {
                    near_cells += 1;
                    assert_eq!(view.seen(x, y), Some(true), "{x},{y}");
                } else if d > 11.0 {
                    assert_ne!(view.seen(x, y), Some(true), "{x},{y}");
                }
            }
        }
        let summary = records(&reply, "viewshed");
        assert_eq!(summary.len(), 1, "{reply}");
        let cells = parse_integer(summary[0].get("visible_cells").unwrap_or(""))
            .expect("visible_cells is a count");
        assert!(cells >= near_cells);
        // 1 m cells: the area is the count.
        assert_eq!(summary[0].get("area").unwrap_or(""), format!("{cells}.000"));
        let last = fixture
            .reference
            .rasters()
            .last()
            .expect("the viewshed's raster");
        assert_eq!(last.name, "flat-viewshed");
        // Nothing to warn about: the display colours name no no-data entry
        // the union grid, which has none, would make GDAL say it ignores.
        assert!(records(&reply, "warning").is_empty(), "{reply}");
        assert_eq!(last.role, RasterRole::Derived);
    }

    #[test]
    fn a_raster_in_another_crs_than_the_drawings_is_refused_not_read_at_the_wrong_place() {
        // flat.asc with a .prj of WGS 84 longitude and latitude, in a project
        // in MGA zone 56: the observer, a drawing point in metres, would be
        // handed to GDAL as degrees, and the result was stamped MGA zone 56
        // whatever the raster was in. Refused, as RASTER SAMPLE, DRAPE and LOS
        // are.
        let name = "ARasterInAnotherCrsThanTheDrawingsIsRefusedNotReadAtTheWrongPlace";
        let mut fixture = fixture(name);
        let files = files(name);
        let flat = grid(&files, "flat.asc", 41, 41, |_, _| 10);
        // The .prj an ASCII grid is read with is Esri's WKT of WGS 84.
        let prj = files.file("flat.prj");
        if let Err(error) = std::fs::write(
            &prj,
            concat!(
                "GEOGCS[\"GCS_WGS_1984\",DATUM[\"D_WGS_1984\",SPHEROID[\"WGS_1984\",6378137.0,",
                "298.257223563]],PRIMEM[\"Greenwich\",0.0],UNIT[\"Degree\",0.0174532925199433]]"
            ),
        ) {
            panic!("{prj}: {error}");
        }
        let wgs84 = must(crs_to_wkt("EPSG:4326"));
        must(
            fixture
                .document
                .set_coordinate_system("EPSG:28356", Document::SET_CRS),
        );
        for line in [
            format!("RASTER VIEWSHED FILE \"{flat}\" OBSERVER 20.5,20.5 max=10 curvature=none"),
            format!("RASTER SAMPLE FILE \"{flat}\" AT 12.5,7.5"),
            format!("LOS FILE \"{flat}\" OBSERVER 1.5,1.5 TARGET 30.5,30.5"),
        ] {
            let error = refused(fixture.run(&line), &line);
            assert_eq!(
                error.code,
                ErrorCode::InvalidCrs,
                "{line}: {}",
                error.describe()
            );
            holds(&error.message, "RASTER REPROJECT");
        }
        assert!(fixture.reference.rasters().is_empty());
        // In the project's own system it runs, and the result keeps it.
        must(
            fixture
                .document
                .set_coordinate_system("EPSG:4326", Document::SET_CRS),
        );
        fixture.ok(&format!(
            "RASTER VIEWSHED FILE \"{flat}\" OBSERVER 20.5,20.5 max=10 curvature=none"
        ));
        let last = fixture
            .reference
            .rasters()
            .last()
            .expect("the viewshed's raster");
        assert_eq!(same_crs(&last.projection_wkt, &wgs84), Some(true));
    }

    #[test]
    fn behind_a_ridge_the_shadow_ends_where_similar_triangles_say() {
        let name = "BehindARidgeTheShadowEndsWhereSimilarTrianglesSay";
        let mut fixture = fixture(name);
        let files = files(name);
        let reply = fixture.ok(&format!(
            "RASTER VIEWSHED FILE \"{}\" OBSERVER 0.5,15.5 height=1.7 target=0 curvature=none",
            wall(&files)
        ));
        let view = kept(&fixture);
        assert!(view.info.width > 0, "{reply}");
        // 20 x 1.7 / 0.7 = 48.571 m: hidden 21 to 48 m out, seen from 49 m.
        for d in 0..60 {
            let x = 0.5 + f64::from(d);
            let hidden = (21..=48).contains(&d);
            assert_eq!(view.seen(x, 15.5), Some(!hidden), "x = {x}");
        }
    }

    #[test]
    fn the_unseen_cells_are_drawn_clear_and_the_seen_tinted() {
        // The overlay drawn over the plan: the cell 30 m out along the
        // observer's row is hidden behind the wall (21 to 48 m out, as above),
        // so it must let the ground show through - alpha 0 - and the cell
        // 10 m out, seen, carries the tint (255, 140, 0) at alpha 150.
        let name = "TheUnseenCellsAreDrawnClearAndTheSeenTinted";
        let mut fixture = fixture(name);
        let files = files(name);
        fixture.ok(&format!(
            "RASTER VIEWSHED FILE \"{}\" OBSERVER 0.5,15.5 curvature=none",
            wall(&files)
        ));
        let overlay = fixture
            .reference
            .rasters()
            .last()
            .expect("the viewshed's raster");
        assert_eq!(
            overlay.rgba.len(),
            overlay.width as usize * overlay.height as usize * 4
        );
        let pixel = |x: f64, y: f64| {
            let gt = &overlay.geotransform;
            let column = ((x - gt[0]) / gt[1]).floor() as usize;
            let row = ((y - gt[3]) / gt[5]).floor() as usize;
            (row * overlay.width as usize + column) * 4
        };
        let hidden = pixel(30.5, 15.5);
        assert_eq!(overlay.rgba[hidden + 3], 0);
        let seen = pixel(10.5, 15.5);
        assert_eq!(overlay.rgba[seen], 255);
        assert_eq!(overlay.rgba[seen + 1], 140);
        assert_eq!(overlay.rgba[seen + 2], 0);
        assert_eq!(overlay.rgba[seen + 3], 150);
    }

    #[test]
    fn two_observers_give_the_union_of_their_viewsheds() {
        // From the east end, (59.5, 15.5), the wall is 39 m off: its grazing
        // sight meets the ground 39 x 1.7 / 0.7 = 94.7 m out, past the raster,
        // so everything west of the wall (centres 0.5 to 19.5) is hidden from
        // there. From the west end the cells 21.5 to 48.5 are hidden. Each
        // sees what the other cannot: together, the whole row.
        let name = "TwoObserversGiveTheUnionOfTheirViewsheds";
        let mut fixture = fixture(name);
        let files = files(name);
        let raster = wall(&files);
        fixture.ok(&format!(
            "RASTER VIEWSHED FILE \"{raster}\" OBSERVER 59.5,15.5 curvature=none NAME east"
        ));
        let east = kept(&fixture);
        assert_eq!(east.seen(10.5, 15.5), Some(false));
        assert_eq!(east.seen(30.5, 15.5), Some(true));
        let reply = fixture.ok(&format!(
            "RASTER VIEWSHED FILE \"{raster}\" OBSERVER 0.5,15.5 OBSERVER 59.5,15.5 \
             curvature=none"
        ));
        assert_eq!(records(&reply, "observer").len(), 2, "{reply}");
        let both = kept(&fixture);
        for d in 0..60 {
            let x = 0.5 + f64::from(d);
            assert_eq!(both.seen(x, 15.5), Some(true), "x = {x}");
        }
    }

    #[test]
    fn observers_from_the_scope_are_its_points_and_the_rest_is_counted() {
        let name = "ObserversFromTheScopeAreItsPointsAndTheRestIsCounted";
        let mut fixture = fixture(name);
        let files = files(name);
        let line = Entity {
            geometry: Geometry::Line(Segment2::new(Point2::new(1.0, 1.0), Point2::new(2.0, 2.0))),
            ..Entity::default()
        };
        must(fixture.document.execute(create_entities(vec![
            point(0.5, 15.5),
            point(59.5, 15.5),
            line,
        ])));
        let reply = fixture.ok(&format!(
            "RASTER VIEWSHED FILE \"{}\" OBSERVERS DRAWING curvature=none",
            wall(&files)
        ));
        let scope = records(&reply, "scope");
        assert_eq!(scope.len(), 1, "{reply}");
        assert_eq!(scope[0].get("points").unwrap_or(""), "2");
        assert_eq!(scope[0].get("skipped.line").unwrap_or(""), "1");
        let observers = records(&reply, "observer");
        assert_eq!(observers.len(), 2);
        assert!(observers[0].get("entity").is_some());
        assert_eq!(kept(&fixture).seen(30.5, 15.5), Some(true));
    }

    #[test]
    fn areas_draw_the_visible_area_in_one_step_and_the_reply_measures_it() {
        let name = "AreasDrawTheVisibleAreaInOneStepAndTheReplyMeasuresIt";
        let mut fixture = fixture(name);
        let files = files(name);
        let flat = grid(&files, "flat.asc", 41, 41, |_, _| 10);
        let before = fixture.document.history().undo_count();
        let reply = fixture.ok(&format!(
            "RASTER VIEWSHED FILE \"{flat}\" OBSERVER 20.5,20.5 max=10 curvature=none \
             areas=site/view"
        ));
        assert_eq!(
            fixture.document.history().undo_count(),
            before + 1,
            "{reply}"
        );
        assert!(
            fixture
                .document
                .model()
                .entities
                .count_on_layer("site/view")
                >= 1
        );
        let drawn = records(&reply, "output")
            .iter()
            .any(|output| output.get("arg") == Some("areas"));
        assert!(drawn, "{reply}");
    }

    #[test]
    fn what_the_verb_cannot_do_is_refused_naming_it() {
        let name = "WhatTheVerbCannotDoIsRefusedNamingIt";
        let mut fixture = fixture(name);
        let files = files(name);
        let raster = format!("RASTER VIEWSHED FILE \"{}\"", wall(&files));
        for words in [
            "",
            " OBSERVER 5",
            " OBSERVER 1,1 OBSERVERS",
            " OBSERVER 1,1 DRAWING",
            " OBSERVER 1,1 height=-1",
            " OBSERVER 1,1 curvature=lots",
            " OBSERVER 1,1 max=0",
            " OBSERVER 1,1 areas=/bad",
        ] {
            let error = refused(fixture.run(&format!("{raster}{words}")), words);
            assert_eq!(error.code, ErrorCode::InvalidArgument, "{words}");
        }
        // Off the 60 x 31 m raster: refused by the work, which knows its
        // extent.
        let off = refused(
            fixture.run(&format!("{raster} OBSERVER 100,100")),
            "an observer off the raster",
        );
        assert_eq!(off.code, ErrorCode::InvalidArgument);
        assert!(fixture.reference.rasters().is_empty());
    }

    #[test]
    fn preview_and_a_cancelled_run_make_nothing() {
        let name = "PreviewAndACancelledRunMakeNothing";
        let mut fixture = fixture(name);
        let files = files(name);
        let raster = wall(&files);
        let reply = fixture.ok(&format!(
            "RASTER VIEWSHED FILE \"{raster}\" OBSERVER 0.5,15.5 PREVIEW"
        ));
        holds(&reply, "preview valid=yes changed=no");
        let prepared = must(fixture.prepare(&format!(
            "RASTER VIEWSHED FILE \"{raster}\" OBSERVER 0.5,15.5"
        )));
        let stop = AtomicBool::new(true);
        let work = prepared.work.expect("a run has work");
        let error = refused(work(Some(&stop), None), "a stopped run");
        assert_eq!(error.message, "cancelled");
        assert!(fixture.reference.rasters().is_empty());
        assert!(is_empty_dir(fixture.scratch.path()));
    }

    // ---- LOS, the verb ----------------------------------------------------------------------------

    #[test]
    fn los_over_a_raster_wall_is_blocked_at_the_wall() {
        // Bilinear between centres, the wall rises from 0 at x = 19.5 to 1 at
        // 20.5. From the eye 1.7 m over (0.5, 15.5) to the ground at (30.5,
        // 15.5) the sight is 1.7 x (1 - (x - 0.5) / 30) high: at x = 20 it is
        // 0.595 over ground 0.5 (clear), at 20.5 it is 0.567 under ground 1
        // (hidden). The stations are half a cell apart from the observer, so
        // 20.5 is the first one hidden.
        let name = "LosOverARasterWallIsBlockedAtTheWall";
        let mut fixture = fixture(name);
        let files = files(name);
        let reply = fixture.ok(&format!(
            "LOS FILE \"{}\" OBSERVER 0.5,15.5 TARGET 30.5,15.5 curvature=none",
            wall(&files)
        ));
        let sight = records(&reply, "sight");
        assert_eq!(sight.len(), 1, "{reply}");
        assert_eq!(sight[0].get("visible").unwrap_or(""), "no");
        let blocked = sight[0].get("blocked_at").unwrap_or("");
        let comma = blocked.find(',').unwrap_or_else(|| panic!("{reply}"));
        near(
            parse_finite_double(&blocked[..comma]).unwrap_or(0.0),
            20.5,
            1e-9,
        );
        near(
            parse_finite_double(&blocked[comma + 1..]).unwrap_or(0.0),
            15.5,
            1e-9,
        );
        // A computed station is given to the millimetre, as the distances
        // are, not as the shortest text of a double: a station on terrain.asc
        // was printed as 149.43965517241378,119.50431034482759.
        assert_eq!(blocked, "20.500,15.500");
        assert_eq!(sight[0].get("step").unwrap_or(""), "0.5");
        assert_eq!(sight[0].get("distance").unwrap_or(""), "30.000");
        assert_eq!(fixture.document.history().undo_count(), 0);
    }

    #[test]
    fn los_needs_both_ends_and_reads_no_scope() {
        let name = "LosNeedsBothEndsAndReadsNoScope";
        let mut fixture = fixture(name);
        let files = files(name);
        let los = format!("LOS FILE \"{}\"", wall(&files));
        for words in [
            " OBSERVER 1,1",
            " TARGET 1,1",
            " OBSERVER 1,1 TARGET 2,2 DRAWING",
            " OBSERVER 1,1 OBSERVER 2,2 TARGET 3,3",
            " OBSERVER 1,1 TARGET 2,2 step=0",
        ] {
            let error = refused(fixture.run(&format!("{los}{words}")), words);
            assert_eq!(error.code, ErrorCode::InvalidArgument, "{words}");
        }
    }
}
