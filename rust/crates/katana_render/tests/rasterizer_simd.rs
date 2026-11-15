//! The rasteriser's fast paths (rasterizer.rs, "conservative coverage", and
//! src/simd/): tiles left out at binning, rows bounded before the fill, eight
//! pixels a step on AVX2 (four on NEON), four vertices a step in the
//! transform: the twin of tests/render/test_rasterizer_simd.cpp.
//!
//! Each one is only allowed to change the time a frame takes. So every scene
//! here is drawn at both SIMD levels and on 0, 1, 3 and 7 worker threads, and
//! all eight frames must agree in every colour and every depth bit (Rule 7);
//! where a count can be worked out by hand, it is checked too.
//!
//! The scenes aim at the edges of the fast paths: triangles far larger than a
//! tile, slivers thinner than a pixel, lines across the whole view, points,
//! equal depths, perspective (where 1/w and so the colour vary per pixel), and
//! passes that neither clear nor write depth.
//!
//! Where the C++ compares the scalar level with itself on a processor without
//! kernels, these fail instead: a comparison of the scalar path with itself
//! does not count as having compared the kernels (docs/rust.md, "Step 1").

pub mod support;

mod render_rasterizer_simd {
    use crate::support::{at_simd_level, count_colour, require_kernel};
    use katana_core::cpu_features::SimdLevel;
    use katana_core::task_pool::TaskPool;
    use katana_math::Vec3;
    use katana_render::{
        Camera, DrawList, Framebuffer, Projection, Rasterizer, RenderOptions, RenderStats, Rgba,
        StandardView, rgba,
    };

    const BACKGROUND: Rgba = rgba(0, 0, 0);
    const RED: Rgba = rgba(255, 0, 0);
    const GREEN: Rgba = rgba(0, 255, 0);
    const WIDTH: i32 = 640;
    const HEIGHT: i32 = 480;

    // Straight down, orthographic, one world unit per pixel, the origin at the
    // image centre: world (x, y) is pixel column x + 320, row 240 - y.
    fn plan_camera() -> Camera {
        let mut camera = Camera::default();
        camera.set_viewport_size(WIDTH, HEIGHT);
        camera.set_projection(Projection::Orthographic);
        camera.set_standard_view(StandardView::Top);
        camera.set_target(Vec3::new(0.0, 0.0, 0.0));
        camera.set_distance(1000.0);
        camera.set_orthographic_height(f64::from(HEIGHT));
        camera.set_depth_range(1.0, 2000.0);
        camera
    }

    // An eye low over the scene, looking across it, so 1/w - and with it the
    // perspective-correct colour - changes from pixel to pixel.
    fn perspective_camera() -> Camera {
        let mut camera = Camera::default();
        camera.set_viewport_size(WIDTH, HEIGHT);
        camera.set_projection(Projection::Perspective);
        camera.set_standard_view(StandardView::IsoSouthWest);
        camera.set_target(Vec3::new(0.0, 0.0, 0.0));
        camera.set_distance(400.0);
        camera.set_depth_range(1.0, 4000.0);
        camera
    }

    struct Frame {
        color: Vec<Rgba>,
        depth: Vec<f32>,
        stats: RenderStats,
    }

    // One frame at `level` on a pool of `workers` threads. `before`, when
    // given, is drawn first into the same target, as a view draws several
    // passes.
    fn draw(
        list: &DrawList,
        camera: &Camera,
        level: SimdLevel,
        workers: usize,
        options: &RenderOptions<'_>,
        before: Option<&DrawList>,
    ) -> Frame {
        at_simd_level(level, || {
            let pool = TaskPool::new(workers);
            let mut target =
                Framebuffer::create(WIDTH, HEIGHT).expect("the test's size is a framebuffer's");
            let mut rasterizer = Rasterizer::default();
            if let Some(before) = before {
                let first = RenderOptions {
                    background: BACKGROUND,
                    pool: Some(&pool),
                    ..RenderOptions::default()
                };
                rasterizer
                    .render(before, camera, &mut target, &first)
                    .expect("the first pass renders");
            }
            let mut mine = *options;
            mine.pool = Some(&pool);
            let stats = rasterizer
                .render(list, camera, &mut target, &mine)
                .expect("the frame renders");
            Frame {
                color: target.color().to_vec(),
                depth: target.depth().to_vec(),
                stats,
            }
        })
    }

    // Draws `list` at both levels on every thread count and expects one frame.
    // Returns it, drawn at the scalar level on one thread.
    fn every_way_the_same(
        list: &DrawList,
        camera: &Camera,
        options: RenderOptions<'_>,
        before: Option<&DrawList>,
    ) -> Frame {
        let kernel = require_kernel();
        let options = RenderOptions {
            background: BACKGROUND,
            ..options
        };
        let reference = draw(list, camera, SimdLevel::Scalar, 0, &options, before);
        for level in [SimdLevel::Scalar, kernel] {
            for workers in [0, 1, 3, 7] {
                let frame = draw(list, camera, level, workers, &options, before);
                let how = format!("{level} on {workers} workers");
                assert!(frame.color == reference.color, "{how} changed a colour");
                assert!(frame.depth == reference.depth, "{how} changed a depth");
                assert_eq!(frame.stats.fragments, reference.stats.fragments, "{how}");
            }
        }
        reference
    }

    // An axis-aligned square of two triangles, corners (x0, y0) and (x1, y1) at
    // height z.
    fn add_square(list: &mut DrawList, x0: f64, y0: f64, x1: f64, y1: f64, z: f64, color: Rgba) {
        let a = list.add_vertex(Vec3::new(x0, y0, z), color);
        let b = list.add_vertex(Vec3::new(x1, y0, z), color);
        let c = list.add_vertex(Vec3::new(x1, y1, z), color);
        let d = list.add_vertex(Vec3::new(x0, y1, z), color);
        list.add_triangle(a, b, c);
        list.add_triangle(a, c, d);
    }

    // The C++'s fixed linear congruential sequence, [0, 1) from its top 53 bits.
    struct Sequence {
        state: u64,
    }

    impl Sequence {
        fn uniform(&mut self) -> f64 {
            self.state = self
                .state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (self.state >> 11) as f64 * (1.0 / 9007199254740992.0)
        }
    }

    #[test]
    fn a_square_many_tiles_across_fills_exactly_its_own_pixels() {
        // 300 x 200 world units at one unit a pixel, corners on pixel edges: the
        // pixel centres inside are exactly 300 x 200 = 60,000. The diagonal the
        // two triangles share runs 3 columns for every 2 rows, so it passes a
        // quarter of a pixel from the nearest centres and no pixel is written
        // twice. It spans 6 x 5 tiles, some wholly inside one triangle, the
        // rest cut by the diagonal - the kernel's covered and tested cases
        // both.
        let mut list = DrawList::default();
        add_square(&mut list, -150.0, -100.0, 150.0, 100.0, 0.0, RED);
        let frame = every_way_the_same(&list, &plan_camera(), RenderOptions::default(), None);
        assert_eq!(count_colour(&frame.color, RED), 60000);
        assert_eq!(frame.stats.fragments, 60000);
    }

    #[test]
    fn overlapping_large_triangles_in_perspective_agree_at_every_level() {
        // Large triangles at crossing depths, each vertex its own colour, seen
        // in perspective: every pixel runs the depth test and the
        // perspective-correct colour. 36 vertices: nine steps of the transform
        // kernel's four.
        let mut list = DrawList::default();
        for i in 0..12 {
            let t = f64::from(i);
            let a = list.add_vertex(
                Vec3::new(-300.0 + 40.0 * t, -250.0, 10.0 * t.sin()),
                rgba((20 * i) as u8, 40, 200),
            );
            let b = list.add_vertex(
                Vec3::new(250.0, -200.0 + 30.0 * t, -20.0 + 3.0 * t),
                rgba(200, (15 * i) as u8, 60),
            );
            let c = list.add_vertex(
                Vec3::new(-100.0 + 10.0 * t, 300.0, 30.0 - 5.0 * t),
                rgba(90, 160, (18 * i) as u8),
            );
            list.add_triangle(a, b, c);
        }
        let frame =
            every_way_the_same(&list, &perspective_camera(), RenderOptions::default(), None);
        assert!(
            frame.stats.fragments > 100000,
            "the scene must reach the fast paths at all"
        );
    }

    #[test]
    fn slivers_thinner_than_a_pixel_agree_at_every_level() {
        // Long triangles a fraction of a pixel wide at many angles: their boxes
        // are large, so they take the row bounds and the kernel, while almost
        // no pixel centre is inside them. Where the float edge tests are
        // closest to their rounding, the bounds must still let every one of
        // those through.
        let mut list = DrawList::default();
        for i in 0..90 {
            let angle = 0.0349 * f64::from(i); // 2 degrees a step
            let c = angle.cos();
            let s = angle.sin();
            let width = 0.05 + 0.01 * f64::from(i % 7);
            let color = rgba((i * 2) as u8, 200, 100);
            let height = 0.1 * f64::from(i);
            let a = list.add_vertex(Vec3::new(-220.0 * c, -220.0 * s, height), color);
            let b = list.add_vertex(Vec3::new(220.0 * c, 220.0 * s, height), color);
            let d = list.add_vertex(
                Vec3::new(220.0 * c - width * s, 220.0 * s + width * c, height),
                color,
            );
            list.add_triangle(a, b, d);
        }
        let frame = every_way_the_same(&list, &plan_camera(), RenderOptions::default(), None);
        assert!(frame.stats.fragments > 0);
    }

    #[test]
    fn lines_across_the_view_are_binned_only_in_the_tiles_they_cross() {
        // A 1 px line corner to corner of a 640 x 480 view, 10 x 8 tiles of
        // 64 px. Widened into two triangles, each with a box the size of the
        // view, it used to be binned in all 80 tiles twice over, 160 entries.
        // Going 10 tiles across and 8 down it crosses at most 10 + 8 - 1 = 17
        // of them, and each triangle is now kept only where it can reach: at
        // most 34.
        let mut list = DrawList::default();
        let a = list.add_vertex(Vec3::new(-319.0, -239.0, 0.0), RED);
        let b = list.add_vertex(Vec3::new(319.0, 239.0, 0.0), RED);
        list.add_line(a, b, 1.0, 0.0);
        // And lines at other angles and widths, to compare frames on.
        for i in 0..24 {
            let angle = 0.2618 * f64::from(i); // 15 degrees a step
            let p = list.add_vertex(Vec3::new(0.0, 0.0, 1.0), GREEN);
            let q = list.add_vertex(
                Vec3::new(300.0 * angle.cos(), 230.0 * angle.sin(), 1.0),
                rgba(0, 255, (10 * i) as u8),
            );
            list.add_line(p, q, 1.0 + (i % 4) as f32, 0.0);
        }
        let mut diagonal = DrawList::default();
        let d0 = diagonal.add_vertex(Vec3::new(-319.0, -239.0, 0.0), RED);
        let d1 = diagonal.add_vertex(Vec3::new(319.0, 239.0, 0.0), RED);
        diagonal.add_line(d0, d1, 1.0, 0.0);
        let alone = every_way_the_same(&diagonal, &plan_camera(), RenderOptions::default(), None);
        assert!(alone.stats.bin_entries <= 34);
        assert!(alone.stats.fragments > 0);
        every_way_the_same(&list, &plan_camera(), RenderOptions::default(), None);
    }

    #[test]
    fn points_among_large_triangles_agree_at_every_level() {
        // Points send the frame through its three sweeps (filled, decide, lines
        // and points), and the filled sweep here is large triangles.
        let mut list = DrawList::default();
        add_square(&mut list, -200.0, -150.0, 200.0, 150.0, 0.0, RED);
        for i in 0..60 {
            let p = list.add_vertex(
                Vec3::new(
                    -190.0 + 6.5 * f64::from(i),
                    -140.0 + 4.7 * f64::from(i),
                    f64::from(i % 3) - 1.0,
                ),
                rgba(0, (4 * i) as u8, 255),
            );
            list.add_point(p, 1.0 + (i % 7) as f32, 1.5);
        }
        let frame = every_way_the_same(&list, &plan_camera(), RenderOptions::default(), None);
        assert!(frame.stats.fragments > 120000);
    }

    #[test]
    fn at_equal_depths_the_first_submitted_large_square_keeps_every_pixel() {
        // Reversed Z with a strictly-greater test: of two equal depths the
        // first written stays. Both squares cover the same 200 x 200 = 40,000
        // pixels, red submitted first, so not one pixel may turn green - at any
        // level, on any number of threads.
        let mut list = DrawList::default();
        add_square(&mut list, -100.0, -100.0, 100.0, 100.0, 5.0, RED);
        add_square(&mut list, -100.0, -100.0, 100.0, 100.0, 5.0, GREEN);
        let frame = every_way_the_same(&list, &plan_camera(), RenderOptions::default(), None);
        assert_eq!(count_colour(&frame.color, RED), 40000);
        assert_eq!(count_colour(&frame.color, GREEN), 0);
    }

    #[test]
    fn the_clear_leaves_far_depth_and_a_pass_without_clear_or_depth_keeps_it() {
        // The clear writes the reversed-Z far value, 0, everywhere. A second
        // pass that neither clears nor writes depth paints its square over the
        // first pass's (it is tested against depth 0 outside and against the
        // first square's depth inside, where it is nearer) and leaves the depth
        // buffer exactly as the first pass left it.
        let mut first = DrawList::default();
        add_square(&mut first, -150.0, -100.0, 0.0, 100.0, 0.0, RED);
        let mut second = DrawList::default();
        add_square(&mut second, -50.0, -50.0, 150.0, 50.0, 10.0, GREEN);

        let cleared = every_way_the_same(&first, &plan_camera(), RenderOptions::default(), None);
        assert_eq!(count_colour(&cleared.color, RED), 150 * 200);
        assert_eq!(
            cleared.depth.iter().filter(|&&depth| depth == 0.0).count(),
            (WIDTH * HEIGHT) as usize - 150 * 200
        );

        let overlay = RenderOptions {
            clear: false,
            depth_write: false,
            ..RenderOptions::default()
        };
        let layered = every_way_the_same(&second, &plan_camera(), overlay, Some(&first));
        assert_eq!(count_colour(&layered.color, GREEN), 200 * 100);
        assert_eq!(count_colour(&layered.color, RED), 150 * 200 - 50 * 100);
        assert!(
            layered.depth == cleared.depth,
            "a pass without depth writes wrote depth"
        );
    }

    #[test]
    fn triangles_of_every_size_from_a_fixed_seed_agree_at_every_level() {
        // 3,000 triangles from a fixed linear congruential sequence, from a
        // tenth of a pixel to several times the view, at random depths, in
        // perspective: every size cutoff is crossed many times over, and many
        // boxes are cut by the guard band and the near plane.
        let mut list = DrawList::default();
        let mut sequence = Sequence {
            state: 0x2545F4914F6CDD1D,
        };
        for _ in 0..3000 {
            let size = 10.0_f64.powf(-1.0 + 4.0 * sequence.uniform()); // 0.1 to 1000 units
            let cx = 600.0 * sequence.uniform() - 300.0;
            let cy = 600.0 * sequence.uniform() - 300.0;
            let cz = 100.0 * sequence.uniform() - 50.0;
            // The colour is drawn before the corners, as in the C++; within one
            // call the C++'s argument order is unspecified, and this takes them
            // left to right.
            let red = (255.0 * sequence.uniform()) as u8;
            let green = (255.0 * sequence.uniform()) as u8;
            let blue = (255.0 * sequence.uniform()) as u8;
            let color = rgba(red, green, blue);
            let mut corner = || {
                let x = cx + size * (sequence.uniform() - 0.5);
                let y = cy + size * (sequence.uniform() - 0.5);
                let z = cz + 0.2 * size * (sequence.uniform() - 0.5);
                Vec3::new(x, y, z)
            };
            let a = list.add_vertex(corner(), color);
            let b = list.add_vertex(corner(), rgba(255, 255, 255));
            let c = list.add_vertex(corner(), color);
            list.add_triangle(a, b, c);
        }
        let frame =
            every_way_the_same(&list, &perspective_camera(), RenderOptions::default(), None);
        assert!(frame.stats.fragments > 0);
    }
}
