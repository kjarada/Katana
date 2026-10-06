// What a bare CUSTOMISE says (customisation_report.hpp): the numbers of the
// summary, and the verb's reply made from one - the count lines, the records,
// the coverage. Counted by hand from what each test loads.
//
// The four tests of an older prose text stood first here ("Loaded files, in
// load order: ...", cad::customisationReport). They went with that text on
// 2026-10-07, when the last front end that printed it took the verb's reply.

#include <gtest/gtest.h>

#include <string>
#include <vector>

#include "katana/cad/customisation_report.hpp"
#include "katana/cad/document.hpp"
#include "katana/entity/style_library.hpp"
#include "katana/entity/survey_map.hpp"

using katana::cad::CustomisationSource;
using katana::cad::Document;
using katana::entity::LineStyle;

namespace {

LineStyle definition(std::string name, std::string group, bool atVertices, std::string source)
{
    LineStyle style;
    style.name = std::move(name);
    style.group = std::move(group);
    style.atVertices = atVertices;
    style.source = std::move(source);
    return style;
}

// Two definitions in two groups: a fence drawn along a line, from a file whose
// name does not say symbol (a linestyle only), and a pit drawn at vertices (a
// symbol only, decision D3). Two survey code rules over one code.
void load(Document& document)
{
    katana::entity::StyleLibrary library;
    EXPECT_TRUE(katana::entity::addOrReplace(library,
                                             definition("TEST Fence", "FENCES", false, "lines.4d"))
                    .ok());
    EXPECT_TRUE(
        katana::entity::addOrReplace(library, definition("TEST Pit", "PITS", true, "pits.4d")).ok());
    document.setStyleLibrary(std::move(library));
    katana::entity::SurveyMap map;
    katana::entity::SurveyRule fence;
    fence.key = "FE*";
    fence.model = "FENCES";
    fence.linestyle = "TEST Fence";
    EXPECT_TRUE(map.add(fence).ok());
    katana::entity::SurveyRule again = fence;
    again.colour = "red";
    EXPECT_TRUE(map.add(again).ok());
    document.setSurveyMap(std::move(map));
}

// {name, brought definitions, brought rules}: two style libraries and a
// survey code file.
const std::vector<CustomisationSource> kLoaded{{"lines.4d", true, false, {}},
                                               {"pits.4d", true, false, {}},
                                               {"site codes.mapfile", false, true, {}}};

} // namespace

TEST(CustomisationReport, TheSummaryIsTheNumbersTheTextIsMadeFrom)
{
    Document document;
    load(document);
    const katana::cad::CustomisationSummary summary =
        katana::cad::customisationSummary(document, kLoaded, {"old symbols.4d"});
    EXPECT_EQ(summary.definitions, 2u);
    EXPECT_EQ(summary.groups, 2u);
    EXPECT_EQ(summary.symbols, 1u);
    EXPECT_EQ(summary.linestyles, 1u);
    EXPECT_EQ(summary.rules, 2u);
    EXPECT_EQ(summary.codes, 1u);
    EXPECT_EQ(summary.loaded, kLoaded);
    EXPECT_EQ(summary.notLoaded, std::vector<std::string>{"old symbols.4d"});
    EXPECT_EQ(summary.coverage.styles, 0u);
}

// ---- the verb's reply (formatCustomisationReply) -----------------------------------------
//
// What the interpreter's CUSTOMISE replies: the two count lines above, then
// records. tests/cad/customisation/test_customisation_verbs.cpp drives it
// through the verb; here it is given a summary written out by hand, which is
// the only way to show it a name the open project is missing without a
// project on disk.

TEST(CustomisationReport, TheVerbsReplyIsTheCountsThenRecordsOfTheSessionThenTheCoverage)
{
    katana::cad::CustomisationSummary summary;
    summary.definitions = 2;
    summary.groups = 2;
    summary.symbols = 1;
    summary.linestyles = 1;
    summary.rules = 2;
    summary.codes = 1;
    summary.loaded = {{"Site Styles", true, false, {}}, {"Site codes", false, true, {}}};
    summary.notLoaded = {"old symbols", "plain"};
    summary.name = "Site Styles";
    summary.origin = katana::cad::CustomisationOrigin::Loaded;
    summary.kept = false;
    summary.colours = 3;
    summary.automation.lineworkOnSurveyImport = false;
    summary.linework.join = ""; // that control is off
    EXPECT_EQ(katana::cad::formatCustomisationReply(summary),
              "2 linestyle and symbol definitions in 2 groups: 1 offered as symbols, 1 as "
              "linestyles (one definition can be both)\n"
              "2 survey code rules over 1 distinct code\n"
              "customisation name=\"Site Styles\" origin=loaded kept=no definitions=2 codes=1 "
              "rules=2 colours=3\n"
              "source name=\"Site Styles\" definitions=yes rules=no\n"
              "source name=\"Site codes\" definitions=no rules=yes\n"
              "automation auto.codes=on auto.linework=off\n"
              "linework linework.start=ST linework.end=END linework.close=CL "
              "linework.arcstart=BC linework.arcend=EC linework.join=\"\" "
              "linework.rectangle=RECT\n"
              "missing name=\"old symbols\"\n"
              "missing name=plain\n"
              "This drawing has no styles yet; import a drawing or survey that carries styles, "
              "or make one in Format > Styles and Linetypes or with STYLE NEW, to see the "
              "customisation take effect.\n");
    // The one record a verb that changed the session ends its reply with.
    EXPECT_EQ(katana::cad::customisationStateRecord(summary),
              "customisation name=\"Site Styles\" origin=loaded kept=no definitions=2 codes=1 "
              "rules=2 colours=3");
}

TEST(CustomisationReport, TheVerbsReplyOfNothingKeepsWhatTheProjectIsMissingAndNamesNoFileKind)
{
    katana::cad::CustomisationSummary summary;
    summary.notLoaded = {"old symbols"};
    const std::string reply = katana::cad::formatCustomisationReply(summary);
    EXPECT_EQ(reply,
              "No customisation is loaded.\n"
              "  CUSTOMISE <file> [<file>...]  loads Katana customisation files\n"
              "customisation name=\"\" origin=none kept=no definitions=0 codes=0 rules=0 "
              "colours=0\n"
              "automation auto.codes=on auto.linework=on\n"
              "linework linework.start=ST linework.end=END linework.close=CL "
              "linework.arcstart=BC linework.arcend=EC linework.join=JPN "
              "linework.rectangle=RECT\n"
              "missing name=\"old symbols\"\n");
    // The kinds of file another program wrote are not what loads any more.
    EXPECT_EQ(reply.find(".4d"), std::string::npos);
    EXPECT_EQ(reply.find(".mapfile"), std::string::npos);
}

TEST(CustomisationReport, TheVerbsReplyNeverSaysNoCustomisationIsLoadedOfOneThatIs)
{
    // A customisation that brought neither kind: a name and a table of two
    // colours, loaded. "No customisation is loaded." above the record that
    // names it contradicted the next line. What is true is that it holds
    // nothing that draws - and there is still no coverage to report of that.
    katana::cad::CustomisationSummary summary;
    summary.name = "Site colours";
    summary.origin = katana::cad::CustomisationOrigin::Loaded;
    summary.colours = 2;
    summary.loaded = {{"Site colours", false, false, {}}};
    const std::string reply = katana::cad::formatCustomisationReply(summary);
    EXPECT_EQ(reply,
              "No linestyle or symbol definitions and no survey code rules are loaded.\n"
              "  CUSTOMISE <file> [<file>...]  loads Katana customisation files\n"
              "customisation name=\"Site colours\" origin=loaded kept=no definitions=0 codes=0 "
              "rules=0 colours=2\n"
              "source name=\"Site colours\" definitions=no rules=no\n"
              "automation auto.codes=on auto.linework=on\n"
              "linework linework.start=ST linework.end=END linework.close=CL "
              "linework.arcstart=BC linework.arcend=EC linework.join=JPN "
              "linework.rectangle=RECT\n");
    EXPECT_EQ(reply.find("No customisation is loaded"), std::string::npos);
    // Every origin but "none" is a customisation that is there.
    for (const auto origin :
         {katana::cad::CustomisationOrigin::BuiltIn, katana::cad::CustomisationOrigin::Kept,
          katana::cad::CustomisationOrigin::Edited}) {
        summary.origin = origin;
        EXPECT_TRUE(katana::cad::formatCustomisationReply(summary).starts_with(
            "No linestyle or symbol definitions and no survey code rules are loaded.\n"))
            << katana::cad::toString(origin);
    }
}

TEST(CustomisationReport, TheSummaryOfASessionReadsItsStateFromTheDocument)
{
    // The raw setters, as an editor calls them: an edited session with no
    // name and no source, the switches and codes as they were set.
    Document document;
    load(document);
    document.setAutomation({false, true});
    katana::entity::LineworkCodes codes;
    codes.start = "S";
    ASSERT_TRUE(document.setLineworkCodes(codes).ok());
    katana::entity::ColourTable colours;
    ASSERT_TRUE(colours.add("site orange", katana::entity::Color{}).ok());
    document.setColourTable(colours);

    const katana::cad::CustomisationSummary summary = katana::cad::customisationSummary(document);
    EXPECT_EQ(summary.definitions, 2u);
    EXPECT_EQ(summary.rules, 2u);
    EXPECT_EQ(summary.codes, 1u);
    EXPECT_EQ(summary.name, "");
    EXPECT_EQ(summary.origin, katana::cad::CustomisationOrigin::Edited);
    EXPECT_FALSE(summary.kept);
    EXPECT_EQ(summary.colours, 1u);
    EXPECT_FALSE(summary.automation.codesOnSurveyImport);
    EXPECT_TRUE(summary.automation.lineworkOnSurveyImport);
    EXPECT_EQ(summary.linework.start, "S");
    EXPECT_TRUE(summary.loaded.empty());
    EXPECT_TRUE(summary.notLoaded.empty());
}

TEST(CustomisationReport, TheNamesTheRulesAskForThatNothingDefinesAreEachListedOnce)
{
    // load() gives a rule naming "TEST Fence", which the library defines.
    // Added here: a linestyle and a symbol nothing defines (the linestyle
    // named by two rules), a plain line, and a symbol Katana draws itself -
    // neither of which needs a definition.
    Document document;
    load(document);
    katana::entity::SurveyMap map = document.surveyMap();
    const auto feature = [&map](const char* key, const char* linestyle) {
        katana::entity::SurveyRule rule;
        rule.key = key;
        rule.model = "SOMEWHERE";
        rule.linestyle = linestyle;
        EXPECT_TRUE(map.add(rule).ok());
    };
    const auto symbol = [&map](const char* key, const char* name) {
        katana::entity::SurveyRule rule;
        rule.key = key;
        rule.section = katana::entity::SurveySection::VertexSymbol;
        rule.symbol = katana::entity::SurveySymbol{name, "", 1.0, 0.0, 0.0, 0.0};
        EXPECT_TRUE(map.add(rule).ok());
    };
    feature("A1*", "ZZ Missing");
    feature("A2*", "ZZ Missing");
    feature("A3*", "0");
    symbol("B1*", "AA Missing");
    symbol("B2*", "cross");
    document.setSurveyMap(map);
    EXPECT_EQ(katana::cad::undefinedRuleNames(document),
              (std::vector<std::string>{"AA Missing", "ZZ Missing"}));

    const Document empty;
    EXPECT_TRUE(katana::cad::undefinedRuleNames(empty).empty());
}
