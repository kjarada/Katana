#pragma once

#include <cmath>
#include <ostream>

#include "katana/math/numerics.hpp"
#include "katana/math/vec3.hpp"

namespace katana::math {

struct Vec4 {
    double x = 0.0;
    double y = 0.0;
    double z = 0.0;
    double w = 0.0;

    constexpr Vec4() = default;
    constexpr Vec4(double xIn, double yIn, double zIn, double wIn) : x(xIn), y(yIn), z(zIn), w(wIn)
    {
    }
    constexpr Vec4(const Vec3& xyz, double wIn) : x(xyz.x), y(xyz.y), z(xyz.z), w(wIn) {}

    [[nodiscard]] constexpr Vec3 xyz() const { return Vec3(x, y, z); }

    [[nodiscard]] constexpr double lengthSquared() const { return x * x + y * y + z * z + w * w; }
    [[nodiscard]] double length() const { return std::sqrt(lengthSquared()); }

    // Unit vector, or the zero vector when the length is zero.
    [[nodiscard]] Vec4 normalized() const
    {
        const double len = length();
        return len > 0.0 ? Vec4(x / len, y / len, z / len, w / len) : Vec4{};
    }

    [[nodiscard]] constexpr double dot(const Vec4& other) const
    {
        return x * other.x + y * other.y + z * other.z + w * other.w;
    }

    [[nodiscard]] double distanceTo(const Vec4& other) const
    {
        return Vec4(x - other.x, y - other.y, z - other.z, w - other.w).length();
    }

    constexpr Vec4& operator+=(const Vec4& other)
    {
        x += other.x;
        y += other.y;
        z += other.z;
        w += other.w;
        return *this;
    }
    constexpr Vec4& operator-=(const Vec4& other)
    {
        x -= other.x;
        y -= other.y;
        z -= other.z;
        w -= other.w;
        return *this;
    }
    constexpr Vec4& operator*=(double scalar)
    {
        x *= scalar;
        y *= scalar;
        z *= scalar;
        w *= scalar;
        return *this;
    }
    constexpr Vec4& operator/=(double scalar)
    {
        x /= scalar;
        y /= scalar;
        z /= scalar;
        w /= scalar;
        return *this;
    }

    friend constexpr bool operator==(const Vec4&, const Vec4&) = default;
};

[[nodiscard]] constexpr Vec4 operator+(Vec4 lhs, const Vec4& rhs) { return lhs += rhs; }
[[nodiscard]] constexpr Vec4 operator-(Vec4 lhs, const Vec4& rhs) { return lhs -= rhs; }
[[nodiscard]] constexpr Vec4 operator*(Vec4 lhs, double scalar) { return lhs *= scalar; }
[[nodiscard]] constexpr Vec4 operator*(double scalar, Vec4 rhs) { return rhs *= scalar; }
[[nodiscard]] constexpr Vec4 operator/(Vec4 lhs, double scalar) { return lhs /= scalar; }
[[nodiscard]] constexpr Vec4 operator-(const Vec4& v) { return Vec4(-v.x, -v.y, -v.z, -v.w); }

[[nodiscard]] constexpr double dot(const Vec4& a, const Vec4& b) { return a.dot(b); }
[[nodiscard]] inline double length(const Vec4& v) { return v.length(); }
[[nodiscard]] inline Vec4 normalize(const Vec4& v) { return v.normalized(); }
[[nodiscard]] inline double distance(const Vec4& a, const Vec4& b) { return a.distanceTo(b); }

[[nodiscard]] constexpr Vec4 lerp(const Vec4& a, const Vec4& b, double t)
{
    return a + (b - a) * t;
}

[[nodiscard]] inline bool nearlyEqual(const Vec4& a, const Vec4& b,
                                      double epsilon = tolerance::kRelative)
{
    return nearlyEqual(a.x, b.x, epsilon) && nearlyEqual(a.y, b.y, epsilon) &&
           nearlyEqual(a.z, b.z, epsilon) && nearlyEqual(a.w, b.w, epsilon);
}

inline std::ostream& operator<<(std::ostream& os, const Vec4& value)
{
    return os << "Vec4(" << value.x << ", " << value.y << ", " << value.z << ", " << value.w
              << ")";
}

} // namespace katana::math
