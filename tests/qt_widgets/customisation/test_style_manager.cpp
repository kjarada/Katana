// The styles and linetypes manager, driven through its widgets by objectName
// and judged by the Document: QT-02 (an unedited Save changes nothing and
// pushes nothing), bulk edits that leave <varies> fields alone, one undo step
// per merge and purge, reloads that keep the selection, and the linetype
// grid's validation.

#include <gtest/gtest.h>

#include <string>
#include <vector>

#include <QCheckBox>
#include <QComboBox>
#include <QCoreApplication>
#include <QDoubleSpinBox>
#include <QKeyEvent>
#include <QLabel>
#include <QLineEdit>
#include <QListWidget>
#include <QPushButton>
#include <QTableView>
#include <QTableWidget>
#include <QToolButton>

#include "customisation/customisation_context.hpp"
#include "customisation/name_picker.hpp"
#include "customisation/row_table_model.hpp"
#include "katana/cad/document.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/tables.hpp"
#include "style_manager.hpp"
#include "widget_harness.hpp"

using katana::cad::Document;
using katana::cad::LinetypeOrigin;
using katana::entity::Style;
using katana::qt::CustomisationContext;
using katana::qt::NamePicker;
using katana::qt::StyleManagerDialog;

namespace {

// Two library definitions written for these tests: a paper linestyle (a
// linestyle: not `mode vertex`, D2) and a vertex symbol. Only their names and
// kinds matter here.
katana::entity::StyleLibrary testLibrary()
{
    using katana::entity::Stroke;
    using katana::entity::StrokeOp;
    katana::entity::StyleLibrary library;
    katana::entity::LineStyle kerb;
    kerb.name = "TEST Dashed Kerb";
    kerb.units = katana::entity::StyleUnits::Paper;
    kerb.length = 4.0;
    kerb.strokes = {Stroke{.op = StrokeOp::Move, .point = {0.0, 0.0}},
                    Stroke{.op = StrokeOp::Draw, .point = {1.5, 0.0}}};
    EXPECT_TRUE(library.add(kerb).ok());
    katana::entity::LineStyle manhole;
    manhole.name = "TEST Manhole";
    manhole.atVertices = true;
    manhole.strokes = {Stroke{.op = StrokeOp::Circle, .radius = 0.5}};
    EXPECT_TRUE(library.add(manhole).ok());
    return library;
}

// The drawing:
//   linetype  DASHED {1, -0.5}
//   styles    "Old 12d"  linetype "Old 12d Kerb", symbol "Old 12d Pit",
//                        hatch "Old 12d Hatch" - three names NOTHING defines -
//                        weight 0.1234 and symbol size 0.03125, which no
//                        spin box here shows exactly; worn by nothing
//             Kerb       TEST Dashed Kerb (the library's), 0.35; one point
//             Fence      DASHED, 0.35; two points
struct Manager : ::testing::Test {
    Document document;
    CustomisationContext context;
    std::vector<QString> messages;
    std::vector<katana::entity::EntityId> fencePoints;
    katana::entity::EntityId kerbPoint = katana::entity::kInvalidEntityId;

    void SetUp() override
    {
        document.setStyleLibrary(testLibrary());
        katana::entity::Linetype dashed;
        dashed.name = "DASHED";
        dashed.pattern = {{1.0}, {-0.5}};
        must(katana::commands::createLinetype(dashed));

        Style old;
        old.name = "Old 12d";
        old.linetype = "Old 12d Kerb";
        old.symbol = "Old 12d Pit";
        old.hatchPattern = "Old 12d Hatch";
        old.lineWeight = 0.1234;
        old.symbolSize = 0.03125;
        old.description = "from 12d";
        must(katana::commands::createStyle(old));
        must(katana::commands::createStyle(style("Kerb", "TEST Dashed Kerb")));
        must(katana::commands::createStyle(style("Fence", "DASHED")));

        must(katana::commands::createPoint({0.0, 0.0}, {.layer = "0", .style = "Kerb"}));
        kerbPoint = document.lastCreatedEntities().front();
        for (double x : {1.0, 2.0}) {
            must(katana::commands::createPoint({x, 0.0}, {.layer = "0", .style = "Fence"}));
            fencePoints.push_back(document.lastCreatedEntities().front());
        }

        context.document = &document;
        context.log = [this](const QString& message, bool) { messages.push_back(message); };
    }

    void must(katana::commands::CommandPtr command)
    {
        const auto status = document.execute(std::move(command));
        ASSERT_TRUE(status.ok()) << status.error().describe();
    }
    static Style style(const char* name, const char* linetype)
    {
        Style made;
        made.name = name;
        made.linetype = linetype;
        made.lineWeight = 0.35;
        return made;
    }
    const Style& stored(const char* name) const { return *document.model().styles.find(name); }
    std::size_t steps() const { return document.history().undoCount(); }

    template <typename T> static T* child(QWidget& parent, const char* name)
    {
        T* found = parent.findChild<T*>(QString::fromLatin1(name));
        EXPECT_NE(found, nullptr) << name;
        return found;
    }
    // NamePicker has no Q_OBJECT of its own, so it is found as the combo it
    // is and told apart by RTTI.
    static NamePicker* picker(QWidget& parent, const char* name)
    {
        auto* found = dynamic_cast<NamePicker*>(child<QComboBox>(parent, name));
        EXPECT_NE(found, nullptr) << name << " is not a NamePicker";
        return found;
    }
    static void click(QWidget& parent, const char* name)
    {
        QPushButton* button = child<QPushButton>(parent, name);
        ASSERT_NE(button, nullptr);
        ASSERT_TRUE(button->isEnabled()) << name << " is disabled";
        button->click();
    }
    static void typeText(QWidget* target, const QString& text)
    {
        for (const QChar c : text) {
            QKeyEvent press(QEvent::KeyPress, 0, Qt::NoModifier, QString(c));
            QCoreApplication::sendEvent(target, &press);
            QKeyEvent release(QEvent::KeyRelease, 0, Qt::NoModifier, QString(c));
            QCoreApplication::sendEvent(target, &release);
        }
    }
};

} // namespace

TEST_F(Manager, SavingAStyleNamingUndefinedNamesUneditedChangesNothingAndPushesNoUndoStep)
{
    // Audit QT-02: the old form showed a name it could not list as the
    // previous row's, and Save wrote that back - every 12d style whose
    // library was not loaded lost its linetype and symbol.
    StyleManagerDialog dialog(context);
    const Style before = stored("Old 12d");
    const std::size_t stepsBefore = steps();

    dialog.selectStyles({"Old 12d"});
    ASSERT_EQ(dialog.selectedStyles(), std::vector<std::string>{"Old 12d"});
    // The form shows the stored names, kept and marked, not a neighbour's.
    NamePicker* linetype = picker(dialog, "styleLinetype");
    NamePicker* symbol = picker(dialog, "styleSymbol");
    EXPECT_EQ(linetype->currentName(), "Old 12d Kerb");
    EXPECT_FALSE(linetype->currentIsDefined());
    EXPECT_EQ(symbol->currentName(), "Old 12d Pit");
    EXPECT_FALSE(symbol->currentIsDefined());
    EXPECT_TRUE(child<QComboBox>(dialog, "styleHatch")->currentText().startsWith("Old 12d Hatch"));

    // A person touches the description, retyping what was there, and saves.
    auto* description = child<QLineEdit>(dialog, "styleDescription");
    description->selectAll();
    typeText(description, "from 12d");
    click(dialog, "styleSave");
    katana::qt::test::processEvents();

    // Field by field first, so a failure says which one moved.
    const Style& after = stored("Old 12d");
    EXPECT_EQ(after.linetype, "Old 12d Kerb");
    EXPECT_EQ(after.symbol, "Old 12d Pit");
    EXPECT_EQ(after.hatchPattern, "Old 12d Hatch");
    EXPECT_EQ(after.lineWeight, 0.1234) << "not what a spin box rounds it to";
    EXPECT_EQ(after.symbolSize, 0.03125);
    EXPECT_EQ(after, before) << "every field, byte for byte";
    EXPECT_EQ(steps(), stepsBefore) << "an unedited Save is no command";

    // And Save pressed again on the reloaded, untouched form: still nothing.
    click(dialog, "styleSave");
    katana::qt::test::processEvents();
    EXPECT_EQ(stored("Old 12d"), before);
    EXPECT_EQ(steps(), stepsBefore);
    EXPECT_EQ(picker(dialog, "styleLinetype")->currentName(), "Old 12d Kerb");
}

TEST_F(Manager, ChangingOnlyTheDescriptionOfAStyleNamingUndefinedNamesKeepsEveryOtherField)
{
    // The audit's own scenario for QT-02: change only the description.
    StyleManagerDialog dialog(context);
    const Style before = stored("Old 12d");
    dialog.selectStyles({"Old 12d"});
    auto* description = child<QLineEdit>(dialog, "styleDescription");
    description->selectAll();
    typeText(description, "kerb and channel");
    const std::size_t stepsBefore = steps();
    click(dialog, "styleSave");
    katana::qt::test::processEvents();

    Style expected = before;
    expected.description = "kerb and channel";
    EXPECT_EQ(stored("Old 12d").linetype, "Old 12d Kerb");
    EXPECT_EQ(stored("Old 12d").symbol, "Old 12d Pit");
    EXPECT_EQ(stored("Old 12d"), expected);
    EXPECT_EQ(steps(), stepsBefore + 1);
}

TEST_F(Manager, ABulkEditWritesTheEditedFieldAndLeavesTheVariesFieldsOfEachStyle)
{
    StyleManagerDialog dialog(context);
    dialog.selectStyles({"Kerb", "Fence"});
    ASSERT_EQ(dialog.selectedStyles(), (std::vector<std::string>{"Fence", "Kerb"}));

    // Their linetypes differ: the picker shows nothing, and says why.
    NamePicker* linetype = picker(dialog, "styleLinetype");
    EXPECT_EQ(linetype->currentName(), "");
    EXPECT_EQ(linetype->lineEdit()->placeholderText(), "<varies>");
    // Their weights agree: 0.35 is shown.
    auto* weight = child<QDoubleSpinBox>(dialog, "styleWeight");
    EXPECT_DOUBLE_EQ(weight->value(), 0.35);

    const std::size_t stepsBefore = steps();
    weight->setValue(0.5);
    click(dialog, "styleSave");
    katana::qt::test::processEvents();

    EXPECT_EQ(steps(), stepsBefore + 1) << "two styles, one undo step";
    EXPECT_EQ(stored("Kerb").lineWeight, 0.5);
    EXPECT_EQ(stored("Fence").lineWeight, 0.5);
    EXPECT_EQ(stored("Kerb").linetype, "TEST Dashed Kerb") << "<varies> was left alone";
    EXPECT_EQ(stored("Fence").linetype, "DASHED");
    // Still both selected after the reload, and still differing.
    EXPECT_EQ(dialog.selectedStyles(), (std::vector<std::string>{"Fence", "Kerb"}));
    EXPECT_EQ(picker(dialog, "styleLinetype")->lineEdit()->placeholderText(), "<varies>");
}

TEST_F(Manager, MergingAStyleThroughTheDialogIsOneUndoStepThatUndoRestores)
{
    StyleManagerDialog dialog(context);
    dialog.selectStyles({"Fence"});
    const std::size_t stepsBefore = steps();

    click(dialog, "styleMerge");
    auto* prompt = child<QWidget>(dialog, "promptPanel");
    ASSERT_FALSE(prompt->isHidden()) << "the target is asked for in the dialog, not a box";
    auto* choice = child<QComboBox>(dialog, "promptChoice");
    // Fence itself is not offered: the targets are Kerb and Old 12d.
    ASSERT_EQ(choice->count(), 2);
    choice->setCurrentIndex(choice->findText("Kerb"));
    click(dialog, "promptOk");
    katana::qt::test::processEvents();

    EXPECT_EQ(steps(), stepsBefore + 1);
    EXPECT_FALSE(document.model().styles.contains("Fence"));
    for (const auto id : fencePoints) {
        EXPECT_EQ(document.model().entities.find(id)->style, "Kerb");
    }
    EXPECT_EQ(dialog.selectedStyles(), std::vector<std::string>{"Kerb"}) << "the target";

    click(dialog, "undoButton");
    katana::qt::test::processEvents();
    EXPECT_TRUE(document.model().styles.contains("Fence"));
    for (const auto id : fencePoints) {
        EXPECT_EQ(document.model().entities.find(id)->style, "Fence");
    }
    EXPECT_EQ(steps(), stepsBefore);
}

TEST_F(Manager, AReloadAfterAnExternalCommandAndItsUndoKeepsTheSelectedRow)
{
    StyleManagerDialog dialog(context);
    dialog.selectStyles({"Kerb"});
    auto* table = child<QTableView>(dialog, "styleTable");
    const int rowsBefore = table->model()->rowCount();

    // A style made elsewhere - the command line, say - sorts before Kerb, so
    // Kerb's row index moves; the selection follows the NAME.
    Style early = style("Aaa", "DASHED");
    must(katana::commands::createStyle(early));
    EXPECT_EQ(table->model()->rowCount(), rowsBefore) << "not rebuilt inside the notification";
    katana::qt::test::processEvents();
    EXPECT_EQ(table->model()->rowCount(), rowsBefore + 1);
    EXPECT_EQ(dialog.selectedStyles(), std::vector<std::string>{"Kerb"});

    ASSERT_TRUE(document.undo().ok());
    katana::qt::test::processEvents();
    EXPECT_EQ(table->model()->rowCount(), rowsBefore);
    EXPECT_EQ(dialog.selectedStyles(), std::vector<std::string>{"Kerb"});
    EXPECT_EQ(picker(dialog, "styleLinetype")->currentName(), "TEST Dashed Kerb");
}

TEST_F(Manager, DeletingAStyleStillWornIsRefusedWithTheCountAndTheFirstHolder)
{
    StyleManagerDialog dialog(context);
    dialog.selectStyles({"Fence"});
    const std::size_t stepsBefore = steps();
    click(dialog, "styleDelete");
    katana::qt::test::processEvents();

    EXPECT_TRUE(document.model().styles.contains("Fence"));
    EXPECT_EQ(steps(), stepsBefore);
    // Worn by the two points; the first holder is the lower id.
    const std::string expected =
        "used by 2 entities, e.g. id=" + std::to_string(std::min(fencePoints[0], fencePoints[1]));
    const QString status = child<QLabel>(dialog, "statusLine")->text();
    EXPECT_TRUE(status.contains(QString::fromStdString(expected))) << status.toStdString();
    ASSERT_FALSE(messages.empty());
    EXPECT_EQ(messages.back(), status) << "the same sentence reaches the log";
}

TEST_F(Manager, PurgeListsWhatNothingUsesAndDeletesTheCheckedItemsAsOneStep)
{
    StyleManagerDialog dialog(context);
    const std::size_t stepsBefore = steps();
    click(dialog, "stylePurge");
    auto* panel = child<QWidget>(dialog, "purgePanel");
    ASSERT_FALSE(panel->isHidden());
    auto* list = child<QListWidget>(dialog, "purgeList");
    // Old 12d is worn by nothing; Kerb and Fence are worn, and DASHED is
    // named by Fence.
    std::vector<QString> items;
    for (int row = 0; row < list->count(); ++row) {
        items.push_back(list->item(row)->text());
    }
    EXPECT_NE(std::ranges::find(items, QString("Style  Old 12d")), items.end());
    EXPECT_EQ(std::ranges::find(items, QString("Style  Kerb")), items.end());
    EXPECT_EQ(std::ranges::find(items, QString("Linetype  DASHED")), items.end());

    click(dialog, "purgeOk");
    katana::qt::test::processEvents();
    EXPECT_FALSE(document.model().styles.contains("Old 12d"));
    EXPECT_EQ(steps(), stepsBefore + 1);
    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(stored("Old 12d").linetype, "Old 12d Kerb");
}

TEST_F(Manager, ApplyToSelectionUsesTheDrawingsSelectionAsItIsWhenPressed)
{
    StyleManagerDialog dialog(context);
    dialog.selectStyles({"Kerb"});
    auto* apply = child<QPushButton>(dialog, "styleApplyToSelection");
    EXPECT_FALSE(apply->isEnabled()) << "nothing is selected in the drawing yet";

    // Selected in the drawing AFTER the dialog opened: a non-modal manager
    // sits beside the drawing while the person picks.
    document.selection().set({fencePoints[0]});
    document.notifySelectionChanged();
    katana::qt::test::processEvents();
    ASSERT_TRUE(apply->isEnabled());
    EXPECT_EQ(apply->text(), "Apply to Selection (1)");
    apply->click();
    EXPECT_EQ(document.model().entities.find(fencePoints[0])->style, "Kerb");
    EXPECT_EQ(document.model().entities.find(fencePoints[1])->style, "Fence");
}

TEST_F(Manager, SelectUsersSelectsTheEntitiesWearingTheSelectedStyles)
{
    StyleManagerDialog dialog(context);
    dialog.selectStyles({"Kerb", "Fence"});
    click(dialog, "styleSelectUsers");
    std::vector<katana::entity::EntityId> expected = fencePoints;
    expected.push_back(kerbPoint);
    std::ranges::sort(expected);
    EXPECT_EQ(document.selection().ids(), expected);
}

TEST_F(Manager, MakeCurrentFeedsNewWorkAndMarksTheRowAndPressedAgainDrawsByLayer)
{
    StyleManagerDialog dialog(context);
    dialog.selectStyles({"Kerb"});
    click(dialog, "styleMakeCurrent");
    katana::qt::test::processEvents();
    EXPECT_EQ(document.currentStyle(), "Kerb");
    auto* table = child<QTableView>(dialog, "styleTable");
    const QModelIndex index = table->selectionModel()->selectedRows(0).front();
    EXPECT_EQ(index.data().toString(), "Kerb  (current)");
    EXPECT_EQ(child<QPushButton>(dialog, "styleMakeCurrent")->text(), "Draw ByLayer");

    click(dialog, "styleMakeCurrent");
    katana::qt::test::processEvents();
    EXPECT_EQ(document.currentStyle(), "");
}

TEST_F(Manager, AnInvalidPatternDisablesSaveAndAPatternEditedBackSavesNothing)
{
    StyleManagerDialog dialog(context);
    dialog.selectLinetype("DASHED", LinetypeOrigin::Drawing);
    auto* grid = child<QTableWidget>(dialog, "patternGrid");
    ASSERT_EQ(grid->rowCount(), 2);
    grid->setCurrentCell(0, 0);

    // A second dash straight after the first: dashes and gaps must alternate.
    click(dialog, "patternAddDash");
    ASSERT_EQ(grid->rowCount(), 3);
    EXPECT_FALSE(child<QPushButton>(dialog, "linetypeSave")->isEnabled());
    const QString invalid = child<QLabel>(dialog, "patternStatus")->text();
    EXPECT_FALSE(invalid.startsWith("Valid")) << invalid.toStdString();

    // Removed again: valid, and equal to what is stored.
    click(dialog, "patternRemove");
    ASSERT_EQ(grid->rowCount(), 2);
    EXPECT_TRUE(child<QLabel>(dialog, "patternStatus")->text().startsWith("Valid"));
    const std::size_t stepsBefore = steps();
    click(dialog, "linetypeSave");
    katana::qt::test::processEvents();
    EXPECT_EQ(steps(), stepsBefore) << "updateLinetypeIfChanged: no change, no step";
    EXPECT_EQ(document.model().linetypes.find("DASHED")->pattern.size(), 2U);
}

TEST_F(Manager, AGapLengthEditedInTheGridIsSavedAsOneStep)
{
    StyleManagerDialog dialog(context);
    dialog.selectLinetype("DASHED", LinetypeOrigin::Drawing);
    auto* gap = child<QDoubleSpinBox>(dialog, "patternLength1");
    ASSERT_NE(gap, nullptr);
    gap->setValue(0.25);
    const std::size_t stepsBefore = steps();
    click(dialog, "linetypeSave");
    katana::qt::test::processEvents();
    EXPECT_EQ(steps(), stepsBefore + 1);
    // The gap keeps its sign: a gap of 0.25 is stored as -0.25; the dash
    // nobody touched is still exactly 1.
    const auto& pattern = document.model().linetypes.find("DASHED")->pattern;
    ASSERT_EQ(pattern.size(), 2U);
    EXPECT_EQ(pattern[0].length, 1.0);
    EXPECT_EQ(pattern[1].length, -0.25);
}

TEST_F(Manager, ALibraryLinestyleIsReadOnlyAndMakesANewStyleUsingIt)
{
    StyleManagerDialog dialog(context);
    dialog.selectLinetype("TEST Dashed Kerb", LinetypeOrigin::Library);
    EXPECT_FALSE(child<QPushButton>(dialog, "linetypeDelete")->isEnabled());
    EXPECT_FALSE(child<QPushButton>(dialog, "linetypeRename")->isEnabled());
    EXPECT_TRUE(child<QLabel>(dialog, "libraryDetails")->text().contains("paperstyle"));

    click(dialog, "newStyleUsing");
    auto* name = child<QLineEdit>(dialog, "promptName");
    // The linestyle's own name is free among the styles.
    EXPECT_EQ(name->text(), "TEST Dashed Kerb");
    click(dialog, "promptOk");
    katana::qt::test::processEvents();
    ASSERT_TRUE(document.model().styles.contains("TEST Dashed Kerb"));
    EXPECT_EQ(stored("TEST Dashed Kerb").linetype, "TEST Dashed Kerb");
    EXPECT_EQ(dialog.selectedStyles(), std::vector<std::string>{"TEST Dashed Kerb"});
}

TEST_F(Manager, DiagnosticsListTheUndefinedNamesAndSelectTheirUsers)
{
    // Old 12d names three undefined names, but only its linetype and symbol
    // are resolved names (a hatch is not); nothing wears it, so give one
    // entity that style.
    must(katana::commands::setEntityStyle({kerbPoint}, "Old 12d"));
    StyleManagerDialog dialog(context);
    auto* table = child<QTableView>(dialog, "diagnosticTable");
    ASSERT_EQ(table->model()->rowCount(), 2);
    EXPECT_EQ(table->model()->index(0, 1).data().toString(), "Old 12d Kerb");
    EXPECT_EQ(table->model()->index(1, 1).data().toString(), "Old 12d Pit");
    table->selectRow(0);
    click(dialog, "diagnosticSelectUsers");
    EXPECT_EQ(document.selection().ids(), std::vector<katana::entity::EntityId>{kerbPoint});
}

TEST_F(Manager, TheDialogMayOutliveItsDocumentAndThenDoesNothing)
{
    auto doomed = std::make_unique<Document>();
    CustomisationContext own = context;
    own.document = doomed.get();
    StyleManagerDialog dialog(own);
    doomed.reset();
    katana::qt::test::processEvents();
    // Pressing Undo on a dead Document must neither crash nor act.
    auto* undo = child<QPushButton>(dialog, "undoButton");
    undo->click();
    SUCCEED();
}

TEST_F(Manager, TheChipsAndTheSearchFilterTheStylesAndTheChipsCountThem)
{
    StyleManagerDialog dialog(context);
    auto* page = child<QWidget>(dialog, "stylesPage");
    auto* table = child<QTableView>(dialog, "styleTable");
    ASSERT_EQ(table->model()->rowCount(), 3);
    // Used: Kerb (1 point) and Fence (2); unused: Old 12d; missing: Old 12d.
    auto* missing = child<QToolButton>(*page, "filterMissing");
    EXPECT_EQ(missing->text(), "Missing (1)");
    EXPECT_EQ(child<QToolButton>(*page, "filterUsed")->text(), "Used (2)");
    missing->click();
    ASSERT_EQ(table->model()->rowCount(), 1);
    EXPECT_EQ(table->model()->index(0, 0).data(katana::qt::kSortRole).toString(), "Old 12d");

    child<QToolButton>(*page, "filterAll")->click();
    // The search folds case, and looks in the linetype too: "dashed" is
    // Fence's DASHED and Kerb's TEST Dashed Kerb.
    child<QLineEdit>(*page, "filterText")->setText("dashed");
    EXPECT_EQ(table->model()->rowCount(), 2);
}

TEST_F(Manager, LoadingALibraryThatDefinesAMissingNameReloadsTheMarksOnce)
{
    StyleManagerDialog dialog(context);
    dialog.selectStyles({"Old 12d"});
    EXPECT_FALSE(picker(dialog, "styleLinetype")->currentIsDefined());

    // A customisation load elsewhere brings "Old 12d Kerb" as a linestyle.
    katana::entity::StyleLibrary library = document.styleLibrary();
    katana::entity::LineStyle kerb = *library.find("TEST Dashed Kerb");
    kerb.name = "Old 12d Kerb";
    ASSERT_TRUE(library.add(kerb).ok());
    document.setStyleLibrary(std::move(library));
    katana::qt::test::processEvents();

    EXPECT_EQ(dialog.selectedStyles(), std::vector<std::string>{"Old 12d"});
    EXPECT_TRUE(picker(dialog, "styleLinetype")->currentIsDefined());
    auto* page = child<QWidget>(dialog, "stylesPage");
    // Its symbol is still undefined, so the style is still "missing".
    EXPECT_EQ(child<QToolButton>(*page, "filterMissing")->text(), "Missing (1)");
    auto* diagnostics = child<QTableView>(dialog, "diagnosticTable");
    EXPECT_EQ(diagnostics->model()->rowCount(), 1) << "only the symbol is left";
}

TEST_F(Manager, AReloadCausedElsewhereKeepsAFormPartWayThroughAnEdit)
{
    StyleManagerDialog dialog(context);
    dialog.selectStyles({"Kerb"});
    auto* description = child<QLineEdit>(dialog, "styleDescription");
    typeText(description, "kerb and gutter");

    // Something else changes the drawing while the person is typing.
    must(katana::commands::createStyle(style("Zzz", "DASHED")));
    katana::qt::test::processEvents();
    EXPECT_EQ(description->text(), "kerb and gutter") << "the edit is not thrown away";

    click(dialog, "styleSave");
    katana::qt::test::processEvents();
    EXPECT_EQ(stored("Kerb").description, "kerb and gutter");
    EXPECT_EQ(stored("Kerb").linetype, "TEST Dashed Kerb");
}

TEST_F(Manager, RenamingAStyleThroughThePromptTakesItsEntitiesAndKeepsItSelected)
{
    StyleManagerDialog dialog(context);
    dialog.selectStyles({"Fence"});
    click(dialog, "styleRename");
    auto* name = child<QLineEdit>(dialog, "promptName");
    EXPECT_EQ(name->text(), "Fence") << "offered the current name to change";
    name->setText("  Timber Fence ");
    click(dialog, "promptOk");
    katana::qt::test::processEvents();

    EXPECT_FALSE(document.model().styles.contains("Fence"));
    ASSERT_TRUE(document.model().styles.contains("Timber Fence")) << "blanks either end dropped";
    for (const auto id : fencePoints) {
        EXPECT_EQ(document.model().entities.find(id)->style, "Timber Fence");
    }
    EXPECT_EQ(dialog.selectedStyles(), std::vector<std::string>{"Timber Fence"});
    EXPECT_TRUE(child<QWidget>(dialog, "promptPanel")->isHidden());
}

TEST_F(Manager, MergingDrawingLinetypesIsOneUndoStepAndRepointsTheirStyles)
{
    katana::entity::Linetype hidden;
    hidden.name = "HIDDEN";
    hidden.pattern = {{0.5}, {-0.25}};
    must(katana::commands::createLinetype(hidden));
    must(katana::commands::createStyle(style("Hidden Edge", "HIDDEN")));
    StyleManagerDialog dialog(context);
    dialog.selectLinetype("HIDDEN", LinetypeOrigin::Drawing);
    const std::size_t stepsBefore = steps();

    click(dialog, "linetypeMerge");
    auto* choice = child<QComboBox>(dialog, "promptChoice");
    // Drawing linetypes only: continuous and DASHED (a library linestyle
    // is no merge target - the commands cannot see the library).
    ASSERT_EQ(choice->count(), 2);
    choice->setCurrentIndex(choice->findText("DASHED"));
    click(dialog, "promptOk");
    katana::qt::test::processEvents();

    EXPECT_EQ(steps(), stepsBefore + 1);
    EXPECT_FALSE(document.model().linetypes.contains("HIDDEN"));
    EXPECT_EQ(stored("Hidden Edge").linetype, "DASHED");
    EXPECT_EQ(dialog.selectedLinetypes(),
              (std::vector<std::pair<std::string, LinetypeOrigin>>{
                  {"DASHED", LinetypeOrigin::Drawing}}));
}

TEST_F(Manager, ChangingAnElementToADotInTheGridDisablesItsLengthAndSavesADot)
{
    StyleManagerDialog dialog(context);
    dialog.selectLinetype("DASHED", LinetypeOrigin::Drawing);
    auto* kind = child<QComboBox>(dialog, "patternKind0");
    ASSERT_NE(kind, nullptr);
    // As a person's choice arrives: the index, then activated - the signal
    // the grid acts on, inside which it must not delete this very combo.
    kind->setCurrentIndex(2);
    Q_EMIT kind->activated(2);
    auto* length = child<QDoubleSpinBox>(dialog, "patternLength0");
    ASSERT_NE(length, nullptr);
    EXPECT_FALSE(length->isEnabled()) << "a dot has no length";
    EXPECT_EQ(length->value(), 0.0);

    click(dialog, "linetypeSave");
    katana::qt::test::processEvents();
    // {1, -0.5} with its dash made a dot: {0, -0.5}, a dot every half metre.
    const auto& pattern = document.model().linetypes.find("DASHED")->pattern;
    ASSERT_EQ(pattern.size(), 2U);
    EXPECT_EQ(pattern[0].length, 0.0);
    EXPECT_EQ(pattern[1].length, -0.5);
}
