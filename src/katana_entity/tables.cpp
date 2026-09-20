#include "katana/entity/tables.hpp"

#include <cmath>
#include <utility>

namespace katana::entity {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;

namespace {

Status validateName(std::string_view name, const char* what)
{
    if (name.empty()) {
        return makeError(ErrorCode::InvalidArgument, std::string(what) + " name is empty");
    }
    return {};
}

Status validateLineWeight(double lineWeight)
{
    if (!(std::isfinite(lineWeight) && lineWeight >= 0.0)) {
        return makeError(ErrorCode::InvalidArgument, "line weight must be finite and not negative",
                         std::to_string(lineWeight));
    }
    return {};
}

template <typename Table> auto collect(const Table& table)
{
    std::vector<typename Table::mapped_type> values;
    values.reserve(table.size());
    for (const auto& [name, value] : table) {
        values.push_back(value);
    }
    return values;
}

} // namespace

// ---- LayerDatabase ---------------------------------------------------------------

LayerDatabase::LayerDatabase()
{
    reset();
}

void LayerDatabase::reset()
{
    layers_.clear();
    layers_.emplace(std::string(kDefaultLayerName), Layer{});
}

Status LayerDatabase::add(Layer layer)
{
    if (auto status = validateLayerPath(layer.name); !status) {
        return status;
    }
    if (auto status = validateLineWeight(layer.lineWeight); !status) {
        return status;
    }
    if (contains(layer.name)) {
        return makeError(ErrorCode::AlreadyExists, "layer already exists", layer.name);
    }
    // Every ancestor is created as a real layer, so each node in the tree has
    // its own colour, visibility and lock. The alternative - implicit nodes
    // that exist only in the tree widget - gives the user a row they can see
    // and cannot switch off, which is worse than not grouping at all.
    //
    // Validation happened above and covers the ancestors too (a valid path has
    // only valid prefixes), so nothing here can fail partway.
    for (const std::string& ancestor : layerAncestors(layer.name)) {
        layers_.try_emplace(ancestor, Layer{ancestor, Color{}, true, false,
                                            std::string(kContinuousLinetype), 0.25});
    }
    std::string name = layer.name;
    layers_.emplace(std::move(name), std::move(layer));
    return {};
}

Status LayerDatabase::update(const Layer& layer)
{
    const auto found = layers_.find(layer.name);
    if (found == layers_.end()) {
        return makeError(ErrorCode::NotFound, "layer does not exist", layer.name);
    }
    if (auto status = validateLineWeight(layer.lineWeight); !status) {
        return status;
    }
    found->second = layer;
    return {};
}

Result<Layer> LayerDatabase::remove(std::string_view name)
{
    if (name == kDefaultLayerName) {
        return makeError(ErrorCode::InvalidArgument, "the default layer cannot be removed");
    }
    const auto found = layers_.find(name);
    if (found == layers_.end()) {
        return makeError(ErrorCode::NotFound, "layer does not exist", std::string(name));
    }
    if (hasChildren(name)) {
        // Deleting the branch silently, or leaving its children pointing at a
        // parent that is gone, are both worse than saying so.
        return makeError(ErrorCode::InvalidArgument,
                         "layer still has nested layers; remove the subtree instead",
                         std::string(name));
    }
    Layer removed = std::move(found->second);
    layers_.erase(found);
    return removed;
}

const Layer* LayerDatabase::find(std::string_view name) const
{
    const auto found = layers_.find(name);
    return found == layers_.end() ? nullptr : &found->second;
}

std::vector<std::string> LayerDatabase::names() const
{
    std::vector<std::string> result;
    result.reserve(layers_.size());
    for (const auto& [name, layer] : layers_) {
        result.push_back(name);
    }
    return result;
}

std::vector<Layer> LayerDatabase::all() const
{
    return collect(layers_);
}

// ---- the layer tree ------------------------------------------------------------
//
// Every one of these is a range scan rather than a search, because the map is
// keyed by full path and so is already ordered as a pre-order walk of the tree.

Status LayerDatabase::ensure(std::string_view name)
{
    if (auto status = validateLayerPath(name); !status) {
        return status;
    }
    if (contains(name)) {
        return {};
    }
    Layer layer;
    layer.name = std::string(name);
    return add(std::move(layer));
}

std::vector<std::string> LayerDatabase::roots() const { return children({}); }

std::vector<std::string> LayerDatabase::children(std::string_view name) const
{
    std::vector<std::string> out;
    const std::size_t wanted = layerDepth(name) + 1;
    for (const auto& [path, layer] : layers_) {
        (void)layer;
        if (!isLayerUnder(path, name) || path == name) {
            continue;
        }
        if (layerDepth(path) == wanted) {
            out.push_back(path);
        }
    }
    return out;
}

std::vector<std::string> LayerDatabase::subtree(std::string_view name) const
{
    std::vector<std::string> out;
    if (!contains(name)) {
        return out;
    }
    for (const auto& [path, layer] : layers_) {
        (void)layer;
        if (isLayerUnder(path, name)) {
            out.push_back(path);
        }
    }
    return out;
}

bool LayerDatabase::hasChildren(std::string_view name) const
{
    // lower_bound past the node itself: the very next key is a child if any is,
    // because "a/b" sorts immediately after "a" and before any sibling of "a".
    auto it = layers_.upper_bound(name);
    return it != layers_.end() && isLayerUnder(it->first, name);
}

bool LayerDatabase::effectivelyVisible(std::string_view name) const
{
    const Layer* layer = find(name);
    if (layer == nullptr) {
        // A layer that is gone cannot be shown: drawing entities that point at
        // one would be drawing something the user cannot turn off or select
        // through the layer tree.
        return false;
    }
    if (!layer->visible) {
        return false;
    }
    for (const std::string& ancestor : layerAncestors(name)) {
        const Layer* above = find(ancestor);
        if (above != nullptr && !above->visible) {
            return false;
        }
    }
    return true;
}

bool LayerDatabase::effectivelyLocked(std::string_view name) const
{
    const Layer* layer = find(name);
    if (layer == nullptr) {
        return true; // cannot edit what is not there
    }
    if (layer->locked) {
        return true;
    }
    for (const std::string& ancestor : layerAncestors(name)) {
        const Layer* above = find(ancestor);
        if (above != nullptr && above->locked) {
            return true;
        }
    }
    return false;
}

Result<std::vector<Layer>> LayerDatabase::removeSubtree(std::string_view name)
{
    if (name == kDefaultLayerName) {
        return makeError(ErrorCode::InvalidArgument, "the default layer cannot be removed",
                         std::string(name));
    }
    if (!contains(name)) {
        return makeError(ErrorCode::NotFound, "layer does not exist", std::string(name));
    }
    const std::vector<std::string> paths = subtree(name);
    // The default layer can only be a root, so it can never be inside another
    // layer's subtree; asserting it anyway would be dead code.

    std::vector<Layer> removed;
    removed.reserve(paths.size());
    // Deepest first, so a caller replaying the list in reverse recreates
    // parents before children.
    for (auto it = paths.rbegin(); it != paths.rend(); ++it) {
        const auto found = layers_.find(*it);
        if (found == layers_.end()) {
            continue;
        }
        removed.push_back(found->second);
        layers_.erase(found);
    }
    return removed;
}

Result<std::vector<std::pair<std::string, std::string>>>
LayerDatabase::renameSubtree(std::string_view from, std::string_view to)
{
    if (from == kDefaultLayerName) {
        return makeError(ErrorCode::InvalidArgument, "the default layer cannot be renamed",
                         std::string(from));
    }
    if (auto status = validateLayerPath(to); !status) {
        return status.error();
    }
    if (!contains(from)) {
        return makeError(ErrorCode::NotFound, "layer does not exist", std::string(from));
    }
    if (from == to) {
        return std::vector<std::pair<std::string, std::string>>{};
    }
    if (isLayerUnder(to, from)) {
        // "design" -> "design/old" would have to be its own descendant.
        return makeError(ErrorCode::InvalidArgument, "a layer cannot be moved inside itself",
                         std::string(from) + " -> " + std::string(to));
    }

    const std::vector<std::string> paths = subtree(from);
    std::vector<std::pair<std::string, std::string>> mapping;
    mapping.reserve(paths.size());
    for (const std::string& path : paths) {
        mapping.emplace_back(path, rewriteLayerPrefix(path, from, to));
    }

    // Every target is checked BEFORE anything moves, so a collision leaves the
    // tree exactly as it was rather than half renamed.
    for (const auto& [before, after] : mapping) {
        (void)before;
        if (contains(after) && !isLayerUnder(after, from)) {
            return makeError(ErrorCode::AlreadyExists, "a layer of that name already exists",
                             after);
        }
    }
    // The new parents have to exist too, and creating them cannot be allowed to
    // fail halfway either, so they are validated first and added after.
    for (const std::string& ancestor : layerAncestors(to)) {
        if (auto status = validateLayerPath(ancestor); !status) {
            return status.error();
        }
    }

    std::vector<Layer> moved;
    moved.reserve(mapping.size());
    for (const auto& [before, after] : mapping) {
        const auto found = layers_.find(before);
        if (found == layers_.end()) {
            continue;
        }
        Layer layer = found->second;
        layer.name = after;
        moved.push_back(std::move(layer));
    }
    for (const auto& [before, after] : mapping) {
        (void)after;
        layers_.erase(std::string(before));
    }
    for (const std::string& ancestor : layerAncestors(to)) {
        layers_.try_emplace(ancestor, Layer{ancestor, Color{}, true, false,
                                            std::string(kContinuousLinetype), 0.25});
    }
    for (Layer& layer : moved) {
        std::string key = layer.name;
        layers_.insert_or_assign(std::move(key), std::move(layer));
    }
    return mapping;
}

// ---- StyleDatabase ---------------------------------------------------------------

Status StyleDatabase::add(Style style)
{
    if (auto status = validateName(style.name, "style"); !status) {
        return status;
    }
    if (auto status = validateLineWeight(style.lineWeight); !status) {
        return status;
    }
    if (find(style.name) != nullptr) {
        return makeError(ErrorCode::AlreadyExists, "style already exists", style.name);
    }
    std::string name = style.name;
    styles_.emplace(std::move(name), std::move(style));
    return {};
}

Status StyleDatabase::update(const Style& style)
{
    const auto found = styles_.find(style.name);
    if (found == styles_.end()) {
        return makeError(ErrorCode::NotFound, "style does not exist", style.name);
    }
    if (auto status = validateLineWeight(style.lineWeight); !status) {
        return status;
    }
    found->second = style;
    return {};
}

Result<Style> StyleDatabase::remove(std::string_view name)
{
    const auto found = styles_.find(name);
    if (found == styles_.end()) {
        return makeError(ErrorCode::NotFound, "style does not exist", std::string(name));
    }
    Style removed = std::move(found->second);
    styles_.erase(found);
    return removed;
}

const Style* StyleDatabase::find(std::string_view name) const
{
    const auto found = styles_.find(name);
    return found == styles_.end() ? nullptr : &found->second;
}

std::vector<Style> StyleDatabase::all() const
{
    return collect(styles_);
}

// ---- PropertyDatabase ------------------------------------------------------------

Status PropertyDatabase::define(PropertyDefinition definition)
{
    if (auto status = validateName(definition.name, "property"); !status) {
        return status;
    }
    if (find(definition.name) != nullptr) {
        return makeError(ErrorCode::AlreadyExists, "property is already defined", definition.name);
    }
    if (definition.defaultValue && typeOf(*definition.defaultValue) != definition.type) {
        return makeError(ErrorCode::InvalidArgument,
                         "default value does not match the property type", definition.name);
    }
    std::string name = definition.name;
    definitions_.emplace(std::move(name), std::move(definition));
    return {};
}

Result<PropertyDefinition> PropertyDatabase::undefine(std::string_view name)
{
    const auto found = definitions_.find(name);
    if (found == definitions_.end()) {
        return makeError(ErrorCode::NotFound, "property is not defined", std::string(name));
    }
    PropertyDefinition removed = std::move(found->second);
    definitions_.erase(found);
    return removed;
}

const PropertyDefinition* PropertyDatabase::find(std::string_view name) const
{
    const auto found = definitions_.find(name);
    return found == definitions_.end() ? nullptr : &found->second;
}

std::vector<PropertyDefinition> PropertyDatabase::all() const
{
    return collect(definitions_);
}

Status PropertyDatabase::validate(std::string_view name, const PropertyValue& value) const
{
    const PropertyDefinition* definition = find(name);
    if (definition != nullptr && typeOf(value) != definition->type) {
        return makeError(ErrorCode::InvalidArgument, "property value has the wrong type",
                         std::string(name));
    }
    if (const double* real = std::get_if<double>(&value); real != nullptr && !std::isfinite(*real)) {
        return makeError(ErrorCode::InvalidArgument, "property value is not finite",
                         std::string(name));
    }
    // Strings are persisted through a JSON writer that throws on invalid UTF-8,
    // from a path that returns Status. Rejecting the bytes here - which covers
    // entity properties AND metadata - keeps that impossible.
    if (!isValidUtf8(name)) {
        return makeError(ErrorCode::InvalidArgument, "property name is not valid UTF-8",
                         std::string(name));
    }
    if (const auto* text = std::get_if<std::string>(&value); text != nullptr &&
                                                             !isValidUtf8(*text)) {
        return makeError(ErrorCode::InvalidArgument, "property value is not valid UTF-8",
                         std::string(name));
    }
    return {};
}

} // namespace katana::entity
