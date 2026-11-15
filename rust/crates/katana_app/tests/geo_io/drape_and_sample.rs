//! tests/geo/test_drape_and_sample.cpp: RASTER SAMPLE and DRAPE
//! (docs/terrain.md, "Sampling and drape") - the ground's height at points,
//! reported, and given to the drawing's points and vertices in one undo step;
//! off the ground, no height, never 0. Its RasterSampler and Drape suites are
//! the lower crates' (katana_gis, katana_cad).
//!
//! Fixture: tests/geo/data/plane.asc, 40 x 30 cells of 1 m from (0,0), with
//! z = 100 + 0.05 x at the cell centres. Bilinear interpolation of a linear function
//! is the function itself, so between any four cell centres the sample is
//! exactly the plane: z(x, y) = 100 + 0.05 x. The file is read as Float32,
//! each height within 3.815e-6 of its value near 100 (half an ulp, 2^-17), and
//! a bilinear sample is a convex combination of four of them: within 3.815e-6
//! as well. 1e-5 allows that.

mod drape_and_sample {
    use std::sync::Arc;

    use katana_app::geo::prepare;
    use katana_app::geo::replies::{Record, parse_records};
    use katana_cad::Document;
    use katana_commands::{create_entities, move_entities};
    use katana_core::ErrorCode;
    use katana_core::text::parse_finite_double;
    use katana_entity::entity::{INVALID_ENTITY_ID, heights_of};
    use katana_entity::{Entity, EntityId, Geometry, PointGeometry};
    use katana_geometry::{Circle2, Point2, Point3, Polyline2, Segment2, Vec2};
    use katana_terrain::{NamedSurface, TinSurface};

    use crate::geo_fixture::{GeoFixture, geo_data, holds, must, near, refused, shared_data};

    const FLOAT32: f64 = 1e-5;

    fn plane_file() -> String {
        format!("\"{}/plane.asc\"", geo_data())
    }

    fn plane(x: f64) -> f64 {
        100.0 + 0.05 * x
    }

    /// The C++ fixture: its scratch folder named after the test.
    fn fixture(name: &str) -> GeoFixture {
        GeoFixture::new("katana-drape-sample-", name)
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

    fn number_in(record: &Record, key: &str) -> Option<f64> {
        record.get(key).and_then(parse_finite_double)
    }

    fn draw(fixture: &mut GeoFixture, entity: Entity) -> EntityId {
        assert!(
            fixture
                .document
                .execute(create_entities(vec![entity]))
                .is_ok()
        );
        fixture
            .document
            .last_created_entities()
            .first()
            .copied()
            .unwrap_or(INVALID_ENTITY_ID)
    }

    fn drawn(geometry: Geometry) -> Entity {
        Entity {
            geometry,
            ..Entity::default()
        }
    }

    fn polyline(fixture: &mut GeoFixture, vertices: &[(f64, f64)]) -> EntityId {
        let line = Polyline2 {
            vertices: vertices.iter().map(|&(x, y)| Point2::new(x, y)).collect(),
            closed: false,
        };
        draw(fixture, drawn(Geometry::Polyline(line)))
    }

    fn point(fixture: &mut GeoFixture, x: f64, y: f64) -> EntityId {
        draw(
            fixture,
            drawn(Geometry::Point(PointGeometry {
                position: Point2::new(x, y),
            })),
        )
    }

    fn heights(fixture: &GeoFixture, id: EntityId, count: usize) -> Vec<Option<f64>> {
        fixture
            .document
            .model()
            .entities
            .find(id)
            .map(|entity| heights_of(&entity.properties, count))
            .unwrap_or_default()
    }

    /// Two triangles folded along the diagonal from (0,0) to (10,10):
    /// below it (y < x), through (0,0,10) (10,0,10) (10,10,20): z = 10 + y;
    /// above it (y > x), through (0,0,10) (10,10,20) (0,10,10): z = 10 + x.
    fn add_fold(fixture: &mut GeoFixture) {
        let tin = must(TinSurface::create(
            vec![
                Point3::new(0.0, 0.0, 10.0),
                Point3::new(10.0, 0.0, 10.0),
                Point3::new(10.0, 10.0, 20.0),
                Point3::new(0.0, 10.0, 10.0),
            ],
            vec![[0, 1, 2], [0, 2, 3]],
            Vec::new(),
        ));
        // Into the drawing's surfaces, as SURFACE FROM would put it.
        let mut next = fixture.surfaces().clone();
        must(next.add(NamedSurface {
            name: String::from("fold"),
            surface: Some(Arc::new(tin)),
            source: String::from("test"),
            ..NamedSurface::default()
        }));
        must(fixture.document.set_surfaces(next, Document::SET_SURFACES));
    }

    // ---- RASTER SAMPLE ----

    #[test]
    fn sample_on_a_plane_is_exact_with_bilinear() {
        let _shared = shared_data();
        let mut fixture = fixture("SampleOnAPlaneIsExactWithBilinear");
        let reply = ran(
            &mut fixture,
            &format!(
                "RASTER SAMPLE FILE {} AT 12.3,7.7 AT 30.05,20.5",
                plane_file()
            ),
        );
        let samples = records(&reply, "sample");
        assert_eq!(samples.len(), 2, "{reply}");
        // 100 + 0.05 x 12.3 = 100.615; 100 + 0.05 x 30.05 = 101.5025.
        assert_eq!(samples[0].get("at").unwrap_or(""), "12.3,7.7");
        near(number_in(&samples[0], "z").unwrap_or(0.0), 100.615, FLOAT32);
        near(
            number_in(&samples[1], "z").unwrap_or(0.0),
            101.5025,
            FLOAT32,
        );
        holds(&reply, "samples method=bilinear count=2 on=2 off=0");
        assert_eq!(fixture.document.history().undo_count(), 0);
    }

    #[test]
    fn a_point_off_the_data_has_no_height_not_zero() {
        let _shared = shared_data();
        let mut fixture = fixture("APointOffTheDataHasNoHeightNotZero");
        let reply = ran(
            &mut fixture,
            &format!("RASTER SAMPLE FILE {} AT 50,10 AT 10,10", plane_file()),
        );
        let samples = records(&reply, "sample");
        assert_eq!(samples.len(), 2, "{reply}");
        assert!(samples[0].get("z").is_none(), "{reply}");
        assert_eq!(samples[0].get("ground").unwrap_or(""), "no");
        near(
            number_in(&samples[1], "z").unwrap_or(0.0),
            plane(10.0),
            FLOAT32,
        );
        holds(&reply, "count=2 on=1 off=1");
        assert!(!records(&reply, "warning").is_empty());
    }

    #[test]
    fn sample_reads_the_scopes_points_and_says_what_else_it_took() {
        let _shared = shared_data();
        let mut fixture = fixture("SampleReadsTheScopesPointsAndSaysWhatElseItTook");
        let id = point(&mut fixture, 20.0, 5.0);
        polyline(&mut fixture, &[(1.0, 1.0), (2.0, 2.0)]);
        let reply = ran(
            &mut fixture,
            &format!("RASTER SAMPLE FILE {} DRAWING", plane_file()),
        );
        let samples = records(&reply, "sample");
        assert_eq!(samples.len(), 1, "{reply}");
        assert_eq!(samples[0].get("entity").unwrap_or(""), format!("{id}"));
        near(
            number_in(&samples[0], "z").unwrap_or(0.0),
            plane(20.0),
            FLOAT32,
        );
        let scope = records(&reply, "scope");
        assert_eq!(scope.len(), 1);
        assert_eq!(scope[0].get("points").unwrap_or(""), "1");
        assert_eq!(scope[0].get("skipped.polyline").unwrap_or(""), "1");
    }

    // ---- DRAPE ----

    #[test]
    fn drape_of_a_polyline_sets_every_vertex_height() {
        let _shared = shared_data();
        let mut fixture = fixture("DrapeOfAPolylineSetsEveryVertexHeight");
        // 100 + 0.05 x: 5.5 -> 100.275, 20.25 -> 101.0125, 35 -> 101.75.
        let line = polyline(&mut fixture, &[(5.5, 5.0), (20.25, 10.0), (35.0, 25.0)]);
        let before = fixture.document.history().undo_count();
        let reply = ran(
            &mut fixture,
            &format!("DRAPE FILE {} DRAWING", plane_file()),
        );
        assert_eq!(
            fixture.document.history().undo_count(),
            before + 1,
            "{reply}"
        );
        let z = heights(&fixture, line, 3);
        assert_eq!(z.len(), 3);
        assert!(z.iter().all(Option::is_some), "{reply}");
        near(z[0].unwrap_or_default(), 100.275, FLOAT32);
        near(z[1].unwrap_or_default(), 101.0125, FLOAT32);
        near(z[2].unwrap_or_default(), 101.75, FLOAT32);
        holds(&reply, "drape method=bilinear entities=1 vertices=3 off=0");
        must(fixture.document.undo());
        assert!(heights(&fixture, line, 3)[0].is_none());
    }

    #[test]
    fn drape_from_a_surface_uses_the_tin_not_a_grid() {
        // On the fold, the point (5.2, 5) is below the diagonal: z = 10 + 5 =
        // 15 exactly. A grid of the surface at 1 m, sampled bilinearly, would
        // not give it: its centres (4.5,4.5) 14.5, (5.5,4.5) 14.5, (4.5,5.5)
        // 14.5 and (5.5,5.5) 15.5 give 14.5 + 0.7 x 0.5 x 1 = 14.85. The line
        // from (2,6) (above: 10 + 2 = 12) to (8,3) (below: 10 + 3 = 13) is
        // exact too. The TIN's barycentric arithmetic is exact to rounding:
        // 1e-9.
        let mut fixture = fixture("DrapeFromASurfaceUsesTheTinNotAGrid");
        add_fold(&mut fixture);
        let mark = point(&mut fixture, 5.2, 5.0);
        let segment = Segment2::new(Point2::new(2.0, 6.0), Point2::new(8.0, 3.0));
        let line = draw(&mut fixture, drawn(Geometry::Line(segment)));
        let reply = ran(&mut fixture, "DRAPE SURFACE fold DRAWING");
        holds(
            &reply,
            "input arg=input source=surface name=fold read=triangles",
        );
        holds(&reply, "drape method=triangles");
        near(heights(&fixture, mark, 1)[0].unwrap_or(0.0), 15.0, 1e-9);
        let ends = heights(&fixture, line, 2);
        near(ends[0].unwrap_or(0.0), 12.0, 1e-9);
        near(ends[1].unwrap_or(0.0), 13.0, 1e-9);
    }

    #[test]
    fn a_vertex_off_the_ground_is_left_without_a_height_and_counted() {
        let _shared = shared_data();
        let mut fixture = fixture("AVertexOffTheGroundIsLeftWithoutAHeightAndCounted");
        // plane.asc ends at x = 40: the second vertex has no ground under it.
        let line = polyline(&mut fixture, &[(35.0, 10.0), (45.0, 10.0)]);
        let reply = ran(
            &mut fixture,
            &format!("DRAPE FILE {} DRAWING", plane_file()),
        );
        let z = heights(&fixture, line, 2);
        assert_eq!(z.len(), 2);
        near(z[0].unwrap_or(0.0), plane(35.0), FLOAT32);
        assert!(z[1].is_none());
        holds(&reply, "vertices=2 off=1 entities_off=0");
        assert!(!records(&reply, "warning").is_empty());
    }

    #[test]
    fn a_second_drape_on_the_same_ground_changes_nothing() {
        let _shared = shared_data();
        let mut fixture = fixture("ASecondDrapeOnTheSameGroundChangesNothing");
        polyline(&mut fixture, &[(5.0, 5.0), (15.0, 5.0)]);
        ran(
            &mut fixture,
            &format!("DRAPE FILE {} DRAWING", plane_file()),
        );
        let after = fixture.document.history().undo_count();
        let reply = ran(
            &mut fixture,
            &format!("DRAPE FILE {} DRAWING", plane_file()),
        );
        assert_eq!(fixture.document.history().undo_count(), after, "{reply}");
        holds(&reply, "updated=0");
        holds(&reply, "unchanged=1");
    }

    #[test]
    fn what_has_no_vertices_is_counted_and_left() {
        let _shared = shared_data();
        let mut fixture = fixture("WhatHasNoVerticesIsCountedAndLeft");
        let ring = Circle2::new(Point2::new(10.0, 10.0), 3.0);
        let circle = draw(&mut fixture, drawn(Geometry::Circle(ring)));
        polyline(&mut fixture, &[(5.0, 5.0), (15.0, 5.0)]);
        let reply = ran(
            &mut fixture,
            &format!("DRAPE FILE {} DRAWING", plane_file()),
        );
        holds(&reply, "used=1 skipped.circle=1");
        assert!(
            fixture
                .document
                .model()
                .entities
                .find(circle)
                .is_some_and(|entity| entity.properties.is_empty())
        );
    }

    #[test]
    fn an_entity_edited_while_the_job_ran_is_not_written_over() {
        let _shared = shared_data();
        let mut fixture = fixture("AnEntityEditedWhileTheJobRanIsNotWrittenOver");
        let line = polyline(&mut fixture, &[(5.0, 5.0), (15.0, 5.0)]);
        let prepared = must(prepare(
            &mut fixture.context(),
            &format!("DRAPE FILE {} DRAWING", plane_file()),
        ));
        let work = prepared.work.expect("a drape has work");
        let apply = must(work(None, None));
        must(
            fixture
                .document
                .execute(move_entities(vec![line], Vec2::new(0.0, 2.0))),
        );
        let error = refused(
            apply(&mut fixture.context()),
            "an apply after the entity moved",
        );
        assert_eq!(error.code, ErrorCode::InvalidState);
        assert!(heights(&fixture, line, 2)[0].is_none());
    }

    #[test]
    fn what_the_verbs_cannot_do_is_refused_naming_it() {
        let _shared = shared_data();
        let mut fixture = fixture("WhatTheVerbsCannotDoIsRefusedNamingIt");
        add_fold(&mut fixture);
        polyline(&mut fixture, &[(5.0, 5.0), (15.0, 5.0)]);
        for line in [
            String::from("DRAPE SURFACE fold DRAWING method=cubic"),
            String::from("DRAPE SURFACE fold CELL 1 DRAWING"),
            String::from("DRAPE DRAWING"),
            format!("DRAPE FILE {} DRAWING method=lanczos", plane_file()),
            format!("RASTER SAMPLE FILE {} AT 10", plane_file()),
            format!("RASTER SAMPLE FILE {} AT", plane_file()),
            String::from("RASTER SAMPLE SURFACE nothing AT 1,1"),
        ] {
            let error = refused(fixture.run(&line), &line);
            assert!(
                error.code == ErrorCode::InvalidArgument || error.code == ErrorCode::NotFound,
                "{line}: {}",
                error.describe()
            );
        }
    }
}
