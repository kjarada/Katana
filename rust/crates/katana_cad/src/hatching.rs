//! Turning a hatch pattern and a boundary into what a renderer draws
//! (include/katana/cad/hatching.hpp).
//!
//! The same division of labour as dashing.rs: the geometry of the line family
//! lives in `geometry::hatch_lines`, and what lives here is the decision about
//! whether drawing it is worth doing at the current zoom.
//!
//! That decision matters more for hatching than for dashes. A 0.2 m pattern
//! over a 200 m parcel is a thousand lines; zoomed out to where the parcel is
//! forty pixels across, those thousand lines land on forty pixels and read as
//! a solid block - so the honest drawing is a solid block, and it costs one
//! filled polygon instead of a thousand clipped segments. This is the same
//! rule `dashing::should_dash` applies to a linetype that has become finer
//! than the screen can show.

use katana_entity::display::{ResolvedDisplay, resolve_display};
use katana_entity::{Entity, HatchPattern, Model};
use katana_geometry::polygon::hatch_lines_with_offset;
use katana_geometry::{Polyline2, Segment2};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HatchOptions {
    /// Screen pixels per model unit, as the viewport's transform gives it.
    pub view_scale: f64,
    /// Below this on-screen spacing a family is drawn as a solid fill instead.
    /// Three pixels: at two the lines merge into a tone with no gaps visible,
    /// and at one they alias into moire. Chosen to match the threshold
    /// `dashing::should_dash` uses for the same reason.
    pub minimum_spacing_pixels: f64,
}

impl Default for HatchOptions {
    fn default() -> Self {
        Self {
            view_scale: 1.0,
            minimum_spacing_pixels: 3.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HatchDrawing {
    /// The pattern draws nothing - the built-in "none".
    None,
    /// A solid fill, either asked for or too fine to resolve.
    Solid,
    /// The line families, drawn individually.
    Lines,
}

/// What `pattern` should be drawn as at this zoom. A pattern with any family
/// finer than the threshold is drawn solid in its entirety rather than
/// per-family, so a crosshatch does not become half solid and half lines as
/// the user zooms out.
pub fn hatch_drawing(pattern: &HatchPattern, options: &HatchOptions) -> HatchDrawing {
    if pattern.draws_nothing() {
        return HatchDrawing::None;
    }
    if pattern.solid {
        return HatchDrawing::Solid;
    }
    // A non-finite or non-positive scale is a view that has not been set up
    // yet. Drawing the lines then would mean asking for a family whose
    // spacing in pixels is meaningless, so the cheap answer is the safe one.
    if !(options.view_scale > 0.0) {
        return HatchDrawing::Solid;
    }
    for family in &pattern.families {
        if family.spacing * options.view_scale < options.minimum_spacing_pixels {
            return HatchDrawing::Solid;
        }
    }
    HatchDrawing::Lines
}

/// Every family of `pattern` clipped to `boundary`, concatenated in family
/// order.
///
/// Returns an empty list rather than an error when `boundary` cannot be
/// hatched - an open polyline, fewer than three vertices, a pattern that is
/// solid or draws nothing. A renderer asks this question once per entity per
/// frame and has nothing useful to do with a failure; the model has already
/// refused an invalid pattern on the way in, and `geometry::hatch_lines` is
/// where a caller that wants the reason should look.
pub fn hatch_segments(boundary: &Polyline2, pattern: &HatchPattern) -> Vec<Segment2> {
    let mut segments = Vec::new();
    if pattern.solid || pattern.draws_nothing() {
        return segments;
    }
    for family in &pattern.families {
        // A renderer has no use for the reason a family cannot be laid.
        if let Ok(lines) =
            hatch_lines_with_offset(boundary, family.angle, family.spacing, family.offset)
        {
            segments.extend(lines);
        }
    }
    segments
}

/// The pattern an entity resolves to, or None when it is not hatched. Follows
/// the same ByLayer chain as `entity::resolve_display`, which is what resolves
/// the NAME; this looks that name up in the document's table.
pub fn resolve_hatch_pattern<'a>(model: &'a Model, entity: &Entity) -> Option<&'a HatchPattern> {
    resolve_hatch_pattern_of_display(model, &resolve_display(model, entity))
}

/// The same answer for a caller that has already resolved the display (the
/// C++ overload taking a ResolvedDisplay). Every entity drawn used to be
/// resolved TWICE a frame - once by the caller and once inside here - and a
/// resolution walks the layer and style tables and builds three strings.
pub fn resolve_hatch_pattern_of_display<'a>(
    model: &'a Model,
    display: &ResolvedDisplay,
) -> Option<&'a HatchPattern> {
    // An entity may name a pattern that has been removed, exactly as it may
    // name a removed layer. Not hatched is the right reading of that, and it
    // is what the document drew before the pattern existed.
    let pattern = model.hatch_patterns.find(&display.hatch_pattern)?;
    if pattern.draws_nothing() {
        return None;
    }
    Some(pattern)
}
