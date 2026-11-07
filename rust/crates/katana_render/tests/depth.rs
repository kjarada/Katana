//! Depth: what the 3D view got wrong before the depth-range fit (docs/render.md,
//! "Depth"): the twin of tests/render/test_depth.cpp. Each test is one of the
//! defects measured on real survey data, rebuilt as the smallest scene that
//! shows it:
//!
//! - z-fighting: near = far * 1e-5 over standard [0, 1] depth gave a framed
//!   scene a few hundred distinct depths; a surface 5 cm above another 1 km
//!   across lost three quarters of its pixels.
//! - x-ray: a constant NDC bias bigger than the whole scene's depth span
//!   showed linework through a building.
//! - zoom-out: the depth range was fixed at frame(), so eight wheel notches
//!   out pushed the model past the far plane.
//! - ortho zoom-in: an orthographic eye a zoom had moved into the model cut
//!   away everything in front of it.
//!
//! The expected values are properties, not captured numbers: a hidden line has
//! no pixels, a surface above another shows every pixel it shows alone.

pub mod support;

mod render_depth {
    use crate::support::count_pixels;
    use katana_core::task_pool::TaskPool;
    use katana_core::text::format_exact_real;
    use katana_math::{AABB, Vec3};
    use katana_render::{
        Camera, DrawList, Framebuffer, Projection, Rasterizer, RenderOptions, Rgba, StandardView,
        VertexIndex, rgba,
    };

    const BACKGROUND: Rgba = rgba(0, 0, 0);
    const GROUND: Rgba = rgba(200, 40, 40);
    const DESIGN: Rgba = rgba(40, 200, 40);
    const BUILDING: Rgba = rgba(40, 40, 200);
    const LINE: Rgba = rgba(250, 250, 250);

    // The scene's own linework bias (the cad layer's
    // SceneOptions::entityDepthBias): one and a half pixel footprints towards
    // the eye.
    const ENTITY_BIAS: f32 = 1.5;

    // A rectangle cut into n x n cells, as a surface is: one big triangle
    // across a kilometre would test the barycentric rounding of a shape no TIN
    // has. Flat at height z.
    #[allow(
        clippy::too_many_arguments,
        reason = "The C++ helper's arguments, its defaulted slopes left out."
    )]
    fn add_plane(
        list: &mut DrawList,
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
        z: f64,
        n: i32,
        color: Rgba,
    ) {
        add_sloped_plane(list, x0, y0, x1, y1, z, n, color, 0.0, 0.0);
    }

    // The same at height z + slope_x * x + slope_y * y.
    #[allow(
        clippy::too_many_arguments,
        reason = "The C++ helper's arguments, one a coordinate or a slope."
    )]
    fn add_sloped_plane(
        list: &mut DrawList,
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
        z: f64,
        n: i32,
        color: Rgba,
        slope_x: f64,
        slope_y: f64,
    ) {
        let base = list.positions.len() as VertexIndex;
        for j in 0..=n {
            for i in 0..=n {
                let x = x0 + (x1 - x0) * f64::from(i) / f64::from(n);
                let y = y0 + (y1 - y0) * f64::from(j) / f64::from(n);
                list.add_vertex(Vec3::new(x, y, z + slope_x * x + slope_y * y), color);
            }
        }
        let at = |i: i32, j: i32| base + (j * (n + 1) + i) as VertexIndex;
        for j in 0..n {
            for i in 0..n {
                list.add_triangle(at(i, j), at(i + 1, j), at(i + 1, j + 1));
                list.add_triangle(at(i, j), at(i + 1, j + 1), at(i, j + 1));
            }
        }
    }

    // A closed box, twelve triangles.
    fn add_box(list: &mut DrawList, lo: Vec3, hi: Vec3, color: Rgba) {
        let mut c: [VertexIndex; 8] = [0; 8];
        for (corner, slot) in c.iter_mut().enumerate() {
            *slot = list.add_vertex(
                Vec3::new(
                    if (corner & 1) != 0 { hi.x } else { lo.x },
                    if (corner & 2) != 0 { hi.y } else { lo.y },
                    if (corner & 4) != 0 { hi.z } else { lo.z },
                ),
                color,
            );
        }
        let faces = [
            [0, 1, 3, 2],
            [4, 6, 7, 5],
            [0, 4, 5, 1],
            [2, 3, 7, 6],
            [0, 2, 6, 4],
            [1, 5, 7, 3],
        ];
        for f in faces {
            list.add_triangle(c[f[0]], c[f[1]], c[f[2]]);
            list.add_triangle(c[f[0]], c[f[2]], c[f[3]]);
        }
    }

    fn append(to: &mut DrawList, from: &DrawList) {
        let base = to.positions.len() as VertexIndex;
        to.positions.extend_from_slice(&from.positions);
        to.colors.extend_from_slice(&from.colors);
        for t in &from.triangles {
            to.add_triangle(base + t.a, base + t.b, base + t.c);
        }
        for l in &from.lines {
            to.add_line(base + l.a, base + l.b, l.width, l.depth_bias);
        }
        for p in &from.points {
            to.add_point(base + p.a, p.size, p.depth_bias);
        }
    }

    fn render(list: &DrawList, camera: &Camera) -> Framebuffer {
        let mut target = Framebuffer::create(camera.viewport_width(), camera.viewport_height())
            .expect("the camera's size is a framebuffer's");
        let pool = TaskPool::new(0);
        let options = RenderOptions {
            background: BACKGROUND,
            pool: Some(&pool),
            ..RenderOptions::default()
        };
        let mut rasterizer = Rasterizer::default();
        rasterizer
            .render(list, camera, &mut target, &options)
            .expect("the frame renders");
        target
    }

    // The measured case: a 1 km ground and a 400 m design surface `gap` above
    // its middle, framed from the south-west as the 3D view opens.
    struct Overlap {
        ground: DrawList,
        design: DrawList,
        bounds: AABB,
    }

    impl Overlap {
        fn new(gap: f64) -> Self {
            let mut ground = DrawList::default();
            let mut design = DrawList::default();
            add_plane(&mut ground, -500.0, -500.0, 500.0, 500.0, 0.0, 20, GROUND);
            add_plane(&mut design, -200.0, -200.0, 200.0, 200.0, gap, 8, DESIGN);
            let mut bounds = ground.bounds();
            bounds.expand_box(design.bounds());
            Self {
                ground,
                design,
                bounds,
            }
        }
    }

    fn framed_camera(bounds: AABB, projection: Projection) -> Camera {
        let mut camera = Camera::default();
        camera.set_viewport_size(1200, 800);
        camera.set_projection(projection);
        camera.set_standard_view(StandardView::IsoSouthWest);
        assert!(camera.frame(bounds));
        camera
    }

    // What the 3D view does to a camera before every frame.
    const ZOOM_PER_NOTCH: f64 = 1.15;

    // The scene's own point size (SceneOptions::pointSize); the bias is the
    // linework's, ENTITY_BIAS.
    const POINT_SIZE: f32 = 5.0;
    const POINT: Rgba = rgba(250, 250, 0);

    // A 400 m site rising 5% to the east and 2% to the north, the slope the
    // review measured on, and 20 x 20 survey points lying exactly on it.
    struct SlopedSite {
        ground: DrawList,
        points: DrawList,
        bounds: AABB,
    }

    impl Default for SlopedSite {
        fn default() -> Self {
            let mut ground = DrawList::default();
            add_sloped_plane(
                &mut ground,
                0.0,
                0.0,
                400.0,
                400.0,
                0.0,
                40,
                GROUND,
                0.05,
                0.02,
            );
            let mut points = DrawList::default();
            for j in 0..20 {
                for i in 0..20 {
                    let x = 10.0 + 20.0 * f64::from(i);
                    let y = 10.0 + 20.0 * f64::from(j);
                    let v = points.add_vertex(Vec3::new(x, y, 0.05 * x + 0.02 * y), POINT);
                    points.add_point(v, POINT_SIZE, ENTITY_BIAS);
                }
            }
            let bounds = ground.bounds();
            Self {
                ground,
                points,
                bounds,
            }
        }
    }

    // Renders `passes` one after another into one buffer, as the 3D view draws
    // its layers.
    fn render_passes(passes: &[&DrawList], camera: &Camera) -> Framebuffer {
        let mut target = Framebuffer::create(camera.viewport_width(), camera.viewport_height())
            .expect("the camera's size is a framebuffer's");
        let pool = TaskPool::new(0);
        let mut options = RenderOptions {
            background: BACKGROUND,
            pool: Some(&pool),
            ..RenderOptions::default()
        };
        let mut rasterizer = Rasterizer::default();
        for list in passes {
            rasterizer
                .render(list, camera, &mut target, &options)
                .expect("the pass renders");
            options.clear = false;
        }
        target
    }

    // A camera south-west of the scene at `elevation`, framing `bounds`.
    fn iso_camera(bounds: AABB, projection: Projection, elevation: f64) -> Camera {
        let mut camera = Camera::default();
        camera.set_viewport_size(1200, 800);
        camera.set_projection(projection);
        camera.set_standard_view(StandardView::IsoSouthWest);
        camera.set_orientation(camera.azimuth(), elevation);
        assert!(camera.frame(bounds));
        camera
    }

    // The view of the building tests: from the south, 200 m away and 0.25 rad
    // above level.
    fn south_camera(projection: Projection) -> Camera {
        let mut camera = Camera::default();
        camera.set_viewport_size(640, 480);
        camera.set_projection(projection);
        camera.set_standard_view(StandardView::Front); // looking north
        camera.set_orientation(camera.azimuth(), 0.25);
        camera.set_target(Vec3::new(0.0, 0.0, 10.0));
        camera.set_distance(200.0);
        camera.set_orthographic_height(120.0);
        camera
    }

    #[test]
    fn a_surface_five_centimetres_above_another_a_kilometre_across_shows_every_pixel() {
        // near = far * 1e-5 on standard depth lost 74% of these pixels to the
        // ground. Fitted to the scene, the two planes 5 cm apart at 1-2 km are
        // hundreds of float steps apart in depth: d = n (f - z) / ((f - n) z)
        // moves by n f / ((f - n) z^2) * 0.05 = about 4e-5 for n = 1200, f =
        // 2700 and z = 1800, against a float step near 0.5 of 6e-8.
        for projection in [Projection::Perspective, Projection::Orthographic] {
            let how = format!("{}", projection as i32);
            let scene = Overlap::new(0.05);
            let camera = framed_camera(scene.bounds, projection);

            let alone = count_pixels(&render(&scene.design, &camera), DESIGN);
            assert!(
                alone > 10000,
                "{how}: the design surface is not in view, so this proves nothing"
            );

            // Either order: the ground drawn first or last must lose everywhere
            // the design covers it.
            let mut ground_first = scene.ground.clone();
            append(&mut ground_first, &scene.design);
            assert_eq!(
                count_pixels(&render(&ground_first, &camera), DESIGN),
                alone,
                "{how}"
            );
            let mut design_first = scene.design.clone();
            append(&mut design_first, &scene.ground);
            assert_eq!(
                count_pixels(&render(&design_first, &camera), DESIGN),
                alone,
                "{how}"
            );
        }
    }

    #[test]
    fn a_line_on_the_ground_behind_a_building_is_hidden() {
        // A 40 x 40 x 25 m building at the origin and a line on the ground 30 m
        // behind it, seen from the south 200 m away and 0.25 rad above level.
        // The eye is at about (0, -194, 59); the ray to the line's middle
        // (0, 30, 0) meets the building's south face (y = -20) at
        // z = 59 * (1 - 174 / 224) = 13, inside its 0..25, and the line's ends
        // (x = +-10) cross that face at x = +-7.8, inside its +-20: the whole
        // line is behind the building. The line carries the scene's bias; the
        // constant NDC bias it replaced was larger than the building's whole
        // depth span and drew the line through it.
        let mut building = DrawList::default();
        add_box(
            &mut building,
            Vec3::new(-20.0, -20.0, 0.0),
            Vec3::new(20.0, 20.0, 25.0),
            BUILDING,
        );
        let mut line = DrawList::default();
        line.add_segment(
            Vec3::new(-10.0, 30.0, 0.0),
            Vec3::new(10.0, 30.0, 0.0),
            LINE,
            2.0,
            ENTITY_BIAS,
        );

        for projection in [Projection::Perspective, Projection::Orthographic] {
            let how = format!("{}", projection as i32);
            let mut camera = south_camera(projection);
            let mut bounds = building.bounds();
            bounds.expand_box(line.bounds());
            assert!(camera.fit_depth_range(bounds));

            assert!(
                count_pixels(&render(&line, &camera), LINE) > 20,
                "{how}: the line is not in view, so its being hidden proves nothing"
            );
            let mut building_first = building.clone();
            append(&mut building_first, &line);
            let seen = render(&building_first, &camera);
            assert_eq!(count_pixels(&seen, LINE), 0, "{how}");
            assert!(count_pixels(&seen, BUILDING) > 1000, "{how}");
            let mut line_first = line.clone();
            append(&mut line_first, &building);
            assert_eq!(
                count_pixels(&render(&line_first, &camera), LINE),
                0,
                "{how}"
            );
        }
    }

    #[test]
    fn a_line_lying_on_the_ground_is_drawn_whole_over_it() {
        // The bias exists for this: linework draped on a surface, exactly in
        // its plane, must win against it everywhere it would draw on its own -
        // even seen at a slant, where the ground a pixel beside the line's
        // centre is nearer than the line by a pixel of its depth slope.
        let mut ground = DrawList::default();
        add_plane(&mut ground, -500.0, -500.0, 500.0, 500.0, 0.0, 20, GROUND);
        let mut line = DrawList::default();
        line.add_segment(
            Vec3::new(-300.0, -100.0, 0.0),
            Vec3::new(300.0, 150.0, 0.0),
            LINE,
            2.0,
            ENTITY_BIAS,
        );
        line.add_segment(
            Vec3::new(-100.0, -300.0, 0.0),
            Vec3::new(50.0, 300.0, 0.0),
            LINE,
            1.0,
            ENTITY_BIAS,
        );

        for projection in [Projection::Perspective, Projection::Orthographic] {
            let how = format!("{}", projection as i32);
            let camera = framed_camera(ground.bounds(), projection);
            let alone = count_pixels(&render(&line, &camera), LINE);
            assert!(alone > 500, "{how}");
            let mut both = ground.clone();
            append(&mut both, &line);
            assert_eq!(count_pixels(&render(&both, &camera), LINE), alone, "{how}");
        }
    }

    #[test]
    fn zooming_out_eight_notches_never_blanks_the_view() {
        // Eight wheel notches out is the camera 1.15^8 = 3.06 times further
        // away. The planes frame() chose put the far one about 3.2 radii past
        // the target, so the whole model went past it and the view drew three
        // fragments. The view fits the depth range every frame.
        let scene = Overlap::new(0.05);
        let mut both = scene.ground.clone();
        append(&mut both, &scene.design);
        for projection in [Projection::Perspective, Projection::Orthographic] {
            let how = format!("{}", projection as i32);
            let mut camera = framed_camera(scene.bounds, projection);
            for _ in 0..8 {
                camera.dolly_at_pixel(ZOOM_PER_NOTCH, 600.0, 400.0);
            }
            assert!(camera.fit_depth_range(scene.bounds));
            let seen = render(&both, &camera);
            assert!(count_pixels(&seen, GROUND) > 1000, "{how}");
            // And still without z-fighting: all of the design it shows alone.
            assert_eq!(
                count_pixels(&seen, DESIGN),
                count_pixels(&render(&scene.design, &camera), DESIGN),
                "{how}"
            );
        }
    }

    #[test]
    fn an_orthographic_zoom_into_the_model_does_not_cut_its_front_away() {
        // Sixteen notches in at the middle of an orthographic view moves the
        // eye to 1/1.15^16 = 0.107 of the framing distance, about 160 m from
        // the target: inside the ground's reach back towards the viewer (its
        // south-west corner is 707 m * cos 35 = 577 m nearer than the target),
        // so a near plane in front of the eye cut the ground away. The view is
        // then 0.107 of the framed 1500 x 1000 m: 160 x 107 m, and the 400 m
        // design seen from 35 degrees up is a diamond 283 m and 163 m from its
        // middle to its corners: 80 / 283 + 53 / 163 = 0.61 < 1, so every pixel
        // is the design.
        let scene = Overlap::new(0.05);
        let mut both = scene.ground.clone();
        append(&mut both, &scene.design);
        let mut camera = framed_camera(scene.bounds, Projection::Orthographic);
        for _ in 0..16 {
            camera.dolly_at_pixel(1.0 / ZOOM_PER_NOTCH, 600.0, 400.0);
        }
        assert!(camera.fit_depth_range(scene.bounds));
        assert!(
            camera.orthographic_standoff() > 0.0,
            "the eye was not inside, so this proves nothing"
        );
        let seen = render(&both, &camera);
        assert_eq!(count_pixels(&seen, DESIGN), 1200 * 800);
    }

    #[test]
    fn the_orthographic_standoff_moves_only_the_depths_not_the_picture() {
        // Where along the view direction an orthographic eye sits changes no
        // ray's screen position, so moving it back to clear the scene must not
        // move a single projected point. The probes are in the target's own
        // plane, in front of the eye before the move as after it.
        let mut camera = Camera::default();
        camera.set_viewport_size(400, 300);
        camera.set_projection(Projection::Orthographic);
        camera.set_standard_view(StandardView::IsoSouthWest);
        camera.set_target(Vec3::new(10.0, 20.0, 5.0));
        camera.set_distance(1.0); // inside the box below
        camera.set_orthographic_height(60.0);
        let probes = [
            camera.target(),
            camera.target() + camera.right() * 12.0,
            camera.target() + camera.up() * -7.0 + camera.right() * -20.0,
        ];
        let before: Vec<Vec3> = probes
            .iter()
            .map(|&probe| camera.project(probe).expect("a probe projects"))
            .collect();
        assert!(camera.fit_depth_range(AABB::new(
            Vec3::new(-100.0, -100.0, -20.0),
            Vec3::new(100.0, 100.0, 40.0)
        )));
        assert!(
            camera.orthographic_standoff() > 0.0,
            "the eye was not moved, so this proves nothing"
        );
        for (&probe, earlier) in probes.iter().zip(&before) {
            let after = camera.project(probe).expect("a probe projects");
            assert!((after.x - earlier.x).abs() <= 1e-9);
            assert!((after.y - earlier.y).abs() <= 1e-9);
        }
        // The target is still the middle of the view: (200, 150).
        assert!((before[0].x - 200.0).abs() <= 1e-9);
        assert!((before[0].y - 150.0).abs() <= 1e-9);
    }

    #[test]
    fn with_the_eye_inside_the_scene_surfaces_a_kilometre_away_still_separate() {
        // Walking a model: the eye 30 m above a 4 km ground, inside the scene's
        // box (a 100 m mast behind it makes the box reach above the eye), so
        // the near plane falls back to far * 1e-6: the far corner is about
        // 2000 m away, so near is about 2 mm. A strip 5 cm above the ground
        // 700-1300 m ahead, seen 0.03 rad below level, has the ground 0.05 /
        // 0.03 = 1.7 m behind it along each ray: n / z^2 * 1.7 = 2e-3 / 1e6 *
        // 1.7 = 3e-9 of depth at z = 1000, where the depth is n / z = 2e-6 and
        // a float's step is 2e-13 - thousands of steps. On standard depth the
        // same scene sits at 1 - 2e-6, where a float's step is 6e-8, and the
        // strip and the ground there are the same number - which is why the
        // depth is reversed (camera.rs, Clip).
        let mut ground = DrawList::default();
        add_plane(
            &mut ground,
            -2000.0,
            -2000.0,
            2000.0,
            2000.0,
            0.0,
            80,
            GROUND,
        );
        add_box(
            &mut ground,
            Vec3::new(-5.0, -1900.0, 0.0),
            Vec3::new(5.0, -1890.0, 100.0),
            BUILDING,
        );
        let mut design = DrawList::default();
        add_plane(&mut design, -150.0, 700.0, 150.0, 1300.0, 0.05, 12, DESIGN);

        let mut camera = Camera::default();
        camera.set_viewport_size(800, 500);
        camera.set_projection(Projection::Perspective);
        camera.set_standard_view(StandardView::Front); // looking north
        camera.set_orientation(camera.azimuth(), 0.03);
        camera.set_target(Vec3::new(0.0, 1000.0, 0.0));
        camera.set_distance(1000.0); // the eye is 1000 m south and 30 m up
        let mut bounds = ground.bounds();
        bounds.expand_box(design.bounds());
        assert!(camera.fit_depth_range(bounds));
        assert!(
            bounds.contains(camera.eye()),
            "the eye must be inside the scene"
        );
        assert!(camera.near_plane() < 0.01);

        let alone = count_pixels(&render(&design, &camera), DESIGN);
        assert!(alone > 2000);
        let mut both = ground.clone();
        append(&mut both, &design);
        assert_eq!(count_pixels(&render(&both, &camera), DESIGN), alone);
    }

    #[test]
    fn survey_points_lying_on_a_sloped_surface_are_drawn_whole() {
        // A point is a 5 x 5 square at ONE depth, its centre's. Tested pixel by
        // pixel against the surface it lies on, it lost its lower rows: seen at
        // elevation e, a row k pixels below the centre looks at ground about
        // k / tan(e) footprints nearer. The surface is pushed back by one pixel
        // of its own slope and the point pulled 1.5 footprints, so the row two
        // down is covered once 1 / tan(e) > 1.5 (e below 0.59 rad; the slope
        // facing the eye steepens it at the iso view's 0.61) and the row one
        // down too below 0.3 rad. Decided at its centre, whose pixel centre is
        // at most half a pixel of slope from the point's own, a point is drawn
        // whole: in the surface's list or in a later pass.
        let site = SlopedSite::default();
        let mut together = site.ground.clone();
        append(&mut together, &site.points);
        for projection in [Projection::Perspective, Projection::Orthographic] {
            for elevation in [0.61, 0.25, 0.12] {
                let how = format!("{} at {}", projection as i32, format_exact_real(elevation));
                let camera = iso_camera(site.bounds, projection, elevation);

                let alone = count_pixels(&render(&site.points, &camera), POINT);
                assert!(
                    alone > 2000,
                    "{how}: the points are not in view, so this proves nothing"
                );
                assert_eq!(
                    count_pixels(&render(&together, &camera), POINT),
                    alone,
                    "{how}"
                );
                assert_eq!(
                    count_pixels(
                        &render_passes(&[&site.ground, &site.points], &camera),
                        POINT
                    ),
                    alone,
                    "{how}"
                );
            }
        }
    }

    #[test]
    fn a_point_on_the_ground_behind_a_building_is_hidden() {
        // The middle of the line in ALineOnTheGroundBehindABuildingIsHidden,
        // (0, 30, 0): the ray from the eye to it meets the building's south
        // face at z = 13 (worked there). Deciding a point at its centre must
        // not let it through, in the building's list or in a later pass.
        let mut building = DrawList::default();
        add_box(
            &mut building,
            Vec3::new(-20.0, -20.0, 0.0),
            Vec3::new(20.0, 20.0, 25.0),
            BUILDING,
        );
        let mut point = DrawList::default();
        let v = point.add_vertex(Vec3::new(0.0, 30.0, 0.0), POINT);
        point.add_point(v, POINT_SIZE, ENTITY_BIAS);
        let mut together = building.clone();
        append(&mut together, &point);

        for projection in [Projection::Perspective, Projection::Orthographic] {
            let how = format!("{}", projection as i32);
            let mut camera = south_camera(projection);
            let mut bounds = building.bounds();
            bounds.expand_box(point.bounds());
            assert!(camera.fit_depth_range(bounds));

            assert_eq!(
                count_pixels(&render(&point, &camera), POINT),
                25,
                "{how}: the point is not in view, so its being hidden proves nothing"
            );
            assert_eq!(count_pixels(&render(&together, &camera), POINT), 0, "{how}");
            assert_eq!(
                count_pixels(&render_passes(&[&building, &point], &camera), POINT),
                0,
                "{how}"
            );
        }
    }

    #[test]
    fn of_two_overlapping_points_the_nearer_is_on_top_whichever_is_drawn_first() {
        // Seen from above in an orthographic view 100 m high on 500 px, a metre
        // is 5 px and the target (0, 0) is the pixel corner (250, 250). A point
        // at x = 0.25 m is centred on 251.25 and covers the pixels whose
        // centres are in [248.75, 253.75): 249..253; one at x = -0.15 m covers
        // 247..251. Both at y = 0.05 m cover the same five rows, so they share
        // 3 x 5 = 15 pixels (a quarter pixel off the grid, so no rounding moves
        // a square). Those go to the nearer, higher point in either order: it
        // shows all 25, the lower one the other 10.
        const NEAR: Rgba = rgba(0, 250, 250);
        const FAR: Rgba = rgba(250, 0, 250);
        let near_point = |list: &mut DrawList| {
            let v = list.add_vertex(Vec3::new(0.25, 0.05, 10.0), NEAR);
            list.add_point(v, POINT_SIZE, ENTITY_BIAS);
        };
        let far_point = |list: &mut DrawList| {
            let v = list.add_vertex(Vec3::new(-0.15, 0.05, 0.0), FAR);
            list.add_point(v, POINT_SIZE, ENTITY_BIAS);
        };
        for near_first in [true, false] {
            let mut list = DrawList::default();
            if near_first {
                near_point(&mut list);
                far_point(&mut list);
            } else {
                far_point(&mut list);
                near_point(&mut list);
            }
            let mut camera = Camera::default();
            camera.set_viewport_size(500, 500);
            camera.set_projection(Projection::Orthographic);
            camera.set_standard_view(StandardView::Top);
            camera.set_target(Vec3::new(0.0, 0.0, 5.0));
            camera.set_orthographic_height(100.0);
            assert!(camera.fit_depth_range(list.bounds()));
            let seen = render(&list, &camera);
            assert_eq!(count_pixels(&seen, NEAR), 25, "near_first={near_first}");
            assert_eq!(count_pixels(&seen, FAR), 10, "near_first={near_first}");
        }
    }

    #[test]
    fn a_pass_that_writes_no_depth_is_covered_by_whatever_is_drawn_after_it() {
        // The grid and the surface edges are drawn so (the cad layer's
        // renderLayers): a pass with depth_write off draws its colour where the
        // test passes and leaves the depth buffer as it found it, so a surface
        // 5 cm BEHIND it, drawn next, is tested against the cleared buffer and
        // covers every pixel it draws alone.
        let scene = Overlap::new(0.05); // the design 5 cm above the ground
        let camera = framed_camera(scene.bounds, Projection::Perspective);
        let alone = count_pixels(&render(&scene.ground, &camera), GROUND);
        assert!(alone > 10000);

        let mut target = Framebuffer::create(camera.viewport_width(), camera.viewport_height())
            .expect("the camera's size is a framebuffer's");
        let pool = TaskPool::new(0);
        let mut options = RenderOptions {
            background: BACKGROUND,
            pool: Some(&pool),
            depth_write: false,
            ..RenderOptions::default()
        };
        let mut rasterizer = Rasterizer::default();
        rasterizer
            .render(&scene.design, &camera, &mut target, &options)
            .expect("the design renders");
        assert!(count_pixels(&target, DESIGN) > 10000);
        assert!(
            target.depth().iter().all(|&depth| depth == 0.0),
            "the pass wrote depth"
        );
        options.depth_write = true;
        options.clear = false;
        rasterizer
            .render(&scene.ground, &camera, &mut target, &options)
            .expect("the ground renders");
        assert_eq!(count_pixels(&target, GROUND), alone);
        assert_eq!(count_pixels(&target, DESIGN), 0);
    }

    #[test]
    fn points_draw_the_same_pixels_on_the_calling_thread_and_across_threads() {
        // Up to 4096 points are decided on the calling thread (a dispatch costs
        // more; decide_points), more across the pool. The same frame both ways:
        // 64 x 64 = 4096 points on the sloped site, then the same plus one point
        // 50 m under the ground in the middle of the view - 4097, so the pool's
        // path, and hidden, so it adds no pixel. Colour and depth must match
        // byte for byte on 0, 3 and 7 workers, and the points must still be
        // whole.
        let mut ground = DrawList::default();
        add_sloped_plane(
            &mut ground,
            0.0,
            0.0,
            400.0,
            400.0,
            0.0,
            40,
            GROUND,
            0.05,
            0.02,
        );
        let mut points = DrawList::default();
        for j in 0..64 {
            for i in 0..64 {
                let x = 3.0 + 6.2 * f64::from(i);
                let y = 3.0 + 6.2 * f64::from(j);
                let v = points.add_vertex(Vec3::new(x, y, 0.05 * x + 0.02 * y), POINT);
                points.add_point(v, POINT_SIZE, ENTITY_BIAS);
            }
        }
        let mut inline_scene = ground.clone();
        append(&mut inline_scene, &points);
        let mut pooled_scene = inline_scene.clone();
        let hidden = pooled_scene.add_vertex(Vec3::new(200.0, 200.0, 14.0 - 50.0), POINT);
        pooled_scene.add_point(hidden, POINT_SIZE, ENTITY_BIAS);

        let mut camera = Camera::default();
        camera.set_viewport_size(800, 600);
        camera.set_standard_view(StandardView::IsoSouthWest);
        camera.set_orientation(camera.azimuth(), 0.25);
        assert!(camera.frame(ground.bounds()));
        // Framed on the ground alone, so the depth range must reach the point
        // under it too, and the point must be on the image, or it is clipped
        // before it is counted and both scenes take the same path.
        let bounds = pooled_scene.bounds();
        assert!(camera.fit_depth_range(bounds));
        let under = camera
            .project(Vec3::new(200.0, 200.0, 14.0 - 50.0))
            .expect("the hidden point projects");
        assert!(
            under.x > 0.0 && under.x < 800.0 && under.y > 0.0 && under.y < 600.0,
            "the hidden point is off the image, so this proves nothing"
        );
        let alone = count_pixels(&render(&points, &camera), POINT);
        assert!(alone > 20000);

        let draw = |list: &DrawList, workers: usize| {
            let mut target = Framebuffer::create(camera.viewport_width(), camera.viewport_height())
                .expect("the camera's size is a framebuffer's");
            let pool = TaskPool::new(workers);
            let options = RenderOptions {
                background: BACKGROUND,
                pool: Some(&pool),
                ..RenderOptions::default()
            };
            let mut rasterizer = Rasterizer::default();
            rasterizer
                .render(list, &camera, &mut target, &options)
                .expect("the frame renders");
            target
        };
        let reference = draw(&inline_scene, 3);
        assert_eq!(count_pixels(&reference, POINT), alone);
        for workers in [0, 3, 7] {
            let pooled = draw(&pooled_scene, workers);
            assert!(
                pooled.color() == reference.color(),
                "{workers} workers changed a colour"
            );
            assert!(
                pooled.depth() == reference.depth(),
                "{workers} workers changed a depth"
            );
        }
    }

    #[test]
    fn survey_points_on_the_vertices_of_draped_strings_are_drawn_whole_over_them() {
        // Survey points usually sit on the strings through them, in the same
        // pass with the same pull. Decided against a depth buffer that already
        // held the pass's lines, a point whose centre pixel a string crossed
        // tied with it and vanished whole - the yellow survey points along the
        // kerbs of a real archive did at 12 notches in. Solids and earlier
        // passes hide a point; linework of its own pass does not: it is decided
        // after the pass's filled triangles and before its lines. The site's
        // 20 x 20 points, each row joined by a string through them, on the
        // sloped plane drawn first in the same list and as an earlier pass.
        let site = SlopedSite::default();
        const STRING: Rgba = rgba(255, 255, 255);
        let mut strings = DrawList::default();
        for j in 0..20 {
            let y = 10.0 + 20.0 * f64::from(j);
            let from = strings.add_vertex(Vec3::new(10.0, y, 0.05 * 10.0 + 0.02 * y), STRING);
            let to = strings.add_vertex(Vec3::new(390.0, y, 0.05 * 390.0 + 0.02 * y), STRING);
            strings.add_line(from, to, 2.0, ENTITY_BIAS);
        }
        let mut drawing = strings.clone();
        append(&mut drawing, &site.points);
        let mut together = site.ground.clone();
        append(&mut together, &drawing);
        for projection in [Projection::Perspective, Projection::Orthographic] {
            for elevation in [0.61, 0.25, 0.12] {
                let how = format!("{} at {}", projection as i32, format_exact_real(elevation));
                let camera = iso_camera(site.bounds, projection, elevation);

                let alone = count_pixels(&render(&site.points, &camera), POINT);
                assert!(alone > 2000, "{how}");
                assert_eq!(
                    count_pixels(&render(&drawing, &camera), POINT),
                    alone,
                    "{how}"
                );
                assert_eq!(
                    count_pixels(&render(&together, &camera), POINT),
                    alone,
                    "{how}"
                );
                assert_eq!(
                    count_pixels(&render_passes(&[&site.ground, &drawing], &camera), POINT),
                    alone,
                    "{how}"
                );
            }
        }
    }
}
