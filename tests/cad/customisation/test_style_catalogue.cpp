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

// `symbol`: its customisation lists it as a symbol (LineStyle::symbol). Two of
// these definitions were once symbols because `source` held the word
// "symbol"; the flag says so now, and the sources only say where each came
// from.
LineStyle definition(const char* name, bool atVertices, const char* source,
                     const char* group = "", bool symbol = false)
{
    LineStyle style;
    style.name = name;
    style.atVertices = atVertices;
    style.source = source;
    style.group = group;
    style.symbol = symbol;
    style.units = StyleUnits::World;
    // One stroke: a definition with none is legal but not a realistic one.
    style.strokes.push_back({katana::entity::StrokeOp::Draw, katana::geometry::Point2(1.0, 0.0)});
    return style;
}

// The drawing and customisation every test here reads.
//
//   library   TEST Water Main           listed a linestyle, not vertex -> linestyle
//             TEST Bollard        listed a symbol, `mode vertex` -> symbol
//             TEST Manhole Cover  listed a symbol, NOT vertex    -> both (D3: the listing)
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
//             cross, a built-in), Plain (linetype "0", the plain continuous line)
struct Catalogue : ::testing::Test {
    Document document;

    void SetUp() override
    {
        katana::entity::StyleLibrary library;
        for (LineStyle style : {
                 definition("TEST Water Main", false, "linestyles_test.4d", "Services/WATR"),
                 definition("TEST Bollard", true, "symbols_test.4d", "Culture", true),
                 definition("TEST Manhole Cover", false, "SYMBOLS_Test.4d", "Services/SEWR",
                            true),
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
    const DefinitionKind bollard = classifyDefinition(document, "TEST Bollard");
    EXPECT_TRUE(bollard.symbol);
    EXPECT_FALSE(bollard.linestyle) << "`mode vertex` is never a line pattern (D8)";
    EXPECT_TRUE(bollard.atVertices);
    EXPECT_TRUE(bollard.listedAsSymbol);

    // Not `mode vertex`, but its customisation lists it as a symbol. (It was
    // once the NAME of its source that said so, "symbol" in any case; the
    // definition says it itself now, so the expectation stands and the cause
    // is new.)
    const DefinitionKind manhole = classifyDefinition(document, "TEST Manhole Cover");
    EXPECT_EQ(manhole, (DefinitionKind{.linestyle = true,
                                       .symbol = true,
                                       .atVertices = false,
                                       .namedBySurveyRule = false,
                                       .namedByStyle = false,
                                       .listedAsSymbol = true}));

    const DefinitionKind palm = classifyDefinition(document, "TREE Palm");
    EXPECT_TRUE(palm.symbol && palm.namedByStyle && palm.linestyle);
    EXPECT_FALSE(palm.listedAsSymbol);

    const DefinitionKind pole = classifyDefinition(document, "ELEC Pole");
    EXPECT_TRUE(pole.symbol && pole.namedBySurveyRule);
    EXPECT_FALSE(pole.namedByStyle);

    const DefinitionKind watr = classifyDefinition(document, "TEST Water Main");
    EXPECT_TRUE(watr.linestyle);
    EXPECT_FALSE(watr.symbol);

    // A model linetype or a built-in shape is not a library definition.
    EXPECT_FALSE(classifyDefinition(document, "dash").known());
    EXPECT_FALSE(classifyDefinition(document, "cross").known());
}

TEST_F(Catalogue, ADefinitionIsListedAsASymbolByItsCustomisationAndNotByTheNameOfItsSource)
{
    // The fixture above cannot tell the two apart: its symbols also come from
    // sources named "symbols". Here the two disagree, both ways. One
    // customisation holds linestyles and symbols under ONE name, so a source's
    // name says nothing: "Roadside Symbols" would otherwise make every
    // definition in it a symbol, and "NSW" none.
    katana::entity::StyleLibrary library;
    ASSERT_TRUE(
        library.add(definition("KERB Line", false, "Roadside Symbols", "", false)).ok());
    ASSERT_TRUE(library.add(definition("SIGN Post", false, "NSW", "", true)).ok());
    document.setStyleLibrary(std::move(library));

    const DefinitionKind kerb = classifyDefinition(document, "KERB Line");
    EXPECT_FALSE(kerb.listedAsSymbol);
    EXPECT_FALSE(kerb.symbol) << "its source's name is not what makes a symbol";
    EXPECT_TRUE(kerb.linestyle);

    const DefinitionKind post = classifyDefinition(document, "SIGN Post");
    EXPECT_TRUE(post.listedAsSymbol);
    EXPECT_TRUE(post.symbol);
    EXPECT_TRUE(post.linestyle) << "not `mode vertex`, so a linestyle as well";

    // And so in the picker: the post, not the kerb.
    const std::vector<CatalogueEntry> choices = symbolChoices(document);
    EXPECT_NE(find(choices, "SIGN Post"), nullptr);
    EXPECT_EQ(find(choices, "KERB Line"), nullptr);
}

TEST_F(Catalogue, TheLinetypePickerOffersByLayerFirstThenEveryLinestyleSortedWithCaseFolded)
{
    // Non-vertex library definitions: TEST Water Main, TEST Manhole Cover, TREE
    // Palm, ELEC Pole, fence. Model linetypes: continuous, dash, fence (the
    // collision, listed once as the library's). Folded order: continuous,
    // dash, elec pole, fence, test manhole cover, test water main, tree palm
    // ("e" < "r": TEST before TREE).
    const std::vector<CatalogueEntry> choices = linetypeChoices(document);
    EXPECT_EQ(names(choices),
              (std::vector<std::string>{"ByLayer", "continuous", "dash", "ELEC Pole", "fence",
                                        "TEST Manhole Cover", "TEST Water Main", "TREE Palm"}));
    EXPECT_EQ(choices.front().source, DefinitionSource::BuiltIn);
    EXPECT_EQ(choices.front().users.styles, std::vector<std::string>{"Palm"})
        << "the styles that inherit their layer's linetype";

    const CatalogueEntry* fence = find(choices, "fence");
    ASSERT_NE(fence, nullptr);
    EXPECT_EQ(fence->source, DefinitionSource::Library) << "the library's definition draws (D2)";
    EXPECT_TRUE(fence->collision);
    const CatalogueEntry* watr = find(choices, "TEST Water Main");
    ASSERT_NE(watr, nullptr);
    EXPECT_EQ(watr->sourceFile, "linestyles_test.4d");
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
    // Library symbols: TEST Bollard, TEST Manhole Cover, TREE Palm, ELEC
    // Pole, circle (5). Built-ins: 16, less "circle", which the library
    // defines and so draws: 15. Neither TEST Water Main nor fence.
    EXPECT_EQ(choices.size(), 20u);
    EXPECT_EQ(find(choices, "TEST Water Main"), nullptr);
    EXPECT_EQ(find(choices, "fence"), nullptr);
    ASSERT_NE(find(choices, "TEST Manhole Cover"), nullptr)
        << "a NON-vertex definition listed as a symbol is a symbol";
    ASSERT_NE(find(choices, "TREE Palm"), nullptr);
    EXPECT_EQ(find(choices, "TREE Palm")->users.styles, std::vector<std::string>{"Palm"});
    EXPECT_EQ(find(choices, "circle")->source, DefinitionSource::Library);
    EXPECT_EQ(find(choices, "cross")->source, DefinitionSource::BuiltIn);
    EXPECT_EQ(find(choices, "cross")->users.styles, std::vector<std::string>{"Cross"});
    EXPECT_TRUE(find(choices, "TEST Bollard")->atVertices);

    // Sorted with case folded: "arrow" (built-in) before "dot" before "ELEC
    // Pole" before "TEST Bollard" - an upper-case name among lower-case ones is
    // placed by its letters, so "ELEC Pole" falls between "dot" and "TEST
    // Bollard" ("d" < "e" < "t"), where byte order would put both ahead of them.
    const auto position = [&](std::string_view name) {
        return std::ranges::find(choices, name, &CatalogueEntry::name) - choices.begin();
    };
    EXPECT_LT(position("arrow"), position("dot"));
    EXPECT_LT(position("dot"), position("ELEC Pole"));
    EXPECT_LT(position("ELEC Pole"), position("TEST Bollard"));
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
    // "manhole" (built-in) and "TEST Manhole Cover", by name.
    EXPECT_EQ(names(filterChoices(symbols, "MANHOLE")),
              (std::vector<std::string>{"manhole", "TEST Manhole Cover"}));
    // Both symbol-file definitions, by their file.
    EXPECT_EQ(names(filterChoices(symbols, "symbols_test")),
              (std::vector<std::string>{"TEST Bollard", "TEST Manhole Cover"}));
    // By group.
    EXPECT_EQ(names(filterChoices(linetypeChoices(document), "services/w")),
              std::vector<std::string>{"TEST Water Main"});
    EXPECT_EQ(filterChoices(symbols, "").size(), symbols.size());
}

TEST_F(Catalogue, MissingNamesAreTheOnesNothingDefinesWithWhoUsesThemAndWhatIsDrawnInstead)
{
    const std::vector<MissingName> missing = missingNames(document);
    // Linetypes, ascending: the layer's and Ghost's. Not Palm's ByLayer,
    // Plain's "0" (the plain continuous line), Cross's continuous, nor any library
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
    // Nothing defines any of the three: none is merely the wrong kind.
    for (const MissingName& name : missing) {
        EXPECT_EQ(name.status, NameStatus::Undefined) << name.name;
    }
}

TEST_F(Catalogue, EachNamesStatusIsWhatTheViewportDrawsForIt)
{
    // Linetypes. The plain names, in any case, name nothing (D4).
    for (const char* plain : {"", "ByLayer", "CONTINUOUS", "0", "1"}) {
        EXPECT_EQ(linetypeStatus(document, plain), NameStatus::Plain) << plain;
    }
    EXPECT_EQ(linetypeStatus(document, "TEST Water Main"), NameStatus::Library);
    EXPECT_EQ(linetypeStatus(document, "dash"), NameStatus::Katana) << "a model linetype";
    EXPECT_EQ(linetypeStatus(document, "fence"), NameStatus::Library)
        << "a collision: the library's linestyle is what draws (D2)";
    // `mode vertex` only: resolveLinetype gives Solid, and the linetype
    // picker (linetypeChoices, which the NamePicker lists) does not offer it.
    EXPECT_EQ(linetypeStatus(document, "TEST Bollard"), NameStatus::NotALinestyle);
    EXPECT_EQ(find(linetypeChoices(document), "TEST Bollard"), nullptr);
    EXPECT_EQ(linetypeStatus(document, "NOPE"), NameStatus::Undefined);
    // A style's own symbol as its linetype draws plain (D8), whatever kind
    // of definition the name is, and even when nothing defines it.
    EXPECT_EQ(linetypeStatus(document, "TEST Bollard", "TEST Bollard"), NameStatus::OwnSymbol);
    EXPECT_EQ(linetypeStatus(document, "TEST Manhole Cover", "TEST Manhole Cover"),
              NameStatus::OwnSymbol);
    EXPECT_EQ(linetypeStatus(document, "NOPE", "NOPE"), NameStatus::OwnSymbol);
    // ... but only its OWN symbol.
    EXPECT_EQ(linetypeStatus(document, "TEST Bollard", "TREE Palm"), NameStatus::NotALinestyle);

    // Symbols: any library definition, `mode vertex` or not (D3); else a
    // built-in shape; "circle" is defined, and the library's wins.
    EXPECT_EQ(symbolStatus(document, ""), NameStatus::Plain);
    EXPECT_EQ(symbolStatus(document, "TEST Bollard"), NameStatus::Library);
    EXPECT_EQ(symbolStatus(document, "TEST Water Main"), NameStatus::Library);
    EXPECT_EQ(symbolStatus(document, "circle"), NameStatus::Library);
    EXPECT_EQ(symbolStatus(document, "cross"), NameStatus::Katana);
    EXPECT_EQ(symbolStatus(document, "NOPE Symbol"), NameStatus::Undefined);
    EXPECT_EQ(toString(NameStatus::NotALinestyle), "not a linestyle");
}

TEST_F(Catalogue, ALinetypeNamingOnlyAVertexSymbolIsMissingAsNotALinestyle)
{
    // TEST Bollard is `mode vertex`: a symbol, never a line pattern. A layer
    // and a style that give it as their LINETYPE draw solid, and the
    // NamePicker marks it "(not defined)", so missingNames says so too -
    // with why. "circle" is the same, and a built-in shape besides: as a
    // linetype that makes it no less solid.
    katana::entity::Layer posts;
    posts.name = "posts";
    posts.linetype = "TEST Bollard";
    must(katana::commands::createLayer(posts));
    must(katana::commands::createStyle(style("Bollard Line", "TEST Bollard", "")));
    must(katana::commands::createStyle(style("Ring Line", "circle", "")));

    const std::vector<MissingName> missing = missingNames(document);
    // Linetypes ascending: MISSING Layer Linestyle, NOPE Linestyle, TEST
    // Bollard, circle ("M" < "N" < "T" < "c", by byte). Then the symbol.
    ASSERT_EQ(missing.size(), 5u);
    EXPECT_EQ(missing[2].name, "TEST Bollard");
    EXPECT_EQ(missing[2].role, NameRole::Linetype);
    EXPECT_EQ(missing[2].status, NameStatus::NotALinestyle);
    EXPECT_EQ(missing[2].users.layers, std::vector<std::string>{"posts"});
    EXPECT_EQ(missing[2].users.styles, std::vector<std::string>{"Bollard Line"});
    EXPECT_EQ(missing[2].fallback, "continuous") << "what the viewport draws: solid";
    EXPECT_EQ(missing[3].name, "circle");
    EXPECT_EQ(missing[3].status, NameStatus::NotALinestyle);
    EXPECT_EQ(missing[3].users.styles, std::vector<std::string>{"Ring Line"});
    EXPECT_EQ(missing[4].name, "NOPE Symbol Manhole");
    EXPECT_EQ(missing[4].role, NameRole::Symbol);
}

TEST_F(Catalogue, AStyleWhoseLinetypeIsItsOwnSymbolIsTheImportersPatternAndNotMissing)
{
    // What an archive import writes for a symbol string: style, linetype and
    // symbol all the symbol's name. resolveLinePattern draws the line plain
    // and the symbol at every vertex (D8), so neither name is missing -
    // for a `mode vertex` symbol and for one that is not.
    must(katana::commands::createStyle(style("TEST Bollard", "TEST Bollard", "TEST Bollard")));
    must(katana::commands::createStyle(
        style("TEST Manhole Cover", "TEST Manhole Cover", "TEST Manhole Cover")));
    // An undefined symbol in the same pattern is missing once, as a SYMBOL:
    // its linetype still draws plain.
    must(katana::commands::createStyle(style("NOPE Post", "NOPE Post", "NOPE Post")));

    std::vector<std::string> linetypes;
    std::vector<std::string> symbols;
    for (const MissingName& name : missingNames(document)) {
        (name.role == NameRole::Linetype ? linetypes : symbols).push_back(name.name);
    }
    EXPECT_EQ(linetypes,
              (std::vector<std::string>{"MISSING Layer Linestyle", "NOPE Linestyle"}));
    EXPECT_EQ(symbols, (std::vector<std::string>{"NOPE Post", "NOPE Symbol Manhole"}));
}

TEST_F(Catalogue, WhenALayerAlsoNamesAStylesOwnSymbolOnlyTheLayersLinesAreCounted)
{
    // Style "TEST Bollard" is the importer's pattern; layer "fences" gives
    // the same name as a plain linetype, which draws solid. Entities, in id
    // order: a on fences wearing the style, b on fences with no style, c on
    // layer 0 wearing the style. tableUsage counts all three against the
    // linetype (each resolves to it); a and c draw it plain under the
    // symbol, so the missing name's lines are b alone.
    katana::entity::Layer fences;
    fences.name = "fences";
    fences.linetype = "TEST Bollard";
    must(katana::commands::createLayer(fences));
    must(katana::commands::createStyle(style("TEST Bollard", "TEST Bollard", "TEST Bollard")));
    using katana::geometry::Point2;
    must(katana::commands::createLine(Point2(0, 0), Point2(1, 0), {"fences", "TEST Bollard"}));
    must(katana::commands::createLine(Point2(0, 1), Point2(1, 1), {"fences", ""}));
    must(katana::commands::createLine(Point2(0, 2), Point2(1, 2), {"0", "TEST Bollard"}));
    const std::vector<katana::entity::EntityId> ids = document.model().entities.ids();
    ASSERT_EQ(ids.size(), 3u);

    const std::vector<MissingName> missing = missingNames(document);
    const auto bollard =
        std::ranges::find(missing, std::string("TEST Bollard"), &MissingName::name);
    ASSERT_NE(bollard, missing.end());
    EXPECT_EQ(bollard->status, NameStatus::NotALinestyle);
    EXPECT_EQ(bollard->users.layers, std::vector<std::string>{"fences"});
    EXPECT_TRUE(bollard->users.styles.empty()) << "the style draws it plain (D8)";
    EXPECT_EQ(bollard->users.entities, 1u);
    EXPECT_EQ(bollard->users.firstEntity, ids[1]);
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
