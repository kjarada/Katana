//! DrawList's bounds (draw_list.rs): the box of every finite vertex a
//! primitive uses, each vertex visited once: the twin of
//! tests/render/test_draw_list.cpp.
//!
//! The reference below is the walk bounds() replaced - every use of every
//! vertex, primitive by primitive - and the result must be its box to the bit,
//! the signs of zeros included.

#![allow(
    clippy::excessive_precision,
    reason = "Preserve the decimal constants copied verbatim from the C++ oracle."
)]

pub mod support;

mod draw_list_bounds {
    use crate::support::Random;
    use katana_math::{AABB, Vec3};
    use katana_render::{DrawList, VertexIndex};

    fn bounds_primitive_by_primitive(list: &DrawList) -> AABB {
        let mut bounds = AABB::default();
        let mut include = |index: VertexIndex| {
            if let Some(position) = list.positions.get(index as usize)
                && position.is_finite()
            {
                bounds.expand_point(*position);
            }
        };
        for triangle in &list.triangles {
            include(triangle.a);
            include(triangle.b);
            include(triangle.c);
        }
        for line in &list.lines {
            include(line.a);
            include(line.b);
        }
        for point in &list.points {
            include(point.a);
        }
        bounds
    }

    fn same_box(a: &AABB, b: &AABB) -> bool {
        let left = [a.min.x, a.min.y, a.min.z, a.max.x, a.max.y, a.max.z];
        let right = [b.min.x, b.min.y, b.min.z, b.max.x, b.max.y, b.max.z];
        left.map(f64::to_bits) == right.map(f64::to_bits)
    }

    const SPECIALS: [f64; 6] = [
        0.0,
        -0.0,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NAN,
        4.9e-324,
    ];

    // Few distinct values in the coarse rounds, so that ties - the zeros among
    // them - are common.
    fn coordinate(random: &mut Random, coarse: bool) -> f64 {
        if random.next_u64().is_multiple_of(4) {
            return SPECIALS[(random.next_u64() % SPECIALS.len() as u64) as usize];
        }
        if coarse {
            f64::from((random.next_u64() % 5) as i32 - 2)
        } else {
            random.real(-1000.0, 1000.0)
        }
    }

    // Mostly valid indices, some one past the end or far past it.
    fn index(random: &mut Random, count: usize) -> VertexIndex {
        let pick = random.next_u64() % 20;
        if pick == 0 {
            return count as VertexIndex;
        }
        if pick == 1 {
            return VertexIndex::MAX;
        }
        (random.next_u64() % count as u64) as VertexIndex
    }

    // A vertex no primitive uses, an index past the last vertex and a vertex
    // that is not finite are all left out; the box is of the rest.
    #[test]
    fn are_the_box_of_the_finite_vertices_the_primitives_use() {
        let mut list = DrawList::default();
        let a = list.add_vertex(Vec3::new(1.0, 2.0, 3.0), 0);
        let b = list.add_vertex(Vec3::new(4.0, -5.0, 6.0), 0);
        list.add_vertex(Vec3::new(-100.0, 100.0, 100.0), 0); // never used
        let c = list.add_vertex(Vec3::new(f64::NAN, 0.0, 0.0), 0);
        let d = list.add_vertex(Vec3::new(2.0, 3.0, -7.0), 0);
        list.add_line(a, b, 1.0, 0.0);
        list.add_triangle(a, c, 999); // c is not finite, 999 is not a vertex
        list.add_point(d, 1.0, 0.0);

        let bounds = list.bounds();
        assert_eq!(bounds.min.x, 1.0);
        assert_eq!(bounds.min.y, -5.0);
        assert_eq!(bounds.min.z, -7.0);
        assert_eq!(bounds.max.x, 4.0);
        assert_eq!(bounds.max.y, 3.0);
        assert_eq!(bounds.max.z, 6.0);
    }

    #[test]
    fn of_a_list_with_nothing_drawn_are_empty() {
        let mut list = DrawList::default();
        list.add_vertex(Vec3::new(1.0, 2.0, 3.0), 0);
        assert!(list.bounds().empty());
        list.add_line(5, 6, 1.0, 0.0); // neither end is a vertex
        assert!(list.bounds().empty());
    }

    // Which zero an extreme is follows the order the vertices are met in
    // (AABB::expand keeps the first of two equal values), so bounding each
    // vertex once, in index order, could pick the other one. Here index order
    // meets +0 first and primitive order -0: the result is primitive order's.
    #[test]
    fn where_an_extreme_is_a_zero_it_is_the_zero_primitive_order_meets_first() {
        let mut list = DrawList::default();
        let plus = list.add_vertex(Vec3::new(0.0, 1.0, 1.0), 0);
        let minus = list.add_vertex(Vec3::new(-0.0, 2.0, 2.0), 0);
        list.add_line(minus, plus, 1.0, 0.0);
        let bounds = list.bounds();
        assert!(bounds.min.x.is_sign_negative());
        assert!(bounds.max.x.is_sign_negative());
        assert!(same_box(&bounds, &bounds_primitive_by_primitive(&list)));
    }

    // Many lists, each vertex used by several primitives in a shuffled order,
    // with zeros of both signs, infinities, NaN, unused vertices and indices
    // past the end: the same box as the reference every time. The scratch is
    // reused across lists of different sizes, as the scene builder reuses it.
    #[test]
    fn are_the_reference_box_to_the_bit_whatever_the_vertices_and_the_order() {
        let mut random = Random::new(20260925);
        let mut scratch = Vec::new();
        let mut zero_extremes = 0;
        for round in 0..2000 {
            let mut list = DrawList::default();
            let count = 1 + (random.next_u64() % 40) as usize;
            let coarse = round % 2 == 0;
            for _ in 0..count {
                let x = coordinate(&mut random, coarse);
                let y = coordinate(&mut random, coarse);
                let z = coordinate(&mut random, coarse);
                list.add_vertex(Vec3::new(x, y, z), 0);
            }
            let primitives = random.next_u64() % 30;
            for _ in 0..primitives {
                match random.next_u64() % 3 {
                    0 => {
                        let a = index(&mut random, count);
                        let b = index(&mut random, count);
                        let c = index(&mut random, count);
                        list.add_triangle(a, b, c);
                    }
                    1 => {
                        let a = index(&mut random, count);
                        let b = index(&mut random, count);
                        list.add_line(a, b, 1.0, 0.0);
                    }
                    _ => {
                        let a = index(&mut random, count);
                        list.add_point(a, 1.0, 0.0);
                    }
                }
            }
            let expected = bounds_primitive_by_primitive(&list);
            assert!(same_box(&list.bounds(), &expected), "round {round}");
            assert!(
                same_box(&list.bounds_with_scratch(&mut scratch), &expected),
                "round {round}"
            );
            for extreme in [
                expected.min.x,
                expected.min.y,
                expected.min.z,
                expected.max.x,
                expected.max.y,
                expected.max.z,
            ] {
                if extreme == 0.0 {
                    zero_extremes += 1;
                }
            }
        }
        // The case the fallback exists for, met often enough to have been
        // tested.
        assert!(zero_extremes > 100);
    }
}
