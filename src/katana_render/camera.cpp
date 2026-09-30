#include "katana/render/camera.hpp"

#include <algorithm>
#include <cmath>
#include <limits>

#include "katana/math/numerics.hpp"

namespace katana::render {

namespace tol = katana::math::tolerance;

namespace {

// How close to straight up or straight down the camera may get. Exactly at the
// pole the view direction is parallel to world +Z and `up` is undefined, so the
// basis collapses; a milliradian short of it is visually indistinguishable and
// always well conditioned.
constexpr double kPoleGuard = 1.0e-3;
constexpr double kHalfPi = 1.570796326794896619231;
constexpr double kTwoPi = 6.283185307179586476925;

constexpr double kIsometricElevation = 0.615479708670387341067; // atan(1/sqrt(2))

[[nodiscard]] double wrapAngle(double radians)
{
    if (!std::isfinite(radians)) {
        return 0.0;
    }
    double wrapped = std::fmod(radians, kTwoPi);
    if (wrapped <= -kHalfPi * 2.0) {
        wrapped += kTwoPi;
    } else if (wrapped > kHalfPi * 2.0) {
        wrapped -= kTwoPi;
    }
    return wrapped;
}

} // namespace

const char* toString(StandardView view)
{
    switch (view) {
    case StandardView::Top:
        return "Top";
    case StandardView::Bottom:
        return "Bottom";
    case StandardView::Front:
        return "Front";
    case StandardView::Back:
        return "Back";
    case StandardView::Left:
        return "Left";
    case StandardView::Right:
        return "Right";
    case StandardView::IsoSouthWest:
        return "SW Isometric";
    case StandardView::IsoSouthEast:
        return "SE Isometric";
    case StandardView::IsoNorthEast:
        return "NE Isometric";
    case StandardView::IsoNorthWest:
        return "NW Isometric";
    }
    return "Unknown";
}

// ---- framing -------------------------------------------------------------------

void Camera::setTarget(const Point3& target)
{
    if (target.isFinite()) {
        target_ = target;
    }
}

void Camera::setDistance(double distance)
{
    if (std::isfinite(distance)) {
        distance_ = std::max(distance, tol::kGeometric);
    }
}

void Camera::setOrientation(double azimuth, double elevation)
{
    if (!std::isfinite(azimuth) || !std::isfinite(elevation)) {
        return;
    }
    azimuth_ = wrapAngle(azimuth);
    elevation_ = std::clamp(elevation, -kHalfPi + kPoleGuard, kHalfPi - kPoleGuard);
}

void Camera::setStandardView(StandardView view)
{
    switch (view) {
    case StandardView::Top:
        setOrientation(-kHalfPi, kHalfPi); // looking straight down, north up
        break;
    case StandardView::Bottom:
        setOrientation(-kHalfPi, -kHalfPi);
        break;
    case StandardView::Front:
        setOrientation(-kHalfPi, 0.0); // eye to the south, looking north
        break;
    case StandardView::Back:
        setOrientation(kHalfPi, 0.0);
        break;
    case StandardView::Left:
        setOrientation(3.141592653589793238463, 0.0);
        break;
    case StandardView::Right:
        setOrientation(0.0, 0.0);
        break;
    case StandardView::IsoSouthWest:
        setOrientation(-2.356194490192344928847, kIsometricElevation); // -135 degrees
        break;
    case StandardView::IsoSouthEast:
        setOrientation(-0.785398163397448309616, kIsometricElevation); // -45 degrees
        break;
    case StandardView::IsoNorthEast:
        setOrientation(0.785398163397448309616, kIsometricElevation);
        break;
    case StandardView::IsoNorthWest:
        setOrientation(2.356194490192344928847, kIsometricElevation);
        break;
    }
}

bool Camera::frame(const katana::math::AABB& bounds, double marginFraction)
{
    if (bounds.empty() || !bounds.min.isFinite() || !bounds.max.isFinite()) {
        return false;
    }
    const Point3 centre = bounds.center();
    Vec3 half = (bounds.max - bounds.min) * 0.5;
    if (half.length() < kMinimumFrameRadius) {
        // Grown on every axis to at least r / sqrt(3), so the half-diagonal
        // reaches the minimum whatever shape the box had (audit REN-07).
        const double least = kMinimumFrameRadius / 1.7320508075688772;
        half = Vec3(std::max(half.x, least), std::max(half.y, least), std::max(half.z, least));
    }
    const katana::math::AABB box(centre - half, centre + half);
    const double margin = 1.0 + std::max(marginFraction, 0.0);

    setTarget(centre);

    // Fit the eight projected corners, not the bounding sphere: a sphere is
    // orientation independent, but it fits a 12 km corridor as if it were
    // 12 km tall as well. A box projects inside the hull of its corners, so
    // the corners alone decide. Before the view has a size the aspect is
    // unknown and taken as square; the view frames again at its first size.
    const double a = aspect() > 0.0 ? aspect() : 1.0;
    const double tanV = std::tan(fovY_ * 0.5);
    const double tanH = tanV * a;
    const Vec3 s = right();
    const Vec3 u = up();
    const Vec3 f = forward();
    double needed = 0.0;
    double halfWidth = 0.0;
    double halfHeight = 0.0;
    for (int corner = 0; corner < 8; ++corner) {
        const Vec3 p((corner & 1) != 0 ? box.max.x : box.min.x,
                     (corner & 2) != 0 ? box.max.y : box.min.y,
                     (corner & 4) != 0 ? box.max.z : box.min.z);
        const Vec3 rel = p - centre;
        const double x = std::abs(rel.dot(s));
        const double y = std::abs(rel.dot(u));
        const double z = rel.dot(f); // positive: further from the eye than the target
        // At eye distance d the corner is d + z in front of the eye and must
        // sit inside the frustum with the margin to spare.
        needed = std::max({needed, margin * x / tanH - z, margin * y / tanV - z});
        halfWidth = std::max(halfWidth, x);
        halfHeight = std::max(halfHeight, y);
    }
    setDistance(std::max(needed, half.length() * 1.0e-3));
    // The tighter of the two axes, so a portrait view does not crop the
    // sides (audit REN-08: this ignored the aspect).
    setOrthographicHeight(2.0 * margin * std::max(halfHeight, halfWidth / a));
    fitDepthRange(box);
    return true;
}

bool Camera::fitDepthRange(const katana::math::AABB& bounds)
{
    if (bounds.empty() || !bounds.min.isFinite() || !bounds.max.isFinite()) {
        return false;
    }
    const Vec3 f = forward();
    // A little slack either side, so a vertex exactly on a corner is never
    // cut by rounding and a flat box still has a depth range.
    const double pad = std::max((bounds.max - bounds.min).length() * 1.0e-3, 1.0e-3);
    const auto cornerOf = [&bounds](int corner) {
        return Vec3((corner & 1) != 0 ? bounds.max.x : bounds.min.x,
                    (corner & 2) != 0 ? bounds.max.y : bounds.min.y,
                    (corner & 4) != 0 ? bounds.max.z : bounds.min.z);
    };

    if (projection_ == Projection::Orthographic) {
        // How far the box reaches back towards the eye from the target. The
        // eye must be further back than that or the near plane cuts the
        // model, so it is moved back: only the depths change, never what is
        // seen, because an orthographic ray does not depend on where along
        // the view direction its origin sits.
        double reach = -std::numeric_limits<double>::infinity();
        for (int corner = 0; corner < 8; ++corner) {
            reach = std::max(reach, -(cornerOf(corner) - target_).dot(f));
        }
        standoff_ = std::max(0.0, reach + 2.0 * pad - distance_);
    } else {
        standoff_ = 0.0;
    }

    const Point3 e = eye();
    double nearest = std::numeric_limits<double>::infinity();
    double furthest = -std::numeric_limits<double>::infinity();
    for (int corner = 0; corner < 8; ++corner) {
        const double depth = (cornerOf(corner) - e).dot(f);
        nearest = std::min(nearest, depth);
        furthest = std::max(furthest, depth);
    }
    if (!(furthest > 0.0)) {
        // The whole box is behind the eye: nothing of it can be seen, but the
        // range must stay valid for whatever else is drawn.
        setDepthRange(std::max(distance_ * 1.0e-3, tol::kGeometric), std::max(distance_, 1.0));
        return true;
    }
    const double far = furthest + pad;
    // With the eye inside the box `nearest` is negative and the floor decides;
    // reversed Z keeps its precision down there (header, Clip). The pivot's
    // share of the floor decides only once the pivot is nearer than a
    // thousandth of the far plane, so a view not zoomed that deep keeps the
    // planes it always had.
    const double near =
        std::max(nearest - pad, std::min(far * kNearFarFloor, distance_ * kNearPivotFloor));
    setDepthRange(std::max(near, tol::kGeometric), far);
    return true;
}

// ---- projection ----------------------------------------------------------------

void Camera::setProjection(Projection projection) { projection_ = projection; }

void Camera::setFieldOfView(double radians)
{
    if (std::isfinite(radians)) {
        fovY_ = std::clamp(radians, 1.0e-3, 3.0);
    }
}

void Camera::setOrthographicHeight(double height)
{
    if (std::isfinite(height)) {
        orthoHeight_ = std::max(height, tol::kGeometric);
    }
}

void Camera::setDepthRange(double nearPlane, double farPlane)
{
    if (!std::isfinite(nearPlane) || !std::isfinite(farPlane)) {
        return;
    }
    if (nearPlane <= 0.0 || farPlane <= nearPlane) {
        return;
    }
    near_ = nearPlane;
    far_ = farPlane;
}

void Camera::setViewportSize(int width, int height)
{
    width_ = std::max(width, 0);
    height_ = std::max(height, 0);
}

double Camera::aspect() const
{
    if (width_ <= 0 || height_ <= 0) {
        return 0.0;
    }
    return static_cast<double>(width_) / static_cast<double>(height_);
}

// ---- derived -------------------------------------------------------------------

Vec3 Camera::forward() const
{
    const double ce = std::cos(elevation_);
    // The eye sits at target + distance * (away from the scene); forward is the
    // negation of that, so a positive elevation looks DOWN.
    return Vec3(-ce * std::cos(azimuth_), -ce * std::sin(azimuth_), -std::sin(elevation_));
}

Vec3 Camera::right() const
{
    // Elevation is clamped off the poles, so forward is never parallel to +Z
    // and this cross product is always well conditioned.
    return forward().cross(Vec3(0.0, 0.0, 1.0)).normalized();
}

Vec3 Camera::up() const { return right().cross(forward()).normalized(); }

Point3 Camera::eye() const
{
    // The orthographic standoff moves only the planes (fitDepthRange), so it
    // applies only while the projection is orthographic.
    const double back = projection_ == Projection::Orthographic ? standoff_ : 0.0;
    return target_ - forward() * (distance_ + back);
}

Mat4 Camera::viewMatrix() const
{
    const Vec3 f = forward();
    const Vec3 s = right();
    const Vec3 u = up();
    const Point3 e = eye();
    // Rows are the eye-space basis; the translation column is -basis . eye.
    // Eye space looks down -Z, hence the negated third row.
    return Mat4(s.x, s.y, s.z, -s.dot(e),      //
                u.x, u.y, u.z, -u.dot(e),      //
                -f.x, -f.y, -f.z, f.dot(e),    //
                0.0, 0.0, 0.0, 1.0);
}

Mat4 Camera::projectionMatrix() const
{
    const double a = aspect();
    if (a <= 0.0) {
        return Mat4{};
    }
    if (projection_ == Projection::Perspective) {
        const double t = 1.0 / std::tan(fovY_ * 0.5);
        const double range = far_ - near_;
        // REVERSED Z (see the header): z_clip = near (z_eye + far) / range and
        // w = -z_eye, so depth = near (far + z_eye) / (range * -z_eye): 1 at
        // z_eye = -near, 0 at z_eye = -far.
        return Mat4(t / a, 0.0, 0.0, 0.0,                          //
                    0.0, t, 0.0, 0.0,                              //
                    0.0, 0.0, near_ / range, near_ * far_ / range, //
                    0.0, 0.0, -1.0, 0.0);
    }
    const double halfHeight = orthoHeight_ * 0.5;
    const double halfWidth = halfHeight * a;
    const double range = far_ - near_;
    // Reversed and linear: depth = (z_eye + far) / range, 1 at -near, 0 at -far.
    return Mat4(1.0 / halfWidth, 0.0, 0.0, 0.0,     //
                0.0, 1.0 / halfHeight, 0.0, 0.0,    //
                0.0, 0.0, 1.0 / range, far_ / range, //
                0.0, 0.0, 0.0, 1.0);
}

std::optional<Point3> Camera::project(const Point3& world) const
{
    if (width_ <= 0 || height_ <= 0 || !world.isFinite()) {
        return std::nullopt;
    }
    const katana::math::Vec4 clip =
        viewProjection() * katana::math::Vec4(world.x, world.y, world.z, 1.0);
    if (clip.w <= 0.0) {
        return std::nullopt; // at or behind the eye plane
    }
    const double ndcX = clip.x / clip.w;
    const double ndcY = clip.y / clip.w;
    const double ndcZ = clip.z / clip.w;
    return Point3((ndcX * 0.5 + 0.5) * static_cast<double>(width_),
                  (0.5 - ndcY * 0.5) * static_cast<double>(height_), // screen +y is DOWN
                  ndcZ);
}

katana::math::Ray Camera::rayThroughPixel(double x, double y) const
{
    const Vec3 f = forward();
    if (width_ <= 0 || height_ <= 0) {
        return katana::math::Ray{eye(), f};
    }
    // Pixel centres, so pixel (0, 0) samples the middle of the top-left pixel
    // rather than its corner.
    const double ndcX = ((x + 0.5) / static_cast<double>(width_)) * 2.0 - 1.0;
    const double ndcY = 1.0 - ((y + 0.5) / static_cast<double>(height_)) * 2.0;
    const Vec3 s = right();
    const Vec3 u = up();

    if (projection_ == Projection::Orthographic) {
        const double halfHeight = orthoHeight_ * 0.5;
        const double halfWidth = halfHeight * aspect();
        const Point3 origin = eye() + s * (ndcX * halfWidth) + u * (ndcY * halfHeight);
        return katana::math::Ray{origin, f};
    }
    const double halfHeight = std::tan(fovY_ * 0.5);
    const double halfWidth = halfHeight * aspect();
    const Vec3 direction = (f + s * (ndcX * halfWidth) + u * (ndcY * halfHeight)).normalized();
    return katana::math::Ray{eye(), direction};
}

double Camera::worldPerPixel() const
{
    if (height_ <= 0) {
        return 0.0;
    }
    const double visible = projection_ == Projection::Orthographic
                               ? orthoHeight_
                               : 2.0 * distance_ * std::tan(fovY_ * 0.5);
    return visible / static_cast<double>(height_);
}

double Camera::worldPerPixelAt(double depth) const
{
    if (height_ <= 0) {
        return 0.0;
    }
    const double visible = projection_ == Projection::Orthographic
                               ? orthoHeight_
                               : 2.0 * std::max(depth, 0.0) * std::tan(fovY_ * 0.5);
    return visible / static_cast<double>(height_);
}

// ---- interaction ---------------------------------------------------------------

void Camera::orbit(double deltaAzimuth, double deltaElevation)
{
    setOrientation(azimuth_ + deltaAzimuth, elevation_ + deltaElevation);
}

void Camera::panPixels(double dx, double dy)
{
    if (!std::isfinite(dx) || !std::isfinite(dy)) {
        return;
    }
    const double scale = worldPerPixel();
    // Dragging right moves the SCENE right, so the target moves left; screen +y
    // is down, so a positive dy moves the target up.
    setTarget(target_ - right() * (dx * scale) + up() * (dy * scale));
}

void Camera::dolly(double factor)
{
    if (!std::isfinite(factor) || factor <= 0.0) {
        return;
    }
    setDistance(distance_ * factor);
    // Scaled together, so switching projection mid-session does not change how
    // much of the scene is on screen.
    setOrthographicHeight(orthoHeight_ * factor);
}

void Camera::dollyAtPixel(double factor, double x, double y)
{
    if (!std::isfinite(factor) || factor <= 0.0 || width_ <= 0 || height_ <= 0) {
        return;
    }
    // Anchor on the point of the target plane under the cursor: zoom, then slide
    // the target so that same world point lands back under the same pixel.
    const katana::math::Ray before = rayThroughPixel(x, y);
    const Vec3 f = forward();
    const double denominator = before.direction.dot(f);
    if (std::abs(denominator) < 1.0e-12) {
        dolly(factor);
        return;
    }
    const double t = (target_ - before.origin).dot(f) / denominator;
    const Point3 anchor = before.origin + before.direction * t;

    dolly(factor);

    const katana::math::Ray after = rayThroughPixel(x, y);
    const double denominatorAfter = after.direction.dot(f);
    if (std::abs(denominatorAfter) < 1.0e-12) {
        return;
    }
    const double tAfter = (target_ - after.origin).dot(f) / denominatorAfter;
    const Point3 moved = after.origin + after.direction * tAfter;
    setTarget(target_ + (anchor - moved));
}

void Camera::setPivotDepth(double depth)
{
    if (!std::isfinite(depth) || !(depth > 0.0)) {
        return;
    }
    // setDistance's floor, applied before the target is placed so that the
    // eye stays exactly where it was.
    const double distance = std::max(depth, tol::kGeometric);
    const Point3 pivot = eye() + forward() * distance;
    if (!pivot.isFinite()) {
        return;
    }
    target_ = pivot;
    distance_ = distance;
    if (projection_ == Projection::Perspective) {
        setOrthographicHeight(2.0 * distance_ * std::tan(fovY_ * 0.5));
    } else {
        // eye() is target - forward * (distance + standoff): with the standoff
        // folded into the depth it is the same point. fitDepthRange works
        // the standoff out again at the next frame.
        standoff_ = 0.0;
    }
}

} // namespace katana::render
