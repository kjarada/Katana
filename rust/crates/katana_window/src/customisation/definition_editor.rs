//! The definition editor: ONE linestyle or symbol definition of the session's customisation, made,
//! changed, copied or deleted. The port of customisation/definition_editor.
//!
//! One non-modal dialog, made once by the Format workbench and kept, as the managers are. The
//! Symbol Library's New, Edit, Duplicate and Delete and the Linetypes tab's Edit Definition and New
//! Library Linestyle ask for it (`request`); it has New, Duplicate and Delete of its own, so every
//! one of the four is reached from either.
//!
//! ```text
//! left   a FORM over entity's LineStyle - name, group, kind (the format's two lists: a linestyle
//!        or a symbol), units, at vertices, length, factor, origin, and a two-point definition's
//!        anchors and modes - and under it the STROKES as text
//! right  the definition as it is drawn, live and before it has a name: as a symbol at a point
//!        and, for a linestyle, along the sample line; under them a line that says when the
//!        picture is of the last text that read, not of what is typed
//! below  the message area, and the buttons
//! ```
//!
//! THE STROKES ARE TEXT, in the Katana customisation format's own form (docs/customisation.md,
//! "The members"), one stroke a line:
//!
//! ```text
//!     ["move", 0, 0],
//!     ["draw", 8, 0],
//!     ["text", {"text": "W", "height": 1.5, "justify": "middle-centre"}]
//! ```
//!
//! so what a person learns here is what a customisation file holds, and a line copied from one to
//! the other means the same. They are read as they are typed by the format's own reader and by no
//! second parser of this dialog's; what joins the lines for it, finds the line a refusal is about
//! and words it is below the window, where it is tested without one (cad's definition_edit). The
//! comma that ends a line in a file may be left off here, and a blank line is passed over. A
//! number is shown as the shortest text that reads back as itself, and an empty number field is
//! the value a file's missing member has.
//!
//! A NAME IS FIXED once its definition exists. Styles and survey code rules NAME a definition and
//! a library has no rename, so another name would be another definition, and everything naming the
//! first left naming nothing. Duplicate is how a definition comes by a new name. A NEW name is
//! refused when no command line could name the definition by it - a double quote, or a word of the
//! CUSTOMISE REMOVE line itself (cad's remove_definition_line) - since it could then never be
//! removed.
//!
//! THE STROKES BOX IS READ CHARACTER FOR CHARACTER: a no-break space or a line separator in a
//! stroke's text is a character of it like any other, and a form opened on a text holding one
//! reads as unedited. The C++'s box broke a line at a paragraph separator, so a text holding one
//! could not be kept on one line, and a definition with such a text is not opened; that is kept
//! (`box_holds`).
//!
//! COMMITTING. The library is session data with no undo (cad's Document::style_library):
//!
//! ```text
//! Save     installs a copy of the session's library with the definition added or replaced
//!          (Document::set_style_library), as the Survey Code Manager's Apply installs its map.
//!          Enabled only while the form reads as a definition, and for a new one only while its
//!          name is free.
//! Revert   the form as it was opened, or as the library has had the definition since.
//! Delete   the line `CUSTOMISE REMOVE "<name>"` (cad's remove_definition_line) through the
//!          window's one executor (Host::run). That verb refuses a definition something names.
//!          What the line answered is shown as it answered it - the editor does not guess why a
//!          line failed - and, when something does name the definition (cad's definition_users,
//!          which is what the verb asks), that is listed under it and Delete Anyway is offered:
//!          the same line with FORCE. Delete Anyway exists only while that refusal is on the
//!          page: its button is hidden and disabled otherwise, and remove(true) is refused. No box
//!          asks either question: a headless run meets none. What was deleted stays in the form,
//!          and Save puts it back.
//! ```
//!
//! Round each commit the editor calls the context's commit hook, and afterwards what that handed
//! back.
//!
//! The form holds one definition. Opening another while it has edits that are not saved is REFUSED
//! and said, in the message area and the log: there is no undo to get typed strokes back from, so
//! Save or Revert comes first. Closing the WINDOW over such edits asks (the customisation
//! workbench's confirm_close); a yes is remembered here and reverts nothing, since the close may
//! yet be cancelled by a later question.
//!
//! HEADLESS. Enter in a field presses nothing. The object names: definitionHeading;
//! definitionName, definitionGroup, definitionKind, definitionUnits (and definitionUnitsNote),
//! definitionAtVertices, definitionLength, definitionFactor, definitionOriginX / Y,
//! definitionAnchor1X / Y, definitionAnchor2X / Y, definitionStretchMode, definitionCycleMode,
//! definitionSource; definitionStrokes (and definitionStrokesHelp); definitionPreview,
//! definitionLinePreview, definitionPreviewNote, definitionPlotScale; definitionIssues;
//! definitionDeleteAnyway, definitionDeleteCancel (in definitionDeletePanel); definitionNew,
//! definitionDuplicate, definitionDelete, definitionRevert, definitionSave, definitionClose.
//!
//! LIFETIME. As the managers: it watches the Document through a DocumentWatcher, acts on a change
//! at its next frame, and does nothing once the Document it was made with has gone. A definition
//! changed or removed elsewhere while the form holds edits is said in the message area, and what
//! was typed is kept.
//!
//! What is the C++'s and not here. The line of the strokes box that does not read is washed by its
//! place in the box - its number times a row's height - where the C++ washed the text block
//! itself; `washed_lines` says which line.

use egui::{Context, RichText, Ui};
use katana_cad::Document;
use katana_cad::definition_edit::{
    free_definition_name, read_stroke_text, remove_definition_line, stroke_text,
};
use katana_cad::definition_users::{DefinitionUsers, definition_users};
use katana_core::text::parse_finite_double;
use katana_core::{Error, ErrorCode, Result};
use katana_entity::style_library::add_or_replace;
use katana_entity::{LineStyle, StyleUnits};
use katana_geometry::Point2;

use super::codes_seams::{CodesContext, DefinitionAs, DefinitionEdit, StylePreview};
use super::document_watcher::{DocumentChanges, DocumentWatcher};
use crate::annotation::dialog_support::show_dialog;
use crate::command_word::exact_number;
use crate::drive::controls::{self, Choice, LineOptions, TextOptions, custom};
use crate::drive::registry::{ControlKind, ControlRecord};
use crate::host::Host;
use crate::shell::DialogFrame;
use crate::survey::survey_dialogs::hidden;
use crate::theme::palette;

const DIALOG: &str = "definitionEditorDialog";
const TITLE: &str = "Definition Editor";

/// The units box's rows. The words are the format's own for "units" (docs/customisation.md, "The
/// members"), so the box reads as a file does.
const UNITS_ROWS: [(StyleUnits, &str, &str); 3] = [
    (
        StyleUnits::World,
        "world",
        "metres on the ground: its size on a plot follows the plot scale",
    ),
    (
        StyleUnits::Paper,
        "paper",
        "millimetres on the plot: the same size at every scale",
    ),
    (
        StyleUnits::TwoPoint,
        "twoPoint",
        "stretched between the two points it is drawn across",
    ),
];

/// The name a form with no name yet is READ under, for the picture alone: the format's reader
/// wants one, and what a definition draws does not depend on it. Never saved - a form with no name
/// has no definition - and worded so that a refusal citing it still reads.
const STAND_IN_NAME: &str = "(not named yet)";

const STROKES_HELP: &str = "Strokes, one a line, as a customisation file holds them:  [\"move\", x, y]   \
     [\"draw\", x, y]   [\"arc\", radius, start, end]   [\"circle\", radius]   [\"dot\", radius]   \
     [\"pen\", \"colour name\"]   [\"text\", {\"text\": \"W\", \"height\": 1.5}].  Coordinates and \
     radii are in the units above and angles in degrees, counter-clockwise; an arc, a circle, a \
     dot and a text sit at the point the pen is at.";

/// A whole number kept as the customisation gives it: the C++ box's range, an int's.
const MODE_RANGE: std::ops::RangeInclusive<i64> = (i32::MIN as i64)..=(i32::MAX as i64);

fn units_row(units: StyleUnits) -> usize {
    UNITS_ROWS
        .iter()
        .position(|(listed, _, _)| *listed == units)
        .unwrap_or(0)
}

fn in_quotes(value: &str) -> String {
    format!("\"{value}\"")
}

/// A refusal as the message area shows it: what was refused, then what it was refused of. Not
/// Error::describe(), which leads with the code's name.
fn worded(error: &Error) -> String {
    if error.context.is_empty() {
        error.message.clone()
    } else {
        format!("{} - {}", error.message, error.context)
    }
}

fn refusal(problem: &str) -> Error {
    Error::new(ErrorCode::InvalidArgument, problem)
}

/// A number field: its value, `when_empty` for an empty one - a member a file leaves out - or
/// nothing, said in `problem`, for text that is not a number.
fn number_in(field: &str, when_empty: f64, label: &str, problem: &mut String) -> Option<f64> {
    let typed = field.trim();
    if typed.is_empty() {
        return Some(when_empty);
    }
    let value = parse_finite_double(typed);
    if value.is_none() {
        *problem = format!("{label}: \"{typed}\" is not a number.");
    }
    value
}

/// Whether the strokes box holds `strokes` as given, one stroke a line. The C++'s box broke a line
/// at more than a line feed - at a paragraph separator, which a stroke's text may hold - and a
/// definition with such a text was not opened as something it is not.
fn box_holds(strokes: &str) -> bool {
    !strokes.contains('\u{2029}')
}

/// What names a definition is drawn as once the library has none of that name: a sentence for
/// what names it as a linetype and one for what draws it as a symbol. The drawing's own linetype
/// of the name, or the built-in shape of that name, answers in its place (cad's DefinitionUsers),
/// and only where neither does is a line plain and a point a stand-in mark.
fn drawn_without_it(users: &DefinitionUsers) -> Vec<String> {
    let mut out = Vec::new();
    if users.named_as_linetype() {
        out.push(String::from(if users.drawing_linetype {
            "A line naming it is then drawn with the drawing's own linetype of that name."
        } else {
            "A line naming it is then drawn plain."
        }));
    }
    if users.named_as_symbol() {
        out.push(String::from(if users.built_in_shape {
            "A point naming it is then drawn as the built-in shape of that name."
        } else {
            "A point naming it is then drawn as a stand-in mark."
        }));
    }
    out
}

/// The same of a definition that is NOT in the library - being made, or deleted and still in the
/// form: what its name draws until it is saved, and what saving it takes over. `at_vertices` is
/// the form's: the resolver draws a definition along a line only when it is not at vertices
/// (cad's style_resolver, decision D2).
fn drawn_until_saved(users: &DefinitionUsers, at_vertices: bool, name: &str) -> Vec<String> {
    let mut out = Vec::new();
    if users.drawing_linetype {
        out.push(if at_vertices {
            format!(
                "The drawing has a linetype {name} of its own, which goes on drawing the lines \
                 that name it: a definition at vertices is not drawn along a line."
            )
        } else {
            format!(
                "The drawing has a linetype {name} of its own, which draws the lines that name it \
                 now. Saved, this definition is drawn in its place, and Styles and Linetypes lists \
                 the name under Diagnostics as one in both."
            )
        });
    } else if users.named_as_linetype() {
        out.push(String::from(if at_vertices {
            "A line naming it is drawn plain, and stays so: a definition at vertices is not drawn \
             along a line."
        } else {
            "Until it is saved a line naming it is drawn plain."
        }));
    }
    if users.built_in_shape {
        out.push(format!(
            "{name} is a built-in shape, which draws the points that name it now. Saved, this \
             definition is drawn in its place."
        ));
    } else if users.named_as_symbol() {
        out.push(String::from(
            "Until it is saved a point naming it is drawn as a stand-in mark.",
        ));
    }
    out
}

fn counted(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

fn kind_word(symbol: bool) -> &'static str {
    if symbol { "symbol" } else { "linestyle" }
}

/// A user a line, set in under the sentence that introduces them.
fn listed(each: &[String]) -> String {
    each.iter()
        .map(|line| format!("  {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// True when `shown` holds every one of `lines`, and there is one to hold: whether the reply on
/// the page already cites each user of a definition as CUSTOMISE REMOVE's refusal cites it. Not a
/// reading of the reply - the lines are the verb's own, made by the function the verb makes them
/// with.
fn cites_every(shown: &str, lines: &[String]) -> bool {
    !lines.is_empty() && lines.iter().all(|line| shown.contains(line.as_str()))
}

/// A choice over `items` by its row: true when a person or a driver chose another.
pub(crate) fn index_choice(ui: &mut Ui, name: &str, items: &[String], index: &mut usize) -> bool {
    let mut chosen = Choice::at(*index);
    let changed = controls::choice(ui, name, items, &mut chosen).changed();
    if let Some(at) = chosen.index {
        *index = at;
    }
    changed
}

/// Everything the form shows that a person can change. Two of them equal is "nothing was
/// edited"; it is compared as TEXT because that is what was typed, and a number field holding
/// "0.50" has been edited although the definition it describes has not.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Form {
    name: String,
    group: String,
    symbol: bool,
    units: usize,
    at_vertices: bool,
    length: String,
    factor: String,
    origin_x: String,
    origin_y: String,
    anchor1_x: String,
    anchor1_y: String,
    anchor2_x: String,
    anchor2_y: String,
    stretch_mode: i64,
    cycle_mode: i64,
    strokes: String,
}

/// What reading the form gave: a definition, or why none and at which line.
#[derive(Clone, Debug, Default)]
struct Reading {
    definition: Option<LineStyle>,
    /// What the fields and the strokes DRAW, which needs no name: a new definition is pictured
    /// from its first stroke, under a stand-in name while it has none. Set whenever `definition`
    /// is.
    drawable: Option<LineStyle>,
    error: Error,
    problem: String,
    line: i32,
}

/// The same form without its strokes, which is all of a definition that has none.
fn fields_of(definition: &LineStyle) -> Form {
    Form {
        name: definition.name.clone(),
        group: definition.group.clone(),
        symbol: definition.symbol,
        units: units_row(definition.units),
        at_vertices: definition.at_vertices,
        length: exact_number(definition.length),
        factor: exact_number(definition.factor),
        origin_x: exact_number(definition.origin.x),
        origin_y: exact_number(definition.origin.y),
        anchor1_x: exact_number(definition.anchor1.x),
        anchor1_y: exact_number(definition.anchor1.y),
        anchor2_x: exact_number(definition.anchor2.x),
        anchor2_y: exact_number(definition.anchor2.y),
        stretch_mode: i64::from(definition.stretch_mode),
        cycle_mode: i64::from(definition.cycle_mode),
        strokes: String::new(),
    }
}

/// The form a definition is shown as. Fails, saying why as the end of a sentence ("because
/// ..."), when the format cannot write its strokes (a stroke carrying what its kind does not use)
/// or the strokes box cannot hold them one a line (a text holding a paragraph separator).
fn form_of(definition: &LineStyle) -> Result<Form> {
    let strokes = stroke_text(definition).map_err(|error| {
        Error::new(
            error.code,
            format!(
                "the customisation format cannot write it: {}",
                worded(&error)
            ),
        )
    })?;
    let mut form = fields_of(definition);
    form.strokes = strokes;
    if !box_holds(&form.strokes) {
        return Err(Error::new(
            ErrorCode::Unsupported,
            "one of its texts holds a paragraph separator, which the strokes box breaks a line \
             at: its strokes could not be shown one a line",
        ));
    }
    Ok(form)
}

/// A button pressed in a frame, carried out after the frame's drawing with the host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pressed {
    New,
    Duplicate,
    Delete,
    DeleteAnyway,
    DeleteCancel,
    Revert,
    Save,
    Close,
}

/// The definition editor (see the module comment).
pub struct DefinitionEditorDialog {
    frame: DialogFrame,
    open: bool,
    context: CodesContext,
    /// The library definition Save replaces; None while a new one is being made.
    existing: Option<String>,
    /// The library's definition of that name as the editor last knew it; None when the library
    /// had none. What "changed elsewhere" is measured against.
    seen: Option<LineStyle>,
    baseline: Form,
    form: Form,
    /// LineStyle::source: where the definition came from, which no field edits.
    source: String,
    /// How the form came to hold what it does, for the heading: "a copy of ...".
    origin: String,
    /// What happened to the definition outside the form; kept until Save or Revert.
    note: String,
    /// What the editor last did or refused, shown first until the next edit.
    said: String,
    said_is_error: bool,
    /// The plain REMOVE line was refused and something names the definition: what does is listed,
    /// and Delete Anyway offered, until the form changes.
    remove_refused: bool,
    discard_agreed: bool,
    /// A picture is up in the panes; it is of the last form that read.
    pictured: bool,
    problem_line: i32,

    // What follows from the form and the Document, worked out by `refresh`.
    heading: String,
    units_note: String,
    source_label: String,
    preview_note: String,
    issues: String,
    issues_error: bool,
    live: bool,
    two_point: bool,
    save_enabled: bool,
    revert_enabled: bool,
    in_library: bool,
    offered: bool,

    preview: StylePreview,
    line_preview: StylePreview,
    plot_scale: usize,

    /// A control of the form was edited this frame.
    edited_now: bool,
    pressed: Option<Pressed>,
    logs: Vec<(String, bool)>,
    watcher: DocumentWatcher,
}

impl DefinitionEditorDialog {
    /// The editor over `document`, shown, holding an empty linestyle.
    pub fn new(document: &Document, context: CodesContext) -> Self {
        let preview = StylePreview::new();
        let plot_scale = StylePreview::PLOT_SCALES
            .iter()
            .position(|&denominator| denominator == preview.scale_denominator())
            .unwrap_or(0);
        let mut dialog = Self {
            frame: DialogFrame::new(DIALOG, TITLE),
            open: true,
            context,
            existing: None,
            seen: None,
            baseline: Form::default(),
            form: Form::default(),
            source: String::new(),
            origin: String::new(),
            note: String::new(),
            said: String::new(),
            said_is_error: false,
            remove_refused: false,
            discard_agreed: false,
            pictured: false,
            problem_line: 0,
            heading: String::new(),
            units_note: String::new(),
            source_label: String::new(),
            preview_note: String::new(),
            issues: String::new(),
            issues_error: false,
            live: true,
            two_point: false,
            save_enabled: false,
            revert_enabled: false,
            in_library: false,
            offered: false,
            preview: preview.clone(),
            line_preview: preview,
            plot_scale,
            edited_now: false,
            pressed: None,
            logs: Vec::new(),
            watcher: DocumentWatcher::new(document),
        };
        dialog.load(
            Some(document),
            fields_of(&LineStyle::default()),
            None,
            String::new(),
            String::new(),
        );
        dialog
    }

    fn alive(&self) -> bool {
        self.watcher.document_alive()
    }

    /// `document`, while it is the Document the editor was made with and that is still there.
    fn live_document<'d>(&self, document: &'d Document) -> Option<&'d Document> {
        self.alive().then_some(document)
    }

    fn log(&mut self, message: &str, is_error: bool) {
        self.logs.push((String::from(message), is_error));
    }

    /// What was said since the last flush, to the host's log in the order it was said.
    fn flush(&mut self, host: &mut dyn Host) {
        for (message, is_error) in std::mem::take(&mut self.logs) {
            host.log(&message, is_error);
        }
    }

    // ---- what it holds -------------------------------------------------------------------------

    /// The library definition Save replaces; empty while a new one is being made. It may no
    /// longer be IN the library: deleted here, or elsewhere.
    pub fn edited_name(&self) -> String {
        self.existing.clone().unwrap_or_default()
    }

    /// The form differs from what it was opened with.
    pub fn dirty(&self) -> bool {
        self.form != self.baseline
    }

    /// The definition the form describes, or why it describes none - the format's own refusal, or
    /// entity's validate's.
    pub fn definition(&self) -> Result<LineStyle> {
        let reading = self.read();
        reading.definition.ok_or(reading.error)
    }

    /// The line of the strokes box, counted from 1, that the last reading stopped at; 0 when it
    /// read, or stopped at something else.
    pub fn problem_line(&self) -> i32 {
        self.problem_line
    }

    /// The lines of the strokes box that are washed as not reading, from 1.
    pub fn washed_lines(&self) -> Vec<i32> {
        let lines = self.form.strokes.split('\n').count();
        let washed = usize::try_from(self.problem_line).unwrap_or(0);
        if (1..=lines).contains(&washed) {
            vec![self.problem_line]
        } else {
            Vec::new()
        }
    }

    /// The heading: what the form is on, and whether it is edited.
    pub fn heading(&self) -> &str {
        &self.heading
    }

    /// The message area's text.
    pub fn issues(&self) -> &str {
        &self.issues
    }

    /// Whether the message area is in the colour of a refusal.
    pub fn says_an_error(&self) -> bool {
        self.issues_error
    }

    /// The window is closing, a person was asked whether the unsaved edits here may be discarded,
    /// and said yes (the customisation workbench's confirm_close). Only REMEMBERED, until the form
    /// next changes: a close has more questions after this one, and one answered Cancel must find
    /// what was typed still here. The edits go when the window does.
    pub fn agree_to_discard(&mut self) {
        self.discard_agreed = self.dirty();
    }

    pub fn discard_agreed(&self) -> bool {
        self.discard_agreed
    }

    pub fn symbol_preview(&self) -> &StylePreview {
        &self.preview
    }

    pub fn line_preview(&self) -> &StylePreview {
        &self.line_preview
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn show(&mut self) {
        self.open = true;
    }

    /// A field of the form, or the strokes box, set by its object name as a program sets it -
    /// whether or not a person could type into it now - and what follows from the edit. False for
    /// a name that is no text field of the form.
    pub fn type_into(&mut self, host: &mut dyn Host, name: &str, text: &str) -> bool {
        let slot = match name {
            "definitionName" => &mut self.form.name,
            "definitionGroup" => &mut self.form.group,
            "definitionLength" => &mut self.form.length,
            "definitionFactor" => &mut self.form.factor,
            "definitionOriginX" => &mut self.form.origin_x,
            "definitionOriginY" => &mut self.form.origin_y,
            "definitionAnchor1X" => &mut self.form.anchor1_x,
            "definitionAnchor1Y" => &mut self.form.anchor1_y,
            "definitionAnchor2X" => &mut self.form.anchor2_x,
            "definitionAnchor2Y" => &mut self.form.anchor2_y,
            "definitionStrokes" => &mut self.form.strokes,
            _ => return false,
        };
        if slot.as_str() != text {
            *slot = String::from(text);
            let document = self.live_document(host.document());
            self.edited(document);
        }
        true
    }

    // ---- the form ------------------------------------------------------------------------------

    fn read(&self) -> Reading {
        let mut reading = Reading::default();
        // Everything wrong, the first of them as the error: a name that is missing does not stop
        // the strokes being read, so a new definition is pictured, and a line of it that does not
        // read is washed, before it has a name.
        let mut problems: Vec<String> = Vec::new();
        fn wrong(reading: &mut Reading, problems: &mut Vec<String>, why: String, error: Error) {
            if problems.is_empty() {
                reading.error = error;
            }
            problems.push(why);
        }

        let mut members = LineStyle::default();
        // A blank at an end of a NEW name is taken off: it cannot be seen where the name is shown,
        // and a rule naming "Valve" would miss "Valve ". The name of a definition that exists is
        // its own, whatever it holds.
        let name = match &self.existing {
            Some(existing) => existing.clone(),
            None => String::from(self.form.name.trim()),
        };
        if name.is_empty() {
            let why = String::from(
                "Type a name: it is what a style or a survey code names the definition by.",
            );
            let error = refusal(&why);
            wrong(&mut reading, &mut problems, why, error);
        } else if self.existing.is_none() {
            // Nor a name no command line could say: saved, the definition could never be removed
            // again (cad's remove_definition_line says which names those are). One that already
            // exists under such a name - out of a file - is still opened and changed.
            if let Err(error) = remove_definition_line(&name, false) {
                let why = format!(
                    "A new definition cannot be named {}: {}.",
                    in_quotes(&name),
                    error.message
                );
                wrong(&mut reading, &mut problems, why, error);
            }
        }
        members.group = self.form.group.clone();
        members.symbol = self.form.symbol;
        members.units = UNITS_ROWS[self.form.units.min(UNITS_ROWS.len() - 1)].0;
        members.at_vertices = self.form.at_vertices;
        // Read whether or not their fields can be typed into: a definition that is not two-point
        // may still carry anchors, and an edit of its length must not drop them.
        let mut problem = String::new();
        let fresh = LineStyle::default();
        let mut number = |field: &str, when_empty: f64, label: &str| -> Option<f64> {
            if problem.is_empty() {
                number_in(field, when_empty, label, &mut problem)
            } else {
                None
            }
        };
        let length = number(&self.form.length, fresh.length, "Length");
        let factor = number(&self.form.factor, fresh.factor, "Factor");
        let origin_x = number(&self.form.origin_x, 0.0, "Origin x");
        let origin_y = number(&self.form.origin_y, 0.0, "Origin y");
        let anchor1_x = number(&self.form.anchor1_x, 0.0, "Anchor 1 x");
        let anchor1_y = number(&self.form.anchor1_y, 0.0, "Anchor 1 y");
        let anchor2_x = number(&self.form.anchor2_x, 0.0, "Anchor 2 x");
        let anchor2_y = number(&self.form.anchor2_y, 0.0, "Anchor 2 y");
        let (
            Some(length),
            Some(factor),
            Some(origin_x),
            Some(origin_y),
            Some(anchor1_x),
            Some(anchor1_y),
            Some(anchor2_x),
            Some(anchor2_y),
        ) = (
            length, factor, origin_x, origin_y, anchor1_x, anchor1_y, anchor2_x, anchor2_y,
        )
        else {
            // With a number that is none there are no members to read strokes under.
            let error = refusal(&problem);
            wrong(&mut reading, &mut problems, problem, error);
            reading.problem = problems.join("\n");
            return reading;
        };
        members.length = length;
        members.factor = factor;
        members.origin = Point2::new(origin_x, origin_y);
        members.anchor1 = Point2::new(anchor1_x, anchor1_y);
        members.anchor2 = Point2::new(anchor2_x, anchor2_y);
        members.stretch_mode = i32::try_from(self.form.stretch_mode).unwrap_or(0);
        members.cycle_mode = i32::try_from(self.form.cycle_mode).unwrap_or(0);
        members.source = self.source.clone();
        members.name = if name.is_empty() {
            String::from(STAND_IN_NAME)
        } else {
            name
        };

        // The strokes as typed, read under those members by the format's reader (cad's
        // read_stroke_text) - which is also where entity's validate refuses a length below 0 or a
        // factor of 0.
        let strokes = read_stroke_text(&members, &self.form.strokes);
        match strokes.definition {
            Some(definition) => reading.drawable = Some(definition),
            None => {
                reading.line = strokes.line;
                wrong(&mut reading, &mut problems, strokes.problem, strokes.error);
            }
        }
        if problems.is_empty() {
            reading.definition = reading.drawable.clone();
        } else {
            reading.problem = problems.join("\n");
        }
        reading
    }

    // ---- opening -------------------------------------------------------------------------------

    fn say(&mut self, document: Option<&Document>, message: &str, is_error: bool) {
        self.said = String::from(message);
        self.said_is_error = is_error;
        self.refresh(document);
    }

    /// Whether another definition may take the form's place; says so when not.
    fn may_replace(&mut self, document: Option<&Document>, what: &str) -> bool {
        if document.is_none() {
            self.refresh(None);
            return false;
        }
        if !self.dirty() {
            return true;
        }
        let held = match &self.existing {
            Some(existing) => in_quotes(existing),
            None => format!("a new {}", kind_word(self.form.symbol)),
        };
        let message = format!(
            "{what}: the definition editor holds edits to {held} that are not saved. Save or \
             Revert them first."
        );
        self.log(&message, true);
        self.say(document, &message, true);
        false
    }

    /// The form loaded with `form` as what there is to go back to.
    fn load(
        &mut self,
        document: Option<&Document>,
        form: Form,
        existing: Option<String>,
        source: String,
        origin: String,
    ) {
        self.existing = existing;
        self.source = source;
        self.origin = origin;
        self.baseline = form.clone();
        self.note.clear();
        self.said.clear();
        self.remove_refused = false;
        self.discard_agreed = false;
        self.seen = match (&self.existing, document) {
            (Some(existing), Some(document)) => document.style_library().find(existing).cloned(),
            _ => None,
        };
        // Not the last definition's picture over a form that does not read yet.
        self.preview.clear();
        self.line_preview.clear();
        self.pictured = false;
        self.form = form;
        self.refresh(document);
    }

    /// An empty form for a symbol or a linestyle. Refused - false, said in the message area and
    /// the log - while the form has edits that are not saved, and once the Document has gone.
    pub fn new_definition(&mut self, host: &mut dyn Host, symbol: bool) -> bool {
        let document = self.live_document(host.document());
        let opened = self.new_definition_in(document, symbol);
        self.flush(host);
        opened
    }

    fn new_definition_in(&mut self, document: Option<&Document>, symbol: bool) -> bool {
        if !self.may_replace(document, "New") {
            return false;
        }
        let mut form = fields_of(&LineStyle::default());
        form.symbol = symbol;
        self.load(document, form, None, String::new(), String::new());
        true
    }

    /// The library's definition `name`, its name fixed. Refused as `new_definition` is, and for a
    /// name the library does not define.
    pub fn edit_definition(&mut self, host: &mut dyn Host, name: &str) -> bool {
        let document = self.live_document(host.document());
        let opened = self.edit_definition_in(document, name);
        self.flush(host);
        opened
    }

    fn edit_definition_in(&mut self, document: Option<&Document>, name: &str) -> bool {
        // Already on it: what is typed stays.
        if self.existing.as_deref() == Some(name) && document.is_some() {
            return true;
        }
        if !self.may_replace(document, &format!("Edit {}", in_quotes(name))) {
            return false;
        }
        let Some(document) = document else {
            return false;
        };
        let Some(found) = document.style_library().find(name) else {
            let message = format!("Edit: the library has no definition {}.", in_quotes(name));
            self.log(&message, true);
            self.say(Some(document), &message, true);
            return false;
        };
        match form_of(found) {
            Ok(form) => {
                self.load(
                    Some(document),
                    form,
                    Some(String::from(name)),
                    found.source.clone(),
                    String::new(),
                );
                true
            }
            Err(error) => {
                let message = format!(
                    "{} cannot be edited here, because {}",
                    in_quotes(name),
                    error.message
                );
                self.log(&message, true);
                self.say(Some(document), &message, true);
                false
            }
        }
    }

    /// A copy of the library's definition `name` under the first free name ("TEST Valve 2"), as a
    /// new definition whose name can still be changed. Refused as `edit_definition` is.
    pub fn duplicate_definition(&mut self, host: &mut dyn Host, name: &str) -> bool {
        let document = self.live_document(host.document());
        let opened = self.duplicate_definition_in(document, name);
        self.flush(host);
        opened
    }

    fn duplicate_definition_in(&mut self, document: Option<&Document>, name: &str) -> bool {
        if !self.may_replace(document, &format!("Duplicate {}", in_quotes(name))) {
            return false;
        }
        let Some(document) = document else {
            return false;
        };
        let Some(found) = document.style_library().find(name) else {
            let message = format!(
                "Duplicate: the library has no definition {}.",
                in_quotes(name)
            );
            self.log(&message, true);
            self.say(Some(document), &message, true);
            return false;
        };
        // Equal but for its name - where it came from included: its strokes did.
        let mut copy = found.clone();
        // The first name free in the library AND in the drawing's own linetypes, so the copy does
        // not begin over either (cad's free_definition_name).
        copy.name = free_definition_name(document, name);
        match form_of(&copy) {
            Ok(form) => {
                self.load(
                    Some(document),
                    form,
                    None,
                    copy.source.clone(),
                    format!("a copy of {}", in_quotes(name)),
                );
                true
            }
            Err(error) => {
                let message = format!(
                    "{} cannot be copied here, because {}",
                    in_quotes(name),
                    error.message
                );
                self.log(&message, true);
                self.say(Some(document), &message, true);
                false
            }
        }
    }

    /// Shows the library's definition `name`, then runs Delete on it (`remove`). True when it
    /// went.
    pub fn delete_definition(&mut self, host: &mut dyn Host, name: &str) -> bool {
        // Asked here, so the refusal over unsaved edits says what was asked for.
        let on_it = self.existing.as_deref() == Some(name);
        let document = self.live_document(host.document());
        let shown = (on_it
            || self.may_replace(document, &format!("Delete {}", in_quotes(name))))
            && self.edit_definition_in(document, name);
        let removed = shown && self.remove_in(host, false);
        self.flush(host);
        removed
    }

    /// A manager's request, as one of the four.
    pub fn request(&mut self, host: &mut dyn Host, what: DefinitionEdit, name: &str) -> bool {
        match what {
            DefinitionEdit::NewSymbol => self.new_definition(host, true),
            DefinitionEdit::NewLinestyle => self.new_definition(host, false),
            DefinitionEdit::Edit => self.edit_definition(host, name),
            DefinitionEdit::Duplicate => self.duplicate_definition(host, name),
            DefinitionEdit::Delete => self.delete_definition(host, name),
        }
    }

    // ---- committing ----------------------------------------------------------------------------

    /// Save. False, and nothing changed, when the form does not read, a new definition's name is
    /// taken, or the Document has gone.
    pub fn save(&mut self, host: &mut dyn Host) -> bool {
        let saved = self.save_in(host);
        self.flush(host);
        saved
    }

    fn save_in(&mut self, host: &mut dyn Host) -> bool {
        if !self.alive() {
            self.refresh(None);
            return false;
        }
        let reading = self.read();
        let saved = match reading.definition {
            Some(definition)
                if self.existing.is_some()
                    || !host.document().style_library().contains(&definition.name) =>
            {
                definition
            }
            _ => {
                // The message area says which.
                self.refresh(Some(host.document()));
                return false;
            }
        };
        // A COPY of the library with the definition in it, installed whole: the library has no
        // command and no undo (Document::style_library), and this is how the Survey Code
        // Manager's Apply commits its map.
        let mut library = host.document().style_library().clone();
        let replaced = match add_or_replace(&mut library, saved.clone()) {
            Ok(replaced) => replaced,
            Err(error) => {
                self.say(Some(host.document()), &worded(&error), true);
                return false;
            }
        };
        let committed = self.context.begin(host);
        host.document_mut().set_style_library(library);
        if let Some(committed) = committed {
            committed(host);
        }

        self.existing = Some(saved.name.clone());
        self.seen = Some(saved.clone());
        self.origin.clear();
        self.note.clear();
        self.remove_refused = false;
        self.discard_agreed = false;
        // A new name was trimmed on the way in; the field shows what was saved.
        self.form.name = saved.name.clone();
        self.baseline = self.form.clone();
        let done = format!(
            "{} {} {}: {}, {}.",
            if saved.symbol { "Symbol" } else { "Linestyle" },
            in_quotes(&saved.name),
            if replaced {
                "saved"
            } else {
                "added to the library"
            },
            counted(saved.strokes.len(), "stroke", "strokes"),
            counted(saved.texts.len(), "text", "texts")
        );
        self.log(&done, false);
        self.say(Some(host.document()), &done, false);
        true
    }

    /// Revert: the form as it was opened, or as the library has had the definition since.
    pub fn revert(&mut self, host: &mut dyn Host) {
        let document = self.live_document(host.document());
        self.revert_in(document);
        self.flush(host);
    }

    fn revert_in(&mut self, document: Option<&Document>) {
        self.said.clear();
        self.remove_refused = false;
        self.discard_agreed = false;
        // "Changed elsewhere" is answered by showing what the library has; a definition that is
        // no longer there is still not there.
        if self.seen.is_some() || self.existing.is_none() {
            self.note.clear();
        }
        // The picture of what was typed goes with it: what the form goes back to may not read at
        // all.
        self.preview.clear();
        self.line_preview.clear();
        self.pictured = false;
        self.form = self.baseline.clone();
        self.refresh(document);
    }

    fn offered_in(&self, document: Option<&Document>) -> bool {
        match (document, &self.existing) {
            (Some(document), Some(existing)) => {
                self.remove_refused && document.style_library().contains(existing)
            }
            _ => false,
        }
    }

    /// Whether the plain line's refusal is showing, and Delete Anyway with it.
    pub fn delete_anyway_offered(&self, document: &Document) -> bool {
        self.offered_in(self.live_document(document))
    }

    /// `CUSTOMISE REMOVE "<name>"`, with FORCE when `anyway`. True when the line was carried out.
    /// `anyway` is refused, and nothing run, unless the plain line's refusal is showing with what
    /// uses the definition: FORCE answers a list somebody has read, not a press out of nowhere.
    pub fn remove(&mut self, host: &mut dyn Host, anyway: bool) -> bool {
        let removed = self.remove_in(host, anyway);
        self.flush(host);
        removed
    }

    fn remove_in(&mut self, host: &mut dyn Host, anyway: bool) -> bool {
        if !self.alive() {
            self.refresh(None);
            return false;
        }
        let name = match &self.existing {
            Some(existing) if host.document().style_library().contains(existing) => {
                existing.clone()
            }
            _ => {
                self.say(
                    Some(host.document()),
                    "Delete: the form holds no definition that is in the library.",
                    true,
                );
                return false;
            }
        };
        // FORCE answers a refusal that is on the page with what uses the definition listed under
        // it. Asked for with none showing - the button pressed while the page was not offering
        // it, by a script or a caller - it is refused: nobody has been shown what it would pass
        // over, and the library has no undo.
        if anyway && !self.remove_refused {
            self.say(
                Some(host.document()),
                "Delete Anyway is offered once Delete has been refused, under the list of what \
                 uses the definition: press Delete first.",
                true,
            );
            return false;
        }
        // The line, or why no line can name this definition: a double quote in its name, or a
        // name that is one of the line's own words (cad's remove_definition_line).
        let line = match remove_definition_line(&name, anyway) {
            Ok(line) => line,
            Err(error) => {
                let message = format!(
                    "Delete: no command line can name {} - {}.",
                    in_quotes(&name),
                    error.message
                );
                self.say(Some(host.document()), &message, true);
                return false;
            }
        };
        if !self.context.command_line {
            self.say(
                Some(host.document()),
                "Nothing here can run CUSTOMISE REMOVE: the editor has no command line.",
                true,
            );
            return false;
        }
        let committed = self.context.begin(host);
        self.flush(host);
        let outcome = host.run(&line);
        if !self.alive() {
            return false;
        }
        if !outcome.ok {
            // What the line answered, as it answered it: the editor does not guess why a line
            // failed. Delete Anyway is offered when it was the PLAIN line that failed and
            // something names the definition - which is what the verb refuses for, by the
            // function it asks - and the list goes under the verb's own words, so a line that
            // failed for another reason is read as that. A FORCE that failed has nothing further
            // to offer.
            self.remove_refused = !anyway && !definition_users(host.document(), &name).empty();
            let message = format!(
                "{} was not deleted: {}",
                in_quotes(&name),
                if outcome.error.is_empty() {
                    "the command was refused."
                } else {
                    outcome.error.as_str()
                }
            );
            self.say(Some(host.document()), &message, true);
            return false;
        }
        if let Some(committed) = committed {
            committed(host);
        }
        self.remove_refused = false;
        self.discard_agreed = false;
        self.seen = None;
        self.note = format!(
            "{} was deleted from the library. It is still shown here: Save puts it back.",
            in_quotes(&name)
        );
        self.said.clear();
        self.refresh(Some(host.document()));
        true
    }

    /// Closing hides the dialog and keeps what was typed; said in the log, so a headless run that
    /// closed it over edits reads that it did.
    pub fn reject(&mut self, host: &mut dyn Host) {
        if self.alive() && self.dirty() {
            self.log(
                "The definition editor was closed with edits that are not saved; they are kept \
                 in it.",
                false,
            );
        }
        self.open = false;
        self.flush(host);
    }

    // ---- following the form and the Document ---------------------------------------------------

    fn edited(&mut self, document: Option<&Document>) {
        // What the editor last said - and what a person agreed might be discarded - was about the
        // form as it then was.
        self.said.clear();
        self.remove_refused = false;
        self.discard_agreed = false;
        self.refresh(document);
    }

    fn on_document_changed(&mut self, document: &Document, changes: DocumentChanges) {
        if changes.library
            && let Some(existing) = self.existing.clone()
        {
            let now = document.style_library().find(&existing).cloned();
            if now != self.seen {
                let was_edited = self.dirty();
                self.seen = now.clone();
                match now {
                    None => {
                        self.note = format!(
                            "{} was removed from the library elsewhere. It is still shown here: \
                             Save puts it back.",
                            in_quotes(&existing)
                        );
                    }
                    Some(now) => match form_of(&now) {
                        Err(error) => {
                            self.note = format!(
                                "{} was changed elsewhere into something that cannot be shown \
                                 here, because {}. What is typed here is kept.",
                                in_quotes(&existing),
                                error.message
                            );
                        }
                        Ok(form) if !was_edited => {
                            // Nothing typed is lost by showing what the library has now.
                            self.source = now.source.clone();
                            self.baseline = form.clone();
                            self.note.clear();
                            self.form = form;
                        }
                        Ok(form) => {
                            // What was typed is kept, and what it is measured against moves to
                            // the library's: Revert must show what is there now, not what was
                            // there when the form was opened.
                            self.source = now.source.clone();
                            self.baseline = form;
                            self.note = format!(
                                "{} was changed elsewhere since it was opened here. What is \
                                 typed here is kept: Save puts it in place of the library's, \
                                 Revert shows the library's.",
                                in_quotes(&existing)
                            );
                        }
                    },
                }
            }
        }
        if changes.library || changes.survey_map || changes.model {
            self.refresh(Some(document));
        }
    }

    /// Everything that follows from what the form holds: the previews, the message area, the
    /// heading and which buttons can be pressed. `document` is None once the Document has gone.
    fn refresh(&mut self, document: Option<&Document>) {
        let live = document.is_some();
        let reading = self.read();
        let symbol = self.form.symbol;
        let units = UNITS_ROWS[self.form.units.min(UNITS_ROWS.len() - 1)];
        let two_point = units.0 == StyleUnits::TwoPoint;
        self.problem_line = reading.line;

        // The picture follows text that reads, named or not. While a line is half typed the last
        // definition that did read stays up - a pane that went blank at every keystroke of a
        // stroke would show nothing for most of the time a person types one - and the note under
        // the panes says the picture is not of what is typed now.
        if let Some(drawable) = &reading.drawable {
            self.preview.set_definition(drawable, DefinitionAs::Symbol);
            self.line_preview
                .set_definition(drawable, DefinitionAs::Linestyle);
            self.pictured = true;
        }
        self.preview_note = if self.pictured && reading.drawable.is_none() {
            String::from("Not what is typed: the picture is of the last text that read.")
        } else {
            String::new()
        };

        let in_library = match (document, &self.existing) {
            (Some(document), Some(existing)) => document.style_library().contains(existing),
            _ => false,
        };
        let taken = match (document, &self.existing, &reading.definition) {
            (Some(document), None, Some(definition)) => {
                document.style_library().contains(&definition.name)
            }
            _ => false,
        };
        let changed = self.dirty();
        let offered = self.offered_in(document);

        let mut parts: Vec<String> = Vec::new();
        let mut is_error = false;
        match document {
            None => {
                parts.push(String::from(
                    "The drawing this editor worked on has closed: nothing here acts on anything \
                     any more.",
                ));
                is_error = true;
            }
            Some(document) => {
                if !self.said.is_empty() {
                    parts.push(self.said.clone());
                    is_error = self.said_is_error;
                }
                if offered && let Some(existing) = &self.existing {
                    // Of the definition the form is ON, whatever the form reads as now: Delete
                    // acts on the library's, and an offer with nothing under it would be FORCE
                    // unexplained.
                    let users = definition_users(document, existing);
                    // ONCE. The verb's refusal, shown above as it came, cites who names the
                    // definition, in its own words (DefinitionUsers::cited), and a person read
                    // the same users twice when this list followed it. The list goes here only
                    // where what is shown above does not cite every one of them: a line that
                    // failed for another reason cites nobody, and a refusal that came before
                    // something else came to name the definition does not cite that - and FORCE
                    // is never offered with nothing saying what it passes over.
                    if !cites_every(&self.said, &users.cited(existing)) {
                        parts.push(format!("It is named by:\n{}", listed(&users.describe())));
                    }
                    let mut offer = vec![String::from(
                        "Delete Anyway runs the line again with FORCE.",
                    )];
                    offer.extend(drawn_without_it(&users));
                    parts.push(offer.join(" "));
                    is_error = true;
                }
                match &reading.definition {
                    None => {
                        parts.push(reading.problem.clone());
                        is_error = true;
                    }
                    Some(definition) => {
                        let name = &definition.name;
                        if taken {
                            parts.push(format!(
                                "{} is already in the library. A new definition needs a name of \
                                 its own; Edit changes the one there is.",
                                in_quotes(name)
                            ));
                            is_error = true;
                        }
                        if !self.note.is_empty() {
                            parts.push(self.note.clone());
                        }
                        if definition.strokes.is_empty() {
                            // The format holds one, so it is saved; said, since a definition
                            // that draws nothing is rarely what was meant.
                            parts.push(String::from(
                                "No strokes yet: a definition is drawn by its strokes, and this \
                                 one has none.",
                            ));
                        }
                        // Not of a name that is taken: what names that, names the definition
                        // there is and not the one being made. Nor twice, under a refusal that
                        // has just listed them.
                        if !taken && !offered {
                            let users = definition_users(document, name);
                            let named = users.describe();
                            if in_library {
                                parts.push(if named.is_empty() {
                                    String::from("No survey code, style or layer names it.")
                                } else {
                                    format!("Named by:\n{}", listed(&named))
                                });
                            } else {
                                if !named.is_empty() {
                                    parts.push(format!("Named already:\n{}", listed(&named)));
                                }
                                parts.extend(drawn_until_saved(
                                    &users,
                                    definition.at_vertices,
                                    &in_quotes(name),
                                ));
                            }
                        }
                    }
                }
            }
        }
        self.issues = parts.join("\n");
        self.issues_error = is_error;

        self.live = live;
        self.two_point = two_point;
        self.units_note = String::from(units.2);
        self.source_label = if self.source.is_empty() {
            String::from("Made in this session")
        } else {
            self.source.clone()
        };
        // Save has something to do: an edit, a definition not yet in the library, or one that
        // has gone from it.
        self.save_enabled =
            live && reading.definition.is_some() && !taken && (changed || !in_library);
        self.revert_enabled = live && changed;
        self.in_library = in_library;
        self.offered = offered;

        let mut heading = match &self.existing {
            Some(existing) => {
                let mut heading = format!(
                    "{} {}",
                    if symbol { "Symbol" } else { "Linestyle" },
                    in_quotes(existing)
                );
                if live && !in_library {
                    heading.push_str(" - not in the library");
                }
                heading
            }
            None => {
                let mut heading = format!("New {}", kind_word(symbol));
                if !self.origin.is_empty() {
                    heading.push_str(", ");
                    heading.push_str(&self.origin);
                }
                heading
            }
        };
        if changed {
            heading.push_str(" - edited, not saved");
        }
        self.heading = heading;
    }

    // ---- the frame -----------------------------------------------------------------------------

    /// Draws the editor for this frame, when it is shown - the drawing's changes since the last
    /// frame first, as the C++'s watcher delivered them at the event loop - and carries out what
    /// was pressed. True while it is shown.
    pub fn ui(&mut self, ctx: &Context, host: &mut dyn Host) -> bool {
        if let Some(changes) = self.watcher.deliver(host.document()) {
            self.on_document_changed(host.document(), changes);
        }
        if !self.open {
            return false;
        }
        let mut frame = std::mem::replace(&mut self.frame, DialogFrame::new(DIALOG, TITLE));
        frame.set_title(if self.dirty() {
            "Definition Editor *"
        } else {
            TITLE
        });
        let closed = show_dialog(&mut frame, ctx, self, host, Self::body, Self::footer);
        self.frame = frame;
        if std::mem::take(&mut self.edited_now) {
            let document = self.live_document(host.document());
            self.edited(document);
        }
        match self.pressed.take() {
            Some(Pressed::New) => {
                let symbol = self.form.symbol;
                self.new_definition(host, symbol);
            }
            Some(Pressed::Duplicate) => {
                if let Some(existing) = self.existing.clone() {
                    self.duplicate_definition(host, &existing);
                }
            }
            Some(Pressed::Delete) => {
                self.remove(host, false);
            }
            Some(Pressed::DeleteAnyway) => {
                self.remove(host, true);
            }
            Some(Pressed::DeleteCancel) => {
                self.remove_refused = false;
                self.said.clear();
                let document = self.live_document(host.document());
                self.refresh(document);
            }
            Some(Pressed::Revert) => self.revert(host),
            Some(Pressed::Save) => {
                self.save(host);
            }
            Some(Pressed::Close) => self.reject(host),
            None => {}
        }
        if closed {
            self.reject(host);
        }
        self.flush(host);
        self.open
    }

    fn line_field(
        &mut self,
        ui: &mut Ui,
        name: &str,
        tip: &str,
        placeholder: &str,
        value: fn(&mut Form) -> &mut String,
    ) {
        let response = controls::line_with(
            ui,
            name,
            value(&mut self.form),
            LineOptions {
                placeholder,
                ..LineOptions::default()
            },
        )
        .on_hover_text(tip);
        if response.changed() {
            self.edited_now = true;
        }
    }

    fn body(&mut self, ui: &mut Ui, _host: &mut dyn Host) {
        controls::label_as(
            ui,
            "definitionHeading",
            &self.heading,
            egui::Label::new(RichText::new(self.heading.as_str()).strong()),
        );
        let live = self.live;
        let anchors = self.live && self.two_point;
        ui.horizontal_top(|ui| {
            // Left: the form, and the strokes under it.
            ui.vertical(|ui| {
                egui::Grid::new("definitionForm")
                    .num_columns(2)
                    .show(ui, |ui| {
                        ui.label("Name");
                        ui.add_enabled_ui(live && self.existing.is_none(), |ui| {
                            self.line_field(
                                ui,
                                "definitionName",
                                "What styles and survey codes name it by. Fixed once the \
                                 definition exists: another name would be another definition, \
                                 and everything naming this one left naming nothing. Duplicate \
                                 makes a copy under a new name.",
                                "",
                                |form| &mut form.name,
                            );
                        });
                        ui.end_row();
                        ui.label("Group");
                        ui.add_enabled_ui(live, |ui| {
                            self.line_field(
                                ui,
                                "definitionGroup",
                                "Where a browser lists it: a path with / between its parts, \
                                 such as Survey/Water",
                                "",
                                |form| &mut form.group,
                            );
                        });
                        ui.end_row();
                        ui.label("Kind");
                        ui.add_enabled_ui(live, |ui| {
                            let kinds = [String::from("Linestyle"), String::from("Symbol")];
                            let mut kind = usize::from(self.form.symbol);
                            if index_choice(ui, "definitionKind", &kinds, &mut kind) {
                                self.form.symbol = kind == 1;
                                self.edited_now = true;
                            }
                        });
                        ui.end_row();
                        ui.label("Units");
                        ui.horizontal(|ui| {
                            ui.add_enabled_ui(live, |ui| {
                                let words: Vec<String> = UNITS_ROWS
                                    .iter()
                                    .map(|(_, word, _)| String::from(*word))
                                    .collect();
                                if index_choice(
                                    ui,
                                    "definitionUnits",
                                    &words,
                                    &mut self.form.units,
                                ) {
                                    self.edited_now = true;
                                }
                            });
                            controls::label(ui, "definitionUnitsNote", &self.units_note);
                        });
                        ui.end_row();
                        ui.label("");
                        ui.add_enabled_ui(live, |ui| {
                            if controls::check(
                                ui,
                                "definitionAtVertices",
                                "At vertices",
                                &mut self.form.at_vertices,
                            )
                            .on_hover_text(
                                "Drawn at each vertex of a string rather than along it; a line \
                                 whose linetype names it is then drawn plain",
                            )
                            .changed()
                            {
                                self.edited_now = true;
                            }
                        });
                        ui.end_row();
                        ui.label("Length");
                        ui.add_enabled_ui(live, |ui| {
                            self.line_field(
                                ui,
                                "definitionLength",
                                "One repeat of the pattern along a line, in the definition's \
                                 units. Left empty, or 0, the span of the strokes is used.",
                                "0",
                                |form| &mut form.length,
                            );
                        });
                        ui.end_row();
                        ui.label("Factor");
                        ui.add_enabled_ui(live, |ui| {
                            self.line_field(
                                ui,
                                "definitionFactor",
                                "A scale applied to every coordinate; above 0",
                                "1",
                                |form| &mut form.factor,
                            );
                        });
                        ui.end_row();
                        let origin_tip = "The point of the strokes' coordinates that is put on \
                                          the point drawn";
                        ui.label("Origin x, y");
                        ui.add_enabled_ui(live, |ui| {
                            ui.horizontal(|ui| {
                                self.line_field(ui, "definitionOriginX", origin_tip, "0", |form| {
                                    &mut form.origin_x
                                });
                                self.line_field(ui, "definitionOriginY", origin_tip, "0", |form| {
                                    &mut form.origin_y
                                });
                            });
                        });
                        ui.end_row();
                        let anchor_tip = "For the units twoPoint only: where the two points it \
                                          is drawn across sit in the strokes' coordinates";
                        ui.label("Anchor 1 x, y");
                        ui.add_enabled_ui(anchors, |ui| {
                            ui.horizontal(|ui| {
                                self.line_field(ui, "definitionAnchor1X", anchor_tip, "0", |form| {
                                    &mut form.anchor1_x
                                });
                                self.line_field(ui, "definitionAnchor1Y", anchor_tip, "0", |form| {
                                    &mut form.anchor1_y
                                });
                            });
                        });
                        ui.end_row();
                        ui.label("Anchor 2 x, y");
                        ui.add_enabled_ui(anchors, |ui| {
                            ui.horizontal(|ui| {
                                self.line_field(ui, "definitionAnchor2X", anchor_tip, "0", |form| {
                                    &mut form.anchor2_x
                                });
                                self.line_field(ui, "definitionAnchor2Y", anchor_tip, "0", |form| {
                                    &mut form.anchor2_y
                                });
                            });
                        });
                        ui.end_row();
                        ui.label("Stretch, cycle mode");
                        ui.add_enabled_ui(anchors, |ui| {
                            ui.horizontal(|ui| {
                                let mode_tip = "For the units twoPoint only: a whole number kept \
                                                as the customisation gives it";
                                if controls::whole(
                                    ui,
                                    "definitionStretchMode",
                                    &mut self.form.stretch_mode,
                                    MODE_RANGE,
                                )
                                .on_hover_text(mode_tip)
                                .changed()
                                {
                                    self.edited_now = true;
                                }
                                if controls::whole(
                                    ui,
                                    "definitionCycleMode",
                                    &mut self.form.cycle_mode,
                                    MODE_RANGE,
                                )
                                .on_hover_text(mode_tip)
                                .changed()
                                {
                                    self.edited_now = true;
                                }
                            });
                        });
                        ui.end_row();
                        ui.label("From");
                        controls::label(ui, "definitionSource", &self.source_label);
                        ui.end_row();
                    });
                controls::label_as(
                    ui,
                    "definitionStrokesHelp",
                    STROKES_HELP,
                    egui::Label::new(STROKES_HELP).wrap(),
                );
                ui.add_enabled_ui(live, |ui| {
                    let response = controls::text_with(
                        ui,
                        "definitionStrokes",
                        &mut self.form.strokes,
                        TextOptions {
                            read_only: false,
                            rows: 8,
                            monospace: true,
                        },
                    );
                    if response.changed() {
                        self.edited_now = true;
                    }
                    // The line that does not read, washed: by its place in the box.
                    if let Some(&line) = self.washed_lines().first() {
                        let row = ui.text_style_height(&egui::TextStyle::Monospace);
                        let top = response.rect.top() + 2.0 + row * (line - 1) as f32;
                        if top + row <= response.rect.bottom() {
                            let wash = egui::Rect::from_min_max(
                                egui::pos2(response.rect.left(), top),
                                egui::pos2(response.rect.right(), top + row),
                            );
                            ui.painter().rect_filled(
                                wash,
                                0.0,
                                palette::error().with_alpha(70).to_egui(),
                            );
                        }
                    }
                });
            });

            // Right: what it draws.
            ui.vertical(|ui| {
                ui.group(|ui| {
                    ui.label("As a symbol, on its insertion point");
                    self.preview
                        .ui(ui, "definitionPreview", egui::vec2(260.0, 170.0));
                });
                let line_box = |ui: &mut Ui, preview: &StylePreview| {
                    ui.group(|ui| {
                        ui.label("Along a line");
                        preview.ui(ui, "definitionLinePreview", egui::vec2(260.0, 130.0));
                    });
                };
                // A symbol has no pattern along a line: that pane goes.
                if self.form.symbol {
                    hidden(ui, "definitionLineBox", |ui| {
                        line_box(ui, &self.line_preview);
                    });
                } else {
                    line_box(ui, &self.line_preview);
                }
                // Said while the text does not read and the last picture that did stays up.
                controls::label_as(
                    ui,
                    "definitionPreviewNote",
                    &self.preview_note,
                    egui::Label::new(
                        RichText::new(self.preview_note.as_str())
                            .color(palette::error().to_egui()),
                    ),
                );
                ui.horizontal(|ui| {
                    ui.label("Plot scale");
                    let scales: Vec<String> = StylePreview::PLOT_SCALES
                        .iter()
                        .map(|denominator| format!("1:{denominator}"))
                        .collect();
                    if index_choice(ui, "definitionPlotScale", &scales, &mut self.plot_scale) {
                        let denominator = StylePreview::PLOT_SCALES
                            [self.plot_scale.min(StylePreview::PLOT_SCALES.len() - 1)];
                        self.preview.set_scale_denominator(denominator);
                        self.line_preview.set_scale_denominator(denominator);
                    }
                });
            });
        });
    }

    fn footer(&mut self, ui: &mut Ui, _host: &mut dyn Host) {
        // What is wrong, or what names it; then the buttons.
        let mut issues = self.issues.clone();
        let shown = controls::text_with(
            ui,
            "definitionIssues",
            &mut issues,
            TextOptions {
                read_only: true,
                rows: 5,
                monospace: false,
            },
        );
        if self.issues_error {
            ui.painter().rect_stroke(
                shown.rect,
                0.0,
                egui::Stroke::new(1.0, palette::error().to_egui()),
                egui::StrokeKind::Inside,
            );
        }

        // The two buttons THEMSELVES, not only the row they sit in: a button in a hidden row is
        // still a button a script can press, and this one deletes what something uses.
        let delete_panel = |ui: &mut Ui, pressed: &mut Option<Pressed>| {
            let row = ui.horizontal(|ui| {
                if controls::button(ui, "definitionDeleteAnyway", "Delete Anyway")
                    .on_hover_text(
                        "Delete it although it is used: what names it then names nothing",
                    )
                    .clicked()
                {
                    *pressed = Some(Pressed::DeleteAnyway);
                }
                if controls::button(ui, "definitionDeleteCancel", "Keep It").clicked() {
                    *pressed = Some(Pressed::DeleteCancel);
                }
            });
            custom(
                ui,
                &row.response,
                ControlRecord::new("definitionDeletePanel", ControlKind::Other),
            );
        };
        if self.offered {
            delete_panel(ui, &mut self.pressed);
        } else {
            hidden(ui, "definitionDeletePanel", |ui| {
                delete_panel(ui, &mut self.pressed);
            });
        }

        let live = self.live;
        let in_library = self.in_library;
        ui.horizontal(|ui| {
            ui.add_enabled_ui(live, |ui| {
                if controls::button(ui, "definitionNew", "New")
                    .on_hover_text("An empty definition of the kind shown")
                    .clicked()
                {
                    self.pressed = Some(Pressed::New);
                }
            });
            ui.add_enabled_ui(in_library, |ui| {
                if controls::button(ui, "definitionDuplicate", "Duplicate")
                    .on_hover_text("A copy of this definition under a new name")
                    .clicked()
                {
                    self.pressed = Some(Pressed::Duplicate);
                }
                if controls::button(ui, "definitionDelete", "Delete")
                    .on_hover_text(
                        "Take it out of the library. Refused, listing what uses it, while a \
                         survey code, a style or a layer names it",
                    )
                    .clicked()
                {
                    self.pressed = Some(Pressed::Delete);
                }
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if controls::button(ui, "definitionClose", "Close").clicked() {
                    self.pressed = Some(Pressed::Close);
                }
                ui.add_enabled_ui(self.save_enabled, |ui| {
                    if controls::button(ui, "definitionSave", "Save")
                        .on_hover_text(
                            "Put the definition into the session's library, in place of the one \
                             of its name",
                        )
                        .clicked()
                    {
                        self.pressed = Some(Pressed::Save);
                    }
                });
                ui.add_enabled_ui(self.revert_enabled, |ui| {
                    if controls::button(ui, "definitionRevert", "Revert")
                        .on_hover_text("Forget what was changed here")
                        .clicked()
                    {
                        self.pressed = Some(Pressed::Revert);
                    }
                });
            });
        });
    }
}
