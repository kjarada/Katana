//! The GDAL Toolbox's Pipeline tab (geo/pipeline_builder.hpp; docs/geoprocessing.md, "Pipelines"):
//! several algorithms chained in one run, `read ! step ! step ! write`.
//!
//! The builder runs nothing itself. Its steps are GDAL's own pipeline steps, read at run time from
//! the pipeline's usage; each step's arguments are an ArgumentForm; and what it writes is the one
//! line `GDAL pipeline "<text>" FROM input <source> TO <kind> <name>`, handed to the window's one
//! executor (geo_dialog_support's GeoRunPanel). The text can be typed instead: an edit is read
//! back into the steps, and a text the steps cannot follow still runs as typed when the executor
//! takes it.
//!
//! ```text
//! gdalPipeline            the tab
//! gdalPipelineKind ...    what the pipeline reads (BindingPicker named from gdalPipeline)
//! gdalPipelineSteps       read, the steps in order, write
//! gdalPipelineAdd         the steps that read what the pipeline makes at that point
//! gdalPipelineUp, gdalPipelineDown, gdalPipelineRemove
//! gdalStep.<argument>     the selected step's arguments; gdalPipelineAdvanced
//! gdalPipelineText        the pipeline as GDAL reads it
//! gdalPipelineOutputKind, gdalPipelineOutputName
//! gdalPipelineScript, gdalPipelineSave   the line kept in a script (.kcs)
//! gdalPipelineCommand, gdalPipelinePreview, gdalPipelineRun, gdalPipelineStatus,
//! gdalPipelineReply       (GeoRunPanel)
//! ```

use std::collections::BTreeSet;
use std::io::Write;
use std::sync::OnceLock;

use egui::Ui;
use katana_core::error::make_error;
use katana_core::json::Json;
use katana_core::{ErrorCode, Result, Status};
use katana_gis::processing as gp;

use super::argument_form::ArgumentForm;
use super::binding_picker::{BindingKind, BindingPicker};
use super::geo_dialog_support::{GeoRunPanel, line_word};
use super::terrain_dialog_parts::{field, invalid, one_column, row};
use crate::drive::controls::custom;
use crate::drive::registry::{ControlKind, ControlRecord};
use crate::drive::{Choice, button, choice, line, rows};
use crate::host::Host;
use crate::ifc_dialogs::{get_save_file_name, to_native_separators};
use crate::survey::survey_dialogs::hidden;

/// A step a pipeline can take.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PipelineStepKind {
    /// "hillshade"
    pub name: String,
    /// The catalogue algorithm: {"raster", "hillshade"}.
    pub path: Vec<String>,
    /// processing::DatasetKind bits.
    pub reads: u32,
    pub makes: u32,
    pub description: String,
}

/// One step of a pipeline: the algorithm and its words (--name=value).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PipelineStep {
    pub path: Vec<String>,
    pub words: Vec<String>,
}

/// What the Pipeline tab's fields hold.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PipelineForm {
    pub text: String,
    pub source: String,
    pub source_error: String,
    /// LAYER, REFERENCE, FILE
    pub output_kind: String,
    pub output_name: String,
}

const RASTER: u32 = gp::DatasetKind::Raster as u32;
const VECTOR: u32 = gp::DatasetKind::Vector as u32;

fn strings(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| String::from(*word)).collect()
}

/// The kinds the first dataset input reads and the output makes.
fn kinds_of(spec: &gp::AlgorithmSpec) -> (u32, u32) {
    let mut reads = 0;
    let mut makes = 0;
    for arg in &spec.args {
        if arg.is_dataset() && arg.is_input && !arg.is_output && reads == 0 {
            reads = arg.dataset_kinds;
        }
        if arg.is_dataset() && arg.is_output && makes == 0 {
            makes = arg.dataset_kinds;
        }
    }
    (reads, makes)
}

fn step_at(path: &[String]) -> Option<&'static PipelineStepKind> {
    pipeline_steps().iter().find(|step| step.path == path)
}

/// The first step called `name` that reads `kind` (0: either).
fn step_reading(name: &str, kind: u32) -> Option<&'static PipelineStepKind> {
    pipeline_steps()
        .iter()
        .find(|step| step.name == name && (kind == 0 || step.reads & kind != 0))
}

/// A step's words into its form: --name=value, --name for a flag. `Err` says what did not fit.
fn load_words(form: &mut ArgumentForm, words: &[String]) -> std::result::Result<(), String> {
    for word in words {
        let body = word.strip_prefix("--").unwrap_or(word);
        let (name, value) = match body.split_once('=') {
            Some((name, value)) => (name, value),
            None => (body, "true"),
        };
        if !form.set_value(name, value) {
            return Err(format!("'{value}' does not fit {name}"));
        }
    }
    Ok(())
}

/// The argument of `spec` a word names: a long name, an alias or a short name.
fn argument_named<'a>(spec: &'a gp::AlgorithmSpec, name: &str) -> Option<&'a gp::ArgSpec> {
    spec.args.iter().find(|arg| {
        arg.name == name
            || (!arg.short_name.is_empty() && arg.short_name == name)
            || arg.aliases.iter().any(|alias| alias == name)
    })
}

fn kind_word(kind: u32) -> &'static str {
    if kind == RASTER {
        "a raster"
    } else if kind == VECTOR {
        "features"
    } else {
        "data"
    }
}

/// GDAL's usage JSON with its non-JSON numbers - a default of Infinity (vector grid's radius), NaN
/// - made null: no JSON reader takes them, and a reader that stops there sees no steps at all.
fn strict_json(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut in_string = false;
    let mut at = 0;
    while at < bytes.len() {
        let byte = bytes[at];
        if in_string {
            out.push(byte);
            if byte == b'\\' && at + 1 < bytes.len() {
                at += 1;
                out.push(bytes[at]);
            } else if byte == b'"' {
                in_string = false;
            }
            at += 1;
            continue;
        }
        if byte == b'"' {
            in_string = true;
            out.push(byte);
            at += 1;
            continue;
        }
        let rest = &bytes[at..];
        let bare = ["-Infinity", "Infinity", "NaN"]
            .iter()
            .find(|word| rest.starts_with(word.as_bytes()));
        match bare {
            Some(word) => {
                out.extend_from_slice(b"null");
                at += word.len();
            }
            None => {
                out.push(byte);
                at += 1;
            }
        }
    }
    out
}

/// The names GDAL's pipeline usage lists as steps, as it lists them.
pub fn pipeline_step_names() -> Vec<String> {
    let Ok(spec) = gp::describe(&strings(&["pipeline"])) else {
        return Vec::new();
    };
    let Ok(usage) = Json::parse(strict_json(&spec.usage_json)) else {
        return Vec::new();
    };
    let Ok(steps) = usage.at("pipeline_algorithms") else {
        return Vec::new();
    };
    steps
        .iter()
        .map(|step| {
            String::from(
                step.at("name")
                    .ok()
                    .and_then(|name| name.as_str())
                    .unwrap_or(""),
            )
        })
        .collect()
}

/// Every step offered, in GDAL's order: in its pipeline usage's pipeline_algorithms and a leaf of
/// the catalogue under raster or vector. Read and write are the pipeline's own ends; external and
/// tee are not offered. Read once.
pub fn pipeline_steps() -> &'static [PipelineStepKind] {
    static STEPS: OnceLock<Vec<PipelineStepKind>> = OnceLock::new();
    STEPS.get_or_init(|| {
        // A pipeline's own ends, a step that runs a program, and one that is a pipeline of its own
        // are not steps to offer.
        let not_offered = ["read", "write", "external", "tee"];
        let mut made = Vec::new();
        let mut seen = BTreeSet::new();
        for name in pipeline_step_names() {
            if not_offered.contains(&name.as_str()) || !seen.insert(name.clone()) {
                continue;
            }
            for group in ["raster", "vector"] {
                let path = vec![String::from(group), name.clone()];
                let leaf = gp::catalogue()
                    .iter()
                    .any(|info| !info.container && info.path == path);
                if !leaf {
                    continue;
                }
                let Ok(spec) = gp::describe(&path) else {
                    continue;
                };
                let (reads, makes) = kinds_of(&spec);
                // A step that prints (info) ends a pipeline without a write, and this builder
                // always writes.
                if makes == 0 {
                    continue;
                }
                made.push(PipelineStepKind {
                    name: name.clone(),
                    path,
                    reads,
                    makes,
                    description: spec.info.description,
                });
            }
        }
        made
    })
}

fn last_of(path: &[String]) -> &str {
    path.last().map_or("", String::as_str)
}

/// "read ! hillshade --zfactor=2 ! write". InvalidArgument for a word with a blank or a double
/// quote, which a pipeline's text on one line cannot carry.
pub fn pipeline_text(steps: &[PipelineStep]) -> Result<String> {
    let mut text = String::from("read");
    for step in steps {
        let name = last_of(&step.path);
        text.push_str(&format!(" ! {name}"));
        for word in &step.words {
            if word.contains('"') || word.contains(' ') {
                let argument = word.split('=').next().unwrap_or("").replace('"', "");
                return Err(invalid(format!(
                    "{name}: {argument} holds a blank, which a pipeline on one line cannot carry"
                )));
            }
            text.push_str(&format!(" {word}"));
        }
    }
    Ok(text + " ! write")
}

/// QString::section(' ', 0, 0): the text before its first blank.
fn first_field(text: &str) -> &str {
    text.split(' ').next().unwrap_or("")
}

/// A pipeline's text read into steps, each value checked by the step's own form: read first and
/// write last, bare (the source and the target name their datasets); `--name=value`, `--name
/// value`, `-n value`, a flag, and values given by position in GDAL's order. `reads` is what the
/// source is (a DatasetKind bit, or 0 for either), which chooses between a raster and a vector
/// step of one name. InvalidArgument naming what it cannot read.
pub fn parse_pipeline(text: &str, reads: u32) -> Result<Vec<PipelineStep>> {
    let parts: Vec<&str> = text.split('!').map(str::trim).collect();
    let first = parts.first().copied().unwrap_or("");
    let last = parts.last().copied().unwrap_or("");
    if parts.len() < 2 || first_field(first) != "read" || first_field(last) != "write" {
        return Err(invalid("a pipeline is read ! <steps> ! write"));
    }
    if first != "read" || last != "write" {
        return Err(invalid(
            "read and write take their datasets from the source and the output, so the text \
             names none",
        ));
    }
    let mut steps = Vec::new();
    let mut kind = reads;
    for part in &parts[1..parts.len() - 1] {
        let words: Vec<&str> = part.split_whitespace().collect();
        let Some(&name) = words.first() else {
            return Err(invalid("a step between two ! is empty"));
        };
        if name == "external" {
            return Err(invalid("external runs a program and is refused"));
        }
        let Some(chosen) = step_reading(name, kind) else {
            return Err(invalid(format!(
                "no pipeline step {name} reads {} here",
                kind_word(kind)
            )));
        };
        let spec = gp::describe(&chosen.path)?;
        let mut form = ArgumentForm::new("gdalScratch.", "gdalScratchAdvanced");
        form.set_algorithm(&spec, &BTreeSet::new(), true);
        let positional: Vec<&str> = spec
            .args
            .iter()
            .filter(|arg| arg.positional && !arg.is_dataset() && form.has_control(&arg.name))
            .map(|arg| arg.name.as_str())
            .collect();
        let mut next_positional = 0;
        let mut at = 1;
        while at < words.len() {
            let word = words[at];
            let letters: Vec<char> = word.chars().collect();
            let short = letters.len() == 2 && letters[0] == '-' && letters[1].is_alphabetic();
            let mut value: Option<&str> = None;
            let arg_name = if word.starts_with("--") || short {
                let body = &word[if word.starts_with("--") { 2 } else { 1 }..];
                match body.split_once('=') {
                    Some((name, given)) => {
                        value = Some(given);
                        name
                    }
                    None => body,
                }
            } else {
                let Some(&by_position) = positional.get(next_positional) else {
                    return Err(invalid(format!(
                        "{name} takes no value by position here: {word}"
                    )));
                };
                next_positional += 1;
                value = Some(word);
                by_position
            };
            let arg = argument_named(&spec, arg_name).filter(|arg| form.has_control(&arg.name));
            let Some(arg) = arg else {
                return Err(invalid(format!(
                    "{name} takes no argument {arg_name} in a pipeline"
                )));
            };
            let value = match value {
                Some(value) => value,
                None if arg.r#type == gp::ArgType::Boolean => "true",
                None if at + 1 < words.len() => {
                    at += 1;
                    words[at]
                }
                None => {
                    return Err(invalid(format!("{name}: {arg_name} needs a value")));
                }
            };
            if !form.set_value(&arg.name, value) {
                return Err(invalid(format!(
                    "{name}: '{value}' does not fit {}",
                    arg.name
                )));
            }
            at += 1;
        }
        let checked = form
            .words()
            .map_err(|error| invalid(format!("{name}: {}", error.message)))?;
        steps.push(PipelineStep {
            path: chosen.path.clone(),
            words: checked,
        });
        kind = chosen.makes;
    }
    Ok(steps)
}

/// GDAL pipeline "<text>" FROM input <source> TO <kind> <name>. InvalidArgument for no source, a
/// text the executor refuses (a step called external), an empty text, and a layer or file path
/// left empty.
pub fn pipeline_line(form: &PipelineForm) -> Result<String> {
    let text = form.text.trim();
    if text.is_empty() {
        return Err(invalid("the pipeline has no steps"));
    }
    if text.contains('"') {
        return Err(invalid(
            "the pipeline holds a double quote, which a command line cannot carry",
        ));
    }
    if let Err(refused) = gp::check_tokens(&strings(&["pipeline"]), &[String::from(text)]) {
        return Err(invalid(refused.message));
    }
    if !form.source_error.is_empty() {
        return Err(invalid(format!("the source: {}", form.source_error)));
    }
    let source = form.source.trim();
    if source.is_empty() {
        return Err(invalid("choose what the pipeline reads"));
    }
    let mut line = format!("GDAL pipeline \"{text}\" FROM input {source}");
    let name = form.output_name.trim();
    if form.output_kind == "LAYER" || form.output_kind == "FILE" {
        if name.is_empty() {
            return Err(invalid(if form.output_kind == "LAYER" {
                "give the layer's path"
            } else {
                "give the file's path"
            }));
        }
        line.push_str(&format!(
            " TO {} {}",
            form.output_kind,
            line_word(name, "the output")?
        ));
    } else if form.output_kind == "REFERENCE" {
        line.push_str(" TO REFERENCE");
        if !name.is_empty() {
            line.push_str(&format!(" {}", line_word(name, "the raster's name")?));
        }
    }
    Ok(line)
}

/// The output's default name for a kind.
fn default_output_name(kind: &str) -> &'static str {
    match kind {
        "LAYER" => "gis/pipeline",
        "REFERENCE" => "pipeline",
        _ => "",
    }
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

/// The Pipeline tab (see the module comment).
pub struct PipelineBuilder {
    steps: Vec<PipelineStep>,
    source: BindingPicker,
    /// The row selected in the list (0 is read); None for none.
    row: Option<usize>,
    add: Choice,
    step_form: ArgumentForm,
    step_form_visible: bool,
    text: String,
    /// Each output kind offered: its word (LAYER) and what the choice shows.
    output_words: Vec<&'static str>,
    output_texts: Vec<String>,
    output_kind: Choice,
    output_name: String,
    script: String,
    panel: GeoRunPanel,
    /// The text as typed when it could not be read into steps: the line runs it as it is.
    typed: String,
}

impl PipelineBuilder {
    /// The tab over the window's reference rasters, surfaces, drawing and views.
    pub fn new(host: &mut dyn Host) -> Self {
        let mut builder = Self {
            steps: Vec::new(),
            source: BindingPicker::new(
                "gdalPipeline",
                BindingKind::DRAWING | BindingKind::RASTER | BindingKind::SURFACE | BindingKind::FILE,
                host,
            ),
            row: None,
            add: Choice::at(0),
            step_form: ArgumentForm::new("gdalStep.", "gdalPipelineAdvanced"),
            step_form_visible: true,
            text: String::new(),
            output_words: Vec::new(),
            output_texts: Vec::new(),
            output_kind: Choice::default(),
            output_name: String::new(),
            script: String::new(),
            panel: GeoRunPanel::new("gdalPipeline", true),
            typed: String::new(),
        };
        builder.rebuild(true);
        builder.select(0);
        builder
    }

    pub fn steps(&self) -> &[PipelineStep] {
        &self.steps
    }

    pub fn run_panel(&mut self) -> &mut GeoRunPanel {
        &mut self.panel
    }

    pub fn step_form(&mut self) -> &mut ArgumentForm {
        &mut self.step_form
    }

    pub fn source(&mut self) -> &mut BindingPicker {
        &mut self.source
    }

    /// The list's rows: read, the steps in order with their words, write.
    pub fn rows(&self) -> Vec<String> {
        let mut listed = vec![String::from("read")];
        for step in &self.steps {
            let mut label = String::from(last_of(&step.path));
            if !step.words.is_empty() {
                label.push_str(&format!("  {}", step.words.join(" ")));
            }
            listed.push(label);
        }
        listed.push(String::from("write"));
        listed
    }

    fn output_word(&self) -> &'static str {
        self.output_kind
            .index
            .and_then(|at| self.output_words.get(at))
            .copied()
            .unwrap_or("")
    }

    pub fn form(&self) -> PipelineForm {
        let (source, source_error) = match self.source.words() {
            Ok(words) => (words, String::new()),
            Err(error) => (String::new(), error.message),
        };
        PipelineForm {
            text: self.text.clone(),
            source,
            source_error,
            output_kind: String::from(self.output_word()),
            output_name: self.output_name.clone(),
        }
    }

    fn source_kind(&self) -> u32 {
        match self.source.kind() {
            BindingKind::DRAWING => VECTOR,
            BindingKind::RASTER | BindingKind::SURFACE => RASTER,
            // A file may be either.
            _ => 0,
        }
    }

    fn kind_after(&self, count: usize) -> u32 {
        let mut kind = self.source_kind();
        for step in self.steps.iter().take(count) {
            if let Some(found) = step_at(&step.path) {
                kind = found.makes;
            }
        }
        kind
    }

    /// Whether the row selected is a step: neither read nor write.
    fn middle(&self) -> Option<usize> {
        self.row
            .filter(|row| *row >= 1 && *row <= self.steps.len())
    }

    /// Where a step added now goes: after the step selected; after the last when read or write is.
    fn insert_at(&self) -> usize {
        self.middle().unwrap_or(self.steps.len())
    }

    /// The steps gdalPipelineAdd offers after "Add a step...": those that read what the pipeline
    /// makes at the place a step would be added.
    pub fn offered(&self) -> Vec<String> {
        let kind = self.kind_after(self.insert_at());
        let mut listed = BTreeSet::new();
        pipeline_steps()
            .iter()
            .filter(|step| kind == 0 || step.reads & kind != 0)
            .filter(|step| listed.insert(step.name.clone()))
            .map(|step| step.name.clone())
            .collect()
    }

    /// Adds the step `name` after the one selected (before write), and selects it. False when no
    /// step of that name reads what the pipeline makes there.
    pub fn add_step(&mut self, name: &str) -> bool {
        let at = self.insert_at();
        let kind = self.kind_after(at);
        let Some(chosen) = step_reading(name, kind) else {
            self.panel.set_status(
                &format!(
                    "No pipeline step {name} reads {} there.",
                    kind_word(kind)
                ),
                true,
            );
            return false;
        };
        self.steps.insert(
            at,
            PipelineStep {
                path: chosen.path.clone(),
                words: Vec::new(),
            },
        );
        self.typed.clear();
        self.rebuild(true);
        self.select(at + 1);
        true
    }

    /// Selects the step at `index` (0 is read). The step's arguments are shown when the row
    /// selected changes, as the list's currentRowChanged did.
    pub fn select(&mut self, index: usize) {
        let row = Some(index.min(self.steps.len() + 1));
        if row != self.row {
            self.row = row;
            self.show_step();
        }
    }

    /// The row selected (0 is read); None for none.
    pub fn selected(&self) -> Option<usize> {
        self.row
    }

    fn rebuild(&mut self, text: bool) {
        // The list is refilled with the row kept, and nobody told: the row is only held to what
        // there is.
        if let Some(row) = self.row {
            self.row = Some(row.min(self.steps.len() + 1));
        }
        if text && self.typed.is_empty() {
            match pipeline_text(&self.steps) {
                Ok(made) => self.text = made,
                Err(error) => {
                    self.text.clear();
                    self.panel.set_status(&error.message, true);
                }
            }
        }
        // Where the result can go, as the last step makes it.
        let kind = self.kind_after(self.steps.len());
        let kept = self.output_word();
        self.output_words.clear();
        self.output_texts.clear();
        let mut offer = |word: &'static str, shown: &str| {
            self.output_words.push(word);
            self.output_texts.push(String::from(shown));
        };
        if kind != RASTER {
            offer("LAYER", "Layer");
        }
        if kind != VECTOR {
            offer("REFERENCE", "Reference raster");
        }
        offer("FILE", "File");
        let index = self.output_words.iter().position(|word| *word == kept);
        self.output_kind.index = Some(index.unwrap_or(0));
        if index.is_none() {
            self.output_name = String::from(default_output_name(self.output_word()));
        }
    }

    fn show_step(&mut self) {
        let middle = self.middle();
        self.step_form_visible = middle.is_some();
        let Some(row) = middle else {
            return;
        };
        let step = self.steps[row - 1].clone();
        let spec = match gp::describe(&step.path) {
            Ok(spec) => spec,
            Err(error) => {
                self.panel.set_status(&error.message, true);
                return;
            }
        };
        self.step_form.set_algorithm(&spec, &BTreeSet::new(), true);
        if let Err(why) = load_words(&mut self.step_form, &step.words) {
            self.panel.set_status(&why, true);
        }
    }

    fn step_edited(&mut self) {
        let Some(row) = self.middle() else {
            return;
        };
        match self.step_form.words() {
            Err(error) => {
                let name = String::from(last_of(&self.steps[row - 1].path));
                self.panel
                    .set_status(&format!("{name}: {}", error.message), true);
                self.typed.clear();
                self.text.clear();
            }
            Ok(words) => {
                self.steps[row - 1].words = words;
                self.typed.clear();
                self.rebuild(true);
            }
        }
    }

    fn text_edited(&mut self) {
        let text = self.text.clone();
        match parse_pipeline(&text, self.source_kind()) {
            Err(error) => {
                // Run as typed when it can be; the steps stay as they were, and the status says
                // why - and, when the line itself is refused (an external step), that it does not
                // run, beside the disabled Run.
                self.typed = text;
                let ending = match pipeline_line(&self.form()) {
                    Ok(_) => String::from(". It runs as typed."),
                    Err(refused) => format!(". It cannot run: {}.", refused.message),
                };
                self.panel.set_status(
                    &format!(
                        "The text is not read into steps: {}{ending}",
                        error.message
                    ),
                    true,
                );
            }
            Ok(parsed) => {
                self.typed.clear();
                self.steps = parsed;
                self.rebuild(false);
                self.panel.set_status("The steps follow the text.", false);
                self.select(1);
            }
        }
    }

    fn move_step(&mut self, by: isize) {
        let Some(row) = self.middle() else {
            return;
        };
        let Some(to) = row.checked_add_signed(by) else {
            return;
        };
        if to < 1 || to > self.steps.len() {
            return;
        }
        self.steps.swap(row - 1, to - 1);
        self.typed.clear();
        self.rebuild(true);
        self.select(to);
    }

    fn remove_step(&mut self) {
        let Some(row) = self.middle() else {
            return;
        };
        self.steps.remove(row - 1);
        self.typed.clear();
        self.rebuild(true);
        self.select(row.min(self.steps.len()));
    }

    /// Writes the line into the script `path` (.kcs), after what is there.
    pub fn save_to(&self, path: &str) -> Status {
        let line = pipeline_line(&self.form())?;
        let failed = |error: &std::io::Error| {
            make_error(
                ErrorCode::FileExportFailure,
                format!("cannot write {path}"),
                file_error_text(error),
            )
        };
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(path)
            .map_err(|error| failed(&error))?;
        // A text file's line end, as QIODevice::Text writes it.
        let end = if cfg!(windows) { "\r\n" } else { "\n" };
        file.write_all(format!("{line}{end}").as_bytes())
            .map_err(|error| failed(&error))
    }

    fn save(&mut self, host: &dyn Host) {
        let mut path = String::from(self.script.trim());
        if path.is_empty() {
            if host.headless() {
                self.panel.set_status(
                    "A headless session opens no file dialog: fill gdalPipelineScript with the \
                     script's path instead.",
                    true,
                );
                return;
            }
            path = get_save_file_name(
                "Save the Pipeline to a Script",
                "",
                "Katana scripts (*.kcs);;All files (*)",
            );
            if path.is_empty() {
                return;
            }
            self.script = to_native_separators(&path);
        }
        if let Err(error) = self.save_to(&path) {
            self.panel.set_status(&error.describe(), true);
            return;
        }
        self.panel.set_status(
            &format!(
                "Saved to {}; File > Run Script replays it.",
                to_native_separators(&path)
            ),
            false,
        );
    }

    pub fn reload(&mut self, host: &dyn Host) {
        self.source.reload(host);
    }

    /// Draws the tab, running its line when Run or Preview is pressed.
    pub fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        let outer = ui.vertical(|ui| self.contents(ui, host));
        custom(
            ui,
            &outer.response,
            ControlRecord::new("gdalPipeline", ControlKind::Other),
        );
    }

    fn contents(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        let mut source_changed = false;
        let mut picked_row = false;
        let mut added: Option<String> = None;
        let mut up = false;
        let mut down = false;
        let mut remove = false;
        let mut step_changed = false;
        ui.horizontal_top(|ui| {
            ui.group(|ui| {
                ui.vertical(|ui| {
                    ui.label("Read");
                    source_changed = self.source.ui(ui, &*host);
                });
            });
            ui.group(|ui| {
                ui.vertical(|ui| {
                    ui.label("Steps");
                    let listed = one_column(&self.rows());
                    let mut row = self.row;
                    if rows(ui, "gdalPipelineSteps", &listed, &mut row)
                        .on_hover_text("read, the steps in order, write")
                        .changed()
                    {
                        picked_row = true;
                    }
                    let middle = self.middle();
                    ui.horizontal(|ui| {
                        let mut items = vec![String::from("Add a step...")];
                        items.extend(self.offered());
                        self.add.index = Some(0);
                        if choice(ui, "gdalPipelineAdd", &items, &mut self.add)
                            .on_hover_text(
                                "GDAL's pipeline steps that read what the pipeline makes at that \
                                 point",
                            )
                            .changed()
                            && let Some(at) = self.add.index.filter(|at| *at > 0)
                        {
                            added = items.get(at).cloned();
                        }
                        self.add.index = Some(0);
                        let steps = self.steps.len();
                        up = ui
                            .add_enabled_ui(middle.is_some_and(|row| row > 1), |ui| {
                                button(ui, "gdalPipelineUp", "Up").clicked()
                            })
                            .inner;
                        down = ui
                            .add_enabled_ui(middle.is_some_and(|row| row < steps), |ui| {
                                button(ui, "gdalPipelineDown", "Down").clicked()
                            })
                            .inner;
                        remove = ui
                            .add_enabled_ui(middle.is_some(), |ui| {
                                button(ui, "gdalPipelineRemove", "Remove").clicked()
                            })
                            .inner;
                    });
                    if picked_row && let Some(row) = row {
                        self.select(row);
                    }
                });
            });
            ui.group(|ui| {
                ui.vertical(|ui| {
                    ui.label("The step's arguments");
                    step_changed = if self.step_form_visible {
                        self.step_form.ui(ui)
                    } else {
                        hidden(ui, "gdalPipelineStepForm", |ui| self.step_form.ui(ui))
                    };
                });
            });
        });
        let text_changed = row(ui, "Pipeline:", |ui| {
            line(ui, "gdalPipelineText", &mut self.text)
                .on_hover_text(
                    "The pipeline as GDAL reads it; an edit is read back into the steps",
                )
                .changed()
        });
        let kind_changed = row(ui, "Write to:", |ui| {
            let changed = choice(
                ui,
                "gdalPipelineOutputKind",
                &self.output_texts,
                &mut self.output_kind,
            )
            .changed();
            line(ui, "gdalPipelineOutputName", &mut self.output_name);
            changed
        });
        let save = row(ui, "Recipe:", |ui| {
            field(
                ui,
                "gdalPipelineScript",
                &mut self.script,
                "a script (.kcs) to keep the line in",
            );
            button(ui, "gdalPipelineSave", "Save to Script")
                .on_hover_text("Adds the line to the script; File > Run Script replays it")
                .clicked()
        });

        // What each edit does, as its signal did.
        if let Some(text) = self.source.take_said() {
            self.panel.set_status(&text, true);
        }
        if source_changed {
            self.rebuild(false);
        }
        if let Some(name) = added {
            let _ = self.add_step(&name);
        }
        if up {
            self.move_step(-1);
        }
        if down {
            self.move_step(1);
        }
        if remove {
            self.remove_step();
        }
        if step_changed {
            self.step_edited();
        }
        // Any edit of the text - typed, or filled by a headless run - but not the builder's own
        // writing of it.
        if text_changed {
            self.text_edited();
        }
        if kind_changed {
            self.output_name = String::from(default_output_name(self.output_word()));
        }
        if save {
            self.save(&*host);
        }
        let made = pipeline_line(&self.form());
        if self.panel.ui(ui, host, &made).is_some() {
            self.reload(&*host);
        }
    }
}
