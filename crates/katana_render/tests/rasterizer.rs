//! Software rasteriser (PLAN.MD Phase 15): the twin of
//! tests/render/test_rasterizer.cpp.
//!
//! These are pixel assertions, not smoke tests: the whole reason the first
//! renderer is on the CPU is that its output can be checked exactly, so that
//! the clipping and depth rules are nailed down before a GPU backend has to
//! reproduce them.

pub mod support;

mod render_rasterizer {
    use crate::support::{count_pixels, float_eq, rendered};
    use katana_core::ErrorCode;
    use katana_core::task_pool::TaskPool;
    use katana_math::Vec3;
    use katana_render::{
        Camera, DrawList, Framebuffer, Projection, Rasterizer, RenderOptions, Rgba, StandardView,
        rgba,
    };

    const BACKGROUND: Rgba = rgba(0, 0, 0);
    const RED: Rgba = rgba(255, 0, 0);
    const GREEN: Rgba = rgba(0, 255, 0);
    const BLUE: Rgba = rgba(0, 0, 255);

    // A camera looking straight down at the XY plane, orthographic, with
    // exactly one world unit per pixel and the origin at the image centre.
    // Every geometric assertion below is then a statement about pixels that
    // can be read off by hand.
    fn plan_camera(width: i32, height: i32) -> Camera {
        let mut camera = Camera::default();
        camera.set_viewport_size(width, height);
        camera.set_projection(Projection::Orthographic);
        camera.set_standard_view(StandardView::Top);
        camera.set_target(Vec3::new(0.0, 0.0, 0.0));
        camera.set_distance(1000.0);
        camera.set_orthographic_height(f64::from(height));
        camera.set_depth_range(1.0, 2000.0);
        camera
    }

    fn serial_options(pool: &TaskPool) -> RenderOptions<'_> {
        RenderOptions {
            background: BACKGROUND,
            pool: Some(pool),
            ..RenderOptions::default()
        }
    }

    fn framebuffer(width: i32, height: i32) -> Framebuffer {
        Framebuffer::create(width, height).expect("the test's size is a framebuffer's")
    }

    // A level eye 50 units south of the origin, looking north: the view the
    // near-plane tests below are worked out for.
    fn level_eye_camera(width: i32, height: i32, near_plane: f64, far_plane: f64) -> Camera {
        let mut camera = Camera::default();
        camera.set_viewport_size(width, height);
        camera.set_projection(Projection::Perspective);
        camera.set_standard_view(StandardView::Front); // looking north from the south
        camera.set_target(Vec3::new(0.0, 0.0, 0.0));
        camera.set_distance(50.0); // eye at y = -50
        camera.set_depth_range(near_plane, far_plane);
        camera
    }

    // 80 px is the size the first test below was written at. 400 px and
    // 1920x1080 are sizes at which clipping at w = 1e-6, as the rasteriser
    // once did, put the cut vertex's screen coordinate beyond INT_MAX, so the
    // float-to-int conversion in the binning was undefined and dropped the
    // whole primitive - its visible part included (audit of 2026-09-23; the
    // threshold for that scene was about 213 px).
    const NEAR_PLANE_SIZES: [(i32, i32); 3] = [(80, 80), (400, 400), (1920, 1080)];

    fn add_quad(list: &mut DrawList, z: f64, color: Rgba) {
        let v0 = list.add_vertex(Vec3::new(-50.0, -50.0, z), color);
        let v1 = list.add_vertex(Vec3::new(50.0, -50.0, z), color);
        let v2 = list.add_vertex(Vec3::new(50.0, 50.0, z), color);
        let v3 = list.add_vertex(Vec3::new(-50.0, 50.0, z), color);
        list.add_triangle(v0, v1, v2);
        list.add_triangle(v0, v2, v3);
    }

    #[test]
    fn rejects_a_target_that_does_not_match_the_camera() {
        let mut target = framebuffer(64, 64);
        let camera = plan_camera(32, 32); // deliberately the wrong size

        let mut rasterizer = Rasterizer::default();
        let list = DrawList::default();
        let result = rasterizer.render(&list, &camera, &mut target, &RenderOptions::default());
        let error = result.expect_err("a target of another size must be refused");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }

    #[test]
    fn an_empty_list_leaves_the_cleared_background() {
        let mut target = framebuffer(64, 48);
        let camera = plan_camera(64, 48);
        let pool = TaskPool::new(0);

        let mut rasterizer = Rasterizer::default();
        let list = DrawList::default();
        let stats =
            rendered(rasterizer.render(&list, &camera, &mut target, &serial_options(&pool)));
        assert_eq!(stats.fragments, 0);
        assert_eq!(count_pixels(&target, BACKGROUND), 64 * 48);
        // Depth must clear to the far plane, which reversed Z puts at 0.
        float_eq(target.depth_at(10, 10), 0.0);
    }

    #[test]
    fn fills_exactly_the_area_of_an_axis_aligned_triangle() {
        let mut target = framebuffer(100, 100);
        let camera = plan_camera(100, 100);
        let pool = TaskPool::new(0);

        // A right triangle with legs of 40 world units = 40 pixels, so the
        // filled area is 800 pixels give or take the boundary.
        let mut list = DrawList::default();
        let a = list.add_vertex(Vec3::new(-20.0, -20.0, 0.0), RED);
        let b = list.add_vertex(Vec3::new(20.0, -20.0, 0.0), RED);
        let c = list.add_vertex(Vec3::new(-20.0, 20.0, 0.0), RED);
        list.add_triangle(a, b, c);

        let mut rasterizer = Rasterizer::default();
        let stats =
            rendered(rasterizer.render(&list, &camera, &mut target, &serial_options(&pool)));
        assert_eq!(stats.triangles_submitted, 1);
        assert_eq!(stats.triangles_rasterised, 1);

        let filled = count_pixels(&target, RED);
        // Half of 40x40. The tolerance is the boundary row and column, which a
        // half-open sampling rule may or may not include.
        assert!((filled as f64 - 800.0).abs() <= 45.0, "filled {filled}");

        // Inside is red, well outside is background.
        assert_eq!(target.color_at(45, 65), RED);
        assert_eq!(target.color_at(90, 90), BACKGROUND);
    }

    #[test]
    fn the_nearer_triangle_wins_whichever_order_they_are_submitted() {
        let camera = plan_camera(40, 40);
        let pool = TaskPool::new(0);

        // Both cover the whole image; the green one is 10 units higher, so it
        // is nearer the camera looking down and must survive.
        let build = |near_first: bool| {
            let mut list = DrawList::default();
            if near_first {
                add_quad(&mut list, 10.0, GREEN);
                add_quad(&mut list, 0.0, RED);
            } else {
                add_quad(&mut list, 0.0, RED);
                add_quad(&mut list, 10.0, GREEN);
            }
            list
        };

        for near_first in [true, false] {
            let mut target = framebuffer(40, 40);
            let mut rasterizer = Rasterizer::default();
            let list = build(near_first);
            rendered(rasterizer.render(&list, &camera, &mut target, &serial_options(&pool)));
            assert_eq!(
                count_pixels(&target, GREEN),
                40 * 40,
                "submission order must not decide occlusion (near_first={near_first})"
            );
            assert_eq!(count_pixels(&target, RED), 0);
        }
    }

    #[test]
    fn geometry_behind_the_near_plane_is_clipped_not_mirrored() {
        for (width, height) in NEAR_PLANE_SIZES {
            let size = format!("{width}x{height}");
            let mut target = framebuffer(width, height);
            let pool = TaskPool::new(0);
            let camera = level_eye_camera(width, height, 10.0, 500.0); // near at y = -40

            // A big triangle straddling the near plane: one vertex is behind
            // the eye.
            let mut list = DrawList::default();
            let a = list.add_vertex(Vec3::new(-30.0, -100.0, 0.0), RED); // behind the eye
            let b = list.add_vertex(Vec3::new(30.0, 20.0, -20.0), RED);
            let c = list.add_vertex(Vec3::new(30.0, 20.0, 20.0), RED);
            list.add_triangle(a, b, c);

            let mut rasterizer = Rasterizer::default();
            let stats =
                rendered(rasterizer.render(&list, &camera, &mut target, &serial_options(&pool)));
            // Clipping the triangle against the near plane yields a quad,
            // hence two screen triangles rather than one.
            assert_eq!(stats.triangles_submitted, 1, "{size}");
            assert!(stats.triangles_rasterised >= 1, "{size}");

            // Every written depth must be inside [0, 1]: a vertex that slipped
            // through the divide with a negative w lands outside it.
            let out_of_range = target
                .depth()
                .iter()
                .filter(|&depth| !(0.0..=1.0).contains(depth))
                .count();
            assert_eq!(out_of_range, 0, "{size}");
            assert!(
                count_pixels(&target, RED) > 0,
                "{size}: the visible part must still be drawn"
            );
        }
    }

    #[test]
    fn ground_running_behind_the_eye_covers_the_lower_view_and_nothing_above() {
        // Flat ground 10 units below a level eye, running from 950 units behind
        // it to 1050 ahead. Worked out without the program: the eye looks
        // horizontally, so the horizon is the image's middle row and no ray
        // above it meets the ground. A ray through the bottom quarter descends
        // at least half as steeply as the frustum's lower edge, tan(22.5 deg)
        // for the default 45 degree field of view, so it meets the ground
        // within 10 / (0.5 * 0.414) = 48 units: inside the ground, past the
        // near plane, within the far one, and within 1.78 * 0.414 * 48 = 36
        // units of the centre line across the widest image here. Both triangles
        // cross the eye plane, which is the case clipping at w = 1e-6 lost.
        for (width, height) in NEAR_PLANE_SIZES {
            let size = format!("{width}x{height}");
            let mut target = framebuffer(width, height);
            let pool = TaskPool::new(0);
            let camera = level_eye_camera(width, height, 1.0, 5000.0);

            let mut list = DrawList::default();
            let v0 = list.add_vertex(Vec3::new(-1000.0, -1000.0, -10.0), RED);
            let v1 = list.add_vertex(Vec3::new(1000.0, -1000.0, -10.0), RED);
            let v2 = list.add_vertex(Vec3::new(1000.0, 1000.0, -10.0), RED);
            let v3 = list.add_vertex(Vec3::new(-1000.0, 1000.0, -10.0), RED);
            list.add_triangle(v0, v1, v2);
            list.add_triangle(v0, v2, v3);

            let mut rasterizer = Rasterizer::default();
            rendered(rasterizer.render(&list, &camera, &mut target, &serial_options(&pool)));

            let mut sky_not_background = 0;
            let mut ground_not_red = 0;
            for y in 0..height {
                for x in 0..width {
                    if y < height / 2 {
                        sky_not_background += usize::from(target.color_at(x, y) != BACKGROUND);
                    } else if y >= height * 3 / 4 {
                        ground_not_red += usize::from(target.color_at(x, y) != RED);
                    }
                }
            }
            assert_eq!(
                sky_not_background, 0,
                "{size}: nothing may be drawn above the horizon"
            );
            assert_eq!(
                ground_not_red, 0,
                "{size}: every pixel of the bottom quarter looks at the ground"
            );
        }
    }

    #[test]
    fn a_line_running_behind_the_eye_draws_its_visible_part() {
        // A centreline on the same ground, from 50 units behind the eye to 150
        // ahead, directly below it: it projects onto the image's centre column,
        // from the bottom edge (ground 24 units ahead) up towards the horizon.
        for (width, height) in NEAR_PLANE_SIZES {
            let size = format!("{width}x{height}");
            let mut target = framebuffer(width, height);
            let pool = TaskPool::new(0);
            let camera = level_eye_camera(width, height, 1.0, 5000.0);

            let mut list = DrawList::default();
            list.add_segment(
                Vec3::new(0.0, -100.0, -10.0),
                Vec3::new(0.0, 100.0, -10.0),
                BLUE,
                3.0,
                0.0,
            );

            let mut rasterizer = Rasterizer::default();
            rendered(rasterizer.render(&list, &camera, &mut target, &serial_options(&pool)));
            // The centre column's pixel centres are half a pixel from the line,
            // inside its 3 px width.
            assert_eq!(
                target.color_at(width / 2, height - 1),
                BLUE,
                "{size}: the part under the eye"
            );
            assert_eq!(target.color_at(width / 2, height * 3 / 4), BLUE, "{size}");
            assert_eq!(
                target.color_at(width / 2, height / 4),
                BACKGROUND,
                "{size}: above the horizon"
            );
        }
    }

    #[test]
    fn clipping_at_the_guard_band_does_not_move_a_visible_edge() {
        // A triangle whose long edge is the line z = x / 2 through the image
        // centre and whose other two edges are at least m / 2 units away.
        // Inside the image it covers exactly the pixels whose centres are below
        // that line, for any m large enough to put the other edges out of
        // sight: clipping it must cut the triangle, not bend the edge that
        // shows. At m = 1e10 the unclipped screen coordinates overflow an int,
        // and a cut computed in float lands at the image centre instead of on
        // the band (2 + -3.1e8 is -3.1e8 in float), losing a quarter of the
        // image - both checked by hand-mutating the C++ rasteriser (2026-09-23:
        // 2048 and 768 wrong pixels respectively).
        //
        // A level elevation, not plan view: Top is clamped just short of
        // looking straight down (camera.rs), so a vertex 1e10 away in y leaks
        // thousands of units into depth, the near plane cuts the triangle small
        // before the guard band sees it, and that hides the precision this is
        // about.
        let mut camera = plan_camera(64, 64);
        camera.set_standard_view(StandardView::Front); // x to the right, z up, exactly
        let pool = TaskPool::new(0);
        for m in [1.0e3, 1.0e6, 1.0e10] {
            let mut target = framebuffer(64, 64);
            let mut list = DrawList::default();
            let a = list.add_vertex(Vec3::new(-m, 0.0, -m / 2.0), RED);
            let b = list.add_vertex(Vec3::new(m, 0.0, m / 2.0), RED);
            let c = list.add_vertex(Vec3::new(m, 0.0, -m), RED);
            list.add_triangle(a, b, c);

            let mut rasterizer = Rasterizer::default();
            rendered(rasterizer.render(&list, &camera, &mut target, &serial_options(&pool)));

            // One unit per pixel, origin at the centre: pixel (col, row) has
            // its centre at x = col + 0.5 - 32, z = 32 - row - 0.5, so its
            // offset from the line, z - x / 2 = 47.25 - row - col / 2, is an
            // odd multiple of 0.25 - never zero, so every pixel has a right
            // answer.
            let mut wrong = 0;
            for row in 0..64 {
                for col in 0..64 {
                    let x = f64::from(col) + 0.5 - 32.0;
                    let z = 32.0 - f64::from(row) - 0.5;
                    let expected = if z - x / 2.0 < 0.0 { RED } else { BACKGROUND };
                    wrong += usize::from(target.color_at(col, row) != expected);
                }
            }
            assert_eq!(wrong, 0, "m = {m:e}");
        }
    }

    #[test]
    fn a_line_far_longer_than_the_image_draws_across_all_of_it() {
        // Horizontal, at y = 0.25: screen row 31.75, so a 1 px line covers rows
        // [31.25, 32.25] and every centre on row 31 (31.5) is inside it. Its
        // ends are m units off either side; at m = 1e10 they overflowed an int
        // unclipped.
        let camera = plan_camera(64, 64);
        let pool = TaskPool::new(0);
        for m in [1.0e3, 1.0e6, 1.0e10] {
            let mut target = framebuffer(64, 64);
            let mut list = DrawList::default();
            list.add_segment(
                Vec3::new(-m, 0.25, 0.0),
                Vec3::new(m, 0.25, 0.0),
                BLUE,
                1.0,
                0.0,
            );

            let mut rasterizer = Rasterizer::default();
            rendered(rasterizer.render(&list, &camera, &mut target, &serial_options(&pool)));
            let missing = (0..64)
                .filter(|&col| target.color_at(col, 31) != BLUE)
                .count();
            assert_eq!(missing, 0, "m = {m:e}");
        }
    }

    #[test]
    fn a_point_centred_just_outside_the_image_still_draws_the_part_inside() {
        // Centred one unit beyond the right edge, 5 px across: its left two
        // columns are inside the image. Rejecting points by their centre alone
        // would lose them, which is one reason the guard band is wider than the
        // image.
        let mut target = framebuffer(64, 64);
        let camera = plan_camera(64, 64);
        let pool = TaskPool::new(0);

        let mut list = DrawList::default();
        let v = list.add_vertex(Vec3::new(33.0, 0.0, 0.0), GREEN); // screen x = 65
        list.add_point(v, 5.0, 0.0);

        let mut rasterizer = Rasterizer::default();
        rendered(rasterizer.render(&list, &camera, &mut target, &serial_options(&pool)));
        assert_eq!(target.color_at(63, 32), GREEN);
    }

    #[test]
    fn a_triangle_entirely_behind_the_eye_draws_nothing() {
        let mut target = framebuffer(64, 64);
        let pool = TaskPool::new(0);

        let mut camera = Camera::default();
        camera.set_viewport_size(64, 64);
        camera.set_projection(Projection::Perspective);
        camera.set_standard_view(StandardView::Front);
        camera.set_target(Vec3::new(0.0, 0.0, 0.0));
        camera.set_distance(50.0);
        camera.set_depth_range(1.0, 500.0);

        let mut list = DrawList::default();
        let a = list.add_vertex(Vec3::new(-10.0, -200.0, 0.0), RED);
        let b = list.add_vertex(Vec3::new(10.0, -200.0, 0.0), RED);
        let c = list.add_vertex(Vec3::new(0.0, -180.0, 10.0), RED);
        list.add_triangle(a, b, c);

        let mut rasterizer = Rasterizer::default();
        let stats =
            rendered(rasterizer.render(&list, &camera, &mut target, &serial_options(&pool)));
        assert_eq!(stats.triangles_rasterised, 0);
        assert_eq!(count_pixels(&target, BACKGROUND), 64 * 64);
    }

    #[test]
    fn lines_honour_their_pixel_width() {
        let camera = plan_camera(100, 100);
        let pool = TaskPool::new(0);

        for width in [1.0f32, 3.0, 7.0] {
            let mut target = framebuffer(100, 100);
            let mut list = DrawList::default();
            // 60 world units = 60 pixels, horizontal across the middle.
            list.add_segment(
                Vec3::new(-30.0, 0.0, 0.0),
                Vec3::new(30.0, 0.0, 0.0),
                BLUE,
                width,
                0.0,
            );

            let mut rasterizer = Rasterizer::default();
            let stats =
                rendered(rasterizer.render(&list, &camera, &mut target, &serial_options(&pool)));
            assert_eq!(stats.lines_submitted, 1);

            let filled = count_pixels(&target, BLUE) as f64;
            let expected = 60.0 * f64::from(width);
            assert!(
                (filled - expected).abs() <= expected * 0.25 + 60.0,
                "width {width:?} filled {filled:?}"
            );
        }
    }

    #[test]
    fn a_line_seen_end_on_still_leaves_a_mark() {
        let mut target = framebuffer(64, 64);
        let camera = plan_camera(64, 64); // looking straight down
        let pool = TaskPool::new(0);

        // A vertical pipe: in plan view its two ends land on the same pixel. A
        // naive widening divides by a zero length and the pipe vanishes.
        let mut list = DrawList::default();
        list.add_segment(
            Vec3::new(0.0, 0.0, -5.0),
            Vec3::new(0.0, 0.0, 5.0),
            BLUE,
            3.0,
            0.0,
        );

        let mut rasterizer = Rasterizer::default();
        rendered(rasterizer.render(&list, &camera, &mut target, &serial_options(&pool)));
        assert!(count_pixels(&target, BLUE) > 0);
    }

    #[test]
    fn depth_bias_lifts_wireframe_off_the_surface_it_bounds() {
        let camera = plan_camera(60, 60);
        let pool = TaskPool::new(0);

        let render_with = |bias: f32| {
            let mut target = framebuffer(60, 60);
            let mut list = DrawList::default();
            let v0 = list.add_vertex(Vec3::new(-25.0, -25.0, 0.0), RED);
            let v1 = list.add_vertex(Vec3::new(25.0, -25.0, 0.0), RED);
            let v2 = list.add_vertex(Vec3::new(25.0, 25.0, 0.0), RED);
            list.add_triangle(v0, v1, v2);
            // Exactly coplanar with the triangle's edge: without a bias this is
            // a coin toss per pixel.
            list.add_segment(
                Vec3::new(-25.0, -25.0, 0.0),
                Vec3::new(25.0, -25.0, 0.0),
                GREEN,
                3.0,
                bias,
            );

            let mut rasterizer = Rasterizer::default();
            rendered(rasterizer.render(&list, &camera, &mut target, &serial_options(&pool)));
            count_pixels(&target, GREEN)
        };

        // One pixel footprint towards the eye (DrawLine::depth_bias is in those
        // since the depth-range fit; it was NDC depth, 1e-4 here).
        let biased = render_with(1.0);
        assert!(
            biased > 100,
            "a biased edge must win against the surface it lies in"
        );
    }

    #[test]
    fn points_are_drawn_at_exactly_their_requested_pixel_size() {
        // plan_camera: one unit per pixel, the origin at the image's centre
        // (32, 32) and world +y up the screen. A point covers the pixels whose
        // CENTRES (i + 0.5) lie in [c - size/2, c + size/2).
        //   size 5 at world (0.5, 0.5) -> screen (32.5, 31.5): x in [30, 35)
        //     holds centres 30.5..34.5, five of them; y likewise: 25 pixels.
        //   size 4 at world (0, 0) -> screen (32, 32): [30, 34) holds
        //     30.5..33.5, four: 16 pixels.
        // It drew floor(c - h)..floor(c + h), a pixel too many each way: 36 and
        // 25 (audit REN-10).
        let drawn = |at: Vec3, size: f32| {
            let mut target = framebuffer(64, 64);
            let camera = plan_camera(64, 64);
            let pool = TaskPool::new(0);
            let mut list = DrawList::default();
            let v = list.add_vertex(at, GREEN);
            list.add_point(v, size, 0.0);
            let mut rasterizer = Rasterizer::default();
            let stats =
                rendered(rasterizer.render(&list, &camera, &mut target, &serial_options(&pool)));
            assert_eq!(stats.points_submitted, 1);
            count_pixels(&target, GREEN)
        };
        assert_eq!(drawn(Vec3::new(0.5, 0.5, 0.0), 5.0), 25);
        assert_eq!(drawn(Vec3::new(0.0, 0.0, 0.0), 4.0), 16);
    }

    #[test]
    fn backface_culling_discards_only_the_far_side() {
        let camera = plan_camera(50, 50);
        let pool = TaskPool::new(0);

        // Counter-clockwise in plan, so it faces up, towards a camera looking
        // down.
        let mut list = DrawList::default();
        let v0 = list.add_vertex(Vec3::new(-20.0, -20.0, 0.0), RED);
        let v1 = list.add_vertex(Vec3::new(20.0, -20.0, 0.0), RED);
        let v2 = list.add_vertex(Vec3::new(0.0, 20.0, 0.0), RED);
        list.add_triangle(v0, v1, v2);

        let mut options = RenderOptions {
            backface_cull: true,
            ..serial_options(&pool)
        };

        let mut from_above = framebuffer(50, 50);
        let mut rasterizer = Rasterizer::default();
        rendered(rasterizer.render(&list, &camera, &mut from_above, &options));
        assert!(
            count_pixels(&from_above, RED) > 0,
            "the front face must survive culling"
        );

        let mut below = plan_camera(50, 50);
        below.set_standard_view(StandardView::Bottom);
        let mut from_below = framebuffer(50, 50);
        rendered(rasterizer.render(&list, &below, &mut from_below, &options));
        assert_eq!(
            count_pixels(&from_below, RED),
            0,
            "the back face must be culled"
        );

        // And with culling off it is visible from both sides, which is the
        // default precisely because a survey surface is inspected from
        // underneath.
        options.backface_cull = false;
        rendered(rasterizer.render(&list, &below, &mut from_below, &options));
        assert!(count_pixels(&from_below, RED) > 0);
    }

    #[test]
    fn thread_count_does_not_change_a_single_pixel() {
        // Rule 7. The binning is chunked by primitive index rather than by
        // core, so a tile visits its primitives in the same order on every
        // machine. This is the assertion that keeps that true.
        let camera = plan_camera(320, 240);

        let mut list = DrawList::default();
        // Enough primitives to span several chunks, at overlapping depths so
        // that any order-dependence shows up as a different pixel.
        for i in 0..400 {
            let t = f64::from(i);
            let x = (t * 7.0) % 300.0 - 150.0;
            let y = (t * 11.0) % 220.0 - 110.0;
            let z = (t * 3.0) % 20.0 - 10.0;
            let color = rgba(
                (i * 3 % 256) as u8,
                (i * 7 % 256) as u8,
                (i * 11 % 256) as u8,
            );
            let a = list.add_vertex(Vec3::new(x, y, z), color);
            let b = list.add_vertex(Vec3::new(x + 40.0, y + 5.0, z), color);
            let c = list.add_vertex(Vec3::new(x + 10.0, y + 35.0, z), color);
            list.add_triangle(a, b, c);
            list.add_line(a, b, 2.0, 0.0);
            list.add_point(c, 3.0, 0.0);
        }

        let mut reference: Vec<Rgba> = Vec::new();
        let mut reference_depth: Vec<f32> = Vec::new();
        for workers in [0, 1, 3, 7] {
            let pool = TaskPool::new(workers);
            let mut target = framebuffer(320, 240);
            let mut rasterizer = Rasterizer::default();
            let options = RenderOptions {
                background: BACKGROUND,
                pool: Some(&pool),
                ..RenderOptions::default()
            };
            let stats = rendered(rasterizer.render(&list, &camera, &mut target, &options));
            assert!(stats.fragments > 0);

            if reference.is_empty() {
                reference = target.color().to_vec();
                reference_depth = target.depth().to_vec();
            } else {
                assert!(
                    target.color() == reference.as_slice(),
                    "{workers} workers changed the colour"
                );
                assert!(
                    target.depth() == reference_depth.as_slice(),
                    "{workers} workers changed the depth"
                );
            }
        }
    }

    #[test]
    fn reusing_a_rasterizer_gives_the_same_frame_as_a_fresh_one() {
        // The scratch buffers are reused across frames to avoid reallocating; a
        // stale entry left in one would show as a ghost of the previous frame.
        let camera = plan_camera(80, 80);
        let pool = TaskPool::new(2);

        let mut first = DrawList::default();
        let a = first.add_vertex(Vec3::new(-30.0, -30.0, 0.0), RED);
        let b = first.add_vertex(Vec3::new(30.0, -30.0, 0.0), RED);
        let c = first.add_vertex(Vec3::new(0.0, 30.0, 0.0), RED);
        first.add_triangle(a, b, c);

        let mut second = DrawList::default();
        let d = second.add_vertex(Vec3::new(-10.0, -10.0, 0.0), GREEN);
        let e = second.add_vertex(Vec3::new(10.0, -10.0, 0.0), GREEN);
        let f = second.add_vertex(Vec3::new(0.0, 10.0, 0.0), GREEN);
        second.add_triangle(d, e, f);

        let mut reused = Rasterizer::default();
        let mut target = framebuffer(80, 80);
        let options = RenderOptions {
            background: BACKGROUND,
            pool: Some(&pool),
            ..RenderOptions::default()
        };
        rendered(reused.render(&first, &camera, &mut target, &options));
        rendered(reused.render(&second, &camera, &mut target, &options));

        let mut fresh = Rasterizer::default();
        let mut expected = framebuffer(80, 80);
        rendered(fresh.render(&second, &camera, &mut expected, &options));

        assert!(target.color() == expected.color());
        assert_eq!(
            count_pixels(&target, RED),
            0,
            "the first frame must leave no ghost"
        );
    }

    #[test]
    fn primitives_spanning_many_tiles_are_drawn_whole() {
        // One triangle covering the entire image crosses every 64x64 tile
        // boundary. A binning mistake shows up as missing tiles, which is
        // visible as an exact pixel count rather than as "looks fine".
        let mut target = framebuffer(200, 150);
        let camera = plan_camera(200, 150);
        let pool = TaskPool::new(4);

        let mut list = DrawList::default();
        let v0 = list.add_vertex(Vec3::new(-500.0, -500.0, 0.0), RED);
        let v1 = list.add_vertex(Vec3::new(500.0, -500.0, 0.0), RED);
        let v2 = list.add_vertex(Vec3::new(500.0, 500.0, 0.0), RED);
        let v3 = list.add_vertex(Vec3::new(-500.0, 500.0, 0.0), RED);
        list.add_triangle(v0, v1, v2);
        list.add_triangle(v0, v2, v3);

        let mut rasterizer = Rasterizer::default();
        let options = RenderOptions {
            background: BACKGROUND,
            pool: Some(&pool),
            ..RenderOptions::default()
        };
        let stats = rendered(rasterizer.render(&list, &camera, &mut target, &options));
        assert_eq!(
            count_pixels(&target, RED),
            200 * 150,
            "every tile must be covered"
        );
        assert_eq!(stats.tiles, 4 * 3); // ceil(200/64) x ceil(150/64)
    }

    #[test]
    fn off_screen_geometry_costs_no_fragments() {
        let mut target = framebuffer(64, 64);
        let camera = plan_camera(64, 64);
        let pool = TaskPool::new(0);

        let mut list = DrawList::default();
        let a = list.add_vertex(Vec3::new(1000.0, 1000.0, 0.0), RED);
        let b = list.add_vertex(Vec3::new(1100.0, 1000.0, 0.0), RED);
        let c = list.add_vertex(Vec3::new(1000.0, 1100.0, 0.0), RED);
        list.add_triangle(a, b, c);

        let mut rasterizer = Rasterizer::default();
        let stats =
            rendered(rasterizer.render(&list, &camera, &mut target, &serial_options(&pool)));
        assert_eq!(stats.fragments, 0);
        assert_eq!(
            stats.bin_entries, 0,
            "geometry outside the image must not be binned at all"
        );
    }
}
