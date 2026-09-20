#include "katana/render/camera.hpp"

#include <algorithm>
#include <cmath>

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
    const Vec3 span = bounds.max - bounds.min;
    // The bounding SPHERE, not the box: it is orientation independent, so
    // orbiting after a frame() never pushes a corner out of view.
    const double radius = std::max(0.5 * span.length(), tol::kGeometric);
    const double margin = 1.0 + std::max(marginFraction, 0.0);

    setTarget(bounds.center());

    const double vertical = radius * 2.0 * margin;
    setOrthographicHeight(vertical);

    // Fit the tighter of the two screen axes, or a tall thin window crops the
    // scene horizontally.
    const double a = aspect();
    const double halfVertical = fovY_ * 0.5;
    const double halfHorizontal = std::atan(std::tan(halfVertical) * (a > 0.0 ? a : 1.0));
    const double limiting = std::min(halfVertical, halfHorizontal);
    const double needed = radius * margin / std::max(std::sin(limiting), 1.0e-9);
    setDistance(needed);

    // Keep the whole scene between the planes whatever the projection, with a
    // near plane that still resolves depth: a near of 1e-4 against a far of
    // 1e5 wastes the entire float mantissa on the first millimetre.
    const double far = std::max(needed + radius * 4.0, radius * 8.0);
    setDepthRange(std::max(far * 1.0e-5, tol::kGeometric), far);
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

Point3 Camera::eye() const { return target_ - forward() * distance_; }

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
        const double range = near_ - far_;
        // Depth maps -near -> 0 and -far -> 1 (see the header: Vulkan range).
        return Mat4(t / a, 0.0, 0.0, 0.0,                  //
                    0.0, t, 0.0, 0.0,                      //
                    0.0, 0.0, far_ / range, near_ * far_ / range, //
                    0.0, 0.0, -1.0, 0.0);
    }
    const double halfHeight = orthoHeight_ * 0.5;
    const double halfWidth = halfHeight * a;
    const double range = near_ - far_;
    return Mat4(1.0 / halfWidth, 0.0, 0.0, 0.0,      //
                0.0, 1.0 / halfHeight, 0.0, 0.0,     //
                0.0, 0.0, 1.0 / range, near_ / range, //
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

} // namespace katana::render
