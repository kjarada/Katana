// Plot styles and the page setup (plot.hpp's colour modes, page_setup.hpp):
// what a colour prints as in each mode, which sheets a selection names, what
// the files of a plot are called, and the page setup's JSON and its one
// undoable step.

#include <gtest/gtest.h>

#include <cmath>
#include <limits>
#include <string>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/cad/plot.hpp"
#include "katana/cad/plotting/page_setup.hpp"
#include "katana/cad/plotting/sheet_commands.hpp"
#include "katana/cad/plotting/sheet_json.hpp"
#include "katana/cad/plotting/sheet_set.hpp"

using katana::cad::Document;
using katana::cad::luminance;
using katana::cad::paperColour;
using katana::cad::paperFillColour;
using katana::cad::PlotColourMode;
using katana::cad::PlotSettings;
using katana::core::ErrorCode;
using katana::entity::Color;
using namespace katana::cad::plotting;

namespace {

PlotSettings inMode(PlotColourMode mode)
{
    PlotSettings settings;
    settings.colourMode = mode;
    return settings;
}

// A set of `count` sheets named "SHEET 1"..., with ids s1, s2 ...
SheetSet setOf(std::size_t count)
{
    SheetSet set;
    for (std::size_t i = 0; i < count; ++i) {
        Sheet sheet;
        sheet.id = "s" + std::to_string(i + 1);
        sheet.name = "SHEET " + std::to_string(i + 1);
        set.sheets.push_back(sheet);
    }
    return set;
}

std::vector<std::size_t> selected(std::string_view text, const SheetSet& set)
{
    const auto result = parseSheetSelection(text, set);
    EXPECT_TRUE(result.ok()) << text << ": " << result.error().describe();
    return result.ok() ? *result : std::vector<std::size_t>{};
}

} // namespace

// ---- colour modes ------------------------------------------------------------------------

TEST(PlotStyle, LuminanceIsTheRec601LumaInIntegersAndAGreyIsItsOwn)
{
    EXPECT_EQ(luminance(Color{255, 0, 0, 255}), 76);  // 76.245
    EXPECT_EQ(luminance(Color{0, 255, 0, 255}), 150); // 149.685
    EXPECT_EQ(luminance(Color{0, 0, 255, 255}), 29);  // 29.07
    EXPECT_EQ(luminance(Color{255, 255, 255, 255}), 255);
    EXPECT_EQ(luminance(Color{0, 0, 0, 255}), 0);
    for (int level = 0; level <= 255; ++level) {
        const auto v = static_cast<std::uint8_t>(level);
        ASSERT_EQ(luminance(Color{v, v, v, 255}), v) << level;
    }
    // Alpha is not lightness.
    EXPECT_EQ(luminance(Color{255, 0, 0, 10}), 76);
}

TEST(PlotStyle, ColourKeepsEveryColourAndTheWhiteRuleAsItWas)
{
    const PlotSettings colour = inMode(PlotColourMode::Colour);
    EXPECT_EQ(paperColour(Color{255, 0, 0, 255}, colour), (Color{255, 0, 0, 255}));
    EXPECT_EQ(paperColour(Color{255, 255, 255, 200}, colour), (Color{0, 0, 0, 200}));
    EXPECT_EQ(paperFillColour(Color{0, 90, 200, 40}, colour), (Color{0, 90, 200, 40}));
    // The default plot is in colour, at the line weights drawn.
    EXPECT_EQ(PlotSettings{}.colourMode, PlotColourMode::Colour);
    EXPECT_EQ(PlotSettings{}.lineWeightScale, 1.0);
}

TEST(PlotStyle, GreyscalePrintsEachColourAsTheGreyOfItsLuminanceAndKeepsAlpha)
{
    const PlotSettings grey = inMode(PlotColourMode::Greyscale);
    EXPECT_EQ(paperColour(Color{255, 0, 0, 255}, grey), (Color{76, 76, 76, 255}));
    EXPECT_EQ(paperColour(Color{0, 125, 50, 128}, grey), (Color{79, 79, 79, 128}));
    // The white rule comes first: white prints black, not white.
    EXPECT_EQ(paperColour(Color{255, 255, 255, 255}, grey), (Color{0, 0, 0, 255}));
    // A light colour that is not white stays light.
    EXPECT_EQ(paperColour(Color{255, 255, 224, 255}, grey), (Color{251, 251, 251, 255}));
    // A fill keeps its lightness as a grey, as a pen does.
    EXPECT_EQ(paperFillColour(Color{0, 90, 200, 40}, grey), (Color{76, 76, 76, 40}));
    // Nothing printed in greyscale has any colour in it.
    for (int r = 0; r < 256; r += 15) {
        for (int g = 0; g < 256; g += 15) {
            for (int b = 0; b < 256; b += 15) {
                const Color c = paperColour(
                    Color{static_cast<std::uint8_t>(r), static_cast<std::uint8_t>(g),
                          static_cast<std::uint8_t>(b), 255},
                    grey);
                ASSERT_TRUE(c.r == c.g && c.g == c.b) << r << "," << g << "," << b;
            }
        }
    }
}

TEST(PlotStyle, MonochromePrintsEveryPenBlackAndAFillBlackOrWhiteByItsLightness)
{
    const PlotSettings mono = inMode(PlotColourMode::Monochrome);
    // Every pen, however light, is black; its alpha is its own.
    EXPECT_EQ(paperColour(Color{255, 255, 0, 255}, mono), (Color{0, 0, 0, 255}));
    EXPECT_EQ(paperColour(Color{190, 190, 190, 255}, mono), (Color{0, 0, 0, 255}));
    EXPECT_EQ(paperColour(Color{0, 90, 200, 99}, mono), (Color{0, 0, 0, 99}));
    // A fill in the darker half prints black, in the lighter half drops to
    // the paper's white; the threshold is a luminance of 128.
    EXPECT_EQ(paperFillColour(Color{127, 127, 127, 255}, mono), (Color{0, 0, 0, 255}));
    EXPECT_EQ(paperFillColour(Color{128, 128, 128, 255}, mono), (Color{255, 255, 255, 255}));
    EXPECT_EQ(paperFillColour(Color{200, 0, 30, 18}, mono), (Color{0, 0, 0, 18}));   // 63
    EXPECT_EQ(paperFillColour(Color{255, 255, 0, 90}, mono), (Color{255, 255, 255, 90})); // 226
    // A white fill takes the white rule first, as a white pen does.
    EXPECT_EQ(paperFillColour(Color{255, 255, 255, 255}, mono), (Color{0, 0, 0, 255}));
    PlotSettings keepWhite = mono;
    keepWhite.whiteToBlack = false;
    EXPECT_EQ(paperFillColour(Color{255, 255, 255, 255}, keepWhite), (Color{255, 255, 255, 255}));
}

TEST(PlotStyle, ColourModesAreNamedAndReadWithTheirCommonSpellings)
{
    for (const PlotColourMode mode :
         {PlotColourMode::Colour, PlotColourMode::Greyscale, PlotColourMode::Monochrome}) {
        EXPECT_EQ(katana::cad::plotColourModeFrom(katana::cad::toString(mode)), mode);
    }
    EXPECT_EQ(katana::cad::toString(PlotColourMode::Greyscale), "greyscale");
    EXPECT_EQ(katana::cad::plotColourModeFrom("COLOR"), PlotColourMode::Colour);
    EXPECT_EQ(katana::cad::plotColourModeFrom("grey"), PlotColourMode::Greyscale);
    EXPECT_EQ(katana::cad::plotColourModeFrom("Gray"), PlotColourMode::Greyscale);
    EXPECT_EQ(katana::cad::plotColourModeFrom("grayscale"), PlotColourMode::Greyscale);
    EXPECT_EQ(katana::cad::plotColourModeFrom("mono"), PlotColourMode::Monochrome);
    EXPECT_FALSE(katana::cad::plotColourModeFrom("sepia").has_value());
    EXPECT_FALSE(katana::cad::plotColourModeFrom("").has_value());
}

// ---- selections ----------------------------------------------------------------------------

TEST(SheetSelection, PositionsAndRangesNameSheetsInTheOrderGivenEachOnce)
{
    const SheetSet set = setOf(6);
    EXPECT_EQ(selected("1,3-5", set), (std::vector<std::size_t>{0, 2, 3, 4}));
    EXPECT_EQ(selected(" 4 , 2 ", set), (std::vector<std::size_t>{3, 1}));
    EXPECT_EQ(selected("2-3,1-4", set), (std::vector<std::size_t>{1, 2, 0, 3}));
    EXPECT_EQ(selected("6", set), (std::vector<std::size_t>{5}));
    EXPECT_EQ(selected("3-3", set), (std::vector<std::size_t>{2}));
    // A trailing or doubled comma is only punctuation.
    EXPECT_EQ(selected("1,,2,", set), (std::vector<std::size_t>{0, 1}));
}

TEST(SheetSelection, NothingOrAllIsEverySheetAndAnIdNamesItsSheetWhereverItIs)
{
    SheetSet set = setOf(3);
    EXPECT_EQ(selected("", set), (std::vector<std::size_t>{0, 1, 2}));
    EXPECT_EQ(selected("all", set), (std::vector<std::size_t>{0, 1, 2}));
    EXPECT_EQ(selected(" ALL ", set), (std::vector<std::size_t>{0, 1, 2}));
    // Ids follow a sheet that has moved.
    std::swap(set.sheets[0], set.sheets[2]);
    EXPECT_EQ(selected("s1", set), (std::vector<std::size_t>{2}));
    EXPECT_EQ(selected("s3,2", set), (std::vector<std::size_t>{0, 1}));
}

TEST(SheetSelection, ASheetThatIsNotThereOrABackwardRangeIsRefusedSayingWhich)
{
    const SheetSet set = setOf(5);
    const auto why = [&set](std::string_view text) {
        const auto result = parseSheetSelection(text, set);
        EXPECT_FALSE(result.ok()) << text;
        EXPECT_EQ(result.ok() ? ErrorCode::Internal : result.error().code, ErrorCode::InvalidArgument);
        return result.ok() ? std::string{} : result.error().message;
    };
    EXPECT_EQ(why("7"), "there is no sheet 7: the set has 5 sheets");
    EXPECT_EQ(why("0"), "there is no sheet 0: sheets are numbered from 1");
    EXPECT_EQ(why("2-9"), "there is no sheet 9: the set has 5 sheets");
    EXPECT_EQ(why("5-3"), "the range 5-3 runs backwards: write 3-5");
    EXPECT_EQ(why("s9"), "\"s9\" is neither a sheet number nor a sheet's id");
    EXPECT_EQ(why("1-"), "\"1-\" is neither a sheet number nor a sheet's id");
    EXPECT_EQ(why("99999999999999999999999"),
              "there is no sheet 99999999999999999999999: the set has 5 sheets");
    EXPECT_EQ(why(","), "the selection names no sheet");
    // A space inside a part is not a separator, and never joins two numbers
    // into another sheet ("1 3" is not sheet 13).
    EXPECT_EQ(why("1 3"), "\"1 3\" is neither a sheet number nor a sheet's id");
    EXPECT_EQ(why("1 - 2 3"), "\"1-2 3\" is neither a sheet number nor a sheet's id");
    EXPECT_EQ(selected(" 1 - 3 ,\t5 ", set), (std::vector<std::size_t>{0, 1, 2, 4}));
    EXPECT_EQ(parseSheetSelection("", SheetSet{}).error().message, "there are no sheets to plot");
}

TEST(SheetSelection, ASelectionIsWrittenShortAndReadsBackTheSame)
{
    const std::vector<std::size_t> some{0, 2, 3, 4, 7, 8};
    EXPECT_EQ(formatSheetSelection(some), "1,3-5,8,9");
    EXPECT_EQ(formatSheetSelection(std::vector<std::size_t>{4, 3, 2}), "5,4,3");
    EXPECT_EQ(formatSheetSelection(std::vector<std::size_t>{}), "");
    const SheetSet set = setOf(10);
    EXPECT_EQ(selected(formatSheetSelection(some), set), some);
}

// ---- file names ------------------------------------------------------------------------------

TEST(PlotFileNames, ANameIsMadeSafeForEveryFileSystem)
{
    EXPECT_EQ(sanitiseFileName("PLAN: CH 0/100 <A>?"), "PLAN- CH 0-100 -A--");
    EXPECT_EQ(sanitiseFileName("  ROAD   PLAN  "), "ROAD PLAN");
    EXPECT_EQ(sanitiseFileName("SHEET 1..."), "SHEET 1");
    EXPECT_EQ(sanitiseFileName("A\tB\nC"), "A-B-C");
    EXPECT_EQ(sanitiseFileName("con"), "_con");
    EXPECT_EQ(sanitiseFileName("LPT1.pdf"), "_LPT1.pdf");
    EXPECT_EQ(sanitiseFileName("COM10"), "COM10"); // not a device
    EXPECT_EQ(sanitiseFileName(" .. "), "sheet");
    EXPECT_EQ(sanitiseFileName(""), "sheet");
    // Non-ASCII text is kept, and a long name is cut between characters,
    // never inside one: 60 two-byte characters are 120 bytes, so a 61st
    // would not fit whole.
    std::string greek;
    for (int i = 0; i < 70; ++i) {
        greek += "\xCE\xB1"; // alpha
    }
    const std::string cut = sanitiseFileName(greek);
    EXPECT_EQ(cut.size(), 120u);
    EXPECT_EQ(cut.substr(0, 2), "\xCE\xB1");
    std::string mixed = "X" + greek; // the cut now falls inside a character
    const std::string cutMixed = sanitiseFileName(mixed);
    EXPECT_EQ(cutMixed.size(), 119u);
    EXPECT_EQ(cutMixed.back(), '\xB1');
}

TEST(PlotFileNames, ThePatternUsesTheSheetNumberTokensAndTheSheetsNameAndId)
{
    SheetSet set = setOf(12);
    set.sheets[2].name = "PLAN TILE 3";
    set.defaults.setNumber = "C";
    set.numbering = "{set}-{n:03}";
    EXPECT_EQ(sheetFileName(set, 2, kDefaultFileNamePattern), "C03 PLAN TILE 3");
    EXPECT_EQ(sheetFileName(set, 2, "{number} {name}"), "C-003 PLAN TILE 3");
    EXPECT_EQ(sheetFileName(set, 2, "{id}_{n}_of_{N}"), "s3_3_of_12");
    EXPECT_EQ(sheetFileName(set, 11, "{n:02}"), "12");
    EXPECT_EQ(sheetFileName(set, 12, "{n}"), ""); // past the end
    // Without a set number the default pattern starts at the number.
    set.defaults.setNumber.clear();
    EXPECT_EQ(sheetFileName(set, 0, kDefaultFileNamePattern), "01 SHEET 1");
    // A name with a separator in it stays one file name.
    set.sheets[0].name = "ROAD CH 0/250";
    EXPECT_EQ(sheetFileName(set, 0, "{name}"), "ROAD CH 0-250");
}

TEST(PlotFileNames, TwoSheetsThatWouldShareANameAreToldApartInOrder)
{
    SheetSet set = setOf(4);
    set.sheets[0].name = "PLAN";
    set.sheets[1].name = "plan";
    set.sheets[2].name = "PLAN (2)";
    set.sheets[3].name = "PLAN";
    const std::vector<std::size_t> all{0, 1, 2, 3};
    // Names that differ only in case are one file on Windows.
    EXPECT_EQ(sheetFileNames(set, all, "{name}"),
              (std::vector<std::string>{"PLAN", "plan (2)", "PLAN (2) (2)", "PLAN (3)"}));
    // A pattern with no per-sheet token still gives every sheet its file.
    EXPECT_EQ(sheetFileNames(set, std::vector<std::size_t>{0, 1}, "SET"),
              (std::vector<std::string>{"SET", "SET (2)"}));
}

TEST(PlotFileNames, APatternWithAnUnknownTokenOrAnOpenBraceIsRefused)
{
    EXPECT_TRUE(validateFileNamePattern(kDefaultFileNamePattern).ok());
    EXPECT_TRUE(validateFileNamePattern("{number} - {name} ({id}) {N}").ok());
    EXPECT_TRUE(validateFileNamePattern("plan}").ok()); // a closing brace is only text
    EXPECT_EQ(validateFileNamePattern("{sheet}").error().message,
              "the file-name pattern has no token {sheet}: use {n}, {n:02}, {N}, {set}, {name}, "
              "{number} or {id}");
    EXPECT_FALSE(validateFileNamePattern("{n:xx}").ok());
    EXPECT_EQ(validateFileNamePattern("{name").error().message,
              "a brace is left open in the file-name pattern \"{name\"");
    EXPECT_EQ(validateFileNamePattern("   ").error().message, "the file-name pattern is empty");
}

// ---- the page setup ----------------------------------------------------------------------------

TEST(PageSetup, AResolutionOrLineWeightScaleOutOfRangeIsRefused)
{
    PageSetup setup;
    EXPECT_TRUE(validatePageSetup(setup).ok());
    setup.dpi = 49.0;
    EXPECT_EQ(validatePageSetup(setup).error().message, "the resolution must be 50 to 1200 dpi");
    setup.dpi = std::numeric_limits<double>::quiet_NaN();
    EXPECT_FALSE(validatePageSetup(setup).ok());
    setup.dpi = 1200.0;
    EXPECT_TRUE(validatePageSetup(setup).ok());
    setup.lineWeightScale = 0.0;
    EXPECT_EQ(validatePageSetup(setup).error().message, "the line weight scale must be 0.1 to 5");
    setup.lineWeightScale = 5.0;
    EXPECT_TRUE(validatePageSetup(setup).ok());
    setup.fileNamePattern = "{nope}";
    EXPECT_FALSE(validatePageSetup(setup).ok());
}

TEST(PageSetup, ThePlotSettingsCarryItsStyleAndResolution)
{
    PageSetup setup;
    setup.colourMode = PlotColourMode::Monochrome;
    setup.lineWeightScale = 1.4;
    setup.dpi = 600.0;
    const PlotSettings settings = plotSettingsFor(setup);
    EXPECT_EQ(settings.colourMode, PlotColourMode::Monochrome);
    EXPECT_EQ(settings.lineWeightScale, 1.4);
    EXPECT_EQ(settings.dpi, 600.0);
    EXPECT_TRUE(settings.whiteToBlack);
}

TEST(PageSetup, ItRoundTripsThroughJsonAndIsLeftOutAtItsDefaults)
{
    SheetSet set = setOf(1);
    const std::string plain = *sheetSetToJson(set);
    EXPECT_EQ(plain.find("page_setup"), std::string::npos);

    set.pageSetup.colourMode = PlotColourMode::Greyscale;
    set.pageSetup.lineWeightScale = 0.1 + 0.2; // written in full
    set.pageSetup.dpi = 450.0;
    set.pageSetup.fileNamePattern = "{number} {name}";
    set.pageSetup.filePerSheet = true;
    const auto json = sheetSetToJson(set);
    ASSERT_TRUE(json.ok()) << json.error().describe();
    EXPECT_NE(json->find(R"("page_setup":{"colour_mode":"greyscale","dpi":450.0,)"
                         R"("file_name_pattern":"{number} {name}","file_per_sheet":true,)"
                         R"("line_weight_scale":0.30000000000000004})"),
              std::string::npos)
        << *json;
    const auto back = sheetSetFromJson(*json);
    ASSERT_TRUE(back.ok()) << back.error().describe();
    EXPECT_TRUE(*back == set);
    EXPECT_EQ(*sheetSetToJson(*back), *json);

    // Only what differs is written.
    SheetSet one = setOf(1);
    one.pageSetup.colourMode = PlotColourMode::Monochrome;
    EXPECT_NE(sheetSetToJson(one)->find(R"("page_setup":{"colour_mode":"monochrome"})"),
              std::string::npos);
}

TEST(PageSetup, AnUnknownColourModeIsAParseFailure)
{
    const auto set = sheetSetFromJson(
        R"({"format":"katana-sheets","version":1,"page_setup":{"colour_mode":"sepia"}})");
    ASSERT_FALSE(set.ok());
    EXPECT_EQ(set.error().code, ErrorCode::ParseFailure);
    const auto read = sheetSetFromJson(
        R"({"format":"katana-sheets","version":1,"page_setup":{"colour_mode":"monochrome","dpi":150}})");
    ASSERT_TRUE(read.ok()) << read.error().describe();
    EXPECT_EQ(read->pageSetup.colourMode, PlotColourMode::Monochrome);
    EXPECT_EQ(read->pageSetup.dpi, 150.0);
    EXPECT_EQ(read->pageSetup.fileNamePattern, kDefaultFileNamePattern);
}

TEST(PageSetup, SettingItIsOneUndoableStepAndNoStepWhenNothingChanges)
{
    Document document;
    ASSERT_TRUE(addSheet(document, setOf(1).sheets[0]).ok());
    PageSetup setup;
    setup.colourMode = PlotColourMode::Greyscale;
    setup.dpi = 150.0;
    ASSERT_TRUE(setPageSetup(document, setup).ok());
    EXPECT_EQ(document.sheetSet().pageSetup, setup);
    EXPECT_EQ(document.history().undoName(), "PAGE_SETUP");
    // The same again records nothing.
    ASSERT_TRUE(setPageSetup(document, setup).ok());
    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(document.sheetSet().pageSetup, PageSetup{});
    EXPECT_EQ(document.sheetSet().sheets.size(), 1u); // the sheet's step is still there
    ASSERT_TRUE(document.redo().ok());
    EXPECT_EQ(document.sheetSet().pageSetup, setup);

    // Refused setups change nothing.
    PageSetup wrong = setup;
    wrong.dpi = 5.0;
    EXPECT_EQ(setPageSetup(document, wrong).error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(document.sheetSet().pageSetup, setup);
}
