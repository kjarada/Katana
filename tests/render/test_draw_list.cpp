// DrawList's bounds (draw_list.hpp): the box of every finite vertex a
// primitive uses, each vertex visited once.
//
// The reference below is the walk bounds() replaced - every use of every
// vertex, primitive by primitive - and the result must be its box to the bit,
// the signs of zeros included.

#include <gtest/gtest.h>

#include <cmath>
#include <cstdint>
#include <cstring>
#include <iterator>
#include <limits>
#include <random>
#include <vector>

#include "katana/render/draw_list.hpp"

using katana::math::AABB;
using katana::math::Vec3;
using katana::render::DrawList;
using katana::render::VertexIndex;

namespace {

AABB boundsPrimitiveByPrimitive(const DrawList& list)
{
    AABB box;
    const auto include = [&](VertexIndex index) {
        if (index < list.positions.size() && list.positions[index].isFinite()) {
            box.expand(list.positions[index]);
        }
    };
    for (const auto& triangle : list.triangles) {
        include(triangle.a);
        include(triangle.b);
        include(triangle.c);
    }
    for (const auto& line : list.lines) {
        include(line.a);
        include(line.b);
    }
    for (const auto& point : list.points) {
        include(point.a);
    }
    return box;
}

bool sameBox(const AABB& a, const AABB& b)
{
    const double left[6] = {a.min.x, a.min.y, a.min.z, a.max.x, a.max.y, a.max.z};
    const double right[6] = {b.min.x, b.min.y, b.min.z, b.max.x, b.max.y, b.max.z};
    return std::memcmp(left, right, sizeof(left)) == 0;
}

} // namespace

// A vertex no primitive uses, an index past the last vertex and a vertex
// that is not finite are all left out; the box is of the rest.
TEST(DrawListBounds, AreTheBoxOfTheFiniteVerticesThePrimitivesUse)
{
    DrawList list;
    const auto a = list.addVertex(Vec3(1.0, 2.0, 3.0), 0);
    const auto b = list.addVertex(Vec3(4.0, -5.0, 6.0), 0);
    (void)list.addVertex(Vec3(-100.0, 100.0, 100.0), 0); // never used
    const auto c = list.addVertex(Vec3(std::numeric_limits<double>::quiet_NaN(), 0.0, 0.0), 0);
    const auto d = list.addVertex(Vec3(2.0, 3.0, -7.0), 0);
    list.addLine(a, b);
    list.addTriangle(a, c, 999); // c is not finite, 999 is not a vertex
    list.addPoint(d);

    const AABB box = list.bounds();
    EXPECT_EQ(box.min.x, 1.0);
    EXPECT_EQ(box.min.y, -5.0);
    EXPECT_EQ(box.min.z, -7.0);
    EXPECT_EQ(box.max.x, 4.0);
    EXPECT_EQ(box.max.y, 3.0);
    EXPECT_EQ(box.max.z, 6.0);
}

TEST(DrawListBounds, OfAListWithNothingDrawnAreEmpty)
{
    DrawList list;
    (void)list.addVertex(Vec3(1.0, 2.0, 3.0), 0);
    EXPECT_TRUE(list.bounds().empty());
    list.addLine(5, 6); // neither end is a vertex
    EXPECT_TRUE(list.bounds().empty());
}

// Which zero an extreme is follows the order the vertices are met in
// (AABB::expand keeps the first of two equal values), so bounding each vertex
// once, in index order, could pick the other one. Here index order meets +0
// first and primitive order -0: the result is primitive order's.
TEST(DrawListBounds, WhereAnExtremeIsAZeroItIsTheZeroPrimitiveOrderMeetsFirst)
{
    DrawList list;
    const auto plus = list.addVertex(Vec3(0.0, 1.0, 1.0), 0);
    const auto minus = list.addVertex(Vec3(-0.0, 2.0, 2.0), 0);
    list.addLine(minus, plus);
    const AABB box = list.bounds();
    EXPECT_TRUE(std::signbit(box.min.x));
    EXPECT_TRUE(std::signbit(box.max.x));
    EXPECT_TRUE(sameBox(box, boundsPrimitiveByPrimitive(list)));
}

// Many lists, each vertex used by several primitives in a shuffled order, with
// zeros of both signs, infinities, NaN, unused vertices and indices past the
// end: the same box as the reference every time. The scratch is reused across
// lists of different sizes, as the scene builder reuses it.
TEST(DrawListBounds, AreTheReferenceBoxToTheBitWhateverTheVerticesAndTheOrder)
{
    std::mt19937_64 random(20260925);
    std::uniform_real_distribution<double> value(-1000.0, 1000.0);
    const double specials[] = {0.0,
                               -0.0,
                               std::numeric_limits<double>::infinity(),
                               -std::numeric_limits<double>::infinity(),
                               std::numeric_limits<double>::quiet_NaN(),
                               4.9e-324};
    std::vector<std::uint8_t> scratch;
    int zeroExtremes = 0;
    for (int round = 0; round < 2000; ++round) {
        SCOPED_TRACE(round);
        DrawList list;
        const std::size_t count = 1 + random() % 40;
        // Few distinct values in some rounds, so that ties - the zeros among
        // them - are common.
        const bool coarse = round % 2 == 0;
        const auto coordinate = [&]() {
            if (random() % 4 == 0) {
                return specials[random() % std::size(specials)];
            }
            return coarse ? static_cast<double>(static_cast<int>(random() % 5) - 2) : value(random);
        };
        for (std::size_t i = 0; i < count; ++i) {
            (void)list.addVertex(Vec3(coordinate(), coordinate(), coordinate()), 0);
        }
        // Mostly valid indices, some one past the end or far past it.
        const auto index = [&]() -> VertexIndex {
            const auto pick = random() % 20;
            if (pick == 0) {
                return static_cast<VertexIndex>(count);
            }
            if (pick == 1) {
                return std::numeric_limits<VertexIndex>::max();
            }
            return static_cast<VertexIndex>(random() % count);
        };
        const std::size_t primitives = random() % 30;
        for (std::size_t i = 0; i < primitives; ++i) {
            switch (random() % 3) {
            case 0:
                list.addTriangle(index(), index(), index());
                break;
            case 1:
                list.addLine(index(), index());
                break;
            default:
                list.addPoint(index());
                break;
            }
        }
        const AABB expected = boundsPrimitiveByPrimitive(list);
        ASSERT_TRUE(sameBox(list.bounds(), expected));
        ASSERT_TRUE(sameBox(list.bounds(scratch), expected));
        for (const double extreme : {expected.min.x, expected.min.y, expected.min.z, expected.max.x,
                                     expected.max.y, expected.max.z}) {
            zeroExtremes += extreme == 0.0 ? 1 : 0;
        }
    }
    // The case the fallback exists for, met often enough to have been tested.
    EXPECT_GT(zeroExtremes, 100);
}
