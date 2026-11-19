//! The Leaders manager (Annotate > Leaders...; docs/annotation.md "In the window"): smart leaders
//! from the window, a thin front end over the LEADER and BALLOON verbs. The port of
//! annotation/leader_manager.
//!
//! THE DIALOG CHANGES NOTHING ITSELF. Every button builds the verb line a person would type -
//! LEADER SET, ATTACH, FREEZE, DETACH, PROP, FOR, ALIGN, BALLOON FOR and RENUMBER - shown as it
//! will run in leaderLine, and hands it to the window's one executor (host::Host::run), so the
//! edit is echoed in the command log, kept in the history, ONE undo step, and refused in the
//! verb's own words in the dialog's problem line. It still calls the read-only helpers of cad's
//! annotation::leader_edit (what a note would say, the place a leader goes on an entity, the value
//! rows) to preview; none of them edits. A note a command line cannot carry - one holding a double
//! quote - is refused in the problem line, naming the field, and nothing runs.
//!
//! ```text
//! Leader         the drawing's leaders in a list - every one, or those a scope and filter take
//!                ("Which leaders...": the shared Apply to / Only those that match controls) -
//!                and a form for the one chosen (it follows the drawing's selection): what its
//!                tip is on and how far along; its note as text, a template or a label style's,
//!                checked as it is typed and shown as it would read, or a PRESET's template (the
//!                level, the height above a surface, the cut or fill, the name, the code, the
//!                station - cad's leader_presets) with the surface or the alignment it reads; the
//!                values the entity its tip is on offers, the drawing's surfaces' and alignments'
//!                among them, a double-click putting {name} into the note; its arrow, callout,
//!                text style, sizes and landing direction; the exact coordinates of its tip, bends
//!                and note end; an attribute of that entity set through it; the tip attached to a
//!                selected entity; Freeze, Detach and Apply. With several leaders selected,
//!                Apply, Freeze and Detach change them all.
//! For Selection  a leader, or a numbered balloon, to each entity the Apply to / Only those that
//!                match controls take - the selected ones to begin with (LEADER FOR, BALLOON
//!                FOR) -, its note - typed, or a preset's - checked and shown as it would read for
//!                the first, with that entity's values and the look the leaders are made with.
//! Arrange        the notes of the leaders the Apply to controls take - the selected ones to
//!                begin with - in a column (LEADER ALIGN), and the balloons numbered again
//!                (BALLOON RENUMBER; all of them, or with "Only the balloons Apply to takes"
//!                those).
//! ```
//!
//! Apply sends only what the user CHANGED: the form remembers what it showed, and a field still
//! showing that is left out of the line, so a value the widgets cannot hold exactly (a height finer
//! than their 3 decimals, such as 2.3456 mm, a landing past their 1000 mm) is never rewritten, and
//! an untouched form applied is no step. A note made a label style's is the exception the core
//! forces: the style lends its look to whatever the edit leaves out, so the look the form shows is
//! sent unless it is the style's own. Numbers are written exactly (core's format_exact_real).
//!
//! It follows docs/desktop.md, "The rules a dialog or panel follows": non-modal and kept by the
//! annotation workbench, a command never run from the list's or the tables' own changes, the
//! document watched through a coalescing DocumentWatcher - delivered at the dialog's next frame,
//! as the C++'s at the event loop - and every control named for the tests and the headless
//! driver. It holds no copy of the drawing: the form is re-read from the leader whenever the
//! leader changes under it (an undo, a verb typed) or the drawing is replaced, and left alone -
//! unapplied edits and all - when neither happens.
//!
//! Object names, besides the form's (leaderNote, leaderApply ...):
//!
//! ```text
//! leaderListScopeToggle     "Which leaders...": pressed, the list shows the leaders the scope
//!                           takes
//! leaderListScope...        the shared controls (ScopeFilterWidget, prefix "leaderList":
//!                           leaderListScopeDrawing, leaderListFilterLayer ...)
//! leaderListScopeProblem    why the controls cannot be read
//! leaderForScope...         the same for the For Selection tab (prefix "leaderFor":
//!                           leaderForScopeSelection ...)
//! leaderForScopeReport      what the scope took: the verb's scope record
//! leaderArrangeScope...     the same for Arrange (prefix "leaderArrange")
//! balloonRenumberScoped     renumber only the balloons the scope takes
//! leaderLine                the line the last action ran (read-only)
//! ```
//!
//! What is the C++'s and not here. A value double-clicked in a values table goes in at the end of
//! the note, as it did until a person put the cursor elsewhere; a caret a person clicks into the
//! middle of the note is egui's own and is not followed. The text height and landing boxes show
//! their least value as a number, where the C++'s showed "the text style's" and "none" in its
//! place.

use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use egui::{Context, RichText, Ui};
use katana_cad::annotation::command_words::{anchor_word, command_word};
use katana_cad::annotation::dimension_build::AnchoredPoint;
use katana_cad::annotation::leader_edit::{
    BalloonOrder, LeaderChange, LeaderNote, LeaderNoteKind, LeaderVertexMove, LeadersForOptions,
    apply_leader_change, check_numbered_balloon, leader_place_on, leader_says,
    leader_target_values, leader_value_rows, next_balloon_number, note_of,
    parse_leader_landing_angle, tip_place_on,
};
use katana_cad::annotation::leader_presets::{
    PresetReads, leader_context, leader_preset_named, leader_presets, preset_info,
    recognise_preset, resolve_leader_preset,
};
use katana_cad::global_modify::match_entities;
use katana_cad::survey_coding::code_property_candidates;
use katana_cad::{Document, DocumentChange, ListenerHandle};
use katana_core::error::make_error;
use katana_core::text::{ReplyRecord, format_exact_real, parse_finite_double, read_reply_record};
use katana_core::{Error, ErrorCode, Result, Status};
use katana_entity::anchor::resolve_anchor;
use katana_entity::entity::to_string;
use katana_entity::leader_values::{anchor_values, is_smart};
use katana_entity::{
    AnchorPoint, AnchorRef, ArrowHead, CalloutShape, Entity, EntityId, Geometry, LabelStyle,
    LeaderGeometry, Model,
};
use katana_geometry::{Point2, Vec2};
use katana_math::cpp;
use katana_math::numerics::{DEG_TO_RAD, RAD_TO_DEG};

use super::dialog_support::{TableLook, show_dialog, spin_value, table_look};
use super::form_widgets::{button, check, combo, describe, edit, problem_line};
use crate::customisation::scope_filter_widget::WIDGET_WIDTH;
use crate::customisation::{
    DocumentChanges, DocumentWatcher, ScopeChoice, ScopeFilterView, ScopeFilterWidget,
};
use crate::dialog_layout as layout;
use crate::drive::controls::{self, Choice, SpinRange, TextOptions};
use crate::drive::registry::ControlKind;
use crate::format::qt_fixed;
use crate::host::{Host, VerbOutcome};
use crate::shell::DialogFrame;
use crate::shell::dialog;
use crate::survey::survey_dialogs::{hidden, offstage};
use crate::theme::palette;

const DIALOG: &str = "leaderManagerDialog";
const TITLE: &str = "Leaders";

const ARROWS: [ArrowHead; 5] = [
    ArrowHead::ClosedFilled,
    ArrowHead::Open,
    ArrowHead::Tick,
    ArrowHead::Dot,
    ArrowHead::None,
];
const CALLOUTS: [CalloutShape; 3] = [CalloutShape::None, CalloutShape::Box, CalloutShape::Circle];

/// The note kinds, in the note-kind boxes' order, and what each shows.
const NOTE_KINDS: [LeaderNoteKind; 3] = [
    LeaderNoteKind::Text,
    LeaderNoteKind::Template,
    LeaderNoteKind::LabelStyle,
];
const NOTE_KIND_TEXTS: [&str; 3] = [
    "Text",
    "Template (read off the entity)",
    "Label style's template",
];

/// A leader's sizes: millimetres on paper, to 3 decimals and up to a metre. Wider than any drawing
/// needs, so a size typed here is kept as typed; one the form cannot hold exactly is kept as it is
/// by not being sent (form_change).
const LARGEST_SIZE: f64 = 1000.0;
/// The unit is the rows' labels' ("Text height (mm)"), not a suffix: drawn by egui, a box's suffix
/// stays on after the words a box says for its least value ("Text style's mm"), and the three
/// boxes of a form say their unit one way.
const SIZE: SpinRange = SpinRange::new(0.0, LARGEST_SIZE, 0.5, 3);
/// A note's text height: 0 is the text style's own height, said in words - "0.000 mm" read as a
/// note of no height (the fourth look audit's 3).
const TEXT_HEIGHT: SpinRange = SIZE.with_special_text("Text style's");
/// A landing: 0 is none, said in words.
const LANDING: SpinRange = SIZE.with_special_text("None");
const SIZE_DECIMALS: usize = 3;
/// A number box that, greyed, says in `words` what stands in its place rather than a number it does
/// not use: Along with the tip on nothing, At x with the topmost note setting x. They showed
/// "0.000 %" and "0.000", zeros that read as values (the sixth look audit's H). Lit, it is the box
/// as ever; greyed, its record says the words and a fill of it is refused as disabled, as before.
fn spin_or_words(
    ui: &mut Ui,
    name: &str,
    value: &mut f64,
    range: SpinRange,
    words: &'static str,
) -> egui::Response {
    if ui.is_enabled() {
        controls::spin(ui, name, value, range)
    } else {
        // The box at its least value, which a special text says in words; the value kept.
        let mut shown = range.minimum;
        controls::spin(ui, name, &mut shown, range.with_special_text(words))
    }
}

const ALONG: SpinRange = SpinRange::new(0.0, 100.0, 5.0, 3).with_suffix(" %");
const FOR_ANGLE: SpinRange = SpinRange::new(-360.0, 360.0, 15.0, 1).with_suffix(" deg");
const FOR_LENGTH: SpinRange = SpinRange::new(0.5, LARGEST_SIZE, 1.0, 1).with_suffix(" mm");
/// Model units: wider than any drawing's coordinates, a national grid's included.
const ALIGN_X: SpinRange = SpinRange::new(-1.0e9, 1.0e9, 1.0, 3);
const ALIGN_SPACING: SpinRange = SpinRange::new(0.5, 500.0, 1.0, 1).with_suffix(" mm apart");
/// Where Arrange's stacking starts: a two-line note of the default 2.5 mm text runs 2.5 + 2.5 x
/// 5/3 = 6.7 mm from the top of its first line to its last baseline (entity's line pitch), so 8 mm
/// stacks such notes with a gap.
const STACK_SPACING: f64 = 8.0;
/// More balloons than any sheet holds, either way from 0.
const RENUMBER_RANGE: std::ops::RangeInclusive<i64> = -1_000_000..=1_000_000;

const ATTACH_TIP: &str = "Put the tip on the selected entity - inside a lot or circle the tip is \
                          in, else at the place of it nearest the tip: select the leader and the \
                          entity, then press (LEADER ATTACH)";
const FREEZE_TIP: &str = "Keep the words the note says now; the tip still follows (LEADER FREEZE)";
const DETACH_TIP: &str =
    "Let the tip go of the entity it is on, freezing a smart note first (LEADER DETACH)";

const NOT_APPLIED: &str =
    "the form has changes not applied: Apply them first, or show the leader again";

/// The views the scopes' "What a view shows" lists, shared by the three scopes.
pub type LeaderViews = Rc<dyn Fn() -> Vec<ScopeFilterView>>;

// ---- the lines the dialog hands to the executor -----------------------------------------------
//
// Pure functions of plain data, so a test checks the very line without a window. Each fails with
// InvalidArgument, naming the field, for a value a command line cannot carry: a double quote
// anywhere, a line break in a name.

/// A name or a value as one word, or a refusal that names the field: the command line has no way
/// to carry a double quote.
fn plain_word(field: &str, text: &str) -> Result<String> {
    command_word(text).map_err(|error| {
        make_error(
            error.code,
            format!("the {field}: {}", error.message),
            error.context,
        )
    })
}

/// A note (text or template) as one word of a LEADER line. The verbs read a backslash and an n as
/// a line break and two backslashes as one (core's unescape_typed), so a line break is written as
/// that pair and a backslash twice: a note holding a backslash before an n, which
/// annotation_text_word refuses, is carried whole. Only a double quote, which no word can hold, is
/// refused - with a sentence that names the field, as the dialog's other fields do.
fn note_word(field: &str, text: &str) -> Result<String> {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        if c == '\\' {
            escaped.push_str("\\\\");
        } else if c == '\n' {
            escaped.push_str("\\n");
        } else if c != '\r' {
            // A Windows line end is one break, not two.
            escaped.push(c);
        }
    }
    plain_word(field, &escaped)
}

fn point_word(point: &Point2) -> String {
    format!(
        "{},{}",
        format_exact_real(point.x),
        format_exact_real(point.y)
    )
}

/// The options of LEADER SET and LEADER FOR that make `change`, one leading blank each, in the
/// order text=|template=|labelstyle=, arrow=, callout=, style=, paper=, arrowsize=, landing=,
/// landingangle=, tip=, vertex= and at=: only what the change sets, the numbers exact. Empty for a
/// change that sets nothing.
pub fn leader_option_words(change: &LeaderChange) -> Result<String> {
    let mut words = String::new();
    let mut add = |key: &str, value: &str| {
        words.push(' ');
        words.push_str(key);
        words.push('=');
        words.push_str(value);
    };
    if let Some(note) = &change.note {
        let styled = note.kind == LeaderNoteKind::LabelStyle;
        let field = if styled { "label style" } else { "note" };
        let word = if styled {
            plain_word(field, &note.text)?
        } else {
            note_word(field, &note.text)?
        };
        let key = match note.kind {
            LeaderNoteKind::Text => "text",
            LeaderNoteKind::Template => "template",
            LeaderNoteKind::LabelStyle => "labelstyle",
        };
        add(key, &word);
    }
    if let Some(arrow) = change.arrow {
        add("arrow", to_string(arrow));
    }
    if let Some(callout) = change.callout {
        add("callout", to_string(callout));
    }
    if let Some(style) = &change.text_style {
        add("style", &plain_word("text style", style)?);
    }
    if let Some(height) = change.paper_height {
        add("paper", &format_exact_real(height));
    }
    if let Some(size) = change.arrow_size {
        add("arrowsize", &format_exact_real(size));
    }
    if let Some(landing) = change.landing {
        add("landing", &format_exact_real(landing));
    }
    if let Some(angle) = change.landing_angle {
        let value = match angle {
            Some(angle) => format_exact_real(angle * RAD_TO_DEG),
            None => String::from("auto"),
        };
        add("landingangle", &value);
    }
    if let Some(tip) = &change.tip {
        let value = if tip.r#ref.associated() {
            anchor_word(&tip.r#ref)
        } else {
            point_word(&tip.point)
        };
        add("tip", &value);
    }
    if let Some(hang) = &change.hang {
        add("at", &point_word(hang));
    }
    if let Some(vertex) = &change.vertex {
        add("vertex", &format!("{}", vertex.index));
        add("at", &point_word(&vertex.position));
    }
    Ok(words)
}

/// The ids of leaders as the words of a line.
fn id_words(ids: &[EntityId]) -> String {
    ids.iter().map(|id| format!(" {id}")).collect()
}

/// LEADER SET id [id...] <options>; an empty string for a change that sets nothing (no line to
/// run). InvalidArgument for no leader.
pub fn leader_set_line(ids: &[EntityId], change: &LeaderChange) -> Result<String> {
    if ids.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "no leader to change",
        ));
    }
    let words = leader_option_words(change)?;
    if words.is_empty() {
        return Ok(String::new());
    }
    Ok(format!("LEADER SET{}{words}", id_words(ids)))
}

/// LEADER FOR <options> angle=<deg> length=<mm> <scope> or the BALLOON FOR line: the options
/// first, since a WHERE in the scope words takes every key=value after it as a condition
/// (annotation_verbs). `scope` is ScopeFilterWidget::verb_words.
pub fn leaders_for_line(
    balloon: bool,
    scope: &str,
    change: &LeaderChange,
    angle_degrees: f64,
    length_millimetres: f64,
) -> Result<String> {
    if scope.trim().is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "say which entities to make leaders for",
        ));
    }
    if change.tip.is_some() || change.hang.is_some() || change.vertex.is_some() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "a leader made for an entity has its tip on it: tip and at do not apply",
        ));
    }
    let words = leader_option_words(change)?;
    // The options first: a WHERE in the scope's words takes every key=value after it.
    Ok(format!(
        "{}{words} angle={} length={} {}",
        if balloon { "BALLOON FOR" } else { "LEADER FOR" },
        format_exact_real(angle_degrees),
        format_exact_real(length_millimetres),
        scope.trim()
    ))
}

/// What the dialog is given by the window, besides the host it is drawn with (whose `run` is the
/// executor its lines run through): the open views the scopes' "What a view shows" lists.
#[derive(Default)]
pub struct LeaderManagerContext {
    /// None: the scopes offer one "Whole drawing view" hiding nothing.
    pub views: Option<LeaderViews>,
}

// ---- the helpers --------------------------------------------------------------------------------

/// A QComboBox whose items each carry a word of data: what it shows, what it means, and the item
/// chosen.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct DataBox {
    texts: Vec<String>,
    data: Vec<String>,
    index: Option<usize>,
}

impl DataBox {
    fn clear(&mut self) {
        self.texts.clear();
        self.data.clear();
        self.index = None;
    }

    /// QComboBox::addItem: the first item put in an empty box is chosen.
    fn add(&mut self, text: &str, data: &str) {
        self.texts.push(String::from(text));
        self.data.push(String::from(data));
        if self.index.is_none() && self.texts.len() == 1 {
            self.index = Some(0);
        }
    }

    fn current_data(&self) -> String {
        self.index
            .and_then(|at| self.data.get(at))
            .cloned()
            .unwrap_or_default()
    }

    fn find_data(&self, data: &str) -> Option<usize> {
        self.data.iter().position(|item| item == data)
    }

    /// QComboBox::setCurrentIndex(std::max(0, found)): the item found, else the first; none in an
    /// empty box.
    fn choose_or_first(&mut self, found: Option<usize>) {
        self.index = if self.texts.is_empty() {
            None
        } else {
            Some(found.unwrap_or(0))
        };
    }

    /// Draws it as the choice `name`; true when a person or a driver chose another item.
    fn ui(&mut self, ui: &mut Ui, name: &str) -> bool {
        let mut choice = Choice {
            index: self.index,
            ..Choice::default()
        };
        let changed = controls::choice(ui, name, &self.texts, &mut choice).changed();
        if changed {
            self.index = choice.index;
        }
        changed
    }
}

/// The look a label style lent a text style box and a height box, as labelstyle= lends a leader
/// it: what each showed before, and what the style put there, so that putting it back leaves
/// alone a field the user has changed since.
#[derive(Clone, Debug, Default, PartialEq)]
struct Lender {
    text_style_before: Option<String>,
    text_style_lent: Option<String>,
    paper_height_before: Option<f64>,
    paper_height_lent: Option<f64>,
}

impl Lender {
    /// Nothing lent to put back: the boxes' look is the form's own now.
    fn forget(&mut self) {
        *self = Self::default();
    }
}

/// A tab's note and its look: the note-kind box, the preset boxes, the note, the label style, the
/// text style and height, the look a style lent them, the values table and the check and preview.
#[derive(Clone, Debug, Default, PartialEq)]
struct NoteForm {
    kind: LeaderNoteKind,
    preset: DataBox,
    preset_reads: DataBox,
    reads_enabled: bool,
    note: String,
    label_styles: Vec<String>,
    label_style: Choice,
    text_style: DataBox,
    paper_height: f64,
    lender: Lender,
    values: Vec<Vec<String>>,
    values_current: Option<usize>,
    check: String,
    check_error: bool,
    preview: String,
}

impl NoteForm {
    /// The presets (cad's leader_presets): a first entry that is none - the note is what is
    /// typed - then each by its title, its word (preset=) as its data.
    fn new() -> Self {
        let mut form = Self::default();
        // "None", as every empty choice reads (drive::controls::NO_CHOICE), never "(none)".
        form.preset.add("None: the note as typed", "");
        for info in leader_presets() {
            form.preset.add(info.title, info.word);
        }
        form
    }

    fn label_style_name(&self) -> String {
        self.label_style.current_text(&self.label_styles)
    }

    /// The label styles, by name, keeping the choice where it can.
    fn fill_label_styles(&mut self, model: &Model) {
        let keep = self.label_style_name();
        self.label_styles = model.label_styles.names();
        let found = self.label_styles.iter().position(|name| *name == keep);
        self.label_style = Choice {
            index: if self.label_styles.is_empty() {
                None
            } else {
                Some(found.unwrap_or(0))
            },
            ..Choice::default()
        };
    }

    /// The text styles: `first` for none (data ""), then each by name, keeping the choice where it
    /// can.
    fn fill_text_styles(&mut self, model: &Model, first: &str) {
        let keep = self.text_style.current_data();
        self.text_style.clear();
        self.text_style.add(first, "");
        model.text_styles.for_each(|style| {
            self.text_style.add(&style.name, &style.name);
        });
        let found = self.text_style.find_data(&keep);
        self.text_style.choose_or_first(found);
    }

    /// What the preset shown reads, into the reads box: after a first entry leaving the choice to
    /// the preset (the first visible surface, the first alignment), the drawing's surfaces or
    /// alignments by name; empty and off for a preset that reads only its target. The choice is
    /// kept where it can be.
    fn fill_preset_reads(&mut self, presets_enabled: bool, document: &Document) {
        let keep = self.preset_reads.current_data();
        self.preset_reads.clear();
        let word = self.preset.current_data();
        let what = match leader_preset_named(&word) {
            Ok(preset) if !word.is_empty() => preset_info(preset).reads,
            _ => PresetReads::Target,
        };
        if what == PresetReads::Surface {
            self.preset_reads.add("(first visible surface)", "");
            for named in document.surfaces().all() {
                self.preset_reads.add(&named.name, &named.name);
            }
        } else if what == PresetReads::Alignment {
            self.preset_reads.add("(first alignment)", "");
            for name in document.model().alignments.names() {
                self.preset_reads.add(&name, &name);
            }
        }
        self.reads_enabled = what != PresetReads::Target && presets_enabled;
        let found = self.preset_reads.find_data(&keep);
        self.preset_reads.choose_or_first(found);
    }

    /// The preset boxes made to say what `note` - a template, or empty for a note that is none -
    /// is: the preset whose template it is, and what it reads, else none. Shown, never chosen, so
    /// nothing is written into the note; what the user types away from a preset's template is no
    /// preset.
    fn show_preset_of(&mut self, note: &str, presets_enabled: bool, document: &Document) {
        let recognised = recognise_preset(note);
        let found = recognised
            .as_ref()
            .and_then(|known| self.preset.find_data(preset_info(known.preset).word));
        self.preset.choose_or_first(found);
        self.fill_preset_reads(presets_enabled, document);
        if let Some(known) = recognised
            && !known.reads.is_empty()
        {
            let found = self.preset_reads.find_data(&known.reads);
            self.preset_reads.choose_or_first(found);
        }
    }

    /// `style`'s look into the boxes, after putting back another's, so one lending less (no text
    /// style, no height) leaves the look from before, not the last one's.
    fn lend(&mut self, style: Option<&LabelStyle>) {
        self.unlend();
        let Some(style) = style else {
            return;
        };
        if !style.text_style.is_empty() {
            self.lender.text_style_before = Some(self.text_style.current_data());
            let found = self.text_style.find_data(&style.text_style);
            self.text_style.choose_or_first(found);
            self.lender.text_style_lent = Some(self.text_style.current_data());
        }
        if style.paper_height > 0.0 {
            self.lender.paper_height_before = Some(self.paper_height);
            self.paper_height = spin_value(&SIZE, style.paper_height);
            self.lender.paper_height_lent = Some(self.paper_height);
        }
    }

    /// The look before, in each field still showing what was lent.
    fn unlend(&mut self) {
        if let (Some(lent), Some(before)) =
            (&self.lender.text_style_lent, &self.lender.text_style_before)
            && self.text_style.current_data() == *lent
        {
            let found = self.text_style.find_data(before);
            self.text_style.choose_or_first(found);
        }
        if let (Some(lent), Some(before)) = (
            self.lender.paper_height_lent,
            self.lender.paper_height_before,
        ) && self.paper_height == lent
        {
            self.paper_height = spin_value(&SIZE, before);
        }
        self.lender.forget();
    }

    /// The note the kind box, the editor and the style box say.
    fn note(&self) -> LeaderNote {
        LeaderNote {
            kind: self.kind,
            text: if self.kind == LeaderNoteKind::LabelStyle {
                self.label_style_name()
            } else {
                self.note.clone()
            },
        }
    }

    /// "{name}" put into the note at its cursor (its end).
    fn insert_field(&mut self, name: &str) {
        self.note.push('{');
        self.note.push_str(name);
        self.note.push('}');
    }

    fn set_values(&mut self, rows: &[katana_cad::annotation::leader_edit::LeaderValueRow]) {
        self.values = rows
            .iter()
            .map(|row| vec![row.name.clone(), row.text.clone()])
            .collect();
        self.values_current = None;
    }

    /// The preview as it reads: the check when it found a problem, else what the note says.
    fn previewed(&self) -> String {
        if self.check_error {
            self.check.clone()
        } else {
            self.preview.clone()
        }
    }
}

/// What the Leader tab's widgets say, compared field by field with what show_form put there to
/// find what the user changed.
#[derive(Clone, Debug, PartialEq)]
struct FormValues {
    note_kind: LeaderNoteKind,
    note_text: String,
    label_style: String,
    arrow: ArrowHead,
    callout: CalloutShape,
    text_style: String,
    paper_height: f64,
    arrow_size: f64,
    landing: f64,
    along: f64,
    landing_angle: String,
}

/// The first line of what a leader says, for the list.
fn list_text(id: EntityId, says: &str) -> String {
    let mut first = String::from(says.split('\n').next().unwrap_or(""));
    if says.contains('\n') {
        first.push_str(" ...");
    }
    format!(
        "{id}  {}",
        if first.is_empty() {
            "(no note)"
        } else {
            first.as_str()
        }
    )
}

/// Where on its entity a tip is, in words: "50% along segment 2", "inside it", "at its end". The
/// command line's terse "along 1 0.5" is entity's describe; this is for reading.
fn place_text(reference: &AnchorRef, polyline: bool) -> String {
    match reference.point {
        AnchorPoint::Position => String::from("at its position"),
        AnchorPoint::Start => String::from("at its start"),
        AnchorPoint::End => String::from("at its end"),
        AnchorPoint::Mid => String::from("at its middle"),
        AnchorPoint::Centre => String::from("at its centre"),
        AnchorPoint::Vertex => format!("at vertex {}", reference.index + 1),
        AnchorPoint::SegmentMid => format!("at the middle of segment {}", reference.index + 1),
        AnchorPoint::Along => format!(
            "{}% along{}",
            qt_fixed(cpp::clamp(reference.parameter, 0.0, 1.0) * 100.0, 1),
            if polyline {
                format!(" segment {}", reference.index + 1)
            } else {
                String::from(" it")
            }
        ),
        AnchorPoint::Inside => String::from("inside it"),
    }
}

/// The first record of a reply, one line each, that has the field `key`.
fn record_with(reply: &str, key: &str) -> Option<ReplyRecord> {
    reply
        .split('\n')
        .filter_map(read_reply_record)
        .find(|record| record.value(key).is_some())
}

/// The "scope=... matched=N" line of a reply: what the scope took.
fn scope_line_of(reply: &str) -> String {
    reply
        .split('\n')
        .find(|line| line.starts_with("scope="))
        .map(String::from)
        .unwrap_or_default()
}

/// How many LEADER FOR or BALLOON FOR made, by its record's skipped= field's record.
fn made_by(record: &ReplyRecord) -> String {
    String::from(
        record
            .value("leaders")
            .or_else(|| record.value("balloons"))
            .unwrap_or("0"),
    )
}

/// What LEADER FOR replied, as the For Selection tab reports it: "made=2 skipped=1: id=5 (a
/// Dimension) has no place ...".
fn for_report_of(reply: &str) -> String {
    let Some(record) = record_with(reply, "skipped") else {
        return String::from(reply);
    };
    let mut text = format!(
        "made={} skipped={}",
        made_by(&record),
        record.value("skipped").unwrap_or("0")
    );
    if let Some(reason) = record.value("firstskip") {
        text.push_str(": ");
        text.push_str(reason);
    }
    text
}

fn leader_of(entity: &Entity) -> Option<&LeaderGeometry> {
    match &entity.geometry {
        Geometry::Leader(leader) => Some(leader),
        _ => None,
    }
}

/// Every leader of the drawing, ascending.
fn all_leaders(document: &Document) -> Vec<EntityId> {
    let mut ids = Vec::new();
    document.model().entities.for_each(|entity| {
        if leader_of(entity).is_some() {
            ids.push(entity.id);
        }
    });
    ids
}

/// What the shared controls take (cad's match_entities, which is what the verbs' scope is resolved
/// by too), ascending; refused when the controls cannot be read.
fn matched(document: &Document, controls: &ScopeFilterWidget) -> Result<Vec<EntityId>> {
    let scope = controls.scope()?;
    let filter = controls.filter()?;
    match_entities(document, &scope, &filter)
}

/// The leaders the shared controls take (cad's match_entities, which is what the verbs' scope is
/// resolved by too), ascending. None, with `why` saying what in the controls cannot be read, when
/// they cannot be: no layer ticked, a colour that is not a colour.
fn leaders_taken(
    document: &Document,
    controls: &ScopeFilterWidget,
    why: Option<&mut String>,
) -> Vec<EntityId> {
    match matched(document, controls) {
        Ok(mut ids) => {
            ids.retain(|&id| {
                document
                    .model()
                    .entities
                    .find(id)
                    .and_then(leader_of)
                    .is_some()
            });
            ids
        }
        Err(error) => {
            if let Some(why) = why {
                *why = describe(&Err(error));
            }
            Vec::new()
        }
    }
}

/// cad's code property candidates as the entity layer takes them.
fn code_properties() -> Vec<String> {
    code_property_candidates()
        .iter()
        .map(|&code| String::from(code))
        .collect()
}

/// The For Selection tab's arrow and callout boxes: a first entry that leaves the choice to the
/// leader - an arrow that is a dot inside an outline, a balloon's circle - then each by name.
fn for_arrow_texts() -> Vec<String> {
    std::iter::once(String::from("Automatic (a dot inside an outline)"))
        .chain(ARROWS.iter().map(|&head| String::from(to_string(head))))
        .collect()
}

fn for_callout_texts() -> Vec<String> {
    std::iter::once(String::from("Automatic (a circle for a balloon)"))
        .chain(CALLOUTS.iter().map(|&shape| String::from(to_string(shape))))
        .collect()
}

/// Balloon Renumber's orders, as its box shows them.
const ORDERS: [BalloonOrder; 3] = [BalloonOrder::Id, BalloonOrder::X, BalloonOrder::Y];
const ORDER_TEXTS: [&str; 3] = ["As made", "Across, left to right", "Down, top first"];

/// The attribute's type: as typed (PROP SET guesses it), or stated.
const ATTRIBUTE_TYPES: [(&str, &str); 5] = [
    ("as typed", ""),
    ("text", "text"),
    ("integer", "integer"),
    ("real", "real"),
    ("boolean", "boolean"),
];

/// The tabs, in order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Tab {
    #[default]
    Leader = 0,
    ForSelection = 1,
    Arrange = 2,
}

/// What a button asked for, done after the frame drew it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pressed {
    Apply,
    Freeze,
    Detach,
    Attach,
    AttributeSet,
    AttributeRemove,
    Make,
    Align,
    Renumber,
    Close,
}

/// The manager's body width on every tab (the look of 2026-11-16): the list or the scope
/// beside the form - the widest tab's, For Selection's, whose Look group (its long choices,
/// "Automatic (a dot inside an outline)", beside "Note distance on paper") needs 946. At 900 that
/// tab ran 46 past it and the dialog was 926 wide on one tab and 972 on another.
const BODY_WIDTH: f32 = 950.0;

/// The values tables' header: a value's name, which a double-click puts in the note, and what it
/// reads at the tip. "Value | Here" named neither column.
const VALUE_HEADERS: [&str; 2] = ["Name", "Value here"];

/// What each tab drawn off the stage is kept apart by (`body`), in the tabs' order.
const TAB_SALTS: [&str; 3] = ["leaderTabLeader", "leaderTabFor", "leaderTabArrange"];

/// The width of the Leader tab's list of leaders: its rows span it, and the form has the rest of
/// the body - a leader's line ("#12 IL 10.50") and the scope toggle's words with room to spare.
const LIST_WIDTH: f32 = 240.0;

/// The Leaders manager (see the module comment).
pub struct LeaderManagerDialog {
    frame: DialogFrame,
    /// False for a dialog given no command line (the C++'s empty runner).
    command_line: bool,
    tab: usize,
    problem: String,
    last_line: String,
    line: String,

    shown: EntityId,
    /// The leader as the form last showed it: re-read only when the drawing's differs, so an
    /// unrelated edit leaves unapplied changes in the form.
    shown_geometry: Option<LeaderGeometry>,
    /// The widgets as show_form left them.
    loaded: Option<FormValues>,
    /// The note kind the form shows now, and the user's own note while a label style's template
    /// stands in for it.
    shown_kind: LeaderNoteKind,
    stash: Option<String>,

    // Leader
    list_scope_toggle: bool,
    list_scope: ScopeFilterWidget,
    list_scope_problem: String,
    list: Vec<(EntityId, String)>,
    list_current: Option<usize>,
    scope_text: String,
    scope_visible: bool,
    target: String,
    /// The target says what to do rather than what the tip is on (no leader, a plain leader):
    /// drawn muted, as a hint is.
    target_is_hint: bool,
    along: f64,
    along_enabled: bool,
    leader: NoteForm,
    fields_enabled: bool,
    note_read_only: bool,
    label_style_enabled: bool,
    arrow: ArrowHead,
    callout: CalloutShape,
    arrow_size: f64,
    landing: f64,
    landing_angles: Vec<String>,
    landing_angle: Choice,
    position_points: Vec<String>,
    position_point: Option<usize>,
    position_x: String,
    position_y: String,
    position_enabled: bool,
    position_hint: String,
    attribute_name: String,
    attribute_value: String,
    attribute_types: Vec<String>,
    attribute_type: Choice,
    attribute_enabled: bool,
    attach_enabled: bool,
    freeze_enabled: bool,
    detach_enabled: bool,
    apply_enabled: bool,
    attach_tip: String,
    freeze_tip: String,
    detach_tip: String,

    // For Selection
    for_scope: ScopeFilterWidget,
    for_scope_report: String,
    for_selection: String,
    /// Whether the scope takes anything, so Make for Selection has something to make for.
    for_takes_any: bool,
    for_note: NoteForm,
    for_arrow: Option<ArrowHead>,
    for_callout: Option<CalloutShape>,
    for_arrow_size: f64,
    for_landing: f64,
    for_angle: f64,
    for_length: f64,
    for_balloon: bool,
    for_report: String,

    // Arrange
    arrange_scope: ScopeFilterWidget,
    align_use_x: bool,
    align_x: f64,
    align_use_spacing: bool,
    align_spacing: f64,
    renumber_start: i64,
    renumber_order: BalloonOrder,
    renumber_scoped: bool,
    arrange_report: String,
    /// Whether Align Selected Leaders has leaders to align, and Renumber Balloons balloons to
    /// number (`update_arrange_ready`): each greys with nothing to act on.
    align_ready: bool,
    renumber_ready: bool,

    pressed: Option<Pressed>,
    /// Set by the document when the drawing is replaced (File > New, Open): the next refresh
    /// starts the form afresh rather than applying edits meant for the old drawing to a leader of
    /// the new one.
    replaced: Arc<AtomicBool>,
    // Kept, never read: the registration lasts as long as it does. Before the watcher, so it
    // goes first.
    _replaced_registration: ListenerHandle,
    watcher: DocumentWatcher,
}

impl LeaderManagerDialog {
    /// The manager over `document`; its lines run through the host it is drawn with.
    pub fn new(document: &Document, context: LeaderManagerContext) -> Self {
        let scope = |prefix: &str| {
            let mut widget = ScopeFilterWidget::new(prefix);
            if let Some(views) = &context.views {
                let views = Rc::clone(views);
                widget.views = Some(Box::new(move || views()));
            }
            widget
        };
        let mut list_scope = scope("leaderList");
        // Every leader to begin with: the whole drawing, no filter.
        list_scope.set_choice(ScopeChoice::Drawing);
        let replaced = Arc::new(AtomicBool::new(false));
        let heard = Arc::clone(&replaced);
        // Only a flag, never the form touched: the watcher's delivery does the work.
        let _replaced_registration = document.add_change_listener(move |change| {
            if change.has(DocumentChange::REPLACED) {
                heard.store(true, Ordering::Release);
            }
        });
        let leader_geometry = LeaderGeometry::default();
        let for_options = LeadersForOptions::default();
        let mut dialog = Self {
            frame: DialogFrame::new(DIALOG, TITLE),
            command_line: true,
            tab: Tab::Leader as usize,
            problem: String::new(),
            last_line: String::new(),
            line: String::new(),
            shown: 0,
            shown_geometry: None,
            loaded: None,
            shown_kind: LeaderNoteKind::Text,
            stash: None,
            list_scope_toggle: false,
            list_scope,
            list_scope_problem: String::new(),
            list: Vec::new(),
            list_current: None,
            scope_text: String::new(),
            scope_visible: false,
            target: String::new(),
            target_is_hint: true,
            along: 0.0,
            along_enabled: false,
            leader: NoteForm::new(),
            fields_enabled: false,
            note_read_only: false,
            label_style_enabled: false,
            arrow: leader_geometry.arrow,
            callout: CalloutShape::None,
            arrow_size: 0.0,
            landing: 0.0,
            landing_angles: ["Auto", "0", "90", "180", "270"]
                .iter()
                .map(|&angle| String::from(angle))
                .collect(),
            landing_angle: Choice::editable("Auto"),
            position_points: Vec::new(),
            position_point: None,
            position_x: String::new(),
            position_y: String::new(),
            position_enabled: false,
            position_hint: String::new(),
            attribute_name: String::new(),
            attribute_value: String::new(),
            attribute_types: ATTRIBUTE_TYPES
                .iter()
                .map(|&(text, _)| String::from(text))
                .collect(),
            attribute_type: Choice::at(0),
            attribute_enabled: false,
            attach_enabled: false,
            freeze_enabled: false,
            detach_enabled: false,
            apply_enabled: false,
            attach_tip: String::from(ATTACH_TIP),
            freeze_tip: String::from(FREEZE_TIP),
            detach_tip: String::from(DETACH_TIP),
            for_scope: scope("leaderFor"),
            for_scope_report: String::new(),
            for_selection: String::new(),
            for_takes_any: false,
            for_note: NoteForm::new(),
            for_arrow: None,
            for_callout: None,
            // A new leader's own sizes to start from; sent only when changed.
            for_arrow_size: spin_value(&SIZE, leader_geometry.arrow_size),
            for_landing: spin_value(&SIZE, leader_geometry.landing),
            // LEADER FOR's own defaults, from their one source.
            for_angle: spin_value(&FOR_ANGLE, for_options.angle * RAD_TO_DEG),
            for_length: spin_value(&FOR_LENGTH, for_options.length),
            for_balloon: false,
            for_report: String::new(),
            arrange_scope: scope("leaderArrange"),
            align_use_x: false,
            align_x: 0.0,
            align_use_spacing: false,
            align_spacing: spin_value(&ALIGN_SPACING, STACK_SPACING),
            renumber_start: 1,
            renumber_order: BalloonOrder::Id,
            renumber_scoped: false,
            arrange_report: String::new(),
            align_ready: false,
            renumber_ready: false,
            pressed: None,
            replaced,
            _replaced_registration,
            watcher: DocumentWatcher::new(document),
        };
        dialog.refresh(
            document,
            DocumentChanges {
                model: true,
                selection: true,
                ..DocumentChanges::default()
            },
        );
        dialog
    }

    /// The same manager given no command line to run on: every action says so.
    pub fn without_command_line(mut self) -> Self {
        self.command_line = false;
        self
    }

    /// Brings `tab` to the front, with the views open now in the scopes' lists: they come and go
    /// without the drawing changing.
    pub fn show_tab(&mut self, tab: Tab) {
        for scope in [
            &mut self.list_scope,
            &mut self.for_scope,
            &mut self.arrange_scope,
        ] {
            scope.reload_views();
        }
        self.tab = tab as usize;
    }

    /// The "Which leaders..." controls, the For Selection tab's and Arrange's: the shared
    /// ScopeFilterWidget, for what the drive cannot reach (a layer ticked in its list).
    pub fn list_scope_mut(&mut self) -> &mut ScopeFilterWidget {
        &mut self.list_scope
    }

    pub fn for_scope_mut(&mut self) -> &mut ScopeFilterWidget {
        &mut self.for_scope
    }

    pub fn arrange_scope_mut(&mut self) -> &mut ScopeFilterWidget {
        &mut self.arrange_scope
    }

    fn shown_leader<'d>(&self, document: &'d Document) -> Option<&'d LeaderGeometry> {
        if self.shown == 0 {
            return None;
        }
        document
            .model()
            .entities
            .find(self.shown)
            .and_then(leader_of)
    }

    fn selected_leaders(document: &Document) -> Vec<EntityId> {
        document
            .selection()
            .ids()
            .into_iter()
            .filter(|&id| {
                document
                    .model()
                    .entities
                    .find(id)
                    .and_then(leader_of)
                    .is_some()
            })
            .collect()
    }

    /// The leader the form is for; 0 for none.
    pub fn current_leader(&self) -> EntityId {
        self.shown
    }

    /// The leaders Apply, Freeze and Detach change: the selected ones when the shown leader is one
    /// of several selected, else the shown one.
    pub fn targets(&self, document: &Document) -> Vec<EntityId> {
        if self.shown == 0 {
            return Vec::new();
        }
        let selected = Self::selected_leaders(document);
        if selected.len() > 1 && selected.contains(&self.shown) {
            return selected;
        }
        vec![self.shown]
    }

    fn refresh(&mut self, document: &Document, delivered: DocumentChanges) {
        let mut changes = delivered;
        if self.replaced.swap(false, Ordering::AcqRel) {
            // A new drawing: nothing the form holds is about it.
            self.shown = 0;
            self.shown_geometry = None;
            self.loaded = None;
            changes.model = true;
            changes.selection = true;
        }
        let model = document.model();
        if changes.model {
            let chosen = self.leader.label_style_name();
            let for_chosen = self.for_note.label_style_name();
            // The layers the scopes offer, and the views, as the drawing has them.
            for scope in [
                &mut self.list_scope,
                &mut self.for_scope,
                &mut self.arrange_scope,
            ] {
                scope.reload(model);
            }
            self.fill_styles(model);
            self.fill_list(document);
            // A surface or an alignment made or gone is offered, or no longer.
            self.leader.fill_preset_reads(self.fields_enabled, document);
            self.for_note.fill_preset_reads(true, document);
            // The style chosen is gone, or the first one came: the box names another now, which
            // lends its look as choosing it does (show_form, below, puts the leader's own back
            // where the leader changed).
            if self.leader.label_style_name() != chosen {
                self.lend_style_look(document);
            }
            if self.for_note.label_style_name() != for_chosen {
                self.lend_for_style_look(document);
            }
        }
        if changes.selection {
            // The form follows the drawing's selection: the shown leader while it is one of
            // those selected, else the first selected.
            let selected = Self::selected_leaders(document);
            if let Some(&first) = selected.first()
                && !selected.contains(&self.shown)
            {
                self.show_leader(document, first);
            }
            // The list is of the selection while its scope says so.
            if !changes.model && self.list_scope_toggle {
                self.fill_list(document);
            }
        }
        if changes.model || changes.selection {
            match self.shown_leader(document).cloned() {
                None => {
                    // Gone (erased, undone): the first leader there is, or none.
                    self.shown = 0;
                    self.shown_geometry = None;
                    if let Some(&(first, _)) = self.list.first() {
                        self.show_leader(document, first);
                    } else {
                        self.show_form(document, &LeaderGeometry::default());
                    }
                }
                Some(current) if self.shown_geometry.as_ref() != Some(&current) => {
                    // Changed under the form: show it as it is now.
                    self.show_form(document, &current);
                }
                Some(_) => {
                    // The leader is as shown; what its entity and its style say may not be: the
                    // style's template, the values, the preview.
                    if self.leader.kind == LeaderNoteKind::LabelStyle {
                        self.update_note_kind(document);
                    }
                    self.show_values(document);
                    self.update_preview(document);
                }
            }
            self.update_for_preview(document);
        }
    }

    fn fill_styles(&mut self, model: &Model) {
        self.leader.fill_label_styles(model);
        self.for_note.fill_label_styles(model);
        // The default said as the Settings dialog says its own ("Katana Standard (default)"),
        // where "(default: Standard)" was a bracketed word of its own (the fifth look audit).
        self.leader.fill_text_styles(model, "Standard (default)");
        self.for_note.fill_text_styles(model, "Standard (default)");
    }

    /// The leaders the list shows: every one, or those the "Which leaders..." scope takes
    /// (ascending). The scope's problem, when the controls cannot be read, is in
    /// leaderListScopeProblem and the list is empty.
    pub fn listed_leaders(&self, document: &Document) -> Vec<EntityId> {
        if self.list_scope_toggle {
            leaders_taken(document, &self.list_scope, None)
        } else {
            all_leaders(document)
        }
    }

    fn fill_list(&mut self, document: &Document) {
        self.list.clear();
        self.list_current = None;
        let model = document.model();
        let context = leader_context(document);
        let mut why = String::new();
        let ids = if self.list_scope_toggle {
            leaders_taken(document, &self.list_scope, Some(&mut why))
        } else {
            all_leaders(document)
        };
        self.list_scope_problem = why;
        for id in ids {
            let Some(shape) = model.entities.find(id).and_then(leader_of) else {
                continue;
            };
            self.list
                .push((id, list_text(id, &leader_says(model, shape, &context))));
            if id == self.shown {
                self.list_current = Some(self.list.len() - 1);
            }
        }
    }

    /// Shows leader `id` in the form; false when it is not a leader.
    pub fn show_leader(&mut self, document: &Document, id: EntityId) -> bool {
        let Some(shape) = document.model().entities.find(id).and_then(leader_of) else {
            return false;
        };
        let shape = shape.clone();
        if self.shown != id {
            self.position_point = None;
        }
        self.shown = id;
        if let Some(row) = self.list.iter().position(|(listed, _)| *listed == id) {
            self.list_current = Some(row);
        }
        self.show_form(document, &shape);
        true
    }

    fn show_form(&mut self, document: &Document, shape: &LeaderGeometry) {
        let any = self.shown != 0;
        self.shown_geometry = any.then(|| shape.clone());
        self.stash = None;
        self.leader.lender.forget();
        self.fields_enabled = any;
        let note = note_of(shape);
        // The preset the note is, if it is one's, and what it reads.
        let template = if note.kind == LeaderNoteKind::Template {
            note.text.clone()
        } else {
            String::new()
        };
        self.leader.show_preset_of(&template, any, document);
        self.leader.kind = note.kind;
        if note.kind == LeaderNoteKind::LabelStyle {
            let found = self
                .leader
                .label_styles
                .iter()
                .position(|name| *name == note.text);
            self.leader.label_style.index = if self.leader.label_styles.is_empty() {
                None
            } else {
                Some(found.unwrap_or(0))
            };
        } else {
            self.leader.note.clone_from(&note.text);
        }
        self.arrow = shape.arrow;
        self.callout = shape.callout;
        let found = self.leader.text_style.find_data(&shape.style);
        self.leader.text_style.choose_or_first(found);
        self.leader.paper_height = spin_value(&SIZE, shape.paper_height);
        self.arrow_size = spin_value(&SIZE, shape.arrow_size);
        self.landing = spin_value(&SIZE, shape.landing);
        let angle = match shape.landing_angle {
            Some(angle) => format_exact_real(angle * RAD_TO_DEG),
            None => String::from("Auto"),
        };
        self.landing_angle = Choice::editable(&angle);
        let chosen_point = self.position_point;
        let count = shape.vertices.len();
        self.position_points = (0..count)
            .map(|i| {
                if i == 0 {
                    String::from("Arrow tip")
                } else if i + 1 == count {
                    String::from("Note end")
                } else {
                    format!("Bend {i}")
                }
            })
            .collect();
        self.position_point = match chosen_point {
            Some(point) if point < count => Some(point),
            _ => count.checked_sub(1),
        };
        let along = shape.tip_ref.point == AnchorPoint::Along;
        self.along = spin_value(
            &ALONG,
            if along {
                shape.tip_ref.parameter * 100.0
            } else {
                0.0
            },
        );
        self.shown_kind = note.kind;
        self.target_is_hint = !any || !shape.tip_ref.associated();
        self.target = if !any {
            String::from(
                "No leader: draw one with the Leader tool, or make them on the For Selection tab.",
            )
        } else if !shape.tip_ref.associated() {
            String::from(
                "Nothing - a plain leader. Select it and an entity, then Attach to Selected.",
            )
        } else {
            let on = document.model().entities.find(shape.tip_ref.entity);
            let polyline = on.is_some_and(|entity| {
                matches!(
                    entity.geometry,
                    Geometry::Polyline(_) | Geometry::CurvePolyline(_)
                )
            });
            format!(
                "{} {}, {}",
                on.map_or("a gone entity", |entity| to_string(entity.r#type())),
                shape.tip_ref.entity,
                place_text(&shape.tip_ref, polyline)
            )
        };
        self.update_note_kind(document);
        self.show_position(document);
        self.loaded = Some(self.read_form());
        self.show_values(document);
        self.update_preview(document);
    }

    fn read_form(&self) -> FormValues {
        FormValues {
            note_kind: self.leader.kind,
            note_text: self.leader.note.clone(),
            label_style: self.leader.label_style_name(),
            arrow: self.arrow,
            callout: self.callout,
            text_style: self.leader.text_style.current_data(),
            paper_height: self.leader.paper_height,
            arrow_size: self.arrow_size,
            landing: self.landing,
            along: self.along,
            landing_angle: self.landing_angle.current_text(&self.landing_angles),
        }
    }

    fn show_position(&mut self, document: &Document) {
        let current = self.shown_leader(document).cloned();
        let index = self.position_point;
        let vertex = current
            .as_ref()
            .zip(index)
            .and_then(|(shape, index)| shape.vertices.get(index).copied());
        let attached = vertex.is_some()
            && index == Some(0)
            && current
                .as_ref()
                .is_some_and(|shape| shape.tip_ref.associated());
        self.position_enabled = vertex.is_some() && !attached;
        self.position_x = vertex.map(|at| format_exact_real(at.x)).unwrap_or_default();
        self.position_y = vertex.map(|at| format_exact_real(at.y)).unwrap_or_default();
        self.position_hint = String::from(if attached {
            "The tip is attached. Use Along, Attach or Detach to change its position."
        } else {
            "Coordinates are in model units. Select one leader to move a point, then Apply. You \
             can also drag its point grips in the plan view."
        });
    }

    fn show_values(&mut self, document: &Document) {
        self.leader.set_values(&[]);
        let Some(current) = self.shown_leader(document) else {
            return;
        };
        if let Some(values) =
            leader_target_values(document.model(), current, &leader_context(document))
        {
            let rows = leader_value_rows(&values);
            self.leader.set_values(&rows);
        }
    }

    fn update_note_kind(&mut self, document: &Document) {
        let styled = self.leader.kind == LeaderNoteKind::LabelStyle;
        self.label_style_enabled = self.shown != 0 && styled;
        self.note_read_only = styled;
        if styled {
            // The style's template, to read: the note is the style's, live.
            let name = self.leader.label_style_name();
            self.leader.note = document
                .model()
                .label_styles
                .find(&name)
                .map(|style| style.text.clone())
                .unwrap_or_default();
        }
    }

    /// The note-kind box chosen by the user (its own change).
    fn note_kind_chosen(&mut self, document: &Document) {
        let now = self.leader.kind;
        let styled = LeaderNoteKind::LabelStyle;
        if self.shown_kind != styled && now == styled {
            // The user's words are kept while the style's template stands in.
            self.stash = Some(self.leader.note.clone());
            self.shown_kind = now;
            self.update_note_kind(document);
            self.lend_style_look(document);
        } else if self.shown_kind == styled && now != styled {
            // Back to the user's words and the look before the style lent its own, where the
            // user has not changed it since; a leader shown in a style starts from the style's
            // template.
            self.shown_kind = now;
            self.update_note_kind(document);
            if let Some(stash) = self.stash.take() {
                self.leader.note = stash;
            }
            self.leader.unlend();
        } else {
            self.shown_kind = now;
            self.update_note_kind(document);
        }
        self.update_preview(document);
    }

    fn lend_style_look(&mut self, document: &Document) {
        if self.shown == 0 || self.leader.kind != LeaderNoteKind::LabelStyle {
            return;
        }
        let name = self.leader.label_style_name();
        self.leader.lend(document.model().label_styles.find(&name));
    }

    fn lend_for_style_look(&mut self, document: &Document) {
        if self.for_note.kind != LeaderNoteKind::LabelStyle {
            self.for_note.unlend();
            return;
        }
        let name = self.for_note.label_style_name();
        self.for_note
            .lend(document.model().label_styles.find(&name));
    }

    /// What the user changed in the form: what Apply sends. Empty for an untouched form; an error
    /// for invalid coordinates or landing direction.
    pub fn form_change(&self, document: &Document) -> Result<LeaderChange> {
        let mut change = LeaderChange::default();
        let (Some(current), Some(was)) = (self.shown_leader(document), &self.loaded) else {
            return Ok(change);
        };
        let now = self.read_form();
        let kind = now.note_kind;
        let note_changed = now.note_kind != was.note_kind
            || if kind == LeaderNoteKind::LabelStyle {
                now.label_style != was.label_style
            } else {
                now.note_text != was.note_text
            };
        if note_changed {
            change.note = Some(LeaderNote {
                kind,
                text: if kind == LeaderNoteKind::LabelStyle {
                    now.label_style.clone()
                } else {
                    now.note_text.clone()
                },
            });
        }
        if now.arrow != was.arrow {
            change.arrow = Some(now.arrow);
        }
        if now.callout != was.callout {
            change.callout = Some(now.callout);
        }
        // A note made a label style's is lent the style's look in each part the edit leaves out
        // (apply_leader_change). So, for each part the style lends: left out while the form shows
        // the style's own - the core then lends it exactly, finer than the box holds, as
        // labelstyle= does - else sent, as the leader's own exact value where the form still shows
        // that, or as the form's.
        let lender = if change.note.is_some() && kind == LeaderNoteKind::LabelStyle {
            document.model().label_styles.find(&now.label_style)
        } else {
            None
        };
        match lender {
            Some(style) if !style.text_style.is_empty() => {
                if now.text_style != style.text_style {
                    change.text_style = Some(if now.text_style == was.text_style {
                        current.style.clone()
                    } else {
                        now.text_style.clone()
                    });
                }
            }
            _ => {
                if now.text_style != was.text_style {
                    change.text_style = Some(now.text_style.clone());
                }
            }
        }
        match lender {
            Some(style) if style.paper_height > 0.0 => {
                if now.paper_height != spin_value(&SIZE, style.paper_height) {
                    change.paper_height = Some(if now.paper_height == was.paper_height {
                        current.paper_height
                    } else {
                        now.paper_height
                    });
                }
            }
            _ => {
                if now.paper_height != was.paper_height {
                    change.paper_height = Some(now.paper_height);
                }
            }
        }
        if now.arrow_size != was.arrow_size {
            change.arrow_size = Some(now.arrow_size);
        }
        if now.landing != was.landing {
            change.landing = Some(now.landing);
        }
        if now.landing_angle != was.landing_angle {
            change.landing_angle = Some(parse_leader_landing_angle(now.landing_angle.trim())?);
        }
        let Some(index) = self
            .position_point
            .filter(|&index| index < current.vertices.len())
        else {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "choose a leader point to edit",
            ));
        };
        let east = parse_finite_double(self.position_x.trim());
        let north = parse_finite_double(self.position_y.trim());
        let (Some(east), Some(north)) = (east, north) else {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "leader point easting and northing must be finite numbers",
            ));
        };
        let edited = Point2::new(east, north);
        if edited != current.vertices[index] {
            change.vertex = Some(LeaderVertexMove {
                index,
                position: edited,
            });
        }
        if now.along != was.along && current.tip_ref.point == AnchorPoint::Along {
            let mut reference = current.tip_ref;
            reference.parameter = cpp::clamp(now.along / 100.0, 0.0, 1.0);
            let on = document.model().entities.find(reference.entity);
            // A circle's turn is [0, 1), as the verb's along reads it: a whole turn is where it
            // starts.
            let whole_turn = on.is_some_and(|entity| match &entity.geometry {
                Geometry::Circle(_) => true,
                Geometry::Ellipse(ellipse) => ellipse.is_full(),
                _ => false,
            });
            if whole_turn && reference.parameter == 1.0 {
                reference.parameter = 0.0;
            }
            if let Some(on) = on
                && let Some(point) = resolve_anchor(on, &reference)
            {
                change.tip = Some(AnchoredPoint {
                    point,
                    r#ref: reference,
                });
            }
        }
        Ok(change)
    }

    fn update_preview(&mut self, document: &Document) {
        let Some(current) = self.shown_leader(document) else {
            self.leader.check.clear();
            self.leader.preview.clear();
            self.update_buttons(document);
            return;
        };
        let mut would = current.clone();
        let context = leader_context(document);
        let status: Status = self.form_change(document).and_then(|change| {
            apply_leader_change(document.model(), &change, &mut would, self.shown, &context)
        });
        match status {
            Err(error) => {
                self.leader.check_error = true;
                self.leader.check = describe(&Err(error));
                self.leader.preview.clear();
            }
            Ok(()) => {
                self.leader.check_error = false;
                self.leader.check = String::from(if is_smart(&would) {
                    "Read off the entity whenever it is drawn."
                } else {
                    "Plain text: the note says exactly this."
                });
                self.leader.preview = leader_says(document.model(), &would, &context);
            }
        }
        self.update_buttons(document);
    }

    fn update_buttons(&mut self, document: &Document) {
        let any = self.shown != 0;
        let dirty = !matches!(self.form_change(document), Ok(change) if change.empty());
        let ids = self.targets(document);
        let mut any_smart = false;
        let mut any_attached = false;
        for &id in &ids {
            if let Some(shape) = document.model().entities.find(id).and_then(leader_of) {
                any_smart = any_smart || is_smart(shape);
                any_attached = any_attached || shape.tip_ref.associated();
            }
        }
        let current = self.shown_leader(document);
        let attached = current.is_some_and(|shape| shape.tip_ref.associated());
        let along = current.is_some_and(|shape| shape.tip_ref.point == AnchorPoint::Along);
        self.apply_enabled = any;
        self.freeze_enabled = any && !dirty && any_smart;
        self.detach_enabled = any && !dirty && any_attached;
        // Each acts on the leader as drawn, not the form: while the form has changes not applied,
        // their tips say so rather than leave the user guessing why they are greyed.
        let apply_first = if dirty {
            "\n\nThe form has changes not applied: Apply them first."
        } else {
            ""
        };
        self.freeze_tip = format!("{FREEZE_TIP}{apply_first}");
        self.detach_tip = format!("{DETACH_TIP}{apply_first}");
        self.attach_tip = format!("{ATTACH_TIP}{apply_first}");
        self.attribute_enabled = attached;
        let other = document
            .selection()
            .ids()
            .iter()
            .any(|&id| id != self.shown);
        self.attach_enabled = any && !dirty && other;
        // Enabled with several selected too, so an Along changed before the others were selected
        // can be set back (apply() refuses it for them).
        self.along_enabled = any && along;
        self.scope_text = if ids.len() > 1 {
            format!(
                "Apply, Freeze and Detach change the {} selected leaders; the form shows {}.",
                ids.len(),
                self.shown
            )
        } else {
            String::new()
        };
        self.scope_visible = ids.len() > 1;
    }

    /// What the note would say with the form as it is, or why it cannot be.
    pub fn preview(&self) -> String {
        self.leader.previewed()
    }

    /// What the last refused action said; empty after one that worked.
    pub fn problem(&self) -> &str {
        &self.problem
    }

    /// The line the last action ran (or tried to); empty before the first.
    pub fn last_line(&self) -> &str {
        &self.last_line
    }

    fn report(&mut self, status: &Status) {
        self.problem = describe(status);
    }

    /// Puts "{name}" into the note at the cursor (the end of the note, until the user puts the
    /// cursor elsewhere), making the note a template.
    pub fn insert_value(&mut self, document: &Document, name: &str) {
        if self.shown == 0 || name.is_empty() {
            return;
        }
        if self.leader.kind != LeaderNoteKind::Template {
            // A field makes the note a template. A label style's template is the starting point
            // (its look, lent, stays); a plain note's words stay.
            self.leader.kind = LeaderNoteKind::Template;
            self.shown_kind = LeaderNoteKind::Template;
            self.stash = None;
            self.leader.lender.forget();
            self.update_note_kind(document);
        }
        self.leader.insert_field(name);
        self.note_changed(document);
    }

    /// The Leader tab's note changed: the preset boxes say what it now is, and the preview.
    fn note_changed(&mut self, document: &Document) {
        let template = if self.leader.kind == LeaderNoteKind::Template {
            self.leader.note.clone()
        } else {
            String::new()
        };
        self.leader
            .show_preset_of(&template, self.fields_enabled, document);
        self.update_preview(document);
    }

    /// The For Selection tab's note changed.
    fn for_note_changed(&mut self, document: &Document) {
        let template = if self.for_note.kind == LeaderNoteKind::Template {
            self.for_note.note.clone()
        } else {
            String::new()
        };
        self.for_note.show_preset_of(&template, true, document);
        self.update_for_preview(document);
    }

    /// The For Selection tab's note-kind box changed.
    fn for_note_kind_chosen(&mut self, document: &Document) {
        // Its note box and its style box each serve one kind, and are drawn enabled by it.
        self.lend_for_style_look(document);
        self.update_for_preview(document);
    }

    /// The note made the preset chosen in the Leader tab's boxes, or the For Selection tab's;
    /// true when none is chosen.
    fn use_preset(&mut self, document: &Document, for_tab: bool) -> bool {
        let word = if for_tab {
            self.for_note.preset.current_data()
        } else {
            self.leader.preset.current_data()
        };
        if word.is_empty() {
            return true; // none chosen: the note is what is typed
        }
        if !for_tab && self.shown == 0 {
            self.report(&Err(Error::new(
                ErrorCode::InvalidState,
                "no leader is shown",
            )));
            return false;
        }
        let preset = match leader_preset_named(&word) {
            Ok(preset) => preset,
            Err(error) => {
                self.report(&Err(error));
                return false;
            }
        };
        // The name chosen, or none: the preset's own choice, said in the problem line's place as
        // the verb's reply says it.
        let form = if for_tab {
            &self.for_note
        } else {
            &self.leader
        };
        let name = form.preset_reads.current_data();
        let mut surface = None;
        let mut alignment = None;
        if !name.is_empty() {
            if preset_info(preset).reads == PresetReads::Surface {
                surface = Some(name);
            } else {
                alignment = Some(name);
            }
        }
        let resolved =
            match resolve_leader_preset(document, preset, surface.as_deref(), alignment.as_deref())
            {
                Ok(resolved) => resolved,
                Err(error) => {
                    self.report(&Err(error));
                    return false;
                }
            };
        // Through the box's own change, so a label style's look lent is put back as choosing
        // Template by hand puts it back.
        if for_tab {
            if self.for_note.kind != LeaderNoteKind::Template {
                self.for_note.kind = LeaderNoteKind::Template;
                self.for_note_kind_chosen(document);
            }
            self.for_note.note.clone_from(&resolved.template_text);
            self.update_for_preview(document);
        } else {
            if self.leader.kind != LeaderNoteKind::Template {
                self.leader.kind = LeaderNoteKind::Template;
                self.note_kind_chosen(document);
            }
            self.leader.note.clone_from(&resolved.template_text);
            self.update_preview(document);
        }
        self.problem = if resolved.chosen.is_empty() {
            String::new()
        } else {
            format!("Reads {}, the {}.", resolved.reads, resolved.chosen)
        };
        true
    }

    fn pick_preset(&mut self, document: &Document, for_tab: bool, word: &str, reads: &str) -> bool {
        let presets_enabled = for_tab || self.fields_enabled;
        let form = if for_tab {
            &mut self.for_note
        } else {
            &mut self.leader
        };
        let index = form.preset.find_data(&word.to_lowercase());
        let Some(index) = index.filter(|_| !word.is_empty()) else {
            let status: Status = leader_preset_named(word).map(|_| ());
            self.report(&status);
            return false;
        };
        form.preset.index = Some(index);
        form.fill_preset_reads(presets_enabled, document);
        let named = if reads.is_empty() {
            Some(0)
        } else {
            form.preset_reads.find_data(reads)
        };
        let Some(named) = named else {
            self.report(&Err(make_error(
                ErrorCode::NotFound,
                "the drawing has no surface or alignment of that name to read",
                reads,
            )));
            return false;
        };
        form.preset_reads.index = if form.preset_reads.texts.is_empty() {
            None
        } else {
            Some(named)
        };
        self.use_preset(document, for_tab)
    }

    /// Makes the note preset `word`'s template (LEADER preset=word), reading `reads` - a surface
    /// or an alignment of the drawing; empty for the first visible surface or the first alignment -
    /// as choosing them in the Preset boxes does. Apply then stores it. False, saying why in the
    /// problem line, for no such preset, no leader shown, or nothing the preset can read.
    pub fn choose_preset(&mut self, document: &Document, word: &str, reads: &str) -> bool {
        self.pick_preset(document, false, word, reads)
    }

    /// choose_preset for the leaders For Selection makes (LEADER FOR preset=).
    pub fn choose_for_preset(&mut self, document: &Document, word: &str, reads: &str) -> bool {
        self.pick_preset(document, true, word, reads)
    }

    /// Runs `line` through the executor and shows what came back: the problem line says why it was
    /// refused, empty when it was carried out. False when `line` could not be built (its error is
    /// shown), there is no executor, or the line was refused. `outcome`, when given, gets what the
    /// line logged.
    fn submit(
        &mut self,
        host: &mut dyn Host,
        line: Result<String>,
        outcome: Option<&mut VerbOutcome>,
    ) -> bool {
        let line = match line {
            Ok(line) => line,
            Err(error) => {
                self.report(&Err(error));
                return false;
            }
        };
        if line.is_empty() {
            // The line of an edit that changes nothing: no line, no step.
            self.problem.clear();
            return true;
        }
        self.last_line.clone_from(&line);
        self.line.clone_from(&line);
        if !self.command_line {
            self.problem = String::from("no command line to run on");
            return false;
        }
        let result = host.run(&line);
        self.problem = if result.ok {
            String::new()
        } else if result.error.is_empty() {
            String::from("The line was refused.")
        } else {
            result.error.clone()
        };
        let ok = result.ok;
        if let Some(outcome) = outcome {
            *outcome = result;
        }
        ok
    }

    fn no_leader_shown(&mut self) -> bool {
        self.report(&Err(Error::new(
            ErrorCode::InvalidState,
            "no leader is shown",
        )));
        false
    }

    /// Apply: LEADER SET with only what changed; an untouched form is no line.
    pub fn apply(&mut self, host: &mut dyn Host) -> bool {
        if self.shown == 0 {
            return self.no_leader_shown();
        }
        let ids = self.targets(host.document());
        let change = match self.form_change(host.document()) {
            Ok(change) => change,
            Err(error) => {
                self.report(&Err(error));
                return false;
            }
        };
        if change.tip.is_some() && ids.len() > 1 {
            // Along is where THIS leader's tip is on THIS leader's entity: sent to the others
            // too, it would move every tip onto that one place.
            let along = self.loaded.as_ref().map_or(0.0, |was| was.along);
            self.report(&Err(Error::new(
                ErrorCode::InvalidArgument,
                format!(
                    "Along moves one leader's tip: select only leader {} to move it, or set Along \
                     back to {} %",
                    self.shown,
                    qt_fixed(along, SIZE_DECIMALS)
                ),
            )));
            return false;
        }
        if !self.submit(host, leader_set_line(&ids, &change), None) {
            return false;
        }
        // The watcher coalesces notifications: an immediate undo must compare against the applied
        // form, not mistake it for edits never submitted.
        let shown = self.shown;
        self.show_leader(host.document(), shown);
        true
    }

    /// Freezing keeps what the leader says now - not the form's unapplied note, which a re-read
    /// form would then throw away.
    fn form_untouched(&mut self, document: &Document) -> bool {
        match self.form_change(document) {
            Ok(change) if change.empty() => true,
            _ => {
                self.report(&Err(Error::new(ErrorCode::InvalidState, NOT_APPLIED)));
                false
            }
        }
    }

    /// Freeze Note: LEADER FREEZE of the targets.
    pub fn freeze(&mut self, host: &mut dyn Host) -> bool {
        if self.shown == 0 {
            return self.no_leader_shown();
        }
        if !self.form_untouched(host.document()) {
            return false;
        }
        let line = format!("LEADER FREEZE{}", id_words(&self.targets(host.document())));
        self.submit(host, Ok(line), None)
    }

    /// Detach Tip: LEADER DETACH of the targets.
    pub fn detach(&mut self, host: &mut dyn Host) -> bool {
        if self.shown == 0 {
            return self.no_leader_shown();
        }
        if !self.form_untouched(host.document()) {
            return false;
        }
        let line = format!("LEADER DETACH{}", id_words(&self.targets(host.document())));
        self.submit(host, Ok(line), None)
    }

    /// The shown leader's tip put on the first selected entity that is not a leader (else any
    /// other selected entity), at the place of it nearest the tip.
    pub fn attach_to_selected(&mut self, host: &mut dyn Host) -> bool {
        let Some(current) = self.shown_leader(host.document()).cloned() else {
            return self.no_leader_shown();
        };
        if !self.form_untouched(host.document()) {
            return false;
        }
        // The first other selected entity that offers a place for the tip (a dimension or a
        // label does not), those that are not leaders first: inside an outline the tip is in,
        // else the place nearest it.
        let document = host.document();
        let mut first: Option<EntityId> = None;
        let mut place: Option<AnchoredPoint> = None;
        for leaders_too in [false, true] {
            for id in document.selection().ids() {
                let Some(entity) = document.model().entities.find(id) else {
                    continue;
                };
                if place.is_some() || id == self.shown || leaders_too != leader_of(entity).is_some()
                {
                    continue;
                }
                first.get_or_insert(id);
                place = tip_place_on(entity, current.vertices[0]);
            }
        }
        let Some(first) = first else {
            self.report(&Err(Error::new(
                ErrorCode::InvalidState,
                "select the entity to put the tip on, with the leader",
            )));
            return false;
        };
        let Some(place) = place else {
            self.report(&Err(make_error(
                ErrorCode::InvalidArgument,
                "nothing selected with the leader has a place on it to attach to",
                format!("id={first}"),
            )));
            return false;
        };
        // The place as a word the verb reads back exactly (#12.along1:0.25).
        let line = format!("LEADER ATTACH {} {}", self.shown, anchor_word(&place.r#ref));
        self.submit(host, Ok(line), None)
    }

    /// The attribute named in the form set on the entity the tip is on, to the form's value (of
    /// the stated type).
    pub fn set_attribute(&mut self, host: &mut dyn Host) -> bool {
        if self.shown == 0 {
            return self.no_leader_shown();
        }
        let key = match plain_word("attribute name", self.attribute_name.trim()) {
            Ok(key) => key,
            Err(error) => {
                self.report(&Err(error));
                return false;
            }
        };
        let value = match plain_word("attribute value", &self.attribute_value) {
            Ok(value) => value,
            Err(error) => {
                self.report(&Err(error));
                return false;
            }
        };
        // The value is read as PROP SET reads one - its type guessed - unless a type is chosen.
        let stated = self
            .attribute_type
            .index
            .and_then(|at| ATTRIBUTE_TYPES.get(at))
            .map_or("", |&(_, data)| data);
        let line = format!(
            "LEADER PROP {} SET {key} {value}{}",
            self.shown,
            if stated.is_empty() {
                String::new()
            } else {
                format!(" {stated}")
            }
        );
        self.submit(host, Ok(line), None)
    }

    /// The attribute named in the form removed from the entity the tip is on.
    pub fn remove_attribute(&mut self, host: &mut dyn Host) -> bool {
        if self.shown == 0 {
            return self.no_leader_shown();
        }
        let key = match plain_word("attribute name", self.attribute_name.trim()) {
            Ok(key) => key,
            Err(error) => {
                self.report(&Err(error));
                return false;
            }
        };
        let line = format!("LEADER PROP {} DELETE {key}", self.shown);
        self.submit(host, Ok(line), None)
    }

    /// The Leader tab's note replaced by `text`, as a person typing it does (setPlainText and the
    /// change it signals): unlike a fill, a backslash and an n stay two characters.
    pub fn type_note(&mut self, document: &Document, text: &str) {
        self.leader.note = String::from(text);
        self.note_changed(document);
    }

    /// Whether the Leader tab's note is shown to read only (QPlainTextEdit::isReadOnly): a label
    /// style's template stands in it.
    pub fn note_is_read_only(&self) -> bool {
        self.note_read_only
    }

    /// The tooltip of the button `name` (QWidget::toolTip): leaderAttachSelected, leaderFreeze or
    /// leaderDetach, which say when the form has changes not applied.
    pub fn button_tip(&self, name: &str) -> Option<&str> {
        match name {
            "leaderAttachSelected" => Some(&self.attach_tip),
            "leaderFreeze" => Some(&self.freeze_tip),
            "leaderDetach" => Some(&self.detach_tip),
            _ => None,
        }
    }

    // ---- For Selection ----------------------------------------------------------------------

    fn for_options(&self, document: &Document) -> LeadersForOptions {
        let mut options = LeadersForOptions {
            balloon: self.for_balloon,
            ..LeadersForOptions::default()
        };
        let note = self.for_note.note();
        // Nothing typed: a balloon is numbered, a leader is a bare arrow - of whatever kind the
        // box was left at.
        let nothing = note.kind != LeaderNoteKind::LabelStyle && note.text.is_empty();
        if !nothing {
            options.change.note = Some(note.clone());
        } else if !options.balloon {
            options.change.note = Some(LeaderNote {
                kind: LeaderNoteKind::Text,
                text: String::new(),
            });
        }
        options.change.arrow = self.for_arrow;
        options.change.callout = self.for_callout;
        // What the boxes show is what the leaders get. A new leader's own look is the default
        // text style and its height, so those are left out - but not where a label style lends
        // its look to what is left out: then only the style's own, as the boxes show it, is (and
        // is lent exactly).
        let lender = if note.kind == LeaderNoteKind::LabelStyle {
            document.model().label_styles.find(&note.text)
        } else {
            None
        };
        let text_style = self.for_note.text_style.current_data();
        match lender {
            Some(style) if !style.text_style.is_empty() => {
                if text_style != style.text_style {
                    options.change.text_style = Some(text_style);
                }
            }
            _ => {
                if !text_style.is_empty() {
                    options.change.text_style = Some(text_style);
                }
            }
        }
        let height = self.for_note.paper_height;
        match lender {
            Some(style) if style.paper_height > 0.0 => {
                if height != spin_value(&SIZE, style.paper_height) {
                    options.change.paper_height = Some(height);
                }
            }
            _ => {
                if height > 0.0 {
                    options.change.paper_height = Some(height);
                }
            }
        }
        let defaults = LeaderGeometry::default();
        if self.for_arrow_size != defaults.arrow_size {
            options.change.arrow_size = Some(self.for_arrow_size);
        }
        if self.for_landing != defaults.landing {
            options.change.landing = Some(self.for_landing);
        }
        options.angle = self.for_angle * DEG_TO_RAD;
        options.length = self.for_length;
        options
    }

    /// The entities the For Selection scope takes, or none when it cannot be read.
    fn for_targets(&self, document: &Document) -> Vec<EntityId> {
        matched(document, &self.for_scope).unwrap_or_default()
    }

    fn for_target<'d>(&self, document: &'d Document) -> Option<&'d Entity> {
        let angle = self.for_angle * DEG_TO_RAD;
        self.for_targets(document).into_iter().find_map(|id| {
            document
                .model()
                .entities
                .find(id)
                .filter(|entity| leader_place_on(entity, angle).is_some())
        })
    }

    fn for_check_problem(&mut self, text: String) {
        self.for_note.check_error = true;
        self.for_note.check = text;
        self.for_note.preview.clear();
    }

    fn update_for_preview(&mut self, document: &Document) {
        self.for_note.set_values(&[]);
        // Controls that cannot be read - no layer ticked, a colour that is none - say why,
        // instead of a scope that quietly takes nothing.
        let filter = match self.for_scope.scope().and_then(|_| self.for_scope.filter()) {
            Ok(filter) => filter,
            Err(error) => {
                // Lit, so a press says in the verb's words what does not read.
                self.for_takes_any = true;
                self.for_selection = String::from("Apply to cannot be read.");
                self.for_check_problem(describe(&Err(error)));
                return;
            }
        };
        // What the controls take: the selected entities to begin with, said as it always was;
        // any other scope or a filter is said by its count.
        let taken = self.for_targets(document);
        let plain_selection = self.for_scope.choice() == ScopeChoice::Selection && filter.empty();
        let count = taken.len();
        self.for_takes_any = count > 0;
        self.for_selection = if plain_selection {
            match count {
                0 => String::from("Nothing is selected: select the entities to put leaders on."),
                1 => String::from("1 entity selected"),
                _ => format!("{count} entities selected"),
            }
        } else {
            match count {
                0 => String::from("The scope takes no entity."),
                1 => String::from("1 entity taken by Apply to"),
                _ => format!("{count} entities taken by Apply to"),
            }
        };
        let Some(target) = self.for_target(document).cloned() else {
            self.for_note.check_error = false;
            // With nothing taken the line over the form says so (for_selection): said again
            // under the note, "Nothing is selected" stood twice in one form (the fourth look
            // audit's "For 10").
            self.for_note.check = if taken.is_empty() {
                String::new()
            } else {
                String::from(
                    "Nothing taken offers a place for a leader (a dimension, a label or a leader \
                     does not).",
                )
            };
            self.for_note.preview.clear();
            return;
        };
        let options = self.for_options(document);
        let Some(place) = leader_place_on(&target, options.angle) else {
            return;
        };
        let context = leader_context(document);
        let rows = leader_value_rows(&anchor_values(
            &target,
            &place.r#ref,
            place.point,
            &code_properties(),
            &context,
        ));
        self.for_note.set_values(&rows);
        let mut would = LeaderGeometry {
            vertices: vec![place.point, place.point + Vec2::new(1.0, 1.0)],
            tip_ref: place.r#ref,
            ..LeaderGeometry::default()
        };
        if options.balloon {
            would.callout = CalloutShape::Circle;
        }
        let mut change = options.change.clone();
        if let Err(error) = check_numbered_balloon(&change, options.balloon) {
            self.for_check_problem(describe(&Err(error)));
            return;
        }
        if options.balloon && change.note.is_none() {
            match next_balloon_number(document.model()) {
                Ok(number) => {
                    change.note = Some(LeaderNote {
                        kind: LeaderNoteKind::Text,
                        text: format!("{number}"),
                    });
                }
                Err(error) => {
                    self.for_check_problem(describe(&Err(error)));
                    return;
                }
            }
        }
        let first = format!("{} {}", to_string(target.r#type()), target.id);
        if let Err(error) = apply_leader_change(document.model(), &change, &mut would, 0, &context)
        {
            self.for_check_problem(format!("{first}: {}", describe(&Err(error))));
            return;
        }
        self.for_note.check_error = false;
        self.for_note.check = format!("For {first}, the first selected:");
        let says = leader_says(document.model(), &would, &context);
        self.for_note.preview = if says.is_empty() {
            String::from("(a bare leader)")
        } else {
            says
        };
    }

    /// What a leader for the first entity the scope takes would say, or why not.
    pub fn for_preview(&self) -> String {
        self.for_note.previewed()
    }

    /// Puts "{name}" into the For Selection tab's note, making it a template.
    pub fn insert_for_value(&mut self, document: &Document, name: &str) {
        if name.is_empty() {
            return;
        }
        if self.for_note.kind != LeaderNoteKind::Template {
            self.for_note.kind = LeaderNoteKind::Template;
            self.for_note_kind_chosen(document);
        }
        self.for_note.insert_field(name);
        self.for_note_changed(document);
    }

    /// Make for Selection: LEADER FOR (or BALLOON FOR) of what the scope takes, in one step.
    pub fn make_for_selection(&mut self, host: &mut dyn Host) -> bool {
        self.for_report.clear();
        self.for_scope_report.clear();
        let scope = match self.for_scope.verb_words() {
            Ok(scope) => scope,
            Err(error) => {
                self.report(&Err(error));
                return false;
            }
        };
        let options = self.for_options(host.document());
        // The options first, then the scope: a WHERE takes every key=value after it.
        let line = leaders_for_line(
            options.balloon,
            &scope,
            &options.change,
            self.for_angle,
            self.for_length,
        );
        let mut outcome = VerbOutcome::default();
        if !self.submit(host, line, Some(&mut outcome)) {
            return false;
        }
        self.for_scope_report = scope_line_of(&outcome.reply);
        self.for_report = for_report_of(&outcome.reply);
        // What was made is selected, so the Leader tab shows it and Arrange can line it up next.
        // Only when something was: after a scope that took nothing the last creations are some
        // other command's.
        let made = record_with(&outcome.reply, "skipped")
            .map_or_else(|| String::from("0"), |record| made_by(&record));
        if made != "0" {
            let document = host.document_mut();
            let created = document.last_created_entities();
            document.selection_mut().set(created);
        }
        true
    }

    /// What the last Make for Selection made, as LEADER FOR replies.
    pub fn for_report(&self) -> &str {
        &self.for_report
    }

    // ---- Arrange ------------------------------------------------------------------------------

    /// Align Selected Leaders: LEADER ALIGN of what the Arrange scope takes.
    pub fn align_selection(&mut self, host: &mut dyn Host) -> bool {
        let scope = match self.arrange_scope.verb_words() {
            Ok(scope) => scope,
            Err(error) => {
                self.report(&Err(error));
                return false;
            }
        };
        let mut line = String::from("LEADER ALIGN");
        if self.align_use_x {
            line.push_str(&format!(" x={}", format_exact_real(self.align_x)));
        }
        if self.align_use_spacing {
            // Paper millimetres, as the verb takes them (it applies the scale).
            line.push_str(&format!(
                " spacing={}",
                format_exact_real(self.align_spacing)
            ));
        }
        let mut outcome = VerbOutcome::default();
        if !self.submit(host, Ok(format!("{line} {scope}")), Some(&mut outcome)) {
            return false;
        }
        if let Some(record) = record_with(&outcome.reply, "leaders") {
            self.arrange_report = format!("aligned={}", record.value("leaders").unwrap_or("0"));
        }
        true
    }

    /// Renumber Balloons: BALLOON RENUMBER, of every numbered balloon or of those the Arrange
    /// scope takes.
    pub fn renumber_balloons(&mut self, host: &mut dyn Host) -> bool {
        let order = match self.renumber_order {
            BalloonOrder::X => "x",
            BalloonOrder::Y => "y",
            BalloonOrder::Id => "id",
        };
        let mut line = format!(
            "BALLOON RENUMBER start={} order={order}",
            self.renumber_start
        );
        if self.renumber_scoped {
            match self.arrange_scope.verb_words() {
                Ok(scope) => {
                    line.push(' ');
                    line.push_str(&scope);
                }
                Err(error) => {
                    self.report(&Err(error));
                    return false;
                }
            }
        }
        let mut outcome = VerbOutcome::default();
        if !self.submit(host, Ok(line), Some(&mut outcome)) {
            return false;
        }
        if let Some(record) = record_with(&outcome.reply, "renumbered") {
            self.arrange_report = format!(
                "balloons={} renumbered={}",
                record.value("balloons").unwrap_or("0"),
                record.value("renumbered").unwrap_or("0")
            );
        }
        true
    }

    pub fn arrange_report(&self) -> &str {
        &self.arrange_report
    }

    /// Whether Arrange's two acts have anything to act on: Align Selected Leaders the leaders the
    /// Arrange scope takes, Renumber Balloons the balloons - those the scope takes, or the
    /// drawing's. Lit with nothing selected, each invited a press that changed nothing (the fifth
    /// look audit). Controls that cannot be read keep them lit, so a press says why (as Make for
    /// Selection is kept).
    fn update_arrange_ready(&mut self, document: &Document) {
        let mut why = String::new();
        let taken = leaders_taken(document, &self.arrange_scope, Some(&mut why));
        let unreadable = !why.is_empty();
        self.align_ready = unreadable || !taken.is_empty();
        // A balloon is a leader in a circle; BALLOON RENUMBER numbers those that say a number.
        let balloon = |id: &EntityId| {
            document
                .model()
                .entities
                .find(*id)
                .and_then(leader_of)
                .is_some_and(|leader| leader.callout == CalloutShape::Circle)
        };
        self.renumber_ready = if self.renumber_scoped {
            unreadable || taken.iter().any(balloon)
        } else {
            all_leaders(document).iter().any(balloon)
        };
    }

    // ---- the frame ---------------------------------------------------------------------------

    /// Draws the dialog for this frame - the drawing's changes since the last delivered first, as
    /// the C++'s watcher delivered them at the event loop; false once its title bar's close button
    /// is clicked.
    pub fn ui(&mut self, ctx: &Context, host: &mut dyn Host) -> bool {
        if let Some(changes) = self.watcher.deliver(host.document()) {
            self.refresh(host.document(), changes);
        }
        self.update_arrange_ready(host.document());
        let mut frame = std::mem::replace(&mut self.frame, DialogFrame::new(DIALOG, TITLE));
        frame.set_body_width(BODY_WIDTH);
        // One size on every tab is the body's (`body`, every tab measured): a least height kept
        // from the frame as drawn caught the frame a tab was turned in, taller, and kept it - the
        // dialog grew from 806 to 844 and scrolled once For Selection was turned to (the fourth
        // look audit's 8).
        let mut closed = show_dialog(&mut frame, ctx, self, host, Self::body, Self::footer);
        self.frame = frame;
        match self.pressed.take() {
            Some(Pressed::Apply) => {
                self.apply(host);
            }
            Some(Pressed::Freeze) => {
                self.freeze(host);
            }
            Some(Pressed::Detach) => {
                self.detach(host);
            }
            Some(Pressed::Attach) => {
                self.attach_to_selected(host);
            }
            Some(Pressed::AttributeSet) => {
                self.set_attribute(host);
            }
            Some(Pressed::AttributeRemove) => {
                self.remove_attribute(host);
            }
            Some(Pressed::Make) => {
                self.make_for_selection(host);
            }
            Some(Pressed::Align) => {
                self.align_selection(host);
            }
            Some(Pressed::Renumber) => {
                self.renumber_balloons(host);
            }
            Some(Pressed::Close) => closed = true,
            None => {}
        }
        !closed
    }

    fn body(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        // One width on every tab, so the dialog does not change size as they are turned; the
        // fields fill their form rows (shell::dialog::labelled).
        ui.set_width(BODY_WIDTH);
        let titles = ["Leader", "For Selection", "Arrange"];
        let before = self.tab;
        controls::tabs(ui, "leaderTabs", &titles, &mut self.tab);
        if self.tab != before {
            for scope in [
                &mut self.list_scope,
                &mut self.for_scope,
                &mut self.arrange_scope,
            ] {
                scope.reload_views();
            }
        }
        // The strip's own rule parts it from the page (a separator under it drew a second).
        let document = host.document();
        // The pages as tall as the tallest tab, whichever is in front: every tab is drawn each
        // frame, those not in front off the stage (as Settings' pages are), so the tallest is
        // measured whichever the dialog opens on. Kept from the tab drawn alone, it was 806 high
        // opened on Leader, 695 opened on Arrange and 844 once For Selection was turned to (the
        // fourth look audit's 8).
        let tallest_id = ui.id().with("leaderTallestTab");
        let tallest = layout::last_height(ui, tallest_id).unwrap_or(0.0);
        let mut measured: f32 = 0.0;
        let front = self.tab.min(2);
        ui.vertical(|ui| {
            ui.set_min_height(tallest);
            for (at, salt) in TAB_SALTS.iter().enumerate() {
                let mut draw = |ui: &mut Ui| {
                    let page = ui.vertical(|ui| match at {
                        0 => self.leader_tab(ui, document),
                        1 => self.for_tab(ui, document),
                        _ => self.arrange_tab(ui),
                    });
                    measured = measured.max(page.response.rect.height());
                };
                if at == front {
                    draw(ui);
                } else {
                    offstage(ui, salt, draw);
                }
            }
        });
        layout::keep_height(ui, tallest_id, measured);
    }

    /// A note's check, a row of `form` under its field - none while it says nothing: an empty
    /// check's row stood blank, a control high, above "Says" (dialog_layout::quiet_label).
    fn checked_label(form: &mut dialog::Form<'_>, name: &str, text: &str, error: bool) {
        let colour = if error {
            palette::error().to_egui()
        } else {
            palette::text().to_egui()
        };
        if text.is_empty() {
            layout::quiet_label(form.ui(), name, text, colour);
        } else {
            form.under(|ui| layout::quiet_label(ui, name, text, colour));
        }
    }

    fn leader_tab(&mut self, ui: &mut Ui, document: &Document) {
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                // A column of its own width, which the list's rows span as a table's do; the form
                // has the rest of the body.
                ui.set_width(LIST_WIDTH);
                // A tick over the list, as every on-off choice of a dialog is: a toggle button of
                // a word or two read as a button floating over the list (the third look audit's
                // J).
                let mut scoped = self.list_scope_toggle;
                if controls::check(ui, "leaderListScopeToggle", "Only what a scope takes", &mut scoped)
                    .on_hover_text(
                        "List only the leaders a scope and a filter take: the selection, what a \
                         view shows, layers, an area or the whole drawing, and only those that \
                         match (as Global Modify chooses what to change)",
                    )
                    .changed()
                {
                    self.list_scope_toggle = scoped;
                    self.fill_list(document);
                }
                if self.list_scope_toggle && self.list_scope.ui(ui) {
                    self.fill_list(document);
                }
                layout::quiet_label(
                    ui,
                    "leaderListScopeProblem",
                    &self.list_scope_problem,
                    palette::error().to_egui(),
                );
                let rows: Vec<Vec<String>> = self
                    .list
                    .iter()
                    .map(|(_, text)| vec![text.clone()])
                    .collect();
                // The list in the table look, its header over it and an empty state under it: a
                // bare list of words under the toggle headed nothing when it was empty (the second
                // look audit's N13).
                let shown = table_look(
                    ui,
                    "leaderList",
                    &["Leader"],
                    &rows,
                    self.list_current,
                    &[],
                    None,
                    TableLook {
                        fill: true,
                        empty: "No leaders: draw one with the Leader tool",
                    },
                );
                if let Some(at) = shown.chosen
                    && self.list_current != Some(at)
                    && let Some(&(id, _)) = self.list.get(at)
                {
                    self.list_current = Some(at);
                    self.show_leader(document, id);
                }
            });
            ui.vertical(|ui| self.leader_form(ui, document));
        });
    }

    fn leader_form(&mut self, ui: &mut Ui, document: &Document) {
        let enabled = self.fields_enabled;
        let mut preview = false;
        // One form, its sections under headings rather than in boxes, so every field of the tab
        // starts at one edge and ends at one: a Position box, a Look box beside a table and an
        // Attribute box had four right edges (the second look audit's N13).
        dialog::section(ui, "Position");
        {
            named_row(ui, "leaderPositionPointLabel", "Point", |ui| {
                ui.add_enabled_ui(enabled, |ui| {
                    let mut point = Choice {
                        index: self.position_point,
                        ..Choice::default()
                    };
                    if controls::choice(
                        ui,
                        "leaderPositionPoint",
                        &self.position_points,
                        &mut point,
                    )
                    .changed()
                    {
                        self.position_point = point.index;
                        self.show_position(document);
                        preview = true;
                    }
                });
            });
            let position_enabled = self.position_enabled;
            {
                // Easting and northing on one row, as a point is read: the two fields share what
                // the northing's label leaves of it. The fields grey, never their labels, as on
                // every row of the tab (the third look audit's J).
                named_row(ui, "leaderPositionXLabel", "Easting", |ui| {
                    if !position_enabled {
                        ui.disable();
                    }
                    let northing = ui
                        .painter()
                        .layout_no_wrap(
                            String::from("Northing"),
                            egui::TextStyle::Body.resolve(ui.style()),
                            egui::Color32::PLACEHOLDER,
                        )
                        .size()
                        .x;
                    let width =
                        (ui.available_width() - northing - 2.0 * ui.spacing().item_spacing.x) / 2.0;
                    dialog::fixed_fields(ui, width);
                    // Centred on the row, as the label column's labels are.
                    ui.horizontal(|ui| {
                        preview |= edit(ui, "leaderPositionX", &mut self.position_x, "").changed();
                        named_label(ui, "leaderPositionYLabel", "Northing");
                        preview |= edit(ui, "leaderPositionY", &mut self.position_y, "").changed();
                    });
                });
            }
            // What the position rows do, under their fields in the secondary text.
            dialog::labelled(ui, "", |ui| {
                controls::label_as(
                    ui,
                    "leaderPositionHint",
                    &self.position_hint,
                    egui::Label::new(
                        RichText::new(self.position_hint.as_str())
                            .color(palette::text_muted().to_egui()),
                    )
                    .wrap(),
                );
            });
        }
        // The note.
        dialog::section(ui, "Note");
        // Hidden while there is no scope to say, taking no room: an invisible label kept its row
        // and left a band above "On".
        let scope_label = |ui: &mut Ui| {
            controls::label(ui, "leaderScope", &self.scope_text);
        };
        if self.scope_visible {
            scope_label(ui);
        } else {
            hidden(ui, "leaderScope", scope_label);
        }
        dialog::labelled(ui, "On", |ui| {
            // What it is on wraps in what Attach to Selected leaves of the row.
            let width = dialog::beside_a_button(ui, "Attach to Selected");
            ui.scope(|ui| {
                ui.set_max_width(width);
                // Muted while it says what to do (no leader, a plain leader), as a hint is: in the
                // text colour it read as a value (the fourth look audit's "For 10").
                let colour = if self.target_is_hint {
                    palette::text_muted()
                } else {
                    palette::text()
                };
                controls::label_as(
                    ui,
                    "leaderTarget",
                    &self.target,
                    egui::Label::new(RichText::new(self.target.as_str()).color(colour.to_egui()))
                        .wrap(),
                );
            });
            // At the row's right end, where the fields end: after the words, wherever they
            // stopped, it ended 11 points short of them (the third look audit's D).
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                ui.add_enabled_ui(self.attach_enabled, |ui| {
                    if button(ui, "leaderAttachSelected", "Attach to Selected")
                        .on_hover_text(self.attach_tip.as_str())
                        .clicked()
                    {
                        self.pressed = Some(Pressed::Attach);
                    }
                });
            });
        });
        let mut kind_chosen = false;
        let mut style_chosen = false;
        let mut note_typed = false;
        let mut preset_chosen = false;
        let mut reads_chosen = false;
        let mut double_clicked: Option<String> = None;
        dialog::form(ui, |form| {
            form.row("Along", |ui| {
                ui.add_enabled_ui(self.along_enabled, |ui| {
                    preview |= spin_or_words(
                        ui,
                        "leaderAlong",
                        &mut self.along,
                        ALONG,
                        "Tip on nothing",
                    )
                    .on_hover_text(
                        "How far along what it is on the tip is: of the line or the arc from its \
                         start, of the polyline segment, of a turn of the circle (LEADER SET id \
                         tip=#on.alongN:T)",
                    )
                    .changed();
                });
            });
            form.row("Note is", |ui| {
                ui.add_enabled_ui(enabled, |ui| {
                    kind_chosen = note_kind_box(ui, "leaderNoteKind", &mut self.leader.kind);
                });
            });
            form.row("Preset", |ui| {
                // The preset and what it reads share the row.
                let width = (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
                dialog::fixed_fields(ui, width);
                ui.add_enabled_ui(enabled, |ui| {
                    preset_chosen = self.leader.preset.ui(ui, "leaderPreset");
                });
                ui.add_enabled_ui(self.leader.reads_enabled, |ui| {
                    reads_chosen = self.leader.preset_reads.ui(ui, "leaderPresetReads");
                });
            });
            form.row("Note", |ui| {
                ui.add_enabled_ui(enabled, |ui| {
                    note_typed = controls::text_with(
                        ui,
                        "leaderNote",
                        &mut self.leader.note,
                        TextOptions {
                            read_only: self.note_read_only,
                            rows: 3,
                            monospace: false,
                        },
                    )
                    .changed();
                });
            });
            form.row("Label style", |ui| {
                ui.add_enabled_ui(self.label_style_enabled, |ui| {
                    style_chosen = controls::choice(
                        ui,
                        "leaderLabelStyle",
                        &self.leader.label_styles,
                        &mut self.leader.label_style,
                    )
                    .changed();
                });
            });
            Self::checked_label(
                form,
                "leaderTemplateCheck",
                &self.leader.check,
                self.leader.check_error,
            );
            // What the note says, while it says something: an empty "Says" stood alone over the
            // values' header, a row's height above it (the third look audit's J).
            says_row(form, "leaderPreview", &self.leader.preview);
            // The values the note may use, under the note in the field column and as wide as
            // the fields: beside the Look box it was a strip 100 points wide (the look audits'
            // T1). Its row is labelled, so the label column beside its header is not a void.
            form.row("Values", |ui| {
                ui.add_enabled_ui(enabled, |ui| {
                    let shown = table_look(
                        ui,
                        "leaderValues",
                        &VALUE_HEADERS,
                        &self.leader.values,
                        self.leader.values_current,
                        &[],
                        None,
                        TableLook {
                            fill: true,
                            empty: "No values: the tip is on nothing that offers any",
                        },
                    );
                    if let Some(at) = shown.chosen {
                        self.leader.values_current = Some(at);
                    }
                    if let Some(at) = shown.double_clicked {
                        double_clicked = self.leader.values.get(at).map(|row| row[0].clone());
                    }
                });
            });
        });
        dialog::section(ui, "Look");
        // The fields grey with no leader shown, never their labels, as the Note's rows above
        // them: the whole form greyed, labels and all, beside a Note whose labels stayed (the
        // third look audit's J).
        let fields = |ui: &mut Ui| {
            if !enabled {
                ui.disable();
            }
        };
        {
            dialog::form(ui, |form| {
                form.row("Arrow", |ui| {
                    fields(ui);
                    preview |= combo(ui, "leaderArrow", &ARROWS, &mut self.arrow).changed();
                });
                form.row("Note frame", |ui| {
                    fields(ui);
                    preview |= combo(ui, "leaderCallout", &CALLOUTS, &mut self.callout).changed();
                });
                form.row("Text style", |ui| {
                    fields(ui);
                    preview |= self.leader.text_style.ui(ui, "leaderTextStyle");
                });
                let boxes = [
                    (
                        "Text height (mm)",
                        "leaderPaperHeight",
                        &mut self.leader.paper_height,
                        TEXT_HEIGHT,
                    ),
                    ("Arrow size (mm)", "leaderArrowSize", &mut self.arrow_size, SIZE),
                    ("Landing (mm)", "leaderLanding", &mut self.landing, LANDING),
                ];
                for (title, name, value, range) in boxes {
                    form.row(title, |ui| {
                        fields(ui);
                        preview |= controls::spin(ui, name, value, range).changed();
                    });
                }
                named_row(
                    form.ui(),
                    "leaderLandingAngleLabel",
                    "Landing direction",
                    |ui| {
                        fields(ui);
                        preview |= controls::choice(
                            ui,
                            "leaderLandingAngle",
                            &self.landing_angles,
                            &mut self.landing_angle,
                        )
                        .on_hover_text(
                            "Landing direction in degrees: 0 right, 180 left; Auto follows the \
                             last leader segment.",
                        )
                        .changed();
                    },
                );
            });
        }
        dialog::section(ui, "Attribute of the entity it is on");
        dialog::labelled(ui, "Name, value, type", |ui| {
            // The name, the value and the type share what Set and Remove leave of the row.
            let width = (dialog::beside_buttons(ui, &["Set", "Remove"])
                - 2.0 * ui.spacing().item_spacing.x)
                / 3.0;
            dialog::fixed_fields(ui, width);
            ui.add_enabled_ui(enabled, |ui| {
                edit(
                    ui,
                    "leaderAttributeName",
                    &mut self.attribute_name,
                    "invert",
                );
                edit(
                    ui,
                    "leaderAttributeValue",
                    &mut self.attribute_value,
                    "10.25",
                );
                controls::choice(
                    ui,
                    "leaderAttributeType",
                    &self.attribute_types,
                    &mut self.attribute_type,
                );
            });
            // Set and Remove at the row's right end, where the fields above end: placed after
            // the fields, Remove ended 5 points short of them (the third look audit's D).
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                ui.add_enabled_ui(self.attribute_enabled, |ui| {
                    if button(ui, "leaderAttributeRemove", "Remove").clicked() {
                        self.pressed = Some(Pressed::AttributeRemove);
                    }
                    if button(ui, "leaderAttributeSet", "Set")
                        .on_hover_text(
                            "Set this attribute on the entity the tip is on (LEADER PROP)",
                        )
                        .clicked()
                    {
                        self.pressed = Some(Pressed::AttributeSet);
                    }
                });
            });
        });
        // Freeze Note, Detach Tip and Apply, the tab's actions, are in the footer (`footer`).

        // What the changes mean, as the C++'s signals said it.
        if kind_chosen {
            self.note_kind_chosen(document);
        }
        if style_chosen {
            self.update_note_kind(document);
            self.lend_style_look(document);
            self.update_preview(document);
        }
        if note_typed {
            self.note_changed(document);
        }
        // A preset chosen writes its template into the note, as typing it would; only Apply or
        // Make stores it.
        if preset_chosen {
            self.leader.fill_preset_reads(self.fields_enabled, document);
            self.use_preset(document, false);
        }
        if reads_chosen {
            self.use_preset(document, false);
        }
        let reacted = kind_chosen
            || style_chosen
            || note_typed
            || preset_chosen
            || reads_chosen
            || double_clicked.is_some()
            || preview;
        if let Some(name) = double_clicked {
            self.insert_value(document, &name);
        }
        if preview {
            self.update_preview(document);
        }
        // What a change meant is worked out after the frame drew the form, which shows it above
        // and beside the control that changed: the next frame shows it (as the For Selection
        // tab's does).
        if reacted {
            ui.ctx().request_repaint();
        }
    }

    fn for_tab(&mut self, ui: &mut Ui, document: &Document) {
        let mut preview = false;
        let mut kind_chosen = false;
        let mut style_chosen = false;
        let mut note_typed = false;
        let mut preset_chosen = false;
        let mut reads_chosen = false;
        let mut double_clicked: Option<String> = None;
        let styled = self.for_note.kind == LeaderNoteKind::LabelStyle;
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                // The form's column: what the scope and filter beside it leave of the body, so
                // that its fields, which fill their rows, stop short of the scope.
                let gap = ui.spacing().item_spacing.x;
                ui.set_width(BODY_WIDTH - WIDGET_WIDTH - gap);
                // What the scope takes, muted: a note over the form, not one of its values.
                controls::label_as(
                    ui,
                    "leaderForSelection",
                    &self.for_selection,
                    egui::Label::new(
                        RichText::new(self.for_selection.as_str())
                            .color(palette::text_muted().to_egui()),
                    )
                    .wrap(),
                );
                dialog::form(ui, |form| {
                    form.row("Note is", |ui| {
                        kind_chosen =
                            note_kind_box(ui, "leaderForNoteKind", &mut self.for_note.kind);
                    });
                    form.row("Preset", |ui| {
                        // The preset and what it reads share the row.
                        let width = (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
                        dialog::fixed_fields(ui, width);
                        preset_chosen = self.for_note.preset.ui(ui, "leaderForPreset");
                        ui.add_enabled_ui(self.for_note.reads_enabled, |ui| {
                            reads_chosen =
                                self.for_note.preset_reads.ui(ui, "leaderForPresetReads");
                        });
                    });
                    form.row("Note", |ui| {
                        ui.add_enabled_ui(!styled, |ui| {
                            note_typed = controls::text_with(
                                ui,
                                "leaderForNote",
                                &mut self.for_note.note,
                                TextOptions {
                                    rows: 3,
                                    ..TextOptions::default()
                                },
                            )
                            .changed();
                        });
                    });
                    form.row("Label style", |ui| {
                        ui.add_enabled_ui(styled, |ui| {
                            style_chosen = controls::choice(
                                ui,
                                "leaderForLabelStyle",
                                &self.for_note.label_styles,
                                &mut self.for_note.label_style,
                            )
                            .changed();
                        });
                    });
                    Self::checked_label(
                        form,
                        "leaderForCheck",
                        &self.for_note.check,
                        self.for_note.check_error,
                    );
                    says_row(form, "leaderForPreview", &self.for_note.preview);
                    // The values under the note, as wide as its fields (the Leader tab's).
                    form.row("Values", |ui| {
                        let shown = table_look(
                            ui,
                            "leaderForValues",
                            &VALUE_HEADERS,
                            &self.for_note.values,
                            self.for_note.values_current,
                            &[],
                            None,
                            TableLook {
                                fill: true,
                                empty: "No values: select an entity that offers some",
                            },
                        );
                        if let Some(at) = shown.chosen {
                            self.for_note.values_current = Some(at);
                        }
                        if let Some(at) = shown.double_clicked {
                            double_clicked = self.for_note.values.get(at).map(|row| row[0].clone());
                        }
                    });
                });
                dialog::section(ui, "Look");
                dialog::form(ui, |form| {
                    form.row("Arrow", |ui| {
                        preview |= optional_box(
                            ui,
                            "leaderForArrow",
                            &for_arrow_texts(),
                            &ARROWS,
                            &mut self.for_arrow,
                        );
                    });
                    form.row("Callout", |ui| {
                        preview |= optional_box(
                            ui,
                            "leaderForCallout",
                            &for_callout_texts(),
                            &CALLOUTS,
                            &mut self.for_callout,
                        );
                    });
                    form.row("Text style", |ui| {
                        preview |= self.for_note.text_style.ui(ui, "leaderForTextStyle");
                    });
                    let boxes = [
                        (
                            "Text height (mm)",
                            "leaderForPaperHeight",
                            &mut self.for_note.paper_height,
                            TEXT_HEIGHT,
                        ),
                        (
                            "Arrow size (mm)",
                            "leaderForArrowSize",
                            &mut self.for_arrow_size,
                            SIZE,
                        ),
                        ("Landing (mm)", "leaderForLanding", &mut self.for_landing, LANDING),
                        (
                            "Note direction",
                            "leaderForAngle",
                            &mut self.for_angle,
                            FOR_ANGLE,
                        ),
                        (
                            "Note distance on paper",
                            "leaderForLength",
                            &mut self.for_length,
                            FOR_LENGTH,
                        ),
                    ];
                    for (title, name, value, range) in boxes {
                        form.row(title, |ui| {
                            preview |= controls::spin(ui, name, value, range).changed();
                        });
                    }
                    form.under(|ui| {
                        preview |= check(
                            ui,
                            "leaderForBalloon",
                            "Balloons (numbered when no note)",
                            &mut self.for_balloon,
                        )
                        .changed();
                    });
                });
                layout::quiet_label(
                    ui,
                    "leaderForReport",
                    &self.for_report,
                    palette::text().to_egui(),
                );
            });
            ui.vertical(|ui| {
                if self.for_scope.ui(ui) {
                    preview = true;
                }
                layout::quiet_label(
                    ui,
                    "leaderForScopeReport",
                    &self.for_scope_report,
                    palette::text().to_egui(),
                );
            });
        });
        // Make for Selection, the tab's action, is the footer's primary (`footer`).

        if kind_chosen {
            self.for_note_kind_chosen(document);
        }
        if style_chosen {
            self.lend_for_style_look(document);
            self.update_for_preview(document);
        }
        if note_typed {
            self.for_note_changed(document);
        }
        if preset_chosen {
            self.for_note.fill_preset_reads(true, document);
            self.use_preset(document, true);
        }
        if reads_chosen {
            self.use_preset(document, true);
        }
        let reacted = kind_chosen
            || style_chosen
            || note_typed
            || preset_chosen
            || reads_chosen
            || double_clicked.is_some()
            || preview;
        if let Some(name) = double_clicked {
            self.insert_for_value(document, &name);
        }
        if preview {
            self.update_for_preview(document);
        }
        // What a change meant is worked out after the frame drew the tab, and the selection's
        // count, the check and the preview are drawn above the controls they follow: the next
        // frame shows them. A change that came as no input - a scope's setter, a layer ticked -
        // would otherwise wait for the next event.
        if reacted {
            ui.ctx().request_repaint();
        }
    }

    fn arrange_tab(&mut self, ui: &mut Ui) {
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                // What the scope and filter beside it leave of the body (as on For Selection).
                let gap = ui.spacing().item_spacing.x;
                ui.set_width(BODY_WIDTH - WIDGET_WIDTH - gap);
                // Sections under headings, as the other tabs' are; their two actions, which do
                // not go together, are the footer's (`footer`), as every tab's actions are. One
                // column: a heading first in a column of its own has no gap above it, and
                // "Number the balloons again" stood 4 points under Stacked (the fourth look
                // audit's 7).
                {
                    dialog::section(ui, "Line the notes up");
                    dialog::note(ui, "The topmost note sets x.");
                    // Each tick in the field column, as a form's ticks are, its box beside it:
                    // drawn alone they stood at the labels' edge (the fourth look audit's checks).
                    dialog::labelled(ui, "", |ui| {
                        check(ui, "leaderAlignUseX", "At x", &mut self.align_use_x);
                        ui.add_enabled_ui(self.align_use_x, |ui| {
                            spin_or_words(
                                ui,
                                "leaderAlignX",
                                &mut self.align_x,
                                ALIGN_X,
                                "Topmost note's",
                            );
                        });
                    });
                    dialog::labelled(ui, "", |ui| {
                        check(
                            ui,
                            "leaderAlignUseSpacing",
                            "Stacked",
                            &mut self.align_use_spacing,
                        );
                        ui.add_enabled_ui(self.align_use_spacing, |ui| {
                            controls::spin(
                                ui,
                                "leaderAlignSpacing",
                                &mut self.align_spacing,
                                ALIGN_SPACING,
                            );
                        });
                    });
                }
                {
                    dialog::section(ui, "Number the balloons again");
                    dialog::labelled(ui, "From", |ui| {
                        controls::whole(
                            ui,
                            "balloonRenumberStart",
                            &mut self.renumber_start,
                            RENUMBER_RANGE,
                        );
                    });
                    dialog::labelled(ui, "Order", |ui| {
                        let texts: Vec<String> =
                            ORDER_TEXTS.iter().map(|&text| String::from(text)).collect();
                        let mut order = Choice {
                            index: ORDERS
                                .iter()
                                .position(|&order| order == self.renumber_order),
                            ..Choice::default()
                        };
                        if controls::choice(ui, "balloonRenumberOrder", &texts, &mut order)
                            .changed()
                            && let Some(&chosen) = order.index.and_then(|at| ORDERS.get(at))
                        {
                            self.renumber_order = chosen;
                        }
                    });
                    // In the field column, under the fields, as a form's ticks are.
                    dialog::labelled(ui, "", |ui| {
                        check(
                            ui,
                            "balloonRenumberScoped",
                            "Only the balloons Apply to takes",
                            &mut self.renumber_scoped,
                        )
                        .on_hover_text(
                            "Unticked, every numbered balloon in the drawing is numbered again, \
                             as it always was; ticked, only those the Apply to / Only those that \
                             match controls take - and the others keep their numbers",
                        );
                    });
                }
                layout::quiet_label(
                    ui,
                    "leaderArrangeReport",
                    &self.arrange_report,
                    palette::text().to_egui(),
                );
            });
            ui.vertical(|ui| {
                self.arrange_scope.ui(ui);
            });
        });
    }

    fn footer(&mut self, ui: &mut Ui, _host: &mut dyn Host) {
        let problem = self.problem.clone();
        // The one footer (shell::dialog::button_row), the front tab's: on Leader, Freeze Note and
        // Detach Tip at the left beside the last refusal, away from Apply (as Text Styles keeps
        // Delete), then Close and Apply, the primary; on For Selection, Close and Make for
        // Selection; on Arrange, Close, Renumber Balloons and Align Selected Leaders - the tab's
        // acts, at the right where every tab's act is, Align the primary as its section is the
        // tab's first; at the left they read as tools beside a status (the fifth look audit).
        // Each greys with nothing to act on (`update_arrange_ready`). The tabs' actions stood in
        // their bodies, under the forms, with the footer empty but for Close (the look audits'
        // T5; Arrange's, the third look audit's A).
        let tab = self.tab;
        let (freeze, detach) = (self.freeze_enabled, self.detach_enabled);
        let apply = self.apply_enabled;
        let make = self.for_takes_any;
        let (align, renumber) = (self.align_ready, self.renumber_ready);
        let (freeze_tip, detach_tip) = (self.freeze_tip.clone(), self.detach_tip.clone());
        let (left, (closed, primary)) = dialog::button_row(
            ui,
            |ui| {
                let mut pressed = None;
                if tab == 0 {
                    ui.add_enabled_ui(freeze, |ui| {
                        if button(ui, "leaderFreeze", "Freeze Note")
                            .on_hover_text(freeze_tip.as_str())
                            .clicked()
                        {
                            pressed = Some(Pressed::Freeze);
                        }
                    });
                    ui.add_enabled_ui(detach, |ui| {
                        if button(ui, "leaderDetach", "Detach Tip")
                            .on_hover_text(detach_tip.as_str())
                            .clicked()
                        {
                            pressed = Some(Pressed::Detach);
                        }
                    });
                }
                problem_line(ui, "leaderProblem", &problem);
                pressed
            },
            |ui| {
                let closed = controls::button(ui, "leaderClose", "Close")
                    .on_hover_text("Close the dialog; edits not applied are dropped")
                    .clicked();
                let primary = match tab {
                    0 => ui
                        .add_enabled_ui(apply, |ui| {
                            controls::primary(ui, "leaderApply", "Apply")
                                .on_hover_text(
                                    "Store what the form changed on the leaders shown, as one \
                                     undo step (LEADER SET)",
                                )
                                .clicked()
                        })
                        .inner
                        .then_some(Pressed::Apply),
                    // Greyed while the scope takes nothing: lit under "Nothing is selected" it
                    // invited a press that makes nothing (the fourth look audit's 1).
                    1 => ui
                        .add_enabled_ui(make, |ui| {
                            controls::primary(ui, "leaderForMake", "Make for Selection")
                                .on_hover_text(
                                    "A leader, or a balloon, to each entity the Apply to \
                                     controls take (LEADER FOR, BALLOON FOR)",
                                )
                                .clicked()
                        })
                        .inner
                        .then_some(Pressed::Make),
                    _ => {
                        let renumbered = ui
                            .add_enabled_ui(renumber, |ui| {
                                button(ui, "balloonRenumber", "Renumber Balloons")
                                    .on_hover_text("Number the balloons again (BALLOON RENUMBER)")
                                    .clicked()
                            })
                            .inner;
                        let aligned = ui
                            .add_enabled_ui(align, |ui| {
                                controls::primary(ui, "leaderAlign", "Align Selected Leaders")
                                    .on_hover_text(
                                        "Line up the notes of the leaders Apply to takes \
                                         (LEADER ALIGN)",
                                    )
                                    .clicked()
                            })
                            .inner;
                        if aligned {
                            Some(Pressed::Align)
                        } else {
                            renumbered.then_some(Pressed::Renumber)
                        }
                    }
                };
                (closed, primary)
            },
        );
        if let Some(pressed) = left.or(primary) {
            self.pressed = Some(pressed);
        }
        if closed {
            self.pressed = Some(Pressed::Close);
        }
        // The line the last action handed to the command line, as it ran: muted text under the
        // row, no room until there is one (dialog_layout::reply_text).
        layout::reply_text(ui, "leaderLine", &mut self.line, ControlKind::Line, false)
            .on_hover_text("The command the last action ran, as you could type it");
    }
}

/// The "Says" row of a note's form: what the note says, `text`, on its label's line, named `name`;
/// while it says nothing the row takes no room and the label is recorded where it would be
/// (dialog_layout::quiet_label).
fn says_row(form: &mut dialog::Form<'_>, name: &str, text: &str) {
    if text.is_empty() {
        layout::quiet_label(form.ui(), name, text, palette::text().to_egui());
    } else {
        form.row("Says", |ui| {
            dialog::text_value(ui, |ui| {
                controls::label(ui, name, text);
            });
        });
    }
}

/// A form's row whose label is a control of its own, named `name` (a driver reads it, as it read
/// the C++'s QLabel), in the label colour; what `add` draws beside it, as dialog::labelled lays it
/// out.
fn named_row<R>(ui: &mut Ui, name: &str, text: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    dialog::row_with_text(ui, text, |ui| named_label(ui, name, text), add)
}

/// A label that is a control of its own, named `name`, in the label colour.
fn named_label(ui: &mut Ui, name: &str, text: &str) {
    controls::label_as(
        ui,
        name,
        text,
        egui::Label::new(RichText::new(text).color(palette::text_muted().to_egui())).wrap(),
    );
}

/// A note-kind box: Text, Template, Label style. True when a person or a driver chose another.
fn note_kind_box(ui: &mut Ui, name: &str, kind: &mut LeaderNoteKind) -> bool {
    let texts: Vec<String> = NOTE_KIND_TEXTS
        .iter()
        .map(|&text| String::from(text))
        .collect();
    let mut choice = Choice {
        index: NOTE_KINDS.iter().position(|known| *known == *kind),
        ..Choice::default()
    };
    let changed = controls::choice(ui, name, &texts, &mut choice).changed();
    if changed && let Some(&chosen) = choice.index.and_then(|at| NOTE_KINDS.get(at)) {
        *kind = chosen;
    }
    changed
}

/// A box whose first item leaves the choice to the leader (None), then `values` each as `texts`
/// shows it after that first. True when a person or a driver chose another.
fn optional_box<T: Copy + PartialEq>(
    ui: &mut Ui,
    name: &str,
    texts: &[String],
    values: &[T],
    value: &mut Option<T>,
) -> bool {
    let mut choice = Choice {
        index: Some(match value {
            None => 0,
            Some(chosen) => values
                .iter()
                .position(|known| *known == *chosen)
                .map_or(0, |at| at + 1),
        }),
        ..Choice::default()
    };
    let changed = controls::choice(ui, name, texts, &mut choice).changed();
    if changed {
        *value = match choice.index {
            Some(0) | None => None,
            Some(at) => values.get(at - 1).copied(),
        };
    }
    changed
}
