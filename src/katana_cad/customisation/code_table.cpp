#include "katana/cad/code_table.hpp"

#include <algorithm>
#include <map>
#include <set>
#include <sstream>

#include "katana/core/text.hpp"
#include "katana/entity/layer_path.hpp"
#include "katana/entity/tables.hpp"

namespace katana::cad {

namespace {

using katana::entity::LineStyle;
using katana::entity::SurveyAttribute;
using katana::entity::SurveyMap;
using katana::entity::SurveyMatchKind;
using katana::entity::SurveyRule;
using katana::entity::SurveySection;

// A key's own shape, as SurveyMap reads it: a trailing `*` makes a prefix,
// and the bare `*` alone is the fallback every code meets.
[[nodiscard]] SurveyMatchKind kindOfKey(std::string_view key)
{
    if (key.empty() || key.back() != '*') {
        return SurveyMatchKind::Exact;
    }
    return key.size() == 1 ? SurveyMatchKind::FallbackOnly : SurveyMatchKind::Prefix;
}

// Whether `key` catches `code`, byte for byte - the same test SurveyMap
// makes, repeated here only to ask it of a code the map was not given (a
// folded or trimmed one).
[[nodiscard]] bool keyCatches(std::string_view key, std::string_view code)
{
    if (kindOfKey(key) == SurveyMatchKind::Exact) {
        return key == code;
    }
    return code.starts_with(key.substr(0, key.size() - 1));
}

[[nodiscard]] std::string real(double value)
{
    return katana::core::formatExactReal(value);
}

[[nodiscard]] std::string yesNo(bool value)
{
    return value ? "yes" : "no";
}

[[nodiscard]] std::string symbolText(const katana::entity::SurveySymbol& symbol)
{
    std::string text = symbol.style.empty() ? std::string("(no name)") : symbol.style;
    if (symbol.size != 0.0) {
        text += ", size " + real(symbol.size);
    }
    if (!symbol.colour.empty()) {
        text += ", colour " + symbol.colour;
    }
    if (symbol.rotation != 0.0) {
        text += ", rotation " + real(symbol.rotation);
    }
    if (symbol.offset != 0.0) {
        text += ", offset " + real(symbol.offset);
    }
    if (symbol.raise != 0.0) {
        text += ", raise " + real(symbol.raise);
    }
    return text;
}

[[nodiscard]] std::string textStyleText(const katana::entity::SurveyTextStyle& style)
{
    std::string text = style.textstyle.empty() ? std::string("(default)") : style.textstyle;
    if (style.size != 0.0) {
        text += ", size " + real(style.size);
        if (!style.type.empty()) {
            text += " " + style.type;
        }
    }
    if (!style.colour.empty()) {
        text += ", colour " + style.colour;
    }
    return text;
}

[[nodiscard]] std::string pipeText(const katana::entity::SurveyPipe& pipe)
{
    std::string text = pipe.shape.empty() ? std::string("(no shape)") : pipe.shape;
    if (!pipe.size1.empty()) {
        text += " " + pipe.size1;
        if (!pipe.size2.empty()) {
            text += " x " + pipe.size2;
        }
    }
    if (!pipe.justify.empty()) {
        text += ", " + pipe.justify;
    }
    if (!pipe.active) {
        text += ", inactive";
    }
    return text;
}

// The fields a rule can set, in the order CodeExplanation::fields lists them,
// each with the test SurveyMap::lookup makes - an empty string or an absent
// optional says nothing - and the value as a person reads it.
struct FieldSpec {
    const char* name;
    bool (*sets)(const SurveyRule&);
    std::string (*text)(const SurveyRule&);
};

const std::vector<FieldSpec>& fieldSpecs()
{
    static const std::vector<FieldSpec> specs = {
        {"layer", [](const SurveyRule& r) { return !r.model.empty(); },
         [](const SurveyRule& r) { return r.model; }},
        {"colour", [](const SurveyRule& r) { return !r.colour.empty(); },
         [](const SurveyRule& r) { return r.colour; }},
        {"linestyle", [](const SurveyRule& r) { return !r.linestyle.empty(); },
         [](const SurveyRule& r) { return r.linestyle; }},
        {"weight", [](const SurveyRule& r) { return !r.weight.empty(); },
         [](const SurveyRule& r) { return r.weight; }},
        {"group", [](const SurveyRule& r) { return !r.group.empty(); },
         [](const SurveyRule& r) { return r.group; }},
        {"comment", [](const SurveyRule& r) { return !r.comment.empty(); },
         [](const SurveyRule& r) { return r.comment; }},
        {"breakline", [](const SurveyRule& r) { return r.breakline.has_value(); },
         [](const SurveyRule& r) { return std::string(katana::entity::toString(*r.breakline)); }},
        {"surface", [](const SurveyRule& r) { return r.tinable.has_value(); },
         [](const SurveyRule& r) { return yesNo(*r.tinable); }},
        {"hide", [](const SurveyRule& r) { return r.hide.has_value(); },
         [](const SurveyRule& r) { return yesNo(*r.hide); }},
        {"symbol", [](const SurveyRule& r) { return r.symbol.has_value(); },
         [](const SurveyRule& r) { return symbolText(*r.symbol); }},
        {"text style", [](const SurveyRule& r) { return r.textStyle.has_value(); },
         [](const SurveyRule& r) { return textStyleText(*r.textStyle); }},
        {"pipe", [](const SurveyRule& r) { return r.pipe.has_value(); },
         [](const SurveyRule& r) { return pipeText(*r.pipe); }},
        {"vertex pipe", [](const SurveyRule& r) { return r.vertexPipe.has_value(); },
         [](const SurveyRule& r) { return pipeText(*r.vertexPipe); }},
        {"segment pipe", [](const SurveyRule& r) { return r.segmentPipe.has_value(); },
         [](const SurveyRule& r) { return pipeText(*r.segmentPipe); }},
    };
    return specs;
}

// The three attribute lists, by the scope word CodeAttribute uses.
struct AttributeScope {
    const char* name;
    const std::vector<SurveyAttribute>& (*of)(const SurveyRule&);
};

const std::vector<AttributeScope>& attributeScopes()
{
    static const std::vector<AttributeScope> scopes = {
        {"string", [](const SurveyRule& r) -> const std::vector<SurveyAttribute>& {
             return r.attributes;
         }},
        {"vertex", [](const SurveyRule& r) -> const std::vector<SurveyAttribute>& {
             return r.vertexAttributes;
         }},
        {"segment", [](const SurveyRule& r) -> const std::vector<SurveyAttribute>& {
             return r.segmentAttributes;
         }},
    };
    return scopes;
}

[[nodiscard]] bool namesAttribute(const std::vector<SurveyAttribute>& list, std::string_view name)
{
    return std::any_of(list.begin(), list.end(),
                       [name](const SurveyAttribute& a) { return a.name == name; });
}

[[nodiscard]] bool isDeferred(const SurveyAttribute& attribute)
{
    return !attribute.value.empty() && attribute.value.front() == '$';
}

// Whether a definition may be drawn at a vertex (decision D3). A definition of
// unknown origin is given the benefit of the doubt: most symbols the
// reference mapfiles name are not `mode vertex`, and without a file name
// nothing else says what they are.
[[nodiscard]] bool symbolCapable(const LineStyle& definition)
{
    return definition.atVertices || definition.source.empty() ||
           katana::core::lowered(definition.source).find("symbol") != std::string::npos;
}

[[nodiscard]] std::string inQuotes(std::string_view text)
{
    return "\"" + std::string(text) + "\"";
}

[[nodiscard]] std::string joined(const std::vector<std::string>& items, std::string_view with = ", ")
{
    std::string text;
    for (std::size_t i = 0; i < items.size(); ++i) {
        if (i != 0) {
            text += with;
        }
        text += items[i];
    }
    return text;
}

[[nodiscard]] std::string ruleRef(std::size_t index, std::string_view key, SurveySection section)
{
    return "rule #" + std::to_string(index) + " " + std::string(key) + " (" +
           katana::entity::toString(section) + ")";
}

[[nodiscard]] std::string counted(std::size_t n, std::string_view one, std::string_view many)
{
    return std::to_string(n) + " " + std::string(n == 1 ? one : many);
}

} // namespace

const char* toString(NearMissKind kind)
{
    switch (kind) {
    case NearMissKind::Case:
        return "differs only in letter case";
    case NearMissKind::Whitespace:
        return "differs by blanks around the code";
    }
    return "differs";
}

// ---- explaining one code ------------------------------------------------------

CodeExplanation explainCode(const SurveyMap& map, std::string_view code,
                            const DefinitionLookup& definitionOf, const ColourLookup& colourOf)
{
    CodeExplanation out;
    out.code = std::string(code);
    const katana::entity::SurveyMatch match = map.lookup(code);
    out.kind = match.kind;
    out.matched = match.matched();
    out.resolved = match.resolved;
    out.rules = map.matchIndices(code);
    const std::vector<SurveyRule>& rules = map.rules();

    // The first rule, most specific first, that sets a field is the one
    // lookup took it from; any later one saying otherwise is overruled.
    for (const FieldSpec& spec : fieldSpecs()) {
        CodeFieldSource* source = nullptr;
        for (const std::size_t index : out.rules) {
            const SurveyRule& rule = rules[index];
            if (!spec.sets(rule)) {
                continue;
            }
            std::string value = spec.text(rule);
            if (source == nullptr) {
                source = &out.fields.emplace_back(CodeFieldSource{
                    spec.name, std::move(value), index, rule.key, rule.section, {}});
            } else if (value != source->value) {
                source->overruled.push_back({index, rule.key, rule.section, std::move(value)});
            }
        }
    }

    const auto definition = [&definitionOf](const std::string& name) -> const LineStyle* {
        return definitionOf && !name.empty() ? definitionOf(name) : nullptr;
    };
    out.linestyle.name = match.resolved.linestyle;
    out.linestyle.plain = !out.linestyle.name.empty() && isPlainLinestyle(out.linestyle.name);
    if (!out.linestyle.plain) {
        if (const LineStyle* found = definition(out.linestyle.name)) {
            out.linestyle.defined = true;
            out.linestyle.vertexMode = found->atVertices;
        }
    }
    if (match.resolved.symbol) {
        out.symbol.name = match.resolved.symbol->style;
    }
    if (const LineStyle* found = definition(out.symbol.name)) {
        out.symbol.defined = true;
        out.symbol.vertexMode = found->atVertices;
    } else {
        out.symbol.builtIn =
            !out.symbol.name.empty() && katana::entity::isBuiltInSymbolName(out.symbol.name);
    }

    out.colour.name = match.resolved.colour;
    if (colourOf && !out.colour.name.empty()) {
        out.colour.rgb = colourOf(out.colour.name);
    }

    // An attribute comes from the first matching rule that names it in the
    // same list - lookup keeps the first of each name.
    for (const AttributeScope& scope : attributeScopes()) {
        for (const SurveyAttribute& attribute : scope.of(match.resolved)) {
            CodeAttribute entry;
            entry.scope = scope.name;
            entry.attribute = attribute;
            entry.deferred = isDeferred(attribute);
            for (const std::size_t index : out.rules) {
                if (namesAttribute(scope.of(rules[index]), attribute.name)) {
                    entry.rule = index;
                    entry.key = rules[index].key;
                    entry.section = rules[index].section;
                    break;
                }
            }
            out.attributes.push_back(std::move(entry));
        }
    }

    // Keys this code misses only by case or by blanks. The bare `*` is never
    // one: it catches every code already.
    const std::string_view trimmedCode = katana::core::trimmed(code);
    const bool padded = trimmedCode.size() != code.size();
    const std::string foldedCode = katana::core::lowered(code);
    const std::string foldedTrimmed = katana::core::lowered(trimmedCode);
    for (const std::string& key : map.keys()) {
        if (key == "*" || keyCatches(key, code)) {
            continue;
        }
        const std::string foldedKey = katana::core::lowered(key);
        if (padded && (keyCatches(key, trimmedCode) || keyCatches(foldedKey, foldedTrimmed))) {
            out.nearMisses.push_back({key, NearMissKind::Whitespace});
        } else if (keyCatches(foldedKey, foldedCode)) {
            out.nearMisses.push_back({key, NearMissKind::Case});
        }
    }
    return out;
}

// ---- the map as a table of codes ------------------------------------------------

std::vector<CodeTableRow> codeTable(const SurveyMap& map)
{
    std::map<std::string, std::vector<std::size_t>> byKey;
    const std::vector<SurveyRule>& rules = map.rules();
    for (std::size_t i = 0; i < rules.size(); ++i) {
        byKey[rules[i].key].push_back(i);
    }
    std::vector<CodeTableRow> rows;
    rows.reserve(byKey.size());
    for (auto& [key, indices] : byKey) {
        CodeTableRow row;
        row.key = key;
        row.kind = kindOfKey(key);
        std::set<SurveySection> sections;
        for (const std::size_t index : indices) {
            sections.insert(rules[index].section);
        }
        row.sections.assign(sections.begin(), sections.end());
        row.rules = std::move(indices);
        // A key is itself a code its own rules catch: "WM*" read as a code
        // starts with "WM", so lookup gives WM*, then the shorter prefixes,
        // then `*` - and no exact key, since none ends in `*`.
        row.combined = map.lookup(key).resolved;
        rows.push_back(std::move(row));
    }
    return rows;
}

bool codeTableRowMatches(const CodeTableRow& row, std::string_view filter)
{
    if (filter.empty()) {
        return true;
    }
    const std::string needle = katana::core::lowered(filter);
    const SurveyRule& combined = row.combined;
    for (const std::string* field : {&row.key, &combined.comment, &combined.group, &combined.model,
                                     &combined.colour, &combined.linestyle}) {
        if (katana::core::lowered(*field).find(needle) != std::string::npos) {
            return true;
        }
    }
    return combined.symbol.has_value() &&
           katana::core::lowered(combined.symbol->style).find(needle) != std::string::npos;
}

// ---- the codes a drawing carries ------------------------------------------------

CodeCensus codeCensus(const Document& document, std::string_view property)
{
    const katana::entity::Model& model = document.model();
    const std::vector<katana::entity::EntityId> ids = model.entities.ids();
    CodeCensus census;
    census.property = property.empty() ? findCodeProperty(model, ids) : std::string(property);

    std::map<std::string, std::size_t> counts;
    for (const katana::entity::EntityId id : ids) {
        const katana::entity::Entity* entity = model.entities.find(id);
        if (entity == nullptr) {
            continue;
        }
        if (const std::string* code = surveyCodeOf(*entity, census.property)) {
            ++census.coded;
            ++counts[*code];
        }
    }
    const SurveyMap& map = document.surveyMap();
    census.codes.reserve(counts.size());
    for (const auto& [code, entities] : counts) {
        const katana::entity::SurveyMatch match = map.lookup(code);
        census.codes.push_back({code, entities, match.kind, match.matched(), match.resolved.model});
    }
    return census;
}

// ---- what is wrong with a map -----------------------------------------------------

const char* toString(LintSeverity severity)
{
    switch (severity) {
    case LintSeverity::Error:
        return "error";
    case LintSeverity::Warning:
        return "warning";
    }
    return "warning";
}

const char* toString(LintKind kind)
{
    switch (kind) {
    case LintKind::UnresolvedLinestyle:
        return "unresolved linestyle";
    case LintKind::UnresolvedSymbol:
        return "unresolved symbol";
    case LintKind::SymbolNotSymbolCapable:
        return "not a symbol";
    case LintKind::LinestyleIsVertex:
        return "linestyle is a symbol";
    case LintKind::UnknownColour:
        return "unknown colour";
    case LintKind::InvalidLayerPath:
        return "invalid layer path";
    case LintKind::NoModel:
        return "no layer";
    case LintKind::DuplicateRule:
        return "duplicate rule";
    case LintKind::ShadowedRule:
        return "shadowed rule";
    case LintKind::KeyWhitespace:
        return "key whitespace";
    }
    return "issue";
}

std::vector<LintIssue> lintSurveyRule(const SurveyRule& rule, std::size_t index,
                                      const katana::entity::StyleLibrary& library,
                                      const ColourLookup& colourOf,
                                      const BuiltInSymbolTest& isBuiltInSymbol)
{
    std::vector<LintIssue> issues;
    const auto report = [&](LintSeverity severity, LintKind kind, std::string message) {
        issues.push_back({index, rule.key, rule.section, severity, kind, std::move(message)});
    };

    if (!rule.linestyle.empty() && !isPlainLinestyle(rule.linestyle)) {
        const LineStyle* found = library.find(rule.linestyle);
        if (found == nullptr) {
            report(LintSeverity::Warning, LintKind::UnresolvedLinestyle,
                   "linestyle " + inQuotes(rule.linestyle) +
                       " is in no loaded library, so it draws as a plain line");
        } else if (found->atVertices) {
            // A vertex definition is a symbol: drawn at each vertex, never
            // along the line (decision D2).
            report(LintSeverity::Warning, LintKind::LinestyleIsVertex,
                   "linestyle " + inQuotes(rule.linestyle) +
                       " is a symbol (at vertices), drawn at each vertex rather than along the "
                       "line");
        }
    }
    if (rule.symbol && !rule.symbol->style.empty()) {
        const std::string& name = rule.symbol->style;
        const LineStyle* found = library.find(name);
        if (found == nullptr) {
            if (!(isBuiltInSymbol && isBuiltInSymbol(name))) {
                report(LintSeverity::Warning, LintKind::UnresolvedSymbol,
                       "symbol " + inQuotes(name) +
                           " is in no loaded library and is not one Katana draws itself");
            }
        } else if (!symbolCapable(*found)) {
            report(LintSeverity::Warning, LintKind::SymbolNotSymbolCapable,
                   "symbol " + inQuotes(name) + " is a linestyle from " + inQuotes(found->source) +
                       ", not a symbol");
        }
    }
    if (colourOf) {
        const auto checkColour = [&](const std::string& name, std::string_view what) {
            if (!name.empty() && !colourOf(name)) {
                report(LintSeverity::Warning, LintKind::UnknownColour,
                       std::string(what) + " " + inQuotes(name) +
                           " is not a colour name Katana knows, so the entity keeps its own");
            }
        };
        checkColour(rule.colour, "colour");
        if (rule.symbol) {
            checkColour(rule.symbol->colour, "symbol colour");
        }
        if (rule.textStyle) {
            checkColour(rule.textStyle->colour, "text colour");
        }
    }
    if (!rule.model.empty()) {
        if (auto status = katana::entity::validateLayerPath(rule.model); !status) {
            report(LintSeverity::Error, LintKind::InvalidLayerPath,
                   "layer " + inQuotes(rule.model) +
                       " is not a layer path: " + status.error().message);
        }
    } else if (rule.section == SurveySection::Map) {
        report(LintSeverity::Warning, LintKind::NoModel,
               "a feature rule with no layer: its codes stay on whatever layer they are on");
    }
    if (!rule.key.empty() && (katana::core::isAsciiSpace(rule.key.front()) ||
                              katana::core::isAsciiSpace(rule.key.back()))) {
        report(LintSeverity::Error, LintKind::KeyWhitespace,
               "key " + inQuotes(rule.key) +
                   " has blanks around it, so it matches no code a surveyor types");
    }
    std::stable_sort(issues.begin(), issues.end(),
                     [](const LintIssue& a, const LintIssue& b) { return a.kind < b.kind; });
    return issues;
}

std::vector<LintIssue> lintSurveyMap(const SurveyMap& map,
                                     const katana::entity::StyleLibrary& library,
                                     const ColourLookup& colourOf,
                                     const BuiltInSymbolTest& isBuiltInSymbol)
{
    std::vector<LintIssue> issues;
    const std::vector<SurveyRule>& rules = map.rules();
    // The earlier rules of each key. Only a rule of the SAME key always comes
    // before another in a lookup: a longer prefix or an exact key catches
    // only some of the codes a shorter one does, so it cannot shadow it.
    std::map<std::string, std::vector<std::size_t>> earlier;
    for (std::size_t i = 0; i < rules.size(); ++i) {
        const SurveyRule& rule = rules[i];
        std::vector<LintIssue> own = lintSurveyRule(rule, i, library, colourOf, isBuiltInSymbol);
        std::vector<std::size_t>& before = earlier[rule.key];

        const auto duplicate =
            std::find_if(before.begin(), before.end(), [&](std::size_t j) { return rules[j] == rule; });
        if (duplicate != before.end()) {
            own.push_back({i, rule.key, rule.section, LintSeverity::Warning, LintKind::DuplicateRule,
                           "the same as rule #" + std::to_string(*duplicate) +
                               ", field for field"});
        } else if (!before.empty()) {
            // Among rules of one key the earlier wins every field, and an
            // attribute is kept once per name - so a rule whose every field
            // and attribute an earlier one already sets never contributes.
            const auto setEarlier = [&](const FieldSpec& spec) {
                return std::any_of(before.begin(), before.end(),
                                   [&](std::size_t j) { return spec.sets(rules[j]); });
            };
            bool contributes = false;
            for (const FieldSpec& spec : fieldSpecs()) {
                contributes = contributes || (spec.sets(rule) && !setEarlier(spec));
            }
            for (const AttributeScope& scope : attributeScopes()) {
                for (const SurveyAttribute& attribute : scope.of(rule)) {
                    contributes = contributes ||
                                  std::none_of(before.begin(), before.end(), [&](std::size_t j) {
                                      return namesAttribute(scope.of(rules[j]), attribute.name);
                                  });
                }
            }
            if (!contributes) {
                std::vector<std::string> cited;
                for (const std::size_t j : before) {
                    cited.push_back("#" + std::to_string(j));
                }
                own.push_back({i, rule.key, rule.section, LintSeverity::Warning,
                               LintKind::ShadowedRule,
                               "earlier rules for " + inQuotes(rule.key) + " (" + joined(cited) +
                                   ") already set everything this one sets, so it never "
                                   "contributes"});
            }
        }
        before.push_back(i);
        std::stable_sort(own.begin(), own.end(), [](const LintIssue& a, const LintIssue& b) {
            return a.kind < b.kind;
        });
        issues.insert(issues.end(), own.begin(), own.end());
    }
    return issues;
}

// ---- one report text for every front end ------------------------------------------

std::string formatCodingReport(const SurveyCodingReport& report)
{
    std::ostringstream out;
    const std::size_t noRule = report.coded - report.matched - report.fallbackOnly;
    out << counted(report.coded, "entity carries", "entities carry") << " a code in "
        << inQuotes(report.property) << ": " << report.matched << " matched, "
        << report.fallbackOnly << " fallback-only (only the bare * rule answers), " << noRule
        << " with no rule\n";
    out << counted(report.changed, "entity", "entities") << " changed\n";
    if (!report.layersCreated.empty()) {
        out << "Layers created: " << joined(report.layersCreated) << "\n";
    }
    if (!report.stylesCreated.empty()) {
        out << "Styles created: " << joined(report.stylesCreated) << "\n";
    }
    if (!report.stylesReused.empty()) {
        out << "Styles reused: " << joined(report.stylesReused) << "\n";
    }
    if (report.skippedNoLayer != 0) {
        out << counted(report.skippedNoLayer, "entity", "entities")
            << " kept their layer: it does not exist and layers are not being created\n";
    }
    if (report.skippedNoStyle != 0) {
        out << counted(report.skippedNoStyle, "entity", "entities")
            << " kept their style: no style looks as the code says and styles are not being "
               "created\n";
    }
    if (!report.unmatchedCodes.empty()) {
        out << "Codes with no rule: " << joined(report.unmatchedCodes) << "\n";
    }
    if (!report.fallbackOnlyCodes.empty()) {
        out << "Fallback-only codes: " << joined(report.fallbackOnlyCodes) << "\n";
    }
    if (!report.missingDefinitions.empty()) {
        out << "Named but in no loaded library: " << joined(report.missingDefinitions) << "\n";
    }
    if (report.deferredAttributes != 0) {
        out << counted(report.deferredAttributes, "attribute", "attributes")
            << " left: the value names another attribute\n";
    }
    if (!report.codes.empty()) {
        out << "By code:\n";
    }
    for (const SurveyCodeRow& row : report.codes) {
        out << "  " << row.code << ": " << counted(row.entities, "entity", "entities") << ", "
            << katana::entity::toString(row.kind) << (row.matched ? ", matched" : ", not matched");
        if (row.layerKept != 0) {
            // Not an arrow: they did not move.
            out << "; layer " << joined(row.layersFrom) << " kept: " << row.layer
                << " does not exist and layers are not being created";
        } else if (row.layersFrom == std::vector<std::string>{row.layer}) {
            out << "; layer " << row.layer << " already";
        } else if (!row.layer.empty()) {
            out << "; layer " << joined(row.layersFrom) << " -> " << row.layer;
        }
        if (row.styleOutcome != SurveyStyleOutcome::None) {
            out << "; style ";
            if (!row.style.empty()) {
                out << row.style << " ";
            }
            out << "(" << toString(row.styleOutcome) << ")";
        }
        if (!row.attributesSet.empty()) {
            out << "; set " << joined(row.attributesSet);
        }
        if (!row.attributesDeferred.empty()) {
            out << "; deferred " << joined(row.attributesDeferred);
        }
        out << "; " << row.changed << " changed\n";
    }
    return out.str();
}

std::string formatCodeExplanation(const CodeExplanation& explanation)
{
    std::ostringstream out;
    out << "Code " << inQuotes(explanation.code) << ": ";
    switch (explanation.kind) {
    case SurveyMatchKind::None:
        out << "no rule matches it\n";
        break;
    case SurveyMatchKind::FallbackOnly:
        out << (explanation.matched ? "only the bare * rule answers it, and that names what it is\n"
                                    : "only the bare * rule answers it: fallback-only, not "
                                      "matched\n");
        break;
    default:
        out << katana::entity::toString(explanation.kind) << " match, matched\n";
        break;
    }
    for (const CodeFieldSource& field : explanation.fields) {
        out << "  " << field.field << ": " << field.value << "  <- "
            << ruleRef(field.rule, field.key, field.section) << "\n";
        for (const CodeFieldSource::Overruled& other : field.overruled) {
            out << "    overruled: " << other.value << "  <- "
                << ruleRef(other.rule, other.key, other.section) << "\n";
        }
    }
    if (!explanation.linestyle.name.empty()) {
        out << "  linestyle " << inQuotes(explanation.linestyle.name) << ": ";
        if (explanation.linestyle.plain) {
            out << "a plain line\n";
        } else if (!explanation.linestyle.defined) {
            out << "in no loaded library, so drawn as a plain line\n";
        } else {
            out << (explanation.linestyle.vertexMode
                        ? "defined, but as a symbol (at vertices)\n"
                        : "defined\n");
        }
    }
    if (!explanation.symbol.name.empty()) {
        out << "  symbol " << inQuotes(explanation.symbol.name) << ": ";
        if (explanation.symbol.defined) {
            out << (explanation.symbol.vertexMode ? "defined, at vertices\n" : "defined\n");
        } else if (explanation.symbol.builtIn) {
            out << "drawn by Katana itself\n";
        } else {
            out << "in no loaded library\n";
        }
    }
    if (!explanation.colour.name.empty()) {
        out << "  colour " << inQuotes(explanation.colour.name) << ": "
            << (explanation.colour.rgb ? explanation.colour.rgb->toHex()
                                       : std::string("unknown name, so the entity keeps its own"))
            << "\n";
    }
    for (const CodeAttribute& attribute : explanation.attributes) {
        out << "  " << attribute.scope << " attribute " << attribute.attribute.name << " = "
            << attribute.attribute.value;
        if (attribute.deferred) {
            out << " (deferred: the value names another attribute)";
        }
        out << "  <- " << ruleRef(attribute.rule, attribute.key, attribute.section) << "\n";
    }
    for (const CodeNearMiss& miss : explanation.nearMisses) {
        out << "  near miss: key " << inQuotes(miss.key) << " " << toString(miss.kind) << "\n";
    }
    return out.str();
}

std::string formatCodeTable(const std::vector<CodeTableRow>& rows, std::string_view filter)
{
    std::ostringstream out;
    std::size_t shown = 0;
    for (const CodeTableRow& row : rows) {
        if (!codeTableRowMatches(row, filter)) {
            continue;
        }
        ++shown;
        std::vector<std::string> sections;
        for (const SurveySection section : row.sections) {
            sections.emplace_back(katana::entity::toString(section));
        }
        std::vector<std::string> parts;
        const SurveyRule& combined = row.combined;
        if (!combined.model.empty()) {
            parts.push_back("layer " + combined.model);
        }
        if (!combined.linestyle.empty()) {
            parts.push_back("linestyle " + combined.linestyle);
        }
        if (combined.symbol && !combined.symbol->style.empty()) {
            parts.push_back("symbol " + combined.symbol->style);
        }
        if (!combined.colour.empty()) {
            parts.push_back("colour " + combined.colour);
        }
        if (!combined.group.empty()) {
            parts.push_back("group " + combined.group);
        }
        if (!combined.comment.empty()) {
            parts.push_back(inQuotes(combined.comment));
        }
        out << row.key << "  [" << katana::entity::toString(row.kind) << "; "
            << counted(row.rules.size(), "rule", "rules") << ": " << joined(sections) << "]";
        if (!parts.empty()) {
            out << "  " << joined(parts, "; ");
        }
        out << "\n";
    }
    out << shown << " of " << counted(rows.size(), "code", "codes");
    if (!filter.empty()) {
        out << " match " << inQuotes(filter);
    }
    out << "\n";
    return out.str();
}

std::string formatCodeCensus(const CodeCensus& census)
{
    std::ostringstream out;
    out << counted(census.coded, "entity carries", "entities carry") << " a code in "
        << inQuotes(census.property) << ", " << counted(census.codes.size(), "distinct code",
                                                       "distinct codes")
        << "\n";
    for (const CodeCensusRow& row : census.codes) {
        out << "  " << row.code << ": " << row.entities << ", "
            << katana::entity::toString(row.kind) << (row.matched ? ", matched" : ", not matched");
        if (!row.model.empty()) {
            out << ", layer " << row.model;
        }
        out << "\n";
    }
    return out.str();
}

std::string formatLint(const std::vector<LintIssue>& issues, std::size_t rules)
{
    std::ostringstream out;
    const auto errors = static_cast<std::size_t>(
        std::count_if(issues.begin(), issues.end(),
                      [](const LintIssue& issue) { return issue.severity == LintSeverity::Error; }));
    out << counted(rules, "rule", "rules") << " checked: ";
    if (issues.empty()) {
        out << "no problems found\n";
        return out.str();
    }
    out << counted(errors, "error", "errors") << ", "
        << counted(issues.size() - errors, "warning", "warnings") << "\n";
    // The reference customisation gives hundreds, most of a few kinds, so the
    // count of each kind comes before the list it summarises.
    std::map<LintKind, std::size_t> byKind;
    for (const LintIssue& issue : issues) {
        ++byKind[issue.kind];
    }
    std::vector<std::string> kinds;
    for (const auto& [kind, count] : byKind) {
        kinds.push_back(std::to_string(count) + " " + toString(kind));
    }
    out << "  by kind: " << joined(kinds) << "\n";
    for (const LintIssue& issue : issues) {
        out << "  " << ruleRef(issue.rule, issue.key, issue.section) << ": "
            << toString(issue.severity) << ", " << toString(issue.kind) << ": " << issue.message
            << "\n";
    }
    return out.str();
}

std::string formatCoverage(const CustomisationCoverage& coverage)
{
    std::ostringstream out;
    if (coverage.styles == 0) {
        out << "This drawing has no styles yet; import a drawing or survey that carries "
               "styles, or make one in Format > Styles and Linetypes or with STYLE NEW, to "
               "see the customisation take effect.\n";
        return out.str();
    }
    out << coverage.resolved << " of this drawing's " << coverage.styles
        << " styles are drawn with a loaded definition (" << coverage.named << " name one";
    if (coverage.builtIn != 0) {
        out << ", " << coverage.builtIn << " use a shape Katana draws itself";
    }
    out << "; the rest are plain lines)\n";
    if (!coverage.unresolved.empty()) {
        std::vector<std::string> names;
        for (const std::string& name : coverage.unresolved) {
            names.push_back(inQuotes(name));
        }
        out << "  " << counted(coverage.unresolved.size(), "name is", "names are")
            << " in no loaded library: " << joined(names) << "\n";
    }
    // Its own line, because the library that defines these IS loaded: the
    // fix is a linestyle in the style, not another library.
    if (!coverage.notLinestyles.empty()) {
        std::vector<std::string> names;
        for (const std::string& name : coverage.notLinestyles) {
            names.push_back(inQuotes(name));
        }
        out << "  "
            << counted(coverage.notLinestyles.size(),
                       "name is loaded as an `at vertices` symbol, not a linestyle, so a "
                       "linetype naming it",
                       "names are loaded as `at vertices` symbols, not linestyles, so a "
                       "linetype naming one")
            << " draws solid: " << joined(names) << "\n";
    }
    return out.str();
}

} // namespace katana::cad
