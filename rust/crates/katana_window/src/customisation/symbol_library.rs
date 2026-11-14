//! The symbol library: AutoCAD's Blocks palette and MicroStation's cell selector in one non-modal
//! window over the drawing. The port of customisation/symbol_library.
//!
//! ```text
//! left    a tree of groups - All, Built-in, the library's `/` groups, "(ungrouped)", and "Not
//!         defined" when a name resolves to nothing
//! centre  a grid of 64-pixel pictures (DefinitionThumbnails), captioned with the name and badged
//!         with how many entities draw it, under a FilterBar: search over name, group and survey
//!         code, and the chips All / In drawing / Used by codes / Missing / At vertices
//! right   a preview at a plot scale, the details of the current symbol - including how big it
//!         prints: "2 x 2 mm at 1:500, 1 x 1 m on the ground" - and the actions
//! ```
//!
//! WHAT IS LISTED is cad's symbol_library: decision D3's symbols (a definition is a symbol when it
//! is drawn at vertices, a survey code draws it as one, a style names it as one, or its
//! customisation lists it as a symbol), the built-in shapes, and every symbol name a style or a
//! code gives that nothing defines, marked in the pickers' amber with the shape it is drawn as
//! instead.
//!
//! THE ACTIONS. Model changes go through Document::execute, one undo step each, and only ever from
//! a button - never from the grid's own selection (docs/cad.md):
//!
//! ```text
//! Assign to Selected Points   cad's assign_symbol_to_points: the points among the selection move
//!                             into a style that draws the symbol at the size given, found or made
//! Set on Style                the chosen style's symbol, through commands' update_style_if_changed
//! Select Points Using         the points wearing a style that names it, selected and shown
//! Replace in Styles           cad's replace_symbol_in_styles: every style naming the current
//!                             symbol names another
//! Import Definitions...       the line CUSTOMISE DEFINITIONS <file>: a Katana customisation file's
//!                             DEFINITIONS and colours MERGED into the session's (decision D1:
//!                             never a Replace from here); its survey code rules and settings are
//!                             left alone and counted
//! Export Selected...          the line CUSTOMISE EXPORT <file> NAME <name> ONLY <names>: the
//!                             selected library definitions as a Katana customisation file, with
//!                             the session's notice, the sources that brought it definitions and
//!                             the colours their pens name
//! ```
//!
//! HEADLESS. A headless session never opens a file dialog: importing and exporting are
//! `import_definitions_file` and `export_selected_to`, taking a path, which the buttons call once
//! a file dialog has given one. In a headless session (the host's, or `set_headless`) the buttons
//! ask `choose_import_file` / `choose_export_file` when set and otherwise say in the log what to
//! call.
//!
//! The four definition buttons (symbolNew, symbolEdit, symbolDuplicate, symbolDelete) only ask:
//! the definition editor is the maker's, which reads what was asked with
//! `take_definition_requests` after the frame and hands it to its editor.
//!
//! LIFETIME. The dialog may outlive its Document: it watches it through a DocumentWatcher and does
//! nothing once it has gone. Reloads are the watcher's - at the dialog's next frame - never a
//! rebuild inside a Document notification.
//!
//! What is the C++'s and not here. The grid selects one symbol by a click and adds or takes one
//! with the command key held; a range by Shift is not made.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use egui::{Context, RichText, Ui};
use katana_cad::Document;
use katana_cad::style_catalogue::{CatalogueEntry, DefinitionSource, NameRole};
use katana_cad::style_drawing::StyleTextMark;
use katana_cad::symbol_assign::{
    SymbolCode, SymbolLibraryEntry, TextExtent, assign_symbol_to_points, replace_symbol_in_styles,
    symbol_library, symbol_print_size, symbol_users,
};
use katana_commands::update_style_if_changed;
use katana_core::Error;
use katana_entity::{LineStyle, StrokeOp, StyleUnits, Users};
use katana_geometry::Box2;

use super::code_manager::{TreeLine, tree_rows};
use super::code_manager_support::{customisation_file_word_of, exported_customisation_name};
use super::codes_seams::{
    CodesContext, DefinitionEdit, NamePicker, PreviewGround, StylePreview, UNDEFINED_NAME_COLOUR,
};
use super::definition_editor::index_choice;
use super::definition_thumbnails::{DefinitionThumbnails, ThumbnailKind};
use super::document_watcher::{DocumentChanges, DocumentWatcher};
use super::filter_bar::FilterBar;
use super::style_painter::{CanvasRasterBackend, style_text_extent};
use super::symbol_library_model::{
    SymbolChip, SymbolFilter, SymbolGridModel, SymbolGroupFilter, SymbolGroupKind,
};
use crate::annotation::dialog_support::show_dialog;
use crate::canvas::{Font, Image, TextMetrics};
use crate::command_word::command_word;
use crate::drive::controls::{self, SpinRange, custom};
use crate::drive::mailbox::{Act, fill_disabled, lists_no, no_button};
use crate::drive::registry::{ControlKind, ControlRecord, record, settle, take};
use crate::format::{grouped, qt_fixed};
use crate::host::Host;
use crate::shell::DialogFrame;
use crate::survey::survey_dialogs::{hidden, open_file_dialog, save_file_dialog};
use crate::theme::palette;

const DIALOG: &str = "symbolLibraryDialog";
const TITLE: &str = "Symbol Library";

/// The chips, in SymbolChip's order.
const CHIP_LABELS: [&str; 5] = [
    "All",
    "In drawing",
    "Used by codes",
    "Missing",
    "At vertices",
];

/// What each chip's object name is made from (FilterBar::chip_object_name): the label it was
/// first shown with. The last was "Vertex mode", and tests and headless scripts find it as
/// "filterVertexmode", so the name outlives the label.
const CHIP_NAMES: [&str; 5] = [
    "All",
    "In drawing",
    "Used by codes",
    "Missing",
    "Vertex mode",
];

/// The symbol's width on the ground: 0 is not a size, it is "the definition's own".
const SIZE: SpinRange = SpinRange::new(0.0, 10000.0, 0.5, 3).with_suffix(" m");

const FILE_FILTERS: [(&str, &[&str]); 2] = [
    ("Katana customisation", &["customisation.json", "json"]),
    ("All files", &["*"]),
];

fn in_quotes(value: &str) -> String {
    format!("\"{value}\"")
}

/// A file's name as a person reads it, for the log: never its folder.
fn path_text(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn describe(error: &Error) -> String {
    error.describe()
}

/// A length as a person reads it: at most three decimals, no trailing zeros, so 2 mm is "2" and a
/// quarter metre "0.25".
fn number(value: f64) -> String {
    let mut written = qt_fixed(value, 3);
    if written.contains('.') {
        while written.ends_with('0') {
            written.pop();
        }
        if written.ends_with('.') {
            written.pop();
        }
    }
    if written == "-0" {
        String::from("0")
    } else {
        written
    }
}

const TIMES: &str = " \u{d7} ";

/// "a, b, c and 12 more": a load of 800 definitions is one readable line.
fn list_of(names: &[String]) -> String {
    const SHOWN: usize = 40;
    let mut joined = names
        .iter()
        .take(SHOWN)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    if names.len() > SHOWN {
        joined.push_str(&format!(
            " and {} more",
            grouped((names.len() - SHOWN) as u64)
        ));
    }
    joined
}

fn plural(count: usize, one: &str, many: &str) -> String {
    format!(
        "{} {}",
        grouped(count as u64),
        if count == 1 { one } else { many }
    )
}

fn units_text(units: StyleUnits) -> &'static str {
    match units {
        StyleUnits::World => "World: metres on the ground",
        StyleUnits::Paper => "Paper: millimetres on the plot",
        StyleUnits::TwoPoint => "Two-point: stretched between two points",
    }
}

fn unit_suffix(units: StyleUnits) -> &'static str {
    if units == StyleUnits::Paper {
        " mm"
    } else {
        " m"
    }
}

/// Why D3 lists a definition that is not at vertices as a symbol.
fn mode_text(entry: &CatalogueEntry) -> String {
    if entry.at_vertices {
        return String::from("at vertices: drawn at each vertex of a string");
    }
    let mut reasons = Vec::new();
    if entry.kind.named_by_survey_rule {
        reasons.push("a survey code draws it as one");
    }
    if entry.kind.named_by_style {
        reasons.push("a style names it as one");
    }
    if entry.kind.listed_as_symbol {
        reasons.push("its customisation lists it as a symbol");
    }
    format!("along a line; a symbol because {}", reasons.join(", "))
}

fn content_text(definition: &LineStyle) -> String {
    let mut strokes = 0;
    let mut texts = 0;
    let mut pens = 0;
    for stroke in &definition.strokes {
        match stroke.op {
            StrokeOp::Text => texts += 1,
            StrokeOp::Pen => pens += 1,
            _ => strokes += 1,
        }
    }
    format!(
        "{}, {}, {}",
        plural(strokes, "stroke", "strokes"),
        plural(texts, "text", "texts"),
        plural(pens, "pen change", "pen changes")
    )
}

fn codes_text(codes: &[SymbolCode]) -> String {
    if codes.is_empty() {
        return String::from("No survey code draws it");
    }
    codes
        .iter()
        .map(|code| {
            let size = if code.size > 0.0 {
                format!("size {}", number(code.size))
            } else {
                String::from("its own size")
            };
            let colour = if code.colour.is_empty() {
                "the string's colour"
            } else {
                code.colour.as_str()
            };
            format!("{} ({size}, {colour})", code.key)
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn styles_text(users: &Users) -> String {
    if users.styles.is_empty() {
        return String::from("No style names it");
    }
    format!(
        "{} - {} in the drawing",
        list_of(&users.styles),
        plural(users.entities, "entity", "entities")
    )
}

/// The picture of one entry, PICTURE_SIZE logical pixels across at `ratio` device pixels each; a
/// null image once the Document has gone.
fn picture_of(
    document: Option<&Document>,
    thumbnails: &RefCell<DefinitionThumbnails>,
    entry: &SymbolLibraryEntry,
    ratio: f32,
) -> Image {
    let Some(document) = document else {
        return Image::default();
    };
    // Device pixels, as DefinitionThumbnails asks, so a HiDPI grid is sharp.
    let pixels = (f64::from(SymbolGridModel::PICTURE_SIZE) * f64::from(ratio)).round() as u32;
    let mut image = thumbnails
        .borrow_mut()
        .thumbnail_of(
            document,
            ThumbnailKind::Symbol,
            &entry.entry.name,
            (pixels, pixels),
            palette::viewport(),
        )
        .image;
    image.set_device_pixel_ratio(f64::from(ratio));
    image
}

/// One row of the group tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupRow {
    /// How deep it sits: 0 at the top, 1 for "Test/Marks" under "Test".
    pub depth: usize,
    /// "Marks (3)".
    pub label: String,
    pub filter: SymbolGroupFilter,
    /// Drawn in the amber of a name nothing defines: "Not defined".
    pub marked: bool,
}

/// What the details pane says of the current symbol; a row with nothing to say is not shown.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Details {
    name: String,
    missing: String,
    print: String,
    source: String,
    group: String,
    units: String,
    mode: String,
    factor: String,
    origin: String,
    origin_warned: bool,
    extent: String,
    content: String,
    codes: String,
    styles: String,
}

/// Which buttons can be pressed, worked out by `update_actions`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Actions {
    assign: bool,
    assign_tip: String,
    set_on_style: bool,
    replace: bool,
    select_using: bool,
    size: bool,
    load: bool,
    export: bool,
    new_definition: bool,
    defined: bool,
}

/// What a frame's drawing asks for, carried out after it with the host.
#[derive(Clone, Debug, PartialEq)]
enum Event {
    Tree(usize),
    Filter,
    /// A grid row clicked; `toggle` with the command key held.
    Grid {
        row: usize,
        toggle: bool,
    },
    PlotScale,
    Ground,
    Size,
    ReplaceWith,
    Assign,
    SetOnStyle,
    Replace,
    SelectUsing,
    Import,
    Export,
    Door(DefinitionEdit),
    Close,
}

/// A file chosen in place of a file dialog; an empty path cancels.
pub type ChooseFile = Box<dyn Fn() -> PathBuf>;

/// The symbol library (see the module comment).
pub struct SymbolLibraryDialog {
    frame: DialogFrame,
    open: bool,
    context: CodesContext,
    /// The context's picture cache, or this dialog's own when it gave none: a grid of names
    /// without pictures is not a symbol library.
    thumbnails: Rc<RefCell<DefinitionThumbnails>>,
    /// None: the host's.
    headless: Option<bool>,
    /// Asked instead of a file dialog when set; an empty path cancels.
    pub choose_import_file: Option<ChooseFile>,
    pub choose_export_file: Option<ChooseFile>,
    current: String,
    model: SymbolGridModel,
    /// The names selected in the grid; every one is shown.
    selected: Vec<String>,
    groups: Vec<GroupRow>,
    group: SymbolGroupFilter,
    filter_bar: FilterBar,
    count_label: String,
    preview: StylePreview,
    plot_scale: usize,
    ground: usize,
    details: Details,
    size: f64,
    target_styles: Vec<String>,
    target_style: Option<usize>,
    replace_with: NamePicker,
    actions: Actions,
    requests: Vec<(DefinitionEdit, String)>,
    grid_textures: BTreeMap<String, egui::TextureHandle>,
    events: Vec<Event>,
    logs: Vec<(String, bool)>,
    watcher: DocumentWatcher,
}

impl SymbolLibraryDialog {
    /// The library over `document`, shown and loaded.
    pub fn new(document: &Document, context: CodesContext) -> Self {
        let thumbnails = context.thumbnails.clone().unwrap_or_else(|| {
            Rc::new(RefCell::new(DefinitionThumbnails::new(Arc::new(
                CanvasRasterBackend,
            ))))
        });
        let mut filter_bar = FilterBar::new(&CHIP_NAMES);
        for (index, label) in CHIP_LABELS.iter().enumerate() {
            filter_bar.set_chip_label(index, label);
        }
        filter_bar.set_placeholder_text("Search name, group or survey code");
        let preview = StylePreview::new();
        let plot_scale = StylePreview::PLOT_SCALES
            .iter()
            .position(|&denominator| denominator == preview.scale_denominator())
            .unwrap_or(0);
        let mut dialog = Self {
            frame: DialogFrame::new(DIALOG, TITLE),
            open: true,
            context,
            thumbnails,
            headless: None,
            choose_import_file: None,
            choose_export_file: None,
            current: String::new(),
            model: SymbolGridModel::new(),
            selected: Vec::new(),
            groups: Vec::new(),
            group: SymbolGroupFilter::default(),
            filter_bar,
            count_label: String::new(),
            preview,
            plot_scale,
            ground: 0,
            details: Details::default(),
            size: 0.0,
            target_styles: Vec::new(),
            target_style: None,
            replace_with: NamePicker::new(NameRole::Symbol, false),
            actions: Actions::default(),
            requests: Vec::new(),
            grid_textures: BTreeMap::new(),
            events: Vec::new(),
            logs: Vec::new(),
            watcher: DocumentWatcher::new(document),
        };
        dialog.reload_in(Some(document));
        dialog
    }

    fn alive(&self) -> bool {
        self.watcher.document_alive()
    }

    /// `document`, while it is the Document the library was made with and that is still there.
    fn live_document<'d>(&self, document: &'d Document) -> Option<&'d Document> {
        self.alive().then_some(document)
    }

    fn log(&mut self, message: String, is_error: bool) {
        self.logs.push((message, is_error));
    }

    fn flush(&mut self, host: &mut dyn Host) {
        for (message, is_error) in std::mem::take(&mut self.logs) {
            host.log(&message, is_error);
        }
    }

    // ---- what is shown -------------------------------------------------------------------------

    /// The symbol the details, the preview and the actions are about; empty when none is.
    pub fn current_symbol(&self) -> &str {
        &self.current
    }

    /// The names in the grid, in order.
    pub fn shown_names(&self) -> Vec<String> {
        (0..self.model.row_count())
            .filter_map(|row| self.model.display(row))
            .collect()
    }

    /// The names selected in the grid, in grid order.
    pub fn selected_names(&self) -> Vec<String> {
        (0..self.model.row_count())
            .filter_map(|row| self.model.display(row))
            .filter(|name| self.selected.contains(name))
            .collect()
    }

    pub fn grid_model(&self) -> &SymbolGridModel {
        &self.model
    }

    pub fn preview(&self) -> &StylePreview {
        &self.preview
    }

    /// The group tree's rows, in order.
    pub fn group_rows(&self) -> &[GroupRow] {
        &self.groups
    }

    /// The picker beside Replace in Styles.
    pub fn replace_with(&self) -> &NamePicker {
        &self.replace_with
    }

    /// A replacement chosen in the picker as a program chooses it (NamePicker::setCurrentName),
    /// and the button beside it enabled or not.
    pub fn set_replacement(&mut self, host: &mut dyn Host, name: &str) {
        if let Some(document) = self.live_document(host.document()) {
            self.replace_with.set_current_name(document, name);
        }
        let document = self.live_document(host.document());
        self.update_actions(document);
    }

    /// A replacement typed into the picker (QComboBox::setEditText).
    pub fn type_replacement(&mut self, host: &mut dyn Host, text: &str) {
        self.replace_with.set_edit_text(text);
        let document = self.live_document(host.document());
        self.update_actions(document);
    }

    pub fn set_headless(&mut self, headless: bool) {
        self.headless = Some(headless);
    }

    /// Whether the session has nobody to answer a file dialog: as set, else the host's.
    pub fn headless(&self, host: &dyn Host) -> bool {
        self.headless.unwrap_or_else(|| host.headless())
    }

    /// What the definition buttons asked since this was last called, for the maker's editor.
    pub fn take_definition_requests(&mut self) -> Vec<(DefinitionEdit, String)> {
        std::mem::take(&mut self.requests)
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn show(&mut self) {
        self.open = true;
    }

    /// Makes `name` current and selects it alone, clearing the tree, the chip and the search
    /// first when they hide it. False when nothing lists that exact name.
    pub fn select_symbol(&mut self, host: &mut dyn Host, name: &str) -> bool {
        let document = self.live_document(host.document());
        let selected = self.select_symbol_in(document, name);
        self.flush(host);
        selected
    }

    fn select_symbol_in(&mut self, document: Option<&Document>, name: &str) -> bool {
        if !self
            .model
            .entries()
            .iter()
            .any(|entry| entry.entry.name == name)
        {
            return false;
        }
        if self.model.row_of(name).is_none() {
            self.filter_bar.set_text("");
            self.filter_bar.set_chip(0);
            self.filter_bar.take_changes();
            self.group = SymbolGroupFilter::default();
        }
        self.current = String::from(name);
        self.apply_filter(document, vec![String::from(name)]);
        !self.current.is_empty()
    }

    /// `name` added to the grid's selection, the current symbol left as it is
    /// (QItemSelectionModel::Select). Nothing for a name the grid does not show.
    pub fn add_to_selection(&mut self, host: &mut dyn Host, name: &str) {
        if self.model.row_of(name).is_some() && !self.selected.iter().any(|held| held == name) {
            self.selected.push(String::from(name));
        }
        let document = self.live_document(host.document());
        self.update_actions(document);
    }

    /// A click on `name` in the grid with the command key held, as a person makes one: the item is
    /// made current and whether it is selected is toggled.
    pub fn ctrl_click(&mut self, host: &mut dyn Host, name: &str) {
        if let Some(row) = self.model.row_of(name) {
            let document = self.live_document(host.document());
            self.grid_clicked(document, row, true);
        }
    }

    fn grid_clicked(&mut self, document: Option<&Document>, row: usize, toggle: bool) {
        let Some(name) = self.model.display(row) else {
            return;
        };
        if toggle {
            match self.selected.iter().position(|held| *held == name) {
                Some(at) => {
                    self.selected.remove(at);
                }
                None => self.selected.push(name.clone()),
            }
        } else {
            self.selected = vec![name.clone()];
        }
        self.current = name;
        self.show_details(document);
        self.update_actions(document);
    }

    // ---- reloading -----------------------------------------------------------------------------

    fn on_document_changed(&mut self, document: &Document, changes: DocumentChanges) {
        if changes.model || changes.library || changes.survey_map {
            self.reload_in(Some(document));
        } else if changes.selection {
            self.update_actions(Some(document));
        }
    }

    /// Rebuilds from the Document now. The watcher's delivery calls it on its own; a caller
    /// needing the result at once may too.
    pub fn reload(&mut self, host: &mut dyn Host) {
        let document = self.live_document(host.document());
        self.reload_in(document);
        self.flush(host);
    }

    fn reload_in(&mut self, document: Option<&Document>) {
        let Some(document) = document else {
            return;
        };
        let selected = self.selected_names();
        self.model.set_entries(symbol_library(document));
        self.grid_textures.clear();
        self.rebuild_tree();
        self.rebuild_styles(document);
        self.replace_with.refresh(document);
        self.apply_filter(Some(document), selected);
    }

    fn rebuild_tree(&mut self) {
        let mut built_in = 0;
        let mut ungrouped = 0;
        let mut missing = 0;
        // Every group and every group above it, with the entries at or below it. A BTreeMap walks
        // a path before any path it prefixes, so each parent is listed before its children.
        let mut groups: BTreeMap<String, usize> = BTreeMap::new();
        for entry in self.model.entries() {
            let item = &entry.entry;
            if item.missing {
                missing += 1;
            } else if item.source == DefinitionSource::BuiltIn {
                built_in += 1;
            } else if item.group.is_empty() {
                ungrouped += 1;
            } else {
                for (slash, _) in item.group.match_indices('/') {
                    *groups.entry(String::from(&item.group[..slash])).or_default() += 1;
                }
                *groups.entry(item.group.clone()).or_default() += 1;
            }
        }

        let row = |depth: usize, label: &str, count: usize, kind: SymbolGroupKind, path: &str| {
            GroupRow {
                depth,
                label: format!("{label} ({})", grouped(count as u64)),
                filter: SymbolGroupFilter {
                    kind,
                    path: String::from(path),
                },
                marked: kind == SymbolGroupKind::Missing,
            }
        };
        let mut rows = vec![row(
            0,
            "All symbols",
            self.model.entries().len(),
            SymbolGroupKind::All,
            "",
        )];
        if built_in > 0 {
            rows.push(row(0, "Built-in", built_in, SymbolGroupKind::BuiltIn, ""));
        }
        // A map's order is by bytes, in which '/' sorts after a blank: "Test Extra" would come
        // between "Test" and "Test/Marks". Each group is put under its parent instead.
        let mut placed: Vec<(String, GroupRow)> = Vec::new();
        for (path, count) in &groups {
            let (parent, leaf) = match path.rfind('/') {
                Some(slash) => (Some(&path[..slash]), &path[slash + 1..]),
                None => (None, path.as_str()),
            };
            let depth = path.matches('/').count();
            let made = row(depth, leaf, *count, SymbolGroupKind::Group, path);
            // After the parent's last descendant so far.
            let at = match parent {
                Some(parent) => {
                    let prefix = format!("{parent}/");
                    placed
                        .iter()
                        .rposition(|(held, _)| held == parent || held.starts_with(&prefix))
                        .map_or(placed.len(), |last| last + 1)
                }
                None => placed.len(),
            };
            placed.insert(at, (path.clone(), made));
        }
        rows.extend(placed.into_iter().map(|(_, made)| made));
        if ungrouped > 0 {
            rows.push(row(
                0,
                "(ungrouped)",
                ungrouped,
                SymbolGroupKind::Ungrouped,
                "",
            ));
        }
        if missing > 0 {
            rows.push(row(0, "Not defined", missing, SymbolGroupKind::Missing, ""));
        }
        // What was chosen, found again by what it is; else everything.
        if !rows.iter().any(|held| held.filter == self.group) {
            self.group = SymbolGroupFilter::default();
        }
        self.groups = rows;
    }

    fn rebuild_styles(&mut self, document: &Document) {
        let kept = self
            .target_style
            .and_then(|at| self.target_styles.get(at))
            .cloned();
        self.target_styles = document.model().styles.names();
        // A list refilled chooses its first item, unless the one chosen is still there.
        self.target_style = kept
            .and_then(|kept| self.target_styles.iter().position(|name| *name == kept))
            .or(if self.target_styles.is_empty() {
                None
            } else {
                Some(0)
            });
    }

    /// Refilters, keeping `selected` (by name: rows mean nothing across a refill) and the current
    /// symbol where they are still shown.
    fn apply_filter(&mut self, document: Option<&Document>, selected: Vec<String>) {
        let chip = self.filter_bar.chip().unwrap_or(0);
        self.model.set_filter(SymbolFilter {
            text: String::from(self.filter_bar.text()),
            chip: SymbolChip::ALL[chip.min(SymbolChip::ALL.len() - 1)],
            group: self.group.clone(),
        });
        self.update_chip_counts();
        self.count_label = format!(
            "{} of {} symbols",
            grouped(self.model.row_count() as u64),
            grouped(self.model.entries().len() as u64)
        );
        self.restore_current(document, selected);
    }

    fn update_chip_counts(&mut self) {
        for (index, label) in CHIP_LABELS.iter().enumerate() {
            let count = self.model.count_for(SymbolChip::ALL[index]);
            self.filter_bar
                .set_chip_label(index, &format!("{label} ({})", grouped(count as u64)));
        }
    }

    fn restore_current(&mut self, document: Option<&Document>, selected: Vec<String>) {
        // Exactly the names asked for, where they are still shown, whether or not the current
        // symbol is among them: a click with the command key that deselects an item leaves it
        // current, and putting it back in place of what the person kept made the next search or
        // reload export the wrong symbols. select_symbol asks for its symbol alone by passing
        // just that name.
        self.selected = selected
            .into_iter()
            .filter(|name| self.model.row_of(name).is_some())
            .collect();
        if self.model.row_of(&self.current).is_none() {
            // Filtered away: nothing is current, so no action can reach a symbol the person can
            // no longer see.
            self.current.clear();
        }
        self.show_details(document);
    }

    fn current_entry(&self) -> Option<&SymbolLibraryEntry> {
        if self.current.is_empty() {
            return None;
        }
        self.model
            .entries()
            .iter()
            .find(|entry| entry.entry.name == self.current)
    }

    // ---- details -------------------------------------------------------------------------------

    fn show_details(&mut self, document: Option<&Document>) {
        let mut details = Details::default();
        let entry = document.and_then(|_| self.current_entry().cloned());
        let (Some(document), Some(entry)) = (document, entry) else {
            details.name = String::from("Choose a symbol");
            self.details = details;
            self.preview.clear();
            self.update_actions(document);
            return;
        };
        let item = &entry.entry;
        details.name = item.name.clone();
        self.preview.set_symbol(&item.name, self.size);
        details.codes = codes_text(&entry.codes);
        details.styles = styles_text(&item.users);
        match item.source {
            DefinitionSource::Library => {
                if let Some(definition) = document.definition_for(&item.name) {
                    details.source = if definition.source.is_empty() {
                        String::from("Made in this session")
                    } else {
                        definition.source.clone()
                    };
                    details.group = if definition.group.is_empty() {
                        String::from("(ungrouped)")
                    } else {
                        definition.group.clone()
                    };
                    details.units = String::from(units_text(definition.units));
                    details.mode = mode_text(item);
                    details.factor = number(definition.factor);
                    let bounds = definition.bounds();
                    details.extent = if bounds.empty() {
                        String::from("Draws nothing to measure")
                    } else {
                        format!(
                            "{}{TIMES}{}{}",
                            number(bounds.width()),
                            number(bounds.height()),
                            unit_suffix(definition.units)
                        )
                    };
                    details.content = content_text(definition);
                }
            }
            DefinitionSource::BuiltIn => {
                details.source = String::from("Built in: Katana draws it without a library");
                details.group = String::from("Built-in");
                details.units =
                    String::from("model units: the style's size, else the plain mark's");
                details.mode = String::from("a point mark");
            }
            DefinitionSource::Undefined | DefinitionSource::ModelLinetype => {
                details.source = String::from("Not defined in any loaded library");
                details.missing = format!(
                    "Not defined - drawn as the built-in \"{}\" until a library defines it",
                    entry.fallback
                );
            }
        }
        self.details = details;
        self.update_print_size(Some(document));
        self.update_actions(Some(document));
    }

    fn update_print_size(&mut self, document: Option<&Document>) {
        let entry = document.and_then(|_| self.current_entry().cloned());
        let (Some(document), Some(entry)) = (document, entry) else {
            self.details.print.clear();
            return;
        };
        let denominator =
            StylePreview::PLOT_SCALES[self.plot_scale.min(StylePreview::PLOT_SCALES.len() - 1)];
        let size = self.size;
        let scale = format!("1:{denominator}");

        if entry.entry.source == DefinitionSource::Library {
            let definition = document.definition_for(&entry.entry.name);
            // A text measured in the face the preview beside this paints it in, so "how big it
            // prints" and the picture agree about a symbol's letters; cad alone can only estimate
            // them.
            let font = Font::default();
            let metrics = TextMetrics::screen();
            let measured =
                move |mark: &StyleTextMark| -> Box2 { style_text_extent(mark, &font, &metrics) };
            let extent: &TextExtent = &measured;
            let printed = definition.and_then(|definition| {
                symbol_print_size(definition, size, f64::from(denominator), Some(extent))
            });
            let (Some(definition), Some(printed)) = (definition, printed) else {
                self.details.print = String::from("Draws nothing to measure");
                return;
            };
            let paper = format!(
                "{}{TIMES}{}",
                number(printed.paper_width),
                number(printed.paper_height)
            );
            let ground = format!(
                "{}{TIMES}{}",
                number(printed.ground_width),
                number(printed.ground_height)
            );
            self.details.print = format!("{paper} mm at {scale}, {ground} m on the ground");
            let origin = definition.origin;
            let mut origin_text = format!("({}, {})", number(origin.x), number(origin.y));
            if !printed.insertion_inside {
                origin_text.push_str(
                    " - insertion point outside the drawing: the mark sits away from the point \
                     it is put on",
                );
            }
            self.details.origin = origin_text;
            self.details.origin_warned = !printed.insertion_inside;
            return;
        }
        // A built-in shape (or the stand-in for a missing name) has no size of its own: without
        // one it is the viewport's plain mark, a screen size.
        let prefix = if entry.entry.missing {
            "As its stand-in: "
        } else {
            ""
        };
        self.details.print = if size > 0.0 {
            let paper = number(size * 1000.0 / f64::from(denominator));
            format!(
                "{prefix}{paper} mm across at {scale}, {} m on the ground",
                number(size)
            )
        } else {
            format!(
                "{prefix}the plain point mark's screen size; give it a size to fix what it prints"
            )
        };
    }

    fn update_actions(&mut self, document: Option<&Document>) {
        let entry = document.and_then(|_| self.current_entry());
        let selected = document.map_or(0, |document| document.selection().size());
        let mut actions = Actions {
            assign: entry.is_some() && selected > 0,
            assign_tip: format!(
                "Put the points among the {} selected into a style that draws this symbol at the \
                 size given (one undo step)",
                grouped(selected as u64)
            ),
            set_on_style: entry.is_some() && !self.target_styles.is_empty(),
            ..Actions::default()
        };
        // Only once a replacement is chosen: the picker starts empty, and an empty replacement
        // took the symbol off every style naming it - its points and lines lost their mark - and
        // was logged as a success.
        let replacement = self.replace_with.current_name();
        actions.replace = entry.is_some_and(|entry| {
            !entry.entry.users.styles.is_empty()
                && !replacement.is_empty()
                && replacement != entry.entry.name
        });
        actions.select_using = entry.is_some_and(|entry| entry.entry.users.entities > 0);
        actions.size = entry.is_some();
        actions.load = document.is_some();
        if let Some(document) = document {
            actions.export = self
                .selected_names()
                .iter()
                .any(|name| document.definition_for(name).is_some());
        }
        // New needs only an editor to ask; the other three a definition the library holds - not a
        // built-in shape, nor a name nothing defines.
        let editor = document.is_some() && self.context.definition_editor;
        actions.new_definition = editor;
        actions.defined = editor
            && entry.is_some()
            && document.is_some_and(|document| document.style_library().contains(&self.current));
        self.actions = actions;
    }

    // ---- actions: each logs what it did or why it did nothing, and returns whether the drawing
    // or a file changed ----------------------------------------------------------------------------

    /// The current symbol onto the points among the Document's selection, at the size in the size
    /// field (0: the definition's own).
    pub fn assign_to_selected_points(&mut self, host: &mut dyn Host) -> bool {
        let assigned = self.assign_in(host);
        self.flush(host);
        assigned
    }

    fn assign_in(&mut self, host: &mut dyn Host) -> bool {
        if !self.alive() {
            return false;
        }
        if self.current.is_empty() {
            self.log(String::from("Assign to points: choose a symbol first"), true);
            return false;
        }
        let current = self.current.clone();
        let ids = host.document().selection().ids();
        let done = match assign_symbol_to_points(host.document(), &ids, &current, self.size) {
            Ok(done) => done,
            Err(error) => {
                self.log(
                    format!(
                        "Assign {} to points: {}",
                        in_quotes(&current),
                        describe(&error)
                    ),
                    true,
                );
                return false;
            }
        };
        let mut left = String::new();
        if done.not_points > 0 {
            left = format!(
                " {} not a point and left as {}.",
                if done.not_points == 1 {
                    String::from("1 selected entity is")
                } else {
                    format!("{} selected entities are", grouped(done.not_points as u64))
                },
                if done.not_points == 1 {
                    "it was"
                } else {
                    "they were"
                }
            );
        }
        let Some(command) = done.command else {
            self.log(
                format!(
                    "{} already in style {}, which draws {}: nothing to change.{left}",
                    plural(done.already_in_style, "point is", "points are"),
                    in_quotes(&done.style),
                    in_quotes(&current)
                ),
                false,
            );
            return false;
        };
        if let Err(error) = host.document_mut().execute(command) {
            self.log(
                format!(
                    "Assign {} to points: {}",
                    in_quotes(&current),
                    describe(&error)
                ),
                true,
            );
            return false;
        }
        self.log(
            format!(
                "{} now in style {}{}, drawing {}.{left}",
                plural(done.points, "point", "points"),
                in_quotes(&done.style),
                if done.creates_style { " (new)" } else { "" },
                in_quotes(&current)
            ),
            false,
        );
        true
    }

    /// `style`'s symbol set to the current one.
    pub fn set_on_style(&mut self, host: &mut dyn Host, style: &str) -> bool {
        let set = self.set_on_style_in(host, style);
        self.flush(host);
        set
    }

    fn set_on_style_in(&mut self, host: &mut dyn Host, style: &str) -> bool {
        if !self.alive() {
            return false;
        }
        if self.current.is_empty() {
            self.log(String::from("Set on style: choose a symbol first"), true);
            return false;
        }
        let current = self.current.clone();
        let model = host.document().model();
        let Some(found) = model.styles.find(style) else {
            self.log(
                format!("Set on style: there is no style {}", in_quotes(style)),
                true,
            );
            return false;
        };
        let mut changed = found.clone();
        changed.symbol = current.clone();
        let Some(command) = update_style_if_changed(model, changed) else {
            self.log(
                format!(
                    "Style {} already draws {}",
                    in_quotes(style),
                    in_quotes(&current)
                ),
                false,
            );
            return false;
        };
        if let Err(error) = host.document_mut().execute(command) {
            self.log(
                format!(
                    "Set {} on style {}: {}",
                    in_quotes(&current),
                    in_quotes(style),
                    describe(&error)
                ),
                true,
            );
            return false;
        }
        self.log(
            format!(
                "Style {} now draws {}",
                in_quotes(style),
                in_quotes(&current)
            ),
            false,
        );
        true
    }

    /// Selects (and through the context shows) the points wearing a style that names the current
    /// symbol.
    pub fn select_points_using(&mut self, host: &mut dyn Host) -> bool {
        let selected = self.select_points_using_in(host);
        self.flush(host);
        selected
    }

    fn select_points_using_in(&mut self, host: &mut dyn Host) -> bool {
        if !self.alive() || self.current.is_empty() {
            return false;
        }
        let current = self.current.clone();
        let users = symbol_users(host.document(), &current);
        if users.points.is_empty() {
            self.log(format!("No point draws {}", in_quotes(&current)), false);
            return false;
        }
        match self.context.select_and_show.clone() {
            Some(select_and_show) => select_and_show(host, &users.points),
            None => {
                host.document_mut()
                    .selection_mut()
                    .set(users.points.clone());
                host.document().notify_selection_changed();
            }
        }
        let others = if users.other_entities > 0 {
            format!(
                " ({} in the same styles also draw it at their vertices)",
                plural(users.other_entities, "line", "lines")
            )
        } else {
            String::new()
        };
        self.log(
            format!(
                "Selected {} drawing {}{others}",
                plural(users.points.len(), "point", "points"),
                in_quotes(&current)
            ),
            false,
        );
        true
    }

    /// Every style naming the current symbol names `replacement` instead. Refused (and logged)
    /// for an empty `replacement`: taking a symbol off its styles is not a replacement.
    pub fn replace_in_styles(&mut self, host: &mut dyn Host, replacement: &str) -> bool {
        let replaced = self.replace_in_styles_in(host, replacement);
        self.flush(host);
        replaced
    }

    fn replace_in_styles_in(&mut self, host: &mut dyn Host, replacement: &str) -> bool {
        if !self.alive() || self.current.is_empty() {
            return false;
        }
        let current = self.current.clone();
        // "" is not a symbol to put in its place: set on a style, it would take the symbol off
        // (see update_actions).
        if replacement.is_empty() {
            self.log(
                format!(
                    "Replace {}: choose the symbol to put in its place first",
                    in_quotes(&current)
                ),
                true,
            );
            return false;
        }
        let styles = symbol_users(host.document(), &current).styles;
        let Some(command) =
            replace_symbol_in_styles(host.document().model(), &current, replacement)
        else {
            self.log(
                format!(
                    "Replace {}: no style draws it, or it is the replacement",
                    in_quotes(&current)
                ),
                false,
            );
            return false;
        };
        if let Err(error) = host.document_mut().execute(command) {
            self.log(
                format!(
                    "Replace {} with {}: {}",
                    in_quotes(&current),
                    in_quotes(replacement),
                    describe(&error)
                ),
                true,
            );
            return false;
        }
        self.log(
            format!(
                "Replaced {} with {} in {}: {}",
                in_quotes(&current),
                in_quotes(replacement),
                plural(styles.len(), "style", "styles"),
                list_of(&styles)
            ),
            false,
        );
        true
    }

    /// Runs the line CUSTOMISE DEFINITIONS <file> through the window's executor: a Katana
    /// customisation file's definitions and colours are MERGED into the session's customisation,
    /// as every load is. The survey code rules, linework codes and automation switches the file
    /// also holds are not taken, and the reply says how many were left. The commit hook is called
    /// round the line: before it, and what it hands back after a load that was taken. False, with
    /// the reason in the log and the session as it was, for a file that does not read, one that
    /// holds neither a definition nor a colour, or a dialog given no command line.
    pub fn import_definitions_file(&mut self, host: &mut dyn Host, path: &Path) -> bool {
        let imported = self.import_in(host, path);
        self.flush(host);
        imported
    }

    fn import_in(&mut self, host: &mut dyn Host, path: &Path) -> bool {
        if !self.alive() {
            return false;
        }
        let file = path_text(path);
        let word = match customisation_file_word_of(path) {
            Ok(word) => word,
            Err(error) => {
                self.log(
                    format!("Could not import {file}: {}", describe(&error)),
                    true,
                );
                return false;
            }
        };
        if !self.context.command_line {
            self.log(
                String::from(
                    "Nothing here can run CUSTOMISE DEFINITIONS: the library was given no \
                     command line.",
                ),
                true,
            );
            return false;
        }
        // A load MERGES (decision D1): replacing the session's library from a symbol browser
        // would throw away every definition the drawing uses that this file does not happen to
        // hold.
        //
        // Round the commit, the hook every editor of the customisation calls: before the line,
        // which leaves the session "not kept", and what it hands back after a load that was
        // TAKEN - never after one that was refused, when nothing changed and there is nothing to
        // keep.
        let committed = self.context.begin(host);
        self.flush(host);
        let outcome = host.run(&format!("CUSTOMISE DEFINITIONS {word}"));
        if !self.alive() {
            return false;
        }
        if !outcome.ok {
            self.log(
                format!(
                    "Could not import {file}: {}",
                    if outcome.error.is_empty() {
                        "the command was refused."
                    } else {
                        outcome.error.as_str()
                    }
                ),
                true,
            );
            return false;
        }
        if let Some(committed) = committed {
            committed(host);
        }
        true
    }

    /// Runs CUSTOMISE EXPORT <file> NAME <name> ONLY <the selected names> to write the selected
    /// library definitions to a Katana customisation file under the session's name, with what of
    /// the session belongs with them (cad's customisation_part). Built-in and undefined names
    /// cannot be written, and are named in the log.
    pub fn export_selected_to(&mut self, host: &mut dyn Host, path: &Path) -> bool {
        let exported = self.export_in(host, path);
        self.flush(host);
        exported
    }

    fn export_in(&mut self, host: &mut dyn Host, path: &Path) -> bool {
        if !self.alive() {
            return false;
        }
        let mut names = Vec::new();
        let mut skipped = Vec::new();
        for name in self.selected_names() {
            if host.document().definition_for(&name).is_some() {
                names.push(name);
            } else {
                skipped.push(name);
            }
        }
        if !skipped.is_empty() {
            self.log(
                format!(
                    "Not exported, as no library defines them: {}",
                    list_of(&skipped)
                ),
                false,
            );
        }
        if names.is_empty() {
            self.log(
                String::from("Export: select one or more library symbols in the grid first"),
                true,
            );
            return false;
        }
        // The name is always given, so that a session nothing was loaded into is written under
        // the file's own.
        let file = path_text(path);
        let word = match customisation_file_word_of(path) {
            Ok(word) => word,
            Err(error) => {
                self.log(format!("Export to {file}: {}", describe(&error)), true);
                return false;
            }
        };
        let session = exported_customisation_name(Some(host.document()), path);
        let session_word = match command_word(&session, "the customisation's name") {
            Ok(word) => word,
            Err(error) => {
                self.log(format!("Export to {file}: {}", describe(&error)), true);
                return false;
            }
        };
        let mut line = format!("CUSTOMISE EXPORT {word} NAME {session_word} ONLY");
        for name in &names {
            match command_word(name, "a definition's name") {
                Ok(each) => {
                    line.push(' ');
                    line.push_str(&each);
                }
                Err(error) => {
                    self.log(
                        format!(
                            "Export: no command line can name {} - {}",
                            in_quotes(name),
                            error.message
                        ),
                        true,
                    );
                    return false;
                }
            }
        }
        if !self.context.command_line {
            self.log(
                String::from(
                    "Nothing here can run CUSTOMISE EXPORT: the library was given no command \
                     line.",
                ),
                true,
            );
            return false;
        }
        self.flush(host);
        let outcome = host.run(&line);
        if !self.alive() {
            return false;
        }
        if !outcome.ok {
            self.log(
                format!(
                    "Export to {file}: {}",
                    if outcome.error.is_empty() {
                        "the command was refused."
                    } else {
                        outcome.error.as_str()
                    }
                ),
                true,
            );
            return false;
        }
        self.log(
            format!(
                "Exported {} to {file}: {}",
                plural(names.len(), "symbol", "symbols"),
                list_of(&names)
            ),
            false,
        );
        true
    }

    /// Import Definitions... pressed.
    pub fn import_clicked(&mut self, host: &mut dyn Host) {
        let path = if let Some(choose) = &self.choose_import_file {
            choose()
        } else if self.headless(host) {
            self.log(
                String::from(
                    "Import Definitions: a headless session opens no file dialog; call \
                     importDefinitionsFile with a path",
                ),
                true,
            );
            self.flush(host);
            return;
        } else {
            open_file_dialog(
                "Import Definitions from a Katana Customisation",
                &FILE_FILTERS,
            )
            .unwrap_or_default()
        };
        if !path.as_os_str().is_empty() {
            self.import_definitions_file(host, &path);
        }
    }

    /// Export Selected... pressed.
    pub fn export_clicked(&mut self, host: &mut dyn Host) {
        let path = if let Some(choose) = &self.choose_export_file {
            choose()
        } else if self.headless(host) {
            self.log(
                String::from(
                    "Export Selected: a headless session opens no file dialog; call \
                     exportSelectedTo with a path",
                ),
                true,
            );
            self.flush(host);
            return;
        } else {
            save_file_dialog(
                "Export Symbols to a Katana Customisation",
                "symbols.customisation.json",
                &FILE_FILTERS,
            )
            .unwrap_or_default()
        };
        if !path.as_os_str().is_empty() {
            self.export_selected_to(host, &path);
        }
    }

    // ---- the frame -----------------------------------------------------------------------------

    /// Draws the library for this frame, when it is shown - the drawing's changes since the last
    /// frame first - and carries out what was pressed. True while it is shown.
    pub fn ui(&mut self, ctx: &Context, host: &mut dyn Host) -> bool {
        if let Some(changes) = self.watcher.deliver(host.document()) {
            self.on_document_changed(host.document(), changes);
        }
        if !self.open {
            return false;
        }
        let mut frame = std::mem::replace(&mut self.frame, DialogFrame::new(DIALOG, TITLE));
        let closed = show_dialog(&mut frame, ctx, self, host, Self::body, Self::footer);
        self.frame = frame;
        for event in std::mem::take(&mut self.events) {
            self.react(host, event);
        }
        if closed {
            self.open = false;
        }
        self.flush(host);
        self.open
    }

    fn react(&mut self, host: &mut dyn Host, event: Event) {
        match event {
            Event::Tree(row) => {
                if let Some(chosen) = self.groups.get(row) {
                    self.group = chosen.filter.clone();
                    let selected = self.selected_names();
                    let document = self.live_document(host.document());
                    self.apply_filter(document, selected);
                }
            }
            Event::Filter => {
                let selected = self.selected_names();
                let document = self.live_document(host.document());
                self.apply_filter(document, selected);
            }
            Event::Grid { row, toggle } => {
                let document = self.live_document(host.document());
                self.grid_clicked(document, row, toggle);
            }
            Event::PlotScale => {
                let denominator = StylePreview::PLOT_SCALES
                    [self.plot_scale.min(StylePreview::PLOT_SCALES.len() - 1)];
                self.preview.set_scale_denominator(denominator);
                let document = self.live_document(host.document());
                self.update_print_size(document);
            }
            Event::Ground => {
                self.preview.set_ground(if self.ground == 1 {
                    PreviewGround::Screen
                } else {
                    PreviewGround::Paper
                });
            }
            Event::Size => {
                if !self.current.is_empty() {
                    let current = self.current.clone();
                    self.preview.set_symbol(&current, self.size);
                }
                let document = self.live_document(host.document());
                self.update_print_size(document);
            }
            Event::ReplaceWith => {
                let document = self.live_document(host.document());
                self.update_actions(document);
            }
            Event::Assign => {
                self.assign_to_selected_points(host);
            }
            Event::SetOnStyle => {
                let style = self
                    .target_style
                    .and_then(|at| self.target_styles.get(at))
                    .cloned()
                    .unwrap_or_default();
                self.set_on_style(host, &style);
            }
            Event::Replace => {
                let replacement = self.replace_with.current_name();
                self.replace_in_styles(host, &replacement);
            }
            Event::SelectUsing => {
                self.select_points_using(host);
            }
            Event::Import => self.import_clicked(host),
            Event::Export => self.export_clicked(host),
            Event::Door(what) => {
                if self.context.definition_editor {
                    self.requests.push((what, self.current.clone()));
                }
            }
            Event::Close => self.open = false,
        }
    }

    /// The grid: a cell a symbol, its picture over its name. A fill makes the symbol of that name
    /// current and selects it alone, as a click on it does.
    fn grid_ui(&mut self, ui: &mut Ui, document: Option<&Document>) {
        let name = "symbolGrid";
        let taken = take(ui, name);
        if let Some(taken) = &taken {
            match &taken.posted.act {
                Act::Fill(text) => {
                    if ui.is_visible() && !ui.is_enabled() {
                        settle(ui, taken, Err(fill_disabled(&taken.posted)));
                    } else if let Some(row) = self.model.row_of(text) {
                        self.events.push(Event::Grid { row, toggle: false });
                        settle(ui, taken, Ok(()));
                    } else {
                        settle(ui, taken, Err(lists_no(name, text)));
                    }
                }
                Act::Press => settle(ui, taken, Err(no_button(&taken.posted))),
                Act::ScrollTo => {}
            }
        }
        let toggle = ui.input(|input| input.modifiers.command);
        let ratio = ui.ctx().pixels_per_point();
        let side = SymbolGridModel::PICTURE_SIZE as f32;
        let amber = UNDEFINED_NAME_COLOUR.to_egui();
        let mut clicked = None;
        let output = egui::ScrollArea::vertical()
            .id_salt("symbolGridScroll")
            .max_height(420.0)
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for row in 0..self.model.row_count() {
                        let Some(caption) = self.model.display(row) else {
                            continue;
                        };
                        let texture = self.grid_texture(ui.ctx(), document, row, &caption, ratio);
                        let selected = self.selected.contains(&caption);
                        let missing = self.model.is_missing(row) == Some(true);
                        let tip = self.model.tool_tip(row).unwrap_or_default();
                        ui.vertical(|ui| {
                            // Room for a two-line caption under the picture.
                            ui.set_width(side + 44.0);
                            let (rect, _) = ui
                                .allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
                            if let Some(texture) = texture
                                && ui.is_rect_visible(rect)
                            {
                                ui.painter().image(
                                    texture,
                                    rect,
                                    egui::Rect::from_min_max(
                                        egui::pos2(0.0, 0.0),
                                        egui::pos2(1.0, 1.0),
                                    ),
                                    egui::Color32::WHITE,
                                );
                            }
                            let mut text = RichText::new(caption.as_str());
                            if missing {
                                text = text.color(amber);
                            }
                            if ui
                                .selectable_label(selected, text)
                                .on_hover_text(tip)
                                .clicked()
                            {
                                clicked = Some(row);
                            }
                        });
                    }
                });
            });
        if let Some(row) = clicked {
            self.events.push(Event::Grid { row, toggle });
        }
        let response = ui.interact(output.inner_rect, ui.id().with(name), egui::Sense::hover());
        if let Some(taken) = &taken
            && taken.posted.act == Act::ScrollTo
        {
            response.scroll_to_me(None);
            settle(ui, taken, Ok(()));
        }
        let mut grid = ControlRecord::new(name, ControlKind::Rows);
        grid.items = self.shown_names();
        grid.value = self.current.clone();
        grid.enabled = ui.is_enabled();
        grid.visible = ui.is_visible();
        record(ui, &response, grid);
    }

    /// The texture of the picture at `row`, made when it is first drawn and kept until the
    /// entries change.
    fn grid_texture(
        &mut self,
        ctx: &Context,
        document: Option<&Document>,
        row: usize,
        name: &str,
        ratio: f32,
    ) -> Option<egui::TextureId> {
        if let Some(found) = self.grid_textures.get(name) {
            return Some(found.id());
        }
        let thumbnails = Rc::clone(&self.thumbnails);
        let picture = |entry: &SymbolLibraryEntry| picture_of(document, &thumbnails, entry, ratio);
        let image = self.model.decoration_with(row, &picture)?;
        let handle = ctx.load_texture(
            format!("symbolGrid/{name}"),
            image.to_color_image(),
            egui::TextureOptions::LINEAR,
        );
        let id = handle.id();
        self.grid_textures.insert(String::from(name), handle);
        Some(id)
    }

    /// A row of the details pane: said, or not there.
    fn detail(ui: &mut Ui, name: &str, title: &str, text: &str, warned: bool) {
        let row = |ui: &mut Ui| {
            ui.horizontal_wrapped(|ui| {
                if !title.is_empty() {
                    ui.label(title);
                }
                let mut shown = RichText::new(text);
                if warned {
                    shown = shown.color(UNDEFINED_NAME_COLOUR.to_egui());
                }
                controls::label_as(ui, name, text, egui::Label::new(shown).wrap());
            });
        };
        // A row says something or is not there: a built-in shape has no source file, units or
        // strokes, and empty rows read as values missing.
        if text.is_empty() {
            hidden(ui, name, row);
        } else {
            row(ui);
        }
    }

    fn body(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        let document = self.live_document(host.document());
        ui.horizontal(|ui| {
            ui.add_enabled_ui(self.actions.load, |ui| {
                if controls::button(ui, "importDefinitions", "Import Definitions...")
                    .on_hover_text(
                        "Merge the linestyle and symbol definitions of a Katana customisation \
                         file, and its colours, into this session's: they are added, or replace \
                         those of the same name; nothing else is removed",
                    )
                    .clicked()
                {
                    self.events.push(Event::Import);
                }
            });
            ui.add_enabled_ui(self.actions.export, |ui| {
                if controls::button(ui, "exportSelected", "Export Selected...")
                    .on_hover_text(
                        "Write the selected library symbols to a Katana customisation file",
                    )
                    .clicked()
                {
                    self.events.push(Event::Export);
                }
            });
        });
        ui.horizontal_top(|ui| {
            // Left: the groups.
            ui.vertical(|ui| {
                ui.set_width(190.0);
                let cells: Vec<[String; 1]> = self
                    .groups
                    .iter()
                    .map(|group| [group.label.clone()])
                    .collect();
                let marks: Vec<[bool; 1]> =
                    self.groups.iter().map(|group| [group.marked]).collect();
                let lines: Vec<TreeLine<'_>> = self
                    .groups
                    .iter()
                    .enumerate()
                    .map(|(at, group)| TreeLine {
                        depth: group.depth,
                        cells: &cells[at],
                        marked: &marks[at],
                        bold: false,
                        pictures: &[],
                        tip: "",
                    })
                    .collect();
                let current = self
                    .groups
                    .iter()
                    .position(|group| group.filter == self.group);
                let shown = tree_rows(ui, "symbolGroups", &[], &lines, current);
                if let Some(row) = shown.chosen
                    && Some(row) != current
                {
                    self.events.push(Event::Tree(row));
                }
            });

            // Centre: search, chips, the grid.
            ui.vertical(|ui| {
                ui.set_width(560.0);
                let bar = ui.vertical(|ui| self.filter_bar.ui(ui));
                custom(
                    ui,
                    &bar.response,
                    ControlRecord::new("symbolFilter", ControlKind::Other),
                );
                if bar.inner.any() {
                    self.events.push(Event::Filter);
                }
                self.grid_ui(ui, document);
                controls::label(ui, "symbolCount", &self.count_label);
            });

            // Right: preview, details, actions.
            ui.vertical(|ui| {
                self.preview
                    .ui(ui, "symbolPreview", egui::vec2(240.0, 160.0));
                ui.horizontal(|ui| {
                    ui.label("Plot scale");
                    let scales: Vec<String> = StylePreview::PLOT_SCALES
                        .iter()
                        .map(|denominator| format!("1:{denominator}"))
                        .collect();
                    if index_choice(ui, "plotScale", &scales, &mut self.plot_scale) {
                        self.events.push(Event::PlotScale);
                    }
                    let grounds = [String::from("On paper"), String::from("On screen")];
                    if index_choice(ui, "previewGround", &grounds, &mut self.ground) {
                        self.events.push(Event::Ground);
                    }
                });
                let details = ui.vertical(|ui| {
                    let said = self.details.clone();
                    controls::label_as(
                        ui,
                        "detailName",
                        &said.name,
                        egui::Label::new(RichText::new(said.name.as_str()).strong()).wrap(),
                    );
                    Self::detail(ui, "detailMissing", "", &said.missing, true);
                    Self::detail(ui, "detailPrint", "Prints", &said.print, false);
                    Self::detail(ui, "detailSource", "Source", &said.source, false);
                    Self::detail(ui, "detailGroup", "Group", &said.group, false);
                    Self::detail(ui, "detailUnits", "Units", &said.units, false);
                    Self::detail(ui, "detailMode", "Mode", &said.mode, false);
                    Self::detail(ui, "detailFactor", "Factor", &said.factor, false);
                    Self::detail(
                        ui,
                        "detailOrigin",
                        "Origin",
                        &said.origin,
                        said.origin_warned,
                    );
                    Self::detail(ui, "detailExtent", "Size", &said.extent, false);
                    Self::detail(ui, "detailContent", "Content", &said.content, false);
                    Self::detail(ui, "detailCodes", "Survey codes", &said.codes, false);
                    Self::detail(ui, "detailStyles", "Styles", &said.styles, false);
                });
                custom(
                    ui,
                    &details.response,
                    ControlRecord::new("symbolDetails", ControlKind::Other),
                );

                let actions = ui.group(|ui| {
                    ui.label("Use");
                    ui.horizontal(|ui| {
                        ui.label("Size");
                        ui.add_enabled_ui(self.actions.size, |ui| {
                            if controls::spin(ui, "assignSize", &mut self.size, SIZE)
                                .on_hover_text(
                                    "The symbol's width on the ground; 0 draws it as its \
                                     definition says",
                                )
                                .changed()
                            {
                                self.events.push(Event::Size);
                            }
                        });
                        ui.add_enabled_ui(self.actions.assign, |ui| {
                            if controls::button(ui, "assignToPoints", "Assign to Selected Points")
                                .on_hover_text(self.actions.assign_tip.as_str())
                                .clicked()
                            {
                                self.events.push(Event::Assign);
                            }
                        });
                    });
                    ui.horizontal(|ui| {
                        ui.label("Style");
                        let mut style = controls::Choice {
                            index: self.target_style,
                            ..controls::Choice::default()
                        };
                        if controls::choice(ui, "targetStyle", &self.target_styles, &mut style)
                            .changed()
                        {
                            self.target_style = style.index;
                        }
                        ui.add_enabled_ui(self.actions.set_on_style, |ui| {
                            if controls::button(ui, "setOnStyle", "Set on Style").clicked() {
                                self.events.push(Event::SetOnStyle);
                            }
                        });
                    });
                    ui.horizontal(|ui| {
                        ui.label("With");
                        // Choosing or typing a replacement only enables the button beside it.
                        if self.replace_with.ui(ui, "replaceWith") {
                            self.events.push(Event::ReplaceWith);
                        }
                        ui.add_enabled_ui(self.actions.replace, |ui| {
                            if controls::button(ui, "replaceInStyles", "Replace in Styles")
                                .on_hover_text(
                                    "Every style drawing this symbol draws the one chosen beside \
                                     instead (one undo step)",
                                )
                                .clicked()
                            {
                                self.events.push(Event::Replace);
                            }
                        });
                    });
                    ui.add_enabled_ui(self.actions.select_using, |ui| {
                        if controls::button(ui, "selectPointsUsing", "Select Points Using")
                            .clicked()
                        {
                            self.events.push(Event::SelectUsing);
                        }
                    });
                });
                custom(
                    ui,
                    &actions.response,
                    ControlRecord::new("symbolActions", ControlKind::Other),
                );
            });
        });
    }

    fn footer(&mut self, ui: &mut Ui, _host: &mut dyn Host) {
        // The definition editor's four doors: the editor is the maker's, and a button only asks
        // for it.
        let doors = [
            (
                "symbolNew",
                "New Symbol...",
                "Make a symbol definition in the definition editor",
                DefinitionEdit::NewSymbol,
            ),
            (
                "symbolEdit",
                "Edit Definition...",
                "Change this symbol's definition: its strokes, units and size",
                DefinitionEdit::Edit,
            ),
            (
                "symbolDuplicate",
                "Duplicate Definition...",
                "A copy of this symbol's definition under a new name",
                DefinitionEdit::Duplicate,
            ),
            (
                "symbolDelete",
                "Delete Definition",
                "Take this symbol's definition out of the library. Refused, in the definition \
                 editor, while a survey code, a style or a layer names it",
                DefinitionEdit::Delete,
            ),
        ];
        ui.horizontal(|ui| {
            for (name, label, tip, what) in doors {
                let enabled = if what == DefinitionEdit::NewSymbol {
                    self.actions.new_definition
                } else {
                    self.actions.defined
                };
                ui.add_enabled_ui(enabled, |ui| {
                    if controls::button(ui, name, label)
                        .on_hover_text(tip)
                        .clicked()
                    {
                        self.events.push(Event::Door(what));
                    }
                });
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if controls::button(ui, "closeButton", "Close").clicked() {
                    self.events.push(Event::Close);
                }
            });
        });
    }
}
