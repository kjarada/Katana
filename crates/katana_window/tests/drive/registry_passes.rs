//! The drive registry over egui's nested passes. Not a twin: the C++ window's floating docks
//! were Qt windows in the one object tree. On the desktop a floating view is a window of its own,
//! an egui immediate viewport, whose pass egui runs INSIDE the main window's (docs/desktop.md, "A
//! floating view is a window of its own"); the registry's frame is the main window's, and the
//! view window's controls are recorded into it.
//!
//! The nested pass is made here as eframe makes it - the viewport's pass run on the same context
//! from the renderer egui calls for an immediate viewport - with no window opened.

mod registry_passes {
    use egui::{
        Context, Pos2, RawInput, Rect, ViewportBuilder, ViewportClass, ViewportId, ViewportInfo,
        vec2,
    };
    use katana_window::drive::Harness;
    use katana_window::drive::controls;

    /// The view window's size in this case: smaller than the harness's screen, so a frame that
    /// took the view window's screen for its own is told apart.
    const WINDOW: Rect = Rect::from_min_max(Pos2::ZERO, Pos2::new(400.0, 300.0));

    /// What eframe's renderer does for an immediate viewport (eframe's
    /// `render_immediate_viewport`), less the window: its pass run on the same context, with
    /// input of its own, while the pass that showed it is still being drawn. The renderer is the
    /// test thread's own (egui keeps it in a thread local), so no other case sees it.
    fn render_immediate_viewports_as_the_desktop_does() {
        Context::set_immediate_viewport_renderer(|ctx, mut viewport| {
            let mut input = RawInput {
                viewport_id: viewport.ids.this,
                screen_rect: Some(WINDOW),
                ..RawInput::default()
            };
            input
                .viewports
                .insert(viewport.ids.this, ViewportInfo::default());
            let _ = ctx.run_ui(input, |ui| (viewport.viewport_ui_cb)(ui));
        });
    }

    #[test]
    fn a_view_windows_pass_inside_the_main_windows_adds_to_its_frame_and_does_not_end_it() {
        render_immediate_viewports_as_the_desktop_does();
        let mut harness = Harness::new();
        harness.context().set_embed_viewports(false);
        let mut classes = Vec::new();
        harness
            .run(|ui| {
                let _ = controls::button(ui, "before", "Before");
                ui.ctx().show_viewport_immediate(
                    ViewportId::from_hash_of("aViewWindow"),
                    ViewportBuilder::default(),
                    |ui, class| {
                        classes.push(class);
                        let _ = controls::button(ui, "inside", "Inside");
                    },
                );
                let _ = controls::button(ui, "after", "After");
            })
            .unwrap_or_else(|problem| panic!("{problem}"));
        // The case reached what it is about: a pass of a window of its own, not one egui
        // embedded in the main window's.
        // (egui's ViewportClass has no Debug, so the passes are counted, not printed.)
        assert!(!classes.is_empty());
        let immediate = classes
            .iter()
            .filter(|class| **class == ViewportClass::Immediate)
            .count();
        assert_eq!(immediate, classes.len(), "a pass egui embedded");
        // One frame holds all three, in the order drawn. Begun and ended by every pass, the
        // registry kept "after" alone: the view window's pass wiped "before", and the frame it
        // completed was thrown away when the main window's ended.
        let frame = harness.snapshot();
        let names: Vec<&str> = frame
            .controls
            .iter()
            .map(|control| control.name.as_str())
            .filter(|name| ["before", "inside", "after"].contains(name))
            .collect();
        assert_eq!(names, ["before", "inside", "after"]);
        // The frame is the main window's: its screen, not the view window's.
        assert_eq!(frame.screen, harness.screen());
        assert_ne!(frame.screen.size(), vec2(400.0, 300.0));
    }
}
