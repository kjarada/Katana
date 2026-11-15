//! Terrain > Analysis > Statistics by Area (geo/zonal_stats_dialog.hpp; docs/terrain.md, "Zonal
//! statistics"): the window onto the RASTER ZONAL verb.
//!
//! The dialog measures nothing itself. It writes the RASTER ZONAL line its fields describe - shown,
//! as it will run, in zonalCommand - and Run hands it to the window's one executor as if it had
//! been typed (terrain_dialog_support): the statistics are the verb's background job, written on
//! each zone as properties in one undo step, and the reply comes back into zonalReply.
//!
//! ```text
//! zonalStatsDialog:
//!   zonalSource     the surface or reference raster
//!   zonalScope      the zones: the closed shapes the scope takes (ScopeFilterWidget)
//!   zonalStats      the statistics, each a box to tick
//!   zonalPrefix     the property prefix: <prefix>_<statistic>
//!   zonalPixels     which cells count: fractional, centre, all-touched
//!   zonalCsv        also write the table to this CSV; zonalOverwrite
//!   zonalCommand, zonalPreview, zonalRun, zonalReply
//! ```

use egui::Ui;
use katana_core::Result;

use super::terrain_dialog_parts::{
    Framed, WordChoice, check_list, command_field, field, host_views, invalid, row, run_buttons,
    show_framed,
};
use super::terrain_dialog_support::{
    SourcePicker, TerrainRun, TerrainSourceKinds, fill_sources, line_word, terrain_reply_field,
    terrain_sources,
};
use crate::customisation::{ScopeChoice, ScopeFilterWidget};
use crate::drive::check;
use crate::host::Host;
use crate::shell::DialogFrame;

/// The fields of the dialog, as the line is written from them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ZonalForm {
    /// SURFACE <name> or RASTER <id>, as the source picker's data says it.
    pub source: String,
    pub stats: Vec<String>,
    pub prefix: String,
    pub pixels: String,
    pub csv: String,
    pub overwrite: bool,
    /// The scope and filter words (ScopeFilterWidget::verb_words), or - in `scope_error` - why the
    /// controls say none.
    pub scope: String,
    pub scope_error: String,
}

impl Default for ZonalForm {
    fn default() -> Self {
        Self {
            source: String::new(),
            stats: Vec::new(),
            prefix: String::from("zone"),
            pixels: String::from("fractional"),
            csv: String::new(),
            overwrite: false,
            scope: String::new(),
            scope_error: String::new(),
        }
    }
}

/// What the list offers, in the verb's words, the first five ticked: the verb's own default set.
const STAT_CHOICES: [(&str, &str, bool); 10] = [
    ("mean", "Mean", true),
    ("min", "Least", true),
    ("max", "Greatest", true),
    ("count", "Cells (count)", true),
    ("sum", "Sum", true),
    ("median", "Median", false),
    ("stdev", "Standard deviation", false),
    ("variance", "Variance", false),
    ("mode", "Commonest value", false),
    ("variety", "Distinct values", false),
];

/// `^[A-Za-z0-9_.-]+$`: a property key's first part.
fn is_key(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
}

/// ```text
/// RASTER ZONAL <source> <scope> stats=<a,b,...> prefix=<p> pixels=<how> [csv=<file>] [OVERWRITE]
/// ```
///
/// InvalidArgument naming the field: no source, no statistic ticked, a prefix that is no property
/// key's first part, a file name with a double quote, a scope the controls cannot say.
pub fn zonal_line(form: &ZonalForm) -> Result<String> {
    let source = form.source.trim();
    if source.is_empty() {
        return Err(invalid(
            "Source: there is no surface or raster to measure; build a surface or import a DEM \
             first",
        ));
    }
    if !form.scope_error.is_empty() {
        return Err(invalid(format!("Zones: {}", form.scope_error)));
    }
    if form.stats.is_empty() {
        return Err(invalid("Statistics: tick at least one"));
    }
    let prefix = form.prefix.trim();
    if !is_key(prefix) {
        return Err(invalid(
            "Property prefix: letters, digits, '_', '.' and '-', as zone",
        ));
    }
    let mut line = format!("RASTER ZONAL {source}");
    let scope = form.scope.trim();
    if !scope.is_empty() {
        line.push_str(&format!(" {scope}"));
    }
    line.push_str(&format!(
        " stats={} prefix={prefix} pixels={}",
        form.stats.join(","),
        form.pixels
    ));
    let csv = form.csv.trim();
    if !csv.is_empty() {
        let Some(word) = line_word(&format!("csv={csv}")) else {
            return Err(invalid(
                "CSV file: a file name cannot hold a double quote",
            ));
        };
        line.push_str(&format!(" {word}"));
        if form.overwrite {
            line.push_str(" OVERWRITE");
        }
    }
    Ok(line)
}

/// Terrain > Analysis > Statistics by Area (see the module comment).
pub struct ZonalStatsDialog {
    frame: DialogFrame,
    visible: bool,
    source: SourcePicker,
    scope: ScopeFilterWidget,
    /// Each statistic's label and whether it is ticked, in STAT_CHOICES' order.
    stats: Vec<(String, bool)>,
    prefix: String,
    pixels: WordChoice,
    csv: String,
    overwrite: bool,
    runner: TerrainRun,
}

impl ZonalStatsDialog {
    /// The dialog over the window's surfaces, reference rasters, drawing and views.
    pub fn new(host: &mut dyn Host) -> Self {
        let mut scope = ScopeFilterWidget::new("zonal");
        scope.views = host_views(host);
        scope.set_choice(ScopeChoice::Selection);
        let mut dialog = Self {
            frame: DialogFrame::new("zonalStatsDialog", "Statistics by Area"),
            visible: false,
            source: SourcePicker::default(),
            scope,
            stats: STAT_CHOICES
                .iter()
                .map(|&(_, label, ticked)| (String::from(label), ticked))
                .collect(),
            prefix: String::from("zone"),
            pixels: WordChoice::new(&[
                ("Each cell by the part inside (fractional)", "fractional"),
                ("Cells whose centre is inside", "centre"),
                ("Every cell the shape touches", "all-touched"),
            ]),
            csv: String::new(),
            overwrite: false,
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

    /// The statistics the list offers, in the verb's words, in the list's order.
    pub fn stat_words() -> Vec<&'static str> {
        STAT_CHOICES.iter().map(|&(word, _, _)| word).collect()
    }

    /// Ticks or clears the statistic whose word is `word` (its item's setCheckState).
    pub fn set_stat(&mut self, word: &str, on: bool) {
        if let Some(at) = STAT_CHOICES
            .iter()
            .position(|&(listed, _, _)| listed == word)
        {
            self.stats[at].1 = on;
        }
    }

    pub fn form(&self) -> ZonalForm {
        let (scope, scope_error) = match self.scope.verb_words() {
            Ok(words) => (words, String::new()),
            Err(error) => (String::new(), error.message),
        };
        ZonalForm {
            source: String::from(self.source.words().unwrap_or("")),
            stats: STAT_CHOICES
                .iter()
                .zip(&self.stats)
                .filter(|(_, (_, ticked))| *ticked)
                .map(|(&(word, _, _), _)| String::from(word))
                .collect(),
            prefix: self.prefix.clone(),
            pixels: String::from(self.pixels.word()),
            csv: self.csv.clone(),
            overwrite: self.overwrite,
            scope,
            scope_error,
        }
    }

    pub fn command(&self) -> Result<String> {
        zonal_line(&self.form())
    }

    pub fn scope_controls(&mut self) -> &mut ScopeFilterWidget {
        &mut self.scope
    }

    /// What it said last (zonalReply).
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

impl Framed for ZonalStatsDialog {
    fn frame_mut(&mut self) -> &mut DialogFrame {
        &mut self.frame
    }

    fn body(&mut self, ui: &mut Ui, _host: &mut dyn Host) {
        row(ui, "Source:", |ui| {
            self.source.ui(ui, "zonalSource");
        });
        ui.scope(|ui| {
            self.scope.ui(ui);
        })
        .response
        .on_hover_text(
            "The zones: the closed shapes these take. Open lines and points bound no area and \
             are left out.",
        );
        row(ui, "Statistics:", |ui| {
            ui.vertical(|ui| {
                check_list(ui, "zonalStats", &mut self.stats, 120.0);
            });
        });
        row(ui, "Property prefix:", |ui| {
            field(ui, "zonalPrefix", &mut self.prefix, "").on_hover_text(
                "Each statistic is written on its zone as <prefix>_<statistic>: zone_mean, \
                 zone_count ...",
            );
        });
        row(ui, "Cells:", |ui| {
            self.pixels.ui(ui, "zonalPixels");
        });
        row(ui, "Also to CSV:", |ui| {
            field(ui, "zonalCsv", &mut self.csv, "blank: no file");
        });
        let writes = !self.csv.trim().is_empty();
        ui.add_enabled_ui(writes, |ui| {
            check(
                ui,
                "zonalOverwrite",
                "Replace the file if it is there",
                &mut self.overwrite,
            );
        });
        command_field(ui, "zonalCommand", &self.command());
    }

    fn footer(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        let _ = self.runner.poll();
        let ready = self.command().is_ok();
        let (preview, run) = run_buttons(
            ui,
            Some(("zonalPreview", ready)),
            ("zonalRun", "Measure", ready),
        );
        terrain_reply_field(ui, "zonalReply", self.runner.reply_mut());
        if preview {
            self.run(host, true);
        }
        if run {
            self.run(host, false);
        }
    }
}
