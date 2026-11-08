// Point symbols (tests/cad/test_symbols.cpp): one definition of each shape,
// in model units, for every renderer.

use crate::near;
use katana_cad::symbols::symbol_strokes;
use katana_entity::tables::{NO_SYMBOL, symbol_names};
use katana_geometry::Point2;
use katana_math::numerics::PI;

fn radius(vertex: Point2, centre: Point2) -> f64 {
    (vertex.x - centre.x).hypot(vertex.y - centre.y)
}

fn origin() -> Point2 {
    Point2::new(0.0, 0.0)
}

mod symbols {
    use super::*;

    #[test]
    fn every_named_symbol_has_its_own_shape_and_any_other_name_falls_back_to_one() {
        for &name in symbol_names() {
            let strokes = symbol_strokes(name, origin(), 1.0, 0.0);
            assert!(!strokes.is_empty(), "{name}");
            for stroke in &strokes {
                assert!(stroke.vertices.len() >= 2, "{name}");
                // Every shape fits the unit square: a symbol that reaches
                // outside its own size would overlap the text beside it.
                for vertex in &stroke.vertices {
                    assert!(vertex.x.abs() <= 1.0 + 1e-12, "{name}");
                    assert!(vertex.y.abs() <= 1.0 + 1e-12, "{name}");
                }
            }
        }
        // Only "no symbol" draws nothing. A name that is not one of the
        // sixteen is a real library linestyle name - "SEWR Manhole Cover" -
        // and falls back to the shape its words suggest, because the
        // alternative is a point that draws nothing at all when no library
        // defines its name.
        assert!(symbol_strokes(NO_SYMBOL, origin(), 1.0, 0.0).is_empty());
        assert!(!symbol_strokes("SEWR Manhole Cover", origin(), 1.0, 0.0).is_empty());
        assert_eq!(
            symbol_strokes("SEWR Manhole Cover", origin(), 1.0, 0.0),
            symbol_strokes("manhole", origin(), 1.0, 0.0)
        );
        // A name suggesting nothing still draws: a circle is the last resort.
        assert_eq!(
            symbol_strokes("blob", origin(), 1.0, 0.0),
            symbol_strokes("circle", origin(), 1.0, 0.0)
        );
    }

    #[test]
    fn a_circle_is_chords_on_the_circle_of_the_given_size_about_the_centre() {
        let centre = Point2::new(100.0, -50.0);
        let strokes = symbol_strokes("circle", centre, 2.5, 0.0);
        assert_eq!(strokes.len(), 1);
        assert!(strokes[0].closed);
        assert_eq!(strokes[0].vertices.len(), 24);
        for &vertex in &strokes[0].vertices {
            near(radius(vertex, centre), 2.5, 1e-12);
        }
        // 24 chords of a circle: consecutive vertices 15 degrees apart.
        let a = strokes[0].vertices[0];
        let b = strokes[0].vertices[1];
        near(
            (b.y - centre.y).atan2(b.x - centre.x) - (a.y - centre.y).atan2(a.x - centre.x),
            PI / 12.0,
            1e-12,
        );
    }

    #[test]
    fn a_cross_is_the_two_diagonals_of_the_square() {
        let strokes = symbol_strokes("cross", Point2::new(10.0, 20.0), 3.0, 0.0);
        assert_eq!(strokes.len(), 2);
        assert!(!strokes[0].closed);
        assert_eq!(strokes[0].vertices[0], Point2::new(7.0, 17.0));
        assert_eq!(strokes[0].vertices[1], Point2::new(13.0, 23.0));
        assert_eq!(strokes[1].vertices[0], Point2::new(7.0, 23.0));
        assert_eq!(strokes[1].vertices[1], Point2::new(13.0, 17.0));
    }

    #[test]
    fn rotation_turns_the_shape_counter_clockwise_about_the_centre() {
        // A quarter turn takes the arrow's tip from east to north.
        let centre = Point2::new(5.0, 5.0);
        let east = symbol_strokes("arrow", centre, 2.0, 0.0);
        let north = symbol_strokes("arrow", centre, 2.0, PI / 2.0);
        assert_eq!(east.len(), north.len());
        let east_tip = east[0].vertices[east[0].vertices.len() - 1];
        let north_tip = north[0].vertices[north[0].vertices.len() - 1];
        near(east_tip.x, 7.0, 1e-12);
        near(east_tip.y, 5.0, 1e-12);
        near(north_tip.x, 5.0, 1e-12);
        near(north_tip.y, 7.0, 1e-12);
        // Rotation moves nothing closer to or further from the centre.
        for (east_stroke, north_stroke) in east.iter().zip(&north) {
            assert_eq!(east_stroke.vertices.len(), north_stroke.vertices.len());
            for (&e, &n) in east_stroke.vertices.iter().zip(&north_stroke.vertices) {
                near(radius(e, centre), radius(n, centre), 1e-12);
            }
        }
    }

    #[test]
    fn size_zero_draws_nothing_so_the_caller_must_choose_a_mark_size() {
        // Documented in symbols: 0 means "the viewport's own mark", which is
        // the caller's to size in pixels. Everything collapses onto the
        // centre.
        for stroke in symbol_strokes("target", Point2::new(1.0, 2.0), 0.0, 0.0) {
            for vertex in stroke.vertices {
                assert_eq!(vertex, Point2::new(1.0, 2.0));
            }
        }
    }
}
