//! Terrain > Analysis > Slope and Aspect (geo/slope_analysis_dialog.hpp; docs/terrain.md, "Slope
//! and aspect"): the window onto the RASTER SLOPE and RASTER ASPECT verbs (katana_app::geo's slope
//! verbs).
//!
//! The dialog computes nothing itself. It writes the line its fields describe - shown, as it will
//! run, in slopeCommand - and Run hands it to the window's one executor as if it had been typed
//! (terrain_dialog_support): the analysis is the verb's background job, the class areas one undo
//! step, and its reply - each class's area - comes back into slopeReply. The line is made by a pure
//! function of the fields, tested without a window.
//!
//! ```text
//! slopeAnalysisDialog:
//!   slopeSource      the surface or reference raster
//!   slopeKind        Slope or Aspect
//!   slopeUnit        percent or degree (slope)
//!   slopeClasses     the breaks between classes, "5,10,25"; blank: none
//!   slopeAreasLayer  the layer the class areas go under, <layer>/<class>
//!   slopeMinArea     regions smaller than this many square units are merged into their
//!                    neighbours; blank: none
//!   slopeName        the reference raster's name; blank: the source's
//!   slopeClip        keep the analysis inside closed shapes of the drawing
//!   slopeScope       which shapes (ScopeFilterWidget: slopeScopeDrawing, slopeFilterLayer ...),
//!                    on with slopeClip
//!   slopeCommand     the exact line Run will run (read-only)
//!   slopePreview     run it with PREVIEW: what would be read, nothing made
//!   slopeRun         run it
//!   slopeReply       what it said
//! ```

use egui::Ui;
use katana_core::Result;

use super::terrain_dialog_parts::{
    Framed, WordChoice, command_field, field, host_views, invalid, number_of, row, run_buttons,
    show_framed, without_blanks,
};
use super::terrain_dialog_support::{
    SourcePicker, TerrainRun, TerrainSourceKinds, fill_sources, line_word, number_list,
    terrain_reply_field, terrain_sources,
};
use crate::customisation::{ScopeChoice, ScopeFilterWidget};
use crate::drive::check;
use crate::host::Host;
use crate::shell::DialogFrame;

/// The fields of the dialog, as the line is written from them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlopeForm {
    /// SURFACE <name> or RASTER <id>, as the source picker's data says it.
    pub source: String,
    pub aspect: bool,
    pub unit: String,
    pub classes: String,
    pub areas: String,
    pub min_area: String,
    pub name: String,
    pub clip: bool,
    /// The scope and filter words (ScopeFilterWidget::verb_words), or - in `scope_error` - why the
    /// controls say none.
    pub scope: String,
    pub scope_error: String,
}

impl Default for SlopeForm {
    fn default() -> Self {
        Self {
            source: String::new(),
            aspect: false,
            unit: String::from("percent"),
            classes: String::new(),
            areas: String::new(),
            min_area: String::new(),
            name: String::new(),
            clip: false,
            scope: String::new(),
            scope_error: String::new(),
        }
    }
}

/// ```text
/// RASTER SLOPE <source> unit=<u> [classes=<b1>,...] [areas=<layer>] [min_area=<m2>]
///              [NAME <n>] [<scope>]
/// RASTER ASPECT <source> [NAME <n>] [<scope>]
/// ```
///
/// areas= and min_area= are written only with classes; the scope only with clip. InvalidArgument
/// naming the field: no source, classes that are not numbers, a least area that is not a positive
/// number, a name or layer with a double quote, a scope the controls cannot say.
pub fn slope_line(form: &SlopeForm) -> Result<String> {
    let source = form.source.trim();
    if source.is_empty() {
        return Err(invalid(
            "Source: there is no surface or raster to analyse; build a surface or import a DEM \
             first",
        ));
    }
    let mut line = format!(
        "{}{source}",
        if form.aspect {
            "RASTER ASPECT "
        } else {
            "RASTER SLOPE "
        }
    );
    if !form.aspect {
        line.push_str(&format!(" unit={}", form.unit));
        let classes = without_blanks(&form.classes);
        if !classes.is_empty() {
            if !number_list(&classes, 0, false) {
                return Err(invalid(
                    "Classes: the slopes between classes, separated by commas, as 5,10,25",
                ));
            }
            line.push_str(&format!(" classes={classes}"));
            let layer = form.areas.trim();
            if !layer.is_empty() {
                let Some(word) = line_word(layer) else {
                    return Err(invalid("Areas on: a layer name cannot hold a double quote"));
                };
                line.push_str(&format!(" areas={word}"));
            }
            let least = form.min_area.trim();
            if !least.is_empty() {
                if !number_list(least, 1, false) || !(number_of(least) > 0.0) {
                    return Err(invalid("Least area: a positive area, as 25"));
                }
                line.push_str(&format!(" min_area={least}"));
            }
        }
    }
    let name = form.name.trim();
    if !name.is_empty() {
        let Some(word) = line_word(name) else {
            return Err(invalid("Name: a name cannot hold a double quote"));
        };
        line.push_str(&format!(" NAME {word}"));
    }
    if form.clip {
        if !form.scope_error.is_empty() {
            return Err(invalid(format!("Keep inside: {}", form.scope_error)));
        }
        line.push_str(&format!(" {}", form.scope.trim()));
    }
    Ok(line)
}

/// Terrain > Analysis > Slope and Aspect (see the module comment).
pub struct SlopeAnalysisDialog {
    frame: DialogFrame,
    visible: bool,
    source: SourcePicker,
    kind: WordChoice,
    unit: WordChoice,
    classes: String,
    areas: String,
    min_area: String,
    name: String,
    clip: bool,
    scope: ScopeFilterWidget,
    runner: TerrainRun,
}

impl SlopeAnalysisDialog {
    /// The dialog over the window's surfaces, reference rasters, drawing and views.
    pub fn new(host: &mut dyn Host) -> Self {
        let mut scope = ScopeFilterWidget::new("slope");
        scope.views = host_views(host);
        scope.set_choice(ScopeChoice::Selection);
        let mut dialog = Self {
            frame: DialogFrame::new("slopeAnalysisDialog", "Slope and Aspect"),
            visible: false,
            source: SourcePicker::default(),
            kind: WordChoice::new(&[("Slope", "slope"), ("Aspect", "aspect")]),
            unit: WordChoice::new(&[("Percent", "percent"), ("Degrees", "degree")]),
            classes: String::new(),
            areas: String::from("terrain/slope"),
            min_area: String::new(),
            name: String::new(),
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

    fn aspect(&self) -> bool {
        self.kind.word() == "aspect"
    }

    pub fn form(&self) -> SlopeForm {
        let (scope, scope_error) = match self.scope.verb_words() {
            Ok(words) => (words, String::new()),
            Err(error) => (String::new(), error.message),
        };
        SlopeForm {
            source: String::from(self.source.words().unwrap_or("")),
            aspect: self.aspect(),
            unit: String::from(self.unit.word()),
            classes: self.classes.clone(),
            areas: self.areas.clone(),
            min_area: self.min_area.clone(),
            name: self.name.clone(),
            clip: self.clip,
            scope,
            scope_error,
        }
    }

    pub fn command(&self) -> Result<String> {
        slope_line(&self.form())
    }

    pub fn scope_controls(&mut self) -> &mut ScopeFilterWidget {
        &mut self.scope
    }

    /// What it said last (slopeReply).
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

impl Framed for SlopeAnalysisDialog {
    fn frame_mut(&mut self) -> &mut DialogFrame {
        &mut self.frame
    }

    fn body(&mut self, ui: &mut Ui, _host: &mut dyn Host) {
        row(ui, "Source:", |ui| {
            self.source.ui(ui, "slopeSource");
        });
        row(ui, "Analysis:", |ui| {
            self.kind.ui(ui, "slopeKind");
        });
        let slope = !self.aspect();
        let classed = slope && !self.classes.trim().is_empty();
        row(ui, "Unit:", |ui| {
            ui.add_enabled_ui(slope, |ui| {
                self.unit.ui(ui, "slopeUnit");
            });
        });
        row(ui, "Classes:", |ui| {
            ui.add_enabled_ui(slope, |ui| {
                field(
                    ui,
                    "slopeClasses",
                    &mut self.classes,
                    "5,10,25 - blank: the slope alone",
                )
                .on_hover_text(
                    "The slopes between classes: 5,10,25 makes 0-5, 5-10, 10-25 and 25+.",
                );
            });
        });
        row(ui, "Areas on:", |ui| {
            ui.add_enabled_ui(classed, |ui| {
                field(ui, "slopeAreasLayer", &mut self.areas, "")
                    .on_hover_text("Each class's areas go to <layer>/<class>.");
            });
        });
        row(ui, "Least area:", |ui| {
            ui.add_enabled_ui(classed, |ui| {
                field(
                    ui,
                    "slopeMinArea",
                    &mut self.min_area,
                    "blank: keep every region",
                )
                .on_hover_text(
                    "Regions smaller than this are merged into their largest neighbour.",
                );
            });
        });
        row(ui, "Name:", |ui| {
            field(ui, "slopeName", &mut self.name, "the source's");
        });
        check(
            ui,
            "slopeClip",
            "Keep inside closed shapes of the drawing",
            &mut self.clip,
        );
        let clip = self.clip;
        ui.add_enabled_ui(clip, |ui| {
            self.scope.ui(ui);
        });
        command_field(ui, "slopeCommand", &self.command());
    }

    fn footer(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        let _ = self.runner.poll();
        let ready = self.command().is_ok();
        let (preview, run) = run_buttons(
            ui,
            Some(("slopePreview", ready)),
            ("slopeRun", "Analyse", ready),
        );
        terrain_reply_field(ui, "slopeReply", self.runner.reply_mut());
        if preview {
            self.run(host, true);
        }
        if run {
            self.run(host, false);
        }
    }
}
