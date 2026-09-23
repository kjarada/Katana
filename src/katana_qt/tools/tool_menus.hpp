#pragma once

// The window's tool menus and toolbars, generated from the tool catalogue
// (include/katana/cad/interactive_tool.hpp) rather than listed by hand: a tool
// is added by writing it and adding it to its family, and it appears in its
// menu, its toolbar and the command line with no edit to the window.
//
// Each tool gets ONE QAction, shared by its menu and its toolbar, named by the
// tool's id (so tests and the headless driver find it by objectName), with its
// family's icon (tool_icons.hpp), its shortcut, its tip as the status tip, and
// a tooltip naming its command-line aliases the way AutoCAD's do.
//
// Layout, as a CAD user expects it:
//   - one menu and one toolbar per category (Draw, Modify, Annotate, ...);
//   - a separator between groups (Lines | Curves), groups in kGroupOrder;
//   - tools named "Family, Variant" ("Circle, 2 Points") gathered into a
//     submenu named by the family, each item by its variant, as AutoCAD's Draw
//     menu gathers Circle and Arc; on a toolbar the family is ONE button that
//     runs its first variant and drops the rest down.

#include <functional>
#include <map>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/cad/interactive_tool.hpp"

class QAction;
class QActionGroup;
class QMenu;
class QObject;
class QToolBar;

namespace katana::qt::tools {

using StartTool = std::function<void(const std::string& toolId)>;

// The order groups appear in within a category's menu: the everyday ones
// first, as AutoCAD puts Line before Circle. A group not listed follows the
// listed ones, in the catalogue's (alphabetical) order.
inline const std::vector<std::string> kGroupOrder = {
    "Lines", "Curves", "Transform", "Edit", "Text", "Dimensions", "Leaders",
};

// Where the tools go. A category with no menu here is left out of the menus
// (and listed in ToolActions::unplaced); likewise for toolbars, which are
// optional.
struct ToolMenuTargets {
    std::map<std::string, QMenu*> menus;       // by category: "Draw" -> the Draw menu
    std::map<std::string, QToolBar*> toolBars; // by category
};

// What fillToolMenus made: the actions by tool id, so the window can find
// them, and check the running tool's.
class ToolActions {
  public:
    [[nodiscard]] QAction* action(std::string_view toolId) const;
    [[nodiscard]] const std::map<std::string, QAction*, std::less<>>& all() const
    {
        return actions_;
    }
    // Checks the running tool's action and unchecks the rest; "" unchecks
    // every one. Wire it to ViewWorkspace::onActiveToolChanged.
    void setActive(std::string_view toolId) const;
    // Categories that had tools but no menu to put them in.
    [[nodiscard]] const std::vector<std::string>& unplaced() const { return unplaced_; }

  private:
    friend ToolActions fillToolMenus(const katana::cad::ToolCatalog&, const ToolMenuTargets&,
                                     QObject*, StartTool);
    std::map<std::string, QAction*, std::less<>> actions_;
    QActionGroup* group_ = nullptr;
    std::vector<std::string> unplaced_;
};

// Fills `targets` from `catalog` and returns the actions. Each action, when
// triggered, calls `start` with its tool's id - never starts anything itself,
// so the window decides which view the tool runs in. `owner` owns the actions
// and the submenus (the main window).
ToolActions fillToolMenus(const katana::cad::ToolCatalog& catalog, const ToolMenuTargets& targets,
                          QObject* owner, StartTool start);

// The tool a word typed at the command line starts: an alias ("L", "line",
// "TRIM"), case-insensitively, or a tool's own id ("draw.line"). Nullopt for a
// word that starts no tool - it is then a command for the interpreter.
[[nodiscard]] std::optional<std::string> toolIdForCommand(std::string_view word);

// The text a tool's action shows as its tooltip: the name, the aliases and
// the shortcut, then the tip - "<b>Line</b> (LINE, L)<br>Draws ...".
[[nodiscard]] std::string toolTooltip(const katana::cad::ToolInfo& info);
// The family and variant of a name "Circle, 2 Points"; nullopt for a name
// with no comma.
[[nodiscard]] std::optional<std::pair<std::string, std::string>>
familyAndVariant(std::string_view name);

} // namespace katana::qt::tools
