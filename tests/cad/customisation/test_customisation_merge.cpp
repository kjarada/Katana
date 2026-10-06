// Loading customisations ON TOP of the one a session has: cad::mergeCustomisation
// in Merge and Replace mode (customisation_merge.hpp).
//
// The first twelve tests are the merge's own, moved here with it from the
// reader of the older formats, where the same rule was written over style
// libraries and survey code files. Their expectations are the ones those tests
// worked out by hand and are unchanged; what changed is how the inputs are
// made. The "current" customisation stands for the one a session starts with,
// and the three loaded ones are that suite's hand-written fixture - a
// linestyle library, a survey code file and a symbol library - as three
// customisations of the same names, built here in code:
//
//   test_linestyles  3 definitions, none listed as a symbol
//   test_survey      11 rules in 11 (section, key) groups over 8 keys:
//                    7 feature rules (WM* KB* AC* TR* 1* 2* PX*), 3 symbol
//                    rules (AC* TR* PX*), 1 string attribute rule (*)
//   test_symbols     4 definitions, all listed as symbols
//
// The strokes are cut down to one or two: the merge reads none of them.

#include <gtest/gtest.h>

#include <algorithm>
#include <optional>
#include <string>
#include <utility>
#include <vector>

#include "katana/cad/customisation_merge.hpp"
#include "katana/cad/customisation_record.hpp"

using katana::cad::CustomisationLoad;
using katana::cad::CustomisationMerge;
using katana::cad::CustomisationSource;
using katana::cad::LoadMode;
using katana::cad::mergeCustomisation;
using katana::entity::Color;
using katana::entity::Customisation;
using katana::entity::LineStyle;
using katana::entity::Stroke;
using katana::entity::StrokeOp;
using katana::entity::SurveyAttribute;
using katana::entity::SurveyMap;
using katana::entity::SurveyPipe;
using katana::entity::SurveyRule;
using katana::entity::SurveySection;

namespace {

Stroke moveTo(double x, double y)
{
    Stroke stroke;
    stroke.op = StrokeOp::Move;
    stroke.point = {x, y};
    return stroke;
}

Stroke drawTo(double x, double y)
{
    Stroke stroke;
    stroke.op = StrokeOp::Draw;
    stroke.point = {x, y};
    return stroke;
}

Stroke circle(double radius)
{
    Stroke stroke;
    stroke.op = StrokeOp::Circle;
    stroke.radius = radius;
    return stroke;
}

// A definition of `customisation`, which is its source as a file's reader
// stamps it; `symbol` is the array of the file it would sit in.
void define(Customisation& customisation, const char* name, bool symbol, bool atVertices,
            std::vector<Stroke> strokes)
{
    LineStyle definition;
    definition.name = name;
    definition.source = customisation.name;
    definition.symbol = symbol;
    definition.atVertices = atVertices;
    definition.strokes = std::move(strokes);
    const auto added = customisation.library.add(std::move(definition));
    ASSERT_TRUE(added.ok()) << added.error().describe();
}

void rule(Customisation& customisation, SurveyRule made)
{
    const auto added = customisation.map.add(std::move(made));
    ASSERT_TRUE(added.ok()) << added.error().describe();
}

SurveyRule feature(const char* key, const char* model, const char* colour = "",
                   const char* linestyle = "")
{
    SurveyRule made;
    made.key = key;
    made.section = SurveySection::Map;
    made.model = model;
    made.colour = colour;
    made.linestyle = linestyle;
    return made;
}

SurveyRule symbolOf(const char* key, const char* style)
{
    SurveyRule made;
    made.key = key;
    made.section = SurveySection::VertexSymbol;
    made.symbol = katana::entity::SurveySymbol{.style = style};
    return made;
}

// A rule of one of the four sections that give attributes: the two pipe
// sections also say how the pipe is justified.
SurveyRule attributesOf(SurveySection section, const char* key,
                        std::vector<SurveyAttribute> attributes, const char* justify = "")
{
    SurveyRule made;
    made.key = key;
    made.section = section;
    switch (section) {
    case SurveySection::Pipe:
        made.attributes = std::move(attributes);
        made.pipe = SurveyPipe{.justify = justify};
        break;
    case SurveySection::StringAttribute:
        made.attributes = std::move(attributes);
        break;
    case SurveySection::VertexPipe:
        made.vertexAttributes = std::move(attributes);
        made.vertexPipe = SurveyPipe{.justify = justify};
        break;
    case SurveySection::VertexAttribute:
        made.vertexAttributes = std::move(attributes);
        break;
    default:
        ADD_FAILURE() << "not a section that gives attributes";
    }
    return made;
}

Customisation named(const char* name)
{
    Customisation customisation;
    customisation.name = name;
    return customisation;
}

// What is loaded before the load: one definition the fixture also defines
// ("TEST Survey Mark", with a different stroke) and one it does not; a WM*
// feature rule the fixture also gives, a WM* symbol rule it does not, and a
// ZZ* rule it does not.
Customisation current()
{
    Customisation now = named("builtin");
    define(now, "TEST Survey Mark", true, true, {moveTo(0, 0), circle(9)});
    define(now, "Kept Style", false, false, {moveTo(0, 0), drawTo(1, 0)});
    rule(now, feature("WM*", "OLD SERVICES", "red"));
    rule(now, feature("ZZ*", "KEPT"));
    rule(now, symbolOf("WM*", "Kept Style"));
    return now;
}

Customisation fixtureLinestyles()
{
    Customisation lines = named("test_linestyles");
    define(lines, "TEST Dashed Kerb", false, false, {moveTo(0, 0), drawTo(1.5, 0)});
    define(lines, "TEST Water Main", false, false, {moveTo(0, 0), drawTo(4.5, 0)});
    define(lines, "TEST Gate", false, false, {moveTo(0, 0), drawTo(4, 0)});
    return lines;
}

Customisation fixtureSurvey()
{
    Customisation survey = named("test_survey");
    rule(survey, feature("WM*", "TEST SERVICES", "blue", "TEST Water Main"));
    rule(survey, feature("KB*", "TEST ROADS", "sui test purple", "TEST Dashed Kerb"));
    rule(survey, feature("AC*", "TEST FURNITURE", "white", "0"));
    rule(survey, feature("TR*", "TEST VEGETATION", "green", "0"));
    rule(survey, feature("1*", "TEST TEXT", "orange", "0"));
    rule(survey, feature("2*", "TEST TEXT", "red", "0"));
    rule(survey, feature("PX*", "TEST MISC", "yellow", "0"));
    rule(survey, symbolOf("AC*", "TEST Survey Mark"));
    rule(survey, symbolOf("TR*", "TEST Tree"));
    rule(survey, symbolOf("PX*", "TEST Missing Symbol"));
    rule(survey, attributesOf(SurveySection::StringAttribute, "*",
                              {{"text", "Source", "Katana test fixture"}}));
    return survey;
}

Customisation fixtureSymbols()
{
    Customisation symbols = named("test_symbols");
    define(symbols, "TEST Survey Mark", true, true, {moveTo(0, 0), circle(0.3)});
    define(symbols, "TEST Tree", true, false, {moveTo(0, 0), circle(1)});
    define(symbols, "TEST Valve", true, true, {moveTo(-0.4, -0.4), drawTo(0.4, -0.4)});
    define(symbols, "TEST U Turn", true, true, {moveTo(0, 0), drawTo(1, 0)});
    return symbols;
}

// The whole fixture, in the order its files loaded in: by name.
std::vector<Customisation> fixture()
{
    return {fixtureLinestyles(), fixtureSurvey(), fixtureSymbols()};
}

const CustomisationLoad* loadNamed(const CustomisationMerge& merge, const std::string& name)
{
    const auto found = std::find_if(merge.loads.begin(), merge.loads.end(),
                                    [&](const CustomisationLoad& load) { return load.name == name; });
    return found == merge.loads.end() ? nullptr : &*found;
}

std::vector<std::string> sorted(std::vector<std::string> names)
{
    std::sort(names.begin(), names.end());
    return names;
}

// "type name=value" each, so that a failure prints something readable.
std::vector<std::string> shown(const std::vector<SurveyAttribute>& attributes)
{
    std::vector<std::string> out;
    for (const auto& attribute : attributes) {
        out.push_back(attribute.type + " " + attribute.name + "=" + attribute.value);
    }
    return out;
}

// Each rule's section, in map order.
std::vector<SurveySection> sections(const SurveyMap& map)
{
    std::vector<SurveySection> out;
    for (const auto& one : map.rules()) {
        out.push_back(one.section);
    }
    return out;
}

// Two pairs of sections fill ONE field of a code: the pipe and the string
// attribute sections both give the string's attributes, the vertex pipe and
// the vertex attribute sections both give each vertex's. Where two rules of
// one key name the same attribute the EARLIER rule's value is the one a code
// gets (SurveyMap::lookup), whichever sections they are in.
Customisation pipeSession()
{
    Customisation now = named("builtin");
    rule(now, attributesOf(SurveySection::Pipe, "*",
                           {{"text", "DepthLocation", "Top of Pipe"}, {"text", "Material", "PVC"}},
                           "Obvert"));
    rule(now, attributesOf(SurveySection::VertexPipe, "SW*", {{"text", "Pit", "Grated"}},
                           "Invert"));
    return now;
}

} // namespace

// ---- the twelve tests the merge came with ----------------------------------------------------

TEST(MergeCustomisation, MergingKeepsEveryCurrentDefinitionTheLoadDoesNotReplace)
{
    const Customisation now = current();
    const std::vector<Customisation> loaded = fixture();
    const CustomisationMerge merged = mergeCustomisation(now, loaded, LoadMode::Merge);

    // 2 current + 7 loaded, less the 1 name both hold ("TEST Survey Mark") = 8.
    EXPECT_TRUE(merged.definitionsLoaded);
    EXPECT_EQ(merged.merged.library.size(), 8u);
    ASSERT_NE(merged.merged.library.find("Kept Style"), nullptr);
    EXPECT_EQ(merged.merged.library.find("Kept Style")->source, "builtin");
    // The loaded definition won, and says where it came from.
    const auto* mark = merged.merged.library.find("TEST Survey Mark");
    ASSERT_NE(mark, nullptr);
    EXPECT_EQ(mark->source, "test_symbols");
    EXPECT_TRUE(merged.removedDefinitions.empty()) << "merging removes nothing";
    EXPECT_TRUE(merged.problems.empty());
    EXPECT_TRUE(merged.ok());
}

TEST(MergeCustomisation, EachLoadedCustomisationReportsTheNamesItAddedAndTheNamesItReplaced)
{
    const Customisation now = current();
    const std::vector<Customisation> loaded = fixture();
    const CustomisationMerge merged = mergeCustomisation(now, loaded, LoadMode::Merge);

    // One entry per loaded customisation, in load order.
    ASSERT_EQ(merged.loads.size(), 3u);
    EXPECT_EQ(merged.loads[0].name, "test_linestyles");
    EXPECT_EQ(merged.loads[1].name, "test_survey");
    EXPECT_EQ(merged.loads[2].name, "test_symbols");

    const auto* lines = loadNamed(merged, "test_linestyles");
    ASSERT_NE(lines, nullptr);
    EXPECT_TRUE(lines->definitions);
    EXPECT_FALSE(lines->rules);
    EXPECT_EQ(sorted(lines->addedDefinitions),
              (std::vector<std::string>{"TEST Dashed Kerb", "TEST Gate", "TEST Water Main"}));
    EXPECT_TRUE(lines->replacedDefinitions.empty());

    const auto* symbols = loadNamed(merged, "test_symbols");
    ASSERT_NE(symbols, nullptr);
    EXPECT_EQ(sorted(symbols->addedDefinitions),
              (std::vector<std::string>{"TEST Tree", "TEST U Turn", "TEST Valve"}));
    EXPECT_EQ(symbols->replacedDefinitions, (std::vector<std::string>{"TEST Survey Mark"}));

    // The 11 rules are 11 (section, key) groups: 7 feature, 3 symbol, 1
    // string attribute. Only the WM* feature group was already there; AC*,
    // TR* and PX* are listed twice because the customisation gives each
    // rules in two sections.
    const auto* survey = loadNamed(merged, "test_survey");
    ASSERT_NE(survey, nullptr);
    EXPECT_FALSE(survey->definitions);
    EXPECT_TRUE(survey->rules);
    EXPECT_EQ(survey->replacedKeys, (std::vector<std::string>{"WM*"}));
    EXPECT_EQ(sorted(survey->addedKeys),
              (std::vector<std::string>{"*", "1*", "2*", "AC*", "AC*", "KB*", "PX*", "PX*", "TR*",
                                        "TR*"}));
}

TEST(MergeCustomisation, ALoadedKeyReplacesThatKeysRulesInOneSectionAndNoOther)
{
    const Customisation now = current();
    const std::vector<Customisation> loaded = fixture();
    const CustomisationMerge merged = mergeCustomisation(now, loaded, LoadMode::Merge);

    // 3 current rules, less the 1 WM* feature rule the load replaces, plus
    // the 11 loaded = 13.
    EXPECT_TRUE(merged.rulesLoaded);
    const SurveyMap& map = merged.merged.map;
    ASSERT_EQ(map.size(), 13u);

    // WM1 now goes where the fixture sends it, and keeps the symbol the
    // current map gave WM* - the fixture has no WM* symbol rule.
    const auto wm = map.lookup("WM1");
    EXPECT_EQ(wm.resolved.model, "TEST SERVICES");
    EXPECT_EQ(wm.resolved.colour, "blue");
    ASSERT_TRUE(wm.resolved.symbol.has_value());
    EXPECT_EQ(wm.resolved.symbol->style, "Kept Style");
    // A key the load does not mention is untouched.
    EXPECT_EQ(map.lookup("ZZ1").resolved.model, "KEPT");

    // The loaded WM* rule stands where the key's first current rule stood -
    // here the one it replaces - ahead of the WM* symbol rule it leaves; the
    // rest keep their order, and keys that are new follow in the order they
    // were loaded.
    const auto& rules = map.rules();
    EXPECT_EQ(rules[0].key, "WM*");
    EXPECT_EQ(rules[0].model, "TEST SERVICES");
    EXPECT_EQ(rules[1].key, "ZZ*");
    EXPECT_EQ(rules[2].key, "WM*");
    EXPECT_EQ(rules[2].section, SurveySection::VertexSymbol);
    EXPECT_EQ(rules[3].key, "KB*");
    EXPECT_EQ(rules[12].key, "*");
    EXPECT_EQ(rules[12].section, SurveySection::StringAttribute);
}

TEST(MergeCustomisation, RulesOfOneKeyInALoadedCustomisationAllReplaceAndAreReportedOnce)
{
    const Customisation now = current();
    // Two WM* feature rules in one customisation: the earlier wins a field
    // both set (SurveyMap::add), so both must come in, in their order, and
    // the old WM* feature rule must go - left in front it would outrank them
    // both.
    Customisation mine = named("mine");
    rule(mine, feature("WM*", "MINE"));
    rule(mine, feature("WM*", "SHADOWED", "green"));
    const std::vector<Customisation> loaded{mine};
    const CustomisationMerge merged = mergeCustomisation(now, loaded, LoadMode::Merge);

    // 3 current - 1 WM* feature rule + 2 = 4.
    ASSERT_EQ(merged.merged.map.size(), 4u);
    const auto wm = merged.merged.map.lookup("WM1");
    EXPECT_EQ(wm.resolved.model, "MINE");
    EXPECT_EQ(wm.resolved.colour, "green") << "the second rule supplies what the first leaves out";
    ASSERT_EQ(merged.loads.size(), 1u);
    EXPECT_EQ(merged.loads[0].replacedKeys, (std::vector<std::string>{"WM*"}));
    EXPECT_TRUE(merged.loads[0].addedKeys.empty());
}

TEST(MergeCustomisation, ASymbolLibraryAloneLeavesTheSurveyMapAsItWas)
{
    // Loading a personal symbol library once installed its EMPTY map over
    // the survey codes.
    const Customisation now = current();
    const std::vector<Customisation> loaded{fixtureSymbols()};
    for (const LoadMode mode : {LoadMode::Merge, LoadMode::Replace}) {
        const CustomisationMerge merged = mergeCustomisation(now, loaded, mode);
        EXPECT_FALSE(merged.rulesLoaded) << katana::cad::toString(mode);
        EXPECT_EQ(merged.merged.map.rules(), now.map.rules()) << katana::cad::toString(mode);
        EXPECT_TRUE(merged.removedKeys.empty()) << katana::cad::toString(mode);
    }
}

TEST(MergeCustomisation, SurveyCodesAloneLeaveTheLibraryAsItWas)
{
    const Customisation now = current();
    const std::vector<Customisation> loaded{fixtureSurvey()};
    for (const LoadMode mode : {LoadMode::Merge, LoadMode::Replace}) {
        const CustomisationMerge merged = mergeCustomisation(now, loaded, mode);
        EXPECT_FALSE(merged.definitionsLoaded) << katana::cad::toString(mode);
        EXPECT_EQ(merged.merged.library.all(), now.library.all()) << katana::cad::toString(mode);
        EXPECT_TRUE(merged.removedDefinitions.empty()) << katana::cad::toString(mode);
    }
}

TEST(MergeCustomisation, ALoadThatBroughtNothingChangesNothing)
{
    const Customisation now = current();
    const CustomisationMerge merged = mergeCustomisation(now, {}, LoadMode::Replace);
    EXPECT_FALSE(merged.definitionsLoaded);
    EXPECT_FALSE(merged.rulesLoaded);
    EXPECT_EQ(merged.merged.library.all(), now.library.all());
    EXPECT_EQ(merged.merged.map.rules(), now.map.rules());
    EXPECT_TRUE(merged.loads.empty());
    EXPECT_TRUE(merged.merged == now) << "not its name, its sources or anything else";

    // A customisation that is named and holds nothing is still a load, listed
    // with nothing against it, and replaces neither kind: it brought neither.
    const std::vector<Customisation> empty{named("empty")};
    const CustomisationMerge nothing = mergeCustomisation(now, empty, LoadMode::Replace);
    ASSERT_EQ(nothing.loads.size(), 1u);
    EXPECT_EQ(nothing.loads[0].name, "empty");
    EXPECT_FALSE(nothing.loads[0].definitions);
    EXPECT_FALSE(nothing.loads[0].rules);
    EXPECT_EQ(nothing.merged.library.all(), now.library.all());
    EXPECT_EQ(nothing.merged.map.rules(), now.map.rules());
    EXPECT_TRUE(nothing.removedDefinitions.empty());
    EXPECT_TRUE(nothing.removedKeys.empty());
}

TEST(MergeCustomisation, ReplacingInstallsOnlyWhatWasLoadedAndListsWhatWentWithIt)
{
    const Customisation now = current();
    const std::vector<Customisation> loaded = fixture();
    const CustomisationMerge replaced = mergeCustomisation(now, loaded, LoadMode::Replace);

    // The 7 loaded definitions and nothing else, each as it was loaded, in
    // name order; and the 11 loaded rules in the order they were loaded.
    std::vector<LineStyle> definitions = loaded[0].library.all();
    const std::vector<LineStyle> symbols = loaded[2].library.all();
    definitions.insert(definitions.end(), symbols.begin(), symbols.end());
    std::sort(definitions.begin(), definitions.end(),
              [](const LineStyle& a, const LineStyle& b) { return a.name < b.name; });
    ASSERT_EQ(definitions.size(), 7u);
    EXPECT_EQ(replaced.merged.library.all(), definitions);
    EXPECT_EQ(replaced.merged.map.rules(), loaded[1].map.rules());
    // "Kept Style" is not in the fixture; "TEST Survey Mark" is. WM* is still
    // a key of the loaded map (its feature rule), so only ZZ* went - even
    // though the WM* symbol rule went with the rest of the old map.
    EXPECT_EQ(replaced.removedDefinitions, (std::vector<std::string>{"Kept Style"}));
    EXPECT_EQ(replaced.removedKeys, (std::vector<std::string>{"ZZ*"}));
    // What each customisation added or replaced is reported the same way as
    // a merge.
    const auto* fromSymbols = loadNamed(replaced, "test_symbols");
    ASSERT_NE(fromSymbols, nullptr);
    EXPECT_EQ(fromSymbols->replacedDefinitions, (std::vector<std::string>{"TEST Survey Mark"}));
}

TEST(MergeCustomisation, ALoadedCustomisationThatChangesNothingIsStillListed)
{
    // Loading a library twice over itself: the second copy replaces every
    // name, and a customisation whose definitions all lost to a later one is
    // still named, with nothing against it, so "it was loaded" is on the
    // report.
    Customisation a = named("a");
    define(a, "X", false, false, {moveTo(0, 0)});
    Customisation b = named("b");
    define(b, "X", false, false, {moveTo(0, 0), drawTo(1, 0)});
    const std::vector<Customisation> loaded{a, b};
    const CustomisationMerge merged = mergeCustomisation({}, loaded, LoadMode::Merge);
    ASSERT_EQ(merged.loads.size(), 2u);
    EXPECT_EQ(merged.loads[0].name, "a");
    EXPECT_TRUE(merged.loads[0].addedDefinitions.empty());
    EXPECT_EQ(merged.loads[1].name, "b");
    EXPECT_EQ(merged.loads[1].addedDefinitions, (std::vector<std::string>{"X"}));
    ASSERT_NE(merged.merged.library.find("X"), nullptr);
    EXPECT_EQ(merged.merged.library.find("X")->strokes.size(), 2u)
        << "the later customisation's definition";
}

TEST(MergeCustomisation, ALoadedAttributeWinsOverTheSameAttributeFromTheOtherSectionThatSetsIt)
{
    // The load gives `*` a DepthLocation in the string attribute section and
    // SW* a Pit in the vertex attribute section; the current map gives both
    // in the sibling sections, the pipe and the vertex pipe. Neither
    // (section, key) group is in the current map, so both are ADDED - and
    // what the load brings must still be what a code gets.
    const Customisation now = pipeSession();
    Customisation mine = named("mine");
    rule(mine, attributesOf(SurveySection::StringAttribute, "*",
                            {{"text", "DepthLocation", "Invert"}}));
    rule(mine, attributesOf(SurveySection::VertexAttribute, "SW*", {{"text", "Pit", "Solid"}}));
    const std::vector<Customisation> loaded{mine};
    const CustomisationMerge merged = mergeCustomisation(now, loaded, LoadMode::Merge);

    ASSERT_EQ(merged.loads.size(), 1u);
    EXPECT_EQ(merged.loads[0].addedKeys, (std::vector<std::string>{"*", "SW*"}));
    EXPECT_TRUE(merged.loads[0].replacedKeys.empty());
    // 2 current rules, none replaced, + 2 loaded = 4.
    const SurveyMap& map = merged.merged.map;
    ASSERT_EQ(map.size(), 4u);

    const auto sw = map.lookup("SW1");
    // SW1 meets SW* and then `*`. The string's attributes all come from `*`:
    // DepthLocation from the LOADED string attribute rule, Material - which
    // the load says nothing about - still from the current pipe rule, in
    // that order.
    EXPECT_EQ(shown(sw.resolved.attributes),
              (std::vector<std::string>{"text DepthLocation=Invert", "text Material=PVC"}));
    // Each vertex's Pit comes from SW*: the loaded vertex attribute rule, not
    // the current vertex pipe one.
    EXPECT_EQ(shown(sw.resolved.vertexAttributes), (std::vector<std::string>{"text Pit=Solid"}));
    // And the current rules still give what only they say.
    ASSERT_TRUE(sw.resolved.pipe.has_value());
    EXPECT_EQ(sw.resolved.pipe->justify, "Obvert");
    ASSERT_TRUE(sw.resolved.vertexPipe.has_value());
    EXPECT_EQ(sw.resolved.vertexPipe->justify, "Invert");

    // Worked out by hand: at the current map's first `*` rule the loaded `*`
    // rule goes in first, then the current one it leaves standing; the same
    // at SW*.
    EXPECT_EQ(sections(map),
              (std::vector<SurveySection>{SurveySection::StringAttribute, SurveySection::Pipe,
                                          SurveySection::VertexAttribute,
                                          SurveySection::VertexPipe}));
}

TEST(MergeCustomisation, ALoadedGroupReplacingOneInPlaceStillGoesAheadOfItsKeysOtherSections)
{
    // The current map has `*` in the pipe section AND, after it, in the
    // string attribute section. The load replaces the string attribute group;
    // put back where that group stood, it would stand behind the pipe rule
    // and its DepthLocation would lose to "Top of Pipe".
    Customisation now = named("builtin");
    rule(now, attributesOf(SurveySection::Pipe, "*", {{"text", "DepthLocation", "Top of Pipe"}},
                           "Obvert"));
    rule(now, attributesOf(SurveySection::StringAttribute, "*",
                           {{"text", "DepthLocation", "Centre"}}));
    Customisation mine = named("mine");
    rule(mine, attributesOf(SurveySection::StringAttribute, "*",
                            {{"text", "DepthLocation", "Invert"}}));
    const std::vector<Customisation> loaded{mine};
    const CustomisationMerge merged = mergeCustomisation(now, loaded, LoadMode::Merge);

    ASSERT_EQ(merged.loads.size(), 1u);
    EXPECT_EQ(merged.loads[0].replacedKeys, (std::vector<std::string>{"*"}));
    // 2 current - 1 replaced + 1 loaded = 2: the loaded rule, then the pipe.
    const SurveyMap& map = merged.merged.map;
    ASSERT_EQ(map.size(), 2u);
    EXPECT_EQ(sections(map), (std::vector<SurveySection>{SurveySection::StringAttribute,
                                                         SurveySection::Pipe}));
    EXPECT_EQ(shown(map.lookup("ANY").resolved.attributes),
              (std::vector<std::string>{"text DepthLocation=Invert"}));
}

TEST(MergeCustomisation, WhatAMergedLoadBroughtStillWinsAfterTheSessionIsWrittenAndReadBack)
{
    // An edited customisation persists only by being written, so a merge is
    // only as good as what is written of it. The merged map holds `*` in the
    // string attribute section (loaded) ahead of `*` in the pipe section
    // (current), and the file must keep the loaded DepthLocation winning.
    const Customisation now = pipeSession();
    Customisation mine = named("mine");
    rule(mine, attributesOf(SurveySection::StringAttribute, "*",
                            {{"text", "DepthLocation", "Invert"}}));
    const std::vector<Customisation> loaded{mine};
    const CustomisationMerge merged = mergeCustomisation(now, loaded, LoadMode::Merge);
    const auto written = katana::entity::customisationToJson(merged.merged);
    ASSERT_TRUE(written.ok()) << written.error().describe();
    const auto again = katana::entity::customisationFromJson(*written);
    ASSERT_TRUE(again.ok()) << again.error().describe();

    // By hand, as in ALoadedAttributeWinsOverTheSameAttributeFromTheOther-
    // SectionThatSetsIt: DepthLocation from the loaded rule, Material from
    // the current pipe rule.
    EXPECT_EQ(shown(again->map.lookup("SW1").resolved.attributes),
              (std::vector<std::string>{"text DepthLocation=Invert", "text Material=PVC"}));
    // And the whole of it: what was merged is what was read.
    EXPECT_TRUE(*again == merged.merged);
}

// ---- what one customisation holds beyond definitions and rules -------------------------------

TEST(MergeCustomisation, ALoadedColourTakesThePlaceOfTheSessionsOfThatNameHoweverItIsSpelled)
{
    // Colour names compare by their fold: "SUI_Gas" is "sui gas". By hand:
    // the session has two names; the load gives one of them again, spelled
    // another way and meaning another colour, and one that is new. Three
    // names after it, the respelled one the loaded colour - in BOTH modes: a
    // colour is no kind a Replace takes the place of.
    Customisation now = named("builtin");
    ASSERT_TRUE(now.colours.add("sui gas", Color{1, 2, 3, 255}).ok());
    ASSERT_TRUE(now.colours.add("site kerb", Color{9, 9, 9, 255}).ok());
    Customisation mine = named("mine");
    ASSERT_TRUE(mine.colours.add("SUI_Gas", Color{10, 11, 12, 255}).ok());
    ASSERT_TRUE(mine.colours.add("new one", Color{20, 21, 22, 255}).ok());
    const std::vector<Customisation> loaded{mine};

    for (const LoadMode mode : {LoadMode::Merge, LoadMode::Replace}) {
        const CustomisationMerge merged = mergeCustomisation(now, loaded, mode);
        ASSERT_TRUE(merged.ok()) << katana::cad::toString(mode);
        const katana::entity::ColourTable& colours = merged.merged.colours;
        EXPECT_EQ(colours.size(), 3u);
        EXPECT_EQ(colours.find("sui gas"), (std::optional<Color>{Color{10, 11, 12, 255}}));
        EXPECT_EQ(colours.find("site kerb"), (std::optional<Color>{Color{9, 9, 9, 255}}));
        EXPECT_EQ(colours.find("new one"), (std::optional<Color>{Color{20, 21, 22, 255}}));
        // Written back as the loaded customisation spelled it.
        std::vector<std::string> names;
        for (const auto& entry : colours.entries()) {
            names.push_back(entry.name);
        }
        EXPECT_EQ(names, (std::vector<std::string>{"new one", "site kerb", "SUI_Gas"}));
        ASSERT_EQ(merged.loads.size(), 1u);
        EXPECT_EQ(merged.loads[0].replacedColours, (std::vector<std::string>{"SUI_Gas"}));
        EXPECT_EQ(merged.loads[0].addedColours, (std::vector<std::string>{"new one"}));
        // Colours alone are neither kind.
        EXPECT_FALSE(merged.definitionsLoaded);
        EXPECT_FALSE(merged.rulesLoaded);
    }
}

TEST(MergeCustomisation, LineworkCodesAndAutomationAreTakenOnlyFromACustomisationThatSaysThem)
{
    // A file of symbols for a colleague must not reset their control codes.
    Customisation now = named("builtin");
    katana::entity::LineworkCodes own;
    own.start = "S";
    now.linework = own;
    now.automation = katana::entity::CustomisationAutomation{true, false};

    const std::vector<Customisation> silent{fixtureSymbols()};
    const CustomisationMerge kept = mergeCustomisation(now, silent, LoadMode::Replace);
    ASSERT_TRUE(kept.merged.linework.has_value());
    EXPECT_EQ(*kept.merged.linework, own);
    EXPECT_EQ(kept.merged.automation, now.automation);
    EXPECT_FALSE(kept.loads[0].linework);
    EXPECT_FALSE(kept.loads[0].automation);

    // One that says them is taken at its word, the later of two winning.
    Customisation first = named("first");
    katana::entity::LineworkCodes begin;
    begin.start = "BEGIN";
    first.linework = begin;
    Customisation second = named("second");
    katana::entity::LineworkCodes go;
    go.start = "GO";
    second.linework = go;
    second.automation = katana::entity::CustomisationAutomation{false, false};
    const std::vector<Customisation> saying{first, second};
    const CustomisationMerge taken = mergeCustomisation(now, saying, LoadMode::Merge);
    ASSERT_TRUE(taken.merged.linework.has_value());
    EXPECT_EQ(taken.merged.linework->start, "GO");
    ASSERT_TRUE(taken.merged.automation.has_value());
    EXPECT_EQ(*taken.merged.automation, (katana::entity::CustomisationAutomation{false, false}));
    EXPECT_TRUE(taken.loads[0].linework);
    EXPECT_FALSE(taken.loads[0].automation);
    EXPECT_TRUE(taken.loads[1].linework);
    EXPECT_TRUE(taken.loads[1].automation);

    // A session that says neither, loaded with one that says neither, still
    // says neither: absent is not the defaults.
    const CustomisationMerge neither =
        mergeCustomisation(named("bare"), silent, LoadMode::Merge);
    EXPECT_FALSE(neither.merged.linework.has_value());
    EXPECT_FALSE(neither.merged.automation.has_value());
}

TEST(MergeCustomisation, EachLoadedCustomisationIsRecordedAsASourceWithWhatItBrought)
{
    // By hand. The session lists no sources, so it is its own one source,
    // bringing both kinds; the three loads follow in load order, each with
    // what it brought, and the notice the survey codes came with.
    const Customisation now = current();
    std::vector<Customisation> loaded = fixture();
    loaded[1].notice = {"Hand-written for tests."};
    const CustomisationMerge merged = mergeCustomisation(now, loaded, LoadMode::Merge);
    EXPECT_EQ(merged.merged.sources,
              (std::vector<CustomisationSource>{
                  {"builtin", true, true, {}},
                  {"test_linestyles", true, false, {}},
                  {"test_survey", false, true, {"Hand-written for tests."}},
                  {"test_symbols", true, false, {}}}));
    // A load is added to the session, which keeps its name.
    EXPECT_EQ(merged.merged.name, "builtin");

    // A Replace by survey codes alone takes the rules the session brought and
    // leaves it its definitions; it is still the session it was.
    const std::vector<Customisation> codes{loaded[1]};
    const CustomisationMerge recoded = mergeCustomisation(now, codes, LoadMode::Replace);
    EXPECT_EQ(recoded.merged.sources,
              (std::vector<CustomisationSource>{
                  {"builtin", true, false, {}},
                  {"test_survey", false, true, {"Hand-written for tests."}}}));
    EXPECT_EQ(recoded.merged.name, "builtin");

    // A Replace that brought BOTH kinds leaves nothing of the session's
    // definitions or rules for its name to be about: the first loaded
    // customisation's name, description and notice are the session's now, and
    // only the loads are sources. Its notice is said once, as the session's.
    loaded[0].description = "Lines for tests";
    loaded[0].notice = {"Every stroke drawn for this file."};
    const CustomisationMerge replaced = mergeCustomisation(now, loaded, LoadMode::Replace);
    EXPECT_EQ(replaced.merged.name, "test_linestyles");
    EXPECT_EQ(replaced.merged.description, "Lines for tests");
    EXPECT_EQ(replaced.merged.notice,
              (std::vector<std::string>{"Every stroke drawn for this file."}));
    EXPECT_EQ(replaced.merged.sources,
              (std::vector<CustomisationSource>{
                  {"test_linestyles", true, false, {}},
                  {"test_survey", false, true, {"Hand-written for tests."}},
                  {"test_symbols", true, false, {}}}));
}

TEST(MergeCustomisation, ALoadedCustomisationThatListsItsSourcesKeepsItsOwnNotice)
{
    // What a session writes of itself (Document::customisation, pinned by
    // CustomisationState.TheSessionAsOneCustomisationInstallsBackAsTheSame-
    // Session): its notice at the top level and its sources listed, itself
    // among them WITHOUT a notice. Every kept or exported customisation has
    // that shape, and its notice was dropped when it was merged into another
    // session.
    const Customisation now = current();
    Customisation site = named("Site");
    define(site, "SITE Peg", true, true, {moveTo(0, 0), circle(1)});
    rule(site, feature("PG*", "PEGS"));
    site.notice = {"For this site only."};
    site.sources = {{"Site", true, true, {}}};

    // By hand: the session keeps its name and has no notice of its own;
    // "Site" follows it as a source, carrying the notice it came with.
    const std::vector<Customisation> one{site};
    const CustomisationMerge merged = mergeCustomisation(now, one, LoadMode::Merge);
    ASSERT_TRUE(merged.ok());
    EXPECT_EQ(merged.merged.name, "builtin");
    EXPECT_TRUE(merged.merged.notice.empty());
    EXPECT_EQ(merged.merged.sources,
              (std::vector<CustomisationSource>{
                  {"builtin", true, true, {}},
                  {"Site", true, true, {"For this site only."}}}));

    // The second of a Replace that brought both kinds. The first becomes the
    // session: its notice is the session's, said once, and its entry has
    // none. The second is a source, and keeps its own.
    Customisation first = named("First");
    define(first, "FIRST Line", false, false, {moveTo(0, 0), drawTo(1, 0)});
    rule(first, feature("FL*", "FIRST"));
    first.notice = {"The first one's."};
    first.sources = {{"First", true, true, {}}};
    const std::vector<Customisation> two{first, site};
    const CustomisationMerge replaced = mergeCustomisation(now, two, LoadMode::Replace);
    ASSERT_TRUE(replaced.ok());
    EXPECT_EQ(replaced.merged.name, "First");
    EXPECT_EQ(replaced.merged.notice, (std::vector<std::string>{"The first one's."}));
    EXPECT_EQ(replaced.merged.sources,
              (std::vector<CustomisationSource>{
                  {"First", true, true, {}},
                  {"Site", true, true, {"For this site only."}}}));

    // An entry of its name that already says something keeps that too, and a
    // line said in both places is said once.
    Customisation twice = site;
    twice.sources = {{"Site", true, true, {"An earlier line.", "For this site only."}}};
    const std::vector<Customisation> said{twice};
    EXPECT_EQ(mergeCustomisation(now, said, LoadMode::Merge).merged.sources,
              (std::vector<CustomisationSource>{
                  {"builtin", true, true, {}},
                  {"Site", true, true, {"For this site only.", "An earlier line."}}}));

    // Written under another name than any source it lists: nothing says
    // which of them the notice came with, so every one of them carries it
    // rather than none - ahead of what it said already.
    Customisation renamed = site;
    renamed.name = "Site for Others";
    renamed.sources = {{"NSW", true, true, {}}, {"roads", true, false, {"Roads notice."}}};
    const std::vector<Customisation> other{renamed};
    EXPECT_EQ(mergeCustomisation(now, other, LoadMode::Merge).merged.sources,
              (std::vector<CustomisationSource>{
                  {"builtin", true, true, {}},
                  {"NSW", true, true, {"For this site only."}},
                  {"roads", true, false, {"For this site only.", "Roads notice."}}}));
}

TEST(MergeCustomisation, ASessionWithNoNameYetTakesTheFirstLoadedCustomisationsName)
{
    // Nothing installed: the first thing loaded is what the session is.
    Customisation first = fixtureSymbols();
    first.description = "Marks";
    first.basedOn = katana::entity::CustomisationBase{"Marks", "0123456789abcdef"};
    const std::vector<Customisation> loaded{first, fixtureSurvey()};
    const CustomisationMerge merged = mergeCustomisation({}, loaded, LoadMode::Merge);
    ASSERT_TRUE(merged.ok());
    EXPECT_EQ(merged.merged.name, "test_symbols");
    EXPECT_EQ(merged.merged.description, "Marks");
    EXPECT_EQ(merged.merged.basedOn, first.basedOn);
    EXPECT_EQ(merged.merged.sources,
              (std::vector<CustomisationSource>{{"test_symbols", true, false, {}},
                                                {"test_survey", false, true, {}}}));
    EXPECT_EQ(merged.merged.library.size(), 4u);
    EXPECT_EQ(merged.merged.map.size(), 11u);
}

TEST(MergeCustomisation, ALoadWithAProblemInstallsNothingAndSaysEveryProblem)
{
    // Two faulty customisations in one load, built in code as no file could
    // be read: one whose name a project could not record, one whose control
    // codes spell two controls alike. Both are said, each naming its
    // customisation, and NOTHING of the load is in what would be installed -
    // not even the sound customisation loaded beside them.
    const Customisation now = current();
    Customisation badName = fixtureSymbols();
    badName.name = "roads/2026";
    Customisation badCodes = named("codes");
    katana::entity::LineworkCodes alike;
    alike.start = "X";
    alike.end = "x";
    badCodes.linework = alike;
    const std::vector<Customisation> loaded{fixtureLinestyles(), badName, badCodes};

    for (const LoadMode mode : {LoadMode::Merge, LoadMode::Replace}) {
        const CustomisationMerge merged = mergeCustomisation(now, loaded, mode);
        EXPECT_FALSE(merged.ok());
        ASSERT_EQ(merged.problems.size(), 2u) << katana::cad::toString(mode);
        // The one with no usable name is known by its place in the load.
        EXPECT_EQ(merged.problems[0].rfind("customisation 2 of this load: its name: ", 0), 0u)
            << merged.problems[0];
        EXPECT_EQ(merged.problems[1].rfind("\"codes\": its linework codes: ", 0), 0u)
            << merged.problems[1];
        EXPECT_TRUE(merged.merged == now) << "all or nothing";
        // The report still says what each would have done.
        ASSERT_EQ(merged.loads.size(), 3u);
        EXPECT_EQ(merged.loads[0].addedDefinitions.size(), 3u);
    }
}

TEST(MergeCustomisation, ADefinitionsSourceAndWhatALoadIsBasedOnAreJudgedAsItsNameIs)
{
    // Built in code, as no file could be read. A definition whose source is a
    // path reaches the project's record as a name, and the store refuses the
    // record: every SAVE of the session then fails. A basedOn whose digest is
    // not one is refused by the kept file's writer: every KEEP fails. Both
    // are problems of the load that brought them, said before anything is
    // installed.
    const Customisation now = current();
    Customisation roads = named("roads");
    LineStyle kerb;
    kerb.name = "ROAD Kerb";
    kerb.source = "lib/roads";
    kerb.strokes = {moveTo(0, 0), drawTo(1, 0)};
    ASSERT_TRUE(roads.library.add(kerb).ok());
    roads.basedOn = katana::entity::CustomisationBase{"NSW", "0123"};
    const std::vector<Customisation> loaded{roads};

    const CustomisationMerge merged = mergeCustomisation(now, loaded, LoadMode::Merge);
    EXPECT_FALSE(merged.ok());
    ASSERT_EQ(merged.problems.size(), 2u);
    const std::string ofSource = "\"roads\": the source of its definition \"ROAD Kerb\": ";
    EXPECT_EQ(merged.problems[0].rfind(ofSource, 0), 0u) << merged.problems[0];
    EXPECT_EQ(merged.problems[1].rfind("\"roads\": its basedOn: ", 0), 0u) << merged.problems[1];
    EXPECT_TRUE(merged.merged == now) << "all or nothing";
}
