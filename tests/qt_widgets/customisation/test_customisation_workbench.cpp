// The Format menu's workbench (src/katana_qt/customisation/customisation_workbench):
// the menu and toolbar it fills, the three managers it opens - non-modal, one
// of each, kept between uses and deleted before the Document - the context it
// builds them from, and Purge Unused. Driven through its QActions, as a click
// on the menu drives it. And File > Settings, whose action and one dialog are
// the workbench's too (the last section).
//
// The customisation the managers open on is the three hand-written fixtures of
// tests/data/customisation, loaded by the CUSTOMISE line as a person loads
// them: 3 + 4 definitions and 11 survey code rules, counted by hand from the
// files (test_linestyles, test_symbols, test_survey).
//
// The drawing Purge reads, set up by hand in PurgeDrawing below:
//   linetype  fence   named only by style Spare
//   styles    Kerb    worn by the one point
//             Spare   (fence) worn by nothing
//             Current worn by nothing, but the current style
// So a purge takes Spare and - because Spare goes - fence with it; it keeps
// Kerb (worn) and Current (what the next thing drawn will wear), and the
// protected continuous and none. 1 style, 1 linetype, 0 hatch patterns.

#include <gtest/gtest.h>

#include <filesystem>
#include <functional>
#include <memory>
#include <string>
#include <vector>

#include <QAbstractButton>
#include <QAction>
#include <QApplication>
#include <QCheckBox>
#include <QComboBox>
#include <QDialog>
#include <QDir>
#include <QLabel>
#include <QLineEdit>
#include <QMainWindow>
#include <QMenu>
#include <QMessageBox>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QTabWidget>
#include <QTemporaryDir>
#include <QTimer>
#include <QToolBar>

#include "command_runner.hpp"
#include "customisation/code_manager.hpp"
#include "customisation/customisation_context.hpp"
#include "customisation/customisation_workbench.hpp"
#include "customisation/definition_editor.hpp"
#include "customisation/settings_dialog.hpp"
#include "customisation/symbol_library.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/customisation_host.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/customisation.hpp"
#include "katana/entity/survey_map.hpp"
#include "style_manager.hpp"
#include "view_workspace.hpp"
#include "widget_harness.hpp"

using katana::cad::Document;
using katana::qt::CustomisationServices;
using katana::qt::CustomisationWorkbench;

namespace {

const std::filesystem::path kFixture = std::filesystem::path(__FILE__)
                                           .parent_path()
                                           .parent_path()
                                           .parent_path() /
                                       "data" / "customisation";

// A window with a Format menu and toolbar, and the window's own Layers
// action, as MainWindow hands them over.
struct Bench {
    Document document;
    // The window's one executor, as MainWindow hands it to the views: the
    // lines their controls run, recorded, then run through an interpreter
    // whose view host is the views (withViews).
    katana::cad::CommandInterpreter interpreter{document};
    QStringList ran;
    QMainWindow window;
    QMenu* menu = new QMenu("Format", &window);
    QToolBar* bar = new QToolBar("Format", &window);
    QAction* layers = new QAction("Layers...", &window);
    std::vector<QString> log;
    bool headless = true;
    // The window's views, when a test asks for them: what "show me what
    // uses this" frames in.
    katana::qt::ViewWorkspace* views = nullptr;
    std::unique_ptr<CustomisationWorkbench> bench;

    explicit Bench(bool withViews = false)
    {
        if (withViews) {
            views = new katana::qt::ViewWorkspace(document, &window);
            window.setCentralWidget(views);
            window.resize(800, 600);
            interpreter.setViewHost([this] { return &views->verbHost(); });
            views->setCommandRunner([this](const QString& line) {
                ran << line;
                const auto reply = interpreter.run(line.toStdString());
                katana::qt::VerbOutcome outcome;
                outcome.ok = reply.ok();
                if (reply.ok()) {
                    outcome.reply = QString::fromStdString(*reply);
                } else {
                    outcome.error = QString::fromStdString(reply.error().describe());
                }
                return outcome;
            });
        }
        layers->setObjectName("formatLayers");
        CustomisationServices services;
        services.document = &document;
        services.views = views;
        services.makeAction = [this](katana::qt::Icon icon, const QString& text, const QString&,
                                     const QKeySequence&, const QString& name) {
            auto* action = new QAction(katana::qt::icon(icon), text, &window);
            action->setObjectName(name);
            return action;
        };
        services.log = [this](const QString& text, bool) { log.push_back(text); };
        services.headless = [this] { return headless; };
        services.layers = layers;
        bench = std::make_unique<CustomisationWorkbench>(window, std::move(services), *menu, *bar);
    }

    QAction& action(const char* name)
    {
        auto* found = window.findChild<QAction*>(name);
        EXPECT_NE(found, nullptr) << name;
        return *found;
    }

    // The three fixtures in one CUSTOMISE line, each path quoted (a checkout
    // may sit in a folder with a blank in its name).
    void loadFixture()
    {
        std::string line = "CUSTOMISE";
        for (const char* name : {"test_linestyles", "test_survey", "test_symbols"}) {
            const std::string file = std::string(name) + ".customisation.json";
            line += " \"" + (kFixture / file).generic_string() + "\"";
        }
        const auto loaded = interpreter.run(line);
        ASSERT_TRUE(loaded.ok()) << loaded.error().describe();
        ASSERT_EQ(document.styleLibrary().size(), 7u);
        ASSERT_EQ(document.surveyMap().size(), 11u);
    }
};

std::vector<std::string> names(const QList<QAction*>& actions)
{
    std::vector<std::string> out;
    for (const QAction* action : actions) {
        out.push_back(action->isSeparator() ? "---" : action->objectName().toStdString());
    }
    return out;
}

void must(Document& document, katana::commands::CommandPtr command)
{
    const auto status = document.execute(std::move(command));
    ASSERT_TRUE(status.ok()) << status.error().describe();
}

void purgeDrawing(Document& document)
{
    katana::entity::Linetype fence;
    fence.name = "fence";
    fence.pattern = {{1.0}, {-0.5}};
    must(document, katana::commands::createLinetype(fence));
    for (const auto& [name, linetype] : std::vector<std::pair<const char*, const char*>>{
             {"Kerb", "continuous"}, {"Spare", "fence"}, {"Current", "continuous"}}) {
        katana::entity::Style style;
        style.name = name;
        style.linetype = linetype;
        must(document, katana::commands::createStyle(style));
    }
    katana::commands::EntityAttributes attributes;
    attributes.style = "Kerb";
    must(document, katana::commands::createPoint(katana::geometry::Point2(0.0, 0.0), attributes));
    ASSERT_TRUE(document.setCurrentStyle("Current").ok());
}

// A widget of the code manager by the object name the headless driver finds
// it by; the test stops where one is missing.
template <typename T> T* named(QWidget* dialog, const char* name)
{
    T* found = dialog != nullptr ? dialog->findChild<T*>(QString::fromLatin1(name)) : nullptr;
    EXPECT_NE(found, nullptr) << "no " << name;
    return found;
}

// Answers the next question box with `button`, from inside its exec(), as a
// person clicking it would. Stops with this object, so a box the test did not
// expect is never answered by a later test's timer.
struct BoxAnswer {
    QTimer timer;
    bool asked = false;

    explicit BoxAnswer(QMessageBox::StandardButton button)
    {
        timer.setInterval(10);
        QObject::connect(&timer, &QTimer::timeout, [this, button] {
            auto* box = qobject_cast<QMessageBox*>(QApplication::activeModalWidget());
            if (box == nullptr || box->button(button) == nullptr) {
                return;
            }
            asked = true;
            timer.stop();
            box->button(button)->click();
        });
        timer.start();
    }
};

} // namespace

TEST(CustomisationWorkbench, TheFormatMenuOffersLayersTheManagersGlobalModifyAndPurgeInThatOrder)
{
    Bench bench;
    // Each group under a titled section, the first too (theme.cpp draws the
    // titles). Two sections: the third, Customisation Files, went with the
    // Load and Replace items it held - a customisation file is loaded by the
    // CUSTOMISE line, which is no menu item of Format's.
    EXPECT_EQ(names(bench.menu->actions()),
              (std::vector<std::string>{"---", "formatLayers", "formatStyles", "formatSymbols",
                                        "formatSurveyCodes", "---", "formatGlobalModify",
                                        "formatPurge"}));
    QStringList titles;
    for (const QAction* action : bench.menu->actions()) {
        if (action->isSeparator()) {
            titles << action->text();
        }
    }
    EXPECT_EQ(titles, (QStringList{"Tables and Libraries", "Across the Drawing"}));
    EXPECT_EQ(names(bench.bar->actions()),
              (std::vector<std::string>{"formatStyles", "formatSymbols", "formatSurveyCodes",
                                        "formatGlobalModify"}));
    EXPECT_EQ(bench.bench->codeManagerAction(), &bench.action("formatSurveyCodes"))
        << "the action the Survey menu shows too";
}

TEST(CustomisationWorkbench, EachManagerOpensBesideTheDrawingAndTheSameOneComesBack)
{
    Bench bench;
    bench.loadFixture();
    for (const char* name : {"formatStyles", "formatSymbols", "formatSurveyCodes"}) {
        QAction& action = bench.action(name);
        action.trigger();
        auto* dialog = bench.window.findChild<QDialog*>(action.data().toString());
        ASSERT_NE(dialog, nullptr) << name << " names the dialog it opens in its data";
        EXPECT_TRUE(dialog->isVisible()) << name;
        EXPECT_FALSE(dialog->isModal()) << name << " is beside the drawing, not over it";
        dialog->close();
        EXPECT_FALSE(dialog->isVisible());
        action.trigger();
        EXPECT_EQ(bench.window.findChildren<QDialog*>(action.data().toString()).size(), 1)
            << name << ": one of each";
        EXPECT_TRUE(dialog->isVisible()) << name << ": the same one, shown again";
    }
    EXPECT_NE(bench.bench->styleManager(), nullptr);
    EXPECT_NE(bench.bench->symbolLibrary(), nullptr);
    EXPECT_NE(bench.bench->codeManager(), nullptr);
}

TEST(CustomisationWorkbench, TheCodeManagersUnappliedEditsSurviveClosingAndReopeningIt)
{
    // By hand: the fixture map has 11 rules; a duplicate of rule 0 makes 12
    // in the buffer and none in the drawing's map until Apply.
    Bench bench;
    bench.loadFixture();
    bench.action("formatSurveyCodes").trigger();
    katana::qt::SurveyCodeManagerDialog* codes = bench.bench->codeManager();
    ASSERT_NE(codes, nullptr);
    ASSERT_TRUE(codes->duplicateRule(0).ok());
    ASSERT_TRUE(codes->dirty());

    codes->close();
    bench.action("formatSurveyCodes").trigger();

    EXPECT_EQ(bench.bench->codeManager(), codes);
    EXPECT_TRUE(codes->dirty());
    EXPECT_EQ(codes->buffer().size(), 12u);
    EXPECT_EQ(bench.document.surveyMap().size(), 11u) << "nothing applied";
}

TEST(CustomisationWorkbench,
     TheLineworkTabStringsWithTheCustomisationsControlCodesAndFollowsAChange)
{
    // By hand. The fixture's rule WM* draws a line, so four points coded WM1
    // are one string, joined in the order made: (0,0), (10,0), (20,0),
    // (30,0). The third is coded "WM1 GO". While the start code is the
    // default ST, GO is a token nothing knows: ONE line through the four,
    // and one note. Once the customisation's start code is GO, the third
    // point begins a second line: TWO lines of two points, and no note.
    Bench bench;
    bench.loadFixture();
    for (const char* line : {"POINT 0,0", "POINT 10,0", "POINT 20,0", "POINT 30,0", "SELECT ALL",
                             "PROP SET code WM1", "SELECT 3", "PROP SET code \"WM1 GO\"",
                             "SELECT NONE"}) {
        const auto ran = bench.interpreter.run(line);
        ASSERT_TRUE(ran.ok()) << line << ": " << ran.error().describe();
    }
    bench.action("formatSurveyCodes").trigger();
    katana::qt::SurveyCodeManagerDialog* codes = bench.bench->codeManager();
    auto* start = named<QLineEdit>(codes, "lineworkStart");
    auto* order = named<QComboBox>(codes, "lineworkOrder");
    auto* preview = named<QPushButton>(codes, "lineworkPreview");
    auto* execute = named<QPushButton>(codes, "lineworkExecute");
    auto* summary = named<QLabel>(codes, "lineworkSummary");
    ASSERT_FALSE(start == nullptr || order == nullptr || preview == nullptr ||
                 execute == nullptr || summary == nullptr);
    // In the order observed: the points were typed, and carry no number.
    order->setCurrentIndex(1);

    EXPECT_EQ(start->text(), "ST");
    preview->click();
    EXPECT_EQ(summary->text(), "Preview only: 1 lines from 4 points; 0 points not placed; 1 "
                               "notes. Execute builds them as one undoable step.");

    const auto set = bench.interpreter.run("CUSTOMISE SET linework.start=GO");
    ASSERT_TRUE(set.ok()) << set.error().describe();
    katana::qt::test::processEvents();

    EXPECT_EQ(start->text(), "GO") << "the tab shows the customisation's code";
    EXPECT_EQ(codes->lineworkCodes().start, "GO");
    EXPECT_EQ(bench.bench->lineworkCodes().start, "GO");
    ASSERT_FALSE(bench.log.empty());
    EXPECT_EQ(bench.log.back(), "Linework control codes set for this session.");
    // Execute with no Preview pressed since: what is built is planned with
    // the codes the tab now shows, not the plan made with ST.
    execute->click();
    EXPECT_EQ(bench.document.model().entities.size(), 6u) << "the 4 points and 2 lines";
    EXPECT_EQ(bench.document.lastCreatedEntities().size(), 2u);
    EXPECT_EQ(summary->text(),
              "Preview only: 2 lines from 4 points; 0 points not placed; 0 notes. Execute builds "
              "them as one undoable step. Executed as one undoable step.");
}

TEST(CustomisationWorkbench, ACodeManagerOpenedLaterIsBuiltOnTheCustomisationsLineworkCodes)
{
    Bench bench;
    const auto set = bench.interpreter.run("CUSTOMISE SET linework.start=BEGIN linework.end=STOP");
    ASSERT_TRUE(set.ok()) << set.error().describe();
    // Opened in the same turn: no event loop has run since the line.
    bench.action("formatSurveyCodes").trigger();
    katana::qt::SurveyCodeManagerDialog* codes = bench.bench->codeManager();
    auto* start = named<QLineEdit>(codes, "lineworkStart");
    auto* end = named<QLineEdit>(codes, "lineworkEnd");
    auto* use = named<QPushButton>(codes, "lineworkCodesUse");
    ASSERT_FALSE(start == nullptr || end == nullptr || use == nullptr);
    EXPECT_EQ(start->text(), "BEGIN");
    EXPECT_EQ(end->text(), "STOP");
    EXPECT_EQ(codes->lineworkCodes().start, "BEGIN");
    EXPECT_EQ(codes->lineworkCodes().close, "CL") << "a code the line did not name is as it was";

    // The tab's own Use These Codes sets the window's copy alone: the
    // customisation still says BEGIN. (That button running CUSTOMISE SET is
    // the code manager's own change to make.)
    start->setText("MINE");
    use->click();
    ASSERT_EQ(codes->lineworkCodes().start, "MINE");
    EXPECT_EQ(bench.document.customisationState().linework.start, "BEGIN");
    // A change of the customisation that is not of its control codes leaves
    // what the tab was given ...
    const auto other = bench.interpreter.run("CUSTOMISE SET auto.codes=off");
    ASSERT_TRUE(other.ok()) << other.error().describe();
    katana::qt::test::processEvents();
    EXPECT_EQ(codes->lineworkCodes().start, "MINE");
    EXPECT_EQ(start->text(), "MINE");
    // ... and a change of them takes over.
    const auto changed = bench.interpreter.run("CUSTOMISE SET linework.start=GO");
    ASSERT_TRUE(changed.ok()) << changed.error().describe();
    katana::qt::test::processEvents();
    EXPECT_EQ(codes->lineworkCodes().start, "GO");
    EXPECT_EQ(start->text(), "GO");
    EXPECT_EQ(end->text(), "STOP");
}

TEST(CustomisationWorkbench, TheManagersAskNothingInAHeadlessSessionAndMayInAnInteractiveOne)
{
    Bench bench;
    bench.action("formatSurveyCodes").trigger();
    bench.action("formatSymbols").trigger();
    EXPECT_FALSE(bench.bench->codeManager()->interactive());
    EXPECT_TRUE(bench.bench->symbolLibrary()->headless());

    bench.bench->codeManager()->hide();
    bench.bench->symbolLibrary()->hide();
    bench.headless = false;
    bench.action("formatSurveyCodes").trigger();
    bench.action("formatSymbols").trigger();
    EXPECT_TRUE(bench.bench->codeManager()->interactive());
    EXPECT_FALSE(bench.bench->symbolLibrary()->headless());
    bench.bench->codeManager()->hide();
    bench.bench->symbolLibrary()->hide();
}

TEST(CustomisationWorkbench, TheManagersGoWithTheWorkbenchBeforeTheDocument)
{
    // The window destroys its members - the workbench, then the Document -
    // before its children; a dialog left to the children would outlive the
    // Document it holds.
    Bench bench;
    for (const char* name : {"formatStyles", "formatSymbols", "formatSurveyCodes"}) {
        bench.action(name).trigger();
    }
    const QPointer<QDialog> styles(bench.bench->styleManager());
    const QPointer<QDialog> symbols(bench.bench->symbolLibrary());
    const QPointer<QDialog> codes(bench.bench->codeManager());
    ASSERT_FALSE(styles.isNull() || symbols.isNull() || codes.isNull());
    bench.bench.reset();
    EXPECT_TRUE(styles.isNull());
    EXPECT_TRUE(symbols.isNull());
    EXPECT_TRUE(codes.isNull());
}

TEST(CustomisationWorkbench, ShowingWhatUsesAThingSelectsItAndFramesItInTheActivePlanView)
{
    // A line from (1000, 1000) to (1010, 1000): framed, the active plan
    // view's centre is the line's middle, (1005, 1000). Framed by that
    // view's Zoom to Selection line through the window's executor, so the
    // command log says what moved the view and the views linked with it
    // follow: the direct zoom made before framed the same place outside the
    // log, and only the line the executor was handed tells the two apart.
    Bench bench(true);
    katana::qt::test::paint(bench.window);
    katana::qt::ViewWorkspace* views = bench.views;
    must(bench.document, katana::commands::createLine(katana::geometry::Point2(1000.0, 1000.0),
                                                      katana::geometry::Point2(1010.0, 1000.0)));
    const auto ids = bench.document.lastCreatedEntities();
    ASSERT_EQ(ids.size(), 1u);
    bench.bench->context().selectAndShow(ids);

    EXPECT_EQ(bench.document.selection().ids(), ids);
    ASSERT_NE(views->activePlanView(), nullptr);
    const katana::cad::ViewId active = views->activePlanView()->state().id;
    EXPECT_EQ(bench.ran, QStringList{QString("ZOOM SELECTION view=%1").arg(active)});
    const katana::geometry::Point2 centre = views->activePlanView()->viewTransform().center;
    EXPECT_NEAR(centre.x, 1005.0, 1e-6);
    EXPECT_NEAR(centre.y, 1000.0, 1e-6);
}

TEST(CustomisationWorkbench, PurgeUnusedDeletesWhatNothingUsesAsOneUndoStepAndNamesIt)
{
    Bench bench;
    purgeDrawing(bench.document);
    const std::size_t steps = bench.document.history().undoCount();

    bench.action("formatPurge").trigger();

    EXPECT_EQ(bench.document.history().undoCount(), steps + 1) << "one undo step";
    EXPECT_EQ(bench.document.model().styles.find("Spare"), nullptr);
    EXPECT_EQ(bench.document.model().linetypes.find("fence"), nullptr);
    EXPECT_NE(bench.document.model().styles.find("Kerb"), nullptr) << "worn";
    EXPECT_NE(bench.document.model().styles.find("Current"), nullptr) << "current";
    ASSERT_FALSE(bench.log.empty());
    EXPECT_EQ(bench.log.back(), "Purge Unused: purged 1 style, 1 linetype and 0 hatch patterns "
                                "(one Undo restores them). Styles: Spare. Linetypes: fence.");

    ASSERT_TRUE(bench.document.undo().ok());
    EXPECT_NE(bench.document.model().styles.find("Spare"), nullptr);
    EXPECT_NE(bench.document.model().linetypes.find("fence"), nullptr);
}

TEST(CustomisationWorkbench, PurgeWithNothingUnusedMakesNoUndoStepAndSaysSo)
{
    Bench bench;
    const std::size_t steps = bench.document.history().undoCount();
    EXPECT_FALSE(bench.bench->purgeUnused());
    EXPECT_EQ(bench.document.history().undoCount(), steps);
    ASSERT_FALSE(bench.log.empty());
    EXPECT_EQ(bench.log.back(), "Purge Unused: nothing to purge - every style, linetype and hatch "
                                "pattern is used.");
}

TEST(CustomisationWorkbench, AnInteractivePurgeIsAskedFirstAndANoDeletesNothing)
{
    Bench bench;
    bench.headless = false;
    purgeDrawing(bench.document);
    QString asked;
    bench.bench->confirm = [&](const QString& question) {
        asked = question;
        return false;
    };
    const std::size_t steps = bench.document.history().undoCount();

    EXPECT_FALSE(bench.bench->purgeUnused());

    EXPECT_EQ(asked, "Delete 1 style, 1 linetype and 0 hatch patterns that nothing uses? One Undo "
                     "restores them.");
    EXPECT_EQ(bench.document.history().undoCount(), steps);
    EXPECT_NE(bench.document.model().styles.find("Spare"), nullptr);

    bench.bench->confirm = [](const QString&) { return true; };
    EXPECT_TRUE(bench.bench->purgeUnused());
    EXPECT_EQ(bench.document.model().styles.find("Spare"), nullptr);
}

TEST(CustomisationWorkbench, TheWindowMayCloseWhenNoManagerHoldsUnappliedEdits)
{
    Bench bench;
    EXPECT_TRUE(bench.bench->confirmClose()) << "no manager opened";
    bench.loadFixture();
    bench.action("formatSurveyCodes").trigger();
    EXPECT_TRUE(bench.bench->confirmClose()) << "opened, nothing edited";
    EXPECT_TRUE(bench.log.empty());
}

TEST(CustomisationWorkbench, AHeadlessCloseWithUnappliedCodeEditsIsRefusedAndSaid)
{
    // By hand: the fixture's 11 rules and a duplicate of rule 0 make 12 in
    // the buffer, none of them on the drawing. Nobody can be asked, so the
    // close is refused, not the edits dropped - and not called kept.
    Bench bench;
    bench.loadFixture();
    bench.action("formatSurveyCodes").trigger();
    katana::qt::SurveyCodeManagerDialog* codes = bench.bench->codeManager();
    ASSERT_TRUE(codes->duplicateRule(0).ok());
    codes->hide();

    EXPECT_FALSE(bench.bench->confirmClose());

    ASSERT_FALSE(bench.log.empty());
    EXPECT_EQ(bench.log.back(),
              "Unapplied Edits: the Survey Code Manager has rule edits that are not on the "
              "drawing, and a headless run has nobody to ask whether to discard them; Apply or "
              "Revert them first.");
    EXPECT_TRUE(codes->dirty());
    EXPECT_EQ(codes->buffer().size(), 12u);
    EXPECT_EQ(bench.document.surveyMap().size(), 11u);

    codes->revert();
    EXPECT_TRUE(bench.bench->confirmClose()) << "reverted: nothing left to lose";
}

TEST(CustomisationWorkbench, AnInteractiveCloseAsksOverTheCodeManagerAndCancelKeepsItsEdits)
{
    // The manager was closed with its edits kept, so it is hidden: it comes
    // back to ask. Cancel leaves it open and the window with it; Discard lets
    // the window go with the drawing's 11 rules; Apply puts the 12 on it.
    Bench bench;
    bench.headless = false;
    bench.loadFixture();
    bench.action("formatSurveyCodes").trigger();
    katana::qt::SurveyCodeManagerDialog* codes = bench.bench->codeManager();
    ASSERT_TRUE(codes->interactive());
    ASSERT_TRUE(codes->duplicateRule(0).ok());
    codes->hide();

    {
        BoxAnswer cancel(QMessageBox::Cancel);
        EXPECT_FALSE(bench.bench->confirmClose());
        EXPECT_TRUE(cancel.asked);
    }
    EXPECT_TRUE(codes->isVisible()) << "shown to ask, and left open";
    EXPECT_TRUE(codes->dirty());
    EXPECT_EQ(codes->buffer().size(), 12u);

    {
        BoxAnswer discard(QMessageBox::Discard);
        EXPECT_TRUE(bench.bench->confirmClose());
        EXPECT_TRUE(discard.asked);
    }
    EXPECT_FALSE(codes->isVisible());
    EXPECT_FALSE(codes->dirty());
    EXPECT_EQ(bench.document.surveyMap().size(), 11u);

    ASSERT_TRUE(codes->duplicateRule(0).ok());
    {
        BoxAnswer apply(QMessageBox::Apply);
        EXPECT_TRUE(bench.bench->confirmClose());
        EXPECT_TRUE(apply.asked);
    }
    EXPECT_FALSE(codes->dirty());
    EXPECT_EQ(bench.document.surveyMap().size(), 12u);
}

// ---- an editor's own commit keeps a kept session kept ------------------------------------
//
// CustomisationContext::beginCommit, answered by the workbench: asked before a
// manager or the definition editor commits, it hands back the CUSTOMISE KEEP
// line - to run after the commit - only when the session WAS the kept one and
// this session has a kept file (CustomisationServices::hasKeptFile).
//
// The session of these tests starts as a program's does (cad::startCustomisation
// through a host), with the fixture test_symbols as its built-in: 4 symbols, no
// rule, counted by hand from its text. With no kept file on disk the built-in is
// what starts, and it is "kept" - it is what the next start would give.

namespace {

// The workbench with an executor of its own - each line recorded, then run by
// an interpreter that holds the host - and a kept file in a folder of the
// test's, which does not exist until a KEEP writes it.
struct KeptBench {
    Document document;
    katana::cad::CommandInterpreter interpreter{document};
    QTemporaryDir folder;
    std::filesystem::path keptFile;
    katana::cad::CustomisationHost host;
    QStringList ran;
    std::vector<QString> log;
    QMainWindow window;
    QMenu* menu = new QMenu("Format", &window);
    QToolBar* bar = new QToolBar("Format", &window);
    std::unique_ptr<CustomisationWorkbench> bench;

    // `withKeptFile`: whether the host names a kept file, which is what the
    // window tells the workbench.
    explicit KeptBench(bool withKeptFile)
    {
        EXPECT_TRUE(folder.isValid());
        keptFile = std::filesystem::path(folder.path().toStdU16String()) / "customisation.json";
        const auto builtIn =
            katana::cad::readCustomisationFile(kFixture / "test_symbols.customisation.json");
        EXPECT_TRUE(builtIn.ok()) << (builtIn.ok() ? "" : builtIn.error().describe());
        if (builtIn.ok()) {
            host.builtIn.customisation =
                std::make_shared<const katana::entity::Customisation>(builtIn->customisation);
            host.builtIn.digest = builtIn->digest;
        }
        if (withKeptFile) {
            host.keptFile = keptFile;
        }
        interpreter.setCustomisationHost(host);
        const katana::cad::CustomisationStart start =
            katana::cad::startCustomisation(document, host);
        EXPECT_EQ(start.installed, katana::cad::CustomisationOrigin::BuiltIn);
        EXPECT_TRUE(document.customisationState().kept);

        CustomisationServices services;
        services.document = &document;
        services.makeAction = [this](katana::qt::Icon icon, const QString& text, const QString&,
                                     const QKeySequence&, const QString& name) {
            auto* action = new QAction(katana::qt::icon(icon), text, &window);
            action->setObjectName(name);
            return action;
        };
        services.log = [this](const QString& text, bool) { log.push_back(text); };
        services.headless = [] { return true; };
        services.run = [this](const QString& line) {
            ran << line;
            const auto reply = interpreter.run(line.toStdString());
            katana::qt::VerbOutcome outcome;
            outcome.ok = reply.ok();
            if (reply.ok()) {
                outcome.reply = QString::fromStdString(*reply);
            } else {
                outcome.error = QString::fromStdString(reply.error().describe());
                log.push_back(outcome.error);
            }
            return outcome;
        };
        services.hasKeptFile = [withKeptFile] { return withKeptFile; };
        // The host's two facts as the window tells them: the built-in this
        // session started from, and the kept file's path as text, with '/'.
        services.hasBuiltIn = [] { return true; };
        services.keptFile = [this, withKeptFile] {
            return withKeptFile ? folder.path() + QStringLiteral("/customisation.json")
                                : QString();
        };
        bench = std::make_unique<CustomisationWorkbench>(window, std::move(services), *menu, *bar);
    }

    [[nodiscard]] int keepLines() const
    {
        return static_cast<int>(ran.count(QStringLiteral("CUSTOMISE KEEP")));
    }

    // The three commits an editor makes, in turn: the Survey Code Manager's
    // Apply of one new rule, the Symbol Library's Import Definitions of the
    // fixture's three linestyles, and the definition editor's Save of a new
    // symbol of two strokes. After each, `afterEach` is told which it was.
    void commitThroughEachEditor(const std::function<void(const char* which)>& afterEach)
    {
        katana::qt::SurveyCodeManagerDialog& codes = bench->showCodeManager();
        katana::entity::SurveyRule rule;
        rule.key = "KQ*";
        rule.model = "KEPT";
        ASSERT_TRUE(codes.addRule(rule).ok());
        const auto applied = codes.apply();
        ASSERT_TRUE(applied.ok()) << applied.error().describe();
        ASSERT_EQ(document.surveyMap().size(), 1u);
        afterEach("the Survey Code Manager's Apply");

        ASSERT_TRUE(bench->showSymbolLibrary().importDefinitionsFile(
            kFixture / "test_linestyles.customisation.json"));
        ASSERT_EQ(document.styleLibrary().size(), 7u) << "4 symbols and 3 linestyles";
        afterEach("the Symbol Library's Import Definitions");

        katana::qt::DefinitionEditorDialog& editor = bench->showDefinitionEditor();
        ASSERT_TRUE(editor.newDefinition(true));
        named<QLineEdit>(&editor, "definitionName")->setText(QStringLiteral("TEST Post"));
        named<QPlainTextEdit>(&editor, "definitionStrokes")
            ->setPlainText(QStringLiteral("[\"move\", 0, 0],\n[\"draw\", 0, 2]"));
        ASSERT_TRUE(editor.save());
        ASSERT_EQ(document.styleLibrary().size(), 8u);
        afterEach("the definition editor's Save");
    }
};

} // namespace

TEST(CustomisationWorkbench, AnEditorsCommitOfAKeptSessionRunsTheKeepLineOnceAndItStaysKept)
{
    KeptBench bench(true);
    ASSERT_FALSE(std::filesystem::exists(bench.keptFile));
    int commits = 0;
    bench.commitThroughEachEditor([&](const char* which) {
        ++commits;
        // ONE line more after each commit, and it is the last line run.
        EXPECT_EQ(bench.keepLines(), commits) << which;
        ASSERT_FALSE(bench.ran.isEmpty()) << which;
        EXPECT_EQ(bench.ran.last(), QStringLiteral("CUSTOMISE KEEP")) << which;
        // The line did what a typed one does: the session is the kept one
        // again, which is why the NEXT editor's commit keeps it too.
        EXPECT_TRUE(bench.document.customisationState().kept) << which;
        EXPECT_TRUE(std::filesystem::exists(bench.keptFile)) << which;
    });
    ASSERT_EQ(commits, 3);
    // Only the three: the manager's Apply, the import and the Save are not
    // lines themselves.
    EXPECT_EQ(bench.ran, (QStringList{QStringLiteral("CUSTOMISE KEEP"),
                                      QStringLiteral("CUSTOMISE KEEP"),
                                      QStringLiteral("CUSTOMISE KEEP")}));

    // The definition editor's Delete is a line of its own, and is kept too.
    katana::qt::DefinitionEditorDialog* editor = bench.bench->definitionEditor();
    ASSERT_NE(editor, nullptr);
    ASSERT_TRUE(editor->remove(false)) << "nothing names TEST Post";
    ASSERT_EQ(bench.ran.size(), 5);
    EXPECT_EQ(bench.ran[3], QStringLiteral("CUSTOMISE REMOVE \"TEST Post\""));
    EXPECT_EQ(bench.ran[4], QStringLiteral("CUSTOMISE KEEP"));

    // What the next start reads is what the editors left: by hand, the
    // built-in's 4 symbols and the 3 imported linestyles (the new symbol was
    // deleted again), and the one rule.
    Document next;
    const katana::cad::CustomisationStart start = katana::cad::startCustomisation(next, bench.host);
    EXPECT_EQ(start.installed, katana::cad::CustomisationOrigin::Kept);
    EXPECT_EQ(start.definitions, 7u);
    EXPECT_EQ(start.symbols, 4u);
    EXPECT_EQ(start.rules, 1u);
    EXPECT_TRUE(start.problems.empty());
}

TEST(CustomisationWorkbench, ASessionThatWasNotTheKeptOneIsNotKeptByAnEditorsCommit)
{
    // A customisation typed in lasts the session: the load makes the session
    // "not kept", and an editor's change on top of it is not written either.
    KeptBench bench(true);
    const auto loaded = bench.interpreter.run(
        "CUSTOMISE \"" + (kFixture / "test_symbols.customisation.json").generic_string() + "\"");
    ASSERT_TRUE(loaded.ok()) << loaded.error().describe();
    ASSERT_FALSE(bench.document.customisationState().kept);

    int commits = 0;
    bench.commitThroughEachEditor([&](const char* which) {
        ++commits;
        EXPECT_TRUE(bench.ran.isEmpty()) << which << " ran " << bench.ran.join(" | ").toStdString();
        EXPECT_FALSE(bench.document.customisationState().kept) << which;
    });
    EXPECT_EQ(commits, 3);
    EXPECT_FALSE(std::filesystem::exists(bench.keptFile));
}

TEST(CustomisationWorkbench, WithNoKeptFileAnEditorsCommitRunsNoLineAndLogsNoRefusal)
{
    // The session IS the kept one - the built-in, as started - but nothing
    // names a file to keep it in. KEEP would be refused after every commit,
    // so it is not asked.
    KeptBench bench(false);
    int commits = 0;
    bench.commitThroughEachEditor([&](const char* which) {
        ++commits;
        EXPECT_TRUE(bench.ran.isEmpty()) << which << " ran " << bench.ran.join(" | ").toStdString();
    });
    EXPECT_EQ(commits, 3);
    for (const QString& line : bench.log) {
        EXPECT_FALSE(line.contains(QStringLiteral("KEEP"))) << line.toStdString();
    }
}

// ---- File > Settings: the workbench's action and its one dialog ---------------------------
//
// The item is File's, but the action and the dialog are made and kept here
// (customisation_workbench.hpp), so that Settings is given what only the
// workbench knows - whether the Survey Code Manager holds unapplied edits, and
// how to open the editors - beside what the window tells it: the executor,
// the log and its host's built-in and kept file.

TEST(CustomisationWorkbench, TheSettingsActionIsFilesItemAndOpensTheOneNonModalDialogItNames)
{
    Bench bench;
    QAction& action = bench.action("fileSettings");
    EXPECT_EQ(bench.bench->settingsAction(), &action);
    EXPECT_EQ(action.text(), "Settin&gs...");
    // What macOS moves into the application menu.
    EXPECT_EQ(action.menuRole(), QAction::PreferencesRole);
    // How the headless --dialog finds what the item opened.
    EXPECT_EQ(action.data().toString(), "settingsDialog");
    EXPECT_FALSE(action.icon().isNull());
    // File's item: the workbench puts it in neither Format nor its toolbar.
    EXPECT_FALSE(bench.menu->actions().contains(&action));
    EXPECT_FALSE(bench.bar->actions().contains(&action));
    EXPECT_EQ(bench.bench->settings(), nullptr) << "made the first time it is asked for";

    action.trigger();
    auto* dialog = bench.window.findChild<QDialog*>(action.data().toString());
    ASSERT_NE(dialog, nullptr);
    EXPECT_EQ(dialog, static_cast<QDialog*>(bench.bench->settings()));
    EXPECT_TRUE(dialog->isVisible());
    EXPECT_FALSE(dialog->isModal()) << "beside the drawing, not over it";

    dialog->close();
    EXPECT_FALSE(dialog->isVisible());
    action.trigger();
    EXPECT_EQ(bench.window.findChildren<QDialog*>("settingsDialog").size(), 1) << "one, kept";
    EXPECT_TRUE(dialog->isVisible()) << "the same one, shown again";

    // It holds the Document, so it goes with the workbench, which the window
    // destroys before the Document.
    const QPointer<QDialog> kept(dialog);
    bench.bench.reset();
    EXPECT_TRUE(kept.isNull());
}

TEST(CustomisationWorkbench, SettingsIsToldWhenTheCodeManagerHoldsUnappliedEditsAndRefusesToLoad)
{
    // By hand. The session starts as the built-in, test_symbols: no rule. One
    // rule added in the Survey Code Manager and not applied is in its buffer
    // alone. A file imported in Settings then would be undone by that
    // buffer's Apply, so Import is refused in the dialog's own words and NO
    // line is run; reverted, the same press runs the line, and the fixture's
    // 11 rules are on the drawing.
    KeptBench bench(false);
    katana::qt::SettingsDialog& settings = bench.bench->showSettings();
    const QString file =
        QDir::fromNativeSeparators(QString::fromStdU16String(
            (kFixture / "test_survey.customisation.json").u16string()));
    auto* path = named<QLineEdit>(&settings, "settingsImportPath");
    auto* import = named<QPushButton>(&settings, "settingsImport");
    auto* status = named<QPlainTextEdit>(&settings, "settingsStatus");
    ASSERT_FALSE(path == nullptr || import == nullptr || status == nullptr);
    path->setText(file);

    // The manager made AFTER the dialog: what is asked is asked at the press.
    katana::qt::SurveyCodeManagerDialog& codes = bench.bench->showCodeManager();
    katana::entity::SurveyRule rule;
    rule.key = "KQ*";
    rule.model = "KEPT";
    ASSERT_TRUE(codes.addRule(rule).ok());
    ASSERT_TRUE(codes.dirty());

    import->click();
    const QString refusal =
        "Import was not run: the Survey Code Manager has rule edits that are not on the "
        "drawing, and its Apply would then put the rules it holds back over what Import "
        "brought. Apply or Revert them first.";
    EXPECT_EQ(status->toPlainText(), refusal);
    EXPECT_TRUE(bench.ran.isEmpty()) << bench.ran.join(" | ").toStdString();
    ASSERT_FALSE(bench.log.empty());
    EXPECT_EQ(bench.log.back(), refusal) << "said in the window's log too";
    EXPECT_EQ(bench.document.surveyMap().size(), 0u);

    codes.revert();
    ASSERT_FALSE(codes.dirty());
    import->click();
    EXPECT_EQ(bench.ran, QStringList{"CUSTOMISE \"" + file + "\""});
    EXPECT_EQ(bench.document.surveyMap().size(), 11u);
}

TEST(CustomisationWorkbench, SettingsOpensTheEditorsAndTheCodeManagerAtItsLineworkTab)
{
    Bench bench;
    bench.loadFixture();
    katana::qt::SettingsDialog& settings = bench.bench->showSettings();
    ASSERT_EQ(bench.bench->codeManager(), nullptr);
    ASSERT_EQ(bench.bench->symbolLibrary(), nullptr);

    // Edit Linework Codes: the manager, made, shown, and on the tab that is
    // the control codes' one editor - its last, where it opens on its first.
    named<QPushButton>(&settings, "settingsEditLinework")->click();
    katana::qt::SurveyCodeManagerDialog* codes = bench.bench->codeManager();
    ASSERT_NE(codes, nullptr);
    EXPECT_TRUE(codes->isVisible());
    auto* tabs = named<QTabWidget>(codes, "codeManagerTabs");
    ASSERT_NE(tabs, nullptr);
    ASSERT_NE(tabs->currentWidget(), nullptr);
    EXPECT_EQ(tabs->currentWidget()->objectName(), "lineworkTab");
    EXPECT_NE(tabs->currentIndex(), 0);

    // Survey Codes shows the same manager as it was left - here put back on
    // its first tab - and does not move it.
    tabs->setCurrentIndex(0);
    codes->hide();
    named<QPushButton>(&settings, "settingsOpenCodes")->click();
    EXPECT_EQ(bench.bench->codeManager(), codes);
    EXPECT_TRUE(codes->isVisible());
    EXPECT_EQ(tabs->currentIndex(), 0);

    named<QPushButton>(&settings, "settingsOpenSymbols")->click();
    ASSERT_NE(bench.bench->symbolLibrary(), nullptr);
    EXPECT_TRUE(bench.bench->symbolLibrary()->isVisible());
    // Nothing was said of a tab that could not be found.
    for (const QString& line : bench.log) {
        EXPECT_FALSE(line.contains("Linework tab")) << line.toStdString();
    }
}

TEST(CustomisationWorkbench, SettingsShowsAndUsesWhatTheWindowsHostOffers)
{
    // A window that tells of no host (the first bench): no built-in to reset
    // to and no kept file, and the page says so in the dialog's words.
    {
        Bench bench;
        katana::qt::SettingsDialog& settings = bench.bench->showSettings();
        EXPECT_FALSE(named<QPushButton>(&settings, "settingsReset")->isEnabled());
        EXPECT_FALSE(named<QPushButton>(&settings, "settingsKeep")->isEnabled());
        EXPECT_FALSE(named<QPushButton>(&settings, "settingsRevert")->isEnabled());
        EXPECT_EQ(named<QLabel>(&settings, "settingsActiveKept")->text(),
                  "Not kept: the next start does not give this customisation.\nThis session "
                  "has no kept file; the environment variable KATANA_CUSTOMISATION names one.");
    }
    // One whose host has a built-in and a kept file. The session is the
    // built-in as started, which is kept, so Keep has nothing to do yet.
    KeptBench bench(true);
    katana::qt::SettingsDialog& settings = bench.bench->showSettings();
    EXPECT_TRUE(named<QPushButton>(&settings, "settingsReset")->isEnabled());
    EXPECT_FALSE(named<QPushButton>(&settings, "settingsKeep")->isEnabled());
    EXPECT_TRUE(named<QPushButton>(&settings, "settingsRevert")->isEnabled());
    EXPECT_EQ(named<QLabel>(&settings, "settingsActiveKept")->text(),
              "Kept: the next start gives this customisation.\nKept file: " +
                  bench.folder.path() + "/customisation.json");

    // An edit made in Settings goes through the window's executor, and - the
    // session having been the kept one, with a kept file - is followed by the
    // KEEP line there too: two lines, in that order, and the file is written.
    ASSERT_FALSE(std::filesystem::exists(bench.keptFile));
    auto* codes = named<QCheckBox>(&settings, "settingsAutoCodes");
    ASSERT_NE(codes, nullptr);
    ASSERT_TRUE(codes->isChecked()) << "on by default";
    codes->click();
    EXPECT_EQ(bench.ran, (QStringList{"CUSTOMISE SET auto.codes=off", "CUSTOMISE KEEP"}));
    EXPECT_FALSE(bench.document.customisationState().automation.codesOnSurveyImport);
    EXPECT_TRUE(bench.document.customisationState().kept);
    EXPECT_TRUE(std::filesystem::exists(bench.keptFile));
}
