//! GIS > Check - GDAL > Gaps and Overlaps...: the window onto GIS COVERAGE CHECK and CLEAN
//! (geo/coverage_dialog.hpp; docs/geoprocessing.md, "V4"), built on the GIS dialog frame
//! (gis_tool_dialog).
//!
//! Its fields, by object name (the frame's are in gis_tool_dialog; the scope's are
//! gisCoverageScope, gisCoverageScopeDrawing, ...):
//!
//! ```text
//! gisCoverageMode      check (report the problems) or clean (move the boundaries so the areas
//!                      meet)
//! gisCoverageGap       the widest gap to find, or to close, metres; blank looks for none
//! gisCoverageSnap      clean: snap vertices this close together, metres
//! gisCoverageMerge     clean: which neighbour an overlap goes to
//! gisCoverageReplace   clean: change the areas in place - required, since surveyed boundaries
//!                      must not move silently
//! gisCoverageMarkers   check: a layer to draw each bad edge on, replacing the last check's
//! gisCoverageProblems  the problems found, a row each
//! ```

use egui::Ui;
use katana_core::error::make_error;
use katana_core::{ErrorCode, Result};

use super::buffer_dialog::Words;
use super::geometry_check_dialog::gis_problems_summary;
use super::gis_tool_dialog::{
    GisDialogContext, GisScopeWords, GisTool, GisToolFrame, gis_field, gis_number_option,
    gis_scope, gis_summary, gis_word,
};
use crate::drive::check;
use crate::host::Host;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GisCoverageForm {
    pub scope: GisScopeWords,
    pub clean: bool,
    pub gap: String,
    pub snap: String,
    /// Empty: GDAL's default.
    pub merge: String,
    pub replace: bool,
    pub markers: String,
}

/// GIS COVERAGE CHECK <scope> [gap=<m>] [markers=<layer>], or GIS COVERAGE CLEAN <scope>
/// [gap=<m>] [snap=<m>] [merge=<strategy>] REPLACE, exactly as it would be typed.
/// InvalidArgument naming the field for a scope the controls cannot say, a length that does not
/// read, and a clean without Replace ticked - which the verb refuses too.
pub fn gis_coverage_line(form: &GisCoverageForm) -> Result<String> {
    let scope = gis_scope(&form.scope)?;
    let mut line = format!(
        "GIS COVERAGE {}{scope}",
        if form.clean { "CLEAN " } else { "CHECK " }
    );
    line.push_str(&gis_number_option("gap", &form.gap, "The gap width")?);
    if form.clean {
        line.push_str(&gis_number_option(
            "snap",
            &form.snap,
            "The snapping distance",
        )?);
        let merge = form.merge.trim();
        if !merge.is_empty() {
            line.push_str(" merge=");
            line.push_str(merge);
        }
        if !form.replace {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "a clean moves boundaries: tick Replace to change the areas in place, or Check \
                 first",
                String::new(),
            ));
        }
        line.push_str(" REPLACE");
        return Ok(line);
    }
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

/// GIS > Check - GDAL > Gaps and Overlaps... (see the module comment).
pub struct GisCoverageDialog {
    frame: GisToolFrame,
    mode: Words,
    gap: String,
    snap: String,
    merge: Words,
    replace: bool,
    markers: String,
}

impl GisCoverageDialog {
    pub fn new(context: GisDialogContext, host: &dyn Host) -> Self {
        let mut frame = GisToolFrame::new("gisCoverage", "Gaps and Overlaps", context, host, true);
        frame.add_problems_table();
        Self {
            frame,
            mode: Words::new(&["check", "clean"]),
            gap: String::new(),
            snap: String::new(),
            merge: Words::new(&["", "longest-border", "max-area", "min-area", "min-index"]),
            replace: false,
            markers: String::new(),
        }
    }

    /// The window's Gaps and Overlaps dialog, as the menu makes it.
    pub fn make(context: GisDialogContext, host: &dyn Host) -> Box<dyn GisTool> {
        Box::new(Self::new(context, host))
    }

    pub fn form(&self) -> GisCoverageForm {
        GisCoverageForm {
            scope: self.frame.scope_words(),
            clean: self.mode.text() == "clean",
            gap: self.gap.clone(),
            snap: self.snap.clone(),
            merge: self.merge.text(),
            replace: self.replace,
            markers: self.markers.clone(),
        }
    }
}

impl GisTool for GisCoverageDialog {
    fn frame(&self) -> &GisToolFrame {
        &self.frame
    }

    fn frame_mut(&mut self) -> &mut GisToolFrame {
        &mut self.frame
    }

    fn command(&self) -> Result<String> {
        gis_coverage_line(&self.form())
    }

    fn summary(&self, reply: &str) -> String {
        if reply.contains(" preview=yes")
            || reply.contains(" ran=no")
            || reply.contains("coverage mode=clean")
        {
            return gis_summary(reply);
        }
        gis_problems_summary(reply, "No problem: the areas meet edge to edge.")
    }

    fn fields_ui(&mut self, ui: &mut Ui, _host: &mut dyn Host) {
        self.mode.ui(
            ui,
            "gisCoverageMode",
            "Mode:",
            "check reports overlaps, gaps and edges that do not match; clean moves the \
             boundaries so the areas meet - enclosed gaps only",
        );
        // The mode chooses which fields apply.
        let clean = self.mode.text() == "clean";
        gis_field(
            ui,
            "gisCoverageGap",
            "Gap width (m):",
            &mut self.gap,
            "none - no gaps looked for",
            "gap=: find (check) or close (clean) the enclosed gaps narrower than this",
        );
        ui.add_enabled_ui(clean, |ui| {
            gis_field(
                ui,
                "gisCoverageSnap",
                "Snap within (m):",
                &mut self.snap,
                "none",
                "snap=: clean only - vertices this close together become one",
            );
            self.merge.ui(
                ui,
                "gisCoverageMerge",
                "Overlaps go to:",
                "merge=: clean only - which neighbour an overlap goes to; blank is GDAL's \
                 default (the longest shared border)",
            );
            check(
                ui,
                "gisCoverageReplace",
                "Change the areas in place",
                &mut self.replace,
            )
            .on_hover_text(
                "REPLACE: a clean moves boundaries. Surveyed legal boundaries must not be \
                 adjusted silently - Check first; one UNDO puts them back",
            );
        });
        ui.add_enabled_ui(!clean, |ui| {
            gis_field(
                ui,
                "gisCoverageMarkers",
                "Markers on layer:",
                &mut self.markers,
                "none - report only",
                "markers=<layer>: check only - each bad edge drawn on this layer, replacing the \
                 last check's",
            );
        });
    }
}
