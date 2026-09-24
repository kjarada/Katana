// Packing a DrawList for the GPU (gpu/gpu_scene.hpp). No device needed.

#include <gtest/gtest.h>

#include <vector>

#include "gpu/gpu_scene.hpp"

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
    EXPECT_EQ(scene.lines[0].ax, -2.0f);
    EXPECT_EQ(scene.lines[0].ay, -2.0f);
    EXPECT_EQ(scene.lines[0].bx, 2.0f);
    EXPECT_EQ(scene.lines[0].by, 2.0f);
    EXPECT_EQ(scene.lines[0].colorA, rgba(10, 20, 30));
    EXPECT_EQ(scene.lines[0].colorB, rgba(40, 50, 60));
    EXPECT_EQ(scene.lines[0].width, 3.0f);
    EXPECT_EQ(scene.lines[1].width, 1.0f);
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
