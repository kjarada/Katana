//! The window's side of docking (main_window::docking): the actions that move a toolbar or a
//! torn-off menu, each through one door (`MainWindow::docking_action`), what a headless run
//! reports of where they are, and the drawing of what does not sit in a row - a floating toolbar,
//! a torn-off menu's panel, a drag's drop target - and of a docked bar's grip.
//!
//! THE GRIP. Every docked toolbar has six dots in the padding before its first item - its grip -
//! and is dragged by it, and by its name when it shows one: the grip costs no room, so no bar is
//! longer for being movable and the rows still fit a 1280 laptop. The handle is a named control
//! (`<Bar>Grip`), and its right-click menu holds the bar's View > Toolbars > Place items. A drag shows where the bar would go as an accent
//! outline, the only accent in the window that is not a thing chosen or active, since it is the
//! place being chosen; it is moved when it is let go. Rejected: moving the bar with the pointer
//! while it is docked, which reflows the row under the pointer at every frame and leaves the
//! person chasing the place they aimed at.
//!
//! A FLOATING TOOLBAR is an egui Area over the views: a title strip (its grip and its name) over
//! its buttons, wrapped in rows of its width (`flyout_button::tool_bar_wrapped`), and a right edge
//! that sets the width. A double click on its title docks it where it was last docked. Rejected:
//! an egui Window, whose title bar, frame and resize grip are egui's and would not match the
//! window's chrome; and a floating bar sized by its height as well, which is two ways to choose one
//! shape.
//!
//! A TORN-OFF MENU is a panel of the window's own chrome (the panels' `DockChrome`, its title bar
//! with Minimise, Float and Close), named `<menu>TornOff`, holding the menu's items drawn by the
//! one menu drawer (drive::action::menu_items_ui): its icons, ticks and shortcuts, each item a
//! click that runs its action as the menu's does, and kept open. Docked, it is in a band between
//! the views and a side; floating, an Area of its own sized by its edges. Its title bar is dragged
//! to a side of the views to dock it there, or into the views to float it.

use std::collections::BTreeMap;

use egui::{Align, CursorIcon, Layout, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, UiBuilder, Vec2};
use katana_core::error::make_error;
use katana_core::{ErrorCode, Status};

use super::actions::{BarPlace, Composed, reset_tool_bars_action, tool_bar_place_menu};
use super::docking::{
    DRAG_THRESHOLD, DockingAction, DropSpot, DropZones, FLOAT_HEADER, FLOAT_WIDTH, FloatAt,
    LEAST_PANEL, NEW_LINE_ZONE, PANEL_SIZE, PanelDrag, PanelPlace, Places, Side,
    apply_drop, drop_spot, last_line, line_members, panel_dock_name, panel_drop_side, panel_menu,
    panel_place_text, spot_at, spot_outline,
};
use super::layout::{self, BarLines, Geometry, natural_width};
use super::window::MainWindow;
use crate::dock_chrome::{ChromeRequest, DockRole};
use crate::drive::action::{Menu, MenuItem, ToolBar};
use crate::drive::registry::{ControlKind, ControlRecord, record};
use crate::theme::palette;
use crate::tools::flyout_button::{
    StripItem, TOOL_BAR_PADDING_ACROSS, TOOL_BAR_PADDING_ALONG, wrap_strip,
};

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

/// The six dots of a grip in `rect`: two columns of three along a row's bar, three columns of two
/// across a column's, in the muted token - a handle that reads as one without a line of its own.
pub fn paint_grip(painter: &egui::Painter, rect: Rect, vertical: bool) {
    let colour = palette::text_muted().to_egui();
    let (columns, rows) = if vertical { (3, 2) } else { (2, 3) };
    let step = 3.0;
    let origin = rect.center()
        - Vec2::new(
            step * (columns - 1) as f32 / 2.0,
            step * (rows - 1) as f32 / 2.0,
        );
    for column in 0..columns {
        for row in 0..rows {
            painter.circle_filled(
                origin + Vec2::new(column as f32 * step, row as f32 * step),
                0.8,
                colour,
            );
        }
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

    /// Toolbar `name` docked to `side` (None: floated), as View > Toolbars > Place does: along the
    /// top, in the first row it fits in whole at the window's width, else a row of its own; down
    /// a side or along the bottom, at the end of the side's first line. A bar already on that side
    /// stays where it is on it.
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
            Some(Side::Top) => self.top_row_for(name, &places),
            Some(side) => match last_line(&places, side) {
                Some(_) => DropSpot::Line {
                    side,
                    line: 0,
                    before: None,
                },
                None => DropSpot::NewLine { side, before: 0 },
            },
        };
        self.drop_bar(name, &spot)
    }

    fn show_bar(&mut self, name: &str) -> Status {
        self.ui.bars.hidden.remove(name);
        Ok(())
    }

    /// The first top row with room for `name` whole at the window's last width (the rows as the
    /// toolbar-fit check measures them), else a new row under the last.
    fn top_row_for(&self, name: &str, places: &Places) -> DropSpot {
        let width = self.ui.docking.screen.width();
        let composed = self.composed();
        let natural = |bar: &str| -> f32 {
            composed
                .bars
                .iter()
                .find(|(each, _)| each.name == bar)
                .map_or(0.0, |(each, _)| self.docked_natural(each, false) as f32)
        };
        let needed = natural(name);
        let last = last_line(places, Side::Top);
        for line in 0..=last.unwrap_or(0) {
            let taken: f32 = line_members(places, Side::Top, line)
                .into_iter()
                .filter(|bar| *bar != name && self.ui.bars.shown(bar))
                .map(natural)
                .sum();
            if last.is_some() && taken + needed <= width {
                return DropSpot::Line {
                    side: Side::Top,
                    line,
                    before: None,
                };
            }
        }
        DropSpot::NewLine {
            side: Side::Top,
            before: last.map_or(0, |last| last + 1),
        }
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
                .map(|bar| {
                    let docked = layout::docked_bar(bar, self.ui.bars.names_shown, true);
                    let strip = self.strip_of(&docked, true, self.ui.bars.icon_size);
                    strip
                        .items
                        .iter()
                        .map(|item| item.size(self.ui.bars.icon_size).0)
                        .max()
                        .unwrap_or(0)
                        + 2 * TOOL_BAR_PADDING_ACROSS
                })
                .max()
                .map_or(layout::SIDE, |widest| (widest as f32).max(layout::SIDE))
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
        // The dots in the bar's padding before its first item, across a row, along a column.
        let vertical = place.vertical();
        let dots = if vertical {
            Rect::from_min_size(
                bar_rect.min,
                Vec2::new(bar_rect.width(), TOOL_BAR_PADDING_ALONG as f32),
            )
        } else {
            Rect::from_min_size(
                bar_rect.min,
                Vec2::new(TOOL_BAR_PADDING_ALONG as f32, bar_rect.height()),
            )
        };
        paint_grip(ui.painter(), dots, vertical);
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
            let width = at.width.clamp(least as f32, natural as f32).round();
            let separators: Vec<bool> = strip
                .items
                .iter()
                .map(|item| *item == StripItem::Separator)
                .collect();
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
                    let grip = Rect::from_min_size(
                        header.min + Vec2::new(4.0, 0.0),
                        Vec2::new(TOOL_BAR_PADDING_ALONG as f32, FLOAT_HEADER),
                    );
                    paint_grip(&painter, grip, false);
                    // The name kept to the strip: a bar narrowed to a button's width is not
                    // as wide as its name.
                    painter.with_clip_rect(header.shrink2(Vec2::new(2.0, 0.0))).text(
                        Pos2::new(grip.max.x + 4.0, header.center().y),
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
                    if resize.dragged() {
                        events.push(FloatEvent::Width(
                            bar.name.clone(),
                            width + resize.drag_delta().x,
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
            self.ui
                .panels
                .chrome
                .install(&panel_dock_name(menu), icon, DockRole::Panel)
                .set_title(&found.title.replace('&', ""));
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
                self.menu_panel_at(ui, rect, menu, composed);
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
            let response = ui
                .interact(
                    seam,
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
                    self.menu_panel_at(ui, rect, &menu, composed);
                    // The right and bottom edges and the corner between them size it.
                    let edge = 4.0;
                    for (part, area, cursor) in [
                        (
                            "Right",
                            Rect::from_min_max(
                                Pos2::new(rect.max.x - edge, rect.min.y + 24.0),
                                Pos2::new(rect.max.x, rect.max.y - edge),
                            ),
                            CursorIcon::ResizeHorizontal,
                        ),
                        (
                            "Bottom",
                            Rect::from_min_max(
                                Pos2::new(rect.min.x, rect.max.y - edge),
                                Pos2::new(rect.max.x - edge, rect.max.y),
                            ),
                            CursorIcon::ResizeVertical,
                        ),
                        (
                            "Corner",
                            Rect::from_min_max(rect.max - Vec2::splat(edge * 2.0), rect.max),
                            CursorIcon::ResizeSouthEast,
                        ),
                    ] {
                        let response = ui
                            .interact(
                                area,
                                egui::Id::new(("katanaTornOffResize", menu.as_str(), part)),
                                Sense::drag(),
                            )
                            .on_hover_cursor(cursor);
                        if response.dragged() {
                            let delta = response.drag_delta();
                            let grow = match part {
                                "Right" => Vec2::new(delta.x, 0.0),
                                "Bottom" => Vec2::new(0.0, delta.y),
                                _ => delta,
                            };
                            resized = Some(Rect::from_min_size(
                                rect.min,
                                (rect.size() + grow).max(LEAST_PANEL),
                            ));
                        }
                    }
                });
            if let Some(rect) = resized {
                self.ui.docking.panels.place(&menu, PanelPlace::Floating(rect));
            }
        }
    }

    /// Torn-off menu `menu`'s panel at `rect`: the chrome's title bar, and the menu's items by the
    /// one menu drawer, in a column that scrolls; a click runs the item's action and keeps the
    /// panel open.
    fn menu_panel_at(&mut self, ui: &mut Ui, rect: Rect, menu: &str, composed: &Composed) {
        let Some(found) = crate::drive::steps::find_menu(&composed.menus, menu) else {
            return;
        };
        let found = found.clone();
        let dock = panel_dock_name(menu);
        self.ui.docking.drawn.push((dock.clone(), rect));
        self.dock_at(ui, rect, &dock, |window, ui| {
            let mut chosen = None;
            egui::ScrollArea::vertical()
                .id_salt(("katanaTornOffItems", dock.as_str()))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.with_layout(Layout::top_down_justified(Align::Min), |ui| {
                        crate::drive::action::menu_items_ui(ui, &found, &mut chosen);
                    });
                });
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
            ChromeRequest::Minimise(_) | ChromeRequest::Close(_) => {
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
        let outline = |rect: Rect| {
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

/// The error for a torn-off menu's action on a menu not torn off.
fn not_torn_off(menu: &str) -> katana_core::Error {
    make_error(
        ErrorCode::NotFound,
        "that menu is not torn off; viewMenuTearOff<Menu> tears it off",
        menu,
    )
}
