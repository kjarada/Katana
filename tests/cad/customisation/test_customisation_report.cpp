// What a bare CUSTOMISE says (customisation_report.hpp): the window's command
// line and katana_cli print this one text. Counted by hand from what each
// test loads.

#include <gtest/gtest.h>

#include <string>
#include <vector>

#include "katana/cad/command_interpreter.hpp"
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

const std::vector<CustomisationSource> kLoaded{{"lines.4d", true},
                                               {"pits.4d", true},
                                               {"site codes.mapfile", false}};

} // namespace

TEST(CustomisationReport, NothingLoadedSaysSoAndHowToLoad)
{
    const Document document;
    EXPECT_EQ(katana::cad::customisationReport(document, {}, {}),
              "No customisation is loaded.\n"
              "  CUSTOMISE <file> [<file>...]  loads style libraries (.4d) and survey code "
              "files (.mapfile)\n");
}

TEST(CustomisationReport, TheProjectsMissingFilesAreNamedEvenWithNothingLoaded)
{
    const Document document;
    EXPECT_EQ(katana::cad::customisationReport(document, {}, {"lines.4d", "site codes.mapfile"}),
              "No customisation is loaded.\n"
              "  CUSTOMISE <file> [<file>...]  loads style libraries (.4d) and survey code "
              "files (.mapfile)\n"
              "This project was drawn with customisation files that are not loaded: "
              "\"lines.4d\", \"site codes.mapfile\"\n");
}

TEST(CustomisationReport, CountsNamesTheFilesInLoadOrderAndSaysWhatTheDrawingUses)
{
    Document document;
    load(document);
    EXPECT_EQ(katana::cad::customisationReport(document, kLoaded, {"old symbols.4d"}),
              "2 linestyle and symbol definitions in 2 groups: 1 offered as symbols, 1 as "
              "linestyles (one definition can be both)\n"
              "2 survey code rules over 1 distinct code\n"
              "Loaded files, in load order:\n"
              "  \"lines.4d\", a style library\n"
              "  \"pits.4d\", a style library\n"
              "  \"site codes.mapfile\", a survey code file\n"
              "This project was drawn with customisation files that are not loaded: "
              "\"old symbols.4d\"\n"
              "This drawing has no styles yet; import a drawing or survey that carries styles, "
              "or make one in Format > Styles and Linetypes or with STYLE NEW, to see the "
              "customisation take effect.\n");

    // A style that names the fence is drawn with it: 1 of 1.
    katana::cad::CommandInterpreter interpreter(document);
    ASSERT_TRUE(interpreter.run("STYLE NEW Fence").ok());
    ASSERT_TRUE(interpreter.run("STYLE SET Fence linetype \"TEST Fence\"").ok());
    const std::string report = katana::cad::customisationReport(document, kLoaded, {});
    EXPECT_NE(report.find("\"site codes.mapfile\", a survey code file\n"
                          "1 of this drawing's 1 styles are drawn with a loaded definition "
                          "(1 name one; the rest are plain lines)\n"),
              std::string::npos)
        << report;
}

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
