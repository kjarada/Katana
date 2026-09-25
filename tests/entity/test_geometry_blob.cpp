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
using katana::entity::LabelGeometry;
using katana::entity::LeaderGeometry;
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
        katana::geometry::CurvePolyline2{
            {{Point2(0.0, 0.0), 0.5, 12.25}, {Point2(5.0, 1.0), -0.0, std::nullopt},
             {Point2(7.0, 4.0), -1.5, -0.0}},
            true},
        katana::geometry::Ellipse2{Point2(3.0, 4.0), katana::geometry::Vec2(6.0, -2.0), 0.35,
                                   -0.75, 1.25},
        *katana::geometry::Spline2::throughPoints(
            {Point2(0.0, 0.0), Point2(2.0, 3.0), Point2(5.0, 1.0), Point2(8.0, 4.0)}, 3),
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
        katana::geometry::CurvePolyline2{
            {{Point2(0.0, 0.0), 0.5, 12.25}, {Point2(5.0, 1.0), -0.0, std::nullopt},
             {Point2(7.0, 4.0), -1.5, -0.0}},
            true},
        katana::geometry::Ellipse2{Point2(3.0, 4.0), katana::geometry::Vec2(6.0, -2.0), 0.35,
                                   -0.75, 1.25},
        *katana::geometry::Spline2::throughPoints(
            {Point2(0.0, 0.0), Point2(2.0, 3.0), Point2(5.0, 1.0), Point2(8.0, 4.0)}, 3),
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

TEST(GeometryBlobWireFormat, TheKindByteIsPinnedToTheVariantOrder)
{
    // THE test that makes the "append only" rule enforceable rather than a
    // comment. geometryToBlob writes `geometry.index()` as the on-disk kind
    // byte, so reordering or inserting an alternative reinterprets every
    // project ever saved. Most shifts are caught by the payload-length checks,
    // but two kinds of EQUAL payload size swap with no complaint at all - a
    // Line and any other two-point kind, for instance - and the drawing simply
    // reloads as the wrong shapes.
    //
    // The JSON path is unaffected because it keys on the type NAME, which is
    // exactly why this would be missed: the bug would look encoding-specific.
    struct Expected {
        std::uint8_t kind;
        const char* name;
        Geometry geometry;
    };
    const std::vector<Expected> pinned = {
        {0, "Point", PointGeometry{Point2(1.0, 2.0)}},
        {1, "Line", Segment2{Point2(0.0, 0.0), Point2(1.0, 1.0)}},
        {2, "Arc", Arc2{Point2(0.0, 0.0), 1.0, 0.0, 1.0}},
        {3, "Polyline", polylineOf(3, false)},
        {4, "Circle", Circle2{Point2(0.0, 0.0), 1.0}},
        {5, "Text", TextGeometry{Point2(0.0, 0.0), "x", 2.5, 0.0}},
        {6, "Dimension", DimensionGeometry{Point2(0.0, 0.0), Point2(1.0, 0.0), 0.0, ""}},
        {7, "Label", LabelGeometry{.target = 1, .style = "S"}},
        {8, "Leader", LeaderGeometry{.vertices = {Point2(0.0, 0.0), Point2(1.0, 1.0)}}},
        // The drawing system's, appended after the annotation system's.
        {9, "CurvePolyline",
         katana::geometry::CurvePolyline2::fromPoints({Point2(0.0, 0.0), Point2(1.0, 1.0)})},
        {10, "Ellipse", katana::geometry::Ellipse2{}},
        {11, "Spline",
         *katana::geometry::Spline2::fromControlPoints({Point2(0.0, 0.0), Point2(1.0, 1.0)}, 1)},
    };

    ASSERT_EQ(pinned.size(), std::variant_size_v<Geometry>)
        << "a geometry kind was added; append it here and confirm it went on the END";

    for (const Expected& item : pinned) {
        EXPECT_EQ(item.geometry.index(), item.kind) << item.name << " moved in the variant";
        EXPECT_EQ(static_cast<std::uint8_t>(katana::entity::typeOf(item.geometry)), item.kind)
            << item.name << ": EntityType and the variant have drifted apart";
        EXPECT_EQ(katana::entity::toString(katana::entity::typeOf(item.geometry)), item.name);

        const auto blob = geometryToBlob(item.geometry);
        ASSERT_TRUE(blob.ok()) << item.name;
        ASSERT_GE(blob->size(), 2u);
        // The kinds version 1 had are written in it; the two added with the
        // annotation layout exist only in version 2 (geometry_blob.hpp).
        EXPECT_EQ(static_cast<std::uint8_t>((*blob)[0]),
                  item.kind <= 6 ? katana::entity::kBlobVersion
                                 : katana::entity::kBlobVersionAnnotation)
            << item.name;
        EXPECT_EQ(static_cast<std::uint8_t>((*blob)[1]), item.kind)
            << item.name << ": the on-disk kind byte changed, which reinterprets every "
                            "project already saved";
    }
}

TEST(GeometryBlobWireFormat, ACurvePolylinesLayoutIsPinned)
{
    // Written by hand from geometry_blob.hpp's description, so the layout of
    // the drawing system's polyline cannot drift with writer and reader
    // agreeing: version 2, kind 9, u32 count, u8 closed, then x, y, bulge and
    // height a vertex, a NaN height for none.
    const auto d = [](double value) {
        const auto bits = std::bit_cast<std::uint64_t>(value);
        std::vector<std::byte> bytes;
        for (int shift = 0; shift < 64; shift += 8) {
            bytes.push_back(static_cast<std::byte>((bits >> shift) & 0xFFu));
        }
        return bytes;
    };
    std::vector<std::byte> stored = {std::byte{0x02}, std::byte{0x09}, std::byte{0x02},
                                     std::byte{0x00}, std::byte{0x00}, std::byte{0x00},
                                     std::byte{0x00}};
    for (const double value : {1.0, 2.0, 0.5, std::numeric_limits<double>::quiet_NaN(), 3.0,
                               4.0, 0.0, 7.25}) {
        const auto bytes = d(value);
        stored.insert(stored.end(), bytes.begin(), bytes.end());
    }
    // The NaN is the canonical quiet one, 0x7FF8000000000000.
    EXPECT_EQ(stored[7 + 3 * 8 + 7], std::byte{0x7F});
    EXPECT_EQ(stored[7 + 3 * 8 + 6], std::byte{0xF8});

    const auto decoded = geometryFromBlob(stored);
    ASSERT_TRUE(decoded.ok()) << decoded.error().describe();
    const auto* polyline = std::get_if<katana::geometry::CurvePolyline2>(&*decoded);
    ASSERT_NE(polyline, nullptr) << "kind 9 no longer means CurvePolyline";
    ASSERT_EQ(polyline->vertices.size(), 2u);
    EXPECT_FALSE(polyline->closed);
    EXPECT_EQ(polyline->vertices[0].position, Point2(1.0, 2.0));
    EXPECT_EQ(polyline->vertices[0].bulge, 0.5);
    EXPECT_FALSE(polyline->vertices[0].height.has_value());
    EXPECT_EQ(polyline->vertices[1].height, 7.25);

    const auto written = geometryToBlob(Geometry{*polyline});
    ASSERT_TRUE(written.ok());
    EXPECT_EQ(*written, stored) << "the on-disk layout changed";

    // A drawing kind in a version-1 blob is refused, not guessed at.
    std::vector<std::byte> old = stored;
    old[0] = std::byte{0x01};
    EXPECT_FALSE(geometryFromBlob(old).ok());
}

TEST(GeometryBlobWireFormat, StoredBytesFromAPreviousBuildStillDecode)
{
    // A blob captured by hand rather than produced by the current writer, so
    // this fails if the LAYOUT changes even when the writer and reader change
    // together and agree with each other.
    //
    // Circle at centre (1.5, 2.5) radius 4.0: version 1, kind 4, then three
    // little-endian IEEE-754 doubles.
    const std::vector<std::byte> stored = {
        std::byte{0x01}, std::byte{0x04},
        // 1.5
        std::byte{0x00}, std::byte{0x00}, std::byte{0x00}, std::byte{0x00},
        std::byte{0x00}, std::byte{0x00}, std::byte{0xF8}, std::byte{0x3F},
        // 2.5
        std::byte{0x00}, std::byte{0x00}, std::byte{0x00}, std::byte{0x00},
        std::byte{0x00}, std::byte{0x00}, std::byte{0x04}, std::byte{0x40},
        // 4.0
        std::byte{0x00}, std::byte{0x00}, std::byte{0x00}, std::byte{0x00},
        std::byte{0x00}, std::byte{0x00}, std::byte{0x10}, std::byte{0x40},
    };

    const auto decoded = geometryFromBlob(stored);
    ASSERT_TRUE(decoded.ok()) << decoded.error().describe();
    const auto* circle = std::get_if<Circle2>(&*decoded);
    ASSERT_NE(circle, nullptr) << "the kind byte no longer means Circle";
    EXPECT_DOUBLE_EQ(circle->center.x, 1.5);
    EXPECT_DOUBLE_EQ(circle->center.y, 2.5);
    EXPECT_DOUBLE_EQ(circle->radius, 4.0);

    // And the current writer still produces exactly those bytes.
    const auto written = geometryToBlob(Geometry{Circle2{Point2(1.5, 2.5), 4.0}});
    ASSERT_TRUE(written.ok());
    EXPECT_EQ(*written, stored) << "the on-disk layout changed";
}
