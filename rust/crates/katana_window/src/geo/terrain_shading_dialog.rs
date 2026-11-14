//! Terrain > Analysis > Terrain Shading (geo/terrain_shading_dialog.hpp; docs/terrain.md,
//! "Shading"): the window onto the RASTER SHADE verb (katana_app::geo's shade verbs).
//!
//! The dialog renders nothing itself. It writes the RASTER SHADE line its fields describe - shown,
//! as it will run, in shadingCommand - and Run hands it to the window's one executor as if it had
//! been typed (terrain_dialog_support): the picture is the verb's background job, kept as a derived
//! reference raster, and the reply - with the legend - comes back into shadingReply. The line is
//! made by a pure function of the fields, tested without a window.
//!
//! ```text
//! terrainShadingDialog:
//!   shadingSource     the surface or reference raster
//!   shadingStyle      hillshade, relief, relief over hillshade, slope, plain
//!   shadingAzimuth    the light's direction (hillshades)
//!   shadingAltitude   the light's height (hillshades)
//!   shadingZFactor    the vertical exaggeration (hillshades)
//!   shadingVariant    regular, combined, multidirectional, igor (hillshades)
//!   shadingRamp       a ramp of the one table (terrain, earth, diverging, slope, grey,
//!                     spectral), or a colour-map file typed in (coloured styles)
//!   shadingRangeMin, shadingRangeMax   the values the ramp spans; blank: the data's
//!   shadingName       the reference raster's name; blank: the source's and the style's
//!   shadingSave       also write the picture to this GeoTIFF; shadingSaveBrowse,
//!                     shadingOverwrite
//!   shadingCommand    the exact line Run will run (read-only)
//!   shadingPreview    run it with PREVIEW: what would be read, nothing made
//!   shadingRun        run it
//!   shadingReply      what it said
//! ```

use std::path::Path;

use egui::Ui;
use katana_core::Result;
use katana_core::text::format_exact_real;
use katana_terrain::colour_ramp_names;

use super::terrain_dialog_parts::{
    Framed, WordChoice, command_field, field, invalid, number_of, row, run_buttons, show_framed,
};
use super::terrain_dialog_support::{
    SourcePicker, TerrainRun, TerrainSourceKinds, fill_sources, line_word, number_list,
    terrain_reply_field, terrain_sources,
};
use crate::drive::{Choice, SpinRange, button, check, choice, spin};
use crate::host::Host;
use crate::ifc_dialogs::{from_native_separators, get_save_file_name};
use crate::shell::DialogFrame;

/// The fields of the dialog, as the line is written from them.
#[derive(Clone, Debug, PartialEq)]
pub struct ShadingForm {
    /// SURFACE <name> or RASTER <id>, as the source picker's data says it.
    pub source: String,
    /// The verb's word: hillshade, relief, relief+hillshade, slope, plain.
    pub style: String,
    pub azimuth: f64,
    pub altitude: f64,
    pub z_factor: f64,
    pub variant: String,
    pub ramp: String,
    pub range_min: String,
    pub range_max: String,
    pub name: String,
    pub save: String,
    pub overwrite: bool,
}

impl Default for ShadingForm {
    fn default() -> Self {
        Self {
            source: String::new(),
            style: String::from("hillshade"),
            azimuth: 315.0,
            altitude: 45.0,
            z_factor: 1.0,
            variant: String::from("regular"),
            ramp: String::new(),
            range_min: String::new(),
            range_max: String::new(),
            name: String::new(),
            save: String::new(),
            overwrite: false,
        }
    }
}

fn lit(style: &str) -> bool {
    style == "hillshade" || style == "relief+hillshade"
}

/// The ramp a coloured style paints with unless another is chosen, as the verb has it; empty for a
/// hillshade, which is grey.
fn default_ramp(style: &str) -> &'static str {
    match style {
        "relief" | "relief+hillshade" => "terrain",
        "slope" => "slope",
        "plain" => "grey",
        _ => "",
    }
}

/// ```text
/// RASTER SHADE <source> style=<style> [azimuth=] [altitude=] [z=] [variant=]
///              [ramp=] [range=<min>,<max>] [NAME <n>] [save=<file>] [OVERWRITE]
/// ```
///
/// The light is written for a hillshade style only, and only where it is not GDAL's default (315,
/// 45, 1, regular) - never an azimuth for multidirectional, which lights from every side; a ramp
/// only for a coloured style and when it is not the style's own. InvalidArgument naming the field:
/// no source, one end of the range without the other or ends that are not numbers low before high,
/// a name, ramp or path with a double quote.
#[allow(
    clippy::float_cmp,
    reason = "A light left at GDAL's default is exactly the default the box was made with."
)]
pub fn shading_line(form: &ShadingForm) -> Result<String> {
    let source = form.source.trim();
    if source.is_empty() {
        return Err(invalid(
            "Source: there is no surface or raster to shade; build a surface or import a DEM \
             first",
        ));
    }
    let mut line = format!("RASTER SHADE {source} style={}", form.style);
    if lit(&form.style) {
        let every_side = form.variant == "multidirectional";
        if form.azimuth != 315.0 && !every_side {
            line.push_str(&format!(" azimuth={}", format_exact_real(form.azimuth)));
        }
        if form.altitude != 45.0 {
            line.push_str(&format!(" altitude={}", format_exact_real(form.altitude)));
        }
        if form.z_factor != 1.0 {
            line.push_str(&format!(" z={}", format_exact_real(form.z_factor)));
        }
        if form.variant != "regular" {
            line.push_str(&format!(" variant={}", form.variant));
        }
    }
    let ramp = form.ramp.trim();
    let own = default_ramp(&form.style);
    if !own.is_empty() {
        if !ramp.is_empty() && ramp.to_lowercase() != own {
            let Some(word) = line_word(&from_native_separators(ramp)) else {
                return Err(invalid(
                    "Ramp: a colour-map path cannot hold a double quote",
                ));
            };
            line.push_str(&format!(" ramp={word}"));
        }
        let low = form.range_min.trim();
        let high = form.range_max.trim();
        if low.is_empty() != high.is_empty() {
            return Err(invalid(
                "Range: both ends, or neither for the data's own",
            ));
        }
        if !low.is_empty() {
            if !number_list(low, 1, false)
                || !number_list(high, 1, false)
                || !(number_of(high) > number_of(low))
            {
                return Err(invalid(
                    "Range: two numbers, the low end before the high",
                ));
            }
            line.push_str(&format!(" range={low},{high}"));
        }
    }
    let name = form.name.trim();
    if !name.is_empty() {
        let Some(word) = line_word(name) else {
            return Err(invalid("Name: a name cannot hold a double quote"));
        };
        line.push_str(&format!(" NAME {word}"));
    }
    let save = form.save.trim();
    if !save.is_empty() {
        let Some(file) = line_word(&from_native_separators(save)) else {
            return Err(invalid("Save to: a path cannot hold a double quote"));
        };
        line.push_str(&format!(" save={file}"));
        if form.overwrite {
            line.push_str(" OVERWRITE");
        }
    }
    Ok(line)
}

/// Terrain > Analysis > Terrain Shading (see the module comment).
pub struct TerrainShadingDialog {
    frame: DialogFrame,
    visible: bool,
    source: SourcePicker,
    style: WordChoice,
    azimuth: f64,
    altitude: f64,
    z_factor: f64,
    variant: WordChoice,
    ramps: Vec<String>,
    ramp: Choice,
    range_min: String,
    range_max: String,
    name: String,
    save: String,
    overwrite: bool,
    runner: TerrainRun,
}

impl TerrainShadingDialog {
    /// The dialog over the window's surfaces and reference rasters.
    pub fn new(host: &dyn Host) -> Self {
        // The one table's names (katana_terrain's colour ramps), as the verb takes them: a ramp
        // added there is offered here.
        let ramps: Vec<String> = colour_ramp_names()
            .iter()
            .map(|name| String::from(*name))
            .collect();
        // An editable QComboBox shows its first item.
        let mut ramp = Choice::editable(ramps.first().map_or("", String::as_str));
        if !ramps.is_empty() {
            ramp.index = Some(0);
        }
        let mut dialog = Self {
            frame: DialogFrame::new("terrainShadingDialog", "Terrain Shading"),
            visible: false,
            source: SourcePicker::default(),
            style: WordChoice::new(&[
                ("Hillshade", "hillshade"),
                ("Colour relief", "relief"),
                ("Relief over hillshade", "relief+hillshade"),
                ("Slope", "slope"),
                ("Plain (grey)", "plain"),
            ]),
            azimuth: 315.0,
            altitude: 45.0,
            z_factor: 1.0,
            variant: WordChoice::new(&[
                ("regular", "regular"),
                ("combined", "combined"),
                ("multidirectional", "multidirectional"),
                ("igor", "igor"),
            ]),
            ramps,
            ramp,
            range_min: String::new(),
            range_max: String::new(),
            name: String::new(),
            save: String::new(),
            overwrite: false,
            runner: TerrainRun::new(),
        };
        dialog.reload(host);
        dialog
    }

    /// Shows the dialog, its sources read again (showEvent).
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

    /// Refills the sources, keeping what is chosen: done when shown.
    pub fn reload(&mut self, host: &dyn Host) {
        fill_sources(
            &mut self.source,
            terrain_sources(
                Some(host.document().surfaces()),
                Some(host.reference()),
                TerrainSourceKinds::SURFACES | TerrainSourceKinds::RASTERS,
            ),
        );
    }

    pub fn form(&self) -> ShadingForm {
        ShadingForm {
            source: String::from(self.source.words().unwrap_or("")),
            style: String::from(self.style.word()),
            azimuth: self.azimuth,
            altitude: self.altitude,
            z_factor: self.z_factor,
            variant: String::from(self.variant.word()),
            ramp: self.ramp.current_text(&self.ramps),
            range_min: self.range_min.clone(),
            range_max: self.range_max.clone(),
            name: self.name.clone(),
            save: self.save.clone(),
            overwrite: self.overwrite,
        }
    }

    pub fn command(&self) -> Result<String> {
        shading_line(&self.form())
    }

    /// The ramps the box offers, in its order.
    pub fn ramps(&self) -> &[String] {
        &self.ramps
    }

    /// What it said last (shadingReply).
    pub fn reply(&self) -> &str {
        self.runner.reply()
    }

    /// Chooses the style whose word is `style` (setCurrentIndex(findData)), and with it the
    /// style's own ramp, as the verb would take it.
    pub fn set_style(&mut self, style: &str) {
        if self.style.set_word(style) {
            self.style_changed();
        }
    }

    fn style_changed(&mut self) {
        let ramp = default_ramp(self.style.word());
        if !ramp.is_empty() {
            self.ramp.text = String::from(ramp);
            self.ramp.index = self.ramps.iter().position(|listed| listed == ramp);
        }
    }

    fn browse(&mut self, host: &dyn Host) {
        // A headless run has nobody to answer a file dialog: the field is filled by name instead
        // (--fill shadingSave=...).
        if host.headless() {
            *self.runner.reply_mut() = String::from(
                "A headless session opens no file dialog: fill shadingSave with the path instead.",
            );
            return;
        }
        let chosen = get_save_file_name(
            "Save the Shading",
            "shading.tif",
            "GeoTIFF (*.tif *.tiff)",
        );
        if chosen.is_empty() {
            return;
        }
        // The save dialog asked before it gave back a file that is there.
        self.overwrite = Path::new(&chosen).exists();
        self.save = from_native_separators(&chosen);
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

impl Framed for TerrainShadingDialog {
    fn frame_mut(&mut self) -> &mut DialogFrame {
        &mut self.frame
    }

    fn body(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        row(ui, "Source:", |ui| {
            self.source.ui(ui, "shadingSource");
        });
        let style_changed = row(ui, "Style:", |ui| {
            self.style.ui(ui, "shadingStyle").changed()
        });
        if style_changed {
            self.style_changed();
        }
        let lights = lit(self.style.word());
        let every_side = self.variant.word() == "multidirectional";
        let coloured = !default_ramp(self.style.word()).is_empty();
        row(ui, "Light from:", |ui| {
            ui.add_enabled_ui(lights && !every_side, |ui| {
                spin(
                    ui,
                    "shadingAzimuth",
                    &mut self.azimuth,
                    SpinRange::new(0.0, 360.0, 1.0, 1).with_suffix(" deg"),
                )
                .on_hover_text("Where the light comes from, clockwise from north.");
            });
        });
        row(ui, "Light height:", |ui| {
            ui.add_enabled_ui(lights, |ui| {
                spin(
                    ui,
                    "shadingAltitude",
                    &mut self.altitude,
                    SpinRange::new(0.0, 90.0, 1.0, 1).with_suffix(" deg"),
                );
            });
        });
        row(ui, "Exaggeration:", |ui| {
            ui.add_enabled_ui(lights, |ui| {
                spin(
                    ui,
                    "shadingZFactor",
                    &mut self.z_factor,
                    SpinRange::new(0.01, 1000.0, 1.0, 2),
                )
                .on_hover_text("Vertical exaggeration of the heights before they are lit.");
            });
        });
        row(ui, "Hillshade:", |ui| {
            ui.add_enabled_ui(lights, |ui| {
                self.variant.ui(ui, "shadingVariant");
            });
        });
        row(ui, "Ramp:", |ui| {
            ui.add_enabled_ui(coloured, |ui| {
                choice(ui, "shadingRamp", &self.ramps, &mut self.ramp).on_hover_text(
                    "A built-in ramp, or the path of a GDAL colour-map file typed in.",
                );
            });
        });
        row(ui, "Range:", |ui| {
            ui.add_enabled_ui(coloured, |ui| {
                field(
                    ui,
                    "shadingRangeMin",
                    &mut self.range_min,
                    "the data's least",
                );
                field(
                    ui,
                    "shadingRangeMax",
                    &mut self.range_max,
                    "the data's greatest",
                );
            });
        });
        row(ui, "Name:", |ui| {
            field(
                ui,
                "shadingName",
                &mut self.name,
                "the source's and the style's",
            );
        });
        let browse = row(ui, "Save to:", |ui| {
            field(
                ui,
                "shadingSave",
                &mut self.save,
                "also write the picture to a GeoTIFF (.tif)",
            );
            button(ui, "shadingSaveBrowse", "Browse...").clicked()
        });
        if browse {
            self.browse(&*host);
        }
        let saves = !self.save.trim().is_empty();
        ui.add_enabled_ui(saves, |ui| {
            check(
                ui,
                "shadingOverwrite",
                "Replace a file already there",
                &mut self.overwrite,
            );
        });
        command_field(ui, "shadingCommand", &self.command());
    }

    fn footer(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        let _ = self.runner.poll();
        let ready = self.command().is_ok();
        let (preview, run) = run_buttons(
            ui,
            Some(("shadingPreview", ready)),
            ("shadingRun", "Shade", ready),
        );
        terrain_reply_field(ui, "shadingReply", self.runner.reply_mut());
        if preview {
            self.run(host, true);
        }
        if run {
            self.run(host, false);
        }
    }
}
