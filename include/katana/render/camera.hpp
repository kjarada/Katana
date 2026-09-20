#pragma once

// Orbiting 3D camera (PLAN.MD Phase 15).
//
// CONVENTIONS, all of which the rasteriser and the pick ray depend on:
//
//   World      right-handed, Z UP. Survey and civil data is Z-up - elevations
//              are z, not y - and converting at the door would mean every
//              picked coordinate had to be converted back.
//   Azimuth    radians CCW from +X about +Z. Elevation is measured from the XY
//              plane, positive looking down at the ground from above.
//   Eye space  right-handed, looking down -Z, +Y up, +X right.
//   Clip       x, y in [-w, w]; z in [0, w], i.e. NDC depth 0 at the near plane
//              and 1 at the far plane. This is the Vulkan range rather than
//              OpenGL's [-1, 1]: Phase 15's real backend is Vulkan, and a float
//              depth buffer resolves far more of [0, 1] than of [-1, 1].
//   Screen     pixels, origin TOP-LEFT, +y DOWN (so NDC y is flipped).
//
// The camera is a value: copying one is how a viewport records the view it had
// before an orbit, and how four tiled viewports each keep their own.

#include <optional>

#include "katana/geometry/primitives3d.hpp"
#include "katana/math/mat4.hpp"
#include "katana/math/primitives.hpp"

namespace katana::render {

using katana::geometry::Point3;
using katana::math::Mat4;
using katana::math::Vec3;

enum class Projection { Perspective, Orthographic };

// The standard CAD view directions. Plan is what a 2D drawing shows.
enum class StandardView {
    Top, // plan
    Bottom,
    Front, // looking north, i.e. from -Y
    Back,
    Left,
    Right,
    IsoSouthWest,
    IsoSouthEast,
    IsoNorthEast,
    IsoNorthWest,
};

[[nodiscard]] const char* toString(StandardView view);

class Camera {
  public:
    Camera() = default;

    // ---- framing ---------------------------------------------------------------

    // Orbit pivot: what pan moves and what dolly moves towards.
    [[nodiscard]] const Point3& target() const { return target_; }
    void setTarget(const Point3& target);

    [[nodiscard]] double distance() const { return distance_; }
    // Clamped to a positive minimum: a zero distance has no view direction.
    void setDistance(double distance);

    [[nodiscard]] double azimuth() const { return azimuth_; }
    [[nodiscard]] double elevation() const { return elevation_; }
    // Elevation is clamped just inside the poles so `up` never becomes parallel
    // to the view direction; azimuth wraps.
    void setOrientation(double azimuth, double elevation);

    void setStandardView(StandardView view);

    // Frames `bounds` so the whole box is visible with a small margin. A box
    // that is empty or not finite leaves the camera alone and returns false.
    bool frame(const katana::math::AABB& bounds, double marginFraction = 0.06);

    // ---- projection ------------------------------------------------------------

    [[nodiscard]] Projection projection() const { return projection_; }
    void setProjection(Projection projection);

    [[nodiscard]] double fieldOfView() const { return fovY_; }
    void setFieldOfView(double radians);

    // World units visible vertically under an orthographic projection.
    [[nodiscard]] double orthographicHeight() const { return orthoHeight_; }
    void setOrthographicHeight(double height);

    [[nodiscard]] double nearPlane() const { return near_; }
    [[nodiscard]] double farPlane() const { return far_; }
    // Both must be positive with near < far; ignored otherwise.
    void setDepthRange(double nearPlane, double farPlane);

    // Pixels. A zero or negative size makes the camera produce no matrices.
    void setViewportSize(int width, int height);
    [[nodiscard]] int viewportWidth() const { return width_; }
    [[nodiscard]] int viewportHeight() const { return height_; }
    [[nodiscard]] double aspect() const;

    // ---- derived ---------------------------------------------------------------

    [[nodiscard]] Vec3 forward() const; // unit, eye -> target
    [[nodiscard]] Vec3 right() const;   // unit, screen +x
    [[nodiscard]] Vec3 up() const;      // unit, screen +y (i.e. screen UP)
    [[nodiscard]] Point3 eye() const;

    [[nodiscard]] Mat4 viewMatrix() const;
    [[nodiscard]] Mat4 projectionMatrix() const;
    [[nodiscard]] Mat4 viewProjection() const { return projectionMatrix() * viewMatrix(); }

    // World -> screen pixels with depth in [0, 1]. nullopt when the point is
    // behind the near plane (where a projection is meaningless), so callers
    // cannot silently draw a mirrored point.
    [[nodiscard]] std::optional<Point3> project(const Point3& world) const;

    // Ray from the eye through the centre of pixel (x, y), for picking. Under
    // an orthographic projection the origin moves and the direction is fixed.
    [[nodiscard]] katana::math::Ray rayThroughPixel(double x, double y) const;

    // World units per pixel at the distance of the target. The number a
    // viewport needs to turn a pixel aperture into a model-space tolerance.
    [[nodiscard]] double worldPerPixel() const;

    // ---- interaction ------------------------------------------------------------

    void orbit(double deltaAzimuth, double deltaElevation);
    // Slides the target in the view plane by a screen displacement in pixels.
    void panPixels(double dx, double dy);
    // Multiplies the viewing distance (and the orthographic height with it, so
    // an orthographic view zooms rather than sitting still). Values <= 0 ignored.
    void dolly(double factor);
    // Zoom centred on a pixel: the world point under that pixel stays under it.
    void dollyAtPixel(double factor, double x, double y);

  private:
    Point3 target_{0.0, 0.0, 0.0};
    double distance_ = 100.0;
    double azimuth_ = -0.785398163397448309616;    // -45 degrees: looking NE
    double elevation_ = 0.615479708670387341067;   // ~35.264 degrees: true isometric
    Projection projection_ = Projection::Perspective;
    double fovY_ = 0.785398163397448309616;        // 45 degrees
    double orthoHeight_ = 100.0;
    double near_ = 0.1;
    double far_ = 100000.0;
    int width_ = 0;
    int height_ = 0;
};

} // namespace katana::render
