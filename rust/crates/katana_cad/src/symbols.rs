//! The shapes a point symbol is drawn with (include/katana/cad/symbols.hpp).
//!
//! A symbol is a named mark at a point: a circle, a cross, a tree, a manhole.
//! `entity::symbol_names()` is the set; this is what each looks like, as plain
//! strokes in model units, so that the viewport, the plotter and a preview all
//! paint the same thing from one definition. Everything is polylines - a
//! circle is chorded here at a fixed count, since a symbol is small on any
//! output and the eye cannot tell 24 chords from a circle at that size.
//!
//! The unit shape fits the square from -1 to 1 about the origin; `size` is the
//! model-unit half-width it is scaled to (half of Style::symbol_size, which is
//! a width the way a library symbol's is), `rotation` radians
//! counter-clockwise. A size of 0 means "the viewport's own mark", and the
//! caller decides what that is in pixels - a symbol with no size is not drawn
//! at no size.

use katana_entity::tables::built_in_symbol_for;
use katana_geometry::{Point2, Polyline2};
use katana_math::numerics::PI;

// Enough that a symbol, which is small on any output, reads as round.
const CIRCLE_CHORDS: i32 = 24;

// Strokes in the unit square, before scale and rotation.
#[derive(Default)]
struct Unit {
    strokes: Vec<Polyline2>,
}

impl Unit {
    fn line(&mut self, x0: f64, y0: f64, x1: f64, y1: f64) {
        self.strokes.push(Polyline2::new(
            vec![Point2::new(x0, y0), Point2::new(x1, y1)],
            false,
        ));
    }

    fn ring(&mut self, cx: f64, cy: f64, r: f64) {
        let mut circle = Polyline2::new(Vec::new(), true);
        for i in 0..CIRCLE_CHORDS {
            let a = 2.0 * PI * f64::from(i) / f64::from(CIRCLE_CHORDS);
            circle
                .vertices
                .push(Point2::new(cx + r * a.cos(), cy + r * a.sin()));
        }
        self.strokes.push(circle);
    }

    fn polygon(&mut self, points: &[Point2]) {
        self.strokes.push(Polyline2::new(points.to_vec(), true));
    }
}

fn unit_shape(symbol: &str) -> Unit {
    let mut u = Unit::default();
    match symbol {
        "circle" => u.ring(0.0, 0.0, 1.0),
        "ring" => {
            u.ring(0.0, 0.0, 1.0);
            u.ring(0.0, 0.0, 0.5);
        }
        "dot" => {
            u.ring(0.0, 0.0, 0.35);
            u.ring(0.0, 0.0, 0.2);
            u.ring(0.0, 0.0, 0.05);
        }
        "square" => u.polygon(&[
            Point2::new(-1.0, -1.0),
            Point2::new(1.0, -1.0),
            Point2::new(1.0, 1.0),
            Point2::new(-1.0, 1.0),
        ]),
        "triangle" => u.polygon(&[
            Point2::new(-1.0, -0.75),
            Point2::new(1.0, -0.75),
            Point2::new(0.0, 1.0),
        ]),
        "diamond" => u.polygon(&[
            Point2::new(0.0, -1.0),
            Point2::new(1.0, 0.0),
            Point2::new(0.0, 1.0),
            Point2::new(-1.0, 0.0),
        ]),
        "cross" => {
            u.line(-1.0, -1.0, 1.0, 1.0);
            u.line(-1.0, 1.0, 1.0, -1.0);
        }
        "plus" => {
            u.line(-1.0, 0.0, 1.0, 0.0);
            u.line(0.0, -1.0, 0.0, 1.0);
        }
        "tick" => {
            u.line(-0.8, 0.0, -0.2, -0.8);
            u.line(-0.2, -0.8, 1.0, 0.9);
        }
        "star" => {
            for i in 0..4 {
                let a = PI * f64::from(i) / 4.0;
                u.line(-a.cos(), -a.sin(), a.cos(), a.sin());
            }
        }
        "tree" => {
            // A trunk and a crown, the way a survey plan draws one.
            u.line(0.0, -1.0, 0.0, -0.2);
            u.ring(0.0, 0.3, 0.7);
        }
        "pole" => {
            // A circle with a bar through it: the plan symbol of a pole.
            u.ring(0.0, 0.0, 0.5);
            u.line(-1.0, 0.0, 1.0, 0.0);
        }
        "manhole" => {
            u.ring(0.0, 0.0, 1.0);
            u.line(-0.7, -0.7, 0.7, 0.7);
            u.line(-0.7, 0.7, 0.7, -0.7);
        }
        "arrow" => {
            u.line(-1.0, 0.0, 1.0, 0.0);
            u.line(0.4, 0.5, 1.0, 0.0);
            u.line(0.4, -0.5, 1.0, 0.0);
        }
        "flag" => {
            u.line(-0.6, -1.0, -0.6, 1.0);
            u.polygon(&[
                Point2::new(-0.6, 1.0),
                Point2::new(0.8, 0.6),
                Point2::new(-0.6, 0.2),
            ]);
        }
        "target" => {
            // The survey mark: a circle with a cross that reaches past it.
            u.ring(0.0, 0.0, 0.6);
            u.line(-1.0, 0.0, 1.0, 0.0);
            u.line(0.0, -1.0, 0.0, 1.0);
        }
        _ => {}
    }
    u
}

/// The strokes of `symbol` at `centre`, `size` the half-width and `rotation`
/// radians counter-clockwise (the C++ default rotation is 0). Empty for a name
/// that is not a symbol (NO_SYMBOL among them): the caller draws its plain
/// mark instead.
pub fn symbol_strokes(symbol: &str, centre: Point2, size: f64, rotation: f64) -> Vec<Polyline2> {
    // Exact first, then the shape the name's words suggest. Katana used to
    // store the GUESS on the style and throw the real name away, which meant
    // a loaded symbol library could never be matched against it. The real
    // name is kept now, so the guess belongs here - at the last moment, when
    // nothing better has answered.
    let mut unit = unit_shape(symbol);
    if unit.strokes.is_empty() && !symbol.is_empty() {
        unit = unit_shape(built_in_symbol_for(symbol));
    }
    let c = rotation.cos();
    let s = rotation.sin();
    for stroke in &mut unit.strokes {
        for vertex in &mut stroke.vertices {
            let x = vertex.x * size;
            let y = vertex.y * size;
            *vertex = Point2::new(centre.x + x * c - y * s, centre.y + x * s + y * c);
        }
    }
    unit.strokes
}
