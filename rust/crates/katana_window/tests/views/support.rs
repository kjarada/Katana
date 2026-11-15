//! What the view twins share: a drawing, its views' states and the window's surfaces, lent to a
//! view for each call as the window lends them (`Bench::with`), and the events the C++ tests sent
//! a widget as the view's own methods at a pixel.
//!
//! The C++ tests make a widget over a `ViewState&` of a ViewSet and keep both; a Rust view is lent
//! its state for each call, so the bench keeps the set and lends the state of the view asked for.
//! The window's surfaces point into TIN surfaces the bench owns, so they are made for each call
//! from them (`ground`), as the window hands its views a list over the surfaces it keeps.

#![allow(
    dead_code,
    reason = "Each file of the target uses only some of the bench's helpers."
)]

use katana_cad::Document;
use katana_cad::scene::{SceneMesh, SceneSurface};
use katana_cad::view_set::{ViewId, ViewSet, ViewState};
use katana_cad::viewport_layout::ViewKind;
use katana_commands::{EntityAttributes, create_line};
use katana_geometry::Point2;
use katana_render::{Camera, Rgba};
use katana_terrain::{TinBuildOptions, TinInput, TinSurface, build_tin};
use katana_window::canvas::{Image, PointF};
use katana_window::render_view_widget::{GpuSession, RenderScene, RenderView, RenderViewEvent};
use katana_window::viewport_widget::{KeyModifiers, MouseButton};

/// EXPECT_DOUBLE_EQ: within four units in the last place.
pub fn double_eq(a: f64, b: f64) -> bool {
    if a == b {
        return true;
    }
    if a.is_nan() || b.is_nan() || a.is_sign_negative() != b.is_sign_negative() {
        return false;
    }
    a.to_bits().abs_diff(b.to_bits()) <= 4
}

/// EXPECT_NEAR.
#[track_caller]
pub fn assert_near(actual: f64, expected: f64, tolerance: f64, what: &str) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "{what}: {actual} is not within {tolerance} of {expected}"
    );
}

/// A drawing, its views' states, the window's surfaces and meshes, and the session's renderer
/// rules (see the module comment).
pub struct Bench {
    pub document: Document,
    pub views: ViewSet,
    /// Each a surface named as the C++ test named it, drawn as the window draws it.
    pub ground: Vec<(String, TinSurface)>,
    pub meshes: Vec<SceneMesh<'static>>,
    pub session: GpuSession,
}

impl Default for Bench {
    fn default() -> Self {
        Self::new()
    }
}

impl Bench {
    /// An empty drawing, no surface, a headless session: the software rasteriser draws.
    pub fn new() -> Self {
        Self {
            document: Document::new(None),
            views: ViewSet::new(),
            ground: Vec::new(),
            meshes: Vec::new(),
            session: GpuSession::headless(),
        }
    }

    /// The C++ tests' OneLine: one line from (0, 0) to (100, 50).
    pub fn one_line() -> Self {
        let mut bench = Self::new();
        bench.line(Point2::new(0.0, 0.0), Point2::new(100.0, 50.0));
        bench
    }

    /// A line drawn as createLine draws it: its id.
    pub fn line(&mut self, start: Point2, end: Point2) -> katana_entity::EntityId {
        self.document
            .execute(create_line(start, end, EntityAttributes::default()))
            .expect("the line is made");
        self.document.last_created_entities()[0]
    }

    /// A view of `kind` added to the set: its id.
    pub fn add(&mut self, kind: ViewKind) -> ViewId {
        self.views.add(kind).id
    }

    pub fn state(&self, id: ViewId) -> &ViewState {
        self.views.find(id).expect("the view is open")
    }

    pub fn state_mut(&mut self, id: ViewId) -> &mut ViewState {
        self.views.find_mut(id).expect("the view is open")
    }

    pub fn camera(&self, id: ViewId) -> Camera {
        self.state(id).camera
    }

    /// `call` lent the scene and view `id`'s state, as the window lends them.
    pub fn with<R>(&mut self, id: ViewId, call: impl FnOnce(&RenderScene<'_>, &mut ViewState) -> R) -> R {
        let Self {
            document,
            views,
            ground,
            meshes,
            ..
        } = self;
        let surfaces: Vec<SceneSurface<'_>> = ground
            .iter()
            .map(|(name, surface)| SceneSurface {
                name: name.clone(),
                surface: Some(surface),
                ..SceneSurface::default()
            })
            .collect();
        let scene = RenderScene {
            document,
            surfaces: &surfaces,
            meshes: meshes.as_slice(),
        };
        let state = views.find_mut(id).expect("the view is open");
        call(&scene, state)
    }

    /// A view over view `id`'s state as the workspace builds one, not yet sized or seen.
    pub fn make(&mut self, id: ViewId) -> RenderView {
        let session = self.session.clone();
        let Self {
            document, views, ..
        } = self;
        RenderView::new(document, views.find_mut(id).expect("the view is open"), &session)
    }

    /// The C++ fixtures' `show`: a view as the workspace builds one, resized to 400 x 300 and
    /// seen once.
    pub fn show(&mut self, id: ViewId) -> RenderView {
        let mut view = self.make(id);
        self.resize(&mut view, id, 400, 300);
        self.paint(&mut view, id);
        view
    }

    /// The widget harness's `paint`: the view painted at its size now, as the screen would do it.
    pub fn paint(&mut self, view: &mut RenderView, id: ViewId) {
        self.with(id, |scene, state| {
            let _ = view.paint(scene, state);
        });
    }

    /// QWidget::resize, delivered.
    pub fn resize(&mut self, view: &mut RenderView, id: ViewId, width: i32, height: i32) {
        self.with(id, |scene, state| view.resize(scene, state, width, height));
    }

    pub fn zoom_extents(&mut self, view: &mut RenderView, id: ViewId) {
        self.with(id, |scene, state| view.zoom_extents(scene, state));
    }

    /// A wheel event of `eighths` eighths of a degree at `at`.
    pub fn turn_wheel(&mut self, view: &mut RenderView, id: ViewId, at: PointF, eighths: i32) -> bool {
        self.with(id, |scene, state| {
            view.wheel(scene, state, at, f64::from(eighths))
        })
    }

    /// A press with `button` at `from`, a move to `to` holding it, and the release there.
    pub fn drag(&mut self, view: &mut RenderView, id: ViewId, from: PointF, to: PointF, button: MouseButton) {
        self.with(id, |scene, state| {
            view.mouse_press(scene, state, from, button, KeyModifiers::NONE);
            view.mouse_move(scene, state, to);
            view.mouse_release();
        });
    }

    pub fn middle_drag(&mut self, view: &mut RenderView, id: ViewId, from: PointF, to: PointF) {
        self.drag(view, id, from, to, MouseButton::Middle);
    }

    pub fn left_drag(&mut self, view: &mut RenderView, id: ViewId, from: PointF, to: PointF) {
        self.drag(view, id, from, to, MouseButton::Left);
    }
}

/// Flat ground 200 m square at `height`, from an 11 x 11 grid of points 20 m apart.
pub fn flat_ground(height: f64) -> TinSurface {
    let mut input = TinInput::default();
    for j in 0..=10 {
        for i in 0..=10 {
            input.points.push(katana_geometry::Point3::new(
                20.0 * f64::from(i),
                20.0 * f64::from(j),
                height,
            ));
        }
    }
    build_tin(&input, &TinBuildOptions::default())
        .unwrap_or_else(|error| panic!("{}", error.describe()))
        .surface
}

/// The frame stats the view raised, in order.
pub fn frame_stats(events: &[RenderViewEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| match event {
            RenderViewEvent::FrameStats(text) => Some(text.clone()),
            _ => None,
        })
        .collect()
}

/// What the view said on the status line, in order.
pub fn statuses(events: &[RenderViewEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| match event {
            RenderViewEvent::Status(text) => Some(text.clone()),
            _ => None,
        })
        .collect()
}

/// Pixels of an image that are not `background` (its red, green and blue).
pub fn drawn_pixels(image: &Image, background: Rgba) -> usize {
    let (r, g, b) = (
        katana_render::red_of(background),
        katana_render::green_of(background),
        katana_render::blue_of(background),
    );
    let mut drawn = 0;
    for y in 0..image.height() {
        for x in 0..image.width() {
            let pixel = image.pixel(x, y);
            if (pixel.r, pixel.g, pixel.b) != (r, g, b) {
                drawn += 1;
            }
        }
    }
    drawn
}

/// Whether anything but `background` is drawn within `radius` pixels of `at`, a point in the
/// image's own (device) pixels.
pub fn drawn_near(image: &Image, at: PointF, radius: i32, background: Rgba) -> bool {
    let (r, g, b) = (
        katana_render::red_of(background),
        katana_render::green_of(background),
        katana_render::blue_of(background),
    );
    let cx = at.x.floor() as i32;
    let cy = at.y.floor() as i32;
    for y in cy - radius..=cy + radius {
        for x in cx - radius..=cx + radius {
            if x >= 0 && y >= 0 && (x as u32) < image.width() && (y as u32) < image.height() {
                let pixel = image.pixel(x as u32, y as u32);
                if (pixel.r, pixel.g, pixel.b) != (r, g, b) {
                    return true;
                }
            }
        }
    }
    false
}

/// With KATANA_RENDER_VIEW_IMAGES (or, for a GPU frame, KATANA_GPU_TEST_IMAGES) set to a folder,
/// saves `image` there as <name>.png: a zoom looked at rather than read in numbers.
pub fn save_for_looking(variable: &str, name: &str, image: &Image) {
    let Some(directory) = std::env::var_os(variable) else {
        return;
    };
    if directory.is_empty() || image.is_null() {
        return;
    }
    let directory = std::path::PathBuf::from(directory);
    if std::fs::create_dir_all(&directory).is_ok() {
        let _ = katana_window::canvas::write_png(image, &directory.join(format!("{name}.png")));
    }
}
