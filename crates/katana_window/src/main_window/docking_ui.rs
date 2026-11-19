//! The window's side of docking (main_window::docking): the actions that move a toolbar or a
//! torn-off menu, each through one door (`MainWindow::docking_action`), what a headless run
//! reports of where they are, and the drawing of what does not sit in a row - a floating toolbar,
//! a torn-off menu's panel, a drag's drop target - and of a docked bar's grip.
//!
//! THE GRIP. Every docked toolbar has six dots in the padding before its first item - its grip -
//! and is dragged by it, and by its name when it shows one: the grip costs a bar no more than the
//! 2 points before its name that keep the dots clear of it (layout::NAME_LEAD), so the rows still
//! fit a 1280 laptop. The dots are faint at rest and lit under the pointer (`paint_grip`). The
//! handle is a named control (`<Bar>Grip`), and its right-click menu holds the bar's View >
//! Toolbars > Place items. A drag shows where the bar would go as an accent outline over a faint
//! accent fill, the only accent in the window that is not a thing chosen or active, since it is
//! the place being chosen; it is moved when it is let go. Rejected: moving the bar with the
//! pointer while it is docked, which reflows the row under the pointer at every frame and leaves
//! the person chasing the place they aimed at.
//!
//! A FLOATING TOOLBAR is an egui Area over the views: a title strip (its grip and its name) over
//! its buttons, wrapped in rows of its width (`flyout_button::tool_bar_wrapped`) and drawn at the
//! least width for those rows (`balanced_wrap_width`), and a right edge that sets the width. A
//! double click on its title docks it where it was last docked. Rejected: an egui Window, whose
//! title bar, frame and resize grip are egui's and would not match the window's chrome; and a
//! floating bar sized by its height as well, which is two ways to choose one shape.
//!
//! A TORN-OFF MENU is a panel of the window's own chrome (the panels' `DockChrome`, its title bar
//! with its menu's icon, Minimise, Float and Close), named `<menu>TornOff`, holding the menu's
//! items drawn by the one menu drawer (drive::action::menu_items_ui): its icons, ticks and
//! shortcuts, each item a click that runs its action as the menu's does, and kept open. Docked,
//! it is in a band between the views and a side - down a side in one column that scrolls, along
//! the top or the bottom flowed into columns side by side (`flowed_items_ui`); floating, an Area
//! of its own sized by any edge or corner (`RESIZE_HANDLES`). Its title bar is dragged to a side
//! of the views to dock it there, or into the views to float it; its Minimise hides it to the
//! status bar's tray (`MainWindow::tray_items`), whose button puts it back where it was.

use std::collections::BTreeMap;

use egui::{Align, CursorIcon, Layout, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, UiBuilder, Vec2};
use katana_core::error::make_error;
use katana_core::{ErrorCode, Status};

use super::actions::{BarPlace, Composed, reset_tool_bars_action, tool_bar_place_menu};
use super::docking::{
    DRAG_THRESHOLD, DockingAction, DropSpot, DropZones, FLOAT_HEADER, FLOAT_WIDTH, FloatAt,
    FlowItem, LEAST_PANEL, NEW_LINE_ZONE, PANEL_SIZE, PanelDrag, PanelPlace, Places, Side,
    apply_drop, drop_spot, flow_columns, last_line, line_members, panel_dock_name,
    panel_drop_side, panel_menu, panel_place_text, spot_at, spot_outline,
};
use super::layout::{self, BarLines, Geometry, natural_width};
use super::window::MainWindow;
use crate::dock_chrome::{ChromeRequest, DockRole};
use crate::drive::action::{Menu, MenuItem, ToolBar};
use crate::drive::registry::{ControlKind, ControlRecord, record};
use crate::theme::palette;
use crate::tools::flyout_button::{
    StripItem, TOOL_BAR_PADDING_ACROSS, TOOL_BAR_PADDING_ALONG, balanced_wrap_width, wrap_strip,
};

/// How near two bars' breadths are for them to share a column (`MainWindow::line_for`): the
/// breadths are whole points, so equal within rounding.
const BREADTH_TOLERANCE: f32 = 1.0;

/// How far in from a floating torn-off menu's edge a press sizes it: Windows' own sizing border
/// is 8 pixels at 100% (GetSystemMetrics SM_CXSIZEFRAME plus SM_CXPADDEDBORDER), and 6 points
/// keeps clear of an item's icon, 15 points in.
const RESIZE_EDGE: f32 = 6.0;

/// One edge or corner of a floating torn-off menu that sizes it, and which of the rectangle's
/// sides it moves: `moves` is (left, top, right, bottom).
struct ResizeHandle {
    part: &'static str,
    cursor: CursorIcon,
    moves: (bool, bool, bool, bool),
}

/// The edges first and the corners after, so a corner, drawn later, is over the two edges it
/// joins.
const RESIZE_HANDLES: [ResizeHandle; 8] = [
    ResizeHandle { part: "Left", cursor: CursorIcon::ResizeWest, moves: (true, false, false, false) },
    ResizeHandle { part: "Top", cursor: CursorIcon::ResizeNorth, moves: (false, true, false, false) },
    ResizeHandle { part: "Right", cursor: CursorIcon::ResizeEast, moves: (false, false, true, false) },
    ResizeHandle { part: "Bottom", cursor: CursorIcon::ResizeSouth, moves: (false, false, false, true) },
    ResizeHandle { part: "TopLeft", cursor: CursorIcon::ResizeNorthWest, moves: (true, true, false, false) },
    ResizeHandle { part: "TopRight", cursor: CursorIcon::ResizeNorthEast, moves: (false, true, true, false) },
    ResizeHandle { part: "BottomLeft", cursor: CursorIcon::ResizeSouthWest, moves: (true, false, false, true) },
    ResizeHandle { part: "BottomRight", cursor: CursorIcon::ResizeSouthEast, moves: (false, false, true, true) },
];

impl ResizeHandle {
    /// Where on `panel` it is pressed: a strip RESIZE_EDGE in along each side it moves, a corner
    /// twice that square.
    fn area(&self, panel: Rect) -> Rect {
        let (left, top, right, bottom) = self.moves;
        let reach = if (left || right) && (top || bottom) {
            RESIZE_EDGE * 2.0
        } else {
            RESIZE_EDGE
        };
        let x = if left {
            panel.min.x..=panel.min.x + reach
        } else if right {
            panel.max.x - reach..=panel.max.x
        } else {
            panel.min.x..=panel.max.x
        };
        let y = if top {
            panel.min.y..=panel.min.y + reach
        } else if bottom {
            panel.max.y - reach..=panel.max.y
        } else {
            panel.min.y..=panel.max.y
        };
        Rect::from_x_y_ranges(x, y)
    }

    /// `panel` dragged by it `delta`: the sides it moves follow the pointer, the others stay, and
    /// it is never less than LEAST_PANEL, the side being moved stopping there.
    fn resized(&self, panel: Rect, delta: Vec2) -> Rect {
        let (left, top, right, bottom) = self.moves;
        let mut sized = panel;
        if left {
            sized.min.x = (panel.min.x + delta.x).min(panel.max.x - LEAST_PANEL.x);
        }
        if right {
            sized.max.x = (panel.max.x + delta.x).max(panel.min.x + LEAST_PANEL.x);
        }
        if top {
            sized.min.y = (panel.min.y + delta.y).min(panel.max.y - LEAST_PANEL.y);
        }
        if bottom {
            sized.max.y = (panel.max.y + delta.y).max(panel.min.y + LEAST_PANEL.y);
        }
        sized
    }
}

/// The opacity of the accent filling a drop target under its outline (`drags_ui`): enough to
/// read as a place over a plan's lines, faint enough that what is under it still shows.
pub const DROP_FILL: f32 = 0.18;

/// The toolbars a frame shows, by where they are.
pub struct Arranged<'a> {
    /// Each side's lines by `Side::index`, from the window's edge inwards: the line's number and
    /// its bars in order.
    pub lines: [Vec<(usize, Vec<&'a ToolBar>)>; 4],
    pub floating: Vec<(&'a ToolBar, FloatAt)>,
    pub drafting: Vec<&'a ToolBar>,
}

/// The shown toolbars of `bars` by where they are.
pub fn arrange<'a>(bars: &'a [(ToolBar, BarPlace)], shown: &dyn Fn(&str) -> bool) -> Arranged<'a> {
    let mut lines: [BTreeMap<usize, Vec<&ToolBar>>; 4] = Default::default();
    let mut floating = Vec::new();
    let mut drafting = Vec::new();
    for (bar, place) in bars {
        if !shown(&bar.name) {
            continue;
        }
        match place {
            BarPlace::Floating(at) => floating.push((bar, *at)),
            BarPlace::Drafting => drafting.push(bar),
            BarPlace::Nowhere => {}
            docked => {
                if let Some((side, line)) = docked.docked() {
                    lines[side.index()].entry(line).or_default().push(bar);
                }
            }
        }
    }
    Arranged {
        lines: lines.map(|side| side.into_iter().collect()),
        floating,
        drafting,
    }
}

/// The grip's dots: their radius, and the step between them.
pub const GRIP_DOT: f32 = 0.8;
pub const GRIP_STEP: f32 = 2.5;
/// How far the grip's first column of dots is from the bar's start: the dots' ink starts some
/// two points in, clear of the window's edge for the first bar of a row.
pub const GRIP_INSET: f32 = 2.5;
/// The grip's opacity at rest: present, so a person sees the bars can be moved and where each
/// begins, but below everything the bar holds; at full strength under the pointer and while
/// dragged.
pub const GRIP_AT_REST: f32 = 0.55;

/// The six dots of a docked bar's grip, for a bar starting at `start` (its corner) and `breadth`
/// across: two columns of three along a row's bar, three columns of two across a column's, in the
/// bar's padding before its first item and centred across the bar on its buttons' middle line.
///
/// They were centred in the padding's 6 points and on the line's rectangle: their ink began a
/// point from the window's edge and ended some 3 points before a bar's name, so "File" read
/// "#File" (the sixth look audit), and with large icons, whose buttons are broader than the line,
/// the dots sat above the buttons' middle like a superscript.
pub fn grip_dots(start: Pos2, breadth: f32, vertical: bool) -> Vec<Pos2> {
    let mut dots = Vec::with_capacity(6);
    for along in 0..2 {
        for across in 0..3 {
            let along = GRIP_INSET + along as f32 * GRIP_STEP;
            let across = breadth / 2.0 + (across as f32 - 1.0) * GRIP_STEP;
            dots.push(if vertical {
                Pos2::new(start.x + across, start.y + along)
            } else {
                Pos2::new(start.x + along, start.y + across)
            });
        }
    }
    dots
}

/// The grip's dots painted, in the muted token: at GRIP_AT_REST, or in the text's colour when
/// `lit` (the pointer on its handle, or the bar dragged).
///
/// Chosen: dots always shown, faint at rest. Rejected: dots shown only under the pointer, as some
/// editors draw them: the grip is how a person learns the bars move, and since a row has no rule
/// between its bars, it is also what says where each bar begins.
pub fn paint_grip(painter: &egui::Painter, dots: &[Pos2], lit: bool) {
    let colour = if lit {
        palette::text().to_egui()
    } else {
        palette::text_muted().to_egui().gamma_multiply(GRIP_AT_REST)
    };
    for dot in dots {
        painter.circle_filled(*dot, GRIP_DOT, colour);
    }
}

/// What a floating toolbar's frame asked this frame.
enum FloatEvent {
    Width(String, f32),
    DockBack(String),
}

impl MainWindow {
    // ---- the actions ----------------------------------------------------------------------------

    /// The one door every docking action goes through (main_window::docking): a menu item, a
    /// toolbar's right-click menu, a drop, a `*` step.
    pub fn docking_action(&mut self, action: DockingAction) -> Status {
        match action {
            DockingAction::Dock { key, side } => {
                let name = self.bar_named(&key)?;
                self.dock_bar(&name, side)
            }
            DockingAction::Place {
                bar,
                side,
                line,
                index,
                float,
            } => {
                let name = self.bar_named(&bar)?;
                let places = self.bar_places();
                let spot = match (side, float) {
                    (Some(side), _) => spot_at(&places, &name, side, line, index),
                    (None, Some(at)) => DropSpot::Float(at),
                    (None, None) => DropSpot::Float(self.float_spot(&name)),
                };
                self.drop_bar(&name, &spot)
            }
            DockingAction::Reset => {
                self.reset_bars();
                Ok(())
            }
            DockingAction::TearOff { menu } => self.tear_off_menu(&menu, None),
            DockingAction::PanelDock { menu, side } => {
                let place = match side {
                    Some(side) => PanelPlace::Docked(side),
                    None => PanelPlace::Floating(self.panel_float_rect(None)),
                };
                self.place_menu_panel(&menu, place)
            }
            DockingAction::PanelPlace { menu, place } => self.place_menu_panel(&menu, place),
            DockingAction::PanelClose { menu } => {
                if self.ui.docking.panels.close(&menu) {
                    Ok(())
                } else {
                    Err(not_torn_off(&menu))
                }
            }
            DockingAction::Band { side, breadth } => {
                self.ui.docking.panels.set_band(side, breadth);
                Ok(())
            }
        }
    }

    /// The object name of the toolbar `key` names: its object name, or its title without spaces
    /// (its toggle's and its dock actions' key). The Drafting bar is not one: it stays where it is.
    pub fn bar_named(&self, key: &str) -> katana_core::Result<String> {
        self.bar_places()
            .into_iter()
            .map(|(name, _)| name)
            .find(|name| {
                name == key
                    || self
                        .composed_bar_title(name)
                        .is_some_and(|title| title.replace(' ', "") == key)
            })
            .ok_or_else(|| make_error(ErrorCode::NotFound, "no toolbar is named", key))
    }

    fn composed_bar_title(&self, name: &str) -> Option<String> {
        self.composed()
            .bars
            .into_iter()
            .find(|(bar, _)| bar.name == name)
            .map(|(bar, _)| bar.title)
    }

    /// Every toolbar's place now, in order along its line, the Drafting bar left out.
    pub fn bar_places(&self) -> Places {
        layout::current_places(self)
    }

    /// Toolbar `name`'s place now.
    pub fn bar_place(&self, name: &str) -> Option<BarPlace> {
        self.bar_places()
            .into_iter()
            .find(|(bar, _)| bar == name)
            .map(|(_, place)| place)
    }

    /// Toolbar `name` moved to `spot`, and shown: the one move every way of moving a bar ends in.
    pub fn drop_bar(&mut self, name: &str, spot: &DropSpot) -> Status {
        let mut places = self.bar_places();
        let before = places
            .iter()
            .find(|(bar, _)| bar == name)
            .map(|(_, place)| *place);
        if !apply_drop(&mut places, name, spot) {
            return Err(make_error(ErrorCode::NotFound, "no toolbar is named", name));
        }
        if let Some(before) = before.filter(|place| place.docked().is_some()) {
            let kept = &mut self.ui.docking.docked_before;
            kept.retain(|(bar, _)| bar != name);
            kept.push((String::from(name), before));
        }
        self.ui.bars.places = Some(places);
        self.ui.bars.hidden.remove(name);
        Ok(())
    }

    /// Toolbar `name` docked to `side` (None: floated), as View > Toolbars > Place does: on the
    /// first line of that side it fits in whole - along the top and the bottom at the window's
    /// width, down the left and the right in the column's height, and down a side only beside
    /// bars of its own breadth - else on a line of its own, the side's next. A bar already on that
    /// side stays where it is on it.
    ///
    /// Every side did what Dock Top did from the sixth look audit on: a side or the bottom put the
    /// bar at the end of its first line whatever that line held, and Draw docked right behind
    /// Modify was one button long, its twenty tools behind an arrow; the Properties bar docked
    /// left made the whole left column as broad as its style box.
    pub fn dock_bar(&mut self, name: &str, side: Option<Side>) -> Status {
        let places = self.bar_places();
        let Some(place) = places
            .iter()
            .find(|(bar, _)| bar == name)
            .map(|(_, place)| *place)
        else {
            return Err(make_error(ErrorCode::NotFound, "no toolbar is named", name));
        };
        let spot = match side {
            None if matches!(place, BarPlace::Floating(_)) => return self.show_bar(name),
            None => DropSpot::Float(self.float_spot(name)),
            Some(side) if place.docked().is_some_and(|(at, _)| at == side) => {
                return self.show_bar(name);
            }
            Some(side) => self.line_for(name, side, &places),
        };
        self.drop_bar(name, &spot)
    }

    fn show_bar(&mut self, name: &str) -> Status {
        self.ui.bars.hidden.remove(name);
        Ok(())
    }

    /// The first line of `side` with room for `name` whole (the lines as the window last drew
    /// them, measured as the toolbar-fit check measures a row), else a new line after the last.
    /// Down the left and the right a line is also only one of bars of `name`'s breadth: a column
    /// is as broad as its broadest bar, and a narrow bar beside a broad one leaves a void.
    fn line_for(&self, name: &str, side: Side, places: &Places) -> DropSpot {
        let vertical = side.vertical();
        let room = self.side_room(side);
        let composed = self.composed();
        let bar_of = |bar: &str| composed.bars.iter().find(|(each, _)| each.name == bar);
        let natural = |bar: &str| -> f32 {
            bar_of(bar).map_or(0.0, |(each, _)| self.docked_natural(each, vertical) as f32)
        };
        let breadth = |bar: &str| -> f32 {
            bar_of(bar).map_or(layout::SIDE, |(each, _)| self.column_breadth(each))
        };
        let needed = natural(name);
        let own = breadth(name);
        let last = last_line(places, side);
        if let Some(last) = last {
            for line in 0..=last {
                let members: Vec<&str> = line_members(places, side, line)
                    .into_iter()
                    .filter(|bar| *bar != name && self.ui.bars.shown(bar))
                    .collect();
                let taken: f32 = members.iter().map(|bar| natural(bar)).sum();
                let alike = !vertical
                    || members
                        .iter()
                        .all(|bar| (breadth(bar) - own).abs() <= BREADTH_TOLERANCE);
                if alike && taken + needed <= room {
                    return DropSpot::Line {
                        side,
                        line,
                        before: None,
                    };
                }
            }
        }
        DropSpot::NewLine {
            side,
            before: last.map_or(0, |last| last + 1),
        }
    }

    /// How long a line of `side` is: the window's width along the top and the bottom, the height
    /// between the top and bottom toolbars down the left and the right - as the last frame laid
    /// the window out, or as it would be laid out now before one has.
    fn side_room(&self, side: Side) -> f32 {
        let screen = self.ui.docking.screen;
        if !side.vertical() {
            return screen.width();
        }
        let composed = self.composed();
        let shown = |name: &str| self.ui.bars.shown(name);
        let lines = self.bar_lines(&arrange(&composed.bars, &shown));
        layout::geometry(
            screen,
            &self.ui.docks,
            false,
            &lines,
            self.ui.docking.panels.band_breadths(),
        )
        .middle
        .height()
    }

    /// How broad `bar` is down a side: its broadest item and the strip's padding across it, and
    /// never less than a column (SIDE).
    pub(crate) fn column_breadth(&self, bar: &ToolBar) -> f32 {
        let docked = layout::docked_bar(bar, self.ui.bars.names_shown, true);
        let strip = self.strip_of(&docked, true, self.ui.bars.icon_size);
        let widest = strip
            .items
            .iter()
            .map(|item| item.size(self.ui.bars.icon_size).0)
            .max()
            .unwrap_or(0)
            + 2 * TOOL_BAR_PADDING_ACROSS;
        (widest as f32).max(layout::SIDE)
    }

    /// The width `bar` needs docked to show every action: as a row draws it when `vertical` is
    /// false, as a column does when true.
    pub(crate) fn docked_natural(&self, bar: &ToolBar, vertical: bool) -> i32 {
        let docked = layout::docked_bar(bar, self.ui.bars.names_shown, vertical);
        let strip = self.strip_of(&docked, vertical, self.ui.bars.icon_size);
        natural_width(&strip.items, self.ui.bars.icon_size)
    }

    /// Where toolbar `name` floats when a menu item or an action floats it: over the views' top
    /// left, a step down and along for each bar floating already, at its natural width up to
    /// FLOAT_WIDTH.
    fn float_spot(&self, name: &str) -> FloatAt {
        let views = self.ui.docking.views;
        let floating = self
            .bar_places()
            .iter()
            .filter(|(bar, place)| bar != name && matches!(place, BarPlace::Floating(_)))
            .count() as f32;
        let width = self
            .composed()
            .bars
            .iter()
            .find(|(bar, _)| bar.name == name)
            .map_or(FLOAT_WIDTH, |(bar, _)| self.float_natural(bar).min(FLOAT_WIDTH));
        FloatAt {
            x: (views.min.x + 24.0 + 24.0 * floating).round(),
            y: (views.min.y + 24.0 + 24.0 * floating).round(),
            width,
        }
    }

    /// The width a floating `bar` shows its buttons in one row at.
    fn float_natural(&self, bar: &ToolBar) -> f32 {
        let strip = self.strip_of(bar, false, self.ui.bars.icon_size);
        natural_width(&strip.items, self.ui.bars.icon_size) as f32
    }

    /// Every toolbar back where a new window has it, and shown (View > Toolbars > Reset Toolbars).
    pub fn reset_bars(&mut self) {
        self.ui.bars.places = None;
        self.ui.bars.hidden.clear();
        self.ui.docking.docked_before.clear();
    }

    /// Menu `menu` (a menu of the menu bar or a submenu, by its object name) torn off into a
    /// floating panel at `at`, or where a panel is first put; a menu torn off already is left
    /// where it is.
    pub fn tear_off_menu(&mut self, menu: &str, at: Option<Pos2>) -> Status {
        let composed = self.composed();
        let Some(found) = crate::drive::steps::find_menu(&composed.menus, menu) else {
            return Err(make_error(ErrorCode::NotFound, "no menu is named", menu));
        };
        let icon = if found.icon.is_empty() { "Panels" } else { found.icon };
        let title = found.title.replace('&', "");
        self.ui
            .panels
            .chrome
            .install(&panel_dock_name(menu), icon, DockRole::Panel)
            .set_title(&title);
        let rect = self.panel_float_rect(at);
        self.ui.docking.panels.tear_off(menu, rect);
        Ok(())
    }

    /// A torn-off menu's floating rectangle: at `at`, else over the views' top left a step along
    /// for each floating already, PANEL_SIZE big and no taller than the views.
    fn panel_float_rect(&self, at: Option<Pos2>) -> Rect {
        let views = self.ui.docking.views;
        let floating = self
            .ui
            .docking
            .panels
            .open
            .iter()
            .filter(|panel| matches!(panel.place, PanelPlace::Floating(_)))
            .count() as f32;
        let corner = at.unwrap_or_else(|| {
            views.min + Vec2::new(48.0 + 24.0 * floating, 48.0 + 24.0 * floating)
        });
        let height = PANEL_SIZE.y.min(views.height().max(LEAST_PANEL.y));
        Rect::from_min_size(corner.round(), Vec2::new(PANEL_SIZE.x, height))
    }

    /// Torn-off menu `menu`'s panel put at `place`.
    pub fn place_menu_panel(&mut self, menu: &str, place: PanelPlace) -> Status {
        if self.ui.docking.panels.place(menu, place) {
            Ok(())
        } else {
            Err(not_torn_off(menu))
        }
    }

    // ---- what a headless run reports ------------------------------------------------------------

    /// `--report` of a toolbar (`place=top:0 at=1 shown=yes drawn=x,y,w,h`), of a torn-off menu's
    /// panel (`place=left items=12 drawn=...`), or of `toolBarLayout` (every toolbar's place in
    /// order, `Name=place` each); None for any other name.
    pub fn docking_report(&self, name: &str) -> Option<String> {
        let places = self.bar_places();
        if name == "toolBarLayout" {
            let mut parts: Vec<String> = places
                .iter()
                .map(|(bar, place)| format!("{bar}={}", place.text()))
                .collect();
            for panel in &self.ui.docking.panels.open {
                parts.push(format!(
                    "{}={}",
                    panel_dock_name(&panel.menu),
                    panel_place_text(&panel.place)
                ));
            }
            return Some(parts.join(" "));
        }
        let drawn = |rect: Option<Rect>| match rect {
            Some(rect) => format!(
                "{},{},{},{}",
                rect.min.x.round() as i32,
                rect.min.y.round() as i32,
                rect.width().round() as i32,
                rect.height().round() as i32
            ),
            None => String::from("none"),
        };
        if let Some((_, place)) = places.iter().find(|(bar, _)| bar == name) {
            let at = match place.docked() {
                Some((side, line)) => line_members(&places, side, line)
                    .iter()
                    .position(|bar| *bar == name)
                    .unwrap_or(0),
                None => 0,
            };
            return Some(format!(
                "place={} at={at} shown={} drawn={}",
                place.text(),
                if self.ui.bars.shown(name) { "yes" } else { "no" },
                drawn(self.ui.docking.drawn_rect(name))
            ));
        }
        let menu = panel_menu(name)?;
        let panel = self.ui.docking.panels.find(menu)?;
        let composed = self.composed();
        let items = crate::drive::steps::find_menu(&composed.menus, menu)
            .map_or(0, |found| found.actions().len());
        Some(format!(
            "place={} items={items} drawn={}",
            panel_place_text(&panel.place),
            drawn(self.ui.docking.drawn_rect(name))
        ))
    }

    // ---- the drawing --------------------------------------------------------------------------

    /// The lines each side's shown toolbars take this frame: a column as broad as its broadest
    /// bar, and never less than SIDE.
    pub(crate) fn bar_lines(&self, arranged: &Arranged<'_>) -> BarLines {
        let breadth = |bars: &Vec<&ToolBar>| -> f32 {
            bars.iter()
                .map(|bar| self.column_breadth(bar))
                .fold(layout::SIDE, f32::max)
        };
        BarLines {
            top: arranged.lines[Side::Top.index()].len(),
            left: arranged.lines[Side::Left.index()]
                .iter()
                .map(|(_, bars)| breadth(bars))
                .collect(),
            right: arranged.lines[Side::Right.index()]
                .iter()
                .map(|(_, bars)| breadth(bars))
                .collect(),
            bottom: arranged.lines[Side::Bottom.index()].len(),
        }
    }

    /// A docked toolbar's grip: its six dots in the padding before its first item, and the handle
    /// it is dragged by - its padding, and its name when it shows one - with its right-click menu.
    /// `handle` is the handle's rectangle, `bar_rect` the bar's.
    pub(crate) fn grip_ui(
        &mut self,
        ui: &mut Ui,
        bar: &ToolBar,
        place: BarPlace,
        bar_rect: Rect,
        handle: Rect,
    ) {
        let vertical = place.vertical();
        let response = ui
            .interact(
                handle,
                egui::Id::new(("katanaToolBarGrip", bar.name.as_str())),
                Sense::click_and_drag(),
            )
            .on_hover_cursor(CursorIcon::Grab)
            .on_hover_text(format!(
                "Drag the {} toolbar to another side, a row of its own or over the views; \
                 right-click for where it goes",
                bar.title
            ));
        // The dots centred on the strip's own breadth, which is the line's only at the standard
        // icon size (layout::bar_breadth).
        let lit = response.hovered()
            || response.dragged()
            || self
                .ui
                .docking
                .bar_drag
                .as_ref()
                .is_some_and(|(name, _)| *name == bar.name);
        let breadth = layout::bar_breadth(self.ui.bars.icon_size);
        paint_grip(ui.painter(), &grip_dots(bar_rect.min, breadth, vertical), lit);
        let mut grip = ControlRecord::new(&format!("{}Grip", bar.name), ControlKind::Other);
        grip.text = bar.title.clone();
        grip.enabled = true;
        grip.visible = ui.is_visible();
        record(ui, &response, grip);
        // Where the press was, not where the pointer is when egui calls it a drag: by then it has
        // moved past the drag threshold, and the bar would float that far from the grip.
        if response.drag_started()
            && let Some(pointer) = ui.input(|input| input.pointer.press_origin())
        {
            self.ui.docking.bar_drag = Some((bar.name.clone(), pointer - bar_rect.min));
        }
        self.bar_context_menu(&response, bar, place);
    }

    /// A toolbar's right-click menu: its Place submenu's items, then its toggle and Reset Toolbars.
    fn bar_context_menu(&mut self, response: &egui::Response, bar: &ToolBar, place: BarPlace) {
        let icon = "Toolbars";
        let place_menu = tool_bar_place_menu(bar, &place, icon);
        let mut menu = Menu::new(&format!("{}Menu", bar.name), &bar.title).section("Place");
        menu.items.extend(place_menu.items);
        menu.items.push(MenuItem::Section(String::from("Toolbars")));
        menu.items.push(MenuItem::Action(super::actions::tool_bar_toggle(
            bar,
            self.ui.bars.shown(&bar.name),
            icon,
        )));
        menu.items.push(MenuItem::Action(reset_tool_bars_action()));
        let mut chosen = None;
        response.context_menu(|ui| {
            crate::drive::action::menu_items_ui(ui, &menu, &mut chosen);
            if chosen.is_some() {
                ui.close();
            }
        });
        if let Some(chosen) = chosen {
            self.ui.triggered.push(chosen);
        }
    }

    /// The drop zones of the lines drawn this frame, and the strips beside each side's bars that
    /// open a new line: under the top rows, over the bottom rows, inside each side's columns.
    pub(crate) fn new_line_zones(&self, geometry: &Geometry, zones: &mut DropZones) {
        let places = self.bar_places();
        let next = |side: Side| last_line(&places, side).map_or(0, |last| last + 1);
        let middle = geometry.middle;
        let zone = NEW_LINE_ZONE;
        zones.new_lines = vec![
            (
                Side::Top,
                next(Side::Top),
                Rect::from_min_max(
                    Pos2::new(geometry.menu_bar.min.x, middle.min.y),
                    Pos2::new(geometry.menu_bar.max.x, middle.min.y + zone),
                ),
            ),
            (
                Side::Bottom,
                next(Side::Bottom),
                Rect::from_min_max(
                    Pos2::new(geometry.menu_bar.min.x, middle.max.y - zone),
                    Pos2::new(geometry.menu_bar.max.x, middle.max.y),
                ),
            ),
            (
                Side::Left,
                next(Side::Left),
                Rect::from_min_max(middle.min, Pos2::new(middle.min.x + zone, middle.max.y)),
            ),
            (
                Side::Right,
                next(Side::Right),
                Rect::from_min_max(Pos2::new(middle.max.x - zone, middle.min.y), middle.max),
            ),
        ];
    }

    /// The floating toolbars, each over the views in an Area of its own: its title strip, its
    /// buttons wrapped in its width, and its right edge to widen or narrow it.
    pub(crate) fn floating_bars_ui(&mut self, ui: &mut Ui, composed: &Composed) {
        let ctx = ui.ctx().clone();
        let screen = self.ui.docking.screen;
        let icon_size = self.ui.bars.icon_size;
        let shown = |name: &str| self.ui.bars.shown(name);
        let arranged = arrange(&composed.bars, &shown);
        let floating: Vec<(ToolBar, FloatAt)> = arranged
            .floating
            .iter()
            .map(|(bar, at)| ((*bar).clone(), *at))
            .collect();
        let mut events = Vec::new();
        for (bar, at) in floating {
            let strip = self.strip_of(&bar, false, icon_size);
            let sizes: Vec<(i32, i32)> = strip.items.iter().map(|item| item.size(icon_size)).collect();
            let least = sizes.iter().map(|size| size.0).max().unwrap_or(0) + 2 * TOOL_BAR_PADDING_ALONG;
            let natural = natural_width(&strip.items, icon_size).max(least);
            let asked = at.width.clamp(least as f32, natural as f32).round();
            let separators: Vec<bool> = strip
                .items
                .iter()
                .map(|item| *item == StripItem::Separator)
                .collect();
            let width = balanced_wrap_width(&sizes, &separators, asked as i32) as f32;
            let body = wrap_strip(&sizes, &separators, width as i32).size.1 as f32;
            let size = Vec2::new(width, FLOAT_HEADER + body);
            // Kept on the window: its title strip always within reach of the pointer.
            let corner = Pos2::new(
                at.x.clamp(screen.min.x, (screen.max.x - 48.0).max(screen.min.x)),
                at.y.clamp(screen.min.y, (screen.max.y - FLOAT_HEADER).max(screen.min.y)),
            );
            let rect = Rect::from_min_size(corner, size);
            self.ui.docking.drawn.push((bar.name.clone(), rect));
            let dragging = self
                .ui
                .docking
                .bar_drag
                .as_ref()
                .is_some_and(|(name, _)| *name == bar.name);
            egui::Area::new(egui::Id::new(("katanaFloatingToolBar", bar.name.as_str())))
                .order(egui::Order::Middle)
                .fixed_pos(corner)
                .constrain(false)
                .show(&ctx, |ui| {
                    let painter = ui.painter().clone();
                    painter.rect_filled(
                        rect,
                        egui::CornerRadius::same(crate::theme::style::CORNER_RADIUS),
                        palette::window().to_egui(),
                    );
                    let header = Rect::from_min_size(rect.min, Vec2::new(width, FLOAT_HEADER));
                    // The grip as a docked bar's, a little further in from the frame's corner,
                    // and the name as far after it as a docked bar's name is (layout::NAME_LEAD
                    // past the padding).
                    let grip_start = header.min + Vec2::new(2.0, 0.0);
                    let name_x = grip_start.x
                        + TOOL_BAR_PADDING_ALONG as f32
                        + layout::NAME_LEAD as f32;
                    // The name kept to the strip: a bar narrowed to a button's width is not
                    // as wide as its name.
                    painter.with_clip_rect(header.shrink2(Vec2::new(2.0, 0.0))).text(
                        Pos2::new(name_x, header.center().y),
                        egui::Align2::LEFT_CENTER,
                        &bar.title,
                        egui::TextStyle::Small.resolve(ui.style()),
                        palette::text_muted().to_egui(),
                    );
                    let handle = ui
                        .interact(
                            header,
                            egui::Id::new(("katanaToolBarGrip", bar.name.as_str())),
                            Sense::click_and_drag(),
                        )
                        .on_hover_cursor(CursorIcon::Grab)
                        .on_hover_text(format!(
                            "Drag the {} toolbar to move it, or onto a side of the window to dock \
                             it there; double-click to dock it where it was",
                            bar.title
                        ));
                    let mut grip_record =
                        ControlRecord::new(&format!("{}Grip", bar.name), ControlKind::Other);
                    grip_record.text = bar.title.clone();
                    grip_record.enabled = true;
                    grip_record.visible = true;
                    record(ui, &handle, grip_record);
                    if handle.drag_started()
                        && let Some(pointer) = ui.input(|input| input.pointer.press_origin())
                    {
                        self.ui.docking.bar_drag = Some((bar.name.clone(), pointer - rect.min));
                    }
                    if handle.double_clicked() {
                        events.push(FloatEvent::DockBack(bar.name.clone()));
                    }
                    paint_grip(
                        &painter,
                        &grip_dots(grip_start, FLOAT_HEADER, false),
                        handle.hovered() || handle.dragged() || dragging,
                    );
                    self.bar_context_menu(&handle, &bar, BarPlace::Floating(at));
                    let buttons = Rect::from_min_max(Pos2::new(rect.min.x, header.max.y), rect.max);
                    let mut floating_strip = strip.clone();
                    floating_strip.vertical = false;
                    ui.scope_builder(UiBuilder::new().max_rect(buttons), |ui| {
                        self.strip_drawn(
                            ui,
                            &floating_strip,
                            &[],
                            super::panels::StripShape::Wrapped { width },
                        );
                    });
                    // The right edge sets the width; the buttons wrap in it.
                    let edge = Rect::from_min_max(Pos2::new(rect.max.x - 4.0, rect.min.y), rect.max);
                    let resize = ui
                        .interact(
                            edge,
                            egui::Id::new(("katanaToolBarResize", bar.name.as_str())),
                            Sense::drag(),
                        )
                        .on_hover_cursor(CursorIcon::ResizeHorizontal);
                    let mut resize_record =
                        ControlRecord::new(&format!("{}Resize", bar.name), ControlKind::Other);
                    resize_record.enabled = true;
                    resize_record.visible = true;
                    record(ui, &resize, resize_record);
                    // The width asked, not the width drawn, follows the edge: the drawn width keeps
                    // to the least for its rows (`balanced_wrap_width`), and a drag measured from
                    // it would never reach the next row count.
                    if resize.dragged() {
                        events.push(FloatEvent::Width(
                            bar.name.clone(),
                            asked + resize.drag_delta().x,
                        ));
                    }
                    painter.rect_stroke(
                        rect,
                        egui::CornerRadius::same(crate::theme::style::CORNER_RADIUS),
                        Stroke::new(
                            1.0,
                            if dragging {
                                palette::accent()
                            } else {
                                palette::border()
                            }
                            .to_egui(),
                        ),
                        StrokeKind::Inside,
                    );
                });
        }
        for event in events {
            match event {
                FloatEvent::Width(name, width) => {
                    if let Some(BarPlace::Floating(at)) = self.bar_place(&name) {
                        let _ = self.drop_bar(&name, &DropSpot::Float(FloatAt { width, ..at }));
                    }
                }
                FloatEvent::DockBack(name) => {
                    let back = self
                        .ui
                        .docking
                        .docked_before
                        .iter()
                        .find(|(bar, _)| *bar == name)
                        .map(|(_, place)| *place)
                        .or_else(|| {
                            layout::places_of(&self.composed_as_built())
                                .into_iter()
                                .find(|(bar, _)| *bar == name)
                                .map(|(_, place)| place)
                        });
                    if let Some((side, line)) = back.and_then(|place| place.docked()) {
                        let spot = spot_at(&self.bar_places(), &name, side, line, usize::MAX);
                        let _ = self.drop_bar(&name, &spot);
                    }
                }
            }
        }
    }

    /// The torn-off menus: those docked in their bands beside the views, each band shared among its
    /// panels, with the seam to the views that sets the band's breadth; then the floating ones,
    /// each in an Area of its own with its edges to size it.
    pub(crate) fn menu_panels_ui(&mut self, ui: &mut Ui, geometry: &Geometry, composed: &Composed) {
        let open: Vec<String> = self
            .ui
            .docking
            .panels
            .open
            .iter()
            .map(|panel| panel.menu.clone())
            .collect();
        for menu in &open {
            let Some(found) = crate::drive::steps::find_menu(&composed.menus, menu) else {
                continue;
            };
            let icon = if found.icon.is_empty() { "Panels" } else { found.icon };
            let dock = panel_dock_name(menu);
            let chrome = &mut self.ui.panels.chrome;
            chrome
                .install(&dock, icon, DockRole::Panel)
                .set_title(&found.title.replace('&', ""));
            let floating = matches!(
                self.ui.docking.panels.find(menu).map(|panel| panel.place),
                Some(PanelPlace::Floating(_))
            );
            chrome.set_floating(&dock, floating);
        }
        for side in Side::ALL {
            let Some(band) = geometry.menu_bands[side.index()] else {
                continue;
            };
            let menus: Vec<String> = self
                .ui
                .docking
                .panels
                .docked_to(side)
                .iter()
                .map(|panel| panel.menu.clone())
                .collect();
            let count = menus.len().max(1) as f32;
            let separator = layout::SEPARATOR;
            for (at, menu) in menus.iter().enumerate() {
                let rect = if side.vertical() {
                    let each = (band.height() - separator * (count - 1.0)) / count;
                    Rect::from_min_size(
                        Pos2::new(band.min.x, band.min.y + at as f32 * (each + separator)),
                        Vec2::new(band.width(), each),
                    )
                } else {
                    let each = (band.width() - separator * (count - 1.0)) / count;
                    Rect::from_min_size(
                        Pos2::new(band.min.x + at as f32 * (each + separator), band.min.y),
                        Vec2::new(each, band.height()),
                    )
                };
                self.menu_panel_at(ui, rect, menu, composed, !side.vertical());
            }
            // The seam between the band and the views: dragged, it sets the band's breadth.
            let seam = match side {
                Side::Left => Rect::from_min_size(
                    Pos2::new(band.max.x, band.min.y),
                    Vec2::new(separator, band.height()),
                ),
                Side::Right => Rect::from_min_size(
                    Pos2::new(band.min.x - separator, band.min.y),
                    Vec2::new(separator, band.height()),
                ),
                Side::Top => Rect::from_min_size(
                    Pos2::new(band.min.x, band.max.y),
                    Vec2::new(band.width(), separator),
                ),
                Side::Bottom => Rect::from_min_size(
                    Pos2::new(band.min.x, band.min.y - separator),
                    Vec2::new(band.width(), separator),
                ),
            };
            // Pressed up to half a floating panel's sizing edge (RESIZE_EDGE) either side of the
            // 4 point seam, 10 points in all: the seam alone was a target too thin to find.
            let reach = RESIZE_EDGE / 2.0;
            let grab = if side.vertical() {
                seam.expand2(Vec2::new(reach, 0.0))
            } else {
                seam.expand2(Vec2::new(0.0, reach))
            };
            let response = ui
                .interact(
                    grab,
                    egui::Id::new(("katanaMenuBandSeam", side.word())),
                    Sense::drag(),
                )
                .on_hover_cursor(if side.vertical() {
                    CursorIcon::ResizeHorizontal
                } else {
                    CursorIcon::ResizeVertical
                });
            let mut seam_record = ControlRecord::new(
                &format!("menuBandSeam{}", side.title()),
                ControlKind::Other,
            );
            seam_record.enabled = true;
            seam_record.visible = true;
            record(ui, &response, seam_record);
            if response.dragged() {
                let delta = response.drag_delta();
                let breadth = self.ui.docking.panels.bands[side.index()]
                    + match side {
                        Side::Left => delta.x,
                        Side::Right => -delta.x,
                        Side::Top => delta.y,
                        Side::Bottom => -delta.y,
                    };
                self.ui.docking.panels.set_band(side, breadth);
            }
        }
        let floating: Vec<(String, Rect)> = self
            .ui
            .docking
            .panels
            .open
            .iter()
            .filter_map(|panel| match panel.place {
                PanelPlace::Floating(rect) => Some((panel.menu.clone(), rect)),
                PanelPlace::Docked(_) => None,
            })
            .collect();
        let ctx = ui.ctx().clone();
        let screen = self.ui.docking.screen;
        for (menu, rect) in floating {
            let corner = Pos2::new(
                rect.min.x.clamp(screen.min.x, (screen.max.x - 48.0).max(screen.min.x)),
                rect.min.y.clamp(screen.min.y, (screen.max.y - 24.0).max(screen.min.y)),
            );
            let rect = Rect::from_min_size(corner, rect.size());
            let mut resized = None;
            egui::Area::new(egui::Id::new(("katanaTornOffMenu", menu.as_str())))
                .order(egui::Order::Middle)
                .fixed_pos(corner)
                .constrain(false)
                .show(&ctx, |ui| {
                    self.menu_panel_at(ui, rect, &menu, composed, false);
                    // Every edge and corner sizes it, as a window's do. They were the right and
                    // bottom edges only, 4 points in, and a person going for the left edge, or
                    // missing a strip that thin, found it would not size (the owner, 2026-11-19).
                    // The handles are inside the panel's rectangle, where the Area's layer takes
                    // the pointer, and drawn after its contents, so they are over the title bar
                    // and the scroll bar at the edges.
                    for handle in RESIZE_HANDLES {
                        let response = ui
                            .interact(
                                handle.area(rect),
                                egui::Id::new(("katanaTornOffResize", menu.as_str(), handle.part)),
                                Sense::drag(),
                            )
                            .on_hover_cursor(handle.cursor);
                        let mut control = ControlRecord::new(
                            &format!("{}Resize{}", panel_dock_name(&menu), handle.part),
                            ControlKind::Other,
                        );
                        control.enabled = true;
                        control.visible = true;
                        record(ui, &response, control);
                        if response.dragged() {
                            resized = Some(handle.resized(rect, response.drag_delta()));
                        }
                    }
                });
            if let Some(rect) = resized {
                self.ui.docking.panels.place(&menu, PanelPlace::Floating(rect));
            }
        }
    }

    /// Torn-off menu `menu`'s panel at `rect`: the chrome's title bar, and the menu's items by the
    /// one menu drawer - in a column that scrolls, or, `across` a band along the top or the
    /// bottom, flowed into columns side by side (`flowed_items_ui`); a click runs the item's
    /// action and keeps the panel open.
    fn menu_panel_at(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        menu: &str,
        composed: &Composed,
        across: bool,
    ) {
        let Some(found) = crate::drive::steps::find_menu(&composed.menus, menu) else {
            return;
        };
        let found = found.clone();
        let dock = panel_dock_name(menu);
        self.ui.docking.drawn.push((dock.clone(), rect));
        self.dock_at(ui, rect, &dock, |window, ui| {
            let mut chosen = None;
            if across {
                flowed_items_ui(ui, &found, &dock, &mut chosen);
            } else {
                egui::ScrollArea::vertical()
                    .id_salt(("katanaTornOffItems", dock.as_str()))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.with_layout(Layout::top_down_justified(Align::Min), |ui| {
                            crate::drive::action::menu_items_ui(ui, &found, &mut chosen);
                        });
                    });
            }
            if let Some(chosen) = chosen {
                window.ui.triggered.push(chosen);
            }
        });
    }

    /// A torn-off menu's title bar asked: Minimise and Close close it, Float floats it or docks it
    /// again where it was, a press starts a drag. False for a request about any other dock.
    pub(crate) fn menu_panel_request(&mut self, request: &ChromeRequest) -> bool {
        let dock = match request {
            ChromeRequest::Minimise(name)
            | ChromeRequest::Close(name)
            | ChromeRequest::Float(name)
            | ChromeRequest::Pressed(name) => name,
            _ => return false,
        };
        let Some(menu) = panel_menu(dock).map(String::from) else {
            return false;
        };
        let Some(panel) = self.ui.docking.panels.find(&menu).cloned() else {
            return false;
        };
        match request {
            // Minimise hides it to the tray in the status bar, whose button puts it back where it
            // was; it was a second Close, and the panel was gone (the owner, 2026-11-19).
            ChromeRequest::Minimise(_) => {
                self.ui.docking.panels.minimise(&menu);
            }
            ChromeRequest::Close(_) => {
                self.ui.docking.panels.close(&menu);
            }
            ChromeRequest::Float(_) => {
                let place = match panel.place {
                    PanelPlace::Floating(_) => PanelPlace::Docked(panel.last_side),
                    PanelPlace::Docked(_) => PanelPlace::Floating(self.panel_float_rect(None)),
                };
                self.ui.docking.panels.place(&menu, place);
            }
            _ => {
                let from = self
                    .ui
                    .docking
                    .screen
                    .center();
                let pointer = self.ui.docking.pointer.unwrap_or(from);
                let rect = self.ui.docking.drawn_rect(dock).unwrap_or(Rect::NOTHING);
                // A press on a floating panel's sizing edge, which runs along its title bar too,
                // sizes it and does not also move it.
                let floating = matches!(panel.place, PanelPlace::Floating(_));
                if floating
                    && RESIZE_HANDLES
                        .iter()
                        .any(|handle| handle.area(rect).contains(pointer))
                {
                    return true;
                }
                self.ui.docking.panel_drag = Some(PanelDrag {
                    menu,
                    from: pointer,
                    rect,
                    moved: false,
                });
            }
        }
        true
    }

    /// A drag in progress, after everything else is drawn: a toolbar's drop target outlined in the
    /// accent until it is let go, and then the bar moved there; a torn-off menu's title bar held
    /// and moved, which floats it with the pointer or docks it to the side of the views it is let
    /// go at.
    pub(crate) fn drags_ui(&mut self, ui: &mut Ui, geometry: &Geometry) {
        let ctx = ui.ctx().clone();
        let (pointer, down) =
            ctx.input(|input| (input.pointer.latest_pos(), input.pointer.primary_down()));
        if pointer.is_some() {
            self.ui.docking.pointer = pointer;
        }
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("katanaDropTarget"),
        ));
        // The target filled faintly in the accent under its outline, as VS Code and Blender show
        // where a dragged panel lands: a 2 point outline alone was lost over a busy plan (the
        // sixth look audit).
        let outline = |rect: Rect| {
            painter.rect_filled(
                rect.shrink(1.0),
                egui::CornerRadius::same(2),
                palette::accent().to_egui().gamma_multiply(DROP_FILL),
            );
            painter.rect_stroke(
                rect.shrink(1.0),
                egui::CornerRadius::same(2),
                Stroke::new(2.0, palette::accent().to_egui()),
                StrokeKind::Inside,
            );
        };
        if let Some((name, grab)) = self.ui.docking.bar_drag.clone() {
            let at = pointer.unwrap_or_default();
            let width = self
                .bar_place(&name)
                .and_then(|place| match place {
                    BarPlace::Floating(float) => Some(float.width),
                    _ => None,
                })
                .unwrap_or_else(|| {
                    self.composed()
                        .bars
                        .iter()
                        .find(|(bar, _)| bar.name == name)
                        .map_or(FLOAT_WIDTH, |(bar, _)| self.float_natural(bar).min(FLOAT_WIDTH))
                });
            let spot = drop_spot(&self.ui.docking.zones, &name, at, grab, width);
            if down {
                let size = self
                    .ui
                    .docking
                    .drawn_rect(&name)
                    .map_or(Vec2::new(width, 48.0), |rect| match spot {
                        // A docked bar floated keeps its length as its width.
                        DropSpot::Float(_) => Vec2::new(width, rect.height().min(rect.width())),
                        _ => rect.size(),
                    });
                if let Some(rect) = spot_outline(&self.ui.docking.zones, &spot, size) {
                    outline(rect);
                }
                ctx.request_repaint();
            } else {
                self.ui.docking.bar_drag = None;
                if let Err(error) = self.drop_bar(&name, &spot) {
                    self.exec.log_message(&error.describe(), true);
                }
            }
        }
        if let Some(mut drag) = self.ui.docking.panel_drag.clone() {
            let at = pointer.unwrap_or(drag.from);
            if !drag.moved && (at - drag.from).length() > DRAG_THRESHOLD {
                drag.moved = true;
            }
            let floating = matches!(
                self.ui.docking.panels.find(&drag.menu).map(|panel| panel.place),
                Some(PanelPlace::Floating(_))
            );
            let side = panel_drop_side(geometry.views_and_bands, at);
            if drag.moved && floating {
                let moved = drag.rect.translate(at - drag.from);
                self.ui.docking.panels.place(&drag.menu, PanelPlace::Floating(moved));
            }
            if down {
                if drag.moved
                    && let Some(side) = side
                {
                    outline(self.band_outline(geometry, side));
                }
                if drag.moved {
                    ctx.request_repaint();
                }
                self.ui.docking.panel_drag = Some(drag);
            } else {
                self.ui.docking.panel_drag = None;
                if drag.moved {
                    let place = match side {
                        Some(side) => Some(PanelPlace::Docked(side)),
                        None if !floating => Some(PanelPlace::Floating(Rect::from_min_size(
                            at - Vec2::new(24.0, 12.0),
                            PANEL_SIZE,
                        ))),
                        None => None,
                    };
                    if let Some(place) = place {
                        self.ui.docking.panels.place(&drag.menu, place);
                    }
                }
            }
        }
    }

    /// Where a torn-off menu docked to `side` would be: that side's band, or the strip of the
    /// views a new band would take.
    fn band_outline(&self, geometry: &Geometry, side: Side) -> Rect {
        if let Some(band) = geometry.menu_bands[side.index()] {
            return band;
        }
        let views = geometry.views_and_bands;
        let breadth = self.ui.docking.panels.bands[side.index()];
        match side {
            Side::Left => Rect::from_min_max(views.min, Pos2::new(views.min.x + breadth, views.max.y)),
            Side::Right => Rect::from_min_max(Pos2::new(views.max.x - breadth, views.min.y), views.max),
            Side::Top => Rect::from_min_max(views.min, Pos2::new(views.max.x, views.min.y + breadth)),
            Side::Bottom => Rect::from_min_max(Pos2::new(views.min.x, views.max.y - breadth), views.max),
        }
    }
}

/// The least width of a column of a torn-off menu flowed across a band, and the space between two
/// columns: some sixteen characters of an item's text, and the menu frame's gap twice.
pub const FLOW_LEAST_COLUMN: f32 = 160.0;
pub const FLOW_COLUMN_GAP: f32 = 12.0;

/// `menu`'s items flowed into columns the height of `ui` (docking::flow_columns), side by side
/// in a strip that scrolls across when they are wider than the band; each column as wide as its
/// widest item. The items are drawn by the one menu drawer (drive::action::menu_item_ui), each
/// once, so a step finds each by its name as in any menu.
///
/// The heights the columns are flowed by are the items' as the last frame drew them (kept under
/// the panel's id), worked out from the style before a frame has; a frame that finds them
/// changed asks for another, so no item is left cut at the band's bottom.
fn flowed_items_ui(ui: &mut Ui, menu: &Menu, dock: &str, chosen: &mut Option<String>) {
    egui::containers::menu::menu_style(ui.style_mut());
    let room = ui.available_height().max(0.0);
    let spacing = ui.spacing().item_spacing.y;
    let key = egui::Id::new(("katanaTornOffFlow", dock));
    let kept: Vec<f32> = ui.data(|data| data.get_temp(key)).unwrap_or_default();
    let kinds: Vec<FlowItem> = menu
        .items
        .iter()
        .map(|item| match item {
            MenuItem::Section(_) => FlowItem::Section,
            MenuItem::Separator => FlowItem::Separator,
            MenuItem::Action(_) | MenuItem::Menu(_) => FlowItem::Item,
        })
        .collect();
    let heights: Vec<f32> = if kept.len() == menu.items.len() {
        kept.clone()
    } else {
        menu.items.iter().map(|item| item_height(ui, item)).collect()
    };
    let widths: Vec<f32> = menu.items.iter().map(|item| item_width(ui, item)).collect();
    let columns = flow_columns(&kinds, &heights, room, spacing);
    let mut drawn = heights.clone();
    egui::ScrollArea::horizontal()
        .id_salt(("katanaTornOffColumns", dock))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = FLOW_COLUMN_GAP;
                for column in &columns {
                    let width = column
                        .iter()
                        .map(|&index| widths[index])
                        .fold(FLOW_LEAST_COLUMN, f32::max);
                    ui.allocate_ui_with_layout(
                        Vec2::new(width, room),
                        Layout::top_down_justified(Align::Min),
                        |ui| {
                            ui.set_width(width);
                            ui.spacing_mut().item_spacing.y = spacing;
                            for &index in column {
                                let response = crate::drive::action::menu_item_ui(
                                    ui,
                                    &menu.items[index],
                                    chosen,
                                );
                                drawn[index] = response.rect.height();
                            }
                        },
                    );
                }
            });
        });
    // A rule left out at a column's top drew nothing: its kept height stays as it was.
    let changed = kept.len() != drawn.len()
        || kept
            .iter()
            .zip(&drawn)
            .any(|(kept, drawn)| (kept - drawn).abs() > 0.5);
    if changed {
        ui.data_mut(|data| data.insert_temp(key, drawn));
        ui.ctx().request_repaint();
    }
}

/// An item's height before a frame has drawn it, as the menu drawer draws it: an action's or a
/// submenu's button a control high (its icon and its text inside), a section's title a line of
/// its font and its padding, a rule its height.
fn item_height(ui: &Ui, item: &MenuItem) -> f32 {
    use crate::theme::style::{MENU_ICON_SIZE, SECTION_PADDING, SEPARATOR_HEIGHT, section_font_id};
    match item {
        MenuItem::Action(_) | MenuItem::Menu(_) => ui
            .spacing()
            .interact_size
            .y
            .max(MENU_ICON_SIZE)
            .max(ui.text_style_height(&egui::TextStyle::Button)),
        MenuItem::Section(title) => {
            ui.painter()
                .layout_no_wrap(title.clone(), section_font_id(), egui::Color32::PLACEHOLDER)
                .size()
                .y
                + SECTION_PADDING
        }
        MenuItem::Separator => SEPARATOR_HEIGHT,
    }
}

/// The width an item needs, as the menu drawer lays it out: the button's padding, the gutter, its
/// text, and its shortcut or submenu arrow with the space after it; a section's title from the
/// items' text indent with the theme's margins. A column is as wide as its widest item.
fn item_width(ui: &Ui, item: &MenuItem) -> f32 {
    use crate::theme::style::{INDENT, MENU_CHECK_WIDTH, MENU_GUTTER_WIDTH, section_font_id};
    let text = |text: String, font: egui::FontId| {
        ui.painter()
            .layout_no_wrap(text, font, egui::Color32::PLACEHOLDER)
            .size()
            .x
    };
    let body = egui::TextStyle::Button.resolve(ui.style());
    let padding = ui.spacing().button_padding.x;
    let gap = ui.spacing().icon_spacing;
    // The gutter, the gap after it and the text; then, for a shortcut or an arrow, the gap before
    // the space egui grows between them, the gap after it, the right-hand text, and the space
    // after that (drive::action's right_trail) - the check column's width in all.
    let button = |label: String, right: Option<String>| {
        let mut width = 2.0 * padding + MENU_GUTTER_WIDTH + gap + text(label, body.clone());
        if let Some(right) = right {
            width += 2.0 * gap + text(right, body.clone()) + MENU_CHECK_WIDTH.max(gap);
        }
        width
    };
    match item {
        MenuItem::Action(action) => button(
            action.display_text(),
            action
                .shortcut
                .as_ref()
                .map(crate::drive::action::shortcut_text),
        ),
        MenuItem::Menu(submenu) => button(
            submenu.title.replace('&', ""),
            Some(String::from(
                egui::containers::menu::SubMenuButton::RIGHT_ARROW,
            )),
        ),
        MenuItem::Section(title) => {
            crate::theme::style::menu_text_indent(ui)
                + text(title.clone(), section_font_id())
                + padding
                + 2.0 * INDENT
        }
        MenuItem::Separator => 0.0,
    }
}

fn not_torn_off(menu: &str) -> katana_core::Error {
    make_error(
        ErrorCode::NotFound,
        "that menu is not torn off; viewMenuTearOff<Menu> tears it off",
        menu,
    )
}
