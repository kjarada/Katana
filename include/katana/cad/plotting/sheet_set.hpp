#pragma once

// Sheets: a project's drawings on paper (docs/plotting.md).
//
// A SheetSet is every sheet of a project: the title-block values they share,
// how they are numbered, the revisions table, and the sheets themselves. A
// Sheet is one piece of paper - its size, its frame, the few title-block
// values it overrides, and its viewports. A Viewport is a window onto the
// drawing (a plan, a long section, cross sections, a 3D snapshot) or a panel
// of paper furniture (a legend, notes, an image, a key plan) at a rectangle on
// the paper.
//
// All of it is plain values: saved as versioned JSON under the project's
// "sheets" metadata key (sheet_json.hpp), changed through undoable commands
// (sheet_commands.hpp), and painted by the Qt layer. Paper rectangles are
// millimetres from the bottom-left corner of the paper, Y up (frame.hpp).
//
// Title-block FIELDS resolve automatically - sheet number and count, scale,
// dates, project, coordinate system, file name - and a value the user typed
// always wins (resolveFields).

#include <cstddef>
#include <map>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/cad/layer_overrides.hpp"
#include "katana/cad/plot.hpp"
#include "katana/cad/plotting/frame.hpp"
#include "katana/core/error.hpp"
#include "katana/geometry/primitives2d.hpp"

namespace katana::cad::plotting {

// What a viewport shows. Every kind has a tiling rank (layout.hpp).
enum class ViewportKind {
    Plan,          // the drawing in plan, at a scale, rotated to suit
    LongSection,   // a profile along an alignment
    CrossSections, // one or more cross sections of an alignment
    Model3D,       // a snapshot of the 3D view
    Legend,        // the styles and layers drawn on the sheet
    Notes,         // free text
    Image,         // an image from the project's assets
    KeyPlan,       // where the other sheets are: their outlines, numbered
    SheetIndex,    // the drawing register: every sheet's number, title, scale,
                   // paper and revision (tables.hpp)
    Revisions,     // the revision table, newest first (tables.hpp)
};

[[nodiscard]] std::string_view toString(ViewportKind kind);
[[nodiscard]] std::optional<ViewportKind> viewportKindFrom(std::string_view name);

// What a section or strip viewport is cut from.
struct ViewportSource {
    std::string alignment;        // the alignment's name; empty for a plain plan
    double chainageFrom = 0.0;    // the chainage range shown (LongSection, strips)
    double chainageTo = 0.0;
    double sectionInterval = 0.0; // CrossSections: every so many metres, when > 0
    std::vector<double> stations; // CrossSections: these chainages, when given
    double sectionHalfWidth = 0.0;

    friend bool operator==(const ViewportSource&, const ViewportSource&) = default;
};

// A line or outline drawn over a viewport in world coordinates: the match
// line where the next sheet takes over, or a key plan's sheet outlines.
struct WorldMark {
    enum class Kind { MatchLine, SheetOutline };
    Kind kind = Kind::MatchLine;
    std::vector<geometry::Point2> points;
    // "MATCH LINE CH 250.000"; the sheet it leads to is added when it is
    // drawn (markLabel), so reordering the sheets cannot make it lie.
    std::string label;
    // The id of the sheet this mark refers to; empty for none.
    std::string sheet;

    friend bool operator==(const WorldMark&, const WorldMark&) = default;
};

// What a Legend viewport lists (legend.hpp): what the plans of its own sheet
// show, what the plans of every sheet show, or everything drawn. Stored by
// name ("this_sheet", "whole_set", "whole_drawing"), never renamed.
enum class LegendScope {
    ThisSheet,
    WholeSet,
    WholeDrawing,
};

struct Viewport {
    std::string id; // unique within the sheet set; "vp1", "vp2" ...
    ViewportKind kind = ViewportKind::Plan;
    Box2 rect;      // on the paper, millimetres
    // 1 : scale. With autoScale the painter chooses the largest standard scale
    // (kSheetScales) at which the content fits, and this is the last choice.
    double scale = 500.0;
    bool autoScale = false;
    // The world point at the rectangle's centre. For a section it is (offset
    // or chainage, elevation); with autoCentre the painter centres the
    // content, keeping the scale - a section whose ground is not sampled yet.
    geometry::Point2 centre{};
    bool autoCentre = false;
    // Radians counter-clockwise: the world direction that runs left to right
    // across the paper. A strip along an alignment is rotated to its bearing.
    double rotation = 0.0;
    double verticalExaggeration = 1.0; // sections only
    double tiltDegrees = 30.0;         // Model3D: the camera's height angle
    ViewportSource source;
    LayerOverrides hiddenLayers; // hidden in this viewport, as a view hides them
    std::string title;           // empty: automaticTitle()
    bool northArrow = false;
    bool scaleBar = false;
    bool locked = false; // tiling and snapping leave a locked viewport alone
    std::string text;    // Notes: the text; Image: the asset's file name
    std::vector<WorldMark> marks;
    // Revisions: only the newest so many; 0 shows every revision.
    std::size_t revisionLimit = 0;
    LegendScope legendScope = LegendScope::ThisSheet; // Legend only

    friend bool operator==(const Viewport&, const Viewport&) = default;
};

// The scale and centre a plan viewport is drawn at once "auto" is decided:
// what the legend reads its window from (legend.hpp) and what preflight
// checks (preflight.hpp), both resolved by the painter's rule.
struct PlanWindow {
    double scale = 500.0;
    geometry::Point2 centre{};

    friend bool operator==(const PlanWindow&, const PlanWindow&) = default;
};

struct Sheet {
    // Stable for the sheet's life - "s1", "s2" ... - so a match line or key
    // plan can refer to it however the sheets are reordered or renamed; and
    // not given to a new sheet while a mark still names it (newSheetIds).
    std::string id;
    std::string name;
    PaperSize paper = PaperSize::A3;
    bool landscape = true;
    std::string frame{kBuiltInFrameId}; // empty: no frame
    bool frameLegend = true;             // the frame's utility legend block
    // Title-block values this sheet overrides, by field name. A value here
    // always wins over the set's defaults and over anything automatic.
    std::map<std::string, std::string, std::less<>> fields;
    std::vector<Viewport> viewports; // back to front

    friend bool operator==(const Sheet&, const Sheet&) = default;
};

// A sign-off row: who, and when. An empty date prints the plot date.
struct SignOff {
    std::string name;
    std::string date;

    friend bool operator==(const SignOff&, const SignOff&) = default;
};

struct Revision {
    std::string code; // "A", "B", "0" ...
    std::string date;
    std::string description;
    std::string by;

    friend bool operator==(const Revision&, const Revision&) = default;
};

// The title-block values every sheet shares. An empty value falls back to
// what the project knows (docs/plotting.md lists which).
struct SheetDefaults {
    std::string organisation;
    std::vector<std::string> projectLines; // up to four; the first two default
                                           // to the project's name and description
    std::string client;
    SignOff locator;  // UTILITIES
    SignOff surveyor; // SURVEYED
    SignOff compiler; // COMPILED
    SignOff reviewer; // REVIEWED
    SignOff approver; // the manager's row under the notes
    std::string notes;
    std::string heightDatum;
    std::string coordinateSystem; // empty: the project's coordinate system
    std::string modelName;
    std::string setNumber;
    std::string logoAsset; // a file in the project's assets/ directory

    friend bool operator==(const SheetDefaults&, const SheetDefaults&) = default;
};

inline constexpr int kSheetSetVersion = 1;

struct SheetSet {
    SheetDefaults defaults;
    // How a sheet number is written: {n} is the sheet's position from 1,
    // {n:02} the same padded to two digits, {N} the count, {set} the set
    // number. "{n}" by default, as the frame's SHEET No. cell expects.
    std::string numbering = "{n}";
    // Oldest first, in the order issued: the last is the current revision,
    // whatever its date or code says (resolveFields, tables.hpp).
    std::vector<Revision> revisions;
    std::vector<Sheet> sheets;

    friend bool operator==(const SheetSet&, const SheetSet&) = default;
};

// What the project contributes to the fields; filled by the caller so this
// stays a pure function (fieldContextFor in sheet_commands.hpp builds one from
// a Document).
struct FieldContext {
    std::string projectName;
    std::string projectDescription;
    std::string coordinateSystem;
    std::string fileName;
    std::string plotDate; // as it is printed: the frame writes dd/mm/yy
};

// Every field of sheet `sheetIndex`, resolved: an override on the sheet, else
// the set's value, else the automatic one. Empty map for an index past the
// end. The names are those frame texts use: sheet_number, sheet_count,
// sheet_name, scale, paper, plot_date, <role>_name and <role>_date for the
// five sign-offs, organisation, client, project_line_1..4, set_number,
// coordinate_system, height_datum, model_name, file_name, notes, revision.
[[nodiscard]] std::map<std::string, std::string, std::less<>>
resolveFields(const SheetSet& set, std::size_t sheetIndex, const FieldContext& context);

// `text` with each {name} replaced by fields[name]; an unknown name becomes
// empty text. "{{" is a literal brace.
[[nodiscard]] std::string
expandTemplate(std::string_view text, const std::map<std::string, std::string, std::less<>>& fields);

// The sheet number as `pattern` writes it for sheet `number` of `count`.
[[nodiscard]] std::string formatSheetNumber(std::string_view pattern, std::size_t number,
                                            std::size_t count, std::string_view setNumber);

// How a scale is written: "1:500"; a section says both, "H 1:500 V 1:50",
// even when they are equal - a lone 1:500 leaves the reader to assume the
// vertical, and that is the assumption that goes wrong.
[[nodiscard]] std::string scaleText(const Viewport& viewport);

// "PLAN 1:500", "LONG SECTION H 1:500 V 1:50", "CROSS SECTION CH 120.000"...
[[nodiscard]] std::string automaticTitle(const Viewport& viewport);

// The scale the title block reports: the main viewport's (the lowest tiling
// rank among plans and sections), "AS SHOWN" when those disagree. A key plan
// beside a plan has a scale of its own and does not make the sheet "AS
// SHOWN"; on a sheet with no plan or section - a key-plan sheet - the key
// plan's scale is the sheet's. "N.T.S." when nothing is drawn to scale.
[[nodiscard]] std::string sheetScaleText(const Sheet& sheet);

// Where a sheet's viewports go: the frame's drawing area, or the paper less
// 10 mm all round when the sheet has no frame (or cannot have one).
[[nodiscard]] Box2 drawingArea(const Sheet& sheet);

// `count` viewport ids, in order, that no viewport in `set` has: "vp" and the
// numbers after the highest in use. Ids come from stored JSON unchecked, so
// one too near the largest count to have `count` numbers after it gives the
// lowest free numbers instead; nothing here throws or repeats an id.
[[nodiscard]] std::vector<std::string> newViewportIds(const SheetSet& set, std::size_t count);
// `count` sheet ids, in order, that no sheet in `set` has and no mark in it
// names: a sheet removed while a match line or a key-plan outline still leads
// to it keeps its id from the sheets added after it, so the mark leads
// nowhere (markLabel prints its label alone) rather than to a stranger.
[[nodiscard]] std::vector<std::string> newSheetIds(const SheetSet& set, std::size_t count);
// One of each, as above.
[[nodiscard]] std::string nextViewportId(const SheetSet& set);
[[nodiscard]] std::string nextSheetId(const SheetSet& set);
// The index of the sheet with id `id`, if there is one.
[[nodiscard]] std::optional<std::size_t> sheetIndex(const SheetSet& set, std::string_view id);

// What a mark says on paper: its label, and for a mark that refers to a sheet
// " - SEE SHEET " and that sheet's number as the set numbers it now. A key
// plan's outline is labelled with the number alone.
[[nodiscard]] std::string markLabel(const SheetSet& set, const WorldMark& mark);

} // namespace katana::cad::plotting
