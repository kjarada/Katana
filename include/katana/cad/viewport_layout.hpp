#pragma once

// Tiled viewports (PLAN.MD Phase 08/15).
//
// The drawing area is split into non-overlapping tiles - AutoCAD calls these
// tiled viewports, as against the floating ones on a paper-space layout - each
// showing the same document through its own camera. The point is to have plan,
// a 3D view and a section on screen at once, all live on one model.
//
// This class is the MODEL of that split: which cells exist, where they are in
// normalised coordinates, which is active, and what each is looking at. It owns
// no widgets and draws nothing, so the arithmetic that decides "which viewport
// is under this pixel and where inside it" is unit tested rather than
// discovered by clicking. The Qt container reads it and arranges real widgets
// to match.

#include <cstddef>
#include <optional>
#include <string>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/render/camera.hpp"

namespace katana::cad {

// What a cell shows.
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
// How many cells the layout has. Always >= 1.
[[nodiscard]] std::size_t cellCount(LayoutKind kind);

// A cell's rectangle in normalised [0, 1] coordinates of the drawing area, with
// the origin at the TOP-LEFT to match screen convention.
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

struct ViewportCell {
    ViewKind kind = ViewKind::Plan;
    CellRect rect;
    // Each cell keeps its own camera, so orbiting the 3D view does not disturb
    // the plan beside it. Unused by Plan and Section cells, which have their
    // own 2D transforms, but kept so a cell can change kind without losing it.
    katana::render::Camera camera;
    // For Section cells: which alignment is being cut. Empty means none yet.
    std::string sectionAlignment;
    // Set by the container after a resize; 0 until then.
    int pixelWidth = 0;
    int pixelHeight = 0;
};

class ViewportLayout {
  public:
    // Starts as a single Plan view, which is what opening a drawing should give
    // you.
    ViewportLayout();

    // Replaces the split. Cells that survive keep their kind and camera by
    // position, so switching Single -> Quad leaves the existing view in the
    // top-left rather than resetting it; new cells are given the default kinds
    // for that layout (plan, 3D, section, elevation).
    void setLayout(LayoutKind kind);
    [[nodiscard]] LayoutKind layout() const { return layout_; }

    [[nodiscard]] std::size_t size() const { return cells_.size(); }
    [[nodiscard]] const ViewportCell& cell(std::size_t index) const { return cells_[index]; }
    [[nodiscard]] ViewportCell& cell(std::size_t index) { return cells_[index]; }
    [[nodiscard]] const std::vector<ViewportCell>& cells() const { return cells_; }

    [[nodiscard]] std::size_t activeIndex() const { return active_; }
    [[nodiscard]] const ViewportCell& active() const { return cells_[active_]; }
    [[nodiscard]] ViewportCell& active() { return cells_[active_]; }
    // Out-of-range indices are ignored rather than clamped: silently activating
    // a different viewport than the one asked for is worse than doing nothing.
    void setActiveIndex(std::size_t index);

    // Fails with InvalidArgument for an index outside the layout.
    [[nodiscard]] katana::core::Status setCellKind(std::size_t index, ViewKind kind);

    // The cell containing the normalised point, or nullopt outside [0, 1).
    // Rects are half-open, so a point on a shared border belongs to exactly one
    // cell and a click on a splitter never activates two.
    [[nodiscard]] std::optional<std::size_t> cellAt(double u, double v) const;

    // Records the pixel size of the whole drawing area and updates every cell's
    // pixel size and camera viewport from it. Sizes are floored, and a cell
    // never reports fewer than 1x1 pixels so a framebuffer allocation from it
    // cannot fail on a very narrow window.
    void setPixelSize(int width, int height);
    [[nodiscard]] int pixelWidth() const { return pixelWidth_; }
    [[nodiscard]] int pixelHeight() const { return pixelHeight_; }

    // Pixel rectangle of a cell within the drawing area. Computed from the
    // normalised rect and the recorded size, so it is the single definition the
    // widget layout and the hit test both use.
    [[nodiscard]] katana::core::Result<CellRect> pixelRect(std::size_t index) const;

  private:
    void applyLayout(LayoutKind kind);

    LayoutKind layout_ = LayoutKind::Single;
    std::vector<ViewportCell> cells_;
    std::size_t active_ = 0;
    int pixelWidth_ = 0;
    int pixelHeight_ = 0;
};

// The rectangles of a layout, top-left origin, in normalised coordinates.
// Exposed so the layout can be checked without building a ViewportLayout.
[[nodiscard]] std::vector<CellRect> layoutRects(LayoutKind kind);

} // namespace katana::cad
