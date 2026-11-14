//! GIS > Analysis - GDAL > Dissolve...: the window onto GIS DISSOLVE (geo/dissolve_dialog.hpp;
//! docs/geoprocessing.md, "V1"), built on the GIS dialog frame (gis_tool_dialog).
//!
//! Its fields, by object name (the frame's are in gis_tool_dialog; the scope's are
//! gisDissolveScope, gisDissolveScopeDrawing, ...):
//!
//! ```text
//! gisDissolveBy             merge areas whose properties agree (a,b); blank merges every area
//!                           in the scope
//! gisDissolveKeepIdentical  keep the other properties the whole group shares
//! gisDissolveReplace        delete the areas merged, in the same undo step
//! gisDissolveLayer          the layer the result goes to (default gis/dissolve)
//! ```

use egui::Ui;
use katana_core::Result;

use super::gis_tool_dialog::{
    GisDialogContext, GisScopeWords, GisTool, GisToolFrame, gis_field, gis_layer_target,
    gis_scope, gis_word,
};
use crate::drive::check;
use crate::host::Host;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GisDissolveForm {
    pub scope: GisScopeWords,
    pub by: String,
    pub keep_identical: bool,
    pub replace: bool,
    pub layer: String,
}

/// GIS DISSOLVE <scope> [by=a,b] [keep=identical] [TO LAYER <layer>] [REPLACE], exactly as it
/// would be typed. InvalidArgument naming the field for a scope the controls cannot say and a
/// double quote anywhere.
pub fn gis_dissolve_line(form: &GisDissolveForm) -> Result<String> {
    let scope = gis_scope(&form.scope)?;
    let mut line = format!("GIS DISSOLVE {scope}");
    let by = form.by.trim();
    if !by.is_empty() {
        let word = gis_word(&format!("by={by}"), "The properties to merge by")?;
        line.push(' ');
        line.push_str(&word);
    }
    if form.keep_identical {
        line.push_str(" keep=identical");
    }
    line.push_str(&gis_layer_target(&form.layer)?);
    if form.replace {
        line.push_str(" REPLACE");
    }
    Ok(line)
}

/// GIS > Analysis - GDAL > Dissolve... (see the module comment).
pub struct GisDissolveDialog {
    frame: GisToolFrame,
    by: String,
    keep: bool,
    replace: bool,
    layer: String,
}

impl GisDissolveDialog {
    pub fn new(context: GisDialogContext, host: &dyn Host) -> Self {
        Self {
            frame: GisToolFrame::new("gisDissolve", "Dissolve", context, host, true),
            by: String::new(),
            keep: false,
            replace: false,
            layer: String::new(),
        }
    }

    /// The window's Dissolve dialog, as the menu makes it.
    pub fn make(context: GisDialogContext, host: &dyn Host) -> Box<dyn GisTool> {
        Box::new(Self::new(context, host))
    }

    pub fn form(&self) -> GisDissolveForm {
        GisDissolveForm {
            scope: self.frame.scope_words(),
            by: self.by.clone(),
            keep_identical: self.keep,
            replace: self.replace,
            layer: self.layer.clone(),
        }
    }
}

impl GisTool for GisDissolveDialog {
    fn frame(&self) -> &GisToolFrame {
        &self.frame
    }

    fn frame_mut(&mut self) -> &mut GisToolFrame {
        &mut self.frame
    }

    fn command(&self) -> Result<String> {
        gis_dissolve_line(&self.form())
    }

    fn fields_ui(&mut self, ui: &mut Ui, _host: &mut dyn Host) {
        gis_field(
            ui,
            "gisDissolveBy",
            "Merge by properties:",
            &mut self.by,
            "none - merge every area",
            "by=a,b: areas merge only with those whose properties agree - superlots by owner, \
             stages by stage",
        );
        check(
            ui,
            "gisDissolveKeepIdentical",
            "Keep the properties each group shares",
            &mut self.keep,
        )
        .on_hover_text(
            "keep=identical: a property every area of a group holds alike is kept on the merged \
             area; the rest are left behind",
        );
        check(
            ui,
            "gisDissolveReplace",
            "Delete the areas merged",
            &mut self.replace,
        )
        .on_hover_text("REPLACE: the areas that went in are deleted in the same undo step");
        gis_field(
            ui,
            "gisDissolveLayer",
            "Result layer:",
            &mut self.layer,
            "default gis/dissolve",
            "TO LAYER: the layer the merged areas are drawn on",
        );
    }
}
