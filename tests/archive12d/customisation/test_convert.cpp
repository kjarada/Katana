// The converter of the older customisation formats
// (src/katana_archive12d/legacy/convert.hpp): style libraries and survey code
// files in, one Katana customisation out.
//
// Where the expectations come from, since a converter is easily "tested" by
// agreeing with itself:
//
// * The three files of tests/data/customisation are the fixtures of
//   tests/archive12d/data/customisation over again in the Katana format. They
//   were WRITTEN BY HAND, from the fixtures' own text and the format chapter
//   of docs/customisation.md, before the converter was first run on them, and
//   are never to be replaced by its output: they are what it is held to. And
//   since a file cannot show how it was made, what each twin holds is also
//   stated here, value by value, from the legacy fixtures' text
//   (TheTwinsHoldWhatTheLegacyFixturesSay): a twin regenerated from a
//   converter that lost a field would fail there.
// * The small inputs below are written here, and what each becomes is worked
//   out beside it from the rule being tested.
// * The reference customisation's figures are those of a census that uses none
//   of this program's code (tools/reference_census.py over the four files;
//   tools/customisation_census.py gives the same from a converted file).

#include <gtest/gtest.h>

#include <algorithm>
#include <array>
#include <chrono>
#include <filesystem>
#include <fstream>
#include <map>
#include <optional>
#include <sstream>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include "convert.hpp"
#include "katana/archive12d/customisation.hpp"
#include "katana/archive12d/style_library.hpp"
#include "katana/core/text_encoding.hpp"
#include "katana/entity/customisation.hpp"

namespace a12 = katana::archive12d;
using katana::core::ErrorCode;
using katana::entity::Color;
using katana::entity::Customisation;
using katana::entity::LineStyle;
using katana::entity::Stroke;
using katana::entity::StrokeOp;
using katana::entity::StrokeText;
using katana::entity::StyleUnits;
using katana::entity::SurveyAttribute;
using katana::entity::SurveyBreakline;
using katana::entity::SurveyRule;
using katana::entity::SurveySection;
using katana::geometry::Point2;

namespace {

const std::filesystem::path kLegacyFixtures =
    std::filesystem::path(KATANA_ARCHIVE12D_TEST_DATA) / "customisation";
const std::filesystem::path kTwins{KATANA_CUSTOMISATION_TEST_DATA};

std::string bytesOf(const std::filesystem::path& path)
{
    std::ifstream file(path, std::ios::binary);
    EXPECT_TRUE(file.good()) << "cannot open " << path.string();
    std::ostringstream buffer;
    buffer << file.rdbuf();
    return buffer.str();
}

a12::LegacyFile legacyFixture(const std::string& name)
{
    return {name, bytesOf(kLegacyFixtures / name)};
}

// A twin's text with its lines ended as the writer ends them. The files are
// committed with line feeds, but a checkout on Windows may hand them back with
// CRLF (git's core.autocrlf), and it is the layout that is being compared, not
// what a checkout did to it. No carriage return is otherwise in these files: a
// JSON string cannot hold a raw one.
std::string twinText(const std::string& name)
{
    std::string text = bytesOf(kTwins / name);
    std::erase(text, '\r');
    return text;
}

a12::ConvertOptions named(std::string name)
{
    a12::ConvertOptions options;
    options.name = std::move(name);
    return options;
}

a12::Conversion converted(const std::vector<a12::LegacyFile>& files,
                          const a12::ConvertOptions& options)
{
    auto conversion = a12::convertLegacyCustomisation(files, options);
    EXPECT_TRUE(conversion.ok()) << (conversion.ok() ? "" : conversion.error().describe());
    return conversion.ok() ? std::move(*conversion) : a12::Conversion{};
}

// ---- small inputs in the older formats -------------------------------------------------------

// A survey code file of the three sections these tests use. <map_file> is the
// root: the reader looks for that element and needs nothing around it.
std::string surveyCodes(const std::string& features, const std::string& symbols = {},
                        const std::string& texts = {})
{
    return "<map_file><map_data>" + features + "</map_data><vertex_symbol_data>" + symbols +
           "</vertex_symbol_data><vertex_textstyle_data>" + texts +
           "</vertex_textstyle_data></map_file>";
}

// One rule of <map_data>; `fields` is its elements after the key.
std::string feature(const std::string& key, const std::string& fields)
{
    return "<item><key>" + key + "</key>" + fields + "</item>";
}

std::string symbolRule(const std::string& key, const std::string& style,
                       const std::string& colour = {})
{
    return "<item><key>" + key + "</key><symbol_data><style>" + style + "</style><colour>" +
           colour + "</colour></symbol_data></item>";
}

std::string textRule(const std::string& key, const std::string& colour)
{
    return "<item><key>" + key + "</key><textstyle_data><colour>" + colour +
           "</colour></textstyle_data></item>";
}

const LineStyle& definition(const Customisation& customisation, std::string_view name)
{
    static const LineStyle none;
    const LineStyle* found = customisation.library.find(name);
    EXPECT_NE(found, nullptr) << "no definition \"" << name << "\"";
    return found == nullptr ? none : *found;
}

std::string failure(const std::vector<a12::LegacyFile>& files, const a12::ConvertOptions& options,
                    ErrorCode code)
{
    const auto conversion = a12::convertLegacyCustomisation(files, options);
    EXPECT_FALSE(conversion.ok()) << "the conversion was expected to fail";
    if (conversion.ok()) {
        return {};
    }
    EXPECT_EQ(conversion.error().code, code) << conversion.error().describe();
    return conversion.error().describe();
}

} // namespace

// ---- the fixtures and their hand-written twins ----------------------------------------------

namespace {

struct FixtureTwin {
    const char* legacy;
    const char* name;
    const char* twin;
    // Counted by hand from the legacy fixture; its head comment carries the
    // same counts.
    std::size_t definitions;
    std::size_t symbols;
    std::size_t rules;
    std::size_t keys;
};

constexpr std::array<FixtureTwin, 3> kFixtureTwins{{
    {"test_linestyles.4d", "test_linestyles", "test_linestyles.customisation.json", 3, 0, 0, 0},
    {"test_survey.mapfile", "test_survey", "test_survey.customisation.json", 0, 0, 11, 8},
    // Symbols every one, because the FILE is a symbol library: its name says so.
    {"test_symbols.4d", "test_symbols", "test_symbols.customisation.json", 4, 4, 0, 0},
}};

} // namespace

TEST(CustomisationConvert, EachLegacyFixtureConvertsToItsHandWrittenTwin)
{
    for (const FixtureTwin& fixture : kFixtureTwins) {
        SCOPED_TRACE(fixture.legacy);
        const std::string text = twinText(fixture.twin);
        const auto twin = katana::entity::customisationFromJson(text);
        ASSERT_TRUE(twin.ok()) << twin.error().describe();
        // The twin holds what the fixture's head comment counts, so that two
        // empty customisations could not pass for equal ones.
        EXPECT_EQ(twin->name, fixture.name);
        EXPECT_EQ(twin->library.size(), fixture.definitions);
        std::size_t symbols = 0;
        twin->library.forEach([&](const LineStyle& style) { symbols += style.symbol ? 1 : 0; });
        EXPECT_EQ(symbols, fixture.symbols);
        EXPECT_EQ(twin->map.size(), fixture.rules);
        EXPECT_EQ(twin->map.keys().size(), fixture.keys);

        const a12::Conversion conversion =
            converted({legacyFixture(fixture.legacy)}, named(fixture.name));
        // Whole: name, description, notice, sources, colours, linework,
        // automation, every definition and the rules in their order.
        EXPECT_TRUE(conversion.customisation == *twin);
        // And as text, which is what shows the difference when they differ.
        EXPECT_EQ(conversion.json, text);
    }
}

namespace {

Customisation twinOf(const std::string& name)
{
    auto read = katana::entity::customisationFromJson(twinText(name));
    EXPECT_TRUE(read.ok()) << name << ": " << (read.ok() ? "" : read.error().describe());
    return read.ok() ? std::move(*read) : Customisation{};
}

Stroke moveTo(double x, double y)
{
    Stroke stroke;
    stroke.op = StrokeOp::Move;
    stroke.point = Point2{x, y};
    return stroke;
}

Stroke drawTo(double x, double y)
{
    Stroke stroke = moveTo(x, y);
    stroke.op = StrokeOp::Draw;
    return stroke;
}

Stroke penOf(const std::string& colour)
{
    Stroke stroke;
    stroke.op = StrokeOp::Pen;
    stroke.pen = colour;
    return stroke;
}

} // namespace

TEST(CustomisationConvert, TheTwinsHoldWhatTheLegacyFixturesSay)
{
    // Every value below is read off the text of a legacy fixture
    // (tests/archive12d/data/customisation), command by command and element
    // by element - none is taken from a twin or from a conversion. A twin
    // that agreed with the converter and with nothing else would pass the
    // comparison above; it would not pass this.

    // ---- test_linestyles.4d ----
    const Customisation lines = twinOf("test_linestyles.customisation.json");
    // twoptstyle; xorigin1 0 yorigin1 0 xorigin2 4 yorigin2 0; stretch_mode 2
    // cycle_mode 2; move 0 0, draw 4 0, move 0 0, arc 4 0 90.
    const LineStyle& gate = definition(lines, "TEST Gate");
    EXPECT_EQ(gate.units, StyleUnits::TwoPoint);
    EXPECT_EQ(gate.group, "Test/Lines");
    EXPECT_EQ(gate.anchor1, (Point2{0.0, 0.0}));
    EXPECT_EQ(gate.anchor2, (Point2{4.0, 0.0}));
    EXPECT_EQ(gate.stretchMode, 2);
    EXPECT_EQ(gate.cycleMode, 2);
    EXPECT_EQ(gate.length, 0.0) << "no `length` in the block";
    EXPECT_FALSE(gate.atVertices);
    EXPECT_FALSE(gate.symbol) << "a linestyle library: its name holds no \"symbol\"";
    ASSERT_EQ(gate.strokes.size(), 4u);
    EXPECT_EQ(gate.strokes[1], drawTo(4.0, 0.0));
    EXPECT_EQ(gate.strokes[3].op, StrokeOp::Arc);
    EXPECT_EQ(gate.strokes[3].radius, 4.0);
    EXPECT_EQ(gate.strokes[3].startAngle, 0.0);
    EXPECT_EQ(gate.strokes[3].endAngle, 90.0);
    // paperstyle; length 4; move 0 0, draw 1.5 0, move 2 0, draw 3 0, move 4 0.
    const LineStyle& kerb = definition(lines, "TEST Dashed Kerb");
    EXPECT_EQ(kerb.units, StyleUnits::Paper);
    EXPECT_EQ(kerb.length, 4.0);
    EXPECT_EQ(kerb.strokes, (std::vector<Stroke>{moveTo(0.0, 0.0), drawTo(1.5, 0.0), moveTo(2.0, 0.0),
                                                 drawTo(3.0, 0.0), moveTo(4.0, 0.0)}));
    // worldstyle; length 6; colour "blue" - a pen, and a stroke - then
    // move 0 0, draw 4.5 0, move 5 -0.25, draw 5 0.25, move 6 0.
    const LineStyle& waterMain = definition(lines, "TEST Water Main");
    EXPECT_EQ(waterMain.units, StyleUnits::World);
    EXPECT_EQ(waterMain.group, "Test/Services");
    EXPECT_EQ(waterMain.length, 6.0);
    EXPECT_EQ(waterMain.strokes,
              (std::vector<Stroke>{penOf("blue"), moveTo(0.0, 0.0), drawTo(4.5, 0.0), moveTo(5.0, -0.25),
                                   drawTo(5.0, 0.25), moveTo(6.0, 0.0)}));

    // ---- test_symbols.4d ----
    const Customisation symbols = twinOf("test_symbols.customisation.json");
    // mode vertex; five strokes of a square, move 0 0.6, then
    // text "V" 0 0.5 "bottom-centre" "Arial" 0.8 0 -0.1 0.02: the angle, the
    // height, the justification, the font, the width factor and the three
    // numbers a text ends with.
    const LineStyle& valve = definition(symbols, "TEST Valve");
    EXPECT_TRUE(valve.atVertices);
    EXPECT_TRUE(valve.symbol);
    EXPECT_EQ(valve.group, "Test/Marks");
    ASSERT_EQ(valve.strokes.size(), 7u);
    EXPECT_EQ(valve.strokes[0], moveTo(-0.4, -0.4));
    EXPECT_EQ(valve.strokes[3], drawTo(-0.4, 0.4));
    EXPECT_EQ(valve.strokes[5], moveTo(0.0, 0.6));
    EXPECT_EQ(valve.strokes[6].op, StrokeOp::Text);
    ASSERT_EQ(valve.texts.size(), 1u);
    ASSERT_EQ(valve.strokes[6].text, 0u);
    StrokeText letter;
    letter.text = "V";
    letter.angle = 0.0;
    letter.height = 0.5;
    letter.justify = "bottom-centre";
    letter.font = "Arial";
    letter.widthFactor = 0.8;
    letter.unnamed = {0.0, -0.1, 0.02};
    EXPECT_EQ(valve.texts[0], letter);
    // No `mode vertex`, and a symbol all the same: the file is a symbol
    // library. colour "green", move 0 0, circle 1, move -0.7 -0.7, draw 0.7 0.7.
    const LineStyle& tree = definition(symbols, "TEST Tree");
    EXPECT_FALSE(tree.atVertices);
    EXPECT_TRUE(tree.symbol);
    EXPECT_EQ(tree.group, "Test/Vegetation");
    ASSERT_EQ(tree.strokes.size(), 5u);
    EXPECT_EQ(tree.strokes[0], penOf("green"));
    EXPECT_EQ(tree.strokes[2].op, StrokeOp::Circle);
    EXPECT_EQ(tree.strokes[2].radius, 1.0);
    EXPECT_EQ(tree.strokes[4], drawTo(0.7, 0.7));
    // arc -0.5 -90 90: the radius is negative, and stays so.
    const LineStyle& turn = definition(symbols, "TEST U Turn");
    EXPECT_TRUE(turn.atVertices);
    ASSERT_EQ(turn.strokes.size(), 3u);
    EXPECT_EQ(turn.strokes[2].op, StrokeOp::Arc);
    EXPECT_EQ(turn.strokes[2].radius, -0.5);
    EXPECT_EQ(turn.strokes[2].startAngle, -90.0);
    EXPECT_EQ(turn.strokes[2].endAngle, 90.0);
    // Six strokes, the last circle 0.3.
    const LineStyle& mark = definition(symbols, "TEST Survey Mark");
    EXPECT_TRUE(mark.atVertices);
    ASSERT_EQ(mark.strokes.size(), 6u);
    EXPECT_EQ(mark.strokes[5].op, StrokeOp::Circle);
    EXPECT_EQ(mark.strokes[5].radius, 0.3);

    // ---- test_survey.mapfile: seven of <map_data>, three of
    // <vertex_symbol_data>, one of <string_attribute_data>, in that order ----
    const Customisation survey = twinOf("test_survey.customisation.json");
    const std::vector<SurveyRule>& rules = survey.map.rules();
    ASSERT_EQ(rules.size(), 11u);
    // The first <item> whole: key WM*, model, colour, breakline Line,
    // linestyle, weight 0, comment, group.
    SurveyRule water;
    water.key = "WM*";
    water.section = SurveySection::Map;
    water.model = "TEST SERVICES";
    water.colour = "blue";
    water.breakline = SurveyBreakline::Line;
    water.linestyle = "TEST Water Main";
    water.weight = "0";
    water.comment = "[WM*] Water main";
    water.group = "TEST - SERVICES";
    EXPECT_EQ(rules[0], water);
    // KB*: the colour no table knows, written as it is.
    EXPECT_EQ(rules[1].key, "KB*");
    EXPECT_EQ(rules[1].colour, "sui test purple");
    EXPECT_EQ(rules[1].linestyle, "TEST Dashed Kerb");
    // AC* is <breakline>Point</breakline> with <weight>0</weight> ...
    EXPECT_EQ(rules[2].key, "AC*");
    EXPECT_EQ(rules[2].breakline, SurveyBreakline::Point);
    EXPECT_EQ(rules[2].weight, "0");
    // ... and 1* has neither element: nothing said is not "0" and not a line.
    EXPECT_EQ(rules[4].key, "1*");
    EXPECT_EQ(rules[4].model, "TEST TEXT");
    EXPECT_EQ(rules[4].colour, "orange");
    EXPECT_FALSE(rules[4].breakline.has_value());
    EXPECT_TRUE(rules[4].weight.empty());
    EXPECT_EQ(rules[4].comment, "Text 1.5 mm");
    // PX* has a breakline and no <weight>.
    EXPECT_EQ(rules[6].key, "PX*");
    EXPECT_EQ(rules[6].breakline, SurveyBreakline::Point);
    EXPECT_TRUE(rules[6].weight.empty());
    // The first <vertex_symbol_data> item whole: style, colour white, size
    // 1.5, an EMPTY <rotation/> (nothing said), <hide>no</hide>, comment.
    SurveyRule chamber;
    chamber.key = "AC*";
    chamber.section = SurveySection::VertexSymbol;
    chamber.symbol.emplace();
    chamber.symbol->style = "TEST Survey Mark";
    chamber.symbol->colour = "white";
    chamber.symbol->size = 1.5;
    chamber.hide = false;
    chamber.comment = "[AC*] Access chamber";
    EXPECT_EQ(rules[7], chamber);
    EXPECT_EQ(rules[8].key, "TR*");
    ASSERT_TRUE(rules[8].symbol.has_value());
    EXPECT_EQ(rules[8].symbol->style, "TEST Tree");
    EXPECT_EQ(rules[8].symbol->size, 3.0);
    EXPECT_EQ(rules[8].hide, std::optional<bool>{false});
    // PX* has no <hide>: not said, which is not "no".
    EXPECT_EQ(rules[9].key, "PX*");
    ASSERT_TRUE(rules[9].symbol.has_value());
    EXPECT_EQ(rules[9].symbol->style, "TEST Missing Symbol");
    EXPECT_EQ(rules[9].symbol->colour, "yellow");
    EXPECT_EQ(rules[9].symbol->size, 1.0);
    EXPECT_FALSE(rules[9].hide.has_value());
    // The one <string_attribute_data> item whole: a <text> attribute.
    SurveyRule every;
    every.key = "*";
    every.section = SurveySection::StringAttribute;
    every.attributes = {SurveyAttribute{"text", "Source", "Katana test fixture"}};
    every.comment = "Every code gets this";
    EXPECT_EQ(rules[10], every);
}

TEST(CustomisationConvert, TheTwinsAreInTheWritersLayoutByteForByte)
{
    // They were written to the eleven rules of docs/customisation.md,
    // "Layout", so reading one and writing it again gives its own bytes
    // (twinText says what is done about a checkout's line ends).
    for (const FixtureTwin& fixture : kFixtureTwins) {
        SCOPED_TRACE(fixture.twin);
        const std::string text = twinText(fixture.twin);
        const auto twin = katana::entity::customisationFromJson(text);
        ASSERT_TRUE(twin.ok()) << twin.error().describe();
        const auto written = katana::entity::customisationToJson(*twin);
        ASSERT_TRUE(written.ok()) << written.error().describe();
        EXPECT_EQ(*written, text);
    }
}

TEST(CustomisationConvert, TheThreeFixturesTogetherAreOneCustomisationOfSevenDefinitionsAndElevenRules)
{
    // In name order, as a folder of them was loaded: the linestyles, the
    // survey codes, the symbols.
    const a12::Conversion conversion =
        converted({legacyFixture("test_linestyles.4d"), legacyFixture("test_survey.mapfile"),
                   legacyFixture("test_symbols.4d")},
                  named("fixture"));
    const a12::ConvertReport& report = conversion.report;
    EXPECT_EQ(report.files, 3u);
    EXPECT_EQ(report.definitions, 7u); // 3 linestyles + 4 symbols, no name twice
    EXPECT_EQ(report.symbols, 4u);
    EXPECT_EQ(report.linestyles, 3u);
    EXPECT_EQ(report.atVertices, 3u); // TEST Survey Mark, TEST U Turn, TEST Valve
    EXPECT_EQ(report.groups, 4u);     // Test/Lines, Test/Marks, Test/Services, Test/Vegetation
    // 5 + 4 + 6 in the linestyle fixture (a `colour` is a stroke), 6 + 5 + 3 + 7
    // in the symbol fixture.
    EXPECT_EQ(report.strokes, 36u);
    EXPECT_EQ(report.rules, 11u);
    EXPECT_EQ(report.keys, 8u);
    // The survey fixture's own comment: of the six names it references, "0"
    // and "TEST Missing Symbol" are defined by neither library.
    EXPECT_EQ(report.unresolvedReferences,
              (std::vector<std::string>{"0", "TEST Missing Symbol"}));
    // "sui test purple" was invented for the fixture as a colour no table
    // knows; nothing gives it an RGB, so it stays a name.
    EXPECT_TRUE(report.coloursResolved.empty());
    EXPECT_EQ(report.coloursUnresolved, (std::vector<std::string>{"sui test purple"}));
    EXPECT_TRUE(conversion.customisation.colours.empty());
    EXPECT_TRUE(report.warnings.empty());

    // Every definition is its twin's, under the one name they now share.
    for (const char* twinFile :
         {"test_linestyles.customisation.json", "test_symbols.customisation.json"}) {
        const auto twin = katana::entity::customisationFromJson(twinText(twinFile));
        ASSERT_TRUE(twin.ok()) << twin.error().describe();
        twin->library.forEach([&](const LineStyle& expected) {
            LineStyle renamed = expected;
            renamed.source = "fixture";
            EXPECT_EQ(definition(conversion.customisation, expected.name), renamed)
                << expected.name;
        });
    }
    const auto survey =
        katana::entity::customisationFromJson(twinText("test_survey.customisation.json"));
    ASSERT_TRUE(survey.ok()) << survey.error().describe();
    EXPECT_TRUE(conversion.customisation.map == survey->map);
}

// ---- what a conversion makes of the files ---------------------------------------------------

TEST(CustomisationConvert, EveryDefinitionComesFromTheCustomisationAndIsASymbolWhenItsFileSaysSo)
{
    const a12::Conversion conversion = converted(
        {{"site_lines.4d", "paperstyle \"Fence\" { group \"Site\" length 2 move 0 0 draw 1 0 }"},
         // "symbol" anywhere in the name, in any case, makes a symbol library.
         {"Site_SYMBOLS_v2.4d", "worldstyle \"Peg\" { mode vertex move 0 0 circle 0.5 }\n"
                                "worldstyle \"Tree\" { move 0 0 circle 1 }"}},
        named("Site"));
    const Customisation& site = conversion.customisation;
    EXPECT_EQ(site.name, "Site");
    EXPECT_EQ(definition(site, "Fence").source, "Site");
    EXPECT_EQ(definition(site, "Peg").source, "Site");
    EXPECT_FALSE(definition(site, "Fence").symbol);
    EXPECT_TRUE(definition(site, "Peg").symbol);
    EXPECT_TRUE(definition(site, "Tree").symbol) << "listed as a symbol without `mode vertex`";
    EXPECT_FALSE(definition(site, "Tree").atVertices);
    // What a conversion does not say, it leaves unsaid: no sources (the file
    // is its own one source), no linework codes and no automation switches.
    EXPECT_TRUE(site.description.empty());
    EXPECT_TRUE(site.notice.empty());
    EXPECT_TRUE(site.sources.empty());
    EXPECT_FALSE(site.basedOn.has_value());
    EXPECT_FALSE(site.linework.has_value());
    EXPECT_FALSE(site.automation.has_value());
    // The text is the customisation, and nothing else.
    const auto read = katana::entity::customisationFromJson(conversion.json);
    ASSERT_TRUE(read.ok()) << read.error().describe();
    EXPECT_TRUE(*read == site);
    EXPECT_EQ(conversion.report.bytes, conversion.json.size());
    EXPECT_EQ(conversion.json.find('\r'), std::string::npos);
    EXPECT_TRUE(conversion.json.ends_with("}\n"));
}

TEST(CustomisationConvert, TheDescriptionIsCarried)
{
    a12::ConvertOptions options = named("Site");
    options.description = "The site's own codes";
    const a12::Conversion conversion =
        converted({{"lines.4d", "worldstyle \"A\" { move 0 0 draw 1 0 }"}}, options);
    EXPECT_EQ(conversion.customisation.description, "The site's own codes");
}

TEST(CustomisationConvert, ALaterLibraryWinsADefinitionWhicheverOrderTheyComeIn)
{
    // The same two libraries both ways round. "Shared" is one stroke long in
    // the first and two in the second, and only the second is a symbol library.
    const a12::LegacyFile lines{"a_lines.4d",
                                "worldstyle \"Shared\" { group \"Lines\" move 0 0 }\n"
                                "worldstyle \"Only Lines\" { move 0 0 }"};
    const a12::LegacyFile symbols{"b_symbols.4d",
                                  "worldstyle \"Shared\" { group \"Symbols\" move 1 1 draw 2 2 }\n"
                                  "worldstyle \"Only Symbols\" { move 0 0 }"};

    const a12::Conversion symbolsLast = converted({lines, symbols}, named("Order"));
    EXPECT_EQ(symbolsLast.report.definitions, 3u) << "one name of the four is defined twice";
    EXPECT_EQ(definition(symbolsLast.customisation, "Shared").group, "Symbols");
    EXPECT_EQ(definition(symbolsLast.customisation, "Shared").strokes.size(), 2u);
    EXPECT_TRUE(definition(symbolsLast.customisation, "Shared").symbol);
    EXPECT_EQ(symbolsLast.report.symbols, 2u);

    const a12::Conversion linesLast = converted({symbols, lines}, named("Order"));
    EXPECT_EQ(linesLast.report.definitions, 3u);
    EXPECT_EQ(definition(linesLast.customisation, "Shared").group, "Lines");
    EXPECT_EQ(definition(linesLast.customisation, "Shared").strokes.size(), 1u);
    EXPECT_FALSE(definition(linesLast.customisation, "Shared").symbol)
        << "the winner brings its kind with it";
    EXPECT_EQ(linesLast.report.symbols, 1u);
}

TEST(CustomisationConvert, ADefinitionThatLostItsPlaceIsNamedWithWhatWentAndHowTheRulesUseIt)
{
    // Two libraries of which one is a symbol library by its name. "Shared" is
    // in both and differs; "Same" is in both, block for block the same;
    // "Twice" is given twice by the first alone, and differs.
    const a12::LegacyFile lines{"a_lines.4d",
                                "worldstyle \"Shared\" { group \"Lines\" move 0 0 draw 4 0 }\n"
                                "worldstyle \"Same\" { move 0 0 draw 1 0 }\n"
                                "worldstyle \"Twice\" { move 0 0 draw 1 0 }\n"
                                "worldstyle \"Twice\" { move 0 0 draw 2 0 }\n"
                                "worldstyle \"Only Lines\" { move 0 0 }"};
    const a12::LegacyFile symbols{"b_symbols.4d",
                                  "worldstyle \"Shared\" { mode vertex move 1 1 circle 2 }\n"
                                  "worldstyle \"Same\" { move 0 0 draw 1 0 }\n"
                                  "worldstyle \"Only Symbols\" { move 0 0 }"};
    // Two rules draw a line with "Shared" and none places it as a symbol; one
    // draws a line with "Same" and one places it.
    const a12::LegacyFile codes{
        "codes.mapfile",
        surveyCodes(feature("K*", "<linestyle>Shared</linestyle>") +
                        feature("L*", "<linestyle>Shared</linestyle>") +
                        feature("M*", "<linestyle>Same</linestyle>"),
                    symbolRule("S*", "Same"))};
    using Replaced = a12::ReplacedDefinition;

    // The symbol library last: it keeps both names it shares.
    const a12::Conversion symbolsLast = converted({lines, codes, symbols}, named("Order"));
    EXPECT_EQ(symbolsLast.report.definitions, 5u);
    EXPECT_EQ(symbolsLast.report.symbols, 3u);
    // In the order they went: "Twice" while the first library was read, then
    // the two the second replaced, as it gives them.
    EXPECT_EQ(symbolsLast.report.replaced,
              (std::vector<Replaced>{
                  Replaced{"Twice", "a_lines.4d", "a_lines.4d", false, false, true, 0, 0},
                  Replaced{"Shared", "b_symbols.4d", "a_lines.4d", true, false, true, 2, 0},
                  Replaced{"Same", "b_symbols.4d", "a_lines.4d", true, false, false, 1, 1}}));
    // Kept as symbols, and rules draw lines with them: said, each once.
    EXPECT_EQ(symbolsLast.report.warnings,
              (std::vector<std::string>{
                  "b_symbols.4d: \"Shared\" is kept as a symbol in place of the linestyle of "
                  "a_lines.4d; rules naming it as their linestyle: 2 (the library given last is "
                  "the one kept)",
                  "b_symbols.4d: \"Same\" is kept as a symbol in place of the linestyle of "
                  "a_lines.4d; rules naming it as their linestyle: 1 (the library given last is "
                  "the one kept)"}));
    const std::string said = a12::toText(symbolsLast.report);
    EXPECT_NE(said.find("strokes=8\n" // of the five kept: 2 + 2 + 2 + 1 + 1
                        "definitions_replaced=3\n"
                        "definition_replaced=\"Twice\" kept=\"a_lines.4d\" kept_as=linestyle "
                        "dropped=\"a_lines.4d\" dropped_as=linestyle differs=yes "
                        "linestyle_rules=0 symbol_rules=0\n"
                        "definition_replaced=\"Shared\" kept=\"b_symbols.4d\" kept_as=symbol "
                        "dropped=\"a_lines.4d\" dropped_as=linestyle differs=yes "
                        "linestyle_rules=2 symbol_rules=0\n"
                        "definition_replaced=\"Same\" kept=\"b_symbols.4d\" kept_as=symbol "
                        "dropped=\"a_lines.4d\" dropped_as=linestyle differs=no "
                        "linestyle_rules=1 symbol_rules=1\n"
                        "rules=4\n"),
              std::string::npos)
        << said;

    // The linestyle library last: "Shared" is the linestyle the rules draw
    // with and nothing is astray there; "Same" is a linestyle that one rule
    // places as a symbol.
    const a12::Conversion linesLast = converted({symbols, codes, lines}, named("Order"));
    EXPECT_EQ(linesLast.report.symbols, 1u);
    EXPECT_EQ(linesLast.report.replaced,
              (std::vector<Replaced>{
                  Replaced{"Shared", "a_lines.4d", "b_symbols.4d", false, true, true, 2, 0},
                  Replaced{"Same", "a_lines.4d", "b_symbols.4d", false, true, false, 1, 1},
                  Replaced{"Twice", "a_lines.4d", "a_lines.4d", false, false, true, 0, 0}}));
    EXPECT_EQ(linesLast.report.warnings,
              (std::vector<std::string>{
                  "a_lines.4d: \"Same\" is kept as a linestyle in place of the symbol of "
                  "b_symbols.4d; rules naming it as their symbol: 1 (the library given last is "
                  "the one kept)"}));

    // Nothing replaced is nothing said.
    const a12::Conversion alone = converted({symbols, codes}, named("Order"));
    EXPECT_TRUE(alone.report.replaced.empty());
    EXPECT_TRUE(alone.report.warnings.empty());
    EXPECT_NE(a12::toText(alone.report).find("definitions_replaced=0\nrules=4\n"),
              std::string::npos);

    // It is named as the FILES give it - they are what the person converting
    // has to look at - though the customisation holds it without its word.
    a12::ConvertOptions strip = named("Order");
    strip.stripLeadingWords = {"ACME"};
    const a12::Conversion stripped =
        converted({{"a.4d", "worldstyle \"ACME Gate\" { move 0 0 }"},
                   {"b.4d", "worldstyle \"ACME Gate\" { move 0 0 }"},
                   {"codes.mapfile", surveyCodes({}, symbolRule("G*", "ACME Gate"))}},
                  strip);
    EXPECT_EQ(stripped.report.replaced,
              (std::vector<Replaced>{Replaced{"ACME Gate", "b.4d", "a.4d", false, false, false, 0, 1}}));
    EXPECT_TRUE(stripped.customisation.library.contains("Gate"));
    // Nothing is said of it beyond that. A rule places it as a symbol and it
    // is a linestyle - but it was one in both files, so no order of them
    // would have kept anything else; and a library whose every definition
    // replaced one still gave definitions.
    EXPECT_TRUE(stripped.report.warnings.empty());
}

TEST(CustomisationConvert, AStyleLibraryHandsBackEachDefinitionItReplacedAsItWas)
{
    // What the report above is made from: the reader counted what it
    // replaced, and a count does not say which.
    const auto first = a12::readStyleLibrary(
        "worldstyle \"A\" { group \"One\" move 0 0 draw 1 0 }\n"
        "worldstyle \"B\" { move 0 0 }\n"
        "worldstyle \"A\" { group \"Two\" move 0 0 draw 2 0 }\n",
        "lines.4d");
    ASSERT_TRUE(first.ok()) << first.error().describe();
    EXPECT_EQ(first->replaced, 1u);
    ASSERT_EQ(first->replacedDefinitions.size(), 1u);
    EXPECT_EQ(first->replacedDefinitions[0].name, "A");
    EXPECT_EQ(first->replacedDefinitions[0].group, "One");
    EXPECT_EQ(first->replacedDefinitions[0].strokes.back().point.x, 1.0);
    ASSERT_NE(first->library.find("A"), nullptr);
    EXPECT_EQ(first->library.find("A")->group, "Two") << "the later block is the one kept";

    // Into a library already loaded: what THIS text replaced, with the file
    // and the kind it had - and nothing for the name that is new.
    const auto second = a12::readStyleLibraryInto(
        first->library,
        "worldstyle \"C\" { move 0 0 }\n"
        "worldstyle \"A\" { group \"Three\" move 0 0 }\n",
        "more_symbols.4d");
    ASSERT_TRUE(second.ok()) << second.error().describe();
    EXPECT_EQ(second->replaced, 1u);
    ASSERT_EQ(second->replacedDefinitions.size(), 1u);
    EXPECT_EQ(second->replacedDefinitions[0].group, "Two");
    EXPECT_EQ(second->replacedDefinitions[0].source, "lines.4d");
    EXPECT_FALSE(second->replacedDefinitions[0].symbol);
    ASSERT_NE(second->library.find("A"), nullptr);
    EXPECT_EQ(second->library.find("A")->group, "Three");
    EXPECT_TRUE(second->library.find("A")->symbol);

    // A text that replaces nothing hands back nothing.
    const auto none = a12::readStyleLibrary("worldstyle \"A\" { move 0 0 }", "lines.4d");
    ASSERT_TRUE(none.ok()) << none.error().describe();
    EXPECT_EQ(none->replaced, 0u);
    EXPECT_TRUE(none->replacedDefinitions.empty());
}

TEST(CustomisationConvert, AnEarlierSurveyCodeFileWinsAFieldBecauseItsRulesComeFirst)
{
    const a12::LegacyFile detail{
        "detail.mapfile",
        surveyCodes(feature("KB*", "<model>ROADS</model>") + feature("FN*", "<model>FENCES</model>"))};
    const a12::LegacyFile names{
        "names.mapfile",
        surveyCodes(feature("KB*", "<model>NAMES</model><colour>red</colour>"))};

    const a12::Conversion detailFirst = converted({detail, names}, named("Order"));
    const std::vector<SurveyRule>& rules = detailFirst.customisation.map.rules();
    ASSERT_EQ(rules.size(), 3u);
    EXPECT_EQ(rules[0].model, "ROADS");
    EXPECT_EQ(rules[1].model, "FENCES");
    EXPECT_EQ(rules[2].model, "NAMES");
    EXPECT_EQ(detailFirst.customisation.map.lookup("KB1").resolved.model, "ROADS");
    EXPECT_EQ(detailFirst.customisation.map.lookup("KB1").resolved.colour, "red")
        << "what only the later file says still reaches the code";

    const a12::Conversion namesFirst = converted({names, detail}, named("Order"));
    EXPECT_EQ(namesFirst.customisation.map.rules()[0].model, "NAMES");
    EXPECT_EQ(namesFirst.customisation.map.lookup("KB1").resolved.model, "NAMES");
}

// ---- --strip-leading-word ---------------------------------------------------------------------

TEST(CustomisationConvert, ALeadingWordIsTakenOffNamesGroupPathsAndWhatTheRulesName)
{
    const a12::LegacyFile lines{
        "lines.4d",
        "worldstyle \"ACME Kerb\" { group \"ACME Roads/Kerbs\" move 0 0 draw 1 0 }\n"
        // The word and nothing more is the name itself, and the group too.
        "worldstyle \"ACME\" { group \"ACME\" move 0 0 draw 2 0 }\n"
        // Not at the front.
        "worldstyle \"Big ACME Kerb\" { group \"Roads/ACME Kerbs\" move 0 0 draw 3 0 }\n"
        // Another case is another word; its group has the SECOND word given.
        "worldstyle \"acme Kerb\" { group \"VND Old/Stuff\" move 0 0 draw 4 0 }\n"
        // A longer word is not the word; two blanks after one both go.
        "worldstyle \"ACMEX Kerb\" { group \"ACME  Two/Blanks\" move 0 0 draw 5 0 }\n"
        // The word with blanks after it and nothing beyond them is still
        // nothing but the word, and is kept as written.
        "worldstyle \"Solo\" { group \"ACME  \" move 0 0 draw 6 0 }\n"};
    const a12::LegacyFile symbols{
        "symbols.4d", "worldstyle \"ACME Mark\" { group \"ACME Marks\" mode vertex move 0 0 }\n"};
    const a12::LegacyFile codes{
        "codes.mapfile",
        surveyCodes(feature("KB*", "<linestyle>ACME Kerb</linestyle><group>ACME ROADS - KERBS</group>") +
                        feature("BK*", "<linestyle>Big ACME Kerb</linestyle><group>ROADS ACME</group>") +
                        // Defined under neither name: it loses the word and stays unresolved.
                        feature("ZZ*", "<linestyle>ACME Nowhere</linestyle>") +
                        feature("PL*", "<linestyle>0</linestyle>"),
                    symbolRule("MK*", "ACME Mark"))};
    a12::ConvertOptions options = named("Site");
    options.stripLeadingWords = {"ACME", "VND"};

    const a12::Conversion conversion = converted({lines, codes, symbols}, options);
    const Customisation& site = conversion.customisation;
    EXPECT_EQ(site.library.names(),
              (std::vector<std::string>{"ACME", "ACMEX Kerb", "Big ACME Kerb", "Kerb", "Mark",
                                        "Solo", "acme Kerb"}));
    EXPECT_EQ(definition(site, "Solo").group, "ACME  ");
    EXPECT_EQ(definition(site, "Kerb").group, "Roads/Kerbs");
    EXPECT_EQ(definition(site, "Kerb").strokes.back().point.x, 1.0) << "it is \"ACME Kerb\"";
    EXPECT_EQ(definition(site, "ACME").group, "ACME");
    EXPECT_EQ(definition(site, "Big ACME Kerb").group, "Roads/ACME Kerbs");
    EXPECT_EQ(definition(site, "acme Kerb").group, "Old/Stuff");
    EXPECT_EQ(definition(site, "ACMEX Kerb").group, "Two/Blanks");
    EXPECT_EQ(definition(site, "Mark").group, "Marks");
    EXPECT_TRUE(definition(site, "Mark").symbol);

    const std::vector<SurveyRule>& rules = site.map.rules();
    ASSERT_EQ(rules.size(), 5u);
    EXPECT_EQ(rules[0].linestyle, "Kerb") << "the rule still names the definition it named";
    EXPECT_EQ(rules[0].group, "ROADS - KERBS");
    EXPECT_EQ(rules[1].linestyle, "Big ACME Kerb");
    EXPECT_EQ(rules[1].group, "ROADS ACME");
    EXPECT_EQ(rules[2].linestyle, "Nowhere");
    EXPECT_EQ(rules[3].linestyle, "0");
    ASSERT_TRUE(rules[4].symbol.has_value());
    EXPECT_EQ(rules[4].symbol->style, "Mark");

    const a12::ConvertReport& report = conversion.report;
    EXPECT_EQ(report.namesRenamed, 2u);      // ACME Kerb, ACME Mark
    EXPECT_EQ(report.groupsRenamed, 4u);     // Kerb's, acme Kerb's, ACMEX Kerb's, Mark's
    EXPECT_EQ(report.referencesRenamed, 3u); // KB*, ZZ*, MK*
    EXPECT_EQ(report.ruleGroupsRenamed, 1u); // KB*
    EXPECT_EQ(report.unresolvedReferences, (std::vector<std::string>{"0", "Nowhere"}));
}

TEST(CustomisationConvert, WithoutAWordToStripNothingIsRenamed)
{
    const a12::Conversion conversion = converted(
        {{"lines.4d", "worldstyle \"ACME Kerb\" { group \"ACME Roads\" move 0 0 draw 1 0 }"},
         {"codes.mapfile",
          surveyCodes(feature("KB*", "<linestyle>ACME Kerb</linestyle><group>ACME ROADS</group>"))}},
        named("Site"));
    EXPECT_EQ(definition(conversion.customisation, "ACME Kerb").group, "ACME Roads");
    EXPECT_EQ(conversion.customisation.map.rules()[0].linestyle, "ACME Kerb");
    EXPECT_EQ(conversion.customisation.map.rules()[0].group, "ACME ROADS");
    EXPECT_EQ(conversion.report.namesRenamed, 0u);
    EXPECT_EQ(conversion.report.groupsRenamed, 0u);
    EXPECT_EQ(conversion.report.referencesRenamed, 0u);
    EXPECT_EQ(conversion.report.ruleGroupsRenamed, 0u);
    EXPECT_TRUE(conversion.report.unresolvedReferences.empty());
}

TEST(CustomisationConvert, ARenameThatWouldGiveTwoDefinitionsOneNameIsRefusedNamingBoth)
{
    a12::ConvertOptions options = named("Site");
    options.stripLeadingWords = {"ACME"};
    const std::string said =
        failure({{"lines.4d", "worldstyle \"Kerb\" { move 0 0 draw 1 0 }\n"
                              "worldstyle \"ACME Kerb\" { move 0 0 draw 2 0 }"}},
                options, ErrorCode::InvalidArgument);
    EXPECT_NE(said.find("\"ACME Kerb\" and \"Kerb\" would both be \"Kerb\""), std::string::npos)
        << said;
}

TEST(CustomisationConvert, ARenameThatWouldMakeARuleNameAnotherDefinitionIsRefused)
{
    a12::ConvertOptions options = named("Site");
    options.stripLeadingWords = {"ACME"};
    // "ACME Tree" is defined nowhere; without its word it would be "Tree",
    // which is a definition the rule never named.
    const std::string stripped =
        failure({{"lines.4d", "worldstyle \"Tree\" { move 0 0 draw 1 0 }"},
                 {"codes.mapfile", surveyCodes(feature("OK*", "<linestyle>Tree</linestyle>") +
                                               feature("TR*", "<linestyle>ACME Tree</linestyle>"))}},
                options, ErrorCode::InvalidArgument);
    EXPECT_NE(stripped.find("codes[1] \"TR*\": \"ACME Tree\" is the name of no definition"),
              std::string::npos)
        << stripped;
    EXPECT_NE(stripped.find("would make it name \"Tree\""), std::string::npos) << stripped;

    // The other way round: "Post" is defined nowhere, and "ACME Post" would
    // come to be called that.
    const std::string met =
        failure({{"lines.4d", "worldstyle \"ACME Post\" { move 0 0 draw 1 0 }"},
                 {"codes.mapfile", surveyCodes({}, symbolRule("PO*", "Post"))}},
                options, ErrorCode::InvalidArgument);
    EXPECT_NE(met.find("codes[0] \"PO*\": \"Post\" is the name of no definition"),
              std::string::npos)
        << met;
    EXPECT_NE(met.find("would make it name \"ACME Post\""), std::string::npos) << met;
}

// ---- --remove-word ----------------------------------------------------------------------------

TEST(CustomisationConvert, AWordIsTakenOutOfRuleCommentsAsAWholeWordWithTheBlanksAroundIt)
{
    const a12::LegacyFile codes{
        "codes.mapfile",
        surveyCodes(feature("A*", "<comment>Kerb to ACME standard</comment>") +
                    feature("B*", "<comment>ACME kerb</comment>") +
                    feature("C*", "<comment>kerb ACME</comment>") +
                    feature("D*", "<comment>kerb ACME, top</comment>") +
                    feature("E*", "<comment>kerb (ACME top)</comment>") +
                    // Inside a longer word it is not the word.
                    feature("F*", "<comment>kerb ACMEX and XACME and ACME_2</comment>") +
                    feature("G*", "<comment>ACME kerb ACME top ACME</comment>") +
                    // Blanks nothing was taken from are left as written.
                    feature("H*", "<comment>kerb  top</comment>") +
                    // Only comments: a group keeps the word unless it is stripped.
                    feature("I*", "<group>ROADS ACME</group><model>ACME</model>"))};
    a12::ConvertOptions options = named("Site");
    options.removeWords = {"ACME"};

    const a12::Conversion conversion = converted({codes}, options);
    const std::vector<SurveyRule>& rules = conversion.customisation.map.rules();
    ASSERT_EQ(rules.size(), 9u);
    EXPECT_EQ(rules[0].comment, "Kerb to standard");
    EXPECT_EQ(rules[1].comment, "kerb");
    EXPECT_EQ(rules[2].comment, "kerb");
    EXPECT_EQ(rules[3].comment, "kerb, top");
    EXPECT_EQ(rules[4].comment, "kerb (top)");
    EXPECT_EQ(rules[5].comment, "kerb ACMEX and XACME and ACME_2");
    EXPECT_EQ(rules[6].comment, "kerb top");
    EXPECT_EQ(rules[7].comment, "kerb  top");
    EXPECT_EQ(rules[8].group, "ROADS ACME");
    EXPECT_EQ(rules[8].model, "ACME");
    EXPECT_EQ(conversion.report.commentsChanged, 6u); // A to E and G
}

// ---- --colours and the plot pens ----------------------------------------------------------------

namespace {

// A colour table as such files are: R G B, the table's own index, the name in
// quotes, then whatever else the line holds.
const a12::LegacyFile kColourTable{
    "colours.4d", "// a comment line, and a blank one below\n"
                  "\n"
                  "0 112 255 21 \"Site_Blue\" 21 -9 \"x\" // (0.25)\n"
                  "\t10\t20\t30\t22\t\"site-brown\"\n"
                  "1 2 3 1 \"red\" 1\n"
                  "40 50 60 23 \"site text\"\n"
                  "70 80 90 24 \"pen 018\"\n"
                  "70 80 91 25 \"Pen_7\"\n"
                  "70 80 92 26 \"pen 12a\"\n"
                  "70 80 93 27 \"pen 12ab\"\n"
                  "70 80 94 28 \"pencil\"\n"};

} // namespace

TEST(CustomisationConvert, TheColourTableGivesAColourToTheNamesTheStandardOnesLack)
{
    const a12::LegacyFile codes{
        "codes.mapfile",
        surveyCodes(feature("A*", "<colour>site blue</colour>") +
                        // The same name by another spelling: one entry, as first written.
                        feature("B*", "<colour>SITE_BLUE</colour>") +
                        // A standard name is never taken from the table, whatever it says.
                        feature("C*", "<colour>Red</colour>") +
                        // Not in the table.
                        feature("D*", "<colour>site green</colour>"),
                    symbolRule("S*", "Peg", "Site Brown"), textRule("T*", "site text"))};
    a12::ConvertOptions options = named("Site");
    options.colours = kColourTable;

    const a12::Conversion conversion = converted({codes}, options);
    const katana::entity::ColourTable& colours = conversion.customisation.colours;
    ASSERT_EQ(colours.size(), 3u);
    EXPECT_EQ(colours.find("site blue"), (Color{0, 112, 255, 255}));
    EXPECT_EQ(colours.find("site brown"), (Color{10, 20, 30, 255})) << "a symbol's colour";
    EXPECT_EQ(colours.find("site text"), (Color{40, 50, 60, 255})) << "a text's colour";
    // As the rules first write each, in the order of the folded names.
    std::vector<std::string> written;
    for (const katana::entity::ColourTable::Entry& entry : colours.entries()) {
        written.push_back(entry.name);
    }
    EXPECT_EQ(written, (std::vector<std::string>{"site blue", "Site Brown", "site text"}));
    EXPECT_EQ(conversion.report.coloursResolved, written);
    EXPECT_EQ(conversion.report.coloursUnresolved, (std::vector<std::string>{"site green"}));
    // The rules keep the names they had; the table is beside them.
    EXPECT_EQ(conversion.customisation.map.rules()[1].colour, "SITE_BLUE");
    EXPECT_EQ(katana::entity::resolveColour(colours, "Red"), (Color{255, 0, 0, 255}));
}

TEST(CustomisationConvert, AStandardNameTheTableColoursOtherwiseKeepsTheStandardColourAndIsSaid)
{
    // The standard brown is 165, 42, 42 and the standard dark green 0, 100, 0
    // (docs/customisation.md, "Colour names"). This table gives brown and
    // dark green other colours, and red the very colour it has.
    const a12::LegacyFile table{"colours.4d", "150 75 0 15 \"brown\"\n"
                                              "0 128 0 12 \"dark green\"\n"
                                              "255 0 0 1 \"red\"\n"
                                              "0 112 255 21 \"site blue\"\n"};
    const a12::LegacyFile codes{
        "codes.mapfile",
        surveyCodes(feature("A*", "<colour>red</colour>") +
                        // By another spelling: it is the standard name all the same.
                        feature("B*", "<colour>Dark_Green</colour>") +
                        feature("C*", "<colour>site blue</colour>"),
                    symbolRule("S*", "Peg", "brown"))};
    a12::ConvertOptions options = named("Site");
    options.colours = table;

    const a12::Conversion conversion = converted({codes}, options);
    // The table cannot hold a standard name, so the customisation has the one
    // colour that is its own ...
    ASSERT_EQ(conversion.customisation.colours.size(), 1u);
    EXPECT_EQ(conversion.customisation.colours.find("site blue"), (Color{0, 112, 255, 255}));
    EXPECT_EQ(conversion.report.coloursResolved, (std::vector<std::string>{"site blue"}));
    EXPECT_TRUE(conversion.report.coloursUnresolved.empty());
    // ... the names draw as the standard has them ...
    EXPECT_EQ(katana::entity::resolveColour(conversion.customisation.colours, "brown"),
              (Color{165, 42, 42, 255}));
    // ... and the two colours the table meant and this could not carry are
    // said, in the order of the folded names and as the rules spell them:
    // 165, 42, 42 is A5 2A 2A; 150, 75, 0 is 96 4B 00; 0, 100, 0 is 00 64 00;
    // 0, 128, 0 is 00 80 00. Red, which the table gives its own colour, is not.
    using Kept = a12::StandardColourKept;
    EXPECT_EQ(conversion.report.coloursKeptStandard,
              (std::vector<Kept>{Kept{"brown", "#A52A2A", "#964B00"},
                                 Kept{"Dark_Green", "#006400", "#008000"}}));
    EXPECT_NE(a12::toText(conversion.report)
                  .find("colours_unresolved=0\n"
                        "colours_kept_standard=2\n"
                        "colour_kept_standard=\"brown\" standard=\"#A52A2A\" table=\"#964B00\"\n"
                        "colour_kept_standard=\"Dark_Green\" standard=\"#006400\" "
                        "table=\"#008000\"\n"
                        "names_renamed=0\n"),
              std::string::npos)
        << a12::toText(conversion.report);

    // Without a table there is nothing a standard name could have been.
    EXPECT_TRUE(converted({codes}, named("Site")).report.coloursKeptStandard.empty());
}

TEST(CustomisationConvert, WithoutAColourTableEveryNameTheStandardOnesLackIsLeftUnresolved)
{
    const a12::Conversion conversion = converted(
        {{"codes.mapfile", surveyCodes(feature("A*", "<colour>site blue</colour>") +
                                       feature("B*", "<colour>dark_gray</colour>"))}},
        named("Site"));
    EXPECT_TRUE(conversion.customisation.colours.empty());
    EXPECT_TRUE(conversion.report.coloursResolved.empty());
    // "dark_gray" folds to the standard "dark grey" and needs nothing.
    EXPECT_EQ(conversion.report.coloursUnresolved, (std::vector<std::string>{"site blue"}));
}

TEST(CustomisationConvert, APlotPenIsNeverGivenAColourThoughTheTableHasOne)
{
    const a12::LegacyFile codes{
        "codes.mapfile",
        surveyCodes(feature("A*", "<colour>pen 018</colour>") +
                        feature("B*", "<colour>PEN 7</colour>") +
                        // Digits and ONE letter is still a pen; two letters is a name.
                        feature("C*", "<colour>pen 12a</colour>") +
                        feature("D*", "<colour>pen 12ab</colour>") +
                        // And so is a word that merely begins with "pen".
                        feature("E*", "<colour>pencil</colour>"),
                    symbolRule("S*", "Peg", "pen 018"))};
    a12::ConvertOptions options = named("Site");
    options.colours = kColourTable;

    const a12::Conversion conversion = converted({codes}, options);
    const katana::entity::ColourTable& colours = conversion.customisation.colours;
    EXPECT_EQ(colours.size(), 2u);
    EXPECT_EQ(colours.find("pen 12ab"), (Color{70, 80, 93, 255}));
    EXPECT_EQ(colours.find("pencil"), (Color{70, 80, 94, 255}));
    EXPECT_FALSE(colours.find("pen 018").has_value());
    EXPECT_FALSE(colours.find("pen 7").has_value());
    EXPECT_FALSE(colours.find("pen 12a").has_value());
    // By folded name: "pen 018" < "pen 12a" < "pen 7".
    EXPECT_EQ(conversion.report.coloursUnresolved,
              (std::vector<std::string>{"pen 018", "pen 12a", "PEN 7"}));
    EXPECT_EQ(conversion.report.coloursResolved, (std::vector<std::string>{"pen 12ab", "pencil"}));
}

TEST(CustomisationConvert, AColourTableThatCannotBeReadAsOneIsRefusedByItsLine)
{
    const a12::LegacyFile codes{"codes.mapfile",
                                surveyCodes(feature("A*", "<colour>site blue</colour>"))};
    a12::ConvertOptions options = named("Site");
    for (const char* line : {"0 112 300 21 \"site blue\"", // not a colour: 300
                             "0 112 21 \"site blue\"",     // three numbers
                             "0 112 255 21 site blue",     // the name is not in quotes
                             "0 112 255 21 \"site blue",   // the quote never closes
                             "0 112 x 21 \"site blue\"",   // not a number
                             "0 112 255"}) {               // nothing after the colour
        SCOPED_TRACE(line);
        options.colours = a12::LegacyFile{"colours.4d", std::string("1 2 3 1 \"a\"\n\n") + line};
        const std::string said = failure({codes}, options, ErrorCode::InvalidArgument);
        EXPECT_NE(said.find("colours.4d: line 3 of the colour table is not"), std::string::npos)
            << said;
    }
}

TEST(CustomisationConvert, ANameTheTableGivesTwoColoursIsRefusedOnlyWhenARuleUsesIt)
{
    const a12::LegacyFile table{"colours.4d", "1 2 3 1 \"site blue\"\n"
                                              "9 9 9 2 \"other\"\n"
                                              "1 2 3 3 \"SITE_BLUE\"\n" // the same colour again
                                              "4 5 6 4 \"Site Blue\"\n"
                                              "7 7 7 5 \"other\"\n"};
    a12::ConvertOptions options = named("Site");
    options.colours = table;
    const std::string said =
        failure({{"codes.mapfile", surveyCodes(feature("A*", "<colour>site blue</colour>"))}},
                options, ErrorCode::InvalidArgument);
    EXPECT_NE(said.find("colours.4d: lines 1 and 4 of the colour table give \"site blue\" two colours"),
              std::string::npos)
        << said;

    // "other" is given two colours as well, and no rule asks for it.
    const a12::Conversion unused = converted(
        {{"codes.mapfile", surveyCodes(feature("A*", "<colour>red</colour>"))}}, options);
    EXPECT_TRUE(unused.customisation.colours.empty());
}

// ---- --notice-from -------------------------------------------------------------------------------

TEST(CustomisationConvert, TheNoticeIsAFilesLeadingCommentBlockLineForLine)
{
    const a12::LegacyFile lines{
        "lines.4d",
        "\r\n"
        "// ------\r\n"
        "//  Two blanks: one is the marker's, one is kept. \r\n"
        "//\r\n"
        "//No blank after the marker\r\n"
        "  // indented\r\n"
        "\r\n"
        "// after a blank line: not the leading block\r\n"
        "worldstyle \"A\" { move 0 0 draw 1 0 }\r\n"
        "// nor is this\r\n"};
    a12::ConvertOptions options = named("Site");
    options.noticeFrom = {lines};

    const a12::Conversion conversion = converted({lines}, options);
    EXPECT_EQ(conversion.customisation.notice,
              (std::vector<std::string>{"------", " Two blanks: one is the marker's, one is kept. ",
                                        "", "No blank after the marker", "indented"}));
    EXPECT_EQ(conversion.report.noticeLines, 5u);
}

TEST(CustomisationConvert, ABlockTheSameAsTheOneBeforeItIsTakenOnce)
{
    const std::string body = "worldstyle \"A\" { move 0 0 draw 1 0 }\n";
    const a12::LegacyFile first{"a.4d", "// Ours\n// All rights reserved\n" + body};
    const a12::LegacyFile same{"b.4d", "// Ours\n// All rights reserved\n\n" + body};
    const a12::LegacyFile other{"c.4d", "// Theirs\n" + body};
    a12::ConvertOptions options = named("Site");

    options.noticeFrom = {first, same};
    EXPECT_EQ(converted({first}, options).customisation.notice,
              (std::vector<std::string>{"Ours", "All rights reserved"}));

    // Only the block BEFORE it: the same notice again after another is kept.
    options.noticeFrom = {first, other, same};
    EXPECT_EQ(converted({first}, options).customisation.notice,
              (std::vector<std::string>{"Ours", "All rights reserved", "Theirs", "Ours",
                                        "All rights reserved"}));
}

TEST(CustomisationConvert, AFileWithNoLeadingCommentBlockCannotGiveANotice)
{
    const a12::LegacyFile lines{"lines.4d", "worldstyle \"A\" { move 0 0 draw 1 0 }\n// late\n"};
    a12::ConvertOptions options = named("Site");
    options.noticeFrom = {lines};
    const std::string said = failure({lines}, options, ErrorCode::InvalidArgument);
    EXPECT_NE(said.find("lines.4d: there is no // comment block at the head of this file"),
              std::string::npos)
        << said;
}

// ---- what it refuses ----------------------------------------------------------------------------

TEST(CustomisationConvert, ACustomisationMustBeGivenANameAndOneItMayHave)
{
    const std::vector<a12::LegacyFile> files{
        {"lines.4d", "worldstyle \"A\" { move 0 0 draw 1 0 }"}};
    const std::string missing = failure(files, a12::ConvertOptions{}, ErrorCode::InvalidArgument);
    EXPECT_NE(missing.find("the customisation has no name: give one with --name <name>"),
              std::string::npos)
        << missing;
    // The format's own rule (entity::validateCustomisationName): a project
    // records the name, and its store takes no path.
    failure(files, named("Site/2026"), ErrorCode::InvalidArgument);
    failure(files, named(" Site"), ErrorCode::InvalidArgument);
}

TEST(CustomisationConvert, ThereMustBeAFileAndEveryWordGivenMustBeOne)
{
    const std::string none = failure({}, named("Site"), ErrorCode::InvalidArgument);
    EXPECT_NE(none.find("there is no file to convert"), std::string::npos) << none;

    const std::vector<a12::LegacyFile> files{
        {"lines.4d", "worldstyle \"A\" { move 0 0 draw 1 0 }"}};
    a12::ConvertOptions strip = named("Site");
    strip.stripLeadingWords = {"ACME", ""};
    const std::string stripSaid = failure(files, strip, ErrorCode::InvalidArgument);
    EXPECT_NE(stripSaid.find("--strip-leading-word"), std::string::npos) << stripSaid;

    a12::ConvertOptions remove = named("Site");
    remove.removeWords = {""};
    const std::string removeSaid = failure(files, remove, ErrorCode::InvalidArgument);
    EXPECT_NE(removeSaid.find("--remove-word"), std::string::npos) << removeSaid;
}

TEST(CustomisationConvert, AFileThatIsNeitherKindIsRefusedByName)
{
    const std::string said =
        failure({{"lines.4d", "worldstyle \"A\" { move 0 0 draw 1 0 }"},
                 {"notes.txt", "these are somebody's notes"}},
                named("Site"), ErrorCode::InvalidArgument);
    EXPECT_NE(said.find("notes.txt: "), std::string::npos) << said;
    EXPECT_NE(said.find("neither a survey code file nor a linestyle or symbol library"),
              std::string::npos)
        << said;

    // A Katana customisation is not one of the older files either: there is
    // nothing to convert it from.
    const std::string twin = failure({{"twin.json", twinText("test_symbols.customisation.json")}},
                                     named("Site"), ErrorCode::InvalidArgument);
    EXPECT_NE(twin.find("twin.json: "), std::string::npos) << twin;
}

TEST(CustomisationConvert, AFileAReaderCannotReadAtAllIsRefusedByName)
{
    const a12::LegacyFile lines{"lines.4d", "worldstyle \"A\" { move 0 0 draw 1 0 }"};
    // A quoted text that never closes: the style library reader's one refusal.
    const std::string quote =
        failure({lines, {"open.4d", "worldstyle \"A { move 0 0 draw 1 0 }\n"}}, named("Site"),
                ErrorCode::InvalidArgument);
    EXPECT_NE(quote.find("open.4d: "), std::string::npos) << quote;
    EXPECT_NE(quote.find("a quoted text is never closed"), std::string::npos) << quote;

    // XML that is not well formed: <item> is still open where <map_data> ends.
    const std::string xml = failure(
        {lines, {"broken.mapfile", "<map_file><map_data><item><key>A*</key></map_data></map_file>"}},
        named("Site"), ErrorCode::InvalidArgument);
    EXPECT_NE(xml.find("broken.mapfile: "), std::string::npos) << xml;

    // Bytes that are no text: a UTF-16 byte order mark and then half a character.
    const std::string bytes = failure({lines, {"half.4d", std::string("\xFF\xFEw", 3)}},
                                      named("Site"), ErrorCode::ParseFailure);
    EXPECT_NE(bytes.find("cannot read half.4d: "), std::string::npos) << bytes;

    // And no bytes at all are neither kind of file.
    const std::string empty = failure({lines, {"empty.4d", ""}}, named("Site"),
                                      ErrorCode::InvalidArgument);
    EXPECT_NE(empty.find("empty.4d: "), std::string::npos) << empty;
}

TEST(CustomisationConvert, AFileThatGaveNothingIsSaidAndConvertsToNothing)
{
    // A file is asked what it is by a keyword anywhere in its text, so a
    // comment is enough to make one a style library - of no definitions.
    const a12::Conversion conversion =
        converted({{"nodefs.4d", "// worldstyle is only named in this comment\n"},
                   {"norules.mapfile", "<map_file><map_data></map_data></map_file>"}},
                  named("Site"));
    EXPECT_EQ(conversion.report.definitions, 0u);
    EXPECT_EQ(conversion.report.rules, 0u);
    EXPECT_EQ(conversion.report.keys, 0u);
    EXPECT_EQ(conversion.report.strokes, 0u);
    EXPECT_TRUE(conversion.customisation.library.empty());
    EXPECT_TRUE(conversion.customisation.map.empty());
    EXPECT_EQ(conversion.report.warnings,
              (std::vector<std::string>{
                  "nodefs.4d: no definition was read from this style library",
                  "norules.mapfile: no rule was read from this survey code file"}));
    // It is still a customisation, of a name and nothing else, and reads as one.
    const auto read = katana::entity::customisationFromJson(conversion.json);
    ASSERT_TRUE(read.ok()) << read.error().describe();
    EXPECT_EQ(read->name, "Site");
    EXPECT_TRUE(read->library.empty());
}

TEST(CustomisationConvert, CharactersReadByAnInferredEncodingAreCountedAndTheEncodingNamed)
{
    // Byte D8 is no UTF-8 by itself, and the file has no byte order mark: it
    // is read as Windows-1252, where D8 is U+00D8 (a capital O with a stroke),
    // which is C3 98 in UTF-8. Two of them: one in a name, one in a comment.
    const a12::LegacyFile ansi{"ansi_lines.4d", "// pipes of 100\xD8\n"
                                                "worldstyle \"\xD8 Kerb\" { move 0 0 draw 1 0 }\n"};
    a12::ConvertOptions options = named("Site");
    // Named twice on the command line, as a library asked for its notice is.
    options.noticeFrom = {ansi};
    const a12::Conversion conversion = converted({ansi}, options);
    EXPECT_TRUE(conversion.customisation.library.contains("\xC3\x98 Kerb"));
    EXPECT_EQ(conversion.customisation.notice, (std::vector<std::string>{"pipes of 100\xC3\x98"}));
    EXPECT_EQ(conversion.report.warnings,
              (std::vector<std::string>{
                  "ansi_lines.4d: neither UTF-8 nor marked as UTF-16, so read as Windows-1252 by "
                  "inference; characters outside ASCII that rest on it: 2"}));

    // The same text as UTF-8 is what it says it is, and nothing is inferred.
    const a12::LegacyFile utf8{"utf8_lines.4d",
                               "// pipes of 100\xC3\x98\n"
                               "worldstyle \"\xC3\x98 Kerb\" { move 0 0 draw 1 0 }\n"};
    const a12::Conversion plain = converted({utf8}, named("Site"));
    EXPECT_TRUE(plain.customisation.library.contains("\xC3\x98 Kerb"));
    EXPECT_TRUE(plain.report.warnings.empty());

    // A colour table read by a guess is said as well: its names are matched.
    const a12::LegacyFile table{"ansi_colours.4d", "1 2 3 1 \"gr\xD8n\"\n"};
    a12::ConvertOptions coloured = named("Site");
    coloured.colours = table;
    const a12::Conversion withTable = converted(
        {{"codes.mapfile", surveyCodes(feature("A*", "<colour>red</colour>"))}}, coloured);
    EXPECT_EQ(withTable.report.warnings,
              (std::vector<std::string>{
                  "ansi_colours.4d: neither UTF-8 nor marked as UTF-16, so read as Windows-1252 by "
                  "inference; characters outside ASCII that rest on it: 1"}));
}

// ---- from files, to a file -----------------------------------------------------------------------

namespace {

// A directory of this test's own under the system's temporary one, removed
// when the test ends. Its name ends in the time it was made, so that the same
// test running at once in another checkout's suite - which happens, with
// several worktrees on one machine - has a directory of its own too.
class Scratch {
  public:
    explicit Scratch(const std::string& name)
        : directory_(std::filesystem::temp_directory_path() /
                     ("katana_convert_" + name + "_" +
                      std::to_string(
                          std::chrono::steady_clock::now().time_since_epoch().count())))
    {
        std::filesystem::remove_all(directory_);
        std::filesystem::create_directories(directory_);
    }
    ~Scratch()
    {
        std::error_code ignored;
        std::filesystem::remove_all(directory_, ignored);
    }
    Scratch(const Scratch&) = delete;
    Scratch& operator=(const Scratch&) = delete;

    [[nodiscard]] std::filesystem::path file(const std::string& name) const
    {
        return directory_ / name;
    }
    [[nodiscard]] const std::filesystem::path& directory() const { return directory_; }

  private:
    std::filesystem::path directory_;
};

} // namespace

TEST(CustomisationConvert, FilesAreReadByPathAndTheTextIsWrittenWithLineFeedsAlone)
{
    const Scratch scratch("writes");
    a12::ConvertRequest request;
    request.name = "test_symbols";
    request.files = {kLegacyFixtures / "test_symbols.4d"};
    // A directory that is not there yet is made.
    request.output = scratch.file("made/here/test_symbols.customisation.json");

    const auto conversion = a12::convertLegacyFiles(request);
    ASSERT_TRUE(conversion.ok()) << conversion.error().describe();
    // The bytes on disk are the hand-written twin's, and so end their lines
    // with a line feed alone whatever the platform.
    const std::string written = bytesOf(request.output);
    EXPECT_EQ(written, twinText("test_symbols.customisation.json"));
    EXPECT_EQ(written, conversion->json);
    EXPECT_EQ(written.find('\r'), std::string::npos);
    EXPECT_EQ(conversion->report.bytes, written.size());
    EXPECT_EQ(std::filesystem::path(std::u8string(conversion->report.output.begin(),
                                                  conversion->report.output.end())),
              request.output);
}

namespace {

void writeFile(const std::filesystem::path& path, const std::string& text)
{
    std::ofstream file(path, std::ios::binary | std::ios::trunc);
    file.write(text.data(), static_cast<std::streamsize>(text.size()));
    ASSERT_TRUE(file.good()) << "cannot write " << path.string();
}

} // namespace

TEST(CustomisationConvert, EveryMemberOfARequestReachesTheFileThatIsWritten)
{
    // The path the program takes, with nothing left at its default: each of
    // the eight members has to be seen in the file, so one that is dropped on
    // the way from the request to the conversion - or wired to another - is
    // found here. The word stripped and the word removed are two different
    // words, so that neither can stand in for the other.
    const Scratch scratch("request");
    writeFile(scratch.file("lines.4d"),
              "// Site lines\n"
              "worldstyle \"ACME Kerb\" { group \"ACME Roads\" move 0 0 draw 1 0 }\n");
    writeFile(scratch.file("marks_symbols.4d"),
              "// Site marks\n"
              "worldstyle \"Peg\" { mode vertex move 0 0 circle 1 }\n");
    writeFile(scratch.file("codes.mapfile"),
              surveyCodes(feature("KB*", "<colour>site blue</colour>"
                                         "<linestyle>ACME Kerb</linestyle>"
                                         "<comment>Kerb to VND standard</comment>"
                                         "<group>ACME ROADS</group>"),
                          symbolRule("PG*", "Peg")));
    writeFile(scratch.file("colours.4d"), "0 112 255 21 \"site_blue\"\n");

    a12::ConvertRequest request;
    request.name = "Site";
    request.description = "The site's own codes";
    request.noticeFrom = {scratch.file("lines.4d"), scratch.file("marks_symbols.4d")};
    request.colours = scratch.file("colours.4d");
    request.stripLeadingWords = {"ACME"};
    request.removeWords = {"VND"};
    request.output = scratch.file("site.customisation.json");
    request.files = {scratch.file("lines.4d"), scratch.file("codes.mapfile"),
                     scratch.file("marks_symbols.4d")};

    const auto conversion = a12::convertLegacyFiles(request);
    ASSERT_TRUE(conversion.ok()) << conversion.error().describe();
    // Asked of the FILE: it is what the person who ran the program has.
    const auto written = katana::entity::customisationFromJson(bytesOf(request.output));
    ASSERT_TRUE(written.ok()) << written.error().describe();
    EXPECT_EQ(written->name, "Site");                                         // name
    EXPECT_EQ(written->description, "The site's own codes");                  // description
    EXPECT_EQ(written->notice, (std::vector<std::string>{"Site lines", "Site marks"})); // noticeFrom
    EXPECT_EQ(written->colours.find("site blue"), (Color{0, 112, 255, 255})); // colours
    EXPECT_EQ(written->library.names(), (std::vector<std::string>{"Kerb", "Peg"})); // files, strip
    EXPECT_EQ(definition(*written, "Kerb").group, "Roads");
    EXPECT_FALSE(definition(*written, "Kerb").symbol);
    EXPECT_TRUE(definition(*written, "Peg").symbol) << "its file's name says so";
    ASSERT_EQ(written->map.rules().size(), 2u);
    EXPECT_EQ(written->map.rules()[0].linestyle, "Kerb");
    EXPECT_EQ(written->map.rules()[0].group, "ROADS");
    EXPECT_EQ(written->map.rules()[0].comment, "Kerb to standard");           // removeWords
    EXPECT_TRUE(*written == conversion->customisation);

    const a12::ConvertReport& report = conversion->report;
    EXPECT_EQ(report.files, 3u);
    EXPECT_EQ(report.definitions, 2u);
    EXPECT_EQ(report.symbols, 1u);
    EXPECT_EQ(report.rules, 2u);
    EXPECT_EQ(report.namesRenamed, 1u);
    EXPECT_EQ(report.groupsRenamed, 1u);
    EXPECT_EQ(report.referencesRenamed, 1u);
    EXPECT_EQ(report.ruleGroupsRenamed, 1u);
    EXPECT_EQ(report.commentsChanged, 1u);
    EXPECT_EQ(report.coloursResolved, (std::vector<std::string>{"site blue"}));
    EXPECT_TRUE(report.coloursUnresolved.empty());
    EXPECT_EQ(report.noticeLines, 2u);
    EXPECT_TRUE(report.unresolvedReferences.empty());
    EXPECT_TRUE(report.replaced.empty());
    EXPECT_TRUE(report.warnings.empty());
}

TEST(CustomisationConvert, AWriteThatFailsLeavesTheFileThatWasThere)
{
    // The output is the file a build compiles in. It is written beside itself
    // first and then put in its place, so that it is never half of a new one.
    const Scratch scratch("partial");
    a12::ConvertRequest request;
    request.name = "test_symbols";
    request.files = {kLegacyFixtures / "test_symbols.4d"};
    request.output = scratch.file("out.customisation.json");
    std::filesystem::path partial = request.output;
    partial += ".partial";

    ASSERT_TRUE(a12::convertLegacyFiles(request).ok());
    EXPECT_EQ(bytesOf(request.output), twinText("test_symbols.customisation.json"));
    EXPECT_FALSE(std::filesystem::exists(partial)) << "nothing is left beside it";

    // Again, under another name: the file that was there is replaced, whole.
    request.name = "again";
    ASSERT_TRUE(a12::convertLegacyFiles(request).ok());
    const std::string second = bytesOf(request.output);
    EXPECT_NE(second.find("\"name\": \"again\""), std::string::npos);
    EXPECT_EQ(second.find("\"name\": \"test_symbols\""), std::string::npos);
    EXPECT_FALSE(std::filesystem::exists(partial));

    // And a third time where the text cannot be written at all: a directory,
    // with something in it, stands where it is written first.
    std::filesystem::create_directories(partial / "in the way");
    request.name = "third";
    const auto failed = a12::convertLegacyFiles(request);
    ASSERT_FALSE(failed.ok());
    EXPECT_EQ(failed.error().code, ErrorCode::FileExportFailure) << failed.error().describe();
    // It is the WRITE that is said to have failed, and it is noticed there:
    // nothing that could not be written is ever put in the output's place.
    EXPECT_NE(failed.error().describe().find("cannot write this file"), std::string::npos)
        << failed.error().describe();
    EXPECT_NE(failed.error().describe().find("out.customisation.json"), std::string::npos)
        << failed.error().describe();
    EXPECT_EQ(bytesOf(request.output), second) << "the file that was there, byte for byte";
}

TEST(CustomisationConvert, WithoutAnOutputNothingIsWrittenAndTheReportStillSaysWhatWasFound)
{
    a12::ConvertRequest request;
    request.name = "test_survey";
    request.files = {kLegacyFixtures / "test_survey.mapfile"};
    const auto conversion = a12::convertLegacyFiles(request);
    ASSERT_TRUE(conversion.ok()) << conversion.error().describe();
    EXPECT_TRUE(conversion->report.output.empty());
    EXPECT_EQ(conversion->report.rules, 11u);
    EXPECT_EQ(conversion->json, twinText("test_survey.customisation.json"));
}

TEST(CustomisationConvert, AMissingFileIsNotFoundByItsPathAndNothingIsWritten)
{
    const Scratch scratch("missing");
    const std::filesystem::path output = scratch.file("out.customisation.json");
    const std::filesystem::path present = kLegacyFixtures / "test_symbols.4d";
    const std::filesystem::path absent = scratch.file("no_such_library.4d");

    // Wherever on the command line it is named: a file to convert, a notice,
    // the colour table.
    for (int place = 0; place < 3; ++place) {
        SCOPED_TRACE(place);
        a12::ConvertRequest request;
        request.name = "Site";
        request.output = output;
        request.files = {present};
        if (place == 0) {
            request.files.push_back(absent);
        } else if (place == 1) {
            request.noticeFrom = {absent};
        } else {
            request.colours = absent;
        }
        const auto conversion = a12::convertLegacyFiles(request);
        ASSERT_FALSE(conversion.ok());
        EXPECT_EQ(conversion.error().code, ErrorCode::NotFound) << conversion.error().describe();
        EXPECT_NE(conversion.error().describe().find("no_such_library.4d"), std::string::npos)
            << conversion.error().describe();
        EXPECT_FALSE(std::filesystem::exists(output));
    }
}

TEST(CustomisationConvert, AnOutputThatCannotBeWrittenFailsTheConversion)
{
    const Scratch scratch("unwritable");
    a12::ConvertRequest request;
    request.name = "Site";
    request.files = {kLegacyFixtures / "test_symbols.4d"};
    // A directory stands where the file is to go.
    request.output = scratch.file("taken");
    std::filesystem::create_directories(request.output);
    const auto conversion = a12::convertLegacyFiles(request);
    ASSERT_FALSE(conversion.ok());
    EXPECT_EQ(conversion.error().code, ErrorCode::FileExportFailure)
        << conversion.error().describe();
}

// ---- the report -----------------------------------------------------------------------------------

TEST(CustomisationConvert, TheReportIsOneFactALineWithEveryListCountedAndThenNamed)
{
    a12::ConvertReport report;
    report.name = "Site";
    report.files = 4;
    report.definitions = 7;
    report.symbols = 4;
    report.linestyles = 3;
    report.atVertices = 2;
    report.groups = 5;
    report.strokes = 36;
    report.replaced = {a12::ReplacedDefinition{"Gate \"A\"", "site_symbols.4d", "site_lines.4d",
                                               true, false, true, 2, 0}};
    report.rules = 11;
    report.keys = 8;
    report.coloursResolved = {"site blue"};
    report.coloursUnresolved = {"pen 018", "say \"what\""};
    report.coloursKeptStandard = {a12::StandardColourKept{"brown", "#A52A2A", "#964B00"}};
    report.namesRenamed = 1;
    report.groupsRenamed = 6;
    report.referencesRenamed = 2;
    report.ruleGroupsRenamed = 3;
    report.commentsChanged = 9;
    report.unresolvedReferences = {"0"};
    report.noticeLines = 12;
    report.warnings = {};
    report.bytes = 4096;
    EXPECT_EQ(a12::toText(report), "name=\"Site\"\n"
                                   "files=4\n"
                                   "definitions=7\n"
                                   "symbols=4\n"
                                   "linestyles=3\n"
                                   "at_vertices=2\n"
                                   "groups=5\n"
                                   "strokes=36\n"
                                   "definitions_replaced=1\n"
                                   "definition_replaced=\"Gate \\\"A\\\"\" "
                                   "kept=\"site_symbols.4d\" kept_as=symbol "
                                   "dropped=\"site_lines.4d\" dropped_as=linestyle differs=yes "
                                   "linestyle_rules=2 symbol_rules=0\n"
                                   "rules=11\n"
                                   "keys=8\n"
                                   "colours_resolved=1\n"
                                   "colour_resolved=\"site blue\"\n"
                                   "colours_unresolved=2\n"
                                   "colour_unresolved=\"pen 018\"\n"
                                   "colour_unresolved=\"say \\\"what\\\"\"\n"
                                   "colours_kept_standard=1\n"
                                   "colour_kept_standard=\"brown\" standard=\"#A52A2A\" "
                                   "table=\"#964B00\"\n"
                                   "names_renamed=1\n"
                                   "groups_renamed=6\n"
                                   "references_renamed=2\n"
                                   "rule_groups_renamed=3\n"
                                   "comments_changed=9\n"
                                   "unresolved_references=1\n"
                                   "unresolved_reference=\"0\"\n"
                                   "notice_lines=12\n"
                                   "warnings=0\n"
                                   "bytes=4096\n");
    // Where it was written is said only when it was.
    report.output = "out/site.customisation.json";
    EXPECT_TRUE(a12::toText(report).ends_with("bytes=4096\n"
                                              "output=\"out/site.customisation.json\"\n"));
}

TEST(CustomisationConvert, WhatAReaderSaidOfAFileIsCarriedInTheReport)
{
    // An <item> with no key names no code; the survey code file reader skips
    // it and says so, and the conversion passes that on under the file's name.
    const a12::Conversion conversion = converted(
        {{"codes.mapfile",
          surveyCodes(feature("A*", "<model>ROADS</model>") + "<item><group>LOST</group></item>")}},
        named("Site"));
    EXPECT_EQ(conversion.report.rules, 1u);
    ASSERT_EQ(conversion.report.warnings.size(), 1u);
    EXPECT_TRUE(conversion.report.warnings[0].starts_with("codes.mapfile: "))
        << conversion.report.warnings[0];
    EXPECT_NE(a12::toText(conversion.report).find("warnings=1\nwarning=\"codes.mapfile: "),
              std::string::npos);
}

// ---- the reference customisation ----------------------------------------------------------------
//
// The four files of KATANA_REFERENCE_CUSTOMISATION_DIR. They are third-party
// material under their author's own licence and are in no clone, so this SKIPS
// where the folder is absent or holds no file in the legacy formats - and
// FAILS where it holds some that are not the four: a folder with a file
// missing, or one too many, is a mistake to be told of, and a skip there
// would read as "nothing to test". Nothing here spells a file's name or a
// word of its text beyond what the census gives as figures: the files are
// found by looking inside each, and the words to strip are found in the data.

namespace {

struct Reference {
    std::vector<a12::LegacyFile> inLoadOrder{};
    std::vector<a12::LegacyFile> libraries{}; // the linestyle library, then the symbol library
    std::optional<a12::LegacyFile> colours{};
};

// What a folder holds of the reference customisation.
struct ReferenceFolder {
    // Nothing to convert, and so nothing to test: the folder is not there, or
    // no file in it is a style library or a survey code file.
    bool empty = true;
    // What it holds instead of the four, when it holds something else; empty
    // when `reference` is the four.
    std::string problem{};
    Reference reference{};
};

bool holds(const std::string& name, std::string_view word)
{
    std::string lower;
    for (const char c : name) {
        lower += (c >= 'A' && c <= 'Z') ? static_cast<char>(c - 'A' + 'a') : c;
    }
    return lower.find(word) != std::string::npos;
}

// The documented load order: the linestyle library, the survey code file, the
// names file, the symbol library. Which library is the symbol one is said by
// its name, as the reader itself goes by; so is which survey code file is the
// names file. A file that is neither kind - a note, a converted customisation
// - is passed over.
ReferenceFolder referenceFolder(const std::filesystem::path& directory)
{
    ReferenceFolder folder;
    std::error_code ignored;
    if (!std::filesystem::is_directory(directory, ignored)) {
        return folder;
    }
    std::vector<std::filesystem::path> paths;
    for (const auto& entry : std::filesystem::directory_iterator(directory, ignored)) {
        if (entry.is_regular_file(ignored)) {
            paths.push_back(entry.path());
        }
    }
    std::sort(paths.begin(), paths.end());

    std::vector<a12::LegacyFile> linestyles;
    std::vector<a12::LegacyFile> symbols;
    std::vector<a12::LegacyFile> codes;
    std::vector<a12::LegacyFile> names;
    for (const std::filesystem::path& path : paths) {
        const std::u8string name = path.filename().u8string();
        a12::LegacyFile file{std::string(name.begin(), name.end()), bytesOf(path)};
        const auto decoded = katana::core::decodeText(file.bytes);
        const auto kind = decoded ? a12::customisationKind(decoded->text)
                                  : katana::core::Result<a12::CustomisationFile>(decoded.error());
        if (!kind) {
            continue;
        }
        if (*kind == a12::CustomisationFile::StyleLibrary) {
            (holds(file.name, "symbol") ? symbols : linestyles).push_back(std::move(file));
        } else {
            (holds(file.name, "names") ? names : codes).push_back(std::move(file));
        }
    }
    if (linestyles.empty() && symbols.empty() && codes.empty() && names.empty()) {
        return folder;
    }
    folder.empty = false;
    if (linestyles.size() != 1 || symbols.size() != 1 || codes.size() != 1 || names.size() != 1) {
        folder.problem = "the reference folder holds linestyle libraries: " +
                         std::to_string(linestyles.size()) +
                         ", symbol libraries: " + std::to_string(symbols.size()) +
                         ", survey code files: " + std::to_string(codes.size()) +
                         ", names files: " + std::to_string(names.size()) +
                         " - the reference customisation is one of each";
        return folder;
    }
    Reference& reference = folder.reference;
    reference.inLoadOrder = {linestyles[0], codes[0], names[0], symbols[0]};
    reference.libraries = {linestyles[0], symbols[0]};
    const std::filesystem::path table = directory / "support" / "colours.4d";
    if (std::filesystem::is_regular_file(table, ignored)) {
        reference.colours = a12::LegacyFile{"colours.4d", bytesOf(table)};
    }
    return folder;
}

// The words standing IN FRONT of group paths, each with the number of
// definitions whose path it begins: what precedes a path's first blank and is
// no part of the path itself (it holds no '/').
std::map<std::string, std::size_t> wordsBeforeGroupPaths(const Customisation& customisation,
                                                         std::size_t& grouped)
{
    std::map<std::string, std::size_t> words;
    grouped = 0;
    customisation.library.forEach([&](const LineStyle& style) {
        if (style.group.empty()) {
            return;
        }
        ++grouped;
        const std::size_t blank = style.group.find(' ');
        if (blank != std::string::npos && blank > 0 &&
            style.group.find('/') > blank) { // npos is greater too: a path of one part
            ++words[style.group.substr(0, blank)];
        }
    });
    return words;
}

bool beginsWithWord(const std::string& text, const std::vector<std::string>& words)
{
    return std::any_of(words.begin(), words.end(),
                       [&](const std::string& word) { return text.starts_with(word + " "); });
}

// A plot pen as the brief states it: "pen", digits, at most one letter.
// Restated here, apart from the converter's own test of it.
bool looksLikeAPlotPen(std::string_view name)
{
    if (!name.starts_with("pen ")) {
        return false;
    }
    name.remove_prefix(4);
    std::size_t digits = 0;
    while (digits < name.size() && name[digits] >= '0' && name[digits] <= '9') {
        ++digits;
    }
    return digits > 0 && name.size() - digits <= 1;
}

} // namespace

TEST(CustomisationConvert, AReferenceFolderIsSkippedOnlyWhenItHoldsNoLegacyFileAndRefusedWhenItHoldsTheWrongOnes)
{
    // What decides between a skip and a failure of the test below, on folders
    // made here: the test itself reads one folder, the one the build names.
    const Scratch scratch("reference");
    const std::filesystem::path absent = scratch.file("not_there");
    EXPECT_TRUE(referenceFolder(absent).empty);

    // Files, and none of them in the legacy formats: still nothing to test.
    const std::filesystem::path other = scratch.file("other");
    std::filesystem::create_directories(other);
    writeFile(other / "notes.txt", "these are somebody's notes");
    writeFile(other / "converted.customisation.json", twinText("test_symbols.customisation.json"));
    EXPECT_TRUE(referenceFolder(other).empty);
    EXPECT_TRUE(referenceFolder(other).problem.empty());

    // Three of the four: the names file is not there.
    const std::filesystem::path three = scratch.file("three");
    std::filesystem::create_directories(three);
    writeFile(three / "lines.4d", "worldstyle \"A\" { move 0 0 draw 1 0 }");
    writeFile(three / "x_symbols.4d", "worldstyle \"B\" { mode vertex move 0 0 }");
    writeFile(three / "codes.mapfile", surveyCodes(feature("A*", "<linestyle>A</linestyle>")));
    const ReferenceFolder missing = referenceFolder(three);
    EXPECT_FALSE(missing.empty) << "a folder with legacy files in it is never skipped";
    EXPECT_EQ(missing.problem,
              "the reference folder holds linestyle libraries: 1, symbol libraries: 1, survey "
              "code files: 1, names files: 0 - the reference customisation is one of each");

    // The four: no problem, and in the documented load order whatever their
    // names sort as.
    writeFile(three / "a_names.mapfile", surveyCodes(feature("A*", "<comment>a</comment>")));
    const ReferenceFolder four = referenceFolder(three);
    EXPECT_FALSE(four.empty);
    EXPECT_TRUE(four.problem.empty()) << four.problem;
    ASSERT_EQ(four.reference.inLoadOrder.size(), 4u);
    EXPECT_EQ(four.reference.inLoadOrder[0].name, "lines.4d");
    EXPECT_EQ(four.reference.inLoadOrder[1].name, "codes.mapfile");
    EXPECT_EQ(four.reference.inLoadOrder[2].name, "a_names.mapfile");
    EXPECT_EQ(four.reference.inLoadOrder[3].name, "x_symbols.4d");
    EXPECT_FALSE(four.reference.colours.has_value()) << "no support folder here";

    // And a fifth legacy file beside them is one too many.
    writeFile(three / "more_lines.4d", "worldstyle \"C\" { move 0 0 draw 1 0 }");
    const ReferenceFolder five = referenceFolder(three);
    EXPECT_FALSE(five.empty);
    EXPECT_EQ(five.problem,
              "the reference folder holds linestyle libraries: 2, symbol libraries: 1, survey "
              "code files: 1, names files: 1 - the reference customisation is one of each");
}

TEST(CustomisationConvert, TheReferenceCustomisationConvertsToTheFiguresOfItsCensus)
{
    const ReferenceFolder folder =
        referenceFolder(std::filesystem::path{KATANA_REFERENCE_CUSTOMISATION_FILES});
    if (folder.empty) {
        GTEST_SKIP() << "the reference customisation is not on this machine";
    }
    ASSERT_TRUE(folder.problem.empty()) << folder.problem;
    const Reference* const reference = &folder.reference;

    // As the files are: to find the words that stand before their group paths.
    std::size_t grouped = 0;
    const std::map<std::string, std::size_t> found = wordsBeforeGroupPaths(
        converted(reference->inLoadOrder, named("NSW")).customisation, grouped);
    // Two: the publisher's, before most paths, and the vendor's, before a few.
    ASSERT_EQ(found.size(), 2u);
    std::vector<std::pair<std::string, std::size_t>> byCount(found.begin(), found.end());
    std::sort(byCount.begin(), byCount.end(),
              [](const auto& a, const auto& b) { return a.second > b.second; });
    const std::string publisher = byCount[0].first;
    const std::string vendor = byCount[1].first;
    EXPECT_GT(byCount[0].second * 2, grouped) << "it begins most group paths";
    EXPECT_LT(byCount[1].second * 20, grouped) << "it begins a few";
    const std::vector<std::string> words{publisher, vendor};

    a12::ConvertOptions options = named("NSW");
    options.description = "The reference customisation";
    options.noticeFrom = reference->libraries;
    options.colours = reference->colours;
    options.stripLeadingWords = words;
    options.removeWords = {publisher};
    const a12::Conversion conversion = converted(reference->inLoadOrder, options);
    const Customisation& nsw = conversion.customisation;
    const a12::ConvertReport& report = conversion.report;

    // ---- the census (tools/reference_census.py over the four files) ---------
    // 796 blocks, four of them defined twice: 792 definitions. 474 from the
    // symbol library, which is what makes them symbols, and 318 not.
    EXPECT_EQ(nsw.library.size(), 792u);
    EXPECT_EQ(report.definitions, 792u);
    EXPECT_EQ(report.symbols, 474u);
    EXPECT_EQ(report.linestyles, 318u);
    EXPECT_EQ(report.atVertices, 157u);
    EXPECT_EQ(report.groups, 71u);
    EXPECT_EQ(katana::entity::styleGroups(nsw.library).size(), 71u);
    EXPECT_EQ(nsw.map.size(), 1624u); // 725 + 899
    EXPECT_EQ(report.rules, 1624u);
    EXPECT_EQ(nsw.map.keys().size(), 632u);
    EXPECT_EQ(report.keys, 632u);
    EXPECT_EQ(nsw.map.stylesReferenced().size(), 426u);

    // The strokes of the definitions that survive, by kind.
    std::map<StrokeOp, std::size_t> strokes;
    std::size_t symbols = 0;
    std::size_t atVertices = 0;
    nsw.library.forEach([&](const LineStyle& style) {
        symbols += style.symbol ? 1 : 0;
        atVertices += style.atVertices ? 1 : 0;
        for (const Stroke& stroke : style.strokes) {
            ++strokes[stroke.op];
        }
        EXPECT_EQ(style.source, "NSW") << style.name;
    });
    EXPECT_EQ(symbols, 474u);
    EXPECT_EQ(atVertices, 157u);
    EXPECT_EQ(strokes[StrokeOp::Move], 17014u);
    EXPECT_EQ(strokes[StrokeOp::Draw], 17220u);
    EXPECT_EQ(strokes[StrokeOp::Arc], 104u);
    EXPECT_EQ(strokes[StrokeOp::Circle], 312u);
    EXPECT_EQ(strokes[StrokeOp::Dot], 178u);
    EXPECT_EQ(strokes[StrokeOp::Pen], 342u);
    EXPECT_EQ(strokes[StrokeOp::Text], 514u);
    EXPECT_EQ(report.strokes, 17014u + 17220u + 104u + 312u + 178u + 342u + 514u);

    // A water main, end to end: a code, through the rules, to a definition.
    const auto water = nsw.map.lookup("WM01");
    EXPECT_EQ(water.resolved.model, "SURVEY SERVICES");
    EXPECT_EQ(water.resolved.linestyle, "WATR Main");
    EXPECT_EQ(water.resolved.breakline, SurveyBreakline::Line);
    const LineStyle* main = nsw.library.find(water.resolved.linestyle);
    ASSERT_NE(main, nullptr);
    EXPECT_FALSE(main->symbol);
    EXPECT_FALSE(main->strokes.empty());
    // And a code that gets a symbol, defined at vertices.
    const auto bollard = nsw.map.lookup("AC01");
    ASSERT_TRUE(bollard.resolved.symbol.has_value());
    const LineStyle* shape = nsw.library.find(bollard.resolved.symbol->style);
    ASSERT_NE(shape, nullptr);
    EXPECT_TRUE(shape->atVertices);
    EXPECT_TRUE(shape->symbol);

    // Five of the 426 names the rules use are defined by neither library.
    EXPECT_EQ(report.unresolvedReferences,
              (std::vector<std::string>{"0", "1", "Circle Single",
                                        "LNMK Dividing - Separation Line S2 Multi Lane", "SBEND"}));

    // ---- the two words are gone from the front of everything ------------------
    nsw.library.forEach([&](const LineStyle& style) {
        EXPECT_FALSE(beginsWithWord(style.name, words)) << style.name;
        EXPECT_FALSE(beginsWithWord(style.group, words)) << style.group;
    });
    for (const SurveyRule& rule : nsw.map.rules()) {
        EXPECT_FALSE(beginsWithWord(rule.group, words)) << rule.group;
        EXPECT_FALSE(beginsWithWord(rule.linestyle, words)) << rule.linestyle;
        if (rule.symbol) {
            EXPECT_FALSE(beginsWithWord(rule.symbol->style, words)) << rule.symbol->style;
        }
        EXPECT_EQ(rule.comment.find(" " + publisher + " "), std::string::npos) << rule.comment;
    }
    // The names the earlier clean-up gave the 29 definitions that carried the
    // publisher's word: cad::builtinDefinitionRenames, restated (this suite
    // may be built without cad). Each is defined, so each lost exactly the
    // word and nothing else.
    for (const char* now :
         {"Accepted For Construction",   "Horizontal Scale 1 to 100",  "Horizontal Scale 1 to 1000",
          "Horizontal Scale 1 to 10000", "Horizontal Scale 1 to 200",  "Horizontal Scale 1 to 2000",
          "Horizontal Scale 1 to 250",   "Horizontal Scale 1 to 2500", "Horizontal Scale 1 to 50",
          "Horizontal Scale 1 to 500",   "Horizontal Scale 1 to 5000", "North Point no whiteout",
          "North Point with whiteout",   "Not For Construction",       "SM Basin Label",
          "SM Pit",                      "SURVEY - FU",                "SURVEY - FZ",
          "SURVEY - HO",                 "SURVEY - HZ",                "Site of Work",
          "Vertical Scale 1 to 100",     "Vertical Scale 1 to 1000",   "Vertical Scale 1 to 20",
          "Vertical Scale 1 to 200",     "Vertical Scale 1 to 25",     "Vertical Scale 1 to 250",
          "Vertical Scale 1 to 50",      "Vertical Scale 1 to 500"}) {
        EXPECT_TRUE(nsw.library.contains(now)) << now;
    }
    EXPECT_EQ(report.namesRenamed, 29u);
    // What else carried a word, by the census (its `carried`, given the two
    // words to strip and the publisher's to remove): the group paths of 786
    // of the 792 definitions; the group of every one of the 1,029 rules of
    // where a code goes; eight rules naming four of the renamed linestyles;
    // one comment.
    EXPECT_EQ(report.groupsRenamed, 786u);
    EXPECT_EQ(report.ruleGroupsRenamed, 1029u);
    EXPECT_EQ(report.referencesRenamed, 8u);
    EXPECT_EQ(report.commentsChanged, 1u);

    // ---- what went (the census's `replaced`) -----------------------------------
    // Four blocks of the 796 were replaced. One within the linestyle library,
    // by a block that says the same words. Three of the linestyle library's
    // by the symbol library's, read last, each a different definition - and
    // two of those three names are what four rules give as their linestyle,
    // while no rule places any of them as a symbol.
    ASSERT_EQ(report.replaced.size(), 4u);
    std::size_t withinAFile = 0;
    std::size_t acrossFiles = 0;
    std::size_t theSame = 0;
    std::size_t linestyleRules = 0;
    std::size_t namesWithLinestyleRules = 0;
    std::size_t symbolRules = 0;
    for (const a12::ReplacedDefinition& entry : report.replaced) {
        theSame += entry.differs ? 0 : 1;
        if (entry.keptFrom == entry.droppedFrom) {
            ++withinAFile;
            EXPECT_EQ(entry.keptAsSymbol, entry.droppedAsSymbol) << entry.name;
            continue;
        }
        ++acrossFiles;
        EXPECT_TRUE(entry.keptAsSymbol) << entry.name;
        EXPECT_FALSE(entry.droppedAsSymbol) << entry.name;
        EXPECT_TRUE(entry.differs) << entry.name;
        linestyleRules += entry.linestyleRules;
        namesWithLinestyleRules += entry.linestyleRules != 0 ? 1 : 0;
        symbolRules += entry.symbolRules;
    }
    EXPECT_EQ(withinAFile, 1u);
    EXPECT_EQ(acrossFiles, 3u);
    EXPECT_EQ(theSame, 1u);
    EXPECT_EQ(linestyleRules, 4u);
    EXPECT_EQ(namesWithLinestyleRules, 2u);
    EXPECT_EQ(symbolRules, 0u);

    // ---- the notice, the warnings and the colours -------------------------------
    // Each library opens with a block of eight comment lines, and the two
    // blocks differ (the census's `noticeLines`).
    EXPECT_EQ(nsw.notice.size(), 16u);
    // Four warnings. The reader's one, and it is right: an <item> that names
    // no code. The linestyle library, which is neither UTF-8 nor marked as
    // UTF-16 and holds 82 characters outside ASCII (`inferredEncoding`): said
    // once, though the file is named twice - to convert and for its notice.
    // And the two names kept as symbols that rules give as their linestyle.
    ASSERT_EQ(report.warnings.size(), 4u);
    const auto warningsHolding = [&](std::string_view piece) {
        return std::count_if(report.warnings.begin(), report.warnings.end(),
                             [&](const std::string& warning) {
                                 return warning.find(piece) != std::string::npos;
                             });
    };
    EXPECT_EQ(warningsHolding("has no <key>"), 1);
    EXPECT_EQ(warningsHolding(": neither UTF-8 nor marked as UTF-16, so read as Windows-1252 by "
                              "inference; characters outside ASCII that rest on it: 82"),
              1);
    EXPECT_EQ(warningsHolding(" is kept as a symbol in place of the linestyle of "), 2);
    EXPECT_EQ(warningsHolding("; rules naming it as their linestyle: 2 (the library given last "
                              "is the one kept)"),
              2);
    if (reference->colours) {
        // The census's `colours`, given the table: the rules, their symbols
        // and their texts use 22 names by fold. Nine are standard names;
        // seven the table gives a colour; six are plot pens, and stay names.
        EXPECT_EQ(report.coloursResolved.size(), 7u);
        EXPECT_EQ(nsw.colours.size(), 7u);
        EXPECT_EQ(report.coloursUnresolved.size(), 6u);
        for (const std::string& name : report.coloursUnresolved) {
            EXPECT_TRUE(looksLikeAPlotPen(name)) << name;
        }
        // And two of the nine standard names the table gives another colour
        // than the standard one, which stands.
        EXPECT_EQ(report.coloursKeptStandard.size(), 2u);
    }
}
