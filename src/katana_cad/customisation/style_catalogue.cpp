#include "katana/cad/style_catalogue.hpp"

#include <algorithm>
#include <set>
#include <utility>

#include "katana/cad/style_resolver.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/display.hpp"

namespace katana::cad {

namespace {

using katana::entity::LineStyle;
using katana::entity::TableUsage;
using katana::entity::Users;
using NameSet = std::set<std::string, std::less<>>;

// What D3 needs beyond the definition itself, gathered once for a whole
// list rather than once per definition - a list is eight hundred of them.
struct SymbolEvidence {
    NameSet styleSymbols;
    NameSet ruleSymbols;

    explicit SymbolEvidence(const Document& document)
    {
        document.model().styles.forEach([&](const katana::entity::Style& style) {
            if (!style.symbol.empty()) {
                styleSymbols.insert(style.symbol);
            }
        });
        for (const katana::entity::SurveyRule& rule : document.surveyMap().rules()) {
            if (rule.section == katana::entity::SurveySection::VertexSymbol && rule.symbol &&
                !rule.symbol->style.empty()) {
                ruleSymbols.insert(rule.symbol->style);
            }
        }
    }
};

DefinitionKind classify(const LineStyle& definition, const SymbolEvidence& evidence)
{
    DefinitionKind kind;
    kind.atVertices = definition.atVertices;
    kind.linestyle = !definition.atVertices;
    kind.namedBySurveyRule = evidence.ruleSymbols.contains(definition.name);
    kind.namedByStyle = evidence.styleSymbols.contains(definition.name);
    // The file NAME, folded: symbol libraries are conventionally named
    // *symbols*.4d, and a customisation's author follows the convention in
    // any case.
    kind.fromSymbolFile =
        katana::core::lowered(definition.source).find("symbol") != std::string::npos;
    kind.symbol =
        kind.atVertices || kind.namedBySurveyRule || kind.namedByStyle || kind.fromSymbolFile;
    return kind;
}

CatalogueEntry fromDefinition(const LineStyle& definition, const DefinitionKind& kind)
{
    CatalogueEntry entry;
    entry.name = definition.name;
    entry.source = DefinitionSource::Library;
    entry.sourceFile = definition.source;
    entry.group = definition.group;
    entry.units = definition.units;
    entry.kind = kind;
    entry.atVertices = definition.atVertices;
    return entry;
}

// ByLayer first, as every CAD package lists it; then by name with case
// folded, so "kerb" sits beside "KERB" rather than after "Z", and the exact
// name breaks the tie so the order never depends on insertion.
void sortForPicker(std::vector<CatalogueEntry>& entries)
{
    std::vector<std::pair<std::string, CatalogueEntry>> keyed;
    keyed.reserve(entries.size());
    for (CatalogueEntry& entry : entries) {
        std::string key = katana::core::lowered(entry.name);
        keyed.emplace_back(std::move(key), std::move(entry));
    }
    const auto rank = [](const CatalogueEntry& entry) {
        return entry.source == DefinitionSource::BuiltIn &&
                       entry.name == katana::entity::kByLayerLinetype
                   ? 0
                   : 1;
    };
    std::ranges::sort(keyed, [&](const auto& a, const auto& b) {
        const int rankA = rank(a.second);
        const int rankB = rank(b.second);
        if (rankA != rankB) {
            return rankA < rankB;
        }
        if (a.first != b.first) {
            return a.first < b.first;
        }
        return a.second.name < b.second.name;
    });
    entries.clear();
    for (auto& [key, entry] : keyed) {
        entries.push_back(std::move(entry));
    }
}

// A line with no pattern, whatever defines it or not: ByLayer and "" say
// nothing, and "continuous", "0" and "1" are the plain continuous line in a
// style library and DXF alike (D4). None of them is a name that is missing.
bool isPlainLinetype(std::string_view name)
{
    return name.empty() || katana::entity::isByLayer(name) ||
           katana::core::equalsIgnoringCase(name, katana::entity::kContinuousLinetype) ||
           name == "0" || name == "1";
}

bool containsFolded(std::string_view text, const std::string& foldedNeedle)
{
    return katana::core::lowered(text).find(foldedNeedle) != std::string::npos;
}

// The users of linetype `name` less the styles that name it as their own
// symbol's name: such a style draws a plain line under its symbol (D8), so
// it does not use the name as a linetype, and neither do the entities
// wearing it. Only when some other layer or style still names it are the
// entities counted again - one pass, and only in that rare mixed case - so
// that `entities` and `firstEntity` stay those of the lines drawn solid.
Users withoutOwnSymbolStyles(const Document& document, std::string_view name, const Users& users)
{
    const katana::entity::Model& model = document.model();
    NameSet ownSymbol;
    for (const std::string& styleName : users.styles) {
        const katana::entity::Style* style = model.styles.find(styleName);
        if (style != nullptr &&
            linetypeStatus(document, name, style->symbol) == NameStatus::OwnSymbol) {
            ownSymbol.insert(styleName);
        }
    }
    if (ownSymbol.empty()) {
        return users;
    }
    Users kept;
    kept.layers = users.layers;
    for (const std::string& styleName : users.styles) {
        if (!ownSymbol.contains(styleName)) {
            kept.styles.push_back(styleName);
        }
    }
    if (!kept.named()) {
        return kept; // nothing else names it: not missing at all
    }
    // tableUsage's own resolution (entity::resolvedLinetype), less the
    // styles set aside above.
    model.entities.forEach([&](const katana::entity::Entity& entity) {
        const katana::entity::Style* style =
            entity.style.empty() ? nullptr : model.styles.find(entity.style);
        if (style != nullptr && ownSymbol.contains(style->name)) {
            return;
        }
        if (katana::entity::resolvedLinetype(model.layers.find(entity.layer), style) != name) {
            return;
        }
        if (kept.entities == 0 || entity.id < kept.firstEntity) {
            kept.firstEntity = entity.id;
        }
        ++kept.entities;
    });
    return kept;
}

} // namespace

std::string_view toString(DefinitionSource source)
{
    switch (source) {
    case DefinitionSource::ModelLinetype:
        return "model linetype";
    case DefinitionSource::Library:
        return "library";
    case DefinitionSource::BuiltIn:
        return "built-in";
    case DefinitionSource::Undefined:
        return "defined nowhere";
    }
    return "unknown";
}

std::string_view toString(NameRole role)
{
    return role == NameRole::Linetype ? "linetype" : "symbol";
}

DefinitionKind classifyDefinition(const Document& document, std::string_view name)
{
    const LineStyle* definition = document.definitionFor(name);
    if (definition == nullptr) {
        return {};
    }
    return classify(*definition, SymbolEvidence(document));
}

std::vector<CatalogueEntry> linetypeChoices(const Document& document, bool forStyle)
{
    const katana::entity::Model& model = document.model();
    const TableUsage usage = katana::entity::tableUsage(model);
    const SymbolEvidence evidence(document);
    std::vector<CatalogueEntry> entries;

    if (forStyle) {
        CatalogueEntry byLayer;
        byLayer.name = std::string(katana::entity::kByLayerLinetype);
        byLayer.source = DefinitionSource::BuiltIn;
        byLayer.kind.linestyle = true;
        // Only the styles: an entity reaching a linetype through ByLayer is
        // counted under the layer's linetype, which is the one it draws.
        model.styles.forEach([&](const katana::entity::Style& style) {
            if (katana::entity::isByLayer(style.linetype)) {
                byLayer.users.styles.push_back(style.name);
            }
        });
        entries.push_back(std::move(byLayer));
    }
    document.styleLibrary().forEach([&](const LineStyle& definition) {
        const DefinitionKind kind = classify(definition, evidence);
        if (!kind.linestyle) {
            return;
        }
        CatalogueEntry entry = fromDefinition(definition, kind);
        entry.collision = model.linetypes.contains(definition.name);
        entry.users = TableUsage::of(usage.linetypes, definition.name);
        entries.push_back(std::move(entry));
    });
    model.linetypes.forEach([&](const katana::entity::Linetype& linetype) {
        // A collision is listed once, as the library's: that is what draws.
        if (const LineStyle* definition = document.definitionFor(linetype.name);
            definition != nullptr && !definition->atVertices) {
            return;
        }
        CatalogueEntry entry;
        entry.name = linetype.name;
        entry.source = DefinitionSource::ModelLinetype;
        entry.kind.linestyle = true;
        entry.users = TableUsage::of(usage.linetypes, linetype.name);
        entries.push_back(std::move(entry));
    });
    sortForPicker(entries);
    return entries;
}

std::vector<CatalogueEntry> symbolChoices(const Document& document)
{
    const TableUsage usage = katana::entity::tableUsage(document.model());
    const SymbolEvidence evidence(document);
    std::vector<CatalogueEntry> entries;

    document.styleLibrary().forEach([&](const LineStyle& definition) {
        const DefinitionKind kind = classify(definition, evidence);
        if (!kind.symbol) {
            return;
        }
        CatalogueEntry entry = fromDefinition(definition, kind);
        entry.users = TableUsage::of(usage.symbols, definition.name);
        entries.push_back(std::move(entry));
    });
    for (const std::string_view name : katana::entity::symbolNames()) {
        // Drawn from the library when it defines the name, so listed as it.
        if (document.definitionFor(name) != nullptr) {
            continue;
        }
        CatalogueEntry entry;
        entry.name = std::string(name);
        entry.source = DefinitionSource::BuiltIn;
        entry.kind.symbol = true;
        entry.users = TableUsage::of(usage.symbols, name);
        entries.push_back(std::move(entry));
    }
    sortForPicker(entries);
    return entries;
}

std::vector<CatalogueEntry> keepCurrent(std::vector<CatalogueEntry> choices,
                                        std::string_view current)
{
    if (current.empty() ||
        std::ranges::any_of(choices, [&](const CatalogueEntry& entry) { return entry.name == current; })) {
        return choices;
    }
    // First, where it is seen: the value being edited, which the editor is
    // about to write back, must be the one it shows.
    CatalogueEntry kept;
    kept.name = std::string(current);
    kept.source = DefinitionSource::Undefined;
    kept.missing = true;
    choices.insert(choices.begin(), std::move(kept));
    return choices;
}

std::vector<CatalogueEntry> filterChoices(const std::vector<CatalogueEntry>& choices,
                                          std::string_view text)
{
    if (text.empty()) {
        return choices;
    }
    const std::string needle = katana::core::lowered(text);
    std::vector<CatalogueEntry> kept;
    for (const CatalogueEntry& entry : choices) {
        if (containsFolded(entry.name, needle) || containsFolded(entry.group, needle) ||
            containsFolded(entry.sourceFile, needle)) {
            kept.push_back(entry);
        }
    }
    return kept;
}

std::string_view toString(NameStatus status)
{
    switch (status) {
    case NameStatus::Plain:
        return "plain line";
    case NameStatus::OwnSymbol:
        return "own symbol";
    case NameStatus::Library:
        return "library";
    case NameStatus::Katana:
        return "built in";
    case NameStatus::NotALinestyle:
        return "not a linestyle";
    case NameStatus::Undefined:
        return "defined nowhere";
    }
    return "defined nowhere";
}

NameStatus linetypeStatus(const Document& document, std::string_view linetype,
                          std::string_view ownSymbol)
{
    if (isPlainLinetype(linetype)) {
        return NameStatus::Plain;
    }
    const ResolvedLinetype resolved = resolveLinePattern(
        document.model(), document.styleLibrary(), linetype, ownSymbol);
    switch (resolved.kind) {
    case LinetypeKind::LibraryDefinition:
        return NameStatus::Library;
    case LinetypeKind::ModelLinetype:
        return NameStatus::Katana;
    case LinetypeKind::Solid:
        break;
    }
    // Solid: why, in the order resolveLinePattern decides it.
    if (!ownSymbol.empty() && linetype == ownSymbol) {
        return NameStatus::OwnSymbol;
    }
    return document.definitionFor(linetype) != nullptr ? NameStatus::NotALinestyle
                                                        : NameStatus::Undefined;
}

NameStatus symbolStatus(const Document& document, std::string_view symbol)
{
    switch (resolveSymbol(document.styleLibrary(), symbol).kind) {
    case SymbolKind::None:
        return NameStatus::Plain;
    case SymbolKind::LibraryDefinition:
        return NameStatus::Library;
    case SymbolKind::BuiltIn:
        return NameStatus::Katana;
    case SymbolKind::BuiltInFallback:
        break;
    }
    return NameStatus::Undefined;
}

std::vector<MissingName> missingNames(const Document& document)
{
    const katana::entity::Model& model = document.model();
    const TableUsage usage = katana::entity::tableUsage(model);
    std::vector<MissingName> missing;

    for (const auto& [name, users] : usage.linetypes) {
        // Only what a layer or style NAMES: a name reached by entities alone
        // is a default the chain fell back to, not something anyone chose.
        if (!users.named()) {
            continue;
        }
        // As a layer names it: a layer has no symbol of its own.
        const NameStatus status = linetypeStatus(document, name);
        if (!isMissing(status)) {
            continue;
        }
        Users naming = withoutOwnSymbolStyles(document, name, users);
        if (!naming.named()) {
            continue;
        }
        missing.push_back(MissingName{name, NameRole::Linetype, std::move(naming),
                                      std::string(katana::entity::kContinuousLinetype), status});
    }
    for (const auto& [name, users] : usage.symbols) {
        const NameStatus status = symbolStatus(document, name);
        if (!users.named() || !isMissing(status)) {
            continue;
        }
        missing.push_back(MissingName{name, NameRole::Symbol, users,
                                      std::string(katana::entity::builtInSymbolFor(name)),
                                      status});
    }
    return missing;
}

std::vector<std::string> linetypeCollisions(const Document& document)
{
    std::vector<std::string> names;
    document.model().linetypes.forEach([&](const katana::entity::Linetype& linetype) {
        if (const LineStyle* definition = document.definitionFor(linetype.name);
            definition != nullptr && !definition->atVertices) {
            names.push_back(linetype.name);
        }
    });
    return names;
}

std::string freeStyleName(const katana::entity::Model& model, std::string_view base)
{
    std::string name(base);
    for (int suffix = 2; model.styles.contains(name); ++suffix) {
        name = std::string(base) + " " + std::to_string(suffix);
    }
    return name;
}

} // namespace katana::cad
