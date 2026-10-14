// A TIN surface as bytes and back (terrain/tin_codec.hpp): exact, checked on
// the way in, and a newer version refused.

#include <gtest/gtest.h>

#include <bit>
#include <cstdint>
#include <limits>
#include <string>
#include <vector>

#include "katana/terrain/tin_codec.hpp"
#include "katana/terrain/tin_surface.hpp"
#include "terrain_test_support.hpp"

using namespace katana::terrain;
using namespace katana::terrain::testing;
using katana::core::ErrorCode;

namespace {

// Two triangles over the square (0,0)-(10,10), counter-clockwise, with the
// diagonal 0-2 constrained in both (edge 2 of the first runs 2 -> 0, edge 0
// of the second runs 0 -> 2) and the first's bottom edge 0 -> 1 too: masks
// 0b101 and 0b001.
TinSurface squareWithMasks(const std::vector<Point3>& corners)
{
    auto made = TinSurface::create(corners, {{0, 1, 2}, {0, 2, 3}}, {0b101, 0b001});
    EXPECT_TRUE(made.ok()) << (made.ok() ? "" : made.error().describe());
    return made.ok() ? std::move(made).value() : TinSurface{};
}

std::vector<std::uint64_t> bitsOf(const std::vector<Point3>& points)
{
    std::vector<std::uint64_t> bits;
    for (const Point3& point : points) {
        bits.push_back(std::bit_cast<std::uint64_t>(point.x));
        bits.push_back(std::bit_cast<std::uint64_t>(point.y));
        bits.push_back(std::bit_cast<std::uint64_t>(point.z));
    }
    return bits;
}

// Little-endian u32 written into `bytes` at `at`.
void putU32(std::string& bytes, std::size_t at, std::uint32_t value)
{
    for (std::size_t i = 0; i < 4; ++i) {
        bytes[at + i] = static_cast<char>((value >> (8 * i)) & 0xFFU);
    }
}

} // namespace

TEST(TinCodec, ASurfaceComesBackBitForBitWithItsConstrainedEdges)
{
    // Values chosen because a text form would lose them: 0.1 has no finite
    // binary expansion, a negative zero compares equal to zero, a value of
    // 1e-300 and one past 2^53 need every bit, and a survey coordinate in
    // the hundreds of thousands carries its millimetres in the low bits.
    const std::vector<Point3> corners{Point3(0.0, -0.0, 0.1), Point3(10.0, 1e-300, 101.25),
                                      Point3(10.0, 10.0, 9007199254740993.0),
                                      Point3(0.0, 10.0, 554321.987654321)};
    const TinSurface surface = squareWithMasks(corners);
    ASSERT_EQ(surface.triangleCount(), 2u);

    const std::string bytes = encodeTin(surface);
    // 24 + 24 x 4 vertices + 13 x 2 triangles (tin_codec.hpp's layout).
    EXPECT_EQ(bytes.size(), 24u + 24u * 4u + 13u * 2u);
    EXPECT_EQ(encodedTinBytes(4, 2), bytes.size());

    auto back = decodeTin(bytes);
    ASSERT_TRUE(back.ok()) << back.error().describe();
    EXPECT_TRUE(*back == surface) << "vertices, triangles, adjacency and masks";
    EXPECT_EQ(bitsOf(back->vertices()), bitsOf(corners)) << "every coordinate's bit pattern";
    EXPECT_EQ(back->constrainedEdges(), (std::vector<std::uint8_t>{0b101, 0b001}));
    EXPECT_TRUE(back->isEdgeConstrained(1, 0)) << "the diagonal, from the second triangle";
}

TEST(TinCodec, ABuiltSurfaceWithBreaklinesRoundTripsExactly)
{
    katana::test::Random random(11);
    const TinBuildResult built = build(pyramidInput(20.0, 5.0, 40, random));
    ASSERT_FALSE(built.surface.empty());
    auto back = decodeTin(encodeTin(built.surface));
    ASSERT_TRUE(back.ok()) << back.error().describe();
    EXPECT_TRUE(*back == built.surface);
    EXPECT_EQ(bitsOf(back->vertices()), bitsOf(built.surface.vertices()));
    // The pyramid's four ridges are breaklines, so some edge is constrained.
    bool constrained = false;
    for (const std::uint8_t mask : back->constrainedEdges()) {
        constrained = constrained || mask != 0;
    }
    EXPECT_TRUE(constrained);
    // Answers the way the original does: the apex is 5 m up, by construction.
    EXPECT_DOUBLE_EQ(back->elevationAt(10.0, 10.0).value_or(-1.0), 5.0);
}

TEST(TinCodec, TheEmptySurfaceIsTheHeaderAlone)
{
    const std::string bytes = encodeTin(TinSurface{});
    EXPECT_EQ(bytes.size(), 24u);
    auto back = decodeTin(bytes);
    ASSERT_TRUE(back.ok()) << back.error().describe();
    EXPECT_TRUE(back->empty());
}

TEST(TinCodec, ANewerVersionIsRefusedAsUnsupported)
{
    const std::vector<Point3> corners{Point3(0, 0, 0), Point3(10, 0, 0), Point3(10, 10, 1),
                                      Point3(0, 10, 1)};
    std::string bytes = encodeTin(squareWithMasks(corners));
    putU32(bytes, 4, kTinCodecVersion + 1);
    auto refused = decodeTin(bytes);
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::Unsupported);
}

TEST(TinCodec, DamagedBytesAreRefusedNotRead)
{
    const std::vector<Point3> corners{Point3(0, 0, 0), Point3(10, 0, 0), Point3(10, 10, 1),
                                      Point3(0, 10, 1)};
    const std::string good = encodeTin(squareWithMasks(corners));

    // Nothing at all, and a header cut short.
    EXPECT_EQ(decodeTin(std::string()).error().code, ErrorCode::ParseFailure);
    EXPECT_EQ(decodeTin(good.substr(0, 10)).error().code, ErrorCode::ParseFailure);
    // Not the magic.
    std::string magic = good;
    magic[0] = 'X';
    EXPECT_EQ(decodeTin(magic).error().code, ErrorCode::ParseFailure);
    // Version 0, which nothing writes.
    std::string zero = good;
    putU32(zero, 4, 0);
    EXPECT_EQ(decodeTin(zero).error().code, ErrorCode::ParseFailure);
    // A byte short, and a byte over.
    EXPECT_EQ(decodeTin(good.substr(0, good.size() - 1)).error().code, ErrorCode::ParseFailure);
    EXPECT_EQ(decodeTin(good + '\0').error().code, ErrorCode::ParseFailure);
    // A vertex count of 2^63, which a reader that allocated first would try
    // to: refused from the size alone.
    std::string huge = good;
    for (std::size_t i = 0; i < 8; ++i) {
        huge[8 + i] = static_cast<char>(i == 7 ? 0x80 : 0x00);
    }
    EXPECT_EQ(decodeTin(huge).error().code, ErrorCode::ParseFailure);
}

TEST(TinCodec, AnInvalidTriangulationIsRefusedByTheSurfacesOwnChecks)
{
    const std::vector<Point3> corners{Point3(0, 0, 0), Point3(10, 0, 0), Point3(10, 10, 1),
                                      Point3(0, 10, 1)};
    const std::string good = encodeTin(squareWithMasks(corners));
    // The first triangle's first index (just after the four vertices) made 9:
    // there are four vertices.
    std::string outOfRange = good;
    putU32(outOfRange, 24 + 24 * 4, 9);
    auto refused = decodeTin(outOfRange);
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, ErrorCode::InvalidArgument);

    // The first vertex's x made a NaN: a surface holds finite coordinates.
    std::string notFinite = good;
    const auto nan = std::bit_cast<std::uint64_t>(std::numeric_limits<double>::quiet_NaN());
    for (std::size_t i = 0; i < 8; ++i) {
        notFinite[24 + i] = static_cast<char>((nan >> (8 * i)) & 0xFFU);
    }
    EXPECT_EQ(decodeTin(notFinite).error().code, ErrorCode::InvalidArgument);

    // A mask with a bit beyond the three edges.
    std::string mask = good;
    mask[good.size() - 1] = static_cast<char>(0x08);
    EXPECT_EQ(decodeTin(mask).error().code, ErrorCode::InvalidArgument);
}
