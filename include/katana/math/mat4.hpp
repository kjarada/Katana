#pragma once

#include <array>
#include <cmath>
#include <optional>
#include <ostream>

#include "katana/math/numerics.hpp"
#include "katana/math/vec3.hpp"
#include "katana/math/vec4.hpp"

namespace katana::math {

// Row-major 4x4 matrix acting on column vectors: v' = M * v.
struct Mat4 {
    std::array<double, 16> data{1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0,
                                0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0};

    constexpr Mat4() = default;
    constexpr Mat4(double m00, double m01, double m02, double m03, double m10, double m11,
                   double m12, double m13, double m20, double m21, double m22, double m23,
                   double m30, double m31, double m32, double m33)
        : data{m00, m01, m02, m03, m10, m11, m12, m13, m20, m21, m22, m23, m30, m31, m32, m33}
    {
    }

    [[nodiscard]] static constexpr Mat4 translation(const Vec3& t)
    {
        return Mat4(1.0, 0.0, 0.0, t.x, 0.0, 1.0, 0.0, t.y, 0.0, 0.0, 1.0, t.z, 0.0, 0.0, 0.0,
                    1.0);
    }

    [[nodiscard]] static constexpr Mat4 scaling(const Vec3& s)
    {
        return Mat4(s.x, 0.0, 0.0, 0.0, 0.0, s.y, 0.0, 0.0, 0.0, 0.0, s.z, 0.0, 0.0, 0.0, 0.0,
                    1.0);
    }

    // Rotation by `radians` about `axis` (Rodrigues). Identity for a zero axis.
    [[nodiscard]] static Mat4 rotation(const Vec3& axis, double radians)
    {
        const Vec3 u = axis.normalized();
        if (u == Vec3{}) {
            return Mat4{};
        }
        const double c = std::cos(radians);
        const double s = std::sin(radians);
        const double t = 1.0 - c;
        return Mat4(t * u.x * u.x + c, t * u.x * u.y - s * u.z, t * u.x * u.z + s * u.y, 0.0,
                    t * u.x * u.y + s * u.z, t * u.y * u.y + c, t * u.y * u.z - s * u.x, 0.0,
                    t * u.x * u.z - s * u.y, t * u.y * u.z + s * u.x, t * u.z * u.z + c, 0.0, 0.0,
                    0.0, 0.0, 1.0);
    }

    [[nodiscard]] constexpr double& operator()(std::size_t row, std::size_t col)
    {
        return data[row * 4 + col];
    }
    [[nodiscard]] constexpr const double& operator()(std::size_t row, std::size_t col) const
    {
        return data[row * 4 + col];
    }

    [[nodiscard]] constexpr Vec4 multiply(const Vec4& v) const
    {
        return Vec4(data[0] * v.x + data[1] * v.y + data[2] * v.z + data[3] * v.w,
                    data[4] * v.x + data[5] * v.y + data[6] * v.z + data[7] * v.w,
                    data[8] * v.x + data[9] * v.y + data[10] * v.z + data[11] * v.w,
                    data[12] * v.x + data[13] * v.y + data[14] * v.z + data[15] * v.w);
    }

    [[nodiscard]] constexpr Mat4 transposed() const
    {
        Mat4 result;
        for (std::size_t row = 0; row < 4; ++row) {
            for (std::size_t col = 0; col < 4; ++col) {
                result(row, col) = (*this)(col, row);
            }
        }
        return result;
    }

    [[nodiscard]] constexpr double determinant() const
    {
        const SubDeterminants d(*this);
        return d.det();
    }

    // Laplace expansion by complementary 2x2 minors. nullopt when singular:
    // |det| <= kAbsolute * product of row lengths (Hadamard bound).
    [[nodiscard]] std::optional<Mat4> inverse() const
    {
        const SubDeterminants d(*this);
        const double det = d.det();
        double scale = 1.0;
        for (std::size_t row = 0; row < 4; ++row) {
            scale *= std::sqrt((*this)(row, 0) * (*this)(row, 0) + (*this)(row, 1) * (*this)(row, 1) +
                               (*this)(row, 2) * (*this)(row, 2) + (*this)(row, 3) * (*this)(row, 3));
        }
        if (!(std::abs(det) > tolerance::kAbsolute * scale)) {
            return std::nullopt;
        }

        const auto& a = data;
        const double inv = 1.0 / det;
        return Mat4((a[5] * d.c5 - a[6] * d.c4 + a[7] * d.c3) * inv,
                    (-a[1] * d.c5 + a[2] * d.c4 - a[3] * d.c3) * inv,
                    (a[13] * d.s5 - a[14] * d.s4 + a[15] * d.s3) * inv,
                    (-a[9] * d.s5 + a[10] * d.s4 - a[11] * d.s3) * inv,

                    (-a[4] * d.c5 + a[6] * d.c2 - a[7] * d.c1) * inv,
                    (a[0] * d.c5 - a[2] * d.c2 + a[3] * d.c1) * inv,
                    (-a[12] * d.s5 + a[14] * d.s2 - a[15] * d.s1) * inv,
                    (a[8] * d.s5 - a[10] * d.s2 + a[11] * d.s1) * inv,

                    (a[4] * d.c4 - a[5] * d.c2 + a[7] * d.c0) * inv,
                    (-a[0] * d.c4 + a[1] * d.c2 - a[3] * d.c0) * inv,
                    (a[12] * d.s4 - a[13] * d.s2 + a[15] * d.s0) * inv,
                    (-a[8] * d.s4 + a[9] * d.s2 - a[11] * d.s0) * inv,

                    (-a[4] * d.c3 + a[5] * d.c1 - a[6] * d.c0) * inv,
                    (a[0] * d.c3 - a[1] * d.c1 + a[2] * d.c0) * inv,
                    (-a[12] * d.s3 + a[13] * d.s1 - a[14] * d.s0) * inv,
                    (a[8] * d.s3 - a[9] * d.s1 + a[10] * d.s0) * inv);
    }

    friend constexpr bool operator==(const Mat4&, const Mat4&) = default;

  private:
    // 2x2 minors of the top two rows (s*) and bottom two rows (c*).
    struct SubDeterminants {
        double s0, s1, s2, s3, s4, s5;
        double c0, c1, c2, c3, c4, c5;

        constexpr explicit SubDeterminants(const Mat4& m)
            : s0(m.data[0] * m.data[5] - m.data[4] * m.data[1]),
              s1(m.data[0] * m.data[6] - m.data[4] * m.data[2]),
              s2(m.data[0] * m.data[7] - m.data[4] * m.data[3]),
              s3(m.data[1] * m.data[6] - m.data[5] * m.data[2]),
              s4(m.data[1] * m.data[7] - m.data[5] * m.data[3]),
              s5(m.data[2] * m.data[7] - m.data[6] * m.data[3]),
              c0(m.data[8] * m.data[13] - m.data[12] * m.data[9]),
              c1(m.data[8] * m.data[14] - m.data[12] * m.data[10]),
              c2(m.data[8] * m.data[15] - m.data[12] * m.data[11]),
              c3(m.data[9] * m.data[14] - m.data[13] * m.data[10]),
              c4(m.data[9] * m.data[15] - m.data[13] * m.data[11]),
              c5(m.data[10] * m.data[15] - m.data[14] * m.data[11])
        {
        }

        [[nodiscard]] constexpr double det() const
        {
            return s0 * c5 - s1 * c4 + s2 * c3 + s3 * c2 - s4 * c1 + s5 * c0;
        }
    };
};

[[nodiscard]] constexpr Vec4 operator*(const Mat4& m, const Vec4& v) { return m.multiply(v); }

[[nodiscard]] constexpr Mat4 operator*(const Mat4& a, const Mat4& b)
{
    Mat4 result;
    for (std::size_t row = 0; row < 4; ++row) {
        for (std::size_t col = 0; col < 4; ++col) {
            double sum = 0.0;
            for (std::size_t k = 0; k < 4; ++k) {
                sum += a(row, k) * b(k, col);
            }
            result(row, col) = sum;
        }
    }
    return result;
}

// Applies an affine transform to a position (w = 1). No perspective divide.
[[nodiscard]] constexpr Vec3 transformPoint(const Mat4& m, const Vec3& p)
{
    return Vec3(m.data[0] * p.x + m.data[1] * p.y + m.data[2] * p.z + m.data[3],
                m.data[4] * p.x + m.data[5] * p.y + m.data[6] * p.z + m.data[7],
                m.data[8] * p.x + m.data[9] * p.y + m.data[10] * p.z + m.data[11]);
}

// Applies the linear part of an affine transform to a direction (w = 0).
[[nodiscard]] constexpr Vec3 transformVector(const Mat4& m, const Vec3& v)
{
    return Vec3(m.data[0] * v.x + m.data[1] * v.y + m.data[2] * v.z,
                m.data[4] * v.x + m.data[5] * v.y + m.data[6] * v.z,
                m.data[8] * v.x + m.data[9] * v.y + m.data[10] * v.z);
}

[[nodiscard]] inline bool nearlyEqual(const Mat4& a, const Mat4& b,
                                      double epsilon = tolerance::kRelative)
{
    for (std::size_t i = 0; i < a.data.size(); ++i) {
        if (!nearlyEqual(a.data[i], b.data[i], epsilon)) {
            return false;
        }
    }
    return true;
}

inline std::ostream& operator<<(std::ostream& os, const Mat4& m)
{
    os << "Mat4([";
    for (std::size_t row = 0; row < 4; ++row) {
        os << (row == 0 ? "[" : ", [") << m(row, 0) << ", " << m(row, 1) << ", " << m(row, 2)
           << ", " << m(row, 3) << "]";
    }
    return os << "])";
}

} // namespace katana::math
