//! Export Drawing's dialog (gis_export_dialog.hpp; File > Export > Export Drawing...;
//! docs/interop.md, "Export options"): the window onto EXPORT, built on the GIS dialog frame
//! (geo::gis_tool_dialog). What is written is the shared "Apply to" and "Only those that match"
//! controls (CLAUDE.md section 1.1); the rest are EXPORT's words. The dialog writes nothing
//! itself: Run hands the EXPORT line it shows to the window's one executor, and Preview runs it
//! with PREVIEW, which says what the scope takes and writes nothing.
//!
//! Its fields, by object name (the frame's - vectorExportDialog, vectorExportCommand,
//! vectorExportPreview, vectorExportRun, vectorExportReply ... - are in gis_tool_dialog; the
//! scope's are vectorExportScope, vectorExportScopeDrawing, ...):
//!
//! ```text
//! vectorExportFile              the file; its extension picks the format
//! vectorExportBrowse            choose it with a file dialog
//! vectorExportLayerName         the layer's name in the file (layername=)
//! vectorExportSplit             one file layer per drawing layer (split=layer)
//! vectorExportAppend            add to the file rather than replace it
//! vectorExportCrs               project | native | a code (crs=)
//! vectorExportCreationOptions   KEY=VALUE ... (co=)
//! vectorExportLayerOptions      KEY=VALUE ... (lco=)
//! vectorExportText              skip | points (text=)
//! vectorExportCurve             the chords' largest stray from an arc (curve=)
//! vectorExportProperties        write entity properties (properties=)
//! ```
//!
//! A .dxf or an archive (.12da) takes the scope alone: the GDAL fields are off for one.

use egui::Ui;
use katana_app::geo::replies::parse_records;
use katana_core::error::make_error;
use katana_core::text::parse_finite_double;
use katana_core::{Error, ErrorCode, Result};
use katana_interop::export::vector_export_formats;

use crate::customisation::ScopeChoice;
use crate::drive::{Choice, button, check, choice};
use crate::geo::gis_tool_dialog::{
    GisDialogContext, GisScopeWords, GisTool, GisToolFrame, gis_field, gis_number_option,
    gis_scope, gis_summary, gis_word,
};
use crate::host::Host;
use crate::ifc_dialogs::get_save_file_name;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VectorExportForm {
    pub file: String,
    pub scope: GisScopeWords,
    pub layer_name: String,
    pub split: bool,
    pub append: bool,
    /// Empty: the project's (the verb's default).
    pub crs: String,
    /// Blank-separated KEY=VALUE.
    pub creation_options: String,
    /// Blank-separated KEY=VALUE.
    pub layer_options: String,
    pub text_as_points: bool,
    /// Empty: the verb's 1 mm.
    pub curve: String,
    pub properties: bool,
}

impl Default for VectorExportForm {
    fn default() -> Self {
        Self {
            file: String::new(),
            scope: GisScopeWords::default(),
            layer_name: String::new(),
            split: false,
            append: false,
            crs: String::new(),
            creation_options: String::new(),
            layer_options: String::new(),
            text_as_points: false,
            curve: String::new(),
            properties: true,
        }
    }
}

fn invalid(message: String) -> Error {
    make_error(ErrorCode::InvalidArgument, message, String::new())
}

/// " key=value" for each blank-separated KEY=VALUE of `text`.
fn option_words(key: &str, text: &str, field: &str) -> Result<String> {
    let mut words = String::new();
    for option in text.split_whitespace() {
        // A key before the '=': an option that starts with it, or has none, is no KEY=VALUE.
        if !option.find('=').is_some_and(|at| at > 0) {
            return Err(invalid(format!(
                "{field}: an option is KEY=VALUE, not '{option}'"
            )));
        }
        if option.contains('"') {
            return Err(invalid(format!(
                "{field} holds a double quote, which a command line cannot carry"
            )));
        }
        words.push_str(&format!(" {key}={option}"));
    }
    Ok(words)
}

/// Whether EXPORT writes `file` natively (a .dxf, an archive), which takes a scope and none of
/// GDAL's options.
pub fn export_is_native(file: &str) -> bool {
    let lower = file.trim().to_lowercase();
    lower.ends_with(".dxf") || lower.ends_with(".12da") || lower.ends_with(".12daz")
}

/// EXPORT "<file>" <scope> [layername=<n>] [split=layer] [append] [crs=<c>] [co=K=V]...
/// [lco=K=V]... [text=points] [curve=<m>] [properties=no], exactly as it would be typed; a
/// default is left out. InvalidArgument naming the field for no file, a double quote, an option
/// that is not KEY=VALUE, a curve that is not a number above 0, a layer name with split, and a
/// scope the controls cannot say.
pub fn vector_export_line(form: &VectorExportForm) -> Result<String> {
    let file = form.file.trim();
    if file.is_empty() {
        return Err(invalid(String::from("choose the file to write first")));
    }
    if file.contains('"') {
        return Err(invalid(String::from(
            "the file's path holds a double quote, which a command line cannot carry",
        )));
    }
    let scope = gis_scope(&form.scope)?;
    let mut line = format!("EXPORT \"{file}\" {scope}");
    if export_is_native(file) {
        // A .dxf and an archive take the scope alone.
        return Ok(line);
    }
    let name = form.layer_name.trim();
    if !name.is_empty() {
        if form.split {
            return Err(invalid(String::from(
                "one file layer per drawing layer is named after it: clear the layer name",
            )));
        }
        line.push_str(" layername=");
        line.push_str(&gis_word(name, "The layer name")?);
    }
    if form.split {
        line.push_str(" split=layer");
    }
    if form.append {
        line.push_str(" append");
    }
    let crs = form.crs.trim();
    if !crs.is_empty() && crs != "project" {
        line.push_str(" crs=");
        line.push_str(&gis_word(crs, "The coordinate system")?);
    }
    let creation = option_words("co", &form.creation_options, "Creation options")?;
    let layer = option_words("lco", &form.layer_options, "Layer options")?;
    line.push_str(&creation);
    line.push_str(&layer);
    if form.text_as_points {
        line.push_str(" text=points");
    }
    let curve = gis_number_option("curve", &form.curve, "Arcs as chords within")?;
    // What reads is finite, so "not above 0" is "at most 0".
    if !curve.is_empty() && parse_finite_double(form.curve.trim()).unwrap_or(0.0) <= 0.0 {
        return Err(invalid(String::from(
            "Arcs as chords within must be a distance above 0",
        )));
    }
    line.push_str(&curve);
    if !form.properties {
        line.push_str(" properties=no");
    }
    Ok(line)
}

/// What the Text choice offers, and the word each stands for.
const TEXT_CHOICES: [(&str, &str); 2] = [
    ("Leave text out (counted)", "skip"),
    ("Text as points with its text, height and angle", "points"),
];

/// Export Drawing's dialog (see the module comment).
pub struct VectorExportDialog {
    frame: GisToolFrame,
    file: String,
    layer_name: String,
    split: bool,
    append: bool,
    crs_items: Vec<String>,
    crs: Choice,
    creation_options: String,
    layer_options: String,
    text_items: Vec<String>,
    text: Choice,
    curve: String,
    properties: bool,
}

impl VectorExportDialog {
    pub fn new(context: GisDialogContext, host: &dyn Host) -> Self {
        let mut frame = GisToolFrame::new("vectorExport", "Export Drawing", context, host, true);
        // A file handed to someone is usually the whole drawing.
        if let Some(scope) = frame.scope_controls_mut() {
            scope.set_choice(ScopeChoice::Drawing);
        }
        let crs_items = vec![
            String::from("project"),
            String::from("native"),
            String::from("EPSG:4326"),
        ];
        // An editable choice showing its first item, as a QComboBox filled does.
        let mut crs = Choice::editable("project");
        crs.index = Some(0);
        Self {
            frame,
            file: String::new(),
            layer_name: String::new(),
            split: false,
            append: false,
            crs_items,
            crs,
            creation_options: String::new(),
            layer_options: String::new(),
            text_items: TEXT_CHOICES
                .iter()
                .map(|(shown, _)| String::from(*shown))
                .collect(),
            text: Choice::at(0),
            curve: String::new(),
            properties: true,
        }
    }

    /// The window's Export Drawing dialog, as the menu makes it.
    pub fn make(context: GisDialogContext, host: &dyn Host) -> Box<dyn GisTool> {
        Box::new(Self::new(context, host))
    }

    pub fn object_name(&self) -> &str {
        self.frame.object_name()
    }

    pub fn window_title(&self) -> &str {
        self.frame.title()
    }

    /// The file to write, as File > Export > Export Drawing's file dialog chose it.
    pub fn set_file(&mut self, file: &str) {
        self.file = String::from(file);
    }

    /// Whether the GDAL fields apply: not for a file EXPORT writes natively.
    fn gdal(&self) -> bool {
        !export_is_native(&self.file)
    }

    pub fn form(&self) -> VectorExportForm {
        let layer_name_enabled = self.gdal() && !self.split;
        VectorExportForm {
            file: self.file.clone(),
            scope: self.frame.scope_words(),
            layer_name: if layer_name_enabled {
                self.layer_name.clone()
            } else {
                String::new()
            },
            split: self.split,
            append: self.append,
            crs: self.crs.current_text(&self.crs_items),
            creation_options: self.creation_options.clone(),
            layer_options: self.layer_options.clone(),
            text_as_points: self
                .text
                .index
                .and_then(|at| TEXT_CHOICES.get(at))
                .is_some_and(|(_, word)| *word == "points"),
            curve: self.curve.clone(),
            properties: self.properties,
        }
    }

    /// Browse: a file dialog on the desktop; headless, a sentence saying which field to fill, as
    /// nobody could close a file dialog there.
    fn browse(&mut self, host: &dyn Host) {
        if host.headless() {
            self.frame.set_status(
                "A headless session opens no file dialog: fill vectorExportFile with the path \
                 instead.",
                true,
            );
            return;
        }
        let mut filters: Vec<String> = vector_export_formats()
            .iter()
            .map(|format| format!("{} (*.{})", format.description, format.extension))
            .collect();
        filters.push(String::from("Archive (*.12da)"));
        filters.push(String::from("Archive, zipped (*.12daz)"));
        let chosen = get_save_file_name("Export", &self.file, &filters.join(";;"));
        if !chosen.is_empty() {
            self.set_file(&chosen);
        }
    }
}

impl GisTool for VectorExportDialog {
    fn frame(&self) -> &GisToolFrame {
        &self.frame
    }

    fn frame_mut(&mut self) -> &mut GisToolFrame {
        &mut self.frame
    }

    fn command(&self) -> Result<String> {
        vector_export_line(&self.form())
    }

    fn summary(&self, reply: &str) -> String {
        for record in parse_records(reply) {
            if record.kind == "export" && record.get("preview") == Some("yes") {
                return format!(
                    "Preview: {} entities would be written; nothing was.",
                    record.get("entities").unwrap_or("?")
                );
            }
            if record.kind == "export" && record.get("ran") == Some("no") {
                return String::from("The scope took nothing: nothing was written.");
            }
            if record.kind == "exported" {
                let count = record
                    .get("features")
                    .or_else(|| record.get("entities"))
                    .unwrap_or("?");
                return format!(
                    "Wrote {count} features to {}.",
                    record.get("file").unwrap_or("the file")
                );
            }
        }
        gis_summary(reply)
    }

    fn fields_ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        gis_field(
            ui,
            "vectorExportFile",
            "File:",
            &mut self.file,
            "the file to write, e.g. site.gpkg",
            "Its extension picks the format: .gpkg, .geojson, .shp, .kml, .dxf, .12da ...",
        );
        let browse = button(ui, "vectorExportBrowse", "Browse...").clicked();
        // A .dxf and an archive take the scope alone: the GDAL fields are off for one.
        let gdal = self.gdal();
        // One file layer per drawing layer names each: the layer name goes off.
        ui.add_enabled_ui(gdal && !self.split, |ui| {
            gis_field(
                ui,
                "vectorExportLayerName",
                "Layer name in the file:",
                &mut self.layer_name,
                "katana",
                "layername=: the layer's name inside the file, for formats that hold several \
                 (GeoPackage, KML). Katana's own layers go out as each feature's 'layer' \
                 attribute.",
            );
        });
        ui.add_enabled_ui(gdal, |ui| {
            check(
                ui,
                "vectorExportSplit",
                "One file layer per drawing layer",
                &mut self.split,
            )
            .on_hover_text(
                "split=layer: each drawing layer becomes a layer of the file, named after it",
            );
            check(
                ui,
                "vectorExportAppend",
                "Add to the file rather than replace it",
                &mut self.append,
            )
            .on_hover_text(
                "append: a GeoPackage keeps its layers and gains this one; a layer name it has \
                 already is refused",
            );
            ui.horizontal(|ui| {
                ui.label("Coordinates:");
                choice(ui, "vectorExportCrs", &self.crs_items, &mut self.crs).on_hover_text(
                    "project: the project's coordinate system (a GeoJSON in longitude and \
                     latitude, as RFC 7946 says); native: the project's where the format allows \
                     it; a code: the coordinates moved into it (crs=)",
                );
            });
            gis_field(
                ui,
                "vectorExportCreationOptions",
                "Creation options:",
                &mut self.creation_options,
                "none, e.g. VERSION=1.4",
                "co=: the driver's creation options, KEY=VALUE separated by blanks, each checked \
                 against its list (FORMATS OPTIONS <driver>)",
            );
            gis_field(
                ui,
                "vectorExportLayerOptions",
                "Layer options:",
                &mut self.layer_options,
                "none, e.g. SPATIAL_INDEX=NO",
                "lco=: the driver's layer creation options, checked likewise",
            );
            ui.horizontal(|ui| {
                ui.label("Text:");
                choice(ui, "vectorExportText", &self.text_items, &mut self.text);
            });
            // Arcs and circles have no exact form in these formats. The tolerance is the sagitta
            // - the furthest a chord may stray from the true curve - and is the user's to
            // choose, because it is a loss of accuracy.
            gis_field(
                ui,
                "vectorExportCurve",
                "Arcs as chords within:",
                &mut self.curve,
                "0.001",
                "curve=: the largest distance a chord may stray from its arc, in model units",
            );
            check(
                ui,
                "vectorExportProperties",
                "Write entity properties as attributes",
                &mut self.properties,
            );
        });
        if browse {
            self.browse(&*host);
        }
    }
}
