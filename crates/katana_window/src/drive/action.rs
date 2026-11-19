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

/// A menu: its object name ("formatMenu"), its title with its '&', its items, and the status tip
/// of the item that opens it (menuAction()->setStatusTip), empty for none.
#[derive(Clone, Debug, PartialEq)]
pub struct Menu {
    pub name: String,
    pub title: String,
    pub items: Vec<MenuItem>,
    pub tip: String,
    /// The icon of the item that opens it as a submenu (menuAction()->setIcon), empty for none.
    pub icon: &'static str,
}

impl Menu {
    pub fn new(name: &str, title: &str) -> Self {
        Self {
            name: String::from(name),
            title: String::from(title),
            items: Vec::new(),
            tip: String::new(),
            icon: "",
        }
    }

    /// The icon of the item that opens the menu, as a submenu shows it beside its title.
    pub fn icon(mut self, icon: &'static str) -> Self {
        self.icon = icon;
        self
    }

    /// The status tip of the item that opens the menu: what a parent menu's report shows of it in
    /// brackets, as of an action.
    pub fn tip(mut self, tip: &str) -> Self {
        self.tip = String::from(tip);
        self
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
    /// carries the title) and is left out with them. A submenu is its title, and its status tip
    /// in brackets when it has one: Qt's submenu is the action menuAction(), reported as any.
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
                MenuItem::Menu(menu) => {
                    let mut line = menu.title.replace('&', "");
                    if !menu.tip.is_empty() {
                        line.push_str(&format!(" [{}]", menu.tip));
                    }
                    Some(line)
                }
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

/// The id of a menu item's gutter atom (theme::style::paint_menu_gutter).
fn gutter_id() -> egui::Id {
    egui::Id::new("katanaMenuGutter")
}

/// The id of the space after a menu item's shortcut or submenu arrow (`right_trail`).
fn trail_id() -> egui::Id {
    egui::Id::new("katanaMenuTrail")
}

/// The space after a menu item's shortcut or submenu arrow, so they stand as far in from the
/// menu's right edge as its icons do from its left: the button's padding and a check column's
/// width (theme::style::MENU_CHECK_WIDTH), the gap egui leaves before an atom included. They sat
/// the button's 2 points from the edge against the icons' 14 (the sixth look audit).
fn right_trail(ui: &Ui) -> egui::Atom<'static> {
    let width = (crate::theme::style::MENU_CHECK_WIDTH - ui.spacing().icon_spacing).max(0.0);
    egui::Atom::custom(trail_id(), egui::vec2(width, 0.0))
}

/// A menu item's button: its gutter (theme::style::MENU_GUTTER_WIDTH, a menu icon high) then
/// `text`, unframed until the pointer is on it wherever it is drawn - a menu bar's popup gives its
/// items egui's menu style, a popup of the window's own (shell::popup_menu) does not.
fn item_button<'a>(text: impl Into<egui::WidgetText>) -> egui::Button<'a> {
    use crate::theme::style::{MENU_GUTTER_WIDTH, MENU_ICON_SIZE};
    egui::Button::new((
        egui::Atom::custom(gutter_id(), egui::vec2(MENU_GUTTER_WIDTH, MENU_ICON_SIZE)),
        text.into(),
    ))
    .frame_when_inactive(false)
}

/// `item_button` for a row with no shortcut or arrow: its gutter, `text` and then `trail`
/// (`right_trail`), so the row's text ends as far in from the menu's right edge as its icon starts
/// from the left.
fn plain_item_button<'a>(
    text: impl Into<egui::WidgetText>,
    trail: egui::Atom<'static>,
) -> egui::Button<'a> {
    use crate::theme::style::{MENU_GUTTER_WIDTH, MENU_ICON_SIZE};
    egui::Button::new((
        egui::Atom::custom(gutter_id(), egui::vec2(MENU_GUTTER_WIDTH, MENU_ICON_SIZE)),
        text.into(),
        trail,
    ))
    .frame_when_inactive(false)
}

/// An action drawn as a menu item, named by its object name: its icon and its text, its shortcut
/// beside it, a tick before the icon while it is checked (theme::style::paint_menu_gutter), and
/// greyed when it is disabled. A click - a person's or a press - is `clicked()`; the caller
/// triggers the action. A disabled item refuses a press as a disabled button does.
pub fn action_item(ui: &mut Ui, action: &Action) -> Response {
    let taken = take(ui, &action.name);
    ui.add_enabled_ui(action.enabled, |ui| {
        let text = action.display_text();
        let button = match &action.shortcut {
            Some(shortcut) => item_button(text.clone())
                .shortcut_text((shortcut_text(shortcut), right_trail(ui))),
            // In a menu, a row with nothing on its right keeps the same space after its text: a
            // menu is as wide as its widest row, and when that was a plain one ("Sheets and
            // Plotting Commands...") its text ran to the button's 2 points from the popup's edge
            // against the icons' 14 on the left (the seventh look audit). The trail follows the
            // text with no space grown before it, so the text stays where it was. A toolbar's
            // button (the sheet editor's, the minimised panels' bar) and a torn-off menu's
            // columns, which are not in a popup, keep their text's own width.
            None if egui::containers::menu::is_in_menu(ui) => {
                plain_item_button(text.clone(), right_trail(ui))
            }
            None => item_button(text.clone()),
        };
        let shown = button.atom_ui(ui);
        if let Some(gutter) = shown.rect(gutter_id()) {
            crate::theme::style::paint_menu_gutter(
                ui.painter(),
                gutter,
                action.icon,
                action.checked == Some(true),
            );
        }
        let mut response = shown.response;
        if let Some(checked) = action.checked {
            // A screen reader hears a checkable item as checked or not, as egui's selectable
            // button said it; the look is the gutter's tick.
            let enabled = ui.is_enabled();
            response.widget_info(|| {
                egui::WidgetInfo::selected(egui::WidgetType::Checkbox, enabled, checked, &text)
            });
        }
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

/// The items of `menu` drawn into a menu's `ui`, the one way every menu of the window is drawn
/// (the menu bar's, a step's `--panel` of one, the plan view's shortcut menu, the sheet editor's):
/// each action an `action_item`, a titled section its title (theme::menu_section), a separator a
/// rule inset as the items are (theme::menu_separator), and a submenu an item with its icon, its
/// title and an arrow, whose own items open beside it in one column that scrolls when it is
/// taller than the screen (shell::dialog::menu_column). The action clicked, if any, is put in
/// `chosen`; an action with no name (a list's "(none)") is never chosen.
///
/// The caller puts the menu's own items in a `menu_column` too, where its popup does not already
/// (shell::popup_menu does).
pub fn menu_items_ui(ui: &mut Ui, menu: &Menu, chosen: &mut Option<String>) {
    items_ui(ui, menu, chosen, None);
}

/// `menu_items_ui` for a menu of the menu bar (and a step's `--panel` of one): a tear-off line
/// at the top of the menu and of each of its submenus (`tear_off_line`), and the menu whose line
/// was clicked put in `torn` with where its line was - the window tears it off into a panel of
/// its own there (main_window::docking_ui).
pub fn menu_items_tearable_ui(
    ui: &mut Ui,
    menu: &Menu,
    chosen: &mut Option<String>,
    torn: &mut Option<(String, egui::Pos2)>,
) {
    items_ui(ui, menu, chosen, Some(torn));
}

fn items_ui(
    ui: &mut Ui,
    menu: &Menu,
    chosen: &mut Option<String>,
    mut torn: Option<&mut Option<(String, egui::Pos2)>>,
) {
    // egui's menu look - items unframed, the pointer's item shaded - also where the popup is the
    // window's own rather than egui's (shell::popup_menu, a step's menu), which gave every item a
    // button's border.
    egui::containers::menu::menu_style(ui.style_mut());
    if torn.is_some() {
        // A popup of the menu bar is as wide as its widest item, whose text never wraps: egui
        // sizes the popup before its column scrolls, and the scroll bar that a menu taller than
        // the screen then shows took its width from the items - View > Toolbars' Show Toolbar
        // Names wrapped onto two lines once the tear-off line no longer made every menu 590
        // points wide.
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
    }
    if let Some(torn) = torn.as_deref_mut() {
        let line = tear_off_line(ui, menu);
        if line.clicked() {
            *torn = Some((menu.name.clone(), line.rect.min));
        }
    }
    for item in &menu.items {
        let _ = item_ui(ui, item, chosen, torn.as_deref_mut());
    }
}

/// One item of a menu drawn into a menu's `ui` as `menu_items_ui` draws each, with what it drew:
/// for a torn-off menu that lays its items out itself, in columns along a band
/// (main_window::docking_ui). The caller gives `ui` egui's menu look
/// (egui::containers::menu::menu_style), as `menu_items_ui` does.
pub fn menu_item_ui(ui: &mut Ui, item: &MenuItem, chosen: &mut Option<String>) -> Response {
    item_ui(ui, item, chosen, None)
}

fn item_ui(
    ui: &mut Ui,
    item: &MenuItem,
    chosen: &mut Option<String>,
    torn: Option<&mut Option<(String, egui::Pos2)>>,
) -> Response {
    match item {
        MenuItem::Action(action) => {
            let response = action_item(ui, action);
            if response.clicked() && !action.name.is_empty() {
                *chosen = Some(action.name.clone());
            }
            response
        }
        MenuItem::Section(title) => crate::theme::menu_section(ui, title),
        MenuItem::Separator => crate::theme::menu_separator(ui),
        MenuItem::Menu(submenu) => submenu_item(ui, submenu, |ui| {
            crate::shell::dialog::menu_column(ui, |ui| {
                items_ui(ui, submenu, chosen, torn);
            });
        }),
    }
}

/// The tear-off line's height: a dashed rule and a few points over and under it to click.
pub const TEAR_OFF_HEIGHT: f32 = 9.0;
/// The least the tear-off line asks of a menu's width: a few dashes between its 6 point insets.
/// Every menu's items are wider, so it never widens one.
pub const TEAR_OFF_LEAST_WIDTH: f32 = 48.0;

/// The object name of `menu`'s tear-off line: `tearOff` and the menu's object name with its first
/// letter upper case ("tearOffViewMenu"), as the action that tears it off is named after it
/// (`viewMenuTearOffViewMenu`, main_window::docking).
pub fn tear_off_name(menu: &str) -> String {
    let mut characters = menu.chars();
    match characters.next() {
        Some(first) => format!(
            "tearOff{}{}",
            first.to_uppercase(),
            characters.as_str()
        ),
        None => String::from("tearOff"),
    }
}

/// The tear-off line at the top of a menu (QMenu::setTearOffEnabled's): a dashed rule in the muted
/// token, the text's colour under the pointer, a person clicks to keep the menu open as a panel.
/// A named control (`tear_off_name`), so a step presses it as a click does.
pub fn tear_off_line(ui: &mut Ui, menu: &Menu) -> Response {
    let name = tear_off_name(&menu.name);
    let taken = take(ui, &name);
    // The line takes the menu's width and never sets it. egui lays a popup out first in a sizing
    // pass, unjustified, offered its default area's width; a line as wide as that room made every
    // menu that wide - 590 points, where File needs 201 and Edit 157 - and flung the shortcuts
    // and submenu arrows to its far edge (the sixth look audit). Asked for its least width, it is
    // that in the sizing pass, and the column's whole width afterwards, since a menu's column is
    // justified (`Ui::allocate_response` gives a justified layout's widget the full width).
    let mut response = ui.allocate_response(
        egui::vec2(TEAR_OFF_LEAST_WIDTH, TEAR_OFF_HEIGHT),
        egui::Sense::click(),
    );
    let rect = response.rect;
    let hovered = response.hovered();
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if hovered {
            painter.rect_filled(
                rect,
                egui::CornerRadius::same(2),
                crate::theme::palette::hover().to_egui(),
            );
        }
        let colour = if hovered {
            crate::theme::palette::text()
        } else {
            crate::theme::palette::text_muted()
        }
        .to_egui();
        let y = painter.round_to_pixel_center(rect.center().y);
        painter.add(egui::Shape::dashed_line(
            &[
                egui::pos2(rect.left() + 6.0, y),
                egui::pos2(rect.right() - 6.0, y),
            ],
            egui::Stroke::new(1.0, colour),
            3.0,
            2.0,
        ));
    }
    let title = menu.title.replace('&', "");
    response = response.on_hover_text(format!(
        "Tear off: keep the {title} menu open as a panel, docked or floating"
    ));
    if let Some(taken) = &taken {
        match &taken.posted.act {
            Act::Press => {
                if super::controls::accept_press(ui, taken) {
                    response
                        .flags
                        .insert(egui::response::Flags::FAKE_PRIMARY_CLICKED);
                    super::registry::settle(ui, taken, Ok(()));
                }
            }
            Act::Fill(text) => {
                if super::controls::accept_fill(ui, taken) {
                    super::registry::settle(ui, taken, Err(super::mailbox::no_field(&name, text)));
                }
            }
            Act::ScrollTo => {
                response.scroll_to_me(None);
                super::registry::settle(ui, taken, Ok(()));
            }
        }
    }
    let mut record = ControlRecord::new(&name, ControlKind::Button);
    record.text = format!("Tear off {title}");
    record.enabled = ui.is_enabled();
    record.visible = ui.is_visible();
    super::registry::record(ui, &response, record);
    response
}

/// The item that opens `submenu` (egui's SubMenuButton in a menu, a MenuButton elsewhere, as
/// `Ui::menu_button` chooses): its icon in the gutter its siblings have, its title, and the arrow
/// egui draws a submenu with; `add` draws what it opens.
fn submenu_item(ui: &mut Ui, submenu: &Menu, add: impl FnOnce(&mut Ui)) -> Response {
    let button = item_button(submenu.title.replace('&', "")).right_text((
        egui::containers::menu::SubMenuButton::RIGHT_ARROW,
        right_trail(ui),
    ));
    let response = if egui::containers::menu::is_in_menu(ui) {
        egui::containers::menu::SubMenuButton::from_button(button)
            .ui(ui, add)
            .0
    } else {
        egui::containers::menu::MenuButton::from_button(button)
            .ui(ui, add)
            .0
    };
    // Where item_button lays its gutter out: its button padding in from the left, centred down.
    let gutter = egui::Rect::from_min_size(
        egui::pos2(
            response.rect.left() + ui.spacing().button_padding.x,
            response.rect.center().y - crate::theme::style::MENU_ICON_SIZE / 2.0,
        ),
        egui::vec2(
            crate::theme::style::MENU_GUTTER_WIDTH,
            crate::theme::style::MENU_ICON_SIZE,
        ),
    );
    if ui.is_rect_visible(gutter) {
        crate::theme::style::paint_menu_gutter(ui.painter(), gutter, submenu.icon, false);
    }
    response
}

#[cfg(test)]
mod tests {
    use egui::{Key, KeyboardShortcut, Modifiers};

    use super::{Action, Menu, menu_items_tearable_ui, menu_items_ui, shortcut_text};

    /// A menu like File: sections, items with and without shortcuts, a submenu.
    fn file_like() -> Menu {
        Menu::new("fileMenu", "&File")
            .section("Drawing")
            .action(
                Action::new("fileNew", "&New")
                    .icon("New")
                    .shortcut(KeyboardShortcut::new(Modifiers::COMMAND, Key::N)),
            )
            .action(
                Action::new("fileSaveAs", "Save &As...")
                    .icon("SaveAs")
                    .shortcut(KeyboardShortcut::new(
                        Modifiers::COMMAND | Modifiers::SHIFT,
                        Key::S,
                    )),
            )
            .section("Import and export")
            .submenu(Menu::new("fileImportMenu", "&Import").icon("Import"))
            .action(Action::new("fileQuit", "&Quit").icon("Quit"))
    }

    /// The width of `menu` popped up as the menu bar pops one up - an Area, laid out first in
    /// egui's sizing pass, framed as a menu with its items in one column - with its tear-off line
    /// when `tearable`, after the frames that settle it.
    fn popup_width(menu: &Menu, tearable: bool) -> f32 {
        let mut harness = crate::drive::Harness::new();
        let mut width = 0.0;
        for _ in 0..4 {
            let _ = harness.frame(|ui| {
                let shown = egui::Area::new(egui::Id::new("menuUnderTest"))
                    .fixed_pos(egui::pos2(10.0, 10.0))
                    .show(ui.ctx(), |ui| {
                        egui::Frame::menu(ui.style()).show(ui, |ui| {
                            ui.with_layout(
                                egui::Layout::top_down_justified(egui::Align::Min),
                                |ui| {
                                    crate::shell::dialog::menu_column(ui, |ui| {
                                        let mut chosen = None;
                                        let mut torn = None;
                                        if tearable {
                                            menu_items_tearable_ui(ui, menu, &mut chosen, &mut torn);
                                        } else {
                                            menu_items_ui(ui, menu, &mut chosen);
                                        }
                                    });
                                },
                            );
                        });
                    });
                width = shown.response.rect.width();
            });
        }
        width
    }

    #[test]
    fn a_menus_tear_off_line_takes_the_menus_width_and_never_widens_it() {
        // The line asked for the room egui's sizing pass offers a popup, and every menu was 590
        // points wide: File needs some 200 (the sixth look audit).
        let menu = file_like();
        let without = popup_width(&menu, false);
        let with = popup_width(&menu, true);
        assert!(without > 100.0, "the premise: the menu is drawn ({without})");
        assert!(
            (with - without).abs() <= 1.0,
            "with its tear-off line {with}, without {without}"
        );
        assert!(with < 300.0, "{with}");
    }

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
            .action(Action::new("formatLayers", "&Layers"))
            .submenu(Menu::new("formatPlain", "&Plain"))
            .submenu(Menu::new("formatTipped", "&Tipped").tip("Opens more"));
        assert_eq!(
            menu.report(),
            "Format: Styles... [Edit the styles] ; Layers ; Plain ; Tipped [Opens more]"
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
