#pragma once

#include <cmath>
#include <ostream>

#include "katana/math/numerics.hpp"

namespace katana::math {

struct Vec3 {
    double x = 0.0;
    double y = 0.0;
    double z = 0.0;

    constexpr Vec3() = default;
    constexpr Vec3(double xIn, double yIn, double zIn) : x(xIn), y(yIn), z(zIn) {}

    [[nodiscard]] constexpr double lengthSquared() const { return x * x + y * y + z * z; }
    [[nodiscard]] double length() const { return std::hypot(x, y, z); }

    // Unit vector, or the zero vector when the length is zero.
    [[nodiscard]] Vec3 normalized() const
    {
        const double len = length();
        return len > 0.0 ? Vec3(x / len, y / len, z / len) : Vec3{};
    }

    [[nodiscard]] constexpr double dot(const Vec3& other) const
    {
        return x * other.x + y * other.y + z * other.z;
    }

    [[nodiscard]] constexpr Vec3 cross(const Vec3& other) const
    {
        return Vec3(y * other.z - z * other.y, z * other.x - x * other.z,
                    x * other.y - y * other.x);
    }

    [[nodiscard]] double distanceTo(const Vec3& other) const
    {
        return std::hypot(x - other.x, y - other.y, z - other.z);
    }

    [[nodiscard]] bool isFinite() const
    {
        return std::isfinite(x) && std::isfinite(y) && std::isfinite(z);
    }

    constexpr Vec3& operator+=(const Vec3& other)
    {
        x += other.x;
        y += other.y;
        z += other.z;
        return *this;
    }
    constexpr Vec3& operator-=(const Vec3& other)
    {
        x -= other.x;
        y -= other.y;
        z -= other.z;
        return *this;
    }
    constexpr Vec3& operator*=(double scalar)
    {
        x *= scalar;
        y *= scalar;
        z *= scalar;
        return *this;
    }
    constexpr Vec3& operator/=(double scalar)
    {
        x /= scalar;
        y /= scalar;
        z /= scalar;
        return *this;
    }

    friend constexpr bool operator==(const Vec3&, const Vec3&) = default;
};

[[nodiscard]] constexpr Vec3 operator+(Vec3 lhs, const Vec3& rhs) { return lhs += rhs; }
[[nodiscard]] constexpr Vec3 operator-(Vec3 lhs, const Vec3& rhs) { return lhs -= rhs; }
[[nodiscard]] constexpr Vec3 operator*(Vec3 lhs, double scalar) { return lhs *= scalar; }
[[nodiscard]] constexpr Vec3 operator*(double scalar, Vec3 rhs) { return rhs *= scalar; }
[[nodiscard]] constexpr Vec3 operator/(Vec3 lhs, double scalar) { return lhs /= scalar; }
[[nodiscard]] constexpr Vec3 operator-(const Vec3& v) { return Vec3(-v.x, -v.y, -v.z); }

[[nodiscard]] constexpr double dot(const Vec3& a, const Vec3& b) { return a.dot(b); }
[[nodiscard]] constexpr Vec3 cross(const Vec3& a, const Vec3& b) { return a.cross(b); }
[[nodiscard]] inline double length(const Vec3& v) { return v.length(); }
[[nodiscard]] inline Vec3 normalize(const Vec3& v) { return v.normalized(); }
[[nodiscard]] inline double distance(const Vec3& a, const Vec3& b) { return a.distanceTo(b); }

[[nodiscard]] constexpr Vec3 lerp(const Vec3& a, const Vec3& b, double t)
{
    return a + (b - a) * t;
}

// Projection of `v` onto `onto`; zero when `onto` has zero length.
[[nodiscard]] constexpr Vec3 project(const Vec3& v, const Vec3& onto)
{
    const double denom = onto.lengthSquared();
    return denom > 0.0 ? onto * (v.dot(onto) / denom) : Vec3{};
}

// Reflection of `v` about the plane whose unit normal is `unitNormal`.
[[nodiscard]] constexpr Vec3 reflect(const Vec3& v, const Vec3& unitNormal)
{
    return v - unitNormal * (2.0 * v.dot(unitNormal));
}

[[nodiscard]] inline bool nearlyEqual(const Vec3& a, const Vec3& b,
                                      double epsilon = tolerance::kRelative)
{
    return nearlyEqual(a.x, b.x, epsilon) && nearlyEqual(a.y, b.y, epsilon) &&
           nearlyEqual(a.z, b.z, epsilon);
}

inline std::ostream& operator<<(std::ostream& os, const Vec3& value)
{
    return os << "Vec3(" << value.x << ", " << value.y << ", " << value.z << ")";
}

} // namespace katana::math
