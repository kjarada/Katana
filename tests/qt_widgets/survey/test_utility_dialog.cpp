// Survey > Subsurface Utilities (AS 5488): the dialog (src/katana_qt/survey/
// utility_dialog.hpp) and its workbench (utility_workbench.hpp), driven by
// their object names as a person drives them by clicking.
//
// The dialog runs nothing of the AS 5488 library: it writes the UTILITY line
// its fields describe and hands it to an executor. So what is tested here is
// that line - exactly the one a person would type, for every tab - that a line
// which cannot be written runs nothing and says why, that the executor's reply
// is what the dialog shows, and that the workbench opens the dialog on the
// tab of the menu item chosen and frames a DRAW's bounds in the plan views.
// The executors and the interpreter are the tests' own, so nothing here needs
// the verb itself: the headless qt_utility_dialog_headless runs the real one.

#include <gtest/gtest.h>

#include <QAction>
#include <QApplication>
#include <QClipboard>
#include <QDialog>
#include <QFile>
#include <QKeySequence>
#include <QLabel>
#include <QLineEdit>
#include <QMainWindow>
#include <QMenu>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QTabWidget>
#include <QTemporaryDir>

#include <memory>
#include <string>
#include <vector>

#include "katana/cad/document.hpp"
#include "survey/utility_dialog.hpp"
#include "survey/utility_workbench.hpp"
#include "view_workspace.hpp"
#include "widget_harness.hpp"

namespace {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::qt::Icon;
using katana::qt::UtilityDialogContext;
using katana::qt::UtilityForm;
using katana::qt::UtilityServices;
using katana::qt::UtilityTool;
using katana::qt::UtilityToolsDialog;
using katana::qt::UtilityWorkbench;
using katana::qt::utilityCommandLine;

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
          "utilitySave", "utilityStatus", "utilityClose"}) {
        EXPECT_NE(dialog.findChild<QWidget*>(name), nullptr) << name;
    }
    auto* tabs = child<QTabWidget>(dialog, "utilityTabs");
    ASSERT_EQ(tabs->count(), 5);
    EXPECT_EQ(tabs->tabText(0), "Draw");
    EXPECT_EQ(tabs->tabText(1), "Report");
    EXPECT_EQ(tabs->tabText(2), "Verify");
    EXPECT_EQ(tabs->tabText(3), "Clearance");
    EXPECT_EQ(tabs->tabText(4), "Check");
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
    // The section's title, then its five items, in the order of the tabs.
    QStringList items;
    for (const QAction* item : bench.menu.actions()) {
        items << (item->isSeparator() ? "[" + item->text() + "]" : item->objectName());
    }
    EXPECT_EQ(items, (QStringList{"[Subsurface Utilities (AS 5488)]", "utilityDraw",
                                  "utilityReport", "utilityVerify", "utilityClearance",
                                  "utilityCheck"}));
    const std::vector<std::pair<const char*, UtilityTool>> expected = {
        {"utilityClearance", UtilityTool::Clearance}, {"utilityDraw", UtilityTool::Draw},
        {"utilityCheck", UtilityTool::Check},         {"utilityReport", UtilityTool::Report},
        {"utilityVerify", UtilityTool::Verify}};
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
