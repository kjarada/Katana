//! The precision rule and the reversed-Z projection (scene_origin): the twin
//! of tests/gpu/test_scene_origin.cpp. Plain double arithmetic: none of this
//! needs a GPU.

use crate::render_support::near;
use katana_core::text::{format_exact_real, format_general};
use katana_gpu::scene_origin::{
    ClipSpace, ORIGIN_ERROR_PIXELS, ProjectionOptions, choose_scene_origin, origin_error_pixels,
    relative_position, relative_view_projection, reversed_z_projection, to_float_column_major,
};
use katana_math::{AABB, Mat4, Vec3, Vec4, cpp};
use katana_render::{Camera, Projection, StandardView};

// Eye-space depth d in front of the eye (z = -d) through a projection: z/w.
fn depth_at(projection: Mat4, distance: f64) -> f64 {
    let clip = projection * Vec4::new(0.0, 0.0, -distance, 1.0);
    clip.z / clip.w
}

fn perspective_camera(near_plane: f64, far_plane: f64) -> Camera {
    let mut camera = Camera::default();
    camera.set_viewport_size(100, 100);
    camera.set_projection(Projection::Perspective);
    camera.set_depth_range(near_plane, far_plane);
    camera
}

// What a vertex shader computes: a column-major float matrix times a float
// position, in float - then where on a width x height screen it lands.
fn shade(m: &[f32; 16], p: &[f32; 3], width: i32, height: i32) -> [f64; 2] {
    let mut clip = [0.0f32; 4];
    for (row, value) in clip.iter_mut().enumerate() {
        *value = m[row] * p[0] + m[4 + row] * p[1] + m[8 + row] * p[2] + m[12 + row];
    }
    [
        f64::from((clip[0] / clip[3] * 0.5 + 0.5) * width as f32),
        f64::from((0.5 - clip[1] / clip[3] * 0.5) * height as f32),
    ]
}

mod scene_origin {
    use super::*;

    #[test]
    fn is_the_centre_of_the_box_and_zero_for_an_empty_box() {
        let bounds = AABB::new(
            Vec3::new(300000.0, 6250000.0, 20.0),
            Vec3::new(300100.0, 6250400.0, 60.0),
        );
        let origin = choose_scene_origin(bounds);
        // (300000 + 300100) / 2, (6250000 + 6250400) / 2, (20 + 60) / 2.
        assert_eq!(origin.x, 300050.0);
        assert_eq!(origin.y, 6250200.0);
        assert_eq!(origin.z, 40.0);
        let none = choose_scene_origin(AABB::default());
        assert_eq!(none.x, 0.0);
        assert_eq!(none.y, 0.0);
        assert_eq!(none.z, 0.0);
    }

    #[test]
    fn an_offset_from_the_origin_keeps_the_millimetres_a_float_at_survey_coordinates_loses() {
        // A float has a 24-bit significand. 6 250 000 lies in [2^22, 2^23), so
        // its floats are 2^(22 - 23) = 0.5 apart: 6250000.456 can only be
        // stored as 6250000.5, 44 mm out.
        let world = Vec3::new(300000.123, 6250000.456, 50.789);
        assert_eq!(world.y as f32, 6250000.5f32);

        // Relative to an origin a few metres away the offset is below 1, where
        // floats are at most 2^-24 apart: every millimetre survives.
        let offset = relative_position(world, Vec3::new(300000.0, 6250000.0, 50.0));
        near(f64::from(offset[0]), 0.123, 1.0e-6);
        near(f64::from(offset[1]), 0.456, 1.0e-6);
        near(f64::from(offset[2]), 0.789, 1.0e-6);
    }

    #[test]
    fn the_error_bound_is_nothing_without_a_viewport() {
        let mut camera = Camera::default();
        camera.set_target(Vec3::new(300000.0, 6200000.0, 0.0));
        assert_eq!(origin_error_pixels(&camera, Vec3::new(0.0, 0.0, 0.0)), 0.0);
    }
}

mod reversed_z {
    use super::*;

    #[test]
    fn perspective_maps_the_near_plane_to_one_and_falls_as_near_over_distance_towards_infinity() {
        let camera = perspective_camera(2.0, 1000.0);
        // Infinite far by default.
        let projection = reversed_z_projection(&camera, &ProjectionOptions::default());
        // depth = near / distance with near = 2.
        near(depth_at(projection, 2.0), 1.0, 1e-15);
        near(depth_at(projection, 4.0), 0.5, 1e-15);
        near(depth_at(projection, 2000.0), 0.001, 1e-15);
        // Beyond the camera's far plane (1000) is still in front of the far end
        // of the buffer: zooming out cannot clip the scene away.
        assert!(depth_at(projection, 1.0e9) > 0.0);
    }

    #[test]
    fn finite_perspective_maps_the_far_plane_to_zero() {
        let camera = perspective_camera(1.0, 100.0);
        let options = ProjectionOptions {
            infinite_far: false,
            ..ProjectionOptions::default()
        };
        let projection = reversed_z_projection(&camera, &options);
        // A = n/(f-n) = 1/99, B = nf/(f-n) = 100/99; depth = (B - A d) / d.
        near(depth_at(projection, 1.0), 1.0, 1e-15);
        near(depth_at(projection, 100.0), 0.0, 1e-15);
        // d = 10: (100/99 - 10/99) / 10 = 90/990 = 1/11.
        near(depth_at(projection, 10.0), 1.0 / 11.0, 1e-15);
    }

    #[test]
    fn orthographic_depth_runs_linearly_from_one_at_the_near_plane_to_zero_at_the_far() {
        let mut camera = perspective_camera(10.0, 110.0);
        camera.set_projection(Projection::Orthographic);
        let projection = reversed_z_projection(&camera, &ProjectionOptions::default());
        // depth = (f - d) / (f - n) = (110 - d) / 100.
        near(depth_at(projection, 10.0), 1.0, 1e-15);
        near(depth_at(projection, 60.0), 0.5, 1e-15);
        near(depth_at(projection, 110.0), 0.0, 1e-15);
    }

    #[test]
    fn a_vulkan_or_open_gl_clip_space_flips_y_or_widens_depth_and_nothing_else() {
        let camera = perspective_camera(1.0, 100.0);
        let d3d = reversed_z_projection(&camera, &ProjectionOptions::default());
        let vulkan = ProjectionOptions {
            clip: ClipSpace {
                y_up_in_ndc: false,
                ..ClipSpace::default()
            },
            ..ProjectionOptions::default()
        };
        let flipped = reversed_z_projection(&camera, &vulkan);
        let gl = ProjectionOptions {
            clip: ClipSpace {
                depth_zero_to_one: false,
                ..ClipSpace::default()
            },
            ..ProjectionOptions::default()
        };
        let signed_depth = reversed_z_projection(&camera, &gl);
        for col in 0..4 {
            assert_eq!(flipped[(0, col)], d3d[(0, col)]);
            assert_eq!(flipped[(1, col)], -d3d[(1, col)]);
            assert_eq!(flipped[(2, col)], d3d[(2, col)]);
            // z' = 2z - w.
            assert_eq!(signed_depth[(2, col)], 2.0 * d3d[(2, col)] - d3d[(3, col)]);
        }
    }
}

mod relative_view_projection {
    use super::*;

    const WIDTH: i32 = 1000;
    const HEIGHT: i32 = 800;

    // The rule the whole GPU path rests on, checked without a GPU: the camera
    // matrix built against the scene origin, rounded to float and applied to
    // float offsets, puts survey-coordinate points where the software path's
    // double arithmetic does; the same thing done with float WORLD
    // coordinates does not.
    #[test]
    fn puts_survey_coordinate_points_where_the_camera_does_when_rounded_to_float() {
        let mut camera = Camera::default();
        camera.set_viewport_size(WIDTH, HEIGHT);
        camera.set_projection(Projection::Perspective);
        camera.set_standard_view(StandardView::IsoSouthWest);
        let target = Vec3::new(300000.0, 6250000.0, 50.0);
        camera.set_target(target);
        camera.set_distance(100.0);
        camera.set_depth_range(1.0, 10000.0);
        let origin = Vec3::new(300010.0, 6250005.0, 48.0);

        // Fractions of a quarter metre, which a float at y = 6.25e6 cannot hold.
        let offsets = [
            Vec3::new(10.25, -7.25, 3.0),
            Vec3::new(-12.75, 4.25, -2.0),
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(24.75, 25.25, 5.0),
        ];

        let relative = relative_view_projection(&camera, origin, &ProjectionOptions::default());
        let relative_float = to_float_column_major(relative);
        let world_float = to_float_column_major(camera.view_projection());

        let mut worst_relative = 0.0f64;
        let mut worst_world = 0.0f64;
        for offset in offsets {
            let world = target + offset;
            // The software path, in double.
            let Some(expected) = camera.project(world) else {
                panic!("{world:?} is behind the eye");
            };

            // In double the relative matrix is the camera's to a millionth of
            // a pixel.
            let clip = relative * Vec4::from_xyz(world - origin, 1.0);
            near(
                (clip.x / clip.w * 0.5 + 0.5) * f64::from(WIDTH),
                expected.x,
                1e-6,
            );
            near(
                (0.5 - clip.y / clip.w * 0.5) * f64::from(HEIGHT),
                expected.y,
                1e-6,
            );

            let gpu = shade(
                &relative_float,
                &relative_position(world, origin),
                WIDTH,
                HEIGHT,
            );
            worst_relative = cpp::max3(
                worst_relative,
                (gpu[0] - expected.x).abs(),
                (gpu[1] - expected.y).abs(),
            );
            let raw = [world.x as f32, world.y as f32, world.z as f32];
            let naive = shade(&world_float, &raw, WIDTH, HEIGHT);
            worst_world = cpp::max3(
                worst_world,
                (naive[0] - expected.x).abs(),
                (naive[1] - expected.y).abs(),
            );
        }
        // Relative: offsets under 40 m hold floats 4e-6 m apart and the
        // matrix's translation is 100 m, rounded to 8e-6 m; at 100 m with a 45
        // degree field over 800 px a metre is 800 / (2 * 100 * tan 22.5) = 9.7
        // px, so the error is around 1e-4 px. A hundredth of a pixel leaves a
        // wide margin.
        assert!(worst_relative < 0.01, "worst relative {worst_relative}");
        // World floats: the positions alone are up to 0.25 m out, about 2 px.
        assert!(worst_world > 0.5, "worst world {worst_world}");
    }

    // The rule a deep zoom re-packs the GPU's layers by (scene_origin, "The
    // origin follows a deep zoom"), measured: the eye 0.2 m from a point of a
    // survey at MGA coordinates, an 800 px tall view, and origins from a metre
    // to 3000 km away from the point. A pixel there is 0.2 x 2 tan(22.5 deg) /
    // 800 = 2.07e-4 m, so the bound, 10 x 2^-24 of the distance over it, is
    // 2.9e-3 px a metre of it. Around the point, what the shader's float
    // arithmetic draws stays inside the bound at every distance; past
    // ORIGIN_ERROR_PIXELS it is out by more than the view can hide; and with
    // the origin at the pivot, as the host re-packs it, it is out by nothing
    // to speak of.
    #[test]
    fn at_the_origins_error_bound_the_pivot_is_drawn_within_it_and_its_own_origin_draws_it_exactly()
    {
        let mut camera = Camera::default();
        camera.set_viewport_size(WIDTH, HEIGHT);
        camera.set_projection(Projection::Perspective);
        camera.set_standard_view(StandardView::IsoSouthWest);
        let target = Vec3::new(300050.3, 6200050.7, 31.1);
        camera.set_target(target);
        camera.set_distance(0.2);
        camera.set_depth_range(1e-4, 1000.0);
        near(
            camera.world_per_pixel(),
            0.2 * 2.0 * 0.41421356237309503 / 800.0,
            1e-15,
        );

        // Points round the pivot, as far out as the view reaches at its depth.
        let probes = [
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(0.031, -0.012, 0.004),
            Vec3::new(-0.027, 0.019, -0.008),
            Vec3::new(0.012, 0.033, 0.0),
            Vec3::new(-0.041, -0.022, 0.011),
        ];
        let worst_from = |origin: Vec3| {
            let matrix = to_float_column_major(relative_view_projection(
                &camera,
                origin,
                &ProjectionOptions::default(),
            ));
            let mut worst = 0.0f64;
            for probe in probes {
                let world = target + probe;
                let Some(expected) = camera.project(world) else {
                    panic!("{world:?} is behind the eye");
                };
                let gpu = shade(&matrix, &relative_position(world, origin), WIDTH, HEIGHT);
                worst = cpp::max3(
                    worst,
                    (gpu[0] - expected.x).abs(),
                    (gpu[1] - expected.y).abs(),
                );
            }
            worst
        };

        // Along a direction no axis favours, so that no coordinate's rounding
        // happens to vanish.
        let away = Vec3::new(0.6, -0.48, 0.64);
        let mut worst_below_the_rule = 0.0f64;
        for distance in [1.0, 10.0, 100.0, 1000.0, 1.0e4, 1.0e5, 3.0e6] {
            let origin = target + away * distance;
            let bound = origin_error_pixels(&camera, origin);
            near(
                bound,
                10.0 * (1.0 / 16_777_216.0) * distance / camera.world_per_pixel(),
                1e-9 * bound,
            );
            let measured = worst_from(origin);
            assert!(
                measured <= bound,
                "origin {} m from the pivot: {measured} against {bound}",
                format_general(distance, 6)
            );
            if bound <= ORIGIN_ERROR_PIXELS {
                worst_below_the_rule = cpp::max(worst_below_the_rule, measured);
            }
            // The C++ records each as a gtest property.
            println!(
                "origin_{}_m_px={}",
                distance as i64,
                format_exact_real(measured)
            );
        }
        // What the rule leaves alone is drawn right, and 3000 km off - the
        // centre of a survey's box with a stray entity at the origin, where a
        // float steps in 0.125-0.25 m, 600-1200 of these pixels - the points
        // round the pivot are drawn well out.
        assert!(worst_below_the_rule < ORIGIN_ERROR_PIXELS);
        assert!(worst_from(target + away * 3.0e6) > 10.0);
        // Re-packed against the pivot, as the host does past the rule.
        assert_eq!(origin_error_pixels(&camera, target), 0.0);
        assert!(worst_from(target) < 1e-3);
    }
}
