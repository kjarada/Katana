//! What the terrain dialogs share (geo/terrain_dialog_support.hpp; docs/terrain.md;
//! docs/geoprocessing.md T0 to T3): Terrain > Surface From, GIS > Export Surface as DEM, Terrain >
//! Analysis > Contours, Terrain Shading, and Slope and Aspect.
//!
//! Each dialog writes the line its fields describe - shown, as it will run, in its <d>Command
//! field - and Run hands that line to the window's one executor (`Host::run`), exactly as if it had
//! been typed. The geoprocessing executor runs it as a background job: headless the line waits for
//! its job and the reply comes straight back; interactively it answers `job id=<n> ...
//! state=started`, and the reply is shown when the job ends (`Host::await_job`). So there is
//! nothing a dialog does that an agent cannot do by typing the same line.
//!
//! The C++'s TerrainDialogContext - the executor, whether the session is headless, the reference
//! data, the surfaces, the drawing, the views, the jobs' listener and the point picker - is the
//! `Host` a dialog is drawn with (the picker is `point_pick::PointPick` over the host's views), so
//! it, terrainDialogContext and showTerrainDialog have no Rust of their own.

use std::cell::RefCell;
use std::rc::Rc;

use egui::{Response, Ui};
use katana_app::geo::replies;
use katana_core::text::{parse_finite_double, parse_integer};
use katana_interop::ReferenceData;
use katana_terrain::SurfaceStore;

use super::geo_workbench::started_job;
use crate::drive::{Choice, LineOptions, TextOptions, choice, line_with, text_with};
use crate::host::{Host, JobId, NO_JOB, VerbOutcome};

/// What a source picker offers: a surface, a reference raster or a point cloud, and the words a
/// line names it by ("SURFACE ground", "RASTER 3", "CLOUD 2").
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TerrainSourceChoice {
    /// "ground (surface)", "terrain.asc (raster 3)".
    pub label: String,
    pub words: String,
}

/// The kinds of source a picker lists, as bits to combine (the C++'s TerrainSourceKinds).
pub struct TerrainSourceKinds;

impl TerrainSourceKinds {
    pub const SURFACES: u32 = 1;
    pub const RASTERS: u32 = 2;
    pub const CLOUDS: u32 = 4;
}

/// The sources of `kinds` the drawing's surfaces and the reference data hold; either may be None
/// (a test's), and lists nothing then. A surface whose name no line can carry is left out.
pub fn terrain_sources(
    surfaces: Option<&SurfaceStore>,
    reference: Option<&ReferenceData>,
    kinds: u32,
) -> Vec<TerrainSourceChoice> {
    let mut choices = Vec::new();
    if kinds & TerrainSourceKinds::SURFACES != 0
        && let Some(surfaces) = surfaces
    {
        for surface in surfaces.all() {
            let name = line_word(&surface.name).unwrap_or_default();
            if !name.is_empty() {
                choices.push(TerrainSourceChoice {
                    label: format!("{} (surface)", surface.name),
                    words: format!("SURFACE {name}"),
                });
            }
        }
    }
    if kinds & TerrainSourceKinds::RASTERS != 0
        && let Some(reference) = reference
    {
        // By id: two imports of one file have one name.
        for raster in reference.rasters() {
            choices.push(TerrainSourceChoice {
                label: format!("{} (raster {})", raster.name, raster.id),
                words: format!("RASTER {}", raster.id),
            });
        }
    }
    if kinds & TerrainSourceKinds::CLOUDS != 0
        && let Some(reference) = reference
    {
        for cloud in reference.point_clouds() {
            choices.push(TerrainSourceChoice {
                label: format!("{} (point cloud {})", cloud.name, cloud.id),
                words: format!("CLOUD {}", cloud.id),
            });
        }
    }
    choices
}

/// A source picker: the choices it lists and the one chosen (a QComboBox whose items carry their
/// words as data).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SourcePicker {
    pub choices: Vec<TerrainSourceChoice>,
    pub chosen: Choice,
}

impl SourcePicker {
    /// The words of the source chosen; None when none is.
    pub fn words(&self) -> Option<&str> {
        self.chosen
            .index
            .and_then(|at| self.choices.get(at))
            .map(|choice| choice.words.as_str())
    }

    /// The picker drawn as `name`.
    pub fn ui(&mut self, ui: &mut Ui, name: &str) -> Response {
        let labels: Vec<String> = self
            .choices
            .iter()
            .map(|choice| choice.label.clone())
            .collect();
        choice(ui, name, &labels, &mut self.chosen)
    }
}

/// Refills `picker` with `choices`, keeping the one chosen by its words when it is still there
/// (fillSources: a QComboBox's currentData found again).
pub fn fill_sources(picker: &mut SourcePicker, choices: Vec<TerrainSourceChoice>) {
    let kept = picker.words().map(String::from);
    picker.choices = choices;
    picker.chosen.index = None;
    if let Some(kept) = kept
        && let Some(again) = picker
            .choices
            .iter()
            .position(|choice| choice.words == kept)
    {
        picker.chosen.index = Some(again);
    }
    // A QComboBox given items with none chosen shows its first.
    if picker.chosen.index.is_none() && !picker.choices.is_empty() {
        picker.chosen.index = Some(0);
    }
}

/// A word of a line: as it is, or double-quoted when it holds a blank (geo::replies::line_word, the
/// one quoting rule: a keyword as a name is quoted too). None when it holds a double quote, which
/// no line can say.
pub fn line_word(text: &str) -> Option<String> {
    replies::line_word(text)
}

/// A list of `count` numbers separated by commas, as a line writes them ("0,0,40,30"; 0: any
/// count); each must read as a number, whole when `whole`, locale-independently.
pub fn number_list(text: &str, count: usize, whole: bool) -> bool {
    let items: Vec<&str> = text.split(',').collect();
    if count > 0 && items.len() != count {
        return false;
    }
    items.iter().all(|item| {
        let word = item.trim();
        if whole {
            parse_integer(word).is_some()
        } else {
            parse_finite_double(word).is_some()
        }
    })
}

/// The field every terrain dialog has for the line it will run: read-only, in a fixed-width font.
pub fn terrain_command_field(ui: &mut Ui, name: &str, text: &mut String) -> Response {
    line_with(
        ui,
        name,
        text,
        LineOptions {
            read_only: true,
            ..LineOptions::default()
        },
    )
}

/// The field every terrain dialog has for what the line said, in a fixed-width font.
pub fn terrain_reply_field(ui: &mut Ui, name: &str, text: &mut String) -> Response {
    text_with(
        ui,
        name,
        text,
        TextOptions {
            read_only: true,
            rows: 6,
            monospace: true,
        },
    )
}

/// Runs a dialog's line and shows what it said in its reply: at once when it answered (a refusal, a
/// PREVIEW, a headless run), or when its job ends - "Running ..." until then. The reply is the
/// run's to keep; the dialog draws it (`terrain_reply_field`).
pub struct TerrainRun {
    reply: String,
    waiting: JobId,
    heard: Rc<RefCell<Option<VerbOutcome>>>,
}

impl Default for TerrainRun {
    fn default() -> Self {
        Self::new()
    }
}

impl TerrainRun {
    pub fn new() -> Self {
        Self {
            reply: String::new(),
            waiting: NO_JOB,
            heard: Rc::new(RefCell::new(None)),
        }
    }

    /// The outcome the executor gave back at once. A line that started a job the host will tell
    /// the end of waits for it, its reply saying so; otherwise what it said is shown now.
    pub fn run(&mut self, host: &mut dyn Host, line: &str) -> VerbOutcome {
        let outcome = host.run(line);
        let job = if outcome.ok {
            started_job(&outcome.reply).unwrap_or(NO_JOB)
        } else {
            NO_JOB
        };
        if job != NO_JOB {
            let heard = Rc::clone(&self.heard);
            *heard.borrow_mut() = None;
            let listening = host.await_job(
                &outcome,
                Box::new(move |done: &VerbOutcome, _host: &mut dyn Host| {
                    *heard.borrow_mut() = Some(done.clone());
                }),
            );
            if listening {
                self.waiting = job;
                self.reply = format!(
                    "{}\nRunning - the result is shown here when the job ends, and in the \
                     command log.",
                    outcome.reply
                );
                return outcome;
            }
        }
        self.show(&outcome);
        outcome
    }

    /// What the line said in the end, when its job's end has been heard since the last call: shown,
    /// and answered (the C++'s `done`).
    pub fn poll(&mut self) -> Option<VerbOutcome> {
        if self.waiting == NO_JOB {
            return None;
        }
        let heard = self.heard.borrow_mut().take();
        let outcome = heard?;
        self.waiting = NO_JOB;
        self.show(&outcome);
        Some(outcome)
    }

    /// Whether a job's end is awaited.
    pub fn waiting(&self) -> bool {
        self.waiting != NO_JOB
    }

    fn show(&mut self, outcome: &VerbOutcome) {
        let mut text = outcome.reply.clone();
        if !outcome.error.is_empty() {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&outcome.error);
        }
        self.reply = text;
    }

    pub fn reply(&self) -> &str {
        &self.reply
    }

    pub fn reply_mut(&mut self) -> &mut String {
        &mut self.reply
    }
}
