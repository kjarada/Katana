#pragma once

#include <algorithm>
#include <limits>
#include <optional>

#include "katana/math/numerics.hpp"
#include "katana/math/vec3.hpp"

namespace katana::math {

// Plane { p : normal . p + offset = 0 }. `normal` must be unit length for
// signedDistance() to return a true distance.
struct Plane {
    Vec3 normal{0.0, 0.0, 1.0};
    double offset = 0.0;

    constexpr Plane() = default;
    constexpr Plane(const Vec3& normalIn, double offsetIn) : normal(normalIn), offset(offsetIn) {}

    // `normal` is normalised; nullopt when it has zero length.
    [[nodiscard]] static std::optional<Plane> fromPointNormal(const Vec3& point, const Vec3& normal)
    {
        const Vec3 n = normal.normalized();
        if (n == Vec3{}) {
            return std::nullopt;
        }
        return Plane(n, -n.dot(point));
    }

    // Counter-clockwise a,b,c gives a normal by the right-hand rule. nullopt
    // when the points are collinear (|sin| of the corner angle <= kAngular).
    [[nodiscard]] static std::optional<Plane> fromPoints(const Vec3& a, const Vec3& b, const Vec3& c)
    {
        const Vec3 ab = b - a;
        const Vec3 ac = c - a;
        const Vec3 n = ab.cross(ac);
        if (!(n.length() > tolerance::kAngular * ab.length() * ac.length())) {
            return std::nullopt;
        }
        return fromPointNormal(a, n);
    }

    [[nodiscard]] constexpr double signedDistance(const Vec3& point) const
    {
        return normal.dot(point) + offset;
    }

    // Orthogonal projection of `point` onto the plane.
    [[nodiscard]] constexpr Vec3 project(const Vec3& point) const
    {
        return point - normal * signedDistance(point);
    }
};

// Half-line origin + t * direction, t >= 0. `direction` need not be unit length;
// parameters returned by intersection functions are in units of |direction|.
struct Ray {
    Vec3 origin;
    Vec3 direction{1.0, 0.0, 0.0};

    [[nodiscard]] constexpr Vec3 at(double t) const { return origin + direction * t; }
};

// Axis-aligned box. Default constructed boxes are empty (inverted infinite
// bounds) so that they can be grown with expand().
struct AABB {
    Vec3 min{std::numeric_limits<double>::infinity(), std::numeric_limits<double>::infinity(),
             std::numeric_limits<double>::infinity()};
    Vec3 max{-std::numeric_limits<double>::infinity(), -std::numeric_limits<double>::infinity(),
             -std::numeric_limits<double>::infinity()};

    constexpr AABB() = default;
    constexpr AABB(const Vec3& minIn, const Vec3& maxIn) : min(minIn), max(maxIn) {}

    [[nodiscard]] constexpr bool empty() const
    {
        return min.x > max.x || min.y > max.y || min.z > max.z;
    }

    [[nodiscard]] constexpr Vec3 center() const { return (min + max) * 0.5; }
    [[nodiscard]] constexpr Vec3 size() const { return empty() ? Vec3{} : max - min; }

    constexpr void expand(const Vec3& p)
    {
        min = Vec3(std::min(min.x, p.x), std::min(min.y, p.y), std::min(min.z, p.z));
        max = Vec3(std::max(max.x, p.x), std::max(max.y, p.y), std::max(max.z, p.z));
    }

    constexpr void expand(const AABB& other)
    {
        if (!other.empty()) {
            expand(other.min);
            expand(other.max);
        }
    }

    // Boundary inclusive.
    [[nodiscard]] constexpr bool contains(const Vec3& p) const
    {
        return p.x >= min.x && p.x <= max.x && p.y >= min.y && p.y <= max.y && p.z >= min.z &&
               p.z <= max.z;
    }

    // Boundary inclusive: touching boxes intersect.
    [[nodiscard]] constexpr bool intersects(const AABB& o) const
    {
        return !empty() && !o.empty() && min.x <= o.max.x && max.x >= o.min.x &&
               min.y <= o.max.y && max.y >= o.min.y && min.z <= o.max.z && max.z >= o.min.z;
    }
};

// Ray parameter t >= 0 of the hit point. nullopt when the ray is parallel to
// the plane (|n.d| <= kAngular * |d|, including a ray lying in the plane) or the
// plane is behind the origin.
[[nodiscard]] inline std::optional<double> intersect(const Ray& ray, const Plane& plane)
{
    const double denom = plane.normal.dot(ray.direction);
    if (!(std::abs(denom) > tolerance::kAngular * ray.direction.length())) {
        return std::nullopt;
    }
    const double t = -plane.signedDistance(ray.origin) / denom;
    if (t < 0.0) {
        return std::nullopt;
    }
    return t;
}

struct RayBoxHit {
    double tEnter = 0.0; // clamped to 0 when the origin is inside the box
    double tExit = 0.0;
};

// Slab method. Exact zero direction components are handled explicitly so that a
// ray travelling inside a slab plane does not produce NaN from 0 * inf.
[[nodiscard]] inline std::optional<RayBoxHit> intersect(const Ray& ray, const AABB& box)
{
    if (box.empty()) {
        return std::nullopt;
    }
    double tEnter = 0.0;
    double tExit = std::numeric_limits<double>::infinity();

    const double origin[3] = {ray.origin.x, ray.origin.y, ray.origin.z};
    const double dir[3] = {ray.direction.x, ray.direction.y, ray.direction.z};
    const double lo[3] = {box.min.x, box.min.y, box.min.z};
    const double hi[3] = {box.max.x, box.max.y, box.max.z};

    for (int axis = 0; axis < 3; ++axis) {
        if (dir[axis] == 0.0) {
            if (origin[axis] < lo[axis] || origin[axis] > hi[axis]) {
                return std::nullopt;
            }
            continue;
        }
        double t0 = (lo[axis] - origin[axis]) / dir[axis];
        double t1 = (hi[axis] - origin[axis]) / dir[axis];
        if (t0 > t1) {
            std::swap(t0, t1);
        }
        tEnter = std::max(tEnter, t0);
        tExit = std::min(tExit, t1);
        if (tEnter > tExit) {
            return std::nullopt;
        }
    }
    return RayBoxHit{tEnter, tExit};
}

} // namespace katana::math
