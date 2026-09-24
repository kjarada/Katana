#include "katana/dxf/import_command.hpp"

#include <algorithm>
#include <memory>

#include "katana/commands/command_stack.hpp"
#include "katana/commands/entity_commands.hpp"

namespace katana::dxf {

katana::commands::CommandPtr importCommand(DxfImport& imported, const katana::entity::Model& model)
{
    namespace cmd = katana::commands;
    auto transaction = std::make_unique<cmd::Transaction>("IMPORT");
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
    if (transaction->size() == 0) {
        return nullptr;
    }
    return transaction;
}

} // namespace katana::dxf
