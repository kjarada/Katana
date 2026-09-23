#include "katana/cad/layer_overrides.hpp"

#include <algorithm>

#include "katana/entity/layer_path.hpp"
#include "katana/entity/tables.hpp"

namespace katana::cad {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Status;
using katana::entity::isLayerUnder;
using katana::entity::layerParent;

bool LayerOverrides::hide(std::string_view path)
{
    if (path.empty()) {
        return false;
    }
    return hidden_.emplace(path).second;
}

bool LayerOverrides::show(std::string_view path)
{
    const auto found = hidden_.find(path);
    if (found == hidden_.end()) {
        return false;
    }
    hidden_.erase(found);
    return true;
}

bool LayerOverrides::hides(std::string_view path) const
{
    if (hidden_.empty()) {
        return false;
    }
    for (std::string_view at = path; !at.empty(); at = layerParent(at)) {
        if (hidden_.contains(at)) {
            return true;
        }
    }
    return false;
}

bool LayerOverrides::hidesDirectly(std::string_view path) const
{
    return hidden_.contains(path);
}

Status LayerOverrides::isolate(std::string_view path, const std::vector<std::string>& allPaths)
{
    const bool exists = std::ranges::any_of(
        allPaths, [&](const std::string& name) { return isLayerUnder(name, path); });
    if (path.empty() || !exists) {
        return makeError(ErrorCode::NotFound, "no layer to isolate", std::string(path));
    }

    // Every node of the derived tree, not only the names that have a record:
    // "design" is a node when only "design/surface" is a layer, and hiding it
    // is how its siblings of "keep" get hidden with one entry.
    std::set<std::string, std::less<>> nodes;
    for (const std::string& name : allPaths) {
        for (std::string_view at = name; !at.empty(); at = layerParent(at)) {
            nodes.emplace(at);
        }
    }

    std::set<std::string, std::less<>> next;
    for (const std::string& node : nodes) {
        if (isLayerUnder(node, path) || isLayerUnder(path, node)) {
            continue; // the isolated branch itself, or on the way down to it
        }
        // Hide the highest node off the path: one whose parent IS on the path
        // (an ancestor of it), or a root. Anything deeper is already hidden by
        // that entry.
        const std::string_view parent = layerParent(node);
        if (parent.empty() || isLayerUnder(path, parent)) {
            next.emplace(node);
        }
    }
    hidden_ = std::move(next);
    return {};
}

std::size_t LayerOverrides::pruneMissing(const katana::entity::LayerDatabase& layers)
{
    if (hidden_.empty()) {
        return 0;
    }
    const std::vector<std::string> names = layers.names();
    return std::erase_if(hidden_, [&](const std::string& entry) {
        return std::ranges::none_of(
            names, [&](const std::string& name) { return isLayerUnder(name, entry); });
    });
}

} // namespace katana::cad
