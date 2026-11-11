//! tests/geo/test_overlay_verb.cpp: GIS OVERLAY
//! (src/katana_app/geo/overlay_verbs.cpp, docs/geoprocessing.md "V2"): polygon
//! booleans between two scopes, or a scope and a file, through the one
//! executor. The drawing is tests/geo/data/lots.geojson drawn: two 50 x 40 m
//! lots side by side and a 4 m corridor across both, 120 m long. Its
//! GisOverlayContract suite is step 5's (katana_gis).

mod gis_overlay {
    use katana_app::geo::replies::{Record, parse_records};
    use katana_core::ErrorCode;
    use katana_entity::{EntityId, Geometry, PropertyValue};

    use crate::geo_fixture::{GeoFixture, geo_data, near, refused, shared_data};
    use crate::{fixture, props, records_of, text};

    /// The C++ fixture: its SetUp's drawing, and the ids it drew.
    struct Lots {
        fixture: GeoFixture,
        a: EntityId,
        b: EntityId,
        c: EntityId,
    }

    fn set_up(name: &str) -> Lots {
        let mut fixture = fixture(name);
        let a = fixture.rect(
            (0.0, 0.0),
            (50.0, 40.0),
            "lots",
            props([
                ("kind", PropertyValue::from("lot")),
                ("name", PropertyValue::from("A")),
                ("owner", PropertyValue::from("Smith")),
            ]),
        );
        let b = fixture.rect(
            (50.0, 0.0),
            (100.0, 40.0),
            "lots",
            props([
                ("kind", PropertyValue::from("lot")),
                ("name", PropertyValue::from("B")),
                ("owner", PropertyValue::from("Jones")),
            ]),
        );
        let c = fixture.rect(
            (-10.0, 18.0),
            (110.0, 22.0),
            "corridor",
            props([
                ("kind", PropertyValue::from("corridor")),
                ("name", PropertyValue::from("C")),
            ]),
        );
        Lots { fixture, a, b, c }
    }

    fn rows(reply: &str) -> Vec<Record> {
        records_of(reply, "row")
    }

    #[test]
    fn intersection_of_a_four_metre_corridor_with_each_lot_is200_square_metres() {
        // By hand: the corridor crosses each 50 m lot 4 m wide: 4 x 50 = 200 m2.
        let Lots {
            mut fixture,
            a,
            b,
            c,
        } = set_up("IntersectionOfAFourMetreCorridorWithEachLotIs200SquareMetres");
        let reply = fixture.ok("GIS OVERLAY intersection LAYERS lots WITH LAYERS corridor");
        let pieces = fixture.on("gis/overlay");
        assert_eq!(pieces.len(), 2);
        for piece in &pieces {
            let Geometry::Polyline(ring) = &piece.geometry else {
                panic!("a piece is not a polyline: {reply}");
            };
            near(ring.area(), 200.0, 1e-9);
            assert_eq!(text(piece, "gis.with"), format!("{c}"));
            // owner is the lot's alone, so it keeps its name; name is on both
            // sides, so each keeps GDAL's prefix.
            assert_ne!(text(piece, "owner"), "<none>");
            assert_eq!(text(piece, "method_name"), "C");
        }
        let found = rows(&reply);
        assert_eq!(found.len(), 2);
        for row in &found {
            assert_eq!(row.get("area"), Some("200.000"));
            assert_eq!(row.get("with"), Some(format!("{c}").as_str()));
        }
        assert_eq!(found[0].get("entity"), Some(format!("{a}").as_str()));
        assert_eq!(found[1].get("entity"), Some(format!("{b}").as_str()));
        let overlay = GeoFixture::record(&reply, "overlay").expect("an overlay record");
        assert_eq!(overlay.get("area"), Some("400.000"));
    }

    #[test]
    fn difference_leaves1800_square_metres_of_each_lot() {
        // Each lot less the corridor: two 50 x 18 strips, 1800 m2 a lot.
        let Lots { mut fixture, .. } = set_up("DifferenceLeaves1800SquareMetresOfEachLot");
        let reply = fixture.ok("GIS OVERLAY difference LAYERS lots WITH LAYERS corridor");
        let found = rows(&reply);
        assert_eq!(found.len(), 2);
        for row in &found {
            assert_eq!(row.get("area"), Some("1800.000"));
        }
        assert_eq!(fixture.on("gis/overlay").len(), 4); // each lot's two strips
        near(fixture.area_on("gis/overlay"), 3600.0, 1e-9);
    }

    #[test]
    fn a_row_carries_the_lots_properties_not_the_drawings_bookkeeping() {
        // A difference keeps the subject's fields without GDAL's input_ prefix,
        // so the drawing's layer, style, colour and type came back as the row's
        // - words about the entity, not properties of it - where an
        // intersection's rows left them out.
        let Lots { mut fixture, .. } =
            set_up("ARowCarriesTheLotsPropertiesNotTheDrawingsBookkeeping");
        let reply = fixture.ok("GIS OVERLAY difference LAYERS lots WITH LAYERS corridor");
        let found = rows(&reply);
        assert_eq!(found.len(), 2);
        for row in &found {
            for bookkeeping in ["layer", "style", "colour", "type"] {
                assert!(row.get(bookkeeping).is_none(), "{bookkeeping} in {reply}");
            }
            assert!(row.get("owner").is_some(), "{reply}");
        }
    }

    #[test]
    fn an_overlay_made_by_another_verb_lends_its_id_not_its_provenance() {
        // The corridor as another verb's result carries gis.op and gis.source.
        // A row says which overlay piece it met by with=; the overlay's own
        // gis.op and gis.source, unprefixed on a row, read as the piece's own
        // (gis.source=<the pipe> beside entity=<the lot>), and the drawn piece
        // is given the overlay's own anyway.
        let Lots { mut fixture, .. } =
            set_up("AnOverlayMadeByAnotherVerbLendsItsIdNotItsProvenance");
        let made = fixture.rect(
            (-10.0, 18.0),
            (110.0, 22.0),
            "made",
            props([
                ("gis.op", PropertyValue::from("vector buffer")),
                ("gis.source", PropertyValue::Integer(99)),
            ]),
        );
        let reply = fixture.ok("GIS OVERLAY intersection LAYERS lots WITH LAYERS made");
        let found = rows(&reply);
        assert_eq!(found.len(), 2);
        for row in &found {
            assert_eq!(row.get("with"), Some(format!("{made}").as_str()));
            assert!(row.get("gis.op").is_none(), "{reply}");
            assert!(row.get("gis.source").is_none(), "{reply}");
        }
        for piece in &fixture.on("gis/overlay") {
            assert_eq!(text(piece, "gis.op"), "vector layer-algebra");
            assert_ne!(text(piece, "gis.source"), "99");
        }
    }

    #[test]
    fn union_area_is_lots_plus_corridor_minus_overlap() {
        // By hand: 2000 + 2000 + 120 x 4 - 100 x 4 = 4080 m2, in pieces.
        let Lots { mut fixture, .. } = set_up("UnionAreaIsLotsPlusCorridorMinusOverlap");
        let reply = fixture.ok("GIS OVERLAY union LAYERS lots WITH LAYERS corridor");
        let overlay = GeoFixture::record(&reply, "overlay").expect("an overlay record");
        assert_eq!(overlay.get("area"), Some("4080.000"));
        near(fixture.area_on("gis/overlay"), 4080.0, 1e-9);
    }

    #[test]
    fn identity_keeps_the_lot_fields_on_each_piece() {
        // Identity splits the lots where the corridor crosses them and keeps
        // their extent - 4000 m2 - each piece with its lot's properties.
        let Lots { mut fixture, .. } = set_up("IdentityKeepsTheLotFieldsOnEachPiece");
        let reply = fixture.ok("GIS OVERLAY identity LAYERS lots WITH LAYERS corridor");
        near(fixture.area_on("gis/overlay"), 4000.0, 1e-9);
        let found = rows(&reply);
        assert_eq!(found.len(), 4);
        for row in &found {
            let name = row.get("input_name").unwrap_or("");
            assert!(name == "A" || name == "B", "{name}");
            assert!(row.get("owner").is_some());
        }
    }

    #[test]
    fn a_line_against_lots_gives_its_length_per_lot() {
        // A 100 m pipe along y = 10 crosses both lots: 50 m in each.
        let Lots {
            mut fixture, a, b, ..
        } = set_up("ALineAgainstLotsGivesItsLengthPerLot");
        let pipe = fixture.line(
            (0.0, 10.0),
            (100.0, 10.0),
            "pipes",
            props([("pipe", PropertyValue::from("W1"))]),
        );
        let reply = fixture.ok("GIS OVERLAY intersection LAYERS pipes WITH LAYERS lots");
        let found = rows(&reply);
        assert_eq!(found.len(), 2);
        for row in &found {
            assert_eq!(row.get("length"), Some("50.000"));
            assert_eq!(row.get("entity"), Some(format!("{pipe}").as_str()));
            assert_eq!(row.get("pipe"), Some("W1"));
        }
        assert_eq!(found[0].get("with"), Some(format!("{a}").as_str()));
        assert_eq!(found[1].get("with"), Some(format!("{b}").as_str()));
        assert_eq!(fixture.on("gis/overlay").len(), 2);
    }

    #[test]
    fn a_file_overlay_works_like_a_drawing_one() {
        // The corridor of lots.geojson, read by where=, cuts the drawn lots as
        // the drawn corridor does: 200 m2 each.
        let _shared = shared_data();
        let Lots { mut fixture, .. } = set_up("AFileOverlayWorksLikeADrawingOne");
        let reply = fixture.ok(&format!(
            "GIS OVERLAY intersection LAYERS lots WITH FILE \"{}/lots.geojson\" \
             where=\"kind='corridor'\" TO LAYER easements",
            geo_data()
        ));
        let found = rows(&reply);
        assert_eq!(found.len(), 2, "{reply}");
        for row in &found {
            assert_eq!(row.get("area"), Some("200.000"));
            assert_eq!(row.get("method_kind"), Some("corridor"));
        }
        near(fixture.area_on("easements"), 400.0, 1e-9);
        let input = GeoFixture::record(&reply, "input").expect("an input record");
        assert_eq!(input.get("arg"), Some("method"));
        assert_eq!(input.get("source"), Some("file"));
    }

    #[test]
    fn an_empty_second_scope_is_reported() {
        let Lots { mut fixture, .. } = set_up("AnEmptySecondScopeIsReported");
        let reply =
            fixture.ok("GIS OVERLAY intersection LAYERS lots WITH AREA 1000,1000,1010,1010");
        assert!(
            reply.starts_with("gis op=overlay seconds=0.000 cancelled=no ran=no"),
            "{reply}"
        );
        let mut method = false;
        for one in parse_records(&reply) {
            if one.kind == "scope" && one.get("arg") == Some("method") {
                method = true;
                assert_eq!(one.get("matched"), Some("0"));
            }
        }
        assert!(method);
        assert!(fixture.on("gis/overlay").is_empty());
    }

    #[test]
    fn keep_none_carries_only_where_each_piece_came_from() {
        let Lots { mut fixture, c, .. } = set_up("KeepNoneCarriesOnlyWhereEachPieceCameFrom");
        fixture.ok(
            "GIS OVERLAY intersection LAYERS lots WITH LAYERS corridor keep=none keepwith=none",
        );
        for piece in &fixture.on("gis/overlay") {
            assert_eq!(text(piece, "owner"), "<none>");
            assert_eq!(text(piece, "method_name"), "<none>");
            assert_ne!(text(piece, "gis.source"), "<none>");
            assert_eq!(text(piece, "gis.with"), format!("{c}"));
        }
    }

    #[test]
    fn the_rows_are_written_to_csv_and_a_file_is_replaced_only_when_asked() {
        let Lots { mut fixture, .. } =
            set_up("TheRowsAreWrittenToCsvAndAFileIsReplacedOnlyWhenAsked");
        let csv = fixture.scratch.file("rows.csv");
        fixture.ok(&format!(
            "GIS OVERLAY intersection LAYERS lots WITH LAYERS corridor keepwith=none csv=\"{csv}\""
        ));
        let written = std::fs::read_to_string(&csv).unwrap_or_default();
        assert!(written.starts_with("entity,"), "{written}");
        assert!(written.contains(",200.000\n"), "{written}");
        assert_eq!(written.matches('\n').count(), 3); // the header and two rows
        let again = refused(
            fixture.run(&format!(
                "GIS OVERLAY intersection LAYERS lots WITH LAYERS corridor csv=\"{csv}\""
            )),
            "a csv= over a file without OVERWRITE",
        );
        assert_eq!(again.code, ErrorCode::AlreadyExists);
        fixture.ok(&format!(
            "GIS OVERLAY intersection LAYERS lots WITH LAYERS corridor csv=\"{csv}\" OVERWRITE"
        ));
    }

    #[test]
    fn preview_changes_nothing() {
        let Lots { mut fixture, .. } = set_up("PreviewChangesNothing");
        let before = fixture.entity_count();
        let reply = fixture.ok("GIS OVERLAY intersection LAYERS lots WITH LAYERS corridor PREVIEW");
        assert!(reply.starts_with("gis op=overlay preview=yes"), "{reply}");
        assert_eq!(fixture.entity_count(), before);
    }

    #[test]
    fn what_it_cannot_do_is_refused_by_name() {
        let Lots { mut fixture, .. } = set_up("WhatItCannotDoIsRefusedByName");
        let mut refuse = |line: &str| {
            let error = refused(fixture.run(line), line);
            assert_eq!(
                error.code,
                ErrorCode::InvalidArgument,
                "{line}: {}",
                error.describe()
            );
        };
        refuse("GIS OVERLAY merge LAYERS lots WITH LAYERS corridor");
        refuse("GIS OVERLAY intersection LAYERS lots");
        refuse("GIS OVERLAY intersection LAYERS lots WITH");
        refuse("GIS OVERLAY intersection LAYERS lots WITH FILE x.geojson LAYERS corridor");
        refuse("GIS OVERLAY intersection LAYERS lots WITH LAYERS corridor where=\"kind='x'\"");
        refuse("GIS OVERLAY intersection LAYERS lots WITH LAYERS corridor keep=");
        assert!(fixture.on("gis/overlay").is_empty());
    }
}
