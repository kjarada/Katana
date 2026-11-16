//! Where the main window's parts go, and the window between sessions: the geometry its panels,
//! toolbars and views are laid out in, the person's settings it keeps (restoreSession,
//! saveSession), and the window's own checks - `--check-shortcuts` (shortcutClashes),
//! `--check-menus` (menuGaps), `--check-toolbars` (menuSignClashes), `--check-toolbar-fit`
//! (toolBarFit) and `--check-session-restore`.
//!
//! THE GEOMETRY is the design of 2026-11-16 (the owner: "panels are oversized"), on the C++'s
//! arrangement: at 1360 x 860 the menu bar (26) and three rows of toolbars (39 each) take the top
//! 143 points; Draw and Annotate run down the left in 39, Modify down the right in 39; the
//! Drafting bar (30) and the status bar (24) take the bottom 54; the Layers dock is at 39,143
//! 240 x 312 over Reference Data at 39,459 240 x 347; Properties at 1081,143 240 x 663; the
//! command line at 283,696 794 x 110 under the views, which have 283,143 794 x 549. The C++'s,
//! measured from its own headless screenshots (docs/rust.md, "The headless window has the C++'s
//! geometry"), were a 42 point menu bar, 45 point side columns, 300 point panels, a 150 point
//! command line, a 73 point bottom and 5 point gaps: a drawing area of 660 x 473 in the same
//! window, where this leaves 794 x 549 (41 % more). The panels keep their widths and the command
//! line its height at every size, as Qt's resizeDocks left them, and the views take what is left;
//! a panel hidden gives its room to the views. Rejected: egui_dock for the four panels, whose
//! split shares would not hold these numbers as the window is sized, and whose tabs would draw a
//! strip over every panel.
//!
//! THE SETTINGS FILE (decided here; the plan's section 4.11): the person's settings are the
//! window's own JSON file (host::UserSettings), under the keys the C++ gave QSettings -
//! `window/layout`, `window/minimised`, `appearance/textSize`, `appearance/toolBarIcons`,
//! `appearance/toolBarNames`, `start/showAtStart`, `start/coordinateSystem`, `scripts/recent`,
//! `files/projectFolder` - so a reader of either finds the same names. The layout is the window's
//! own form, a JSON object of its own (`LAYOUT_VERSION`): which panels and toolbars are shown and
//! the toolbars' rows, with a version as saveState had one. A headless run reads and writes none
//! of it.
//!
//! THE OLDER BUILD'S LAYOUT (decided here, for --check-session-restore and --check-toolbar-fit):
//! a version 1 layout in the Rust's own form - every toolbar of the second row, Surface Edit and
//! BIM beside Survey, Terrain and GIS, as every layout saved until 2026-10-15 had them - which
//! `restore_layout` carries over by seating Surface Edit and BIM on a third row of their own.

use std::collections::{BTreeMap, BTreeSet};

use egui::{Pos2, Rect};
use katana_app::console::Console;

use super::actions::{BarPlace, Composed};
use super::window::MainWindow;
use crate::drive::action::{Action, Menu, MenuItem, ToolBar, ToolBarItem, shortcut_text};
use crate::drive::program::MenuSigns;
use crate::host::{SettingValue, UserSettings};
use crate::keyboard_shortcuts_dialog::ShortcutRow;
use crate::tools::flyout_button::{
    ButtonIcon, ButtonPaint, ButtonState, MeasuredButton, MenuSign, SignHolder, Strip, StripItem,
    TOOL_BAR_PADDING_ACROSS, ToolButtonLook, menu_sign_clashes as measure_signs, split_paint,
    strip_sign_holder,
};

// ---- the keys of the settings file ---------------------------------------------------------

pub const LAYOUT_KEY: &str = "window/layout";
pub const MINIMISED_KEY: &str = "window/minimised";
pub const TEXT_SIZE_KEY: &str = "appearance/textSize";
pub const TOOL_BAR_ICONS_KEY: &str = "appearance/toolBarIcons";
pub const TOOL_BAR_NAMES_KEY: &str = "appearance/toolBarNames";
pub const START_SHOW_KEY: &str = "start/showAtStart";
pub const START_CRS_KEY: &str = "start/coordinateSystem";

/// The layout's version this window saves, and the one before the third toolbar row
/// (MainWindow::kLayoutVersion, kLayoutVersionBeforeThirdRow).
pub const LAYOUT_VERSION: i64 = 2;
pub const LAYOUT_VERSION_BEFORE_THIRD_ROW: i64 = 1;

// ---- the geometry ----------------------------------------------------------------------------

/// The menu bar: one row of menu titles, a control's height and a point either side.
pub const MENU_BAR: f32 = crate::theme::style::CONTROL_HEIGHT + 2.0;
/// One row of toolbars: a 20 px icon's button (33) and the bar's padding across it.
pub const TOOL_BAR_ROW: f32 = 39.0;
/// The menu bar and the three rows of toolbars along the top.
pub const TOP: f32 = MENU_BAR + 3.0 * TOOL_BAR_ROW;
/// Draw and Annotate down the left side, Modify down the right: a toolbar's breadth, nothing
/// beside it (the C++'s 45 left 6 points of nothing between the bar and the panel).
pub const SIDE: f32 = TOOL_BAR_ROW;
/// The Drafting bar: its toggles are words, a control high, with the bar's padding across them.
pub const DRAFTING_BAR: f32 = crate::theme::style::CONTROL_HEIGHT + 6.0;
/// The status bar, under the Drafting bar: a line of secondary text and its padding.
pub const STATUS_BAR: f32 = crate::theme::style::CONTROL_HEIGHT;
/// The Drafting bar and the status bar along the bottom.
pub const BOTTOM: f32 = DRAFTING_BAR + STATUS_BAR;
/// The side panels' width and the command line's height (resizeDocks): the design's 240 and 110,
/// where the C++'s 300 and 150 took more of a laptop's screen than the drawing did.
pub const PANEL_WIDTH: f32 = 240.0;
pub const COMMAND_LINE_HEIGHT: f32 = 110.0;
/// Between two docks, and between a dock and the views: the design's 4, the frame's colour
/// showing between the panels' (QMainWindow's separator was 5).
pub const SEPARATOR: f32 = 4.0;
/// Layers' share of the left column, measured on the C++: 295 of the 623 it shares with Reference
/// Data.
const LAYERS_SHARE: f32 = 295.0 / 623.0;

/// Which panels are shown, and which of the left column's lower tab group is in front.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Docks {
    pub layers: bool,
    pub properties: bool,
    pub command_line: bool,
    pub reference: bool,
    /// The Surfaces panel: hidden until asked for, tabbed with Reference Data.
    pub surfaces: bool,
    /// Whether Surfaces is the page in front of its tab group (else Reference Data is).
    pub surfaces_in_front: bool,
}

impl Default for Docks {
    fn default() -> Self {
        Self {
            layers: true,
            properties: true,
            command_line: true,
            reference: true,
            surfaces: false,
            surfaces_in_front: false,
        }
    }
}

impl Docks {
    /// The dock in front of the left column's lower tab group: ReferenceDataDock or
    /// SurfacesDock; None when neither is shown.
    pub fn lower_left(&self) -> Option<&'static str> {
        match (self.reference, self.surfaces) {
            (false, false) => None,
            (true, false) => Some("ReferenceDataDock"),
            (false, true) => Some("SurfacesDock"),
            (true, true) => Some(if self.surfaces_in_front {
                "SurfacesDock"
            } else {
                "ReferenceDataDock"
            }),
        }
    }

    /// The tabs of the lower tab group, in order: what a tab strip over it shows.
    pub fn lower_left_tabs(&self) -> Vec<&'static str> {
        let mut tabs = Vec::new();
        if self.reference {
            tabs.push("ReferenceDataDock");
        }
        if self.surfaces {
            tabs.push("SurfacesDock");
        }
        tabs
    }
}

/// The toolbars' settings: which are hidden, the icons' size, and whether a toolbar along the
/// top shows its name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BarSettings {
    pub hidden: BTreeSet<String>,
    pub icon_size: i32,
    pub names_shown: bool,
    /// The rows a restored layout put the top toolbars on, by name; None: as built. A toolbar the
    /// layout does not place stays where the window built it.
    pub rows: Option<BTreeMap<String, usize>>,
    /// Each toolbar name's width as the window last drew it, by its title, in the text size of
    /// that frame (`measure_names`): what its label takes on its row.
    pub name_widths: BTreeMap<String, i32>,
}

impl Default for BarSettings {
    fn default() -> Self {
        Self {
            hidden: BTreeSet::new(),
            icon_size: 20,
            names_shown: true,
            rows: None,
            name_widths: BTreeMap::new(),
        }
    }
}

impl BarSettings {
    pub fn shown(&self, name: &str) -> bool {
        !self.hidden.contains(name)
    }

    pub fn toggle(&mut self, name: &str) {
        if !self.hidden.remove(name) {
            self.hidden.insert(String::from(name));
        }
    }
}

/// Where everything goes in a window of `screen`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geometry {
    pub menu_bar: Rect,
    /// The three rows of toolbars along the top.
    pub rows: [Rect; 3],
    pub left_bars: Rect,
    pub right_bars: Rect,
    pub bottom_bar: Rect,
    pub status_bar: Rect,
    pub layers: Option<Rect>,
    pub lower_left: Option<Rect>,
    pub properties: Option<Rect>,
    pub command_line: Option<Rect>,
    pub views: Rect,
}

/// The window's parts in `screen`, the panels `docks` shows.
pub fn geometry(screen: Rect, docks: &Docks, right_extra: bool) -> Geometry {
    let left = screen.min.x;
    let top = screen.min.y;
    let right = screen.max.x;
    let bottom = screen.max.y;
    let menu_bar = Rect::from_min_max(Pos2::new(left, top), Pos2::new(right, top + MENU_BAR));
    let row = |at: usize| {
        let y = top + MENU_BAR + TOOL_BAR_ROW * at as f32;
        Rect::from_min_max(Pos2::new(left, y), Pos2::new(right, y + TOOL_BAR_ROW))
    };
    let middle_top = top + TOP;
    let middle_bottom = (bottom - BOTTOM).max(middle_top);
    let left_bars = Rect::from_min_max(
        Pos2::new(left, middle_top),
        Pos2::new(left + SIDE, middle_bottom),
    );
    let right_bars = Rect::from_min_max(
        Pos2::new(right - SIDE, middle_top),
        Pos2::new(right, middle_bottom),
    );
    let bottom_bar = Rect::from_min_max(
        Pos2::new(left, middle_bottom),
        Pos2::new(right, middle_bottom + DRAFTING_BAR),
    );
    let status_bar = Rect::from_min_max(Pos2::new(left, bottom - STATUS_BAR), Pos2::new(right, bottom));
    let mut inner_left = left + SIDE;
    let mut inner_right = right - SIDE;
    let height = middle_bottom - middle_top;
    // The left column: Layers over the lower tab group, or one of them alone.
    let lower = docks.lower_left().is_some();
    let (layers, lower_left) = if docks.layers || lower {
        let column = Rect::from_min_max(
            Pos2::new(inner_left, middle_top),
            Pos2::new(inner_left + PANEL_WIDTH, middle_bottom),
        );
        inner_left += PANEL_WIDTH + SEPARATOR;
        match (docks.layers, lower) {
            (true, true) => {
                let layers_height = ((height - SEPARATOR) * LAYERS_SHARE).round();
                let split = column.min.y + layers_height;
                (
                    Some(Rect::from_min_max(column.min, Pos2::new(column.max.x, split))),
                    Some(Rect::from_min_max(
                        Pos2::new(column.min.x, split + SEPARATOR),
                        column.max,
                    )),
                )
            }
            (true, false) => (Some(column), None),
            _ => (None, Some(column)),
        }
    } else {
        (None, None)
    };
    let properties = if docks.properties || right_extra {
        let column = Rect::from_min_max(
            Pos2::new(inner_right - PANEL_WIDTH, middle_top),
            Pos2::new(inner_right, middle_bottom),
        );
        inner_right -= PANEL_WIDTH + SEPARATOR;
        Some(column)
    } else {
        None
    };
    let mut views_bottom = middle_bottom;
    let command_line = if docks.command_line {
        let rect = Rect::from_min_max(
            Pos2::new(inner_left, middle_bottom - COMMAND_LINE_HEIGHT),
            Pos2::new(inner_right, middle_bottom),
        );
        views_bottom = rect.min.y - SEPARATOR;
        Some(rect)
    } else {
        None
    };
    Geometry {
        menu_bar,
        rows: [row(0), row(1), row(2)],
        left_bars,
        right_bars,
        bottom_bar,
        status_bar,
        layers,
        lower_left,
        properties,
        command_line,
        views: Rect::from_min_max(
            Pos2::new(inner_left, middle_top),
            Pos2::new(inner_right, views_bottom),
        ),
    }
}

// ---- toolbars along a row ----------------------------------------------------------------------

/// The width a toolbar needs to show every action (QToolBar::sizeHint): its padding, its items and
/// the spaces between them, at `icon_size`.
pub fn natural_width(items: &[StripItem], icon_size: i32) -> i32 {
    use crate::tools::flyout_button::{TOOL_BAR_PADDING_ALONG, TOOL_BAR_SPACING};
    if items.is_empty() {
        return 2 * TOOL_BAR_PADDING_ALONG;
    }
    let sum: i32 = items.iter().map(|item| item.size(icon_size).0).sum();
    sum + TOOL_BAR_SPACING * (items.len() as i32 - 1) + 2 * TOOL_BAR_PADDING_ALONG
}

/// The room each toolbar of a row is given, in order: its natural width while the row has it, the
/// rest of the row to the one that does not fit, and nothing to those after it - Qt's toolbar
/// layout, which shrinks the last bars of a full row behind their extension buttons.
pub fn row_rooms(naturals: &[i32], row: f32) -> Vec<f32> {
    let mut left = row;
    naturals
        .iter()
        .map(|&natural| {
            let room = (natural as f32).min(left).max(0.0);
            left -= room;
            room
        })
        .collect()
}

/// The height of a toolbar across it at `icon_size`.
pub fn bar_breadth(icon_size: i32) -> f32 {
    (ToolButtonLook::TOOL.button_size(icon_size).1 + 2 * TOOL_BAR_PADDING_ACROSS) as f32
}

// ---- the window between sessions -----------------------------------------------------------------

/// The layout in the Rust's own form (see the module comment): which panels and toolbars are
/// shown, and the toolbars' rows by name, with its version.
fn layout_json(window: &MainWindow, version: i64, rows: &BTreeMap<String, usize>) -> String {
    let docks = &window.ui.docks;
    let hidden: Vec<String> = window
        .ui
        .bars
        .hidden
        .iter()
        .map(|name| format!("\"{name}\""))
        .collect();
    // Each toolbar's row as "name=row": the object names hold no '='.
    let placed: Vec<String> = rows
        .iter()
        .map(|(name, row)| format!("\"{name}={row}\""))
        .collect();
    format!(
        "{{\"version\":{version},\"layers\":{},\"properties\":{},\"commandLine\":{},\
         \"reference\":{},\"surfaces\":{},\"hidden\":[{}],\"rows\":[{}]}}",
        docks.layers,
        docks.properties,
        docks.command_line,
        docks.reference,
        docks.surfaces,
        hidden.join(","),
        placed.join(",")
    )
}

/// The toolbars' rows this window has, by name.
pub fn current_rows(window: &MainWindow) -> BTreeMap<String, usize> {
    window
        .composed()
        .bars
        .iter()
        .filter_map(|(bar, place)| match place {
            BarPlace::Top(row) => Some((bar.name.clone(), *row)),
            _ => None,
        })
        .collect()
}

/// What a saved layout said, read: None for a layout of another version, or none.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SavedLayout {
    pub version: i64,
    pub docks: Docks,
    pub hidden: BTreeSet<String>,
    pub rows: BTreeMap<String, usize>,
}

/// Reads a layout `layout_json` wrote.
pub fn read_layout(text: &str) -> Option<SavedLayout> {
    use katana_core::json::Json;
    let value = Json::parse(text.as_bytes()).ok()?;
    value.as_object()?;
    // A positive number is read as unsigned, as nlohmann reads it: as_integer alone refused every
    // layout this window wrote (`"version":2`), the older build's carried over among them.
    let version = value.find("version").and_then(|number| {
        number
            .as_integer()
            .or_else(|| number.as_unsigned().and_then(|unsigned| i64::try_from(unsigned).ok()))
    })?;
    let flag = |key: &str, default: bool| {
        value
            .find(key)
            .and_then(Json::as_bool)
            .unwrap_or(default)
    };
    let strings = |key: &str| -> Vec<String> {
        value
            .find(key)
            .and_then(Json::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    };
    let docks = Docks {
        layers: flag("layers", true),
        properties: flag("properties", true),
        command_line: flag("commandLine", true),
        reference: flag("reference", true),
        surfaces: flag("surfaces", false),
        surfaces_in_front: false,
    };
    let hidden: BTreeSet<String> = strings("hidden").into_iter().collect();
    let rows: BTreeMap<String, usize> = strings("rows")
        .into_iter()
        .filter_map(|placed| {
            let (name, row) = placed.split_once('=')?;
            Some((String::from(name), row.parse().ok()?))
        })
        .collect();
    Some(SavedLayout {
        version,
        docks,
        hidden,
        rows,
    })
}

/// MainWindow::restoreLayout: the panels and toolbars where `text` had them, when this window can
/// read it - a layout saved by this version, or by the one before the third toolbar row, which is
/// carried over (Surface Edit and BIM seated on the third row). False, the window as it was, for
/// any other.
pub fn restore_layout(window: &mut MainWindow, text: &str) -> bool {
    let Some(saved) = read_layout(text) else {
        return false;
    };
    if saved.version != LAYOUT_VERSION && saved.version != LAYOUT_VERSION_BEFORE_THIRD_ROW {
        return false;
    }
    window.ui.docks = saved.docks;
    window.ui.bars.hidden = saved.hidden;
    // Version 1's Surface Edit and BIM were not on a row of their own: carried over, they are
    // seated on the third row, as the window builds them. A layout of this version keeps its rows.
    window.ui.bars.rows = if saved.version == LAYOUT_VERSION {
        Some(saved.rows)
    } else {
        None
    };
    true
}

/// MainWindow::restoreSession: the appearance first, then the layout and the minimised panels.
pub fn restore_session(window: &mut MainWindow) {
    if let Ok(settings) = UserSettings::open(UserSettings::default_path()) {
        window.exec.settings = settings;
    }
    let settings = &window.exec.settings;
    if let Some(size) = settings
        .text(TEXT_SIZE_KEY)
        .and_then(crate::theme::text_size_from)
    {
        window.ui.text_size_asked = Some(size);
    }
    if let Some(pixels) = settings.number(TOOL_BAR_ICONS_KEY) {
        window.ui.bars.icon_size = (pixels as i32).clamp(12, 48);
    }
    if let Some(names) = settings.flag(TOOL_BAR_NAMES_KEY) {
        window.ui.bars.names_shown = names;
    }
    let layout = settings.text(LAYOUT_KEY).map(String::from);
    if let Some(layout) = layout {
        let _ = restore_layout(window, &layout);
    }
}

/// MainWindow::saveSession: the layout and the minimised panels, in the person's settings file.
pub fn save_session(window: &mut MainWindow) {
    let rows = current_rows(window);
    let layout = layout_json(window, LAYOUT_VERSION, &rows);
    let settings = &mut window.exec.settings;
    settings.set_text(LAYOUT_KEY, &layout);
    settings.set(MINIMISED_KEY, SettingValue::List(Vec::new()));
    let _ = settings.save();
}

// ---- the checks -------------------------------------------------------------------------------

/// The letter after a single '&', upper case; empty for a text with none.
pub fn mnemonic(text: &str) -> String {
    let characters: Vec<char> = text.chars().collect();
    let mut at = 0;
    while at + 1 < characters.len() {
        if characters[at] == '&' {
            if characters[at + 1] != '&' {
                return characters[at + 1].to_uppercase().collect();
            }
            at += 2;
            continue;
        }
        at += 1;
    }
    String::new()
}

/// MainWindow::shortcutClashes: every key sequence two things share, one line each, and how many
/// distinct sequences there are. The actions' sequences, the Alt letter of each top-level menu,
/// and each item's letter within its menu: two items of one menu with one letter make the key
/// cycle between them.
pub fn shortcut_clashes(composed: &Composed) -> (Vec<String>, usize) {
    let mut owners: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let name_of = |action: &Action| {
        if action.name.is_empty() {
            action.text.replace('&', "")
        } else {
            action.name.clone()
        }
    };
    for action in composed.actions().iter().chain(&composed.panel_actions) {
        if let Some(shortcut) = &action.shortcut {
            owners
                .entry(shortcut_text(shortcut))
                .or_default()
                .push(name_of(action));
        }
    }
    fn letters(menu: &Menu, path: &str, owners: &mut BTreeMap<String, Vec<String>>) {
        for item in &menu.items {
            let (text, sub) = match item {
                MenuItem::Action(action) => (action.text.clone(), None),
                MenuItem::Menu(submenu) => (submenu.title.clone(), Some(submenu)),
                MenuItem::Section(_) | MenuItem::Separator => continue,
            };
            let letter = mnemonic(&text);
            if !letter.is_empty() {
                owners
                    .entry(format!("{path} > {letter}"))
                    .or_default()
                    .push(text.replace('&', ""));
            }
            if let Some(sub) = sub {
                letters(sub, &format!("{path} > {}", text.replace('&', "")), owners);
            }
        }
    }
    for menu in &composed.menus {
        let title = menu.title.replace('&', "");
        let letter = mnemonic(&menu.title);
        if !letter.is_empty() {
            owners
                .entry(format!("Alt+{letter}"))
                .or_default()
                .push(format!("the {title} menu"));
        }
        letters(menu, &format!("Alt+{letter} ({title})"), &mut owners);
    }
    let count = owners.len();
    let clashes = owners
        .into_iter()
        .filter(|(_, names)| names.len() > 1)
        .map(|(key, names)| format!("{key}: {}", names.join(", ")))
        .collect();
    (clashes, count)
}

/// Help > Keyboard Shortcuts' table: every key the actions answer to, with the menu path its
/// command is under, in the menus' order; then any action no menu shows: the panels' first, as
/// the C++'s findChildren<QAction*> met them before the drafting bar's.
pub fn shortcut_rows(composed: &Composed) -> Vec<ShortcutRow> {
    let mut rows = Vec::new();
    let mut in_menus: BTreeSet<String> = BTreeSet::new();
    fn walk(menu: &Menu, path: &str, rows: &mut Vec<ShortcutRow>, in_menus: &mut BTreeSet<String>) {
        for item in &menu.items {
            match item {
                MenuItem::Menu(sub) => walk(
                    sub,
                    &format!("{path} > {}", sub.title.replace('&', "")),
                    rows,
                    in_menus,
                ),
                MenuItem::Action(action) => {
                    in_menus.insert(action.name.clone());
                    if let Some(shortcut) = &action.shortcut {
                        rows.push(ShortcutRow {
                            key: shortcut_text(shortcut),
                            command: action.text.replace('&', "").replace("...", ""),
                            menu: String::from(path),
                            tip: action.tip.clone(),
                        });
                    }
                }
                MenuItem::Section(_) | MenuItem::Separator => {}
            }
        }
    }
    for menu in &composed.menus {
        walk(menu, &menu.title.replace('&', ""), &mut rows, &mut in_menus);
    }
    for action in composed.panel_actions.iter().chain(&composed.actions()) {
        if in_menus.contains(&action.name) {
            continue;
        }
        if let Some(shortcut) = &action.shortcut {
            rows.push(ShortcutRow {
                key: shortcut_text(shortcut),
                command: action.text.replace('&', "").replace("...", ""),
                menu: String::from("(no menu)"),
                tip: action.tip.clone(),
            });
        }
    }
    rows
}

/// MainWindow::menuGaps: every item of the menu bar's menus, their submenus' too, that has no icon
/// or no status tip, one line each; and how many items there are.
pub fn menu_gaps(composed: &Composed) -> (Vec<String>, usize) {
    let mut gaps = Vec::new();
    let mut counted = 0;
    fn walk(
        menu: &Menu,
        path: &str,
        signs: &BTreeMap<String, (&'static str, String)>,
        gaps: &mut Vec<String>,
        counted: &mut usize,
    ) {
        for item in &menu.items {
            let (text, icon, tip, sub) = match item {
                MenuItem::Action(action) => (
                    action.text.clone(),
                    String::from(action.icon),
                    action.tip.clone(),
                    None,
                ),
                MenuItem::Menu(sub) => {
                    let (icon, tip) = signs
                        .get(&sub.name)
                        .map(|(icon, tip)| (String::from(*icon), tip.clone()))
                        .unwrap_or_default();
                    (sub.title.clone(), icon, tip, Some(sub))
                }
                MenuItem::Section(_) | MenuItem::Separator => continue,
            };
            let place = format!("{path} > {}", text.replace('&', ""));
            *counted += 1;
            let mut missing = Vec::new();
            if icon.is_empty() {
                missing.push("no icon");
            }
            if tip.is_empty() {
                missing.push("no status tip");
            }
            if !missing.is_empty() {
                gaps.push(format!("{place}: {}", missing.join(", ")));
            }
            if let Some(sub) = sub {
                walk(sub, &place, signs, gaps, counted);
            }
        }
    }
    for menu in &composed.menus {
        walk(
            menu,
            &menu.title.replace('&', ""),
            &composed.signs,
            &mut gaps,
            &mut counted,
        );
    }
    (gaps, counted)
}

/// tools::menuSignClashes over the window's toolbars: each toolbar's buttons as its strip draws
/// them, Undo's and Redo's split buttons among the Edit bar's; then the views' title bars.
pub fn menu_sign_clashes(window: &mut MainWindow) -> MenuSigns {
    let composed = window.composed();
    let icon_size = window.ui.bars.icon_size;
    let mut holders: Vec<SignHolder> = Vec::new();
    for (bar, place) in &composed.bars {
        if !window.ui.bars.shown(&bar.name) {
            continue;
        }
        let items = window.ui.tools.strip_items(bar, &|_| None);
        let strip = Strip {
            name: bar.name.clone(),
            title: bar.title.clone(),
            vertical: matches!(place, BarPlace::Left | BarPlace::Right),
            icon_size,
            items,
        };
        let mut holder = strip_sign_holder(&strip);
        for item in &bar.items {
            if let ToolBarItem::Widget(name) = item
                && (name == "editUndoButton" || name == "editRedoButton")
            {
                let icon = if name == "editUndoButton" { "Undo" } else { "Redo" };
                holder.buttons.push(MeasuredButton {
                    name: name.clone(),
                    has_menu: true,
                    icon_only: true,
                    paint: split_paint(ToolButtonLook::SPLIT, icon, icon_size, ButtonState::default()),
                });
            }
        }
        holders.push(holder);
    }
    // A view's title bar opens with its kind switcher, a button with a menu at the left of every
    // view: the C++ measured the docks' title bars after the toolbars, each dock in the order it
    // was made. Only a view's bar has a button with a menu; it is the bar's leading button.
    for id in window.exec.workspace.view_ids() {
        let Some(bar) = window.exec.workspace.title_bar(id) else {
            continue;
        };
        let Some(lead) = bar.leading().filter(|lead| lead.menu) else {
            continue;
        };
        let paint = ButtonPaint {
            width: lead.width.round() as i32,
            height: crate::dock_chrome::BUTTON_SIZE.round() as i32,
            look: ToolButtonLook::TITLE_BAR_MENU,
            icon: ButtonIcon::Named(lead.icon.clone()),
            icon_size: crate::dock_chrome::ICON_SIZE.round() as i32,
            state: ButtonState {
                disabled: !lead.enabled,
                ..ButtonState::default()
            },
            sign: MenuSign::Indicator(if lead.enabled {
                crate::theme::palette::text()
            } else {
                crate::theme::palette::text_disabled()
            }),
            ground: crate::canvas::Color::TRANSPARENT,
        };
        holders.push(SignHolder {
            holder: format!(
                "title bar button {}",
                crate::view_workspace::dock_name(id)
            ),
            buttons: vec![MeasuredButton {
                name: lead.name.clone(),
                has_menu: true,
                icon_only: true,
                paint,
            }],
        });
    }
    let ratio = f64::from(crate::drive::headless::scale_factor(
        &katana_core::path_text::environment_variable(Some(
            crate::drive::headless::SCALE_FACTOR_VARIABLE,
        )),
    ));
    let raster = crate::icons::CanvasRasteriser;
    measure_signs(&holders, ratio, &raster)
}

/// MainWindow::toolBarFit at a size: each toolbar along the top that hides actions behind its
/// extension button, one line each, with how many lie along the top and on how many rows, and a
/// line a row of what its toolbars need beside the width they have.
pub fn tool_bar_fit(window: &MainWindow, width: f32) -> (Vec<String>, usize, usize, Vec<String>) {
    let composed = window.composed();
    let icon_size = window.ui.bars.icon_size;
    let mut hiding = Vec::new();
    let mut counted = 0;
    let mut rows: BTreeMap<usize, Vec<(String, i32, Vec<StripItem>)>> = BTreeMap::new();
    for (bar, place) in &composed.bars {
        let BarPlace::Top(row) = place else {
            continue;
        };
        if !window.ui.bars.shown(&bar.name) {
            continue;
        }
        // The strip as the row draws it, its name included (panels' tool_bars_ui), so that its
        // natural width and what its room hides are worked out on the same items. The name was
        // added to the natural width but not to the strip laid out in the room, and a bar short
        // of its name's width was reported with "0 of 9 actions" behind its arrow.
        let named = with_name(bar, window.ui.bars.names_shown);
        let bars = &window.ui.bars;
        let items = window
            .ui
            .tools
            .strip_items(&named, &|name| top_widget_size(bars, name, icon_size));
        let natural = natural_width(&items, icon_size);
        rows.entry(*row)
            .or_default()
            .push((bar.name.clone(), natural, items));
        counted += 1;
    }
    let mut widths = Vec::new();
    for (index, (_, bars)) in rows.iter().enumerate() {
        let naturals: Vec<i32> = bars.iter().map(|(_, natural, _)| *natural).collect();
        let rooms = row_rooms(&naturals, width);
        for ((name, natural, items), room) in bars.iter().zip(&rooms) {
            if (*room as i32) < *natural {
                let (shown, actions) = shown_within(items, *room as i32, icon_size);
                hiding.push(format!(
                    "{name}: {} of {actions} actions are behind its extension button",
                    actions - shown
                ));
            }
        }
        let parts: Vec<String> = bars
            .iter()
            .map(|(name, natural, _)| format!("{name} {natural}"))
            .collect();
        let needed: i32 = naturals.iter().sum();
        widths.push(format!(
            "row {}: {} = {needed} of {} px",
            index + 1,
            parts.join(" + "),
            width as i32
        ));
    }
    (hiding, counted, rows.len(), widths)
}

/// How many of a strip's actions show in `room`, and how many it has: its buttons and the
/// controls it draws itself, not its separators or its name.
fn shown_within(items: &[StripItem], room: i32, icon_size: i32) -> (usize, usize) {
    let extents: Vec<i32> = items.iter().map(|item| item.size(icon_size).0).collect();
    let separators: Vec<bool> = items
        .iter()
        .map(|item| *item == StripItem::Separator)
        .collect();
    let layout = crate::tools::flyout_button::lay_out_strip(&extents, &separators, room);
    let counted = |item: &StripItem| {
        *item != StripItem::Separator && !item.name().starts_with("toolBarName:")
    };
    let shown = items
        .iter()
        .zip(&layout.shown)
        .filter(|(item, shown)| **shown && counted(item))
        .count();
    (shown, items.iter().filter(|item| counted(item)).count())
}

/// The size of a control a toolbar's owner draws itself: the name label, Undo's and Redo's split
/// buttons, the current style's box, the annotation scale's box, the active surface's list.
pub fn widget_size(name: &str, icon_size: i32) -> Option<(i32, i32)> {
    let (_, height) = ToolButtonLook::TOOL.button_size(icon_size);
    match name {
        "editUndoButton" | "editRedoButton" => Some(ToolButtonLook::SPLIT.button_size(icon_size)),
        // The " Style " label and a box sixteen characters wide.
        "CurrentStyleCombo" => Some((44 + 170, height)),
        "annotationScaleCombo" => Some((110, height)),
        "surfaceEditActive" => Some((150, height)),
        _ => None,
    }
}

/// A toolbar's name before its buttons: its text as the window last drew it (`measure_names`), and
/// the label's margins - 2 points before the word, NAME_GAP after it. Until a frame has measured
/// it, the estimate the window began with, 7 points a character and 12 of margins: wider than the
/// secondary text's words are, it cost the third row the 18 points it was short of 1280 by.
pub fn name_width(bars: &BarSettings, title: &str) -> i32 {
    bars.name_widths
        .get(title)
        .copied()
        .unwrap_or_else(|| 7 * title.chars().count() as i32 + 12)
}

/// The space after a toolbar's name before its first button, beside the strip's own spacing.
pub const NAME_GAP: i32 = 6;

/// Measures each title in `titles` in the secondary text, as its label is drawn (panels'
/// strip_ui), into `bars.name_widths`: the label's room on its row follows the text size.
pub fn measure_names<'a>(bars: &mut BarSettings, ui: &egui::Ui, titles: impl Iterator<Item = &'a str>) {
    let font = egui::TextStyle::Small.resolve(ui.style());
    for title in titles {
        let text = ui
            .painter()
            .layout_no_wrap(String::from(title), font.clone(), egui::Color32::PLACEHOLDER)
            .size()
            .x;
        let width = text.ceil() as i32 + 2 + NAME_GAP;
        bars.name_widths.insert(String::from(title), width);
    }
}

/// `bar` as a top row draws it: its name first, as a widget `toolBarName:<title>`, when names are
/// shown (the Properties bar's " Style " label already says what it holds).
pub fn with_name(bar: &ToolBar, names_shown: bool) -> ToolBar {
    let mut bar = bar.clone();
    if names_shown && bar.name != "PropertiesToolBar" {
        bar.items
            .insert(0, ToolBarItem::Widget(format!("toolBarName:{}", bar.title)));
    }
    bar
}

/// The size of a widget of a top row: a toolbar's name, else what `widget_size` says.
pub fn top_widget_size(bars: &BarSettings, name: &str, icon_size: i32) -> Option<(i32, i32)> {
    if let Some(title) = name.strip_prefix("toolBarName:") {
        let (_, height) = ToolButtonLook::TOOL.button_size(icon_size);
        return Some((name_width(bars, title), height));
    }
    widget_size(name, icon_size)
}


/// The rows as built, Surface Edit and BIM moved to the second row before GIS: the layout of the
/// build that put them beside GIS (main.cpp's `crowd`).
fn crowded_rows(window: &MainWindow) -> BTreeMap<String, usize> {
    let mut rows = BTreeMap::new();
    for (bar, place) in &window.composed_as_built().bars {
        if let BarPlace::Top(row) = place {
            rows.insert(bar.name.clone(), *row);
        }
    }
    for name in ["SurfaceEditToolBar", "BIMToolBar"] {
        rows.insert(String::from(name), 1);
    }
    rows
}

/// The window's layout now, saved as version `version` (QMainWindow::saveState).
fn save_state(window: &MainWindow, version: i64) -> String {
    let rows = current_rows(window);
    layout_json(window, version, &rows)
}

/// One line for the window as it is (main.cpp's `report`): whether it held, and how many toolbars
/// lie along the top. `expected`, when not negative, is how many should.
fn report_fit(
    window: &MainWindow,
    width: f32,
    what: &str,
    expected: i64,
    err: &mut dyn Console,
) -> (bool, i64) {
    let (hiding, bars, rows, widths) = tool_bar_fit(window, width);
    for line in &hiding {
        err.err(&format!("toolbar fit clash: {what}: {line}\n"));
    }
    let counted = expected < 0 || bars as i64 == expected;
    if !counted {
        err.err(&format!(
            "toolbar fit clash: {what}: {bars} toolbars along the top, {expected} expected\n"
        ));
    }
    if hiding.is_empty() && counted {
        err.err(&format!(
            "toolbar fit {what}: {bars} toolbars along the top in {rows} rows, none with an \
             action behind its extension button\n"
        ));
    }
    for line in &widths {
        err.err(&format!("  {line}\n"));
    }
    (hiding.is_empty() && counted, bars as i64)
}

/// `state` restored, and the window reported; a layout refused is a failure.
fn restore_and_report(
    window: &mut MainWindow,
    state: &str,
    width: f32,
    what: &str,
    expected: i64,
    err: &mut dyn Console,
) -> bool {
    if !restore_layout(window, state) {
        err.err(&format!(
            "toolbar fit clash: {what}: the saved layout was refused\n"
        ));
        return false;
    }
    report_fit(window, width, what, expected, err).0
}

/// The layout of the build that put Surface Edit and BIM beside GIS, both shown.
fn crowd(window: &mut MainWindow) {
    window.ui.bars.rows = Some(crowded_rows(window));
    window.ui.bars.hidden.remove("SurfaceEditToolBar");
    window.ui.bars.hidden.remove("BIMToolBar");
}

/// main.cpp's checkToolBarFit after its sizes are read: at each size the toolbars along the top
/// show every action; then, at the narrowest, the older builds' layouts restored hold to the same,
/// and a toolbar hidden in a saved layout stays hidden. Every size and every restore is reported
/// on `err`. Whether all held.
pub fn check_tool_bar_fit(
    window: &mut MainWindow,
    sizes: &[(u32, u32)],
    err: &mut dyn Console,
) -> bool {
    let mut ok = true;
    let Some(&first) = sizes.first() else {
        err.err("--check-toolbar-fit needs sizes like 1366x768, a comma between\n");
        return false;
    };
    let mut as_built = 0;
    let mut narrow = first;
    for &(width, height) in sizes {
        let (held, bars) = report_fit(window, width as f32, &format!("{width}x{height}"), -1, err);
        ok = ok && held;
        if (width, height) == first {
            as_built = bars;
        }
        if width < narrow.0 {
            narrow = (width, height);
        }
    }
    let at = format!("{}x{}", narrow.0, narrow.1);
    let width = narrow.0 as f32;
    crowd(window);
    if tool_bar_fit(window, width).0.is_empty() {
        // The check must be able to fail: the layout it restores really is crowded.
        err.err(&format!(
            "toolbar fit clash: the second-row layout of the older build fitted at {at}, so \
             restoring it proves nothing\n"
        ));
        ok = false;
    }
    let state = save_state(window, LAYOUT_VERSION_BEFORE_THIRD_ROW);
    ok &= restore_and_report(
        window,
        &state,
        width,
        &format!("{at} after restoring a layout with Surface Edit and BIM on the second row"),
        as_built,
        err,
    );
    // A layout saved before Surface Edit and BIM existed: neither in any row.
    let mut rows = current_rows(window);
    rows.remove("SurfaceEditToolBar");
    rows.remove("BIMToolBar");
    window.ui.bars.rows = Some(rows);
    let state = save_state(window, LAYOUT_VERSION_BEFORE_THIRD_ROW);
    ok &= restore_and_report(
        window,
        &state,
        width,
        &format!("{at} after restoring a layout saved before Surface Edit and BIM existed"),
        as_built,
        err,
    );
    // A toolbar hidden in a layout of this version stays hidden.
    window.ui.bars.hidden.insert(String::from("BIMToolBar"));
    let bim_hidden = save_state(window, LAYOUT_VERSION);
    window.ui.bars.hidden.remove("BIMToolBar");
    ok &= restore_and_report(
        window,
        &bim_hidden,
        width,
        &format!("{at} after restoring a layout of this version with BIM hidden"),
        as_built - 1,
        err,
    );
    if window.ui.bars.shown("BIMToolBar") {
        err.err("toolbar fit clash: BIM was hidden in the saved layout and came back shown\n");
        ok = false;
    }
    // And in one of the older version, carried over: Surface Edit hidden there stays hidden, BIM
    // is seated.
    window.ui.bars.hidden.remove("BIMToolBar");
    crowd(window);
    window
        .ui
        .bars
        .hidden
        .insert(String::from("SurfaceEditToolBar"));
    let state = save_state(window, LAYOUT_VERSION_BEFORE_THIRD_ROW);
    ok &= restore_and_report(
        window,
        &state,
        width,
        &format!("{at} after restoring an older layout with Surface Edit hidden"),
        as_built - 1,
        err,
    );
    if window.ui.bars.shown("SurfaceEditToolBar") {
        err.err(
            "toolbar fit clash: Surface Edit was hidden in the saved layout and came back shown\n",
        );
        ok = false;
    }
    ok
}

/// main.cpp's checkSessionRestore: what an older build saved - Surface Edit and BIM beside GIS on
/// the second row, a layout of version 1, in a window left maximised - restored into this window
/// before it is first shown. Whether every toolbar was along the top afterwards, Surface Edit and
/// BIM on a row of their own below GIS's.
pub fn check_session_restore(window: &mut MainWindow, err: &mut dyn Console) -> bool {
    if window.ui.visible {
        err.err("session restore: the window was shown before the check\n");
        return false;
    }
    // Every toolbar the window has, and the minimised panels' tray.
    let toolbars = window.composed().bars.len() + 1;
    crowd(window);
    let older = save_state(window, LAYOUT_VERSION_BEFORE_THIRD_ROW);
    if !restore_layout(window, &older) {
        err.err("session restore: the older build's layout was refused\n");
        return false;
    }
    let composed = window.composed();
    let lost: Vec<String> = composed
        .bars
        .iter()
        .filter(|(_, place)| *place == BarPlace::Nowhere)
        .map(|(bar, _)| bar.name.clone())
        .collect();
    if !lost.is_empty() {
        err.err(&format!("session restore: in no area: {}\n", lost.join(", ")));
        return false;
    }
    let row_of = |name: &str| {
        composed.bars.iter().find_map(|(bar, place)| match place {
            BarPlace::Top(row) if bar.name == name => Some(*row),
            _ => None,
        })
    };
    let gis = row_of("GISToolBar").unwrap_or(0);
    let seated = [row_of("SurfaceEditToolBar"), row_of("BIMToolBar")]
        .iter()
        .all(|row| row.is_some_and(|row| row > gis))
        && window.ui.bars.shown("SurfaceEditToolBar")
        && window.ui.bars.shown("BIMToolBar");
    if !seated {
        err.err(
            "session restore: Surface Edit and BIM are not on a row of their own below GIS\n",
        );
        return false;
    }
    err.err(&format!(
        "session restore: an older build's layout restored into a maximised window before it was \
         shown; {toolbars} toolbars, none lost\n"
    ));
    true
}
