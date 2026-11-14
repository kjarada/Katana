//! The styles and linetypes manager (Format > Styles and Linetypes). The port of style_manager.
//!
//! One window for what decides how a line or a point is drawn: the drawing's styles, its own dash
//! linetypes and the session's library linestyles, with a Diagnostics tab for the names nothing
//! defines and the names two sources define (decision D2's collisions). Everything it changes goes
//! through the ordinary commands, so every edit made here is on the same undo stack as one made on
//! the command line - and its Undo and Redo buttons are that stack's.
//!
//! NON-MODAL: it is shown beside the drawing. It hears the Document through a DocumentWatcher, so
//! an undo, an import or a library load made anywhere reloads it once, at its next frame, keeping
//! the selected rows; and every button that acts on the drawing's selection reads the selection at
//! the moment it is pressed.
//!
//! THE QT-02 RULE. A form writes back only the fields a person EDITED (cad's StyleFields); an
//! unedited Save is no command at all (cad's edit_styles_command, update_style_if_changed); and
//! every name field keeps a name it cannot list (NamePicker, kept_name_combo). Selecting several
//! styles shows <varies> for the fields they differ in, and Save leaves those fields of each style
//! alone unless they were edited.
//!
//! No modal box is ever opened by a button: names are asked for in a prompt row inside the dialog
//! and a purge is checked in a panel, so a headless session and a test drive every action by
//! object name.
//!
//! Object names: the dialog "styleManager"; tabs "managerTabs" with pages "stylesPage",
//! "linetypesPage", "hatchPatternsPage" (customisation::hatch_patterns_tab names its own) and
//! "diagnosticsPage". Styles: "styleTable", the chips and "filterText" of its filter bar,
//! "styleFormTitle", "styleLinetype", "styleWeight", "styleColour", "styleColourByLayer",
//! "styleHatch", "styleSymbol", "styleSymbolSize", "styleDescription", "styleSave", "styleRevert",
//! "stylePreview", "stylePreviewScale", "stylePreviewScreen", "stylePreviewNote", "styleNew",
//! "styleDuplicate", "styleRename", "styleMerge", "styleDelete", "stylePurge",
//! "styleApplyToSelection", "styleSelectUsers", "styleMakeCurrent". Linetypes: "linetypeTable",
//! "linetypeGroup", "noLinetype", "patternGrid" with "patternKind<row>" and "patternLength<row>",
//! "patternAddDash", "patternAddGap", "patternAddDot", "patternRemove", "patternUp",
//! "patternDown", "patternStatus", "linetypeDescription", "linetypeSave", "linetypeRevert",
//! "libraryDetails", "newStyleUsing", "linetypeEditDefinition", "linetypePreview",
//! "linetypePreviewScale", "linetypePreviewScreen", "linetypeNew", "linetypeDuplicate",
//! "linetypeRename", "linetypeMerge", "linetypeDelete", "linetypePurge", "linetypeNewDefinition",
//! "linetypeSelectUsers". Diagnostics: "diagnosticSummary", "diagnosticTable",
//! "diagnosticSelectUsers". Shared: "promptPanel", "promptLabel", "promptName", "promptChoice",
//! "promptOk", "promptCancel", "purgePanel", "purgeList", "purgeOk", "purgeCancel", "undoButton",
//! "redoButton", "statusLine", "closeButton".
//!
//! What is the C++'s and not here. The tables are rows of text: the Preview column's pictures, the
//! colour swatches, the amber of a name nothing defines and the bold of the current style are not
//! drawn, and a table is listed by name and not sorted by a click on a header. The two filter bars
//! carry the same object names ("filterText", "filterAll" ...), which the C++ tells apart by their
//! page; a drive target cannot hold a name twice, so only the bar of the page in front is drawn,
//! the rest of each page being there to fill and press whichever is in front. The colour menu of
//! "styleColour" is a row of the standard colours and egui's colour button, shown under it while
//! it is open. A number box showing <varies> says so beside it. The library details are plain
//! lines, where the C++ label was rich text. As in the C++, this dialog runs its commands on the
//! Document itself, not as lines through the executor.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use egui::{Context, Ui};
use katana_cad::document::Document;
use katana_cad::purge::{PurgeOptions, plan_purge};
use katana_cad::style_catalogue::{NameRole, free_style_name};
use katana_cad::style_manager_rows::{
    LinetypeOrigin, LinetypeRow, LinetypeRowKind, StyleDiagnostic, StyleDiagnosticKind,
    StyleFields, StyleFilter, StyleRow, UsageTable, apply_edit, common_fields,
    edit_styles_command, entities_using, free_linetype_name, linetype_rows, matches_filter,
    matches_search, style_diagnostics, style_rows,
};
use katana_commands::{
    CommandPtr, TableItems, Transaction, create_linetype, create_style, delete_linetype,
    delete_style, duplicate_linetype, duplicate_style, merge_linetype, merge_style,
    purge_table_items, rename_linetype, rename_style, set_entity_style,
    update_linetype_if_changed,
};
use katana_core::text::{format_general, lowered};
use katana_entity::display::is_by_layer;
use katana_entity::style_library::style_groups;
use katana_entity::tables::CONTINUOUS_LINETYPE;
use katana_entity::{Color, EntityId, Linetype, LinetypeElement, Style, validate};

use crate::annotation::dialog_support::{show_dialog, spin_value, table};
use crate::customisation::customisation_context::{CustomisationContext, DefinitionEdit};
use crate::customisation::definition_thumbnails::DefinitionThumbnails;
use crate::customisation::document_watcher::{DocumentChanges, DocumentWatcher};
use crate::customisation::filter_bar::FilterBar;
use crate::customisation::hatch_patterns_tab::HatchPatternsTab;
use crate::customisation::name_picker::NamePicker;
use crate::customisation::style_painter::CanvasRasterBackend;
use crate::customisation::style_preview::{PreviewGround, StylePreview};
use crate::drive::controls::{self, Choice, SpinRange, custom};
use crate::drive::registry::{ControlKind, ControlRecord};
use crate::format::qt_fixed;
use crate::host::Host;
use crate::kept_name_combo::{KeptChoice, KeptNameCombo, VARIES};
use crate::shell::DialogFrame;
use crate::survey::survey_dialogs::offstage;

/// The dialog's object name.
pub const STYLE_MANAGER: &str = "styleManager";
const TITLE: &str = "Styles and Linetypes";
/// The error colour of a refusal in the dialog's own status line and of an invalid pattern: the
/// theme has no token for it, and the log panel's red is the main window's own.
const ERROR_COLOUR: egui::Color32 = egui::Color32::from_rgb(0xE0, 0x5A, 0x4E);
const WEIGHT: SpinRange = SpinRange::new(0.0, 10.0, 0.05, 3).with_suffix(" mm on paper");
const SYMBOL_SIZE: SpinRange =
    SpinRange::new(0.0, 10000.0, 1.0, 4).with_suffix(" m wide (0 = its own size)");
const PATTERN_LENGTH: SpinRange = SpinRange::new(0.0, 1.0e6, 0.1, 4).with_suffix(" m");
const STYLE_HEADERS: [&str; 9] = [
    "Name",
    "Preview",
    "Linetype",
    "Weight",
    "Colour",
    "Hatch",
    "Symbol",
    "Used",
    "Description",
];
const LINETYPE_HEADERS: [&str; 7] = [
    "Name",
    "Source",
    "Group",
    "Kind",
    "Period",
    "Used",
    "Description",
];
const DIAGNOSTIC_HEADERS: [&str; 4] = ["Problem", "Name", "Used by", "Drawn as"];

/// The standard colours the style colour menu offers, by the standard colour names.
const STANDARD_COLOURS: [(&str, Color); 12] = [
    ("red", Color::new(255, 0, 0, 255)),
    ("yellow", Color::new(255, 255, 0, 255)),
    ("green", Color::new(0, 255, 0, 255)),
    ("cyan", Color::new(0, 255, 255, 255)),
    ("blue", Color::new(0, 0, 255, 255)),
    ("magenta", Color::new(255, 0, 255, 255)),
    ("white", Color::new(255, 255, 255, 255)),
    ("black", Color::new(0, 0, 0, 255)),
    ("grey", Color::new(128, 128, 128, 255)),
    ("orange", Color::new(255, 128, 0, 255)),
    ("brown", Color::new(150, 75, 0, 255)),
    ("light grey", Color::new(192, 192, 192, 255)),
];

/// The dialog's tabs, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tab {
    Styles,
    Linetypes,
    HatchPatterns,
    Diagnostics,
}

/// A length as a person reads it: no trailing zeros, at most six figures.
fn number(value: f64) -> String {
    format_general(value, 6)
}

fn period_text(row: &LinetypeRow) -> String {
    match row.kind {
        LinetypeRowKind::Dash => {
            if row.period > 0.0 {
                format!("{} m", number(row.period))
            } else {
                String::from("continuous")
            }
        }
        LinetypeRowKind::Paper => {
            if row.period > 0.0 {
                format!("{} mm", number(row.period))
            } else {
                String::from("-")
            }
        }
        LinetypeRowKind::World => {
            if row.period > 0.0 {
                format!("{} m", number(row.period))
            } else {
                String::from("-")
            }
        }
        LinetypeRowKind::TwoPoint => String::from("stretched"),
    }
}

fn kind_text(kind: LinetypeRowKind) -> &'static str {
    match kind {
        LinetypeRowKind::Dash => "dash pattern",
        LinetypeRowKind::Paper => "paper",
        LinetypeRowKind::World => "world",
        LinetypeRowKind::TwoPoint => "two-point",
    }
}

fn kind_explained(kind: LinetypeRowKind) -> &'static str {
    match kind {
        LinetypeRowKind::Dash => "a dash pattern in model units, saved with the drawing",
        LinetypeRowKind::Paper => {
            "Paper linestyle: millimetres on the plot, the same at every scale"
        }
        LinetypeRowKind::World => {
            "World linestyle: model units, the same on the ground at every scale"
        }
        LinetypeRowKind::TwoPoint => {
            "Two-point linestyle: stretched between the two points it is drawn across"
        }
    }
}

fn entities_selected(count: usize) -> String {
    format!(
        "{count} entit{} selected.",
        if count == 1 { "y" } else { "ies" }
    )
}

/// What an element of a pattern is, in the order the grid's kind boxes list them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Element {
    Dash,
    Gap,
    Dot,
}

const ELEMENTS: [Element; 3] = [Element::Dash, Element::Gap, Element::Dot];

fn element_of(length: f64) -> Element {
    if length > 0.0 {
        Element::Dash
    } else if length < 0.0 {
        Element::Gap
    } else {
        Element::Dot
    }
}

/// What the Linetypes tab's detail pane shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Detail {
    /// "Select a linetype.", or how many are selected.
    None,
    Pattern,
    Library,
}

/// What the prompt row's answer is for.
#[derive(Clone, Debug, PartialEq, Eq)]
enum PromptFor {
    NewStyle,
    DuplicateStyle(String),
    RenameStyle(String),
    MergeStyles(Vec<String>),
    NewStyleUsing(String),
    NewLinetype,
    DuplicateLinetype(String),
    RenameLinetype(String),
    MergeLinetypes(Vec<String>),
}

/// One row of the purge panel: what it says, the item, its table (0 a style, 1 a linetype, 2 a
/// hatch pattern) and whether it is checked.
#[derive(Clone, Debug, PartialEq, Eq)]
struct PurgeItem {
    label: String,
    name: String,
    table: u8,
    checked: bool,
}

/// What each button may do, as the last look at the drawing left it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Buttons {
    duplicate_style: bool,
    rename_style: bool,
    merge_styles: bool,
    delete_styles: bool,
    select_style_users: bool,
    make_current: bool,
    make_current_text: &'static str,
    make_current_tip: &'static str,
    apply_text: String,
    apply: bool,
    save_style: bool,
    revert_style: bool,
    duplicate_linetype: bool,
    rename_linetype: bool,
    merge_linetypes: bool,
    delete_linetypes: bool,
    select_linetype_users: bool,
    save_linetype: bool,
    revert_linetype: bool,
    remove_element: bool,
    move_up: bool,
    move_down: bool,
    select_diagnostic_users: bool,
    undo: bool,
    redo: bool,
}

/// A click on row `row` of a table that takes several: alone it is the selection; with the
/// command key held it is added to the selection, or taken from it. True when the selection
/// changed.
fn pick(selected: &mut Vec<usize>, row: usize, adding: bool) -> bool {
    let before = selected.clone();
    if adding {
        match selected.iter().position(|&kept| kept == row) {
            Some(at) => {
                selected.remove(at);
            }
            None => selected.push(row),
        }
        selected.sort_unstable();
    } else {
        *selected = vec![row];
    }
    *selected != before
}

/// A page's own record, so a driver finds the page as the C++ finds the named widget.
fn page_record(ui: &mut Ui, name: &str) {
    let page = ui.allocate_response(egui::Vec2::ZERO, egui::Sense::hover());
    custom(ui, &page, ControlRecord::new(name, ControlKind::Other));
}

/// See the module comment.
pub struct StyleManagerDialog {
    context: CustomisationContext,
    frame: DialogFrame,
    tab: usize,
    diagnostics_title: String,
    status: String,
    status_is_error: bool,

    // ---- styles
    style_rows: Vec<StyleRow>,
    /// The rows the table shows, filtered, in name order: places in `style_rows`.
    style_view: Vec<usize>,
    /// The rows selected: places in `style_rows`, ascending.
    style_selected: Vec<usize>,
    style_filter: FilterBar,
    form_title: String,
    fields_enabled: bool,
    linetype_picker: NamePicker,
    symbol_picker: NamePicker,
    weight: f64,
    weight_varies: bool,
    symbol_size: f64,
    symbol_size_varies: bool,
    /// Whether the colour is ByLayer; None while the styles do not agree.
    by_layer: Option<bool>,
    colour_menu_open: bool,
    hatch: KeptNameCombo,
    description: String,
    description_placeholder: String,
    style_preview: StylePreview,
    preview_note: String,
    preview_scale: usize,
    preview_screen: bool,
    /// The styles the form was loaded for, and what a person changed since.
    form_styles: Vec<String>,
    edited: StyleFields,
    /// The colour the form shows when it is not ByLayer.
    picked_colour: Option<Color>,

    // ---- linetypes
    linetype_rows: Vec<LinetypeRow>,
    linetype_view: Vec<usize>,
    linetype_selected: Vec<usize>,
    linetype_filter: FilterBar,
    /// The group box: what each item says and the group it filters to ("" for all).
    groups: Vec<(String, String)>,
    group: usize,
    detail: Detail,
    no_linetype: String,
    /// The grid's rows as its boxes show them: the kind each kind box shows, the length each
    /// length box shows, and whether that box is enabled.
    pattern_kinds: Vec<Element>,
    pattern_lengths: Vec<f64>,
    pattern_length_enabled: Vec<bool>,
    pattern_current: Option<usize>,
    pattern_enabled: bool,
    pattern_status: String,
    pattern_status_is_error: bool,
    linetype_description: String,
    library_details: String,
    linetype_preview: StylePreview,
    linetype_preview_scale: usize,
    linetype_preview_screen: bool,
    /// The drawing linetype the grid was loaded for, its elements as edited (exact: a length
    /// nobody touched is the stored number, not what a box rounded it to), and which parts a
    /// person changed.
    pattern_for: String,
    working_pattern: Vec<f64>,
    pattern_edited: bool,
    description_edited: bool,
    pattern_valid: bool,

    // ---- diagnostics
    diagnostic_rows: Vec<StyleDiagnostic>,
    diagnostic_selected: Vec<usize>,
    diagnostic_summary: String,

    hatch_patterns: HatchPatternsTab,

    // ---- the prompt row and the purge panel, shared by the tabs
    prompt: Option<PromptFor>,
    prompt_label: String,
    prompt_name: String,
    /// The prompt asks for a choice from prompt_choices rather than a typed name.
    prompt_is_choice: bool,
    prompt_choices: Vec<String>,
    prompt_choice: Option<usize>,
    purge_open: bool,
    purge_label: String,
    purge_items: Vec<PurgeItem>,
    purge_options: PurgeOptions,

    buttons: Buttons,
    /// Set once the dialog has found its Document gone and shut itself down.
    orphan: bool,
    /// What the next reload selects, after a command this dialog ran.
    select_styles_after_reload: Option<Vec<String>>,
    select_linetype_after_reload: Option<(String, LinetypeOrigin)>,
    closing: bool,
    // Last, so it goes first: no delivery reaches a half-dropped dialog.
    watcher: DocumentWatcher,
}

impl StyleManagerDialog {
    /// `context.thumbnails` may be None (the dialog then keeps its own cache); `select_and_show`
    /// may be None (Select Users then only selects). The Document may be dropped before the
    /// dialog: the dialog then does nothing more.
    pub fn new(context: &CustomisationContext, document: &Document) -> Self {
        let mut context = context.clone();
        if context.thumbnails.is_none() {
            // Used when the maker supplied no cache, so the pickers still have pictures.
            context.thumbnails = Some(Rc::new(RefCell::new(DefinitionThumbnails::new(
                Arc::new(CanvasRasterBackend),
            ))));
        }
        let scale = StylePreview::PLOT_SCALES
            .iter()
            .position(|&denominator| denominator == 500)
            .unwrap_or(0);
        let mut style_filter = FilterBar::new(&["All", "Used", "Unused", "Missing"]);
        style_filter.set_placeholder_text("Search names, linetypes, symbols and descriptions");
        let mut linetype_filter = FilterBar::new(&["All", "Drawing", "Library", "Used", "Unused"]);
        linetype_filter.set_placeholder_text("Search names, groups, files and descriptions");
        let mut dialog = Self {
            frame: DialogFrame::new(STYLE_MANAGER, TITLE),
            tab: 0,
            diagnostics_title: String::from("Diagnostics"),
            status: String::new(),
            status_is_error: false,
            style_rows: Vec::new(),
            style_view: Vec::new(),
            style_selected: Vec::new(),
            style_filter,
            form_title: String::new(),
            fields_enabled: false,
            linetype_picker: NamePicker::new(&context, document, NameRole::Linetype, true),
            symbol_picker: NamePicker::new(&context, document, NameRole::Symbol, false),
            weight: 0.25,
            weight_varies: false,
            symbol_size: 0.0,
            symbol_size_varies: false,
            by_layer: Some(true),
            colour_menu_open: false,
            hatch: KeptNameCombo::new(),
            description: String::new(),
            description_placeholder: String::new(),
            style_preview: StylePreview::new(document),
            preview_note: String::new(),
            preview_scale: scale,
            preview_screen: false,
            form_styles: Vec::new(),
            edited: StyleFields::default(),
            picked_colour: None,
            linetype_rows: Vec::new(),
            linetype_view: Vec::new(),
            linetype_selected: Vec::new(),
            linetype_filter,
            groups: Vec::new(),
            group: 0,
            detail: Detail::None,
            no_linetype: String::from("Select a linetype."),
            pattern_kinds: Vec::new(),
            pattern_lengths: Vec::new(),
            pattern_length_enabled: Vec::new(),
            pattern_current: None,
            pattern_enabled: true,
            pattern_status: String::new(),
            pattern_status_is_error: false,
            linetype_description: String::new(),
            library_details: String::new(),
            linetype_preview: StylePreview::new(document),
            linetype_preview_scale: scale,
            linetype_preview_screen: false,
            pattern_for: String::new(),
            working_pattern: Vec::new(),
            pattern_edited: false,
            description_edited: false,
            pattern_valid: true,
            diagnostic_rows: Vec::new(),
            diagnostic_selected: Vec::new(),
            diagnostic_summary: String::new(),
            hatch_patterns: HatchPatternsTab::new(&context, document),
            prompt: None,
            prompt_label: String::new(),
            prompt_name: String::new(),
            prompt_is_choice: false,
            prompt_choices: Vec::new(),
            prompt_choice: None,
            purge_open: false,
            purge_label: String::new(),
            purge_items: Vec::new(),
            purge_options: PurgeOptions::default(),
            buttons: Buttons::default(),
            orphan: false,
            select_styles_after_reload: None,
            select_linetype_after_reload: None,
            closing: false,
            watcher: DocumentWatcher::new(document),
            context,
        };
        dialog.reload(document, true);
        dialog
    }

    // ---- the Document, which may have gone -----------------------------------------------------

    fn alive(&self) -> bool {
        self.watcher.document_alive()
    }

    /// `alive`, and when the Document has gone, the dialog shut down as well. What a person can
    /// still reach asks this before it reads the Document.
    fn live(&mut self) -> bool {
        if self.alive() {
            return true;
        }
        self.orphaned();
        false
    }

    /// The Document has gone: the dialog may outlive it, and no delivery tells it so - the
    /// watcher simply stops - so the first thing a person does afterwards finds out. Once: every
    /// control but Close is disabled, the panels close and the previews let go, so nothing left on
    /// screen can reach the Document; the status line says why. Not through the log: that belongs
    /// to whoever owned the Document, and may have gone with it.
    fn orphaned(&mut self) {
        if self.orphan {
            return;
        }
        self.orphan = true;
        self.prompt = None;
        self.purge_open = false;
        self.colour_menu_open = false;
        self.buttons = Buttons::default();
        self.style_preview.clear();
        self.linetype_preview.clear();
        self.status = String::from(
            "The drawing this dialog edited has closed: nothing here acts on anything any more.",
        );
        self.status_is_error = false;
    }

    fn log(&mut self, host: &mut dyn Host, message: &str, is_error: bool) {
        self.status = String::from(message);
        self.status_is_error = is_error;
        host.log(message, is_error);
    }

    /// Runs a command and says what it did, or why not. The reload that shows its effect comes
    /// from the watcher, once, at the next frame.
    fn run(&mut self, host: &mut dyn Host, command: Option<CommandPtr>, done: &str) -> bool {
        if !self.alive() {
            return false;
        }
        let Some(command) = command else {
            self.log(host, "Nothing to do.", false);
            return false;
        };
        if let Err(error) = host.document_mut().execute(command) {
            self.log(host, &error.describe(), true);
            return false;
        }
        self.log(host, done, false);
        true
    }

    /// Several commands as one undo step; the one command itself when there is only one.
    fn together(name: &str, mut parts: Vec<CommandPtr>) -> Option<CommandPtr> {
        if parts.len() == 1 {
            return parts.pop();
        }
        let mut transaction = Transaction::new(name);
        for part in parts {
            transaction.add(part);
        }
        Some(Box::new(transaction))
    }

    // ---- selection -----------------------------------------------------------------------------

    /// The status line.
    pub fn status(&self) -> &str {
        &self.status
    }

    /// Brings a tab to the front, as a click on it does.
    pub fn show_tab(&mut self, tab: Tab) {
        self.tab = match tab {
            Tab::Styles => 0,
            Tab::Linetypes => 1,
            Tab::HatchPatterns => 2,
            Tab::Diagnostics => 3,
        };
    }

    /// The styles selected in the Styles tab, by name, in table order.
    pub fn selected_styles(&self) -> Vec<String> {
        self.style_selected
            .iter()
            .filter_map(|&row| self.style_rows.get(row))
            .map(|row| row.style.name.clone())
            .collect()
    }

    /// Replaces the styles selected (names the table lacks are ignored). For the workbench ("open
    /// on this style") and for tests.
    pub fn select_styles(&mut self, document: &Document, names: &[String]) {
        // The rows are selected whatever became of the Document; what follows a selection reads
        // it, and so asks first.
        if self.select_style_names(names) && self.live() {
            self.load_style_form(document);
        }
    }

    /// The linetypes selected in the Linetypes tab, where a name may be both a drawing linetype
    /// and a library linestyle (a D2 collision): the origin says which.
    pub fn selected_linetypes(&self) -> Vec<(String, LinetypeOrigin)> {
        self.selected_linetype_rows()
            .into_iter()
            .map(|row| (row.name.clone(), row.origin))
            .collect()
    }

    pub fn select_linetype(&mut self, document: &Document, name: &str, origin: LinetypeOrigin) {
        self.select_linetypes(document, &[(String::from(name), origin)]);
    }

    /// Replaces the linetypes selected, as clicks with the command key held gather several.
    pub fn select_linetypes(&mut self, document: &Document, wanted: &[(String, LinetypeOrigin)]) {
        if self.select_linetype_rows(wanted) && self.live() {
            self.load_linetype_form(document);
            self.update_buttons(document);
        }
    }

    /// For the headless screenshot: selects the first row of the styles and the linetypes tables,
    /// so a grab shows the forms and previews filled in.
    pub fn show_first_rows(&mut self, document: &Document) {
        if !self.live() {
            return;
        }
        if let Some(&first) = self.style_view.first()
            && self.style_selected != [first]
        {
            self.style_selected = vec![first];
            self.load_style_form(document);
        }
        if let Some(&first) = self.linetype_view.first()
            && self.linetype_selected != [first]
        {
            self.linetype_selected = vec![first];
            self.load_linetype_form(document);
            self.update_buttons(document);
        }
    }

    /// The pattern grid's element made current, as a click on its row does.
    pub fn set_pattern_current(&mut self, document: &Document, row: usize) {
        if row < self.working_pattern.len() {
            self.pattern_current = Some(row);
            self.update_buttons(document);
        }
    }

    /// The linetype and the symbol pickers of the style form, for a test reading what they keep.
    pub fn linetype_picker(&self) -> &NamePicker {
        &self.linetype_picker
    }

    pub fn symbol_picker(&self) -> &NamePicker {
        &self.symbol_picker
    }

    /// Whether the prompt row is shown.
    pub fn prompt_open(&self) -> bool {
        self.prompt.is_some()
    }

    /// The hatch patterns tab.
    pub fn hatch_patterns(&mut self) -> &mut HatchPatternsTab {
        &mut self.hatch_patterns
    }

    fn selected_linetype_rows(&self) -> Vec<&LinetypeRow> {
        self.linetype_selected
            .iter()
            .filter_map(|&row| self.linetype_rows.get(row))
            .collect()
    }

    /// The selected drawing linetypes' names; empty when a library row is among the selection,
    /// since what acts on these acts on the model.
    fn selected_drawing_linetypes(&self) -> Vec<String> {
        let mut names = Vec::new();
        for row in self.selected_linetype_rows() {
            if row.origin != LinetypeOrigin::Drawing {
                return Vec::new();
            }
            names.push(row.name.clone());
        }
        names
    }

    /// Selects the rows the table shows whose style is one of `names`, replacing the selection.
    /// True when the selection changed.
    fn select_style_names(&mut self, names: &[String]) -> bool {
        let mut selected: Vec<usize> = self
            .style_view
            .iter()
            .copied()
            .filter(|&row| names.contains(&self.style_rows[row].style.name))
            .collect();
        selected.sort_unstable();
        let changed = selected != self.style_selected;
        self.style_selected = selected;
        changed
    }

    fn select_linetype_rows(&mut self, wanted: &[(String, LinetypeOrigin)]) -> bool {
        let mut selected: Vec<usize> = self
            .linetype_view
            .iter()
            .copied()
            .filter(|&row| {
                let listed = &self.linetype_rows[row];
                wanted
                    .iter()
                    .any(|(name, origin)| *name == listed.name && *origin == listed.origin)
            })
            .collect();
        selected.sort_unstable();
        let changed = selected != self.linetype_selected;
        self.linetype_selected = selected;
        changed
    }

    // ---- the filters ---------------------------------------------------------------------------

    fn style_filter_of(&self) -> StyleFilter {
        // The chips are in StyleFilter's order: All, Used, Unused, Missing.
        match self.style_filter.chip() {
            Some(1) => StyleFilter::Used,
            Some(2) => StyleFilter::Unused,
            Some(3) => StyleFilter::Missing,
            _ => StyleFilter::All,
        }
    }

    fn style_filtering(&self) -> bool {
        self.style_filter_of() != StyleFilter::All || !self.style_filter.text().is_empty()
    }

    /// The rows the Styles table shows: those the chip and the search take, by name with case
    /// folded. Rows filtered out leave the selection, as a proxy's removed rows do; true when
    /// that changed it.
    fn refilter_styles(&mut self) -> bool {
        let filter = self.style_filter_of();
        let search = String::from(self.style_filter.text());
        let mut view: Vec<usize> = (0..self.style_rows.len())
            .filter(|&row| {
                let listed = &self.style_rows[row];
                matches_filter(listed, filter) && matches_search(listed, &search)
            })
            .collect();
        view.sort_by_cached_key(|&row| {
            let name = &self.style_rows[row].style.name;
            (name.to_lowercase(), name.clone())
        });
        self.style_view = view;
        let before = self.style_selected.len();
        let view = &self.style_view;
        self.style_selected.retain(|row| view.contains(row));
        self.style_selected.len() != before
    }

    fn group_value(&self) -> String {
        self.groups
            .get(self.group)
            .map(|(_, value)| value.clone())
            .unwrap_or_default()
    }

    /// The Linetypes tab's filter, from what its chips, search box and group box show.
    fn refilter_linetypes(&mut self) -> bool {
        let chip = self.linetype_filter.chip().unwrap_or(0);
        let search = lowered(self.linetype_filter.text());
        let group = self.group_value();
        let has = |text: &str| {
            let folded = lowered(text);
            folded
                .windows(search.len().max(1))
                .any(|window| window == search.as_slice())
        };
        let mut view: Vec<usize> = (0..self.linetype_rows.len())
            .filter(|&at| {
                let row = &self.linetype_rows[at];
                let chip_takes = match chip {
                    1 => row.origin == LinetypeOrigin::Drawing,
                    2 => row.origin == LinetypeOrigin::Library,
                    3 => row.users.used(),
                    4 => !row.users.used(),
                    _ => true,
                };
                if !chip_takes {
                    return false;
                }
                // A group and everything under it: "Services" holds "Services/WATR".
                if !group.is_empty()
                    && row.group != group
                    && !row.group.starts_with(&format!("{group}/"))
                {
                    return false;
                }
                search.is_empty()
                    || has(&row.name)
                    || has(&row.group)
                    || has(&row.source_file)
                    || has(&row.description)
            })
            .collect();
        view.sort_by_cached_key(|&at| {
            let name = &self.linetype_rows[at].name;
            (name.to_lowercase(), name.clone())
        });
        self.linetype_view = view;
        let before = self.linetype_selected.len();
        let view = &self.linetype_view;
        self.linetype_selected.retain(|row| view.contains(row));
        self.linetype_selected.len() != before
    }

    // ---- reload --------------------------------------------------------------------------------

    fn on_document_changed(&mut self, document: &Document, changes: DocumentChanges) {
        if !self.alive() {
            return;
        }
        if changes.model || changes.library || changes.survey_map || changes.current {
            self.reload(
                document,
                changes.library || changes.survey_map || changes.model,
            );
        }
        self.update_buttons(document);
    }

    /// Rebuilds the three tables from the Document, keeping what was selected.
    fn reload(&mut self, document: &Document, refresh_pickers: bool) {
        let styles = self
            .select_styles_after_reload
            .take()
            .unwrap_or_else(|| self.selected_styles());
        // Every selected row, by name and source, as the Styles tab keeps its own: a merge or
        // delete being gathered is not dropped because something elsewhere changed the drawing.
        let linetypes = match self.select_linetype_after_reload.take() {
            Some(one) => vec![one],
            None => self.selected_linetypes(),
        };
        let diagnostics: Vec<(StyleDiagnosticKind, String)> = self
            .diagnostic_selected
            .iter()
            .filter_map(|&row| self.diagnostic_rows.get(row))
            .map(|row| (row.kind, row.name.clone()))
            .collect();

        self.style_rows = style_rows(document);
        self.linetype_rows = linetype_rows(document);
        self.diagnostic_rows = style_diagnostics(document);
        self.style_selected.clear();
        self.linetype_selected.clear();
        self.fill_groups(document);
        self.fill_hatches(document);
        if refresh_pickers {
            self.linetype_picker.refresh(document);
            self.symbol_picker.refresh(document);
        }
        self.refilter_styles();
        self.refilter_linetypes();
        if !styles.is_empty() && self.style_filtering() {
            // A style asked for but filtered out is shown by clearing the filter: selecting what
            // cannot be seen would edit it unseen.
            let hidden = styles.iter().any(|name| {
                self.style_rows
                    .iter()
                    .position(|row| row.style.name == *name)
                    .is_some_and(|row| !self.style_view.contains(&row))
            });
            if hidden {
                self.style_filter.set_text("");
                self.style_filter.set_chip(0);
                // Heard here, not at the next frame: the selection below needs the rows shown.
                let _ = self.style_filter.take_changes();
                self.refilter_styles();
            }
        }
        self.select_style_names(&styles);
        self.select_linetype_rows(&linetypes);
        self.diagnostic_selected = (0..self.diagnostic_rows.len())
            .filter(|&at| {
                let row = &self.diagnostic_rows[at];
                diagnostics
                    .iter()
                    .any(|(kind, name)| *kind == row.kind && *name == row.name)
            })
            .collect();

        // The form follows the selection - unless a person is part-way through editing the very
        // styles still selected, whose edits a reload caused elsewhere must not throw away.
        if self.selected_styles() != self.form_styles || self.edited.empty() {
            self.load_style_form(document);
        } else {
            self.refresh_style_preview(document);
        }
        let same_linetype = {
            let rows = self.selected_linetype_rows();
            rows.len() == 1
                && rows[0].origin == LinetypeOrigin::Drawing
                && rows[0].name == self.pattern_for
        };
        if !same_linetype || !(self.pattern_edited || self.description_edited) {
            self.load_linetype_form(document);
        } else {
            self.refresh_linetype_preview();
        }
        self.update_counts();
        self.update_buttons(document);
    }

    fn update_counts(&mut self) {
        let total = self.style_rows.len();
        let used = self.style_rows.iter().filter(|row| row.entities > 0).count();
        let missing = self.style_rows.iter().filter(|row| row.missing()).count();
        self.style_filter
            .set_chip_label(0, &format!("All ({total})"));
        self.style_filter
            .set_chip_label(1, &format!("Used ({used})"));
        self.style_filter
            .set_chip_label(2, &format!("Unused ({})", total - used));
        self.style_filter
            .set_chip_label(3, &format!("Missing ({missing})"));

        let linetypes = self.linetype_rows.len();
        let drawing = self
            .linetype_rows
            .iter()
            .filter(|row| row.origin == LinetypeOrigin::Drawing)
            .count();
        let named = self
            .linetype_rows
            .iter()
            .filter(|row| row.users.used())
            .count();
        self.linetype_filter
            .set_chip_label(0, &format!("All ({linetypes})"));
        self.linetype_filter
            .set_chip_label(1, &format!("Drawing ({drawing})"));
        self.linetype_filter
            .set_chip_label(2, &format!("Library ({})", linetypes - drawing));
        self.linetype_filter
            .set_chip_label(3, &format!("Used ({named})"));
        self.linetype_filter
            .set_chip_label(4, &format!("Unused ({})", linetypes - named));

        let collisions = self
            .diagnostic_rows
            .iter()
            .filter(|row| row.kind == StyleDiagnosticKind::Collision)
            .count();
        let missing_names = self.diagnostic_rows.len() - collisions;
        if missing_names + collisions == 0 {
            self.diagnostics_title = String::from("Diagnostics");
            self.diagnostic_summary = String::from(
                "No problems: every linetype and symbol a style or layer names is defined, and no \
                 drawing linetype shares its name with a library linestyle.",
            );
        } else {
            self.diagnostics_title = format!("Diagnostics ({})", missing_names + collisions);
            self.diagnostic_summary = format!(
                "{missing_names} name(s) that nothing defines - drawn by a fallback until a \
                 library defining them is loaded or the style is changed - and {collisions} \
                 drawing linetype(s) a library linestyle of the same name shadows (the library's \
                 is what is drawn)."
            );
        }
    }

    fn fill_groups(&mut self, document: &Document) {
        let was = self.group_value();
        self.groups = vec![(String::from("All groups"), String::new())];
        for group in style_groups(document.style_library()) {
            self.groups.push((group.clone(), group));
        }
        // A group a library replaced falls back to All groups, and the table is filtered by what
        // the box now shows: left filtered by a group the box no longer shows, it is often down to
        // nothing.
        self.group = self
            .groups
            .iter()
            .position(|(_, value)| *value == was)
            .unwrap_or(0);
    }

    fn fill_hatches(&mut self, document: &Document) {
        let mut choices = vec![KeptChoice::new("(ByLayer)", "")];
        for name in document.model().hatch_patterns.names() {
            choices.push(KeptChoice::new(&name, &name));
        }
        let was = self.hatch.current();
        let varies = !self.hatch.rows().is_empty() && was.is_none();
        self.hatch.fill(&choices);
        if varies {
            self.hatch.show_varies();
        } else {
            self.hatch.show(&was.unwrap_or_default());
        }
    }

    // ---- the style form ------------------------------------------------------------------------

    fn styles_named(document: &Document, names: &[String]) -> Vec<Style> {
        names
            .iter()
            .filter_map(|name| document.model().styles.find(name))
            .cloned()
            .collect()
    }

    fn show_picker(
        picker: &mut NamePicker,
        document: &Document,
        value: Option<&str>,
        placeholder: &str,
    ) {
        picker.set_placeholder_text(if value.is_some() {
            placeholder
        } else {
            VARIES
        });
        picker.set_current_name(document, value.unwrap_or(""));
    }

    /// What the colour button says: the colour, ByLayer, or that the styles differ.
    fn colour_text(&self) -> String {
        match self.by_layer {
            None => String::from(VARIES),
            Some(true) => String::from("ByLayer"),
            Some(false) => self
                .picked_colour
                .unwrap_or(Color::new(255, 255, 255, 255))
                .to_hex(),
        }
    }

    fn load_style_form(&mut self, document: &Document) {
        if !self.live() {
            return;
        }
        self.form_styles = self.selected_styles();
        self.edited = StyleFields::default();
        let styles = Self::styles_named(document, &self.form_styles);
        let common = common_fields(&styles);
        let any = !styles.is_empty();

        self.fields_enabled = any;
        self.form_title = if !any {
            String::from("Select a style to edit it, or several to edit them together.")
        } else if styles.len() == 1 {
            styles[0].name.clone()
        } else {
            format!(
                "{} styles - a field showing <varies> differs between them and is left as each \
                 has it unless you change it.",
                styles.len()
            )
        };
        Self::show_picker(
            &mut self.linetype_picker,
            document,
            if any {
                common.linetype.as_deref()
            } else {
                Some("")
            },
            NamePicker::default_placeholder(NameRole::Linetype),
        );
        Self::show_picker(
            &mut self.symbol_picker,
            document,
            if any {
                common.symbol.as_deref()
            } else {
                Some("")
            },
            NamePicker::default_placeholder(NameRole::Symbol),
        );
        let weight = if any { common.line_weight } else { Some(0.25) };
        self.weight_varies = weight.is_none();
        self.weight = spin_value(&WEIGHT, weight.unwrap_or(0.0));
        let symbol_size = if any { common.symbol_size } else { Some(0.0) };
        self.symbol_size_varies = symbol_size.is_none();
        self.symbol_size = spin_value(&SYMBOL_SIZE, symbol_size.unwrap_or(0.0));
        match (any, common.color) {
            (true, None) => {
                self.by_layer = None;
                self.picked_colour = None;
            }
            (true, Some(colour)) => {
                self.by_layer = Some(colour.is_none());
                self.picked_colour = colour;
            }
            (false, _) => {
                self.by_layer = Some(true);
                self.picked_colour = None;
            }
        }
        self.colour_menu_open = false;
        match (any, &common.hatch_pattern) {
            (true, None) => self.hatch.show_varies(),
            (true, Some(name)) => self.hatch.show(name),
            (false, _) => self.hatch.show(""),
        }
        self.description = if any {
            common.description.clone().unwrap_or_default()
        } else {
            String::new()
        };
        self.description_placeholder = if any && common.description.is_none() {
            String::from(VARIES)
        } else {
            String::new()
        };
        self.refresh_style_preview(document);
        self.update_buttons(document);
    }

    /// A person changed a field of the form.
    fn after_edit(&mut self, document: &Document) {
        self.refresh_style_preview(document);
        self.update_buttons(document);
    }

    fn refresh_style_preview(&mut self, document: &Document) {
        let styles = Self::styles_named(document, &self.form_styles);
        let Some(first) = styles.first() else {
            self.style_preview.clear();
            self.preview_note.clear();
            return;
        };
        // What Save would store, before it is stored: the first style with the edits written over
        // it.
        let shown = apply_edit(first.clone(), &self.edited);
        self.style_preview.set_style(&shown);
        let mut note = if styles.len() > 1 {
            format!("Showing {}, the first of {}.", shown.name, styles.len())
        } else {
            String::new()
        };
        if is_by_layer(&shown.linetype) {
            if !note.is_empty() {
                note.push(' ');
            }
            note.push_str("ByLayer: drawn in each layer's linetype; shown plain.");
        }
        self.preview_note = note;
    }

    /// A colour chosen in the colour menu.
    pub fn choose_colour(&mut self, document: &Document, colour: Color) {
        if !self.live() {
            return;
        }
        self.picked_colour = Some(colour);
        self.by_layer = Some(false);
        self.edited.color = Some(Some(colour));
        self.after_edit(document);
    }

    fn by_layer_clicked(&mut self, document: &Document, checked: bool) {
        // Out of <varies> for good once a person has said which.
        self.by_layer = Some(checked);
        if checked {
            self.edited.color = Some(None);
        } else {
            if self.picked_colour.is_none() {
                self.picked_colour = Some(Color::new(255, 255, 255, 255));
            }
            self.edited.color = Some(self.picked_colour);
        }
        self.after_edit(document);
    }

    /// Save: the fields a person changed, stored; the others stay as each style has them.
    pub fn save_styles(&mut self, host: &mut dyn Host) {
        if !self.live() || self.form_styles.is_empty() {
            return;
        }
        let command =
            match edit_styles_command(host.document().model(), &self.form_styles, &self.edited) {
                Ok(command) => command,
                Err(error) => {
                    self.log(host, &error.describe(), true);
                    return;
                }
            };
        let Some(command) = command else {
            // Not an edit: nothing is pushed on the undo stack (QT-02).
            self.log(host, "Nothing changed: the style is as it was.", false);
            self.edited = StyleFields::default();
            self.update_buttons(host.document());
            return;
        };
        let what = if self.form_styles.len() == 1 {
            format!("Style {} updated.", self.form_styles[0])
        } else {
            format!("{} styles updated as one step.", self.form_styles.len())
        };
        if self.run(host, Some(command), &what) {
            self.edited = StyleFields::default();
            self.select_styles_after_reload = Some(self.form_styles.clone());
        }
    }

    // ---- the linetype form ---------------------------------------------------------------------

    fn load_linetype_form(&mut self, document: &Document) {
        if !self.live() {
            return;
        }
        self.pattern_edited = false;
        self.description_edited = false;
        self.pattern_for.clear();
        let rows: Vec<LinetypeRow> = self
            .selected_linetype_rows()
            .into_iter()
            .cloned()
            .collect();
        if rows.len() != 1 {
            self.no_linetype = if rows.is_empty() {
                String::from("Select a linetype.")
            } else {
                format!("{} selected.", rows.len())
            };
            self.detail = Detail::None;
            self.refresh_linetype_preview();
            return;
        }
        let row = &rows[0];
        if row.origin == LinetypeOrigin::Library {
            self.show_library_details(document, row);
            self.detail = Detail::Library;
            self.refresh_linetype_preview();
            return;
        }
        let Some(linetype) = document.model().linetypes.find(&row.name) else {
            self.detail = Detail::None;
            return;
        };
        self.pattern_for.clone_from(&linetype.name);
        self.working_pattern = linetype
            .pattern
            .iter()
            .map(|element| element.length)
            .collect();
        self.linetype_description.clone_from(&linetype.description);
        // "continuous" is what an unset chain resolves to: the table refuses it a pattern, and the
        // grid says so rather than failing on Save.
        let protected_item = linetype.name == CONTINUOUS_LINETYPE;
        self.pattern_enabled = !protected_item;
        self.rebuild_grid();
        self.detail = Detail::Pattern;
        if protected_item {
            self.pattern_status = String::from(
                "continuous is the plain line everything falls back to: its pattern cannot \
                 change.",
            );
        }
        self.refresh_linetype_preview();
    }

    fn show_library_details(&mut self, document: &Document, row: &LinetypeRow) {
        let mut lines = vec![row.name.clone()];
        lines.push(format!(
            "Group: {}",
            if row.group.is_empty() {
                "(none)"
            } else {
                row.group.as_str()
            }
        ));
        // A library definition's source is the name of its customisation.
        lines.push(format!(
            "Customisation: {}",
            if row.source_file.is_empty() {
                "(made in this session)"
            } else {
                row.source_file.as_str()
            }
        ));
        lines.push(format!("Kind: {}", kind_explained(row.kind)));
        lines.push(format!("One repeat: {}", period_text(row)));
        if let Some(definition) = document.style_library().find(&row.name) {
            lines.push(format!(
                "{} stroke(s), {} text(s)",
                definition.strokes.len(),
                definition.texts.len()
            ));
        }
        lines.push(format!(
            "Used by: {}",
            if row.users.used() {
                row.users.describe()
            } else {
                String::from("nothing")
            }
        ));
        if row.collision {
            lines.push(String::new());
            lines.push(String::from(
                "The drawing also has a linetype of this name. This linestyle is what is drawn; \
                 rename or merge the drawing's to end the clash.",
            ));
        }
        lines.push(String::new());
        lines.push(String::from(
            "Library linestyles belong to the session's customisation and are read-only here. \
             Edit Definition opens this one in the definition editor.",
        ));
        self.library_details = lines.join("\n");
    }

    fn refresh_linetype_preview(&mut self) {
        let shown = {
            let rows = self.selected_linetype_rows();
            match rows.as_slice() {
                [row] => Some((row.name.clone(), row.origin)),
                _ => None,
            }
        };
        match shown {
            None => self.linetype_preview.clear(),
            Some((name, LinetypeOrigin::Library)) => self.linetype_preview.set_linestyle(&name),
            Some((name, LinetypeOrigin::Drawing)) => {
                // The stored linetype as the drawing draws it - through the same resolver, so a
                // shadowed name (D2) shows the library's strokes.
                let sample = Style {
                    name: name.clone(),
                    linetype: name,
                    ..Style::default()
                };
                self.linetype_preview.set_style(&sample);
            }
        }
    }

    fn linetype_from_form(&self, document: &Document) -> Linetype {
        let mut linetype = document
            .model()
            .linetypes
            .find(&self.pattern_for)
            .cloned()
            .unwrap_or_default();
        if self.pattern_edited {
            linetype.pattern = self
                .working_pattern
                .iter()
                .map(|&length| LinetypeElement { length })
                .collect();
        }
        if self.description_edited {
            linetype.description.clone_from(&self.linetype_description);
        }
        linetype
    }

    /// The grid's boxes made again from the pattern as it stands.
    fn rebuild_grid(&mut self) {
        self.pattern_kinds = self
            .working_pattern
            .iter()
            .map(|&length| element_of(length))
            .collect();
        self.pattern_lengths = self
            .working_pattern
            .iter()
            .map(|&length| spin_value(&PATTERN_LENGTH, length.abs()))
            .collect();
        self.pattern_length_enabled = self
            .working_pattern
            .iter()
            .map(|&length| length != 0.0)
            .collect();
        self.pattern_current = if self.working_pattern.is_empty() {
            None
        } else {
            Some(
                self.pattern_current
                    .unwrap_or(0)
                    .min(self.working_pattern.len() - 1),
            )
        };
        self.validate_pattern();
    }

    /// A kind chosen in row `row`'s kind box.
    fn change_element(&mut self, row: usize, element: Element) {
        let Some(length) = self.working_pattern.get_mut(row) else {
            return;
        };
        // Keep the size a person gave when only the kind changes; a dot becoming a dash or a gap
        // needs one, and half a metre is visible.
        let size = if length.abs() > 0.0 { length.abs() } else { 0.5 };
        *length = match element {
            Element::Dash => size,
            Element::Gap => -size,
            Element::Dot => 0.0,
        };
        let length = *length;
        self.pattern_kinds[row] = element;
        self.pattern_lengths[row] = spin_value(&PATTERN_LENGTH, length.abs());
        self.pattern_length_enabled[row] = length != 0.0;
        self.pattern_changed();
    }

    /// A length typed into row `row`'s length box.
    fn change_length(&mut self, row: usize, value: f64) {
        let (Some(element), Some(&shown)) = (
            self.working_pattern.get_mut(row),
            self.pattern_kinds.get(row),
        ) else {
            return;
        };
        // The sign comes from the kind the row's box shows, never from the length stored: typing
        // 0.25 over a gap's 0.5 passes through "0", stored for that moment as -0.0, which is not
        // below zero and so says nothing of the gap the row still is.
        *element = match shown {
            Element::Gap => -value,
            Element::Dot => 0.0,
            Element::Dash => value,
        };
        self.pattern_changed();
    }

    fn insert_element(&mut self, document: &Document, length: f64) {
        let at = match self.pattern_current {
            Some(row) => (row + 1).min(self.working_pattern.len()),
            None => self.working_pattern.len(),
        };
        self.working_pattern.insert(at, length);
        self.pattern_changed();
        self.rebuild_grid();
        self.pattern_current = Some(at);
        self.update_buttons(document);
    }

    fn remove_element(&mut self, document: &Document) {
        let Some(row) = self
            .pattern_current
            .filter(|&row| row < self.working_pattern.len())
        else {
            return;
        };
        self.working_pattern.remove(row);
        self.pattern_changed();
        self.rebuild_grid();
        self.update_buttons(document);
    }

    fn move_element(&mut self, document: &Document, down: bool) {
        let Some(row) = self.pattern_current else {
            return;
        };
        let to = if down {
            row + 1
        } else if row == 0 {
            return;
        } else {
            row - 1
        };
        if to >= self.working_pattern.len() {
            return;
        }
        self.working_pattern.swap(row, to);
        self.pattern_changed();
        self.rebuild_grid();
        self.pattern_current = Some(to);
        self.update_buttons(document);
    }

    fn pattern_changed(&mut self) {
        self.pattern_edited = true;
        self.validate_pattern();
    }

    fn validate_pattern(&mut self) {
        let linetype = Linetype {
            name: self.pattern_for.clone(),
            pattern: self
                .working_pattern
                .iter()
                .map(|&length| LinetypeElement { length })
                .collect(),
            ..Linetype::default()
        };
        match validate(&linetype) {
            Ok(()) => {
                self.pattern_valid = true;
                self.pattern_status_is_error = false;
                self.pattern_status = if linetype.pattern.is_empty() {
                    String::from("No elements: a continuous line.")
                } else {
                    format!(
                        "Valid. One repeat is {} m.",
                        number(linetype.pattern_length())
                    )
                };
            }
            Err(error) => {
                self.pattern_valid = false;
                self.pattern_status_is_error = true;
                self.pattern_status = error.message;
            }
        }
    }

    /// Save on the Linetypes tab: the pattern and the description, stored.
    pub fn save_linetype(&mut self, host: &mut dyn Host) {
        if !self.live() || self.pattern_for.is_empty() || !self.pattern_valid {
            return;
        }
        let command = update_linetype_if_changed(
            host.document().model(),
            self.linetype_from_form(host.document()),
        );
        let Some(command) = command else {
            self.log(host, "Nothing changed: the linetype is as it was.", false);
            self.pattern_edited = false;
            self.description_edited = false;
            self.update_buttons(host.document());
            return;
        };
        let name = self.pattern_for.clone();
        if self.run(host, Some(command), &format!("Linetype {name} updated.")) {
            self.pattern_edited = false;
            self.description_edited = false;
            self.select_linetype_after_reload = Some((name, LinetypeOrigin::Drawing));
        }
    }

    // ---- the prompt row and the purge panel ----------------------------------------------------

    fn ask_name(&mut self, question: &str, suggestion: &str, what_for: PromptFor) {
        self.purge_open = false;
        self.prompt_label = String::from(question);
        self.prompt_name = String::from(suggestion);
        self.prompt_is_choice = false;
        self.prompt = Some(what_for);
    }

    fn ask_choice(&mut self, question: &str, choices: Vec<String>, what_for: PromptFor) {
        self.purge_open = false;
        self.prompt_label = String::from(question);
        self.prompt_choice = if choices.is_empty() { None } else { Some(0) };
        self.prompt_choices = choices;
        self.prompt_is_choice = true;
        self.prompt = Some(what_for);
    }

    /// The prompt's name field, typed into.
    pub fn set_prompt_name(&mut self, text: &str) {
        self.prompt_name = String::from(text);
    }

    /// OK, or Enter in the prompt's name.
    pub fn accept_prompt(&mut self, host: &mut dyn Host) {
        if !self.live() {
            return;
        }
        let answer = if self.prompt_is_choice {
            self.prompt_choice
                .and_then(|at| self.prompt_choices.get(at))
                .cloned()
                .unwrap_or_default()
        } else {
            // Names are kept as given, but blanks either end of a typed name are never meant
            // (and a style name may not start with one).
            String::from(
                self.prompt_name
                    .trim_matches(|c: char| c.is_ascii_whitespace()),
            )
        };
        if answer.is_empty() {
            self.log(host, "Give a name first.", true);
            return;
        }
        let Some(what_for) = self.prompt.take() else {
            return;
        };
        match what_for {
            PromptFor::NewStyle => {
                let style = Style {
                    name: answer.clone(),
                    ..Style::default()
                };
                if self.run(
                    host,
                    Some(create_style(style)),
                    &format!("Style {answer} created."),
                ) {
                    self.select_styles_after_reload = Some(vec![answer]);
                }
            }
            PromptFor::DuplicateStyle(from) => {
                if self.run(
                    host,
                    Some(duplicate_style(from.clone(), answer.clone())),
                    &format!("Style {from} copied as {answer}."),
                ) {
                    self.select_styles_after_reload = Some(vec![answer]);
                }
            }
            PromptFor::RenameStyle(from) => {
                if answer == from {
                    return;
                }
                if self.run(
                    host,
                    Some(rename_style(from.clone(), answer.clone())),
                    &format!(
                        "Style {from} renamed to {answer}; every entity wearing it came too."
                    ),
                ) {
                    self.select_styles_after_reload = Some(vec![answer]);
                }
            }
            PromptFor::MergeStyles(names) => {
                let parts: Vec<CommandPtr> = names
                    .iter()
                    .map(|from| merge_style(from.clone(), answer.clone()))
                    .collect();
                if self.run(
                    host,
                    Self::together("MergeStyles", parts),
                    &format!(
                        "Merged {} style(s) into {answer}; their entities wear it now.",
                        names.len()
                    ),
                ) {
                    self.select_styles_after_reload = Some(vec![answer]);
                }
            }
            PromptFor::NewStyleUsing(linestyle) => {
                let style = Style {
                    name: answer.clone(),
                    linetype: linestyle.clone(),
                    ..Style::default()
                };
                if self.run(
                    host,
                    Some(create_style(style)),
                    &format!("Style {answer} created, drawn with {linestyle}."),
                ) {
                    self.select_styles_after_reload = Some(vec![answer]);
                    self.tab = 0;
                }
            }
            PromptFor::NewLinetype => {
                let linetype = Linetype {
                    name: answer.clone(),
                    // A pattern to start from rather than a continuous line, which would need
                    // changing before it meant anything.
                    pattern: vec![
                        LinetypeElement { length: 1.0 },
                        LinetypeElement { length: -0.5 },
                    ],
                    ..Linetype::default()
                };
                if self.run(
                    host,
                    Some(create_linetype(linetype)),
                    &format!("Linetype {answer} created."),
                ) {
                    self.select_linetype_after_reload = Some((answer, LinetypeOrigin::Drawing));
                }
            }
            PromptFor::DuplicateLinetype(from) => {
                if self.run(
                    host,
                    Some(duplicate_linetype(from.clone(), answer.clone())),
                    &format!("Linetype {from} copied as {answer}."),
                ) {
                    self.select_linetype_after_reload = Some((answer, LinetypeOrigin::Drawing));
                }
            }
            PromptFor::RenameLinetype(from) => {
                if answer == from {
                    return;
                }
                if self.run(
                    host,
                    Some(rename_linetype(from.clone(), answer.clone())),
                    &format!(
                        "Linetype {from} renamed to {answer}; every layer and style naming it \
                         came too."
                    ),
                ) {
                    self.select_linetype_after_reload = Some((answer, LinetypeOrigin::Drawing));
                }
            }
            PromptFor::MergeLinetypes(names) => {
                let parts: Vec<CommandPtr> = names
                    .iter()
                    .map(|from| merge_linetype(from.clone(), answer.clone()))
                    .collect();
                if self.run(
                    host,
                    Self::together("MergeLinetypes", parts),
                    &format!("Merged {} linetype(s) into {answer}.", names.len()),
                ) {
                    self.select_linetype_after_reload = Some((answer, LinetypeOrigin::Drawing));
                }
            }
        }
    }

    fn show_purge(&mut self, host: &mut dyn Host, mut options: PurgeOptions, title: &str) {
        if !self.live() {
            return;
        }
        self.prompt = None;
        let current = String::from(host.document().current_style());
        if !current.is_empty() {
            options.keep_styles.push(current);
        }
        let plan = plan_purge(host.document().model(), &options);
        if plan.empty() {
            self.log(host, "Nothing is unused: there is nothing to purge.", false);
            self.purge_open = false;
            return;
        }
        self.purge_options = options;
        self.purge_label = String::from(title);
        self.purge_items.clear();
        let tables = [
            ("Style", &plan.styles, 0_u8),
            ("Linetype", &plan.linetypes, 1),
            ("Hatch pattern", &plan.hatch_patterns, 2),
        ];
        for (kind, names, table) in tables {
            for name in names {
                self.purge_items.push(PurgeItem {
                    label: format!("{kind}  {name}"),
                    name: name.clone(),
                    table,
                    checked: true,
                });
            }
        }
        self.purge_open = true;
    }

    /// Purge Checked: the checked items deleted as one step.
    pub fn accept_purge(&mut self, host: &mut dyn Host) {
        if !self.live() {
            return;
        }
        let mut checked = TableItems::default();
        let mut options = self.purge_options.clone();
        for item in &self.purge_items {
            if !item.checked {
                // A style kept keeps what it names: planned again below.
                if item.table == 0 {
                    options.keep_styles.push(item.name.clone());
                }
                continue;
            }
            match item.table {
                0 => checked.styles.push(item.name.clone()),
                1 => checked.linetypes.push(item.name.clone()),
                _ => checked.hatch_patterns.push(item.name.clone()),
            }
        }
        // Planned again with the unchecked styles kept, so a linetype only a kept style names is
        // not in the set (the command would refuse it).
        let plan = plan_purge(host.document().model(), &options);
        let both = |a: &[String], b: &[String]| -> Vec<String> {
            a.iter().filter(|name| b.contains(name)).cloned().collect()
        };
        let items = TableItems {
            styles: both(&checked.styles, &plan.styles),
            linetypes: both(&checked.linetypes, &plan.linetypes),
            hatch_patterns: both(&checked.hatch_patterns, &plan.hatch_patterns),
        };
        self.purge_open = false;
        if items.empty() {
            self.log(host, "Nothing was checked: nothing purged.", false);
            return;
        }
        let count = items.size();
        self.run(
            host,
            Some(purge_table_items(items)),
            &format!("Purged {count} unused item(s) as one step."),
        );
    }

    // ---- buttons -------------------------------------------------------------------------------

    fn update_buttons(&mut self, document: &Document) {
        if !self.live() {
            return;
        }
        let selected = self.selected_styles();
        let styles = selected.len();
        let current_style = document.current_style();
        let is_current = styles == 1 && !current_style.is_empty() && selected[0] == current_style;
        let picked = document.selection().size();
        // Save is offered whenever there is a style to save: saving one nobody edited is allowed,
        // and is no command at all (QT-02).
        let dirty = !self.edited.empty();

        let rows = self.selected_linetype_rows().len();
        let drawing = self.selected_drawing_linetypes();
        let any_continuous = drawing.iter().any(|name| name == CONTINUOUS_LINETYPE);
        let drawing_total = self
            .linetype_rows
            .iter()
            .filter(|row| row.origin == LinetypeOrigin::Drawing)
            .count();
        let linetype_dirty = self.pattern_edited || self.description_edited;
        let grid = self.pattern_enabled;
        let current = self.pattern_current;

        self.buttons = Buttons {
            duplicate_style: styles == 1,
            rename_style: styles == 1,
            merge_styles: styles >= 1 && document.model().styles.size() > styles,
            delete_styles: styles >= 1,
            select_style_users: styles >= 1,
            make_current: styles == 1,
            make_current_text: if is_current {
                "Draw ByLayer"
            } else {
                "Make Current"
            },
            make_current_tip: if is_current {
                "New work stops using this style and is drawn ByLayer"
            } else {
                "Draw new work in this style"
            },
            apply_text: if picked > 0 {
                format!("Apply to Selection ({picked})")
            } else {
                String::from("Apply to Selection")
            },
            apply: styles == 1 && picked > 0,
            save_style: styles >= 1,
            revert_style: styles >= 1 && dirty,
            duplicate_linetype: drawing.len() == 1,
            rename_linetype: drawing.len() == 1 && !any_continuous,
            merge_linetypes: !drawing.is_empty()
                && !any_continuous
                && drawing_total > drawing.len(),
            delete_linetypes: !drawing.is_empty() && !any_continuous,
            select_linetype_users: rows > 0,
            save_linetype: !self.pattern_for.is_empty() && linetype_dirty && self.pattern_valid,
            revert_linetype: !self.pattern_for.is_empty() && linetype_dirty,
            remove_element: current.is_some() && grid,
            move_up: current.is_some_and(|row| row > 0) && grid,
            move_down: current.is_some_and(|row| row + 1 < self.working_pattern.len()) && grid,
            select_diagnostic_users: !self.diagnostic_selected.is_empty(),
            undo: document.history().can_undo(),
            redo: document.history().can_redo(),
        };
    }

    fn select_ids(&mut self, host: &mut dyn Host, ids: Vec<EntityId>) {
        let count = ids.len();
        match self.context.select_and_show.clone() {
            Some(select_and_show) => select_and_show(host, &ids),
            None => {
                host.document_mut().selection_mut().set(ids);
                host.document().notify_selection_changed();
            }
        }
        self.log(host, &entities_selected(count), false);
    }

    fn select_users(&mut self, host: &mut dyn Host, table: UsageTable, names: &[String]) {
        if !self.alive() || names.is_empty() {
            return;
        }
        let ids = entities_using(host.document().model(), table, names);
        if ids.is_empty() {
            self.log(host, "No entity is drawn with that.", false);
            return;
        }
        self.select_ids(host, ids);
    }

    // ---- what the styles page's buttons do -----------------------------------------------------

    /// New... on the Styles tab.
    pub fn ask_new_style(&mut self, document: &Document) {
        if !self.live() {
            return;
        }
        let suggestion = free_style_name(document.model(), "New Style");
        self.ask_name("Name of the new style:", &suggestion, PromptFor::NewStyle);
    }

    fn ask_duplicate_style(&mut self, document: &Document) {
        let names = self.selected_styles();
        let [from] = names.as_slice() else {
            return;
        };
        let suggestion = free_style_name(document.model(), from);
        self.ask_name(
            &format!("Name of the copy of {from}:"),
            &suggestion,
            PromptFor::DuplicateStyle(from.clone()),
        );
    }

    fn ask_rename_style(&mut self) {
        let names = self.selected_styles();
        let [from] = names.as_slice() else {
            return;
        };
        self.ask_name(
            &format!("New name for {from}:"),
            from,
            PromptFor::RenameStyle(from.clone()),
        );
    }

    fn ask_merge_styles(&mut self, document: &Document) {
        let names = self.selected_styles();
        if names.is_empty() {
            return;
        }
        let targets: Vec<String> = document
            .model()
            .styles
            .names()
            .into_iter()
            .filter(|name| !names.contains(name))
            .collect();
        let question = if names.len() == 1 {
            format!("Merge {} into:", names[0])
        } else {
            format!("Merge the {} selected styles into:", names.len())
        };
        self.ask_choice(&question, targets, PromptFor::MergeStyles(names));
    }

    fn delete_styles(&mut self, host: &mut dyn Host) {
        let names = self.selected_styles();
        if names.is_empty() {
            return;
        }
        let parts: Vec<CommandPtr> = names.iter().map(|name| delete_style(name.clone())).collect();
        let done = if names.len() == 1 {
            format!("Style {} deleted.", names[0])
        } else {
            format!("{} styles deleted.", names.len())
        };
        self.run(host, Self::together("DeleteStyles", parts), &done);
    }

    fn apply_to_selection(&mut self, host: &mut dyn Host) {
        let names = self.selected_styles();
        let [name] = names.as_slice() else {
            return;
        };
        if !self.alive() {
            return;
        }
        // The selection as it is NOW, not as it was when the dialog opened: the dialog may sit
        // beside the drawing for as long as it likes.
        let ids = host.document().selection().ids();
        if ids.is_empty() {
            self.log(
                host,
                "Select the entities to style in the drawing first.",
                true,
            );
            return;
        }
        let count = ids.len();
        self.run(
            host,
            Some(set_entity_style(ids, name.clone())),
            &format!(
                "{count} entit{} now wear{} style {name}.",
                if count == 1 { "y" } else { "ies" },
                if count == 1 { "s" } else { "" }
            ),
        );
    }

    fn make_current(&mut self, host: &mut dyn Host) {
        let names = self.selected_styles();
        let [name] = names.as_slice() else {
            return;
        };
        if !self.alive() {
            return;
        }
        // Not a command: the current style is session state, as the current layer is (decision
        // D9).
        let clearing = host.document().current_style() == name;
        let next = if clearing { String::new() } else { name.clone() };
        if let Err(error) = host.document_mut().set_current_style(&next) {
            self.log(host, &error.describe(), true);
            return;
        }
        let said = if clearing {
            String::from("New work is drawn ByLayer.")
        } else {
            format!("New work is drawn in style {next}.")
        };
        self.log(host, &said, false);
    }

    // ---- what the linetypes page's buttons do --------------------------------------------------

    fn ask_new_style_using(&mut self, document: &Document) {
        let linestyle = {
            let rows = self.selected_linetype_rows();
            match rows.as_slice() {
                [row] if row.origin == LinetypeOrigin::Library => row.name.clone(),
                _ => return,
            }
        };
        let suggestion = free_style_name(document.model(), &linestyle);
        self.ask_name(
            &format!("Name of the new style drawn with {linestyle}:"),
            &suggestion,
            PromptFor::NewStyleUsing(linestyle.clone()),
        );
    }

    fn edit_definition(&mut self) {
        let name = {
            let rows = self.selected_linetype_rows();
            match rows.as_slice() {
                [row] if row.origin == LinetypeOrigin::Library => row.name.clone(),
                _ => return,
            }
        };
        if let Some(edit) = &self.context.edit_definition {
            edit(DefinitionEdit::Edit, &name);
        }
    }

    fn ask_new_linetype(&mut self, document: &Document) {
        let suggestion = free_linetype_name(document, "New Linetype");
        self.ask_name(
            "Name of the new linetype:",
            &suggestion,
            PromptFor::NewLinetype,
        );
    }

    fn ask_duplicate_linetype(&mut self, document: &Document) {
        let names = self.selected_drawing_linetypes();
        let [from] = names.as_slice() else {
            return;
        };
        let suggestion = free_linetype_name(document, from);
        self.ask_name(
            &format!("Name of the copy of {from}:"),
            &suggestion,
            PromptFor::DuplicateLinetype(from.clone()),
        );
    }

    fn ask_rename_linetype(&mut self) {
        let names = self.selected_drawing_linetypes();
        let [from] = names.as_slice() else {
            return;
        };
        self.ask_name(
            &format!("New name for {from}:"),
            from,
            PromptFor::RenameLinetype(from.clone()),
        );
    }

    fn ask_merge_linetypes(&mut self, document: &Document) {
        let names = self.selected_drawing_linetypes();
        if names.is_empty() {
            return;
        }
        // Drawing linetypes only: the commands cannot see the library, so a library linestyle is
        // no merge target (a rename onto its name is).
        let targets: Vec<String> = document
            .model()
            .linetypes
            .names()
            .into_iter()
            .filter(|name| !names.contains(name))
            .collect();
        let question = if names.len() == 1 {
            format!("Merge {} into:", names[0])
        } else {
            format!("Merge the {} selected linetypes into:", names.len())
        };
        self.ask_choice(&question, targets, PromptFor::MergeLinetypes(names));
    }

    fn delete_linetypes(&mut self, host: &mut dyn Host) {
        let names = self.selected_drawing_linetypes();
        if names.is_empty() {
            return;
        }
        let parts: Vec<CommandPtr> = names
            .iter()
            .map(|name| delete_linetype(name.clone()))
            .collect();
        let done = if names.len() == 1 {
            format!("Linetype {} deleted.", names[0])
        } else {
            format!("{} linetypes deleted.", names.len())
        };
        self.run(host, Self::together("DeleteLinetypes", parts), &done);
    }

    fn select_diagnostic_users(&mut self, host: &mut dyn Host) {
        let mut linetypes = Vec::new();
        let mut symbols = Vec::new();
        for row in self
            .diagnostic_selected
            .iter()
            .filter_map(|&row| self.diagnostic_rows.get(row))
        {
            if row.kind == StyleDiagnosticKind::MissingSymbol {
                symbols.push(row.name.clone());
            } else {
                linetypes.push(row.name.clone());
            }
        }
        if !self.alive() {
            return;
        }
        let model = host.document().model();
        let mut ids = entities_using(model, UsageTable::Linetype, &linetypes);
        ids.extend(entities_using(model, UsageTable::Symbol, &symbols));
        ids.sort_unstable();
        ids.dedup();
        if ids.is_empty() {
            self.log(
                host,
                "No entity is drawn with that; a layer or an unused style names it.",
                false,
            );
            return;
        }
        self.select_ids(host, ids);
    }

    /// Undo: the drawing's own stack.
    pub fn undo(&mut self, host: &mut dyn Host) {
        if !self.live() {
            return;
        }
        match host.document_mut().undo() {
            Ok(()) => self.log(host, "Undone.", false),
            Err(error) => self.log(host, &error.describe(), true),
        }
    }

    /// Redo.
    pub fn redo(&mut self, host: &mut dyn Host) {
        if !self.live() {
            return;
        }
        match host.document_mut().redo() {
            Ok(()) => self.log(host, "Redone.", false),
            Err(error) => self.log(host, &error.describe(), true),
        }
    }

    // ---- the frame -----------------------------------------------------------------------------

    /// Draws the dialog for this frame - the drawing's changes since the last first; false once it
    /// is closed, by its Close button or its title bar's.
    pub fn ui(&mut self, ctx: &Context, host: &mut dyn Host) -> bool {
        if let Some(changes) = self.watcher.deliver(host.document()) {
            self.on_document_changed(host.document(), changes);
        }
        let mut frame = std::mem::replace(&mut self.frame, DialogFrame::new(STYLE_MANAGER, TITLE));
        let mut closed = show_dialog(&mut frame, ctx, self, host, Self::body, Self::footer);
        self.frame = frame;
        closed |= std::mem::take(&mut self.closing);
        !closed
    }

    fn body(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        ui.add_enabled_ui(!self.orphan, |ui| {
            let diagnostics = self.diagnostics_title.clone();
            let titles = ["Styles", "Linetypes", "Hatch Patterns", diagnostics.as_str()];
            controls::tabs(ui, "managerTabs", &titles, &mut self.tab);
            ui.separator();
            // A page not in front is still there, as a QTabWidget's is: what it holds can be
            // filled, pressed and read. Only its filter bar is not (see the module comment).
            let pages: [(&str, fn(&mut Self, &mut Ui, &mut dyn Host, bool)); 4] = [
                ("stylesPage", Self::styles_page),
                ("linetypesPage", Self::linetypes_page),
                ("hatchPatternsPageHolder", Self::hatch_page),
                ("diagnosticsPage", Self::diagnostics_page),
            ];
            for (at, (name, page)) in pages.into_iter().enumerate() {
                if at == self.tab {
                    page(self, ui, host, true);
                } else {
                    offstage(ui, name, |ui| page(self, ui, host, false));
                }
            }
            self.prompt_row(ui, host);
            self.purge_panel(ui, host);
        });
    }

    fn footer(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        ui.horizontal(|ui| {
            ui.add_enabled_ui(self.buttons.undo && !self.orphan, |ui| {
                if controls::button(ui, "undoButton", "Undo")
                    .on_hover_text("Undo the last change to the drawing")
                    .clicked()
                {
                    self.undo(host);
                }
            });
            ui.add_enabled_ui(self.buttons.redo && !self.orphan, |ui| {
                if controls::button(ui, "redoButton", "Redo")
                    .on_hover_text("Redo what was undone")
                    .clicked()
                {
                    self.redo(host);
                }
            });
            let mut said = egui::RichText::new(self.status.as_str());
            if self.status_is_error {
                said = said.color(ERROR_COLOUR);
            }
            controls::label_as(ui, "statusLine", &self.status, egui::Label::new(said).wrap());
            // Close alone is not held back by a Document that has gone: it is how a person
            // dismisses a dialog that has nothing left to edit.
            if controls::button(ui, "closeButton", "Close").clicked() {
                self.closing = true;
            }
        });
    }

    fn hatch_page(&mut self, ui: &mut Ui, host: &mut dyn Host, _front: bool) {
        self.hatch_patterns.ui(ui, host);
    }

    /// A button of a page: drawn enabled or not, with its tip; true when pressed.
    fn page_button(ui: &mut Ui, enabled: bool, name: &str, text: &str, tip: &str) -> bool {
        ui.add_enabled_ui(enabled, |ui| {
            controls::button(ui, name, text).on_hover_text(tip).clicked()
        })
        .inner
    }

    /// The plot scale box and the Screen box under a preview; true when either changed.
    fn preview_controls(
        ui: &mut Ui,
        scale_name: &str,
        screen_name: &str,
        scale: &mut usize,
        screen: &mut bool,
    ) -> bool {
        let items: Vec<String> = StylePreview::PLOT_SCALES
            .iter()
            .map(|denominator| format!("1:{denominator}"))
            .collect();
        ui.label("Plot scale");
        let mut choice = Choice::at(*scale);
        let mut changed = controls::choice(ui, scale_name, &items, &mut choice)
            .on_hover_text(
                "The plot scale the preview draws at: a paper linestyle keeps its printed size, a \
                 world one shrinks as the scale grows",
            )
            .changed();
        if let Some(index) = choice.index {
            *scale = index;
        }
        changed |= controls::check(ui, screen_name, "Screen", screen)
            .on_hover_text("Show it on the viewport's dark ground rather than on paper")
            .changed();
        changed
    }

    fn apply_preview_controls(preview: &mut StylePreview, scale: usize, screen: bool) {
        if let Some(&denominator) = StylePreview::PLOT_SCALES.get(scale) {
            preview.set_scale_denominator(denominator);
        }
        preview.set_ground(if screen {
            PreviewGround::Screen
        } else {
            PreviewGround::Paper
        });
    }

    // ---- the styles page -----------------------------------------------------------------------

    fn style_cells(row: &StyleRow) -> Vec<String> {
        vec![
            if row.current {
                format!("{}  (current)", row.style.name)
            } else {
                row.style.name.clone()
            },
            String::new(),
            row.style.linetype.clone(),
            qt_fixed(row.style.line_weight, 2),
            row.style
                .color
                .map_or_else(|| String::from("ByLayer"), Color::to_hex),
            if row.style.hatch_pattern.is_empty() {
                String::from("ByLayer")
            } else {
                row.style.hatch_pattern.clone()
            },
            row.style.symbol.clone(),
            format!("{}", row.entities),
            row.style.description.clone(),
        ]
    }

    fn styles_page(&mut self, ui: &mut Ui, host: &mut dyn Host, front: bool) {
        page_record(ui, "stylesPage");
        if front {
            let changes = self.style_filter.ui(ui);
            if changes.any() && self.live() && self.refilter_styles() {
                self.load_style_form(host.document());
            }
        }
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                let rows: Vec<Vec<String>> = self
                    .style_view
                    .iter()
                    .map(|&row| Self::style_cells(&self.style_rows[row]))
                    .collect();
                let highlighted: Vec<bool> = self
                    .style_view
                    .iter()
                    .map(|row| self.style_selected.contains(row))
                    .collect();
                let shown = table(
                    ui,
                    "styleTable",
                    &STYLE_HEADERS,
                    &rows,
                    None,
                    &highlighted,
                    None,
                );
                if let Some(&row) = shown.chosen.and_then(|at| self.style_view.get(at))
                    && self.live()
                    && pick(&mut self.style_selected, row, shown.adding)
                {
                    self.load_style_form(host.document());
                }
            });
            ui.vertical(|ui| {
                self.style_form(ui, host);
                ui.group(|ui| {
                    ui.strong("Preview");
                    self.style_preview.ui(
                        ui,
                        "stylePreview",
                        host.document(),
                        egui::vec2(360.0, 150.0),
                    );
                    ui.horizontal(|ui| {
                        if Self::preview_controls(
                            ui,
                            "stylePreviewScale",
                            "stylePreviewScreen",
                            &mut self.preview_scale,
                            &mut self.preview_screen,
                        ) {
                            Self::apply_preview_controls(
                                &mut self.style_preview,
                                self.preview_scale,
                                self.preview_screen,
                            );
                        }
                        controls::label_as(
                            ui,
                            "stylePreviewNote",
                            &self.preview_note,
                            egui::Label::new(self.preview_note.as_str()).wrap(),
                        );
                    });
                });
            });
        });
        self.style_buttons(ui, host);
    }

    fn style_form(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        ui.group(|ui| {
            ui.strong("Definition");
            controls::label_as(
                ui,
                "styleFormTitle",
                &self.form_title,
                egui::Label::new(self.form_title.as_str()).wrap(),
            );
            let enabled = self.fields_enabled;
            ui.add_enabled_ui(enabled, |ui| {
                egui::Grid::new("styleForm").num_columns(2).show(ui, |ui| {
                    ui.label("Linetype");
                    if let Some(name) = self.linetype_picker.ui(ui, "styleLinetype")
                        && self.live()
                    {
                        self.edited.linetype = Some(name);
                        self.after_edit(host.document());
                    }
                    ui.end_row();

                    ui.label("Line weight");
                    ui.horizontal(|ui| {
                        if controls::spin(ui, "styleWeight", &mut self.weight, WEIGHT).changed()
                            && self.live()
                        {
                            self.weight_varies = false;
                            self.edited.line_weight = Some(self.weight);
                            self.after_edit(host.document());
                        }
                        if self.weight_varies {
                            ui.weak(VARIES);
                        }
                    });
                    ui.end_row();

                    ui.label("Colour");
                    ui.vertical(|ui| self.colour_row(ui, host));
                    ui.end_row();

                    ui.label("Hatch");
                    if self.hatch.ui(ui, "styleHatch").changed() && self.live() {
                        // Choosing <varies> again means "leave each as it is".
                        self.edited.hatch_pattern = self.hatch.current();
                        self.after_edit(host.document());
                    }
                    ui.end_row();

                    ui.label("Symbol");
                    if let Some(name) = self.symbol_picker.ui(ui, "styleSymbol")
                        && self.live()
                    {
                        self.edited.symbol = Some(name);
                        self.after_edit(host.document());
                    }
                    ui.end_row();

                    ui.label("Symbol size");
                    ui.horizontal(|ui| {
                        if controls::spin(ui, "styleSymbolSize", &mut self.symbol_size, SYMBOL_SIZE)
                            .changed()
                            && self.live()
                        {
                            self.symbol_size_varies = false;
                            self.edited.symbol_size = Some(self.symbol_size);
                            self.after_edit(host.document());
                        }
                        if self.symbol_size_varies {
                            ui.weak(VARIES);
                        }
                    });
                    ui.end_row();

                    ui.label("Description");
                    let placeholder = self.description_placeholder.clone();
                    if controls::line_with(
                        ui,
                        "styleDescription",
                        &mut self.description,
                        controls::LineOptions {
                            placeholder: &placeholder,
                            ..controls::LineOptions::default()
                        },
                    )
                    .changed()
                        && self.live()
                    {
                        self.description_placeholder.clear();
                        self.edited.description = Some(self.description.clone());
                        self.after_edit(host.document());
                    }
                    ui.end_row();
                });
            });
            ui.horizontal(|ui| {
                if Self::page_button(
                    ui,
                    self.buttons.revert_style,
                    "styleRevert",
                    "Revert",
                    "Forget the changes made in the form",
                ) {
                    self.load_style_form(host.document());
                }
                if Self::page_button(
                    ui,
                    self.buttons.save_style,
                    "styleSave",
                    "Save",
                    "Store the fields you changed; the others stay as each style has them",
                ) {
                    self.save_styles(host);
                }
            });
        });
    }

    fn colour_row(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        ui.horizontal(|ui| {
            let text = self.colour_text();
            ui.add_enabled_ui(self.by_layer != Some(true), |ui| {
                if controls::button(ui, "styleColour", &text).clicked() {
                    self.colour_menu_open = !self.colour_menu_open;
                }
            });
            let mut checked = self.by_layer == Some(true);
            if controls::check(ui, "styleColourByLayer", "ByLayer", &mut checked)
                .on_hover_text("Drawn in the colour of the layer it is on")
                .changed()
                && self.live()
            {
                self.by_layer_clicked(host.document(), checked);
            }
        });
        if self.colour_menu_open {
            let mut chosen = None;
            ui.horizontal_wrapped(|ui| {
                for (name, colour) in STANDARD_COLOURS {
                    if ui.small_button(name).clicked() {
                        chosen = Some(colour);
                    }
                }
                // "Other...": any colour, picked without a box that waits.
                let shown = self.picked_colour.unwrap_or(Color::new(255, 255, 255, 255));
                let mut rgb = [shown.r, shown.g, shown.b];
                let other = ui.color_edit_button_srgb(&mut rgb);
                custom(
                    ui,
                    &other,
                    ControlRecord::new("styleColourDialog", ControlKind::Other),
                );
                if other.changed() {
                    chosen = Some(Color::new(rgb[0], rgb[1], rgb[2], 255));
                }
            });
            if let Some(colour) = chosen {
                self.choose_colour(host.document(), colour);
            }
        }
    }

    fn style_buttons(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        let buttons = self.buttons.clone();
        ui.horizontal_wrapped(|ui| {
            if Self::page_button(
                ui,
                true,
                "styleNew",
                "New...",
                "A new style, drawn plainly until you define it",
            ) {
                self.ask_new_style(host.document());
            }
            if Self::page_button(
                ui,
                buttons.duplicate_style,
                "styleDuplicate",
                "Duplicate...",
                "A copy of the selected style",
            ) && self.live()
            {
                self.ask_duplicate_style(host.document());
            }
            if Self::page_button(
                ui,
                buttons.rename_style,
                "styleRename",
                "Rename...",
                "Rename it; every entity wearing it comes too, as one undo step",
            ) && self.live()
            {
                self.ask_rename_style();
            }
            if Self::page_button(
                ui,
                buttons.merge_styles,
                "styleMerge",
                "Merge Into...",
                "Move every entity wearing the selected styles onto another style, then delete \
                 them: one undo step",
            ) && self.live()
            {
                self.ask_merge_styles(host.document());
            }
            if Self::page_button(
                ui,
                buttons.delete_styles,
                "styleDelete",
                "Delete",
                "Refused while any entity wears it, saying how many and which first",
            ) && self.live()
            {
                self.delete_styles(host);
            }
            if Self::page_button(
                ui,
                true,
                "stylePurge",
                "Purge...",
                "Delete the styles, linetypes and hatch patterns nothing uses: one undo step",
            ) {
                self.show_purge(
                    host,
                    PurgeOptions::default(),
                    "Unused styles, linetypes and hatch patterns - uncheck any to keep:",
                );
            }
            if Self::page_button(
                ui,
                buttons.apply,
                "styleApplyToSelection",
                &buttons.apply_text,
                "Give the entities selected in the drawing now this style",
            ) && self.live()
            {
                self.apply_to_selection(host);
            }
            if Self::page_button(
                ui,
                buttons.select_style_users,
                "styleSelectUsers",
                "Select Users",
                "Select the entities wearing the selected styles",
            ) && self.live()
            {
                let names = self.selected_styles();
                self.select_users(host, UsageTable::Style, &names);
            }
            if Self::page_button(
                ui,
                buttons.make_current,
                "styleMakeCurrent",
                buttons.make_current_text,
                buttons.make_current_tip,
            ) && self.live()
            {
                self.make_current(host);
            }
        });
    }

    // ---- the linetypes page --------------------------------------------------------------------

    fn linetype_cells(row: &LinetypeRow) -> Vec<String> {
        vec![
            row.name.clone(),
            String::from(match (row.origin, row.collision) {
                (LinetypeOrigin::Drawing, true) => "Drawing (shadowed)",
                (LinetypeOrigin::Drawing, false) => "Drawing",
                (LinetypeOrigin::Library, _) => "Library",
            }),
            row.group.clone(),
            String::from(kind_text(row.kind)),
            period_text(row),
            format!("{}", row.users.entities),
            if row.origin == LinetypeOrigin::Drawing {
                row.description.clone()
            } else {
                row.source_file.clone()
            },
        ]
    }

    fn linetypes_page(&mut self, ui: &mut Ui, host: &mut dyn Host, front: bool) {
        page_record(ui, "linetypesPage");
        let mut refilter = false;
        ui.horizontal(|ui| {
            if front {
                refilter |= self.linetype_filter.ui(ui).any();
            }
            let items: Vec<String> = self.groups.iter().map(|(label, _)| label.clone()).collect();
            let mut choice = Choice::at(self.group);
            if controls::choice(ui, "linetypeGroup", &items, &mut choice)
                .on_hover_text("Library linestyles of one group and the groups under it")
                .changed()
            {
                refilter = true;
            }
            if let Some(index) = choice.index {
                self.group = index;
            }
        });
        if refilter && self.live() && self.refilter_linetypes() {
            self.load_linetype_form(host.document());
            self.update_buttons(host.document());
        }
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                let rows: Vec<Vec<String>> = self
                    .linetype_view
                    .iter()
                    .map(|&row| Self::linetype_cells(&self.linetype_rows[row]))
                    .collect();
                let highlighted: Vec<bool> = self
                    .linetype_view
                    .iter()
                    .map(|row| self.linetype_selected.contains(row))
                    .collect();
                let shown = table(
                    ui,
                    "linetypeTable",
                    &LINETYPE_HEADERS,
                    &rows,
                    None,
                    &highlighted,
                    None,
                );
                if let Some(&row) = shown.chosen.and_then(|at| self.linetype_view.get(at))
                    && self.live()
                    && pick(&mut self.linetype_selected, row, shown.adding)
                {
                    self.load_linetype_form(host.document());
                    self.update_buttons(host.document());
                }
            });
            ui.vertical(|ui| {
                // The three faces of the detail pane; those not shown are there all the same.
                let faces: [(Detail, &str, fn(&mut Self, &mut Ui, &mut dyn Host)); 3] = [
                    (Detail::None, "noLinetypeFace", Self::no_linetype_face),
                    (Detail::Pattern, "patternEditor", Self::pattern_face),
                    (Detail::Library, "libraryPage", Self::library_face),
                ];
                for (detail, name, face) in faces {
                    if detail == self.detail {
                        face(self, ui, host);
                    } else {
                        offstage(ui, name, |ui| face(self, ui, host));
                    }
                }
                ui.group(|ui| {
                    ui.strong("As drawn");
                    self.linetype_preview.ui(
                        ui,
                        "linetypePreview",
                        host.document(),
                        egui::vec2(360.0, 120.0),
                    );
                    ui.horizontal(|ui| {
                        if Self::preview_controls(
                            ui,
                            "linetypePreviewScale",
                            "linetypePreviewScreen",
                            &mut self.linetype_preview_scale,
                            &mut self.linetype_preview_screen,
                        ) {
                            Self::apply_preview_controls(
                                &mut self.linetype_preview,
                                self.linetype_preview_scale,
                                self.linetype_preview_screen,
                            );
                        }
                    });
                });
            });
        });
        self.linetype_buttons(ui, host);
    }

    fn no_linetype_face(&mut self, ui: &mut Ui, _host: &mut dyn Host) {
        controls::label(ui, "noLinetype", &self.no_linetype);
    }

    fn pattern_face(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        ui.group(|ui| {
            ui.strong("Pattern");
            let buttons = self.buttons.clone();
            ui.add_enabled_ui(self.pattern_enabled, |ui| {
                let mut kind_chosen: Option<(usize, Element)> = None;
                let mut length_typed: Option<(usize, f64)> = None;
                let mut clicked: Option<usize> = None;
                let kinds: Vec<String> = ["Dash", "Gap", "Dot"]
                    .iter()
                    .map(|kind| String::from(*kind))
                    .collect();
                let grid = egui::Grid::new("patternGrid")
                    .num_columns(3)
                    .show(ui, |ui| {
                        ui.strong("");
                        ui.strong("Element");
                        ui.strong("Length");
                        ui.end_row();
                        for row in 0..self.pattern_kinds.len() {
                            if ui
                                .selectable_label(
                                    self.pattern_current == Some(row),
                                    format!("{}", row + 1),
                                )
                                .clicked()
                            {
                                clicked = Some(row);
                            }
                            let shown = self.pattern_kinds[row];
                            let mut choice = Choice::at(
                                ELEMENTS
                                    .iter()
                                    .position(|&kind| kind == shown)
                                    .unwrap_or(0),
                            );
                            if controls::choice(
                                ui,
                                &format!("patternKind{row}"),
                                &kinds,
                                &mut choice,
                            )
                            .changed()
                                && let Some(&kind) =
                                    choice.index.and_then(|index| ELEMENTS.get(index))
                            {
                                kind_chosen = Some((row, kind));
                            }
                            ui.add_enabled_ui(self.pattern_length_enabled[row], |ui| {
                                if controls::spin(
                                    ui,
                                    &format!("patternLength{row}"),
                                    &mut self.pattern_lengths[row],
                                    PATTERN_LENGTH,
                                )
                                .changed()
                                {
                                    length_typed = Some((row, self.pattern_lengths[row]));
                                }
                            });
                            ui.end_row();
                        }
                    });
                let response = grid.response.on_hover_text(
                    "A dash is drawn, a gap is not, a dot is a point: lengths in model units, so \
                     a 1 m dash is 1 m on the ground at every scale",
                );
                let mut record = ControlRecord::new("patternGrid", ControlKind::Rows);
                record.items = (0..self.pattern_kinds.len())
                    .map(|row| {
                        format!(
                            "{} | {}",
                            kinds[ELEMENTS
                                .iter()
                                .position(|&kind| kind == self.pattern_kinds[row])
                                .unwrap_or(0)],
                            qt_fixed(self.pattern_lengths[row], 4)
                        )
                    })
                    .collect();
                custom(ui, &response, record);
                if self.live() {
                    if let Some(row) = clicked {
                        self.set_pattern_current(host.document(), row);
                    }
                    if let Some((row, kind)) = kind_chosen {
                        self.change_element(row, kind);
                        self.update_buttons(host.document());
                    }
                    if let Some((row, value)) = length_typed {
                        self.change_length(row, value);
                        self.update_buttons(host.document());
                    }
                }
                ui.horizontal_wrapped(|ui| {
                    let adds = [
                        ("patternAddDash", "+ Dash", "Add a dash after the selected element", 1.0),
                        ("patternAddGap", "+ Gap", "Add a gap after the selected element", -0.5),
                        ("patternAddDot", "+ Dot", "Add a dot after the selected element", 0.0),
                    ];
                    for (name, text, tip, length) in adds {
                        if Self::page_button(ui, true, name, text, tip) && self.live() {
                            self.insert_element(host.document(), length);
                        }
                    }
                    if Self::page_button(
                        ui,
                        buttons.remove_element,
                        "patternRemove",
                        "Remove",
                        "Remove the selected element",
                    ) && self.live()
                    {
                        self.remove_element(host.document());
                    }
                    if Self::page_button(
                        ui,
                        buttons.move_up,
                        "patternUp",
                        "Up",
                        "Move the selected element earlier",
                    ) && self.live()
                    {
                        self.move_element(host.document(), false);
                    }
                    if Self::page_button(
                        ui,
                        buttons.move_down,
                        "patternDown",
                        "Down",
                        "Move the selected element later",
                    ) && self.live()
                    {
                        self.move_element(host.document(), true);
                    }
                });
            });
            self.pattern_strip(ui);
            let mut said = egui::RichText::new(self.pattern_status.as_str());
            if self.pattern_status_is_error {
                said = said.color(ERROR_COLOUR);
            }
            controls::label_as(
                ui,
                "patternStatus",
                &self.pattern_status,
                egui::Label::new(said).wrap(),
            );
            ui.horizontal(|ui| {
                ui.label("Description");
                if controls::line(ui, "linetypeDescription", &mut self.linetype_description)
                    .changed()
                    && self.live()
                {
                    self.description_edited = true;
                    self.update_buttons(host.document());
                }
            });
            ui.horizontal(|ui| {
                if Self::page_button(
                    ui,
                    self.buttons.revert_linetype,
                    "linetypeRevert",
                    "Revert",
                    "Forget the changes made above",
                ) && self.live()
                {
                    self.load_linetype_form(host.document());
                    self.update_buttons(host.document());
                }
                if Self::page_button(
                    ui,
                    self.buttons.save_linetype,
                    "linetypeSave",
                    "Save",
                    "Store the pattern and description; disabled while the pattern is invalid",
                ) {
                    self.save_linetype(host);
                }
            });
        });
    }

    /// One repeat of the pattern being edited, three times across: what Save would store, before
    /// it is stored. The preview beside it draws the stored linetype as the drawing does; this
    /// draws the grid.
    fn pattern_strip(&self, ui: &mut Ui) {
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width().clamp(120.0, 360.0), 28.0),
            egui::Sense::hover(),
        );
        custom(
            ui,
            &response,
            ControlRecord::new("patternStrip", ControlKind::Other),
        );
        if !ui.is_rect_visible(rect) {
            return;
        }
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, ui.visuals().extreme_bg_color);
        let ink = ui.visuals().text_color();
        let stroke = egui::Stroke::new(2.0, ink);
        let y = rect.center().y;
        let left = rect.left() + 8.0;
        let right = rect.right() - 8.0;
        let period: f64 = self.working_pattern.iter().map(|length| length.abs()).sum();
        if self.working_pattern.is_empty() || period.is_nan() || period <= 0.0 || right <= left {
            painter.line_segment([egui::pos2(left, y), egui::pos2(right, y)], stroke);
            return;
        }
        let scale = f64::from(right - left) / (3.0 * period);
        let mut x = f64::from(left);
        // A bound on the loop, not a limit anyone sees: three repeats of even a long pattern are
        // far fewer elements.
        for placed in 0..4096_usize {
            if x >= f64::from(right) {
                break;
            }
            let length = self.working_pattern[placed % self.working_pattern.len()];
            if length > 0.0 {
                let end = (x + length * scale).min(f64::from(right));
                painter.line_segment(
                    [egui::pos2(x as f32, y), egui::pos2(end as f32, y)],
                    stroke,
                );
            } else if length == 0.0 {
                painter.circle_filled(egui::pos2(x as f32, y), 1.5, ink);
            }
            x += length.abs() * scale;
        }
    }

    fn library_face(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        ui.group(|ui| {
            ui.strong("Library linestyle");
            controls::label_as(
                ui,
                "libraryDetails",
                &self.library_details,
                egui::Label::new(self.library_details.as_str()).wrap(),
            );
            if Self::page_button(
                ui,
                true,
                "newStyleUsing",
                "New Style Using This",
                "A drawing style whose linetype is this linestyle",
            ) && self.live()
            {
                self.ask_new_style_using(host.document());
            }
            // The definition editor's door here (customisation::definition_editor): the editor is
            // the maker's, and has New, Duplicate and Delete of its own.
            if Self::page_button(
                ui,
                self.context.edit_definition.is_some(),
                "linetypeEditDefinition",
                "Edit Definition...",
                "Change this linestyle's definition - its strokes, units and length - in the \
                 definition editor, which also makes, copies and deletes one",
            ) && self.live()
            {
                self.edit_definition();
            }
        });
    }

    fn linetype_buttons(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        let buttons = self.buttons.clone();
        ui.horizontal_wrapped(|ui| {
            if Self::page_button(
                ui,
                true,
                "linetypeNew",
                "New...",
                "A new drawing linetype: a dash and a gap to start from",
            ) && self.live()
            {
                self.ask_new_linetype(host.document());
            }
            if Self::page_button(
                ui,
                buttons.duplicate_linetype,
                "linetypeDuplicate",
                "Duplicate...",
                "A copy of the selected drawing linetype",
            ) && self.live()
            {
                self.ask_duplicate_linetype(host.document());
            }
            if Self::page_button(
                ui,
                buttons.rename_linetype,
                "linetypeRename",
                "Rename...",
                "Rename it; every layer and style naming it comes too, as one undo step",
            ) && self.live()
            {
                self.ask_rename_linetype();
            }
            if Self::page_button(
                ui,
                buttons.merge_linetypes,
                "linetypeMerge",
                "Merge Into...",
                "Point every layer and style naming the selected linetypes at another, then \
                 delete them: one undo step",
            ) && self.live()
            {
                self.ask_merge_linetypes(host.document());
            }
            if Self::page_button(
                ui,
                buttons.delete_linetypes,
                "linetypeDelete",
                "Delete",
                "Refused while a layer or style names it, saying how many and which first",
            ) && self.live()
            {
                self.delete_linetypes(host);
            }
            if Self::page_button(
                ui,
                true,
                "linetypePurge",
                "Purge...",
                "Delete the drawing linetypes nothing uses: one undo step",
            ) {
                self.show_purge(
                    host,
                    PurgeOptions {
                        styles: false,
                        linetypes: true,
                        hatches: false,
                        ..PurgeOptions::default()
                    },
                    "Drawing linetypes nothing uses - uncheck any to keep:",
                );
            }
            // The definition editor's other door here: a library linestyle is begun from the tab
            // that lists them, with none to select first (the library page above shows only for
            // one that exists).
            if Self::page_button(
                ui,
                self.context.edit_definition.is_some(),
                "linetypeNewDefinition",
                "New Library Linestyle...",
                "Make a linestyle definition in the session's library, in the definition editor",
            ) && self.live()
                && let Some(edit) = &self.context.edit_definition
            {
                edit(DefinitionEdit::NewLinestyle, "");
            }
            if Self::page_button(
                ui,
                buttons.select_linetype_users,
                "linetypeSelectUsers",
                "Select Users",
                "Select the entities drawn with the selected linetypes",
            ) && self.live()
            {
                let names: Vec<String> = self
                    .selected_linetype_rows()
                    .into_iter()
                    .map(|row| row.name.clone())
                    .collect();
                self.select_users(host, UsageTable::Linetype, &names);
            }
        });
    }

    // ---- the diagnostics page ------------------------------------------------------------------

    fn diagnostics_page(&mut self, ui: &mut Ui, host: &mut dyn Host, _front: bool) {
        page_record(ui, "diagnosticsPage");
        controls::label_as(
            ui,
            "diagnosticSummary",
            &self.diagnostic_summary,
            egui::Label::new(self.diagnostic_summary.as_str()).wrap(),
        );
        let rows: Vec<Vec<String>> = self
            .diagnostic_rows
            .iter()
            .map(|row| {
                vec![
                    String::from(match row.kind {
                        StyleDiagnosticKind::MissingLinetype => "Linetype not defined",
                        StyleDiagnosticKind::MissingSymbol => "Symbol not defined",
                        StyleDiagnosticKind::Collision => "Name in both",
                    }),
                    row.name.clone(),
                    if row.users.used() {
                        row.users.describe()
                    } else {
                        String::from("nothing")
                    },
                    row.drawn_as.clone(),
                ]
            })
            .collect();
        let highlighted: Vec<bool> = (0..rows.len())
            .map(|row| self.diagnostic_selected.contains(&row))
            .collect();
        let shown = table(
            ui,
            "diagnosticTable",
            &DIAGNOSTIC_HEADERS,
            &rows,
            None,
            &highlighted,
            None,
        );
        if let Some(row) = shown.chosen
            && self.live()
            && pick(&mut self.diagnostic_selected, row, shown.adding)
        {
            self.update_buttons(host.document());
        }
        if Self::page_button(
            ui,
            self.buttons.select_diagnostic_users,
            "diagnosticSelectUsers",
            "Select Users",
            "Select the entities drawn with the selected names",
        ) && self.live()
        {
            self.select_diagnostic_users(host);
        }
    }

    // ---- the prompt row and the purge panel ----------------------------------------------------

    fn prompt_row(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        let open = self.prompt.is_some();
        ui.scope(|ui| {
            if !open {
                ui.set_invisible();
            }
            let row = ui.horizontal(|ui| {
                controls::label(ui, "promptLabel", &self.prompt_label);
                let mut entered = false;
                ui.scope(|ui| {
                    if self.prompt_is_choice {
                        ui.set_invisible();
                    }
                    let name = controls::line(ui, "promptName", &mut self.prompt_name);
                    entered =
                        name.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
                });
                ui.scope(|ui| {
                    if !self.prompt_is_choice {
                        ui.set_invisible();
                    }
                    let mut choice = Choice {
                        index: self.prompt_choice,
                        ..Choice::default()
                    };
                    controls::choice(ui, "promptChoice", &self.prompt_choices, &mut choice);
                    self.prompt_choice = choice.index;
                });
                if controls::button(ui, "promptOk", "OK").clicked() || entered {
                    self.accept_prompt(host);
                }
                if controls::button(ui, "promptCancel", "Cancel").clicked() {
                    self.prompt = None;
                }
            });
            custom(
                ui,
                &row.response,
                ControlRecord::new("promptPanel", ControlKind::Other),
            );
        });
    }

    fn purge_panel(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        let open = self.purge_open;
        ui.scope(|ui| {
            if !open {
                ui.set_invisible();
            }
            let panel = ui.vertical(|ui| {
                ui.label(self.purge_label.as_str());
                let list = ui.vertical(|ui| {
                    for item in &mut self.purge_items {
                        ui.checkbox(&mut item.checked, item.label.as_str());
                    }
                });
                let mut record = ControlRecord::new("purgeList", ControlKind::Rows);
                record.items = self
                    .purge_items
                    .iter()
                    .map(|item| item.label.clone())
                    .collect();
                custom(ui, &list.response, record);
                ui.horizontal(|ui| {
                    if controls::button(ui, "purgeOk", "Purge Checked")
                        .on_hover_text("Delete the checked items as one undo step")
                        .clicked()
                    {
                        self.accept_purge(host);
                    }
                    if controls::button(ui, "purgeCancel", "Cancel").clicked() {
                        self.purge_open = false;
                    }
                });
            });
            custom(
                ui,
                &panel.response,
                ControlRecord::new("purgePanel", ControlKind::Other),
            );
        });
    }
}
