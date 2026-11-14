//! Terrain > Analysis > Viewshed and Line of Sight (geo/viewshed_dialog.hpp; docs/terrain.md,
//! "Viewshed and line of sight"): the window onto the RASTER VIEWSHED and LOS verbs.
//!
//! The dialog sees nothing itself. Each tab writes the line its fields describe - shown, as it will
//! run, in its Command field - and its button hands it to the window's one executor as if it had
//! been typed (terrain_dialog_support). An observer or a target is typed, or picked in a plan view
//! (point_pick): the pick hands back only a point, written into the line as OBSERVER x,y.
//!
//! ```text
//! viewshedDialog:
//!   viewshedTabs       Viewshed | Line of Sight
//!   viewshedSource     the ground: a surface or a reference raster
//!   viewshedObserver   an observer to add, x,y; viewshedAdd, viewshedPick
//!   viewshedObservers  the observers; viewshedRemove
//!   viewshedUseScope   the observers are the points a scope takes instead
//!   viewshedScope      which points (ScopeFilterWidget: viewshedScopeDrawing ...)
//!   viewshedHeight, viewshedTarget, viewshedMax, viewshedCurvature, viewshedAreas, viewshedName
//!   viewshedCommand, viewshedPreview, viewshedRun, viewshedReply
//!   losSource, losObserver, losPickObserver, losTarget, losPickTarget, losHeight,
//!   losTargetHeight, losCurvature, losCommand, losRun, losReply
//! ```

use egui::Ui;
use katana_core::Result;
use katana_geometry::Point2;

use super::drape_dialog::{PICK_TIP, point_words};
use super::point_pick::PointPick;
use super::terrain_dialog_parts::{
    Framed, command_field, field, host_views, invalid, number_of, one_column, remove_current, row,
    run_buttons, show_framed, without_blanks,
};
use super::terrain_dialog_support::{
    SourcePicker, TerrainRun, TerrainSourceKinds, fill_sources, line_word, number_list,
    terrain_reply_field, terrain_sources,
};
use crate::customisation::{ScopeChoice, ScopeFilterWidget};
use crate::drive::{button, check, rows, tabs};
use crate::host::Host;
use crate::shell::DialogFrame;
use crate::survey::survey_dialogs::offstage;

/// The Viewshed tab's fields, as its line is written from them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewshedForm {
    /// SURFACE <name> or RASTER <id>, as the source picker's data says it.
    pub source: String,
    /// "x,y" each.
    pub observers: Vec<String>,
    pub use_scope: bool,
    pub scope: String,
    pub scope_error: String,
    pub height: String,
    pub target: String,
    pub max: String,
    pub curvature: String,
    pub areas: String,
    pub name: String,
}

impl Default for ViewshedForm {
    fn default() -> Self {
        Self {
            source: String::new(),
            observers: Vec::new(),
            use_scope: false,
            scope: String::new(),
            scope_error: String::new(),
            height: String::from("1.7"),
            target: String::from("0"),
            max: String::new(),
            curvature: String::new(),
            areas: String::new(),
            name: String::new(),
        }
    }
}

/// The Line of Sight tab's fields.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LosForm {
    pub source: String,
    /// x,y
    pub observer: String,
    /// x,y
    pub target: String,
    pub height: String,
    pub target_height: String,
    pub curvature: String,
}

impl Default for LosForm {
    fn default() -> Self {
        Self {
            source: String::new(),
            observer: String::new(),
            target: String::new(),
            height: String::from("1.7"),
            target_height: String::from("0"),
            curvature: String::new(),
        }
    }
}

/// A point as the line takes it, blanks dropped; none when it is not x,y.
fn point_text(text: &str) -> Option<String> {
    let words = without_blanks(text);
    if words.is_empty() || !number_list(&words, 2, false) {
        return None;
    }
    Some(words)
}

/// A number field: blank is the default, the text when it reads.
fn number_field(label: &str, text: &str, allow_blank: bool) -> Result<String> {
    let value = text.trim();
    if value.is_empty() && allow_blank {
        return Ok(String::new());
    }
    if !number_list(value, 1, false) || number_of(value) < 0.0 {
        return Err(invalid(format!("{label}: a number, 0 or more")));
    }
    Ok(String::from(value))
}

/// blank (GDAL's own coefficient), none, or a coefficient.
fn curvature_field(text: &str) -> Result<String> {
    let value = text.trim();
    if value.is_empty() || value.to_lowercase() == "none" {
        return Ok(value.to_lowercase());
    }
    if !number_list(value, 1, false) || number_of(value) < 0.0 {
        return Err(invalid(
            "Curvature: blank for 0.85714, none, or a coefficient of 0 or more",
        ));
    }
    Ok(String::from(value))
}

const NO_SOURCE: &str =
    "Source: there is no surface or raster to look over; build a surface or import a DEM first";

/// ```text
/// RASTER VIEWSHED <source> (OBSERVER x,y)... | OBSERVERS <scope>
///                 height=<h> target=<t> [max=<m>] [curvature=<k>|none]
///                 [areas=<layer>] [NAME <n>]
/// ```
///
/// InvalidArgument naming the field: no source, no observer, a point, a height or a distance that
/// does not read, a scope the controls cannot say, a layer or name with a double quote.
pub fn viewshed_line(form: &ViewshedForm) -> Result<String> {
    let source = form.source.trim();
    if source.is_empty() {
        return Err(invalid(NO_SOURCE));
    }
    let mut line = format!("RASTER VIEWSHED {source}");
    if form.use_scope {
        if !form.scope_error.is_empty() {
            return Err(invalid(format!("Observers: {}", form.scope_error)));
        }
        line.push_str(&format!(" OBSERVERS {}", form.scope.trim()));
    } else {
        if form.observers.is_empty() {
            return Err(invalid(
                "Observers: add an observer, typed or picked, or take the points of a scope",
            ));
        }
        for observer in &form.observers {
            let Some(point) = point_text(observer) else {
                return Err(invalid(format!("Observers: {observer} is not x,y")));
            };
            line.push_str(&format!(" OBSERVER {point}"));
        }
    }
    let height = number_field("Eye height", &form.height, false)?;
    let target = number_field("Target height", &form.target, false)?;
    line.push_str(&format!(" height={height} target={target}"));
    let reach = number_field("Look as far as", &form.max, true)?;
    if !reach.is_empty() {
        if !(number_of(&reach) > 0.0) {
            return Err(invalid(
                "Look as far as: a distance above 0, or blank for the whole raster",
            ));
        }
        line.push_str(&format!(" max={reach}"));
    }
    let curvature = curvature_field(&form.curvature)?;
    if !curvature.is_empty() {
        line.push_str(&format!(" curvature={curvature}"));
    }
    let areas = form.areas.trim();
    if !areas.is_empty() {
        let Some(word) = line_word(&format!("areas={areas}")) else {
            return Err(invalid("Areas on: a layer name cannot hold a double quote"));
        };
        line.push_str(&format!(" {word}"));
    }
    let name = form.name.trim();
    if !name.is_empty() {
        let Some(word) = line_word(name) else {
            return Err(invalid("Name: a name cannot hold a double quote"));
        };
        line.push_str(&format!(" NAME {word}"));
    }
    Ok(line)
}

/// ```text
/// LOS <source> OBSERVER x,y TARGET x,y height=<h> target=<t> [curvature=<k>|none]
/// ```
pub fn los_line(form: &LosForm) -> Result<String> {
    let source = form.source.trim();
    if source.is_empty() {
        return Err(invalid(NO_SOURCE));
    }
    let Some(observer) = point_text(&form.observer) else {
        return Err(invalid("Observer: x,y, typed or picked"));
    };
    let Some(target) = point_text(&form.target) else {
        return Err(invalid("Target: x,y, typed or picked"));
    };
    let height = number_field("Eye height", &form.height, false)?;
    let target_height = number_field("Target height", &form.target_height, false)?;
    let mut line = format!(
        "LOS {source} OBSERVER {observer} TARGET {target} height={height} target={target_height}"
    );
    let curvature = curvature_field(&form.curvature)?;
    if !curvature.is_empty() {
        line.push_str(&format!(" curvature={curvature}"));
    }
    Ok(line)
}

/// Where a pick's point goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PickInto {
    Observers,
    LosObserver,
    LosTarget,
}

/// Terrain > Analysis > Viewshed and Line of Sight (see the module comment).
pub struct ViewshedDialog {
    frame: DialogFrame,
    visible: bool,
    tab: usize,
    source: SourcePicker,
    observer: String,
    observers: Vec<String>,
    observers_current: Option<usize>,
    use_scope: bool,
    scope: ScopeFilterWidget,
    height: String,
    target: String,
    max: String,
    curvature: String,
    areas: String,
    name: String,
    viewshed_runner: TerrainRun,
    los_source: SourcePicker,
    los_observer: String,
    los_target: String,
    los_height: String,
    los_target_height: String,
    los_curvature: String,
    los_runner: TerrainRun,
    pick: PointPick,
    pick_into: PickInto,
}

impl ViewshedDialog {
    /// The dialog over the window's surfaces, reference rasters, drawing and views.
    pub fn new(host: &mut dyn Host) -> Self {
        let mut scope = ScopeFilterWidget::new("viewshed");
        scope.views = host_views(host);
        scope.set_choice(ScopeChoice::Selection);
        let mut dialog = Self {
            frame: DialogFrame::new("viewshedDialog", "Viewshed and Line of Sight"),
            visible: false,
            tab: 0,
            source: SourcePicker::default(),
            observer: String::new(),
            observers: Vec::new(),
            observers_current: None,
            use_scope: false,
            scope,
            height: String::from("1.7"),
            target: String::from("0"),
            max: String::new(),
            curvature: String::new(),
            areas: String::new(),
            name: String::new(),
            viewshed_runner: TerrainRun::new(),
            los_source: SourcePicker::default(),
            los_observer: String::new(),
            los_target: String::new(),
            los_height: String::from("1.7"),
            los_target_height: String::from("0"),
            los_curvature: String::new(),
            los_runner: TerrainRun::new(),
            pick: PointPick::new(),
            pick_into: PickInto::Observers,
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
            &mut self.source,
            terrain_sources(surfaces, reference, kinds),
        );
        fill_sources(
            &mut self.los_source,
            terrain_sources(surfaces, reference, kinds),
        );
        self.scope.reload(host.document().model());
    }

    pub fn viewshed_form(&self) -> ViewshedForm {
        let (scope, scope_error) = match self.scope.verb_words() {
            Ok(words) => (words, String::new()),
            Err(error) => (String::new(), error.message),
        };
        ViewshedForm {
            source: String::from(self.source.words().unwrap_or("")),
            observers: self.observers.clone(),
            use_scope: self.use_scope,
            scope,
            scope_error,
            height: self.height.clone(),
            target: self.target.clone(),
            max: self.max.clone(),
            curvature: self.curvature.clone(),
            areas: self.areas.clone(),
            name: self.name.clone(),
        }
    }

    pub fn los_form(&self) -> LosForm {
        LosForm {
            source: String::from(self.los_source.words().unwrap_or("")),
            observer: self.los_observer.clone(),
            target: self.los_target.clone(),
            height: self.los_height.clone(),
            target_height: self.los_target_height.clone(),
            curvature: self.los_curvature.clone(),
        }
    }

    pub fn scope_controls(&mut self) -> &mut ScopeFilterWidget {
        &mut self.scope
    }

    /// The observers the Viewshed tab lists.
    pub fn observers(&self) -> &[String] {
        &self.observers
    }

    pub fn viewshed_reply(&self) -> &str {
        self.viewshed_runner.reply()
    }

    pub fn los_reply(&self) -> &str {
        self.los_runner.reply()
    }

    pub fn run_viewshed(&mut self, host: &mut dyn Host, preview: bool) {
        match viewshed_line(&self.viewshed_form()) {
            Err(error) => *self.viewshed_runner.reply_mut() = error.message,
            Ok(line) => {
                let line = if preview {
                    format!("{line} PREVIEW")
                } else {
                    line
                };
                self.viewshed_runner.run(host, &line);
            }
        }
    }

    pub fn run_los(&mut self, host: &mut dyn Host) {
        match los_line(&self.los_form()) {
            Err(error) => *self.los_runner.reply_mut() = error.message,
            Ok(line) => {
                self.los_runner.run(host, &line);
            }
        }
    }

    /// Adds "x,y" to the observers; false, and nothing added, when it is not two numbers.
    pub fn add_observer(&mut self, text: &str) -> bool {
        match point_text(text) {
            Some(point) => {
                self.observers.push(point);
                true
            }
            None => false,
        }
    }

    fn reply_of(&mut self, into: PickInto) -> &mut String {
        match into {
            PickInto::Observers => self.viewshed_runner.reply_mut(),
            PickInto::LosObserver | PickInto::LosTarget => self.los_runner.reply_mut(),
        }
    }

    /// Picks a point in a plan view into the observer list or a Line of Sight field, saying so in
    /// that tab's reply.
    fn pick_into(&mut self, host: &mut dyn Host, into: PickInto) {
        if !PointPick::offered(host) {
            return;
        }
        *self.reply_of(into) = String::from(PICK_TIP);
        self.pick_into = into;
        if let Some(answer) = self.pick.pick(host) {
            self.picked(answer);
        }
    }

    fn picked(&mut self, answer: Option<Point2>) {
        let into = self.pick_into;
        let Some(point) = answer else {
            *self.reply_of(into) = String::from("No point was picked.");
            return;
        };
        let words = point_words(point.x, point.y);
        match into {
            PickInto::Observers => {
                let _ = self.add_observer(&words);
            }
            PickInto::LosObserver => self.los_observer = words,
            PickInto::LosTarget => self.los_target = words,
        }
        self.reply_of(into).clear();
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

    fn viewshed_page(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        let _ = self.viewshed_runner.poll();
        let can_pick = PointPick::offered(host);
        let from_scope = self.use_scope;
        row(ui, "Ground:", |ui| {
            self.source.ui(ui, "viewshedSource");
        });
        let (add, pick) = row(ui, "Observer:", |ui| {
            let add = ui
                .add_enabled_ui(!from_scope, |ui| {
                    let typed = field(ui, "viewshedObserver", &mut self.observer, "x,y");
                    let entered =
                        typed.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
                    button(ui, "viewshedAdd", "Add").clicked() || entered
                })
                .inner;
            let pick = ui
                .add_enabled_ui(!from_scope && can_pick, |ui| {
                    button(ui, "viewshedPick", "Pick")
                        .on_hover_text(PICK_TIP)
                        .clicked()
                })
                .inner;
            (add, pick)
        });
        let listed = one_column(&self.observers);
        ui.add_enabled_ui(!from_scope, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("viewshedObserversScroll")
                .max_height(90.0)
                .show(ui, |ui| {
                    rows(ui, "viewshedObservers", &listed, &mut self.observers_current);
                });
        });
        let removable = !from_scope && !self.observers.is_empty();
        let remove = ui
            .add_enabled_ui(removable, |ui| {
                button(ui, "viewshedRemove", "Remove").clicked()
            })
            .inner;
        check(
            ui,
            "viewshedUseScope",
            "The observers are the points these take instead",
            &mut self.use_scope,
        );
        let use_scope = self.use_scope;
        ui.add_enabled_ui(use_scope, |ui| {
            self.scope.ui(ui);
        });
        row(ui, "Eye height:", |ui| {
            field(ui, "viewshedHeight", &mut self.height, "");
        });
        row(ui, "Target height:", |ui| {
            field(ui, "viewshedTarget", &mut self.target, "")
                .on_hover_text("How high above the ground a cell counts as seen.");
        });
        row(ui, "Look as far as:", |ui| {
            field(ui, "viewshedMax", &mut self.max, "blank: the whole raster");
        });
        row(ui, "Curvature:", |ui| {
            field(
                ui,
                "viewshedCurvature",
                &mut self.curvature,
                "blank: 0.85714 (curvature and refraction); none",
            );
        });
        row(ui, "Draw visible area on:", |ui| {
            field(ui, "viewshedAreas", &mut self.areas, "blank: no areas drawn");
        });
        row(ui, "Name:", |ui| {
            field(ui, "viewshedName", &mut self.name, "the source's");
        });
        let made = viewshed_line(&self.viewshed_form());
        command_field(ui, "viewshedCommand", &made);
        let ready = made.is_ok();
        let (preview, run) = run_buttons(
            ui,
            Some(("viewshedPreview", ready)),
            ("viewshedRun", "Compute", ready),
        );
        terrain_reply_field(ui, "viewshedReply", self.viewshed_runner.reply_mut());
        if add {
            let text = self.observer.clone();
            if self.add_observer(&text) {
                self.observer.clear();
            } else {
                *self.viewshed_runner.reply_mut() = format!("Observer: {text} is not x,y");
            }
        }
        if pick {
            self.pick_into(host, PickInto::Observers);
        }
        if remove {
            remove_current(&mut self.observers, &mut self.observers_current);
        }
        if preview {
            self.run_viewshed(host, true);
        }
        if run {
            self.run_viewshed(host, false);
        }
    }

    fn los_page(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        let _ = self.los_runner.poll();
        let can_pick = PointPick::offered(host);
        row(ui, "Ground:", |ui| {
            self.los_source.ui(ui, "losSource");
        });
        let pick_observer = row(ui, "Observer:", |ui| {
            field(ui, "losObserver", &mut self.los_observer, "x,y");
            ui.add_enabled_ui(can_pick, |ui| {
                button(ui, "losPickObserver", "Pick")
                    .on_hover_text(PICK_TIP)
                    .clicked()
            })
            .inner
        });
        let pick_target = row(ui, "Target:", |ui| {
            field(ui, "losTarget", &mut self.los_target, "x,y");
            ui.add_enabled_ui(can_pick, |ui| {
                button(ui, "losPickTarget", "Pick")
                    .on_hover_text(PICK_TIP)
                    .clicked()
            })
            .inner
        });
        row(ui, "Eye height:", |ui| {
            field(ui, "losHeight", &mut self.los_height, "");
        });
        row(ui, "Target height:", |ui| {
            field(ui, "losTargetHeight", &mut self.los_target_height, "");
        });
        row(ui, "Curvature:", |ui| {
            field(
                ui,
                "losCurvature",
                &mut self.los_curvature,
                "blank: 0.85714; none",
            );
        });
        let made = los_line(&self.los_form());
        command_field(ui, "losCommand", &made);
        let (_, run) = run_buttons(ui, None, ("losRun", "Check", made.is_ok()));
        terrain_reply_field(ui, "losReply", self.los_runner.reply_mut());
        if pick_observer {
            self.pick_into(host, PickInto::LosObserver);
        }
        if pick_target {
            self.pick_into(host, PickInto::LosTarget);
        }
        if run {
            self.run_los(host);
        }
    }
}

impl Framed for ViewshedDialog {
    fn frame_mut(&mut self) -> &mut DialogFrame {
        &mut self.frame
    }

    // Each tab holds its own buttons and reply, as the C++ pages do, so the dialog has no footer
    // of its own and everything scrolls together.
    fn body(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        tabs(
            ui,
            "viewshedTabs",
            &["Viewshed", "Line of Sight"],
            &mut self.tab,
        );
        // The page not in front is still there, as a QTabWidget's is: what it holds can be filled,
        // pressed and read.
        if self.tab == 0 {
            self.viewshed_page(ui, host);
            offstage(ui, "viewshedLosPage", |ui| self.los_page(ui, host));
        } else {
            offstage(ui, "viewshedViewshedPage", |ui| {
                self.viewshed_page(ui, host);
            });
            self.los_page(ui, host);
        }
    }

    fn footer(&mut self, _ui: &mut Ui, _host: &mut dyn Host) {}
}
