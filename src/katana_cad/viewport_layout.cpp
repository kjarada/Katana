#include "katana/cad/viewport_layout.hpp"

#include <algorithm>
#include <cmath>

namespace katana::cad {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;
using katana::render::StandardView;

namespace {

// The kind each cell gets when a layout grows into it. Ordered so that the
// first cell is always the plan view the user was already working in, and the
// four-up arrangement is the one a civil engineer expects: plan, 3D, section,
// elevation.
constexpr ViewKind kDefaultKinds[] = {ViewKind::Plan, ViewKind::Model3D, ViewKind::Section,
                                      ViewKind::Elevation};

void configureCamera(ViewportCell& cell)
{
    switch (cell.kind) {
    case ViewKind::Model3D:
        cell.camera.setProjection(katana::render::Projection::Perspective);
        cell.camera.setStandardView(StandardView::IsoSouthWest);
        break;
    case ViewKind::Elevation:
        // Orthographic, because a side view is read off with a scale rule and
        // perspective would make that a lie.
        cell.camera.setProjection(katana::render::Projection::Orthographic);
        cell.camera.setStandardView(StandardView::Front);
        break;
    case ViewKind::Plan:
        cell.camera.setProjection(katana::render::Projection::Orthographic);
        cell.camera.setStandardView(StandardView::Top);
        break;
    case ViewKind::Section:
        // A section has its own 2D station/elevation transform; the camera is
        // left as-is so switching back to a model view restores what it had.
        break;
    }
}

} // namespace

const char* toString(ViewKind kind)
{
    switch (kind) {
    case ViewKind::Plan:
        return "Plan";
    case ViewKind::Model3D:
        return "3D";
    case ViewKind::Section:
        return "Section";
    case ViewKind::Elevation:
        return "Elevation";
    }
    return "Unknown";
}

const char* toString(LayoutKind kind)
{
    switch (kind) {
    case LayoutKind::Single:
        return "Single";
    case LayoutKind::SplitVertical:
        return "Two: Vertical";
    case LayoutKind::SplitHorizontal:
        return "Two: Horizontal";
    case LayoutKind::ThreeLeft:
        return "Three: Left";
    case LayoutKind::ThreeTop:
        return "Three: Top";
    case LayoutKind::Quad:
        return "Four: Equal";
    }
    return "Unknown";
}

std::size_t cellCount(LayoutKind kind) { return layoutRects(kind).size(); }

std::vector<CellRect> layoutRects(LayoutKind kind)
{
    switch (kind) {
    case LayoutKind::Single:
        return {CellRect{0.0, 0.0, 1.0, 1.0}};
    case LayoutKind::SplitVertical:
        return {CellRect{0.0, 0.0, 0.5, 1.0}, CellRect{0.5, 0.0, 0.5, 1.0}};
    case LayoutKind::SplitHorizontal:
        return {CellRect{0.0, 0.0, 1.0, 0.5}, CellRect{0.0, 0.5, 1.0, 0.5}};
    case LayoutKind::ThreeLeft:
        return {CellRect{0.0, 0.0, 0.5, 1.0}, CellRect{0.5, 0.0, 0.5, 0.5},
                CellRect{0.5, 0.5, 0.5, 0.5}};
    case LayoutKind::ThreeTop:
        return {CellRect{0.0, 0.0, 1.0, 0.5}, CellRect{0.0, 0.5, 0.5, 0.5},
                CellRect{0.5, 0.5, 0.5, 0.5}};
    case LayoutKind::Quad:
        return {CellRect{0.0, 0.0, 0.5, 0.5}, CellRect{0.5, 0.0, 0.5, 0.5},
                CellRect{0.0, 0.5, 0.5, 0.5}, CellRect{0.5, 0.5, 0.5, 0.5}};
    }
    return {CellRect{0.0, 0.0, 1.0, 1.0}};
}

ViewportLayout::ViewportLayout() { applyLayout(LayoutKind::Single); }

void ViewportLayout::applyLayout(LayoutKind kind)
{
    const std::vector<CellRect> rects = layoutRects(kind);
    std::vector<ViewportCell> next;
    next.reserve(rects.size());

    for (std::size_t i = 0; i < rects.size(); ++i) {
        ViewportCell cell;
        if (i < cells_.size()) {
            // Carried over by position: growing the split must not throw away
            // the view the user had, or every layout change costs them their
            // zoom.
            cell = cells_[i];
        } else {
            cell.kind = kDefaultKinds[std::min<std::size_t>(
                i, sizeof(kDefaultKinds) / sizeof(kDefaultKinds[0]) - 1)];
            configureCamera(cell);
        }
        cell.rect = rects[i];
        next.push_back(std::move(cell));
    }

    cells_ = std::move(next);
    layout_ = kind;
    if (active_ >= cells_.size()) {
        active_ = cells_.empty() ? 0 : cells_.size() - 1;
    }
    setPixelSize(pixelWidth_, pixelHeight_);
}

void ViewportLayout::setLayout(LayoutKind kind) { applyLayout(kind); }

void ViewportLayout::setActiveIndex(std::size_t index)
{
    if (index < cells_.size()) {
        active_ = index;
    }
}

Status ViewportLayout::setCellKind(std::size_t index, ViewKind kind)
{
    if (index >= cells_.size()) {
        return makeError(ErrorCode::InvalidArgument, "no such viewport",
                         std::to_string(index) + " of " + std::to_string(cells_.size()));
    }
    if (cells_[index].kind == kind) {
        return {};
    }
    cells_[index].kind = kind;
    configureCamera(cells_[index]);
    return {};
}

std::optional<std::size_t> ViewportLayout::cellAt(double u, double v) const
{
    if (!std::isfinite(u) || !std::isfinite(v) || u < 0.0 || v < 0.0 || u >= 1.0 || v >= 1.0) {
        return std::nullopt;
    }
    for (std::size_t i = 0; i < cells_.size(); ++i) {
        if (cells_[i].rect.contains(u, v)) {
            return i;
        }
    }
    return std::nullopt;
}

void ViewportLayout::setPixelSize(int width, int height)
{
    pixelWidth_ = std::max(width, 0);
    pixelHeight_ = std::max(height, 0);
    for (std::size_t i = 0; i < cells_.size(); ++i) {
        const auto rect = pixelRect(i);
        if (!rect) {
            continue;
        }
        cells_[i].pixelWidth = static_cast<int>(rect->width);
        cells_[i].pixelHeight = static_cast<int>(rect->height);
        cells_[i].camera.setViewportSize(cells_[i].pixelWidth, cells_[i].pixelHeight);
    }
}

Result<CellRect> ViewportLayout::pixelRect(std::size_t index) const
{
    if (index >= cells_.size()) {
        return makeError(ErrorCode::InvalidArgument, "no such viewport",
                         std::to_string(index) + " of " + std::to_string(cells_.size()));
    }
    const CellRect& rect = cells_[index].rect;
    const double w = static_cast<double>(pixelWidth_);
    const double h = static_cast<double>(pixelHeight_);

    // Edges are rounded, then the size is the difference between them, so
    // adjacent cells share a boundary exactly and the whole area is covered
    // with no one-pixel seam and no overlap.
    const double x0 = std::round(rect.x * w);
    const double y0 = std::round(rect.y * h);
    const double x1 = std::round((rect.x + rect.width) * w);
    const double y1 = std::round((rect.y + rect.height) * h);

    CellRect out;
    out.x = x0;
    out.y = y0;
    // Never zero: a framebuffer of zero width cannot be created, and a viewport
    // that is momentarily one pixel wide during a drag is not an error.
    out.width = std::max(x1 - x0, 1.0);
    out.height = std::max(y1 - y0, 1.0);
    return out;
}

} // namespace katana::cad
