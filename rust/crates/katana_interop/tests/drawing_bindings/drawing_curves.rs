//! tests/geo/test_drawing_curves.cpp: the draw system's curves - the curve
//! polyline (arcs by bulge, heights in its vertices), the ellipse and the
//! spline - through the one entity-to-feature conversion and the surface
//! built from the drawing (docs/geoprocessing.md "Bindings", docs/terrain.md
//! "Surfaces on every front end"). The file's drape, zone, clip, dissolve and
//! EXPORT cases drive the verbs through the app layer (step 6); the fixture
//! here holds only the drawing and `add`, as the plan's L1 entry has it.
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
    use std::f64::consts::PI;

    use katana_cad::Document;
    use katana_cad::geo::surface_input::surface_input;
    use katana_commands::{create_entities, create_layer};
    use katana_entity::entity::INVALID_ENTITY_ID;
    use katana_entity::{Entity, EntityId, Geometry, Layer};
    use katana_geometry::{CurvePolyline2, CurveVertex, Ellipse2, Point2, Spline2, Vec2};
    use katana_gis::processing::{FeatureTable, FieldValue};
    use katana_gis::{GeoPoint, GeometryKind};
    use katana_interop::geo::drawing_dataset::{
        DrawingDataset, DrawingDatasetOptions, drawing_dataset,
    };

    use crate::{double_eq, must, near};

    /// DrawingDatasetOptions::curve_tolerance.
    const TOLERANCE: f64 = 0.001;

    /// The drawing the C++'s VectorFixture gives these cases, with `add` - the
    /// one of its helpers they use.
    struct DrawingCurves {
        document: Document,
    }

    impl DrawingCurves {
        fn new() -> Self {
            Self {
                document: Document::default(),
            }
        }

        /// One entity drawn as one undo step on layer "0"; its id.
        #[track_caller]
        fn add(&mut self, geometry: impl Into<Geometry>) -> EntityId {
            let layer = "0";
            if !self.document.model().layers.contains(layer) {
                let made = Layer {
                    name: String::from(layer),
                    ..Layer::default()
                };
                assert!(self.document.execute(create_layer(made)).is_ok());
            }
            let entity = Entity {
                geometry: geometry.into(),
                layer: String::from(layer),
                ..Entity::default()
            };
            assert!(self.document.execute(create_entities(vec![entity])).is_ok());
            self.document
                .last_created_entities()
                .first()
                .copied()
                .unwrap_or(INVALID_ENTITY_ID)
        }

        fn dataset(&self, ids: &[EntityId], options: &DrawingDatasetOptions) -> DrawingDataset {
            must(drawing_dataset(self.document.model(), ids, options))
        }
    }

    fn table<'a>(dataset: &'a DrawingDataset, name: &str) -> Option<&'a FeatureTable> {
        dataset.set.tables.iter().find(|each| each.name == name)
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
    /// the east - outside. The arc: chord 10, included angle theta = pi/2,
    /// bulge tan(theta/4) = tan(pi/8); radius 10 / (2 sin(pi/4)) = 5 sqrt 2,
    /// so r^2 = 50; the segment between chord and arc has area r^2 / 2
    /// (theta - sin theta) = 25 (pi/2 - 1). The whole area is
    /// 100 + 25 (pi/2 - 1) = 114.2699...; the arc's length is r theta =
    /// 5 sqrt 2 x pi / 2 = 11.107.
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
    /// semicircle (bulge 1): centre (5,0), radius 5, through (5,-5). A point
    /// at angle phi about the centre (phi from pi to 2 pi, anticlockwise from
    /// +x) is (phi - pi) / pi of the way along the arc by length, so its
    /// height by the geometry's rule (linear by length along the segment) is
    /// 10 + 10 (phi - pi) / pi.
    fn sloping_semicircle(end_height: Option<f64>) -> CurvePolyline2 {
        CurvePolyline2 {
            vertices: vec![
                vertex(0.0, 0.0, 1.0, Some(10.0)),
                vertex(10.0, 0.0, 0.0, end_height),
            ],
            closed: false,
        }
    }

    fn height_on_semicircle(x: f64, y: f64) -> f64 {
        // atan2 gives (-pi, pi]: below the diameter is (-pi, 0], and the end
        // (10,0) is 0 - each a turn short of the angle anticlockwise from the
        // start's pi. The start itself is pi (or -pi, read at y = -0).
        let mut phi = y.atan2(x - 5.0);
        if phi < PI / 2.0 {
            phi += 2.0 * PI;
        }
        10.0 + 10.0 * (phi - PI) / PI
    }

    fn ring_area(ring: &[GeoPoint]) -> f64 {
        let mut twice = 0.0;
        for (i, a) in ring.iter().enumerate() {
            let b = &ring[(i + 1) % ring.len()];
            twice += a.x * b.y - b.x * a.y;
        }
        twice / 2.0
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

    // ---- the one conversion ---------------------------------------------------------------

    #[test]
    fn a_closed_curve_polyline_is_an_area_short_of_its_arcs_segment_by_at_most_the_chord_error() {
        let mut fixture = DrawingCurves::new();
        let id = fixture.add(bulged_square());
        let dataset = fixture.dataset(&[id], &DrawingDatasetOptions::default());
        assert_eq!(dataset.set.tables.len(), 1);
        let areas = table(&dataset, "polygons").expect("a polygons table");
        assert_eq!(areas.features.len(), 1);
        let ring = &areas.features[0].parts[0];
        assert_eq!(ring.kind, GeometryKind::Polygon);
        assert!(!ring.has_z); // no vertex has a height
        assert_eq!(ring.parts.len(), 1);
        let points = &ring.parts[0];
        // More than the four vertices: the arc is chords. The ring is not
        // closed by a repeat, as every area's ring here.
        assert!(points.len() > 4);
        let (first, last) = (points[0], points[points.len() - 1]);
        assert!(first.x != last.x || first.y != last.y);
        let exact = 100.0 + 25.0 * (PI / 2.0 - 1.0);
        let arc_length = 5.0 * 2.0_f64.sqrt() * PI / 2.0;
        let area = ring_area(points);
        assert!(area > 0.0); // anticlockwise, as drawn
        assert!(area <= exact + 1e-9, "{area}");
        assert!(area >= exact - arc_length * TOLERANCE, "{area}");
        assert_eq!(dataset.stats.polygons, 1);
        assert_eq!(
            areas.features[0].values[4],
            FieldValue::Text(String::from("curvepolyline"))
        );
    }

    #[test]
    fn a_sloping_arcs_chord_points_take_the_height_linear_by_length_along_it() {
        let mut fixture = DrawingCurves::new();
        let id = fixture.add(sloping_semicircle(Some(20.0)));
        let dataset = fixture.dataset(&[id], &DrawingDatasetOptions::default());
        let lines = table(&dataset, "lines").expect("a lines table");
        assert_eq!(lines.features.len(), 1);
        let line = &lines.features[0].parts[0];
        assert_eq!(line.kind, GeometryKind::LineString);
        assert!(line.has_z);
        assert!(lines.has_z);
        let points = &line.parts[0];
        assert!(points.len() > 2);
        double_eq(points[0].z, 10.0);
        double_eq(points[points.len() - 1].z, 20.0);
        for p in points {
            // On the circle, below its diameter, at the height worked out
            // above. The trigonometry is exact to a few ulps of the
            // coordinates: 1e-9.
            near((p.x - 5.0).hypot(p.y), 5.0, 1e-9);
            assert!(p.y <= 1e-9);
            near(p.z, height_on_semicircle(p.x, p.y), 1e-9);
        }
    }

    #[test]
    fn an_arc_with_an_end_not_surveyed_goes_in_plan_not_at_zero() {
        let mut fixture = DrawingCurves::new();
        let id = fixture.add(sloping_semicircle(None));
        let dataset = fixture.dataset(&[id], &DrawingDatasetOptions::default());
        let lines = table(&dataset, "lines").expect("a lines table");
        assert!(!lines.features[0].parts[0].has_z);
        assert!(!dataset.stats.warnings.is_empty());
        assert!(
            dataset.stats.warnings[0].contains("height at every vertex"),
            "{}",
            dataset.stats.warnings[0]
        );
        // And an algorithm that needs heights is not handed it.
        let heighted = DrawingDatasetOptions {
            require_heights: true,
            ..DrawingDatasetOptions::default()
        };
        let gridded = fixture.dataset(&[id], &heighted);
        assert_eq!(
            gridded
                .stats
                .skipped
                .get("heightless")
                .copied()
                .unwrap_or(0),
            1
        );
    }

    #[test]
    fn a_whole_ellipse_is_an_area_and_an_arc_of_one_a_line_both_in_plan() {
        let mut fixture = DrawingCurves::new();
        let whole = fixture.add(zone_ellipse());
        let half = Ellipse2 {
            sweep: PI,
            ..zone_ellipse()
        };
        let arc = fixture.add(half);
        let dataset = fixture.dataset(&[whole, arc], &DrawingDatasetOptions::default());
        let areas = table(&dataset, "polygons").expect("a polygons table");
        let lines = table(&dataset, "lines").expect("a lines table");
        assert_eq!(areas.features.len(), 1);
        assert_eq!(lines.features.len(), 1);
        let ring = &areas.features[0].parts[0];
        assert!(!ring.has_z);
        let area = ring_area(&ring.parts[0]);
        assert!(area <= ELLIPSE_AREA + 1e-9, "{area}");
        assert!(
            area >= ELLIPSE_AREA - ellipse_perimeter() * TOLERANCE,
            "{area}"
        );
        // The half from t = 0 to pi: (35,15) round the top to (5,15).
        let path = &lines.features[0].parts[0];
        assert_eq!(path.kind, GeometryKind::LineString);
        let points = &path.parts[0];
        near(points[0].x, 35.0, 1e-9);
        near(points[points.len() - 1].x, 5.0, 1e-9);
        near(points[points.len() - 1].y, 15.0, 1e-9);
    }

    #[test]
    fn a_closed_spline_is_an_area_and_an_open_one_a_line() {
        let corners = [
            (0.0, 0.0),
            (10.0, 0.0),
            (10.0, 10.0),
            (0.0, 10.0),
            (0.0, 0.0),
        ];
        let closed = must(Spline2::through_points(
            corners.iter().map(|&(x, y)| Point2::new(x, y)).collect(),
            3,
        ));
        let open = must(Spline2::through_points(
            vec![
                Point2::new(0.0, 20.0),
                Point2::new(5.0, 25.0),
                Point2::new(10.0, 20.0),
            ],
            3,
        ));
        let mut fixture = DrawingCurves::new();
        let shut = fixture.add(closed);
        let path = fixture.add(open);
        let dataset = fixture.dataset(&[shut, path], &DrawingDatasetOptions::default());
        let areas = table(&dataset, "polygons").expect("a polygons table");
        let lines = table(&dataset, "lines").expect("a lines table");
        assert_eq!(areas.features.len(), 1);
        assert_eq!(lines.features.len(), 1);
        // The open one runs from its first fit point to its last.
        let points = &lines.features[0].parts[0].parts[0];
        near(points[0].x, 0.0, 1e-9);
        near(points[points.len() - 1].x, 10.0, 1e-9);
        near(points[points.len() - 1].y, 20.0, 1e-9);
    }

    // ---- the surface from the drawing ---------------------------------------------------------

    #[test]
    fn a_surface_takes_a_curve_polyline_as_a_breakline_through_its_chords_at_their_heights() {
        let mut fixture = DrawingCurves::new();
        let string = fixture.add(sloping_semicircle(Some(20.0)));
        let ellipse = fixture.add(zone_ellipse());
        let input = surface_input(fixture.document.model(), &[string, ellipse]);
        assert_eq!(input.used, 1);
        assert_eq!(input.heightless_vertices, 0);
        assert_eq!(input.skipped.get("ellipse"), Some(&1));
        assert_eq!(input.input.breaklines.len(), 1);
        let vertices = &input.input.breaklines[0].vertices;
        assert!(!input.input.breaklines[0].closed);
        // The arc as chords (more than its two ends), within a millimetre
        // (katana_geometry's CURVE_CHORD_TOLERANCE), each at its height by
        // length.
        assert!(vertices.len() > 2);
        assert_eq!(input.input.points.len(), vertices.len());
        for p in vertices {
            near((p.x - 5.0).hypot(p.y), 5.0, 1e-9);
            near(p.z, height_on_semicircle(p.x, p.y), 1e-9);
        }
    }

    #[test]
    fn a_surface_breaks_a_curve_polyline_where_an_end_has_no_height() {
        // The arc's far end is not surveyed: every chord point after the
        // first has no height (none is invented), so only the first vertex is
        // a point and there is no breakline of two.
        let mut fixture = DrawingCurves::new();
        let string = fixture.add(sloping_semicircle(None));
        let input = surface_input(fixture.document.model(), &[string]);
        assert!(input.input.breaklines.is_empty());
        assert_eq!(input.input.points.len(), 1);
        double_eq(input.input.points[0].z, 10.0);
        assert!(input.heightless_vertices > 1);
    }

    #[test]
    fn a_closed_curve_polyline_is_a_closed_breakline_with_no_repeated_point() {
        // Two vertices at height 5, each bulge 1: two semicircles, a circle
        // of radius 5 about (5,0), level.
        let circle = CurvePolyline2 {
            vertices: vec![
                vertex(0.0, 0.0, 1.0, Some(5.0)),
                vertex(10.0, 0.0, 1.0, Some(5.0)),
            ],
            closed: true,
        };
        let mut fixture = DrawingCurves::new();
        let id = fixture.add(circle);
        let input = surface_input(fixture.document.model(), &[id]);
        assert_eq!(input.input.breaklines.len(), 1);
        let ring = &input.input.breaklines[0];
        assert!(ring.closed);
        assert!(ring.vertices.len() > 3);
        let (first, last) = (ring.vertices[0], ring.vertices[ring.vertices.len() - 1]);
        assert!(first.x != last.x || first.y != last.y);
        for p in &ring.vertices {
            double_eq(p.z, 5.0);
        }
    }
}
