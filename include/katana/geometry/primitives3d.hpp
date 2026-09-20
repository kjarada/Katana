#pragma once

// 3D primitives needed by terrain and picking (PLAN.MD Phase 03).
// Plane, Ray and AABB live in katana/math/primitives.hpp.

#include <algorithm>
#include <cmath>
#include <optional>

#include "katana/math/numerics.hpp"
#include "katana/math/primitives.hpp"
#include "katana/math/vec3.hpp"

namespace katana::geometry {

using Vec3 = katana::math::Vec3;
using Point3 = katana::math::Vec3;

struct Segment3 {
    Point3 start;
    Point3 end;

    [[nodiscard]] double length() const { return start.distanceTo(end); }
    [[nodiscard]] Point3 pointAt(double t) const { return start + (end - start) * t; }

    // Parameter in [0, 1] of the point of the segment closest to `p`.
    [[nodiscard]] double parameterOf(const Point3& p) const
    {
        const Vec3 d = end - start;
        const double lengthSquared = d.lengthSquared();
        if (!(lengthSquared > 0.0)) {
            return 0.0;
        }
        return std::clamp((p - start).dot(d) / lengthSquared, 0.0, 1.0);
    }
    [[nodiscard]] Point3 closestPoint(const Point3& p) const { return pointAt(parameterOf(p)); }
    [[nodiscard]] double distanceTo(const Point3& p) const
    {
        return closestPoint(p).distanceTo(p);
    }
};

struct Triangle3 {
    Point3 a;
    Point3 b;
    Point3 c;

    // Not normalised; length is twice the area. Right-hand rule over a, b, c.
    [[nodiscard]] Vec3 scaledNormal() const { return (b - a).cross(c - a); }
    [[nodiscard]] double area() const { return 0.5 * scaledNormal().length(); }
    [[nodiscard]] Point3 centroid() const { return a + ((b - a) + (c - a)) / 3.0; }

    // nullopt when the vertices are collinear.
    [[nodiscard]] std::optional<katana::math::Plane> plane() const
    {
        return katana::math::Plane::fromPoints(a, b, c);
    }

    [[nodiscard]] katana::math::AABB boundingBox() const
    {
        katana::math::AABB box;
        box.expand(a);
        box.expand(b);
        box.expand(c);
        return box;
    }
};

// Moller-Trumbore. Returns the ray parameter t >= 0 of the hit (both faces are
// hit). nullopt when the ray misses, is parallel to the triangle, or the
// triangle is degenerate.
[[nodiscard]] inline std::optional<double> intersect(const katana::math::Ray& ray,
                                                     const Triangle3& triangle)
{
    const Vec3 edge1 = triangle.b - triangle.a;
    const Vec3 edge2 = triangle.c - triangle.a;
    const Vec3 p = ray.direction.cross(edge2);
    const double determinant = edge1.dot(p);
    // determinant = |d| * |e1 x e2| * cos(angle to the normal)
    const double scale = ray.direction.length() * edge1.length() * edge2.length();
    if (!(std::abs(determinant) > katana::math::tolerance::kAngular * scale)) {
        return std::nullopt;
    }
    const double inverse = 1.0 / determinant;
    const Vec3 toOrigin = ray.origin - triangle.a;
    const double u = toOrigin.dot(p) * inverse;
    if (u < 0.0 || u > 1.0) {
        return std::nullopt;
    }
    const Vec3 q = toOrigin.cross(edge1);
    const double v = ray.direction.dot(q) * inverse;
    if (v < 0.0 || u + v > 1.0) {
        return std::nullopt;
    }
    const double t = edge2.dot(q) * inverse;
    if (t < 0.0) {
        return std::nullopt;
    }
    return t;
}

} // namespace katana::geometry
