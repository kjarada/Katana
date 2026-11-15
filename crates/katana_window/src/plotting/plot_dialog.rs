//! The Plot dialog and the plot's progress (docs/plotting.md, "Plot styles and output"). The port
//! of plotting/plot_dialog.
//!
//! The dialog only collects a PlotRequest - which sheets, the format, the plot style, the
//! resolution, where it goes - and finish_plot hands it to plot_output, the same call the command
//! line and an agent make. Every control has an object name (listed at PlotDialog), so a test or a
//! scripted session fills it the way a person does.
//!
//! THE PROGRESS. The C++ showed an application-modal progress dialog that advanced a sheet at a
//! time and let the event loop run between sheets, its Cancel stopping the plot before the next
//! sheet. egui has no nested event loop: a frame cannot be drawn in the middle of another. The
//! plot runs in the frame that asked for it, and its progress is a `PlotProgressDialog` the plot
//! reports to before each sheet and asks whether to go on - what a test presses Cancel on, as the
//! C++ test pressed the QProgressDialog's. Rejected: a thread of its own, which the painter cannot
//! have while it reads the live drawing the window keeps editing (the C++ stayed on the GUI thread
//! for the same reason).
//!
//! The set is copied before the first sheet and the source asked for again before each, as the
//! C++ did.

#![allow(
    clippy::float_cmp,
    reason = "A page setup kept as it is compares equal to the one it came from, exactly."
)]

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};

use egui::Ui;
use katana_cad::document::Document;
use katana_cad::plot::{PlotColourMode, plot_colour_mode_from};
use katana_cad::plotting::page_setup::{
    MAXIMUM_LINE_WEIGHT_SCALE, MAXIMUM_PLOT_DPI, MINIMUM_LINE_WEIGHT_SCALE, MINIMUM_PLOT_DPI,
    PageSetup, format_sheet_selection, parse_sheet_selection, sanitise_file_name,
    set_page_setup, sheet_file_name, validate_page_setup,
};
use katana_cad::plotting::sheet_set::SheetSet;
use katana_core::path_text::generic_path_utf8;
use katana_core::{ErrorCode, Result, Status};
use katana_entity::EnumName;

use crate::drive::controls::{self, Choice, LineOptions, SpinRange};
use crate::ifc_dialogs::{get_save_file_name, to_native_separators};
use crate::plotting::plot_output::{
    NoPrinters, PlotFormat, PlotReport, PlotRequest, Printers, SheetSourceProvider,
    page_setup_for, planned_files, plot_format_from, plot_sheets, plots_file_per_sheet,
    print_sheets_on, validate_plot_request,
};
use crate::sheet_painter::SheetPaintCache;
use crate::shell::DialogFrame;

/// The format list's item data, beside the PlotFormat names: the printer.
const PRINTER_ITEM: &str = "printer";

const FORMATS: [(&str, &str); 5] = [
    ("PDF, one file", "pdf"),
    ("PDF, a file per sheet", "pdfs"),
    ("PNG images, one per sheet", "png"),
    ("TIFF images, one per sheet", "tiff"),
    ("Printer", PRINTER_ITEM),
];

const COLOUR_MODES: [(&str, &str); 3] = [
    ("Colour", "colour"),
    ("Greyscale", "greyscale"),
    ("Monochrome (all ink black)", "monochrome"),
];

/// Which sheets the dialog plots.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PlotSheets {
    #[default]
    All,
    Current,
    List,
}

/// What the dialog is for: a plot, or the set's page setup alone (Sheet Set > Page Setup).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PlotDialogMode {
    #[default]
    Plot,
    PageSetup,
}

/// What a frame of the dialog came to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlotAnswer {
    Open,
    Cancelled,
    /// Plot (plotRun), or in the page-setup mode Save (pageSetupSave).
    Accepted,
}

/// Object names: plotDialog; the sheets plotSheetsAll, plotSheetsCurrent, plotSheetsList (radio
/// buttons), plotSheetsText and plotSheetsSummary; the output plotFormat (pdf, pdfs, png, tiff,
/// printer - the item data), plotColourMode (colour, greyscale, monochrome),
/// plotLineWeightScale, plotDpi; the destination plotFile and plotBrowseFile, plotFolder and
/// plotBrowseFolder, plotPattern and plotPatternPreview, plotPrinter; then plotOpenAfter,
/// plotKeepSetup, plotProblem, and the buttons plotRun and plotCancel.
///
/// In its page-setup mode it is pageSetupDialog, "Page Setup": only what the set's page setup
/// keeps - plotColourMode, plotLineWeightScale, plotDpi, plotPattern with plotPatternPreview, and
/// pageSetupFilePerSheet ("A PDF per sheet") - with plotProblem, and the buttons pageSetupSave
/// and plotCancel. Nothing is plotted: Save's line is page_setup_line.
pub struct PlotDialog {
    frame: DialogFrame,
    set: SheetSet,
    current: usize,
    mode: PlotDialogMode,
    pub sheets: PlotSheets,
    pub list_text: String,
    pub format: Choice,
    pub colour_mode: Choice,
    pub line_weight_scale: f64,
    pub dpi: i64,
    pub file_per_sheet: bool,
    pub file: String,
    pub folder: String,
    pub pattern: String,
    printers: Vec<String>,
    pub printer: Choice,
    pub open_after: bool,
    pub keep_setup: bool,
}

impl PlotDialog {
    /// For `set`, starting from its page setup. `current` is the sheet the editor shows, offered
    /// as "Current sheet"; `all_sheets` picks All rather than Current to start with.
    /// `suggested_file` is the PDF offered, and its folder the folder offered for a file a sheet.
    pub fn new(
        set: &SheetSet,
        current: usize,
        all_sheets: bool,
        suggested_file: &str,
        mode: PlotDialogMode,
    ) -> Self {
        Self::with_printers(set, current, all_sheets, suggested_file, mode, &NoPrinters)
    }

    /// The same, listing the printers `printers` knows of.
    pub fn with_printers(
        set: &SheetSet,
        current: usize,
        all_sheets: bool,
        suggested_file: &str,
        mode: PlotDialogMode,
        printers: &dyn Printers,
    ) -> Self {
        let page_setup = mode == PlotDialogMode::PageSetup;
        let setup = &set.page_setup;
        let current = current.min(set.sheets.len().saturating_sub(1));
        let names = printers.names();
        let printer = names
            .iter()
            .position(|name| Some(name.clone()) == printers.default_name())
            .map_or_else(Choice::default, Choice::at);
        // The suggested file's folder: what QFileInfo::path says, "." for a bare name.
        let folder = if suggested_file.is_empty() {
            String::new()
        } else {
            let generic = suggested_file.replace('\\', "/");
            let folder = match generic.rfind('/') {
                Some(0) => String::from("/"),
                Some(at) => String::from(&generic[..at]),
                None => String::from("."),
            };
            to_native_separators(&folder)
        };
        Self {
            frame: DialogFrame::new(
                if page_setup {
                    "pageSetupDialog"
                } else {
                    "plotDialog"
                },
                if page_setup { "Page Setup" } else { "Plot Sheets" },
            )
            // Modal in both modes, as the C++'s open() (Page Setup) and exec() (Plot) make it. The
            // page setup is only ever sheet_set_menu's openPageSetupDialog, which deleted it on
            // close.
            .modal(true)
            .deletes_on_close(page_setup),
            set: set.clone(),
            current,
            mode,
            sheets: if all_sheets {
                PlotSheets::All
            } else {
                PlotSheets::Current
            },
            list_text: String::new(),
            format: Choice::at(usize::from(setup.file_per_sheet)),
            colour_mode: Choice {
                index: COLOUR_MODES
                    .iter()
                    .position(|(_, data)| *data == setup.colour_mode.name()),
                ..Choice::default()
            },
            line_weight_scale: SpinRange::new(
                MINIMUM_LINE_WEIGHT_SCALE,
                MAXIMUM_LINE_WEIGHT_SCALE,
                0.1,
                2,
            )
            .rounded(setup.line_weight_scale),
            dpi: setup.dpi.round() as i64,
            file_per_sheet: setup.file_per_sheet,
            file: to_native_separators(suggested_file),
            folder,
            pattern: setup.file_name_pattern.clone(),
            printers: names,
            printer,
            open_after: false,
            keep_setup: true,
        }
    }

    pub fn name(&self) -> &str {
        self.frame.name()
    }

    fn format_data(&self) -> &'static str {
        self.format
            .index
            .and_then(|index| FORMATS.get(index))
            .map_or("pdf", |(_, data)| *data)
    }

    fn colour_data(&self) -> &'static str {
        self.colour_mode
            .index
            .and_then(|index| COLOUR_MODES.get(index))
            .map_or("colour", |(_, data)| *data)
    }

    /// The format list's item data now (plotFormat's currentData).
    pub fn format_name(&self) -> &'static str {
        self.format_data()
    }

    /// The colour list's item data now (plotColourMode's currentData).
    pub fn colour_mode_name(&self) -> &'static str {
        self.colour_data()
    }

    /// The page setup the controls describe.
    pub fn page_setup(&self) -> PageSetup {
        let mut setup = self.set.page_setup.clone();
        setup.colour_mode = plot_colour_mode_from(self.colour_data()).unwrap_or(PlotColourMode::Colour);
        setup.line_weight_scale = self.line_weight_scale;
        setup.dpi = self.dpi as f64;
        setup.file_name_pattern.clone_from(&self.pattern);
        setup.file_per_sheet = if self.mode == PlotDialogMode::PageSetup {
            self.file_per_sheet
        } else {
            self.format_data() == "pdfs"
        };
        setup
    }

    /// The SHEETS PAGESETUP line that makes the page setup the set's - every option written out,
    /// the pattern quoted and its backslashes doubled so the verb reads it back as it is.
    pub fn page_setup_line(&self) -> String {
        // The line weight to the hundredth the box holds, without trailing zeros: 0.7, not 0.70.
        let mut weight = crate::format::qt_fixed(self.line_weight_scale, 2);
        while weight.ends_with('0') {
            weight.pop();
        }
        if weight.ends_with('.') {
            weight.pop();
        }
        // A typed "\\" is read back as one backslash, and any other as itself: doubled, every
        // backslash comes back as it was.
        let pattern = self.pattern.replace('\\', "\\\\");
        format!(
            "SHEETS PAGESETUP style={} lineweight={weight} dpi={} pattern=\"{pattern}\" filepersheet={}",
            self.colour_data(),
            self.dpi,
            if self.page_setup().file_per_sheet {
                "on"
            } else {
                "off"
            }
        )
    }

    pub fn to_printer(&self) -> bool {
        self.format_data() == PRINTER_ITEM
    }

    pub fn printer_name(&self) -> String {
        self.printer.current_text(&self.printers)
    }

    pub fn open_afterwards(&self) -> bool {
        self.open_after
    }

    /// Whether the style, resolution and naming are to become the set's page setup, so the next
    /// plot starts from them.
    pub fn keep_as_page_setup(&self) -> bool {
        self.keep_setup
    }

    /// The plot the controls describe. For the printer the destination is empty and the format
    /// is the page setup's.
    pub fn request(&self) -> PlotRequest {
        let mut request = PlotRequest::default();
        match self.sheets {
            PlotSheets::Current => request.sheets = format!("{}", self.current + 1),
            PlotSheets::List => request.sheets.clone_from(&self.list_text),
            PlotSheets::All => {}
        }
        request.format = plot_format_from(self.format_data()).unwrap_or(
            if self.set.page_setup.file_per_sheet {
                PlotFormat::PdfPerSheet
            } else {
                PlotFormat::Pdf
            },
        );
        request.colour_mode = plot_colour_mode_from(self.colour_data()).unwrap_or(PlotColourMode::Colour);
        request.line_weight_scale = self.line_weight_scale;
        request.dpi = self.dpi as f64;
        request.file_name_pattern.clone_from(&self.pattern);
        if !self.to_printer() {
            let typed = if plots_file_per_sheet(request.format) {
                &self.folder
            } else {
                &self.file
            };
            request.destination = typed.trim().replace('\\', "/");
        }
        request
    }

    /// What stops the choice being plotted - a sheet list that names no sheet, a raster too
    /// large, no destination - as validate_plot_request says it; shown in the dialog, and Plot is
    /// disabled while there is one. In the page-setup mode, what stops the page setup being
    /// saved: validate_page_setup's refusal, or a double quote in the pattern, which the command
    /// line has no way to type.
    pub fn check(&self) -> Status {
        if self.mode == PlotDialogMode::PageSetup {
            if self.pattern.contains('"') {
                return Err(katana_core::Error::new(
                    ErrorCode::InvalidArgument,
                    "a file-name pattern cannot hold a double quote: the command line has no way \
                     to type one",
                ));
            }
            return validate_page_setup(&self.page_setup());
        }
        let asked = self.request();
        if self.to_printer() {
            if self.printers.is_empty() {
                return Err(katana_core::Error::new(
                    ErrorCode::InvalidArgument,
                    "there is no printer to print on",
                ));
            }
            // The printer takes no destination; everything else is checked alike.
            let mut checked = asked;
            checked.destination = String::from("printer");
            return validate_plot_request(&self.set, &checked);
        }
        validate_plot_request(&self.set, &asked)
    }

    /// What the problem line says (plotProblem): check's refusal, or nothing.
    pub fn problem(&self) -> String {
        match self.check() {
            Ok(()) => String::new(),
            Err(error) => error.message,
        }
    }

    /// The sheets the choice names, as the set numbers them (plotSheetsSummary).
    pub fn sheets_summary(&self) -> String {
        match parse_sheet_selection(&self.request().sheets, &self.set) {
            Ok(selection) => {
                let mut text = format!(
                    "{} sheet{}",
                    selection.len(),
                    if selection.len() == 1 { "" } else { "s" }
                );
                if selection.len() < self.set.sheets.len() {
                    text.push_str(&format!(": {}", format_sheet_selection(&selection)));
                }
                text
            }
            Err(_) => String::new(),
        }
    }

    /// The first file a file a sheet makes (plotPatternPreview).
    pub fn pattern_preview(&self) -> String {
        if self.mode == PlotDialogMode::PageSetup {
            // The pattern is always the page setup's to keep, a file per sheet or not; the first
            // sheet's file shows what it makes.
            if self.set.sheets.is_empty() {
                return String::new();
            }
            let first = sheet_file_name(&self.set, 0, &self.pattern);
            return if first.is_empty() {
                String::new()
            } else {
                format!("First file: {first}.pdf")
            };
        }
        let asked = self.request();
        if !self.per_sheet() {
            return String::new();
        }
        match planned_files(&self.set, &asked) {
            Ok(files) => files.first().map_or_else(String::new, |first| {
                let generic = first.replace('\\', "/");
                let name = generic.rsplit('/').next().unwrap_or_default();
                format!("First file: {name}")
            }),
            Err(_) => String::new(),
        }
    }

    fn per_sheet(&self) -> bool {
        !self.to_printer() && plots_file_per_sheet(self.request().format)
    }

    /// One frame of the dialog.
    pub fn ui(&mut self, ctx: &egui::Context, headless: bool) -> PlotAnswer {
        let mut frame = self.frame.clone();
        let mut answer = PlotAnswer::Open;
        let shown = frame.show(
            ctx,
            &mut (&mut *self, &mut answer),
            |ui, (dialog, _)| dialog.body(ui, headless),
            |ui, (dialog, answer)| {
                let ok = dialog.check().is_ok();
                ui.horizontal(|ui| {
                    let (name, text) = if dialog.mode == PlotDialogMode::PageSetup {
                        ("pageSetupSave", "Save")
                    } else {
                        ("plotRun", "Plot")
                    };
                    if ui
                        .add_enabled_ui(ok, |ui| controls::button(ui, name, text))
                        .inner
                        .clicked()
                    {
                        **answer = PlotAnswer::Accepted;
                    }
                    if controls::button(ui, "plotCancel", "Cancel").clicked() {
                        **answer = PlotAnswer::Cancelled;
                    }
                });
            },
        );
        self.frame = frame;
        if shown.closed {
            answer = PlotAnswer::Cancelled;
        }
        answer
    }

    fn body(&mut self, ui: &mut Ui, headless: bool) {
        ui.set_min_width(520.0);
        let page_setup = self.mode == PlotDialogMode::PageSetup;
        let before = (self.sheets, self.format.clone(), self.list_text.clone());
        // A row shown or not; drawn either way, so a driver can fill it, as a hidden Qt row.
        let row = |ui: &mut Ui, shown: bool, add: &mut dyn FnMut(&mut Ui)| {
            ui.scope(|ui| {
                if !shown {
                    ui.set_invisible();
                }
                add(ui);
            });
        };
        let per_sheet = self.per_sheet();
        let printer = self.to_printer();
        egui::Grid::new("plotForm").num_columns(2).show(ui, |ui| {
            // Which sheets.
            ui.label(if page_setup { "" } else { "Sheets" });
            let count = self.set.sheets.len();
            let current = match self.set.sheets.get(self.current) {
                Some(sheet) => format!("Current sheet ({}, {})", self.current + 1, sheet.name),
                None => String::from("Current sheet"),
            };
            let summary = self.sheets_summary();
            row(ui, !page_setup, &mut |ui| {
                ui.vertical(|ui| {
                    if controls::radio(
                        ui,
                        "plotSheetsAll",
                        &format!("All {count} sheets"),
                        self.sheets == PlotSheets::All,
                    )
                    .clicked()
                    {
                        self.sheets = PlotSheets::All;
                    }
                    if controls::radio(
                        ui,
                        "plotSheetsCurrent",
                        &current,
                        self.sheets == PlotSheets::Current,
                    )
                    .clicked()
                    {
                        self.sheets = PlotSheets::Current;
                    }
                    ui.horizontal(|ui| {
                        if controls::radio(
                            ui,
                            "plotSheetsList",
                            "Sheets",
                            self.sheets == PlotSheets::List,
                        )
                        .clicked()
                        {
                            self.sheets = PlotSheets::List;
                        }
                        let listed = self.sheets == PlotSheets::List;
                        // Read-only rather than disabled while another choice is made (the
                        // C++ disabled it): a fill still writes it, as the C++ test's setText
                        // did, and the text typed chooses the list.
                        let typed = controls::line_with(
                            ui,
                            "plotSheetsText",
                            &mut self.list_text,
                            LineOptions {
                                placeholder: "1,3-5 or sheet ids",
                                read_only: !listed,
                                ..LineOptions::default()
                            },
                        );
                        // Typing a list chooses it.
                        if typed.changed() {
                            self.sheets = PlotSheets::List;
                        }
                    });
                    controls::label(ui, "plotSheetsSummary", &summary);
                });
            });
            ui.end_row();

            // What is made.
            ui.label(if page_setup { "" } else { "Output" });
            let formats: Vec<String> = FORMATS.iter().map(|(text, _)| String::from(*text)).collect();
            row(ui, !page_setup, &mut |ui| {
                controls::choice(ui, "plotFormat", &formats, &mut self.format);
            });
            ui.end_row();
            ui.label("Colours");
            let colours: Vec<String> =
                COLOUR_MODES.iter().map(|(text, _)| String::from(*text)).collect();
            controls::choice(ui, "plotColourMode", &colours, &mut self.colour_mode);
            ui.end_row();
            ui.label("Line weights");
            controls::spin(
                ui,
                "plotLineWeightScale",
                &mut self.line_weight_scale,
                SpinRange::new(MINIMUM_LINE_WEIGHT_SCALE, MAXIMUM_LINE_WEIGHT_SCALE, 0.1, 2)
                    .with_suffix(" x"),
            )
            .on_hover_text("Every line weight is multiplied by this");
            ui.end_row();
            ui.label("Resolution");
            controls::whole(
                ui,
                "plotDpi",
                &mut self.dpi,
                (MINIMUM_PLOT_DPI as i64)..=(MAXIMUM_PLOT_DPI as i64),
            )
            .on_hover_text("The resolution of an image, and of a PDF's 3D snapshot");
            ui.end_row();
            if page_setup {
                // The page setup's own choice of output; a plot chooses its format itself.
                ui.label("");
                controls::check(
                    ui,
                    "pageSetupFilePerSheet",
                    "A PDF per sheet, named by the pattern",
                    &mut self.file_per_sheet,
                );
                ui.end_row();
            }

            // Where it goes.
            ui.label("File");
            row(ui, !page_setup && !printer && !per_sheet, &mut |ui| {
                ui.horizontal(|ui| {
                    controls::line(ui, "plotFile", &mut self.file);
                    if controls::button(ui, "plotBrowseFile", "Browse...").clicked() && !headless {
                        let path = get_save_file_name("Plot to PDF", &self.file, "PDF (*.pdf)");
                        if !path.is_empty() {
                            self.file = to_native_separators(&path);
                        }
                    }
                });
            });
            ui.end_row();
            ui.label("Folder");
            row(ui, !page_setup && per_sheet, &mut |ui| {
                ui.horizontal(|ui| {
                    controls::line(ui, "plotFolder", &mut self.folder);
                    if controls::button(ui, "plotBrowseFolder", "Browse...").clicked() && !headless
                        && let Some(path) = rfd::FileDialog::new()
                            .set_title("Plot into Folder")
                            .set_directory(&self.folder)
                            .pick_folder()
                    {
                        self.folder = to_native_separators(&path.to_string_lossy());
                    }
                });
            });
            ui.end_row();
            ui.label("File names");
            row(ui, page_setup || per_sheet, &mut |ui| {
                controls::line(ui, "plotPattern", &mut self.pattern).on_hover_text(
                    "{n} the sheet's position, {n:02} padded to two, {N} the count, {set} the set \
                     number, {number} the sheet number, {name} the sheet's name, {id} its id",
                );
            });
            ui.end_row();
            ui.label("");
            let preview = self.pattern_preview();
            row(ui, page_setup || per_sheet, &mut |ui| {
                controls::label(ui, "plotPatternPreview", &preview);
            });
            ui.end_row();
            ui.label("Printer");
            row(ui, !page_setup && printer, &mut |ui| {
                controls::choice(ui, "plotPrinter", &self.printers, &mut self.printer);
            });
            ui.end_row();
        });
        row(ui, !page_setup, &mut |ui| {
            ui.add_enabled_ui(!printer, |ui| {
                controls::check(
                    ui,
                    "plotOpenAfter",
                    "Open the result afterwards",
                    &mut self.open_after,
                );
            });
            controls::check(
                ui,
                "plotKeepSetup",
                "Keep these settings as the project's page setup",
                &mut self.keep_setup,
            );
        });
        let problem = self.problem();
        controls::label_as(
            ui,
            "plotProblem",
            &problem,
            egui::Label::new(
                egui::RichText::new(problem.as_str()).color(egui::Color32::from_rgb(0xc0, 0x39, 0x2b)),
            )
            .wrap(),
        );
        if (self.sheets, self.format.clone(), self.list_text.clone()) != before {
            // The rows a choice shows are drawn at the next frame.
            ui.ctx().request_repaint();
        }
    }
}

// ---- running a plot ----------------------------------------------------------------------------

/// The progress a plot reports, a sheet at a time, and its Cancel (the C++'s QProgressDialog,
/// object name plotProgress): shared, so whoever shows it - the window, or a test standing in for
/// a person - reads it and presses Cancel while the plot runs.
#[derive(Clone, Debug, Default)]
pub struct PlotProgressDialog {
    state: Rc<RefCell<ProgressState>>,
}

#[derive(Clone, Debug, Default)]
struct ProgressState {
    label: String,
    value: usize,
    maximum: usize,
    cancelled: bool,
    shown: bool,
}

impl PlotProgressDialog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Cancel pressed: the plot stops before the next sheet.
    pub fn cancel(&self) {
        self.state.borrow_mut().cancelled = true;
    }

    pub fn was_cancelled(&self) -> bool {
        self.state.borrow().cancelled
    }

    /// Whether a plot is showing it now.
    pub fn is_shown(&self) -> bool {
        self.state.borrow().shown
    }

    pub fn label(&self) -> String {
        self.state.borrow().label.clone()
    }

    /// How many sheets are done, of how many.
    pub fn value(&self) -> (usize, usize) {
        let state = self.state.borrow();
        (state.value, state.maximum)
    }
}

/// Plots `request` from a copy of `set`, reporting to `progress` a sheet at a time; its Cancel
/// stops the plot before the next sheet (PlotReport::cancelled). With `printer` (a printer's
/// name, and the spool file the sheets are written to for it) the sheets are printed on it
/// instead (print_sheets_on).
pub fn plot_with_progress<'s>(
    set: &SheetSet,
    request: &PlotRequest,
    source: SheetSourceProvider<'_, 's>,
    printer: Option<(&dyn Printers, &str, &std::path::Path, bool)>,
    progress: &PlotProgressDialog,
) -> Result<PlotReport> {
    let snapshot = set.clone();
    {
        let mut state = progress.state.borrow_mut();
        state.label = String::from("Starting the plot...");
        state.shown = true;
    }
    let mut step = |done: usize, total: usize, name: &str| {
        let mut state = progress.state.borrow_mut();
        state.maximum = total;
        if done < total {
            state.label = format!("Plotting sheet {} of {total}: {name}", done + 1);
        }
        state.value = done;
        !state.cancelled
    };
    let mut cache = SheetPaintCache::new();
    let result = match printer {
        Some((printers, name, spool, headless)) => print_sheets_on(
            printers,
            name,
            spool,
            headless,
            &snapshot,
            request,
            source,
            &mut cache,
            Some(&mut step),
        ),
        None => plot_sheets(&snapshot, request, source, &mut cache, Some(&mut step)),
    };
    progress.state.borrow_mut().shown = false;
    result
}

/// The PDF a Plot dialog offers first: "<project name>.pdf" in the project's folder, or in the
/// user's documents folder for a drawing not saved as a project yet ("sheets.pdf" for a project
/// with no name). Written with '/', as QDir::filePath writes it.
pub fn suggested_plot_file(document: &Document) -> String {
    let mut name = sanitise_file_name(&document.metadata().name);
    if document.metadata().name.is_empty() {
        name = String::from("sheets");
    }
    let folder = match document.project_directory() {
        Some(project) => String::from_utf8_lossy(&generic_path_utf8(&project)).into_owned(),
        None => documents_folder(),
    };
    format!("{}/{name}.pdf", folder.trim_end_matches('/'))
}

/// QStandardPaths::DocumentsLocation: the user's Documents folder in their home.
fn documents_folder() -> String {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(std::path::PathBuf::from)
        .unwrap_or_default();
    String::from_utf8_lossy(&generic_path_utf8(&home.join("Documents"))).into_owned()
}

/// One plot at a time: a second Plot while one runs would paint over the first.
static BUSY: AtomicBool = AtomicBool::new(false);

/// Plot...: the dialog for `set`, opened (plotInteractively's first half; the plot runs when it is
/// accepted, finish_plot). None, and the reason said through `report`, while a plot runs or when
/// there are no sheets.
pub fn plot_interactively(
    set: &SheetSet,
    current: usize,
    all_sheets: bool,
    suggested_file: &str,
    report: &mut dyn FnMut(&str, bool),
) -> Option<PlotDialog> {
    if BUSY.load(Ordering::Acquire) {
        report("A plot is already running.", true);
        return None;
    }
    if set.sheets.is_empty() {
        report("There are no sheets to plot. Generate some first.", true);
        return None;
    }
    Some(PlotDialog::new(
        set,
        current,
        all_sheets,
        suggested_file,
        PlotDialogMode::Plot,
    ))
}

/// The plot an accepted Plot dialog asks for (plotInteractively's second half): when asked, the
/// choice kept as `document`'s page setup (one undoable step - none when nothing changed); then
/// the plot with its progress; the summary, every problem and any error said through `report`.
/// The file or folder to open afterwards, when the dialog asked for it.
#[allow(
    clippy::too_many_arguments,
    reason = "plotInteractively's arguments, with the printers and the progress the window owns."
)]
pub fn finish_plot(
    dialog: &PlotDialog,
    set: &SheetSet,
    title: &str,
    source: &crate::sheet_editor::SourceProvider,
    document: &mut Document,
    printers: &dyn Printers,
    progress: &PlotProgressDialog,
    report: &mut dyn FnMut(&str, bool),
) -> Option<String> {
    // A copy: `set` may be the document's own, which keeping the page setup replaces.
    let set = set.clone();
    let mut request = dialog.request();
    request.title = String::from(title);
    if dialog.keep_as_page_setup() {
        let kept = page_setup_for(&request, &set.page_setup);
        if kept != set.page_setup {
            match set_page_setup(document, &kept) {
                Err(error) => report(&error.describe(), true),
                Ok(()) => report("Kept as the project's page setup.", false),
            }
        }
    }
    let spool = std::env::temp_dir().join("katana-print.pdf");
    let printer = dialog.to_printer().then_some(dialog.printer_name());
    if let Some(name) = &printer
        && !printers.names().contains(name)
    {
        // A printer taken away since the dialog listed it would otherwise quietly become the
        // default one.
        report(&format!("There is no printer named {name}."), true);
        return None;
    }
    BUSY.store(true, Ordering::Release);
    let drawing: &Document = document;
    let result = plot_with_progress(
        &set,
        &request,
        &mut || source(drawing),
        printer
            .as_deref()
            .map(|name| (printers, name, spool.as_path(), false)),
        progress,
    );
    BUSY.store(false, Ordering::Release);
    let result = match result {
        Ok(result) => result,
        Err(error) => {
            report(&error.describe(), true);
            return None;
        }
    };
    for problem in &result.problems {
        report(problem, true);
    }
    for (index, scale) in &result.shrunk {
        report(
            &format!(
                "{} was printed at {}% to fit the printer's paper.",
                set.sheets[*index].name,
                (scale * 100.0).round()
            ),
            false,
        );
    }
    report(&result.summary(&request), false);
    if dialog.open_afterwards() && !result.cancelled && !result.files.is_empty() {
        return Some(if plots_file_per_sheet(request.format) {
            request.destination.clone()
        } else {
            result.files[0].clone()
        });
    }
    None
}

// ---- the editor's side -------------------------------------------------------------------------

impl crate::sheet_editor::SheetEditor {
    /// The Plot or Page Setup dialog, when one is open.
    pub fn plot_dialog(&mut self) -> Option<&mut PlotDialog> {
        self.plot_dialog.as_mut()
    }

    /// The Plot and Page Setup dialogs, a frame each; an accepted one does its work.
    pub(crate) fn plot_dialogs_ui(&mut self, ctx: &egui::Context, host: &mut dyn crate::host::Host) {
        let Some(mut dialog) = self.plot_dialog.take() else {
            return;
        };
        match dialog.ui(ctx, self.headless) {
            PlotAnswer::Open => self.plot_dialog = Some(dialog),
            PlotAnswer::Cancelled => {}
            PlotAnswer::Accepted => self.accept_plot_dialog(ctx, host, &dialog),
        }
    }

    pub(crate) fn accept_plot_dialog(
        &mut self,
        ctx: &egui::Context,
        host: &mut dyn crate::host::Host,
        dialog: &PlotDialog,
    ) {
        if dialog.mode == PlotDialogMode::PageSetup {
            let _ = self.run_line(host, &dialog.page_setup_line());
            return;
        }
        let set = host.document().sheet_set();
        let title = host.document().metadata().name.clone();
        let source = std::rc::Rc::clone(&self.source);
        let mut said: Vec<(String, bool)> = Vec::new();
        let open = finish_plot(
            dialog,
            &set,
            &title,
            &source,
            host.document_mut(),
            &NoPrinters,
            &PlotProgressDialog::new(),
            &mut |text: &str, error: bool| said.push((String::from(text), error)),
        );
        for (text, error) in said {
            self.report(host, &text, error);
        }
        if let Some(target) = open {
            ctx.open_url(egui::OpenUrl::new_tab(format!("file:///{}", target.trim_start_matches('/'))));
        }
        self.observe(&*host);
    }
}
