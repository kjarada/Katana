//! The twins of tests/qt_widgets/test_render_view_gpu.cpp: the 3D view drawn on the GPU
//! (render_view_widget, "TWO RENDERERS"; docs/gpu.md, "Hosting").
//!
//! The C++ ran these only where its GPU view could be shown - a desktop platform, under Xvfb on
//! Linux - with KATANA_RENDERER=gpu so that lavapipe could draw them. Here each is a device case
//! (the foundation's rule): its view's session is the desktop platform the build's backend is
//! shown on, with the shared device of each kind in turn and KATANA_RENDERER=gpu as the session's
//! own setting (GpuSession::renderer_override), Hardware then Software, a device that is not there
//! skipped. Each view has a session of its own, where the C++ ran each case in a process of its
//! own: a GPU view that failed is not tried again for the rest of its session.

mod render_view_gpu {
    use katana_cad::viewport_layout::ViewKind;
    use katana_commands::{EntityAttributes, create_point};
    use katana_geometry::Point2;
    use katana_gpu::GpuDevice;
    use katana_math::Vec3;
    use katana_render::rgba;
    use katana_window::canvas::PointF;
    use katana_window::render_view_widget::GpuSession;

    use crate::gpu_support::{DEVICES, device_label, shared_context};
    use crate::support::{
        Bench, double_eq, drawn_near, drawn_pixels, save_for_looking, statuses,
    };

    /// The bench's session: the desktop, on the shared device of `kind`, a software device
    /// allowed; None after saying why there is no such device.
    fn on_device(bench: &mut Bench, kind: GpuDevice) -> bool {
        match shared_context(kind) {
            Ok(context) => {
                let mut session = GpuSession::desktop(Some(context.clone()));
                session.renderer_override = Some(String::from("gpu"));
                bench.session = session;
                true
            }
            Err(error) => {
                eprintln!("[  SKIPPED ] {}: {}", device_label(kind), error.describe());
                false
            }
        }
    }

    const BACKGROUND: katana_render::Rgba = rgba(28, 30, 36);

    #[test]
    fn on_the_desktop_the_hosts_layers_are_drawn_by_its_gpu_child() {
        // The host builds its layers as for the software view and its GPU child draws them, framed
        // by the host: one line from (0, 0) to (100, 50) at the entity elevation 0, whose box's
        // centre, (50, 25, 0), Camera::frame makes the target (as test_render_view.cpp works out
        // for the software view).
        for kind in DEVICES {
            let mut scene = Bench::one_line();
            if !on_device(&mut scene, kind) {
                continue;
            }
            let label = device_label(kind);
            let id = scene.add(ViewKind::Model3D);
            let mut view = scene.make(id);
            scene.resize(&mut view, id, 320, 200);
            assert!(view.drawn_on_gpu(), "{label}: {}", view.renderer_reason());

            let frame = scene.with(id, |scene, state| view.grab_gpu_frame(scene, state));
            let gpu = view.gpu_view().expect("the GPU child");
            assert!(!gpu.failed(), "{label}");
            let frame = frame.expect("a frame");
            assert!(!frame.is_null(), "{label}");
            assert!(
                drawn_pixels(&frame, BACKGROUND) > 100,
                "{label}: the line and the grid under it"
            );
            save_for_looking("KATANA_GPU_TEST_IMAGES", &format!("render_view_gpu_{label}"), &frame);
            // Five layers, as render_layers draws them; the line is the drawing's.
            assert!(gpu.last_stats().lines >= 1, "{label}");
            let state = scene.state(id);
            assert!(double_eq(state.camera.target().x, 50.0), "{label}");
            assert!(double_eq(state.camera.target().y, 25.0), "{label}");
            assert!(double_eq(state.camera.target().z, 0.0), "{label}");
            assert!(state.camera_framed, "{label}");
            // The GPU child keeps the camera in its logical pixels.
            assert_eq!(state.camera.viewport_width(), gpu.width(), "{label}");
        }
    }

    #[test]
    fn on_the_desktop_an_edit_reaches_the_gpu_view() {
        // An edit reaches the GPU view: the host rebuilds the drawing's layers and hands them over
        // at the next frame, the terrain's untouched.
        for kind in DEVICES {
            let mut scene = Bench::one_line();
            if !on_device(&mut scene, kind) {
                continue;
            }
            let label = device_label(kind);
            let id = scene.add(ViewKind::Model3D);
            let mut view = scene.make(id);
            scene.resize(&mut view, id, 320, 200);
            assert!(view.drawn_on_gpu(), "{label}: {}", view.renderer_reason());
            let _ = scene.with(id, |scene, state| view.grab_gpu_frame(scene, state));
            let before = view.gpu_view().expect("the GPU child").last_stats().lines;
            let builds = view.entity_builds();

            scene.line(Point2::new(0.0, 50.0), Point2::new(100.0, 0.0));
            let _ = scene.with(id, |scene, state| view.grab_gpu_frame(scene, state));
            assert!(view.entity_builds() > builds, "{label}");
            assert!(
                view.gpu_view().expect("the GPU child").last_stats().lines > before,
                "{label}"
            );
        }
    }

    #[test]
    fn on_the_desktop_the_wheel_over_the_gpu_view_zooms_towards_what_is_under_the_cursor() {
        // The wheel over the GPU child zooms through the host, towards what the host built under
        // the cursor: the cursor over the line's point (75, 37.5, 0), three quarters along it,
        // which lies 21.7 m beyond the plane of the framed target (50, 25, 0) - (25, 12.5, 0) .
        // (1, 1, -1) / sqrt 3 - so that the child's own zoom, anchored on that plane, magnified it
        // by 1.124 a notch. Through the host each notch divides its depth by 1.15 (docs/render.md,
        // "Zooming towards the cursor"), and the frame draws the line under the cursor. The GPU
        // child's camera counts in its logical pixels, which are the wheel's.
        for kind in DEVICES {
            let mut scene = Bench::one_line();
            if !on_device(&mut scene, kind) {
                continue;
            }
            let label = device_label(kind);
            let id = scene.add(ViewKind::Model3D);
            let mut view = scene.make(id);
            scene.resize(&mut view, id, 320, 200);
            assert!(view.drawn_on_gpu(), "{label}: {}", view.renderer_reason());
            let _ = scene.with(id, |scene, state| view.grab_gpu_frame(scene, state));
            let point = Vec3::new(75.0, 37.5, 0.0);
            let depth_of = |scene: &Bench| {
                let camera = scene.camera(id);
                (point - camera.eye()).dot(camera.forward())
            };
            assert!(
                (depth_of(&scene) - scene.camera(id).distance() - 21.650635094610966).abs()
                    <= 1e-9,
                "{label}: the point is on the target's plane, where the child's own zoom was right"
            );
            let screen = scene.camera(id).project(point).expect("it projects");
            let cursor = PointF::new(screen.x - 0.5, screen.y - 0.5);
            let mut depth = depth_of(&scene);
            for notch in 1..=30 {
                assert!(scene.turn_wheel(&mut view, id, cursor, 120), "{label}");
                let now = depth_of(&scene);
                assert!(
                    (depth / now - 1.15).abs() <= 1e-9,
                    "{label}: notch {notch}"
                );
                depth = now;
            }
            let frame = scene
                .with(id, |scene, state| view.grab_gpu_frame(scene, state))
                .expect("a frame");
            let gpu = view.gpu_view().expect("the GPU child");
            assert!(!gpu.failed(), "{label}");
            save_for_looking(
                "KATANA_GPU_TEST_IMAGES",
                &format!("render_view_gpu_zoomed_{label}"),
                &frame,
            );
            let ratio = gpu.device_ratio();
            assert!(
                drawn_near(
                    &frame,
                    PointF::new(cursor.x * ratio, cursor.y * ratio),
                    3,
                    BACKGROUND
                ),
                "{label}: the line is not drawn under the cursor"
            );
        }
    }

    #[test]
    fn on_the_desktop_a_deep_zoom_far_from_the_scenes_centre_packs_the_layers_against_the_pivot() {
        // A survey at MGA coordinates with one stray point at the origin: the centre of the
        // scene's box, what the GPU packs its layers against at first, is 3100 km from the survey,
        // where a float steps in 0.25 m. Looked at from 50 m and zoomed 30 notches in towards the
        // line under the cursor, to 0.75 m where a pixel is 3 mm, the line drawn from that origin
        // lands far off the cursor - without the repack, a stub at the top edge. The host packs
        // the layers again against the pivot once the error there passes ORIGIN_ERROR_PIXELS
        // (scene_origin, "The origin follows a deep zoom"), and the line is drawn under the
        // cursor.
        for kind in DEVICES {
            let mut scene = Bench::new();
            if !on_device(&mut scene, kind) {
                continue;
            }
            let label = device_label(kind);
            scene.line(
                Point2::new(300000.0, 6200000.0),
                Point2::new(300100.0, 6200050.0),
            );
            scene
                .document
                .execute(create_point(Point2::new(0.0, 0.0), EntityAttributes::default()))
                .expect("the point is made");
            let id = scene.add(ViewKind::Model3D);
            let mut view = scene.make(id);
            scene.resize(&mut view, id, 320, 200);
            assert!(view.drawn_on_gpu(), "{label}: {}", view.renderer_reason());
            let _ = scene.with(id, |scene, state| view.grab_gpu_frame(scene, state));
            let point = Vec3::new(300075.0, 6200037.5, 0.0); // on the line
            assert!(
                (view.gpu_view().expect("the GPU child").scene_origin() - point).length() > 1.0e6,
                "{label}: the GPU packed the scene near the line already, so this proves nothing"
            );

            {
                let camera = &mut scene.state_mut(id).camera;
                camera.set_target(point);
                camera.set_distance(50.0);
            }
            if let Some(gpu) = view.gpu_view_mut() {
                gpu.update();
            }
            let _ = scene.with(id, |scene, state| view.grab_gpu_frame(scene, state));
            let screen = scene.camera(id).project(point).expect("it projects");
            let cursor = PointF::new(screen.x - 0.5, screen.y - 0.5);
            for _ in 1..=30 {
                scene.turn_wheel(&mut view, id, cursor, 120);
                let _ = scene.with(id, |scene, state| view.grab_gpu_frame(scene, state));
            }
            assert!(
                (scene.camera(id).distance() - 50.0 / 1.15_f64.powf(30.0)).abs() <= 1e-6,
                "{label}"
            );
            let frame = scene
                .with(id, |scene, state| view.grab_gpu_frame(scene, state))
                .expect("a frame");
            let gpu = view.gpu_view().expect("the GPU child");
            assert!(!gpu.failed(), "{label}");
            save_for_looking(
                "KATANA_GPU_TEST_IMAGES",
                &format!("render_view_gpu_deep_zoom_{label}"),
                &frame,
            );
            // The origin followed the zoom to within the view's reach of the pivot.
            assert!(
                (gpu.scene_origin() - scene.camera(id).target()).length() < 50.0,
                "{label}"
            );
            let ratio = gpu.device_ratio();
            assert!(
                drawn_near(
                    &frame,
                    PointF::new(cursor.x * ratio, cursor.y * ratio),
                    3,
                    BACKGROUND
                ),
                "{label}: the line is not drawn under the cursor"
            );
        }
    }

    #[test]
    fn on_the_desktop_a_failed_gpu_view_leaves_the_session_to_the_software_rasteriser() {
        // A GPU view that fails hands the view to the software rasteriser, and no view of the
        // session tries the GPU again (renderer_choice, rule 4).
        for kind in DEVICES {
            let mut scene = Bench::one_line();
            if !on_device(&mut scene, kind) {
                continue;
            }
            let label = device_label(kind);
            let id = scene.add(ViewKind::Model3D);
            let mut view = scene.make(id);
            scene.resize(&mut view, id, 320, 200);
            assert!(view.drawn_on_gpu(), "{label}: {}", view.renderer_reason());

            // What the GPU view raises when its device is lost or a pipeline fails.
            view.gpu_view_mut()
                .expect("the GPU child")
                .fail("a device lost in a test");
            view.deliver();
            assert!(!view.drawn_on_gpu(), "{label}");
            let status = statuses(&view.take_events()).join("\n");
            assert!(status.contains("a device lost in a test"), "{label}: {status}");
            // The software rasteriser draws it now, at the display's device pixels.
            scene.paint(&mut view, id);
            assert!(!view.framebuffer().empty(), "{label}");
            save_for_looking(
                "KATANA_GPU_TEST_IMAGES",
                &format!("render_view_software_after_the_gpu_failed_{label}"),
                view.picture(),
            );
            assert!(view.last_stats().lines_submitted > 0, "{label}");

            let other = scene.add(ViewKind::Model3D);
            let later = scene.make(other);
            assert!(!later.drawn_on_gpu(), "{label}");
            assert!(
                later.renderer_reason().contains("failed earlier"),
                "{label}: {}",
                later.renderer_reason()
            );
        }
    }
}
