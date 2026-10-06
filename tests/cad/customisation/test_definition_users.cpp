// cad::definitionUsers: what names a library definition, and so what is left
// naming nothing when it is removed. The customisation and the drawing below
// are written out by hand, and every expected list is read off them in the
// comment beside it.
//
// The rules, by index (a rule has no identity but its place):
//   #0  WM*  feature           linestyle "TEST Water Main"
//   #1  AC*  symbol            symbol    "TEST Valve"
//   #2  GT*  feature           linestyle "TEST Valve" AND symbol "TEST Valve"
//   #3  WM*  attributes        symbol    "TEST Water Main"   (a member a rule of that
//                                                             kind is not expected to
//                                                             carry: the model allows it)
//   #4  KB*  feature           linestyle "TEST Kerb"
//   #5  *    feature           nothing
// The styles:
//   Marks   symbol   "TEST Valve"
//   Mains   linetype "TEST Water Main"
//   Both    linetype "TEST Valve", symbol "TEST Valve"
//   Other   linetype "test valve"      (another name: names compare with case)
// The layers (besides the default "0"):
//   services  linetype "TEST Water Main"
//   kerbs     linetype "TEST Kerb"

#include <gtest/gtest.h>

#include <array>
#include <string>
#include <vector>

#include "katana/cad/customisation_record.hpp"
#include "katana/cad/definition_users.hpp"
#include "katana/cad/document.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/style_library.hpp"
#include "katana/entity/survey_map.hpp"
#include "katana/entity/tables.hpp"

using katana::cad::DefinitionRule;
using katana::cad::DefinitionUsers;
using katana::cad::definitionUsers;
using katana::cad::Document;
using katana::entity::LineStyle;
using katana::entity::SurveyRule;
using katana::entity::SurveySection;

namespace {

void define(katana::entity::StyleLibrary& library, const char* name)
{
    LineStyle definition;
    definition.name = name;
    const auto added = library.add(definition);
    ASSERT_TRUE(added.ok()) << added.error().describe();
}

SurveyRule rule(const char* key, SurveySection section, const char* linestyle, const char* symbol)
{
    SurveyRule made;
    made.key = key;
    made.section = section;
    made.linestyle = linestyle;
    if (symbol != nullptr) {
        made.symbol = katana::entity::SurveySymbol{};
        made.symbol->style = symbol;
    }
    return made;
}

void must(Document& document, katana::commands::CommandPtr command)
{
    const auto status = document.execute(std::move(command));
    ASSERT_TRUE(status.ok()) << status.error().describe();
}

// The customisation and the drawing of the file comment.
void makeDrawing(Document& document)
{
    katana::entity::StyleLibrary library;
    for (const char* name : {"TEST Water Main", "TEST Valve", "TEST Kerb", "TEST Unused"}) {
        define(library, name);
    }
    document.setStyleLibrary(std::move(library));

    katana::entity::SurveyMap map;
    for (const SurveyRule& each :
         {rule("WM*", SurveySection::Map, "TEST Water Main", nullptr),
          rule("AC*", SurveySection::VertexSymbol, "", "TEST Valve"),
          rule("GT*", SurveySection::Map, "TEST Valve", "TEST Valve"),
          rule("WM*", SurveySection::StringAttribute, "", "TEST Water Main"),
          rule("KB*", SurveySection::Map, "TEST Kerb", nullptr),
          rule("*", SurveySection::Map, "", nullptr)}) {
        const auto added = map.add(each);
        ASSERT_TRUE(added.ok()) << added.error().describe();
    }
    document.setSurveyMap(std::move(map));

    struct StyleSpec {
        const char* name;
        const char* linetype;
        const char* symbol;
    };
    for (const StyleSpec& spec : {StyleSpec{"Marks", "continuous", "TEST Valve"},
                                  StyleSpec{"Mains", "TEST Water Main", ""},
                                  StyleSpec{"Both", "TEST Valve", "TEST Valve"},
                                  StyleSpec{"Other", "test valve", ""}}) {
        katana::entity::Style style;
        style.name = spec.name;
        style.linetype = spec.linetype;
        style.symbol = spec.symbol;
        must(document, katana::commands::createStyle(style));
    }
    for (const auto& [name, linetype] : std::vector<std::pair<const char*, const char*>>{
             {"services", "TEST Water Main"}, {"kerbs", "TEST Kerb"}}) {
        katana::entity::Layer layer;
        layer.name = name;
        layer.linetype = linetype;
        must(document, katana::commands::createLayer(layer));
    }
}

TEST(DefinitionUsers, TheRulesStylesAndLayersNamingADefinitionAreListedEachByHowItNamesIt)
{
    Document document;
    makeDrawing(document);

    // "TEST Valve": rule #1 draws it, rule #2 names it both ways; Both names
    // it as a linetype, and Both and Marks as a symbol (ascending: B before
    // M). No layer names it. "test valve" is another name.
    const DefinitionUsers valve = definitionUsers(document, "TEST Valve");
    EXPECT_EQ(valve.rules,
              (std::vector<DefinitionRule>{
                  {1, "AC*", SurveySection::VertexSymbol, false, true},
                  {2, "GT*", SurveySection::Map, true, true}}));
    EXPECT_EQ(valve.linetypeStyles, (std::vector<std::string>{"Both"}));
    EXPECT_EQ(valve.symbolStyles, (std::vector<std::string>{"Both", "Marks"}));
    EXPECT_TRUE(valve.layers.empty());
    EXPECT_FALSE(valve.empty());

    // "TEST Water Main": rule #0 as a linestyle, rule #3 - an attributes rule
    // carrying a symbol - as a symbol; the style Mains and the layer services.
    const DefinitionUsers main = definitionUsers(document, "TEST Water Main");
    EXPECT_EQ(main.rules,
              (std::vector<DefinitionRule>{
                  {0, "WM*", SurveySection::Map, true, false},
                  {3, "WM*", SurveySection::StringAttribute, false, true}}));
    EXPECT_EQ(main.linetypeStyles, (std::vector<std::string>{"Mains"}));
    EXPECT_TRUE(main.symbolStyles.empty());
    EXPECT_EQ(main.layers, (std::vector<std::string>{"services"}));
}

TEST(DefinitionUsers, ADefinitionNothingNamesHasNoUsersAndNeitherHasNoName)
{
    Document document;
    makeDrawing(document);

    EXPECT_TRUE(definitionUsers(document, "TEST Unused").empty());
    EXPECT_TRUE(definitionUsers(document, "TEST Unused").describe().empty());
    // A name no library defines is still only a name: nothing names this one.
    EXPECT_TRUE(definitionUsers(document, "TEST Nowhere").empty());
    // No name is not a name, although rule #5 has an empty linestyle and the
    // style Mains an empty symbol.
    EXPECT_TRUE(definitionUsers(document, "").empty());
    // An empty drawing with no customisation.
    EXPECT_TRUE(definitionUsers(Document{}, "TEST Valve").empty());
}

TEST(DefinitionUsers, NamesCompareWithRegardToCase)
{
    Document document;
    makeDrawing(document);

    // Only the style Other spells it so, as its linetype.
    const DefinitionUsers lower = definitionUsers(document, "test valve");
    EXPECT_TRUE(lower.rules.empty());
    EXPECT_EQ(lower.linetypeStyles, (std::vector<std::string>{"Other"}));
    EXPECT_TRUE(lower.symbolStyles.empty());
    EXPECT_TRUE(lower.layers.empty());
}

TEST(DefinitionUsers, EachUserIsDescribedOnALineOfItsOwnInTheOrderOfTheLists)
{
    Document document;
    makeDrawing(document);

    EXPECT_EQ(definitionUsers(document, "TEST Valve").describe(),
              (std::vector<std::string>{
                  "rule #1 AC* (symbol) draws it as its symbol",
                  "rule #2 GT* (feature) names it as its linestyle and its symbol",
                  "style \"Both\" names it as its linetype",
                  "style \"Both\" draws it as its symbol",
                  "style \"Marks\" draws it as its symbol"}));
    EXPECT_EQ(definitionUsers(document, "TEST Kerb").describe(),
              (std::vector<std::string>{"rule #4 KB* (feature) names it as its linestyle",
                                        "layer \"kerbs\" names it as its linetype"}));
}

TEST(DefinitionUsers, AnEarlierNameOfARenamedDefinitionCountsUntilTheLibraryDefinesThatNameItself)
{
    // "OLD Valve" is the name "TEST Valve" had before it was renamed. A style
    // and a rule still give the earlier name, and the resolver draws them with
    // "TEST Valve" (findDefinition), so removing it would leave them drawn as
    // a stand-in: they use it.
    const std::array<katana::cad::RenamedDefinition, 1> renamed{
        {{katana::cad::sourceNameHash("OLD Valve"), "TEST Valve"}}};
    Document document;
    makeDrawing(document);
    katana::entity::Style old;
    old.name = "Earlier";
    old.symbol = "OLD Valve";
    must(document, katana::commands::createStyle(old));
    katana::entity::SurveyMap map = document.surveyMap();
    ASSERT_TRUE(map.add(rule("OV*", SurveySection::VertexSymbol, "", "OLD Valve")).ok());
    document.setSurveyMap(map);

    const DefinitionUsers users = definitionUsers(document, "TEST Valve", renamed);
    // Rules #1 and #2 as before, and the new rule, #6; the style Earlier
    // sorts between Both and Marks.
    ASSERT_EQ(users.rules.size(), 3u);
    EXPECT_EQ(users.rules.back(),
              (DefinitionRule{6, "OV*", SurveySection::VertexSymbol, false, true}));
    EXPECT_EQ(users.symbolStyles, (std::vector<std::string>{"Both", "Earlier", "Marks"}));
    // Without the rename nothing ties the earlier name to it.
    EXPECT_EQ(definitionUsers(document, "TEST Valve", {}).symbolStyles,
              (std::vector<std::string>{"Both", "Marks"}));
    // The earlier name's own users are the two that spell it, whatever is renamed.
    EXPECT_EQ(definitionUsers(document, "OLD Valve", renamed).symbolStyles,
              (std::vector<std::string>{"Earlier"}));

    // Once the library holds a definition of the earlier name itself, that
    // one is what is drawn, and "TEST Valve" is no longer used through it.
    katana::entity::StyleLibrary library = document.styleLibrary();
    define(library, "OLD Valve");
    document.setStyleLibrary(std::move(library));
    const DefinitionUsers after = definitionUsers(document, "TEST Valve", renamed);
    EXPECT_EQ(after.rules.size(), 2u);
    EXPECT_EQ(after.symbolStyles, (std::vector<std::string>{"Both", "Marks"}));
}

TEST(DefinitionUsers, WhatElseAnswersToTheNameIsSaidBesideItsUsersAndIsNotOneOfThem)
{
    Document document;
    makeDrawing(document);

    // Only the library holds "TEST Kerb"; rule #4 and the layer kerbs name it
    // as a linetype and nothing as a symbol.
    const DefinitionUsers kerb = definitionUsers(document, "TEST Kerb");
    EXPECT_FALSE(kerb.drawingLinetype);
    EXPECT_FALSE(kerb.builtInShape);
    EXPECT_TRUE(kerb.namedAsLinetype());
    EXPECT_FALSE(kerb.namedAsSymbol());
    // "TEST Valve": rule #2 names it as a linestyle, rule #1 draws it.
    const DefinitionUsers valve = definitionUsers(document, "TEST Valve");
    EXPECT_TRUE(valve.namedAsLinetype());
    EXPECT_TRUE(valve.namedAsSymbol());
    const DefinitionUsers unused = definitionUsers(document, "TEST Unused");
    EXPECT_FALSE(unused.namedAsLinetype());
    EXPECT_FALSE(unused.namedAsSymbol());

    // The DRAWING holds a linetype "Fence" of its own and a layer names it; no
    // library definition has that name. The layer is drawn with the drawing's
    // dashes (decision D2) - not as a plain line, which is what naming
    // nothing would mean - and a definition made under the name would take
    // the layer from them.
    katana::entity::Linetype fence;
    fence.name = "Fence";
    fence.pattern = {katana::entity::LinetypeElement{2.0}, katana::entity::LinetypeElement{-1.0}};
    must(document, katana::commands::createLinetype(fence));
    katana::entity::Layer fences;
    fences.name = "fences";
    fences.linetype = "Fence";
    must(document, katana::commands::createLayer(fences));
    const DefinitionUsers fenceUsers = definitionUsers(document, "Fence");
    EXPECT_TRUE(fenceUsers.drawingLinetype);
    EXPECT_FALSE(fenceUsers.builtInShape);
    EXPECT_EQ(fenceUsers.layers, (std::vector<std::string>{"fences"}));
    // The fact is not a user: the layer alone is listed, and with no layer
    // there would be nothing to list.
    EXPECT_EQ(fenceUsers.describe(),
              (std::vector<std::string>{"layer \"fences\" names it as its linetype"}));
    katana::entity::Linetype lone;
    lone.name = "Lone";
    lone.pattern = fence.pattern;
    must(document, katana::commands::createLinetype(lone));
    const DefinitionUsers loneUsers = definitionUsers(document, "Lone");
    EXPECT_TRUE(loneUsers.drawingLinetype);
    EXPECT_TRUE(loneUsers.empty());
    EXPECT_TRUE(loneUsers.describe().empty());

    // "circle" is one of the built-in shapes, and a style draws its points
    // with it: with no library definition of the name they are drawn as that
    // shape, not as a stand-in for a missing one.
    katana::entity::Style rings;
    rings.name = "Rings";
    rings.symbol = "circle";
    must(document, katana::commands::createStyle(rings));
    const DefinitionUsers circle = definitionUsers(document, "circle");
    EXPECT_TRUE(circle.builtInShape);
    EXPECT_FALSE(circle.drawingLinetype);
    EXPECT_EQ(circle.symbolStyles, (std::vector<std::string>{"Rings"}));
    EXPECT_TRUE(circle.namedAsSymbol());
    EXPECT_FALSE(circle.namedAsLinetype());
}

} // namespace
