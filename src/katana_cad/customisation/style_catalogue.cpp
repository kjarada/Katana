#include "katana/cad/style_catalogue.hpp"

#include <algorithm>
#include <set>
#include <utility>

#include "katana/core/text.hpp"
#include "katana/entity/display.hpp"

namespace katana::cad {

namespace {

using katana::entity::LineStyle;
using katana::entity::TableUsage;
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
    // The file NAME, folded: 12d's own symbol files are user_symbols_*.4d,
    // and a customisation's author follows the convention in any case.
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
// nothing, and "continuous", "0" and "1" are the plain line in 12d and DXF
// alike (D4). None of them is a name that is missing.
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

std::vector<MissingName> missingNames(const Document& document)
{
    const katana::entity::Model& model = document.model();
    const TableUsage usage = katana::entity::tableUsage(model);
    std::vector<MissingName> missing;

    for (const auto& [name, users] : usage.linetypes) {
        // Only what a layer or style NAMES: a name reached by entities alone
        // is a default the chain fell back to, not something anyone chose.
        if (!users.named() || isPlainLinetype(name) || model.linetypes.contains(name) ||
            document.definitionFor(name) != nullptr) {
            continue;
        }
        missing.push_back(MissingName{name, NameRole::Linetype, users,
                                      std::string(katana::entity::kContinuousLinetype)});
    }
    for (const auto& [name, users] : usage.symbols) {
        // A built-in name is drawn as itself, so it is not missing (the
        // symptom of audit CAD-17 in customisationCoverage).
        if (!users.named() || katana::entity::isBuiltInSymbolName(name) ||
            document.definitionFor(name) != nullptr) {
            continue;
        }
        missing.push_back(MissingName{name, NameRole::Symbol, users,
                                      std::string(katana::entity::builtInSymbolFor(name))});
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
