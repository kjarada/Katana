// Binary geometry encoding (PLAN.MD Phase 07; docs/storage.md).
//
// Two jobs here. The first is that a round trip is EXACT - a saved drawing must
// reload as the same drawing, not one that agrees to fifteen digits (Rule 7).
// The second is that this decoder runs on bytes from disk, which may be damaged
// or written by another version, so every malformed input must produce an error
// rather than a crash or a silently wrong shape.

#include <gtest/gtest.h>

#include <bit>
#include <cmath>
#include <cstdint>
#include <span>
#include <variant>
#include <limits>
#include <vector>

#include "katana/entity/geometry_blob.hpp"
#include "katana/entity/serialization.hpp"

using katana::core::ErrorCode;
using katana::entity::DimensionGeometry;
using katana::entity::Geometry;
using katana::entity::geometryFromBlob;
using katana::entity::geometryToBlob;
using katana::entity::PointGeometry;
using katana::entity::TextGeometry;
using katana::geometry::Arc2;
using katana::geometry::Circle2;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;

namespace {

Geometry roundTrip(const Geometry& geometry)
{
    const auto blob = geometryToBlob(geometry);
    EXPECT_TRUE(blob.ok()) << (blob.ok() ? "" : blob.error().describe());
    if (!blob.ok()) {
        return {};
    }
    auto back = geometryFromBlob(*blob);
    EXPECT_TRUE(back.ok()) << (back.ok() ? "" : back.error().describe());
    return back.ok() ? *back : Geometry{};
}

Polyline2 polylineOf(std::size_t vertices, bool closed)
{
    Polyline2 polyline;
    for (std::size_t i = 0; i < vertices; ++i) {
        polyline.vertices.emplace_back(static_cast<double>(i) * 1.7,
                                       std::sin(static_cast<double>(i)) * 3.0);
    }
    polyline.closed = closed;
    return polyline;
}

} // namespace

TEST(GeometryBlob, EveryGeometryKindSurvivesARoundTripExactly)
{
    const std::vector<Geometry> samples = {
        PointGeometry{Point2(1.5, -2.25)},
        Segment2{Point2(0.0, 0.0), Point2(123.456, -789.012)},
        Arc2{Point2(3.0, 4.0), 25.0, 0.5, -1.25},
        polylineOf(9, false),
        polylineOf(4, true),
        Circle2{Point2(-10.0, 10.0), 7.5},
        TextGeometry{Point2(2.0, 3.0), "Lot 42", 2.5, 0.7853981633974483},
        DimensionGeometry{Point2(0.0, 0.0), Point2(10.0, 0.0), 1.5, "10.00 m"},
    };

    for (const Geometry& original : samples) {
        const Geometry back = roundTrip(original);
        // operator== on these is member-wise and exact, which is the point.
        EXPECT_EQ(back, original) << "kind " << original.index();
        EXPECT_EQ(back.index(), original.index());
    }
}

TEST(GeometryBlob, DoublesComeBackBitForBit)
{
    // Stored by bit pattern rather than through any decimal form, so values a
    // text format loses or mangles survive. A drawing that reloads "almost"
    // the same is a drawing that fails a determinism test later.
    const std::vector<double> awkward = {
        0.0,
        -0.0,
        1.0 / 3.0,
        std::numeric_limits<double>::min(),
        std::numeric_limits<double>::denorm_min(),
        std::numeric_limits<double>::max(),
        -std::numeric_limits<double>::max(),
        std::numeric_limits<double>::epsilon(),
        3.141592653589793238462643383279,
        1.0e-300,
        1.0e300,
    };

    for (const double value : awkward) {
        const Geometry original = Segment2{Point2(value, -value), Point2(value * 0.5, value)};
        const Geometry back = roundTrip(original);
        const auto* segment = std::get_if<Segment2>(&back);
        ASSERT_NE(segment, nullptr);
        EXPECT_EQ(std::bit_cast<std::uint64_t>(segment->start.x),
                  std::bit_cast<std::uint64_t>(value))
            << "value " << value;
    }

    // Negative zero specifically: it compares equal to zero but is a different
    // bit pattern, so a test using == alone would not notice losing it.
    const Geometry negativeZero = PointGeometry{Point2(-0.0, 0.0)};
    const Geometry back = roundTrip(negativeZero);
    const auto* point = std::get_if<PointGeometry>(&back);
    ASSERT_NE(point, nullptr);
    EXPECT_TRUE(std::signbit(point->position.x)) << "negative zero lost its sign";
    EXPECT_FALSE(std::signbit(point->position.y));
}

TEST(GeometryBlob, NonFiniteValuesSurviveRatherThanBecomingSomethingElse)
{
    // The model rejects non-finite geometry on the way in, so this should never
    // occur in a real project. It is asserted anyway because the ENCODING must
    // not be the thing that quietly turns an infinity into a large number: if
    // one ever reaches here, it must come back recognisable so the validator
    // can reject it rather than accept nonsense.
    const double infinity = std::numeric_limits<double>::infinity();
    const Geometry original = Segment2{Point2(infinity, -infinity), Point2(0.0, 0.0)};
    const Geometry back = roundTrip(original);
    const auto* segment = std::get_if<Segment2>(&back);
    ASSERT_NE(segment, nullptr);
    EXPECT_TRUE(std::isinf(segment->start.x));
    EXPECT_GT(segment->start.x, 0.0);
    EXPECT_LT(segment->start.y, 0.0);

    const double nan = std::numeric_limits<double>::quiet_NaN();
    const Geometry withNan = PointGeometry{Point2(nan, 1.0)};
    const Geometry nanBack = roundTrip(withNan);
    const auto* nanPoint = std::get_if<PointGeometry>(&nanBack);
    ASSERT_NE(nanPoint, nullptr);
    EXPECT_TRUE(std::isnan(nanPoint->position.x));
}

TEST(GeometryBlob, AnEmptyPolylineAndASingleVertexOneBothRoundTrip)
{
    for (const std::size_t count : {std::size_t{0}, std::size_t{1}, std::size_t{2}}) {
        for (const bool closed : {false, true}) {
            const Geometry original = polylineOf(count, closed);
            EXPECT_EQ(roundTrip(original), original) << count << " vertices, closed " << closed;
        }
    }
}

TEST(GeometryBlob, EmptyStringsAreDistinctFromAbsentOnes)
{
    // An empty textOverride means "show the measured distance"; losing the
    // difference would change what the dimension displays.
    const Geometry blank = DimensionGeometry{Point2(0, 0), Point2(1, 0), 0.5, ""};
    const Geometry filled = DimensionGeometry{Point2(0, 0), Point2(1, 0), 0.5, "TYP"};
    EXPECT_EQ(roundTrip(blank), blank);
    EXPECT_EQ(roundTrip(filled), filled);
    EXPECT_NE(roundTrip(blank), roundTrip(filled));
}

TEST(GeometryBlob, MultiByteTextSurvives)
{
    const Geometry original =
        TextGeometry{Point2(0.0, 0.0), "Grundstück 12 \xE2\x80\x94 café", 3.0, 0.0};
    EXPECT_EQ(roundTrip(original), original);
}

TEST(GeometryBlob, TextThatIsNotValidUtf8IsRefusedRatherThanStored)
{
    // The same rule the JSON writer enforces, so a blob can always be
    // re-encoded as JSON for an export without that writer throwing.
    TextGeometry text;
    text.position = Point2(0.0, 0.0);
    text.text = std::string("caf\xE9"); // CP1252, not UTF-8
    const auto blob = geometryToBlob(Geometry{text});
    ASSERT_FALSE(blob.ok());
    EXPECT_EQ(blob.error().code, ErrorCode::InvalidArgument);
}

// ---- hostile input ---------------------------------------------------------------

TEST(GeometryBlob, AnEmptyOrTruncatedBlobIsAnErrorNotACrash)
{
    EXPECT_FALSE(geometryFromBlob({}).ok());

    const auto full = geometryToBlob(Geometry{polylineOf(6, true)});
    ASSERT_TRUE(full.ok());

    // Every proper prefix must be rejected. This is the loop that would find an
    // unchecked read.
    for (std::size_t length = 0; length < full->size(); ++length) {
        const auto result =
            geometryFromBlob(std::span<const std::byte>(full->data(), length));
        ASSERT_FALSE(result.ok()) << "prefix of " << length << " bytes was accepted";
        EXPECT_EQ(result.error().code, ErrorCode::ParseFailure) << "prefix " << length;
    }
    EXPECT_TRUE(geometryFromBlob(*full).ok()) << "the whole blob must still be fine";
}

TEST(GeometryBlob, TrailingBytesAreRejectedRatherThanIgnored)
{
    auto blob = geometryToBlob(Geometry{Circle2{Point2(1.0, 2.0), 3.0}});
    ASSERT_TRUE(blob.ok());
    blob->push_back(std::byte{0x00});

    const auto result = geometryFromBlob(*blob);
    ASSERT_FALSE(result.ok()) << "extra bytes mean the layouts disagree";
    EXPECT_EQ(result.error().code, ErrorCode::ParseFailure);
}

TEST(GeometryBlob, AnUnknownVersionOrKindIsReportedNotGuessedAt)
{
    auto blob = geometryToBlob(Geometry{Circle2{Point2(1.0, 2.0), 3.0}});
    ASSERT_TRUE(blob.ok());

    auto wrongVersion = *blob;
    wrongVersion[0] = std::byte{99};
    const auto versionResult = geometryFromBlob(wrongVersion);
    ASSERT_FALSE(versionResult.ok());
    EXPECT_EQ(versionResult.error().code, ErrorCode::ParseFailure);

    auto wrongKind = *blob;
    wrongKind[1] = std::byte{77};
    const auto kindResult = geometryFromBlob(wrongKind);
    ASSERT_FALSE(kindResult.ok());
    EXPECT_EQ(kindResult.error().code, ErrorCode::ParseFailure);
}

TEST(GeometryBlob, AnImplausibleLengthIsRejectedBeforeAnythingIsAllocated)
{
    // The classic decoder bug: a corrupt count field turned into a huge
    // allocation, or into a read past the end.
    auto blob = geometryToBlob(Geometry{polylineOf(3, false)});
    ASSERT_TRUE(blob.ok());

    // Vertex count sits right after the two header bytes.
    (*blob)[2] = std::byte{0xFF};
    (*blob)[3] = std::byte{0xFF};
    (*blob)[4] = std::byte{0xFF};
    (*blob)[5] = std::byte{0x7F};

    const auto result = geometryFromBlob(*blob);
    ASSERT_FALSE(result.ok());
    EXPECT_EQ(result.error().code, ErrorCode::ParseFailure);
}

TEST(GeometryBlob, ACorruptStringLengthIsRejected)
{
    auto blob = geometryToBlob(Geometry{TextGeometry{Point2(0, 0), "abc", 2.5, 0.0}});
    ASSERT_TRUE(blob.ok());
    // The string length is the last 4 bytes before the text itself.
    const std::size_t lengthAt = blob->size() - 3 - 4;
    (*blob)[lengthAt] = std::byte{0xFF};
    (*blob)[lengthAt + 1] = std::byte{0xFF};

    const auto result = geometryFromBlob(*blob);
    ASSERT_FALSE(result.ok());
    EXPECT_EQ(result.error().code, ErrorCode::ParseFailure);
}

TEST(GeometryBlob, EverySingleByteCorruptionIsEitherRejectedOrDecodesToSomethingValid)
{
    // Fuzzing by hand: flip each byte of a real blob and require that the
    // decoder never crashes and never reads out of bounds. It is allowed to
    // succeed - flipping a coordinate bit gives a different but perfectly legal
    // shape - but it must not do anything else.
    const auto original = geometryToBlob(Geometry{polylineOf(5, true)});
    ASSERT_TRUE(original.ok());

    std::size_t rejected = 0;
    for (std::size_t i = 0; i < original->size(); ++i) {
        for (const std::uint8_t pattern : {0x00u, 0xFFu, 0x7Fu, 0x80u}) {
            auto damaged = *original;
            damaged[i] = std::byte{pattern};
            const auto result = geometryFromBlob(damaged);
            if (!result.ok()) {
                ++rejected;
                EXPECT_EQ(result.error().code, ErrorCode::ParseFailure) << "byte " << i;
            }
        }
    }
    // A run where nothing was ever rejected would mean the checks are not
    // reachable, i.e. the test proves nothing.
    EXPECT_GT(rejected, 0u);
}

// ---- against the JSON path it replaces --------------------------------------------

TEST(GeometryBlob, AgreesWithTheJsonEncodingItReplaces)
{
    // Both encodings must describe the same geometry, or a project written by
    // one build and read by another would differ.
    const std::vector<Geometry> samples = {
        PointGeometry{Point2(1.5, -2.25)},
        Segment2{Point2(0.0, 0.0), Point2(123.456, -789.012)},
        Arc2{Point2(3.0, 4.0), 25.0, 0.5, -1.25},
        polylineOf(7, true),
        Circle2{Point2(-10.0, 10.0), 7.5},
        TextGeometry{Point2(2.0, 3.0), "Lot 42", 2.5, 0.5},
        DimensionGeometry{Point2(0.0, 0.0), Point2(10.0, 0.0), 1.5, ""},
    };

    for (const Geometry& original : samples) {
        const auto json = katana::entity::geometryToJson(original);
        ASSERT_TRUE(json.ok());
        const auto viaJson = katana::entity::geometryFromJson(*json);
        ASSERT_TRUE(viaJson.ok()) << viaJson.error().describe();

        const Geometry viaBlob = roundTrip(original);
        EXPECT_EQ(viaBlob, *viaJson) << "kind " << original.index();
    }
}

TEST(GeometryBlob, IsSubstantiallySmallerThanTheJsonItReplaces)
{
    // Not the point of the change - the parse cost is - but a format that was
    // BIGGER would be a sign the layout is wrong, so it is worth pinning.
    const Geometry polyline = polylineOf(8, false);
    const auto blob = geometryToBlob(polyline);
    const auto json = katana::entity::geometryToJson(polyline);
    ASSERT_TRUE(blob.ok());
    ASSERT_TRUE(json.ok());

    EXPECT_LT(blob->size(), json->size())
        << "blob " << blob->size() << " json " << json->size();
    // 8 vertices = 128 bytes of doubles, plus a 7-byte header.
    EXPECT_EQ(blob->size(), 2u + 4u + 1u + 8u * 16u);
}
