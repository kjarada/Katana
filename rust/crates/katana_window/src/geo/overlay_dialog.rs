//! GIS > Analysis - GDAL > Overlay...: the window onto GIS OVERLAY (geo/overlay_dialog.hpp;
//! docs/geoprocessing.md, "V2"), built on the GIS dialog frame (gis_tool_dialog).
//!
//! Two scopes: the subject is the frame's (gisOverlayScope, gisOverlayScopeDrawing, ...), and
//! what it is overlaid with is a second set of the same controls (gisOverlayWithScope,
//! gisOverlayWithScopeDrawing, gisOverlayWithFilterLayer, ...) - or a file. Its fields, by object
//! name:
//!
//! ```text
//! gisOverlayOperation          intersection | difference | union | symdifference | identity |
//!                              update | clip
//! gisOverlayWithSourceDrawing  overlay with what the second scope takes
//! gisOverlayWithSourceFile     ... or with a file (radio buttons)
//! gisOverlayWithFile           the file's path
//! gisOverlayWithFileBrowse     choose it with a file dialog
//! gisOverlayWithFileLayer      the file's layer (blank: its first)
//! gisOverlayWithFileWhere      only the file's features this SQL WHERE takes
//! gisOverlayKeep               the subject's properties to carry (a,b | all | none; blank: all)
//! gisOverlayKeepWith           the overlay's likewise
//! gisOverlayLayer              the layer the pieces go to (default gis/overlay)
//! gisOverlayCsv                a .csv to write the rows to (optional)
//! gisOverlayOverwrite          replace that file when it exists
//! ```

use egui::Ui;
use katana_core::error::make_error;
use katana_core::{ErrorCode, Result};

use super::buffer_dialog::Words;
use super::gis_tool_dialog::{
    GisDialogContext, GisScopeWords, GisTool, GisToolFrame, gis_field, gis_layer_target,
    gis_scope, gis_word,
};
use crate::customisation::{ScopeChoice, ScopeFilterWidget};
use crate::drive::{button, check, radio};
use crate::host::Host;
use crate::ifc_dialogs::{get_open_file_name, to_native_separators};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GisOverlayForm {
    pub operation: String,
    pub subject: GisScopeWords,
    pub with_file: bool,
    pub with: GisScopeWords,
    pub file: String,
    pub file_layer: String,
    pub file_where: String,
    pub keep: String,
    pub keep_with: String,
    pub layer: String,
    pub csv: String,
    pub overwrite: bool,
}

impl Default for GisOverlayForm {
    fn default() -> Self {
        Self {
            operation: String::from("intersection"),
            subject: GisScopeWords::default(),
            with_file: false,
            with: GisScopeWords::default(),
            file: String::new(),
            file_layer: String::new(),
            file_where: String::new(),
            keep: String::new(),
            keep_with: String::new(),
            layer: String::new(),
            csv: String::new(),
            overwrite: false,
        }
    }
}

/// " key=value" for a value given, quoted when it holds a blank.
fn option(key: &str, text: &str, field: &str) -> Result<String> {
    let value = text.trim();
    if value.is_empty() {
        return Ok(String::new());
    }
    let word = gis_word(&format!("{key}={value}"), field)?;
    Ok(format!(" {word}"))
}

/// GIS OVERLAY <operation> <subject> WITH (<scope> | FILE <path> [LAYER <name>] [where="<sql>"])
/// [keep=..] [keepwith=..] [TO LAYER <layer>] [csv=<file>] [OVERWRITE], exactly as it would be
/// typed. InvalidArgument naming the field for a scope the controls cannot say, a file left
/// empty, and a double quote anywhere but a WHERE clause's own single quotes.
pub fn gis_overlay_line(form: &GisOverlayForm) -> Result<String> {
    let subject = gis_scope(&form.subject)?;
    let mut line = format!("GIS OVERLAY {} {subject} WITH ", form.operation);
    if form.with_file {
        let path = form.file.trim();
        if path.is_empty() {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "choose the file to overlay with first",
                String::new(),
            ));
        }
        line.push_str("FILE ");
        line.push_str(&gis_word(path, "The file's path")?);
        let layer = form.file_layer.trim();
        if !layer.is_empty() {
            line.push_str(" LAYER ");
            line.push_str(&gis_word(layer, "The file's layer")?);
        }
        line.push_str(&option(
            "where",
            &form.file_where,
            "The file's WHERE clause",
        )?);
    } else {
        let with = gis_scope(&form.with).map_err(|error| {
            make_error(
                ErrorCode::InvalidArgument,
                format!("what to overlay with: {}", error.message),
                String::new(),
            )
        })?;
        line.push_str(&with);
    }
    for (key, text, field) in [
        ("keep", &form.keep, "The subject's properties"),
        ("keepwith", &form.keep_with, "The overlay's properties"),
        ("csv", &form.csv, "The rows' file"),
    ] {
        line.push_str(&option(key, text, field)?);
    }
    line.push_str(&gis_layer_target(&form.layer)?);
    if form.overwrite && !form.csv.trim().is_empty() {
        line.push_str(" OVERWRITE");
    }
    Ok(line)
}

/// GIS > Analysis - GDAL > Overlay... (see the module comment).
pub struct GisOverlayDialog {
    frame: GisToolFrame,
    operation: Words,
    with: ScopeFilterWidget,
    from_file: bool,
    file: String,
    file_layer: String,
    file_where: String,
    keep: String,
    keep_with: String,
    layer: String,
    csv: String,
    overwrite: bool,
}

impl GisOverlayDialog {
    pub fn new(context: GisDialogContext, host: &dyn Host) -> Self {
        let mut with = ScopeFilterWidget::new("gisOverlayWith");
        with.views = context.views_fn();
        with.set_choice(ScopeChoice::Layers);
        let mut frame = GisToolFrame::new("gisOverlay", "Overlay", context, host, true);
        frame.set_width(1150.0);
        Self {
            frame,
            operation: Words::new(&[
                "intersection",
                "difference",
                "union",
                "symdifference",
                "identity",
                "update",
                "clip",
            ]),
            with,
            from_file: false,
            file: String::new(),
            file_layer: String::new(),
            file_where: String::new(),
            keep: String::new(),
            keep_with: String::new(),
            layer: String::new(),
            csv: String::new(),
            overwrite: false,
        }
    }

    /// The window's Overlay dialog, as the menu makes it.
    pub fn make(context: GisDialogContext, host: &dyn Host) -> Box<dyn GisTool> {
        Box::new(Self::new(context, host))
    }

    /// The second scope's controls.
    pub fn with_controls(&mut self) -> &mut ScopeFilterWidget {
        &mut self.with
    }

    pub fn form(&self) -> GisOverlayForm {
        GisOverlayForm {
            operation: self.operation.text_or("intersection"),
            subject: self.frame.scope_words(),
            with_file: self.from_file,
            with: GisScopeWords::of(&self.with),
            file: self.file.clone(),
            file_layer: self.file_layer.clone(),
            file_where: self.file_where.clone(),
            keep: self.keep.clone(),
            keep_with: self.keep_with.clone(),
            layer: self.layer.clone(),
            csv: self.csv.clone(),
            overwrite: self.overwrite,
        }
    }

    /// Browse: a file dialog on the desktop; headless, a sentence saying which field to fill.
    fn browse(&mut self, host: &dyn Host) {
        if host.headless() {
            self.frame.set_status(
                "A headless session opens no file dialog: fill gisOverlayWithFile with the path \
                 instead.",
                true,
            );
            return;
        }
        let path = get_open_file_name("Overlay With", &self.file, "");
        if !path.is_empty() {
            self.file = to_native_separators(&path);
            self.from_file = true;
        }
    }
}

impl GisTool for GisOverlayDialog {
    fn frame(&self) -> &GisToolFrame {
        &self.frame
    }

    fn frame_mut(&mut self) -> &mut GisToolFrame {
        &mut self.frame
    }

    fn command(&self) -> Result<String> {
        gis_overlay_line(&self.form())
    }

    fn reload(&mut self, host: &dyn Host) {
        self.frame.reload_scope(&mut self.with, host);
        self.frame.reload(host);
    }

    fn fields_ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        self.operation.ui(
            ui,
            "gisOverlayOperation",
            "Operation:",
            "intersection: where both are (easement area per lot); difference: the subject less \
             the overlay (net developable area); union: all of both, in pieces; identity: the \
             subject split by the overlay",
        );
        ui.horizontal(|ui| {
            ui.label("Overlay with:");
            if radio(
                ui,
                "gisOverlayWithSourceDrawing",
                "What the drawing holds (below)",
                !self.from_file,
            )
            .clicked()
            {
                self.from_file = false;
            }
            if radio(ui, "gisOverlayWithSourceFile", "A file", self.from_file).clicked() {
                self.from_file = true;
            }
        });
        let file = self.from_file;
        ui.add_enabled_ui(!file, |ui| {
            self.with.ui(ui);
        });
        let mut browse = false;
        ui.add_enabled_ui(file, |ui| {
            gis_field(
                ui,
                "gisOverlayWithFile",
                "File:",
                &mut self.file,
                "a GeoPackage, shapefile, GeoJSON ...",
                "FILE: overlay with this file's features, read through GDAL",
            );
            browse = button(ui, "gisOverlayWithFileBrowse", "Browse...").clicked();
            gis_field(
                ui,
                "gisOverlayWithFileLayer",
                "File's layer:",
                &mut self.file_layer,
                "its first",
                "LAYER: which of the file's layers",
            );
            gis_field(
                ui,
                "gisOverlayWithFileWhere",
                "Only where:",
                &mut self.file_where,
                "e.g. kind='corridor'",
                "where=: an SQL WHERE clause on the file's fields",
            );
        });
        gis_field(
            ui,
            "gisOverlayKeep",
            "Keep subject's:",
            &mut self.keep,
            "all its properties",
            "keep=a,b | all | none: the subject's properties each piece carries",
        );
        gis_field(
            ui,
            "gisOverlayKeepWith",
            "Keep overlay's:",
            &mut self.keep_with,
            "all its properties",
            "keepwith=a,b | all | none: the overlay's properties each piece carries",
        );
        gis_field(
            ui,
            "gisOverlayLayer",
            "Result layer:",
            &mut self.layer,
            "default gis/overlay",
            "TO LAYER: the layer the pieces are drawn on",
        );
        gis_field(
            ui,
            "gisOverlayCsv",
            "Rows to CSV:",
            &mut self.csv,
            "none",
            "csv=: the rows - each piece's area or length and properties - written to this file \
             too",
        );
        check(
            ui,
            "gisOverlayOverwrite",
            "Replace the CSV file if it exists",
            &mut self.overwrite,
        );
        if browse {
            self.browse(&*host);
        }
    }
}
