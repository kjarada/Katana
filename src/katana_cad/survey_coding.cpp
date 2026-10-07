#include "katana/cad/survey_coding.hpp"

#include <algorithm>
#include <map>
#include <set>
#include <tuple>
#include <utility>

#include "katana/cad/linework.hpp"
#include "katana/cad/style_catalogue.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/display.hpp"
#include "katana/core/text.hpp"
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

// The code a rule is looked up by. A POINT's code is written on a field
// controller, where the string name is followed by linework controls -
// "PABB ST" starts a string of PABB - and the mapfile's key is the name
// alone: the whole field, blank included, can never match one, because no
// key holds a blank (SurveyMap::add refuses it). So a point is coded by its
// string name, as processLinework reads the same field. Any other entity's
// code is taken as it is: the lines linework makes carry the name alone.
//
// Either is then followed by the string number the entity carries beside its
// code, when it carries one (surveyLookupName says when that name is the one
// looked up). Without one this is the code, exactly as it was.
[[nodiscard]] std::string codeToLookUp(const katana::entity::SurveyMap& map, const Entity& entity,
                                       const std::string& code)
{
    std::string name = code;
    if (entity.type() == katana::entity::EntityType::Point) {
        FieldCode field = parseFieldCode(code, LineworkCodes{});
        // Blanks alone have no first token; the field is then reported as it is.
        if (!field.name.empty()) {
            name = std::move(field.name);
        }
    }
    const std::string_view string = surveyStringOf(entity);
    return string.empty() ? name : surveyLookupName(map, name, string);
}

using katana::core::lowered;

// What a code looks like on the page (decision D4): the linestyle or a plain
// line, the symbol and its size, and the colour. Two codes with the same
// appearance share a style; two with different ones never do.
struct Appearance {
    std::string linestyle{}; // empty: a plain line
    std::string symbol{};
    double symbolSize = 0.0;
    // What colourOf made of the rule's colour name, or nothing.
    std::optional<Color> colour{};
    // The name, when there is one. A name the callback does not know is the
    // only thing that tells "sui water potable" from "sui electricity", so
    // it keeps two such codes apart when styles are CREATED for them - each
    // gets its own, carrying its name. It does not change how either DRAWS:
    // with no RGB both draw with no colour of their own, which is why reuse
    // (existingStyleFor) does not ask for the name. A known name is only a
    // label for its RGB.
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

// Whether an existing style draws exactly this appearance. Only what is drawn
// is compared - a weight or a description someone changed on a coded style
// does not stop it being reused, and neither does a new name, which is the
// point.
[[nodiscard]] bool drawsAs(const Style& style, const Appearance& appearance)
{
    const bool lineMatches = appearance.linestyle.empty() ? isPlainLinestyle(style.linetype)
                                                          : style.linetype == appearance.linestyle;
    if (!lineMatches || style.symbol != appearance.symbol ||
        style.symbolSize != appearance.symbolSize) {
        return false;
    }
    // A colour name with no RGB draws as no colour at all, whatever the name:
    // an archive import's "WATR Main" (no colour, the import's description)
    // draws exactly as the style coding would make for "sui water potable".
    // Asking for the name here too made coding duplicate every such style
    // whose description was not that name.
    return appearance.colour ? style.color == appearance.colour : !style.color.has_value();
}

// The colour names in the map that colourOf has no RGB for. A style made by
// coding for one of these has no colour and carries the name as its
// description - the only mark of which colour it was made for.
[[nodiscard]] std::set<std::string> unknownColourNames(const katana::entity::SurveyMap& map,
                                                       const SurveyCodingOptions& options)
{
    std::set<std::string> asked;
    std::set<std::string> unknown;
    for (const katana::entity::SurveyRule& rule : map.rules()) {
        if (rule.colour.empty() || !asked.insert(rule.colour).second) {
            continue;
        }
        if (!options.colourOf || !options.colourOf(rule.colour)) {
            unknown.insert(rule.colour);
        }
    }
    return unknown;
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

// The style an existing drawing already has for an appearance. Of those that
// draw it, one whose description is the appearance's colour name comes first:
// that is the style an earlier pass made for it, so applying codes again
// keeps each code on its own style even when several draw alike. Otherwise the
// first in name order, so the choice never depends on how the table was
// filled - except that, for a colour with no RGB, a style described by ANOTHER
// unknown colour name of the map is that colour's style, not this one's. The
// map decides that set, not the entities being coded, so a code added later
// gets the style a fresh drawing would give it rather than borrowing its
// neighbour's.
[[nodiscard]] const Style* existingStyleFor(const katana::entity::Model& model,
                                            const Appearance& appearance,
                                            const std::set<std::string>& unknownColours)
{
    const Style* own = nullptr;
    const Style* other = nullptr;
    model.styles.forEach([&](const Style& style) {
        if (!drawsAs(style, appearance)) {
            return;
        }
        if (style.description == appearance.colourName) {
            own = own != nullptr ? own : &style;
        } else if (appearance.colour.has_value() || !unknownColours.contains(style.description)) {
            other = other != nullptr ? other : &style;
        }
    });
    return own != nullptr ? own : other;
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
        "12d.name", // an archive string's name, which is what a coded survey puts there
        "Code",     "CODE", "feature_code",
    };
    return candidates;
}

std::string_view surveyStringOf(const Entity& entity)
{
    const auto found = entity.properties.find(kSurveyStringProperty);
    if (found == entity.properties.end()) {
        return {};
    }
    const auto* text = std::get_if<std::string>(&found->second);
    return text != nullptr ? katana::core::trimmed(*text) : std::string_view{};
}

std::string surveyLookupName(const katana::entity::SurveyMap& map, std::string_view code,
                             std::string_view string)
{
    if (code.empty() || string.empty()) {
        return std::string(code);
    }
    std::string name(code);
    name += string;
    // Most specific first, so when the first rule is the bare "*" nothing
    // else answers the name: a handful of hash probes, not a combined lookup.
    const auto rules = map.match(name);
    if (rules.empty() || rules.front()->key == "*") {
        return std::string(code);
    }
    // The best rule for the name. An exact key, or a prefix as long as the
    // code or longer ("KJ*" for KJ, "B1*" for B), was written for the
    // numbered names of this code. A SHORTER prefix ("PA*" for PTBB) is a
    // rule for a family of codes: it answers the code itself too, and less
    // well than an exact key the code has of its own - which the number must
    // not take the code away from.
    const std::string& best = rules.front()->key;
    if (best.back() == '*' && best.size() - 1 < code.size()) {
        const auto own = map.match(code);
        if (!own.empty() && own.front()->key == code) {
            return std::string(code);
        }
    }
    return name;
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
    // Folded because library names are compared without regard to case,
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
    std::set<std::string> undefined;
    std::set<std::string> notLinestyles;
    // forEach rather than all(): all() returns the whole style table by
    // value, and this only reads it.
    document.model().styles.forEach([&](const Style& style) {
        ++coverage.styles;
        // A style names a definition through either field, judged by the one
        // rule cad::missingNames reads (style_catalogue.hpp), so this list
        // and the style manager's Diagnostics cannot disagree. A plain line,
        // ByLayer and a linetype that is the style's own symbol (D8) name
        // nothing; a model linetype and a built-in symbol shape are drawn by
        // Katana itself, unless a loaded library defines the name, which then
        // wins (decision D2).
        bool names = false;
        bool found = false;
        bool katanaDrawn = false;
        const std::pair<NameStatus, const std::string*> fields[] = {
            {linetypeStatus(document, style.linetype, style.symbol), &style.linetype},
            {symbolStatus(document, style.symbol), &style.symbol}};
        for (const auto& [status, name] : fields) {
            switch (status) {
            case NameStatus::Plain:
            case NameStatus::OwnSymbol:
                break;
            case NameStatus::Library:
                names = true;
                found = true;
                break;
            case NameStatus::Katana:
                katanaDrawn = true;
                break;
            // Kept apart, not merged into one "missing" list: a caller
            // printing "in no loaded library" for a name the loaded library
            // defines as a symbol sends people to load what is loaded.
            case NameStatus::NotALinestyle:
                names = true;
                notLinestyles.insert(*name);
                break;
            case NameStatus::Undefined:
                names = true;
                undefined.insert(*name);
                break;
            }
        }
        coverage.named += names ? 1 : 0;
        coverage.resolved += found ? 1 : 0;
        coverage.builtIn += (katanaDrawn && !names) ? 1 : 0;
    });
    coverage.unresolved.assign(undefined.begin(), undefined.end());
    coverage.notLinestyles.assign(notLinestyles.begin(), notLinestyles.end());
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
            entitiesByCode[codeToLookUp(map, *entity, *code)].push_back(id);
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
    const std::set<std::string> unknownColours =
        choices.empty() ? std::set<std::string>{} : unknownColourNames(map, options);
    for (auto& [identity, choice] : choices) {
        if (const Style* existing = existingStyleFor(model, choice.appearance, unknownColours)) {
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
        // does not define is deliberate: it is what the rules say this is, and it
        // draws plainly until a library defines it.
        style.linetype = choice.appearance.linestyle.empty()
                             ? std::string(katana::entity::kContinuousLinetype)
                             : choice.appearance.linestyle;
        style.symbol = choice.appearance.symbol;
        style.symbolSize = choice.appearance.symbolSize;
        // The colour name is kept whatever Katana makes of it, so a colour
        // this build has no RGB for is still known by name - the same thing
        // an archive import does.
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
                    ++row.layerKept;
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
