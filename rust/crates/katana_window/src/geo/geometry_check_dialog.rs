//! GIS > Check - GDAL > Check Geometry... and Repair Geometry...: the windows onto GIS CHECK and
//! GIS REPAIR (geo/geometry_check_dialog.hpp; docs/geoprocessing.md, "V4"), built on the GIS
//! dialog frame (gis_tool_dialog): each line shown as it will run, and Run handing it to the
//! window's one executor.
//!
//! Their fields, by object name (the frame's are in gis_tool_dialog):
//!
//! ```text
//! gisCheckDialog     Check Geometry; its scope gisCheckScope, ...
//!   gisCheckMarkers  a layer to draw a point at each problem on, replacing the last check's
//!                    markers there; blank draws none
//!   gisCheckProblems the problems found, a row each
//! gisRepairDialog    Repair Geometry; its scope gisRepairScope, ...
//!   gisRepairMethod  linework (GEOS's default, keeps every edge) or structure (rebuilds from
//!                    the rings' structure)
//! ```

use std::collections::BTreeSet;

use egui::Ui;
use katana_app::geo::replies::parse_records;
use katana_core::Result;

use super::buffer_dialog::Words;
use super::gis_tool_dialog::{
    GisDialogContext, GisScopeWords, GisTool, GisToolFrame, gis_field, gis_scope, gis_summary,
    gis_word,
};
use crate::host::Host;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GisCheckForm {
    pub scope: GisScopeWords,
    pub markers: String,
}

/// GIS CHECK <scope> [markers=<layer>], exactly as it would be typed.
pub fn gis_check_line(form: &GisCheckForm) -> Result<String> {
    let scope = gis_scope(&form.scope)?;
    let mut line = format!("GIS CHECK {scope}");
    let markers = form.markers.trim();
    if !markers.is_empty() {
        line.push(' ');
        line.push_str(&gis_word(
            &format!("markers={markers}"),
            "The markers' layer",
        )?);
    }
    Ok(line)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GisRepairForm {
    pub scope: GisScopeWords,
    pub method: String,
}

impl Default for GisRepairForm {
    fn default() -> Self {
        Self {
            scope: GisScopeWords::default(),
            method: String::from("linework"),
        }
    }
}

/// GIS REPAIR <scope> [method=structure]; the default method left out.
pub fn gis_repair_line(form: &GisRepairForm) -> Result<String> {
    let scope = gis_scope(&form.scope)?;
    let mut line = format!("GIS REPAIR {scope}");
    if form.method != "linework" {
        line.push_str(" method=");
        line.push_str(&form.method);
    }
    Ok(line)
}

/// A check's reply in a sentence: how many problems, on how many entities.
pub fn gis_problems_summary(reply: &str, none: &str) -> String {
    let mut problems: usize = 0;
    let mut entities: BTreeSet<String> = BTreeSet::new();
    for record in parse_records(reply) {
        if record.kind == "problem" {
            problems += 1;
            if let Some(entity) = record.get("entity")
                && !entity.is_empty()
            {
                entities.insert(String::from(entity));
            }
        }
    }
    if problems == 0 {
        return String::from(none);
    }
    format!(
        "{problems} problems on {} entities: each is a row of the table, and a problem record in \
         the reply.",
        entities.len()
    )
}

/// GIS > Check - GDAL > Check Geometry... (see the module comment).
pub struct GisCheckDialog {
    frame: GisToolFrame,
    markers: String,
}

impl GisCheckDialog {
    pub fn new(context: GisDialogContext, host: &dyn Host) -> Self {
        let mut frame = GisToolFrame::new("gisCheck", "Check Geometry", context, host, true);
        frame.add_problems_table();
        Self {
            frame,
            markers: String::new(),
        }
    }

    /// The window's Check Geometry dialog, as the menu makes it.
    pub fn make(context: GisDialogContext, host: &dyn Host) -> Box<dyn GisTool> {
        Box::new(Self::new(context, host))
    }

    pub fn form(&self) -> GisCheckForm {
        GisCheckForm {
            scope: self.frame.scope_words(),
            markers: self.markers.clone(),
        }
    }
}

impl GisTool for GisCheckDialog {
    fn frame(&self) -> &GisToolFrame {
        &self.frame
    }

    fn frame_mut(&mut self) -> &mut GisToolFrame {
        &mut self.frame
    }

    fn command(&self) -> Result<String> {
        gis_check_line(&self.form())
    }

    fn summary(&self, reply: &str) -> String {
        if reply.contains(" preview=yes") || reply.contains(" ran=no") {
            return gis_summary(reply);
        }
        gis_problems_summary(
            reply,
            "No problem: every line and area in the scope is valid.",
        )
    }

    fn fields_ui(&mut self, ui: &mut Ui, _host: &mut dyn Host) {
        gis_field(
            ui,
            "gisCheckMarkers",
            "Markers on layer:",
            &mut self.markers,
            "none - report only",
            "markers=<layer>: a point at each problem on this layer, replacing the points the \
             last check drew there",
        );
    }
}

/// GIS > Check - GDAL > Repair Geometry... (see the module comment).
pub struct GisRepairDialog {
    frame: GisToolFrame,
    method: Words,
}

impl GisRepairDialog {
    pub fn new(context: GisDialogContext, host: &dyn Host) -> Self {
        Self {
            frame: GisToolFrame::new("gisRepair", "Repair Geometry", context, host, true),
            method: Words::new(&["linework", "structure"]),
        }
    }

    /// The window's Repair Geometry dialog, as the menu makes it.
    pub fn make(context: GisDialogContext, host: &dyn Host) -> Box<dyn GisTool> {
        Box::new(Self::new(context, host))
    }

    pub fn form(&self) -> GisRepairForm {
        GisRepairForm {
            scope: self.frame.scope_words(),
            method: self.method.text_or("linework"),
        }
    }
}

impl GisTool for GisRepairDialog {
    fn frame(&self) -> &GisToolFrame {
        &self.frame
    }

    fn frame_mut(&mut self) -> &mut GisToolFrame {
        &mut self.frame
    }

    fn command(&self) -> Result<String> {
        gis_repair_line(&self.form())
    }

    fn fields_ui(&mut self, ui: &mut Ui, _host: &mut dyn Host) {
        self.method.ui(
            ui,
            "gisRepairMethod",
            "Method:",
            "method=: linework keeps every edge the area had and nodes them; structure rebuilds \
             it from its rings, dropping what is inside out",
        );
    }
}
