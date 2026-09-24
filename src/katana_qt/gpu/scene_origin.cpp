#include "scene_origin.hpp"

#include <cmath>

namespace katana::qt::gpu {

using katana::render::Camera;
using katana::render::Projection;

Vec3 chooseSceneOrigin(const katana::math::AABB& bounds)
{
    if (bounds.empty() || !bounds.min.isFinite() || !bounds.max.isFinite()) {
        return Vec3(0.0, 0.0, 0.0);
    }
    return bounds.center();
}

Mat4 relativeViewMatrix(const Camera& camera, const Vec3& origin)
{
    const Vec3 f = camera.forward();
    const Vec3 s = camera.right();
    const Vec3 u = camera.up();
    // The eye relative to the origin, in double: this subtraction is the whole
    // precision rule. Written out rather than as viewMatrix() * translation(o)
    // because that product forms s.eye and s.origin separately - two
    // earth-sized numbers - and only then cancels them.
    const Vec3 e = camera.eye() - origin;
    return Mat4(s.x, s.y, s.z, -s.dot(e),   //
                u.x, u.y, u.z, -u.dot(e),   //
                -f.x, -f.y, -f.z, f.dot(e), //
                0.0, 0.0, 0.0, 1.0);
}

Mat4 reversedZProjection(const Camera& camera, const ProjectionOptions& options)
{
    const double a = camera.aspect();
    if (a <= 0.0) {
        return Mat4{};
    }
    const double n = camera.nearPlane();
    const double f = camera.farPlane();
    Mat4 projection;
    if (camera.projection() == Projection::Perspective) {
        const double t = 1.0 / std::tan(camera.fieldOfView() * 0.5);
        // Eye-space z is negative in front of the eye and clip w = -z. For
        // depth = (A z + B) / -z to be 1 at z = -n and 0 at z = -f:
        //   (-A n + B) / n = 1  and  -A f + B = 0
        //   => A = n / (f - n), B = n f / (f - n).
        // As f goes to infinity A -> 0 and B -> n: depth = n / distance.
        double depthA = 0.0;
        double depthB = n;
        if (!options.infiniteFar) {
            depthA = n / (f - n);
            depthB = n * f / (f - n);
        }
        projection = Mat4(t / a, 0.0, 0.0, 0.0,      //
                          0.0, t, 0.0, 0.0,          //
                          0.0, 0.0, depthA, depthB,  //
                          0.0, 0.0, -1.0, 0.0);
    } else {
        const double halfHeight = camera.orthographicHeight() * 0.5;
        const double halfWidth = halfHeight * a;
        // No divide: depth = A z + B with A(-n) + B = 1 and A(-f) + B = 0
        //   => A = 1 / (f - n), B = f / (f - n).
        projection = Mat4(1.0 / halfWidth, 0.0, 0.0, 0.0,        //
                          0.0, 1.0 / halfHeight, 0.0, 0.0,       //
                          0.0, 0.0, 1.0 / (f - n), f / (f - n),  //
                          0.0, 0.0, 0.0, 1.0);
    }

    // Backend corrections, applied after the projection. Direct3D needs none.
    // The two touch different rows, so their order does not matter.
    Mat4 correction;
    if (!options.clip.depthZeroToOne) {
        // [0, 1] -> [-1, 1]: z' = 2z - w.
        correction = Mat4(1.0, 0.0, 0.0, 0.0,  //
                          0.0, 1.0, 0.0, 0.0,  //
                          0.0, 0.0, 2.0, -1.0, //
                          0.0, 0.0, 0.0, 1.0);
    }
    if (!options.clip.yUpInNdc) {
        correction(1, 1) = -1.0;
    }
    return correction * projection;
}

Mat4 relativeViewProjection(const Camera& camera, const Vec3& origin,
                            const ProjectionOptions& options)
{
    return reversedZProjection(camera, options) * relativeViewMatrix(camera, origin);
}

std::array<float, 3> relativePosition(const Vec3& position, const Vec3& origin)
{
    return {static_cast<float>(position.x - origin.x), static_cast<float>(position.y - origin.y),
            static_cast<float>(position.z - origin.z)};
}

std::array<float, 16> toFloatColumnMajor(const Mat4& matrix)
{
    std::array<float, 16> out{};
    for (std::size_t column = 0; column < 4; ++column) {
        for (std::size_t row = 0; row < 4; ++row) {
            out[column * 4 + row] = static_cast<float>(matrix(row, column));
        }
    }
    return out;
}

} // namespace katana::qt::gpu
