// Colour names: the one fold, the standard names, a customisation's own table
// and the resolver that asks both (include/katana/entity/colour_names.hpp).
//
// The standard RGB values asserted here are those of the CSS named colours
// (CSS Color Module Level 4, section 6.1), written out by hand, and the two
// departures from that list the table's own comment states.

#include <gtest/gtest.h>

#include <algorithm>
#include <string>
#include <vector>

#include "katana/entity/colour_names.hpp"

using katana::core::ErrorCode;
using katana::entity::Color;
using katana::entity::ColourTable;
using katana::entity::foldColourName;
using katana::entity::nearestStandardColour;
using katana::entity::resolveColour;
using katana::entity::standardColour;
using katana::entity::standardColourNames;

// ---- the fold -------------------------------------------------------------------------------

TEST(ColourNames, AFoldedNameIsLowerCaseTrimmedAndReadsUnderscoreAndHyphenAsABlank)
{
    EXPECT_EQ(foldColourName("red"), "red");
    EXPECT_EQ(foldColourName("RED"), "red");
    EXPECT_EQ(foldColourName("  Dark Green\t"), "dark green");
    EXPECT_EQ(foldColourName("dark_green"), "dark green");
    EXPECT_EQ(foldColourName("Dark-Green"), "dark green");
    EXPECT_EQ(foldColourName("SUI Water_Potable"), "sui water potable");
    EXPECT_EQ(foldColourName(""), "");
    EXPECT_EQ(foldColourName("   "), "");
    // Only ASCII letters change case: a name in another script is left as it
    // is, byte for byte (u-umlaut is C3 BC, its capital C3 9C).
    EXPECT_EQ(foldColourName("GR\xC3\x9CN"), "gr\xC3\x9Cn");
    // Blanks INSIDE a name are kept as they are, so two blanks are not one.
    EXPECT_EQ(foldColourName("dark  red"), "dark  red");
    EXPECT_EQ(foldColourName("dark__red"), "dark  red");
}

TEST(ColourNames, GrayIsGreyWhereItIsTheWholeNameOrItsLastWord)
{
    EXPECT_EQ(foldColourName("gray"), "grey");
    EXPECT_EQ(foldColourName("GRAY"), "grey");
    EXPECT_EQ(foldColourName("Dark Gray"), "dark grey");
    EXPECT_EQ(foldColourName("light_gray"), "light grey");
    EXPECT_EQ(foldColourName("sui dark-gray"), "sui dark grey");
    // Not inside a word, and not a word that is not the last.
    EXPECT_EQ(foldColourName("grayish"), "grayish");
    EXPECT_EQ(foldColourName("gray blue"), "gray blue");
    EXPECT_EQ(foldColourName("stingray"), "stingray");
}

// ---- the standard names ---------------------------------------------------------------------

TEST(ColourNames, TheStandardNamesAreTwentySevenPlainThenDarkThenLight)
{
    // 14 plain names, 7 dark and 6 light, in that order.
    const std::vector<std::string> expected = {
        "red",          "green",        "blue",      "yellow",     "cyan",       "magenta",
        "white",        "black",        "grey",      "orange",     "brown",      "purple",
        "pink",         "violet",       "dark red",  "dark green", "dark blue",  "dark cyan",
        "dark magenta", "dark orange",  "dark grey", "light grey", "light blue", "light green",
        "light cyan",   "light yellow", "light pink"};
    ASSERT_EQ(expected.size(), 27u);
    EXPECT_EQ(standardColourNames(), expected);
    for (const std::string& name : expected) {
        EXPECT_TRUE(standardColour(name).has_value()) << name;
        EXPECT_EQ(foldColourName(name), name) << "a listed name is already in its folded form";
    }
}

TEST(ColourNames, AStandardNameHasTheRgbTheSameNameHasInTheCssList)
{
    struct Named {
        const char* name;
        Color colour;
    };
    // CSS: red #FF0000, blue #0000FF, yellow #FFFF00, cyan #00FFFF, magenta
    // #FF00FF, white #FFFFFF, black #000000, gray #808080, orange #FFA500,
    // brown #A52A2A, purple #800080, pink #FFC0CB, violet #EE82EE, darkred
    // #8B0000, darkgreen #006400, darkblue #00008B, darkcyan #008B8B,
    // darkmagenta #8B008B, darkorange #FF8C00, lightblue #ADD8E6, lightgreen
    // #90EE90, lightcyan #E0FFFF, lightyellow #FFFFE0, lightpink #FFB6C1.
    for (const Named& named : {Named{"red", {0xFF, 0x00, 0x00, 255}},
                               Named{"blue", {0x00, 0x00, 0xFF, 255}},
                               Named{"yellow", {0xFF, 0xFF, 0x00, 255}},
                               Named{"cyan", {0x00, 0xFF, 0xFF, 255}},
                               Named{"magenta", {0xFF, 0x00, 0xFF, 255}},
                               Named{"white", {0xFF, 0xFF, 0xFF, 255}},
                               Named{"black", {0x00, 0x00, 0x00, 255}},
                               Named{"grey", {0x80, 0x80, 0x80, 255}},
                               Named{"orange", {0xFF, 0xA5, 0x00, 255}},
                               Named{"brown", {0xA5, 0x2A, 0x2A, 255}},
                               Named{"purple", {0x80, 0x00, 0x80, 255}},
                               Named{"pink", {0xFF, 0xC0, 0xCB, 255}},
                               Named{"violet", {0xEE, 0x82, 0xEE, 255}},
                               Named{"dark red", {0x8B, 0x00, 0x00, 255}},
                               Named{"dark green", {0x00, 0x64, 0x00, 255}},
                               Named{"dark blue", {0x00, 0x00, 0x8B, 255}},
                               Named{"dark cyan", {0x00, 0x8B, 0x8B, 255}},
                               Named{"dark magenta", {0x8B, 0x00, 0x8B, 255}},
                               Named{"dark orange", {0xFF, 0x8C, 0x00, 255}},
                               Named{"light blue", {0xAD, 0xD8, 0xE6, 255}},
                               Named{"light green", {0x90, 0xEE, 0x90, 255}},
                               Named{"light cyan", {0xE0, 0xFF, 0xFF, 255}},
                               Named{"light yellow", {0xFF, 0xFF, 0xE0, 255}},
                               Named{"light pink", {0xFF, 0xB6, 0xC1, 255}}}) {
        EXPECT_EQ(standardColour(named.name), named.colour) << named.name;
    }
    // The departures. CSS "green" is the half-bright #008000; a CAD palette
    // means the primary, CSS "lime". And CSS darkgray (#A9A9A9) is LIGHTER
    // than gray, so the greys are set either side of it instead: a quarter
    // and three quarters.
    EXPECT_EQ(standardColour("green"), (Color{0, 255, 0, 255}));
    EXPECT_EQ(standardColour("dark grey"), (Color{64, 64, 64, 255}));
    EXPECT_EQ(standardColour("light grey"), (Color{192, 192, 192, 255}));
}

TEST(ColourNames, AStandardNameIsFoundHoweverItIsSpelledAndNoOtherNameIs)
{
    EXPECT_EQ(standardColour("RED"), standardColour("red"));
    EXPECT_EQ(standardColour(" Dark_Green "), standardColour("dark green"));
    EXPECT_EQ(standardColour("Light-Gray"), standardColour("light grey"));
    EXPECT_EQ(standardColour("gray"), (Color{128, 128, 128, 255}));
    for (const char* name : {"", "pen 025", "sui water potable", "off yellow", "dark", "reddish",
                             "dark  red", "darkred"}) {
        EXPECT_FALSE(standardColour(name).has_value()) << '"' << name << '"';
    }
}

TEST(ColourNames, TheNearestStandardNameIsTheOneAtTheLeastDistanceInRgb)
{
    // Each name is its own nearest: no two share an RGB.
    for (const std::string& name : standardColourNames()) {
        EXPECT_EQ(nearestStandardColour(*standardColour(name)), name);
    }
    // (250, 5, 5) is 5, 5, 5 from red - 75 squared - and 111 at least from
    // dark red in the first channel alone.
    EXPECT_EQ(nearestStandardColour({250, 5, 5, 255}), "red");
    // (100, 100, 100): from grey (128) 3 x 28^2 = 2352, from dark grey (64)
    // 3 x 36^2 = 3888.
    EXPECT_EQ(nearestStandardColour({100, 100, 100, 255}), "grey");
    // (90, 90, 90): from grey 3 x 38^2 = 4332, from dark grey 3 x 26^2 = 2028.
    EXPECT_EQ(nearestStandardColour({90, 90, 90, 255}), "dark grey");
    // Opacity is not part of the distance.
    EXPECT_EQ(nearestStandardColour({0, 0, 255, 0}), "blue");
}

// ---- a customisation's own table ------------------------------------------------------------

TEST(ColourTable, ANameIsFoundByItsFoldAndKeptAsItWasWritten)
{
    ColourTable table;
    EXPECT_TRUE(table.empty());
    ASSERT_TRUE(table.add("SUI Electricity", Color{255, 127, 0, 255}).ok());
    ASSERT_TRUE(table.add("sui_gas", Color{255, 255, 0, 128}).ok());
    EXPECT_EQ(table.size(), 2u);

    EXPECT_EQ(table.find("sui electricity"), (Color{255, 127, 0, 255}));
    EXPECT_EQ(table.find(" Sui-Electricity "), (Color{255, 127, 0, 255}));
    EXPECT_EQ(table.find("SUI GAS"), (Color{255, 255, 0, 128})) << "opacity and all";
    EXPECT_FALSE(table.find("sui water").has_value());
    EXPECT_FALSE(table.find("").has_value());

    // In the order of the folds - "sui electricity" before "sui gas" - with
    // each name as its author wrote it.
    const std::vector<ColourTable::Entry> entries = table.entries();
    ASSERT_EQ(entries.size(), 2u);
    EXPECT_EQ(entries[0].name, "SUI Electricity");
    EXPECT_EQ(entries[1].name, "sui_gas");
    EXPECT_EQ(entries[1].colour, (Color{255, 255, 0, 128}));
}

TEST(ColourTable, TheEntriesComeInTheOrderOfTheirFoldsNotOfTheirBytes)
{
    // By bytes "Zinc" (5A) comes before "apple" (61); folded, "apple" first.
    ColourTable table;
    ASSERT_TRUE(table.add("Zinc", Color{1, 1, 1, 255}).ok());
    ASSERT_TRUE(table.add("apple", Color{2, 2, 2, 255}).ok());
    ASSERT_TRUE(table.add("Mid_tone", Color{3, 3, 3, 255}).ok());
    const auto entries = table.entries();
    ASSERT_EQ(entries.size(), 3u);
    EXPECT_EQ(entries[0].name, "apple");
    EXPECT_EQ(entries[1].name, "Mid_tone");
    EXPECT_EQ(entries[2].name, "Zinc");
}

TEST(ColourTable, AnEmptyNameAndANameThatIsNotTextAreRefused)
{
    ColourTable table;
    // Nothing, blanks, and nothing but the separators the fold reads as blanks.
    for (const char* name : {"", "   ", "\t", "_", "-", " _- "}) {
        const auto status = table.add(name, Color{});
        ASSERT_FALSE(status.ok()) << '"' << name << '"';
        EXPECT_EQ(status.error().code, ErrorCode::InvalidArgument);
    }
    const auto notText = table.add("caf\xFF", Color{});
    ASSERT_FALSE(notText.ok());
    EXPECT_EQ(notText.error().code, ErrorCode::InvalidArgument);
    EXPECT_TRUE(table.empty()) << "a refused name leaves the table as it was";
}

TEST(ColourTable, TwoNamesWithOneFoldAreRefusedAndTheFirstIsKept)
{
    ColourTable table;
    ASSERT_TRUE(table.add("SUI Gas", Color{255, 255, 0, 255}).ok());
    for (const char* again : {"SUI Gas", "sui gas", "sui_gas", "Sui-Gas", " sui gas "}) {
        const auto status = table.add(again, Color{1, 2, 3, 255});
        ASSERT_FALSE(status.ok()) << again;
        EXPECT_EQ(status.error().code, ErrorCode::InvalidArgument);
        // Both spellings are in what it says.
        EXPECT_NE(status.error().describe().find("\"SUI Gas\""), std::string::npos)
            << status.error().describe();
        EXPECT_NE(status.error().describe().find("\"" + std::string(again) + "\""),
                  std::string::npos)
            << status.error().describe();
    }
    EXPECT_EQ(table.size(), 1u);
    EXPECT_EQ(table.find("sui gas"), (Color{255, 255, 0, 255}));
}

TEST(ColourTable, AStandardNameCannotBeGivenAnotherColour)
{
    ColourTable table;
    // Every one of the 27, and spellings that fold to one of them.
    std::vector<std::string> names = standardColourNames();
    names.insert(names.end(), {"Red", "DARK_GREEN", "light-gray", "Gray"});
    for (const std::string& name : names) {
        const auto status = table.add(name, Color{1, 2, 3, 255});
        ASSERT_FALSE(status.ok()) << name;
        EXPECT_EQ(status.error().code, ErrorCode::InvalidArgument);
    }
    EXPECT_TRUE(table.empty());
    // A name that only CONTAINS one is another name.
    EXPECT_TRUE(table.add("off red", Color{200, 0, 0, 255}).ok());
    EXPECT_TRUE(table.add("reddish", Color{210, 0, 0, 255}).ok());
}

TEST(ColourTable, TwoTablesAreEqualWhenTheyHoldTheSameNamesAsWrittenAndTheSameColours)
{
    ColourTable a;
    ColourTable b;
    EXPECT_EQ(a, b);
    ASSERT_TRUE(a.add("sui gas", Color{255, 255, 0, 255}).ok());
    EXPECT_NE(a, b);
    ASSERT_TRUE(b.add("sui gas", Color{255, 255, 0, 255}).ok());
    EXPECT_EQ(a, b);

    // The order of adding is not part of a table.
    ASSERT_TRUE(a.add("sui water", Color{0, 0, 255, 255}).ok());
    ColourTable c;
    ASSERT_TRUE(c.add("sui water", Color{0, 0, 255, 255}).ok());
    ASSERT_TRUE(c.add("sui gas", Color{255, 255, 0, 255}).ok());
    EXPECT_EQ(a, c);

    // Another colour, or another spelling of the same name, is another table:
    // a file written from one would not be the file written from the other.
    ColourTable recoloured;
    ASSERT_TRUE(recoloured.add("sui gas", Color{255, 255, 1, 255}).ok());
    EXPECT_NE(recoloured, b);
    ColourTable respelled;
    ASSERT_TRUE(respelled.add("SUI Gas", Color{255, 255, 0, 255}).ok());
    EXPECT_NE(respelled, b);
}

// ---- the resolver ---------------------------------------------------------------------------

TEST(ColourNames, ANameIsResolvedFromTheTableThenFromTheStandardNames)
{
    ColourTable table;
    ASSERT_TRUE(table.add("sui electricity", Color{255, 127, 0, 255}).ok());

    EXPECT_EQ(resolveColour(table, "SUI Electricity"), (Color{255, 127, 0, 255}));
    EXPECT_EQ(resolveColour(table, "Dark_Green"), (Color{0, 100, 0, 255}));
    EXPECT_FALSE(resolveColour(table, "pen 035").has_value())
        << "a name neither knows leaves the colour alone";
    EXPECT_FALSE(resolveColour(table, "").has_value());

    // With no table at all, the standard names are still there.
    EXPECT_EQ(resolveColour(ColourTable{}, "red"), (Color{255, 0, 0, 255}));
    EXPECT_FALSE(resolveColour(ColourTable{}, "sui electricity").has_value());
}
