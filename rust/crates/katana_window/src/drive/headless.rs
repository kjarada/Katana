//! The headless runner: the window drawn with no display and no GPU, as the C++ drew it under
//! `QT_QPA_PLATFORM=offscreen` (the plan's sections 4.1, 4.3 and 4.5). One egui context, the
//! registry installed on it, a fixed screen, a fixed time step, and the soft renderer keeping the
//! textures the frames deliver, so the last frame can be written as a PNG.
//!
//! THE SCREEN is the C++'s headless window: 1360 x 860 points (MainWindow::restoreSession's size
//! for a headless run), at a pixel a point - or at `QT_SCALE_FACTOR` pixels a point when the
//! variable says so, as Qt reads it, so a test's environment means the same to both programs (the
//! plan's section 4.6, item 3). At 1.25 the picture is 1700 x 1075 pixels, the C++'s.
//!
//! THE IDLE RULE (D's decision; the C++'s two `processEvents()` after a step stood for it). After a
//! step the runner draws frames until the window is idle: a frame after which
//!
//! ```text
//! no repaint is asked           egui's root viewport asks for no repaint at once (an animation,
//!                               a scroll, a dialog frame measuring what it holds)
//! no job is running             DrivenWindow::busy is false
//! the mailbox is empty          no act waits for its control
//! no act was taken in it        a frame that took an act drew the controls before the taker
//!                               with the state from before it, so one more frame is drawn
//! ```
//!
//! A frame in which a job runs is not counted, and the runner waits a little before the next, for
//! as long as the job takes: the C++ window waited on its jobs with no limit of its own (a
//! headless run's JobRunner::waitFor), and ctest's timeout ends a run that hangs. Every other frame
//! counts, and a window that is not idle after `MOST_FRAMES` of them fails the step: "the window
//! did not settle in 100 frames". An act refused, or a name drawn twice in one target, fails it at
//! once, in the registry's words (drive::harness has the same rule for the tests).
//!
//! Nothing here touches winit or wgpu: a headless run opens no window (the step-7 brief, "Never open
//! a window").

use std::time::Duration;

use egui::epaint::ClippedShape;
use egui::{Color32, Context, Event, Pos2, Rect, TextureId, Ui, ViewportId};
use katana_core::path_text::environment_variable;
use katana_core::text::parse_finite_double;

use super::harness::{HEADLESS_HEIGHT, HEADLESS_WIDTH};
use super::mailbox::{ActId, ActOutcome, Posted};
use super::program::DrivenWindow;
use super::registry::{Snapshot, install, outcome, post, snapshot, waiting};
use super::soft_renderer::{SoftRenderer, crop_rect};
use crate::canvas::Image;

/// The variable Qt reads a screen's scale from, honoured by a headless run until the C++ goes.
pub const SCALE_FACTOR_VARIABLE: &str = "QT_SCALE_FACTOR";

/// The most frames, a job aside, a step waits for the window to come idle. An egui animation takes
/// its `animation_time` (a twelfth of a second, five frames); a dialog's first show takes a
/// measuring frame and a frame to place it; a hundred is far more than any of these, and the
/// harness the tests use has the same.
pub const MOST_FRAMES: usize = 100;

/// The time a frame takes on the runner's clock: a sixtieth of a second, egui's own guess.
const FRAME_SECONDS: f64 = 1.0 / 60.0;

/// How long the runner waits before the next frame while a job runs: a job's completion is
/// delivered by a frame, and a frame drawn again at once would only spin.
const BUSY_WAIT: Duration = Duration::from_millis(5);

/// The scale `QT_SCALE_FACTOR`'s text gives, as Qt takes it: a positive number, else none (1).
pub fn scale_factor(text: &[u8]) -> f32 {
    match parse_finite_double(katana_core::text::trimmed(text)) {
        Some(scale) if scale > 0.0 => scale as f32,
        _ => 1.0,
    }
}

/// What a frame is drawn from: the window, or what a test draws.
trait Drawn {
    fn draw(&mut self, ui: &mut Ui);
    fn busy(&self) -> bool;
}

struct WindowDrawn<'a, W: ?Sized>(&'a mut W);

impl<W: DrivenWindow + ?Sized> Drawn for WindowDrawn<'_, W> {
    fn draw(&mut self, ui: &mut Ui) {
        self.0.ui(ui);
    }

    fn busy(&self) -> bool {
        self.0.busy()
    }
}

struct UiDrawn<F>(F);

impl<F: FnMut(&mut Ui)> Drawn for UiDrawn<F> {
    fn draw(&mut self, ui: &mut Ui) {
        (self.0)(ui);
    }

    fn busy(&self) -> bool {
        false
    }
}

/// The headless runner (see the module comment).
pub struct Headless {
    ctx: Context,
    screen: Rect,
    pixels_per_point: f32,
    time: f64,
    most_frames: usize,
    events: Vec<Event>,
    posted: Vec<ActId>,
    frames: u64,
    renderer: SoftRenderer,
    /// The last frame's shapes and the pixels a point it was drawn at: what a screenshot draws.
    shapes: Vec<ClippedShape>,
    shapes_pixels_per_point: f32,
    /// The textures the last frame freed, dropped once it has been drawn (egui frees after
    /// painting).
    to_free: Vec<TextureId>,
}

impl Default for Headless {
    fn default() -> Self {
        Self::new()
    }
}

impl Headless {
    /// The headless window, 1360 x 860 points, a pixel a point.
    pub fn new() -> Self {
        Self::with_pixels_per_point(1.0)
    }

    /// The headless window at `pixels_per_point` pixels a point.
    pub fn with_pixels_per_point(pixels_per_point: f32) -> Self {
        Self::with_screen(HEADLESS_WIDTH, HEADLESS_HEIGHT, pixels_per_point)
    }

    /// A screen of `width` by `height` points at `pixels_per_point`.
    pub fn with_screen(width: f32, height: f32, pixels_per_point: f32) -> Self {
        let ctx = Context::default();
        install(&ctx);
        Self {
            ctx,
            screen: Rect::from_min_size(Pos2::ZERO, egui::vec2(width, height)),
            pixels_per_point,
            time: 0.0,
            most_frames: MOST_FRAMES,
            events: Vec::new(),
            posted: Vec::new(),
            frames: 0,
            renderer: SoftRenderer::new(),
            shapes: Vec::new(),
            shapes_pixels_per_point: pixels_per_point,
            to_free: Vec::new(),
        }
    }

    /// The headless window at the scale `QT_SCALE_FACTOR` gives, a pixel a point without it.
    pub fn from_environment() -> Self {
        let text = environment_variable(Some(SCALE_FACTOR_VARIABLE));
        Self::with_pixels_per_point(scale_factor(&text))
    }

    pub fn context(&self) -> &Context {
        &self.ctx
    }

    /// The screen, in points.
    pub fn screen(&self) -> Rect {
        self.screen
    }

    pub fn pixels_per_point(&self) -> f32 {
        self.pixels_per_point
    }

    /// How many frames have been drawn.
    pub fn frames(&self) -> u64 {
        self.frames
    }

    /// A cap of frames other than `MOST_FRAMES`, for a test of the cap.
    pub fn set_most_frames(&mut self, frames: usize) {
        self.most_frames = frames;
    }

    /// An input event for the next frame: the pointer, a key, the wheel.
    pub fn push_event(&mut self, event: Event) {
        self.events.push(event);
    }

    /// Posts an act to the mailbox, for the frames after: its outcome is in `act_outcome`, and the
    /// next settling fails on its refusal.
    pub fn post(&mut self, posted: Posted) -> Option<ActId> {
        let id = post(&self.ctx, posted)?;
        self.posted.push(id);
        Some(id)
    }

    /// What became of act `id`.
    pub fn act_outcome(&self, id: ActId) -> ActOutcome {
        outcome(&self.ctx, id)
    }

    /// The last frame's records.
    pub fn snapshot(&self) -> Snapshot {
        snapshot(&self.ctx)
    }

    /// One frame of `window`; whether it asked to be drawn again at once.
    pub fn frame<W: DrivenWindow + ?Sized>(&mut self, window: &mut W) -> bool {
        self.frame_drawn(&mut WindowDrawn(window))
    }

    /// One frame of `ui`; whether it asked to be drawn again at once.
    pub fn frame_ui(&mut self, ui: impl FnMut(&mut Ui)) -> bool {
        self.frame_drawn(&mut UiDrawn(ui))
    }

    /// Frames of `window` until it is idle (the module comment's rule).
    pub fn settle<W: DrivenWindow + ?Sized>(&mut self, window: &mut W) -> Result<(), String> {
        self.settle_drawn(&mut WindowDrawn(window))
    }

    /// Frames of `ui` until it is idle.
    pub fn settle_ui(&mut self, ui: impl FnMut(&mut Ui)) -> Result<(), String> {
        self.settle_drawn(&mut UiDrawn(ui))
    }

    /// `posted` posted, and frames of `window` until it is idle: the act's refusal, or the
    /// settling's failure, as the error.
    pub fn act<W: DrivenWindow + ?Sized>(
        &mut self,
        window: &mut W,
        posted: Posted,
    ) -> Result<(), String> {
        self.post(posted);
        self.settle(window)
    }

    /// `act`, for what a test draws.
    pub fn act_ui(&mut self, ui: impl FnMut(&mut Ui), posted: Posted) -> Result<(), String> {
        self.post(posted);
        self.settle_ui(ui)
    }

    fn frame_drawn(&mut self, drawn: &mut dyn Drawn) -> bool {
        for id in std::mem::take(&mut self.to_free) {
            self.renderer.free_texture(id);
        }
        let mut input = egui::RawInput {
            screen_rect: Some(self.screen),
            time: Some(self.time),
            predicted_dt: FRAME_SECONDS as f32,
            events: std::mem::take(&mut self.events),
            ..egui::RawInput::default()
        };
        input
            .viewports
            .entry(ViewportId::ROOT)
            .or_default()
            .native_pixels_per_point = Some(self.pixels_per_point);
        let output = self.ctx.run_ui(input, |ui| drawn.draw(ui));
        self.time += FRAME_SECONDS;
        self.frames += 1;
        self.renderer.set_textures(&output.textures_delta);
        self.to_free = output.textures_delta.free.iter().copied().collect();
        self.shapes = output.shapes;
        self.shapes_pixels_per_point = output.pixels_per_point;
        output
            .viewport_output
            .get(&ViewportId::ROOT)
            .is_some_and(|viewport| viewport.repaint_delay == Duration::ZERO)
    }

    fn settle_drawn(&mut self, drawn: &mut dyn Drawn) -> Result<(), String> {
        let mut counted = 0;
        loop {
            let had_acts = !self.posted.is_empty();
            let repaint = self.frame_drawn(drawn);
            if let Some(problem) = self.problem() {
                return Err(problem);
            }
            if drawn.busy() {
                std::thread::sleep(BUSY_WAIT);
                continue;
            }
            counted += 1;
            if !had_acts && !repaint && self.posted.is_empty() && waiting(&self.ctx) == 0 {
                return Ok(());
            }
            if counted >= self.most_frames {
                return Err(format!(
                    "the window did not settle in {} frames",
                    self.most_frames
                ));
            }
        }
    }

    /// The first refusal of an act posted, or the first name drawn twice in a target; the acts
    /// decided are forgotten.
    fn problem(&mut self) -> Option<String> {
        let mut refused = None;
        let mut still = Vec::new();
        for id in std::mem::take(&mut self.posted) {
            match outcome(&self.ctx, id) {
                ActOutcome::Waiting => still.push(id),
                ActOutcome::Done => {}
                ActOutcome::Refused(sentence) => {
                    refused.get_or_insert(sentence);
                }
            }
        }
        self.posted = still;
        refused.or_else(|| self.snapshot().duplicates.into_iter().next())
    }

    /// The pixels the screen is: the C++ window's 1360 x 860 times the scale, rounded.
    pub fn screen_pixels(&self) -> (u32, u32) {
        let width = (self.screen.width() * self.pixels_per_point)
            .round()
            .max(0.0) as u32;
        let height = (self.screen.height() * self.pixels_per_point)
            .round()
            .max(0.0) as u32;
        (width, height)
    }

    /// The last frame, drawn by the soft renderer: the whole screen, on the window's own colour.
    pub fn render(&self) -> Image {
        let primitives = self
            .ctx
            .tessellate(self.shapes.clone(), self.shapes_pixels_per_point);
        let (width, height) = self.screen_pixels();
        let ground: Color32 = crate::theme::palette::window().to_egui();
        self.renderer.render(
            &primitives,
            width,
            height,
            self.shapes_pixels_per_point,
            ground,
        )
    }

    /// The last frame's picture of target `name` (the window is `registry::WINDOW`): its rectangle
    /// as the registry recorded it, cropped as `crop_rect` says. Refused for a target the last frame
    /// did not draw.
    pub fn screenshot(&self, name: &str) -> Result<Image, String> {
        let frame = self.snapshot();
        let Some(target) = frame.target(name) else {
            return Err(format!("the window drew no target {name} to grab"));
        };
        let whole = self.render();
        let crop = crop_rect(
            target.rect,
            self.shapes_pixels_per_point,
            whole.width(),
            whole.height(),
        );
        Ok(whole.copy(crop))
    }

    /// The last frame's picture of `rect` (points), cropped as a target's is.
    pub fn picture_of(&self, rect: Rect) -> Image {
        let whole = self.render();
        let crop = crop_rect(
            rect,
            self.shapes_pixels_per_point,
            whole.width(),
            whole.height(),
        );
        whole.copy(crop)
    }
}

#[cfg(test)]
mod tests {
    use super::scale_factor;

    // Qt takes a positive number and ignores anything else.
    #[test]
    #[allow(clippy::float_cmp, reason = "each scale is read back exactly")]
    fn the_scale_factor_is_a_positive_number_or_none() {
        assert_eq!(scale_factor(b"1.25"), 1.25);
        assert_eq!(scale_factor(b" 2 "), 2.0);
        assert_eq!(scale_factor(b""), 1.0);
        assert_eq!(scale_factor(b"0"), 1.0);
        assert_eq!(scale_factor(b"-1"), 1.0);
        assert_eq!(scale_factor(b"big"), 1.0);
    }
}
