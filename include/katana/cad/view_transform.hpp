#pragma once

// Mapping between model space (y up, model units) and a viewport in pixels
// (y down). Pure maths, shared by every 2D view and unit tested without a GUI.

#include "katana/geometry/primitives2d.hpp"

namespace katana::cad {

struct ViewTransform {
    // Zoom limits in pixels per model unit. Beyond these a double can no longer
    // resolve a pixel at survey coordinate magnitudes (1e7 m), so panning and
    // picking would become erratic rather than merely useless.
    static constexpr double kMinimumScale = 1e-7;
    static constexpr double kMaximumScale = 1e7;

    katana::geometry::Point2 center{0.0, 0.0}; // model point shown at the viewport centre
    double scale = 1.0;                        // pixels per model unit
    double widthPixels = 1.0;
    double heightPixels = 1.0;

    void resize(double width, double height);

    [[nodiscard]] katana::geometry::Point2
    worldToScreen(const katana::geometry::Point2& world) const;
    [[nodiscard]] katana::geometry::Point2
    screenToWorld(const katana::geometry::Point2& screen) const;

    // Length conversion, e.g. a pick aperture in pixels to model units.
    [[nodiscard]] double pixelsToWorld(double pixels) const { return pixels / scale; }

    // Drags the drawing with the cursor.
    void panByPixels(double dx, double dy);

    // Zooms by `factor` (> 1 zooms in) keeping the model point under `screen`
    // fixed. The scale is clamped to the limits above.
    void zoomAt(const katana::geometry::Point2& screen, double factor);

    // Shows all of `bounds` with a margin. Empty or degenerate bounds centre the
    // view without changing scale sensibly (scale 1 for empty bounds).
    void fit(const katana::geometry::Box2& bounds, double marginFraction = 0.05);

    [[nodiscard]] katana::geometry::Box2 visibleWorldBounds() const;
};

// Spacing of grid lines, in model units, such that adjacent lines are at least
// `minimumPixels` apart: a 1-2-5 sequence (…, 0.1, 0.2, 0.5, 1, 2, 5, 10, …).
[[nodiscard]] double gridSpacing(double scale, double minimumPixels = 12.0);

} // namespace katana::cad
