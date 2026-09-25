#pragma once

// A sheet set, plotted: to one PDF, to a PDF a sheet, to PNG or TIFF rasters,
// or to a printer (docs/plotting.md, "Plot styles and output").
//
// Everything a plot needs is in a PlotRequest - which sheets, the format, the
// plot style, the resolution, where the files go and how they are named - so
// the plot dialog, the command line and an agent all make the same call and
// get the same files. Nothing here shows a window: the dialog and the
// progress bar (plot_dialog.hpp) are a front end over plotSheets.
//
//   Pdf          one vector PDF at `destination`, a page a sheet, each page
//                the size of its sheet's paper
//   PdfPerSheet  a vector PDF a sheet in the folder `destination`, each named
//                by the file-name pattern (sheetFileNames)
//   Png, Tiff    a raster a sheet at `dpi`, named the same way; greyscale
//                when the colour mode is not Colour, the resolution recorded
//                in the file so it prints at its scale
//   printSheets  the sheets to a QPrinter, a page each, on the sheet's paper
//                when the printer has it and scaled down to fit when not
//
// PROGRESS AND CANCEL. `progress` is called before each sheet and may return
// false to stop; the sheets finished by then are reported. Every file is
// written under a temporary name and takes its own only when it is complete
// (QSaveFile), so a cancelled or failed single PDF is not written at all -
// half a set is not a set - and an earlier file of that name is left as it
// was; the files of a per-sheet plot finished before the cancel are kept and
// listed.
//
// The source is asked for again before each sheet (SheetSourceProvider), so
// a plot that lets the event loop run between sheets never paints from a
// surface that has since been replaced.

#include <cstddef>
#include <functional>
#include <optional>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include <QString>

#include "katana/cad/plot.hpp"
#include "katana/cad/plotting/page_setup.hpp"
#include "katana/cad/plotting/sheet_set.hpp"
#include "katana/cad/plotting/sheet_verbs.hpp"
#include "katana/core/error.hpp"
#include "sheet_painter.hpp"

class QPrinter;

namespace katana::qt {

// Stored and typed by name ("pdf", "pdfs", "png", "tiff"), so values may be
// added at the end without changing what a name means.
enum class PlotFormat {
    Pdf,         // one PDF, a page a sheet
    PdfPerSheet, // a PDF a sheet
    Png,         // a PNG a sheet
    Tiff,        // a TIFF a sheet
};

[[nodiscard]] std::string_view toString(PlotFormat format);
// toString's names, and "pdf-per-sheet", "tif"; case is ignored.
[[nodiscard]] std::optional<PlotFormat> plotFormatFrom(std::string_view name);
// ".pdf", ".png", ".tif".
[[nodiscard]] QString plotFormatExtension(PlotFormat format);
// Whether the format writes a file a sheet into a folder.
[[nodiscard]] bool plotsFilePerSheet(PlotFormat format);

// The most pixels a raster sheet may have: an A0 sheet at 350 dpi, an A1 at
// 500, an A3 at 1000 - 800 MB while it is painted. Beyond it the plot is
// refused and told the resolution that fits, rather than failing half-way
// for want of memory.
inline constexpr double kMaximumRasterPixels = 200.0e6;

struct PlotRequest {
    // Which sheets, as parseSheetSelection reads them: "" or "all", "1,3-5",
    // sheet ids.
    std::string sheets;
    PlotFormat format = PlotFormat::Pdf;
    katana::cad::PlotColourMode colourMode = katana::cad::PlotColourMode::Colour;
    double lineWeightScale = 1.0;
    double dpi = 300.0;
    // The PDF file for Pdf; the folder the files go in for the others,
    // made when it is missing. Ignored by printSheets.
    QString destination;
    std::string fileNamePattern{katana::cad::plotting::kDefaultFileNamePattern};
    // The PDF's title; the file's own name when empty.
    QString title;
};

// A request as the set's page setup would make it: its style, resolution,
// naming and (filePerSheet) format, to `destination`, for `sheets`.
[[nodiscard]] PlotRequest plotRequestFor(const katana::cad::plotting::PageSetup& setup,
                                         QString destination, std::string sheets = {});
// The request the PLOTSHEETS verb's words make (sheet_verbs.hpp,
// parsePlotSheets): the page setup's, with what the words give in its place.
// The format is one PDF unless the words name one, as --plot-sheets has it.
[[nodiscard]] PlotRequest plotRequestFor(const katana::cad::plotting::PageSetup& setup,
                                         const katana::cad::plotting::PlotSheetsRequest& words);
// The page setup a request amounts to: `base` with the request's style,
// resolution, naming and whether it is a PDF a sheet. The format of a raster
// plot is not a page setup's to keep, so a PNG or TIFF request keeps `base`'s
// filePerSheet.
[[nodiscard]] katana::cad::plotting::PageSetup
pageSetupFor(const PlotRequest& request, const katana::cad::plotting::PageSetup& base);

// What a request asks for, checked without writing anything: the selection
// against `set`, the style and resolution (validatePageSetup), a destination,
// and a raster's size (kMaximumRasterPixels). InvalidArgument saying what.
[[nodiscard]] katana::core::Status validatePlotRequest(const katana::cad::plotting::SheetSet& set,
                                                       const PlotRequest& request);

// The files `request` would write, in order, without writing them: the one
// PDF, or a file a selected sheet. validatePlotRequest's errors.
[[nodiscard]] katana::core::Result<std::vector<QString>>
plannedFiles(const katana::cad::plotting::SheetSet& set, const PlotRequest& request);

// What a plot did.
struct PlotReport {
    std::vector<QString> files;       // written, in order
    std::vector<std::size_t> sheets;  // the sheets plotted, as indices, in order
    std::size_t sheetsAsked = 0;      // how many the selection named
    std::vector<std::string> problems; // "<sheet name> - vp3: no alignment named X"
    bool cancelled = false;
    // Scaled to fit the printer's paper: a factor below 1 for each sheet
    // printed smaller than its paper, by sheet index. Files are never scaled.
    std::vector<std::pair<std::size_t, double>> shrunk;
    // printSheets: the printer's name; empty for a plot to files.
    QString printer;

    // One line for a log: "Plotted 3 sheets to C:/out/set.pdf.", "Plotted 3
    // sheets to 3 PNG files in C:/out.", "Printed 2 sheets on Office." or
    // "Cancelled after 2 of 5 sheets; 2 files kept in C:/out."
    [[nodiscard]] QString summary(const PlotRequest& request) const;
};

// Called before sheet `done` + 1 of `total` (`name` is that sheet's name) and
// once more with done == total at the end. Returning false cancels the plot.
using PlotProgress =
    std::function<bool(std::size_t done, std::size_t total, const std::string& name)>;
// What the sheets are drawn from, asked for before each sheet.
using SheetSourceProvider = std::function<SheetSource()>;

// Plots the sheets of `set` that `request` selects, in the order selected.
// validatePlotRequest's errors, and FileExportFailure (naming the file) when
// a file cannot be written; files finished before a failure are kept, and
// the error's message ends by listing them ("; 2 files written before it
// were kept: ...").
[[nodiscard]] katana::core::Result<PlotReport>
plotSheets(const katana::cad::plotting::SheetSet& set, const PlotRequest& request,
           const SheetSourceProvider& source, SheetPaintCache& cache,
           const PlotProgress& progress = {});
// The same from one fixed source.
[[nodiscard]] katana::core::Result<PlotReport>
plotSheets(const katana::cad::plotting::SheetSet& set, const PlotRequest& request,
           const SheetSource& source, SheetPaintCache& cache, const PlotProgress& progress = {});

// Prints the selected sheets on `printer`, a page each, in the request's
// style. Each page is asked for at its sheet's paper size and orientation;
// when the printer gives a smaller page the sheet is scaled down to fit it,
// centred, and recorded in PlotReport::shrunk. The printer's own resolution
// is used, not the request's. FileExportFailure when the printer cannot be
// started.
[[nodiscard]] katana::core::Result<PlotReport>
printSheets(QPrinter& printer, const katana::cad::plotting::SheetSet& set,
            const PlotRequest& request, const SheetSourceProvider& source, SheetPaintCache& cache,
            const PlotProgress& progress = {});
// The same on the printer the system knows as `printerName`
// (QPrinterInfo::availablePrinterNames), or on its default printer when the
// name is empty, at the printer's high resolution. NotFound, naming it, when
// there is no such printer.
[[nodiscard]] katana::core::Result<PlotReport>
printSheets(const QString& printerName, const katana::cad::plotting::SheetSet& set,
            const PlotRequest& request, const SheetSourceProvider& source, SheetPaintCache& cache,
            const PlotProgress& progress = {});

} // namespace katana::qt
