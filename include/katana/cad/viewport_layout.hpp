#pragma once

// View kinds and the layout presets (PLAN.MD Phase 08/15, 47).
//
// The drawing area shows several views of one document at once - plan, a 3D
// view, a section - each through its own camera. Until 2026-09-23 they were
// TILES: a ViewportLayout class split the area into fixed cells, computed
// every cell's pixel rectangle and answered which cell a click landed in.
// The views are docks now (view_set.hpp; docs/cad.md, "The workspace"), so Qt
// owns the pixels and the hit test, and a view is identified by an id rather
// than by its position in a split. What survives here is what a dock layout
// still needs from the model:
//
//   * ViewKind, what a view shows;
//   * the presets (LayoutKind) and their pictures (layoutRects), which the
//     View menu offers and draws;
//   * dockSplits, the same pictures as a sequence of dock splits, tested to
//     reproduce layoutRects exactly;
//   * the kind each slot of a preset opens (defaultViewKind) and the camera a
//     fresh view of a kind starts with (configureCamera).
//
// The class and its ViewportCell were retired with their last user, the tiled
// ViewportContainer; tests/cad/test_viewport_layout.cpp says where each of
// their tested properties went.

#include <cstddef>
#include <vector>

#include "katana/render/camera.hpp"

namespace katana::cad {

// What a view shows.
enum class ViewKind {
    Plan,        // 2D top-down; the classic drawing editor
    Model3D,     // orbiting 3D view of surfaces and drawing geometry
    Section,     // elevation against station along an alignment
    Elevation,   // fixed orthographic side view (front/back/left/right)
};

[[nodiscard]] const char* toString(ViewKind kind);

// The standard splits. Named for what they give you rather than for the
// geometry, because that is how they are chosen from a menu.
enum class LayoutKind {
    Single,
    SplitVertical,    // two side by side
    SplitHorizontal,  // two stacked
    ThreeLeft,        // one tall on the left, two stacked on the right
    ThreeTop,         // one wide on top, two side by side beneath
    Quad,             // four equal: the plan/3D/section/elevation arrangement
};

[[nodiscard]] const char* toString(LayoutKind kind);
// How many views the preset arranges. Always >= 1.
[[nodiscard]] std::size_t cellCount(LayoutKind kind);

// A view's place in a preset, in normalised [0, 1] coordinates of the drawing
// area, with the origin at the TOP-LEFT to match screen convention. Half-open,
// so a point on a shared border belongs to exactly one.
struct CellRect {
    double x = 0.0;
    double y = 0.0;
    double width = 1.0;
    double height = 1.0;

    [[nodiscard]] bool contains(double u, double v) const
    {
        return u >= x && u < x + width && v >= y && v < y + height;
    }
    friend bool operator==(const CellRect&, const CellRect&) = default;
};

// The rectangles of a preset, top-left origin, in normalised coordinates, in
// the order its views are placed.
[[nodiscard]] std::vector<CellRect> layoutRects(LayoutKind kind);

// The kind a view gets when an arrangement needs one more than are open:
// plan, 3D, section, elevation - the four-up arrangement a civil engineer
// expects. Slots past the fourth repeat the last.
[[nodiscard]] ViewKind defaultViewKind(std::size_t slot);

// Points a camera the way a fresh view of `kind` looks: a 3D view in
// perspective from the south-west, an elevation orthographic from the front,
// a plan orthographic from the top. A Section leaves the camera alone, so
// switching back to the model kind the camera was set up for restores what it
// had (ViewSet::setKind, ViewState::cameraKind).
void configureCamera(katana::render::Camera& camera, ViewKind kind);

// How an arrangement is built out of docked views (the dock workspace of
// 2026-09-23). View 0 is placed first; each step then splits a view already
// placed in two, putting the next view to the right of it or below it, each
// half the size. Applying the steps to rectangles reproduces layoutRects(kind)
// exactly - a test asserts it - and the Qt workspace applies the same steps
// with QMainWindow::splitDockWidget, so the menu's picture of "Three: Left"
// and the docks it produces cannot drift apart.
struct DockSplit {
    std::size_t existing = 0; // index of a view already placed
    std::size_t added = 0;    // index of the view this step places
    bool sideBySide = true;   // true: `added` to the right; false: below
    friend bool operator==(const DockSplit&, const DockSplit&) = default;
};

// Steps for every view after the first, in order; empty for Single.
[[nodiscard]] std::vector<DockSplit> dockSplits(LayoutKind kind);

} // namespace katana::cad
