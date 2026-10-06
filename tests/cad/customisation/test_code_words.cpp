// The words the survey code tools show a rule's parts by, where no other test
// pins them: every section's word in the code table, a rule's `surface`, a
// definition drawn `at vertices`, and a layer that is not a layer path.
//
// They are the Katana customisation format's words - "feature", "layer",
// "surface", "at vertices" - and each expected text is written out by hand
// from the rules the test builds and the table "The words a rule is shown by"
// in docs/customisation.md, never captured from a run.

#include <gtest/gtest.h>

#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include "katana/cad/code_table.hpp"
#include "katana/cad/style_catalogue.hpp"
#include "katana/cad/style_manager_rows.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/tables.hpp"

namespace cmd = katana::commands;
using katana::cad::Document;
using katana::cad::LintIssue;
using katana::cad::LintKind;
using katana::cad::LintSeverity;
using katana::entity::LineStyle;
using katana::entity::SurveyMap;
using katana::entity::SurveyRule;
using katana::entity::SurveySection;

namespace {

// Two definitions drawn at vertices - symbols, never line patterns - and
// nothing else. Their names, and the one below that nothing defines, are
// invented as the other fixtures' are ("TEST ..."): a name a real
// customisation defines is its author's, and a fixture is committed.
katana::entity::StyleLibrary marks()
{
    katana::entity::StyleLibrary library;
    for (const char* name : {"TEST Peg", "TEST Nail"}) {
        LineStyle style;
        style.name = name;
        style.atVertices = true;
        EXPECT_TRUE(katana::entity::addOrReplace(library, std::move(style)).ok()) << name;
    }
    return library;
}

// A drawing with that library and one style per linetype name given.
void styledWith(Document& document, const std::vector<std::pair<const char*, const char*>>& styles)
{
    document.setStyleLibrary(marks());
    for (const auto& [name, linetype] : styles) {
        katana::entity::Style style;
        style.name = name;
        style.linetype = linetype;
        ASSERT_TRUE(document.execute(cmd::createStyle(style)).ok()) << name;
    }
}

} // namespace

TEST(CodeWords, TheCodeTableNamesEachOfTheNineSectionsByItsWord)
{
    // One key with a rule in every section, none saying anything else: the
    // row lists the sections in the enumeration's order and has no parts.
    SurveyMap map;
    for (const SurveySection section :
         {SurveySection::Map, SurveySection::VertexSymbol, SurveySection::VertexTextStyle,
          SurveySection::Pipe, SurveySection::VertexPipe, SurveySection::SegmentPipe,
          SurveySection::StringAttribute, SurveySection::VertexAttribute,
          SurveySection::Tinable}) {
        SurveyRule rule;
        rule.key = "K*";
        rule.section = section;
        ASSERT_TRUE(map.add(rule).ok());
    }
    EXPECT_EQ(katana::cad::formatCodeTable(katana::cad::codeTable(map), {}),
              "K*  [prefix; 9 rules: feature, symbol, text, pipe, vertexPipe, segmentPipe, "
              "attributes, vertexAttributes, surface]\n"
              "1 of 1 code\n");
}

TEST(CodeWords, WhetherACodeGoesIntoASurfaceIsExplainedAsItsSurface)
{
    // GS, an exact key, says only that its code goes into a surface.
    SurveyMap map;
    SurveyRule ground;
    ground.key = "GS";
    ground.section = SurveySection::Tinable;
    ground.tinable = true;
    ASSERT_TRUE(map.add(ground).ok());

    const katana::cad::CodeExplanation why = katana::cad::explainCode(map, "GS", {}, {});
    ASSERT_EQ(why.fields.size(), 1u);
    EXPECT_EQ(why.fields[0].field, "surface");
    EXPECT_EQ(katana::cad::formatCodeExplanation(why),
              "Code \"GS\": exact match, matched\n"
              "  surface: yes  <- rule #0 GS (surface)\n");
}

TEST(CodeWords, ALinestyleNamingADefinitionDrawnAtVerticesIsSaidToBeASymbolAtVertices)
{
    // PG* draws its line with TEST Peg, which the library holds as a symbol.
    const katana::entity::StyleLibrary library = marks();
    SurveyRule pegs;
    pegs.key = "PG*";
    pegs.model = "SURVEY MARKS";
    pegs.linestyle = "TEST Peg";
    SurveyMap map;
    ASSERT_TRUE(map.add(pegs).ok());

    // The lint: that one warning, and nothing else is wrong with the rule.
    const std::vector<LintIssue> issues = katana::cad::lintSurveyRule(pegs, 0, library, {}, {});
    ASSERT_EQ(issues.size(), 1u);
    EXPECT_EQ(issues[0].kind, LintKind::LinestyleIsVertex);
    EXPECT_EQ(issues[0].message, "linestyle \"TEST Peg\" is a symbol (at vertices), drawn at each "
                                 "vertex rather than along the line");

    // The explanation: the layer and the linestyle from rule #0, then what
    // the linestyle's name resolves to.
    const katana::cad::CodeExplanation why = katana::cad::explainCode(
        map, "PG1", [&library](std::string_view name) { return library.find(name); }, {});
    EXPECT_EQ(katana::cad::formatCodeExplanation(why),
              "Code \"PG1\": prefix match, matched\n"
              "  layer: SURVEY MARKS  <- rule #0 PG* (feature)\n"
              "  linestyle: TEST Peg  <- rule #0 PG* (feature)\n"
              "  linestyle \"TEST Peg\": defined, but as a symbol (at vertices)\n");
}

TEST(CodeWords, ALayerThatIsNotALayerPathIsAnErrorNamingTheLayerAndNeverSayingValidLayer)
{
    // "A//B" has an empty level. SurveyMap::add refuses such a rule, so the
    // lint meets it only on a rule not yet in a map: an editor's form.
    SurveyRule rule;
    rule.key = "WM*";
    rule.model = "A//B";
    const std::vector<LintIssue> issues =
        katana::cad::lintSurveyRule(rule, 0, katana::entity::StyleLibrary{}, {}, {});
    ASSERT_EQ(issues.size(), 1u);
    EXPECT_EQ(issues[0].kind, LintKind::InvalidLayerPath);
    EXPECT_EQ(issues[0].severity, LintSeverity::Error);
    EXPECT_TRUE(issues[0].message.starts_with("layer \"A//B\" is not a layer path: "))
        << issues[0].message;
    // The code manager shows this message under the Layer field, where "valid
    // layer" is what its two GOOD states say ("A valid layer the drawing
    // has."): the refusal must not read as one of them.
    EXPECT_EQ(issues[0].message.find("valid layer"), std::string::npos) << issues[0].message;
}

TEST(CodeWords, TwoLinetypesNamingSymbolsAtVerticesAreCountedInThePluralByThatWord)
{
    // Two styles, each naming a definition the library holds only as a
    // symbol: neither resolves, both name one, and the names come in name
    // order ("TEST Nail" before "TEST Peg").
    Document document;
    styledWith(document, {{"pegs", "TEST Peg"}, {"nails", "TEST Nail"}});
    EXPECT_EQ(katana::cad::formatCoverage(katana::cad::customisationCoverage(document)),
              "0 of this drawing's 2 styles are drawn with a loaded definition (2 name one; the "
              "rest are plain lines)\n"
              "  2 names are loaded as `at vertices` symbols, not linestyles, so a linetype "
              "naming one draws solid: \"TEST Nail\", \"TEST Peg\"\n");
}

TEST(CodeWords, TheStyleManagerSaysALinetypeNamingASymbolIsDrawnSolidAndWhy)
{
    // One style whose linetype is a symbol: drawn as the continuous line, and
    // the reason is that the name is defined - as a symbol at vertices.
    Document document;
    styledWith(document, {{"pegs", "TEST Peg"}});
    const std::vector<katana::cad::StyleDiagnostic> diagnostics =
        katana::cad::styleDiagnostics(document);
    ASSERT_EQ(diagnostics.size(), 1u);
    EXPECT_EQ(diagnostics[0].kind, katana::cad::StyleDiagnosticKind::MissingLinetype);
    EXPECT_EQ(diagnostics[0].name, "TEST Peg");
    EXPECT_EQ(diagnostics[0].drawnAs,
              "a solid line (continuous): an `at vertices` symbol, not a linestyle");
}

TEST(CodeWords, GivingASymbolAtVerticesAsALinetypeIsRefusedInTheStyleManagersWords)
{
    // What STYLE SET, LAYER LTYPE and Global Modify ask before taking a
    // linetype name. TEST Peg is defined - as a symbol - so it is refused by
    // what it is, in the words "Drawn as" has for the same state above, and
    // not as a name nothing defines.
    Document document;
    document.setStyleLibrary(marks());
    const katana::core::Status asked = katana::cad::checkLinetypeName(document, "TEST Peg");
    ASSERT_FALSE(asked.ok());
    EXPECT_EQ(asked.error().code, katana::core::ErrorCode::InvalidArgument);
    EXPECT_EQ(asked.error().message,
              "that is an `at vertices` symbol, not a linestyle: give it as the symbol");
    EXPECT_EQ(asked.error().context, "TEST Peg");
    // A name nothing defines is the other refusal, and says nothing of symbols.
    const katana::core::Status missing = katana::cad::checkLinetypeName(document, "TEST Spike");
    ASSERT_FALSE(missing.ok());
    EXPECT_EQ(missing.error().code, katana::core::ErrorCode::NotFound);
    EXPECT_EQ(missing.error().message.find("symbol"), std::string::npos)
        << missing.error().message;
}
