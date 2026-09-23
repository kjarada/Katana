#include "katana/cad/survey_coding.hpp"

#include <algorithm>
#include <cctype>
#include <map>
#include <set>
#include <tuple>

#include "katana/commands/entity_commands.hpp"
#include "katana/entity/tables.hpp"

namespace katana::cad {

namespace {

namespace cmd = katana::commands;
using katana::entity::Color;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::entity::Style;
using katana::entity::SurveyMatch;
using katana::entity::SurveyMatchKind;

[[nodiscard]] std::string lowered(std::string_view text)
{
    std::string out(text);
    std::transform(out.begin(), out.end(), out.begin(), [](unsigned char c) {
        return static_cast<char>(std::tolower(c));
    });
    return out;
}

// What a code looks like on the page (decision D4): the linestyle or a plain
// line, the symbol and its size, and the colour. Two codes with the same
// appearance share a style; two with different ones never do.
struct Appearance {
    std::string linestyle{}; // empty: a plain line
    std::string symbol{};
    double symbolSize = 0.0;
    // What colourOf made of the rule's colour name, or nothing.
    std::optional<Color> colour{};
    // The name, when there is one: a name the callback does not know is the
    // ONLY thing that tells "sui water potable" from "sui electricity", so
    // then it is part of the appearance; a known one is only its label.
    std::string colourName{};

    // The colour's identity: its RGB when that is known (so "White" and
    // "white" are one colour), otherwise its name, otherwise nothing.
    [[nodiscard]] std::string colourIdentity() const
    {
        if (colour) {
            return "rgb " + colour->toHex();
        }
        return colourName.empty() ? std::string{} : "name " + colourName;
    }

    // A fixed order over appearances, which is what makes the names given to
    // new styles follow from the rules rather than from the entities.
    using Identity = std::tuple<std::string, std::string, double, std::string>;
    [[nodiscard]] Identity identity() const
    {
        return {linestyle, symbol, symbolSize, colourIdentity()};
    }
};

// Whether an existing style draws exactly this appearance. Only the four
// parts of an appearance are compared - a weight or a description someone
// changed on a coded style does not stop it being reused, and neither does a
// new name, which is the point.
[[nodiscard]] bool drawsAs(const Style& style, const Appearance& appearance)
{
    const bool lineMatches = appearance.linestyle.empty() ? isPlainLinestyle(style.linetype)
                                                          : style.linetype == appearance.linestyle;
    if (!lineMatches || style.symbol != appearance.symbol ||
        style.symbolSize != appearance.symbolSize) {
        return false;
    }
    if (appearance.colour) {
        return style.color == appearance.colour;
    }
    // An unknown colour name has no RGB, so a style made for it has none
    // either and keeps the name in its description - the same as the 12da
    // import does.
    return !style.color.has_value() &&
           (appearance.colourName.empty() || style.description == appearance.colourName);
}

// The appearance a combined rule gives, or nothing when it says nothing
// about how its code is drawn - a code only attributes are attached to keeps
// its style.
[[nodiscard]] std::optional<Appearance> appearanceOf(const SurveyMatch& match,
                                                     const SurveyCodingOptions& options)
{
    const katana::entity::SurveyRule& rule = match.resolved;
    const bool hasSymbol = rule.symbol.has_value() && !rule.symbol->style.empty();
    if (rule.linestyle.empty() && !hasSymbol && rule.colour.empty()) {
        return std::nullopt;
    }
    Appearance appearance;
    appearance.linestyle = isPlainLinestyle(rule.linestyle) ? std::string{} : rule.linestyle;
    if (hasSymbol) {
        appearance.symbol = rule.symbol->style;
        appearance.symbolSize = rule.symbol->size;
    }
    appearance.colourName = rule.colour;
    if (options.colourOf && !rule.colour.empty()) {
        appearance.colour = options.colourOf(rule.colour);
    }
    return appearance;
}

// The style an existing drawing already has for an appearance: the first in
// name order, so which one is chosen does not depend on how the table was
// filled.
[[nodiscard]] const Style* existingStyleFor(const katana::entity::Model& model,
                                            const Appearance& appearance)
{
    const Style* found = nullptr;
    model.styles.forEach([&](const Style& style) {
        if (found == nullptr && drawsAs(style, appearance)) {
            found = &style;
        }
    });
    return found;
}

// The name a new style for this appearance takes: the linestyle, else the
// symbol, else "Plain"; then with the colour; then numbered. `taken` says
// whether a name is already held - by an existing style (which, having
// failed drawsAs, draws something else) or by a style planned earlier in the
// fixed order.
template <class Taken>
[[nodiscard]] std::string nameFor(const Appearance& appearance, const Taken& taken)
{
    const std::string base = !appearance.linestyle.empty() ? appearance.linestyle
                             : !appearance.symbol.empty()  ? appearance.symbol
                                                           : std::string("Plain");
    if (!taken(base)) {
        return base;
    }
    std::string stem = base;
    if (!appearance.colourName.empty()) {
        stem += " (" + appearance.colourName + ")";
        if (!taken(stem)) {
            return stem;
        }
    }
    for (std::size_t n = 2;; ++n) {
        std::string numbered = stem + " " + std::to_string(n);
        if (!taken(numbered)) {
            return numbered;
        }
    }
}

// Whether the library lacks a name a rule gives. Never true of a plain line
// or of one of the shapes Katana draws itself: both draw correctly without a
// library, so neither is "missing".
[[nodiscard]] bool missingFromLibrary(const Document& document, const std::string& name,
                                      bool symbol)
{
    if (isPlainLinestyle(name)) {
        return false;
    }
    if (symbol && katana::entity::isBuiltInSymbolName(name)) {
        return false;
    }
    return document.definitionFor(name) == nullptr;
}

[[nodiscard]] bool propertyHolds(const Entity& entity, const std::string& name,
                                 const std::string& value)
{
    const auto found = entity.properties.find(name);
    if (found == entity.properties.end()) {
        return false;
    }
    const auto* text = std::get_if<std::string>(&found->second);
    return text != nullptr && *text == value;
}

} // namespace

const std::vector<std::string>& codePropertyCandidates()
{
    static const std::vector<std::string> candidates = {
        "code",     // a survey file's own field code
        "12d.name", // a 12da string's name, which is what a coded survey puts there
        "Code",     "CODE", "feature_code",
    };
    return candidates;
}

const std::string* surveyCodeOf(const Entity& entity, const std::string& property)
{
    for (const katana::entity::PropertyMap* where : {&entity.properties, &entity.metadata}) {
        const auto found = where->find(property);
        if (found == where->end()) {
            continue;
        }
        // A property of the right name that is not a code does not fall
        // through to the metadata: it IS the entity's value for that name.
        const auto* text = std::get_if<std::string>(&found->second);
        return text != nullptr && !text->empty() ? text : nullptr;
    }
    return nullptr;
}

std::string findCodeProperty(const katana::entity::Model& model,
                             const std::vector<EntityId>& subject)
{
    for (const std::string& candidate : codePropertyCandidates()) {
        for (const EntityId id : subject) {
            const Entity* entity = model.entities.find(id);
            if (entity != nullptr && surveyCodeOf(*entity, candidate) != nullptr) {
                return candidate;
            }
        }
    }
    return codePropertyCandidates().front();
}

bool isPlainLinestyle(std::string_view name)
{
    // Folded because 12d's own names are compared without regard to case,
    // and "Continuous" in a hand-written mapfile means what "continuous" does.
    const std::string word = lowered(name);
    return word.empty() || word == "0" || word == "1" || word == katana::entity::kContinuousLinetype;
}

const char* toString(SurveyStyleOutcome outcome)
{
    switch (outcome) {
    case SurveyStyleOutcome::None:
        return "none";
    case SurveyStyleOutcome::Reused:
        return "reused";
    case SurveyStyleOutcome::Created:
        return "created";
    case SurveyStyleOutcome::Skipped:
        return "skipped";
    }
    return "none";
}

CustomisationCoverage customisationCoverage(const Document& document)
{
    CustomisationCoverage coverage;
    std::set<std::string> missing;
    // forEach rather than all(): all() returns the whole style table by
    // value, and this only reads it.
    document.model().styles.forEach([&](const Style& style) {
        ++coverage.styles;
        // A style names a definition through either field. A plain line names
        // nothing; neither does a name Katana draws itself - a linetype of
        // the drawing's own, or a built-in symbol shape - unless a loaded
        // library defines it, which then wins (decision D2).
        bool names = false;
        bool found = false;
        bool katanaDrawn = false;
        for (const bool symbol : {false, true}) {
            const std::string& name = symbol ? style.symbol : style.linetype;
            if (isPlainLinestyle(name)) {
                continue;
            }
            if (document.definitionFor(name) != nullptr) {
                names = true;
                found = true;
            } else if (symbol ? katana::entity::isBuiltInSymbolName(name)
                              : document.model().linetypes.contains(name)) {
                katanaDrawn = true;
            } else {
                names = true;
                missing.insert(name);
            }
        }
        coverage.named += names ? 1 : 0;
        coverage.resolved += found ? 1 : 0;
        coverage.builtIn += (katanaDrawn && !names) ? 1 : 0;
    });
    coverage.unresolved.assign(missing.begin(), missing.end());
    return coverage;
}

katana::core::Result<katana::commands::CommandPtr>
applySurveyCodes(const Document& document, const SurveyCodingOptions& options,
                 SurveyCodingReport* report)
{
    const katana::entity::Model& model = document.model();
    const katana::entity::SurveyMap& map = document.surveyMap();
    SurveyCodingReport tally;

    std::vector<EntityId> subject = options.ids;
    if (subject.empty()) {
        subject = model.entities.ids();
    }
    const std::string property =
        options.property.empty() ? findCodeProperty(model, subject) : options.property;
    tally.property = property;

    // The entities by code. Everything below is decided once per DISTINCT
    // code rather than once per entity: a survey of 30,000 points is a few
    // hundred codes, and each lookup combines every rule that matches.
    std::map<std::string, std::vector<EntityId>> entitiesByCode;
    for (const EntityId id : subject) {
        const Entity* entity = model.entities.find(id);
        if (entity == nullptr) {
            continue;
        }
        if (const std::string* code = surveyCodeOf(*entity, property); code != nullptr) {
            ++tally.coded;
            entitiesByCode[*code].push_back(id);
        }
    }

    struct CodePlan {
        const std::string* code = nullptr;
        const std::vector<EntityId>* ids = nullptr;
        SurveyMatch match{};
        std::optional<Appearance> appearance{};
    };
    std::vector<CodePlan> plans;
    plans.reserve(entitiesByCode.size());
    std::set<std::string> missing;
    for (const auto& [code, ids] : entitiesByCode) {
        CodePlan plan{&code, &ids, map.lookup(code), std::nullopt};
        if (!plan.match.empty()) {
            // Checked for every code, not only when it creates a style: a
            // code given an existing style names the same definitions.
            const katana::entity::SurveyRule& rule = plan.match.resolved;
            if (!rule.linestyle.empty() && missingFromLibrary(document, rule.linestyle, false)) {
                missing.insert(rule.linestyle);
            }
            if (rule.symbol && !rule.symbol->style.empty() &&
                missingFromLibrary(document, rule.symbol->style, true)) {
                missing.insert(rule.symbol->style);
            }
            plan.appearance = appearanceOf(plan.match, options);
        }
        plans.push_back(std::move(plan));
    }

    // Each distinct appearance gets ONE style, found or made, in a fixed
    // order. Codes that share an appearance but spell its known colour
    // differently ("White", "white") share the style, labelled by the first
    // spelling in name order.
    struct StyleChoice {
        Appearance appearance{};
        std::string name{};
        SurveyStyleOutcome outcome = SurveyStyleOutcome::None;
    };
    std::map<Appearance::Identity, StyleChoice> choices;
    for (const CodePlan& plan : plans) {
        if (!plan.appearance) {
            continue;
        }
        auto [at, inserted] = choices.try_emplace(plan.appearance->identity());
        if (inserted || plan.appearance->colourName < at->second.appearance.colourName) {
            at->second.appearance = *plan.appearance;
        }
    }
    std::set<std::string> planned;
    std::map<std::string, Style> stylesNeeded;
    std::set<std::string> reused;
    const auto taken = [&](const std::string& name) {
        return model.styles.contains(name) || planned.contains(name);
    };
    for (auto& [identity, choice] : choices) {
        if (const Style* existing = existingStyleFor(model, choice.appearance)) {
            choice.name = existing->name;
            choice.outcome = SurveyStyleOutcome::Reused;
            continue;
        }
        if (!options.createStyles) {
            choice.outcome = SurveyStyleOutcome::Skipped;
            continue;
        }
        choice.name = nameFor(choice.appearance, taken);
        choice.outcome = SurveyStyleOutcome::Created;
        planned.insert(choice.name);

        Style style;
        style.name = choice.name;
        // The linestyle and the symbol are NAMES resolved when they are
        // drawn, against the loaded library. Recording a name the library
        // does not define is deliberate: it is what 12d says this is, and it
        // draws plainly until a library defines it.
        style.linetype = choice.appearance.linestyle.empty()
                             ? std::string(katana::entity::kContinuousLinetype)
                             : choice.appearance.linestyle;
        style.symbol = choice.appearance.symbol;
        style.symbolSize = choice.appearance.symbolSize;
        // The 12d colour name is kept whatever Katana makes of it, so a
        // colour this build has no RGB for is still known by name - the same
        // thing the 12da import does.
        style.description = choice.appearance.colourName;
        style.color = choice.appearance.colour;
        if (auto status = katana::entity::validate(style); !status) {
            return status.error();
        }
        stylesNeeded.emplace(choice.name, std::move(style));
    }

    // Grouped, so that one command covers every entity that resolved the same
    // way: a command per entity would make an undo stack nobody can use.
    std::map<std::string, std::vector<EntityId>> byLayer;
    std::map<std::string, std::vector<EntityId>> byStyle;
    std::map<std::pair<std::string, std::string>, std::vector<EntityId>> byProperty;
    std::set<std::string> layersNeeded;
    std::set<std::string> unmatched;
    std::set<std::string> fallbackOnly;
    std::set<EntityId> changed;

    for (const CodePlan& plan : plans) {
        const SurveyMatch& match = plan.match;
        SurveyCodeRow row;
        row.code = *plan.code;
        row.entities = plan.ids->size();
        row.kind = match.kind;
        row.matched = match.matched();
        row.layer = match.resolved.model;

        std::set<std::string> layersFrom;
        for (const EntityId id : *plan.ids) {
            layersFrom.insert(model.entities.find(id)->layer);
        }
        row.layersFrom.assign(layersFrom.begin(), layersFrom.end());

        if (match.empty()) {
            unmatched.insert(row.code);
            tally.codes.push_back(std::move(row));
            continue;
        }
        if (row.matched) {
            tally.matched += row.entities;
        } else {
            tally.fallbackOnly += row.entities;
            fallbackOnly.insert(row.code);
        }

        const StyleChoice* choice = nullptr;
        if (plan.appearance) {
            choice = &choices.at(plan.appearance->identity());
            row.style = choice->name;
            row.styleOutcome = choice->outcome;
            if (choice->outcome == SurveyStyleOutcome::Reused) {
                reused.insert(choice->name);
            }
        }
        const bool layerExists = row.layer.empty() || model.layers.contains(row.layer);
        for (const katana::entity::SurveyAttribute& attribute : match.resolved.attributes) {
            // "$PipeDiameter" names another attribute rather than being a
            // value. Resolving it needs the survey data this drawing came
            // from, which is not here, so it is counted and left.
            const bool deferred = !attribute.value.empty() && attribute.value.front() == '$';
            (deferred ? row.attributesDeferred : row.attributesSet).push_back(attribute.name);
        }
        if (!options.setAttributes) {
            row.attributesSet.clear();
            row.attributesDeferred.clear();
        }

        for (const EntityId id : *plan.ids) {
            const Entity& entity = *model.entities.find(id);
            bool alters = false;
            // Each part is decided on its own: a layer that cannot be made
            // does not stop the style and the attributes (audit CAD-18).
            if (!row.layer.empty() && entity.layer != row.layer) {
                if (!layerExists && !options.createLayers) {
                    ++tally.skippedNoLayer;
                } else {
                    if (!layerExists) {
                        layersNeeded.insert(row.layer);
                    }
                    byLayer[row.layer].push_back(id);
                    alters = true;
                }
            }
            if (choice != nullptr) {
                if (choice->outcome == SurveyStyleOutcome::Skipped) {
                    ++tally.skippedNoStyle;
                } else if (entity.style != choice->name) {
                    byStyle[choice->name].push_back(id);
                    alters = true;
                }
            }
            if (options.setAttributes) {
                for (const katana::entity::SurveyAttribute& attribute :
                     match.resolved.attributes) {
                    if (!attribute.value.empty() && attribute.value.front() == '$') {
                        ++tally.deferredAttributes;
                        continue;
                    }
                    if (!propertyHolds(entity, attribute.name, attribute.value)) {
                        byProperty[{attribute.name, attribute.value}].push_back(id);
                        alters = true;
                    }
                }
            }
            if (alters) {
                ++row.changed;
                changed.insert(id);
            }
        }
        tally.codes.push_back(std::move(row));
    }

    tally.changed = changed.size();
    tally.unmatchedCodes.assign(unmatched.begin(), unmatched.end());
    tally.fallbackOnlyCodes.assign(fallbackOnly.begin(), fallbackOnly.end());
    tally.missingDefinitions.assign(missing.begin(), missing.end());
    tally.layersCreated.assign(layersNeeded.begin(), layersNeeded.end());
    tally.stylesReused.assign(reused.begin(), reused.end());
    for (const auto& [name, style] : stylesNeeded) {
        tally.stylesCreated.push_back(name);
    }

    auto transaction = std::make_unique<cmd::Transaction>("APPLY SURVEY CODES");
    for (const std::string& name : tally.layersCreated) {
        katana::entity::Layer layer;
        layer.name = name;
        transaction->add(cmd::createLayer(std::move(layer)));
    }
    for (auto& [name, style] : stylesNeeded) {
        transaction->add(cmd::createStyle(style));
    }
    for (auto& [layer, ids] : byLayer) {
        transaction->add(cmd::setEntityLayer(std::move(ids), layer));
    }
    for (auto& [style, ids] : byStyle) {
        transaction->add(cmd::setEntityStyle(std::move(ids), style));
    }
    for (auto& [nameAndValue, ids] : byProperty) {
        transaction->add(cmd::setEntityProperty(std::move(ids), nameAndValue.first,
                                                katana::entity::PropertyValue(nameAndValue.second)));
    }

    if (report != nullptr) {
        *report = std::move(tally);
    }
    if (transaction->size() == 0) {
        return katana::commands::CommandPtr{}; // nothing to do, and no error
    }
    return katana::commands::CommandPtr{std::move(transaction)};
}

} // namespace katana::cad
