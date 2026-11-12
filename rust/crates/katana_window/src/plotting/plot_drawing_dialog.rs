//! File > Plot to PDF and the PLOT verb: the drawing, as the active plan view shows it, on one
//! sheet (docs/headless.md, "--plot"; docs/desktop.md, "Plot to PDF and view images"). The port of
//! plotting/plot_drawing_dialog.
//!
//! ```text
//! PLOT <file.pdf> [paper=A0|A1|A2|A3|A4] [landscape|portrait] [fit|scale=N]
//!      [dpi=N] [style=colour|grey|mono] [lineweight=F] [margin=MM]
//! ```
//!
//! What is not given takes PlotSettings' default: A3 landscape, fitted at the first standard scale
//! the drawing fits, 300 dpi, in colour, line weights as drawn, a 10 mm margin. scale= may be
//! written 500 or 1:500. The verb is the window's, beside PLOTSHEETS: the painter is the window's,
//! which katana_cli and katana_mcp do not have; a headless run has --plot, whose switches take the
//! same settings.
//!
//! The dialog is non-modal and runs nothing itself: it writes the PLOT line its fields describe,
//! shown in plotDrawingCommand, and Plot hands it to the window's one executor. Object names:
//!
//! ```text
//! plotDrawingDialog            the dialog (File > Plot to PDF, filePlot)
//! plotDrawingPath              the PDF to write
//! plotDrawingBrowse            choose it with a file dialog (not headless)
//! plotDrawingPaper             A4 .. A0
//! plotDrawingOrientation       Landscape / Portrait
//! plotDrawingFit               fit the drawing at a standard scale
//! plotDrawingScale             1 : N when not fitting
//! plotDrawingDpi               the resolution
//! plotDrawingMargin            the margin all round, mm
//! plotDrawingColourMode        Colour / Greyscale / Monochrome
//! plotDrawingLineWeightScale   every line weight times this, 0.10 to 5.00
//! plotDrawingCommand           the exact line Plot will run (read-only)
//! plotDrawingPlot              run it
//! plotDrawingStatus            what happened last
//! plotDrawingWarnings          what the plot is without, one line each - the `warning
//!                              surface=... style=... reason=...` records of the PLOT reply (a
//!                              surface's contours too fine to trace) - hidden when none
//! plotDrawingClose             close
//! ```
//!
//! WHAT DIFFERS FROM THE C++. The number boxes have no "1 : " before the scale (the foundation's
//! box takes a suffix only), and the command line is shown in the window's own face, not the
//! fixed-pitch one (the foundation's line field takes no font).

use std::path::Path;

use egui::Ui;
use katana_cad::command_interpreter::CommandInterpreter;
use katana_cad::plot::{PaperSize, PlotColourMode, PlotSettings, plot_colour_mode_from};
use katana_cad::plotting::page_setup::{MAXIMUM_LINE_WEIGHT_SCALE, MINIMUM_LINE_WEIGHT_SCALE};
use katana_core::error::make_error;
use katana_core::text::{format_general, parse_finite_double};
use katana_core::{Error, ErrorCode, Result};
use katana_entity::EnumName;

use crate::drive::controls::{self, Choice, LineOptions, SpinRange};
use crate::host::Host;
use crate::ifc_dialogs::{from_native_separators, get_save_file_name, to_native_separators};
use crate::shell::DialogFrame;

const USAGE: &str = "usage: PLOT <file.pdf> [paper=A0|A1|A2|A3|A4] [landscape|portrait] \
                     [fit|scale=N] [dpi=N] [style=colour|grey|mono] [lineweight=F] [margin=MM]";

// The dialog's ranges, which the verb keeps too, so the two take the same plots: 72 dpi is a
// screen's and 1200 a plotter's finest; a margin wider than 50 mm leaves an A4 sheet a postcard.
const MINIMUM_DPI: f64 = 72.0;
const MAXIMUM_DPI: f64 = 1200.0;
const MAXIMUM_MARGIN_MM: f64 = 50.0;

/// In the order the dialog lists them.
const PAPERS: [(PaperSize, &str); 5] = [
    (PaperSize::A4, "A4"),
    (PaperSize::A3, "A3"),
    (PaperSize::A2, "A2"),
    (PaperSize::A1, "A1"),
    (PaperSize::A0, "A0"),
];
const MODES: [PlotColourMode; 3] = [
    PlotColourMode::Colour,
    PlotColourMode::Greyscale,
    PlotColourMode::Monochrome,
];

fn paper_name(paper: PaperSize) -> &'static str {
    PAPERS
        .iter()
        .find(|(size, _)| *size == paper)
        .map_or("A3", |(_, name)| name)
}

/// QString::number(value, 'g', 10).
fn number_text(value: f64) -> String {
    format_general(value, 10)
}

fn refused(why: &str, word: &str) -> Error {
    make_error(ErrorCode::InvalidArgument, format!("{why}; {USAGE}"), word)
}

/// A number in [low, high], or nothing.
fn number_in(text: &str, low: f64, high: f64) -> Option<f64> {
    parse_finite_double(text).filter(|value| (low..=high).contains(value))
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlotDrawingRequest {
    pub path: String,
    /// Paper, orientation, scale, dpi, margin, colour mode and line weight scale; the centre is
    /// the plot's to set.
    pub settings: PlotSettings,
    pub fit: bool,
}

impl Default for PlotDrawingRequest {
    fn default() -> Self {
        Self {
            path: String::new(),
            settings: PlotSettings::default(),
            fit: true,
        }
    }
}

/// A PLOT line read by the interpreter's word rules (a path with blanks quoted). InvalidArgument
/// naming the word, with the usage, for anything the grammar above does not take: no path, an
/// unknown option, a paper that is not A0 to A4, a scale that is not a positive number, a dpi
/// outside 72 to 1200, a line weight scale outside 0.1 to 5, a margin outside 0 to 50 mm.
pub fn parse_plot_drawing(line: &str) -> Result<PlotDrawingRequest> {
    let tokens = CommandInterpreter::tokenize(line)?;
    if tokens.len() < 2 || tokens[1].is_empty() {
        return Err(Error::new(ErrorCode::InvalidArgument, USAGE));
    }
    let mut request = PlotDrawingRequest {
        path: tokens[1].clone(),
        ..PlotDrawingRequest::default()
    };
    for raw in &tokens[2..] {
        let upper = raw.to_uppercase();
        if upper == "FIT" {
            request.fit = true;
            continue;
        }
        if upper == "LANDSCAPE" || upper == "PORTRAIT" {
            request.settings.landscape = upper == "LANDSCAPE";
            continue;
        }
        let Some(equals) = raw.find('=').filter(|at| *at > 0) else {
            return Err(refused("not a PLOT option", raw));
        };
        let key = raw[..equals].to_lowercase();
        let mut value = &raw[equals + 1..];
        match key.as_str() {
            "paper" => {
                let Some((paper, _)) = PAPERS
                    .iter()
                    .find(|(_, name)| value.eq_ignore_ascii_case(name))
                else {
                    return Err(refused("paper= is A0, A1, A2, A3 or A4", raw));
                };
                request.settings.paper = *paper;
            }
            "scale" => {
                // 1:500 as a scale rule writes it, or 500.
                if let Some(denominator) = value.strip_prefix("1:") {
                    value = denominator;
                }
                let Some(denominator) = parse_finite_double(value).filter(|d| *d > 0.0) else {
                    return Err(refused("scale= is a positive number, 500 or 1:500", raw));
                };
                request.settings.scale_denominator = denominator;
                request.fit = false;
            }
            "dpi" => {
                let Some(dpi) = number_in(value, MINIMUM_DPI, MAXIMUM_DPI) else {
                    return Err(refused("dpi= is from 72 to 1200", raw));
                };
                request.settings.dpi = dpi;
            }
            "style" => {
                let Some(mode) = plot_colour_mode_from(value) else {
                    return Err(refused("style= is colour, grey or mono", raw));
                };
                request.settings.colour_mode = mode;
            }
            "lineweight" => {
                let Some(factor) =
                    number_in(value, MINIMUM_LINE_WEIGHT_SCALE, MAXIMUM_LINE_WEIGHT_SCALE)
                else {
                    return Err(refused("lineweight= is from 0.1 to 5", raw));
                };
                request.settings.line_weight_scale = factor;
            }
            "margin" => {
                let Some(margin) = number_in(value, 0.0, MAXIMUM_MARGIN_MM) else {
                    return Err(refused("margin= is from 0 to 50 mm", raw));
                };
                request.settings.margin_mm = margin;
            }
            _ => return Err(refused("not a PLOT option", raw)),
        }
    }
    Ok(request)
}

/// The line parse_plot_drawing reads back, every setting written out: the path quoted with '/'
/// separators, the paper, the orientation, fit or the scale, the dpi, the style, the line weight
/// scale and the margin.
pub fn plot_drawing_command_line(request: &PlotDrawingRequest) -> String {
    let settings = &request.settings;
    format!(
        "PLOT \"{}\" paper={} {} {} dpi={} style={} lineweight={} margin={}",
        from_native_separators(&request.path),
        paper_name(settings.paper),
        if settings.landscape {
            "landscape"
        } else {
            "portrait"
        },
        if request.fit {
            String::from("fit")
        } else {
            format!("scale={}", number_text(settings.scale_denominator))
        },
        number_text(settings.dpi),
        settings.colour_mode.name(),
        number_text(settings.line_weight_scale),
        number_text(settings.margin_mm),
    )
}

// ---- the dialog ---------------------------------------------------------------------------------

/// Asked before Plot writes over a file that is there: true to write it.
pub type ConfirmReplace = Box<dyn FnMut(&str) -> bool>;

/// File > Plot to PDF (see the module comment).
pub struct PlotDrawingDialog {
    frame: DialogFrame,
    visible: bool,
    /// The last path suggested, to tell it from one typed.
    suggested: String,
    path: String,
    paper: Choice,
    orientation: Choice,
    fit: bool,
    scale: f64,
    dpi: f64,
    margin: f64,
    colour_mode: Choice,
    line_weight: f64,
    status: String,
    warnings: String,
    /// Unset, a file that is there is written - as the PLOT verb writes, asking nothing. The
    /// window asks the person, and never in a headless run.
    confirm_replace: Option<ConfirmReplace>,
}

/// The dialog and the window, lent together to the dialog frame's body and footer.
struct Lent<'a, 'h> {
    dialog: &'a mut PlotDrawingDialog,
    host: &'a mut (dyn Host + 'h),
}

impl PlotDrawingDialog {
    /// The dialog on `suggested_path`: the file Browse and a new dialog start from.
    pub fn new(suggested_path: &str) -> Self {
        let defaults = PlotSettings::default();
        Self {
            // Not modal: the drawing stays in reach while a plot is set up.
            frame: DialogFrame::new("plotDrawingDialog", "Plot to PDF"),
            visible: false,
            suggested: String::from(suggested_path),
            path: String::from(suggested_path),
            paper: Choice::at(
                PAPERS
                    .iter()
                    .position(|(size, _)| *size == defaults.paper)
                    .unwrap_or(1),
            ),
            orientation: Choice::at(usize::from(!defaults.landscape)),
            // Fitting is the default because it is what a first plot of any drawing wants, and it
            // picks a scale a scale rule carries.
            fit: true,
            scale: defaults.scale_denominator,
            dpi: defaults.dpi,
            margin: defaults.margin_mm,
            colour_mode: Choice::at(0),
            line_weight: defaults.line_weight_scale,
            status: String::new(),
            warnings: String::new(),
            confirm_replace: None,
        }
    }

    /// Sets who is asked before a file that is there is written over.
    pub fn set_confirm_replace(&mut self, confirm: ConfirmReplace) {
        self.confirm_replace = Some(confirm);
    }

    pub fn is_modal(&self) -> bool {
        self.frame.is_modal()
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn show(&mut self) {
        self.visible = true;
    }

    /// The Close button's: hidden, its fields kept.
    pub fn close(&mut self) {
        self.visible = false;
    }

    /// What plotDrawingPath holds.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Types `path` into plotDrawingPath.
    pub fn set_path(&mut self, path: &str) {
        self.path = String::from(path);
    }

    /// What plotDrawingStatus says.
    pub fn status(&self) -> &str {
        &self.status
    }

    /// What plotDrawingWarnings says; empty while it is hidden.
    pub fn warnings(&self) -> &str {
        &self.warnings
    }

    /// The request the fields describe, or why there is none (no file named).
    pub fn request(&self) -> Result<PlotDrawingRequest> {
        let path = self.path.trim();
        if path.is_empty() {
            return Err(Error::new(ErrorCode::InvalidArgument, "no PDF is named"));
        }
        let mut request = PlotDrawingRequest {
            path: String::from(path),
            fit: self.fit,
            ..PlotDrawingRequest::default()
        };
        let settings = &mut request.settings;
        settings.paper = PAPERS[self.paper.index.unwrap_or(1).min(PAPERS.len() - 1)].0;
        settings.landscape = self.orientation.index == Some(0);
        settings.scale_denominator = self.scale;
        settings.dpi = self.dpi;
        settings.margin_mm = self.margin;
        settings.colour_mode = MODES[self.colour_mode.index.unwrap_or(0).min(MODES.len() - 1)];
        settings.line_weight_scale = self.line_weight;
        Ok(request)
    }

    /// The line plotDrawingCommand shows: the one Plot runs, or nothing while there is none.
    pub fn command(&self) -> String {
        self.request()
            .map(|request| plot_drawing_command_line(&request))
            .unwrap_or_default()
    }

    /// What Plot does: the PLOT line handed to the window's executor and the outcome said in
    /// plotDrawingStatus.
    pub fn plot(&mut self, host: &mut dyn Host) {
        // Whatever the last plot was without is not this plot's.
        self.warnings.clear();
        let made = match self.request() {
            Ok(made) => made,
            Err(error) => {
                self.status = error.message;
                return;
            }
        };
        // Browse's file dialog asked before a file was written over; a path typed or suggested is
        // asked about here, as the one before this dialog was.
        if let Some(confirm) = self.confirm_replace.as_mut()
            && Path::new(&made.path).exists()
            && !confirm(&made.path)
        {
            self.status = format!(
                "Not plotted: {} is there and was kept.",
                to_native_separators(&made.path)
            );
            return;
        }
        let outcome = host.run(&plot_drawing_command_line(&made));
        let last_line = |text: &str| String::from(text.rsplit('\n').next().unwrap_or(""));
        self.status = if outcome.ok {
            format!("Plotted: {}", last_line(&outcome.reply))
        } else {
            format!("Not plotted: {}", last_line(&outcome.error))
        };
        // The reply's warning records, as they came: the dialog reads the reply and keeps no rule
        // of its own about what is wrong with a plot.
        let warnings: Vec<&str> = outcome
            .reply
            .split('\n')
            .filter(|line| line.starts_with("warning "))
            .collect();
        self.warnings = warnings.join("\n");
    }

    /// The file for the drawing as it is now - another project opened, an untitled one saved:
    /// shown in plotDrawingPath unless something else has been typed there since the last
    /// suggestion. The dialog is kept while the window lives, and once plotted every drawing into
    /// the folder, and under the name, of the project open when it was first shown.
    pub fn suggest_path(&mut self, path: &str) {
        if self.path == self.suggested {
            self.path = String::from(path);
        }
        self.suggested = String::from(path);
    }

    fn browse(&mut self, host: &dyn Host) {
        if host.headless() {
            self.status = String::from(
                "A headless session opens no file dialog: fill plotDrawingPath with the path \
                 instead.",
            );
            return;
        }
        let mut path = get_save_file_name("Plot to PDF", self.path.trim(), "PDF (*.pdf)");
        if path.is_empty() {
            return;
        }
        if !path.to_lowercase().ends_with(".pdf") {
            path.push_str(".pdf");
        }
        self.path = to_native_separators(&path);
    }

    /// One frame of the dialog, when it is shown.
    pub fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        if !self.visible {
            return;
        }
        let ctx = ui.ctx().clone();
        let mut frame = self.frame.clone();
        let mut lent = Lent {
            dialog: &mut *self,
            host: &mut *host,
        };
        let shown = frame.show(
            &ctx,
            &mut lent,
            |ui, lent| lent.dialog.body(ui, &*lent.host),
            |ui, lent| lent.dialog.footer(ui, &mut *lent.host),
        );
        self.frame = frame;
        if shown.closed {
            self.close();
        }
    }

    fn body(&mut self, ui: &mut Ui, host: &dyn Host) {
        // Wide enough for a path and the line it runs to be read whole.
        ui.set_min_width(560.0);
        egui::Grid::new("plotDrawingForm")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label("PDF:");
                ui.horizontal(|ui| {
                    controls::line_with(
                        ui,
                        "plotDrawingPath",
                        &mut self.path,
                        LineOptions {
                            placeholder: "the PDF to write",
                            ..LineOptions::default()
                        },
                    );
                    if controls::button(ui, "plotDrawingBrowse", "Browse...").clicked() {
                        self.browse(host);
                    }
                });
                ui.end_row();

                ui.label("Paper:");
                let papers: Vec<String> =
                    PAPERS.iter().map(|(_, name)| String::from(*name)).collect();
                controls::choice(ui, "plotDrawingPaper", &papers, &mut self.paper);
                ui.end_row();

                ui.label("Orientation:");
                let orientations = [String::from("Landscape"), String::from("Portrait")];
                controls::choice(
                    ui,
                    "plotDrawingOrientation",
                    &orientations,
                    &mut self.orientation,
                );
                ui.end_row();

                ui.label("");
                controls::check(
                    ui,
                    "plotDrawingFit",
                    "Fit the drawing to the sheet at a standard scale",
                    &mut self.fit,
                );
                ui.end_row();

                ui.label("Scale, when not fitting:");
                ui.add_enabled_ui(!self.fit, |ui| {
                    controls::spin(
                        ui,
                        "plotDrawingScale",
                        &mut self.scale,
                        SpinRange::new(1.0, 1_000_000.0, 1.0, 0),
                    );
                });
                ui.end_row();

                ui.label("Resolution (dpi):");
                controls::spin(
                    ui,
                    "plotDrawingDpi",
                    &mut self.dpi,
                    SpinRange::new(MINIMUM_DPI, MAXIMUM_DPI, 1.0, 0),
                );
                ui.end_row();

                ui.label("Margin:");
                controls::spin(
                    ui,
                    "plotDrawingMargin",
                    &mut self.margin,
                    SpinRange::new(0.0, MAXIMUM_MARGIN_MM, 1.0, 1).with_suffix(" mm"),
                );
                ui.end_row();

                ui.label("Colours:");
                let modes = [
                    String::from("Colour"),
                    String::from("Greyscale"),
                    String::from("Monochrome"),
                ];
                controls::choice(ui, "plotDrawingColourMode", &modes, &mut self.colour_mode)
                    .on_hover_text(
                        "Colour as drawn (white printing black), every colour as its grey, or \
                         black ink alone",
                    );
                ui.end_row();

                ui.label("Line weights:");
                controls::spin(
                    ui,
                    "plotDrawingLineWeightScale",
                    &mut self.line_weight,
                    SpinRange::new(
                        MINIMUM_LINE_WEIGHT_SCALE,
                        MAXIMUM_LINE_WEIGHT_SCALE,
                        0.1,
                        2,
                    )
                    .with_suffix(" x"),
                )
                .on_hover_text(
                    "Every line weight times this: 0.7 for a check plot, 1.4 for a bold one; \
                     text, dashes and symbols keep their size",
                );
                ui.end_row();

                ui.label("Command:");
                let mut command = self.command();
                controls::line_with(
                    ui,
                    "plotDrawingCommand",
                    &mut command,
                    LineOptions {
                        read_only: true,
                        ..LineOptions::default()
                    },
                )
                .on_hover_text(
                    "The line Plot hands to the command line - type it there and it does the same",
                );
                ui.end_row();
            });
    }

    fn footer(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        ui.horizontal(|ui| {
            let status = self.status.clone();
            controls::label_as(
                ui,
                "plotDrawingStatus",
                &status,
                egui::Label::new(status.as_str()).wrap().selectable(true),
            );
            let ready = self.request().is_ok();
            let pressed = ui
                .add_enabled_ui(ready, |ui| {
                    controls::button(ui, "plotDrawingPlot", "Plot").clicked()
                })
                .inner;
            if pressed {
                self.plot(&mut *host);
                ui.ctx().request_repaint();
            }
            if controls::button(ui, "plotDrawingClose", "Close").clicked() {
                self.close();
            }
        });
        // What the plot is without, from the reply's warning records: shown only when the reply
        // had some, so a plot that lost nothing looks as it did.
        let warnings = self.warnings.clone();
        ui.scope(|ui| {
            if warnings.is_empty() {
                ui.set_invisible();
            }
            controls::label_as(
                ui,
                "plotDrawingWarnings",
                &warnings,
                egui::Label::new(warnings.as_str()).wrap().selectable(true),
            );
        });
    }
}
