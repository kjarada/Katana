// The menus, toolbars and command-line words generated from the tool
// catalogue (src/katana_qt/tools/tool_menus), and the workspace starting a
// tool in its active plan view.

#include <gtest/gtest.h>

#include <algorithm>
#include <map>
#include <string>
#include <vector>

#include <QAction>
#include <QMainWindow>
#include <QMenu>
#include <QToolBar>
#include <QToolButton>

#include "katana/cad/interactive_tool.hpp"
#include "tools/tool_menus.hpp"
#include "view_workspace.hpp"
#include "widget_harness.hpp"

using katana::cad::toolCatalog;
using katana::qt::tools::fillToolMenus;
using katana::qt::tools::ToolActions;
using katana::qt::tools::toolIdForCommand;
using katana::qt::tools::ToolMenuTargets;

namespace {

// Every action a menu shows, its submenus' included, by object name, with
// how many times each appears.
void countActions(const QMenu& menu, std::map<std::string, int>& seen)
{
    for (QAction* action : menu.actions()) {
        if (action->isSeparator()) {
            continue;
        }
        if (QMenu* sub = action->menu()) {
            countActions(*sub, seen);
            continue;
        }
        ++seen[action->objectName().toStdString()];
    }
}

// One menu and one toolbar per category the catalogue has.
struct Menus {
    QMainWindow window;
    std::map<std::string, QMenu*> menus;
    std::map<std::string, QToolBar*> bars;
    std::vector<std::string> started;
    ToolActions actions;

    Menus()
    {
        ToolMenuTargets targets;
        for (const auto* info : toolCatalog().all()) {
            if (!menus.contains(info->category)) {
                menus[info->category] = new QMenu(QString::fromStdString(info->category), &window);
                bars[info->category] =
                    new QToolBar(QString::fromStdString(info->category), &window);
            }
        }
        targets.menus = menus;
        targets.toolBars = bars;
        actions = fillToolMenus(toolCatalog(), targets, &window,
                                [this](const std::string& id) { started.push_back(id); });
    }
};

} // namespace

TEST(ToolMenus, EveryCatalogueToolIsInAMenuExactlyOnceNamedByItsId)
{
    Menus built;
    std::map<std::string, int> seen;
    for (const auto& [category, menu] : built.menus) {
        countActions(*menu, seen);
    }
    ASSERT_GT(toolCatalog().size(), 0u);
    for (const auto* info : toolCatalog().all()) {
        EXPECT_EQ(seen[info->id], 1) << info->id;
    }
    // And nothing in the menus that is not a catalogue tool.
    EXPECT_EQ(seen.size(), toolCatalog().size());
    EXPECT_TRUE(built.actions.unplaced().empty());
}

TEST(ToolMenus, EachToolIsInTheMenuOfItsOwnCategory)
{
    Menus built;
    for (const auto* info : toolCatalog().all()) {
        std::map<std::string, int> seen;
        countActions(*built.menus.at(info->category), seen);
        EXPECT_EQ(seen[info->id], 1) << info->id << " is not in " << info->category;
    }
}

TEST(ToolMenus, AnActionCarriesItsIconTipAndAliasesAndStartsItsTool)
{
    Menus built;
    QAction* line = built.actions.action("draw.line");
    ASSERT_NE(line, nullptr);
    EXPECT_EQ(line->objectName(), "draw.line");
    EXPECT_FALSE(line->icon().isNull());
    const auto* info = toolCatalog().find("draw.line");
    EXPECT_EQ(line->statusTip().toStdString(), info->tip);
    // The aliases as AutoCAD's tooltips give them: "(LINE, L)".
    EXPECT_TRUE(line->toolTip().contains("(LINE, L)")) << line->toolTip().toStdString();

    line->trigger();
    ASSERT_EQ(built.started.size(), 1u);
    EXPECT_EQ(built.started.front(), "draw.line");
}

TEST(ToolMenus, ACircleVariantIsItemInTheCircleSubmenuNamedByItsVariant)
{
    // "Circle, 2 Points" is the Circle family's "2 Points".
    Menus built;
    auto* family = built.window.findChild<QMenu*>("toolFamily.Draw.Circle");
    ASSERT_NE(family, nullptr);
    QAction* twoPoints = nullptr;
    for (QAction* action : family->actions()) {
        if (action->objectName() == "draw.circle.2p") {
            twoPoints = action;
        }
    }
    ASSERT_NE(twoPoints, nullptr);
    EXPECT_EQ(twoPoints->text(), "2 Points");
    EXPECT_TRUE(twoPoints->toolTip().contains("Circle, 2 Points"));
    // On the toolbar the family is one button that runs its first variant.
    auto* button = built.window.findChild<QToolButton*>("toolFamilyButton.Draw.Circle");
    ASSERT_NE(button, nullptr);
    EXPECT_EQ(button->defaultAction()->objectName(), "draw.circle");
}

TEST(ToolMenus, LinesComeBeforeCurvesInTheDrawMenu)
{
    Menus built;
    const QList<QAction*> items = built.menus.at("Draw")->actions();
    ASSERT_FALSE(items.isEmpty());
    EXPECT_EQ(items.front()->objectName(), "draw.point") << "the Lines group, order 10";
}

TEST(ToolMenus, TheRunningToolsActionIsCheckedAndNoneWhenItEnds)
{
    Menus built;
    built.actions.setActive("modify.trim");
    EXPECT_TRUE(built.actions.action("modify.trim")->isChecked());
    built.actions.setActive("draw.line");
    EXPECT_TRUE(built.actions.action("draw.line")->isChecked());
    EXPECT_FALSE(built.actions.action("modify.trim")->isChecked());
    built.actions.setActive("");
    EXPECT_FALSE(built.actions.action("draw.line")->isChecked());
}

TEST(ToolMenus, ACategoryWithNoMenuIsReportedNotLost)
{
    QMainWindow window;
    ToolMenuTargets targets;
    targets.menus["Draw"] = new QMenu("Draw", &window);
    const ToolActions actions = fillToolMenus(toolCatalog(), targets, &window, {});
    EXPECT_NE(std::ranges::find(actions.unplaced(), std::string("Modify")), actions.unplaced().end());
    // Its tools still have actions, for the window to place as it likes.
    EXPECT_NE(actions.action("modify.move"), nullptr);
}

TEST(ToolMenus, TypedAliasesStartTheirToolsInAnyCase)
{
    EXPECT_EQ(toolIdForCommand("L"), "draw.line");
    EXPECT_EQ(toolIdForCommand("LINE"), "draw.line");
    EXPECT_EQ(toolIdForCommand("line"), "draw.line");
    EXPECT_EQ(toolIdForCommand(" TR "), "modify.trim");
    EXPECT_EQ(toolIdForCommand("draw.circle.ttr"), "draw.circle.ttr") << "an id is a word too";
    EXPECT_EQ(toolIdForCommand("ZOOM"), std::nullopt) << "a command, not a tool";
    EXPECT_EQ(toolIdForCommand(""), std::nullopt);
}

TEST(ToolMenus, TheWorkspaceStartsAToolInItsActivePlanViewOnly)
{
    katana::cad::Document document;
    QMainWindow window;
    auto* views = new katana::qt::ViewWorkspace(document, &window);
    window.setCentralWidget(views);
    std::vector<std::string> changes;
    views->onActiveToolChanged = [&](const std::string& id) { changes.push_back(id); };

    ASSERT_TRUE(views->startTool("draw.circle").ok());
    EXPECT_EQ(views->activeToolId(), "draw.circle");
    ASSERT_NE(views->activePlanView(), nullptr);
    EXPECT_EQ(views->activePlanView()->activeToolId(), "draw.circle");

    // The command line's Enter hands its line to the running tool: the
    // centre, then the radius. Circle restarts for the next one.
    EXPECT_TRUE(views->typeIntoTool("3,4"));
    EXPECT_TRUE(views->typeIntoTool("2"));
    EXPECT_EQ(document.model().entities.size(), 1u);

    EXPECT_EQ(views->startTool("draw.nothing").error().code, katana::core::ErrorCode::NotFound);
    views->setTool(katana::qt::Tool::Select);
    EXPECT_EQ(views->activeToolId(), "");
    EXPECT_FALSE(views->typeIntoTool("1,1")) << "no tool: the line is the command line's";
    ASSERT_FALSE(changes.empty());
    EXPECT_EQ(changes.front(), "draw.circle");
    EXPECT_EQ(changes.back(), "");
}
