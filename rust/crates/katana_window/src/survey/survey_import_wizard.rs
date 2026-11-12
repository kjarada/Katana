//! Survey > Import Survey Points... (survey_import_wizard.hpp/.cpp): a paged dialog. A delimited
//! coordinate file takes the six steps it always has; a field file with a reader of its own
//! (Leica GSI, Trimble JobXML, Topcon and Carlson RW5, Topcon GTS-7, RINEX ...) takes the
//! instrument path, which reads the file whole, reduces and adjusts it, and keeps it as a SURVEY
//! JOB that can be adjusted again later (Survey > Survey Jobs).
//!
//! ```text
//! 1 File        the file to read.
//! 2 Format      surveyio's detection: EVERY registered format, the candidates first, each with
//!               its evidence and its FormatDescriptor record. When the detection is not
//!               Identified - and a delimited text file never is - the person MUST choose: the
//!               page starts on "(choose the format)" and Next refuses it.
//! 3 Layout      delimited files only: propose_layout's reading of the header, its candidates,
//!               presets, a role box per column of the list as typed, the delimiter, header
//!               lines, comment prefix and quoting, saved templates (survey_templates), and a
//!               preview of the first points with the parser's error inline. When the proposal is
//!               Uncertain, Next needs the person to tick that they have checked the order of
//!               northing and easting (the order is never guessed).
//! 3 Content     instrument files only, in place of Layout: the file read with surveyio's
//!               read_survey - setups, observations by kind, points, coded features, control,
//!               GNSS sessions, the files found beside it, what the file does not carry, and
//!               every warning with its record. A large file is read on a thread of its own, with
//!               a busy bar and Cancel (survey_task).
//! 4 System      the unit the numbers are in (no default), the system the file is in (unknown
//!               unless stated) and, only when BOTH a source and a target EPSG code are given, a
//!               transformation through katana_geodesy (cad's transform_survey_project). For an
//!               instrument file the units are the file's own and nothing is transformed.
//! 5 Reduction   instrument files only: the reduction and adjustment options
//!   & Adjustment (reduction_options_widget) and Preview, which runs the reduction and shows
//!               its report inline (reduction_report_view).
//! 6 Options     the layer, a layer per code, what to do with ids the drawing already has
//!               (cad's ExistingPointPolicy), and the FINISH: whether the imported points are
//!               coded by the session's survey codes and joined into lines.
//! 7 Report      records read, every warning, the error that blocks the import or the points and
//!               layers it will create, and what is asked of the survey codes. Import makes ONE
//!               undoable command of the points, their codes and their lines.
//! ```
//!
//! Nothing survey-specific is computed here: the pages gather text and hand it to surveyio, survey
//! and cad. What the finish is - whether, with which colours and control codes - is cad's
//! survey_import_finish, the one function SURVEY IMPORT asks too, so the wizard's import and the
//! line's cannot come to differ.
//!
//! As the C++ does, Import executes its cad command on the drawing itself (one undo step) rather
//! than handing the SURVEY IMPORT line to the executor (docs/survey.md, "Not done").
//!
//! Every page is drawn at every frame - the one in front shown, the others off the stage
//! (survey_dialogs::offstage) - so a field of a page not in front is still there to fill, as a
//! QStackedWidget's other pages' widgets are.
//!
//! The object names are an interface: the headless --dialog switch fills fields and presses
//! buttons by them.
//!
//! ```text
//! dialog  surveyImportDialog
//! fields  file | format | candidate preset template templateName columns role1.. delimiter
//!         headerLines comment quoting confirmOrder | unit declared target | the reduction options
//!         (method, atmospheric, controlPick, advanced, ...) | layer layerPerCode existing
//!         applyCodes drawLinework
//! shown   step detectionSummary candidates formatRecord proposal preview parseError | content
//!         contentSummary | systemSummary | finishNote | previewReport (and its Sections, Browser,
//!         Summary) | report importReport (and its Sections, Browser, Summary) | task taskStatus |
//!         message
//! buttons browse | saveTemplate deleteTemplate | addControl removeControl advanced
//!         previewReduction | cancelTask | back next import close
//! ```

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use egui::{Context, Ui};
use katana_cad::linework_verbs::{SurveyImportFinish, survey_import_finish};
use katana_cad::survey_finish::{
    SurveyFinishEarlier, SurveyFinishReport, describe as describe_finish, with_survey_finish,
};
use katana_cad::survey_job::{
    ImportSurveyJobCommand, SurveyJob, SurveyJobContext, SurveyJobFile, SurveyJobImport,
    reduce_for_drawing, reduction_context_for,
};
use katana_cad::survey_points::{
    ExistingPointPolicy, SurveyImportSummary, SurveyPointImportReport, declared_system_from_epsg,
    format_survey_import_report, import_survey_points, transform_survey_project,
};
use katana_cad::{Document, SurveyImportOptions};
use katana_core::civil_date::now_utc;
use katana_core::error::make_error;
use katana_core::path_text::{path_from_utf8, path_to_utf8};
use katana_core::text::{format_exact_real, parse_integer};
use katana_core::{Error, ErrorCode, Result, Status};
use katana_entity::EnumName;
use katana_survey::data_model::{observation_kind_name, to_string as survey_name};
use katana_survey::reduction::ReductionOutcome;
use katana_survey::reduction_report::render_html;
use katana_survey::reduction_settings::control_from_file;
use katana_survey::{
    AngularUnit, DeclaredUnits, LinearUnit, ReductionSettings, SurveyPoint, SurveyProject,
};
use katana_surveyio::{
    ColumnRole, DELIMITED_POINTS_FORMAT_ID, DelimitedImportOptions, DelimitedLayout, Delimiter,
    DetectionOutcome, ImportResult, LayoutProposal, LayoutProposalOutcome, MAX_SURVEY_FILE_BYTES,
    PROBE_BYTES, Quoting, ReadOptions, ReadResult, all_column_presets, column_preset_named,
    describe_format, detect_format, format_registry, layout_template, parse_delimited_points,
    parse_layout_template, preset_columns, probe_of, propose_layout, read_survey,
    siblings_in_folder, template_letter, to_string as surveyio_name,
};

use super::reduction_options_widget::ReductionOptionsWidget;
use super::reduction_report_view::{ReductionReportView, display_report};
use super::survey_dialogs::{
    choice_of, coloured_label, field, field_if, form_row, frame_views, hidden, message_colour,
    muted, no_file_dialog, offstage, open_file_dialog,
};
use super::survey_job_support::{
    BACKGROUND_OBSERVATIONS, BACKGROUND_READ_BYTES, PrecomputedReduction, REPORT_ROWS_ON_SCREEN,
    adjustment_summary, precomputed_reduction, rejected_observations, report_input_for,
    survey_size,
};
use super::survey_points_ui::{column_letters, file_name_of};
use super::survey_task::SurveyTaskBar;
use super::survey_templates::{
    TemplateChoice, delete_layout_template, save_layout_template, saved_layout_templates,
};
use crate::drive::{self, Choice, TextOptions};
use crate::format::qt_fixed;
use crate::host::Host;
use crate::shell::DialogFrame;
use crate::theme::palette;

/// Points the layout page previews. Enough rows to see that the columns are the right way round
/// and the numbers are what the file holds; the whole file is still parsed, so an error on its
/// last line shows here too.
const PREVIEW_ROWS: usize = 20;

/// Warnings listed one by one on the Content step; the rest are counted. A damaged file can have
/// hundreds of thousands, and the list is for reading.
const LISTED_WARNINGS: usize = 1_000;

/// The role boxes' items, in their order.
const ROLES: [ColumnRole; 7] = [
    ColumnRole::PointId,
    ColumnRole::Northing,
    ColumnRole::Easting,
    ColumnRole::Elevation,
    ColumnRole::Code,
    ColumnRole::Description,
    ColumnRole::Ignore,
];

/// The units a coordinate file may be in, after "(choose the unit)".
const UNITS: [LinearUnit; 4] = [
    LinearUnit::Metres,
    LinearUnit::Feet,
    LinearUnit::UsSurveyFeet,
    LinearUnit::Links,
];

/// What happens to an id the drawing already has, in the choice's order.
const POLICIES: [ExistingPointPolicy; 4] = [
    ExistingPointPolicy::Refuse,
    ExistingPointPolicy::Skip,
    ExistingPointPolicy::Replace,
    ExistingPointPolicy::KeepBoth,
];

const DELIMITERS: [Delimiter; 4] = [
    Delimiter::Comma,
    Delimiter::Tab,
    Delimiter::Semicolon,
    Delimiter::Whitespace,
];

const QUOTINGS: [Quoting; 2] = [Quoting::DoubleQuote, Quoting::None];

// The pages, in the order the C++'s Page enumeration has them; path() says which a file goes
// through.
const FILE_PAGE: usize = 0;
const FORMAT_PAGE: usize = 1;
const LAYOUT_PAGE: usize = 2;
const CONTENT_PAGE: usize = 3;
const SYSTEM_PAGE: usize = 4;
const REDUCTION_PAGE: usize = 5;
const OPTIONS_PAGE: usize = 6;
const REPORT_PAGE: usize = 7;
const PAGES: usize = 8;

const TITLES: [&str; PAGES] = [
    "Choose the file",
    "The file's format",
    "Columns and delimiter",
    "What the file holds",
    "Units and coordinate system",
    "Reduction and adjustment",
    "Options",
    "Report",
];

/// What a field file holds, counted for the Content step: worked out on the thread that read the
/// file, so the interface thread only shows it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SurveyContent {
    pub setups: usize,
    /// On setups and loose.
    pub observations: usize,
    pub observations_by_kind: BTreeMap<String, usize>,
    pub positioned_points: usize,
    pub unpositioned_points: usize,
    pub features: usize,
    pub control_points: usize,
    pub gnss_sessions: usize,
    pub traverses: usize,
}

pub fn survey_content_of(project: &SurveyProject) -> SurveyContent {
    let mut content = SurveyContent {
        setups: project.stations.len(),
        positioned_points: project.points.len(),
        unpositioned_points: project.unpositioned_points.len(),
        features: project.features.len(),
        control_points: project.control_points.len(),
        gnss_sessions: project.gnss_sessions.len(),
        traverses: project.traverses.len(),
        ..SurveyContent::default()
    };
    let observations = project
        .stations
        .iter()
        .flat_map(|station| station.observations.iter())
        .chain(project.observations.iter());
    for observation in observations {
        content.observations += 1;
        *content
            .observations_by_kind
            .entry(observation_kind_name(observation))
            .or_insert(0) += 1;
    }
    content
}

/// The File step's Browse filter: every extension a registered format names
/// (FormatDescriptor::extensions), so a format added to the registry is offered without editing
/// this, and the patterns no descriptor states.
pub fn survey_file_filter() -> String {
    // What no descriptor states: point lists named .pnt and .xyz, which the delimited reader
    // takes by content; GTS-6's .gt6; the numbered files of a DBX job (.x01); and RINEX 2's
    // names, whose extension is the year's last two digits and the file's type (.24o, .24d).
    const UNREGISTERED: [&str; 6] = ["*.pnt", "*.xyz", "*.gt6", "*.x01", "*.??o", "*.??d"];
    let mut patterns: BTreeSet<String> = UNREGISTERED.iter().map(|p| String::from(*p)).collect();
    for descriptor in format_registry().formats() {
        for extension in &descriptor.extensions {
            patterns.insert(format!("*.{extension}"));
        }
    }
    let listed: Vec<String> = patterns.into_iter().collect();
    format!(
        "Survey files ({});;Point files (*.csv *.txt *.tsv *.pnt *.xyz);;All files (*)",
        listed.join(" ")
    )
}

/// One row of the Content step's tree: what it counts, what was read, and its children.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ContentRow {
    pub what: String,
    pub detail: String,
    pub children: Vec<(String, String)>,
    /// Shown open: its children are listed under it.
    pub expanded: bool,
    /// In the error colour.
    pub error: bool,
}

impl ContentRow {
    fn new(what: &str, detail: String) -> Self {
        Self {
            what: String::from(what),
            detail,
            ..Self::default()
        }
    }
}

/// The column list as typed: one trimmed entry per comma-separated part, none for a blank field.
/// NOT validated, because the role boxes follow it while it is not yet a layout surveyio accepts
/// - two easting columns half way through swapping northing and easting, say.
fn typed_columns(text: &str) -> Vec<String> {
    let text = text.trim();
    if text.is_empty() {
        return Vec::new();
    }
    text.split(',')
        .map(|entry| String::from(entry.trim()))
        .collect()
}

/// The role an entry's letter spells, matched without regard to case as templates are; None for
/// an entry that is not one letter surveyio knows (its box is left blank, and the parser names
/// the entry). Found through template_letter, so the letters are spelt in one place.
fn role_index_of(entry: &str) -> Option<usize> {
    let mut characters = entry.chars();
    let letter = characters.next()?;
    if characters.next().is_some() {
        return None;
    }
    ROLES
        .iter()
        .position(|role| template_letter(*role).eq_ignore_ascii_case(&letter))
}

/// A whole number from a field, or None when it is blank; ParseFailure for anything else.
fn epsg_field(text: &str, what: &str) -> Result<Option<i32>> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    let body = if text.len() > 5 && (text.starts_with("EPSG:") || text.starts_with("epsg:")) {
        &text[5..]
    } else {
        text
    };
    match parse_integer(body) {
        Some(code) if (1..=i64::from(i32::MAX)).contains(&code) => Ok(Some(code as i32)),
        _ => Err(make_error(
            ErrorCode::ParseFailure,
            format!("{what}: an EPSG code is a positive whole number"),
            text,
        )),
    }
}

/// "50.2 MB", "812 KB": a file's size as the busy row says it.
fn size_text(bytes: u64) -> String {
    if bytes >= 1_000_000 {
        return format!("{} MB", qt_fixed(bytes as f64 / 1e6, 1));
    }
    format!("{} KB", bytes.div_ceil(1000).max(1))
}

fn units_text(units: &DeclaredUnits) -> String {
    let linear = if units.linear == LinearUnit::Unknown {
        String::from("lengths not stated")
    } else {
        format!("lengths in {}", survey_name(units.linear))
    };
    let angular = if units.angular == AngularUnit::Unknown {
        String::from("angles not stated")
    } else {
        format!("angles in {}", survey_name(units.angular))
    };
    format!("{linear}, {angular}")
}

/// What Import asks of the survey codes, as the two boxes stand and the drawing is NOW: cad's
/// survey_import_finish, the function SURVEY IMPORT asks, which hands on nothing at all while no
/// survey codes are loaded. A box asks only while it is enabled and ticked.
fn finish_asked(
    document: &Document,
    enabled: bool,
    codes: bool,
    linework: bool,
) -> SurveyImportFinish {
    survey_import_finish(document, Some(enabled && codes), Some(enabled && linework))
}

/// How the points are drawn. A layer per field code only while the codes are NOT applied:
/// applied, they move each coded point to its rule's layer, and the layer made for its code
/// beneath `layer` would be left empty.
fn drawing_options(
    layer: &str,
    layer_per_code: bool,
    finish: &SurveyImportFinish,
) -> SurveyImportOptions {
    SurveyImportOptions {
        layer: String::from(layer.trim()),
        layer_per_code: layer_per_code && !finish.options.codes,
        ..SurveyImportOptions::default()
    }
}

/// What the import will ask of the survey codes, for the Report step and the note under the two
/// boxes: two sentences, or one when there are no codes to ask anything of. `numbered` is a field
/// file's path, whose reader may hand over strings the file numbered itself.
fn finish_plan(finish: &SurveyImportFinish, numbered: bool) -> Vec<String> {
    if !finish.survey_codes_loaded {
        return vec![String::from(
            "Survey codes: the session has none, so the points are drawn as the file has them \
             and no line is drawn.",
        )];
    }
    let mut plan = vec![String::from(if finish.options.codes {
        "Survey codes: applied to the imported points. Each goes on the layer and takes the \
         style its code's rule gives it; a code with no rule is left as imported and named."
    } else {
        "Survey codes: not applied."
    })];
    plan.push(String::from(if !finish.options.linework {
        "Linework: not drawn."
    } else if numbered {
        "Linework: the imported points are joined into lines where a rule makes their code a \
         line - the strings the file numbered itself, then the rest by code."
    } else {
        "Linework: the imported points are joined into lines, by code, where a rule makes their \
         code a line."
    }));
    plan
}

/// What a read off the interface thread gives the wizard.
struct ReadDone {
    bytes: Arc<Vec<u8>>,
    read: Arc<ReadResult>,
    raw: Arc<SurveyProject>,
    counts: SurveyContent,
}

/// What follows a preview once it has succeeded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Then {
    Nothing,
    Report,
    Import,
}

/// What a thread of the wizard's hands back.
#[allow(
    clippy::large_enum_variant,
    reason = "one value a task, moved once from its thread to the wizard: a box would only add an allocation"
)]
enum WizardWork {
    /// The file read with its reader. An error that the file could not be opened is shown
    /// whatever was read since; a reader's error only while its read is the latest.
    Read {
        generation: u64,
        result: std::result::Result<ReadDone, (Error, bool)>,
    },
    Reduce {
        generation: u64,
        revision: u64,
        system: String,
        drawing_points: Vec<SurveyPoint>,
        settings: ReductionSettings,
        then: Then,
        result: Result<(Arc<ReductionOutcome>, String)>,
    },
}

/// What the wizard draws, and what was done to it this frame.
struct WizardForm {
    step: String,
    page: usize,
    file: String,
    detection_summary: String,
    detection_is_error: bool,
    candidates: Vec<Vec<String>>,
    candidate_row: Option<usize>,
    format: Choice,
    format_items: Vec<String>,
    format_ids: Vec<String>,
    format_record: String,
    proposal: String,
    proposal_is_error: bool,
    candidate: Choice,
    candidate_items: Vec<String>,
    preset: Choice,
    template: TemplateChoice,
    templates: Vec<(String, String)>,
    template_name: String,
    columns: String,
    roles: Vec<Choice>,
    delimiter: Choice,
    header_lines: i64,
    comment: String,
    quoting: Choice,
    confirm_order: bool,
    confirm_shown: bool,
    preview: Vec<Vec<String>>,
    preview_row: Option<usize>,
    parse_error: String,
    parse_error_is_error: bool,
    content_summary: String,
    content: Vec<ContentRow>,
    content_row: Option<usize>,
    system_summary: String,
    system_summary_shown: bool,
    system_fields_shown: bool,
    system_fields_enabled: bool,
    unit: Choice,
    declared: String,
    target: String,
    options: ReductionOptionsWidget,
    preview_report: ReductionReportView,
    layer: String,
    layer_per_code: bool,
    layer_per_code_enabled: bool,
    layer_per_code_tip: String,
    existing: Choice,
    apply_codes: bool,
    draw_linework: bool,
    codes_enabled: bool,
    apply_codes_tip: String,
    draw_linework_tip: String,
    finish_note: String,
    report: String,
    import_report: ReductionReportView,
    import_report_shown: bool,
    task: SurveyTaskBar<WizardWork>,
    message: String,
    message_is_error: bool,
    blocked: bool,
    // What was done to it this frame.
    pressed: Vec<&'static str>,
    task_cancelled: bool,
    format_changed: bool,
    candidate_chosen: Option<usize>,
    preset_changed: bool,
    template_chosen: Option<String>,
    layout_changed: bool,
    role_changed: Option<usize>,
    finish_changed: bool,
    options_changed: bool,
}

/// The wizard (the C++ SurveyImportWizard).
pub struct SurveyImportWizard {
    frame: DialogFrame,
    open: bool,
    form: WizardForm,
    // What the pages have established so far.
    bytes: Vec<u8>,
    /// The NAME only.
    file_name: String,
    layout_proposal: Option<LayoutProposal>,
    parsed: Option<ImportResult>,
    project: Option<SurveyProject>,
    layout_text: String,
    system_text: String,
    transform_text: String,
    // The instrument path's state. The raw project is shared, not copied, with the threads that
    // reduce it.
    reader_path: bool,
    /// `bytes` holds the whole file, not only its head.
    bytes_complete: bool,
    format_id: String,
    source_bytes: Option<Arc<Vec<u8>>>,
    read: Option<Arc<ReadResult>>,
    raw: Option<Arc<SurveyProject>>,
    content_counts: SurveyContent,
    /// Bumped by every read.
    read_generation: u64,
    /// The read the options were set up for.
    options_generation: u64,
    /// The read the finish boxes were last set for from the customisation, and whether the
    /// session had survey codes then (prepare_finish_choice).
    finish_generation: u64,
    finish_with_codes: bool,
    // The last successful preview and what it was made from.
    outcome: Option<Arc<ReductionOutcome>>,
    outcome_settings: ReductionSettings,
    outcome_drawing_points: Vec<SurveyPoint>,
    outcome_read: u64,
    outcome_revision: u64,
    outcome_system: String,
    preview_pending: bool,
    preview_failed: bool,
}

impl Default for SurveyImportWizard {
    fn default() -> Self {
        Self::new()
    }
}

impl SurveyImportWizard {
    pub fn new() -> Self {
        let defaults = SurveyImportOptions::default();
        let mut wizard = Self {
            frame: DialogFrame::new("surveyImportDialog", "Import Survey Points"),
            open: false,
            form: WizardForm {
                step: String::new(),
                page: FILE_PAGE,
                file: String::new(),
                detection_summary: String::new(),
                detection_is_error: false,
                candidates: Vec::new(),
                candidate_row: None,
                format: Choice::default(),
                format_items: Vec::new(),
                format_ids: Vec::new(),
                format_record: String::new(),
                proposal: String::new(),
                proposal_is_error: false,
                candidate: Choice::default(),
                candidate_items: Vec::new(),
                preset: Choice::at(0),
                template: TemplateChoice::new(),
                templates: Vec::new(),
                template_name: String::new(),
                columns: String::new(),
                roles: Vec::new(),
                delimiter: Choice::at(0),
                header_lines: 0,
                comment: String::new(),
                quoting: Choice::at(0),
                confirm_order: false,
                confirm_shown: true,
                preview: Vec::new(),
                preview_row: None,
                parse_error: String::new(),
                parse_error_is_error: false,
                content_summary: String::new(),
                content: Vec::new(),
                content_row: None,
                system_summary: String::new(),
                system_summary_shown: false,
                system_fields_shown: true,
                system_fields_enabled: true,
                unit: Choice::at(0),
                declared: String::new(),
                target: String::new(),
                options: ReductionOptionsWidget::new(),
                preview_report: ReductionReportView::new("previewReport"),
                layer: defaults.layer,
                layer_per_code: false,
                layer_per_code_enabled: true,
                layer_per_code_tip: String::new(),
                existing: Choice::at(0),
                apply_codes: false,
                draw_linework: false,
                codes_enabled: true,
                apply_codes_tip: String::new(),
                draw_linework_tip: String::new(),
                finish_note: String::new(),
                report: String::new(),
                import_report: ReductionReportView::new("importReport"),
                import_report_shown: false,
                task: SurveyTaskBar::new(),
                message: String::new(),
                message_is_error: false,
                blocked: true,
                pressed: Vec::new(),
                task_cancelled: false,
                format_changed: false,
                candidate_chosen: None,
                preset_changed: false,
                template_chosen: None,
                layout_changed: false,
                role_changed: None,
                finish_changed: false,
                options_changed: false,
            },
            bytes: Vec::new(),
            file_name: String::new(),
            layout_proposal: None,
            parsed: None,
            project: None,
            layout_text: String::new(),
            system_text: String::new(),
            transform_text: String::new(),
            reader_path: false,
            bytes_complete: false,
            format_id: String::new(),
            source_bytes: None,
            read: None,
            raw: None,
            content_counts: SurveyContent::default(),
            read_generation: 0,
            options_generation: 0,
            finish_generation: 0,
            finish_with_codes: false,
            outcome: None,
            outcome_settings: ReductionSettings::default(),
            outcome_drawing_points: Vec::new(),
            outcome_read: 0,
            outcome_revision: 0,
            outcome_system: String::new(),
            preview_pending: false,
            preview_failed: true,
        };
        wizard.show_step_title();
        wizard
    }

    pub fn show(&mut self) {
        self.open = true;
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// The task row: whether a read or a reduction is running in the background.
    pub fn task_busy(&self) -> bool {
        self.form.task.busy()
    }

    /// The Content step's rows, top level first and their children.
    pub fn content(&self) -> &[ContentRow] {
        &self.form.content
    }

    /// "Step 3 of 7: What the file holds".
    pub fn step(&self) -> &str {
        &self.form.step
    }

    /// The tooltip of "A layer per field code": why it is not offered while the codes are
    /// applied.
    pub fn layer_per_code_tip(&self) -> &str {
        &self.form.layer_per_code_tip
    }

    /// The report pane's text ("report"). It outlives the dialog's close, as the C++'s hidden
    /// QPlainTextEdit did: an import closes the wizard, and the pane then holds what was imported
    /// and what the finish did, though no frame draws it.
    pub fn report_text(&self) -> &str {
        &self.form.report
    }

    pub fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        if !self.open {
            return;
        }
        let ctx = ui.ctx().clone();
        // The preview is rebuilt on the event loop's turn, never inside the change of the box
        // that asked: the role boxes it rebuilds are among the boxes whose changes ask.
        if self.preview_pending {
            self.preview_pending = false;
            self.preview();
        }
        if let Some(work) = self.form.task.poll() {
            self.apply_work(host, &ctx, work);
        }
        // Read at every frame: what is saved or deleted meanwhile, here or in the Export dialog,
        // is offered at once, or no longer.
        self.form.templates = saved_layout_templates(host.settings());
        let form = &mut self.form;
        form.pressed.clear();
        form.task_cancelled = false;
        form.format_changed = false;
        form.candidate_chosen = None;
        form.preset_changed = false;
        form.template_chosen = None;
        form.layout_changed = false;
        form.role_changed = None;
        form.finish_changed = false;
        form.options_changed = false;
        let shown = self
            .frame
            .show(&ctx, &mut self.form, Self::draw_body, Self::draw_footer);
        if shown.closed {
            self.open = false;
        }
        self.after_frame(host, &ctx);
    }

    fn after_frame(&mut self, host: &mut dyn Host, ctx: &Context) {
        if self.form.task_cancelled {
            self.show_message("Cancelled: nothing was changed.");
        }
        if let Some(column) = self.form.role_changed {
            self.roles_changed(column);
        }
        if self.form.layout_changed {
            self.schedule_preview(ctx);
        }
        if self.form.format_changed {
            self.show_format_record();
            // The steps ahead depend on the format: seven for a field file, six for a
            // coordinate file.
            if self.form.page == FORMAT_PAGE {
                self.show_step_title();
            }
        }
        if let Some(index) = self.form.candidate_chosen
            && let Some(layout) = self
                .layout_proposal
                .as_ref()
                .and_then(|proposal| proposal.candidates().get(index))
                .map(|candidate| candidate.layout.clone())
        {
            self.apply_layout(&layout, ctx);
        }
        if self.form.preset_changed
            && let Some(index) = self.form.preset.index.filter(|index| *index > 0)
            && let Some(preset) = all_column_presets().get(index - 1)
        {
            match column_preset_named(surveyio_name(*preset)) {
                Err(error) => self.show_error(host, &error),
                Ok(preset) => {
                    self.form.columns = column_letters(&preset_columns(preset));
                    self.schedule_preview(ctx);
                }
            }
        }
        if let Some(text) = self.form.template_chosen.take() {
            match parse_layout_template(&text) {
                Err(error) => self.show_error(host, &error),
                Ok(saved) => {
                    self.apply_layout(&saved, ctx);
                    if let Some(name) = self.form.template.current() {
                        self.form.template_name = String::from(name);
                    }
                }
            }
        }
        if self.form.finish_changed {
            self.show_finish_choice(host);
        }
        if self.form.options_changed && self.outcome.is_some() && !self.preview_is_current(host) {
            self.form.preview_report.show_note(
                "The options have changed since the preview: press Preview to run the reduction \
                 again.",
            );
        }
        let pressed = std::mem::take(&mut self.form.pressed);
        for name in pressed {
            match name {
                "browse" => self.browse(host),
                "saveTemplate" => self.save_template(host),
                "deleteTemplate" => self.delete_template(host),
                "previewReduction" => self.run_preview(host, ctx, Then::Nothing),
                "back" => self.go_to(self.previous_page(self.form.page), host),
                "next" => self.advance(host, ctx),
                "import" => self.import_now(host, ctx),
                "close" => self.open = false,
                _ => {}
            }
        }
    }

    // ---- drawing --------------------------------------------------------------------------

    fn draw_body(ui: &mut Ui, form: &mut WizardForm) {
        drive::label_as(
            ui,
            "step",
            &form.step,
            egui::Label::new(egui::RichText::new(form.step.as_str()).strong()),
        );
        let pages: [fn(&mut Ui, &mut WizardForm); PAGES] = [
            Self::file_page,
            Self::format_page,
            Self::layout_page,
            Self::content_page,
            Self::system_page,
            Self::reduction_page,
            Self::options_page,
            Self::report_page,
        ];
        for (page, draw) in pages.iter().enumerate() {
            if page == form.page {
                draw(ui, form);
            } else {
                offstage(ui, TITLES[page], |ui| draw(ui, form));
            }
        }
        if form.task.ui(ui) {
            form.task_cancelled = true;
        }
        coloured_label(
            ui,
            "message",
            &form.message,
            message_colour(form.message_is_error),
        );
    }

    fn draw_footer(ui: &mut Ui, form: &mut WizardForm) {
        // While a file is read or reduced, nothing that would start another or leave the page is
        // pressed; Cancel is.
        let busy = form.task.busy();
        let page = form.page;
        ui.horizontal(|ui| {
            let buttons = [
                ("back", "< Back", !busy && page > FILE_PAGE),
                ("next", "Next >", !busy && page < REPORT_PAGE),
                (
                    "import",
                    "Import",
                    !busy && page == REPORT_PAGE && !form.blocked,
                ),
                ("close", "Close", true),
            ];
            for (name, text, enabled) in buttons {
                if ui
                    .add_enabled_ui(enabled, |ui| drive::button(ui, name, text))
                    .inner
                    .clicked()
                {
                    form.pressed.push(name);
                }
            }
        });
    }

    fn file_page(ui: &mut Ui, form: &mut WizardForm) {
        muted(
            ui,
            "Choose a file of surveyed points, or a field file from an instrument or a GNSS \
             receiver. The next step says what Katana makes of it and, when it cannot be sure, \
             asks you: a file is never read with a format nobody chose.",
        );
        ui.horizontal(|ui| {
            field(ui, "file", &mut form.file, "The file to import", "");
            if drive::button(ui, "browse", "Browse...").clicked() {
                form.pressed.push("browse");
            }
        });
    }

    fn format_page(ui: &mut Ui, form: &mut WizardForm) {
        coloured_label(
            ui,
            "detectionSummary",
            &form.detection_summary,
            message_colour(form.detection_is_error),
        );
        ui.weak("Format | Confidence | Evidence | Record");
        egui::ScrollArea::vertical()
            .id_salt("surveyImportCandidates")
            .max_height(240.0)
            .show(ui, |ui| {
                drive::rows(ui, "candidates", &form.candidates, &mut form.candidate_row);
            });
        form_row(ui, "Read the file as:", |ui| {
            if drive::choice(ui, "format", &form.format_items, &mut form.format)
                .on_hover_text("The format to read the file with")
                .changed()
            {
                form.format_changed = true;
            }
        });
        coloured_label(
            ui,
            "formatRecord",
            &form.format_record,
            palette::text_muted().to_egui(),
        );
    }

    fn layout_page(ui: &mut Ui, form: &mut WizardForm) {
        coloured_label(
            ui,
            "proposal",
            &form.proposal,
            message_colour(form.proposal_is_error),
        );
        form_row(ui, "Proposed:", |ui| {
            if drive::choice(ui, "candidate", &form.candidate_items, &mut form.candidate)
                .on_hover_text("The layouts that fit the file, best first")
                .changed()
            {
                form.candidate_chosen = form.candidate.index;
            }
        });
        form_row(ui, "Or use:", |ui| {
            let mut presets = vec!["(presets)"];
            presets.extend(
                all_column_presets()
                    .iter()
                    .map(|preset| surveyio_name(*preset)),
            );
            if choice_of(ui, "preset", &presets, &mut form.preset)
                .on_hover_text("A column order survey software names by its letters")
                .changed()
            {
                form.preset_changed = true;
            }
            let (_, chosen) = form.template.ui(ui, "template", &form.templates);
            form.template_chosen = chosen;
            field(
                ui,
                "templateName",
                &mut form.template_name,
                "name to save as",
                "",
            );
            if drive::button(ui, "saveTemplate", "Save").clicked() {
                form.pressed.push("saveTemplate");
            }
            if drive::button(ui, "deleteTemplate", "Delete").clicked() {
                form.pressed.push("deleteTemplate");
            }
        });
        form_row(ui, "Columns:", |ui| {
            if field(
                ui,
                "columns",
                &mut form.columns,
                "",
                "One letter per column in file order: P point id, N northing, E easting, Z \
                 elevation, C code, D description, - ignore",
            )
            .changed()
            {
                form.layout_changed = true;
            }
        });
        // A role box per entry of the column list as typed.
        let roles: Vec<&str> = ROLES.iter().map(|role| surveyio_name(*role)).collect();
        let shown = ui.horizontal(|ui| {
            ui.add_space(8.0);
            for (column, role) in form.roles.iter_mut().enumerate() {
                // Any change of the choice, not only a click: the headless --fill chooses, and a
                // box that ignored it would be a box no test could drive.
                if choice_of(ui, &format!("role{}", column + 1), &roles, role)
                    .on_hover_text(format!("What column {} holds", column + 1))
                    .changed()
                {
                    form.role_changed = Some(column);
                }
            }
        });
        drive::custom(
            ui,
            &shown.response,
            drive::ControlRecord::new("columnRoles", drive::ControlKind::Other),
        );
        form_row(ui, "Reading:", |ui| {
            ui.label("Delimiter");
            let delimiters: Vec<&str> = DELIMITERS.iter().map(|d| surveyio_name(*d)).collect();
            form.layout_changed |=
                choice_of(ui, "delimiter", &delimiters, &mut form.delimiter).changed();
            ui.label("Header lines");
            form.layout_changed |=
                drive::whole(ui, "headerLines", &mut form.header_lines, 0..=1000)
                    .on_hover_text("Lines at the top of the file skipped whatever they hold")
                    .changed();
            ui.label("Comment");
            form.layout_changed |= drive::line_with(
                ui,
                "comment",
                &mut form.comment,
                drive::LineOptions {
                    placeholder: "none",
                    width: Some(80.0),
                    ..drive::LineOptions::default()
                },
            )
            .on_hover_text("A line starting with this is a comment")
            .changed();
            ui.label("Quotes");
            let quotings: Vec<&str> = QUOTINGS.iter().map(|q| surveyio_name(*q)).collect();
            form.layout_changed |= choice_of(ui, "quoting", &quotings, &mut form.quoting).changed();
        });
        let mut confirm = |ui: &mut Ui| {
            drive::check(
                ui,
                "confirmOrder",
                "I have checked the preview: the northing and easting columns are the right way \
                 round",
                &mut form.confirm_order,
            );
        };
        if form.confirm_shown {
            confirm(ui);
        } else {
            hidden(ui, "confirmOrder", confirm);
        }
        ui.weak("Line | Point | Easting | Northing | Elevation | Code | Description");
        egui::ScrollArea::vertical()
            .id_salt("surveyImportPreview")
            .max_height(240.0)
            .show(ui, |ui| {
                drive::rows(ui, "preview", &form.preview, &mut form.preview_row);
            });
        coloured_label(
            ui,
            "parseError",
            &form.parse_error,
            message_colour(form.parse_error_is_error),
        );
    }

    fn content_page(ui: &mut Ui, form: &mut WizardForm) {
        coloured_label(
            ui,
            "contentSummary",
            &form.content_summary,
            palette::text().to_egui(),
        );
        // A tree's rows depth first, down every row it shows open: what a person reads in it.
        let mut rows = Vec::new();
        for row in &form.content {
            rows.push(vec![row.what.clone(), row.detail.clone()]);
            if row.expanded {
                rows.extend(
                    row.children
                        .iter()
                        .map(|(what, detail)| vec![what.clone(), detail.clone()]),
                );
            }
        }
        ui.weak("In the file | What was read");
        egui::ScrollArea::vertical()
            .id_salt("surveyImportContent")
            .max_height(360.0)
            .show(ui, |ui| {
                drive::rows(ui, "content", &rows, &mut form.content_row);
            });
        muted(
            ui,
            "What the reader made of the file, before anything is reduced or drawn. Every record \
             it could not read is a warning naming the record; nothing is dropped without a \
             word.",
        );
    }

    fn system_page(ui: &mut Ui, form: &mut WizardForm) {
        let summary = |ui: &mut Ui| {
            coloured_label(
                ui,
                "systemSummary",
                &form.system_summary,
                palette::text().to_egui(),
            );
        };
        if form.system_summary_shown {
            summary(ui);
        } else {
            hidden(ui, "systemSummary", summary);
        }
        // What a coordinate file needs; a field file's reader states its units and nothing of it
        // is transformed, so its path hides all of this.
        let enabled = form.system_fields_enabled;
        let mut fields = |ui: &mut Ui| {
            muted(
                ui,
                "A point file does not say what its numbers are: choose the unit they are in \
                 (there is no default - a wrong unit scales the whole survey). The coordinate \
                 system is recorded as unknown unless you state it. Katana transforms the points \
                 only when you give BOTH the file's system and a target, by EPSG code, through \
                 its geodesy library; both must be projected (grid) systems, and heights are \
                 carried through unchanged - no vertical datum change is made.",
            );
            let mut units = vec!["(choose the unit)"];
            units.extend(UNITS.iter().map(|unit| survey_name(*unit)));
            form_row(ui, "The numbers are in:", |ui| {
                ui.add_enabled_ui(enabled, |ui| choice_of(ui, "unit", &units, &mut form.unit));
            });
            form_row(ui, "The file's system:", |ui| {
                field_if(
                    ui,
                    enabled,
                    "declared",
                    &mut form.declared,
                    "unknown - or an EPSG code, e.g. 28356",
                    "",
                );
            });
            form_row(ui, "Transform into:", |ui| {
                field_if(
                    ui,
                    enabled,
                    "target",
                    &mut form.target,
                    "none - or an EPSG code to transform into",
                    "",
                );
            });
        };
        if form.system_fields_shown {
            fields(ui);
        } else {
            hidden(ui, "systemFields", fields);
        }
    }

    fn reduction_page(ui: &mut Ui, form: &mut WizardForm) {
        egui::ScrollArea::vertical()
            .id_salt("surveyImportOptions")
            .max_height(360.0)
            .show(ui, |ui| {
                if form.options.ui(ui) {
                    form.options_changed = true;
                }
            });
        let busy = form.task.busy();
        ui.horizontal(|ui| {
            if ui
                .add_enabled_ui(!busy, |ui| {
                    drive::button(ui, "previewReduction", "Preview").on_hover_text(
                        "Reduce and adjust with these options and show the report here; nothing \
                         is drawn",
                    )
                })
                .inner
                .clicked()
            {
                form.pressed.push("previewReduction");
            }
            muted(
                ui,
                "Runs the reduction as the import would and shows its report below. Nothing is \
                 drawn until Import.",
            );
        });
        form.preview_report.ui(ui);
    }

    fn options_page(ui: &mut Ui, form: &mut WizardForm) {
        form_row(ui, "Layer:", |ui| {
            field(
                ui,
                "layer",
                &mut form.layer,
                "",
                "The layer the points go on; '/' makes a child layer",
            )
        });
        let per_code_enabled = form.layer_per_code_enabled;
        let per_code_tip = form.layer_per_code_tip.clone();
        ui.add_enabled_ui(per_code_enabled, |ui| {
            drive::check(
                ui,
                "layerPerCode",
                "A layer per field code beneath it",
                &mut form.layer_per_code,
            )
            .on_hover_text(per_code_tip.as_str())
        });
        let policies: Vec<&str> = POLICIES.iter().map(|policy| policy.name()).collect();
        form_row(ui, "Ids already in the drawing:", |ui| {
            choice_of(ui, "existing", &policies, &mut form.existing).on_hover_text(
                "What happens to a point whose id a survey point in the drawing already has",
            )
        });
        let codes_enabled = form.codes_enabled;
        let codes_tip = form.apply_codes_tip.clone();
        let linework_tip = form.draw_linework_tip.clone();
        ui.add_enabled_ui(codes_enabled, |ui| {
            if drive::check(
                ui,
                "applyCodes",
                "Apply survey codes to the imported points",
                &mut form.apply_codes,
            )
            .on_hover_text(codes_tip.as_str())
            .changed()
            {
                form.finish_changed = true;
            }
            if drive::check(
                ui,
                "drawLinework",
                "Draw linework through the imported points",
                &mut form.draw_linework,
            )
            .on_hover_text(linework_tip.as_str())
            .changed()
            {
                form.finish_changed = true;
            }
        });
        // Plain text, said rather than left to a guess: what stands in angle brackets in this
        // note (CUSTOMISE <file>) is a word of a command line, never markup.
        coloured_label(
            ui,
            "finishNote",
            &form.finish_note,
            palette::text_muted().to_egui(),
        );
        muted(
            ui,
            "Refusing is the default for ids the drawing already has: such an import is more \
             often the wrong file than a wanted update.",
        );
    }

    fn report_page(ui: &mut Ui, form: &mut WizardForm) {
        muted(
            ui,
            "What the import will do. Import makes it one command: one Undo removes it all.",
        );
        drive::text_with(
            ui,
            "report",
            &mut form.report,
            TextOptions {
                read_only: true,
                rows: 14,
                monospace: true,
            },
        );
        // The reduction report, for a field file: what will happen to the data.
        if form.import_report_shown {
            form.import_report.ui(ui);
        } else {
            hidden(ui, "importReport", |ui| form.import_report.ui(ui));
        }
    }

    // ---- moving between pages --------------------------------------------------------------

    /// The pages this file goes through, in order: the delimited path or the instrument path.
    /// Until the format is chosen, the path is the one the format on show would take, so the
    /// step count does not change under the person's feet between the Format step and the next.
    fn path(&self) -> Vec<usize> {
        let mut reader = self.reader_path;
        if self.form.page <= FORMAT_PAGE {
            let id = self.current_format_id();
            reader = !id.is_empty() && format_registry().reader(id).is_some();
        }
        if reader {
            vec![
                FILE_PAGE,
                FORMAT_PAGE,
                CONTENT_PAGE,
                SYSTEM_PAGE,
                REDUCTION_PAGE,
                OPTIONS_PAGE,
                REPORT_PAGE,
            ]
        } else {
            vec![
                FILE_PAGE,
                FORMAT_PAGE,
                LAYOUT_PAGE,
                SYSTEM_PAGE,
                OPTIONS_PAGE,
                REPORT_PAGE,
            ]
        }
    }

    fn current_format_id(&self) -> &str {
        self.form
            .format
            .index
            .and_then(|index| self.form.format_ids.get(index))
            .map_or("", String::as_str)
    }

    fn next_page(&self, page: usize) -> usize {
        let pages = self.path();
        match pages.iter().position(|each| *each == page) {
            Some(at) if at + 1 < pages.len() => pages[at + 1],
            _ => page,
        }
    }

    fn previous_page(&self, page: usize) -> usize {
        let pages = self.path();
        match pages.iter().position(|each| *each == page) {
            Some(at) if at > 0 => pages[at - 1],
            _ => FILE_PAGE,
        }
    }

    fn show_step_title(&mut self) {
        let page = self.form.page;
        let pages = self.path();
        let number = pages
            .iter()
            .position(|each| *each == page)
            .map_or(0, |at| at + 1);
        self.form.step = format!(
            "Step {number} of {}: {}",
            pages.len(),
            TITLES[page.min(PAGES - 1)]
        );
    }

    fn go_to(&mut self, page: usize, host: &mut dyn Host) {
        self.form.page = page.min(PAGES - 1);
        self.show_step_title();
        if self.form.page == OPTIONS_PAGE {
            self.prepare_finish_choice(host);
        }
        self.form.message.clear();
    }

    /// The Options step is reached: the two finish boxes are enabled as the session has survey
    /// codes or not and, for a file read since they were last set (or when survey codes have
    /// arrived or gone since), set as the drawing's customisation has them switched. Going back
    /// and forth over one file keeps what the person ticked.
    fn prepare_finish_choice(&mut self, host: &mut dyn Host) {
        let document = host.document();
        let codes = !document.survey_map().empty();
        self.form.codes_enabled = codes;
        if self.finish_generation != self.read_generation || self.finish_with_codes != codes {
            // As the customisation has them switched, which is what SURVEY IMPORT does with a
            // file when its line says nothing; and neither while there are no survey codes to
            // apply or to string by.
            let automation = &document.customisation_state().automation;
            self.form.apply_codes = codes && automation.codes_on_survey_import;
            self.form.draw_linework = codes && automation.linework_on_survey_import;
            self.finish_generation = self.read_generation;
            self.finish_with_codes = codes;
        }
        self.show_finish_choice(host);
    }

    fn finish_asked(&self, document: &Document) -> SurveyImportFinish {
        finish_asked(
            document,
            self.form.codes_enabled,
            self.form.apply_codes,
            self.form.draw_linework,
        )
    }

    /// A finish box changed, or the step was reached: the layer-per-code box and the note follow.
    fn show_finish_choice(&mut self, host: &mut dyn Host) {
        let finish = self.finish_asked(host.document());
        let none = if finish.survey_codes_loaded {
            ""
        } else {
            " Not offered: the session has no survey codes."
        };
        self.form.apply_codes_tip = format!(
            "Each imported point goes on the layer and takes the style the session's survey \
             codes give its code.{none}"
        );
        self.form.draw_linework_tip = format!(
            "The imported points are joined into lines where a survey code rule makes their \
             code a line.{none}"
        );
        // The codes choose the layers: a layer per field code as well would be made for every
        // code and then left empty. The tick is kept, for when the codes are switched off again;
        // drawing_options is what reads whether it counts.
        self.form.layer_per_code_enabled = !finish.options.codes;
        self.form.layer_per_code_tip = String::from(if finish.options.codes {
            "Not used while survey codes are applied: the codes choose each point's layer, and \
             a layer per field code as well would be left empty"
        } else {
            "Each code's points go on a layer of their own beneath the layer above"
        });
        let mut note = finish_plan(&finish, self.reader_path).join(" ");
        if finish.survey_codes_loaded {
            note.push_str(if finish.options.any() {
                " It is all one command: one Undo takes back the points with their codes and \
                 their lines."
            } else {
                " The points are drawn on the layer above and nothing else is done."
            });
        } else {
            // What loads survey codes is a Katana customisation file: Settings' Import, or the
            // line that runs.
            note.push_str(
                " A Katana customisation file brings survey codes: load one in File > Settings > \
                 Import, or type CUSTOMISE <file> on the command line.",
            );
        }
        self.form.finish_note = note;
    }

    fn advance(&mut self, host: &mut dyn Host, ctx: &Context) {
        if self.form.task.busy() {
            return;
        }
        let page = self.form.page;
        // The steps whose work may run off the interface thread go on from the work's result,
        // not from here.
        if page == FORMAT_PAGE {
            if let Err(error) = self.choose_format(ctx) {
                self.show_error(host, &error);
                return;
            }
            if self.reader_path {
                self.read_with_reader(host, ctx);
                return;
            }
            self.go_to(LAYOUT_PAGE, host);
            return;
        }
        if page == OPTIONS_PAGE && self.reader_path {
            if self.preview_is_current(host) {
                self.prepare_reader_report(host);
                self.go_to(REPORT_PAGE, host);
                return;
            }
            self.run_preview(host, ctx, Then::Report);
            return;
        }
        if let Err(error) = self.leave(page, host) {
            self.show_error(host, &error);
            return;
        }
        let next = self.next_page(page);
        self.go_to(next, host);
    }

    /// What leaving `page` forward needs: the page's input checked and the next page's content
    /// prepared. An error keeps the person where they are.
    fn leave(&mut self, page: usize, host: &mut dyn Host) -> Status {
        match page {
            FILE_PAGE => self.read_file(),
            LAYOUT_PAGE => {
                self.layout_from_fields()?;
                // Now, not on the next turn: a field typed a moment ago must be what Next judges.
                self.preview();
                if self.preview_failed {
                    return Err(Error::new(
                        ErrorCode::ParseFailure,
                        "the file does not read with this layout - see the error under the \
                         preview",
                    ));
                }
                if self
                    .layout_proposal
                    .as_ref()
                    .is_some_and(|proposal| proposal.outcome() != LayoutProposalOutcome::Decided)
                    && !self.form.confirm_order
                {
                    return Err(Error::new(
                        ErrorCode::InvalidState,
                        "Katana could not tell the order of the columns from the file: check \
                         the preview and tick that northing and easting are the right way round",
                    ));
                }
                Ok(())
            }
            CONTENT_PAGE => {
                self.prepare_system_for_reader(host);
                Ok(())
            }
            SYSTEM_PAGE => {
                if self.reader_path {
                    self.prepare_reduction(host);
                    return Ok(());
                }
                self.parse_and_transform()
            }
            REDUCTION_PAGE => {
                // The options must make sense before the page is left; the reduction itself
                // runs on Preview or on the way to the Report step.
                self.form.options.settings()?;
                Ok(())
            }
            OPTIONS_PAGE => {
                self.prepare_report(host);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn read_file(&mut self) -> Status {
        let path = String::from(self.form.file.trim());
        if path.is_empty() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "choose a file to import",
            ));
        }
        let mut file = std::fs::File::open(path_from_utf8(&path)).map_err(|error| {
            make_error(
                ErrorCode::FileImportFailure,
                "the file cannot be opened",
                format!("{path}: {error}"),
            )
        })?;
        // A large file is not read here, on the interface thread: detection needs only its
        // head, and its reader reads it whole on a thread of its own (Content).
        let size = file.metadata().map_or(0, |metadata| metadata.len());
        self.bytes_complete = size <= BACKGROUND_READ_BYTES;
        let mut bytes = Vec::new();
        let _ = if self.bytes_complete {
            file.read_to_end(&mut bytes)
        } else {
            (&mut file).take(PROBE_BYTES as u64).read_to_end(&mut bytes)
        };
        self.bytes = bytes;
        self.file_name = file_name_of(&path);
        self.parsed = None;
        self.project = None;
        self.read = None;
        self.raw = None;
        self.source_bytes = None;
        self.outcome = None;
        self.read_generation += 1;

        let sample = &self.bytes[..self.bytes.len().min(PROBE_BYTES)];
        let detection = detect_format(&probe_of(
            sample,
            &self.file_name,
            size > PROBE_BYTES as u64,
        ));
        if detection.outcome() == DetectionOutcome::Empty {
            return Err(make_error(
                ErrorCode::FileImportFailure,
                "the file is empty",
                self.file_name.as_str(),
            ));
        }
        let identified = detection.outcome() == DetectionOutcome::Identified;
        let mut summary = format!(
            "Detection: {}. {}",
            surveyio_name(detection.outcome()),
            detection.summary()
        );
        if !identified {
            summary.push_str(
                "\nKatana is not sure what this file is: choose the format to read it with.",
            );
        }
        self.form.detection_summary = summary;
        self.form.detection_is_error = !identified;

        // Every registered format, the candidates first with their evidence: what Katana can
        // read at all is worth seeing when the file is not what the person thought, and each
        // line carries its record.
        let registry = format_registry();
        let mut rows = Vec::new();
        let mut items = Vec::new();
        let mut ids = Vec::new();
        if !identified {
            items.push(String::from("(choose the format)"));
            ids.push(String::new());
        }
        let mut listed = BTreeSet::new();
        for candidate in detection.candidates() {
            let descriptor = registry.find(&candidate.format_id).ok();
            if let Some(descriptor) = &descriptor {
                rows.push(vec![
                    descriptor.human_name.clone(),
                    qt_fixed(candidate.confidence, 2),
                    candidate.evidence.clone(),
                    describe_format(descriptor),
                ]);
            }
            listed.insert(candidate.format_id.clone());
            items.push(descriptor.map_or_else(|| candidate.format_id.clone(), |d| d.human_name));
            ids.push(candidate.format_id.clone());
        }
        for descriptor in registry.formats() {
            if listed.contains(&descriptor.id) {
                continue;
            }
            rows.push(vec![
                descriptor.human_name.clone(),
                String::from("-"),
                String::from("not detected in this file"),
                describe_format(&descriptor),
            ]);
            // Still choosable: a detection can be wrong, and the choice is the person's. A
            // format that cannot import is listed, not offered.
            if descriptor.can_import {
                items.push(descriptor.human_name.clone());
                ids.push(descriptor.id.clone());
            }
        }
        let index = if identified {
            detection
                .format()
                .ok()
                .and_then(|chosen| ids.iter().position(|id| *id == chosen.id))
                .unwrap_or(0)
        } else {
            0
        };
        self.form.candidates = rows;
        self.form.candidate_row = None;
        self.form.format_items = items;
        self.form.format_ids = ids;
        self.form.format = if self.form.format_items.is_empty() {
            Choice::default()
        } else {
            Choice::at(index)
        };
        self.show_format_record();
        Ok(())
    }

    fn show_format_record(&mut self) {
        let id = String::from(self.current_format_id());
        self.form.format_record = if id.is_empty() {
            String::from("No format chosen.")
        } else {
            match format_registry().find(&id) {
                Ok(descriptor) => describe_format(&descriptor),
                Err(_) => format!("Not a registered format: {id}"),
            }
        };
    }

    fn choose_format(&mut self, ctx: &Context) -> Status {
        let id = String::from(self.current_format_id());
        if id.is_empty() {
            return Err(Error::new(
                ErrorCode::InvalidState,
                "the file's format is not certain: choose the format to read it with",
            ));
        }
        let descriptor = format_registry().find(&id)?;
        if !descriptor.can_import {
            return Err(make_error(
                ErrorCode::Unsupported,
                "Katana cannot import this format",
                descriptor.human_name.as_str(),
            ));
        }
        // A format with a reader of its own is read whole by it (the instrument path); delimited
        // points are read through the layout the person confirms. Anything else is refused by
        // name, never handed to the delimited reader.
        if format_registry().reader(&descriptor.id).is_some() {
            self.reader_path = true;
            self.format_id = descriptor.id;
            return Ok(());
        }
        if descriptor.id != DELIMITED_POINTS_FORMAT_ID {
            return Err(make_error(
                ErrorCode::Unsupported,
                "Katana has no reader for this format: export the points as a delimited text \
                 file instead",
                descriptor.human_name.as_str(),
            ));
        }
        self.reader_path = false;
        self.format_id = descriptor.id;
        self.form.system_summary_shown = false;
        self.form.system_fields_shown = true;
        self.form.system_fields_enabled = true;
        self.form.import_report_shown = false;
        if !self.bytes_complete {
            // A large coordinate file: the layout page needs all of it, read here as it always
            // was.
            let path = path_from_utf8(self.form.file.trim());
            self.bytes = std::fs::read(path).map_err(|error| {
                make_error(
                    ErrorCode::FileImportFailure,
                    "the file cannot be opened",
                    format!("{}: {error}", self.file_name),
                )
            })?;
            self.bytes_complete = true;
        }
        let proposal = propose_layout(&self.bytes, false)?;
        let decided = proposal.outcome() == LayoutProposalOutcome::Decided;
        let mut text = String::from(proposal.summary());
        if !decided {
            text = format!(
                "Not decided: {text}\nThe order of the columns is yours to state - check the \
                 preview below and tick the box."
            );
        }
        self.form.proposal = text;
        self.form.proposal_is_error = !decided;
        self.form.confirm_order = false;
        self.form.confirm_shown = !decided;
        self.form.candidate_items = proposal
            .candidates()
            .iter()
            .map(|candidate| layout_template(&candidate.layout))
            .collect();
        self.form.candidate = if self.form.candidate_items.is_empty() {
            Choice::default()
        } else {
            Choice::at(0)
        };
        let first = proposal
            .candidates()
            .first()
            .map(|candidate| candidate.layout.clone());
        self.layout_proposal = Some(proposal);
        match first {
            Some(layout) => self.apply_layout(&layout, ctx),
            None => self.schedule_preview(ctx),
        }
        Ok(())
    }

    // ---- the layout ------------------------------------------------------------------------

    fn apply_layout(&mut self, layout: &DelimitedLayout, ctx: &Context) {
        self.form.columns = column_letters(&layout.columns);
        self.form.delimiter = Choice::at(
            DELIMITERS
                .iter()
                .position(|delimiter| *delimiter == layout.delimiter)
                .unwrap_or(0),
        );
        self.form.header_lines = layout.header_lines as i64;
        self.form.comment.clone_from(&layout.comment_prefix);
        self.form.quoting = Choice::at(
            QUOTINGS
                .iter()
                .position(|quoting| *quoting == layout.quoting)
                .unwrap_or(0),
        );
        self.schedule_preview(ctx);
    }

    /// The fields assembled into surveyio's own template text and read by its parser, so what
    /// this page accepts is exactly what a saved template may hold.
    fn layout_from_fields(&self) -> Result<DelimitedLayout> {
        let form = &self.form;
        let mut text = format!(
            "{};delimiter={};header={};quote={}",
            form.columns.trim(),
            surveyio_name(DELIMITERS[form.delimiter.index.unwrap_or(0).min(3)]),
            form.header_lines,
            surveyio_name(QUOTINGS[form.quoting.index.unwrap_or(0).min(1)])
        );
        let comment = form.comment.trim();
        if !comment.is_empty() {
            text.push_str(";comment=");
            text.push_str(comment);
        }
        parse_layout_template(&text)
    }

    fn schedule_preview(&mut self, ctx: &Context) {
        self.preview_pending = true;
        ctx.request_repaint();
    }

    fn preview(&mut self) {
        let layout = self.layout_from_fields();
        // A role box per entry of the column list AS TYPED, rebuilt only when the number of
        // entries changes - never because the layout does not validate, since swapping two
        // columns passes through a layout that does not.
        let entries = typed_columns(&self.form.columns);
        if self.form.roles.len() != entries.len() {
            self.form.roles = vec![Choice::default(); entries.len()];
        }
        for (role, entry) in self.form.roles.iter_mut().zip(&entries) {
            role.index = role_index_of(entry);
        }
        let layout = match layout {
            Ok(layout) => layout,
            Err(error) => {
                self.fail_preview(&error);
                return;
            }
        };
        if self.bytes.is_empty() {
            self.preview_failed = true;
            self.form.parse_error.clear();
            return;
        }
        // The numbers as the file writes them: the unit is chosen on the next page, and metres
        // converts nothing.
        let options = DelimitedImportOptions {
            unit: LinearUnit::Metres,
            encoding: None,
        };
        let parsed = match parse_delimited_points(&self.bytes, &layout, &self.file_name, &options) {
            Ok(parsed) => parsed,
            Err(error) => {
                self.fail_preview(&error);
                return;
            }
        };
        let points = &parsed.project.points;
        let shown = points.len().min(PREVIEW_ROWS);
        self.form.preview = points[..shown]
            .iter()
            .map(|point| {
                vec![
                    format!("{}", point.source.record_number),
                    point.id.clone(),
                    format_exact_real(point.easting),
                    format_exact_real(point.northing),
                    point.elevation.map(format_exact_real).unwrap_or_default(),
                    point.code.clone(),
                    point.description.clone(),
                ]
            })
            .collect();
        self.form.preview_row = None;
        self.form.parse_error = format!(
            "{} points read; the first {shown} shown, as written in the file.",
            points.len()
        );
        self.form.parse_error_is_error = false;
        self.preview_failed = false;
    }

    fn fail_preview(&mut self, error: &Error) {
        self.preview_failed = true;
        self.form.parse_error = error.describe();
        self.form.parse_error_is_error = true;
        self.form.preview.clear();
        self.form.preview_row = None;
    }

    /// The role box of `column` (0-based) changed: its entry in the column list is rewritten,
    /// and the preview follows on the next turn. Only that entry: the others stay as typed, so an
    /// entry no box can show is kept for the parser to name rather than silently read as the
    /// first role in the list.
    fn roles_changed(&mut self, column: usize) {
        let mut entries = typed_columns(&self.form.columns);
        if column >= entries.len() || column >= self.form.roles.len() {
            return;
        }
        let Some(index) = self.form.roles[column].index else {
            return;
        };
        entries[column] = String::from(template_letter(ROLES[index.min(ROLES.len() - 1)]));
        self.form.columns = entries.join(",");
        self.form.layout_changed = true;
    }

    fn save_template(&mut self, host: &mut dyn Host) {
        let current = match self.layout_from_fields() {
            Ok(layout) => layout,
            Err(error) => {
                self.show_error(host, &error);
                return;
            }
        };
        let name = String::from(self.form.template_name.trim());
        let text = layout_template(&current);
        if let Err(error) = save_layout_template(host.settings(), &name, &text) {
            self.show_error(host, &error);
            return;
        }
        // The new name is chosen without applying what the fields hold.
        self.form.template.set_current(Some(&name));
        self.show_message(&format!("Saved the layout as the template \"{name}\"."));
        host.log(
            &format!("Saved the survey layout template \"{name}\": {text}"),
            false,
        );
    }

    fn delete_template(&mut self, host: &mut dyn Host) {
        let name = match self.form.template.current() {
            Some(chosen) => String::from(chosen),
            None => self.form.template_name.clone(),
        };
        let name = String::from(name.trim());
        if let Err(error) = delete_layout_template(host.settings(), &name) {
            self.show_error(host, &error);
            return;
        }
        self.show_message(&format!("Deleted the template \"{name}\"."));
        host.log(
            &format!("Deleted the survey layout template \"{name}\""),
            false,
        );
    }

    fn browse(&mut self, host: &mut dyn Host) {
        if host.headless() {
            self.show_error(host, &no_file_dialog());
            return;
        }
        // The filter's patterns that are an extension each; the rest are for Qt's matcher.
        let filter = survey_file_filter();
        let extensions: Vec<String> = filter
            .split(";;")
            .next()
            .unwrap_or("")
            .split(['(', ')', ' '])
            .filter_map(|pattern| pattern.strip_prefix("*."))
            .filter(|extension| !extension.contains('?'))
            .map(String::from)
            .collect();
        let extensions: Vec<&str> = extensions.iter().map(String::as_str).collect();
        if let Some(path) = open_file_dialog(
            "Import Survey Points",
            &[("Survey files", &extensions[..]), ("All files", &["*"][..])],
        ) {
            self.form.file = String::from_utf8_lossy(&path_to_utf8(&path)).into_owned();
        }
    }

    // ---- reading, transforming, reporting, importing ---------------------------------------

    fn parse_and_transform(&mut self) -> Status {
        let unit_index = self.form.unit.index.unwrap_or(0);
        if unit_index == 0 {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "choose the unit the file's numbers are in - there is no default",
            ));
        }
        let layout = self.layout_from_fields()?;
        let options = DelimitedImportOptions {
            unit: UNITS[(unit_index - 1).min(UNITS.len() - 1)],
            encoding: None,
        };
        let parsed = parse_delimited_points(&self.bytes, &layout, &self.file_name, &options)?;
        let declared = epsg_field(&self.form.declared, "the file's system")?;
        let target = epsg_field(&self.form.target, "transform into")?;
        if target.is_some() && declared.is_none() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "a transformation needs the file's system as well as the target: Katana never \
                 assumes what a file is in",
            ));
        }
        let mut project = parsed.project.clone();
        self.system_text = String::from("unknown (none stated)");
        self.transform_text.clear();
        if let Some(code) = declared {
            let system = declared_system_from_epsg(code)?;
            self.system_text = format!("EPSG:{code} {} (stated)", system.name);
            project.coordinate_system = system;
        }
        if let (Some(source), Some(target)) = (declared, target) {
            project = transform_survey_project(project, source, target)?;
            self.transform_text = format!(
                "from EPSG:{source} to EPSG:{target} {} by {}; heights unchanged",
                project.coordinate_system.name,
                project
                    .metadata
                    .get("transformation")
                    .cloned()
                    .unwrap_or_default()
            );
        }
        self.layout_text = layout_template(&layout);
        self.parsed = Some(parsed);
        self.project = Some(project);
        Ok(())
    }

    fn policy(&self) -> ExistingPointPolicy {
        POLICIES[self
            .form
            .existing
            .index
            .unwrap_or(0)
            .min(POLICIES.len() - 1)]
    }

    fn prepare_report(&mut self, host: &mut dyn Host) {
        let document = host.document();
        let finish = self.finish_asked(document);
        let options = drawing_options(&self.form.layer, self.form.layer_per_code, &finish);
        let policy = self.policy();
        let (Some(parsed), Some(project)) = (&self.parsed, &self.project) else {
            return;
        };
        let format = match format_registry().find(&parsed.format_id) {
            Ok(descriptor) => format!(
                "{} (parser {})",
                descriptor.human_name, descriptor.parser_version
            ),
            Err(_) => parsed.format_id.clone(),
        };
        let summary = SurveyImportSummary {
            file_name: self.file_name.clone(),
            format,
            layout: self.layout_text.clone(),
            records_read: parsed.records_read,
            reader_warnings: parsed.warnings.clone(),
            coordinate_system: self.system_text.clone(),
            transformation: self.transform_text.clone(),
            units: String::from(survey_name(project.units.linear)),
            policy,
        };
        let mut report = SurveyPointImportReport::default();
        let command = import_survey_points(document, project, &options, policy, Some(&mut report));
        let blocked = !matches!(command, Ok(Some(_)));
        let mut text = match &command {
            Err(error) => format_survey_import_report(&summary, &report, Some(error)),
            Ok(command) => {
                let mut text = format_survey_import_report(&summary, &report, None);
                if command.is_none() {
                    text.push_str("Nothing to import.\n");
                }
                text
            }
        };
        // What is asked of the survey codes: the points planned above are coded and strung when
        // the command runs, not before, so what it DID follows the import.
        if !blocked {
            text.push_str(&finish_plan(&finish, false).join("\n"));
            text.push('\n');
        }
        self.form.blocked = blocked;
        self.form.report = text;
    }

    /// After an import: what the finish did, in the log and at the end of the report pane.
    /// Nothing for no report or one of an import that asked for neither step.
    fn report_finish(&mut self, host: &mut dyn Host, report: Option<&SurveyFinishReport>) {
        let Some(report) = report else {
            return;
        };
        let sentences = describe_finish(report);
        if sentences.is_empty() {
            return; // neither step was asked for: the report said so before the import
        }
        for sentence in &sentences {
            host.log(sentence, false);
        }
        // A new paragraph, as QPlainTextEdit::appendPlainText adds one.
        self.form.report.push('\n');
        self.form.report.push_str("Imported:\n");
        self.form.report.push_str(&sentences.join("\n"));
    }

    fn import_now(&mut self, host: &mut dyn Host, ctx: &Context) {
        if self.reader_path {
            self.import_job(host, ctx);
            return;
        }
        let Some(project) = self.project.clone() else {
            return;
        };
        // Asked again rather than kept from the report: the dialog is not modal, and the drawing
        // - its survey codes among it - may have changed since the report was written.
        let finish = self.finish_asked(host.document());
        let options = drawing_options(&self.form.layer, self.form.layer_per_code, &finish);
        let policy = self.policy();
        let mut report = SurveyPointImportReport::default();
        let command = match import_survey_points(
            host.document(),
            &project,
            &options,
            policy,
            Some(&mut report),
        ) {
            Err(error) => {
                self.show_error(host, &error);
                return;
            }
            Ok(None) => {
                self.show_error(
                    host,
                    &Error::new(ErrorCode::InvalidState, "nothing to import"),
                );
                return;
            }
            Ok(Some(command)) => command,
        };
        // ONE command: the points, then - planned as it runs, against the points it has just
        // drawn - their codes and their lines. A point list strings nothing itself (the empty
        // project), so its points are joined by their codes alone.
        let finished = Arc::new(Mutex::new(SurveyFinishReport::default()));
        let Some(whole) = with_survey_finish(
            host.document(),
            Some(command),
            SurveyProject::default(),
            &options,
            finish.options,
            Some(Arc::clone(&finished)),
            SurveyFinishEarlier::default(),
        ) else {
            return;
        };
        if let Err(error) = host.document_mut().execute(whole) {
            self.show_error(host, &error);
            return;
        }
        host.log(self.form.report.trim(), false);
        let finished = finished
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        // "on <layer>" is where the points were drawn; coded, they have since gone to the
        // layers their rules name, which the sentences after say.
        let line = if finished.codes_ran() || finished.linework_ran() {
            format!(
                "Imported {} point(s) from {}, drawn on {} and then finished by the survey codes \
                 as follows: one command - Undo removes it all.",
                report.import.points, self.file_name, options.layer
            )
        } else {
            format!(
                "Imported {} point(s) from {} on {}: one command - Undo removes it.",
                report.import.points, self.file_name, options.layer
            )
        };
        host.log(&line, false);
        self.report_finish(host, Some(&finished));
        frame_views(host);
        // Done, as a wizard's Finish is: the report is in the log, and the next Import Survey
        // Points starts at the first page with the fields kept.
        self.form.blocked = true;
        self.go_to(FILE_PAGE, host);
        self.open = false;
    }

    fn show_error(&mut self, host: &mut dyn Host, error: &Error) {
        let mut text = error.message.clone();
        if !error.context.is_empty() {
            text.push_str(" - ");
            text.push_str(&error.context);
        }
        self.form.message = text;
        self.form.message_is_error = true;
        host.log(&format!("Import Survey Points: {}", error.describe()), true);
    }

    fn show_message(&mut self, text: &str) {
        self.form.message = String::from(text);
        self.form.message_is_error = false;
    }

    // ---- the instrument path ---------------------------------------------------------------

    fn read_with_reader(&mut self, host: &mut dyn Host, ctx: &Context) {
        let path = String::from(self.form.file.trim());
        let name = self.file_name.clone();
        let id = self.format_id.clone();
        // The bytes the File step read, when it read them all; a large file is read on the
        // thread instead.
        let have = self.bytes_complete.then(|| Arc::new(self.bytes.clone()));
        let location = path_from_utf8(&path);
        let folder: PathBuf = std::path::absolute(&location)
            .ok()
            .and_then(|absolute| absolute.parent().map(PathBuf::from))
            .unwrap_or_default();
        self.read_generation += 1;
        let generation = self.read_generation;
        let size = std::fs::metadata(&location).map_or(0, |metadata| metadata.len());
        let background = size > BACKGROUND_READ_BYTES;
        let format = format_registry()
            .find(&id)
            .map_or_else(|_| id.clone(), |descriptor| descriptor.human_name);
        self.form.message.clear();
        // Runs on its own thread for a large file: it touches nothing of this dialog or the
        // drawing, only what it was given, until its result is applied.
        let started = self.form.task.run(
            &format!("Reading {name} ({}) as {format}", size_text(size)),
            background,
            ctx,
            move || {
                let bytes = match have {
                    Some(bytes) => bytes,
                    None => match std::fs::read(location) {
                        Ok(bytes) => Arc::new(bytes),
                        Err(error) => {
                            return WizardWork::Read {
                                generation,
                                result: Err((
                                    make_error(
                                        ErrorCode::FileImportFailure,
                                        "the file cannot be opened",
                                        format!("{path}: {error}"),
                                    ),
                                    false,
                                )),
                            };
                        }
                    },
                };
                // The job's other files - a DBX folder, a RINEX navigation file - by name, from
                // the folder the file is in and nowhere else.
                let options = ReadOptions {
                    siblings: Some(siblings_in_folder(folder, MAX_SURVEY_FILE_BYTES)),
                    ..ReadOptions::default()
                };
                let result = match read_survey(format_registry(), &id, &bytes, &name, &options) {
                    Err(error) => Err((error, true)),
                    Ok(mut read) => {
                        let raw = Arc::new(std::mem::take(&mut read.project));
                        let counts = survey_content_of(&raw);
                        Ok(ReadDone {
                            bytes,
                            read: Arc::new(read),
                            raw,
                            counts,
                        })
                    }
                };
                WizardWork::Read { generation, result }
            },
        );
        if let Some(work) = started {
            self.apply_work(host, ctx, work);
        }
    }

    fn apply_work(&mut self, host: &mut dyn Host, ctx: &Context, work: WizardWork) {
        match work {
            WizardWork::Read { generation, result } => match result {
                Err((error, only_the_latest)) => {
                    if !only_the_latest || generation == self.read_generation {
                        self.show_error(host, &error);
                    }
                }
                Ok(done) => {
                    if generation != self.read_generation {
                        return;
                    }
                    self.source_bytes = Some(done.bytes);
                    self.read = Some(done.read);
                    self.raw = Some(done.raw);
                    self.content_counts = done.counts;
                    self.outcome = None;
                    self.show_content();
                    self.go_to(CONTENT_PAGE, host);
                }
            },
            WizardWork::Reduce {
                generation,
                revision,
                system,
                drawing_points,
                settings,
                then,
                result,
            } => match result {
                Err(error) => {
                    self.form.preview_report.show_note(&format!(
                        "The reduction could not run: {}",
                        error.describe()
                    ));
                    self.show_error(host, &error);
                }
                Ok((outcome, html)) => {
                    if generation != self.read_generation {
                        return;
                    }
                    self.outcome_settings = settings;
                    self.outcome_drawing_points = drawing_points;
                    self.outcome_read = generation;
                    self.outcome_revision = revision;
                    self.outcome_system = system;
                    self.form.preview_report.set_report_html(&html);
                    self.form.import_report.set_report_html(&html);
                    self.show_message(&format!(
                        "Preview: {} point(s); {}; {} observation(s) rejected.",
                        outcome.points.len(),
                        adjustment_summary(&outcome.report),
                        rejected_observations(&outcome.report)
                    ));
                    self.outcome = Some(outcome);
                    match then {
                        Then::Nothing => {}
                        Then::Report => {
                            self.prepare_reader_report(host);
                            self.go_to(REPORT_PAGE, host);
                        }
                        Then::Import => self.import_job(host, ctx),
                    }
                }
            },
        }
    }

    fn show_content(&mut self) {
        let (Some(read), Some(raw)) = (&self.read, &self.raw) else {
            return;
        };
        let counts = &self.content_counts;
        let format = format_registry().find(&read.format_id).map_or_else(
            |_| read.format_id.clone(),
            |descriptor| descriptor.human_name,
        );
        self.form.content_summary = format!(
            "{format} (parser {}) read {} record(s) of {}; {}",
            read.parser_version,
            read.records_read,
            self.file_name,
            if read.records_skipped == 0 {
                String::from("none skipped.")
            } else {
                format!(
                    "{} skipped, each with a warning below.",
                    read.records_skipped
                )
            }
        );
        let number = |n: usize| format!("{n}");
        let mut rows = vec![ContentRow::new("Setups", number(counts.setups))];
        let mut observations = ContentRow::new("Observations", number(counts.observations));
        observations.children = counts
            .observations_by_kind
            .iter()
            .map(|(kind, n)| (kind.clone(), number(*n)))
            .collect();
        rows.push(observations);
        rows.push(ContentRow::new(
            "Points with coordinates",
            number(counts.positioned_points),
        ));
        rows.push(ContentRow::new(
            "Points without coordinates",
            number(counts.unpositioned_points),
        ));
        rows.push(ContentRow::new("Coded features", number(counts.features)));
        let mut control =
            ContentRow::new("Control the file declares", number(counts.control_points));
        control.children = raw
            .control_points
            .iter()
            .map(|point| (point.point_id.clone(), String::new()))
            .collect();
        rows.push(control);
        if counts.traverses > 0 {
            rows.push(ContentRow::new("Traverses", number(counts.traverses)));
        }
        let mut sessions = ContentRow::new("GNSS sessions", number(counts.gnss_sessions));
        sessions.children = raw
            .gnss_sessions
            .iter()
            .map(|session| {
                let mut detail = format!(
                    "receiver {}, antenna {}, {} epoch(s)",
                    if session.receiver_type.is_empty() {
                        "not stated"
                    } else {
                        session.receiver_type.as_str()
                    },
                    if session.antenna.r#type.is_empty() {
                        "not stated"
                    } else {
                        session.antenna.r#type.as_str()
                    },
                    session.epoch_count
                );
                if session.first_epoch.year != 0 {
                    detail.push_str(&format!(
                        ", {} to {}",
                        survey_name(&session.first_epoch),
                        survey_name(&session.last_epoch)
                    ));
                }
                let marker = if session.marker_name.is_empty() {
                    "(no marker name)"
                } else {
                    session.marker_name.as_str()
                };
                (
                    format!("{marker}: {detail}"),
                    format!("{} epoch(s)", session.epoch_count),
                )
            })
            .collect();
        rows.push(sessions);
        let mut siblings =
            ContentRow::new("Files read beside it", number(read.siblings_read.len()));
        siblings.children = read
            .siblings_read
            .iter()
            .map(|sibling| (sibling.name.clone(), size_text(sibling.bytes.len() as u64)))
            .collect();
        rows.push(siblings);
        rows.push(ContentRow::new(
            "Units the file states",
            units_text(&raw.units),
        ));
        rows.push(ContentRow::new(
            "Coordinate system the file declares",
            if raw.coordinate_system.unknown {
                String::from("not stated")
            } else {
                raw.coordinate_system.name.clone()
            },
        ));
        let mut missing = ContentRow::new("Not in the file", number(read.not_carried.len()));
        missing.children = read
            .not_carried
            .iter()
            .map(|line| (line.clone(), String::new()))
            .collect();
        rows.push(missing);
        let mut warnings = ContentRow::new("Warnings", number(read.warnings.len()));
        let listed = read.warnings.len().min(LISTED_WARNINGS);
        warnings.children = read.warnings[..listed]
            .iter()
            .map(|warning| {
                (
                    warning.message.clone(),
                    format!("{} record {}", warning.file_name, warning.record),
                )
            })
            .collect();
        if listed < read.warnings.len() {
            warnings.children.push((
                format!(
                    "{} more, in the reduction report",
                    read.warnings.len() - listed
                ),
                String::from("..."),
            ));
        }
        warnings.error = !read.warnings.is_empty();
        rows.push(warnings);
        // Open what there is to read; the counts alone say the rest.
        for row in &mut rows {
            if matches!(
                row.what.as_str(),
                "Observations" | "Not in the file" | "Warnings" | "Files read beside it"
            ) {
                row.expanded = !row.children.is_empty() && row.children.len() <= 50;
            }
        }
        self.form.content = rows;
        self.form.content_row = None;
    }

    /// The reader states the file's units and has converted to metres and radians; nothing is
    /// transformed - the observations are reduced into the drawing's system, which is what the
    /// reduction's scale factor, geoid and GNSS conversions come from.
    fn prepare_system_for_reader(&mut self, host: &mut dyn Host) {
        let Some(raw) = &self.raw else {
            return;
        };
        self.form.system_fields_enabled = false;
        self.form.system_fields_shown = false;
        let drawing = &host.document().metadata().coordinate_system;
        self.form.system_summary = format!(
            "The file's own units: {} - converted to metres and radians as it was read.\nThe \
             file declares: {}.\nThe observations are reduced into the drawing's coordinate \
             system: {}.\nNothing is transformed.",
            units_text(&raw.units),
            if raw.coordinate_system.unknown {
                String::from("no coordinate system")
            } else {
                raw.coordinate_system.name.clone()
            },
            if drawing.is_empty() {
                String::from(
                    "none set - a local drawing, so no scale factor from a projection, geoid or \
                     GNSS conversion is available",
                )
            } else {
                drawing.clone()
            }
        );
        self.form.system_summary_shown = true;
    }

    fn context_for_reduction(&self, document: &Document) -> Result<SurveyJobContext> {
        let mut context = reduction_context_for(document)?;
        if let Some(read) = &self.read {
            context.input = report_input_for(read, &self.file_name);
        }
        context.created_utc = now_utc();
        Ok(context)
    }

    fn prepare_reduction(&mut self, host: &mut dyn Host) {
        // The options start from the defaults and the control the file declares, once per file
        // read: going back and forth keeps what the person set.
        if self.options_generation != self.read_generation
            && let Some(raw) = &self.raw
        {
            let settings = ReductionSettings {
                control: control_from_file(raw),
                ..ReductionSettings::default()
            };
            self.form.options.set_settings(&settings);
            self.form.options.set_project(raw);
            self.options_generation = self.read_generation;
            self.form.preview_report.show_note(
                "Press Preview to reduce and adjust with these options and read the report here \
                 before importing.",
            );
        }
        // The drawing's points, as they are now.
        match reduction_context_for(host.document()) {
            Ok(context) => self
                .form
                .options
                .set_drawing_points(&context.drawing_points),
            Err(error) => {
                self.form.options.set_drawing_points(&[]);
                self.show_error(host, &error);
            }
        }
    }

    fn preview_is_current(&self, host: &mut dyn Host) -> bool {
        let document = host.document();
        if self.outcome.is_none()
            || self.outcome_read != self.read_generation
            || self.outcome_revision != document.model_revision()
            || self.outcome_system != document.metadata().coordinate_system
        {
            return false;
        }
        self.form
            .options
            .settings()
            .is_ok_and(|settings| settings == self.outcome_settings)
    }

    /// Runs the reduction with the options as they stand; `then` follows once it has succeeded
    /// (the Report step, the import).
    fn run_preview(&mut self, host: &mut dyn Host, ctx: &Context, then: Then) {
        let (Some(raw), Some(_)) = (self.raw.clone(), &self.read) else {
            return;
        };
        let settings = match self.form.options.settings() {
            Ok(settings) => settings,
            Err(error) => {
                self.show_error(host, &error);
                return;
            }
        };
        let context = match self.context_for_reduction(host.document()) {
            Ok(context) => context,
            Err(error) => {
                self.show_error(host, &error);
                return;
            }
        };
        let size = survey_size(&raw);
        let generation = self.read_generation;
        let revision = host.document().model_revision();
        let system = host.document().metadata().coordinate_system.clone();
        let drawing_points = context.drawing_points.clone();
        self.form.message.clear();
        self.form
            .preview_report
            .show_note("Reducing and adjusting...");
        let started = self.form.task.run(
            &format!("Reducing and adjusting {size} observation(s) and point(s)"),
            size > BACKGROUND_OBSERVATIONS,
            ctx,
            move || {
                let result = reduce_for_drawing(&raw, &settings, &context.to_reduction_context())
                    .map(|outcome| {
                        // Rendered here, off the interface thread; cut to what a view can lay
                        // out.
                        let html =
                            render_html(&display_report(&outcome.report, REPORT_ROWS_ON_SCREEN));
                        (Arc::new(outcome), html)
                    });
                WizardWork::Reduce {
                    generation,
                    revision,
                    system,
                    drawing_points,
                    settings,
                    then,
                    result,
                }
            },
        );
        if let Some(work) = started {
            self.apply_work(host, ctx, work);
        }
    }

    fn prepare_reader_report(&mut self, host: &mut dyn Host) {
        let (Some(read), Some(outcome)) = (&self.read, &self.outcome) else {
            self.form.blocked = self.outcome.is_none();
            return;
        };
        let report = &outcome.report;
        let format = format_registry().find(&read.format_id).map_or_else(
            |_| read.format_id.clone(),
            |descriptor| descriptor.human_name,
        );
        let mut text = format!("File: {}\n", self.file_name);
        text.push_str(&format!(
            "Format: {format} (parser {})\n",
            read.parser_version
        ));
        text.push_str(&format!(
            "Records read: {}, skipped: {}, warnings: {}\n",
            read.records_read,
            read.records_skipped,
            read.warnings.len()
        ));
        text.push_str(&format!("Adjustment: {}\n", adjustment_summary(report)));
        text.push_str(&format!(
            "Points computed: {}; observations rejected: {}; reduction warnings: {}\n",
            outcome.points.len(),
            rejected_observations(report),
            report.warnings.len()
        ));
        let finish = self.finish_asked(host.document());
        let options = drawing_options(&self.form.layer, self.form.layer_per_code, &finish);
        text.push_str(&format!(
            "Layer: {}{}\n",
            options.layer,
            if options.layer_per_code {
                ", a layer per code beneath it"
            } else {
                ""
            }
        ));
        text.push_str(&format!(
            "Ids the drawing already has: {}\n",
            self.policy().name()
        ));
        text.push_str(&finish_plan(&finish, true).join("\n"));
        text.push('\n');
        text.push_str(
            "Import keeps the file as a survey job: Survey > Survey Jobs adjusts it again.\n",
        );
        self.form.report = text;
        self.form.import_report_shown = true;
        self.form.blocked = false;
    }

    fn import_job(&mut self, host: &mut dyn Host, ctx: &Context) {
        let (Some(read), Some(raw), Some(source_bytes)) = (
            self.read.clone(),
            self.raw.clone(),
            self.source_bytes.clone(),
        ) else {
            return;
        };
        // The drawing may have changed since the preview (the dialog is not modal): the
        // reduction is run again then, as the preview ran it.
        if !self.preview_is_current(host) {
            self.run_preview(host, ctx, Then::Import);
            return;
        }
        let context = match self.context_for_reduction(host.document()) {
            Ok(context) => context,
            Err(error) => {
                self.show_error(host, &error);
                return;
            }
        };
        // What the job is asked beyond its points, as the boxes stand and the drawing is now:
        // the job's own finish, so the codes and the lines are part of the one command and are
        // kept with the job.
        let finish = self.finish_asked(host.document());
        let options = drawing_options(&self.form.layer, self.form.layer_per_code, &finish);
        let policy = self.policy();
        let job = SurveyJob {
            name: self.file_name.clone(),
            format_id: read.format_id.clone(),
            parser_version: read.parser_version.clone(),
            source_file_name: self.file_name.clone(),
            source_bytes: Vec::clone(&source_bytes),
            sibling_files: read
                .siblings_read
                .iter()
                .map(|sibling| SurveyJobFile {
                    name: sibling.name.clone(),
                    bytes: sibling.bytes.clone(),
                })
                .collect(),
            settings: self.outcome_settings.clone(),
            layer: options.layer.clone(),
            imported_utc: context.created_utc.clone(),
            ..SurveyJob::default()
        };
        let request = SurveyJobImport {
            job,
            raw: SurveyProject::clone(&raw),
            context,
            import_options: options.clone(),
            existing_points: policy,
            finish: finish.options,
        };
        // The reduction the preview ran off the interface thread, not a second one here.
        let reduce = precomputed_reduction(PrecomputedReduction {
            outcome: self.outcome.clone(),
            settings: self.outcome_settings.clone(),
            previous: Vec::new(),
            drawing_points: self.outcome_drawing_points.clone(),
        });
        let command = ImportSurveyJobCommand::new(host.document(), request, Some(reduce));
        let results = command.results();
        if let Err(error) = host.document_mut().execute(Box::new(command)) {
            self.show_error(host, &error);
            return;
        }
        let report = results.report();
        let format = format_registry().find(&read.format_id).map_or_else(
            |_| read.format_id.clone(),
            |descriptor| descriptor.human_name,
        );
        let points = self
            .outcome
            .as_ref()
            .map_or(0, |outcome| outcome.points.len());
        let summary = format!(
            "Imported survey job {} from {} ({format}, parser {}): {points} point(s) on {}; {}; {} \
             observation(s) rejected; {} warning(s). One command - Undo removes it; the reduction \
             report is in Survey > Survey Jobs.",
            results.job_id(),
            self.file_name,
            read.parser_version,
            options.layer,
            report
                .as_ref()
                .map_or_else(|| String::from("-"), adjustment_summary),
            report.as_ref().map_or(0, rejected_observations),
            read.warnings.len() + report.as_ref().map_or(0, |report| report.warnings.len())
        );
        host.log(&summary, false);
        // What the job's finish did; None for a job that asked for neither.
        let finished = results.finish_report();
        self.report_finish(host, finished.as_ref());
        frame_views(host);
        self.form.blocked = true;
        self.go_to(FILE_PAGE, host);
        self.open = false;
    }
}

#[cfg(test)]
mod tests {
    use super::{epsg_field, role_index_of, size_text, typed_columns};

    #[test]
    fn the_column_list_as_typed_and_the_role_each_letter_spells() {
        assert_eq!(typed_columns(" P, n ,E,,Z "), vec!["P", "n", "E", "", "Z"]);
        assert!(typed_columns("  ").is_empty());
        assert_eq!(role_index_of("n"), Some(1));
        assert_eq!(role_index_of("-"), Some(6));
        assert_eq!(role_index_of("NE"), None);
        assert_eq!(role_index_of("Q"), None);
    }

    #[test]
    fn an_epsg_code_and_a_size_are_read_and_said_as_the_wizard_says_them() {
        assert_eq!(epsg_field("EPSG:28356", "x"), Ok(Some(28356)));
        assert_eq!(epsg_field("", "x"), Ok(None));
        assert!(epsg_field("-1", "x").is_err());
        assert_eq!(size_text(812_000), "812 KB");
        assert_eq!(size_text(1), "1 KB");
        assert_eq!(size_text(50_200_000), "50.2 MB");
    }
}
