//! GIS > Analysis - GDAL > Boundary Around Features...: the window onto GIS HULL
//! (geo/hull_dialog.hpp; docs/geoprocessing.md, "V3"), built on the GIS dialog frame
//! (gis_tool_dialog).
//!
//! Its fields, by object name (the frame's are in gis_tool_dialog; the scope's are gisHullScope,
//! gisHullScopeDrawing, ...):
//!
//! ```text
//! gisHullKind   convex (the tightest convex boundary) | concave
//! gisHullRatio  concave: 0 (the tightest) to 1 (the convex hull)
//! gisHullHoles  concave: allow holes where the points leave a gap inside
//! gisHullLayer  the layer the boundary goes to (default gis/hull)
//! ```

use egui::Ui;
use katana_core::error::make_error;
use katana_core::text::parse_finite_double;
use katana_core::{ErrorCode, Result};

use super::buffer_dialog::Words;
use super::gis_tool_dialog::{
    GisDialogContext, GisScopeWords, GisTool, GisToolFrame, gis_field, gis_layer_target,
    gis_scope,
};
use crate::drive::check;
use crate::host::Host;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GisHullForm {
    pub scope: GisScopeWords,
    pub concave: bool,
    pub ratio: String,
    pub holes: bool,
    pub layer: String,
}

/// GIS HULL <scope> [concave=<ratio> [holes]] [TO LAYER <layer>], exactly as it would be typed;
/// convex, the verb's default, is left out. InvalidArgument naming the field for a ratio that is
/// not a number from 0 to 1, and a scope the controls cannot say.
pub fn gis_hull_line(form: &GisHullForm) -> Result<String> {
    let scope = gis_scope(&form.scope)?;
    let mut line = format!("GIS HULL {scope}");
    if form.concave {
        let ratio = form.ratio.trim();
        match parse_finite_double(ratio) {
            Some(value) if (0.0..=1.0).contains(&value) => {}
            _ => {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    format!(
                        "the ratio is a number from 0 (the tightest) to 1 (the convex hull), not \
                         '{ratio}'"
                    ),
                    String::new(),
                ));
            }
        }
        line.push_str(" concave=");
        line.push_str(ratio);
        if form.holes {
            line.push_str(" holes");
        }
    }
    line.push_str(&gis_layer_target(&form.layer)?);
    Ok(line)
}

/// GIS > Analysis - GDAL > Boundary Around Features... (see the module comment).
pub struct GisHullDialog {
    frame: GisToolFrame,
    kind: Words,
    ratio: String,
    holes: bool,
    layer: String,
}

impl GisHullDialog {
    pub fn new(context: GisDialogContext, host: &dyn Host) -> Self {
        Self {
            frame: GisToolFrame::new("gisHull", "Boundary Around Features", context, host, true),
            kind: Words::new(&["convex", "concave"]),
            ratio: String::from("0.1"),
            holes: false,
            layer: String::new(),
        }
    }

    /// The window's Boundary Around Features dialog, as the menu makes it.
    pub fn make(context: GisDialogContext, host: &dyn Host) -> Box<dyn GisTool> {
        Box::new(Self::new(context, host))
    }

    pub fn form(&self) -> GisHullForm {
        GisHullForm {
            scope: self.frame.scope_words(),
            concave: self.kind.text() == "concave",
            ratio: self.ratio.clone(),
            holes: self.holes,
            layer: self.layer.clone(),
        }
    }
}

impl GisTool for GisHullDialog {
    fn frame(&self) -> &GisToolFrame {
        &self.frame
    }

    fn frame_mut(&mut self) -> &mut GisToolFrame {
        &mut self.frame
    }

    fn command(&self) -> Result<String> {
        gis_hull_line(&self.form())
    }

    fn fields_ui(&mut self, ui: &mut Ui, _host: &mut dyn Host) {
        self.kind.ui(
            ui,
            "gisHullKind",
            "Boundary:",
            "convex: the tightest boundary with no inward corner; concave: one that follows the \
             points in, as far as the ratio lets it",
        );
        // The ratio and the holes are a concave boundary's alone.
        let concave = self.kind.text() == "concave";
        ui.add_enabled_ui(concave, |ui| {
            gis_field(
                ui,
                "gisHullRatio",
                "Ratio:",
                &mut self.ratio,
                "0.1 - from 0 (tightest) to 1 (convex)",
                "concave=: GEOS's ratio of the longest edge; 1 gives the convex hull",
            );
            check(ui, "gisHullHoles", "Allow holes", &mut self.holes).on_hover_text(
                "holes: where the points leave a gap inside, the boundary has a hole",
            );
        });
        gis_field(
            ui,
            "gisHullLayer",
            "Result layer:",
            &mut self.layer,
            "default gis/hull",
            "TO LAYER: the layer the boundary is drawn on",
        );
    }
}
