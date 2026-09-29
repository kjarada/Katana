#pragma once

// Linked views: what it means for one view to follow another (the owner's
// request of 2026-09-30, "sync views together so i can zoom in a design view
// and the as-built view zooms in too"; docs/desktop.md, "Linked views").
//
// A link keeps its views showing the same place at the same scale. Plan to
// plan, that is the centre and the scale in pixels per unit, EXACTLY: two
// views of different shapes then show the same point in their middles and
// the same distance per pixel, and a wider view simply shows more. Linking
// by the visible extent was rejected: views of different shapes would fit the
// same box at different scales, and each move would round the scale a little
// differently in each, so the views would drift apart over a session.
//
// The membership and who follows whom is cad::ViewSet's (link, follow); the
// rule for one pair of views is here, as a pure function, so that a pair of
// kinds that does not map (a section has no plan position) is one place that
// says no. Only plan views link for now: 3D, elevation and section views need
// the camera in logical pixels first (docs/desktop.md, "Not done").

#include "katana/cad/view_set.hpp"

namespace katana::cad {

// Whether a view of this kind can join the link: plan views only.
[[nodiscard]] bool linkable(ViewKind kind);

// `to` takes what `from` shows, as a linked view follows the view that moved.
// Plan to plan copies the centre and the scale and never the size: each view
// keeps its own shape, its own hidden layers and whether it refits on a
// resize. Every other pair of kinds returns false and leaves `to` as it was.
bool followView(const ViewState& from, ViewState& to);

} // namespace katana::cad
