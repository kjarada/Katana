//! The chrome every dock wears - the four panels and every drawing view: a title bar of the
//! window's own with Minimise, Float, Maximise (a view's) and Close, and a view's own tools; and
//! the bookkeeping that puts a dock back where it was. The port of dock_chrome.hpp/.cpp onto
//! egui_dock 0.21.1 (the plan's section 4.3): the window's `TabViewer` draws each tab through
//! `DockChrome::dock_ui`, and the window applies what its buttons asked once the dock area has
//! been drawn (`DockChrome::apply`), since egui_dock holds its `DockState` while it draws.
//!
//! WHY A TITLE BAR OF OUR OWN (the C++'s reasons, kept). A plain dock's title bar has no Minimise,
//! no Maximise and nowhere for a view's own controls (its kind, its layers), so every dock wears
//! this one: [icon] title ... [tools] [_] [float] [max] [x], 26 points tall - one row of 22 point
//! buttons with 2 above and below, the height of the old style-drawn bar, so the drawing area did
//! not shrink to make room for the buttons.
//!
//! WHY MINIMISE IS A TRAY. A floating dock keeps the window's shortcuts, and a docked panel is not
//! a window at all, so Minimise HIDES the dock and puts a button for it on a toolbar at the bottom
//! of the main window ("MinimisedToolBar", its buttons `Restore<dock>`), and the button puts it
//! back exactly where it was: in its place and tab when docked, at its rectangle when floating.
//! Docked, the place is egui_dock's tree: the layout as it was when the dock went - every split
//! and its share - is kept with it, and given back whole when the tabs open then are the tabs open
//! now; otherwise the dock is put in the first place there is (the C++ gave back the sizes of the
//! docks around it, which Qt shared out when a dock came back).
//!
//! ONE OWNER. One `DockChrome` per window holds the tray and the bookkeeping, and the panels (the
//! main window) and the views (the workspace) share it: a panel and a view minimised into two
//! trays would be two ways of doing one thing. It knows nothing of views or panels beyond the
//! `DockRole` it is told, and of a tab nothing but its dock's object name (`DockTab`).
//!
//! WHAT egui_dock CANNOT DO that the C++ did: a floating dock is a window inside the main window,
//! not one that can go to another screen (an owner's question, the plan's last section); and a
//! dock is dragged by egui_dock's own tab, not by this bar. Rolling a dock up to its title bar was
//! rejected in the C++ (it took the whole column with it) and is not done here either.
//!
//! The icons are the window's (I's icon set, by name), lent through `ChromeIcons`; the colours
//! are the theme's tokens. Every button is a named control (drive::controls), so a step presses
//! `DockMinimiseButton` of `View2` as a person clicks it.

use std::collections::{BTreeSet, HashMap};
use std::fmt;

use egui::{
    Align, Color32, Context, CornerRadius, Layout, Pos2, Rect, Response, RichText, Sense, Shape,
    Stroke, StrokeKind, TextureHandle, TextureId, TextureOptions, Ui, UiBuilder, Vec2,
};
use egui_dock::{DockState, TabPath};

use crate::canvas::Image;
use crate::drive::action::Action;
use crate::drive::controls::{accept_fill, accept_press, custom};
use crate::drive::mailbox::{Act, no_field};
use crate::drive::registry::{
    ControlKind, ControlRecord, TargetKind, TargetSpec, begin_target, end_target, settle, take,
};
use crate::theme::palette;

/// A title bar button's side: the window buttons of the desktop the person is on are drawn at 10
/// to 16 px, and four of them and a view's tools must fit a view a third of a screen wide.
pub const BUTTON_SIZE: f32 = 22.0;
/// Above and below the buttons: 22 + 2 + 2 is the old style-drawn bar's 26.
pub const BAR_PADDING: f32 = 2.0;
/// The bar's height.
pub const BAR_HEIGHT: f32 = BUTTON_SIZE + 2.0 * BAR_PADDING;
/// The icons in the buttons: 14 points in a 22 point button.
pub const ICON_SIZE: f32 = 14.0;
/// The dock's own icon before its title, in its 18 x 22 cell.
pub const DOCK_ICON_SIZE: f32 = 16.0;
/// The accent stroke across the top of the active view's bar: the 2 px line under the selected tab,
/// so the two read as one idea.
pub const ACTIVE_STROKE: f32 = 2.0;
/// A tool shown whatever the bar's width.
pub const ALWAYS: i32 = i32::MAX;
/// What an optional tool leaves the title: room to read it whole - its text's width and TITLE_GAP
/// after it - and never more than TITLE_ROOM, so a narrow view keeps its name before its extras.
pub const TITLE_ROOM: f32 = 56.0;
/// Between the title's text and the first tool: the gap the bar leaves before its window buttons.
pub const TITLE_GAP: f32 = 6.0;
/// The least the title is given.
pub const TITLE_LEAST: f32 = 24.0;
/// The bar's margins, left and right, and the spacing between what it holds (its QHBoxLayout's).
const LEFT_MARGIN: f32 = 6.0;
const RIGHT_MARGIN: f32 = 3.0;
const SPACING: f32 = 1.0;
/// After the icon, before the title; and before the window buttons.
const ICON_GAP: f32 = 4.0;
const BUTTONS_GAP: f32 = 6.0;
/// The icon's cell.
const ICON_CELL: Vec2 = Vec2::new(18.0, BUTTON_SIZE);
/// A button's frame and the padding inside it (theme.cpp's [chrome] rules): a 1 point border,
/// 2 points of padding, and 13 on the right of a button whose menu opens on a click, the box its
/// arrow is drawn in.
const BORDER: f32 = 1.0;
const PADDING: f32 = 2.0;
const MENU_PADDING: f32 = 13.0;
const RADIUS: u8 = 4;
/// Qt's dash pattern for a one pixel pen: four on, two off.
const DASH: f32 = 4.0;
const GAP: f32 = 2.0;

/// `value` (points) on the device pixel grid: every button sits on whole device pixels at every
/// scale, so its icon is drawn at the same device pixels in each (the 125 % reruns).
fn snap(value: f32, pixels_per_point: f32) -> f32 {
    (value * pixels_per_point).round() / pixels_per_point
}

/// Paints an icon of the window's set by name into an image so many device pixels square.
type IconPainter = Box<dyn Fn(&str, u32) -> Option<Image>>;

/// The icons the chrome draws, by name - the window's icon set (I's), lent by the window - kept as
/// egui textures for each size they are drawn at.
#[derive(Default)]
pub struct ChromeIcons {
    paint: Option<IconPainter>,
    textures: HashMap<(String, u32), Option<TextureHandle>>,
}

impl fmt::Debug for ChromeIcons {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ChromeIcons")
            .field("painted", &self.paint.is_some())
            .field("textures", &self.textures.len())
            .finish()
    }
}

impl ChromeIcons {
    /// Icons painted by `paint`: the icon `name` in an image `pixels` square, None for no such
    /// icon.
    pub fn new(paint: impl Fn(&str, u32) -> Option<Image> + 'static) -> Self {
        Self {
            paint: Some(Box::new(paint)),
            textures: HashMap::new(),
        }
    }

    /// No icons: the buttons are drawn with their frames alone.
    pub fn none() -> Self {
        Self::default()
    }

    /// The texture of icon `name` at `pixels`, painted the first time it is asked for.
    pub fn texture(&mut self, ctx: &Context, name: &str, pixels: u32) -> Option<TextureId> {
        if name.is_empty() || pixels == 0 {
            return None;
        }
        let paint = self.paint.as_ref()?;
        let handle = self
            .textures
            .entry((String::from(name), pixels))
            .or_insert_with(|| {
                paint(name, pixels)
                    .filter(|image| !image.is_null())
                    .map(|image| {
                        ctx.load_texture(
                            format!("katanaChromeIcon{name}{pixels}"),
                            image.to_color_image(),
                            TextureOptions::NEAREST,
                        )
                    })
            });
        handle.as_ref().map(TextureHandle::id)
    }
}

/// One button of a title bar (makeTitleBarButton): its object name, its icon, the label a screen
/// reader gives it and its tooltip's sentence, and how it is drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct TitleBarButton {
    pub name: String,
    pub icon: String,
    /// Shown in bold over the tip, and the button's accessible name: what `--report` says.
    pub label: String,
    pub tip: String,
    /// 22 points; a view's kind switcher is widened to 34 by its workspace.
    pub width: f32,
    /// Some for a button that can be checked: whether it is (a view's Link).
    pub checked: Option<bool>,
    /// A Link waiting for a second view: framed dashed when checked.
    pub waiting: bool,
    /// Its menu opens on a click (InstantPopup): padded on the right for the arrow beside its icon.
    pub menu: bool,
    /// The one window button that loses something: red on hover.
    pub close: bool,
    pub enabled: bool,
}

impl TitleBarButton {
    /// makeTitleBarButton: a 22 point button named `name`, its icon `icon`, read as `label`, with
    /// `tip` under the label in its tooltip.
    pub fn new(name: &str, icon: &str, label: &str, tip: &str) -> Self {
        Self {
            name: String::from(name),
            icon: String::from(icon),
            label: String::from(label),
            tip: String::from(tip),
            width: BUTTON_SIZE,
            checked: None,
            waiting: false,
            menu: false,
            close: false,
            enabled: true,
        }
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    pub fn checkable(mut self, checked: bool) -> Self {
        self.checked = Some(checked);
        self
    }

    pub fn waiting(mut self, waiting: bool) -> Self {
        self.waiting = waiting;
        self
    }

    pub fn with_menu(mut self) -> Self {
        self.menu = true;
        self
    }

    pub fn close_style(mut self) -> Self {
        self.close = true;
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// setTitleBarTip: a button whose words change with what it does (a view's Link and zoom
    /// tools).
    pub fn set_tip(&mut self, label: &str, tip: &str) {
        self.label = String::from(label);
        self.tip = String::from(tip);
    }

    /// Where its icon is drawn, in points from its top left: centred in what its border and
    /// padding leave (4, 4 in a 22 point button, and in the 34 point kind switcher padded for its
    /// arrow).
    pub fn icon_offset(&self) -> Vec2 {
        let right = if self.menu { MENU_PADDING } else { PADDING };
        let across = self.width - 2.0 * BORDER - PADDING - right;
        let down = BUTTON_SIZE - 2.0 * (BORDER + PADDING);
        Vec2::new(
            BORDER + PADDING + ((across - ICON_SIZE) / 2.0).floor(),
            BORDER + PADDING + ((down - ICON_SIZE) / 2.0).floor(),
        )
    }
}

/// A frame drawn as Qt draws a dashed one pixel border: four on, two off, along each straight
/// side, between the rounded corners.
fn dashed_frame(ui: &Ui, rect: Rect, color: Color32) {
    let painter = ui.painter();
    let corner = f32::from(RADIUS);
    let dash = |from: Pos2, along: Vec2, length: f32| {
        let mut at = 0.0;
        while at < length {
            let piece = DASH.min(length - at);
            let start = from + along * at;
            let size = if along.x != 0.0 {
                Vec2::new(piece, 1.0)
            } else {
                Vec2::new(1.0, piece)
            };
            // A polygon, not a rectangle: epaint draws a rectangle a pixel thin as a line with
            // caps a pixel long, which would fill the two pixel gaps half way.
            let piece = Rect::from_min_size(start, size);
            painter.add(Shape::convex_polygon(
                vec![
                    piece.left_top(),
                    piece.right_top(),
                    piece.right_bottom(),
                    piece.left_bottom(),
                ],
                color,
                Stroke::NONE,
            ));
            at += DASH + GAP;
        }
    };
    let width = rect.width() - 2.0 * corner;
    let height = rect.height() - 2.0 * corner;
    dash(rect.min + Vec2::new(corner, 0.0), Vec2::X, width);
    dash(
        Pos2::new(rect.min.x + corner, rect.max.y - 1.0),
        Vec2::X,
        width,
    );
    dash(rect.min + Vec2::new(0.0, corner), Vec2::Y, height);
    dash(
        Pos2::new(rect.max.x - 1.0, rect.min.y + corner),
        Vec2::Y,
        height,
    );
}

/// A title bar button drawn at `rect`, a named control (drive::controls): a press of it is a
/// click, refused hidden or disabled as every button's is; a fill is refused as a button's.
/// Drawn by theme.cpp's rules for the chrome: no frame at rest, the hover colour under the
/// pointer (the error colour for Close), and when checked the raised colour in an accent frame,
/// dashed while the button is waiting.
pub fn title_bar_button(
    ui: &mut Ui,
    rect: Rect,
    button: &TitleBarButton,
    icons: &mut ChromeIcons,
) -> Response {
    ui.add_enabled_ui(button.enabled, |ui| {
        let taken = take(ui, &button.name);
        let id = ui.id().with(("katanaTitleBarButton", button.name.as_str()));
        let mut response = ui.interact(rect, id, Sense::click());
        if let Some(taken) = &taken {
            match &taken.posted.act {
                Act::Press => {
                    if accept_press(ui, taken) {
                        response
                            .flags
                            .insert(egui::response::Flags::FAKE_PRIMARY_CLICKED);
                        settle(ui, taken, Ok(()));
                    }
                }
                Act::Fill(text) => {
                    if accept_fill(ui, taken) {
                        settle(ui, taken, Err(no_field(&button.name, text)));
                    }
                }
                Act::ScrollTo => {
                    response.scroll_to_me(None);
                    settle(ui, taken, Ok(()));
                }
            }
        }

        let checked = button.checked == Some(true);
        let hovered = response.hovered() && ui.is_enabled();
        let radius = CornerRadius::same(RADIUS);
        let fill = if checked {
            Some(palette::raised())
        } else if hovered && button.close {
            Some(palette::error())
        } else if hovered {
            Some(palette::hover())
        } else {
            None
        };
        if let Some(fill) = fill {
            ui.painter().rect_filled(rect, radius, fill.to_egui());
        }
        let accent = palette::accent().to_egui();
        if checked && button.waiting {
            dashed_frame(ui, rect, accent);
        } else if checked || response.has_focus() {
            ui.painter()
                .rect_stroke(rect, radius, Stroke::new(1.0, accent), StrokeKind::Inside);
        }

        let pixels_per_point = ui.ctx().pixels_per_point();
        let pixels = (ICON_SIZE * pixels_per_point).round() as u32;
        if let Some(texture) = icons.texture(ui.ctx(), &button.icon, pixels) {
            let offset = button.icon_offset();
            let at = Pos2::new(
                snap(rect.min.x + offset.x, pixels_per_point),
                snap(rect.min.y + offset.y, pixels_per_point),
            );
            let side = pixels as f32 / pixels_per_point;
            let tint = if ui.is_enabled() {
                Color32::WHITE
            } else {
                Color32::from_gray(128)
            };
            ui.painter().image(
                texture,
                Rect::from_min_size(at, Vec2::splat(side)),
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                tint,
            );
        }
        if button.menu {
            // Fusion's arrow, 8 wide and 4 deep, centred in the 13 point box set at the right of
            // the padding: 22.5 to 30.5 across and 9.5 to 13.5 down in the 34 point switcher.
            let left = rect.min.x + button.width - 11.5;
            let top = rect.min.y + 9.5;
            ui.painter().add(Shape::convex_polygon(
                vec![
                    Pos2::new(left, top),
                    Pos2::new(left + 8.0, top),
                    Pos2::new(left + 4.0, top + 4.0),
                ],
                palette::text().to_egui(),
                Stroke::NONE,
            ));
        }

        let kind = if button.checked.is_some() {
            ControlKind::Toggle
        } else {
            ControlKind::Button
        };
        let mut record = ControlRecord::new(&button.name, kind);
        record.text.clone_from(&button.label);
        record.checked = button.checked;
        custom(ui, &response, record);
        let (label, tip) = (button.label.clone(), button.tip.clone());
        response.on_hover_ui(move |ui| {
            ui.strong(label);
            if !tip.is_empty() {
                ui.label(tip);
            }
        })
    })
    .inner
}

/// What a dock is (dock_chrome.hpp's DockRole).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DockRole {
    /// Close hides it; the menus bring it back.
    #[default]
    Panel,
    /// A drawing view: it can be maximised, and one of them is active.
    View,
}

/// The window buttons of a bar, in their order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WindowButton {
    Minimise,
    Float,
    Maximise,
    Close,
}

impl WindowButton {
    /// Its object name.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Minimise => "DockMinimiseButton",
            Self::Float => "DockFloatButton",
            Self::Maximise => "DockMaximiseButton",
            Self::Close => "DockCloseButton",
        }
    }
}

/// What a bar shows of its dock's state, read from the chrome before the dock area is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BarFacts {
    pub floating: bool,
    pub maximised: bool,
}

/// A tool on a bar: shown whatever the width (ALWAYS) or only while it fits, highest priority
/// first.
#[derive(Clone, Debug, PartialEq)]
struct BarTool {
    button: TitleBarButton,
    priority: i32,
    wanted: bool,
    shown: bool,
}

/// Where a bar's parts go, for one width.
#[derive(Clone, Debug, PartialEq)]
pub struct BarLayout {
    /// The icon's cell, or the leading control's rectangle.
    pub lead: Rect,
    pub title: Rect,
    /// The shown tools, by their index among the bar's tools.
    pub tools: Vec<(usize, Rect)>,
    pub buttons: Vec<(WindowButton, Rect)>,
}

/// What a press on a bar asks of its dock's owner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChromeRequest {
    /// A press on the bar that is not a button: a view's title makes it the active view.
    Pressed(String),
    Minimise(String),
    /// Float, or dock again (the Float button, a double click on the title).
    Float(String),
    Maximise(String),
    /// Close: whatever closing that dock means to its owner (a view closes, a panel hides).
    Close(String),
    /// A dock's own tool or its leading control: the dock, and the tool's object name.
    Tool(String, String),
    /// A minimised dock's tray button.
    Restore(String),
}

/// The title bar of one dock (DockTitleBar): its icon, its title, the leading control that takes
/// the icon's place (a view's kind switcher), its tools, and whether it is the active view's.
#[derive(Clone, Debug, PartialEq)]
pub struct DockTitleBar {
    role: DockRole,
    icon: String,
    title: String,
    active: bool,
    leading: Option<TitleBarButton>,
    tools: Vec<BarTool>,
    /// A dock that cannot float shows no Float.
    pub floatable: bool,
    /// A dock that cannot be closed shows no Close.
    pub closable: bool,
}

impl DockTitleBar {
    pub fn new(icon: &str, role: DockRole) -> Self {
        Self {
            role,
            icon: String::from(icon),
            title: String::new(),
            active: false,
            leading: None,
            tools: Vec::new(),
            floatable: true,
            closable: true,
        }
    }

    pub fn role(&self) -> DockRole {
        self.role
    }

    pub fn icon(&self) -> &str {
        &self.icon
    }

    pub fn set_icon(&mut self, icon: &str) {
        self.icon = String::from(icon);
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    /// The dock's window title, which the bar shows elided to the room it has.
    pub fn set_title(&mut self, title: &str) {
        self.title = String::from(title);
    }

    /// The active view's bar carries the accent. Panels are never active.
    pub fn set_active(&mut self, active: bool) {
        self.active = active;
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Puts `button` where the icon was: a view's kind switcher is its icon.
    pub fn set_leading(&mut self, button: TitleBarButton) {
        self.leading = Some(button);
    }

    pub fn leading(&self) -> Option<&TitleBarButton> {
        self.leading.as_ref()
    }

    /// Adds `button` to the slot between the title and the window buttons, in the order added.
    /// With a `priority` below ALWAYS the tool is optional: shown only while it fits with the
    /// title's room left for it, highest priority first - tools of one priority come and go
    /// together - and never counted in the bar's least width.
    pub fn add_tool(&mut self, button: TitleBarButton, priority: i32) {
        self.tools.push(BarTool {
            button,
            priority,
            wanted: true,
            shown: priority == ALWAYS,
        });
    }

    /// Whether the owner wants a tool on the bar at all; an optional tool that is wanted is still
    /// shown only where it fits.
    pub fn set_tool_wanted(&mut self, name: &str, wanted: bool) {
        for tool in &mut self.tools {
            if tool.button.name == name {
                tool.wanted = wanted;
                if tool.priority == ALWAYS {
                    tool.shown = wanted;
                }
            }
        }
    }

    /// The tool `name`, to change what it shows (a Link checked or waiting).
    pub fn tool_mut(&mut self, name: &str) -> Option<&mut TitleBarButton> {
        self.tools
            .iter_mut()
            .find(|tool| tool.button.name == name)
            .map(|tool| &mut tool.button)
    }

    /// The tools shown after the last fit, by name.
    pub fn shown_tools(&self) -> Vec<String> {
        self.tools
            .iter()
            .filter(|tool| tool.shown)
            .map(|tool| tool.button.name.clone())
            .collect()
    }

    /// The window buttons this bar shows, in their order.
    pub fn window_buttons(&self) -> Vec<WindowButton> {
        let mut buttons = vec![WindowButton::Minimise];
        if self.floatable {
            buttons.push(WindowButton::Float);
        }
        if self.role == DockRole::View {
            buttons.push(WindowButton::Maximise);
        }
        if self.closable {
            buttons.push(WindowButton::Close);
        }
        buttons
    }

    fn lead_width(&self) -> f32 {
        self.leading
            .as_ref()
            .map_or(ICON_CELL.x, |button| button.width)
    }

    /// The least width the always-shown tools, the window buttons and the title's least need; an
    /// optional tool is never in it, so it never stops a view being narrowed.
    pub fn minimum_width(&self) -> f32 {
        let tools: f32 = self
            .tools
            .iter()
            .filter(|tool| tool.priority == ALWAYS && tool.wanted)
            .map(|tool| tool.button.width + SPACING)
            .sum();
        let buttons = self.window_buttons().len() as f32 * (BUTTON_SIZE + SPACING);
        LEFT_MARGIN
            + self.lead_width()
            + ICON_GAP
            + SPACING
            + TITLE_LEAST
            + tools
            + BUTTONS_GAP
            + buttons
            + RIGHT_MARGIN
    }

    /// fitOptionalTools: at `width`, with the title `title_width` wide, the optional tools that fit
    /// shown, highest priority first, and never a lower one past a higher one that did not fit -
    /// the tools a person reaches for most go last.
    pub fn fit(&mut self, width: f32, title_width: f32) {
        let title_room = (title_width + TITLE_GAP).clamp(TITLE_LEAST, TITLE_LEAST.max(TITLE_ROOM));
        let mut room = width - self.minimum_width() - (title_room - TITLE_LEAST);
        let mut priorities: Vec<i32> = Vec::new();
        for tool in &self.tools {
            if tool.priority != ALWAYS && !priorities.contains(&tool.priority) {
                priorities.push(tool.priority);
            }
        }
        priorities.sort_unstable();
        priorities.reverse();
        let mut fits = true;
        for priority in priorities {
            let needed: f32 = self
                .tools
                .iter()
                .filter(|tool| tool.priority == priority && tool.wanted)
                .map(|tool| tool.button.width + SPACING)
                .sum();
            fits = fits && needed <= room;
            if fits {
                room -= needed;
            }
            for tool in &mut self.tools {
                if tool.priority == priority {
                    tool.shown = fits && tool.wanted;
                }
            }
        }
    }

    /// Where the bar's parts go in `rect` (the bar's QHBoxLayout: margins 6 and 3, the icon's
    /// cell or the leading control, 4 and the spacing, the title, the tools, 6 and the spacing,
    /// the window buttons), every button on the device pixel grid at `pixels_per_point`.
    pub fn layout(&self, rect: Rect, pixels_per_point: f32) -> BarLayout {
        let top = snap(rect.min.y + BAR_PADDING, pixels_per_point);
        let at = |left: f32, width: f32| {
            Rect::from_min_size(
                Pos2::new(snap(left, pixels_per_point), top),
                Vec2::new(width, BUTTON_SIZE),
            )
        };
        let mut right = rect.max.x - RIGHT_MARGIN;
        let mut buttons = Vec::new();
        for which in self.window_buttons().into_iter().rev() {
            buttons.push((which, at(right - BUTTON_SIZE, BUTTON_SIZE)));
            right -= BUTTON_SIZE + SPACING;
        }
        buttons.reverse();
        right -= BUTTONS_GAP;
        let mut tools = Vec::new();
        for (index, tool) in self.tools.iter().enumerate().rev() {
            if !tool.shown {
                continue;
            }
            tools.push((index, at(right - tool.button.width, tool.button.width)));
            right -= tool.button.width + SPACING;
        }
        tools.reverse();
        let lead = at(rect.min.x + LEFT_MARGIN, self.lead_width());
        let title_left = lead.max.x + ICON_GAP + SPACING;
        let title = Rect::from_min_max(
            Pos2::new(title_left, rect.min.y),
            Pos2::new(right.max(title_left), rect.max.y),
        );
        BarLayout {
            lead,
            title,
            tools,
            buttons,
        }
    }

    /// The button `which` as it is for the dock's state now: the Float button is Dock when
    /// floating, Maximise is Restore when maximised, and each says what it does to a panel or a
    /// view.
    pub fn window_button(&self, which: WindowButton, facts: BarFacts) -> TitleBarButton {
        let what = if self.role == DockRole::View {
            "view"
        } else {
            "panel"
        };
        match which {
            WindowButton::Minimise => TitleBarButton::new(
                which.name(),
                "Minimise",
                "Minimise",
                &format!(
                    "Hide this {what} to the bar at the bottom of the window; its button there puts it back where it was."
                ),
            ),
            WindowButton::Float if facts.floating => TitleBarButton::new(
                which.name(),
                "Dock",
                "Dock",
                &format!(
                    "Put this {what} back where it was docked in the window. Double-clicking the title bar does the same."
                ),
            ),
            WindowButton::Float => TitleBarButton::new(
                which.name(),
                "Float",
                "Float",
                &format!(
                    "Take this {what} out into a window of its own, which can go on another screen. Double-clicking the title bar does the same; drag the title bar back over the window to dock it anywhere."
                ),
            ),
            WindowButton::Maximise if facts.maximised => TitleBarButton::new(
                which.name(),
                "Restore",
                "Restore",
                if facts.floating {
                    "Put this window back to the size and place it had."
                } else {
                    "Bring back the other views, at the sizes they had."
                },
            ),
            WindowButton::Maximise => TitleBarButton::new(
                which.name(),
                "Maximise",
                "Maximise",
                if facts.floating {
                    "Fill the screen this window is on."
                } else {
                    "Give this view the whole drawing area, hiding the other docked views until Restore."
                },
            ),
            WindowButton::Close => TitleBarButton::new(
                which.name(),
                "Close",
                "Close",
                if self.role == DockRole::View {
                    "Close this view. Its zoom and the layers it hides go with it; the drawing is not changed."
                } else {
                    "Hide this panel. View > Panels brings it back."
                },
            )
            .close_style(),
        }
    }

    /// The bar drawn in `rect` for dock `dock`: what its presses asked.
    pub fn ui(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        dock: &str,
        facts: BarFacts,
        icons: &mut ChromeIcons,
    ) -> Vec<ChromeRequest> {
        let mut requests = Vec::new();
        // Panels are always readable; views are muted unless active, so the one the menus act on
        // stands out without the others fading away.
        let marked = self.role == DockRole::View && self.active;
        let painter = ui.painter().clone();
        painter.rect_filled(
            rect,
            CornerRadius::ZERO,
            if marked {
                palette::panel()
            } else {
                palette::window()
            }
            .to_egui(),
        );
        painter.rect_filled(
            Rect::from_min_max(Pos2::new(rect.min.x, rect.max.y - 1.0), rect.max),
            CornerRadius::ZERO,
            palette::border().to_egui(),
        );
        if marked {
            painter.rect_filled(
                Rect::from_min_size(rect.min, Vec2::new(rect.width(), ACTIVE_STROKE)),
                CornerRadius::ZERO,
                palette::accent().to_egui(),
            );
        }

        // A press anywhere on the bar that is not a button; first, so the buttons are over it.
        let bar = ui.interact(
            rect,
            ui.id().with(("katanaDockTitleBar", dock)),
            Sense::click(),
        );
        if bar.contains_pointer() && ui.input(|input| input.pointer.primary_pressed()) {
            requests.push(ChromeRequest::Pressed(String::from(dock)));
        }
        if bar.double_clicked() && self.floatable {
            requests.push(ChromeRequest::Float(String::from(dock)));
        }

        let font = egui::TextStyle::Body.resolve(ui.style());
        let colour = if self.role == DockRole::Panel || self.active {
            palette::text()
        } else {
            palette::text_muted()
        }
        .to_egui();
        let title_width = painter
            .layout_no_wrap(self.title.clone(), font, colour)
            .size()
            .x;
        self.fit(rect.width(), title_width);
        let pixels_per_point = ui.ctx().pixels_per_point();
        let layout = self.layout(rect, pixels_per_point);

        match &self.leading {
            Some(leading) => {
                let lead = leading.clone();
                if title_bar_button(ui, layout.lead, &lead, icons).clicked() {
                    requests.push(ChromeRequest::Tool(String::from(dock), lead.name));
                }
            }
            None => {
                let pixels = (DOCK_ICON_SIZE * pixels_per_point).round() as u32;
                if let Some(texture) = icons.texture(ui.ctx(), &self.icon, pixels) {
                    let side = pixels as f32 / pixels_per_point;
                    let at = Pos2::new(
                        snap(
                            layout.lead.min.x + ((ICON_CELL.x - DOCK_ICON_SIZE) / 2.0).floor(),
                            pixels_per_point,
                        ),
                        snap(
                            layout.lead.min.y + ((ICON_CELL.y - DOCK_ICON_SIZE) / 2.0).floor(),
                            pixels_per_point,
                        ),
                    );
                    painter.image(
                        texture,
                        Rect::from_min_size(at, Vec2::splat(side)),
                        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }
            }
        }

        // The title, elided to the room the bar leaves it, in the colour of the bar's state.
        if layout.title.width() > 0.0 {
            let title = self.title.clone();
            ui.scope_builder(
                UiBuilder::new()
                    .max_rect(layout.title)
                    .layout(Layout::left_to_right(Align::Center)),
                |ui| {
                    ui.add(
                        egui::Label::new(RichText::new(title).color(colour))
                            .truncate()
                            .selectable(false),
                    );
                },
            );
        }

        for (index, at) in &layout.tools {
            let tool = self.tools[*index].button.clone();
            if title_bar_button(ui, *at, &tool, icons).clicked() {
                requests.push(ChromeRequest::Tool(String::from(dock), tool.name));
            }
        }
        for (which, at) in &layout.buttons {
            let button = self.window_button(*which, facts);
            if title_bar_button(ui, *at, &button, icons).clicked() {
                let dock = String::from(dock);
                requests.push(match which {
                    WindowButton::Minimise => ChromeRequest::Minimise(dock),
                    WindowButton::Float => ChromeRequest::Float(dock),
                    WindowButton::Maximise => ChromeRequest::Maximise(dock),
                    WindowButton::Close => ChromeRequest::Close(dock),
                });
            }
        }
        requests
    }
}

/// What the chrome needs of a tab of the window's dock area: its dock's object name.
pub trait DockTab: Clone {
    fn dock_name(&self) -> &str;
}

#[derive(Clone, Debug)]
struct Installed {
    name: String,
    bar: DockTitleBar,
}

/// A minimised dock: its tab, and where it was.
#[derive(Clone, Debug)]
struct Minimised<Tab> {
    tab: Tab,
    floating: bool,
    /// Floating: its window's rectangle.
    window: Rect,
    /// The layout as it was, the dock in it.
    layout: DockState<Tab>,
}

/// A maximised view.
#[derive(Clone, Debug)]
struct Maximised<Tab> {
    name: String,
    floating: bool,
    /// Floating: the rectangle to go back to.
    window: Rect,
    /// Docked: the views hidden for it, and the layout before.
    hidden: Vec<Tab>,
    layout: Option<DockState<Tab>>,
}

/// A dock floated by its button: the layout it left, given back when the same puts it back.
#[derive(Clone, Debug)]
struct Floated<Tab> {
    name: String,
    layout: DockState<Tab>,
}

/// The tab of dock `name`.
fn find<Tab: DockTab>(state: &DockState<Tab>, name: &str) -> Option<TabPath> {
    state.find_tab_from(|tab| tab.dock_name() == name)
}

/// The names of the tabs open in `state`.
fn names<Tab: DockTab>(state: &DockState<Tab>) -> BTreeSet<String> {
    state
        .iter_all_tabs()
        .map(|(_, tab)| String::from(tab.dock_name()))
        .collect()
}

/// `returning` put back where `layout` had them: the layout whole, every tab as it is now, when the
/// tabs it holds are the tabs open now and the returning; else each returning tab where there is
/// room - in a window of its own at `window` when it was floating, else the first place there is.
fn put_back<Tab: DockTab>(
    state: &mut DockState<Tab>,
    layout: &DockState<Tab>,
    returning: Vec<Tab>,
    window: Option<Rect>,
) {
    let now: Vec<Tab> = state.iter_all_tabs().map(|(_, tab)| tab.clone()).collect();
    let mut wanted: BTreeSet<String> = now
        .iter()
        .map(|tab| String::from(tab.dock_name()))
        .collect();
    let back: Vec<String> = returning
        .iter()
        .map(|tab| String::from(tab.dock_name()))
        .collect();
    wanted.extend(back.iter().cloned());
    if names(layout) == wanted {
        let mut by_name: HashMap<String, Tab> = now
            .into_iter()
            .chain(returning)
            .map(|tab| (String::from(tab.dock_name()), tab))
            .collect();
        *state = layout.filter_map_tabs(|tab| by_name.remove(tab.dock_name()));
    } else {
        for tab in returning {
            match window.filter(|rect| rect.is_finite() && rect.is_positive()) {
                Some(rect) => {
                    let surface = state.add_window(vec![tab]);
                    if let Some(window) = state.get_window_state_mut(surface) {
                        window.set_position(rect.min);
                        window.set_size(rect.size());
                    }
                }
                None => state.push_to_first_leaf(tab),
            }
        }
    }
    // For a tab page, raising it is what makes it the current tab again.
    for name in back {
        if let Some(path) = find(state, &name) {
            let _ = state.set_active_tab(path);
        }
    }
}

/// The chrome's bookkeeping for one window (DockChrome; see the module comment).
pub struct DockChrome<Tab> {
    installed: Vec<Installed>,
    minimised: Vec<Minimised<Tab>>,
    maximised: Vec<Maximised<Tab>>,
    floated: Vec<Floated<Tab>>,
    /// The docks floating at the last refresh: what their bars show.
    floating: BTreeSet<String>,
    requests: Vec<ChromeRequest>,
    changed: bool,
    icons: ChromeIcons,
}

impl<Tab: DockTab> DockChrome<Tab> {
    pub fn new(icons: ChromeIcons) -> Self {
        Self {
            installed: Vec::new(),
            minimised: Vec::new(),
            maximised: Vec::new(),
            floated: Vec::new(),
            floating: BTreeSet::new(),
            requests: Vec::new(),
            changed: false,
            icons,
        }
    }

    /// The icons, to draw a bar or a button outside a dock.
    pub fn icons(&mut self) -> &mut ChromeIcons {
        &mut self.icons
    }

    /// Puts dock `name` under the chrome with a bar of its own, `icon` on its bar and its tray
    /// button. A dock installed twice keeps its first bar, which is returned.
    pub fn install(&mut self, name: &str, icon: &str, role: DockRole) -> &mut DockTitleBar {
        let at = match self.installed.iter().position(|entry| entry.name == name) {
            Some(at) => at,
            None => {
                self.installed.push(Installed {
                    name: String::from(name),
                    bar: DockTitleBar::new(icon, role),
                });
                self.installed.len() - 1
            }
        };
        &mut self.installed[at].bar
    }

    pub fn title_bar(&self, name: &str) -> Option<&DockTitleBar> {
        self.installed
            .iter()
            .find(|entry| entry.name == name)
            .map(|entry| &entry.bar)
    }

    pub fn title_bar_mut(&mut self, name: &str) -> Option<&mut DockTitleBar> {
        self.installed
            .iter_mut()
            .find(|entry| entry.name == name)
            .map(|entry| &mut entry.bar)
    }

    fn role(&self, name: &str) -> Option<DockRole> {
        self.title_bar(name).map(DockTitleBar::role)
    }

    /// Whether the set of minimised docks or a maximise changed since this was last asked (the
    /// C++'s onChanged, for a menu that lists them).
    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    /// What the bars asked since this was last asked.
    pub fn take_requests(&mut self) -> Vec<ChromeRequest> {
        std::mem::take(&mut self.requests)
    }

    // ---- the docks' state, before the dock area is drawn -------------------------------------

    /// Reads `state` before the dock area is drawn, and ends what something else ended, as the
    /// C++'s event filter did: a minimised dock whose tab is open again (View > Panels showed it)
    /// is not minimised any more; a dock floated by its button and docked by a drag keeps no record
    /// of where it was; a view a maximise hid, open again, ends that maximise; a maximise of a
    /// docked view that floated, or of a floating one that docked, is over.
    pub fn refresh(&mut self, state: &mut DockState<Tab>) {
        let open = names(state);
        let before = self.minimised.len();
        self.minimised
            .retain(|record| !open.contains(record.tab.dock_name()));
        if self.minimised.len() != before {
            self.changed = true;
        }
        self.floating = state
            .iter_all_tabs()
            .filter(|(path, _)| !path.surface.is_main())
            .map(|(_, tab)| String::from(tab.dock_name()))
            .collect();
        let floating = self.floating.clone();
        self.floated
            .retain(|record| floating.contains(&record.name) || !open.contains(&record.name));
        let mut ended = Vec::new();
        for record in &self.maximised {
            let crossed =
                open.contains(&record.name) && floating.contains(&record.name) != record.floating;
            let shown_again = record
                .hidden
                .iter()
                .any(|tab| open.contains(tab.dock_name()));
            if crossed || shown_again {
                ended.push(record.name.clone());
            }
        }
        for name in ended {
            self.unmaximise(state, &name);
        }
    }

    /// The bar's facts for dock `name`.
    pub fn facts(&self, name: &str) -> BarFacts {
        BarFacts {
            floating: self.floating.contains(name),
            maximised: self.is_maximised(name),
        }
    }

    /// One tab of the dock area, drawn by the window's `TabViewer::ui`: the dock `name` as a drive
    /// target (TargetKind::Dock), its bar at the top and `content` under it. What the bar's buttons
    /// ask is kept for `take_requests` (or `apply`) once the dock area has been drawn. A dock not
    /// installed is drawn with no bar.
    pub fn dock_ui<R>(&mut self, ui: &mut Ui, name: &str, content: impl FnOnce(&mut Ui) -> R) -> R {
        let rect = ui.available_rect_before_wrap();
        let title = self
            .title_bar(name)
            .map_or_else(String::new, |bar| String::from(bar.title()));
        begin_target(ui, TargetSpec::new(name, TargetKind::Dock, &title));
        let facts = self.facts(name);
        if let Some(at) = self.installed.iter().position(|entry| entry.name == name) {
            let bar_rect = Rect::from_min_size(rect.min, Vec2::new(rect.width(), BAR_HEIGHT));
            ui.allocate_rect(bar_rect, Sense::hover());
            let requests = self.installed[at]
                .bar
                .ui(ui, bar_rect, name, facts, &mut self.icons);
            self.requests.extend(requests);
        }
        let inner = content(ui);
        end_target(ui, rect);
        inner
    }

    /// Applies one request a bar or the tray made to `state` - minimise, float, maximise, restore -
    /// and hands back what is its owner's: a press on the bar, a close, a tool. `screen` is the
    /// room a floating view maximised fills.
    pub fn apply(
        &mut self,
        state: &mut DockState<Tab>,
        request: ChromeRequest,
        screen: Rect,
    ) -> Option<ChromeRequest> {
        match request {
            ChromeRequest::Minimise(name) => self.minimise(state, &name),
            ChromeRequest::Float(name) => self.toggle_floating(state, &name),
            ChromeRequest::Maximise(name) => self.toggle_maximised(state, &name, screen),
            ChromeRequest::Restore(name) => {
                self.restore(state, &name);
            }
            other => return Some(other),
        }
        None
    }

    // ---- minimise --------------------------------------------------------------------------

    /// Hides dock `name` and puts a button for it on the tray. A maximised view is restored
    /// first, so the views it hid come back. Nothing for a dock not installed here, or not open.
    pub fn minimise(&mut self, state: &mut DockState<Tab>, name: &str) {
        if self.role(name).is_none() || self.is_minimised(name) {
            return;
        }
        self.unmaximise(state, name);
        let Some(path) = find(state, name) else {
            return;
        };
        let floating = !path.surface.is_main();
        let window = if floating {
            state
                .get_window_state_mut(path.surface)
                .map_or(Rect::NOTHING, |window| window.rect())
        } else {
            Rect::NOTHING
        };
        let layout = state.clone();
        let Some(tab) = state.remove_tab(path) else {
            return;
        };
        self.minimised.push(Minimised {
            tab,
            floating,
            window,
            layout,
        });
        self.changed = true;
    }

    /// Puts a minimised dock back where it was, the current tab of its group; false when it is not
    /// minimised. A docked view maximised in the window meanwhile is restored first: the dock comes
    /// back beside it, so the maximise is over.
    pub fn restore(&mut self, state: &mut DockState<Tab>, name: &str) -> bool {
        let Some(at) = self
            .minimised
            .iter()
            .position(|record| record.tab.dock_name() == name)
        else {
            return false;
        };
        let record = self.minimised.remove(at);
        if !record.floating {
            self.unmaximise_docked(state);
        }
        let window = record.floating.then_some(record.window);
        put_back(state, &record.layout, vec![record.tab], window);
        self.changed = true;
        true
    }

    pub fn is_minimised(&self, name: &str) -> bool {
        self.minimised
            .iter()
            .any(|record| record.tab.dock_name() == name)
    }

    /// The minimised docks' object names, in the order they were minimised, which is the tray's:
    /// for the saved layout.
    pub fn minimised_names(&self) -> Vec<String> {
        self.minimised
            .iter()
            .map(|record| String::from(record.tab.dock_name()))
            .collect()
    }

    /// Minimises the docks `names` name, after a saved layout is restored; the names no installed
    /// dock carries are returned rather than dropped, so the caller can say so.
    pub fn minimise_named(&mut self, state: &mut DockState<Tab>, names: &[String]) -> Vec<String> {
        let mut missing = Vec::new();
        for name in names {
            if self.role(name).is_none() {
                missing.push(name.clone());
                continue;
            }
            self.minimise(state, name);
        }
        missing
    }

    /// The tray's buttons as actions: `Restore<dock>`, the dock's title and icon, which a step
    /// triggers by name as a click does (the window lists them among its actions).
    pub fn tray_actions(&self) -> Vec<Action> {
        self.minimised
            .iter()
            .map(|record| {
                let name = record.tab.dock_name();
                let bar = self.title_bar(name);
                let title = bar.map_or("", DockTitleBar::title);
                Action::new(&format!("Restore{name}"), title)
                    .tip(&format!("<b>{title}</b><br>Put it back where it was."))
            })
            .collect()
    }

    /// The tray - a toolbar named MinimisedToolBar along the bottom of the window, "Minimised:"
    /// and a button for each minimised dock - drawn while anything is minimised; a press of a
    /// button restores its dock. Its icons are left to the window's toolbar (the actions carry no
    /// icon name yet: `tray_actions`).
    pub fn tray_ui(&mut self, ui: &mut Ui, state: &mut DockState<Tab>) {
        if self.minimised.is_empty() {
            return;
        }
        let actions = self.tray_actions();
        let mut chosen = None;
        let shown = ui.horizontal(|ui| {
            begin_target(
                ui,
                TargetSpec::new("MinimisedToolBar", TargetKind::ToolBar, "Minimised"),
            );
            crate::drive::controls::label_as(
                ui,
                "MinimisedLabel",
                "Minimised:",
                egui::Label::new(
                    RichText::new("Minimised:").color(palette::text_muted().to_egui()),
                ),
            );
            for action in &actions {
                if crate::drive::action::action_item(ui, action).clicked() {
                    chosen = action.name.strip_prefix("Restore").map(String::from);
                }
            }
        });
        end_target(ui, shown.response.rect);
        if let Some(name) = chosen {
            self.restore(state, &name);
        }
    }

    // ---- float -----------------------------------------------------------------------------

    /// Floats a docked dock where it stands, or docks a floating one back where it came from, in
    /// the layout it left.
    pub fn toggle_floating(&mut self, state: &mut DockState<Tab>, name: &str) {
        let floatable = self.title_bar(name).is_some_and(|bar| bar.floatable);
        if !floatable {
            return;
        }
        self.unmaximise(state, name);
        let Some(path) = find(state, name) else {
            return;
        };
        if path.surface.is_main() {
            self.floated.retain(|record| record.name != name);
            self.floated.push(Floated {
                name: String::from(name),
                layout: state.clone(),
            });
            let rect = state
                .leaf(path.node_path())
                .map(|leaf| leaf.rect)
                .ok()
                .filter(|rect| rect.is_finite() && rect.is_positive())
                .unwrap_or_else(|| {
                    Rect::from_min_size(Pos2::new(40.0, 40.0), Vec2::new(400.0, 300.0))
                });
            state.detach_tab(path, rect);
        } else {
            let Some(tab) = state.remove_tab(path) else {
                return;
            };
            match self.floated.iter().position(|record| record.name == name) {
                Some(at) => {
                    let record = self.floated.remove(at);
                    put_back(state, &record.layout, vec![tab], None);
                }
                None => state.push_to_first_leaf(tab),
            }
        }
        self.changed = true;
    }

    // ---- maximise (views) --------------------------------------------------------------------

    /// A docked view: every other docked view is hidden until unmaximise, so this one has the whole
    /// drawing area. A floating view: it fills `screen`. One docked view at a time.
    pub fn maximise(&mut self, state: &mut DockState<Tab>, name: &str, screen: Rect) {
        if self.role(name) != Some(DockRole::View) || self.is_maximised(name) {
            return;
        }
        let Some(path) = find(state, name) else {
            return;
        };
        if !path.surface.is_main() {
            let window = state
                .get_window_state_mut(path.surface)
                .map_or(Rect::NOTHING, |window| {
                    let before = window.rect();
                    window.set_position(screen.min);
                    window.set_size(screen.size());
                    before
                });
            self.maximised.push(Maximised {
                name: String::from(name),
                floating: true,
                window,
                hidden: Vec::new(),
                layout: None,
            });
        } else {
            self.unmaximise_docked(state);
            let views: BTreeSet<&str> = self
                .installed
                .iter()
                .filter(|entry| entry.bar.role() == DockRole::View && entry.name != name)
                .map(|entry| entry.name.as_str())
                .collect();
            let layout = state.clone();
            let hidden: Vec<Tab> = state
                .iter_all_tabs()
                .filter(|(path, tab)| path.surface.is_main() && views.contains(tab.dock_name()))
                .map(|(_, tab)| tab.clone())
                .collect();
            let hiding: BTreeSet<String> = hidden
                .iter()
                .map(|tab| String::from(tab.dock_name()))
                .collect();
            state.retain_tabs(|tab| !hiding.contains(tab.dock_name()));
            self.maximised.push(Maximised {
                name: String::from(name),
                floating: false,
                window: Rect::NOTHING,
                hidden,
                layout: Some(layout),
            });
        }
        self.changed = true;
    }

    /// Undoes `maximise(name)`: the hidden views come back where they were, a floating view goes
    /// back to its rectangle. False when it is not maximised.
    pub fn unmaximise(&mut self, state: &mut DockState<Tab>, name: &str) -> bool {
        let Some(at) = self.maximised.iter().position(|record| record.name == name) else {
            return false;
        };
        let record = self.maximised.remove(at);
        if record.floating {
            if let Some(path) = find(state, name)
                && !path.surface.is_main()
                && record.window.is_finite()
                && record.window.is_positive()
                && let Some(window) = state.get_window_state_mut(path.surface)
            {
                window.set_position(record.window.min);
                window.set_size(record.window.size());
            }
        } else if let Some(layout) = &record.layout {
            let returning: Vec<Tab> = record
                .hidden
                .into_iter()
                .filter(|tab| !self.is_minimised(tab.dock_name()))
                .collect();
            put_back(state, layout, returning, None);
        }
        self.changed = true;
        true
    }

    pub fn toggle_maximised(&mut self, state: &mut DockState<Tab>, name: &str, screen: Rect) {
        if !self.unmaximise(state, name) {
            self.maximise(state, name, screen);
        }
    }

    /// Undoes every docked maximise: before views are opened or arranged, since a maximise is a
    /// passing state, not a layout.
    pub fn unmaximise_docked(&mut self, state: &mut DockState<Tab>) {
        let docked: Vec<String> = self
            .maximised
            .iter()
            .filter(|record| !record.floating)
            .map(|record| record.name.clone())
            .collect();
        for name in docked {
            self.unmaximise(state, &name);
        }
    }

    /// Undoes every maximise: before a layout is saved.
    pub fn unmaximise_all(&mut self, state: &mut DockState<Tab>) {
        let all: Vec<String> = self
            .maximised
            .iter()
            .map(|record| record.name.clone())
            .collect();
        for name in all {
            self.unmaximise(state, &name);
        }
    }

    pub fn is_maximised(&self, name: &str) -> bool {
        self.maximised.iter().any(|record| record.name == name)
    }

    /// Drops every trace of dock `name`, putting back what it changed: its tray button goes, and
    /// when it was the maximised view the views it hid come back. A workspace calls it before it
    /// closes a view.
    pub fn forget(&mut self, state: &mut DockState<Tab>, name: &str) {
        self.minimised
            .retain(|record| record.tab.dock_name() != name);
        self.unmaximise(state, name);
        for record in &mut self.maximised {
            record.hidden.retain(|tab| tab.dock_name() != name);
        }
        self.floated.retain(|record| record.name != name);
        self.changed = true;
    }
}
