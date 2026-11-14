//! What the plan view's twins share: a view on a drawing with its events sent as the C++ tests
//! send Qt events to the widget, what the view raised kept by kind, and the ink of a colour in the
//! view's picture.
//!
//! The C++ tests send a QMouseEvent or a QKeyEvent straight to a ViewportWidget at a pixel of the
//! widget; here the same events are the view's own methods at the same pixel, and `paint` is the
//! widget's grab. A hook the C++ test set is a flag of `PlanViewHooks` and the events it would
//! have been called with, read from `log`.

#![allow(
    dead_code,
    reason = "Each file of the target uses only some of the fixture's helpers."
)]

use katana_cad::Document;
use katana_cad::view_set::{ViewSet, ViewState};
use katana_cad::viewport_layout::ViewKind;
use katana_commands::{EntityAttributes, create_line, create_polyline};
use katana_entity::{Entity, EntityId, Geometry};
use katana_geometry::{Point2, Polyline2};
use katana_window::canvas::{Color, Image, PointF};
use katana_window::theme::palette;
use katana_window::viewport_widget::{
    KeyModifiers, MouseButton, PlanView, PlanViewEvent, Tool, ViewKey,
};

/// The state of a new plan view, as `ViewSet::add(ViewKind::Plan)` gives the C++ tests theirs: the
/// first view of a set, id 1.
pub fn plan_state() -> ViewState {
    let mut views = ViewSet::new();
    views.add(ViewKind::Plan).clone()
}

/// A plan view on a drawing, with everything it has raised.
pub struct Plan {
    pub document: Document,
    pub state: ViewState,
    pub view: PlanView,
    /// Every event the view has raised, in order (`drain` moves them here).
    pub log: Vec<PlanViewEvent>,
    /// The polyline of `over_polyline`'s drawing; 0 otherwise.
    pub polyline: EntityId,
}

impl Plan {
    /// A view of `document` through `state`, 400 x 300 pixels and painted once.
    fn painted(document: Document, mut state: ViewState) -> Self {
        let mut view = PlanView::new(&document, &mut state);
        view.resize(&mut state, 400, 300);
        let mut plan = Self {
            document,
            state,
            view,
            log: Vec::new(),
            polyline: 0,
        };
        plan.paint();
        plan
    }

    /// The view test_plan_view_tools.cpp works by hand: 400 x 300 pixels centred on the model
    /// origin at 10 pixels a unit and never framed, so
    /// `x = (px - 200) / 10` and `y = -(py - 150) / 10`. Snapping is off unless a test turns it
    /// on, so a click lands exactly where the hand-worked sum says.
    pub fn at_origin() -> Self {
        let document = Document::new(None);
        let mut state = plan_state();
        // Never framed: the centre and scale stay put.
        state.plan_framed = true;
        state.plan.center = Point2::new(0.0, 0.0);
        state.plan.scale = 10.0;
        state.plan.resize(400.0, 300.0);
        let mut view = PlanView::new(&document, &mut state);
        view.resize(&mut state, 400, 300);
        let mut plan = Self {
            document,
            state,
            view,
            log: Vec::new(),
            polyline: 0,
        };
        plan.view.set_snap_enabled(&mut plan.document, false);
        plan.paint();
        plan
    }

    /// The view the grip and vertex-tool tests share: the polyline (0,0)-(10,0)-(20,0) in a
    /// 400 x 300 view at 10 px a unit centred on (10, 5), so model (0, 0) is at pixel (100, 200)
    /// and a unit is ten pixels. Nothing snaps, so a drag lands exactly where the mouse is.
    pub fn over_polyline() -> Self {
        let mut document = Document::new(None);
        document
            .execute(create_polyline(
                Polyline2::new(
                    vec![
                        Point2::new(0.0, 0.0),
                        Point2::new(10.0, 0.0),
                        Point2::new(20.0, 0.0),
                    ],
                    false,
                ),
                EntityAttributes::default(),
            ))
            .expect("the polyline is made");
        let polyline = document.last_created_entities()[0];
        let mut state = plan_state();
        state.plan_framed = true;
        state.plan.center = Point2::new(10.0, 5.0);
        state.plan.scale = 10.0;
        let mut plan = Self::painted(document, state);
        plan.polyline = polyline;
        plan.state.plan.center = Point2::new(10.0, 5.0);
        plan.state.plan.scale = 10.0;
        plan.view.set_snap_enabled(&mut plan.document, false);
        plan
    }

    /// The widget painted at its size now, as the screen would do it (the C++ `paint(view)`).
    pub fn paint(&mut self) {
        let _ = self.view.paint(&self.document, &mut self.state);
        self.drain();
    }

    /// The widget's grab: painted, and the picture.
    pub fn grab(&mut self) -> Image {
        self.paint();
        self.view.picture().clone()
    }

    pub fn resize(&mut self, width: i32, height: i32) {
        self.view.resize(&mut self.state, width, height);
    }

    /// What the view has raised since the last drain, moved to `log`.
    pub fn drain(&mut self) {
        self.log.extend(self.view.take_events());
    }

    /// The pixel of the view a model point is drawn at.
    pub fn pixel(&self, world: Point2) -> PointF {
        let p = self.state.plan.world_to_screen(world);
        PointF::new(p.x, p.y)
    }

    // ---- the mouse, at a pixel -------------------------------------------------------------

    pub fn mouse_press(&mut self, at: PointF, button: MouseButton, modifiers: KeyModifiers) {
        self.view
            .mouse_press(&mut self.document, &mut self.state, at, button, modifiers);
        self.drain();
    }

    pub fn mouse_release(&mut self, at: PointF, button: MouseButton, modifiers: KeyModifiers) {
        self.view
            .mouse_release(&mut self.document, &mut self.state, at, button, modifiers);
        self.drain();
    }

    pub fn mouse_double_click(&mut self, at: PointF, button: MouseButton) {
        self.view.mouse_double_click(
            &mut self.document,
            &mut self.state,
            at,
            button,
            KeyModifiers::NONE,
        );
        self.drain();
    }

    pub fn mouse_move(&mut self, at: PointF, modifiers: KeyModifiers) {
        self.view
            .mouse_move(&mut self.document, &mut self.state, at, modifiers);
        self.drain();
    }

    /// A press and its release at pixel (x, y) with the left button.
    pub fn press(&mut self, x: f64, y: f64) {
        self.press_with(x, y, MouseButton::Left, KeyModifiers::NONE);
    }

    pub fn press_with(&mut self, x: f64, y: f64, button: MouseButton, modifiers: KeyModifiers) {
        let at = PointF::new(x, y);
        self.mouse_press(at, button, modifiers);
        self.mouse_release(at, button, modifiers);
    }

    /// A press at one pixel and the release at another: a selection box.
    pub fn drag_pixels(&mut self, from: PointF, to: PointF, modifiers: KeyModifiers) {
        self.mouse_press(from, MouseButton::Left, modifiers);
        self.mouse_release(to, MouseButton::Left, modifiers);
    }

    pub fn move_to(&mut self, x: f64, y: f64) {
        self.mouse_move(PointF::new(x, y), KeyModifiers::NONE);
    }

    /// As Qt delivers a double click: press, release, double click, release.
    pub fn double_click(&mut self, x: f64, y: f64) {
        let at = PointF::new(x, y);
        self.press(x, y);
        self.mouse_double_click(at, MouseButton::Left);
        self.mouse_release(at, MouseButton::Left, KeyModifiers::NONE);
    }

    pub fn wheel(&mut self, x: f64, y: f64, angle_delta: f64) {
        self.view.wheel(
            &mut self.document,
            &mut self.state,
            PointF::new(x, y),
            angle_delta,
        );
        self.drain();
    }

    // ---- the mouse, at a model point (the grip and vertex-tool fixtures) -------------------

    pub fn press_at(&mut self, world: Point2, modifiers: KeyModifiers) {
        let at = self.pixel(world);
        self.mouse_press(at, MouseButton::Left, modifiers);
    }

    pub fn release_at(&mut self, world: Point2, modifiers: KeyModifiers) {
        let at = self.pixel(world);
        self.mouse_release(at, MouseButton::Left, modifiers);
    }

    /// A press and its release at a model point.
    pub fn click_at(&mut self, world: Point2, modifiers: KeyModifiers) {
        self.press_at(world, modifiers);
        self.release_at(world, modifiers);
    }

    pub fn hover(&mut self, world: Point2) {
        self.hover_with(world, KeyModifiers::NONE);
    }

    pub fn hover_with(&mut self, world: Point2, modifiers: KeyModifiers) {
        let at = self.pixel(world);
        self.mouse_move(at, modifiers);
    }

    /// A press, a move half way, a move to the end and the release there.
    pub fn drag(&mut self, from: Point2, to: Point2, modifiers: KeyModifiers) {
        self.press_at(from, modifiers);
        self.hover_with(
            Point2::new((from.x + to.x) * 0.5, (from.y + to.y) * 0.5),
            modifiers,
        );
        self.hover_with(to, modifiers);
        self.release_at(to, modifiers);
    }

    // ---- the keys --------------------------------------------------------------------------

    /// A key press alone (QEvent::KeyPress).
    pub fn key_press(&mut self, key: ViewKey, modifiers: KeyModifiers, text: &str) -> bool {
        let taken = self
            .view
            .key_press(&mut self.document, &mut self.state, key, modifiers, text);
        self.drain();
        taken
    }

    /// Whether the view claims the key from the window's shortcuts (QEvent::ShortcutOverride).
    pub fn claims(&mut self, key: ViewKey, modifiers: KeyModifiers) -> bool {
        self.view
            .shortcut_override(&self.document, &self.state, key, modifiers)
    }

    /// A shortcut is offered to the widget first, as Qt does, so the view's ShortcutOverride
    /// handling is exercised too; then the key press.
    pub fn key(&mut self, key: ViewKey, modifiers: KeyModifiers, text: &str) {
        let _ = self.claims(key, modifiers);
        let _ = self.key_press(key, modifiers, text);
    }

    /// A key with no modifiers that types nothing.
    pub fn plain_key(&mut self, key: ViewKey) {
        let _ = self.key_press(key, KeyModifiers::NONE, "");
    }

    /// The key a character is on, as far as the view tells keys apart.
    pub fn key_of(character: char) -> ViewKey {
        match character {
            ' ' => ViewKey::Space,
            'z' | 'Z' => ViewKey::Z,
            _ => ViewKey::Other,
        }
    }

    /// Types `text` a character at a time, as a keyboard does (shortcut offer, then the press).
    pub fn type_text(&mut self, text: &str) {
        for character in text.chars() {
            self.key(
                Self::key_of(character),
                KeyModifiers::NONE,
                &character.to_string(),
            );
        }
    }

    /// Types `text` a character at a time with key presses alone.
    pub fn type_keys(&mut self, text: &str) {
        for character in text.chars() {
            let _ = self.key_press(
                Self::key_of(character),
                KeyModifiers::NONE,
                &character.to_string(),
            );
        }
    }

    pub fn enter(&mut self) {
        self.key(ViewKey::Enter, KeyModifiers::NONE, "");
    }

    pub fn escape(&mut self) {
        self.key(ViewKey::Escape, KeyModifiers::NONE, "");
    }

    // ---- the view's controls ---------------------------------------------------------------

    pub fn start_tool(&mut self, id: &str) -> katana_core::Status {
        let started = self
            .view
            .start_tool(&mut self.document, &mut self.state, id);
        self.drain();
        started
    }

    pub fn set_tool(&mut self, tool: Tool) {
        self.view
            .set_tool(&mut self.document, &mut self.state, tool);
        self.drain();
    }

    pub fn set_snap_enabled(&mut self, enabled: bool) {
        self.view.set_snap_enabled(&mut self.document, enabled);
    }

    /// The fixture's polyline selected, announced and painted.
    pub fn select(&mut self) {
        self.document.selection_mut().add(self.polyline);
        self.document.notify_selection_changed();
        self.paint();
    }

    // ---- the drawing -----------------------------------------------------------------------

    /// A line from `start` to `end` with the current attributes; its id.
    pub fn add_line(&mut self, start: Point2, end: Point2) -> EntityId {
        self.document
            .execute(create_line(start, end, EntityAttributes::default()))
            .expect("the line is made");
        self.document.last_created_entities()[0]
    }

    /// Every entity of the drawing, in id order.
    pub fn entities(&self) -> Vec<Entity> {
        let mut out = Vec::new();
        self.document
            .model()
            .entities
            .for_each(|entity| out.push(entity.clone()));
        out
    }

    pub fn entity(&self, id: EntityId) -> Option<&Entity> {
        self.document.model().entities.find(id)
    }

    pub fn undo_steps(&self) -> usize {
        self.document.history().undo_count()
    }

    /// The fixture's polyline's vertices (a Polyline2's).
    pub fn vertices(&self) -> Vec<Point2> {
        match &self
            .entity(self.polyline)
            .expect("the polyline is there")
            .geometry
        {
            Geometry::Polyline(polyline) => polyline.vertices.clone(),
            other => panic!("the fixture's polyline is no plain polyline: {other:?}"),
        }
    }

    // ---- what the view raised --------------------------------------------------------------

    pub fn errors(&self) -> Vec<String> {
        self.log
            .iter()
            .filter_map(|event| match event {
                PlanViewEvent::Error(text) => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    pub fn typed_out(&self) -> Vec<String> {
        self.log
            .iter()
            .filter_map(|event| match event {
                PlanViewEvent::TextTyped(text) => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    pub fn prompts(&self) -> Vec<String> {
        self.log
            .iter()
            .filter_map(|event| match event {
                PlanViewEvent::Prompt(text) => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    pub fn messages(&self) -> Vec<String> {
        self.log
            .iter()
            .filter_map(|event| match event {
                PlanViewEvent::ToolMessage(text) => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    pub fn tool_changes(&self) -> Vec<Tool> {
        self.log
            .iter()
            .filter_map(|event| match event {
                PlanViewEvent::ToolChanged(tool) => Some(*tool),
                _ => None,
            })
            .collect()
    }

    pub fn active_tool_changes(&self) -> Vec<String> {
        self.log
            .iter()
            .filter_map(|event| match event {
                PlanViewEvent::ActiveToolChanged(id) => Some(id.clone()),
                _ => None,
            })
            .collect()
    }

    pub fn menus(&self) -> usize {
        self.log
            .iter()
            .filter(|event| matches!(event, PlanViewEvent::ContextMenu { .. }))
            .count()
    }

    pub fn double_clicked(&self) -> Vec<EntityId> {
        self.log
            .iter()
            .filter_map(|event| match event {
                PlanViewEvent::EntityDoubleClicked(id) => Some(*id),
                _ => None,
            })
            .collect()
    }

    pub fn frame_stats(&self) -> Vec<String> {
        self.log
            .iter()
            .filter_map(|event| match event {
                PlanViewEvent::FrameStats(text) => Some(text.clone()),
                _ => None,
            })
            .collect()
    }
}

/// Whether `pixel` is `colour` laid over the view's ground at a coverage of 40 % or more, within
/// 24 a channel (as the feedback painter's tests judge ink).
pub fn ink_of(pixel: Color, colour: Color) -> bool {
    let ground = palette::viewport();
    let channel = |c: Color| [f64::from(c.r), f64::from(c.g), f64::from(c.b)];
    let (p_raw, c_raw, g_raw) = (channel(pixel), channel(colour), channel(ground));
    let d = [
        c_raw[0] - g_raw[0],
        c_raw[1] - g_raw[1],
        c_raw[2] - g_raw[2],
    ];
    let p = [
        p_raw[0] - g_raw[0],
        p_raw[1] - g_raw[1],
        p_raw[2] - g_raw[2],
    ];
    let t = (p[0] * d[0] + p[1] * d[1] + p[2] * d[2]) / (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]);
    if !(0.4..=1.1).contains(&t) {
        return false;
    }
    (0..3).all(|i| (p[i] - t * d[i]).abs() <= 24.0)
}

/// How many pixels within `radius` of `centre` are inked in `colour`.
pub fn ink_near(image: &Image, centre: PointF, radius: f64, colour: Color) -> i32 {
    let mut count = 0;
    for y in (centre.y - radius) as i32..=(centre.y + radius) as i32 {
        for x in (centre.x - radius) as i32..=(centre.x + radius) as i32 {
            if x >= 0
                && y >= 0
                && x < image.width() as i32
                && y < image.height() as i32
                && (f64::from(x) - centre.x).hypot(f64::from(y) - centre.y) <= radius
                && ink_of(image.pixel(x as u32, y as u32), colour)
            {
                count += 1;
            }
        }
    }
    count
}
