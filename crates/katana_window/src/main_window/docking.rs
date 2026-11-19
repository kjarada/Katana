//! Where the toolbars and the torn-off menus are: the model behind docking a toolbar along any side
//! of the window or floating it, and tearing a menu off into a panel of its own (the owner's ask of
//! 2026-11-19, "make toolbars and menus resizable and dockable"). Pure data and arithmetic, so the
//! window, the drive harness and the tests move a bar by the same functions; main_window::panels
//! draws what this says, and main_window::window runs the actions that change it.
//!
//! THE PLACES. Every toolbar but the Drafting bar has a `BarPlace`: a LINE of one of the four
//! sides - a row along the top or the bottom, a column down the left or the right - or floating at
//! a point of the window with a width its buttons wrap in. A side's lines are numbered from the
//! window's edge inwards (the top's row 0 is under the menu bar, the bottom's row 0 is over the
//! Drafting bar, the left's column 0 at the window's left edge), so a new line next to the views is
//! always the side's next number. The order of the window's list of places is the order of the bars
//! along a line. The Drafting bar stays where it is: its toggles are words, which a 39 point column
//! cannot hold, and it is the drawing's state rather than a set of commands. The Properties bar,
//! one choice box broader than a column, docks along the top or the bottom or floats, never down a
//! side (`ROW_ONLY_BARS`).
//!
//! ONE DOOR. A drag dropped, a View > Toolbars item, a bar's right-click menu, a `*` step and a
//! test all end in `drop_bar` on the window's list (`apply_drop` here), which is normalised after
//! every move: a side's line numbers are made 0, 1, 2 ... again, so an emptied line closes up.
//!
//! THE TORN-OFF MENUS. A menu torn off is a `MenuPanel`: the menu's object name, and docked to a
//! side of the views or floating at a rectangle. The panels docked to one side share a band
//! between the views and that side's panels, whose breadth the person drags; a floating one is
//! moved by its title bar and sized by its edges.

use egui::{Pos2, Rect, Vec2};
use katana_core::error::make_error;
use katana_core::{ErrorCode, Status};

use super::actions::BarPlace;

/// A side of the window a toolbar or a torn-off menu docks along.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Side {
    Top,
    Left,
    Right,
    Bottom,
}

impl Side {
    pub const ALL: [Self; 4] = [Self::Top, Self::Left, Self::Right, Self::Bottom];

    /// Its word in a saved layout, a `*` step and a report.
    pub const fn word(self) -> &'static str {
        match self {
            Self::Top => "top",
            Self::Left => "left",
            Self::Right => "right",
            Self::Bottom => "bottom",
        }
    }

    /// Its word in an action's object name and a menu item.
    pub const fn title(self) -> &'static str {
        match self {
            Self::Top => "Top",
            Self::Left => "Left",
            Self::Right => "Right",
            Self::Bottom => "Bottom",
        }
    }

    /// The side `word` names, in any case.
    pub fn from_word(word: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|side| side.word().eq_ignore_ascii_case(word))
    }

    /// Whether a bar docked here runs down the window: its orientation follows its side.
    pub const fn vertical(self) -> bool {
        matches!(self, Self::Left | Self::Right)
    }

    /// Its index in `ALL`, for the arrays kept by side.
    pub const fn index(self) -> usize {
        match self {
            Self::Top => 0,
            Self::Left => 1,
            Self::Right => 2,
            Self::Bottom => 3,
        }
    }
}

/// Where a floating toolbar is: its top left corner in the window's points, and the width its
/// buttons wrap in. Its height follows from the width (`flyout_button::wrap_strip`), so a
/// floating bar is sized by its width alone.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FloatAt {
    pub x: f32,
    pub y: f32,
    pub width: f32,
}

impl FloatAt {
    pub fn min(&self) -> Pos2 {
        Pos2::new(self.x, self.y)
    }
}

/// A floating toolbar's own title strip, over its buttons: its grip and its name, what it is
/// dragged by.
pub const FLOAT_HEADER: f32 = 18.0;
/// The width a toolbar floated by a menu item or an action is given at most: a palette of some
/// nine buttons a row, which a person widens by its edge.
pub const FLOAT_WIDTH: f32 = 320.0;
/// How far from the bars of a side a drop still docks there, as a new line next to them.
pub const NEW_LINE_ZONE: f32 = 10.0;

/// The window's places of its toolbars, in their order along their lines.
pub type Places = Vec<(String, BarPlace)>;

impl BarPlace {
    /// A line of `side`.
    pub const fn on(side: Side, line: usize) -> Self {
        match side {
            Side::Top => Self::Top(line),
            Side::Left => Self::Left(line),
            Side::Right => Self::Right(line),
            Side::Bottom => Self::Bottom(line),
        }
    }

    /// The side and line of a docked bar; None for a floating one, the Drafting bar or none.
    pub const fn docked(&self) -> Option<(Side, usize)> {
        match *self {
            Self::Top(line) => Some((Side::Top, line)),
            Self::Left(line) => Some((Side::Left, line)),
            Self::Right(line) => Some((Side::Right, line)),
            Self::Bottom(line) => Some((Side::Bottom, line)),
            Self::Floating(_) | Self::Drafting | Self::Nowhere => None,
        }
    }

    /// Whether the bar runs down the window.
    pub fn vertical(&self) -> bool {
        self.docked().is_some_and(|(side, _)| side.vertical())
    }

    /// The place as a saved layout and a report write it: "top:1", "left:0", "float:400,300,320".
    pub fn text(&self) -> String {
        match self {
            Self::Floating(at) => format!(
                "float:{},{},{}",
                at.x.round() as i32,
                at.y.round() as i32,
                at.width.round() as i32
            ),
            Self::Drafting => String::from("drafting"),
            Self::Nowhere => String::from("nowhere"),
            docked => match docked.docked() {
                Some((side, line)) => format!("{}:{line}", side.word()),
                None => String::new(),
            },
        }
    }

    /// The place `text` writes (`text`); None for anything else.
    pub fn from_text(text: &str) -> Option<Self> {
        let (word, rest) = text.split_once(':')?;
        if word == "float" {
            let numbers: Vec<f32> = rest
                .split(',')
                .map(|part| part.trim().parse::<i32>().ok().map(|n| n as f32))
                .collect::<Option<Vec<f32>>>()?;
            let [x, y, width] = numbers[..] else {
                return None;
            };
            return Some(Self::Floating(FloatAt { x, y, width }));
        }
        let side = Side::from_word(word)?;
        Some(Self::on(side, rest.trim().parse().ok()?))
    }
}

/// Makes each side's line numbers 0, 1, 2 ... in their order, so a line emptied by a move closes
/// up and a saved layout holds no gaps.
pub fn normalise(places: &mut Places) {
    for side in Side::ALL {
        let mut lines: Vec<usize> = places
            .iter()
            .filter_map(|(_, place)| place.docked())
            .filter(|(at, _)| *at == side)
            .map(|(_, line)| line)
            .collect();
        lines.sort_unstable();
        lines.dedup();
        for (_, place) in places.iter_mut() {
            if let Some((at, line)) = place.docked()
                && at == side
                && let Ok(rank) = lines.binary_search(&line)
            {
                *place = BarPlace::on(side, rank);
            }
        }
    }
}

/// The toolbars that never dock down a side: the Properties bar is one choice box, the current
/// style's, some 214 points broad. Down the left it made a column that broad for one control a
/// row high, an empty 218 x 625 point column beside the panels (the seventh look audit). So it
/// docks along the top or the bottom, or floats; a side is refused with `side_refused`'s reason,
/// its Dock Left and Dock Right items are greyed (actions::tool_bar_place_menu), a drag to a side
/// floats it (`drop_spot`), and a saved layout that had it down a side leaves it where the window
/// builds it (layout::restore_layout).
///
/// Rejected: drawing it down a side as a short row across the column's top, which would be a
/// fourth shape of docked bar for one bar, and still a column as broad as its box below it.
/// The Surface Edit bar has a choice box too, but a column of tools under it that a side shows.
pub const ROW_ONLY_BARS: [&str; 1] = ["PropertiesToolBar"];

/// Whether toolbar `bar` (its object name) docks down the left and the right.
pub fn docks_down_a_side(bar: &str) -> bool {
    !ROW_ONLY_BARS.contains(&bar)
}

/// The refusal of a docking `action` that would put a bar of ROW_ONLY_BARS down a side, its
/// object name found by `named` from the name or key the action gives; Ok for any other action.
pub fn side_refused(action: &DockingAction, named: impl Fn(&str) -> Option<String>) -> Status {
    let (key, side) = match action {
        DockingAction::Dock { key, side } => (key, *side),
        DockingAction::Place { bar, side, .. } => (bar, *side),
        _ => return Ok(()),
    };
    let Some(side) = side.filter(|side| side.vertical()) else {
        return Ok(());
    };
    match named(key) {
        Some(bar) if !docks_down_a_side(&bar) => Err(make_error(
            ErrorCode::Unsupported,
            format!(
                "this toolbar docks along the top or the bottom, or floats; not down the {}: \
                 its box is broader than a column",
                side.word()
            ),
            bar,
        )),
        _ => Ok(()),
    }
}

/// Where a dragged toolbar would go if it were let go now.
#[derive(Clone, Debug, PartialEq)]
pub enum DropSpot {
    /// Onto line `line` of `side`, before the bar `before` (None: at the line's end).
    Line {
        side: Side,
        line: usize,
        before: Option<String>,
    },
    /// Onto a new line of `side`, numbered `before`: the lines from `before` on move one out.
    NewLine { side: Side, before: usize },
    /// Floating, at a place of its own.
    Float(FloatAt),
}

/// `name` moved to `spot` in `places`: taken out of where it was, put where `spot` says, and the
/// lines normalised. False, nothing changed, for a bar `places` does not hold.
pub fn apply_drop(places: &mut Places, name: &str, spot: &DropSpot) -> bool {
    let Some(was) = places.iter().position(|(bar, _)| bar == name) else {
        return false;
    };
    let (bar, _) = places.remove(was);
    match spot {
        DropSpot::Line { side, line, before } => {
            let place = BarPlace::on(*side, *line);
            let at = before
                .as_ref()
                .and_then(|before| {
                    places
                        .iter()
                        .position(|(other, at)| other == before && *at == place)
                })
                .or_else(|| {
                    places
                        .iter()
                        .rposition(|(_, at)| *at == place)
                        .map(|last| last + 1)
                })
                .unwrap_or(places.len());
            places.insert(at, (bar, place));
        }
        DropSpot::NewLine { side, before } => {
            for (_, place) in places.iter_mut() {
                if let Some((at, line)) = place.docked()
                    && at == *side
                    && line >= *before
                {
                    *place = BarPlace::on(*side, line + 1);
                }
            }
            places.insert(was.min(places.len()), (bar, BarPlace::on(*side, *before)));
        }
        DropSpot::Float(at) => {
            places.insert(was.min(places.len()), (bar, BarPlace::Floating(*at)));
        }
    }
    normalise(places);
    true
}

/// The bars of line `line` of `side` in `places`, in their order.
pub fn line_members(places: &Places, side: Side, line: usize) -> Vec<&str> {
    places
        .iter()
        .filter(|(_, place)| place.docked() == Some((side, line)))
        .map(|(bar, _)| bar.as_str())
        .collect()
}

/// The highest line of `side` in `places`, if it has one.
pub fn last_line(places: &Places, side: Side) -> Option<usize> {
    places
        .iter()
        .filter_map(|(_, place)| place.docked())
        .filter(|(at, _)| *at == side)
        .map(|(_, line)| line)
        .max()
}

/// The precise place a `viewToolBarPlace:` action names: line `line` of `side` at `index` among
/// its bars - an existing line, else a new one after the last - or floating.
pub fn spot_at(places: &Places, name: &str, side: Side, line: usize, index: usize) -> DropSpot {
    match last_line(places, side) {
        Some(last) if line <= last => {
            let members: Vec<&str> = line_members(places, side, line)
                .into_iter()
                .filter(|bar| *bar != name)
                .collect();
            DropSpot::Line {
                side,
                line,
                before: members.get(index).map(|bar| String::from(*bar)),
            }
        }
        last => DropSpot::NewLine {
            side,
            before: last.map_or(0, |last| last + 1),
        },
    }
}

/// What a drop can land on, from the last frame: each shown line's band, each docked bar as it was
/// drawn, and the strips beside each side's bars that open a new line there.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DropZones {
    pub lines: Vec<(Side, usize, Rect)>,
    pub bars: Vec<(String, Side, usize, Rect)>,
    pub new_lines: Vec<(Side, usize, Rect)>,
}

/// Where a bar dragged to `pointer` lands: on the line under the pointer, before the first of its
/// bars whose middle is past the pointer along the line; else on a new line, in the strip beside
/// a side's bars; else floating, its grip under the pointer (`grab` is the pointer's offset from
/// the bar's corner) at `width`. A bar that does not dock down a side (`ROW_ONLY_BARS`) dragged
/// onto one floats there instead.
pub fn drop_spot(zones: &DropZones, dragged: &str, pointer: Pos2, grab: Vec2, width: f32) -> DropSpot {
    let takes = |side: &Side| !side.vertical() || docks_down_a_side(dragged);
    for (side, line, rect) in &zones.lines {
        if !rect.contains(pointer) || !takes(side) {
            continue;
        }
        let along = |at: Pos2| if side.vertical() { at.y } else { at.x };
        let mut members: Vec<&(String, Side, usize, Rect)> = zones
            .bars
            .iter()
            .filter(|(bar, at, at_line, _)| bar != dragged && at == side && at_line == line)
            .collect();
        members.sort_by(|a, b| along(a.3.center()).total_cmp(&along(b.3.center())));
        let before = members
            .iter()
            .find(|(_, _, _, rect)| along(rect.center()) > along(pointer))
            .map(|(bar, ..)| bar.clone());
        return DropSpot::Line {
            side: *side,
            line: *line,
            before,
        };
    }
    for (side, before, rect) in &zones.new_lines {
        if rect.contains(pointer) && takes(side) {
            return DropSpot::NewLine {
                side: *side,
                before: *before,
            };
        }
    }
    let corner = pointer - grab;
    DropSpot::Float(FloatAt {
        x: corner.x.round(),
        y: corner.y.round(),
        width,
    })
}

/// The rectangle a drop at `spot` is shown by while dragging (an accent outline): the line it
/// joins, the strip of a new line, or the floating bar's own rectangle `float_size` big.
pub fn spot_outline(zones: &DropZones, spot: &DropSpot, float_size: Vec2) -> Option<Rect> {
    match spot {
        DropSpot::Line { side, line, .. } => zones
            .lines
            .iter()
            .find(|(at, at_line, _)| at == side && at_line == line)
            .map(|(_, _, rect)| *rect),
        DropSpot::NewLine { side, before } => zones
            .new_lines
            .iter()
            .find(|(at, at_before, _)| at == side && at_before == before)
            .map(|(_, _, rect)| *rect),
        DropSpot::Float(at) => Some(Rect::from_min_size(at.min(), float_size)),
    }
}

// ---- the torn-off menus -----------------------------------------------------------------------

/// Where a torn-off menu's panel is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PanelPlace {
    Docked(Side),
    Floating(Rect),
}

/// A menu torn off into a panel: the menu's object name and where its panel is.
#[derive(Clone, Debug, PartialEq)]
pub struct MenuPanel {
    pub menu: String,
    pub place: PanelPlace,
    /// The side it was last docked to: where its Float button docks it again.
    pub last_side: Side,
}

/// A torn-off menu's band's breadth before the person drags it: the side panels' 240 at the left
/// and right, and some six items' height along the top and bottom.
pub const DEFAULT_BANDS: [f32; 4] = [168.0, 240.0, 240.0, 168.0];
/// The least and most a band is dragged to.
pub const LEAST_BAND: f32 = 96.0;
pub const MOST_BAND: f32 = 640.0;
/// A floating menu panel's size when it is torn off.
pub const PANEL_SIZE: Vec2 = Vec2::new(260.0, 420.0);
/// The least a floating menu panel is sized to: its title bar's buttons and a few items.
pub const LEAST_PANEL: Vec2 = Vec2::new(160.0, 96.0);
/// How near a side of the views a torn-off menu dragged by its title bar docks there.
pub const PANEL_EDGE_ZONE: f32 = 48.0;

/// The torn-off menus and their bands.
#[derive(Clone, Debug, PartialEq)]
pub struct MenuPanels {
    pub open: Vec<MenuPanel>,
    /// The panels their Minimise hid to the tray, in the order they went, each keeping its place
    /// for when its tray button puts it back. Kept apart from `open`, so that nothing drawing,
    /// docking or sizing the bands has to remember to pass over them.
    pub minimised: Vec<MenuPanel>,
    /// Each side's band breadth, by `Side::index`.
    pub bands: [f32; 4],
}

impl Default for MenuPanels {
    fn default() -> Self {
        Self {
            open: Vec::new(),
            minimised: Vec::new(),
            bands: DEFAULT_BANDS,
        }
    }
}

impl MenuPanels {
    /// The breadth of each side's band that holds a docked panel, by `Side::index`.
    pub fn band_breadths(&self) -> [Option<f32>; 4] {
        let mut breadths = [None; 4];
        for panel in &self.open {
            if let PanelPlace::Docked(side) = panel.place {
                breadths[side.index()] = Some(self.bands[side.index()]);
            }
        }
        breadths
    }

    pub fn find(&self, menu: &str) -> Option<&MenuPanel> {
        self.open.iter().find(|panel| panel.menu == menu)
    }

    pub fn find_mut(&mut self, menu: &str) -> Option<&mut MenuPanel> {
        self.open.iter_mut().find(|panel| panel.menu == menu)
    }

    /// The panels docked to `side`, in their order.
    pub fn docked_to(&self, side: Side) -> Vec<&MenuPanel> {
        self.open
            .iter()
            .filter(|panel| panel.place == PanelPlace::Docked(side))
            .collect()
    }

    /// `menu` torn off floating at `rect`; a menu already torn off is left where it is.
    pub fn tear_off(&mut self, menu: &str, rect: Rect) {
        // Torn off again while minimised, it comes back where it was rather than twice.
        if self.restore(menu) {
            return;
        }
        if self.find(menu).is_none() {
            self.open.push(MenuPanel {
                menu: String::from(menu),
                place: PanelPlace::Floating(rect),
                last_side: Side::Right,
            });
        }
    }

    /// `menu`'s panel put at `place`. False for a menu not torn off.
    pub fn place(&mut self, menu: &str, place: PanelPlace) -> bool {
        let Some(panel) = self.find_mut(menu) else {
            return false;
        };
        if let PanelPlace::Docked(side) = place {
            panel.last_side = side;
        }
        panel.place = place;
        true
    }

    /// `menu`'s panel closed. False for a menu not torn off.
    pub fn close(&mut self, menu: &str) -> bool {
        let before = self.open.len() + self.minimised.len();
        self.open.retain(|panel| panel.menu != menu);
        self.minimised.retain(|panel| panel.menu != menu);
        self.open.len() + self.minimised.len() != before
    }

    /// `menu`'s panel hidden to the tray, keeping its place. False for a menu not open.
    pub fn minimise(&mut self, menu: &str) -> bool {
        let Some(at) = self.open.iter().position(|panel| panel.menu == menu) else {
            return false;
        };
        let panel = self.open.remove(at);
        self.minimised.push(panel);
        true
    }

    /// `menu`'s minimised panel put back where it was. False for a menu not minimised.
    pub fn restore(&mut self, menu: &str) -> bool {
        let Some(at) = self.minimised.iter().position(|panel| panel.menu == menu) else {
            return false;
        };
        let panel = self.minimised.remove(at);
        self.open.push(panel);
        true
    }

    /// `side`'s band dragged to `breadth`, kept between LEAST_BAND and MOST_BAND.
    pub fn set_band(&mut self, side: Side, breadth: f32) {
        self.bands[side.index()] = breadth.clamp(LEAST_BAND, MOST_BAND);
    }
}

/// What an item of a torn-off menu is to the flow of its columns (`flow_columns`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlowItem {
    /// An action or a submenu.
    Item,
    /// A titled section: it begins a group, and is never the last of a column.
    Section,
    /// A rule: it begins a group, and is left out where it would begin a column.
    Separator,
}

/// A torn-off menu's items, of `kinds` and `heights`, flowed into columns `room` high with
/// `spacing` between the items of a column: each column filled to the room, then the next. A
/// group - a section's title or a rule and the items up to the next - starts a new column when it
/// does not fit under the items before it but fits a column of its own, so a section is not
/// split where it need not be; a group taller than a column runs on from where the last ended. A
/// section's title is never the last of a column, and a rule never the first (it parts nothing
/// there, and is left out). Every item lies wholly inside the room, unless one alone is taller.
///
/// For a menu docked along the top or the bottom (`MainWindow::menu_panels_ui`): drawn as one
/// column, as down a side, a band of some six items' height showed four of Draw's forty and left
/// the rest of the window's width empty (the sixth look audit).
pub fn flow_columns(kinds: &[FlowItem], heights: &[f32], room: f32, spacing: f32) -> Vec<Vec<usize>> {
    let height = |index: usize| heights.get(index).copied().unwrap_or(0.0);
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for (index, kind) in kinds.iter().enumerate() {
        if *kind != FlowItem::Item || groups.is_empty() {
            groups.push(Vec::new());
        }
        if let Some(group) = groups.last_mut() {
            group.push(index);
        }
    }
    let stacked = |items: &[usize]| -> f32 {
        items.iter().map(|&index| height(index)).sum::<f32>()
            + spacing * items.len().saturating_sub(1) as f32
    };
    let fits = |used: f32, column: &[usize], extent: f32| {
        let gap = if column.is_empty() { 0.0 } else { spacing };
        used + gap + extent <= room
    };
    let mut columns: Vec<Vec<usize>> = vec![Vec::new()];
    let mut used = 0.0;
    for group in groups {
        let body = match group.first() {
            Some(&first) if kinds[first] == FlowItem::Separator => &group[1..],
            _ => &group[..],
        };
        let started = columns
            .last()
            .is_some_and(|column| !column.is_empty() && !fits(used, column, stacked(&group)));
        if started && stacked(body) <= room {
            columns.push(Vec::new());
            used = 0.0;
        }
        for (at, &index) in group.iter().enumerate() {
            let kind = kinds[index];
            // A title needs its first item under it in the same column.
            let extent = match (kind, group.get(at + 1)) {
                (FlowItem::Section, Some(&next)) => height(index) + spacing + height(next),
                _ => height(index),
            };
            let full = columns
                .last()
                .is_some_and(|column| !column.is_empty() && !fits(used, column, extent));
            if full {
                columns.push(Vec::new());
                used = 0.0;
            }
            let Some(column) = columns.last_mut() else {
                continue;
            };
            if kind == FlowItem::Separator && column.is_empty() {
                continue;
            }
            used += if column.is_empty() { 0.0 } else { spacing } + height(index);
            column.push(index);
        }
    }
    columns.retain(|column| !column.is_empty());
    columns
}

/// The dock object name of `menu`'s torn-off panel: its drive target and its chrome's.
pub fn panel_dock_name(menu: &str) -> String {
    format!("{menu}TornOff")
}

/// The menu whose torn-off panel `dock` is.
pub fn panel_menu(dock: &str) -> Option<&str> {
    dock.strip_suffix("TornOff").filter(|menu| !menu.is_empty())
}

/// A panel's place as a saved layout writes it: "left", "float:500,200,260,400".
pub fn panel_place_text(place: &PanelPlace) -> String {
    match place {
        PanelPlace::Docked(side) => String::from(side.word()),
        PanelPlace::Floating(rect) => format!(
            "float:{},{},{},{}",
            rect.min.x.round() as i32,
            rect.min.y.round() as i32,
            rect.width().round() as i32,
            rect.height().round() as i32
        ),
    }
}

/// The place `panel_place_text` writes; None for anything else.
pub fn panel_place_from_text(text: &str) -> Option<PanelPlace> {
    if let Some(rest) = text.strip_prefix("float:") {
        let numbers: Vec<f32> = rest
            .split(',')
            .map(|part| part.trim().parse::<i32>().ok().map(|n| n as f32))
            .collect::<Option<Vec<f32>>>()?;
        let [x, y, width, height] = numbers[..] else {
            return None;
        };
        if width <= 0.0 || height <= 0.0 {
            return None;
        }
        return Some(PanelPlace::Floating(Rect::from_min_size(
            Pos2::new(x, y),
            Vec2::new(width, height),
        )));
    }
    Side::from_word(text).map(PanelPlace::Docked)
}

/// The side of `views` a torn-off menu dragged to `pointer` docks to: the nearest edge within
/// PANEL_EDGE_ZONE of it; None to float.
pub fn panel_drop_side(views: Rect, pointer: Pos2) -> Option<Side> {
    if !views.expand(PANEL_EDGE_ZONE).contains(pointer) {
        return None;
    }
    let distances = [
        (Side::Top, (pointer.y - views.min.y).abs()),
        (Side::Left, (pointer.x - views.min.x).abs()),
        (Side::Right, (views.max.x - pointer.x).abs()),
        (Side::Bottom, (views.max.y - pointer.y).abs()),
    ];
    distances
        .into_iter()
        .filter(|(_, distance)| *distance <= PANEL_EDGE_ZONE)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(side, _)| side)
}

// ---- the actions --------------------------------------------------------------------------------

/// The prefix of a toolbar's dock actions (`viewToolBarDock<Bar><Side>`, `<Side>` one of Top,
/// Left, Right, Bottom or Float), of the precise placing (`viewToolBarPlace:<bar>:<side>:<line>:
/// <index>` or `viewToolBarPlace:<bar>:float:<x>,<y>,<width>`), of tearing a menu off
/// (`viewMenuTearOff<Menu>`), of a torn-off menu's places (`viewMenuPanelDock<Menu><Side>`,
/// `viewMenuPanelClose<Menu>`, `viewMenuPanelPlace:<menu>:<place>`) and of a band's breadth
/// (`viewMenuBand:<side>:<breadth>`).
pub const DOCK_ACTION: &str = "viewToolBarDock";
pub const PLACE_ACTION: &str = "viewToolBarPlace:";
pub const RESET_ACTION: &str = "viewToolBarsReset";
pub const TEAR_OFF_ACTION: &str = "viewMenuTearOff";
pub const PANEL_DOCK_ACTION: &str = "viewMenuPanelDock";
pub const PANEL_CLOSE_ACTION: &str = "viewMenuPanelClose";
pub const PANEL_PLACE_ACTION: &str = "viewMenuPanelPlace:";
pub const BAND_ACTION: &str = "viewMenuBand:";

/// What a docking action asks, read from its object name.
#[derive(Clone, Debug, PartialEq)]
pub enum DockingAction {
    /// A toolbar, by the key its toggle is named by (its title without spaces), docked to a side
    /// (None: floated).
    Dock { key: String, side: Option<Side> },
    /// A toolbar, by its object name or key, at a precise place.
    Place {
        bar: String,
        side: Option<Side>,
        line: usize,
        index: usize,
        float: Option<FloatAt>,
    },
    Reset,
    TearOff { menu: String },
    PanelDock { menu: String, side: Option<Side> },
    PanelClose { menu: String },
    PanelPlace { menu: String, place: PanelPlace },
    Band { side: Side, breadth: f32 },
}

/// The name of `bar`'s dock action to `side` (None: Float), from its key (its title without
/// spaces, as its View > Toolbars toggle is named).
pub fn dock_action_name(key: &str, side: Option<Side>) -> String {
    format!("{DOCK_ACTION}{key}{}", side.map_or("Float", Side::title))
}

/// A menu's object name as it is written in an action's name: its first letter upper case.
pub fn menu_key(menu: &str) -> String {
    let mut characters = menu.chars();
    match characters.next() {
        Some(first) => first.to_uppercase().chain(characters).collect(),
        None => String::new(),
    }
}

/// The menu's object name a key of `menu_key` came from: its first letter lower case.
pub fn menu_from_key(key: &str) -> String {
    let mut characters = key.chars();
    match characters.next() {
        Some(first) => first.to_lowercase().chain(characters).collect(),
        None => String::new(),
    }
}

/// `text` without the side's title at its end, and the side: Float is None.
fn split_side(text: &str) -> Option<(&str, Option<Side>)> {
    if let Some(rest) = text.strip_suffix("Float") {
        return Some((rest, None));
    }
    Side::ALL.into_iter().find_map(|side| {
        text.strip_suffix(side.title())
            .map(|rest| (rest, Some(side)))
    })
}

/// The docking action `name` is; None for any other name.
pub fn parse_action(name: &str) -> Option<DockingAction> {
    if name == RESET_ACTION {
        return Some(DockingAction::Reset);
    }
    if let Some(rest) = name.strip_prefix(PLACE_ACTION) {
        let parts: Vec<&str> = rest.split(':').collect();
        return match parts[..] {
            [bar, "float", at] => {
                let BarPlace::Floating(float) = BarPlace::from_text(&format!("float:{at}"))?
                else {
                    return None;
                };
                Some(DockingAction::Place {
                    bar: String::from(bar),
                    side: None,
                    line: 0,
                    index: 0,
                    float: Some(float),
                })
            }
            [bar, side, line, index] => Some(DockingAction::Place {
                bar: String::from(bar),
                side: Some(Side::from_word(side)?),
                line: line.parse().ok()?,
                index: index.parse().ok()?,
                float: None,
            }),
            [bar, side, line] => Some(DockingAction::Place {
                bar: String::from(bar),
                side: Some(Side::from_word(side)?),
                line: line.parse().ok()?,
                index: usize::MAX,
                float: None,
            }),
            _ => None,
        };
    }
    if let Some(rest) = name.strip_prefix(PANEL_PLACE_ACTION) {
        let (menu, place) = rest.split_once(':')?;
        return Some(DockingAction::PanelPlace {
            menu: String::from(menu),
            place: panel_place_from_text(place)?,
        });
    }
    if let Some(rest) = name.strip_prefix(BAND_ACTION) {
        let (side, breadth) = rest.split_once(':')?;
        return Some(DockingAction::Band {
            side: Side::from_word(side)?,
            breadth: breadth.parse::<i32>().ok()? as f32,
        });
    }
    if let Some(rest) = name.strip_prefix(DOCK_ACTION) {
        let (key, side) = split_side(rest)?;
        return (!key.is_empty()).then(|| DockingAction::Dock {
            key: String::from(key),
            side,
        });
    }
    if let Some(rest) = name.strip_prefix(TEAR_OFF_ACTION) {
        return (!rest.is_empty()).then(|| DockingAction::TearOff {
            menu: menu_from_key(rest),
        });
    }
    if let Some(rest) = name.strip_prefix(PANEL_DOCK_ACTION) {
        let (key, side) = split_side(rest)?;
        return (!key.is_empty()).then(|| DockingAction::PanelDock {
            menu: menu_from_key(key),
            side,
        });
    }
    if let Some(rest) = name.strip_prefix(PANEL_CLOSE_ACTION) {
        return (!rest.is_empty()).then(|| DockingAction::PanelClose {
            menu: menu_from_key(rest),
        });
    }
    None
}

/// What the window keeps of its docking between frames: the torn-off menus (saved with the
/// layout), and what the last frame drew - each docked bar's rectangle, the zones a drop lands on,
/// the views' area - with a drag in progress.
#[derive(Clone, Debug)]
pub struct DockingState {
    pub panels: MenuPanels,
    /// The window's rectangle at the last frame: what Dock Top fits a bar into.
    pub screen: Rect,
    /// The views' area at the last frame, the torn-off menus' bands included: what a torn-off
    /// menu docks to the sides of, and where a floated bar or menu is first put.
    pub views: Rect,
    /// The views alone at the last frame, the bands left out.
    pub drawing: Rect,
    pub zones: DropZones,
    /// Each toolbar drawn at the last frame, docked or floating, by object name.
    pub drawn: Vec<(String, Rect)>,
    /// The toolbar being dragged by its grip: its name, and the pointer's offset from its corner.
    pub bar_drag: Option<(String, Vec2)>,
    /// A torn-off menu pressed on its title bar: its menu, where the press was, its rectangle
    /// then, and whether the pointer has moved far enough to be a drag.
    pub panel_drag: Option<PanelDrag>,
    /// Each toolbar's last docked place, where a double click on its floating title docks it.
    pub docked_before: Vec<(String, BarPlace)>,
    /// Where the pointer was at the last frame: where a torn-off menu's title bar was pressed.
    pub pointer: Option<Pos2>,
}

/// A torn-off menu's title bar held down (see `DockingState::panel_drag`).
#[derive(Clone, Debug, PartialEq)]
pub struct PanelDrag {
    pub menu: String,
    pub from: Pos2,
    pub rect: Rect,
    pub moved: bool,
}

/// How far the pointer moves with a title bar held before it is a drag, not a click.
pub const DRAG_THRESHOLD: f32 = 6.0;

impl Default for DockingState {
    fn default() -> Self {
        let screen = Rect::from_min_size(
            Pos2::ZERO,
            Vec2::new(
                crate::drive::harness::HEADLESS_WIDTH,
                crate::drive::harness::HEADLESS_HEIGHT,
            ),
        );
        // Until a frame is drawn, the window as built at the headless size: a bar floated or a
        // menu torn off before the first frame goes over the views, not the menu bar.
        let built = super::layout::geometry(
            screen,
            &super::layout::Docks::default(),
            false,
            &super::layout::BarLines::default(),
            [None; 4],
        );
        Self {
            panels: MenuPanels::default(),
            screen,
            views: built.views_and_bands,
            drawing: built.views,
            zones: DropZones::default(),
            drawn: Vec::new(),
            bar_drag: None,
            panel_drag: None,
            docked_before: Vec::new(),
            pointer: None,
        }
    }
}

impl DockingState {
    /// Toolbar `name`'s rectangle as the last frame drew it.
    pub fn drawn_rect(&self, name: &str) -> Option<Rect> {
        self.drawn
            .iter()
            .find(|(bar, _)| bar == name)
            .map(|(_, rect)| *rect)
    }
}
