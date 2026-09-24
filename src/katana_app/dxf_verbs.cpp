#include "dxf_verbs.hpp"

#include <algorithm>
#include <filesystem>
#include <iostream>
#include <memory>

#include "katana/commands/command_stack.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/dxf/reader.hpp"
#include "katana/dxf/writer.hpp"

#if defined(KATANA_WITH_INTEROP)
#include "katana/interop/import.hpp"
#endif

namespace katana::app {

namespace {

namespace cmd = katana::commands;

bool importDxf(katana::cad::Document& document, const std::filesystem::path& file, bool local)
{
    katana::dxf::ImportOptions options;
    options.sourceName = file.filename().string();
    auto imported = katana::dxf::readDxfFile(file, options);
    if (imported && local && !imported->bounds.empty()) {
        // LOCAL: moved as one piece to sit at the origin, as the other
        // importers do. Read again rather than moved afterwards, so the one
        // reader applies the one shift to every kind of geometry.
        options.originShift =
            katana::geometry::Vec2(imported->bounds.min.x, imported->bounds.min.y);
        imported = katana::dxf::readDxfFile(file, options);
    }
    if (!imported) {
        std::cerr << "error: " << imported.error().describe() << '\n';
        return false;
    }
    const katana::entity::Model& model = document.model();
    // Linetypes before the layers that name them, parents before children,
    // and all of it with the entities as ONE undo step.
    auto transaction = std::make_unique<cmd::Transaction>("IMPORT");
    for (const katana::entity::Linetype& linetype : imported->linetypes) {
        if (!model.linetypes.contains(linetype.name)) {
            transaction->add(cmd::createLinetype(linetype));
        }
    }
    std::vector<katana::entity::Layer> layers = imported->layers;
    std::sort(layers.begin(), layers.end(),
              [](const auto& a, const auto& b) { return a.name < b.name; });
    for (const katana::entity::Layer& layer : layers) {
        if (!model.layers.contains(layer.name)) {
            transaction->add(cmd::createLayer(layer));
        }
    }
    const std::size_t count = imported->entities.size();
#if defined(KATANA_WITH_INTEROP)
    const auto existingBounds = model.entities.bounds();
#endif
    if (count != 0) {
        transaction->add(cmd::createEntities(std::move(imported->entities)));
    }
    if (const auto status = document.execute(std::move(transaction)); !status) {
        std::cerr << "error: " << status.error().describe() << '\n';
        return false;
    }
    std::cout << "imported " << count << " entities from " << file.filename().string()
              << " (DXF" << (imported->release.empty() ? "" : " ") << imported->release << ")\n";
    for (const auto& tally : imported->tally) {
        std::cout << "  " << tally.kind << ": " << tally.read << " read, " << tally.imported
                  << " imported\n";
    }
    for (const std::string& warning : imported->warnings) {
        std::cout << "  " << warning << '\n';
    }
    const auto& bounds = imported->bounds;
    if (!bounds.empty()) {
        std::cout << "  extent " << bounds.min.x << "," << bounds.min.y << " to " << bounds.max.x
                  << "," << bounds.max.y << '\n';
    }
#if defined(KATANA_WITH_INTEROP)
    // Said out loud, as for every other import: a drawing that has become a
    // dot beside survey coordinates is otherwise found by zooming.
    const auto advice = katana::interop::advisePlacement(existingBounds, bounds);
    if (advice.farApart) {
        std::cout << "  WARNING: " << advice.message << '\n'
                  << "  undo, then re-import with  IMPORT <file> LOCAL  to shift it "
                     "alongside the drawing\n";
    }
#endif
    return true;
}

bool exportDxf(const katana::cad::Document& document, const std::filesystem::path& file)
{
    const auto written = katana::dxf::writeDxfFile(document.model(), file);
    if (!written) {
        std::cerr << "error: " << written.error().describe() << '\n';
        return false;
    }
    std::cout << "exported " << written->entitiesWritten << " entities and "
              << written->layersWritten << " layers (DXF R2000, " << written->bytesWritten
              << " bytes)\n";
    for (const std::string& warning : written->warnings) {
        std::cout << "  " << warning << '\n';
    }
    return true;
}

std::string unquoted(std::string text)
{
    if (text.size() >= 2 && text.front() == '"' && text.back() == '"') {
        return text.substr(1, text.size() - 2);
    }
    return text;
}

} // namespace

std::optional<bool> runDxfVerb(katana::cad::Document& document, std::string_view verb,
                               std::string argument)
{
    bool local = false;
    if (verb == "IMPORT") {
        // IMPORT <file> LOCAL: the last word, in any case.
        const std::size_t space = argument.find_last_of(" \t");
        if (space != std::string::npos) {
            std::string tail = argument.substr(space + 1);
            std::transform(tail.begin(), tail.end(), tail.begin(), [](unsigned char c) {
                return static_cast<char>(std::toupper(c));
            });
            if (tail == "LOCAL") {
                local = true;
                argument = argument.substr(0, argument.find_last_not_of(" \t", space) + 1);
            }
        }
    }
    const std::filesystem::path path(unquoted(argument));
    if (argument.empty() || !katana::dxf::isDxfPath(path)) {
        return std::nullopt;
    }
    return verb == "IMPORT" ? importDxf(document, path, local) : exportDxf(document, path);
}

} // namespace katana::app
