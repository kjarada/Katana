//! Mapping between model space (y up, model units) and a viewport in pixels
//! (y down) (include/katana/cad/view_transform.hpp). Pure maths, shared by
//! every 2D view and unit tested without a window.

use katana_geometry::{Box2, Point2};
use katana_math::cpp;

/// The margin fit leaves when the caller has no reason to choose: the C++
/// default argument.
pub const DEFAULT_FIT_MARGIN: f64 = 0.05;
/// The least spacing grid_spacing keeps lines apart when the caller has no
/// reason to choose: the C++ default argument.
pub const DEFAULT_GRID_PIXELS: f64 = 12.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewTransform {
    /// The model point shown at the viewport centre.
    pub center: Point2,
    /// Pixels per model unit.
    pub scale: f64,
    pub width_pixels: f64,
    pub height_pixels: f64,
}

impl Default for ViewTransform {
    fn default() -> Self {
        Self {
            center: Point2::new(0.0, 0.0),
            scale: 1.0,
            width_pixels: 1.0,
            height_pixels: 1.0,
        }
    }
}

impl ViewTransform {
    /// Zoom limits in pixels per model unit. Beyond these a double can no
    /// longer resolve a pixel at survey coordinate magnitudes (1e7 m), so
    /// panning and picking would become erratic rather than merely useless.
    pub const MINIMUM_SCALE: f64 = 1e-7;
    pub const MAXIMUM_SCALE: f64 = 1e7;

    pub fn resize(&mut self, width: f64, height: f64) {
        self.width_pixels = cpp::max(1.0, width);
        self.height_pixels = cpp::max(1.0, height);
    }

    pub fn world_to_screen(&self, world: Point2) -> Point2 {
        Point2::new(
            0.5 * self.width_pixels + (world.x - self.center.x) * self.scale,
            0.5 * self.height_pixels - (world.y - self.center.y) * self.scale,
        )
    }

    pub fn screen_to_world(&self, screen: Point2) -> Point2 {
        Point2::new(
            self.center.x + (screen.x - 0.5 * self.width_pixels) / self.scale,
            self.center.y - (screen.y - 0.5 * self.height_pixels) / self.scale,
        )
    }

    /// Length conversion, e.g. a pick aperture in pixels to model units.
    pub fn pixels_to_world(&self, pixels: f64) -> f64 {
        pixels / self.scale
    }

    /// Drags the drawing with the cursor.
    pub fn pan_by_pixels(&mut self, dx: f64, dy: f64) {
        self.center.x -= dx / self.scale;
        self.center.y += dy / self.scale;
    }

    /// Zooms by `factor` (> 1 zooms in) keeping the model point under
    /// `screen` fixed. The scale is clamped to the limits above.
    pub fn zoom_at(&mut self, screen: Point2, factor: f64) {
        if !(factor.is_finite() && factor > 0.0) {
            return;
        }
        let anchor = self.screen_to_world(screen);
        self.scale = cpp::clamp(
            self.scale * factor,
            Self::MINIMUM_SCALE,
            Self::MAXIMUM_SCALE,
        );
        // Re-centre so that `anchor` is still under `screen` at the new scale.
        self.center.x = anchor.x - (screen.x - 0.5 * self.width_pixels) / self.scale;
        self.center.y = anchor.y + (screen.y - 0.5 * self.height_pixels) / self.scale;
    }

    /// Shows all of `bounds` with a margin (DEFAULT_FIT_MARGIN is the C++
    /// default). Empty or degenerate bounds centre the view without changing
    /// scale sensibly (scale 1 for empty bounds).
    pub fn fit(&mut self, bounds: &Box2, margin_fraction: f64) {
        if bounds.empty() {
            self.center = Point2::new(0.0, 0.0);
            self.scale = 1.0;
            return;
        }
        self.center = bounds.center();
        let usable = cpp::max(0.0, 1.0 - 2.0 * margin_fraction);
        let scale_x = if bounds.width() > 0.0 {
            usable * self.width_pixels / bounds.width()
        } else {
            Self::MAXIMUM_SCALE
        };
        let scale_y = if bounds.height() > 0.0 {
            usable * self.height_pixels / bounds.height()
        } else {
            Self::MAXIMUM_SCALE
        };
        let fitted = cpp::min(scale_x, scale_y);
        // A single point has no extent to fit, so the zoom level is kept; the
        // centre above still moves to it. That is the ONLY case that keeps the
        // current scale. A box with real extent always sets the scale, clamped
        // - testing `fitted` against MAXIMUM_SCALE instead would also skip a
        // box that is merely too small to fill the window at maximum zoom, and
        // Zoom Extents would then leave a small drawing off screen.
        if bounds.width() <= 0.0 && bounds.height() <= 0.0 {
            return;
        }
        self.scale = cpp::clamp(fitted, Self::MINIMUM_SCALE, Self::MAXIMUM_SCALE);
    }

    pub fn visible_world_bounds(&self) -> Box2 {
        let mut bounds = Box2::default();
        bounds.expand(self.screen_to_world(Point2::new(0.0, 0.0)));
        bounds.expand(self.screen_to_world(Point2::new(self.width_pixels, self.height_pixels)));
        bounds
    }
}

/// Spacing of grid lines, in model units, such that adjacent lines are at
/// least `minimum_pixels` apart (DEFAULT_GRID_PIXELS is the C++ default): a
/// 1-2-5 sequence (..., 0.1, 0.2, 0.5, 1, 2, 5, 10, ...).
pub fn grid_spacing(scale: f64, minimum_pixels: f64) -> f64 {
    if !(scale > 0.0) || !(minimum_pixels > 0.0) {
        return 1.0;
    }
    let needed = minimum_pixels / scale; // smallest acceptable spacing in model units
    let decade = 10.0_f64.powf(needed.log10().floor());
    for step in [1.0, 2.0, 5.0] {
        if step * decade >= needed {
            return step * decade;
        }
    }
    10.0 * decade
}
