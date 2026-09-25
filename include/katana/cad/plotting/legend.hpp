#pragma once

// The legend a sheet prints: what its plans actually SHOW (docs/plotting.md,
// "The smart legend").
//
// A legend that lists every layer of the drawing tells the reader about
// things that are not on the sheet, and a sheet strung along a road names the
// whole survey on every page. This gathers the legend from what the plan
// viewports show instead:
//
//   which entities   those drawn (a visible entity on a layer the document
//                    shows) that a plan viewport does not hide, and whose
//                    shape meets the viewport's window on the ground - the
//                    ROTATED rectangle a twisted viewport shows, not the box
//                    around it, so a strip along a road does not list the
//                    pipe beside the next strip;
//   grouped by       what prints: an entity drawn in a style (a library
//                    linestyle or symbol, or the style a survey code gave it)
//                    by that style, any other by its layer; and within that by
//                    what kind of mark it is - a symbol, a plain point, a
//                    line, a hatched area or a text - since a point and a line
//                    of one layer print differently;
//   labelled         by the description the survey code library gives the
//                    code the entities carry, when every coded one agrees on
//                    it, else by the style's or the layer's name. A style's
//                    own description is not used: in practice it holds a
//                    colour name or the import's provenance, not words for
//                    a reader;
//   drawn with       the look most of the group's entities resolve to (the
//                    entity::resolveDisplay chain), ties to the one met at
//                    the lowest entity id;
//   in the order     symbols, points, lines, areas, texts, each by label with
//                    letter case folded.
//
// The SCOPE is the Legend viewport's own (Viewport::legendScope): this
// sheet's plans, every plan of the set, or the whole drawing. A sheet with
// no plan lists what the set's plans show, and a set with no plan the whole
// drawing. Key plans are not counted: they show the drawing faded, as a map
// of the sheets.
//
// Everything here is a pure function of the model, the sheet set and the
// options, so the painter, a test and an agent get the same list; the
// painter draws it (src/katana_qt/sheet_painter.cpp and
// src/katana_qt/plotting/legend_painter.hpp).

#include <cstddef>
#include <functional>
#include <optional>
#include <span>
#include <string>
#include <string_view>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/cad/plotting/sheet_set.hpp"
#include "katana/core/error.hpp"
#include "katana/entity/entity.hpp"
#include "katana/entity/model.hpp"
#include "katana/entity/survey_map.hpp"
#include "katana/geometry/primitives2d.hpp"
#include "katana/geometry/spatial_index.hpp"

namespace katana::cad::plotting {

// "this_sheet", "whole_set", "whole_drawing": the JSON and command names.
[[nodiscard]] std::string_view toString(LegendScope scope);
[[nodiscard]] std::optional<LegendScope> legendScopeFrom(std::string_view name);

// What kind of mark an entry's sample is, in the order the legend lists them.
enum class LegendKind {
    Symbol, // a point drawn with a symbol
    Point,  // a point drawn as the plain mark
    Line,   // a line, arc, polyline or circle; with its style's symbol at each vertex
    Area,   // a closed polyline filled or hatched
    Text,
};

// "symbol", "point", "line", "area", "text".
[[nodiscard]] std::string_view toString(LegendKind kind);

struct LegendEntry {
    LegendKind kind = LegendKind::Line;
    std::string label;
    // The style the entities are drawn in, for an entry grouped by style;
    // empty for one grouped by layer.
    std::string style;
    // The layer, for an entry grouped by layer; empty for one grouped by style.
    std::string layer;
    // The survey code whose description is the label; empty when the label
    // is a name.
    std::string code;
    // How it prints, as entity::ResolvedDisplay gives it (before the paper
    // colour rule, which the painter applies).
    katana::entity::Color colour{};
    double lineWeight = 0.25; // millimetres on paper
    std::string linetype{katana::entity::kContinuousLinetype};
    std::string symbol{katana::entity::kNoSymbol};
    double symbolSize = 0.0; // model units; 0 for the definition's or the mark's own
    std::string hatchPattern{katana::entity::kNoHatch};
    // The entities it stands for: each counted once, however many plans show it.
    std::size_t count = 0;

    friend bool operator==(const LegendEntry&, const LegendEntry&) = default;
};

struct Legend {
    // What the entries were gathered from once a sheet or set without plans
    // has fallen back: ThisSheet, WholeSet or WholeDrawing.
    LegendScope scope = LegendScope::ThisSheet;
    // The plan viewports looked through (0 for the whole drawing).
    std::size_t windows = 0;
    // 1 : scale, what a sample sized on the ground (a symbol in metres, a
    // world linestyle) is drawn at: the first plan on the legend's sheet,
    // else the first plan the entries came from; 0 when there is none, and
    // the painter uses the legend viewport's own scale.
    double scale = 0.0;
    std::vector<LegendEntry> entries;

    friend bool operator==(const Legend&, const Legend&) = default;
};

// The window `viewport` shows: its own scale and centre, or for autoScale and
// autoCentre the fit to its stretch of its alignment or else to what it draws
// (the entities it lets through and the alignments), with 4% to spare - the
// painter's rule (resolvePlanViewport), without the imagery and meshes the
// painter also counts, which the model does not hold.
[[nodiscard]] PlanWindow fittedPlanWindow(const katana::entity::Model& model,
                                          const Viewport& viewport);

struct LegendOptions {
    // How an automatic plan's window is decided; null: fittedPlanWindow. The
    // painter passes its own, so a legend lists what the plan beside it drew.
    std::function<PlanWindow(const Viewport&)> window;
    // Speeds up a plan that shows part of a large drawing; the answer is the
    // same without it.
    const katana::geometry::SpatialIndex* index = nullptr;
    // The survey code library whose descriptions label coded entities; null
    // or empty: every label is a name.
    const katana::entity::SurveyMap* codes = nullptr;
    // The property holding an entity's code; empty: the first of
    // codePropertyCandidates() any entity carries (findCodeProperty).
    std::string codeProperty;
};

// The legend of sheet `sheetIndex` of `set` for `scope`. InvalidArgument for
// an index past the end.
[[nodiscard]] core::Result<Legend> computeLegend(const katana::entity::Model& model,
                                                 const SheetSet& set, std::size_t sheetIndex,
                                                 LegendScope scope,
                                                 const LegendOptions& options = {});

// The legend the Legend viewport `viewportId` of the document's sheets
// lists, at its own scope, with the document's spatial index and survey code
// library. NotFound for no such viewport; InvalidArgument for a viewport that
// is not a legend.
[[nodiscard]] core::Result<Legend> legendFor(const Document& document,
                                             std::string_view viewportId);

// Sets what the Legend viewport `viewportId` lists, as one undoable step (an
// unchanged scope records none). NotFound for no such viewport;
// InvalidArgument for one that is not a legend.
[[nodiscard]] core::Status setLegendScope(Document& document, std::string_view viewportId,
                                          LegendScope scope);

// The legend as JSON, for an agent or a report:
//   {"scope": "this_sheet", "windows": 1, "scale": 500,
//    "entries": [{"kind": "symbol", "label": "...", "style": "...", "layer": "...",
//                 "code": "...", "colour": "#RRGGBB", "weight": 0.25,
//                 "linetype": "...", "symbol": "...", "symbol_size": 0,
//                 "hatch": "none", "count": 3}]}
// Every member of an entry is written, so a reader need not know a default.
[[nodiscard]] std::string legendJson(const Legend& legend);

// ---- laying the legend out on the paper ------------------------------------

// The legend panel's measures, in paper millimetres.
struct LegendMetrics {
    double margin = 2.5;       // inside the viewport's edge
    double heading = 8.25;     // from the top edge to the first row's top
    double pitch = 4.5;        // from one row to the next
    double sampleWidth = 10.0; // the sample's cell
    double sampleHeight = 3.6;
    double labelGap = 2.0;  // from the sample to its label
    double columnGap = 4.0; // between columns
};

// Where one entry goes.
struct LegendCell {
    std::size_t entry = 0;
    katana::geometry::Box2 sample;
    katana::geometry::Point2 label; // the label's left end, on its middle line
    double labelRoom = 0.0;         // how wide the label may be
};

struct LegendLayout {
    std::vector<LegendCell> cells; // in entry order
    std::size_t rows = 0;          // rows in the tallest column
    std::size_t columns = 0;       // columns used
    double columnWidth = 0.0;
    // Entries left out because they do not fit, and where "+N more" goes:
    // the last cell's place, which the entry there gives up.
    std::size_t more = 0;
    std::optional<katana::geometry::Point2> moreAt;
};

// Flows `labelWidths.size()` entries down columns and then across, inside
// `rect`: each column as wide as the widest label needs (at most the room
// there is), as few columns as hold every entry, rows shared evenly between
// them. When they do not all fit, the last cell says how many are left out.
[[nodiscard]] LegendLayout layoutLegend(const katana::geometry::Box2& rect,
                                        std::span<const double> labelWidths,
                                        const LegendMetrics& metrics = {});

} // namespace katana::cad::plotting
