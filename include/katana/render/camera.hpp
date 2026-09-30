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
//   Clip       x, y in [-w, w]; z in [0, w] with REVERSED Z: NDC depth 1 at
//              the near plane and 0 at the far plane, nearer is LARGER. Float
//              depth is dense near 0 and sparse near 1, and a perspective
//              divide crowds the far scene towards one end; reversing the
//              range puts the dense end of the float where the divide crowds,
//              so the step between depths stays about 2^-23 of the distance
//              from the eye at any distance and almost whatever the near
//              plane. Measured for an eye inside the scene (the case of
//              RenderDepth.WithTheEyeInsideTheScene...: near = far * 1e-6,
//              far 2 km, a strip 5 cm above the ground 0.7-1.3 km ahead):
//              the strip and the ground behind it round to the SAME float
//              depth at 93.9% of distances in the standard range, and at
//              none in the reversed one. The GPU convention (D32F, compare
//              Greater) is the same.
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

    // Frames `bounds` so the whole box is visible with a small margin, from
    // the current direction: the eight corners are projected and the distance
    // (perspective) and height (orthographic) chosen so they just fit, on the
    // tighter of the two screen axes. It used to fit the bounding SPHERE,
    // which left a long corridor as a sliver in the middle of the view (1.5%
    // of the pixels on a 12 km archive). A box smaller than
    // kMinimumFrameRadius is grown to it first (audit REN-07), and the depth
    // range is fitted to the box (fitDepthRange). A box that is empty or not
    // finite leaves the camera alone and returns false.
    bool frame(const katana::math::AABB& bounds, double marginFraction = 0.06);

    // Half-diagonal below which frame() grows the box: one survey point, or a
    // drawing whose every entity is coincident, is framed as a 2 m patch
    // rather than a 1e-7 m one that no amount of zooming out recovers.
    static constexpr double kMinimumFrameRadius = 1.0;

    // Fits the near and far planes to `bounds` for the camera as it now is.
    // Call it every frame, after any orbit, pan or zoom: a depth range fixed
    // at frame() let a zoom-out push the model past the far plane (blank view
    // after 8 wheel notches) and wasted the whole depth buffer on the
    // space in front of the model (audit REN-02, REN-05).
    //
    //   Perspective   near and far bracket the box's corners along the view
    //                 direction; when the eye is inside the box the near plane
    //                 falls back to far * kNearFarFloor, which reversed Z can
    //                 afford (see Clip above) - or to distance() *
    //                 kNearPivotFloor when that is nearer. A zoom into a large
    //                 scene brings the pivot, which is what it zoomed towards,
    //                 closer than a millionth of the far plane, and a floor
    //                 tied to the far plane alone cut it away: one stray
    //                 entity at the origin of a survey at MGA coordinates put
    //                 the floor 5 m out (docs/render.md, "Zooming towards the
    //                 cursor"). Never below tol::kGeometric.
    //   Orthographic  the eye is moved back along the view direction until
    //                 the whole box is in front of it (orthographicStandoff),
    //                 because an orthographic view has no reason to cut away
    //                 what lies behind an eye that a zoom-in moved into the
    //                 model - the elevation views lost their front that way.
    //                 What the view shows does not change: under an
    //                 orthographic projection the eye's distance only moves
    //                 the planes.
    //
    // False, with the camera untouched, for an empty or non-finite box.
    bool fitDepthRange(const katana::math::AABB& bounds);
    static constexpr double kNearFarFloor = 1.0e-6;
    // A thousandth, as for a box wholly behind the eye: what the pivot keeps
    // between itself and the near plane, so that the ground round what a
    // zoom went towards - nearer than it towards the bottom of the view - is
    // not cut either. Flat ground at the bottom edge of a 45 degree view
    // looking 5 degrees below level at the pivot is a tenth of the pivot's
    // distance away: sin 5 / sin 50 = 0.11.
    static constexpr double kNearPivotFloor = 1.0e-3;

    // How far behind `distance()` fitDepthRange had to put an orthographic
    // eye to keep the scene in front of it; 0 when it did not need to.
    [[nodiscard]] double orthographicStandoff() const { return standoff_; }

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

    // World -> screen pixels with depth in [0, 1] (1 at the near plane, 0 at
    // the far: reversed Z). nullopt when the point is behind the eye plane
    // (where a projection is meaningless), so callers cannot silently draw a
    // mirrored point.
    [[nodiscard]] std::optional<Point3> project(const Point3& world) const;

    // Ray from the eye through the centre of pixel (x, y), for picking. Under
    // an orthographic projection the origin moves and the direction is fixed.
    [[nodiscard]] katana::math::Ray rayThroughPixel(double x, double y) const;

    // World units per pixel at the distance of the target. The number a
    // viewport needs to turn a pixel aperture into a model-space tolerance.
    [[nodiscard]] double worldPerPixel() const;
    // World units per pixel at a given distance in front of the eye: the
    // size of a pixel's footprint there. Constant under an orthographic
    // projection, proportional to the depth under a perspective one.
    [[nodiscard]] double worldPerPixelAt(double depth) const;

    // ---- interaction ------------------------------------------------------------

    // What one wheel notch zooms by: 2x in five notches. Both 3D widgets and
    // cad::zoomAtPixel count in it, so the software and GPU views cannot
    // drift apart.
    static constexpr double kZoomPerNotch = 1.15;

    void orbit(double deltaAzimuth, double deltaElevation);
    // Slides the target in the view plane by a screen displacement in pixels:
    // a point on the target's plane moves exactly with the drag.
    void panPixels(double dx, double dy);
    // Multiplies the viewing distance (and the orthographic height with it, so
    // an orthographic view zooms rather than sitting still). Values <= 0 ignored.
    void dolly(double factor);
    // Zoom centred on a pixel: the point under that pixel OF THE TARGET'S
    // PLANE (through the target, facing the eye) stays under it. Under a
    // perspective projection that point is not the ground under the pixel
    // but in the air in front of it or under it, and notch after notch the
    // eye closes on it - the magnification of the ground stalls, or the eye
    // goes through the ground. So a 3D view moves the pivot to what the
    // cursor points at first - what is drawn there, else the datum or the
    // depth the scene reaches (setPivotDepth, cad::zoomAnchor,
    // cad::zoomAtPixel; docs/render.md, "Zooming towards the cursor").
    void dollyAtPixel(double factor, double x, double y);
    // Moves the pivot along the view axis to `depth` in front of the eye: the
    // target to eye() + forward() * depth and the distance to `depth`. The eye
    // stays where it is, so nothing on screen moves; what moves is everything
    // the pivot's distance governs - the plane dollyAtPixel anchors on and a
    // pan drags, the point an orbit turns about, the scale the edges fade by.
    // Under a perspective projection the orthographic height becomes what
    // the view shows at that depth, 2 depth tan(fov / 2), so switching to
    // orthographic keeps the scale there; under an orthographic one the
    // height is the picture and is kept, and the standoff is folded into the
    // distance. A depth that is not positive and finite is ignored.
    void setPivotDepth(double depth);

  private:
    Point3 target_{0.0, 0.0, 0.0};
    double distance_ = 100.0;
    // -45 degrees: the eye to the south-east, looking north-west (forward()),
    // as StandardView::IsoSouthEast.
    double azimuth_ = -0.785398163397448309616;
    double elevation_ = 0.615479708670387341067;   // ~35.264 degrees: true isometric
    Projection projection_ = Projection::Perspective;
    double fovY_ = 0.785398163397448309616;        // 45 degrees
    double orthoHeight_ = 100.0;
    double near_ = 0.1;
    double far_ = 100000.0;
    int width_ = 0;
    int height_ = 0;
    double standoff_ = 0.0; // orthographic only; see fitDepthRange
};

} // namespace katana::render
