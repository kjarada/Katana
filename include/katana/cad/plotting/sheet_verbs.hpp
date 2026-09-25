#pragma once

// The sheet system on the text command line (docs/plotting.md, "Sheets on the
// command line"): the verbs a person, a script, the headless katana_cli and
// an AI agent type to lay out, edit and inspect a project's sheets.
//
//   SHEETS [LIST] | JSON [path] | SAVE path | LOAD path | CHECK | PAGESETUP
//   SHEET NEW | REMOVE | MOVE | COPY | RENAME | SET | FIELD | SUGGESTPAPER
//   VIEW ADD | SET | REMOVE | LIST | FIT | BESTROTATE
//   TILE n preset
//   ARRANGE n | ALIGN | DISTRIBUTE | MATCHSCALE
//   GENERATE fit | grid | strips | profile | sections | frames | register
//   TITLEBLOCK [LIST] | field value | REVISION ADD | REVISION REMOVE | LOGO path
//
// Sheets are named by their number in the set (1, 2 ...) or by their id (s1,
// s2 ...), views by their id (vp1, vp2 ...). Options are key=value and verbs,
// keys and words are case-insensitive. Every edit is ONE undoable step
// through sheet_commands.hpp, and a refused edit changes nothing. A reply is
// one fact per line in the same key=value form the options take, so an
// option SHEETS LIST prints can be typed back. Stored sheets that cannot be
// read refuse every verb but SHEETS LOAD, rather than be overwritten.
//
// Plotting needs Qt, so PLOTSHEETS is the desktop window's verb; it parses
// its line with parsePlotSheets below, in the words the others use.

#include <cstddef>
#include <filesystem>
#include <functional>
#include <optional>
#include <span>
#include <string>
#include <string_view>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/cad/plot.hpp"
#include "katana/cad/plotting/preflight.hpp"
#include "katana/cad/plotting/section_fit.hpp"
#include "katana/cad/plotting/sheet_set.hpp"
#include "katana/cad/section.hpp"
#include "katana/core/error.hpp"

namespace katana::cad::plotting {

// What the verbs need that the Document does not hold, from the front end
// that has it. The desktop window supplies both; headless they are left
// empty and the verbs do without.
struct SheetVerbContext {
    // The surfaces GENERATE SECTIONS samples, to centre each section on its
    // ground and choose the exaggeration. None: each section is centred when
    // it is drawn, at an exaggeration of 1 (crossSectionSheets).
    std::vector<SectionSurfaceInput> surfaces;
    // What GENERATE FIT and GRID cover without area=: in the window,
    // everything its plan view draws (imagery and point clouds included), as
    // the sheet editor's Generate Sheets covers. Not given: the drawing's
    // entities (drawnExtent).
    std::optional<Box2> drawingExtent;
    // What SHEETS CHECK runs on the sheets at the indices given (every sheet
    // when empty): in the window, the checks with what the painter knows -
    // its imagery and meshes, its rule for an automatic plan
    // (checkSheetsFor). Not given: checkDocumentSheets.
    std::function<std::vector<Finding>(std::span<const std::size_t>)> check;
    // What a viewport shows, as world points, for ARRANGE MATCHSCALE, VIEW
    // FIT, VIEW BESTROTATE and SHEET SUGGESTPAPER: in the window with its
    // reference layers and meshes, and a key plan's outlines placed as the
    // painter places them. Not given: viewportContent over the drawing.
    std::function<std::vector<Point2>(const Viewport&)> content;
    // The scale and exaggeration an automatic section is drawn at, for
    // ARRANGE MATCHSCALE from one: the painter fits it to the ground it cuts,
    // which the document does not hold. Not given: its stored ones.
    std::function<SectionFit(const Viewport&)> fitSection;
    // The set as the painter draws it, every automatic scale decided, for
    // SHEET FIELD to print the scale the title block prints. Not given: the
    // stored scales.
    std::function<SheetSet(const SheetSet&)> drawn;
};
// Called only by the verbs that need it (GENERATE), so a front end may build
// the context lazily.
using SheetVerbContextProvider = std::function<SheetVerbContext()>;

// True for SHEETS, SHEET, VIEW (and VIEWPORT), TILE, ARRANGE, GENERATE and
// TITLEBLOCK, in any case.
[[nodiscard]] bool isSheetVerb(std::string_view verb);

// Runs one sheet verb: `tokens` is the whole line split into words with the
// quotes removed (CommandInterpreter::tokenize), the verb first. Returns the
// reply, or an error saying specifically what was refused and why.
[[nodiscard]] core::Result<std::string>
runSheetVerb(Document& document, const std::vector<std::string>& tokens,
             const SheetVerbContextProvider& context = {});

// Every sheet verb and option, for HELP SHEETS.
[[nodiscard]] std::string sheetVerbHelp();

// ---- the pieces the verbs are made of, for front ends and tests ----------------------

// The sheet `text` names in `set`: its number from 1, or its id. NotFound for
// a number or id the set does not have; ParseFailure for anything else.
[[nodiscard]] core::Result<std::size_t> sheetIndexFrom(const SheetSet& set, std::string_view text);

// PLOTSHEETS [path] [format=pdf|pdfs|png|tiff] [style=colour|grey|mono]
// [sheets=1,3-5] [dpi=n] [lineweight=f] [folder=path] [pattern=text]: the
// words after the verb. What is not given comes from the set's page setup,
// as the Plot dialog and --plot-sheets take it; the format is one PDF unless
// format= says otherwise, or unless only folder= is given, which is a PDF a
// sheet. The path is the PDF of the one-PDF format, or the folder of a file
// a sheet (folder= says the same). The sheets are the selection
// parseSheetSelection (page_setup.hpp) reads, as every plot takes it.
struct PlotSheetsRequest {
    std::filesystem::path path;
    std::string sheets;
    // "pdf", "pdfs", "png" or "tiff"; nothing: "pdf", or "pdfs" for folder=.
    std::optional<std::string> format;
    std::optional<PlotColourMode> colourMode;
    std::optional<double> dpi;
    std::optional<double> lineWeightScale;
    std::optional<std::string> fileNamePattern;
};
[[nodiscard]] core::Result<PlotSheetsRequest> parsePlotSheets(const SheetSet& set,
                                                              const std::vector<std::string>& args);

// A finding as SHEETS CHECK prints it, one line: its severity, its code, the
// sheet's number (- for the set), the view's id (- for none), then
// message="..." and, when there is one, fix="..." and subject="..." - the
// words quoted as every reply quotes them, so a line splits on spaces:
//   warning plan.empty 2 vp3 message="PLAN 1:500 (vp3) shows nothing ..." fix="..."
// (preflight.hpp's findingLine is the sentence a log shows.)
[[nodiscard]] std::string checkReplyLine(const Finding& finding);

// One viewport option, `key` = `value`, as VIEW ADD and VIEW SET take it
// (sheetVerbHelp lists them). `model` checks an alignment= name. The
// viewport is left as it was when the option is refused.
[[nodiscard]] core::Status setViewportOption(Viewport& viewport, std::string_view key,
                                             std::string_view value,
                                             const entity::Model& model);

// A new viewport of `kind` for sheet `sheetIndex`, set up as the sheet
// editor's Add View sets one up: its size for the kind, centred in the
// tiling area (or filling it on an empty sheet), a plan scaled and centred
// automatically with a north arrow and scale bar, sections on the drawing's
// first alignment, a key plan fitted to the sheets' plans, which it outlines
// live (key_plan.hpp), as the editor's Add View > Key Plan does. Its id is
// the set's next.
[[nodiscard]] Viewport defaultViewport(const SheetSet& set, std::size_t sheetIndex,
                                       ViewportKind kind, const entity::Model& model);

// The shared title-block value `field` names (organisation, project1..4,
// client, setnumber, numbering, coordsys, datum, model, notes, and
// <role>name / <role>date for locator, surveyor, compiler, reviewer and
// approver; the frame's own field names are accepted too), read or set.
// NotFound for a field that is not one of them.
[[nodiscard]] core::Result<std::string> titleBlockValue(const SheetSet& set, std::string_view field);
[[nodiscard]] core::Status setTitleBlockValue(SheetSet& set, std::string_view field,
                                              std::string value);
// The fields above in the order TITLEBLOCK LIST prints them.
[[nodiscard]] std::vector<std::string_view> titleBlockFields();

// The set written to, or read from, a JSON file (sheet_json.hpp).
[[nodiscard]] core::Status writeSheetSetFile(const SheetSet& set, const std::filesystem::path& path);
[[nodiscard]] core::Result<SheetSet> readSheetSetFile(const std::filesystem::path& path);

// The lines SHEETS LIST and VIEW LIST print.
[[nodiscard]] std::string describeSheet(const SheetSet& set, std::size_t index);
[[nodiscard]] std::string describeViewport(const Viewport& viewport);

} // namespace katana::cad::plotting
