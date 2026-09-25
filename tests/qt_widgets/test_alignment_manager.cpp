// Terrain > Alignment Manager (src/katana_qt/alignment_manager.hpp), driven by
// its object names as a person drives it by clicking and typing.
//
// The dialog changes nothing itself: it writes the ALIGN (or LABEL ALIGN) line
// a person would type and hands it to the window's executor. So what is tested
// here is each line - exactly the one a person would type - that the drawing
// then holds what the line says, that each edit is one undo step, and that
// what the dialog shows is what the model and the setting-out function hold.
// The executor here runs the line through the real CommandInterpreter on the
// same drawing, which is where the window's command line sends an ALIGN line;
// the headless qt_alignment_manager_headless runs it through the window.

#include <gtest/gtest.h>

#include <QApplication>
#include <QClipboard>
#include <QComboBox>
#include <QDir>
#include <QFile>
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>
#include <QTableWidget>
#include <QTemporaryDir>

#include <memory>
#include <string>
#include <vector>

#include "alignment_manager.hpp"
#include "command_runner.hpp"
#include "katana/cad/alignment_report.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/geometry/alignment.hpp"

using katana::cad::CommandInterpreter;
using katana::cad::Document;
using katana::geometry::Point2;
using katana::qt::AlignmentManagerContext;
using katana::qt::AlignmentManagerDialog;
using katana::qt::VerbOutcome;

namespace {

template <typename T> T* child(QWidget& parent, const char* name)
{
    T* found = parent.findChild<T*>(QString::fromLatin1(name));
    EXPECT_NE(found, nullptr) << "no " << name;
    return found;
}

void fill(QWidget& parent, const char* name, const QString& text)
{
    auto* field = child<QLineEdit>(parent, name);
    ASSERT_NE(field, nullptr);
    field->setText(text);
}

void click(QWidget& parent, const char* name)
{
    auto* button = child<QPushButton>(parent, name);
    ASSERT_NE(button, nullptr);
    ASSERT_TRUE(button->isEnabled()) << name << " is disabled";
    button->click();
    QApplication::processEvents();
}

// Types into a grid cell as a person commits an edit, then lets the event
// loop run what the dialog queued for it.
void edit(QWidget& parent, const char* table, int row, int column, const QString& text)
{
    auto* grid = child<QTableWidget>(parent, table);
    ASSERT_NE(grid, nullptr);
    ASSERT_NE(grid->item(row, column), nullptr) << table << " " << row << "," << column;
    grid->item(row, column)->setText(text);
    QApplication::processEvents();
    QApplication::processEvents();
}

QString cellText(QWidget& parent, const char* table, int row, int column)
{
    auto* grid = child<QTableWidget>(parent, table);
    if (grid == nullptr || grid->item(row, column) == nullptr) {
        ADD_FAILURE() << "no cell " << row << "," << column << " in " << table;
        return {};
    }
    return grid->item(row, column)->text();
}

QString status(QWidget& parent) { return child<QLabel>(parent, "alignmentStatus")->text(); }

// The window's executor, as far as an ALIGN line goes: the interpreter on the
// same drawing. It keeps every line it was handed.
struct Executor {
    explicit Executor(Document& document) : interpreter(document) {}

    CommandInterpreter interpreter;
    std::vector<QString> lines;

    AlignmentManagerContext context(Document& document, bool headless = false)
    {
        AlignmentManagerContext made;
        made.document = &document;
        made.run = [this](const QString& line) {
            lines.push_back(line);
            VerbOutcome outcome;
            const auto reply = interpreter.run(line.toStdString());
            outcome.ok = reply.ok();
            if (reply.ok()) {
                outcome.reply = QString::fromStdString(*reply);
            } else {
                outcome.error = QString::fromStdString(reply.error().describe());
            }
            return outcome;
        };
        made.headless = [headless] { return headless; };
        return made;
    }
    [[nodiscard]] QString last() const { return lines.empty() ? QString() : lines.back(); }
};

// A drawing with the corner (0,0) (100,0) (100,100) defined as "road".
struct Fixture {
    Fixture() : executor(document)
    {
        EXPECT_TRUE(executor.interpreter.run("ALIGN NEW road 0,0 100,0 100,100").ok());
        dialog = std::make_unique<AlignmentManagerDialog>(executor.context(document));
    }
    [[nodiscard]] const katana::entity::Alignment* road() const
    {
        return document.model().alignments.find("road");
    }

    Document document;
    Executor executor;
    std::unique_ptr<AlignmentManagerDialog> dialog;
};

} // namespace

TEST(AlignmentManager, NewFromTheSelectedPolylineRunsAlignNewWithItsVerticesAsOneStep)
{
    Document document;
    Executor executor(document);
    katana::geometry::Polyline2 line;
    line.vertices = {Point2(0, 0), Point2(100.25, 0), Point2(100.25, 80)};
    const auto made =
        document.execute(katana::commands::createPolyline(line, document.currentAttributes()));
    ASSERT_TRUE(made.ok());
    const katana::entity::EntityId id = document.model().entities.ids().back();
    document.selection().set({id});
    document.notifySelectionChanged();

    AlignmentManagerDialog dialog(executor.context(document));
    EXPECT_EQ(child<QTableWidget>(dialog, "alignmentList")->rowCount(), 0);
    // Nothing to act on yet: the buttons that need an alignment say so.
    EXPECT_FALSE(child<QPushButton>(dialog, "alignmentDelete")->isEnabled());

    fill(dialog, "alignmentName", "main road");
    click(dialog, "alignmentUseSelection");
    EXPECT_EQ(child<QLineEdit>(dialog, "alignmentPoints")->text().toStdString(),
              "0,0 100.25,0 100.25,80");
    const std::size_t before = document.history().undoCount();
    click(dialog, "alignmentNew");
    // A name with a blank is quoted, as a person would have to type it.
    EXPECT_EQ(executor.last().toStdString(), "ALIGN NEW \"main road\" 0,0 100.25,0 100.25,80");
    const auto* created = document.model().alignments.find("main road");
    ASSERT_NE(created, nullptr);
    EXPECT_EQ(created->horizontal.pis.size(), 3u);
    EXPECT_EQ(dialog.currentAlignment().toStdString(), "main road");
    EXPECT_EQ(document.history().undoCount(), before + 1);
    EXPECT_TRUE(child<QPushButton>(dialog, "alignmentDelete")->isEnabled());

    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(document.model().alignments.find("main road"), nullptr);
}

TEST(AlignmentManager, UseSelectionRefusesAnythingButOneLineOrPolyline)
{
    Document document;
    Executor executor(document);
    AlignmentManagerDialog dialog(executor.context(document));
    click(dialog, "alignmentUseSelection");
    EXPECT_TRUE(status(dialog).contains("Select one line or polyline")) << status(dialog).toStdString();
    EXPECT_TRUE(child<QLineEdit>(dialog, "alignmentPoints")->text().isEmpty());
    click(dialog, "alignmentNew");
    EXPECT_TRUE(executor.lines.empty()); // no name: nothing is run
}

TEST(AlignmentManager, TheListShowsEveryAlignmentAndReloadsWhenOneIsTypedElsewhere)
{
    Fixture f;
    auto* list = child<QTableWidget>(*f.dialog, "alignmentList");
    ASSERT_EQ(list->rowCount(), 1);
    EXPECT_EQ(cellText(*f.dialog, "alignmentList", 0, 0).toStdString(), "road");
    EXPECT_EQ(cellText(*f.dialog, "alignmentList", 0, 1).toStdString(), "3");
    EXPECT_EQ(cellText(*f.dialog, "alignmentList", 0, 4).toStdString(), "200.000");
    EXPECT_EQ(cellText(*f.dialog, "alignmentList", 0, 5).toStdString(), "none");
    EXPECT_EQ(f.dialog->currentAlignment().toStdString(), "road");

    // Typed on the command line, not in the dialog: the watcher reloads it
    // from the event loop, keeping the chosen alignment.
    ASSERT_TRUE(f.executor.interpreter.run("ALIGN NEW access 0,0 10,0").ok());
    QApplication::processEvents();
    ASSERT_EQ(list->rowCount(), 2);
    EXPECT_EQ(f.dialog->currentAlignment().toStdString(), "road");
    EXPECT_TRUE(f.dialog->selectAlignment("access"));
    EXPECT_EQ(child<QTableWidget>(*f.dialog, "alignmentPiTable")->rowCount(), 2);
    EXPECT_FALSE(f.dialog->selectAlignment("nosuch"));
}

TEST(AlignmentManager, AGridEditRunsNothingUntilApplyRunsOnePisLineAsOneUndoStep)
{
    Fixture f;
    auto* table = child<QTableWidget>(*f.dialog, "alignmentPiTable");
    ASSERT_EQ(table->rowCount(), 3);
    EXPECT_EQ(cellText(*f.dialog, "alignmentPiTable", 2, 0).toStdString(), "2");

    edit(*f.dialog, "alignmentPiTable", 1, 3, "50");
    // Nothing runs from the table's own signal.
    EXPECT_TRUE(f.executor.lines.empty());
    EXPECT_EQ(f.road()->horizontal.pis[1].radius, 0.0);

    const std::size_t before = f.document.history().undoCount();
    click(*f.dialog, "alignmentApplyPis");
    // A corner with no curve is written as x,y alone.
    EXPECT_EQ(f.executor.last().toStdString(), "ALIGN PIS road 0,0 100,0,50 100,100");
    EXPECT_EQ(f.road()->horizontal.pis[1].radius, 50.0);
    EXPECT_EQ(f.document.history().undoCount(), before + 1);
    // R = 50 round a right angle: 100 + 50 pi / 2 = 178.540 long.
    EXPECT_EQ(cellText(*f.dialog, "alignmentList", 0, 4).toStdString(), "178.540");

    ASSERT_TRUE(f.document.undo().ok());
    EXPECT_EQ(f.road()->horizontal.pis[1].radius, 0.0);
    QApplication::processEvents();
    EXPECT_EQ(cellText(*f.dialog, "alignmentPiTable", 1, 3).toStdString(), "0");
}

TEST(AlignmentManager, AnEditedEastingIsWrittenAndEveryOtherNumberGoesBackExactly)
{
    Document document;
    Executor executor(document);
    ASSERT_TRUE(executor.interpreter.run("ALIGN NEW road 0,0 100,0.123456789012 200,300").ok());
    AlignmentManagerDialog dialog(executor.context(document));
    EXPECT_EQ(cellText(dialog, "alignmentPiTable", 1, 2).toStdString(), "0.123456789012");
    edit(dialog, "alignmentPiTable", 1, 1, "101");
    click(dialog, "alignmentApplyPis");
    EXPECT_EQ(executor.last().toStdString(), "ALIGN PIS road 0,0 101,0.123456789012 200,300");
    const auto* road = document.model().alignments.find("road");
    ASSERT_NE(road, nullptr);
    EXPECT_EQ(road->horizontal.pis[1].point, Point2(101, 0.123456789012));
}

TEST(AlignmentManager, ARefusedApplyKeepsTheGridForCorrectingAndRevertPutsTheDrawingsBack)
{
    Fixture f;
    // A spiral cannot be negative (geometry::solveAlignment).
    edit(*f.dialog, "alignmentPiTable", 1, 4, "-5");
    click(*f.dialog, "alignmentApplyPis");
    EXPECT_EQ(f.executor.last().toStdString(), "ALIGN PIS road 0,0 100,0,0,-5 100,100");
    EXPECT_TRUE(status(*f.dialog).contains("spiral")) << status(*f.dialog).toStdString();
    EXPECT_EQ(f.road()->horizontal.pis[1].spiralIn, 0.0);
    EXPECT_EQ(cellText(*f.dialog, "alignmentPiTable", 1, 4).toStdString(), "-5");
    click(*f.dialog, "alignmentRevertPis");
    EXPECT_EQ(cellText(*f.dialog, "alignmentPiTable", 1, 4).toStdString(), "0");
}

TEST(AlignmentManager, RowsAreAddedAndRemovedInTheGridAndTheStartChainageHasItsOwnLine)
{
    Fixture f;
    auto* table = child<QTableWidget>(*f.dialog, "alignmentPiTable");
    click(*f.dialog, "alignmentAddPi");
    ASSERT_EQ(table->rowCount(), 4);
    EXPECT_EQ(cellText(*f.dialog, "alignmentPiTable", 3, 0).toStdString(), "3");
    table->item(3, 1)->setText("200");
    table->item(3, 2)->setText("100");
    click(*f.dialog, "alignmentApplyPis");
    EXPECT_EQ(f.executor.last().toStdString(), "ALIGN PIS road 0,0 100,0 100,100 200,100");
    ASSERT_EQ(f.road()->horizontal.pis.size(), 4u);

    table->setCurrentCell(1, 1);
    click(*f.dialog, "alignmentRemovePi");
    // The rows below are counted again, as the PIs will be.
    EXPECT_EQ(cellText(*f.dialog, "alignmentPiTable", 1, 0).toStdString(), "1");
    click(*f.dialog, "alignmentApplyPis");
    EXPECT_EQ(f.executor.last().toStdString(), "ALIGN PIS road 0,0 100,100 200,100");
    EXPECT_EQ(f.road()->horizontal.pis.size(), 3u);

    // A row with a northing and no easting is refused before anything runs.
    const std::size_t ran = f.executor.lines.size();
    click(*f.dialog, "alignmentAddPi");
    table->item(3, 2)->setText("5");
    click(*f.dialog, "alignmentApplyPis");
    EXPECT_EQ(f.executor.lines.size(), ran);
    EXPECT_EQ(status(*f.dialog).toStdString(), "PI 3 needs an easting and a northing.");
    click(*f.dialog, "alignmentRevertPis");

    fill(*f.dialog, "alignmentStartStation", "1000");
    click(*f.dialog, "alignmentStart");
    EXPECT_EQ(f.executor.last().toStdString(), "ALIGN START road 1000");
    EXPECT_EQ(f.road()->horizontal.startStation, 1000.0);
    EXPECT_EQ(cellText(*f.dialog, "alignmentList", 0, 2).toStdString(), "1000.000");
}

TEST(AlignmentManager, UnappliedPIEditsSurviveAReloadThatLeftThePIsAlone)
{
    Fixture f;
    edit(*f.dialog, "alignmentPiTable", 2, 2, "150");
    // The profile is not the PIs: a design typed elsewhere keeps the edit.
    ASSERT_TRUE(f.executor.interpreter.run("ALIGN DESIGN road 0,16 200,17").ok());
    QApplication::processEvents();
    EXPECT_EQ(cellText(*f.dialog, "alignmentPiTable", 2, 2).toStdString(), "150");
    // The PIs changed under it: the grid is the drawing's again.
    ASSERT_TRUE(f.executor.interpreter.run("ALIGN PI road 200,100").ok());
    QApplication::processEvents();
    ASSERT_EQ(child<QTableWidget>(*f.dialog, "alignmentPiTable")->rowCount(), 4);
    EXPECT_EQ(cellText(*f.dialog, "alignmentPiTable", 2, 2).toStdString(), "100");
}

TEST(AlignmentManager, TheProfileGridIsAppliedAsOneDesignLineAndItsElementsAreShown)
{
    Fixture f;
    auto* grid = child<QTableWidget>(*f.dialog, "alignmentPviTable");
    ASSERT_EQ(grid->rowCount(), 0);
    EXPECT_TRUE(child<QLabel>(*f.dialog, "alignmentHighLow")->text().startsWith("No design profile"));
    // The textbook sag: -3% into +2% over 100 m.
    for (const auto& row : {QStringList{"0", "16", ""}, QStringList{"100", "13", "100"},
                            QStringList{"300", "17", "0"}}) {
        click(*f.dialog, "alignmentAddPvi");
        const int at = grid->rowCount() - 1;
        for (int column = 0; column < 3; ++column) {
            grid->item(at, column)->setText(row[column]);
        }
    }
    const std::size_t before = f.document.history().undoCount();
    click(*f.dialog, "alignmentApplyProfile");
    EXPECT_EQ(f.executor.last().toStdString(), "ALIGN DESIGN road 0,16 100,13,100 300,17");
    ASSERT_TRUE(f.road()->vertical.has_value());
    EXPECT_EQ(f.road()->vertical->pvis.size(), 3u);
    EXPECT_EQ(f.document.history().undoCount(), before + 1);

    // Tangent to 50, the curve 50 to 150 with K = 100 / 5 = 20, tangent on;
    // the low point where the grade passes through level, 110 @ 13.6.
    auto* elements = child<QTableWidget>(*f.dialog, "alignmentProfileElements");
    ASSERT_EQ(elements->rowCount(), 3);
    EXPECT_EQ(cellText(*f.dialog, "alignmentProfileElements", 1, 0).toStdString(), "curve");
    EXPECT_EQ(cellText(*f.dialog, "alignmentProfileElements", 1, 3).toStdString(), "-3.000");
    EXPECT_EQ(cellText(*f.dialog, "alignmentProfileElements", 1, 4).toStdString(), "2.000");
    EXPECT_EQ(cellText(*f.dialog, "alignmentProfileElements", 1, 5).toStdString(), "20.000");
    EXPECT_TRUE(cellText(*f.dialog, "alignmentProfileElements", 0, 5).isEmpty());
    EXPECT_EQ(child<QLabel>(*f.dialog, "alignmentHighLow")->text().toStdString(),
              "Low point at chainage 110.000, level 13.600.");
    EXPECT_EQ(cellText(*f.dialog, "alignmentList", 0, 5).toStdString(), "3 PVIs");

    click(*f.dialog, "alignmentClearProfile");
    EXPECT_EQ(f.executor.last().toStdString(), "ALIGN CLEARPROFILE road");
    EXPECT_FALSE(f.road()->vertical.has_value());
    EXPECT_EQ(grid->rowCount(), 0);
}

TEST(AlignmentManager, AnIncompleteProfileRowIsRefusedBeforeAnythingRuns)
{
    Fixture f;
    auto* grid = child<QTableWidget>(*f.dialog, "alignmentPviTable");
    click(*f.dialog, "alignmentAddPvi");
    click(*f.dialog, "alignmentAddPvi");
    grid->item(0, 0)->setText("0");
    grid->item(0, 1)->setText("16");
    grid->item(1, 0)->setText("100");
    click(*f.dialog, "alignmentApplyProfile");
    EXPECT_TRUE(f.executor.lines.empty());
    EXPECT_EQ(status(*f.dialog).toStdString(), "Profile row 2 needs a chainage and a level.");
}

TEST(AlignmentManager, UnappliedProfileEditsSurviveAnotherEditsReload)
{
    Fixture f;
    ASSERT_TRUE(f.executor.interpreter.run("ALIGN DESIGN road 0,16 300,17").ok());
    QApplication::processEvents();
    auto* grid = child<QTableWidget>(*f.dialog, "alignmentPviTable");
    ASSERT_EQ(grid->rowCount(), 2);
    grid->item(1, 1)->setText("18");
    // A new start chainage reloads the dialog; the profile it did not
    // touch keeps the grid's unapplied level.
    fill(*f.dialog, "alignmentStartStation", "10");
    click(*f.dialog, "alignmentStart");
    ASSERT_EQ(f.road()->horizontal.startStation, 10.0);
    EXPECT_EQ(cellText(*f.dialog, "alignmentPviTable", 1, 1).toStdString(), "18");
    // A change to the profile itself refills the grid from the model.
    ASSERT_TRUE(f.executor.interpreter.run("ALIGN PVI road 400 20").ok());
    QApplication::processEvents();
    ASSERT_EQ(grid->rowCount(), 3);
    EXPECT_EQ(cellText(*f.dialog, "alignmentPviTable", 1, 1).toStdString(), "17");
}

TEST(AlignmentManager, TheSettingOutTableIsTheSettingOutFunctionsWithTheKeyStations)
{
    Fixture f;
    ASSERT_TRUE(f.executor.interpreter.run("ALIGN SET road 1 50").ok());
    QApplication::processEvents();
    auto* stations = child<QTableWidget>(*f.dialog, "alignmentStations");
    fill(*f.dialog, "alignmentInterval", "25");

    auto solved = katana::geometry::solveAlignment(f.road()->horizontal);
    ASSERT_TRUE(solved.ok());
    const auto rows = katana::cad::settingOutStations(*solved, 25.0);
    ASSERT_TRUE(rows.ok());
    ASSERT_EQ(stations->rowCount(), static_cast<int>(rows->size()));
    ASSERT_EQ(stations->rowCount(), 10);
    EXPECT_EQ(child<QLabel>(*f.dialog, "alignmentStationsNote")->text().toStdString(),
              "10 stations, 4 of them key stations.");
    // The TC at 50 is on the interval and is one row; the CT at 128.540 is
    // not, and is there all the same.
    EXPECT_EQ(cellText(*f.dialog, "alignmentStations", 2, 0).toStdString(), "50.000");
    EXPECT_EQ(cellText(*f.dialog, "alignmentStations", 2, 5).toStdString(), "TC");
    EXPECT_EQ(cellText(*f.dialog, "alignmentStations", 6, 0).toStdString(), "128.540");
    EXPECT_EQ(cellText(*f.dialog, "alignmentStations", 6, 5).toStdString(), "CT");
    EXPECT_EQ(cellText(*f.dialog, "alignmentStations", 0, 3).toStdString(), "90°00'00\"");
    EXPECT_EQ(cellText(*f.dialog, "alignmentStations", 3, 4).toStdString(), "50.000 L");
    EXPECT_EQ(cellText(*f.dialog, "alignmentStations", 1, 4).toStdString(), "straight");

    QTemporaryDir folder;
    ASSERT_TRUE(folder.isValid());
    const QString path = folder.filePath("road.csv");
    ASSERT_TRUE(f.dialog->saveStationsCsv(path).ok());
    QFile file(path);
    ASSERT_TRUE(file.open(QIODevice::ReadOnly));
    EXPECT_EQ(file.readAll().toStdString(), katana::cad::settingOutCsv(*rows));

    click(*f.dialog, "alignmentStationsCopy");
    const QString copied = QApplication::clipboard()->text();
    EXPECT_TRUE(copied.startsWith("Chainage\tEasting\tNorthing\tAzimuth\tRadius\tKey\n"))
        << copied.toStdString();
    EXPECT_EQ(copied.count('\n'), 11);
}

TEST(AlignmentManager, AnIntervalThatCannotBeUsedEmptiesTheTableAndSaysWhy)
{
    Fixture f;
    fill(*f.dialog, "alignmentInterval", "0");
    EXPECT_EQ(child<QTableWidget>(*f.dialog, "alignmentStations")->rowCount(), 0);
    const QString note = child<QLabel>(*f.dialog, "alignmentStationsNote")->text();
    EXPECT_TRUE(note.contains("interval must be positive")) << note.toStdString();
    // The line's own status is not where a half-typed interval is told off.
    EXPECT_TRUE(status(*f.dialog).isEmpty()) << status(*f.dialog).toStdString();
    EXPECT_FALSE(f.dialog->saveStationsCsv(QDir::temp().filePath("never.csv")).ok());
}

TEST(AlignmentManager, SaveCsvInAHeadlessSessionOpensNoFileDialog)
{
    Document document;
    Executor executor(document);
    ASSERT_TRUE(executor.interpreter.run("ALIGN NEW road 0,0 100,0").ok());
    AlignmentManagerDialog dialog(executor.context(document, true));
    click(dialog, "alignmentStationsCsv");
    EXPECT_TRUE(status(dialog).contains("headless")) << status(dialog).toStdString();
}

TEST(AlignmentManager, ChainageLabelsNeedAChainageStyleAndAreOneStep)
{
    Fixture f;
    auto* styles = child<QComboBox>(*f.dialog, "alignmentLabelStyle");
    EXPECT_EQ(styles->count(), 0);
    click(*f.dialog, "alignmentLabelChainages");
    EXPECT_TRUE(f.executor.lines.empty());
    EXPECT_TRUE(status(*f.dialog).contains("LABELSTYLE DEFAULTS")) << status(*f.dialog).toStdString();

    ASSERT_TRUE(f.executor.interpreter.run("LABELSTYLE DEFAULTS").ok());
    QApplication::processEvents();
    // Only the styles that label chainages are offered.
    ASSERT_EQ(styles->count(), 1);
    EXPECT_EQ(styles->itemText(0).toStdString(), "Chainage");

    ASSERT_TRUE(f.executor.interpreter.run("LAYER NEW chainage").ok());
    const std::size_t entities = f.document.model().entities.size();
    const std::size_t before = f.document.history().undoCount();
    fill(*f.dialog, "alignmentLabelLayer", "chainage");
    click(*f.dialog, "alignmentLabelChainages");
    EXPECT_EQ(f.executor.last().toStdString(),
              "LABEL ALIGN road style=Chainage layer=chainage");
    EXPECT_GT(f.document.model().entities.size(), entities);
    EXPECT_EQ(f.document.history().undoCount(), before + 1);
}

TEST(AlignmentManager, DeleteRunsAlignDeleteAndLeavesNothingToEdit)
{
    Fixture f;
    click(*f.dialog, "alignmentDelete");
    EXPECT_EQ(f.executor.last().toStdString(), "ALIGN DELETE road");
    EXPECT_EQ(f.road(), nullptr);
    EXPECT_EQ(child<QTableWidget>(*f.dialog, "alignmentList")->rowCount(), 0);
    EXPECT_EQ(child<QTableWidget>(*f.dialog, "alignmentPiTable")->rowCount(), 0);
    EXPECT_FALSE(child<QPushButton>(*f.dialog, "alignmentApplyPis")->isEnabled());
    EXPECT_FALSE(child<QPushButton>(*f.dialog, "alignmentLabelChainages")->isEnabled());
}

TEST(AlignmentManager, ADialogWithNoExecutorSaysSoAndChangesNothing)
{
    Document document;
    CommandInterpreter interpreter(document);
    ASSERT_TRUE(interpreter.run("ALIGN NEW road 0,0 100,0").ok());
    AlignmentManagerContext context;
    context.document = &document;
    AlignmentManagerDialog dialog(context);
    click(dialog, "alignmentDelete");
    EXPECT_TRUE(status(dialog).startsWith("Nothing here can run")) << status(dialog).toStdString();
    EXPECT_NE(document.model().alignments.find("road"), nullptr);
}

TEST(AlignmentManager, ADrawingThatGoesFirstLeavesTheDialogInert)
{
    auto document = std::make_unique<Document>();
    std::vector<QString> lines;
    AlignmentManagerContext context;
    context.document = document.get();
    context.run = [&lines](const QString& line) {
        lines.push_back(line);
        return VerbOutcome{true, {}, {}};
    };
    {
        CommandInterpreter interpreter(*document);
        ASSERT_TRUE(interpreter.run("ALIGN NEW road 0,0 100,0").ok());
    }
    AlignmentManagerDialog dialog(context);
    document.reset();
    QApplication::processEvents();
    click(dialog, "alignmentDelete");
    EXPECT_TRUE(lines.empty());
    EXPECT_TRUE(status(dialog).contains("gone")) << status(dialog).toStdString();
}
