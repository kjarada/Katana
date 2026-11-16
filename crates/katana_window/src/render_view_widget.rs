//! The 3D and elevation view: the port of render_view_widget.
//!
//! Rule 3 again: this view owns a camera's framebuffer and NOTHING else. It asks katana_cad to
//! turn the document into layers of draw lists (SceneBuilder), hands them to a renderer, and
//! shows the result. Every geometric decision - what to draw, how an arc is chorded, where the
//! scene's bounds are, what a drag does to a camera - lives in a tested library below it.
//!
//! TWO RENDERERS (docs/gpu.md, "Fallback"). Where katana_gpu's choose_renderer says so - a
//! desktop platform, the GPU renderer built in, nothing against it - the view is drawn by a GPU
//! scene view (gpu_scene_view): the same layers, the same depth rules (render_layers,
//! GpuRenderer::set_layers), the same camera and the same mouse. Everywhere else, and after the
//! GPU view fails once in a session, the software rasteriser draws it; it is also what every
//! headless run and test uses. The legend and the empty message are painted over either.
//!
//! THE RUST SHAPE, as the plan view's (viewport_widget):
//!
//! * **Every call is lent what it works on**: the scene (`RenderScene`: the document, and the
//!   window's surfaces and meshes, which belong to the window and are one list for every view)
//!   and the view's `ViewState`, which belongs to the workspace's ViewSet and outlives the view -
//!   the camera and the layers hidden in this view live there, so changing the view's kind and
//!   back, or floating its dock, resets neither.
//! * **The events are methods** in the view's own logical pixels, and `ui` is the adaptor from
//!   egui. What the C++ raised through hooks is queued (`RenderViewEvent`, `take_events`).
//! * **The GPU child is lent its host** for each call (`GpuSceneHost`): the C++ child called its
//!   host back through hooks, one of them in the middle of a frame; here the view takes its child
//!   out for the call and lends it the rest of itself.
//! * **A session's GPU failure is the session's**, a flag every view made from one `GpuSession`
//!   shares, where the C++ kept it in a process-wide static: the tests run in threads of one
//!   process, and one case's failed GPU must not turn another's to software.
//!
//! The document is heard as the plan view hears it: a listener counts notifications and the view
//! looks at the next call - a model revision that moved rebuilds the drawing, any notification
//! the selection overlay - which is the C++'s order, a burst of changes folded into one rebuild.

#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "Keep the C++'s negated comparisons, which refuse NaN."
)]

use std::borrow::Cow;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use katana_cad::scene::{
    SceneBuilder, SceneLayers, SceneMesh, SceneOptions, SceneSurface, elevation_ramp_color,
    render_layers, scene_bounds, scene_depth_box,
};
use katana_cad::scene_zoom::zoom_at_pixel;
use katana_cad::view_set::ViewState;
use katana_cad::viewport_layout::ViewKind;
use katana_cad::{Document, ListenerHandle};
use katana_entity::EntityId;
use katana_entity::entity::to_string;
use katana_gpu::{
    GpuBackend, GpuContext, LayerSource, ORIGIN_ERROR_PIXELS, RendererDecision,
    RendererEnvironment, choose_scene_origin, current_renderer_environment, origin_error_pixels,
};
use katana_math::{AABB, Vec3};
use katana_render::{
    Camera, DrawList, Framebuffer, Projection, Rasterizer, RenderOptions, RenderStats, Rgba,
    StandardView, blue_of, green_of, red_of, rgba,
};

use crate::canvas::{
    Brush, Canvas, Color, Font, Image, LinearGradient, Pen, PointF, RasterCanvas, RectF,
    TextFlags,
};
use crate::drive::controls;
use crate::drive::registry::{ControlKind, ControlRecord};
use crate::format::qt_fixed;
use crate::gpu_scene_view::{
    GpuSceneEvent, GpuSceneHost, GpuSceneView, SceneKey, image_of, make_gpu_scene_view_if_chosen,
    toggled,
};
use crate::terrain::surface_plan_painter::{DrawnLegend, LegendStack, paint_surface_legend_stack};
use crate::theme::palette;
use crate::theme::style::ui_font;
use crate::view_focus::{FocusPrevious, FocusReason, focus_chooses_view};
use crate::viewport_widget::{KeyModifiers, MouseButton};

/// Radians per pixel of drag. A full turn in roughly 800 pixels, which is about a screen width
/// and matches what every other CAD package feels like.
pub const ORBIT_PER_PIXEL: f64 = 0.008;

/// The ground behind the model, and behind the empty view's message: the plan's (the viewport
/// token), so every view of the workspace stands on one ground. It was (28, 30, 36), and in Quad
/// the plan, the 3D and the section were three darks side by side (the third look audit's H).
pub const BACKGROUND: Color = palette::viewport();
const BACKGROUND_RGBA: Rgba = rgba(BACKGROUND.r, BACKGROUND.g, BACKGROUND.b);

/// The smallest the view may be made (QWidget::setMinimumSize(40, 40)).
pub const MINIMUM_SIZE: i32 = 40;

/// A widget's size before anything has sized it: Qt's, for a widget never shown.
const UNSIZED_WIDTH: i32 = 640;
const UNSIZED_HEIGHT: i32 = 480;

/// The layers in render_layers' order, with its depth rules (scene.rs: the grid, the edges and
/// the selection's casing test depth and write none).
const GRID_LAYER: usize = 0;
const EDGES_LAYER: usize = 2;
const ENTITIES_LAYER: usize = 3;
const SELECTION_CASING_LAYER: usize = 4;
const SELECTION_LAYER: usize = 5;

/// What the empty view says: what would put something there.
pub const EMPTY_MESSAGE: &str = "Nothing to show in 3D yet.\nDraw or import something, or build \
                                 a surface from the Terrain menu.";

/// What the zoom says when it stops at its limit.
pub const ZOOM_LIMIT_MESSAGE: &str = "3D view: zoomed in as far as it can be drawn exactly";

/// The double click's time and distance, as the plan view reads them from raw presses.
const DOUBLE_CLICK_SECONDS: f64 = 0.4;
const DOUBLE_CLICK_PIXELS: f64 = 5.0;
/// Points of egui's smooth scroll that make one notch.
const WHEEL_POINTS_A_NOTCH: f64 = 50.0;

/// The platform name the build's GPU view can be shown on, in renderer_choice's vocabulary: what
/// the window says its desktop is.
pub fn desktop_platform() -> &'static str {
    match GpuBackend::built() {
        GpuBackend::Direct3D12 => "windows",
        GpuBackend::Vulkan => "xcb",
        GpuBackend::Metal => "cocoa",
    }
}

/// What the window's views share about the GPU: the platform they are shown on, the device the
/// window drew with, the KATANA_RENDERER the session reads, and whether a GPU view has failed in
/// this session (renderer_choice's rule 4), a flag every clone of one session shares.
#[derive(Clone, Debug)]
pub struct GpuSession {
    /// renderer_choice's platform name: "offscreen" for a headless run.
    pub platform: String,
    /// The device a GPU view draws on: the window's own (GpuContext::from_parts), or none.
    pub device: Option<GpuContext>,
    /// KATANA_RENDERER as this session reads it; None reads the process environment, as the C++
    /// read it. A test sets it where ctest set the variable for the whole process.
    pub renderer_override: Option<String>,
    failed: Rc<Cell<bool>>,
}

impl GpuSession {
    /// A headless run's: the offscreen platform and no device, so every view is drawn by the
    /// software rasteriser.
    pub fn headless() -> Self {
        Self::on("offscreen", None)
    }

    /// A session on `platform` with `device`.
    pub fn on(platform: &str, device: Option<GpuContext>) -> Self {
        Self {
            platform: String::from(platform),
            device,
            renderer_override: None,
            failed: Rc::new(Cell::new(false)),
        }
    }

    /// The desktop the build's GPU view is shown on, with `device`.
    pub fn desktop(device: Option<GpuContext>) -> Self {
        Self::on(desktop_platform(), device)
    }

    /// Whether a GPU view has failed in this session.
    pub fn failed(&self) -> bool {
        self.failed.get()
    }

    /// A GPU view failed: no view of the session tries the GPU again.
    pub fn set_failed(&self) {
        self.failed.set(true);
    }

    /// The environment the renderer is chosen by (current_renderer_environment).
    pub fn environment(&self) -> RendererEnvironment {
        let mut environment = current_renderer_environment(false, self.failed(), &self.platform);
        if let Some(value) = &self.renderer_override {
            environment.renderer_override = String::from(value.trim());
        }
        environment
    }
}

/// What a view draws from, lent for a call: the document, and the window's surfaces and meshes -
/// one list for every view, made with the drawing's own hidden layers.
#[derive(Clone, Copy)]
pub struct RenderScene<'a> {
    pub document: &'a Document,
    pub surfaces: &'a [SceneSurface<'a>],
    pub meshes: &'a [SceneMesh<'a>],
}

impl<'a> RenderScene<'a> {
    /// The document alone: no surfaces and no meshes.
    pub fn of(document: &'a Document) -> Self {
        Self {
            document,
            surfaces: &[],
            meshes: &[],
        }
    }
}

/// What the C++ view raised through its hooks, in order.
#[derive(Clone, Debug, PartialEq)]
pub enum RenderViewEvent {
    /// The view was clicked, or the user moved the keyboard focus into it, so the workspace can
    /// make it active; the workspace ignores re-activating the active view.
    Activated,
    /// A message the user must see: a failure, never a statistic (onStatus).
    Status(String),
    /// What each frame drew and how long it took ("3D  1916 tri  1.6 ms"), on every paint
    /// (onFrameStats).
    FrameStats(String),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Drag {
    #[default]
    None,
    Orbit,
    Pan,
}

/// The egui adaptor's own state.
#[derive(Default)]
struct EguiSide {
    texture: Option<egui::TextureHandle>,
    held: Vec<MouseButton>,
    last_press: Option<(f64, PointF, MouseButton)>,
}

/// See the module comment.
pub struct RenderView {
    // First, so it is dropped first: nothing counts into a half-dropped view.
    registration: ListenerHandle,
    /// Counted by the document listener; the view looks at it at its next call.
    notifications: Arc<AtomicU64>,
    seen_notifications: u64,
    /// The document listened to, by address: setting the same one again is no change.
    listened: usize,
    name: String,
    width: i32,
    height: i32,
    device_ratio: f64,
    /// The scene options but the layers, which are the state's and given at each build.
    options: SceneOptions<'static>,

    builder: SceneBuilder,
    layers: SceneLayers,
    rasterizer: Rasterizer,
    framebuffer: Framebuffer,
    /// The last paint whole: the frame with the legend and the empty message over it.
    picture: Image,
    stats: RenderStats,
    last_frame_ms: f64,
    last_build_ms: f64,
    /// What needs building before the next paint, layer by layer; each one dirties the layers
    /// built from it (rebuild_if_needed).
    terrain_dirty: bool,
    entities_dirty: bool,
    selection_dirty: bool,
    terrain_builds: i32,
    entity_builds: i32,
    selection_builds: i32,
    /// The drawing's revision the entities were built from.
    built_revision: u64,
    /// Until the user moves the camera, a resize frames the scene again: the first frame is often
    /// made before the dock has its real size.
    refit_on_resize: bool,
    scene_empty: bool,
    empty_message_shown: bool,
    drawn_legends: Vec<DrawnLegend>,
    /// The first paint frames the scene unless the camera was framed already
    /// (ViewState::camera_framed): `new` sets this from the state.
    framed: bool,
    /// The last frame was of an empty scene, so it framed a patch of ground round the origin. The
    /// first rebuild that finds something to show frames that instead: a line drawn at survey
    /// coordinates would otherwise be kilometres outside a view that kept looking at the origin.
    framed_empty: bool,
    /// A repaint asked for (QWidget::update).
    dirty: bool,

    drag: Drag,
    last_mouse: PointF,

    /// The GPU child while there is one, and taken out while it is lent its host.
    gpu: Option<GpuSceneView>,
    /// True while the GPU view draws this one, whether or not it is lent out now.
    on_gpu: bool,
    session: GpuSession,
    renderer_reason: String,
    /// A GPU failure heard, to drop the GPU view at the next turn of the window's loop, after the
    /// failing call returned (the C++'s QTimer::singleShot(0)).
    pending_failure: Option<String>,
    /// What the GPU view has not been given since the last rebuild: every layer (the terrain was
    /// rebuilt); the drawing's - grid, entities and selection, the terrain's being unchanged; or
    /// the selection alone. And whether it last drew the edges.
    gpu_layers_stale: bool,
    gpu_drawing_stale: bool,
    gpu_selection_stale: bool,
    gpu_edges_shown: bool,
    rebuilt_since_stats: bool,
    /// The origin the GPU view's layers are packed against once a deep zoom moved it to the pivot
    /// (prepare_gpu_frame); until then, none: the renderer's own choice, the centre of the scene.
    gpu_origin: Option<Vec3>,

    events: Vec<RenderViewEvent>,
    egui: EguiSide,
}

/// The view lent to its GPU child as its host for one call (see the module comment).
struct Hosting<'h, 'a> {
    view: &'h mut RenderView,
    scene: &'h RenderScene<'a>,
    state: &'h mut ViewState,
}

impl GpuSceneHost for Hosting<'_, '_> {
    fn camera(&mut self) -> &mut Camera {
        &mut self.state.camera
    }

    /// The host frames: it knows what "the scene" is, and frames what it built.
    fn zoom_extents(&mut self, view: &mut GpuSceneView) -> bool {
        self.view
            .zoom_extents_on(Some(view), self.scene, self.state);
        true
    }

    /// And zooms, for the same reason: towards what it built under the cursor.
    fn wheel_zoom(&mut self, view: &mut GpuSceneView, notches: f64, at: PointF) -> bool {
        self.view
            .zoom_at_pixel_on(Some(view), self.state, notches, at);
        true
    }

    fn prepare_frame(&mut self, view: &mut GpuSceneView, frame_camera: &mut Camera) {
        self.view
            .prepare_gpu_frame(view, self.scene, self.state, frame_camera);
    }
}

impl RenderView {
    /// A view of `document` through `state`, its renderer chosen by `session`'s rules.
    pub fn new(document: &Document, state: &mut ViewState, session: &GpuSession) -> Self {
        let notifications = Arc::new(AtomicU64::new(0));
        let mut view = Self {
            registration: ListenerHandle::default(),
            notifications,
            seen_notifications: 0,
            listened: 0,
            // As its dock is View<id>: what a headless run's --wheel and --report name it by
            // (docs/headless.md).
            name: format!("RenderView{}", state.id),
            width: UNSIZED_WIDTH,
            height: UNSIZED_HEIGHT,
            device_ratio: 1.0,
            options: SceneOptions::default(),
            builder: SceneBuilder::new(),
            layers: SceneLayers::default(),
            rasterizer: Rasterizer::default(),
            framebuffer: Framebuffer::default(),
            picture: Image::default(),
            stats: RenderStats::default(),
            last_frame_ms: 0.0,
            last_build_ms: 0.0,
            terrain_dirty: true,
            entities_dirty: true,
            selection_dirty: true,
            terrain_builds: 0,
            entity_builds: 0,
            selection_builds: 0,
            built_revision: 0,
            refit_on_resize: false,
            scene_empty: true,
            empty_message_shown: false,
            drawn_legends: Vec::new(),
            // A view rebuilt over a camera that is already framed - the view went to a section
            // and came back - must not frame it again at its first paint: that reset the target
            // and the distance the user had zoomed to. camera_kind is checked as well so that a
            // state whose flag outlived a reconfiguration still frames.
            framed: state.camera_framed && state.camera_kind == state.kind,
            framed_empty: false,
            dirty: true,
            drag: Drag::None,
            last_mouse: PointF::default(),
            gpu: None,
            on_gpu: false,
            session: session.clone(),
            renderer_reason: String::new(),
            pending_failure: None,
            gpu_layers_stale: true,
            gpu_drawing_stale: false,
            gpu_selection_stale: false,
            gpu_edges_shown: false,
            rebuilt_since_stats: false,
            gpu_origin: None,
            events: Vec::new(),
            egui: EguiSide::default(),
        };
        view.listen_to(document);
        view.make_gpu_view(state);
        view
    }

    /// Registers with `document`; a no-op when it is the document already listened to.
    fn listen_to(&mut self, document: &Document) {
        let address = std::ptr::from_ref(document) as usize;
        if address == self.listened && self.registration.active() {
            return;
        }
        self.listened = address;
        // Every edit, undo, layer change and selection change: the 3D view was rebuilt only when
        // something else happened to invalidate it, so a line drawn in plan never appeared here
        // and an erased one stayed (audit QT-05). The rebuild is lazy - this counts, and the
        // next look marks the scene dirty.
        let counter = Arc::clone(&self.notifications);
        self.registration = document.add_listener(move || {
            counter.fetch_add(1, Ordering::AcqRel);
        });
        self.seen_notifications = self.notifications.load(Ordering::Acquire);
        self.built_revision = document.model_revision();
    }

    /// What the document said since the view last looked: the drawing changed (its revision
    /// moved), or only something else did - the selection, the current layer.
    fn look(&mut self, document: &Document) {
        let heard = self.notifications.load(Ordering::Acquire);
        if heard == self.seen_notifications {
            return;
        }
        self.seen_notifications = heard;
        // The revision moves for every command, undo, redo and opened drawing and for nothing
        // else (Document::model_revision), so a notification that leaves it where it was is a
        // selection or a current-layer change: only the overlay depends on that. A click used to
        // rebuild the whole scene - 60 to 115 ms on a 229k-triangle surface - to recolour one
        // line.
        if document.model_revision() != self.built_revision {
            self.entities_dirty = true;
        }
        self.selection_dirty = true;
        self.dirty = true;
    }

    // ---- the GPU child ------------------------------------------------------------------------

    /// The GPU child, when the renderer rules choose it.
    fn make_gpu_view(&mut self, state: &mut ViewState) {
        let mut decision = RendererDecision::default();
        let made = make_gpu_scene_view_if_chosen(&self.session.environment(), Some(&mut decision));
        self.renderer_reason = decision.reason;
        let Some(mut view) = made else {
            return;
        };
        view.set_device(self.session.device.clone());
        view.set_device_ratio(self.device_ratio);
        view.resize(&mut state.camera, self.width, self.height);
        // The software view's own test (new): a camera already framed for this kind of view is
        // kept through the first frame.
        view.set_camera_framed(self.framed);
        view.set_orbit_allowed(state.kind != ViewKind::Elevation);
        self.gpu = Some(view);
        self.on_gpu = true;
        // Widths in logical pixels from now on (scene_scale), so the scene is rebuilt for the GPU
        // on its first frame.
        self.terrain_dirty = true;
        self.entities_dirty = true;
        self.selection_dirty = true;
    }

    /// Back to the software rasteriser for the rest of the session, saying why.
    fn drop_gpu_view(&mut self, reason: &str) {
        if !self.on_gpu {
            return;
        }
        self.gpu = None;
        self.on_gpu = false;
        self.renderer_reason =
            format!("the GPU renderer failed ({reason}); the software renderer draws");
        self.events.push(RenderViewEvent::Status(format!(
            "3D view: {}",
            self.renderer_reason
        )));
        // The software view counts the camera in device pixels, and builds widths for them:
        // rebuild_if_needed sees the scale change.
        self.gpu_layers_stale = true;
        self.dirty = true;
    }

    /// What the window's event loop delivers at its next turn: a GPU failure heard during a call
    /// drops the GPU view now, after that call returned (QApplication::processEvents).
    pub fn deliver(&mut self) {
        if let Some(gpu) = &mut self.gpu {
            let failures: Vec<String> = gpu
                .take_events()
                .into_iter()
                .filter_map(|event| match event {
                    GpuSceneEvent::RenderFailed(why) => Some(why),
                    GpuSceneEvent::Activated => {
                        self.events.push(RenderViewEvent::Activated);
                        None
                    }
                    GpuSceneEvent::FrameStats(_) => None,
                })
                .collect();
            for why in failures {
                self.session.set_failed();
                self.pending_failure.get_or_insert(why);
            }
        }
        if let Some(why) = self.pending_failure.take() {
            self.drop_gpu_view(&why);
        }
    }

    /// What the GPU child raised during the call just made, as the C++ hooks acted on it.
    fn take_gpu_events(&mut self, kind: ViewKind) {
        let Some(gpu) = &mut self.gpu else {
            return;
        };
        for event in gpu.take_events() {
            match event {
                GpuSceneEvent::Activated => self.events.push(RenderViewEvent::Activated),
                GpuSceneEvent::FrameStats(text) => {
                    let mut line = format!("{}  {text}", to_string(kind));
                    if self.rebuilt_since_stats {
                        line.push_str(&format!(" (scene {} ms)", qt_fixed(self.last_build_ms, 1)));
                        self.rebuilt_since_stats = false;
                    }
                    self.events.push(RenderViewEvent::FrameStats(line));
                }
                GpuSceneEvent::RenderFailed(why) => {
                    self.session.set_failed();
                    self.pending_failure.get_or_insert(why);
                }
            }
        }
    }

    /// `call` made on the GPU child with this view lent as its host; None when there is none.
    fn on_gpu_view<R>(
        &mut self,
        scene: &RenderScene<'_>,
        state: &mut ViewState,
        call: impl FnOnce(&mut GpuSceneView, &mut dyn GpuSceneHost) -> R,
    ) -> Option<R> {
        let mut gpu = self.gpu.take()?;
        let result = {
            let mut hosting = Hosting {
                view: &mut *self,
                scene,
                state: &mut *state,
            };
            call(&mut gpu, &mut hosting)
        };
        if self.on_gpu {
            self.gpu = Some(gpu);
        }
        self.take_gpu_events(state.kind);
        Some(result)
    }

    /// `call` made with the GPU child, if any, lent beside this view.
    fn with_gpu<R>(&mut self, call: impl FnOnce(&mut Self, Option<&mut GpuSceneView>) -> R) -> R {
        let mut gpu = self.gpu.take();
        let result = call(self, gpu.as_mut());
        if self.on_gpu && self.gpu.is_none() {
            self.gpu = gpu;
        }
        result
    }

    /// What the C++'s cad::renderLayers does before drawing, for the GPU's frame: the scene rebuilt
    /// if it is dirty, the depth range fitted, the edges faded for the frame's scale, and whatever
    /// changed handed to the GPU view.
    fn prepare_gpu_frame(
        &mut self,
        gpu: &mut GpuSceneView,
        scene: &RenderScene<'_>,
        state: &mut ViewState,
        frame_camera: &mut Camera,
    ) {
        let rebuilt = self.terrain_dirty || self.entities_dirty || self.selection_dirty;
        self.rebuild_if_needed(Some(&mut *gpu), scene, state);
        if rebuilt {
            self.rebuilt_since_stats = true;
            self.dirty = true; // the legend and the empty message follow the scene
        }
        // The rebuild may have framed the host's camera (a first scene with something in it):
        // the frame is drawn through the camera as it is now.
        let width = frame_camera.viewport_width();
        let height = frame_camera.viewport_height();
        *frame_camera = state.camera;
        frame_camera.set_viewport_size(width, height);
        gpu.set_orbit_allowed(state.kind != ViewKind::Elevation);

        // As render_layers: the depth range fitted to what is drawn, every frame, and the edges
        // faded for how large the triangles are now.
        let _ = frame_camera.fit_depth_range(scene_depth_box(&self.layers));
        let applied: Vec<f32> = self.layers.edge_runs.iter().map(|run| run.applied).collect();
        let draw_edges = SceneBuilder::fade_edges(&mut self.layers, frame_camera);
        let mut edges_changed = draw_edges != self.gpu_edges_shown;
        for (before, run) in applied.iter().zip(&self.layers.edge_runs) {
            if edges_changed {
                break;
            }
            edges_changed = *before != run.applied;
        }
        // The GPU draws float offsets from the origin its layers are packed against, which zoomed
        // in close to a point far from it round to more than the view can hide: every layer is
        // packed again against the pivot once the bound there passes ORIGIN_ERROR_PIXELS
        // (scene_origin, "The origin follows a deep zoom"). Only then - a repack is a pass over
        // every vertex. The origin checked is the one the layers will have: a stale scene is
        // packed next against its own centre, unless one was chosen.
        let origin = match self.gpu_origin {
            Some(origin) => origin,
            None if self.gpu_layers_stale => choose_scene_origin(scene_depth_box(&self.layers)),
            None => gpu.scene_origin(),
        };
        if origin_error_pixels(frame_camera, origin) > ORIGIN_ERROR_PIXELS {
            self.gpu_origin = Some(frame_camera.target());
            self.gpu_layers_stale = true;
        }
        self.send_layers_to_gpu(gpu, draw_edges, edges_changed);
    }

    /// Hands the GPU view what it has not been given since the last rebuild (the stale flags),
    /// the edges drawn or not.
    fn send_layers_to_gpu(&mut self, gpu: &mut GpuSceneView, draw_edges: bool, edges_changed: bool) {
        let nothing = DrawList::default();
        let layers = &self.layers;
        let edges = if draw_edges { &layers.edges } else { &nothing };
        if self.gpu_layers_stale {
            let sources = [
                LayerSource {
                    list: Some(&layers.grid),
                    depth_write: false,
                },
                LayerSource {
                    list: Some(&layers.terrain),
                    depth_write: true,
                },
                LayerSource {
                    list: Some(edges),
                    depth_write: false,
                },
                LayerSource {
                    list: Some(&layers.entities),
                    depth_write: true,
                },
                LayerSource {
                    list: Some(&layers.selection_casing),
                    depth_write: false,
                },
                LayerSource {
                    list: Some(&layers.selection),
                    depth_write: true,
                },
            ];
            gpu.set_layers(&sources, self.gpu_origin);
        } else {
            // An edit of the drawing over a large surface re-sends the drawing alone, against the
            // origin the terrain set, as the software view rebuilds only what changed.
            if self.gpu_drawing_stale {
                gpu.update_layer(GRID_LAYER, &layers.grid);
                gpu.update_layer(ENTITIES_LAYER, &layers.entities);
            }
            if self.gpu_drawing_stale || self.gpu_selection_stale {
                gpu.update_layer(SELECTION_CASING_LAYER, &layers.selection_casing);
                gpu.update_layer(SELECTION_LAYER, &layers.selection);
            }
            if edges_changed {
                gpu.update_layer(EDGES_LAYER, edges);
            }
        }
        self.gpu_layers_stale = false;
        self.gpu_drawing_stale = false;
        self.gpu_selection_stale = false;
        self.gpu_edges_shown = draw_edges;
    }

    // ---- what the view is ---------------------------------------------------------------------

    /// The view's object name, "RenderView<id>".
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The view's size in its own logical pixels.
    pub fn size(&self) -> (i32, i32) {
        (self.width, self.height)
    }

    /// What the view has raised since the last call of this, in order.
    pub fn take_events(&mut self) -> Vec<RenderViewEvent> {
        std::mem::take(&mut self.events)
    }

    /// Lines in the last scene built, every layer counted: the headless checks' proof that a
    /// layer hidden in this view left it, and that an edit reached this view.
    pub fn last_scene_line_count(&self) -> usize {
        let layers = &self.layers;
        layers.grid.lines.len()
            + layers.terrain.lines.len()
            + layers.edges.lines.len()
            + layers.entities.lines.len()
            + layers.selection_casing.lines.len()
            + layers.selection.lines.len()
    }

    /// The scene as last built, by layer.
    pub fn scene_layers(&self) -> &SceneLayers {
        &self.layers
    }

    /// The legends the last paint stacked down the top-right corner - one for each visible
    /// surface drawn in a style that colours its faces, in drawing order, as many as the view has
    /// room for - each with its caption ("surface - style") and where its panel lies. Empty when
    /// none was drawn, and then the one elevation ramp's bar is, if a surface has it.
    pub fn drawn_legends(&self) -> &[DrawnLegend] {
        &self.drawn_legends
    }

    /// How many times each layer has been built: the proof that a selection click rebuilds the
    /// overlay and nothing under it.
    pub fn terrain_builds(&self) -> i32 {
        self.terrain_builds
    }

    pub fn entity_builds(&self) -> i32 {
        self.entity_builds
    }

    pub fn selection_builds(&self) -> i32 {
        self.selection_builds
    }

    /// The software framebuffer the last paint rendered, in device pixels.
    pub fn framebuffer(&self) -> &Framebuffer {
        &self.framebuffer
    }

    /// The last paint whole, the overlays over the frame: what the screen shows.
    pub fn picture(&self) -> &Image {
        &self.picture
    }

    /// True when the last scene built had nothing to show - no drawn entity, surface or mesh, only
    /// the grid. What the framing goes by.
    pub fn scene_empty(&self) -> bool {
        self.scene_empty
    }

    /// True when the last paint told the user there is nothing to show and what would put
    /// something there: only for an empty scene of a drawing with nothing in it, and only where
    /// the view had room to say it.
    pub fn empty_message_shown(&self) -> bool {
        self.empty_message_shown
    }

    /// Milliseconds the last paint took - any scene build it did, the render and the blit - and
    /// what it contained.
    pub fn last_frame_milliseconds(&self) -> f64 {
        self.last_frame_ms
    }

    pub fn last_build_milliseconds(&self) -> f64 {
        self.last_build_ms
    }

    pub fn last_stats(&self) -> &RenderStats {
        &self.stats
    }

    /// True while the GPU view draws this one; false for the software rasteriser.
    pub fn drawn_on_gpu(&self) -> bool {
        self.on_gpu
    }

    /// The GPU child while there is one: the tests' way to its frame.
    pub fn gpu_view(&self) -> Option<&GpuSceneView> {
        self.gpu.as_ref()
    }

    pub fn gpu_view_mut(&mut self) -> Option<&mut GpuSceneView> {
        self.gpu.as_mut()
    }

    /// Why this renderer, in one sentence (RendererDecision::reason, or the failure that sent it
    /// back to software).
    pub fn renderer_reason(&self) -> &str {
        &self.renderer_reason
    }

    /// The device pixels a logical pixel is (QWidget::devicePixelRatioF).
    pub fn device_ratio(&self) -> f64 {
        self.device_ratio
    }

    pub fn set_device_ratio(&mut self, ratio: f64) {
        if ratio.is_finite() && ratio > 0.0 && ratio != self.device_ratio {
            self.device_ratio = ratio;
            if let Some(gpu) = &mut self.gpu {
                gpu.set_device_ratio(ratio);
            }
            self.dirty = true;
        }
    }

    /// The scene options, for what the window sets (the grid, the linework heights); the layers
    /// are always this view's own.
    pub fn scene_options(&self) -> &SceneOptions<'static> {
        &self.options
    }

    pub fn vertical_exaggeration(&self) -> f64 {
        self.options.vertical_exaggeration
    }

    /// Takes the document and the options but the layers: those are this view's own, and the
    /// workspace hands every render view the same context (setContext).
    pub fn set_context(&mut self, document: &Document, options: SceneOptions<'static>) {
        self.options = SceneOptions {
            layers: None,
            ..options
        };
        self.listen_to(document);
        self.invalidate_scene();
    }

    /// Rebuilds the scene on the next paint. The view looks at every document change itself;
    /// call it for what the document does not announce - surfaces, meshes, the scene options,
    /// this view's own hidden layers.
    pub fn invalidate_scene(&mut self) {
        self.terrain_dirty = true;
        self.entities_dirty = true;
        self.selection_dirty = true;
        self.request_frame_now();
    }

    /// A repaint by whichever renderer draws the view.
    fn request_frame(&mut self, gpu: Option<&mut GpuSceneView>) {
        if let Some(gpu) = gpu {
            gpu.update();
        }
        self.dirty = true;
    }

    fn request_frame_now(&mut self) {
        if let Some(gpu) = &mut self.gpu {
            gpu.update();
        }
        self.dirty = true;
    }

    /// Whether a frame has been asked for since the last paint.
    pub fn needs_frame(&self) -> bool {
        self.dirty || self.gpu.as_ref().is_some_and(GpuSceneView::needs_frame)
    }

    /// Device pixels per logical pixel the scene's widths are built for: the display's for the
    /// software framebuffer, 1 for the GPU view, which scales logical widths itself
    /// (FrameSettings::pixel_ratio).
    fn scene_scale(&self) -> f64 {
        if self.on_gpu { 1.0 } else { self.pixel_ratio() }
    }

    fn pixel_ratio(&self) -> f64 {
        if self.device_ratio.is_finite() && self.device_ratio > 0.0 {
            self.device_ratio
        } else {
            1.0
        }
    }

    /// The window's meshes less those on a layer THIS view hides (ViewState::layers): the
    /// window's list is one for every view, made with the drawing's own hidden layers, so a view
    /// that hid "road/kerb" for itself still drew the kerbs. The list itself when this view hides
    /// none of them, so the usual case copies nothing.
    fn shown_meshes<'m>(
        meshes: &'m [SceneMesh<'m>],
        state: &ViewState,
    ) -> Cow<'m, [SceneMesh<'m>]> {
        let hidden = &state.layers;
        let hidden_here =
            |mesh: &SceneMesh<'_>| !mesh.layer.is_empty() && hidden.hides(&mesh.layer);
        if hidden.empty() || !meshes.iter().any(hidden_here) {
            return Cow::Borrowed(meshes);
        }
        Cow::Owned(
            meshes
                .iter()
                .filter(|&mesh| !hidden_here(mesh))
                .cloned()
                .collect(),
        )
    }

    /// The options with this view's hidden layers.
    fn options_for<'s>(&self, state: &'s ViewState) -> SceneOptions<'s> {
        let mut options: SceneOptions<'s> = self.options;
        options.layers = Some(&state.layers);
        options
    }

    // ---- the camera ---------------------------------------------------------------------------

    pub fn set_standard_view(&mut self, state: &mut ViewState, view: StandardView) {
        state.camera.set_standard_view(view);
        self.request_frame_now();
    }

    pub fn set_projection(&mut self, state: &mut ViewState, projection: Projection) {
        state.camera.set_projection(projection);
        self.request_frame_now();
    }

    /// Exaggerates heights by `factor` about the middle of what is drawn, and frames the result;
    /// a factor that is not finite and above 0 does nothing.
    pub fn set_vertical_exaggeration(
        &mut self,
        scene: &RenderScene<'_>,
        state: &mut ViewState,
        factor: f64,
    ) {
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        self.options.vertical_exaggeration = factor;
        // The datum is the middle of what is drawn so exaggerating does not also launch the model
        // off the top of the screen. Not only when there are surfaces: a scene of meshes alone
        // needs its datum as much.
        let mut flat = self.options_for(state);
        flat.vertical_exaggeration = 1.0;
        let meshes = Self::shown_meshes(scene.meshes, state);
        let bounds = scene_bounds(scene.document, scene.surfaces, &flat, &meshes);
        self.options.exaggeration_datum = if bounds.empty() {
            0.0
        } else {
            bounds.center().z
        };
        self.invalidate_scene();
        self.zoom_extents(scene, state);
    }

    fn rebuild_if_needed(
        &mut self,
        gpu: Option<&mut GpuSceneView>,
        scene: &RenderScene<'_>,
        state: &mut ViewState,
    ) {
        self.look(scene.document);
        // Line widths and point sizes follow the display the view is on - for the software
        // framebuffer; the GPU view scales logical ones itself.
        let ratio = self.scene_scale() as f32;
        if ratio != self.options.pixel_scale {
            self.options.pixel_scale = ratio;
            self.terrain_dirty = true;
            self.entities_dirty = true;
            self.selection_dirty = true;
        }
        // The view's own ghost switch (ViewState::selection_ghosts), which only the selection
        // overlay reads.
        if state.selection_ghosts != self.options.selection_ghosts {
            self.options.selection_ghosts = state.selection_ghosts;
            self.selection_dirty = true;
        }
        if !self.terrain_dirty && !self.entities_dirty && !self.selection_dirty {
            return;
        }
        let started = Instant::now();
        // Each layer is built from the ones before it: the drawing drapes on the terrain and
        // takes its datum from it, the grid stands on that datum and spans the bounds of both.
        let grid_dirty = self.terrain_dirty || self.entities_dirty;
        let terrain_rebuilt = self.terrain_dirty;
        {
            let options = self.options_for(state);
            if self.terrain_dirty {
                let meshes = Self::shown_meshes(scene.meshes, state);
                self.builder
                    .build_terrain(scene.surfaces, &meshes, &options, &mut self.layers);
                self.terrain_builds += 1;
                self.entities_dirty = true;
            }
            if self.entities_dirty {
                self.builder
                    .build_entities(scene.document, scene.surfaces, &options, &mut self.layers);
                self.built_revision = scene.document.model_revision();
                self.entity_builds += 1;
                self.selection_dirty = true;
            }
            if self.selection_dirty {
                self.builder.build_selection(
                    scene.document,
                    scene.surfaces,
                    &options,
                    &mut self.layers,
                );
                self.selection_builds += 1;
            }
            if grid_dirty {
                self.builder.build_grid(&options, &mut self.layers);
            }
        }
        // What the GPU view must be given at its next frame (prepare_gpu_frame).
        if terrain_rebuilt {
            self.gpu_layers_stale = true;
        } else if grid_dirty {
            self.gpu_drawing_stale = true;
        } else {
            self.gpu_selection_stale = true;
        }
        self.terrain_dirty = false;
        self.entities_dirty = false;
        self.selection_dirty = false;
        self.last_build_ms = started.elapsed().as_secs_f64() * 1.0e3;
        // Empty when nothing but the grid was built.
        self.scene_empty = self.layers.terrain.empty()
            && self.layers.edges.empty()
            && self.layers.entities.empty();
        if self.framed_empty && !self.scene_empty {
            self.zoom_extents_on(gpu, scene, state);
        }
    }

    /// Frames what the view draws; a patch of ground round the origin when it draws nothing.
    pub fn zoom_extents(&mut self, scene: &RenderScene<'_>, state: &mut ViewState) {
        self.with_gpu(|view, gpu| view.zoom_extents_on(gpu, scene, state));
    }

    fn zoom_extents_on(
        &mut self,
        mut gpu: Option<&mut GpuSceneView>,
        scene: &RenderScene<'_>,
        state: &mut ViewState,
    ) {
        // The camera at the view's size first, so frame() fits the aspect the view is seen at.
        // A Qt view's camera took its size at every resize event; here the layout's size
        // (take_layout_size) reaches the camera only at a paint, and a view framed before its
        // first paint - opened by a surface import, then framed by the window - fitted the
        // unknown aspect as square and kept it, as the resize that would refit it had been taken.
        if gpu.is_none() && !self.on_gpu {
            let _ = self.resize_target(state);
        }
        // The box of what was BUILT, so the framing sees exactly the z the linework was put at
        // (draped, own heights, datum) - and a rebuild is due anyway before the next paint.
        self.framed_empty = false; // so the rebuild below does not frame again
        self.rebuild_if_needed(gpu.as_deref_mut(), scene, state);
        // The GPU view frames through this: it is given the scene now, so that what it frames is
        // what it will draw (GpuSceneView::frame_scene). Its first frame fades the edges; until
        // then they are drawn as built.
        if let Some(gpu) = gpu.as_deref_mut()
            && (self.gpu_layers_stale || self.gpu_drawing_stale || self.gpu_selection_stale)
        {
            let edges = self.gpu_layers_stale || self.gpu_edges_shown;
            self.send_layers_to_gpu(gpu, edges, false);
        }
        let mut bounds = self.layers.bounds;
        self.framed_empty = bounds.empty() || self.scene_empty;
        if self.framed_empty {
            // Nothing to frame: show a patch of ground round the origin rather than leaving the
            // camera wherever it was, which looks like a broken view - and the paint says there
            // is nothing to show. 100 m across and a tenth of that in height, so the frame is the
            // ground plane seen at a slant rather than a cube.
            const HALF: f64 = 50.0;
            bounds = AABB::new(
                Vec3::new(-HALF, -HALF, -0.1 * HALF),
                Vec3::new(HALF, HALF, 0.1 * HALF),
            );
        }
        let _ = state.camera.frame(bounds);
        self.framed = true;
        self.refit_on_resize = true;
        // Only a frame of something drawn is worth keeping for the next view: one of the empty
        // ground is replaced by the first real frame anyway.
        state.camera_framed = !self.framed_empty;
        self.request_frame(gpu);
    }

    /// ZOOM (view_verbs): `factor` times closer, below 1 farther, as the wheel turned that many
    /// notches over the middle of the view. A view that has framed nothing yet - a script's VIEWS
    /// OPEN 3d then ZOOM IN, with no paint between - frames the scene first, as its first paint
    /// would, or that paint would frame the zoom away. Out no farther than where the scene is a
    /// pixel across, as a plan view's scale stops at its least. Nothing for a factor that is not
    /// finite and above 0.
    pub fn zoom_by(&mut self, scene: &RenderScene<'_>, state: &mut ViewState, factor: f64) {
        if !factor.is_finite() || !(factor > 0.0) {
            return;
        }
        self.with_gpu(|view, mut gpu| {
            // A view never painted has neither its framebuffer's size nor a frame: both as its
            // first paint would give them, so the zoom starts from what it will show.
            if gpu.is_none() {
                let _ = view.resize_target(state);
            }
            if !view.framed {
                view.zoom_extents_on(gpu.as_deref_mut(), scene, state);
            }
            if let Some(gpu) = gpu.as_deref_mut() {
                gpu.set_camera_framed(true); // or its first frame frames the zoom away
            }
            let mut factor = factor;
            // Out no farther than where the scene is a pixel across: past that a zoom out shows
            // nothing more, and ZOOM 1e-300 took the eye 4.7e302 units off, where the camera's
            // arithmetic had no digits left for where it looked and the target the zoom keeps in
            // the middle moved. A factor above 1 is closer, and the eye is d / factor away
            // afterwards; the scene's diagonal D is a pixel across at D (H / 2) / tan(fov / 2),
            // H the view's height in the camera's pixels. Zoomed out that far already, it goes no
            // farther.
            let bounds = view.layers.bounds;
            if !bounds.empty() && factor < 1.0 {
                let across = (bounds.max - bounds.min).length();
                let camera = &state.camera;
                let farthest = across * 0.5 * f64::from(camera.viewport_height())
                    / (0.5 * camera.field_of_view()).tan();
                if farthest.is_finite() && farthest > 0.0 {
                    factor = katana_math::cpp::max(
                        factor,
                        katana_math::cpp::min(1.0, camera.distance() / farthest),
                    );
                }
            }
            // The notches that make `factor` at ZOOM_PER_NOTCH each, over the middle: the pixel
            // whose centre the view's axis passes through, as Camera::ray_through_pixel samples
            // pixel centres - so the target is still the middle of the view afterwards - in the
            // logical pixels zoom_at_pixel takes.
            let scale = view.scene_scale();
            let middle = PointF::new(
                (0.5 * f64::from(state.camera.viewport_width()) - 0.5) / scale,
                (0.5 * f64::from(state.camera.viewport_height()) - 0.5) / scale,
            );
            view.zoom_at_pixel_on(
                gpu,
                state,
                factor.ln() / Camera::ZOOM_PER_NOTCH.ln(),
                middle,
            );
        });
    }

    /// Frames the camera on the entities `ids` as this view draws them - the ones it shows and,
    /// with its ghosts on, the ones it ghosts (SceneBuilder::overlay_bounds): ZOOM on a scope,
    /// Zoom to Selection. Kept on a resize, as the user's own view is. False, and the camera
    /// untouched, when the view draws none of them.
    pub fn zoom_to_entities(
        &mut self,
        scene: &RenderScene<'_>,
        state: &mut ViewState,
        ids: &[EntityId],
    ) -> bool {
        if ids.is_empty() {
            return false;
        }
        self.with_gpu(|view, mut gpu| {
            if gpu.is_none() {
                let _ = view.resize_target(state); // frame() fits the camera's aspect
            }
            // The scene as the view draws it now - its datum, its options, its hidden layers and
            // ghost switch - so the box is where they are drawn.
            view.rebuild_if_needed(gpu.as_deref_mut(), scene, state);
            let options = view.options_for(state);
            let bounds = view.builder.overlay_bounds(
                scene.document,
                scene.surfaces,
                &options,
                &view.layers,
                ids,
            );
            if bounds.empty() || !state.camera.frame(bounds) {
                return false;
            }
            view.framed = true;
            view.framed_empty = false;
            // Where the user asked to look: a resize keeps it, as after the wheel, rather than
            // framing the whole scene again.
            view.refit_on_resize = false;
            state.camera_framed = true;
            if let Some(gpu) = gpu.as_deref_mut() {
                gpu.set_camera_framed(true);
            }
            view.request_frame(gpu);
            true
        })
    }

    /// Zooms by `notches` wheel notches, positive in, about `at` in this view's logical pixels,
    /// towards what is drawn there (the cad layer's zoom_at_pixel): what the wheel does over
    /// either renderer, and what zoom_by calls, so a zoom from a view's bar is the wheel's own.
    pub fn zoom_at_pixel(&mut self, state: &mut ViewState, notches: f64, at: PointF) {
        self.with_gpu(|view, gpu| view.zoom_at_pixel_on(gpu, state, notches, at));
    }

    fn zoom_at_pixel_on(
        &mut self,
        gpu: Option<&mut GpuSceneView>,
        state: &mut ViewState,
        notches: f64,
        at: PointF,
    ) {
        if !notches.is_finite() || notches == 0.0 {
            return;
        }
        self.refit_on_resize = false;
        // In the layers as last built, not rebuilt first: the cursor points at what the last
        // frame drew. The camera counts in the framebuffer's device pixels here and in the GPU
        // view's logical ones there - scene_scale, the widths' scale, is the same ratio.
        let scale = self.scene_scale();
        let zoomed = zoom_at_pixel(
            &self.layers,
            &mut state.camera,
            notches,
            at.x * scale,
            at.y * scale,
        );
        // A wheel that stops without a word reads as the fault it once was.
        if zoomed.limited {
            self.events
                .push(RenderViewEvent::Status(String::from(ZOOM_LIMIT_MESSAGE)));
        }
        self.request_frame(gpu);
    }

    // ---- the size -----------------------------------------------------------------------------

    /// Sizes the framebuffer and the camera for the view's size in DEVICE pixels. False when it
    /// could not.
    fn resize_target(&mut self, state: &mut ViewState) -> bool {
        // In DEVICE pixels: a 3D view on a 125% display rendered at 80% of its resolution and was
        // scaled up nearest-neighbour, so a 1 px line beaded into 1 and 2 px steps while the plan
        // view beside it stayed crisp.
        let ratio = self.pixel_ratio();
        let wide = ((f64::from(self.width) * ratio).round() as i32).max(1);
        let high = ((f64::from(self.height) * ratio).round() as i32).max(1);
        if self.framebuffer.width() == wide
            && self.framebuffer.height() == high
            && state.camera.viewport_width() == wide
            && state.camera.viewport_height() == high
        {
            return true;
        }
        if let Err(error) = self.framebuffer.resize(wide, high) {
            self.events
                .push(RenderViewEvent::Status(error.describe()));
            return false;
        }
        state.camera.set_viewport_size(wide, high);
        true
    }

    /// The size the workspace's layout gives a view that has framed nothing yet, before its first
    /// frame draws it: a script's VIEWS OPEN 3d then ZOOM IN, with no frame between, frames the
    /// scene at the size the view is seen at (the C++'s workspace does the layout Qt has queued
    /// before it zooms). A framed view is resized by its frame, which refits it with the scene.
    pub fn take_layout_size(&mut self, width: i32, height: i32) {
        if self.framed {
            return;
        }
        self.width = width.max(1);
        self.height = height.max(1);
        self.dirty = true;
    }

    /// QWidget::resizeEvent: the view is `width` by `height` of its own logical pixels now.
    pub fn resize(
        &mut self,
        scene: &RenderScene<'_>,
        state: &mut ViewState,
        width: i32,
        height: i32,
    ) {
        self.width = width.max(1);
        self.height = height.max(1);
        self.dirty = true;
        if self.on_gpu {
            let refit = self.framed && self.refit_on_resize;
            let (wide, high) = (self.width, self.height);
            if let Some(gpu) = &mut self.gpu {
                gpu.resize(&mut state.camera, wide, high);
                // Framed again at the GPU view's next frame, once it has the new size - framing
                // now would fit the aspect it had.
                if refit {
                    gpu.set_camera_framed(false);
                }
            }
            return;
        }
        if !self.resize_target(state) {
            return;
        }
        // A frame made before the view had its real size fitted the wrong aspect - or none,
        // before the first resize - and cut the sides off. Until the user moves the camera, every
        // resize frames again.
        if self.framed && self.refit_on_resize {
            self.zoom_extents(scene, state);
        }
    }

    // ---- painting -----------------------------------------------------------------------------

    /// QWidget::paintEvent: the view painted at its size now, by whichever renderer draws it,
    /// into `picture`, with the legend and the empty message over it.
    pub fn paint(&mut self, scene: &RenderScene<'_>, state: &mut ViewState) -> &Image {
        self.deliver();
        if self.on_gpu {
            self.paint_gpu(scene, state);
            return &self.picture;
        }
        let started = Instant::now();
        self.empty_message_shown = false;
        self.dirty = false;
        // The display the view is on may have changed since the last resize.
        if !self.resize_target(state) || self.framebuffer.empty() {
            self.picture = filled(self.width, self.height, BACKGROUND);
            return &self.picture;
        }
        let rebuilt = self.terrain_dirty || self.entities_dirty || self.selection_dirty;
        if !self.framed {
            self.zoom_extents_on(None, scene, state);
        }
        self.rebuild_if_needed(None, scene, state);

        // The depth range fitted, the edges faded and the layers drawn in their order and with
        // their depth rules: render_layers, so the tests draw exactly what the view does.
        let options = RenderOptions {
            background: BACKGROUND_RGBA,
            ..RenderOptions::default()
        };
        let mut failure = String::new();
        match render_layers(
            &mut self.layers,
            &mut state.camera,
            &mut self.rasterizer,
            &mut self.framebuffer,
            options,
        ) {
            Ok(stats) => self.stats = stats,
            Err(error) => {
                self.stats = RenderStats::default();
                failure = error.describe();
            }
        }
        if !failure.is_empty() {
            let mut picture = filled(self.width, self.height, Color::rgb(60, 20, 20));
            let rect = RectF::new(0.0, 0.0, f64::from(self.width), f64::from(self.height));
            RasterCanvas::paint_image(&mut picture, &mut |canvas: &mut dyn Canvas| {
                canvas.set_font(ui_font());
                canvas.set_pen(Pen::new(Color::rgb(255, 255, 255)));
                canvas.draw_text_in(rect, TextFlags::CENTER, &failure);
            });
            self.picture = picture;
            return &self.picture;
        }

        // The framebuffer's pixels, at the display's ratio: drawn over the view's logical size
        // one to one, instead of scaled up.
        let mut picture = image_of(
            self.framebuffer.color(),
            self.framebuffer.width(),
            self.framebuffer.height(),
        );
        picture.set_device_pixel_ratio(self.pixel_ratio());
        self.paint_overlays_onto(&mut picture, scene);
        self.picture = picture;

        self.last_frame_ms = started.elapsed().as_secs_f64() * 1.0e3;
        // The whole paint - build, render and blit - and the build on its own when this paint
        // did one.
        let mut text = format!(
            "{}  {} tri  {} ms",
            to_string(state.kind),
            self.stats.triangles_rasterised,
            qt_fixed(self.last_frame_ms, 1)
        );
        if rebuilt {
            text.push_str(&format!(" (scene {} ms)", qt_fixed(self.last_build_ms, 1)));
        }
        self.events.push(RenderViewEvent::FrameStats(text));
        &self.picture
    }

    /// The GPU child's frame, drawn afresh, with the overlays over it; the background alone when
    /// it could not draw.
    fn paint_gpu(&mut self, scene: &RenderScene<'_>, state: &mut ViewState) {
        self.empty_message_shown = false;
        self.dirty = false;
        let frame = self
            .on_gpu_view(scene, state, |gpu, host| gpu.grab_framebuffer(host))
            .flatten();
        let mut picture = match frame {
            Some(frame) => frame,
            None => filled(self.width, self.height, BACKGROUND),
        };
        let wide = picture.width();
        if self.width > 0 && wide > 0 {
            picture.set_device_pixel_ratio(f64::from(wide) / f64::from(self.width));
        }
        self.paint_overlays_onto(&mut picture, scene);
        self.picture = picture;
    }

    /// The GPU child's frame drawn afresh, with nothing over it: QRhiWidget::grabFramebuffer on
    /// the child. None when there is no child, or it could not draw.
    pub fn grab_gpu_frame(&mut self, scene: &RenderScene<'_>, state: &mut ViewState) -> Option<Image> {
        self.deliver();
        self.on_gpu_view(scene, state, |gpu, host| gpu.grab_framebuffer(host))
            .flatten()
    }

    /// Pixels that are not the background in a frame painted afresh, by whichever renderer draws
    /// it: a headless run's word that the view shows something.
    pub fn painted_pixel_count(&mut self, scene: &RenderScene<'_>, state: &mut ViewState) -> usize {
        let background = BACKGROUND_RGBA & 0x00FF_FFFF;
        if self.on_gpu {
            // The GPU child's frame, drawn afresh: the software framebuffer is not what the view
            // shows while the GPU draws it.
            let Some(frame) = self.grab_gpu_frame(scene, state) else {
                return 0;
            };
            let mut painted = 0;
            for y in 0..frame.height() {
                for x in 0..frame.width() {
                    let c = frame.pixel(x, y);
                    if (c.r, c.g, c.b) != (red_of(background), green_of(background), blue_of(background)) {
                        painted += 1;
                    }
                }
            }
            return painted;
        }
        // Painted afresh first, as the C++ report grabs the widget: the count is of what the view
        // shows now.
        let _ = self.paint(scene, state);
        self.framebuffer
            .color()
            .iter()
            .filter(|&&colour| (colour & 0x00FF_FFFF) != background)
            .count()
    }

    /// The legend and the empty message over `picture`, the frame either renderer drew.
    fn paint_overlays_onto(&mut self, picture: &mut Image, scene: &RenderScene<'_>) {
        let drawing_empty = Self::drawing_is_empty(scene);
        RasterCanvas::paint_image(picture, &mut |canvas: &mut dyn Canvas| {
            self.paint_overlays(canvas, drawing_empty);
        });
    }

    fn paint_overlays(&mut self, canvas: &mut dyn Canvas, drawing_empty: bool) {
        self.empty_message_shown = false;
        canvas.set_font(ui_font());
        self.draw_legend(canvas);
        // Only for a drawing with nothing in it. A scene of the grid alone is also what a drawing
        // whose every layer is hidden - in the document or in this view - builds, and telling
        // that user to draw or import something would be wrong; the plan view says nothing then
        // either.
        if self.scene_empty && drawing_empty {
            self.empty_message_shown = self.draw_empty_message(canvas);
        }
    }

    /// The legends of the surfaces drawn in a display style, stacked down the corner; or the one
    /// elevation ramp's bar, when a surface is coloured by it and none has a legend of its own.
    /// Records what it drew (drawn_legends).
    fn draw_legend(&mut self, canvas: &mut dyn Canvas) {
        self.drawn_legends.clear();
        if self.height < 160 || self.width < 160 {
            return;
        }
        let backing = Color::rgba(BACKGROUND.r, BACKGROUND.g, BACKGROUND.b, 200);
        // Surface display (docs/surface_display.md): each surface drawn in a display style is
        // read off its style's legend, drawn as the plan view draws them, through the one stack:
        // down the corner in drawing order, each captioned with its surface, none cut by the
        // view's edge and none over another. Unlike the plan view, which keeps three over a
        // drawing being edited, the stack here is limited only by the room: a surface whose key
        // is missing is a surface nobody can read.
        if !self.layers.legends.is_empty() {
            let stack = LegendStack {
                top_right: PointF::new(f64::from(self.width) - 16.0, 16.0),
                bottom: f64::from(self.height) - 16.0,
                maximum: self.layers.legends.len(),
                font: canvas.font(),
                text: palette::text_muted(),
                backing,
                ..LegendStack::default()
            };
            self.drawn_legends = paint_surface_legend_stack(canvas, &self.layers.legends, &stack);
            return;
        }
        if !self.layers.has_ramp() {
            return;
        }
        // A small bar of the one elevation ramp with its two ends, so a colour can be read as a
        // height. QRect's whole pixels, as the C++ laid it out.
        const BAR_WIDTH: i32 = 10;
        const BAR_HEIGHT: i32 = 110;
        const STOPS: i32 = 8;
        let font = canvas.font();
        let metrics = canvas.metrics();
        let high = qt_fixed(self.layers.ramp_high, 2);
        let low = qt_fixed(self.layers.ramp_low, 2);
        let text_width = (metrics.horizontal_advance(&font, &high).round() as i32)
            .max(metrics.horizontal_advance(&font, &low).round() as i32);
        let text_height = metrics.height(&font).round() as i32;
        let ascent = metrics.ascent(&font).round() as i32;
        let left = self.width - 16 - BAR_WIDTH - text_width - 6;
        let top = 16;
        // QRect's right and bottom are its last pixel.
        let right = left + BAR_WIDTH - 1;
        let bottom = top + BAR_HEIGHT - 1;

        canvas.save();
        let mut gradient = LinearGradient::new(
            PointF::new(f64::from(left), f64::from(top)),
            PointF::new(f64::from(left), f64::from(bottom)),
        );
        for i in 0..=STOPS {
            let t = f64::from(i) / f64::from(STOPS);
            let c = elevation_ramp_color(1.0 - t);
            gradient.set_color_at(t, Color::rgb(red_of(c), green_of(c), blue_of(c)));
        }
        canvas.set_pen(Pen::none());
        canvas.set_brush(Brush::solid(backing));
        // bar.adjusted(-6, -6 - h / 2, w + 12, 6 + h / 2), as a QRectF of its pixels.
        let x1 = left - 6;
        let y1 = top - 6 - text_height / 2;
        let x2 = right + text_width + 12;
        let y2 = bottom + 6 + text_height / 2;
        canvas.draw_rounded_rect(
            RectF::new(
                f64::from(x1),
                f64::from(y1),
                f64::from(x2 - x1 + 1),
                f64::from(y2 - y1 + 1),
            ),
            4.0,
            4.0,
        );
        canvas.set_brush(Brush::linear(gradient));
        canvas.draw_rect(RectF::new(
            f64::from(left),
            f64::from(top),
            f64::from(BAR_WIDTH),
            f64::from(BAR_HEIGHT),
        ));
        canvas.set_pen(Pen::new(palette::text_muted()));
        canvas.draw_text(
            PointF::new(f64::from(right + 6), f64::from(top + ascent / 2)),
            &high,
        );
        canvas.draw_text(
            PointF::new(f64::from(right + 6), f64::from(bottom + ascent / 2)),
            &low,
        );
        canvas.restore();
    }

    /// True when the document has no entity or alignment and the view has no surface or mesh:
    /// nothing that any layer could be hiding.
    fn drawing_is_empty(scene: &RenderScene<'_>) -> bool {
        let model = scene.document.model();
        model.entities.empty()
            && model.alignments.empty()
            && scene.surfaces.is_empty()
            && scene.meshes.is_empty()
    }

    /// An empty 3D view looked broken: a grid and nothing on it, with no word of why. It says what
    /// would put something there. False when the view is too small to hold the message, which is
    /// then left out: the grid alone says less wrongly.
    fn draw_empty_message(&self, canvas: &mut dyn Canvas) -> bool {
        let area = RectF::new(
            12.0,
            12.0,
            f64::from(self.width - 24),
            f64::from(self.height - 24),
        );
        let flags = TextFlags::CENTER.wrapped();
        let font = canvas.font();
        let text = wrapped_bounds(canvas, &font, area, flags, EMPTY_MESSAGE);
        if text.height > area.height {
            return false;
        }
        canvas.save();
        // A backing of the view's own ground so the grid does not strike through the words, and
        // the chrome's muted text so it reads as the application talking rather than as
        // something in the model.
        canvas.set_pen(Pen::none());
        canvas.set_brush(Brush::solid(Color::rgba(
            BACKGROUND.r,
            BACKGROUND.g,
            BACKGROUND.b,
            225,
        )));
        canvas.draw_rounded_rect(text.adjusted(-12.0, -8.0, 12.0, 8.0), 6.0, 6.0);
        canvas.set_pen(Pen::new(palette::text_muted()));
        canvas.draw_text_in(area, flags, EMPTY_MESSAGE);
        canvas.restore();
        true
    }

    // ---- interaction --------------------------------------------------------------------------

    /// Focus has arrived in this view for `reason`, from `previous`: Activated when that is the
    /// user choosing the view (view_focus's rule).
    pub fn focus_arrived(
        &mut self,
        reason: FocusReason,
        previous: Option<FocusPrevious>,
        view_floats: bool,
    ) {
        if focus_chooses_view(reason, previous, view_floats) {
            self.events.push(RenderViewEvent::Activated);
        }
    }

    pub fn mouse_press(
        &mut self,
        scene: &RenderScene<'_>,
        state: &mut ViewState,
        at: PointF,
        button: MouseButton,
        modifiers: KeyModifiers,
    ) {
        self.deliver();
        // The user is moving the camera now: a resize no longer frames the scene again.
        self.refit_on_resize = false;
        if self.on_gpu {
            let _ = self.on_gpu_view(scene, state, |gpu, _host| {
                gpu.mouse_press(at, button, modifiers);
            });
            return;
        }
        self.events.push(RenderViewEvent::Activated);
        self.last_mouse = at;
        let pan_modifier = modifiers.shift;
        self.drag = match button {
            MouseButton::Middle => Drag::Pan,
            MouseButton::Left if pan_modifier => Drag::Pan,
            // An orthographic elevation view is a measured drawing; orbiting it would quietly
            // turn it into something you cannot scale off. Pan only.
            MouseButton::Left if state.kind == ViewKind::Elevation => Drag::Pan,
            MouseButton::Left => Drag::Orbit,
            _ => Drag::None,
        };
    }

    pub fn mouse_move(&mut self, scene: &RenderScene<'_>, state: &mut ViewState, at: PointF) {
        if self.on_gpu {
            let _ = self.on_gpu_view(scene, state, |gpu, host| gpu.mouse_move(host, at));
            return;
        }
        if self.drag == Drag::None {
            return;
        }
        let delta = at - self.last_mouse;
        self.last_mouse = at;
        if self.drag == Drag::Orbit {
            // Dragging right turns the model to the right, so the camera goes the other way;
            // dragging down tips the top towards you.
            state
                .camera
                .orbit(-delta.x * ORBIT_PER_PIXEL, delta.y * ORBIT_PER_PIXEL);
        } else {
            // The camera counts in framebuffer (device) pixels.
            let ratio = self.pixel_ratio();
            state.camera.pan_pixels(delta.x * ratio, delta.y * ratio);
        }
        self.dirty = true;
    }

    pub fn mouse_release(&mut self) {
        self.drag = Drag::None;
        if let Some(gpu) = &mut self.gpu {
            gpu.mouse_release();
        }
    }

    pub fn mouse_double_click(&mut self, scene: &RenderScene<'_>, state: &mut ViewState) {
        if self.on_gpu {
            let _ = self.on_gpu_view(scene, state, |gpu, host| gpu.mouse_double_click(host));
            return;
        }
        self.zoom_extents(scene, state);
    }

    /// The wheel turned over `at` by `angle_delta` eighths of a degree, 120 a notch (one notch is
    /// 120 eighths of a degree; a fine wheel sends fractions): a zoom towards what is drawn
    /// there. True when the event was taken; a turn of nothing is not.
    pub fn wheel(
        &mut self,
        scene: &RenderScene<'_>,
        state: &mut ViewState,
        at: PointF,
        angle_delta: f64,
    ) -> bool {
        self.deliver();
        if self.on_gpu {
            // The user is moving the camera now, as on a press.
            self.refit_on_resize = false;
            return self
                .on_gpu_view(scene, state, |gpu, host| gpu.wheel(host, at, angle_delta))
                .unwrap_or(false);
        }
        let notches = angle_delta / 120.0;
        if notches == 0.0 {
            return false;
        }
        self.zoom_at_pixel(state, notches, at);
        true
    }

    /// A key: true when the view took it.
    pub fn key_press(&mut self, scene: &RenderScene<'_>, state: &mut ViewState, key: SceneKey) -> bool {
        self.deliver();
        if self.on_gpu {
            self.refit_on_resize = false;
            return self
                .on_gpu_view(scene, state, |gpu, host| gpu.key_press(host, key))
                .unwrap_or(false);
        }
        if let Some(view) = key.standard_view() {
            self.set_standard_view(state, view);
            return true;
        }
        match key {
            SceneKey::P => {
                let projection = toggled(state.camera.projection());
                self.set_projection(state, projection);
                true
            }
            SceneKey::E => {
                self.zoom_extents(scene, state);
                true
            }
            _ => false,
        }
    }

    // ---- egui ---------------------------------------------------------------------------------

    /// The view drawn into the rectangle `ui` has left, never smaller than its minimum: the
    /// frame's input turned into the view's events, the view painted when a frame was asked for,
    /// and its picture shown. The view is the control "RenderView<id>" (a View record). The
    /// caller reads `take_events` after.
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        scene: &RenderScene<'_>,
        state: &mut ViewState,
    ) -> egui::Response {
        let wanted = ui
            .available_size()
            .max(egui::vec2(MINIMUM_SIZE as f32, MINIMUM_SIZE as f32));
        let (rect, response) = ui.allocate_exact_size(wanted, egui::Sense::click_and_drag());
        self.set_device_ratio(f64::from(ui.ctx().pixels_per_point()));
        let width = rect.width().round() as i32;
        let height = rect.height().round() as i32;
        if (width, height) != (self.width, self.height) {
            self.resize(scene, state, width, height);
        }
        let local =
            |at: egui::Pos2| PointF::new(f64::from(at.x - rect.min.x), f64::from(at.y - rect.min.y));
        let over = response.contains_pointer();
        let (events, now) = ui.input(|input| (input.events.clone(), input.time));
        for event in events {
            match event {
                egui::Event::PointerMoved(at) => {
                    if over || !self.egui.held.is_empty() {
                        self.mouse_move(scene, state, local(at));
                    }
                }
                egui::Event::PointerButton {
                    pos,
                    button,
                    pressed,
                    modifiers,
                } => {
                    let button = match button {
                        egui::PointerButton::Primary => MouseButton::Left,
                        egui::PointerButton::Secondary => MouseButton::Right,
                        egui::PointerButton::Middle => MouseButton::Middle,
                        _ => MouseButton::Other,
                    };
                    let modifiers = KeyModifiers::from_egui(modifiers);
                    let at = local(pos);
                    if pressed {
                        if !over {
                            continue;
                        }
                        response.request_focus();
                        self.egui.held.push(button);
                        let double = self.egui.last_press.is_some_and(|(when, place, which)| {
                            which == button
                                && now - when <= DOUBLE_CLICK_SECONDS
                                && (at.x - place.x).abs() <= DOUBLE_CLICK_PIXELS
                                && (at.y - place.y).abs() <= DOUBLE_CLICK_PIXELS
                        });
                        if double {
                            // The third press of a run starts a new click, as Qt's does.
                            self.egui.last_press = None;
                            self.mouse_double_click(scene, state);
                        } else {
                            self.egui.last_press = Some((now, at, button));
                            self.mouse_press(scene, state, at, button, modifiers);
                        }
                    } else if let Some(held) =
                        self.egui.held.iter().position(|&held| held == button)
                    {
                        self.egui.held.remove(held);
                        self.mouse_release();
                    }
                }
                egui::Event::MouseWheel { unit, delta, .. } => {
                    if over && let Some(at) = ui.input(|input| input.pointer.latest_pos()) {
                        let notches = match unit {
                            egui::MouseWheelUnit::Point => {
                                f64::from(delta.y) / WHEEL_POINTS_A_NOTCH
                            }
                            _ => f64::from(delta.y),
                        };
                        let _ = self.wheel(scene, state, local(at), notches * 120.0);
                    }
                }
                egui::Event::Key { key, pressed, .. } if pressed && response.has_focus() => {
                    let key = SceneKey::from_egui(key);
                    if key != SceneKey::Other {
                        let _ = self.key_press(scene, state, key);
                    }
                }
                _ => {}
            }
        }
        if ui.is_rect_visible(rect) {
            if self.needs_frame() || self.egui.texture.is_none() {
                let picture = self.paint(scene, state).to_color_image();
                match &mut self.egui.texture {
                    Some(texture) => texture.set(picture, egui::TextureOptions::NEAREST),
                    None => {
                        self.egui.texture = Some(ui.ctx().load_texture(
                            self.name.as_str(),
                            picture,
                            egui::TextureOptions::NEAREST,
                        ));
                    }
                }
            }
            if let Some(texture) = &self.egui.texture {
                ui.painter().image(
                    texture.id(),
                    rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
        }
        controls::custom(
            ui,
            &response,
            ControlRecord::new(&self.name, ControlKind::View),
        );
        response
    }
}

/// An image of the view's size in logical pixels filled with `colour`.
fn filled(width: i32, height: i32, colour: Color) -> Image {
    let mut image = Image::new(width.max(1) as u32, height.max(1) as u32);
    image.fill(colour);
    image
}

/// Where `text` would go in `rect` as `flags` lay it out, drawn with no pen: QFontMetrics's
/// boundingRect(rect, flags, text), which a canvas answers by laying the text out.
pub(crate) fn wrapped_bounds(
    canvas: &mut dyn Canvas,
    font: &Font,
    rect: RectF,
    flags: TextFlags,
    text: &str,
) -> RectF {
    canvas.save();
    canvas.set_pen(Pen::none());
    canvas.set_font(font.clone());
    let bounds = canvas.draw_text_in(rect, flags, text);
    canvas.restore();
    bounds
}
