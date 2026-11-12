//! The window's tool menus and toolbars, generated from the tool catalogue
//! (katana_cad::interactive_tool) rather than listed by hand: a tool is added by writing it and
//! adding it to its family, and it appears in its menu, its toolbar and the command line with no
//! edit to the window.
//!
//! Each tool gets ONE action, shared by its menu and its toolbar, named by the tool's id (so tests
//! and the headless driver find it by name), with its family's icon (tool_icons), its shortcut,
//! its tip as the status tip, and a tooltip naming its command-line aliases the way other CAD
//! programs' do.
//!
//! ```text
//! Layout, as a CAD user expects it:
//!   - one menu and one toolbar per category (Draw, Modify, Annotate, ...);
//!   - groups in GROUP_ORDER, each under a section titled with its name in a menu and parted by a
//!     separator on a toolbar;
//!   - tools named "Family, Variant" ("Circle, 2 Points") gathered into a submenu named by the
//!     family, each item by its variant; on a toolbar the family is ONE button that runs its first
//!     variant and offers the rest from the triangle in its corner, a press held or a right click
//!     (flyout_button);
//!   - a category the window gave no menu (Tools: the Inquiry and Select groups) gets one of its
//!     own, beside the window's tool menus on their menu bar;
//!   - every item a menu letter of its own within its menu, and a made menu one of its own on the
//!     bar.
//! ```
//!
//! In the C++ the actions were QActions shared by the menus and toolbars; here a menu or a toolbar
//! holds rows of data (drive::Menu, drive::ToolBar), so `ToolActions` keeps the one action of each
//! tool - its text, its check - and the rows hold copies the window brings up to date each frame
//! (`refresh_menu`, `refresh_tool_bar`), and a family's button is a Widget row the toolbar draws
//! from `ToolActions::flyout`. An action never starts its tool itself: triggered, it says which
//! tool, so the window decides which view the tool runs in.

use std::collections::BTreeMap;

use egui::{Key, KeyboardShortcut, Modifiers};
use katana_cad::interactive_tool::tool_catalog;
use katana_cad::{ToolCatalog, ToolInfo};

use super::flyout_button::{Flyout, StripItem, html_escaped};
use crate::drive::action::{Action, Menu, MenuItem, ToolBar, ToolBarItem, shortcut_text};

/// The order groups appear in within a category's menu: the everyday ones first, as CAD programs
/// put Line before Circle. A group not listed follows the listed ones, in the catalogue's
/// (alphabetical) order.
pub const GROUP_ORDER: [&str; 11] = [
    "Lines",
    "Curves",
    "Points",
    "Transform",
    "Edit",
    "Properties",
    "Text",
    "Dimensions",
    "Leaders",
    "Inquiry",
    "Select",
];

/// A menu on the window's menu bar, as `fill_tool_menus` sees the bar: its object name and title.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BarMenu {
    pub name: String,
    pub title: String,
}

impl BarMenu {
    pub fn new(name: &str, title: &str) -> Self {
        Self {
            name: String::from(name),
            title: String::from(title),
        }
    }
}

/// Where the tools go. `menus` and `tool_bars` are filled in place, by category ("Draw" -> the
/// Draw menu). A category with no menu here gets a menu made for it on the menu bar the given
/// menus are on (named "<category>Menu" in lower case, "toolsMenu", placed after the last of
/// them, and put into `menu_bar`); with no such bar it is left out of the menus and listed in
/// ToolActions::unplaced. A target menu is on the bar when the bar holds a menu of its object
/// name. Toolbars are optional and never made.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ToolMenuTargets {
    pub menus: BTreeMap<String, Menu>,
    pub tool_bars: BTreeMap<String, ToolBar>,
    pub menu_bar: Option<Vec<BarMenu>>,
}

/// A family of tools ("Circle": Center Radius, 2 Points, ...): its menu, and its button when its
/// category has a toolbar.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolFamily {
    pub category: String,
    pub name: String,
    /// "toolFamily.<category>.<family>", titled with the family's name, an item for each of its
    /// tools named by its variant.
    pub menu: Menu,
    /// "toolFamilyButton.<category>.<family>", the toolbar's Widget row, when the category has a
    /// toolbar.
    pub button: Option<String>,
    /// The id of its first variant: what a click on its button runs, and its icon.
    pub first: String,
    /// The family's own line in the status bar, saying what is inside: "Circle: Center, Radius /
    /// 2 Points / 3 Points / Tan, Tan, Radius" - slashes, since a variant's name holds commas and
    /// a menu's report parts its items with semicolons.
    pub status_tip: String,
    /// The object name of what holds its menu: its category's menu, or its toolbar when the
    /// category has no menu.
    pub owner: String,
}

/// What `fill_tool_menus` made: the actions by tool id, so the window can find them, and check
/// the running tool's.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ToolActions {
    actions: BTreeMap<String, Action>,
    tooltips: BTreeMap<String, String>,
    families: Vec<ToolFamily>,
    unplaced: Vec<String>,
    made: BTreeMap<String, Menu>,
}

impl ToolActions {
    /// The action of tool `tool_id`.
    pub fn action(&self, tool_id: &str) -> Option<&Action> {
        self.actions.get(tool_id)
    }

    /// Every tool's action, by id.
    pub fn all(&self) -> &BTreeMap<String, Action> {
        &self.actions
    }

    /// The tooltip of tool `tool_id`'s action (toolTooltip).
    pub fn tooltip(&self, tool_id: &str) -> Option<&str> {
        self.tooltips.get(tool_id).map(String::as_str)
    }

    /// Checks the running tool's action and unchecks the rest; "" unchecks every one.
    pub fn set_active(&mut self, tool_id: &str) {
        let known = self.actions.contains_key(tool_id);
        for (id, action) in &mut self.actions {
            action.checked = Some(known && id == tool_id);
        }
    }

    /// The tool whose action is checked, if any.
    pub fn active(&self) -> Option<&str> {
        self.actions
            .iter()
            .find(|(_, action)| action.checked == Some(true))
            .map(|(id, _)| id.as_str())
    }

    /// The action `name` triggered as a click triggers a checkable action of an exclusive but
    /// optional group (QAction::trigger): checked if it was not, the others unchecked, or
    /// unchecked if it was. The tool's id, for the window to start; None for no tool's action.
    pub fn trigger(&mut self, name: &str) -> Option<String> {
        let was = self.actions.get(name)?.checked == Some(true);
        for (id, action) in &mut self.actions {
            action.checked = Some(!was && id == name);
        }
        Some(String::from(name))
    }

    /// Categories that had tools but no menu to put them in.
    pub fn unplaced(&self) -> &[String] {
        &self.unplaced
    }

    /// The menus made for categories the targets had none for, by category.
    pub fn made_menus(&self) -> &BTreeMap<String, Menu> {
        &self.made
    }

    /// Every family made, in the order made.
    pub fn families(&self) -> &[ToolFamily] {
        &self.families
    }

    /// The family whose menu or button is named `name`.
    pub fn family(&self, name: &str) -> Option<&ToolFamily> {
        self.families
            .iter()
            .find(|family| family.menu.name == name || family.button.as_deref() == Some(name))
    }

    /// The family's menu as it is now: its items the tools' actions with their checks.
    pub fn family_menu(&self, family: &ToolFamily) -> Menu {
        let mut menu = family.menu.clone();
        self.refresh_menu(&mut menu);
        menu
    }

    /// What the family's button named `button` is drawn from this frame.
    pub fn flyout(&self, button: &str) -> Option<Flyout> {
        let family = self.family(button)?;
        let default = self.actions.get(&family.first)?.clone();
        Some(Flyout {
            name: family.button.clone().unwrap_or_default(),
            menu: self.family_menu(family),
            tool_tip: self
                .tooltips
                .get(&family.first)
                .cloned()
                .unwrap_or_default(),
            default,
            family_tip: family.status_tip.clone(),
        })
    }

    /// `menu`'s rows of tools - its submenus' too - brought up to date with the actions: their
    /// texts and their checks.
    pub fn refresh_menu(&self, menu: &mut Menu) {
        for item in &mut menu.items {
            match item {
                MenuItem::Action(action) => {
                    if let Some(own) = self.actions.get(&action.name) {
                        action.clone_from(own);
                    }
                }
                MenuItem::Menu(submenu) => self.refresh_menu(submenu),
                MenuItem::Section(_) | MenuItem::Separator => {}
            }
        }
    }

    /// `bar`'s rows of tools brought up to date with the actions.
    pub fn refresh_tool_bar(&self, bar: &mut ToolBar) {
        for item in &mut bar.items {
            if let ToolBarItem::Action(action) = item
                && let Some(own) = self.actions.get(&action.name)
            {
                action.clone_from(own);
            }
        }
    }

    /// `bar`'s rows as its strip draws them this frame: the tools' actions as they are, each
    /// family's button, and the window's own Widget rows sized by `widget_size` (None for one it
    /// does not size).
    pub fn strip_items(
        &self,
        bar: &ToolBar,
        widget_size: &dyn Fn(&str) -> Option<(i32, i32)>,
    ) -> Vec<StripItem> {
        bar.items
            .iter()
            .map(|item| match item {
                ToolBarItem::Action(action) => match self.actions.get(&action.name) {
                    Some(own) => StripItem::Action {
                        action: own.clone(),
                        tool: true,
                        tooltip: self.tooltips.get(&own.name).cloned().unwrap_or_default(),
                    },
                    None => StripItem::Action {
                        action: action.clone(),
                        tool: false,
                        tooltip: action.tooltip(),
                    },
                },
                ToolBarItem::Separator => StripItem::Separator,
                ToolBarItem::Widget(name) => match self.flyout(name) {
                    Some(flyout) => StripItem::Family(flyout),
                    None => {
                        let (width, height) = widget_size(name).unwrap_or((0, 0));
                        StripItem::Widget {
                            name: name.clone(),
                            width,
                            height,
                        }
                    }
                },
            })
            .collect()
    }
}

// ---- menu letters ------------------------------------------------------------------------------

/// The letter after a single '&' in `text`, upper case; None for none. "&&" is a literal
/// ampersand.
fn mnemonic_of(text: &str) -> Option<u8> {
    let bytes = text.as_bytes();
    let mut at = text.find('&');
    while let Some(index) = at {
        if index + 1 >= bytes.len() {
            break;
        }
        if bytes[index + 1] != b'&' {
            return Some(bytes[index + 1].to_ascii_uppercase());
        }
        at = text[index + 2..].find('&').map(|found| found + index + 2);
    }
    None
}

fn is_ascii_alnum(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
}

/// Adds `letter` to `taken` (a byte of the text: ASCII for every letter a menu is given).
fn take_letter(taken: &mut String, letter: u8) {
    taken.push(char::from(letter));
}

fn is_taken(taken: &str, letter: u8) -> bool {
    taken.bytes().any(|byte| byte == letter)
}

/// Whether byte `index` of `text` starts a word.
fn starts_word(text: &[u8], index: usize) -> bool {
    index == 0 || text[index - 1] == b' '
}

/// The letters `text` could be underlined at, upper case, each once: the first letters of its
/// words first, then the rest in reading order.
fn candidate_letters(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out: Vec<u8> = Vec::new();
    let mut add = |byte: u8| {
        let upper = byte.to_ascii_uppercase();
        if is_ascii_alnum(byte) && !out.contains(&upper) {
            out.push(upper);
        }
    };
    for (index, &byte) in bytes.iter().enumerate() {
        if starts_word(bytes, index) {
            add(byte);
        }
    }
    for &byte in bytes {
        add(byte);
    }
    out
}

/// `text` with '&' before `letter`: at a word's start when it starts one, else at its first
/// occurrence.
fn underlined(text: &str, letter: u8) -> String {
    let bytes = text.as_bytes();
    let is = |byte: u8| byte.to_ascii_uppercase() == letter;
    let at = (0..bytes.len())
        .find(|&index| starts_word(bytes, index) && is(bytes[index]))
        .or_else(|| (0..bytes.len()).find(|&index| is(bytes[index])));
    match at {
        Some(at) => format!("{}&{}", &text[..at], &text[at..]),
        None => String::from(text),
    }
}

/// `text` with an '&' before the letter that is to be its menu letter: the first letter or digit
/// starting a word that is not in `taken`, else the first one anywhere that is not, else `text`
/// unchanged (every letter taken). The letter is added to `taken` in upper case. A text that has a
/// menu letter already keeps it and adds its letter. "&&" is a literal ampersand.
pub fn with_mnemonic(text: &str, taken: &mut String) -> String {
    if let Some(letter) = mnemonic_of(text) {
        take_letter(taken, letter);
        return String::from(text);
    }
    let bytes = text.as_bytes();
    let free = |byte: u8| is_ascii_alnum(byte) && !is_taken(taken, byte.to_ascii_uppercase());
    // A word's first letter first: "Match &Properties" reads better than "Matc&h Properties", and
    // is what a user guesses.
    let chosen = (0..bytes.len())
        .find(|&index| starts_word(bytes, index) && free(bytes[index]))
        .or_else(|| (0..bytes.len()).find(|&index| free(bytes[index])));
    let Some(chosen) = chosen else {
        return String::from(text);
    };
    take_letter(taken, bytes[chosen].to_ascii_uppercase());
    format!("{}&{}", &text[..chosen], &text[chosen..])
}

/// The text of a menu's item as its letter is chosen from: an action's text, a submenu's title.
fn item_text(item: &MenuItem) -> Option<&str> {
    match item {
        MenuItem::Action(action) => Some(&action.text),
        MenuItem::Menu(menu) => Some(&menu.title),
        MenuItem::Section(_) | MenuItem::Separator => None,
    }
}

fn set_item_text(item: &mut MenuItem, text: String) {
    match item {
        MenuItem::Action(action) => action.text = text,
        MenuItem::Menu(menu) => menu.title = text,
        MenuItem::Section(_) | MenuItem::Separator => {}
    }
}

/// An augmenting path for `item` (Kuhn's): its best free letter, else a letter whose holder can
/// move to another of its own.
fn place(
    item: usize,
    candidates: &[Vec<u8>],
    owner: &mut BTreeMap<u8, usize>,
    visited: &mut Vec<u8>,
) -> bool {
    // A free letter first, the item's best: an augmenting path moves an earlier item off its
    // letter only when no free one is left.
    for &letter in &candidates[item] {
        if let std::collections::btree_map::Entry::Vacant(entry) = owner.entry(letter) {
            entry.insert(item);
            return true;
        }
    }
    for &letter in &candidates[item] {
        if visited.contains(&letter) {
            continue;
        }
        visited.push(letter);
        let held = owner.get(&letter).copied();
        if held.is_none_or(|held| place(held, candidates, owner, visited)) {
            owner.insert(letter, item);
            return true;
        }
    }
    false
}

/// Gives every item of `menu` without a menu letter one no other item there has, and recurses
/// into its submenus. Items that already have a letter (the window's own) keep it, and no tool
/// takes it.
///
/// A matching, not first come first served: taken greedily, Modify's early items used up every
/// letter of "Trim" and of "Match Properties" (Stretch took the T, Mirror the I, Move the M,
/// Rotate the R), and those two were left without one. Each item, the fewest letters first, takes
/// its best free letter (word-initial letters first); only when none is free does an augmenting
/// path (Kuhn's) move another item to another of its letters. So every item gets a letter
/// whenever the menu's texts allow it, short names keep the letter a user guesses, and the letters
/// are the same on every run - at most two dozen items of a dozen letters each, so nothing to
/// measure.
pub fn assign_mnemonics(menu: &mut Menu) {
    let mut taken = String::new();
    let mut items: Vec<usize> = Vec::new();
    for (index, item) in menu.items.iter().enumerate() {
        let Some(text) = item_text(item) else {
            continue;
        };
        match mnemonic_of(text) {
            Some(letter) => take_letter(&mut taken, letter),
            None => items.push(index),
        }
    }
    let candidates: Vec<Vec<u8>> = items
        .iter()
        .map(|&index| {
            let mut letters = candidate_letters(item_text(&menu.items[index]).unwrap_or(""));
            letters.retain(|&letter| !is_taken(&taken, letter));
            letters
        })
        .collect();
    let mut owner: BTreeMap<u8, usize> = BTreeMap::new();
    let mut visited: Vec<u8> = Vec::new();
    // The items with the fewest letters choose first ("Trim" has four, "Match Properties"
    // eleven), in menu order among equals: a short name cannot give way, a long one can.
    let mut order: Vec<usize> = (0..items.len()).collect();
    order.sort_by_key(|&item| candidates[item].len());
    for item in order {
        visited.clear();
        // An item with no letter left keeps its text.
        let _ = place(item, &candidates, &mut owner, &mut visited);
    }
    for (&letter, &item) in &owner {
        let index = items[item];
        let text = underlined(item_text(&menu.items[index]).unwrap_or(""), letter);
        set_item_text(&mut menu.items[index], text);
    }
    for item in &mut menu.items {
        if let MenuItem::Menu(submenu) = item {
            assign_mnemonics(submenu);
        }
    }
}

// ---- names, tooltips and words ------------------------------------------------------------------

/// `text` with the ASCII white space at its ends cut (core::trimmed).
fn trimmed(text: &str) -> &str {
    // Cutting ASCII bytes off the ends of UTF-8 leaves UTF-8.
    std::str::from_utf8(katana_core::text::trimmed(text.as_bytes())).unwrap_or("")
}

/// The family and variant of a name "Circle, 2 Points"; None for a name with no comma, or with
/// nothing either side of it.
pub fn family_and_variant(name: &str) -> Option<(String, String)> {
    let comma = name.find(',')?;
    let family = trimmed(&name[..comma]);
    let variant = trimmed(&name[comma + 1..]);
    if family.is_empty() || variant.is_empty() {
        return None;
    }
    Some((String::from(family), String::from(variant)))
}

/// A shortcut written as Qt's portable text ("Ctrl+Shift+L") as egui's; None for one it cannot
/// read.
fn parse_shortcut(text: &str) -> Option<KeyboardShortcut> {
    let parts: Vec<&str> = text.split('+').collect();
    let (key, held) = parts.split_last()?;
    let mut modifiers = Modifiers::NONE;
    for word in held {
        match word.trim().to_ascii_lowercase().as_str() {
            "ctrl" => modifiers.ctrl = true,
            "alt" => modifiers.alt = true,
            "shift" => modifiers.shift = true,
            "meta" => modifiers.mac_cmd = true,
            _ => return None,
        }
    }
    let key = Key::from_name(key.trim())?;
    Some(KeyboardShortcut::new(modifiers, key))
}

/// The text a tool's action shows as its tooltip: the name, the aliases and the shortcut, then the
/// tip - "<b>Line</b> (LINE, L)<br>Draws ...".
pub fn tool_tooltip(info: &ToolInfo) -> String {
    let mut text = format!("<b>{}</b>", html_escaped(&info.name));
    if !info.aliases.is_empty() {
        let aliases: Vec<String> = info
            .aliases
            .iter()
            .map(|alias| html_escaped(alias))
            .collect();
        text.push_str(&format!(" ({})", aliases.join(", ")));
    }
    if !info.shortcut.is_empty() {
        // QKeySequence's native text, which off a Mac is its portable text.
        let shown = parse_shortcut(&info.shortcut).map_or_else(
            || info.shortcut.clone(),
            |shortcut| shortcut_text(&shortcut),
        );
        text.push_str(&format!(" &nbsp;{}", html_escaped(&shown)));
    }
    if !info.tip.is_empty() {
        text.push_str("<br>");
        text.push_str(&html_escaped(&info.tip));
    }
    text
}

/// The tool a word typed at the command line starts: an alias ("L", "line", "TRIM"),
/// case-insensitively, or a tool's own id ("draw.line"). None for a word that starts no tool - it
/// is then a command for the interpreter.
pub fn tool_id_for_command(word: &str) -> Option<String> {
    let typed = trimmed(word);
    if typed.is_empty() {
        return None;
    }
    let catalog = tool_catalog();
    if let Some(info) = catalog.find_by_alias(typed) {
        return Some(info.id.clone());
    }
    // The id itself, so a script or a user reading a tooltip's object name can start any tool,
    // aliased or not ("draw.circle.ttr" has no alias).
    catalog.find(typed).map(|info| info.id.clone())
}

// ---- filling the menus ------------------------------------------------------------------------

/// Where a group sorts within its category: its place in GROUP_ORDER, or after every listed group.
fn group_rank(group: &str) -> usize {
    GROUP_ORDER
        .iter()
        .position(|listed| *listed == group)
        .unwrap_or(GROUP_ORDER.len())
}

/// The catalogue's tools of one category, grouped: groups in GROUP_ORDER, the tools within each in
/// the catalogue's order (by `order`, then id). Stable: unlisted groups keep the catalogue's order
/// among themselves.
fn groups_of<'a>(tools: &[&'a ToolInfo]) -> Vec<Vec<&'a ToolInfo>> {
    let mut groups: Vec<Vec<&'a ToolInfo>> = Vec::new();
    for &info in tools {
        match groups.iter_mut().find(|group| group[0].group == info.group) {
            Some(group) => group.push(info),
            None => groups.push(vec![info]),
        }
    }
    groups.sort_by_key(|group| group_rank(&group[0].group));
    groups
}

/// The object name of the menu made for `category`: its name in lower case with its spaces gone,
/// and "Menu" ("toolsMenu").
fn made_menu_name(category: &str) -> String {
    let mut name: String = category
        .chars()
        .filter(|character| *character != ' ')
        .map(|character| character.to_ascii_lowercase())
        .collect();
    name.push_str("Menu");
    name
}

/// Every action of `menu` and its submenus, their texts as the menu letters left them, into
/// `texts`; and each family's submenu, as filled, into `families`.
fn collect_texts(
    menu: &Menu,
    texts: &mut BTreeMap<String, String>,
    families: &mut BTreeMap<String, Menu>,
) {
    for item in &menu.items {
        match item {
            MenuItem::Action(action) => {
                texts.insert(action.name.clone(), action.text.clone());
            }
            MenuItem::Menu(submenu) => {
                families.insert(submenu.name.clone(), submenu.clone());
                collect_texts(submenu, texts, families);
            }
            MenuItem::Section(_) | MenuItem::Separator => {}
        }
    }
}

/// Fills `targets` from `catalog` and returns the actions. Each action, triggered, names its tool
/// (ToolActions::trigger) - never starts anything itself, so the window decides which view the
/// tool runs in.
pub fn fill_tool_menus(
    catalog: &'static ToolCatalog,
    targets: &mut ToolMenuTargets,
) -> ToolActions {
    let mut result = ToolActions::default();

    // One action per tool, whether or not a menu takes it: the window may still want it for a
    // shortcut or a toolbar of its own.
    for info in catalog.all() {
        let mut action = Action::new(&info.id, &info.name)
            .icon(info.id.as_str())
            .tip(&info.tip)
            .checkable(false);
        if !info.shortcut.is_empty()
            && let Some(shortcut) = parse_shortcut(&info.shortcut)
        {
            action = action.shortcut(shortcut);
        }
        result.tooltips.insert(info.id.clone(), tool_tooltip(info));
        result.actions.insert(info.id.clone(), action);
    }

    // The catalogue's categories in its order.
    let mut categories: Vec<&str> = Vec::new();
    for info in catalog.all() {
        if !categories.contains(&info.category.as_str()) {
            categories.push(&info.category);
        }
    }

    // The menu bar the window's tool menus are on, or none when they are on none (a test's
    // free-standing menus).
    let on_bar = targets.menu_bar.as_ref().is_some_and(|bar| {
        targets
            .menus
            .values()
            .any(|menu| bar.iter().any(|top| top.name == menu.name))
    });
    // Where the next made menu goes on the bar: after the last of the window's tool menus there.
    let mut last_made: Option<String> = None;
    let mut filled: Vec<String> = Vec::new();

    for category in categories {
        let tools: Vec<&'static ToolInfo> = catalog
            .all()
            .into_iter()
            .filter(|info| info.category == category)
            .collect();
        let mut menu = targets.menus.remove(category);
        let mut made = false;
        if menu.is_none()
            && on_bar
            && let Some(bar) = targets.menu_bar.as_mut()
        {
            let made_menu = make_category_menu(bar, &targets.menus, category, &mut last_made);
            menu = Some(made_menu);
            made = true;
        }
        let mut tool_bar = targets.tool_bars.remove(category);
        if menu.is_none() {
            result.unplaced.push(String::from(category));
            if tool_bar.is_none() {
                continue; // nowhere to put them; the actions exist all the same
            }
        }
        let owner = menu
            .as_ref()
            .map(|menu| menu.name.clone())
            .or_else(|| tool_bar.as_ref().map(|bar| bar.name.clone()))
            .unwrap_or_default();

        let mut first_group = true;
        for group in groups_of(&tools) {
            // A menu titles every group, the first too, so a long one reads as its parts
            // (Transform, Edit); a toolbar has no room for words and parts its groups with a line.
            if let Some(menu) = menu.as_mut() {
                menu.items.push(MenuItem::Section(group[0].group.clone()));
            }
            if !first_group && let Some(bar) = tool_bar.as_mut() {
                bar.items.push(ToolBarItem::Separator);
            }
            first_group = false;

            // Families of two or more variants in this group, in the order their first variant
            // comes.
            let mut family_size: BTreeMap<String, i32> = BTreeMap::new();
            for info in &group {
                if let Some((family, _)) = family_and_variant(&info.name) {
                    *family_size.entry(family).or_insert(0) += 1;
                }
            }
            let mut family_menus: BTreeMap<String, usize> = BTreeMap::new();
            for info in &group {
                let parts = family_and_variant(&info.name)
                    .filter(|(family, _)| family_size.get(family).copied().unwrap_or(0) >= 2);
                let Some((family, variant)) = parts else {
                    let action = result.actions[&info.id].clone();
                    if let Some(menu) = menu.as_mut() {
                        menu.items.push(MenuItem::Action(action.clone()));
                    }
                    if let Some(bar) = tool_bar.as_mut() {
                        bar.items.push(ToolBarItem::Action(action));
                    }
                    continue;
                };
                let at = match family_menus.get(&family) {
                    Some(&at) => at,
                    None => {
                        let menu_name = format!("toolFamily.{category}.{family}");
                        // The family's place in the category's menu, filled when its group is.
                        if let Some(menu) = menu.as_mut() {
                            menu.items
                                .push(MenuItem::Menu(Menu::new(&menu_name, &family)));
                        }
                        // One button for the family on the toolbar: a click runs its first
                        // variant; its corner triangle, a press held or a right click offers the
                        // others (flyout_button).
                        let button = tool_bar.as_mut().map(|bar| {
                            let button = format!("toolFamilyButton.{category}.{family}");
                            bar.items.push(ToolBarItem::Widget(button.clone()));
                            button
                        });
                        result.families.push(ToolFamily {
                            category: String::from(category),
                            name: family.clone(),
                            menu: Menu::new(&menu_name, &family),
                            button,
                            first: info.id.clone(),
                            status_tip: String::new(),
                            owner: owner.clone(),
                        });
                        let at = result.families.len() - 1;
                        family_menus.insert(family.clone(), at);
                        at
                    }
                };
                // In the submenu the variant alone, "Circle > 2 Points". The same action, not a
                // copy, so there is one action per tool id; its full name stays in the tooltip,
                // which is all a toolbar shows of it.
                if let Some(action) = result.actions.get_mut(&info.id) {
                    action.text = variant;
                    result.families[at]
                        .menu
                        .items
                        .push(MenuItem::Action(action.clone()));
                }
            }
            // The family's own line in the status bar says what is inside.
            for (name, &at) in &family_menus {
                let variants: Vec<String> = result.families[at]
                    .menu
                    .items
                    .iter()
                    .filter_map(|item| match item {
                        MenuItem::Action(action) => Some(action.text.replace('&', "")),
                        _ => None,
                    })
                    .collect();
                result.families[at].status_tip = format!("{name}: {}", variants.join(" / "));
                // Its place in the category's menu takes the family's menu as filled.
                if let Some(menu) = menu.as_mut() {
                    let family_menu = result.families[at].menu.clone();
                    for item in &mut menu.items {
                        if let MenuItem::Menu(placed) = item
                            && placed.name == family_menu.name
                        {
                            *placed = family_menu.clone();
                        }
                    }
                }
            }
        }

        if let Some(menu) = menu {
            filled.push(String::from(category));
            if made {
                result.made.insert(String::from(category), menu);
            } else {
                targets.menus.insert(String::from(category), menu);
            }
        }
        if let Some(bar) = tool_bar {
            targets.tool_bars.insert(String::from(category), bar);
        }
    }

    // Last, when every item is in: a letter is chosen against the whole menu. Then the actions
    // take the texts their menus gave them, as the one QAction a menu and a toolbar shared did.
    let mut texts: BTreeMap<String, String> = BTreeMap::new();
    let mut filled_families: BTreeMap<String, Menu> = BTreeMap::new();
    for category in &filled {
        let menu = match targets.menus.get_mut(category) {
            Some(menu) => menu,
            None => match result.made.get_mut(category) {
                Some(menu) => menu,
                None => continue,
            },
        };
        assign_mnemonics(menu);
        collect_texts(menu, &mut texts, &mut filled_families);
    }
    for (name, text) in texts {
        if let Some(action) = result.actions.get_mut(&name) {
            action.text = text;
        }
    }
    for family in &mut result.families {
        if let Some(menu) = filled_families.remove(&family.menu.name) {
            family.menu = menu;
        }
    }
    for bar in targets.tool_bars.values_mut() {
        result.refresh_tool_bar(bar);
    }
    result
}

/// A menu for `category` on `bar`, after the last of the window's tool menus there, titled with a
/// menu letter no other menu on the bar has.
fn make_category_menu(
    bar: &mut Vec<BarMenu>,
    targets: &BTreeMap<String, Menu>,
    category: &str,
    after: &mut Option<String>,
) -> Menu {
    let mut taken = String::new();
    for top in bar.iter() {
        if let Some(letter) = mnemonic_of(&top.title) {
            take_letter(&mut taken, letter);
        }
    }
    let title = with_mnemonic(category, &mut taken);
    let name = made_menu_name(category);
    if after.is_none() {
        // After the last target menu on the bar, in the bar's order.
        for top in bar.iter() {
            if targets.values().any(|target| target.name == top.name) {
                *after = Some(top.name.clone());
            }
        }
    }
    let index = after
        .as_ref()
        .and_then(|after| bar.iter().position(|top| top.name == *after));
    let entry = BarMenu::new(&name, &title);
    match index {
        Some(index) if index + 1 < bar.len() => bar.insert(index + 1, entry),
        _ => bar.push(entry),
    }
    *after = Some(name.clone());
    Menu::new(&name, &title)
}
