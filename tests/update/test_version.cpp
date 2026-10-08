// Version text and its order (include/katana/update/version.hpp, src/katana_update/version.cpp).
//
// Every expectation is worked out from the rule the header states and from the arithmetic
// of the numbers, never read off the parser: "0.10" is above "0.9" because 10 > 9; a
// component of nine digits is the largest that fits 32 bits because 999,999,999 is below
// 2^32 = 4,294,967,296 and a tenth digit could reach 9,999,999,999, above it.

#include <gtest/gtest.h>

#include <array>
#include <compare>
#include <cstdint>
#include <string>
#include <string_view>
#include <vector>

#include "katana/update/version.hpp"

using namespace katana;

namespace {

update::Version parsed(std::string_view text)
{
    const core::Result<update::Version> version = update::parseVersion(text);
    EXPECT_TRUE(version.ok()) << text << ": " << (version.ok() ? "" : version.error().describe());
    return version.ok() ? version.value() : update::Version{};
}

using Components = std::array<std::uint32_t, 4>;

} // namespace

// ---- what is accepted ---------------------------------------------------------------------

TEST(UpdateVersion, TwoComponentsAreMajorAndMinorAndTheRestAreZero)
{
    const update::Version version = parsed("0.3");
    EXPECT_EQ(version.components, (Components{0, 3, 0, 0}));
    EXPECT_EQ(version.written, 2);
}

TEST(UpdateVersion, ThreeAndFourComponentsKeepEachNumberInPlace)
{
    const update::Version three = parsed("1.2.3");
    EXPECT_EQ(three.components, (Components{1, 2, 3, 0}));
    EXPECT_EQ(three.written, 3);

    const update::Version four = parsed("1.2.3.4");
    EXPECT_EQ(four.components, (Components{1, 2, 3, 4}));
    EXPECT_EQ(four.written, 4);
}

TEST(UpdateVersion, ABuildNumberAloneDoesNotMakeATrailingZeroLookDifferent)
{
    // "1.2.0.0" has four components written, all of them 1, 2, 0, 0.
    const update::Version version = parsed("1.2.0.0");
    EXPECT_EQ(version.components, (Components{1, 2, 0, 0}));
    EXPECT_EQ(version.written, 4);
}

TEST(UpdateVersion, TheLargestComponentIsNineNinesBecauseThatStillFitsThirtyTwoBits)
{
    const update::Version version = parsed("999999999.999999999.999999999.999999999");
    EXPECT_EQ(version.components, (Components{999'999'999, 999'999'999, 999'999'999, 999'999'999}));
    // 999,999,999 < 4,294,967,295: the digit count, not arithmetic, is what guards the type.
    static_assert(999'999'999u < 0xFFFF'FFFFu);
}

TEST(UpdateVersion, LeadingZerosAreReadAsDecimalNotOctal)
{
    // "1.02" is 1.2 and "010" is ten; a reader that took a leading 0 for octal would say 8.
    EXPECT_EQ(parsed("1.02").components, (Components{1, 2, 0, 0}));
    EXPECT_EQ(parsed("010.009").components, (Components{10, 9, 0, 0}));
    EXPECT_EQ(parsed("0.0").components, (Components{0, 0, 0, 0}));
    EXPECT_EQ(parsed("000000001.000000000").components, (Components{1, 0, 0, 0}));
}

// ---- the order ----------------------------------------------------------------------------

TEST(UpdateVersion, ComponentsAreComparedAsNumbersSoZeroTenIsAboveZeroNine)
{
    // As text "0.10" < "0.9" ('1' < '9'); as numbers 10 > 9. The fixtures' two releases are
    // exactly this pair (0.9.0 and 0.10.0).
    EXPECT_GT(parsed("0.10"), parsed("0.9"));
    EXPECT_GT(parsed("0.10.0"), parsed("0.9.0"));
    EXPECT_GT(parsed("10.0"), parsed("9.99.99.99"));
    EXPECT_LT(parsed("2.0"), parsed("10.0"));
    EXPECT_GT(parsed("1.10.0"), parsed("1.9.9.9"));
    EXPECT_GT(parsed("0.0.10"), parsed("0.0.9"));
    EXPECT_GT(parsed("0.0.0.10"), parsed("0.0.0.9"));
}

TEST(UpdateVersion, AMissingTrailingComponentIsZeroSoTheShortSpellingIsTheSameVersion)
{
    EXPECT_EQ(parsed("0.3"), parsed("0.3.0"));
    EXPECT_EQ(parsed("0.3"), parsed("0.3.0.0"));
    EXPECT_EQ(parsed("1.2.3"), parsed("1.2.3.0"));
    EXPECT_EQ(parsed("1.2"), parsed("1.02"));
    EXPECT_EQ(parsed("1.2") <=> parsed("1.2.0"), std::strong_ordering::equal);
    // ... but a build number above zero is a later version.
    EXPECT_LT(parsed("0.3"), parsed("0.3.0.1"));
}

TEST(UpdateVersion, TheOrderIsStrictlyAscendingAlongAHandWrittenList)
{
    // Each is below the next by the first component that differs, read left to right.
    const std::vector<std::string> ascending{"0.1",     "0.9",     "0.9.1",   "0.10",
                                             "0.10.0.1", "0.11",   "1.0",     "1.0.0.1",
                                             "1.0.1",    "1.1",    "2.0",     "10.0",
                                             "10.0.1",   "999999999.0"};
    for (std::size_t lower = 0; lower < ascending.size(); ++lower) {
        for (std::size_t upper = lower + 1; upper < ascending.size(); ++upper) {
            EXPECT_LT(parsed(ascending[lower]), parsed(ascending[upper]))
                << ascending[lower] << " < " << ascending[upper];
            EXPECT_GT(parsed(ascending[upper]), parsed(ascending[lower]))
                << ascending[upper] << " > " << ascending[lower];
            EXPECT_NE(parsed(ascending[lower]), parsed(ascending[upper]));
        }
    }
}

TEST(UpdateVersion, EqualityIgnoresHowManyComponentsWereWritten)
{
    update::Version two = parsed("5.6");
    update::Version four = parsed("5.6.0.0");
    ASSERT_NE(two.written, four.written);
    EXPECT_TRUE(two == four);
    EXPECT_FALSE(two != four);
}

// ---- the text back ----------------------------------------------------------------------------

TEST(UpdateVersion, ToStringGivesBackAsManyComponentsAsWereWritten)
{
    EXPECT_EQ(update::toString(parsed("0.3")), "0.3");
    EXPECT_EQ(update::toString(parsed("0.3.0")), "0.3.0");
    EXPECT_EQ(update::toString(parsed("0.3.0.0")), "0.3.0.0");
    EXPECT_EQ(update::toString(parsed("10.20.30.40")), "10.20.30.40");
    EXPECT_EQ(update::toString(parsed("999999999.0")), "999999999.0");
}

TEST(UpdateVersion, ToStringWritesTheNumberNotTheLeadingZeros)
{
    // 1.02 is 1.2 (above), and the text that comes back says so.
    EXPECT_EQ(update::toString(parsed("1.02")), "1.2");
    EXPECT_EQ(update::toString(parsed("000.0010.7")), "0.10.7");
}

TEST(UpdateVersion, ToStringOfAHandBuiltVersionStaysWithinTwoToFourComponents)
{
    update::Version version;
    version.components = {1, 2, 3, 4};
    version.written = 0; // out of range on purpose: a value parseVersion never makes
    EXPECT_EQ(update::toString(version), "1.2");
    version.written = 1;
    EXPECT_EQ(update::toString(version), "1.2");
    version.written = 9;
    EXPECT_EQ(update::toString(version), "1.2.3.4");
    // The default-constructed Version is 0.0 and says so.
    EXPECT_EQ(update::toString(update::Version{}), "0.0");
}

TEST(UpdateVersion, WhatToStringWritesParsesBackToTheSameVersion)
{
    for (const std::string_view text : {"0.0", "0.3", "1.2.3", "1.2.3.4", "999999999.999999999"}) {
        const update::Version version = parsed(text);
        EXPECT_EQ(update::toString(version), text);
        EXPECT_EQ(parsed(update::toString(version)).written, version.written);
    }
}

// ---- what is refused ---------------------------------------------------------------------------

TEST(UpdateVersion, TextThatIsNotMajorMinorPatchBuildOfDigitsIsAParseFailure)
{
    const std::vector<std::string> refused{
        "",                         // nothing
        ".",                        // two empty components
        "1",                        // a single component
        "1.",                       // an empty last component
        ".1",                       // an empty first component
        "1..2",                     // an empty middle component
        "1.2.",                     // a trailing dot
        "1.2.3.4.",                 // a trailing dot after four
        "1.2.3.4.5",                // five components
        "1.2.3.4.5.6",              // six
        "a.b",                      // letters
        "v1.2",                     // a leading letter
        "V1.2",                     //
        "garbage1.2",               // leading garbage
        "1.2garbage",               // trailing garbage
        "1.2-beta",                 // a pre-release suffix: there is no such syntax
        "1.2.3-rc1",                //
        "1.2+build5",               // build metadata
        "1.2.3.x",                  // a non-digit in the last component
        "-1.2",                     // a sign
        "+1.2",                     //
        "1.-2",                     //
        "1.+2",                     //
        " 1.2",                     // white space, either end and inside
        "1.2 ",                     //
        "1. 2",                     //
        "1 .2",                     //
        "1.2\n",                    // a line end
        "1.2\r",                    //
        "\t1.2",                    //
        "1,2",                      // the wrong separator
        "1_2",                      //
        "1:2",                      //
        "1/2",                      //
        "0x1.2",                    // hexadecimal
        "1e3.2",                    // an exponent
        "1.2e1",                    //
        "1.5.",                     //
        "1.2.3.4a",                 //
        "1.1234567890",             // ten digits: 1,234,567,890 would fit, 9,999,999,999 would not
        "4294967296.0",             // 2^32 exactly: ten digits
        "4294967295.0",             // 2^32 - 1: still ten digits, refused by the same rule
        "99999999999999999999.1",    // twenty digits: overflows even 64 bits
        "1.0000000000",             // ten digits of zeros: the length is the rule
        "1.2.3.10000000000",        //
        "\xD9\xA1.\xD9\xA2",        // Arabic-Indic digits 1 and 2 (U+0661, U+0662), UTF-8
        "\xEF\xBC\x91.\xEF\xBC\x92", // fullwidth digits 1 and 2 (U+FF11, U+FF12), UTF-8
    };
    for (const std::string& text : refused) {
        const core::Result<update::Version> version = update::parseVersion(text);
        ASSERT_FALSE(version.ok()) << "accepted: " << text;
        EXPECT_EQ(version.error().code, core::ErrorCode::ParseFailure) << text;
        EXPECT_FALSE(version.error().message.empty()) << text;
    }
}

TEST(UpdateVersion, ANulByteAnywhereIsRefusedAndNeverEndsTheText)
{
    // A C string would end at the NUL and read "1.2": the text is a string_view of its
    // full length, and the NUL is a character that is neither a digit nor a dot.
    EXPECT_FALSE(update::parseVersion(std::string("1.2\0", 4)).ok());
    EXPECT_FALSE(update::parseVersion(std::string("1.2\0""3", 5)).ok());
    EXPECT_FALSE(update::parseVersion(std::string("\0""1.2", 4)).ok());
    EXPECT_FALSE(update::parseVersion(std::string("1.\0""2", 4)).ok());
    EXPECT_FALSE(update::parseVersion(std::string("\0", 1)).ok());
}

TEST(UpdateVersion, ARefusalQuotesTheTextBoundedAndOnOneLine)
{
    // The context is the text (version.hpp) for text of a sensible length.
    const core::Result<update::Version> shortText = update::parseVersion("1.2-beta");
    ASSERT_FALSE(shortText.ok());
    EXPECT_EQ(shortText.error().context, "1.2-beta");

    // A megabyte of digits is refused without being copied into the message.
    const core::Result<update::Version> huge = update::parseVersion(std::string(1u << 20, '7'));
    ASSERT_FALSE(huge.ok());
    EXPECT_LE(huge.error().context.size(), 64u);
    EXPECT_LE(huge.error().message.size(), 200u);

    // A NUL or a newline in the text does not reach the message as itself.
    const core::Result<update::Version> hostile = update::parseVersion(std::string("1.\0\n2", 5));
    ASSERT_FALSE(hostile.ok());
    EXPECT_EQ(hostile.error().context.find('\0'), std::string::npos);
    EXPECT_EQ(hostile.error().context.find('\n'), std::string::npos);
    EXPECT_EQ(hostile.error().describe().find('\n'), std::string::npos);
}

TEST(UpdateVersion, TheRefusalSaysWhichRuleItBrokeInWordsOfItsOwn)
{
    // Different faults give different sentences, so a person reading a log can tell them apart.
    const auto message = [](std::string_view text) {
        const core::Result<update::Version> version = update::parseVersion(text);
        return version.ok() ? std::string() : version.error().message;
    };
    EXPECT_NE(message(""), message("1"));
    EXPECT_NE(message("1"), message("1..2"));
    EXPECT_NE(message("1..2"), message("1.2.3.4.5"));
    EXPECT_NE(message("1.2.3.4.5"), message("1.2a"));
    EXPECT_NE(message("1.2a"), message("1.12345678901"));
}

// ---- the program's own version ---------------------------------------------------------------

TEST(UpdateVersion, TheProgramsOwnVersionParsesAndSpellsTheTextItWasBuiltWith)
{
    const core::Result<update::Version> own = update::programVersion();
    ASSERT_TRUE(own.ok()) << own.error().describe();
    EXPECT_EQ(own.value(), parsed(update::programVersionText()));
    // The project writes MAJOR.MINOR.PATCH without leading zeros (CMakeLists.txt), so the
    // text comes back exactly.
    EXPECT_EQ(update::toString(own.value()), update::programVersionText());
}
