//! tests/geo/test_hull_and_clip.cpp: GIS HULL and GIS CLIP
//! (src/katana_app/geo/hull_clip_verbs.cpp, docs/geoprocessing.md "V3"),
//! through the one executor, every expected value worked by hand beside it.
//! Its GisHullClipContract suite is step 5's (katana_gis).

use katana_entity::{Entity, Geometry};

/// A line's or a polyline's length; 0 for anything else.
fn length_of(entity: &Entity) -> f64 {
    match &entity.geometry {
        Geometry::Line(segment) => segment.length(),
        Geometry::Polyline(line) => line.length(),
        _ => 0.0,
    }
}

mod gis_hull {
    use katana_entity::{Geometry, PointGeometry, PropertyMap};
    use katana_geometry::Point2;

    use crate::fixture;
    use crate::geo_fixture::{GeoFixture, holds, near};

    fn point(fixture: &mut GeoFixture, x: f64, y: f64) {
        fixture.add(
            Geometry::Point(PointGeometry {
                position: Point2::new(x, y),
            }),
            "survey",
            PropertyMap::new(),
        );
    }

    #[test]
    fn convex_hull_of_the_corners_and_centre_of_a_square_is_the_square() {
        let mut fixture = fixture("ConvexHullOfTheCornersAndCentreOfASquareIsTheSquare");
        for (x, y) in [
            (0.0, 0.0),
            (10.0, 0.0),
            (10.0, 10.0),
            (0.0, 10.0),
            (5.0, 5.0),
        ] {
            point(&mut fixture, x, y);
        }
        let reply = fixture.ok("GIS HULL LAYERS survey");
        let hulls = fixture.on("gis/hull");
        assert_eq!(hulls.len(), 1);
        let Geometry::Polyline(ring) = &hulls[0].geometry else {
            panic!("the hull is not a polyline: {reply}");
        };
        assert_eq!(ring.vertices.len(), 4); // the centre is inside, not a corner
        near(ring.area(), 100.0, 1e-9);
        let hull = GeoFixture::record(&reply, "hull").expect("a hull record");
        assert_eq!(hull.get("kind"), Some("convex"));
        assert_eq!(hull.get("points"), Some("5"));
        assert_eq!(hull.get("area"), Some("100.000"));
    }

    #[test]
    fn concave_hull_of_an_l_shaped_point_set_is_smaller_than_its_convex_hull() {
        // The points of a 1 m grid over an L: the arm y = 0..2 for x = 0..10,
        // and the arm x = 0..2 for y = 3..10 - 57 points in an L of 10 x 2 +
        // 2 x 8 = 36 m2. Its convex hull cuts the corner from (10,2) to (2,10):
        // 100 - 8 x 8 / 2 = 68 m2. Tight (ratio 0.1), the hull follows the L
        // but for the half square it cuts across the inner corner, (2,3) to
        // (3,2): 36 + 0.5 = 36.5 m2.
        let mut fixture = fixture("ConcaveHullOfAnLShapedPointSetIsSmallerThanItsConvexHull");
        for x in 0..=10_i32 {
            for y in 0..=2_i32 {
                point(&mut fixture, f64::from(x), f64::from(y));
            }
        }
        for x in 0..=2_i32 {
            for y in 3..=10_i32 {
                point(&mut fixture, f64::from(x), f64::from(y));
            }
        }
        let convex = fixture.ok("GIS HULL LAYERS survey convex TO LAYER convex");
        let concave = fixture.ok("GIS HULL LAYERS survey concave=0.1 TO LAYER concave");
        let convex_hull = GeoFixture::record(&convex, "hull").expect("a hull record");
        assert_eq!(convex_hull.get("points"), Some("57"));
        near(fixture.area_on("convex"), 68.0, 1e-9);
        near(fixture.area_on("concave"), 36.5, 1e-9);
        let concave_hull = GeoFixture::record(&concave, "hull").expect("a hull record");
        assert_eq!(concave_hull.get("kind"), Some("concave"));
        assert_eq!(concave_hull.get("ratio"), Some("0.1"));
    }

    #[test]
    fn fewer_than_three_points_bound_no_area_and_say_so() {
        let mut fixture = fixture("FewerThanThreePointsBoundNoAreaAndSaySo");
        point(&mut fixture, 0.0, 0.0);
        point(&mut fixture, 10.0, 0.0);
        let reply = fixture.ok("GIS HULL LAYERS survey");
        assert!(fixture.on("gis/hull").is_empty());
        holds(&reply, "they bound no area");
    }
}

mod gis_clip {
    use katana_commands::move_entities;
    use katana_core::ErrorCode;
    use katana_entity::entity::property_to_string;
    use katana_entity::{PropertyMap, PropertyValue};
    use katana_geometry::Vec2;

    use super::length_of;
    use crate::geo_fixture::{GeoFixture, geo_data, must, near, refused, shared_data};
    use crate::{fixture, props};

    #[test]
    fn clip_of_a_line_by_a_box_has_hand_computed_length() {
        // A 100 m line along y = 0 through a box from x = 10 to 40: 30 m of it.
        let mut fixture = fixture("ClipOfALineByABoxHasHandComputedLength");
        fixture.line((0.0, 0.0), (100.0, 0.0), "pipes", PropertyMap::new());
        fixture.rect((10.0, -5.0), (40.0, 5.0), "site", PropertyMap::new());
        let reply = fixture.ok("GIS CLIP LAYERS pipes BY LAYERS site");
        let pieces = fixture.on("gis/clip");
        assert_eq!(pieces.len(), 1);
        near(length_of(&pieces[0]), 30.0, 1e-9);
        let clip = GeoFixture::record(&reply, "clip").expect("a clip record");
        assert_eq!(clip.get("length"), Some("30.000"));
        assert_eq!(fixture.on("pipes").len(), 1); // drawn beside, the pipe stays whole
    }

    #[test]
    fn clip_in_place_keeps_the_id() {
        let mut fixture = fixture("ClipInPlaceKeepsTheId");
        let pipe = fixture.line((0.0, 0.0), (100.0, 0.0), "pipes", PropertyMap::new());
        fixture.rect((10.0, -5.0), (40.0, 5.0), "site", PropertyMap::new());
        let reply = fixture.ok("GIS CLIP LAYERS pipes BY LAYERS site REPLACE");
        let kept = fixture
            .document
            .model()
            .entities
            .find(pipe)
            .cloned()
            .expect("the pipe keeps its id");
        near(length_of(&kept), 30.0, 1e-9);
        let output = GeoFixture::record(&reply, "output").expect("an output record");
        assert_eq!(output.get("target"), Some("in-place"));
        assert_eq!(output.get("updated"), Some("1"));
        assert!(fixture.on("gis/clip").is_empty());
    }

    #[test]
    fn a_part_split_into_two_keeps_the_id_on_the_larger_and_reports_the_other() {
        // Boxes over x = 10..40 (30 m) and 60..95 (35 m): the pipe keeps its id
        // on the 35 m piece, and the 30 m one is made beside it.
        let mut fixture = fixture("APartSplitIntoTwoKeepsTheIdOnTheLargerAndReportsTheOther");
        let pipe = fixture.line((0.0, 0.0), (100.0, 0.0), "pipes", PropertyMap::new());
        fixture.rect((10.0, -5.0), (40.0, 5.0), "site", PropertyMap::new());
        fixture.rect((60.0, -5.0), (95.0, 5.0), "site", PropertyMap::new());
        let reply = fixture.ok("GIS CLIP LAYERS pipes BY LAYERS site REPLACE");
        let pipes = fixture.on("pipes");
        assert_eq!(pipes.len(), 2);
        assert_eq!(pipes[0].id, pipe);
        near(length_of(&pipes[0]), 35.0, 1e-9);
        near(length_of(&pipes[1]), 30.0, 1e-9);
        assert_eq!(
            pipes[1]
                .properties
                .get("gis.source")
                .map(property_to_string),
            Some(format!("{pipe}"))
        );
        let split = GeoFixture::record(&reply, "split").expect("a split record");
        assert_eq!(split.get("entity"), Some(format!("{pipe}").as_str()));
        assert_eq!(split.get("parts"), Some("2"));
        assert_eq!(
            split.get("created"),
            Some(format!("{}", pipes[1].id).as_str())
        );
    }

    #[test]
    fn in_place_what_is_outside_goes_and_what_is_inside_stays_as_it_was_in_one_step() {
        let mut fixture = fixture("InPlaceWhatIsOutsideGoesAndWhatIsInsideStaysAsItWasInOneStep");
        let inside = fixture.line((15.0, 0.0), (35.0, 0.0), "pipes", PropertyMap::new());
        let outside = fixture.line((0.0, 50.0), (100.0, 50.0), "pipes", PropertyMap::new());
        fixture.rect((10.0, -5.0), (40.0, 5.0), "site", PropertyMap::new());
        let before = fixture
            .document
            .model()
            .entities
            .find(inside)
            .cloned()
            .expect("the inside pipe is drawn");
        let reply = fixture.ok("GIS CLIP LAYERS pipes BY LAYERS site REPLACE");
        assert_eq!(
            fixture.document.model().entities.find(inside),
            Some(&before)
        );
        assert!(!fixture.document.model().entities.contains(outside));
        let clip = GeoFixture::record(&reply, "clip").expect("a clip record");
        assert_eq!(clip.get("whole"), Some("1"));
        assert_eq!(clip.get("outside"), Some("1"));
        let output = GeoFixture::record(&reply, "output").expect("an output record");
        assert_eq!(output.get("deleted"), Some("1"));
        fixture.type_line("UNDO");
        assert!(fixture.document.model().entities.contains(outside));
    }

    #[test]
    fn an_area_wholly_outside_goes_with_its_holes_in_one_step() {
        // A lot with a courtyard cut out of it (an area and its hole are
        // entities of their own, joined by the gis.ring tag), far from the site:
        // clipped in place, the lot is outside, and so is its hole - left
        // behind, the hole would become an area of its own the next time the
        // layer is read.
        let mut fixture = fixture("AnAreaWhollyOutsideGoesWithItsHolesInOneStep");
        let lot = fixture.rect((100.0, 100.0), (200.0, 200.0), "lots", PropertyMap::new());
        let hole = fixture.rect(
            (140.0, 140.0),
            (160.0, 160.0),
            "lots",
            props([("gis.ring", PropertyValue::from("hole"))]),
        );
        let kept = fixture.rect((10.0, 10.0), (20.0, 20.0), "lots", PropertyMap::new());
        fixture.rect((0.0, 0.0), (50.0, 50.0), "site", PropertyMap::new());
        let reply = fixture.ok("GIS CLIP LAYERS lots BY LAYERS site REPLACE");
        assert!(!fixture.document.model().entities.contains(lot));
        assert!(!fixture.document.model().entities.contains(hole));
        assert!(fixture.document.model().entities.contains(kept));
        let clip = GeoFixture::record(&reply, "clip").expect("a clip record");
        assert_eq!(clip.get("outside"), Some("1"), "{reply}");
        let output = GeoFixture::record(&reply, "output").expect("an output record");
        assert_eq!(output.get("deleted"), Some("2"), "{reply}");
        fixture.type_line("UNDO");
        assert!(fixture.document.model().entities.contains(lot));
        assert!(fixture.document.model().entities.contains(hole));
    }

    #[test]
    fn an_in_place_clip_refuses_a_drawing_changed_while_it_ran() {
        // Prepared, then the pipe moves before the apply: cutting it now would
        // write over the move, so the apply refuses and changes nothing.
        let mut fixture = fixture("AnInPlaceClipRefusesADrawingChangedWhileItRan");
        let pipe = fixture.line((0.0, 0.0), (100.0, 0.0), "pipes", PropertyMap::new());
        fixture.rect((10.0, -5.0), (40.0, 5.0), "site", PropertyMap::new());
        let prepared = must(fixture.prepare("GIS CLIP LAYERS pipes BY LAYERS site REPLACE"));
        let work = prepared.work.expect("a run has work");
        let apply = must(work(None, None));
        assert!(
            fixture
                .document
                .execute(move_entities(vec![pipe], Vec2::new(1.0, 0.0)))
                .is_ok()
        );
        let moved = fixture
            .document
            .model()
            .entities
            .find(pipe)
            .cloned()
            .expect("the pipe is there");
        let error = refused(
            apply(&mut fixture.context()),
            "an apply on a changed drawing",
        );
        assert_eq!(error.code, ErrorCode::InvalidState);
        assert_eq!(fixture.document.model().entities.find(pipe), Some(&moved));
    }

    #[test]
    fn a_hole_moved_while_it_ran_is_not_deleted_with_its_area() {
        // The hole is deleted with its area, so an edit to the hole while the
        // job ran is a changed drawing too.
        let mut fixture = fixture("AHoleMovedWhileItRanIsNotDeletedWithItsArea");
        fixture.rect((100.0, 100.0), (200.0, 200.0), "lots", PropertyMap::new());
        let hole = fixture.rect(
            (140.0, 140.0),
            (160.0, 160.0),
            "lots",
            props([("gis.ring", PropertyValue::from("hole"))]),
        );
        fixture.rect((0.0, 0.0), (50.0, 50.0), "site", PropertyMap::new());
        let prepared = must(fixture.prepare("GIS CLIP LAYERS lots BY LAYERS site REPLACE"));
        let work = prepared.work.expect("a run has work");
        let apply = must(work(None, None));
        assert!(
            fixture
                .document
                .execute(move_entities(vec![hole], Vec2::new(1.0, 0.0)))
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

    #[test]
    fn a_file_boundary_works_like_a_drawn_one() {
        // The corridor of lots.geojson runs from x = -10 to 110: a line from
        // -20 to 120 along its middle keeps 120 m.
        let _shared = shared_data();
        let mut fixture = fixture("AFileBoundaryWorksLikeADrawnOne");
        fixture.line((-20.0, 20.0), (120.0, 20.0), "pipes", PropertyMap::new());
        let reply = fixture.ok(&format!(
            "GIS CLIP LAYERS pipes BY FILE \"{}/lots.geojson\" where=\"kind='corridor'\"",
            geo_data()
        ));
        let pieces = fixture.on("gis/clip");
        assert_eq!(pieces.len(), 1, "{reply}");
        near(length_of(&pieces[0]), 120.0, 1e-9);
    }

    #[test]
    fn what_it_cannot_do_is_refused_by_name() {
        let mut fixture = fixture("WhatItCannotDoIsRefusedByName");
        fixture.line((0.0, 0.0), (100.0, 0.0), "pipes", PropertyMap::new());
        fixture.rect((10.0, -5.0), (40.0, 5.0), "site", PropertyMap::new());
        let mut refuse = |line: &str| {
            let error = refused(fixture.run(line), line);
            assert_eq!(
                error.code,
                ErrorCode::InvalidArgument,
                "{line}: {}",
                error.describe()
            );
        };
        refuse("GIS CLIP LAYERS pipes");
        refuse("GIS CLIP LAYERS pipes BY");
        refuse("GIS CLIP LAYERS pipes BY LAYERS site REPLACE TO LAYER cut");
        refuse("GIS CLIP LAYERS pipes BY LAYERS site where=\"a=1\"");
        refuse("GIS HULL DRAWING concave=2");
        refuse("GIS HULL DRAWING holes");
        refuse("GIS HULL DRAWING convex concave=0.5");
    }
}
