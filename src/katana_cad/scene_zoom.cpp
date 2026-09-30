#include "katana/cad/scene_zoom.hpp"

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <limits>
#include <vector>

#include "katana/math/numerics.hpp"

namespace katana::cad {

namespace {

using katana::math::AABB;
using katana::math::Vec3;
using katana::render::Camera;
using katana::render::DrawList;
using katana::render::Projection;

// The pixel's ray and a frame about it: w along the ray, u across it as near
// the screen's right as w allows, v = u x w as near the screen's up. In this
// frame the ray is the positive w axis: it meets a triangle where the
// triangle's shadow along w on the (u, v) plane covers the origin, and how far
// a point lies from the ray is its (u, v) part.
struct RayFrame {
    Vec3 origin;
    Vec3 u;
    Vec3 v;
    Vec3 w;

    [[nodiscard]] Vec3 of(const Vec3& p) const
    {
        const Vec3 d = p - origin;
        return {d.dot(u), d.dot(v), d.dot(w)};
    }
};

// How far across the ray, at a distance s along it, a line or a point still
// counts as under the pixel: a cone under a perspective projection, a prism
// under an orthographic one.
struct Aperture {
    double slope = 0.0;
    double base = 0.0;

    [[nodiscard]] double at(double s) const { return slope * s + base; }
};

// Which side of the ray's two planes (u = 0, v = 0) a point lies, and which
// side of the aperture's four. A primitive whose vertices all share one of
// these bits lies wholly beyond that plane, so it cannot meet the ray (the
// low four bits) or come within the aperture (the high four).
constexpr std::uint8_t kRaySides = 0x0F;
constexpr std::uint8_t kApertureSides = 0xF0;

[[nodiscard]] std::uint8_t raySides(const Vec3& q)
{
    return static_cast<std::uint8_t>((q.x > 0.0 ? 0x01 : 0) | (q.x < 0.0 ? 0x02 : 0) |
                                      (q.y > 0.0 ? 0x04 : 0) | (q.y < 0.0 ? 0x08 : 0));
}

// q.x > r is the same comparison as r - q.x < 0, which is how withinAperture
// reads it: the sign of a rounded difference is the sign of the exact one.
// So a line culled here is one withinAperture would refuse.
[[nodiscard]] std::uint8_t apertureSides(const Vec3& q, const Aperture& aperture)
{
    const double r = aperture.at(q.z);
    return static_cast<std::uint8_t>((q.x > r ? 0x10 : 0) | (q.x < -r ? 0x20 : 0) |
                                      (q.y > r ? 0x40 : 0) | (q.y < -r ? 0x80 : 0));
}

// Where the ray - the positive w axis - meets the triangle abc, given in the
// ray's frame: the distance along the ray. Nothing when it misses, lies in the
// triangle's plane or meets it behind the origin.
[[nodiscard]] std::optional<double> alongRay(const Vec3& a, const Vec3& b, const Vec3& c)
{
    // The cull's own test, made here as well, so that a culled pick and a
    // full one decide every triangle alike.
    if ((raySides(a) & raySides(b) & raySides(c)) != 0) {
        return std::nullopt;
    }
    // Twice the signed areas the origin makes with each edge in the (u, v)
    // plane. Two triangles sharing an edge compute its area from the same two
    // vertices, exactly negated (-ffp-contract=off), so the ray cannot slip
    // between them.
    const double ab = a.x * b.y - a.y * b.x;
    const double bc = b.x * c.y - b.y * c.x;
    const double ca = c.x * a.y - c.y * a.x;
    // Inside when none of the three disagrees in sign; a zero is on an edge
    // and counts.
    const bool negative = ab < 0.0 || bc < 0.0 || ca < 0.0;
    const bool positive = ab > 0.0 || bc > 0.0 || ca > 0.0;
    if (negative && positive) {
        return std::nullopt;
    }
    const double sum = ab + bc + ca;
    if (sum == 0.0) {
        return std::nullopt; // the ray lies in the triangle's plane
    }
    // The barycentric weights of a, b and c are bc, ca and ab over the sum.
    const double s = (bc * a.z + ca * b.z + ab * c.z) / sum;
    if (!(s >= 0.0)) {
        return std::nullopt;
    }
    return s;
}

// The point of the segment ab, given in the ray's frame, nearest the ray
// among the part that lies within the aperture and in front of the origin
// (Liang-Barsky against the aperture's four sides and the plane across the
// ray through the origin): its parameter along ab. Nothing when no part does.
[[nodiscard]] std::optional<double> withinAperture(const Vec3& a, const Vec3& b,
                                                   const Aperture& aperture)
{
    double enter = 0.0;
    double leave = 1.0;
    // Keeps the part where g >= 0, g running from ga at a to gb at b.
    const auto keep = [&enter, &leave](double ga, double gb) {
        if (ga < 0.0 && gb < 0.0) {
            return false;
        }
        if (ga < 0.0) {
            enter = std::max(enter, ga / (ga - gb));
        } else if (gb < 0.0) {
            leave = std::min(leave, ga / (ga - gb));
        }
        return enter <= leave;
    };
    const double ra = aperture.at(a.z);
    const double rb = aperture.at(b.z);
    if (!keep(ra - a.x, rb - b.x) || !keep(ra + a.x, rb + b.x) || !keep(ra - a.y, rb - b.y) ||
        !keep(ra + a.y, rb + b.y) || !keep(a.z, b.z)) {
        return std::nullopt;
    }
    // Across the ray the distance is |(a + t (b - a)) in (u, v)|, least where
    // its square's derivative is zero; clamped to the part kept. A segment
    // running along the ray is as near all the way: its nearer end.
    const double du = b.x - a.x;
    const double dv = b.y - a.y;
    const double across = du * du + dv * dv;
    if (!(across > 0.0)) {
        return a.z <= b.z ? enter : leave;
    }
    return std::clamp(-(a.x * du + a.y * dv) / across, enter, leave);
}

// The nearest candidate so far, by depth. A tie keeps the one found first -
// the terrain before the drawing, a lower index before a higher - so equal
// depths resolve the same way every time.
class Nearest {
  public:
    Nearest(const Vec3& eye, const Vec3& forward) : eye_(eye), forward_(forward) {}

    void offer(const Vec3& point, ScenePick::Source source)
    {
        const double depth = (point - eye_).dot(forward_);
        if (!(depth > 0.0)) {
            return; // at or behind the eye: nothing to zoom towards
        }
        if (!pick_ || depth < pick_->depth) {
            pick_ = ScenePick{point, depth, source};
        }
    }

    [[nodiscard]] const std::optional<ScenePick>& pick() const { return pick_; }

  private:
    Vec3 eye_;
    Vec3 forward_;
    std::optional<ScenePick> pick_;
};

// Offers everything of `list` under the pixel. `sides` is scratch.
void scan(const DrawList& list, ScenePick::Source source, const RayFrame& frame,
          const Aperture& aperture, bool cull, std::vector<std::uint8_t>& sides,
          Nearest& nearest)
{
    if (list.empty()) {
        return;
    }
    const std::vector<Vec3>& positions = list.positions;
    if (cull) {
        // A surface's list is triangles alone: the aperture's sides are for
        // lines and points, and cost a third of this pass.
        const bool linework = !list.lines.empty() || !list.points.empty();
        sides.resize(positions.size());
        for (std::size_t i = 0; i < positions.size(); ++i) {
            const Vec3 q = frame.of(positions[i]);
            sides[i] = linework
                           ? static_cast<std::uint8_t>(raySides(q) | apertureSides(q, aperture))
                           : raySides(q);
        }
    }
    for (const auto& triangle : list.triangles) {
        if (cull && (sides[triangle.a] & sides[triangle.b] & sides[triangle.c] & kRaySides) != 0) {
            continue;
        }
        if (const auto s = alongRay(frame.of(positions[triangle.a]),
                                    frame.of(positions[triangle.b]),
                                    frame.of(positions[triangle.c]))) {
            nearest.offer(frame.origin + frame.w * *s, source);
        }
    }
    for (const auto& line : list.lines) {
        if (cull && (sides[line.a] & sides[line.b] & kApertureSides) != 0) {
            continue;
        }
        const Vec3& a = positions[line.a];
        const Vec3& b = positions[line.b];
        if (const auto t = withinAperture(frame.of(a), frame.of(b), aperture)) {
            nearest.offer(a + (b - a) * *t, source);
        }
    }
    for (const auto& point : list.points) {
        if (cull && (sides[point.a] & kApertureSides) != 0) {
            continue;
        }
        const Vec3 q = frame.of(positions[point.a]);
        if (q.z > 0.0 && apertureSides(q, aperture) == 0) {
            nearest.offer(positions[point.a], source);
        }
    }
}

} // namespace

std::optional<ScenePick> pickDrawnPoint(const SceneLayers& layers, const Camera& camera, double x,
                                        double y, const PickOptions& options)
{
    if (camera.viewportWidth() <= 0 || camera.viewportHeight() <= 0 || !std::isfinite(x) ||
        !std::isfinite(y)) {
        return std::nullopt;
    }
    const katana::math::Ray ray = camera.rayThroughPixel(x, y);
    RayFrame frame;
    frame.origin = ray.origin;
    frame.w = ray.direction.normalized();
    const Vec3 right = camera.right();
    frame.u = (right - frame.w * right.dot(frame.w)).normalized();
    frame.v = frame.u.cross(frame.w);

    // A pixel across the ray: an angle (its tangent) under a perspective
    // projection, a length under an orthographic one.
    const double height = static_cast<double>(camera.viewportHeight());
    const double pixels = std::isfinite(options.aperture) ? std::max(options.aperture, 0.0) : 0.0;
    Aperture aperture;
    if (camera.projection() == Projection::Perspective) {
        aperture.slope = pixels * 2.0 * std::tan(camera.fieldOfView() * 0.5) / height;
    } else {
        aperture.base = pixels * camera.orthographicHeight() / height;
    }

    Nearest nearest(camera.eye(), camera.forward());
    std::vector<std::uint8_t> sides;
    scan(layers.terrain, ScenePick::Source::Terrain, frame, aperture, options.cull, sides, nearest);
    scan(layers.entities, ScenePick::Source::Drawing, frame, aperture, options.cull, sides,
         nearest);
    if (nearest.pick()) {
        return nearest.pick();
    }

    // Nothing drawn under the pixel: the datum's plane, but only inside the
    // scene's box in plan, where the grid is drawn. Further out nothing is
    // drawn; zoomAnchor goes on from there.
    if (frame.w.z == 0.0) {
        return std::nullopt;
    }
    const double s = (layers.datum - frame.origin.z) / frame.w.z;
    if (!(s >= 0.0)) {
        return std::nullopt;
    }
    const Vec3 point = frame.origin + frame.w * s;
    const AABB box = sceneDepthBox(layers);
    if (box.empty() || point.x < box.min.x || point.x > box.max.x || point.y < box.min.y ||
        point.y > box.max.y) {
        return std::nullopt;
    }
    nearest.offer(point, ScenePick::Source::Datum);
    return nearest.pick();
}

std::optional<ScenePick> zoomAnchor(const SceneLayers& layers, const Camera& camera, double x,
                                    double y)
{
    if (auto drawn = pickDrawnPoint(layers, camera, x, y)) {
        return drawn;
    }
    if (camera.viewportWidth() <= 0 || camera.viewportHeight() <= 0 || !std::isfinite(x) ||
        !std::isfinite(y)) {
        return std::nullopt;
    }
    const AABB box = sceneDepthBox(layers);
    if (box.empty()) {
        return std::nullopt;
    }
    const Vec3 eye = camera.eye();
    const Vec3 forward = camera.forward();
    // How deep the scene reaches: its box's furthest corner along the view.
    double reach = -std::numeric_limits<double>::infinity();
    for (int corner = 0; corner < 8; ++corner) {
        const Vec3 p((corner & 1) != 0 ? box.max.x : box.min.x,
                     (corner & 2) != 0 ? box.max.y : box.min.y,
                     (corner & 4) != 0 ? box.max.z : box.min.z);
        reach = std::max(reach, (p - eye).dot(forward));
    }
    const katana::math::Ray ray = camera.rayThroughPixel(x, y);
    const Vec3 w = ray.direction.normalized();
    // Depth per unit along the ray: positive for every pixel's ray, which
    // lies inside the frustum.
    const double slope = w.dot(forward);
    if (!(reach > 0.0) || !(slope > 0.0)) {
        return std::nullopt; // the whole scene is behind the eye
    }
    if (w.z != 0.0) {
        const double s = (layers.datum - ray.origin.z) / w.z;
        const Vec3 point = ray.origin + w * s;
        const double depth = (point - eye).dot(forward);
        if (s >= 0.0 && depth > 0.0 && depth <= reach) {
            return ScenePick{point, depth, ScenePick::Source::Datum};
        }
    }
    // As deep as the scene reaches, measured from where the ray starts: the
    // eye, or its own point of an orthographic view's plane.
    const double start = (ray.origin - eye).dot(forward);
    const double along = (reach - start) / slope;
    if (!(along > 0.0)) {
        return std::nullopt;
    }
    return ScenePick{ray.origin + w * along, reach, ScenePick::Source::Reach};
}

double minimumApproach(const Camera& camera, const Vec3& point)
{
    namespace tol = katana::math::tolerance;
    const double nearPlane = tol::kGeometric / Camera::kNearPivotFloor;
    const double height = static_cast<double>(std::max(camera.viewportHeight(), 1));
    // The pixel's angle: a footprint at depth d is d times it.
    const double angle = 2.0 * std::tan(camera.fieldOfView() * 0.5) / height;
    constexpr double kUnitRoundoff = 0.5 * std::numeric_limits<double>::epsilon(); // 2^-53
    constexpr double kRoundings = 16.0;
    const double size = point.isFinite() ? point.length() : 0.0;
    const double precision = kRoundings * kUnitRoundoff * size / (angle * kZoomPrecisionPixels);
    return std::max(nearPlane, precision);
}

ZoomResult zoomAtPixel(const SceneLayers& layers, Camera& camera, double notches, double x,
                       double y)
{
    ZoomResult result;
    if (!std::isfinite(notches) || notches == 0.0 || !std::isfinite(x) || !std::isfinite(y) ||
        camera.viewportWidth() <= 0 || camera.viewportHeight() <= 0) {
        return result;
    }
    // The least factor the minimum approach allows (above 1: none).
    double least = 0.0;
    if (camera.projection() == Projection::Perspective) {
        result.anchor = zoomAnchor(layers, camera, x, y);
        if (result.anchor) {
            camera.setPivotDepth(result.anchor->depth);
        }
        const Vec3 towards = result.anchor ? result.anchor->point : camera.target();
        least = minimumApproach(camera, towards) / camera.distance();
    } else {
        least = 2.0 * minimumApproach(camera, camera.target()) *
                std::tan(camera.fieldOfView() * 0.5) / camera.orthographicHeight();
    }
    // Underflows to 0 for an enormous zoom in, which the limit then catches;
    // an enormous zoom out overflows and is refused below.
    double factor = std::pow(1.0 / Camera::kZoomPerNotch, notches);
    if (factor < 1.0 && factor < least) {
        factor = std::min(1.0, least);
        result.limited = true;
    }
    if (!std::isfinite(factor) || factor == 1.0) {
        return result;
    }
    camera.dollyAtPixel(factor, x, y);
    result.factor = factor;
    return result;
}

} // namespace katana::cad
