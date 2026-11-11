//! The registry: what each frame drew, control by control and target by target, by the names the
//! C++ objects had.
//!
//! In Qt a control is an object that exists between frames, found by `findChild` on its object
//! name; in egui a control exists only while a frame draws it. So every named control
//! (drive::controls) records itself here as it is drawn, and a driver - the step engine of a
//! headless run, a test's harness, an assistive tool through AccessKit's author id - reads the
//! last completed frame's records. One door, as `findChild` and `--fill` were one door to the
//! same object names.
//!
//! THE RECORD. A control's record is its name, its kind, the target it was drawn in and the part
//! of that target (a dialog's scrolling body or its footer), what it shows (its text, its value,
//! its items, whether it is checked), whether it was enabled and visible, and where it was: its
//! rectangle and the clip rectangle it was drawn in, which for a control in a scrolling body is
//! the body's viewport, so "reachable" is a rectangle inside another.
//!
//! TARGETS. A target is what the C++ steps make the place their fills and presses go - a dialog, a
//! dock, a panel, a toolbar, a popup, a menu - and what `--screenshot` grabs. The window itself is
//! a target (`WINDOW`), always drawn. A target records its rectangle in points, the whole of it
//! with its frame; the screenshot is that rectangle times the frame's pixels per point, its
//! corners rounded outward to whole pixels and kept inside the screen (D crops by it).
//!
//! A NAME USED TWICE in one target in one frame cannot be told apart by a driver; the registry
//! keeps the first and says so (`Snapshot::duplicates`), and a driver refuses the frame.
//!
//! WHEN A FRAME COUNTS. egui may lay a frame out twice (a pass it discards) and draws a new area
//! once invisibly to measure it (a sizing pass). Neither is what a person sees: a control drawn in
//! either records nothing and takes no act, and a target first drawn in a sizing pass is recorded
//! as not settled, so an act addressed to it waits for the next frame (drive::mailbox).
//!
//! The registry lives in an egui plugin (`Drive`), so a control reaches it through its `Ui` alone,
//! and a pass's records are reset as the pass begins. Never hold the plugin's lock across a call
//! into egui: a control takes its act, unlocks, draws, and then records.

use egui::{Context, Rect, Response, Ui};

use super::mailbox::{ActId, ActOutcome, Mailbox, Posted, Taken};

/// The main window's own target: what a step with no dialog, dock or panel opened acts on. The
/// C++ main window has no object name, and neither has this.
pub const WINDOW: &str = "";

/// What a control is, for how a driver fills, presses and reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ControlKind {
    /// QPushButton, QToolButton.
    Button,
    /// A checkable button (QToolButton with setCheckable): a chip, a toolbar toggle.
    Toggle,
    /// QCheckBox.
    Check,
    /// QRadioButton.
    Radio,
    /// QLineEdit.
    Line,
    /// QPlainTextEdit, QTextEdit.
    Text,
    /// QComboBox.
    Choice,
    /// QDoubleSpinBox.
    Spin,
    /// QSpinBox.
    Whole,
    /// A list, a table or a tree: whole rows (QAbstractItemView).
    Rows,
    /// QTabWidget's tabs.
    Tabs,
    /// QLabel.
    Label,
    /// A menu's item: an action row drawn in a menu.
    MenuItem,
    /// A view (a plan, 3D or section view), reported by the window itself.
    View,
    /// Anything else a module records for its own reasons.
    Other,
}

/// Which part of a dialog's frame a control was drawn in (shell::dialog).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Part {
    /// Not in a dialog's frame.
    #[default]
    None,
    /// The body, which scrolls when the dialog does not fit its screen.
    Body,
    /// The footer - the dialog's button row and what it says under it - below the scroll area,
    /// always in sight.
    Footer,
}

/// One control as a frame drew it.
#[derive(Clone, Debug, PartialEq)]
pub struct ControlRecord {
    /// The C++ object name.
    pub name: String,
    pub kind: ControlKind,
    /// The target it was drawn in (`WINDOW` for the window's own controls).
    pub target: String,
    pub part: Part,
    /// What it shows as its own words: a button's, a check box's, a label's text. For a button
    /// drawn with an icon alone, the name a screen reader gives it.
    pub text: String,
    /// What it holds: a field's text, a choice's current text, a box's number as it shows it, the
    /// current tab's title, a list's current row's first cell, "on" or "off" for a check box.
    pub value: String,
    /// A choice's items, a list's rows (each row's cells joined by " | "), a tab bar's titles.
    pub items: Vec<String>,
    /// For what can be checked: whether it is.
    pub checked: Option<bool>,
    pub enabled: bool,
    pub visible: bool,
    /// Where it was drawn, in points.
    pub rect: Rect,
    /// The clip rectangle it was drawn in: a scroll area's viewport when it is in one.
    pub clip: Rect,
    /// egui's id for it (an AccessKit node's).
    pub id: egui::Id,
}

impl ControlRecord {
    /// A record of `kind` named `name`, showing nothing yet, enabled and visible.
    pub fn new(name: &str, kind: ControlKind) -> Self {
        Self {
            name: String::from(name),
            kind,
            target: String::from(WINDOW),
            part: Part::None,
            text: String::new(),
            value: String::new(),
            items: Vec::new(),
            checked: None,
            enabled: true,
            visible: true,
            rect: Rect::NOTHING,
            clip: Rect::NOTHING,
            id: egui::Id::NULL,
        }
    }

    /// What `--report NAME` says of it (main.cpp's reportWidget), before its whitespace is
    /// simplified: a label's text as written; a field's or a text box's text; a list's rows, their
    /// cells joined by " | " and the rows by " ; "; a button's (a check box's, a radio button's, a
    /// menu item's) text without its '&', and ", checked" or ", unchecked" when it can be checked.
    /// None for a choice, a number box and a tab bar, which reportWidget reports no more than Qt
    /// does, and for a view, which the window reports itself.
    pub fn report(&self) -> Option<String> {
        match self.kind {
            ControlKind::Label => Some(self.text.clone()),
            ControlKind::Line | ControlKind::Text => Some(self.value.clone()),
            ControlKind::Rows => Some(self.items.join(" ; ")),
            ControlKind::Button
            | ControlKind::Toggle
            | ControlKind::Check
            | ControlKind::Radio
            | ControlKind::MenuItem => {
                let mut text = self.text.replace('&', "");
                match self.checked {
                    Some(true) => text.push_str(", checked"),
                    Some(false) => text.push_str(", unchecked"),
                    None => {}
                }
                Some(text)
            }
            ControlKind::Choice
            | ControlKind::Spin
            | ControlKind::Whole
            | ControlKind::Tabs
            | ControlKind::View
            | ControlKind::Other => None,
        }
    }
}

/// QString::simplified: the text with whitespace at its ends removed and each run of it inside
/// one space - how `--report` prints what a control shows.
pub fn simplified(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// What a target is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TargetKind {
    /// The main window.
    Window,
    /// A dialog, in the dialog frame (shell::dialog).
    Dialog,
    /// A dock: a panel or a view's dock.
    Dock,
    /// A panel of the window's own that is not a dock.
    Panel,
    ToolBar,
    /// A popup the steps opened: a view bar's Layers popup.
    Popup,
    /// A menu opened under its title, or a context menu.
    Menu,
}

/// What a target is drawn as, given as it begins.
#[derive(Clone, Debug, PartialEq)]
pub struct TargetSpec {
    /// The C++ object name: the dialog's, the dock's.
    pub name: String,
    pub kind: TargetKind,
    /// Its window title, for "--dialog NAME opened NAME \"title\"".
    pub title: String,
    /// Whether it blocks the rest of the window while it is open (QDialog::isModal).
    pub modal: bool,
}

impl TargetSpec {
    pub fn new(name: &str, kind: TargetKind, title: &str) -> Self {
        Self {
            name: String::from(name),
            kind,
            title: String::from(title),
            modal: false,
        }
    }

    pub fn modal(mut self, modal: bool) -> Self {
        self.modal = modal;
        self
    }
}

/// A scroll area's state as a frame left it: the dialog frame's body.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollRecord {
    /// Where the content shows, on the screen.
    pub viewport: Rect,
    /// How large the content is.
    pub content: egui::Vec2,
    /// How far it is scrolled, right and down.
    pub offset: egui::Vec2,
}

impl ScrollRecord {
    /// How far it can scroll each way (QScrollBar::maximum): what the content has beyond the
    /// viewport, none when it fits.
    pub fn max_offset(&self) -> egui::Vec2 {
        (self.content - self.viewport.size()).max(egui::Vec2::ZERO)
    }

    /// Whether it scrolls at all: content beyond the viewport either way (half a point allowed for
    /// egui's rounding).
    pub fn scrolls(&self) -> bool {
        let beyond = self.max_offset();
        beyond.x > 0.5 || beyond.y > 0.5
    }
}

/// One target as a frame drew it.
#[derive(Clone, Debug, PartialEq)]
pub struct TargetRecord {
    pub name: String,
    pub kind: TargetKind,
    pub title: String,
    pub modal: bool,
    /// The whole of it, its frame and title bar included, in points: what the screenshot crops
    /// to.
    pub rect: Rect,
    /// Drawn as a person sees it, not in egui's measuring pass of a new area.
    pub settled: bool,
    /// The dialog frame's body, when the dialog scrolls.
    pub scroll: Option<ScrollRecord>,
    /// The dialog frame's footer, when it has one kept below the body.
    pub footer: Option<Rect>,
}

/// What one completed frame drew.
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub controls: Vec<ControlRecord>,
    pub targets: Vec<TargetRecord>,
    /// "NAME is drawn twice in TARGET": names a driver could not tell apart.
    pub duplicates: Vec<String>,
    /// The screen, in points, and how many pixels a point is.
    pub screen: Rect,
    pub pixels_per_point: f32,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            controls: Vec::new(),
            targets: Vec::new(),
            duplicates: Vec::new(),
            screen: Rect::NOTHING,
            pixels_per_point: 1.0,
        }
    }
}

impl Snapshot {
    /// The control `name` drawn in `target`.
    pub fn control(&self, target: &str, name: &str) -> Option<&ControlRecord> {
        self.controls
            .iter()
            .find(|control| control.target == target && control.name == name)
    }

    /// The first control `name` drawn in any target (the C++'s `window.findChild`).
    pub fn control_anywhere(&self, name: &str) -> Option<&ControlRecord> {
        self.controls.iter().find(|control| control.name == name)
    }

    pub fn target(&self, name: &str) -> Option<&TargetRecord> {
        self.targets.iter().find(|target| target.name == name)
    }

    /// Every control drawn in `target`, in the order they were drawn.
    pub fn in_target<'a>(&'a self, target: &'a str) -> impl Iterator<Item = &'a ControlRecord> {
        self.controls
            .iter()
            .filter(move |control| control.target == target)
    }

    /// Every control named `name`, in any target: more than one is a name a driver must take
    /// from the right target.
    pub fn named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a ControlRecord> {
        self.controls
            .iter()
            .filter(move |control| control.name == name)
    }
}

/// A target open in the frame being drawn: its record's place and the part being drawn now.
#[derive(Clone, Debug)]
struct Open {
    target: usize,
    part: Part,
}

/// The registry and the mailbox, as an egui plugin. Install it once on a context (`install`).
#[derive(Debug, Default)]
pub struct Drive {
    /// The pass being drawn.
    current: Snapshot,
    /// The last pass completed and not discarded: the frame a driver reads.
    completed: Snapshot,
    open: Vec<Open>,
    mailbox: Mailbox,
}

impl egui::Plugin for Drive {
    fn debug_name(&self) -> &'static str {
        "katana_window::drive"
    }

    fn on_begin_pass(&mut self, ui: &mut Ui) {
        let screen = ui.ctx().content_rect();
        self.current = Snapshot {
            screen,
            pixels_per_point: ui.ctx().pixels_per_point(),
            ..Snapshot::default()
        };
        // The window is always drawn, and is the target of what is drawn outside any other.
        self.current.targets.push(TargetRecord {
            name: String::from(WINDOW),
            kind: TargetKind::Window,
            title: String::new(),
            modal: false,
            rect: screen,
            settled: true,
            scroll: None,
            footer: None,
        });
        self.open.clear();
    }

    fn on_end_pass(&mut self, ui: &mut Ui) {
        // A pass egui will draw again is not the frame: what it recorded is not what is seen.
        if ui.ctx().will_discard() {
            return;
        }
        self.completed = std::mem::take(&mut self.current);
        self.mailbox.end_frame(&self.completed);
    }
}

impl Drive {
    fn current_target(&self) -> (usize, Part) {
        self.open
            .last()
            .map_or((0, Part::None), |open| (open.target, open.part))
    }

    fn target_name(&self, at: usize) -> String {
        self.current
            .targets
            .get(at)
            .map_or_else(|| String::from(WINDOW), |target| target.name.clone())
    }
}

/// Installs the registry on `ctx`; once is enough, and a second call changes nothing. A context
/// without it draws every named control all the same, recording nothing.
pub fn install(ctx: &Context) {
    ctx.add_plugin(Drive::default());
}

/// Whether what `ui` draws now counts (see "WHEN A FRAME COUNTS" above).
pub fn live(ui: &Ui) -> bool {
    !ui.is_sizing_pass() && !ui.ctx().will_discard()
}

/// Opens target `spec` for what is drawn until the matching `end_target`, inside the target's own
/// `Ui` (so a sizing pass is seen). A target begun twice in one frame is a duplicate name.
pub fn begin_target(ui: &Ui, spec: TargetSpec) {
    let settled = !ui.is_sizing_pass();
    let discarded = ui.ctx().will_discard();
    ui.ctx().with_plugin::<Drive, _>(|drive| {
        if discarded {
            // Still opened, so the controls drawn in it know their target.
            drive.open.push(Open {
                target: 0,
                part: Part::None,
            });
            return;
        }
        if drive.current.target(&spec.name).is_some() {
            drive.current.duplicates.push(format!(
                "the target {} is drawn twice in one frame",
                spec.name
            ));
        }
        drive.current.targets.push(TargetRecord {
            name: spec.name,
            kind: spec.kind,
            title: spec.title,
            modal: spec.modal,
            rect: Rect::NOTHING,
            settled,
            scroll: None,
            footer: None,
        });
        let target = drive.current.targets.len() - 1;
        drive.open.push(Open {
            target,
            part: Part::None,
        });
    });
}

/// Closes the target opened last, recording the rectangle it took.
pub fn end_target(ui: &Ui, rect: Rect) {
    ui.ctx().with_plugin::<Drive, _>(|drive| {
        if let Some(open) = drive.open.pop()
            && open.target != 0
            && let Some(target) = drive.current.targets.get_mut(open.target)
        {
            target.rect = rect;
        }
    });
}

/// The dialog frame's body and footer, for the target opened last.
pub fn set_target_frame(ui: &Ui, scroll: Option<ScrollRecord>, footer: Option<Rect>) {
    ui.ctx().with_plugin::<Drive, _>(|drive| {
        if let Some(open) = drive.open.last()
            && open.target != 0
            && let Some(target) = drive.current.targets.get_mut(open.target)
        {
            target.scroll = scroll;
            target.footer = footer;
        }
    });
}

/// What is drawn from now in the target opened last is in `part` of it.
pub fn begin_part(ui: &Ui, part: Part) {
    ui.ctx().with_plugin::<Drive, _>(|drive| {
        if let Some(open) = drive.open.last_mut() {
            open.part = part;
        }
    });
}

/// What is drawn from now is in no part of the target.
pub fn end_part(ui: &Ui) {
    begin_part(ui, Part::None);
}

/// An act a driver posted to the control `name` of the target being drawn, taken from the mailbox
/// for the control to apply and answer (`settle`). None when there is none, and always when the
/// frame does not count.
pub fn take(ui: &Ui, name: &str) -> Option<Taken> {
    if !live(ui) {
        return None;
    }
    ui.ctx()
        .with_plugin::<Drive, _>(|drive| {
            let (at, _) = drive.current_target();
            let target = drive.target_name(at);
            drive.mailbox.take(&target, name)
        })
        .flatten()
}

/// The control's answer to the act it took: done, or refused with the C++'s sentence.
pub fn settle(ui: &Ui, taken: &Taken, answer: std::result::Result<(), String>) {
    ui.ctx()
        .with_plugin::<Drive, _>(|drive| drive.mailbox.settle(taken.id, answer));
}

/// Records `record` for the control `response` answers, in the target and part being drawn, with
/// where it was drawn; sets its AccessKit author id to its name, so an assistive tool sees the name
/// a driver does. Nothing when the frame does not count.
pub fn record(ui: &Ui, response: &Response, mut record: ControlRecord) {
    if !live(ui) {
        return;
    }
    record.rect = response.rect;
    record.clip = ui.clip_rect();
    record.id = response.id;
    let name = record.name.clone();
    ui.ctx().with_plugin::<Drive, _>(|drive| {
        let (at, part) = drive.current_target();
        record.target = drive.target_name(at);
        record.part = part;
        if drive
            .current
            .control(&record.target, &record.name)
            .is_some()
        {
            drive.current.duplicates.push(format!(
                "the name {} is drawn twice in {}: a driver could not tell the two apart",
                record.name,
                if record.target.is_empty() {
                    "the window"
                } else {
                    record.target.as_str()
                }
            ));
            return;
        }
        drive.current.controls.push(record);
    });
    ui.ctx().accesskit_node_builder(response.id, |node| {
        node.set_author_id(name);
    });
}

/// The last completed frame's records (a copy); an empty snapshot when the registry is not
/// installed.
pub fn snapshot(ctx: &Context) -> Snapshot {
    ctx.with_plugin::<Drive, _>(|drive| drive.completed.clone())
        .unwrap_or_default()
}

/// Posts an act to the mailbox; it is taken by the control it names when a frame draws it. None
/// when the registry is not installed.
pub fn post(ctx: &Context, posted: Posted) -> Option<ActId> {
    ctx.with_plugin::<Drive, _>(|drive| drive.mailbox.post(posted))
}

/// What became of act `id`.
pub fn outcome(ctx: &Context, id: ActId) -> ActOutcome {
    ctx.with_plugin::<Drive, _>(|drive| drive.mailbox.outcome(id))
        .unwrap_or(ActOutcome::Waiting)
}

/// How many posted acts are still waiting for a control.
pub fn waiting(ctx: &Context) -> usize {
    ctx.with_plugin::<Drive, _>(|drive| drive.mailbox.waiting())
        .unwrap_or(0)
}
