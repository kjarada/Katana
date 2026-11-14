//! Terrain > Analysis > Contours (geo/contours_dialog.hpp; docs/terrain.md, "Contours"): the window
//! onto the CONTOUR verb (katana_app::geo's contour verbs).
//!
//! The dialog traces nothing itself. It writes the CONTOUR line its fields describe - shown, as it
//! will run, in contourCommand - and Run hands it to the window's one executor as if it had been
//! typed (terrain_dialog_support): the contours are the verb's background job, one undo step, and
//! its reply comes back into contourReply. The line is made by a pure function of the fields,
//! tested without a window.
//!
//! ```text
//! contoursDialog:
//!   contourSource      the surface or reference raster (a surface is traced exactly on its
//!                      triangles, a raster by GDAL)
//!   contourInterval    the height between contours
//!   contourMajorEvery  every how many is a major contour (0: none)
//!   contourBase        the level the interval counts from
//!   contourLayer       the parent layer: <layer>/major and <layer>/minor
//!   contourSmooth      a raster's gaussian smoothing, none, 3 or 5 cells (off for a surface)
//!   contourClip        keep the contours inside closed shapes of the drawing
//!   contourScope       which shapes (ScopeFilterWidget: contourScopeDrawing, contourFilterLayer
//!                      ...), on with contourClip
//!   contourCommand     the exact line Run will run (read-only)
//!   contourPreview     run it with PREVIEW: what would be read, nothing drawn
//!   contourRun         run it
//!   contourReply       what it said
//! ```

use egui::Ui;
use katana_core::Result;

use super::terrain_dialog_parts::{
    Framed, WordChoice, command_field, field, host_views, invalid, number_of, row, run_buttons,
    show_framed,
};
use super::terrain_dialog_support::{
    SourcePicker, TerrainRun, TerrainSourceKinds, fill_sources, line_word, number_list,
    terrain_reply_field, terrain_sources,
};
use crate::customisation::{ScopeChoice, ScopeFilterWidget};
use crate::drive::{check, whole};
use crate::host::Host;
use crate::shell::DialogFrame;

/// The fields of the dialog, as the line is written from them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContourForm {
    /// SURFACE <name> or RASTER <id>, as the source picker's data says it.
    pub source: String,
    pub interval: String,
    pub major_every: i32,
    pub base: String,
    pub layer: String,
    pub smooth: i32,
    pub clip: bool,
    /// The scope and filter words (ScopeFilterWidget::verb_words), or - in `scope_error` - why the
    /// controls say none.
    pub scope: String,
    pub scope_error: String,
}

impl Default for ContourForm {
    fn default() -> Self {
        Self {
            source: String::new(),
            interval: String::new(),
            major_every: 5,
            base: String::new(),
            layer: String::new(),
            smooth: 0,
            clip: false,
            scope: String::new(),
            scope_error: String::new(),
        }
    }
}

fn is_surface(source: &str) -> bool {
    source.starts_with("SURFACE ")
}

/// ```text
/// CONTOUR <source> interval=<m> major=<n> [base=<b>] [layer=<l>] [smooth=3|5] [<scope>]
/// ```
///
/// base is left out when blank, layer when blank, smooth for a surface and when none; the scope is
/// written only with clip. InvalidArgument naming the field: no source, an interval that is not a
/// positive number, a base that is not a number, a layer with a double quote, a scope the controls
/// cannot say.
pub fn contour_line(form: &ContourForm) -> Result<String> {
    let source = form.source.trim();
    if source.is_empty() {
        return Err(invalid(
            "Source: there is no surface or raster to contour; build a surface or import a DEM \
             first",
        ));
    }
    let interval = form.interval.trim();
    if !number_list(interval, 1, false) || !(number_of(interval) > 0.0) {
        return Err(invalid("Interval: a positive height, as 0.5"));
    }
    let mut line = format!(
        "CONTOUR {source} interval={interval} major={}",
        form.major_every
    );
    let base = form.base.trim();
    if !base.is_empty() {
        if !number_list(base, 1, false) {
            return Err(invalid("Base: a height, as 100"));
        }
        line.push_str(&format!(" base={base}"));
    }
    let layer = form.layer.trim();
    if !layer.is_empty() {
        let Some(word) = line_word(layer) else {
            return Err(invalid("Layer: a layer name cannot hold a double quote"));
        };
        line.push_str(&format!(" layer={word}"));
    }
    if form.smooth != 0 && !is_surface(&form.source) {
        line.push_str(&format!(" smooth={}", form.smooth));
    }
    if form.clip {
        if !form.scope_error.is_empty() {
            return Err(invalid(format!("Keep inside: {}", form.scope_error)));
        }
        line.push_str(&format!(" {}", form.scope.trim()));
    }
    Ok(line)
}

/// Terrain > Analysis > Contours (see the module comment).
pub struct ContoursDialog {
    frame: DialogFrame,
    visible: bool,
    source: SourcePicker,
    interval: String,
    major_every: i64,
    base: String,
    layer: String,
    smooth: WordChoice,
    clip: bool,
    scope: ScopeFilterWidget,
    runner: TerrainRun,
}

impl ContoursDialog {
    /// The dialog over the window's surfaces, reference rasters, drawing and views.
    pub fn new(host: &mut dyn Host) -> Self {
        let mut scope = ScopeFilterWidget::new("contour");
        scope.views = host_views(host);
        // The closed shapes a person has just picked, at first.
        scope.set_choice(ScopeChoice::Selection);
        let mut dialog = Self {
            frame: DialogFrame::new("contoursDialog", "Contours"),
            visible: false,
            source: SourcePicker::default(),
            interval: String::from("1"),
            major_every: 5,
            base: String::new(),
            layer: String::from("terrain/contours"),
            smooth: WordChoice::new(&[("None", "0"), ("3 cells", "3"), ("5 cells", "5")]),
            clip: false,
            scope,
            runner: TerrainRun::new(),
        };
        dialog.reload(&*host);
        dialog
    }

    /// Shows the dialog, its lists read again (showEvent).
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

    /// Refills the sources and the scope's layers and views, keeping what is chosen: done when
    /// shown.
    pub fn reload(&mut self, host: &dyn Host) {
        fill_sources(
            &mut self.source,
            terrain_sources(
                Some(host.document().surfaces()),
                Some(host.reference()),
                TerrainSourceKinds::SURFACES | TerrainSourceKinds::RASTERS,
            ),
        );
        self.scope.reload(host.document().model());
    }

    pub fn form(&self) -> ContourForm {
        let (scope, scope_error) = match self.scope.verb_words() {
            Ok(words) => (words, String::new()),
            Err(error) => (String::new(), error.message),
        };
        ContourForm {
            source: String::from(self.source.words().unwrap_or("")),
            interval: self.interval.clone(),
            major_every: i32::try_from(self.major_every).unwrap_or(0),
            base: self.base.clone(),
            layer: self.layer.clone(),
            smooth: self.smooth.word().parse().unwrap_or(0),
            clip: self.clip,
            scope,
            scope_error,
        }
    }

    pub fn command(&self) -> Result<String> {
        contour_line(&self.form())
    }

    pub fn scope_controls(&mut self) -> &mut ScopeFilterWidget {
        &mut self.scope
    }

    /// What it said last (contourReply).
    pub fn reply(&self) -> &str {
        self.runner.reply()
    }

    /// What Run (`preview` false) and Preview do.
    pub fn run(&mut self, host: &mut dyn Host, preview: bool) {
        match self.command() {
            Err(error) => *self.runner.reply_mut() = error.message,
            Ok(line) => {
                let line = if preview {
                    format!("{line} PREVIEW")
                } else {
                    line
                };
                self.runner.run(host, &line);
            }
        }
    }

    /// One frame of the dialog, when it is shown.
    pub fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        if !self.visible {
            return;
        }
        if show_framed(self, ui, host) {
            self.hide();
        }
    }
}

impl Framed for ContoursDialog {
    fn frame_mut(&mut self) -> &mut DialogFrame {
        &mut self.frame
    }

    fn body(&mut self, ui: &mut Ui, _host: &mut dyn Host) {
        row(ui, "Source:", |ui| {
            self.source.ui(ui, "contourSource").on_hover_text(
                "A surface is contoured exactly on its triangles; a raster by GDAL at its full \
                 resolution.",
            );
        });
        row(ui, "Interval:", |ui| {
            field(ui, "contourInterval", &mut self.interval, "");
        });
        row(ui, "Major every:", |ui| {
            whole(ui, "contourMajorEvery", &mut self.major_every, 0..=1000);
        });
        row(ui, "Counted from:", |ui| {
            field(ui, "contourBase", &mut self.base, "0");
        });
        row(ui, "Layer:", |ui| {
            field(ui, "contourLayer", &mut self.layer, "").on_hover_text(
                "Major contours go to <layer>/major, the others to <layer>/minor.",
            );
        });
        let surface = is_surface(self.source.words().unwrap_or(""));
        row(ui, "Smoothing:", |ui| {
            ui.add_enabled_ui(!surface, |ui| {
                self.smooth.ui(ui, "contourSmooth").on_hover_text(
                    "A raster's gaussian smoothing before it is contoured, for presentation; the \
                     reply says smoothed=yes.",
                );
            });
        });
        check(
            ui,
            "contourClip",
            "Keep inside closed shapes of the drawing",
            &mut self.clip,
        );
        let clip = self.clip;
        ui.add_enabled_ui(clip, |ui| {
            self.scope.ui(ui);
        });
        command_field(ui, "contourCommand", &self.command());
    }

    fn footer(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        let _ = self.runner.poll();
        let ready = self.command().is_ok();
        let (preview, run) = run_buttons(
            ui,
            Some(("contourPreview", ready)),
            ("contourRun", "Draw Contours", ready),
        );
        terrain_reply_field(ui, "contourReply", self.runner.reply_mut());
        if preview {
            self.run(host, true);
        }
        if run {
            self.run(host, false);
        }
    }
}
