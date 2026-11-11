//! tests/geo/test_drawing_curves.cpp: the draw system's curves - the curve
//! polyline (arcs by bulge, heights in its vertices), the ellipse and the
//! spline - through the verbs that read the drawing (docs/geoprocessing.md
//! "Bindings", docs/terrain.md "Sampling and drape", docs/interop.md "Export
//! options"): the drape, a zone, a clip boundary, a dissolve and EXPORT, each
//! through the one executor. The file's cases of the one conversion and of the
//! surface built from the drawing are step 5's (katana_interop's
//! drawing_bindings target). These six drive DRAPE, RASTER ZONAL, EXPORT and
//! INFO beside GIS CLIP and GIS DISSOLVE, so they pass when step 6's wave 2 is
//! integrated as one.
//!
//! Every expected value is worked by hand beside it. Two bounds recur:
//!
//! ```text
//! A chorded curve's area. Every chord point lies on the curve, so on a
//!   convex curve the chorded ring lies inside it and its area is SMALLER, by
//!   the sum of the segments between each chord and its arc. A segment of
//!   chord c whose arc strays at most s from it has area at most c x s (it
//!   lies inside the c by s rectangle over the chord); the chords are each
//!   shorter than their arcs and s is at most the chord tolerance, so the
//!   loss is at most (the curve's length) x (the tolerance).
//! The default chord tolerance is DrawingDatasetOptions::curve_tolerance,
//!   1 mm (the export's).
//! ```

mod drawing_curves {
    use std::collections::BTreeMap;
    use std::f64::consts::PI;

    use katana_app::geo::replies::{Record, parse_records};
    use katana_entity::entity::ELEVATIONS_PROPERTY;
    use katana_entity::{Geometry, PropertyMap, PropertyValue};
    use katana_geometry::{CurvePolyline2, CurveVertex, Ellipse2, Point2, Spline2, Vec2};

    use crate::geo_fixture::{GeoFixture, geo_data, must, near, shared_data};
    use crate::{double_eq, fixture};

    /// DrawingDatasetOptions::curve_tolerance.
    const TOLERANCE: f64 = 0.001;

    /// tests/geo/data/plane.asc, as a line names it.
    fn plane() -> String {
        format!("{}/plane.asc", geo_data())
    }

    fn quoted(path: &str) -> String {
        format!("\"{path}\"")
    }

    /// One entity on `layer`: the C++ fixture's add(geometry, layer).
    fn add(fixture: &mut GeoFixture, geometry: impl Into<Geometry>, layer: &str) {
        fixture.add(geometry.into(), layer, PropertyMap::new());
    }

    fn vertex(x: f64, y: f64, bulge: f64, height: Option<f64>) -> CurveVertex {
        CurveVertex {
            position: Point2::new(x, y),
            bulge,
            height,
        }
    }

    /// The 10 x 10 square from (0,0), anticlockwise, its east side (from
    /// (10,0) to (10,10)) an arc of a quarter turn bulging out of it. A
    /// positive bulge turns anticlockwise, which going north puts the arc to
    /// the east - outside.
    fn bulged_square() -> CurvePolyline2 {
        CurvePolyline2 {
            vertices: vec![
                vertex(0.0, 0.0, 0.0, None),
                vertex(10.0, 0.0, (PI / 8.0).tan(), None),
                vertex(10.0, 10.0, 0.0, None),
                vertex(0.0, 10.0, 0.0, None),
            ],
            closed: true,
        }
    }

    /// From (0,0) at height 10 to (10,0) at height 20 by an anticlockwise
    /// semicircle (bulge 1): centre (5,0), radius 5, through (5,-5).
    fn sloping_semicircle(end_height: Option<f64>) -> CurvePolyline2 {
        CurvePolyline2 {
            vertices: vec![
                vertex(0.0, 0.0, 1.0, Some(10.0)),
                vertex(10.0, 0.0, 0.0, end_height),
            ],
            closed: false,
        }
    }

    /// The ellipse centred (20,15) with semi-axes 15 (along x) and 10: inside
    /// plane.asc (0..40 by 0..30). Its area is pi a b = 150 pi = 471.2389;
    /// its perimeter, by Ramanujan's second approximation (error below 1e-6
    /// at this eccentricity), pi (3 (a + b) - sqrt((3a + b)(a + 3b))) = pi
    /// (75 - sqrt(55 x 45)) = 79.327.
    fn zone_ellipse() -> Ellipse2 {
        Ellipse2 {
            center: Point2::new(20.0, 15.0),
            major_axis: Vec2::new(15.0, 0.0),
            ratio: 10.0 / 15.0,
            ..Ellipse2::default()
        }
    }

    const ELLIPSE_AREA: f64 = 150.0 * PI;

    fn ellipse_perimeter() -> f64 {
        PI * (75.0 - (55.0_f64 * 45.0).sqrt())
    }

    // ---- the drape ----------------------------------------------------------------------------

    #[test]
    fn drape_gives_a_curve_polylines_vertices_their_heights_in_its_geometry_as_one_step() {
        // plane.asc is z = 100 + 0.05 x at the cell centres; bilinear sampling
        // of a plane is the plane (tests/geo/test_drape_and_sample.cpp): 5.5 ->
        // 100.275, 20.25 -> 101.0125, 35 -> 101.75, each within 1e-5 (Float32).
        let _shared = shared_data();
        let mut fixture =
            fixture("DrapeGivesACurvePolylinesVerticesTheirHeightsInItsGeometryAsOneStep");
        let drawn = CurvePolyline2 {
            vertices: vec![
                vertex(5.5, 5.0, 0.2, None),
                vertex(20.25, 10.0, 0.0, None),
                vertex(35.0, 25.0, 0.0, None),
            ],
            closed: false,
        };
        let id = fixture.add(drawn.clone().into(), "0", PropertyMap::new());
        let before = fixture.document.history().undo_count();
        let reply = fixture.ok(&format!("DRAPE FILE {} DRAWING", quoted(&plane())));
        // One step, or the undo at the end would take the drawing of it away.
        assert_eq!(
            fixture.document.history().undo_count(),
            before + 1,
            "{reply}"
        );
        assert!(reply.contains("entities=1 vertices=3 off=0"), "{reply}");
        let draped = fixture
            .document
            .model()
            .entities
            .find(id)
            .cloned()
            .expect("the string is still there");
        let Geometry::CurvePolyline(curve) = &draped.geometry else {
            panic!("the drape kept the arcs");
        };
        assert_eq!(curve.vertices.len(), 3);
        near(curve.vertices[0].height.unwrap_or(0.0), 100.275, 1e-5);
        near(curve.vertices[1].height.unwrap_or(0.0), 101.0125, 1e-5);
        near(curve.vertices[2].height.unwrap_or(0.0), 101.75, 1e-5);
        double_eq(curve.vertices[0].bulge, 0.2);
        assert_eq!(curve.vertices[0].position, Point2::new(5.5, 5.0));
        // The heights are the geometry's: no elevation property is written.
        assert!(!draped.properties.contains_key(ELEVATIONS_PROPERTY));
        // A second drape on the same ground changes nothing.
        let again = fixture.ok(&format!("DRAPE FILE {} DRAWING", quoted(&plane())));
        assert_eq!(
            fixture.document.history().undo_count(),
            before + 1,
            "{again}"
        );
        assert!(fixture.document.undo().is_ok());
        match fixture
            .document
            .model()
            .entities
            .find(id)
            .map(|entity| &entity.geometry)
        {
            Some(Geometry::CurvePolyline(undone)) => assert_eq!(*undone, drawn),
            _ => panic!("the undone string is not a curve polyline"),
        }
    }

    #[test]
    fn drape_leaves_an_ellipse_and_a_spline_and_counts_them() {
        let _shared = shared_data();
        let mut fixture = fixture("DrapeLeavesAnEllipseAndASplineAndCountsThem");
        add(&mut fixture, zone_ellipse(), "0");
        let spline = must(Spline2::through_points(
            vec![
                Point2::new(5.0, 5.0),
                Point2::new(10.0, 8.0),
                Point2::new(15.0, 5.0),
            ],
            3,
        ));
        add(&mut fixture, spline, "0");
        let reply = fixture.ok(&format!("DRAPE FILE {} DRAWING PREVIEW", quoted(&plane())));
        assert!(reply.contains("skipped.ellipse=1"), "{reply}");
        assert!(reply.contains("skipped.spline=1"), "{reply}");
    }

    // ---- zones, boundaries, dissolve ----------------------------------------------------------

    #[test]
    fn an_ellipse_zones_count_is_its_area_in_cells_within_the_chord_error() {
        // plane.asc's cells are 1 m, so a fractional count is the zone's area:
        // the chorded ellipse's, between pi a b - (perimeter x tolerance) and
        // pi a b (see the top of the file): 471.2389 - 0.0793 .. 471.2389. The
        // coverage fractions may be single precision (exactextract keeps them
        // as float): 6e-8 relative over 471 cells is 2.8e-5; 1e-4 allows that.
        let _shared = shared_data();
        let mut fixture = fixture("AnEllipseZonesCountIsItsAreaInCellsWithinTheChordError");
        let zone = fixture.add(zone_ellipse().into(), "zones", PropertyMap::new());
        let reply = fixture.ok(&format!(
            "RASTER ZONAL FILE {} LAYERS zones stats=count",
            quoted(&plane())
        ));
        let entity = fixture
            .document
            .model()
            .entities
            .find(zone)
            .cloned()
            .expect("the zone is still there");
        let count = match entity.properties.get("zone_count") {
            Some(PropertyValue::Real(real)) => *real,
            Some(PropertyValue::Integer(whole)) => *whole as f64,
            Some(_) => panic!("zone_count is not a number: {reply}"),
            None => panic!("no zone_count: {reply}"),
        };
        assert!(count <= ELLIPSE_AREA + 1e-4, "{count}");
        assert!(
            count >= ELLIPSE_AREA - ellipse_perimeter() * TOLERANCE - 1e-4,
            "{count}"
        );
    }

    #[test]
    fn a_line_clipped_by_an_ellipse_keeps_the_major_axis_length() {
        // The line y = 15 from x = 0 to 40 crosses the ellipse where its major
        // axis ends, (5,15) and (35,15): 30 m inside. There the ellipse's
        // tangent is vertical, so a chord cutting the corner lies at most the
        // chord tolerance inside it along x: the clipped length is 30 less at
        // most 2 mm at the two ends.
        let mut fixture = fixture("ALineClippedByAnEllipseKeepsTheMajorAxisLength");
        fixture.line((0.0, 15.0), (40.0, 15.0), "pipes", PropertyMap::new());
        add(&mut fixture, zone_ellipse(), "zones");
        let reply = fixture.ok("GIS CLIP LAYERS pipes BY LAYERS zones");
        let pieces = fixture.on("gis/clip");
        assert_eq!(pieces.len(), 1, "{reply}");
        let length = match &pieces[0].geometry {
            Geometry::Line(segment) => segment.length(),
            Geometry::Polyline(line) => line.length(),
            _ => 0.0,
        };
        assert!(length <= 30.0 + 1e-9, "{length}");
        assert!(length >= 30.0 - 2.0 * TOLERANCE, "{length}");
    }

    #[test]
    fn dissolve_replace_deletes_the_ellipses_it_merged() {
        // Two whole ellipses and a closed curve polyline are three areas, and
        // REPLACE deletes all three with the merged area made - the dissolve's
        // own list of what it merged is the conversion's areas table.
        let mut fixture = fixture("DissolveReplaceDeletesTheEllipsesItMerged");
        add(&mut fixture, zone_ellipse(), "lots");
        let second = Ellipse2 {
            center: Point2::new(30.0, 15.0),
            ..zone_ellipse()
        };
        add(&mut fixture, second, "lots");
        add(&mut fixture, bulged_square(), "lots");
        let reply = fixture.ok("GIS DISSOLVE LAYERS lots REPLACE");
        let output = GeoFixture::record(&reply, "output").expect("an output record");
        assert_eq!(output.get("deleted"), Some("3"), "{reply}");
        assert!(fixture.on("lots").is_empty());
    }

    // ---- EXPORT -------------------------------------------------------------------------------

    #[test]
    fn export_writes_each_curve_kind_and_info_reads_it_back() {
        let mut fixture = fixture("ExportWritesEachCurveKindAndInfoReadsItBack");
        add(&mut fixture, bulged_square(), "curves");
        add(&mut fixture, sloping_semicircle(Some(20.0)), "strings");
        add(&mut fixture, zone_ellipse(), "ellipses");
        let spline = must(Spline2::through_points(
            vec![
                Point2::new(0.0, 20.0),
                Point2::new(5.0, 25.0),
                Point2::new(10.0, 20.0),
            ],
            3,
        ));
        add(&mut fixture, spline, "splines");
        let out = fixture.scratch.file("curves.gpkg");
        let exported = fixture.ok(&format!("EXPORT {} split=layer", quoted(&out)));
        let info = fixture.ok(&format!("INFO {}", quoted(&out)));
        let mut layers: BTreeMap<String, Record> = BTreeMap::new();
        for each in parse_records(&info) {
            if each.kind == "layer" {
                let name = String::from(each.get("name").unwrap_or(""));
                layers.entry(name).or_insert(each);
            }
        }
        assert_eq!(layers.len(), 4, "{exported}\n{info}");
        for (name, layer) in &layers {
            assert_eq!(layer.get("features"), Some("1"), "{name}");
        }
        // A closed curve polyline and a whole ellipse are areas, in plan; the
        // 3D string is a line with Z; the open spline a line in plan. INFO
        // names a layer's geometry by its ISO SQL/MM well-known-text type (ISO
        // 13249-3, as OGC Simple Features 1.2.1 does): Polygon, LineString,
        // LineStringZ.
        assert_eq!(layers["curves"].get("geometry"), Some("Polygon"), "{info}");
        assert_eq!(
            layers["ellipses"].get("geometry"),
            Some("Polygon"),
            "{info}"
        );
        assert_eq!(
            layers["strings"].get("geometry"),
            Some("LineStringZ"),
            "{info}"
        );
        assert_eq!(
            layers["splines"].get("geometry"),
            Some("LineString"),
            "{info}"
        );
    }
}
