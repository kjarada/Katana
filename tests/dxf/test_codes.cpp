#include <gtest/gtest.h>

#include <string>

#include "katana/dxf/codes.hpp"

namespace dxf = katana::dxf;
using katana::entity::Color;

namespace {

Color rgb(int r, int g, int b)
{
    return Color{static_cast<std::uint8_t>(r), static_cast<std::uint8_t>(g),
                 static_cast<std::uint8_t>(b), 255};
}

} // namespace

TEST(DxfCodes, IndexedColoursOneToNineAreTheNamedColours)
{
    EXPECT_EQ(dxf::indexedColour(1), rgb(255, 0, 0));
    EXPECT_EQ(dxf::indexedColour(2), rgb(255, 255, 0));
    EXPECT_EQ(dxf::indexedColour(3), rgb(0, 255, 0));
    EXPECT_EQ(dxf::indexedColour(4), rgb(0, 255, 255));
    EXPECT_EQ(dxf::indexedColour(5), rgb(0, 0, 255));
    EXPECT_EQ(dxf::indexedColour(6), rgb(255, 0, 255));
    EXPECT_EQ(dxf::indexedColour(7), rgb(255, 255, 255));
    EXPECT_EQ(dxf::indexedColour(8), rgb(128, 128, 128));
    EXPECT_EQ(dxf::indexedColour(9), rgb(192, 192, 192));
}

TEST(DxfCodes, ByBlockByLayerAndOutOfRangeIndicesAreNotColours)
{
    EXPECT_FALSE(dxf::indexedColour(0).has_value());
    EXPECT_FALSE(dxf::indexedColour(256).has_value());
    EXPECT_FALSE(dxf::indexedColour(-1).has_value());
}

TEST(DxfCodes, TheHueRangeStepsFifteenDegreesAtFiveBrightnesses)
{
    // Index 10 is hue 0 at full brightness: red. 11 is the same hue a third
    // saturated: 255 * (1 - (1 - 0) / 3) = 170 in the other channels.
    EXPECT_EQ(dxf::indexedColour(10), rgb(255, 0, 0));
    EXPECT_EQ(dxf::indexedColour(11), rgb(255, 170, 170));
    // 12 is the second brightness, 189.
    EXPECT_EQ(dxf::indexedColour(12), rgb(189, 0, 0));
    // Hue 4 (60 degrees) is yellow, hue 8 green, hue 16 blue.
    EXPECT_EQ(dxf::indexedColour(50), rgb(255, 255, 0));
    EXPECT_EQ(dxf::indexedColour(90), rgb(0, 255, 0));
    EXPECT_EQ(dxf::indexedColour(170), rgb(0, 0, 255));
    // Hue 1 (15 degrees): red full, green a quarter of 255 = 63.75 -> 64.
    EXPECT_EQ(dxf::indexedColour(20), rgb(255, 64, 0));
    // The grey ramp.
    EXPECT_EQ(dxf::indexedColour(250), rgb(51, 51, 51));
    EXPECT_EQ(dxf::indexedColour(255), rgb(255, 255, 255));
}

TEST(DxfCodes, EveryIndexedColourIsItsOwnNearestIndexedColour)
{
    for (int index = 1; index < 256; ++index) {
        const Color colour = *dxf::indexedColour(index);
        // Several indices share an RGB (1 and 10 are both red); the nearest is
        // then the lower index, which draws the same.
        EXPECT_EQ(dxf::indexedColour(dxf::nearestIndexedColour(colour)), colour) << index;
    }
}

TEST(DxfCodes, BlackIsWrittenAsTheForegroundColour)
{
    EXPECT_EQ(dxf::nearestIndexedColour(rgb(0, 0, 0)), 7);
}

TEST(DxfCodes, AColourBetweenIndicesGoesToTheNearest)
{
    // (250, 5, 5) is 5.0 from red (index 1) in each of three channels and
    // much further from anything else; red's lowest index is 1.
    EXPECT_EQ(dxf::nearestIndexedColour(rgb(250, 5, 5)), 1);
}

TEST(DxfCodes, TrueColourIsRedGreenBlueInTheLowTwentyFourBits)
{
    // 16744448 = 0xFF8000.
    EXPECT_EQ(dxf::trueColour(16744448), rgb(255, 128, 0));
    EXPECT_EQ(dxf::trueColour(255), rgb(0, 0, 255));
}

TEST(DxfCodes, LineweightsSnapToTheNearestStandardWeight)
{
    EXPECT_EQ(dxf::nearestLineweight(0.25), 25);
    EXPECT_EQ(dxf::nearestLineweight(0.26), 25); // 1 from 25, 4 from 30
    EXPECT_EQ(dxf::nearestLineweight(0.28), 30); // 3 from 25, 2 from 30
    EXPECT_EQ(dxf::nearestLineweight(0.5), 50);
    EXPECT_EQ(dxf::nearestLineweight(5.0), 211); // past the heaviest
    EXPECT_EQ(dxf::nearestLineweight(0.0), 0);
    EXPECT_EQ(dxf::nearestLineweight(-1.0), 0);
}

TEST(DxfCodes, PercentCodesBecomeTheSignsTheyName)
{
    EXPECT_EQ(dxf::plainText("45%%d"), "45\u00B0");
    EXPECT_EQ(dxf::plainText("%%p0.005"), "\u00B10.005");
    EXPECT_EQ(dxf::plainText("%%c300"), "\u2205300");
    EXPECT_EQ(dxf::plainText("100%%%"), "100%");
    EXPECT_EQ(dxf::plainText("%%uUNDER%%u"), "UNDER");
    EXPECT_EQ(dxf::plainText("%%065"), "A");
    // A lone percent sign is a percent sign.
    EXPECT_EQ(dxf::plainText("50%"), "50%");
}

TEST(DxfCodes, UnicodeEscapesBecomeTheCharactersTheyName)
{
    EXPECT_EQ(dxf::plainText("B\\U+00F6schung"), "B\u00F6schung");
    // Not four hex digits: left as it is.
    EXPECT_EQ(dxf::plainText("\\U+00G0"), "\\U+00G0");
}

TEST(DxfCodes, MTextFormattingCodesAreStripped)
{
    EXPECT_EQ(dxf::plainMText("{\\fArial|b1|i0|c0|p34;SITE}"), "SITE");
    EXPECT_EQ(dxf::plainMText("\\H2.5;BIG\\H1x;small"), "BIGsmall");
    EXPECT_EQ(dxf::plainMText("\\C1;red\\C256;"), "red");
    EXPECT_EQ(dxf::plainMText("\\LUnder\\l \\Oover\\o"), "Under over");
    EXPECT_EQ(dxf::plainMText("\\A1;\\pxqc;centred"), "centred");
    EXPECT_EQ(dxf::plainMText("one\\Ptwo"), "one\ntwo");
    EXPECT_EQ(dxf::plainMText("a\\~b"), "a\u00A0b");
    EXPECT_EQ(dxf::plainMText("\\\\server\\{x\\}"), "\\server{x}");
    EXPECT_EQ(dxf::plainMText("\\S1/2;"), "1/2");
    EXPECT_EQ(dxf::plainMText("\\S+0.1^-0.1;"), "+0.1/-0.1");
    EXPECT_EQ(dxf::plainMText("\\U+00B0 and %%d"), "\u00B0 and \u00B0");
}

TEST(DxfCodes, EncodedTextIsAsciiThatReadsBackAsTheOriginal)
{
    EXPECT_EQ(dxf::encodeText("LOT 42"), "LOT 42");
    EXPECT_EQ(dxf::encodeText("45\u00B0"), "45%%d");
    EXPECT_EQ(dxf::encodeText("\u00B1\u2205"), "%%p%%c");
    EXPECT_EQ(dxf::encodeText("B\u00F6schung"), "B\\U+00F6schung");
    // A percent sign another follows would begin a code: "%%%" is one.
    EXPECT_EQ(dxf::encodeText("50%%d"), "50%%%%d");
    EXPECT_EQ(dxf::plainText(dxf::encodeText("50%%d")), "50%%d");
    // A line break cannot be in a TEXT string.
    EXPECT_EQ(dxf::encodeText("a\tb"), "a b");
    for (const std::string sample : {"R\u00E9sum\u00E9 \u00B0 \u00B1 \u2205 100%", "\u6E2C\u91CF"}) {
        EXPECT_EQ(dxf::plainText(dxf::encodeText(sample)), sample);
    }
}
