//! tests/geo/test_slope_verbs.cpp: the RASTER SLOPE and RASTER ASPECT verbs
//! (src/katana_app/geo/slope_verbs.rs, docs/terrain.md "Slope and aspect"):
//! the slope or aspect kept as a derived reference raster of its values, and
//! slope classes drawn as areas on <areas>/<class> in one undo step, with each
//! class's area.
//!
//! Fixtures: tests/geo/data/plane.asc (40 x 30 cells of 1 m from (0,0), z =
//! 100 + 0.05 x at the cell centres: a slope of 5 %, falling to the west) and
//! grids written here. Slope is Horn's: dz/dx = ((c + 2f + i) - (a + 2d + g))
//! / 8 cells over the 3 x 3 window, which on a surface that varies in x alone
//! is the difference of the two neighbours over two cells. Its SlopeContract
//! suite is step 5's.

mod slope_verbs {
    use std::sync::atomic::AtomicBool;

    use katana_app::geo::replies::Record;
    use katana_commands::{create_entities, create_layer};
    use katana_core::ErrorCode;
    use katana_core::text::format_general;
    use katana_entity::{Entity, Geometry, Layer, PropertyValue};
    use katana_geometry::{Point2, Polyline2};
    use katana_gis::GdalDataset;
    use katana_interop::{RasterDisplayStyle, RasterRole};

    use crate::geo_fixture::{
        GeoFixture, TempDir, geo_data, holds, must, near, refused, shared_data,
    };
    use crate::raster_support::{ascii_grid, is_empty_dir, records};

    // plane.asc is read as Float32: each height within 3.815e-6 of its value
    // near 100. Horn's dz/dx is a weighted difference whose weights sum to 8
    // over 8 cells, so it is within 3.815e-6 of 0.05: 3.8e-4 % of slope, and
    // 3.815e-6 rad x 57.3 = 2.2e-4 degrees. The Float32 result adds half its
    // step (2.4e-7 near 5, 1.2e-7 near 2.9).
    const PERCENT_TOLERANCE: f64 = 5e-4;
    const DEGREE_TOLERANCE: f64 = 2.5e-4;

    /// The C++ fixture's: its scratch folder named after the test.
    fn fixture(name: &str) -> GeoFixture {
        GeoFixture::new("katana-slope-verbs-", name)
    }

    /// The C++ fixture's second folder, for the files a case writes.
    fn files(name: &str) -> TempDir {
        TempDir::new("katana-slope-verbs-", &format!("{name}-files"))
    }

    /// tests/geo/data/plane.asc.
    fn plane() -> String {
        format!("{}/plane.asc", geo_data())
    }

    /// An ESRI ASCII grid of 1 m cells from (0,0), height(column, row) at
    /// each cell centre, row 0 the northern (top) row, written at the C++
    /// stream's precision(17).
    fn grid(
        folder: &TempDir,
        name: &str,
        columns: i32,
        rows: i32,
        height: impl Fn(i32, i32) -> f64,
    ) -> String {
        ascii_grid(folder, name, columns, rows, |column, row| {
            format_general(height(column, row), 17)
        })
    }

    /// The class record of `name`.
    fn class_record(reply: &str, name: &str) -> Option<Record> {
        records(reply, "class")
            .into_iter()
            .find(|each| each.get("name") == Some(name))
    }

    /// The width and the values of the reference raster the line made, from
    /// its file.
    fn values(fixture: &GeoFixture) -> (i32, Vec<f64>) {
        let raster = fixture
            .reference
            .rasters()
            .last()
            .expect("a reference raster");
        let opened = must(GdalDataset::open(&raster.source));
        let info = must(opened.raster_info());
        (info.width, must(opened.read_band(1)))
    }

    fn count_on(fixture: &GeoFixture, layer: &str) -> usize {
        fixture.document.model().entities.count_on_layer(layer)
    }

    /// A polyline through `corners` (x, y).
    fn polyline(corners: &[(f64, f64)], closed: bool, layer: &str) -> Entity {
        Entity {
            geometry: Geometry::Polyline(Polyline2 {
                vertices: corners.iter().map(|&(x, y)| Point2::new(x, y)).collect(),
                closed,
            }),
            layer: String::from(layer),
            ..Entity::default()
        }
    }

    // ---- slope and aspect -----------------------------------------------------------------------

    #[test]
    fn a_plane_rising_one_in_twenty_is_five_percent_and_two_point_eight_six_degrees() {
        // Inside the edge: GDAL computes the edge cells too, by its own edge
        // rule (a corner reads half the gradient: 2.5 %), which is GDAL's and
        // not the plane's.
        let _shared = shared_data();
        let mut fixture = fixture("APlaneRisingOneInTwentyIsFivePercentAndTwoPointEightSixDegrees");
        let reply = fixture.ok(&format!("RASTER SLOPE FILE \"{}\"", plane()));
        let slope = records(&reply, "slope");
        assert_eq!(slope.len(), 1, "{reply}");
        assert_eq!(slope[0].get("unit").unwrap_or(""), "percent");
        let percent = fixture
            .reference
            .rasters()
            .last()
            .expect("the slope's raster")
            .clone();
        assert_eq!(percent.role, RasterRole::Derived);
        assert_eq!(percent.display_style, RasterDisplayStyle::Slope);
        assert_eq!(percent.name, "plane-slope");
        let (width, slope) = values(&fixture);
        assert_eq!(slope.len(), 1200);
        for row in 1..29 {
            for column in 1..39 {
                near(
                    slope[(row * width + column) as usize],
                    5.0,
                    PERCENT_TOLERANCE,
                );
            }
        }
        // Drawn coloured, not as a grey stretch of its values.
        let middle = (15 * percent.width as usize + 20) * 4;
        assert!(
            percent.rgba[middle] != percent.rgba[middle + 1]
                || percent.rgba[middle + 1] != percent.rgba[middle + 2]
        );

        fixture.ok(&format!("RASTER SLOPE FILE \"{}\" unit=degree", plane()));
        let (width, slope) = values(&fixture);
        // 2.8624052
        let degrees = 0.05_f64.atan() * 180.0 / std::f64::consts::PI;
        for row in 1..29 {
            for column in 1..39 {
                near(
                    slope[(row * width + column) as usize],
                    degrees,
                    DEGREE_TOLERANCE,
                );
            }
        }
    }

    #[test]
    fn its_aspect_is270() {
        // It rises to the east, so it faces west: 270 degrees clockwise from
        // north. The direction's error is the cross-slope's over the slope:
        // at most 3.815e-6 / 0.05 rad = 4.4e-3 degrees.
        let _shared = shared_data();
        let mut fixture = fixture("ItsAspectIs270");
        let reply = fixture.ok(&format!("RASTER ASPECT FILE \"{}\"", plane()));
        assert_eq!(records(&reply, "aspect").len(), 1, "{reply}");
        assert_eq!(
            fixture
                .reference
                .rasters()
                .last()
                .expect("the aspect's raster")
                .name,
            "plane-aspect"
        );
        let (width, aspect) = values(&fixture);
        for row in 1..29 {
            for column in 1..39 {
                near(aspect[(row * width + column) as usize], 270.0, 5e-3);
            }
        }
    }

    #[test]
    fn a_surface_is_sampled_at_its_cell_and_sloped() {
        // The surface spans the cell centres, 39 x 29 m: 39 x 29 cells of 1 m.
        let _shared = shared_data();
        let mut fixture = fixture("ASurfaceIsSampledAtItsCellAndSloped");
        fixture.ok(&format!("SURFACE FROM FILE \"{}\" NAME plane", plane()));
        let reply = fixture.ok("RASTER SLOPE SURFACE plane CELL 1");
        let slope = records(&reply, "slope");
        assert_eq!(slope.len(), 1, "{reply}");
        assert_eq!(slope[0].get("raster").unwrap_or(""), "39x29", "{reply}");
        assert_eq!(
            fixture
                .reference
                .rasters()
                .last()
                .expect("the slope's raster")
                .name,
            "plane-slope"
        );
    }

    // ---- classes as areas -----------------------------------------------------------------------

    #[test]
    fn classes_two_and_ten_put_the_whole_plane_in_one_class() {
        // Every one of the 40 x 30 cells has a slope - GDAL's edge rule
        // computes the edges, 2.5 % at the corners - and every one lies in
        // [2, 10): one region of 1200 m2.
        let _shared = shared_data();
        let mut fixture = fixture("ClassesTwoAndTenPutTheWholePlaneInOneClass");
        let reply = fixture.ok(&format!("RASTER SLOPE FILE \"{}\" classes=2,10", plane()));
        let middle = class_record(&reply, "2-10").unwrap_or_else(|| panic!("{reply}"));
        assert_eq!(middle.get("area").unwrap_or(""), "1200.000");
        assert_eq!(middle.get("polygons").unwrap_or(""), "1");
        assert_eq!(
            class_record(&reply, "0-2")
                .expect("the class 0-2")
                .get("area")
                .unwrap_or(""),
            "0.000"
        );
        assert_eq!(
            class_record(&reply, "10+")
                .expect("the class 10+")
                .get("to")
                .unwrap_or("x"),
            ""
        );
        assert_eq!(count_on(&fixture, "terrain/slope/2-10"), 1);
        let ids = fixture
            .document
            .model()
            .entities
            .ids_on_layer("terrain/slope/2-10");
        let area = fixture
            .document
            .model()
            .entities
            .find(ids[0])
            .expect("the class's area");
        let Geometry::Polyline(outline) = &area.geometry else {
            panic!("the class's area is a polyline");
        };
        assert!(outline.closed);
        near(outline.area(), 1200.0, 1e-9);
        assert_eq!(
            area.properties.get("slope_class"),
            Some(&PropertyValue::Integer(2))
        );
        assert_eq!(
            area.properties.get("slope_from"),
            Some(&PropertyValue::Real(2.0))
        );
        assert_eq!(
            area.properties.get("slope_to"),
            Some(&PropertyValue::Real(10.0))
        );
        assert_eq!(
            area.properties.get("slope_unit"),
            Some(&PropertyValue::Text(String::from("percent")))
        );
        assert_eq!(records(&reply, "legend").len(), 3);
    }

    #[test]
    fn a_raster_with_a_step_gives_two_classes_with_hand_computed_areas() {
        // 20 columns rising 2 % (z = 0.02 x), then 20 rising 20 % (z = 0.4 +
        // 0.2 (x - 20)), each 20 x 30 m. Horn reads the two neighbours across
        // the step: at x = 19.5 (0.5 - 0.37) / 2 = 6.5 %, at x = 20.5 (0.7 -
        // 0.39) / 2 = 15.5 %; the 20 % plane's corners read half, 10 %. With a
        // break at 8, the first 20 columns are gentle and the rest steep:
        // 600 m2 each.
        let name = "ARasterWithAStepGivesTwoClassesWithHandComputedAreas";
        let mut fixture = fixture(name);
        let files = files(name);
        let step = grid(&files, "step.asc", 40, 30, |column, _| {
            let x = f64::from(column) + 0.5;
            if x < 20.0 {
                0.02 * x
            } else {
                0.4 + 0.2 * (x - 20.0)
            }
        });
        let reply = fixture.ok(&format!(
            "RASTER SLOPE FILE \"{step}\" classes=8 areas=site/grade"
        ));
        let (Some(gentle), Some(steep)) = (class_record(&reply, "0-8"), class_record(&reply, "8+"))
        else {
            panic!("{reply}");
        };
        assert_eq!(gentle.get("area").unwrap_or(""), "600.000");
        assert_eq!(steep.get("area").unwrap_or(""), "600.000");
        assert_eq!(count_on(&fixture, "site/grade/0-8"), 1);
        assert_eq!(count_on(&fixture, "site/grade/8+"), 1);
    }

    #[test]
    fn min_area_sieves_specks() {
        // One cell raised 1 m in flat ground: Horn's window gives its four
        // side neighbours 2 x 1 / 8 = 25 % and its four diagonal ones
        // sqrt(2) / 8 = 17.7 %, and the raised cell itself 0 (its own height
        // is not in its window). So a ring of 8 steep cells - 8 m2, a region
        // with a hole - and the 1 m2 top inside it, gentle. min_area=10 sieves
        // both specks into the ground around them: 900 m2 gentle, nothing
        // steep.
        let name = "MinAreaSievesSpecks";
        let mut fixture = fixture(name);
        let files = files(name);
        let bump = grid(&files, "bump.asc", 30, 30, |column, row| {
            if column == 15 && row == 15 {
                101.0
            } else {
                100.0
            }
        });
        let specks = fixture.ok(&format!(
            "RASTER SLOPE FILE \"{bump}\" classes=10 areas=specks"
        ));
        let steep = class_record(&specks, "10+").unwrap_or_else(|| panic!("{specks}"));
        assert_eq!(steep.get("area").unwrap_or(""), "8.000", "{specks}");
        assert_eq!(steep.get("polygons").unwrap_or(""), "1");
        let gentle = class_record(&specks, "0-10").unwrap_or_else(|| panic!("{specks}"));
        assert_eq!(gentle.get("area").unwrap_or(""), "892.000");
        assert_eq!(gentle.get("polygons").unwrap_or(""), "2");

        let sieved = fixture.ok(&format!(
            "RASTER SLOPE FILE \"{bump}\" classes=10 min_area=10 areas=sieved"
        ));
        let sieves = records(&sieved, "sieved");
        assert_eq!(sieves.len(), 1, "{sieved}");
        assert_eq!(sieves[0].get("cells").unwrap_or(""), "10");
        assert_eq!(
            class_record(&sieved, "10+")
                .unwrap_or_else(|| panic!("{sieved}"))
                .get("area")
                .unwrap_or(""),
            "0.000"
        );
        assert_eq!(
            class_record(&sieved, "0-10")
                .unwrap_or_else(|| panic!("{sieved}"))
                .get("area")
                .unwrap_or(""),
            "900.000"
        );
        assert_eq!(count_on(&fixture, "sieved/10+"), 0);
        assert_eq!(count_on(&fixture, "sieved/0-10"), 1);
    }

    #[test]
    fn one_undo_removes_every_class_polygon() {
        let name = "OneUndoRemovesEveryClassPolygon";
        let mut fixture = fixture(name);
        let files = files(name);
        let bump = grid(&files, "bump.asc", 30, 30, |column, row| {
            if column == 15 && row == 15 {
                101.0
            } else {
                100.0
            }
        });
        fixture.ok(&format!("RASTER SLOPE FILE \"{bump}\" classes=10"));
        // The ring and its hole, the ground and its hole, the top: 5 rings.
        assert_eq!(fixture.entity_count(), 5);
        must(fixture.document.undo());
        assert_eq!(fixture.entity_count(), 0);
    }

    #[test]
    fn a_scope_keeps_the_analysis_inside_its_closed_shapes() {
        // A triangle with its right angle at (5,5) and its legs 10.25 long:
        // the plane lies in [2, 10) all over it, so that class's area is the
        // triangle's, 10.25 x 10.25 / 2 = 52.53125 m2 - not the 66 cells
        // GDAL's raster clip keeps (every cell it touches: i + j <= 10 of the
        // 11 x 11 cells from (5,5)), nor the 121 of its box. The open line
        // beside it bounds nothing and is counted.
        let _shared = shared_data();
        let mut fixture = fixture("AScopeKeepsTheAnalysisInsideItsClosedShapes");
        let square = polyline(&[(5.0, 5.0), (15.25, 5.0), (5.0, 15.25)], true, "site");
        let open = polyline(&[(20.0, 5.0), (30.0, 5.0)], false, "site");
        let site = Layer {
            name: String::from("site"),
            ..Layer::default()
        };
        must(fixture.document.execute(create_layer(site)));
        must(
            fixture
                .document
                .execute(create_entities(vec![square, open])),
        );
        let reply = fixture.ok(&format!(
            "RASTER SLOPE FILE \"{}\" classes=2,10 areas=inside LAYERS site",
            plane()
        ));
        let areas = records(&reply, "areas");
        assert_eq!(areas.len(), 1, "{reply}");
        assert_eq!(areas[0].get("used").unwrap_or(""), "1");
        assert_eq!(areas[0].get("skipped.open").unwrap_or(""), "1");
        // The record rounds to the thousandth: 52.531.
        assert_eq!(
            class_record(&reply, "2-10")
                .unwrap_or_else(|| panic!("{reply}"))
                .get("area")
                .unwrap_or(""),
            "52.531"
        );
        assert_eq!(count_on(&fixture, "inside/2-10"), 1);
    }

    #[test]
    fn its_options_and_name_may_follow_the_filter() {
        // The words are read as every GIS verb's are: classes= and areas=
        // after the filter are the verb's own (they were refused as "not a
        // WHERE key"). The triangle is the one above: 52.53125 m2 in class
        // 2-10.
        let _shared = shared_data();
        let mut fixture = fixture("ItsOptionsAndNameMayFollowTheFilter");
        let triangle = polyline(
            &[(5.0, 5.0), (15.25, 5.0), (5.0, 15.25)],
            true,
            &Entity::default().layer,
        );
        must(fixture.document.execute(create_entities(vec![triangle])));
        let reply = fixture.ok(&format!(
            "RASTER SLOPE FILE \"{}\" DRAWING WHERE TYPE=polyline classes=2,10 areas=inside \
             NAME s",
            plane()
        ));
        assert_eq!(
            class_record(&reply, "2-10")
                .unwrap_or_else(|| panic!("{reply}"))
                .get("area")
                .unwrap_or(""),
            "52.531",
            "{reply}"
        );
        assert_eq!(count_on(&fixture, "inside/2-10"), 1);
        let error = refused(
            fixture.run(&format!("RASTER SLOPE FILE \"{}\" NAME a NAME b", plane())),
            "NAME given twice",
        );
        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }

    #[test]
    fn a_scope_with_no_closed_shape_makes_nothing_and_says_so() {
        let _shared = shared_data();
        let mut fixture = fixture("AScopeWithNoClosedShapeMakesNothingAndSaysSo");
        let open = polyline(&[(0.0, 0.0), (10.0, 10.0)], false, &Entity::default().layer);
        must(fixture.document.execute(create_entities(vec![open])));
        let reply = fixture.ok(&format!(
            "RASTER SLOPE FILE \"{}\" classes=5 DRAWING",
            plane()
        ));
        holds(&reply, "made=no");
        assert!(fixture.reference.rasters().is_empty());
        assert_eq!(fixture.entity_count(), 1);
    }

    #[test]
    fn preview_and_a_cancelled_run_make_nothing() {
        let _shared = shared_data();
        let mut fixture = fixture("PreviewAndACancelledRunMakeNothing");
        let reply = fixture.ok(&format!(
            "RASTER SLOPE FILE \"{}\" classes=5 PREVIEW",
            plane()
        ));
        holds(&reply, "preview valid=yes changed=no");
        let prepared =
            must(fixture.prepare(&format!("RASTER SLOPE FILE \"{}\" classes=5", plane())));
        let stop = AtomicBool::new(true);
        let work = prepared.work.expect("a run has work");
        let error = refused(work(Some(&stop), None), "a stopped run");
        assert_eq!(error.message, "cancelled");
        assert!(fixture.reference.rasters().is_empty());
        assert_eq!(fixture.entity_count(), 0);
        assert!(is_empty_dir(fixture.scratch.path()));
    }

    #[test]
    fn what_the_verbs_cannot_do_is_refused_naming_it() {
        let _shared = shared_data();
        let mut fixture = fixture("WhatTheVerbsCannotDoIsRefusedNamingIt");
        let slope = format!("RASTER SLOPE FILE \"{}\"", plane());
        for words in [
            " unit=radian",
            " classes=10,5",
            " classes=0",
            " classes=5,5",
            " classes=95 unit=degree",
            " min_area=10",
            " areas=grades",
            " classes=5 min_area=0",
            " classes=5 areas=/bad",
            " colour=red",
        ] {
            let error = refused(fixture.run(&format!("{slope}{words}")), words);
            assert_eq!(error.code, ErrorCode::InvalidArgument, "{words}");
        }
        assert_eq!(
            refused(
                fixture.run(&format!("RASTER ASPECT FILE \"{}\" unit=degree", plane())),
                "a unit for an aspect"
            )
            .code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            refused(fixture.run("RASTER SLOPE DRAWING"), "a scope as the source").code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            refused(
                fixture.run("RASTER SLOPE SURFACE nothing"),
                "no such surface"
            )
            .code,
            ErrorCode::NotFound
        );
        assert!(fixture.reference.rasters().is_empty());
    }
}

// ---- through a Session, as katana_cli and katana_mcp run it -------------------------------------

mod slope_session {
    use katana_app::{Captured, Session};

    use crate::geo_fixture::{geo_data, holds, shared_data};

    #[test]
    fn the_session_draws_the_classes_and_reports_their_areas() {
        let _shared = shared_data();
        let mut session = Session::new(None, &mut Captured::default());
        let mut captured = Captured::default();
        let made = session.run(
            format!(
                "RASTER SLOPE FILE \"{}/plane.asc\" classes=2,10",
                geo_data()
            ),
            &mut captured,
        );
        assert!(made);
        holds(
            &captured.out,
            "class name=2-10 from=2 to=10 unit=percent area=1200.000 polygons=1",
        );
    }
}
