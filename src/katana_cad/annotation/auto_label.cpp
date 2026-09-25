#include "katana/cad/annotation/auto_label.hpp"

#include <set>
#include <tuple>
#include <utility>

#include "katana/cad/annotation/label_layout.hpp"
#include "katana/commands/change_set.hpp"
#include "katana/commands/command_stack.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/label_values.hpp"
#include "katana/entity/layer_path.hpp"

namespace katana::cad::annotation {

using katana::commands::ChangeSet;
using katana::commands::CommandPtr;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::entity::Entity;
using katana::entity::LabelGeometry;
using katana::entity::LabelKind;
using katana::entity::LabelRule;
using katana::entity::LabelStyle;

namespace {

// A change set, as a command of its own name.
CommandPtr changeCommand(std::string name, ChangeSet changes, bool destructive = false)
{
    return std::make_unique<katana::commands::ChangeSetCommand>(
        std::move(name),
        [changes = std::move(changes)](const katana::commands::CommandContext&)
            -> Result<ChangeSet> { return changes; },
        destructive);
}

// Whether a label with these pieces says anything at all.
bool saysSomething(const LabelGeometry& label, const LabelStyle& style,
                   const std::vector<katana::entity::LabelPiece>& pieces)
{
    for (const auto& piece : pieces) {
        if (!piece.tickOnly && !labelText(label, style, piece).empty()) {
            return true;
        }
    }
    return false;
}

// The label a rule puts on a target, before it has an id.
Entity ruleLabel(const LabelRule& rule, const LabelStyle& style, const Entity* target,
                 const std::string& alignment, const katana::geometry::Point2& anchor)
{
    Entity entity;
    LabelGeometry label;
    label.target = target != nullptr ? target->id : 0;
    label.alignment = alignment;
    label.style = style.name;
    label.anchor = anchor;
    label.rule = rule.name;
    entity.geometry = std::move(label);
    entity.layer = !rule.labelLayer.empty()
                       ? rule.labelLayer
                       : (target != nullptr ? target->layer
                                            : std::string(katana::entity::kDefaultLayerName));
    return entity;
}

// A label's identity for a re-run: what it labels, how, and which rule made it.
using LabelKey = std::tuple<katana::entity::EntityId, std::string, std::int32_t, std::string,
                            std::string>;

LabelKey keyOf(const LabelGeometry& label)
{
    return {label.target, label.alignment, label.part, label.style, label.rule};
}

} // namespace

bool ruleMatches(const LabelRule& rule, const LabelStyle& style, const Entity& entity)
{
    if (!rule.enabled) {
        return false;
    }
    if (!katana::entity::labels(style.kind, entity.geometry)) {
        return false;
    }
    if (!rule.entityType.empty() &&
        !katana::entity::globMatches(rule.entityType, katana::entity::toString(entity.type()))) {
        return false;
    }
    if (!rule.layer.empty()) {
        // The layer or any ancestor: "survey" matches survey/points/EP, as
        // switching "survey" off would.
        bool matched = katana::entity::globMatches(rule.layer, entity.layer);
        for (const std::string& ancestor : katana::entity::layerAncestors(entity.layer)) {
            matched = matched || katana::entity::globMatches(rule.layer, ancestor);
        }
        if (!matched) {
            return false;
        }
    }
    if (!rule.code.empty()) {
        const std::string code = surveyCode(entity);
        if (code.empty() || !katana::entity::globMatches(rule.code, code)) {
            return false;
        }
    }
    return true;
}

Result<CommandPtr> createLabel(const katana::entity::Model& model, const LabelRequest& request)
{
    const LabelStyle* style = model.labelStyles.find(request.style);
    if (style == nullptr) {
        return makeError(ErrorCode::NotFound, "label style does not exist", request.style);
    }
    LabelGeometry label;
    label.style = style->name;
    label.part = request.part;
    label.position = request.position;
    label.textOverride = request.textOverride;
    std::string layer = request.layer;
    if (style->kind == LabelKind::Chainage) {
        if (request.alignment.empty() || !model.alignments.contains(request.alignment)) {
            return makeError(ErrorCode::NotFound, "a chainage label needs an alignment",
                             request.alignment);
        }
        label.alignment = request.alignment;
        if (layer.empty()) {
            layer = std::string(katana::entity::kDefaultLayerName);
        }
    } else {
        const Entity* target = model.entities.find(request.target);
        if (target == nullptr) {
            return makeError(ErrorCode::NotFound, "entity does not exist",
                             "id=" + std::to_string(request.target));
        }
        if (!katana::entity::labels(style->kind, target->geometry)) {
            return makeError(ErrorCode::InvalidArgument,
                             "a " + std::string(katana::entity::toString(style->kind)) +
                                 " label style cannot label a " +
                                 std::string(katana::entity::toString(target->type())),
                             "id=" + std::to_string(request.target));
        }
        label.target = target->id;
        if (layer.empty()) {
            layer = target->layer;
        }
    }
    const auto pieces = labelPiecesOf(model, label, *style);
    if (pieces.empty()) {
        return makeError(ErrorCode::InvalidArgument, "the target has no such part to label",
                         "part=" + std::to_string(request.part));
    }
    if (!saysSomething(label, *style, pieces)) {
        return makeError(ErrorCode::InvalidArgument,
                         "the label would say nothing: the target has none of the values its "
                         "template asks for",
                         style->text);
    }
    const auto anchor = katana::entity::labelAnchor(model, label, *style);
    label.anchor = anchor.value_or(pieces.front().anchor);
    Entity entity;
    entity.geometry = std::move(label);
    entity.layer = layer;
    ChangeSet changes;
    changes.add.push_back(std::move(entity));
    return changeCommand("CREATE_LABEL", std::move(changes));
}

Result<CommandPtr> createLabels(const katana::entity::Model& model,
                                const std::vector<LabelRequest>& requests)
{
    if (requests.empty()) {
        return makeError(ErrorCode::InvalidArgument, "there is nothing to label");
    }
    auto transaction = std::make_unique<katana::commands::Transaction>("CREATE_LABEL");
    for (const LabelRequest& one : requests) {
        auto built = createLabel(model, one);
        if (!built) {
            return makeError(built.error().code, built.error().message,
                             (one.target != 0 ? "id=" + std::to_string(one.target)
                                              : "alignment=" + one.alignment) +
                                 " " + built.error().context);
        }
        transaction->add(std::move(*built));
    }
    return CommandPtr(std::move(transaction));
}

Result<CommandPtr> autoLabel(const katana::entity::Model& model,
                             const std::vector<std::string>& ruleNames, AutoLabelReport* report)
{
    AutoLabelReport local;
    AutoLabelReport& out = report != nullptr ? *report : local;
    out = AutoLabelReport{};

    // The rules to run, in name order, and their styles.
    std::vector<std::pair<const LabelRule*, const LabelStyle*>> rules;
    const auto addRule = [&](const LabelRule& rule) -> katana::core::Status {
        const LabelStyle* style = model.labelStyles.find(rule.labelStyle);
        if (style == nullptr) {
            return makeError(ErrorCode::NotFound, "the rule's label style does not exist",
                             "rule=" + rule.name + " style=" + rule.labelStyle);
        }
        rules.emplace_back(&rule, style);
        return {};
    };
    if (ruleNames.empty()) {
        std::vector<const LabelRule*> all;
        model.labelRules.forEach([&](const LabelRule& rule) {
            if (rule.enabled) {
                all.push_back(&rule);
            }
        });
        for (const LabelRule* rule : all) {
            if (auto status = addRule(*rule); !status) {
                return status.error();
            }
        }
    } else {
        for (const std::string& name : ruleNames) {
            const LabelRule* rule = model.labelRules.find(name);
            if (rule == nullptr) {
                return makeError(ErrorCode::NotFound, "label rule does not exist", name);
            }
            if (auto status = addRule(*rule); !status) {
                return status.error();
            }
        }
    }
    if (rules.empty()) {
        return makeError(ErrorCode::InvalidState, "there is no enabled label rule to run");
    }
    std::set<std::string> running;
    for (const auto& [rule, style] : rules) {
        running.insert(rule->name);
    }

    // What the rules made before, by identity.
    std::map<LabelKey, katana::entity::EntityId> existing;
    model.entities.forEach([&](const Entity& entity) {
        if (const auto* label = std::get_if<LabelGeometry>(&entity.geometry);
            label != nullptr && running.contains(label->rule)) {
            existing.emplace(keyOf(*label), entity.id);
        }
    });

    // What they ask for now.
    ChangeSet changes;
    std::set<LabelKey> wanted;
    std::set<std::string> layersNeeded;
    const auto want = [&](const LabelRule& rule, const LabelStyle& style, const Entity* target,
                          const std::string& alignment) {
        LabelGeometry probe;
        probe.target = target != nullptr ? target->id : 0;
        probe.alignment = alignment;
        probe.style = style.name;
        probe.rule = rule.name;
        const LabelKey key = keyOf(probe);
        if (wanted.contains(key)) {
            return;
        }
        const auto pieces = labelPiecesOf(model, probe, style);
        if (pieces.empty() || !saysSomething(probe, style, pieces)) {
            ++out.skipped;
            return;
        }
        Entity entity = ruleLabel(rule, style, target, alignment,
                                  katana::entity::labelAnchor(model, probe, style)
                                      .value_or(pieces.front().anchor));
        if (const auto* layer = model.layers.find(entity.layer); layer != nullptr) {
            if (model.layers.resolve(entity.layer).locked) {
                ++out.skipped;
                return;
            }
        } else {
            layersNeeded.insert(entity.layer);
        }
        wanted.insert(key);
        ++out.perRule[rule.name];
        if (existing.contains(key)) {
            ++out.kept;
            return;
        }
        changes.add.push_back(std::move(entity));
        ++out.created;
    };
    for (const auto& [rule, style] : rules) {
        if (style->kind == LabelKind::Chainage) {
            model.alignments.forEach([&](const katana::entity::Alignment& alignment) {
                // An alignment has no layer or code; a rule's type filter,
                // when it has one, must say Alignment.
                if (rule->entityType.empty() ||
                    katana::entity::globMatches(rule->entityType, "Alignment")) {
                    want(*rule, *style, nullptr, alignment.name);
                }
            });
            continue;
        }
        model.entities.forEach([&](const Entity& entity) {
            if (std::holds_alternative<LabelGeometry>(entity.geometry) ||
                !ruleMatches(*rule, *style, entity)) {
                return;
            }
            want(*rule, *style, &entity, {});
        });
    }
    for (const auto& [key, id] : existing) {
        if (!wanted.contains(key)) {
            const Entity* entity = model.entities.find(id);
            if (entity != nullptr && !model.layers.resolve(entity->layer).locked) {
                changes.remove.push_back(id);
                ++out.removed;
            }
        }
    }
    if (changes.empty()) {
        return CommandPtr{};
    }
    if (layersNeeded.empty()) {
        return changeCommand("AUTOLABEL", std::move(changes));
    }
    // A rule's label layer that does not exist yet, made in the same step.
    auto transaction = std::make_unique<katana::commands::Transaction>("AUTOLABEL");
    for (const std::string& name : layersNeeded) {
        katana::entity::Layer layer;
        layer.name = name;
        transaction->add(katana::commands::createLayer(std::move(layer)));
    }
    transaction->add(changeCommand("AUTOLABEL", std::move(changes)));
    return CommandPtr(std::move(transaction));
}

Result<CommandPtr> clearAutoLabels(const katana::entity::Model& model,
                                   const std::vector<std::string>& ruleNames, std::size_t* removed)
{
    const std::set<std::string> named(ruleNames.begin(), ruleNames.end());
    for (const std::string& name : named) {
        if (!model.labelRules.contains(name)) {
            // A rule that was deleted may still have labels; they are asked
            // for by the name they carry, so a missing rule is not an error
            // unless it also has none.
            bool any = false;
            model.entities.forEach([&](const Entity& entity) {
                if (const auto* label = std::get_if<LabelGeometry>(&entity.geometry)) {
                    any = any || label->rule == name;
                }
            });
            if (!any) {
                return makeError(ErrorCode::NotFound, "no label rule or rule labels of that name",
                                 name);
            }
        }
    }
    ChangeSet changes;
    model.entities.forEach([&](const Entity& entity) {
        const auto* label = std::get_if<LabelGeometry>(&entity.geometry);
        if (label == nullptr || label->rule.empty()) {
            return;
        }
        if ((named.empty() || named.contains(label->rule)) &&
            !model.layers.resolve(entity.layer).locked) {
            changes.remove.push_back(entity.id);
        }
    });
    if (removed != nullptr) {
        *removed = changes.remove.size();
    }
    if (changes.empty()) {
        return CommandPtr{};
    }
    return changeCommand("AUTOLABEL_CLEAR", std::move(changes), true);
}

std::vector<LabelStyle> defaultLabelStyles()
{
    std::vector<LabelStyle> styles;
    const auto add = [&](std::string name, LabelKind kind, std::string text,
                         auto&& adjust) {
        LabelStyle style;
        style.name = std::move(name);
        style.kind = kind;
        style.text = std::move(text);
        adjust(style);
        styles.push_back(std::move(style));
    };
    // A point's number, beside it.
    add("Point Number", LabelKind::Point, "{point}", [](LabelStyle&) {});
    // A spot level: a cross at the point and its level to 3 decimals, as a
    // survey plan shows one.
    add("Spot Level", LabelKind::Point, "{z:.3f}", [](LabelStyle& s) {
        s.marker = katana::entity::LabelMarker::Cross;
        s.markerSize = 1.5;
        s.placement = katana::entity::LabelPlacement::Right;
    });
    // Number, code and level, a line each.
    add("Point Details", LabelKind::Point, "{point}\n{code}\nRL {z:.3f}", [](LabelStyle&) {});
    // A boundary's bearing in whole seconds and its distance to the
    // millimetre, along the line - the form a survey plan carries.
    add("Bearing Distance", LabelKind::Segment, "{bearing:dms} {distance:.3f}",
        [](LabelStyle& s) { s.minimumLength = 5.0; });
    add("Arc Data", LabelKind::Arc, "R {radius:.3f}\nA {length:.3f}", [](LabelStyle&) {});
    // A lot's area in square metres, and in hectares on a second line.
    add("Lot Area", LabelKind::Area, "{area:m2:.1f} m²\n{area:ha:.4f} ha", [](LabelStyle& s) {
        s.placement = katana::entity::LabelPlacement::Centroid;
        s.priority = 10;
    });
    // Chainages every 20 m with a tick every 10 m, across the line.
    add("Chainage", LabelKind::Chainage, "{chainage:ch}", [](LabelStyle& s) {
        s.interval = 20.0;
        s.tickInterval = 10.0;
        s.priority = 5;
    });
    return styles;
}

} // namespace katana::cad::annotation
