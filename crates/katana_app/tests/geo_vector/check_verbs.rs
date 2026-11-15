//! tests/geo/test_check_verbs.cpp: GIS CHECK, GIS REPAIR and GIS COVERAGE
//! (src/katana_app/geo/check_verbs.cpp, docs/geoprocessing.md "V4"), through
//! the one executor, every expected value worked by hand beside it. Its
//! GisCheckContract suite is step 5's (katana_gis).

mod gis_check {
    use katana_app::geo::replies::Record;
    use katana_commands::move_entities;
    use katana_core::ErrorCode;
    use katana_entity::{EntityId, Geometry, PointGeometry, PropertyMap, PropertyValue};
    use katana_geometry::{Point2, Vec2};

    use crate::geo_fixture::{GeoFixture, holds, lacks, must, near, refused};
    use crate::{fixture, props, records_of, text};

    /// (30,0) (40,10) (40,0) (30,10): the edges (30,0)-(40,10) and
    /// (40,0)-(30,10) cross where x - 30 = y and 40 - x = y, at (35, 5).
    fn bow_tie(fixture: &mut GeoFixture, properties: PropertyMap) -> EntityId {
        fixture.area(
            &[
                Point2::new(30.0, 0.0),
                Point2::new(40.0, 10.0),
                Point2::new(40.0, 0.0),
                Point2::new(30.0, 10.0),
            ],
            "lots",
            properties,
        )
    }

    fn problems(reply: &str) -> Vec<Record> {
        records_of(reply, "problem")
    }

    /// Four lots about an enclosed gap 0.02 m wide: L (0..50) and R
    /// (50.02..100) side by side, T above and B below them across the full
    /// width, each with vertices where the others' corners meet it, so every
    /// edge but the gap's matches.
    fn gap_fixture(fixture: &mut GeoFixture) {
        fixture.rect(
            (0.0, 0.0),
            (50.0, 40.0),
            "lots",
            props([("name", PropertyValue::from("L"))]),
        );
        fixture.rect(
            (50.02, 0.0),
            (100.0, 40.0),
            "lots",
            props([("name", PropertyValue::from("R"))]),
        );
        fixture.area(
            &[
                Point2::new(0.0, 40.0),
                Point2::new(50.0, 40.0),
                Point2::new(50.02, 40.0),
                Point2::new(100.0, 40.0),
                Point2::new(100.0, 80.0),
                Point2::new(0.0, 80.0),
            ],
            "lots",
            props([("name", PropertyValue::from("T"))]),
        );
        fixture.area(
            &[
                Point2::new(0.0, -40.0),
                Point2::new(100.0, -40.0),
                Point2::new(100.0, 0.0),
                Point2::new(50.02, 0.0),
                Point2::new(50.0, 0.0),
                Point2::new(0.0, 0.0),
            ],
            "lots",
            props([("name", PropertyValue::from("B"))]),
        );
    }

    /// The area of an entity that must be a polyline.
    #[track_caller]
    fn polyline_area(fixture: &GeoFixture, id: EntityId) -> f64 {
        match fixture
            .document
            .model()
            .entities
            .find(id)
            .map(|entity| &entity.geometry)
        {
            Some(Geometry::Polyline(ring)) => ring.area(),
            _ => panic!("entity {id} is not a polyline"),
        }
    }

    #[test]
    fn a_bow_tie_self_intersects_at35_comma5() {
        let mut fixture = fixture("ABowTieSelfIntersectsAt35Comma5");
        let tie = bow_tie(&mut fixture, PropertyMap::new());
        fixture.rect((0.0, 0.0), (10.0, 10.0), "lots", PropertyMap::new());
        let reply = fixture.ok("GIS CHECK DRAWING");
        let found = problems(&reply);
        assert_eq!(found.len(), 1, "{reply}");
        assert_eq!(found[0].get("kind"), Some("self-intersection"));
        assert_eq!(found[0].get("entity"), Some(format!("{tie}").as_str()));
        assert_eq!(found[0].get("at"), Some("35,5"));
        assert_eq!(found[0].get("reason"), Some("Self-intersection"));
        let check = GeoFixture::record(&reply, "check").expect("a check record");
        assert_eq!(check.get("features"), Some("2"));
        assert_eq!(check.get("problems"), Some("1"));
        // A check changes nothing without markers.
        assert_eq!(fixture.entity_count(), 2);
    }

    #[test]
    fn repair_of_the_bow_tie_gives_two_triangles_of25_square_metres_each() {
        // Split at (35,5): two triangles of base 10 and height 5, 25 m2 each.
        let mut fixture = fixture("RepairOfTheBowTieGivesTwoTrianglesOf25SquareMetresEach");
        let tie = bow_tie(&mut fixture, PropertyMap::new());
        let reply = fixture.ok("GIS REPAIR DRAWING");
        let lots = fixture.on("lots");
        assert_eq!(lots.len(), 2);
        for lot in &lots {
            near(polyline_area(&fixture, lot.id), 25.0, 1e-9);
        }
        let split = GeoFixture::record(&reply, "split").expect("a split record");
        assert_eq!(split.get("entity"), Some(format!("{tie}").as_str()));
        assert_eq!(split.get("parts"), Some("2"));
        assert_eq!(
            split.get("created"),
            Some(format!("{}", lots[lots.len() - 1].id).as_str())
        );
        let output = GeoFixture::record(&reply, "output").expect("an output record");
        assert_eq!(output.get("target"), Some("in-place"));
        assert_eq!(output.get("updated"), Some("1"));
        assert_eq!(output.get("created"), Some("1"));
        // Repaired, the check finds nothing.
        assert!(problems(&fixture.ok("GIS CHECK DRAWING")).is_empty());
    }

    #[test]
    fn repair_keeps_the_id() {
        // The id - and so a label or an association on it - survives; the other
        // triangle is a copy of the lot, properties and all, naming its source.
        let mut fixture = fixture("RepairKeepsTheId");
        let tie = bow_tie(
            &mut fixture,
            props([("owner", PropertyValue::from("Smith"))]),
        );
        let valid = fixture.rect((0.0, 0.0), (10.0, 10.0), "lots", PropertyMap::new());
        let square_before = fixture
            .document
            .model()
            .entities
            .find(valid)
            .cloned()
            .expect("the square is drawn");
        let reply = fixture.ok("GIS REPAIR LAYERS lots");
        let kept = fixture
            .document
            .model()
            .entities
            .find(tie)
            .cloned()
            .expect("the bow-tie keeps its id");
        near(polyline_area(&fixture, tie), 25.0, 1e-9);
        assert_eq!(text(&kept, "owner"), "Smith");
        let lots = fixture.on("lots");
        let made = lots.last().expect("the other triangle is made");
        assert_ne!(made.id, tie);
        assert_eq!(text(made, "owner"), "Smith");
        assert_eq!(text(made, "gis.source"), format!("{tie}"));
        // A valid area is not touched, not even rewritten.
        assert_eq!(
            fixture.document.model().entities.find(valid),
            Some(&square_before)
        );
        let repair = GeoFixture::record(&reply, "repair").expect("a repair record");
        assert_eq!(repair.get("valid"), Some("1"));
        assert_eq!(repair.get("repaired"), Some("1"));
        // One step: UNDO brings the bow-tie back whole.
        fixture.type_line("UNDO");
        assert_eq!(fixture.on("lots").len(), 2);
        match fixture
            .document
            .model()
            .entities
            .find(tie)
            .map(|entity| &entity.geometry)
        {
            Some(Geometry::Polyline(ring)) => assert_eq!(ring.vertices.len(), 4),
            _ => panic!("the bow-tie is a polyline again"),
        }
    }

    #[test]
    fn repair_refuses_a_drawing_changed_while_it_ran() {
        let mut fixture = fixture("RepairRefusesADrawingChangedWhileItRan");
        let tie = bow_tie(&mut fixture, PropertyMap::new());
        let prepared = must(fixture.prepare("GIS REPAIR DRAWING"));
        let work = prepared.work.expect("a run has work");
        let apply = must(work(None, None));
        assert!(
            fixture
                .document
                .execute(move_entities(vec![tie], Vec2::new(1.0, 0.0)))
                .is_ok()
        );
        let moved = fixture
            .document
            .model()
            .entities
            .find(tie)
            .cloned()
            .expect("the bow-tie is there");
        let error = refused(
            apply(&mut fixture.context()),
            "an apply on a changed drawing",
        );
        assert_eq!(error.code, ErrorCode::InvalidState);
        assert_eq!(fixture.document.model().entities.find(tie), Some(&moved));
    }

    #[test]
    fn preview_changes_nothing() {
        let mut fixture = fixture("PreviewChangesNothing");
        bow_tie(&mut fixture, PropertyMap::new());
        let before = fixture.entity_count();
        let reply = fixture.ok("GIS REPAIR DRAWING PREVIEW");
        assert!(reply.starts_with("gis op=repair preview=yes"), "{reply}");
        assert_eq!(fixture.entity_count(), before);
        holds(
            &fixture.ok("GIS CHECK DRAWING markers=checks PREVIEW"),
            "preview valid=yes changed=no",
        );
        assert!(fixture.on("checks").is_empty());
    }

    #[test]
    fn markers_are_replaced_on_rerun() {
        let mut fixture = fixture("MarkersAreReplacedOnRerun");
        bow_tie(&mut fixture, PropertyMap::new());
        fixture.ok("GIS CHECK LAYERS lots markers=checks");
        let markers = fixture.on("checks");
        assert_eq!(markers.len(), 1);
        let marker = &markers[0];
        let Geometry::Point(PointGeometry { position }) = &marker.geometry else {
            panic!("a check's marker is a point");
        };
        assert_eq!(*position, Point2::new(35.0, 5.0));
        assert_eq!(text(marker, "gis.marker"), "check");
        assert_eq!(text(marker, "gis.problem"), "self-intersection");
        // Again: the last check's marker gives way to this one's.
        let again = fixture.ok("GIS CHECK LAYERS lots markers=checks");
        assert_eq!(fixture.on("checks").len(), 1);
        let output = GeoFixture::record(&again, "output").expect("an output record");
        assert_eq!(output.get("arg"), Some("markers"));
        assert_eq!(output.get("created"), Some("1"));
        assert_eq!(output.get("deleted"), Some("1"));
        // Repaired and checked again: no problem, and no marker left behind.
        fixture.ok("GIS REPAIR LAYERS lots");
        fixture.ok("GIS CHECK LAYERS lots markers=checks");
        assert!(fixture.on("checks").is_empty());
    }

    #[test]
    fn overlapping_lots_report_their_overlap_edges() {
        // A at 0..50 and B at 49..100 overlap by a metre: each has an edge
        // inside the other - A's at x = 50, B's at x = 49.
        let mut fixture = fixture("OverlappingLotsReportTheirOverlapEdges");
        let a = fixture.rect((0.0, 0.0), (50.0, 40.0), "lots", PropertyMap::new());
        let b = fixture.rect((49.0, 0.0), (100.0, 40.0), "lots", PropertyMap::new());
        let reply = fixture.ok("GIS COVERAGE CHECK LAYERS lots");
        let found = problems(&reply);
        assert_eq!(found.len(), 2, "{reply}");
        let mut entities = Vec::new();
        for problem in &found {
            assert_eq!(problem.get("kind"), Some("overlap"), "{reply}");
            entities.push(String::from(problem.get("entity").unwrap_or("")));
        }
        assert!(entities.contains(&format!("{a}")));
        assert!(entities.contains(&format!("{b}")));
        let coverage = GeoFixture::record(&reply, "coverage").expect("a coverage record");
        assert_eq!(coverage.get("overlaps"), Some("2"));
    }

    #[test]
    fn an_enclosed_gap_narrower_than_gap_is_reported() {
        let mut fixture = fixture("AnEnclosedGapNarrowerThanGapIsReported");
        gap_fixture(&mut fixture);
        // Without gap= a gap is not looked for, and every edge matches.
        assert!(problems(&fixture.ok("GIS COVERAGE CHECK LAYERS lots")).is_empty());
        // The 0.02 m gap is narrower than 0.05: L's edge at x = 50 and R's at
        // x = 50.02 border it.
        let reply = fixture.ok("GIS COVERAGE CHECK LAYERS lots gap=0.05");
        let found = problems(&reply);
        assert_eq!(found.len(), 2, "{reply}");
        for problem in &found {
            assert_eq!(problem.get("kind"), Some("gap"), "{reply}");
            assert_eq!(problem.get("length"), Some("40.000"));
        }
        let coverage = GeoFixture::record(&reply, "coverage").expect("a coverage record");
        assert_eq!(coverage.get("gaps"), Some("2"));
        assert_eq!(coverage.get("gap"), Some("0.05"));
    }

    #[test]
    fn where_a_problem_is_is_said_to_the_millimetre() {
        // Four rectangles about a 0.02 m gap (the reviewer's case): the gap along
        // R's west edge, x = 50.02 from y = 0 to 40, is found at its middle,
        // (50.02, 20). GEOS works it out as 20.000000000000007, which the reply
        // printed; a coordinate is said to the millimetre, as length= is.
        let mut fixture = fixture("WhereAProblemIsIsSaidToTheMillimetre");
        fixture.rect((0.0, 0.0), (50.0, 40.0), "lots", PropertyMap::new());
        let right = fixture.rect((50.02, 0.0), (100.0, 40.0), "lots", PropertyMap::new());
        fixture.rect((0.0, 40.0), (100.0, 80.0), "lots", PropertyMap::new());
        fixture.rect((0.0, -40.0), (100.0, 0.0), "lots", PropertyMap::new());
        let reply = fixture.ok("GIS COVERAGE CHECK LAYERS lots gap=0.05");
        let mut found = false;
        for problem in problems(&reply) {
            let at = problem.get("at").unwrap_or("");
            lacks(at, "0000000");
            if problem.get("entity") == Some(format!("{right}").as_str()) {
                assert_eq!(at, "50.02,20", "{reply}");
                found = true;
            }
        }
        assert!(found, "{reply}");
    }

    #[test]
    fn clean_without_replace_is_refused() {
        let mut fixture = fixture("CleanWithoutReplaceIsRefused");
        gap_fixture(&mut fixture);
        let before = fixture.entity_count();
        let error = refused(
            fixture.run("GIS COVERAGE CLEAN LAYERS lots gap=0.05"),
            "a clean without REPLACE",
        );
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        holds(&error.message, "REPLACE");
        assert_eq!(fixture.entity_count(), before);
    }

    #[test]
    fn clean_closes_an_enclosed_gap_and_one_undo_restores_it() {
        // The four lots cover 100 x 120 = 12000 m2 less the gap's 0.02 x 40 =
        // 0.8 m2. Cleaned, one of L and R takes the gap: 12000 m2, no gap left.
        let mut fixture = fixture("CleanClosesAnEnclosedGapAndOneUndoRestoresIt");
        gap_fixture(&mut fixture);
        near(fixture.area_on("lots"), 12000.0 - 0.8, 1e-9);
        let reply = fixture.ok("GIS COVERAGE CLEAN LAYERS lots gap=0.05 REPLACE");
        near(fixture.area_on("lots"), 12000.0, 1e-9);
        let coverage = GeoFixture::record(&reply, "coverage").expect("a coverage record");
        assert_eq!(coverage.get("changed"), Some("1"), "{reply}");
        holds(&reply, "boundaries moved");
        assert!(problems(&fixture.ok("GIS COVERAGE CHECK LAYERS lots gap=0.05")).is_empty());
        assert_eq!(fixture.on("lots").len(), 4);
        fixture.type_line("UNDO");
        near(fixture.area_on("lots"), 12000.0 - 0.8, 1e-9);
        assert_eq!(
            problems(&fixture.ok("GIS COVERAGE CHECK LAYERS lots gap=0.05")).len(),
            2
        );
    }

    #[test]
    fn clean_refuses_a_drawing_changed_while_it_ran() {
        // Prepared, then lot L moves before the apply: writing the cleaned
        // boundaries now would undo the move, so the apply refuses and changes
        // nothing.
        let mut fixture = fixture("CleanRefusesADrawingChangedWhileItRan");
        gap_fixture(&mut fixture);
        let left = fixture.on("lots")[0].id;
        let prepared = must(fixture.prepare("GIS COVERAGE CLEAN LAYERS lots gap=0.05 REPLACE"));
        let work = prepared.work.expect("a run has work");
        let apply = must(work(None, None));
        assert!(
            fixture
                .document
                .execute(move_entities(vec![left], Vec2::new(0.0, 1.0)))
                .is_ok()
        );
        let moved = fixture.on("lots");
        let error = refused(
            apply(&mut fixture.context()),
            "an apply on a changed drawing",
        );
        assert_eq!(error.code, ErrorCode::InvalidState);
        assert_eq!(fixture.on("lots"), moved);
    }

    #[test]
    fn mixed_geometry_in_scope_is_filtered_and_counted() {
        // check-coverage refuses anything but areas: the line and the point are
        // left out, counted and said, and the lot is checked.
        let mut fixture = fixture("MixedGeometryInScopeIsFilteredAndCounted");
        fixture.rect((0.0, 0.0), (50.0, 40.0), "lots", PropertyMap::new());
        fixture.line((0.0, 50.0), (100.0, 50.0), "lots", PropertyMap::new());
        fixture.add(
            Geometry::Point(PointGeometry {
                position: Point2::new(5.0, 5.0),
            }),
            "lots",
            PropertyMap::new(),
        );
        let reply = fixture.ok("GIS COVERAGE CHECK LAYERS lots");
        let coverage = GeoFixture::record(&reply, "coverage").expect("a coverage record");
        assert_eq!(coverage.get("areas"), Some("1"));
        assert_eq!(coverage.get("ignored"), Some("2"));
        assert_eq!(coverage.get("problems"), Some("0"));
        holds(&reply, "are not areas");
    }

    #[test]
    fn coverage_markers_are_the_bad_edges() {
        let mut fixture = fixture("CoverageMarkersAreTheBadEdges");
        fixture.rect((0.0, 0.0), (50.0, 40.0), "lots", PropertyMap::new());
        fixture.rect((49.0, 0.0), (100.0, 40.0), "lots", PropertyMap::new());
        fixture.ok("GIS COVERAGE CHECK LAYERS lots markers=coverage");
        let markers = fixture.on("coverage");
        assert_eq!(markers.len(), 2);
        for marker in &markers {
            assert!(matches!(marker.geometry, Geometry::Polyline(_)));
            assert_eq!(text(marker, "gis.marker"), "coverage");
            assert_eq!(text(marker, "gis.problem"), "overlap");
        }
    }

    #[test]
    fn what_it_cannot_do_is_refused_by_name() {
        let mut fixture = fixture("WhatItCannotDoIsRefusedByName");
        fixture.rect((0.0, 0.0), (50.0, 40.0), "lots", PropertyMap::new());
        let mut refuse = |line: &str, code: ErrorCode| {
            let error = refused(fixture.run(line), line);
            assert_eq!(error.code, code, "{line}: {}", error.describe());
        };
        refuse("GIS REPAIR DRAWING method=glue", ErrorCode::InvalidArgument);
        refuse("GIS COVERAGE DRAWING", ErrorCode::InvalidArgument);
        refuse(
            "GIS COVERAGE CHECK DRAWING gap=-1",
            ErrorCode::InvalidArgument,
        );
        refuse(
            "GIS COVERAGE CHECK DRAWING snap=1",
            ErrorCode::InvalidArgument,
        ); // CLEAN's word
        refuse(
            "GIS COVERAGE CLEAN DRAWING markers=m REPLACE",
            ErrorCode::InvalidArgument,
        );
        refuse(
            "GIS CHECK DRAWING markers=/bad//layer",
            ErrorCode::InvalidArgument,
        );
    }
}

mod gis_check_session {
    use katana_app::Session;

    use crate::CliSession;
    use crate::geo_fixture::holds;

    #[test]
    fn the_verbs_run_through_the_session_and_help_names_them() {
        let mut session = CliSession::start();
        assert!(session.run("RECT 0,0 50,40"));
        assert!(session.run("GIS CHECK DRAWING"));
        assert!(session.run("GIS COVERAGE CHECK DRAWING"));
        assert!(!session.run("GIS COVERAGE CLEAN DRAWING"));
        let help = Session::help_text();
        for verb in ["GIS CHECK", "GIS REPAIR", "GIS COVERAGE CHECK"] {
            holds(&help, verb);
        }
    }
}
