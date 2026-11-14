//! The surface editing dialogs (terrain/surface_edit_dialogs.hpp; docs/terrain.md, "Editing a
//! surface"): Terrain > Edit Surface's Raise, Lower or Set Level, Smooth, Thin, Delete Long or
//! Thin Triangles, the four that take the drawing's data by a scope, and Combine Surfaces.
//!
//! No dialog edits a surface itself. Each writes the SURFACE EDIT, MERGE, PASTE, CLIP or OFFSET
//! line its fields describe - shown, as it will run, in its Command field - and Run hands it to
//! the window's one executor (`Host::run`) as if it had been typed: one undo step, logged. The
//! lines are made by pure functions of the fields, tested without a window.
//!
//! ```text
//! <p>Dialog, p = surfaceHeights, surfaceSmooth, surfaceThin, surfaceDeleteLong,
//!               surfaceDeletePoints, surfaceDeleteTriangles, surfaceAddPoints,
//!               surfaceDeleteBreakline:
//!   <p>Surface, <p>Value, <p>Operation, <p>Base, <p>Passes, <p>Weight, <p>Measure, <p>Side,
//!   <p>Inside, <p>Scope..., <p>Command, <p>Preview, <p>Run, <p>Close, <p>Reply
//! surfaceCombineDialog:
//!   surfaceCombineMode, surfaceCombinePages, surfaceCombineMergeList, surfaceCombineUp,
//!   surfaceCombineDown, surfaceCombineTop, surfaceCombineBase, surfaceCombineSource,
//!   surfaceCombineDz, surfaceCombineScope..., surfaceCombineName, surfaceCombineCommand,
//!   surfaceCombinePreview, surfaceCombineRun, surfaceCombineClose, surfaceCombineReply
//! ```
//!
//! THE LISTS FOLLOW THE DRAWING. A dialog is non-modal and kept, so the drawing changes under it:
//! an undo that removes the surface it offers, a typed SURFACE line, New, Open. A DocumentWatcher
//! says so at the next frame, and the lists are read again with what is chosen kept where it is
//! still there.

use egui::Ui;
use katana_cad::Document;
use katana_core::Result;
use katana_core::text::{equals_ignoring_case, parse_finite_double};

use crate::customisation::{DocumentWatcher, ScopeFilterWidget};
use crate::drive::controls::custom;
use crate::drive::registry::{ControlKind, ControlRecord};
use crate::drive::{Choice, button, check, choice, line, whole};
use crate::geo::terrain_dialog_parts::{
    Framed, check_list_with_current, command_field, field, host_views, invalid, row, show_framed,
};
use crate::geo::terrain_dialog_support::{line_word, terrain_reply_field};
use crate::host::{Host, VerbOutcome};
use crate::shell::DialogFrame;
use crate::survey::survey_dialogs::{hidden, offstage};

fn number(text: &str) -> Option<f64> {
    parse_finite_double(text.trim())
}

/// A surface's name as a word of a line, or why it cannot be one.
fn surface_word(name: &str, field: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(invalid(format!(
            "{field}: there is no surface; make one first (Terrain > Surface From)"
        )));
    }
    line_word(name).ok_or_else(|| {
        invalid(format!(
            "{field}: a name with a double quote cannot be written on a line"
        ))
    })
}

/// The surfaces of the drawing, by name, in their order.
fn surface_names(document: &Document) -> Vec<String> {
    document
        .surfaces()
        .all()
        .iter()
        .map(|surface| surface.name.clone())
        .collect()
}

/// Refills a choice's `names`, keeping the chosen one when it is still there.
fn fill_names(listed: &mut Vec<String>, chosen: &mut Choice, names: &[String]) {
    let kept = chosen.current_text(listed);
    *listed = names.to_vec();
    chosen.index = listed
        .iter()
        .position(|name| *name == kept)
        .or(if listed.is_empty() { None } else { Some(0) });
}

/// What a line said, as the reply shows it.
fn outcome_text(outcome: &VerbOutcome) -> String {
    if outcome.ok || outcome.error.is_empty() {
        outcome.reply.clone()
    } else {
        outcome.error.clone()
    }
}

/// Which edit a levels dialog makes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SurfaceLevelsKind {
    Heights,
    Smooth,
    Thin,
    DeleteLong,
    // The four that take the drawing's data by a scope, as the shared scope widget gives it - the
    // polygons of any number (a union), the lines, or the levelled points - and that a click on
    // one entity cannot say.
    DeletePoints,
    DeleteTriangles,
    AddPoints,
    /// The lines whose breakline edges are made ordinary edges.
    DeleteBreakline,
}

/// The object name prefix of a kind: surfaceHeights, surfaceSmooth ...
pub fn surface_levels_prefix(kind: SurfaceLevelsKind) -> &'static str {
    match kind {
        SurfaceLevelsKind::Heights => "surfaceHeights",
        SurfaceLevelsKind::Smooth => "surfaceSmooth",
        SurfaceLevelsKind::Thin => "surfaceThin",
        SurfaceLevelsKind::DeleteLong => "surfaceDeleteLong",
        SurfaceLevelsKind::DeletePoints => "surfaceDeletePoints",
        SurfaceLevelsKind::DeleteTriangles => "surfaceDeleteTriangles",
        SurfaceLevelsKind::AddPoints => "surfaceAddPoints",
        SurfaceLevelsKind::DeleteBreakline => "surfaceDeleteBreakline",
    }
}

fn levels_title(kind: SurfaceLevelsKind) -> &'static str {
    match kind {
        SurfaceLevelsKind::Heights => "Raise, Lower or Set Level",
        SurfaceLevelsKind::Smooth => "Smooth Surface",
        SurfaceLevelsKind::Thin => "Thin Surface",
        SurfaceLevelsKind::DeleteLong => "Delete Long or Thin Triangles",
        SurfaceLevelsKind::DeletePoints => "Delete Points Inside Polygons",
        SurfaceLevelsKind::DeleteTriangles => "Delete Triangles Inside or Outside Polygons",
        SurfaceLevelsKind::AddPoints => "Add Points from the Drawing",
        SurfaceLevelsKind::DeleteBreakline => "Delete Breaklines Along Lines",
    }
}

/// The fields of a levels dialog, as its line is written from them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SurfaceLevelsForm {
    pub surface: String,
    /// Heights: 0 Raise, 1 Lower, 2 Set Level, 3 Scale.
    pub operation: i32,
    pub value: String,
    pub base: String,
    pub passes: i32,
    pub weight: String,
    /// Delete long: sides longer than the value (true) or angles below it.
    pub long_sides: bool,
    /// Delete triangles: those outside the polygons (the union is kept).
    pub outside: bool,
    pub inside: bool,
    /// The scope words (ScopeFilterWidget::verb_words), or why there are none.
    pub scope: String,
    pub scope_error: String,
}

impl Default for SurfaceLevelsForm {
    fn default() -> Self {
        Self {
            surface: String::new(),
            operation: 0,
            value: String::new(),
            base: String::new(),
            passes: 3,
            weight: String::from("0.5"),
            long_sides: true,
            outside: false,
            inside: false,
            scope: String::new(),
            scope_error: String::new(),
        }
    }
}

/// ```text
/// SURFACE EDIT <s> RAISE|LOWER|SETLEVEL|SCALE <v> [base=<b>] [INSIDE FROM <scope>]
/// SURFACE EDIT <s> SMOOTH passes=<n> weight=<w> [INSIDE FROM <scope>]
/// SURFACE EDIT <s> THIN <tolerance>
/// SURFACE EDIT <s> DELETE LONG <length> | DELETE THIN <degrees>
/// SURFACE EDIT <s> DELETE POINTS INSIDE FROM <scope>
/// SURFACE EDIT <s> DELETE TRIANGLES INSIDE|OUTSIDE FROM <scope>
/// SURFACE EDIT <s> DELETE BREAKLINE FROM <scope>
/// SURFACE EDIT <s> ADD POINTS FROM <scope>
/// ```
///
/// InvalidArgument naming the field: no surface, a value that is not a number (or not positive
/// where it must be), a base that is not a number, a weight outside (0, 1], a scope the controls
/// cannot say.
pub fn surface_levels_line(kind: SurfaceLevelsKind, form: &SurfaceLevelsForm) -> Result<String> {
    let surface = surface_word(&form.surface, "Surface")?;
    let mut line = format!("SURFACE EDIT {surface}");
    let value = number(&form.value);
    let typed = form.value.trim();
    match kind {
        SurfaceLevelsKind::Heights => {
            const WORDS: [&str; 4] = ["RAISE", "LOWER", "SETLEVEL", "SCALE"];
            let Some(word) = usize::try_from(form.operation)
                .ok()
                .and_then(|at| WORDS.get(at))
            else {
                return Err(invalid(
                    "Change: choose Raise, Lower, Set Level or Scale",
                ));
            };
            if value.is_none() {
                return Err(invalid(match form.operation {
                    2 => "Level: type the level, a number",
                    3 => "Factor: type the factor, a number",
                    _ => "By: type how far, a number",
                }));
            }
            line.push_str(&format!(" {word} {typed}"));
            let base = form.base.trim();
            if form.operation == 3 && !base.is_empty() {
                if number(base).is_none() {
                    return Err(invalid(
                        "Base: a level, a number, or blank for the lowest level changed",
                    ));
                }
                line.push_str(&format!(" base={base}"));
            }
        }
        SurfaceLevelsKind::Smooth => {
            if !number(&form.weight).is_some_and(|weight| weight > 0.0 && weight <= 1.0) {
                return Err(invalid("Weight: a number above 0 and at most 1"));
            }
            if form.passes < 1 || form.passes > 1000 {
                return Err(invalid("Passes: from 1 to 1000"));
            }
            line.push_str(&format!(
                " SMOOTH passes={} weight={}",
                form.passes,
                form.weight.trim()
            ));
        }
        SurfaceLevelsKind::Thin => {
            if !value.is_some_and(|value| value >= 0.0) {
                return Err(invalid("Tolerance: a level difference, 0 or more"));
            }
            return Ok(format!("{line} THIN {typed}"));
        }
        SurfaceLevelsKind::DeleteLong => {
            let reads = value.is_some_and(|value| {
                value > 0.0 && (form.long_sides || value < 180.0)
            });
            if !reads {
                return Err(invalid(if form.long_sides {
                    "Longer than: a length above 0"
                } else {
                    "Thinner than: an angle in degrees, above 0 and below 180"
                }));
            }
            let words = if form.long_sides {
                "DELETE LONG"
            } else {
                "DELETE THIN"
            };
            return Ok(format!("{line} {words} {typed}"));
        }
        SurfaceLevelsKind::DeletePoints
        | SurfaceLevelsKind::DeleteTriangles
        | SurfaceLevelsKind::DeleteBreakline
        | SurfaceLevelsKind::AddPoints => {
            // The data is always the scope's: a click on one entity is the tool's.
            let (what, which) = match kind {
                SurfaceLevelsKind::AddPoints => ("Points", "points to add"),
                SurfaceLevelsKind::DeleteBreakline => ("Lines", "lines to use"),
                _ => ("Polygons", "polygons to use"),
            };
            if !form.scope_error.is_empty() {
                return Err(invalid(format!("{what}: {}", form.scope_error)));
            }
            let scope = form.scope.trim();
            if scope.is_empty() {
                return Err(invalid(format!("{what}: say which {which}")));
            }
            let words = match kind {
                SurfaceLevelsKind::DeletePoints => "DELETE POINTS INSIDE",
                SurfaceLevelsKind::DeleteTriangles if form.outside => "DELETE TRIANGLES OUTSIDE",
                SurfaceLevelsKind::DeleteTriangles => "DELETE TRIANGLES INSIDE",
                SurfaceLevelsKind::DeleteBreakline => "DELETE BREAKLINE",
                _ => "ADD POINTS",
            };
            return Ok(format!("{line} {words} FROM {scope}"));
        }
    }
    if form.inside {
        if !form.scope_error.is_empty() {
            return Err(invalid(format!("Only inside: {}", form.scope_error)));
        }
        let scope = form.scope.trim();
        if scope.is_empty() {
            return Err(invalid("Only inside: say which polygon"));
        }
        line.push_str(&format!(" INSIDE FROM {scope}"));
    }
    Ok(line)
}

/// What Combine Surfaces makes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SurfaceCombineMode {
    #[default]
    Merge,
    Paste,
    Clip,
    Offset,
}

const COMBINE_MODES: [SurfaceCombineMode; 4] = [
    SurfaceCombineMode::Merge,
    SurfaceCombineMode::Paste,
    SurfaceCombineMode::Clip,
    SurfaceCombineMode::Offset,
];

/// The fields of Combine Surfaces, as its line is written from them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SurfaceCombineForm {
    pub mode: SurfaceCombineMode,
    /// In rank order: later ones win.
    pub merge: Vec<String>,
    pub top: String,
    pub base: String,
    pub surface: String,
    pub dz: String,
    pub name: String,
    pub scope: String,
    pub scope_error: String,
}

/// ```text
/// SURFACE MERGE <a> <b> ... NAME <new>
/// SURFACE PASTE <top> INTO <base> [NAME <new>]
/// SURFACE CLIP <s> FROM <scope> NAME <new>
/// SURFACE OFFSET <s> <dz> NAME <new>
/// ```
///
/// InvalidArgument naming the field: fewer than two surfaces to merge, a top that is its base, no
/// surface, a dz that is not a number, no name where one is needed, a scope the controls cannot
/// say.
pub fn surface_combine_line(form: &SurfaceCombineForm) -> Result<String> {
    let name = |required: bool| -> Result<String> {
        let name = form.name.trim();
        if name.is_empty() {
            if required {
                return Err(invalid("New surface: type its name"));
            }
            return Ok(String::new());
        }
        match line_word(name) {
            Some(word) => Ok(format!(" NAME {word}")),
            None => Err(invalid(
                "New surface: a name with a double quote cannot be written on a line",
            )),
        }
    };
    match form.mode {
        SurfaceCombineMode::Merge => {
            if form.merge.len() < 2 {
                return Err(invalid("Surfaces: tick at least two to merge"));
            }
            let mut line = String::from("SURFACE MERGE");
            for surface in &form.merge {
                line.push_str(&format!(" {}", surface_word(surface, "Surfaces")?));
            }
            Ok(line + &name(true)?)
        }
        SurfaceCombineMode::Paste => {
            let top = surface_word(&form.top, "Top")?;
            let base = surface_word(&form.base, "Base")?;
            if equals_ignoring_case(form.top.trim().as_bytes(), form.base.trim().as_bytes()) {
                return Err(invalid("Top: choose a surface other than the base"));
            }
            Ok(format!("SURFACE PASTE {top} INTO {base}{}", name(false)?))
        }
        SurfaceCombineMode::Clip => {
            let surface = surface_word(&form.surface, "Surface")?;
            if !form.scope_error.is_empty() {
                return Err(invalid(format!("Polygon: {}", form.scope_error)));
            }
            let scope = form.scope.trim();
            if scope.is_empty() {
                return Err(invalid("Polygon: say which polygon"));
            }
            Ok(format!(
                "SURFACE CLIP {surface} FROM {scope}{}",
                name(true)?
            ))
        }
        SurfaceCombineMode::Offset => {
            let surface = surface_word(&form.surface, "Surface")?;
            if number(&form.dz).is_none() {
                return Err(invalid("Raise by: a number (negative lowers)"));
            }
            Ok(format!(
                "SURFACE OFFSET {surface} {}{}",
                form.dz.trim(),
                name(true)?
            ))
        }
    }
}

fn strings(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| String::from(*word)).collect()
}

// ---- SurfaceLevelsDialog ------------------------------------------------------------------------

/// One of Terrain > Edit Surface's dialogs (see the module comment).
pub struct SurfaceLevelsDialog {
    // First, so it goes first: no delivery reaches a half-dropped dialog.
    watcher: DocumentWatcher,
    kind: SurfaceLevelsKind,
    frame: DialogFrame,
    visible: bool,
    surfaces: Vec<String>,
    surface: Choice,
    operations: Vec<String>,
    operation: Choice,
    value: String,
    base: String,
    passes: i64,
    weight: String,
    measures: Vec<String>,
    measure: Choice,
    sides: Vec<String>,
    side: Choice,
    inside: bool,
    scope: Option<ScopeFilterWidget>,
    reply: String,
}

impl SurfaceLevelsDialog {
    /// The dialog of `kind` over the window's drawing and views.
    pub fn new(kind: SurfaceLevelsKind, host: &mut dyn Host) -> Self {
        let prefix = surface_levels_prefix(kind);
        // The four data dialogs are about their scope; Heights and Smooth keep it behind a tick
        // box; Thin and Delete Long have none.
        let has_scope = !matches!(
            kind,
            SurfaceLevelsKind::Thin | SurfaceLevelsKind::DeleteLong
        );
        let scope = has_scope.then(|| {
            let mut scope = ScopeFilterWidget::new(prefix);
            scope.views = host_views(host);
            scope
        });
        let value = match kind {
            SurfaceLevelsKind::Heights => "1",
            SurfaceLevelsKind::Thin => "0.01",
            SurfaceLevelsKind::DeleteLong => "50",
            _ => "",
        };
        let mut dialog = Self {
            watcher: DocumentWatcher::new(host.document()),
            kind,
            frame: DialogFrame::new(&format!("{prefix}Dialog"), levels_title(kind)),
            visible: false,
            surfaces: Vec::new(),
            surface: Choice::default(),
            operations: strings(&["Raise", "Lower", "Set Level", "Scale"]),
            operation: Choice::at(0),
            value: String::from(value),
            base: String::new(),
            passes: 3,
            weight: String::from("0.5"),
            measures: strings(&["A side longer than", "An angle smaller than (degrees)"]),
            measure: Choice::at(0),
            sides: strings(&[
                "Inside the polygons",
                "Outside the polygons (keep what is inside)",
            ]),
            side: Choice::at(0),
            inside: false,
            scope,
            reply: String::new(),
        };
        dialog.reload(&*host);
        dialog
    }

    pub fn kind(&self) -> SurfaceLevelsKind {
        self.kind
    }

    /// The dialog's object name: <prefix>Dialog.
    pub fn object_name(&self) -> &str {
        self.frame.name()
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

    /// Refills the surfaces and the scope's layers and views, keeping what is chosen: done when
    /// shown.
    pub fn reload(&mut self, host: &dyn Host) {
        let names = surface_names(host.document());
        fill_names(&mut self.surfaces, &mut self.surface, &names);
        if let Some(scope) = &mut self.scope {
            scope.reload(host.document().model());
        }
    }

    /// Whether the kind keeps its scope behind the Only inside box.
    fn has_inside_box(&self) -> bool {
        matches!(
            self.kind,
            SurfaceLevelsKind::Heights | SurfaceLevelsKind::Smooth
        )
    }

    pub fn form(&self) -> SurfaceLevelsForm {
        let mut form = SurfaceLevelsForm {
            surface: self.surface.current_text(&self.surfaces),
            value: self.value.clone(),
            ..SurfaceLevelsForm::default()
        };
        match self.kind {
            SurfaceLevelsKind::Heights => {
                form.operation = self
                    .operation
                    .index
                    .and_then(|at| i32::try_from(at).ok())
                    .unwrap_or(-1);
                form.base.clone_from(&self.base);
            }
            SurfaceLevelsKind::Smooth => {
                form.passes = i32::try_from(self.passes).unwrap_or(0);
                form.weight.clone_from(&self.weight);
            }
            SurfaceLevelsKind::DeleteLong => form.long_sides = self.measure.index == Some(0),
            SurfaceLevelsKind::DeleteTriangles => form.outside = self.side.index == Some(1),
            _ => {}
        }
        // A scope behind a tick box is asked when ticked; one with none always.
        if let Some(scope) = &self.scope
            && (!self.has_inside_box() || self.inside)
        {
            form.inside = true;
            match scope.verb_words() {
                Ok(words) => form.scope = words,
                Err(error) => form.scope_error = error.message,
            }
        }
        form
    }

    pub fn command(&self) -> Result<String> {
        surface_levels_line(self.kind, &self.form())
    }

    /// The scope and filter controls; None for a kind that has none.
    pub fn scope_controls(&mut self) -> Option<&mut ScopeFilterWidget> {
        self.scope.as_mut()
    }

    pub fn reply(&self) -> &str {
        &self.reply
    }

    /// What Run (`preview` false) and Preview do.
    pub fn run_line(&mut self, host: &mut dyn Host, preview: bool) {
        let line = match self.command() {
            Ok(line) => line,
            Err(error) => {
                self.reply = error.message;
                return;
            }
        };
        let line = if preview {
            format!("{line} PREVIEW")
        } else {
            line
        };
        self.reply = outcome_text(&host.run(&line));
        if !preview {
            self.reload(&*host);
        }
    }

    /// One frame of the dialog, when it is shown: its drawing's changes delivered first.
    pub fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        if !self.visible {
            return;
        }
        if self
            .watcher
            .deliver(host.document())
            .is_some_and(|changes| changes.model)
        {
            self.reload(&*host);
        }
        if show_framed(self, ui, host) {
            self.hide();
        }
    }
}

impl Framed for SurfaceLevelsDialog {
    fn frame_mut(&mut self) -> &mut DialogFrame {
        &mut self.frame
    }

    fn body(&mut self, ui: &mut Ui, _host: &mut dyn Host) {
        let p = surface_levels_prefix(self.kind);
        let named = |part: &str| format!("{p}{part}");
        row(ui, "Surface:", |ui| {
            choice(ui, &named("Surface"), &self.surfaces, &mut self.surface).on_hover_text(
                "The surface of the drawing the edit changes, as one undo step",
            );
        });
        match self.kind {
            SurfaceLevelsKind::Heights => {
                row(ui, "Change:", |ui| {
                    choice(ui, &named("Operation"), &self.operations, &mut self.operation);
                });
                row(ui, "By / to:", |ui| {
                    line(ui, &named("Value"), &mut self.value).on_hover_text(
                        "How far to raise or lower, the level to set, or the factor to scale \
                         heights by",
                    );
                });
                let scales = self.operation.index == Some(3);
                row(ui, "Scale about:", |ui| {
                    ui.add_enabled_ui(scales, |ui| {
                        field(ui, &named("Base"), &mut self.base, "lowest level changed")
                            .on_hover_text("Scale only: the level that stays where it is");
                    });
                });
            }
            SurfaceLevelsKind::Smooth => {
                row(ui, "Passes:", |ui| {
                    whole(ui, &named("Passes"), &mut self.passes, 1..=1000);
                });
                row(ui, "Weight:", |ui| {
                    line(ui, &named("Weight"), &mut self.weight).on_hover_text(
                        "How far each pass moves a vertex towards the mean of its neighbours: \
                         above 0, at most 1",
                    );
                });
            }
            SurfaceLevelsKind::Thin => {
                row(ui, "Tolerance:", |ui| {
                    line(ui, &named("Value"), &mut self.value).on_hover_text(
                        "The most the surface may move, vertically, anywhere",
                    );
                });
            }
            SurfaceLevelsKind::DeleteLong => {
                row(ui, "Delete triangles with:", |ui| {
                    choice(ui, &named("Measure"), &self.measures, &mut self.measure);
                });
                row(ui, "Value:", |ui| {
                    line(ui, &named("Value"), &mut self.value);
                });
            }
            SurfaceLevelsKind::DeleteTriangles => {
                row(ui, "Delete triangles:", |ui| {
                    choice(ui, &named("Side"), &self.sides, &mut self.side).on_hover_text(
                        "Several polygons are their union: outside deletes what none of them \
                         covers",
                    );
                });
            }
            SurfaceLevelsKind::DeletePoints
            | SurfaceLevelsKind::DeleteBreakline
            | SurfaceLevelsKind::AddPoints => {}
        }
        // The value field is there for every kind, hidden where the kind has no value.
        if matches!(
            self.kind,
            SurfaceLevelsKind::Smooth
                | SurfaceLevelsKind::DeleteTriangles
                | SurfaceLevelsKind::DeletePoints
                | SurfaceLevelsKind::DeleteBreakline
                | SurfaceLevelsKind::AddPoints
        ) {
            hidden(ui, &named("ValueHidden"), |ui| {
                line(ui, &named("Value"), &mut self.value);
            });
        }
        let boxed = self.has_inside_box();
        if boxed {
            check(
                ui,
                &named("Inside"),
                "Only inside a polygon of the drawing",
                &mut self.inside,
            );
        }
        // The scope is what the data dialogs are about, so it is not behind a tick box there.
        let asked = !boxed || self.inside;
        if let Some(scope) = &mut self.scope {
            ui.add_enabled_ui(asked, |ui| {
                scope.ui(ui);
            });
        }
        command_field(ui, &named("Command"), &self.command());
    }

    fn footer(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        let p = surface_levels_prefix(self.kind);
        // Nothing to run until the fields say a line; the placeholder says why.
        let ready = self.command().is_ok();
        let (preview, run, close) = ui
            .horizontal(|ui| {
                let (preview, run) = ui
                    .add_enabled_ui(ready, |ui| {
                        let preview = button(ui, &format!("{p}Preview"), "Preview")
                            .on_hover_text(
                                "Run the line with PREVIEW: what it would change, nothing changed",
                            )
                            .clicked();
                        let run = button(ui, &format!("{p}Run"), "Run").clicked();
                        (preview, run)
                    })
                    .inner;
                let close = button(ui, &format!("{p}Close"), "Close").clicked();
                (preview, run, close)
            })
            .inner;
        terrain_reply_field(ui, &format!("{p}Reply"), &mut self.reply);
        if preview {
            self.run_line(host, true);
        }
        if run {
            self.run_line(host, false);
        }
        if close {
            self.hide();
        }
    }
}

// ---- SurfaceCombineDialog -----------------------------------------------------------------------

/// Terrain > Edit Surface > Combine Surfaces (see the module comment).
pub struct SurfaceCombineDialog {
    // First, so it goes first: no delivery reaches a half-dropped dialog.
    watcher: DocumentWatcher,
    frame: DialogFrame,
    visible: bool,
    modes: Vec<String>,
    mode: Choice,
    /// The merge list: each surface and whether it is ticked, in rank order.
    merge: Vec<(String, bool)>,
    merge_current: Option<usize>,
    surfaces: Vec<String>,
    top: Choice,
    bases: Vec<String>,
    base: Choice,
    sources: Vec<String>,
    source: Choice,
    dz: String,
    scope: ScopeFilterWidget,
    name: String,
    reply: String,
}

impl SurfaceCombineDialog {
    /// The dialog over the window's drawing and views, on Merge.
    pub fn new(host: &mut dyn Host) -> Self {
        let mut scope = ScopeFilterWidget::new("surfaceCombine");
        scope.views = host_views(host);
        let mut dialog = Self {
            watcher: DocumentWatcher::new(host.document()),
            frame: DialogFrame::new("surfaceCombineDialog", "Combine Surfaces"),
            visible: false,
            modes: strings(&[
                "Merge: later surfaces win",
                "Paste: a top into a base, with a clean seam",
                "Clip: a copy of the part inside a polygon",
                "Offset: a copy raised by dz",
            ]),
            mode: Choice::at(0),
            merge: Vec::new(),
            merge_current: None,
            surfaces: Vec::new(),
            top: Choice::default(),
            bases: Vec::new(),
            base: Choice::default(),
            sources: Vec::new(),
            source: Choice::default(),
            dz: String::from("1"),
            scope,
            name: String::new(),
            reply: String::new(),
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

    pub fn mode(&self) -> SurfaceCombineMode {
        self.mode
            .index
            .and_then(|at| COMBINE_MODES.get(at))
            .copied()
            .unwrap_or_default()
    }

    pub fn set_mode(&mut self, mode: SurfaceCombineMode) {
        self.mode.index = COMBINE_MODES.iter().position(|listed| *listed == mode);
    }

    /// The merge list: each surface and whether it is ticked, in rank order.
    pub fn merge_list(&self) -> &[(String, bool)] {
        &self.merge
    }

    /// Ticks or clears the merge list's row `at` (its item's setCheckState).
    pub fn set_merge_ticked(&mut self, at: usize, ticked: bool) {
        if let Some(item) = self.merge.get_mut(at) {
            item.1 = ticked;
        }
    }

    /// Picks the merge list's row `at` (setCurrentRow), which Up and Down move.
    pub fn set_merge_current(&mut self, at: usize) {
        if at < self.merge.len() {
            self.merge_current = Some(at);
        }
    }

    pub fn reload(&mut self, host: &dyn Host) {
        let names = surface_names(host.document());
        // The merge list keeps its order and ticks for the names still there.
        let mut ordered: Vec<(String, bool)> = self
            .merge
            .iter()
            .filter(|(name, _)| names.contains(name))
            .cloned()
            .collect();
        for name in &names {
            if !ordered.iter().any(|(listed, _)| listed == name) {
                ordered.push((name.clone(), false));
            }
        }
        self.merge = ordered;
        // A list refilled has no row picked.
        self.merge_current = None;
        fill_names(&mut self.surfaces, &mut self.top, &names);
        fill_names(&mut self.bases, &mut self.base, &names);
        fill_names(&mut self.sources, &mut self.source, &names);
        if self.base.index == self.top.index && self.bases.len() > 1 {
            self.base.index = Some(if self.top.index == Some(0) { 1 } else { 0 });
        }
        self.scope.reload(host.document().model());
    }

    fn move_row(&mut self, by: isize) {
        let Some(row) = self.merge_current else {
            return;
        };
        let Some(to) = row.checked_add_signed(by) else {
            return;
        };
        if to >= self.merge.len() {
            return;
        }
        let item = self.merge.remove(row);
        self.merge.insert(to, item);
        self.merge_current = Some(to);
    }

    pub fn form(&self) -> SurfaceCombineForm {
        let mode = self.mode();
        let mut form = SurfaceCombineForm {
            mode,
            merge: self
                .merge
                .iter()
                .filter(|(_, ticked)| *ticked)
                .map(|(name, _)| name.clone())
                .collect(),
            top: self.top.current_text(&self.surfaces),
            base: self.base.current_text(&self.bases),
            surface: self.source.current_text(&self.sources),
            dz: self.dz.clone(),
            name: self.name.clone(),
            ..SurfaceCombineForm::default()
        };
        if mode == SurfaceCombineMode::Clip {
            match self.scope.verb_words() {
                Ok(words) => form.scope = words,
                Err(error) => form.scope_error = error.message,
            }
        }
        form
    }

    pub fn command(&self) -> Result<String> {
        surface_combine_line(&self.form())
    }

    pub fn scope_controls(&mut self) -> &mut ScopeFilterWidget {
        &mut self.scope
    }

    pub fn reply(&self) -> &str {
        &self.reply
    }

    /// What Run (`preview` false) and Preview do.
    pub fn run_line(&mut self, host: &mut dyn Host, preview: bool) {
        let line = match self.command() {
            Ok(line) => line,
            Err(error) => {
                self.reply = error.message;
                return;
            }
        };
        let line = if preview {
            format!("{line} PREVIEW")
        } else {
            line
        };
        self.reply = outcome_text(&host.run(&line));
        if !preview {
            self.reload(&*host);
        }
    }

    /// One frame of the dialog, when it is shown: its drawing's changes delivered first.
    pub fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        if !self.visible {
            return;
        }
        if self
            .watcher
            .deliver(host.document())
            .is_some_and(|changes| changes.model)
        {
            self.reload(&*host);
        }
        if show_framed(self, ui, host) {
            self.hide();
        }
    }

    fn merge_page(&mut self, ui: &mut Ui) {
        let (up, down) = ui
            .horizontal_top(|ui| {
                ui.vertical(|ui| {
                    check_list_with_current(
                        ui,
                        "surfaceCombineMergeList",
                        &mut self.merge,
                        &mut self.merge_current,
                        160.0,
                        true,
                    );
                })
                .response
                .on_hover_text(
                    "Tick the surfaces to merge; lower in the list wins where they overlap",
                );
                ui.vertical(|ui| {
                    let up = button(ui, "surfaceCombineUp", "Up").clicked();
                    let down = button(ui, "surfaceCombineDown", "Down").clicked();
                    (up, down)
                })
                .inner
            })
            .inner;
        if up {
            self.move_row(-1);
        }
        if down {
            self.move_row(1);
        }
    }

    fn paste_page(&mut self, ui: &mut Ui) {
        row(ui, "Top (wins inside its footprint):", |ui| {
            choice(ui, "surfaceCombineTop", &self.surfaces, &mut self.top);
        });
        row(ui, "Base:", |ui| {
            choice(ui, "surfaceCombineBase", &self.bases, &mut self.base);
        });
    }

    /// Clip and Offset share the source.
    fn source_page(&mut self, ui: &mut Ui) {
        let mode = self.mode();
        row(ui, "Surface:", |ui| {
            choice(ui, "surfaceCombineSource", &self.sources, &mut self.source);
        });
        row(ui, "Raise by:", |ui| {
            ui.add_enabled_ui(mode == SurfaceCombineMode::Offset, |ui| {
                line(ui, "surfaceCombineDz", &mut self.dz)
                    .on_hover_text("How far to raise the copy; negative lowers it");
            });
        });
        ui.add_enabled_ui(mode == SurfaceCombineMode::Clip, |ui| {
            self.scope.ui(ui);
        });
    }
}

impl Framed for SurfaceCombineDialog {
    fn frame_mut(&mut self) -> &mut DialogFrame {
        &mut self.frame
    }

    fn body(&mut self, ui: &mut Ui, _host: &mut dyn Host) {
        row(ui, "Make:", |ui| {
            choice(ui, "surfaceCombineMode", &self.modes, &mut self.mode);
        });
        // The page shown takes the room; the others are still there, as a QStackedWidget's are:
        // what they hold can be filled, pressed and read.
        let page = match self.mode() {
            SurfaceCombineMode::Merge => 0,
            SurfaceCombineMode::Paste => 1,
            SurfaceCombineMode::Clip | SurfaceCombineMode::Offset => 2,
        };
        let pages = ui.vertical(|ui| {
            for at in 0..3 {
                let mut draw = |ui: &mut Ui| match at {
                    0 => self.merge_page(ui),
                    1 => self.paste_page(ui),
                    _ => self.source_page(ui),
                };
                if at == page {
                    draw(ui);
                } else {
                    offstage(ui, &format!("surfaceCombinePage{at}"), draw);
                }
            }
        });
        custom(
            ui,
            &pages.response,
            ControlRecord::new("surfaceCombinePages", ControlKind::Other),
        );
        let placeholder = if self.mode() == SurfaceCombineMode::Paste {
            "blank: paste into the base in place"
        } else {
            "the new surface's name"
        };
        row(ui, "New surface:", |ui| {
            field(ui, "surfaceCombineName", &mut self.name, placeholder);
        });
        command_field(ui, "surfaceCombineCommand", &self.command());
    }

    fn footer(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        // Nothing to run until the fields say a line; the placeholder says why.
        let ready = self.command().is_ok();
        let (preview, run, close) = ui
            .horizontal(|ui| {
                let (preview, run) = ui
                    .add_enabled_ui(ready, |ui| {
                        let preview = button(ui, "surfaceCombinePreview", "Preview")
                            .on_hover_text(
                                "Run the line with PREVIEW: what it would make, nothing changed",
                            )
                            .clicked();
                        let run = button(ui, "surfaceCombineRun", "Run").clicked();
                        (preview, run)
                    })
                    .inner;
                let close = button(ui, "surfaceCombineClose", "Close").clicked();
                (preview, run, close)
            })
            .inner;
        terrain_reply_field(ui, "surfaceCombineReply", &mut self.reply);
        if preview {
            self.run_line(host, true);
        }
        if run {
            self.run_line(host, false);
        }
        if close {
            self.hide();
        }
    }
}
