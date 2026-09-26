// Survey > Subsurface Utilities (AS 5488): the dialog (src/katana_qt/survey/
// utility_dialog.hpp) and its workbench (utility_workbench.hpp), driven by
// their object names as a person drives them by clicking.
//
// The dialog runs nothing of the AS 5488 library: it writes the UTILITY line
// its fields describe and hands it to an executor. So what is tested here is
// that line - exactly the one a person would type, for every tab and for both
// sources, a schedule file or what is drawn by the shared scope and filter
// controls - that a line which cannot be written runs nothing and says why,
// that the executor's reply is what the dialog shows, and that the workbench
// opens the dialog on the tab of the menu item chosen and frames a DRAW's or a
// REGRADE's bounds in the plan views.
// The executors and the interpreter are the tests' own, so nothing here needs
// the verb itself: the headless qt_utility_dialog_headless runs the real one.

#include <gtest/gtest.h>

#include <QAbstractButton>
#include <QAction>
#include <QApplication>
#include <QCheckBox>
#include <QClipboard>
#include <QComboBox>
#include <QDialog>
#include <QFile>
#include <QKeySequence>
#include <QLabel>
#include <QLineEdit>
#include <QListWidget>
#include <QMainWindow>
#include <QMenu>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QTabWidget>
#include <QTemporaryDir>

#include <memory>
#include <string>
#include <vector>

#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/tables.hpp"
#include "survey/utility_dialog.hpp"
#include "survey/utility_workbench.hpp"
#include "view_workspace.hpp"
#include "widget_harness.hpp"

namespace {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::qt::Icon;
using katana::qt::UtilityDesignSource;
using katana::qt::UtilityDialogContext;
using katana::qt::UtilityForm;
using katana::qt::UtilityScopeView;
using katana::qt::UtilityServices;
using katana::qt::UtilitySource;
using katana::qt::UtilityTool;
using katana::qt::UtilityToolsDialog;
using katana::qt::UtilityWorkbench;
using katana::qt::utilityCommandLine;
using katana::qt::utilitySourceOf;

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

QString text(QWidget& parent, const char* name)
{
    if (auto* label = parent.findChild<QLabel*>(QString::fromLatin1(name))) {
        return label->text();
    }
    if (auto* line = parent.findChild<QLineEdit*>(QString::fromLatin1(name))) {
        return line->text();
    }
    if (auto* box = parent.findChild<QPlainTextEdit*>(QString::fromLatin1(name))) {
        return box->toPlainText();
    }
    ADD_FAILURE() << "no text widget " << name;
    return {};
}

// A DRAW reply as the contract writes it: the summary record, then a record
// per line.
const std::string kDrawReply =
    "utilities drawn lines=2 vertices=5 segments=3 entities=9 layers=5 "
    "bounds=334000.000,6250000.000,334040.000,6250007.200\n"
    "line id=W1 type=water length=30.024 ql_a=1.420 ql_b=16.102 ql_c=12.502 ql_d=0.000\n"
    "line id=G1 type=gas length=40.000 ql_a=0.000 ql_b=0.000 ql_c=0.000 ql_d=40.000\n";

// What a dialog handed its executor, and what the executor answers.
struct Executor {
    std::vector<QString> lines;
    Result<std::string> reply = std::string("done\n");

    UtilityDialogContext context(bool headless = false)
    {
        UtilityDialogContext made;
        made.execute = [this](const QString& line) {
            lines.push_back(line);
            return reply;
        };
        made.headless = [headless] { return headless; };
        return made;
    }
};

UtilityForm formFor(UtilityTool tool)
{
    UtilityForm form;
    form.tool = tool;
    form.schedule = "C:/survey/schedule.csv";
    return form;
}

} // namespace

TEST(UtilityDialog, EveryControlHasItsObjectName)
{
    Executor executor;
    UtilityToolsDialog dialog(executor.context());
    EXPECT_EQ(dialog.objectName(), "utilityDialog");
    EXPECT_FALSE(dialog.isModal());
    for (const char* name :
         {"utilityTabs", "utilitySchedule", "utilityScheduleBrowse", "utilityDesign",
          "utilityDesignBrowse", "utilitySchema", "utilitySchemaBrowse", "utilityMinCover",
          "utilitySpacing", "utilityWidth", "utilityH", "utilityV", "utilityMargin",
          "utilityLayerPrefix", "utilityCommand", "utilityRun", "utilityOutput", "utilityCopy",
          "utilitySave", "utilityStatus", "utilityClose",
          // Where the services come from, and the drawing's scope and filter.
          "utilitySourceFile", "utilitySourceDrawing", "utilityScope", "utilityScopeSelection",
          "utilityScopeView", "utilityView", "utilityOnScreen", "utilityScopeLayers",
          "utilityLayers", "utilitySublayers", "utilityScopeDrawing", "utilityTypePoint",
          "utilityFilterLayer", "utilityFilterProperty", "utilityFilterValue", "utilityDrawnOnly",
          // Clearance's works, and the Schedule tab.
          "utilityDesignFile", "utilityDesignEntity", "utilityDesignEntityId",
          "utilityDesignUseSelected", "utilityDesignLevel", "utilityDesignAlignment",
          "utilityDesignAlignmentName", "utilityScheduleOut", "utilityScheduleOutBrowse",
          "utilityScheduleSchema", "utilityScheduleSchemaBrowse",
          // Draw's fields for geometry in the drawing.
          "utilityGeometryGroup", "utilityServiceType", "utilityMethod", "utilityHUnc",
          "utilityVUnc", "utilityHeights", "utilityLevelRef", "utilityPath", "utilityOwner",
          "utilityMaterial", "utilityDiameter", "utilityServiceStatus", "utilityFields"}) {
        EXPECT_NE(dialog.findChild<QWidget*>(name), nullptr) << name;
    }
    auto* tabs = child<QTabWidget>(dialog, "utilityTabs");
    // Seven since the tools act on what is drawn: Regrade and Schedule joined
    // the five (docs/subsurface_utilities.md, "Drawing data").
    ASSERT_EQ(tabs->count(), 7);
    EXPECT_EQ(tabs->tabText(0), "Draw");
    EXPECT_EQ(tabs->tabText(1), "Report");
    EXPECT_EQ(tabs->tabText(2), "Verify");
    EXPECT_EQ(tabs->tabText(3), "Clearance");
    EXPECT_EQ(tabs->tabText(4), "Check");
    EXPECT_EQ(tabs->tabText(5), "Regrade");
    EXPECT_EQ(tabs->tabText(6), "Schedule");
    // What is shown is not edited: the line and the reply are read-only.
    EXPECT_TRUE(child<QLineEdit>(dialog, "utilityCommand")->isReadOnly());
    EXPECT_TRUE(child<QPlainTextEdit>(dialog, "utilityOutput")->isReadOnly());
}

TEST(UtilityDialog, EachTabWritesTheLineTheVerbReads)
{
    UtilityForm draw = formFor(UtilityTool::Draw);
    draw.spacing = "8";
    draw.minCover = "0.6";
    draw.layerPrefix = "services/existing";
    EXPECT_EQ(utilityCommandLine(draw).valueOr({}),
              "UTILITY DRAW C:/survey/schedule.csv SPACING 8 MINCOVER 0.6 LAYER services/existing");

    UtilityForm report = formFor(UtilityTool::Report);
    report.minCover = "0.6";
    report.spacing = "12.5";
    EXPECT_EQ(utilityCommandLine(report).valueOr({}),
              "UTILITY REPORT C:/survey/schedule.csv MINCOVER 0.6 SPACING 12.5");

    EXPECT_EQ(utilityCommandLine(formFor(UtilityTool::Verify)).valueOr({}),
              "UTILITY VERIFY C:/survey/schedule.csv");

    UtilityForm clearance = formFor(UtilityTool::Clearance);
    clearance.design = "C:/survey/design.csv";
    clearance.width = "0.375";
    clearance.horizontal = "0.5";
    clearance.vertical = "0.3";
    clearance.margin = "2";
    EXPECT_EQ(utilityCommandLine(clearance).valueOr({}),
              "UTILITY CLEARANCE C:/survey/schedule.csv C:/survey/design.csv WIDTH 0.375 H 0.5 "
              "V 0.3 MARGIN 2");

    UtilityForm check = formFor(UtilityTool::Check);
    check.schema = "C:/survey/schema.csv";
    EXPECT_EQ(utilityCommandLine(check).valueOr({}),
              "UTILITY CHECK C:/survey/schedule.csv SCHEMA C:/survey/schema.csv");
}

TEST(UtilityDialog, APathWithBlanksIsQuotedAndABlankOptionIsLeftOut)
{
    UtilityForm clearance;
    clearance.tool = UtilityTool::Clearance;
    clearance.schedule = "  C:/Site Survey/schedule.csv ";
    clearance.design = "C:/Design Team/works.csv";
    clearance.vertical = " 0.45 ";
    EXPECT_EQ(utilityCommandLine(clearance).valueOr({}),
              "UTILITY CLEARANCE \"C:/Site Survey/schedule.csv\" \"C:/Design Team/works.csv\" V "
              "0.45");

    UtilityForm draw;
    draw.tool = UtilityTool::Draw;
    draw.schedule = "C:/Site Survey/schedule.csv";
    draw.layerPrefix = "existing services";
    EXPECT_EQ(utilityCommandLine(draw).valueOr({}),
              "UTILITY DRAW \"C:/Site Survey/schedule.csv\" LAYER \"existing services\"");

    UtilityForm check;
    check.tool = UtilityTool::Check;
    check.schedule = "C:/survey/schedule.csv";
    check.schema = "C:/Client Files/schema.csv";
    EXPECT_EQ(utilityCommandLine(check).valueOr({}),
              "UTILITY CHECK C:/survey/schedule.csv SCHEMA \"C:/Client Files/schema.csv\"");
}

TEST(UtilityDialog, OnlyTheChosenToolsFieldsAreRead)
{
    // Numbers typed on other tabs, even ones that do not read, do not stop
    // the tool that is chosen.
    UtilityForm verify = formFor(UtilityTool::Verify);
    verify.minCover = "deep";
    verify.width = "wide";
    verify.spacing = "far";
    verify.design = "C:/survey/design.csv";
    EXPECT_EQ(utilityCommandLine(verify).valueOr({}), "UTILITY VERIFY C:/survey/schedule.csv");
}

TEST(UtilityDialog, AFileLeftEmptyOrANumberThatDoesNotReadWritesNoLine)
{
    UtilityForm blank;
    blank.tool = UtilityTool::Report;
    blank.schedule = "   ";
    auto line = utilityCommandLine(blank);
    ASSERT_FALSE(line.ok());
    EXPECT_EQ(line.error().code, ErrorCode::InvalidArgument);
    EXPECT_NE(line.error().message.find("utility schedule"), std::string::npos);

    line = utilityCommandLine(formFor(UtilityTool::Clearance));
    ASSERT_FALSE(line.ok());
    EXPECT_NE(line.error().message.find("design"), std::string::npos);

    line = utilityCommandLine(formFor(UtilityTool::Check));
    ASSERT_FALSE(line.ok());
    EXPECT_NE(line.error().message.find("delivery schema"), std::string::npos);

    UtilityForm comma = formFor(UtilityTool::Report);
    comma.minCover = "0,6";
    line = utilityCommandLine(comma);
    ASSERT_FALSE(line.ok());
    EXPECT_NE(line.error().message.find("minimum cover must be a number of metres, not '0,6'"),
              std::string::npos)
        << line.error().message;

    UtilityForm words = formFor(UtilityTool::Draw);
    words.spacing = "ten";
    line = utilityCommandLine(words);
    ASSERT_FALSE(line.ok());
    EXPECT_NE(line.error().message.find("detected spacing"), std::string::npos);

    UtilityForm shallow = formFor(UtilityTool::Draw);
    shallow.minCover = "0.6 m";
    line = utilityCommandLine(shallow);
    ASSERT_FALSE(line.ok());
    EXPECT_NE(line.error().message.find("minimum cover must be a number of metres, not '0.6 m'"),
              std::string::npos)
        << line.error().message;

    UtilityForm quote = formFor(UtilityTool::Draw);
    quote.layerPrefix = "say \"when\"";
    line = utilityCommandLine(quote);
    ASSERT_FALSE(line.ok());
    EXPECT_NE(line.error().message.find("double quote"), std::string::npos);
}

TEST(UtilityDialog, TheCommandFieldShowsTheLineAsItWillRun)
{
    Executor executor;
    UtilityToolsDialog dialog(executor.context());
    dialog.showTool(UtilityTool::Report);
    auto* command = child<QLineEdit>(dialog, "utilityCommand");
    // Nothing to run yet, and the field says why rather than showing a line.
    EXPECT_TRUE(command->text().isEmpty());
    EXPECT_TRUE(command->placeholderText().contains("utility schedule"));

    fill(dialog, "utilitySchedule", "C:/survey/schedule.csv");
    fill(dialog, "utilityMinCover", "0.6");
    EXPECT_EQ(command->text(), "UTILITY REPORT C:/survey/schedule.csv MINCOVER 0.6");

    // The tab is part of the line: the same schedule, another tool.
    dialog.showTool(UtilityTool::Verify);
    EXPECT_EQ(command->text(), "UTILITY VERIFY C:/survey/schedule.csv");
    // The detected spacing and the minimum cover grade, so they are live only
    // where something is graded.
    EXPECT_FALSE(child<QLineEdit>(dialog, "utilitySpacing")->isEnabled());
    EXPECT_FALSE(child<QLineEdit>(dialog, "utilityMinCover")->isEnabled());
    dialog.showTool(UtilityTool::Draw);
    EXPECT_TRUE(child<QLineEdit>(dialog, "utilitySpacing")->isEnabled());
    EXPECT_TRUE(child<QLineEdit>(dialog, "utilityMinCover")->isEnabled());
    // The minimum cover given for the report is the draw's too: the verb
    // flags each drawn point against it.
    fill(dialog, "utilityLayerPrefix", "services");
    EXPECT_EQ(command->text(), "UTILITY DRAW C:/survey/schedule.csv MINCOVER 0.6 LAYER services");
}

TEST(UtilityDialog, BadInputRunsNothingAndSaysWhy)
{
    Executor executor;
    UtilityToolsDialog dialog(executor.context());
    dialog.showTool(UtilityTool::Draw);
    click(dialog, "utilityRun");
    EXPECT_TRUE(executor.lines.empty());
    EXPECT_TRUE(text(dialog, "utilityStatus").contains("utility schedule"))
        << text(dialog, "utilityStatus").toStdString();

    fill(dialog, "utilitySchedule", "C:/survey/schedule.csv");
    fill(dialog, "utilitySpacing", "10 m");
    click(dialog, "utilityRun");
    EXPECT_TRUE(executor.lines.empty());
    EXPECT_TRUE(text(dialog, "utilityStatus").contains("not '10 m'"));

    fill(dialog, "utilitySpacing", "");
    dialog.showTool(UtilityTool::Clearance);
    click(dialog, "utilityRun");
    EXPECT_TRUE(executor.lines.empty());
    EXPECT_TRUE(text(dialog, "utilityStatus").contains("design"));

    dialog.showTool(UtilityTool::Check);
    click(dialog, "utilityRun");
    EXPECT_TRUE(executor.lines.empty());
    EXPECT_TRUE(text(dialog, "utilityStatus").contains("delivery schema"));
    // Nothing ran, so there is nothing to copy or save.
    EXPECT_FALSE(child<QPushButton>(dialog, "utilityCopy")->isEnabled());
    EXPECT_FALSE(child<QPushButton>(dialog, "utilitySave")->isEnabled());
}

TEST(UtilityDialog, RunHandsTheLineToTheExecutorAndShowsItsReply)
{
    Executor executor;
    executor.reply = kDrawReply;
    UtilityToolsDialog dialog(executor.context());
    dialog.showTool(UtilityTool::Draw);
    fill(dialog, "utilitySchedule", "C:/Site Survey/schedule.csv");
    fill(dialog, "utilitySpacing", "12");
    click(dialog, "utilityRun");

    ASSERT_EQ(executor.lines.size(), 1u);
    EXPECT_EQ(executor.lines.front(), "UTILITY DRAW \"C:/Site Survey/schedule.csv\" SPACING 12");
    // The reply whole, without the newline that ends it.
    EXPECT_EQ(text(dialog, "utilityOutput").toStdString() + "\n", kDrawReply);
    EXPECT_TRUE(text(dialog, "utilityStatus").contains("one undo step"));

    click(dialog, "utilityCopy");
    EXPECT_EQ(QApplication::clipboard()->text(), text(dialog, "utilityOutput"));
    EXPECT_TRUE(child<QPushButton>(dialog, "utilitySave")->isEnabled());
    QTemporaryDir folder;
    ASSERT_TRUE(folder.isValid());
    const QString path = folder.filePath("drawn.txt");
    ASSERT_TRUE(dialog.saveOutputTo(path).ok());
    QFile saved(path);
    ASSERT_TRUE(saved.open(QIODevice::ReadOnly));
    EXPECT_EQ(saved.readAll().toStdString(), kDrawReply);
}

TEST(UtilityDialog, AFailedLineShowsWhyInTheOutputAndItsFirstLineInTheStatus)
{
    Executor executor;
    executor.reply = makeError(ErrorCode::InvalidArgument,
                               "7 rows: 2 errors\n  AssetStatus \"In service\" is not listed");
    UtilityToolsDialog dialog(executor.context());
    dialog.showTool(UtilityTool::Check);
    fill(dialog, "utilitySchedule", "C:/survey/schedule.csv");
    fill(dialog, "utilitySchema", "C:/survey/schema.csv");
    click(dialog, "utilityRun");

    ASSERT_EQ(executor.lines.size(), 1u);
    EXPECT_EQ(executor.lines.front(),
              "UTILITY CHECK C:/survey/schedule.csv SCHEMA C:/survey/schema.csv");
    EXPECT_EQ(text(dialog, "utilityOutput"),
              "InvalidArgument: 7 rows: 2 errors\n  AssetStatus \"In service\" is not listed");
    EXPECT_EQ(text(dialog, "utilityStatus"), "7 rows: 2 errors");
    // What failed is still a report worth keeping.
    EXPECT_TRUE(child<QPushButton>(dialog, "utilityCopy")->isEnabled());
}

TEST(UtilityDialog, SaveAsNamesTheToolWhoseReplyIsShownNotTheTabInFront)
{
    Executor executor;
    executor.reply = kDrawReply;
    UtilityToolsDialog dialog(executor.context());
    dialog.showTool(UtilityTool::Report);
    EXPECT_EQ(dialog.suggestedFileName().toStdString(), "utility_report.txt");
    dialog.showTool(UtilityTool::Draw);
    fill(dialog, "utilitySchedule", "C:/survey/schedule.csv");
    click(dialog, "utilityRun");
    EXPECT_EQ(dialog.suggestedFileName().toStdString(), "utility_draw.txt");
    // The records stay while the next run is prepared on another tab, and
    // they are still the draw's.
    dialog.showTool(UtilityTool::Report);
    EXPECT_EQ(text(dialog, "utilityOutput").toStdString() + "\n", kDrawReply);
    EXPECT_EQ(dialog.suggestedFileName().toStdString(), "utility_draw.txt");
    // A check that failed is still the check's report.
    executor.reply = makeError(ErrorCode::InvalidArgument, "7 rows: 2 errors");
    dialog.showTool(UtilityTool::Check);
    fill(dialog, "utilitySchema", "C:/survey/schema.csv");
    click(dialog, "utilityRun");
    dialog.showTool(UtilityTool::Verify);
    EXPECT_EQ(dialog.suggestedFileName().toStdString(), "utility_check.txt");
}

TEST(UtilityDialog, AHeadlessSessionOpensNoFileDialog)
{
    Executor executor;
    UtilityToolsDialog dialog(executor.context(true));
    // Were a file dialog opened here, the test would wait on it forever.
    click(dialog, "utilityScheduleBrowse");
    EXPECT_TRUE(text(dialog, "utilityStatus").contains("fill utilitySchedule"));
    click(dialog, "utilitySchemaBrowse");
    EXPECT_TRUE(text(dialog, "utilityStatus").contains("fill utilitySchema"));
}

// ---- what is drawn as the source (docs/cad.md, "Scope and filter") --------------------------

namespace {

UtilityForm drawn(UtilityTool tool, const char* scope)
{
    UtilityForm form;
    form.tool = tool;
    form.source = UtilitySource::Drawing;
    form.scope = scope;
    return form;
}

std::string lineOf(const UtilityForm& form)
{
    const auto line = utilityCommandLine(form);
    EXPECT_TRUE(line.ok()) << (line.ok() ? "" : line.error().describe());
    return line.ok() ? line->toStdString() : std::string{};
}

std::string refusalOf(const UtilityForm& form)
{
    const auto line = utilityCommandLine(form);
    EXPECT_FALSE(line.ok()) << "written as " << (line.ok() ? line->toStdString() : "");
    if (line.ok()) {
        return {};
    }
    EXPECT_EQ(line.error().code, ErrorCode::InvalidArgument);
    return line.error().message;
}

bool has(const std::string& text, const std::string& part)
{
    return text.find(part) != std::string::npos;
}

} // namespace

TEST(UtilityDialog, OnWhatIsDrawnEachTabWritesTheScopeWordsWhereTheScheduleWas)
{
    UtilityForm report = drawn(UtilityTool::Report, "LAYERS utilities/water WHERE TYPE=point");
    report.minCover = "0.6";
    report.spacing = "12.5";
    EXPECT_EQ(lineOf(report), "UTILITY REPORT LAYERS utilities/water WHERE TYPE=point MINCOVER "
                              "0.6 SPACING 12.5");
    // The schedule's path is not read when the drawing is the source.
    report.schedule = "C:/survey/schedule.csv";
    EXPECT_EQ(lineOf(report), "UTILITY REPORT LAYERS utilities/water WHERE TYPE=point MINCOVER "
                              "0.6 SPACING 12.5");
    EXPECT_EQ(lineOf(drawn(UtilityTool::Verify, "VIEW 3")), "UTILITY VERIFY VIEW 3");
    UtilityForm check = drawn(UtilityTool::Check, "SELECTION");
    check.schema = "C:/Client Files/schema.csv";
    EXPECT_EQ(lineOf(check), "UTILITY CHECK SELECTION SCHEMA \"C:/Client Files/schema.csv\"");
}

TEST(UtilityDialog, ClearanceMeasuresAgainstADesignFileADrawnLineOrAnAlignment)
{
    // A schedule against a design file: the positional line it always was.
    UtilityForm file = formFor(UtilityTool::Clearance);
    file.design = "C:/survey/design.csv";
    EXPECT_EQ(lineOf(file), "UTILITY CLEARANCE C:/survey/schedule.csv C:/survey/design.csv");
    // On what is drawn the works follow DESIGN, which also ends the filter.
    UtilityForm drawnFile = drawn(UtilityTool::Clearance, "DRAWING WHERE PROP=utility.type:water");
    drawnFile.design = "C:/Design Team/works.csv";
    drawnFile.width = "0.375";
    EXPECT_EQ(lineOf(drawnFile), "UTILITY CLEARANCE DRAWING WHERE PROP=utility.type:water DESIGN "
                                 "\"C:/Design Team/works.csv\" WIDTH 0.375");

    // A drawn line or polyline by its id, typed with the hash or without, at
    // a level - or at its own heights when none is given.
    UtilityForm entity = formFor(UtilityTool::Clearance);
    entity.designSource = UtilityDesignSource::Entity;
    entity.designEntity = "42";
    entity.designLevel = "18.25";
    entity.horizontal = "0.5";
    EXPECT_EQ(lineOf(entity),
              "UTILITY CLEARANCE C:/survey/schedule.csv DESIGN #42 LEVEL 18.25 H 0.5");
    entity.designEntity = " #42 ";
    entity.designLevel = "";
    EXPECT_EQ(lineOf(entity), "UTILITY CLEARANCE C:/survey/schedule.csv DESIGN #42 H 0.5");
    // A level below the datum is a level.
    entity.designLevel = "-2.5";
    EXPECT_EQ(lineOf(entity),
              "UTILITY CLEARANCE C:/survey/schedule.csv DESIGN #42 LEVEL -2.5 H 0.5");

    // An alignment by its name, quoted when it holds a blank.
    UtilityForm alignment = drawn(UtilityTool::Clearance, "VIEW 1 EXTENTS");
    alignment.designSource = UtilityDesignSource::Alignment;
    alignment.alignment = "Main Road";
    alignment.margin = "2";
    EXPECT_EQ(lineOf(alignment),
              "UTILITY CLEARANCE VIEW 1 EXTENTS DESIGN ALIGNMENT \"Main Road\" MARGIN 2");

    // LEVEL is the drawn line's alone: a file and an alignment carry their own.
    file.designLevel = "5";
    alignment.designLevel = "5";
    EXPECT_EQ(lineOf(file), "UTILITY CLEARANCE C:/survey/schedule.csv C:/survey/design.csv");
    EXPECT_EQ(lineOf(alignment),
              "UTILITY CLEARANCE VIEW 1 EXTENTS DESIGN ALIGNMENT \"Main Road\" MARGIN 2");
}

TEST(UtilityDialog, RegradeAndScheduleAlwaysReadWhatIsDrawnAndDrawReadsEither)
{
    // Whatever source is chosen above the tabs.
    UtilityForm regrade = formFor(UtilityTool::Regrade);
    regrade.scope = "LAYERS utilities";
    regrade.spacing = "20";
    regrade.minCover = "0.6";
    EXPECT_EQ(utilitySourceOf(regrade), UtilitySource::Drawing);
    EXPECT_EQ(lineOf(regrade), "UTILITY REGRADE LAYERS utilities SPACING 20 MINCOVER 0.6");

    UtilityForm schedule = formFor(UtilityTool::Schedule);
    schedule.scope = "DRAWING";
    schedule.scheduleOut = "C:/Deliverables/as built.csv";
    EXPECT_EQ(utilitySourceOf(schedule), UtilitySource::Drawing);
    EXPECT_EQ(lineOf(schedule), "UTILITY SCHEDULE \"C:/Deliverables/as built.csv\" DRAWING");
    schedule.scheduleSchema = "C:/survey/schema.csv";
    EXPECT_EQ(lineOf(schedule), "UTILITY SCHEDULE \"C:/Deliverables/as built.csv\" DRAWING "
                                "SCHEMA C:/survey/schema.csv");

    // Draw reads what is chosen: the survey or an import in the drawing, or
    // the schedule file.
    UtilityForm draw = drawn(UtilityTool::Draw, "DRAWING");
    draw.schedule = "C:/survey/schedule.csv";
    draw.spacing = "8";
    EXPECT_EQ(utilitySourceOf(draw), UtilitySource::Drawing);
    EXPECT_EQ(lineOf(draw), "UTILITY DRAW DRAWING SPACING 8");
    draw.source = UtilitySource::File;
    EXPECT_EQ(utilitySourceOf(draw), UtilitySource::File);
    EXPECT_EQ(lineOf(draw), "UTILITY DRAW C:/survey/schedule.csv SPACING 8");
}

TEST(UtilityDialog, ADrawOfWhatIsDrawnWritesWhatTheGeometryCannotSayInTheVerbsOrder)
{
    UtilityForm draw = drawn(UtilityTool::Draw, "LAYERS survey WHERE PROP=code:W*");
    // Nothing given: each line and point says its own, or the verb refuses.
    EXPECT_EQ(lineOf(draw), "UTILITY DRAW LAYERS survey WHERE PROP=code:W*");
    draw.serviceType = "water";
    draw.method = "EML";
    draw.horizontalUncertainty = "0.1";
    draw.verticalUncertainty = "0.3";
    draw.heights = "surface";
    draw.levelReference = "centre";
    draw.path = "detected";
    draw.owner = "Water Co";
    draw.material = "DICL";
    draw.diameter = "150";
    draw.status = "in service";
    draw.fields = "line=ASSET_ID, point=PT_ID";
    draw.spacing = "8";
    draw.minCover = "0.6";
    draw.layerPrefix = "located";
    EXPECT_EQ(lineOf(draw),
              "UTILITY DRAW LAYERS survey WHERE PROP=code:W* TYPE water METHOD EML H_UNC 0.1 "
              "V_UNC 0.3 HEIGHTS surface LEVEL_REF centre PATH detected OWNER \"Water Co\" "
              "MATERIAL DICL DIAMETER_MM 150 STATUS \"in service\" FIELDS \"line=ASSET_ID, "
              "point=PT_ID\" SPACING 8 MINCOVER 0.6 LAYER located");
    // A schedule says all of it in its columns: none of it is written.
    draw.source = UtilitySource::File;
    draw.schedule = "C:/survey/schedule.csv";
    EXPECT_EQ(lineOf(draw),
              "UTILITY DRAW C:/survey/schedule.csv SPACING 8 MINCOVER 0.6 LAYER located");

    // What does not read is refused by its field, and nothing is written.
    UtilityForm bad = drawn(UtilityTool::Draw, "DRAWING");
    bad.horizontalUncertainty = "ten cm";
    EXPECT_TRUE(has(refusalOf(bad), "The horizontal uncertainty must be a number of metres"));
    bad.horizontalUncertainty.clear();
    bad.diameter = "150mm";
    EXPECT_TRUE(has(refusalOf(bad), "The diameter must be a number of millimetres, not '150mm'"));
    bad.diameter.clear();
    bad.owner = "The \"Water\" Board";
    EXPECT_TRUE(has(refusalOf(bad), "The owner holds a double quote"));
}

TEST(UtilityDialog, AScopeOrWorksThatCannotBeSaidWritesNoLine)
{
    // The scope controls' own refusal, said as theirs.
    UtilityForm untaken = drawn(UtilityTool::Report, "");
    untaken.scopeError = "tick at least one layer to apply to";
    EXPECT_TRUE(has(refusalOf(untaken), "the drawing's scope: tick at least one layer"));
    EXPECT_TRUE(has(refusalOf(drawn(UtilityTool::Verify, "  ")), "what in the drawing"));
    // Regrade refuses it too, whatever the source above the tabs.
    UtilityForm regrade = formFor(UtilityTool::Regrade);
    regrade.scopeError = "that view is no longer open; choose another";
    EXPECT_TRUE(has(refusalOf(regrade), "that view is no longer open"));

    UtilityForm noOut = formFor(UtilityTool::Schedule);
    noOut.scope = "DRAWING";
    EXPECT_TRUE(has(refusalOf(noOut), "schedule to write"));

    UtilityForm entity = formFor(UtilityTool::Clearance);
    entity.designSource = UtilityDesignSource::Entity;
    EXPECT_TRUE(has(refusalOf(entity), "Use Selected"));
    for (const char* bad : {"#x", "0", "#0", "-3", "12a", "#", "1.5"}) {
        entity.designEntity = bad;
        EXPECT_TRUE(has(refusalOf(entity), "#<entity id>, not '")) << bad;
    }
    entity.designEntity = "#7";
    entity.designLevel = "high";
    EXPECT_TRUE(
        has(refusalOf(entity), "The design level must be a number of metres, not 'high'"));

    UtilityForm alignment = formFor(UtilityTool::Clearance);
    alignment.designSource = UtilityDesignSource::Alignment;
    EXPECT_TRUE(has(refusalOf(alignment), "alignment"));
    alignment.alignment = "say \"main\"";
    EXPECT_TRUE(has(refusalOf(alignment), "double quote"));
}

namespace {

// A drawing with a layer for the scope to list, and one plan view (id 4)
// for its View choice to offer.
struct DrawnBench {
    katana::cad::Document document;
    Executor executor;
    katana::cad::CommandInterpreter interpreter{document};

    DrawnBench()
    {
        katana::entity::Layer layer;
        layer.name = "utilities/water/points";
        EXPECT_TRUE(document.execute(katana::commands::createLayer(layer)).ok());
    }

    UtilityDialogContext context()
    {
        UtilityDialogContext made = executor.context(true);
        made.document = &document;
        made.views = [] {
            return std::vector<UtilityScopeView>{
                UtilityScopeView{4, "Plan 2", nullptr,
                                 katana::geometry::Box2(katana::geometry::Point2(0, 0),
                                                        katana::geometry::Point2(40, 20))}};
        };
        return made;
    }
};

void press(QWidget& parent, const char* name)
{
    auto* button = child<QAbstractButton>(parent, name);
    ASSERT_NE(button, nullptr);
    ASSERT_TRUE(button->isEnabled()) << name << " is disabled";
    button->click();
    QApplication::processEvents();
}

bool enabled(QWidget& parent, const char* name)
{
    auto* widget = child<QWidget>(parent, name);
    return widget != nullptr && widget->isEnabled();
}

} // namespace

TEST(UtilityDialog, TheSourceAndTheScopeControlsGiveTheLineItsServices)
{
    DrawnBench bench;
    UtilityToolsDialog dialog(bench.context());
    dialog.showTool(UtilityTool::Report);
    fill(dialog, "utilitySchedule", "C:/survey/schedule.csv");
    auto* command = child<QLineEdit>(dialog, "utilityCommand");
    EXPECT_EQ(command->text(), "UTILITY REPORT C:/survey/schedule.csv");
    // The scope is live only when the drawing is the source, the schedule
    // only when the file is.
    EXPECT_FALSE(enabled(dialog, "utilityScope"));
    EXPECT_TRUE(enabled(dialog, "utilitySchedule"));

    press(dialog, "utilitySourceDrawing");
    EXPECT_TRUE(enabled(dialog, "utilityScope"));
    EXPECT_FALSE(enabled(dialog, "utilitySchedule"));
    EXPECT_FALSE(enabled(dialog, "utilityScheduleBrowse"));
    // What the utility tools act on unless told otherwise: all that is drawn.
    EXPECT_EQ(command->text(), "UTILITY REPORT DRAWING");

    // The drawing's layers are listed, to tick.
    auto* layers = child<QListWidget>(dialog, "utilityLayers");
    ASSERT_NE(layers, nullptr);
    EXPECT_FALSE(layers->findItems("utilities/water/points", Qt::MatchExactly).isEmpty());
    // The window's plan view, by its id: its layers anywhere, or on screen.
    press(dialog, "utilityScopeView");
    auto* view = child<QComboBox>(dialog, "utilityView");
    ASSERT_EQ(view->count(), 1);
    EXPECT_EQ(view->itemText(0), "Plan 2 (VIEW 4)");
    EXPECT_EQ(command->text(), "UTILITY REPORT VIEW 4 EXTENTS");
    child<QCheckBox>(dialog, "utilityOnScreen")->setChecked(true);
    EXPECT_EQ(command->text(), "UTILITY REPORT VIEW 4");
    fill(dialog, "utilityFilterProperty", "utility.type");
    fill(dialog, "utilityFilterValue", "water");
    EXPECT_EQ(command->text(), "UTILITY REPORT VIEW 4 WHERE PROP=utility.type:water");

    // Draw reads what is chosen: here what is drawn, the survey's or an
    // import's geometry, with the fields for what it cannot say live.
    dialog.showTool(UtilityTool::Draw);
    EXPECT_EQ(command->text(), "UTILITY DRAW VIEW 4 WHERE PROP=utility.type:water");
    EXPECT_FALSE(enabled(dialog, "utilitySchedule"));
    EXPECT_TRUE(enabled(dialog, "utilityScope"));
    EXPECT_TRUE(enabled(dialog, "utilitySourceFile"));
    EXPECT_TRUE(enabled(dialog, "utilitySourceDrawing"));
    EXPECT_TRUE(enabled(dialog, "utilityGeometryGroup"));
    // Regrade reads the drawing, and grades: the spacing is live.
    dialog.showTool(UtilityTool::Regrade);
    EXPECT_EQ(command->text(), "UTILITY REGRADE VIEW 4 WHERE PROP=utility.type:water");
    EXPECT_TRUE(enabled(dialog, "utilityScope"));
    EXPECT_TRUE(enabled(dialog, "utilitySpacing"));
    EXPECT_FALSE(enabled(dialog, "utilitySchedule"));
    fill(dialog, "utilitySpacing", "20");
    EXPECT_EQ(command->text(), "UTILITY REGRADE VIEW 4 WHERE PROP=utility.type:water SPACING 20");
    // Schedule names what it writes first.
    dialog.showTool(UtilityTool::Schedule);
    EXPECT_TRUE(command->text().isEmpty());
    EXPECT_TRUE(command->placeholderText().contains("schedule to write"));
    fill(dialog, "utilityScheduleOut", "C:/Deliverables/as built.csv");
    EXPECT_EQ(command->text(), "UTILITY SCHEDULE \"C:/Deliverables/as built.csv\" VIEW 4 WHERE "
                               "PROP=utility.type:water");

    // Run hands that line to the executor, as for every tab; the status is
    // read from the reply, here SCHEDULE's own record (HELP UTILITY).
    bench.executor.reply = std::string(
        "utilities scheduled path=\"C:/Deliverables/as built.csv\" lines=1 vertices=6\n"
        "scope=view view=4 area=0,0,40,20 where=\"PROP=utility.type:water\" matched=9 lines=1 "
        "completed=0 ignored=0\n");
    click(dialog, "utilityRun");
    ASSERT_EQ(bench.executor.lines.size(), 1u);
    EXPECT_EQ(bench.executor.lines.front(), command->text());
    EXPECT_TRUE(text(dialog, "utilityStatus").startsWith("Written."));

    // Back to the file: the schedule is read again.
    dialog.showTool(UtilityTool::Verify);
    press(dialog, "utilitySourceFile");
    EXPECT_EQ(command->text(), "UTILITY VERIFY C:/survey/schedule.csv");
}

TEST(UtilityDialog, ALayerMadeWhileTheDialogIsOpenIsListedOnceTheEventLoopTurns)
{
    DrawnBench bench;
    UtilityToolsDialog dialog(bench.context());
    katana::entity::Layer layer;
    layer.name = "utilities/gas/QL-D";
    ASSERT_TRUE(bench.document.execute(katana::commands::createLayer(layer)).ok());
    katana::qt::test::processEvents();
    auto* layers = child<QListWidget>(dialog, "utilityLayers");
    ASSERT_NE(layers, nullptr);
    EXPECT_FALSE(layers->findItems("utilities/gas/QL-D", Qt::MatchExactly).isEmpty());
}

TEST(UtilityDialog, AScopeThatTakesNoUtilityLineIsSaidSoAndNothingIsSaidRegradedOrWritten)
{
    DrawnBench bench;
    ASSERT_TRUE(bench.interpreter.run("LINE 0,0 10,0").ok());
    // The real verb behind the dialog, so the status is held to the reply
    // the verb gives.
    UtilityDialogContext context = bench.context();
    std::vector<QString> ran;
    context.execute = [&bench, &ran](const QString& line) -> Result<std::string> {
        ran.push_back(line);
        return bench.interpreter.run(line.toStdString());
    };
    UtilityToolsDialog dialog(context);
    dialog.showTool(UtilityTool::Regrade);
    dialog.scopeControls().setChoice(katana::qt::ScopeChoice::Drawing);
    const std::size_t steps = bench.document.history().undoCount();
    click(dialog, "utilityRun");
    ASSERT_EQ(ran.size(), 1u);
    EXPECT_EQ(ran.back(), "UTILITY REGRADE DRAWING");
    EXPECT_TRUE(text(dialog, "utilityOutput").contains("no utility lines in the scope"))
        << text(dialog, "utilityOutput").toStdString();
    // Not "Regraded as one undo step": no step was pushed for Undo to take.
    EXPECT_EQ(text(dialog, "utilityStatus"),
              "Nothing in the scope is a utility line: nothing was regraded, and nothing was "
              "added to the undo history.");
    EXPECT_EQ(bench.document.history().undoCount(), steps);

    // Not "Written": there is no file.
    QTemporaryDir folder;
    ASSERT_TRUE(folder.isValid());
    const QString out = folder.filePath("out.csv");
    dialog.showTool(UtilityTool::Schedule);
    fill(dialog, "utilityScheduleOut", out);
    click(dialog, "utilityRun");
    ASSERT_EQ(ran.size(), 2u);
    EXPECT_EQ(text(dialog, "utilityStatus"),
              "Nothing in the scope is a utility line: nothing was written.");
    EXPECT_FALSE(QFile::exists(out));
}

TEST(UtilityDialog, ADrawOfTheSurveyInTheDrawingRunsTheVerbAndSaysWhenNothingWasLeftToDraw)
{
    // A located main as an import leaves it: a line named by its code.
    DrawnBench bench;
    ASSERT_TRUE(bench.interpreter.run("LINE 334000,6250000 334010,6250000").ok());
    const katana::entity::EntityId main = bench.document.model().entities.ids().back();
    ASSERT_TRUE(bench.document
                    .execute(katana::commands::setEntityProperty({main}, "code", std::string("W7")))
                    .ok());
    UtilityDialogContext context = bench.context();
    std::vector<QString> ran;
    context.execute = [&bench, &ran](const QString& line) -> Result<std::string> {
        ran.push_back(line);
        return bench.interpreter.run(line.toStdString());
    };
    UtilityToolsDialog dialog(context);
    dialog.showTool(UtilityTool::Draw);
    // The file is the source: the geometry's fields are not the line's.
    EXPECT_FALSE(enabled(dialog, "utilityGeometryGroup"));
    press(dialog, "utilitySourceDrawing");
    EXPECT_TRUE(enabled(dialog, "utilityGeometryGroup"));
    dialog.scopeControls().setChoice(katana::qt::ScopeChoice::Drawing);
    // The choices are the verb's own words, as --fill takes them.
    auto* method = child<QComboBox>(dialog, "utilityMethod");
    ASSERT_NE(method, nullptr);
    EXPECT_EQ(method->itemText(0), "not given");
    method->setCurrentIndex(method->findText("EML"));
    auto* type = child<QComboBox>(dialog, "utilityServiceType");
    ASSERT_NE(type, nullptr);
    type->setCurrentIndex(type->findText("water"));
    fill(dialog, "utilityHUnc", "0.2");
    EXPECT_EQ(text(dialog, "utilityCommand"),
              "UTILITY DRAW DRAWING TYPE water METHOD EML H_UNC 0.2");

    const std::size_t steps = bench.document.history().undoCount();
    click(dialog, "utilityRun");
    ASSERT_EQ(ran.size(), 1u);
    EXPECT_TRUE(text(dialog, "utilityOutput").startsWith("utilities drawn lines=1 "))
        << text(dialog, "utilityOutput").toStdString();
    EXPECT_TRUE(text(dialog, "utilityOutput").contains("line id=W7 type=water"));
    EXPECT_TRUE(text(dialog, "utilityStatus").startsWith("Drawn as one undo step"));
    EXPECT_EQ(bench.document.history().undoCount(), steps + 1);

    // Again: the main is drawn already, and nothing else in the scope can be.
    click(dialog, "utilityRun");
    EXPECT_TRUE(text(dialog, "utilityOutput").contains("nothing drawn"));
    EXPECT_EQ(text(dialog, "utilityStatus"),
              "Nothing in the scope to draw: what it took is drawn already or cannot be a "
              "service. Nothing was added to the undo history.");
    EXPECT_EQ(bench.document.history().undoCount(), steps + 1);
}

TEST(UtilityDialog, UseSelectedTakesTheOneSelectedEntityAsTheWorks)
{
    DrawnBench bench;
    ASSERT_TRUE(bench.interpreter.run("LINE 334010,6250000 334010,6250010").ok());
    ASSERT_TRUE(bench.interpreter.run("LINE 334020,6250000 334020,6250010").ok());
    const std::vector<katana::entity::EntityId> ids = bench.document.model().entities.ids();
    ASSERT_EQ(ids.size(), 2u);
    UtilityToolsDialog dialog(bench.context());
    dialog.showTool(UtilityTool::Clearance);
    fill(dialog, "utilitySchedule", "C:/survey/schedule.csv");
    // Its button is live once the works are a drawn line.
    EXPECT_FALSE(enabled(dialog, "utilityDesignUseSelected"));
    press(dialog, "utilityDesignEntity");
    EXPECT_TRUE(enabled(dialog, "utilityDesignUseSelected"));

    // None, then two: which one is meant is not the dialog's to guess.
    bench.document.selection().set({});
    EXPECT_FALSE(dialog.useSelectedDesign());
    EXPECT_TRUE(text(dialog, "utilityStatus").contains("Select the line or polyline"));
    bench.document.selection().set({ids[0], ids[1]});
    EXPECT_FALSE(dialog.useSelectedDesign());
    EXPECT_TRUE(text(dialog, "utilityStatus").contains("2 entities are selected"));

    bench.document.selection().set({ids[1]});
    press(dialog, "utilityDesignUseSelected");
    const QString id = "#" + QString::number(ids[1]);
    EXPECT_EQ(text(dialog, "utilityDesignEntityId"), id);
    EXPECT_EQ(text(dialog, "utilityCommand"),
              "UTILITY CLEARANCE C:/survey/schedule.csv DESIGN " + id);
    fill(dialog, "utilityDesignLevel", "18.4");
    EXPECT_EQ(text(dialog, "utilityCommand"),
              "UTILITY CLEARANCE C:/survey/schedule.csv DESIGN " + id + " LEVEL 18.4");
}

TEST(UtilityDialog, TheDrawingsAlignmentsAreOfferedByName)
{
    DrawnBench bench;
    ASSERT_TRUE(bench.interpreter.run("ALIGN NEW MR1 334000,6250000 334040,6250010").ok());
    UtilityToolsDialog dialog(bench.context());
    dialog.showTool(UtilityTool::Clearance);
    fill(dialog, "utilitySchedule", "C:/survey/schedule.csv");
    auto* names = child<QComboBox>(dialog, "utilityDesignAlignmentName");
    ASSERT_NE(names, nullptr);
    EXPECT_FALSE(names->isEnabled());
    press(dialog, "utilityDesignAlignment");
    EXPECT_TRUE(names->isEnabled());
    ASSERT_EQ(names->count(), 1);
    EXPECT_EQ(text(dialog, "utilityCommand"),
              "UTILITY CLEARANCE C:/survey/schedule.csv DESIGN ALIGNMENT MR1");
    // One made while the dialog is open is offered once the event loop
    // turns, in name order, and the one chosen stays chosen.
    ASSERT_TRUE(bench.interpreter.run("ALIGN NEW Access 334000,6250005 334040,6250005").ok());
    katana::qt::test::processEvents();
    ASSERT_EQ(names->count(), 2);
    EXPECT_EQ(names->itemText(0), "Access");
    EXPECT_EQ(names->currentText(), "MR1");
}

namespace {

// The window side of the workbench: a main window with a Survey menu and the
// views, an interpreter that answers what the test says, and a command line
// that echoes and hands the line back to the workbench as MainWindow's does.
struct Bench {
    katana::cad::Document document;
    QMainWindow window;
    QMenu menu{"Survey"};
    katana::qt::ViewWorkspace* views = nullptr;
    std::vector<std::string> interpreted;
    std::vector<QString> typed;
    std::vector<std::pair<QString, bool>> log;
    Result<std::string> reply = kDrawReply;
    std::unique_ptr<UtilityWorkbench> workbench;

    Bench()
    {
        views = new katana::qt::ViewWorkspace(document, &window);
        window.setCentralWidget(views);
        window.resize(800, 600);
        window.show();
        katana::qt::test::processEvents();
        UtilityServices services;
        services.views = views;
        services.makeAction = [this](Icon, const QString& label, const QString& tip,
                                     const QKeySequence&, const QString& name) {
            auto* action = new QAction(label, &window);
            action->setObjectName(name);
            action->setStatusTip(tip);
            return action;
        };
        services.log = [this](const QString& message, bool isError) {
            log.emplace_back(message, isError);
        };
        services.headless = [] { return true; };
        services.interpret = [this](const std::string& line) {
            interpreted.push_back(line);
            return reply;
        };
        // As MainWindow::runVerbLine does: echoed, then to the workbench -
        // never to a running tool.
        services.run = [this](const QString& line) {
            typed.push_back(line);
            log.emplace_back("> " + line, false);
            (void)workbench->runLine(line);
            return katana::qt::VerbOutcome{true, {}, {}};
        };
        workbench = std::make_unique<UtilityWorkbench>(window, std::move(services), menu);
    }
};

} // namespace

TEST(UtilityWorkbench, EachMenuItemOpensTheOneDialogOnItsTab)
{
    Bench bench;
    // The section's title, then its seven items, in the order of the tabs.
    QStringList items;
    for (const QAction* item : bench.menu.actions()) {
        items << (item->isSeparator() ? "[" + item->text() + "]" : item->objectName());
    }
    EXPECT_EQ(items, (QStringList{"[Subsurface Utilities (AS 5488)]", "utilityDraw",
                                  "utilityReport", "utilityVerify", "utilityClearance",
                                  "utilityCheck", "utilityRegrade", "utilityWriteSchedule"}));
    const std::vector<std::pair<const char*, UtilityTool>> expected = {
        {"utilityClearance", UtilityTool::Clearance}, {"utilityDraw", UtilityTool::Draw},
        {"utilityCheck", UtilityTool::Check},         {"utilityReport", UtilityTool::Report},
        {"utilityVerify", UtilityTool::Verify},       {"utilityRegrade", UtilityTool::Regrade},
        {"utilityWriteSchedule", UtilityTool::Schedule}};
    for (const auto& [name, tool] : expected) {
        auto* action = bench.window.findChild<QAction*>(name);
        ASSERT_NE(action, nullptr) << name;
        // The dialog's name, which is how --dialog finds what the item opened.
        EXPECT_EQ(action->data().toString(), "utilityDialog");
        action->trigger();
        QApplication::processEvents();
        const auto dialogs = bench.window.findChildren<QDialog*>("utilityDialog");
        ASSERT_EQ(dialogs.size(), 1) << name;
        EXPECT_TRUE(dialogs.front()->isVisible());
        EXPECT_EQ(child<QTabWidget>(*dialogs.front(), "utilityTabs")->currentIndex(),
                  static_cast<int>(tool))
            << name;
        EXPECT_EQ(bench.workbench->action(tool), action);
    }
}

TEST(UtilityWorkbench, ADrawFramesWhatItAddedInThePlanViews)
{
    Bench bench;
    ASSERT_TRUE(bench.workbench->runLine("utility  draw C:/survey/schedule.csv"));
    ASSERT_EQ(bench.interpreted.size(), 1u);
    EXPECT_EQ(bench.interpreted.front(), "utility  draw C:/survey/schedule.csv");
    ASSERT_FALSE(bench.log.empty());
    EXPECT_FALSE(bench.log.back().second);
    EXPECT_TRUE(bench.log.back().first.startsWith("utilities drawn lines=2"));
    // Framed on the middle of bounds=, thousands of kilometres from where a
    // new view looks.
    const auto* plan = bench.views->activePlanView();
    ASSERT_NE(plan, nullptr);
    EXPECT_NEAR(plan->viewTransform().center.x, 334020.0, 1e-6);
    EXPECT_NEAR(plan->viewTransform().center.y, 6250003.6, 1e-6);
}

TEST(UtilityWorkbench, ARegradeFramesWhatItDrewAgain)
{
    Bench bench;
    bench.reply = std::string(
        "utilities regraded changed=1 lines=2 vertices=5 segments=3 entities=9 layers=5 "
        "bounds=334000.000,6250000.000,334040.000,6250007.200\n"
        "scope=drawing matched=9 lines=2 completed=0 ignored=0\n");
    ASSERT_TRUE(bench.workbench->runLine("UTILITY REGRADE DRAWING SPACING 20"));
    ASSERT_EQ(bench.interpreted.size(), 1u);
    const auto* plan = bench.views->activePlanView();
    ASSERT_NE(plan, nullptr);
    EXPECT_NEAR(plan->viewTransform().center.x, 334020.0, 1e-6);
    EXPECT_NEAR(plan->viewTransform().center.y, 6250003.6, 1e-6);
}

TEST(UtilityWorkbench, TheDialogsViewScopeOffersTheWindowsViewsByTheirIds)
{
    Bench bench;
    bench.workbench->open(UtilityTool::Report);
    UtilityToolsDialog& dialog = bench.workbench->dialog();
    const std::vector<katana::cad::ViewState*> open = bench.views->viewSet().views();
    ASSERT_FALSE(open.empty());
    auto* view = child<QComboBox>(dialog, "utilityView");
    ASSERT_NE(view, nullptr);
    ASSERT_EQ(view->count(), static_cast<int>(open.size()));
    EXPECT_EQ(view->itemText(0).toStdString(),
              "Plan 1 (VIEW " + std::to_string(open.front()->id) + ")");
    dialog.setSource(katana::qt::UtilitySource::Drawing);
    dialog.scopeControls().setChoice(katana::qt::ScopeChoice::View);
    view->setCurrentIndex(0);
    EXPECT_EQ(text(dialog, "utilityCommand").toStdString(),
              "UTILITY REPORT VIEW " + std::to_string(open.front()->id) + " EXTENTS");
}

TEST(UtilityWorkbench, OnlyADrawThatWorkedIsFramed)
{
    Bench bench;
    const auto before = bench.views->activePlanView()->viewTransform().center;
    // A report is not framed, whatever it says.
    ASSERT_TRUE(bench.workbench->runLine("UTILITY REPORT C:/survey/schedule.csv"));
    EXPECT_EQ(bench.views->activePlanView()->viewTransform().center, before);
    // Nor is a draw that was refused - said in the log as an error.
    bench.reply = makeError(ErrorCode::InvalidArgument, "line W9 has one vertex; nothing was drawn");
    ASSERT_TRUE(bench.workbench->runLine("UTILITY DRAW C:/survey/schedule.csv"));
    EXPECT_EQ(bench.views->activePlanView()->viewTransform().center, before);
    ASSERT_FALSE(bench.log.empty());
    EXPECT_TRUE(bench.log.back().second);
    EXPECT_EQ(bench.log.back().first,
              "InvalidArgument: line W9 has one vertex; nothing was drawn");
}

TEST(UtilityWorkbench, OnlyAUtilityLineIsTaken)
{
    Bench bench;
    EXPECT_FALSE(bench.workbench->runLine("LINE 0,0 10,0"));
    EXPECT_FALSE(bench.workbench->runLine("UTILITYDRAW x.csv"));
    EXPECT_TRUE(bench.interpreted.empty());
    EXPECT_TRUE(bench.log.empty());
}

TEST(UtilityWorkbench, TheDialogsRunGoesThroughTheWindowsCommandLine)
{
    Bench bench;
    bench.workbench->open(UtilityTool::Draw);
    UtilityToolsDialog& dialog = bench.workbench->dialog();
    fill(dialog, "utilitySchedule", "C:/survey/schedule.csv");
    click(dialog, "utilityRun");
    // Typed on the command line - so echoed, kept and undone as a typed line
    // is - and from there to the interpreter.
    ASSERT_EQ(bench.typed.size(), 1u);
    EXPECT_EQ(bench.typed.front(), "UTILITY DRAW C:/survey/schedule.csv");
    ASSERT_EQ(bench.interpreted.size(), 1u);
    EXPECT_EQ(text(dialog, "utilityOutput").toStdString() + "\n", kDrawReply);
    EXPECT_NEAR(bench.views->activePlanView()->viewTransform().center.x, 334020.0, 1e-6);
}

TEST(UtilityWorkbench, ALineTheCommandLineDidNotRunIsAnError)
{
    Bench bench;
    // A command line that swallows the line - a tool taking it for an answer
    // - leaves the dialog nothing to show but that.
    UtilityServices services;
    services.makeAction = [&bench](Icon, const QString& label, const QString&, const QKeySequence&,
                                   const QString& name) {
        auto* action = new QAction(label, &bench.window);
        action->setObjectName(name + "Other");
        return action;
    };
    services.run = [](const QString&) { return katana::qt::VerbOutcome{true, {}, {}}; };
    QMenu menu;
    UtilityWorkbench swallowed(bench.window, std::move(services), menu);
    const auto reply = swallowed.execute("UTILITY VERIFY C:/survey/schedule.csv");
    ASSERT_FALSE(reply.ok());
    EXPECT_EQ(reply.error().code, ErrorCode::InvalidState);
}

// How the bounds= record is read is the verb's (utilities::drawReplyBounds,
// UtilityVerbs.TheDrawRepliesWithItsRecordsBoundsFirst); what is the
// workbench's is that a reply which carries no box moves no view, even from a
// DRAW that worked.
TEST(UtilityWorkbench, ADrawReplyWithoutABoxLeavesTheViewsWhereTheyWere)
{
    Bench bench;
    const auto before = bench.views->activePlanView()->viewTransform().center;
    bench.reply = std::string("utilities drawn lines=1 bounds=5,2,3,4\n");
    ASSERT_TRUE(bench.workbench->runLine("UTILITY DRAW C:/survey/schedule.csv"));
    EXPECT_EQ(bench.views->activePlanView()->viewTransform().center, before);
    ASSERT_FALSE(bench.log.empty());
    EXPECT_FALSE(bench.log.back().second);
}
