#pragma once

#include <optional>

#include "katana/math/mat4.hpp"
#include "katana/math/quaternion.hpp"
#include "katana/math/vec3.hpp"

namespace katana::math {

// Translation * Rotation * Scale, applied to a point as T(R(S(p))).
// `rotation` must be a unit quaternion.
struct Transform {
    Vec3 translation{0.0, 0.0, 0.0};
    Quaternion rotation{0.0, 0.0, 0.0, 1.0};
    Vec3 scale{1.0, 1.0, 1.0};

    [[nodiscard]] constexpr Mat4 matrix() const
    {
        const Mat3 r = rotation.toMatrix();
        return Mat4(r(0, 0) * scale.x, r(0, 1) * scale.y, r(0, 2) * scale.z, translation.x,
                    r(1, 0) * scale.x, r(1, 1) * scale.y, r(1, 2) * scale.z, translation.y,
                    r(2, 0) * scale.x, r(2, 1) * scale.y, r(2, 2) * scale.z, translation.z, 0.0,
                    0.0, 0.0, 1.0);
    }

    [[nodiscard]] constexpr Vec3 transformPoint(const Vec3& p) const
    {
        return rotation.rotate(Vec3(p.x * scale.x, p.y * scale.y, p.z * scale.z)) + translation;
    }

    [[nodiscard]] constexpr Vec3 transformVector(const Vec3& v) const
    {
        return rotation.rotate(Vec3(v.x * scale.x, v.y * scale.y, v.z * scale.z));
    }

    // Inverse mapping of transformPoint(). nullopt when any scale factor is zero.
    // Computed analytically (S^-1 R^-1 T^-1) rather than through a matrix inverse.
    [[nodiscard]] std::optional<Vec3> inverseTransformPoint(const Vec3& p) const
    {
        if (scale.x == 0.0 || scale.y == 0.0 || scale.z == 0.0) {
            return std::nullopt;
        }
        const Vec3 unrotated = rotation.conjugate().rotate(p - translation);
        return Vec3(unrotated.x / scale.x, unrotated.y / scale.y, unrotated.z / scale.z);
    }

    friend constexpr bool operator==(const Transform&, const Transform&) = default;
};

} // namespace katana::math
