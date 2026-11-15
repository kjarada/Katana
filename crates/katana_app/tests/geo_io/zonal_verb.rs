//! tests/geo/test_zonal_verb.cpp: the RASTER ZONAL verb (docs/terrain.md,
//! "Statistics by area") - a raster's statistics inside each closed shape a
//! scope takes, written on the shapes as <prefix>_<stat> properties in one
//! undo step. Its ZonalContract suite is the bridge's (katana_gis).
//!
//! Fixtures:
//!
//! ```text
//! samples/gis/terrain.asc   120 x 90 cells of 1.5 m from (-5,-5), no no-data
//!                           cell (checked by reading the text grid);
//! tests/geo/data/plane.asc  40 x 30 cells of 1 m from (0,0), z = 100 +
//!                           0.05 x at the cell centres.
//! ```
//!
//! With pixels=fractional (exactextract's method) a cell counts by the part of
//! it a zone covers, so a count is an area in cells and a mean is weighted by
//! those parts.

mod zonal_verb {
    use std::path::Path;
    use std::sync::atomic::AtomicBool;

    use katana_app::geo::prepare;
    use katana_app::geo::replies::{Record, parse_records};
    use katana_commands::{create_layer, move_entities, set_entity_property};
    use katana_core::ErrorCode;
    use katana_entity::{Entity, EntityId, Geometry, Layer, PropertyMap, PropertyValue};
    use katana_geometry::{Point2, Polyline2, Vec2};

    use crate::geo_fixture::{
        GeoFixture, geo_data, gis_samples, holds, must, near, refused, shared_data,
    };

    fn plane() -> String {
        format!("\"{}/plane.asc\"", geo_data())
    }

    fn terrain() -> String {
        format!("\"{}/terrain.asc\"", gis_samples())
    }

    /// The C++ fixture: its scratch folder named after the test.
    fn fixture(name: &str) -> GeoFixture {
        GeoFixture::new("katana-zonal-verb-", name)
    }

    #[track_caller]
    fn ran(fixture: &mut GeoFixture, line: &str) -> String {
        match fixture.run(line) {
            Ok(reply) => reply,
            Err(error) => panic!("{line}: {}", error.describe()),
        }
    }

    fn records(reply: &str, kind: &str) -> Vec<Record> {
        parse_records(reply)
            .into_iter()
            .filter(|record| record.kind == kind)
            .collect()
    }

    /// A closed rectangle on `layer`, drawn as a person draws it: its id. The
    /// fixture's rect makes the layer first when the drawing lacks it, as the
    /// C++ fixture's draw does.
    fn draw_box(
        fixture: &mut GeoFixture,
        (x0, y0): (f64, f64),
        (x1, y1): (f64, f64),
        layer: &str,
    ) -> EntityId {
        fixture.rect((x0, y0), (x1, y1), layer, PropertyMap::new())
    }

    fn number(fixture: &GeoFixture, id: EntityId, key: &str) -> Option<f64> {
        let entity = fixture.document.model().entities.find(id)?;
        match entity.properties.get(key)? {
            PropertyValue::Real(real) => Some(*real),
            PropertyValue::Integer(whole) => Some(*whole as f64),
            _ => None,
        }
    }

    // ---- the statistics ----

    #[test]
    fn zonal_count_of_a_forty_by_thirty_box_on_a1_point5_metre_grid_is533_point333() {
        let _shared = shared_data();
        let mut fixture = fixture("ZonalCountOfAFortyByThirtyBoxOnA1Point5MetreGridIs533Point333");
        // The box covers 40 x 30 = 1200 m2; each cell is 1.5 x 1.5 = 2.25 m2,
        // so fractional coverage counts 1200 / 2.25 = 533.333... cells. Its
        // west and south sides lie on cell edges (10 = -5 + 10 x 1.5), its
        // east side cuts a column of cells (50 = -5 + 36.67 x 1.5): the count
        // is the same either way, which is the point of fractional coverage.
        // Tolerance: the coverage fractions may be single precision
        // (exactextract keeps them as float), 6e-8 relative each over 533
        // cells: 3.2e-5; 1e-4 allows that.
        let lot = draw_box(&mut fixture, (10.0, 10.0), (50.0, 40.0), "0");
        let reply = ran(
            &mut fixture,
            &format!("RASTER ZONAL FILE {} DRAWING stats=count", terrain()),
        );
        let count = number(&fixture, lot, "zone_count");
        assert!(count.is_some(), "{reply}");
        near(count.unwrap_or_default(), 1200.0 / 2.25, 1e-4);
        let zones = records(&reply, "zone");
        assert_eq!(zones.len(), 1, "{reply}");
        assert_eq!(zones[0].get("entity").unwrap_or(""), format!("{lot}"));
    }

    #[test]
    fn zonal_mean_on_a_plane_is_the_plane_at_the_centroid() {
        let _shared = shared_data();
        let mut fixture = fixture("ZonalMeanOnAPlaneIsThePlaneAtTheCentroid");
        // plane.asc is z = 100 + 0.05 x at the cell centres (x + 0.5 for
        // column x). The box from x = 10.25 to 30.25 covers column 10 by
        // 0.75, columns 11 to 29 whole and column 30 by 0.25; y from 5 to 25
        // is 20 whole rows. The weighted mean of the centres' x:
        //   (0.75 x 10.5 + (11.5 + ... + 29.5) + 0.25 x 30.5) / (0.75 + 19 + 0.25)
        //   = (7.875 + 399.5 + 7.625) / 20 = 20.25,
        // the box's centroid, so the mean height is 100 + 0.05 x 20.25 =
        // 101.0125. The least centre inside is 10.5 (100.525), the greatest
        // 30.5 (101.525); the count is 20 columns x 20 rows = 400.
        // Tolerance: plane.asc is read as Float32, each height within
        // 3.815e-6 of its value near 100, and a weighted mean of them is too:
        // 1e-5.
        let lot = draw_box(&mut fixture, (10.25, 5.0), (30.25, 25.0), "0");
        let reply = ran(
            &mut fixture,
            &format!("RASTER ZONAL FILE {} DRAWING", plane()),
        );
        assert!(number(&fixture, lot, "zone_mean").is_some(), "{reply}");
        let of = |key: &str| number(&fixture, lot, key).unwrap_or(f64::NAN);
        near(of("zone_mean"), 101.0125, 1e-5);
        near(of("zone_min"), 100.525, 1e-5);
        near(of("zone_max"), 101.525, 1e-5);
        near(of("zone_count"), 400.0, 1e-4);
        // The sum is the mean times the count: 101.0125 x 400 = 40405.
        near(of("zone_sum"), 40405.0, 400.0 * 1e-5);
        holds(
            &reply,
            "zonal stats=mean,min,max,count,sum prefix=zone pixels=fractional zones=1",
        );
    }

    #[test]
    fn an_open_line_in_the_scope_is_skipped_and_counted() {
        let _shared = shared_data();
        let mut fixture = fixture("AnOpenLineInTheScopeIsSkippedAndCounted");
        let lot = draw_box(&mut fixture, (10.0, 5.0), (30.0, 25.0), "site");
        let open = Polyline2 {
            vertices: vec![Point2::new(2.0, 2.0), Point2::new(8.0, 8.0)],
            closed: false,
        };
        let path = fixture.add(Geometry::Polyline(open), "site", PropertyMap::new());
        let reply = ran(
            &mut fixture,
            &format!("RASTER ZONAL FILE {} LAYERS site stats=mean", plane()),
        );
        let zones = records(&reply, "zones");
        assert_eq!(zones.len(), 1, "{reply}");
        assert_eq!(zones[0].get("used").unwrap_or(""), "1");
        assert_eq!(zones[0].get("skipped.open").unwrap_or(""), "1");
        assert!(!records(&reply, "warning").is_empty(), "{reply}");
        // Columns 10 to 29 whole: centres 10.5 to 29.5, mean 20, height 101.
        near(
            number(&fixture, lot, "zone_mean").unwrap_or(0.0),
            101.0,
            1e-5,
        );
        assert!(number(&fixture, path, "zone_mean").is_none());
    }

    #[test]
    fn properties_are_one_undo_step() {
        let _shared = shared_data();
        let mut fixture = fixture("PropertiesAreOneUndoStep");
        let west = draw_box(&mut fixture, (2.0, 2.0), (12.0, 12.0), "0");
        let east = draw_box(&mut fixture, (20.0, 2.0), (30.0, 12.0), "0");
        let before = fixture.document.history().undo_count();
        let reply = ran(
            &mut fixture,
            &format!(
                "RASTER ZONAL FILE {} DRAWING stats=mean prefix=ground",
                plane()
            ),
        );
        assert_eq!(
            fixture.document.history().undo_count(),
            before + 1,
            "{reply}"
        );
        // Centres 2.5 .. 11.5: mean 7, height 100.35; 20.5 .. 29.5: 25,
        // 101.25.
        near(
            number(&fixture, west, "ground_mean").unwrap_or(0.0),
            100.35,
            1e-5,
        );
        near(
            number(&fixture, east, "ground_mean").unwrap_or(0.0),
            101.25,
            1e-5,
        );
        must(fixture.document.undo());
        assert!(number(&fixture, west, "ground_mean").is_none());
        assert!(number(&fixture, east, "ground_mean").is_none());
    }

    #[test]
    fn a_zone_off_the_raster_gets_no_mean_rather_than_zero() {
        let _shared = shared_data();
        let mut fixture = fixture("AZoneOffTheRasterGetsNoMeanRatherThanZero");
        // plane.asc ends at x = 40: a box from 100 to 110 covers no cell, so
        // it has no mean - absent, not 0 - and its count, 0, is a count. The
        // mean an earlier run left on it goes: it was the mean of somewhere
        // else.
        let away = draw_box(&mut fixture, (100.0, 5.0), (110.0, 15.0), "0");
        must(fixture.document.execute(set_entity_property(
            vec![away],
            "zone_mean",
            PropertyValue::Real(5.0),
        )));
        let reply = ran(
            &mut fixture,
            &format!("RASTER ZONAL FILE {} DRAWING stats=mean,count", plane()),
        );
        assert!(number(&fixture, away, "zone_mean").is_none(), "{reply}");
        near(
            number(&fixture, away, "zone_count").unwrap_or(-1.0),
            0.0,
            1e-12,
        );
    }

    #[test]
    fn a_zone_edited_while_the_job_ran_is_not_written_over() {
        let _shared = shared_data();
        let mut fixture = fixture("AZoneEditedWhileTheJobRanIsNotWrittenOver");
        let lot = draw_box(&mut fixture, (10.0, 5.0), (30.0, 25.0), "0");
        let prepared = must(prepare(
            &mut fixture.context(),
            &format!("RASTER ZONAL FILE {} DRAWING", plane()),
        ));
        assert!(prepared.reply.is_none());
        let work = prepared.work.expect("a zonal run has work");
        let apply = must(work(None, None));
        // Moved while the job ran: the statistics are of where it was.
        must(
            fixture
                .document
                .execute(move_entities(vec![lot], Vec2::new(1.0, 0.0))),
        );
        let error = refused(
            apply(&mut fixture.context()),
            "an apply after the zone moved",
        );
        assert_eq!(error.code, ErrorCode::InvalidState);
        assert!(number(&fixture, lot, "zone_mean").is_none());
    }

    #[test]
    fn preview_a_scope_of_no_shape_and_a_cancelled_run_change_nothing() {
        let _shared = shared_data();
        let mut fixture = fixture("PreviewAScopeOfNoShapeAndACancelledRunChangeNothing");
        let lot = draw_box(&mut fixture, (10.0, 5.0), (30.0, 25.0), "0");
        let before = fixture.document.history().undo_count();
        let reply = ran(
            &mut fixture,
            &format!("RASTER ZONAL FILE {} DRAWING PREVIEW", plane()),
        );
        holds(&reply, "preview valid=yes changed=no");
        // An empty layer: a scope that takes nothing is said, not refused.
        let empty = Layer {
            name: String::from("empty"),
            ..Layer::default()
        };
        must(fixture.document.execute(create_layer(empty)));
        let after_layer = fixture.document.history().undo_count();
        let reply = ran(
            &mut fixture,
            &format!("RASTER ZONAL FILE {} LAYERS empty", plane()),
        );
        holds(&reply, "ran=no zones=0");
        let prepared = must(prepare(
            &mut fixture.context(),
            &format!("RASTER ZONAL FILE {} DRAWING", plane()),
        ));
        let stop = AtomicBool::new(true);
        let work = prepared.work.expect("a zonal run has work");
        let error = refused(work(Some(&stop), None), "a stopped zonal run");
        assert_eq!(error.message, "cancelled");
        assert_eq!(fixture.document.history().undo_count(), after_layer);
        assert_eq!(after_layer, before + 1);
        assert!(number(&fixture, lot, "zone_mean").is_none());
    }

    #[test]
    fn the_csv_has_a_row_per_zone_and_is_not_replaced_without_overwrite() {
        let _shared = shared_data();
        let mut fixture = fixture("TheCsvHasARowPerZoneAndIsNotReplacedWithoutOverwrite");
        let lot = draw_box(&mut fixture, (10.0, 5.0), (30.0, 25.0), "0");
        let csv = fixture.scratch.file("zones.csv");
        let line = format!(
            "RASTER ZONAL FILE {} DRAWING stats=count csv=\"{csv}\"",
            plane()
        );
        ran(&mut fixture, &line);
        let text = std::fs::read_to_string(Path::new(&csv)).unwrap_or_default();
        let mut lines = text.lines();
        let header = lines.next().unwrap_or_default();
        let row = lines.next().unwrap_or_default();
        assert_eq!(header, "entity,count");
        // The row names its zone.
        assert_eq!(row.split(',').next().unwrap_or_default(), format!("{lot}"));
        let again = refused(fixture.run(&line), "a CSV that is there");
        assert_eq!(again.code, ErrorCode::AlreadyExists);
        assert!(fixture.run(&format!("{line} OVERWRITE")).is_ok());
    }

    #[test]
    fn a_csv_that_cannot_be_written_leaves_the_zones_as_they_were() {
        let _shared = shared_data();
        let mut fixture = fixture("ACsvThatCannotBeWrittenLeavesTheZonesAsTheyWere");
        // The reply is a failure, so the drawing must not have changed: the
        // statistics were once written, as an undo step, before the CSV was
        // tried.
        let lot = draw_box(&mut fixture, (10.0, 5.0), (30.0, 25.0), "0");
        let before: Option<Entity> = fixture.document.model().entities.find(lot).cloned();
        let revision = fixture.document.model_revision();
        let csv = fixture.scratch.file("no such folder/zones.csv");
        let error = refused(
            fixture.run(&format!(
                "RASTER ZONAL FILE {} DRAWING stats=count csv=\"{csv}\"",
                plane()
            )),
            "a CSV that cannot be written",
        );
        assert_eq!(
            error.code,
            ErrorCode::FileExportFailure,
            "{}",
            error.describe()
        );
        assert_eq!(fixture.document.model().entities.find(lot).cloned(), before);
        assert_eq!(fixture.document.model_revision(), revision);
    }

    #[test]
    fn what_the_verb_cannot_do_is_refused_naming_it() {
        let _shared = shared_data();
        let mut fixture = fixture("WhatTheVerbCannotDoIsRefusedNamingIt");
        draw_box(&mut fixture, (10.0, 5.0), (30.0, 25.0), "0");
        let zonal = format!("RASTER ZONAL FILE {} DRAWING", plane());
        for words in [
            " stats=values",
            " stats=mean,mean",
            " stats=",
            " prefix=a!b",
            " pixels=middle",
            " colour=red",
        ] {
            let error = refused(fixture.run(&format!("{zonal}{words}")), words);
            assert_eq!(error.code, ErrorCode::InvalidArgument, "{words}");
        }
        // A drawing is no raster to measure.
        let drawing = refused(fixture.run("RASTER ZONAL DRAWING"), "a drawing as a raster");
        assert_eq!(drawing.code, ErrorCode::InvalidArgument);
    }
}
