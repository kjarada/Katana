#pragma once

#include <cmath>
#include <ostream>

#include "katana/math/mat3.hpp"
#include "katana/math/numerics.hpp"
#include "katana/math/vec3.hpp"

namespace katana::math {

// Rotation quaternion (x, y, z, w) with w the scalar part. Hamilton convention.
struct Quaternion {
    double x = 0.0;
    double y = 0.0;
    double z = 0.0;
    double w = 1.0;

    constexpr Quaternion() = default;
    constexpr Quaternion(double xIn, double yIn, double zIn, double wIn)
        : x(xIn), y(yIn), z(zIn), w(wIn)
    {
    }

    // Identity for a zero-length axis.
    [[nodiscard]] static Quaternion fromAxisAngle(const Vec3& axis, double radians)
    {
        const Vec3 u = axis.normalized();
        if (u == Vec3{}) {
            return Quaternion{};
        }
        const double half = 0.5 * radians;
        const double s = std::sin(half);
        return Quaternion(u.x * s, u.y * s, u.z * s, std::cos(half));
    }

    [[nodiscard]] constexpr double lengthSquared() const { return x * x + y * y + z * z + w * w; }
    [[nodiscard]] double length() const { return std::sqrt(lengthSquared()); }

    // Unit quaternion, or identity when the length is zero.
    [[nodiscard]] Quaternion normalized() const
    {
        const double len = length();
        return len > 0.0 ? Quaternion(x / len, y / len, z / len, w / len) : Quaternion{};
    }

    [[nodiscard]] constexpr Quaternion conjugate() const { return Quaternion(-x, -y, -z, w); }

    // Identity when the length is zero.
    [[nodiscard]] constexpr Quaternion inverse() const
    {
        const double lenSq = lengthSquared();
        if (!(lenSq > 0.0)) {
            return Quaternion{};
        }
        const double inv = 1.0 / lenSq;
        return Quaternion(-x * inv, -y * inv, -z * inv, w * inv);
    }

    [[nodiscard]] constexpr double dot(const Quaternion& o) const
    {
        return x * o.x + y * o.y + z * o.z + w * o.w;
    }

    // Rotates `v`. The quaternion must be unit length.
    [[nodiscard]] constexpr Vec3 rotate(const Vec3& v) const
    {
        // v' = v + 2w(q x v) + 2 q x (q x v), with q the vector part.
        const Vec3 q(x, y, z);
        const Vec3 t = q.cross(v) * 2.0;
        return v + t * w + q.cross(t);
    }

    // Rotation matrix of a unit quaternion.
    [[nodiscard]] constexpr Mat3 toMatrix() const
    {
        const double xx = x * x, yy = y * y, zz = z * z;
        const double xy = x * y, xz = x * z, yz = y * z;
        const double wx = w * x, wy = w * y, wz = w * z;
        return Mat3(1.0 - 2.0 * (yy + zz), 2.0 * (xy - wz), 2.0 * (xz + wy), 2.0 * (xy + wz),
                    1.0 - 2.0 * (xx + zz), 2.0 * (yz - wx), 2.0 * (xz - wy), 2.0 * (yz + wx),
                    1.0 - 2.0 * (xx + yy));
    }

    friend constexpr bool operator==(const Quaternion&, const Quaternion&) = default;
};

// Composition: (a * b) applies b first, then a.
[[nodiscard]] constexpr Quaternion operator*(const Quaternion& a, const Quaternion& b)
{
    return Quaternion(a.w * b.x + a.x * b.w + a.y * b.z - a.z * b.y,
                      a.w * b.y - a.x * b.z + a.y * b.w + a.z * b.x,
                      a.w * b.z + a.x * b.y - a.y * b.x + a.z * b.w,
                      a.w * b.w - a.x * b.x - a.y * b.y - a.z * b.z);
}

// Spherical interpolation along the shortest arc between unit quaternions.
[[nodiscard]] inline Quaternion slerp(const Quaternion& a, Quaternion b, double t)
{
    double cosTheta = a.dot(b);
    if (cosTheta < 0.0) { // q and -q are the same rotation; take the short way round
        b = Quaternion(-b.x, -b.y, -b.z, -b.w);
        cosTheta = -cosTheta;
    }
    // Nearly parallel: sin(theta) underflows, fall back to normalised lerp.
    if (cosTheta > 1.0 - tolerance::kAbsolute) {
        return Quaternion(lerp(a.x, b.x, t), lerp(a.y, b.y, t), lerp(a.z, b.z, t),
                          lerp(a.w, b.w, t))
            .normalized();
    }
    const double theta = std::acos(cosTheta);
    const double sinTheta = std::sin(theta);
    const double wa = std::sin((1.0 - t) * theta) / sinTheta;
    const double wb = std::sin(t * theta) / sinTheta;
    return Quaternion(wa * a.x + wb * b.x, wa * a.y + wb * b.y, wa * a.z + wb * b.z,
                      wa * a.w + wb * b.w);
}

[[nodiscard]] inline bool nearlyEqual(const Quaternion& a, const Quaternion& b,
                                      double epsilon = tolerance::kRelative)
{
    return nearlyEqual(a.x, b.x, epsilon) && nearlyEqual(a.y, b.y, epsilon) &&
           nearlyEqual(a.z, b.z, epsilon) && nearlyEqual(a.w, b.w, epsilon);
}

inline std::ostream& operator<<(std::ostream& os, const Quaternion& q)
{
    return os << "Quaternion(" << q.x << ", " << q.y << ", " << q.z << ", " << q.w << ")";
}

} // namespace katana::math
