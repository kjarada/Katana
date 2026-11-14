//! The GIS menu's import dialogs (gis_import_dialogs.hpp; docs/interop.md, "Import options"):
//! what to take from a GDAL or PDAL source and how - vector data, a raster, a point cloud.
//!
//! Each is built FROM a description of the file (interop's describe_source), so it offers what
//! the file actually holds - the layers of this GeoPackage, the point count of this LAS, whether
//! it is COPC - and says its choices as the IMPORT line they make, shown read-only in <d>Command.
//! The dialog imports nothing itself: the window hands that line to its one executor
//! (`Host::run`) when Import is pressed, as if it had been typed, so the dialog does nothing
//! katana_cli or katana_mcp cannot do by sending the same line. The vector dialog's Preview runs
//! the line with PREVIEW, which reads with the filters and says how many features match of how
//! many, importing nothing.
//!
//! Object names:
//!
//! ```text
//! vector (vectorImportDialog): vectorImportFileLayers, vectorImportWhere, vectorImportSql,
//!   vectorImportDialect, vectorImportUseScope, vectorImportScope (the shared ScopeFilterWidget,
//!   its parts vectorImportScopeSelection ... vectorImportDrawnOnly), vectorImportClip,
//!   vectorImportFields, vectorImportAttributes, vectorImportTarget, vectorImportMax,
//!   vectorImportOpenOptions, vectorImportCrs, vectorImportAssumeCrs, vectorImportPreview,
//!   vectorImportMatchCount, vectorImportCommand, and the Placement group's (import_placement);
//! raster (rasterImportDialog): rasterImportName, rasterImportMaxPixels, rasterImportBand,
//!   rasterImportSubdataset, rasterImportCrs, rasterImportCommand;
//! point cloud (pointCloudImportDialog): pointCloudImportName, pointCloudImportBudget,
//!   pointCloudImportClasses, pointCloudImportUseResolution, pointCloudImportResolution,
//!   pointCloudImportCommand.
//! ```
//!
//! The file's layer list is vectorImportFileLayers here, where the C++ named it
//! vectorImportLayers: that is also the name of the scope's own layer list (the shared widget's
//! <prefix>Layers), and a name drawn twice in one dialog is one a driver cannot tell apart.
//!
//! and each dialog's Import and Cancel are <d>Run and <d>Cancel (gis_dialog_support). A dialog
//! answers once one of them was pressed (`result`); the window then runs `command()`.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use egui::{RichText, Ui};
use katana_app::geo::replies::parse_records;
use katana_app::geo::vector_support::sql_for_line;
use katana_cad::import_placement::{ImportPlacementMode, placement_word};
use katana_core::error::make_error;
use katana_core::path_text::generic_path_utf8;
use katana_core::text::format_exact_real;
use katana_core::{Error, ErrorCode, Result};
use katana_geometry::Box2;
use katana_interop::dataset_info::format_description;
use katana_interop::{
    PointCloudImportOptions, RasterImportOptions, SourceDescription, VectorImportOptions,
};
use katana_pointcloud::point_cloud_engine::PointCloudEngine;

use crate::annotation::dialog_support::table;
use crate::customisation::ScopeFilterWidget;
use crate::drive::{
    Choice, LineOptions, SpinRange, button, check, choice, label_as, line_with, spin, whole,
};
use crate::format::grouped;
use crate::geo::gis_tool_dialog::GisDialogContext;
use crate::gis_dialog_support::ok_cancel;
use crate::host::{Host, VerbOutcome};
use crate::import_placement::{DialogResult, ImportPlacementBox};
use crate::shell::DialogFrame;
use crate::theme::palette;

fn invalid(message: String) -> Error {
    make_error(ErrorCode::InvalidArgument, message, String::new())
}

/// The file as the first word of its IMPORT line: always quoted, so a path holding a blank - or
/// a word IMPORT reads as an option - is one word.
fn path_word(path: &Path) -> Result<String> {
    let text = String::from_utf8_lossy(&generic_path_utf8(path)).into_owned();
    if text.contains('"') {
        return Err(invalid(String::from(
            "the file's path holds a double quote, which a command line cannot carry",
        )));
    }
    Ok(format!("\"{text}\""))
}

/// A path's last part, and the same without its extension, as text.
fn file_name_of(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn stem_of(path: &Path) -> String {
    path.file_stem()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The line a dialog shows, and whether there is one for Import to run.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct CommandShown {
    text: String,
    placeholder: String,
    ready: bool,
}

impl CommandShown {
    fn of(line: &Result<String>) -> Self {
        match line {
            Ok(line) => Self {
                text: line.clone(),
                placeholder: String::new(),
                ready: true,
            },
            Err(error) => Self {
                text: String::new(),
                placeholder: format!("nothing to import yet: {}", error.message),
                ready: false,
            },
        }
    }

    /// The read-only line, under "Command:".
    fn ui(&mut self, ui: &mut Ui, name: &str) {
        ui.horizontal(|ui| {
            ui.label("Command:");
            line_with(
                ui,
                name,
                &mut self.text,
                LineOptions {
                    placeholder: &self.placeholder,
                    read_only: true,
                    width: Some(f32::INFINITY),
                    ..LineOptions::default()
                },
            )
            .on_hover_text(
                "The line Import runs - type it on the command line, or give it to katana_cli \
                 or katana_mcp, and it does the same",
            );
        });
    }
}

/// What the file is, above the options, in the words Dataset Information uses: the same
/// format_description, so the dialog and the info window cannot describe one file two ways.
fn summary_label(ui: &mut Ui, summary: &str) {
    // The file's own words, never markup.
    ui.label(
        RichText::new(summary)
            .monospace()
            .color(palette::text_muted().to_egui()),
    );
}

/// A choice whose items each stand for a word of the line (QComboBox's item data).
struct DataChoice {
    items: Vec<String>,
    data: Vec<String>,
    chosen: Choice,
}

impl DataChoice {
    fn new(entries: &[(&str, &str)]) -> Self {
        Self {
            items: entries.iter().map(|(shown, _)| String::from(*shown)).collect(),
            data: entries.iter().map(|(_, data)| String::from(*data)).collect(),
            chosen: Choice::at(0),
        }
    }

    fn push(&mut self, shown: String, data: String) {
        self.items.push(shown);
        self.data.push(data);
    }

    /// QComboBox::currentData: the chosen item's word; empty for none.
    fn data(&self) -> &str {
        self.chosen
            .index
            .and_then(|at| self.data.get(at))
            .map_or("", String::as_str)
    }

    /// Chooses the item whose word is `data` (setCurrentIndex(findData)).
    fn choose_data(&mut self, data: &str) {
        if let Some(at) = self.data.iter().position(|each| each == data) {
            self.chosen.index = Some(at);
        }
    }

    fn ui(&mut self, ui: &mut Ui, name: &str, label: &str, tip: &str) {
        ui.horizontal(|ui| {
            ui.label(label);
            let response = choice(ui, name, &self.items, &mut self.chosen);
            if !tip.is_empty() {
                response.on_hover_text(tip);
            }
        });
    }
}

/// A one-line field under its label, with a placeholder and a tip.
fn text_row(
    ui: &mut Ui,
    name: &str,
    label: &str,
    value: &mut String,
    placeholder: &str,
    tip: &str,
) {
    ui.horizontal(|ui| {
        ui.label(label);
        let response = line_with(
            ui,
            name,
            value,
            LineOptions {
                placeholder,
                ..LineOptions::default()
            },
        );
        if !tip.is_empty() {
            response.on_hover_text(tip);
        }
    });
}

/// ASPRS LAS 1.4 R15 table 17, the standard point classes. 8 and 12 are reserved in 1.4 (Model
/// Key-point and Overlap in 1.2 and 1.3), so they are named as reserved rather than given a
/// meaning the file may not have.
const ASPRS_CLASSES: [(i32, &str); 19] = [
    (0, "Created, never classified"),
    (1, "Unclassified"),
    (2, "Ground"),
    (3, "Low vegetation"),
    (4, "Medium vegetation"),
    (5, "High vegetation"),
    (6, "Building"),
    (7, "Low point (noise)"),
    (8, "Reserved"),
    (9, "Water"),
    (10, "Rail"),
    (11, "Road surface"),
    (12, "Reserved"),
    (13, "Wire - guard (shield)"),
    (14, "Wire - conductor (phase)"),
    (15, "Transmission tower"),
    (16, "Wire-structure connector"),
    (17, "Bridge deck"),
    (18, "High noise"),
];

/// QSpinBox's own ceiling (an int).
const SPIN_MAX: i64 = i32::MAX as i64;

// ---- the words -------------------------------------------------------------------------------

/// " key=value" as a line takes it: the value double-quoted when it holds a blank.
/// InvalidArgument naming `field` for a double quote or a line break, which no line can carry.
pub fn import_option_word(key: &str, value: &str, field: &str) -> Result<String> {
    if value.contains('"') || value.contains('\n') {
        return Err(invalid(format!(
            "{field} holds a double quote or a line break, which a command line cannot carry"
        )));
    }
    let blank = value.is_empty() || value.chars().any(char::is_whitespace);
    Ok(if blank {
        format!(" {key}=\"{value}\"")
    } else {
        format!(" {key}={value}")
    })
}

// ---- vector import --------------------------------------------------------------------------

/// One layer of the file, as the list offers it.
struct OfferedLayer {
    name: String,
    shown: String,
    ticked: bool,
}

/// GIS > Import Vector Data.
pub struct VectorImportDialog {
    frame: DialogFrame,
    path: PathBuf,
    summary: String,
    context: GisDialogContext,
    layers: Vec<OfferedLayer>,
    where_clause: String,
    sql: String,
    dialect: DataChoice,
    use_scope: bool,
    scope: ScopeFilterWidget,
    clip: bool,
    fields: String,
    attributes: bool,
    target: String,
    max: i64,
    open_options: String,
    crs: DataChoice,
    assume_crs: String,
    placement: ImportPlacementBox,
    match_count: String,
    /// Where a preview's job's end is told, for the next look to take.
    heard: Rc<RefCell<Vec<VerbOutcome>>>,
    result: Option<DialogResult>,
}

impl VectorImportDialog {
    /// `drawing` is the drawing's extent now, for what the Placement group says a move onto it
    /// will do. The scope offers the host's drawing's layers (when the context reads it) and the
    /// context's views.
    pub fn new(
        source: &SourceDescription,
        drawing: Box2,
        context: GisDialogContext,
        host: &mut dyn Host,
    ) -> Self {
        // Every layer ticked is no layers= word: the whole file, as a GeoPackage's layers are
        // usually meant to arrive together.
        let layers = source
            .vector_layers
            .iter()
            .map(|layer| OfferedLayer {
                name: layer.name.clone(),
                shown: format!(
                    "{}  ({} {})",
                    layer.name,
                    grouped(layer.feature_count),
                    layer.geometry_type
                ),
                ticked: true,
            })
            .collect();
        // What of the drawing bounds the read: the shared "Apply to" and "Only those that match"
        // controls (CLAUDE.md section 1.1).
        let mut scope = ScopeFilterWidget::new("vectorImport");
        scope.views = context.views_fn();
        if context.reads_drawing {
            scope.reload(host.document().model());
        } else {
            scope.reload_views();
        }
        let placement = ImportPlacementBox::new(drawing, "", host.settings());
        Self {
            frame: DialogFrame::new(
                "vectorImportDialog",
                &format!("Import Vector Data - {}", file_name_of(&source.path)),
            )
            .modal(true),
            path: source.path.clone(),
            summary: String::from(format_description(source).trim()),
            context,
            layers,
            where_clause: String::new(),
            sql: String::new(),
            dialect: DataChoice::new(&[
                ("The driver's own", ""),
                ("OGR SQL", "ogrsql"),
                ("SQLite", "sqlite"),
            ]),
            use_scope: false,
            scope,
            clip: false,
            fields: String::new(),
            attributes: VectorImportOptions::default().attributes_as_properties,
            target: String::new(),
            max: 0,
            open_options: String::new(),
            crs: DataChoice::new(&[
                ("As the file has them", ""),
                ("Into the project's coordinate system", "project"),
                (
                    "Set the project's from the file, if it has none",
                    "adopt",
                ),
            ]),
            assume_crs: String::new(),
            placement,
            match_count: String::new(),
            heard: Rc::new(RefCell::new(Vec::new())),
            result: None,
        }
    }

    pub fn object_name(&self) -> &str {
        self.frame.name()
    }

    /// Where the data lands; the window keeps the choice (remember).
    pub fn placement_box(&self) -> &ImportPlacementBox {
        &self.placement
    }

    pub fn placement_box_mut(&mut self) -> &mut ImportPlacementBox {
        &mut self.placement
    }

    /// The scope and filter controls.
    pub fn scope_controls(&mut self) -> &mut ScopeFilterWidget {
        &mut self.scope
    }

    /// Ticks or clears the file's layer at `index` in the list, as its box does.
    pub fn set_layer_ticked(&mut self, index: usize, ticked: bool) {
        if let Some(layer) = self.layers.get_mut(index) {
            layer.ticked = ticked;
        }
    }

    /// QDialog::result: None while it is open.
    pub fn result(&self) -> Option<DialogResult> {
        self.result
    }

    /// The IMPORT line the fields make, or why there is none.
    pub fn command(&self) -> Result<String> {
        let mut line = format!("IMPORT {}", path_word(&self.path)?);
        let placement = self.placement.placement();
        let word = placement_word(&placement);
        if !word.is_empty() {
            line.push(' ');
            line.push_str(&word);
        }

        let chosen: Vec<&str> = self
            .layers
            .iter()
            .filter(|layer| layer.ticked)
            .map(|layer| layer.name.as_str())
            .collect();
        let sql = self.sql.trim();
        if chosen.len() != self.layers.len() {
            if chosen.is_empty() {
                return Err(invalid(String::from("tick at least one layer")));
            }
            if !sql.is_empty() {
                return Err(invalid(String::from(
                    "SQL reads the rows its statement gives, not layers: tick every layer, or \
                     clear the SQL",
                )));
            }
            for name in &chosen {
                if name.contains(',') {
                    return Err(invalid(format!(
                        "the layer '{name}' has a comma in its name, which layers= cannot carry"
                    )));
                }
            }
            line.push_str(&import_option_word(
                "layers",
                &chosen.join(","),
                "A layer's name",
            )?);
        }
        let clause = self.where_clause.trim();
        if !clause.is_empty() {
            line.push_str(&import_option_word("where", clause, "Only where")?);
        }
        if !sql.is_empty() {
            let dialect = self.dialect.data();
            // One word of a line: SQLite's "identifiers" become [identifiers], which it reads
            // alike (vector_support::sql_for_line, GIS SQL's rule).
            let text = sql_for_line(sql, dialect == "sqlite")
                .map_err(|error| invalid(format!("the SQL: {}", error.message)))?;
            line.push_str(&import_option_word("sql", &text, "The SQL")?);
            if !dialect.is_empty() {
                line.push_str(" dialect=");
                line.push_str(dialect);
            }
        }
        let fields = self.fields.trim();
        if !fields.is_empty() {
            let names: Vec<&str> = fields
                .split(',')
                .filter(|name| !name.is_empty())
                .map(str::trim)
                .collect();
            line.push_str(&import_option_word(
                "fields",
                &names.join(","),
                "Attributes",
            )?);
        }
        if !self.attributes {
            line.push_str(" attributes=no");
        }
        let target = self.target.trim();
        if !target.is_empty() {
            line.push_str(&import_option_word("target", target, "The Katana layer")?);
        }
        if self.max > 0 {
            line.push_str(&format!(" max={}", self.max));
        }
        for option in self.open_options.split_whitespace() {
            if !option.find('=').is_some_and(|at| at > 0) {
                return Err(invalid(format!(
                    "an open option is KEY=VALUE, not '{option}'"
                )));
            }
            line.push_str(&import_option_word("oo", option, "An open option")?);
        }
        let crs = self.crs.data();
        if !crs.is_empty() {
            line.push_str(" crs=");
            line.push_str(crs);
        }
        let assumed = self.assume_crs.trim();
        if !assumed.is_empty() {
            line.push_str(&import_option_word("srs", assumed, "The file's CRS")?);
        }
        if self.use_scope {
            if placement.mode != ImportPlacementMode::Keep {
                return Err(invalid(String::from(
                    "a scope is in the drawing's coordinates: keep the data where it is to limit \
                     it to one",
                )));
            }
            let words = self
                .scope
                .verb_words()
                .map_err(|error| invalid(format!("the scope: {}", error.message)))?;
            line.push(' ');
            line.push_str(words.trim());
            if self.clip {
                line.push_str(" clip");
            }
        }
        Ok(line)
    }

    /// Runs the line with PREVIEW through the window's executor (and waits to hear of the job it
    /// starts); the match count goes to vectorImportMatchCount when it answers.
    pub fn preview(&mut self, host: &mut dyn Host) {
        let line = match self.command() {
            Ok(line) => line,
            Err(error) => {
                self.match_count = error.message;
                return;
            }
        };
        if !self.context.runs {
            self.match_count = String::from("Nothing here can run a command.");
            return;
        }
        let outcome = host.run(&format!("{line} PREVIEW"));
        // Interactively the line became a job: its reply comes when it ends. Never after the
        // dialog has gone.
        let heard = Rc::downgrade(&self.heard);
        let waits = host.await_job(
            &outcome,
            Box::new(move |done: &VerbOutcome, _host: &mut dyn Host| {
                if let Some(heard) = heard.upgrade() {
                    heard.borrow_mut().push(done.clone());
                }
            }),
        );
        if waits {
            self.match_count = String::from("Reading with the filters...");
            return;
        }
        self.show_preview(&outcome);
    }

    /// Takes what the preview's job has said since the dialog last looked.
    pub fn poll(&mut self) {
        let heard: Vec<VerbOutcome> = std::mem::take(&mut *self.heard.borrow_mut());
        for outcome in heard {
            self.show_preview(&outcome);
        }
    }

    fn show_preview(&mut self, outcome: &VerbOutcome) {
        if !outcome.ok {
            self.match_count = String::from(outcome.error.split('\n').next().unwrap_or(""));
            return;
        }
        for record in parse_records(&outcome.reply) {
            if record.kind != "import" {
                continue;
            }
            if record.get("ran") == Some("no") {
                self.match_count = String::from(
                    "The scope takes nothing of the drawing: nothing would be read.",
                );
                return;
            }
            let text = |key: &str| record.get(key).unwrap_or("?");
            self.match_count = format!(
                "{} of {} features match; {} entities would be imported.",
                text("features"),
                text("of"),
                text("entities")
            );
            return;
        }
        self.match_count = String::from(outcome.reply.trim().split('\n').next().unwrap_or(""));
    }

    /// What vectorImportMatchCount says.
    pub fn match_text(&self) -> &str {
        &self.match_count
    }

    /// Draws the dialog while it is open; answers once Import or Cancel was pressed. The window
    /// then runs `command()` through its executor when it was accepted.
    pub fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) -> Option<DialogResult> {
        if self.result.is_some() {
            return self.result;
        }
        self.poll();
        let mut command = CommandShown::of(&self.command());
        let ready = command.ready;
        let ctx = ui.ctx().clone();
        let mut frame = self.frame.clone();
        let mut pressed = None;
        let mut preview = false;
        let shown = {
            let mut lent = (&mut *self, &mut command, &mut pressed, &mut preview);
            frame.show(
                &ctx,
                &mut lent,
                |ui, lent| lent.0.body(ui, &mut *lent.1, &mut *lent.3),
                |ui, lent| {
                    let buttons = ok_cancel(ui, "vectorImportDialog", "Import", ready);
                    if buttons.accepted {
                        *lent.2 = Some(DialogResult::Accepted);
                    }
                    if buttons.rejected {
                        *lent.2 = Some(DialogResult::Rejected);
                    }
                },
            )
        };
        self.frame = frame;
        if preview {
            self.preview(host);
        }
        match pressed {
            Some(answer) => self.result = Some(answer),
            None if shown.closed => self.result = Some(DialogResult::Rejected),
            None => {}
        }
        self.result
    }

    fn body(&mut self, ui: &mut Ui, command: &mut CommandShown, preview: &mut bool) {
        summary_label(ui, &self.summary);
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.label("Layers:");
                let rows: Vec<Vec<String>> = self
                    .layers
                    .iter()
                    .map(|layer| vec![layer.shown.clone()])
                    .collect();
                let ticks: Vec<bool> = self.layers.iter().map(|layer| layer.ticked).collect();
                let shown = egui::ScrollArea::vertical()
                    .id_salt("vectorImportFileLayers")
                    .max_height(110.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        table(
                            ui,
                            "vectorImportFileLayers",
                            &["Layer"],
                            &rows,
                            None,
                            &[],
                            Some((0, &ticks)),
                        )
                    })
                    .inner;
                if let Some((row, on)) = shown.checked {
                    self.set_layer_ticked(row, on);
                }
                text_row(
                    ui,
                    "vectorImportWhere",
                    "Only where:",
                    &mut self.where_clause,
                    "every feature, e.g. kind = 'lot'",
                    "An OGR SQL WHERE clause, applied by the file's driver (where=)",
                );
                text_row(
                    ui,
                    "vectorImportSql",
                    "SQL:",
                    &mut self.sql,
                    "none, e.g. SELECT * FROM parcels WHERE area_m2 > 7000",
                    "A SELECT run on the file; its rows are imported in place of its layers \
                     (sql=)",
                );
                self.dialect
                    .ui(ui, "vectorImportDialect", "SQL dialect:", "");
                text_row(
                    ui,
                    "vectorImportFields",
                    "Attributes:",
                    &mut self.fields,
                    "every attribute",
                    "Only these attributes become properties, a,b (fields=)",
                );
                check(
                    ui,
                    "vectorImportAttributes",
                    "Keep feature attributes as entity properties",
                    &mut self.attributes,
                );
                text_row(
                    ui,
                    "vectorImportTarget",
                    "Katana layer:",
                    &mut self.target,
                    "one Katana layer per source layer",
                    "Put every imported entity on this layer (target=). Left empty, each source \
                     layer - or each DXF layer - becomes a Katana layer of its own.",
                );
                ui.horizontal(|ui| {
                    ui.label("At most:");
                    // 0 is "every feature" (the box's special value).
                    whole(ui, "vectorImportMax", &mut self.max, 0..=SPIN_MAX)
                        .on_hover_text("At most this many features (max=); 0 is every feature");
                });
                text_row(
                    ui,
                    "vectorImportOpenOptions",
                    "Open options:",
                    &mut self.open_options,
                    "none, e.g. FLATTEN_NESTED_ATTRIBUTES=YES",
                    "The driver's open options, KEY=VALUE separated by blanks (oo=); each is \
                     checked against the driver's own list. FORMATS OPTIONS <driver> lists them.",
                );
                self.crs.ui(
                    ui,
                    "vectorImportCrs",
                    "Coordinates:",
                    "crs=project reprojects the features; crs=adopt sets the project's \
                     coordinate system in the same undo step",
                );
                text_row(
                    ui,
                    "vectorImportAssumeCrs",
                    "File's CRS if none:",
                    &mut self.assume_crs,
                    "the file's own, e.g. EPSG:28356",
                    "The coordinate system of a file that declares none (srs=)",
                );
            });
            ui.group(|ui| {
                ui.vertical(|ui| {
                    ui.label("Only what lies within");
                    check(
                        ui,
                        "vectorImportUseScope",
                        "Limit the import to this part of the drawing",
                        &mut self.use_scope,
                    );
                    let limited = self.use_scope;
                    ui.add_enabled_ui(limited, |ui| {
                        self.scope.ui(ui);
                        check(
                            ui,
                            "vectorImportClip",
                            "Cut features at its edge (clip)",
                            &mut self.clip,
                        );
                    });
                });
            });
        });
        self.placement.ui(ui);
        command.ui(ui, "vectorImportCommand");
        let ready = command.ready;
        ui.horizontal(|ui| {
            // GDAL's words, shown as they are: read as markup, a "<code>" in a message was taken
            // for a tag and vanished.
            let said = self.match_count.clone();
            label_as(
                ui,
                "vectorImportMatchCount",
                &said,
                egui::Label::new(said.as_str()).wrap(),
            );
            ui.add_enabled_ui(ready, |ui| {
                if button(ui, "vectorImportPreview", "Preview")
                    .on_hover_text(
                        "Run the line with PREVIEW: how many features the filters take, and \
                         nothing imported",
                    )
                    .clicked()
                {
                    *preview = true;
                }
            });
        });
    }
}

// ---- raster import --------------------------------------------------------------------------

/// GIS > Import Raster.
pub struct RasterImportDialog {
    frame: DialogFrame,
    path: PathBuf,
    summary: String,
    name: String,
    resolution: DataChoice,
    band: i64,
    bands: i64,
    subdataset: DataChoice,
    has_subdatasets: bool,
    crs: DataChoice,
    result: Option<DialogResult>,
}

impl RasterImportDialog {
    pub fn new(source: &SourceDescription) -> Self {
        // The DISPLAY copy's longest side. The file itself is never resampled for analysis -
        // Surface From Raster reads the band through GDAL - so this is a trade of sharpness
        // against memory (4 bytes a pixel), nothing more.
        let mut resolution = DataChoice::new(&[]);
        for pixels in [1024_u64, 2048, 4096, 8192] {
            resolution.push(
                format!(
                    "{pixels} px  ({} MB at most)",
                    pixels * pixels * 4 / (1024 * 1024)
                ),
                format!("{pixels}"),
            );
        }
        resolution.choose_data(&format!("{}", RasterImportOptions::default().max_pixels));
        let mut subdataset = DataChoice::new(&[("The file itself", "")]);
        for (at, inside) in source.subdatasets.iter().enumerate() {
            subdataset.push(
                format!("{}  {}", at + 1, inside.description),
                format!("{}", at + 1),
            );
        }
        let bands = source
            .raster
            .as_ref()
            .map_or(1, |raster| i64::from(raster.band_count));
        Self {
            frame: DialogFrame::new(
                "rasterImportDialog",
                &format!("Import Raster - {}", file_name_of(&source.path)),
            )
            .modal(true),
            path: source.path.clone(),
            summary: String::from(format_description(source).trim()),
            name: stem_of(&source.path),
            resolution,
            band: 0,
            bands: bands.max(1),
            subdataset,
            has_subdatasets: !source.subdatasets.is_empty(),
            crs: DataChoice::new(&[
                ("Keep the project's coordinate system", ""),
                (
                    "Set the project's from the raster, if it has none",
                    "adopt",
                ),
            ]),
            result: None,
        }
    }

    pub fn object_name(&self) -> &str {
        self.frame.name()
    }

    /// QDialog::result: None while it is open.
    pub fn result(&self) -> Option<DialogResult> {
        self.result
    }

    /// The IMPORT line the fields make, or why there is none.
    pub fn command(&self) -> Result<String> {
        let mut line = format!("IMPORT {}", path_word(&self.path)?);
        let name = self.name.trim();
        if !name.is_empty() && name != stem_of(&self.path) {
            line.push_str(&import_option_word("name", name, "The name")?);
        }
        let pixels = self.resolution.data();
        if pixels != format!("{}", RasterImportOptions::default().max_pixels) {
            line.push_str(" maxpixels=");
            line.push_str(pixels);
        }
        if self.band > 0 {
            line.push_str(&format!(" band={}", self.band));
        }
        let subdataset = self.subdataset.data();
        if !subdataset.is_empty() {
            line.push_str(" subdataset=");
            line.push_str(subdataset);
        }
        let crs = self.crs.data();
        if !crs.is_empty() {
            line.push_str(" crs=");
            line.push_str(crs);
        }
        Ok(line)
    }

    /// Draws the dialog while it is open; answers once Import or Cancel was pressed.
    pub fn ui(&mut self, ui: &mut Ui, _host: &mut dyn Host) -> Option<DialogResult> {
        if self.result.is_some() {
            return self.result;
        }
        let mut command = CommandShown::of(&self.command());
        let ready = command.ready;
        let ctx = ui.ctx().clone();
        let mut frame = self.frame.clone();
        let mut pressed = None;
        let shown = {
            let mut lent = (&mut *self, &mut command, &mut pressed);
            frame.show(
                &ctx,
                &mut lent,
                |ui, lent| lent.0.body(ui, &mut *lent.1),
                |ui, lent| {
                    let buttons = ok_cancel(ui, "rasterImportDialog", "Import", ready);
                    if buttons.accepted {
                        *lent.2 = Some(DialogResult::Accepted);
                    }
                    if buttons.rejected {
                        *lent.2 = Some(DialogResult::Rejected);
                    }
                },
            )
        };
        self.frame = frame;
        match pressed {
            Some(answer) => self.result = Some(answer),
            None if shown.closed => self.result = Some(DialogResult::Rejected),
            None => {}
        }
        self.result
    }

    fn body(&mut self, ui: &mut Ui, command: &mut CommandShown) {
        summary_label(ui, &self.summary);
        text_row(ui, "rasterImportName", "Name:", &mut self.name, "", "");
        self.resolution
            .ui(ui, "rasterImportMaxPixels", "Display resolution:", "");
        ui.horizontal(|ui| {
            ui.label("Band:");
            // 0 is "As the file's colours" (the box's special value).
            whole(ui, "rasterImportBand", &mut self.band, 0..=self.bands).on_hover_text(
                "One band alone, as grey (band=): a multispectral image's near infrared, or one \
                 grid of a stack; 0 is as the file's colours",
            );
        });
        ui.add_enabled_ui(self.has_subdatasets, |ui| {
            self.subdataset
                .ui(ui, "rasterImportSubdataset", "Dataset inside:", "");
        });
        self.crs.ui(ui, "rasterImportCrs", "Coordinates:", "");
        command.ui(ui, "rasterImportCommand");
    }
}

// ---- point cloud import ---------------------------------------------------------------------

/// GIS > Import Point Cloud.
pub struct PointCloudImportDialog {
    frame: DialogFrame,
    path: PathBuf,
    summary: String,
    point_count: u64,
    copc: bool,
    name: String,
    budget: i64,
    classification: DataChoice,
    use_resolution: bool,
    resolution: f64,
    result: Option<DialogResult>,
}

impl PointCloudImportDialog {
    pub fn new(source: &SourceDescription) -> Self {
        let mut classification = DataChoice::new(&[("Every class", "-1")]);
        for (value, name) in ASPRS_CLASSES {
            classification.push(format!("{value} - {name}"), format!("{value}"));
        }
        // The budget is what makes a billion-point file open like a small one; the ceiling is
        // QSpinBox's own (an int), far above what fits in memory as point values (40 bytes each:
        // 2^31 of them is 80 GB).
        let budget = i64::try_from(PointCloudImportOptions::default().budget)
            .unwrap_or(SPIN_MAX)
            .clamp(1_000, SPIN_MAX);
        Self {
            frame: DialogFrame::new(
                "pointCloudImportDialog",
                &format!("Import Point Cloud - {}", file_name_of(&source.path)),
            )
            .modal(true),
            path: source.path.clone(),
            summary: String::from(format_description(source).trim()),
            point_count: source
                .point_cloud
                .as_ref()
                .map_or(0, |cloud| cloud.point_count),
            // COPC answers a level-of-detail query from its own octree instead of being
            // decimated; offered only where the file can answer it, because the engine refuses
            // the question of any other file.
            copc: source.point_cloud.as_ref().is_some_and(|cloud| cloud.copc),
            name: stem_of(&source.path),
            budget,
            classification,
            use_resolution: false,
            resolution: 1.0,
            result: None,
        }
    }

    pub fn object_name(&self) -> &str {
        self.frame.name()
    }

    /// QDialog::result: None while it is open.
    pub fn result(&self) -> Option<DialogResult> {
        self.result
    }

    /// The class chosen; negative for every class.
    fn class(&self) -> i32 {
        katana_core::text::parse_integer(self.classification.data())
            .and_then(|value| i32::try_from(value).ok())
            .unwrap_or(-1)
    }

    /// What the dialog says the choices will read.
    pub fn estimate(&self) -> String {
        if self.use_resolution {
            return String::from("The octree levels at or above that spacing, up to the budget.");
        }
        let budget = u64::try_from(self.budget).unwrap_or(0);
        let step = PointCloudEngine::decimation_for_budget(self.point_count, budget);
        let mut text = if step > 1 {
            format!(
                "One point in {step} - about {} of {}.",
                grouped(self.point_count / u64::from(step)),
                grouped(self.point_count)
            )
        } else {
            format!("Every point - {}.", grouped(self.point_count))
        };
        // A class filter thins after the step, and how much depends on the file: say so rather
        // than print a number that is only an upper bound.
        if self.class() >= 0 {
            text.push_str(" Fewer with a class filter.");
        }
        text
    }

    /// The IMPORT line the fields make, or why there is none.
    pub fn command(&self) -> Result<String> {
        let mut line = format!("IMPORT {}", path_word(&self.path)?);
        let name = self.name.trim();
        if !name.is_empty() && name != stem_of(&self.path) {
            line.push_str(&import_option_word("name", name, "The name")?);
        }
        let budget = u64::try_from(self.budget).unwrap_or(0);
        if budget != PointCloudImportOptions::default().budget {
            line.push_str(&format!(" budget={budget}"));
        }
        let class = self.class();
        if class >= 0 {
            line.push_str(&format!(" class={class}"));
        }
        if self.use_resolution {
            line.push_str(" resolution=");
            line.push_str(&format_exact_real(self.resolution));
        }
        Ok(line)
    }

    /// Draws the dialog while it is open; answers once Import or Cancel was pressed.
    pub fn ui(&mut self, ui: &mut Ui, _host: &mut dyn Host) -> Option<DialogResult> {
        if self.result.is_some() {
            return self.result;
        }
        let mut command = CommandShown::of(&self.command());
        let ready = command.ready;
        let ctx = ui.ctx().clone();
        let mut frame = self.frame.clone();
        let mut pressed = None;
        let shown = {
            let mut lent = (&mut *self, &mut command, &mut pressed);
            frame.show(
                &ctx,
                &mut lent,
                |ui, lent| lent.0.body(ui, &mut *lent.1),
                |ui, lent| {
                    let buttons = ok_cancel(ui, "pointCloudImportDialog", "Import", ready);
                    if buttons.accepted {
                        *lent.2 = Some(DialogResult::Accepted);
                    }
                    if buttons.rejected {
                        *lent.2 = Some(DialogResult::Rejected);
                    }
                },
            )
        };
        self.frame = frame;
        match pressed {
            Some(answer) => self.result = Some(answer),
            None if shown.closed => self.result = Some(DialogResult::Rejected),
            None => {}
        }
        self.result
    }

    fn body(&mut self, ui: &mut Ui, command: &mut CommandShown) {
        summary_label(ui, &self.summary);
        text_row(ui, "pointCloudImportName", "Name:", &mut self.name, "", "");
        ui.horizontal(|ui| {
            ui.label("Points to keep:");
            whole(
                ui,
                "pointCloudImportBudget",
                &mut self.budget,
                1_000..=SPIN_MAX,
            );
        });
        self.classification
            .ui(ui, "pointCloudImportClasses", "Classification:", "");
        ui.horizontal(|ui| {
            ui.label(if self.copc {
                "Level of detail:"
            } else {
                "Level of detail (COPC only):"
            });
            ui.add_enabled_ui(self.copc, |ui| {
                check(
                    ui,
                    "pointCloudImportUseResolution",
                    "Read the COPC octree at a point spacing of",
                    &mut self.use_resolution,
                );
            });
            ui.add_enabled_ui(self.use_resolution, |ui| {
                spin(
                    ui,
                    "pointCloudImportResolution",
                    &mut self.resolution,
                    SpinRange::new(0.001, 1_000_000.0, 1.0, 3).with_suffix(" units"),
                );
            });
        });
        ui.label(RichText::new(self.estimate()).color(palette::text_muted().to_egui()));
        command.ui(ui, "pointCloudImportCommand");
    }
}
