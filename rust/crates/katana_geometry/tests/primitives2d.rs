#![allow(
    clippy::excessive_precision,
    clippy::approx_constant,
    reason = "Preserve decimal constants and expected values copied verbatim from the C++ oracle."
)]
#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

use katana_geometry::{Box2, Point2};

#[path = "cases/alignment.rs"]
mod alignment;
#[path = "cases/chording.rs"]
mod chording;
#[path = "cases/curves2d.rs"]
mod curves2d;
#[path = "cases/editing.rs"]
mod editing;
#[path = "cases/intersection.rs"]
mod intersection;
#[path = "cases/mesh.rs"]
mod mesh;
#[path = "cases/point_batch.rs"]
mod point_batch;
#[path = "cases/point_splat.rs"]
mod point_splat;
#[path = "cases/polygon.rs"]
mod polygon;
#[path = "cases/polyline_vertices.rs"]
mod polyline_vertices;
#[path = "cases/primitives2d.rs"]
mod primitives2d;
#[path = "cases/primitives3d.rs"]
mod primitives3d;
#[path = "cases/profile.rs"]
mod profile;
#[path = "cases/spatial_index.rs"]
mod spatial_index;
#[path = "cases/spiral2.rs"]
mod spiral2;
mod support;

mod geometry_box2 {
    use super::*;
    use crate::support::double_eq;

    #[test]
    fn empty_expand_contain_intersect() {
        let mut box_value = Box2::default();
        assert!(box_value.empty());
        double_eq(box_value.width(), 0.0);
        box_value.expand(Point2::new(1.0, 2.0));
        box_value.expand(Point2::new(-3.0, 5.0));
        assert_eq!(
            box_value,
            Box2::new(Point2::new(-3.0, 2.0), Point2::new(1.0, 5.0))
        );
        double_eq(box_value.width(), 4.0);
        double_eq(box_value.height(), 3.0);
        assert!(box_value.contains(Point2::new(1.0, 5.0)));
        assert!(box_value.intersects(Box2::new(Point2::new(1.0, 5.0), Point2::new(9.0, 9.0))));
        assert!(!box_value.intersects(Box2::default()));
        assert!(box_value.inflated(1.0).contains(Point2::new(2.0, 6.0)));
        assert!(Box2::default().inflated(1.0).empty());
    }
}
