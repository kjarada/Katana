// Writing a 12d style library (.4d) and a mapfile, tested as the inverses of
// their readers: read(write(x)) == x.

#include <gtest/gtest.h>

#include <filesystem>
#include <fstream>
#include <map>
#include <sstream>
#include <string>
#include <vector>

#include "katana/archive12d/map_file.hpp"
#include "katana/archive12d/style_library.hpp"
#include "katana/core/text_encoding.hpp"

#include "../reference_files.hpp"

namespace a12 = katana::archive12d;
using katana::entity::LineStyle;
using katana::entity::StyleLibrary;
using katana::entity::SurveyRule;

namespace {

const std::filesystem::path kFixture =
    std::filesystem::path(KATANA_ARCHIVE12D_TEST_DATA) / "customisation";

std::string fileText(const std::filesystem::path& path)
{
    std::ifstream file(path, std::ios::binary);
    std::ostringstream bytes;
    bytes << file.rdbuf();
    auto decoded = katana::core::decodeText(bytes.str());
    EXPECT_TRUE(decoded.ok()) << path;
    return decoded.ok() ? decoded->text : std::string{};
}

a12::StyleLibraryRead readLibrary(std::string_view text, std::string_view source = {})
{
    auto read = a12::readStyleLibrary(text, source);
    EXPECT_TRUE(read.ok()) << (read.ok() ? "" : read.error().describe());
    return read.ok() ? std::move(*read) : a12::StyleLibraryRead{};
}

std::string writeLibrary(const StyleLibrary& library, const a12::StyleLibraryWriteOptions& options = {})
{
    auto written = a12::writeStyleLibrary(library, options);
    EXPECT_TRUE(written.ok()) << (written.ok() ? "" : written.error().describe());
    return written.ok() ? *written : std::string{};
}

a12::MapFileRead readMap(std::string_view text)
{
    auto read = a12::readMapFile(text);
    EXPECT_TRUE(read.ok()) << (read.ok() ? "" : read.error().describe());
    return read.ok() ? std::move(*read) : a12::MapFileRead{};
}

std::string writeMap(const katana::entity::SurveyMap& map, const a12::MapFileWriteOptions& options = {})
{
    auto written = a12::writeMapFile(map, options);
    EXPECT_TRUE(written.ok()) << (written.ok() ? "" : written.error().describe());
    return written.ok() ? *written : std::string{};
}

std::string wrap(const std::string& sections)
{
    return "<xml12d><map_file><version>10.0</version>" + sections + "</map_file></xml12d>";
}

// Every field the .4d reader keeps, in one library.
constexpr const char* kEveryField = R"(// licence line one
// licence line two
worldstyle "W Everything" {
    group  "Survey/W"
    mode vertex
    length 2.5
    factor 3
    xorigin 1
    yorigin -2
    colour "pen 035"
    move 0 0
    draw 3 0.1
    arc -1.75 0 180
    circle 0.5
    dot 0
    text "W \"M\" \\ 1" 90 1.5 "middle-centre" "Arial" 0.85 0 -0.3 0.035
    colour "view_colour"
    move 0.00001 12345678.125
}
paperstyle "P Dashes" { length 4 move 0 0 draw 1.5 0 move 2 0 draw 3 0 move 4 0 }
twoptstyle "T Gate" { xorigin1 0 yorigin1 0.75 xorigin2 14 yorigin2 0.75
                      stretch_mode 2 cycle_mode 1 move 0 0 draw 14 0 }
twoptstyle "T Degenerate" { move 0 0 }
worldstyle "Anchored World" { xorigin2 5 cycle_mode 3 move 0 0 }
worldstyle "Empty" { }
)";

} // namespace

// ---- .4d ------------------------------------------------------------------------------------------

TEST(StyleLibraryWriter, EveryFieldOfEveryKindComesBackAsItWas)
{
    const auto original = readLibrary(kEveryField, "every.4d");
    ASSERT_EQ(original.library.size(), 6u) << (original.warnings.empty() ? "" : original.warnings[0]);

    a12::StyleLibraryWriteOptions options;
    options.comments = original.comments;
    const std::string text = writeLibrary(original.library, options);
    const auto again = readLibrary(text, "every.4d");
    EXPECT_TRUE(again.warnings.empty()) << again.warnings.front() << "\n" << text;
    EXPECT_EQ(again.library.all(), original.library.all()) << text;
    EXPECT_EQ(again.comments, original.comments);
}

TEST(StyleLibraryWriter, NumbersAreWrittenPlainAndReadBackExactly)
{
    const auto original = readLibrary(kEveryField);
    const std::string text = writeLibrary(original.library);
    // 0.00001 would be "1e-05" in general notation; 12d's files never use an
    // exponent, so neither does this.
    EXPECT_NE(text.find("move 0.00001 12345678.125"), std::string::npos) << text;
    EXPECT_EQ(text.find('e' + std::string("-0")), std::string::npos) << text;
    // A negative radius keeps its sign, and a text keeps its three numbers.
    EXPECT_NE(text.find("arc -1.75 0 180"), std::string::npos) << text;
    EXPECT_NE(text.find("0.85 0 -0.3 0.035"), std::string::npos) << text;
}

TEST(StyleLibraryWriter, QuotesAndBackslashesInTextAreEscapedAndComeBack)
{
    const auto original = readLibrary(kEveryField);
    const LineStyle* style = original.library.find("W Everything");
    ASSERT_NE(style, nullptr);
    ASSERT_EQ(style->texts.size(), 1u);
    EXPECT_EQ(style->texts[0].text, "W \"M\" \\ 1") << "the reader resolved the escapes";
    const auto again = readLibrary(writeLibrary(original.library));
    EXPECT_EQ(again.library.find("W Everything")->texts[0].text, "W \"M\" \\ 1");
}

TEST(StyleLibraryWriter, ASubsetWritesOnlyTheNamedDefinitionsAndAnUnknownNameFails)
{
    const auto original = readLibrary(kEveryField);
    a12::StyleLibraryWriteOptions options;
    options.names = {"T Gate", "P Dashes"};
    const auto again = readLibrary(writeLibrary(original.library, options));
    EXPECT_EQ(again.library.names(), (std::vector<std::string>{"P Dashes", "T Gate"}));

    options.names = {"T Gate", "No Such Style"};
    const auto refused = a12::writeStyleLibrary(original.library, options);
    ASSERT_FALSE(refused.ok());
    EXPECT_NE(refused.error().describe().find("No Such Style"), std::string::npos)
        << refused.error().describe();
}

TEST(StyleLibraryWriter, ACommentThatWouldSpillIntoTheDefinitionsIsRefused)
{
    a12::StyleLibraryWriteOptions options;
    options.comments = {"fine", "not fine\nworldstyle \"Injected\" { }"};
    EXPECT_FALSE(a12::writeStyleLibrary(StyleLibrary{}, options).ok());
}

TEST(StyleLibraryWriter, TheFixtureLibrariesComeBackAsTheyWere)
{
    for (const char* name : {"test_symbols.4d", "test_linestyles.4d"}) {
        const auto original = readLibrary(fileText(kFixture / name), name);
        ASSERT_FALSE(original.library.empty()) << name;
        a12::StyleLibraryWriteOptions options;
        options.comments = original.comments;
        const auto again = readLibrary(writeLibrary(original.library, options), name);
        EXPECT_EQ(again.library.all(), original.library.all()) << name;
        EXPECT_EQ(again.comments, original.comments) << name;
    }
}

TEST(StyleLibraryWriter, TheReferenceLibrariesComeBackAsTheyWere)
{
    const auto reference = katana::testing::referenceCustomisation();
    if (reference.libraries.empty()) {
        GTEST_SKIP() << "the reference customisation is not in this checkout";
    }
    for (const std::string& text : reference.libraries) {
        const auto original = readLibrary(text, "reference.4d");
        const auto again = readLibrary(writeLibrary(original.library), "reference.4d");
        EXPECT_TRUE(again.warnings.empty()) << again.warnings.front();
        EXPECT_EQ(again.library.size(), original.library.size());
        EXPECT_TRUE(again.library.all() == original.library.all())
            << "a definition changed on the way through the writer";
    }
}

// ---- mapfile --------------------------------------------------------------------------------------

namespace {

// Every section, and in each every field the reader keeps.
const std::string kEverySection = wrap(R"(
<comments><item>First comment</item><item>A &amp; B &lt;c&gt; "d" 'e'</item></comments>
<map_data>
  <item><key>WM*</key><model>SURVEY SERVICES</model><colour>blue</colour>
        <breakline>Line</breakline><linestyle>WATR Main</linestyle><weight>0</weight>
        <comment>[WM*] Water main</comment><group>G - SERVICES</group></item>
  <item><key>PT</key><breakline>point</breakline></item>
</map_data>
<vertex_symbol_data>
  <item><key>AC*</key>
    <symbol_data><style>CULT Bollard</style><colour>white</colour><size>1.5</size>
      <rotation>45</rotation><offset>0.2</offset><raise>-0.1</raise></symbol_data>
    <hide>yes</hide><comment>bollard</comment></item>
  <item><key>AD*</key><symbol_data><style>Plain</style><size/><rotation/></symbol_data></item>
</vertex_symbol_data>
<tinable_data><item><key>TN*</key><tinable>no</tinable></item></tinable_data>
<vertex_textstyle_data>
  <item><key>1</key>
    <textstyle_data><textstyle>ISO</textstyle><colour>red</colour><type>paper</type>
      <size>1.5</size><justify_x>left</justify_x><justify_y>bottom</justify_y>
      <offset>0.5</offset><raise>0.25</raise><angle>30</angle><slant>10</slant>
      <x_factor>0.8</x_factor><underline>yes</underline><strikeout>yes</strikeout>
      <italic>yes</italic><weight>Normal</weight></textstyle_data>
    <comment>text</comment></item>
</vertex_textstyle_data>
<pipe_data>
  <item><key>*</key>
    <attributes><text><name>DepthLocation</name><value>Top of Pipe</value></text></attributes>
    <justify>Obvert</justify><shape>diameter</shape><size1>$PipeDiameter</size1>
    <size2>0.3</size2><active>yes</active></item>
</pipe_data>
<vertex_pipe_data>
  <item><key>VP*</key>
    <vertex_attributes><integer><name>N</name><value>2</value></integer></vertex_attributes>
    <justify>Invert</justify></item>
</vertex_pipe_data>
<segment_pipe_data>
  <item><key>SP*</key>
    <segment_attributes><text><name>S</name><value></value></text></segment_attributes>
    <shape>culvert</shape></item>
</segment_pipe_data>
<string_attribute_data>
  <item><key>LP*</key>
    <map_attributes><integer><name>Zone</name><value>1</value></integer>
                    <text><name>Owner</name><value>Council &amp; Co</value></text></map_attributes>
    <comment>zone</comment></item>
</string_attribute_data>
<vertex_attribute_data>
  <item><key>PNAL</key>
    <map_attributes><text><name>OLD CODES</name><value>PNAL</value></text></map_attributes></item>
</vertex_attribute_data>)");

} // namespace

TEST(MapFileWriter, EverySectionAndEveryFieldComesBackAsItWas)
{
    const auto original = readMap(kEverySection);
    ASSERT_TRUE(original.warnings.empty()) << original.warnings.front();
    ASSERT_EQ(original.map.size(), 11u);

    const std::string text = writeMap(original.map, {original.version, original.comments});
    const auto again = readMap(text);
    EXPECT_TRUE(again.warnings.empty()) << again.warnings.front() << "\n" << text;
    EXPECT_EQ(again.map.rules(), original.map.rules()) << text;
    EXPECT_EQ(again.version, "10.0");
    EXPECT_EQ(again.comments, original.comments);
}

TEST(MapFileWriter, WhatARuleDoesNotSayIsWrittenAsNoElementNotAsZero)
{
    const auto original = readMap(kEverySection);
    const std::string text = writeMap(original.map);
    // AD*'s symbol said nothing of its size or rotation, and PT says nothing
    // but its breakline: none of those may come back as a 0 or an empty
    // element that a person reading the file would take for a value.
    const std::size_t plain = text.find("<style>Plain</style>");
    ASSERT_NE(plain, std::string::npos) << text;
    const std::size_t end = text.find("</symbol_data>", plain);
    const std::string symbol = text.substr(plain, end - plain);
    EXPECT_EQ(symbol.find("<size"), std::string::npos) << symbol;
    EXPECT_EQ(symbol.find("<rotation"), std::string::npos) << symbol;
    EXPECT_EQ(text.find("<hide>", end), std::string::npos) << "AD* has no hide flag";

    const std::size_t pt = text.find("<key>PT</key>");
    ASSERT_NE(pt, std::string::npos);
    const std::string item = text.substr(pt, text.find("</item>", pt) - pt);
    EXPECT_EQ(item.find("<model"), std::string::npos) << item;
    EXPECT_EQ(item.find("<weight"), std::string::npos) << item;
    EXPECT_NE(item.find("<breakline>Point</breakline>"), std::string::npos) << item;
}

TEST(MapFileWriter, TextIsEscapedForXml)
{
    const auto original = readMap(kEverySection);
    const std::string text = writeMap(original.map, {original.version, original.comments});
    EXPECT_NE(text.find("A &amp; B &lt;c&gt; &quot;d&quot; &apos;e&apos;"), std::string::npos)
        << text;
    EXPECT_NE(text.find("Council &amp; Co"), std::string::npos);
}

TEST(MapFileWriter, SectionsAreWrittenIn12dsOrderAndRulesKeepTheirOrderWithinOne)
{
    // Added deliberately out of order: a symbol, then two map rules, then a
    // tinable rule - each of a DIFFERENT key, so putting them in 12d's order
    // moves no rule ahead of an earlier rule of its own key (for that, see
    // ARuleIsNeverWrittenAheadOfAnEarlierRuleOfItsOwnKey).
    katana::entity::SurveyMap map;
    SurveyRule symbol;
    symbol.key = "PX*";
    symbol.section = katana::entity::SurveySection::VertexSymbol;
    symbol.symbol = katana::entity::SurveySymbol{.style = "S"};
    ASSERT_TRUE(map.add(symbol).ok());
    SurveyRule second;
    second.key = "WM*";
    second.model = "B";
    ASSERT_TRUE(map.add(second).ok());
    SurveyRule first = second;
    first.key = "AC*";
    first.model = "A";
    ASSERT_TRUE(map.add(first).ok());
    SurveyRule tin;
    tin.key = "TN*";
    tin.section = katana::entity::SurveySection::Tinable;
    tin.tinable = false;
    ASSERT_TRUE(map.add(tin).ok());

    const std::string text = writeMap(map);
    const std::size_t mapData = text.find("<map_data>");
    const std::size_t symbols = text.find("<vertex_symbol_data>");
    const std::size_t tinable = text.find("<tinable_data>");
    ASSERT_NE(mapData, std::string::npos);
    EXPECT_LT(mapData, symbols);
    EXPECT_LT(symbols, tinable);
    EXPECT_LT(text.find("<key>WM*</key>"), text.find("<key>AC*</key>", mapData))
        << "within map_data, the order the rules were added";

    const auto again = readMap(text);
    ASSERT_EQ(again.map.size(), 4u);
    EXPECT_EQ(again.map.rules()[0], second);
    EXPECT_EQ(again.map.rules()[1], first);
    EXPECT_EQ(again.map.rules()[2], symbol);
    EXPECT_EQ(again.map.rules()[3], tin);
}

TEST(MapFileWriter, ARuleHoldingSomethingItsSectionCannotCarryIsRefusedByName)
{
    katana::entity::SurveyMap map;
    SurveyRule rule;
    rule.key = "WM*";
    rule.model = "M";
    rule.symbol = katana::entity::SurveySymbol{.style = "S"}; // map_data has no symbol
    ASSERT_TRUE(map.add(rule).ok());
    const auto refused = a12::writeMapFile(map);
    ASSERT_FALSE(refused.ok());
    EXPECT_NE(refused.error().describe().find("WM*"), std::string::npos)
        << refused.error().describe();
}

TEST(MapFileWriter, TextXmlWouldNotGiveBackIsRefusedRatherThanWrittenWrong)
{
    const auto refusedFor = [](const std::string& comment) {
        katana::entity::SurveyMap map;
        SurveyRule rule;
        rule.key = "WM*";
        rule.comment = comment;
        EXPECT_TRUE(map.add(rule).ok());
        return !a12::writeMapFile(map).ok();
    };
    EXPECT_TRUE(refusedFor(" leading space")) << "XML text is trimmed on reading";
    EXPECT_TRUE(refusedFor("trailing space "));
    EXPECT_TRUE(refusedFor(std::string("bell \x07 inside"))) << "XML 1.0 cannot carry it";
    EXPECT_FALSE(refusedFor("inner  spaces\tand a tab are fine"));

    katana::entity::SurveyMap map;
    SurveyRule rule;
    rule.key = "LP*";
    rule.section = katana::entity::SurveySection::StringAttribute;
    rule.attributes = {{"real", "Depth", "1.5"}};
    ASSERT_TRUE(map.add(rule).ok());
    EXPECT_FALSE(a12::writeMapFile(map).ok()) << "a mapfile holds text and integer attributes only";
}

TEST(MapFileWriter, TheFixtureMapfileComesBackAsItWasThroughUtf16)
{
    const auto original = readMap(fileText(kFixture / "test_survey.mapfile"));
    ASSERT_EQ(original.map.size(), 11u);
    const std::string text = writeMap(original.map, {original.version, original.comments});

    // As 12d writes it: UTF-16 little-endian behind a byte order mark.
    const auto bytes = katana::core::encodeUtf16LittleEndian(text);
    ASSERT_TRUE(bytes.ok());
    ASSERT_GE(bytes->size(), 2u);
    EXPECT_EQ(static_cast<unsigned char>((*bytes)[0]), 0xFFu);
    EXPECT_EQ(static_cast<unsigned char>((*bytes)[1]), 0xFEu);
    const auto decoded = katana::core::decodeText(*bytes);
    ASSERT_TRUE(decoded.ok());

    const auto again = readMap(decoded->text);
    EXPECT_EQ(again.map.rules(), original.map.rules());
    EXPECT_EQ(again.comments, original.comments);
    EXPECT_EQ(again.version, "11.0");
}

TEST(MapFileWriter, TheReferenceMapfilesComeBackAsTheyWere)
{
    const auto reference = katana::testing::referenceCustomisation();
    if (reference.mapfiles.empty()) {
        GTEST_SKIP() << "the reference customisation is not in this checkout";
    }
    for (const std::string& text : reference.mapfiles) {
        const auto original = readMap(text);
        const std::string written = writeMap(original.map, {original.version, original.comments});
        const auto again = readMap(written);
        EXPECT_EQ(again.map.size(), original.map.size());
        EXPECT_TRUE(again.map.rules() == original.map.rules())
            << "a rule changed on the way through the writer";
        EXPECT_EQ(again.comments, original.comments);
        EXPECT_EQ(again.version, original.version);
    }
}

namespace {

// Each key's rules, in map order: the order that decides what a code
// resolves to (rules of DIFFERENT keys never compete - see writeMapFile).
std::map<std::string, std::vector<SurveyRule>> rulesByKey(const katana::entity::SurveyMap& map)
{
    std::map<std::string, std::vector<SurveyRule>> out;
    for (const SurveyRule& rule : map.rules()) {
        out[rule.key].push_back(rule);
    }
    return out;
}

// The section elements of a written mapfile, in the order they are written:
// the elements the writer indents two levels, less the three there that are
// not sections.
std::vector<std::string> sectionElements(const std::string& text)
{
    std::vector<std::string> out;
    for (std::size_t at = text.find("\n    <"); at != std::string::npos;
         at = text.find("\n    <", at + 1)) {
        const std::size_t name = at + 6;
        if (text[name] == '/' || text[name] == ' ') {
            continue; // a closing tag, or deeper than two levels
        }
        const std::string element = text.substr(name, text.find_first_of(">/", name) - name);
        if (element != "units" && element != "version" && element != "comments") {
            out.push_back(element);
        }
    }
    return out;
}

// SW*: a string_attribute_data rule, THEN a pipe_data rule, both naming
// Material; AC*: a symbol rule with a comment, THEN a map_data rule with
// another. 12d's section order is the reverse of each pair - and a map made
// by loading one mapfile giving SW* string attributes and then another giving
// it a pipe holds exactly this.
katana::entity::SurveyMap keysOutOf12dsOrder()
{
    katana::entity::SurveyMap map;
    SurveyRule attributes;
    attributes.key = "SW*";
    attributes.section = katana::entity::SurveySection::StringAttribute;
    attributes.attributes = {{"text", "Material", "PVC"}};
    EXPECT_TRUE(map.add(attributes).ok());
    SurveyRule pipe;
    pipe.key = "SW*";
    pipe.section = katana::entity::SurveySection::Pipe;
    pipe.attributes = {{"text", "Material", "$Mat"}};
    pipe.pipe = katana::entity::SurveyPipe{.justify = "Invert", .shape = "diameter", .size1 = "0.3"};
    EXPECT_TRUE(map.add(pipe).ok());
    SurveyRule symbol;
    symbol.key = "AC*";
    symbol.section = katana::entity::SurveySection::VertexSymbol;
    symbol.symbol = katana::entity::SurveySymbol{.style = "S"};
    symbol.comment = "symbol comment";
    EXPECT_TRUE(map.add(symbol).ok());
    SurveyRule where;
    where.key = "AC*";
    where.model = "M";
    where.comment = "map comment";
    EXPECT_TRUE(map.add(where).ok());
    return map;
}

} // namespace

TEST(MapFileWriter, ARuleIsNeverWrittenAheadOfAnEarlierRuleOfItsOwnKey)
{
    const katana::entity::SurveyMap map = keysOutOf12dsOrder();
    // What the map says before it is written: the EARLIER rule of a key wins
    // a field two of its rules fill.
    ASSERT_EQ(map.lookup("SW1").resolved.attributes.size(), 1u);
    ASSERT_EQ(map.lookup("SW1").resolved.attributes[0].value, "PVC");
    ASSERT_EQ(map.lookup("AC1").resolved.comment, "symbol comment");

    const std::string text = writeMap(map);
    const auto again = readMap(text);
    ASSERT_TRUE(again.warnings.empty()) << again.warnings.front();

    // Worked out by hand. A first pass of the sections, in 12d's order, holds
    // AC*'s vertex_symbol_data rule and SW*'s string_attribute_data rule.
    // AC*'s map_data rule and SW*'s pipe_data rule are in sections 12d writes
    // EARLIER than their key's first rule, so they go in a second pass of the
    // sections, after it.
    EXPECT_EQ(sectionElements(text),
              (std::vector<std::string>{"vertex_symbol_data", "string_attribute_data",
                                        "map_data", "pipe_data"}))
        << text;
    EXPECT_TRUE(rulesByKey(again.map) == rulesByKey(map)) << text;
    for (const char* code : {"SW1", "AC1"}) {
        const auto before = map.lookup(code);
        const auto after = again.map.lookup(code);
        EXPECT_TRUE(after.resolved == before.resolved) << code << "\n" << text;
        EXPECT_EQ(after.keys, before.keys) << code;
    }
    ASSERT_EQ(again.map.lookup("SW1").resolved.attributes.size(), 1u);
    EXPECT_EQ(again.map.lookup("SW1").resolved.attributes[0].value, "PVC");
    EXPECT_EQ(again.map.lookup("AC1").resolved.comment, "symbol comment");
}

TEST(MapFileWriter, AMapWhoseKeysAreEachIn12dsOrderHasEachSectionOnce)
{
    // The fixture is one file as 12d writes one: map_data, then
    // vertex_symbol_data, then string_attribute_data, each once.
    const auto original = readMap(fileText(kFixture / "test_survey.mapfile"));
    const std::string text = writeMap(original.map);
    EXPECT_EQ(sectionElements(text),
              (std::vector<std::string>{"map_data", "vertex_symbol_data", "string_attribute_data"}))
        << text;
}

TEST(MapFileWriter, TwoMapfilesLoadedTogetherExportAsOneThatResolvesEveryCodeAlike)
{
    // One file gives SW* its string attributes, a second gives it a pipe
    // naming the same attribute: loaded in that order, the FIRST file's
    // Material and comment are what SW1 gets, and the exported map must say
    // the same.
    auto first = a12::readMapFile(wrap(R"(<string_attribute_data>
  <item><key>SW*</key><map_attributes><text><name>Material</name><value>PVC</value></text>
  </map_attributes><comment>first</comment></item></string_attribute_data>)"));
    ASSERT_TRUE(first.ok());
    auto both = a12::readMapFileInto(std::move(first->map), wrap(R"(<pipe_data>
  <item><key>SW*</key><attributes><text><name>Material</name><value>$Mat</value></text>
  </attributes><justify>Invert</justify><comment>second</comment></item></pipe_data>)"));
    ASSERT_TRUE(both.ok());
    const katana::entity::SurveyMap& map = both->map;
    ASSERT_EQ(map.lookup("SW1").resolved.attributes[0].value, "PVC");

    const auto again = readMap(writeMap(map));
    EXPECT_TRUE(again.map.rules() == map.rules()) << "one key, so its order is the whole order";
    ASSERT_FALSE(again.map.lookup("SW1").resolved.attributes.empty());
    EXPECT_EQ(again.map.lookup("SW1").resolved.attributes[0].value, "PVC");
    EXPECT_EQ(again.map.lookup("SW1").resolved.comment, "first");
}

TEST(MapFileWriter, TheReferenceMapfilesLoadedTogetherExportAsOneThatResolvesEveryKeyAlike)
{
    // The built-in customisation is the reference mapfiles loaded one after
    // the other, which can put a key's rules out of 12d's section order.
    // Exported as one file, every key's rules must come back in their order,
    // so every code resolves as it did.
    const auto reference = katana::testing::referenceCustomisation();
    if (reference.mapfiles.size() < 2) {
        GTEST_SKIP() << "the reference customisation is not in this checkout";
    }
    katana::entity::SurveyMap map;
    for (const std::string& text : reference.mapfiles) {
        auto read = a12::readMapFileInto(std::move(map), text);
        ASSERT_TRUE(read.ok());
        map = std::move(read->map);
    }
    const auto again = readMap(writeMap(map));
    EXPECT_EQ(again.map.size(), map.size());
    EXPECT_TRUE(rulesByKey(again.map) == rulesByKey(map)) << "a key's rules changed order";
    for (const std::string& key : map.keys()) {
        // The shortest code the key matches: the key itself, or its prefix.
        const std::string code = key.back() == '*' ? key.substr(0, key.size() - 1) : key;
        EXPECT_TRUE(again.map.lookup(code).resolved == map.lookup(code).resolved) << key;
    }
}
