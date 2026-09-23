#include "katana/cad/viewport_layout.hpp"

#include <algorithm>

namespace katana::cad {

using katana::render::StandardView;

namespace {

// The kind each slot of an arrangement opens. Ordered so that the first is
// always the plan view the user was already working in, and the four-up
// arrangement is the one a civil engineer expects: plan, 3D, section,
// elevation.
constexpr ViewKind kDefaultKinds[] = {ViewKind::Plan, ViewKind::Model3D, ViewKind::Section,
                                      ViewKind::Elevation};

} // namespace

void configureCamera(katana::render::Camera& camera, ViewKind kind)
{
    switch (kind) {
    case ViewKind::Model3D:
        camera.setProjection(katana::render::Projection::Perspective);
        camera.setStandardView(StandardView::IsoSouthWest);
        break;
    case ViewKind::Elevation:
        // Orthographic, because a side view is read off with a scale rule and
        // perspective would make that a lie.
        camera.setProjection(katana::render::Projection::Orthographic);
        camera.setStandardView(StandardView::Front);
        break;
    case ViewKind::Plan:
        camera.setProjection(katana::render::Projection::Orthographic);
        camera.setStandardView(StandardView::Top);
        break;
    case ViewKind::Section:
        // A section has its own 2D station/elevation transform; the camera is
        // left as-is so switching back to a model view restores what it had.
        break;
    }
}

ViewKind defaultViewKind(std::size_t slot)
{
    constexpr std::size_t count = sizeof(kDefaultKinds) / sizeof(kDefaultKinds[0]);
    return kDefaultKinds[std::min(slot, count - 1)];
}

std::vector<DockSplit> dockSplits(LayoutKind kind)
{
    // Each step halves `existing`, so the order matters: Quad splits the top
    // row first and then each half downwards, which is what gives four equal
    // quarters rather than one half and two quarters.
    switch (kind) {
    case LayoutKind::Single:
        return {};
    case LayoutKind::SplitVertical:
        return {{0, 1, true}};
    case LayoutKind::SplitHorizontal:
        return {{0, 1, false}};
    case LayoutKind::ThreeLeft:
        return {{0, 1, true}, {1, 2, false}};
    case LayoutKind::ThreeTop:
        return {{0, 1, false}, {1, 2, true}};
    case LayoutKind::Quad:
        return {{0, 1, true}, {0, 2, false}, {1, 3, false}};
    }
    return {};
}

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

} // namespace katana::cad
