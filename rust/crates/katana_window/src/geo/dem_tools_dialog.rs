//! Terrain > DEM Tools (geo/dem_tools_dialog.hpp; docs/terrain.md, "The DEM tools"): the window
//! onto the RASTER MOSAIC, CLIP, FILL, REPROJECT, FOOTPRINT and DIFFERENCE verbs, a tab each.
//!
//! No tab does the work. Each writes the line its fields describe - shown, as it will run, in its
//! Command field - and Run hands that line to the window's one executor as if it had been typed
//! (geo_dialog_support's GeoRunPanel). The line is made by a pure function of the fields, tested
//! without a window.
//!
//! ```text
//! demToolsDialog:
//!   demToolsTabs   Mosaic | Clip | Fill | Reproject | Footprint | Difference
//!   each tab <p> = demMosaic, demClip, demFill, demReproject, demFootprint, demDifference:
//!     <p>Kind, <p>Raster, <p>Surface, <p>Cell, <p>File, <p>Browse   the raster (BindingPicker)
//!     <p>Options   the tool's own fields
//!     <p>Name      the result's name (not Footprint)
//!     <p>Command, <p>Preview, <p>Run, <p>Status, <p>Reply           (GeoRunPanel)
//!   demMosaicTiles, demMosaicAdd, demMosaicRemove, demMosaicResolution, demMosaicSave
//!   demClipNote, demClipByArea, demClipArea, demClipByBoundaries, demClipBoundaryScope...
//!   demFillDistance, demFillSmoothing, demFillStrategy
//!   demReprojectCrs, demReprojectLike, demReprojectFrom, demReprojectResampling, demReprojectCell
//!   demFootprintLayer
//!   demDifferenceSecondKind ..., demDifferenceLimit, demDifferenceWithinScope...,
//!   demDifferenceResampling
//! ```

use egui::Ui;
use katana_core::Result;
use katana_core::text::parse_finite_double;
use katana_gis::processing as gp;

use super::binding_picker::{BindingKind, BindingList, BindingPicker};
use super::geo_dialog_support::{GeoRunPanel, line_word};
use super::terrain_dialog_parts::{Framed, field, host_views, invalid, row, show_framed};
use crate::customisation::{DocumentWatcher, ScopeChoice, ScopeFilterWidget};
use crate::drive::controls::custom;
use crate::drive::registry::{ControlKind, ControlRecord};
use crate::drive::{Choice, check, choice, label, radio, tabs};
use crate::host::Host;
use crate::shell::DialogFrame;
use crate::survey::survey_dialogs::offstage;

/// The tools, in the order of the tabs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DemTool {
    #[default]
    Mosaic,
    Clip,
    Fill,
    Reproject,
    Footprint,
    Difference,
}

pub const DEM_TOOL_COUNT: usize = 6;

const TOOLS: [DemTool; DEM_TOOL_COUNT] = [
    DemTool::Mosaic,
    DemTool::Clip,
    DemTool::Fill,
    DemTool::Reproject,
    DemTool::Footprint,
    DemTool::Difference,
];

const TAB_TITLES: [&str; DEM_TOOL_COUNT] = [
    "Mosaic",
    "Clip",
    "Fill",
    "Reproject",
    "Footprint",
    "Difference",
];

impl DemTool {
    fn index(self) -> usize {
        TOOLS.iter().position(|tool| *tool == self).unwrap_or(0)
    }

    /// The prefix of the tab's object names: demMosaic, demClip ...
    fn prefix(self) -> String {
        format!("dem{}", TAB_TITLES[self.index()])
    }
}

/// The verb's second word: "MOSAIC", "CLIP" ...
pub fn dem_tool_word(tool: DemTool) -> &'static str {
    match tool {
        DemTool::Mosaic => "MOSAIC",
        DemTool::Clip => "CLIP",
        DemTool::Fill => "FILL",
        DemTool::Reproject => "REPROJECT",
        DemTool::Footprint => "FOOTPRINT",
        DemTool::Difference => "DIFFERENCE",
    }
}

/// What one tab's fields hold, as typed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DemToolForm {
    pub tool: DemTool,
    /// The source clauses' words, in order: a mosaic's tiles, a difference's first and second; one
    /// for the rest.
    pub sources: Vec<String>,
    /// CLIP: AREA x0,y0,x1,y1 or scope words; DIFFERENCE: scope words or empty. `region_error`
    /// says why the controls give none.
    pub region: String,
    pub region_error: String,
    /// key=value options, as typed; a blank value is left out.
    pub options: Vec<(String, String)>,
    /// MOSAIC: SAVE <file>
    pub save: String,
    /// NAME
    pub name: String,
    /// FOOTPRINT: TO LAYER
    pub layer: String,
}

/// The options whose value must read as a number.
fn numeric(key: &str) -> bool {
    key == "distance" || key == "smoothing" || key == "cell"
}

/// GDAL's own choices for an argument, so the dialog offers what this GDAL takes; a blank first,
/// which leaves GDAL's default.
fn choices_of(path: &[&str], argument: &str) -> Vec<String> {
    let mut choices = vec![String::new()];
    let path: Vec<String> = path.iter().map(|word| String::from(*word)).collect();
    if let Ok(spec) = gp::describe(&path) {
        for arg in &spec.args {
            if arg.name == argument {
                choices.extend(arg.choices.iter().cloned());
            }
        }
    }
    choices
}

/// The line `form` describes, exactly as it would be typed:
///
/// ```text
/// RASTER MOSAIC <source>... [resolution=<r>] [SAVE <file>] [NAME <name>]
/// RASTER CLIP <source> <region> [NAME <name>]
/// RASTER FILL <source> [distance=<cells>] [smoothing=<n>] [strategy=<s>] [NAME <name>]
/// RASTER REPROJECT <source> [crs=<crs>] [like=<raster>] [from=<crs>]
///                  [resampling=<m>] [cell=<m>] [NAME <name>]
/// RASTER FOOTPRINT <source> [TO LAYER <path>]
/// RASTER DIFFERENCE <first> <second> [<scope>] [resampling=<m>] [NAME <name>]
/// ```
///
/// InvalidArgument naming the field for a source or region the controls cannot say, an option
/// value with a blank, an '=' or a double quote (an option's value is one word), and a number that
/// does not read where one is asked for (distance, smoothing, cell).
pub fn dem_tool_command_line(form: &DemToolForm) -> Result<String> {
    let mut line = format!("RASTER {}", dem_tool_word(form.tool));
    if form.sources.is_empty() {
        return Err(invalid("choose the raster first"));
    }
    for source in &form.sources {
        let source = source.trim();
        if source.is_empty() {
            return Err(invalid("choose the raster first"));
        }
        line.push_str(&format!(" {source}"));
    }
    if !form.region_error.is_empty() {
        return Err(invalid(form.region_error.as_str()));
    }
    let region = form.region.trim();
    if form.tool == DemTool::Clip && region.is_empty() {
        return Err(invalid(
            "choose what to clip to: a box, or the closed boundaries of a scope",
        ));
    }
    if !region.is_empty() {
        line.push_str(&format!(" {region}"));
    }
    for (key, text) in &form.options {
        let value = text.trim();
        if value.is_empty() {
            continue;
        }
        if value.chars().any(char::is_whitespace) || value.contains('=') || value.contains('"') {
            return Err(invalid(format!(
                "{key} is one word with no blank, '=' or double quote: '{value}'"
            )));
        }
        if numeric(key) && parse_finite_double(value).is_none() {
            return Err(invalid(format!(
                "{key} must be a number, not '{value}'"
            )));
        }
        line.push_str(&format!(" {key}={value}"));
    }
    let save = form.save.trim();
    if !save.is_empty() {
        line.push_str(&format!(
            " SAVE {}",
            line_word(save, "the file SAVE writes")?
        ));
    }
    let name = form.name.trim();
    if !name.is_empty() {
        line.push_str(&format!(" NAME {}", line_word(name, "the name")?));
    }
    let layer = form.layer.trim();
    if !layer.is_empty() {
        line.push_str(&format!(" TO LAYER {}", line_word(layer, "the layer")?));
    }
    Ok(line)
}

fn option(key: &str, value: &str) -> (String, String) {
    (String::from(key), String::from(value))
}

/// Terrain > DEM Tools (see the module comment).
pub struct DemToolsDialog {
    // First, so it goes first: no delivery reaches a half-dropped dialog.
    watcher: DocumentWatcher,
    frame: DialogFrame,
    visible: bool,
    tab: usize,
    /// A mosaic's tiles: rasters or files, several (a surface is no tile).
    mosaic_tiles: BindingList,
    /// The one raster every other tool reads, in the tabs' order from Clip.
    sources: Vec<BindingPicker>,
    panels: Vec<GeoRunPanel>,
    names: Vec<String>,
    resolutions: Vec<String>,
    resolution: Choice,
    save: String,
    clip_by_area: bool,
    clip_area: String,
    clip_boundaries: ScopeFilterWidget,
    fill_distance: String,
    fill_smoothing: String,
    fill_strategies: Vec<String>,
    fill_strategy: Choice,
    reproject_crs: String,
    /// Each raster REPROJECT can align to: its id as a line writes it ("" for none) and what the
    /// choice shows.
    like_ids: Vec<String>,
    like_texts: Vec<String>,
    reproject_like: Choice,
    reproject_from: String,
    resamplings: Vec<String>,
    reproject_resampling: Choice,
    reproject_cell: String,
    footprint_layer: String,
    difference_second: BindingPicker,
    difference_limit: bool,
    difference_within: ScopeFilterWidget,
    difference_resampling: Choice,
    /// A run ended this frame: what it made is offered to the other tabs at once.
    reload_wanted: bool,
}

impl DemToolsDialog {
    /// The dialog over the window's reference rasters, surfaces, drawing and views.
    pub fn new(host: &mut dyn Host) -> Self {
        let rasters = BindingKind::RASTER | BindingKind::SURFACE | BindingKind::FILE;
        let mosaic_tiles = BindingList::new(
            &DemTool::Mosaic.prefix(),
            BindingKind::RASTER | BindingKind::FILE,
            "Tiles",
            host,
        );
        let sources = TOOLS[1..]
            .iter()
            .map(|tool| BindingPicker::new(&tool.prefix(), rasters, host))
            .collect();
        let mut clip_boundaries = ScopeFilterWidget::new("demClipBoundary");
        clip_boundaries.views = host_views(host);
        clip_boundaries.set_choice(ScopeChoice::Selection);
        let mut difference_within = ScopeFilterWidget::new("demDifferenceWithin");
        difference_within.views = host_views(host);
        difference_within.set_choice(ScopeChoice::Selection);
        let resamplings = choices_of(&["raster", "reproject"], "resampling");
        let mut dialog = Self {
            watcher: DocumentWatcher::new(host.document()),
            frame: DialogFrame::new("demToolsDialog", "DEM Tools"),
            visible: false,
            tab: 0,
            mosaic_tiles,
            sources,
            panels: TOOLS
                .iter()
                .map(|tool| GeoRunPanel::new(&tool.prefix(), true))
                .collect(),
            names: vec![String::new(); DEM_TOOL_COUNT],
            // GDAL's words for raster mosaic --resolution (its documentation): or <x>,<y> typed.
            resolutions: ["", "same", "highest", "lowest", "average"]
                .iter()
                .map(|word| String::from(*word))
                .collect(),
            resolution: Choice {
                index: Some(0),
                text: String::new(),
                editable: true,
            },
            save: String::new(),
            clip_by_area: true,
            clip_area: String::new(),
            clip_boundaries,
            fill_distance: String::new(),
            fill_smoothing: String::new(),
            fill_strategies: choices_of(&["raster", "fill-nodata"], "strategy"),
            fill_strategy: Choice::at(0),
            reproject_crs: String::new(),
            like_ids: Vec::new(),
            like_texts: Vec::new(),
            reproject_like: Choice::at(0),
            reproject_from: String::new(),
            resamplings,
            reproject_resampling: Choice::at(0),
            reproject_cell: String::new(),
            footprint_layer: String::new(),
            difference_second: BindingPicker::new("demDifferenceSecond", rasters, host),
            difference_limit: false,
            difference_within,
            difference_resampling: Choice::at(0),
            reload_wanted: false,
        };
        dialog.reload(&*host);
        dialog
    }

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

    /// Brings `tool`'s tab to the front.
    pub fn show_tool(&mut self, tool: DemTool) {
        self.tab = tool.index();
    }

    pub fn tool(&self) -> DemTool {
        TOOLS[self.tab.min(DEM_TOOL_COUNT - 1)]
    }

    pub fn run_panel(&mut self, tool: DemTool) -> &mut GeoRunPanel {
        &mut self.panels[tool.index()]
    }

    /// The raster `tool` reads: a mosaic's is its tiles' picker.
    pub fn source(&mut self, tool: DemTool) -> &mut BindingPicker {
        match tool {
            DemTool::Mosaic => self.mosaic_tiles.picker(),
            _ => &mut self.sources[tool.index() - 1],
        }
    }

    fn source_words(&self, tool: DemTool) -> Result<String> {
        self.sources[tool.index() - 1].words()
    }

    /// A mosaic's tiles as its list shows them.
    pub fn mosaic_tiles(&self) -> &[String] {
        self.mosaic_tiles.listed()
    }

    /// The id REPROJECT's "onto the grid of" names; empty for none.
    fn like_id(&self) -> &str {
        self.reproject_like
            .index
            .and_then(|at| self.like_ids.get(at))
            .map_or("", String::as_str)
    }

    pub fn form(&self, tool: DemTool) -> DemToolForm {
        let mut made = DemToolForm {
            tool,
            ..DemToolForm::default()
        };
        let take = |made: &mut DemToolForm, words: Result<String>| match words {
            Ok(words) => made.sources.push(words),
            Err(error) => made.region_error = error.message,
        };
        if tool == DemTool::Mosaic {
            match self.mosaic_tiles.words() {
                Ok(tiles) => made.sources = tiles,
                Err(error) => made.region_error = error.message,
            }
        } else {
            take(&mut made, self.source_words(tool));
        }
        if tool != DemTool::Footprint {
            made.name.clone_from(&self.names[tool.index()]);
        }
        match tool {
            DemTool::Mosaic => {
                made.options = vec![option(
                    "resolution",
                    &self.resolution.current_text(&self.resolutions),
                )];
                made.save.clone_from(&self.save);
            }
            DemTool::Clip => {
                if self.clip_by_area {
                    let area = self.clip_area.trim();
                    let corners: Vec<&str> = area.split(',').collect();
                    if area.is_empty() {
                        if made.region_error.is_empty() {
                            made.region_error = String::from("give the box, x0,y0,x1,y1");
                        }
                    } else if corners.len() != 4
                        || !corners
                            .iter()
                            .all(|corner| parse_finite_double(corner.trim()).is_some())
                    {
                        made.region_error = String::from("the box is four numbers, x0,y0,x1,y1");
                    } else {
                        made.region = format!("AREA {area}");
                    }
                } else {
                    match self.clip_boundaries.verb_words() {
                        Ok(words) => made.region = words,
                        Err(error) => {
                            if made.region_error.is_empty() {
                                made.region_error =
                                    format!("the boundaries' scope: {}", error.message);
                            }
                        }
                    }
                }
            }
            DemTool::Fill => {
                made.options = vec![
                    option("distance", &self.fill_distance),
                    option("smoothing", &self.fill_smoothing),
                    option(
                        "strategy",
                        &self.fill_strategy.current_text(&self.fill_strategies),
                    ),
                ];
            }
            DemTool::Reproject => {
                // like= and crs= both say where the raster goes.
                let crs = if self.like_id().is_empty() {
                    self.reproject_crs.as_str()
                } else {
                    ""
                };
                made.options = vec![
                    option("crs", crs),
                    option("like", self.like_id()),
                    option("from", &self.reproject_from),
                    option(
                        "resampling",
                        &self.reproject_resampling.current_text(&self.resamplings),
                    ),
                    option("cell", &self.reproject_cell),
                ];
            }
            DemTool::Footprint => made.layer.clone_from(&self.footprint_layer),
            DemTool::Difference => {
                take(&mut made, self.difference_second.words());
                if self.difference_limit {
                    match self.difference_within.verb_words() {
                        Ok(words) => made.region = words,
                        Err(error) => {
                            if made.region_error.is_empty() {
                                made.region_error =
                                    format!("the boundaries' scope: {}", error.message);
                            }
                        }
                    }
                }
                made.options = vec![option(
                    "resampling",
                    &self.difference_resampling.current_text(&self.resamplings),
                )];
            }
        }
        made
    }

    /// Refills the pickers' rasters and surfaces, the scopes' layers and views, and the rasters
    /// demReprojectLike offers, keeping what is chosen.
    pub fn reload(&mut self, host: &dyn Host) {
        self.mosaic_tiles.reload(host);
        for picker in &mut self.sources {
            picker.reload(host);
        }
        self.difference_second.reload(host);
        let model = host.document().model();
        self.clip_boundaries.reload(model);
        self.difference_within.reload(model);
        let kept = String::from(self.like_id());
        self.like_ids = vec![String::new()];
        self.like_texts = vec![String::from("(none)")];
        for raster in host.reference().rasters() {
            self.like_ids.push(format!("{}", raster.id));
            self.like_texts
                .push(format!("{}  {}", raster.id, raster.name));
        }
        let index = self.like_ids.iter().position(|id| *id == kept).unwrap_or(0);
        self.reproject_like.index = Some(index);
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
        // A result may be a raster the other tabs can take next.
        if std::mem::take(&mut self.reload_wanted) {
            self.reload(&*host);
        }
    }

    /// The tool's own fields.
    fn options(&mut self, ui: &mut Ui, host: &dyn Host, tool: DemTool) {
        match tool {
            DemTool::Mosaic => {
                ui.label(
                    "Tiles joined into one DEM, kept as a reference raster reading them where \
                     they are (a VRT); SAVE writes the mosaic to a file of its own and keeps \
                     that. A folder is every raster in it, a pattern (tiles/*.tif) every file it \
                     matches.",
                );
                row(ui, "Resolution:", |ui| {
                    choice(
                        ui,
                        "demMosaicResolution",
                        &self.resolutions,
                        &mut self.resolution,
                    )
                    .on_hover_text(
                        "resolution=: the mosaic's cell when the tiles' differ - same (they must \
                         agree), highest, lowest, average, or x,y",
                    );
                });
                row(ui, "Save to:", |ui| {
                    field(
                        ui,
                        "demMosaicSave",
                        &mut self.save,
                        "keep it virtual (a VRT)",
                    )
                    .on_hover_text(
                        "SAVE: write the mosaic to this file and keep that file as the reference \
                         raster",
                    );
                });
            }
            DemTool::Clip => {
                // GDAL's raster clip keeps every cell a boundary touches, not only those whose
                // centre is inside (docs/terrain.md, "The DEM tools": 231 cells of a triangle
                // where the centre rule gives 210); this said "centre" until a hand count caught
                // it.
                label(
                    ui,
                    "demClipNote",
                    "The raster cut to a box, or to the closed boundaries a scope takes: every \
                     cell a boundary touches is kept, and the box's other cells hold no value.",
                );
                let by_area = self.clip_by_area;
                ui.horizontal(|ui| {
                    if radio(ui, "demClipByArea", "To a box:", by_area).clicked() {
                        self.clip_by_area = true;
                    }
                    ui.add_enabled_ui(by_area, |ui| {
                        field(ui, "demClipArea", &mut self.clip_area, "x0,y0,x1,y1")
                            .on_hover_text("AREA: the box, in the drawing's units");
                    });
                });
                if radio(
                    ui,
                    "demClipByBoundaries",
                    "To the closed boundaries of:",
                    !by_area,
                )
                .clicked()
                {
                    self.clip_by_area = false;
                }
                ui.add_enabled_ui(!by_area, |ui| {
                    self.clip_boundaries.ui(ui);
                });
            }
            DemTool::Fill => {
                ui.label(
                    "The raster's cells with no value filled by inverse-distance weighting from \
                     the cells round them; the reply counts what was empty and what was filled.",
                );
                row(ui, "Search distance (cells):", |ui| {
                    field(
                        ui,
                        "demFillDistance",
                        &mut self.fill_distance,
                        "GDAL's default, 100",
                    )
                    .on_hover_text("distance=: how far to look for values, in cells");
                });
                row(ui, "Smoothing passes:", |ui| {
                    field(ui, "demFillSmoothing", &mut self.fill_smoothing, "none").on_hover_text(
                        "smoothing=: passes of a 3 x 3 average over what was filled",
                    );
                });
                row(ui, "Strategy:", |ui| {
                    choice(
                        ui,
                        "demFillStrategy",
                        &self.fill_strategies,
                        &mut self.fill_strategy,
                    )
                    .on_hover_text("strategy=: GDAL's own choices; blank for its default");
                });
            }
            DemTool::Reproject => {
                ui.label(
                    "The raster into the project's coordinate system, another (an EPSG code), or \
                     another raster's grid. A raster that says nothing of where it is is \
                     refused: say it with From.",
                );
                let to_system = self.like_id().is_empty();
                row(ui, "To system:", |ui| {
                    ui.add_enabled_ui(to_system, |ui| {
                        field(
                            ui,
                            "demReprojectCrs",
                            &mut self.reproject_crs,
                            "the project's coordinate system",
                        )
                        .on_hover_text("crs=: EPSG:28356, or any system GDAL reads");
                    });
                });
                row(ui, "Or onto the grid of:", |ui| {
                    choice(
                        ui,
                        "demReprojectLike",
                        &self.like_texts,
                        &mut self.reproject_like,
                    )
                    .on_hover_text(
                        "like=: onto this reference raster's grid - its system, extent and cells",
                    );
                });
                row(ui, "From system:", |ui| {
                    field(
                        ui,
                        "demReprojectFrom",
                        &mut self.reproject_from,
                        "what the raster says",
                    )
                    .on_hover_text("from=: the raster's own system, when it does not say");
                });
                row(ui, "Resampling:", |ui| {
                    choice(
                        ui,
                        "demReprojectResampling",
                        &self.resamplings,
                        &mut self.reproject_resampling,
                    )
                    .on_hover_text(
                        "resampling=: GDAL's own choices; blank for its default (nearest)",
                    );
                });
                row(ui, "Cell:", |ui| {
                    field(
                        ui,
                        "demReprojectCell",
                        &mut self.reproject_cell,
                        "as GDAL chooses",
                    )
                    .on_hover_text("cell=: the result's cell, in the target system's units");
                });
            }
            DemTool::Footprint => {
                ui.label(
                    "Where the raster has data, drawn as closed polylines on a layer, as one undo \
                     step.",
                );
                row(ui, "Layer:", |ui| {
                    field(
                        ui,
                        "demFootprintLayer",
                        &mut self.footprint_layer,
                        "gis/footprint",
                    )
                    .on_hover_text("TO LAYER: the layer the footprint is drawn on");
                });
            }
            DemTool::Difference => {
                ui.label(
                    "The first minus the second, the second aligned to the first's grid where \
                     they differ: a raster of the difference, and the volumes of fill (the first \
                     above) and cut (below) by the grid method.",
                );
                ui.group(|ui| {
                    ui.label("Second");
                    self.difference_second.ui(ui, host);
                });
                if let Some(text) = self.difference_second.take_said() {
                    self.panels[tool.index()].set_status(&text, true);
                }
                check(
                    ui,
                    "demDifferenceLimit",
                    "Only within the closed boundaries of:",
                    &mut self.difference_limit,
                );
                let limit = self.difference_limit;
                ui.add_enabled_ui(limit, |ui| {
                    self.difference_within.ui(ui);
                });
                row(ui, "Alignment:", |ui| {
                    choice(
                        ui,
                        "demDifferenceResampling",
                        &self.resamplings,
                        &mut self.difference_resampling,
                    )
                    .on_hover_text(
                        "resampling=: how the second is aligned when the grids differ; blank for \
                         bilinear",
                    );
                });
            }
        }
        if tool != DemTool::Footprint {
            let name = format!("{}Name", tool.prefix());
            let placeholder = format!("default: {}", dem_tool_word(tool).to_lowercase());
            row(ui, "Name:", |ui| {
                field(ui, &name, &mut self.names[tool.index()], &placeholder).on_hover_text(
                    "NAME: the reference raster's name; a name taken becomes <name>-2",
                );
            });
        }
    }

    /// One tool's tab: its raster, its options, and its Command, Run and Reply.
    fn page(&mut self, ui: &mut Ui, host: &mut dyn Host, tool: DemTool) {
        let prefix = tool.prefix();
        let page = ui.vertical(|ui| {
            ui.horizontal_top(|ui| {
                ui.group(|ui| {
                    ui.vertical(|ui| {
                        ui.label(if tool == DemTool::Difference {
                            "First (minus the second)"
                        } else {
                            "Raster"
                        });
                        if tool == DemTool::Mosaic {
                            self.mosaic_tiles.ui(ui, &*host);
                        } else {
                            self.sources[tool.index() - 1].ui(ui, &*host);
                        }
                    });
                });
                let options = ui.group(|ui| {
                    ui.vertical(|ui| self.options(ui, &*host, tool));
                });
                custom(
                    ui,
                    &options.response,
                    ControlRecord::new(&format!("{prefix}Options"), ControlKind::Other),
                );
            });
            if let Some(text) = self.source(tool).take_said() {
                self.panels[tool.index()].set_status(&text, true);
            }
            let made = dem_tool_command_line(&self.form(tool));
            if self.panels[tool.index()].ui(ui, host, &made).is_some() {
                self.reload_wanted = true;
            }
        });
        custom(
            ui,
            &page.response,
            ControlRecord::new(
                &format!("demTools{}", TAB_TITLES[tool.index()]),
                ControlKind::Other,
            ),
        );
    }
}

impl Framed for DemToolsDialog {
    fn frame_mut(&mut self) -> &mut DialogFrame {
        &mut self.frame
    }

    // Each tab holds its own Run and Reply, as the C++ pages do, so the dialog has no footer of
    // its own and everything scrolls together.
    fn body(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        tabs(ui, "demToolsTabs", &TAB_TITLES, &mut self.tab);
        // The pages not in front are still there, as a QTabWidget's are: what they hold can be
        // filled, pressed and read.
        let current = self.tool();
        for tool in TOOLS {
            if tool == current {
                self.page(ui, host, tool);
            } else {
                let salt = format!("demToolsPage{}", tool.index());
                offstage(ui, &salt, |ui| self.page(ui, host, tool));
            }
        }
    }

    fn footer(&mut self, _ui: &mut Ui, _host: &mut dyn Host) {}
}
