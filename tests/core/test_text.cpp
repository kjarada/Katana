#include <gtest/gtest.h>

#include <clocale>
#include <cstdint>
#include <limits>
#include <string>
#include <string_view>
#include <vector>

#include "katana/core/text.hpp"

using katana::core::equalsIgnoringCase;
using katana::core::formatExactReal;
using katana::core::isAsciiSpace;
using katana::core::lowered;
using katana::core::parseFiniteDouble;
using katana::core::parseInteger;
using katana::core::splitLines;
using katana::core::trimmed;

TEST(CoreText, TrimRemovesExactlyTheCLocaleBlanksFromBothEnds)
{
    EXPECT_EQ(trimmed(" \t\r\n\v\fword \t\r\n\v\f"), "word");
    EXPECT_EQ(trimmed("  two words  "), "two words");
    EXPECT_EQ(trimmed(""), "");
    EXPECT_EQ(trimmed(" \t "), "");
    // A NUL and a non-breaking space are not blanks: C's isspace in the "C"
    // locale accepts exactly the six above (C17 7.4.1.10).
    EXPECT_EQ(trimmed(std::string_view("\0a\0", 3)), std::string_view("\0a\0", 3));
    EXPECT_EQ(trimmed("\xC2\xA0x\xC2\xA0"), "\xC2\xA0x\xC2\xA0");
}

TEST(CoreText, TrimNeverCutsAUtf8SequenceWhateverTheLocale)
{
    // "voilà" ends in C3 A0; 0xA0 is a space in Latin-1 locales. A locale
    // that makes it one is set if the machine has it, and trimming must not
    // notice either way.
    const char* previous = std::setlocale(LC_CTYPE, nullptr);
    const std::string saved = previous != nullptr ? previous : "C";
    for (const char* name : {"en_US.ISO-8859-1", "de_DE.ISO8859-1", ".1252", "C"}) {
        if (std::setlocale(LC_CTYPE, name) == nullptr) {
            continue;
        }
        EXPECT_EQ(trimmed("voil\xC3\xA0"), "voil\xC3\xA0") << name;
        EXPECT_EQ(lowered("VOIL\xC3\x80"), "voil\xC3\x80") << name;
    }
    std::setlocale(LC_CTYPE, saved.c_str());
}

TEST(CoreText, CaseFoldingTouchesOnlyAsciiLetters)
{
    EXPECT_EQ(lowered("Survey CODE 12-b"), "survey code 12-b");
    EXPECT_TRUE(equalsIgnoringCase("GSI16", "gsi16"));
    EXPECT_FALSE(equalsIgnoringCase("GSI16", "GSI8"));
    EXPECT_FALSE(equalsIgnoringCase("ab", "abc"));
    // 'A' + 32 is 'a'; '@' + 32 is '`' and must not be treated as a letter.
    EXPECT_FALSE(equalsIgnoringCase("@", "`"));
    EXPECT_TRUE(isAsciiSpace('\v'));
    EXPECT_FALSE(isAsciiSpace('\0'));
}

TEST(CoreText, AnIntegerMustBeTheWholeTokenWithAtMostOneSign)
{
    EXPECT_EQ(parseInteger("42"), 42);
    EXPECT_EQ(parseInteger("+42"), 42);
    EXPECT_EQ(parseInteger("-42"), -42);
    EXPECT_EQ(parseInteger("0007"), 7);
    EXPECT_EQ(parseInteger("9223372036854775807"), std::numeric_limits<std::int64_t>::max());
    for (std::string_view bad :
         {"", "+", "-", "+-1", "++1", "--1", "2.5", "1e3", "12a", " 1", "1 ", "9223372036854775808"}) {
        EXPECT_FALSE(parseInteger(bad).has_value()) << "'" << bad << "'";
    }
}

TEST(CoreText, ARealMustBeTheWholeTokenAndFinite)
{
    // Chosen to be exact in binary, so the comparison can be equality.
    EXPECT_EQ(parseFiniteDouble("0.5"), 0.5);
    EXPECT_EQ(parseFiniteDouble("+0.5"), 0.5);
    EXPECT_EQ(parseFiniteDouble("-1234.25"), -1234.25);
    EXPECT_EQ(parseFiniteDouble("1e3"), 1000.0);
    EXPECT_EQ(parseFiniteDouble("6.25E-2"), 0.0625);
    EXPECT_EQ(parseFiniteDouble("12"), 12.0);
    // A northing at the magnitude a survey has: from_chars is correctly rounded,
    // so this is the double nearest the decimal, the same one the literal is.
    EXPECT_EQ(parseFiniteDouble("7410850.123"), 7410850.123);
    for (std::string_view bad : {"", "+", "-", "+-1", "1,5", "0x10", "inf", "-inf", "nan",
                                 "infinity", "1e999", "1.5m", " 1", "."}) {
        EXPECT_FALSE(parseFiniteDouble(bad).has_value()) << "'" << bad << "'";
    }
}

TEST(CoreText, EveryKindOfLineEndingEndsALineAndInteriorBlankLinesAreKept)
{
    const std::vector<std::string_view> lines = splitLines("a\r\nb\rc\n\nd\n");
    const std::vector<std::string_view> expected = {"a", "b", "c", "", "d"};
    EXPECT_EQ(lines, expected);
    EXPECT_TRUE(splitLines("").empty());
    EXPECT_EQ(splitLines("only"), std::vector<std::string_view>{"only"});
    // "\n\r" is TWO endings - LF then CR - not one, so there is an empty line
    // between them; only CR LF is a pair.
    EXPECT_EQ(splitLines("a\n\rb"), (std::vector<std::string_view>{"a", "", "b"}));
    EXPECT_EQ(splitLines("\n"), std::vector<std::string_view>{""});
}

TEST(CoreText, ExactRealTextReadsBackAsTheSameDouble)
{
    EXPECT_EQ(formatExactReal(0.1), "0.1");
    EXPECT_EQ(formatExactReal(502000.125), "502000.125");
    EXPECT_EQ(formatExactReal(-3.0), "-3");
    for (const double value : {0.1, 1.0 / 3.0, 7410850.123, -255440.0625, 5e-324, 1.7976931348623157e308}) {
        const auto back = parseFiniteDouble(formatExactReal(value));
        ASSERT_TRUE(back.has_value()) << value;
        EXPECT_EQ(*back, value);
    }
}
