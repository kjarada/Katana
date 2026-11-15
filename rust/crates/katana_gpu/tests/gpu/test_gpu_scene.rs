//! Packing a DrawList for the GPU (gpu_scene): the twin of
//! tests/gpu/test_gpu_scene.cpp. No device needed.

use crate::render_support::{Random, at_simd_level, require_kernel};
use katana_core::cpu_features::SimdLevel;
use katana_gpu::gpu_scene::{
    GpuSceneData, PointCloudData, pack_draw_list, pack_draw_list_with_origin,
    pack_engine_point_cloud, pack_point_cloud,
};
use katana_math::Vec3;
use katana_pointcloud::{PointCloud, PointCloudBounds, PointCloudPoint};
use katana_render::{DrawList, Rgba, VertexIndex, rgba};

mod pack_draw_list {
    use super::*;

    #[test]
    fn stores_positions_as_offsets_from_the_centre_of_what_the_list_draws() {
        let mut list = DrawList::default();
        let a = list.add_vertex(Vec3::new(300000.0, 6250000.0, 10.0), rgba(255, 0, 0));
        let b = list.add_vertex(Vec3::new(300100.0, 6250000.0, 30.0), rgba(0, 255, 0));
        let c = list.add_vertex(Vec3::new(300000.0, 6250200.0, 20.0), rgba(0, 0, 255));
        list.add_triangle(a, b, c);

        let mut scene = GpuSceneData::default();
        pack_draw_list(&list, &mut scene);
        // Box x [300000, 300100], y [6250000, 6250200], z [10, 30]: centre
        // (300050, 6250100, 20).
        assert_eq!(scene.origin.x, 300050.0);
        assert_eq!(scene.origin.y, 6250100.0);
        assert_eq!(scene.origin.z, 20.0);
        assert_eq!(scene.vertices.len(), 3);
        assert_eq!(scene.vertices[0].x, -50.0f32);
        assert_eq!(scene.vertices[0].y, -100.0f32);
        assert_eq!(scene.vertices[0].z, -10.0f32);
        assert_eq!(scene.vertices[1].x, 50.0f32);
        assert_eq!(scene.vertices[2].y, 100.0f32);
        assert_eq!(scene.vertices[1].color, rgba(0, 255, 0));
        assert_eq!(scene.triangle_indices, [0, 1, 2]);
    }

    #[test]
    fn drops_primitives_that_index_past_the_vertices() {
        let mut list = DrawList::default();
        let a = list.add_vertex(Vec3::new(0.0, 0.0, 0.0), rgba(255, 255, 255));
        let b = list.add_vertex(Vec3::new(1.0, 0.0, 0.0), rgba(255, 255, 255));
        list.add_triangle(a, b, 7);
        list.add_line(a, 9, 1.0, 0.0);
        list.add_point(5, 1.0, 0.0);
        list.add_line(a, b, 1.0, 0.0);

        let mut scene = GpuSceneData::default();
        pack_draw_list(&list, &mut scene);
        assert!(scene.triangle_indices.is_empty());
        assert_eq!(scene.lines.len(), 1);
        assert!(scene.points.is_empty());
    }

    #[test]
    fn gives_each_line_its_endpoints_colours_and_width_and_each_point_its_size() {
        let mut list = DrawList::default();
        let a = list.add_vertex(Vec3::new(-2.0, 0.0, 0.0), rgba(10, 20, 30));
        let b = list.add_vertex(Vec3::new(2.0, 4.0, 0.0), rgba(40, 50, 60));
        list.add_line(a, b, 3.0, 2.0e-4);
        // Thinner than a pixel is drawn one pixel wide, as on the CPU.
        list.add_line(b, a, 0.25, 0.0);
        list.add_point(b, 5.0, 0.0);

        let mut scene = GpuSceneData::default();
        pack_draw_list(&list, &mut scene);
        // Box x [-2, 2], y [0, 4]: centre (0, 2, 0).
        assert_eq!(scene.lines.len(), 2);
        assert_eq!(scene.lines[0].a.x, -2.0f32);
        assert_eq!(scene.lines[0].a.y, -2.0f32);
        assert_eq!(scene.lines[0].b.x, 2.0f32);
        assert_eq!(scene.lines[0].b.y, 2.0f32);
        assert_eq!(scene.lines[0].a.color, rgba(10, 20, 30));
        assert_eq!(scene.lines[0].b.color, rgba(40, 50, 60));
        // The width at both ends, so each end is a whole record.
        assert_eq!(scene.lines[0].a.width, 3.0f32);
        assert_eq!(scene.lines[0].b.width, 3.0f32);
        assert_eq!(scene.lines[1].a.width, 1.0f32);
        assert_eq!(scene.lines[1].b.width, 1.0f32);
        assert_eq!(scene.points.len(), 1);
        assert_eq!(scene.points[0].size, 5.0f32);
        assert_eq!(scene.points[0].y, 2.0f32);
        // 2 vertices x 16 + 2 lines x 40 + 1 point x 24 bytes.
        assert_eq!(scene.byte_size(), 2 * 16 + 2 * 40 + 24);
    }

    // The same float, or both NaN: a NaN's payload is not compared
    // (docs/performance.md, "NaN payloads").
    fn same_float(a: f32, b: f32) -> bool {
        (a.is_nan() && b.is_nan()) || a.to_bits() == b.to_bits()
    }

    // The AVX2 or NEON packing (simd/pack_avx2.rs, pack_neon.rs) against the
    // loop it replaces, through pack_draw_list itself: every length from 0 to
    // 70, so every tail of one to three past every block edge, and lists
    // shorter than one block, over coordinates as survey data has them and the
    // values a conversion can get wrong - both zeros, infinities, NaN,
    // subnormals, doubles too large for a float and too small for one.
    //
    // The draws are taken in the order written; the C++ leaves the order of a
    // call's arguments to the compiler, so the lists are not the C++'s draw for
    // draw. The subject is that two paths agree on whatever they are given,
    // and the specials reach every lane: a fifth of the coordinates are one.
    #[test]
    fn every_vertex_packs_to_the_same_bits_at_each_simd_level() {
        let kernel = require_kernel();
        let mut random = Random::new(20260926);
        let specials: [f64; 12] = [
            0.0,
            -0.0,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NAN,
            6250000.123456789,
            -300000.5,
            // The smallest subnormal double.
            4.9e-324,
            // A subnormal float.
            1.0e-40,
            // Below every float: rounds to zero.
            1.0e-50,
            // Above every float: rounds to infinity.
            7.0e38,
            // Half an ulp above FLT_MAX: the tie that rounds to infinity, to
            // even.
            3.4028235677973366e38,
        ];
        let coordinate = |random: &mut Random| {
            if random.next_u64().is_multiple_of(5) {
                specials[(random.next_u64() % specials.len() as u64) as usize]
            } else {
                random.real(-7.0e6, 7.0e6)
            }
        };
        for count in 0..=70usize {
            for round in 0..4 {
                let trace = format!("{count} vertices, round {round}");
                let mut list = DrawList::default();
                for _ in 0..count {
                    let x = coordinate(&mut random);
                    let y = coordinate(&mut random);
                    let z = coordinate(&mut random);
                    let color = random.next_u64() as Rgba;
                    list.add_vertex(Vec3::new(x, y, z), color);
                }
                if count >= 3 {
                    list.add_triangle(0, 1, 2);
                    list.add_line(0, (count - 1) as VertexIndex, 2.0, 0.0);
                    list.add_point(1, 4.0, 0.0);
                }
                // The origin is a double like the positions: a small z, as a
                // scene's usually is.
                let ox = random.real(-7.0e6, 7.0e6);
                let oy = random.real(-7.0e6, 7.0e6);
                let oz = random.real(-7.0e6, 7.0e6) * 1.0e-4;
                let origin = Vec3::new(ox, oy, oz);
                let mut scalar = GpuSceneData::default();
                let mut vector = GpuSceneData::default();
                at_simd_level(SimdLevel::Scalar, || {
                    pack_draw_list_with_origin(&list, origin, &mut scalar)
                });
                at_simd_level(kernel, || {
                    pack_draw_list_with_origin(&list, origin, &mut vector)
                });

                assert_eq!(vector.vertices.len(), scalar.vertices.len(), "{trace}");
                for (i, (v, s)) in vector.vertices.iter().zip(&scalar.vertices).enumerate() {
                    assert!(
                        same_float(v.x, s.x) && same_float(v.y, s.y) && same_float(v.z, s.z),
                        "{trace}: vertex {i}: {} {} {} against {} {} {}",
                        v.x,
                        v.y,
                        v.z,
                        s.x,
                        s.y,
                        s.z
                    );
                    assert_eq!(v.color, s.color, "{trace}: vertex {i}");
                }
                // What is built from the vertices is built from the same bits.
                assert_eq!(vector.lines.len(), scalar.lines.len(), "{trace}");
                for (v, s) in vector.lines.iter().zip(&scalar.lines) {
                    assert!(same_float(v.a.x, s.a.x), "{trace}");
                    assert!(same_float(v.b.z, s.b.z), "{trace}");
                    assert_eq!(v.b.color, s.b.color, "{trace}");
                }
                assert_eq!(vector.triangle_indices, scalar.triangle_indices, "{trace}");
                assert_eq!(vector.points.len(), scalar.points.len(), "{trace}");
            }
        }
    }

    // The values themselves, not only the agreement of two paths that could
    // share a mistake: a list long enough for the kernel, each vertex's offset
    // worked out here. The origin is exact in double and each offset exact in
    // float, so no rounding stands between the arithmetic and the expectation.
    #[test]
    fn the_kernel_writes_each_vertexs_offset_and_colour_where_the_scalar_loop_does() {
        let mut list = DrawList::default();
        // Five blocks of four and a tail of three.
        for i in 0..23u32 {
            let step = f64::from(i);
            list.add_vertex(
                Vec3::new(300000.0 + step, 6250000.0 - 2.0 * step, 10.0 + 0.5 * step),
                0xFF00_0000 + i,
            );
        }
        let mut scene = GpuSceneData::default();
        pack_draw_list_with_origin(&list, Vec3::new(300000.0, 6250000.0, 10.0), &mut scene);
        assert_eq!(scene.vertices.len(), 23);
        for (i, vertex) in (0..23u32).zip(&scene.vertices) {
            assert_eq!(vertex.x, i as f32, "{i}");
            assert_eq!(vertex.y, (-2 * i as i32) as f32, "{i}");
            assert_eq!(vertex.z, 0.5f32 * i as f32, "{i}");
            assert_eq!(vertex.color, 0xFF00_0000 + i, "{i}");
        }
    }
}

mod pack_point_cloud {
    use super::*;

    #[test]
    fn thins_evenly_through_the_source_order_to_stay_within_the_budget() {
        let positions: Vec<Vec3> = (0..10)
            .map(|i| Vec3::new(1000.0 + f64::from(i), 2000.0, 3.0))
            .collect();
        let mut cloud = PointCloudData::default();
        // 10 points, budget 4: every ceil(10 / 4) = 3rd point, i.e. 0, 3, 6, 9.
        pack_point_cloud(
            &positions,
            &[],
            rgba(1, 2, 3),
            Vec3::new(1000.0, 2000.0, 0.0),
            4,
            &mut cloud,
        );
        assert_eq!(cloud.points.len(), 4);
        assert_eq!(cloud.points[0].x, 0.0f32);
        assert_eq!(cloud.points[1].x, 3.0f32);
        assert_eq!(cloud.points[3].x, 9.0f32);
        assert_eq!(cloud.points[3].z, 3.0f32);
        assert_eq!(cloud.points[0].color, rgba(1, 2, 3));
        assert_eq!(cloud.source_count, 10);

        // Budget 5: every 2nd, 5 points. Budget 0: no limit.
        pack_point_cloud(
            &positions,
            &[],
            rgba(1, 2, 3),
            Vec3::default(),
            5,
            &mut cloud,
        );
        assert_eq!(cloud.points.len(), 5);
        pack_point_cloud(
            &positions,
            &[],
            rgba(1, 2, 3),
            Vec3::default(),
            0,
            &mut cloud,
        );
        assert_eq!(cloud.points.len(), 10);
    }

    #[test]
    fn takes_the_engines_points_in_their_own_colour_about_the_centre_of_their_bounds() {
        // Four points of a survey-coordinate cloud; the second has no colour.
        let x0 = 300000.0;
        let y0 = 6250000.0;
        let point = |x: f64, y: f64, z: f64, red: u8, green: u8, blue: u8, has_color: bool| {
            PointCloudPoint {
                x,
                y,
                z,
                intensity: 0.0,
                classification: 2,
                red,
                green,
                blue,
                has_color,
            }
        };
        let source = PointCloud {
            points: vec![
                point(x0 + 0.0, y0 + 0.0, 10.0, 200, 10, 20, true),
                point(x0 + 4.0, y0 + 0.0, 12.0, 0, 0, 0, false),
                point(x0 + 4.0, y0 + 8.0, 14.0, 30, 40, 50, true),
                point(x0 + 0.0, y0 + 8.0, 16.0, 60, 70, 80, true),
            ],
            bounds: PointCloudBounds {
                min_x: x0,
                min_y: y0,
                min_z: 10.0,
                max_x: x0 + 4.0,
                max_y: y0 + 8.0,
                max_z: 16.0,
            },
            ..PointCloud::default()
        };
        let mut cloud = PointCloudData::default();
        // Budget 2 of 4: every ceil(4 / 2) = 2nd point - the first and the third.
        pack_engine_point_cloud(&source, rgba(9, 9, 9), 2, &mut cloud);
        // The centre of the bounds: (x0 + 2, y0 + 4, 13).
        assert_eq!(cloud.origin, Vec3::new(x0 + 2.0, y0 + 4.0, 13.0));
        assert_eq!(cloud.points.len(), 2);
        assert_eq!(cloud.points[0].x, -2.0f32);
        assert_eq!(cloud.points[0].y, -4.0f32);
        assert_eq!(cloud.points[0].z, -3.0f32);
        assert_eq!(cloud.points[0].color, rgba(200, 10, 20));
        assert_eq!(cloud.points[1].x, 2.0f32);
        assert_eq!(cloud.points[1].y, 4.0f32);
        assert_eq!(cloud.points[1].z, 1.0f32);
        assert_eq!(cloud.points[1].color, rgba(30, 40, 50));
        assert_eq!(cloud.source_count, 4);

        // All of them: the uncoloured one takes the fallback.
        pack_engine_point_cloud(&source, rgba(9, 9, 9), 0, &mut cloud);
        assert_eq!(cloud.points.len(), 4);
        assert_eq!(cloud.points[1].color, rgba(9, 9, 9));
    }
}
