#pragma once

// Text command line for a Document (PLAN.MD Phase 08 "command line").
//
// One line in, one command out: the interpreter only parses and then builds the
// same Command objects the GUI uses, so everything typed is validated, atomic
// and undoable. It has no GUI dependency; the desktop application and the
// headless `katana_cli` share it.
//
// Syntax
//   VERB arg arg ...                 verbs are case-insensitive, with aliases
//   points      12.5,40   absolute
//               @3,4      relative to the last point entered
//               @5<30     polar: distance < angle (degrees, counter-clockwise from +x)
//   angles      degrees
//   text        "quoted strings may contain spaces"
//   ids         entity ids as shown by LIST
// Numbers are parsed locale-independently ('.' is always the decimal point).
//
// HELP lists every command.

#include <optional>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include "katana/cad/code_table.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/import_placement.hpp"
#include "katana/cad/plotting/sheet_verbs.hpp"
#include "katana/cad/scope_verbs.hpp"
#include "katana/cad/view_verbs.hpp"
#include "katana/core/error.hpp"

namespace katana::cad {

namespace annotation {
struct AnchoredPoint;
}

class CommandInterpreter {
  public:
    explicit CommandInterpreter(Document& document) : document_(document) {}

    // Runs one line. On success returns a short human-readable message.
    // Empty lines succeed with an empty message.
    [[nodiscard]] katana::core::Result<std::string> run(std::string_view line);

    // Every non-empty line passed to run(), oldest first.
    [[nodiscard]] const std::vector<std::string>& history() const { return history_; }

    [[nodiscard]] static std::string helpText();

    // A line split into words as run() splits it: on whitespace, double
    // quotes grouping words and removed ("" is an empty word). ParseFailure
    // for a quote never closed. For a front end's own verbs (PLOTSHEETS), so
    // they read a line exactly as the interpreter would.
    [[nodiscard]] static katana::core::Result<std::vector<std::string>>
    tokenize(std::string_view line);

    // What the sheet verbs (plotting/sheet_verbs.hpp) cannot see in the
    // Document - the surfaces a cross section samples, the extent the plan
    // view draws - from the front end that has them. Headless there is none,
    // and the verbs do without.
    void setSheetContext(katana::cad::plotting::SheetVerbContextProvider provider)
    {
        sheetContext_ = std::move(provider);
    }

    // A standard colour name's RGB, for CODE and MAPFILE CHECK
    // (survey_code_verbs.hpp): the table is archive12d's, which cad may not
    // see, so the front end that can see it passes it. Unset, no colour is
    // known - CODE leaves colours alone and CHECK checks none.
    void setColourLookup(ColourLookup colourOf) { colourOf_ = std::move(colourOf); }

    // The rest of a front end's IMPORT line, after the verb: the path, one
    // layer of surrounding quotes removed so that a path may hold blanks, and
    // where the data is to land (import_placement.hpp), from the line's last
    // word when that is LOCAL, ALONGSIDE or OFFSET=dE,dN - a word of its own,
    // in any case, never inside the quotes, so a quoted "site LOCAL" is a
    // file, and never the only word, so a file called LOCAL can be imported.
    // The window and the session both read the line with it: the session
    // once took LOCAL off and left the quotes on, so every quoted IMPORT ...
    // LOCAL - what katana_import sends - looked for a file named with its
    // quotes. InvalidArgument for an OFFSET= that is not two numbers.
    struct ImportArgument {
        std::string path{};
        ImportPlacement placement{};
    };
    [[nodiscard]] static katana::core::Result<ImportArgument> importArgument(std::string_view rest);

    // Whether `word` names an entity as INFO takes one: a positive whole
    // number, or one written #n as an anchored point names its entity. For a
    // front end that reads INFO <file> first, so that INFO 12 still reaches
    // the entity when no file of that name exists.
    [[nodiscard]] static bool isEntityId(std::string_view word);
    // Whether `line` replaces the drawing: NEW, or an OPEN of a project
    // directory - not OPEN with no target, SELECTION or #ids, which opens
    // polylines (docs/drawing.md). The window asks it before its discard
    // question and its reset of the backdrop and views, the session before
    // its missing-customisation check, katana_mcp before its unsaved-changes
    // guard, so an OPEN #12 neither asks to discard the drawing nor is
    // refused for unsaved changes.
    [[nodiscard]] static bool replacesDocument(std::string_view line);
    // The verb `line` runs, as run() reads it: its first word in capitals,
    // the apostrophe of 'ZOOM taken off (only a view verb takes one; any
    // other word keeps it, and run() refuses it), and an alias resolved - Z
    // is ZOOM. "" for a line with no word or one that does not split. The
    // window asks it whether a line typed inside a tool is a ZOOM, so which
    // words are ZOOM is this table's alone.
    [[nodiscard]] static std::string verbOf(std::string_view line);
    // What the scope word VIEW means (scope_verbs.hpp): the window's active
    // plan view, or the view with the id given, with its own hidden layers
    // and its visible area. Headless there is none, and VIEW is refused in
    // favour of AREA.
    void setScopeContext(ScopeViewProvider provider) { scopeViews_ = std::move(provider); }
    // What VIEW is answered by, for a front end's own verbs that take the
    // shared scope (the geoprocessing verbs above katana_cad): empty
    // headless, so VIEW is refused there exactly as MODIFY refuses it.
    [[nodiscard]] const ScopeViewProvider& scopeContext() const { return scopeViews_; }
    // What VIEWS and ZOOM act on (view_verbs.hpp): the window's views. The
    // window supplies its workspace; headless there is none, and both verbs
    // are refused by name.
    void setViewHost(ViewHostProvider provider) { viewHost_ = std::move(provider); }

    // Forgets the "last point" that relative (@dx,dy) and polar (@d<a) points
    // resolve against. The interpreter cannot see a document being replaced
    // behind it, so a front end that swaps documents must say so - otherwise
    // "@10,10" after File > New silently measures from the discarded drawing.
    void resetPointState();

    // A property value as PROP SET reads one: of the stated `type` (text,
    // integer, real, boolean) when there is one, else guessed from the text -
    // "true" and "false" are booleans, a whole number an integer, another
    // number a real, anything else text. LEADER PROP and the window's
    // Leaders manager read a value the same way.
    [[nodiscard]] static katana::core::Result<katana::entity::PropertyValue>
    propertyValue(const std::string& text, const std::string* type);

  private:
    using Tokens = std::vector<std::string>;
    using Reply = katana::core::Result<std::string>;

    [[nodiscard]] katana::core::Result<katana::geometry::Point2> parsePoint(const std::string& text);
    [[nodiscard]] Reply requireSelection() const;
    [[nodiscard]] Reply finish(katana::core::Status status, std::string message);

    [[nodiscard]] Reply draw(const std::string& verb, const Tokens& args);
    [[nodiscard]] Reply transform(const std::string& verb, const Tokens& args);
    [[nodiscard]] Reply edit(const std::string& verb, const Tokens& args);
    [[nodiscard]] Reply select(const Tokens& args);
    [[nodiscard]] Reply layer(const Tokens& args);
    [[nodiscard]] Reply linetype(const Tokens& args);
    [[nodiscard]] Reply dimensionStyle(const Tokens& args);
    [[nodiscard]] Reply hatchPattern(const Tokens& args);
    [[nodiscard]] Reply style(const Tokens& args);
    [[nodiscard]] Reply alignment(const Tokens& args);
    // CRS: the project's coordinate system - shown, set, cleared, found in the
    // common list, suggested for a place (project_crs.hpp). `line` is the
    // whole line, which CRS SET reads its system from as typed.
    [[nodiscard]] Reply coordinateSystem(const Tokens& args, std::string_view line);
    [[nodiscard]] Reply parcel(const Tokens& args);
    // INVERSE, FORWARD (RADIATE) and AREA: the survey tools of survey_tools.hpp,
    // printing the same report the Survey menu's dialogs print.
    [[nodiscard]] Reply survey(const std::string& verb, const Tokens& args);
    [[nodiscard]] Reply attributes(const std::string& verb, const Tokens& args);
    // MODIFY: Global Modify (global_modify.hpp) on the shared scope and filter
    // words (scope_verbs.hpp); VIEW through the window's scope context.
    [[nodiscard]] Reply modify(const Tokens& args);
    [[nodiscard]] Reply undoRedo(const std::string& verb, const Tokens& args);
    [[nodiscard]] Reply file(const std::string& verb, const Tokens& args);
    [[nodiscard]] Reply inspect(const std::string& verb, const Tokens& args) const;

    // The annotation verbs (annotation/annotation_verbs.cpp, docs/annotation.md):
    // ANNOSCALE, TEXTSTYLE, TEXT and MTEXT with options, TEXTEDIT, LABELSTYLE,
    // LABEL, AUTOLABEL, the DIM kinds, LEADER and BALLOON. Options are
    // key=value; replies are key=value records, one per line.
    [[nodiscard]] static bool isAnnotationVerb(const std::string& verb, const Tokens& args);
    [[nodiscard]] static std::string annotationHelpText();
    [[nodiscard]] Reply annotation(const std::string& verb, const Tokens& args);
    [[nodiscard]] Reply annotationScale(const Tokens& args);
    [[nodiscard]] Reply textStyle(const Tokens& args);
    [[nodiscard]] Reply styledText(const std::string& verb, const Tokens& args);
    [[nodiscard]] Reply textEdit(const Tokens& args);
    [[nodiscard]] Reply labelStyle(const Tokens& args);
    [[nodiscard]] Reply label(const Tokens& args);
    [[nodiscard]] Reply autoLabel(const Tokens& args);
    [[nodiscard]] Reply dimension(const Tokens& args);
    [[nodiscard]] Reply leader(const std::string& verb, const Tokens& args);
    // A point that may name another entity's point - "#12", "#12.end",
    // "#12.v3", "#12.s2", "#12.mid", "#12.centre", "#12.inside" - or the
    // point of it nearest a point, "#12@x,y" (on it, Along) - or any point
    // parsePoint takes. A named point carries its reference, so what is made
    // from it follows the entity (annotation/associative.hpp).
    [[nodiscard]] katana::core::Result<annotation::AnchoredPoint>
    parseAnchoredPoint(const std::string& text);

    // The drawing system's verbs (drawing/drawing_verbs.cpp, docs/drawing.md):
    // VERTEX, WEED, DENSIFY, STRAIGHTEN, CLOSE, OPEN (of polylines),
    // STARTVERTEX, VERTEXZ, PLINE with ARC and LINE, PLINE3D, SPLINE, ELLIPSE,
    // XLINE, RAY, DLINE and the drafting settings. Options are key=value;
    // replies are key=value records, one per line.
    [[nodiscard]] static bool isDrawingVerb(const std::string& verb, const Tokens& args);
    [[nodiscard]] static std::string drawingHelpText();
    [[nodiscard]] Reply drawingVerb(const std::string& verb, const Tokens& args);
    [[nodiscard]] Reply vertexVerb(const Tokens& args);
    [[nodiscard]] Reply polylineVerb(const std::string& verb, const Tokens& args);
    [[nodiscard]] Reply drawShapeVerb(const std::string& verb, const Tokens& args);
    [[nodiscard]] Reply draftingVerb(const std::string& verb, const Tokens& args);
    // A point with an optional height, relative input measured from `from`
    // when given (a vertex being moved) or else from the last point.
    [[nodiscard]] katana::core::Result<PrecisePoint>
    parseDrawingPoint(const std::string& text,
                      std::optional<katana::geometry::Point2> from = std::nullopt);

    Document& document_;
    std::vector<std::string> history_;
    std::optional<katana::geometry::Point2> lastPoint_;
    katana::cad::plotting::SheetVerbContextProvider sheetContext_;
    ColourLookup colourOf_;
    ScopeViewProvider scopeViews_;
    ViewHostProvider viewHost_;
};

} // namespace katana::cad
