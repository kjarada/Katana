// The hand-written customisation in tests/archive12d/data/customisation, read
// whole. Every expected number below was counted by hand from the three files
// (their head comments carry the same counts), not captured from a run: if
// this fails, either the reader or a fixture file changed, and the fixture's
// own comment says which number it should be.

#include <gtest/gtest.h>

#include <array>
#include <filesystem>
#include <fstream>
#include <sstream>
#include <string>
#include <vector>

#include "katana/archive12d/customisation.hpp"
#include "katana/archive12d/domain.hpp"
#include "katana/entity/style_library.hpp"

namespace a12 = katana::archive12d;
using katana::entity::LineStyle;
using katana::entity::StrokeOp;
using katana::entity::StyleUnits;

namespace {

const std::filesystem::path kFixture =
    std::filesystem::path(KATANA_ARCHIVE12D_TEST_DATA) / "customisation";

std::vector<std::filesystem::path> fixtureFiles()
{
    // In name order, as a customisation directory is loaded:
    // test_linestyles.4d, test_survey.mapfile, test_symbols.4d.
    return {kFixture / "test_linestyles.4d", kFixture / "test_survey.mapfile",
            kFixture / "test_symbols.4d"};
}

a12::Customisation loadFixture()
{
    auto loaded = a12::readCustomisation(fixtureFiles());
    EXPECT_TRUE(loaded.ok()) << (loaded.ok() ? "" : loaded.error().describe());
    return loaded.ok() ? std::move(*loaded) : a12::Customisation{};
}

} // namespace

TEST(CustomisationFixture, TheWholeFixtureReadsWithoutAWarningAndCountsAsWritten)
{
    const a12::Customisation fixture = loadFixture();
    EXPECT_TRUE(fixture.warnings.empty()) << fixture.warnings.front();
    EXPECT_TRUE(fixture.errors.empty());

    // 3 linestyles + 4 symbols; no name is defined twice.
    EXPECT_EQ(fixture.library.size(), 7u);
    ASSERT_EQ(fixture.files.size(), 3u);
    EXPECT_EQ(fixture.files[0].kind, a12::CustomisationFile::StyleLibrary);
    EXPECT_EQ(fixture.files[0].read, 3u);
    EXPECT_EQ(fixture.files[1].kind, a12::CustomisationFile::MapFile)
        << "found by what is inside it: it is UTF-16 and says <map_file>";
    EXPECT_EQ(fixture.files[1].read, 11u);
    EXPECT_EQ(fixture.files[2].kind, a12::CustomisationFile::StyleLibrary);
    EXPECT_EQ(fixture.files[2].read, 4u);

    // Survey Mark, Valve and U Turn say `mode vertex`; the Tree does not.
    EXPECT_EQ(katana::entity::vertexStyleNames(fixture.library),
              (std::vector<std::string>{"TEST Survey Mark", "TEST U Turn", "TEST Valve"}));
    EXPECT_EQ(katana::entity::styleGroups(fixture.library),
              (std::vector<std::string>{"Test/Lines", "Test/Marks", "Test/Services",
                                        "Test/Vegetation"}));
}

TEST(CustomisationFixture, EachDefinitionSaysWhichFixtureFileItCameFrom)
{
    const a12::Customisation fixture = loadFixture();
    for (const char* name : {"TEST Dashed Kerb", "TEST Water Main", "TEST Gate"}) {
        const LineStyle* style = fixture.library.find(name);
        ASSERT_NE(style, nullptr) << name;
        EXPECT_EQ(style->source, "test_linestyles.4d") << name;
    }
    for (const char* name : {"TEST Survey Mark", "TEST Tree", "TEST Valve", "TEST U Turn"}) {
        const LineStyle* style = fixture.library.find(name);
        ASSERT_NE(style, nullptr) << name;
        EXPECT_EQ(style->source, "test_symbols.4d") << name;
    }
}

TEST(CustomisationFixture, TheDefinitionsCarryTheFeaturesTheyWereWrittenToHave)
{
    const a12::Customisation fixture = loadFixture();

    const LineStyle* kerb = fixture.library.find("TEST Dashed Kerb");
    ASSERT_NE(kerb, nullptr);
    EXPECT_EQ(kerb->units, StyleUnits::Paper);
    EXPECT_EQ(kerb->length, 4.0);
    // move draw move draw move: two dashes and the gaps between them.
    ASSERT_EQ(kerb->strokes.size(), 5u);
    EXPECT_EQ(kerb->strokes[2].op, StrokeOp::Move) << "the pen lifts: a gap";

    const LineStyle* main = fixture.library.find("TEST Water Main");
    ASSERT_NE(main, nullptr);
    EXPECT_EQ(main->units, StyleUnits::World);
    ASSERT_FALSE(main->strokes.empty());
    EXPECT_EQ(main->strokes[0].op, StrokeOp::Pen);
    EXPECT_EQ(main->strokes[0].pen, "blue");

    const LineStyle* gate = fixture.library.find("TEST Gate");
    ASSERT_NE(gate, nullptr);
    EXPECT_EQ(gate->units, StyleUnits::TwoPoint);
    EXPECT_EQ(gate->anchor2, katana::geometry::Point2(4.0, 0.0));
    EXPECT_EQ(gate->stretchMode, 2);
    EXPECT_EQ(gate->cycleMode, 2);

    const LineStyle* tree = fixture.library.find("TEST Tree");
    ASSERT_NE(tree, nullptr);
    EXPECT_FALSE(tree->atVertices) << "a symbol only because the mapfile draws it as one";

    const LineStyle* valve = fixture.library.find("TEST Valve");
    ASSERT_NE(valve, nullptr);
    ASSERT_EQ(valve->texts.size(), 1u);
    EXPECT_EQ(valve->texts[0].text, "V");
    EXPECT_EQ(valve->texts[0].widthFactor, 0.8);
    EXPECT_EQ(valve->texts[0].unnamed, (std::array<double, 3>{0.0, -0.1, 0.02}));

    const LineStyle* turn = fixture.library.find("TEST U Turn");
    ASSERT_NE(turn, nullptr);
    ASSERT_EQ(turn->strokes.size(), 3u);
    EXPECT_EQ(turn->strokes[2].op, StrokeOp::Arc);
    EXPECT_EQ(turn->strokes[2].radius, -0.5) << "the sign is kept as written";
}

TEST(CustomisationFixture, TheMapfileHasTheRulesAndGapsItWasWrittenToHave)
{
    const a12::Customisation fixture = loadFixture();
    // 7 map_data + 3 vertex_symbol_data + 1 string_attribute_data.
    EXPECT_EQ(fixture.map.size(), 11u);
    EXPECT_EQ(fixture.map.keys(),
              (std::vector<std::string>{"*", "1*", "2*", "AC*", "KB*", "PX*", "TR*", "WM*"}));
    EXPECT_EQ(fixture.map.stylesReferenced(),
              (std::vector<std::string>{"0", "TEST Dashed Kerb", "TEST Missing Symbol",
                                        "TEST Survey Mark", "TEST Tree", "TEST Water Main"}));
    // "0" is 12d's plain line, which no library defines; the missing symbol
    // is the rule written to name one nothing defines.
    EXPECT_EQ(fixture.unresolvedStyles(),
              (std::vector<std::string>{"0", "TEST Missing Symbol"}));

    // AC*: a plain line, a `mode vertex` symbol, and the attribute every code gets.
    const auto chamber = fixture.map.lookup("AC12");
    EXPECT_EQ(chamber.resolved.model, "TEST FURNITURE");
    EXPECT_EQ(chamber.resolved.linestyle, "0");
    ASSERT_TRUE(chamber.resolved.symbol.has_value());
    EXPECT_EQ(chamber.resolved.symbol->style, "TEST Survey Mark");
    EXPECT_EQ(chamber.resolved.symbol->size, 1.5);
    ASSERT_EQ(chamber.resolved.attributes.size(), 1u);
    EXPECT_EQ(chamber.resolved.attributes[0].name, "Source");

    // The two text codes differ only in colour.
    EXPECT_EQ(fixture.map.lookup("1A").resolved.linestyle, "0");
    EXPECT_EQ(fixture.map.lookup("2A").resolved.linestyle, "0");
    EXPECT_EQ(fixture.map.lookup("1A").resolved.colour, "orange");
    EXPECT_EQ(fixture.map.lookup("2A").resolved.colour, "red");

    // A code nothing names meets only the "*" rule.
    EXPECT_EQ(fixture.map.lookup("ZZ9").keys, (std::vector<std::string>{"*"}));

    // KB*'s colour is one no table knows; the others are standard.
    EXPECT_EQ(fixture.map.lookup("KB1").resolved.colour, "sui test purple");
    EXPECT_FALSE(a12::standardColour("sui test purple").has_value());
    EXPECT_TRUE(a12::standardColour("orange").has_value());
}

TEST(CustomisationFixture, TheMapfileIsUtf16WithAByteOrderMarkAs12dWritesThem)
{
    std::ifstream file(kFixture / "test_survey.mapfile", std::ios::binary);
    ASSERT_TRUE(file);
    std::ostringstream bytes;
    bytes << file.rdbuf();
    const std::string raw = bytes.str();
    ASSERT_GE(raw.size(), 2u);
    EXPECT_EQ(static_cast<unsigned char>(raw[0]), 0xFFu);
    EXPECT_EQ(static_cast<unsigned char>(raw[1]), 0xFEu);
}
