// Reading a 12d mapfile, and what a survey code resolves to (PLAN.MD 20.3).

#include <gtest/gtest.h>

#include <algorithm>
#include <fstream>
#include <optional>
#include <sstream>
#include <string>

#include "katana/archive12d/map_file.hpp"
#include "katana/archive12d/text_encoding.hpp"

#include "reference_files.hpp"

namespace a12 = katana::archive12d;
using katana::entity::SurveyBreakline;
using katana::entity::SurveyMap;
using katana::entity::SurveyRule;

namespace {

// A mapfile is written in SECTIONS and it is the section, not the fields,
// that says what a rule is about. `sections` is their XML, verbatim.
std::string wrap(const std::string& sections)
{
    return R"(<?xml version="1.0"?>
<xml12d xmlns="http://www.12d.com/schema/xml12d-10.0">
  <map_file>
    <version>11.0</version>)" +
           sections + R"(
  </map_file>
</xml12d>)";
}

std::string mapData(const std::string& items)
{
    return "<map_data>" + items + "</map_data>";
}

a12::MapFileRead read(const std::string& sections)
{
    auto result = a12::readMapFile(wrap(sections));
    EXPECT_TRUE(result.ok()) << (result.ok() ? "" : result.error().describe());
    return result.ok() ? std::move(*result) : a12::MapFileRead{};
}

std::string allWarnings(const a12::MapFileRead& map)
{
    std::string text;
    for (const std::string& warning : map.warnings) {
        text += "\n  " + warning;
    }
    return text;
}

bool anyWarningContains(const a12::MapFileRead& map, std::string_view needle)
{
    return std::any_of(map.warnings.begin(), map.warnings.end(),
                       [needle](const std::string& warning) {
                           return warning.find(needle) != std::string::npos;
                       });
}

} // namespace

TEST(MapFile, AWaterMainIsAModelAColourALineAndALinestyle)
{
    // The rule a water main gets, copied from the reference mapfile.
    const auto map = read(mapData(R"(
      <item>
        <key>WM*</key>
        <model>SURVEY SERVICES</model>
        <colour>sui water potable</colour>
        <breakline>Line</breakline>
        <linestyle>WATR Main</linestyle>
        <weight>0</weight>
        <comment>[WM*] Main</comment>
        <group>SURVEY - WATR</group>
      </item>)"));
    ASSERT_EQ(map.map.size(), 1u) << allWarnings(map);
    EXPECT_EQ(map.version, "11.0");

    const katana::entity::SurveyMatch match = map.map.lookup("WM01");
    ASSERT_FALSE(match.empty()) << "WM01 is a water main";
    EXPECT_EQ(match.keys, (std::vector<std::string>{"WM*"}));
    EXPECT_EQ(match.resolved.model, "SURVEY SERVICES");
    EXPECT_EQ(match.resolved.colour, "sui water potable");
    EXPECT_EQ(match.resolved.linestyle, "WATR Main");
    ASSERT_TRUE(match.resolved.breakline.has_value());
    EXPECT_EQ(*match.resolved.breakline, SurveyBreakline::Line);
    EXPECT_EQ(match.resolved.group, "SURVEY - WATR");

    EXPECT_TRUE(map.map.lookup("SW01").empty()) << "a code no rule covers resolves to nothing";
    EXPECT_TRUE(map.map.lookup("W").empty()) << "shorter than the prefix is not a match";
    EXPECT_FALSE(map.map.lookup("WM").empty()) << "the prefix itself matches";
}

TEST(MapFile, ACodeGetsASymbolAndItsSizeAndRotation)
{
    const auto map = read(R"(<vertex_symbol_data>
      <item>
        <key>AC*</key>
        <symbol_data>
          <style>CULT Bollard</style>
          <colour>white</colour>
          <size>1.5</size>
          <rotation>90</rotation>
          <offset/>
          <raise/>
        </symbol_data>
        <hide>no</hide>
        <comment>E CULT Bollard</comment>
      </item></vertex_symbol_data>)");
    const auto match = map.map.lookup("AC12");
    ASSERT_TRUE(match.resolved.symbol.has_value()) << allWarnings(map);
    EXPECT_EQ(match.resolved.symbol->style, "CULT Bollard");
    EXPECT_EQ(match.resolved.symbol->colour, "white");
    EXPECT_EQ(match.resolved.symbol->size, 1.5);
    EXPECT_EQ(match.resolved.symbol->rotation, 90.0);
    EXPECT_EQ(match.resolved.symbol->offset, 0.0) << "an empty <offset/> is silence, not a value";
    ASSERT_TRUE(match.resolved.hide.has_value());
    EXPECT_FALSE(*match.resolved.hide);
    EXPECT_EQ(map.map.stylesReferenced(), (std::vector<std::string>{"CULT Bollard"}));
}

TEST(MapFile, ACodeTakesTheUnionOfEveryRuleThatMatchesIt)
{
    // 213 keys in the real mapfile carry more than one rule, each about a
    // different aspect: where the code goes, and what it is drawn with.
    const auto map = read(R"(
      <map_data><item><key>AC*</key><model>SURVEY DETAIL</model><colour>green</colour></item></map_data>
      <vertex_symbol_data>
        <item><key>AC*</key><symbol_data><style>CULT Bollard</style><size>1</size></symbol_data></item>
      </vertex_symbol_data>)");
    const auto match = map.map.lookup("AC01");
    EXPECT_EQ(match.keys.size(), 2u);
    EXPECT_EQ(match.resolved.model, "SURVEY DETAIL");
    ASSERT_TRUE(match.resolved.symbol.has_value());
    EXPECT_EQ(match.resolved.symbol->style, "CULT Bollard");
}

TEST(MapFile, TheMoreSpecificKeyWinsAFieldTheyBothSet)
{
    const auto map = read(mapData(R"(
      <item><key>*</key><model>EVERYTHING</model><colour>grey</colour></item>
      <item><key>P*</key><model>SURVEY POINTS</model></item>
      <item><key>PABB</key><model>BENCH MARKS</model></item>)"));
    // Exact beats prefix beats the catch-all, and the catch-all still
    // supplies the colour that neither of the others mentions.
    const auto exact = map.map.lookup("PABB");
    EXPECT_EQ(exact.keys, (std::vector<std::string>{"PABB", "P*", "*"}));
    EXPECT_EQ(exact.resolved.model, "BENCH MARKS");
    EXPECT_EQ(exact.resolved.colour, "grey");

    const auto prefix = map.map.lookup("PXYZ");
    EXPECT_EQ(prefix.keys, (std::vector<std::string>{"P*", "*"}));
    EXPECT_EQ(prefix.resolved.model, "SURVEY POINTS");

    EXPECT_EQ(map.map.lookup("ZZ").resolved.model, "EVERYTHING");
}

TEST(MapFile, ALongerPrefixBeatsAShorterOne)
{
    const auto map = read(mapData(R"(
      <item><key>S*</key><model>SHORT</model></item>
      <item><key>SW*</key><model>STORMWATER</model></item>)"));
    EXPECT_EQ(map.map.lookup("SW01").resolved.model, "STORMWATER");
    EXPECT_EQ(map.map.lookup("SX01").resolved.model, "SHORT");
    EXPECT_EQ(map.map.lookup("SW01").keys, (std::vector<std::string>{"SW*", "S*"}));
}

TEST(MapFile, AttributesAccumulateRatherThanOverrideBecauseBothAreMeant)
{
    const auto map = read(R"(
      <pipe_data>
        <item>
          <key>*</key>
          <attributes><text><name>DepthLocation</name><value>Top of Pipe</value></text></attributes>
          <justify>Obvert</justify><shape>diameter</shape>
          <size1>$PipeDiameter</size1><active>yes</active>
        </item>
      </pipe_data>
      <string_attribute_data>
        <item>
          <key>SW*</key>
          <map_attributes><text><name>Material</name><value>RCP</value></text></map_attributes>
        </item>
      </string_attribute_data>)");
    const auto match = map.map.lookup("SW01");
    ASSERT_EQ(match.resolved.attributes.size(), 2u) << allWarnings(map);
    EXPECT_EQ(match.resolved.attributes[0].name, "Material") << "the more specific rule first";
    EXPECT_EQ(match.resolved.attributes[1].name, "DepthLocation");
    EXPECT_EQ(match.resolved.attributes[1].value, "Top of Pipe");
    ASSERT_TRUE(match.resolved.pipe.has_value());
    EXPECT_EQ(match.resolved.pipe->size1, "$PipeDiameter")
        << "kept verbatim: it names another attribute rather than being a number";
    EXPECT_EQ(match.resolved.pipe->justify, "Obvert");
    EXPECT_TRUE(match.resolved.pipe->active);
}

TEST(MapFile, ATextStyleRuleKeepsEveryFieldItSets)
{
    const auto map = read(R"(<vertex_textstyle_data>
      <item>
        <key>1</key>
        <textstyle_data>
          <textstyle>ISO</textstyle><colour>blue</colour><type>paper</type>
          <size>1.5</size><justify_x>left</justify_x><justify_y>bottom</justify_y>
          <angle>0</angle><slant>0</slant><x_factor>1</x_factor>
          <underline>no</underline><strikeout>no</strikeout><italic>no</italic>
          <weight>Normal</weight>
        </textstyle_data>
        <comment>1.5mm TEXT</comment>
      </item></vertex_textstyle_data>)");
    const auto match = map.map.lookup("1");
    ASSERT_TRUE(match.resolved.textStyle.has_value()) << allWarnings(map);
    const auto& style = *match.resolved.textStyle;
    EXPECT_EQ(style.textstyle, "ISO");
    EXPECT_EQ(style.colour, "blue");
    EXPECT_EQ(style.type, "paper");
    EXPECT_EQ(style.size, 1.5);
    EXPECT_EQ(style.justifyX, "left");
    EXPECT_EQ(style.justifyY, "bottom");
    EXPECT_EQ(style.widthFactor, 1.0);
    EXPECT_FALSE(style.italic);
    EXPECT_EQ(style.weight, "Normal") << "a pen name, not a number";
}

TEST(MapFile, AFieldItDoesNotKnowIsNamedAndTheRuleSurvives)
{
    const auto map = read(mapData(R"(
      <item><key>ZZ*</key><model>M</model><tilt_angle>45</tilt_angle></item>)"));
    ASSERT_EQ(map.map.size(), 1u) << allWarnings(map);
    EXPECT_EQ(map.map.lookup("ZZ1").resolved.model, "M");
    EXPECT_TRUE(anyWarningContains(map, "tilt_angle")) << allWarnings(map);
}

TEST(MapFile, AKeyWithAWildcardInTheMiddleIsRefusedRatherThanMatchedApproximately)
{
    const auto map = read(mapData(R"(
      <item><key>W*M</key><model>M</model></item>
      <item><key>WM*</key><model>WATER</model></item>)"));
    EXPECT_EQ(map.map.size(), 1u) << "the good rule is kept" << allWarnings(map);
    EXPECT_TRUE(anyWarningContains(map, "last character")) << allWarnings(map);
    EXPECT_EQ(map.map.lookup("WM1").resolved.model, "WATER");
}

TEST(MapFile, SomethingThatIsNotAMapfileIsRefusedWithItsReason)
{
    const auto notXml = a12::readMapFile("worldstyle \"S\" { move 0 0 }");
    EXPECT_FALSE(notXml.ok());

    const auto wrongXml = a12::readMapFile("<xml12d><chain_file/></xml12d>");
    ASSERT_FALSE(wrongXml.ok());
    EXPECT_NE(wrongXml.error().describe().find("no <map_file>"), std::string::npos)
        << wrongXml.error().describe();

    const auto unclosed = a12::readMapFile("<xml12d><map_file>");
    EXPECT_FALSE(unclosed.ok());
}

TEST(MapFile, TwoMapfilesLoadAsOneAndTheFirstWinsWhatBothSet)
{
    auto first =
        a12::readMapFile(wrap(mapData("<item><key>WM*</key><model>FIRST</model></item>")));
    ASSERT_TRUE(first.ok());
    auto both = a12::readMapFileInto(
        std::move(first->map),
        wrap(mapData("<item><key>WM*</key><model>SECOND</model><colour>blue</colour></item>")));
    ASSERT_TRUE(both.ok());
    EXPECT_EQ(both->map.size(), 2u);
    const auto match = both->map.lookup("WM1");
    EXPECT_EQ(match.resolved.model, "FIRST");
    EXPECT_EQ(match.resolved.colour, "blue") << "the second still supplies what the first did not";
}

// ---- the customisation this was built against -------------------------------------------------

TEST(MapFile, TheReferenceMapfilesAreReadWhole)
{
    const auto customisation = katana::testing::referenceCustomisation();
    if (customisation.mapfiles.size() < 2) {
        GTEST_SKIP() << "the reference customisation is not in this checkout";
    }

    katana::entity::SurveyMap map;
    std::vector<std::size_t> perFile;
    std::size_t warnings = 0;
    for (const std::string& text : customisation.mapfiles) {
        const std::size_t before = map.size();
        auto read = a12::readMapFileInto(std::move(map), text);
        ASSERT_TRUE(read.ok()) << read.error().describe();
        map = std::move(read->map);
        perFile.push_back(map.size() - before);
        warnings += read->warnings.size();
    }
    std::sort(perFile.begin(), perFile.end());

    // Counted from the files by a script using none of Katana's code. Sorted,
    // so this says nothing about which file is which.
    EXPECT_EQ(perFile, (std::vector<std::size_t>{725u, 899u}));
    EXPECT_EQ(map.size(), 1624u);
    // One warning, and it is right: an <item> holding only a <group>, which
    // names no code and so could never apply to one.
    EXPECT_EQ(warnings, 1u);

    // The rule a water main gets, end to end.
    const auto water = map.lookup("WM01");
    ASSERT_FALSE(water.empty());
    EXPECT_EQ(water.resolved.model, "SURVEY SERVICES");
    EXPECT_EQ(water.resolved.linestyle, "WATR Main");
    ASSERT_TRUE(water.resolved.breakline.has_value());
    EXPECT_EQ(*water.resolved.breakline, SurveyBreakline::Line);
    EXPECT_GE(map.stylesReferenced().size(), 200u);
}

// ---- the XML a hand-edited mapfile might contain ----------------------------------------------
//
// The mapfile reader has its own small XML reader rather than a linked one,
// because this module deliberately has no third-party dependency - it is the
// one that builds with -DKATANA_BUILD_IO=OFF and so the one the sanitizer
// job covers. These are its edges, exercised through the only door it has.

TEST(MapFile, EntitiesCommentsAndCdataAreRead)
{
    const auto map = read(R"(<!-- which codes are fences -->
      <map_data>
        <item>
          <key>F&amp;G*</key>
          <model>FENCES &amp; GATES</model>
          <comment><![CDATA[a <comment> with markup in it]]></comment>
          <colour>&#82;ed</colour>
        </item>
      </map_data>)");
    ASSERT_EQ(map.map.size(), 1u) << allWarnings(map);
    const auto match = map.map.lookup("F&G1");
    EXPECT_EQ(match.resolved.model, "FENCES & GATES");
    EXPECT_EQ(match.resolved.comment, "a <comment> with markup in it");
    EXPECT_EQ(match.resolved.colour, "Red") << "&#82; is a capital R";
}

TEST(MapFile, MalformedXmlIsRefusedWithWhereItWentWrong)
{
    struct Case {
        const char* text;
        const char* says;
    };
    // Each is refused rather than half-read: a mapfile whose rules are
    // silently half-read draws the wrong thing.
    for (const Case& bad : {
             Case{"<a><b></a></b>", "closed by"},
             Case{"<a>&nosuch;</a>", "entity"},
             Case{"<a", "never closed"},
             Case{"<a x=1></a>", "not in quotes"},
             Case{"<!DOCTYPE a><a/>", "document type"},
             Case{"<a><!-- never ends", "comment"},
         }) {
        const auto result = a12::readMapFile(bad.text);
        ASSERT_FALSE(result.ok()) << bad.text;
        EXPECT_NE(result.error().describe().find(bad.says), std::string::npos)
            << bad.text << " -> " << result.error().describe();
    }
}

TEST(MapFile, ASectionItDoesNotKnowIsNamedWithHowMuchWasNotRead)
{
    const auto map = read(R"(<map_data><item><key>A*</key><model>M</model></item></map_data>
      <chainage_data><item><key>B*</key></item><item><key>C*</key></item></chainage_data>)");
    EXPECT_EQ(map.map.size(), 1u) << allWarnings(map);
    EXPECT_TRUE(anyWarningContains(map, "chainage_data")) << allWarnings(map);
    EXPECT_TRUE(anyWarningContains(map, "2 rules")) << allWarnings(map);
}
