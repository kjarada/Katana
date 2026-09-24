// The text kernels held to their scalar references: decodeText and
// isValidUtf8 at every SIMD level must give the same text, the same error at
// the same byte, and the same answer. The AVX2 path works in blocks of 32 units
// (decoding) or 32 and 64 bytes (validation), so the cases put the interesting
// unit before, on and after each block edge.

#include <gtest/gtest.h>

#include <algorithm>
#include <cstdint>
#include <initializer_list>
#include <iterator>
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
    // Text as a description in another language has it: runs of ASCII of
    // every length from 0 to 40 between accented letters, with the text's end
    // at every distance from the last one, so that the validator's final
    // window (the text's last 32 bytes, judged again) begins at every place
    // in a character.
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

namespace {

bool validAt(SimdLevel level, const std::string& text)
{
    return atSimdLevel(level, [&] { return isValidUtf8(text); });
}

// Text of `size` bytes of 'a' with `bytes` written from `at`.
std::string with(std::size_t size, std::size_t at, std::initializer_list<unsigned char> bytes)
{
    std::string text(size, 'a');
    for (const unsigned char byte : bytes) {
        text[at++] = static_cast<char>(byte);
    }
    return text;
}

} // namespace

TEST(SimdText, TheValidatorFindsEachKindOfErrorWorkedByHandAtABlockEdge)
{
    // The AVX2 validator judges each byte against the three before it, 32
    // bytes at a time, 64 a step; the last 32 bytes of the text are judged
    // again with the 32 before them. Each case puts one character across one
    // of those edges in a text of 100 bytes (steps [0,64) and [64,96), then
    // the window [68,100)) or at its very end. Worked from the UTF-8 rules:
    struct Case {
        const char* what;
        std::string text;
        bool valid;
    };
    const std::vector<Case> cases = {
        // U+1F600 is F0 9F 98 80: a four-byte lead, then three continuations.
        {"a four-byte character across the 64-byte step", with(100, 62, {0xF0, 0x9F, 0x98, 0x80}),
         true},
        // U+20AC is E2 82 AC; here across the start of the final window at 68.
        {"a three-byte character across the final window", with(100, 66, {0xE2, 0x82, 0xAC}), true},
        {"a two-byte character that ends the text", with(100, 98, {0xC3, 0xA9}), true},
        // F4 90 80 80 would be U+110000, above the last code point U+10FFFF.
        {"a code point above U+10FFFF", with(100, 62, {0xF4, 0x90, 0x80, 0x80}), false},
        // F5 can only begin a value above U+13FFFF.
        {"the lead F5", with(100, 63, {0xF5, 0x80, 0x80, 0x80}), false},
        // E0 9F BF would be U+07FF in three bytes; it fits in two.
        {"an overlong three-byte form", with(100, 31, {0xE0, 0x9F, 0xBF}), false},
        // F0 8F BF BF would be U+FFFF in four bytes; it fits in three.
        {"an overlong four-byte form", with(100, 30, {0xF0, 0x8F, 0xBF, 0xBF}), false},
        // C1 BF would be U+007F in two bytes.
        {"an overlong two-byte form", with(100, 31, {0xC1, 0xBF}), false},
        // ED A0 80 would be U+D800, a surrogate half.
        {"a surrogate half", with(100, 63, {0xED, 0xA0, 0x80}), false},
        {"a continuation with no lead", with(100, 64, {0x80}), false},
        {"a two-byte character with a second continuation", with(100, 31, {0xC3, 0xA9, 0xA9}),
         false},
        // E2 82 then 'a': the three-byte character is one byte short.
        {"a three-byte character cut short by ASCII", with(100, 62, {0xE2, 0x82}), false},
        // E2 at 63 is open at the end of the first step, and the whole second
        // step is ASCII, which the validator passes over without judging it.
        {"a lead open before a step of ASCII", with(128, 63, {0xE2}), false},
        {"a four-byte character with the text ending after three bytes",
         with(100, 97, {0xF0, 0x9F, 0x98}), false},
        // 64 bytes: one whole step and no final window; C3 is left open.
        {"a lead that ends a text of exactly one step", with(64, 63, {0xC3}), false},
        {"a two-byte character that ends a text of exactly one step", with(64, 62, {0xC3, 0xA9}),
         true},
    };
    for (const Case& c : cases) {
        EXPECT_EQ(validAt(SimdLevel::Scalar, c.text), c.valid) << c.what;
        if (katana::test::avx2Available()) {
            EXPECT_EQ(validAt(SimdLevel::Avx2, c.text), c.valid) << c.what;
        }
    }
}

TEST(SimdText, EveryPairOfBytesAtEveryBlockEdgeIsJudgedAsTheByteLoopJudgesIt)
{
    KATANA_REQUIRE_AVX2();
    // All 65536 pairs in a text of 100 bytes of 'a' (steps [0,64) and
    // [64,96), then the final window [68,100)): beginning the text, across
    // the 32-byte edge in the first step, across the step edge, across the
    // start of the final window, across the edge the final window covers
    // again, and ending the text.
    const std::size_t places[] = {0, 31, 63, 67, 95, 98};
    std::size_t differ = 0;
    std::size_t valid = 0;
    for (unsigned first = 0; first < 256; ++first) {
        for (unsigned second = 0; second < 256; ++second) {
            for (const std::size_t at : places) {
                const std::string text = with(100, at, {static_cast<unsigned char>(first),
                                                        static_cast<unsigned char>(second)});
                const bool scalar = validAt(SimdLevel::Scalar, text);
                valid += scalar ? 1 : 0;
                if (validAt(SimdLevel::Avx2, text) != scalar && ++differ <= 10) {
                    ADD_FAILURE() << "bytes " << first << " " << second << " at " << at;
                }
            }
        }
    }
    EXPECT_EQ(differ, 0u);
    // By hand: a pair is valid when both bytes are ASCII (128 x 128) or it is
    // one two-byte character (30 leads C2-DF x 64 continuations 80-BF):
    // 16384 + 1920 = 18304 pairs, at each of the 6 places.
    EXPECT_EQ(valid, 18304u * 6);
}

TEST(SimdText, EveryLeadWithEachKindOfFollowerIsJudgedAsTheByteLoopJudgesIt)
{
    KATANA_REQUIRE_AVX2();
    // Every lead byte C0-FF, and the continuations 80 and BF in its place, as
    // the first of four bytes across the step edge (and of three ending the
    // text), followed by bytes from every range the rules tell apart: ASCII;
    // the edges of 80-8F, 90-9F and A0-BF; leads of each length.
    const unsigned char followers[] = {0x41, 0x7F, 0x80, 0x8F, 0x90, 0x9F, 0xA0, 0xBF,
                                       0xC0, 0xC2, 0xDF, 0xE0, 0xED, 0xF0, 0xF4, 0xFF};
    std::vector<unsigned char> firsts = {0x80, 0xBF};
    for (unsigned lead = 0xC0; lead < 256; ++lead) {
        firsts.push_back(static_cast<unsigned char>(lead));
    }
    std::size_t differ = 0;
    std::size_t valid = 0;
    const auto compare = [&](const std::string& text) {
        const bool scalar = validAt(SimdLevel::Scalar, text);
        valid += scalar ? 1 : 0;
        if (validAt(SimdLevel::Avx2, text) != scalar && ++differ <= 10) {
            std::string bytes;
            for (const char ch : text) {
                if (ch != 'a') {
                    bytes += std::to_string(static_cast<unsigned char>(ch)) + " ";
                }
            }
            ADD_FAILURE() << "bytes " << bytes;
        }
    };
    for (const unsigned char first : firsts) {
        for (const unsigned char b : followers) {
            for (const unsigned char c : followers) {
                compare(with(100, 97, {first, b, c}));
                for (const unsigned char d : followers) {
                    compare(with(100, 62, {first, b, c, d}));
                }
            }
        }
    }
    EXPECT_EQ(differ, 0u);
    // Some of these are characters, or the comparison would prove little:
    // C2 80 then ASCII, for one, is valid.
    EXPECT_GT(valid, 0u);
}

TEST(SimdText, GeneratedTextWithDamageIsJudgedAsTheByteLoopJudgesIt)
{
    KATANA_REQUIRE_AVX2();
    // Characters of every length (the first and last of each, and U+10FFFF)
    // and runs of ASCII, 64 to 400 bytes, then damaged: a byte replaced,
    // removed or inserted, or the text cut.
    const char* const pieces[] = {"a",
                                  " ",
                                  "\r\n",
                                  "\xC2\x80",
                                  "\xC3\xA9",
                                  "\xDF\xBF",
                                  "\xE0\xA0\x80",
                                  "\xE2\x82\xAC",
                                  "\xED\x9F\xBF",
                                  "\xEE\x80\x80",
                                  "\xEF\xBF\xBF",
                                  "\xF0\x90\x80\x80",
                                  "\xF0\x9F\x98\x80",
                                  "\xF4\x8F\xBF\xBF"};
    std::uint32_t seed = 924u;
    const auto next = [&seed] {
        seed = seed * 1664525u + 1013904223u;
        return seed >> 8;
    };
    std::size_t compared = 0;
    std::size_t valid = 0;
    for (int round = 0; round < 6000; ++round) {
        std::string text;
        const std::size_t size = 64 + next() % 337;
        while (text.size() < size) {
            text += next() % 3 == 0 ? std::string(next() % 40, 'q')
                                    : std::string(pieces[next() % std::size(pieces)]);
        }
        const std::size_t damage = next() % 3;
        for (std::size_t k = 0; k < damage; ++k) {
            const std::size_t at = next() % text.size();
            switch (next() % 4) {
            case 0:
                text[at] = static_cast<char>(next());
                break;
            case 1:
                text.erase(at, 1);
                break;
            case 2:
                text.insert(at, 1, static_cast<char>(0x80 + next() % 0x80));
                break;
            default:
                text.resize(std::max<std::size_t>(at, 64));
                break;
            }
        }
        const bool scalar = validAt(SimdLevel::Scalar, text);
        valid += scalar ? 1 : 0;
        EXPECT_EQ(validAt(SimdLevel::Avx2, text), scalar) << "round " << round;
        ++compared;
    }
    EXPECT_EQ(compared, 6000u);
    // Both answers are well represented, or the comparison proves little.
    EXPECT_GT(valid, 1000u);
    EXPECT_LT(valid, 5000u);
}
