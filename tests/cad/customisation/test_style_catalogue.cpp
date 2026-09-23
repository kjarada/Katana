// The style catalogue: what a linetype and a symbol picker offer (the lead's
// D2 and D3), what each library definition is, and the names a drawing uses
// that nothing defines. Hand-built libraries only - the reference files are
// not in a worktree build, and the rules have to hold for any customisation.

#include <gtest/gtest.h>

#include <algorithm>

#include "katana/cad/document.hpp"
#include "katana/cad/style_catalogue.hpp"

using namespace katana::cad;
using katana::entity::LineStyle;
using katana::entity::StyleUnits;

namespace {

LineStyle definition(const char* name, bool atVertices, const char* source,
                     const char* group = "")
{
    LineStyle style;
    style.name = name;
    style.atVertices = atVertices;
    style.source = source;
    style.group = group;
    style.units = StyleUnits::World;
    // One stroke: a definition with none is legal but not a realistic one.
    style.strokes.push_back({katana::entity::StrokeOp::Draw, katana::geometry::Point2(1.0, 0.0)});
    return style;
}

// The drawing and customisation every test here reads.
//
//   library   WATR Main           linestyle file, not vertex   -> linestyle
//             CULT Bollard        symbol file, `mode vertex`    -> symbol
//             SEWR Manhole Cover  symbol file, NOT vertex       -> both (D3: the file)
//             TREE Palm           other file, not vertex, named
//                                 by style Palm's symbol        -> both (D3: a style)
//             ELEC Pole           other file, not vertex, named
//                                 by a VertexSymbol rule        -> both (D3: the map)
//             fence               other file, not vertex; ALSO
//                                 a model linetype              -> a collision (D2)
//             circle              other file, `mode vertex`     -> hides the built-in
//   model     linetypes continuous, fence, dash
//             layer roads naming "MISSING Layer Linestyle"
//             styles Palm (ByLayer, symbol TREE Palm), Ghost (linetype
//             "NOPE Linestyle", symbol "NOPE Symbol Manhole"), Cross (symbol
//             cross, a built-in), Plain (linetype "0", 12d's plain line)
struct Catalogue : ::testing::Test {
    Document document;

    void SetUp() override
    {
        katana::entity::StyleLibrary library;
        for (LineStyle style : {
                 definition("WATR Main", false, "user_linestyl_test.4d", "Services/WATR"),
                 definition("CULT Bollard", true, "user_symbols_test.4d", "Culture"),
                 definition("SEWR Manhole Cover", false, "User_SYMBOLS_test.4d", "Services/SEWR"),
                 definition("TREE Palm", false, "extra.4d"),
                 definition("ELEC Pole", false, "extra.4d"),
                 definition("fence", false, "extra.4d"),
                 definition("circle", true, "extra.4d"),
             }) {
            ASSERT_TRUE(library.add(std::move(style)).ok());
        }
        document.setStyleLibrary(std::move(library));

        katana::entity::SurveyMap map;
        katana::entity::SurveyRule pole;
        pole.key = "EP*";
        pole.section = katana::entity::SurveySection::VertexSymbol;
        pole.symbol = katana::entity::SurveySymbol{.style = "ELEC Pole"};
        ASSERT_TRUE(map.add(pole).ok());
        document.setSurveyMap(std::move(map));

        for (const char* name : {"fence", "dash"}) {
            katana::entity::Linetype linetype;
            linetype.name = name;
            linetype.pattern = {{1.0}, {-0.5}};
            must(katana::commands::createLinetype(linetype));
        }
        katana::entity::Layer roads;
        roads.name = "roads";
        roads.linetype = "MISSING Layer Linestyle";
        must(katana::commands::createLayer(roads));
        must(katana::commands::createStyle(style("Palm", "ByLayer", "TREE Palm")));
        must(katana::commands::createStyle(
            style("Ghost", "NOPE Linestyle", "NOPE Symbol Manhole")));
        must(katana::commands::createStyle(style("Cross", "continuous", "cross")));
        must(katana::commands::createStyle(style("Plain", "0", "")));
    }

    void must(katana::commands::CommandPtr command)
    {
        const auto status = document.execute(std::move(command));
        ASSERT_TRUE(status.ok()) << status.error().describe();
    }
    static katana::entity::Style style(const char* name, const char* linetype, const char* symbol)
    {
        katana::entity::Style made;
        made.name = name;
        made.linetype = linetype;
        made.symbol = symbol;
        return made;
    }
    static std::vector<std::string> names(const std::vector<CatalogueEntry>& entries)
    {
        std::vector<std::string> result;
        for (const CatalogueEntry& entry : entries) {
            result.push_back(entry.name);
        }
        return result;
    }
    static const CatalogueEntry* find(const std::vector<CatalogueEntry>& entries,
                                      std::string_view name)
    {
        const auto found = std::ranges::find(entries, name, &CatalogueEntry::name);
        return found == entries.end() ? nullptr : &*found;
    }
};

} // namespace

TEST_F(Catalogue, ADefinitionIsASymbolForAnyOfTheFourReasonsAndALinestyleWhenNotAtVertices)
{
    const DefinitionKind bollard = classifyDefinition(document, "CULT Bollard");
    EXPECT_TRUE(bollard.symbol);
    EXPECT_FALSE(bollard.linestyle) << "`mode vertex` is never a line pattern (D8)";
    EXPECT_TRUE(bollard.atVertices);
    EXPECT_TRUE(bollard.fromSymbolFile);

    // Not `mode vertex`, but read from a file whose name says symbol - in
    // another case, which the rule folds.
    const DefinitionKind manhole = classifyDefinition(document, "SEWR Manhole Cover");
    EXPECT_EQ(manhole, (DefinitionKind{.linestyle = true,
                                       .symbol = true,
                                       .atVertices = false,
                                       .namedBySurveyRule = false,
                                       .namedByStyle = false,
                                       .fromSymbolFile = true}));

    const DefinitionKind palm = classifyDefinition(document, "TREE Palm");
    EXPECT_TRUE(palm.symbol && palm.namedByStyle && palm.linestyle);
    EXPECT_FALSE(palm.fromSymbolFile);

    const DefinitionKind pole = classifyDefinition(document, "ELEC Pole");
    EXPECT_TRUE(pole.symbol && pole.namedBySurveyRule);
    EXPECT_FALSE(pole.namedByStyle);

    const DefinitionKind watr = classifyDefinition(document, "WATR Main");
    EXPECT_TRUE(watr.linestyle);
    EXPECT_FALSE(watr.symbol);

    // A model linetype or a built-in shape is not a library definition.
    EXPECT_FALSE(classifyDefinition(document, "dash").known());
    EXPECT_FALSE(classifyDefinition(document, "cross").known());
}

TEST_F(Catalogue, TheLinetypePickerOffersByLayerFirstThenEveryLinestyleSortedWithCaseFolded)
{
    // Non-vertex library definitions: WATR Main, SEWR Manhole Cover, TREE
    // Palm, ELEC Pole, fence. Model linetypes: continuous, dash, fence (the
    // collision, listed once as the library's). Folded order: continuous,
    // dash, elec pole, fence, sewr manhole cover, tree palm, watr main.
    const std::vector<CatalogueEntry> choices = linetypeChoices(document);
    EXPECT_EQ(names(choices),
              (std::vector<std::string>{"ByLayer", "continuous", "dash", "ELEC Pole", "fence",
                                        "SEWR Manhole Cover", "TREE Palm", "WATR Main"}));
    EXPECT_EQ(choices.front().source, DefinitionSource::BuiltIn);
    EXPECT_EQ(choices.front().users.styles, std::vector<std::string>{"Palm"})
        << "the styles that inherit their layer's linetype";

    const CatalogueEntry* fence = find(choices, "fence");
    ASSERT_NE(fence, nullptr);
    EXPECT_EQ(fence->source, DefinitionSource::Library) << "the library's definition draws (D2)";
    EXPECT_TRUE(fence->collision);
    const CatalogueEntry* watr = find(choices, "WATR Main");
    ASSERT_NE(watr, nullptr);
    EXPECT_EQ(watr->sourceFile, "user_linestyl_test.4d");
    EXPECT_EQ(watr->group, "Services/WATR");
    EXPECT_EQ(watr->units, StyleUnits::World);
    EXPECT_FALSE(watr->collision);
    EXPECT_EQ(find(choices, "dash")->source, DefinitionSource::ModelLinetype);

    // A layer's picker has no ByLayer: a layer is what it inherits from.
    const std::vector<CatalogueEntry> forLayer = linetypeChoices(document, false);
    EXPECT_EQ(forLayer.size(), choices.size() - 1);
    EXPECT_EQ(forLayer.front().name, "continuous");
}

TEST_F(Catalogue, TheSymbolPickerOffersEveryD3SymbolAndTheBuiltInsALibraryDoesNotHide)
{
    const std::vector<CatalogueEntry> choices = symbolChoices(document);
    // Library symbols: CULT Bollard, SEWR Manhole Cover, TREE Palm, ELEC
    // Pole, circle (5). Built-ins: 16, less "circle", which the library
    // defines and so draws: 15. Neither WATR Main nor fence.
    EXPECT_EQ(choices.size(), 20u);
    EXPECT_EQ(find(choices, "WATR Main"), nullptr);
    EXPECT_EQ(find(choices, "fence"), nullptr);
    ASSERT_NE(find(choices, "SEWR Manhole Cover"), nullptr)
        << "a NON-vertex definition from a symbol file is a symbol";
    ASSERT_NE(find(choices, "TREE Palm"), nullptr);
    EXPECT_EQ(find(choices, "TREE Palm")->users.styles, std::vector<std::string>{"Palm"});
    EXPECT_EQ(find(choices, "circle")->source, DefinitionSource::Library);
    EXPECT_EQ(find(choices, "cross")->source, DefinitionSource::BuiltIn);
    EXPECT_EQ(find(choices, "cross")->users.styles, std::vector<std::string>{"Cross"});
    EXPECT_TRUE(find(choices, "CULT Bollard")->atVertices);

    // Sorted with case folded: "arrow" (built-in) before "CULT Bollard"
    // before "dot" before "ELEC Pole".
    const auto position = [&](std::string_view name) {
        return std::ranges::find(choices, name, &CatalogueEntry::name) - choices.begin();
    };
    EXPECT_LT(position("arrow"), position("CULT Bollard"));
    EXPECT_LT(position("CULT Bollard"), position("dot"));
    EXPECT_LT(position("dot"), position("ELEC Pole"));
}

TEST_F(Catalogue, APickerKeepsTheCurrentValueItCannotOfferMarkedRatherThanDroppingIt)
{
    // The real QT-02 fix: whatever the list holds, the value being edited
    // is shown, so saving the form cannot silently replace it.
    const std::vector<CatalogueEntry> choices = linetypeChoices(document);
    const std::vector<CatalogueEntry> kept = keepCurrent(choices, "OLD Linestyle");
    ASSERT_EQ(kept.size(), choices.size() + 1);
    EXPECT_EQ(kept.front().name, "OLD Linestyle");
    EXPECT_TRUE(kept.front().missing);
    EXPECT_EQ(kept.front().source, DefinitionSource::Undefined);

    EXPECT_EQ(keepCurrent(choices, "dash").size(), choices.size()) << "already offered";
    EXPECT_EQ(keepCurrent(choices, "").size(), choices.size()) << "nothing to keep";
    // Names are case-sensitive: "DASH" is not "dash", and is kept as itself.
    EXPECT_EQ(keepCurrent(choices, "DASH").front().name, "DASH");
}

TEST_F(Catalogue, ASearchFoldsCaseOverNameGroupAndSourceFile)
{
    const std::vector<CatalogueEntry> symbols = symbolChoices(document);
    // "manhole" (built-in) and "SEWR Manhole Cover", by name.
    EXPECT_EQ(names(filterChoices(symbols, "MANHOLE")),
              (std::vector<std::string>{"manhole", "SEWR Manhole Cover"}));
    // Both symbol-file definitions, by their file.
    EXPECT_EQ(names(filterChoices(symbols, "symbols_test")),
              (std::vector<std::string>{"CULT Bollard", "SEWR Manhole Cover"}));
    // By group.
    EXPECT_EQ(names(filterChoices(linetypeChoices(document), "services/w")),
              std::vector<std::string>{"WATR Main"});
    EXPECT_EQ(filterChoices(symbols, "").size(), symbols.size());
}

TEST_F(Catalogue, MissingNamesAreTheOnesNothingDefinesWithWhoUsesThemAndWhatIsDrawnInstead)
{
    const std::vector<MissingName> missing = missingNames(document);
    // Linetypes, ascending: the layer's and Ghost's. Not Palm's ByLayer,
    // Plain's "0" (12d's plain line), Cross's continuous, nor any library
    // name. Then the one symbol: Ghost's. Not "cross" - a built-in draws
    // as itself (audit CAD-17's symptom) - nor TREE Palm, which is defined.
    ASSERT_EQ(missing.size(), 3u);

    EXPECT_EQ(missing[0].name, "MISSING Layer Linestyle");
    EXPECT_EQ(missing[0].role, NameRole::Linetype);
    EXPECT_EQ(missing[0].users.layers, std::vector<std::string>{"roads"});
    EXPECT_EQ(missing[0].fallback, "continuous");

    EXPECT_EQ(missing[1].name, "NOPE Linestyle");
    EXPECT_EQ(missing[1].users.styles, std::vector<std::string>{"Ghost"});

    EXPECT_EQ(missing[2].name, "NOPE Symbol Manhole");
    EXPECT_EQ(missing[2].role, NameRole::Symbol);
    EXPECT_EQ(missing[2].users.styles, std::vector<std::string>{"Ghost"});
    // builtInSymbolFor: "manhole" is in the name, so the manhole shape.
    EXPECT_EQ(missing[2].fallback, "manhole");
}

TEST_F(Catalogue, ANameThatIsBothAModelLinetypeAndALibraryLinestyleIsACollision)
{
    EXPECT_EQ(linetypeCollisions(document), std::vector<std::string>{"fence"});
    // A vertex definition of the same name as a model linetype is not one:
    // a symbol is never drawn as a line pattern (D8), so the dashes stand.
    katana::entity::StyleLibrary library = document.styleLibrary();
    ASSERT_TRUE(library.add(definition("dash", true, "extra.4d")).ok());
    document.setStyleLibrary(std::move(library));
    EXPECT_EQ(linetypeCollisions(document), std::vector<std::string>{"fence"});
}
