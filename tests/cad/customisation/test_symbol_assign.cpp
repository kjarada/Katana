// The symbol library's logic: putting a symbol on points through a style (one
// undo step, reusing a style that already says exactly that), swapping one
// symbol for another in the styles, who uses a symbol, what the library
// lists and how big a symbol prints. Every expectation is worked out by hand
// in the comment beside it.

#include <gtest/gtest.h>

#include <algorithm>
#include <cmath>
#include <numbers>
#include <string>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/cad/symbol_assign.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/display.hpp"
#include "katana/entity/tables.hpp"

using namespace katana::cad;
using katana::entity::EntityId;
using katana::entity::LineStyle;
using katana::entity::Stroke;
using katana::entity::StrokeOp;
using katana::entity::Style;
using katana::entity::StyleUnits;
using katana::geometry::Box2;
using katana::geometry::Point2;

namespace {

Stroke move(double x, double y) { return Stroke{.op = StrokeOp::Move, .point = {x, y}}; }
Stroke draw(double x, double y) { return Stroke{.op = StrokeOp::Draw, .point = {x, y}}; }

// The fixture's "TEST Survey Mark" (tests/archive12d/data/customisation/
// test_symbols.4d), written out: a cross of two 1 m strokes through the
// origin and a 0.3 m circle round it - so it covers exactly -0.5..0.5 each
// way, the circle lying inside.
LineStyle surveyMark()
{
    LineStyle mark;
    mark.name = "TEST Survey Mark";
    mark.group = "Test/Marks";
    mark.atVertices = true;
    mark.source = "test_symbols.4d";
    mark.strokes = {move(-0.5, 0.0), draw(0.5, 0.0), move(0.0, -0.5), draw(0.0, 0.5),
                    move(0.0, 0.0), Stroke{.op = StrokeOp::Circle, .radius = 0.3}};
    return mark;
}

// A linestyle (not `mode vertex`, from a file not named for symbols, named by
// nothing): D3 does not offer it as a symbol.
LineStyle kerbLinestyle()
{
    LineStyle kerb;
    kerb.name = "TEST Dashed Kerb";
    kerb.units = StyleUnits::Paper;
    kerb.source = "test_linestyles.4d";
    kerb.strokes = {move(0.0, 0.0), draw(1.5, 0.0)};
    return kerb;
}

struct SymbolAssign : ::testing::Test {
    Document document;
    EntityId p1 = 0, p2 = 0, p3 = 0, line = 0;

    void SetUp() override
    {
        katana::entity::StyleLibrary library;
        ASSERT_TRUE(library.add(surveyMark()).ok());
        ASSERT_TRUE(library.add(kerbLinestyle()).ok());
        document.setStyleLibrary(std::move(library));
        p1 = create(katana::commands::createPoint(Point2(0.0, 0.0)));
        p2 = create(katana::commands::createPoint(Point2(10.0, 0.0)));
        p3 = create(katana::commands::createPoint(Point2(20.0, 0.0)));
        line = create(katana::commands::createLine(Point2(0.0, 5.0), Point2(10.0, 5.0)));
    }

    EntityId create(katana::commands::CommandPtr command)
    {
        EXPECT_TRUE(document.execute(std::move(command)).ok());
        const auto created = document.lastCreatedEntities();
        return created.empty() ? 0 : created.front();
    }

    std::string styleOf(EntityId id) const { return document.model().entities.find(id)->style; }

    // Assigns and executes, returning what the assignment said.
    SymbolAssignment assign(std::vector<EntityId> ids, const char* symbol, double size)
    {
        auto assignment = assignSymbolToPoints(document, ids, symbol, size);
        EXPECT_TRUE(assignment.ok());
        if (!assignment.ok()) {
            return {};
        }
        SymbolAssignment result = std::move(assignment).value();
        if (result.command) {
            EXPECT_TRUE(document.execute(std::move(result.command)).ok());
        }
        return result;
    }

    void addStyle(Style style)
    {
        ASSERT_TRUE(document.execute(katana::commands::createStyle(std::move(style))).ok());
    }
};

TEST_F(SymbolAssign, TheFirstAssignMakesAStyleNamedAfterTheSymbolAndMovesOnlyThePoints)
{
    // Selected: two points and a line. The line is counted, not changed; the
    // style is new (the table has none), named after the symbol, and says
    // nothing but the symbol: ByLayer colour, linetype and hatch.
    const SymbolAssignment done = assign({p1, p2, line}, "TEST Survey Mark", 0.0);

    EXPECT_TRUE(done.createsStyle);
    EXPECT_EQ(done.style, "TEST Survey Mark");
    EXPECT_EQ(done.points, 2u);
    EXPECT_EQ(done.notPoints, 1u);
    EXPECT_EQ(done.notFound, 0u);
    EXPECT_EQ(styleOf(p1), "TEST Survey Mark");
    EXPECT_EQ(styleOf(p2), "TEST Survey Mark");
    EXPECT_EQ(styleOf(p3), "");
    EXPECT_EQ(styleOf(line), "");

    const Style* made = document.model().styles.find("TEST Survey Mark");
    ASSERT_NE(made, nullptr);
    EXPECT_EQ(made->symbol, "TEST Survey Mark");
    EXPECT_EQ(made->symbolSize, 0.0);
    EXPECT_FALSE(made->color.has_value());
    EXPECT_TRUE(katana::entity::isByLayer(made->linetype));
    EXPECT_TRUE(made->hatchPattern.empty());
}

TEST_F(SymbolAssign, OneUndoTakesBackBothTheNewStyleAndThePointsMove)
{
    // Four creations in SetUp, then the assignment: five steps, and one undo
    // goes back to four with no style and the points ByLayer again.
    ASSERT_EQ(document.history().undoCount(), 4u);
    assign({p1, p2}, "TEST Survey Mark", 0.0);
    EXPECT_EQ(document.history().undoCount(), 5u);

    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(document.history().undoCount(), 4u);
    EXPECT_FALSE(document.model().styles.contains("TEST Survey Mark"));
    EXPECT_EQ(styleOf(p1), "");
    EXPECT_EQ(styleOf(p2), "");

    ASSERT_TRUE(document.redo().ok());
    EXPECT_TRUE(document.model().styles.contains("TEST Survey Mark"));
    EXPECT_EQ(styleOf(p1), "TEST Survey Mark");
}

TEST_F(SymbolAssign, ASecondAssignOfTheSameSymbolAndSizeReusesTheStyle)
{
    assign({p1}, "TEST Survey Mark", 0.0);
    const std::size_t styles = document.model().styles.size();

    const SymbolAssignment second = assign({p3}, "TEST Survey Mark", 0.0);

    EXPECT_FALSE(second.createsStyle);
    EXPECT_EQ(second.style, "TEST Survey Mark");
    EXPECT_EQ(document.model().styles.size(), styles);
    EXPECT_EQ(styleOf(p3), "TEST Survey Mark");
}

TEST_F(SymbolAssign, AnotherSizeIsAnotherStyleSuffixedTwoAndThenItIsReused)
{
    // 0 (own size) takes the plain name; 1.5 finds that name taken by a
    // style of a different size, so it is "... 2"; a second 1.5 reuses it.
    assign({p1}, "TEST Survey Mark", 0.0);
    const SymbolAssignment bigger = assign({p2}, "TEST Survey Mark", 1.5);
    EXPECT_TRUE(bigger.createsStyle);
    EXPECT_EQ(bigger.style, "TEST Survey Mark 2");
    EXPECT_EQ(document.model().styles.find("TEST Survey Mark 2")->symbolSize, 1.5);

    const SymbolAssignment again = assign({p3}, "TEST Survey Mark", 1.5);
    EXPECT_FALSE(again.createsStyle);
    EXPECT_EQ(again.style, "TEST Survey Mark 2");
}

TEST_F(SymbolAssign, AStyleThatAlsoSetsAColourOrALinetypeIsNotReused)
{
    // "TEST Survey Mark" red, and "TEST Survey Mark 2" with the Style
    // default linetype "continuous" (not ByLayer): both draw the symbol at
    // size 0, neither says ONLY that. The first free name is "... 3".
    Style red;
    red.name = "TEST Survey Mark";
    red.symbol = "TEST Survey Mark";
    red.linetype = std::string(katana::entity::kByLayerLinetype);
    red.color = katana::entity::Color{255, 0, 0};
    addStyle(red);
    Style continuous;
    continuous.name = "TEST Survey Mark 2";
    continuous.symbol = "TEST Survey Mark";
    addStyle(continuous);

    const SymbolAssignment done = assign({p1}, "TEST Survey Mark", 0.0);

    EXPECT_TRUE(done.createsStyle);
    EXPECT_EQ(done.style, "TEST Survey Mark 3");
}

TEST_F(SymbolAssign, AMatchingStyleUnderAnotherNameIsReusedAndItsDescriptionIsNoBar)
{
    Style marks;
    marks.name = "Marks";
    marks.symbol = "TEST Survey Mark";
    marks.linetype = "BYLAYER"; // ByLayer in another spelling is still ByLayer
    marks.description = "control marks";
    addStyle(marks);

    const SymbolAssignment done = assign({p1}, "TEST Survey Mark", 0.0);

    EXPECT_FALSE(done.createsStyle);
    EXPECT_EQ(done.style, "Marks");
    EXPECT_FALSE(document.model().styles.contains("TEST Survey Mark"));
}

TEST_F(SymbolAssign, SymbolNamesAreCaseSensitiveWhenAStyleIsReused)
{
    // D3: "test survey mark" is another name, so its style is not reused.
    Style lower;
    lower.name = "lower";
    lower.symbol = "test survey mark";
    lower.linetype = std::string(katana::entity::kByLayerLinetype);
    addStyle(lower);

    EXPECT_EQ(findSymbolStyle(document.model(), "TEST Survey Mark", 0.0), nullptr);
    EXPECT_EQ(findSymbolStyle(document.model(), "test survey mark", 0.0),
              document.model().styles.find("lower"));
}

TEST_F(SymbolAssign, PointsAlreadyInTheStyleMakeNoUndoStep)
{
    assign({p1}, "TEST Survey Mark", 0.0);
    const std::size_t steps = document.history().undoCount();

    auto again = assignSymbolToPoints(document, {p1}, "TEST Survey Mark", 0.0);
    ASSERT_TRUE(again.ok());
    EXPECT_EQ(again.value().command, nullptr);
    EXPECT_EQ(again.value().alreadyInStyle, 1u);
    EXPECT_EQ(again.value().points, 0u);
    EXPECT_EQ(document.history().undoCount(), steps);
}

TEST_F(SymbolAssign, NoPointAmongTheIdsAnEmptyNameOrABadSizeIsRefusedAndSaysWhy)
{
    const auto onlyLine = assignSymbolToPoints(document, {line, 9999}, "TEST Survey Mark", 0.0);
    ASSERT_FALSE(onlyLine.ok());
    EXPECT_EQ(onlyLine.error().code, katana::core::ErrorCode::InvalidArgument);
    // One line and one id nothing has.
    EXPECT_EQ(onlyLine.error().context,
              "1 entity that is not a point, 1 id no entity has");

    EXPECT_FALSE(assignSymbolToPoints(document, {p1}, "", 0.0).ok());
    EXPECT_FALSE(assignSymbolToPoints(document, {p1}, "TEST Survey Mark", -1.0).ok());
    EXPECT_FALSE(assignSymbolToPoints(document, {p1}, "TEST Survey Mark", std::nan("")).ok());
    EXPECT_FALSE(assignSymbolToPoints(document, {}, "TEST Survey Mark", 0.0).ok());
}

TEST_F(SymbolAssign, APointOnALockedLayerRefusesTheWholeStepAndLeavesNoStyle)
{
    katana::entity::Layer locked;
    locked.name = "locked";
    ASSERT_TRUE(document.execute(katana::commands::createLayer(locked)).ok());
    const EntityId held = create(katana::commands::createPoint(
        Point2(30.0, 0.0), katana::commands::EntityAttributes{.layer = "locked"}));
    locked.locked = true;
    ASSERT_TRUE(document.execute(katana::commands::updateLayer(locked)).ok());
    const std::size_t steps = document.history().undoCount();

    auto assignment = assignSymbolToPoints(document, {p1, held}, "TEST Survey Mark", 0.0);
    ASSERT_TRUE(assignment.ok());
    ASSERT_TRUE(assignment.value().createsStyle);

    EXPECT_FALSE(document.execute(std::move(assignment.value().command)).ok());
    // The Transaction undid the style it had made before the move failed.
    EXPECT_FALSE(document.model().styles.contains("TEST Survey Mark"));
    EXPECT_EQ(styleOf(p1), "");
    EXPECT_EQ(document.history().undoCount(), steps);
}

TEST_F(SymbolAssign, ReplacingASymbolChangesEveryStyleNamingItInOneUndoStep)
{
    // A and B name the mark, C names another: two styles change, C does not.
    for (const auto& [name, symbol] : {std::pair{"A", "TEST Survey Mark"},
                                       std::pair{"B", "TEST Survey Mark"},
                                       std::pair{"C", "cross"}}) {
        Style style;
        style.name = name;
        style.symbol = symbol;
        style.symbolSize = 2.0;
        addStyle(style);
    }
    const std::size_t steps = document.history().undoCount();

    auto replace = replaceSymbolInStyles(document.model(), "TEST Survey Mark", "circle");
    ASSERT_NE(replace, nullptr);
    ASSERT_TRUE(document.execute(std::move(replace)).ok());
    EXPECT_EQ(document.history().undoCount(), steps + 1);
    EXPECT_EQ(document.model().styles.find("A")->symbol, "circle");
    EXPECT_EQ(document.model().styles.find("B")->symbol, "circle");
    EXPECT_EQ(document.model().styles.find("C")->symbol, "cross");
    // Only the symbol: the size each style gave is kept.
    EXPECT_EQ(document.model().styles.find("A")->symbolSize, 2.0);

    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(document.model().styles.find("A")->symbol, "TEST Survey Mark");
    EXPECT_EQ(document.model().styles.find("B")->symbol, "TEST Survey Mark");
}

TEST_F(SymbolAssign, ReplacingANameNoStyleUsesGivesNoCommand)
{
    Style style;
    style.name = "A";
    style.symbol = "cross";
    addStyle(style);

    EXPECT_EQ(replaceSymbolInStyles(document.model(), "TEST Survey Mark", "circle"), nullptr);
    EXPECT_EQ(replaceSymbolInStyles(document.model(), "cross", "cross"), nullptr);
    // "" is "no symbol", not a name: styles without one are not rewritten.
    EXPECT_EQ(replaceSymbolInStyles(document.model(), "", "cross"), nullptr);
}

TEST_F(SymbolAssign, TheUsersOfASymbolAreItsStylesAndThePointsWearingThemLinesCountedApart)
{
    // Style "Marks" draws the mark; p1, p3 and the line wear it; p2 wears
    // nothing. Points ascending: p1 then p3; the line is the one other.
    Style marks;
    marks.name = "Marks";
    marks.symbol = "TEST Survey Mark";
    addStyle(marks);
    ASSERT_TRUE(
        document.execute(katana::commands::setEntityStyle({p3, p1, line}, "Marks")).ok());

    const SymbolUsers users = symbolUsers(document, "TEST Survey Mark");

    EXPECT_EQ(users.styles, std::vector<std::string>{"Marks"});
    EXPECT_EQ(users.points, (std::vector<EntityId>{p1, p3}));
    EXPECT_EQ(users.otherEntities, 1u);
    EXPECT_EQ(symbolUsers(document, "cross"), SymbolUsers{});
    EXPECT_EQ(symbolUsers(document, ""), SymbolUsers{});
}

TEST_F(SymbolAssign, TheCodesNamingASymbolAreItsVertexSymbolRulesWithSizeAndColour)
{
    // Two rules draw the mark (AC* at 1.5 white, SM* at its own size in the
    // string's colour); a map_data rule with that linestyle is not a symbol
    // rule and is not listed.
    katana::entity::SurveyMap map;
    katana::entity::SurveyRule ac;
    ac.key = "AC*";
    ac.section = katana::entity::SurveySection::VertexSymbol;
    ac.symbol = katana::entity::SurveySymbol{.style = "TEST Survey Mark", .colour = "white",
                                             .size = 1.5};
    ASSERT_TRUE(map.add(ac).ok());
    katana::entity::SurveyRule linework;
    linework.key = "LN*";
    linework.linestyle = "TEST Survey Mark";
    ASSERT_TRUE(map.add(linework).ok());
    katana::entity::SurveyRule sm;
    sm.key = "SM*";
    sm.section = katana::entity::SurveySection::VertexSymbol;
    sm.symbol = katana::entity::SurveySymbol{.style = "TEST Survey Mark"};
    ASSERT_TRUE(map.add(sm).ok());

    const std::vector<SymbolCode> codes = symbolCodes(map, "TEST Survey Mark");

    ASSERT_EQ(codes.size(), 2u);
    EXPECT_EQ(codes[0], (SymbolCode{"AC*", 1.5, "white", ""}));
    EXPECT_EQ(codes[1], (SymbolCode{"SM*", 0.0, "", ""}));
    EXPECT_TRUE(symbolCodes(map, "test survey mark").empty());
}

TEST_F(SymbolAssign, TheLibraryListsD3sSymbolsTheBuiltInsAndEveryUndefinedNameWithItsFallback)
{
    // A style names "Old Pit" (defined nowhere) and a survey code names
    // "Old Valve" (defined nowhere, worn by no style). Both are listed,
    // marked missing, each with builtInSymbolFor's shape; the kerb linestyle
    // is not a symbol under D3 and is not listed.
    Style old;
    old.name = "Old";
    old.symbol = "Old Pit";
    addStyle(old);
    katana::entity::SurveyMap map;
    katana::entity::SurveyRule valve;
    valve.key = "VV*";
    valve.section = katana::entity::SurveySection::VertexSymbol;
    valve.symbol = katana::entity::SurveySymbol{.style = "Old Valve", .size = 0.8};
    ASSERT_TRUE(map.add(valve).ok());
    document.setSurveyMap(std::move(map));

    const std::vector<SymbolLibraryEntry> entries = symbolLibrary(document);
    const auto find = [&](const char* name) -> const SymbolLibraryEntry* {
        const auto found = std::ranges::find_if(
            entries, [&](const SymbolLibraryEntry& entry) { return entry.entry.name == name; });
        return found == entries.end() ? nullptr : &*found;
    };

    ASSERT_NE(find("TEST Survey Mark"), nullptr);
    EXPECT_EQ(find("TEST Survey Mark")->entry.source, DefinitionSource::Library);
    EXPECT_EQ(find("TEST Dashed Kerb"), nullptr);
    ASSERT_NE(find("cross"), nullptr);
    EXPECT_EQ(find("cross")->entry.source, DefinitionSource::BuiltIn);

    const SymbolLibraryEntry* pit = find("Old Pit");
    ASSERT_NE(pit, nullptr);
    EXPECT_TRUE(pit->entry.missing);
    EXPECT_EQ(pit->fallback, katana::entity::builtInSymbolFor("Old Pit"));
    EXPECT_EQ(pit->entry.users.styles, std::vector<std::string>{"Old"});

    const SymbolLibraryEntry* valveEntry = find("Old Valve");
    ASSERT_NE(valveEntry, nullptr);
    EXPECT_TRUE(valveEntry->entry.missing);
    EXPECT_EQ(valveEntry->fallback, katana::entity::builtInSymbolFor("Old Valve"));
    ASSERT_EQ(valveEntry->codes.size(), 1u);
    EXPECT_EQ(valveEntry->codes[0].key, "VV*");
    EXPECT_EQ(valveEntry->codes[0].size, 0.8);

    // Sorted with case folded: every neighbour pair is in order.
    for (std::size_t i = 1; i < entries.size(); ++i) {
        EXPECT_LE(katana::core::lowered(entries[i - 1].entry.name),
                  katana::core::lowered(entries[i].entry.name))
            << "at " << i;
    }
}

TEST(SymbolPrintSize, AWorldSymbolPrintsItsGroundSizeDividedByTheScale)
{
    // The mark covers 1 m x 1 m on the ground. At 1:500 a plot millimetre is
    // 0.5 m, so it prints 1 / 0.5 = 2 mm each way; its insertion point (the
    // crossing) is inside it.
    const auto printed = symbolPrintSize(surveyMark(), 0.0, 500.0);
    ASSERT_TRUE(printed.has_value());
    EXPECT_DOUBLE_EQ(printed->groundWidth, 1.0);
    EXPECT_DOUBLE_EQ(printed->groundHeight, 1.0);
    EXPECT_DOUBLE_EQ(printed->paperWidth, 2.0);
    EXPECT_DOUBLE_EQ(printed->paperHeight, 2.0);
    EXPECT_TRUE(printed->insertionInside);
}

TEST(SymbolPrintSize, ASizeMakesTheLargerSideSpanIt)
{
    // Size 1.5 m over a 1 m definition: 1.5 m each way, 3 mm at 1:500 and
    // 15 mm at 1:100.
    const auto at500 = symbolPrintSize(surveyMark(), 1.5, 500.0);
    ASSERT_TRUE(at500.has_value());
    EXPECT_DOUBLE_EQ(at500->groundWidth, 1.5);
    EXPECT_DOUBLE_EQ(at500->paperWidth, 3.0);
    const auto at100 = symbolPrintSize(surveyMark(), 1.5, 100.0);
    ASSERT_TRUE(at100.has_value());
    EXPECT_DOUBLE_EQ(at100->paperWidth, 15.0);
}

TEST(SymbolPrintSize, APaperSymbolPrintsItsOwnMillimetresAtEveryScale)
{
    // A paperstyle 2 mm wide and 1 mm high: 2 x 1 mm on the plot at any
    // scale; on the ground 2 x 0.25 = 0.5 m by 0.25 m at 1:250, and 2 x 1 =
    // 2 m by 1 m at 1:1000.
    LineStyle tick;
    tick.name = "tick";
    tick.units = StyleUnits::Paper;
    tick.strokes = {move(0.0, 0.0), draw(2.0, 0.0), draw(2.0, 1.0)};

    const auto at250 = symbolPrintSize(tick, 0.0, 250.0);
    ASSERT_TRUE(at250.has_value());
    EXPECT_DOUBLE_EQ(at250->paperWidth, 2.0);
    EXPECT_DOUBLE_EQ(at250->paperHeight, 1.0);
    EXPECT_DOUBLE_EQ(at250->groundWidth, 0.5);
    EXPECT_DOUBLE_EQ(at250->groundHeight, 0.25);
    const auto at1000 = symbolPrintSize(tick, 0.0, 1000.0);
    ASSERT_TRUE(at1000.has_value());
    EXPECT_DOUBLE_EQ(at1000->paperWidth, 2.0);
    EXPECT_DOUBLE_EQ(at1000->groundWidth, 2.0);
}

TEST(SymbolPrintSize, ASymbolDrawnAwayFromItsInsertionPointSaysSo)
{
    // Strokes from (5, 5) to (6, 6): the point it is put on, (0, 0), is
    // outside the 1 x 1 box it draws.
    LineStyle away;
    away.name = "away";
    away.strokes = {move(5.0, 5.0), draw(6.0, 5.0), draw(6.0, 6.0)};

    const auto printed = symbolPrintSize(away, 0.0, 500.0);
    ASSERT_TRUE(printed.has_value());
    EXPECT_DOUBLE_EQ(printed->groundWidth, 1.0);
    EXPECT_FALSE(printed->insertionInside);
}

// The fixture's "TEST Valve" written out: a 0.8 m box about the insertion
// point, then `move 0 0.6` and a 0.5 m "V", bottom-centre, 0.8 wide factor.
LineStyle valve()
{
    LineStyle style;
    style.name = "TEST Valve";
    style.atVertices = true;
    style.strokes = {move(-0.4, -0.4), draw(0.4, -0.4), draw(0.4, 0.4), draw(-0.4, 0.4),
                     draw(-0.4, -0.4), move(0.0, 0.6),
                     Stroke{.op = StrokeOp::Text, .text = 0}};
    style.texts = {katana::entity::StrokeText{.text = "V",
                                              .height = 0.5,
                                              .justify = "bottom-centre",
                                              .font = "Arial",
                                              .widthFactor = 0.8}};
    return style;
}

// `mode vertex; move 0 0; text "H" 0 1`: a symbol that is only a letter,
// 1 m high, with no justification.
LineStyle letterOnly()
{
    LineStyle style;
    style.name = "letter";
    style.atVertices = true;
    style.strokes = {move(0.0, 0.0), Stroke{.op = StrokeOp::Text, .text = 0}};
    style.texts = {katana::entity::StrokeText{.text = "H", .height = 1.0}};
    return style;
}

TEST(SymbolPrintSize, AValvesLetterAboveItsBoxIsPartOfWhatPrints)
{
    // The box spans -0.4..0.4 each way. The V stands on (0, 0.6), 0.5 m
    // high, so it reaches y = 1.1; it is 0.6 x 0.5 x 0.8 = 0.24 m wide,
    // centred on x = 0 (-0.12..0.12), inside the box's width. So the valve
    // covers 0.8 m by -0.4..1.1 = 1.5 m, which at 1:500 (0.5 m a plot
    // millimetre) prints 1.6 x 3 mm. Measuring the V's anchor alone gave
    // 0.8 x 1 m, 1.6 x 2 mm.
    const auto printed = symbolPrintSize(valve(), 0.0, 500.0);
    ASSERT_TRUE(printed.has_value());
    EXPECT_DOUBLE_EQ(printed->groundWidth, 0.8);
    EXPECT_DOUBLE_EQ(printed->groundHeight, 1.5);
    EXPECT_DOUBLE_EQ(printed->paperWidth, 1.6);
    EXPECT_DOUBLE_EQ(printed->paperHeight, 3.0);
    EXPECT_TRUE(printed->insertionInside);
}

TEST(SymbolPrintSize, ASymbolThatIsOnlyALetterPrintsTheLettersSize)
{
    // "H" stands on (0, 0) from its left: 0.6 x 1 x 1 = 0.6 m wide and 1 m
    // high, so 0.6 / 0.5 = 1.2 by 1 / 0.5 = 2 mm at 1:500 - not 0 x 0.
    const auto own = symbolPrintSize(letterOnly(), 0.0, 500.0);
    ASSERT_TRUE(own.has_value());
    EXPECT_DOUBLE_EQ(own->groundWidth, 0.6);
    EXPECT_DOUBLE_EQ(own->groundHeight, 1.0);
    EXPECT_DOUBLE_EQ(own->paperWidth, 1.2);
    EXPECT_DOUBLE_EQ(own->paperHeight, 2.0);
    // A size is a width of LineStyle::bounds(), which for a lone text is its
    // anchor: no width, so symbolDrawing keeps the definition's own scale
    // rather than dividing by zero - and it prints the same at size 2.
    const auto sized = symbolPrintSize(letterOnly(), 2.0, 500.0);
    ASSERT_TRUE(sized.has_value());
    EXPECT_DOUBLE_EQ(sized->paperWidth, 1.2);
    EXPECT_DOUBLE_EQ(sized->paperHeight, 2.0);
}

TEST(SymbolPrintSize, AnEstimatedTextFollowsItsJustificationWidthFactorAndAngle)
{
    StyleTextMark mark;
    mark.at = Point2(10.0, 20.0);
    mark.text = "12";
    mark.height = 0.5;

    // Two characters 0.6 x 0.5 = 0.3 m each: 0.6 m, ending at the anchor
    // ("right"), and centred on it upright ("middle"): x 9.4..10, y
    // 19.75..20.25.
    mark.justify = "middle-right";
    Box2 box = estimatedTextExtent(mark);
    EXPECT_DOUBLE_EQ(box.min.x, 9.4);
    EXPECT_DOUBLE_EQ(box.max.x, 10.0);
    EXPECT_DOUBLE_EQ(box.min.y, 19.75);
    EXPECT_DOUBLE_EQ(box.max.y, 20.25);

    // Half as wide (0.3 m), centred ("centre"), hanging from it ("top"):
    // x 9.85..10.15, y 19.5..20.
    mark.justify = "top-centre";
    mark.widthFactor = 0.5;
    box = estimatedTextExtent(mark);
    EXPECT_DOUBLE_EQ(box.min.x, 9.85);
    EXPECT_DOUBLE_EQ(box.max.x, 10.15);
    EXPECT_DOUBLE_EQ(box.min.y, 19.5);
    EXPECT_DOUBLE_EQ(box.max.y, 20.0);

    // Unjustified and turned a quarter anticlockwise: its 0.3 m run goes
    // up from the anchor (y 20..20.3) and its 0.5 m height leftwards (x
    // 9.5..10).
    mark.justify.clear();
    mark.angle = std::numbers::pi / 2.0;
    box = estimatedTextExtent(mark);
    EXPECT_NEAR(box.min.x, 9.5, 1e-12);
    EXPECT_NEAR(box.max.x, 10.0, 1e-12);
    EXPECT_NEAR(box.min.y, 20.0, 1e-12);
    EXPECT_NEAR(box.max.y, 20.3, 1e-12);

    // Nothing to print: no box, not a point at the anchor.
    mark.text.clear();
    EXPECT_TRUE(estimatedTextExtent(mark).empty());
    mark.text = "12";
    mark.height = 0.0;
    EXPECT_TRUE(estimatedTextExtent(mark).empty());
}

TEST(SymbolPrintSize, ATextExtentGivenByTheCallerReplacesTheEstimate)
{
    // A font that sets the V 0.2 m wide and 0.6 m tall above (0, 0.6) - a
    // stand-in for a measured face - makes the valve -0.4..1.2: 1.6 m, or
    // 3.2 mm at 1:500.
    const TextExtent measured = [](const StyleTextMark& text) {
        return Box2(Point2(text.at.x - 0.1, text.at.y), Point2(text.at.x + 0.1, text.at.y + 0.6));
    };
    const auto printed = symbolPrintSize(valve(), 0.0, 500.0, measured);
    ASSERT_TRUE(printed.has_value());
    EXPECT_DOUBLE_EQ(printed->groundHeight, 1.6);
    EXPECT_DOUBLE_EQ(printed->paperHeight, 3.2);
    EXPECT_DOUBLE_EQ(printed->paperWidth, 1.6);
}

TEST(SymbolPrintSize, NothingToMeasureOrNoScaleGivesNoSize)
{
    LineStyle empty;
    empty.name = "empty";
    EXPECT_FALSE(symbolPrintSize(empty, 0.0, 500.0).has_value());
    EXPECT_FALSE(symbolPrintSize(surveyMark(), 0.0, 0.0).has_value());
    EXPECT_FALSE(symbolPrintSize(surveyMark(), -1.0, 500.0).has_value());
}

} // namespace
