// The text kernels held to their scalar references: decodeText and
// isValidUtf8 at every SIMD level must give the same text, the same error at
// the same byte, and the same answer. The AVX2 path works in blocks of 32 units
// (decoding) or 32 and 64 bytes (validation), so the cases put the interesting
// unit before, on and after each block edge.

#include <gtest/gtest.h>

#include <cstdint>
#include <string>
#include <vector>

#include "katana/core/cpu_features.hpp"
#include "katana/core/text_encoding.hpp"
#include "simd_levels.hpp"

using katana::core::decodeText;
using katana::core::isValidUtf8;
using katana::core::SimdLevel;
using katana::test::atSimdLevel;

namespace {

// UTF-16 bytes of `units` (code UNITS, so a test can write a broken surrogate
// on purpose), with a byte order mark first when `mark`.
std::string utf16(const std::vector<std::uint16_t>& units, bool little, bool mark = true)
{
    std::string bytes;
    const auto put = [&](std::uint16_t unit) {
        const char high = static_cast<char>(unit >> 8);
        const char low = static_cast<char>(unit & 0xFF);
        bytes += little ? low : high;
        bytes += little ? high : low;
    };
    if (mark) {
        put(0xFEFF);
    }
    for (const std::uint16_t unit : units) {
        put(unit);
    }
    return bytes;
}

std::vector<std::uint16_t> ascii(std::size_t count, char ch)
{
    return std::vector<std::uint16_t>(count, static_cast<std::uint16_t>(ch));
}

std::vector<std::uint16_t> operator+(std::vector<std::uint16_t> a,
                                     const std::vector<std::uint16_t>& b)
{
    a.insert(a.end(), b.begin(), b.end());
    return a;
}

struct Outcome {
    bool ok = false;
    std::string text; // the decoded text, or the error described
    katana::core::TextEncoding encoding = katana::core::TextEncoding::Utf8;
    bool guessed = false;

    friend bool operator==(const Outcome&, const Outcome&) = default;
};

Outcome decode(const std::string& bytes)
{
    const auto decoded = decodeText(bytes);
    if (!decoded) {
        return {false, decoded.error().describe(), {}, false};
    }
    return {true, decoded->text, decoded->encoding, decoded->guessed};
}

Outcome decodeAt(SimdLevel level, const std::string& bytes)
{
    return atSimdLevel(level, [&] { return decode(bytes); });
}

} // namespace

TEST(SimdText, AnAsciiRunLongerThanABlockIsNarrowedByteForByteAtEveryLevel)
{
    // 70 'A', U+00E9, 5 'b'. U+00E9 in UTF-8: 0xE9 = 000 1110 1001 splits as
    // 00011 / 101001, giving 110 00011 = C3 and 10 101001 = A9.
    const std::string bytes =
        utf16(ascii(70, 'A') + std::vector<std::uint16_t>{0x00E9} + ascii(5, 'b'), true);
    const std::string expected = std::string(70, 'A') + "\xC3\xA9" + "bbbbb";
    for (const SimdLevel level : {SimdLevel::Scalar, SimdLevel::Avx2}) {
        if (level == SimdLevel::Avx2 && !katana::test::avx2Available()) {
            continue;
        }
        const Outcome outcome = decodeAt(level, bytes);
        ASSERT_TRUE(outcome.ok) << outcome.text;
        EXPECT_EQ(outcome.text, expected) << katana::core::toString(level);
    }
}

TEST(SimdText, BigEndianAsciiIsTakenFromTheSecondByteOfEachUnit)
{
    KATANA_REQUIRE_AVX2();
    // FE FF, then 33 'x' as 00 78, U+00E9 as 00 E9, then 40 'y': the block of
    // units 32-63 holds the non-ASCII unit at its second place.
    const std::string bytes =
        utf16(ascii(33, 'x') + std::vector<std::uint16_t>{0x00E9} + ascii(40, 'y'), false);
    const std::string expected = std::string(33, 'x') + "\xC3\xA9" + std::string(40, 'y');
    const Outcome avx2 = decodeAt(SimdLevel::Avx2, bytes);
    ASSERT_TRUE(avx2.ok) << avx2.text;
    EXPECT_EQ(avx2.text, expected);
    EXPECT_EQ(avx2.encoding, katana::core::TextEncoding::Utf16BigEndian);
    EXPECT_EQ(avx2, decodeAt(SimdLevel::Scalar, bytes));
}

TEST(SimdText, AUnitIsAsciiOnlyWhenBelow0x80InBothByteOrders)
{
    KATANA_REQUIRE_AVX2();
    // Each unit alone at place 35 of a 64-unit run. Worked by hand:
    //   U+007F (DEL) is ASCII and stays one byte, 7F.
    //   U+0080 -> 00010 / 000000 -> C2 80: only the top bit of the low byte set.
    //   U+0100 -> 00100 / 000000 -> C4 80: only the high byte set.
    //   U+7F00 -> 0111 / 111100 / 000000 -> E7 BC 80: the high byte 7F, which
    //   is an ASCII byte value in the wrong half of the unit.
    struct Case {
        std::uint16_t unit;
        std::string utf8;
    };
    for (const Case& c : {Case{0x007F, "\x7F"}, Case{0x0080, "\xC2\x80"}, Case{0x0100, "\xC4\x80"},
                          Case{0x7F00, "\xE7\xBC\x80"}}) {
        for (const bool little : {true, false}) {
            const std::string bytes =
                utf16(ascii(35, 'q') + std::vector<std::uint16_t>{c.unit} + ascii(28, 'r'), little);
            const Outcome avx2 = decodeAt(SimdLevel::Avx2, bytes);
            ASSERT_TRUE(avx2.ok) << avx2.text;
            EXPECT_EQ(avx2.text, std::string(35, 'q') + c.utf8 + std::string(28, 'r'))
                << std::hex << c.unit << (little ? " LE" : " BE");
            EXPECT_EQ(avx2, decodeAt(SimdLevel::Scalar, bytes));
        }
    }
}

TEST(SimdText, AMalformedSurrogateIsReportedAtTheSameByteAtEveryLevel)
{
    KATANA_REQUIRE_AVX2();
    // A lone low surrogate after 37 ASCII units: unit 37 of the text after the
    // mark begins at byte 2 x 37 = 74 of it.
    const std::string lowAlone =
        utf16(ascii(37, 'a') + std::vector<std::uint16_t>{0xDC00} + ascii(30, 'a'), true);
    const Outcome lowScalar = decodeAt(SimdLevel::Scalar, lowAlone);
    ASSERT_FALSE(lowScalar.ok);
    EXPECT_NE(lowScalar.text.find("low surrogate with no high"), std::string::npos)
        << lowScalar.text;
    EXPECT_NE(lowScalar.text.find("[byte 74]"), std::string::npos) << lowScalar.text;
    EXPECT_EQ(decodeAt(SimdLevel::Avx2, lowAlone), lowScalar);

    // A high surrogate followed by 'a' at unit 40: byte 80.
    const std::string highAlone =
        utf16(ascii(40, 'a') + std::vector<std::uint16_t>{0xD800, 'a'} + ascii(30, 'a'), true);
    const Outcome highScalar = decodeAt(SimdLevel::Scalar, highAlone);
    ASSERT_FALSE(highScalar.ok);
    EXPECT_NE(highScalar.text.find("[byte 80]"), std::string::npos) << highScalar.text;
    EXPECT_EQ(decodeAt(SimdLevel::Avx2, highAlone), highScalar);

    // A high surrogate as the very last unit, after two whole blocks.
    const std::string truncated = utf16(ascii(64, 'a') + std::vector<std::uint16_t>{0xD83D}, true);
    const Outcome truncatedScalar = decodeAt(SimdLevel::Scalar, truncated);
    ASSERT_FALSE(truncatedScalar.ok);
    EXPECT_NE(truncatedScalar.text.find("ends in the middle"), std::string::npos)
        << truncatedScalar.text;
    EXPECT_EQ(decodeAt(SimdLevel::Avx2, truncated), truncatedScalar);
}

TEST(SimdText, DecodingGivesTheSameTextOrTheSameErrorAtEveryLevel)
{
    KATANA_REQUIRE_AVX2();
    // Generated: runs of ASCII of every length up to three blocks, broken by
    // every kind of unit - two- and three-byte characters, whole surrogate
    // pairs, broken ones - in both byte orders, marked and not.
    const std::vector<std::uint16_t> breakers = {0x00B0, 0x00E9, 0x0080, 0x07FF, 0x0800, 0x20AC,
                                                 0xFFFD, 0xD83D, 0xDE00, 0x7F41};
    std::uint32_t seed = 2026u;
    const auto next = [&seed] {
        seed = seed * 1664525u + 1013904223u;
        return seed >> 8;
    };
    std::size_t compared = 0;
    for (int round = 0; round < 3000; ++round) {
        std::vector<std::uint16_t> units;
        const std::size_t pieces = 1 + next() % 6;
        for (std::size_t piece = 0; piece < pieces; ++piece) {
            const std::size_t run = next() % 100;
            for (std::size_t k = 0; k < run; ++k) {
                units.push_back(static_cast<std::uint16_t>(0x20 + next() % 0x5F));
            }
            const std::uint16_t breaker = breakers[next() % breakers.size()];
            units.push_back(breaker);
            if (breaker == 0xD83D && next() % 4 != 0) {
                units.push_back(0xDE00); // mostly a whole pair, sometimes not
            }
        }
        const bool little = next() % 2 == 0;
        // Unmarked text needs its NULs to be told from UTF-8; long enough runs
        // provide them.
        const bool mark = next() % 3 != 0;
        const std::string bytes = utf16(units, little, mark);
        EXPECT_EQ(decodeAt(SimdLevel::Avx2, bytes), decodeAt(SimdLevel::Scalar, bytes))
            << "round " << round;
        ++compared;
    }
    EXPECT_EQ(compared, 3000u);
}

TEST(SimdText, Utf8ValidationGivesTheSameAnswerAtEveryLevel)
{
    KATANA_REQUIRE_AVX2();
    struct Case {
        std::string text;
        bool valid;
    };
    // Worked by hand: ASCII is valid; C3 A9 is a whole two-byte character; a
    // lone 80 is a continuation byte with no lead; a trailing C3 is a lead
    // with no continuation; ED A0 80 would encode U+D800, a surrogate half.
    const std::vector<Case> cases = {
        {std::string(100, 'a'), true},
        {std::string(100, 'a') + "\xC3\xA9" + std::string(40, 'b'), true},
        {std::string(70, 'a') + "\x80" + std::string(10, 'a'), false},
        {std::string(64, 'a') + "\xC3", false},
        {std::string(33, 'a') + "\xED\xA0\x80" + std::string(40, 'a'), false},
        {std::string(31, 'a') + "\xFF" + std::string(40, 'a'), false},
        {std::string(127, 'a') + "\xE2\x82\xAC", true},
    };
    for (const Case& c : cases) {
        const bool scalar = atSimdLevel(SimdLevel::Scalar, [&] { return isValidUtf8(c.text); });
        const bool avx2 = atSimdLevel(SimdLevel::Avx2, [&] { return isValidUtf8(c.text); });
        EXPECT_EQ(scalar, c.valid) << c.text.size();
        EXPECT_EQ(avx2, c.valid) << c.text.size();
    }

    // Every position of one bad byte, and of one good character, across the
    // first three 64-byte steps and their 32-byte halves.
    for (std::size_t at = 0; at < 200; ++at) {
        for (const std::string& insert : {std::string("\x80"), std::string("\xC3\xA9")}) {
            std::string text(200, 'z');
            text.insert(at, insert);
            const bool scalar = atSimdLevel(SimdLevel::Scalar, [&] { return isValidUtf8(text); });
            const bool avx2 = atSimdLevel(SimdLevel::Avx2, [&] { return isValidUtf8(text); });
            EXPECT_EQ(scalar, insert.size() == 2) << at;
            EXPECT_EQ(avx2, scalar) << at;
        }
    }
}

TEST(SimdText, TextDenseWithMultibyteCharactersValidatesAlikeAtEveryLevel)
{
    // isValidUtf8 reads an ASCII run byte by byte for its first 16 bytes and
    // hands the rest to the kernel only when 32 more remain, so the runs here
    // are every length from 0 to 40 - short ones between accented letters, as
    // in a description in another language, and ones that cross the hand-over
    // - with the text's end at every distance from it.
    //
    // Worked by hand: "é" is C3 A9, a whole two-byte character, so any mix of
    // it and ASCII is valid; a lone 80 is a continuation with no lead; a C3 at
    // the very end is a lead with nothing after it.
    for (std::size_t run = 0; run <= 40; ++run) {
        std::string text;
        while (text.size() < 160) {
            text += "\xC3\xA9";
            text += std::string(run, 'a');
        }
        for (std::size_t cut = text.size() - 40; cut <= text.size(); ++cut) {
            const std::string prefix = text.substr(0, cut);
            // A cut through the middle of "é" leaves a lead byte with no
            // continuation. The two broken texts are broken either way: after
            // "a" an 80 has no lead (and a C3 cut from its A9 has an "a" where
            // its continuation should be); a text that ends in C3 is cut short.
            const bool whole = !(cut > 0 && static_cast<unsigned char>(prefix.back()) == 0xC3);
            const std::string loneContinuation = prefix + "a\x80" + std::string(run, 'b');
            const std::string truncated = prefix + std::string(run, 'b') + "\xC3";
            for (const SimdLevel level : {SimdLevel::Scalar, SimdLevel::Avx2}) {
                if (level == SimdLevel::Avx2 && !katana::test::avx2Available()) {
                    continue;
                }
                EXPECT_EQ(atSimdLevel(level, [&] { return isValidUtf8(prefix); }), whole)
                    << "run " << run << " cut " << cut << " at " << katana::core::toString(level);
                EXPECT_FALSE(atSimdLevel(level, [&] { return isValidUtf8(loneContinuation); }))
                    << "run " << run << " cut " << cut << " at " << katana::core::toString(level);
                EXPECT_FALSE(atSimdLevel(level, [&] { return isValidUtf8(truncated); }))
                    << "run " << run << " cut " << cut << " at " << katana::core::toString(level);
            }
        }
    }
}
