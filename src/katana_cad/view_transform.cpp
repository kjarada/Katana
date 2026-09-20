#include "katana/cad/view_transform.hpp"

#include <algorithm>
#include <cmath>

namespace katana::cad {

using katana::geometry::Box2;
using katana::geometry::Point2;

void ViewTransform::resize(double width, double height)
{
    widthPixels = std::max(1.0, width);
    heightPixels = std::max(1.0, height);
}

Point2 ViewTransform::worldToScreen(const Point2& world) const
{
    return Point2(0.5 * widthPixels + (world.x - center.x) * scale,
                  0.5 * heightPixels - (world.y - center.y) * scale);
}

Point2 ViewTransform::screenToWorld(const Point2& screen) const
{
    return Point2(center.x + (screen.x - 0.5 * widthPixels) / scale,
                  center.y - (screen.y - 0.5 * heightPixels) / scale);
}

void ViewTransform::panByPixels(double dx, double dy)
{
    center.x -= dx / scale;
    center.y += dy / scale;
}

void ViewTransform::zoomAt(const Point2& screen, double factor)
{
    if (!(std::isfinite(factor) && factor > 0.0)) {
        return;
    }
    const Point2 anchor = screenToWorld(screen);
    scale = std::clamp(scale * factor, kMinimumScale, kMaximumScale);
    // Re-centre so that `anchor` is still under `screen` at the new scale.
    center.x = anchor.x - (screen.x - 0.5 * widthPixels) / scale;
    center.y = anchor.y + (screen.y - 0.5 * heightPixels) / scale;
}

void ViewTransform::fit(const Box2& bounds, double marginFraction)
{
    if (bounds.empty()) {
        center = Point2(0.0, 0.0);
        scale = 1.0;
        return;
    }
    center = bounds.center();
    const double usable = std::max(0.0, 1.0 - 2.0 * marginFraction);
    const double scaleX = bounds.width() > 0.0 ? usable * widthPixels / bounds.width()
                                               : kMaximumScale;
    const double scaleY = bounds.height() > 0.0 ? usable * heightPixels / bounds.height()
                                                : kMaximumScale;
    const double fitted = std::min(scaleX, scaleY);
    // A single point has no extent to fit, so the zoom level is kept; the
    // centre above still moves to it. That is the ONLY case that keeps the
    // current scale. A box with real extent always sets the scale, clamped -
    // testing `fitted` against kMaximumScale instead would also skip a box
    // that is merely too small to fill the window at maximum zoom, and Zoom
    // Extents would then leave a small drawing off screen.
    if (bounds.width() <= 0.0 && bounds.height() <= 0.0) {
        return;
    }
    scale = std::clamp(fitted, kMinimumScale, kMaximumScale);
}

Box2 ViewTransform::visibleWorldBounds() const
{
    Box2 box;
    box.expand(screenToWorld(Point2(0.0, 0.0)));
    box.expand(screenToWorld(Point2(widthPixels, heightPixels)));
    return box;
}

double gridSpacing(double scale, double minimumPixels)
{
    if (!(scale > 0.0) || !(minimumPixels > 0.0)) {
        return 1.0;
    }
    const double needed = minimumPixels / scale; // smallest acceptable spacing in model units
    const double decade = std::pow(10.0, std::floor(std::log10(needed)));
    for (const double step : {1.0, 2.0, 5.0}) {
        if (step * decade >= needed) {
            return step * decade;
        }
    }
    return 10.0 * decade;
}

} // namespace katana::cad
