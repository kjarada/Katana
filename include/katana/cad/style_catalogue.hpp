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
//   names it; its source file's name contains "symbol" in any case (12d's
//   own user_symbols_*.4d). It is offered as a LINESTYLE if it is not `mode
//   vertex`. It may be both: most symbols the reference mapfiles use are not
//   `mode vertex`, which is why atVertices alone cannot decide.
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
    Library,       // a 12d definition in the Document's StyleLibrary
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

// A name a Style or Layer uses that nothing defines.
struct MissingName {
    std::string name{};
    NameRole role = NameRole::Linetype;
    katana::entity::Users users{}; // who names it, and the entities reaching it
    // What is drawn instead: "continuous" (a solid line) for a linetype, and
    // for a symbol the built-in shape entity::builtInSymbolFor picks.
    std::string fallback{};
};

// Every Style or Layer linetype and every Style symbol that is in no model
// table, in no loaded library and not built in - linetypes first, then
// symbols, each ascending. NOT missing: a built-in symbol name ("cross" draws
// a cross; audit CAD-17 was reporting it), ByLayer, and the plain-line names
// "continuous", "0" and "1" in any case (a 12d plain line; D4).
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
