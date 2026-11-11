//! The window's actions as rows of data: what the C++'s QActions were - a menu item and the
//! toolbar button that runs the same command, one object with one object name - so menus and
//! toolbars are drawn from tables, and a driver looks an action up without a frame (`*NAME` and
//! `@NAME` in a headless run, `--check-menus` reading every row). Each workbench delivers its
//! rows; the main window (K) composes them in the C++'s order.

use egui::{Key, KeyboardShortcut, Response, Ui};

use super::mailbox::Act;
use super::registry::{ControlKind, ControlRecord, take};

/// One action: a QAction's object name, text, icon, status tip, shortcut, check state and the
/// dialog (or dock) it opens - its `data()`, by which `--dialog` finds the dialog an action opens.
#[derive(Clone, Debug, PartialEq)]
pub struct Action {
    /// The C++ object name ("formatStyles").
    pub name: String,
    /// The text as the C++ wrote it, '&' before the letter Alt reaches it by ("&Styles...").
    pub text: String,
    /// The icon's name in the window's icon set (I's icons), empty for none.
    pub icon: &'static str,
    /// The status tip: the sentence the status bar shows, and with the shortcut the tooltip.
    pub tip: String,
    pub shortcut: Option<KeyboardShortcut>,
    /// Some for an action that can be checked: whether it is.
    pub checked: Option<bool>,
    pub enabled: bool,
    /// The object name of the dialog or dock it opens (QAction::data), when it opens one.
    pub dialog: Option<String>,
}

impl Action {
    /// An enabled action with no icon, tip, shortcut or dialog.
    pub fn new(name: &str, text: &str) -> Self {
        Self {
            name: String::from(name),
            text: String::from(text),
            icon: "",
            tip: String::new(),
            shortcut: None,
            checked: None,
            enabled: true,
            dialog: None,
        }
    }

    pub fn icon(mut self, icon: &'static str) -> Self {
        self.icon = icon;
        self
    }

    pub fn tip(mut self, tip: &str) -> Self {
        self.tip = String::from(tip);
        self
    }

    pub fn shortcut(mut self, shortcut: KeyboardShortcut) -> Self {
        self.shortcut = Some(shortcut);
        self
    }

    pub fn checkable(mut self, checked: bool) -> Self {
        self.checked = Some(checked);
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn opens(mut self, dialog: &str) -> Self {
        self.dialog = Some(String::from(dialog));
        self
    }

    /// The text as a menu shows it: '&&' is an ampersand, a single '&' marks the letter and is not
    /// shown.
    pub fn display_text(&self) -> String {
        let mut shown = String::with_capacity(self.text.len());
        let mut characters = self.text.chars().peekable();
        while let Some(character) = characters.next() {
            if character == '&' {
                if characters.peek() == Some(&'&') {
                    characters.next();
                    shown.push('&');
                }
                continue;
            }
            shown.push(character);
        }
        shown
    }

    /// What `--report NAME` says of an action (main.cpp's reportWidget): its text with every '&'
    /// removed, and ", checked" or ", unchecked" when it can be checked.
    pub fn report(&self) -> String {
        let mut text = self.text.replace('&', "");
        match self.checked {
            Some(true) => text.push_str(", checked"),
            Some(false) => text.push_str(", unchecked"),
            None => {}
        }
        text
    }

    /// The tooltip the window makes of an action: its tip, and its shortcut after it in
    /// parentheses.
    pub fn tooltip(&self) -> String {
        match &self.shortcut {
            Some(shortcut) => format!("{} ({})", self.tip, shortcut_text(shortcut)),
            None => self.tip.clone(),
        }
    }
}

/// A key's name as Qt writes it in a key sequence (QKeySequence::PortableText): "Esc", "Del",
/// "PgUp", "Return", a punctuation key by its character.
fn qt_key_name(key: Key) -> &'static str {
    match key {
        Key::Escape => "Esc",
        Key::Enter => "Return",
        Key::Insert => "Ins",
        Key::Delete => "Del",
        Key::PageUp => "PgUp",
        Key::PageDown => "PgDown",
        Key::Minus => "-",
        Key::Quote => "'",
        Key::Colon
        | Key::Comma
        | Key::Period
        | Key::Plus
        | Key::Equals
        | Key::Semicolon
        | Key::Backslash
        | Key::Slash
        | Key::Pipe
        | Key::Questionmark
        | Key::Exclamationmark
        | Key::OpenBracket
        | Key::CloseBracket
        | Key::OpenCurlyBracket
        | Key::CloseCurlyBracket
        | Key::Backtick => key.symbol_or_name(),
        _ => key.name(),
    }
}

/// A shortcut as Qt's portable text writes it ("Ctrl+Shift+L"): the modifiers in Qt's order -
/// Ctrl, Alt, Shift, Meta - then the key. egui's "command" is Ctrl off a Mac, and so it is here.
pub fn shortcut_text(shortcut: &KeyboardShortcut) -> String {
    let modifiers = shortcut.modifiers;
    let mut text = String::new();
    if modifiers.ctrl || modifiers.command {
        text.push_str("Ctrl+");
    }
    if modifiers.alt {
        text.push_str("Alt+");
    }
    if modifiers.shift {
        text.push_str("Shift+");
    }
    if modifiers.mac_cmd && !modifiers.command {
        text.push_str("Meta+");
    }
    text.push_str(qt_key_name(shortcut.logical_key));
    text
}

/// One item of a menu.
#[derive(Clone, Debug, PartialEq)]
pub enum MenuItem {
    Action(Action),
    /// A titled section (QMenu::addSection): its title.
    Section(String),
    Separator,
    /// A submenu.
    Menu(Menu),
}

/// A menu: its object name ("formatMenu"), its title with its '&', and its items.
#[derive(Clone, Debug, PartialEq)]
pub struct Menu {
    pub name: String,
    pub title: String,
    pub items: Vec<MenuItem>,
}

impl Menu {
    pub fn new(name: &str, title: &str) -> Self {
        Self {
            name: String::from(name),
            title: String::from(title),
            items: Vec::new(),
        }
    }

    pub fn action(mut self, action: Action) -> Self {
        self.items.push(MenuItem::Action(action));
        self
    }

    pub fn section(mut self, title: &str) -> Self {
        self.items.push(MenuItem::Section(String::from(title)));
        self
    }

    pub fn separator(mut self) -> Self {
        self.items.push(MenuItem::Separator);
        self
    }

    pub fn submenu(mut self, menu: Menu) -> Self {
        self.items.push(MenuItem::Menu(menu));
        self
    }

    /// What `--report NAME` says of a menu (reportWidget): its title without '&', then its items
    /// that are not separators, each without '&' and with its status tip in brackets, joined by
    /// " ; ". A section is a separator to Qt (QMenu::addSection makes a separator action that
    /// carries the title) and is left out with them. A submenu is its title.
    pub fn report(&self) -> String {
        let items: Vec<String> = self
            .items
            .iter()
            .filter_map(|item| match item {
                MenuItem::Action(action) => {
                    let mut line = action.text.replace('&', "");
                    if !action.tip.is_empty() {
                        line.push_str(&format!(" [{}]", action.tip));
                    }
                    Some(line)
                }
                MenuItem::Section(_) | MenuItem::Separator => None,
                MenuItem::Menu(menu) => Some(menu.title.replace('&', "")),
            })
            .collect();
        format!("{}: {}", self.title.replace('&', ""), items.join(" ; "))
    }

    /// Every action of the menu and its submenus, depth first in their order.
    pub fn actions(&self) -> Vec<&Action> {
        let mut found = Vec::new();
        for item in &self.items {
            match item {
                MenuItem::Action(action) => found.push(action),
                MenuItem::Menu(menu) => found.extend(menu.actions()),
                MenuItem::Section(_) | MenuItem::Separator => {}
            }
        }
        found
    }
}

/// One item of a toolbar.
#[derive(Clone, Debug, PartialEq)]
pub enum ToolBarItem {
    Action(Action),
    Separator,
    /// A control the toolbar's owner draws itself (a tool family's button, the status bar's CRS
    /// button), by its object name.
    Widget(String),
}

/// A toolbar: its object name ("SurveyToolBar"), the title its name label and View > Toolbars
/// show, and its items.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolBar {
    pub name: String,
    pub title: String,
    pub items: Vec<ToolBarItem>,
}

impl ToolBar {
    pub fn new(name: &str, title: &str) -> Self {
        Self {
            name: String::from(name),
            title: String::from(title),
            items: Vec::new(),
        }
    }

    /// Every action of the toolbar, in order.
    pub fn actions(&self) -> Vec<&Action> {
        self.items
            .iter()
            .filter_map(|item| match item {
                ToolBarItem::Action(action) => Some(action),
                ToolBarItem::Separator | ToolBarItem::Widget(_) => None,
            })
            .collect()
    }
}

/// An action drawn as a menu item, named by its object name: its text, its shortcut beside it,
/// checked when it is, and greyed when it is disabled. A click - a person's or a press - is
/// `clicked()`; the caller triggers the action. A disabled item refuses a press as a disabled
/// button does.
pub fn action_item(ui: &mut Ui, action: &Action) -> Response {
    let taken = take(ui, &action.name);
    ui.add_enabled_ui(action.enabled, |ui| {
        let text = action.display_text();
        let mut button = match action.checked {
            Some(checked) => egui::Button::selectable(checked, text),
            None => egui::Button::new(text),
        };
        if let Some(shortcut) = &action.shortcut {
            button = button.shortcut_text(shortcut_text(shortcut));
        }
        let mut response = ui.add(button);
        if let Some(taken) = &taken {
            match &taken.posted.act {
                Act::Press => {
                    // Refused hidden, then disabled, as every named control refuses a press.
                    if super::controls::accept_press(ui, taken) {
                        response
                            .flags
                            .insert(egui::response::Flags::FAKE_PRIMARY_CLICKED);
                        super::registry::settle(ui, taken, Ok(()));
                    }
                }
                Act::Fill(text) => {
                    if super::controls::accept_fill(ui, taken) {
                        super::registry::settle(
                            ui,
                            taken,
                            Err(super::mailbox::no_field(&action.name, text)),
                        );
                    }
                }
                Act::ScrollTo => {
                    response.scroll_to_me(None);
                    super::registry::settle(ui, taken, Ok(()));
                }
            }
        }
        let mut record = ControlRecord::new(&action.name, ControlKind::MenuItem);
        record.text = action.text.clone();
        record.checked = action.checked;
        record.enabled = ui.is_enabled();
        record.visible = ui.is_visible();
        super::registry::record(ui, &response, record);
        response
    })
    .inner
}

#[cfg(test)]
mod tests {
    use egui::{Key, KeyboardShortcut, Modifiers};

    use super::{Action, Menu, shortcut_text};

    #[test]
    fn a_shortcut_is_written_as_qts_portable_text() {
        let layers = KeyboardShortcut::new(Modifiers::CTRL | Modifiers::SHIFT, Key::L);
        assert_eq!(shortcut_text(&layers), "Ctrl+Shift+L");
        assert_eq!(
            shortcut_text(&KeyboardShortcut::new(Modifiers::NONE, Key::Delete)),
            "Del"
        );
        assert_eq!(
            shortcut_text(&KeyboardShortcut::new(Modifiers::NONE, Key::F9)),
            "F9"
        );
    }

    #[test]
    fn a_menu_reports_its_title_and_items_as_report_widget_does() {
        let menu = Menu::new("formatMenu", "F&ormat")
            .section("Styles")
            .action(Action::new("formatStyles", "&Styles...").tip("Edit the styles"))
            .separator()
            .action(Action::new("formatLayers", "&Layers"));
        assert_eq!(
            menu.report(),
            "Format: Styles... [Edit the styles] ; Layers"
        );
        assert_eq!(
            Action::new("a", "Save && Close").display_text(),
            "Save & Close"
        );
        assert_eq!(
            Action::new("grid", "&Grid").checkable(true).report(),
            "Grid, checked"
        );
    }
}
