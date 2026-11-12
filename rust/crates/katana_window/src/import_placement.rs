//! Where an import lands, in the window (import_placement.hpp; docs/interop.md, "Placing an
//! import"): the Placement group the import dialogs share, File > Import's small step for a DXF or
//! a .12da archive, and the one place the window decides - or asks - where data it has read goes.
//!
//! What a placement MEANS is katana_cad's `import_placement::resolve_import_shift`, the same for a
//! typed IMPORT line, katana_cli and katana_mcp; this file is only its window face.
//!
//! Object names:
//!
//! ```text
//! importPlacement            the group
//! importPlacementKeep        its own coordinates (the default)
//! importPlacementLocal       lower-left corner to 0,0 (LOCAL)
//! importPlacementAlongside   lower-left corner onto the drawing's (ALONGSIDE)
//! importPlacementOffset      moved by importOffsetE, importOffsetN (OFFSET=dE,dN)
//! importPlacementShift       what the choice will do, and the line that does the same
//! importPlacementDialog      File > Import's step, with importPlacementImport and
//!                            importPlacementCancel
//! ```
//!
//! The far-apart question was a QMessageBox, exec'd, with no object names. egui cannot stop the
//! frame to ask (the plan's section 4.1): the decision says the window must ask
//! (`PlacementDecision::ask`), the window shows `FarApartBox` - its target and buttons named
//! `importFarApartDialog`, `importFarApartShift`, `importFarApartKeep` and `importFarApartCancel`,
//! names the C++ box did not have - and `decide_far_apart` finishes the decision with the answer.
//! A headless session is never asked, as the C++'s was not.

use std::path::Path;

use egui::Ui;
use katana_cad::import_placement::{
    ImportPlacement, ImportPlacementMode, parse_placement_word, placement_word,
    resolve_import_shift,
};
use katana_geometry::{Box2, Vec2};
use katana_interop::import::advise_placement;

use crate::drive::{SpinRange, button, label_as, radio, spin};
use crate::format::qt_fixed;
use crate::host::{Host, UserSettings};
use crate::shell::DialogFrame;

/// Where `remember` keeps the last choice, as its IMPORT word.
pub const PLACEMENT_KEY: &str = "import/placement";

/// Survey coordinates run to 10^7 m (a national grid's northings); an offset past a thousand times
/// that is a typing slip, not a place.
const LARGEST_OFFSET: f64 = 1.0e10;

/// The offset boxes: the millimetre, as survey coordinates are quoted; QDoubleSpinBox's step of 1.
fn offset_range(suffix: &'static str) -> SpinRange {
    SpinRange::new(-LARGEST_OFFSET, LARGEST_OFFSET, 1.0, 3).with_suffix(suffix)
}

/// The IMPORT line that imports `path` with `placement`: the path quoted, then the placement's word
/// when it has one - what a person would type, and what File > Import runs through the window's one
/// executor.
pub fn import_line(path: &str, placement: &ImportPlacement) -> String {
    let word = placement_word(placement);
    if word.is_empty() {
        format!("IMPORT \"{path}\"")
    } else {
        format!("IMPORT \"{path}\" {word}")
    }
}

/// The Placement group (see the module comment).
#[derive(Clone, Debug, PartialEq)]
pub struct ImportPlacementBox {
    drawing: Box2,
    path: String,
    mode: ImportPlacementMode,
    east: f64,
    north: f64,
}

impl ImportPlacementBox {
    /// `drawing` is the drawing's extent now, for what Alongside will do; `path` the file, for the
    /// line shown. Starts at the choice last made (`remember`), Keep the first time.
    pub fn new(drawing: Box2, path: &str, settings: &UserSettings) -> Self {
        let mut made = Self {
            drawing,
            path: String::from(path),
            mode: ImportPlacementMode::Keep,
            east: 0.0,
            north: 0.0,
        };
        // The last choice, as its IMPORT word; anything unreadable is Keep.
        let mut last = ImportPlacement::default();
        if let Ok(Some(remembered)) =
            parse_placement_word(settings.text(PLACEMENT_KEY).unwrap_or(""))
        {
            last = remembered;
        }
        made.set_placement(&last);
        made
    }

    pub fn placement(&self) -> ImportPlacement {
        ImportPlacement {
            mode: self.mode,
            offset: if self.mode == ImportPlacementMode::Offset {
                Vec2::new(self.east, self.north)
            } else {
                Vec2::default()
            },
        }
    }

    /// The choice made `placement`; an offset's numbers as the boxes hold them (QDoubleSpinBox
    /// keeps a value in its range, to its three decimals).
    pub fn set_placement(&mut self, placement: &ImportPlacement) {
        self.mode = placement.mode;
        if placement.mode == ImportPlacementMode::Offset {
            let range = offset_range("");
            self.east = range.rounded(katana_math::cpp::clamp(
                placement.offset.x,
                -LARGEST_OFFSET,
                LARGEST_OFFSET,
            ));
            self.north = range.rounded(katana_math::cpp::clamp(
                placement.offset.y,
                -LARGEST_OFFSET,
                LARGEST_OFFSET,
            ));
        }
    }

    /// What importPlacementShift says now.
    pub fn said(&self) -> String {
        let chosen = self.placement();
        let mut text = match chosen.mode {
            ImportPlacementMode::Keep => String::from(
                "The data keeps its own coordinates; if it lands far from the drawing, you are \
                 asked where to put it.",
            ),
            ImportPlacementMode::Local => String::from(
                "The data is moved as one piece so its lower-left corner sits at 0,0. The move \
                 is read off the file, and logged.",
            ),
            // The drawing is known now; the data's corner only once it is read.
            ImportPlacementMode::Alongside => {
                if self.drawing.empty() {
                    String::from(
                        "The drawing is empty, so there is nothing to sit beside: the data keeps \
                         its own coordinates.",
                    )
                } else {
                    format!(
                        "The data is moved as one piece so its lower-left corner sits on the \
                         drawing's, at {},{}.",
                        qt_fixed(self.drawing.min.x + 0.0, 3),
                        qt_fixed(self.drawing.min.y + 0.0, 3)
                    )
                }
            }
            ImportPlacementMode::Offset => format!(
                "The data is moved as one piece by {},{}.",
                qt_fixed(chosen.offset.x + 0.0, 3),
                qt_fixed(chosen.offset.y + 0.0, 3)
            ),
        };
        if !self.path.is_empty() {
            text.push_str("\nTyped: ");
            text.push_str(&import_line(&self.path, &chosen));
        }
        text
    }

    /// Keeps the choice for the next import: called when an import is accepted, not on every
    /// click.
    pub fn remember(&self, settings: &mut UserSettings) {
        settings.set_text(PLACEMENT_KEY, &placement_word(&self.placement()));
    }

    /// The group drawn; true when the choice changed.
    pub fn ui(&mut self, ui: &mut Ui) -> bool {
        let before = self.placement();
        ui.group(|ui| {
            ui.strong("Placement");
            for (name, text, mode, tip) in [
                (
                    "importPlacementKeep",
                    "Keep its own coordinates",
                    ImportPlacementMode::Keep,
                    "Where the file says. If that is far from the drawing, you are asked where to \
                     put it.",
                ),
                (
                    "importPlacementLocal",
                    "Move to a local origin (lower-left corner at 0,0)",
                    ImportPlacementMode::Local,
                    "",
                ),
                (
                    "importPlacementAlongside",
                    "Move alongside the drawing (lower-left corner to the drawing's)",
                    ImportPlacementMode::Alongside,
                    "",
                ),
            ] {
                let mut response = radio(ui, name, text, self.mode == mode);
                if !tip.is_empty() {
                    response = response.on_hover_text(tip);
                }
                if response.clicked() {
                    self.mode = mode;
                }
            }
            ui.horizontal(|ui| {
                if radio(
                    ui,
                    "importPlacementOffset",
                    "Move by",
                    self.mode == ImportPlacementMode::Offset,
                )
                .clicked()
                {
                    self.mode = ImportPlacementMode::Offset;
                }
                let offset = self.mode == ImportPlacementMode::Offset;
                ui.add_enabled_ui(offset, |ui| {
                    spin(ui, "importOffsetE", &mut self.east, offset_range(" east"));
                    spin(ui, "importOffsetN", &mut self.north, offset_range(" north"));
                });
            });
            let said = self.said();
            label_as(
                ui,
                "importPlacementShift",
                &said,
                egui::Label::new(said.as_str()).wrap(),
            );
        });
        self.placement() != before
    }
}

/// What File > Import's step answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogResult {
    Accepted,
    Rejected,
}

/// File > Import's step for a file whose only choice is where it lands: a DXF or a .12da archive
/// (a GIS vector file's is in Import Vector Data's own dialog, which has more to ask).
pub struct ImportPlacementDialog {
    frame: DialogFrame,
    path: String,
    placement: ImportPlacementBox,
    result: Option<DialogResult>,
}

impl ImportPlacementDialog {
    pub fn new(path: &str, drawing: Box2, settings: &UserSettings) -> Self {
        let file = Path::new(path)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        Self {
            frame: DialogFrame::new("importPlacementDialog", &format!("Import - {file}"))
                .modal(true),
            path: String::from(path),
            placement: ImportPlacementBox::new(drawing, path, settings),
            result: None,
        }
    }

    pub fn object_name(&self) -> &str {
        self.frame.name()
    }

    pub fn placement_box(&self) -> &ImportPlacementBox {
        &self.placement
    }

    pub fn placement_box_mut(&mut self) -> &mut ImportPlacementBox {
        &mut self.placement
    }

    /// The IMPORT line the choice makes (`import_line`).
    pub fn line(&self) -> String {
        import_line(&self.path, &self.placement.placement())
    }

    /// QDialog::result: None while it is open.
    pub fn result(&self) -> Option<DialogResult> {
        self.result
    }

    /// Import: the choice remembered for the next import, and the dialog accepted.
    pub fn accept(&mut self, settings: &mut UserSettings) {
        self.placement.remember(settings);
        self.result = Some(DialogResult::Accepted);
    }

    pub fn reject(&mut self) {
        self.result = Some(DialogResult::Rejected);
    }

    /// Draws the step while it is open; answers once Import or Cancel was pressed. The window then
    /// runs `line()` through its executor when it was accepted.
    pub fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) -> Option<DialogResult> {
        if self.result.is_some() {
            return self.result;
        }
        let ctx = ui.ctx().clone();
        let mut pressed = None;
        let mut parts = (&mut self.placement, &mut pressed);
        let shown = self.frame.show(
            &ctx,
            &mut parts,
            |ui, parts| {
                parts.0.ui(ui);
            },
            |ui, parts| {
                ui.horizontal(|ui| {
                    if button(ui, "importPlacementImport", "Import").clicked() {
                        *parts.1 = Some(DialogResult::Accepted);
                    }
                    if button(ui, "importPlacementCancel", "Cancel").clicked() {
                        *parts.1 = Some(DialogResult::Rejected);
                    }
                });
            },
        );
        match pressed {
            Some(DialogResult::Accepted) => self.accept(host.settings()),
            Some(DialogResult::Rejected) => self.reject(),
            None if shown.closed => self.reject(),
            None => {}
        }
        self.result
    }
}

// ---- deciding ---------------------------------------------------------------------------------

/// The person's answer when data that keeps its own coordinates lands far from the drawing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FarApartAnswer {
    ShiftAlongside,
    Keep,
    Cancel,
}

/// The far-apart question's longer text: where the file lies, and what each answer does.
pub fn far_apart_informative_text(incoming: &Box2) -> String {
    format!(
        "The file covers {},{} to {},{}.\n\nShifting moves everything it holds as one piece so \
         its lower-left corner sits on the drawing's; its shape and internal dimensions are \
         unchanged. IMPORT ... ALONGSIDE does the same without asking.",
        qt_fixed(incoming.min.x, 2),
        qt_fixed(incoming.min.y, 2),
        qt_fixed(incoming.max.x, 2),
        qt_fixed(incoming.max.y, 2)
    )
}

/// The window's question (askFarApart): Shift Alongside, Keep Survey Coordinates, or Cancel. Drawn
/// modally on the desktop; never shown headless.
pub struct FarApartBox {
    frame: DialogFrame,
    advice: String,
    incoming: Box2,
}

impl FarApartBox {
    /// `advice` is interop's advice message, `incoming` the data's extent.
    pub fn new(advice: &str, incoming: Box2) -> Self {
        Self {
            frame: DialogFrame::new("importFarApartDialog", "Far from the current drawing")
                .modal(true),
            advice: String::from(advice),
            incoming,
        }
    }

    /// The box drawn; the answer once a button was pressed (the title bar's close is Cancel).
    pub fn ui(&mut self, ui: &mut Ui) -> Option<FarApartAnswer> {
        let ctx = ui.ctx().clone();
        let mut answer = None;
        let text = format!("{}.", self.advice);
        let informative = far_apart_informative_text(&self.incoming);
        let shown = self.frame.show(
            &ctx,
            &mut answer,
            |ui, _| {
                ui.add(egui::Label::new(text.as_str()).wrap());
                ui.add(egui::Label::new(informative.as_str()).wrap());
            },
            |ui, answer| {
                ui.horizontal(|ui| {
                    if button(ui, "importFarApartShift", "Shift Alongside").clicked() {
                        *answer = Some(FarApartAnswer::ShiftAlongside);
                    }
                    if button(ui, "importFarApartKeep", "Keep Survey Coordinates").clicked() {
                        *answer = Some(FarApartAnswer::Keep);
                    }
                    if button(ui, "importFarApartCancel", "Cancel").clicked() {
                        *answer = Some(FarApartAnswer::Cancel);
                    }
                });
            },
        );
        if answer.is_none() && shown.closed {
            answer = Some(FarApartAnswer::Cancel);
        }
        answer
    }
}

/// What the window does about where data it has read lands.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlacementDecision {
    pub cancelled: bool,
    /// The origin shift to read the data again with; None keeps it where it is.
    pub shift: Option<Vec2>,
    /// Set when the window must ask before going on (the data lands far from the drawing and
    /// somebody is there to answer): the advice to ask with (`FarApartBox`), the answer then given
    /// to `decide_far_apart`.
    pub ask: Option<String>,
}

/// The one place the window's importers decide where data lands (decideImportPlacement). Any
/// placement but Keep is cad's resolve_import_shift, whose sentence is logged. Keep asks, when the
/// data lands far from the drawing (interop's advise_placement) - and in a headless session, where
/// nobody can answer, logs the advice and keeps the coordinates.
pub fn decide_import_placement(
    headless: bool,
    placement: &ImportPlacement,
    drawing: &Box2,
    incoming: &Box2,
    log: &mut dyn FnMut(&str, bool),
) -> PlacementDecision {
    let mut decision = PlacementDecision::default();
    if placement.mode != ImportPlacementMode::Keep {
        let resolved = resolve_import_shift(placement, drawing, incoming);
        decision.shift = resolved.shift;
        log(&resolved.said, false);
        return decision;
    }
    // Survey data in a projected CRS carries coordinates like (255440, 7410850) while a drawing
    // started from scratch sits near the origin. Merging them succeeds and leaves one of the two a
    // dot smaller than a pixel, so the choice is put BEFORE anything is added rather than left to
    // be found by zooming to extents.
    let advice = advise_placement(drawing, incoming);
    if !advice.far_apart {
        return decision;
    }
    if headless {
        log(&format!("{} (kept: no one to ask).", advice.message), true);
        return decision;
    }
    decision.ask = Some(advice.message);
    decision
}

/// The decision finished with the person's `answer` to the question asked with `advice`.
pub fn decide_far_apart(
    answer: FarApartAnswer,
    advice: &str,
    drawing: &Box2,
    incoming: &Box2,
    log: &mut dyn FnMut(&str, bool),
) -> PlacementDecision {
    let mut decision = PlacementDecision::default();
    match answer {
        FarApartAnswer::Cancel => {
            decision.cancelled = true;
        }
        FarApartAnswer::ShiftAlongside => {
            let resolved = resolve_import_shift(
                &ImportPlacement {
                    mode: ImportPlacementMode::Alongside,
                    offset: Vec2::default(),
                },
                drawing,
                incoming,
            );
            decision.shift = resolved.shift;
            log(&resolved.said, false);
        }
        FarApartAnswer::Keep => log(&format!("{advice}."), true),
    }
    decision
}
