#pragma once

#include <cmath>
#include <ostream>

#include "katana/math/numerics.hpp"

namespace katana::math {

// operator== is exact. Use nearlyEqual() for tolerance comparisons: fuzzy
// equality is not transitive and must never hide inside an operator.
struct Vec2 {
    double x = 0.0;
    double y = 0.0;

    constexpr Vec2() = default;
    constexpr Vec2(double xIn, double yIn) : x(xIn), y(yIn) {}

    [[nodiscard]] constexpr double lengthSquared() const { return x * x + y * y; }
    [[nodiscard]] double length() const { return std::hypot(x, y); }

    // Unit vector, or the zero vector when the length is zero.
    [[nodiscard]] Vec2 normalized() const
    {
        const double len = length();
        return len > 0.0 ? Vec2(x / len, y / len) : Vec2{};
    }

    [[nodiscard]] constexpr double dot(const Vec2& other) const { return x * other.x + y * other.y; }

    // z component of the 3D cross product; positive when `other` is counter-clockwise.
    [[nodiscard]] constexpr double cross(const Vec2& other) const
    {
        return x * other.y - y * other.x;
    }

    // Rotated +90 degrees (counter-clockwise).
    [[nodiscard]] constexpr Vec2 perpendicular() const { return Vec2(-y, x); }

    [[nodiscard]] Vec2 rotated(double radians) const
    {
        const double c = std::cos(radians);
        const double s = std::sin(radians);
        return Vec2(c * x - s * y, s * x + c * y);
    }

    // Direction angle in (-pi, pi], measured counter-clockwise from +x.
    [[nodiscard]] double angle() const { return std::atan2(y, x); }

    [[nodiscard]] double distanceTo(const Vec2& other) const
    {
        return std::hypot(x - other.x, y - other.y);
    }

    [[nodiscard]] bool isFinite() const { return std::isfinite(x) && std::isfinite(y); }

    constexpr Vec2& operator+=(const Vec2& other)
    {
        x += other.x;
        y += other.y;
        return *this;
    }
    constexpr Vec2& operator-=(const Vec2& other)
    {
        x -= other.x;
        y -= other.y;
        return *this;
    }
    constexpr Vec2& operator*=(double scalar)
    {
        x *= scalar;
        y *= scalar;
        return *this;
    }
    constexpr Vec2& operator/=(double scalar)
    {
        x /= scalar;
        y /= scalar;
        return *this;
    }

    friend constexpr bool operator==(const Vec2&, const Vec2&) = default;
};

[[nodiscard]] constexpr Vec2 operator+(Vec2 lhs, const Vec2& rhs) { return lhs += rhs; }
[[nodiscard]] constexpr Vec2 operator-(Vec2 lhs, const Vec2& rhs) { return lhs -= rhs; }
[[nodiscard]] constexpr Vec2 operator*(Vec2 lhs, double scalar) { return lhs *= scalar; }
[[nodiscard]] constexpr Vec2 operator*(double scalar, Vec2 rhs) { return rhs *= scalar; }
[[nodiscard]] constexpr Vec2 operator/(Vec2 lhs, double scalar) { return lhs /= scalar; }
[[nodiscard]] constexpr Vec2 operator-(const Vec2& value) { return Vec2(-value.x, -value.y); }

[[nodiscard]] constexpr double dot(const Vec2& a, const Vec2& b) { return a.dot(b); }
[[nodiscard]] constexpr double cross(const Vec2& a, const Vec2& b) { return a.cross(b); }
[[nodiscard]] inline double length(const Vec2& v) { return v.length(); }
[[nodiscard]] inline Vec2 normalize(const Vec2& v) { return v.normalized(); }
[[nodiscard]] inline double distance(const Vec2& a, const Vec2& b) { return a.distanceTo(b); }

[[nodiscard]] constexpr Vec2 lerp(const Vec2& a, const Vec2& b, double t)
{
    return a + (b - a) * t;
}

// Projection of `v` onto `onto`; zero when `onto` has zero length.
[[nodiscard]] constexpr Vec2 project(const Vec2& v, const Vec2& onto)
{
    const double denom = onto.lengthSquared();
    return denom > 0.0 ? onto * (v.dot(onto) / denom) : Vec2{};
}

// Reflection of `v` about the line whose unit normal is `unitNormal`.
[[nodiscard]] constexpr Vec2 reflect(const Vec2& v, const Vec2& unitNormal)
{
    return v - unitNormal * (2.0 * v.dot(unitNormal));
}

[[nodiscard]] inline bool nearlyEqual(const Vec2& a, const Vec2& b,
                                      double epsilon = tolerance::kRelative)
{
    return nearlyEqual(a.x, b.x, epsilon) && nearlyEqual(a.y, b.y, epsilon);
}

inline std::ostream& operator<<(std::ostream& os, const Vec2& value)
{
    return os << "Vec2(" << value.x << ", " << value.y << ")";
}

} // namespace katana::math
