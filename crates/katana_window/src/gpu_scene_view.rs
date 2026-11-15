//! The 3D view drawn on the GPU (docs/gpu.md, "The widget"): a draw list shown through a camera
//! with katana_gpu's renderer. The port of gpu/gpu_scene_view.
//!
//! Written to be HOSTED, as the C++ widget is. The render view (render_view_widget) owns the scene
//! building, the empty message, the status line and the document; this only draws and turns the
//! mouse into camera moves - the SAME moves the render view makes (left drag orbits, or pans in an
//! elevation view; middle or Shift+left drag pans; the wheel zooms towards what is under the
//! cursor, through the host, which has the scene to find it in; double-click and E frame the
//! scene; 1-5 and 0 pick standard views; P toggles the projection), so a host can draw either in
//! the same place and the user feels no difference.
//!
//! THE RUST SHAPE. The C++ widget holds its host's camera by reference and calls the host back
//! through `std::function` hooks, one of them (onPrepareFrame) in the middle of drawing a frame.
//! Here the host is lent to each call as a `GpuSceneHost`: it hands out the camera - the host's,
//! a ViewState's, so switching between this view and the software one keeps the view - and its
//! hooks are the trait's methods, each lent this view so the host can hand it layers while it
//! frames or prepares a frame. A hook a host does not set answers false and the view does what
//! the C++ widget did without one. What the C++ raised only to tell (onActivated, onFrameStats,
//! onRenderFailed) is queued (`GpuSceneEvent`, `take_events`). A bare `Camera` is a host with no
//! hooks.
//!
//! THE FRAME. The C++ widget was a QRhiWidget: Qt gave it a device and a surface. Here the window
//! gives it a device (`set_device`, the window's own, or a test's shared one), and each frame is
//! drawn into an offscreen target on it (katana_gpu's OffscreenGpu) at the view's size in device
//! pixels and read back: the picture the window shows as a texture and a test reads
//! (`grab_framebuffer`, QRhiWidget::grabFramebuffer's twin). The renderer and its packed scene are
//! the view's own, lent to the target for each frame, so a resize - a new target - packs nothing
//! again. Rejected: drawing into egui-wgpu's pass through a paint callback, which needs egui-wgpu
//! named in this crate (it is not a dependency of it), and which a test could not read back.
//!
//! The camera's view - eye, target, zoom - depends on its viewport's aspect, not its pixel count,
//! so this view keeps the host's camera in its LOGICAL pixels and draws each frame through a copy
//! sized to the device pixels it draws, where the render view sizes it to device pixels before
//! every paint - and the software view taking over after a failure finds the same view at its own
//! resolution.

use std::time::Instant;

use katana_core::Result;
use katana_gpu::{
    FrameSettings, GpuBackend, GpuContext, GpuDevice, GpuFrameStats, GpuRenderer, LayerSource,
    OffscreenGpu, OffscreenOptions, RendererDecision, RendererEnvironment, RendererKind,
    TargetFormat, choose_renderer, to_string,
};
use katana_math::Vec3;
use katana_render::{
    Camera, DrawList, Projection, Rgba, StandardView, alpha_of, blue_of, green_of, red_of,
};

use crate::canvas::{Image, PointF};
use crate::format::qt_fixed;
use crate::viewport_widget::{KeyModifiers, MouseButton};

/// The same number as the render view's (render_view_widget): the two views must feel identical
/// under the mouse, so a change to one belongs in both. Radians per logical pixel of drag. The
/// wheel's factor is the camera's own, Camera::ZOOM_PER_NOTCH.
pub const ORBIT_PER_PIXEL: f64 = 0.008;

/// The smallest the view may be made (QWidget::setMinimumSize(40, 40)).
pub const MINIMUM_SIZE: i32 = 40;

/// The samples a pixel the view asks for (QRhiWidget::setSampleCount(4)).
const SAMPLE_COUNT: u32 = 4;

/// OffscreenGpu's formats (offscreen_gpu.rs), which the view's own renderer is made for: RGBA8 as
/// the C++'s colour, a 32-bit float depth for reversed Z.
const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// The keys the 3D views tell apart (Qt::Key); every other key is `Other`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SceneKey {
    /// Top.
    One,
    /// Front.
    Two,
    /// Right.
    Three,
    /// Back.
    Four,
    /// Left.
    Five,
    /// The south-west isometric view.
    Zero,
    /// Perspective and orthographic in turn.
    P,
    /// Zoom Extents.
    E,
    Other,
}

impl SceneKey {
    /// The standard view a number key picks, if it is one.
    pub fn standard_view(self) -> Option<StandardView> {
        match self {
            SceneKey::One => Some(StandardView::Top),
            SceneKey::Two => Some(StandardView::Front),
            SceneKey::Three => Some(StandardView::Right),
            SceneKey::Four => Some(StandardView::Back),
            SceneKey::Five => Some(StandardView::Left),
            SceneKey::Zero => Some(StandardView::IsoSouthWest),
            _ => None,
        }
    }

    /// egui's key as the views tell it.
    pub fn from_egui(key: egui::Key) -> Self {
        match key {
            egui::Key::Num1 => SceneKey::One,
            egui::Key::Num2 => SceneKey::Two,
            egui::Key::Num3 => SceneKey::Three,
            egui::Key::Num4 => SceneKey::Four,
            egui::Key::Num5 => SceneKey::Five,
            egui::Key::Num0 => SceneKey::Zero,
            egui::Key::P => SceneKey::P,
            egui::Key::E => SceneKey::E,
            _ => SceneKey::Other,
        }
    }
}

/// The other projection: P's toggle, in both views.
pub(crate) fn toggled(projection: Projection) -> Projection {
    if projection == Projection::Perspective {
        Projection::Orthographic
    } else {
        Projection::Perspective
    }
}

/// What the C++ view raised only to tell its host, in order.
#[derive(Clone, Debug, PartialEq)]
pub enum GpuSceneEvent {
    /// The view was clicked, so a workspace can make it active (onActivated).
    Activated,
    /// "GPU  12345 tri  0.8 ms" after every frame, the CPU's time to record it (onFrameStats).
    FrameStats(String),
    /// Rendering failed, the first time it did, and why (onRenderFailed): the host should swap in
    /// the software view (renderer_choice's rule 4).
    RenderFailed(String),
}

/// What hosts the view: its camera, and the hooks a host may set (see the module comment).
pub trait GpuSceneHost {
    /// The host's camera, which the view moves and draws through. It must be the same camera at
    /// every call.
    fn camera(&mut self) -> &mut Camera;

    /// onZoomExtents: the host frames the scene - it knows what "the scene" is - and may hand the
    /// view its layers as it does. False when the host sets no such hook; the view then frames
    /// its packed lists' bounds itself.
    fn zoom_extents(&mut self, view: &mut GpuSceneView) -> bool {
        let _ = view;
        false
    }

    /// onWheelZoom: the wheel, in notches (positive in, a fine wheel's fractions too) at `at` in
    /// logical pixels. A host that knows the scene zooms towards what is drawn under the cursor;
    /// false when it sets no such hook, and the view then zooms about the target's plane itself
    /// (Camera::dolly_at_pixel), which stalls over ground beyond that plane (docs/render.md,
    /// "Zooming towards the cursor").
    fn wheel_zoom(&mut self, view: &mut GpuSceneView, notches: f64, at: PointF) -> bool {
        let _ = (view, notches, at);
        false
    }

    /// onPrepareFrame: called at the start of every frame with the camera it will be drawn
    /// through - the host's, sized to the device pixels drawn - before anything is uploaded:
    /// where a host fits the depth range to its scene and fades its edges for the frame's scale,
    /// as the cad layer's render_layers does for the software path, and calls `update_layer` for
    /// what that changed.
    fn prepare_frame(&mut self, view: &mut GpuSceneView, frame_camera: &mut Camera) {
        let _ = (view, frame_camera);
    }
}

/// A camera alone: a host with no hooks.
impl GpuSceneHost for Camera {
    fn camera(&mut self) -> &mut Camera {
        self
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Drag {
    #[default]
    None,
    Orbit,
    Pan,
}

/// See the module comment.
pub struct GpuSceneView {
    /// The device it draws on; None until the window gives it one.
    device: Option<GpuContext>,
    /// The target the last frame was drawn into, at the size it was drawn.
    target: Option<OffscreenGpu>,
    /// The packed scene, lent to the target for each frame.
    renderer: GpuRenderer,
    settings: FrameSettings,
    stats: GpuFrameStats,
    failed: bool,
    /// The first frame frames the scene unless this is set: by a frame of something, by the host
    /// (set_camera_framed), or by the host's framing hook.
    framed: bool,
    /// The last framing was of an empty draw list (the host's hook framed whatever it frames for
    /// nothing): the first list with something in it clears `framed`, so that it is framed in
    /// its turn.
    framed_empty: bool,
    orbit_allowed: bool,
    software_device_allowed: bool,
    /// True while the host's prepare_frame runs, inside a frame.
    in_frame: bool,
    /// A repaint asked for (QWidget::update).
    dirty: bool,
    width: i32,
    height: i32,
    /// Device pixels a logical pixel, until a target says what was drawn.
    device_ratio: f64,
    drag: Drag,
    last_mouse: PointF,
    events: Vec<GpuSceneEvent>,
}

impl Default for GpuSceneView {
    fn default() -> Self {
        Self::new()
    }
}

impl GpuSceneView {
    /// A view with no device, the size Qt gives a widget never shown.
    pub fn new() -> Self {
        Self {
            device: None,
            target: None,
            renderer: GpuRenderer::new(),
            settings: FrameSettings::default(),
            stats: GpuFrameStats::default(),
            failed: false,
            framed: false,
            framed_empty: false,
            orbit_allowed: true,
            software_device_allowed: false,
            in_frame: false,
            dirty: true,
            width: 640,
            height: 480,
            device_ratio: 1.0,
            drag: Drag::None,
            last_mouse: PointF::default(),
            events: Vec::new(),
        }
    }

    /// The device to draw on: the window's (katana_gpu's GpuContext::from_parts over egui-wgpu's
    /// device), or a test's. None takes it away: the next frame then fails, as a QRhiWidget Qt
    /// made no device for does.
    pub fn set_device(&mut self, device: Option<GpuContext>) {
        self.device = device;
        self.target = None;
        self.renderer.release_resources();
        self.dirty = true;
    }

    pub fn device(&self) -> Option<&GpuContext> {
        self.device.as_ref()
    }

    /// What the view raised since the last call of this, in order.
    pub fn take_events(&mut self) -> Vec<GpuSceneEvent> {
        std::mem::take(&mut self.events)
    }

    /// QWidget::update: a frame is wanted.
    pub fn update(&mut self) {
        self.dirty = true;
    }

    /// Whether a frame has been asked for since the last one was drawn.
    pub fn needs_frame(&self) -> bool {
        self.dirty
    }

    /// The view's size in its logical pixels.
    pub fn size(&self) -> (i32, i32) {
        (self.width, self.height)
    }

    pub fn width(&self) -> i32 {
        self.width
    }

    pub fn height(&self) -> i32 {
        self.height
    }

    /// The device pixels a logical pixel is (QWidget::devicePixelRatioF).
    pub fn set_device_ratio(&mut self, ratio: f64) {
        if ratio.is_finite() && ratio > 0.0 && ratio != self.device_ratio {
            self.device_ratio = ratio;
            self.dirty = true;
        }
    }

    pub fn device_ratio(&self) -> f64 {
        self.device_ratio
    }

    /// Packs `list` for the GPU and asks for a frame; the upload happens in the next frame, and
    /// only then. Call it when the scene changes, not per frame.
    pub fn set_draw_list(&mut self, list: &DrawList) {
        self.renderer.set_draw_list(list);
        // The last framing was of nothing (the host's hook framed an empty scene): this list is
        // the first with something in it, so it is framed at the next frame, as the render view
        // frames its first non-empty scene.
        if self.framed_empty && !self.renderer.scene_bounds().empty() {
            self.framed = false;
            self.framed_empty = false;
        }
        self.update();
    }

    /// The same for a scene of layers drawn with their own depth rules (GpuRenderer::set_layers,
    /// packed against `origin` when one is given).
    pub fn set_layers(&mut self, layers: &[LayerSource<'_>], origin: Option<Vec3>) {
        self.renderer.set_layers(layers, origin);
        // As set_draw_list: the first scene with something in it after an empty framing is
        // framed at the next frame.
        if self.framed_empty && !self.renderer.scene_bounds().empty() {
            self.framed = false;
            self.framed_empty = false;
        }
        // From prepare_frame the frame being drawn uploads it: asking for another would draw
        // every change twice.
        if !self.in_frame {
            self.update();
        }
    }

    /// One layer that changed alone (GpuRenderer::update_layer): a selection, faded edges.
    pub fn update_layer(&mut self, index: usize, list: &DrawList) {
        self.renderer.update_layer(index, list);
        if !self.in_frame {
            self.update();
        }
    }

    /// What the layers are packed against (GpuRenderer::origin).
    pub fn scene_origin(&self) -> Vec3 {
        self.renderer.origin()
    }

    pub fn set_frame_settings(&mut self, settings: FrameSettings) {
        self.settings = settings;
        self.update();
    }

    pub fn frame_settings(&self) -> &FrameSettings {
        &self.settings
    }

    /// False for an orthographic elevation view, which is a measured drawing: a left drag then
    /// pans instead of orbiting, as the render view does.
    pub fn set_orbit_allowed(&mut self, allowed: bool) {
        self.orbit_allowed = allowed;
    }

    /// Whether a software device (WARP, lavapipe) may draw this view; by default it is refused as
    /// a failure, so the host falls back to the software rasteriser
    /// (RendererDecision::software_device_allowed).
    pub fn set_software_device_allowed(&mut self, allowed: bool) {
        self.software_device_allowed = allowed;
    }

    /// Whether the camera already frames the scene, so the first frame must leave it alone: the
    /// host passes ViewState::camera_framed (for a state whose camera_kind matches its kind, as
    /// the render view checks) before the view first draws. False, the default, frames on the
    /// first frame.
    pub fn set_camera_framed(&mut self, framed: bool) {
        self.framed = framed;
        self.framed_empty = false;
    }

    /// True once the camera frames something drawn - framed by this view, or said to be by the
    /// host - for the host to write back into ViewState::camera_framed. A frame of an empty scene
    /// does not count: the first draw list with something in it is framed again.
    pub fn camera_framed(&self) -> bool {
        self.framed && !self.framed_empty
    }

    /// True once the view has failed to render; the host should swap in the software view.
    pub fn failed(&self) -> bool {
        self.failed
    }

    pub fn last_stats(&self) -> &GpuFrameStats {
        &self.stats
    }

    /// Fails the view, saying why, the first time only: RenderFailed is raised once. What a lost
    /// device or a pipeline that does not build raises; public, as the C++ hook a test calls
    /// directly is.
    pub fn fail(&mut self, reason: &str) {
        if self.failed {
            return;
        }
        self.failed = true;
        self.events
            .push(GpuSceneEvent::RenderFailed(String::from(reason)));
    }

    /// Frames the scene: the host's zoom_extents when it sets one (the host knows what "the
    /// scene" is), otherwise the packed lists' bounds - and nothing, for now, when they are empty.
    pub fn zoom_extents(&mut self, host: &mut dyn GpuSceneHost) {
        self.frame_scene(host);
        self.update();
    }

    /// zoom_extents without asking for a frame, for the first frame.
    fn frame_scene(&mut self, host: &mut dyn GpuSceneHost) {
        self.keep_camera_logical(host); // frame() fits the camera's aspect
        if host.zoom_extents(self) {
            // The host frames whatever it frames for an empty scene, and asks for a frame:
            // `framed` is set even then, or every frame would call it again. `framed_empty` has
            // the next non-empty list framed anyway. Read after the hook: the host may hand over
            // its scene as it frames.
            self.framed = true;
            self.framed_empty = self.renderer.scene_bounds().empty();
            return;
        }
        let mut bounds = self.renderer.scene_bounds();
        if bounds.empty() {
            // Nothing to frame yet: `framed` stays as it was, so a first frame that came before
            // the draw list leaves the framing to the frame after it arrives, rather than to the
            // user pressing E.
            return;
        }
        // What is drawn is exaggerated in the shader; frame what is drawn.
        let k = self.settings.vertical_exaggeration;
        let datum = self.settings.exaggeration_datum;
        let low = datum + (bounds.min.z - datum) * k;
        let high = datum + (bounds.max.z - datum) * k;
        bounds.min.z = katana_math::cpp::min(low, high);
        bounds.max.z = katana_math::cpp::max(low, high);
        if host.camera().frame(bounds) {
            self.framed = true;
            self.framed_empty = false;
        }
    }

    /// The host's camera in this view's logical pixels, as the render view keeps it: what the
    /// mouse moves it by, and what the software view needs.
    fn keep_camera_logical(&self, host: &mut dyn GpuSceneHost) {
        host.camera()
            .set_viewport_size(self.width.max(1), self.height.max(1));
    }

    /// QWidget::resizeEvent: the view is `width` by `height` logical pixels now, and the host's
    /// camera with it.
    pub fn resize(&mut self, host: &mut dyn GpuSceneHost, width: i32, height: i32) {
        self.width = width.max(1);
        self.height = height.max(1);
        self.keep_camera_logical(host);
        self.dirty = true;
    }

    /// Device pixels per logical pixel of the colour buffer actually drawn.
    fn pixel_ratio(&self) -> f64 {
        if let Some(target) = &self.target
            && self.width > 0
        {
            return f64::from(target.width()) / f64::from(self.width);
        }
        self.device_ratio
    }

    /// The target at the view's size in device pixels, made when there is none or the size
    /// changed; the renderer made for it the first time. False, having failed the view, when it
    /// cannot be: QRhiWidget's initialize().
    fn ensure_target(&mut self) -> bool {
        let api = to_string(GpuBackend::built());
        let Some(device) = self.device.clone() else {
            // Qt said only that it had no QRhi for the widget: no device of the API here, or a
            // window that is never shown (a headless run). Here the window gave it none.
            self.fail(&format!(
                "the GPU view could not render: the window made it no {api} device - none \
                 here, or its window is never shown"
            ));
            return false;
        };
        if !self.software_device_allowed && device.is_software() {
            self.fail(&format!(
                "the only {api} device is a software one ({}); the software rasteriser draws \
                 faster on the CPU",
                device.device_name()
            ));
            return false;
        }
        let ratio = if self.device_ratio.is_finite() && self.device_ratio > 0.0 {
            self.device_ratio
        } else {
            1.0
        };
        let wide = ((f64::from(self.width) * ratio).round() as i32).max(1);
        let high = ((f64::from(self.height) * ratio).round() as i32).max(1);
        if self
            .target
            .as_ref()
            .is_some_and(|target| target.width() == wide && target.height() == high)
        {
            return true;
        }
        let options = OffscreenOptions {
            device: if device.is_software() {
                GpuDevice::Software
            } else {
                GpuDevice::Hardware
            },
            sample_count: SAMPLE_COUNT,
            timestamps: false,
        };
        let made = match OffscreenGpu::create_on(&device, wide, high, &options) {
            Ok(made) => made,
            Err(error) => {
                self.target = None;
                self.fail(&error.describe());
                return false;
            }
        };
        // The view's renderer is made once for a target of these formats, on this device: a new
        // target of the same formats - a resize - draws with the same pipelines, as the C++ kept
        // its compatible render pass.
        if !self.renderer.initialised() {
            let format = TargetFormat {
                color: COLOR_FORMAT,
                depth: DEPTH_FORMAT,
                sample_count: made.sample_count(),
            };
            if let Err(error) =
                self.renderer
                    .initialise(device.device(), device.queue(), format)
            {
                self.target = None;
                self.fail(&error.describe());
                return false;
            }
        }
        self.target = Some(made);
        true
    }

    /// One frame drawn and read back: the picture, at the view's size in device pixels, or None
    /// once the view has failed. QRhiWidget::grabFramebuffer, which draws afresh.
    pub fn grab_framebuffer(&mut self, host: &mut dyn GpuSceneHost) -> Option<Image> {
        self.render(host)
    }

    /// QRhiWidget::render: frames the scene on the first frame, lets the host prepare it, draws
    /// it and reads it back.
    fn render(&mut self, host: &mut dyn GpuSceneHost) -> Option<Image> {
        if self.failed || !self.ensure_target() {
            return None;
        }
        self.keep_camera_logical(host);
        if !self.framed {
            self.frame_scene(host); // no update(): this IS the frame
        }
        // The frame is drawn in the pixels actually drawn - device pixels - so the image is not
        // upscaled on a HiDPI display. Through a copy: the host's camera stays in logical pixels,
        // where the render view and its rasteriser keep it.
        let target = self.target.as_ref()?;
        let (wide, high) = (target.width(), target.height());
        let mut frame_camera = *host.camera();
        frame_camera.set_viewport_size(wide, high);
        self.in_frame = true;
        host.prepare_frame(self, &mut frame_camera);
        self.in_frame = false;
        let mut settings = self.settings;
        settings.pixel_ratio = self.pixel_ratio();

        let started = Instant::now();
        let mut pixels: Vec<Rgba> = Vec::new();
        let result = self.draw_frame(&frame_camera, &settings, &mut pixels);
        let milliseconds = started.elapsed().as_secs_f64() * 1.0e3;
        self.dirty = false;
        match result {
            Ok(stats) => {
                self.stats = stats;
                self.events.push(GpuSceneEvent::FrameStats(format!(
                    "GPU  {} tri  {} ms",
                    stats.triangles,
                    qt_fixed(milliseconds, 1)
                )));
                Some(image_of(&pixels, wide, high))
            }
            Err(error) => {
                self.fail(&error.describe());
                None
            }
        }
    }

    /// The renderer lent to the target for one frame, and taken back.
    fn draw_frame(
        &mut self,
        camera: &Camera,
        settings: &FrameSettings,
        pixels: &mut Vec<Rgba>,
    ) -> Result<GpuFrameStats> {
        let Some(target) = self.target.as_mut() else {
            return Err(katana_core::Error::new(
                katana_core::ErrorCode::InvalidState,
                "the GPU view has no colour buffer",
            ));
        };
        std::mem::swap(target.renderer(), &mut self.renderer);
        let result = target.render_frame(camera, settings, Some(pixels));
        std::mem::swap(target.renderer(), &mut self.renderer);
        result
    }

    // ---- interaction: the render view's, in logical pixels ------------------------------------

    pub fn mouse_press(&mut self, at: PointF, button: MouseButton, modifiers: KeyModifiers) {
        self.events.push(GpuSceneEvent::Activated);
        self.last_mouse = at;
        let pan_modifier = modifiers.shift;
        self.drag = match button {
            MouseButton::Middle => Drag::Pan,
            MouseButton::Left if pan_modifier => Drag::Pan,
            MouseButton::Left if self.orbit_allowed => Drag::Orbit,
            MouseButton::Left => Drag::Pan,
            _ => Drag::None,
        };
    }

    pub fn mouse_move(&mut self, host: &mut dyn GpuSceneHost, at: PointF) {
        if self.drag == Drag::None {
            return;
        }
        let delta = at - self.last_mouse;
        self.last_mouse = at;
        if self.drag == Drag::Orbit {
            // Radians per LOGICAL pixel: a turn takes the same hand movement at any display
            // scale.
            host.camera()
                .orbit(-delta.x * ORBIT_PER_PIXEL, delta.y * ORBIT_PER_PIXEL);
        } else {
            host.camera().pan_pixels(delta.x, delta.y);
        }
        self.update();
    }

    pub fn mouse_release(&mut self) {
        self.drag = Drag::None;
    }

    pub fn mouse_double_click(&mut self, host: &mut dyn GpuSceneHost) {
        self.zoom_extents(host);
    }

    /// The wheel turned over `at` by `angle_delta` eighths of a degree, 120 a notch: zoomed
    /// through the host when it has the hook, about the target's plane otherwise. True when the
    /// event was taken (QWheelEvent::accept); a turn of nothing is not.
    pub fn wheel(&mut self, host: &mut dyn GpuSceneHost, at: PointF, angle_delta: f64) -> bool {
        let notches = angle_delta / 120.0;
        if notches == 0.0 {
            return false;
        }
        if !host.wheel_zoom(self, notches, at) {
            host.camera().dolly_at_pixel(
                (1.0 / Camera::ZOOM_PER_NOTCH).powf(notches),
                at.x,
                at.y,
            );
            self.update();
        }
        true
    }

    /// A key: true when the view took it.
    pub fn key_press(&mut self, host: &mut dyn GpuSceneHost, key: SceneKey) -> bool {
        if let Some(view) = key.standard_view() {
            host.camera().set_standard_view(view);
            self.update();
            return true;
        }
        match key {
            SceneKey::P => {
                let camera = host.camera();
                camera.set_projection(toggled(camera.projection()));
                self.update();
                true
            }
            SceneKey::E => {
                self.zoom_extents(host);
                true
            }
            _ => false,
        }
    }
}

/// A GPU view when choose_renderer(environment) says the GPU, and None with the decision's reason
/// otherwise: the one call a host makes to decide between this view and the software one. The
/// view gets the decision's permission for a software device.
pub fn make_gpu_scene_view_if_chosen(
    environment: &RendererEnvironment,
    decision: Option<&mut RendererDecision>,
) -> Option<GpuSceneView> {
    let chosen = choose_renderer(environment);
    let kind = chosen.kind;
    let allowed = chosen.software_device_allowed;
    if let Some(decision) = decision {
        *decision = chosen;
    }
    if kind != RendererKind::Gpu {
        return None;
    }
    let mut view = GpuSceneView::new();
    view.set_software_device_allowed(allowed);
    Some(view)
}

/// Framebuffer pixels (0xAARRGGBB, row-major, top row first) as an image of `width` by `height`.
pub(crate) fn image_of(pixels: &[Rgba], width: i32, height: i32) -> Image {
    let (wide, high) = (width.max(0) as u32, height.max(0) as u32);
    let mut bytes = Vec::with_capacity(pixels.len() * 4);
    for &pixel in pixels {
        bytes.extend_from_slice(&[
            red_of(pixel),
            green_of(pixel),
            blue_of(pixel),
            alpha_of(pixel),
        ]);
    }
    Image::from_straight_rgba8(wide, high, bytes).unwrap_or_default()
}
