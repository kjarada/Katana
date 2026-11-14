//! Terrain > Surface From and GIS > Export Surface as DEM (surface_raster_dialog.hpp;
//! docs/terrain.md, "Surfaces from data" and "Export as DEM"): the windows onto the SURFACE FROM and
//! SURFACE EXPORT verbs.
//!
//! Neither dialog does the work. Each writes the line its fields describe - shown, as it will run,
//! in its Command field - and its button hands it to the window's one executor as if it had been
//! typed (terrain_dialog_support): the surface or the DEM is the verb's background job, and its
//! reply comes back into the dialog's Reply. The lines are made by pure functions of the fields,
//! tested without a window.
//!
//! ```text
//! surfaceFromDialog:
//!   surfaceFromKind     Point cloud, Raster or Drawing
//!   surfaceFromSource   the point cloud or reference raster
//!   surfaceFromClasses  a cloud's classes, "2,8"; blank: its ground
//!   surfaceFromMax      the most points taken
//!   surfaceFromArea     a raster's window, x0,y0,x1,y1
//!   surfaceFromScope    the drawing's scope and filter (ScopeFilterWidget)
//!   surfaceFromName     the surface's name; blank: the source's
//!   surfaceFromCommand, surfaceFromPreview, surfaceFromRun, surfaceFromReply
//!
//! surfaceRasterDialog:
//!   surfaceRasterSource     the surface
//!   surfaceRasterCell       the cell size
//!   surfaceRasterGrid       the grid that cell makes of it
//!   surfaceRasterType       Float32 or Float64
//!   surfaceRasterCog        a Cloud Optimised GeoTIFF
//!   surfaceRasterFile, surfaceRasterBrowse, surfaceRasterOverwrite
//!   surfaceRasterCommand, surfaceRasterPreview, surfaceRasterRun, surfaceRasterReply
//! ```

use std::path::Path;

use egui::{RichText, Ui};
use katana_core::Result;
use katana_core::text::format_exact_real;
use katana_interop::SurfaceRasterOptions;
use katana_interop::export::raster_export_formats;
use katana_interop::terrain_io::suggested_cell_size;

use crate::customisation::{ScopeChoice, ScopeFilterWidget};
use crate::drive::{Choice, SpinRange, button, check, choice, label, spin};
use crate::format::grouped;
use crate::geo::terrain_dialog_parts::{
    Framed, WordChoice, command_field, field, host_views, invalid, row, run_buttons, show_framed,
    without_blanks,
};
use crate::geo::terrain_dialog_support::{
    SourcePicker, TerrainRun, TerrainSourceKinds, fill_sources, line_word, number_list,
    terrain_reply_field, terrain_sources,
};
use crate::host::Host;
use crate::ifc_dialogs::{from_native_separators, get_save_file_name};
use crate::shell::DialogFrame;
use crate::theme::palette;

// ---- Surface From -------------------------------------------------------------------------------

/// What a surface is built from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SurfaceFromKind {
    Cloud,
    Raster,
    #[default]
    Drawing,
}

/// The fields of Surface From, as its line is written from them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SurfaceFromForm {
    pub kind: SurfaceFromKind,
    /// CLOUD <id> or RASTER <id>, as the source picker's data says it.
    pub source: String,
    pub classes: String,
    pub max_points: String,
    pub area: String,
    /// The drawing's scope and filter words (ScopeFilterWidget::verb_words), or - in `scope_error`
    /// - why the controls say none.
    pub scope: String,
    pub scope_error: String,
    pub name: String,
}

/// ```text
/// SURFACE FROM CLOUD <id> [classes=a,b] [max=<n>] [NAME <n>]
/// SURFACE FROM RASTER <id> [max=<n>] [AREA x0,y0,x1,y1] [NAME <n>]
/// SURFACE FROM <scope> [NAME <n>]
/// ```
///
/// InvalidArgument naming the field: no source chosen, a scope the controls cannot say, classes
/// that are not whole numbers, a cap that is not a whole number, an area that is not four numbers,
/// a name with a double quote.
pub fn surface_from_line(form: &SurfaceFromForm) -> Result<String> {
    let mut line = String::from("SURFACE FROM ");
    if form.kind == SurfaceFromKind::Drawing {
        if !form.scope_error.is_empty() {
            return Err(invalid(format!("Apply to: {}", form.scope_error)));
        }
        let scope = form.scope.trim();
        line.push_str(if scope.is_empty() { "DRAWING" } else { scope });
    } else {
        let source = form.source.trim();
        if source.is_empty() {
            return Err(invalid(if form.kind == SurfaceFromKind::Cloud {
                "Source: there is no point cloud to build a surface from"
            } else {
                "Source: there is no raster to build a surface from"
            }));
        }
        line.push_str(source);
        if form.kind == SurfaceFromKind::Cloud && !form.classes.trim().is_empty() {
            let classes = without_blanks(&form.classes);
            if !number_list(&classes, 0, true) {
                return Err(invalid(
                    "Classes: whole numbers separated by commas, as 2,8",
                ));
            }
            line.push_str(&format!(" classes={classes}"));
        }
        let most = form.max_points.trim();
        if !most.is_empty() {
            if !number_list(most, 1, true) {
                return Err(invalid("Most points: a whole number"));
            }
            line.push_str(&format!(" max={most}"));
        }
        if form.kind == SurfaceFromKind::Raster && !form.area.trim().is_empty() {
            let area = without_blanks(&form.area);
            if !number_list(&area, 4, false) {
                return Err(invalid("Window: four numbers x0,y0,x1,y1"));
            }
            line.push_str(&format!(" AREA {area}"));
        }
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

/// Terrain > Surface From (see the module comment).
pub struct SurfaceFromDialog {
    frame: DialogFrame,
    visible: bool,
    kind: WordChoice,
    source: SourcePicker,
    classes: String,
    max_points: String,
    area: String,
    scope: ScopeFilterWidget,
    name: String,
    runner: TerrainRun,
}

fn kind_word(kind: SurfaceFromKind) -> &'static str {
    match kind {
        SurfaceFromKind::Cloud => "cloud",
        SurfaceFromKind::Raster => "raster",
        SurfaceFromKind::Drawing => "drawing",
    }
}

impl SurfaceFromDialog {
    /// The dialog over the window's reference data, drawing and views.
    pub fn new(host: &mut dyn Host) -> Self {
        let mut scope = ScopeFilterWidget::new("surfaceFrom");
        scope.views = host_views(host);
        // What the window's Surface From Drawing always took: everything drawn.
        scope.set_choice(ScopeChoice::Drawing);
        scope.set_drawn_only(true);
        let mut dialog = Self {
            frame: DialogFrame::new("surfaceFromDialog", "Surface From"),
            visible: false,
            kind: WordChoice::new(&[
                ("Point cloud", "cloud"),
                ("Raster", "raster"),
                ("Drawing", "drawing"),
            ]),
            source: SourcePicker::default(),
            classes: String::new(),
            max_points: String::new(),
            area: String::new(),
            scope,
            name: String::new(),
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

    /// The kind chosen.
    pub fn kind(&self) -> SurfaceFromKind {
        match self.kind.word() {
            "cloud" => SurfaceFromKind::Cloud,
            "raster" => SurfaceFromKind::Raster,
            _ => SurfaceFromKind::Drawing,
        }
    }

    /// Brings `kind` forward, and the source `words` ("RASTER 3") when given.
    pub fn show_kind(&mut self, host: &dyn Host, kind: SurfaceFromKind, words: &str) {
        let _ = self.kind.set_word(kind_word(kind));
        self.reload(host);
        if !words.is_empty()
            && let Some(found) = self
                .source
                .choices
                .iter()
                .position(|listed| listed.words == words)
        {
            self.source.chosen.index = Some(found);
        }
    }

    /// Refills the sources and the scope's layers and views, keeping what is chosen: done when
    /// shown.
    pub fn reload(&mut self, host: &dyn Host) {
        let kinds = if self.kind() == SurfaceFromKind::Cloud {
            TerrainSourceKinds::CLOUDS
        } else {
            TerrainSourceKinds::RASTERS
        };
        fill_sources(
            &mut self.source,
            terrain_sources(
                Some(host.document().surfaces()),
                Some(host.reference()),
                kinds,
            ),
        );
        self.scope.reload(host.document().model());
    }

    pub fn form(&self) -> SurfaceFromForm {
        let (scope, scope_error) = match self.scope.verb_words() {
            Ok(words) => (words, String::new()),
            Err(error) => (String::new(), error.message),
        };
        SurfaceFromForm {
            kind: self.kind(),
            source: String::from(self.source.words().unwrap_or("")),
            classes: self.classes.clone(),
            max_points: self.max_points.clone(),
            area: self.area.clone(),
            scope,
            scope_error,
            name: self.name.clone(),
        }
    }

    pub fn command(&self) -> Result<String> {
        surface_from_line(&self.form())
    }

    pub fn scope_controls(&mut self) -> &mut ScopeFilterWidget {
        &mut self.scope
    }

    /// What it said last (surfaceFromReply).
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

impl Framed for SurfaceFromDialog {
    fn frame_mut(&mut self) -> &mut DialogFrame {
        &mut self.frame
    }

    fn body(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        let kind_changed = row(ui, "From:", |ui| {
            self.kind.ui(ui, "surfaceFromKind").changed()
        });
        if kind_changed {
            self.reload(&*host);
        }
        let kind = self.kind();
        row(ui, "Source:", |ui| {
            ui.add_enabled_ui(kind != SurfaceFromKind::Drawing, |ui| {
                self.source.ui(ui, "surfaceFromSource");
            });
        });
        row(ui, "Classes:", |ui| {
            ui.add_enabled_ui(kind == SurfaceFromKind::Cloud, |ui| {
                field(
                    ui,
                    "surfaceFromClasses",
                    &mut self.classes,
                    "its ground (class 2), or every return when it has none",
                );
            });
        });
        row(ui, "Most points:", |ui| {
            ui.add_enabled_ui(kind != SurfaceFromKind::Drawing, |ui| {
                field(ui, "surfaceFromMax", &mut self.max_points, "400000");
            });
        });
        row(ui, "Window:", |ui| {
            ui.add_enabled_ui(kind == SurfaceFromKind::Raster, |ui| {
                field(
                    ui,
                    "surfaceFromArea",
                    &mut self.area,
                    "x0,y0,x1,y1 - blank: the whole raster",
                );
            });
        });
        ui.add_enabled_ui(kind == SurfaceFromKind::Drawing, |ui| {
            self.scope.ui(ui);
        });
        row(ui, "Name:", |ui| {
            field(ui, "surfaceFromName", &mut self.name, "the source's name");
        });
        command_field(ui, "surfaceFromCommand", &self.command());
    }

    fn footer(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        let _ = self.runner.poll();
        let ready = self.command().is_ok();
        let (preview, run) = run_buttons(
            ui,
            Some(("surfaceFromPreview", ready)),
            ("surfaceFromRun", "Build Surface", ready),
        );
        terrain_reply_field(ui, "surfaceFromReply", self.runner.reply_mut());
        if preview {
            self.run(host, true);
        }
        if run {
            self.run(host, false);
        }
    }
}

// ---- Export Surface as DEM ----------------------------------------------------------------------

/// The fields of Export Surface as DEM, as its line is written from them.
#[derive(Clone, Debug, PartialEq)]
pub struct SurfaceExportForm {
    pub surface: String,
    pub file: String,
    pub cell: f64,
    /// Float32 or Float64.
    pub data_type: String,
    pub cog: bool,
    pub overwrite: bool,
}

impl Default for SurfaceExportForm {
    fn default() -> Self {
        Self {
            surface: String::new(),
            file: String::new(),
            cell: 0.0,
            data_type: String::from("Float32"),
            cog: false,
            overwrite: false,
        }
    }
}

/// ```text
/// SURFACE EXPORT <surface> <file> cell=<m> type=<Float32|Float64> [COG] [OVERWRITE]
/// ```
///
/// InvalidArgument naming the field: no surface, no file, a cell that is not positive, a double
/// quote in a name or path.
pub fn surface_export_line(form: &SurfaceExportForm) -> Result<String> {
    let surface = form.surface.trim();
    if surface.is_empty() {
        return Err(invalid(
            "Surface: there is no surface to export; build one from the Terrain menu",
        ));
    }
    let file = form.file.trim();
    if file.is_empty() {
        return Err(invalid(
            "File: choose where the DEM goes (.tif, .asc or .img)",
        ));
    }
    if !(form.cell > 0.0) || !form.cell.is_finite() {
        return Err(invalid("Cell: a positive length"));
    }
    let Some(surface) = line_word(surface) else {
        return Err(invalid("Surface: a name cannot hold a double quote"));
    };
    let Some(file) = line_word(&from_native_separators(file)) else {
        return Err(invalid("File: a path cannot hold a double quote"));
    };
    let mut line = format!(
        "SURFACE EXPORT {surface} {file} cell={} type={}",
        format_exact_real(form.cell),
        form.data_type
    );
    if form.cog {
        line.push_str(" COG");
    }
    if form.overwrite {
        line.push_str(" OVERWRITE");
    }
    Ok(line)
}

/// The cell box: three places, a thousandth of a unit to a hundred thousand.
const CELL_RANGE: SpinRange = SpinRange::new(0.001, 100_000.0, 1.0, 3).with_suffix(" units");

/// The cells a suggested size aims for along the surface's longer side
/// (katana/interop/terrain_io.hpp, suggestedCellSize's default).
const SUGGESTED_CELLS: i32 = 1000;

/// GIS > Export Surface as DEM (see the module comment).
pub struct SurfaceRasterDialog {
    frame: DialogFrame,
    visible: bool,
    surfaces: Vec<String>,
    surface: Choice,
    cell_size: f64,
    types: Vec<String>,
    data_type: Choice,
    cog: bool,
    file: String,
    overwrite: bool,
    runner: TerrainRun,
}

impl SurfaceRasterDialog {
    /// The dialog over the window's surfaces.
    pub fn new(host: &dyn Host) -> Self {
        let mut dialog = Self {
            frame: DialogFrame::new("surfaceRasterDialog", "Export Surface as DEM"),
            visible: false,
            surfaces: Vec::new(),
            surface: Choice::default(),
            // A QDoubleSpinBox starts at 0, held to its range.
            cell_size: CELL_RANGE.minimum,
            types: vec![String::from("Float32"), String::from("Float64")],
            data_type: Choice::at(0),
            cog: false,
            file: String::new(),
            overwrite: false,
            runner: TerrainRun::new(),
        };
        dialog.reload(host);
        dialog
    }

    /// Shows the dialog, its surfaces read again (showEvent).
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

    fn surface_name(&self) -> String {
        self.surface.current_text(&self.surfaces)
    }

    /// Refills the surfaces, keeping the one chosen; a surface newly chosen is given its
    /// suggested cell.
    pub fn reload(&mut self, host: &dyn Host) {
        let kept = self.surface_name();
        self.surfaces = host
            .document()
            .surfaces()
            .all()
            .iter()
            .map(|named| named.name.clone())
            .collect();
        let again = self.surfaces.iter().position(|name| *name == kept);
        self.surface.index = again.or(if self.surfaces.is_empty() {
            None
        } else {
            Some(0)
        });
        if self.surface_name() != kept {
            self.suggest_cell(host);
        }
    }

    fn suggest_cell(&mut self, host: &dyn Host) {
        if let Some(named) = host.document().surfaces().find(&self.surface_name())
            && let Some(surface) = &named.surface
        {
            let suggested = suggested_cell_size(surface.bounds(), SUGGESTED_CELLS);
            self.cell_size = CELL_RANGE
                .rounded(suggested)
                .clamp(CELL_RANGE.minimum, CELL_RANGE.maximum);
        }
    }

    pub fn form(&self) -> SurfaceExportForm {
        SurfaceExportForm {
            surface: self.surface_name(),
            file: self.file.clone(),
            cell: self.cell_size,
            data_type: self.data_type.current_text(&self.types),
            cog: self.cog,
            overwrite: self.overwrite,
        }
    }

    pub fn command(&self) -> Result<String> {
        surface_export_line(&self.form())
    }

    /// The cell size the box shows.
    pub fn cell_size(&self) -> f64 {
        self.cell_size
    }

    /// What it said last (surfaceRasterReply).
    pub fn reply(&self) -> &str {
        self.runner.reply()
    }

    /// The grid the export will write, with the same arithmetic and the same ceiling as the
    /// export's own grid, so Export is refused here rather than failing after the job starts: what
    /// the Grid label says, and whether the grid is over the limit.
    fn grid(&self, host: &dyn Host) -> (String, bool) {
        let named = host.document().surfaces().find(&self.surface_name());
        let Some(surface) = named.and_then(|named| named.surface.as_ref()) else {
            return (String::from("no surface"), false);
        };
        let bounds = surface.bounds();
        let cell = self.cell_size;
        let columns = (bounds.width() / cell).ceil().max(1.0);
        let rows = (bounds.height() / cell).ceil().max(1.0);
        let limit = SurfaceRasterOptions::default().max_cells as f64;
        let over_limit = columns * rows > limit;
        let mut text = format!(
            "{} x {} cells",
            grouped(columns as u64),
            grouped(rows as u64)
        );
        if over_limit {
            text.push_str(&format!(
                " - over the {}-cell limit; use a larger cell",
                grouped(limit as u64)
            ));
        }
        (text, over_limit)
    }

    fn browse(&mut self, host: &dyn Host) {
        // A headless run has nobody to answer a file dialog: the field is filled by name instead
        // (--fill surfaceRasterFile=...).
        if host.headless() {
            *self.runner.reply_mut() = String::from(
                "A headless session opens no file dialog: fill surfaceRasterFile with the path \
                 instead.",
            );
            return;
        }
        let filters: Vec<String> = raster_export_formats()
            .iter()
            .map(|format| format!("{} (*.{})", format.description, format.extension))
            .collect();
        let chosen = get_save_file_name(
            "Export Surface as DEM",
            &format!("{}.tif", self.surface_name()),
            &filters.join(";;"),
        );
        if chosen.is_empty() {
            return;
        }
        // The save dialog asked before it gave back a file that is there.
        self.overwrite = Path::new(&chosen).exists();
        self.file = from_native_separators(&chosen);
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

impl Framed for SurfaceRasterDialog {
    fn frame_mut(&mut self) -> &mut DialogFrame {
        &mut self.frame
    }

    fn body(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        let surface_changed = row(ui, "Surface:", |ui| {
            choice(ui, "surfaceRasterSource", &self.surfaces, &mut self.surface).changed()
        });
        if surface_changed {
            self.suggest_cell(&*host);
        }
        row(ui, "Cell size:", |ui| {
            spin(ui, "surfaceRasterCell", &mut self.cell_size, CELL_RANGE);
        });
        let (grid, _) = self.grid(&*host);
        row(ui, "Grid:", |ui| {
            label(ui, "surfaceRasterGrid", &grid);
        });
        ui.label(
            RichText::new(
                "Each cell holds the surface's elevation at its centre; a cell whose centre is \
                 off the surface is no-data.",
            )
            .color(palette::text_muted().to_egui()),
        );
        row(ui, "Stored as:", |ui| {
            choice(ui, "surfaceRasterType", &self.types, &mut self.data_type);
        });
        check(
            ui,
            "surfaceRasterCog",
            "Cloud Optimised GeoTIFF",
            &mut self.cog,
        );
        let browse = row(ui, "File:", |ui| {
            field(
                ui,
                "surfaceRasterFile",
                &mut self.file,
                "dem.tif, dem.asc or dem.img",
            );
            button(ui, "surfaceRasterBrowse", "Browse...").clicked()
        });
        if browse {
            self.browse(&*host);
        }
        check(
            ui,
            "surfaceRasterOverwrite",
            "Replace a file already there",
            &mut self.overwrite,
        );
        command_field(ui, "surfaceRasterCommand", &self.command());
    }

    fn footer(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        let _ = self.runner.poll();
        let ready = self.command().is_ok();
        let (_, over_limit) = self.grid(&*host);
        let (preview, run) = run_buttons(
            ui,
            Some(("surfaceRasterPreview", ready)),
            ("surfaceRasterRun", "Export", ready && !over_limit),
        );
        terrain_reply_field(ui, "surfaceRasterReply", self.runner.reply_mut());
        if preview {
            self.run(host, true);
        }
        if run {
            self.run(host, false);
        }
    }
}
