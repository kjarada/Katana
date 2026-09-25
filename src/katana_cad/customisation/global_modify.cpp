#include "katana/cad/global_modify.hpp"

#include <algorithm>
#include <cmath>
#include <memory>
#include <utility>

#include "katana/cad/selection.hpp"
#include "katana/cad/style_catalogue.hpp"
#include "katana/cad/symbol_assign.hpp"
#include "katana/commands/change_set.hpp"
#include "katana/commands/command_stack.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/display.hpp"
#include "katana/entity/entity_geometry.hpp"
#include "katana/entity/layer_path.hpp"
#include "katana/entity/tables.hpp"

namespace katana::cad {

namespace {

namespace cmd = katana::commands;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;
using katana::entity::Color;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::entity::EntityType;
using katana::entity::Layer;
using katana::entity::Model;
using katana::entity::Style;
using NameSet = std::set<std::string, std::less<>>;

std::string counted(std::size_t count, const char* one, const char* many)
{
    return std::to_string(count) + " " + (count == 1 ? one : many);
}

// `path` is `layer` or lies beneath it.
bool isWithin(std::string_view path, std::string_view layer)
{
    return path == layer ||
           (path.size() > layer.size() && path.starts_with(layer) && path[layer.size()] == '/');
}

// `path` and each of its ancestors, deepest last: "a/b/c" -> "a", "a/b", "a/b/c".
std::vector<std::string_view> selfAndAncestors(std::string_view path)
{
    std::vector<std::string_view> prefixes;
    for (std::size_t at = path.find('/'); at != std::string_view::npos;
         at = path.find('/', at + 1)) {
        prefixes.push_back(path.substr(0, at));
    }
    prefixes.push_back(path);
    return prefixes;
}

bool isByLayerName(std::string_view name)
{
    return name.empty() || katana::core::equalsIgnoringCase(name, "ByLayer");
}

// What a Text or a Dimension says, for the text filter; nullopt for the rest.
std::optional<std::string_view> textOf(const Entity& entity)
{
    if (const auto* text = std::get_if<katana::entity::TextGeometry>(&entity.geometry)) {
        return std::string_view(text->text);
    }
    if (const auto* dimension = std::get_if<katana::entity::DimensionGeometry>(&entity.geometry)) {
        return std::string_view(dimension->textOverride);
    }
    return std::nullopt;
}

bool passes(const Model& model, const Entity& entity, const ModifyFilter& filter)
{
    if (!filter.types.empty() && !filter.types.contains(entity.type())) {
        return false;
    }
    if (!filter.layers.empty() &&
        std::ranges::none_of(filter.layers, [&](const std::string& pattern) {
            return matchesPattern(entity.layer, pattern);
        })) {
        return false;
    }
    if (filter.style) {
        const bool wantsByLayer = isByLayerName(*filter.style);
        if (wantsByLayer ? !entity.style.empty()
                         : (entity.style.empty() || !matchesPattern(entity.style, *filter.style))) {
            return false;
        }
    }
    if (filter.colour && entity.color != *filter.colour) {
        return false;
    }
    if (filter.property) {
        const auto found = entity.properties.find(*filter.property);
        if (found == entity.properties.end()) {
            return false;
        }
        if (filter.propertyValue &&
            !matchesPattern(katana::entity::toString(found->second), *filter.propertyValue)) {
            return false;
        }
    } else if (filter.propertyValue) {
        // A value with no key: any property carrying it.
        if (std::ranges::none_of(entity.properties, [&](const auto& entry) {
                return matchesPattern(katana::entity::toString(entry.second),
                                      *filter.propertyValue);
            })) {
            return false;
        }
    }
    if (filter.text) {
        const auto text = textOf(entity);
        if (!text || !matchesPattern(*text, *filter.text)) {
            return false;
        }
    }
    if (filter.drawnOnly && !isDrawn(model, entity, kNoLayerOverrides)) {
        return false;
    }
    return true;
}

Status checkPositive(std::string_view field, double value)
{
    if (!(std::isfinite(value) && value > 0.0)) {
        return makeError(ErrorCode::InvalidArgument, std::string(field) + " must be above 0",
                         std::to_string(value));
    }
    return {};
}

Status checkNotNegative(std::string_view field, double value)
{
    if (!(std::isfinite(value) && value >= 0.0)) {
        return makeError(ErrorCode::InvalidArgument, std::string(field) + " must be 0 or more",
                         std::to_string(value));
    }
    return {};
}

Status checkSymbolName(const Document& document, std::string_view name)
{
    // The model accepts any name, since a project may be opened before its
    // library is loaded; a person choosing one is still told about a typo,
    // as STYLE SET SYMBOL tells them. "" is the plain point mark.
    if (name.empty() || katana::entity::isBuiltInSymbolName(name) ||
        document.definitionFor(name) != nullptr) {
        return {};
    }
    return makeError(ErrorCode::NotFound,
                     "no symbol of that name is built in or in the loaded library",
                     std::string(name));
}

// Everything the request says that the drawing would refuse, before any of
// it is planned.
Status checkRequest(const Document& document, const GlobalModify& change)
{
    const Model& model = document.model();
    if (change.empty()) {
        return makeError(ErrorCode::InvalidArgument,
                         "nothing to change: choose at least one field to set");
    }
    const EntityModify& entities = change.entities;
    if (entities.layer) {
        if (auto status = katana::entity::validateLayerPath(*entities.layer); !status) {
            return status;
        }
    }
    if (entities.style && !entities.style->empty() && !model.styles.contains(*entities.style)) {
        return makeError(ErrorCode::NotFound, "style does not exist", *entities.style);
    }
    for (const auto& [key, value] : entities.setProperties) {
        if (key.empty()) {
            return makeError(ErrorCode::InvalidArgument, "a property needs a name");
        }
        if (auto status = model.properties.validate(key, value); !status) {
            return status;
        }
    }
    if (entities.textHeight) {
        if (auto status = checkPositive("text height", *entities.textHeight); !status) {
            return status;
        }
    }
    if (entities.symbol) {
        if (entities.symbol->name.empty()) {
            return makeError(ErrorCode::InvalidArgument, "name the symbol to draw at the points");
        }
        if (auto status = checkSymbolName(document, entities.symbol->name); !status) {
            return status;
        }
        if (auto status = checkNotNegative("symbol size", entities.symbol->size); !status) {
            return status;
        }
    }

    const LayerModify& layers = change.layers;
    if (layers.linetype) {
        if (auto status = checkLinetypeName(document, *layers.linetype); !status) {
            return status;
        }
    }
    if (layers.lineWeight) {
        if (auto status = checkNotNegative("line weight", *layers.lineWeight); !status) {
            return status;
        }
    }
    if (layers.hatchPattern && !model.hatchPatterns.contains(*layers.hatchPattern)) {
        return makeError(ErrorCode::NotFound, "hatch pattern does not exist", *layers.hatchPattern);
    }
    if (layers.dimensionStyle && !layers.dimensionStyle->empty() &&
        !model.dimensionStyles.contains(*layers.dimensionStyle)) {
        return makeError(ErrorCode::NotFound, "dimension style does not exist",
                         *layers.dimensionStyle);
    }

    const StyleModify& styles = change.styles;
    if (styles.linetype && !katana::entity::isByLayer(*styles.linetype)) {
        if (auto status = checkLinetypeName(document, *styles.linetype); !status) {
            return status;
        }
    }
    if (styles.lineWeight) {
        if (auto status = checkNotNegative("line weight", *styles.lineWeight); !status) {
            return status;
        }
    }
    if (styles.hatchPattern && !styles.hatchPattern->empty() &&
        !model.hatchPatterns.contains(*styles.hatchPattern)) {
        return makeError(ErrorCode::NotFound, "hatch pattern does not exist", *styles.hatchPattern);
    }
    if (styles.symbol) {
        if (auto status = checkSymbolName(document, *styles.symbol); !status) {
            return status;
        }
    }
    if (styles.symbolSize) {
        if (auto status = checkNotNegative("symbol size", *styles.symbolSize); !status) {
            return status;
        }
    }
    return {};
}

Layer changedLayer(Layer layer, const LayerModify& change)
{
    if (change.colour) {
        layer.color = *change.colour;
    }
    if (change.linetype) {
        layer.linetype = *change.linetype;
    }
    if (change.lineWeight) {
        layer.lineWeight = *change.lineWeight;
    }
    if (change.hatchPattern) {
        layer.hatchPattern = *change.hatchPattern;
    }
    if (change.dimensionStyle) {
        layer.dimensionStyle = *change.dimensionStyle;
    }
    if (change.visible) {
        layer.visible = *change.visible;
    }
    // A lock waits for the end, after the entities on the layer are changed
    // (see planGlobalModify); an unlock comes first, so they can be.
    if (change.locked && !*change.locked) {
        layer.locked = false;
    }
    return layer;
}

Style changedStyle(Style style, const StyleModify& change)
{
    if (change.colour) {
        style.color = *change.colour;
    }
    if (change.linetype) {
        style.linetype = katana::entity::isByLayer(*change.linetype)
                             ? std::string(katana::entity::kByLayerLinetype)
                             : *change.linetype;
    }
    if (change.lineWeight) {
        style.lineWeight = *change.lineWeight;
    }
    if (change.hatchPattern) {
        style.hatchPattern = *change.hatchPattern;
    }
    if (change.symbol) {
        style.symbol = *change.symbol;
    }
    if (change.symbolSize) {
        style.symbolSize = *change.symbolSize;
    }
    return style;
}

// The entity with the request's own attributes, the symbol aside (that goes
// through a style, after), and how it stands against what the request asks.
struct EntityOutcome {
    Entity entity;
    bool textAsked = false; // the request sets a text height and this is not text
};

EntityOutcome changedEntity(const Entity& original, const EntityModify& change)
{
    EntityOutcome outcome{original};
    Entity& entity = outcome.entity;
    if (change.layer) {
        entity.layer = *change.layer;
    }
    if (change.colour) {
        entity.color = *change.colour;
    }
    if (change.style) {
        entity.style = *change.style;
    }
    if (change.visible) {
        entity.visible = *change.visible;
    }
    for (const std::string& key : change.removeProperties) {
        if (const auto found = entity.properties.find(key); found != entity.properties.end()) {
            entity.properties.erase(found);
        }
    }
    for (const auto& [key, value] : change.setProperties) {
        entity.properties.insert_or_assign(key, value);
    }
    if (change.textHeight) {
        if (auto* text = std::get_if<katana::entity::TextGeometry>(&entity.geometry)) {
            text->height = *change.textHeight;
        } else {
            outcome.textAsked = true;
        }
    }
    return outcome;
}

} // namespace

std::string_view toString(ScopeKind kind)
{
    switch (kind) {
    case ScopeKind::Selection:
        return "selection";
    case ScopeKind::View:
        return "view";
    case ScopeKind::Layers:
        return "layers";
    case ScopeKind::Drawing:
        return "drawing";
    }
    return "selection";
}

bool matchesPattern(std::string_view text, std::string_view pattern)
{
    // The classic two-pointer match with one back-track point: linear in
    // practice, and with no recursion for a hostile "*a*a*a*..." to exhaust.
    std::size_t t = 0;
    std::size_t p = 0;
    std::size_t starAt = std::string_view::npos;
    std::size_t resumeAt = 0;
    while (t < text.size()) {
        if (p < pattern.size() && (pattern[p] == '?' || katana::core::asciiLower(pattern[p]) ==
                                                            katana::core::asciiLower(text[t]))) {
            ++t;
            ++p;
        } else if (p < pattern.size() && pattern[p] == '*') {
            starAt = p++;
            resumeAt = t;
        } else if (starAt != std::string_view::npos) {
            p = starAt + 1;
            t = ++resumeAt;
        } else {
            return false;
        }
    }
    while (p < pattern.size() && pattern[p] == '*') {
        ++p;
    }
    return p == pattern.size();
}

Result<std::vector<EntityId>> matchEntities(const Document& document, const ModifyScope& scope,
                                            const ModifyFilter& filter)
{
    const Model& model = document.model();
    if (scope.kind == ScopeKind::Layers) {
        if (scope.layers.empty()) {
            return makeError(ErrorCode::InvalidArgument, "name at least one layer to change");
        }
        for (const std::string& name : scope.layers) {
            if (!model.layers.contains(name)) {
                return makeError(ErrorCode::NotFound, "layer does not exist", name);
            }
        }
    }

    std::vector<EntityId> matched;
    const auto consider = [&](const Entity& entity) {
        switch (scope.kind) {
        case ScopeKind::Selection:
        case ScopeKind::Drawing:
            break;
        case ScopeKind::View: {
            const LayerOverrides& hidden = scope.view != nullptr ? *scope.view : kNoLayerOverrides;
            if (!isDrawn(model, entity, hidden)) {
                return;
            }
            if (scope.area &&
                !katana::entity::boundingBox(entity.geometry).intersects(*scope.area)) {
                return;
            }
            break;
        }
        case ScopeKind::Layers:
            if (std::ranges::none_of(scope.layers, [&](const std::string& layer) {
                    return scope.sublayers ? isWithin(entity.layer, layer) : entity.layer == layer;
                })) {
                return;
            }
            break;
        }
        if (passes(model, entity, filter)) {
            matched.push_back(entity.id);
        }
    };

    if (scope.kind == ScopeKind::Selection) {
        for (const EntityId id : document.selection().ids()) {
            if (const Entity* entity = model.entities.find(id)) {
                consider(*entity);
            }
        }
    } else {
        model.entities.forEach(consider);
    }
    std::ranges::sort(matched);
    return matched;
}

Result<GlobalModifyPlan> planGlobalModify(const Document& document, const ModifyScope& scope,
                                          const ModifyFilter& filter, const GlobalModify& change)
{
    if (auto status = checkRequest(document, change); !status) {
        return status.error();
    }
    auto matched = matchEntities(document, scope, filter);
    if (!matched) {
        return matched.error();
    }
    const Model& model = document.model();
    GlobalModifyPlan plan;
    plan.matched = std::move(*matched);

    // ---- the layers and styles whose definitions change ------------------------------------
    NameSet targetLayers;
    if (!change.layers.empty()) {
        if (scope.kind == ScopeKind::Layers) {
            for (const std::string& name : scope.layers) {
                if (scope.sublayers) {
                    for (std::string& path : model.layers.subtree(name)) {
                        targetLayers.insert(std::move(path));
                    }
                } else {
                    targetLayers.insert(name);
                }
            }
        } else {
            for (const EntityId id : plan.matched) {
                targetLayers.insert(model.entities.find(id)->layer);
            }
        }
    }
    NameSet targetStyles;
    if (!change.styles.empty()) {
        for (const EntityId id : plan.matched) {
            const std::string& style = model.entities.find(id)->style;
            if (!style.empty() && model.styles.contains(style)) {
                targetStyles.insert(style);
            }
        }
    }

    // Phase 1: the layers (an unlock among them) and the styles.
    std::vector<cmd::CommandPtr> first;
    std::vector<std::string> layersToLock;
    for (const std::string& name : targetLayers) {
        const Layer* layer = model.layers.find(name);
        if (layer == nullptr) {
            continue;
        }
        const Layer changed = changedLayer(*layer, change.layers);
        const bool locks = change.layers.locked && *change.layers.locked && !layer->locked;
        if (changed != *layer || locks) {
            plan.layersChanged.push_back(name);
        }
        if (changed != *layer) {
            first.push_back(cmd::updateLayer(changed));
        }
        if (locks) {
            layersToLock.push_back(name);
        }
    }
    for (const std::string& name : targetStyles) {
        const Style* style = model.styles.find(name);
        if (auto command = cmd::updateStyleIfChanged(model, changedStyle(*style, change.styles))) {
            first.push_back(std::move(command));
            plan.stylesChanged.push_back(name);
        }
    }
    if (!plan.stylesChanged.empty()) {
        const NameSet changedStyles(plan.stylesChanged.begin(), plan.stylesChanged.end());
        model.entities.forEach([&](const Entity& entity) {
            if (changedStyles.contains(entity.style) &&
                !std::ranges::binary_search(plan.matched, entity.id)) {
                ++plan.styleReachesOthers;
            }
        });
    }

    // Whether a layer is locked once phase 1 has run: it or an ancestor is,
    // counting the request's unlock of the target layers.
    const bool unlocks = change.layers.locked && !*change.layers.locked;
    const auto lockedAfterPhaseOne = [&](std::string_view path) {
        for (const std::string_view prefix : selfAndAncestors(path)) {
            if (unlocks && targetLayers.contains(prefix)) {
                continue;
            }
            if (const Layer* layer = model.layers.find(prefix); layer != nullptr && layer->locked) {
                return true;
            }
        }
        return false;
    };

    // ---- the entities --------------------------------------------------------------------
    const EntityModify& wanted = change.entities;
    if (wanted.layer) {
        // A new layer's ancestors are made with it, unlocked, so only an
        // ancestor that already exists can lock it.
        if (lockedAfterPhaseOne(*wanted.layer)) {
            return makeError(ErrorCode::InvalidState,
                             model.layers.contains(*wanted.layer)
                                 ? "the layer to move them onto is locked"
                                 : "the layer to move them onto would sit under a locked layer",
                             *wanted.layer);
        }
    }

    cmd::ChangeSet changes;
    std::vector<EntityId> points;
    for (const EntityId id : plan.matched) {
        const Entity& entity = *model.entities.find(id);
        if (lockedAfterPhaseOne(entity.layer)) {
            ++plan.locked;
            continue;
        }
        EntityOutcome outcome = changedEntity(entity, wanted);
        if (outcome.textAsked) {
            ++plan.notText;
        }
        if (wanted.symbol) {
            if (entity.type() == EntityType::Point) {
                points.push_back(id);
            } else {
                ++plan.notPoints;
            }
        }
        if (outcome.entity != entity) {
            changes.modify.push_back(std::move(outcome.entity));
        }
    }
    // Made only when something moves onto it: a request whose every match
    // was left alone must not leave an empty layer behind.
    if (wanted.layer && !model.layers.contains(*wanted.layer) && !changes.modify.empty()) {
        plan.createsLayer = *wanted.layer;
    }

    // The symbol goes through a style that draws it, found or made
    // (symbol_assign.hpp), asked of the drawing as it is now. Of what the
    // entity step changes, only a style bears on it - and a point given a
    // style as well as a symbol ends in the symbol's style, as the header
    // says.
    cmd::CommandPtr symbolCommand;
    std::vector<EntityId> changed;
    for (const Entity& entity : changes.modify) {
        changed.push_back(entity.id);
    }
    if (!points.empty()) {
        auto assignment =
            assignSymbolToPoints(document, points, wanted.symbol->name, wanted.symbol->size);
        if (!assignment) {
            return assignment.error();
        }
        if (assignment->createsStyle) {
            plan.createsStyle = assignment->style;
        }
        symbolCommand = std::move(assignment->command);
        // A request that also names a style has just moved every point into
        // it, the symbol's style included; all of them are moved back.
        const bool restyled = wanted.style && *wanted.style != assignment->style;
        if (restyled) {
            auto both = std::make_unique<cmd::Transaction>("ASSIGN_SYMBOL");
            if (symbolCommand != nullptr) {
                both->add(std::move(symbolCommand));
            }
            both->add(cmd::setEntityStyle(points, assignment->style));
            symbolCommand = std::move(both);
        }
        for (const EntityId id : points) {
            if (restyled || model.entities.find(id)->style != assignment->style) {
                changed.push_back(id);
            }
        }
    }
    std::ranges::sort(changed);
    changed.erase(std::unique(changed.begin(), changed.end()), changed.end());
    plan.changed = std::move(changed);

    // ---- one command -----------------------------------------------------------------------
    auto transaction = std::make_unique<cmd::Transaction>("GLOBAL_MODIFY");
    for (cmd::CommandPtr& command : first) {
        transaction->add(std::move(command));
    }
    if (plan.createsLayer) {
        Layer layer;
        layer.name = *plan.createsLayer;
        transaction->add(cmd::createLayer(std::move(layer)));
    }
    if (!changes.modify.empty()) {
        transaction->add(std::make_unique<cmd::ChangeSetCommand>(
            "GLOBAL_MODIFY_ENTITIES", [changes = std::move(changes)](const cmd::CommandContext&) {
                return Result<cmd::ChangeSet>(changes);
            }));
    }
    if (symbolCommand != nullptr) {
        transaction->add(std::move(symbolCommand));
    }
    for (const std::string& name : layersToLock) {
        Layer locked = changedLayer(*model.layers.find(name), change.layers);
        locked.locked = true;
        transaction->add(cmd::updateLayer(std::move(locked)));
    }
    if (transaction->size() > 0) {
        plan.command = std::move(transaction);
    }
    return plan;
}

std::string GlobalModifyPlan::summary() const
{
    if (matched.empty()) {
        return "Nothing matched: no entity is in the scope and passes the filter.";
    }
    std::string text = counted(matched.size(), "entity matched", "entities matched");
    // Read from what changes, not from `command`, which is gone once it has
    // been executed and the summary is still wanted for the log.
    if (changed.empty() && layersChanged.empty() && stylesChanged.empty() && !createsLayer &&
        !createsStyle) {
        text += "; nothing to change, they are already as asked";
    } else {
        std::vector<std::string> parts;
        if (!changed.empty()) {
            parts.push_back(counted(changed.size(), "entity", "entities"));
        }
        if (!layersChanged.empty()) {
            parts.push_back(counted(layersChanged.size(), "layer", "layers"));
        }
        if (!stylesChanged.empty()) {
            parts.push_back(counted(stylesChanged.size(), "style", "styles"));
        }
        std::string list;
        for (std::size_t i = 0; i < parts.size(); ++i) {
            list += (i == 0 ? "" : i + 1 == parts.size() ? " and " : ", ") + parts[i];
        }
        text += "; changing " + (list.empty() ? std::string("the drawing's tables") : list);
    }
    if (createsLayer) {
        text += "; makes layer " + *createsLayer;
    }
    if (createsStyle) {
        text += "; makes style " + *createsStyle;
    }
    if (locked > 0) {
        text += "; " + counted(locked, "entity", "entities") + " on a locked layer left as " +
                (locked == 1 ? "it is" : "they are");
    }
    if (notPoints > 0) {
        text += "; " + counted(notPoints, "entity is", "entities are") +
                " not a point and draws no symbol";
    }
    if (notText > 0) {
        text +=
            "; " + counted(notText, "entity is", "entities are") + " not text and has no height";
    }
    if (styleReachesOthers > 0) {
        text += "; the changed styles also redraw " +
                counted(styleReachesOthers, "entity", "entities") + " outside the scope";
    }
    return text + ".";
}

} // namespace katana::cad
