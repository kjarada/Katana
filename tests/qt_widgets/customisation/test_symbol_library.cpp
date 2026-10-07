// SymbolLibraryDialog over the committed hand-written customisation fixture
// (tests/data/customisation, Katana customisation files), loaded into the
// Document as a person's would be (fixture_customisation.hpp). Every count
// and every printed size below is worked out by hand from those three files,
// in the comment beside it.
//
// The fixture, as far as these tests need it:
//   test_symbols     listed as SYMBOLS: TEST Survey Mark (at vertices,
//                    Test/Marks), TEST Tree (NOT at vertices,
//                    Test/Vegetation, one pen), TEST Valve (at vertices,
//                    Test/Marks, one text), TEST U Turn (at vertices,
//                    Test/Marks)
//   test_linestyles  listed as LINESTYLES: TEST Dashed Kerb, TEST Water
//                    Main, TEST Gate, none at vertices
//   test_survey      symbol rules AC* -> TEST Survey Mark (1.5, white),
//                    TR* -> TEST Tree (3, green), PX* -> "TEST Missing
//                    Symbol", which no library defines
//
// These were three files of an older format until the library read and
// wrote Katana customisation files; the definitions, rules and counts are
// the same. Two things a test reads off a definition changed with the
// format, by its rules (docs/customisation.md, "A definition") and not by a
// run: its source is its customisation's NAME ("test_symbols"), and what
// makes TEST Tree a symbol is the list it sits in.
//
// A definition drawn at each vertex is said to be "at vertices", the Katana
// customisation format's `atVertices` in words, where the window used to
// say "vertex" after the style library file's `mode vertex`: detailMode's
// expectation was changed from the table "The words a rule is shown by" in
// docs/customisation.md, not from what a run printed. The chip is still
// found as "filterVertexmode": an objectName outlives its label.

#include <gtest/gtest.h>

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <filesystem>
#include <fstream>
#include <functional>
#include <iterator>
#include <memory>
#include <string>
#include <string_view>
#include <system_error>
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

#include <QImage>
#include <QPainter>
#include <QPen>

#include <cstdlib>
#include <optional>

#include "customisation/customisation_context.hpp"
#include "customisation/definition_thumbnails.hpp"
#include "customisation/filter_bar.hpp"
#include "customisation/fixture_customisation.hpp"
#include "customisation/name_picker.hpp"
#include "customisation/style_painter.hpp"
#include "customisation/style_preview.hpp"
#include "customisation/symbol_library.hpp"
#include "katana/cad/document.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/colour_names.hpp"
#include "katana/entity/customisation.hpp"
#include "katana/entity/tables.hpp"
#include "katana/storage/project_store.hpp"
#include "plan_painter.hpp"
#include "plotting/legend_painter.hpp"
#include "sheet_painter.hpp"
#include "widget_harness.hpp"

using katana::cad::Document;
using katana::entity::EntityId;
using katana::geometry::Point2;
using katana::qt::CustomisationContext;
using katana::qt::DefinitionThumbnails;
using katana::qt::SymbolLibraryDialog;
using katana::qt::test::customisationFixtureDirectory;
using katana::qt::test::customisationFixtureFile;

namespace {

struct LibraryFixture {
    Document document;
    DefinitionThumbnails thumbnails;
    // The window's executor, over the same Document: the dialog hands its
    // import and export lines to it (CustomisationContext::run).
    katana::qt::test::InterpreterRunner executor{document};
    CustomisationContext context;
    std::vector<std::pair<QString, bool>> logged;

    // The fixtures by name, in the order they are loaded: the session is
    // named after the first, "test_symbols" unless a test says otherwise.
    explicit LibraryFixture(const std::vector<std::string>& fixtures = {"test_symbols",
                                                                        "test_linestyles",
                                                                        "test_survey"})
    {
        katana::qt::test::installCustomisationFixtures(document, fixtures);
        context.document = &document;
        context.thumbnails = &thumbnails;
        context.run = executor.runner();
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

std::string readBytes(const std::filesystem::path& path)
{
    std::ifstream in(path, std::ios::binary);
    return {std::istreambuf_iterator<char>(in), std::istreambuf_iterator<char>()};
}

// A scratch file of this suite's own, gone before it is written.
std::filesystem::path scratchFile(const std::string& name)
{
    const std::filesystem::path path =
        std::filesystem::temp_directory_path() / ("katana_test_symbol_library_" + name);
    std::filesystem::remove(path);
    return path;
}

// Ink of the colour 128 0 128, or of that colour faded into a dark ground as
// an antialiased edge is: red and blue there and near equal, green under a
// quarter of them. An entity's own pen is a grey or a white - red, green and
// blue equal - so none of its pixels is one of these, on any ground.
bool holdsPurple(const QImage& image)
{
    for (int y = 0; y < image.height(); ++y) {
        for (int x = 0; x < image.width(); ++x) {
            const QColor pixel = image.pixelColor(x, y);
            if (pixel.red() >= 60 && 4 * pixel.green() <= pixel.red() &&
                std::abs(pixel.red() - pixel.blue()) <= 12) {
                return true;
            }
        }
    }
    return false;
}

// On PAPER, which is white: how many pixels of `area` are ink of the colour
// 128 0 128, whole or thinned onto the paper as an antialiased edge is. Such
// a pixel falls short of white by a share s of what the colour itself falls
// short by - 127, 255 and 127 - so its green is short by 255 s and its red
// and blue by 127 s each; a quarter covered or more is counted, and each
// channel may be 30 out. Black ink, and any grey, falls short by the SAME
// amount in all three: at a share of a quarter that is already 32 from what
// purple would give its red, so the entity's own pen, the legend's lettering
// and the grey beyond the paper's edge are none of them counted.
int purpleOnPaper(const QImage& image, const QRect& area)
{
    int count = 0;
    const QRect within = area.intersected(image.rect());
    for (int y = within.top(); y <= within.bottom(); ++y) {
        for (int x = within.left(); x <= within.right(); ++x) {
            const QColor pixel = image.pixelColor(x, y);
            const double green = 255.0 - pixel.green();
            const double share = green / 255.0;
            if (share >= 0.25 && std::abs((255.0 - pixel.red()) - 127.0 * share) <= 30.0 &&
                std::abs((255.0 - pixel.blue()) - 127.0 * share) <= 30.0) {
                ++count;
            }
        }
    }
    return count;
}

// How many pixels of `area` are not the paper's white.
int inkOnPaper(const QImage& image, const QRect& area)
{
    int count = 0;
    const QRect within = area.intersected(image.rect());
    for (int y = within.top(); y <= within.bottom(); ++y) {
        for (int x = within.left(); x <= within.right(); ++x) {
            count += image.pixel(x, y) != qRgb(255, 255, 255) ? 1 : 0;
        }
    }
    return count;
}

// A ring 1 m across whose one pen is a colour only a customisation defines.
katana::entity::LineStyle purpleRing()
{
    katana::entity::LineStyle ring;
    ring.name = "TEST Purple Ring";
    ring.atVertices = true;
    ring.symbol = true;
    ring.strokes = {
        katana::entity::Stroke{.op = katana::entity::StrokeOp::Pen, .pen = "sui test purple"},
        katana::entity::Stroke{.op = katana::entity::StrokeOp::Move, .point = {0.0, 0.0}},
        katana::entity::Stroke{.op = katana::entity::StrokeOp::Circle, .radius = 0.5}};
    return ring;
}

TEST(SymbolLibrary, TheGridListsTheFixturesSymbolsAndTheOneNotAtVerticesThatACodeDrawsAsOne)
{
    LibraryFixture fixture;
    SymbolLibraryDialog dialog(fixture.context);

    // 4 library symbols (D3: three at vertices, and TEST Tree, which its
    // customisation lists as a symbol and TR* draws as one), the 16 built-in
    // shapes (none hidden: the library defines none of their names), and
    // PX*'s undefined name: 21, sorted with case folded. No linestyle of
    // test_linestyles.
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

    // Choosing Vegetation leaves the symbol that is not at vertices alone,
    // and its details say why it is a symbol at all.
    tree->setCurrentItem(treeItem(*tree, "Vegetation (1)"));
    EXPECT_EQ(dialog.shownNames(), std::vector<std::string>{"TEST Tree"});
    ASSERT_TRUE(dialog.selectSymbol("TEST Tree"));
    EXPECT_EQ(labelText(dialog, "detailMode"),
              QStringLiteral("along a line; a symbol because a survey code draws it as one, "
                             "its customisation lists it as a symbol"));
    // move, circle, move, draw and one pen.
    EXPECT_EQ(labelText(dialog, "detailContent"),
              QStringLiteral("4 strokes, 0 texts, 1 pen change"));
    EXPECT_EQ(labelText(dialog, "detailCodes"), QStringLiteral("TR* (size 3, green)"));
    EXPECT_EQ(labelText(dialog, "detailGroup"), QStringLiteral("Test/Vegetation"));
    // The customisation it came from, by name.
    EXPECT_EQ(labelText(dialog, "detailSource"), QStringLiteral("test_symbols"));

    // The search reaches a survey code: only the mark is drawn by AC*.
    tree->setCurrentItem(tree->topLevelItem(0));
    plainChild<katana::qt::FilterBar>(dialog, "symbolFilter")->setText(QStringLiteral("ac*"));
    EXPECT_EQ(dialog.shownNames(), std::vector<std::string>{"TEST Survey Mark"});

    // The At vertices chip: the three definitions drawn at vertices.
    plainChild<katana::qt::FilterBar>(dialog, "symbolFilter")->setText({});
    child<QToolButton>(dialog, "filterVertexmode")->click();
    EXPECT_EQ(dialog.shownNames(),
              (std::vector<std::string>{"TEST Survey Mark", "TEST U Turn", "TEST Valve"}));
    // The chip says what it lists, with the count of those three.
    EXPECT_EQ(child<QToolButton>(dialog, "filterVertexmode")->text(),
              QStringLiteral("At vertices (3)"));
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

    // A world-units cross of 1 m strokes (-0.5..0.5 each way) with a 0.3 m
    // circle inside: 1 x 1 m on the ground. At 1:500 a plot millimetre is
    // 0.5 m, so it prints 1 / 0.5 = 2 mm each way.
    EXPECT_EQ(labelText(dialog, "detailPrint"),
              QStringLiteral("2 × 2 mm at 1:500, 1 × 1 m on the ground"));
    EXPECT_EQ(labelText(dialog, "detailExtent"), QStringLiteral("1 × 1 m"));
    EXPECT_EQ(labelText(dialog, "detailUnits"), QStringLiteral("World: metres on the ground"));
    EXPECT_EQ(labelText(dialog, "detailMode"),
              QStringLiteral("at vertices: drawn at each vertex of a string"));
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

TEST(SymbolLibrary, ImportingMergesTheFilesDefinitionsIntoTheLibraryAndLogsWhatItAddedAndReplaced)
{
    // The session starts with the linestyles and its own TEST Tree;
    // importing the symbols adds three and replaces the tree - and keeps the
    // three linestyles, because a load from here never replaces (D1): 3 + 1
    // + 3.
    LibraryFixture fixture({"test_linestyles"});
    katana::entity::StyleLibrary library = fixture.document.styleLibrary();
    katana::entity::LineStyle ownTree;
    ownTree.name = "TEST Tree";
    ownTree.strokes = {
        katana::entity::Stroke{.op = katana::entity::StrokeOp::Draw, .point = {1.0, 0.0}}};
    ASSERT_TRUE(library.add(ownTree).ok());
    fixture.document.setStyleLibrary(std::move(library));
    SymbolLibraryDialog dialog(fixture.context);

    ASSERT_TRUE(dialog.importDefinitionsFile(customisationFixtureFile("test_symbols")));

    // The dialog handed the executor the line CUSTOMISE DEFINITIONS and the
    // file, and the verb said what it did: 3 added (Survey Mark, U Turn and
    // Valve) and 1 replaced (Tree), the load named by its customisation.
    ASSERT_EQ(fixture.executor.ran.size(), 1);
    EXPECT_EQ(fixture.executor.ran.front(),
              QStringLiteral("CUSTOMISE DEFINITIONS ") +
                  katana::qt::test::quotedFile(customisationFixtureFile("test_symbols")));
    EXPECT_TRUE(fixture.executor.lastReply.contains(
        QStringLiteral("loaded file=")));
    EXPECT_TRUE(fixture.executor.lastReply.contains(
        QStringLiteral("name=test_symbols definitions_added=3 definitions_replaced=1 ")))
        << fixture.executor.lastReply.toStdString();
    EXPECT_EQ(fixture.document.styleLibrary().size(), 7u);
    EXPECT_TRUE(fixture.document.styleLibrary().contains("TEST Dashed Kerb"));
    // What came in says where it came from, the tree that was replaced
    // included, and the session records that customisation as one that
    // brought definitions and no rules.
    EXPECT_EQ(fixture.document.styleLibrary().find("TEST Valve")->source, "test_symbols");
    EXPECT_EQ(fixture.document.styleLibrary().find("TEST Tree")->source, "test_symbols");
    EXPECT_TRUE(fixture.document.styleLibrary().find("TEST Tree")->symbol);
    const auto& sources = fixture.document.customisationState().sources;
    const auto source = std::ranges::find_if(
        sources, [](const auto& each) { return each.name == "test_symbols"; });
    ASSERT_NE(source, sources.end());
    EXPECT_TRUE(source->definitions);
    EXPECT_FALSE(source->rules);
    katana::qt::test::processEvents();
    const auto shown = dialog.shownNames();
    EXPECT_NE(std::ranges::find(shown, std::string("TEST Valve")), shown.end());

    // A file that is not there fails, says so, and changes nothing.
    fixture.logged.clear();
    EXPECT_FALSE(dialog.importDefinitionsFile(customisationFixtureDirectory() /
                                              "no_such.customisation.json"));
    ASSERT_EQ(fixture.logged.size(), 1u);
    EXPECT_TRUE(fixture.logged.front().second);
    EXPECT_EQ(fixture.document.styleLibrary().size(), 7u);
}

TEST(SymbolLibrary, ImportTakesAFilesDefinitionsAndColoursAndLeavesItsRulesAndSettingsAlone)
{
    // A customisation written for this test, holding one of everything: a
    // symbol whose pen is a colour of its own, that colour, a rule and the
    // linework codes.
    LibraryFixture fixture; // 7 definitions, 11 rules, no colours
    const std::filesystem::path path = scratchFile("extra.customisation.json");
    {
        std::ofstream out(path, std::ios::binary | std::ios::trunc);
        out << R"({
  "format": "katana-customisation",
  "version": 1,
  "name": "extra",
  "colours": {
    "sui test purple": "#800080"
  },
  "linework": {"start": "BEG"},
  "symbols": [
    {"name": "TEST Extra Mark", "group": "Test/Marks", "atVertices": true, "strokes": [
      ["pen", "sui test purple"],
      ["circle", 1]
    ]}
  ],
  "codes": [
    {"key": "ZZ*", "sets": "feature", "layer": "TEST MISC"}
  ]
}
)";
    }
    SymbolLibraryDialog dialog(fixture.context);

    ASSERT_TRUE(dialog.importDefinitionsFile(path));
    std::filesystem::remove(path);

    // Taken: the definition, listed as a symbol under its customisation's
    // name, and the colour.
    const katana::entity::StyleLibrary& library = fixture.document.styleLibrary();
    ASSERT_EQ(library.size(), 8u);
    ASSERT_TRUE(library.contains("TEST Extra Mark"));
    EXPECT_TRUE(library.find("TEST Extra Mark")->symbol);
    EXPECT_EQ(library.find("TEST Extra Mark")->source, "extra");
    EXPECT_EQ(fixture.document.customisationState().colours.find("sui test purple"),
              std::optional(katana::entity::Color{128, 0, 128, 255}));
    // One definition and one colour, each added and none replaced.
    EXPECT_TRUE(fixture.executor.lastReply.contains(
        QStringLiteral("name=extra definitions_added=1 definitions_replaced=0 codes_added=0 "
                       "codes_replaced=0 colours_added=1 colours_replaced=0 ")))
        << fixture.executor.lastReply.toStdString();

    // Left alone: its rule, and its spelling of Start. The reply says so, as
    // a record: one rule, linework codes, no automation.
    EXPECT_EQ(fixture.document.surveyMap().size(), 11u);
    EXPECT_EQ(fixture.document.customisationState().linework.start, "ST");
    EXPECT_TRUE(fixture.executor.lastReply.contains(
        QStringLiteral("left rules=1 linework=yes automation=no")))
        << fixture.executor.lastReply.toStdString();
    // The session's record of what went into it: "extra" brought
    // definitions, and no rules.
    const auto& sources = fixture.document.customisationState().sources;
    const auto extra =
        std::ranges::find_if(sources, [](const auto& each) { return each.name == "extra"; });
    ASSERT_NE(extra, sources.end());
    EXPECT_TRUE(extra->definitions);
    EXPECT_FALSE(extra->rules);
}

TEST(SymbolLibrary, ImportCallsTheCommitHookBeforeTheInstallAndWhatItHandsBackAfterOneThatWasTaken)
{
    // CustomisationContext::beginCommit is how a session whose customisation
    // is the kept one stays kept across an editor's own commit: its maker is
    // asked BEFORE - the install leaves the session "not kept", so only
    // before it can "was it kept?" be answered - and what it hands back is
    // called AFTER an install that was taken. The library knows neither; it
    // calls, in that order, as the definition editor's Save does. A counting
    // hook that notes at each call the library's generation, and whether the
    // session says it is kept, shows which side of the commit a call came on.
    LibraryFixture fixture({"test_linestyles"});
    // The session as a start leaves one: kept.
    ASSERT_TRUE(fixture.document
                    .installCustomisation(fixture.document.customisation(),
                                          katana::cad::CustomisationOrigin::Kept, true)
                    .ok());
    ASSERT_TRUE(fixture.document.customisationState().kept);
    int begun = 0;
    int finished = 0;
    std::uint64_t libraryAtBegin = 0;
    std::uint64_t libraryAtFinish = 0;
    bool keptAtBegin = false;
    bool keptAtFinish = true;
    fixture.context.beginCommit = [&]() -> std::function<void()> {
        ++begun;
        libraryAtBegin = fixture.document.libraryGeneration();
        keptAtBegin = fixture.document.customisationState().kept;
        return [&] {
            ++finished;
            libraryAtFinish = fixture.document.libraryGeneration();
            keptAtFinish = fixture.document.customisationState().kept;
        };
    };
    SymbolLibraryDialog dialog(fixture.context);

    // What is not a commit is asked of the hook all the same (the line is
    // run between the two calls, as the definition editor's Delete does) but
    // is never followed by what it hands back: a file that is not there, and
    // one that reads and holds nothing a symbol library takes - the fixture
    // of survey codes, which has rules and no definition or colour.
    EXPECT_FALSE(dialog.importDefinitionsFile(scratchFile("no-such.customisation.json")));
    EXPECT_FALSE(dialog.importDefinitionsFile(customisationFixtureFile("test_survey")));
    EXPECT_EQ(begun, 2);
    EXPECT_EQ(finished, 0);
    EXPECT_TRUE(fixture.document.customisationState().kept);

    const std::uint64_t before = fixture.document.libraryGeneration();
    ASSERT_TRUE(dialog.importDefinitionsFile(customisationFixtureFile("test_symbols")));
    EXPECT_EQ(begun, 3);
    EXPECT_EQ(finished, 1);
    EXPECT_EQ(libraryAtBegin, before) << "asked before the library was touched";
    EXPECT_TRUE(keptAtBegin) << "and so while the session still said it was kept";
    EXPECT_GT(libraryAtFinish, before) << "answered once it had been";
    EXPECT_FALSE(keptAtFinish) << "which is what the answer is there to put right";
    EXPECT_TRUE(fixture.document.styleLibrary().contains("TEST Valve"));
}

TEST(SymbolLibrary, ImportingDefinitionsLeavesOutASourceThatBroughtOnlyRules)
{
    // A project drawn with a customisation "B", opened where B is not
    // loaded: B is missing, and the session says so.
    const std::filesystem::path root =
        std::filesystem::temp_directory_path() / "katana_test_symbol_library_sources";
    std::error_code ignored;
    std::filesystem::remove_all(root, ignored);
    std::filesystem::create_directories(root);
    const std::filesystem::path project = root / "recorded.katana";
    {
        katana::storage::ProjectMetadata metadata;
        metadata.customisation = {"B"};
        auto store = katana::storage::ProjectStore::create(project, metadata);
        ASSERT_TRUE(store.ok()) << store.error().describe();
        const auto saved =
            store->save(katana::storage::captureModel(katana::entity::Model{}, metadata));
        ASSERT_TRUE(saved.ok()) << saved.error().describe();
    }
    Document document;
    ASSERT_TRUE(document.open(project).ok());
    ASSERT_EQ(document.customisationState().missingAtOpen, std::vector<std::string>{"B"});

    // A customisation that was itself merged from two, and lists them: A
    // brought its one definition and B its one rule.
    const std::filesystem::path file = scratchFile("both.customisation.json");
    {
        std::ofstream out(file, std::ios::binary | std::ios::trunc);
        out << R"({
  "format": "katana-customisation",
  "version": 1,
  "name": "both",
  "sources": [
    {"name": "A", "definitions": true, "notice": ["A's notice."]},
    {"name": "B", "rules": true, "notice": ["B's notice."]}
  ],
  "symbols": [
    {"name": "TEST A Mark", "atVertices": true, "from": "A", "strokes": [
      ["circle", 1]
    ]}
  ],
  "codes": [
    {"key": "BB*", "sets": "feature", "layer": "TEST B"}
  ]
}
)";
    }
    DefinitionThumbnails thumbnails;
    katana::qt::test::InterpreterRunner executor(document);
    CustomisationContext context;
    context.document = &document;
    context.thumbnails = &thumbnails;
    context.run = executor.runner();
    context.log = [](const QString&, bool) {};
    {
        SymbolLibraryDialog dialog(context);
        ASSERT_TRUE(dialog.importDefinitionsFile(file));
    }
    std::filesystem::remove(file);

    // The definition came in, and A with it: what A brought HERE is its
    // definitions, and its notice travels with them.
    ASSERT_TRUE(document.styleLibrary().contains("TEST A Mark"));
    EXPECT_EQ(document.styleLibrary().find("TEST A Mark")->source, "A");
    EXPECT_TRUE(document.surveyMap().empty());
    // B brought the file its rules, and none of them was taken: B is no
    // source of this session, and it is as missing as it was. Listed, it
    // would have passed for loaded - the warning gone and the record of the
    // project saying B is here - with not one of its rules in the session.
    EXPECT_EQ(document.customisationState().sources,
              (std::vector<katana::entity::CustomisationSourceNote>{
                  {"A", true, false, {"A's notice."}}}));
    EXPECT_EQ(document.customisationState().missingAtOpen, std::vector<std::string>{"B"});

    // A save keeps B in the record as a name still wanted, so the project
    // opened again - here, in this very session - still says B is missing.
    ASSERT_TRUE(document.save().ok());
    EXPECT_EQ(document.metadata().customisation, (std::vector<std::string>{"A", "B"}));
    ASSERT_TRUE(document.open(project).ok());
    EXPECT_EQ(document.customisationState().missingAtOpen, std::vector<std::string>{"B"});
    std::filesystem::remove_all(root, ignored);
}

TEST(SymbolLibrary, ImportRefusesAFileThatIsNotAKatanaCustomisationAndOneWithNothingForIt)
{
    LibraryFixture fixture;
    SymbolLibraryDialog dialog(fixture.context);
    const auto generation = fixture.document.libraryGeneration();

    // A definition as the older style libraries wrote one: not JSON.
    const std::filesystem::path other = scratchFile("older_library.txt");
    {
        std::ofstream out(other, std::ios::binary | std::ios::trunc);
        out << "worldstyle \"TEST Old\" {\n  move 0 0\n  draw 1 0\n}\n";
    }
    EXPECT_FALSE(dialog.importDefinitionsFile(other));
    std::filesystem::remove(other);
    ASSERT_EQ(fixture.logged.size(), 1u);
    EXPECT_TRUE(fixture.logged.front().second);
    EXPECT_TRUE(fixture.logged.front().first.contains(
        QStringLiteral("not a Katana customisation file")))
        << fixture.logged.front().first.toStdString();

    // A customisation of survey codes alone: nothing a symbol library takes,
    // and the refusal says where its rules are loaded instead.
    fixture.logged.clear();
    EXPECT_FALSE(dialog.importDefinitionsFile(customisationFixtureFile("test_survey")));
    ASSERT_EQ(fixture.logged.size(), 1u);
    EXPECT_TRUE(fixture.logged[0].second);
    EXPECT_TRUE(fixture.logged[0].first.contains(
        QStringLiteral("the file holds no definitions or colours to load, so nothing was "
                       "loaded; its survey code rules are loaded by CUSTOMISE <file>")))
        << fixture.logged[0].first.toStdString();

    EXPECT_EQ(fixture.document.styleLibrary().size(), 7u);
    EXPECT_EQ(fixture.document.surveyMap().size(), 11u);
    EXPECT_EQ(fixture.document.libraryGeneration(), generation);
}

TEST(SymbolLibrary, AHeadlessSessionOpensNoFileDialogAndAChooserStandsInForOne)
{
    LibraryFixture fixture({"test_linestyles"});
    SymbolLibraryDialog dialog(fixture.context);
    ASSERT_TRUE(dialog.headless());

    child<QPushButton>(dialog, "importDefinitions")->click();
    ASSERT_EQ(fixture.logged.size(), 1u);
    EXPECT_TRUE(fixture.logged.front().second);
    EXPECT_TRUE(fixture.logged.front().first.contains(QStringLiteral("importDefinitionsFile")));

    dialog.chooseImportFile = [] { return customisationFixtureFile("test_symbols"); };
    child<QPushButton>(dialog, "importDefinitions")->click();
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
    const std::filesystem::path path = scratchFile("export.customisation.json");

    ASSERT_TRUE(dialog.exportSelectedTo(path));

    const std::string written = readBytes(path);
    std::filesystem::remove(path);
    // A Katana customisation file, under the session's name - the first
    // fixture loaded. Of the three customisations the session was loaded
    // from, the ONE both selected symbols came from is this file's source,
    // one line (docs/customisation.md, "Layout", rules 4, 6 and 11); then the
    // lists at once: the session has no colours, and no source it leaves out
    // has a notice to carry.
    //
    // CHANGED when the managers took the one rule a part is written by
    // (cad::customisationPart, which CUSTOMISE EXPORT ... ONLY writes by
    // too). This pinned the managers' own rule, by which every customisation
    // that brought the session ANY definition was listed - test_linestyles
    // too, with not one of its definitions in the file. A file's sources are
    // taken at their word where it is loaded, by name: so listed, two symbols
    // passed for the linestyles' customisation as well, and would have taken
    // away a project's warning that it is missing. By the one rule a part
    // lists the sources of what is WRITTEN.
    const std::string head = "{\n"
                             "  \"format\": \"katana-customisation\",\n"
                             "  \"version\": 1,\n"
                             "  \"name\": \"test_symbols\",\n"
                             "  \"sources\": [\n"
                             "    {\"name\": \"test_symbols\", \"definitions\": true}\n"
                             "  ],\n"
                             "  \"symbols\": [\n";
    EXPECT_EQ(written.substr(0, head.size()), head);
    auto read = katana::entity::customisationFromJson(written);
    ASSERT_TRUE(read.ok()) << read.error().describe();
    // Neither the source that brought the session's rules nor the one that
    // brought its linestyles is a source of this file.
    EXPECT_EQ(read->sources, (std::vector<katana::entity::CustomisationSourceNote>{
                                 {"test_symbols", true, false, {}}}));
    EXPECT_TRUE(read->notice.empty());
    EXPECT_EQ(read->library.names(), (std::vector<std::string>{"TEST Survey Mark", "TEST Valve"}));
    // The format is lossless: the valve reads back as the session holds it,
    // where it came from and that it is listed as a symbol included - the two
    // things a style library file could not hold.
    const katana::entity::LineStyle& valve = *fixture.document.styleLibrary().find("TEST Valve");
    ASSERT_EQ(valve.source, "test_symbols");
    ASSERT_TRUE(valve.symbol);
    EXPECT_EQ(*read->library.find("TEST Valve"), valve);
    // The definitions alone: none of the session's 11 rules, and nothing
    // said of its settings - a file of two symbols for a colleague must not
    // reset their control codes.
    EXPECT_TRUE(read->map.empty());
    EXPECT_FALSE(read->linework.has_value());
    EXPECT_FALSE(read->automation.has_value());
    EXPECT_TRUE(fixture.loggedExactly(
        QStringLiteral("Not exported, as no library defines them: cross")));
}

// The file a session of four customisations, each with its author's notice,
// exports two of its symbols to (fixture_customisation.hpp,
// installNoticedSession) - written out by hand from docs/customisation.md,
// "Layout". The session's own name, description and notice, and after it the
// notice of the one source the file leaves out; of its four sources the two
// the symbols came from, each with its notice, and the table of colours,
// because the file carries one of its colours; the colour a pen names and
// not the other; and the two symbols, in name order, the one that is not the
// session's own saying where it is from. No rules, nothing of the linework
// codes, and not what the session is based on.
//
// TWO THINGS HERE CHANGED when the managers took the one rule a part is
// written by (cad::customisationPart, which CUSTOMISE EXPORT writes a part by
// too); this file first pinned the managers' own:
//   - "Client codes, for this job only." is in the notice. The client's
//     customisation brought rules alone, so nothing of it is written and it
//     is no source of the file; by the managers' rule its notice went with
//     it. By the one rule the notice of a source left out is written with
//     the part's own - nothing says whose a colour is, and no export drops
//     an author's notice.
//   - no "basedOn", though the session has one (the fixture's base says what
//     it was made from): a part is not an edition of the built-in, to be
//     told from another edition at a start. The managers' rule kept it.
const char* const kNoticedSymbols =
    "{\n"
    "  \"format\": \"katana-customisation\",\n"
    "  \"version\": 1,\n"
    "  \"name\": \"base\",\n"
    "  \"description\": \"The base set.\",\n"
    "  \"notice\": [\n"
    "    \"Base: all rights reserved.\",\n"
    "    \"Client codes, for this job only.\"\n"
    "  ],\n"
    "  \"sources\": [\n"
    "    {\"name\": \"base\", \"definitions\": true},\n"
    "    {\"name\": \"marks\", \"definitions\": true, \"notice\": [\"Marks drawn by hand.\"]},\n"
    "    {\"name\": \"tints\", \"notice\": [\"Tints: free to use.\"]}\n"
    "  ],\n"
    "  \"colours\": {\n"
    "    \"tint teal\": \"#008080\"\n"
    "  },\n"
    "  \"symbols\": [\n"
    "    {\"name\": \"BASE Peg\", \"atVertices\": true, \"strokes\": [\n"
    "      [\"pen\", \"tint teal\"],\n"
    "      [\"circle\", 0.5]\n"
    "    ]},\n"
    "    {\"name\": \"MARK Cross\", \"atVertices\": true, \"from\": \"marks\", \"strokes\": [\n"
    "      [\"move\", -0.5, 0],\n"
    "      [\"draw\", 0.5, 0]\n"
    "    ]}\n"
    "  ]\n"
    "}\n";

TEST(SymbolLibrary, AnExportCarriesTheSessionsNoticeAndTheSourcesThatBroughtDefinitions)
{
    Document document;
    katana::qt::test::installNoticedSession(document);
    ASSERT_EQ(document.customisationState().name, "base");
    ASSERT_EQ(document.customisationState().sources.size(), 4u);
    ASSERT_TRUE(document.customisation().basedOn.has_value());
    DefinitionThumbnails thumbnails;
    katana::qt::test::InterpreterRunner executor(document);
    CustomisationContext context;
    context.document = &document;
    context.thumbnails = &thumbnails;
    context.run = executor.runner();
    context.log = [](const QString&, bool) {};
    SymbolLibraryDialog dialog(context);
    QListView* grid = child<QListView>(dialog, "symbolGrid");
    const auto select = [&](const char* name) {
        const int row = dialog.gridModel().rowOf(name);
        ASSERT_GE(row, 0) << name;
        grid->selectionModel()->select(grid->model()->index(row, 0),
                                       QItemSelectionModel::Select);
    };
    select("MARK Cross");
    const std::filesystem::path path = scratchFile("noticed.customisation.json");

    // The cross alone, which came from marks and has no pen. The file
    // carries no colour, and so nothing of the table of colours: it is no
    // source of this file.
    //
    // CHANGED with the one rule (the comment above kNoticedSymbols): the
    // sources were `base` and `marks`, every source that brought the session
    // a definition, and are `marks` alone - nothing of base's is written, and
    // listed it would pass for loaded wherever the file went. And the notice
    // was the session's alone; it is followed now by the notice of each
    // source left out that has one, in their order: the client's and the
    // table of colours'.
    ASSERT_TRUE(dialog.exportSelectedTo(path));
    auto read = katana::entity::customisationFromJson(readBytes(path));
    ASSERT_TRUE(read.ok()) << read.error().describe();
    EXPECT_EQ(read->library.names(), std::vector<std::string>{"MARK Cross"});
    EXPECT_TRUE(read->colours.empty());
    EXPECT_EQ(read->notice,
              (std::vector<std::string>{"Base: all rights reserved.",
                                        "Client codes, for this job only.",
                                        "Tints: free to use."}));
    EXPECT_EQ(read->sources, (std::vector<katana::entity::CustomisationSourceNote>{
                                 {"marks", true, false, {"Marks drawn by hand."}}}));
    EXPECT_FALSE(read->basedOn.has_value());

    // With the peg, whose pen is one of the table's colours: the whole file.
    select("BASE Peg");
    ASSERT_TRUE(dialog.exportSelectedTo(path));
    EXPECT_EQ(readBytes(path), kNoticedSymbols);

    // The colleague the file is for imports its definitions into a session
    // of their own, which has no name yet: it takes the file's, with its
    // description and notice - the two lines the file carries (was: the
    // session's one) - and each source the file lists keeps its own notice
    // (cad/customisation_merge.hpp). They are shown whose data it is.
    {
        Document colleague;
        DefinitionThumbnails theirThumbnails;
        katana::qt::test::InterpreterRunner theirExecutor(colleague);
        CustomisationContext theirs;
        theirs.document = &colleague;
        theirs.thumbnails = &theirThumbnails;
        theirs.run = theirExecutor.runner();
        theirs.log = [](const QString&, bool) {};
        SymbolLibraryDialog library(theirs);
        ASSERT_TRUE(library.importDefinitionsFile(path));
        const katana::cad::CustomisationState& state = colleague.customisationState();
        EXPECT_EQ(state.name, "base");
        EXPECT_EQ(state.description, "The base set.");
        EXPECT_EQ(state.notice,
                  (std::vector<std::string>{"Base: all rights reserved.",
                                            "Client codes, for this job only."}));
        EXPECT_EQ(state.sources, (std::vector<katana::entity::CustomisationSourceNote>{
                                     {"base", true, false, {}},
                                     {"marks", true, false, {"Marks drawn by hand."}},
                                     {"tints", false, false, {"Tints: free to use."}}}));
        EXPECT_EQ(colleague.styleLibrary().names(),
                  (std::vector<std::string>{"BASE Peg", "MARK Cross"}));
        EXPECT_EQ(colleague.styleLibrary().find("BASE Peg")->source, "base");
        EXPECT_EQ(colleague.styleLibrary().find("MARK Cross")->source, "marks");
        EXPECT_EQ(state.colours.find("tint teal"),
                  std::optional(katana::entity::Color{0, 128, 128, 255}));
        EXPECT_TRUE(colleague.surveyMap().empty());
    }
    std::filesystem::remove(path);
}

TEST(SymbolLibrary, AnExportCarriesTheColoursItsPensNameAndNoOthers)
{
    // Two colours of the session's own, and a symbol whose pen names one.
    LibraryFixture fixture;
    katana::entity::ColourTable colours;
    ASSERT_TRUE(colours.add("sui test purple", katana::entity::Color{128, 0, 128, 255}).ok());
    ASSERT_TRUE(colours.add("sui test teal", katana::entity::Color{0, 128, 128, 255}).ok());
    fixture.document.setColourTable(colours);
    katana::entity::StyleLibrary library = fixture.document.styleLibrary();
    katana::entity::LineStyle mark;
    mark.name = "TEST Purple Mark";
    mark.atVertices = true;
    mark.symbol = true;
    // The pen spelled another way than the table spells it: one name, by the
    // fold a colour name is compared in.
    mark.strokes = {katana::entity::Stroke{.op = katana::entity::StrokeOp::Pen,
                                           .pen = "SUI_Test_Purple"},
                    katana::entity::Stroke{.op = katana::entity::StrokeOp::Circle, .radius = 1.0}};
    ASSERT_TRUE(library.add(mark).ok());
    fixture.document.setStyleLibrary(std::move(library));
    SymbolLibraryDialog dialog(fixture.context);
    QListView* grid = child<QListView>(dialog, "symbolGrid");
    for (const char* name : {"TEST Purple Mark", "TEST Tree"}) {
        grid->selectionModel()->select(grid->model()->index(dialog.gridModel().rowOf(name), 0),
                                       QItemSelectionModel::Select);
    }
    const std::filesystem::path path = scratchFile("coloured.customisation.json");

    ASSERT_TRUE(dialog.exportSelectedTo(path));

    auto read = katana::entity::customisationFromJson(readBytes(path));
    std::filesystem::remove(path);
    ASSERT_TRUE(read.ok()) << read.error().describe();
    EXPECT_EQ(read->library.names(), (std::vector<std::string>{"TEST Purple Mark", "TEST Tree"}));
    // The purple the mark's pen names, as the table spells it; not the teal
    // no selected pen names, and not the tree's "green", which is a standard
    // name and no customisation's to define.
    ASSERT_EQ(read->colours.size(), 1u);
    EXPECT_EQ(read->colours.entries().front().name, "sui test purple");
    EXPECT_EQ(read->colours.entries().front().colour, (katana::entity::Color{128, 0, 128, 255}));
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
    const std::filesystem::path out = scratchFile("ctrl_export.customisation.json");
    ASSERT_TRUE(dialog.exportSelectedTo(out));
    const std::string text = readBytes(out);
    std::filesystem::remove(out);
    auto written = katana::entity::customisationFromJson(text);
    ASSERT_TRUE(written.ok()) << written.error().describe();
    EXPECT_EQ(written->library.names(), kept);
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
    EXPECT_FALSE(dialog.importDefinitionsFile(customisationFixtureFile("test_symbols")));
    EXPECT_FALSE(dialog.selectPointsUsing());
    dialog.reload();
    katana::qt::test::paint(dialog);
}

// ---- the colour a pen inside a definition names -------------------------------------
//
// A definition's pen NAMES its colour, and the one painter every picture and
// the plan go through resolves the name as the drawing does: the session's
// customisation first, then the standard names. Purple is 128 0 128 here, and
// the standard blue is the CSS one, 0 0 255 (docs/customisation.md, "Colour
// names").

TEST(SymbolLibrary, APenIsTheColourTheCustomisationGivesItsNameAndTheEntitysPenWhenNothingDoes)
{
    QPen entity(QColor(10, 20, 30), 2.0);
    entity.setCapStyle(Qt::FlatCap);
    katana::entity::ColourTable colours;
    ASSERT_TRUE(colours.add("sui test purple", katana::entity::Color{128, 0, 128, 255}).ok());

    // The customisation's name, however it is spelled within the one fold:
    // letter case, and `_` or `-` for a blank.
    for (const char* spelled : {"sui test purple", "SUI_Test-Purple"}) {
        const QPen pen = katana::qt::stylePenFor(entity, spelled, nullptr, &colours);
        EXPECT_EQ(pen.color(), QColor(128, 0, 128)) << spelled;
        // Only the colour is the definition's: the width and the cap are the
        // entity's.
        EXPECT_EQ(pen.widthF(), 2.0) << spelled;
        EXPECT_EQ(pen.capStyle(), Qt::FlatCap) << spelled;
    }
    // A standard name is its standard colour, with a table and without one.
    EXPECT_EQ(katana::qt::stylePenFor(entity, "blue", nullptr, &colours).color(),
              QColor(0, 0, 255));
    EXPECT_EQ(katana::qt::stylePenFor(entity, "blue", nullptr).color(), QColor(0, 0, 255));
    // What an unresolved pen draws is what it always drew: the entity's pen,
    // whole. A plot pen's number is such a name, and so is the customisation's
    // own where no table is given or the table is empty.
    const katana::entity::ColourTable none;
    EXPECT_EQ(katana::qt::stylePenFor(entity, "pen 035", nullptr, &colours), entity);
    EXPECT_EQ(katana::qt::stylePenFor(entity, "sui test purple", nullptr), entity);
    EXPECT_EQ(katana::qt::stylePenFor(entity, "sui test purple", nullptr, &none), entity);
    EXPECT_EQ(katana::qt::stylePenFor(entity, "", nullptr, &colours), entity);
}

TEST(SymbolLibrary, ThePicturesAndThePlanDrawAPenInTheColourTheSessionsCustomisationGivesIt)
{
    // A cross 2 m across whose every stroke is in a pen only a customisation
    // can define, and a point wearing it.
    LibraryFixture fixture;
    katana::entity::StyleLibrary library = fixture.document.styleLibrary();
    katana::entity::LineStyle mark;
    mark.name = "TEST Purple Mark";
    mark.atVertices = true;
    mark.symbol = true;
    using katana::entity::Stroke;
    using katana::entity::StrokeOp;
    mark.strokes = {Stroke{.op = StrokeOp::Pen, .pen = "sui test purple"},
                    Stroke{.op = StrokeOp::Move, .point = {-1.0, 0.0}},
                    Stroke{.op = StrokeOp::Draw, .point = {1.0, 0.0}},
                    Stroke{.op = StrokeOp::Move, .point = {0.0, -1.0}},
                    Stroke{.op = StrokeOp::Draw, .point = {0.0, 1.0}}};
    ASSERT_TRUE(library.add(mark).ok());
    fixture.document.setStyleLibrary(std::move(library));
    katana::entity::Style style;
    style.name = "Purple Marks";
    style.symbol = "TEST Purple Mark";
    ASSERT_TRUE(fixture.document.execute(katana::commands::createStyle(style)).ok());
    ASSERT_TRUE(fixture.document
                    .execute(katana::commands::createPoint(
                        {0.0, 0.0}, {.layer = "0", .style = "Purple Marks"}))
                    .ok());

    // The grid's picture of it, and the plan 64 pixels square about the
    // point at 16 pixels a metre, both on black.
    DefinitionThumbnails thumbnails;
    const auto picture = [&] {
        return thumbnails
            .thumbnail(fixture.document, katana::qt::ThumbnailKind::Symbol, "TEST Purple Mark",
                       QSize(64, 64), QColor(Qt::black))
            .image;
    };
    const auto plan = [&] {
        QImage image(64, 64, QImage::Format_ARGB32_Premultiplied);
        image.fill(Qt::black);
        katana::qt::PlanFrame frame;
        frame.transform.resize(64, 64);
        frame.transform.scale = 16.0;
        frame.transform.center = Point2(0.0, 0.0);
        katana::qt::PlanPaintCache cache;
        QPainter painter(&image);
        katana::qt::paintPlan(painter, katana::qt::planSourceOf(fixture.document), frame,
                              katana::qt::PlanPaintOptions{}, cache);
        painter.end();
        return image;
    };

    // And the preview the managers show a symbol in, on its screen ground.
    katana::qt::StylePreview preview(fixture.document);
    preview.setGround(katana::qt::PreviewGround::Screen);
    preview.resize(240, 160);
    preview.setSymbol("TEST Purple Mark");
    const auto previewed = [&] { return preview.grab().toImage(); };

    // Nothing defines the name yet: all three draw the mark, in the entity's
    // pen.
    QImage blank(64, 64, QImage::Format_ARGB32_Premultiplied);
    blank.fill(Qt::black);
    const QImage plainPlan = plan();
    EXPECT_TRUE(plainPlan != blank) << "the plan drew no mark at all";
    EXPECT_TRUE(picture() != blank) << "the picture drew no mark at all";
    EXPECT_FALSE(holdsPurple(picture()));
    EXPECT_FALSE(holdsPurple(plainPlan));
    EXPECT_FALSE(holdsPurple(previewed()));

    // The session's customisation defines it: all three draw it purple, the
    // picture having been dropped with the generation the table moved.
    katana::entity::ColourTable colours;
    ASSERT_TRUE(colours.add("sui test purple", katana::entity::Color{128, 0, 128, 255}).ok());
    fixture.document.setColourTable(colours);
    EXPECT_TRUE(holdsPurple(picture()));
    EXPECT_TRUE(holdsPurple(plan()));
    EXPECT_TRUE(holdsPurple(previewed()));
}

TEST(SymbolLibrary, ASymbolStampedOnThePlanIsDrawnAgainWhenTheCustomisationsColoursChange)
{
    // The plan as a VIEW paints it, where the test above painted it as a
    // plot would: symbols stamped from images kept between frames
    // (PlanPaintOptions::symbolSprites), in ONE cache the view keeps. A stamp
    // bakes in the colour its pen resolved to, and its key holds the symbol
    // and the entity's pen but not that colour - so a stamp made before the
    // customisation's colours changed must not be the one drawn after.
    LibraryFixture fixture;
    katana::entity::StyleLibrary library = fixture.document.styleLibrary();
    ASSERT_TRUE(library.add(purpleRing()).ok());
    fixture.document.setStyleLibrary(std::move(library));
    katana::entity::Style style;
    style.name = "Purple Rings";
    style.symbol = "TEST Purple Ring";
    ASSERT_TRUE(fixture.document.execute(katana::commands::createStyle(style)).ok());
    ASSERT_TRUE(fixture.document
                    .execute(katana::commands::createPoint(
                        {0.0, 0.0}, {.layer = "0", .style = "Purple Rings"}))
                    .ok());

    katana::qt::PlanPaintOptions options;
    options.symbolSprites = true;
    katana::qt::PlanPaintCache cache; // the view's, kept across both paints
    katana::qt::PlanPaintStats stats;
    const auto plan = [&] {
        // 64 pixels square about the point at 32 pixels a metre: the ring is
        // 32 pixels across, well above the size a symbol is drawn as a dot.
        QImage image(64, 64, QImage::Format_ARGB32_Premultiplied);
        image.fill(Qt::black);
        katana::qt::PlanFrame frame;
        frame.transform.resize(64, 64);
        frame.transform.scale = 32.0;
        frame.transform.center = Point2(0.0, 0.0);
        QPainter painter(&image);
        stats = katana::qt::paintPlan(painter, katana::qt::planSourceOf(fixture.document), frame,
                                      options, cache);
        painter.end();
        return image;
    };

    // Nothing defines the pen: the ring is stamped, in the entity's pen.
    const QImage before = plan();
    ASSERT_EQ(stats.spritesDrawn, 1u) << "the ring was not drawn from a stamp";
    ASSERT_EQ(cache.spriteCount(), 1u);
    EXPECT_FALSE(holdsPurple(before));
    // Painted again with nothing changed, it is the same stamp.
    EXPECT_TRUE(plan() == before);
    EXPECT_EQ(cache.spriteCount(), 1u);

    // The customisation defines it: the next frame, from the same cache, is
    // purple - and still from a stamp, a new one.
    katana::entity::ColourTable colours;
    ASSERT_TRUE(colours.add("sui test purple", katana::entity::Color{128, 0, 128, 255}).ok());
    fixture.document.setColourTable(colours);
    const QImage after = plan();
    EXPECT_EQ(stats.spritesDrawn, 1u);
    EXPECT_EQ(cache.spriteCount(), 1u);
    EXPECT_TRUE(holdsPurple(after));
}

// ---- the same pen in a sheet's legend ------------------------------------------------
//
// A legend's sample is painted by the same painter, handed the colours by
// LegendSampleContext, which a sheet fills from its plan's source. On paper,
// which is white, at 8 pixels a millimetre.

TEST(SymbolLibrary, ALegendSampleDrawsAPenInTheColourItsContextsColoursGiveIt)
{
    katana::entity::StyleLibrary library;
    ASSERT_TRUE(library.add(purpleRing()).ok());
    katana::entity::ColourTable colours;
    ASSERT_TRUE(colours.add("sui test purple", katana::entity::Color{128, 0, 128, 255}).ok());
    // The ring is 1 m across: 2 mm at 1:500, 16 pixels, in a black entry.
    katana::cad::plotting::LegendEntry entry;
    entry.kind = katana::cad::plotting::LegendKind::Symbol;
    entry.label = "RINGS";
    entry.colour = katana::entity::Color{0, 0, 0, 255};
    entry.symbol = "TEST Purple Ring";
    const katana::cad::PlotSettings plot;
    const QRectF cell(160.0, 85.6, 80.0, 28.8); // 10 x 3.6 mm about (200, 100)
    const auto sample = [&](const katana::entity::ColourTable* table) {
        QImage image(400, 200, QImage::Format_ARGB32_Premultiplied);
        image.fill(Qt::white);
        katana::qt::LegendSampleContext context;
        context.library = &library;
        context.colours = table;
        context.plot = &plot;
        context.pixelsPerMillimetre = 8.0;
        context.scale = 500.0;
        QPainter painter(&image);
        painter.setRenderHint(QPainter::Antialiasing);
        EXPECT_TRUE(katana::qt::paintLegendSample(painter, entry, cell, context));
        painter.end();
        return image;
    };

    // With no colours the pen's name means nothing, and the sample is drawn
    // in the entry's own black; with them it is purple.
    const QImage plain = sample(nullptr);
    EXPECT_GT(inkOnPaper(plain, plain.rect()), 20);
    EXPECT_EQ(purpleOnPaper(plain, plain.rect()), 0);
    const QImage coloured = sample(&colours);
    EXPECT_GT(purpleOnPaper(coloured, coloured.rect()), 20);
}

TEST(SymbolLibrary, ASheetsLegendDrawsAPenInTheColourThePlansSourceGivesIt)
{
    // A point wearing the ring, on an A3 sheet with a plan of it on the left
    // (30 to 230 mm across) and a legend on the right (300 to 400 mm).
    namespace plotting = katana::cad::plotting;
    using katana::geometry::Box2;
    katana::entity::Model model;
    katana::entity::Layer layer;
    layer.name = "RINGS";
    layer.color = katana::entity::Color{0, 0, 0, 255};
    ASSERT_TRUE(model.layers.add(layer).ok());
    katana::entity::Style style;
    style.name = "Ring";
    style.symbol = "TEST Purple Ring";
    ASSERT_TRUE(model.styles.add(style).ok());
    katana::entity::Entity point;
    point.geometry = katana::entity::PointGeometry{Point2(0.0, 0.0)};
    point.layer = "RINGS";
    point.style = "Ring";
    ASSERT_TRUE(model.entities.add(std::move(point)).ok());
    katana::entity::StyleLibrary library;
    ASSERT_TRUE(library.add(purpleRing()).ok());
    katana::entity::ColourTable colours;
    ASSERT_TRUE(colours.add("sui test purple", katana::entity::Color{128, 0, 128, 255}).ok());

    plotting::SheetSet set;
    plotting::Sheet sheet;
    sheet.id = "s1";
    plotting::Viewport plan;
    plan.id = "vp1";
    plan.kind = plotting::ViewportKind::Plan;
    plan.rect = Box2(Point2(30.0, 40.0), Point2(230.0, 240.0));
    plan.scale = 500.0;
    plan.centre = Point2(0.0, 0.0);
    plotting::Viewport legend;
    legend.id = "vpL";
    legend.kind = plotting::ViewportKind::Legend;
    legend.rect = Box2(Point2(300.0, 40.0), Point2(400.0, 145.0));
    sheet.viewports = {plan, legend};
    set.sheets.push_back(sheet);

    constexpr double kPixelsPerMillimetre = 8.0;
    const auto paper = katana::cad::paperDimensions(set.sheets[0].paper, set.sheets[0].landscape);
    const auto painted = [&](const katana::entity::ColourTable* table) {
        QImage image(static_cast<int>(std::lround(paper.widthMm * kPixelsPerMillimetre)),
                     static_cast<int>(std::lround(paper.heightMm * kPixelsPerMillimetre)),
                     QImage::Format_ARGB32_Premultiplied);
        image.fill(Qt::gray);
        katana::qt::SheetSource source;
        source.plan.model = &model;
        source.plan.library = &library;
        source.plan.libraryGeneration = 1;
        source.plan.colours = table;
        katana::qt::SheetPaintOptions options;
        options.pixelsPerMillimetre = kPixelsPerMillimetre;
        katana::qt::SheetPaintCache cache;
        QPainter painter(&image);
        (void)katana::qt::paintSheet(painter, set, 0, source, options, cache);
        painter.end();
        return image;
    };
    // A box of the paper (millimetres, y up) as device pixels (y down).
    const auto pixels = [&](const Box2& box) {
        return QRect(QPoint(static_cast<int>(std::floor(box.min.x * kPixelsPerMillimetre)),
                            static_cast<int>(std::floor((paper.heightMm - box.max.y) *
                                                        kPixelsPerMillimetre))),
                     QPoint(static_cast<int>(std::floor(box.max.x * kPixelsPerMillimetre)),
                            static_cast<int>(std::floor((paper.heightMm - box.min.y) *
                                                        kPixelsPerMillimetre))));
    };
    const QRect legendPixels = pixels(legend.rect);
    const QRect planPixels = pixels(plan.rect);

    // Handed no colours, the legend still lists the ring - its heading, its
    // label and its sample are ink - and none of that ink is purple.
    const QImage plain = painted(nullptr);
    EXPECT_GT(inkOnPaper(plain, legendPixels), 100);
    EXPECT_EQ(purpleOnPaper(plain, legendPixels), 0);
    EXPECT_EQ(purpleOnPaper(plain, planPixels), 0);
    // Handed the session's, the sample is purple, as the ring on the plan
    // beside it is.
    const QImage coloured = painted(&colours);
    EXPECT_GT(purpleOnPaper(coloured, legendPixels), 20);
    EXPECT_GT(purpleOnPaper(coloured, planPixels), 20);
}

} // namespace
