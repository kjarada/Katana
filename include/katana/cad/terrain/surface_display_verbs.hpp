#pragma once

// What the views show of a surface in its display style, on the command line
// (docs/surface_display.md, "On the command line"): the plan view and the 3D
// view draw a surface for a person, and these say the same in words for an
// agent, a script and katana_cli - the legend of its colours, and what the
// plan view draws of it at a framing.
//
//   SURFACE LEGEND <name> [3D] [JSON]
//       The colours of its faces in its style: in plan (planFaces), or in 3D
//       (faces3d, over the surface's own heights - a 3D view stretches an
//       open range over every visible surface, which a verb has no view to
//       know). One record a band, stop or sector after a head record:
//         legend surface="ground" style="Elevation Bands" view=plan faces=elevation kind=bands entries=10
//         entry index=1 colour=#28468C from=24.892000198364258 to=26.284700202941895 label="24.892 - 26.285"
//       from and to are the edges exactly (the sample terrain's heights are
//       Float32, so they show their stored values); the label is the rounded
//       form a person reads. kind is bands, gradient (a smooth ramp or smooth
//       stops: entries are colours from lowest to highest), sectors (aspect),
//       flat or none. A band that takes in everything past an edge has no
//       from or to there. JSON: the same as one object.
//
//   SURFACE PLAN <name> [AREA x0,y0,x1,y1 | VIEW [<id>]] [pixels=W,H]
//       What the plan view draws of it (cad/terrain/surface_plan.hpp) looking
//       at AREA (default: the whole surface) through a view of W x H pixels
//       (default 1600,1000), or at what the window's plan view shows - its own
//       area at its own size and scale, so what is stroked, tinted or hidden
//       is what that view drew; pixels=W,H frames the view's area as another
//       size instead. VIEW EXTENTS is refused: a plan is looked at through an
//       area. How many triangles are in view, stroked and tinted, the
//       contours, labels, points and pixels. One record, plus the scope's.
//
// Both only read, so neither is an undo step. A surface hidden in the
// drawing is still described (visible=no says so): hiding is the picture's,
// and the question is what its style would draw. Refused: no such surface
// (NotFound, naming it), a word neither verb takes, a bad AREA (not four
// numbers, or one with no width and no height) or pixels=.

#include <span>
#include <string>
#include <string_view>

#include "katana/cad/document.hpp"
#include "katana/cad/scope_verbs.hpp"
#include "katana/core/error.hpp"

namespace katana::cad {

// SURFACE followed by LEGEND or PLAN (any case): this module's lines. The
// other SURFACE words are other modules'.
[[nodiscard]] bool isSurfaceDisplayVerb(std::string_view verb, std::span<const std::string> args);

// `args` are the words after SURFACE. VIEW asks `views`; headless there is
// none, and VIEW is refused naming AREA.
[[nodiscard]] katana::core::Result<std::string>
runSurfaceDisplayVerb(const Document& document, std::span<const std::string> args,
                      const ScopeViewProvider& views);

// The lines HELP prints for them.
[[nodiscard]] std::string surfaceDisplayVerbHelp();

} // namespace katana::cad
