use crate::support::simd::*;
use katana_core::cpu_features::SimdLevel;
use katana_geometry::point_batch::*;
use katana_geometry::{Box2, Point2, Polyline2, TriangleMesh};
use katana_math::primitives::AABB as Aabb;
use katana_math::{Mat3, Mat4, Vec3};
fn same(first: f64, second: f64) -> bool {
    if first.is_nan() || second.is_nan() {
        first.is_nan() && second.is_nan()
    } else {
        first.to_bits() == second.to_bits()
    }
}
fn same2(first: Point2, second: Point2) -> bool {
    same(first.x, second.x) && same(first.y, second.y)
}
fn same3(first: Vec3, second: Vec3) -> bool {
    same(first.x, second.x) && same(first.y, second.y) && same(first.z, second.z)
}
fn same_box(first: Box2, second: Box2) -> bool {
    same2(first.min, second.min) && same2(first.max, second.max)
}
fn same_aabb(first: Aabb, second: Aabb) -> bool {
    same3(first.min, second.min) && same3(first.max, second.max)
}
struct Random {
    state: u64,
}
impl Default for Random {
    fn default() -> Self {
        Self {
            state: 0x9E3779B97F4A7C15,
        }
    }
}
impl Random {
    fn next(&mut self) -> u64 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        self.state
    }
    fn uniform(&mut self, low: f64, high: f64) -> f64 {
        low + (high - low) * (self.next() >> 11) as f64 * (1.0 / 9007199254740992.0)
    }
    fn coordinate(&mut self, center: f64) -> f64 {
        match self.next() % 16 {
            0 => 0.0,
            1 => -0.0,
            2 => f64::NAN,
            3 => {
                if self.next().is_multiple_of(2) {
                    f64::INFINITY
                } else {
                    f64::NEG_INFINITY
                }
            }
            _ => center + self.uniform(-5000.0, 5000.0),
        }
    }
}
const SHORT_FLAT: [Point2; 3] = [
    Point2 { x: 2.0, y: 3.0 },
    Point2 { x: -1.0, y: 5.0 },
    Point2 { x: 4.0, y: -2.0 },
];
const SHORT_SOLID: [Vec3; 3] = [
    Vec3 {
        x: 2.0,
        y: 3.0,
        z: 1.0,
    },
    Vec3 {
        x: -1.0,
        y: 5.0,
        z: 0.5,
    },
    Vec3 {
        x: 4.0,
        y: -2.0,
        z: 6.0,
    },
];
const _: () = {
    assert!(SHORT_FLAT.len() < BOUNDS_BATCH_MINIMUM);
    let flat = bounds_of_2d_inline(&SHORT_FLAT);
    assert!(flat.min.x == -1.0 && flat.min.y == -2.0 && flat.max.x == 4.0 && flat.max.y == 5.0);
    let solid = bounds_of_3d_inline(&SHORT_SOLID);
    assert!(solid.min.x == -1.0 && solid.min.y == -2.0 && solid.min.z == 0.5);
    assert!(solid.max.x == 4.0 && solid.max.y == 5.0 && solid.max.z == 6.0);
};
mod point_batch {
    use super::*;
    #[test]
    fn a3d_transform_gives_the_values_worked_by_hand_at_every_level() {
        let matrix = Mat4::translation(Vec3::new(10.0, 20.0, 30.0))
            * Mat4::scaling(Vec3::new(2.0, 2.0, 2.0));
        for level in simd_levels() {
            let mut points: Vec<_> = (0..=10)
                .map(|index| Vec3::new(index as f64, -(index as f64), 0.5 * index as f64))
                .collect();
            at_simd_level(level, || transform_points(&matrix, &mut points));
            for (index, point) in points.iter().enumerate() {
                assert_eq!(
                    *point,
                    Vec3::new(
                        2.0 * index as f64 + 10.0,
                        -2.0 * index as f64 + 20.0,
                        index as f64 + 30.0
                    )
                );
            }
        }
    }
    #[test]
    fn a2d_transform_gives_the_values_worked_by_hand_at_every_level() {
        let matrix = Mat3 {
            data: [0.0, -1.0, 5.0, 1.0, 0.0, 7.0, 0.0, 0.0, 1.0],
        };
        for level in simd_levels() {
            let mut points: Vec<_> = (0..=10)
                .map(|index| Point2::new(index as f64, 2.0 * index as f64))
                .collect();
            at_simd_level(level, || transform_points(&matrix, &mut points));
            for (index, point) in points.iter().enumerate() {
                assert_eq!(
                    *point,
                    Point2::new(5.0 - 2.0 * index as f64, index as f64 + 7.0)
                );
            }
        }
    }
    #[test]
    fn transforms_equal_transform_point_on_every_point_bit_for_bit_at_every_level() {
        let kernel = require_kernel();
        let mut random = Random::default();
        for round in 0..300 {
            let count = round % 71;
            let shift = Vec3::new(random.uniform(-1e5, 1e5), random.uniform(-1e5, 1e5), 3.0);
            let axis = Vec3::new(random.uniform(-1.0, 1.0), random.uniform(-1.0, 1.0), 1.0);
            let matrix4 = Mat4::translation(shift)
                * Mat4::rotation(axis, random.uniform(-3.0, 3.0))
                * Mat4::scaling(Vec3::new(1.0001, 0.9999, 1.5));
            let shift2 = Point2::new(random.uniform(-1e5, 1e5), random.uniform(-1e5, 1e5));
            let matrix3 = Mat3::translation(shift2) * Mat3::rotation(random.uniform(-3.0, 3.0));
            let mut input3 = Vec::new();
            let mut input2 = Vec::new();
            for _ in 0..count {
                input3.push(Vec3::new(
                    random.coordinate(300000.0),
                    random.coordinate(6250000.0),
                    random.coordinate(40.0),
                ));
                input2.push(Point2::new(
                    random.coordinate(300000.0),
                    random.coordinate(6250000.0),
                ));
            }
            let mut scalar3 = input3.clone();
            let mut kernel3 = input3.clone();
            let mut scalar2 = input2.clone();
            let mut kernel2 = input2.clone();
            at_simd_level(SimdLevel::Scalar, || {
                transform_points(&matrix4, &mut scalar3)
            });
            at_simd_level(kernel, || transform_points(&matrix4, &mut kernel3));
            at_simd_level(SimdLevel::Scalar, || {
                transform_points(&matrix3, &mut scalar2)
            });
            at_simd_level(kernel, || transform_points(&matrix3, &mut kernel2));
            for index in 0..count {
                let reference3 = katana_math::mat4::transform_point(matrix4, input3[index]);
                let reference2 = katana_math::mat3::transform_point(matrix3, input2[index]);
                assert!(same3(scalar3[index], reference3));
                assert!(same3(kernel3[index], reference3));
                assert!(same2(scalar2[index], reference2));
                assert!(same2(kernel2[index], reference2));
            }
        }
    }
    #[test]
    fn bounds_give_the_box_worked_by_hand_at_every_level() {
        let flat = [
            (3.0, -1.0),
            (-2.0, 5.0),
            (6.0, 0.5),
            (1.0, 1.0),
            (0.25, -4.0),
            (6.0, 2.0),
            (-3.5, 3.0),
            (2.0, 8.0),
            (4.0, -2.0),
            (1.5, 1.5),
            (-1.0, -1.0),
            (5.0, 4.0),
            (0.5, 0.75),
            (2.5, 9.0),
            (6.5, -3.0),
            (-3.0, 7.0),
            (7.0, 2.5),
        ]
        .map(|(east, north)| Point2::new(east, north));
        let solid = [
            (1.0, 2.0, 3.0),
            (-1.0, 0.5, 7.0),
            (4.0, -6.0, 2.0),
            (2.0, 2.0, 2.0),
            (0.5, 9.0, -1.0),
            (3.0, 3.0, 3.0),
            (-8.0, 1.0, 1.0),
            (5.0, 5.0, -4.0),
            (6.0, -2.0, 10.0),
            (1.0, 1.0, 1.0),
            (2.0, 3.0, 4.0),
            (-2.0, -1.0, 5.0),
            (3.0, 4.0, -2.0),
            (1.5, 2.5, 6.0),
            (-4.0, 0.5, 0.25),
            (5.5, -5.0, 9.0),
            (0.75, 8.0, -3.0),
            (6.5, 1.0, 2.0),
        ]
        .map(|(east, north, height)| Vec3::new(east, north, height));
        assert!(flat.len() >= BOUNDS_BATCH_MINIMUM);
        assert!(solid.len() >= BOUNDS_BATCH_MINIMUM);
        for level in simd_levels() {
            let bounds = at_simd_level(level, || bounds_of(&flat));
            assert_eq!(
                bounds,
                Box2::new(Point2::new(-3.5, -4.0), Point2::new(7.0, 9.0))
            );
            let cube = at_simd_level(level, || bounds_of(&solid));
            assert_eq!(cube.min, Vec3::new(-8.0, -6.0, -4.0));
            assert_eq!(cube.max, Vec3::new(6.5, 9.0, 10.0));
        }
    }
    #[test]
    fn where_the_extreme_is_zero_the_bounds_keep_the_first_zero_met_as_expand_does() {
        let coordinates = [
            1.0, 2.0, 3.0, -0.0, 0.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0,
            16.0,
        ];
        let mut points = coordinates.map(|east| Point2::new(east, -east));
        assert!(points.len() >= BOUNDS_BATCH_MINIMUM);
        for level in simd_levels() {
            let bounds = at_simd_level(level, || bounds_of(&points));
            assert_eq!(bounds.min.x, 0.0);
            assert!(bounds.min.x.is_sign_negative());
            assert_eq!(bounds.max.y, 0.0);
            assert!(!bounds.max.y.is_sign_negative());
        }
        points.swap(3, 4);
        for level in simd_levels() {
            let bounds = at_simd_level(level, || bounds_of(&points));
            assert!(!bounds.min.x.is_sign_negative());
            assert!(bounds.max.y.is_sign_negative());
        }
    }
    #[test]
    fn bounds_pass_over_na_n_coordinates_as_expand_does() {
        let coordinates = [
            f64::NAN,
            3.0,
            1.0,
            f64::NAN,
            2.0,
            5.0,
            4.0,
            f64::NAN,
            0.5,
            1.5,
            f64::NAN,
            2.5,
            3.5,
            f64::NAN,
            4.5,
            0.75,
            f64::NAN,
        ];
        let points = coordinates.map(|east| Point2::new(east, 1.0));
        assert!(points.len() >= BOUNDS_BATCH_MINIMUM);
        for level in simd_levels() {
            let bounds = at_simd_level(level, || bounds_of(&points));
            assert_eq!(bounds.min.x, 0.5);
            assert_eq!(bounds.max.x, 5.0);
        }
    }
    #[test]
    fn the_bounds_of_nothing_are_empty() {
        for level in simd_levels() {
            assert!(at_simd_level(level, || bounds_of(&Vec::<Point2>::new())).empty());
            assert!(at_simd_level(level, || bounds_of(&Vec::<Vec3>::new())).empty());
        }
    }
    #[test]
    fn bounds_equal_the_expand_loop_bit_for_bit_at_every_level() {
        let kernel = require_kernel();
        let mut random = Random::default();
        for round in 0..600 {
            let count = round % 71;
            let mut flat = Vec::new();
            let mut solid = Vec::new();
            for _ in 0..count {
                let center = if round % 5 == 0 { 0.0 } else { 300000.0 };
                flat.push(Point2::new(
                    random.coordinate(center),
                    random.coordinate(-center),
                ));
                solid.push(Vec3::new(
                    random.coordinate(center),
                    random.coordinate(center),
                    random.coordinate(0.0),
                ));
            }
            let mut loop2 = Box2::default();
            for &point in &flat {
                loop2.expand(point);
            }
            let mut loop3 = Aabb::default();
            for &point in &solid {
                loop3.expand(point);
            }
            assert!(same_box(
                at_simd_level(SimdLevel::Scalar, || bounds_of(&flat)),
                loop2
            ));
            assert!(same_box(at_simd_level(kernel, || bounds_of(&flat)), loop2));
            assert!(same_aabb(
                at_simd_level(SimdLevel::Scalar, || bounds_of(&solid)),
                loop3
            ));
            assert!(same_aabb(
                at_simd_level(kernel, || bounds_of(&solid)),
                loop3
            ));
        }
    }
    #[test]
    fn a_polyline_and_a_mesh_are_bounded_through_the_batch_path() {
        let mut line = Polyline2::default();
        for index in 0..20 {
            line.vertices.push(Point2::new(
                index as f64,
                if index == 5 {
                    -3.0
                } else {
                    19.0 - index as f64
                },
            ));
        }
        let mut mesh = TriangleMesh::default();
        for index in 0..20 {
            mesh.vertices
                .push(Vec3::new(index as f64, -(index as f64), index as f64 * 0.5));
        }
        assert!(line.vertices.len() >= BOUNDS_BATCH_MINIMUM);
        assert!(mesh.vertices.len() >= BOUNDS_BATCH_MINIMUM);
        for level in simd_levels() {
            let bounds = at_simd_level(level, || line.bounding_box());
            assert_eq!(
                bounds,
                Box2::new(Point2::new(0.0, -3.0), Point2::new(19.0, 19.0))
            );
            let cube = at_simd_level(level, || mesh.bounds());
            assert_eq!(cube.min, Vec3::new(0.0, -19.0, 0.0));
            assert_eq!(cube.max, Vec3::new(19.0, 0.0, 9.5));
        }
    }
    #[test]
    fn an_array_shorter_than_the_batch_minimum_is_bounded_inline_without_a_call() {
        for level in simd_levels() {
            assert_eq!(
                at_simd_level(level, || bounds_of(&SHORT_FLAT)),
                Box2::new(Point2::new(-1.0, -2.0), Point2::new(4.0, 5.0))
            );
            let cube = at_simd_level(level, || bounds_of(&SHORT_SOLID));
            assert_eq!(cube.min, Vec3::new(-1.0, -2.0, 0.5));
            assert_eq!(cube.max, Vec3::new(4.0, 5.0, 6.0));
        }
    }
}
