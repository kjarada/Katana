#include <gtest/gtest.h>

#include <string>

#include "katana/core/text_encoding.hpp"

using katana::core::ErrorCode;

namespace {

// ASCII as UTF-16: each character followed (LE) or preceded (BE) by a NUL.
std::string asUtf16(const std::string& ascii, bool littleEndian)
{
    std::string out;
    for (const char ch : ascii) {
        if (littleEndian) {
            out += ch;
            out += '\0';
        } else {
            out += '\0';
            out += ch;
        }
    }
    return out;
}

} // namespace

TEST(TextEncoding, AByteOrderMarkDecidesTheEncodingAndIsRemoved)
{
    const auto little = katana::core::decodeText(std::string("\xFF\xFE", 2) + asUtf16("ab", true));
    ASSERT_TRUE(little.ok());
    EXPECT_EQ(little->text, "ab");
    EXPECT_EQ(little->encoding, katana::core::TextEncoding::Utf16LittleEndian);
    EXPECT_FALSE(little->guessed);

    const auto big = katana::core::decodeText(std::string("\xFE\xFF", 2) + asUtf16("ab", false));
    ASSERT_TRUE(big.ok());
    EXPECT_EQ(big->text, "ab");
    EXPECT_EQ(big->encoding, katana::core::TextEncoding::Utf16BigEndian);

    const auto utf8 = katana::core::decodeText("\xEF\xBB\xBFmodel");
    ASSERT_TRUE(utf8.ok());
    EXPECT_EQ(utf8->text, "model");
    EXPECT_EQ(utf8->encoding, katana::core::TextEncoding::Utf8WithBom);
}

TEST(TextEncoding, Utf16WithoutAMarkIsRecognisedByWhereItsNulsFall)
{
    const std::string source = "model \"Survey Control\"\n";
    const auto little = katana::core::decodeText(asUtf16(source, true));
    ASSERT_TRUE(little.ok());
    EXPECT_EQ(little->text, source);
    EXPECT_EQ(little->encoding, katana::core::TextEncoding::Utf16LittleEndian);
    EXPECT_TRUE(little->guessed);

    const auto big = katana::core::decodeText(asUtf16(source, false));
    ASSERT_TRUE(big.ok());
    EXPECT_EQ(big->text, source);
    EXPECT_EQ(big->encoding, katana::core::TextEncoding::Utf16BigEndian);
}

TEST(TextEncoding, ACharacterOutsideTheBasicPlaneSurvivesItsSurrogatePair)
{
    // U+1F600 is D83D DE00 in UTF-16 and F0 9F 98 80 in UTF-8 (Unicode 6.1
    // code chart; the arithmetic is 0x10000 + (0x3D << 10) + 0x200).
    const std::string bytes("\xFF\xFE\x3D\xD8\x00\xDE", 6);
    const auto decoded = katana::core::decodeText(bytes);
    ASSERT_TRUE(decoded.ok());
    EXPECT_EQ(decoded->text, "\xF0\x9F\x98\x80");
}

TEST(TextEncoding, BytesThatAreNotUtf8AreReadAsWindows1252)
{
    // 0xB0 is the degree sign U+00B0 (UTF-8 C2 B0); 0x80 is the euro sign
    // U+20AC (UTF-8 E2 82 AC) - the byte where CP1252 and Latin-1 part ways.
    const auto decoded = katana::core::decodeText("bearing 45\xB0 cost \x80" "5");
    ASSERT_TRUE(decoded.ok());
    EXPECT_EQ(decoded->encoding, katana::core::TextEncoding::Windows1252);
    EXPECT_TRUE(decoded->guessed);
    EXPECT_EQ(decoded->text, "bearing 45\xC2\xB0 cost \xE2\x82\xAC" "5");
}

TEST(TextEncoding, PlainAsciiIsNotReportedAsAGuess)
{
    const auto decoded = katana::core::decodeText("string super { }");
    ASSERT_TRUE(decoded.ok());
    EXPECT_EQ(decoded->encoding, katana::core::TextEncoding::Utf8);
    EXPECT_FALSE(decoded->guessed);
}

TEST(TextEncoding, Utf8WithoutAMarkIsKeptAndIsNotCalledAGuess)
{
    // UTF-8 is self-checking: the chance of Windows-1252 prose validating as
    // UTF-8 falls off geometrically with every accented character, which is why
    // "valid UTF-8 means UTF-8" is the rule detectors use (RFC 3629 section 8
    // makes the same observation). Flagging it would put a warning on every
    // hand-written file with a dash in a comment.
    const auto decoded = katana::core::decodeText("name \"caf\xC3\xA9\"");
    ASSERT_TRUE(decoded.ok());
    EXPECT_EQ(decoded->encoding, katana::core::TextEncoding::Utf8);
    EXPECT_FALSE(decoded->guessed);
    EXPECT_EQ(decoded->text, "name \"caf\xC3\xA9\"");
}

TEST(TextEncoding, AnOddNumberOfUtf16BytesIsATruncatedFile)
{
    const auto decoded = katana::core::decodeText(std::string("\xFF\xFE" "a\0b", 5));
    ASSERT_FALSE(decoded.ok());
    EXPECT_EQ(decoded.error().code, ErrorCode::ParseFailure);
}

TEST(TextEncoding, ASurrogateWithoutItsPartnerIsRefused)
{
    // A high surrogate followed by an ordinary character, and a low one alone.
    EXPECT_FALSE(katana::core::decodeText(std::string("\xFF\xFE\x3D\xD8" "a\0", 6)).ok());
    EXPECT_FALSE(katana::core::decodeText(std::string("\xFF\xFE\x00\xDE", 4)).ok());
    // ... and one cut off by the end of the file.
    EXPECT_FALSE(katana::core::decodeText(std::string("\xFF\xFE\x3D\xD8", 4)).ok());
}

TEST(TextEncoding, TheEmptyFileIsEmptyText)
{
    const auto decoded = katana::core::decodeText("");
    ASSERT_TRUE(decoded.ok());
    EXPECT_TRUE(decoded->text.empty());
}

TEST(TextEncoding, EncodingToUtf16AndDecodingAgainReturnsTheText)
{
    const std::string text = "name \"caf\xC3\xA9 \xF0\x9F\x98\x80\"\n";
    const auto encoded = katana::core::encodeUtf16LittleEndian(text);
    ASSERT_TRUE(encoded.ok());
    // The mark 12d Model writes, then two bytes per basic-plane character.
    EXPECT_EQ(encoded->substr(0, 2), std::string("\xFF\xFE", 2));
    const auto decoded = katana::core::decodeText(*encoded);
    ASSERT_TRUE(decoded.ok());
    EXPECT_EQ(decoded->text, text);
}

TEST(TextEncoding, EncodingRefusesBytesThatAreNotUtf8)
{
    const auto encoded = katana::core::encodeUtf16LittleEndian("bad \xFF byte");
    ASSERT_FALSE(encoded.ok());
    EXPECT_EQ(encoded.error().code, ErrorCode::InvalidArgument);
}

TEST(TextEncoding, AnEncodingStatedByAPersonIsUsedWithoutDetection)
{
    // "45°" in Windows-1252 is 34 35 B0. Detection would call it Windows-1252
    // too, but only after finding it is not UTF-8; stated, it is never asked.
    const auto latin = katana::core::decodeTextAs("45\xB0", katana::core::TextEncoding::Windows1252);
    ASSERT_TRUE(latin.ok());
    EXPECT_EQ(latin->text, "45\xC2\xB0");
    EXPECT_FALSE(latin->guessed);

    // Unmarked UTF-16LE of "ab" is 61 00 62 00; stated as big-endian the same
    // bytes are U+6100 U+6200, which is what a person who said so gets.
    const std::string unmarked("a\0b\0", 4);
    const auto little =
        katana::core::decodeTextAs(unmarked, katana::core::TextEncoding::Utf16LittleEndian);
    ASSERT_TRUE(little.ok());
    EXPECT_EQ(little->text, "ab");
    const auto big = katana::core::decodeTextAs(unmarked, katana::core::TextEncoding::Utf16BigEndian);
    ASSERT_TRUE(big.ok());
    EXPECT_EQ(big->text, "\xE6\x84\x80\xE6\x88\x80"); // U+6100 U+6200 in UTF-8
}

TEST(TextEncoding, AStatedEncodingStepsOverAMarkThatAgreesAndRefusesOneThatDoesNot)
{
    const auto agreed = katana::core::decodeTextAs(std::string("\xFF\xFE" "a\0", 4),
                                                   katana::core::TextEncoding::Utf16LittleEndian);
    ASSERT_TRUE(agreed.ok());
    EXPECT_EQ(agreed->text, "a");

    const auto utf8 = katana::core::decodeTextAs("\xEF\xBB\xBFx", katana::core::TextEncoding::Utf8);
    ASSERT_TRUE(utf8.ok());
    EXPECT_EQ(utf8->text, "x");
    EXPECT_EQ(utf8->encoding, katana::core::TextEncoding::Utf8WithBom);

    const auto contradicted = katana::core::decodeTextAs(
        std::string("\xFF\xFE" "a\0", 4), katana::core::TextEncoding::Utf16BigEndian);
    ASSERT_FALSE(contradicted.ok());
    EXPECT_EQ(contradicted.error().code, ErrorCode::ParseFailure);
    EXPECT_FALSE(katana::core::decodeTextAs("\xEF\xBB\xBFx", katana::core::TextEncoding::Windows1252).ok());
    EXPECT_FALSE(katana::core::decodeTextAs(std::string("\xFE\xFF\0a", 4), katana::core::TextEncoding::Utf8).ok());
}

TEST(TextEncoding, AStatedUtf8FileThatIsNotUtf8IsRefusedRatherThanRepaired)
{
    const auto decoded = katana::core::decodeTextAs("bearing 45\xB0", katana::core::TextEncoding::Utf8);
    ASSERT_FALSE(decoded.ok());
    EXPECT_EQ(decoded.error().code, ErrorCode::ParseFailure);
}

TEST(TextEncoding, Utf8ValidityFollowsTheUnicodeTableOfWellFormedSequences)
{
    // Unicode 15 table 3-7, one case from each row plus its nearest ill-formed
    // neighbour.
    EXPECT_TRUE(katana::core::isValidUtf8(""));
    EXPECT_TRUE(katana::core::isValidUtf8("\x7F"));
    EXPECT_TRUE(katana::core::isValidUtf8("\xC2\x80"));
    EXPECT_FALSE(katana::core::isValidUtf8("\xC1\xBF")); // overlong
    EXPECT_TRUE(katana::core::isValidUtf8("\xE0\xA0\x80"));
    EXPECT_FALSE(katana::core::isValidUtf8("\xE0\x9F\xBF")); // overlong
    EXPECT_TRUE(katana::core::isValidUtf8("\xED\x9F\xBF"));
    EXPECT_FALSE(katana::core::isValidUtf8("\xED\xA0\x80")); // surrogate half
    EXPECT_TRUE(katana::core::isValidUtf8("\xF0\x90\x80\x80"));
    EXPECT_FALSE(katana::core::isValidUtf8("\xF0\x8F\xBF\xBF")); // overlong
    EXPECT_TRUE(katana::core::isValidUtf8("\xF4\x8F\xBF\xBF"));  // U+10FFFF
    EXPECT_FALSE(katana::core::isValidUtf8("\xF4\x90\x80\x80")); // above U+10FFFF
    EXPECT_FALSE(katana::core::isValidUtf8("\xE2\x82"));         // truncated
    EXPECT_FALSE(katana::core::isValidUtf8("\x80"));             // lone continuation
}
