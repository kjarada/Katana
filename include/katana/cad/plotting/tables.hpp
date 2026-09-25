#pragma once

// Tables on a sheet: the drawing register and the revision table
// (docs/plotting.md, "The drawing register and the revision table").
//
// A table is laid out here and only drawn by the painter: its rules, each of
// its texts and where it goes in paper millimetres, the text size and row
// height chosen to fit the viewport, the second block of columns the rows
// continue in, and how many rows did not fit. So the layout is tested without
// pixels, and what the editor shows, what plots and what an agent reads are
// the same rows.
//
//   drawingRegister   a row per sheet: its number as the set numbers it now,
//                     its title, the scale its title block reports, its paper
//                     and its current revision
//   revisionRows      the set's revisions, newest first; the newest N only
//   layoutTable       any table fitted into a rectangle
//   layoutViewportTable  the table a SheetIndex or Revisions viewport shows
//   registerSheet     a cover sheet holding the register and the revisions
//   addRegisterSheet  that sheet put first in a document's set, one step
//
// Text is measured through a TextWidth the caller supplies - the painter
// passes its font's - or, when none is given, estimated from Arial's advance
// widths: deterministic, and close enough to lay a table out by.

#include <cstddef>
#include <functional>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/cad/plotting/frame.hpp"
#include "katana/cad/plotting/generators.hpp"
#include "katana/cad/plotting/sheet_set.hpp"
#include "katana/core/error.hpp"

namespace katana::cad::plotting {

// The width in paper millimetres of one line of `text` (UTF-8) set in Arial
// at a cap height of `capMm`, bold or not, not condensed. The layout assumes
// a width grows in proportion to the cap height, as a scaled font's does.
using TextWidth = std::function<double(std::string_view text, double capMm, bool bold)>;

// A TextWidth from Arial's advance widths (the metrics Arial shares with
// Helvetica), for when there is no font to ask: deterministic on every
// machine. A character outside ASCII counts as a digit's width, and one from
// the wide East Asian blocks as a full em.
[[nodiscard]] double estimateTextWidth(std::string_view text, double capMm, bool bold);

// One line of the drawing register.
struct RegisterRow {
    std::string sheetId;
    // As the sheet's title block prints each: the number the set gives the
    // sheet now (or the one typed on it), its name, the scale it reports
    // ("1:500", "AS SHOWN", "N.T.S."), its paper, and its current revision -
    // typed on the sheet, else the set's latest.
    std::string number;
    std::string title;
    std::string scale;
    std::string paper;
    std::string revision;

    friend bool operator==(const RegisterRow&, const RegisterRow&) = default;
};

// A row per sheet, in the set's order, from the same fields the title blocks
// print (resolveFields), so the register and the sheets cannot disagree. An
// automatic plan scale reads as its last choice; the painter decides them
// first, as it does for a title block.
[[nodiscard]] std::vector<RegisterRow> drawingRegister(const SheetSet& set);

// The set's revisions newest first - they are stored oldest first, in the
// order they were issued - and only the newest `limit` of them when `limit`
// is not 0.
[[nodiscard]] std::vector<Revision> revisionRows(const SheetSet& set, std::size_t limit = 0);

struct TableColumn {
    std::string header;
    HorizontalJustify justify = HorizontalJustify::Left;
    // Takes the width the other columns leave (at most one column does).
    bool stretch = false;
    // Wraps onto as many lines as it needs, and starts a new line at each
    // '\n'. A cell of any other column is one line - a '\n' in it prints as
    // a space - and one too wide is squeezed to fit, as the frame's fields are.
    // Blanks and line breaks at either end of any cell are dropped.
    bool wrap = false;

    friend bool operator==(const TableColumn&, const TableColumn&) = default;
};

// What a table holds, before it is fitted anywhere.
struct TableSpec {
    std::string heading; // one line across the top in bold; empty for none
    std::vector<TableColumn> columns;
    std::vector<std::vector<std::string>> rows; // a text per column
    std::optional<std::size_t> highlight;        // a row shaded lightly
    // How many blocks of columns, side by side, the rows may continue in
    // before the rest is left out and counted ("+12 more").
    std::size_t maxBlocks = 2;
    std::string overflowWord = "more";

    friend bool operator==(const TableSpec&, const TableSpec&) = default;
};

struct TableStyle {
    // The text is the largest size from maxCapMm down, in steps of stepMm,
    // at which every row fits; never below minCapMm, the smallest text a
    // plotted table is read at.
    double maxCapMm = 2.5;
    double minCapMm = 1.8;
    double stepMm = 0.1;
    double rowPitch = 2.0;     // a one-line row, in cap heights
    double linePitch = 1.5;    // each further line of a wrapped cell (the frame's rule)
    double paddingRatio = 0.5; // each side of a cell's text, in cap heights
    double headingRatio = 1.25; // the heading's cap height over the body's
    double xFactor = 0.9;       // condensing, as the legend and notes are set
    double outerWeightMm = 0.25; // the box, the heading's and headers' rules, blocks
    double innerWeightMm = 0.13; // between rows and columns
    // A layout that squeezes no line below this is preferred to one with
    // larger text that does: a long title set smaller reads better than one
    // pressed to half its width.
    double leastSqueeze = 0.8;

    friend bool operator==(const TableStyle&, const TableStyle&) = default;
};

struct TableRule {
    Point2 from{};
    Point2 to{};
    double weightMm = 0.13;

    friend bool operator==(const TableRule&, const TableRule&) = default;
};

struct TableText {
    std::string text;
    // On the baseline: the text's left end, middle or right end, as
    // `horizontal` says.
    Point2 anchor{};
    double capMm = 1.8;
    double xFactor = 1.0;
    // A further horizontal squeeze for a line wider than its cell; 1 for none.
    double squeeze = 1.0;
    bool bold = false;
    bool muted = false; // "+12 more": printed grey
    HorizontalJustify horizontal = HorizontalJustify::Left;

    friend bool operator==(const TableText&, const TableText&) = default;
};

// A table fitted into a rectangle, in paper millimetres. The painter fills
// `shaded`, then draws `rules`, then `texts`; nothing here needs clipping.
struct TableLayout {
    std::vector<Box2> shaded;
    std::vector<TableRule> rules;
    std::vector<TableText> texts;
    double capMm = 0.0;       // the body's cap height
    double rowHeightMm = 0.0; // a one-line row
    std::size_t blocks = 0;   // blocks of columns side by side
    // The rows shown are the first rowsShown, in order; rowBoxes[i] is row
    // i's rectangle. rowsHidden did not fit and are counted in the last line.
    std::size_t rowsShown = 0;
    std::size_t rowsHidden = 0;
    std::vector<Box2> rowBoxes;

    friend bool operator==(const TableLayout&, const TableLayout&) = default;
};

// `spec` fitted into `rect`: the largest text (from style.maxCapMm down to
// style.minCapMm) at which every row fits one block; else the largest at
// which they fit two blocks side by side (up to spec.maxBlocks), each with
// its own header - first among the layouts that squeeze no line below
// style.leastSqueeze, then among any; else the minimum text in as many
// blocks as are wide enough, with as many rows as fit and "+N more" for the
// rest. A block is wide enough when every column but the stretching one has
// its widest text and the stretching one its header. Rules run the box's
// full height, so a short register is a ruled form with room below. An
// empty rectangle, or no columns, lays out nothing and counts every row
// hidden. `measure` defaults to estimateTextWidth.
[[nodiscard]] TableLayout layoutTable(const TableSpec& spec, const Box2& rect,
                                      const TextWidth& measure = {},
                                      const TableStyle& style = {});

// What a SheetIndex or Revisions viewport on sheet `sheetIndex` shows:
//   SheetIndex  SHEET No. | TITLE | SCALE | PAPER | REV, a row per sheet
//               (drawingRegister), sheet `sheetIndex` highlighted, continuing
//               in a second block;
//   Revisions   REV | DATE | DESCRIPTION | BY, newest first, the viewport's
//               revisionLimit newest only, the description wrapped, in one
//               block ("+N earlier" for what does not fit).
// The heading is the viewport's title, else automaticTitle. A spec with no
// columns for any other kind.
[[nodiscard]] TableSpec tableSpecFor(const SheetSet& set, std::size_t sheetIndex,
                                     const Viewport& viewport);

// layoutTable(tableSpecFor(...), viewport.rect, measure).
[[nodiscard]] TableLayout layoutViewportTable(const SheetSet& set, std::size_t sheetIndex,
                                              const Viewport& viewport,
                                              const TextWidth& measure = {});

// The name registerSheet gives its sheet.
inline constexpr std::string_view kRegisterSheetName = "DRAWING REGISTER";

// A cover sheet named DRAWING REGISTER: the register (SheetIndex) in the big
// cell of "Main and panel right" and the revision table (Revisions) in the
// panel beside it, without the frame's utility legend, which says nothing on
// a cover. Ids s1, vp1 and vp2, as a generator's; prepareForAppend renumbers
// them. AlreadyExists when a sheet of `set` already has a register: two
// registers would be two lists to keep in step.
[[nodiscard]] core::Result<Sheet> registerSheet(const SheetSet& set,
                                                const SheetTemplate& paper = {});

// Puts registerSheet(the document's set, paper) FIRST in the set - a cover
// comes before what it lists - with fresh ids, as ONE undoable step, and
// returns the new sheet's id. The other sheets' numbers move up by one; their
// marks follow them by id.
[[nodiscard]] core::Result<std::string> addRegisterSheet(Document& document,
                                                         const SheetTemplate& paper = {});

} // namespace katana::cad::plotting
