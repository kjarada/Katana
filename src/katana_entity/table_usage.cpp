#include "katana/entity/table_usage.hpp"

#include <algorithm>
#include <utility>

#include "katana/entity/display.hpp"

namespace katana::entity {

namespace {

void countEntity(Users& users, EntityId id, bool keepIds)
{
    if (users.entities == 0 || id < users.firstEntity) {
        users.firstEntity = id;
    }
    ++users.entities;
    if (keepIds) {
        users.entityIds.push_back(id);
    }
}

std::string counted(std::size_t count, const char* one, const char* many)
{
    return std::to_string(count) + " " + (count == 1 ? one : many);
}

Users& usersOf(UsageMap& map, std::string_view name)
{
    if (const auto found = map.find(name); found != map.end()) {
        return found->second;
    }
    return map.emplace(std::string(name), Users{}).first->second;
}

// Where one (layer, style) pair's entities are counted. Entities come in
// long runs of the same pair - a 12d import puts thousands of points on one
// layer in one style - so the five lookups per entity become one.
struct Slot {
    Users* layer = nullptr;
    Users* style = nullptr; // null when the entity names no style
    Users* linetype = nullptr;
    Users* hatch = nullptr;
    Users* symbol = nullptr; // null when its style draws no symbol
};

} // namespace

std::string Users::describe() const
{
    std::vector<std::string> parts;
    if (!layers.empty()) {
        parts.push_back(counted(layers.size(), "layer", "layers"));
    }
    if (!styles.empty()) {
        parts.push_back(counted(styles.size(), "style", "styles"));
    }
    if (entities > 0) {
        parts.push_back(counted(entities, "entity", "entities"));
    }
    if (parts.empty()) {
        return {};
    }
    std::string text = "used by ";
    for (std::size_t i = 0; i < parts.size(); ++i) {
        if (i > 0) {
            text += i + 1 == parts.size() ? " and " : ", ";
        }
        text += parts[i];
    }
    text += ", e.g. ";
    if (!layers.empty()) {
        text += "layer=" + layers.front();
    } else if (!styles.empty()) {
        text += "style=" + styles.front();
    } else {
        text += "id=" + std::to_string(firstEntity);
    }
    return text;
}

const Users& TableUsage::of(const UsageMap& map, std::string_view name)
{
    static const Users kNobody{};
    const auto found = map.find(name);
    return found == map.end() ? kNobody : found->second;
}

TableUsage tableUsage(const Model& model, UsageOptions options)
{
    TableUsage usage;

    // Every defined item is present, used or not: a purge is looking for
    // exactly the ones with nobody.
    model.styles.forEach([&](const Style& style) { usage.styles.try_emplace(style.name); });
    model.linetypes.forEach(
        [&](const Linetype& linetype) { usage.linetypes.try_emplace(linetype.name); });
    model.hatchPatterns.forEach(
        [&](const HatchPattern& pattern) { usage.hatchPatterns.try_emplace(pattern.name); });
    const std::vector<std::string> layerNames = model.layers.names();
    for (const std::string& name : layerNames) {
        usage.layers.try_emplace(name);
    }

    // Who names what. Walked in name order, so every holder list comes out
    // ascending with no sort.
    for (const std::string& name : layerNames) {
        const Layer* layer = model.layers.find(name);
        if (layer == nullptr) {
            continue;
        }
        if (!isByLayer(layer->linetype)) {
            usersOf(usage.linetypes, layer->linetype).layers.push_back(name);
        }
        if (!layer->hatchPattern.empty()) {
            usersOf(usage.hatchPatterns, layer->hatchPattern).layers.push_back(name);
        }
    }
    model.styles.forEach([&](const Style& style) {
        if (!isByLayer(style.linetype)) {
            usersOf(usage.linetypes, style.linetype).styles.push_back(style.name);
        }
        if (!style.hatchPattern.empty()) {
            usersOf(usage.hatchPatterns, style.hatchPattern).styles.push_back(style.name);
        }
        if (!style.symbol.empty()) {
            usersOf(usage.symbols, style.symbol).styles.push_back(style.name);
        }
    });

    // The one pass over the entities. The views in the key point into the
    // entities themselves, which the const model keeps alive for the pass.
    std::map<std::pair<std::string_view, std::string_view>, Slot> slots;
    model.entities.forEach([&](const Entity& entity) {
        const auto key = std::pair<std::string_view, std::string_view>(entity.layer, entity.style);
        auto found = slots.find(key);
        if (found == slots.end()) {
            const Layer* layer = model.layers.find(entity.layer);
            const Style* style = entity.style.empty() ? nullptr : model.styles.find(entity.style);
            Slot made;
            made.layer = &usersOf(usage.layers, entity.layer);
            // A style an entity names but the table lacks is still counted
            // under its name - the manager has to be able to show it - but
            // it draws, and so reaches, nothing of its own.
            made.style = entity.style.empty() ? nullptr : &usersOf(usage.styles, entity.style);
            made.linetype = &usersOf(usage.linetypes, resolvedLinetype(layer, style));
            made.hatch = &usersOf(usage.hatchPatterns, resolvedHatchPattern(layer, style));
            made.symbol = style == nullptr || style->symbol.empty()
                              ? nullptr
                              : &usersOf(usage.symbols, style->symbol);
            found = slots.emplace(key, made).first;
        }
        const Slot& target = found->second;
        for (Users* users :
             {target.layer, target.style, target.linetype, target.hatch, target.symbol}) {
            if (users != nullptr) {
                countEntity(*users, entity.id, options.entityIds);
            }
        }
    });

    if (options.entityIds) {
        // forEach walks in id order today; sorting keeps "ascending" true
        // whatever the database's order becomes.
        for (UsageMap* map : {&usage.styles, &usage.linetypes, &usage.symbols,
                              &usage.hatchPatterns, &usage.layers}) {
            for (auto& [name, users] : *map) {
                std::sort(users.entityIds.begin(), users.entityIds.end());
            }
        }
    }
    return usage;
}

} // namespace katana::entity
