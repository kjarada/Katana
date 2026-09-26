#include <gtest/gtest.h>

#include <charconv>
#include <set>
#include <string>

#include "katana/ifc/step.hpp"

namespace ifc = katana::ifc;

// The expected GlobalIds are IfcOpenShell's (ifcopenshell.guid.compress of
// the same 128 bits), the independent implementation of the encoding the
// IFC documentation gives.
TEST(IfcGuid, CompressesAsTheIfcDocumentationAndIfcOpenShellDo)
{
    EXPECT_EQ(ifc::compressGuid(0, 0), "0000000000000000000000");
    EXPECT_EQ(ifc::compressGuid(0, 1), "0000000000000000000001");
    EXPECT_EQ(ifc::compressGuid(~0ULL, ~0ULL), "3$$$$$$$$$$$$$$$$$$$$$");
    EXPECT_EQ(ifc::compressGuid(0x0123456789ABCDEFULL, 0x0123456789ABCDEFULL),
              "018qLdYQlDxm4ZHMU9gytl");
}

TEST(IfcGuid, AKeyGivesTheSameGlobalIdEveryTimeAndDifferentKeysDifferentOnes)
{
    const std::string alphabet = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz_$";
    EXPECT_EQ(ifc::guidFor("project", "entity/41"), ifc::guidFor("project", "entity/41"));
    std::set<std::string> seen;
    for (int i = 0; i < 20000; ++i) {
        const std::string guid = ifc::guidFor("project", "entity/" + std::to_string(i));
        // PJS003: 22 characters of the alphabet, the first 0 to 3.
        ASSERT_EQ(guid.size(), 22u);
        ASSERT_NE(std::string("0123").find(guid.front()), std::string::npos) << guid;
        ASSERT_EQ(guid.find_first_not_of(alphabet), std::string::npos) << guid;
        ASSERT_TRUE(seen.insert(guid).second) << "collision at " << i;
    }
    // The space keeps two projects apart, and a boundary cannot move
    // between the space and the key.
    EXPECT_NE(ifc::guidFor("a", "bc"), ifc::guidFor("ab", "c"));
    EXPECT_NE(ifc::guidFor("one project", "entity/1"), ifc::guidFor("another", "entity/1"));
}

// ISO 10303-21:2016, 6.4.3: a quote is doubled, a backslash doubled, and a
// character outside the basic alphabet is \X2\ hex UTF-16 \X0\ within the
// Basic Multilingual Plane, \X4\ hex UTF-32 \X0\ beyond it.
TEST(IfcStepString, EncodesQuotesBackslashesAndUnicodeAsTheStandardSays)
{
    EXPECT_EQ(ifc::stepString("LOT 42"), "'LOT 42'");
    EXPECT_EQ(ifc::stepString("It's"), "'It''s'");
    EXPECT_EQ(ifc::stepString("a\\b"), "'a\\\\b'");
    EXPECT_EQ(ifc::stepString("45\xC2\xB0"), "'45\\X2\\00B0\\X0\\'");         // U+00B0
    EXPECT_EQ(ifc::stepString("\xC2\xB1\xC3\xA9"), "'\\X2\\00B100E9\\X0\\'"); // one run
    EXPECT_EQ(ifc::stepString("\xF0\x9F\x98\x80"), "'\\X4\\0001F600\\X0\\'"); // U+1F600
    EXPECT_EQ(ifc::stepString("a\nb"), "'a\\X2\\000A\\X0\\b'");
    // A byte that is not UTF-8 is the replacement character, not a guess.
    EXPECT_EQ(ifc::stepString("\xFF"), "'\\X2\\FFFD\\X0\\'");
}

TEST(IfcStepString, DecodingUndoesEncodingAndReadsTheOtherEscapesToo)
{
    for (const std::string text :
         {"LOT 42", "It's", "a\\b", "45\xC2\xB0", "\xF0\x9F\x98\x80 and \xC3\xA9", "a\nb", ""}) {
        const std::string encoded = ifc::stepString(text);
        EXPECT_EQ(ifc::decodeStepString(encoded.substr(1, encoded.size() - 2)), text) << encoded;
    }
    // \X\hh is an ISO 8859-1 byte, \S\ the upper half of the alphabet
    // (0x30 + 128 = 0xB0, the degree sign), and a surrogate pair in \X2\ one
    // character beyond the Basic Multilingual Plane.
    EXPECT_EQ(ifc::decodeStepString("45\\X\\B0"), "45\xC2\xB0");
    EXPECT_EQ(ifc::decodeStepString("45\\S\\0"), "45\xC2\xB0");
    EXPECT_EQ(ifc::decodeStepString("\\X2\\D83DDE00\\X0\\"), "\xF0\x9F\x98\x80");
    // Raw UTF-8, which other writers put in, is kept; a raw Latin-1 byte is
    // read as Latin-1.
    EXPECT_EQ(ifc::decodeStepString("caf\xC3\xA9"), "caf\xC3\xA9");
    EXPECT_EQ(ifc::decodeStepString("caf\xE9"), "caf\xC3\xA9");
}

// STEP's REAL: digits, a point, digits, optionally E and an exponent; the
// point is required.
TEST(IfcStepReal, WritesTheShortestTextThatReadsBackAndAlwaysAPoint)
{
    EXPECT_EQ(ifc::stepReal(1.0), "1.");
    EXPECT_EQ(ifc::stepReal(-2.0), "-2.");
    EXPECT_EQ(ifc::stepReal(0.1), "0.1");
    EXPECT_EQ(ifc::stepReal(1e-5), "1.E-05");
    EXPECT_EQ(ifc::stepReal(1e20), "1.E+20");
    EXPECT_EQ(ifc::stepReal(-0.0), "0.");
    EXPECT_EQ(ifc::stepReal(6250000.15), "6250000.15");
    for (const double value : {6250000.123456789, 1.0 / 3.0, 334016.08, -1e-300, 2.5e-8}) {
        const std::string text = ifc::stepReal(value);
        ASSERT_NE(text.find('.'), std::string::npos) << text;
        // "1.E-05" is a C number too, so the C reader reads it back whole.
        double back = 0.0;
        const auto [end, error] = std::from_chars(text.data(), text.data() + text.size(), back);
        EXPECT_EQ(error, std::errc{}) << text;
        EXPECT_EQ(end, text.data() + text.size()) << text;
        EXPECT_EQ(back, value) << text;
    }
}

TEST(IfcStepString, CountsCharactersNotBytes)
{
    EXPECT_EQ(ifc::characterCount("abc"), 3u);
    EXPECT_EQ(ifc::characterCount("45\xC2\xB0"), 3u);
    EXPECT_EQ(ifc::characterCount("\xF0\x9F\x98\x80"), 1u);
}
