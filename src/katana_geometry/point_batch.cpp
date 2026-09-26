#include "katana/geometry/point_batch.hpp"

#include <cstddef>
#include <type_traits>

#include "katana/core/cpu_features.hpp"

#include "simd/points_kernels.hpp"

namespace katana::geometry {

using katana::math::AABB;
using katana::math::Mat3;
using katana::math::Mat4;
using katana::math::Vec3;

namespace {

// The kernels read an array of points as an array of doubles.
static_assert(std::is_standard_layout_v<Point2> && sizeof(Point2) == 2 * sizeof(double) &&
              offsetof(Point2, x) == 0 && offsetof(Point2, y) == sizeof(double));
static_assert(std::is_standard_layout_v<Vec3> && sizeof(Vec3) == 3 * sizeof(double) &&
              offsetof(Vec3, x) == 0 && offsetof(Vec3, y) == sizeof(double) &&
              offsetof(Vec3, z) == 2 * sizeof(double));

#if defined(KATANA_HAVE_AVX2_KERNELS) || defined(KATANA_HAVE_NEON_KERNELS)

// Below this a transform kernel spends its time in its scalar tail, and the
// dispatch is pure cost. (Bounds have their own, measured, minimum in the
// header: kBoundsBatchMinimum. Transforms have no caller yet to measure the
// break-even for; 8 is two kernel steps - of the NEON kernels too, which take
// the same four points a step, in registers of two.)
constexpr std::size_t kTransformMinimum = 8;

// The one kernel set this build has: AVX2 on x86-64, NEON on 64-bit ARM.
#if defined(KATANA_HAVE_AVX2_KERNELS)
constexpr auto kKernelLevel = katana::core::SimdLevel::Avx2;
#else
constexpr auto kKernelLevel = katana::core::SimdLevel::Neon;
#endif

[[nodiscard]] bool kernelActive() { return katana::core::activeSimdLevel() == kKernelLevel; }

// Where a coordinate's extreme is zero, WHICH zero the sequential loop kept
// depends on the order it met them: expand() keeps its running value unless the
// new one is strictly beyond it, so the first zero stays and a later zero of
// the other sign does not replace it. A kernel folds its lanes in another
// order, so only the value is taken from it and the sign from the first zero in
// the array - which is where the loop's running extreme became zero, since
// nothing beyond zero followed or the extreme would not be zero.
[[nodiscard]] double firstZero(const double* coordinates, std::size_t stride, std::size_t count)
{
    for (std::size_t i = 0; i < count; ++i) {
        if (coordinates[i * stride] == 0.0) {
            return coordinates[i * stride];
        }
    }
    return 0.0;
}

void settleZero(double& extreme, const double* coordinates, std::size_t stride, std::size_t count)
{
    if (extreme == 0.0) {
        extreme = firstZero(coordinates, stride, count);
    }
}

#endif // KATANA_HAVE_AVX2_KERNELS || KATANA_HAVE_NEON_KERNELS

} // namespace

void transformPoints(const Mat4& m, std::span<Vec3> points)
{
#if defined(KATANA_HAVE_AVX2_KERNELS) || defined(KATANA_HAVE_NEON_KERNELS)
    if (points.size() >= kTransformMinimum && kernelActive()) {
#if defined(KATANA_HAVE_AVX2_KERNELS)
        katana_avx2_transform_points3(m.data.data(), &points.data()->x, points.size());
#else
        katana_neon_transform_points3(m.data.data(), &points.data()->x, points.size());
#endif
        return;
    }
#endif
    for (Vec3& p : points) {
        p = katana::math::transformPoint(m, p);
    }
}

void transformPoints(const Mat3& m, std::span<Point2> points)
{
#if defined(KATANA_HAVE_AVX2_KERNELS) || defined(KATANA_HAVE_NEON_KERNELS)
    if (points.size() >= kTransformMinimum && kernelActive()) {
#if defined(KATANA_HAVE_AVX2_KERNELS)
        katana_avx2_transform_points2(m.data.data(), &points.data()->x, points.size());
#else
        katana_neon_transform_points2(m.data.data(), &points.data()->x, points.size());
#endif
        return;
    }
#endif
    for (Point2& p : points) {
        p = katana::math::transformPoint(m, p);
    }
}

Box2 detail::boundsOfBatch(std::span<const Point2> points)
{
    Box2 box;
#if defined(KATANA_HAVE_AVX2_KERNELS) || defined(KATANA_HAVE_NEON_KERNELS)
    if (kernelActive()) {
        const double* xy = &points.data()->x;
        double out[4];
#if defined(KATANA_HAVE_AVX2_KERNELS)
        katana_avx2_bounds2(xy, points.size(), out);
#else
        katana_neon_bounds2(xy, points.size(), out);
#endif
        for (int k = 0; k < 4; ++k) {
            settleZero(out[k], xy + (k % 2), 2, points.size());
        }
        box.min = Point2(out[0], out[1]);
        box.max = Point2(out[2], out[3]);
        return box;
    }
#endif
    for (const Point2& p : points) {
        box.expand(p);
    }
    return box;
}

AABB detail::boundsOfBatch(std::span<const Vec3> points)
{
    AABB box;
#if defined(KATANA_HAVE_AVX2_KERNELS) || defined(KATANA_HAVE_NEON_KERNELS)
    if (kernelActive()) {
        const double* xyz = &points.data()->x;
        double out[6];
#if defined(KATANA_HAVE_AVX2_KERNELS)
        katana_avx2_bounds3(xyz, points.size(), out);
#else
        katana_neon_bounds3(xyz, points.size(), out);
#endif
        for (int k = 0; k < 6; ++k) {
            settleZero(out[k], xyz + (k % 3), 3, points.size());
        }
        box.min = Vec3(out[0], out[1], out[2]);
        box.max = Vec3(out[3], out[4], out[5]);
        return box;
    }
#endif
    for (const Vec3& p : points) {
        box.expand(p);
    }
    return box;
}

} // namespace katana::geometry
