// SymbolLibraryDialog over the committed hand-written customisation fixture
// (tests/archive12d/data/customisation/), loaded through archive12d into the
// Document exactly as a person's would be. Every count and every printed
// size below is worked out by hand from those three files, in the comment
// beside it.
//
// The fixture, as far as these tests need it:
//   test_symbols.4d     TEST Survey Mark (vertex, Test/Marks), TEST Tree (NOT
//                       vertex, Test/Vegetation, one `colour` pen), TEST Valve
//                       (vertex, Test/Marks, one text), TEST U Turn (vertex,
//                       Test/Marks)
//   test_linestyles.4d  TEST Dashed Kerb, TEST Water Main, TEST Gate: none
//                       vertex, and a file not named for symbols - linestyles
//   test_survey.mapfile vertex symbols AC* -> TEST Survey Mark (1.5, white),
//                       TR* -> TEST Tree (3, green), PX* -> "TEST Missing
//                       Symbol", which no library defines

#include <gtest/gtest.h>

#include <algorithm>
#include <filesystem>
#include <fstream>
#include <iterator>
#include <memory>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include <QApplication>
#include <QComboBox>
#include <QDoubleSpinBox>
#include <QItemSelectionModel>
#include <QLabel>
#include <QListView>
#include <QMouseEvent>
#include <QPushButton>
#include <QRegularExpression>
#include <QToolButton>
#include <QTreeWidget>

#include "customisation/customisation_context.hpp"
#include "customisation/definition_thumbnails.hpp"
#include "customisation/filter_bar.hpp"
#include "customisation/name_picker.hpp"
#include "customisation/style_preview.hpp"
#include "customisation/symbol_library.hpp"
#include "katana/archive12d/customisation.hpp"
#include "katana/archive12d/style_library.hpp"
#include "katana/cad/document.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/tables.hpp"
#include "widget_harness.hpp"

using katana::cad::Document;
using katana::entity::EntityId;
using katana::geometry::Point2;
using katana::qt::CustomisationContext;
using katana::qt::DefinitionThumbnails;
using katana::qt::SymbolLibraryDialog;

namespace {

// Found from this file's own place in the tree rather than through a
// compile definition, so the widget target's CMakeLists - shared with the
// other managers' tests - needs no line for it.
std::filesystem::path fixtureDirectory()
{
    return std::filesystem::path(__FILE__).parent_path().parent_path().parent_path() /
           "archive12d" / "data" / "customisation";
}

struct LibraryFixture {
    Document document;
    DefinitionThumbnails thumbnails;
    CustomisationContext context;
    std::vector<std::pair<QString, bool>> logged;

    explicit LibraryFixture(std::vector<std::string> files = {"test_symbols.4d",
                                                              "test_linestyles.4d",
                                                              "test_survey.mapfile"})
    {
        std::vector<std::filesystem::path> paths;
        for (const std::string& file : files) {
            paths.push_back(fixtureDirectory() / file);
        }
        auto loaded = katana::archive12d::readCustomisation(paths);
        EXPECT_TRUE(loaded.ok()) << (loaded.ok() ? "" : loaded.error().describe());
        if (loaded.ok()) {
            document.setStyleLibrary(loaded.value().library);
            document.setSurveyMap(loaded.value().map);
        }
        context.document = &document;
        context.thumbnails = &thumbnails;
        context.log = [this](const QString& message, bool isError) {
            logged.emplace_back(message, isError);
        };
    }

    EntityId point(double x, double y)
    {
        EXPECT_TRUE(document.execute(katana::commands::createPoint(Point2(x, y))).ok());
        return document.lastCreatedEntities().front();
    }

    void select(std::vector<EntityId> ids)
    {
        document.selection().set(std::move(ids));
        document.notifySelectionChanged();
        katana::qt::test::processEvents();
    }

    [[nodiscard]] bool loggedExactly(const QString& message) const
    {
        return std::ranges::any_of(logged, [&](const auto& line) { return line.first == message; });
    }
};

template <typename T> T* child(const QWidget& parent, const char* name)
{
    T* found = parent.findChild<T*>(QString::fromLatin1(name));
    EXPECT_NE(found, nullptr) << name;
    return found;
}

// FilterBar and NamePicker have no Q_OBJECT (no moc here), which findChild
// needs for its type; they are found as widgets and cast.
template <typename T> T* plainChild(const QWidget& parent, const char* name)
{
    T* found = dynamic_cast<T*>(parent.findChild<QWidget*>(QString::fromLatin1(name)));
    EXPECT_NE(found, nullptr) << name;
    return found;
}

QString labelText(const QWidget& parent, const char* name)
{
    const QLabel* label = child<QLabel>(parent, name);
    return label == nullptr ? QString() : label->text();
}

// The tree's items as "label" paths, depth-first, for one comparison.
void collectTree(const QTreeWidgetItem* item, const QString& prefix, QStringList& into)
{
    const QString here = prefix.isEmpty() ? item->text(0) : prefix + " > " + item->text(0);
    into << here;
    for (int index = 0; index < item->childCount(); ++index) {
        collectTree(item->child(index), here, into);
    }
}

QTreeWidgetItem* treeItem(QTreeWidget& tree, const QString& text)
{
    const QList<QTreeWidgetItem*> found =
        tree.findItems(text, Qt::MatchExactly | Qt::MatchRecursive);
    return found.isEmpty() ? nullptr : found.front();
}

TEST(SymbolLibrary, TheGridListsTheFixturesSymbolsIncludingTheNonVertexOneTheMapfileDrawsAsASymbol)
{
    LibraryFixture fixture;
    SymbolLibraryDialog dialog(fixture.context);

    // 4 library symbols (D3: three `mode vertex`, and TEST Tree from the
    // symbol file and drawn by TR*), the 16 built-in shapes (none hidden:
    // the library defines none of their names), and PX*'s undefined name:
    // 21, sorted with case folded. No linestyle from test_linestyles.4d.
    ASSERT_EQ(katana::entity::symbolNames().size(), 16u);
    const std::vector<std::string> expected{
        "arrow",       "circle",           "cross",
        "diamond",     "dot",              "flag",
        "manhole",     "plus",             "pole",
        "ring",        "square",           "star",
        "target",      "TEST Missing Symbol", "TEST Survey Mark",
        "TEST Tree",   "TEST U Turn",      "TEST Valve",
        "tick",        "tree",             "triangle"};
    EXPECT_EQ(dialog.shownNames(), expected);

    const QListView* grid = child<QListView>(dialog, "symbolGrid");
    ASSERT_NE(grid, nullptr);
    EXPECT_EQ(grid->viewMode(), QListView::IconMode);
    EXPECT_EQ(grid->iconSize(), QSize(64, 64));
    EXPECT_EQ(grid->model()->rowCount(), 21);
    EXPECT_EQ(labelText(dialog, "symbolCount"), QStringLiteral("21 of 21 symbols"));

    // The tree: Test/Marks holds Survey Mark, Valve and U Turn (3),
    // Test/Vegetation the tree (1), so Test 4; nothing is ungrouped.
    QTreeWidget* tree = child<QTreeWidget>(dialog, "symbolGroups");
    ASSERT_NE(tree, nullptr);
    QStringList paths;
    for (int index = 0; index < tree->topLevelItemCount(); ++index) {
        collectTree(tree->topLevelItem(index), {}, paths);
    }
    EXPECT_EQ(paths, (QStringList{"All symbols (21)", "Built-in (16)", "Test (4)",
                                  "Test (4) > Marks (3)", "Test (4) > Vegetation (1)",
                                  "Not defined (1)"}));

    // Choosing Vegetation leaves the non-vertex symbol alone, and its
    // details say why it is a symbol at all.
    tree->setCurrentItem(treeItem(*tree, "Vegetation (1)"));
    EXPECT_EQ(dialog.shownNames(), std::vector<std::string>{"TEST Tree"});
    ASSERT_TRUE(dialog.selectSymbol("TEST Tree"));
    EXPECT_EQ(labelText(dialog, "detailMode"),
              QStringLiteral("along a line; a symbol because a survey code draws it as one, "
                             "its file is a symbol file"));
    // move, circle, move, draw and one `colour`.
    EXPECT_EQ(labelText(dialog, "detailContent"),
              QStringLiteral("4 strokes, 0 texts, 1 pen change"));
    EXPECT_EQ(labelText(dialog, "detailCodes"), QStringLiteral("TR* (size 3, green)"));
    EXPECT_EQ(labelText(dialog, "detailGroup"), QStringLiteral("Test/Vegetation"));
    EXPECT_EQ(labelText(dialog, "detailSource"), QStringLiteral("test_symbols.4d"));

    // The search reaches a survey code: only the mark is drawn by AC*.
    tree->setCurrentItem(tree->topLevelItem(0));
    plainChild<katana::qt::FilterBar>(dialog, "symbolFilter")->setText(QStringLiteral("ac*"));
    EXPECT_EQ(dialog.shownNames(), std::vector<std::string>{"TEST Survey Mark"});

    // The Vertex mode chip: the three `mode vertex` definitions.
    plainChild<katana::qt::FilterBar>(dialog, "symbolFilter")->setText({});
    child<QToolButton>(dialog, "filterVertexmode")->click();
    EXPECT_EQ(dialog.shownNames(),
              (std::vector<std::string>{"TEST Survey Mark", "TEST U Turn", "TEST Valve"}));
}

TEST(SymbolLibrary, TheMissingChipListsAStylesUndefinedSymbolWithItsFallback)
{
    LibraryFixture fixture;
    katana::entity::Style old;
    old.name = "Old Pits";
    old.symbol = "OLD Pit Lid";
    ASSERT_TRUE(fixture.document.execute(katana::commands::createStyle(old)).ok());
    SymbolLibraryDialog dialog(fixture.context);

    // Missing: the style's "OLD Pit Lid" and PX*'s "TEST Missing Symbol",
    // folded order ("old..." < "test...").
    QToolButton* missing = child<QToolButton>(dialog, "filterMissing");
    ASSERT_NE(missing, nullptr);
    missing->click();
    EXPECT_EQ(dialog.shownNames(),
              (std::vector<std::string>{"OLD Pit Lid", "TEST Missing Symbol"}));
    EXPECT_EQ(missing->text(), QStringLiteral("Missing (2)"));

    // Its stand-in, by builtInSymbolFor's words: "old pit lid" has "pit",
    // so a manhole; "test missing symbol" has none of them, so a circle.
    const QListView* grid = child<QListView>(dialog, "symbolGrid");
    const QString pitTip = grid->model()->index(0, 0).data(Qt::ToolTipRole).toString();
    EXPECT_TRUE(pitTip.contains(QStringLiteral("Not defined - drawn as the built-in \"manhole\"")))
        << pitTip.toStdString();
    EXPECT_TRUE(
        grid->model()->index(0, 0).data(katana::qt::SymbolGridModel::kMissingRole).toBool());

    ASSERT_TRUE(dialog.selectSymbol("OLD Pit Lid"));
    EXPECT_EQ(labelText(dialog, "detailMissing"),
              QStringLiteral("Not defined - drawn as the built-in \"manhole\" until a library "
                             "defines it"));
    EXPECT_EQ(labelText(dialog, "detailStyles"),
              QStringLiteral("Old Pits - 0 entities in the drawing"));
    ASSERT_TRUE(dialog.selectSymbol("TEST Missing Symbol"));
    EXPECT_TRUE(labelText(dialog, "detailMissing").contains(QStringLiteral("\"circle\"")));
    EXPECT_EQ(labelText(dialog, "detailCodes"), QStringLiteral("PX* (size 1, yellow)"));
}

TEST(SymbolLibrary, AssignToSelectedPointsIsOneUndoStepAndASecondAssignReusesTheStyle)
{
    LibraryFixture fixture;
    const EntityId p1 = fixture.point(0.0, 0.0);
    const EntityId p2 = fixture.point(10.0, 0.0);
    const EntityId p3 = fixture.point(20.0, 0.0);
    ASSERT_TRUE(fixture.document
                    .execute(katana::commands::createLine(Point2(0.0, 5.0), Point2(9.0, 5.0)))
                    .ok());
    const EntityId line = fixture.document.lastCreatedEntities().front();
    SymbolLibraryDialog dialog(fixture.context);
    ASSERT_TRUE(dialog.selectSymbol("TEST Survey Mark"));
    fixture.select({p1, p2, line});

    QPushButton* assign = child<QPushButton>(dialog, "assignToPoints");
    ASSERT_NE(assign, nullptr);
    ASSERT_TRUE(assign->isEnabled());
    const std::size_t steps = fixture.document.history().undoCount();
    assign->click();

    // One step: the style made and the two points moved; the line left.
    EXPECT_EQ(fixture.document.history().undoCount(), steps + 1);
    const auto styleOf = [&](EntityId id) {
        return fixture.document.model().entities.find(id)->style;
    };
    EXPECT_EQ(styleOf(p1), "TEST Survey Mark");
    EXPECT_EQ(styleOf(p2), "TEST Survey Mark");
    EXPECT_EQ(styleOf(line), "");
    EXPECT_TRUE(fixture.loggedExactly(QStringLiteral(
        "2 points now in style \"TEST Survey Mark\" (new), drawing \"TEST Survey Mark\". "
        "1 selected entity is not a point and left as it was.")));

    ASSERT_TRUE(fixture.document.undo().ok());
    EXPECT_FALSE(fixture.document.model().styles.contains("TEST Survey Mark"));
    EXPECT_EQ(styleOf(p1), "");
    ASSERT_TRUE(fixture.document.redo().ok());

    // The second assign, to p3, finds the style and makes none.
    katana::qt::test::processEvents();
    fixture.select({p3});
    const std::size_t styles = fixture.document.model().styles.size();
    const std::size_t before = fixture.document.history().undoCount();
    ASSERT_TRUE(assign->isEnabled());
    assign->click();
    EXPECT_EQ(fixture.document.history().undoCount(), before + 1);
    EXPECT_EQ(fixture.document.model().styles.size(), styles);
    EXPECT_EQ(styleOf(p3), "TEST Survey Mark");
    EXPECT_TRUE(fixture.loggedExactly(QStringLiteral(
        "1 point now in style \"TEST Survey Mark\", drawing \"TEST Survey Mark\".")));

    // And the grid's badge counts the three points drawing it.
    katana::qt::test::processEvents();
    const QListView* grid = child<QListView>(dialog, "symbolGrid");
    const int row = dialog.gridModel().rowOf("TEST Survey Mark");
    ASSERT_GE(row, 0);
    EXPECT_EQ(grid->model()->index(row, 0).data(katana::qt::SymbolGridModel::kUsesRole).toInt(), 3);
}

TEST(SymbolLibrary, TheDetailsPaneStatesTheSurveyMarksPrintSizeWorkedByHandAt1To500)
{
    LibraryFixture fixture;
    SymbolLibraryDialog dialog(fixture.context);
    ASSERT_TRUE(dialog.selectSymbol("TEST Survey Mark"));
    auto* scale = child<QComboBox>(dialog, "plotScale");
    ASSERT_NE(scale, nullptr);
    scale->setCurrentIndex(scale->findData(500));

    // A worldstyle cross of 1 m strokes (-0.5..0.5 each way) with a 0.3 m
    // circle inside: 1 x 1 m on the ground. At 1:500 a plot millimetre is
    // 0.5 m, so it prints 1 / 0.5 = 2 mm each way.
    EXPECT_EQ(labelText(dialog, "detailPrint"),
              QStringLiteral("2 × 2 mm at 1:500, 1 × 1 m on the ground"));
    EXPECT_EQ(labelText(dialog, "detailExtent"), QStringLiteral("1 × 1 m"));
    EXPECT_EQ(labelText(dialog, "detailUnits"), QStringLiteral("worldstyle: metres on the ground"));
    EXPECT_EQ(labelText(dialog, "detailMode"),
              QStringLiteral("vertex: drawn at each vertex of a string"));
    EXPECT_EQ(labelText(dialog, "detailOrigin"), QStringLiteral("(0, 0)"));
    // move, draw, move, draw, move, circle.
    EXPECT_EQ(labelText(dialog, "detailContent"),
              QStringLiteral("6 strokes, 0 texts, 0 pen changes"));
    EXPECT_EQ(labelText(dialog, "detailCodes"), QStringLiteral("AC* (size 1.5, white)"));

    // At AC*'s size, 1.5 m: 1.5 / 0.5 = 3 mm at 1:500.
    auto* size = dialog.findChild<QDoubleSpinBox*>(QStringLiteral("assignSize"));
    ASSERT_NE(size, nullptr);
    size->setValue(1.5);
    EXPECT_EQ(labelText(dialog, "detailPrint"),
              QStringLiteral("3 × 3 mm at 1:500, 1.5 × 1.5 m on the ground"));

    // At its own size and 1:100 (0.1 m per plot millimetre): 10 mm.
    size->setValue(0.0);
    scale->setCurrentIndex(scale->findData(100));
    EXPECT_EQ(labelText(dialog, "detailPrint"),
              QStringLiteral("10 × 10 mm at 1:100, 1 × 1 m on the ground"));
    EXPECT_EQ(dialog.preview()->scaleDenominator(), 100);
}

TEST(SymbolLibrary, TheDetailsPaneCountsASymbolsLettersInWhatItPrints)
{
    LibraryFixture fixture;
    // A symbol that is nothing but a 1 m "H" standing on its insertion point.
    katana::entity::StyleLibrary library = fixture.document.styleLibrary();
    katana::entity::LineStyle letter;
    letter.name = "TEST Letter";
    letter.atVertices = true;
    letter.strokes = {katana::entity::Stroke{.op = katana::entity::StrokeOp::Move},
                      katana::entity::Stroke{.op = katana::entity::StrokeOp::Text, .text = 0}};
    letter.texts = {katana::entity::StrokeText{.text = "H", .height = 1.0}};
    ASSERT_TRUE(library.add(letter).ok());
    fixture.document.setStyleLibrary(std::move(library));

    SymbolLibraryDialog dialog(fixture.context);
    auto* scale = child<QComboBox>(dialog, "plotScale");
    ASSERT_NE(scale, nullptr);
    scale->setCurrentIndex(scale->findData(500));
    ASSERT_TRUE(dialog.selectSymbol("TEST Valve"));

    // TEST Valve: a 0.8 m box (-0.4..0.4 each way), then a 0.5 m "V" standing
    // bottom-centre on (0, 0.6), in the preview's own face. Its width is
    // the box's: a V is narrower than the size it is set at, so at 0.8 wide
    // it is under 0.5 x 0.8 = 0.4 m, within -0.2..0.2. Its top is 0.6 + 0.5
    // x the face's ascent (as a share of that size), and every face's ascent
    // lies between 0.7 and 1.25 of it (Arial 0.905, Segoe UI 1.079), so the
    // valve is 1 + 0.5 x 0.7 = 1.35 to 1 + 0.5 x 1.25 = 1.625 m high: at
    // 1:500 (0.5 m a plot millimetre), 2.7 to 3.25 mm. Its anchor alone made
    // it 1 m, 2 mm.
    static const QRegularExpression valve(QStringLiteral(
        R"(^1\.6 × ([0-9.]+) mm at 1:500, 0\.8 × ([0-9.]+) m on the ground$)"));
    const QString printed = labelText(dialog, "detailPrint");
    const QRegularExpressionMatch match = valve.match(printed);
    ASSERT_TRUE(match.hasMatch()) << printed.toStdString();
    EXPECT_GE(match.captured(1).toDouble(), 2.7) << printed.toStdString();
    EXPECT_LE(match.captured(1).toDouble(), 3.25) << printed.toStdString();
    EXPECT_NEAR(match.captured(2).toDouble(), match.captured(1).toDouble() * 0.5, 0.001);

    // The letter alone, set 1 m high: it covers its face's ascent above the
    // baseline and descent below it, together 1 to 1.4 of the size in every
    // face (Arial 1.117, Segoe UI 1.33), so 1 to 1.4 m high and 2 to 2.8 mm
    // at 1:500 - not "0 x 0".
    ASSERT_TRUE(dialog.selectSymbol("TEST Letter"));
    static const QRegularExpression letterSize(QStringLiteral(
        R"(^([0-9.]+) × ([0-9.]+) mm at 1:500, ([0-9.]+) × ([0-9.]+) m on the ground$)"));
    const QString letterPrinted = labelText(dialog, "detailPrint");
    const QRegularExpressionMatch letterMatch = letterSize.match(letterPrinted);
    ASSERT_TRUE(letterMatch.hasMatch()) << letterPrinted.toStdString();
    EXPECT_GT(letterMatch.captured(1).toDouble(), 0.0) << letterPrinted.toStdString();
    EXPECT_GE(letterMatch.captured(2).toDouble(), 2.0) << letterPrinted.toStdString();
    EXPECT_LE(letterMatch.captured(2).toDouble(), 2.8) << letterPrinted.toStdString();
}

TEST(SymbolLibrary, LoadingMergesIntoTheLibraryAndLogsWhatEachFileAddedAndReplaced)
{
    // The session starts with the linestyles and its own TEST Tree; loading
    // the symbol file adds three and replaces the tree - and keeps the three
    // linestyles, because a load from here never replaces (D1): 3 + 1 + 3.
    LibraryFixture fixture({"test_linestyles.4d"});
    katana::entity::StyleLibrary library = fixture.document.styleLibrary();
    katana::entity::LineStyle ownTree;
    ownTree.name = "TEST Tree";
    ownTree.strokes = {
        katana::entity::Stroke{.op = katana::entity::StrokeOp::Draw, .point = {1.0, 0.0}}};
    ASSERT_TRUE(library.add(ownTree).ok());
    fixture.document.setStyleLibrary(std::move(library));
    SymbolLibraryDialog dialog(fixture.context);

    ASSERT_TRUE(dialog.loadLibraryFile(fixtureDirectory() / "test_symbols.4d"));

    EXPECT_TRUE(fixture.loggedExactly(QStringLiteral(
        "Merged test_symbols.4d into the library: 3 added (TEST Survey Mark, TEST U Turn, "
        "TEST Valve), 1 replaced (TEST Tree)")));
    EXPECT_EQ(fixture.document.styleLibrary().size(), 7u);
    EXPECT_TRUE(fixture.document.styleLibrary().contains("TEST Dashed Kerb"));
    katana::qt::test::processEvents();
    const auto shown = dialog.shownNames();
    EXPECT_NE(std::ranges::find(shown, std::string("TEST Valve")), shown.end());

    // A file that is not there fails, says so, and changes nothing.
    fixture.logged.clear();
    EXPECT_FALSE(dialog.loadLibraryFile(fixtureDirectory() / "no_such_library.4d"));
    ASSERT_EQ(fixture.logged.size(), 1u);
    EXPECT_TRUE(fixture.logged.front().second);
    EXPECT_EQ(fixture.document.styleLibrary().size(), 7u);
}

TEST(SymbolLibrary, AHeadlessSessionOpensNoFileDialogAndAChooserStandsInForOne)
{
    LibraryFixture fixture({"test_linestyles.4d"});
    SymbolLibraryDialog dialog(fixture.context);
    ASSERT_TRUE(dialog.headless());

    child<QPushButton>(dialog, "loadLibrary")->click();
    ASSERT_EQ(fixture.logged.size(), 1u);
    EXPECT_TRUE(fixture.logged.front().second);
    EXPECT_TRUE(fixture.logged.front().first.contains(QStringLiteral("loadLibraryFile")));

    dialog.chooseLoadFile = [] { return fixtureDirectory() / "test_symbols.4d"; };
    child<QPushButton>(dialog, "loadLibrary")->click();
    EXPECT_TRUE(fixture.document.styleLibrary().contains("TEST Survey Mark"));
}

TEST(SymbolLibrary, ExportWritesOnlyTheSelectedLibraryDefinitionsAndNamesTheRest)
{
    LibraryFixture fixture;
    SymbolLibraryDialog dialog(fixture.context);
    QListView* grid = child<QListView>(dialog, "symbolGrid");
    for (const char* name : {"TEST Valve", "cross", "TEST Survey Mark"}) {
        grid->selectionModel()->select(grid->model()->index(dialog.gridModel().rowOf(name), 0),
                                       QItemSelectionModel::Select);
    }
    const std::filesystem::path path =
        std::filesystem::temp_directory_path() / "katana_test_symbol_library_export.4d";
    std::filesystem::remove(path);

    ASSERT_TRUE(dialog.exportSelectedTo(path));

    std::ifstream in(path, std::ios::binary);
    const std::string written{std::istreambuf_iterator<char>(in), std::istreambuf_iterator<char>()};
    in.close();
    std::filesystem::remove(path);
    auto read = katana::archive12d::readStyleLibrary(written);
    ASSERT_TRUE(read.ok());
    EXPECT_EQ(read.value().library.names(),
              (std::vector<std::string>{"TEST Survey Mark", "TEST Valve"}));
    EXPECT_EQ(*read.value().library.find("TEST Valve"),
              [&] {
                  katana::entity::LineStyle valve =
                      *fixture.document.styleLibrary().find("TEST Valve");
                  valve.source.clear(); // a file does not hold its own name
                  return valve;
              }());
    EXPECT_TRUE(fixture.loggedExactly(
        QStringLiteral("Not exported, as no library defines them: cross")));
}

TEST(SymbolLibrary, TheGridsSelectionAndCurrentSymbolSurviveAReloadAndAFilter)
{
    // Two selected for an export, the mark current; then a command reloads
    // the grid and a search narrows it. What is still shown stays selected.
    LibraryFixture fixture;
    SymbolLibraryDialog dialog(fixture.context);
    ASSERT_TRUE(dialog.selectSymbol("TEST Survey Mark"));
    QListView* grid = child<QListView>(dialog, "symbolGrid");
    grid->selectionModel()->select(grid->model()->index(dialog.gridModel().rowOf("TEST Valve"), 0),
                                   QItemSelectionModel::Select);
    ASSERT_EQ(dialog.selectedNames(),
              (std::vector<std::string>{"TEST Survey Mark", "TEST Valve"}));

    fixture.point(0.0, 0.0);
    katana::qt::test::processEvents();
    EXPECT_EQ(dialog.selectedNames(),
              (std::vector<std::string>{"TEST Survey Mark", "TEST Valve"}));
    EXPECT_EQ(dialog.currentSymbol(), "TEST Survey Mark");

    // "survey" keeps only the mark: the valve is hidden, the mark stays.
    plainChild<katana::qt::FilterBar>(dialog, "symbolFilter")->setText(QStringLiteral("survey"));
    EXPECT_EQ(dialog.selectedNames(), std::vector<std::string>{"TEST Survey Mark"});
    EXPECT_EQ(dialog.currentSymbol(), "TEST Survey Mark");
    // And a search that hides it leaves nothing current for an action to reach.
    plainChild<katana::qt::FilterBar>(dialog, "symbolFilter")->setText(QStringLiteral("valve"));
    EXPECT_EQ(dialog.currentSymbol(), "");
    EXPECT_FALSE(child<QDoubleSpinBox>(dialog, "assignSize")->isEnabled());
}

// A Ctrl+click on a grid item, as a person makes one: in ExtendedSelection
// it makes the item current and toggles whether it is selected.
void ctrlClick(QListView& grid, std::string_view name, const SymbolLibraryDialog& dialog)
{
    const QModelIndex index = grid.model()->index(dialog.gridModel().rowOf(name), 0);
    ASSERT_TRUE(index.isValid()) << name;
    const QPointF at = grid.visualRect(index).center();
    const QPointF global = grid.viewport()->mapToGlobal(at);
    QMouseEvent press(QEvent::MouseButtonPress, at, global, Qt::LeftButton, Qt::LeftButton,
                      Qt::ControlModifier);
    QApplication::sendEvent(grid.viewport(), &press);
    QMouseEvent release(QEvent::MouseButtonRelease, at, global, Qt::LeftButton, Qt::NoButton,
                        Qt::ControlModifier);
    QApplication::sendEvent(grid.viewport(), &release);
    katana::qt::test::processEvents();
}

TEST(SymbolLibrary, ASymbolDeselectedWithCtrlStaysDeselectedAcrossAFilterAndAReload)
{
    // The mark, then the valve and the tree added with Ctrl, then the tree
    // taken out again with Ctrl: the tree is left current but not selected.
    LibraryFixture fixture;
    SymbolLibraryDialog dialog(fixture.context);
    dialog.show();
    katana::qt::test::processEvents();
    ASSERT_TRUE(dialog.selectSymbol("TEST Survey Mark"));
    QListView* grid = child<QListView>(dialog, "symbolGrid");
    ASSERT_NE(grid, nullptr);
    ctrlClick(*grid, "TEST Valve", dialog);
    ctrlClick(*grid, "TEST Tree", dialog);
    // Rows in the grid's order: names with case folded.
    ASSERT_EQ(dialog.selectedNames(),
              (std::vector<std::string>{"TEST Survey Mark", "TEST Tree", "TEST Valve"}));
    ctrlClick(*grid, "TEST Tree", dialog);
    const std::vector<std::string> kept{"TEST Survey Mark", "TEST Valve"};
    ASSERT_EQ(dialog.selectedNames(), kept);
    ASSERT_EQ(dialog.currentSymbol(), "TEST Tree");

    // "test" still shows all three: the selection is what the person left,
    // not the tree they took out of it.
    plainChild<katana::qt::FilterBar>(dialog, "symbolFilter")->setText(QStringLiteral("test"));
    EXPECT_EQ(dialog.selectedNames(), kept);
    EXPECT_EQ(dialog.currentSymbol(), "TEST Tree");

    // A command elsewhere reloads the grid: the same again.
    fixture.point(0.0, 0.0);
    katana::qt::test::processEvents();
    EXPECT_EQ(dialog.selectedNames(), kept);
    EXPECT_EQ(dialog.currentSymbol(), "TEST Tree");

    // So an export writes the two kept, and not the tree.
    const std::filesystem::path out =
        std::filesystem::temp_directory_path() / "katana_symbol_library_ctrl_export.4d";
    std::filesystem::remove(out);
    ASSERT_TRUE(dialog.exportSelectedTo(out));
    std::ifstream in(out, std::ios::binary);
    const std::string text{std::istreambuf_iterator<char>(in), std::istreambuf_iterator<char>()};
    in.close();
    std::filesystem::remove(out);
    auto written = katana::archive12d::readStyleLibrary(text);
    ASSERT_TRUE(written.ok());
    EXPECT_EQ(written.value().library.names(), kept);
}

TEST(SymbolLibrary, ReplaceInStylesSwapsAMissingSymbolForALibraryOneInOneUndoStep)
{
    LibraryFixture fixture;
    for (const char* name : {"Old Pits", "Old Pits Large"}) {
        katana::entity::Style old;
        old.name = name;
        old.symbol = "OLD Pit Lid";
        ASSERT_TRUE(fixture.document.execute(katana::commands::createStyle(old)).ok());
    }
    SymbolLibraryDialog dialog(fixture.context);
    ASSERT_TRUE(dialog.selectSymbol("OLD Pit Lid"));
    auto* with = plainChild<katana::qt::NamePicker>(dialog, "replaceWith");
    ASSERT_NE(with, nullptr);
    with->setCurrentName("TEST Survey Mark");
    const std::size_t steps = fixture.document.history().undoCount();

    child<QPushButton>(dialog, "replaceInStyles")->click();

    EXPECT_EQ(fixture.document.history().undoCount(), steps + 1);
    EXPECT_EQ(fixture.document.model().styles.find("Old Pits")->symbol, "TEST Survey Mark");
    EXPECT_EQ(fixture.document.model().styles.find("Old Pits Large")->symbol, "TEST Survey Mark");
    // Only PX*'s name is left missing once the dialog has caught up.
    katana::qt::test::processEvents();
    EXPECT_EQ(child<QToolButton>(dialog, "filterMissing")->text(), QStringLiteral("Missing (1)"));

    ASSERT_TRUE(fixture.document.undo().ok());
    EXPECT_EQ(fixture.document.model().styles.find("Old Pits")->symbol, "OLD Pit Lid");
}

TEST(SymbolLibrary, ReplaceInStylesWaitsForAReplacementAndNeverTakesTheSymbolOff)
{
    LibraryFixture fixture;
    katana::entity::Style marks;
    marks.name = "Marks";
    marks.symbol = "TEST Valve";
    ASSERT_TRUE(fixture.document.execute(katana::commands::createStyle(marks)).ok());
    SymbolLibraryDialog dialog(fixture.context);
    ASSERT_TRUE(dialog.selectSymbol("TEST Valve"));
    auto* with = plainChild<katana::qt::NamePicker>(dialog, "replaceWith");
    auto* replace = child<QPushButton>(dialog, "replaceInStyles");
    ASSERT_NE(with, nullptr);
    ASSERT_NE(replace, nullptr);

    // The picker starts with nothing chosen: nothing to replace it with.
    ASSERT_EQ(with->currentName(), "");
    EXPECT_FALSE(replace->isEnabled());
    const std::size_t steps = fixture.document.history().undoCount();
    EXPECT_FALSE(dialog.replaceInStyles(""));
    EXPECT_EQ(fixture.document.model().styles.find("Marks")->symbol, "TEST Valve");
    EXPECT_EQ(fixture.document.history().undoCount(), steps);
    EXPECT_TRUE(fixture.loggedExactly(
        QStringLiteral("Replace \"TEST Valve\": choose the symbol to put in its place first")));

    // Chosen from the list, or typed: the button can be used.
    with->setCurrentName("TEST Survey Mark");
    EXPECT_TRUE(replace->isEnabled());
    // The symbol itself is no replacement for itself.
    with->setCurrentName("TEST Valve");
    EXPECT_FALSE(replace->isEnabled());
    with->setEditText(QStringLiteral("TEST Tree"));
    EXPECT_EQ(with->currentName(), "TEST Tree");
    EXPECT_TRUE(replace->isEnabled());
    // Cleared again: not usable again.
    with->setCurrentName("");
    EXPECT_FALSE(replace->isEnabled());
}

TEST(SymbolLibrary, SetOnStyleAndSelectPointsUsingActOnTheCurrentSymbol)
{
    LibraryFixture fixture;
    katana::entity::Style marks;
    marks.name = "Marks";
    ASSERT_TRUE(fixture.document.execute(katana::commands::createStyle(marks)).ok());
    const EntityId p1 = fixture.point(0.0, 0.0);
    const EntityId p2 = fixture.point(5.0, 0.0);
    fixture.point(9.0, 0.0);
    ASSERT_TRUE(fixture.document.execute(katana::commands::setEntityStyle({p2, p1}, "Marks")).ok());
    std::vector<EntityId> shown;
    fixture.context.selectAndShow = [&](const std::vector<EntityId>& ids) { shown = ids; };
    SymbolLibraryDialog dialog(fixture.context);
    ASSERT_TRUE(dialog.selectSymbol("TEST Valve"));

    auto* style = child<QComboBox>(dialog, "targetStyle");
    style->setCurrentIndex(style->findText(QStringLiteral("Marks")));
    child<QPushButton>(dialog, "setOnStyle")->click();
    EXPECT_EQ(fixture.document.model().styles.find("Marks")->symbol, "TEST Valve");

    katana::qt::test::processEvents();
    ASSERT_TRUE(dialog.selectSymbol("TEST Valve"));
    QPushButton* selectUsing = child<QPushButton>(dialog, "selectPointsUsing");
    ASSERT_TRUE(selectUsing->isEnabled());
    selectUsing->click();
    EXPECT_EQ(shown, (std::vector<EntityId>{p1, p2}));
}

TEST(SymbolLibrary, TheDialogOutlivesItsDocumentAndDoesNothingAfterIt)
{
    auto fixture = std::make_unique<LibraryFixture>();
    CustomisationContext context = fixture->context;
    DefinitionThumbnails thumbnails;
    context.thumbnails = &thumbnails;
    context.log = {};
    SymbolLibraryDialog dialog(context);
    ASSERT_TRUE(dialog.selectSymbol("TEST Survey Mark"));

    fixture.reset();
    katana::qt::test::processEvents();

    EXPECT_FALSE(dialog.assignToSelectedPoints());
    EXPECT_FALSE(dialog.loadLibraryFile(fixtureDirectory() / "test_symbols.4d"));
    EXPECT_FALSE(dialog.selectPointsUsing());
    dialog.reload();
    katana::qt::test::paint(dialog);
}

} // namespace
