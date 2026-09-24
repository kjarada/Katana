#pragma once

// What the styles and linetypes manager shows and what its form writes, below
// Qt so it can be tested: the rows of its three tables (styles; drawing
// linetypes with library linestyles; diagnostics), its filters, the fields a
// multi-selection agrees on, and the ONE command a bulk edit makes.
//
// The dialog (src/katana_qt/style_manager.cpp) only renders these. Every
// count comes from one entity::tableUsage pass and every "missing" from
// cad::missingNames, so the manager, the delete guards and purge cannot
// disagree about what is used or what is defined.

#include <cstddef>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/cad/style_catalogue.hpp"
#include "katana/commands/command.hpp"
#include "katana/core/error.hpp"
#include "katana/entity/entity.hpp"
#include "katana/entity/model.hpp"
#include "katana/entity/table_usage.hpp"
#include "katana/entity/tables.hpp"

namespace katana::cad {

// ---- styles ----------------------------------------------------------------------

struct StyleRow {
    katana::entity::Style style{};
    // The entities wearing it (entity::tableUsage), which is what "Used"
    // means for a style: a layer cannot name a style.
    std::size_t entities = 0;
    // Its linetype or symbol is a name cad::missingNames reports: nothing
    // defines it, and it is drawn by a fallback.
    bool missingLinetype = false;
    bool missingSymbol = false;
    // Document::currentStyle: new work is drawn in it (decision D9).
    bool current = false;

    [[nodiscard]] bool missing() const { return missingLinetype || missingSymbol; }
};

// Every style of the model, ascending by name (the table's order).
[[nodiscard]] std::vector<StyleRow> styleRows(const Document& document);

// The manager's chips: All / Used / Unused / Missing.
enum class StyleFilter { All, Used, Unused, Missing };

[[nodiscard]] bool matchesFilter(const StyleRow& row, StyleFilter filter);
// The search box: `text` found in the name, linetype, symbol, hatch pattern
// or description, ASCII case folded (decision D3: a search folds, storage
// never does). Everything matches an empty text.
[[nodiscard]] bool matchesSearch(const StyleRow& row, std::string_view text);

// ---- the form over one style or several ------------------------------------------

// The editable fields of a Style, each either set or not. Read as what a
// selection AGREES on (commonFields: unset = the form shows <varies>), and as
// what a person EDITED (applyEdit: unset = leave every style's own value).
// The name is not here: renaming is its own command, so the entities wearing
// a style come with it.
struct StyleFields {
    std::optional<std::string> linetype{};
    std::optional<double> lineWeight{};
    // The outer optional says whether the field is set; the inner one is the
    // Style's own (empty = ByLayer).
    std::optional<std::optional<katana::entity::Color>> color{};
    std::optional<std::string> hatchPattern{};
    std::optional<std::string> symbol{};
    std::optional<double> symbolSize{};
    std::optional<std::string> description{};

    [[nodiscard]] bool empty() const;
    friend bool operator==(const StyleFields&, const StyleFields&) = default;
};

// The fields every one of `styles` has the same value for, compared exactly
// (a weight of 0.25 and one of 0.2500001 differ, and so show <varies>).
// Every field set for one style; none for no styles.
[[nodiscard]] StyleFields commonFields(const std::vector<katana::entity::Style>& styles);

// `style` with each SET field of `edit` written over it; the rest of it -
// and its name - exactly as it was.
[[nodiscard]] katana::entity::Style applyEdit(katana::entity::Style style,
                                              const StyleFields& edit);

// ONE command writing `edit` into every style named: one undo step however
// many there are, holding only the styles it changes (updateStyleIfChanged
// for each). nullptr when it changes none - an unedited Save is not an edit
// and pushes no undo step (audit QT-02, QT-01's shape). Fails with NotFound
// for a name the model has no style of, and with validate(Style)'s error,
// naming the style, for an edit that would make one invalid - before anything
// is built, so a refused bulk edit changes nothing.
[[nodiscard]] katana::core::Result<katana::commands::CommandPtr>
editStylesCommand(const katana::entity::Model& model, const std::vector<std::string>& names,
                  const StyleFields& edit);

// ---- linetypes and library linestyles ----------------------------------------------

enum class LinetypeOrigin {
    Drawing, // a model Linetype: a dash pattern, saved with the project
    Library, // a non-vertex library definition of the session's StyleLibrary (D2)
};

// What draws the line: a Katana dash pattern, or a library definition laid by
// its units.
enum class LinetypeRowKind {
    Dash,     // model Linetype; continuous when its pattern is empty
    Paper,    // Paper linestyle (`paperstyle`): millimetres on the plot
    World,    // World linestyle (`worldstyle`): model units
    TwoPoint, // Two-point linestyle (`twoptstyle`): stretched between two anchors
};

[[nodiscard]] std::string_view toString(LinetypeOrigin origin);
[[nodiscard]] std::string_view toString(LinetypeRowKind kind);

struct LinetypeRow {
    std::string name{};
    LinetypeOrigin origin = LinetypeOrigin::Drawing;
    LinetypeRowKind kind = LinetypeRowKind::Dash;
    std::string group{};       // LineStyle::group; empty for a drawing linetype
    std::string sourceFile{};  // LineStyle::source; empty for a drawing linetype
    std::string description{}; // Linetype::description; empty for a library one
    // One repeat of the pattern: model units for a Dash or World row,
    // millimetres on paper for a Paper row, as the drawing code lays it (the
    // file's `length`, else the pen's travel). 0 for a continuous linetype
    // and for a TwoPoint row, which is stretched and has no period.
    double period = 0.0;
    // Who names the NAME and the entities drawn with it. Shared by the two
    // rows of a collision: both answer to the same name.
    katana::entity::Users users{};
    // The name is both a drawing linetype and a library linestyle (D2): the
    // library row is what is drawn, the drawing row is shadowed.
    bool collision = false;
};

// Every drawing linetype and every library definition offered as a
// linestyle (not `mode vertex`), ascending by name with case folded and, for
// one name, the drawing row first. A collision is TWO rows, so the shadowed
// drawing linetype can still be edited, merged or renamed out of the way.
[[nodiscard]] std::vector<LinetypeRow> linetypeRows(const Document& document);

// ---- diagnostics -------------------------------------------------------------------

enum class StyleDiagnosticKind {
    MissingLinetype, // a Style or Layer linetype nothing defines
    MissingSymbol,   // a Style symbol nothing defines
    Collision,       // a drawing linetype a library linestyle shadows (D2)
};

[[nodiscard]] std::string_view toString(StyleDiagnosticKind kind);

struct StyleDiagnostic {
    StyleDiagnosticKind kind = StyleDiagnosticKind::MissingLinetype;
    std::string name{};
    katana::entity::Users users{};
    // What is drawn meanwhile, in words: "a solid line (continuous)", "the
    // built-in cross", "the library linestyle; the drawing's pattern is not
    // drawn".
    std::string drawnAs{};
};

// cad::missingNames (linetypes, then symbols), then cad::linetypeCollisions,
// each ascending - the manager's Diagnostics tab.
[[nodiscard]] std::vector<StyleDiagnostic> styleDiagnostics(const Document& document);

// ---- select users -------------------------------------------------------------------

enum class UsageTable { Style, Linetype, Symbol };

// The entities reaching any of `names` in `table` - wearing the style, drawn
// with the linetype, drawn with the symbol - by entity::tableUsage's rule,
// ascending and without repeats. What "Select Users" selects.
[[nodiscard]] std::vector<katana::entity::EntityId>
entitiesUsing(const katana::entity::Model& model, UsageTable table,
              const std::vector<std::string>& names);

// `base`, else "base 2", "base 3" ... for a drawing linetype: free in the
// model's table AND in the library, so a new linetype never starts life as a
// D2 collision. Its twin for styles, freeStyleName, is in style_catalogue.hpp,
// which the symbol library shares.
[[nodiscard]] std::string freeLinetypeName(const Document& document, std::string_view base);

} // namespace katana::cad
