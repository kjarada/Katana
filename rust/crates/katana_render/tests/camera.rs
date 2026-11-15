//! Camera conventions (PLAN.MD Phase 15): the twin of tests/render/test_camera.cpp.
//!
//! Every assertion here pins a convention the rasteriser and the pick ray both
//! assume: Z up, screen +y down, NDC depth 1 at the near plane (reversed Z).
//! Getting one of them backwards produces a picture that looks almost right,
//! which is the worst kind of wrong, so they are asserted rather than commented.

#![allow(
    clippy::excessive_precision,
    clippy::approx_constant,
    reason = "Preserve decimal constants and expected values copied verbatim from the C++ oracle."
)]

pub mod support;

mod render_camera {
    use crate::support::{double_eq, near, near_because};
    use katana_math::{AABB, Vec3};
    use katana_render::{Camera, Projection, StandardView};

    const PI: f64 = 3.14159265358979323846;

    fn default_camera(width: i32, height: i32) -> Camera {
        let mut camera = Camera::default();
        camera.set_viewport_size(width, height);
        camera.set_target(Vec3::new(0.0, 0.0, 0.0));
        camera.set_distance(100.0);
        camera
    }

    fn corner_of(bounds: AABB, corner: i32) -> Vec3 {
        Vec3::new(
            if (corner & 1) != 0 {
                bounds.max.x
            } else {
                bounds.min.x
            },
            if (corner & 2) != 0 {
                bounds.max.y
            } else {
                bounds.min.y
            },
            if (corner & 4) != 0 {
                bounds.max.z
            } else {
                bounds.min.z
            },
        )
    }

    #[test]
    fn top_view_looks_straight_down_with_north_up() {
        let mut camera = default_camera(800, 600);
        camera.set_standard_view(StandardView::Top);

        // Looking down: forward is -Z (within the pole guard).
        near(camera.forward().z, -1.0, 1e-3);
        assert!(
            camera.eye().z > 0.0,
            "the eye must be above the ground in plan view"
        );

        // North (+Y) must appear at the top of the screen, i.e. along screen up.
        near(camera.up().y, 1.0, 1e-3);
        // East (+X) must appear to the right.
        near(camera.right().x, 1.0, 1e-3);
    }

    #[test]
    fn front_view_looks_north_with_elevation_up_the_screen() {
        let mut camera = default_camera(800, 600);
        camera.set_standard_view(StandardView::Front);

        near_because(camera.forward().y, 1.0, 1e-9, "front view looks north");
        near(camera.forward().z, 0.0, 1e-9);
        // Elevation (world +Z) is up the screen: that is what makes a section
        // view read as a section rather than as a plan.
        near(camera.up().z, 1.0, 1e-9);
        near(camera.right().x, 1.0, 1e-9);
    }

    #[test]
    fn screen_y_increases_downwards() {
        let mut camera = default_camera(800, 600);
        camera.set_standard_view(StandardView::Top);
        camera.set_projection(Projection::Orthographic);
        camera.set_orthographic_height(100.0);
        camera.set_depth_range(1.0, 1000.0);

        let north = camera
            .project(Vec3::new(0.0, 10.0, 0.0))
            .expect("north projects");
        let south = camera
            .project(Vec3::new(0.0, -10.0, 0.0))
            .expect("south projects");
        assert!(
            north.y < south.y,
            "north must be nearer the top of the image"
        );

        let centre = camera
            .project(Vec3::new(0.0, 0.0, 0.0))
            .expect("the centre projects");
        near(centre.x, 400.0, 1e-6);
        near(centre.y, 300.0, 1e-6);
    }

    #[test]
    fn depth_is_one_at_the_near_plane_and_zero_at_the_far_because_it_is_reversed() {
        // Reversed Z (camera.rs, Clip): nearer is LARGER. The rasteriser's
        // strictly-greater test and its clear to 0 both rest on this.
        for projection in [Projection::Perspective, Projection::Orthographic] {
            let mut camera = default_camera(800, 600);
            camera.set_projection(projection);
            camera.set_standard_view(StandardView::Front); // forward = +Y
            camera.set_target(Vec3::new(0.0, 0.0, 0.0));
            camera.set_distance(100.0);
            camera.set_depth_range(10.0, 210.0);
            camera.set_orthographic_height(50.0);

            // The eye is 100 south of the origin looking north, so the near
            // plane sits at y = -90 and the far plane at y = 110.
            let at_near = camera
                .project(Vec3::new(0.0, -90.0, 0.0))
                .expect("near projects");
            let at_far = camera
                .project(Vec3::new(0.0, 110.0, 0.0))
                .expect("far projects");
            let how = format!("{}", projection as i32);
            near_because(at_near.z, 1.0, 1e-9, &how);
            near_because(at_far.z, 0.0, 1e-9, &how);
        }
    }

    #[test]
    fn points_behind_the_eye_do_not_project() {
        let mut camera = default_camera(800, 600);
        camera.set_standard_view(StandardView::Front); // eye at y = -100, looking north
        camera.set_depth_range(1.0, 1000.0);

        assert!(
            camera.project(Vec3::new(0.0, -200.0, 0.0)).is_none(),
            "a point behind the eye would otherwise project to a mirrored pixel"
        );
        assert!(camera.project(Vec3::new(0.0, 0.0, 0.0)).is_some());
    }

    #[test]
    fn pick_ray_through_the_centre_pixel_is_the_view_direction() {
        let mut camera = default_camera(801, 601); // odd, so there IS a centre pixel
        camera.set_standard_view(StandardView::IsoSouthWest);

        let ray = camera.ray_through_pixel(400.0, 300.0);
        let forward = camera.forward();
        near(ray.direction.x, forward.x, 1e-9);
        near(ray.direction.y, forward.y, 1e-9);
        near(ray.direction.z, forward.z, 1e-9);
    }

    #[test]
    fn pick_ray_and_projection_are_inverses() {
        let mut camera = default_camera(800, 600);
        camera.set_standard_view(StandardView::IsoNorthEast);
        camera.set_target(Vec3::new(5.0, -3.0, 2.0));
        camera.set_distance(80.0);
        camera.set_depth_range(1.0, 1000.0);

        for projection in [Projection::Perspective, Projection::Orthographic] {
            camera.set_projection(projection);
            for (x, y) in [(123.0, 456.0), (7.0, 11.0), (799.0, 599.0)] {
                let ray = camera.ray_through_pixel(x, y);
                let world = ray.at(50.0);
                let back = camera.project(world).expect("a point on the ray projects");
                // The ray targets the CENTRE of the pixel, so projecting a point
                // on it lands half a pixel on.
                near(back.x, x + 0.5, 1e-6);
                near(back.y, y + 0.5, 1e-6);
            }
        }
    }

    #[test]
    fn framing_a_box_puts_every_corner_on_screen_from_any_direction() {
        let bounds = AABB::new(Vec3::new(100.0, 200.0, -5.0), Vec3::new(180.0, 260.0, 35.0));

        for projection in [Projection::Perspective, Projection::Orthographic] {
            let mut camera = default_camera(640, 480);
            camera.set_projection(projection);
            // frame() fits the corners as seen from where the camera looks, so
            // it is asked from each direction of a full turn.
            for step in 0..16 {
                camera.set_orientation(f64::from(step) * PI / 8.0, 0.4);
                assert!(camera.frame(bounds));
                for corner in 0..8 {
                    let screen = camera
                        .project(corner_of(bounds, corner))
                        .unwrap_or_else(|| panic!("corner {corner} step {step}"));
                    assert!(screen.x >= 0.0);
                    assert!(screen.x <= 640.0);
                    assert!(screen.y >= 0.0);
                    assert!(screen.y <= 480.0);
                    assert!(screen.z >= 0.0);
                    assert!(screen.z <= 1.0);
                }
            }
        }
    }

    #[test]
    fn an_orbit_after_framing_keeps_the_box_inside_the_fitted_depth_range() {
        // The view fits the depth range to the scene every frame. Orbiting a
        // full turn after one frame() must never put a corner past either plane
        // - the planes frame() chose alone did, by up to 3% of the range.
        let bounds = AABB::new(Vec3::new(100.0, 200.0, -5.0), Vec3::new(180.0, 260.0, 35.0));
        for projection in [Projection::Perspective, Projection::Orthographic] {
            let mut camera = default_camera(640, 480);
            camera.set_projection(projection);
            assert!(camera.frame(bounds));
            for step in 0..16 {
                camera.set_orientation(f64::from(step) * PI / 8.0, 0.4);
                assert!(camera.fit_depth_range(bounds));
                for corner in 0..8 {
                    let screen = camera
                        .project(corner_of(bounds, corner))
                        .unwrap_or_else(|| panic!("corner {corner} step {step}"));
                    assert!(screen.z >= 0.0);
                    assert!(screen.z <= 1.0);
                }
            }
        }
    }

    #[test]
    fn with_the_pivot_close_the_near_plane_keeps_a_thousandth_of_its_distance_in_front() {
        // The eye 1 m from a pivot at the origin, on a ground 2000 km square
        // and 1 m thick, seen from the south-west: forward is (1, 1, -1) /
        // sqrt 3 and the eye (-1, -1, 1) / sqrt 3. The far plane is the far
        // bottom corner, (1e6 + 1/sqrt 3, 1e6 + 1/sqrt 3, -1 - 1/sqrt 3) .
        // forward = 2e6 / sqrt 3 + 1 + 1/sqrt 3 deep, 1.1547e6 m, plus the
        // fit's pad of a thousandth of the box's diagonal, sqrt(8e12 + 1) /
        // 1000 = 2828 m. A millionth of that, 1.157 m, put the near plane
        // beyond the pivot and cut away what the view looks at. A thousandth
        // of the pivot's distance: 1 mm.
        let mut camera = default_camera(800, 600);
        camera.set_standard_view(StandardView::IsoSouthWest);
        camera.set_distance(1.0);
        let ground = AABB::new(
            Vec3::new(-1.0e6, -1.0e6, -1.0),
            Vec3::new(1.0e6, 1.0e6, 0.0),
        );
        assert!(camera.fit_depth_range(ground));
        const INV_SQRT3: f64 = 0.57735026918962573;
        let pad = 1.0e-3 * (8.0e12_f64 + 1.0).sqrt();
        near(
            camera.far_plane(),
            2.0e6 * INV_SQRT3 + 1.0 + INV_SQRT3 + pad,
            1e-6,
        );
        assert!(
            camera.far_plane() * Camera::NEAR_FAR_FLOOR > 1.0,
            "a millionth of the far plane would not reach the pivot, so this proves nothing"
        );
        double_eq(camera.near_plane(), 1.0e-3);
        let pivot = camera.project(camera.target()).expect("the pivot projects");
        assert!(pivot.z > 0.0);
        assert!(pivot.z < 1.0);

        // Not zoomed that deep, the planes are the ones they always were: a
        // millionth of the far one.
        camera.set_distance(5000.0);
        assert!(camera.fit_depth_range(ground));
        assert!(camera.far_plane() * Camera::NEAR_FAR_FLOOR < 5000.0 * Camera::NEAR_PIVOT_FLOOR);
        double_eq(
            camera.near_plane(),
            camera.far_plane() * Camera::NEAR_FAR_FLOOR,
        );
    }

    #[test]
    fn framing_a_long_corridor_fills_the_width_not_a_sliver_of_it() {
        // A 12 km x 200 m corridor seen from above in a 1600 x 1000 view. The
        // corners fit: half-width 6000 over an aspect of 1.6 needs a
        // half-height of 3750, against the corridor's own 100, so the width
        // decides and the corridor spans 1 / 1.06 = 94% of it. The bounding
        // sphere (radius 6002) made the view 2 x 6002 x 1.06 tall, 20 360 m
        // wide: 59%.
        let corridor = AABB::new(Vec3::new(0.0, 0.0, 10.0), Vec3::new(12000.0, 200.0, 40.0));
        for projection in [Projection::Orthographic, Projection::Perspective] {
            let mut camera = default_camera(1600, 1000);
            camera.set_projection(projection);
            camera.set_standard_view(StandardView::Top);
            assert!(camera.frame(corridor));
            let west = camera
                .project(Vec3::new(0.0, 100.0, 40.0))
                .expect("west projects");
            let east = camera
                .project(Vec3::new(12000.0, 100.0, 40.0))
                .expect("east projects");
            assert!((east.x - west.x) / 1600.0 > 0.9, "{}", projection as i32);
            assert!(west.x >= 0.0);
            assert!(east.x <= 1600.0);
        }
    }

    #[test]
    fn an_orthographic_frame_in_a_tall_view_keeps_the_sides_in() {
        // Audit REN-08: the orthographic height ignored the aspect, so a
        // portrait view (345 x 545, a docked 3D view beside the plan) cut both
        // sides off.
        let bounds = AABB::new(Vec3::new(0.0, 0.0, 0.0), Vec3::new(100.0, 50.0, 0.0));
        let mut camera = default_camera(345, 545);
        camera.set_projection(Projection::Orthographic);
        camera.set_standard_view(StandardView::Top);
        assert!(camera.frame(bounds));
        for corner in 0..4 {
            let screen = camera
                .project(Vec3::new(
                    if (corner & 1) != 0 { 100.0 } else { 0.0 },
                    if (corner & 2) != 0 { 50.0 } else { 0.0 },
                    0.0,
                ))
                .expect("a corner projects");
            assert!(screen.x >= 0.0);
            assert!(screen.x <= 345.0);
        }
    }

    #[test]
    fn framing_one_point_shows_a_patch_around_it_not_a_micrometre() {
        // Audit REN-07: a box of one point framed at a radius of 1e-7 m, and no
        // zooming out recovered a useful view. It is grown to a half-diagonal
        // of MINIMUM_FRAME_RADIUS (1 m): half 1 / sqrt 3 each way, so points
        // 0.5 m either side of it are in view and not at the view's edge.
        let point = Vec3::new(300000.0, 6250000.0, 42.0);
        for projection in [Projection::Orthographic, Projection::Perspective] {
            let mut camera = default_camera(400, 300);
            camera.set_projection(projection);
            camera.set_standard_view(StandardView::Top);
            assert!(camera.frame(AABB::new(point, point)));
            assert!(camera.world_per_pixel() * 300.0 >= 2.0 / 3.0_f64.sqrt());
            for dx in [-0.5, 0.5] {
                let screen = camera
                    .project(point + Vec3::new(dx, 0.0, 0.0))
                    .expect("a point beside it projects");
                assert!(screen.x > 0.0);
                assert!(screen.x < 400.0);
            }
        }
    }

    #[test]
    fn framing_rejects_an_empty_or_non_finite_box() {
        let mut camera = default_camera(800, 600);
        let before = camera.distance();

        assert!(!camera.frame(AABB::default())); // default constructed is empty
        assert!(!camera.frame(AABB::new(
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(f64::NAN, 1.0, 1.0)
        )));
        assert_eq!(
            camera.distance(),
            before,
            "a rejected frame must leave the view alone"
        );
    }

    #[test]
    fn elevation_is_clamped_off_the_poles_so_the_basis_never_collapses() {
        let mut camera = default_camera(800, 600);
        camera.set_orientation(0.0, 10.0); // far past straight down

        assert!(camera.elevation() < PI / 2.0);
        // The whole point: right() is a cross product with world +Z and must
        // not degenerate.
        near(camera.right().length(), 1.0, 1e-12);
        near(camera.up().length(), 1.0, 1e-12);
        near(camera.forward().length(), 1.0, 1e-12);
        near(camera.right().dot(camera.up()), 0.0, 1e-12);
        near(camera.right().dot(camera.forward()), 0.0, 1e-12);
    }

    #[test]
    fn zoom_at_a_pixel_keeps_that_world_point_under_the_cursor() {
        // dolly_at_pixel's own anchor: the point of the TARGET'S plane under
        // the pixel. That is all this pins - it passed while the 3D view's zoom
        // stalled, because the ground under the cursor is not on that plane.
        // The view moves the pivot to the ground first (set_pivot_depth),
        // pinned by AfterMovingThePivotToAPointEachDollyMagnifiesItByExactlyTheFactor.
        for projection in [Projection::Perspective, Projection::Orthographic] {
            let mut camera = default_camera(800, 600);
            camera.set_projection(projection);
            camera.set_standard_view(StandardView::IsoSouthWest);
            camera.set_depth_range(1.0, 10000.0);
            camera.set_orthographic_height(100.0);

            let px = 620.0;
            let py = 140.0;
            // The world point on the target plane that the cursor is over.
            let ray = camera.ray_through_pixel(px, py);
            let t = (camera.target() - ray.origin).dot(camera.forward())
                / ray.direction.dot(camera.forward());
            let anchor = ray.at(t);

            camera.dolly_at_pixel(0.5, px, py);

            let after = camera.project(anchor).expect("the anchor projects");
            near(after.x, px + 0.5, 1e-6);
            near(after.y, py + 0.5, 1e-6);
        }
    }

    #[test]
    fn moving_the_pivot_along_the_view_axis_moves_nothing_on_screen() {
        // set_pivot_depth puts the target `depth` in front of the eye on the
        // view axis and the distance to `depth`: by its definition the eye does
        // not move, so no point projects anywhere else. Under a perspective
        // projection the orthographic height becomes what the view shows at
        // that depth, 2 x 30 x tan(22.5 deg) = 24.852813742385702 (tan 22.5 deg
        // = sqrt 2 - 1); under an orthographic one it is the picture, and stays.
        for projection in [Projection::Perspective, Projection::Orthographic] {
            let how = format!("{}", projection as i32);
            let mut camera = default_camera(800, 600);
            camera.set_projection(projection);
            camera.set_standard_view(StandardView::IsoNorthEast);
            camera.set_target(Vec3::new(5.0, -3.0, 2.0));
            camera.set_distance(80.0);
            camera.set_orthographic_height(60.0);
            // A box reaching behind the orthographic eye gives it a standoff,
            // which the move must fold in rather than lose.
            assert!(camera.fit_depth_range(AABB::new(
                Vec3::new(-200.0, -200.0, -50.0),
                Vec3::new(200.0, 200.0, 50.0)
            )));
            let probes = [
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(12.0, 7.0, -4.0),
                Vec3::new(-9.0, 15.0, 6.0),
            ];
            let before: Vec<Vec3> = probes
                .iter()
                .map(|&probe| camera.project(probe).expect("a probe projects"))
                .collect();
            let eye = camera.eye();
            let forward = camera.forward();

            camera.set_pivot_depth(30.0);

            near_because((camera.eye() - eye).length(), 0.0, 1e-12, &how);
            near_because(
                (camera.target() - (eye + forward * 30.0)).length(),
                0.0,
                1e-12,
                &how,
            );
            double_eq(camera.distance(), 30.0);
            near_because(
                camera.orthographic_height(),
                if projection == Projection::Perspective {
                    24.852813742385702
                } else {
                    60.0
                },
                1e-12,
                &how,
            );
            for (&probe, earlier) in probes.iter().zip(&before) {
                let after = camera.project(probe).expect("a probe projects");
                near_because(after.x, earlier.x, 1e-9, &how);
                near_because(after.y, earlier.y, 1e-9, &how);
            }
        }
    }

    #[test]
    fn a_pivot_depth_that_is_not_positive_and_finite_is_ignored() {
        for depth in [0.0, -5.0, f64::NAN, f64::INFINITY] {
            let mut camera = default_camera(800, 600);
            camera.set_standard_view(StandardView::IsoSouthWest);
            let target = camera.target();
            camera.set_pivot_depth(depth);
            assert_eq!(camera.target().x, target.x, "{depth:?}");
            assert_eq!(camera.target().y, target.y, "{depth:?}");
            assert_eq!(camera.target().z, target.z, "{depth:?}");
            assert_eq!(camera.distance(), 100.0, "{depth:?}");
        }
    }

    #[test]
    fn after_moving_the_pivot_to_a_point_each_dolly_magnifies_it_by_exactly_the_factor() {
        // Ground at z = -10 under a target at the origin 100 from the eye, seen
        // from the south-west at 1200 x 800: the ground under pixel (700, 150)
        // lies beyond the target's plane. Anchored on that plane, a notch
        // magnified the ground there by less each time, tending to depth /
        // (depth - distance), and it stopped: the stall. With the pivot moved
        // to the ground's depth first, each dolly by 1 / 1.15 divides the
        // ground's depth, and so its footprint, by exactly 1.15, and the same
        // point of the ground stays at the pixel's centre.
        let mut camera = default_camera(1200, 800);
        camera.set_standard_view(StandardView::IsoSouthWest);
        let px = 700.0;
        let py = 150.0;
        let ground_under = |camera: &Camera| {
            let ray = camera.ray_through_pixel(px, py);
            ray.at((-10.0 - ray.origin.z) / ray.direction.z)
        };
        let depth_of = |camera: &Camera, p: Vec3| (p - camera.eye()).dot(camera.forward());
        let ground = ground_under(&camera);
        let first = depth_of(&camera, ground);
        assert!(
            first > 1.2 * camera.distance(),
            "not beyond the target's plane: proves nothing"
        );
        let mut depth = first;
        for notch in 1..=50 {
            let pivot = depth_of(&camera, ground_under(&camera));
            camera.set_pivot_depth(pivot);
            camera.dolly_at_pixel(1.0 / 1.15, px, py);
            let now = depth_of(&camera, ground_under(&camera));
            let how = format!("notch {notch}");
            near_because(depth / now, 1.15, 1e-9, &how);
            depth = now;
            let screen = camera.project(ground).expect("the ground point projects");
            near_because(screen.x, px + 0.5, 1e-6, &how);
            near_because(screen.y, py + 0.5, 1e-6, &how);
        }
        near(
            first / depth,
            1.15_f64.powf(50.0),
            1e-6 * 1.15_f64.powf(50.0),
        );
    }

    #[test]
    fn panning_moves_the_scene_with_the_drag() {
        let mut camera = default_camera(800, 600);
        camera.set_standard_view(StandardView::Top);
        camera.set_projection(Projection::Orthographic);
        camera.set_orthographic_height(600.0); // exactly 1 world unit per pixel
        camera.set_depth_range(1.0, 1000.0);

        let before = camera
            .project(Vec3::new(0.0, 0.0, 0.0))
            .expect("the origin projects");

        camera.pan_pixels(30.0, 20.0); // drag right and down

        let after = camera
            .project(Vec3::new(0.0, 0.0, 0.0))
            .expect("the origin projects");
        near_because(after.x - before.x, 30.0, 1e-6, "the scene follows the drag");
        near(after.y - before.y, 20.0, 1e-6);
    }

    #[test]
    fn world_per_pixel_matches_what_projection_actually_does() {
        let mut camera = default_camera(800, 600);
        // Front, not Top. Top is clamped a milliradian off vertical by the pole
        // guard, so a displacement in the ground plane is foreshortened by
        // cos(1e-3) = 0.9999995 and the identity below only holds to 5e-7.
        // Front sits at elevation 0, where nothing is clamped and the relation
        // is exact - which is what makes this a test of world_per_pixel rather
        // than of the guard.
        camera.set_standard_view(StandardView::Front);
        camera.set_projection(Projection::Orthographic);
        camera.set_orthographic_height(120.0);
        camera.set_depth_range(1.0, 1000.0);

        near(camera.world_per_pixel(), 0.2, 1e-12);

        // Screen up is world +Z in the front view.
        let a = camera
            .project(Vec3::new(0.0, 0.0, 0.0))
            .expect("a projects");
        let b = camera
            .project(Vec3::new(0.0, 0.0, 1.0))
            .expect("b projects");
        near((b.y - a.y).abs() * camera.world_per_pixel(), 1.0, 1e-12);

        // And horizontally, which shares no code path with the vertical extent.
        let c = camera
            .project(Vec3::new(1.0, 0.0, 0.0))
            .expect("c projects");
        near((c.x - a.x).abs() * camera.world_per_pixel(), 1.0, 1e-12);
    }

    #[test]
    fn the_pole_guard_costs_exactly_the_cosine_of_its_angle() {
        // Plan view is a milliradian off vertical on purpose (see camera.rs).
        // That is invisible on screen but it is not nothing, and pinning the
        // size of it here means a later change to the guard shows up as a
        // failure rather than as a drawing that is quietly 0.00005% out of
        // scale.
        let mut camera = default_camera(800, 600);
        camera.set_standard_view(StandardView::Top);
        camera.set_projection(Projection::Orthographic);
        camera.set_orthographic_height(120.0);
        camera.set_depth_range(1.0, 1000.0);

        let a = camera
            .project(Vec3::new(0.0, 0.0, 0.0))
            .expect("a projects");
        let b = camera
            .project(Vec3::new(0.0, 1.0, 0.0))
            .expect("b projects");
        let measured = (b.y - a.y).abs() * camera.world_per_pixel();
        near(measured, 1.0e-3_f64.cos(), 1e-12);
        near_because(
            measured,
            1.0,
            1e-6,
            "still far below one pixel at any sane scale",
        );
    }
}
