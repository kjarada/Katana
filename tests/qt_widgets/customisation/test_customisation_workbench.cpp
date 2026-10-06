// The Format menu's workbench (src/katana_qt/customisation/customisation_workbench):
// the menu and toolbar it fills, the three managers it opens - non-modal, one
// of each, kept between uses and deleted before the Document - the context it
// builds them from, and Purge Unused. Driven through its QActions, as a click
// on the menu drives it.
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
#include <string>
#include <vector>

#include <QAbstractButton>
#include <QAction>
#include <QApplication>
#include <QComboBox>
#include <QLabel>
#include <QLineEdit>
#include <QMainWindow>
#include <QMenu>
#include <QMessageBox>
#include <QPushButton>
#include <QTimer>
#include <QToolBar>

#include "command_runner.hpp"
#include "customisation/code_manager.hpp"
#include "customisation/customisation_context.hpp"
#include "customisation/customisation_workbench.hpp"
#include "customisation/symbol_library.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/commands/entity_commands.hpp"
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
