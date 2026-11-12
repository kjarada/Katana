//! Survey > Subsurface Utilities (AS 5488): the window onto the UTILITY verb
//! (docs/subsurface_utilities.md). The port of survey/utility_dialog.
//!
//! The dialog is non-modal and runs nothing of the AS 5488 library itself. Each tab is one of the
//! verb's seven forms; the dialog writes the UTILITY line its fields describe - shown, as it will
//! run, in utilityCommand - and Run hands that line to the window's command executor exactly as if
//! it had been typed: it is echoed in the command log, kept in the history, and a DRAW or a REGRADE
//! is one undo step like any typed command. The reply comes back into utilityOutput. So there is
//! nothing the dialog can do that an agent cannot do by typing the same line, and the line the
//! dialog shows is the one to type.
//!
//! Where the services come from (docs/cad.md, "Scope and filter"): a schedule file, or what is
//! drawn, taken by the same "Apply to" and "Only those that match" controls as Global Modify (the
//! ONE ScopeFilterWidget, CLAUDE.md section 1.1), which give the line its scope and filter words
//! (SELECTION | DRAWING | VIEW <id> [EXTENTS] | LAYERS a,b [ONLY], then WHERE ...). For Draw, what is
//! drawn is the lines, polylines and points a survey or an import left there, drawn as services
//! with what the "Drawn geometry as services" fields say of them; for the others, the services
//! UTILITY DRAW drew. Draw, Report, Verify, Clearance and Check take either; Regrade and Schedule
//! always read the drawing. Clearance's proposed works are a design file, a line or polyline in the
//! drawing (#id, with a LEVEL) or one of the document's alignments.
//!
//! The line is made by `utility_command_line`, a pure function of what the fields hold, so it is
//! tested without a window: a path with blanks is quoted, an option left blank is left out (the
//! verb's default applies), and a file field left empty, a number that does not read or a scope
//! that cannot be said refuses the whole line, saying which field - nothing runs. Whether a named
//! file can be read is not the dialog's to say: the line runs, and the verb refuses it (NotFound,
//! by path).
//!
//! Every control has the C++ object name, which is how the tests (tests/utilities) and the headless
//! --dialog, --fill and --press switches drive it:
//!
//! ```text
//! utilityDialog          the dialog; every action of the menu section opens it (utilityDraw,
//!                        utilityReport, utilityVerify, utilityClearance, utilityCheck,
//!                        utilityRegrade, utilityWriteSchedule), each on its own tab
//! utilityTabs            Draw / Report / Verify / Clearance / Check / Regrade / Schedule
//! utilitySourceFile      the services come from the schedule file (a radio button each;
//! utilitySourceDrawing   --press chooses one)
//! utilitySchedule        the utility schedule (.csv): Draw's, and the other tabs' when the source
//!                        is the file
//! utilityScheduleBrowse  choose it with a file dialog
//! utilityScope           the drawing's scope and filter (ScopeFilterWidget, its controls named
//!                        utilityScopeSelection, utilityScopeView, utilityView, utilityOnScreen,
//!                        utilityScopeLayers, utilityLayers, utilitySublayers, utilityScopeDrawing,
//!                        utilityType<Point|Line|...>, utilityFilterLayer, utilityFilterStyle,
//!                        utilityFilterColour, utilityFilterProperty, utilityFilterValue,
//!                        utilityFilterText, utilityDrawnOnly); live when the source is the drawing
//! utilitySpacing         the longest detected segment that keeps QL-B, metres (SPACING; Draw,
//!                        Report and Regrade)
//! utilityMinCover        the minimum cover to flag, metres (MINCOVER; Draw, Report and Regrade)
//! utilityLayerPrefix     the top of the layers a Draw makes (LAYER)
//! utilityGeometryGroup   Draw's fields for geometry in the drawing, live when Draw reads what is
//!                        drawn; each left at "not given" or blank is the entity's own, else the
//!                        verb's default. The choices show the verb's own words, so --fill takes
//!                        them as typed:
//! utilityServiceType     TYPE: water, electricity, gas ...
//! utilityMethod          METHOD: EML, GPR, pothole, trench, surface ...
//! utilityHUnc, utilityVUnc  H_UNC, V_UNC: uncertainties, metres
//! utilityHeights         HEIGHTS: surface, service or none
//! utilityLevelRef        LEVEL_REF: top, centre or invert
//! utilityPath            PATH: detected, exposed or assumed
//! utilityOwner, utilityMaterial  OWNER, MATERIAL
//! utilityDiameter        DIAMETER_MM: millimetres
//! utilityServiceStatus   STATUS: in service, disused, abandoned, proposed
//! utilityFields          FIELDS: column=property, ... - an import's own attributes read as the
//!                        schedule's columns
//! utilityDesignFile      Clearance's works are a design file (radio) ...
//! utilityDesign          ... this one (.csv)
//! utilityDesignBrowse    choose it with a file dialog
//! utilityDesignEntity    ... or a line or polyline in the drawing (radio) ...
//! utilityDesignEntityId  ... this one, "#12" or "12"
//! utilityDesignUseSelected  the one selected entity's id into it
//! utilityDesignLevel     ... at this level, metres (LEVEL; else its own)
//! utilityDesignAlignment ... or one of the document's alignments (radio) ...
//! utilityDesignAlignmentName  ... this one (a choice of their names)
//! utilityWidth           the works' width, metres (WIDTH)
//! utilityH, utilityV     the clearances the owners require, metres (H, V)
//! utilityMargin          the margin around a QL-C or QL-D service (MARGIN)
//! utilitySchema          the delivery schema (.csv) for Check
//! utilitySchemaBrowse    choose it with a file dialog
//! utilityScheduleOut     the schedule Schedule writes (.csv)
//! utilityScheduleOutBrowse  choose it with a save dialog
//! utilityScheduleSchema  a delivery schema whose words it is written in (optional; SCHEMA)
//! utilityScheduleSchemaBrowse  choose it with a file dialog
//! utilityCommand         the exact line Run will run (read-only)
//! utilityRun             run it
//! utilityOutput          the reply: the report, the drawn records, or why the line failed
//!                        (read-only, monospace)
//! utilityCopy            copy the reply
//! utilitySave            save the reply to a file
//! utilityStatus          what happened last
//! utilityClose           close
//! ```
//!
//! What is the C++'s and differs here. A QTabWidget keeps every page's controls alive; an egui tab
//! draws only the page in front, so a control of another tab is reached by bringing its tab
//! forward first (`show_tool`, or a fill of utilityTabs), as a person does. A read-only field takes
//! no edit: what a fill writes there is drawn over by the dialog's own line at the next frame. The
//! dialog is drawn with the window as its `Host`: the drawing, whether the session is headless and
//! the clipboard are the host's; the C++ context's `document` pointer is `reads_drawing` (a test's
//! dialog may have no drawing, and its lists are then empty).

use std::path::Path;

use egui::{Align, Layout, RichText, Ui};
use katana_cad::utilities::utility_drawing::utility_type_word;
use katana_core::error::make_error;
use katana_core::text::{format_general, parse_finite_double, parse_integer};
use katana_core::{Error, ErrorCode, Result, Status};
use katana_survey::subsurface::{ClearanceRequirement, GradingSettings, UtilityType};

use crate::customisation::scope_filter_widget::ViewsFn;
use crate::customisation::{DocumentWatcher, ScopeChoice, ScopeFilterView, ScopeFilterWidget};
use crate::drive::controls::{self, Choice, LineOptions, TextOptions};
use crate::drive::registry::{ControlKind, ControlRecord};
use crate::host::Host;
use crate::ifc_dialogs::{get_open_file_name, get_save_file_name, to_native_separators};
use crate::shell::DialogFrame;
use crate::theme;

// ---- the line ----------------------------------------------------------------------------------

/// The verb's forms, in the order of the dialog's tabs and of the menu.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum UtilityTool {
    #[default]
    Draw,
    Report,
    Verify,
    Clearance,
    Check,
    Regrade,
    Schedule,
}

/// kUtilityToolCount.
pub const UTILITY_TOOL_COUNT: usize = 7;

/// Every tool, in the order of the tabs.
pub const UTILITY_TOOLS: [UtilityTool; UTILITY_TOOL_COUNT] = [
    UtilityTool::Draw,
    UtilityTool::Report,
    UtilityTool::Verify,
    UtilityTool::Clearance,
    UtilityTool::Check,
    UtilityTool::Regrade,
    UtilityTool::Schedule,
];

/// The tabs' titles, in the order of the tools.
const TAB_TITLES: [&str; UTILITY_TOOL_COUNT] = [
    "Draw",
    "Report",
    "Verify",
    "Clearance",
    "Check",
    "Regrade",
    "Schedule",
];

impl UtilityTool {
    /// Its tab's place (the C++'s `static_cast<int>(tool)`).
    pub fn index(self) -> usize {
        UTILITY_TOOLS
            .iter()
            .position(|tool| *tool == self)
            .unwrap_or(0)
    }

    /// The tool of the tab at `index`; Draw for one past the last.
    pub fn from_index(index: usize) -> Self {
        UTILITY_TOOLS
            .get(index)
            .copied()
            .unwrap_or(UtilityTool::Draw)
    }

    /// Its tab's title.
    pub fn title(self) -> &'static str {
        TAB_TITLES[self.index()]
    }
}

/// The verb's word for a tool: "DRAW", "REPORT" ...
pub fn utility_verb_word(tool: UtilityTool) -> &'static str {
    match tool {
        UtilityTool::Draw => "DRAW",
        UtilityTool::Report => "REPORT",
        UtilityTool::Verify => "VERIFY",
        UtilityTool::Clearance => "CLEARANCE",
        UtilityTool::Check => "CHECK",
        UtilityTool::Regrade => "REGRADE",
        UtilityTool::Schedule => "SCHEDULE",
    }
}

/// Where the services a tool reads come from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum UtilitySource {
    /// The schedule file.
    #[default]
    File,
    /// What UTILITY DRAW drew, by the scope and filter words.
    Drawing,
}

/// Where Clearance's proposed works come from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum UtilityDesignSource {
    /// A design .csv.
    #[default]
    File,
    /// A line or polyline in the drawing, #id.
    Entity,
    /// One of the document's alignments, by name.
    Alignment,
}

/// What the dialog's fields hold, as typed. Only the fields of `tool` are read; a blank option is
/// left out of the line.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UtilityForm {
    pub tool: UtilityTool,
    /// Draw, Report, Verify, Clearance and Check read either; Regrade and Schedule always the
    /// drawing (utility_source_of).
    pub source: UtilitySource,
    pub schedule: String,
    /// The drawing's scope and filter words, as ScopeFilterWidget::verb_words gives them, or - in
    /// `scope_error` - why the controls say none.
    pub scope: String,
    pub scope_error: String,
    pub design_source: UtilityDesignSource,
    pub design: String,
    pub design_entity: String,
    pub design_level: String,
    pub alignment: String,
    pub schema: String,
    pub schedule_out: String,
    pub schedule_schema: String,
    pub min_cover: String,
    pub spacing: String,
    pub width: String,
    pub horizontal: String,
    pub vertical: String,
    pub margin: String,
    pub layer_prefix: String,
    // Draw's, when it reads what is drawn (UTILITY DRAW <scope>'s options): each the verb's own
    // word - "water", "EML", "service" - or blank, left out, for what each entity says of itself.
    pub service_type: String,
    pub method: String,
    pub horizontal_uncertainty: String,
    pub vertical_uncertainty: String,
    pub heights: String,
    pub level_reference: String,
    pub path: String,
    pub owner: String,
    pub material: String,
    /// Millimetres, as DIAMETER_MM takes it.
    pub diameter: String,
    pub status: String,
    pub fields: String,
}

/// Where `form`'s tool reads its services from: the drawing for Regrade and Schedule, else the
/// form's choice.
pub fn utility_source_of(form: &UtilityForm) -> UtilitySource {
    match form.tool {
        UtilityTool::Regrade | UtilityTool::Schedule => UtilitySource::Drawing,
        UtilityTool::Draw
        | UtilityTool::Report
        | UtilityTool::Verify
        | UtilityTool::Clearance
        | UtilityTool::Check => form.source,
    }
}

/// What the dialog refuses to write, said in its words.
fn invalid(message: String) -> Error {
    Error::new(ErrorCode::InvalidArgument, message)
}

/// A default of the library's own, as a placeholder shows it: 10, 0.3, 2 (QString::number(value,
/// 'g', 12)).
fn default_of(value: f64) -> String {
    format!("default {}", format_general(value, 12))
}

/// A word as the command line reads it: double-quoted when it holds a blank, which the
/// interpreter's tokenizer would otherwise split it at.
fn word(text: &str, field: &str) -> Result<String> {
    if text.contains('"') {
        return Err(invalid(format!(
            "{field} holds a double quote, which a command line cannot carry"
        )));
    }
    // QChar::isSpace's set is Unicode's White_Space, as char::is_whitespace's is.
    let blank = text.chars().any(char::is_whitespace);
    Ok(if blank {
        format!("\"{text}\"")
    } else {
        String::from(text)
    })
}

/// A file the tool needs, trimmed; InvalidArgument naming it when blank.
fn required_file(text: &str, field: &str) -> Result<String> {
    let path = text.trim();
    if path.is_empty() {
        return Err(invalid(format!("choose the {field} first")));
    }
    word(path, &format!("the {field} path"))
}

/// `part` after `prefix`, or its error.
fn prefixed(prefix: &str, part: Result<String>) -> Result<String> {
    part.map(|part| format!("{prefix}{part}"))
}

/// " KEYWORD <number>" for an option that was given, nothing for a blank one. The number goes on
/// the line as typed, once it reads as one.
fn option_in(keyword: &str, text: &str, field: &str, unit: &str) -> Result<String> {
    let number = text.trim();
    if number.is_empty() {
        return Ok(String::new());
    }
    if parse_finite_double(number).is_none() {
        return Err(invalid(format!(
            "{field} must be a number of {unit}, not '{number}'"
        )));
    }
    Ok(format!(" {keyword} {number}"))
}

/// `option_in` in metres, the C++'s default unit.
fn option(keyword: &str, text: &str, field: &str) -> Result<String> {
    option_in(keyword, text, field, "metres")
}

/// " KEYWORD <word>" for a word that was given, quoted when it holds a blank; nothing for a blank
/// one.
fn word_option(keyword: &str, text: &str, field: &str) -> Result<String> {
    let given = text.trim();
    if given.is_empty() {
        return Ok(String::new());
    }
    prefixed(&format!(" {keyword} "), word(given, field))
}

/// The drawing's scope and filter words, or why the controls say none.
fn scope_words(form: &UtilityForm) -> Result<String> {
    if !form.scope_error.is_empty() {
        return Err(invalid(format!(
            "the drawing's scope: {}",
            form.scope_error
        )));
    }
    let words = form.scope.trim();
    if words.is_empty() {
        return Err(invalid(String::from(
            "choose what in the drawing to act on first",
        )));
    }
    Ok(String::from(words))
}

/// Where the services come from: the schedule's path, or the scope words.
fn source_words(form: &UtilityForm) -> Result<String> {
    if utility_source_of(form) == UtilitySource::Drawing {
        scope_words(form)
    } else {
        required_file(&form.schedule, "utility schedule")
    }
}

/// "#12" for "#12" or "12": an entity id is a whole number from 1.
fn entity_word(text: &str) -> Result<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(invalid(String::from(
            "choose the line or polyline of the proposed works first: its #id, or Use Selected",
        )));
    }
    let id = trimmed.strip_prefix('#').unwrap_or(trimmed);
    let number = parse_integer(id);
    let digits = !id.is_empty() && id.chars().all(|character| character.is_ascii_digit());
    if !digits || number.is_none_or(|number| number < 1) {
        return Err(invalid(format!(
            "the design entity must be #<entity id>, not '{trimmed}'"
        )));
    }
    Ok(format!("#{id}"))
}

/// Clearance's works, after the services: the positional design file of a schedule file (the form
/// the line always had), else DESIGN and the works.
fn design_words(form: &UtilityForm) -> Result<String> {
    match form.design_source {
        UtilityDesignSource::File => {
            let path = required_file(&form.design, "design of the proposed works")?;
            Ok(if utility_source_of(form) == UtilitySource::File {
                format!(" {path}")
            } else {
                format!(" DESIGN {path}")
            })
        }
        UtilityDesignSource::Entity => {
            let id = entity_word(&form.design_entity)?;
            let level = option("LEVEL", &form.design_level, "The design level")?;
            Ok(format!(" DESIGN {id}{level}"))
        }
        UtilityDesignSource::Alignment => {
            let name = form.alignment.trim();
            if name.is_empty() {
                return Err(invalid(String::from(
                    "choose the alignment of the proposed works first",
                )));
            }
            prefixed(" DESIGN ALIGNMENT ", word(name, "The alignment's name"))
        }
    }
}

/// The UTILITY line `form` describes, exactly as it would be typed, where <source> is the
/// schedule's path or the scope words:
///
/// ```text
/// UTILITY DRAW <schedule> [SPACING <m>] [MINCOVER <m>] [LAYER <prefix>]
/// UTILITY DRAW <scope> [TYPE <type>] [METHOD <method>] [H_UNC <m>] [V_UNC <m>]
///              [HEIGHTS <h>] [LEVEL_REF <r>] [PATH <p>] [OWNER <text>]
///              [MATERIAL <text>] [DIAMETER_MM <mm>] [STATUS <s>] [FIELDS <pairs>]
///              [SPACING <m>] [MINCOVER <m>] [LAYER <prefix>]
/// UTILITY REPORT <source> [MINCOVER <m>] [SPACING <m>]
/// UTILITY VERIFY <source>
/// UTILITY CLEARANCE <schedule> <design> [WIDTH <m>] [H <m>] [V <m>] [MARGIN <m>]
/// UTILITY CLEARANCE <source> DESIGN <design> | #<id> [LEVEL <z>] | ALIGNMENT <name>
///                   [WIDTH <m>] [H <m>] [V <m>] [MARGIN <m>]
/// UTILITY CHECK <source> SCHEMA <schema>
/// UTILITY REGRADE <scope> [SPACING <m>] [MINCOVER <m>]
/// UTILITY SCHEDULE <out.csv> <scope> [SCHEMA <schema>]
/// ```
///
/// A schedule file measured against a design file keeps the positional form it always had, so a
/// line the dialog wrote before is the line it writes now; every other design follows DESIGN,
/// which also ends a WHERE filter plainly. A word holding a blank is double-quoted, as the command
/// line reads it. InvalidArgument, naming the field, for a schedule, design, entity, alignment,
/// schema or output left empty, for a scope the controls cannot say (`scope_error`), for a number
/// that is not one (a comma is not a decimal point here, as it is not on the command line), for an
/// entity that is not #<whole number from 1>, and for a double quote in any word - the command line
/// has no way to write one inside a quoted word. A negative number is written as typed: the verb
/// says what it accepts.
pub fn utility_command_line(form: &UtilityForm) -> Result<String> {
    let mut line = format!("UTILITY {}", utility_verb_word(form.tool));
    // SCHEDULE names what it writes before the scope it writes.
    if form.tool == UtilityTool::Schedule {
        let out = required_file(&form.schedule_out, "schedule to write")?;
        line.push(' ');
        line.push_str(&out);
    }
    let source = source_words(form)?;
    line.push(' ');
    line.push_str(&source);
    // The rest in the order the verb lists it, an option only when given; the first part that
    // cannot be written refuses the whole line.
    let mut parts: Vec<Result<String>> = Vec::new();
    match form.tool {
        UtilityTool::Draw => {
            // What the geometry in the drawing cannot say for itself; a schedule says it in its
            // columns.
            if utility_source_of(form) == UtilitySource::Drawing {
                parts.push(word_option(
                    "TYPE",
                    &form.service_type,
                    "The type of service",
                ));
                parts.push(word_option("METHOD", &form.method, "The location method"));
                parts.push(option(
                    "H_UNC",
                    &form.horizontal_uncertainty,
                    "The horizontal uncertainty",
                ));
                parts.push(option(
                    "V_UNC",
                    &form.vertical_uncertainty,
                    "The vertical uncertainty",
                ));
                parts.push(word_option(
                    "HEIGHTS",
                    &form.heights,
                    "What the heights are",
                ));
                parts.push(word_option(
                    "LEVEL_REF",
                    &form.level_reference,
                    "The level reference",
                ));
                parts.push(word_option("PATH", &form.path, "The path between vertices"));
                parts.push(word_option("OWNER", &form.owner, "The owner"));
                parts.push(word_option("MATERIAL", &form.material, "The material"));
                parts.push(option_in(
                    "DIAMETER_MM",
                    &form.diameter,
                    "The diameter",
                    "millimetres",
                ));
                parts.push(word_option("STATUS", &form.status, "The status"));
                parts.push(word_option("FIELDS", &form.fields, "The fields"));
            }
            parts.push(option("SPACING", &form.spacing, "The detected spacing"));
            parts.push(option("MINCOVER", &form.min_cover, "The minimum cover"));
            let prefix = form.layer_prefix.trim();
            if !prefix.is_empty() {
                parts.push(prefixed(" LAYER ", word(prefix, "The layer prefix")));
            }
        }
        UtilityTool::Report => {
            parts.push(option("MINCOVER", &form.min_cover, "The minimum cover"));
            parts.push(option("SPACING", &form.spacing, "The detected spacing"));
        }
        UtilityTool::Verify => {}
        UtilityTool::Clearance => {
            parts.push(design_words(form));
            parts.push(option("WIDTH", &form.width, "The works' width"));
            parts.push(option("H", &form.horizontal, "The horizontal clearance"));
            parts.push(option("V", &form.vertical, "The vertical clearance"));
            parts.push(option("MARGIN", &form.margin, "The unverified margin"));
        }
        UtilityTool::Check => {
            parts.push(prefixed(
                " SCHEMA ",
                required_file(&form.schema, "delivery schema"),
            ));
        }
        UtilityTool::Regrade => {
            parts.push(option("SPACING", &form.spacing, "The detected spacing"));
            parts.push(option("MINCOVER", &form.min_cover, "The minimum cover"));
        }
        UtilityTool::Schedule => {
            if !form.schedule_schema.trim().is_empty() {
                parts.push(prefixed(
                    " SCHEMA ",
                    required_file(&form.schedule_schema, "delivery schema"),
                ));
            }
        }
    }
    for part in parts {
        line.push_str(&part?);
    }
    Ok(line)
}

// ---- the dialog --------------------------------------------------------------------------------

/// A scope view of the window's, as the drawing's View scope offers it.
pub type UtilityScopeView = ScopeFilterView;

/// Runs a line as the command line runs a typed one, and returns its reply: the text it logged, or
/// why it failed. Lent the window for the call.
pub type UtilityExecute = Box<dyn FnMut(&mut dyn Host, &str) -> Result<String>>;

/// What the dialog asks of whoever shows it; the workbench supplies these, a test supplies its
/// own. The rest of what the C++ context held - whether the session is headless, the drawing - is
/// the host's.
#[derive(Default)]
pub struct UtilityDialogContext {
    /// What Run hands its line to; None runs nothing ("nothing here can run a command").
    pub execute: Option<UtilityExecute>,
    /// Whether the dialog reads the host's drawing - the scope's layer list, the alignments and Use
    /// Selected - and follows it as it changes (the C++'s `document`, which may be null in a
    /// test's: the lists are then empty).
    pub reads_drawing: bool,
    /// The workspace's open views, for the scope's View choice. None: the View choice then names no
    /// view a line can carry.
    pub views: Option<ViewsFn>,
}

/// A choice whose items are the verb's own words after "not given": its current data is the word,
/// or blank for "not given".
struct WordChoice {
    items: Vec<String>,
    /// What each item is, beside it ("" for none).
    explanations: Vec<&'static str>,
    chosen: Choice,
}

impl WordChoice {
    fn new(words: &[(&str, &'static str)]) -> Self {
        let mut items = vec![String::from("not given")];
        let mut explanations = vec![""];
        for &(word, explanation) in words {
            items.push(String::from(word));
            explanations.push(explanation);
        }
        // A QComboBox chooses the first item added.
        Self {
            items,
            explanations,
            chosen: Choice::at(0),
        }
    }

    /// QComboBox::currentData().toString(): the word, blank for "not given" or none.
    fn word(&self) -> String {
        match self.chosen.index {
            Some(at) if at > 0 => self.items.get(at).cloned().unwrap_or_default(),
            _ => String::new(),
        }
    }

    /// The choice's tip, and what the item chosen is, as the item's own tip said it.
    fn tip(&self, tip: &str) -> String {
        match self
            .chosen
            .index
            .and_then(|at| self.explanations.get(at))
            .filter(|explanation| !explanation.is_empty())
        {
            Some(explanation) => format!("{tip}\n{}: {explanation}", self.word()),
            None => String::from(tip),
        }
    }
}

/// The path fields a Browse button fills.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PathField {
    Schedule,
    Design,
    Schema,
    ScheduleOut,
    ScheduleSchema,
}

impl PathField {
    /// Its field's object name.
    fn name(self) -> &'static str {
        match self {
            PathField::Schedule => "utilitySchedule",
            PathField::Design => "utilityDesign",
            PathField::Schema => "utilitySchema",
            PathField::ScheduleOut => "utilityScheduleOut",
            PathField::ScheduleSchema => "utilityScheduleSchema",
        }
    }
}

const CSV_FILTER: &str = "CSV files (*.csv);;All files (*)";

/// The body's width: the C++ dialog's 1120 less its frame.
const BODY_WIDTH: f32 = 1090.0;

/// The scope and filter column's width, two fifths of the body beside the tabs' three (the C++
/// layout's stretch of 2 to 3).
const SCOPE_WIDTH: f32 = 420.0;

/// Survey > Subsurface Utilities (see the module comment).
pub struct UtilityToolsDialog {
    // First, so it goes first: no delivery reaches a half-dropped dialog.
    watcher: Option<DocumentWatcher>,
    frame: DialogFrame,
    visible: bool,
    execute: Option<UtilityExecute>,
    reads_drawing: bool,
    /// The tool whose reply utilityOutput shows; None before the first run.
    shown: Option<UtilityTool>,
    /// The tab in front.
    tool: UtilityTool,
    source: UtilitySource,
    schedule: String,
    scope: ScopeFilterWidget,
    spacing: String,
    layer_prefix: String,
    min_cover: String,
    service_type: WordChoice,
    method: WordChoice,
    horizontal_uncertainty: String,
    vertical_uncertainty: String,
    heights: WordChoice,
    level_reference: WordChoice,
    path: WordChoice,
    owner: String,
    material: String,
    diameter: String,
    service_status: WordChoice,
    fields: String,
    design_source: UtilityDesignSource,
    design: String,
    design_entity: String,
    design_level: String,
    alignments: Vec<String>,
    alignment: Choice,
    width: String,
    horizontal: String,
    vertical: String,
    margin: String,
    schema: String,
    schedule_out: String,
    schedule_schema: String,
    output: String,
    status: String,
    status_error: bool,
}

/// The dialog and the window, lent together to the dialog frame's body and footer.
struct Lent<'a, 'h> {
    dialog: &'a mut UtilityToolsDialog,
    host: &'a mut (dyn Host + 'h),
}

impl UtilityToolsDialog {
    /// The dialog over `context`, its lists read from `host`'s drawing and the views: shown on the
    /// Draw tab, the services from a schedule file, the scope the whole drawing.
    pub fn new(context: UtilityDialogContext, host: &dyn Host) -> Self {
        let mut scope = ScopeFilterWidget::new("utility");
        scope.views = context.views;
        // What the utility tools usually act on: everything drawn.
        scope.set_choice(ScopeChoice::Drawing);
        // The layers, views and alignments follow the drawing: a DRAW adds layers, an undo takes
        // them away.
        let watcher = context
            .reads_drawing
            .then(|| DocumentWatcher::new(host.document()));
        let service_words: Vec<(&str, &'static str)> = [
            UtilityType::Water,
            UtilityType::Electricity,
            UtilityType::Telecommunications,
            UtilityType::Gas,
            UtilityType::RecycledWater,
            UtilityType::FireService,
            UtilityType::Sewer,
            UtilityType::Stormwater,
            UtilityType::Fuel,
            UtilityType::IntelligentTransport,
            UtilityType::Other,
        ]
        .into_iter()
        .map(|kind| (utility_type_word(kind), ""))
        .collect();
        let mut dialog = Self {
            watcher,
            frame: DialogFrame::new("utilityDialog", "Subsurface Utilities (AS 5488)"),
            visible: false,
            execute: context.execute,
            reads_drawing: context.reads_drawing,
            shown: None,
            tool: UtilityTool::Draw,
            source: UtilitySource::File,
            schedule: String::new(),
            scope,
            spacing: String::new(),
            layer_prefix: String::new(),
            min_cover: String::new(),
            service_type: WordChoice::new(&service_words),
            method: WordChoice::new(&[
                ("EML", "electromagnetic location: QL-B at best"),
                ("GPR", "ground penetrating radar: QL-B at best"),
                ("geophysical", "other geophysics: QL-B at best"),
                (
                    "pothole",
                    "non-destructive excavation, the service seen: QL-A at best",
                ),
                ("trench", "an open trench, the service seen: QL-A at best"),
                ("surface", "a surveyed pit, valve or marker: QL-C at best"),
                ("records", "plans and GIS records: QL-D"),
                ("anecdotal", "site knowledge: QL-D"),
            ]),
            horizontal_uncertainty: String::new(),
            vertical_uncertainty: String::new(),
            heights: WordChoice::new(&[
                (
                    "surface",
                    "the ground over the service: marks shot on the surface",
                ),
                (
                    "service",
                    "the service itself, on its level reference: shot on the pipe",
                ),
                ("none", "not to be used"),
            ]),
            level_reference: WordChoice::new(&[
                ("top", "the crown, or what is met first above it"),
                ("centre", "the centre line"),
                ("invert", "the bottom of the bore"),
            ]),
            path: WordChoice::new(&[
                ("detected", "traced, or detected at intervals"),
                ("exposed", "seen all along: an open trench"),
                ("assumed", "joined up; nothing observed between"),
            ]),
            owner: String::new(),
            material: String::new(),
            diameter: String::new(),
            service_status: WordChoice::new(&[
                ("in service", ""),
                ("disused", ""),
                ("abandoned", ""),
                ("proposed", ""),
            ]),
            fields: String::new(),
            design_source: UtilityDesignSource::File,
            design: String::new(),
            design_entity: String::new(),
            design_level: String::new(),
            alignments: Vec::new(),
            alignment: Choice::default(),
            width: String::new(),
            horizontal: String::new(),
            vertical: String::new(),
            margin: String::new(),
            schema: String::new(),
            schedule_out: String::new(),
            schedule_schema: String::new(),
            output: String::new(),
            status: String::new(),
            status_error: false,
        };
        dialog.reload(host);
        dialog
    }

    /// QDialog::isModal: never - kept open beside the drawing, which a Draw changes.
    pub fn is_modal(&self) -> bool {
        self.frame.is_modal()
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Shows the dialog; one that was hidden reads the views again, which open now, not when the
    /// dialog was made (its showEvent).
    pub fn show(&mut self, host: &dyn Host) {
        if !self.visible {
            self.visible = true;
            self.reload(host);
        }
    }

    pub fn hide(&mut self) {
        self.visible = false;
    }

    fn document_alive(&self) -> bool {
        self.reads_drawing
            && self
                .watcher
                .as_ref()
                .is_some_and(DocumentWatcher::document_alive)
    }

    /// Refills what comes from the drawing and the window - the scope's layers and views, the
    /// alignments - keeping what is chosen. Done when the dialog is shown and when the drawing
    /// changes.
    pub fn reload(&mut self, host: &dyn Host) {
        if !self.document_alive() {
            // The views are the window's, drawing or none.
            self.scope.reload_views();
            return;
        }
        let model = host.document().model();
        self.scope.reload(model);
        let kept = self.alignment.current_text(&self.alignments);
        self.alignments = model.alignments.names();
        // The one chosen, else the first: a choice with a placeholder is left on none when it is
        // filled, and "none" is no alignment.
        let index = self.alignments.iter().position(|name| *name == kept).or(
            if self.alignments.is_empty() {
                None
            } else {
                Some(0)
            },
        );
        self.alignment = Choice {
            index,
            ..Choice::default()
        };
    }

    /// Brings `tool`'s tab to the front.
    pub fn show_tool(&mut self, tool: UtilityTool) {
        self.tool = tool;
    }

    pub fn tool(&self) -> UtilityTool {
        self.tool
    }

    /// The source chosen above the tabs (what Report, Verify, Clearance and Check read).
    pub fn set_source(&mut self, source: UtilitySource) {
        self.source = source;
    }

    /// The drawing's scope and filter controls.
    pub fn scope_controls(&mut self) -> &mut ScopeFilterWidget {
        &mut self.scope
    }

    /// The fields as they are now.
    pub fn form(&self) -> UtilityForm {
        let (scope, scope_error) = match self.scope.verb_words() {
            Ok(words) => (words, String::new()),
            Err(error) => (String::new(), error.message),
        };
        UtilityForm {
            tool: self.tool,
            source: self.source,
            schedule: self.schedule.clone(),
            scope,
            scope_error,
            design_source: self.design_source,
            design: self.design.clone(),
            design_entity: self.design_entity.clone(),
            design_level: self.design_level.clone(),
            alignment: self.alignment.current_text(&self.alignments),
            schema: self.schema.clone(),
            schedule_out: self.schedule_out.clone(),
            schedule_schema: self.schedule_schema.clone(),
            min_cover: self.min_cover.clone(),
            spacing: self.spacing.clone(),
            width: self.width.clone(),
            horizontal: self.horizontal.clone(),
            vertical: self.vertical.clone(),
            margin: self.margin.clone(),
            layer_prefix: self.layer_prefix.clone(),
            service_type: self.service_type.word(),
            method: self.method.word(),
            horizontal_uncertainty: self.horizontal_uncertainty.clone(),
            vertical_uncertainty: self.vertical_uncertainty.clone(),
            heights: self.heights.word(),
            level_reference: self.level_reference.word(),
            path: self.path.word(),
            owner: self.owner.clone(),
            material: self.material.clone(),
            diameter: self.diameter.clone(),
            status: self.service_status.word(),
            fields: self.fields.clone(),
        }
    }

    /// The line Run would run, or why there is none.
    pub fn command(&self) -> Result<String> {
        utility_command_line(&self.form())
    }

    /// What Run does: the line handed to the executor and its reply shown; a line that cannot be
    /// written runs nothing and the status says why.
    pub fn run(&mut self, host: &mut dyn Host) {
        let line = match self.command() {
            Ok(line) => line,
            Err(error) => {
                self.set_status(&error.message, true);
                return;
            }
        };
        let running = self.tool;
        let reply = match self.execute.as_mut() {
            Some(execute) => execute(host, &line),
            None => {
                self.set_status("nothing here can run a command", true);
                return;
            }
        };
        self.shown = Some(running);
        let reply = match reply {
            Ok(reply) => reply,
            Err(error) => {
                // The whole of it below - a schema check with errors is a whole report - and its
                // first line beside Run.
                self.show_output(&error.describe());
                let first = error.message.split('\n').next().unwrap_or("");
                self.set_status(first, true);
                return;
            }
        };
        self.show_output(&reply);
        // Said from the reply's own first record, never from the tab alone: a scope that takes no
        // utility line answers with the scope's record, and then nothing was regraded or written,
        // and no step is there to undo.
        let leads = |record: &str| reply.starts_with(record);
        match running {
            UtilityTool::Draw => {
                if leads("utilities drawn ") {
                    self.set_status(
                        "Drawn as one undo step - Undo removes it all. The records are below and \
                         in the command log.",
                        false,
                    );
                } else {
                    self.set_status(
                        "Nothing in the scope to draw: what it took is drawn already or cannot be \
                         a service. Nothing was added to the undo history.",
                        false,
                    );
                }
            }
            UtilityTool::Regrade => {
                if leads("utilities regraded changed=0 ") {
                    self.set_status(
                        "Nothing needed regrading: the drawing is as it was, and nothing was added \
                         to the undo history.",
                        false,
                    );
                } else if leads("utilities regraded ") {
                    self.set_status(
                        "Regraded as one undo step - Undo puts it back. The records are below and \
                         in the command log.",
                        false,
                    );
                } else {
                    self.set_status(
                        "Nothing in the scope is a utility line: nothing was regraded, and nothing \
                         was added to the undo history.",
                        false,
                    );
                }
            }
            UtilityTool::Schedule => {
                let said = if leads("utilities scheduled ") {
                    "Written. What was written is below and in the command log."
                } else {
                    "Nothing in the scope is a utility line: nothing was written."
                };
                self.set_status(said, false);
            }
            UtilityTool::Report
            | UtilityTool::Verify
            | UtilityTool::Clearance
            | UtilityTool::Check => {
                self.set_status("Done. The report is below and in the command log.", false);
            }
        }
    }

    /// What Use Selected does: the one selected entity's id into utilityDesignEntityId, with the
    /// design source set to it. False, the status saying why, when not exactly one entity is
    /// selected.
    pub fn use_selected_design(&mut self, host: &dyn Host) -> bool {
        if !self.document_alive() {
            self.set_status(
                "There is no drawing here to select the proposed works in.",
                true,
            );
            return false;
        }
        let selected = host.document().selection().ids();
        if selected.len() != 1 {
            let said = if selected.is_empty() {
                String::from("Select the line or polyline of the proposed works first.")
            } else {
                format!(
                    "Select just the line or polyline of the proposed works: {} entities are \
                     selected.",
                    selected.len()
                )
            };
            self.set_status(&said, true);
            return false;
        }
        let id = selected[0];
        self.design_entity = format!("#{id}");
        self.design_source = UtilityDesignSource::Entity;
        self.set_status(
            &format!("The proposed works: #{id}, the entity selected."),
            false,
        );
        true
    }

    fn show_output(&mut self, text: &str) {
        self.output = String::from(text.trim_end_matches('\n'));
    }

    /// The reply shown in utilityOutput.
    pub fn output(&self) -> &str {
        &self.output
    }

    /// What utilityStatus says, and whether it says it as an error.
    pub fn status(&self) -> &str {
        &self.status
    }

    pub fn status_is_error(&self) -> bool {
        self.status_error
    }

    pub fn set_status(&mut self, text: &str, is_error: bool) {
        self.status = String::from(text);
        self.status_error = is_error;
    }

    /// The file name Save As offers, "utility_draw.txt": named for the tool whose reply
    /// utilityOutput shows, which stays while another tab is brought forward to prepare the next
    /// run. The tab in front before anything has run.
    pub fn suggested_file_name(&self) -> String {
        let named = self.shown.unwrap_or(self.tool);
        format!(
            "utility_{}.txt",
            utility_verb_word(named).to_ascii_lowercase()
        )
    }

    /// Writes the reply to `path` as UTF-8 text: what Save As does once a file is chosen.
    pub fn save_output_to(&self, path: &Path) -> Status {
        let bytes = format!("{}\n", self.output);
        std::fs::write(path, bytes.as_bytes()).map_err(|error| {
            make_error(
                ErrorCode::FileExportFailure,
                format!("cannot write {}", path.to_string_lossy()),
                file_error_text(&error),
            )
        })
    }

    fn path_field(&mut self, field: PathField) -> &mut String {
        match field {
            PathField::Schedule => &mut self.schedule,
            PathField::Design => &mut self.design,
            PathField::Schema => &mut self.schema,
            PathField::ScheduleOut => &mut self.schedule_out,
            PathField::ScheduleSchema => &mut self.schedule_schema,
        }
    }

    /// A Browse button: a file dialog on the desktop; headless, a sentence saying which field to
    /// fill, as nobody could close a file dialog there.
    fn browse(&mut self, host: &dyn Host, field: PathField, title: &str, save: bool) {
        if host.headless() {
            self.set_status(
                &format!(
                    "A headless session opens no file dialog: fill {} with the path instead.",
                    field.name()
                ),
                true,
            );
            return;
        }
        let start = String::from(self.path_field(field).trim());
        let path = if save {
            get_save_file_name(title, &start, CSV_FILTER)
        } else {
            get_open_file_name(title, &start, CSV_FILTER)
        };
        if !path.is_empty() {
            *self.path_field(field) = to_native_separators(&path);
        }
    }

    /// Save As.
    fn save_output(&mut self, host: &dyn Host) {
        if host.headless() {
            self.set_status(
                "A headless session opens no file dialog; the reply is in the command log.",
                true,
            );
            return;
        }
        let path = get_save_file_name(
            "Save Utility Output",
            &self.suggested_file_name(),
            "Text files (*.txt);;All files (*)",
        );
        if path.is_empty() {
            return;
        }
        if let Err(error) = self.save_output_to(Path::new(&path)) {
            self.set_status(&error.describe(), true);
            return;
        }
        self.set_status(&format!("Saved to {}.", to_native_separators(&path)), false);
    }

    /// One frame of the dialog, when it is shown: its drawing's changes delivered first, then the
    /// dialog in its frame.
    pub fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        if !self.visible {
            return;
        }
        let changed = match &mut self.watcher {
            Some(watcher) => watcher.deliver(host.document()),
            None => None,
        };
        if changed.is_some_and(|changes| changes.model) {
            self.reload(&*host);
        }
        let ctx = ui.ctx().clone();
        let mut frame = self.frame.clone();
        let mut lent = Lent {
            dialog: &mut *self,
            host: &mut *host,
        };
        let shown = frame.show(
            &ctx,
            &mut lent,
            |ui, lent| lent.dialog.body(ui, &mut *lent.host),
            |ui, lent| lent.dialog.footer(ui),
        );
        self.frame = frame;
        if shown.closed {
            self.hide();
        }
    }

    fn body(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        ui.set_max_width(BODY_WIDTH);
        let tool = self.tool;
        // The source is chosen where a tool reads either; Regrade and Schedule read the drawing
        // whatever is chosen.
        let chooses = tool != UtilityTool::Regrade && tool != UtilityTool::Schedule;
        let drawing = utility_source_of(&self.form()) == UtilitySource::Drawing;

        // ---- where the services come from, above the tabs ----
        let source_group = ui.group(|ui| {
            ui.label("Services from");
            ui.horizontal(|ui| {
                ui.add_enabled_ui(chooses, |ui| {
                    if controls::radio(
                        ui,
                        "utilitySourceFile",
                        "A schedule file",
                        self.source == UtilitySource::File,
                    )
                    .on_hover_text(
                        "Draw, Report, Verify, Clearance and Check read the schedule (.csv)",
                    )
                    .clicked()
                    {
                        self.source = UtilitySource::File;
                    }
                });
                ui.add_enabled_ui(!drawing, |ui| {
                    path_line(
                        ui,
                        "utilitySchedule",
                        &mut self.schedule,
                        "the utility schedule: a .csv, one row per located vertex",
                        "The schedule of located services (docs/subsurface_utilities.md, \"The \
                         schedule format\"): rows with the same line id form one service, in \
                         order",
                    );
                    if controls::button(ui, "utilityScheduleBrowse", "Browse...").clicked() {
                        self.browse(&*host, PathField::Schedule, "Utility Schedule", false);
                    }
                });
            });
            ui.horizontal(|ui| {
                ui.add_enabled_ui(chooses, |ui| {
                    if controls::radio(
                        ui,
                        "utilitySourceDrawing",
                        "What is drawn",
                        self.source == UtilitySource::Drawing,
                    )
                    .on_hover_text(
                        "Draw takes the lines, polylines and points a survey or an import left in \
                         the drawing and draws them as services; Report, Verify, Clearance and \
                         Check read the services UTILITY DRAW drew, each line whole. Both by the \
                         scope and filter below. Regrade and Schedule always read what is drawn",
                    )
                    .clicked()
                    {
                        self.source = UtilitySource::Drawing;
                    }
                });
                note(
                    ui,
                    "in the scope and filter on the left: for Draw, the survey's or an import's \
                     lines and points, drawn as services; for the others, the lines UTILITY DRAW \
                     drew, where a scope that takes part of a line takes all of it",
                );
            });
        });
        controls::custom(
            ui,
            &source_group.response,
            ControlRecord::new("utilitySourceGroup", ControlKind::Other),
        );

        // ---- the scope and filter beside the tabs that read them ----
        ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            ui.vertical(|ui| {
                ui.set_max_width(SCOPE_WIDTH);
                ui.add_enabled_ui(drawing, |ui| {
                    self.scope.ui(ui);
                });
            });
            ui.vertical(|ui| {
                self.tools_ui(ui, &*host, drawing);
            });
        });

        // ---- the line, as it will run: read again, after what this frame's controls changed ----
        let line = self.command();
        let (mut shown, placeholder) = match &line {
            Ok(line) => (line.clone(), String::new()),
            Err(error) => (
                String::new(),
                format!("nothing to run yet: {}", error.message),
            ),
        };
        ui.horizontal(|ui| {
            ui.label("Command:");
            controls::line_with(
                ui,
                "utilityCommand",
                &mut shown,
                LineOptions {
                    placeholder: &placeholder,
                    read_only: true,
                    width: Some(BODY_WIDTH - 90.0),
                    ..LineOptions::default()
                },
            )
            .on_hover_text(
                "The line Run hands to the command line - type it there, or give it to \
                 katana_cli, and it does the same",
            );
        });
        // Run at the right edge and what happened last in the rest of the row, as the C++ row
        // stretches its status: laid out from the right, so a long status wraps beside Run rather
        // than pushing it off the row.
        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            if controls::button(ui, "utilityRun", "Run").clicked() {
                self.run(&mut *host);
                ui.ctx().request_repaint();
            }
            let colour = if self.status_error {
                theme::error().to_egui()
            } else {
                theme::text().to_egui()
            };
            let status = self.status.clone();
            controls::label_as(
                ui,
                "utilityStatus",
                &status,
                egui::Label::new(RichText::new(status.as_str()).color(colour))
                    .wrap()
                    .selectable(true),
            );
        });
        // The reports are tables: shown as written, in a fixed pitch.
        let mut reply = self.output.clone();
        controls::text_with(
            ui,
            "utilityOutput",
            &mut reply,
            TextOptions {
                read_only: true,
                rows: 8,
                monospace: true,
            },
        );
        if self.output.is_empty() {
            ui.weak("The reply appears here and in the command log.");
        }
        // Copy and Save act on the reply; they need the host, so they are in the body's last row.
        let has_output = !self.output.is_empty();
        ui.horizontal(|ui| {
            ui.add_enabled_ui(has_output, |ui| {
                if controls::button(ui, "utilityCopy", "Copy").clicked() {
                    let _ = host.clipboard().set_text(&self.output);
                    self.set_status("Copied.", false);
                }
                if controls::button(ui, "utilitySave", "Save As...").clicked() {
                    self.save_output(&*host);
                }
            });
        });
    }

    /// The dialog's own last row: Close, kept below the scrolling body.
    fn footer(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            if controls::button(ui, "utilityClose", "Close").clicked() {
                self.hide();
            }
        });
    }

    /// The fields every grading tool shares, then the tabs and the page in front.
    fn tools_ui(&mut self, ui: &mut Ui, host: &dyn Host, drawing: bool) {
        let tool = self.tool;
        // The detected spacing grades and the minimum cover flags what was graded; Draw, Report
        // and Regrade grade. Shared by them, as the verb's SPACING and MINCOVER are, so a drawing
        // and a report of one schedule are graded and flagged alike.
        let grades = matches!(
            tool,
            UtilityTool::Draw | UtilityTool::Report | UtilityTool::Regrade
        );
        // The placeholders show the library's own defaults, read from its settings, so what they
        // say cannot drift from what a blank option gets.
        let grading = GradingSettings::default();
        ui.add_enabled_ui(grades, |ui| {
            labelled(ui, "Detected spacing (m):", |ui| {
                path_line(
                    ui,
                    "utilitySpacing",
                    &mut self.spacing,
                    &format!("{} m", default_of(grading.maximum_detected_spacing)),
                    "SPACING: the longest segment a detected path may span and stay QL-B; a longer \
                     one was interpolated, not traced, and grades QL-C. The project \
                     specification's figure, not the standard's",
                );
            });
            labelled(ui, "Minimum cover (m):", |ui| {
                path_line(
                    ui,
                    "utilityMinCover",
                    &mut self.min_cover,
                    "none - cover is given, not tested",
                    "MINCOVER: flag every vertex whose cover is less than this - in the report's \
                     findings, and on each drawn point (utility.cover_below_minimum)",
                );
            });
        });

        let mut current = tool.index();
        if controls::tabs(ui, "utilityTabs", &TAB_TITLES, &mut current).changed() {
            self.tool = UtilityTool::from_index(current);
        }
        ui.separator();
        match self.tool {
            UtilityTool::Draw => self.draw_page(ui, drawing),
            UtilityTool::Report => note(
                ui,
                "The investigation report: every vertex and segment graded by AS 5488 quality \
                 level, the length of each service at each level, depth of cover, and what the \
                 schedule claims better than its evidence supports. Nothing is added to the \
                 drawing. On what is drawn, the reply leads with what the scope took.",
            ),
            UtilityTool::Verify => note(
                ui,
                "The QL-B detections compared with the QL-A exposures that check them (the \
                 schedule's verifies column): how far each detection was from where the service \
                 was seen, in plan and in level, against QL-B's tolerance. Nothing is added to the \
                 drawing.",
            ),
            UtilityTool::Clearance => self.clearance_page(ui, host),
            UtilityTool::Check => {
                note(
                    ui,
                    "The schedule against a client's delivery schema: every mandatory attribute \
                     present, every value from its list, spelt exactly. What is drawn is checked \
                     as the schedule it would write in the schema's words. A schedule with errors \
                     still shows the whole report, and the line fails so that a script can stop \
                     on it.",
                );
                labelled(ui, "Schema:", |ui| {
                    path_line(
                        ui,
                        "utilitySchema",
                        &mut self.schema,
                        "the delivery schema: a .csv of attributes and value lists",
                        "The schema file (include/katana/survey/subsurface/delivery_schema.hpp); \
                         tools/utility_schema_domains.py makes one from a TfNSW Utility Schema \
                         workbook",
                    );
                    if controls::button(ui, "utilitySchemaBrowse", "Browse...").clicked() {
                        self.browse(host, PathField::Schema, "Delivery Schema", false);
                    }
                });
            }
            UtilityTool::Regrade => note(
                ui,
                "Grades the drawn lines in the scope again from their points as they are now - \
                 points moved, levels or location methods edited - and draws their runs again, as \
                 one undo step. A line whose grading did not change is left alone, and when none \
                 did, nothing is added to the undo history. Always reads what is drawn; the \
                 detected spacing and the minimum cover above apply.",
            ),
            UtilityTool::Schedule => {
                note(
                    ui,
                    "Writes the drawn lines in the scope as a schedule (.csv) that UTILITY reads \
                     back as they are: a drawing edited in CAD made a deliverable. With a schema, \
                     in its column names and spellings. Always reads what is drawn; nothing in the \
                     drawing changes.",
                );
                labelled(ui, "Write to:", |ui| {
                    path_line(
                        ui,
                        "utilityScheduleOut",
                        &mut self.schedule_out,
                        "the schedule to write: a .csv",
                        "Where UTILITY SCHEDULE writes the lines in the scope; an existing file is \
                         replaced",
                    );
                    if controls::button(ui, "utilityScheduleOutBrowse", "Browse...").clicked() {
                        self.browse(host, PathField::ScheduleOut, "Write Utility Schedule", true);
                    }
                });
                labelled(ui, "Schema (optional):", |ui| {
                    path_line(
                        ui,
                        "utilityScheduleSchema",
                        &mut self.schedule_schema,
                        "none - the schedule format's own words",
                        "SCHEMA: write in this delivery schema's column names and spellings",
                    );
                    if controls::button(ui, "utilityScheduleSchemaBrowse", "Browse...").clicked() {
                        self.browse(host, PathField::ScheduleSchema, "Delivery Schema", false);
                    }
                });
            }
        }
    }

    fn draw_page(&mut self, ui: &mut Ui, drawing: bool) {
        note(
            ui,
            "Grades the services - the schedule file's, or the survey's or an import's lines and \
             points in the drawing - and adds them to the drawing as one undo step: a layer for \
             each type of service and quality level, with a linetype for each level (QL-A \
             continuous, QL-B dashed, QL-C dash-dot, QL-D dotted), one polyline for each run at \
             one level, and a point at every located vertex carrying its whole schedule row - and, \
             with a minimum cover, whether its cover is below it. A line that cannot be graded \
             refuses the whole draw. What is drawn already is not drawn again; the survey is left \
             as it is.",
        );
        labelled(ui, "Layer prefix:", |ui| {
            path_line(
                ui,
                "utilityLayerPrefix",
                &mut self.layer_prefix,
                "default utilities",
                "LAYER: the layer the drawn services nest under, as <prefix>/<type>/QL-A and \
                 <prefix>/<type>/points",
            );
        });
        // What the geometry cannot say is asked only of a draw of the geometry.
        ui.add_enabled_ui(drawing, |ui| {
            let group = ui.group(|ui| {
                ui.label("Drawn geometry as services (Draw of what is drawn)");
                note(
                    ui,
                    "Each line or open polyline in the scope is a service, named by its code; a \
                     point on a vertex gives it its number. What a line or a point says of itself \
                     (utility.method, utility.depth ... - Format > Global Modify sets them) wins \
                     over these.",
                );
                egui::Grid::new("utilityGeometryGrid")
                    .num_columns(4)
                    .show(ui, |ui| {
                        ui.label("Type:");
                        word_choice(
                            ui,
                            "utilityServiceType",
                            &mut self.service_type,
                            "TYPE: the kind of service, where a line does not say (utility.type); \
                             not given, unknown",
                        );
                        ui.label("Method:");
                        word_choice(
                            ui,
                            "utilityMethod",
                            &mut self.method,
                            "METHOD: how the services were located, where a point or a line does \
                             not say (utility.method). Needed at every vertex",
                        );
                        ui.end_row();
                        ui.label("H uncertainty (m):");
                        path_line(
                            ui,
                            "utilityHUnc",
                            &mut self.horizontal_uncertainty,
                            "not assessed",
                            "H_UNC: the horizontal uncertainty of each located position, +/- \
                             metres. Not assessed is not assumed good: a detection without one is \
                             QL-C",
                        );
                        ui.label("V uncertainty (m):");
                        path_line(
                            ui,
                            "utilityVUnc",
                            &mut self.vertical_uncertainty,
                            "not assessed",
                            "V_UNC: the vertical uncertainty of each level or depth, +/- metres",
                        );
                        ui.end_row();
                        ui.label("Heights are:");
                        word_choice(
                            ui,
                            "utilityHeights",
                            &mut self.heights,
                            "HEIGHTS: what the heights of the lines and points are; not given, the \
                             surface",
                        );
                        ui.label("Level reference:");
                        word_choice(
                            ui,
                            "utilityLevelRef",
                            &mut self.level_reference,
                            "LEVEL_REF: the part of the service a level or depth is on; not given, \
                             the top",
                        );
                        ui.end_row();
                        ui.label("Path:");
                        word_choice(
                            ui,
                            "utilityPath",
                            &mut self.path,
                            "PATH: what is known between each vertex and the next; not given, \
                             detected",
                        );
                        ui.label("Status:");
                        word_choice(
                            ui,
                            "utilityServiceStatus",
                            &mut self.service_status,
                            "STATUS: the service's status",
                        );
                        ui.end_row();
                        ui.label("Owner:");
                        path_line(
                            ui,
                            "utilityOwner",
                            &mut self.owner,
                            "each line's own",
                            "OWNER: the asset owner",
                        );
                        ui.label("Material:");
                        path_line(
                            ui,
                            "utilityMaterial",
                            &mut self.material,
                            "each line's own",
                            "MATERIAL: what the service is made of",
                        );
                        ui.end_row();
                        ui.label("Diameter (mm):");
                        path_line(
                            ui,
                            "utilityDiameter",
                            &mut self.diameter,
                            "each line's own",
                            "DIAMETER_MM: the outside diameter, millimetres",
                        );
                        ui.label("Fields:");
                        path_line(
                            ui,
                            "utilityFields",
                            &mut self.fields,
                            "column=property, ... e.g. type=ASSET_TYPE,line=ASSET_ID",
                            "FIELDS: an import's own attributes read as the schedule's columns - a \
                             shapefile's ASSET_TYPE as the type, its ASSET_ID as the line",
                        );
                        ui.end_row();
                    });
            });
            controls::custom(
                ui,
                &group.response,
                ControlRecord::new("utilityGeometryGroup", ControlKind::Other),
            );
        });
    }

    fn clearance_page(&mut self, ui: &mut Ui, host: &dyn Host) {
        note(
            ui,
            "Clearance of proposed works from every segment of every service, each widened by the \
             positional tolerance of its quality level: Conflict, Unconfirmed (QL-C or QL-D \
             nearby: locate it first), Within tolerance, or Clear. The clearances are the asset \
             owners' rules, not AS 5488's.",
        );
        let source = self.design_source;
        ui.horizontal(|ui| {
            if controls::radio(
                ui,
                "utilityDesignFile",
                "Design file:",
                source == UtilityDesignSource::File,
            )
            .clicked()
            {
                self.design_source = UtilityDesignSource::File;
            }
            ui.add_enabled_ui(source == UtilityDesignSource::File, |ui| {
                path_line(
                    ui,
                    "utilityDesign",
                    &mut self.design,
                    "the proposed works: a .csv of the centre line",
                    "The design centre line (docs/subsurface_utilities.md): easting, northing and \
                     an optional level at each vertex",
                );
                if controls::button(ui, "utilityDesignBrowse", "Browse...").clicked() {
                    self.browse(host, PathField::Design, "Proposed Works", false);
                }
            });
        });
        ui.horizontal(|ui| {
            if controls::radio(
                ui,
                "utilityDesignEntity",
                "Drawn line or polyline:",
                source == UtilityDesignSource::Entity,
            )
            .clicked()
            {
                self.design_source = UtilityDesignSource::Entity;
            }
            ui.add_enabled_ui(source == UtilityDesignSource::Entity, |ui| {
                path_line(
                    ui,
                    "utilityDesignEntityId",
                    &mut self.design_entity,
                    "#id of its centre line",
                    "A line or polyline in the drawing as the works' centre line: #id, or select \
                     it and press Use Selected",
                );
                if controls::button(ui, "utilityDesignUseSelected", "Use Selected")
                    .on_hover_text("The one entity selected in the drawing")
                    .clicked()
                {
                    self.use_selected_design(host);
                }
                ui.label("Level (m):");
                path_line(
                    ui,
                    "utilityDesignLevel",
                    &mut self.design_level,
                    "its own heights, if it has them",
                    "LEVEL: the works' level, metres; left blank, the levels the line or polyline \
                     carries, or none",
                );
            });
        });
        ui.horizontal(|ui| {
            if controls::radio(
                ui,
                "utilityDesignAlignment",
                "Alignment:",
                source == UtilityDesignSource::Alignment,
            )
            .clicked()
            {
                self.design_source = UtilityDesignSource::Alignment;
            }
            ui.add_enabled_ui(source == UtilityDesignSource::Alignment, |ui| {
                controls::choice(
                    ui,
                    "utilityDesignAlignmentName",
                    &self.alignments,
                    &mut self.alignment,
                )
                .on_hover_text(
                    "One of the drawing's alignments: its horizontal geometry, at the levels of \
                     its design profile where it has one",
                );
                if self.alignments.is_empty() {
                    ui.weak("the drawing has no alignment");
                }
            });
        });
        let requirement = ClearanceRequirement::default();
        labelled(ui, "Works width (m):", |ui| {
            path_line(
                ui,
                "utilityWidth",
                &mut self.width,
                "default 0 m - the centre line itself",
                "WIDTH: the works' width - a pipe's outside diameter, a trench's width",
            );
        });
        labelled(ui, "Horizontal clearance (m):", |ui| {
            path_line(
                ui,
                "utilityH",
                &mut self.horizontal,
                &format!("{} m", default_of(requirement.horizontal)),
                "H: the horizontal clearance required, face to face",
            );
        });
        labelled(ui, "Vertical clearance (m):", |ui| {
            path_line(
                ui,
                "utilityV",
                &mut self.vertical,
                &format!("{} m", default_of(requirement.vertical)),
                "V: the vertical clearance required, face to face",
            );
        });
        labelled(ui, "Unverified margin (m):", |ui| {
            path_line(
                ui,
                "utilityMargin",
                &mut self.margin,
                &format!("{} m", default_of(requirement.unverified_margin)),
                "MARGIN: a QL-C or QL-D service this close to the works, beyond the horizontal \
                 clearance, is Unconfirmed - its drawn position is not a measurement",
            );
        });
    }
}

/// A one-line field with its placeholder and tip.
fn path_line(ui: &mut Ui, name: &str, value: &mut String, placeholder: &str, tip: &str) {
    controls::line_with(
        ui,
        name,
        value,
        LineOptions {
            placeholder,
            ..LineOptions::default()
        },
    )
    .on_hover_text(tip);
}

/// A choice of the verb's words.
fn word_choice(ui: &mut Ui, name: &str, choice: &mut WordChoice, tip: &str) {
    let tip = choice.tip(tip);
    controls::choice(ui, name, &choice.items, &mut choice.chosen).on_hover_text(tip);
}

/// A form's row: its caption, then what `add` draws.
fn labelled(ui: &mut Ui, caption: &str, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.label(caption);
        add(ui);
    });
}

/// A note in the muted colour, wrapped (noteLabel).
fn note(ui: &mut Ui, text: &str) {
    ui.add(egui::Label::new(RichText::new(text).color(theme::text_muted().to_egui())).wrap());
}

/// What QFile::errorString says of a file that could not be written: the system's sentence,
/// without the "(os error N)" the standard library adds to it.
fn file_error_text(error: &std::io::Error) -> String {
    let text = format!("{error}");
    match text.rfind(" (os error ") {
        Some(at) if text.ends_with(')') => String::from(&text[..at]),
        _ => text,
    }
}
