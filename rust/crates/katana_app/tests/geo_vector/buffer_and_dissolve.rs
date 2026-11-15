//! tests/geo/test_buffer_and_dissolve.cpp: GIS BUFFER and GIS DISSOLVE
//! (src/katana_app/geo/buffer_verbs.cpp, docs/geoprocessing.md "V1"), run
//! through the one executor as the window and the session run them, every
//! expected value worked by hand beside it. Its GeoLineWord suite is G1's
//! (tests/geo_frame), its GisBufferContract suite step 5's (katana_gis).

use katana_entity::{Entity, Geometry};
use katana_geometry::Polyline2;

/// The one closed polyline of `entities`: the C++'s onlyRing.
#[track_caller]
fn only_ring(entities: &[Entity]) -> Polyline2 {
    assert_eq!(entities.len(), 1);
    match &entities[0].geometry {
        Geometry::Polyline(ring) => ring.clone(),
        _ => panic!("the result is not a polyline"),
    }
}

/// The area of a result that must be a polyline: the C++'s
/// `std::get<Polyline2>(entity.geometry).area()`.
#[track_caller]
fn polyline_area(entity: &Entity) -> f64 {
    match &entity.geometry {
        Geometry::Polyline(ring) => ring.area(),
        _ => panic!("entity {} is not a polyline", entity.id),
    }
}

mod gis_buffer {
    use std::f64::consts::PI;

    use katana_app::geo::replies::line_word;
    use katana_core::ErrorCode;
    use katana_entity::{Geometry, PointGeometry, PropertyMap, PropertyValue};
    use katana_geometry::Point2;

    use super::{only_ring, polyline_area};
    use crate::geo_fixture::{GeoFixture, holds, near, refused};
    use crate::{fixture, props, text};

    #[test]
    fn a_hundred_metre_line_buffered_one_metre_flat_is200_square_metres() {
        // By hand: 100 m long, 1 m either side, no caps: a 100 x 2 rectangle.
        let mut fixture = fixture("AHundredMetreLineBufferedOneMetreFlatIs200SquareMetres");
        let source = fixture.line((0.0, 0.0), (100.0, 0.0), "0", PropertyMap::new());
        let reply = fixture.ok("GIS BUFFER DRAWING distance=1 caps=flat");
        let ring = only_ring(&fixture.on("gis/buffer"));
        assert!(ring.closed);
        near(ring.area(), 200.0, 1e-9);
        let scope = GeoFixture::record(&reply, "scope").expect("a scope record");
        assert_eq!(scope.get("matched"), Some("1"));
        assert_eq!(scope.get("lines"), Some("1"));
        let output = GeoFixture::record(&reply, "output").expect("an output record");
        assert_eq!(output.get("layer"), Some("gis/buffer"));
        assert_eq!(output.get("created"), Some("1"));
        let buffer = GeoFixture::record(&reply, "buffer").expect("a buffer record");
        assert_eq!(buffer.get("area"), Some("200.000"));
        assert_eq!(buffer.get("caps"), Some("flat"));
        // The source is named, and the algorithm; a measure kept on an entity
        // would go stale, so none is written.
        let result = fixture.on("gis/buffer")[0].clone();
        assert_eq!(text(&result, "gis.op"), "vector buffer");
        assert_eq!(text(&result, "gis.source"), format!("{source}"));
        assert_eq!(text(&result, "area"), "<none>");
    }

    #[test]
    fn a_minus_three_metre_mitre_setback_of_a_twenty_by_forty_lot_is14_by34() {
        // By hand: 3 m in from every side of a 20 x 40 lot leaves 14 x 34 =
        // 476 m2, from (3,3) to (17,37); mitred corners stay square.
        let mut fixture = fixture("AMinusThreeMetreMitreSetbackOfATwentyByFortyLotIs14By34");
        fixture.rect((0.0, 0.0), (20.0, 40.0), "lots", PropertyMap::new());
        fixture.ok("GIS BUFFER LAYERS lots distance=-3 joins=mitre TO LAYER setbacks");
        let ring = only_ring(&fixture.on("setbacks"));
        near(ring.area(), 476.0, 1e-9);
        let bounds = ring.bounding_box();
        near(bounds.min.x, 3.0, 1e-9);
        near(bounds.min.y, 3.0, 1e-9);
        near(bounds.max.x, 17.0, 1e-9);
        near(bounds.max.y, 37.0, 1e-9);
        assert_eq!(ring.without_duplicate_vertices().vertices.len(), 4);
    }

    #[test]
    fn left_of_a_west_to_east_line_is_north() {
        // Walking west to east, the left hand points north: y from 0 to 2, and
        // right is south. One-sided, the ends are flat: 100 x 2 = 200 m2 each.
        let mut fixture = fixture("LeftOfAWestToEastLineIsNorth");
        fixture.line((0.0, 0.0), (100.0, 0.0), "0", PropertyMap::new());
        fixture.ok("GIS BUFFER DRAWING distance=2 side=left TO LAYER left");
        fixture.ok("GIS BUFFER LAYERS 0 distance=2 side=right TO LAYER right");
        let left = only_ring(&fixture.on("left"));
        let right = only_ring(&fixture.on("right"));
        near(left.bounding_box().min.y, 0.0, 1e-9);
        near(left.bounding_box().max.y, 2.0, 1e-9);
        near(right.bounding_box().min.y, -2.0, 1e-9);
        near(right.bounding_box().max.y, 0.0, 1e-9);
        near(left.area(), 200.0, 1e-9);
        near(right.area(), 200.0, 1e-9);
    }

    #[test]
    fn a_per_entity_distance_from_a_property_is_used() {
        // Clearances of 1 m (a real) and 3 m (a whole number) on two 100 m
        // lines, flat caps: 200 and 600 m2. A line with no clearance is skipped
        // and counted, not buffered by some default.
        let mut fixture = fixture("APerEntityDistanceFromAPropertyIsUsed");
        let one = fixture.line(
            (0.0, 0.0),
            (100.0, 0.0),
            "0",
            props([("clearance", PropertyValue::Real(1.0))]),
        );
        let three = fixture.line(
            (0.0, 50.0),
            (100.0, 50.0),
            "0",
            props([("clearance", PropertyValue::Integer(3))]),
        );
        fixture.line((0.0, 100.0), (100.0, 100.0), "0", PropertyMap::new());
        let reply = fixture.ok("GIS BUFFER DRAWING distance=prop:clearance caps=flat");
        let results = fixture.on("gis/buffer");
        assert_eq!(results.len(), 2);
        for result in &results {
            let area = polyline_area(result);
            if text(result, "gis.source") == format!("{one}") {
                near(area, 200.0, 1e-9);
                assert_eq!(text(result, "clearance"), "1");
            } else {
                assert_eq!(text(result, "gis.source"), format!("{three}"));
                near(area, 600.0, 1e-9);
            }
        }
        let buffer = GeoFixture::record(&reply, "buffer").expect("a buffer record");
        assert_eq!(buffer.get("distance"), Some("prop:clearance"));
        assert_eq!(buffer.get("groups"), Some("2"));
        assert_eq!(buffer.get("skipped.no_distance"), Some("1"));
        assert_eq!(buffer.get("area"), Some("800.000"));
    }

    #[test]
    fn a_round_point_buffer_is_an_exact_circle() {
        // A point buffered 5 m with round caps is the circle of radius 5 about
        // it, pi x 25 = 78.540 m2 - drawn as a circle, not as chords.
        let mut fixture = fixture("ARoundPointBufferIsAnExactCircle");
        let source = fixture.add(
            Geometry::Point(PointGeometry {
                position: Point2::new(10.0, 20.0),
            }),
            "trees",
            props([("species", PropertyValue::from("fig"))]),
        );
        let reply = fixture.ok("GIS BUFFER LAYERS trees distance=5 TO LAYER tpz");
        let results = fixture.on("tpz");
        assert_eq!(results.len(), 1);
        let Geometry::Circle(circle) = &results[0].geometry else {
            panic!("the buffer of a point is not a circle: {reply}");
        };
        assert_eq!(circle.center, Point2::new(10.0, 20.0));
        assert_eq!(circle.radius, 5.0);
        assert_eq!(text(&results[0], "species"), "fig");
        assert_eq!(text(&results[0], "gis.source"), format!("{source}"));
        let buffer = GeoFixture::record(&reply, "buffer").expect("a buffer record");
        assert_eq!(buffer.get("circles"), Some("1"));
        assert_eq!(buffer.get("area"), Some("78.540"));
        near(fixture.area_on("tpz"), 25.0 * PI, 1e-12);
    }

    #[test]
    fn a_line_buffered_inwards_is_nothing_and_says_so() {
        // GEOS buffers a line inwards to an empty polygon: nothing is drawn, and
        // the reply counts it rather than inventing a shape.
        let mut fixture = fixture("ALineBufferedInwardsIsNothingAndSaysSo");
        fixture.line((0.0, 0.0), (100.0, 0.0), "0", PropertyMap::new());
        let reply = fixture.ok("GIS BUFFER DRAWING distance=-1");
        assert!(fixture.on("gis/buffer").is_empty());
        let buffer = GeoFixture::record(&reply, "buffer").expect("a buffer record");
        assert_eq!(buffer.get("empty"), Some("1"));
        assert_eq!(buffer.get("features"), Some("0"));
    }

    #[test]
    fn dissolved_buffers_of_crossing_lines_are_one_area() {
        // Two 100 m lines crossing at right angles, 1 m either side with flat
        // caps: two 200 m2 strips sharing a 2 x 2 square, 400 - 4 = 396 m2 as
        // one area.
        let mut fixture = fixture("DissolvedBuffersOfCrossingLinesAreOneArea");
        fixture.line((0.0, 50.0), (100.0, 50.0), "0", PropertyMap::new());
        fixture.line((50.0, 0.0), (50.0, 100.0), "0", PropertyMap::new());
        let reply = fixture.ok("GIS BUFFER DRAWING distance=1 caps=flat DISSOLVE");
        assert_eq!(fixture.on("gis/buffer").len(), 1);
        near(fixture.area_on("gis/buffer"), 396.0, 1e-9);
        let buffer = GeoFixture::record(&reply, "buffer").expect("a buffer record");
        assert_eq!(buffer.get("dissolve"), Some("all"));
    }

    #[test]
    fn options_after_a_where_filter_are_the_verbs_own() {
        // The filter ends where its conditions do: distance= and caps= are the
        // buffer's, and only the line matches TYPE=line.
        let mut fixture = fixture("OptionsAfterAWhereFilterAreTheVerbsOwn");
        fixture.line((0.0, 0.0), (100.0, 0.0), "0", PropertyMap::new());
        fixture.rect((0.0, 10.0), (10.0, 20.0), "0", PropertyMap::new());
        let reply = fixture.ok("GIS BUFFER DRAWING WHERE TYPE=line distance=1 caps=flat");
        let scope = GeoFixture::record(&reply, "scope").expect("a scope record");
        assert_eq!(scope.get("matched"), Some("1"));
        near(fixture.area_on("gis/buffer"), 200.0, 1e-9);
    }

    #[test]
    fn preview_changes_nothing() {
        let mut fixture = fixture("PreviewChangesNothing");
        fixture.line((0.0, 0.0), (100.0, 0.0), "0", PropertyMap::new());
        let before = fixture.entity_count();
        let reply = fixture.ok("GIS BUFFER DRAWING distance=1 PREVIEW");
        assert_eq!(fixture.entity_count(), before);
        assert!(reply.starts_with("gis op=buffer preview=yes"), "{reply}");
        holds(&reply, "preview valid=yes changed=no");
        let scope = GeoFixture::record(&reply, "scope").expect("a scope record");
        assert_eq!(scope.get("matched"), Some("1"));
    }

    #[test]
    fn a_scope_that_takes_nothing_is_reported_not_an_error() {
        let mut fixture = fixture("AScopeThatTakesNothingIsReportedNotAnError");
        fixture.line((0.0, 0.0), (100.0, 0.0), "0", PropertyMap::new());
        let reply = fixture.ok("GIS BUFFER AREA 1000,1000,1010,1010 distance=1");
        assert!(
            reply.starts_with("gis op=buffer seconds=0.000 cancelled=no ran=no"),
            "{reply}"
        );
        let scope = GeoFixture::record(&reply, "scope").expect("a scope record");
        assert_eq!(scope.get("matched"), Some("0"));
        assert!(fixture.on("gis/buffer").is_empty());
    }

    #[test]
    fn what_it_cannot_do_is_refused_by_name() {
        let mut fixture = fixture("WhatItCannotDoIsRefusedByName");
        fixture.line((0.0, 0.0), (100.0, 0.0), "0", PropertyMap::new());
        let mut refuse = |words: &str, code: ErrorCode| {
            let error = refused(fixture.run(&format!("GIS BUFFER DRAWING {words}")), words);
            assert_eq!(error.code, code, "{words}: {}", error.describe());
        };
        refuse("caps=flat", ErrorCode::InvalidArgument); // no distance
        refuse("distance=0", ErrorCode::InvalidArgument); // nothing to buffer by
        refuse("distance=wide", ErrorCode::InvalidArgument); // not a number
        refuse("distance=1 side=up", ErrorCode::InvalidArgument); // not a side
        refuse("distance=1 distance=2", ErrorCode::InvalidArgument);
        refuse("distance=prop:clearance", ErrorCode::InvalidArgument); // no such property
        refuse("distance=1 dissolve=owner", ErrorCode::InvalidArgument);
        refuse("distance=1 TO REFERENCE shade", ErrorCode::Unsupported);
        refuse("distance=1 banana", ErrorCode::InvalidArgument);
        assert!(fixture.on("gis/buffer").is_empty());
    }

    #[test]
    fn an_output_named_like_a_keyword_goes_where_it_says_once_quoted() {
        // TO LAYER preview was refused ("needs the layer's path"), and a dialog
        // or an agent that wrote a name as it was typed wrote exactly that.
        // Quoted by the one rule, the name is a name.
        let mut fixture = fixture("AnOutputNamedLikeAKeywordGoesWhereItSaysOnceQuoted");
        fixture.line((0.0, 0.0), (100.0, 0.0), "pipes", PropertyMap::new());
        let bare = fixture.run("GIS BUFFER LAYERS pipes distance=1 TO LAYER preview");
        assert!(bare.is_err());
        for name in ["preview", "CONFIRM", "overwrite", "from"] {
            let word = line_word(name).expect("a keyword is quoted, never refused");
            let reply = fixture.ok(&format!(
                "GIS BUFFER LAYERS pipes distance=1 TO LAYER {word}"
            ));
            assert_eq!(fixture.on(name).len(), 1, "{reply}");
        }
    }
}

mod gis_dissolve {
    use katana_commands::move_entities;
    use katana_core::ErrorCode;
    use katana_entity::{PropertyMap, PropertyValue};
    use katana_geometry::Vec2;

    use super::polyline_area;
    use crate::geo_fixture::{GeoFixture, holds, must, near, refused};
    use crate::{fixture, props, text};

    #[test]
    fn dissolve_by_owner_merges_adjacent_lots() {
        // Two 50 x 40 lots of Smith's sharing an edge: one 100 x 40 area of
        // 4000 m2; Jones's lot on its own stays 2000 m2.
        let mut fixture = fixture("DissolveByOwnerMergesAdjacentLots");
        fixture.rect(
            (0.0, 0.0),
            (50.0, 40.0),
            "lots",
            props([("owner", PropertyValue::from("Smith"))]),
        );
        fixture.rect(
            (50.0, 0.0),
            (100.0, 40.0),
            "lots",
            props([("owner", PropertyValue::from("Smith"))]),
        );
        fixture.rect(
            (0.0, 100.0),
            (50.0, 140.0),
            "lots",
            props([("owner", PropertyValue::from("Jones"))]),
        );
        let reply = fixture.ok("GIS DISSOLVE LAYERS lots by=owner");
        let results = fixture.on("gis/dissolve");
        assert_eq!(results.len(), 2);
        for result in &results {
            let owner = text(result, "owner");
            let expected = if owner == "Smith" { 4000.0 } else { 2000.0 };
            let area = polyline_area(result);
            assert!((area - expected).abs() <= 1e-9, "{owner}: {area}");
        }
        let dissolve = GeoFixture::record(&reply, "dissolve").expect("a dissolve record");
        assert_eq!(dissolve.get("groups"), Some("2"));
        assert_eq!(dissolve.get("areas"), Some("3"));
        assert_eq!(dissolve.get("area"), Some("6000.000"));
        assert_eq!(fixture.on("lots").len(), 3); // without REPLACE the lots stay
    }

    #[test]
    fn keep_identical_keeps_only_shared_values() {
        // Both lots are zoned R1 and differ in name: keep=identical carries the
        // zone to the merged area and leaves the names behind.
        let mut fixture = fixture("KeepIdenticalKeepsOnlySharedValues");
        fixture.rect(
            (0.0, 0.0),
            (50.0, 40.0),
            "lots",
            props([
                ("owner", PropertyValue::from("Smith")),
                ("zone", PropertyValue::from("R1")),
                ("name", PropertyValue::from("A")),
            ]),
        );
        fixture.rect(
            (50.0, 0.0),
            (100.0, 40.0),
            "lots",
            props([
                ("owner", PropertyValue::from("Smith")),
                ("zone", PropertyValue::from("R1")),
                ("name", PropertyValue::from("B")),
            ]),
        );
        fixture.ok("GIS DISSOLVE LAYERS lots by=owner keep=identical TO LAYER kept");
        fixture.ok("GIS DISSOLVE LAYERS lots by=owner TO LAYER plain");
        let kept = fixture.on("kept");
        assert_eq!(kept.len(), 1);
        assert_eq!(text(&kept[0], "owner"), "Smith");
        assert_eq!(text(&kept[0], "zone"), "R1");
        assert_eq!(text(&kept[0], "name"), "<none>");
        let plain = fixture.on("plain");
        assert_eq!(plain.len(), 1);
        assert_eq!(text(&plain[0], "owner"), "Smith");
        assert_eq!(text(&plain[0], "zone"), "<none>");
    }

    #[test]
    fn replace_deletes_the_sources_in_the_same_undo_step() {
        let mut fixture = fixture("ReplaceDeletesTheSourcesInTheSameUndoStep");
        fixture.rect((0.0, 0.0), (50.0, 40.0), "lots", PropertyMap::new());
        fixture.rect((50.0, 0.0), (100.0, 40.0), "lots", PropertyMap::new());
        let reply = fixture.ok("GIS DISSOLVE LAYERS lots REPLACE");
        assert!(fixture.on("lots").is_empty());
        near(fixture.area_on("gis/dissolve"), 4000.0, 1e-9);
        let output = GeoFixture::record(&reply, "output").expect("an output record");
        assert_eq!(output.get("deleted"), Some("2"));
        fixture.type_line("UNDO");
        assert_eq!(fixture.on("lots").len(), 2);
        assert!(fixture.on("gis/dissolve").is_empty());
    }

    #[test]
    fn preview_changes_nothing_and_lines_are_not_merged() {
        let mut fixture = fixture("PreviewChangesNothingAndLinesAreNotMerged");
        fixture.rect((0.0, 0.0), (50.0, 40.0), "lots", PropertyMap::new());
        fixture.line((0.0, 50.0), (100.0, 50.0), "lots", PropertyMap::new());
        let before = fixture.entity_count();
        let reply = fixture.ok("GIS DISSOLVE LAYERS lots REPLACE PREVIEW");
        assert_eq!(fixture.entity_count(), before);
        assert!(reply.starts_with("gis op=dissolve preview=yes"), "{reply}");
        let dissolve = GeoFixture::record(&reply, "dissolve").expect("a dissolve record");
        assert_eq!(dissolve.get("areas"), Some("1"));
        holds(&reply, "points and lines in the scope are not areas");
    }

    #[test]
    fn an_in_place_replace_refuses_a_drawing_changed_while_it_ran() {
        // Prepared, then a lot moves before the apply: deleting it now would
        // lose the move, so the apply refuses and changes nothing.
        let mut fixture = fixture("AnInPlaceReplaceRefusesADrawingChangedWhileItRan");
        fixture.rect((0.0, 0.0), (50.0, 40.0), "lots", PropertyMap::new());
        let moved = fixture.rect((50.0, 0.0), (100.0, 40.0), "lots", PropertyMap::new());
        let prepared = must(fixture.prepare("GIS DISSOLVE LAYERS lots REPLACE"));
        let work = prepared.work.expect("a run has work");
        let apply = must(work(None, None));
        assert!(
            fixture
                .document
                .execute(move_entities(vec![moved], Vec2::new(1.0, 0.0)))
                .is_ok()
        );
        let before = fixture.entity_count();
        let error = refused(
            apply(&mut fixture.context()),
            "an apply on a changed drawing",
        );
        assert_eq!(error.code, ErrorCode::InvalidState);
        assert_eq!(fixture.entity_count(), before);
    }
}

mod gis_buffer_session {
    use katana_app::Session;

    use crate::CliSession;
    use crate::geo_fixture::holds;

    #[test]
    fn the_verbs_run_through_the_session_and_help_names_them() {
        // katana_cli's and katana_mcp's path: the session asks the executor.
        let mut session = CliSession::start();
        assert!(session.run("LINE 0,0 100,0"));
        assert!(session.run("GIS BUFFER DRAWING distance=1 caps=flat"));
        assert!(!session.run("GIS BUFFER DRAWING"));
        let help = Session::help_text();
        holds(&help, "GIS BUFFER");
        holds(&help, "GIS DISSOLVE");
    }
}
