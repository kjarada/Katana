// The styles and linetypes manager below Qt: its rows, its chips and search,
// the fields a multi-selection agrees on, the one command a bulk edit makes,
// its diagnostics and "Select Users". Hand-built library and drawing only.

#include <gtest/gtest.h>

#include <algorithm>
#include <string>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/cad/style_manager_rows.hpp"
#include "katana/commands/entity_commands.hpp"

using namespace katana::cad;
using katana::entity::LineStyle;
using katana::entity::Stroke;
using katana::entity::StrokeOp;
using katana::entity::Style;
using katana::entity::StyleUnits;

namespace {

LineStyle definition(const char* name, StyleUnits units, bool atVertices, double length,
                     std::vector<Stroke> strokes, const char* group = "",
                     const char* source = "")
{
    LineStyle style;
    style.name = name;
    style.units = units;
    style.atVertices = atVertices;
    style.length = length;
    style.strokes = std::move(strokes);
    style.group = group;
    style.source = source;
    return style;
}

// The drawing every test reads.
//
//   library   LS Paper Kerb   paperstyle, `length 4`                 period 4 (mm)
//             LS World Fence  worldstyle, no length; move 0, draw 3,
//                             move 5 - the pen travels 0..5          period 5
//             LS Gate         twoptstyle                             no period
//             dash            worldstyle - ALSO a drawing linetype   a collision (D2)
//             SYM Pit         `mode vertex`                          a symbol, not listed
//   linetypes continuous; dash {1, -0.5} (period 1.5); Dotted {0, -0.25} (0.25)
//   layer     kerbs, linetype dash
//   styles    Kerb   LS Paper Kerb, 0.35      worn by points A and B
//             Fence  LS World Fence, 0.35     worn by nothing
//             Ghost  NOPE Line / NOPE Manhole worn by point C; both names undefined
//             Pit    continuous / cross       worn by nothing; cross is built in
struct StyleManagerRows : ::testing::Test {
    Document document;
    std::vector<katana::entity::EntityId> kerbPoints;
    katana::entity::EntityId ghostPoint = katana::entity::kInvalidEntityId;

    void SetUp() override
    {
        katana::entity::StyleLibrary library;
        const Stroke move0{StrokeOp::Move, katana::geometry::Point2(0.0, 0.0)};
        const Stroke draw3{StrokeOp::Draw, katana::geometry::Point2(3.0, 0.0)};
        const Stroke move5{StrokeOp::Move, katana::geometry::Point2(5.0, 0.0)};
        for (LineStyle style : {
                 definition("LS Paper Kerb", StyleUnits::Paper, false, 4.0, {move0, draw3},
                            "Roads/Kerbs", "user_linestyl_test.4d"),
                 definition("LS World Fence", StyleUnits::World, false, 0.0,
                            {move0, draw3, move5}),
                 definition("LS Gate", StyleUnits::TwoPoint, false, 0.0, {move0, draw3}),
                 definition("dash", StyleUnits::World, false, 0.0, {move0, draw3, move5}),
                 definition("SYM Pit", StyleUnits::World, true, 0.0, {move0, draw3}),
             }) {
            ASSERT_TRUE(library.add(std::move(style)).ok());
        }
        document.setStyleLibrary(std::move(library));

        katana::entity::Linetype dash;
        dash.name = "dash";
        dash.pattern = {{1.0}, {-0.5}};
        must(katana::commands::createLinetype(dash));
        katana::entity::Linetype dotted;
        dotted.name = "Dotted";
        dotted.description = "a dot every quarter metre";
        dotted.pattern = {{0.0}, {-0.25}};
        must(katana::commands::createLinetype(dotted));
        katana::entity::Layer kerbs;
        kerbs.name = "kerbs";
        kerbs.linetype = "dash";
        must(katana::commands::createLayer(kerbs));

        must(katana::commands::createStyle(style("Kerb", "LS Paper Kerb", "", 0.35)));
        must(katana::commands::createStyle(style("Fence", "LS World Fence", "", 0.35)));
        must(katana::commands::createStyle(style("Ghost", "NOPE Line", "NOPE Manhole", 0.25)));
        must(katana::commands::createStyle(style("Pit", "continuous", "cross", 0.25)));

        for (double x : {0.0, 1.0}) {
            must(katana::commands::createPoint(katana::geometry::Point2(x, 0.0),
                                               {.layer = "0", .style = "Kerb"}));
            kerbPoints.push_back(document.lastCreatedEntities().front());
        }
        must(katana::commands::createPoint(katana::geometry::Point2(2.0, 0.0),
                                           {.layer = "0", .style = "Ghost"}));
        ghostPoint = document.lastCreatedEntities().front();
    }

    void must(katana::commands::CommandPtr command)
    {
        const auto status = document.execute(std::move(command));
        ASSERT_TRUE(status.ok()) << status.error().describe();
    }
    static Style style(const char* name, const char* linetype, const char* symbol, double weight)
    {
        Style made;
        made.name = name;
        made.linetype = linetype;
        made.symbol = symbol;
        made.lineWeight = weight;
        return made;
    }
    const StyleRow& row(const std::vector<StyleRow>& rows, const char* name)
    {
        const auto found = std::ranges::find_if(
            rows, [name](const StyleRow& candidate) { return candidate.style.name == name; });
        EXPECT_NE(found, rows.end()) << name;
        return *found;
    }
    std::vector<std::string> filtered(StyleFilter filter)
    {
        std::vector<std::string> names;
        for (const StyleRow& candidate : styleRows(document)) {
            if (matchesFilter(candidate, filter)) {
                names.push_back(candidate.style.name);
            }
        }
        return names;
    }
    std::vector<std::string> searched(std::string_view text)
    {
        std::vector<std::string> names;
        for (const StyleRow& candidate : styleRows(document)) {
            if (matchesSearch(candidate, text)) {
                names.push_back(candidate.style.name);
            }
        }
        return names;
    }
};

using Strings = std::vector<std::string>;

} // namespace

TEST_F(StyleManagerRows, AStyleRowCountsTheEntitiesWearingItAndMarksTheNamesNothingDefines)
{
    const std::vector<StyleRow> rows = styleRows(document);
    ASSERT_EQ(rows.size(), 4U);
    // The table's order: ascending by name.
    EXPECT_EQ(rows[0].style.name, "Fence");
    EXPECT_EQ(rows[3].style.name, "Pit");

    EXPECT_EQ(row(rows, "Kerb").entities, 2U);
    EXPECT_FALSE(row(rows, "Kerb").missing()) << "a library linestyle is defined";
    EXPECT_EQ(row(rows, "Ghost").entities, 1U);
    EXPECT_TRUE(row(rows, "Ghost").missingLinetype);
    EXPECT_TRUE(row(rows, "Ghost").missingSymbol);
    EXPECT_EQ(row(rows, "Pit").entities, 0U);
    EXPECT_FALSE(row(rows, "Pit").missing()) << "continuous and the built-in cross need nothing";
    EXPECT_EQ(row(rows, "Kerb").style.linetype, "LS Paper Kerb") << "the row carries the style";
}

TEST_F(StyleManagerRows, TheCurrentStyleIsMarkedOnItsRowAlone)
{
    for (const StyleRow& candidate : styleRows(document)) {
        EXPECT_FALSE(candidate.current) << "a new drawing draws ByLayer (D9)";
    }
    ASSERT_TRUE(document.setCurrentStyle("Fence").ok());
    const std::vector<StyleRow> rows = styleRows(document);
    EXPECT_TRUE(row(rows, "Fence").current);
    EXPECT_FALSE(row(rows, "Kerb").current);
    EXPECT_FALSE(row(rows, "Pit").current);
}

TEST_F(StyleManagerRows, TheChipsSplitUsedFromUnusedAndPickOutTheMissing)
{
    // Used = worn by an entity: Kerb (2) and Ghost (1); Fence and Pit by none.
    EXPECT_EQ(filtered(StyleFilter::All), (Strings{"Fence", "Ghost", "Kerb", "Pit"}));
    EXPECT_EQ(filtered(StyleFilter::Used), (Strings{"Ghost", "Kerb"}));
    EXPECT_EQ(filtered(StyleFilter::Unused), (Strings{"Fence", "Pit"}));
    EXPECT_EQ(filtered(StyleFilter::Missing), (Strings{"Ghost"}));
}

TEST_F(StyleManagerRows, TheSearchFoldsCaseAndLooksInTheNameLinetypeSymbolAndDescription)
{
    EXPECT_EQ(searched(""), (Strings{"Fence", "Ghost", "Kerb", "Pit"}));
    // "kerb" is Kerb's name and, in another case, in its linetype.
    EXPECT_EQ(searched("kerb"), (Strings{"Kerb"}));
    // "PAPER" only in Kerb's linetype "LS Paper Kerb".
    EXPECT_EQ(searched("PAPER"), (Strings{"Kerb"}));
    // "manhole" only in Ghost's symbol "NOPE Manhole".
    EXPECT_EQ(searched("manhole"), (Strings{"Ghost"}));
    // "continuous": Pit's linetype.
    EXPECT_EQ(searched("Continuous"), (Strings{"Pit"}));
    EXPECT_TRUE(searched("zzz").empty());
}

TEST_F(StyleManagerRows, ASelectionAgreesOnlyOnTheFieldsEveryStyleShares)
{
    const Style& kerb = *document.model().styles.find("Kerb");
    const Style& fence = *document.model().styles.find("Fence");
    const StyleFields common = commonFields({kerb, fence});
    // Both 0.35 wide, both ByLayer colour, no symbol, size 0, no hatch, no
    // description; their linetypes differ.
    EXPECT_FALSE(common.linetype.has_value()) << "LS Paper Kerb against LS World Fence: <varies>";
    ASSERT_TRUE(common.lineWeight.has_value());
    EXPECT_EQ(*common.lineWeight, 0.35);
    ASSERT_TRUE(common.color.has_value()) << "both ByLayer is an agreement, not <varies>";
    EXPECT_FALSE(common.color->has_value());
    EXPECT_EQ(common.symbol, std::optional<std::string>(""));
    EXPECT_EQ(common.hatchPattern, std::optional<std::string>(""));

    // One style agrees with itself on everything; no styles on nothing.
    const StyleFields one = commonFields({kerb});
    EXPECT_EQ(one.linetype, std::optional<std::string>("LS Paper Kerb"));
    EXPECT_EQ(one.description, std::optional<std::string>(""));
    EXPECT_TRUE(commonFields({}).empty());
}

TEST_F(StyleManagerRows, AnEditWritesOnlyTheFieldsItSetsAndNeverTheName)
{
    const Style& ghost = *document.model().styles.find("Ghost");
    StyleFields edit;
    edit.lineWeight = 0.7;
    edit.color = katana::entity::Color{255, 0, 0, 255};
    const Style edited = applyEdit(ghost, edit);
    EXPECT_EQ(edited.name, "Ghost");
    EXPECT_EQ(edited.lineWeight, 0.7);
    EXPECT_EQ(edited.color, (katana::entity::Color{255, 0, 0, 255}));
    // Untouched: the undefined names stay exactly as they were (QT-02).
    EXPECT_EQ(edited.linetype, "NOPE Line");
    EXPECT_EQ(edited.symbol, "NOPE Manhole");
    EXPECT_EQ(applyEdit(ghost, StyleFields{}), ghost);
}

TEST_F(StyleManagerRows, AnEditThatChangesNothingMakesNoCommand)
{
    const std::size_t before = document.history().undoCount();
    auto empty = editStylesCommand(document.model(), {"Kerb", "Fence"}, StyleFields{});
    ASSERT_TRUE(empty.ok());
    EXPECT_EQ(*empty, nullptr);

    // Both are already 0.35 wide: setting 0.35 changes neither.
    StyleFields same;
    same.lineWeight = 0.35;
    auto unchanged = editStylesCommand(document.model(), {"Kerb", "Fence"}, same);
    ASSERT_TRUE(unchanged.ok());
    EXPECT_EQ(*unchanged, nullptr);
    EXPECT_EQ(document.history().undoCount(), before);
}

TEST_F(StyleManagerRows, ABulkEditIsOneUndoStepAndLeavesEachStylesOtherFieldsItsOwn)
{
    const std::size_t before = document.history().undoCount();
    StyleFields edit;
    edit.lineWeight = 0.5;
    auto command = editStylesCommand(document.model(), {"Kerb", "Fence", "Pit"}, edit);
    ASSERT_TRUE(command.ok()) << command.error().describe();
    ASSERT_NE(*command, nullptr);
    must(std::move(*command));
    EXPECT_EQ(document.history().undoCount(), before + 1) << "three styles, one step";

    const auto& styles = document.model().styles;
    for (const char* name : {"Kerb", "Fence", "Pit"}) {
        EXPECT_EQ(styles.find(name)->lineWeight, 0.5) << name;
    }
    EXPECT_EQ(styles.find("Kerb")->linetype, "LS Paper Kerb");
    EXPECT_EQ(styles.find("Fence")->linetype, "LS World Fence");
    EXPECT_EQ(styles.find("Pit")->symbol, "cross");

    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(styles.find("Kerb")->lineWeight, 0.35);
    EXPECT_EQ(styles.find("Fence")->lineWeight, 0.35);
    EXPECT_EQ(styles.find("Pit")->lineWeight, 0.25);
}

TEST_F(StyleManagerRows, AnEditOneStyleRefusesIsRefusedWholeNamingTheStyle)
{
    StyleFields negative;
    negative.lineWeight = -1.0;
    auto refused = editStylesCommand(document.model(), {"Kerb", "Fence"}, negative);
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, katana::core::ErrorCode::InvalidArgument);
    EXPECT_NE(refused.error().context.find("Kerb"), std::string::npos)
        << refused.error().describe();

    auto missing = editStylesCommand(document.model(), {"Kerb", "Nobody"}, StyleFields{});
    ASSERT_FALSE(missing.ok());
    EXPECT_EQ(missing.error().code, katana::core::ErrorCode::NotFound);
    EXPECT_EQ(missing.error().context, "Nobody");
    EXPECT_EQ(document.model().styles.find("Kerb")->lineWeight, 0.35);
}

TEST_F(StyleManagerRows, DrawingLinetypesAndLibraryLinestylesAreOneListAndACollisionIsTwoRows)
{
    const std::vector<LinetypeRow> rows = linetypeRows(document);
    // Folded order: continuous, dash, dash, dotted, ls gate, ls paper kerb,
    // ls world fence; the drawing's dash before the library's. SYM Pit is
    // `mode vertex`: a symbol, never a linestyle (D2).
    std::vector<std::pair<std::string, LinetypeOrigin>> seen;
    for (const LinetypeRow& candidate : rows) {
        seen.emplace_back(candidate.name, candidate.origin);
    }
    const std::vector<std::pair<std::string, LinetypeOrigin>> expected{
        {"continuous", LinetypeOrigin::Drawing},    {"dash", LinetypeOrigin::Drawing},
        {"dash", LinetypeOrigin::Library},          {"Dotted", LinetypeOrigin::Drawing},
        {"LS Gate", LinetypeOrigin::Library},       {"LS Paper Kerb", LinetypeOrigin::Library},
        {"LS World Fence", LinetypeOrigin::Library}};
    EXPECT_EQ(seen, expected);
    EXPECT_TRUE(rows[1].collision);
    EXPECT_TRUE(rows[2].collision);
    EXPECT_FALSE(rows[0].collision);
    EXPECT_FALSE(rows[5].collision);
    // Both rows of the collision answer to one name: layer kerbs names it.
    EXPECT_EQ(rows[1].users.layers, Strings{"kerbs"});
    EXPECT_EQ(rows[2].users.layers, Strings{"kerbs"});
}

TEST_F(StyleManagerRows, EachLinetypeRowSaysWhatDrawsItAndOneRepeatOfIt)
{
    const std::vector<LinetypeRow> rows = linetypeRows(document);
    ASSERT_EQ(rows.size(), 7U);
    const auto at = [&rows](const char* name, LinetypeOrigin origin) -> const LinetypeRow& {
        return *std::ranges::find_if(rows, [&](const LinetypeRow& candidate) {
            return candidate.name == name && candidate.origin == origin;
        });
    };
    const LinetypeRow& continuous = at("continuous", LinetypeOrigin::Drawing);
    EXPECT_EQ(continuous.kind, LinetypeRowKind::Dash);
    EXPECT_EQ(continuous.period, 0.0);
    // |1| + |-0.5|
    EXPECT_DOUBLE_EQ(at("dash", LinetypeOrigin::Drawing).period, 1.5);
    // |0| + |-0.25|: a dot has no length
    const LinetypeRow& dotted = at("Dotted", LinetypeOrigin::Drawing);
    EXPECT_DOUBLE_EQ(dotted.period, 0.25);
    EXPECT_EQ(dotted.description, "a dot every quarter metre");

    const LinetypeRow& kerb = at("LS Paper Kerb", LinetypeOrigin::Library);
    EXPECT_EQ(kerb.kind, LinetypeRowKind::Paper);
    EXPECT_DOUBLE_EQ(kerb.period, 4.0) << "the file's length, in plot millimetres";
    EXPECT_EQ(kerb.group, "Roads/Kerbs");
    EXPECT_EQ(kerb.sourceFile, "user_linestyl_test.4d");
    EXPECT_EQ(kerb.users.styles, Strings{"Kerb"});
    EXPECT_EQ(kerb.users.entities, 2U);

    // No length: the pen travels from x = 0 to the last move at x = 5.
    const LinetypeRow& fence = at("LS World Fence", LinetypeOrigin::Library);
    EXPECT_EQ(fence.kind, LinetypeRowKind::World);
    EXPECT_DOUBLE_EQ(fence.period, 5.0);

    const LinetypeRow& gate = at("LS Gate", LinetypeOrigin::Library);
    EXPECT_EQ(gate.kind, LinetypeRowKind::TwoPoint);
    EXPECT_EQ(gate.period, 0.0) << "stretched between two points: no repeat";
    EXPECT_FALSE(gate.users.used());
}

TEST_F(StyleManagerRows, DiagnosticsListTheMissingNamesThenTheCollisionsEachWithWhoUsesThem)
{
    const std::vector<StyleDiagnostic> diagnostics = styleDiagnostics(document);
    ASSERT_EQ(diagnostics.size(), 3U);

    EXPECT_EQ(diagnostics[0].kind, StyleDiagnosticKind::MissingLinetype);
    EXPECT_EQ(diagnostics[0].name, "NOPE Line");
    EXPECT_EQ(diagnostics[0].users.styles, Strings{"Ghost"});
    EXPECT_EQ(diagnostics[0].users.entities, 1U);
    EXPECT_EQ(diagnostics[0].drawnAs, "a solid line (continuous)");

    // builtInSymbolFor: "manhole" is in the name, so the manhole shape.
    EXPECT_EQ(diagnostics[1].kind, StyleDiagnosticKind::MissingSymbol);
    EXPECT_EQ(diagnostics[1].name, "NOPE Manhole");
    EXPECT_EQ(diagnostics[1].drawnAs, "the built-in manhole");

    EXPECT_EQ(diagnostics[2].kind, StyleDiagnosticKind::Collision);
    EXPECT_EQ(diagnostics[2].name, "dash");
    EXPECT_EQ(diagnostics[2].users.layers, Strings{"kerbs"});
}

TEST_F(StyleManagerRows, SelectUsersGathersTheEntitiesOfEveryNameAscendingAndOnce)
{
    std::vector<katana::entity::EntityId> expected = kerbPoints;
    expected.push_back(ghostPoint);
    std::ranges::sort(expected);
    EXPECT_EQ(entitiesUsing(document.model(), UsageTable::Style, {"Ghost", "Kerb", "Kerb"}),
              expected);
    EXPECT_EQ(entitiesUsing(document.model(), UsageTable::Linetype, {"LS Paper Kerb"}),
              kerbPoints);
    EXPECT_EQ(entitiesUsing(document.model(), UsageTable::Symbol, {"NOPE Manhole"}),
              std::vector<katana::entity::EntityId>{ghostPoint});
    EXPECT_TRUE(entitiesUsing(document.model(), UsageTable::Style, {"Fence"}).empty());
}

TEST_F(StyleManagerRows, AFreeNameCountsUpFromTwoAndALinetypeNameAvoidsTheLibraryToo)
{
    EXPECT_EQ(freeStyleName(document.model(), "New Style"), "New Style");
    EXPECT_EQ(freeStyleName(document.model(), "Kerb"), "Kerb 2");
    must(katana::commands::createStyle(style("Kerb 2", "continuous", "", 0.25)));
    EXPECT_EQ(freeStyleName(document.model(), "Kerb"), "Kerb 3");

    // "LS Gate" is no drawing linetype, but the library has it: a drawing
    // linetype of that name would start as a collision.
    EXPECT_EQ(freeLinetypeName(document, "LS Gate"), "LS Gate 2");
    EXPECT_EQ(freeLinetypeName(document, "Dotted"), "Dotted 2");
    EXPECT_EQ(freeLinetypeName(document, "Hidden"), "Hidden");
}
