// The Format menu's workbench (src/katana_qt/customisation/customisation_workbench):
// the menu and toolbar it fills, the three managers it opens - non-modal, one
// of each, kept between uses and deleted before the Document - the context it
// builds them from, and Purge Unused. Driven through its QActions, as a click
// on the menu drives it.
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
#include <QMainWindow>
#include <QMenu>
#include <QMessageBox>
#include <QTimer>
#include <QToolBar>

#include "customisation/code_manager.hpp"
#include "customisation/customisation_context.hpp"
#include "customisation/customisation_workbench.hpp"
#include "customisation/symbol_library.hpp"
#include "katana/archive12d/customisation.hpp"
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
                                       "archive12d" / "data" / "customisation";

// A window with a Format menu and toolbar, and the window's three shared
// actions, as MainWindow hands them over.
struct Bench {
    Document document;
    QMainWindow window;
    QMenu* menu = new QMenu("Format", &window);
    QToolBar* bar = new QToolBar("Format", &window);
    QAction* layers = new QAction("Layers...", &window);
    QAction* load = new QAction("Load 12d Customisation...", &window);
    QAction* replace = new QAction("Replace Loaded Customisation...", &window);
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
        }
        layers->setObjectName("formatLayers");
        load->setObjectName("loadCustomisation");
        replace->setObjectName("replaceCustomisation");
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
        services.loadCustomisation = load;
        services.replaceCustomisation = replace;
        bench = std::make_unique<CustomisationWorkbench>(window, std::move(services), *menu, *bar);
    }

    QAction& action(const char* name)
    {
        auto* found = window.findChild<QAction*>(name);
        EXPECT_NE(found, nullptr) << name;
        return *found;
    }

    void loadFixture()
    {
        auto loaded = katana::archive12d::readCustomisation(
            {kFixture / "test_linestyles.4d", kFixture / "test_survey.mapfile",
             kFixture / "test_symbols.4d"});
        ASSERT_TRUE(loaded.ok()) << loaded.error().describe();
        document.setStyleLibrary(loaded->library);
        document.setSurveyMap(loaded->map);
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

TEST(CustomisationWorkbench, TheFormatMenuOffersLayersTheManagersTheLoadsAndPurgeInThatOrder)
{
    Bench bench;
    EXPECT_EQ(names(bench.menu->actions()),
              (std::vector<std::string>{"formatLayers", "formatStyles", "formatSymbols",
                                        "formatSurveyCodes", "---", "loadCustomisation",
                                        "replaceCustomisation", "---", "formatPurge"}));
    EXPECT_EQ(names(bench.bar->actions()),
              (std::vector<std::string>{"formatStyles", "formatSymbols", "formatSurveyCodes"}));
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
    // view's centre is the line's middle, (1005, 1000).
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
