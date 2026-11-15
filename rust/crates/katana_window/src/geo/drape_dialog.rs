//! Terrain > Analysis > Drape and Sample Heights (geo/drape_dialog.hpp; docs/terrain.md, "Sampling
//! and drape"): the window onto the DRAPE and RASTER SAMPLE verbs.
//!
//! The dialog reads no height itself. Each tab writes the line its fields describe - shown, as it
//! will run, in its Command field - and its button hands it to the window's one executor as if it
//! had been typed (terrain_dialog_support). A sample point is typed, or picked in a plan view
//! (point_pick): what the pick hands back is only a point, written into the line as AT x,y.
//!
//! ```text
//! drapeDialog:
//!   drapeTabs       Drape | Sample
//!   drapeSource     the ground: a surface or a reference raster
//!   drapeMethod     how a raster is read between cell centres (off for a surface)
//!   drapeScope      what is draped (ScopeFilterWidget: drapeScopeDrawing ...)
//!   drapeCommand, drapePreview, drapeRun, drapeReply
//!   sampleSource, sampleMethod
//!   samplePoint     a point to add, x,y; sampleAdd, samplePick
//!   samplePoints    the points to sample; sampleRemove
//!   sampleCommand, sampleRun, sampleReply
//! ```

use egui::Ui;
use katana_core::Result;

use super::point_pick::PointPick;
use super::terrain_dialog_parts::{
    Framed, WordChoice, command_field, field, host_views, invalid, one_column, remove_current, row,
    run_buttons, show_framed, without_blanks,
};
use super::terrain_dialog_support::{
    SourcePicker, TerrainRun, TerrainSourceKinds, fill_sources, number_list, terrain_reply_field,
    terrain_sources,
};
use crate::customisation::{ScopeChoice, ScopeFilterWidget};
use crate::drive::{button, rows, tabs};
use crate::format::qt_fixed;
use crate::host::Host;
use crate::shell::DialogFrame;
use crate::survey::survey_dialogs::offstage;

/// What a pick's button says while its click is awaited, and its tip.
pub(crate) const PICK_TIP: &str = "Click the point in a plan view; Esc or a right click cancels.";

/// The Drape tab's fields, as its line is written from them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DrapeForm {
    /// SURFACE <name> or RASTER <id>, as the source picker's data says it.
    pub source: String,
    pub method: String,
    pub scope: String,
    pub scope_error: String,
}

impl Default for DrapeForm {
    fn default() -> Self {
        Self {
            source: String::new(),
            method: String::from("bilinear"),
            scope: String::new(),
            scope_error: String::new(),
        }
    }
}

/// The Sample tab's fields.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SampleForm {
    pub source: String,
    pub method: String,
    /// "x,y" each.
    pub points: Vec<String>,
}

impl Default for SampleForm {
    fn default() -> Self {
        Self {
            source: String::new(),
            method: String::from("bilinear"),
            points: Vec::new(),
        }
    }
}

fn methods() -> WordChoice {
    WordChoice::new(&[
        ("Bilinear", "bilinear"),
        ("Nearest cell", "nearest"),
        ("Cubic", "cubic"),
        ("Cubic spline", "cubicspline"),
    ])
}

/// Starts "SURFACE ", any letter case, blanks before it not counted.
fn is_surface(source: &str) -> bool {
    let source = source.trim();
    source
        .get(..8)
        .is_some_and(|start| start.eq_ignore_ascii_case("SURFACE "))
}

/// A picked point as a line takes it: "x,y" at three places.
pub fn point_words(x: f64, y: f64) -> String {
    format!("{},{}", qt_fixed(x, 3), qt_fixed(y, 3))
}

/// ```text
/// DRAPE <source> <scope> [method=<m>]
/// ```
///
/// method= only for a raster. InvalidArgument naming the field: no source, a scope the controls
/// cannot say.
pub fn drape_line(form: &DrapeForm) -> Result<String> {
    let source = form.source.trim();
    if source.is_empty() {
        return Err(invalid(
            "Source: there is no surface or raster to drape on; build a surface or import a DEM \
             first",
        ));
    }
    if !form.scope_error.is_empty() {
        return Err(invalid(format!("Drape: {}", form.scope_error)));
    }
    let mut line = format!("DRAPE {source}");
    let scope = form.scope.trim();
    if !scope.is_empty() {
        line.push_str(&format!(" {scope}"));
    }
    if !is_surface(&form.source) {
        line.push_str(&format!(" method={}", form.method));
    }
    Ok(line)
}

/// ```text
/// RASTER SAMPLE <source> AT x,y [AT x,y]... [method=<m>]
/// ```
///
/// InvalidArgument naming the field: no source, no point, a point that is not two numbers.
pub fn sample_line(form: &SampleForm) -> Result<String> {
    let source = form.source.trim();
    if source.is_empty() {
        return Err(invalid(
            "Source: there is no surface or raster to sample; build a surface or import a DEM \
             first",
        ));
    }
    if form.points.is_empty() {
        return Err(invalid(
            "Points: add a point to sample, typed or picked",
        ));
    }
    let mut line = format!("RASTER SAMPLE {source}");
    for point in &form.points {
        let words = without_blanks(point);
        if !number_list(&words, 2, false) {
            return Err(invalid(format!("Points: {point} is not x,y")));
        }
        line.push_str(&format!(" AT {words}"));
    }
    if !is_surface(&form.source) {
        line.push_str(&format!(" method={}", form.method));
    }
    Ok(line)
}

/// Terrain > Analysis > Drape and Sample Heights (see the module comment).
pub struct DrapeDialog {
    frame: DialogFrame,
    visible: bool,
    tab: usize,
    drape_source: SourcePicker,
    drape_method: WordChoice,
    scope: ScopeFilterWidget,
    drape_runner: TerrainRun,
    sample_source: SourcePicker,
    sample_method: WordChoice,
    sample_point: String,
    sample_points: Vec<String>,
    sample_current: Option<usize>,
    sample_runner: TerrainRun,
    pick: PointPick,
}

impl DrapeDialog {
    /// The dialog over the window's surfaces, reference rasters, drawing and views.
    pub fn new(host: &mut dyn Host) -> Self {
        let mut scope = ScopeFilterWidget::new("drape");
        scope.views = host_views(host);
        scope.set_choice(ScopeChoice::Selection);
        let mut dialog = Self {
            frame: DialogFrame::new("drapeDialog", "Drape and Sample Heights"),
            visible: false,
            tab: 0,
            drape_source: SourcePicker::default(),
            drape_method: methods(),
            scope,
            drape_runner: TerrainRun::new(),
            sample_source: SourcePicker::default(),
            sample_method: methods(),
            sample_point: String::new(),
            sample_points: Vec::new(),
            sample_current: None,
            sample_runner: TerrainRun::new(),
            pick: PointPick::new(),
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

    pub fn reload(&mut self, host: &dyn Host) {
        let kinds = TerrainSourceKinds::SURFACES | TerrainSourceKinds::RASTERS;
        let surfaces = Some(host.document().surfaces());
        let reference = Some(host.reference());
        fill_sources(
            &mut self.drape_source,
            terrain_sources(surfaces, reference, kinds),
        );
        fill_sources(
            &mut self.sample_source,
            terrain_sources(surfaces, reference, kinds),
        );
        self.scope.reload(host.document().model());
    }

    pub fn drape_form(&self) -> DrapeForm {
        let (scope, scope_error) = match self.scope.verb_words() {
            Ok(words) => (words, String::new()),
            Err(error) => (String::new(), error.message),
        };
        DrapeForm {
            source: String::from(self.drape_source.words().unwrap_or("")),
            method: String::from(self.drape_method.word()),
            scope,
            scope_error,
        }
    }

    pub fn sample_form(&self) -> SampleForm {
        SampleForm {
            source: String::from(self.sample_source.words().unwrap_or("")),
            method: String::from(self.sample_method.word()),
            points: self.sample_points.clone(),
        }
    }

    pub fn scope_controls(&mut self) -> &mut ScopeFilterWidget {
        &mut self.scope
    }

    /// The points the Sample tab lists.
    pub fn sample_points(&self) -> &[String] {
        &self.sample_points
    }

    pub fn drape_reply(&self) -> &str {
        self.drape_runner.reply()
    }

    pub fn sample_reply(&self) -> &str {
        self.sample_runner.reply()
    }

    pub fn run_drape(&mut self, host: &mut dyn Host, preview: bool) {
        match drape_line(&self.drape_form()) {
            Err(error) => *self.drape_runner.reply_mut() = error.message,
            Ok(line) => {
                let line = if preview {
                    format!("{line} PREVIEW")
                } else {
                    line
                };
                self.drape_runner.run(host, &line);
            }
        }
    }

    pub fn run_sample(&mut self, host: &mut dyn Host) {
        match sample_line(&self.sample_form()) {
            Err(error) => *self.sample_runner.reply_mut() = error.message,
            Ok(line) => {
                self.sample_runner.run(host, &line);
            }
        }
    }

    /// Adds "x,y" to the points to sample; false, and nothing added, when it is not two numbers.
    pub fn add_sample_point(&mut self, text: &str) -> bool {
        let words = without_blanks(text);
        if words.is_empty() || !number_list(&words, 2, false) {
            return false;
        }
        self.sample_points.push(words);
        true
    }

    /// What a pick answered: the point added, or the reply saying none was picked.
    fn picked(&mut self, answer: Option<katana_geometry::Point2>) {
        match answer {
            None => *self.sample_runner.reply_mut() = String::from("No point was picked."),
            Some(point) => {
                let _ = self.add_sample_point(&point_words(point.x, point.y));
                self.sample_runner.reply_mut().clear();
            }
        }
    }

    /// One frame of the dialog, when it is shown.
    pub fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        if !self.visible {
            return;
        }
        if let Some(answer) = self.pick.poll(host) {
            self.picked(answer);
        }
        if show_framed(self, ui, host) {
            self.hide();
        }
    }

    fn drape_page(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        let _ = self.drape_runner.poll();
        row(ui, "Ground:", |ui| {
            self.drape_source.ui(ui, "drapeSource");
        });
        let surface = is_surface(self.drape_source.words().unwrap_or(""));
        row(ui, "Between cells:", |ui| {
            ui.add_enabled_ui(!surface, |ui| {
                self.drape_method.ui(ui, "drapeMethod").on_hover_text(
                    "How a raster is read between cell centres. A surface is read on its own \
                     triangles.",
                );
            });
        });
        self.scope.ui(ui);
        let made = drape_line(&self.drape_form());
        command_field(ui, "drapeCommand", &made);
        let ready = made.is_ok();
        let (preview, run) = run_buttons(
            ui,
            Some(("drapePreview", ready)),
            ("drapeRun", "Drape", ready),
        );
        terrain_reply_field(ui, "drapeReply", self.drape_runner.reply_mut());
        if preview {
            self.run_drape(host, true);
        }
        if run {
            self.run_drape(host, false);
        }
    }

    fn sample_page(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        let _ = self.sample_runner.poll();
        row(ui, "Ground:", |ui| {
            self.sample_source.ui(ui, "sampleSource");
        });
        let surface = is_surface(self.sample_source.words().unwrap_or(""));
        row(ui, "Between cells:", |ui| {
            ui.add_enabled_ui(!surface, |ui| {
                self.sample_method.ui(ui, "sampleMethod");
            });
        });
        let can_pick = PointPick::offered(host);
        let (add, pick) = row(ui, "Point:", |ui| {
            let typed = field(ui, "samplePoint", &mut self.sample_point, "x,y");
            let entered = typed.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            let add = button(ui, "sampleAdd", "Add").clicked() || entered;
            let pick = ui
                .add_enabled_ui(can_pick, |ui| {
                    button(ui, "samplePick", "Pick")
                        .on_hover_text(PICK_TIP)
                        .clicked()
                })
                .inner;
            (add, pick)
        });
        let listed = one_column(&self.sample_points);
        egui::ScrollArea::vertical()
            .id_salt("samplePointsScroll")
            .max_height(110.0)
            .show(ui, |ui| {
                rows(ui, "samplePoints", &listed, &mut self.sample_current);
            });
        let has_points = !self.sample_points.is_empty();
        let remove = ui
            .add_enabled_ui(has_points, |ui| {
                button(ui, "sampleRemove", "Remove").clicked()
            })
            .inner;
        let made = sample_line(&self.sample_form());
        command_field(ui, "sampleCommand", &made);
        let (_, run) = run_buttons(ui, None, ("sampleRun", "Sample", made.is_ok()));
        terrain_reply_field(ui, "sampleReply", self.sample_runner.reply_mut());
        if add {
            let text = self.sample_point.clone();
            if self.add_sample_point(&text) {
                self.sample_point.clear();
            } else {
                *self.sample_runner.reply_mut() = format!("Point: {text} is not x,y");
            }
        }
        if pick {
            *self.sample_runner.reply_mut() = String::from(PICK_TIP);
            if let Some(answer) = self.pick.pick(host) {
                self.picked(answer);
            }
        }
        if remove {
            remove_current(&mut self.sample_points, &mut self.sample_current);
        }
        if run {
            self.run_sample(host);
        }
    }
}

impl Framed for DrapeDialog {
    fn frame_mut(&mut self) -> &mut DialogFrame {
        &mut self.frame
    }

    // Each tab holds its own buttons and reply, as the C++ pages do, so the dialog has no footer
    // of its own and everything scrolls together.
    fn body(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        tabs(ui, "drapeTabs", &["Drape", "Sample"], &mut self.tab);
        // The page not in front is still there, as a QTabWidget's is: what it holds can be filled,
        // pressed and read.
        if self.tab == 0 {
            self.drape_page(ui, host);
            offstage(ui, "drapeSamplePage", |ui| self.sample_page(ui, host));
        } else {
            offstage(ui, "drapeDrapePage", |ui| self.drape_page(ui, host));
            self.sample_page(ui, host);
        }
    }

    fn footer(&mut self, _ui: &mut Ui, _host: &mut dyn Host) {}
}
