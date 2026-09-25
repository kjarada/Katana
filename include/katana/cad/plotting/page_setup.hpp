#pragma once

// How a sheet set is plotted: its page setup, which sheets a plot takes, and
// what the files it writes are called (docs/plotting.md, "Plot styles and
// output").
//
// The PAGE SETUP is kept with the set (SheetSet::pageSetup, stored in its
// JSON), so a project plots the same way every time and for everyone: the
// plot style (colour mode and line weight scale), the resolution, and for a
// file per sheet the pattern the files are named by. The plot dialog starts
// from it and the command line falls back to it.
//
// A SELECTION is text, so a person, a command line and an agent all say it
// the same way: "1,3-5" is the first, third, fourth and fifth sheets in the
// set's order; a sheet's id ("s7") names that sheet wherever it has moved to;
// "" or "all" is every sheet.
//
// A FILE NAME is the pattern expanded for one sheet and made safe for the
// file system: "{set}{n:02} {name}" is "C03 PLAN TILE 3" for the third
// sheet of set C. Nothing here touches a file; the Qt layer writes them.

#include <cstddef>
#include <span>
#include <string>
#include <string_view>
#include <vector>

#include "katana/cad/plot.hpp"
#include "katana/core/error.hpp"

namespace katana::cad {
class Document;
}

namespace katana::cad::plotting {

struct SheetSet;

// The sheet number, zero-padded to two, after the set number, then the
// sheet's name: files that sort in the set's order and say what they hold.
inline constexpr std::string_view kDefaultFileNamePattern = "{set}{n:02} {name}";

// The resolutions a plot may be made at, and the line weight factors. A
// resolution outside them is a typing mistake (5 dpi, 30 000 dpi) rather
// than a plot anyone wants; 1200 dpi is already an A1 raster of 1.1 billion
// pixels, which plot_output refuses in its own right.
inline constexpr double kMinimumPlotDpi = 50.0;
inline constexpr double kMaximumPlotDpi = 1200.0;
inline constexpr double kMinimumLineWeightScale = 0.1;
inline constexpr double kMaximumLineWeightScale = 5.0;

struct PageSetup {
    PlotColourMode colourMode = PlotColourMode::Colour;
    double lineWeightScale = 1.0;
    double dpi = 300.0;
    // How a file of one sheet is named: formatSheetNumber's {n}, {n:02},
    // {N} and {set}, and {name} (the sheet's name), {number} (its sheet
    // number as the set's numbering writes it) and {id}.
    std::string fileNamePattern{kDefaultFileNamePattern};
    // A PDF a sheet, named by the pattern, rather than one PDF of them all.
    bool filePerSheet = false;

    friend bool operator==(const PageSetup&, const PageSetup&) = default;
};

// InvalidArgument, saying which and why, for a resolution or line weight
// scale out of range or not finite, or a file-name pattern that
// validateFileNamePattern refuses.
[[nodiscard]] core::Status validatePageSetup(const PageSetup& setup);

// InvalidArgument for an empty pattern, a brace left open, or a {token} that
// is not one of those PageSetup::fileNamePattern lists. A pattern with no
// token that changes from sheet to sheet is accepted: sheetFileNames tells
// the files apart by a counter.
[[nodiscard]] core::Status validateFileNamePattern(std::string_view pattern);

// The plot settings a page setup asks for: its colour mode, line weight
// scale and resolution, the rest at their defaults.
[[nodiscard]] PlotSettings plotSettingsFor(const PageSetup& setup);

// The sheets `text` names, as indices into `set.sheets`, in the order named
// and each once: comma-separated sheet positions from 1 ("4"), ranges of them
// ("3-5", low to high) and sheet ids ("s7"); spaces around a comma or a
// range's dash are ignored, and one inside a part ("1 3") is refused rather
// than read as sheet 13. Empty text or "all" is every sheet in order. InvalidArgument, naming the part, for a
// position past the last sheet or below 1, a backward range, or a word that
// is no sheet's id; and for a set with no sheets.
[[nodiscard]] core::Result<std::vector<std::size_t>> parseSheetSelection(std::string_view text,
                                                                         const SheetSet& set);

// The shortest text parseSheetSelection reads back as `indices`: positions
// from 1, runs of three or more written as ranges ("1,3-5"). Empty for none.
[[nodiscard]] std::string formatSheetSelection(std::span<const std::size_t> indices);

// `name` made safe as a file name on every system Katana runs on: the
// characters Windows forbids (< > : " / \ | ? *) and control characters
// become '-', runs of spaces become one, spaces and dots are trimmed from the
// ends (Windows drops them), a device name (CON, PRN, AUX, NUL, COM1-9,
// LPT1-9, with or without an extension) gets a '_' in front, and the result is
// cut to 120 bytes at a character boundary. "sheet" when nothing is left.
[[nodiscard]] std::string sanitiseFileName(std::string_view name);

// The file name, without an extension, of sheet `index` under `pattern`:
// expanded, then sanitised. Empty for an index past the end.
[[nodiscard]] std::string sheetFileName(const SheetSet& set, std::size_t index,
                                        std::string_view pattern);

// sheetFileName for each of `indices`, made distinct ignoring case (as the
// file systems Katana runs on compare names): a name met again gets " (2)",
// " (3)"... in order, so the same set always gives the same names.
[[nodiscard]] std::vector<std::string> sheetFileNames(const SheetSet& set,
                                                      std::span<const std::size_t> indices,
                                                      std::string_view pattern);

// Makes `setup` the document's page setup, as one undoable step
// ("PAGE_SETUP"); no step when it is the page setup already. Refused with
// validatePageSetup's error, or with the document's when its sheets cannot be
// written (a newer version's).
[[nodiscard]] core::Status setPageSetup(Document& document, const PageSetup& setup);

} // namespace katana::cad::plotting
