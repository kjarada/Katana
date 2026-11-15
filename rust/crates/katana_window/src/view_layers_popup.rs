//! The layers ONE view shows: the port of view_layers_popup (the user's request of 2026-09-23 for
//! "per view controls to add and remove layers"; docs/cad.md, "The workspace").
//!
//! Opened from the Layers button on a view's title bar. It sets that view's ViewState - its
//! LayerOverrides, its hidden references and its ghosts of the selection - and nothing else: never
//! an undoable command, never the document's modified flag. And it does not set them itself: every
//! box and button builds the VIEWS line a person would type (VIEWS HIDE, SHOW, ISOLATE, SET:
//! `view_layers_line` and `view_references_line`) and hands it to the window's executor, so a click
//! is logged as a typed line is, runs the one path an agent's line does, and is refused by it the
//! same way. The boxes are then read back from the view, so a refused line leaves each where the
//! view really is. What a view hides is how the person is LOOKING at the drawing, like its zoom;
//! the Layers panel, which edits the document, is where a layer is switched off in every view.
//!
//! The tree is the document's layer tree, derived from the '/'-separated names (layer_path); a
//! path segment that is not itself a layer is still a node, and hiding it hides what lies beneath.
//! A box is ticked when this view does not hide that layer itself. It is greyed, and cannot be
//! ticked, when the layer cannot show here whatever its box says - the document hides it, or a
//! parent is hidden in this view: the Layers panel's own greying, for the same reason. A view can
//! never show what the document hides, so a greyed box offers nothing to tick.
//!
//! THE RUST SHAPE. The C++ popup is a QFrame holding the workspace and reading the document
//! through it; it closes on a click outside it. Here the popup is kept by the workspace
//! (`ViewWorkspace::with_layers_popup`) and lent the workspace and the window's executor for each
//! call, and its lists are read again from the drawing, the reference data and the view at each
//! frame and after each line - what the C++'s rebuild and refreshStates did on their signals. A
//! click on a row's box is `set_layer_checked`, as the C++ tests set an item's check state.

use std::collections::{BTreeMap, BTreeSet};

use egui::{Pos2, RichText, Ui};
use katana_cad::Document;
use katana_cad::view_set::{ViewId, ViewSet, ViewState};
use katana_core::error::make_error;
use katana_core::{ErrorCode, Result, Status};
use katana_entity::LayerDatabase;
use katana_entity::layer_path::{is_layer_under, layer_leaf, layer_parent};
use katana_interop::ReferenceData;

use crate::command_word::command_word;
use crate::drive::controls;
use crate::drive::registry::{
    ControlKind, ControlRecord, TargetKind, TargetSpec, begin_target, end_target,
};
use crate::host::{Host, VerbOutcome};
use crate::theme::palette;
use crate::view_workspace::ViewWorkspace;

/// The popup's least width, in points.
pub const MINIMUM_WIDTH: f32 = 300.0;

/// The VIEWS line that does `action` - HIDE, SHOW or ISOLATE - to `layers` in view `view`: what a
/// click in the popup runs. A name with a blank is quoted (command_word), and a name no line can
/// carry - a double quote or a line break in it - is refused, naming it. Each name is a word, which
/// the verb reads as a comma list; a name with a comma in it, or one the verb reads as its own word
/// (ALL, LAYER, REFERENCE), is written after LAYER instead, which takes every word after it whole.
/// ISOLATE takes one layer. InvalidArgument for no layer, an action that is none of the three, or
/// two layers to isolate.
pub fn view_layers_line(action: &str, view: ViewId, layers: &[String]) -> Result<String> {
    if action != "HIDE" && action != "SHOW" && action != "ISOLATE" {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!("a view's layers are hidden, shown or isolated, not '{action}'"),
            action,
        ));
    }
    if layers.is_empty() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!("no layer to {}", action.to_ascii_lowercase()),
            action,
        ));
    }
    if action == "ISOLATE" && layers.len() != 1 {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!("a view isolates one layer, not {}", layers.len()),
            action,
        ));
    }
    let mut words = Vec::new();
    // Whether the verb's comma list can carry every name as it is.
    let mut plain = true;
    for name in layers {
        words.push(command_word(name, "layer name")?);
        if name.contains(',')
            || name.eq_ignore_ascii_case("ALL")
            || name.eq_ignore_ascii_case("LAYER")
            || name.eq_ignore_ascii_case("REFERENCE")
        {
            plain = false;
        }
    }
    Ok(format!(
        "VIEWS {action} {view} {}{}",
        if plain { "" } else { "LAYER " },
        words.join(" ")
    ))
}

/// VIEWS HIDE or SHOW <view> REFERENCE <ids>: the reference layers (the Reference Data panel's ids)
/// a view hides. `action` is HIDE or SHOW.
pub fn view_references_line(action: &str, view: ViewId, references: &[u64]) -> String {
    let ids: Vec<String> = references.iter().map(u64::to_string).collect();
    format!("VIEWS {action} {view} REFERENCE {}", ids.join(" "))
}

/// Whether the DOCUMENT lets `path` show: the layer's inherited visibility or, for a path segment
/// that is not itself a layer, that of its nearest ancestor that is. The database answers "not
/// shown" for a name it does not hold, which for an intermediate node would grey out everything
/// beneath it.
fn document_shows(layers: &LayerDatabase, path: &str) -> bool {
    let mut path = path;
    while !path.is_empty() {
        if layers.contains(path) {
            return layers.effectively_visible(path);
        }
        path = layer_parent(path);
    }
    true
}

/// The reference layer's own switch, in the Reference Data panel. A view can hide a reference the
/// panel shows, never show one it hides.
fn reference_shown(reference: Option<&ReferenceData>, id: u64) -> bool {
    let Some(reference) = reference else {
        return false;
    };
    reference
        .rasters()
        .iter()
        .find(|raster| raster.id == id)
        .map(|raster| raster.visible)
        .or_else(|| {
            reference
                .point_clouds()
                .iter()
                .find(|cloud| cloud.id == id)
                .map(|cloud| cloud.visible)
        })
        .unwrap_or(false)
}

/// One row of the layer tree, in the order the tree shows them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayerRow {
    /// The full path (the C++'s kPathRole).
    pub path: String,
    /// What the row says: the path's last segment.
    pub label: String,
    pub depth: usize,
    /// False for a node that is no layer itself, drawn in italics.
    pub is_layer: bool,
    /// This view does not hide it itself.
    pub checked: bool,
    /// Greyed: the document hides it, or a parent is hidden in this view.
    pub enabled: bool,
    pub tip: String,
    /// Left out by the filter.
    pub filtered_out: bool,
}

/// One row of the reference layers' list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReferenceRow {
    /// The Reference Data panel's id (the C++'s kReferenceRole).
    pub id: u64,
    pub name: String,
    /// "Raster" or "Point cloud".
    pub kind: String,
    pub checked: bool,
    /// The panel shows it, so this view may hide it.
    pub enabled: bool,
    pub tip: String,
    pub filtered_out: bool,
}

/// What a frame's clicks asked, carried out after the popup has been drawn.
#[derive(Clone, Debug, PartialEq)]
enum Asked {
    Layer(String, bool),
    Reference(u64, bool),
    Current(String),
    ShowAll,
    Isolate,
    HideOthers,
    Ghosts,
    Close,
}

/// A view's Layers popup (see the module comment).
pub struct ViewLayersPopup {
    view: ViewId,
    open: bool,
    heading: String,
    filter: String,
    layers: Vec<LayerRow>,
    references: Vec<ReferenceRow>,
    /// The selected layer, by path.
    current: Option<String>,
    ghosts: bool,
    /// Where it hangs from: the bottom right of the view's Layers button, right-aligned under it.
    anchor: Pos2,
    asked: Vec<Asked>,
}

impl ViewLayersPopup {
    /// The popup of view `view` of `workspace`, built from the drawing, the window's reference data
    /// and the view as they are now.
    pub fn new(
        workspace: &ViewWorkspace,
        document: &Document,
        reference: Option<&ReferenceData>,
        view: ViewId,
        anchor: Pos2,
    ) -> Self {
        let mut popup = Self {
            view,
            open: true,
            heading: String::new(),
            filter: String::new(),
            layers: Vec::new(),
            references: Vec::new(),
            current: None,
            ghosts: true,
            anchor,
            asked: Vec::new(),
        };
        popup.rebuild(workspace, document, reference);
        popup
    }

    pub fn view(&self) -> ViewId {
        self.view
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    /// "Layers in Plan 1".
    pub fn heading(&self) -> &str {
        &self.heading
    }

    /// The layer tree's rows, in the order shown.
    pub fn layer_rows(&self) -> &[LayerRow] {
        &self.layers
    }

    /// The layer row of `path`, or None (the C++'s itemFor).
    pub fn item_for(&self, path: &str) -> Option<&LayerRow> {
        self.layers.iter().find(|row| row.path == path)
    }

    pub fn reference_rows(&self) -> &[ReferenceRow] {
        &self.references
    }

    /// "Show the selection on hidden layers": the view's ghosts.
    pub fn ghosts_checked(&self) -> bool {
        self.ghosts
    }

    pub fn current(&self) -> Option<&str> {
        self.current.as_deref()
    }

    /// Selects the layer row of `path` (None: no row), as a click on it does.
    pub fn set_current(&mut self, path: Option<&str>) {
        self.current = path
            .filter(|path| self.layers.iter().any(|row| row.path == *path))
            .map(String::from);
    }

    /// Isolate and Hide Others are offered only for a selected layer that is not greyed.
    pub fn isolate_enabled(&self) -> bool {
        self.current
            .as_deref()
            .and_then(|path| self.item_for(path))
            .is_some_and(|row| row.enabled)
    }

    pub fn filter(&self) -> &str {
        &self.filter
    }

    /// Shows only the layers and references whose path contains `text`, with the parents that lead
    /// to them; case-insensitive. Empty shows all.
    pub fn set_filter(&mut self, text: &str) {
        self.filter = String::from(text);
        self.apply_filter();
    }

    fn apply_filter(&mut self) {
        let needle = self.filter.trim().to_lowercase();
        let matches = |path: &str| needle.is_empty() || path.to_lowercase().contains(&needle);
        // A row is kept when it or anything beneath it matches: the rows are in pre-order, so a
        // row's descendants are the rows after it that are deeper.
        let count = self.layers.len();
        for at in 0..count {
            let depth = self.layers[at].depth;
            let below = self.layers[at + 1..]
                .iter()
                .take_while(|row| row.depth > depth)
                .any(|row| matches(&row.path));
            let kept = matches(&self.layers[at].path) || below;
            self.layers[at].filtered_out = !kept;
        }
        for row in &mut self.references {
            row.filtered_out =
                !needle.is_empty() && !row.name.to_lowercase().contains(&needle);
        }
    }

    /// The view's state, a copy; None once it has closed.
    fn state_of(workspace: &ViewWorkspace, view: ViewId) -> Option<ViewState> {
        workspace.state(view)
    }

    /// Both lists read again from the drawing, the reference data and the view.
    pub fn rebuild(
        &mut self,
        workspace: &ViewWorkspace,
        document: &Document,
        reference: Option<&ReferenceData>,
    ) {
        let state = Self::state_of(workspace, self.view);
        self.heading = state
            .as_ref()
            .map_or_else(|| String::from("Layers"), |state| {
                format!("Layers in {}", ViewSet::title(state))
            });

        let database = &document.model().layers;
        // Every node is made after its parent, and a segment need not be a layer at all, so a
        // node's parent is looked up - or made - rather than assumed to be the last made.
        let mut children: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut roots: Vec<String> = Vec::new();
        let mut made: BTreeSet<String> = BTreeSet::new();
        fn make(
            path: &str,
            made: &mut BTreeSet<String>,
            roots: &mut Vec<String>,
            children: &mut BTreeMap<String, Vec<String>>,
        ) {
            if made.contains(path) {
                return;
            }
            let parent = layer_parent(path);
            if parent.is_empty() {
                roots.push(String::from(path));
            } else {
                make(parent, made, roots, children);
                children
                    .entry(String::from(parent))
                    .or_default()
                    .push(String::from(path));
            }
            made.insert(String::from(path));
        }
        for path in database.names() {
            make(&path, &mut made, &mut roots, &mut children);
        }
        let mut rows = Vec::new();
        let mut stack: Vec<(String, usize)> = roots.iter().rev().map(|path| (path.clone(), 0)).collect();
        while let Some((path, depth)) = stack.pop() {
            if let Some(below) = children.get(&path) {
                for child in below.iter().rev() {
                    stack.push((child.clone(), depth + 1));
                }
            }
            rows.push(LayerRow {
                label: String::from(layer_leaf(&path)),
                is_layer: database.contains(&path),
                path,
                depth,
                checked: true,
                enabled: true,
                tip: String::new(),
                filtered_out: false,
            });
        }
        self.layers = rows;
        if let Some(current) = self.current.clone()
            && self.item_for(&current).is_none()
        {
            self.current = None;
        }

        let mut references = Vec::new();
        if let Some(reference) = reference {
            for raster in reference.rasters() {
                references.push((raster.id, raster.name.clone(), "Raster"));
            }
            for cloud in reference.point_clouds() {
                references.push((cloud.id, cloud.name.clone(), "Point cloud"));
            }
        }
        self.references = references
            .into_iter()
            .map(|(id, name, kind)| ReferenceRow {
                id,
                name,
                kind: String::from(kind),
                checked: true,
                enabled: true,
                tip: String::new(),
                filtered_out: false,
            })
            .collect();
        if let Some(state) = &state {
            self.refresh_states(document, reference, state);
        }
        self.apply_filter();
    }

    /// The boxes, the greying and the ghosts read again from the view, in place.
    fn refresh_states(
        &mut self,
        document: &Document,
        reference: Option<&ReferenceData>,
        state: &ViewState,
    ) {
        let database = &document.model().layers;
        for row in &mut self.layers {
            let document_hides = !document_shows(database, &row.path);
            let parent = layer_parent(&row.path);
            let parent_hidden = !parent.is_empty() && state.layers.hides(parent);
            row.checked = !state.layers.hides_directly(&row.path);
            row.enabled = !(document_hides || parent_hidden);
            let mut tip = row.path.clone();
            if !database.contains(&row.path) {
                tip += "\nNot a layer itself: hiding it hides the layers beneath it.";
            }
            if document_hides {
                tip += "\nHidden in every view by the Layers panel.";
            } else if parent_hidden {
                tip += "\nHidden in this view by a parent layer.";
            }
            row.tip = tip;
        }
        for row in &mut self.references {
            let shown_by_panel = reference_shown(reference, row.id);
            row.checked = !state.hidden_references.contains(&row.id);
            row.enabled = shown_by_panel;
            row.tip = if shown_by_panel {
                row.name.clone()
            } else {
                format!("{}\nHidden in every view by the Reference Data panel.", row.name)
            };
        }
        self.ghosts = state.selection_ghosts;
    }

    /// What follows a line: the lists read again, so each box says what the view does.
    fn after_line(&mut self, workspace: &ViewWorkspace, host: &dyn Host) {
        let reference = Some(host.reference());
        self.rebuild(workspace, host.document(), reference);
    }

    // ---- what a click does -----------------------------------------------------------------

    /// A click on layer `path`'s box, ticking it (`shown`) or clearing it: its line through the
    /// window's executor. A view that has closed closes the popup.
    pub fn set_layer_checked(
        &mut self,
        workspace: &ViewWorkspace,
        host: &mut dyn Host,
        path: &str,
        shown: bool,
    ) {
        if workspace.state(self.view).is_none() {
            self.close();
            return;
        }
        let _ = workspace.set_view_layers_shown(host, self.view, &[String::from(path)], shown);
        self.after_line(workspace, host);
    }

    /// A click on reference layer `id`'s box.
    pub fn set_reference_checked(
        &mut self,
        workspace: &ViewWorkspace,
        host: &mut dyn Host,
        id: u64,
        shown: bool,
    ) {
        if workspace.state(self.view).is_none() {
            self.close();
            return;
        }
        let _ = workspace.set_view_reference_shown(host, self.view, id, shown);
        self.after_line(workspace, host);
    }

    /// Show All: everything the document shows, references included (VIEWS SHOW <id> ALL). With
    /// nothing hidden, nothing to say.
    pub fn show_all(&mut self, workspace: &ViewWorkspace, host: &mut dyn Host) {
        let Some(state) = workspace.state(self.view) else {
            return;
        };
        if !state.layers.empty() || !state.hidden_references.is_empty() {
            let _ = workspace.show_everything_in_view(host, self.view);
        }
        self.after_line(workspace, host);
    }

    /// Isolate: everything that is not the selected layer, its parents or beneath it hidden,
    /// replacing whatever this view hid (VIEWS ISOLATE). NotFound when no layer is selected; the
    /// line's own refusal when it is refused.
    pub fn isolate_selected(&mut self, workspace: &ViewWorkspace, host: &mut dyn Host) -> Status {
        let current = self.current.clone();
        let (Some(_), Some(path)) = (workspace.state(self.view), current) else {
            return Err(make_error(ErrorCode::NotFound, "no layer is selected", ""));
        };
        let outcome = workspace.isolate_layer_in_view(host, self.view, &path);
        self.after_line(workspace, host);
        refused_unless_ok(&outcome)
    }

    /// Hide Others: as Isolate, keeping what the view already hid beneath the selected layer - that
    /// line, and then VIEWS HIDE of what it had hidden there.
    pub fn hide_others(&mut self, workspace: &ViewWorkspace, host: &mut dyn Host) -> Status {
        let current = self.current.clone();
        let (Some(state), Some(path)) = (workspace.state(self.view), current) else {
            return Err(make_error(ErrorCode::NotFound, "no layer is selected", ""));
        };
        // What the view already hides strictly beneath the selected layer, which isolating
        // replaces and this puts back: the selected layer itself is the one being kept.
        let keep: Vec<String> = state
            .layers
            .hidden()
            .iter()
            .filter(|hidden| **hidden != path && is_layer_under(hidden, &path))
            .cloned()
            .collect();
        let mut outcome = workspace.isolate_layer_in_view(host, self.view, &path);
        if outcome.ok && !keep.is_empty() {
            outcome = workspace.set_view_layers_shown(host, self.view, &keep, false);
        }
        self.after_line(workspace, host);
        refused_unless_ok(&outcome)
    }

    /// The "Show the selection on hidden layers" box clicked: VIEWS SET <id> ghosts=on|off through
    /// the window's executor, and the box then read from the view.
    pub fn click_ghosts(&mut self, workspace: &ViewWorkspace, host: &mut dyn Host) {
        let on = !self.ghosts;
        workspace.set_selection_ghosts(host, self.view, on);
        self.after_line(workspace, host);
    }

    // ---- the frame -------------------------------------------------------------------------

    /// The popup drawn under its anchor, as the target ViewLayersPopup; what was clicked is done
    /// after it is drawn.
    pub fn ui(&mut self, ui: &mut Ui, workspace: &ViewWorkspace, host: &mut dyn Host) {
        if !self.open {
            return;
        }
        if workspace.state(self.view).is_none() {
            self.close();
            return;
        }
        self.after_line(workspace, host);
        let at = Pos2::new(self.anchor.x - MINIMUM_WIDTH, self.anchor.y);
        let area = egui::Area::new(egui::Id::new(("katanaViewLayersPopup", self.view)))
            .order(egui::Order::Foreground)
            .fixed_pos(at)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_min_width(MINIMUM_WIDTH);
                    let spec = TargetSpec::new("ViewLayersPopup", TargetKind::Popup, &self.heading);
                    begin_target(ui, spec);
                    self.body(ui);
                    end_target(ui, ui.min_rect());
                });
            });
        // A click outside it closes it, as a Qt popup does.
        let outside = ui.input(|input| {
            input.pointer.any_pressed()
                && input
                    .pointer
                    .interact_pos()
                    .is_some_and(|pos| !area.response.rect.contains(pos))
        });
        if outside {
            self.asked.push(Asked::Close);
        }
        for asked in std::mem::take(&mut self.asked) {
            match asked {
                Asked::Layer(path, shown) => self.set_layer_checked(workspace, host, &path, shown),
                Asked::Reference(id, shown) => self.set_reference_checked(workspace, host, id, shown),
                Asked::Current(path) => self.set_current(Some(&path)),
                Asked::ShowAll => self.show_all(workspace, host),
                // Offered only for a selection, so a failure is a layer deleted under an open
                // popup - nothing to report but the rebuild that shows it has gone.
                Asked::Isolate => {
                    let _ = self.isolate_selected(workspace, host);
                }
                Asked::HideOthers => {
                    let _ = self.hide_others(workspace, host);
                }
                Asked::Ghosts => self.click_ghosts(workspace, host),
                Asked::Close => self.close(),
            }
        }
    }

    fn body(&mut self, ui: &mut Ui) {
        controls::label_as(
            ui,
            "ViewLayersHeading",
            &self.heading,
            egui::Label::new(RichText::new(self.heading.as_str()).strong()),
        );
        // Said once, where the choice is made: the difference between this and the Layers panel
        // is the thing most likely to be misunderstood.
        let note = "Hidden here only. The Layers panel hides a layer in every view and saves it \
                    with the drawing.";
        controls::label_as(ui, "ViewLayersNote", note, egui::Label::new(note).wrap());
        let mut filter = self.filter.clone();
        if controls::line_with(
            ui,
            "ViewLayersFilter",
            &mut filter,
            controls::LineOptions {
                placeholder: "Filter layers",
                ..controls::LineOptions::default()
            },
        )
        .changed()
        {
            self.set_filter(&filter);
        }

        // Tall enough to show every layer up to eighteen rows, never under eight, so the popup
        // does not jump in size between a small drawing and a large one.
        let rows = self.layers.len().clamp(8, 18) as f32;
        let tree = egui::ScrollArea::vertical()
            .id_salt("ViewLayersTreeScroll")
            .max_height(rows * 22.0)
            .min_scrolled_height(rows * 22.0)
            .show(ui, |ui| {
                for row in self.layers.iter().filter(|row| !row.filtered_out) {
                    ui.horizontal(|ui| {
                        ui.add_space(row.depth as f32 * 14.0);
                        let mut checked = row.checked;
                        let mut text = RichText::new(row.label.as_str());
                        if !row.is_layer {
                            text = text.italics();
                        }
                        if !row.enabled {
                            text = text.color(palette::text_disabled().to_egui());
                        }
                        let boxed = ui.add_enabled(row.enabled, egui::Checkbox::without_text(&mut checked));
                        if boxed.changed() {
                            self.asked.push(Asked::Layer(row.path.clone(), checked));
                        }
                        let selected = self.current.as_deref() == Some(row.path.as_str());
                        let label = ui
                            .add_enabled_ui(row.enabled, |ui| ui.selectable_label(selected, text))
                            .inner;
                        if label.on_hover_text(row.tip.as_str()).clicked() {
                            self.asked.push(Asked::Current(row.path.clone()));
                        }
                    });
                }
            });
        let mut record = ControlRecord::new("ViewLayersTree", ControlKind::Rows);
        record.items = self
            .layers
            .iter()
            .filter(|row| !row.filtered_out)
            .map(|row| row.path.clone())
            .collect();
        record.value = self.current.clone().unwrap_or_default();
        let tree = ui.interact(
            tree.inner_rect,
            ui.id().with("ViewLayersTree"),
            egui::Sense::hover(),
        );
        controls::custom(ui, &tree, record);

        ui.horizontal(|ui| {
            let button = |ui: &mut Ui, name: &str, text: &str, tip: &str, enabled: bool| {
                ui.add_enabled_ui(enabled, |ui| {
                    controls::button(ui, name, text)
                        .on_hover_text(format!("{text}\n{tip}"))
                        .clicked()
                })
                .inner
            };
            if button(
                ui,
                "ViewLayersShowAll",
                "Show All",
                "Show every layer and reference layer the document shows, in this view.",
                true,
            ) {
                self.asked.push(Asked::ShowAll);
            }
            let selectable = self.isolate_enabled();
            if button(
                ui,
                "ViewLayersIsolate",
                "Isolate",
                "Show only the selected layer and what lies beneath it, in this view. Replaces \
                 whatever this view hid.",
                selectable,
            ) {
                self.asked.push(Asked::Isolate);
            }
            if button(
                ui,
                "ViewLayersHideOthers",
                "Hide Others",
                "Hide every layer but the selected one and what lies beneath it, keeping what \
                 this view already hides beneath it.",
                selectable,
            ) {
                self.asked.push(Asked::HideOthers);
            }
        });

        // The view's own switch for the selection's ghosts: here because it is about what this
        // view hides. Clicked, not toggled: only the person's click runs the line, never the box
        // being set from the view.
        let mut ghosts = self.ghosts;
        if controls::check(
            ui,
            "ViewLayersShowSelection",
            "Show the selection on hidden layers",
            &mut ghosts,
        )
        .on_hover_text(
            "A selected feature on a layer this view hides is drawn here faint and dotted, so a \
             selection made in another view shows where it is. It is never picked, snapped to, \
             given grips or plotted here. VIEWS SET <id> ghosts=on|off on the command line.",
        )
        .changed()
        {
            self.asked.push(Asked::Ghosts);
        }

        if !self.references.is_empty() {
            ui.label(RichText::new("Reference data").strong());
            let list = ui.vertical(|ui| {
                for row in self.references.iter().filter(|row| !row.filtered_out) {
                    ui.horizontal(|ui| {
                        let mut checked = row.checked;
                        if ui
                            .add_enabled(row.enabled, egui::Checkbox::new(&mut checked, row.name.as_str()))
                            .on_hover_text(row.tip.as_str())
                            .changed()
                        {
                            self.asked.push(Asked::Reference(row.id, checked));
                        }
                        ui.label(row.kind.as_str());
                    });
                }
            });
            let mut record = ControlRecord::new("ViewReferencesTree", ControlKind::Rows);
            record.items = self
                .references
                .iter()
                .map(|row| format!("{} | {}", row.name, row.kind))
                .collect();
            controls::custom(ui, &list.response, record);
        }
    }
}

/// What a refused line says, as the Status the popup's buttons answer with.
fn refused_unless_ok(outcome: &VerbOutcome) -> Status {
    if outcome.ok {
        return Ok(());
    }
    Err(make_error(
        ErrorCode::CommandRejected,
        if outcome.error.is_empty() {
            String::from("the line was refused")
        } else {
            outcome.error.clone()
        },
        "",
    ))
}
