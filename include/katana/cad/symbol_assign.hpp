#pragma once

// The symbol library's logic, below Qt so it can be tested: what the library
// lists, who uses a symbol, how big one prints, putting a symbol on points
// and swapping one symbol for another in the styles.
//
// A POINT CARRIES NO SYMBOL OF ITS OWN. Style::symbol does (tables.hpp says
// why: "the style of a point" and "the style of a line" are one thing), so
// "give these points the TEST Tree symbol" means "put them in a style that
// draws TEST Tree". assignSymbolToPoints finds such a style, or makes one,
// and moves the points into it - in ONE undo step, so a person who undoes it
// gets back both the points' old styles and a table without the new style.
//
// THE STYLE IT REUSES says nothing of its own beyond the symbol: colour,
// linetype and hatch ByLayer, the default line weight, the exact symbol name
// (names are case-sensitive, D3) and the exact size. A style that also sets a
// colour is someone's deliberate choice for other points, and reusing it
// would recolour these; one that differs only in its description does not
// draw differently, so the description is not compared. The line weight has
// no ByLayer in the model (a style's always wins, entity::resolveDisplay),
// so "says nothing" there means the default a new Style gets.
//
// THE LIBRARY LIST is D3's: every definition symbolChoices offers as a symbol
// (and the built-in shapes it does not hide), plus every symbol name a Style
// or a survey code gives that nothing defines - listed, marked missing, with
// the built-in shape it is drawn as - because that is exactly what a person
// opening a symbol library to fix a drawing needs to see.

#include <cstddef>
#include <functional>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/cad/style_catalogue.hpp"
#include "katana/cad/style_drawing.hpp"
#include "katana/commands/command.hpp"
#include "katana/core/error.hpp"
#include "katana/entity/entity.hpp"
#include "katana/entity/model.hpp"
#include "katana/entity/style_library.hpp"
#include "katana/entity/survey_map.hpp"

namespace katana::cad {

// ---- who uses a symbol --------------------------------------------------------------------

// A survey code that draws a symbol at its vertices (a VertexSymbol rule).
struct SymbolCode {
    std::string key{};    // as written: "AC*"
    double size = 0.0;    // SurveySymbol::size; 0 is the definition's own
    std::string colour{}; // a standard colour name; empty is the string's own
    std::string comment{};

    friend bool operator==(const SymbolCode&, const SymbolCode&) = default;
};

// The VertexSymbol rules naming `name` exactly, in the map's order.
[[nodiscard]] std::vector<SymbolCode> symbolCodes(const katana::entity::SurveyMap& map,
                                                  std::string_view name);

struct SymbolUsers {
    std::vector<std::string> styles{};              // naming it, ascending
    std::vector<katana::entity::EntityId> points{}; // points wearing them, ascending
    // Other entities wearing those styles - lines, which draw the symbol at
    // every vertex (D8). Counted, not listed: "select the points" is the
    // action a symbol library offers.
    std::size_t otherEntities = 0;

    friend bool operator==(const SymbolUsers&, const SymbolUsers&) = default;
};

// Read through entity::tableUsage, the one answer the managers, the delete
// guards and purge share. Empty for "" (no symbol is not a name).
[[nodiscard]] SymbolUsers symbolUsers(const Document& document, std::string_view name);

// ---- what the library lists --------------------------------------------------------------

struct SymbolLibraryEntry {
    // From symbolChoices, or for a name nothing defines one with source
    // Undefined and `missing` set; `users` is tableUsage's either way.
    CatalogueEntry entry{};
    // Missing only: the built-in shape it is drawn as instead
    // (entity::builtInSymbolFor).
    std::string fallback{};
    std::vector<SymbolCode> codes{};
};

// symbolChoices, then every symbol name a Style or a VertexSymbol rule gives
// that no library defines and that is not built in, all sorted by name with
// case folded as a picker sorts.
[[nodiscard]] std::vector<SymbolLibraryEntry> symbolLibrary(const Document& document);

// ---- how big it prints -----------------------------------------------------------------------

struct SymbolPrintSize {
    double groundWidth = 0.0;  // metres on the ground
    double groundHeight = 0.0;
    double paperWidth = 0.0;   // millimetres on the plot
    double paperHeight = 0.0;
    // The point the symbol is put on lies within what it draws. False for a
    // definition drawn away from its insertion point: every point wearing it
    // shows its mark somewhere else.
    bool insertionInside = true;
};

// The box a symbol's text covers, in the model units of the mark, ESTIMATED:
// the face a text is painted in is a Qt question, and cad has no font. The
// text is taken to fill its `height` - the size it is painted at, which its
// capitals and ascenders stay within - standing on its anchor, hanging from
// it ("top") or centred on it ("middle"); and to be kEstimatedCharacterWidth
// of that height, times its widthFactor, wide per character, starting at the
// anchor, centred on it ("centre") or ending at it ("right"). The box is then
// turned about the anchor by the text's angle. The justification is read as
// the painter reads it (style_painter.cpp's justifiedOrigin), so an unknown
// spelling stands on the anchor from its left, as an unjustified text does.
// Descenders are not counted. Empty for an empty text or a height that is
// not a positive number.
//
// 0.6 is about an average sans-serif character: Arial's digits are 0.556 of
// the size they are set at, its capital V 0.667, most of its lower case 0.5.
inline constexpr double kEstimatedCharacterWidth = 0.6;
[[nodiscard]] katana::geometry::Box2 estimatedTextExtent(const StyleTextMark& text);

// Where a text lands, as a box in the text's own model units. A caller that
// has the font the text is painted in - a Qt dialog, through
// style_painter's styleTextExtent - measures it, so its answer agrees with
// the picture beside it; one that does not uses estimatedTextExtent.
using TextExtent = std::function<katana::geometry::Box2(const StyleTextMark&)>;

// What a point wearing `definition` at `size` (Style::symbolSize: a width in
// model units, 0 for the definition's own) draws, plotted at 1:N: the box of
// cad::symbolDrawing's strokes and of the space each of its texts covers, so
// it is what prints - a worldstyle's units are ground metres, a paperstyle's
// plot millimetres, and a size scales the definition so the larger side of
// LineStyle::bounds() spans it. A text's space is `textExtent`'s answer, or
// estimatedTextExtent's when none is given: a text's anchor alone has no
// size, and a symbol that is a letter, or carries one above its mark, prints
// the letter too. nullopt for a definition that draws nothing, a denominator
// that is not positive or a size that is not a finite number of at least 0.
[[nodiscard]] std::optional<SymbolPrintSize>
symbolPrintSize(const katana::entity::LineStyle& definition, double size, double scaleDenominator,
                const TextExtent& textExtent = {});

// ---- changing which symbol is drawn ----------------------------------------------------------

// A style that draws exactly `symbolName` at exactly `size` and says nothing
// else (see the header comment), or nullptr. When several do, the one named
// after the symbol, else the first by name.
[[nodiscard]] const katana::entity::Style* findSymbolStyle(const katana::entity::Model& model,
                                                           std::string_view symbolName,
                                                           double size);

struct SymbolAssignment {
    // ONE undo step: the new style, when one is made, and the points moved
    // into it. Null when there is nothing to change - every point among the
    // ids already wears the style - so no empty step is ever pushed.
    katana::commands::CommandPtr command{};
    std::string style{};       // the style the points wear afterwards
    bool createsStyle = false; // `style` is new, and `command` makes it
    std::size_t points = 0;         // points that move into `style`
    std::size_t alreadyInStyle = 0; // points already wearing it
    std::size_t notPoints = 0;      // other entities among the ids: left as they are
    std::size_t notFound = 0;       // ids no entity has
};

// Fails with InvalidArgument for an empty symbol name, a size that is not a
// finite number of at least 0, or ids among which there is no point at all
// (the message counts what there was instead). A locked layer is refused
// when the command executes, and the whole step with it.
[[nodiscard]] katana::core::Result<SymbolAssignment>
assignSymbolToPoints(const Document& document, const std::vector<katana::entity::EntityId>& ids,
                     std::string_view symbolName, double size);

// Every style whose symbol is exactly `from` set to `to`, in one undo step;
// nullptr when no style names `from`, or `from` is "" or equal to `to`. `to`
// may be any name, "" included (the plain point mark): it is a name resolved
// when drawn, as every Style::symbol is.
[[nodiscard]] katana::commands::CommandPtr replaceSymbolInStyles(const katana::entity::Model& model,
                                                                 std::string_view from,
                                                                 std::string_view to);

} // namespace katana::cad
