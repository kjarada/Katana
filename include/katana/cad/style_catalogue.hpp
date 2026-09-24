#pragma once

// What a linetype picker and a symbol picker offer, what each name is, and
// which names the drawing uses that resolve to nothing - the logic of the
// style, linetype and symbol managers, below Qt so it can be tested.
//
// THE RULES (the lead's decisions D2 and D3, 2026-09-23):
//
//   A line's linetype NAME resolves to a NON-vertex library definition of
//   that name first (drawn by its strokes, with no Katana dash applied), else
//   to a model Linetype (dashes), else to a solid line. A name that is both
//   is a COLLISION: reported, not refused. "ByLayer" on a Style inherits the
//   layer's linetype.
//
//   A library definition is offered as a SYMBOL if any of: it is `mode
//   vertex`; a VertexSymbol rule of the survey map names it; a Style::symbol
//   names it; its source file's name contains "symbol" in any case (symbol
//   libraries are conventionally named *symbols*.4d). It is offered as a
//   LINESTYLE if it is not `mode vertex`. It may be both: most symbols the
//   reference survey code files use are not `mode vertex`, which is why
//   atVertices alone cannot decide.
//
//   A picker never relies on its list being complete: keepCurrent puts the
//   value being edited back, marked, when the list lacks it. That - not a
//   more complete list - is what stops an editor rewriting a name it could
//   not show (audit QT-02).
//
//   Names are case-SENSITIVE, as they are everywhere in the model; a search
//   may fold case, storage never does.

#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/entity/style_library.hpp"
#include "katana/entity/table_usage.hpp"

namespace katana::cad {

enum class DefinitionSource {
    ModelLinetype, // a dash pattern in the model's Linetype table
    Library,       // a library definition in the Document's StyleLibrary
    BuiltIn,       // one of Katana's own: a built-in symbol shape, or ByLayer
    Undefined,     // nothing defines it: a current value keepCurrent put back
};

[[nodiscard]] std::string_view toString(DefinitionSource source);

// D3 for one library definition, with each reason kept so a browser can say
// WHY a definition is listed where it is.
struct DefinitionKind {
    bool linestyle = false; // offered as a linestyle: not `mode vertex`
    bool symbol = false;    // offered as a symbol: any of the four below
    bool atVertices = false;
    bool namedBySurveyRule = false; // a VertexSymbol rule names it
    bool namedByStyle = false;      // some Style::symbol names it
    bool fromSymbolFile = false;    // LineStyle::source contains "symbol", any case

    [[nodiscard]] bool known() const { return linestyle || symbol; }

    friend bool operator==(const DefinitionKind&, const DefinitionKind&) = default;
};

// All false for a name the library does not define: a model linetype or a
// built-in symbol is not a library definition, and is offered by its own
// source.
[[nodiscard]] DefinitionKind classifyDefinition(const Document& document, std::string_view name);

struct CatalogueEntry {
    std::string name{};
    DefinitionSource source = DefinitionSource::Library;
    std::string sourceFile{}; // LineStyle::source; empty unless source is Library
    std::string group{};      // LineStyle::group; empty unless source is Library
    std::optional<katana::entity::StyleUnits> units{}; // Library only
    // D3's flags for a library definition; a model linetype or ByLayer is
    // just a linestyle, and a built-in shape just a symbol.
    DefinitionKind kind{};
    bool atVertices = false; // `mode vertex`; kind.atVertices, said where a picker looks
    // Linetypes: also a model Linetype, which the library definition wins
    // over (D2). Reported so a person can rename one of the two.
    bool collision = false;
    // Defined nowhere: a current value keepCurrent put back.
    bool missing = false;
    // Who names it and the entities reaching it (entity::tableUsage).
    katana::entity::Users users{};
};

// For a Style's linetype (forStyle) the list starts with ByLayer; a layer's
// has no ByLayer. Then every model linetype and every library definition
// offered as a linestyle, one entry per name (a collision is one entry, the
// library's, marked), sorted by name with case folded.
[[nodiscard]] std::vector<CatalogueEntry> linetypeChoices(const Document& document,
                                                          bool forStyle = true);
// Every built-in symbol shape and every library definition offered as a
// symbol, one entry per name (a library definition hides a built-in of the
// same name, as it does when drawn), sorted by name with case folded.
[[nodiscard]] std::vector<CatalogueEntry> symbolChoices(const Document& document);

// `choices` with `current` added, marked missing, when no entry has that
// exact name; unchanged when one has, and for "" (nothing to keep).
[[nodiscard]] std::vector<CatalogueEntry> keepCurrent(std::vector<CatalogueEntry> choices,
                                                      std::string_view current);
// The entries whose name, group or source file contains `text`, case
// folded. Everything for an empty text.
[[nodiscard]] std::vector<CatalogueEntry> filterChoices(const std::vector<CatalogueEntry>& choices,
                                                        std::string_view text);

// ---- diagnostics -----------------------------------------------------------------

enum class NameRole { Linetype, Symbol };

[[nodiscard]] std::string_view toString(NameRole role);

// What a name a Style or a Layer gives draws as - THE rule for "is this name
// missing", read by missingNames here and by customisationCoverage
// (survey_coding.hpp), and so by the style manager's Diagnostics, the symbol
// library, CUSTOMISE and the window's unresolved-names log. Each once had a
// rule of its own, and they disagreed: a linetype naming only a `mode vertex`
// definition was "defined" to one of them while the viewport drew it solid,
// and built-in symbol names were "in no loaded library" to another (audit
// CAD-17). Worked out through resolveLinePattern and resolveSymbol, the
// viewport's own answers, so a name is missing exactly when what is drawn is
// a fallback.
enum class NameStatus {
    Plain,     // names nothing: "", ByLayer, "continuous", "0", "1" in any case (D4)
    OwnSymbol, // a style's linetype that is its own symbol's name - an archive import's
               // pattern for a symbol string - drawn as a plain line under the symbol (D8)
    Library,   // a loaded library definition draws it
    Katana,    // Katana draws it itself: a model linetype, or a built-in symbol shape
    NotALinestyle, // a linetype naming only a `mode vertex` definition: a symbol, not a
                   // linestyle, so the line is drawn solid (D2)
    Undefined,     // nothing defines it: drawn solid, or as the built-in shape its name suggests
};

// "plain line", "own symbol", "library", "built in", "not a linestyle", "defined nowhere".
[[nodiscard]] std::string_view toString(NameStatus status);

// What missingNames reports: a name whose drawing is a fallback.
[[nodiscard]] constexpr bool isMissing(NameStatus status)
{
    return status == NameStatus::NotALinestyle || status == NameStatus::Undefined;
}

// A linetype as a Style or Layer names it. `ownSymbol` is the Style's own
// symbol, for D8's exception; a Layer has no symbol and passes "".
[[nodiscard]] NameStatus linetypeStatus(const Document& document, std::string_view linetype,
                                        std::string_view ownSymbol = {});
// A Style's symbol: Plain for "" (the viewport's plain point mark), Library,
// Katana for a built-in shape, else Undefined. Never NotALinestyle: every
// library definition, `mode vertex` or not, can be drawn as a symbol (D3).
[[nodiscard]] NameStatus symbolStatus(const Document& document, std::string_view symbol);

// A name a Style or Layer uses whose drawing is a fallback.
struct MissingName {
    std::string name{};
    NameRole role = NameRole::Linetype;
    // Who names it, and the entities reaching it. For a linetype, not the
    // styles that name it as their own symbol (D8): those draw no pattern.
    katana::entity::Users users{};
    // What is drawn instead: "continuous" (a solid line) for a linetype, and
    // for a symbol the built-in shape entity::builtInSymbolFor picks.
    std::string fallback{};
    // Why: Undefined (nothing defines it), or, for a linetype only,
    // NotALinestyle (a `mode vertex` symbol defines it, and no linestyle).
    NameStatus status = NameStatus::Undefined;
};

// Every name a Style or Layer gives whose status isMissing - linetypes first,
// then symbols, each ascending. So NOT missing: a model linetype, a library
// linestyle, a built-in symbol name ("cross" draws a cross; audit CAD-17),
// ByLayer, the plain-line names "continuous", "0" and "1" in any case (D4),
// and a style's linetype that is its own symbol's name (D8), which is what
// an archive import writes for every symbol string. Missing, as "not a
// linestyle": a linetype naming only a `mode vertex` definition, which the
// viewport draws solid and a linetype picker does not offer (D2, D3).
[[nodiscard]] std::vector<MissingName> missingNames(const Document& document);

// Names that are both a model Linetype and a non-vertex library definition
// (D2's collisions), ascending.
[[nodiscard]] std::vector<std::string> linetypeCollisions(const Document& document);

// `base` when the style table has no such name, else "base 2", "base 3" ...
// the first free one. For the style manager's Duplicate and "New Style Using
// This", which then offer the name to be changed, and for the symbol
// library's Assign, which names a new style after its symbol.
[[nodiscard]] std::string freeStyleName(const katana::entity::Model& model, std::string_view base);

} // namespace katana::cad
