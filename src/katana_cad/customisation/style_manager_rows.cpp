#include "katana/cad/style_manager_rows.hpp"

#include <algorithm>
#include <set>
#include <utility>

#include "katana/cad/style_catalogue.hpp"
#include "katana/cad/style_drawing.hpp"
#include "katana/commands/command_stack.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/core/text.hpp"

namespace katana::cad {

namespace {

using katana::entity::LineStyle;
using katana::entity::Style;
using katana::entity::StyleUnits;
using katana::entity::TableUsage;
using NameSet = std::set<std::string, std::less<>>;

bool containsFolded(std::string_view text, const std::string& foldedNeedle)
{
    return katana::core::lowered(text).find(foldedNeedle) != std::string::npos;
}

// Every field of `style` set: the fields one style "agrees on".
StyleFields fieldsOf(const Style& style)
{
    return StyleFields{style.linetype,     style.lineWeight, style.color,
                       style.hatchPattern, style.symbol,     style.symbolSize,
                       style.description};
}

// Clears `field` unless `value` equals it: what commonFields folds with.
template <typename T> void keepIfEqual(std::optional<T>& field, const T& value)
{
    if (field && !(*field == value)) {
        field.reset();
    }
}

LinetypeRowKind kindOf(StyleUnits units)
{
    switch (units) {
    case StyleUnits::Paper:
        return LinetypeRowKind::Paper;
    case StyleUnits::TwoPoint:
        return LinetypeRowKind::TwoPoint;
    case StyleUnits::World:
        break;
    }
    return LinetypeRowKind::World;
}

// One repeat as the drawing code lays it (style_drawing.cpp, layLinestyle):
// the file's `length`, else how far the pen travelled - before the scale a
// paperstyle is multiplied by, so a Paper period is in plot millimetres.
double libraryPeriod(const LineStyle& definition)
{
    if (definition.units == StyleUnits::TwoPoint) {
        return 0.0;
    }
    const FlatDefinition flat = flattenDefinition(definition);
    return flat.length > 0.0 ? flat.length : flat.naturalPeriod();
}

std::string foldedKey(std::string_view name) { return katana::core::lowered(name); }

} // namespace

// ---- styles ----------------------------------------------------------------------

std::vector<StyleRow> styleRows(const Document& document)
{
    const katana::entity::Model& model = document.model();
    const TableUsage usage = katana::entity::tableUsage(model);

    // A style is "missing" when it NAMES a missing linetype or symbol itself:
    // missingNames lists the styles naming each, so this is a lookup, not a
    // second resolution that could disagree with it.
    NameSet missingLinetype;
    NameSet missingSymbol;
    for (const MissingName& missing : missingNames(document)) {
        NameSet& set = missing.role == NameRole::Linetype ? missingLinetype : missingSymbol;
        set.insert(missing.users.styles.begin(), missing.users.styles.end());
    }

    std::vector<StyleRow> rows;
    rows.reserve(model.styles.size());
    model.styles.forEach([&](const Style& style) {
        StyleRow row;
        row.style = style;
        row.entities = TableUsage::of(usage.styles, style.name).entities;
        row.missingLinetype = missingLinetype.contains(style.name);
        row.missingSymbol = missingSymbol.contains(style.name);
        row.current = !document.currentStyle().empty() && document.currentStyle() == style.name;
        rows.push_back(std::move(row));
    });
    return rows;
}

bool matchesFilter(const StyleRow& row, StyleFilter filter)
{
    switch (filter) {
    case StyleFilter::All:
        return true;
    case StyleFilter::Used:
        return row.entities > 0;
    case StyleFilter::Unused:
        return row.entities == 0;
    case StyleFilter::Missing:
        return row.missing();
    }
    return true;
}

bool matchesSearch(const StyleRow& row, std::string_view text)
{
    if (text.empty()) {
        return true;
    }
    const std::string needle = katana::core::lowered(text);
    const Style& style = row.style;
    return containsFolded(style.name, needle) || containsFolded(style.linetype, needle) ||
           containsFolded(style.symbol, needle) || containsFolded(style.hatchPattern, needle) ||
           containsFolded(style.description, needle);
}

// ---- the form ------------------------------------------------------------------------

bool StyleFields::empty() const
{
    return !linetype && !lineWeight && !color && !hatchPattern && !symbol && !symbolSize &&
           !description;
}

StyleFields commonFields(const std::vector<Style>& styles)
{
    if (styles.empty()) {
        return {};
    }
    StyleFields common = fieldsOf(styles.front());
    for (const Style& style : styles) {
        keepIfEqual(common.linetype, style.linetype);
        keepIfEqual(common.lineWeight, style.lineWeight);
        keepIfEqual(common.color, style.color);
        keepIfEqual(common.hatchPattern, style.hatchPattern);
        keepIfEqual(common.symbol, style.symbol);
        keepIfEqual(common.symbolSize, style.symbolSize);
        keepIfEqual(common.description, style.description);
    }
    return common;
}

Style applyEdit(Style style, const StyleFields& edit)
{
    if (edit.linetype) {
        style.linetype = *edit.linetype;
    }
    if (edit.lineWeight) {
        style.lineWeight = *edit.lineWeight;
    }
    if (edit.color) {
        style.color = *edit.color;
    }
    if (edit.hatchPattern) {
        style.hatchPattern = *edit.hatchPattern;
    }
    if (edit.symbol) {
        style.symbol = *edit.symbol;
    }
    if (edit.symbolSize) {
        style.symbolSize = *edit.symbolSize;
    }
    if (edit.description) {
        style.description = *edit.description;
    }
    return style;
}

katana::core::Result<katana::commands::CommandPtr>
editStylesCommand(const katana::entity::Model& model, const std::vector<std::string>& names,
                  const StyleFields& edit)
{
    // Checked for every style before any command is built: a bulk edit that
    // one style refuses is refused whole, and says which style.
    std::vector<Style> edited;
    edited.reserve(names.size());
    for (const std::string& name : names) {
        const Style* stored = model.styles.find(name);
        if (stored == nullptr) {
            return katana::core::makeError(katana::core::ErrorCode::NotFound,
                                           "there is no style of that name", name);
        }
        Style style = applyEdit(*stored, edit);
        if (const auto valid = katana::entity::validate(style); !valid) {
            katana::core::Error error = valid.error();
            error.context = name + (error.context.empty() ? "" : ": " + error.context);
            return error;
        }
        edited.push_back(std::move(style));
    }

    std::vector<katana::commands::CommandPtr> parts;
    for (Style& style : edited) {
        if (auto part = katana::commands::updateStyleIfChanged(model, std::move(style))) {
            parts.push_back(std::move(part));
        }
    }
    if (parts.empty()) {
        return katana::commands::CommandPtr{};
    }
    if (parts.size() == 1) {
        return std::move(parts.front());
    }
    auto transaction = std::make_unique<katana::commands::Transaction>("UpdateStyles");
    for (auto& part : parts) {
        transaction->add(std::move(part));
    }
    return katana::commands::CommandPtr(std::move(transaction));
}

// ---- linetypes -------------------------------------------------------------------------

std::string_view toString(LinetypeOrigin origin)
{
    switch (origin) {
    case LinetypeOrigin::Drawing:
        return "drawing";
    case LinetypeOrigin::Library:
        return "library";
    }
    return "drawing";
}

std::string_view toString(LinetypeRowKind kind)
{
    switch (kind) {
    case LinetypeRowKind::Dash:
        return "dash";
    case LinetypeRowKind::Paper:
        return "paper";
    case LinetypeRowKind::World:
        return "world";
    case LinetypeRowKind::TwoPoint:
        return "two-point";
    }
    return "dash";
}

std::vector<LinetypeRow> linetypeRows(const Document& document)
{
    const katana::entity::Model& model = document.model();
    const TableUsage usage = katana::entity::tableUsage(model);
    const katana::entity::StyleLibrary& library = document.styleLibrary();

    std::vector<LinetypeRow> rows;
    rows.reserve(model.linetypes.size() + library.size());
    model.linetypes.forEach([&](const katana::entity::Linetype& linetype) {
        LinetypeRow row;
        row.name = linetype.name;
        row.origin = LinetypeOrigin::Drawing;
        row.kind = LinetypeRowKind::Dash;
        row.description = linetype.description;
        row.period = linetype.patternLength();
        row.users = TableUsage::of(usage.linetypes, linetype.name);
        const LineStyle* definition = library.find(linetype.name);
        row.collision = definition != nullptr && !definition->atVertices;
        rows.push_back(std::move(row));
    });
    library.forEach([&](const LineStyle& definition) {
        // D2: a `mode vertex` definition is a symbol, never a linestyle - a
        // line naming it is drawn solid - so it is not offered here.
        if (definition.atVertices) {
            return;
        }
        LinetypeRow row;
        row.name = definition.name;
        row.origin = LinetypeOrigin::Library;
        row.kind = kindOf(definition.units);
        row.group = definition.group;
        row.sourceFile = definition.source;
        row.period = libraryPeriod(definition);
        row.users = TableUsage::of(usage.linetypes, definition.name);
        row.collision = model.linetypes.contains(definition.name);
        rows.push_back(std::move(row));
    });
    // Case folded, as a person reads a list; the exact name, then the drawing
    // row first, break ties so the order never depends on the input's.
    std::ranges::stable_sort(rows, [](const LinetypeRow& a, const LinetypeRow& b) {
        const std::string foldedA = foldedKey(a.name);
        const std::string foldedB = foldedKey(b.name);
        if (foldedA != foldedB) {
            return foldedA < foldedB;
        }
        if (a.name != b.name) {
            return a.name < b.name;
        }
        return a.origin == LinetypeOrigin::Drawing && b.origin == LinetypeOrigin::Library;
    });
    return rows;
}

// ---- diagnostics ----------------------------------------------------------------------

std::string_view toString(StyleDiagnosticKind kind)
{
    switch (kind) {
    case StyleDiagnosticKind::MissingLinetype:
        return "missing linetype";
    case StyleDiagnosticKind::MissingSymbol:
        return "missing symbol";
    case StyleDiagnosticKind::Collision:
        return "collision";
    }
    return "missing linetype";
}

std::vector<StyleDiagnostic> styleDiagnostics(const Document& document)
{
    std::vector<StyleDiagnostic> diagnostics;
    for (MissingName& missing : missingNames(document)) {
        StyleDiagnostic diagnostic;
        diagnostic.name = std::move(missing.name);
        diagnostic.users = std::move(missing.users);
        if (missing.role == NameRole::Linetype) {
            diagnostic.kind = StyleDiagnosticKind::MissingLinetype;
            diagnostic.drawnAs = "a solid line (" + missing.fallback + ")";
        } else {
            diagnostic.kind = StyleDiagnosticKind::MissingSymbol;
            diagnostic.drawnAs = "the built-in " + missing.fallback;
        }
        diagnostics.push_back(std::move(diagnostic));
    }
    const std::vector<std::string> collisions = linetypeCollisions(document);
    if (!collisions.empty()) {
        const TableUsage usage = katana::entity::tableUsage(document.model());
        for (const std::string& name : collisions) {
            diagnostics.push_back(StyleDiagnostic{
                StyleDiagnosticKind::Collision, name, TableUsage::of(usage.linetypes, name),
                "the library linestyle; the drawing's dash pattern is not drawn"});
        }
    }
    return diagnostics;
}

// ---- select users -----------------------------------------------------------------------

std::vector<katana::entity::EntityId> entitiesUsing(const katana::entity::Model& model,
                                                    UsageTable table,
                                                    const std::vector<std::string>& names)
{
    const TableUsage usage =
        katana::entity::tableUsage(model, katana::entity::UsageOptions{.entityIds = true});
    const katana::entity::UsageMap& map = table == UsageTable::Style      ? usage.styles
                                          : table == UsageTable::Linetype ? usage.linetypes
                                                                          : usage.symbols;
    std::vector<katana::entity::EntityId> ids;
    for (const std::string& name : names) {
        const auto& users = TableUsage::of(map, name);
        ids.insert(ids.end(), users.entityIds.begin(), users.entityIds.end());
    }
    std::ranges::sort(ids);
    ids.erase(std::unique(ids.begin(), ids.end()), ids.end());
    return ids;
}


std::string freeLinetypeName(const Document& document, std::string_view base)
{
    const auto taken = [&document](const std::string& name) {
        return document.model().linetypes.contains(name) ||
               document.styleLibrary().contains(name);
    };
    std::string name(base);
    for (int suffix = 2; taken(name); ++suffix) {
        name = std::string(base) + " " + std::to_string(suffix);
    }
    return name;
}

} // namespace katana::cad
