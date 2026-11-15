//! tests/geo/test_contour_verb.cpp: the CONTOUR verb (contour_verbs.rs,
//! docs/terrain.md "Contours"): contour lines of a surface traced exactly, of
//! a raster by GDAL, drawn as polylines on <layer>/major and <layer>/minor at
//! their levels, in one undo step, and kept inside the closed shapes a scope
//! takes. Its ContourContract and ContourEntities suites are step 5's
//! (katana_gis, katana_interop).
//!
//! The fixture is tests/geo/data/plane.asc: 40 x 30 cells of 1 m from (0,0),
//! z = 100 + 0.05 x at the cell centres, so the level L lies at x = (L - 100)
//! / 0.05: 100.5 at x = 10, 101.0 at 20, 101.5 at 30. Its heights run from
//! 100.025 to 101.975 (the extreme centres, x = 0.5 and 39.5), so those three
//! are the only half-metre levels in it.

use katana_entity::entity::{ELEVATION_PROPERTY, set_heights};
use katana_entity::{Entity, Geometry, PointGeometry, PropertyMap, PropertyValue};
use katana_geometry::{Point2, Polyline2};

use crate::geo_fixture::GeoFixture;
use crate::surface_support::double_eq;

/// GDAL's AAIGrid driver reads plane.asc's decimals as Float32, so each height
/// is within half a Float32 step of the value written: 2^(6-23) / 2 =
/// 3.815e-6 near 100. A contour is placed by linear interpolation between two
/// centres 1 m apart on a gradient of 0.05 per metre, so a height error of e
/// moves it e / 0.05 along x: at most 3.815e-6 / 0.05 = 7.63e-5 m. (GDAL gave
/// x = 9.99998 for 10.) The TIN engine on a surface made from those heights
/// has the same bound, from the same heights.
const FLOAT32_PLANE_TOLERANCE: f64 = 7.62939453125e-6 / 2.0 / 0.05;

/// The C++ fixture's: its scratch folder named after the test.
fn fixture(name: &str) -> GeoFixture {
    GeoFixture::new("katana-contour-verb-", name)
}

fn levelled(x: f64, y: f64, z: f64) -> Entity {
    let mut properties = PropertyMap::default();
    set_heights(&mut properties, &[Some(z)]);
    Entity {
        geometry: Geometry::Point(PointGeometry {
            position: Point2::new(x, y),
        }),
        properties,
        ..Entity::default()
    }
}

fn polyline(vertices: &[(f64, f64)], closed: bool) -> Entity {
    Entity {
        geometry: Geometry::Polyline(Polyline2 {
            vertices: vertices.iter().map(|&(x, y)| Point2::new(x, y)).collect(),
            closed,
        }),
        ..Entity::default()
    }
}

#[track_caller]
fn draw(fixture: &mut GeoFixture, entities: Vec<Entity>) {
    assert!(
        fixture
            .document
            .execute(katana_commands::create_entities(entities))
            .is_ok()
    );
}

/// The contours on `layer`, in id order.
fn on(fixture: &GeoFixture, layer: &str) -> Vec<Entity> {
    let entities = &fixture.document.model().entities;
    entities
        .ids_on_layer(layer)
        .into_iter()
        .filter_map(|id| entities.find(id).cloned())
        .collect()
}

#[track_caller]
fn line_of(entity: &Entity) -> &Polyline2 {
    match &entity.geometry {
        Geometry::Polyline(line) => line,
        _ => panic!("a contour is a polyline"),
    }
}

#[track_caller]
fn level_of(entity: &Entity) -> f64 {
    match entity.properties.get(ELEVATION_PROPERTY) {
        Some(PropertyValue::Real(level)) => *level,
        found => panic!("a contour's level is a real number, not {found:?}"),
    }
}

/// Every vertex of `entity` within `tolerance` of x = (level - 100) / 0.05.
#[track_caller]
fn expect_on_the_plane(entity: &Entity, tolerance: f64) {
    let level = level_of(entity);
    let x = (level - 100.0) / 0.05;
    for vertex in &line_of(entity).vertices {
        assert!(
            (vertex.x - x).abs() <= tolerance,
            "level {level}: x={:?}, expected={x:?}, tolerance={tolerance:?}",
            vertex.x
        );
    }
}

/// The plane z = 100 + 0.05 x made from four levelled corners, which the TIN
/// reproduces exactly (0.05 x 40 = 2 is exact in binary).
#[track_caller]
fn exact_plane(fixture: &mut GeoFixture) {
    draw(
        fixture,
        vec![
            levelled(0.0, 0.0, 100.0),
            levelled(40.0, 0.0, 102.0),
            levelled(40.0, 30.0, 102.0),
            levelled(0.0, 30.0, 100.0),
        ],
    );
    fixture.ok("SURFACE FROM DRAWING NAME exact");
    let ids = fixture.document.model().entities.ids();
    assert!(
        fixture
            .document
            .execute(katana_commands::delete_entities(ids))
            .is_ok()
    );
}

// ---- the two engines --------------------------------------------------------------------------

mod contour_verb {
    use std::sync::atomic::AtomicBool;

    use katana_app::geo::{handles, help_text};
    use katana_core::ErrorCode;
    use katana_entity::entity::heights_of;

    use super::{
        FLOAT32_PLANE_TOLERANCE, double_eq, draw, exact_plane, expect_on_the_plane, fixture,
        level_of, line_of, on, polyline,
    };
    use crate::geo_fixture::{holds, must, near, refused, shared_data};
    use crate::surface_support::{code, get, plane, record_of};

    #[test]
    fn the_executor_takes_contour_and_help_names_it() {
        assert!(handles("CONTOUR SURFACE ground interval=1"));
        assert!(handles("contour raster 1 interval=1"));
        holds(&help_text(), "CONTOUR SURFACE <name>");
    }

    #[test]
    fn the_raster_engine_gives_the_levels_at_ten_twenty_and_thirty() {
        let _shared = shared_data();
        let mut fixture = fixture("TheRasterEngineGivesTheLevelsAtTenTwentyAndThirty");
        let reply = fixture.ok(&format!("CONTOUR FILE \"{}\" interval=0.5", plane()));
        let contours = record_of(&reply, "contours");
        assert_eq!(get(&contours, "method"), "grid");
        assert_eq!(get(&contours, "cell"), "1");
        assert_eq!(get(&contours, "levels"), "3");
        assert_eq!(get(&contours, "count"), "3");
        assert_eq!(get(&contours, "smoothed"), "no");
        // base 0, major every 5: k = 201, 202, 203 are none of them multiples.
        let minor = on(&fixture, "terrain/contours/minor");
        assert_eq!(minor.len(), 3);
        assert!(on(&fixture, "terrain/contours/major").is_empty());
        double_eq(level_of(&minor[0]), 100.5);
        double_eq(level_of(&minor[1]), 101.0);
        double_eq(level_of(&minor[2]), 101.5);
        for contour in &minor {
            expect_on_the_plane(contour, FLOAT32_PLANE_TOLERANCE);
            // GDAL carries a line to the raster's edge, y = 0 to 30.
            near(line_of(contour).length(), 30.0, 1e-9);
            assert!(!line_of(contour).closed);
        }
    }

    #[test]
    fn the_tin_engine_is_exact_on_an_exact_plane() {
        let mut fixture = fixture("TheTinEngineIsExactOnAnExactPlane");
        exact_plane(&mut fixture);
        // base 100.25: the levels 100.25, 100.75, 101.25 and 101.75 lie at
        // x = 5, 15, 25 and 35, none on a vertex. Interpolating exact heights
        // leaves only the rounding of a few operations near 100: far under
        // 1e-9.
        let reply = fixture.ok("CONTOUR SURFACE exact interval=0.5 base=100.25");
        let contours = record_of(&reply, "contours");
        assert_eq!(get(&contours, "method"), "tin");
        assert_eq!(contours.get("cell").unwrap_or("x"), "");
        let minor = on(&fixture, "terrain/contours/minor");
        let major = on(&fixture, "terrain/contours/major");
        assert_eq!(minor.len() + major.len(), 4, "{reply}");
        for contour in minor.iter().chain(major.iter()) {
            expect_on_the_plane(contour, 1e-9);
            // The surface spans y = 0 to 30, so does each contour.
            near(line_of(contour).length(), 30.0, 1e-9);
        }
    }

    #[test]
    fn the_tin_engine_on_the_rasters_surface_has_the_float32_bound() {
        let _shared = shared_data();
        let mut fixture = fixture("TheTinEngineOnTheRastersSurfaceHasTheFloat32Bound");
        fixture.ok(&format!("SURFACE FROM FILE \"{}\" NAME plane", plane()));
        fixture.ok("CONTOUR SURFACE plane interval=0.5");
        let minor = on(&fixture, "terrain/contours/minor");
        assert_eq!(minor.len(), 3);
        for contour in &minor {
            expect_on_the_plane(contour, FLOAT32_PLANE_TOLERANCE);
            // The TIN's vertices are the cell centres, y = 0.5 to 29.5.
            near(line_of(contour).length(), 29.0, 1e-9);
        }
    }

    #[test]
    fn major_every_two_from_base_hundred_makes_the_metre_major() {
        // Levels 100 + k x 0.5 for k = 1, 2, 3: only k = 2 (101.0) is a
        // multiple of 2.
        let _shared = shared_data();
        let mut fixture = fixture("MajorEveryTwoFromBaseHundredMakesTheMetreMajor");
        let reply = fixture.ok(&format!(
            "CONTOUR FILE \"{}\" interval=0.5 major=2 base=100 layer=site/levels",
            plane()
        ));
        let contours = record_of(&reply, "contours");
        assert_eq!(get(&contours, "major"), "1");
        assert_eq!(get(&contours, "minor"), "2");
        assert_eq!(get(&contours, "layer"), "site/levels");
        let major = on(&fixture, "site/levels/major");
        assert_eq!(major.len(), 1);
        double_eq(level_of(&major[0]), 101.0);
        let minor = on(&fixture, "site/levels/minor");
        assert_eq!(minor.len(), 2);
        double_eq(level_of(&minor[0]), 100.5);
        double_eq(level_of(&minor[1]), 101.5);
        assert!(
            fixture
                .document
                .model()
                .layers
                .contains("site/levels/major")
        );
    }

    #[test]
    fn every_vertex_carries_its_contours_level_as_its_height() {
        let _shared = shared_data();
        let mut fixture = fixture("EveryVertexCarriesItsContoursLevelAsItsHeight");
        fixture.ok(&format!("CONTOUR FILE \"{}\" interval=0.5", plane()));
        for contour in on(&fixture, "terrain/contours/minor") {
            let count = line_of(&contour).vertices.len();
            let heights = heights_of(&contour.properties, count);
            assert_eq!(heights.len(), count);
            for height in heights {
                let height = height.expect("every vertex of a contour has a height");
                double_eq(height, level_of(&contour));
            }
        }
    }

    #[test]
    fn one_undo_removes_every_contour() {
        let _shared = shared_data();
        let mut fixture = fixture("OneUndoRemovesEveryContour");
        fixture.ok(&format!(
            "CONTOUR FILE \"{}\" interval=0.25 major=2 base=100",
            plane()
        ));
        assert!(fixture.document.model().entities.size() > 3);
        must(fixture.document.undo());
        assert_eq!(fixture.document.model().entities.size(), 0);
    }

    // ---- boundaries from the scope -------------------------------------------------------------

    #[test]
    fn an_option_may_follow_the_filter_but_layer_there_is_the_filters() {
        // Read as every GIS verb's words are: interval= after the filter is
        // the verb's own (it was refused as "not a WHERE key"), while LAYER=
        // after WHERE is the filter's condition - what the scope widget
        // writes there - so the contours go to the default layer, inside the
        // box on layer 0.
        let _shared = shared_data();
        let mut fixture = fixture("AnOptionMayFollowTheFilterButLayerThereIsTheFilters");
        draw(
            &mut fixture,
            vec![polyline(
                &[(0.0, 0.0), (25.0, 0.0), (25.0, 15.0), (0.0, 15.0)],
                true,
            )],
        );
        let reply = fixture.ok(&format!(
            "CONTOUR FILE \"{}\" DRAWING WHERE TYPE=polyline interval=0.5 layer=0",
            plane()
        ));
        let scope = record_of(&reply, "scope");
        assert_eq!(get(&scope, "where"), "TYPE=polyline LAYER=0", "{reply}");
        // 100.5 at x = 10 and 101.0 at x = 20 cross the box, as in the test
        // below.
        assert_eq!(on(&fixture, "terrain/contours/minor").len(), 2, "{reply}");
    }

    #[test]
    fn a_closed_boundary_keeps_only_what_is_inside_it_on_both_engines() {
        // A 25 x 15 box from the origin: 100.5 (x = 10) and 101.0 (x = 20)
        // cross it from y = 0 to its top at 15; 101.5 (x = 30) lies outside.
        // The open line beside it bounds nothing and is counted.
        let _shared = shared_data();
        let mut fixture = fixture("AClosedBoundaryKeepsOnlyWhatIsInsideItOnBothEngines");
        draw(
            &mut fixture,
            vec![
                polyline(&[(0.0, 0.0), (25.0, 0.0), (25.0, 15.0), (0.0, 15.0)], true),
                polyline(&[(30.0, 0.0), (35.0, 5.0)], false),
            ],
        );
        let grid = fixture.ok(&format!(
            "CONTOUR FILE \"{}\" interval=0.5 layer=grid DRAWING WHERE TYPE=polyline",
            plane()
        ));
        let areas = record_of(&grid, "areas");
        assert_eq!(get(&areas, "used"), "1");
        assert_eq!(get(&areas, "skipped.open"), "1");
        holds(&grid, "warning text=");
        let cut = on(&fixture, "grid/minor");
        assert_eq!(cut.len(), 2, "{grid}");
        for contour in &cut {
            expect_on_the_plane(contour, FLOAT32_PLANE_TOLERANCE);
            // From the raster's edge at y = 0 to the boundary at 15.
            near(line_of(contour).length(), 15.0, 1e-9);
        }
        double_eq(level_of(&cut[0]), 100.5);
        double_eq(level_of(&cut[1]), 101.0);

        fixture.ok(&format!("SURFACE FROM FILE \"{}\" NAME plane", plane()));
        fixture.ok("CONTOUR SURFACE plane interval=0.5 layer=tin LAYERS 0");
        let tin = on(&fixture, "tin/minor");
        assert_eq!(tin.len(), 2);
        for contour in &tin {
            // From the first cell centre at y = 0.5 to the boundary at 15.
            near(line_of(contour).length(), 14.5, 1e-9);
        }
    }

    #[test]
    fn a_scope_with_no_closed_shape_keeps_no_contour_and_says_so() {
        let mut fixture = fixture("AScopeWithNoClosedShapeKeepsNoContourAndSaysSo");
        draw(
            &mut fixture,
            vec![polyline(&[(0.0, 0.0), (40.0, 30.0)], false)],
        );
        let reply = fixture.ok(&format!(
            "CONTOUR FILE \"{}\" interval=0.5 DRAWING",
            plane()
        ));
        let areas = record_of(&reply, "areas");
        assert_eq!(get(&areas, "used"), "0");
        assert_eq!(get(&areas, "skipped.open"), "1");
        assert_eq!(get(&record_of(&reply, "contours"), "count"), "0");
        assert_eq!(fixture.document.model().entities.size(), 1);
    }

    #[test]
    fn smoothing_a_plane_leaves_its_contours_where_they_were() {
        // A gaussian kernel is symmetric and sums to one, so it gives back a
        // linear surface unchanged away from the edges: the levels at x = 10,
        // 20 and 30 are 10 cells from either edge, beyond the 5-cell kernel.
        let _shared = shared_data();
        let mut fixture = fixture("SmoothingAPlaneLeavesItsContoursWhereTheyWere");
        let reply = fixture.ok(&format!(
            "CONTOUR FILE \"{}\" interval=0.5 smooth=5",
            plane()
        ));
        assert_eq!(
            get(&record_of(&reply, "contours"), "smoothed"),
            "yes",
            "{reply}"
        );
        let minor = on(&fixture, "terrain/contours/minor");
        assert_eq!(minor.len(), 3);
        for contour in &minor {
            expect_on_the_plane(contour, FLOAT32_PLANE_TOLERANCE);
        }
    }

    #[test]
    fn preview_says_what_would_be_read_and_draws_nothing() {
        let mut fixture = fixture("PreviewSaysWhatWouldBeReadAndDrawsNothing");
        let reply = fixture.ok(&format!(
            "CONTOUR FILE \"{}\" interval=0.5 PREVIEW",
            plane()
        ));
        holds(&reply, "preview valid=yes changed=no");
        assert_eq!(fixture.document.model().entities.size(), 0);
    }

    #[test]
    fn a_cancelled_run_draws_nothing() {
        let _shared = shared_data();
        let mut fixture = fixture("ACancelledRunDrawsNothing");
        let prepared = must(fixture.prepare(&format!("CONTOUR FILE \"{}\" interval=0.5", plane())));
        let work = prepared.work.expect("a CONTOUR of a file has work to run");
        let stop = AtomicBool::new(true);
        let error = refused(work(Some(&stop), None), "a cancelled run");
        assert_eq!(error.code, ErrorCode::InvalidState);
        assert_eq!(error.message, "cancelled");
        assert_eq!(fixture.document.model().entities.size(), 0);
    }

    #[test]
    fn what_the_verb_cannot_do_is_refused_naming_it() {
        let _shared = shared_data();
        let mut fixture = fixture("WhatTheVerbCannotDoIsRefusedNamingIt");
        fixture.ok(&format!("SURFACE FROM FILE \"{}\" NAME plane", plane()));
        let file = format!("FILE \"{}\"", plane());
        assert_eq!(
            code(fixture.run(&format!("CONTOUR {file}"))),
            ErrorCode::InvalidArgument
        ); // no interval
        assert_eq!(
            code(fixture.run(&format!("CONTOUR {file} interval=0"))),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            code(fixture.run(&format!("CONTOUR {file} interval=1 smooth=4"))),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            code(fixture.run(&format!("CONTOUR {file} interval=1 layer=/bad"))),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            code(fixture.run(&format!("CONTOUR {file} interval=1 colour=red"))),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            code(fixture.run("CONTOUR DRAWING interval=1")),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            code(fixture.run("CONTOUR SURFACE nothing interval=1")),
            ErrorCode::NotFound
        );
        assert_eq!(
            code(fixture.run("CONTOUR SURFACE plane interval=1 smooth=3")),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            code(fixture.run("CONTOUR SURFACE plane CELL 2 interval=1")),
            ErrorCode::InvalidArgument
        );
        // A millionth of a metre over 1.95 m of relief is 1.95 million
        // levels: a unit mistake, refused by both engines before a line is
        // drawn.
        assert_eq!(
            code(fixture.run("CONTOUR SURFACE plane interval=0.000001")),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            code(fixture.run(&format!("CONTOUR {file} interval=0.000001"))),
            ErrorCode::InvalidArgument
        );
        assert_eq!(fixture.document.model().entities.size(), 0);
    }
}

// ---- through a Session, as katana_cli and katana_mcp run it -------------------------------------

mod contour_session {
    use katana_app::{Captured, Session};

    use crate::geo_fixture::{holds, shared_data};
    use crate::surface_support::plane;

    #[test]
    fn the_session_draws_contours_and_reports_their_levels() {
        let _shared = shared_data();
        let mut session = Session::new(None, &mut Captured::default());
        let mut captured = Captured::default();
        let drawn = session.run(
            format!("CONTOUR FILE \"{}\" interval=0.5 major=2 base=100", plane()),
            &mut captured,
        );
        assert!(drawn);
        holds(
            &captured.out,
            "contours method=grid cell=1 levels=3 count=3 major=1 minor=2",
        );
    }
}
