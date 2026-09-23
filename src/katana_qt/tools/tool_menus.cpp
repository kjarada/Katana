#include "tools/tool_menus.hpp"

#include <algorithm>
#include <utility>

#include <QAction>
#include <QActionGroup>
#include <QKeySequence>
#include <QMenu>
#include <QString>
#include <QToolBar>
#include <QToolButton>

#include "katana/core/text.hpp"
#include "tools/tool_icons.hpp"

namespace katana::qt::tools {

namespace cad = katana::cad;

namespace {

QString qs(const std::string& text) { return QString::fromStdString(text); }

// Where a group sorts within its category: its place in kGroupOrder, or
// after every listed group.
std::size_t groupRank(const std::string& group)
{
    const auto found = std::ranges::find(kGroupOrder, group);
    return static_cast<std::size_t>(found - kGroupOrder.begin());
}

// The catalogue's tools of one category, grouped: groups in kGroupOrder, the
// tools within each in the catalogue's order (by `order`, then id).
std::vector<std::vector<const cad::ToolInfo*>> groupsOf(const std::vector<const cad::ToolInfo*>& tools)
{
    std::vector<std::vector<const cad::ToolInfo*>> groups;
    for (const cad::ToolInfo* info : tools) {
        auto group = std::ranges::find_if(groups, [&](const auto& g) {
            return g.front()->group == info->group;
        });
        if (group == groups.end()) {
            groups.push_back({info});
        } else {
            group->push_back(info);
        }
    }
    // Stable: unlisted groups keep the catalogue's order among themselves.
    std::ranges::stable_sort(groups, {}, [](const auto& g) { return groupRank(g.front()->group); });
    return groups;
}

} // namespace

std::optional<std::pair<std::string, std::string>> familyAndVariant(std::string_view name)
{
    const auto comma = name.find(',');
    if (comma == std::string_view::npos) {
        return std::nullopt;
    }
    const std::string_view family = katana::core::trimmed(name.substr(0, comma));
    const std::string_view variant = katana::core::trimmed(name.substr(comma + 1));
    if (family.empty() || variant.empty()) {
        return std::nullopt;
    }
    return std::pair<std::string, std::string>(family, variant);
}

std::string toolTooltip(const cad::ToolInfo& info)
{
    QString text = QString("<b>%1</b>").arg(qs(info.name).toHtmlEscaped());
    if (!info.aliases.empty()) {
        QStringList aliases;
        for (const std::string& alias : info.aliases) {
            aliases << qs(alias).toHtmlEscaped();
        }
        text += QString(" (%1)").arg(aliases.join(", "));
    }
    if (!info.shortcut.empty()) {
        text += QString(" &nbsp;%1").arg(
            QKeySequence(qs(info.shortcut)).toString(QKeySequence::NativeText).toHtmlEscaped());
    }
    if (!info.tip.empty()) {
        text += "<br>" + qs(info.tip).toHtmlEscaped();
    }
    return text.toStdString();
}

std::optional<std::string> toolIdForCommand(std::string_view word)
{
    const std::string_view typed = katana::core::trimmed(word);
    if (typed.empty()) {
        return std::nullopt;
    }
    const cad::ToolCatalog& catalog = cad::toolCatalog();
    if (const cad::ToolInfo* info = catalog.findByAlias(typed)) {
        return info->id;
    }
    // The id itself, so a script or a user reading a tooltip's object name
    // can start any tool, aliased or not ("draw.circle.ttr" has no alias).
    if (const cad::ToolInfo* info = catalog.find(typed)) {
        return info->id;
    }
    return std::nullopt;
}

QAction* ToolActions::action(std::string_view toolId) const
{
    const auto found = actions_.find(toolId);
    return found != actions_.end() ? found->second : nullptr;
}

void ToolActions::setActive(std::string_view toolId) const
{
    if (QAction* running = action(toolId)) {
        running->setChecked(true);
        return;
    }
    // ExclusiveOptional: unchecking the checked one leaves none checked.
    if (group_ != nullptr) {
        if (QAction* checked = group_->checkedAction()) {
            checked->setChecked(false);
        }
    }
}

ToolActions fillToolMenus(const cad::ToolCatalog& catalog, const ToolMenuTargets& targets,
                          QObject* owner, StartTool start)
{
    ToolActions result;
    result.group_ = new QActionGroup(owner);
    result.group_->setObjectName("toolActions");
    // Checked while its tool runs, and none checked while the view selects.
    result.group_->setExclusionPolicy(QActionGroup::ExclusionPolicy::ExclusiveOptional);

    // One action per tool, whether or not a menu takes it: the window may
    // still want it for a shortcut or a toolbar of its own.
    for (const cad::ToolInfo* info : catalog.all()) {
        auto* action = new QAction(toolIcon(info->id), qs(info->name), owner);
        action->setObjectName(qs(info->id));
        action->setStatusTip(qs(info->tip));
        action->setToolTip(qs(toolTooltip(*info)));
        if (!info->shortcut.empty()) {
            action->setShortcut(QKeySequence(qs(info->shortcut)));
        }
        action->setCheckable(true);
        result.group_->addAction(action);
        const std::string id = info->id;
        QObject::connect(action, &QAction::triggered, owner, [start, id] {
            if (start) {
                start(id);
            }
        });
        result.actions_.emplace(info->id, action);
    }

    // The catalogue's categories in its order.
    std::vector<std::string> categories;
    for (const cad::ToolInfo* info : catalog.all()) {
        if (std::ranges::find(categories, info->category) == categories.end()) {
            categories.push_back(info->category);
        }
    }

    for (const std::string& category : categories) {
        std::vector<const cad::ToolInfo*> tools;
        for (const cad::ToolInfo* info : catalog.all()) {
            if (info->category == category) {
                tools.push_back(info);
            }
        }
        const auto menuAt = targets.menus.find(category);
        QMenu* menu = menuAt != targets.menus.end() ? menuAt->second : nullptr;
        const auto barAt = targets.toolBars.find(category);
        QToolBar* bar = barAt != targets.toolBars.end() ? barAt->second : nullptr;
        if (menu == nullptr) {
            result.unplaced_.push_back(category);
            if (bar == nullptr) {
                continue; // nowhere to put them; the actions exist all the same
            }
        }

        bool firstGroup = true;
        for (const auto& group : groupsOf(tools)) {
            if (!firstGroup) {
                if (menu != nullptr) {
                    menu->addSeparator();
                }
                if (bar != nullptr) {
                    bar->addSeparator();
                }
            }
            firstGroup = false;

            // Families of two or more variants in this group, in the order
            // their first variant comes.
            std::map<std::string, int> familySize;
            for (const cad::ToolInfo* info : group) {
                if (const auto parts = familyAndVariant(info->name)) {
                    ++familySize[parts->first];
                }
            }
            std::map<std::string, QMenu*> familyMenus;
            for (const cad::ToolInfo* info : group) {
                QAction* action = result.action(info->id);
                const auto parts = familyAndVariant(info->name);
                if (!parts || familySize[parts->first] < 2) {
                    if (menu != nullptr) {
                        menu->addAction(action);
                    }
                    if (bar != nullptr) {
                        bar->addAction(action);
                    }
                    continue;
                }
                QMenu*& family = familyMenus[parts->first];
                if (family == nullptr) {
                    // Owned by the category's menu, or by its toolbar when
                    // it has no menu, so it goes when they do.
                    QWidget* const parent = menu != nullptr ? static_cast<QWidget*>(menu)
                                                            : static_cast<QWidget*>(bar);
                    family = new QMenu(qs(parts->first), parent);
                    family->setObjectName(qs("toolFamily." + category + "." + parts->first));
                    family->setIcon(toolIcon(info->id));
                    if (menu != nullptr) {
                        menu->addMenu(family);
                    }
                    if (bar != nullptr) {
                        // One button for the family: a click runs its first
                        // variant, the arrow offers the others.
                        auto* button = new QToolButton(bar);
                        button->setObjectName(qs("toolFamilyButton." + category + "." +
                                                 parts->first));
                        button->setDefaultAction(action);
                        button->setMenu(family);
                        button->setPopupMode(QToolButton::MenuButtonPopup);
                        bar->addWidget(button);
                    }
                }
                // In the submenu the variant alone, "Circle > 2 Points". The
                // same action, not a copy, so there is one object per tool
                // id; its full name stays in the tooltip, which is all a
                // toolbar shows of it.
                action->setText(qs(parts->second));
                family->addAction(action);
            }
        }
    }
    return result;
}

} // namespace katana::qt::tools
