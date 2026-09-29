#include "tools/tool_menus.hpp"

#include <algorithm>
#include <cctype>
#include <functional>
#include <utility>

#include <QAction>
#include <QActionGroup>
#include <QKeySequence>
#include <QMenu>
#include <QMenuBar>
#include <QString>
#include <QToolBar>

#include "katana/core/text.hpp"
#include "tools/flyout_button.hpp"
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

// The letter after a single '&' in `text`, upper case; 0 for none.
char mnemonicOf(std::string_view text)
{
    for (std::size_t at = text.find('&'); at != std::string_view::npos && at + 1 < text.size();
         at = text.find('&', at + 2)) {
        if (text[at + 1] != '&') {
            return static_cast<char>(std::toupper(static_cast<unsigned char>(text[at + 1])));
        }
    }
    return 0;
}

bool isAsciiAlnum(char c)
{
    return (c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z') || (c >= '0' && c <= '9');
}

// The letters `text` could be underlined at, upper case, each once: the
// first letters of its words first, then the rest in reading order.
std::string candidateLetters(std::string_view text)
{
    std::string out;
    const auto add = [&](char c) {
        const char upper = static_cast<char>(std::toupper(static_cast<unsigned char>(c)));
        if (isAsciiAlnum(c) && out.find(upper) == std::string::npos) {
            out += upper;
        }
    };
    for (std::size_t i = 0; i < text.size(); ++i) {
        if (i == 0 || text[i - 1] == ' ') {
            add(text[i]);
        }
    }
    for (const char c : text) {
        add(c);
    }
    return out;
}

// `text` with '&' before `letter`: at a word's start when it starts one,
// else at its first occurrence.
std::string underlined(std::string_view text, char letter)
{
    const auto is = [&](char c) {
        return std::toupper(static_cast<unsigned char>(c)) == static_cast<unsigned char>(letter);
    };
    std::optional<std::size_t> at;
    for (std::size_t i = 0; i < text.size() && !at; ++i) {
        if ((i == 0 || text[i - 1] == ' ') && is(text[i])) {
            at = i;
        }
    }
    for (std::size_t i = 0; i < text.size() && !at; ++i) {
        if (is(text[i])) {
            at = i;
        }
    }
    std::string out(text.substr(0, *at));
    out += '&';
    out += text.substr(*at);
    return out;
}

// Gives every item of `menu` without a mnemonic one no other item there has,
// and recurses into its submenus. Items that already have a letter (the
// window's own) keep it, and no tool takes it.
//
// A matching, not first come first served: taken greedily, Modify's early
// items used up every letter of "Trim" and of "Match Properties" (Stretch
// took the T, Mirror the I, Move the M, Rotate the R), and those two were
// left without one. Each item, the fewest letters first, takes its best free
// letter (word-initial letters first); only when none is free does an
// augmenting path (Kuhn's) move another item to another of its letters. So
// every item gets a letter whenever the menu's texts allow it, short names
// keep the letter a user guesses, and the letters are the same on every run -
// at most two dozen items of a dozen letters each, so nothing to measure.
void assignMnemonics(QMenu& menu)
{
    std::string taken;
    std::vector<QAction*> items;
    for (QAction* item : menu.actions()) {
        if (item->isSeparator()) {
            continue;
        }
        if (const char letter = mnemonicOf(item->text().toStdString()); letter != 0) {
            taken += letter;
        } else {
            items.push_back(item);
        }
    }
    std::vector<std::string> candidates;
    for (const QAction* item : items) {
        std::string letters = candidateLetters(item->text().toStdString());
        std::erase_if(letters, [&](char c) { return taken.find(c) != std::string::npos; });
        candidates.push_back(std::move(letters));
    }
    std::map<char, std::size_t> owner; // letter -> item
    std::string visited;
    const std::function<bool(std::size_t)> place = [&](std::size_t item) {
        // A free letter first, the item's best: an augmenting path moves
        // an earlier item off its letter only when no free one is left.
        for (const char letter : candidates[item]) {
            if (!owner.contains(letter)) {
                owner[letter] = item;
                return true;
            }
        }
        for (const char letter : candidates[item]) {
            if (visited.find(letter) != std::string::npos) {
                continue;
            }
            visited += letter;
            const auto held = owner.find(letter);
            if (held == owner.end() || place(held->second)) {
                owner[letter] = item;
                return true;
            }
        }
        return false;
    };
    // The items with the fewest letters choose first ("Trim" has four,
    // "Match Properties" eleven), in menu order among equals: a short name
    // cannot give way, a long one can.
    std::vector<std::size_t> order(items.size());
    for (std::size_t i = 0; i < order.size(); ++i) {
        order[i] = i;
    }
    std::ranges::stable_sort(order, {}, [&](std::size_t i) { return candidates[i].size(); });
    for (const std::size_t item : order) {
        visited.clear();
        (void)place(item); // an item with no letter left keeps its text
    }
    for (const auto& [letter, item] : owner) {
        items[item]->setText(qs(underlined(items[item]->text().toStdString(), letter)));
    }
    for (QAction* item : menu.actions()) {
        if (QMenu* sub = item->menu()) {
            assignMnemonics(*sub);
        }
    }
}

// The menu bar the window's tool menus are on, or null when they are on none
// (a test's free-standing menus).
QMenuBar* barOf(const ToolMenuTargets& targets)
{
    for (const auto& [category, menu] : targets.menus) {
        if (menu != nullptr) {
            if (auto* bar = qobject_cast<QMenuBar*>(menu->parentWidget())) {
                return bar;
            }
        }
    }
    return nullptr;
}

// A menu for `category` on `bar`, after the last of the window's tool menus
// there, titled with a mnemonic no other menu on the bar has.
QMenu* makeCategoryMenu(QMenuBar& bar, const ToolMenuTargets& targets, const std::string& category,
                        QAction*& after)
{
    std::string taken;
    for (const QAction* top : bar.actions()) {
        if (const char letter = mnemonicOf(top->text().toStdString()); letter != 0) {
            taken += letter;
        }
    }
    auto* menu = new QMenu(qs(withMnemonic(category, taken)), &bar);
    std::string name = category;
    std::ranges::transform(name, name.begin(), [](char c) {
        return static_cast<char>(std::tolower(static_cast<unsigned char>(c)));
    });
    name.erase(std::remove(name.begin(), name.end(), ' '), name.end());
    menu->setObjectName(qs(name + "Menu"));
    if (after == nullptr) {
        // After the last target menu on the bar, in the bar's order.
        for (QAction* top : bar.actions()) {
            for (const auto& [targetCategory, target] : targets.menus) {
                if (target != nullptr && top->menu() == target) {
                    after = top;
                }
            }
        }
    }
    const QList<QAction*> tops = bar.actions();
    const qsizetype index = after != nullptr ? tops.indexOf(after) : -1;
    QAction* before = index >= 0 && index + 1 < tops.size() ? tops[index + 1] : nullptr;
    if (before != nullptr) {
        bar.insertMenu(before, menu);
    } else {
        bar.addMenu(menu);
    }
    after = menu->menuAction();
    return menu;
}

} // namespace

std::string withMnemonic(std::string_view text, std::string& taken)
{
    if (const char letter = mnemonicOf(text); letter != 0) {
        taken += letter;
        return std::string(text);
    }
    const auto free = [&](char c) {
        const char upper = static_cast<char>(std::toupper(static_cast<unsigned char>(c)));
        return isAsciiAlnum(c) && taken.find(upper) == std::string::npos;
    };
    std::optional<std::size_t> chosen;
    // A word's first letter first: "Match &Properties" reads better than
    // "Matc&h Properties", and is what a user guesses.
    for (std::size_t i = 0; i < text.size() && !chosen; ++i) {
        const bool starts = i == 0 || text[i - 1] == ' ';
        if (starts && free(text[i])) {
            chosen = i;
        }
    }
    for (std::size_t i = 0; i < text.size() && !chosen; ++i) {
        if (free(text[i])) {
            chosen = i;
        }
    }
    if (!chosen) {
        return std::string(text);
    }
    taken += static_cast<char>(std::toupper(static_cast<unsigned char>(text[*chosen])));
    std::string out(text.substr(0, *chosen));
    out += '&';
    out += text.substr(*chosen);
    return out;
}

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

    QMenuBar* const menuBar = barOf(targets);
    QAction* lastMade = nullptr; // where the next made menu goes on the bar
    std::vector<QMenu*> filled;
    for (const std::string& category : categories) {
        std::vector<const cad::ToolInfo*> tools;
        for (const cad::ToolInfo* info : catalog.all()) {
            if (info->category == category) {
                tools.push_back(info);
            }
        }
        const auto menuAt = targets.menus.find(category);
        QMenu* menu = menuAt != targets.menus.end() ? menuAt->second : nullptr;
        if (menu == nullptr && menuBar != nullptr) {
            menu = makeCategoryMenu(*menuBar, targets, category, lastMade);
            result.made_.emplace(category, menu);
        }
        if (menu != nullptr) {
            filled.push_back(menu);
        }
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
            // A menu titles every group, the first too, so a long one reads
            // as its parts (Transform, Edit); a toolbar has no room for words
            // and parts its groups with a line.
            if (menu != nullptr) {
                menu->addSection(qs(group.front()->group));
            }
            if (!firstGroup && bar != nullptr) {
                bar->addSeparator();
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
                        // variant; its corner triangle, a press held or a
                        // right click offers the others (flyout_button.hpp).
                        auto* button = new FlyoutButton(*family, bar);
                        button->setObjectName(qs("toolFamilyButton." + category + "." +
                                                 parts->first));
                        button->setDefaultAction(action);
                        bar->addWidget(button);
                        followToolBarIconSize(*button, *bar);
                    }
                }
                // In the submenu the variant alone, "Circle > 2 Points". The
                // same action, not a copy, so there is one object per tool
                // id; its full name stays in the tooltip, which is all a
                // toolbar shows of it.
                action->setText(qs(parts->second));
                family->addAction(action);
            }
            // The family's own line in the status bar says what is inside:
            // "Circle: Center, Radius / 2 Points / 3 Points / Tan, Tan,
            // Radius" - slashes, since a variant's name holds commas and a
            // menu's report (--report) parts its items with semicolons.
            for (const auto& [name, family] : familyMenus) {
                QStringList variants;
                for (const QAction* variant : family->actions()) {
                    variants << QString(variant->text()).remove('&');
                }
                family->menuAction()->setStatusTip(qs(name) + ": " + variants.join(" / "));
            }
        }
    }
    // Last, when every item is in: a letter is chosen against the whole menu.
    for (QMenu* menu : filled) {
        assignMnemonics(*menu);
    }
    return result;
}

} // namespace katana::qt::tools
