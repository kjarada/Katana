#include "katana/dxf/import_command.hpp"

#include <algorithm>
#include <memory>
#include <set>
#include <string>

#include "katana/commands/command_stack.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/layer_path.hpp"

namespace katana::dxf {

katana::commands::CommandPtr importCommand(DxfImport& imported, const katana::entity::Model& model)
{
    namespace cmd = katana::commands;
    auto transaction = std::make_unique<cmd::Transaction>("IMPORT");
    // The layers the model already has that would refuse the new entities:
    // locked themselves, or the parent of a layer the entities land on. A
    // second import of the same file lands on the layers the first one
    // locked, and a sheet that shares a locked layer with the last is the
    // same case, so the lock is lifted for the entities and put back after
    // them - the model's lock, not the file's, since the layer is the
    // model's. Ordered, so that the step is the same every time.
    std::set<std::string> receiving;
    for (const katana::entity::Entity& entity : imported.entities) {
        receiving.insert(entity.layer);
    }
    std::set<std::string> holding;
    for (const std::string& name : receiving) {
        std::vector<std::string> lineage = katana::entity::layerAncestors(name);
        lineage.push_back(name);
        for (const std::string& layer : lineage) {
            if (const auto* existing = model.layers.find(layer);
                existing != nullptr && existing->locked) {
                holding.insert(layer);
            }
        }
    }
    std::vector<katana::entity::Layer> toRelock;
    for (const std::string& name : holding) {
        katana::entity::Layer open = *model.layers.find(name);
        toRelock.push_back(open);
        open.locked = false;
        transaction->add(cmd::updateLayer(std::move(open)));
    }
    for (const katana::entity::Linetype& linetype : imported.linetypes) {
        if (!model.linetypes.contains(linetype.name)) {
            transaction->add(cmd::createLinetype(linetype));
        }
    }
    // By name, so that "a" comes before "a/b": adding "a/b" first would make
    // "a" with the defaults, and adding "a" after it would then fail.
    std::vector<katana::entity::Layer> layers = imported.layers;
    std::sort(layers.begin(), layers.end(),
              [](const auto& a, const auto& b) { return a.name < b.name; });
    std::vector<katana::entity::Layer> toLock;
    for (katana::entity::Layer layer : layers) {
        if (model.layers.contains(layer.name)) {
            continue;
        }
        if (layer.locked) {
            toLock.push_back(layer);
            layer.locked = false;
        }
        transaction->add(cmd::createLayer(std::move(layer)));
    }
    if (!imported.entities.empty()) {
        transaction->add(cmd::createEntities(std::move(imported.entities)));
        imported.entities.clear();
    }
    for (katana::entity::Layer& layer : toLock) {
        transaction->add(cmd::updateLayer(std::move(layer)));
    }
    for (katana::entity::Layer& layer : toRelock) {
        transaction->add(cmd::updateLayer(std::move(layer)));
    }
    if (transaction->size() == 0) {
        return nullptr;
    }
    return transaction;
}

} // namespace katana::dxf
