//! The twins of tests/qt_widgets/test_render_view.cpp: the 3D and elevation view
//! (render_view_widget), drawn by the software rasteriser as every headless run draws it.
//!
//! The workspace builds a new view over the same ViewState whenever a view changes kind, so what a
//! view keeps across a change is what its state keeps. Expected values are worked out by hand from
//! the scene: one line from (0, 0) to (100, 50) drawn at the entity elevation 0 has the box
//! (0, 0, 0)-(100, 50, 0), and Camera::frame puts the target at its centre, (50, 25, 0).
//!
//! Not twinned: RenderView.ANewDrawingLeavesNoSurfaceOfThePreviousOneInTheThreeDView, which opens
//! its view through the workspace (view_workspace, wave 3) and asks it to forget the drawing it
//! replaced.

mod render_view {
    use katana_cad::scene::{SceneOptions, scene_depth_box};
    use katana_cad::scene_zoom::{PickOptions, pick_drawn_point};
    use katana_cad::view_set::{ViewId, ViewState};
    use katana_cad::viewport_layout::ViewKind;
    use katana_commands::update_layer;
    use katana_entity::Layer;
    use katana_geometry::Point2;
    use katana_math::Vec3;
    use katana_render::{Camera, Rgba, rgba};
    use katana_window::canvas::PointF;
    use katana_window::render_view_widget::{RenderView, ZOOM_LIMIT_MESSAGE};

    use crate::support::{
        Bench, double_eq, flat_ground, frame_stats, save_for_looking, statuses,
    };

    #[track_caller]
    fn expect_target(state: &ViewState, x: f64, y: f64, z: f64) {
        let target = state.camera.target();
        assert!(double_eq(target.x, x), "target x {} is not {x}", target.x);
        assert!(double_eq(target.y, y), "target y {} is not {y}", target.y);
        assert!(double_eq(target.z, z), "target z {} is not {z}", target.z);
    }

    #[test]
    fn a_three_d_view_turned_into_a_section_and_back_keeps_its_target_and_distance() {
        // ViewState::camera_kind keeps the orbit's ANGLE through a section (cad's ViewSet test),
        // but the rebuilt view framed the scene at its first paint, which moved the target back
        // to the middle of the drawing and the distance back to "all of it" - a zoom onto one
        // culvert was lost.
        let mut scene = Bench::one_line();
        let id = scene.add(ViewKind::Model3D);
        {
            let _first = scene.show(id);
            expect_target(scene.state(id), 50.0, 25.0, 0.0); // the first paint framed the line
            // The user zooms in towards a point off the middle and pans.
            let camera = &mut scene.state_mut(id).camera;
            camera.dolly_at_pixel(0.25, 100.0, 80.0);
            camera.pan_pixels(40.0, -25.0);
        }
        let zoomed: Camera = scene.camera(id);
        assert_ne!(
            zoomed.target().x,
            50.0,
            "the zoom moved the target, or this proves nothing"
        );

        scene.views.set_kind(id, ViewKind::Section).expect("to a section");
        scene.views.set_kind(id, ViewKind::Model3D).expect("and back");
        let _second = scene.show(id);

        let camera = scene.camera(id);
        assert_eq!(camera.target().x, zoomed.target().x);
        assert_eq!(camera.target().y, zoomed.target().y);
        assert_eq!(camera.target().z, zoomed.target().z);
        assert_eq!(camera.distance(), zoomed.distance());
        assert_eq!(camera.orthographic_height(), zoomed.orthographic_height());
        assert_eq!(camera.azimuth(), zoomed.azimuth());
        assert_eq!(camera.elevation(), zoomed.elevation());
    }

    #[test]
    fn a_view_whose_camera_was_pointed_afresh_frames_the_drawing_again() {
        // 3D -> Elevation points the camera at the front (ViewSet::set_kind), so the elevation's
        // first paint must frame the drawing: a target kept from the 3D zoom would leave the
        // elevation looking at a corner of it.
        let mut scene = Bench::one_line();
        let id = scene.add(ViewKind::Model3D);
        {
            let _first = scene.show(id);
            let camera = &mut scene.state_mut(id).camera;
            camera.dolly_at_pixel(0.25, 100.0, 80.0);
            camera.pan_pixels(40.0, -25.0);
        }
        scene.views.set_kind(id, ViewKind::Elevation).expect("to an elevation");
        let _elevation = scene.show(id);
        expect_target(scene.state(id), 50.0, 25.0, 0.0);
    }

    #[test]
    fn an_edit_reaches_a_three_d_view_already_open() {
        // Audit QT-05: the 3D view was rebuilt only when something else happened to invalidate
        // it, so a line drawn in plan never appeared in it and an undone one stayed. A second line
        // inside the first one's box leaves the grid as it was, so the open view must now build
        // what a view opened afresh on the same drawing builds, and after an undo what it built
        // before.
        let mut scene = Bench::one_line();
        let id = scene.add(ViewKind::Model3D);
        let mut open = scene.show(id);
        let before = open.last_scene_line_count();

        scene.line(Point2::new(0.0, 50.0), Point2::new(100.0, 0.0));
        scene.paint(&mut open, id);
        let other = scene.add(ViewKind::Model3D);
        let fresh = scene.show(other);
        assert!(
            fresh.last_scene_line_count() > before,
            "the second line added nothing to draw"
        );
        assert_eq!(open.last_scene_line_count(), fresh.last_scene_line_count());

        scene.document.undo().expect("the undo is done");
        scene.paint(&mut open, id);
        assert_eq!(open.last_scene_line_count(), before);
    }

    #[test]
    fn an_empty_drawing_says_there_is_nothing_to_show() {
        let mut scene = Bench::new();
        let id = scene.add(ViewKind::Model3D);
        let mut view = scene.make(id);
        scene.resize(&mut view, id, 400, 300);
        scene.paint(&mut view, id);
        assert!(view.scene_empty());
        assert!(view.empty_message_shown());
    }

    #[test]
    fn a_drawing_whose_layers_are_all_hidden_is_not_called_empty() {
        // The one line is on layer "0". Hidden in this view, or in the document, the scene is the
        // grid alone - but the drawing is not empty, and telling the user to draw or import
        // something would be wrong.
        let mut scene = Bench::one_line();
        let id = scene.add(ViewKind::Model3D);
        let mut view = scene.show(id);
        assert!(!view.scene_empty());
        assert!(!view.empty_message_shown());

        assert!(scene.state_mut(id).layers.hide("0"));
        view.invalidate_scene(); // the view's own layers are not the document's to announce
        scene.paint(&mut view, id);
        assert!(view.scene_empty());
        assert!(!view.empty_message_shown());

        assert!(scene.state_mut(id).layers.show("0"));
        let hidden = Layer {
            name: String::from("0"),
            visible: false,
            ..Layer::default()
        };
        scene
            .document
            .execute(update_layer(hidden))
            .expect("the layer is hidden");
        scene.paint(&mut view, id);
        assert!(view.scene_empty());
        assert!(!view.empty_message_shown());
    }

    // ---- responsiveness and the look (docs/render.md) --------------------------------------

    /// Pixels of the last paint that are not the view's background, read at its first pixel.
    fn drawn_pixels(view: &RenderView) -> usize {
        let framebuffer = view.framebuffer();
        let background = framebuffer.color_at(0, 0);
        framebuffer
            .color()
            .iter()
            .filter(|&&colour| colour != background)
            .count()
    }

    #[test]
    fn a_selection_click_rebuilds_the_overlay_and_nothing_under_it() {
        // Every document notification rebuilt the whole scene: 60-115 ms on a 229k-triangle
        // surface to recolour one line. A selection leaves the model's revision where it was, so
        // only the overlay is rebuilt.
        let mut scene = Bench::one_line();
        let id = scene.add(ViewKind::Model3D);
        let mut view = scene.show(id);
        assert_eq!(view.terrain_builds(), 1);
        assert_eq!(view.entity_builds(), 1);
        assert_eq!(view.selection_builds(), 1);

        let last = *scene
            .document
            .model()
            .entities
            .ids()
            .last()
            .expect("the line is there");
        scene.document.selection_mut().add(last);
        scene.document.notify_selection_changed();
        scene.paint(&mut view, id);
        assert_eq!(view.terrain_builds(), 1);
        assert_eq!(view.entity_builds(), 1);
        assert_eq!(view.selection_builds(), 2);
        assert_eq!(view.scene_layers().selection.lines.len(), 1);
        // And its casing, in the list drawn under it (SceneLayers::selection_casing).
        assert_eq!(view.scene_layers().selection_casing.lines.len(), 1);

        // An edit is a model change: the drawing is rebuilt, the terrain is not.
        scene.line(Point2::new(0.0, 50.0), Point2::new(100.0, 0.0));
        scene.paint(&mut view, id);
        assert_eq!(view.terrain_builds(), 1);
        assert_eq!(view.entity_builds(), 2);
    }

    #[test]
    fn eight_wheel_notches_out_still_show_the_model() {
        // The depth range was fixed at the frame, so zooming out pushed the model past the far
        // plane and the view went blank. Without the grid, what is drawn is the line alone.
        let mut scene = Bench::one_line();
        let id = scene.add(ViewKind::Model3D);
        let mut view = scene.make(id);
        view.set_context(
            &scene.document,
            SceneOptions {
                draw_grid: false,
                ..SceneOptions::default()
            },
        );
        scene.resize(&mut view, id, 400, 300);
        scene.paint(&mut view, id);
        assert!(drawn_pixels(&view) > 0);
        let centre = PointF::new(200.0, 150.0);
        for _ in 0..8 {
            scene.turn_wheel(&mut view, id, centre, -120);
        }
        scene.paint(&mut view, id);
        assert!(drawn_pixels(&view) > 0);
    }

    #[test]
    fn a_view_framed_before_it_has_its_size_frames_again_at_its_first_real_size() {
        // The window zooms a new view to extents before the dock has laid it out; the frame
        // fitted that size, and a tall thin view then cut the sides off. Until the user moves the
        // camera a resize frames again, so every corner of the drawing's box is inside the
        // 345 x 545 view.
        let mut scene = Bench::one_line();
        let id = scene.add(ViewKind::Model3D);
        let mut view = scene.make(id);
        scene.resize(&mut view, id, 900, 120);
        scene.paint(&mut view, id);
        scene.zoom_extents(&mut view, id);
        scene.resize(&mut view, id, 345, 545);
        scene.paint(&mut view, id);
        let bounds = view.scene_layers().bounds;
        assert!(!bounds.empty());
        // The camera counts in device pixels: 345 x 545 at ratio 1.
        let width = f64::from(view.framebuffer().width());
        let height = f64::from(view.framebuffer().height());
        let camera = scene.camera(id);
        assert_eq!(camera.viewport_width(), view.framebuffer().width());
        for corner in 0..8 {
            let p = Vec3::new(
                if corner & 1 != 0 { bounds.max.x } else { bounds.min.x },
                if corner & 2 != 0 { bounds.max.y } else { bounds.min.y },
                if corner & 4 != 0 { bounds.max.z } else { bounds.min.z },
            );
            let screen = camera.project(p).expect("the corner projects");
            assert!(screen.x >= 0.0, "corner {corner}: x {}", screen.x);
            assert!(screen.x <= width, "corner {corner}: x {}", screen.x);
            assert!(screen.y >= 0.0, "corner {corner}: y {}", screen.y);
            assert!(screen.y <= height, "corner {corner}: y {}", screen.y);
        }
    }

    #[test]
    fn the_frame_label_counts_the_scene_build_when_a_paint_did_one() {
        // The label showed the rasteriser alone, which hid the build a click cost. It is the
        // whole paint, and says so when a build was part of it.
        let mut scene = Bench::one_line();
        let id = scene.add(ViewKind::Model3D);
        let mut view = scene.make(id);
        scene.resize(&mut view, id, 400, 300);
        scene.paint(&mut view, id);
        let label = frame_stats(&view.take_events())
            .pop()
            .expect("the paint said what it drew");
        assert!(label.contains("scene"), "{label}");
        assert!(view.last_frame_milliseconds() >= view.last_build_milliseconds());
        scene.paint(&mut view, id);
        let label = frame_stats(&view.take_events())
            .pop()
            .expect("the paint said what it drew");
        assert!(!label.contains("scene"), "nothing was rebuilt: {label}");
    }

    #[test]
    fn the_framebuffer_is_the_views_size_in_device_pixels() {
        // At the display's own resolution: a 125% display drew at 80% and scaled the image up,
        // beading every 1 px line into 1-2 px steps. The C++ ran this again under
        // QT_SCALE_FACTOR=1.25 as a second ctest entry; a headless egui run is at ratio 1, as the
        // offscreen suite is.
        let mut scene = Bench::one_line();
        let id = scene.add(ViewKind::Model3D);
        let view = scene.show(id);
        let ratio = view.device_ratio();
        assert_eq!(
            view.framebuffer().width(),
            (400.0 * ratio).round() as i32
        );
        assert_eq!(
            view.framebuffer().height(),
            (300.0 * ratio).round() as i32
        );
        assert_eq!(scene.camera(id).viewport_width(), view.framebuffer().width());
    }

    // ---- zooming with the wheel (docs/render.md, "Zooming towards the cursor") --------------
    //
    // The wheel anchored every notch on the plane through the orbit target, facing the eye - not
    // on what is drawn under the cursor. Over ground beyond that plane the eye closed in on the
    // anchor and the zoom stalled: each notch took 13% of what was left of the way, so after
    // about 30 notches nothing on screen moved, and the pan, scaled by that collapsed distance,
    // died with it. Over ground nearer than the plane the eye went through the ground and the
    // view went blank. The cases below find the ground under the cursor by hand - the ray through
    // the cursor's pixel met with the plane z = 10 - and never ask the view what it zoomed
    // towards.

    const GROUND_HEIGHT: f64 = 10.0;
    /// One wheel notch in eighths of a degree, and the magnification it stands for (2x in five
    /// notches).
    const NOTCH: i32 = 120;
    const PER_NOTCH: f64 = 1.15;
    /// The view's background (render_view_widget.rs, BACKGROUND).
    const VIEW_BACKGROUND: Rgba = rgba(28, 30, 36);

    /// Flat ground 200 m square at z = 10, from an 11 x 11 grid of points 20 m apart, and nothing
    /// else: the datum is its floor, so the navigation grid stands in the same plane. Framed from
    /// the south-west, as a 3D view opens, the target is the middle of the ground,
    /// (100, 100, 10), ON the ground: the ground above the middle of the view lies beyond the
    /// target's plane and the ground below the middle nearer than it.
    fn flat_ground_bench() -> Bench {
        let mut scene = Bench::new();
        scene
            .ground
            .push((String::from("Ground"), flat_ground(GROUND_HEIGHT)));
        scene
    }

    /// A 3D view over flat ground, `width` by `height`, painted once.
    fn ground_view(width: i32, height: i32) -> (Bench, ViewId, RenderView) {
        let mut scene = flat_ground_bench();
        let id = scene.add(ViewKind::Model3D);
        let mut view = scene.make(id);
        scene.resize(&mut view, id, width, height);
        scene.paint(&mut view, id);
        (scene, id, view)
    }

    /// Where the ray through the cursor meets the ground; nothing when it does not. The cursor is
    /// in the view's logical pixels, the camera counts in its device pixels.
    fn ground_under(camera: &Camera, cursor: PointF, ratio: f64) -> Option<Vec3> {
        let ray = camera.ray_through_pixel(cursor.x * ratio, cursor.y * ratio);
        if ray.direction.z == 0.0 {
            return None;
        }
        let t = (GROUND_HEIGHT - ray.origin.z) / ray.direction.z;
        if !(t > 0.0) {
            return None;
        }
        Some(ray.at(t))
    }

    /// How far in front of the eye `point` is along the view direction. A pixel's footprint there
    /// is proportional to it, so the ratio of two depths of one point is how much a zoom
    /// magnified it.
    fn depth_of(camera: &Camera, point: Vec3) -> f64 {
        (point - camera.eye()).dot(camera.forward())
    }

    /// Pixels of the last paint that are anything but the view's background.
    fn painted_pixels(view: &RenderView) -> usize {
        view.framebuffer()
            .color()
            .iter()
            .filter(|&&colour| colour != VIEW_BACKGROUND)
            .count()
    }

    fn save(view: &RenderView, name: &str) {
        save_for_looking("KATANA_RENDER_VIEW_IMAGES", name, view.picture());
    }

    fn two_digits(n: i32) -> String {
        format!("{n:02}")
    }

    #[test]
    fn every_wheel_notch_magnifies_the_ground_under_the_cursor_by_the_same_factor() {
        // Above the middle of the view the ground lies beyond the target's plane. Anchored on that
        // plane, the magnification of the ground under the cursor tended to depth / (depth -
        // distance) and stopped there. Every notch must magnify it by 1.15, and the same point of
        // the ground must stay under the cursor.
        let (mut scene, id, mut view) = ground_view(400, 300);
        let ratio = view.device_ratio();
        let cursor = PointF::new(200.0, 90.0);
        let start = ground_under(&scene.camera(id), cursor, ratio).expect("ground under the cursor");
        assert!(
            depth_of(&scene.camera(id), start) > 1.1 * scene.camera(id).distance(),
            "the ground under the cursor is not beyond the target's plane, so this proves nothing"
        );
        save(&view, "zoom_far_00");

        let first = depth_of(&scene.camera(id), start);
        let mut depth = first;
        let mut worst_gain = PER_NOTCH;
        let mut drift: f64 = 0.0;
        let mut gains = String::new();
        for notch in 1..=40 {
            scene.turn_wheel(&mut view, id, cursor, NOTCH);
            let camera = scene.camera(id);
            let under = ground_under(&camera, cursor, ratio)
                .unwrap_or_else(|| panic!("no ground under the cursor after notch {notch}"));
            let now = depth_of(&camera, under);
            let gain = depth / now;
            if (gain - PER_NOTCH).abs() > (worst_gain - PER_NOTCH).abs() {
                worst_gain = gain;
            }
            drift = drift.max((under - start).length());
            if notch == 1 || notch % 10 == 0 {
                gains.push_str(&format!(
                    " notch {notch}: x{gain} (x{} in all);",
                    first / now
                ));
                scene.paint(&mut view, id);
                save(&view, &format!("zoom_far_{}", two_digits(notch)));
            }
            depth = now;
        }
        assert!((worst_gain - PER_NOTCH).abs() <= 1e-9, "{gains}");
        let expected = PER_NOTCH.powf(40.0);
        assert!(
            (first / depth - expected).abs() <= 1e-6 * expected,
            "{gains}"
        );
        // A micrometre, on ground some 300 m from where the eye started.
        assert!(
            drift < 1e-6,
            "the ground under the cursor slid by {drift} m;{gains}"
        );
    }

    #[test]
    fn wheel_notches_over_the_near_ground_never_take_the_eye_through_it() {
        // Below the middle of the view the ground is nearer than the target's plane: the old
        // anchor lay under the ground, so the eye went through it and the view was blank from then
        // on - the ground behind the eye and nothing in front of it. The eye must stay above the
        // ground, the ground under the cursor in front of it and magnified by 1.15 a notch, and the
        // view must go on drawing it.
        let (mut scene, id, mut view) = ground_view(400, 300);
        let ratio = view.device_ratio();
        let cursor = PointF::new(200.0, 250.0);
        let start = ground_under(&scene.camera(id), cursor, ratio).expect("ground under the cursor");
        assert!(
            depth_of(&scene.camera(id), start) < 0.9 * scene.camera(id).distance(),
            "the ground under the cursor is not nearer than the target's plane, so this proves \
             nothing"
        );
        save(&view, "zoom_near_00");

        let mut eye_under = 0;
        let mut blank = 0;
        let mut lost = 0;
        let mut depth = depth_of(&scene.camera(id), start);
        let mut worst_gain = PER_NOTCH;
        for notch in 1..=40 {
            scene.turn_wheel(&mut view, id, cursor, NOTCH);
            scene.paint(&mut view, id);
            let camera = scene.camera(id);
            if eye_under == 0 && !(camera.eye().z > GROUND_HEIGHT) {
                eye_under = notch;
            }
            if blank == 0 && painted_pixels(&view) == 0 {
                blank = notch;
            }
            match ground_under(&camera, cursor, ratio) {
                None => {
                    if lost == 0 {
                        lost = notch;
                    }
                }
                Some(under) if lost == 0 => {
                    let now = depth_of(&camera, under);
                    if (depth / now - PER_NOTCH).abs() > (worst_gain - PER_NOTCH).abs() {
                        worst_gain = depth / now;
                    }
                    depth = now;
                }
                Some(_) => {}
            }
            if notch % 5 == 0 {
                save(&view, &format!("zoom_near_{}", two_digits(notch)));
            }
        }
        assert_eq!(eye_under, 0, "the eye went under the ground at notch {eye_under}");
        assert_eq!(blank, 0, "the view drew nothing but its background from notch {blank}");
        assert_eq!(lost, 0, "no ground under the cursor from notch {lost}");
        assert!((worst_gain - PER_NOTCH).abs() <= 1e-9, "worst gain {worst_gain}");
    }

    #[test]
    fn after_zooming_in_a_drag_still_carries_the_ground_under_the_cursor_with_it() {
        // A pan moves the target's plane by the drag, scaled by the camera's distance. Stalled,
        // the distance had shrunk to centimetres while the ground stayed far off, and a 100 px
        // drag moved the ground under the cursor a tenth of a pixel. Zoomed towards the ground,
        // the target's plane is the ground's, and the ground under the cursor follows the drag
        // exactly: 100 logical pixels, which the camera counts in device pixels.
        let (mut scene, id, mut view) = ground_view(400, 300);
        let ratio = view.device_ratio();
        let cursor = PointF::new(200.0, 90.0);
        for _ in 0..30 {
            scene.turn_wheel(&mut view, id, cursor, NOTCH);
        }
        let under = ground_under(&scene.camera(id), cursor, ratio).expect("ground under the cursor");
        let before = scene.camera(id).project(under).expect("it projects");

        scene.middle_drag(&mut view, id, cursor, cursor + PointF::new(100.0, 0.0));
        let after = scene.camera(id).project(under).expect("it projects");
        assert!(((after.x - before.x) - 100.0 * ratio).abs() <= 1e-6 * 100.0 * ratio);
        assert!((after.y - before.y).abs() <= 1e-6 * 100.0 * ratio);
    }

    #[test]
    fn a_notch_out_after_zooming_in_shrinks_the_ground_under_the_cursor_by_the_same_factor() {
        // Stalled, the eye sat almost on the old anchor and a notch out moved it back 15% of
        // almost nothing: the ground under the cursor barely shrank. A notch out must undo a notch
        // in, and as many notches out as went in must bring the eye back to where it started.
        let (mut scene, id, mut view) = ground_view(400, 300);
        let ratio = view.device_ratio();
        let cursor = PointF::new(200.0, 90.0);
        let eye = scene.camera(id).eye();
        let start = ground_under(&scene.camera(id), cursor, ratio).expect("ground under the cursor");
        let start_depth = depth_of(&scene.camera(id), start);

        for _ in 0..30 {
            scene.turn_wheel(&mut view, id, cursor, NOTCH);
        }
        let inward = ground_under(&scene.camera(id), cursor, ratio).expect("ground in");
        let in_depth = depth_of(&scene.camera(id), inward);
        scene.turn_wheel(&mut view, id, cursor, -NOTCH);
        let out = ground_under(&scene.camera(id), cursor, ratio).expect("ground out");
        assert!((depth_of(&scene.camera(id), out) / in_depth - PER_NOTCH).abs() <= 1e-9);

        for _ in 1..30 {
            scene.turn_wheel(&mut view, id, cursor, -NOTCH);
        }
        assert!((scene.camera(id).eye() - eye).length() <= 1e-9 * start_depth);
        let back = ground_under(&scene.camera(id), cursor, ratio).expect("ground back");
        assert!((depth_of(&scene.camera(id), back) - start_depth).abs() <= 1e-9 * start_depth);
    }

    // ---- off the model: the background round it in a docked view ---------------------------
    //
    // The window's 3D dock is tall and narrow, about 330 x 520, and a scene framed to its width
    // leaves two thirds of it background: the datum beyond the grid, where nothing is drawn.
    // There the wheel still anchored on the target's plane, so below the model the eye dived
    // under the ground and above it the eye converged on a point in the air, and either way the
    // whole view was blank from about the twentieth notch on (docs/render.md, "Zooming towards
    // the cursor").

    /// Nothing of the view's own layers - terrain, drawing, the grid's plane - is under the
    /// cursor.
    fn nothing_drawn_under(view: &RenderView, camera: &Camera, cursor: PointF, ratio: f64) -> bool {
        pick_drawn_point(
            view.scene_layers(),
            camera,
            cursor.x * ratio,
            cursor.y * ratio,
            &PickOptions::default(),
        )
        .is_none()
    }

    /// How deep the scene's box reaches along the view: its furthest corner.
    fn reach_of(view: &RenderView, camera: &Camera) -> f64 {
        let bounds = scene_depth_box(view.scene_layers());
        let mut reach = -1.0e300_f64;
        for corner in 0..8 {
            let p = Vec3::new(
                if corner & 1 != 0 { bounds.max.x } else { bounds.min.x },
                if corner & 2 != 0 { bounds.max.y } else { bounds.min.y },
                if corner & 4 != 0 { bounds.max.z } else { bounds.min.z },
            );
            reach = reach.max(depth_of(camera, p));
        }
        reach
    }

    #[test]
    fn in_a_dock_sized_view_the_wheel_below_the_model_zooms_towards_the_datum_and_back_out_again() {
        // Below the model the ground is the datum, the plane the grid stands on, nearer than the
        // target's plane: the old anchor lay under it, and the eye went under the ground and
        // stayed there. The eye must stay above it, the datum's point under the cursor must divide
        // its depth by 1.15 a notch, and as many notches out must bring the view back as it was.
        // Not that the view never goes blank: the model lies in the plane the zoom closes on,
        // which the eye comes to see edge on - from h above it ground L away is atan(h / L) below
        // the horizon, and the top of this view is 35.26 - 22.5 = 12.76 degrees below it - so the
        // model leaves the top of the view as the eye nears the empty ground it went towards.
        let (mut scene, id, mut view) = ground_view(330, 520);
        let painted = painted_pixels(&view);
        assert!(painted > 1000);
        let ratio = view.device_ratio();
        let cursor = PointF::new(165.0, 510.0);
        assert!(
            nothing_drawn_under(&view, &scene.camera(id), cursor, ratio),
            "the cursor is over the model, so this proves nothing"
        );
        let eye = scene.camera(id).eye();
        let start = ground_under(&scene.camera(id), cursor, ratio).expect("the datum under it");
        let mut depth = depth_of(&scene.camera(id), start);
        assert!(
            depth < 0.9 * scene.camera(id).distance(),
            "the datum is not nearer than the target's plane, so the old anchor did not dive"
        );
        save(&view, "dock_below_00");
        for notch in 1..=40 {
            scene.turn_wheel(&mut view, id, cursor, NOTCH);
            let camera = scene.camera(id);
            assert!(
                camera.eye().z > GROUND_HEIGHT,
                "the eye is under the datum at notch {notch}"
            );
            let under = ground_under(&camera, cursor, ratio)
                .unwrap_or_else(|| panic!("no datum under the cursor at notch {notch}"));
            let now = depth_of(&camera, under);
            assert!((depth / now - PER_NOTCH).abs() <= 1e-9, "notch {notch}");
            depth = now;
            if notch % 10 == 0 {
                scene.paint(&mut view, id);
                save(&view, &format!("dock_below_{}", two_digits(notch)));
            }
        }
        for _ in 1..=40 {
            scene.turn_wheel(&mut view, id, cursor, -NOTCH);
        }
        let camera = scene.camera(id);
        assert!((camera.eye() - eye).length() <= 1e-9 * camera.distance());
        scene.paint(&mut view, id);
        save(&view, "dock_below_back");
        let now = painted_pixels(&view) as f64;
        assert!((now - painted as f64).abs() <= 0.01 * painted as f64);
    }

    #[test]
    fn in_a_dock_sized_view_the_wheel_above_the_model_keeps_magnifying_the_scene_as_far_as_it_reaches()
     {
        // Above the model the ray meets the datum beyond everything the scene reaches, so the
        // zoom goes as deep as the scene reaches: the furthest corner of its box divides its depth
        // by 1.15 a notch, and everything drawn, all of it nearer, grows at least as fast.
        // Anchored on the target's plane, that corner lay beyond the anchor and stalled.
        let (mut scene, id, mut view) = ground_view(330, 520);
        let ratio = view.device_ratio();
        let cursor = PointF::new(165.0, 15.0);
        assert!(
            nothing_drawn_under(&view, &scene.camera(id), cursor, ratio),
            "the cursor is over the model, so this proves nothing"
        );
        let mut reach = reach_of(&view, &scene.camera(id));
        let datum = ground_under(&scene.camera(id), cursor, ratio);
        assert!(
            datum.is_none_or(|datum| depth_of(&scene.camera(id), datum) > reach),
            "the datum is within the scene's reach, so this is the case below the model"
        );
        assert!(
            reach > 1.1 * scene.camera(id).distance(),
            "the scene reaches no further than the target's plane, so nothing stalled"
        );
        save(&view, "dock_above_00");
        for notch in 1..=40 {
            scene.turn_wheel(&mut view, id, cursor, NOTCH);
            let now = reach_of(&view, &scene.camera(id));
            assert!((reach / now - PER_NOTCH).abs() <= 1e-9, "notch {notch}");
            reach = now;
            assert!(scene.camera(id).eye().z > GROUND_HEIGHT, "notch {notch}");
            if notch % 10 == 0 {
                scene.paint(&mut view, id);
                save(&view, &format!("dock_above_{}", two_digits(notch)));
            }
        }
    }

    // ---- the pan and the orbit after a zoom -------------------------------------------------

    #[test]
    fn after_a_zoom_the_orbit_turns_about_the_point_of_the_view_axis_as_deep_as_what_the_zoom_went_towards()
     {
        // The zoom moves the pivot to the depth of what the cursor points at
        // (Camera::set_pivot_depth): on the view axis, as deep as the ground under the cursor. An
        // orbit then turns about that point and keeps it.
        let (mut scene, id, mut view) = ground_view(400, 300);
        let ratio = view.device_ratio();
        let cursor = PointF::new(200.0, 90.0);
        for _ in 0..10 {
            scene.turn_wheel(&mut view, id, cursor, NOTCH);
        }
        let camera = scene.camera(id);
        let under = ground_under(&camera, cursor, ratio).expect("ground under the cursor");
        let depth = depth_of(&camera, under);
        assert!((camera.distance() - depth).abs() <= 1e-9 * depth);
        let pivot = camera.eye() + camera.forward() * depth;
        assert!((camera.target() - pivot).length() <= 1e-9 * depth);

        scene.left_drag(
            &mut view,
            id,
            PointF::new(200.0, 150.0),
            PointF::new(300.0, 170.0),
        );
        let camera = scene.camera(id);
        assert!((camera.target() - pivot).length() <= 1e-9 * depth);
        assert!((camera.distance() - depth).abs() <= 1e-9 * depth);
    }

    #[test]
    fn after_a_zoom_a_pan_started_elsewhere_moves_the_ground_there_in_proportion_to_the_pivots_depth()
     {
        // A pan slides the camera across the view by the drag, counted at the pivot's depth: the
        // ground at the depth the zoom went to follows the drag one to one, and ground at depth d
        // moves pivot / d of it - as a perspective pan always did, the pivot being where the zoom
        // left it.
        let (mut scene, id, mut view) = ground_view(400, 300);
        let ratio = view.device_ratio();
        for _ in 0..10 {
            scene.turn_wheel(&mut view, id, PointF::new(200.0, 90.0), NOTCH);
        }
        let from = PointF::new(200.0, 250.0);
        let camera = scene.camera(id);
        let ground = ground_under(&camera, from, ratio).expect("ground there");
        let d = depth_of(&camera, ground);
        let pivot = camera.distance();
        assert!(
            d < 0.8 * pivot,
            "the ground there is as deep as the pivot, so this proves little"
        );
        let before = camera.project(ground).expect("it projects");
        scene.middle_drag(&mut view, id, from, from + PointF::new(100.0, 0.0));
        let after = scene.camera(id).project(ground).expect("it projects");
        let expected = 100.0 * ratio * pivot / d;
        assert!(((after.x - before.x) - expected).abs() <= 1e-6 * expected);
        assert!((after.y - before.y).abs() <= 1e-6 * expected);
    }

    // ---- where the zoom stops ---------------------------------------------------------------

    #[test]
    fn a_zoom_held_at_its_limit_says_so_on_the_status_line_and_nothing_before() {
        // The zoom stops where the view could no longer be drawn exactly (cad's
        // minimum_approach): here the near plane's limit, 1e-7 / 1e-3 = 0.1 mm, the double
        // arithmetic's bound on ground a few hundred metres from the origin being nanometres. From
        // a depth D the eye reaches it at n = ln(D / 1e-4) / ln 1.15 notches, so ground between
        // 1e-4 x 1.15^100 = 117 m and 1e-4 x 1.15^130 = 8.1 km deep is not reached in 100 notches
        // and is in 130. A wheel that stopped without a word read as the fault the zoom once had,
        // so the view says why it stopped - and only then.
        let (mut scene, id, mut view) = ground_view(400, 300);
        let mut said: Vec<String> = statuses(&view.take_events());
        let ratio = view.device_ratio();
        let cursor = PointF::new(200.0, 90.0);
        let ground = ground_under(&scene.camera(id), cursor, ratio).expect("ground under the cursor");
        let depth = depth_of(&scene.camera(id), ground);
        assert!(
            depth > 1e-4 * PER_NOTCH.powf(100.0),
            "the limit comes within 100 notches"
        );
        assert!(
            depth < 1e-4 * PER_NOTCH.powf(130.0),
            "the limit is not reached in 130"
        );
        let limit_said = |said: &[String]| {
            said.iter()
                .filter(|text| text.as_str() == ZOOM_LIMIT_MESSAGE)
                .count()
        };
        for _ in 0..100 {
            scene.turn_wheel(&mut view, id, cursor, NOTCH);
        }
        said.extend(statuses(&view.take_events()));
        assert_eq!(limit_said(&said), 0, "the view said it was held before it was");
        for _ in 100..130 {
            scene.turn_wheel(&mut view, id, cursor, NOTCH);
        }
        said.extend(statuses(&view.take_events()));
        assert!(limit_said(&said) > 0, "the zoom stopped without a word");
        let under = ground_under(&scene.camera(id), cursor, ratio).expect("ground under the cursor");
        assert!((depth_of(&scene.camera(id), under) - 1e-4).abs() <= 1e-6 * 1e-4);
        // A notch out moves away from the limit and says nothing more.
        let before = limit_said(&said);
        scene.turn_wheel(&mut view, id, cursor, -NOTCH);
        said.extend(statuses(&view.take_events()));
        assert_eq!(limit_said(&said), before);
    }
}
