#pragma once

#include <array>
#include <cmath>
#include <optional>
#include <ostream>

#include "katana/math/numerics.hpp"
#include "katana/math/vec2.hpp"
#include "katana/math/vec3.hpp"

namespace katana::math {

// Row-major 3x3 matrix acting on column vectors: v' = M * v.
// Also used as a 2D homogeneous (affine) transform; see transformPoint().
struct Mat3 {
    std::array<double, 9> data{1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0};

    constexpr Mat3() = default;
    constexpr Mat3(double m00, double m01, double m02, double m10, double m11, double m12,
                   double m20, double m21, double m22)
        : data{m00, m01, m02, m10, m11, m12, m20, m21, m22}
    {
    }

    // ---- 2D affine factories -------------------------------------------------
    [[nodiscard]] static constexpr Mat3 translation(const Vec2& t)
    {
        return Mat3(1.0, 0.0, t.x, 0.0, 1.0, t.y, 0.0, 0.0, 1.0);
    }

    [[nodiscard]] static Mat3 rotation(double radians)
    {
        const double c = std::cos(radians);
        const double s = std::sin(radians);
        return Mat3(c, -s, 0.0, s, c, 0.0, 0.0, 0.0, 1.0);
    }

    [[nodiscard]] static constexpr Mat3 scaling(double sx, double sy)
    {
        return Mat3(sx, 0.0, 0.0, 0.0, sy, 0.0, 0.0, 0.0, 1.0);
    }

    [[nodiscard]] static Mat3 rotationAbout(const Vec2& center, double radians);
    [[nodiscard]] static constexpr Mat3 scalingAbout(const Vec2& center, double sx, double sy);

    // Mirror about the line through `pointOnLine` with direction `direction`.
    // Returns identity when `direction` has zero length.
    [[nodiscard]] static Mat3 reflection(const Vec2& pointOnLine, const Vec2& direction);

    [[nodiscard]] constexpr double& operator()(std::size_t row, std::size_t col)
    {
        return data[row * 3 + col];
    }
    [[nodiscard]] constexpr const double& operator()(std::size_t row, std::size_t col) const
    {
        return data[row * 3 + col];
    }

    [[nodiscard]] constexpr Vec3 multiply(const Vec3& v) const
    {
        return Vec3(data[0] * v.x + data[1] * v.y + data[2] * v.z,
                    data[3] * v.x + data[4] * v.y + data[5] * v.z,
                    data[6] * v.x + data[7] * v.y + data[8] * v.z);
    }

    [[nodiscard]] constexpr Mat3 transposed() const
    {
        return Mat3(data[0], data[3], data[6], data[1], data[4], data[7], data[2], data[5],
                    data[8]);
    }

    [[nodiscard]] constexpr double determinant() const
    {
        return data[0] * (data[4] * data[8] - data[5] * data[7]) -
               data[1] * (data[3] * data[8] - data[5] * data[6]) +
               data[2] * (data[3] * data[7] - data[4] * data[6]);
    }

    // nullopt when singular: |det| <= kAbsolute * product of row lengths (Hadamard bound).
    [[nodiscard]] std::optional<Mat3> inverse() const
    {
        const double det = determinant();
        const double scale = std::hypot(data[0], data[1], data[2]) *
                             std::hypot(data[3], data[4], data[5]) *
                             std::hypot(data[6], data[7], data[8]);
        if (!(std::abs(det) > tolerance::kAbsolute * scale)) {
            return std::nullopt;
        }
        const double inv = 1.0 / det;
        return Mat3((data[4] * data[8] - data[5] * data[7]) * inv,
                    (data[2] * data[7] - data[1] * data[8]) * inv,
                    (data[1] * data[5] - data[2] * data[4]) * inv,
                    (data[5] * data[6] - data[3] * data[8]) * inv,
                    (data[0] * data[8] - data[2] * data[6]) * inv,
                    (data[2] * data[3] - data[0] * data[5]) * inv,
                    (data[3] * data[7] - data[4] * data[6]) * inv,
                    (data[1] * data[6] - data[0] * data[7]) * inv,
                    (data[0] * data[4] - data[1] * data[3]) * inv);
    }

    friend constexpr bool operator==(const Mat3&, const Mat3&) = default;
};

[[nodiscard]] constexpr Vec3 operator*(const Mat3& m, const Vec3& v) { return m.multiply(v); }

[[nodiscard]] constexpr Mat3 operator*(const Mat3& a, const Mat3& b)
{
    Mat3 result;
    for (std::size_t row = 0; row < 3; ++row) {
        for (std::size_t col = 0; col < 3; ++col) {
            double sum = 0.0;
            for (std::size_t k = 0; k < 3; ++k) {
                sum += a(row, k) * b(k, col);
            }
            result(row, col) = sum;
        }
    }
    return result;
}

// Applies a 2D affine transform to a position (w = 1).
[[nodiscard]] constexpr Vec2 transformPoint(const Mat3& m, const Vec2& p)
{
    return Vec2(m.data[0] * p.x + m.data[1] * p.y + m.data[2],
                m.data[3] * p.x + m.data[4] * p.y + m.data[5]);
}

// Applies the linear part of a 2D affine transform to a direction (w = 0).
[[nodiscard]] constexpr Vec2 transformVector(const Mat3& m, const Vec2& v)
{
    return Vec2(m.data[0] * v.x + m.data[1] * v.y, m.data[3] * v.x + m.data[4] * v.y);
}

inline Mat3 Mat3::rotationAbout(const Vec2& center, double radians)
{
    return translation(center) * rotation(radians) * translation(-center);
}

constexpr Mat3 Mat3::scalingAbout(const Vec2& center, double sx, double sy)
{
    return translation(center) * scaling(sx, sy) * translation(-center);
}

inline Mat3 Mat3::reflection(const Vec2& pointOnLine, const Vec2& direction)
{
    const Vec2 d = direction.normalized();
    if (d == Vec2{}) {
        return Mat3{};
    }
    // Householder-style reflection about the direction d: R = 2*d*d^T - I.
    const Mat3 linear(2.0 * d.x * d.x - 1.0, 2.0 * d.x * d.y, 0.0, 2.0 * d.x * d.y,
                      2.0 * d.y * d.y - 1.0, 0.0, 0.0, 0.0, 1.0);
    return translation(pointOnLine) * linear * translation(-pointOnLine);
}

[[nodiscard]] inline bool nearlyEqual(const Mat3& a, const Mat3& b,
                                      double epsilon = tolerance::kRelative)
{
    for (std::size_t i = 0; i < a.data.size(); ++i) {
        if (!nearlyEqual(a.data[i], b.data[i], epsilon)) {
            return false;
        }
    }
    return true;
}

inline std::ostream& operator<<(std::ostream& os, const Mat3& m)
{
    os << "Mat3([";
    for (std::size_t row = 0; row < 3; ++row) {
        os << (row == 0 ? "[" : ", [") << m(row, 0) << ", " << m(row, 1) << ", " << m(row, 2)
           << "]";
    }
    return os << "])";
}

} // namespace katana::math
