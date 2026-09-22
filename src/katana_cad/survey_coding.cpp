#include "katana/cad/survey_coding.hpp"

#include <algorithm>
#include <map>
#include <set>

#include "katana/commands/entity_commands.hpp"

namespace katana::cad {

namespace {

namespace cmd = katana::commands;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::entity::SurveyMatch;

// A 12d model name is a Katana layer name. They are both `/`-separated paths
// and both name where something lives, which is the mapping the 12da import
// already makes - see docs/interop.md.
[[nodiscard]] std::string layerFor(const SurveyMatch& match)
{
    return match.resolved.model;
}

// What the style holding this rule's appearance is called. A 12d linestyle
// name if there is one, otherwise the symbol's - which is the same rule the
// 12da import follows, so a drawing coded here and a drawing imported from
// 12d end up with styles of the same names rather than two sets.
[[nodiscard]] std::string styleNameFor(const SurveyMatch& match)
{
    if (!match.resolved.linestyle.empty()) {
        return match.resolved.linestyle;
    }
    if (match.resolved.symbol && !match.resolved.symbol->style.empty()) {
        return match.resolved.symbol->style;
    }
    return {};
}

// The code an entity carries, or nothing. Only text is a code: a number in
// that property is a measurement someone named badly, and treating "1.5" as
// a field code would put it in whatever model the rule for `1*` names.
[[nodiscard]] const std::string* codeOf(const Entity& entity, const std::string& property)
{
    const auto found = entity.properties.find(property);
    if (found == entity.properties.end()) {
        return nullptr;
    }
    const auto* text = std::get_if<std::string>(&found->second);
    return text != nullptr && !text->empty() ? text : nullptr;
}

} // namespace

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

    // Grouped, so that one command covers every entity that resolved the same
    // way: a survey of 30,000 points is a few hundred codes, and a command
    // per entity would make an undo stack nobody can use.
    std::map<std::string, std::vector<EntityId>> byLayer;
    std::map<std::string, std::vector<EntityId>> byStyle;
    std::map<std::string, std::map<std::string, std::string>> attributesByCode;
    std::map<std::string, std::vector<EntityId>> entitiesByCode;
    std::map<std::string, katana::entity::Style> stylesNeeded;
    std::set<std::string> layersNeeded;
    std::set<std::string> unmatched;
    std::set<std::string> missing;

    for (const EntityId id : subject) {
        const Entity* entity = model.entities.find(id);
        if (entity == nullptr) {
            continue;
        }
        const std::string* code = codeOf(*entity, options.property);
        if (code == nullptr) {
            continue;
        }
        ++tally.coded;
        const SurveyMatch match = map.lookup(*code);
        if (match.empty()) {
            unmatched.insert(*code);
            continue;
        }
        ++tally.matched;
        entitiesByCode[*code].push_back(id);

        const std::string layer = layerFor(match);
        if (!layer.empty() && entity->layer != layer) {
            if (!model.layers.contains(layer)) {
                if (!options.createLayers) {
                    continue;
                }
                layersNeeded.insert(layer);
            }
            byLayer[layer].push_back(id);
        }

        const std::string styleName = styleNameFor(match);
        if (!styleName.empty()) {
            if (!model.styles.contains(styleName) && options.createStyles &&
                !stylesNeeded.contains(styleName)) {
                katana::entity::Style style;
                style.name = styleName;
                // The linestyle and the symbol are NAMES resolved when they
                // are drawn, against the loaded library. Recording a name the
                // library does not define is deliberate: it is what 12d says
                // this is, and it draws plainly until a library defines it.
                if (!match.resolved.linestyle.empty()) {
                    style.linetype = match.resolved.linestyle;
                    if (!document.styleLibrary().contains(match.resolved.linestyle)) {
                        missing.insert(match.resolved.linestyle);
                    }
                }
                if (match.resolved.symbol && !match.resolved.symbol->style.empty()) {
                    style.symbol = match.resolved.symbol->style;
                    style.symbolSize = match.resolved.symbol->size;
                    if (!document.styleLibrary().contains(match.resolved.symbol->style)) {
                        missing.insert(match.resolved.symbol->style);
                    }
                }
                // The 12d colour name is kept whatever Katana makes of it, so
                // a colour this build has no RGB for is still known by name -
                // the same thing the 12da import does.
                style.description = match.resolved.colour;
                if (options.colourOf && !match.resolved.colour.empty()) {
                    style.color = options.colourOf(match.resolved.colour);
                }
                if (auto status = katana::entity::validate(style); !status) {
                    return status.error();
                }
                stylesNeeded.emplace(styleName, std::move(style));
            }
            if (entity->style != styleName &&
                (model.styles.contains(styleName) || stylesNeeded.contains(styleName))) {
                byStyle[styleName].push_back(id);
            }
        }

        if (options.setAttributes) {
            for (const katana::entity::SurveyAttribute& attribute : match.resolved.attributes) {
                // "$PipeDiameter" names another attribute rather than being a
                // value. Resolving it needs the survey data this drawing came
                // from, which is not here, so it is counted and left.
                if (!attribute.value.empty() && attribute.value.front() == '$') {
                    ++tally.deferredAttributes;
                    continue;
                }
                attributesByCode[*code][attribute.name] = attribute.value;
            }
        }
    }

    tally.unmatchedCodes.assign(unmatched.begin(), unmatched.end());
    tally.missingDefinitions.assign(missing.begin(), missing.end());
    tally.layersCreated.assign(layersNeeded.begin(), layersNeeded.end());
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
        tally.changed += ids.size();
        transaction->add(cmd::setEntityLayer(std::move(ids), layer));
    }
    for (auto& [style, ids] : byStyle) {
        transaction->add(cmd::setEntityStyle(std::move(ids), style));
    }
    for (const auto& [code, attributes] : attributesByCode) {
        for (const auto& [name, value] : attributes) {
            transaction->add(
                cmd::setEntityProperty(entitiesByCode[code], name, katana::entity::PropertyValue(value)));
        }
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
