// Packing a DrawList for the GPU (gpu/gpu_scene.hpp). No device needed.

#include <gtest/gtest.h>

#include <bit>
#include <cmath>
#include <cstdint>
#include <iterator>
#include <limits>
#include <random>
#include <vector>

#include "gpu/gpu_scene.hpp"
#include "simd_levels.hpp"

using katana::math::Vec3;
using katana::qt::gpu::GpuSceneData;
using katana::qt::gpu::packDrawList;
using katana::qt::gpu::packPointCloud;
using katana::qt::gpu::PointCloudData;
using katana::render::DrawList;
using katana::render::rgba;

TEST(PackDrawList, StoresPositionsAsOffsetsFromTheCentreOfWhatTheListDraws)
{
    DrawList list;
    const auto a = list.addVertex(Vec3(300000.0, 6250000.0, 10.0), rgba(255, 0, 0));
    const auto b = list.addVertex(Vec3(300100.0, 6250000.0, 30.0), rgba(0, 255, 0));
    const auto c = list.addVertex(Vec3(300000.0, 6250200.0, 20.0), rgba(0, 0, 255));
    list.addTriangle(a, b, c);

    GpuSceneData scene;
    packDrawList(list, scene);
    // Box x [300000, 300100], y [6250000, 6250200], z [10, 30]: centre
    // (300050, 6250100, 20).
    EXPECT_EQ(scene.origin.x, 300050.0);
    EXPECT_EQ(scene.origin.y, 6250100.0);
    EXPECT_EQ(scene.origin.z, 20.0);
    ASSERT_EQ(scene.vertices.size(), 3u);
    EXPECT_EQ(scene.vertices[0].x, -50.0f);
    EXPECT_EQ(scene.vertices[0].y, -100.0f);
    EXPECT_EQ(scene.vertices[0].z, -10.0f);
    EXPECT_EQ(scene.vertices[1].x, 50.0f);
    EXPECT_EQ(scene.vertices[2].y, 100.0f);
    EXPECT_EQ(scene.vertices[1].color, rgba(0, 255, 0));
    EXPECT_EQ(scene.triangleIndices, (std::vector<std::uint32_t>{0, 1, 2}));
}

TEST(PackDrawList, DropsPrimitivesThatIndexPastTheVertices)
{
    DrawList list;
    const auto a = list.addVertex(Vec3(0.0, 0.0, 0.0), rgba(255, 255, 255));
    const auto b = list.addVertex(Vec3(1.0, 0.0, 0.0), rgba(255, 255, 255));
    list.addTriangle(a, b, 7);
    list.addLine(a, 9);
    list.addPoint(5);
    list.addLine(a, b);

    GpuSceneData scene;
    packDrawList(list, scene);
    EXPECT_TRUE(scene.triangleIndices.empty());
    EXPECT_EQ(scene.lines.size(), 1u);
    EXPECT_TRUE(scene.points.empty());
}

TEST(PackDrawList, GivesEachLineItsEndpointsColoursAndWidthAndEachPointItsSize)
{
    DrawList list;
    const auto a = list.addVertex(Vec3(-2.0, 0.0, 0.0), rgba(10, 20, 30));
    const auto b = list.addVertex(Vec3(2.0, 4.0, 0.0), rgba(40, 50, 60));
    list.addLine(a, b, 3.0f, 2.0e-4f);
    list.addLine(b, a, 0.25f); // thinner than a pixel is drawn one pixel wide, as on the CPU
    list.addPoint(b, 5.0f);

    GpuSceneData scene;
    packDrawList(list, scene);
    // Box x [-2, 2], y [0, 4]: centre (0, 2, 0).
    ASSERT_EQ(scene.lines.size(), 2u);
    EXPECT_EQ(scene.lines[0].a.x, -2.0f);
    EXPECT_EQ(scene.lines[0].a.y, -2.0f);
    EXPECT_EQ(scene.lines[0].b.x, 2.0f);
    EXPECT_EQ(scene.lines[0].b.y, 2.0f);
    EXPECT_EQ(scene.lines[0].a.color, rgba(10, 20, 30));
    EXPECT_EQ(scene.lines[0].b.color, rgba(40, 50, 60));
    // The width at both ends, so each end is a whole vertex of a line list.
    EXPECT_EQ(scene.lines[0].a.width, 3.0f);
    EXPECT_EQ(scene.lines[0].b.width, 3.0f);
    EXPECT_EQ(scene.lines[1].a.width, 1.0f);
    EXPECT_EQ(scene.lines[1].b.width, 1.0f);
    ASSERT_EQ(scene.points.size(), 1u);
    EXPECT_EQ(scene.points[0].size, 5.0f);
    EXPECT_EQ(scene.points[0].y, 2.0f);
    // 2 vertices x 16 + 2 lines x 40 + 1 point x 24 bytes.
    EXPECT_EQ(scene.byteSize(), 2u * 16u + 2u * 40u + 24u);
}

TEST(PackPointCloud, ThinsEvenlyThroughTheSourceOrderToStayWithinTheBudget)
{
    std::vector<Vec3> positions;
    for (int i = 0; i < 10; ++i) {
        positions.emplace_back(1000.0 + i, 2000.0, 3.0);
    }
    PointCloudData cloud;
    // 10 points, budget 4: every ceil(10 / 4) = 3rd point, i.e. 0, 3, 6, 9.
    packPointCloud(positions, {}, rgba(1, 2, 3), Vec3(1000.0, 2000.0, 0.0), 4, cloud);
    ASSERT_EQ(cloud.points.size(), 4u);
    EXPECT_EQ(cloud.points[0].x, 0.0f);
    EXPECT_EQ(cloud.points[1].x, 3.0f);
    EXPECT_EQ(cloud.points[3].x, 9.0f);
    EXPECT_EQ(cloud.points[3].z, 3.0f);
    EXPECT_EQ(cloud.points[0].color, rgba(1, 2, 3));
    EXPECT_EQ(cloud.sourceCount, 10u);

    // Budget 5: every 2nd, 5 points. Budget 0: no limit.
    packPointCloud(positions, {}, rgba(1, 2, 3), Vec3(), 5, cloud);
    EXPECT_EQ(cloud.points.size(), 5u);
    packPointCloud(positions, {}, rgba(1, 2, 3), Vec3(), 0, cloud);
    EXPECT_EQ(cloud.points.size(), 10u);
}

TEST(PackPointCloud, TakesTheEnginesPointsInTheirOwnColourAboutTheCentreOfTheirBounds)
{
    katana::pointcloud::PointCloud source;
    // Four points of a survey-coordinate cloud; the second has no colour.
    const double x0 = 300000.0;
    const double y0 = 6250000.0;
    source.points = {
        {x0 + 0.0, y0 + 0.0, 10.0, 0.0, 2, 200, 10, 20, true},
        {x0 + 4.0, y0 + 0.0, 12.0, 0.0, 2, 0, 0, 0, false},
        {x0 + 4.0, y0 + 8.0, 14.0, 0.0, 2, 30, 40, 50, true},
        {x0 + 0.0, y0 + 8.0, 16.0, 0.0, 2, 60, 70, 80, true},
    };
    source.bounds = {x0, y0, 10.0, x0 + 4.0, y0 + 8.0, 16.0};
    PointCloudData cloud;
    // Budget 2 of 4: every ceil(4 / 2) = 2nd point - the first and the third.
    packPointCloud(source, rgba(9, 9, 9), 2, cloud);
    // The centre of the bounds: (x0 + 2, y0 + 4, 13).
    EXPECT_EQ(cloud.origin, Vec3(x0 + 2.0, y0 + 4.0, 13.0));
    ASSERT_EQ(cloud.points.size(), 2u);
    EXPECT_EQ(cloud.points[0].x, -2.0f);
    EXPECT_EQ(cloud.points[0].y, -4.0f);
    EXPECT_EQ(cloud.points[0].z, -3.0f);
    EXPECT_EQ(cloud.points[0].color, rgba(200, 10, 20));
    EXPECT_EQ(cloud.points[1].x, 2.0f);
    EXPECT_EQ(cloud.points[1].y, 4.0f);
    EXPECT_EQ(cloud.points[1].z, 1.0f);
    EXPECT_EQ(cloud.points[1].color, rgba(30, 40, 50));
    EXPECT_EQ(cloud.sourceCount, 4u);

    // All of them: the uncoloured one takes the fallback.
    packPointCloud(source, rgba(9, 9, 9), 0, cloud);
    ASSERT_EQ(cloud.points.size(), 4u);
    EXPECT_EQ(cloud.points[1].color, rgba(9, 9, 9));
}

namespace {

// The same float, or both NaN: a NaN's payload is not compared
// (docs/performance.md, "NaN payloads").
bool sameFloat(float a, float b)
{
    return (std::isnan(a) && std::isnan(b)) ||
           std::bit_cast<std::uint32_t>(a) == std::bit_cast<std::uint32_t>(b);
}

} // namespace

// The AVX2 or NEON packing (simd/pack_avx2.cpp, pack_neon.cpp) against the
// loop it replaces, through packDrawList itself: every length from 0 to 70,
// so every tail of one to three past every block edge, and lists shorter than
// one block, over coordinates
// as survey data has them and the values a conversion can get wrong - both
// zeros, infinities, NaN, subnormals, doubles too large for a float and too
// small for one.
TEST(PackDrawList, EveryVertexPacksToTheSameBitsAtEachSimdLevel)
{
    KATANA_REQUIRE_SIMD_KERNELS();
    using katana::core::SimdLevel;
    std::mt19937_64 random(20260926);
    std::uniform_real_distribution<double> survey(-7.0e6, 7.0e6);
    const double specials[] = {0.0, -0.0, std::numeric_limits<double>::infinity(),
                               -std::numeric_limits<double>::infinity(),
                               std::numeric_limits<double>::quiet_NaN(), 6250000.123456789,
                               -300000.5,
                               4.9e-324, // the smallest subnormal double
                               1.0e-40,  // a subnormal float
                               1.0e-50,  // below every float: rounds to zero
                               7.0e38,   // above every float: rounds to infinity
                               // Half an ulp above FLT_MAX: the tie that rounds
                               // to infinity, to even.
                               3.4028235677973366e38};
    const auto coordinate = [&]() {
        return random() % 5 == 0 ? specials[random() % std::size(specials)] : survey(random);
    };
    for (std::size_t count = 0; count <= 70; ++count) {
        for (int round = 0; round < 4; ++round) {
            SCOPED_TRACE(::testing::Message() << count << " vertices, round " << round);
            DrawList list;
            for (std::size_t i = 0; i < count; ++i) {
                list.addVertex(Vec3(coordinate(), coordinate(), coordinate()),
                               static_cast<katana::render::Rgba>(random()));
            }
            if (count >= 3) {
                list.addTriangle(0, 1, 2);
                list.addLine(0, static_cast<katana::render::VertexIndex>(count - 1), 2.0f);
                list.addPoint(1, 4.0f);
            }
            // The origin is a double like the positions: a small z, as a
            // scene's usually is.
            const Vec3 origin(survey(random), survey(random), survey(random) * 1.0e-4);
            GpuSceneData scalar;
            GpuSceneData vector;
            katana::test::atSimdLevel(SimdLevel::Scalar,
                                      [&] { packDrawList(list, origin, scalar); });
            katana::test::atSimdLevel(katana::test::kernelLevel(),
                                      [&] { packDrawList(list, origin, vector); });

            ASSERT_EQ(vector.vertices.size(), scalar.vertices.size());
            for (std::size_t i = 0; i < count; ++i) {
                const auto& s = scalar.vertices[i];
                const auto& v = vector.vertices[i];
                ASSERT_TRUE(sameFloat(v.x, s.x) && sameFloat(v.y, s.y) && sameFloat(v.z, s.z))
                    << "vertex " << i << ": " << v.x << " " << v.y << " " << v.z << " against "
                    << s.x << " " << s.y << " " << s.z;
                ASSERT_EQ(v.color, s.color) << "vertex " << i;
            }
            // What is built from the vertices is built from the same bits.
            ASSERT_EQ(vector.lines.size(), scalar.lines.size());
            for (std::size_t i = 0; i < scalar.lines.size(); ++i) {
                EXPECT_TRUE(sameFloat(vector.lines[i].a.x, scalar.lines[i].a.x));
                EXPECT_TRUE(sameFloat(vector.lines[i].b.z, scalar.lines[i].b.z));
                EXPECT_EQ(vector.lines[i].b.color, scalar.lines[i].b.color);
            }
            EXPECT_EQ(vector.triangleIndices, scalar.triangleIndices);
            EXPECT_EQ(vector.points.size(), scalar.points.size());
        }
    }
}

// The values themselves, not only the agreement of two paths that could share
// a mistake: a list long enough for the kernel, each vertex's offset worked
// out here. The origin is exact in double and each offset exact in float, so
// no rounding stands between the arithmetic and the expectation.
TEST(PackDrawList, TheKernelWritesEachVertexsOffsetAndColourWhereTheScalarLoopDoes)
{
    DrawList list;
    for (int i = 0; i < 23; ++i) { // five blocks of four and a tail of three
        list.addVertex(Vec3(300000.0 + i, 6250000.0 - 2.0 * i, 10.0 + 0.5 * i),
                       static_cast<katana::render::Rgba>(0xFF000000u + static_cast<unsigned>(i)));
    }
    GpuSceneData scene;
    packDrawList(list, Vec3(300000.0, 6250000.0, 10.0), scene);
    ASSERT_EQ(scene.vertices.size(), 23u);
    for (int i = 0; i < 23; ++i) {
        SCOPED_TRACE(i);
        EXPECT_EQ(scene.vertices[i].x, static_cast<float>(i));
        EXPECT_EQ(scene.vertices[i].y, static_cast<float>(-2 * i));
        EXPECT_EQ(scene.vertices[i].z, 0.5f * static_cast<float>(i));
        EXPECT_EQ(scene.vertices[i].color, 0xFF000000u + static_cast<unsigned>(i));
    }
}
