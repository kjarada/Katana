// Batch transforms and bounds, held to the point-by-point loops they replace:
// the same bits at every SIMD level. The AVX2 kernels take four points a step,
// so sizes run through every remainder and past the dispatch minimums: 8 for
// transforms, kBoundsBatchMinimum (16) for bounds. A hand-worked bounds case
// must be at least that long, or at the AVX2 level it would test the inline
// loop and not the kernel; each one asserts so.

#include <gtest/gtest.h>

#include <array>
#include <bit>
#include <cmath>
#include <cstdint>
#include <limits>
#include <vector>

#include "katana/geometry/mesh.hpp"
#include "katana/geometry/point_batch.hpp"
#include "simd_levels.hpp"

using katana::core::SimdLevel;
using katana::geometry::Box2;
using katana::geometry::boundsOf;
using katana::geometry::kBoundsBatchMinimum;
using katana::geometry::Point2;
using katana::geometry::transformPoints;
using katana::math::AABB;
using katana::math::Mat3;
using katana::math::Mat4;
using katana::math::Vec3;
using katana::test::atSimdLevel;

namespace {

constexpr double kInf = std::numeric_limits<double>::infinity();
const double kNaN = std::numeric_limits<double>::quiet_NaN();

// Equal bits; for a NaN, only that both are NaN. A NaN's payload is not a
// result: which operand's payload an addition keeps depends on operand order,
// and the compiler itself may swap the operands of a commutative operation in
// the scalar reference.
bool same(double a, double b)
{
    if (std::isnan(a) || std::isnan(b)) {
        return std::isnan(a) && std::isnan(b);
    }
    return std::bit_cast<std::uint64_t>(a) == std::bit_cast<std::uint64_t>(b);
}

bool same(const Point2& a, const Point2& b) { return same(a.x, b.x) && same(a.y, b.y); }
bool same(const Vec3& a, const Vec3& b) { return same(a.x, b.x) && same(a.y, b.y) && same(a.z, b.z); }
bool same(const Box2& a, const Box2& b) { return same(a.min, b.min) && same(a.max, b.max); }
bool same(const AABB& a, const AABB& b) { return same(a.min, b.min) && same(a.max, b.max); }

struct Random {
    std::uint64_t state = 0x9E3779B97F4A7C15ull;
    std::uint64_t next()
    {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        return state;
    }
    double uniform(double low, double high)
    {
        return low + (high - low) * static_cast<double>(next() >> 11) * 0x1.0p-53;
    }
    // A survey coordinate, now and then something awkward.
    double coordinate(double centre)
    {
        switch (next() % 16) {
        case 0:
            return 0.0;
        case 1:
            return -0.0;
        case 2:
            return kNaN;
        case 3:
            return (next() % 2 == 0) ? kInf : -kInf;
        default:
            return centre + uniform(-5000.0, 5000.0);
        }
    }
};

} // namespace

TEST(PointBatch, A3dTransformGivesTheValuesWorkedByHandAtEveryLevel)
{
    // M = translate (10, 20, 30) after scaling by 2: the rows are
    // [2 0 0 10], [0 2 0 20], [0 0 2 30], exact. (k, -k, k/2) maps to
    // (2k + 10, -2k + 20, k + 30); every product and sum is exact in double.
    const Mat4 m = Mat4::translation(Vec3(10.0, 20.0, 30.0)) * Mat4::scaling(Vec3(2.0, 2.0, 2.0));
    for (const SimdLevel level : {SimdLevel::Scalar, SimdLevel::Avx2}) {
        if (level == SimdLevel::Avx2 && !katana::test::avx2Available()) {
            continue;
        }
        std::vector<Vec3> points;
        for (int k = 0; k <= 10; ++k) {
            points.emplace_back(k, -k, 0.5 * k);
        }
        atSimdLevel(level, [&] { transformPoints(m, points); });
        for (int k = 0; k <= 10; ++k) {
            EXPECT_EQ(points[static_cast<std::size_t>(k)], Vec3(2.0 * k + 10.0, -2.0 * k + 20.0, k + 30.0))
                << "k " << k << " at " << katana::core::toString(level);
        }
    }
}

TEST(PointBatch, A2dTransformGivesTheValuesWorkedByHandAtEveryLevel)
{
    // A quarter turn and a shift by (5, 7), written out so that cos and sin
    // leave no rounding: (x, y) -> (-y + 5, x + 7). (k, 2k) -> (5 - 2k, k + 7).
    const Mat3 m(0.0, -1.0, 5.0, 1.0, 0.0, 7.0, 0.0, 0.0, 1.0);
    for (const SimdLevel level : {SimdLevel::Scalar, SimdLevel::Avx2}) {
        if (level == SimdLevel::Avx2 && !katana::test::avx2Available()) {
            continue;
        }
        std::vector<Point2> points;
        for (int k = 0; k <= 10; ++k) {
            points.emplace_back(k, 2.0 * k);
        }
        atSimdLevel(level, [&] { transformPoints(m, points); });
        for (int k = 0; k <= 10; ++k) {
            EXPECT_EQ(points[static_cast<std::size_t>(k)], Point2(5.0 - 2.0 * k, k + 7.0))
                << "k " << k << " at " << katana::core::toString(level);
        }
    }
}

TEST(PointBatch, TransformsEqualTransformPointOnEveryPointBitForBitAtEveryLevel)
{
    KATANA_REQUIRE_AVX2();
    Random random;
    for (int round = 0; round < 300; ++round) {
        const std::size_t count = static_cast<std::size_t>(round % 71);
        const Mat4 m4 = Mat4::translation(Vec3(random.uniform(-1e5, 1e5), random.uniform(-1e5, 1e5), 3.0)) *
                        Mat4::rotation(Vec3(random.uniform(-1, 1), random.uniform(-1, 1), 1.0), random.uniform(-3, 3)) *
                        Mat4::scaling(Vec3(1.0001, 0.9999, 1.5));
        const Mat3 m3 = Mat3::translation(Point2(random.uniform(-1e5, 1e5), random.uniform(-1e5, 1e5))) *
                        Mat3::rotation(random.uniform(-3, 3));
        std::vector<Vec3> in3;
        std::vector<Point2> in2;
        for (std::size_t i = 0; i < count; ++i) {
            in3.emplace_back(random.coordinate(300000.0), random.coordinate(6250000.0), random.coordinate(40.0));
            in2.emplace_back(random.coordinate(300000.0), random.coordinate(6250000.0));
        }
        auto scalar3 = in3;
        auto avx3 = in3;
        auto scalar2 = in2;
        auto avx2 = in2;
        atSimdLevel(SimdLevel::Scalar, [&] { transformPoints(m4, scalar3); });
        atSimdLevel(SimdLevel::Avx2, [&] { transformPoints(m4, avx3); });
        atSimdLevel(SimdLevel::Scalar, [&] { transformPoints(m3, scalar2); });
        atSimdLevel(SimdLevel::Avx2, [&] { transformPoints(m3, avx2); });
        for (std::size_t i = 0; i < count; ++i) {
            const Vec3 reference3 = katana::math::transformPoint(m4, in3[i]);
            const Point2 reference2 = katana::math::transformPoint(m3, in2[i]);
            ASSERT_TRUE(same(scalar3[i], reference3)) << "round " << round << " point " << i;
            ASSERT_TRUE(same(avx3[i], reference3)) << "round " << round << " point " << i;
            ASSERT_TRUE(same(scalar2[i], reference2)) << "round " << round << " point " << i;
            ASSERT_TRUE(same(avx2[i], reference2)) << "round " << round << " point " << i;
        }
    }
}

TEST(PointBatch, BoundsGiveTheBoxWorkedByHandAtEveryLevel)
{
    // Seventeen points, so the AVX2 path takes four steps of four and a tail
    // of one. Reading down the columns: x from -3.5 (point 6) to 7 (point 16,
    // the tail); y from -4 (point 4) to 9 (point 13).
    const std::vector<Point2> flat = {{3, -1},     {-2, 5},  {6, 0.5},   {1, 1},     {0.25, -4},
                                      {6, 2},      {-3.5, 3}, {2, 8},     {4, -2},    {1.5, 1.5},
                                      {-1, -1},    {5, 4},   {0.5, 0.75}, {2.5, 9},  {6.5, -3},
                                      {-3, 7},     {7, 2.5}};
    // Eighteen points, a tail of two: x from -8 (point 6) to 6.5 (point 17,
    // the tail); y from -6 (point 2) to 9 (point 4); z from -4 (point 7) to 10
    // (point 8).
    const std::vector<Vec3> solid = {{1, 2, 3},    {-1, 0.5, 7},  {4, -6, 2},   {2, 2, 2},
                                     {0.5, 9, -1}, {3, 3, 3},     {-8, 1, 1},   {5, 5, -4},
                                     {6, -2, 10},  {1, 1, 1},     {2, 3, 4},    {-2, -1, 5},
                                     {3, 4, -2},   {1.5, 2.5, 6}, {-4, 0.5, 0.25}, {5.5, -5, 9},
                                     {0.75, 8, -3}, {6.5, 1, 2}};
    ASSERT_GE(flat.size(), kBoundsBatchMinimum);
    ASSERT_GE(solid.size(), kBoundsBatchMinimum);
    for (const SimdLevel level : {SimdLevel::Scalar, SimdLevel::Avx2}) {
        if (level == SimdLevel::Avx2 && !katana::test::avx2Available()) {
            continue;
        }
        const Box2 box = atSimdLevel(level, [&] { return boundsOf(flat); });
        EXPECT_EQ(box, Box2(Point2(-3.5, -4.0), Point2(7.0, 9.0))) << katana::core::toString(level);
        const AABB cube = atSimdLevel(level, [&] { return boundsOf(solid); });
        EXPECT_EQ(cube.min, Vec3(-8.0, -6.0, -4.0)) << katana::core::toString(level);
        EXPECT_EQ(cube.max, Vec3(6.5, 9.0, 10.0)) << katana::core::toString(level);
    }
}

TEST(PointBatch, WhereTheExtremeIsZeroTheBoundsKeepTheFirstZeroMetAsExpandDoes)
{
    // expand() replaces its running minimum only by something strictly less,
    // and -0 < +0 is false, so of the zeros the first one met stays. x runs
    // 1, 2, 3, -0, +0, 5, 6, ..., 16: the minimum is the -0 at place 3.
    //
    // The places are chosen against the AVX2 path, which keeps one running
    // minimum per lane - point i in lane i mod 4 - and folds the lanes 0 to 3
    // at the end: it meets the +0 at place 4 (lane 0) before the -0 at place 3
    // (lane 3), so a kernel that merely folded would answer +0.
    const double xs[] = {1.0,  2.0,  3.0,  -0.0, 0.0,  5.0,  6.0,  7.0, 8.0,
                         9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 16.0};
    std::vector<Point2> points;
    for (const double x : xs) {
        points.emplace_back(x, -x); // y: the maximum is -(-0) = +0 at place 3
    }
    ASSERT_GE(points.size(), kBoundsBatchMinimum);
    for (const SimdLevel level : {SimdLevel::Scalar, SimdLevel::Avx2}) {
        if (level == SimdLevel::Avx2 && !katana::test::avx2Available()) {
            continue;
        }
        const Box2 box = atSimdLevel(level, [&] { return boundsOf(points); });
        EXPECT_EQ(box.min.x, 0.0);
        EXPECT_TRUE(std::signbit(box.min.x)) << katana::core::toString(level);
        EXPECT_EQ(box.max.y, 0.0);
        EXPECT_FALSE(std::signbit(box.max.y)) << katana::core::toString(level);
    }
    // And the other way round: +0 first.
    std::swap(points[3], points[4]);
    for (const SimdLevel level : {SimdLevel::Scalar, SimdLevel::Avx2}) {
        if (level == SimdLevel::Avx2 && !katana::test::avx2Available()) {
            continue;
        }
        const Box2 box = atSimdLevel(level, [&] { return boundsOf(points); });
        EXPECT_FALSE(std::signbit(box.min.x)) << katana::core::toString(level);
        EXPECT_TRUE(std::signbit(box.max.y)) << katana::core::toString(level);
    }
}

TEST(PointBatch, BoundsPassOverNaNCoordinatesAsExpandDoes)
{
    // A NaN compares false with everything, so expand() never takes it:
    // x = NaN, 3, 1, NaN, 2, 5, 4, NaN, 0.5, 1.5, NaN, 2.5, 3.5, NaN, 4.5,
    // 0.75, NaN has bounds 0.5 to 5 - with a NaN first, some inside the
    // kernel's steps, and one last, alone in the tail.
    const double xs[] = {kNaN, 3.0,  1.0, kNaN, 2.0,  5.0, 4.0,  kNaN, 0.5,
                         1.5,  kNaN, 2.5, 3.5,  kNaN, 4.5, 0.75, kNaN};
    std::vector<Point2> points;
    for (const double x : xs) {
        points.emplace_back(x, 1.0);
    }
    ASSERT_GE(points.size(), kBoundsBatchMinimum);
    for (const SimdLevel level : {SimdLevel::Scalar, SimdLevel::Avx2}) {
        if (level == SimdLevel::Avx2 && !katana::test::avx2Available()) {
            continue;
        }
        const Box2 box = atSimdLevel(level, [&] { return boundsOf(points); });
        EXPECT_EQ(box.min.x, 0.5) << katana::core::toString(level);
        EXPECT_EQ(box.max.x, 5.0) << katana::core::toString(level);
    }
}

TEST(PointBatch, TheBoundsOfNothingAreEmpty)
{
    for (const SimdLevel level : {SimdLevel::Scalar, SimdLevel::Avx2}) {
        if (level == SimdLevel::Avx2 && !katana::test::avx2Available()) {
            continue;
        }
        EXPECT_TRUE(atSimdLevel(level, [] { return boundsOf(std::vector<Point2>{}); }).empty());
        EXPECT_TRUE(atSimdLevel(level, [] { return boundsOf(std::vector<Vec3>{}); }).empty());
    }
}

TEST(PointBatch, BoundsEqualTheExpandLoopBitForBitAtEveryLevel)
{
    KATANA_REQUIRE_AVX2();
    Random random;
    for (int round = 0; round < 600; ++round) {
        const std::size_t count = static_cast<std::size_t>(round % 71);
        std::vector<Point2> flat;
        std::vector<Vec3> solid;
        for (std::size_t i = 0; i < count; ++i) {
            // Centred on zero now and then, so zeros can be the extremes.
            const double centre = round % 5 == 0 ? 0.0 : 300000.0;
            flat.emplace_back(random.coordinate(centre), random.coordinate(-centre));
            solid.emplace_back(random.coordinate(centre), random.coordinate(centre), random.coordinate(0.0));
        }
        Box2 loop2;
        for (const Point2& p : flat) {
            loop2.expand(p);
        }
        AABB loop3;
        for (const Vec3& p : solid) {
            loop3.expand(p);
        }
        ASSERT_TRUE(same(atSimdLevel(SimdLevel::Scalar, [&] { return boundsOf(flat); }), loop2)) << round;
        ASSERT_TRUE(same(atSimdLevel(SimdLevel::Avx2, [&] { return boundsOf(flat); }), loop2)) << round;
        ASSERT_TRUE(same(atSimdLevel(SimdLevel::Scalar, [&] { return boundsOf(solid); }), loop3)) << round;
        ASSERT_TRUE(same(atSimdLevel(SimdLevel::Avx2, [&] { return boundsOf(solid); }), loop3)) << round;
    }
}

TEST(PointBatch, APolylineAndAMeshAreBoundedThroughTheBatchPath)
{
    // Twenty vertices, past the batch minimum: x = 0..19, y = 19..0 with one
    // dip to -3 at vertex 5. The mesh: (i, -i, i/2), so x 0..19, y -19..0,
    // z 0..9.5.
    katana::geometry::Polyline2 line;
    for (int i = 0; i < 20; ++i) {
        line.vertices.emplace_back(i, i == 5 ? -3.0 : 19.0 - i);
    }
    katana::geometry::TriangleMesh mesh;
    for (int i = 0; i < 20; ++i) {
        mesh.vertices.emplace_back(i, -i, i * 0.5);
    }
    ASSERT_GE(line.vertices.size(), kBoundsBatchMinimum);
    ASSERT_GE(mesh.vertices.size(), kBoundsBatchMinimum);
    for (const SimdLevel level : {SimdLevel::Scalar, SimdLevel::Avx2}) {
        if (level == SimdLevel::Avx2 && !katana::test::avx2Available()) {
            continue;
        }
        const Box2 box = atSimdLevel(level, [&] { return line.boundingBox(); });
        EXPECT_EQ(box, Box2(Point2(0.0, -3.0), Point2(19.0, 19.0))) << katana::core::toString(level);
        const AABB cube = atSimdLevel(level, [&] { return mesh.bounds(); });
        EXPECT_EQ(cube.min, Vec3(0.0, -19.0, 0.0)) << katana::core::toString(level);
        EXPECT_EQ(cube.max, Vec3(19.0, 0.0, 9.5)) << katana::core::toString(level);
    }
}

// The short path is the expand loop compiled into the caller, with no call into
// the batch code - so plainly that a constant expression can run it. A
// drawing's strings are mostly shorter than kBoundsBatchMinimum and are bounded
// for every candidate on every repaint; through a call (and, from 8 points, the
// kernel) they were measured 15-35% slower than the loop they replaced.
// Worked by hand: x runs 2, -1, 4; y runs 3, 5, -2; z runs 1, 0.5, 6.
namespace {
constexpr std::array<Point2, 3> kShortFlat = {Point2(2.0, 3.0), Point2(-1.0, 5.0),
                                              Point2(4.0, -2.0)};
constexpr std::array<Vec3, 3> kShortSolid = {Vec3(2.0, 3.0, 1.0), Vec3(-1.0, 5.0, 0.5),
                                             Vec3(4.0, -2.0, 6.0)};
static_assert(kShortFlat.size() < kBoundsBatchMinimum);
static_assert(boundsOf(kShortFlat) == Box2(Point2(-1.0, -2.0), Point2(4.0, 5.0)));
static_assert(boundsOf(kShortSolid).min == Vec3(-1.0, -2.0, 0.5));
static_assert(boundsOf(kShortSolid).max == Vec3(4.0, 5.0, 6.0));
} // namespace

TEST(PointBatch, AnArrayShorterThanTheBatchMinimumIsBoundedInlineWithoutACall)
{
    // The static_asserts above are the test of "inline": they compile only
    // while the short path is in the header. At run time the same arrays give
    // the same boxes at every level.
    for (const SimdLevel level : {SimdLevel::Scalar, SimdLevel::Avx2}) {
        if (level == SimdLevel::Avx2 && !katana::test::avx2Available()) {
            continue;
        }
        EXPECT_EQ(atSimdLevel(level, [] { return boundsOf(kShortFlat); }),
                  Box2(Point2(-1.0, -2.0), Point2(4.0, 5.0)))
            << katana::core::toString(level);
        const AABB cube = atSimdLevel(level, [] { return boundsOf(kShortSolid); });
        EXPECT_EQ(cube.min, Vec3(-1.0, -2.0, 0.5)) << katana::core::toString(level);
        EXPECT_EQ(cube.max, Vec3(4.0, 5.0, 6.0)) << katana::core::toString(level);
    }
}
