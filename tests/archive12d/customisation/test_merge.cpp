// Loading a customisation ON TOP of the one already loaded (audit QT-21 at
// this layer): mergeCustomisation in Merge and Replace mode.
//
// The "current" customisation in these tests stands for the one compiled into
// Katana; the loaded one is the committed fixture (tests/archive12d/data/
// customisation), whose contents and counts are listed in each file's head
// comment. Every expected value below is worked out from those two by hand.

#include <gtest/gtest.h>

#include <algorithm>
#include <filesystem>
#include <string>
#include <utility>
#include <vector>

#include "katana/archive12d/customisation.hpp"

namespace a12 = katana::archive12d;
using katana::entity::SurveySection;

namespace {

const std::filesystem::path kFixture =
    std::filesystem::path(KATANA_ARCHIVE12D_TEST_DATA) / "customisation";

// What is loaded before the load: one definition the fixture also defines
// ("TEST Survey Mark", with a different stroke) and one it does not; a WM*
// map_data rule the fixture also gives, a WM* symbol rule it does not, and a
// ZZ* rule it does not.
constexpr const char* kCurrentLibrary = R"(
worldstyle "TEST Survey Mark" { mode vertex move 0 0 circle 9 }
worldstyle "Kept Style" { move 0 0 draw 1 0 }
)";

constexpr const char* kCurrentMap = R"(<xml12d><map_file><version>11.0</version>
<map_data>
  <item><key>WM*</key><model>OLD SERVICES</model><colour>red</colour></item>
  <item><key>ZZ*</key><model>KEPT</model></item>
</map_data>
<vertex_symbol_data>
  <item><key>WM*</key><symbol_data><style>Kept Style</style></symbol_data></item>
</vertex_symbol_data>
</map_file></xml12d>)";

a12::Customisation current()
{
    auto withLibrary = a12::readCustomisationBytes({}, "builtin.4d", kCurrentLibrary);
    EXPECT_TRUE(withLibrary.ok()) << (withLibrary.ok() ? "" : withLibrary.error().describe());
    auto both = a12::readCustomisationBytes(std::move(*withLibrary), "builtin.mapfile", kCurrentMap);
    EXPECT_TRUE(both.ok()) << (both.ok() ? "" : both.error().describe());
    return std::move(*both);
}

a12::Customisation loadFixture(const std::vector<std::string>& names)
{
    std::vector<std::filesystem::path> paths;
    for (const std::string& name : names) {
        paths.push_back(kFixture / name);
    }
    auto loaded = a12::readCustomisation(paths);
    EXPECT_TRUE(loaded.ok()) << (loaded.ok() ? "" : loaded.error().describe());
    return loaded.ok() ? std::move(*loaded) : a12::Customisation{};
}

const a12::FileMerge* fileNamed(const a12::CustomisationMerge& merge, const std::string& name)
{
    const auto found = std::find_if(merge.files.begin(), merge.files.end(),
                                    [&](const a12::FileMerge& file) { return file.name == name; });
    return found == merge.files.end() ? nullptr : &*found;
}

std::vector<std::string> sorted(std::vector<std::string> names)
{
    std::sort(names.begin(), names.end());
    return names;
}

const std::vector<std::string> kAllFixtureFiles = {"test_linestyles.4d", "test_survey.mapfile",
                                                   "test_symbols.4d"};

} // namespace

TEST(CustomisationMerge, MergingKeepsEveryCurrentDefinitionTheLoadDoesNotReplace)
{
    const a12::Customisation now = current();
    const a12::Customisation loaded = loadFixture(kAllFixtureFiles);
    const auto merged = a12::mergeCustomisation(now.library, now.map, loaded, a12::LoadMode::Merge);

    // 2 current + 7 loaded, less the 1 name both hold ("TEST Survey Mark") = 8.
    EXPECT_TRUE(merged.libraryLoaded);
    EXPECT_EQ(merged.library.size(), 8u);
    ASSERT_NE(merged.library.find("Kept Style"), nullptr);
    EXPECT_EQ(merged.library.find("Kept Style")->source, "builtin.4d");
    // The loaded definition won, and says where it came from.
    const auto* mark = merged.library.find("TEST Survey Mark");
    ASSERT_NE(mark, nullptr);
    EXPECT_EQ(mark->source, "test_symbols.4d");
    EXPECT_TRUE(merged.removedDefinitions.empty()) << "merging removes nothing";
    EXPECT_TRUE(merged.problems.empty());
}

TEST(CustomisationMerge, EachFileReportsTheNamesItAddedAndTheNamesItReplaced)
{
    const a12::Customisation now = current();
    const a12::Customisation loaded = loadFixture(kAllFixtureFiles);
    const auto merged = a12::mergeCustomisation(now.library, now.map, loaded, a12::LoadMode::Merge);

    // One entry per loaded file, in load order.
    ASSERT_EQ(merged.files.size(), 3u);
    EXPECT_EQ(merged.files[0].name, "test_linestyles.4d");
    EXPECT_EQ(merged.files[1].name, "test_survey.mapfile");
    EXPECT_EQ(merged.files[2].name, "test_symbols.4d");

    const auto* lines = fileNamed(merged, "test_linestyles.4d");
    ASSERT_NE(lines, nullptr);
    EXPECT_EQ(lines->kind, a12::CustomisationFile::StyleLibrary);
    EXPECT_EQ(sorted(lines->added),
              (std::vector<std::string>{"TEST Dashed Kerb", "TEST Gate", "TEST Water Main"}));
    EXPECT_TRUE(lines->replaced.empty());

    const auto* symbols = fileNamed(merged, "test_symbols.4d");
    ASSERT_NE(symbols, nullptr);
    EXPECT_EQ(sorted(symbols->added),
              (std::vector<std::string>{"TEST Tree", "TEST U Turn", "TEST Valve"}));
    EXPECT_EQ(symbols->replaced, (std::vector<std::string>{"TEST Survey Mark"}));

    // The mapfile's 11 rules are 11 (section, key) groups: 7 map_data, 3
    // vertex_symbol_data, 1 string_attribute_data. Only WM* map_data was
    // already there; AC*, TR* and PX* are listed twice because the file gives
    // each rules in two sections.
    const auto* map = fileNamed(merged, "test_survey.mapfile");
    ASSERT_NE(map, nullptr);
    EXPECT_EQ(map->kind, a12::CustomisationFile::MapFile);
    EXPECT_EQ(map->replaced, (std::vector<std::string>{"WM*"}));
    EXPECT_EQ(sorted(map->added), (std::vector<std::string>{"*", "1*", "2*", "AC*", "AC*", "KB*",
                                                             "PX*", "PX*", "TR*", "TR*"}));
}

TEST(CustomisationMerge, ALoadedKeyReplacesThatKeysRulesInOneSectionAndNoOther)
{
    const a12::Customisation now = current();
    const a12::Customisation loaded = loadFixture(kAllFixtureFiles);
    const auto merged = a12::mergeCustomisation(now.library, now.map, loaded, a12::LoadMode::Merge);

    // 3 current rules, less the 1 WM* map_data rule the load replaces, plus
    // the 11 loaded = 13.
    EXPECT_TRUE(merged.mapLoaded);
    ASSERT_EQ(merged.map.size(), 13u);

    // WM1 now goes where the fixture sends it, and keeps the symbol the
    // current map gave WM* - the fixture has no WM* symbol rule.
    const auto wm = merged.map.lookup("WM1");
    EXPECT_EQ(wm.resolved.model, "TEST SERVICES");
    EXPECT_EQ(wm.resolved.colour, "blue");
    ASSERT_TRUE(wm.resolved.symbol.has_value());
    EXPECT_EQ(wm.resolved.symbol->style, "Kept Style");
    // A key the load does not mention is untouched.
    EXPECT_EQ(merged.map.lookup("ZZ1").resolved.model, "KEPT");

    // The replacing rules stand where the replaced ones stood; the rest keep
    // their order, and what is new follows in the order it was loaded.
    const auto& rules = merged.map.rules();
    EXPECT_EQ(rules[0].key, "WM*");
    EXPECT_EQ(rules[0].model, "TEST SERVICES");
    EXPECT_EQ(rules[1].key, "ZZ*");
    EXPECT_EQ(rules[2].key, "WM*");
    EXPECT_EQ(rules[2].section, SurveySection::VertexSymbol);
    EXPECT_EQ(rules[3].key, "KB*");
    EXPECT_EQ(rules[12].key, "*");
    EXPECT_EQ(rules[12].section, SurveySection::StringAttribute);
}

TEST(CustomisationMerge, RulesOfOneKeyInALoadedFileAllReplaceAndAreReportedOnce)
{
    const a12::Customisation now = current();
    // Two WM* map_data rules in one file: the earlier wins a field both set
    // (SurveyMap::add), so both must come in, in their order, and the old
    // WM* map_data rule must go - left in front it would outrank them both.
    auto loaded = a12::readCustomisationBytes({}, "mine.mapfile", R"(<xml12d><map_file>
<map_data>
  <item><key>WM*</key><model>MINE</model></item>
  <item><key>WM*</key><model>SHADOWED</model><colour>green</colour></item>
</map_data></map_file></xml12d>)");
    ASSERT_TRUE(loaded.ok()) << loaded.error().describe();
    const auto merged = a12::mergeCustomisation(now.library, now.map, *loaded, a12::LoadMode::Merge);

    // 3 current - 1 WM* map_data + 2 = 4.
    ASSERT_EQ(merged.map.size(), 4u);
    const auto wm = merged.map.lookup("WM1");
    EXPECT_EQ(wm.resolved.model, "MINE");
    EXPECT_EQ(wm.resolved.colour, "green") << "the second rule supplies what the first leaves out";
    ASSERT_EQ(merged.files.size(), 1u);
    EXPECT_EQ(merged.files[0].replaced, (std::vector<std::string>{"WM*"}));
    EXPECT_TRUE(merged.files[0].added.empty());
}

TEST(CustomisationMerge, ASymbolFileAloneLeavesTheSurveyMapAsItWas)
{
    // QT-21: loading a personal symbol file used to install its EMPTY map
    // over the built-in survey codes.
    const a12::Customisation now = current();
    const a12::Customisation loaded = loadFixture({"test_symbols.4d"});
    for (const auto mode : {a12::LoadMode::Merge, a12::LoadMode::Replace}) {
        const auto merged = a12::mergeCustomisation(now.library, now.map, loaded, mode);
        EXPECT_FALSE(merged.mapLoaded) << a12::toString(mode);
        EXPECT_EQ(merged.map.rules(), now.map.rules()) << a12::toString(mode);
        EXPECT_TRUE(merged.removedKeys.empty()) << a12::toString(mode);
    }
}

TEST(CustomisationMerge, AMapfileAloneLeavesTheLibraryAsItWas)
{
    const a12::Customisation now = current();
    const a12::Customisation loaded = loadFixture({"test_survey.mapfile"});
    for (const auto mode : {a12::LoadMode::Merge, a12::LoadMode::Replace}) {
        const auto merged = a12::mergeCustomisation(now.library, now.map, loaded, mode);
        EXPECT_FALSE(merged.libraryLoaded) << a12::toString(mode);
        EXPECT_EQ(merged.library.all(), now.library.all()) << a12::toString(mode);
        EXPECT_TRUE(merged.removedDefinitions.empty()) << a12::toString(mode);
    }
}

TEST(CustomisationMerge, ALoadThatBroughtNothingChangesNothing)
{
    const a12::Customisation now = current();
    const auto merged =
        a12::mergeCustomisation(now.library, now.map, a12::Customisation{}, a12::LoadMode::Replace);
    EXPECT_FALSE(merged.libraryLoaded);
    EXPECT_FALSE(merged.mapLoaded);
    EXPECT_EQ(merged.library.all(), now.library.all());
    EXPECT_EQ(merged.map.rules(), now.map.rules());
    EXPECT_TRUE(merged.files.empty());
}

TEST(CustomisationMerge, ReplacingInstallsOnlyWhatWasLoadedAndListsWhatWentWithIt)
{
    const a12::Customisation now = current();
    const a12::Customisation loaded = loadFixture(kAllFixtureFiles);
    const auto replaced =
        a12::mergeCustomisation(now.library, now.map, loaded, a12::LoadMode::Replace);

    EXPECT_EQ(replaced.library.all(), loaded.library.all());
    EXPECT_EQ(replaced.map.rules(), loaded.map.rules());
    // "Kept Style" is not in the fixture; "TEST Survey Mark" is. WM* is still
    // a key of the loaded map (map_data), so only ZZ* went - even though the
    // WM* symbol rule went with the rest of the old map.
    EXPECT_EQ(replaced.removedDefinitions, (std::vector<std::string>{"Kept Style"}));
    EXPECT_EQ(replaced.removedKeys, (std::vector<std::string>{"ZZ*"}));
    // What each file added or replaced is reported the same way as a merge.
    const auto* symbols = fileNamed(replaced, "test_symbols.4d");
    ASSERT_NE(symbols, nullptr);
    EXPECT_EQ(symbols->replaced, (std::vector<std::string>{"TEST Survey Mark"}));
}

TEST(CustomisationMerge, ALoadFileThatChangesNothingIsStillListed)
{
    // Loading a library twice over itself: the second copy replaces every
    // name, and a file whose definitions all lost to a later file is still
    // named, with nothing against it, so "it was loaded" is on the report.
    auto first = a12::readCustomisationBytes({}, "a.4d", R"(worldstyle "X" { move 0 0 })");
    ASSERT_TRUE(first.ok());
    auto both = a12::readCustomisationBytes(std::move(*first), "b.4d",
                                            R"(worldstyle "X" { move 0 0 draw 1 0 })");
    ASSERT_TRUE(both.ok());
    const auto merged = a12::mergeCustomisation({}, {}, *both, a12::LoadMode::Merge);
    ASSERT_EQ(merged.files.size(), 2u);
    EXPECT_EQ(merged.files[0].name, "a.4d");
    EXPECT_TRUE(merged.files[0].added.empty());
    EXPECT_EQ(merged.files[1].name, "b.4d");
    EXPECT_EQ(merged.files[1].added, (std::vector<std::string>{"X"}));
    ASSERT_NE(merged.library.find("X"), nullptr);
    EXPECT_EQ(merged.library.find("X")->strokes.size(), 2u) << "the later file's definition";
}

namespace {

// Two pairs of sections fill ONE field of a code: pipe_data and
// string_attribute_data both give the string's attributes, vertex_pipe_data
// and vertex_attribute_data both give each vertex's. Where two rules of one
// key name the same attribute the EARLIER rule's value is the one a code gets
// (SurveyMap::lookup), whichever sections they are in.
const std::string kPipeMap = R"(<xml12d><map_file><version>11.0</version>
<pipe_data>
  <item><key>*</key>
    <attributes><text><name>DepthLocation</name><value>Top of Pipe</value></text>
                <text><name>Material</name><value>PVC</value></text></attributes>
    <justify>Obvert</justify></item>
</pipe_data>
<vertex_pipe_data>
  <item><key>SW*</key>
    <vertex_attributes><text><name>Pit</name><value>Grated</value></text></vertex_attributes>
    <justify>Invert</justify></item>
</vertex_pipe_data>
</map_file></xml12d>)";

a12::Customisation mapOnly(const std::string& name, const std::string& text)
{
    auto read = a12::readCustomisationBytes({}, name, text);
    EXPECT_TRUE(read.ok()) << (read.ok() ? "" : read.error().describe());
    return read.ok() ? std::move(*read) : a12::Customisation{};
}

// "type name=value" each, so that a failure prints something readable.
std::vector<std::string> shown(const std::vector<katana::entity::SurveyAttribute>& attributes)
{
    std::vector<std::string> out;
    for (const auto& attribute : attributes) {
        out.push_back(attribute.type + " " + attribute.name + "=" + attribute.value);
    }
    return out;
}

// Each rule's section, in map order.
std::vector<std::string> sections(const katana::entity::SurveyMap& map)
{
    std::vector<std::string> out;
    for (const auto& rule : map.rules()) {
        out.emplace_back(katana::entity::toString(rule.section));
    }
    return out;
}

} // namespace

TEST(CustomisationMerge, ALoadedAttributeWinsOverTheSameAttributeFromTheOtherSectionThatSetsIt)
{
    // The load gives `*` a DepthLocation in string_attribute_data and SW* a
    // Pit in vertex_attribute_data; the current map gives both in the
    // sibling sections, pipe_data and vertex_pipe_data. Neither (section,
    // key) group is in the current map, so both are ADDED - and what the load
    // brings must still be what a code gets.
    const a12::Customisation now = mapOnly("builtin.mapfile", kPipeMap);
    const a12::Customisation loaded = mapOnly("mine.mapfile", R"(<xml12d><map_file>
<string_attribute_data>
  <item><key>*</key>
    <map_attributes><text><name>DepthLocation</name><value>Invert</value></text></map_attributes></item>
</string_attribute_data>
<vertex_attribute_data>
  <item><key>SW*</key>
    <map_attributes><text><name>Pit</name><value>Solid</value></text></map_attributes></item>
</vertex_attribute_data>
</map_file></xml12d>)");
    const auto merged = a12::mergeCustomisation(now.library, now.map, loaded, a12::LoadMode::Merge);

    ASSERT_EQ(merged.files.size(), 1u);
    EXPECT_EQ(merged.files[0].added, (std::vector<std::string>{"*", "SW*"}));
    EXPECT_TRUE(merged.files[0].replaced.empty());
    // 2 current rules, none replaced, + 2 loaded = 4.
    ASSERT_EQ(merged.map.size(), 4u);

    const auto sw = merged.map.lookup("SW1");
    // SW1 meets SW* and then `*`. The string's attributes all come from `*`:
    // DepthLocation from the LOADED string_attribute_data rule, Material -
    // which the load says nothing about - still from the current pipe_data
    // rule, in that order.
    EXPECT_EQ(shown(sw.resolved.attributes),
              (std::vector<std::string>{"text DepthLocation=Invert", "text Material=PVC"}));
    // Each vertex's Pit comes from SW*: the loaded vertex_attribute_data rule,
    // not the current vertex_pipe_data one.
    EXPECT_EQ(shown(sw.resolved.vertexAttributes), (std::vector<std::string>{"text Pit=Solid"}));
    // And the current rules still give what only they say.
    ASSERT_TRUE(sw.resolved.pipe.has_value());
    EXPECT_EQ(sw.resolved.pipe->justify, "Obvert");
    ASSERT_TRUE(sw.resolved.vertexPipe.has_value());
    EXPECT_EQ(sw.resolved.vertexPipe->justify, "Invert");

    // Worked out by hand: at the current map's first `*` rule the loaded `*`
    // rule goes in first, then the current one it leaves standing; the same
    // at SW*.
    EXPECT_EQ(sections(merged.map),
              (std::vector<std::string>{"string_attribute_data", "pipe_data",
                                        "vertex_attribute_data", "vertex_pipe_data"}));
}

TEST(CustomisationMerge, ALoadedGroupReplacingOneInPlaceStillGoesAheadOfItsKeysOtherSections)
{
    // The current map has `*` in pipe_data AND, after it, in
    // string_attribute_data. The load replaces the string_attribute_data
    // group; put back where that group stood, it would stand behind the
    // pipe_data rule and its DepthLocation would lose to "Top of Pipe".
    const a12::Customisation now = mapOnly("builtin.mapfile", R"(<xml12d><map_file>
<pipe_data>
  <item><key>*</key>
    <attributes><text><name>DepthLocation</name><value>Top of Pipe</value></text></attributes>
    <justify>Obvert</justify></item>
</pipe_data>
<string_attribute_data>
  <item><key>*</key>
    <map_attributes><text><name>DepthLocation</name><value>Centre</value></text></map_attributes></item>
</string_attribute_data>
</map_file></xml12d>)");
    const a12::Customisation loaded = mapOnly("mine.mapfile", R"(<xml12d><map_file>
<string_attribute_data>
  <item><key>*</key>
    <map_attributes><text><name>DepthLocation</name><value>Invert</value></text></map_attributes></item>
</string_attribute_data>
</map_file></xml12d>)");
    const auto merged = a12::mergeCustomisation(now.library, now.map, loaded, a12::LoadMode::Merge);

    ASSERT_EQ(merged.files.size(), 1u);
    EXPECT_EQ(merged.files[0].replaced, (std::vector<std::string>{"*"}));
    // 2 current - 1 replaced + 1 loaded = 2: the loaded rule, then pipe_data.
    ASSERT_EQ(merged.map.size(), 2u);
    EXPECT_EQ(sections(merged.map),
              (std::vector<std::string>{"string_attribute_data", "pipe_data"}));
    EXPECT_EQ(shown(merged.map.lookup("ANY").resolved.attributes),
              (std::vector<std::string>{"text DepthLocation=Invert"}));
}
