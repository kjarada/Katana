#pragma once

// How a selection looks, in every kind of view (the owner's request of
// 2026-09-30, "when selecting a feature in 2d it gets highlighted in other
// views"; docs/desktop.md, "The selection in every view"). ONE colour for the
// plan, the 3D and elevation views and the sections - there were two
// oranges, and the 3D one, (255,190,60), sat beside the BOUNDARY layer's
// (255,213,79) and the top of the elevation ramp - and the sizes each view
// draws a selection and a ghost at.
//
// A GHOST is a selected entity on a layer the document shows and a view
// hides: drawn faint in that view, and never picked, snapped to or plotted
// there, so a feature selected in the design view shows where it is in the
// as-built view that hides the design layers. A layer the document hides
// stays hidden everywhere: a ghost says "selected, but hidden in this view",
// never "selected, though switched off".

#include <cstdint>

#include "katana/render/framebuffer.hpp"

namespace katana::cad {

// #FF9F1C, the orange the plan view has always drawn a selection in. Not
// blue: the grips, the active view's stroke and a checked tool are blue.
inline constexpr std::uint8_t kSelectionRed = 0xFF;
inline constexpr std::uint8_t kSelectionGreen = 0x9F;
inline constexpr std::uint8_t kSelectionBlue = 0x1C;
inline constexpr katana::render::Rgba kSelectionColor =
    katana::render::rgba(kSelectionRed, kSelectionGreen, kSelectionBlue);

// Plan and section: a ghost is the selection colour at 60 % (alpha 153) and
// DOTTED where a selection is dashed (plan) or solid (section), so the two
// never read alike. In plan a dot is 2 px square, a selection's width, one
// every 6 px - Qt's dotted pen, one width on and two off - laid down evenly
// at every angle; a point is a ring of 6 px radius, outside the cross a
// point is drawn with, which reaches 4 px from it (the plan painter's
// kPointMarkerPixels). In a section 1 px, the width of the crossings it
// stands among.
inline constexpr int kGhostAlpha = 153;
inline constexpr double kGhostPenPixels = 2.0;
inline constexpr double kGhostDotPitchPixels = 3.0 * kGhostPenPixels;
inline constexpr double kGhostRingPixels = 6.0;
inline constexpr double kGhostCrossingPixels = 1.0;

// 3D and elevation, the scene's selection overlay. A selected line is a 3 px
// core over a 5 px dark casing, which shows a pixel either side of the core
// on any ground - the light face of a hill as well as the dark sky - and
// tells it from a 1 px line of any colour. The casing's depth bias is below
// the core's, so the core wins where the two overlap whichever is drawn
// first, and above an entity's own (SceneOptions::entityDepthBias, 1.5), so
// the casing is not lost behind the line it frames.
inline constexpr katana::render::Rgba kSelectionCasingColor = katana::render::rgba(16, 18, 22);
inline constexpr float kSelectionCoreWidth = 3.0f;
inline constexpr float kSelectionCasingWidth = 5.0f;
inline constexpr float kSelectionCoreBias = 2.5f;
inline constexpr float kSelectionCasingBias = 2.0f;
// A selected point: 9 px over an 11 px casing, where an unselected one is 5.
inline constexpr float kSelectedPointSize = 9.0f;
inline constexpr float kSelectedPointCasing = 11.0f;
// A ghost in 3D: one 1 px line with no casing, the selection colour at 45 %
// over the view's ground (28, 30, 36) mixed beforehand, since the software
// rasteriser draws no alpha: 0.45 x (255, 159, 28) + 0.55 x (28, 30, 36) =
// (130.2, 88.1, 32.4), rounded.
inline constexpr katana::render::Rgba kGhostColor3d = katana::render::rgba(130, 88, 32);
inline constexpr float kGhostWidth3d = 1.0f;

} // namespace katana::cad
