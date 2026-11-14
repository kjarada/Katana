//! A sheet set, plotted: to one PDF, to a PDF a sheet, to PNG or TIFF rasters, or to a printer
//! (docs/plotting.md, "Plot styles and output"). The port of plotting/plot_output.
//!
//! Everything a plot needs is in a PlotRequest - which sheets, the format, the plot style, the
//! resolution, where the files go and how they are named - so the plot dialog, the command line
//! and an agent all make the same call and get the same files. Nothing here shows a window: the
//! dialog and the progress bar (plot_dialog) are a front end over plot_sheets.
//!
//! ```text
//! Pdf           one vector PDF at `destination`, a page a sheet, each page the size of its
//!               sheet's paper
//! PdfPerSheet   a vector PDF a sheet in the folder `destination`, each named by the file-name
//!               pattern (sheet_file_names)
//! Png, Tiff     a raster a sheet at `dpi`, named the same way; greyscale when the colour mode is
//!               not Colour, the resolution recorded in the file so it prints at its scale
//! print_sheets  the sheets as a PDF for a printer, a page each, on the sheet's paper when the
//!               printer has it and scaled down to fit when not
//! ```
//!
//! PROGRESS AND CANCEL. `progress` is called before each sheet and may return false to stop; the
//! sheets finished by then are reported. Every file is written under a temporary name and takes
//! its own only when it is complete (SaveFile, QSaveFile's place), so a cancelled or failed single
//! PDF is not written at all - half a set is not a set - and an earlier file of that name is left
//! as it was; the files of a per-sheet plot finished before the cancel are kept and listed.
//!
//! The source is asked for again before each sheet, so a plot that lets the window run between
//! sheets never paints from a surface that has since been replaced.
//!
//! PRINTING (the owner's decision of 2026-11-05: "write the PDF, hand it to the operating
//! system"). print_sheets is QPrinter's place: it writes the sheets as the pages of a PDF, each
//! page what the printer gives for its sheet, a sheet larger than its page scaled down to fit and
//! centred. Handing that PDF to the operating system's print queue is hand_to_print_queue, which
//! is not written yet: it answers Unsupported with a sentence, headless and not. The printers the
//! system has are asked of a Printers; the port lists none yet (NoPrinters).
//!
//! WHAT DIFFERS FROM THE C++, and why. A PDF's title and creator are not written (the canvas's
//! PDF document has no place for them yet). A PNG's resolution is put into the bytes the canvas's
//! encoder made, as a pHYs chunk: the encoder takes no resolution.

#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "The C++'s !(a > b) is kept: it rejects NaN, which a > b alone would let through."
)]

use std::path::{Path, PathBuf};

use katana_cad::plot::{PaperSize, PlotColourMode, millimetres_to_pixels, paper_dimensions};
use katana_cad::plotting::page_setup::{
    DEFAULT_FILE_NAME_PATTERN, PageSetup, parse_sheet_selection, sheet_file_names,
    validate_page_setup,
};
use katana_cad::plotting::sheet_set::{Sheet, SheetSet};
use katana_cad::plotting::sheet_verbs::PlotSheetsRequest;
use katana_core::error::make_error;
use katana_core::path_text::{path_from_utf8, path_to_utf8};
use katana_core::text::{format_fixed, format_general};
use katana_core::{Error, ErrorCode, Result, Status};
use katana_update::journal::crc32;

use crate::canvas::{
    Canvas, Color, Image, PdfCanvas, PdfDocument, PointF, RasterCanvas, encode_png,
};
use crate::plan_painter::{PlanMedium, pdf_resolution_for};
use crate::plotting::plot_style::greyscale_raster;
use crate::plotting::tiff_writer::write_tiff;
use crate::sheet_painter::{
    SheetPaintCache, SheetPaintOptions, SheetPaintStats, SheetSource, paint_sheet, pdf_page_for,
};

/// Stored and typed by name ("pdf", "pdfs", "png", "tiff"), so values may be added at the end
/// without changing what a name means.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PlotFormat {
    /// One PDF, a page a sheet.
    #[default]
    Pdf,
    /// A PDF a sheet.
    PdfPerSheet,
    /// A PNG a sheet.
    Png,
    /// A TIFF a sheet.
    Tiff,
}

/// The format's name (the C++'s toString).
pub fn plot_format_name(format: PlotFormat) -> &'static str {
    match format {
        PlotFormat::Pdf => "pdf",
        PlotFormat::PdfPerSheet => "pdfs",
        PlotFormat::Png => "png",
        PlotFormat::Tiff => "tiff",
    }
}

/// plot_format_name's names, and "pdf-per-sheet", "tif"; case is ignored.
pub fn plot_format_from(name: &str) -> Option<PlotFormat> {
    match name.to_ascii_lowercase().as_str() {
        "pdf" => Some(PlotFormat::Pdf),
        "pdfs" | "pdf-per-sheet" => Some(PlotFormat::PdfPerSheet),
        "png" => Some(PlotFormat::Png),
        "tiff" | "tif" => Some(PlotFormat::Tiff),
        _ => None,
    }
}

/// ".pdf", ".png", ".tif".
pub fn plot_format_extension(format: PlotFormat) -> &'static str {
    match format {
        PlotFormat::Pdf | PlotFormat::PdfPerSheet => ".pdf",
        PlotFormat::Png => ".png",
        PlotFormat::Tiff => ".tif",
    }
}

/// Whether the format writes a file a sheet into a folder.
pub fn plots_file_per_sheet(format: PlotFormat) -> bool {
    format != PlotFormat::Pdf
}

/// The most pixels a raster sheet may have: an A0 sheet at 350 dpi, an A1 at 500, an A3 at 1000 -
/// 800 MB while it is painted. Beyond it the plot is refused and told the resolution that fits,
/// rather than failing half-way for want of memory.
pub const MAXIMUM_RASTER_PIXELS: f64 = 200.0e6;

#[derive(Clone, Debug, PartialEq)]
pub struct PlotRequest {
    /// Which sheets, as parse_sheet_selection reads them: "" or "all", "1,3-5", sheet ids.
    pub sheets: String,
    pub format: PlotFormat,
    pub colour_mode: PlotColourMode,
    pub line_weight_scale: f64,
    pub dpi: f64,
    /// The PDF file for Pdf; the folder the files go in for the others, made when it is missing.
    /// Ignored by print_sheets.
    pub destination: String,
    pub file_name_pattern: String,
    /// The PDF's title; the file's own name when empty.
    pub title: String,
    /// A PDF written as PdfDocument::uncompressed writes it, for a test that reads it
    /// (docs/rust.md, "A test reads a PDF written readable"). A plot is compressed.
    pub readable_pdf: bool,
}

impl Default for PlotRequest {
    fn default() -> Self {
        Self {
            sheets: String::new(),
            format: PlotFormat::Pdf,
            colour_mode: PlotColourMode::Colour,
            line_weight_scale: 1.0,
            dpi: 300.0,
            destination: String::new(),
            file_name_pattern: String::from(DEFAULT_FILE_NAME_PATTERN),
            title: String::new(),
            readable_pdf: false,
        }
    }
}

/// A request as the set's page setup would make it: its style, resolution, naming and
/// (file_per_sheet) format, to `destination`, for `sheets`.
pub fn plot_request_for(setup: &PageSetup, destination: &str, sheets: &str) -> PlotRequest {
    PlotRequest {
        sheets: String::from(sheets),
        format: if setup.file_per_sheet {
            PlotFormat::PdfPerSheet
        } else {
            PlotFormat::Pdf
        },
        colour_mode: setup.colour_mode,
        line_weight_scale: setup.line_weight_scale,
        dpi: setup.dpi,
        destination: String::from(destination),
        file_name_pattern: setup.file_name_pattern.clone(),
        ..PlotRequest::default()
    }
}

/// The request the PLOTSHEETS verb's words make (cad's parse_plot_sheets): the page setup's, with
/// what the words give in its place. The format is one PDF unless the words name one, as
/// --plot-sheets has it.
pub fn plot_request_for_words(setup: &PageSetup, words: &PlotSheetsRequest) -> PlotRequest {
    let destination = String::from_utf8_lossy(&path_to_utf8(&words.path)).into_owned();
    let mut request = plot_request_for(setup, &destination, &words.sheets);
    request.format = plot_format_from(words.format.as_deref().unwrap_or("pdf"))
        .unwrap_or(PlotFormat::Pdf);
    if let Some(mode) = words.colour_mode {
        request.colour_mode = mode;
    }
    if let Some(dpi) = words.dpi {
        request.dpi = dpi;
    }
    if let Some(scale) = words.line_weight_scale {
        request.line_weight_scale = scale;
    }
    if let Some(pattern) = &words.file_name_pattern {
        request.file_name_pattern.clone_from(pattern);
    }
    request
}

/// The page setup a request amounts to: `base` with the request's style, resolution, naming and
/// whether it is a PDF a sheet. The format of a raster plot is not a page setup's to keep, so a
/// PNG or TIFF request keeps `base`'s file_per_sheet.
pub fn page_setup_for(request: &PlotRequest, base: &PageSetup) -> PageSetup {
    let mut setup = base.clone();
    setup.colour_mode = request.colour_mode;
    setup.line_weight_scale = request.line_weight_scale;
    setup.dpi = request.dpi;
    setup.file_name_pattern.clone_from(&request.file_name_pattern);
    match request.format {
        PlotFormat::Pdf => setup.file_per_sheet = false,
        PlotFormat::PdfPerSheet => setup.file_per_sheet = true,
        PlotFormat::Png | PlotFormat::Tiff => {}
    }
    setup
}

// ---- paths as a person reads them ---------------------------------------------------------------

/// QDir::toNativeSeparators: backslashes on Windows, the path as it is elsewhere.
fn native_separators(path: &str) -> String {
    if cfg!(windows) {
        path.replace('/', "\\")
    } else {
        String::from(path)
    }
}

/// QDir(folder).filePath(name): the name in the folder, joined with '/'.
fn file_in(folder: &str, name: &str) -> String {
    if folder.is_empty() {
        String::from(name)
    } else if folder.ends_with('/') || folder.ends_with('\\') {
        format!("{folder}{name}")
    } else {
        format!("{folder}/{name}")
    }
}

fn sheets_text(count: usize) -> String {
    format!("{count} sheet{}", if count == 1 { "" } else { "s" })
}

// ---- a file that takes its name only when it is whole -------------------------------------------

/// QSaveFile: the bytes go to a file beside the target and are moved onto it when they are all
/// there, so a write that fails or is cancelled leaves what the target held.
struct SaveFile {
    target: PathBuf,
    part: PathBuf,
}

impl SaveFile {
    /// Opens the file beside `target`. Refused when the target is a folder or its folder takes no
    /// file.
    fn open(target: &Path) -> std::io::Result<Self> {
        if target.is_dir() {
            return Err(std::io::Error::other("a folder has that name"));
        }
        let mut name = target.file_name().unwrap_or_default().to_os_string();
        name.push(format!(".{}.part", std::process::id()));
        let part = target.with_file_name(name);
        std::fs::File::create(&part)?;
        Ok(Self {
            target: target.to_path_buf(),
            part,
        })
    }

    /// QSaveFile::cancelWriting.
    fn cancel(self) {
        let _ = std::fs::remove_file(&self.part);
    }

    /// QSaveFile::commit.
    fn commit(self, bytes: &[u8]) -> std::io::Result<()> {
        let written = std::fs::write(&self.part, bytes)
            .and_then(|()| std::fs::rename(&self.part, &self.target));
        if written.is_err() {
            let _ = std::fs::remove_file(&self.part);
        }
        written
    }
}

// ---- one page -----------------------------------------------------------------------------------

/// The paint options a request's plot style asks for, at `pixels_per_millimetre`.
fn paint_options(request: &PlotRequest, pixels_per_millimetre: f64, dpi: f64) -> SheetPaintOptions {
    let mut options = SheetPaintOptions {
        medium: PlanMedium::Paper,
        pixels_per_millimetre,
        ..SheetPaintOptions::default()
    };
    options.plot.dpi = dpi;
    options.plot.colour_mode = request.colour_mode;
    options.plot.line_weight_scale = request.line_weight_scale;
    options
}

fn collect_problems(
    report: &mut PlotReport,
    set: &SheetSet,
    index: usize,
    stats: &SheetPaintStats,
) {
    for line in &stats.problems {
        report
            .problems
            .push(format!("{} - {line}", set.sheets[index].name));
    }
}

/// Paints sheet `index` onto a canvas whose device has the options' resolution, at `offset`, then
/// scaled by `scale` (a PDF's resolution rounding, a printer's fit).
#[allow(
    clippy::too_many_arguments,
    reason = "The C++ paintPage's arguments, the canvas first."
)]
fn paint_page(
    canvas: &mut dyn Canvas,
    set: &SheetSet,
    index: usize,
    source: &SheetSource<'_>,
    options: &SheetPaintOptions,
    cache: &mut SheetPaintCache,
    offset: PointF,
    scale: f64,
) -> SheetPaintStats {
    canvas.save();
    canvas.translate(offset.x, offset.y);
    if scale != 1.0 {
        canvas.scale(scale, scale);
    }
    let stats = paint_sheet(canvas, set, index, source, options, cache);
    canvas.restore();
    stats
}

/// The pixel size of sheet `sheet` at `dpi`.
fn raster_size(sheet: &Sheet, dpi: f64) -> (i64, i64) {
    let paper = paper_dimensions(sheet.paper, sheet.landscape);
    (
        (paper.width_mm * dpi / 25.4).round() as i64,
        (paper.height_mm * dpi / 25.4).round() as i64,
    )
}

/// The request's style and resolution, checked as a page setup is; the file-name pattern only
/// when there are files to name.
fn validate_style(request: &PlotRequest) -> Status {
    let mut setup = PageSetup {
        colour_mode: request.colour_mode,
        line_weight_scale: request.line_weight_scale,
        dpi: request.dpi,
        ..PageSetup::default()
    };
    if plots_file_per_sheet(request.format) {
        setup.file_name_pattern.clone_from(&request.file_name_pattern);
    }
    validate_page_setup(&setup)
}

// ---- progress -----------------------------------------------------------------------------------

/// Called before sheet `done` + 1 of `total` (`name` is that sheet's name) and once more with
/// done == total at the end. Returning false cancels the plot.
pub type PlotProgress<'p> = &'p mut dyn FnMut(usize, usize, &str) -> bool;
/// What the sheets are drawn from, asked for before each sheet.
pub type SheetSourceProvider<'p, 's> = &'p mut dyn FnMut() -> SheetSource<'s>;

/// Asks `progress` whether to go on; a no is remembered in the report.
struct Progress<'p> {
    progress: Option<PlotProgress<'p>>,
    total: usize,
    done: usize,
}

impl Progress<'_> {
    fn next(&mut self, name: &str, report: &mut PlotReport) -> bool {
        if let Some(progress) = self.progress.as_mut()
            && !progress(self.done, self.total, name)
        {
            report.cancelled = true;
            return false;
        }
        true
    }

    fn done(&mut self) {
        self.done += 1;
    }

    fn finish(&mut self, report: &PlotReport) {
        if let Some(progress) = self.progress.as_mut()
            && !report.cancelled
        {
            let _ = progress(self.done, self.total, "");
        }
    }
}

// ---- writing ------------------------------------------------------------------------------------

/// Sheets `pages` of `set` into one PDF at `path`. The file is written through a SaveFile and
/// takes its name only when its last page is done: a plot cancelled or failed part of the way
/// leaves no half a set behind, and an earlier file of that name as it was.
#[allow(
    clippy::too_many_arguments,
    reason = "The C++ writePdf's arguments, less the title the canvas's PDF does not write."
)]
fn write_pdf<'s>(
    path: &str,
    set: &SheetSet,
    pages: &[usize],
    request: &PlotRequest,
    source: SheetSourceProvider<'_, 's>,
    cache: &mut SheetPaintCache,
    progress: &mut Progress<'_>,
    report: &mut PlotReport,
) -> Status {
    let resolution = pdf_resolution_for(request.dpi);
    let options = paint_options(
        request,
        millimetres_to_pixels(1.0, request.dpi),
        request.dpi,
    );
    let target = path_from_utf8(path);
    let mut open: Option<(SaveFile, PdfDocument)> = None;
    let mut plotted = Vec::new();
    for &index in pages {
        let sheet = &set.sheets[index];
        if !progress.next(&sheet.name, report) {
            break;
        }
        if open.is_none() {
            let file = SaveFile::open(&target).map_err(|error| {
                make_error(
                    ErrorCode::FileExportFailure,
                    format!("could not open the PDF for writing: {error}"),
                    path,
                )
            })?;
            let document = if request.readable_pdf {
                PdfDocument::uncompressed()
            } else {
                PdfDocument::new()
            };
            open = Some((file, document));
        }
        let mut canvas = pdf_page_for(sheet, resolution.resolution);
        let stats = paint_page(
            &mut canvas,
            set,
            index,
            &source(),
            &options,
            cache,
            PointF::new(0.0, 0.0),
            resolution.scale,
        );
        let added = match open.as_mut() {
            Some((_, document)) => document.add_page(canvas).is_ok(),
            None => false,
        };
        if !added {
            if let Some((file, _)) = open.take() {
                file.cancel();
            }
            return Err(make_error(
                ErrorCode::FileExportFailure,
                "could not start a page",
                path,
            ));
        }
        collect_problems(report, set, index, &stats);
        plotted.push(index);
        progress.done();
    }
    report.sheets.extend(plotted);
    let Some((file, document)) = open else {
        return Ok(()); // cancelled before the first page: nothing was opened
    };
    if report.cancelled {
        file.cancel(); // half a set is not a set
        return Ok(());
    }
    let finished = match document.finish() {
        Ok(bytes) => file.commit(&bytes).map_err(|error| format!("{error}")),
        Err(error) => {
            file.cancel();
            Err(error.describe())
        }
    };
    if let Err(why) = finished {
        return Err(make_error(
            ErrorCode::FileExportFailure,
            format!("could not finish the PDF: {why}"),
            path,
        ));
    }
    report.files.push(String::from(path));
    Ok(())
}

/// A PNG's bytes with the resolution it prints at recorded: a pHYs chunk, in dots a metre, put
/// after the header chunk. The canvas's encoder takes no resolution, so it is added to what it
/// wrote; bytes that are not a PNG with its header first are returned as they are.
fn png_with_resolution(png: Vec<u8>, dots_per_metre: u32) -> Vec<u8> {
    // The signature's 8 bytes, then IHDR: a length (13), its name, 13 bytes and a checksum.
    const AFTER_HEADER: usize = 8 + 4 + 4 + 13 + 4;
    if png.len() < AFTER_HEADER || &png[12..16] != b"IHDR" {
        return png;
    }
    let mut chunk = Vec::with_capacity(21);
    chunk.extend_from_slice(&9_u32.to_be_bytes());
    chunk.extend_from_slice(b"pHYs");
    chunk.extend_from_slice(&dots_per_metre.to_be_bytes());
    chunk.extend_from_slice(&dots_per_metre.to_be_bytes());
    chunk.push(1); // the unit is the metre
    let checksum = crc32(&chunk[4..]);
    chunk.extend_from_slice(&checksum.to_be_bytes());
    let mut out = Vec::with_capacity(png.len() + chunk.len());
    out.extend_from_slice(&png[..AFTER_HEADER]);
    out.extend_from_slice(&chunk);
    out.extend_from_slice(&png[AFTER_HEADER..]);
    out
}

/// Sheet `index` as a raster at the request's resolution, written as `path`: RGB in colour, one
/// grey channel otherwise (greyscale_raster), with the resolution recorded so it prints at its
/// scale.
fn write_raster(
    path: &str,
    set: &SheetSet,
    index: usize,
    request: &PlotRequest,
    source: &SheetSource<'_>,
    cache: &mut SheetPaintCache,
    report: &mut PlotReport,
) -> Status {
    let sheet = &set.sheets[index];
    let (width, height) = raster_size(sheet, request.dpi);
    let (Ok(pixels_wide), Ok(pixels_high)) = (u32::try_from(width), u32::try_from(height)) else {
        return Err(make_error(
            ErrorCode::FileExportFailure,
            format!("not enough memory for a {width} x {height} raster"),
            path,
        ));
    };
    let mut image = Image::new(pixels_wide, pixels_high);
    image.fill(Color::WHITE);
    let options = paint_options(request, request.dpi / 25.4, request.dpi);
    let mut stats = SheetPaintStats::default();
    RasterCanvas::paint_image(&mut image, &mut |canvas: &mut dyn Canvas| {
        stats = paint_page(
            canvas,
            set,
            index,
            source,
            &options,
            cache,
            PointF::new(0.0, 0.0),
            1.0,
        );
    });
    collect_problems(report, set, index, &stats);
    // Every colour is a grey already; a grey image is a third of the size.
    if request.colour_mode != PlotColourMode::Colour {
        image = greyscale_raster(&image);
    }
    let target = path_from_utf8(path);
    if request.format == PlotFormat::Tiff {
        return write_tiff(&image, &target, request.dpi);
    }
    // Through a SaveFile, as the PDF is: a failed write leaves no broken file.
    let dots_per_metre = (request.dpi / 0.0254).round() as u32;
    let written = SaveFile::open(&target)
        .map_err(|error| format!("{error}"))
        .and_then(|file| match encode_png(&image) {
            Ok(png) => file
                .commit(&png_with_resolution(png, dots_per_metre))
                .map_err(|error| format!("{error}")),
            Err(error) => {
                file.cancel();
                Err(error.describe())
            }
        });
    written.map_err(|why| {
        make_error(
            ErrorCode::FileExportFailure,
            format!("could not write the PNG file: {why}"),
            path,
        )
    })
}

// ---- checking -----------------------------------------------------------------------------------

/// What a request asks for, checked without writing anything: the selection against `set`, the
/// style and resolution (validate_page_setup), a destination, and a raster's size
/// (MAXIMUM_RASTER_PIXELS). InvalidArgument saying what.
pub fn validate_plot_request(set: &SheetSet, request: &PlotRequest) -> Status {
    let selection = parse_sheet_selection(&request.sheets, set)?;
    validate_style(request)?;
    if request.destination.trim().is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            if plots_file_per_sheet(request.format) {
                "choose the folder the plot's files go in"
            } else {
                "choose the PDF file the plot goes in"
            },
        ));
    }
    if request.format == PlotFormat::Png || request.format == PlotFormat::Tiff {
        for &index in &selection {
            let sheet = &set.sheets[index];
            let (width, height) = raster_size(sheet, request.dpi);
            let pixels = width as f64 * height as f64;
            if pixels > MAXIMUM_RASTER_PIXELS {
                let paper = paper_dimensions(sheet.paper, sheet.landscape);
                let fits = ((MAXIMUM_RASTER_PIXELS / (paper.width_mm * paper.height_mm)).sqrt()
                    * 25.4)
                    .floor();
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    format!(
                        "sheet {} ({}) at {} dpi is {} megapixels, more than the {} a raster may \
                         be: plot it at {} dpi or less",
                        index + 1,
                        sheet.name,
                        format_general(request.dpi, 6),
                        format_fixed(pixels / 1.0e6, 0),
                        format_fixed(MAXIMUM_RASTER_PIXELS / 1.0e6, 0),
                        format_general(fits, 6),
                    ),
                ));
            }
        }
    }
    Ok(())
}

/// The files `request` would write, in order, without writing them: the one PDF, or a file a
/// selected sheet. validate_plot_request's errors.
pub fn planned_files(set: &SheetSet, request: &PlotRequest) -> Result<Vec<String>> {
    validate_plot_request(set, request)?;
    if !plots_file_per_sheet(request.format) {
        return Ok(vec![request.destination.clone()]);
    }
    let selection = parse_sheet_selection(&request.sheets, set)?;
    let extension = plot_format_extension(request.format);
    Ok(
        sheet_file_names(set, &selection, &request.file_name_pattern)
            .iter()
            .map(|name| file_in(&request.destination, &format!("{name}{extension}")))
            .collect(),
    )
}

// ---- what a plot did ----------------------------------------------------------------------------

/// What a plot did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlotReport {
    /// Written, in order.
    pub files: Vec<String>,
    /// The sheets plotted, as indices, in order.
    pub sheets: Vec<usize>,
    /// How many the selection named.
    pub sheets_asked: usize,
    /// "<sheet name> - vp3: no alignment named X".
    pub problems: Vec<String>,
    pub cancelled: bool,
    /// Scaled to fit the printer's paper: a factor below 1 for each sheet printed smaller than
    /// its paper, by sheet index. Files are never scaled.
    pub shrunk: Vec<(usize, f64)>,
    /// print_sheets: the printer's name; empty for a plot to files.
    pub printer: String,
}

impl PlotReport {
    /// One line for a log: "Plotted 3 sheets to C:/out/set.pdf.", "Plotted 3 sheets to 3 PNG
    /// files in C:/out.", "Printed 2 sheets on Office." or "Cancelled after 2 of 5 sheets; 2
    /// files kept in C:/out."
    pub fn summary(&self, request: &PlotRequest) -> String {
        let files = |count: usize| if count == 1 { "" } else { "s" };
        if self.cancelled {
            let mut text = format!(
                "Cancelled after {} of {}.",
                self.sheets.len(),
                sheets_text(self.sheets_asked)
            );
            if !self.files.is_empty() && plots_file_per_sheet(request.format) {
                text.pop();
                text.push_str(&format!(
                    "; {} file{} kept in {}.",
                    self.files.len(),
                    files(self.files.len()),
                    native_separators(&request.destination)
                ));
            }
            return text;
        }
        if !self.printer.is_empty() {
            return format!(
                "Printed {} on {}.",
                sheets_text(self.sheets.len()),
                self.printer
            );
        }
        if !plots_file_per_sheet(request.format) {
            return format!(
                "Plotted {} to {}.",
                sheets_text(self.sheets.len()),
                native_separators(&request.destination)
            );
        }
        let kind = match request.format {
            PlotFormat::PdfPerSheet => "PDF",
            PlotFormat::Png => "PNG",
            PlotFormat::Pdf | PlotFormat::Tiff => "TIFF",
        };
        format!(
            "Plotted {} to {} {kind} file{} in {}.",
            sheets_text(self.sheets.len()),
            self.files.len(),
            files(self.files.len()),
            native_separators(&request.destination)
        )
    }
}

fn make_folder(folder: &Path, shown: &str) -> Status {
    if folder.as_os_str().is_empty() {
        return Ok(()); // the folder the program runs in
    }
    std::fs::create_dir_all(folder).map_err(|_| {
        make_error(
            ErrorCode::FileExportFailure,
            "could not make the folder",
            shown,
        )
    })
}

/// Plots the sheets of `set` that `request` selects, in the order selected.
/// validate_plot_request's errors, and FileExportFailure (naming the file) when a file cannot be
/// written; files finished before a failure are kept, and the error's message ends by listing
/// them ("; 2 files written before it were kept: ...").
pub fn plot_sheets<'s>(
    set: &SheetSet,
    request: &PlotRequest,
    source: SheetSourceProvider<'_, 's>,
    cache: &mut SheetPaintCache,
    progress: Option<PlotProgress<'_>>,
) -> Result<PlotReport> {
    let files = planned_files(set, request)?;
    let selection = parse_sheet_selection(&request.sheets, set)?;
    let mut report = PlotReport {
        sheets_asked: selection.len(),
        ..PlotReport::default()
    };
    let mut steps = Progress {
        progress,
        total: selection.len(),
        done: 0,
    };

    if request.format == PlotFormat::Pdf {
        let target = path_from_utf8(&request.destination);
        let folder = target.parent().unwrap_or(Path::new(""));
        make_folder(
            folder,
            &String::from_utf8_lossy(&path_to_utf8(folder)),
        )?;
        write_pdf(
            &request.destination,
            set,
            &selection,
            request,
            source,
            cache,
            &mut steps,
            &mut report,
        )?;
        steps.finish(&report);
        return Ok(report);
    }

    make_folder(&path_from_utf8(&request.destination), &request.destination)?;
    // A file that cannot be written stops the plot; the error says which files before it were
    // written and kept, since the report that would have listed them is not returned.
    let failed = |mut error: Error, report: &PlotReport| {
        if !report.files.is_empty() {
            let one = report.files.len() == 1;
            error.message.push_str(&format!(
                "; {} file{} written before it {} kept:",
                report.files.len(),
                if one { "" } else { "s" },
                if one { "was" } else { "were" }
            ));
            for (at, file) in report.files.iter().enumerate() {
                error.message.push(' ');
                error.message.push_str(&native_separators(file));
                if at + 1 != report.files.len() {
                    error.message.push(',');
                }
            }
        }
        error
    };
    for (n, &index) in selection.iter().enumerate() {
        let path = &files[n];
        if request.format == PlotFormat::PdfPerSheet {
            // One page: write_pdf asks about the sheet itself.
            if let Err(error) = write_pdf(
                path,
                set,
                &[index],
                request,
                source,
                cache,
                &mut steps,
                &mut report,
            ) {
                return Err(failed(error, &report));
            }
            if report.cancelled {
                break;
            }
            continue;
        }
        if !steps.next(&set.sheets[index].name, &mut report) {
            break;
        }
        let from = source();
        if let Err(error) = write_raster(path, set, index, request, &from, cache, &mut report) {
            return Err(failed(error, &report));
        }
        report.sheets.push(index);
        report.files.push(path.clone());
        steps.done();
    }
    steps.finish(&report);
    Ok(report)
}

/// The same from one fixed source.
pub fn plot_sheets_from(
    set: &SheetSet,
    request: &PlotRequest,
    source: &SheetSource<'_>,
    cache: &mut SheetPaintCache,
    progress: Option<PlotProgress<'_>>,
) -> Result<PlotReport> {
    plot_sheets(set, request, &mut || source.clone(), cache, progress)
}

// ---- printing -----------------------------------------------------------------------------------

/// The printer print_sheets prints on: the PDF its pages are written to - what the operating
/// system is handed - and what the printer answers about itself. QPrinter's place.
#[derive(Clone, Debug, PartialEq)]
pub struct PdfPrinter {
    /// The PDF the pages are written to.
    pub path: PathBuf,
    /// The printer's name for the report; empty for a printer that is the PDF itself, which is
    /// reported by its path.
    pub name: String,
    /// Dots an inch. QPrinter::HighResolution is 1200.
    pub resolution: i32,
    /// The largest paper the printer has, in millimetres, the longer side first; None takes any
    /// paper, as a PDF does. A sheet larger than it is scaled down to fit a page of it.
    pub largest_paper_mm: Option<(f64, f64)>,
    /// The PDF written readable, for a test that reads it.
    pub readable: bool,
}

impl PdfPrinter {
    /// A printer that is the PDF at `path`, at the high resolution, taking any paper.
    pub fn for_file(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
            name: String::new(),
            resolution: 1200,
            largest_paper_mm: None,
            readable: false,
        }
    }

    /// The page the printer gives for `sheet`, in millimetres: the sheet's own paper when the
    /// printer has it, else its largest, turned as the sheet is.
    fn page_for(&self, sheet: &Sheet) -> (f64, f64) {
        let paper = paper_dimensions(sheet.paper, sheet.landscape);
        let Some((long, short)) = self.largest_paper_mm else {
            return (paper.width_mm, paper.height_mm);
        };
        let sheet_long = katana_math::cpp::max(paper.width_mm, paper.height_mm);
        let sheet_short = katana_math::cpp::min(paper.width_mm, paper.height_mm);
        if sheet_long <= long && sheet_short <= short {
            (paper.width_mm, paper.height_mm)
        } else if sheet.landscape {
            (long, short)
        } else {
            (short, long)
        }
    }
}

/// The ISO name of a paper, which a printer driver picks its tray by.
pub fn printer_paper_name(paper: PaperSize) -> &'static str {
    match paper {
        PaperSize::A0 => "A0",
        PaperSize::A1 => "A1",
        PaperSize::A2 => "A2",
        PaperSize::A3 => "A3",
        PaperSize::A4 => "A4",
    }
}

/// Prints the selected sheets on `printer`, a page each, in the request's style. Each page is
/// asked for at its sheet's paper size and orientation; when the printer gives a smaller page the
/// sheet is scaled down to fit it, centred, and recorded in PlotReport::shrunk. The printer's own
/// resolution is used, not the request's. FileExportFailure when the printer cannot be started.
pub fn print_sheets<'s>(
    printer: &PdfPrinter,
    set: &SheetSet,
    request: &PlotRequest,
    source: SheetSourceProvider<'_, 's>,
    cache: &mut SheetPaintCache,
    progress: Option<PlotProgress<'_>>,
) -> Result<PlotReport> {
    let selection = parse_sheet_selection(&request.sheets, set)?;
    validate_style(request)?;
    let mut report = PlotReport {
        sheets_asked: selection.len(),
        printer: if printer.name.is_empty() {
            native_separators(&String::from_utf8_lossy(&path_to_utf8(&printer.path)))
        } else {
            printer.name.clone()
        },
        ..PlotReport::default()
    };
    if report.printer.is_empty() {
        report.printer = String::from("the printer");
    }
    let mut steps = Progress {
        progress,
        total: selection.len(),
        done: 0,
    };
    let mut document: Option<PdfDocument> = None;
    let dpi = printer.resolution.max(1);
    let ppmm = f64::from(dpi) / 25.4;
    let points = |millimetres: f64| millimetres * 72.0 / 25.4;
    for &index in &selection {
        let sheet = &set.sheets[index];
        if !steps.next(&sheet.name, &mut report) {
            break;
        }
        if document.is_none() {
            if std::fs::File::create(&printer.path).is_err() {
                return Err(make_error(
                    ErrorCode::FileExportFailure,
                    "could not start the printer",
                    report.printer.clone(),
                ));
            }
            document = Some(if printer.readable {
                PdfDocument::uncompressed()
            } else {
                PdfDocument::new()
            });
        }
        // What the printer gave: its whole page, in its device pixels. A sheet bigger than that
        // is scaled down to fit and centred; a smaller one is printed at its size in the top-left
        // corner, as the page was asked for at its size.
        let (page_w_mm, page_h_mm) = printer.page_for(sheet);
        let (page_w, page_h) = (page_w_mm * ppmm, page_h_mm * ppmm);
        let paper = paper_dimensions(sheet.paper, sheet.landscape);
        let sheet_w = paper.width_mm * ppmm;
        let sheet_h = paper.height_mm * ppmm;
        let scale = katana_math::cpp::min(
            1.0,
            katana_math::cpp::min(page_w / sheet_w, page_h / sheet_h),
        );
        let mut offset = PointF::new(0.0, 0.0);
        if scale < 0.999 {
            offset = PointF::new(
                0.5 * (page_w - sheet_w * scale),
                0.5 * (page_h - sheet_h * scale),
            );
            report.shrunk.push((index, scale));
        }
        let options = paint_options(request, ppmm, f64::from(dpi));
        let mut canvas = PdfCanvas::new(points(page_w_mm), points(page_h_mm), f64::from(dpi));
        let stats = paint_page(
            &mut canvas,
            set,
            index,
            &source(),
            &options,
            cache,
            offset,
            if scale < 0.999 { scale } else { 1.0 },
        );
        let added = match document.as_mut() {
            Some(document) => document.add_page(canvas).is_ok(),
            None => false,
        };
        if !added {
            return Err(make_error(
                ErrorCode::FileExportFailure,
                "the printer refused a new page",
                report.printer.clone(),
            ));
        }
        collect_problems(&mut report, set, index, &stats);
        report.sheets.push(index);
        steps.done();
    }
    if let Some(document) = document
        && document.write(&printer.path).is_err()
    {
        return Err(make_error(
            ErrorCode::FileExportFailure,
            "could not finish printing",
            report.printer.clone(),
        ));
    }
    steps.finish(&report);
    Ok(report)
}

/// The printers the system knows (QPrinterInfo's place).
pub trait Printers {
    /// QPrinterInfo::availablePrinterNames.
    fn names(&self) -> Vec<String>;
    /// QPrinterInfo::defaultPrinter's name; None when there is none.
    fn default_name(&self) -> Option<String>;
}

/// The printers the port knows of: none yet. The operating system's queue is not asked until
/// hand_to_print_queue is written.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoPrinters;

impl Printers for NoPrinters {
    fn names(&self) -> Vec<String> {
        Vec::new()
    }

    fn default_name(&self) -> Option<String> {
        None
    }
}

/// Hands the PDF at `pdf` to the operating system's print queue for `printer_name`. Not written
/// yet: Unsupported, with a sentence saying where the PDF is, in a headless session and in the
/// window alike. A headless session never prints: nobody is there to collect the paper.
pub fn hand_to_print_queue(pdf: &Path, printer_name: &str, headless: bool) -> Status {
    let shown = native_separators(&String::from_utf8_lossy(&path_to_utf8(pdf)));
    Err(make_error(
        ErrorCode::Unsupported,
        if headless {
            "a headless session does not print: the sheets were written as a PDF to print from"
        } else {
            "printing straight to a printer is not there yet: the sheets were written as a PDF \
             to print from"
        },
        format!("{shown} for {printer_name}"),
    ))
}

/// print_sheets on the printer the system knows as `printer_name` (Printers::names), or on its
/// default printer when the name is empty, at the printer's high resolution: the sheets written
/// as a PDF at `spool` and handed to the print queue. NotFound, naming it, when there is no such
/// printer; hand_to_print_queue's refusal when the PDF could not be handed over.
#[allow(
    clippy::too_many_arguments,
    reason = "The C++ printSheets' arguments, with the system's printers, the spool file and \
              whether anybody watches."
)]
pub fn print_sheets_on<'s>(
    printers: &dyn Printers,
    printer_name: &str,
    spool: &Path,
    headless: bool,
    set: &SheetSet,
    request: &PlotRequest,
    source: SheetSourceProvider<'_, 's>,
    cache: &mut SheetPaintCache,
    progress: Option<PlotProgress<'_>>,
) -> Result<PlotReport> {
    let found = if printer_name.is_empty() {
        printers.default_name()
    } else {
        printers
            .names()
            .into_iter()
            .find(|name| name == printer_name)
    };
    let Some(name) = found else {
        return Err(make_error(
            ErrorCode::NotFound,
            if printer_name.is_empty() {
                "there is no default printer"
            } else {
                "there is no printer of that name"
            },
            printer_name,
        ));
    };
    let printer = PdfPrinter {
        name: name.clone(),
        ..PdfPrinter::for_file(spool)
    };
    let report = print_sheets(&printer, set, request, source, cache, progress)?;
    hand_to_print_queue(spool, &name, headless)?;
    Ok(report)
}
