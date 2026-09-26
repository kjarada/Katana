// IMPORT (docs/interop.md, "IMPORT, EXPORT, INFO, REFS and COPC on every front
// end"): a file into the drawing - a DXF natively, vector data and 12d
// archives as entities, rasters and point clouds as reference layers - run as
// the one executor runs every verb. The work reads the file (on a worker in
// the window); the apply adds what it read as ONE undo step and says so in
// records.
//
//   IMPORT <file> [LOCAL | ALONGSIDE | OFFSET=dE,dN]
//
// The readers take no stop: a cancel lands when the read ends, and the job,
// cancelled, never applies - so a cancelled IMPORT imports nothing.

#include <filesystem>
#include <memory>
#include <optional>
#include <string>
#include <utility>
#include <variant>
#include <vector>

#include "../dxf_verbs.hpp"
#include "../import_records.hpp"
#include "gis_records.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/core/text.hpp"
#include "katana/dxf/reader.hpp"
#include "katana/gis/gdal_adapter.hpp"
#include "katana/interop/archive12d.hpp"
#include "katana/interop/import.hpp"
#include "verb_table.hpp"

namespace katana::app::geo {

namespace cmd = katana::commands;
namespace interop = katana::interop;
using katana::cad::ImportPlacement;
using katana::cad::ImportPlacementMode;
using katana::cad::ImportShift;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::geometry::Box2;
using katana::geometry::Vec2;

namespace {

constexpr const char* kUsage = "usage: IMPORT <file> [LOCAL | ALONGSIDE | OFFSET=dE,dN]";

enum class Kind { Dxf, Vector, Archive, Raster, PointCloud };

using Read = std::variant<katana::dxf::DxfImport, interop::VectorImportResult,
                          interop::Archive12dImportResult, interop::RasterOverlay,
                          interop::PointCloudLayer>;

// Data that becomes entities can be placed; reference data is drawn at its
// own coordinates and cannot.
bool placeable(Kind kind)
{
    return kind == Kind::Dxf || kind == Kind::Vector || kind == Kind::Archive;
}

Result<Read> readAt(Kind kind, const std::filesystem::path& file, std::optional<Vec2> shift)
{
    switch (kind) {
    case Kind::Dxf: {
        auto read = readDxfImport(file, shift);
        return read ? Result<Read>(Read(std::move(*read))) : Result<Read>(read.error());
    }
    case Kind::Vector: {
        interop::VectorImportOptions options;
        options.originShift = shift;
        auto read = interop::importVector(file, options);
        return read ? Result<Read>(Read(std::move(*read))) : Result<Read>(read.error());
    }
    case Kind::Archive: {
        interop::Archive12dImportOptions options;
        options.originShift = shift;
        auto read = interop::importArchive12d(file, options);
        return read ? Result<Read>(Read(std::move(*read))) : Result<Read>(read.error());
    }
    case Kind::Raster: {
        auto read = interop::importRaster(file);
        return read ? Result<Read>(Read(std::move(*read))) : Result<Read>(read.error());
    }
    case Kind::PointCloud: {
        auto read = interop::importPointCloud(file);
        return read ? Result<Read>(Read(std::move(*read))) : Result<Read>(read.error());
    }
    }
    return makeError(ErrorCode::Unsupported, "no importer", pathText(file));
}

Box2 boundsOf(const Read& read)
{
    struct Bounds {
        Box2 operator()(const katana::dxf::DxfImport& r) const { return r.bounds; }
        Box2 operator()(const interop::VectorImportResult& r) const { return r.bounds; }
        Box2 operator()(const interop::Archive12dImportResult& r) const { return r.bounds; }
        Box2 operator()(const interop::RasterOverlay& r) const { return r.worldBounds(); }
        Box2 operator()(const interop::PointCloudLayer& r) const { return r.worldBounds(); }
    };
    return std::visit(Bounds{}, read);
}

std::string joined(const std::vector<std::string>& records)
{
    std::string text;
    for (const std::string& record : records) {
        if (!record.empty()) {
            text += (text.empty() ? "" : "\n") + record;
        }
    }
    return text;
}

// ---- the applies, one per kind ------------------------------------------------------------

Result<std::string> applyVector(Context& context, interop::VectorImportResult&& read,
                                const std::filesystem::path& file)
{
    // Layers before the entities on them, and all of it ONE undo step: an
    // import made by mistake is one Ctrl+Z.
    auto transaction = std::make_unique<cmd::Transaction>("IMPORT " + pathText(file.filename()));
    for (const std::string& name : read.layersNeeded) {
        if (!context.document.model().layers.contains(name)) {
            katana::entity::Layer layer;
            layer.name = name;
            transaction->add(cmd::createLayer(layer));
        }
    }
    const std::size_t count = read.entities.size();
    if (count != 0) {
        transaction->add(cmd::createEntities(std::move(read.entities)));
    }
    // A file with nothing the drawing can hold leaves the transaction empty,
    // which the stack refuses as a command that changes nothing: not a
    // failed import - the file was read, and its warnings say why.
    if (transaction->size() != 0) {
        if (auto status = context.document.execute(std::move(transaction)); !status) {
            return status.error();
        }
    }
    std::vector<std::string> records{
        "imported file=" + value(pathText(file)) + " kind=vector entities=" + std::to_string(count) +
        " layers=" + std::to_string(read.layersNeeded.size()) +
        " features=" + std::to_string(read.featuresRead) +
        " skipped=" + std::to_string(read.featuresSkipped) + " bounds=" + boundsText(read.bounds) +
        " crs=" + value(read.projectionWkt.empty() ? std::string()
                                                   : katana::gis::describeCrs(read.projectionWkt))};
    for (const std::string& warning : read.warnings) {
        records.push_back(warningText(warning));
    }
    return joined(records);
}

Result<std::string> applyArchive(Context& context, interop::Archive12dImportResult&& read,
                                 const std::filesystem::path& file)
{
    // Layers, styles, entities and alignments: ONE undo step.
    auto transaction = std::make_unique<cmd::Transaction>("IMPORT " + pathText(file.filename()));
    for (const katana::entity::Layer& layer : read.layersNeeded) {
        if (!context.document.model().layers.contains(layer.name)) {
            transaction->add(cmd::createLayer(layer));
        }
    }
    for (const katana::entity::Style& style : read.stylesNeeded) {
        if (!context.document.model().styles.contains(style.name)) {
            transaction->add(cmd::createStyle(style));
        }
    }
    const std::size_t count = read.entities.size();
    if (count != 0) {
        transaction->add(cmd::createEntities(std::move(read.entities)));
    }
    std::size_t alignments = 0;
    for (katana::entity::Alignment& alignment : read.alignments) {
        // A name the drawing already has would fail the whole transaction.
        const std::string base = alignment.name;
        for (int copy = 2; context.document.model().alignments.contains(alignment.name); ++copy) {
            alignment.name = base + " (" + std::to_string(copy) + ")";
        }
        transaction->add(cmd::createAlignment(alignment));
        ++alignments;
    }
    // An archive of TINs, meshes or clouds alone has nothing for the drawing,
    // and the stack refuses an empty transaction: only one with something in
    // it runs, and the session data below is added either way.
    if (transaction->size() != 0) {
        if (auto status = context.document.execute(std::move(transaction)); !status) {
            return status.error();
        }
    }

    std::vector<std::string> records{
        "imported file=" + value(pathText(file)) + " kind=archive entities=" +
        std::to_string(count) + " alignments=" + std::to_string(alignments) +
        " surfaces=" + std::to_string(read.surfaces.size()) +
        " meshes=" + std::to_string(read.meshes.size()) +
        " clouds=" + std::to_string(read.clouds.size()) +
        " layers=" + std::to_string(read.layersNeeded.size()) +
        " styles=" + std::to_string(read.stylesNeeded.size()) + " encoding=" + value(read.encoding) +
        " version=" + value(read.archiveVersion) + " member=" + value(read.memberName) +
        " bounds=" + boundsText(read.bounds)};
    for (const katana::archive12d::ElementTally& tally : read.tally) {
        records.push_back(tallyRecord(tally.keyword, tally.read, tally.imported));
    }

    // Surfaces and clouds are session data, outside undo
    // (interop/reference_data.hpp), added after the transaction so a refused
    // import leaves none behind. A surface whose name is taken is "name (2)".
    for (katana::archive12d::ImportedSurface& surface : read.surfaces) {
        katana::terrain::NamedSurface named{
            context.surfaces.uniqueName(surface.name),
            std::make_shared<const katana::terrain::TinSurface>(std::move(surface.surface)),
            "12d archive " + pathText(file.filename())};
        if (auto added = context.surfaces.add(named); !added) {
            return added.error();
        }
        records.push_back(surfaceRecord(named));
    }
    for (interop::PointCloudLayer& cloud : read.clouds) {
        const interop::ReferenceId id = context.reference.add(std::move(cloud));
        records.push_back(referenceRecord(*context.reference.findPointCloud(id)));
    }
    ImportShown shown;
    shown.surfaces = read.surfaces.size();
    if (!read.meshes.empty()) {
        std::size_t triangles = 0;
        for (const katana::archive12d::ImportedMesh& mesh : read.meshes) {
            triangles += mesh.mesh.triangleCount();
        }
        const bool held = static_cast<bool>(context.imported);
        records.push_back("meshes count=" + std::to_string(read.meshes.size()) +
                          " triangles=" + std::to_string(triangles) +
                          " held=" + (held ? "yes" : "no"));
        if (!held) {
            // Read and checked, and nowhere to keep them: said in numbers so
            // they are not taken for kept.
            records.push_back(warningText("this session holds no meshes; import the archive in "
                                          "the window to see them"));
        }
        shown.meshes = std::move(read.meshes);
    }
    for (const std::string& warning : read.warnings) {
        records.push_back(warningText(warning));
    }
    if ((!read.surfaces.empty() || !read.clouds.empty()) && context.changed) {
        context.changed();
    }
    if (context.imported && (shown.surfaces != 0 || !shown.meshes.empty())) {
        context.imported(std::move(shown));
    }
    return joined(records);
}

Result<std::string> applyRaster(Context& context, interop::RasterOverlay&& read,
                                const std::filesystem::path& file)
{
    const bool georeferenced = read.hasGeotransform;
    const interop::ReferenceId id = context.reference.add(std::move(read));
    std::vector<std::string> records{"imported file=" + value(pathText(file)) + " kind=raster",
                                     referenceRecord(*context.reference.findRaster(id))};
    if (!georeferenced) {
        // Placing it at the origin is a guess, and whoever imported it has to
        // know that.
        records.push_back(warningText("this file carries no georeferencing; it is placed at the "
                                      "origin at one model unit per pixel"));
    }
    if (context.changed) {
        context.changed();
    }
    return joined(records);
}

Result<std::string> applyCloud(Context& context, interop::PointCloudLayer&& read,
                               const std::filesystem::path& file)
{
    const interop::ReferenceId id = context.reference.add(std::move(read));
    const std::string records = "imported file=" + value(pathText(file)) + " kind=pointcloud\n" +
                                referenceRecord(*context.reference.findPointCloud(id));
    if (context.changed) {
        context.changed();
    }
    return records;
}

// What the work read, placed and applied.
struct Imported {
    Kind kind = Kind::Vector;
    std::filesystem::path file;
    ImportPlacement placement;
    ImportShift placed;
    Box2 drawing; // the drawing's extent when the line was prepared
    Read read;
};

Result<std::string> applyImport(Context& context, Imported& imported)
{
    std::vector<std::string> extra;
    // Data that keeps its own coordinates and lands far from the drawing:
    // merged, one of the two would be a dot at a zoom that shows both. The
    // window asks (Shift Alongside, Keep, Cancel) BEFORE anything is added; a
    // session, and a window with nobody to ask, keeps the coordinates and
    // says what to type instead.
    if (placeable(imported.kind) && imported.placement.mode == ImportPlacementMode::Keep) {
        const Box2 incoming = boundsOf(imported.read);
        const interop::PlacementAdvice advice = interop::advisePlacement(imported.drawing, incoming);
        if (advice.farApart) {
            const FarApartChoice choice =
                context.farApart ? context.farApart(imported.drawing, incoming, advice.message)
                                 : FarApartChoice::Keep;
            if (choice == FarApartChoice::Cancel) {
                return makeError(ErrorCode::InvalidState, "cancelled: nothing was imported",
                                 pathText(imported.file));
            }
            if (choice == FarApartChoice::Alongside) {
                imported.placement = ImportPlacement{ImportPlacementMode::Alongside, {}};
                imported.placed =
                    katana::cad::resolveImportShift(imported.placement, imported.drawing, incoming);
                // Read again with the shift, as a line saying ALONGSIDE is:
                // the one reader moves everything alike.
                auto again = readAt(imported.kind, imported.file, imported.placed.shift);
                if (!again) {
                    return again.error();
                }
                imported.read = std::move(*again);
            } else {
                extra.push_back(warningText(
                    advice.message + "; UNDO, then IMPORT <file> LOCAL moves it as one piece so "
                                     "its lower-left corner sits at 0,0, or IMPORT <file> "
                                     "ALONGSIDE puts that corner on the drawing's"));
            }
        }
    }

    const Box2 bounds = boundsOf(imported.read);
    Result<std::string> applied = std::string();
    switch (imported.kind) {
    case Kind::Dxf:
        applied = applyDxfImport(context.document, std::get<katana::dxf::DxfImport>(std::move(imported.read)),
                                 imported.file, imported.placement, imported.placed);
        break;
    case Kind::Vector:
        applied = applyVector(context, std::get<interop::VectorImportResult>(std::move(imported.read)),
                              imported.file);
        break;
    case Kind::Archive:
        applied = applyArchive(context,
                               std::get<interop::Archive12dImportResult>(std::move(imported.read)),
                               imported.file);
        break;
    case Kind::Raster:
        applied = applyRaster(context, std::get<interop::RasterOverlay>(std::move(imported.read)),
                              imported.file);
        break;
    case Kind::PointCloud:
        applied = applyCloud(context, std::get<interop::PointCloudLayer>(std::move(imported.read)),
                             imported.file);
        break;
    }
    if (!applied) {
        return applied.error();
    }
    // The DXF records carry their own placed record; the others' goes after
    // the imported record, their first line.
    std::string reply = *applied;
    if (imported.kind != Kind::Dxf) {
        if (const std::string record = placedRecord(imported.placement, imported.placed);
            !record.empty()) {
            const std::size_t end = reply.find('\n');
            reply.insert(end == std::string::npos ? reply.size() : end, "\n" + record);
        }
    }
    for (const std::string& record : extra) {
        reply += "\n" + record;
    }
    if (context.frame && !bounds.empty()) {
        context.frame(bounds);
    }
    return reply;
}

} // namespace

Result<Prepared> prepareImport(Context& context, const Tokens& tokens, std::string_view line)
{
    (void)tokens;
    // The path and the placement read as the interpreter reads an IMPORT
    // line, whichever front end it came from: a quoted path, or one with
    // blanks, then LOCAL, ALONGSIDE or OFFSET=dE,dN.
    const std::string_view body = katana::core::trimmed(line);
    const std::size_t blank = body.find_first_of(" \t");
    auto argument = katana::cad::CommandInterpreter::importArgument(
        blank == std::string_view::npos ? std::string_view{} : body.substr(blank));
    if (!argument) {
        return argument.error();
    }
    if (argument->path.empty()) {
        return makeError(ErrorCode::InvalidArgument, kUsage);
    }
    const std::filesystem::path file = pathFromText(argument->path);
    const ImportPlacement placement = argument->placement;

    Kind kind = Kind::Vector;
    if (katana::dxf::isDxfPath(file)) {
        kind = Kind::Dxf;
    } else {
        switch (interop::kindForPath(file)) {
        case interop::SourceKind::Vector:
            kind = Kind::Vector;
            break;
        case interop::SourceKind::Archive12d:
            kind = Kind::Archive;
            break;
        case interop::SourceKind::Raster:
            kind = Kind::Raster;
            break;
        case interop::SourceKind::PointCloud:
            kind = Kind::PointCloud;
            break;
        case interop::SourceKind::Unknown:
            return makeError(ErrorCode::Unsupported,
                             "no importer for '" + pathText(file.extension()) + "'",
                             pathText(file));
        }
    }
    // Refused by name rather than dropped - or, as LOCAL once was, left on
    // the end of the path to fail as a missing file (audit QT-13, QT-14).
    if (!placeable(kind) && placement.mode != ImportPlacementMode::Keep) {
        const std::string word = placement.mode == ImportPlacementMode::Offset
                                     ? std::string("OFFSET")
                                     : katana::cad::placementWord(placement);
        return makeError(ErrorCode::InvalidArgument,
                         word + " is not supported for rasters and point clouds, which are "
                                "reference data drawn at their own coordinates");
    }

    // ALONGSIDE and the far-apart advice are judged against the drawing as
    // it was when the line was given.
    const Box2 drawing = context.document.model().entities.bounds();
    Prepared prepared;
    prepared.title = "IMPORT " + pathText(file.filename());
    prepared.work = [kind, file, placement, drawing](const std::stop_token& stop,
                                                     const Progress&) -> Result<Apply> {
        auto first = readAt(kind, file, std::nullopt);
        if (!first) {
            return first.error();
        }
        if (stop.stop_requested()) {
            return makeError(ErrorCode::InvalidState, "cancelled");
        }
        auto imported = std::make_shared<Imported>(
            Imported{kind, file, placement, ImportShift{}, drawing, std::move(*first)});
        if (placeable(kind)) {
            // Read at its own coordinates, then again with the shift the
            // placement resolves to (cad/import_placement.hpp).
            imported->placed = katana::cad::resolveImportShift(placement, drawing,
                                                               boundsOf(imported->read));
            if (imported->placed.shift) {
                auto again = readAt(kind, file, imported->placed.shift);
                if (!again) {
                    return again.error();
                }
                imported->read = std::move(*again);
            }
        }
        if (stop.stop_requested()) {
            return makeError(ErrorCode::InvalidState, "cancelled");
        }
        return Apply([imported](Context& ctx) { return applyImport(ctx, *imported); });
    };
    return prepared;
}

} // namespace katana::app::geo
