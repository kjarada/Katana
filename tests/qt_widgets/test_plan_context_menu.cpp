// The plan view's shortcut menu and Edit > Select by ID
// (src/katana_qt/plan_context_menu, select_by_id_dialog): what the menu
// offers with and without a selection, that its own items run the verb line
// a person would type through the runner - one undo step each - and that the
// view raises the menu and the double click with the right selection.
//
// The runner here is the window's in miniature: each line is kept, then run
// by a CommandInterpreter on the same Document, and what it answered comes
// back as the window's runner returns it.
//
// The plan view is 400 x 300 pixels, centred on the origin at 10 pixels a
// unit (test_plan_view_tools.cpp works the same view by hand), so pixel
// (250, 150) is model (5, 0).

#include <gtest/gtest.h>

#include <algorithm>
#include <memory>
#include <optional>
#include <string>
#include <vector>

#include <QAction>
#include <QCheckBox>
#include <QLabel>
#include <QLineEdit>
#include <QMainWindow>
#include <QMouseEvent>
#include <QPushButton>

#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/view_set.hpp"
#include "katana/commands/entity_commands.hpp"
#include "plan_context_menu.hpp"
#include "select_by_id_dialog.hpp"
#include "view_workspace.hpp"
#include "widget_harness.hpp"

using katana::cad::CommandInterpreter;
using katana::cad::Document;
using katana::cad::ViewKind;
using katana::cad::ViewSet;
using katana::cad::ViewState;
using katana::entity::EntityId;
using katana::geometry::Point2;
using katana::qt::CommandRunner;
using katana::qt::PlanContextMenu;
using katana::qt::PlanContextMenuContext;
using katana::qt::SelectByIdContext;
using katana::qt::SelectByIdDialog;
using katana::qt::VerbOutcome;
using katana::qt::ViewportWidget;
using katana::qt::ViewWorkspace;

namespace {

QString qs(const std::string& text) { return QString::fromStdString(text); }

// A drawing, the interpreter the runner runs lines with, and a window of
// actions for the menu to find by object name, as it finds the main
// window's.
struct MenuFixture {
    Document document;
    CommandInterpreter interpreter{document};
    QWidget window;
    std::vector<QString> lines;

    // An action of the window's, counting its triggers.
    QAction* action(const QString& name, const QString& text = {})
    {
        auto* made = new QAction(text.isEmpty() ? name : text, &window);
        made->setObjectName(name);
        QObject::connect(made, &QAction::triggered, &window, [this, name] {
            triggered.push_back(name);
        });
        return made;
    }
    std::vector<QString> triggered;

    CommandRunner runner()
    {
        return [this](const QString& line) {
            lines.push_back(line);
            const auto reply = interpreter.run(line.toStdString());
            return reply ? VerbOutcome{true, qs(*reply).trimmed(), {}}
                         : VerbOutcome{false, {}, qs(reply.error().describe())};
        };
    }
    void run(const std::string& line)
    {
        const auto reply = interpreter.run(line);
        ASSERT_TRUE(reply.ok()) << line << ": " << reply.error().describe();
    }
    PlanContextMenuContext context()
    {
        PlanContextMenuContext made;
        made.document = &document;
        made.actions = &window;
        made.run = runner();
        return made;
    }
    const katana::entity::Entity& entity(EntityId id) const
    {
        return *document.model().entities.find(id);
    }
};

// The menu's items in order, a submenu by its own name, separators left out.
std::vector<QString> itemsOf(const QMenu& menu)
{
    std::vector<QString> names;
    for (const QAction* item : menu.actions()) {
        if (item->isSeparator()) {
            continue;
        }
        names.push_back(item->menu() != nullptr ? item->menu()->objectName() : item->objectName());
    }
    return names;
}

QAction* item(const QMenu& menu, const QString& name)
{
    QAction* found = menu.findChild<QAction*>(name);
    EXPECT_NE(found, nullptr) << "the menu has no " << name.toStdString();
    return found;
}

} // namespace

TEST(PlanContextMenu, WithASelectionItOffersTheWindowsActionsAndTheSelectionsVerbs)
{
    MenuFixture f;
    for (const char* name : {"editErase", "modify.move", "modify.rotate", "editAttributes",
                             "inquiry.list", "select.similar", "formatGlobalModify",
                             "editDeselect", "editSelectAll", "viewZoomExtents"}) {
        f.action(name);
    }
    f.run("LINE 0,0 10,0");
    f.run("SELECT 1");

    PlanContextMenu menu(f.context());
    EXPECT_EQ(menu.objectName(), "planContextMenu");
    // The window's own actions, found by name - the same QAction as the menu
    // bar's, not a copy - and the menu's items between them. Selecting all
    // and zooming are for a menu with nothing selected.
    EXPECT_EQ(itemsOf(menu),
              (std::vector<QString>{"editErase", "modify.move", "modify.rotate",
                                    "planContextLayer", "planContextStyle", "planContextColour",
                                    "editAttributes", "planContextInfo", "inquiry.list",
                                    "select.similar", "formatGlobalModify", "editDeselect"}));
    EXPECT_EQ(menu.findChild<QAction*>("editErase"), nullptr)
        << "the window's action is shown, never made again as the menu's child";
    EXPECT_TRUE(menu.actions().contains(f.window.findChild<QAction*>("editErase")));
}

TEST(PlanContextMenu, AnEditorTheWindowDoesNotHaveIsLeftOutAndOneItHasIsOfferedForItsKind)
{
    MenuFixture f;
    f.run("TEXT 0,0 2.5 \"kerb\"");
    f.run("LINE 0,0 10,0");
    f.run("SELECT 1");
    {
        PlanContextMenu menu(f.context());
        EXPECT_EQ(std::ranges::count(itemsOf(menu), QString("annotateEditText")), 0);
    }
    f.action("annotateEditText");
    {
        PlanContextMenu menu(f.context());
        EXPECT_EQ(std::ranges::count(itemsOf(menu), QString("annotateEditText")), 1);
    }
    // A line is no text to edit.
    f.run("SELECT 2");
    PlanContextMenu menu(f.context());
    EXPECT_EQ(std::ranges::count(itemsOf(menu), QString("annotateEditText")), 0);
}

TEST(PlanContextMenu, WithNothingSelectedItOffersSelectingZoomingAndTheLastToolAgain)
{
    MenuFixture f;
    for (const char* name : {"editErase", "editSelectAll", "editSelectById", "viewZoomExtents"}) {
        f.action(name);
    }
    f.action("draw.circle", "Circle");
    PlanContextMenuContext context = f.context();
    context.lastToolId = "draw.circle";
    PlanContextMenu menu(context);
    EXPECT_EQ(itemsOf(menu), (std::vector<QString>{"planContextRepeat", "editSelectAll",
                                                   "editSelectById", "viewZoomExtents"}));
    QAction* repeat = item(menu, "planContextRepeat");
    ASSERT_NE(repeat, nullptr);
    EXPECT_EQ(repeat->text(), "Repeat Circle");
    repeat->trigger();
    EXPECT_EQ(f.triggered, (std::vector<QString>{"draw.circle"}))
        << "Repeat is the tool's own action, so it starts as a click on Circle does";

    // No tool run yet: nothing to repeat.
    PlanContextMenu fresh(f.context());
    EXPECT_EQ(fresh.findChild<QAction*>("planContextRepeat"), nullptr);
}

TEST(PlanContextMenu, PutOnLayerRunsChlayerThroughTheRunnerAsOneUndoStep)
{
    MenuFixture f;
    f.run("LAYER NEW walls");
    f.run("LINE 0,0 10,0");
    f.run("SELECT 1");
    const std::size_t steps = f.document.history().undoCount();

    PlanContextMenu menu(f.context());
    QAction* walls = item(menu, "planContextLayer.walls");
    ASSERT_NE(walls, nullptr);
    walls->trigger();

    EXPECT_EQ(f.lines, (std::vector<QString>{"CHLAYER walls"}));
    EXPECT_EQ(f.entity(1).layer, "walls");
    EXPECT_EQ(f.document.history().undoCount(), steps + 1);
    ASSERT_TRUE(f.document.undo().ok());
    EXPECT_EQ(f.entity(1).layer, "0");
}

TEST(PlanContextMenu, LayersUnderALayerAreASubmenuHeadedByThatLayerAndBlanksAreQuoted)
{
    MenuFixture f;
    f.run("LAYER NEW site/kerb/top");
    f.run("LAYER NEW \"old work\"");
    f.run("LINE 0,0 10,0");
    f.run("SELECT 1");
    PlanContextMenu menu(f.context());

    // LAYER NEW made site and site/kerb as well; each is a submenu of what
    // is under it, headed by the layer itself.
    const auto* site = menu.findChild<QMenu*>("planContextLayerTree.site");
    ASSERT_NE(site, nullptr);
    EXPECT_EQ(itemsOf(*site),
              (std::vector<QString>{"planContextLayer.site", "planContextLayerTree.site/kerb"}));
    const auto* kerb = menu.findChild<QMenu*>("planContextLayerTree.site/kerb");
    ASSERT_NE(kerb, nullptr);
    EXPECT_EQ(itemsOf(*kerb), (std::vector<QString>{"planContextLayer.site/kerb",
                                                     "planContextLayer.site/kerb/top"}));
    const auto* top = menu.findChild<QMenu*>("planContextLayer");
    ASSERT_NE(top, nullptr);
    EXPECT_EQ(itemsOf(*top), (std::vector<QString>{"planContextLayer.0",
                                                    "planContextLayer.old work",
                                                    "planContextLayerTree.site"}));

    item(menu, "planContextLayer.site/kerb/top")->trigger();
    item(menu, "planContextLayer.old work")->trigger();
    EXPECT_EQ(f.lines,
              (std::vector<QString>{"CHLAYER site/kerb/top", "CHLAYER \"old work\""}));
    EXPECT_EQ(f.entity(1).layer, "old work");
}

TEST(PlanContextMenu, TheLayerTheWholeSelectionIsOnIsTickedAndNoneWhenItIsOnSeveral)
{
    MenuFixture f;
    f.run("LAYER NEW walls");
    f.run("LINE 0,0 10,0");
    f.run("LINE 0,5 10,5");
    f.run("SELECT 1 2");
    f.run("CHLAYER walls");
    {
        PlanContextMenu menu(f.context());
        EXPECT_TRUE(item(menu, "planContextLayer.walls")->isChecked());
        EXPECT_FALSE(item(menu, "planContextLayer.0")->isChecked());
    }
    f.run("SELECT 1");
    f.run("CHLAYER 0");
    f.run("SELECT 1 2");
    PlanContextMenu menu(f.context());
    EXPECT_FALSE(item(menu, "planContextLayer.walls")->isChecked());
    EXPECT_FALSE(item(menu, "planContextLayer.0")->isChecked());
}

TEST(PlanContextMenu, StyleAndColourRunStyleApplyAndColorAndACancelledColourRunsNothing)
{
    MenuFixture f;
    f.run("STYLE NEW fence");
    f.run("LINE 0,0 10,0");
    f.run("SELECT 1");
    std::optional<QColor> answer = QColor(255, 0, 0);
    QColor offered;
    PlanContextMenuContext context = f.context();
    context.chooseColour = [&](const QColor& initial) {
        offered = initial;
        return answer;
    };
    {
        PlanContextMenu menu(context);
        EXPECT_TRUE(item(menu, "planContextStyle.ByLayer")->isChecked());
        item(menu, "planContextStyle.fence")->trigger();
        item(menu, "planContextColour.Choose")->trigger();
    }
    EXPECT_EQ(f.entity(1).style, "fence");
    ASSERT_TRUE(f.entity(1).color.has_value());
    EXPECT_EQ(f.entity(1).color->toHex(), "#FF0000");
    {
        PlanContextMenu menu(context);
        EXPECT_TRUE(item(menu, "planContextStyle.fence")->isChecked());
        EXPECT_FALSE(item(menu, "planContextStyle.ByLayer")->isChecked());
        answer.reset(); // the colour dialog cancelled
        item(menu, "planContextColour.Choose")->trigger();
        EXPECT_EQ(offered, QColor(255, 0, 0)) << "the dialog starts from the selection's colour";
        item(menu, "planContextStyle.ByLayer")->trigger();
        item(menu, "planContextColour.ByLayer")->trigger();
    }
    EXPECT_EQ(f.lines, (std::vector<QString>{"STYLE APPLY fence", "COLOR #FF0000",
                                             "STYLE APPLY -", "COLOR BYLAYER"}));
    EXPECT_TRUE(f.entity(1).style.empty());
    EXPECT_FALSE(f.entity(1).color.has_value());
}

TEST(PlanContextMenu, EntityInformationIsForOneEntityAndRunsInfo)
{
    MenuFixture f;
    f.run("RECT 0,0 10,5");
    f.run("POINT 20,20");
    f.run("SELECT 1");
    VerbOutcome outcome;
    PlanContextMenuContext context = f.context();
    const CommandRunner inner = context.run;
    context.run = [&](const QString& line) { return outcome = inner(line); };
    {
        PlanContextMenu menu(context);
        item(menu, "planContextInfo")->trigger();
    }
    EXPECT_EQ(f.lines, (std::vector<QString>{"INFO 1"}));
    EXPECT_TRUE(outcome.ok);
    // A 10 x 5 rectangle: perimeter 30, area 50.
    EXPECT_EQ(outcome.reply, "1  Polyline  layer=0  vertices=4  closed  length=30  area=50");

    f.run("SELECT 1 2");
    PlanContextMenu menu(context);
    EXPECT_EQ(menu.findChild<QAction*>("planContextInfo"), nullptr);
}

TEST(PlanContextMenu, AnArgumentIsQuotedOnlyWhenItMustBe)
{
    using katana::qt::verbArgument;
    EXPECT_EQ(verbArgument("walls"), "walls");
    EXPECT_EQ(verbArgument("site/kerb"), "site/kerb");
    EXPECT_EQ(verbArgument("old work"), "\"old work\"");
    EXPECT_EQ(verbArgument(""), "\"\"");
}

// ---- the plan view raises it ----------------------------------------------------------------

namespace {

// A plan view, as test_plan_view_tools.cpp makes one, with its two hooks
// recorded.
struct PlanFixture {
    Document document;
    ViewSet views;
    ViewState& state;
    ViewportWidget view;
    int menus = 0;
    std::vector<EntityId> doubleClicked;

    PlanFixture() : state(views.add(ViewKind::Plan)), view(document, state)
    {
        state.planFramed = true;
        state.plan.center = Point2(0.0, 0.0);
        state.plan.scale = 10.0;
        state.plan.resize(400.0, 300.0);
        view.resize(400, 300);
        view.setSnapEnabled(false);
        view.onContextMenu = [this](const QPoint&) { ++menus; };
        view.onEntityDoubleClicked = [this](EntityId id) { doubleClicked.push_back(id); };
        katana::qt::test::paint(view);
    }
    EntityId line(Point2 start, Point2 end)
    {
        EXPECT_TRUE(document.execute(katana::commands::createLine(start, end)).ok());
        return document.lastCreatedEntities().front();
    }
    void send(QEvent::Type type, double x, double y, Qt::MouseButton button)
    {
        const QPointF at(x, y);
        QMouseEvent event(type, at, view.mapToGlobal(at), button,
                          type == QEvent::MouseButtonRelease ? Qt::NoButton : button,
                          Qt::NoModifier);
        QCoreApplication::sendEvent(&view, &event);
    }
    void click(double x, double y, Qt::MouseButton button = Qt::LeftButton)
    {
        send(QEvent::MouseButtonPress, x, y, button);
        send(QEvent::MouseButtonRelease, x, y, button);
    }
    // As Qt delivers one: press, release, double click, release.
    void doubleClick(double x, double y)
    {
        click(x, y);
        send(QEvent::MouseButtonDblClick, x, y, Qt::LeftButton);
        send(QEvent::MouseButtonRelease, x, y, Qt::LeftButton);
    }
};

} // namespace

TEST(PlanViewShortcutMenu, ARightClickOnAnEntityWithNothingSelectedSelectsItForTheMenu)
{
    PlanFixture plan;
    const EntityId id = plan.line(Point2(0, 0), Point2(10, 0));
    plan.click(250, 150, Qt::RightButton); // (5, 0), on the line
    EXPECT_EQ(plan.menus, 1);
    EXPECT_EQ(plan.document.selection().ids(), (std::vector<EntityId>{id}));

    // On empty space with nothing selected: the menu, with nothing to act on.
    plan.document.selection().clear();
    plan.click(250, 50, Qt::RightButton); // (5, 10)
    EXPECT_EQ(plan.menus, 2);
    EXPECT_TRUE(plan.document.selection().empty());
}

TEST(PlanViewShortcutMenu, ARightClickKeepsTheSelectionItFindsThere)
{
    PlanFixture plan;
    const EntityId first = plan.line(Point2(0, 0), Point2(10, 0));
    const EntityId second = plan.line(Point2(0, 10), Point2(10, 10));
    plan.document.selection().set({first});
    plan.click(250, 50, Qt::RightButton); // on the second line
    EXPECT_EQ(plan.menus, 1);
    EXPECT_EQ(plan.document.selection().ids(), (std::vector<EntityId>{first}))
        << "the menu is for the selection, which a click beside it must not change";
    (void)second;
}

TEST(PlanViewShortcutMenu, ADoubleClickOnAnEntityAsksToEditItAndEmptySpaceAsksNothing)
{
    PlanFixture plan;
    const EntityId id = plan.line(Point2(0, 0), Point2(10, 0));
    plan.doubleClick(250, 150);
    EXPECT_EQ(plan.doubleClicked, (std::vector<EntityId>{id}));
    EXPECT_EQ(plan.document.selection().ids(), (std::vector<EntityId>{id}));
    plan.doubleClick(250, 50);
    EXPECT_EQ(plan.doubleClicked.size(), 1u);
}

TEST(PlanViewShortcutMenu, ADoubleClickWhileAToolRunsIsTheToolsClickNotAnEdit)
{
    PlanFixture plan;
    (void)plan.line(Point2(0, 0), Point2(10, 0));
    ASSERT_TRUE(plan.view.startTool("draw.line").ok());
    plan.doubleClick(250, 150);
    EXPECT_TRUE(plan.doubleClicked.empty());
    plan.click(250, 150, Qt::RightButton); // Enter, not the menu
    EXPECT_EQ(plan.menus, 0);
}

TEST(PlanViewShortcutMenu, TheWorkspaceGivesTheMenuFromAViewMadeBeforeItsHookWasSet)
{
    // The workspace makes its first plan view in its constructor, before the
    // window sets the workspace's hooks: the view must still reach them.
    Document document;
    QMainWindow window;
    auto* views = new ViewWorkspace(document, &window);
    window.setCentralWidget(views);
    window.resize(800, 600);
    window.show();
    katana::qt::test::processEvents();
    int menus = 0;
    std::vector<EntityId> edited;
    views->onContextMenu = [&](const QPoint&) { ++menus; };
    views->onEntityDoubleClicked = [&](EntityId id) { edited.push_back(id); };
    ViewportWidget* plan = views->activePlanView();
    ASSERT_NE(plan, nullptr);

    const QPointF middle(plan->width() / 2.0, plan->height() / 2.0);
    QMouseEvent down(QEvent::MouseButtonPress, middle, plan->mapToGlobal(middle), Qt::RightButton,
                     Qt::RightButton, Qt::NoModifier);
    QCoreApplication::sendEvent(plan, &down);
    EXPECT_EQ(menus, 1);

    // Unset, a right-click cancels as it always did: the selection goes.
    views->onContextMenu = nullptr;
    ASSERT_TRUE(document.execute(katana::commands::createLine(Point2(0, 0), Point2(1, 0))).ok());
    document.selection().set({document.lastCreatedEntities().front()});
    QCoreApplication::sendEvent(plan, &down);
    EXPECT_EQ(menus, 1);
    EXPECT_TRUE(document.selection().empty());
}

// ---- Select by ID ---------------------------------------------------------------------------

TEST(SelectById, IdsAreReadAcrossCommasAndBlanksWithOrWithoutAHashEachOnce)
{
    const auto ids = katana::qt::parseEntityIds(" 3, 5  #7,3\t12 ");
    ASSERT_TRUE(ids.ok()) << ids.error().describe();
    EXPECT_EQ(*ids, (std::vector<EntityId>{3, 5, 7, 12}));
}

TEST(SelectById, AWordThatIsNotAnIdIsRefusedByNameAndNoIdsIsRefused)
{
    for (const char* text : {"3, x5", "3 0", "#", "1.5", "-2"}) {
        const auto ids = katana::qt::parseEntityIds(text);
        ASSERT_FALSE(ids.ok()) << text;
        EXPECT_EQ(ids.error().code, katana::core::ErrorCode::InvalidArgument) << text;
    }
    EXPECT_NE(katana::qt::parseEntityIds("3, x5").error().describe().find("x5"),
              std::string::npos);
    for (const char* text : {"", "  ", ", ,"}) {
        const auto ids = katana::qt::parseEntityIds(text);
        ASSERT_FALSE(ids.ok()) << text;
        EXPECT_NE(ids.error().describe().find("no id"), std::string::npos) << text;
    }
}

TEST(SelectById, AddingNamesTheCurrentSelectionFirstAndEachIdOnce)
{
    using katana::qt::selectByIdLine;
    EXPECT_EQ(selectByIdLine({5, 2}, {2, 9}, true), "SELECT 2 9 5");
    EXPECT_EQ(selectByIdLine({5, 2}, {2, 9}, false), "SELECT 5 2");
    EXPECT_EQ(selectByIdLine({5}, {}, true), "SELECT 5");
}

namespace {

struct DialogFixture : MenuFixture {
    std::vector<bool> selected; // each onSelected, with its zoom
    std::unique_ptr<SelectByIdDialog> dialog;

    DialogFixture()
    {
        SelectByIdContext context;
        context.document = &document;
        context.run = runner();
        context.onSelected = [this](bool zoom) { selected.push_back(zoom); };
        dialog = std::make_unique<SelectByIdDialog>(std::move(context));
    }
    template <class Widget> Widget* find(const char* name) const
    {
        auto* widget = dialog->findChild<Widget*>(name);
        EXPECT_NE(widget, nullptr) << name;
        return widget;
    }
    void select(const QString& ids)
    {
        find<QLineEdit>("selectByIdIds")->setText(ids);
        find<QPushButton>("selectByIdSelect")->click();
    }
    QString status() const { return find<QLabel>("selectByIdStatus")->text(); }
};

} // namespace

TEST(SelectByIdDialog, SelectRunsSelectThroughTheRunnerAndAsksForWhatItSelectedToBeFramed)
{
    DialogFixture f;
    EXPECT_EQ(f.dialog->objectName(), "selectByIdDialog");
    f.run("POINT 1,1");
    f.run("POINT 2,2");
    f.run("POINT 3,3");
    EXPECT_TRUE(f.find<QCheckBox>("selectByIdZoom")->isChecked()) << "framing is the default";
    f.select("#2, 3");
    EXPECT_EQ(f.lines, (std::vector<QString>{"SELECT 2 3"}));
    EXPECT_EQ(f.document.selection().ids(), (std::vector<EntityId>{2, 3}));
    EXPECT_EQ(f.status(), "2 selected");
    EXPECT_EQ(f.selected, (std::vector<bool>{true}));

    // Added to, without framing.
    f.find<QCheckBox>("selectByIdAdd")->setChecked(true);
    f.find<QCheckBox>("selectByIdZoom")->setChecked(false);
    f.select("1");
    EXPECT_EQ(f.lines.back(), "SELECT 2 3 1");
    EXPECT_EQ(f.document.selection().size(), 3u);
    EXPECT_EQ(f.selected, (std::vector<bool>{true, false}));
}

TEST(SelectByIdDialog, AnIdSelectRefusesIsSaidAndTheSelectionIsLeftAlone)
{
    DialogFixture f;
    f.run("POINT 1,1");
    f.run("LAYER NEW hidden");
    f.run("POINT 2,2");
    f.run("SELECT 2");
    f.run("CHLAYER hidden");
    f.run("LAYER HIDE hidden");
    f.run("SELECT 1");

    f.select("1 99");
    EXPECT_NE(f.status().indexOf("entity does not exist"), -1) << f.status().toStdString();
    EXPECT_NE(f.status().indexOf("99"), -1) << f.status().toStdString();
    f.select("2");
    EXPECT_NE(f.status().indexOf("hidden or locked"), -1) << f.status().toStdString();
    // Not an id at all: refused before anything runs.
    f.select("kerb");
    EXPECT_NE(f.status().indexOf("kerb"), -1) << f.status().toStdString();
    EXPECT_EQ(f.lines, (std::vector<QString>{"SELECT 1 99", "SELECT 2"}));
    EXPECT_EQ(f.document.selection().ids(), (std::vector<EntityId>{1}));
    EXPECT_TRUE(f.selected.empty());
}
