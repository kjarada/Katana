//! The Surfaces panel (terrain/surfaces_panel.hpp; docs/terrain.md, "The Surfaces panel"): the
//! drawing's surfaces in a table - each shown or hidden, its style, where it came from and how
//! many triangles it has - with Rename, Copy, Remove, Zoom To and Edit Styles above it.
//!
//! The panel changes nothing itself. Every change is a line through the window's one executor
//! (`Host::run`): SURFACE SHOW, SURFACE HIDE, SURFACE RENAME, SURFACE STYLE, SURFACE COPY and
//! SURFACE REMOVE, each one undo step, logged as a typed line is.
//!
//! ```text
//! surfacesPanel:
//!   surfacesRename, surfacesCopy, surfacesRemove, surfacesZoomTo, surfacesEditStyles
//!   surfacesTable    Name (with the shown box) | Style | Source | Triangles
//!   surfaceStyle     the picked row's style choice
//!   surfacesStatus   how many surfaces, or why a line was refused
//! ```
//!
//! THE PANEL FOLLOWS THE DRAWING AT ITS NEXT LOOK. The C++ queued a refresh on the event loop when
//! the document changed, never inside the notification; here the table is read again at the start
//! of the next frame (`refresh`), when the surfaces' or the styles' generation has moved.
//!
//! ONE STYLE CHOICE IS NAMED. Each row of the C++ table held a combo named surfaceStyle; the drive
//! registry refuses a name drawn twice in a target, since a driver could not tell the two apart.
//! So the picked row's choice carries the name, and the other rows' choices are drawn without one.
//! Rejected: a name a row ("surfaceStyle0"), which no C++ object has.

use egui::Ui;
use katana_cad::Document;
use katana_core::text::equals_ignoring_case;

use crate::command_word::command_word;
use crate::drive::action::{Action, action_item};
use crate::drive::controls::custom;
use crate::drive::mailbox::{Act, fill_disabled, lists_no, no_button};
use crate::drive::registry::{ControlKind, ControlRecord, record, settle, take};
use crate::drive::{Choice, choice, label};
use crate::host::Host;
use crate::shell::workbench::DockSpec;

/// What the panel may ask of the window, beside running lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SurfacesPanelServices {
    /// Whether the window's executor is there to run the panel's lines (always, in the window).
    pub runs: bool,
    /// Whether Edit Styles opens the style manager; off, the button is off.
    pub edit_styles: bool,
}

impl Default for SurfacesPanelServices {
    fn default() -> Self {
        Self {
            runs: true,
            edit_styles: true,
        }
    }
}

/// One row of the table, as the drawing had it at the last look.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SurfaceRow {
    /// The name it has in the drawing, which an edit renames.
    pub name: String,
    pub shown: bool,
    /// What the style choice offers: the drawing's default first.
    pub style_texts: Vec<String>,
    /// Each offered style's name as a line writes it; empty for the default.
    pub style_names: Vec<String>,
    pub style: usize,
    pub source: String,
    pub triangles: usize,
}

impl SurfaceRow {
    /// What the style choice shows.
    pub fn style_text(&self) -> &str {
        self.style_texts.get(self.style).map_or("", String::as_str)
    }
}

/// The Surfaces panel (see the module comment).
pub struct SurfacesPanel {
    services: SurfacesPanelServices,
    rows: Vec<SurfaceRow>,
    /// The row picked.
    current: Option<usize>,
    /// The row whose name is being typed (Rename, or a double click), and the text so far.
    renaming: Option<(usize, String)>,
    status: String,
    last_reply: String,
    surfaces_seen: u64,
    styles_seen: u64,
    shown_once: bool,
    /// How often Edit Styles was pressed and not yet taken by the window.
    edit_styles_asked: usize,
}

/// What a frame's clicks ask for, done after the table is drawn.
enum Asked {
    Shown(usize, bool),
    Renamed(usize, String),
    Style(usize, usize),
    Rename,
    Copy,
    Remove,
    ZoomTo,
    EditStyles,
}

impl SurfacesPanel {
    /// The panel on `document`'s surfaces.
    pub fn new(document: &Document, services: SurfacesPanelServices) -> Self {
        let mut panel = Self {
            services,
            rows: Vec::new(),
            current: None,
            renaming: None,
            status: String::new(),
            last_reply: String::new(),
            surfaces_seen: 0,
            styles_seen: 0,
            shown_once: false,
            edit_styles_asked: 0,
        };
        panel.refresh(document);
        panel
    }

    /// Reads the drawing's surfaces and styles again when either has changed since the last look
    /// (their generations); the picked surface stays picked.
    pub fn refresh(&mut self, document: &Document) {
        if self.shown_once
            && self.surfaces_seen == document.surfaces_generation()
            && self.styles_seen == document.surface_styles_generation()
        {
            return;
        }
        self.shown_once = true;
        self.surfaces_seen = document.surfaces_generation();
        self.styles_seen = document.surface_styles_generation();
        let picked = self.picked_surface();
        let surfaces = document.surfaces().all();
        let styles = document.surface_styles();
        self.renaming = None;
        self.rows = surfaces
            .iter()
            .map(|named| {
                // The choice's first entry is the drawing's default, which a surface with no style
                // of its own is in.
                let fallback = styles
                    .find(&styles.default_style)
                    .map_or(styles.default_style.as_str(), |style| style.name.as_str());
                let mut style_texts = vec![format!("Default ({fallback})")];
                let mut style_names = vec![String::new()];
                let mut current = 0;
                for style in &styles.styles {
                    style_texts.push(style.name.clone());
                    style_names.push(style.name.clone());
                    if !named.style.is_empty()
                        && equals_ignoring_case(style.name.as_bytes(), named.style.as_bytes())
                    {
                        current = style_texts.len() - 1;
                    }
                }
                if !named.style.is_empty() && current == 0 {
                    // A style the drawing no longer has (a project's from a newer Katana): said,
                    // and drawn in the default meanwhile.
                    style_texts.push(format!("{} (missing)", named.style));
                    style_names.push(named.style.clone());
                    current = style_texts.len() - 1;
                }
                SurfaceRow {
                    name: named.name.clone(),
                    shown: named.visible,
                    style_texts,
                    style_names,
                    style: current,
                    source: named.source.clone(),
                    triangles: named
                        .surface
                        .as_ref()
                        .map_or(0, |surface| surface.triangle_count()),
                }
            })
            .collect();
        self.current = None;
        if let Some(picked) = picked {
            let _ = self.pick_surface(&picked);
        }
        self.status = if self.rows.is_empty() {
            String::from("No surfaces. Terrain > Surface From makes one; an import may bring some.")
        } else {
            format!("{} surfaces", self.rows.len())
        };
    }

    /// The table's rows, as the drawing had them at the last look.
    pub fn rows(&self) -> &[SurfaceRow] {
        &self.rows
    }

    /// The picked surface's name, if a row is picked.
    pub fn picked_surface(&self) -> Option<String> {
        self.current
            .and_then(|row| self.rows.get(row))
            .map(|row| row.name.clone())
    }

    /// Picks the surface of that name (any case); false when there is none.
    pub fn pick_surface(&mut self, name: &str) -> bool {
        match self
            .rows
            .iter()
            .position(|row| equals_ignoring_case(row.name.as_bytes(), name.as_bytes()))
        {
            Some(row) => {
                self.current = Some(row);
                true
            }
            None => false,
        }
    }

    /// What the last line the panel ran said, or why it was refused.
    pub fn last_reply(&self) -> &str {
        &self.last_reply
    }

    pub fn status(&self) -> &str {
        &self.status
    }

    /// How often Edit Styles was pressed since the window last asked; the window opens the style
    /// manager for it.
    pub fn take_edit_styles_asked(&mut self) -> usize {
        std::mem::take(&mut self.edit_styles_asked)
    }

    /// Runs `line` through the executor; what it said is `last_reply`.
    fn run(&mut self, host: &mut dyn Host, line: &str) {
        if !self.services.runs {
            return;
        }
        let outcome = host.run(line);
        if outcome.ok {
            self.last_reply = outcome.reply;
        } else {
            self.status.clone_from(&outcome.error);
            self.last_reply = outcome.error;
        }
    }

    /// `verb` followed by `names`, each written as a word of the line by the window's one rule
    /// (command_word). A name a line cannot carry sends nothing and is said in the status line and
    /// `last_reply`.
    ///
    /// A double quote or a line break cannot be carried by a quoted word, so it is REFUSED, and no
    /// line is sent. The panel once wrapped a quote in quotes without looking, and the line it
    /// built named another surface: a Remove of PAD "A" removed PAD A. A name that passes is
    /// written quoted, always: that keeps a blank in one word, though not a word such as ALL apart
    /// from a keyword - the tokenizer drops the quotes, which is why SURFACE SHOW|HIDE give a
    /// surface's own name the first claim on it.
    fn run_names(&mut self, host: &mut dyn Host, verb: &str, names: &[&str]) {
        let mut line = String::from(verb);
        for name in names {
            if let Err(error) = command_word(name, "the surface's name") {
                self.last_reply = error.message;
                self.status.clone_from(&self.last_reply);
                return;
            }
            line.push_str(&format!(" \"{name}\""));
        }
        self.run(host, &line);
    }

    /// The name cell of `row` changed: its box ticked or cleared to `ticked`, or its text typed as
    /// `typed`. One cell says two things; whichever differs from the drawing is what was changed.
    fn item_changed(&mut self, host: &mut dyn Host, row: usize, ticked: bool, typed: &str) {
        let Some(name) = self.rows.get(row).map(|row| row.name.clone()) else {
            return;
        };
        let visible = host
            .document()
            .surfaces()
            .find(&name)
            .map(|named| named.visible);
        let typed = typed.trim();
        if visible.is_some_and(|visible| ticked != visible) {
            let verb = if ticked {
                "SURFACE SHOW"
            } else {
                "SURFACE HIDE"
            };
            self.run_names(host, verb, &[name.as_str()]);
        } else if !typed.is_empty() && typed != name {
            self.run_names(host, "SURFACE RENAME", &[name.as_str(), typed]);
        } else {
            // Nothing to do: the cell goes back to what the drawing has.
            return;
        }
        // A refused line leaves the drawing as it was; the row is made again from it, so the cell
        // shows what the drawing has, not what was typed.
        self.shown_once = false;
    }

    /// Ticks or clears the shown box of `row` (its item's setCheckState).
    pub fn set_shown(&mut self, host: &mut dyn Host, row: usize, shown: bool) {
        let name = self.rows.get(row).map(|row| row.name.clone());
        if let Some(name) = name {
            self.item_changed(host, row, shown, &name);
        }
    }

    /// Types `name` into the name cell of `row` (its item's setText).
    pub fn set_name(&mut self, host: &mut dyn Host, row: usize, name: &str) {
        let shown = self.rows.get(row).map(|row| row.shown);
        if let Some(shown) = shown {
            self.item_changed(host, row, shown, name);
        }
    }

    /// The style at `index` of `row`'s choice chosen, as a person's choice in the list does.
    pub fn choose_style(&mut self, host: &mut dyn Host, row: usize, index: usize) {
        let Some(found) = self.rows.get(row) else {
            return;
        };
        let Some(style) = found.style_names.get(index).cloned() else {
            return;
        };
        let name = found.name.clone();
        self.run_names(host, "SURFACE STYLE", &[name.as_str(), style.as_str()]);
    }

    fn copy_picked(&mut self, host: &mut dyn Host) {
        let Some(picked) = self.picked_surface() else {
            return;
        };
        // "ground (2)", as an import names a second of a name.
        let copy = host.document().surfaces().unique_name(picked.as_bytes());
        self.run_names(host, "SURFACE COPY", &[picked.as_str(), copy.as_str()]);
    }

    fn remove_picked(&mut self, host: &mut dyn Host) {
        if let Some(picked) = self.picked_surface() {
            self.run_names(host, "SURFACE REMOVE", &[picked.as_str()]);
        }
    }

    fn zoom_to_picked(&mut self, host: &mut dyn Host) {
        let Some(picked) = self.picked_surface() else {
            return;
        };
        let bounds = host
            .document()
            .surfaces()
            .find(&picked)
            .and_then(|named| named.surface.as_ref())
            .filter(|surface| !surface.empty())
            .map(|surface| *surface.bounds());
        if let Some(bounds) = bounds
            && let Some(views) = host.views()
        {
            views.zoom_to(&bounds);
        }
    }

    /// The row tools above the table. Zoom To is on only where there are views to frame it in.
    fn tools(&mut self, ui: &mut Ui, can_zoom: bool, asked: &mut Vec<Asked>) {
        let picked = self.current.is_some();
        let runs = self.services.runs;
        let tools = [
            (
                Action::new("surfacesRename", "Rename")
                    .icon("Rename")
                    .tip("Rename the picked surface (SURFACE RENAME): type the new name in its row.")
                    .enabled(picked && runs),
                0,
            ),
            (
                Action::new("surfacesCopy", "Copy")
                    .icon("Copy")
                    .tip(
                        "Copy the picked surface under a new name (SURFACE COPY), to edit one and \
                         keep the other.",
                    )
                    .enabled(picked && runs),
                1,
            ),
            (
                Action::new("surfacesRemove", "Remove")
                    .icon("Erase")
                    .tip(
                        "Remove the picked surface from the drawing (SURFACE REMOVE). Undo puts \
                         it back.",
                    )
                    .enabled(picked && runs),
                2,
            ),
            (
                Action::new("surfacesZoomTo", "Zoom To")
                    .icon("ZoomTo")
                    .tip("Frame the picked surface in the plan views.")
                    .enabled(picked && can_zoom),
                3,
            ),
            (
                Action::new("surfacesEditStyles", "Edit Styles")
                    .icon("FormatStyles")
                    .tip(
                        "Make and change the styles a surface is drawn in (Terrain > Surface \
                         Styles).",
                    )
                    .enabled(self.services.edit_styles),
                4,
            ),
        ];
        ui.horizontal(|ui| {
            for (action, which) in &tools {
                if action_item(ui, action).clicked() {
                    asked.push(match which {
                        0 => Asked::Rename,
                        1 => Asked::Copy,
                        2 => Asked::Remove,
                        3 => Asked::ZoomTo,
                        _ => Asked::EditStyles,
                    });
                }
            }
        });
    }

    /// The table: a list named surfacesTable. A fill picks the row of that name, as fillField
    /// finds a row by its first column's text.
    fn table(&mut self, ui: &mut Ui, asked: &mut Vec<Asked>) {
        const NAME: &str = "surfacesTable";
        let taken = take(ui, NAME);
        if let Some(taken) = &taken {
            match &taken.posted.act {
                Act::Fill(text) => {
                    if ui.is_visible() && !ui.is_enabled() {
                        settle(ui, taken, Err(fill_disabled(&taken.posted)));
                    } else if let Some(row) = self.rows.iter().position(|row| row.name == *text) {
                        self.current = Some(row);
                        settle(ui, taken, Ok(()));
                    } else {
                        settle(ui, taken, Err(lists_no(NAME, text)));
                    }
                }
                Act::Press => settle(ui, taken, Err(no_button(&taken.posted))),
                Act::ScrollTo => {}
            }
        }
        let current = self.current;
        let mut picked = None;
        let mut renaming = self.renaming.take();
        let rows = &mut self.rows;
        let drawn = ui.vertical(|ui| {
            ui.weak("Name | Style | Source | Triangles");
            for (at, row) in rows.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    let mut shown = row.shown;
                    if ui
                        .checkbox(&mut shown, "")
                        .on_hover_text(if row.shown {
                            "Shown: untick to hide it (SURFACE HIDE)"
                        } else {
                            "Hidden: tick to show it (SURFACE SHOW)"
                        })
                        .changed()
                    {
                        asked.push(Asked::Shown(at, shown));
                    }
                    // Only the name is typed into, and only when asked (Rename, or a double
                    // click): a click picks a row.
                    let mut typed_here = None;
                    if let Some((which, text)) = &mut renaming
                        && *which == at
                    {
                        let edit = ui.text_edit_singleline(text);
                        if edit.lost_focus() {
                            typed_here = Some(text.clone());
                        } else {
                            edit.request_focus();
                        }
                    } else {
                        let name = ui.selectable_label(current == Some(at), row.name.as_str());
                        if name.double_clicked() {
                            picked = Some(at);
                            asked.push(Asked::Rename);
                        } else if name.clicked() {
                            picked = Some(at);
                        }
                    }
                    if let Some(typed) = typed_here {
                        renaming = None;
                        asked.push(Asked::Renamed(at, typed));
                    }
                    let mut style = Choice::at(row.style);
                    let changed = if current == Some(at) {
                        choice(ui, "surfaceStyle", &row.style_texts, &mut style)
                            .on_hover_text("The style the surface is drawn in (SURFACE STYLE)")
                            .changed()
                    } else {
                        let mut changed = false;
                        egui::ComboBox::from_id_salt(("surfaceStyle", at))
                            .selected_text(row.style_text())
                            .show_ui(ui, |ui| {
                                for (index, text) in row.style_texts.iter().enumerate() {
                                    if ui
                                        .selectable_label(style.index == Some(index), text)
                                        .clicked()
                                    {
                                        style.index = Some(index);
                                        changed = true;
                                    }
                                }
                            });
                        changed
                    };
                    if changed && let Some(index) = style.index {
                        asked.push(Asked::Style(at, index));
                    }
                    ui.label(row.source.as_str())
                        .on_hover_text(row.source.as_str());
                    ui.label(format!("{}", row.triangles));
                });
            }
        });
        self.renaming = renaming;
        if picked.is_some() {
            self.current = picked;
        }
        if let Some(taken) = &taken
            && taken.posted.act == Act::ScrollTo
        {
            drawn.response.scroll_to_me(None);
            settle(ui, taken, Ok(()));
        }
        let mut table = ControlRecord::new(NAME, ControlKind::Rows);
        table.items = self
            .rows
            .iter()
            .map(|row| {
                format!(
                    "{} | {} | {} | {}",
                    row.name,
                    row.style_text(),
                    row.source,
                    row.triangles
                )
            })
            .collect();
        table.value = self.picked_surface().unwrap_or_default();
        table.enabled = ui.is_enabled();
        table.visible = ui.is_visible();
        record(ui, &drawn.response, table);
    }

    /// One frame of the panel: the drawing read again if it changed, the tools, the table and the
    /// status; then what was clicked, done through the executor.
    pub fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        self.refresh(host.document());
        let can_zoom = host.views().is_some();
        let mut asked = Vec::new();
        let outer = ui.vertical(|ui| {
            self.tools(ui, can_zoom, &mut asked);
            self.table(ui, &mut asked);
            let status = self.status.clone();
            label(ui, "surfacesStatus", &status);
        });
        custom(
            ui,
            &outer.response,
            ControlRecord::new("surfacesPanel", ControlKind::Other),
        );
        for what in asked {
            match what {
                Asked::Shown(row, shown) => self.set_shown(host, row, shown),
                Asked::Renamed(row, name) => self.set_name(host, row, &name),
                Asked::Style(row, index) => self.choose_style(host, row, index),
                Asked::Rename => {
                    if let Some(row) = self.current
                        && let Some(found) = self.rows.get(row)
                    {
                        self.renaming = Some((row, found.name.clone()));
                    }
                }
                Asked::Copy => self.copy_picked(host),
                Asked::Remove => self.remove_picked(host),
                Asked::ZoomTo => self.zoom_to_picked(host),
                Asked::EditStyles => {
                    if self.services.edit_styles {
                        self.edit_styles_asked += 1;
                    }
                }
            }
        }
        // What a line changed is shown at the next frame, which this asks for.
        if !self.shown_once
            || self.surfaces_seen != host.document().surfaces_generation()
            || self.styles_seen != host.document().surface_styles_generation()
        {
            ui.ctx().request_repaint();
        }
    }
}

/// The panel's dock and its two ways in, for the window to install (installSurfacesPanel): the
/// dock is hidden until asked for, so the window's layout is as it was for a drawing that has no
/// use for the panel; View > Panels > Surfaces shows it and brings it to the front of its tab
/// group, and Terrain > Surfaces Panel - placed after Surface From Drawing when the menu has it
/// ("surfaceFromDrawing"), else at the menu's end - shows and raises it.
pub struct SurfacesPanelUi {
    pub dock: DockSpec,
    /// View > Panels > Surfaces.
    pub toggle: Action,
    /// Terrain > Surfaces Panel.
    pub terrain_item: Action,
}

/// The dock and the two actions as the C++ named and worded them.
pub fn surfaces_panel_ui() -> SurfacesPanelUi {
    let tip = "List the drawing's surfaces: show or hide each, choose its style, rename, copy or \
               remove it";
    SurfacesPanelUi {
        dock: DockSpec::new("SurfacesDock", "Surfaces"),
        toggle: Action::new("viewSurfacesPanel", "&Surfaces")
            .icon("SurfacesPanel")
            .tip(
                "Show or hide the Surfaces panel: the drawing's surfaces, shown or hidden, their \
                 styles",
            )
            .checkable(false),
        terrain_item: Action::new("terrainSurfacesPanel", "S&urfaces Panel")
            .icon("SurfacesPanel")
            .tip(tip),
    }
}
