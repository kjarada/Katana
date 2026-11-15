// The one reader of the drawing's polygons and lines that the surface edits
// and the volumes share (terrain/drawing_shapes.rs;
// tests/cad/terrain/test_drawing_shapes.cpp): what each kind of entity gives,
// typed corners, the words, and what a scope takes. Every corner, height, area
// and count asserted is worked by hand beside it.

use crate::circle_spline::circle_spline;
use crate::{double_eq, must, near, ok};
use katana_cad::document::Document;
use katana_cad::scope_verbs::ScopeSource;
use katana_cad::terrain::drawing_shapes::{
    DrawingShapes, NotAShape, ShapeKind, ShapeOptions, TypedCorner, entities_named,
    parse_typed_corner, read_shape_words, resolve_shapes, shape_of, shape_of_corners,
};
use katana_commands::create_entities;
use katana_core::{ErrorCode, Result};
use katana_entity::entity::{ELEVATION_PROPERTY, set_heights};
use katana_entity::{Entity, EntityId, Geometry, PointGeometry, PropertyValue, TextGeometry};
use katana_geometry::curves2d::CURVE_CHORD_TOLERANCE;
use katana_geometry::{
    Arc2, Circle2, CurvePolyline2, CurveVertex, Ellipse2, Point2, Polyline2, Segment2, Spline2,
    Vec2,
};
use katana_math::numerics::PI;

type Heights = Vec<Option<f64>>;

// `count` vertices with no height: the C++ `Heights(count)`.
fn no_heights(count: usize) -> Heights {
    vec![None; count]
}

// The chord tolerance the rule names: 1 mm of sagitta.
const SAGITTA: f64 = CURVE_CHORD_TOLERANCE;

fn entity_of(shape: impl Into<Geometry>, heights: &[Option<f64>]) -> Entity {
    let mut entity = Entity {
        id: 7,
        geometry: shape.into(),
        ..Entity::default()
    };
    if !heights.is_empty() {
        set_heights(&mut entity.properties, heights);
    }
    entity
}

fn polyline(vertices: Vec<Point2>, closed: bool) -> Polyline2 {
    Polyline2::new(vertices, closed)
}

fn point(x: f64, y: f64) -> Point2 {
    Point2::new(x, y)
}

// The square [0, 10]^2 counter-clockwise from the origin.
fn square() -> Vec<Point2> {
    vec![
        point(0.0, 0.0),
        point(10.0, 0.0),
        point(10.0, 10.0),
        point(0.0, 10.0),
    ]
}

// Twice the signed area by the shoelace, halved.
fn area(ring: &Polyline2) -> f64 {
    let mut twice = 0.0;
    let v = &ring.vertices;
    for (i, a) in v.iter().enumerate() {
        let b = v[(i + 1) % v.len()];
        twice += a.x * b.y - b.x * a.y;
    }
    0.5 * twice
}

fn words_of(line: &str) -> Vec<String> {
    line.split(' ')
        .filter(|word| !word.is_empty())
        .map(String::from)
        .collect()
}

fn half_turn() -> Arc2 {
    Arc2 {
        center: point(0.0, 0.0),
        radius: 5.0,
        start_angle: 0.0,
        sweep: PI,
    }
}

// ---- what an entity gives as a polygon, as a line; typed corners; the words ----------------

mod drawing_shapes {
    use super::*;

    #[test]
    fn a_closed_polyline_is_a_polygon_of_its_vertices_at_their_heights() {
        let of = shape_of(
            &entity_of(
                polyline(square(), true),
                &[Some(1.0), Some(2.0), None, Some(4.0)],
            ),
            ShapeKind::Polygon,
        );
        let shape = of.shape.as_ref().expect("a polygon");
        assert_eq!(shape.outline.vertices, square());
        assert!(shape.outline.closed);
        assert_eq!(shape.heights, vec![Some(1.0), Some(2.0), None, Some(4.0)]);
        assert_eq!(shape.entity, Some(7));
        assert!(!shape.chorded);
    }

    #[test]
    fn a_polyline_drawn_back_to_its_start_is_a_polygon_without_the_repeated_corner() {
        // Open, but its fifth vertex is its first: the square, four corners, the
        // fifth height going with the fifth vertex.
        let mut drawn = square();
        drawn.push(point(0.0, 0.0));
        let open = shape_of(
            &entity_of(
                polyline(drawn.clone(), false),
                &[Some(1.0), Some(2.0), Some(3.0), Some(4.0), Some(1.0)],
            ),
            ShapeKind::Polygon,
        );
        let shape = open.shape.as_ref().expect("a polygon");
        assert_eq!(shape.outline.vertices, square());
        assert!(shape.outline.closed);
        assert_eq!(
            shape.heights,
            vec![Some(1.0), Some(2.0), Some(3.0), Some(4.0)]
        );
        // Closed and repeating it as well: the same four.
        let closed = shape_of(&entity_of(polyline(drawn, true), &[]), ShapeKind::Polygon);
        let shape = closed.shape.as_ref().expect("a polygon");
        assert_eq!(shape.outline.vertices, square());
        assert_eq!(shape.heights, no_heights(4));
    }

    #[test]
    fn an_open_polyline_a_line_an_arc_and_an_elliptical_arc_bound_nothing() {
        assert_eq!(
            shape_of(
                &entity_of(polyline(square(), false), &[]),
                ShapeKind::Polygon
            )
            .why,
            NotAShape::Open
        );
        assert_eq!(
            shape_of(
                &entity_of(Segment2::new(point(0.0, 0.0), point(5.0, 5.0)), &[]),
                ShapeKind::Polygon
            )
            .why,
            NotAShape::Open
        );
        assert_eq!(
            shape_of(&entity_of(half_turn(), &[]), ShapeKind::Polygon).why,
            NotAShape::Open
        );
        let half = Ellipse2 {
            major_axis: Vec2::new(20.0, 0.0),
            ratio: 0.5,
            sweep: PI,
            ..Ellipse2::default()
        };
        let elliptical_arc = shape_of(&entity_of(half, &[]), ShapeKind::Polygon);
        assert!(elliptical_arc.shape.is_none());
        assert_eq!(elliptical_arc.why, NotAShape::Open);
        assert_eq!(
            shape_of(
                &entity_of(CurvePolyline2::from_points(&square(), false), &[]),
                ShapeKind::Polygon
            )
            .why,
            NotAShape::Open
        );
    }

    #[test]
    fn a_curve_polyline_is_chorded_within_a_millimetre_and_its_arc_points_take_their_height_along_it()
     {
        // The half-disc over [0, 10] on the x axis: straight from A (0,0) at 1
        // to B (10,0) at 3, then the bulge-1 arc (a half turn, counter-
        // clockwise) from B back to A through (5,5). On the arc a point at angle
        // t from B (about the centre (5,0)) is t / pi of the way to A, so at
        // 3 + (1 - 3) t / pi.
        let curve = CurvePolyline2 {
            vertices: vec![
                CurveVertex {
                    position: point(0.0, 0.0),
                    bulge: 0.0,
                    height: Some(1.0),
                },
                CurveVertex {
                    position: point(10.0, 0.0),
                    bulge: 1.0,
                    height: Some(3.0),
                },
            ],
            closed: true,
        };
        let of = shape_of(&entity_of(curve, &[]), ShapeKind::Polygon);
        let shape = of.shape.as_ref().expect("a polygon");
        assert!(shape.chorded);
        assert!(shape.outline.closed);
        let vertices = &shape.outline.vertices;
        assert!(vertices.len() > 3);
        assert_eq!(shape.heights.len(), vertices.len());
        assert_eq!(vertices[0], point(0.0, 0.0));
        assert_eq!(shape.heights[0], Some(1.0));
        assert_eq!(vertices[1], point(10.0, 0.0));
        assert_eq!(shape.heights[1], Some(3.0));
        assert!(vertices[vertices.len() - 1] != point(0.0, 0.0)); // A is not repeated
        let centre = point(5.0, 0.0);
        for (i, &p) in vertices.iter().enumerate().skip(2) {
            near(p.distance_to(centre), 5.0, 1e-9);
            assert!(p.y > 0.0, "{i}");
            let turned = p.y.atan2(p.x - 5.0);
            let height = shape.heights[i].expect("a height on the arc");
            near(height, 3.0 - 2.0 * turned / PI, 1e-9);
            // The chord into this point: its middle no more than 1 mm inside.
            let q = vertices[i - 1];
            let middle = point((p.x + q.x) / 2.0, (p.y + q.y) / 2.0);
            assert!(5.0 - middle.distance_to(centre) <= SAGITTA + 1e-12, "{i}");
        }
        // Inscribed chords lose area, at most a chord's length times its
        // sagitta each: the half-disc 25 pi / 2 less at most 5 pi x 1 mm.
        let half_disc = 25.0 * PI / 2.0;
        assert!(area(&shape.outline) <= half_disc);
        assert!(area(&shape.outline) >= half_disc - 5.0 * PI * SAGITTA);
    }

    #[test]
    fn a_curve_polyline_without_arcs_is_its_vertices_exactly_and_not_chorded() {
        let curve = CurvePolyline2::from_polyline(
            &polyline(square(), true),
            &[Some(5.0), None, Some(7.0), Some(8.0)],
        );
        let of = shape_of(&entity_of(curve, &[]), ShapeKind::Polygon);
        let shape = of.shape.as_ref().expect("a polygon");
        assert_eq!(shape.outline.vertices, square());
        assert_eq!(shape.heights, vec![Some(5.0), None, Some(7.0), Some(8.0)]);
        assert!(!shape.chorded);
        // Open and back at its start: the same square.
        let mut drawn = square();
        drawn.push(point(0.0, 0.0));
        let back = shape_of(
            &entity_of(CurvePolyline2::from_points(&drawn, false), &[]),
            ShapeKind::Polygon,
        );
        let shape = back.shape.as_ref().expect("a polygon");
        assert_eq!(shape.outline.vertices, square());
        assert!(shape.outline.closed);
    }

    #[test]
    fn a_circle_and_a_full_ellipse_are_chorded_at_the_entitys_one_elevation() {
        // A circle of radius 3 at 12.5: every corner on it, every chord's middle
        // within 1 mm of it, every corner at 12.5.
        let centre = point(5.0, 5.0);
        let mut circle = entity_of(Circle2::new(centre, 3.0), &[]);
        circle
            .properties
            .insert(String::from(ELEVATION_PROPERTY), PropertyValue::from(12.5));
        let ring = shape_of(&circle, ShapeKind::Polygon);
        let shape = ring.shape.as_ref().expect("a polygon");
        let corners = &shape.outline.vertices;
        assert!(corners.len() >= 3);
        assert!(shape.chorded);
        assert!(shape.outline.closed);
        assert!(corners[0].distance_to(corners[corners.len() - 1]) > 1e-3); // no repeat
        for (i, &p) in corners.iter().enumerate() {
            let q = corners[(i + 1) % corners.len()];
            near(p.distance_to(centre), 3.0, 1e-9);
            let middle = point((p.x + q.x) / 2.0, (p.y + q.y) / 2.0);
            assert!(3.0 - middle.distance_to(centre) <= SAGITTA + 1e-12, "{i}");
        }
        assert_eq!(shape.heights, vec![Some(12.5); corners.len()]);

        // The full ellipse of semi-axes 20 and 10 about the origin, no height:
        // its corners on (x/20)^2 + (y/10)^2 = 1, and its area pi 20 10 less at
        // most its perimeter (Ramanujan: pi (90 - sqrt 3500) = 96.9) times the
        // 1 mm a chord may stand off it - 0.097; 0.15 allows a chord's farthest
        // point to be off its middle.
        let whole = Ellipse2 {
            major_axis: Vec2::new(20.0, 0.0),
            ratio: 0.5,
            ..Ellipse2::default()
        };
        let ellipse = shape_of(&entity_of(whole, &[]), ShapeKind::Polygon);
        let shape = ellipse.shape.as_ref().expect("a polygon");
        assert!(shape.chorded);
        for p in &shape.outline.vertices {
            near(
                (p.x / 20.0) * (p.x / 20.0) + (p.y / 10.0) * (p.y / 10.0),
                1.0,
                1e-9,
            );
        }
        let vertices = &shape.outline.vertices;
        assert!(vertices[0].distance_to(vertices[vertices.len() - 1]) > 1e-3);
        assert!(area(&shape.outline).abs() <= 200.0 * PI);
        assert!(area(&shape.outline).abs() >= 200.0 * PI - 0.15);
        assert_eq!(shape.heights, no_heights(vertices.len()));
    }

    #[test]
    fn fewer_than_three_corners_is_too_few_and_what_is_no_outline_is_said_so() {
        assert_eq!(
            shape_of(
                &entity_of(polyline(vec![point(0.0, 0.0), point(5.0, 0.0)], true), &[]),
                ShapeKind::Polygon
            )
            .why,
            NotAShape::TooFew
        );
        // Drawn back to its start on three vertices: two corners.
        assert_eq!(
            shape_of(
                &entity_of(
                    polyline(
                        vec![point(0.0, 0.0), point(5.0, 0.0), point(0.0, 0.0)],
                        false
                    ),
                    &[]
                ),
                ShapeKind::Polygon
            )
            .why,
            NotAShape::TooFew
        );
        let mark = PointGeometry {
            position: point(1.0, 1.0),
        };
        assert_eq!(
            shape_of(&entity_of(mark, &[]), ShapeKind::Polygon).why,
            NotAShape::NoOutline
        );
        let text = TextGeometry {
            text: String::from("lot 4"),
            ..TextGeometry::default()
        };
        assert_eq!(
            shape_of(&entity_of(text, &[]), ShapeKind::Polygon).why,
            NotAShape::NoOutline
        );
        let spline = must(Spline2::from_control_points(square(), 3));
        assert_eq!(
            shape_of(&entity_of(spline.clone(), &[]), ShapeKind::Polygon).why,
            NotAShape::NoOutline
        );
        assert_eq!(
            shape_of(&entity_of(spline, &[]), ShapeKind::Line).why,
            NotAShape::NoOutline
        );
    }

    #[test]
    fn as_a_line_each_kind_gives_its_vertices_in_order_and_a_closed_one_without_the_repeat() {
        let segment = shape_of(
            &entity_of(
                Segment2::new(point(0.0, 0.0), point(3.0, 4.0)),
                &[Some(1.0), Some(2.0)],
            ),
            ShapeKind::Line,
        );
        let shape = segment.shape.as_ref().expect("a line");
        assert_eq!(
            shape.outline.vertices,
            vec![point(0.0, 0.0), point(3.0, 4.0)]
        );
        assert!(!shape.outline.closed);
        assert_eq!(shape.heights, vec![Some(1.0), Some(2.0)]);

        // An open polyline back at its start stays as drawn: a line may end where
        // it began.
        let mut drawn = square();
        drawn.push(point(0.0, 0.0));
        let open = shape_of(
            &entity_of(polyline(drawn.clone(), false), &[]),
            ShapeKind::Line,
        );
        let shape = open.shape.as_ref().expect("a line");
        assert_eq!(shape.outline.vertices, drawn);
        assert!(!shape.outline.closed);
        // Closed and repeating its first vertex: closed, the square's four.
        let closed = shape_of(&entity_of(polyline(drawn, true), &[]), ShapeKind::Line);
        let shape = closed.shape.as_ref().expect("a line");
        assert_eq!(shape.outline.vertices, square());
        assert!(shape.outline.closed);

        // A curve polyline with an arc: chorded, from its first vertex to its
        // last.
        let curve = CurvePolyline2 {
            vertices: vec![
                CurveVertex {
                    position: point(0.0, 0.0),
                    bulge: 1.0,
                    height: Some(1.0),
                },
                CurveVertex {
                    position: point(10.0, 0.0),
                    bulge: 0.0,
                    height: Some(3.0),
                },
            ],
            closed: false,
        };
        let arc = shape_of(&entity_of(curve, &[]), ShapeKind::Line);
        let shape = arc.shape.as_ref().expect("a line");
        assert!(shape.chorded);
        let vertices = &shape.outline.vertices;
        assert!(vertices.len() > 2);
        assert_eq!(vertices[0], point(0.0, 0.0));
        assert_eq!(vertices[vertices.len() - 1], point(10.0, 0.0));
        assert_eq!(shape.heights[shape.heights.len() - 1], Some(3.0));

        assert_eq!(
            shape_of(
                &entity_of(Circle2::new(point(0.0, 0.0), 2.0), &[]),
                ShapeKind::Line
            )
            .why,
            NotAShape::NoOutline
        );
        assert_eq!(
            shape_of(&entity_of(half_turn(), &[]), ShapeKind::Line).why,
            NotAShape::NoOutline
        );
        assert_eq!(
            shape_of(
                &entity_of(polyline(vec![point(1.0, 1.0)], false), &[]),
                ShapeKind::Line
            )
            .why,
            NotAShape::TooFew
        );
    }

    #[test]
    fn a_typed_corner_is_two_or_three_finite_numbers() {
        let flat = parse_typed_corner("1,2").expect("a corner");
        assert_eq!(flat.at, point(1.0, 2.0));
        assert!(flat.z.is_none());
        let levelled = parse_typed_corner("1,2,-3.5").expect("a corner");
        assert_eq!(levelled.z, Some(-3.5));
        // A quoted word may carry blanks round its numbers.
        let spaced = parse_typed_corner("1.5e1, -2").expect("a corner");
        assert_eq!(spaced.at, point(15.0, -2.0));
        for word in [
            "", "1", "1,2,3,4", "a,b", "1,inf", "1,nan", "1,2,", ",1,2", "1;2",
        ] {
            assert!(parse_typed_corner(word).is_none(), "{word}");
        }
    }

    #[test]
    fn typed_corners_make_a_polygon_with_and_without_heights() {
        let corner = |x: f64, y: f64, z: Option<f64>| TypedCorner { at: point(x, y), z };
        let flat = vec![
            corner(0.0, 0.0, None),
            corner(4.0, 0.0, None),
            corner(4.0, 3.0, None),
        ];
        let triangle = shape_of_corners(&flat, ShapeKind::Polygon);
        let triangle = triangle.shape.as_ref().expect("a polygon");
        assert!(triangle.entity.is_none());
        assert!(triangle.outline.closed);
        assert_eq!(triangle.outline.vertices.len(), 3);
        assert_eq!(triangle.heights, no_heights(3));
        double_eq(area(&triangle.outline), 6.0); // 4 x 3 / 2

        let levelled = vec![
            corner(0.0, 0.0, Some(1.0)),
            corner(4.0, 0.0, None),
            corner(4.0, 3.0, Some(3.0)),
        ];
        let with_heights = shape_of_corners(&levelled, ShapeKind::Polygon);
        let with_heights = with_heights.shape.as_ref().expect("a polygon");
        assert_eq!(with_heights.heights, vec![Some(1.0), None, Some(3.0)]);

        // Typed back to the first corner: the same three.
        let mut closed_by_hand = flat.clone();
        closed_by_hand.push(corner(0.0, 0.0, None));
        let back = shape_of_corners(&closed_by_hand, ShapeKind::Polygon);
        let back = back.shape.as_ref().expect("a polygon");
        assert_eq!(back.outline.vertices, triangle.outline.vertices);

        // Two corners, and three of which the last is the first, are no polygon.
        assert_eq!(
            shape_of_corners(&[flat[0], flat[1]], ShapeKind::Polygon).why,
            NotAShape::TooFew
        );
        assert_eq!(
            shape_of_corners(&[flat[0], flat[1], flat[0]], ShapeKind::Polygon).why,
            NotAShape::TooFew
        );
        assert_eq!(
            shape_of_corners(&[], ShapeKind::Polygon).why,
            NotAShape::TooFew
        );
        // What is TooFew counts the corners LEFT once the closing repeat is off -
        // the number a refusal can quote without contradicting itself ("needs
        // three corners [corners=3]"): 0,0 4,0 0,0 is two, and 5,5 three times is
        // one.
        assert_eq!(
            shape_of_corners(&[flat[0], flat[1]], ShapeKind::Polygon).corners,
            2
        );
        assert_eq!(
            shape_of_corners(&[flat[0], flat[1], flat[0]], ShapeKind::Polygon).corners,
            2
        );
        let five = corner(5.0, 5.0, None);
        assert_eq!(
            shape_of_corners(&[five, five, five], ShapeKind::Polygon).corners,
            1
        );
        assert_eq!(shape_of_corners(&[], ShapeKind::Polygon).corners, 0);
        assert_eq!(shape_of_corners(&[five], ShapeKind::Line).corners, 1);

        // As a line: two vertices are one, open; one is none.
        let line = shape_of_corners(&[flat[0], flat[1]], ShapeKind::Line);
        let line = line.shape.as_ref().expect("a line");
        assert!(!line.outline.closed);
        assert_eq!(
            shape_of_corners(&[flat[0]], ShapeKind::Line).why,
            NotAShape::TooFew
        );
    }

    #[test]
    fn the_words_are_ids_corners_or_a_scope_and_the_reading_stops_where_they_end() {
        let words = words_of("#3 #12 EACH");
        let mut at = 0;
        let read = must(read_shape_words(&words, &mut at));
        assert_eq!(read.ids, vec![3, 12]);
        assert_eq!(at, 2);

        let words = words_of("1,2 3,4,5 bulk=2");
        let mut at = 0;
        let read = must(read_shape_words(&words, &mut at));
        assert_eq!(read.corners.len(), 2);
        assert_eq!(read.corners[1].z, Some(5.0));
        assert_eq!(at, 2);

        let words = words_of("FROM LAYERS a,b ONLY WHERE TYPE=polyline EACH");
        let mut at = 0;
        let read = must(read_shape_words(&words, &mut at));
        let scope = read.scope.as_ref().expect("a scope");
        assert_eq!(scope.source, ScopeSource::Layers);
        assert_eq!(scope.layers, vec![String::from("a"), String::from("b")]);
        assert!(!scope.sublayers);
        assert!(!scope.filter.types.is_empty());
        assert_eq!(words[at], "EACH");

        // FROM with no scope word is the selection, as the scope grammar has it.
        for line in ["FROM", "FROM EACH"] {
            let words = words_of(line);
            let mut at = 0;
            let read = must(read_shape_words(&words, &mut at));
            let scope = read.scope.as_ref().expect("a scope");
            assert_eq!(scope.source, ScopeSource::Selection, "{line}");
            assert_eq!(at, 1, "{line}");
        }

        // No word, or a word that is none of the three: nothing read.
        let words = words_of("EACH");
        let mut at = 1;
        assert!(must(read_shape_words(&words, &mut at)).empty());
        let mut at = 0;
        assert!(must(read_shape_words(&words, &mut at)).empty());
        assert_eq!(at, 0);
    }

    #[test]
    fn a_bad_id_is_given_back_for_the_verb_to_refuse_and_a_scopes_refusal_is_the_parsers() {
        for line in ["#5 #x", "#5 #0", "#5 #-3", "#5 #"] {
            let words = words_of(line);
            let mut at = 0;
            let read = must(read_shape_words(&words, &mut at));
            assert_eq!(read.ids, vec![5], "{line}");
            assert_eq!(read.bad_id, Some(words[1].clone()), "{line}");
            assert_eq!(at, 1, "{line}");
        }
        let two_scopes = words_of("FROM DRAWING SELECTION");
        let mut at = 0;
        let refused = read_shape_words(&two_scopes, &mut at).expect_err("two scopes");
        assert_eq!(refused.code, ErrorCode::InvalidArgument); // scope_verbs: one scope
        let bad_area = words_of("FROM AREA 1,2,3");
        let mut at = 0;
        let area3 = read_shape_words(&bad_area, &mut at).expect_err("three numbers");
        assert_eq!(area3.code, ErrorCode::ParseFailure);
    }

    // ---- a spline that returns to its start ------------------------------------------------

    #[test]
    fn a_closed_spline_is_a_polygon_chorded_within_the_tolerance_at_the_entitys_elevation() {
        // A circle of radius 10 about (3, 4) as a rational spline, exactly: every
        // corner of the walk is on it, and the polygon is its chords, whose area
        // is pi 100 less at most the perimeter (2 pi 10) times the 1 mm a chord
        // may stand off it - 0.063; 0.1 allows a chord's farthest point to be
        // off its middle.
        let centre = point(3.0, 4.0);
        let mut circle = entity_of(circle_spline(centre, 10.0), &[]);
        circle
            .properties
            .insert(String::from(ELEVATION_PROPERTY), PropertyValue::from(5.0));
        let ring = shape_of(&circle, ShapeKind::Polygon);
        let shape = ring.shape.as_ref().expect("a polygon");
        let corners = &shape.outline.vertices;
        assert!(corners.len() >= 8);
        assert!(shape.chorded);
        assert!(shape.outline.closed);
        assert!(corners[0].distance_to(corners[corners.len() - 1]) > 1e-3); // the repeat is taken off
        for corner in corners {
            near(corner.distance_to(centre), 10.0, 1e-9);
        }
        assert!(area(&shape.outline).abs() <= 100.0 * PI);
        assert!(area(&shape.outline).abs() >= 100.0 * PI - 0.1);
        // A spline holds no height; the entity's elevation is the one every
        // corner has.
        assert_eq!(shape.heights, vec![Some(5.0); corners.len()]);
        assert_eq!(shape.entity, Some(7));
        // As a line it is none: a line is a polyline, as before.
        assert_eq!(shape_of(&circle, ShapeKind::Line).why, NotAShape::NoOutline);
        // No elevation, no height: absent is not zero.
        let flat = shape_of(
            &entity_of(circle_spline(centre, 10.0), &[]),
            ShapeKind::Polygon,
        );
        let flat = flat.shape.as_ref().expect("a polygon");
        assert_eq!(flat.heights, no_heights(flat.outline.vertices.len()));
    }

    #[test]
    fn a_spline_is_closed_when_its_end_is_within_the_geometric_tolerance_of_its_start_and_not_otherwise()
     {
        // Through (0,0) (10,0) (10,10) (0,10) and back to x = end_x, 0: the end
        // 5e-8 from the start is within GEOMETRIC (1e-7) and the spline is
        // closed; 5e-7 away it is not, and is no outline, as every open spline
        // always was. An open one made of control points is none either.
        let through = |end_x: f64| {
            must(Spline2::through_points(
                vec![
                    point(0.0, 0.0),
                    point(10.0, 0.0),
                    point(10.0, 10.0),
                    point(0.0, 10.0),
                    point(end_x, 0.0),
                ],
                3,
            ))
        };
        let nearly = shape_of(&entity_of(through(5e-8), &[]), ShapeKind::Polygon);
        let shape = nearly.shape.as_ref().expect("a polygon");
        assert!(shape.outline.closed);
        assert!(shape.outline.vertices.len() >= 8);
        assert!(shape.chorded);
        let apart = shape_of(&entity_of(through(5e-7), &[]), ShapeKind::Polygon);
        assert!(apart.shape.is_none());
        assert_eq!(apart.why, NotAShape::NoOutline);
        // Exactly back at its start: closed as well.
        assert!(
            shape_of(&entity_of(through(0.0), &[]), ShapeKind::Polygon)
                .shape
                .is_some()
        );
        // The whole outline of a closed spline is the same outline whichever way
        // it is read: the first corner is the spline's own start.
        near(shape.outline.vertices[0].x, 0.0, 1e-9);
        near(shape.outline.vertices[0].y, 0.0, 1e-9);
    }
}

// ---- what the words name in a drawing -----------------------------------------------------------

struct InADrawing {
    document: Document,
}

impl InADrawing {
    fn new() -> Self {
        Self {
            document: Document::default(),
        }
    }

    fn add(&mut self, shape: impl Into<Geometry>) -> EntityId {
        self.add_entity(entity_of(shape, &[]))
    }

    // An entity as given, its metadata and properties with it.
    fn add_entity(&mut self, entity: Entity) -> EntityId {
        ok(self.document.execute(create_entities(vec![entity])));
        self.document
            .last_created_entities()
            .first()
            .copied()
            .unwrap_or(0)
    }

    fn resolved(
        &self,
        line: &str,
        kind: ShapeKind,
        options: &ShapeOptions,
    ) -> Result<DrawingShapes> {
        let words = words_of(line);
        let mut at = 0;
        let read = read_shape_words(&words, &mut at)?;
        assert_eq!(at, words.len(), "{line}");
        resolve_shapes(&self.document, &read, kind, None, options)
    }
}

fn ids(list: &[EntityId]) -> String {
    list.iter()
        .map(|id| format!("#{id}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn plain() -> ShapeOptions {
    ShapeOptions::default()
}

// ---- tagged holes ----------------------------------------------------------------------------

fn square_at(x: f64, y: f64, side: f64) -> Polyline2 {
    polyline(
        vec![
            point(x, y),
            point(x + side, y),
            point(x + side, y + side),
            point(x, y + side),
        ],
        true,
    )
}

// A ring as IMPORT leaves it: the tag in the entity's metadata, and the feature
// it came from when it has one.
fn tagged(ring: Polyline2, tag: &str, part: Option<&str>) -> Entity {
    let mut entity = entity_of(ring, &[]);
    entity
        .metadata
        .insert(String::from("source.ring"), PropertyValue::from(tag));
    if let Some(part) = part {
        entity
            .metadata
            .insert(String::from("source.part"), PropertyValue::from(part));
    }
    entity
}

fn joining() -> ShapeOptions {
    ShapeOptions {
        join_tagged_holes: true,
    }
}

mod drawing_shapes_in_a_drawing {
    use super::*;

    #[test]
    fn ids_are_read_in_the_order_typed_and_the_first_that_is_no_shape_refuses_the_line() {
        let mut drawing = InADrawing::new();
        let lot = drawing.add(polyline(square(), true));
        let pond = drawing.add(Circle2::new(point(30.0, 30.0), 4.0));
        let kerb = drawing.add(Segment2::new(point(0.0, 0.0), point(5.0, 5.0)));

        let both = must(drawing.resolved(&ids(&[pond, lot]), ShapeKind::Polygon, &plain()));
        assert_eq!(both.shapes.len(), 2);
        assert_eq!(both.shapes[0].entity, Some(pond));
        assert_eq!(both.shapes[1].entity, Some(lot));
        assert!(both.scope.is_none());
        assert!(both.refused.is_none());

        let open = must(drawing.resolved(&ids(&[lot, kerb]), ShapeKind::Polygon, &plain()));
        let refused = open.refused.expect("a refusal");
        assert_eq!(refused.entity, Some(kerb));
        assert_eq!(refused.why, NotAShape::Open);

        // The same line is a line.
        let line = must(drawing.resolved(&ids(&[kerb]), ShapeKind::Line, &plain()));
        assert!(line.refused.is_none());
        assert_eq!(line.shapes.len(), 1);
    }

    #[test]
    fn an_id_of_no_entity_is_refused_before_any_id_is_read_as_a_shape() {
        let mut drawing = InADrawing::new();
        let kerb = drawing.add(Segment2::new(point(0.0, 0.0), point(5.0, 5.0)));
        let refused = must(drawing.resolved(&ids(&[kerb, 999]), ShapeKind::Polygon, &plain()));
        let refusal = refused.refused.expect("a refusal");
        assert_eq!(refusal.entity, Some(999));
        assert_eq!(refusal.why, NotAShape::NoEntity);
        assert!(refused.shapes.is_empty());
    }

    #[test]
    fn typed_corners_are_one_shape_or_refused_as_too_few() {
        let drawing = InADrawing::new();
        let triangle = must(drawing.resolved("0,0,1 4,0,2 4,3,3", ShapeKind::Polygon, &plain()));
        assert_eq!(triangle.shapes.len(), 1);
        assert_eq!(
            triangle.shapes[0].heights,
            vec![Some(1.0), Some(2.0), Some(3.0)]
        );
        assert!(triangle.shapes[0].entity.is_none());

        let two = must(drawing.resolved("0,0 4,0", ShapeKind::Polygon, &plain()));
        let refusal = two.refused.expect("a refusal");
        assert!(refusal.entity.is_none());
        assert_eq!(refusal.why, NotAShape::TooFew);
        assert_eq!(refusal.corners, 2);
        assert!(two.shapes.is_empty());

        // Typed back to the first corner of two: the refusal counts two, not
        // three.
        let closed = must(drawing.resolved("0,0 4,0 0,0", ShapeKind::Polygon, &plain()));
        assert_eq!(closed.refused.expect("a refusal").corners, 2);
    }

    #[test]
    fn a_scope_takes_the_shapes_and_counts_what_is_none_by_why() {
        let mut drawing = InADrawing::new();
        let lot = drawing.add(polyline(square(), true)); // polygon
        let pond = drawing.add(Circle2::new(point(30.0, 30.0), 4.0)); // polygon
        let whole = Ellipse2 {
            center: point(60.0, 0.0),
            major_axis: Vec2::new(20.0, 0.0),
            ratio: 0.5,
            ..Ellipse2::default()
        };
        let basin = drawing.add(whole); // polygon
        drawing.add(polyline(
            vec![point(0.0, 20.0), point(10.0, 20.0), point(10.0, 30.0)],
            false,
        )); // open
        drawing.add(Segment2::new(point(0.0, 40.0), point(5.0, 45.0))); // open
        drawing.add(Arc2 {
            center: point(0.0, 50.0),
            ..half_turn()
        }); // open
        drawing.add(polyline(vec![point(0.0, 60.0), point(5.0, 60.0)], true)); // too few
        drawing.add(PointGeometry {
            position: point(1.0, 1.0),
        }); // no outline

        let polygons = must(drawing.resolved("FROM DRAWING", ShapeKind::Polygon, &plain()));
        let scope = polygons.scope.as_ref().expect("a scope");
        assert_eq!(scope.matched.len(), 8);
        assert_eq!(polygons.shapes.len(), 3);
        assert_eq!(polygons.shapes[0].entity, Some(lot)); // ascending ids
        assert_eq!(polygons.shapes[1].entity, Some(pond));
        assert_eq!(polygons.shapes[2].entity, Some(basin));
        assert_eq!(polygons.skipped.open, 3);
        assert_eq!(polygons.skipped.too_few, 1);
        assert_eq!(polygons.skipped.no_outline, 1);
        assert_eq!(polygons.skipped.total(), 5);
        assert!(polygons.refused.is_none()); // a scope refuses nothing

        // As lines: the three polylines and the line; the circle, the ellipse,
        // the arc and the point are none.
        let lines = must(drawing.resolved("FROM DRAWING", ShapeKind::Line, &plain()));
        assert_eq!(lines.shapes.len(), 4);
        assert_eq!(lines.skipped.no_outline, 4);
        assert_eq!(lines.skipped.open + lines.skipped.too_few, 0);
    }

    #[test]
    fn a_scope_that_takes_nothing_is_reported_and_not_refused() {
        let mut drawing = InADrawing::new();
        drawing.add(polyline(square(), true));
        let nothing = must(drawing.resolved(
            "FROM AREA 1000,1000,1010,1010",
            ShapeKind::Polygon,
            &plain(),
        ));
        let scope = nothing.scope.as_ref().expect("a scope");
        assert!(scope.matched.is_empty());
        assert!(nothing.shapes.is_empty());
        assert_eq!(nothing.skipped.total(), 0);
        assert!(nothing.refused.is_none());
    }

    #[test]
    fn from_with_no_scope_word_is_the_selection() {
        let mut drawing = InADrawing::new();
        drawing.add(polyline(square(), true));
        let pond = drawing.add(Circle2::new(point(30.0, 30.0), 4.0));
        drawing.document.selection_mut().set(vec![pond]);
        let selected = must(drawing.resolved("FROM", ShapeKind::Polygon, &plain()));
        assert_eq!(selected.shapes.len(), 1);
        assert_eq!(selected.shapes[0].entity, Some(pond));
        assert_eq!(
            selected
                .scope
                .as_ref()
                .expect("a scope")
                .resolved
                .words
                .source,
            ScopeSource::Selection
        );
    }

    #[test]
    fn a_view_scope_with_no_window_is_the_matchers_refusal() {
        let mut drawing = InADrawing::new();
        drawing.add(polyline(square(), true));
        let refused = drawing
            .resolved("FROM VIEW", ShapeKind::Polygon, &plain())
            .expect_err("VIEW headless");
        assert_eq!(refused.code, ErrorCode::InvalidState); // scope_verbs: VIEW headless
    }

    #[test]
    fn the_entities_named_are_what_a_verb_reading_points_takes() {
        let mut drawing = InADrawing::new();
        let lot = drawing.add(polyline(square(), true));
        let mark = drawing.add(PointGeometry {
            position: point(1.0, 1.0),
        });
        let words = words_of("FROM DRAWING");
        let mut at = 0;
        let scope = must(read_shape_words(&words, &mut at));
        let all = must(entities_named(&drawing.document, &scope, None));
        assert_eq!(all.ids, vec![lot, mark]);
        assert!(all.scope.is_some());
        assert!(all.missing.is_none());

        let words = words_of(&ids(&[mark, 999, lot]));
        let mut at = 0;
        let typed = must(read_shape_words(&words, &mut at));
        let named = must(entities_named(&drawing.document, &typed, None));
        assert_eq!(named.missing, Some(999));
        assert_eq!(named.ids, vec![mark]);
        assert!(named.scope.is_none());
    }

    #[test]
    fn a_tagged_ring_joins_the_polygon_it_lies_in_when_asked_and_is_a_polygon_of_its_own_when_not()
    {
        // A lot of 100 x 100 with a hole of 20 x 20 in it, the hole drawn second.
        let mut drawing = InADrawing::new();
        let lot = drawing.add_entity(tagged(square_at(0.0, 0.0, 100.0), "exterior", None));
        let hole = drawing.add_entity(tagged(square_at(40.0, 40.0, 20.0), "hole", None));

        // Not asked: two rings, as the surface edits read them.
        let plain_read = must(drawing.resolved(&ids(&[lot, hole]), ShapeKind::Polygon, &plain()));
        assert_eq!(plain_read.shapes.len(), 2);
        assert!(plain_read.shapes[0].holes.is_empty());
        assert_eq!(plain_read.holes_joined, 0);
        assert_eq!(plain_read.holes_alone, 0);

        // Asked: the lot, with the hole in it.
        let joined = must(drawing.resolved(&ids(&[lot, hole]), ShapeKind::Polygon, &joining()));
        assert_eq!(joined.shapes.len(), 1);
        assert_eq!(joined.shapes[0].entity, Some(lot));
        assert_eq!(joined.shapes[0].holes.len(), 1);
        assert_eq!(
            joined.shapes[0].holes[0].vertices,
            square_at(40.0, 40.0, 20.0).vertices
        );
        assert!(joined.shapes[0].holes[0].closed);
        assert_eq!(joined.shapes[0].hole_entities, vec![hole]);
        assert_eq!(joined.holes_joined, 1);
        assert_eq!(joined.holes_alone, 0);

        // The hole named first is joined all the same: the exteriors are all
        // read before any hole is given to one.
        let first = must(drawing.resolved(&ids(&[hole, lot]), ShapeKind::Polygon, &joining()));
        assert_eq!(first.shapes.len(), 1);
        assert_eq!(first.shapes[0].entity, Some(lot));
        assert_eq!(first.shapes[0].hole_entities, vec![hole]);

        // By a scope, with another polygon in it that has no hole.
        let pond = drawing.add_entity(entity_of(Circle2::new(point(300.0, 300.0), 4.0), &[]));
        let scoped = must(drawing.resolved("FROM DRAWING", ShapeKind::Polygon, &joining()));
        assert_eq!(scoped.shapes.len(), 2);
        assert_eq!(scoped.shapes[0].entity, Some(lot));
        assert_eq!(scoped.shapes[0].holes.len(), 1);
        assert_eq!(scoped.shapes[1].entity, Some(pond));
        assert!(scoped.shapes[1].holes.is_empty());
        assert_eq!(scoped.scope.as_ref().expect("a scope").matched.len(), 3);
    }

    #[test]
    fn a_ring_not_tagged_as_a_hole_is_not_one_however_it_lies() {
        // A building inside a lot is not a hole in the lot, and a ring tagged as
        // an exterior is not a hole either.
        let mut drawing = InADrawing::new();
        let lot = drawing.add_entity(entity_of(square_at(0.0, 0.0, 100.0), &[]));
        let building = drawing.add_entity(entity_of(square_at(40.0, 40.0, 20.0), &[]));
        let other = drawing.add_entity(tagged(square_at(10.0, 10.0, 5.0), "exterior", None));
        let read = must(drawing.resolved(
            &ids(&[lot, building, other]),
            ShapeKind::Polygon,
            &joining(),
        ));
        assert_eq!(read.shapes.len(), 3);
        for shape in &read.shapes {
            assert!(shape.holes.is_empty());
        }
        assert_eq!(read.holes_joined, 0);
        assert_eq!(read.holes_alone, 0);
    }

    #[test]
    fn a_tagged_ring_that_no_polygon_read_holds_stays_a_polygon_of_its_own_and_is_counted() {
        let mut drawing = InADrawing::new();
        let lot = drawing.add_entity(tagged(square_at(0.0, 0.0, 100.0), "exterior", None));
        let away = drawing.add_entity(tagged(square_at(200.0, 0.0, 20.0), "hole", None)); // in no ring
        let straddling = drawing.add_entity(tagged(square_at(90.0, 40.0, 20.0), "hole", None)); // half out of the lot
        let read = must(drawing.resolved(
            &ids(&[lot, away, straddling]),
            ShapeKind::Polygon,
            &joining(),
        ));
        assert_eq!(read.shapes.len(), 3);
        assert!(read.shapes[0].holes.is_empty());
        assert_eq!(read.holes_joined, 0);
        assert_eq!(read.holes_alone, 2);

        // Named alone, a hole is the polygon asked for.
        let alone = must(drawing.resolved(&ids(&[away]), ShapeKind::Polygon, &joining()));
        assert_eq!(alone.shapes.len(), 1);
        assert_eq!(alone.shapes[0].entity, Some(away));
        assert_eq!(alone.holes_alone, 1);
        assert!(alone.refused.is_none());
    }

    #[test]
    fn a_hole_joins_the_exterior_of_its_own_feature_then_the_smallest_that_holds_it() {
        // The big lot (100 x 100) of feature "a" holds the small (60 x 60) of
        // feature "b", and a hole of 10 x 10 lies in both. Of feature "a" it
        // joins the big, of "b" the small, and of neither the smallest.
        let mut drawing = InADrawing::new();
        let big = drawing.add_entity(tagged(square_at(0.0, 0.0, 100.0), "exterior", Some("a")));
        let small = drawing.add_entity(tagged(square_at(20.0, 20.0, 60.0), "exterior", Some("b")));
        let of_a = drawing.add_entity(tagged(square_at(40.0, 40.0, 10.0), "hole", Some("a")));
        let a = must(drawing.resolved(&ids(&[big, small, of_a]), ShapeKind::Polygon, &joining()));
        assert_eq!(a.shapes.len(), 2);
        assert_eq!(a.shapes[0].entity, Some(big));
        assert_eq!(a.shapes[0].hole_entities, vec![of_a]);
        assert!(a.shapes[1].holes.is_empty());

        let of_none = drawing.add_entity(tagged(square_at(60.0, 60.0, 10.0), "hole", Some("z")));
        let none =
            must(drawing.resolved(&ids(&[big, small, of_none]), ShapeKind::Polygon, &joining()));
        assert_eq!(none.shapes.len(), 2);
        assert!(none.shapes[0].holes.is_empty());
        assert_eq!(none.shapes[1].hole_entities, vec![of_none]);
    }

    #[test]
    fn lines_and_typed_corners_have_no_holes_to_join() {
        let mut drawing = InADrawing::new();
        let lot = drawing.add_entity(tagged(square_at(0.0, 0.0, 100.0), "exterior", None));
        let hole = drawing.add_entity(tagged(square_at(40.0, 40.0, 20.0), "hole", None));
        // As lines they are the two rings' vertices, joined to nothing.
        let lines = must(drawing.resolved(&ids(&[lot, hole]), ShapeKind::Line, &joining()));
        assert_eq!(lines.shapes.len(), 2);
        assert_eq!(lines.holes_joined, 0);
        // Typed corners are a polygon of their own.
        let typed = must(drawing.resolved("0,0 10,0 10,10", ShapeKind::Polygon, &joining()));
        assert_eq!(typed.shapes.len(), 1);
        assert!(typed.shapes[0].holes.is_empty());
        assert_eq!(typed.holes_joined + typed.holes_alone, 0);
    }
}
