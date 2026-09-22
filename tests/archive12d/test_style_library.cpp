// Reading a 12d linestyle or symbol library (PLAN.MD 20.3, slice 1).

#include <gtest/gtest.h>

#include <algorithm>
#include <fstream>
#include <optional>
#include <sstream>
#include <string>

#include "katana/archive12d/style_library.hpp"
#include "katana/archive12d/text_encoding.hpp"

namespace a12 = katana::archive12d;
using katana::entity::LineStyle;
using katana::entity::StrokeOp;
using katana::entity::StyleUnits;

namespace {

a12::StyleLibraryRead read(const std::string& text)
{
    auto result = a12::readStyleLibrary(text);
    EXPECT_TRUE(result.ok()) << (result.ok() ? "" : result.error().describe());
    return result.ok() ? std::move(*result) : a12::StyleLibraryRead{};
}

std::string allWarnings(const a12::StyleLibraryRead& library)
{
    std::string text;
    for (const std::string& warning : library.warnings) {
        text += "\n  " + warning;
    }
    return text;
}

bool anyWarningContains(const a12::StyleLibraryRead& library, std::string_view needle)
{
    return std::any_of(library.warnings.begin(), library.warnings.end(),
                       [needle](const std::string& warning) {
                           return warning.find(needle) != std::string::npos;
                       });
}

std::size_t readCount(const a12::StyleLibraryRead& library, std::string_view keyword)
{
    const auto found = std::find_if(library.tally.begin(), library.tally.end(),
                                    [keyword](const a12::ElementTally& tally) {
                                        return tally.keyword == keyword;
                                    });
    return found == library.tally.end() ? 0 : found->read;
}

} // namespace

TEST(StyleLibrary, EveryCommandOfADefinitionIsKept)
{
    const auto library = read(R"(// a header comment
worldstyle "WATR Main" {
    group  "TfNSW Survey/WATR"
    length 2.5
    factor 3
    xorigin 1
    yorigin -2
    colour "pen 035"
    move 0 0
    draw 3 0
    arc 1.75 0 180
    circle 0.5
    dot 0
    text "WM" 90 1.5 "middle-centre" "Arial" 0.85 0 -0.3 0.035
})");
    ASSERT_EQ(library.library.size(), 1u) << allWarnings(library);
    const LineStyle* style = library.library.find("WATR Main");
    ASSERT_NE(style, nullptr);
    EXPECT_EQ(style->group, "TfNSW Survey/WATR");
    EXPECT_EQ(style->units, StyleUnits::World);
    EXPECT_FALSE(style->atVertices) << "no `mode vertex`, so it runs along the line";
    EXPECT_EQ(style->length, 2.5);
    EXPECT_EQ(style->factor, 3.0);
    EXPECT_EQ(style->origin, katana::geometry::Point2(1.0, -2.0));

    // colour, move, draw, arc, circle, dot, text.
    ASSERT_EQ(style->strokes.size(), 7u);
    EXPECT_EQ(style->strokes[0].op, StrokeOp::Pen);
    EXPECT_EQ(style->strokes[0].pen, "pen 035");
    EXPECT_EQ(style->strokes[1].op, StrokeOp::Move);
    EXPECT_EQ(style->strokes[2].op, StrokeOp::Draw);
    EXPECT_EQ(style->strokes[2].point, katana::geometry::Point2(3.0, 0.0));
    EXPECT_EQ(style->strokes[3].op, StrokeOp::Arc);
    EXPECT_EQ(style->strokes[3].radius, 1.75);
    EXPECT_EQ(style->strokes[3].startAngle, 0.0);
    EXPECT_EQ(style->strokes[3].endAngle, 180.0);
    EXPECT_EQ(style->strokes[4].op, StrokeOp::Circle);
    EXPECT_EQ(style->strokes[4].radius, 0.5);
    EXPECT_EQ(style->strokes[5].op, StrokeOp::Dot);
    EXPECT_EQ(style->strokes[6].op, StrokeOp::Text);

    // `dot` and `text` were both read; the text is the sixth stroke's payload.
    EXPECT_EQ(readCount(library, "text"), 1u);
    ASSERT_EQ(style->texts.size(), 1u);
    EXPECT_EQ(style->texts[0].text, "WM");
    EXPECT_EQ(style->texts[0].angle, 90.0);
    EXPECT_EQ(style->texts[0].height, 1.5);
    EXPECT_EQ(style->texts[0].justify, "middle-centre");
    EXPECT_EQ(style->texts[0].font, "Arial");
    EXPECT_EQ(style->texts[0].widthFactor, 0.85);
    EXPECT_EQ(style->texts[0].unnamed, (std::array<double, 3>{0.0, -0.3, 0.035}))
        << "kept verbatim rather than interpreted";
    EXPECT_TRUE(library.warnings.empty()) << allWarnings(library);
    ASSERT_FALSE(library.comments.empty());
    EXPECT_EQ(library.comments[0], "a header comment");
}

TEST(StyleLibrary, ModeVertexIsWhatMakesADefinitionASymbol)
{
    const auto library = read(R"(worldstyle "CULT Bollard" {
    mode vertex
    move 0 0
    circle 0.3
}
paperstyle "BDGE Abutment Bottom" {
    move 0 0
    draw 3 0
})");
    ASSERT_EQ(library.library.size(), 2u) << allWarnings(library);
    EXPECT_TRUE(library.library.find("CULT Bollard")->atVertices);
    EXPECT_FALSE(library.library.find("BDGE Abutment Bottom")->atVertices);
    EXPECT_EQ(katana::entity::vertexStyleNames(library.library),
              (std::vector<std::string>{"CULT Bollard"}));
}

TEST(StyleLibrary, TheThreeKindsSayHowTheirCoordinatesAreMeasured)
{
    const auto library = read(R"(worldstyle "W" { move 0 0 }
paperstyle "P" { move 0 0 }
twoptstyle "T" { xorigin1 0 yorigin1 0.75 xorigin2 14 yorigin2 0.75
                 stretch_mode 2 cycle_mode 1 move 0 0 draw 14 0 })");
    ASSERT_EQ(library.library.size(), 3u) << allWarnings(library);
    EXPECT_EQ(library.library.find("W")->units, StyleUnits::World);
    EXPECT_EQ(library.library.find("P")->units, StyleUnits::Paper);
    const LineStyle* two = library.library.find("T");
    ASSERT_NE(two, nullptr);
    EXPECT_EQ(two->units, StyleUnits::TwoPoint);
    EXPECT_EQ(two->anchor1, katana::geometry::Point2(0.0, 0.75));
    EXPECT_EQ(two->anchor2, katana::geometry::Point2(14.0, 0.75));
    EXPECT_EQ(two->stretchMode, 2);
    EXPECT_EQ(two->cycleMode, 1);
}

TEST(StyleLibrary, ACommandItDoesNotKnowIsNamedAndCostsOnlyItself)
{
    const auto library = read(R"(worldstyle "S" {
    move 0 0
    hatch_angle 45 90
    draw 1 1
})");
    ASSERT_EQ(library.library.size(), 1u) << allWarnings(library);
    const LineStyle* style = library.library.find("S");
    ASSERT_NE(style, nullptr);
    ASSERT_EQ(style->strokes.size(), 2u) << "the move and the draw either side of it";
    EXPECT_EQ(style->strokes[1].point, katana::geometry::Point2(1.0, 1.0))
        << "the unknown command's values were not read as commands of their own";
    EXPECT_TRUE(anyWarningContains(library, "hatch_angle")) << allWarnings(library);
    EXPECT_EQ(readCount(library, "hatch_angle"), 1u);
}

TEST(StyleLibrary, AKindOfDefinitionItDoesNotKnowIsSkippedWholeAndTheNextIsRead)
{
    const auto library = read(R"(cellstyle "C" { move 0 0 draw 9 9 }
worldstyle "S" { move 0 0 draw 1 1 })");
    ASSERT_EQ(library.library.size(), 1u) << allWarnings(library);
    EXPECT_NE(library.library.find("S"), nullptr);
    EXPECT_TRUE(anyWarningContains(library, "cellstyle")) << allWarnings(library);
}

TEST(StyleLibrary, TheLastDefinitionOfANameIsTheOneThatTakesEffect)
{
    // Four of the Transport for NSW definitions are in both library files.
    const auto library = read(R"(worldstyle "BUIL Doorway" { move 0 0 draw 1 0 }
worldstyle "BUIL Doorway" { move 0 0 draw 2 0 })");
    ASSERT_EQ(library.library.size(), 1u) << allWarnings(library);
    EXPECT_EQ(library.library.find("BUIL Doorway")->strokes.at(1).point,
              katana::geometry::Point2(2.0, 0.0));
    EXPECT_EQ(library.replaced, 1u);
}

TEST(StyleLibrary, AValueThatIsNotANumberIsReportedAndTheDefinitionKeepsTheRest)
{
    const auto library = read(R"(worldstyle "S" { move 0 0 draw two 0 circle 0.5 })");
    ASSERT_EQ(library.library.size(), 1u) << allWarnings(library);
    const LineStyle* style = library.library.find("S");
    ASSERT_NE(style, nullptr);
    EXPECT_EQ(style->strokes.size(), 2u) << "the move and the circle; the bad draw is dropped";
    EXPECT_TRUE(anyWarningContains(library, "where a number should be")) << allWarnings(library);
}

TEST(StyleLibrary, AQuoteThatIsNeverClosedFailsTheWholeRead)
{
    // Not a warning: past an unterminated quote nothing that follows can be
    // trusted to be what it looks like.
    const auto result = a12::readStyleLibrary("worldstyle \"S\" { move 0 0 }\nworldstyle \"oops {");
    ASSERT_FALSE(result.ok());
    EXPECT_NE(result.error().describe().find("never closed"), std::string::npos)
        << result.error().describe();
}

TEST(StyleLibrary, AnEmptyLibraryIsNotAnError)
{
    const auto library = read("// nothing but a comment\n");
    EXPECT_TRUE(library.library.empty());
    EXPECT_TRUE(library.warnings.empty()) << allWarnings(library);
}

TEST(StyleLibrary, TwoFilesLoadIntoOneCustomisationWithTheLaterWinning)
{
    auto first = a12::readStyleLibrary(R"(worldstyle "A" { move 0 0 draw 1 0 })");
    ASSERT_TRUE(first.ok());
    auto second = a12::readStyleLibraryInto(std::move(first->library),
                                            R"(worldstyle "A" { move 0 0 draw 5 0 }
worldstyle "B" { move 0 0 })");
    ASSERT_TRUE(second.ok());
    EXPECT_EQ(second->library.size(), 2u);
    EXPECT_EQ(second->library.find("A")->strokes.at(1).point, katana::geometry::Point2(5.0, 0.0));
    EXPECT_EQ(second->replaced, 1u) << "the tally describes THIS file, not the pair";
}

TEST(StyleLibrary, TheBoundsOfADefinitionCoverItsStrokesWithTheFactorApplied)
{
    const auto library = read(R"(worldstyle "S" { factor 2  move -1 -1  draw 3 4  circle 0.5 })");
    const LineStyle* style = library.library.find("S");
    ASSERT_NE(style, nullptr);
    const katana::geometry::Box2 box = style->bounds();
    // move (-1,-1) and draw (3,4) scale to (-2,-2) and (6,8); the circle is
    // drawn about the CURRENT point, so it sits at (6,8) with a radius of
    // 0.5*2 and reaches (7,9).
    EXPECT_EQ(box.min, katana::geometry::Point2(-2.0, -2.0));
    EXPECT_EQ(box.max, katana::geometry::Point2(7.0, 9.0));
}

// ---- the customisation this was built against -------------------------------------------------
//
// A real Transport for NSW library, 796 definitions over two files. These
// SKIP rather than fail when the files are not in the checkout: they carry
// their author's licence notice and may not travel with the source.

namespace {

std::optional<std::string> referenceFile(const std::string& name)
{
    std::ifstream file(std::string(KATANA_12D_REFERENCE_FILES) + "/" + name, std::ios::binary);
    if (!file) {
        return std::nullopt;
    }
    std::ostringstream buffer;
    buffer << file.rdbuf();
    const auto decoded = a12::decodeText(buffer.str());
    EXPECT_TRUE(decoded.ok()) << (decoded.ok() ? "" : decoded.error().describe());
    return decoded.ok() ? std::optional<std::string>(decoded->text) : std::nullopt;
}

} // namespace

TEST(StyleLibrary, TheTransportForNswCustomisationIsReadWhole)
{
    const auto linestyles = referenceFile("user_linestyl_TfNSWv15.4d");
    const auto symbols = referenceFile("user_symbols_TfNSWv15.4d");
    if (!linestyles || !symbols) {
        GTEST_SKIP() << "the reference customisation is not in this checkout";
    }

    auto first = a12::readStyleLibrary(*linestyles);
    ASSERT_TRUE(first.ok()) << first.error().describe();
    // Counted from the file itself, outside Katana: 322 blocks - 238
    // paperstyle, 47 twoptstyle, 37 worldstyle - of which "BDYS Parish" is
    // defined twice, so 321 definitions survive.
    EXPECT_EQ(first->library.size(), 321u);
    EXPECT_EQ(first->replaced, 1u);
    EXPECT_EQ(readCount(*first, "paperstyle"), 238u);
    EXPECT_EQ(readCount(*first, "twoptstyle"), 47u);
    EXPECT_EQ(readCount(*first, "worldstyle"), 37u);
    EXPECT_EQ(readCount(*first, "move"), 10969u);
    EXPECT_EQ(readCount(*first, "draw"), 10770u);
    EXPECT_TRUE(first->warnings.empty()) << allWarnings(*first);

    auto both = a12::readStyleLibraryInto(std::move(first->library), *symbols);
    ASSERT_TRUE(both.ok()) << both.error().describe();
    EXPECT_EQ(readCount(*both, "worldstyle"), 473u);
    EXPECT_EQ(readCount(*both, "twoptstyle"), 1u);
    EXPECT_TRUE(both->warnings.empty()) << allWarnings(*both);
    // Three of this file's 474 are already defined by the first: "BUIL
    // Doorway", "TOPO Tree Foliage" and "C TOPO Tree Foliage".
    EXPECT_EQ(both->replaced, 3u);
    EXPECT_EQ(both->library.size(), 792u);

    // The symbols are the definitions that say `mode vertex`.
    EXPECT_EQ(katana::entity::vertexStyleNames(both->library).size(), 157u);
    EXPECT_EQ(katana::entity::styleGroups(both->library).size(), 71u);

    // The one the user named: a water main's linestyle.
    const LineStyle* main = both->library.find("WATR Main");
    ASSERT_NE(main, nullptr) << "the mapfile sends survey code WM* to this";
    EXPECT_EQ(main->group, "TfNSW Survey/WATR");
    EXPECT_FALSE(main->strokes.empty());
}
