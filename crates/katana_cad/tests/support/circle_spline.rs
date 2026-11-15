//! A spline that is exactly a circle, for the tests of the closed outlines the
//! drawing's polygons are read from (tests/cad/terrain/circle_spline.hpp; used
//! by the twins of test_drawing_shapes.cpp and test_volume_verbs.cpp).

use katana_geometry::{Point2, Spline2};

// The control points' offsets from the centre, in radii: the nine corners and
// mid-sides of the square round the circle, counter-clockwise from (1, 0).
const OFFSETS: [[f64; 2]; 9] = [
    [1.0, 0.0],
    [1.0, 1.0],
    [0.0, 1.0],
    [-1.0, 1.0],
    [-1.0, 0.0],
    [-1.0, -1.0],
    [0.0, -1.0],
    [1.0, -1.0],
    [1.0, 0.0],
];

/// A rational quadratic spline that is a circle of radius `radius` about
/// `centre`: the nine control points of the square round the circle, weights 1
/// and sqrt(1/2) alternately, the knots at the quarters (Piegl and Tiller, The
/// NURBS Book, 2nd edition, section 7.5, the full circle). It starts and ends
/// at (centre.x + radius, centre.y) and runs counter-clockwise, so it is a
/// closed shape (Spline2::is_closed_shape) of area pi radius^2.
pub fn circle_spline(centre: Point2, radius: f64) -> Spline2 {
    let corner = 0.5_f64.sqrt();
    let mut spline = Spline2 {
        degree: 2,
        ..Spline2::default()
    };
    for (i, offset) in OFFSETS.iter().enumerate() {
        spline.control_points.push(Point2::new(
            centre.x + radius * offset[0],
            centre.y + radius * offset[1],
        ));
        spline.weights.push(if i % 2 == 1 { corner } else { 1.0 });
    }
    spline.knots = vec![
        0.0, 0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1.0, 1.0, 1.0,
    ];
    spline
}
