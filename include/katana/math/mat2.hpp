#pragma once

#include <array>
#include <cmath>
#include <optional>
#include <ostream>

#include "katana/math/numerics.hpp"
#include "katana/math/vec2.hpp"

namespace katana::math {

// Row-major 2x2 matrix acting on column vectors: v' = M * v.
struct Mat2 {
    std::array<double, 4> data{1.0, 0.0, 0.0, 1.0};

    constexpr Mat2() = default;
    constexpr Mat2(double m00, double m01, double m10, double m11) : data{m00, m01, m10, m11} {}

    [[nodiscard]] static Mat2 rotation(double radians)
    {
        const double c = std::cos(radians);
        const double s = std::sin(radians);
        return Mat2(c, -s, s, c);
    }

    [[nodiscard]] static constexpr Mat2 scaling(double sx, double sy)
    {
        return Mat2(sx, 0.0, 0.0, sy);
    }

    [[nodiscard]] constexpr double& operator()(std::size_t row, std::size_t col)
    {
        return data[row * 2 + col];
    }
    [[nodiscard]] constexpr const double& operator()(std::size_t row, std::size_t col) const
    {
        return data[row * 2 + col];
    }

    [[nodiscard]] constexpr Vec2 multiply(const Vec2& v) const
    {
        return Vec2(data[0] * v.x + data[1] * v.y, data[2] * v.x + data[3] * v.y);
    }

    [[nodiscard]] constexpr Mat2 transposed() const
    {
        return Mat2(data[0], data[2], data[1], data[3]);
    }

    [[nodiscard]] constexpr double determinant() const
    {
        return data[0] * data[3] - data[1] * data[2];
    }

    // nullopt when the rows are parallel to within tolerance::kAbsolute
    // (|det| relative to the product of the row lengths, so scale invariant).
    [[nodiscard]] std::optional<Mat2> inverse() const
    {
        const double det = determinant();
        const double scale = std::hypot(data[0], data[1]) * std::hypot(data[2], data[3]);
        if (!(std::abs(det) > tolerance::kAbsolute * scale)) {
            return std::nullopt;
        }
        const double inv = 1.0 / det;
        return Mat2(data[3] * inv, -data[1] * inv, -data[2] * inv, data[0] * inv);
    }

    friend constexpr bool operator==(const Mat2&, const Mat2&) = default;
};

[[nodiscard]] constexpr Vec2 operator*(const Mat2& m, const Vec2& v) { return m.multiply(v); }

[[nodiscard]] constexpr Mat2 operator*(const Mat2& a, const Mat2& b)
{
    return Mat2(a.data[0] * b.data[0] + a.data[1] * b.data[2],
                a.data[0] * b.data[1] + a.data[1] * b.data[3],
                a.data[2] * b.data[0] + a.data[3] * b.data[2],
                a.data[2] * b.data[1] + a.data[3] * b.data[3]);
}

[[nodiscard]] inline bool nearlyEqual(const Mat2& a, const Mat2& b,
                                      double epsilon = tolerance::kRelative)
{
    for (std::size_t i = 0; i < a.data.size(); ++i) {
        if (!nearlyEqual(a.data[i], b.data[i], epsilon)) {
            return false;
        }
    }
    return true;
}

inline std::ostream& operator<<(std::ostream& os, const Mat2& m)
{
    return os << "Mat2([[" << m.data[0] << ", " << m.data[1] << "], [" << m.data[2] << ", "
              << m.data[3] << "]])";
}

} // namespace katana::math
