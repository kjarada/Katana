//! GIS > Analysis - GDAL > Clip to Boundary...: the window onto GIS CLIP (geo/clip_dialog.hpp;
//! docs/geoprocessing.md, "V3"), built on the GIS dialog frame (gis_tool_dialog).
//!
//! Two scopes: what is clipped is the frame's (gisClipScope, gisClipScopeDrawing, ...), and the
//! boundary a second set of the same controls (gisClipByScope, gisClipByScopeDrawing, ...) - or a
//! file. Its fields, by object name:
//!
//! ```text
//! gisClipBySourceDrawing  clip to the areas the second scope takes
//! gisClipBySourceFile     ... or to a file's (radio buttons)
//! gisClipByFile           the file's path
//! gisClipByFileBrowse     choose it with a file dialog
//! gisClipByFileLayer      the file's layer (blank: its first)
//! gisClipByFileWhere      only the file's features this SQL WHERE takes
//! gisClipReplace          cut the entities in place, instead of drawing the pieces on a layer
//! gisClipLayer            the layer the pieces go to (default gis/clip)
//! ```

use egui::Ui;
use katana_core::error::make_error;
use katana_core::{ErrorCode, Result};

use super::gis_tool_dialog::{
    GisDialogContext, GisScopeWords, GisTool, GisToolFrame, gis_field, gis_layer_target,
    gis_scope, gis_word,
};
use crate::customisation::{ScopeChoice, ScopeFilterWidget};
use crate::drive::{button, check, radio};
use crate::host::Host;
use crate::ifc_dialogs::{get_open_file_name, to_native_separators};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GisClipForm {
    pub scope: GisScopeWords,
    pub by_file: bool,
    pub by: GisScopeWords,
    pub file: String,
    pub file_layer: String,
    pub file_where: String,
    pub replace: bool,
    pub layer: String,
}

/// GIS CLIP <scope> BY (<scope> | FILE <path> [LAYER <name>] [where="<sql>"]) [TO LAYER <layer> |
/// REPLACE], exactly as it would be typed. InvalidArgument naming the field for a scope the
/// controls cannot say, a file left empty and a double quote anywhere; with Replace the layer is
/// not written.
pub fn gis_clip_line(form: &GisClipForm) -> Result<String> {
    let scope = gis_scope(&form.scope)?;
    let mut line = format!("GIS CLIP {scope} BY ");
    if form.by_file {
        let path = form.file.trim();
        if path.is_empty() {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "choose the boundary's file first",
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
        let clause = form.file_where.trim();
        if !clause.is_empty() {
            line.push(' ');
            line.push_str(&gis_word(
                &format!("where={clause}"),
                "The file's WHERE clause",
            )?);
        }
    } else {
        let by = gis_scope(&form.by).map_err(|error| {
            make_error(
                ErrorCode::InvalidArgument,
                format!("the boundary: {}", error.message),
                String::new(),
            )
        })?;
        line.push_str(&by);
    }
    if form.replace {
        line.push_str(" REPLACE");
        return Ok(line);
    }
    line.push_str(&gis_layer_target(&form.layer)?);
    Ok(line)
}

/// GIS > Analysis - GDAL > Clip to Boundary... (see the module comment).
pub struct GisClipDialog {
    frame: GisToolFrame,
    by: ScopeFilterWidget,
    from_file: bool,
    file: String,
    file_layer: String,
    file_where: String,
    replace: bool,
    layer: String,
}

impl GisClipDialog {
    pub fn new(context: GisDialogContext, host: &dyn Host) -> Self {
        let mut by = ScopeFilterWidget::new("gisClipBy");
        by.views = context.views_fn();
        by.set_choice(ScopeChoice::Layers);
        let mut frame = GisToolFrame::new("gisClip", "Clip to Boundary", context, host, true);
        frame.set_width(1150.0);
        Self {
            frame,
            by,
            from_file: false,
            file: String::new(),
            file_layer: String::new(),
            file_where: String::new(),
            replace: false,
            layer: String::new(),
        }
    }

    /// The window's Clip to Boundary dialog, as the menu makes it.
    pub fn make(context: GisDialogContext, host: &dyn Host) -> Box<dyn GisTool> {
        Box::new(Self::new(context, host))
    }

    /// The boundary's scope controls.
    pub fn by_controls(&mut self) -> &mut ScopeFilterWidget {
        &mut self.by
    }

    pub fn form(&self) -> GisClipForm {
        GisClipForm {
            scope: self.frame.scope_words(),
            by_file: self.from_file,
            by: GisScopeWords::of(&self.by),
            file: self.file.clone(),
            file_layer: self.file_layer.clone(),
            file_where: self.file_where.clone(),
            replace: self.replace,
            layer: self.layer.clone(),
        }
    }

    /// Browse: a file dialog on the desktop; headless, a sentence saying which field to fill, as
    /// nobody could close a file dialog there.
    fn browse(&mut self, host: &dyn Host) {
        if host.headless() {
            self.frame.set_status(
                "A headless session opens no file dialog: fill gisClipByFile with the path \
                 instead.",
                true,
            );
            return;
        }
        let path = get_open_file_name("Clip to Boundary", &self.file, "");
        if !path.is_empty() {
            self.file = to_native_separators(&path);
            self.from_file = true;
        }
    }
}

impl GisTool for GisClipDialog {
    fn frame(&self) -> &GisToolFrame {
        &self.frame
    }

    fn frame_mut(&mut self) -> &mut GisToolFrame {
        &mut self.frame
    }

    fn command(&self) -> Result<String> {
        gis_clip_line(&self.form())
    }

    fn reload(&mut self, host: &dyn Host) {
        self.frame.reload_scope(&mut self.by, host);
        self.frame.reload(host);
    }

    fn fields_ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        ui.horizontal(|ui| {
            ui.label("Clip to:");
            if radio(
                ui,
                "gisClipBySourceDrawing",
                "Areas in the drawing (below)",
                !self.from_file,
            )
            .clicked()
            {
                self.from_file = false;
            }
            if radio(ui, "gisClipBySourceFile", "A file", self.from_file).clicked() {
                self.from_file = true;
            }
        });
        let file = self.from_file;
        ui.add_enabled_ui(!file, |ui| {
            self.by.ui(ui);
        });
        let mut browse = false;
        ui.add_enabled_ui(file, |ui| {
            gis_field(
                ui,
                "gisClipByFile",
                "File:",
                &mut self.file,
                "a GeoPackage, shapefile, GeoJSON ...",
                "FILE: clip to this file's areas, read by GDAL",
            );
            browse = button(ui, "gisClipByFileBrowse", "Browse...").clicked();
            gis_field(
                ui,
                "gisClipByFileLayer",
                "File's layer:",
                &mut self.file_layer,
                "its first",
                "LAYER: which of the file's layers",
            );
            gis_field(
                ui,
                "gisClipByFileWhere",
                "Only where:",
                &mut self.file_where,
                "e.g. kind='site'",
                "where=: an SQL WHERE clause on the file's fields",
            );
        });
        check(
            ui,
            "gisClipReplace",
            "Cut the entities in place",
            &mut self.replace,
        )
        .on_hover_text(
            "REPLACE: each entity keeps its id on its largest piece; the rest are made beside \
             it, and what lies wholly outside is deleted - one undo step",
        );
        // In place there is no layer for the pieces to go to.
        ui.add_enabled_ui(!self.replace, |ui| {
            gis_field(
                ui,
                "gisClipLayer",
                "Result layer:",
                &mut self.layer,
                "default gis/clip",
                "TO LAYER: the layer the pieces are drawn on, the entities left whole",
            );
        });
        if browse {
            self.browse(&*host);
        }
    }
}
