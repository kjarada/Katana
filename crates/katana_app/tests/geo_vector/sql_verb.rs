//! tests/geo/test_sql_verb.cpp: GIS SQL (src/katana_app/geo/sql_verbs.cpp,
//! docs/geoprocessing.md "V5"), the drawing queried as tables through the one
//! executor, every expected value worked by hand beside it. Its GisQueryMcp
//! suite drives the MCP server (step 6's C); its GisSqlContract suite is step
//! 5's (katana_gis).

mod gis_sql {
    use std::path::Path;

    use katana_app::geo::replies::{Record, parse_records};
    use katana_core::ErrorCode;
    use katana_entity::entity::property_to_string;
    use katana_entity::{EntityId, Geometry, PropertyMap, PropertyValue};

    use crate::geo_fixture::{GeoFixture, near, refused};
    use crate::{fixture, props, records_of};

    /// Smith's two 50 x 40 lots (2000 m2 each) and Jones's 30 x 20 (600 m2):
    /// their ids, smithA, smithB and jones.
    fn lots(fixture: &mut GeoFixture) -> (EntityId, EntityId, EntityId) {
        let smith_a = fixture.rect(
            (0.0, 0.0),
            (50.0, 40.0),
            "lots",
            props([("owner", PropertyValue::from("Smith"))]),
        );
        let smith_b = fixture.rect(
            (50.0, 0.0),
            (100.0, 40.0),
            "lots",
            props([("owner", PropertyValue::from("Smith"))]),
        );
        let jones = fixture.rect(
            (0.0, 100.0),
            (30.0, 120.0),
            "lots",
            props([("owner", PropertyValue::from("Jones"))]),
        );
        (smith_a, smith_b, jones)
    }

    fn rows(reply: &str) -> Vec<Record> {
        records_of(reply, "row")
    }

    #[test]
    fn st_area_of_a_fifty_by_forty_lot_is2000() {
        let mut fixture = fixture("StAreaOfAFiftyByFortyLotIs2000");
        let lot = fixture.rect((0.0, 0.0), (50.0, 40.0), "lots", PropertyMap::new());
        let reply = fixture
            .ok("GIS SQL \"SELECT katana_id, ST_Area(geometry) AS area FROM polygons\" DRAWING");
        let found = rows(&reply);
        assert_eq!(found.len(), 1, "{reply}");
        assert_eq!(found[0].get("katana_id"), Some(format!("{lot}").as_str()));
        assert_eq!(found[0].get("area"), Some("2000"));
        // The columns say their types, so a reader knows a number from text.
        let mut typed = false;
        for one in parse_records(&reply) {
            if one.kind == "column" && one.get("name") == Some("area") {
                assert_eq!(one.get("type"), Some("real"));
                typed = true;
            }
        }
        assert!(typed);
        // A query changes nothing.
        assert_eq!(fixture.entity_count(), 1);
    }

    #[test]
    fn st_area_of_its_minus_one_metre_buffer_is1824() {
        // One metre in from each side of 50 x 40 leaves 48 x 38 = 1824 m2; the
        // corners of an inward offset stay square.
        let mut fixture = fixture("StAreaOfItsMinusOneMetreBufferIs1824");
        fixture.rect((0.0, 0.0), (50.0, 40.0), "lots", PropertyMap::new());
        let found = rows(&fixture.ok(
            "GIS SQL \"SELECT ST_Area(ST_Buffer(geometry, -1)) AS inner FROM polygons\" DRAWING",
        ));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].get("inner"), Some("1824"));
    }

    #[test]
    fn group_by_owner_sums_hand_computed_areas() {
        let mut fixture = fixture("GroupByOwnerSumsHandComputedAreas");
        lots(&mut fixture);
        let found = rows(&fixture.ok(
            "GIS SQL \"SELECT owner, SUM(ST_Area(geometry)) AS area, COUNT(*) AS lots FROM \
             polygons GROUP BY owner ORDER BY owner\" DRAWING",
        ));
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].get("owner"), Some("Jones"));
        assert_eq!(found[0].get("area"), Some("600"));
        assert_eq!(found[0].get("lots"), Some("1"));
        assert_eq!(found[1].get("owner"), Some("Smith"));
        assert_eq!(found[1].get("area"), Some("4000"));
        assert_eq!(found[1].get("lots"), Some("2"));
    }

    #[test]
    fn as_select_selects_the_returned_ids() {
        let mut fixture = fixture("AsSelectSelectsTheReturnedIds");
        let (smith_a, smith_b, _) = lots(&mut fixture);
        let reply = fixture.ok(
            "GIS SQL \"SELECT katana_id FROM polygons WHERE owner = 'Smith'\" DRAWING AS SELECT",
        );
        let ids = fixture.document.selection().ids();
        assert_eq!(ids.len(), 2);
        assert_eq!(ids[0], smith_a);
        assert_eq!(ids[1], smith_b);
        let output = GeoFixture::record(&reply, "output").expect("an output record");
        assert_eq!(output.get("target"), Some("selection"));
        assert_eq!(output.get("selected"), Some("2"));
        // Without the katana_id column there is nothing to select by.
        let error = refused(
            fixture.run("GIS SQL \"SELECT owner FROM polygons\" DRAWING AS SELECT"),
            "a selection with no katana_id column",
        );
        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }

    #[test]
    fn as_layer_creates_one_undo_step() {
        let mut fixture = fixture("AsLayerCreatesOneUndoStep");
        let (_, _, jones) = lots(&mut fixture);
        let before = fixture.entity_count();
        fixture
            .ok("GIS SQL \"SELECT * FROM polygons WHERE owner = 'Jones'\" DRAWING AS LAYER copies");
        let made = fixture.on("copies");
        assert_eq!(made.len(), 1);
        let Geometry::Polyline(ring) = &made[0].geometry else {
            panic!("the copy is not a polyline");
        };
        near(ring.area(), 600.0, 1e-9);
        assert_eq!(
            made[0].properties.get("gis.source").map(property_to_string),
            Some(format!("{jones}"))
        );
        assert_eq!(
            made[0].properties.get("owner").map(property_to_string),
            Some(String::from("Jones"))
        );
        fixture.type_line("UNDO");
        assert_eq!(fixture.entity_count(), before);
    }

    #[test]
    fn a_non_select_statement_is_refused() {
        let mut fixture = fixture("ANonSelectStatementIsRefused");
        lots(&mut fixture);
        let before = fixture.entity_count();
        for line in [
            "GIS SQL \"DELETE FROM polygons\" DRAWING",
            "GIS SQL \"UPDATE polygons SET owner = 'x'\" DRAWING",
            "GIS SQL \"SELECT 1; DELETE FROM polygons\" DRAWING",
            "GIS SQL SELECT DRAWING",
        ] {
            let error = refused(fixture.run(line), line);
            assert_eq!(error.code, ErrorCode::InvalidArgument, "{line}");
        }
        assert_eq!(fixture.entity_count(), before);
    }

    #[test]
    fn blob_to_file_is_refused() {
        // Spatialite's file functions stay off (SPATIALITE_SECURITY is never
        // set): BlobToFile writes nothing, whatever the query answers.
        let mut fixture = fixture("BlobToFileIsRefused");
        fixture.rect((0.0, 0.0), (50.0, 40.0), "lots", PropertyMap::new());
        let leak = fixture.scratch.file("leak.bin");
        let reply = fixture.run(&format!(
            "GIS SQL \"SELECT BlobToFile(X'414243', '{leak}') AS written FROM polygons\" DRAWING"
        ));
        if let Ok(reply) = reply {
            let found = rows(&reply);
            assert_eq!(found.len(), 1, "{reply}");
            assert_ne!(found[0].get("written"), Some("1"), "{reply}");
        }
        assert!(!Path::new(&leak).exists());
    }

    #[test]
    fn the_rows_are_written_to_csv() {
        let mut fixture = fixture("TheRowsAreWrittenToCsv");
        lots(&mut fixture);
        let csv = fixture.scratch.file("owners.csv");
        fixture.ok(&format!(
            "GIS SQL \"SELECT owner, SUM(ST_Area(geometry)) AS area FROM polygons GROUP BY owner \
             ORDER BY owner\" DRAWING csv=\"{csv}\""
        ));
        let text = std::fs::read_to_string(csv).unwrap_or_default();
        assert_eq!(text, "owner,area\nJones,600\nSmith,4000\n");
    }

    #[test]
    fn the_gdal_verb_reports_and_selects_too() {
        let mut fixture = fixture("TheGdalVerbReportsAndSelectsToo");
        let (_, _, jones) = lots(&mut fixture);
        let report = fixture.ok(
            "GDAL vector sql \"--sql=SELECT katana_id, owner FROM polygons WHERE owner = 'Jones'\" \
             --dialect=SQLITE FROM DRAWING TO REPORT",
        );
        let found = rows(&report);
        assert_eq!(found.len(), 1, "{report}");
        assert_eq!(found[0].get("owner"), Some("Jones"));
        fixture.ok(
            "GDAL vector sql \"--sql=SELECT katana_id FROM polygons WHERE owner = 'Jones'\" \
             --dialect=SQLITE FROM DRAWING TO SELECTION",
        );
        let ids = fixture.document.selection().ids();
        assert_eq!(ids.len(), 1);
        assert_eq!(ids[0], jones);
    }

    #[test]
    fn preview_changes_nothing() {
        let mut fixture = fixture("PreviewChangesNothing");
        lots(&mut fixture);
        let reply =
            fixture.ok("GIS SQL \"SELECT * FROM polygons\" DRAWING AS LAYER copies PREVIEW");
        assert!(reply.starts_with("gis op=sql preview=yes"), "{reply}");
        assert!(fixture.on("copies").is_empty());
    }
}
