//! How a selection looks, in every kind of view
//! (include/katana/cad/selection_style.hpp; the owner's request of 2026-09-30,
//! "when selecting a feature in 2d it gets highlighted in other views";
//! docs/desktop.md, "The selection in every view"). ONE colour for the plan,
//! the 3D and elevation views and the sections - there were two oranges, and
//! the 3D one, (255,190,60), sat beside the BOUNDARY layer's (255,213,79) and
//! the top of the elevation ramp - and the sizes each view draws a selection
//! and a ghost at.
//!
//! A GHOST is a selected entity on a layer the document shows and a view
//! hides: drawn faint in that view, and never picked, snapped to or plotted
//! there, so a feature selected in the design view shows where it is in the
//! as-built view that hides the design layers. A layer the document hides
//! stays hidden everywhere: a ghost says "selected, but hidden in this view",
//! never "selected, though switched off".

use katana_render::framebuffer::{Rgba, rgba};

/// #FF9F1C, the orange the plan view has always drawn a selection in. Not
/// blue: the grips, the active view's stroke and a checked tool are blue.
pub const SELECTION_RED: u8 = 0xFF;
pub const SELECTION_GREEN: u8 = 0x9F;
pub const SELECTION_BLUE: u8 = 0x1C;
pub const SELECTION_COLOR: Rgba = rgba(SELECTION_RED, SELECTION_GREEN, SELECTION_BLUE);

/// Plan and section: a ghost is the selection colour at 60 % (alpha 153) and
/// DOTTED where a selection is dashed (plan) or solid (section), so the two
/// never read alike. In plan a dot is 2 px square, a selection's width, one
/// every 6 px - a dotted pen, one width on and two off - laid down evenly at
/// every angle; a point is a ring of 6 px radius, outside the cross a point is
/// drawn with, which reaches 4 px from it (the plan painter's point marker).
/// In a section 1 px, the width of the crossings it stands among.
pub const GHOST_ALPHA: i32 = 153;
pub const GHOST_PEN_PIXELS: f64 = 2.0;
pub const GHOST_DOT_PITCH_PIXELS: f64 = 3.0 * GHOST_PEN_PIXELS;
pub const GHOST_RING_PIXELS: f64 = 6.0;
pub const GHOST_CROSSING_PIXELS: f64 = 1.0;

/// 3D and elevation, the scene's selection overlay. A selected line is a 3 px
/// core over a 5 px dark casing, which shows a pixel either side of the core
/// on any ground - the light face of a hill as well as the dark sky - and
/// tells it from a 1 px line of any colour. The casing's depth bias is below
/// the core's, so the core wins where the two overlap whichever is drawn
/// first, and above an entity's own (SceneOptions::entity_depth_bias, 1.5), so
/// the casing is not lost behind the line it frames.
pub const SELECTION_CASING_COLOR: Rgba = rgba(16, 18, 22);
pub const SELECTION_CORE_WIDTH: f32 = 3.0;
pub const SELECTION_CASING_WIDTH: f32 = 5.0;
pub const SELECTION_CORE_BIAS: f32 = 2.5;
pub const SELECTION_CASING_BIAS: f32 = 2.0;
/// A selected point: 9 px over an 11 px casing, where an unselected one is 5.
pub const SELECTED_POINT_SIZE: f32 = 9.0;
pub const SELECTED_POINT_CASING: f32 = 11.0;
/// A ghost in 3D: one 1 px line with no casing, the selection colour at 45 %
/// over the view's ground (28, 30, 36) mixed beforehand, since the software
/// rasteriser draws no alpha: 0.45 x (255, 159, 28) + 0.55 x (28, 30, 36) =
/// (130.2, 88.1, 32.4), rounded.
pub const GHOST_COLOR3D: Rgba = rgba(130, 88, 32);
pub const GHOST_WIDTH3D: f32 = 1.0;
