#include "dxf_verbs.hpp"

#include <algorithm>
#include <filesystem>
#include <iostream>
#include <utility>
#include <variant>
#include <vector>

#include "import_records.hpp"
#include "katana/cad/annotation/export_annotation.hpp"
#include "katana/cad/annotation/leader_presets.hpp"
#include "katana/dxf/import_command.hpp"

namespace katana::app {

namespace {

// A smart leader that came out with no note at all - the entity its tip is on
// is not in the drawing, or has none of the values its template names - is
// written as a bare arrow. The file is what it is; the export says so rather
// than leave a callout silently blank. `only`: just those entities' leaders.
std::vector<std::string> blankLeaders(const katana::entity::Model& model,
                                      const katana::cad::annotation::DrawnAnnotation& drawn,
                                      const std::set<katana::entity::EntityId>* only)
{
    std::vector<std::string> said;
    model.entities.forEach([&](const katana::entity::Entity& entity) {
        const auto* leader = std::get_if<katana::entity::LeaderGeometry>(&entity.geometry);
        if (leader == nullptr || !katana::entity::isSmart(*leader) ||
            (only != nullptr && !only->contains(entity.id))) {
            return;
        }
        const auto found = drawn.find(entity.id);
        const bool hasNote =
            found != drawn.end() &&
            std::ranges::any_of(found->second, [](const katana::entity::Geometry& shape) {
                return std::holds_alternative<katana::entity::TextGeometry>(shape);
            });
        if (!hasNote) {
            said.push_back("leader #" + std::to_string(entity.id) +
                           " is written without its note: the entity its tip is on is not in the "
                           "drawing, or has none of the values its template names");
        }
    });
    return said;
}

} // namespace

katana::core::Result<katana::dxf::DxfImport> readDxfImport(const std::filesystem::path& file,
                                                           std::optional<katana::geometry::Vec2> shift)
{
    katana::dxf::ImportOptions options;
    options.sourceName = pathText(file.filename());
    options.originShift = shift;
    return katana::dxf::readDxfFile(file, options);
}

katana::core::Result<std::string> applyDxfImport(katana::cad::Document& document,
                                                 katana::dxf::DxfImport&& imported,
                                                 const std::filesystem::path& file,
                                                 const katana::cad::ImportPlacement& placement,
                                                 const katana::cad::ImportShift& placed)
{
    const std::size_t count = imported.entities.size();
    const std::size_t layers = imported.layers.size();
    const std::size_t linetypes = imported.linetypes.size();
    // Linetypes, layers and entities: ONE undo step (dxf/import_command.hpp).
    // Null when the file brings nothing the drawing lacks, which is not a
    // failure: the file was read, and its tally says what it held.
    if (auto command = katana::dxf::importCommand(imported, document.model())) {
        if (auto status = document.execute(std::move(command)); !status) {
            return status.error();
        }
    }
    const std::string format =
        imported.release.empty() ? std::string("DXF") : "DXF " + imported.release;
    std::string reply = "imported file=" + recordText(pathText(file)) + " kind=dxf format=" +
                        recordText(format) + " entities=" + std::to_string(count) +
                        " layers=" + std::to_string(layers) +
                        " linetypes=" + std::to_string(linetypes) +
                        " bounds=" + boundsText(imported.bounds);
    if (const std::string record = placedRecord(placement, placed); !record.empty()) {
        reply += "\n" + record;
    }
    for (const katana::dxf::EntityTally& tally : imported.tally) {
        reply += "\n" + tallyRecord(tally.kind, tally.read, tally.imported);
    }
    for (const std::string& warning : imported.warnings) {
        reply += "\n" + warningText(warning);
    }
    return reply;
}

katana::core::Result<katana::dxf::DxfExport>
writeDxfExport(const katana::entity::Model& model, double annotationScale,
               const std::filesystem::path& file, const katana::entity::LeaderContext& leaders)
{
    // Labels and every dimension kind drawn out as the plan view draws them,
    // at the drawing's annotation scale (cad/annotation/export_annotation.hpp).
    katana::dxf::ExportOptions options;
    options.annotationScale = annotationScale;
    const auto drawn = katana::cad::annotation::drawAnnotationForExport(
        model, annotationScale, katana::cad::annotation::estimatedMeasure(), leaders);
    options.drawn = &drawn;
    auto written = katana::dxf::writeDxfFile(model, file, options);
    if (written) {
        for (std::string& blank : blankLeaders(model, drawn, nullptr)) {
            written->warnings.push_back(std::move(blank));
        }
    }
    return written;
}

katana::core::Result<katana::dxf::DxfExport>
writeDxfExport(katana::entity::Model& model, double annotationScale, const std::filesystem::path& file,
               const katana::entity::LeaderContext& leaders,
               const std::set<katana::entity::EntityId>& only)
{
    katana::dxf::ExportOptions options;
    options.annotationScale = annotationScale;
    auto drawn = katana::cad::annotation::drawAnnotationForExport(
        model, annotationScale, katana::cad::annotation::estimatedMeasure(), leaders);
    // Only then the scope: what it left out is not written, and neither is
    // the drawing of an annotation it left out.
    const std::vector<std::string> blank = blankLeaders(model, drawn, &only);
    std::vector<katana::entity::EntityId> left;
    for (const katana::entity::EntityId id : model.entities.ids()) {
        if (!only.contains(id)) {
            left.push_back(id);
        }
    }
    for (const katana::entity::EntityId id : left) {
        (void)model.entities.remove(id);
        drawn.erase(id);
    }
    options.drawn = &drawn;
    auto written = katana::dxf::writeDxfFile(model, file, options);
    if (written) {
        written->warnings.insert(written->warnings.end(), blank.begin(), blank.end());
    }
    return written;
}

std::string dxfExportRecords(const katana::dxf::DxfExport& written,
                             const std::filesystem::path& file)
{
    // The writer writes R2000, the release every reader of DXF reads.
    std::string reply = "exported file=" + recordText(pathText(file)) +
                        " kind=dxf driver=DXF format=" + recordText("DXF R2000") +
                        " entities=" + std::to_string(written.entitiesWritten) +
                        " skipped=" + std::to_string(written.entitiesSkipped) +
                        " layers=" + std::to_string(written.layersWritten) +
                        " bytes=" + std::to_string(written.bytesWritten);
    for (const std::string& warning : written.warnings) {
        reply += "\n" + warningText(warning);
    }
    return reply;
}

std::optional<bool> runDxfVerb(katana::cad::Document& document, std::string_view verb,
                               const std::string& path,
                               const katana::cad::ImportPlacement& placement)
{
    const std::filesystem::path file = pathFromText(path);
    if (path.empty() || !katana::dxf::isDxfPath(file)) {
        return std::nullopt;
    }
    katana::core::Result<std::string> reply = std::string();
    if (verb == "IMPORT") {
        // Read at its own coordinates, then again with the shift the
        // placement resolves to against the drawing as it was.
        auto imported = readDxfImport(file);
        if (!imported) {
            std::cerr << "error: " << imported.error().describe() << '\n';
            return false;
        }
        const katana::cad::ImportShift placed = katana::cad::resolveImportShift(
            placement, document.model().entities.bounds(), imported->bounds);
        if (placed.shift) {
            imported = readDxfImport(file, placed.shift);
            if (!imported) {
                std::cerr << "error: " << imported.error().describe() << '\n';
                return false;
            }
        }
        reply = applyDxfImport(document, std::move(*imported), file, placement, placed);
    } else {
        auto written = writeDxfExport(document.model(), document.annotationScale(), file,
                                      katana::cad::annotation::leaderContext(document));
        if (!written) {
            std::cerr << "error: " << written.error().describe() << '\n';
            return false;
        }
        reply = dxfExportRecords(*written, file);
    }
    if (!reply) {
        std::cerr << "error: " << reply.error().describe() << '\n';
        return false;
    }
    std::cout << *reply << '\n';
    return true;
}

} // namespace katana::app
