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
using katana::core::uppered;

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
        // U+0905, E0 A4 85: Latin-1's toupper makes the lead byte 0xC0, and
        // the name is no longer UTF-8.
        EXPECT_EQ(uppered("\xE0\xA4\x85 voil\xC3\xA0"), "\xE0\xA4\x85 VOIL\xC3\xA0") << name;
    }
    std::setlocale(LC_CTYPE, saved.c_str());
}

TEST(CoreText, CaseFoldingTouchesOnlyAsciiLetters)
{
    EXPECT_EQ(lowered("Survey CODE 12-b"), "survey code 12-b");
    EXPECT_EQ(uppered("Survey code 12-b"), "SURVEY CODE 12-B");
    EXPECT_EQ(uppered(""), "");
    // The bytes either side of 'a' to 'z' are not letters.
    EXPECT_EQ(uppered("`{@["), "`{@[");
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

TEST(CoreText, FixedTextOfOrdinaryValuesIsTheThreeDecimalsARecordAlwaysHad)
{
    using katana::core::formatFixed;
    EXPECT_EQ(formatFixed(0.0, 3), "0.000");
    EXPECT_EQ(formatFixed(12.0, 3), "12.000");
    EXPECT_EQ(formatFixed(1.25, 3), "1.250");
    EXPECT_EQ(formatFixed(-2.25, 3), "-2.250");
    EXPECT_EQ(formatFixed(0.017, 3), "0.017");
    EXPECT_EQ(formatFixed(1471.24, 3), "1471.240");
    EXPECT_EQ(formatFixed(123456.7895, 3), "123456.789");
    // The exact binary value decides, not the decimal text: the double nearest
    // 0.0005 is 0.00050000000000000001040..., above the tie, so it rounds up
    // (and the sign stays, since -0.001 is not nothing); 0.0015 is
    // 0.0015000000000000000312... and 2.0005 is 2.000500000000000166977... Each
    // expansion is Python's Decimal(value), which shares no code with this.
    EXPECT_EQ(formatFixed(0.0005, 3), "0.001");
    EXPECT_EQ(formatFixed(-0.0005, 3), "-0.001");
    EXPECT_EQ(formatFixed(0.0015, 3), "0.002");
    EXPECT_EQ(formatFixed(2.0005, 3), "2.001");
    EXPECT_EQ(formatFixed(1.25, 0), "1");
    EXPECT_EQ(formatFixed(1.5, 2), "1.50");
}

TEST(CoreText, AValueThatRoundsToNothingHasNoSign)
{
    using katana::core::formatFixed;
    EXPECT_EQ(formatFixed(-0.0, 3), "0.000");
    EXPECT_EQ(formatFixed(-0.0004, 3), "0.000");
    EXPECT_EQ(formatFixed(-1e-300, 3), "0.000");
    EXPECT_EQ(formatFixed(-5e-324, 3), "0.000");
    EXPECT_EQ(formatFixed(-0.4, 0), "0");
    // One that does not, keeps its sign.
    EXPECT_EQ(formatFixed(-0.0006, 3), "-0.001");
    EXPECT_EQ(formatFixed(-0.6, 0), "-1");
}

TEST(CoreText, FixedTextOfAFiniteValueNeverFailsHoweverLargeItIs)
{
    using katana::core::formatFixed;
    // 1e60 is the double 999999999999999949387135297074018866963645011013410073083904:
    // sixty digits and three decimals fill the 64 characters the formatter used
    // to have, to the last; 1e61 is the double 9999999999999999493871352970740188669636450110134100730839040,
    // a character more, and was printed "nan" (Python's '%.3f' % value for both).
    EXPECT_EQ(formatFixed(1e60, 3),
              "999999999999999949387135297074018866963645011013410073083904.000");
    EXPECT_EQ(formatFixed(1e61, 3),
              "9999999999999999493871352970740188669636450110134100730839040.000");
    // -1e300: a sign, 301 digits, the point and three decimals.
    const std::string minus = formatFixed(-1e300, 3);
    ASSERT_EQ(minus.size(), 306u);
    EXPECT_EQ(minus.front(), '-');
    EXPECT_EQ(minus.substr(minus.size() - 4), ".000");
    const auto minusBack = parseFiniteDouble(minus.substr(0, minus.size() - 4));
    ASSERT_TRUE(minusBack.has_value());
    EXPECT_EQ(*minusBack, -1e300) << "the digits are the double's, exactly";
    // The largest double: 309 digits, 17976931348623157081... to ...858368.
    const std::string most = formatFixed(std::numeric_limits<double>::max(), 3);
    ASSERT_EQ(most.size(), 313u);
    EXPECT_EQ(most.substr(0, 20), "17976931348623157081");
    EXPECT_EQ(most.substr(most.size() - 10), "858368.000");
    const auto mostBack = parseFiniteDouble(most.substr(0, most.size() - 4));
    ASSERT_TRUE(mostBack.has_value());
    EXPECT_EQ(*mostBack, std::numeric_limits<double>::max());
    // The decimals asked for are bounded, so the text is too.
    EXPECT_EQ(formatFixed(1.5, 100).size(), 62u);
    EXPECT_EQ(formatFixed(1.25, -2), "1");
}

TEST(CoreText, FixedTextOfANonFiniteValueIsWhatToCharsWrites)
{
    using katana::core::formatFixed;
    EXPECT_EQ(formatFixed(std::numeric_limits<double>::quiet_NaN(), 3), "nan");
    EXPECT_EQ(formatFixed(std::numeric_limits<double>::infinity(), 3), "inf");
    EXPECT_EQ(formatFixed(-std::numeric_limits<double>::infinity(), 3), "-inf");
}

// ---- key=value replies ------------------------------------------------------------

// Whatever a text holds - blanks, '=', quotes, backslashes, a line break -
// replyQuoted writes it on one line and readReplyRecord gives it back.
TEST(ReplyRecord, AQuotedValueReadsBackAsTheTextItWas)
{
    const std::string awkward = "layer Survey/Kerb = \"a\" \\ b\nnext";
    const std::string line = "ifc object from=" + katana::core::replyQuoted(awkward) + " count=2";
    EXPECT_EQ(line.find('\n'), std::string::npos);
    const auto record = katana::core::readReplyRecord(line);
    ASSERT_TRUE(record);
    EXPECT_EQ(record->words, (std::vector<std::string>{"ifc", "object"}));
    ASSERT_EQ(record->fields.size(), 2u);
    EXPECT_EQ(record->value("from"), awkward);
    EXPECT_EQ(record->value("count"), "2");
    EXPECT_EQ(record->value("absent"), std::nullopt);
}

// The words before the fields, fields in order, an empty value kept as
// empty - absent is not empty.
TEST(ReplyRecord, WordsComeFirstAndAnEmptyValueIsKept)
{
    const auto record = katana::core::readReplyRecord("  line id=W1 system= why=\"\"  ");
    ASSERT_TRUE(record);
    EXPECT_EQ(record->words, (std::vector<std::string>{"line"}));
    EXPECT_EQ(record->value("id"), "W1");
    EXPECT_EQ(record->value("system"), "");
    EXPECT_EQ(record->value("why"), "");
}

// What is not a record is refused rather than half read.
TEST(ReplyRecord, ALineThatIsNotARecordIsRefused)
{
    EXPECT_FALSE(katana::core::readReplyRecord(""));
    EXPECT_FALSE(katana::core::readReplyRecord("   "));
    EXPECT_FALSE(katana::core::readReplyRecord("ifc why=\"never closed"));
    EXPECT_FALSE(katana::core::readReplyRecord("ifc count=1 stray"));
    EXPECT_FALSE(katana::core::readReplyRecord("ifc =1"));
}
