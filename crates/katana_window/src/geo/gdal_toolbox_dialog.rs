//! GIS > GDAL Toolbox (geo/gdal_toolbox_dialog.hpp; docs/geoprocessing.md, "The window"): every
//! algorithm GDAL has, from its own catalogue, each with a form built from what GDAL declares of
//! its arguments - nothing here is written by hand for an algorithm.
//!
//! The dialog runs nothing itself. It writes the GDAL line its fields describe - shown, as it will
//! run, in gdalToolboxCommand - and Run hands that line to the window's one executor as if it had
//! been typed (geo_dialog_support's GeoRunPanel). The line is made by a pure function of the
//! fields, tested without a window.
//!
//! ```text
//! gdalToolboxDialog:
//!   gdalToolboxTabs        Algorithm | Pipeline (pipeline_builder)
//!   gdalToolboxSearch      a name, an alias or a word of the description
//!   gdalToolboxTree        the catalogue, groups and their algorithms
//!   gdalToolboxTitle, gdalToolboxDescription, gdalToolboxHelp
//!   gdalToolboxInputs      a picker for each dataset read: gdalInput.<argument>.Kind ...
//!   gdalToolboxArguments   gdalArg.<argument>, gdalToolboxAdvanced (argument_form)
//!   gdalToolboxOutput      gdalOutput.kind, gdalOutput.name, gdalOutput.format,
//!                          gdalOutput.overwrite
//!   gdalToolboxConfirm     for an algorithm that changes or removes data that exists
//!   gdalToolboxCommand, gdalToolboxPreview, gdalToolboxRun, gdalToolboxStatus, gdalToolboxReply
//! ```

use std::collections::{BTreeMap, BTreeSet};

use egui::Ui;
use katana_core::Result;
use katana_gis::processing as gp;

use super::argument_form::ArgumentForm;
use super::binding_picker::{BindingList, BindingPicker, binding_kinds_for};
use super::geo_dialog_support::{GeoRunPanel, line_word};
use super::pipeline_builder::PipelineBuilder;
use super::terrain_dialog_parts::{Framed, invalid, row, show_framed};
use crate::customisation::DocumentWatcher;
use crate::drive::controls::custom;
use crate::drive::mailbox::{Act, fill_disabled, lists_no, no_button};
use crate::drive::registry::{ControlKind, ControlRecord, record, settle, take};
use crate::drive::{Choice, LineOptions, button, check, choice, label, line, line_with, tabs};
use crate::host::Host;
use crate::shell::DialogFrame;
use crate::survey::survey_dialogs::{hidden, offstage};

/// Where the last algorithm chosen is kept, per user.
const LAST_ALGORITHM_KEY: &str = "geoprocessing/toolboxAlgorithm";

/// The arguments the output's target says instead of the form: its format and whether a file
/// already there is replaced or added to.
const SAID_BY_THE_TARGET: [&str; 6] = [
    "output-format",
    "overwrite",
    "append",
    "update",
    "overwrite-layer",
    "upsert",
];

/// One dataset input of the form, in GDAL's order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GdalToolboxInput {
    pub arg: String,
    /// Whether it is the first required one.
    pub first_required: bool,
    pub required: bool,
    /// Its source clauses (none: not given).
    pub sources: Vec<String>,
    pub error: String,
}

/// What the Algorithm tab's fields hold.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GdalToolboxForm {
    /// {"raster", "hillshade"}
    pub path: Vec<String>,
    /// ArgumentForm::words, or why they are none.
    pub arguments: Vec<String>,
    pub arguments_error: String,
    pub inputs: Vec<GdalToolboxInput>,
    /// The output: LAYER, REFERENCE, FILE, SURFACE, or empty when the algorithm writes no dataset.
    pub output_kind: String,
    pub output_name: String,
    pub output_format: String,
    pub overwrite: bool,
    /// Whether the algorithm is Confirm, and whether Confirm is ticked.
    pub needs_confirm: bool,
    pub confirm: bool,
}

/// The output's default name for a kind: gis/<last word> for a layer, the last word for a raster
/// or a surface; none for a file, which must be named.
fn default_name(path: &[String], kind: &str) -> String {
    let Some(last) = path.last() else {
        return String::new();
    };
    match kind {
        "FILE" => String::new(),
        "LAYER" => format!("gis/{last}"),
        _ => last.clone(),
    }
}

/// The GDAL line `form` describes, exactly as it would be typed. InvalidArgument naming the field
/// for an argument that does not read, a required input not given, a source the controls cannot
/// say, a layer or file path left empty, and - until it is ticked - a Confirm algorithm's Confirm.
pub fn gdal_toolbox_line(form: &GdalToolboxForm) -> Result<String> {
    if form.path.is_empty() {
        return Err(invalid("choose an algorithm first"));
    }
    if !form.arguments_error.is_empty() {
        return Err(invalid(form.arguments_error.as_str()));
    }
    let path = gp::path_text(&form.path);
    let mut line = format!("GDAL {path}");
    for word in &form.arguments {
        line.push_str(&format!(" {word}"));
    }
    for input in &form.inputs {
        let arg = input.arg.as_str();
        if !input.error.is_empty() {
            return Err(invalid(format!("{arg}: {}", input.error)));
        }
        if input.sources.is_empty() {
            if input.required {
                return Err(invalid(format!("choose where {arg} comes from")));
            }
            continue;
        }
        // The first required input is the one a FROM without a name binds; several datasets, or
        // any other input, name their argument.
        let unnamed = input.first_required && input.sources.len() == 1;
        for source in &input.sources {
            if unnamed {
                line.push_str(&format!(" FROM {source}"));
            } else {
                line.push_str(&format!(" FROM {arg} {source}"));
            }
        }
    }
    let name = form.output_name.trim();
    let kind = form.output_kind.as_str();
    if kind == "LAYER" || kind == "FILE" || kind == "SURFACE" {
        if name.is_empty() {
            return Err(invalid(match kind {
                "LAYER" => "give the layer's path",
                "FILE" => "give the file's path",
                _ => "give the surface's name",
            }));
        }
        line.push_str(&format!(" TO {kind} {}", line_word(name, "the output")?));
        let format = form.output_format.trim();
        if kind == "FILE" && !format.is_empty() {
            line.push_str(&format!(" FORMAT {}", line_word(format, "the format")?));
        }
    } else if kind == "REFERENCE" {
        line.push_str(" TO REFERENCE");
        if !name.is_empty() {
            line.push_str(&format!(" {}", line_word(name, "the raster's name")?));
        }
    }
    if form.needs_confirm {
        if !form.confirm {
            return Err(invalid(format!(
                "tick Confirm: {path} changes or removes data that already exists"
            )));
        }
        line.push_str(" CONFIRM");
    }
    if form.overwrite {
        line.push_str(" OVERWRITE");
    }
    Ok(line)
}

/// A row of the catalogue's tree: a group or an algorithm.
#[derive(Clone, Debug, PartialEq, Eq)]
struct TreeItem {
    /// Its last word, as the tree shows it.
    name: String,
    /// "raster hillshade"; what choosing it chooses, when it is an algorithm.
    path: String,
    tip: String,
    parent: Option<usize>,
    depth: usize,
    algorithm: bool,
    confirm: bool,
    hidden: bool,
    /// A group shown open.
    open: bool,
}

/// How far a level of the catalogue's tree stands in from the one above it: an expander's cell, so
/// a group's algorithms start under its name.
const LEVEL_INDENT: f32 = crate::drive::controls::EXPANDER_WIDTH;

/// The datasets one argument reads: one, or several.
enum InputControl {
    One(BindingPicker),
    Several(BindingList),
}

struct InputRow {
    spec: gp::ArgSpec,
    control: InputControl,
}

/// GIS > GDAL Toolbox (see the module comment).
pub struct GdalToolboxDialog {
    // First, so it goes first: no delivery reaches a half-dropped dialog.
    watcher: DocumentWatcher,
    frame: DialogFrame,
    visible: bool,
    tab: usize,
    search: String,
    tree: Vec<TreeItem>,
    current: Option<usize>,
    title: String,
    description: String,
    help_enabled: bool,
    inputs: Vec<InputRow>,
    arguments: ArgumentForm,
    /// Each output kind offered: its word (LAYER) and what the choice shows.
    output_words: Vec<&'static str>,
    output_texts: Vec<String>,
    output_kind: Choice,
    output_name: String,
    output_formats: Vec<String>,
    output_format: Choice,
    overwrite: bool,
    confirm: bool,
    panel: GeoRunPanel,
    pipeline: PipelineBuilder,
    spec: gp::AlgorithmSpec,
    /// The output kind chosen last, whose default name a new kind replaces.
    last_kind: String,
    remember: bool,
    /// A run ended this frame: what it made - a reference raster, a surface - is offered next.
    reload_wanted: bool,
}

impl GdalToolboxDialog {
    /// The dialog over the window's reference rasters, surfaces, drawing and views, with no
    /// algorithm chosen.
    pub fn new(host: &mut dyn Host) -> Self {
        Self {
            watcher: DocumentWatcher::new(host.document()),
            // The catalogue and the algorithm's groups side by side: at the frame's 520 they ran past
            // the tabs' rule, which stopped at 530 of 645.
            frame: DialogFrame::new("gdalToolboxDialog", "GDAL Toolbox")
                .width(crate::shell::dialog::TWO_COLUMN_WIDTH),
            visible: false,
            tab: 0,
            search: String::new(),
            tree: build_tree(),
            current: None,
            title: String::from("Choose an algorithm on the left."),
            description: String::new(),
            help_enabled: false,
            inputs: Vec::new(),
            arguments: ArgumentForm::new("gdalArg.", "gdalToolboxAdvanced"),
            output_words: Vec::new(),
            output_texts: Vec::new(),
            output_kind: Choice::default(),
            output_name: String::new(),
            output_formats: vec![String::new()],
            output_format: Choice {
                index: Some(0),
                text: String::new(),
                editable: true,
            },
            overwrite: false,
            confirm: false,
            panel: GeoRunPanel::new("gdalToolbox", true),
            // Several algorithms chained in one run (pipeline_builder).
            pipeline: PipelineBuilder::new(host),
            spec: gp::AlgorithmSpec::default(),
            last_kind: String::new(),
            remember: false,
            reload_wanted: false,
        }
    }

    pub fn show(&mut self, host: &dyn Host) {
        self.reload(host);
        self.visible = true;
    }

    pub fn hide(&mut self) {
        self.visible = false;
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn run_panel(&mut self) -> &mut GeoRunPanel {
        &mut self.panel
    }

    pub fn arguments(&mut self) -> &mut ArgumentForm {
        &mut self.arguments
    }

    pub fn pipeline(&mut self) -> &mut PipelineBuilder {
        &mut self.pipeline
    }

    /// The tab in front: 0 the Algorithm tab, 1 the Pipeline tab.
    pub fn tab(&self) -> usize {
        self.tab
    }

    pub fn set_tab(&mut self, tab: usize) {
        self.tab = tab.min(1);
    }

    /// Remembers the algorithm chosen, per user, to choose it again next time (the window's dialog
    /// does; a test's does not).
    pub fn remember_choices(&mut self, remember: bool) {
        self.remember = remember;
    }

    /// What the window does when it makes the dialog: the algorithm last chosen, if this GDAL
    /// still has it, and every choice after remembered.
    pub fn choose_remembered(&mut self, host: &mut dyn Host) {
        let last = host
            .settings()
            .text(LAST_ALGORITHM_KEY)
            .map(String::from)
            .unwrap_or_default();
        let _ = self.choose(host, &last);
        self.remember_choices(true);
    }

    /// The picker of the dataset input `arg`; None when the algorithm chosen reads none of that
    /// name.
    pub fn input(&mut self, arg: &str) -> Option<&mut BindingPicker> {
        self.inputs
            .iter_mut()
            .find(|input| input.spec.name == arg)
            .map(|input| match &mut input.control {
                InputControl::One(picker) => picker,
                InputControl::Several(list) => list.picker(),
            })
    }

    /// The texts gdalOutput.kind offers, in order.
    pub fn output_kinds(&self) -> &[String] {
        &self.output_texts
    }

    /// The formats gdalOutput.format offers, the blank first.
    pub fn output_formats(&self) -> &[String] {
        &self.output_formats
    }

    /// The tree's current row: its last word and its parent's, as the tree shows them; None when
    /// no row is current.
    pub fn tree_current(&self) -> Option<(String, Option<String>)> {
        let item = self.tree.get(self.current?)?;
        let parent = item
            .parent
            .and_then(|at| self.tree.get(at))
            .map(|parent| parent.name.clone());
        Some((item.name.clone(), parent))
    }

    /// Whether the tree's row for `path` ("raster hillshade") is hidden by the search; None for a
    /// path the catalogue does not have.
    pub fn tree_hidden(&self, path: &str) -> Option<bool> {
        self.tree
            .iter()
            .find(|item| item.path == path)
            .map(|item| item.hidden)
    }

    /// Shows the tree's rows the search matches, a group when any of its algorithms does, and
    /// chooses the best match as the search is typed.
    fn filter(&mut self, host: &mut dyn Host) {
        let wanted = self.search.trim().to_lowercase();
        let infos: BTreeMap<String, &gp::AlgorithmInfo> = gp::catalogue()
            .iter()
            .map(|info| (gp::path_text(&info.path), info))
            .collect();
        let matches = |info: &gp::AlgorithmInfo| {
            wanted.is_empty()
                || gp::path_text(&info.path).to_lowercase().contains(&wanted)
                || info.description.to_lowercase().contains(&wanted)
                || info
                    .aliases
                    .iter()
                    .any(|alias| alias.to_lowercase().contains(&wanted))
        };
        let mut exact = None;
        let mut first = None;
        let mut shown = vec![false; self.tree.len()];
        // In the catalogue's order, which is depth first.
        for (at, item) in self.tree.iter().enumerate() {
            if !item.algorithm {
                continue;
            }
            if infos.get(&item.path).is_some_and(|info| matches(info)) {
                shown[at] = true;
                if first.is_none() {
                    first = Some(at);
                }
                if exact.is_none() && item.name.to_lowercase() == wanted {
                    exact = Some(at);
                }
            }
        }
        // A group shows when anything under it does: children come after their parents, so a
        // walk from the end reaches a parent after all it holds.
        for at in (0..self.tree.len()).rev() {
            if shown[at]
                && let Some(parent) = self.tree[at].parent
            {
                shown[parent] = true;
            }
        }
        let open = !wanted.is_empty();
        for (item, shown) in self.tree.iter_mut().zip(shown) {
            item.hidden = !shown;
            item.open = open;
        }
        if wanted.is_empty() {
            return;
        }
        // The best match is chosen as the search is typed, so a search is enough to reach an
        // algorithm: the one named exactly so, else the first shown.
        if let Some(best) = exact.or(first) {
            self.set_current(host, best);
        }
    }

    fn set_current(&mut self, host: &mut dyn Host, at: usize) {
        if self.current != Some(at) {
            self.current = Some(at);
            self.algorithm_chosen(host, at);
        }
    }

    /// Chooses the algorithm at `path` ("raster hillshade"), as a click on its row does. False for
    /// a path that names no algorithm.
    pub fn choose(&mut self, host: &mut dyn Host, path: &str) -> bool {
        let wanted = path.trim().to_lowercase();
        if wanted.is_empty() {
            return false;
        }
        match self
            .tree
            .iter()
            .position(|item| item.algorithm && item.path == wanted)
        {
            Some(at) => {
                self.set_current(host, at);
                true
            }
            None => false,
        }
    }

    /// The algorithm chosen, "raster hillshade"; empty for none.
    pub fn chosen(&self) -> String {
        if self.spec.info.path.is_empty() {
            String::new()
        } else {
            gp::path_text(&self.spec.info.path)
        }
    }

    fn output_word(&self) -> &'static str {
        self.output_kind
            .index
            .and_then(|at| self.output_words.get(at))
            .copied()
            .unwrap_or("")
    }

    fn algorithm_chosen(&mut self, host: &mut dyn Host, at: usize) {
        let Some(item) = self.tree.get(at) else {
            return;
        };
        if !item.algorithm || item.path == self.chosen() {
            return;
        }
        let parts: Vec<String> = item.path.split(' ').map(String::from).collect();
        let mut consumed = 0;
        let described =
            gp::resolve(&parts, &mut consumed).and_then(|resolved| gp::describe(&resolved));
        let spec = match described {
            Ok(spec) => spec,
            Err(error) => {
                self.panel.set_status(&error.describe(), true);
                return;
            }
        };
        self.spec = spec;
        let info = self.spec.info.clone();
        let confirms = info.policy == gp::Policy::Confirm;
        self.title = format!(
            "{} - {}",
            gp::path_text(&info.path),
            if confirms {
                "changes or removes data that already exists: it runs only when Confirm is ticked"
            } else {
                "safe: it changes nothing that exists"
            }
        );
        self.description = if self.spec.long_description.is_empty() {
            info.description.clone()
        } else {
            self.spec.long_description.clone()
        };
        self.help_enabled = !info.help_url.is_empty();
        // The datasets it reads: a picker each, offering only what can be one.
        self.inputs.clear();
        for arg in &self.spec.args {
            if !arg.is_dataset() || !arg.is_input || arg.is_output {
                continue;
            }
            let prefix = format!("gdalInput.{}.", arg.name);
            let kinds = binding_kinds_for(arg.dataset_kinds);
            let control = if arg.r#type == gp::ArgType::DatasetList && arg.max_count != 1 {
                let mut list = BindingList::new(&prefix, kinds, "List", host);
                if !arg.required {
                    list.picker().set_optional();
                }
                InputControl::Several(list)
            } else {
                let mut picker = BindingPicker::new(&prefix, kinds, host);
                if !arg.required {
                    picker.set_optional();
                }
                InputControl::One(picker)
            };
            self.inputs.push(InputRow {
                spec: arg.clone(),
                control,
            });
        }
        let leave_out: BTreeSet<String> = SAID_BY_THE_TARGET
            .iter()
            .map(|name| String::from(*name))
            .collect();
        self.arguments.set_algorithm(&self.spec, &leave_out, false);
        // Where the output can go, as its kinds allow; a surface once the terrain session's store
        // takes a raster result (TO SURFACE is refused until then, so it is offered greyed).
        let output = self
            .spec
            .args
            .iter()
            .find(|arg| arg.is_output && arg.r#type == gp::ArgType::Dataset);
        self.output_words.clear();
        self.output_texts.clear();
        self.output_kind = Choice::default();
        if let Some(output) = output {
            let vector = output.dataset_kinds & gp::DatasetKind::Vector as u32 != 0;
            let raster = output.dataset_kinds & gp::DatasetKind::Raster as u32 != 0;
            let mut offered: Vec<(&'static str, &str)> = Vec::new();
            if vector {
                offered.push(("LAYER", "Layer"));
            }
            if raster {
                offered.push(("REFERENCE", "Reference raster"));
            }
            offered.push(("FILE", "File"));
            if raster {
                offered.push(("SURFACE", "Surface"));
            }
            self.output_words = offered.iter().map(|(word, _)| *word).collect();
            self.output_texts = offered
                .iter()
                .map(|(_, text)| String::from(*text))
                .collect();
            self.output_kind = Choice::at(0);
        }
        let has_output = output.is_some();
        self.last_kind = String::from(self.output_word());
        self.output_name = default_name(&info.path, &self.last_kind);
        self.output_formats = vec![String::new()];
        if has_output {
            self.output_formats
                .extend(gp::suggest(&info.path, "output-format", ""));
        }
        self.output_format = Choice {
            index: Some(0),
            text: String::new(),
            editable: true,
        };
        self.overwrite = false;
        self.confirm = false;
        if self.remember {
            host.settings()
                .set_text(LAST_ALGORITHM_KEY, &gp::path_text(&info.path));
        }
        self.reload(&*host);
    }

    pub fn form(&self) -> GdalToolboxForm {
        let (arguments, arguments_error) = match self.arguments.words() {
            Ok(words) => (words, String::new()),
            Err(error) => (Vec::new(), error.message),
        };
        let mut first_required_taken = false;
        let mut inputs = Vec::new();
        for input in &self.inputs {
            let mut made = GdalToolboxInput {
                arg: input.spec.name.clone(),
                required: input.spec.required,
                ..GdalToolboxInput::default()
            };
            if input.spec.required && !first_required_taken {
                made.first_required = true;
                first_required_taken = true;
            }
            match &input.control {
                InputControl::Several(list) => match list.words() {
                    Ok(words) => made.sources = words,
                    Err(error) => made.error = error.message,
                },
                InputControl::One(picker) => match picker.words() {
                    Ok(words) => {
                        if !words.is_empty() {
                            made.sources.push(words);
                        }
                    }
                    Err(error) => made.error = error.message,
                },
            }
            inputs.push(made);
        }
        let kind = self.output_word();
        let to_file = kind == "FILE";
        GdalToolboxForm {
            path: self.spec.info.path.clone(),
            arguments,
            arguments_error,
            inputs,
            output_kind: String::from(kind),
            output_name: self.output_name.clone(),
            output_format: if to_file {
                self.output_format.text.clone()
            } else {
                String::new()
            },
            overwrite: to_file && self.overwrite,
            needs_confirm: self.spec.info.policy == gp::Policy::Confirm
                && !self.spec.info.path.is_empty(),
            confirm: self.confirm,
        }
    }

    /// Refills the pickers' rasters, surfaces and layers, keeping what is chosen.
    pub fn reload(&mut self, host: &dyn Host) {
        for input in &mut self.inputs {
            match &mut input.control {
                InputControl::One(picker) => picker.reload(host),
                InputControl::Several(list) => list.reload(host),
            }
        }
        self.pipeline.reload(host);
    }

    fn show_help(&mut self, ui: &Ui, host: &dyn Host) {
        let url = self.spec.info.help_url.clone();
        if url.is_empty() {
            return;
        }
        if host.headless() {
            self.panel.set_status(
                &format!(
                    "GDAL's page for {}: {url} (a headless session opens no browser)",
                    self.chosen()
                ),
                false,
            );
            return;
        }
        ui.ctx().open_url(egui::OpenUrl::new_tab(url));
    }

    /// One frame of the dialog, when it is shown: its drawing's changes delivered first.
    pub fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        if !self.visible {
            return;
        }
        if self
            .watcher
            .deliver(host.document())
            .is_some_and(|changes| changes.model)
        {
            self.reload(&*host);
        }
        if show_framed(self, ui, host) {
            self.hide();
        }
        if std::mem::take(&mut self.reload_wanted) {
            self.reload(&*host);
        }
    }

    /// The catalogue's tree: a list (QTreeWidget) named gdalToolboxTree. A fill chooses the
    /// algorithm of that path, as a click on its row does. The row clicked, if one was.
    fn tree_ui(&mut self, ui: &mut Ui) -> Option<usize> {
        const NAME: &str = "gdalToolboxTree";
        let mut clicked = None;
        let taken = take(ui, NAME);
        if let Some(taken) = &taken {
            match &taken.posted.act {
                Act::Fill(text) => {
                    if ui.is_visible() && !ui.is_enabled() {
                        settle(ui, taken, Err(fill_disabled(&taken.posted)));
                    } else if let Some(at) = self
                        .tree
                        .iter()
                        .position(|item| item.algorithm && item.path == *text)
                    {
                        clicked = Some(at);
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
        let tree = &mut self.tree;
        // The tree in the table look (dialog_layout's whole_row and cell_text, as the Command
        // Reference's): a raised header over whole rows, striped, the chosen one lit across the
        // list, where each name was a pill the size of its words.
        let row_height = ui.spacing().interact_size.y;
        let gap = ui.spacing().item_spacing.x;
        let width = ui.available_width();
        let header = egui::Rect::from_min_size(ui.cursor().min, egui::vec2(width, row_height));
        ui.allocate_rect(header, egui::Sense::hover());
        ui.painter()
            .rect_filled(header, 0.0, crate::theme::raised().to_egui());
        crate::dialog_layout::cell_text(
            ui,
            header,
            header.left() + crate::drive::controls::EXPANDER_WIDTH + gap,
            header.right(),
            egui::RichText::new("Algorithm")
                .strong()
                .color(crate::theme::text_muted().to_egui()),
        );
        let confirm_width = crate::shell::dialog::one_line_width(ui, "confirm")
            + crate::dialog_layout::TABLE_COLUMN_GAP;
        let rect = egui::ScrollArea::vertical()
            .id_salt(NAME)
            .max_height(420.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let mut striped = false;
                for at in 0..tree.len() {
                    // Shown when the search keeps it and every group above it is open.
                    let mut above = tree[at].parent;
                    let mut reachable = !tree[at].hidden;
                    while let Some(parent) = above {
                        reachable = reachable && tree[parent].open;
                        above = tree[parent].parent;
                    }
                    if !reachable {
                        continue;
                    }
                    let item = &mut tree[at];
                    let (rect, row) = crate::dialog_layout::whole_row(
                        ui,
                        row_height,
                        current == Some(at),
                        striped,
                    );
                    striped = !striped;
                    // A level a step of LEVEL_INDENT in; a group's expander, an algorithm's empty
                    // cell, so a level's names line up.
                    let left = rect.left() + LEVEL_INDENT * item.depth as f32;
                    let mut cell = ui.new_child(egui::UiBuilder::new().max_rect(
                        egui::Rect::from_min_max(egui::pos2(left, rect.top()), rect.max),
                    ));
                    let expander = crate::drive::controls::tree_expander(
                        &mut cell,
                        (!item.algorithm).then_some(item.open),
                    );
                    let text_left = left + crate::drive::controls::EXPANDER_WIDTH + gap;
                    let text_right = if item.confirm {
                        rect.right() - confirm_width
                    } else {
                        rect.right()
                    };
                    let strong = !item.algorithm;
                    let mut name = egui::RichText::new(item.name.as_str());
                    if strong {
                        name = name.strong();
                    }
                    crate::dialog_layout::cell_text(ui, rect, text_left, text_right, name);
                    let mut tip = item.tip.clone();
                    if item.confirm {
                        crate::dialog_layout::cell_text(
                            ui,
                            rect,
                            text_right,
                            rect.right(),
                            egui::RichText::new("confirm")
                                .color(crate::theme::text_muted().to_egui()),
                        );
                        tip.push_str(
                            "\nChanges or removes data that already exists: runs only when \
                             Confirm is ticked",
                        );
                    }
                    let row = row.on_hover_text(tip);
                    if expander.clicked() || row.clicked() {
                        if item.algorithm {
                            clicked = Some(at);
                        } else {
                            item.open = !item.open;
                        }
                    }
                }
            })
            .inner_rect;
        let response = ui.interact(rect, ui.id().with(NAME), egui::Sense::hover());
        if let Some(taken) = &taken
            && taken.posted.act == Act::ScrollTo
        {
            response.scroll_to_me(None);
            settle(ui, taken, Ok(()));
        }
        let mut list = ControlRecord::new(NAME, ControlKind::Rows);
        list.items = self
            .tree
            .iter()
            .filter(|item| !item.hidden)
            .map(|item| item.path.clone())
            .collect();
        list.value = self
            .current
            .and_then(|at| self.tree.get(at))
            .map(|item| item.path.clone())
            .unwrap_or_default();
        list.enabled = ui.is_enabled();
        list.visible = ui.is_visible();
        record(ui, &response, list);
        clicked
    }

    /// The Output group's fields; true when the kind was changed.
    fn output_ui(&mut self, ui: &mut Ui) -> bool {
        crate::shell::dialog::section(ui, "Output");
        let before = self.output_kind.index;
        let mut kind_changed = row(ui, "To:", |ui| {
            choice(
                ui,
                "gdalOutput.kind",
                &self.output_texts,
                &mut self.output_kind,
            )
            .on_hover_text(
                "Where the result goes: drawn on a layer (one undo step), kept as a reference \
                 raster, written to a file, or kept as a surface (a raster result kept as a \
                 surface arrives with the terrain session)",
            )
            .changed()
        });
        // Surface is listed and cannot be chosen, as the C++'s disabled item could not.
        if kind_changed && self.output_word() == "SURFACE" {
            self.output_kind.index = before;
            kind_changed = false;
        }
        row(ui, "Name:", |ui| {
            line(ui, "gdalOutput.name", &mut self.output_name)
                .on_hover_text("The layer's path, the reference raster's name, or the file's path");
        });
        let to_file = self.output_word() == "FILE";
        row(ui, "Format:", |ui| {
            ui.add_enabled_ui(to_file, |ui| {
                choice(
                    ui,
                    "gdalOutput.format",
                    &self.output_formats,
                    &mut self.output_format,
                )
                .on_hover_text("FORMAT: the file's driver; blank to choose it from the extension");
            });
        });
        ui.add_enabled_ui(to_file, |ui| {
            check(
                ui,
                "gdalOutput.overwrite",
                "Replace a file already there",
                &mut self.overwrite,
            );
        });
        kind_changed
    }

    /// The chosen algorithm's column: its title and documentation, its description, and the
    /// sections of its inputs, arguments and output. Whether GDAL's Documentation was pressed and
    /// the output's kind changed.
    fn algorithm_ui(&mut self, ui: &mut Ui, host: &mut dyn Host) -> (bool, bool) {
        let mut help = false;
        let mut kind_changed = false;
        ui.horizontal(|ui| {
            let title = self.title.clone();
            label(ui, "gdalToolboxTitle", &title);
            let url = self.spec.info.help_url.clone();
            help = ui
                .add_enabled_ui(self.help_enabled, |ui| {
                    button(ui, "gdalToolboxHelp", "GDAL's Documentation")
                        .on_hover_text(url)
                        .clicked()
                })
                .inner;
        });
        let description = self.description.clone();
        label(ui, "gdalToolboxDescription", &description);
        // Headed sections, as every group of a dialog is (shell::dialog::section), where they were
        // bordered boxes.
        if !self.inputs.is_empty() {
            crate::shell::dialog::section(ui, "Inputs");
            let inputs = ui.scope(|ui| {
                ui.vertical(|ui| {
                    for input in &mut self.inputs {
                        ui.label(format!(
                            "{}{} - {}",
                            input.spec.name,
                            if input.spec.required {
                                " (required)"
                            } else {
                                ""
                            },
                            input.spec.description
                        ));
                        let said = match &mut input.control {
                            InputControl::One(picker) => {
                                picker.ui(ui, &*host);
                                picker.take_said()
                            }
                            InputControl::Several(list) => {
                                list.ui(ui, &*host);
                                list.picker().take_said()
                            }
                        };
                        if let Some(text) = said {
                            self.panel.set_status(&text, true);
                        }
                    }
                });
            });
            custom(
                ui,
                &inputs.response,
                ControlRecord::new("gdalToolboxInputs", ControlKind::Other),
            );
        }
        crate::shell::dialog::section(ui, "Arguments");
        let arguments = ui.scope(|ui| {
            ui.vertical(|ui| {
                self.arguments.ui(ui);
            });
        });
        custom(
            ui,
            &arguments.response,
            ControlRecord::new("gdalToolboxArguments", ControlKind::Other),
        );
        // The Output group is there only for an algorithm that writes a dataset;
        // hidden, its fields are still there, as a hidden Qt widget's are.
        if self.output_words.is_empty() {
            hidden(ui, "gdalToolboxOutput", |ui| {
                let _ = self.output_ui(ui);
            });
        } else {
            let output = ui.scope(|ui| {
                ui.vertical(|ui| {
                    kind_changed = self.output_ui(ui);
                });
            });
            custom(
                ui,
                &output.response,
                ControlRecord::new("gdalToolboxOutput", ControlKind::Other),
            );
        }
        let confirms =
            self.spec.info.policy == gp::Policy::Confirm && !self.spec.info.path.is_empty();
        let confirm = &mut self.confirm;
        let mut draw = |ui: &mut Ui| {
            check(
                ui,
                "gdalToolboxConfirm",
                "Confirm: this changes or removes data that already exists",
                confirm,
            );
        };
        if confirms {
            draw(ui);
        } else {
            hidden(ui, "gdalToolboxConfirm", draw);
        }
        (help, kind_changed)
    }

    fn algorithm_page(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        let mut searched = false;
        let mut clicked = None;
        let mut help = false;
        let mut kind_changed = false;
        let catalogue_id = ui.id().with("gdalToolboxCatalogueHeight");
        let page = ui.vertical(|ui| {
            ui.horizontal_top(|ui| {
                // ---- the catalogue ----
                let catalogue = ui.vertical(|ui| {
                    ui.set_max_width(280.0);
                    searched = line_with(
                        ui,
                        "gdalToolboxSearch",
                        &mut self.search,
                        LineOptions {
                            placeholder: "search: a name, an alias or a word of the description",
                            // As wide as the tree under it, whose header spans the column.
                            width: Some(f32::INFINITY),
                            ..LineOptions::default()
                        },
                    )
                    .changed();
                    clicked = self.tree_ui(ui);
                });
                crate::dialog_layout::keep_height(
                    ui,
                    catalogue_id,
                    catalogue.response.rect.height(),
                );
                // ---- the algorithm ----
                let catalogue_height = crate::dialog_layout::last_height(ui, catalogue_id);
                ui.vertical(|ui| {
                    if self.spec.info.path.is_empty() {
                        // Before an algorithm is chosen the column says what goes there, muted
                        // and centred, as an empty preview does; its parts are put away (still
                        // there for a driver). A lone "Arguments" box and a greyed button over a
                        // void read as a page that failed to draw.
                        let height = catalogue_height.unwrap_or(240.0);
                        let (rect, _) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), height),
                            egui::Sense::hover(),
                        );
                        crate::dialog_layout::preview_placeholder(
                            ui,
                            rect,
                            "Choose an algorithm on the left to see what it reads, what it \
                             takes and what it writes.",
                        );
                        hidden(ui, "gdalToolboxNoAlgorithm", |ui| {
                            let _ = self.algorithm_ui(ui, host);
                        });
                    } else {
                        (help, kind_changed) = self.algorithm_ui(ui, host);
                    }
                });
            });
        });
        custom(
            ui,
            &page.response,
            ControlRecord::new("gdalToolboxAlgorithm", ControlKind::Other),
        );
        if searched {
            self.filter(host);
        }
        if let Some(at) = clicked {
            self.set_current(host, at);
        }
        if help {
            self.show_help(ui, &*host);
        }
        if kind_changed {
            // A name left as the last kind's default follows the kind; one typed is kept.
            let kind = String::from(self.output_word());
            let name = self.output_name.trim();
            if name.is_empty() || name == default_name(&self.spec.info.path, &self.last_kind) {
                self.output_name = default_name(&self.spec.info.path, &kind);
            }
            self.last_kind = kind;
        }
        let made = gdal_toolbox_line(&self.form());
        self.panel.command_ui(ui, &made);
    }

    /// The Algorithm page's reply, once there is one, and its row of buttons, in the dialog's
    /// footer: Close only while the page is in front (`close`). Whether Close was clicked.
    fn algorithm_footer(&mut self, ui: &mut Ui, host: &mut dyn Host, close: Option<&str>) -> bool {
        let made = gdal_toolbox_line(&self.form());
        let (finished, closed) = self.panel.footer_ui(ui, host, &made, close);
        if finished.is_some() {
            self.reload_wanted = true;
        }
        closed
    }
}

/// The catalogue as the tree's rows, in GDAL's order: each group, then what it holds.
fn build_tree() -> Vec<TreeItem> {
    let mut items: Vec<TreeItem> = Vec::new();
    let mut places: BTreeMap<String, usize> = BTreeMap::new();
    for info in gp::catalogue() {
        let Some((last, parent_path)) = info.path.split_last() else {
            continue;
        };
        let parent = if parent_path.is_empty() {
            None
        } else {
            places.get(&gp::path_text(parent_path)).copied()
        };
        let path = gp::path_text(&info.path);
        places.insert(path.clone(), items.len());
        items.push(TreeItem {
            name: last.clone(),
            tip: format!("{path}: {}", info.description),
            path,
            parent,
            depth: info.path.len() - 1,
            algorithm: !info.container,
            confirm: !info.container && info.policy == gp::Policy::Confirm,
            hidden: false,
            open: false,
        });
    }
    items
}

impl Framed for GdalToolboxDialog {
    fn frame_mut(&mut self) -> &mut DialogFrame {
        &mut self.frame
    }

    // Each tab holds its own Run and Reply, as the C++ pages do; the page in front shows them in
    // the dialog's footer (Preview and the status at the left, Close, then Run in the accent),
    // where they were in the body over an empty footer band.
    fn body(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        tabs(
            ui,
            "gdalToolboxTabs",
            &["Algorithm", "Pipeline"],
            &mut self.tab,
        );
        // The page not in front is still there, as a QTabWidget's is: what it holds can be filled,
        // pressed and read.
        if self.tab == 0 {
            self.algorithm_page(ui, host);
            offstage(ui, "gdalToolboxPipelinePage", |ui| {
                self.pipeline.page_ui(ui, host);
            });
        } else {
            offstage(ui, "gdalToolboxAlgorithmPage", |ui| {
                self.algorithm_page(ui, host);
            });
            self.pipeline.page_ui(ui, host);
        }
    }

    fn footer(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        // The page not in front keeps its buttons and reply, off the dialog, as its fields are.
        let closed = if self.tab == 0 {
            let closed = self.algorithm_footer(ui, host, Some("gdalToolboxClose"));
            offstage(ui, "gdalToolboxPipelineFooter", |ui| {
                self.pipeline.footer_ui(ui, host, None)
            });
            closed
        } else {
            offstage(ui, "gdalToolboxAlgorithmFooter", |ui| {
                self.algorithm_footer(ui, host, None)
            });
            self.pipeline.footer_ui(ui, host, Some("gdalToolboxClose"))
        };
        if closed {
            self.hide();
        }
    }
}
