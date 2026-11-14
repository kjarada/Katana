//! The survey code manager: the feature-code library a surveyor codes against - an editor of a
//! customisation's survey codes - in one non-modal dialog. The port of customisation/code_manager
//! (this file: the buffer, the files, the Code Table and its detail pane), code_manager_form (the
//! rule form) and code_manager_tabs (the other four tabs).
//!
//! Five tabs, each over one cad foundation, so the dialog decides nothing the CLI would say
//! differently:
//!
//! ```text
//! Code Table        cad's code_table, grouped, with cad's explain_code for the selected key or a
//!                   typed code, and the rule form
//! Codes in Drawing  cad's code_census over what the tab's scope takes, classed against the
//!                   buffer, and the CODE CENSUS line it stands for
//! Issues            cad's lint_survey_map on the buffer
//! Apply Codes       the CODE line, previewed by cad's apply_survey_codes
//! Linework          the LINEWORK line, previewed by cad's plan_linework, and the customisation's
//!                   control codes (CUSTOMISE SET)
//! ```
//!
//! THE THREE TABS ON DRAWING DATA RUN LINES. Each has the "Apply to" and "Only those that match"
//! controls every tool on drawing data has (scope_filter_widget, CLAUDE.md section 1.1), and its
//! Execute builds the line a person would type - CODE <scope> [PROPERTY p], LINEWORK <scope>
//! [PROPERTY p] [ORDER entity] [CHORD x], and for Codes in Drawing CODE CENSUS <scope> [PROPERTY
//! p] - and hands it to the window's executor (Host::run): echoed, kept in the history, one undo
//! step, answered in the verb's words. The line is shown as the controls change (applyCommand,
//! lineworkCommand). Preview stays a plan made here, read-only and unlogged, because it shows more
//! than a reply does - a row a code, every point in no line, every note - but it is planned by
//! what the verb plans with. The Linework tab's Use These Codes is the line CUSTOMISE SET
//! linework.*: the control codes are the customisation's, the Document's, and the tab shows them
//! as they are and follows them as they change.
//!
//! The tabs have no boxes that switch parts of the coding off (create layers, create styles, set
//! attributes) or keep the points a line replaces, because no line can say them: CODE always does
//! what the rules say, and LINEWORK never removes a point, and a second, direct path kept for them
//! would be two code paths for one job.
//!
//! EDITING HAPPENS IN A BUFFER (decision D1). The survey map is session data on the Document, not
//! undoable, so edits go to a copy of the Document's survey map through SurveyMap's own mutators,
//! and nothing reaches the Document until APPLY (set_survey_map). REVERT takes the Document's map
//! back. A map changed from outside while the buffer is unedited is taken up; with edits pending
//! it is not, and the log says so - Apply then replaces it, Revert takes it.
//!
//! Apply Codes and Linework run against the DOCUMENT'S map: the verbs read the Document, which
//! cannot be handed a buffer. The tabs say so while the buffer has unapplied edits.
//!
//! Every control a test or the headless driver needs has its C++ object name; the tabs are
//! codeTableTab, codesInDrawingTab, codeIssuesTab, applyCodesTab and lineworkTab. A tab that is not
//! in front is drawn offstage, so its controls are still there to fill and press. The dialog may
//! outlive its Document: it watches it through a DocumentWatcher and touches it only while that
//! says the Document is there.
//!
//! THE THREE FILTER BARS. Each has a search field `filterText` and a chip `filterAll`, and a
//! target holds a name once. The Code Table's bar is the dialog's own - the first of the three, as
//! the C++'s findChild found it - and the other two are targets of their own inside the dialog,
//! `censusFilter` and `issuesFilter`. The Code Table's chip for the text section is named
//! `filterText` too, as it was in the C++, where a press found the button and a fill the field:
//! here the field answers for both, a fill writing the search and a press pressing the chip.
//!
//! What is the C++'s and not here. The Code Table shows every rule under its key, where the C++'s
//! tree showed a key's rules once the key was opened. Closing with unapplied edits asks in the
//! dialog's own footer (Apply / Discard / Cancel: unappliedApply, unappliedDiscard,
//! unappliedCancel), where the C++ opened a message box.

use std::collections::BTreeMap;
use std::path::Path;
use std::rc::Rc;

use egui::{Context, RichText, Ui};
use katana_cad::Document;
use katana_cad::code_edit::{code_list_csv, suggested_key};
use katana_cad::code_table::{
    ColourLookup, DefinitionLookup, LintIssue, code_table, code_table_row_matches, explain_code,
};
use katana_cad::colour_lookup::colour_lookup;
use katana_cad::customisation_host::read_customisation_file;
use katana_cad::customisation_merge::{LoadMode, merge_customisation};
use katana_core::error::make_error;
use katana_core::path_text::path_to_utf8;
use katana_core::text::format_exact_real;
use katana_core::{Error, ErrorCode, Status};
use katana_entity::entity::to_string;
use katana_entity::linework_codes::LineworkCodes;
use katana_entity::{
    Customisation, EntityId, SurveyBreakline, SurveyMap, SurveyMatchKind, SurveyRule,
    SurveySection,
};

use super::code_manager_form::RuleForm;
use super::code_manager_support::{
    DefinitionState, colour_of, colour_swatch, customisation_file_filter,
    customisation_file_word_of, definition_label, exported_customisation_name, linestyle_state,
    symbol_state, write_file_bytes,
};
use super::code_manager_tabs::TabState;
use super::codes_seams::{CodesContext, StylePreview, UNDEFINED_NAME_COLOUR};
use super::definition_thumbnails::ThumbnailKind;
use super::document_watcher::{DocumentChanges, DocumentWatcher};
use super::filter_bar::FilterBar;
use super::scope_filter_widget::ScopeFilterView;
use crate::annotation::dialog_support::{show_dialog, table};
use crate::canvas::Image;
use crate::command_word::command_word;
use crate::drive::controls::{self, LineOptions, custom};
use crate::drive::mailbox::{
    Act, fill_disabled, lists_no, no_button, press_disabled, press_hidden,
};
use crate::drive::registry::{ControlKind, ControlRecord, record, settle, take};
use crate::host::{Host, VerbOutcome};
use crate::shell::DialogFrame;
use crate::survey::survey_dialogs::{hidden, offstage, open_file_dialog, save_file_dialog};
use crate::theme::palette;

const DIALOG: &str = "surveyCodeManagerDialog";

/// The colour of a refusal in the form and the tabs (#d9534f), as the C++'s stylesheets gave it.
pub(super) const REFUSAL_COLOUR: egui::Color32 = egui::Color32::from_rgb(0xd9, 0x53, 0x4f);

/// The object names of the five tabs' pages, in order.
pub(super) const TAB_PAGES: [&str; 5] = [
    "codeTableTab",
    "codesInDrawingTab",
    "codeIssuesTab",
    "applyCodesTab",
    "lineworkTab",
];

/// The Code Table's section chips, and the sections each stands for: the three pipe sections are
/// one subject to a person, and so are the two attribute sections. The first of each pair is the
/// label a chip's object name was made from when it was first shown ("filterMap",
/// "filterTinable"): tests and headless scripts find a chip by that name, so it outlives the
/// label. What a chip SAYS is section_chip_label's, and is written nowhere here.
const SECTION_CHIPS: [(&str, &[SurveySection]); 7] = [
    ("All", &[]),
    ("Map", &[SurveySection::Map]),
    ("Symbol", &[SurveySection::VertexSymbol]),
    ("Text", &[SurveySection::VertexTextStyle]),
    (
        "Pipe",
        &[
            SurveySection::Pipe,
            SurveySection::VertexPipe,
            SurveySection::SegmentPipe,
        ],
    ),
    (
        "Attributes",
        &[
            SurveySection::StringAttribute,
            SurveySection::VertexAttribute,
        ],
    ),
    ("Tinable", &[SurveySection::Tinable]),
];

/// What a chip says: the word of the first section it stands for, from entity's to_string - the
/// one table the Section list and a customisation file's "sets" take theirs from - with a
/// capital, as a chip is written. A copy typed here would go on saying the old word when that
/// table changed. "All" for the chip that stands for no section.
fn section_chip_label(sections: &[SurveySection]) -> String {
    let Some(first) = sections.first() else {
        return String::from("All");
    };
    let word = to_string(*first);
    let mut letters = word.chars();
    match letters.next() {
        Some(initial) => initial.to_uppercase().chain(letters).collect(),
        None => String::new(),
    }
}

/// The Code Table's columns.
const KEY_COLUMN: usize = 0;
const DESCRIPTION_COLUMN: usize = 1;
const LAYER_COLUMN: usize = 2;
const COLOUR_COLUMN: usize = 3;
const BREAKLINE_COLUMN: usize = 4;
const LINESTYLE_COLUMN: usize = 5;
const SYMBOL_COLUMN: usize = 6;
const TINABLE_COLUMN: usize = 7;
const ATTRIBUTES_COLUMN: usize = 8;
const COLUMN_COUNT: usize = 9;

/// The Code Table's headers, by column.
pub const CODE_TABLE_HEADERS: [&str; COLUMN_COUNT] = [
    "Key",
    "Description",
    "Layer",
    "Colour",
    "Line/Point",
    "Linestyle",
    "Symbol",
    "Surface",
    "Attributes",
];

/// A picture in the Code Table: 48 by 16 device pixels.
const THUMBNAIL: (u32, u32) = (48, 16);

fn breakline_text(breakline: Option<SurveyBreakline>) -> &'static str {
    match breakline {
        None => "",
        Some(SurveyBreakline::Line) => "Line",
        Some(SurveyBreakline::Point) => "Point",
    }
}

fn yes_no(value: Option<bool>) -> &'static str {
    match value {
        None => "",
        Some(true) => "yes",
        Some(false) => "no",
    }
}

fn attribute_names(rule: &SurveyRule) -> String {
    [
        &rule.attributes,
        &rule.vertex_attributes,
        &rule.segment_attributes,
    ]
    .into_iter()
    .flatten()
    .map(|attribute| attribute.name.as_str())
    .collect::<Vec<_>>()
    .join(", ")
}

/// "#7 AC* (symbol)": a rule by its place, key and section.
pub(super) fn cite(rule: usize, key: &str, section: SurveySection) -> String {
    format!("#{rule} {key} ({})", to_string(section))
}

pub(super) fn match_class(kind: SurveyMatchKind, matched: bool) -> &'static str {
    if matched {
        "matched"
    } else if kind == SurveyMatchKind::FallbackOnly {
        "fallback only"
    } else {
        "unmatched"
    }
}

/// A file's name as a person reads it, for the log: never its folder.
fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| String::from_utf8_lossy(&path_to_utf8(Path::new(name))).into_owned())
        .unwrap_or_default()
}

/// QString::toHtmlEscaped.
fn html_escaped(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            other => out.push(other),
        }
    }
    out
}

// ---- a tree of rows as one named control --------------------------------------------------------

/// One row of a tree drawn by `tree_rows`.
pub(crate) struct TreeLine<'a> {
    /// How deep the row sits: its first cell is set in by it.
    pub depth: usize,
    pub cells: &'a [String],
    /// By column: drawn in the amber of a name nothing defines.
    pub marked: &'a [bool],
    pub bold: bool,
    /// By column: a picture before the cell's text, and its size in points.
    pub pictures: &'a [Option<(egui::TextureId, egui::Vec2)>],
    /// What the row says when the pointer rests on it; empty for nothing.
    pub tip: &'a str,
}

/// What a tree did this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TreeShown {
    /// The row a click or a fill made current.
    pub chosen: Option<usize>,
    pub double_clicked: Option<usize>,
}

/// A QTreeWidget of whole rows, named `name`: `headers` (none: no header row) over `lines`, the
/// row `current` shown chosen. A fill makes the first row whose first cell is the text current, as
/// fillField matches column 0 and a click on the row does; it is reported as a list is, the cells
/// joined by " | ".
pub(crate) fn tree_rows(
    ui: &mut Ui,
    name: &str,
    headers: &[&str],
    lines: &[TreeLine<'_>],
    current: Option<usize>,
) -> TreeShown {
    let mut shown = TreeShown::default();
    let taken = take(ui, name);
    if let Some(taken) = &taken {
        match &taken.posted.act {
            Act::Fill(text) => {
                if ui.is_visible() && !ui.is_enabled() {
                    settle(ui, taken, Err(fill_disabled(&taken.posted)));
                } else if let Some(at) = lines
                    .iter()
                    .position(|line| line.cells.first().is_some_and(|cell| cell == text))
                {
                    shown.chosen = Some(at);
                    settle(ui, taken, Ok(()));
                } else {
                    settle(ui, taken, Err(lists_no(name, text)));
                }
            }
            Act::Press => settle(ui, taken, Err(no_button(&taken.posted))),
            Act::ScrollTo => {}
        }
    }
    let amber = UNDEFINED_NAME_COLOUR.to_egui();
    let grid = egui::Grid::new(name)
        .striped(true)
        .num_columns(headers.len().max(1))
        .show(ui, |ui| {
            if !headers.is_empty() {
                for header in headers {
                    ui.strong(*header);
                }
                ui.end_row();
            }
            for (at, line) in lines.iter().enumerate() {
                for (column, cell) in line.cells.iter().enumerate() {
                    ui.horizontal(|ui| {
                        if column == 0 {
                            ui.add_space(14.0 * line.depth as f32);
                        }
                        if let Some(Some((texture, size))) = line.pictures.get(column) {
                            let (rect, _) = ui.allocate_exact_size(*size, egui::Sense::hover());
                            ui.painter().image(
                                *texture,
                                rect,
                                egui::Rect::from_min_max(
                                    egui::pos2(0.0, 0.0),
                                    egui::pos2(1.0, 1.0),
                                ),
                                egui::Color32::WHITE,
                            );
                        }
                        let mut text = RichText::new(cell.as_str());
                        if line.bold {
                            text = text.strong();
                        }
                        if line.marked.get(column).copied().unwrap_or(false) {
                            text = text.color(amber);
                        }
                        let mut response = ui.selectable_label(current == Some(at), text);
                        if !line.tip.is_empty() {
                            response = response.on_hover_text(line.tip);
                        }
                        if response.double_clicked() {
                            shown.double_clicked = Some(at);
                        }
                        if response.clicked() {
                            shown.chosen = Some(at);
                        }
                    });
                }
                ui.end_row();
            }
        });
    let response = grid.response;
    if let Some(taken) = &taken
        && taken.posted.act == Act::ScrollTo
    {
        response.scroll_to_me(None);
        settle(ui, taken, Ok(()));
    }
    let mut tree = ControlRecord::new(name, ControlKind::Rows);
    tree.items = lines.iter().map(|line| line.cells.join(" | ")).collect();
    tree.value = shown
        .chosen
        .or(current)
        .and_then(|at| lines.get(at))
        .and_then(|line| line.cells.first())
        .cloned()
        .unwrap_or_default();
    custom(ui, &response, tree);
    shown
}

// ---- the Code Table's rows ----------------------------------------------------------------------

/// What a row of the Code Table is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum TreeAt {
    /// A group heading, by its place among the headings.
    Group(usize),
    /// A key's row.
    Key(String),
    /// A rule's row, by the rule's index in the buffer.
    Rule(usize),
}

/// One row of the Code Table as it is drawn, top to bottom.
pub(super) struct FlatRow {
    pub(super) at: TreeAt,
    pub(super) depth: usize,
    pub(super) cells: Vec<String>,
    pub(super) marked: Vec<bool>,
    pub(super) images: Vec<Option<Image>>,
    pub(super) tip: String,
}

impl FlatRow {
    fn new(at: TreeAt, depth: usize) -> Self {
        Self {
            at,
            depth,
            cells: vec![String::new(); COLUMN_COUNT],
            marked: vec![false; COLUMN_COUNT],
            images: vec![None; COLUMN_COUNT],
            tip: String::new(),
        }
    }
}

/// A row of the Code Table with the rows under it: a group heading over its keys, a key over its
/// rules. What a test reads the table by.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CodeTreeRow {
    /// By column (CODE_TABLE_HEADERS).
    pub cells: Vec<String>,
    /// By column: the cell has a picture - a colour's swatch, a definition's thumbnail.
    pub pictured: Vec<bool>,
    /// By column: the cell is marked as naming what nothing defines.
    pub marked: Vec<bool>,
    pub children: Vec<CodeTreeRow>,
}

/// One row of the explanation: a field, its value, the rule that set it and what it decided
/// against; `rule` is the rule a double-click opens.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct ExplainRow {
    pub(super) cells: Vec<String>,
    pub(super) rule: usize,
}

/// One distinct code of the census.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct CensusRow {
    pub(super) code: String,
    pub(super) entities: usize,
    pub(super) kind: SurveyMatchKind,
    pub(super) matched: bool,
    pub(super) model: String,
}

/// What a double-click asked for, carried out at the next frame: the jump may rebuild the list
/// that was clicked, and no list is rebuilt inside its own click.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Jump {
    Rule(usize),
    TestCode(String),
}

/// What a frame's drawing asks for, carried out after it with the host.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Event {
    // The frame's own buttons.
    Import,
    Export,
    ExportCodeList,
    Apply,
    Revert,
    Close,
    CloseApply,
    CloseDiscard,
    CloseCancel,
    // The Code Table.
    TableFilter,
    Tree(usize),
    TestCode,
    ExplainActivated(usize),
    // The form.
    FormEdited,
    SectionChosen,
    FormNew,
    FormAdd,
    FormUpdate,
    FormDuplicate,
    FormDelete,
    FormUp,
    FormDown,
    // Codes in Drawing.
    CensusChanged,
    CensusFilter,
    CensusChosen(usize),
    CensusActivated(usize),
    CensusSelect,
    CensusNewRule,
    CensusRun,
    // Issues.
    IssuesFilter,
    IssueActivated(usize),
    // Apply Codes and Linework.
    LinesChanged,
    ApplyPreview,
    ApplyExecute,
    LineworkCodeTyped,
    LineworkUse,
    LineworkDefaults,
    LineworkPreview,
    LineworkExecute,
}

/// The views the scopes' "What a view shows" lists.
pub type CodeManagerViews = Rc<dyn Fn() -> Vec<ScopeFilterView>>;

/// The survey code manager (see the module comment).
pub struct SurveyCodeManagerDialog {
    pub(super) frame: DialogFrame,
    pub(super) open: bool,
    /// The dialog was shown since its last frame: the layers and views the scopes offer are the
    /// drawing's and the window's as they are then.
    pub(super) just_shown: bool,
    pub(super) context: CodesContext,
    pub(super) buffer: SurveyMap,
    /// The drawing's map as the manager last saw it - taken, applied, or changed elsewhere: what
    /// dirty() is measured against.
    pub(super) baseline: SurveyMap,
    /// The customisation's control codes as the seven fields were last filled from them: a change
    /// of the Document's is taken up, and what a person is typing is not overwritten by a change
    /// of anything else.
    pub(super) shown_linework: LineworkCodes,
    /// None: interactive unless the host is headless.
    pub(super) interactive: Option<bool>,
    /// The close question is on the page.
    pub(super) asking_close: bool,
    pub(super) form_index: Option<usize>,
    pub(super) explained: String,
    pub(super) census: Vec<CensusRow>,
    pub(super) census_found_property: String,
    /// What the census tab's scope took at the last count, and why it took nothing when its
    /// controls could not be read: the table is of exactly these entities.
    pub(super) census_ids: Vec<EntityId>,
    pub(super) census_problem: String,
    pub(super) issues: Vec<LintIssue>,

    pub(super) tab: usize,
    pub(super) dirty_text: String,
    pub(super) import_replace: bool,
    // The Code Table.
    pub(super) table_filter: FilterBar,
    pub(super) table_chip_labels: Vec<String>,
    pub(super) tree: Vec<FlatRow>,
    pub(super) tree_current: Option<TreeAt>,
    pub(super) tree_textures: BTreeMap<(usize, usize), egui::TextureHandle>,
    pub(super) test_code: String,
    pub(super) explain_heading: String,
    pub(super) explain_fields: Vec<ExplainRow>,
    pub(super) explain_definitions: String,
    pub(super) explain_near_misses: String,
    pub(super) linestyle_preview: StylePreview,
    pub(super) symbol_preview: StylePreview,
    pub(super) form: RuleForm,
    pub(super) tabs: TabState,

    pub(super) pending: Option<Jump>,
    pub(super) events: Vec<Event>,
    pub(super) logs: Vec<(String, bool)>,
    pub(super) views: Option<CodeManagerViews>,
    pub(super) watcher: DocumentWatcher,
}

impl SurveyCodeManagerDialog {
    /// The manager over `document`, shown, its buffer the drawing's survey map.
    pub fn new(document: &Document, context: CodesContext) -> Self {
        let names: Vec<&str> = SECTION_CHIPS.iter().map(|(name, _)| *name).collect();
        let mut table_filter = FilterBar::new(&names);
        let table_chip_labels: Vec<String> = SECTION_CHIPS
            .iter()
            .map(|(_, sections)| section_chip_label(sections))
            .collect();
        for (index, label) in table_chip_labels.iter().enumerate() {
            table_filter.set_chip_label(index, label);
        }
        table_filter
            .set_placeholder_text("Search keys, descriptions, layers, linestyles, symbols");
        let buffer = document.survey_map().clone();
        let mut dialog = Self {
            frame: DialogFrame::new(DIALOG, "Survey Code Manager"),
            open: true,
            just_shown: false,
            context,
            baseline: buffer.clone(),
            buffer,
            shown_linework: LineworkCodes::default(),
            interactive: None,
            asking_close: false,
            form_index: None,
            explained: String::new(),
            census: Vec::new(),
            census_found_property: String::new(),
            census_ids: Vec::new(),
            census_problem: String::new(),
            issues: Vec::new(),
            tab: 0,
            dirty_text: String::new(),
            import_replace: false,
            table_filter,
            table_chip_labels,
            tree: Vec::new(),
            tree_current: None,
            tree_textures: BTreeMap::new(),
            test_code: String::new(),
            explain_heading: String::new(),
            explain_fields: Vec::new(),
            explain_definitions: String::new(),
            explain_near_misses: String::new(),
            linestyle_preview: StylePreview::new(),
            symbol_preview: StylePreview::new(),
            form: RuleForm::new(Some(document)),
            tabs: TabState::new(),
            pending: None,
            events: Vec::new(),
            logs: Vec::new(),
            views: None,
            watcher: DocumentWatcher::new(document),
        };
        let document = Some(document);
        dialog.rebuild_code_table(document);
        dialog.rebuild_issues(document);
        dialog.refresh_census(document);
        dialog.load_linework_codes(document);
        dialog.reload_scopes(document);
        dialog.show_lines();
        dialog.load_form(document, &SurveyRule::default(), None);
        dialog.update_dirty();
        dialog
    }

    pub(super) fn alive(&self) -> bool {
        self.watcher.document_alive()
    }

    /// `document`, while it is the Document the manager was made with and that is still there.
    pub(super) fn live_document<'d>(&self, document: &'d Document) -> Option<&'d Document> {
        self.alive().then_some(document)
    }

    pub(super) fn log(&mut self, message: impl Into<String>, is_error: bool) {
        self.logs.push((message.into(), is_error));
    }

    /// What was said since the last flush, to the host's log in the order it was said.
    pub(super) fn flush(&mut self, host: &mut dyn Host) {
        for (message, is_error) in std::mem::take(&mut self.logs) {
            host.log(&message, is_error);
        }
    }

    fn closed_error() -> Error {
        Error::new(
            ErrorCode::InvalidState,
            "the drawing this manager edits is closed",
        )
    }

    // ---- session -------------------------------------------------------------------------------

    /// Interactive: file dialogs, and a question on closing with unapplied edits. Off, the dialog
    /// never asks (a headless session has no one to answer). On unless the host is headless.
    pub fn set_interactive(&mut self, interactive: bool) {
        self.interactive = Some(interactive);
    }

    pub fn interactive(&self, host: &dyn Host) -> bool {
        self.interactive.unwrap_or_else(|| !host.headless())
    }

    /// The workspace's open views, for the View scope of the three tabs on drawing data: asked
    /// whenever a view list or a View scope is needed, as Global Modify's is. None (a test, a
    /// manager built on its own): the View scope offers one "Whole drawing view", which a preview
    /// reads and no line can name.
    pub fn set_views(&mut self, views: Option<CodeManagerViews>) {
        self.views = views;
        self.install_views();
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Shows the manager: the layers and the views open now, not those of when it was built or
    /// last shown.
    pub fn show(&mut self) {
        self.open = true;
        self.just_shown = true;
    }

    /// The object name of the tab in front.
    pub fn current_tab(&self) -> &'static str {
        TAB_PAGES[self.tab.min(TAB_PAGES.len() - 1)]
    }

    // ---- the buffer ----------------------------------------------------------------------------

    pub fn buffer(&self) -> &SurveyMap {
        &self.buffer
    }

    /// The buffer differs from the drawing's map: the one it was taken from, last applied, or
    /// last seen changed elsewhere.
    pub fn dirty(&self) -> bool {
        self.buffer != self.baseline
    }

    /// The buffer onto the Document (set_survey_map), with the commit hook called round it: before
    /// it, and what it hands back after. InvalidState when the Document is gone - no commit, so
    /// the hook is not called.
    pub fn apply(&mut self, host: &mut dyn Host) -> Status {
        let applied = self.apply_in(host);
        self.flush(host);
        applied
    }

    fn apply_in(&mut self, host: &mut dyn Host) -> Status {
        if !self.alive() {
            return Err(Self::closed_error());
        }
        // Round the commit, the hook every editor of the customisation calls: before it, since
        // set_survey_map leaves the session "not kept" and only the maker of the context can have
        // noted that it was; and what it hands back after it, which is where a session that was
        // kept is kept again. This manager knows neither.
        let committed = self.context.begin(host);
        host.document_mut().set_survey_map(self.buffer.clone());
        if let Some(committed) = committed {
            committed(host);
        }
        self.baseline = self.buffer.clone();
        self.update_dirty();
        self.log(
            format!(
                "Survey map applied: {} rules over {} codes.",
                self.buffer.size(),
                self.buffer.keys().len()
            ),
            false,
        );
        Ok(())
    }

    /// The Document's map back into the buffer, discarding the edits.
    pub fn revert(&mut self, host: &mut dyn Host) {
        let document = self.live_document(host.document());
        self.revert_in(document);
        self.flush(host);
    }

    fn revert_in(&mut self, document: Option<&Document>) {
        let Some(found) = document else {
            return;
        };
        self.buffer = found.survey_map().clone();
        self.baseline = self.buffer.clone();
        if self
            .form_index
            .is_some_and(|index| index >= self.buffer.size())
        {
            self.form_index = None;
        }
        self.buffer_changed(document);
        match self.form_index {
            Some(index) => self.select_rule_in(document, index),
            None => self.load_form(document, &SurveyRule::default(), None),
        }
        self.log(
            "Edits discarded: the drawing's survey map is back in the manager.",
            false,
        );
    }

    pub(super) fn buffer_changed(&mut self, document: Option<&Document>) {
        self.rebuild_code_table(document);
        self.rebuild_issues(document);
        self.filter_census();
        self.update_dirty();
        if !self.test_code.is_empty() {
            let typed = self.test_code.clone();
            self.explain(document, &typed);
        } else if !self.explained.is_empty() {
            let explained = self.explained.clone();
            self.explain(document, &explained);
        }
        self.update_form_issues(document);
    }

    pub(super) fn update_dirty(&mut self) {
        let edited = self.dirty();
        self.dirty_text = if edited {
            format!(
                "Unapplied edits: Apply puts them on the drawing, Revert discards them. {} rules.",
                self.buffer.size()
            )
        } else {
            format!(
                "No unapplied edits: these are the drawing's {} rules.",
                self.buffer.size()
            )
        };
        self.frame.set_title(if edited {
            "Survey Code Manager *"
        } else {
            "Survey Code Manager"
        });
    }

    fn document_changed(&mut self, found: &Document, changes: DocumentChanges) {
        let document = Some(found);
        // The control codes are the customisation's, which the watcher has no flag for (a
        // CUSTOMISE SET moves none of the generations it compares), so they are compared here at
        // every delivery: a change of them - this tab's own line, a typed one, a customisation
        // loaded - fills the seven fields again. Nothing else does, so what a person is typing
        // there survives a selection click.
        if found.customisation_state().linework != self.shown_linework {
            self.load_linework_codes(document);
        }
        if !changes.any() {
            return;
        }
        if changes.survey_map && *found.survey_map() != self.baseline {
            if !self.dirty() {
                // Unedited: the manager shows what the drawing has.
                self.buffer = found.survey_map().clone();
                self.baseline = self.buffer.clone();
                self.form_index = None;
                self.buffer_changed(document);
                self.load_form(document, &SurveyRule::default(), None);
            } else {
                // The edits are kept, but what they are measured against moves to the map the
                // drawing now has: dirty() means "the buffer is not the drawing's map". Left on
                // the old map, undoing the edits would read as clean - Apply and Revert off, the
                // tabs' notes hidden - while Apply Codes and Linework ran a different map.
                self.baseline = found.survey_map().clone();
                self.update_dirty();
                if self.dirty() {
                    self.log(
                        "The drawing's survey map was changed elsewhere while the manager has \
                         unapplied edits. They are kept: Apply replaces the new map with them, \
                         Revert takes the new map.",
                        false,
                    );
                }
            }
        }
        if changes.library {
            // A change of the customisation's colours arrives as one of the library too
            // (Document::set_colour_table), so the fields that list them are filled again here.
            self.form.refresh_names(found);
            self.rebuild_code_table(document);
            self.rebuild_issues(document);
            if !self.explained.is_empty() {
                let explained = self.explained.clone();
                self.explain(document, &explained);
            }
            self.update_form_issues(document);
        }
        if changes.model {
            self.refresh_census(document);
            self.update_form_issues(document);
            // A layer made or deleted since: the lists the scopes tick from.
            self.reload_scopes(document);
            self.show_lines();
        }
    }

    // ---- editing: by rule index (SurveyMap's), each refused exactly as SurveyMap refuses it,
    // leaving the buffer as it was ----------------------------------------------------------------

    pub fn add_rule(&mut self, host: &mut dyn Host, rule: SurveyRule) -> Status {
        let document = self.live_document(host.document());
        let added = self.add_rule_in(document, rule);
        self.flush(host);
        added
    }

    pub(super) fn add_rule_in(&mut self, document: Option<&Document>, rule: SurveyRule) -> Status {
        self.buffer.add(rule)?;
        let index = self.buffer.size() - 1;
        self.form_index = Some(index);
        self.buffer_changed(document);
        self.select_rule_in(document, index);
        Ok(())
    }

    pub fn replace_rule(&mut self, host: &mut dyn Host, index: usize, rule: SurveyRule) -> Status {
        let document = self.live_document(host.document());
        let replaced = self.replace_rule_in(document, index, rule);
        self.flush(host);
        replaced
    }

    pub(super) fn replace_rule_in(
        &mut self,
        document: Option<&Document>,
        index: usize,
        rule: SurveyRule,
    ) -> Status {
        self.buffer.replace(index, rule)?;
        self.form_index = Some(index);
        self.buffer_changed(document);
        self.select_rule_in(document, index);
        Ok(())
    }

    pub fn duplicate_rule(&mut self, host: &mut dyn Host, index: usize) -> Status {
        let document = self.live_document(host.document());
        let duplicated = self.duplicate_rule_in(document, index);
        self.flush(host);
        duplicated
    }

    pub(super) fn duplicate_rule_in(
        &mut self,
        document: Option<&Document>,
        index: usize,
    ) -> Status {
        let rule = self.buffer.at(index)?;
        // Right after the original: the copy then loses every tie to it, so the map resolves
        // every code as before until the copy is edited.
        self.buffer.insert(index + 1, rule)?;
        self.form_index = Some(index + 1);
        self.buffer_changed(document);
        self.select_rule_in(document, index + 1);
        Ok(())
    }

    pub fn remove_rule(&mut self, host: &mut dyn Host, index: usize) -> Status {
        let document = self.live_document(host.document());
        let removed = self.remove_rule_in(document, index);
        self.flush(host);
        removed
    }

    pub(super) fn remove_rule_in(&mut self, document: Option<&Document>, index: usize) -> Status {
        self.buffer.remove(index)?;
        self.form_index = None;
        self.buffer_changed(document);
        self.load_form(document, &SurveyRule::default(), None);
        Ok(())
    }

    /// -1 up (earlier: wins more ties), +1 down.
    pub fn move_rule(&mut self, host: &mut dyn Host, index: usize, by: i32) -> Status {
        let document = self.live_document(host.document());
        let moved = self.move_rule_in(document, index, by);
        self.flush(host);
        moved
    }

    pub(super) fn move_rule_in(
        &mut self,
        document: Option<&Document>,
        index: usize,
        by: i32,
    ) -> Status {
        if by == 0 {
            return Ok(());
        }
        let steps = by.unsigned_abs() as usize;
        if by < 0 && index < steps {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "the first rule cannot move up",
            ));
        }
        let to = if by < 0 { index - steps } else { index + steps };
        if to >= self.buffer.size() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "the last rule cannot move down",
            ));
        }
        self.buffer.r#move(index, to)?;
        self.form_index = Some(to);
        self.buffer_changed(document);
        self.select_rule_in(document, to);
        Ok(())
    }

    /// The rule the form edits; None while it holds a new rule.
    pub fn current_rule(&self) -> Option<usize> {
        self.form_index
    }

    /// A new feature rule in the form for a code the drawing carries, keyed by cad's
    /// suggested_key, on the Code Table tab. Nothing is added until Add.
    pub fn new_rule_from_code(&mut self, host: &mut dyn Host, code: &str) {
        let document = self.live_document(host.document());
        self.new_rule_from_code_in(document, code);
        self.flush(host);
    }

    pub(super) fn new_rule_from_code_in(&mut self, document: Option<&Document>, code: &str) {
        let rule = SurveyRule {
            key: suggested_key(code),
            section: SurveySection::Map,
            ..SurveyRule::default()
        };
        self.tab = 0;
        self.tree_current = None;
        self.load_form(document, &rule, None);
        self.set_test_code(document, code);
    }

    // ---- files ---------------------------------------------------------------------------------

    /// Reads a Katana customisation file (docs/customisation.md) and merges ITS RULES into the
    /// BUFFER through cad's merge_customisation - Merge by default, as loading does everywhere
    /// (D1) - for review before Apply. The definitions, colours and settings the file also holds
    /// are left alone and counted in the log: this edits the survey codes only. NotFound for a
    /// file with no rules; what the format's reader refuses keeps its own error. The buffer is as
    /// it was after any failure.
    pub fn import_codes(&mut self, host: &mut dyn Host, path: &Path, mode: LoadMode) -> Status {
        let document = self.live_document(host.document());
        let imported = self.import_codes_in(document, path, mode);
        self.flush(host);
        imported
    }

    fn import_codes_in(
        &mut self,
        document: Option<&Document>,
        path: &Path,
        mode: LoadMode,
    ) -> Status {
        let file = read_customisation_file(path)?;
        let held = &file.customisation;
        if held.map.empty() {
            return Err(make_error(
                ErrorCode::NotFound,
                "the file holds no survey code rules",
                file_name(path),
            ));
        }
        // The merge every load goes through, over the two things this manager has a say in: the
        // buffer as "the session" and the file's rules, under the file's name, as the load. So
        // Merge and Replace mean here exactly what they mean to CUSTOMISE, and what else the file
        // holds cannot reach the buffer or the drawing.
        let session = Customisation {
            map: self.buffer.clone(),
            ..Customisation::default()
        };
        let rules = Customisation {
            name: held.name.clone(),
            map: held.map.clone(),
            ..Customisation::default()
        };
        let merged = merge_customisation(&session, std::slice::from_ref(&rules), mode);
        if !merged.ok() {
            // All or nothing, as the merge is: the buffer is as it was.
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!(
                    "the file's survey code rules could not be merged: {}",
                    merged.problems.first().cloned().unwrap_or_default()
                ),
                file_name(path),
            ));
        }
        self.buffer = merged.merged.map;
        for load in &merged.loads {
            // The merge counts a key once for each section it has rules in.
            self.log(
                format!(
                    "{} ({}): {} added, {} replaced in the rules being edited - a key once for \
                     each section it has rules in.",
                    load.name,
                    to_string(mode),
                    load.added_keys.len(),
                    load.replaced_keys.len()
                ),
                false,
            );
        }
        if !merged.removed_keys.is_empty() {
            self.log(
                format!("{} keys removed by Replace.", merged.removed_keys.len()),
                false,
            );
        }
        let mut left = Vec::new();
        if !held.library.empty() {
            left.push(format!("definitions ({})", held.library.size()));
        }
        if !held.colours.empty() {
            left.push(format!("colours ({})", held.colours.size()));
        }
        if held.linework.is_some() {
            left.push(String::from("the linework codes"));
        }
        if held.automation.is_some() {
            left.push(String::from("the automation switches"));
        }
        if !left.is_empty() {
            self.log(
                format!(
                    "Not imported from {}, as this manager edits the survey code rules: {}. \
                     Import Definitions in the Symbol Library takes definitions and colours; the \
                     line CUSTOMISE <file> loads a whole customisation.",
                    file_name(path),
                    left.join(", ")
                ),
                false,
            );
        }
        self.form_index = None;
        self.buffer_changed(document);
        self.load_form(document, &SurveyRule::default(), None);
        self.log("Imported; review the rules and Apply to use them.", false);
        Ok(())
    }

    /// Runs CUSTOMISE EXPORT <file> CODES through the window's executor: the drawing's rules as a
    /// Katana customisation file of survey codes alone, under the session's name (the file's own
    /// for a session with none) and with what of the session belongs with them (cad's
    /// customisation_part). The verb writes what the DRAWING has, so a buffer with unapplied edits
    /// is refused (Apply or Revert first) rather than exported as something no line can name.
    pub fn export_codes(&mut self, host: &mut dyn Host, path: &Path) -> Status {
        let exported = self.export_codes_in(host, path);
        self.flush(host);
        exported
    }

    fn export_codes_in(&mut self, host: &mut dyn Host, path: &Path) -> Status {
        if !self.alive() {
            return Err(Self::closed_error());
        }
        // What the verb writes is the DRAWING'S rules, so unapplied edits are not in it: it is
        // refused for them rather than write rules the person is not looking at: no line can name
        // the buffer.
        if self.dirty() {
            return Err(Error::new(
                ErrorCode::InvalidState,
                "the rules being edited are not on the drawing, and Export Codes writes the \
                 drawing's rules (CUSTOMISE EXPORT ... CODES): Apply or Revert the edits first",
            ));
        }
        let file = customisation_file_word_of(path)?;
        let mut line = format!("CUSTOMISE EXPORT {file} CODES");
        if host.document().customisation_state().name.is_empty() {
            // A session nothing was loaded into has no name, and a file must have one: the file's
            // own.
            let name = command_word(
                &exported_customisation_name(Some(host.document()), path),
                "the customisation's name",
            )?;
            line.push_str(" NAME ");
            line.push_str(&name);
        }
        if let Err(said) = self.run_line(host, &line) {
            return Err(make_error(
                ErrorCode::FileExportFailure,
                said,
                String::from_utf8_lossy(&path_to_utf8(path)).into_owned(),
            ));
        }
        self.log(
            format!(
                "Exported {} rules to {}.",
                self.buffer.size(),
                file_name(path)
            ),
            false,
        );
        Ok(())
    }

    /// The buffer's code list (cad's code_list_csv), UTF-8.
    pub fn export_code_list(&mut self, host: &mut dyn Host, path: &Path) -> Status {
        let written = write_file_bytes(path, code_list_csv(&self.buffer).as_bytes());
        if written.is_ok() {
            self.log(
                format!(
                    "Exported the code list ({} codes) to {}.",
                    self.buffer.keys().len(),
                    file_name(path)
                ),
                false,
            );
        }
        self.flush(host);
        written
    }

    /// A line through the window's executor. Refused, saying why, when the manager was given
    /// none: it then does NOT do the work itself.
    pub(super) fn run_line(
        &mut self,
        host: &mut dyn Host,
        line: &str,
    ) -> std::result::Result<VerbOutcome, String> {
        if !self.context.command_line {
            // Said, and nothing done: the work is the line's, and a manager with no command line
            // to hand it to does not do it some other way.
            return Err(format!(
                "This manager was opened without the window's command line, so it cannot run \
                 \"{line}\". Type it on a command line instead."
            ));
        }
        self.flush(host);
        let outcome = host.run(line);
        if outcome.ok {
            Ok(outcome)
        } else if outcome.error.is_empty() {
            Err(format!("\"{line}\" was refused."))
        } else {
            Err(outcome.error)
        }
    }

    // ---- closing -------------------------------------------------------------------------------

    /// Closing (Close, the title bar's close button): with unapplied edits an interactive session
    /// is asked Apply / Discard / Cancel; a headless one closes and keeps the buffer, and says so
    /// in the log.
    pub fn reject(&mut self, host: &mut dyn Host) {
        if self.dirty() {
            if self.interactive(host) {
                self.asking_close = true;
                return;
            }
            self.log(
                "Survey Code Manager closed with unapplied edits; they are kept in the manager \
                 until applied or reverted.",
                false,
            );
        }
        self.open = false;
        self.flush(host);
    }

    // ---- the code table ------------------------------------------------------------------------

    /// The Code Table as it stands: the group headings, each over its keys, each over its rules.
    pub fn code_tree(&self) -> Vec<CodeTreeRow> {
        let mut groups: Vec<CodeTreeRow> = Vec::new();
        for row in &self.tree {
            let made = CodeTreeRow {
                cells: row.cells.clone(),
                pictured: row.images.iter().map(Option::is_some).collect(),
                marked: row.marked.clone(),
                children: Vec::new(),
            };
            match row.depth {
                0 => groups.push(made),
                1 => {
                    if let Some(group) = groups.last_mut() {
                        group.children.push(made);
                    }
                }
                _ => {
                    if let Some(key) = groups.last_mut().and_then(|group| group.children.last_mut())
                    {
                        key.children.push(made);
                    }
                }
            }
        }
        groups
    }

    /// The Code Table's chips: each one's object name and what it says.
    pub fn table_chips(&self) -> Vec<(String, String)> {
        SECTION_CHIPS
            .iter()
            .zip(&self.table_chip_labels)
            .map(|((named, _), label)| (FilterBar::chip_object_name(named), label.clone()))
            .collect()
    }

    fn fill_row(&self, document: Option<&Document>, row: &mut FlatRow, rule: &SurveyRule) {
        row.cells[DESCRIPTION_COLUMN] = rule.comment.clone();
        row.cells[LAYER_COLUMN] = rule.model.clone();
        if !rule.colour.is_empty() {
            match colour_of(document, &rule.colour) {
                Some(rgb) => {
                    row.images[COLOUR_COLUMN] = Some(colour_swatch(&rgb, THUMBNAIL.1));
                    row.cells[COLOUR_COLUMN] = rule.colour.clone();
                }
                None => {
                    row.cells[COLOUR_COLUMN] = format!("{} (no RGB)", rule.colour);
                    row.marked[COLOUR_COLUMN] = true;
                    row.tip = String::from(
                        "A colour name neither the customisation's colours nor the standard \
                         names know: applying the code leaves the entity's colour as it is",
                    );
                }
            }
        }
        row.cells[BREAKLINE_COLUMN] = String::from(breakline_text(rule.breakline));
        let ground = palette::panel();
        if !rule.linestyle.is_empty() {
            let state = linestyle_state(document, &rule.linestyle);
            row.cells[LINESTYLE_COLUMN] = definition_label(&rule.linestyle, state);
            if state == DefinitionState::Undefined || state == DefinitionState::WrongKind {
                row.marked[LINESTYLE_COLUMN] = true;
            } else if state == DefinitionState::Defined
                && let Some(document) = document
                && let Some(thumbnails) = &self.context.thumbnails
            {
                row.images[LINESTYLE_COLUMN] = Some(
                    thumbnails
                        .borrow_mut()
                        .thumbnail_of(
                            document,
                            ThumbnailKind::Linestyle,
                            &rule.linestyle,
                            THUMBNAIL,
                            ground,
                        )
                        .image,
                );
            }
        }
        if let Some(symbol) = &rule.symbol
            && !symbol.style.is_empty()
        {
            let state = symbol_state(document, &symbol.style);
            let mut label = definition_label(&symbol.style, state);
            if symbol.size != 0.0 {
                label.push_str(", ");
                label.push_str(&format_exact_real(symbol.size));
            }
            row.cells[SYMBOL_COLUMN] = label;
            if state == DefinitionState::Undefined || state == DefinitionState::WrongKind {
                row.marked[SYMBOL_COLUMN] = true;
            } else if let Some(document) = document
                && let Some(thumbnails) = &self.context.thumbnails
            {
                row.images[SYMBOL_COLUMN] = Some(
                    thumbnails
                        .borrow_mut()
                        .thumbnail_of(
                            document,
                            ThumbnailKind::Symbol,
                            &symbol.style,
                            (THUMBNAIL.1, THUMBNAIL.1),
                            ground,
                        )
                        .image,
                );
            }
        }
        row.cells[TINABLE_COLUMN] = String::from(yes_no(rule.tinable));
        row.cells[ATTRIBUTES_COLUMN] = attribute_names(rule);
    }

    pub(super) fn rebuild_code_table(&mut self, document: Option<&Document>) {
        let filter = String::from(self.table_filter.text());
        let chip = self.table_filter.chip().unwrap_or(0);
        let wanted = SECTION_CHIPS[chip.min(SECTION_CHIPS.len() - 1)].1;

        // A group's rows follow its heading; the headings stand in the order their first key was
        // met.
        let mut groups: Vec<(String, Vec<FlatRow>)> = Vec::new();
        let mut keep = None;
        for row in code_table(&self.buffer) {
            if !code_table_row_matches(&row, &filter) {
                continue;
            }
            if !wanted.is_empty() && !row.sections.iter().any(|section| wanted.contains(section)) {
                continue;
            }
            let at = match groups
                .iter()
                .position(|(group, _)| *group == row.combined.group)
            {
                Some(at) => at,
                None => {
                    let mut heading = FlatRow::new(TreeAt::Group(groups.len()), 0);
                    heading.cells[KEY_COLUMN] = if row.combined.group.is_empty() {
                        String::from("(no group)")
                    } else {
                        row.combined.group.clone()
                    };
                    groups.push((row.combined.group.clone(), vec![heading]));
                    groups.len() - 1
                }
            };
            let mut key_row = FlatRow::new(TreeAt::Key(row.key.clone()), 1);
            key_row.cells[KEY_COLUMN] = row.key.clone();
            self.fill_row(document, &mut key_row, &row.combined);
            if key_row.tip.is_empty() {
                key_row.tip = format!(
                    "{} rules; what a code this key catches resolves to",
                    row.rules.len()
                );
            }
            groups[at].1.push(key_row);
            for &index in &row.rules {
                let Some(rule) = self.buffer.rules().get(index) else {
                    continue;
                };
                let mut rule_row = FlatRow::new(TreeAt::Rule(index), 2);
                rule_row.cells[KEY_COLUMN] = cite(index, &rule.key, rule.section);
                self.fill_row(document, &mut rule_row, rule);
                groups[at].1.push(rule_row);
                if self.form_index == Some(index) {
                    keep = Some(TreeAt::Rule(index));
                }
            }
            if keep.is_none() && self.form_index.is_none() && row.key == self.explained {
                keep = Some(TreeAt::Key(row.key.clone()));
            }
        }
        self.tree = groups.into_iter().flat_map(|(_, rows)| rows).collect();
        self.tree_textures.clear();
        self.tree_current = keep;
    }

    fn tree_has(&self, at: &TreeAt) -> bool {
        self.tree.iter().any(|row| row.at == *at)
    }

    /// The search and the chip cleared, and the table rebuilt when either was set: rather than
    /// act on a row no one can see.
    fn clear_table_filter(&mut self, document: Option<&Document>) {
        self.table_filter.set_text("");
        self.table_filter.set_chip(0);
        if self.table_filter.take_changes().any() {
            self.rebuild_code_table(document);
        }
    }

    /// Shows the rule in the Code Table and loads it into the form.
    pub fn select_rule(&mut self, host: &mut dyn Host, index: usize) {
        let document = self.live_document(host.document());
        self.select_rule_in(document, index);
        self.flush(host);
    }

    pub(super) fn select_rule_in(&mut self, document: Option<&Document>, index: usize) {
        let Ok(rule) = self.buffer.at(index) else {
            return;
        };
        self.tab = 0;
        let at = TreeAt::Rule(index);
        if !self.tree_has(&at) {
            // Filtered out: clear the filter rather than edit a rule no one can see.
            self.clear_table_filter(document);
        }
        self.form_index = Some(index);
        if self.tree_has(&at) {
            self.tree_current = Some(at);
        }
        self.load_form(document, &rule, Some(index));
        self.test_code.clear();
        self.explain(document, &rule.key);
    }

    /// Shows the key's row and explains it; its first rule goes into the form.
    pub fn select_key(&mut self, host: &mut dyn Host, key: &str) {
        let document = self.live_document(host.document());
        let at = TreeAt::Key(String::from(key));
        if !self.tree_has(&at) {
            self.clear_table_filter(document);
        }
        if self.tree_has(&at) && self.tree_current.as_ref() != Some(&at) {
            self.tree_current = Some(at);
            self.tree_selection_changed(document);
        }
        self.flush(host);
    }

    pub(super) fn tree_selection_changed(&mut self, document: Option<&Document>) {
        match self.tree_current.clone() {
            // Nothing chosen, or a group heading.
            None | Some(TreeAt::Group(_)) => {}
            Some(TreeAt::Rule(index)) => {
                if let Ok(rule) = self.buffer.at(index) {
                    self.load_form(document, &rule, Some(index));
                    self.test_code.clear();
                    self.explain(document, &rule.key);
                }
            }
            Some(TreeAt::Key(key)) => {
                let first = self
                    .buffer
                    .rules()
                    .iter()
                    .position(|rule| rule.key == key);
                if let Some(index) = first
                    && let Ok(rule) = self.buffer.at(index)
                {
                    self.load_form(document, &rule, Some(index));
                }
                self.test_code.clear();
                self.explain(document, &key);
            }
        }
    }

    // ---- the detail pane -----------------------------------------------------------------------

    /// The code to test, set as a program sets the field: a code that is not empty is explained.
    pub(super) fn set_test_code(&mut self, document: Option<&Document>, code: &str) {
        if self.test_code != code {
            self.test_code = String::from(code);
            if !code.is_empty() {
                self.explain(document, code);
            }
        }
    }

    /// A double-click on row `row` of the explanation: the rule that set the field is opened in
    /// the form - at the next frame, since selecting the rule explains it again, which rebuilds
    /// this very list.
    pub fn activate_explained_field(&mut self, row: usize) {
        if let Some(field) = self.explain_fields.get(row) {
            self.pending = Some(Jump::Rule(field.rule));
        }
    }

    /// The explanation's rows: field, value, set by, decided against.
    pub fn explained_fields(&self) -> Vec<Vec<String>> {
        self.explain_fields
            .iter()
            .map(|field| field.cells.clone())
            .collect()
    }

    pub(super) fn explain(&mut self, document: Option<&Document>, code: &str) {
        self.explained = String::from(code);
        self.explain_fields.clear();
        let Some(document) = document else {
            self.explain_heading = String::from("The drawing is closed.");
            return;
        };
        let definitions = move |name: &str| document.definition_for(name);
        let definition_of: &DefinitionLookup<'_> = &definitions;
        let colours = colour_lookup(document);
        let colour_lookup_of: &ColourLookup<'_> = &colours;
        let explanation = explain_code(
            &self.buffer,
            code,
            Some(definition_of),
            Some(colour_lookup_of),
        );

        let how = match explanation.kind {
            SurveyMatchKind::Exact => format!("the exact key {}", explanation.resolved.key),
            SurveyMatchKind::Prefix => format!("the prefix key {}", explanation.resolved.key),
            SurveyMatchKind::FallbackOnly => String::from("nothing but the fallback key *"),
            SurveyMatchKind::None => String::from("no rule at all"),
        };
        self.explain_heading = format!(
            "<b>{}</b>: {} - met by {how}, {} rules in all.",
            html_escaped(code),
            match_class(explanation.kind, explanation.matched),
            explanation.rules.len()
        );

        for field in &explanation.fields {
            let against: Vec<String> = field
                .overruled
                .iter()
                .map(|lost| {
                    format!(
                        "{}: {}",
                        cite(lost.rule, &lost.key, lost.section),
                        lost.value
                    )
                })
                .collect();
            self.explain_fields.push(ExplainRow {
                cells: vec![
                    field.field.clone(),
                    field.value.clone(),
                    cite(field.rule, &field.key, field.section),
                    against.join("; "),
                ],
                rule: field.rule,
            });
        }
        for attribute in &explanation.attributes {
            let mut value = format!(
                "{} {} = {}",
                attribute.attribute.r#type, attribute.attribute.name, attribute.attribute.value
            );
            if attribute.deferred {
                value.push_str(" (names another attribute: left for the survey data)");
            }
            self.explain_fields.push(ExplainRow {
                cells: vec![
                    format!("attribute ({})", attribute.scope),
                    value,
                    cite(attribute.rule, &attribute.key, attribute.section),
                    String::new(),
                ],
                rule: attribute.rule,
            });
        }

        let mut definitions_said = Vec::new();
        let linestyle = &explanation.linestyle;
        if !linestyle.name.is_empty() {
            let state = if linestyle.plain {
                "the plain continuous line: needs no definition"
            } else if linestyle.defined && linestyle.vertex_mode {
                "an `at vertices` symbol, not a linestyle: drawn as a plain line"
            } else if linestyle.defined {
                "defined in the loaded library"
            } else {
                "NOT DEFINED: drawn as a plain line until a library defines it"
            };
            definitions_said.push(format!("Linestyle {}: {state}", linestyle.name));
        }
        let symbol = &explanation.symbol;
        if !symbol.name.is_empty() {
            let state = if symbol.defined {
                if symbol.vertex_mode {
                    "defined, at vertices"
                } else {
                    "defined"
                }
            } else if symbol.built_in {
                "drawn by Katana itself: no library needed"
            } else {
                "NOT DEFINED: drawn as a mark until a library defines it"
            };
            definitions_said.push(format!("Symbol {}: {state}", symbol.name));
        }
        if !explanation.colour.name.is_empty() {
            definitions_said.push(match explanation.colour.rgb {
                Some(rgb) => format!(
                    "Colour {}: #{:02X}{:02X}{:02X}",
                    explanation.colour.name, rgb.r, rgb.g, rgb.b
                ),
                None => format!(
                    "Colour {}: no RGB known - applying the code leaves the entity's colour as \
                     it is",
                    explanation.colour.name
                ),
            });
        }
        self.explain_definitions = definitions_said.join("\n");

        let misses: Vec<String> = explanation
            .near_misses
            .iter()
            .map(|miss| format!("{} ({})", miss.key, to_string(miss.kind)))
            .collect();
        self.explain_near_misses = if misses.is_empty() {
            String::new()
        } else {
            format!(
                "Would have matched but for letter case or blanks - matching is exact: {}",
                misses.join(", ")
            )
        };

        if !linestyle.name.is_empty() && !linestyle.plain {
            self.linestyle_preview.set_linestyle(&linestyle.name);
        } else {
            self.linestyle_preview.clear();
        }
        match &explanation.resolved.symbol {
            Some(resolved) if !symbol.name.is_empty() => {
                self.symbol_preview.set_symbol(&symbol.name, resolved.size);
            }
            _ => self.symbol_preview.clear(),
        }
    }

    // ---- the frame -----------------------------------------------------------------------------

    /// Draws the manager for this frame, when it is shown - the drawing's changes since the last
    /// frame first, as the C++'s watcher delivered them at the event loop, then what a
    /// double-click of the last frame asked for - and carries out what was pressed. True while it
    /// is shown.
    pub fn ui(&mut self, ctx: &Context, host: &mut dyn Host) -> bool {
        if let Some(changes) = self.watcher.deliver(host.document()) {
            self.document_changed(host.document(), changes);
        }
        if let Some(jump) = self.pending.take() {
            let document = self.live_document(host.document());
            match jump {
                Jump::Rule(index) => self.select_rule_in(document, index),
                Jump::TestCode(code) => {
                    self.tab = 0;
                    self.set_test_code(document, &code);
                }
            }
        }
        if !self.open {
            self.flush(host);
            return false;
        }
        if std::mem::take(&mut self.just_shown) {
            let document = self.live_document(host.document());
            self.reload_scopes(document);
            self.show_lines();
        }
        let mut frame =
            std::mem::replace(&mut self.frame, DialogFrame::new(DIALOG, "Survey Code Manager"));
        let closed = show_dialog(&mut frame, ctx, self, host, Self::body, Self::footer);
        self.frame = frame;
        self.update_dirty();
        for event in std::mem::take(&mut self.events) {
            self.react(host, event);
        }
        if closed {
            self.reject(host);
        }
        self.flush(host);
        self.open
    }

    fn react(&mut self, host: &mut dyn Host, event: Event) {
        match event {
            Event::Import => self.import_pressed(host),
            Event::Export => self.export_pressed(host),
            Event::ExportCodeList => self.export_code_list_pressed(host),
            Event::Apply => {
                if let Err(error) = self.apply(host) {
                    self.log(error.describe(), true);
                }
            }
            Event::Revert => self.revert(host),
            Event::Close => self.reject(host),
            Event::CloseApply => {
                self.asking_close = false;
                match self.apply(host) {
                    Ok(()) => self.open = false,
                    Err(error) => self.log(error.describe(), true),
                }
            }
            Event::CloseDiscard => {
                self.asking_close = false;
                self.revert(host);
                self.open = false;
            }
            Event::CloseCancel => self.asking_close = false,
            Event::TableFilter => {
                let document = self.live_document(host.document());
                self.rebuild_code_table(document);
            }
            Event::Tree(row) => {
                let chosen = self.tree.get(row).map(|row| row.at.clone());
                if chosen.is_some() && chosen != self.tree_current {
                    self.tree_current = chosen;
                    let document = self.live_document(host.document());
                    self.tree_selection_changed(document);
                }
            }
            Event::TestCode => {
                if !self.test_code.is_empty() {
                    let typed = self.test_code.clone();
                    let document = self.live_document(host.document());
                    self.explain(document, &typed);
                }
            }
            Event::ExplainActivated(row) => self.activate_explained_field(row),
            other => self.react_further(host, other),
        }
        self.flush(host);
    }

    fn import_pressed(&mut self, host: &mut dyn Host) {
        if !self.interactive(host) {
            self.log(
                "Import Codes needs a file chosen in an interactive session",
                true,
            );
            return;
        }
        let filters: [(&str, &[&str]); 2] = [
            (customisation_file_filter(), &["customisation.json", "json"]),
            ("All files", &["*"]),
        ];
        let Some(path) = open_file_dialog("Import Survey Codes", &filters) else {
            return;
        };
        let mode = if self.import_replace {
            LoadMode::Replace
        } else {
            LoadMode::Merge
        };
        if let Err(error) = self.import_codes(host, &path, mode) {
            self.log(error.describe(), true);
        }
    }

    fn export_pressed(&mut self, host: &mut dyn Host) {
        if !self.interactive(host) {
            self.log(
                "Export Codes needs a file chosen in an interactive session",
                true,
            );
            return;
        }
        let filters: [(&str, &[&str]); 1] =
            [(customisation_file_filter(), &["customisation.json", "json"])];
        let Some(path) = save_file_dialog("Export Survey Codes", "", &filters) else {
            return;
        };
        if let Err(error) = self.export_codes(host, &path) {
            self.log(error.describe(), true);
        }
    }

    fn export_code_list_pressed(&mut self, host: &mut dyn Host) {
        if !self.interactive(host) {
            self.log(
                "Export Code List needs a file chosen in an interactive session",
                true,
            );
            return;
        }
        let filters: [(&str, &[&str]); 1] = [("CSV files", &["csv"])];
        let Some(path) = save_file_dialog("Export Code List", "", &filters) else {
            return;
        };
        if let Err(error) = self.export_code_list(host, &path) {
            self.log(error.describe(), true);
        }
    }

    /// The Code Table's filter bar, drawn by the dialog (see the module comment): the search field
    /// and the chip of the text section share the name filterText, and the field answers for both.
    fn table_filter_ui(&mut self, ui: &mut Ui) -> bool {
        let bar = ui.horizontal(|ui| {
            let name = "filterText";
            let taken = take(ui, name);
            let mut text = String::from(self.table_filter.text());
            let mut pressed = false;
            if let Some(taken) = &taken {
                match &taken.posted.act {
                    Act::Fill(filled) => {
                        if ui.is_visible() && !ui.is_enabled() {
                            settle(ui, taken, Err(fill_disabled(&taken.posted)));
                        } else {
                            text.clone_from(filled);
                            settle(ui, taken, Ok(()));
                        }
                    }
                    Act::Press => {
                        if !ui.is_visible() {
                            settle(ui, taken, Err(press_hidden(&taken.posted)));
                        } else if !ui.is_enabled() {
                            settle(ui, taken, Err(press_disabled(&taken.posted)));
                        } else {
                            pressed = true;
                            settle(ui, taken, Ok(()));
                        }
                    }
                    Act::ScrollTo => {}
                }
            }
            let placeholder = "Search keys, descriptions, layers, linestyles, symbols";
            let response = ui.add(
                egui::TextEdit::singleline(&mut text)
                    .id_salt("codeTableFilterText")
                    .hint_text(placeholder),
            );
            if let Some(taken) = &taken
                && taken.posted.act == Act::ScrollTo
            {
                response.scroll_to_me(None);
                settle(ui, taken, Ok(()));
            }
            let mut field = ControlRecord::new(name, ControlKind::Line);
            field.text = String::from(placeholder);
            field.value = text.clone();
            field.enabled = ui.is_enabled();
            field.visible = ui.is_visible();
            record(ui, &response, field);
            self.table_filter.set_text(&text);

            for (at, (named, _)) in SECTION_CHIPS.iter().enumerate() {
                let chip = FilterBar::chip_object_name(named);
                let checked = self.table_filter.chip() == Some(at);
                let label = self.table_chip_labels[at].as_str();
                let clicked = if chip == name {
                    // Drawn with no name of its own: its press comes through the field's.
                    ui.add(egui::Button::selectable(checked, label)).clicked() || pressed
                } else {
                    controls::toggle(ui, &chip, label, checked).clicked()
                };
                // Clicking the checked chip leaves it checked: there is always a filter.
                if clicked && !checked {
                    self.table_filter.set_chip(at);
                }
            }
        });
        custom(
            ui,
            &bar.response,
            ControlRecord::new("codeTableFilter", ControlKind::Other),
        );
        self.table_filter.take_changes().any()
    }

    /// The texture of the picture in column `column` of row `row`, made when it is first drawn.
    fn tree_texture(
        &mut self,
        ctx: &Context,
        row: usize,
        column: usize,
    ) -> Option<(egui::TextureId, egui::Vec2)> {
        let image = self.tree.get(row)?.images.get(column)?.as_ref()?;
        let size = egui::vec2(image.width() as f32, image.height() as f32);
        if let Some(found) = self.tree_textures.get(&(row, column)) {
            return Some((found.id(), size));
        }
        let handle = ctx.load_texture(
            format!("codeTree/{row}/{column}"),
            image.to_color_image(),
            egui::TextureOptions::LINEAR,
        );
        let id = handle.id();
        self.tree_textures.insert((row, column), handle);
        Some((id, size))
    }

    fn code_table_tab(&mut self, ui: &mut Ui) {
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                if self.table_filter_ui(ui) {
                    self.events.push(Event::TableFilter);
                }
                let pictures: Vec<Vec<Option<(egui::TextureId, egui::Vec2)>>> = (0..self
                    .tree
                    .len())
                    .map(|row| {
                        (0..COLUMN_COUNT)
                            .map(|column| self.tree_texture(ui.ctx(), row, column))
                            .collect()
                    })
                    .collect();
                let lines: Vec<TreeLine<'_>> = self
                    .tree
                    .iter()
                    .zip(&pictures)
                    .map(|(row, pictures)| TreeLine {
                        depth: row.depth,
                        cells: &row.cells,
                        marked: &row.marked,
                        bold: row.depth == 0,
                        pictures,
                        tip: &row.tip,
                    })
                    .collect();
                let current = self
                    .tree_current
                    .as_ref()
                    .and_then(|at| self.tree.iter().position(|row| row.at == *at));
                let shown = egui::ScrollArea::vertical()
                    .id_salt("codeTreeScroll")
                    .max_height(520.0)
                    .show(ui, |ui| {
                        tree_rows(ui, "codeTree", &CODE_TABLE_HEADERS, &lines, current)
                    })
                    .inner;
                if let Some(row) = shown.chosen {
                    self.events.push(Event::Tree(row));
                }
            });
            ui.vertical(|ui| {
                self.detail_pane(ui);
                ui.separator();
                self.rule_form_ui(ui);
            });
        });
    }

    fn detail_pane(&mut self, ui: &mut Ui) {
        let pane = ui.vertical(|ui| {
            if controls::line_with(
                ui,
                "testCode",
                &mut self.test_code,
                LineOptions {
                    placeholder: "Test a code: type a field code, e.g. WM01",
                    ..LineOptions::default()
                },
            )
            .changed()
            {
                self.events.push(Event::TestCode);
            }
            // The heading is kept as the C++ wrote it, its code in bold tags; it is drawn plain.
            let plain = self
                .explain_heading
                .replace("<b>", "")
                .replace("</b>", "");
            controls::label_as(
                ui,
                "explainHeading",
                &self.explain_heading,
                egui::Label::new(plain).wrap(),
            );
            let rows = self.explained_fields();
            let shown = table(
                ui,
                "explainFields",
                &["Field", "Value", "Set by", "Decided against"],
                &rows,
                None,
                &[],
                None,
            );
            if let Some(row) = shown.double_clicked {
                self.events.push(Event::ExplainActivated(row));
            }
            controls::label_as(
                ui,
                "explainDefinitions",
                &self.explain_definitions,
                egui::Label::new(self.explain_definitions.as_str()).wrap(),
            );
            let misses = |ui: &mut Ui, text: &str| {
                controls::label_as(ui, "explainNearMisses", text, egui::Label::new(text).wrap());
            };
            if self.explain_near_misses.is_empty() {
                hidden(ui, "explainNearMisses", |ui| {
                    misses(ui, &self.explain_near_misses);
                });
            } else {
                misses(ui, &self.explain_near_misses);
            }
            ui.horizontal(|ui| {
                self.linestyle_preview
                    .ui(ui, "linestylePreview", egui::vec2(200.0, 70.0));
                self.symbol_preview
                    .ui(ui, "symbolPreview", egui::vec2(90.0, 70.0));
            });
        });
        custom(
            ui,
            &pane.response,
            ControlRecord::new("codeDetail", ControlKind::Other),
        );
    }

    fn body(&mut self, ui: &mut Ui, _host: &mut dyn Host) {
        // Files: what comes into the buffer and what goes out of it.
        ui.horizontal(|ui| {
            if controls::button(ui, "importCodes", "Import Codes...")
                .on_hover_text(
                    "Read a Katana customisation file and merge its survey code rules into the \
                     rules being edited, for review before Apply",
                )
                .clicked()
            {
                self.events.push(Event::Import);
            }
            controls::check(
                ui,
                "importReplace",
                "Replace instead of merging",
                &mut self.import_replace,
            );
            if controls::button(ui, "exportCodes", "Export Codes...")
                .on_hover_text(
                    "Write the rules being edited to a Katana customisation file that holds the \
                     survey codes alone",
                )
                .clicked()
            {
                self.events.push(Event::Export);
            }
            if controls::button(ui, "exportCodeList", "Export Code List CSV...").clicked() {
                self.events.push(Event::ExportCodeList);
            }
        });

        let issues_title = self.tabs.issues_title.clone();
        let titles = [
            "Code Table",
            "Codes in Drawing",
            issues_title.as_str(),
            "Apply Codes",
            "Linework",
        ];
        controls::tabs(ui, "codeManagerTabs", &titles, &mut self.tab);
        ui.separator();
        // Every tab is drawn, the ones behind offstage: a driver fills and presses a control of a
        // tab that is not in front, as it did on a hidden Qt page.
        let pages: [fn(&mut Self, &mut Ui); 5] = [
            Self::code_table_tab,
            Self::census_tab,
            Self::issues_tab,
            Self::apply_tab,
            Self::linework_tab,
        ];
        let front = self.tab.min(pages.len() - 1);
        for (at, page) in pages.into_iter().enumerate() {
            let mut draw = |ui: &mut Ui| {
                let drawn = ui.vertical(|ui| page(self, ui));
                custom(
                    ui,
                    &drawn.response,
                    ControlRecord::new(TAB_PAGES[at], ControlKind::Other),
                );
            };
            if at == front {
                draw(ui);
            } else {
                offstage(ui, TAB_PAGES[at], draw);
            }
        }
    }

    fn footer(&mut self, ui: &mut Ui, _host: &mut dyn Host) {
        if self.asking_close {
            ui.horizontal(|ui| {
                ui.label("The survey code rules have edits that are not on the drawing yet.");
                if controls::button(ui, "unappliedApply", "Apply").clicked() {
                    self.events.push(Event::CloseApply);
                }
                if controls::button(ui, "unappliedDiscard", "Discard").clicked() {
                    self.events.push(Event::CloseDiscard);
                }
                if controls::button(ui, "unappliedCancel", "Cancel").clicked() {
                    self.events.push(Event::CloseCancel);
                }
            });
        }
        // The buffer's state, and the two ways out of it.
        let edited = self.dirty();
        ui.horizontal(|ui| {
            let mut said = RichText::new(self.dirty_text.as_str());
            if edited {
                said = said.color(UNDEFINED_NAME_COLOUR.to_egui());
            }
            controls::label_as(
                ui,
                "dirtyIndicator",
                &self.dirty_text,
                egui::Label::new(said),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if controls::button(ui, "closeManager", "Close").clicked() {
                    self.events.push(Event::Close);
                }
                ui.add_enabled_ui(edited, |ui| {
                    if controls::button(ui, "revertMap", "Revert")
                        .on_hover_text("Discard the edits and take the drawing's survey map again")
                        .clicked()
                    {
                        self.events.push(Event::Revert);
                    }
                    if controls::button(ui, "applyMap", "Apply")
                        .on_hover_text("Make the edited rules the drawing's survey map")
                        .clicked()
                    {
                        self.events.push(Event::Apply);
                    }
                });
            });
        });
    }
}
