//! The twins of tests/gpu/test_gpu_scene_view.cpp: the GPU view (gpu_scene_view) - what it tells
//! its host when it cannot draw, and that its mouse moves the host's camera exactly as the render
//! view's does.
//!
//! The C++ ran its "OnTheDesktop" cases only on a platform its QRhiWidget could be shown on, and
//! its other cases under the offscreen one, where the widget cannot draw. A Rust GPU view draws
//! into a target on the device the window gives it, with no window, so the desktop cases are
//! device cases (the foundation's rule): each runs on the Hardware device and then the Software
//! one, skipping a device that is not there, and the offscreen case is a view given no device -
//! what a headless run gives it.

mod gpu_scene_view {
    use katana_gpu::{GpuDevice, RendererDecision, RendererEnvironment};
    use katana_math::{AABB, Vec3};
    use katana_render::{
        Camera, DrawList, Framebuffer, Rasterizer, RenderOptions, StandardView, rgba,
    };
    use katana_window::canvas::{Image, PointF};
    use katana_window::gpu_scene_view::{
        GpuSceneEvent, GpuSceneHost, GpuSceneView, make_gpu_scene_view_if_chosen,
    };
    use katana_window::viewport_widget::{KeyModifiers, MouseButton};

    use crate::gpu_support::{DEVICES, device_label, shared_context};
    use crate::support::{drawn_pixels, save_for_looking};

    /// A sloping triangle 40 m across and 8 m high about `offset`.
    fn triangle_at(offset: Vec3) -> DrawList {
        let mut list = DrawList::default();
        let a = list.add_vertex(offset + Vec3::new(-20.0, -20.0, 0.0), rgba(220, 60, 60));
        let b = list.add_vertex(offset + Vec3::new(20.0, -20.0, 0.0), rgba(60, 220, 60));
        let c = list.add_vertex(offset + Vec3::new(0.0, 20.0, 8.0), rgba(60, 60, 220));
        list.add_triangle(a, b, c);
        list
    }

    /// The device of `kind`, or None after saying why it is not here (GTEST_SKIP's line).
    fn device_or_skip(kind: GpuDevice) -> Option<katana_gpu::GpuContext> {
        match shared_context(kind) {
            Ok(context) => Some(context.clone()),
            Err(error) => {
                eprintln!("[  SKIPPED ] {}: {}", device_label(kind), error.describe());
                None
            }
        }
    }

    /// A view on `device`, its software device allowed: the view is under test, not the device
    /// policy.
    fn view_on(device: &katana_gpu::GpuContext) -> GpuSceneView {
        let mut view = GpuSceneView::new();
        view.set_device(Some(device.clone()));
        view.set_software_device_allowed(true);
        view
    }

    fn failures(view: &mut GpuSceneView) -> Vec<String> {
        view.take_events()
            .into_iter()
            .filter_map(|event| match event {
                GpuSceneEvent::RenderFailed(why) => Some(why),
                _ => None,
            })
            .collect()
    }

    /// The pixels of a grabbed frame that are not the view's background (28, 30, 36).
    fn drawn(grabbed: &Image) -> usize {
        drawn_pixels(grabbed, rgba(28, 30, 36))
    }

    #[test]
    fn under_the_offscreen_platform_reports_that_it_cannot_render_so_the_host_falls_back() {
        // A headless run gives the view no device: it cannot draw, and says so once.
        let mut camera = Camera::default();
        let mut view = GpuSceneView::new();
        view.resize(&mut camera, 160, 100);
        let grabbed = view.grab_framebuffer(&mut camera);
        assert!(grabbed.is_none_or(|image| image.is_null() || image.width() == 0));
        assert!(view.failed());
        let said = failures(&mut view);
        assert_eq!(said.len(), 1);
        assert!(!said[0].is_empty());
    }

    #[test]
    fn the_factory_builds_no_gpu_view_where_the_rules_choose_software() {
        let mut environment = RendererEnvironment {
            platform_name: String::from("offscreen"),
            ..RendererEnvironment::default()
        };
        let mut decision = RendererDecision::default();
        let view = make_gpu_scene_view_if_chosen(&environment, Some(&mut decision));
        assert!(view.is_none());
        assert!(decision.reason.contains("offscreen"), "{}", decision.reason);

        environment.platform_name = String::from("windows");
        let view = make_gpu_scene_view_if_chosen(&environment, Some(&mut decision));
        assert!(view.is_some());
    }

    #[test]
    fn dragging_orbits_pans_and_the_wheel_zooms_the_hosts_camera_as_the_software_view_does() {
        let mut camera = Camera::default();
        camera.set_viewport_size(200, 100);
        camera.set_orientation(0.0, 0.5);
        camera.set_distance(100.0);
        let mut view = GpuSceneView::new();
        view.resize(&mut camera, 200, 100);

        // Left drag 50 px right and 25 px down: azimuth -50 * 0.008 = -0.4 rad, elevation
        // +25 * 0.008 = +0.2 rad (the render view's ORBIT_PER_PIXEL).
        view.mouse_press(PointF::new(100.0, 50.0), MouseButton::Left, KeyModifiers::NONE);
        view.mouse_move(&mut camera, PointF::new(150.0, 75.0));
        view.mouse_release();
        assert!((camera.azimuth() - -0.4).abs() <= 1e-12, "{}", camera.azimuth());
        assert!((camera.elevation() - 0.7).abs() <= 1e-12, "{}", camera.elevation());

        // An elevation view does not orbit: the same drag pans.
        view.set_orbit_allowed(false);
        let before = camera.target();
        view.mouse_press(PointF::new(100.0, 50.0), MouseButton::Left, KeyModifiers::NONE);
        view.mouse_move(&mut camera, PointF::new(110.0, 50.0));
        view.mouse_release();
        assert!((camera.azimuth() - -0.4).abs() <= 1e-12);
        assert!((camera.target() - before).length() > 0.0);

        // One wheel notch in: the distance divides by 1.15.
        let distance = camera.distance();
        assert!(view.wheel(&mut camera, PointF::new(100.0, 50.0), 120.0));
        assert!((camera.distance() - distance / 1.15).abs() <= 1e-9);
    }

    /// A host that takes the wheel and keeps what it was handed.
    struct WheelHost {
        camera: Camera,
        notches: f64,
        at: PointF,
        calls: i32,
    }

    impl GpuSceneHost for WheelHost {
        fn camera(&mut self) -> &mut Camera {
            &mut self.camera
        }

        fn wheel_zoom(&mut self, _view: &mut GpuSceneView, notches: f64, at: PointF) -> bool {
            self.notches = notches;
            self.at = at;
            self.calls += 1;
            true
        }
    }

    #[test]
    fn the_wheel_is_handed_to_the_host_that_zooms_towards_what_is_under_the_cursor() {
        // The view holds only packed lists, so the host - which has the scene - does the zoom:
        // the wheel reaches its hook with its notches (a fine wheel's fraction too) and the cursor
        // in logical pixels, and the view leaves the camera alone. Needs no device: no frame is
        // drawn.
        let mut camera = Camera::default();
        camera.set_viewport_size(200, 100);
        camera.set_distance(100.0);
        let mut host = WheelHost {
            camera,
            notches: 0.0,
            at: PointF::default(),
            calls: 0,
        };
        let mut view = GpuSceneView::new();
        view.resize(&mut host, 200, 100);
        let target = host.camera.target();
        let accepted = view.wheel(&mut host, PointF::new(30.0, 70.0), 180.0);
        assert_eq!(host.calls, 1);
        assert_eq!(host.notches, 1.5);
        assert_eq!(host.at, PointF::new(30.0, 70.0));
        assert_eq!(host.camera.distance(), 100.0);
        assert_eq!(host.camera.target(), target);
        assert!(accepted);
    }

    #[test]
    fn framing_an_empty_list_leaves_the_view_to_frame_the_first_list_that_arrives() {
        // Framing an empty draw list frames nothing and leaves the view unframed, so the first
        // frame after a list arrives frames that list. It used to count as framed before it found
        // the list empty, and a view first drawn before its scene was built stayed on the default
        // camera until the user pressed E.
        let mut camera = Camera::default();
        let mut view = GpuSceneView::new();
        view.resize(&mut camera, 200, 100);
        let before = camera.target();
        view.zoom_extents(&mut camera);
        assert!(!view.camera_framed());
        assert_eq!(camera.target(), before);

        // At MGA coordinates: framed means the target moved there.
        view.set_draw_list(&triangle_at(Vec3::new(300000.0, 6250000.0, 50.0)));
        view.zoom_extents(&mut camera);
        assert!(view.camera_framed());
        assert!((camera.target().x - 300000.0).abs() <= 20.0);
        assert!((camera.target().y - 6250000.0).abs() <= 20.0);
    }

    /// A host whose framing hook counts its calls and frames as `frames` says.
    struct FramingHost {
        camera: Camera,
        calls: i32,
        /// The box the hook frames, if it frames one.
        frames: Option<AABB>,
    }

    impl GpuSceneHost for FramingHost {
        fn camera(&mut self) -> &mut Camera {
            &mut self.camera
        }

        fn zoom_extents(&mut self, _view: &mut GpuSceneView) -> bool {
            self.calls += 1;
            if let Some(bounds) = self.frames {
                let _ = self.camera.frame(bounds);
            }
            true
        }
    }

    #[test]
    fn the_host_says_whether_its_camera_is_framed_and_a_hook_framing_nothing_is_redone() {
        // The host's word that its camera is framed stands; and when the host frames through its
        // hook while the list is still empty, the first list with something in it is framed
        // again, as the render view does for its own.
        let mut view = GpuSceneView::new();
        assert!(!view.camera_framed());
        view.set_camera_framed(true);
        assert!(view.camera_framed());
        view.set_camera_framed(false);
        assert!(!view.camera_framed());

        let mut host = FramingHost {
            camera: Camera::default(),
            calls: 0,
            frames: None,
        };
        view.zoom_extents(&mut host); // the list is empty
        assert_eq!(host.calls, 1);
        assert!(!view.camera_framed(), "a frame of nothing is not worth keeping");
        view.set_draw_list(&triangle_at(Vec3::new(0.0, 0.0, 0.0)));
        assert!(
            !view.camera_framed(),
            "the list with something in it is still to be framed"
        );
        view.zoom_extents(&mut host);
        assert_eq!(host.calls, 2);
        assert!(view.camera_framed());
    }

    #[test]
    fn resizing_keeps_the_hosts_camera_in_the_widgets_logical_pixels() {
        // The host's camera is kept in the view's logical pixels, where the software view keeps
        // it: resizing the view sizes the camera so.
        let mut camera = Camera::default();
        let mut view = GpuSceneView::new();
        view.resize(&mut camera, 300, 150);
        assert_eq!(camera.viewport_width(), 300);
        assert_eq!(camera.viewport_height(), 150);
    }

    #[test]
    fn on_the_desktop_draws_the_scene_through_the_hosts_camera() {
        // The view itself draws a scene through its host's camera, which the gpu suite can only
        // prove of the renderer.
        for kind in DEVICES {
            let Some(device) = device_or_skip(kind) else {
                continue;
            };
            let label = device_label(kind);
            let mut list = DrawList::default();
            let a = list.add_vertex(Vec3::new(-20.0, -20.0, 0.0), rgba(220, 60, 60));
            let b = list.add_vertex(Vec3::new(20.0, -20.0, 0.0), rgba(60, 220, 60));
            let c = list.add_vertex(Vec3::new(0.0, 20.0, 8.0), rgba(60, 60, 220));
            list.add_triangle(a, b, c);
            list.add_segment(
                Vec3::new(-20.0, -20.0, 0.0),
                Vec3::new(0.0, 20.0, 8.0),
                rgba(255, 255, 255),
                2.0,
                0.0,
            );

            let mut camera = Camera::default();
            camera.set_standard_view(StandardView::IsoSouthWest);
            let mut view = view_on(&device);
            view.resize(&mut camera, 320, 200);
            view.set_draw_list(&list);
            let grabbed = view.grab_framebuffer(&mut camera);
            assert!(!view.failed(), "{label}: {:?}", failures(&mut view));
            let grabbed = grabbed.expect("a frame");
            assert!(!grabbed.is_null(), "{label}");
            // The host's camera stays in the view's logical pixels; the frame was drawn in its
            // device pixels.
            assert_eq!(camera.viewport_width(), view.width(), "{label}");
            assert_eq!(camera.viewport_height(), view.height(), "{label}");
            assert_eq!(
                grabbed.width() as i32,
                (f64::from(view.width()) * view.device_ratio()).round() as i32,
                "{label}"
            );
            assert_eq!(view.last_stats().triangles, 1, "{label}");
            assert_eq!(view.last_stats().lines, 1, "{label}");
            // Framed on its first frame, so the triangle is on screen.
            let pixels = (grabbed.width() * grabbed.height()) as usize;
            assert!(drawn(&grabbed) > pixels / 20, "{label}");
            save_for_looking(
                "KATANA_GPU_TEST_IMAGES",
                &format!("scene_view_desktop_{label}"),
                &grabbed,
            );
        }
    }

    #[test]
    fn on_the_desktop_a_host_camera_already_framed_keeps_its_view_through_the_first_frame() {
        // A GPU view made over a ViewState whose camera the user has already orbited and zoomed -
        // a renderer switch, a re-tile, a re-dock - keeps that view through its first frame when
        // the host says the camera is framed. It used to frame the scene on every first frame,
        // through the host's own zoom_extents.
        for kind in DEVICES {
            let Some(device) = device_or_skip(kind) else {
                continue;
            };
            let label = device_label(kind);
            let mut camera = Camera::default();
            camera.set_target(Vec3::new(-15.0, -15.0, 0.0));
            camera.set_distance(12.0);
            camera.set_orientation(0.7, 0.5);
            let kept = camera;
            // The host's zoom_extents, as the hosting recipe wires it: frames the triangle's box,
            // whose centre is (0, 0, 4).
            let triangle_box = AABB::new(Vec3::new(-20.0, -20.0, 0.0), Vec3::new(20.0, 20.0, 8.0));

            let mut host = FramingHost {
                camera,
                calls: 0,
                frames: Some(triangle_box),
            };
            let mut view = view_on(&device);
            view.resize(&mut host, 320, 200);
            view.set_camera_framed(true);
            view.set_draw_list(&triangle_at(Vec3::new(0.0, 0.0, 0.0)));
            assert!(view.grab_framebuffer(&mut host).is_some(), "{label}");
            assert!(!view.failed(), "{label}");
            assert_eq!(host.calls, 0, "{label}");
            assert_eq!(host.camera.target(), kept.target(), "{label}");
            assert_eq!(host.camera.distance(), kept.distance(), "{label}");
            assert_eq!(host.camera.azimuth(), kept.azimuth(), "{label}");
            assert_eq!(host.camera.elevation(), kept.elevation(), "{label}");
            assert!(view.camera_framed(), "{label}");

            // The control: the same camera, not said to be framed, is framed by the host's hook -
            // its target moves from (-15, -15, 0) to (0, 0, 4).
            let mut fresh = FramingHost {
                camera: kept,
                calls: 0,
                frames: Some(triangle_box),
            };
            let mut other = view_on(&device);
            other.resize(&mut fresh, 320, 200);
            other.set_draw_list(&triangle_at(Vec3::new(0.0, 0.0, 0.0)));
            assert!(other.grab_framebuffer(&mut fresh).is_some(), "{label}");
            assert!(
                (fresh.camera.target() - Vec3::new(0.0, 0.0, 4.0)).length() <= 1e-9,
                "{label}"
            );
        }
    }

    #[test]
    fn on_the_desktop_a_draw_list_arriving_after_the_first_frame_is_framed() {
        // A view drawn before its scene is built frames the scene when it arrives. It used to take
        // its first, empty, frame for its framing, and showed a site at MGA coordinates nowhere
        // near the default camera: a blank view. (The C++ showed its widget for this one, as a
        // QRhiWidget never shown read back nothing at its second grab; a Rust view draws into a
        // target of its own either way.)
        for kind in DEVICES {
            let Some(device) = device_or_skip(kind) else {
                continue;
            };
            let label = device_label(kind);
            let mut camera = Camera::default();
            let mut view = view_on(&device);
            view.resize(&mut camera, 320, 200);
            assert!(view.grab_framebuffer(&mut camera).is_some(), "{label}"); // nothing yet
            assert!(!view.failed(), "{label}");
            assert_eq!(view.last_stats().triangles, 0, "{label}");
            assert!(!view.camera_framed(), "{label}");

            let site = Vec3::new(300000.0, 6250000.0, 50.0);
            view.set_draw_list(&triangle_at(site));
            let grabbed = view.grab_framebuffer(&mut camera).expect("a frame");
            save_for_looking(
                "KATANA_GPU_TEST_IMAGES",
                &format!("scene_view_list_after_first_frame_{label}"),
                &grabbed,
            );
            assert_eq!(view.last_stats().triangles, 1, "{label}");
            assert!(view.camera_framed(), "{label}");
            assert!((camera.target().x - site.x).abs() <= 20.0, "{label}");
            assert!((camera.target().y - site.y).abs() <= 20.0, "{label}");
            // Framed, the triangle is a good part of the frame; unframed, the camera looks at the
            // origin, 6 250 km from it, and nothing is drawn. (An image read back as nothing, all
            // zeros, would count every pixel as drawn: the background's own count rules that
            // out.)
            let pixels = (grabbed.width() * grabbed.height()) as usize;
            let drawn = drawn(&grabbed);
            assert!(drawn > pixels / 20, "{label}");
            assert!(
                pixels - drawn > pixels / 4,
                "{label}: no background: the frame did not come back"
            );
        }
    }

    #[test]
    fn on_the_desktop_a_frame_leaves_the_hosts_camera_ready_for_the_software_view() {
        // The software view taking over after a failure paints with the same camera, through a
        // rasteriser that refuses a camera whose viewport is not its framebuffer's size - the
        // view's LOGICAL size. The GPU frame, drawn in device pixels, must leave the camera there.
        for kind in DEVICES {
            let Some(device) = device_or_skip(kind) else {
                continue;
            };
            let label = device_label(kind);
            let list = triangle_at(Vec3::new(0.0, 0.0, 0.0));
            let mut camera = Camera::default();
            let mut view = view_on(&device);
            view.resize(&mut camera, 320, 200);
            view.set_draw_list(&list);
            let grabbed = view.grab_framebuffer(&mut camera);
            assert!(grabbed.is_some(), "{label}");
            assert!(!view.failed(), "{label}");

            let mut framebuffer =
                Framebuffer::create(view.width(), view.height()).expect("a framebuffer");
            let mut rasterizer = Rasterizer::default();
            let painted =
                rasterizer.render(&list, &camera, &mut framebuffer, &RenderOptions::default());
            assert!(
                painted.is_ok(),
                "{label}: {}",
                painted.err().map(|error| error.describe()).unwrap_or_default()
            );
        }
    }

    #[test]
    fn on_the_desktop_a_software_device_is_refused_unless_allowed() {
        // A software device - lavapipe, WARP - runs the GPU's work on the CPU, which the
        // rasteriser was written to do better; so the view refuses one as a failure, the host
        // falls back, and only KATANA_RENDERER=gpu (set_software_device_allowed) lets it draw.
        // About the software device alone: the hardware one is never refused.
        let Some(device) = device_or_skip(GpuDevice::Software) else {
            return;
        };
        let mut camera = Camera::default();
        let mut refusing = GpuSceneView::new();
        refusing.set_device(Some(device.clone()));
        refusing.resize(&mut camera, 64, 48);
        refusing.set_draw_list(&triangle_at(Vec3::new(0.0, 0.0, 0.0)));
        let _ = refusing.grab_framebuffer(&mut camera);
        assert!(refusing.failed());
        let reason = failures(&mut refusing).join("; ");
        assert!(reason.contains("software"), "{reason}");

        let mut other = Camera::default();
        let mut allowed = view_on(&device);
        allowed.resize(&mut other, 64, 48);
        allowed.set_draw_list(&triangle_at(Vec3::new(0.0, 0.0, 0.0)));
        let grabbed = allowed.grab_framebuffer(&mut other);
        assert!(!allowed.failed(), "{:?}", failures(&mut allowed));
        assert!(grabbed.is_some_and(|image| drawn(&image) > 0));
    }
}
