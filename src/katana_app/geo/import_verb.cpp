// IMPORT (docs/interop.md, "IMPORT, EXPORT, INFO, REFS and COPC on every front
// end"): a file into the drawing - a DXF natively, vector data and 12d
// archives as entities, rasters and point clouds as reference layers - run as
// the one executor runs every verb. The work reads the file (on a worker in
// the window); the apply adds what it read as ONE undo step and says so in
// records.
//
//   IMPORT <file> [LOCAL | ALONGSIDE | OFFSET=dE,dN] [<options>] [<scope> [clip]] [PREVIEW]
//
// The options (docs/interop.md, "Import options") say what of the file is
// read and how: for vector data layers=, where=, sql= with dialect=, fields=,
// target=, max=, oo=K=V (repeated), attributes=no; for every kind crs=
// (project reprojects vector data into the project's coordinate system,
// adopt sets the project's from the file when it has none) and srs= (the
// CRS of a file that declares none); for a raster band=, subdataset=,
// maxpixels=, name=; for a point cloud budget=, class=, resolution=, name=.
// A scope - the shared grammar, cad::parseScopeWords - limits vector data to
// what lies in its box, handed to the driver as its spatial filter; with
// `clip` features are cut at its edge (AREA, VIEW) or by the closed shapes it
// takes. PREVIEW reads with the filters and says how many features matched
// of how many, and imports nothing.
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
#include "katana/entity/entity_geometry.hpp"
#include "katana/gis/gdal_adapter.hpp"
#include "katana/interop/archive12d.hpp"
#include "katana/interop/import.hpp"
#include "katana/cad/import_placement.hpp"
#include "katana/cad/scope_verbs.hpp"
#include "katana/interop/geo/drawing_dataset.hpp"
#include "terrain_verbs.hpp"
#include "vector_support.hpp"
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

namespace vec = katana::app::geo::vector;

constexpr const char* kUsage =
    "usage: IMPORT <file> [LOCAL | ALONGSIDE | OFFSET=dE,dN] [layers=a,b] [where=\"...\"] "
    "[sql=\"...\"] [dialect=ogrsql|sqlite] [<scope> [clip]] [fields=a,b] [target=<layer>] "
    "[max=N] [oo=K=V]... [attributes=no] [crs=project|adopt] [srs=<code>] [band=N] "
    "[subdataset=N|name] [maxpixels=N] [name=<n>] [budget=N] [class=N] [resolution=<m>] "
    "[PREVIEW]";

// The option keys, lower case, and the kinds each is for. oo= is read apart:
// it may be given more than once.
struct OptionUse {
    const char* key;
    bool vector, raster, cloud;
};
constexpr OptionUse kOptions[] = {
    {"layers", true, false, false},     {"where", true, false, false},
    {"sql", true, false, false},        {"dialect", true, false, false},
    {"fields", true, false, false},     {"target", true, false, false},
    {"max", true, false, false},        {"attributes", true, false, false},
    {"offset", true, false, false},     {"crs", true, true, true},
    {"srs", true, true, false},         {"band", false, true, false},
    {"subdataset", false, true, false}, {"maxpixels", false, true, false},
    {"name", false, true, true},        {"budget", false, false, true},
    {"class", false, false, true},      {"resolution", false, false, true},
};

bool isOptionKey(std::string_view key)
{
    return key == "oo" || std::ranges::any_of(kOptions, [key](const OptionUse& use) {
               return key == use.key;
           });
}

// Whether word `i` begins the words after the path: an option, a flag, a
// placement or a scope word. A path is read up to the first of them, so a
// path with a blank in it and no quotes still reads - as it always did - and
// one that holds such a word is given in quotes.
bool beginsOptions(const Tokens& tokens, std::size_t i)
{
    const std::string& word = tokens[i];
    const std::size_t equals = word.find('=');
    if (equals != std::string::npos && equals > 0 &&
        isOptionKey(katana::core::lowered(word.substr(0, equals)))) {
        return true;
    }
    if (tokens.quoted[i]) {
        return false;
    }
    return tokens.is(i, "LOCAL") || tokens.is(i, "ALONGSIDE") || tokens.is(i, "CLIP") ||
           tokens.is(i, "PREVIEW") || katana::cad::isScopeWord(word);
}

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

// What a line asked of the read and of the apply, beside the placement.
struct Request {
    interop::VectorImportOptions vector;
    interop::RasterImportOptions raster;
    interop::PointCloudImportOptions cloud;
    // crs=adopt: the project takes the file's coordinate system when it has
    // none, in the same undo step as what the import adds.
    bool adopt = false;
    bool preview = false;
    // Whether the reply says how many features the filters took (matched).
    bool reportMatch = false;
    // What the scope took, said in the reply (cad::scopeRecord), and the box.
    std::string scopeRecord;
};

Result<Read> readAt(Kind kind, const std::filesystem::path& file, std::optional<Vec2> shift,
                    const Request& request)
{
    switch (kind) {
    case Kind::Dxf: {
        auto read = readDxfImport(file, shift);
        return read ? Result<Read>(Read(std::move(*read))) : Result<Read>(read.error());
    }
    case Kind::Vector: {
        interop::VectorImportOptions options = request.vector;
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
        auto read = interop::importRaster(file, request.raster);
        return read ? Result<Read>(Read(std::move(*read))) : Result<Read>(read.error());
    }
    case Kind::PointCloud: {
        auto read = interop::importPointCloud(file, request.cloud);
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

// crs=adopt: the command that gives the project the file's coordinate system,
// or null - with `record` saying why not - when the project has one already
// (adopting is for a drawing that has none; CRS SET changes one) or the file
// declares none.
Result<katana::commands::CommandPtr> adoptCrs(Context& context, const std::string& wkt,
                                              std::string& record)
{
    const std::string& project = context.document.metadata().coordinateSystem;
    if (!project.empty()) {
        record = "crs adopted=no project=" + value(project) +
                 " reason=" + value("the project has a coordinate system already");
        return katana::commands::CommandPtr{};
    }
    if (wkt.empty()) {
        record = "crs adopted=no reason=" +
                 value("the file declares no coordinate system; srs= says which it is in");
        return katana::commands::CommandPtr{};
    }
    return context.document.coordinateSystemCommand(wkt, "IMPORT crs=adopt");
}

// crs=adopt for reference data, which adds no undo step of its own: the
// project's coordinate system alone is the step.
Result<std::string> adoptAlone(Context& context, const std::string& wkt)
{
    std::string record;
    auto command = adoptCrs(context, wkt, record);
    if (!command) {
        return command.error();
    }
    if (*command == nullptr) {
        return record;
    }
    if (auto status = context.document.execute(std::move(*command)); !status) {
        return status.error();
    }
    return "crs adopted=yes id=" + value(context.document.metadata().coordinateSystem);
}

// The warning an import's reply carries when what it drew is in another
// coordinate system than the project's: drawn as they are, its coordinates
// do not register against the drawing's. Nothing when either is unknown, or
// when the project has just adopted the file's.
std::optional<std::string> crsWarning(const Context& context, const std::string& wkt,
                                      std::string_view consequence)
{
    auto differs = crsDiffers(wkt, projectCrs(context), "the file", consequence);
    return differs ? std::optional<std::string>(warningText(*differs)) : std::nullopt;
}

Result<std::string> applyVector(Context& context, interop::VectorImportResult&& read,
                                const std::filesystem::path& file, const Request& request)
{
    // crs=project moved the data into the project's system as it was when
    // the line was prepared; a CRS SET since would draw it in the wrong one.
    if (!request.vector.targetCrs.empty() &&
        katana::gis::sameCrs(request.vector.targetCrs, projectCrs(context)) !=
            std::optional<bool>(true)) {
        return makeError(ErrorCode::InvalidState,
                         "the project's coordinate system changed while the import ran, and "
                         "crs=project moved the data into the one it had; import again",
                         pathText(file));
    }
    // Layers before the entities on them, and all of it ONE undo step: an
    // import made by mistake is one Ctrl+Z - the project's coordinate system
    // too, when crs=adopt sets it.
    auto transaction = std::make_unique<cmd::Transaction>("IMPORT " + pathText(file.filename()));
    std::string crsRecord;
    bool adopted = false;
    if (request.adopt) {
        auto command = adoptCrs(context, read.projectionWkt, crsRecord);
        if (!command) {
            return command.error();
        }
        if (*command != nullptr) {
            transaction->add(std::move(*command));
            adopted = true;
        }
    }
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
    if (request.reportMatch) {
        records.push_back("matched features=" + std::to_string(read.featuresRead) +
                          " of=" + std::to_string(read.featuresInFile));
    }
    if (adopted) {
        crsRecord = "crs adopted=yes id=" + value(context.document.metadata().coordinateSystem);
    }
    if (!crsRecord.empty()) {
        records.push_back(crsRecord);
    }
    for (const std::string& warning : read.warnings) {
        records.push_back(warningText(warning));
    }
    if (auto warning = crsWarning(context, read.projectionWkt,
                                  "its coordinates were drawn as they are; crs=project moves "
                                  "them into the project's")) {
        records.push_back(std::move(*warning));
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
                                const std::filesystem::path& file, const Request& request)
{
    std::string crsRecord;
    if (request.adopt) {
        auto adopted = adoptAlone(context, read.projectionWkt);
        if (!adopted) {
            return adopted.error();
        }
        crsRecord = *adopted;
    }
    const bool georeferenced = read.hasGeotransform;
    const auto differs = crsWarning(context, read.projectionWkt,
                                    "it is drawn at its own coordinates; RASTER REPROJECT makes a "
                                    "copy in the project's");
    const interop::ReferenceId id = context.reference.add(std::move(read));
    std::vector<std::string> records{"imported file=" + value(pathText(file)) + " kind=raster",
                                     referenceRecord(*context.reference.findRaster(id))};
    if (!crsRecord.empty()) {
        records.push_back(crsRecord);
    }
    if (!georeferenced) {
        // Placing it at the origin is a guess, and whoever imported it has to
        // know that.
        records.push_back(warningText("this file carries no georeferencing; it is placed at the "
                                      "origin at one model unit per pixel"));
    }
    if (differs) {
        records.push_back(*differs);
    }
    if (context.changed) {
        context.changed();
    }
    return joined(records);
}

Result<std::string> applyCloud(Context& context, interop::PointCloudLayer&& read,
                               const std::filesystem::path& file, const Request& request)
{
    std::string crsRecord;
    if (request.adopt) {
        auto adopted = adoptAlone(context, read.projectionWkt);
        if (!adopted) {
            return adopted.error();
        }
        crsRecord = "\n" + *adopted;
    }
    const auto differs = crsWarning(context, read.projectionWkt,
                                    "it is drawn at its own coordinates, where the drawing's are "
                                    "not");
    const interop::ReferenceId id = context.reference.add(std::move(read));
    const std::string records = "imported file=" + value(pathText(file)) + " kind=pointcloud\n" +
                                referenceRecord(*context.reference.findPointCloud(id)) + crsRecord +
                                (differs ? "\n" + *differs : std::string());
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
    Request request;
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
                auto again = readAt(imported.kind, imported.file, imported.placed.shift,
                                    imported.request);
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
                              imported.file, imported.request);
        break;
    case Kind::Archive:
        applied = applyArchive(context,
                               std::get<interop::Archive12dImportResult>(std::move(imported.read)),
                               imported.file);
        break;
    case Kind::Raster:
        applied = applyRaster(context, std::get<interop::RasterOverlay>(std::move(imported.read)),
                              imported.file, imported.request);
        break;
    case Kind::PointCloud:
        applied = applyCloud(context, std::get<interop::PointCloudLayer>(std::move(imported.read)),
                             imported.file, imported.request);
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
    if (!imported.request.scopeRecord.empty()) {
        const std::size_t end = reply.find('\n');
        reply.insert(end == std::string::npos ? reply.size() : end,
                     "\n" + imported.request.scopeRecord);
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

namespace {

// The option `key` as a whole number in [low, high]; nullopt when not given.
Result<std::optional<std::uint64_t>> wholeOption(const vec::VerbWords& words, std::string_view key,
                                                 double low, double high)
{
    auto number = vec::numberOption(words, key);
    if (!number) {
        return number.error();
    }
    if (!*number) {
        return std::optional<std::uint64_t>();
    }
    const double given = **number;
    if (std::floor(given) != given || given < low || given > high) {
        return makeError(ErrorCode::InvalidArgument,
                         std::string(key) + "= is a whole number from " +
                             katana::core::formatExactReal(low) + " to " +
                             katana::core::formatExactReal(high),
                         std::string(key) + "=" + *words.option(key));
    }
    return std::optional<std::uint64_t>(static_cast<std::uint64_t>(given));
}

// The box a scope stands for: an AREA's box; a VIEW's visible area; else
// the extent of what it took. Empty when it took nothing.
Box2 scopeBox(const katana::cad::ScopeMatch& match, const katana::entity::Model& model)
{
    if (match.resolved.words.source == katana::cad::ScopeSource::Area) {
        return match.resolved.words.area;
    }
    if (match.resolved.words.source == katana::cad::ScopeSource::View && match.resolved.scope.area) {
        return *match.resolved.scope.area;
    }
    Box2 box;
    for (const katana::entity::EntityId id : match.matched) {
        if (const katana::entity::Entity* entity = model.entities.find(id)) {
            const Box2 one = katana::entity::boundingBox(entity->geometry);
            if (!one.empty()) {
                box.expand(one.min);
                box.expand(one.max);
            }
        }
    }
    return box;
}

std::string kindWord(Kind kind)
{
    switch (kind) {
    case Kind::Dxf:
        return "dxf";
    case Kind::Vector:
        return "vector";
    case Kind::Archive:
        return "archive";
    case Kind::Raster:
        return "raster";
    case Kind::PointCloud:
        return "pointcloud";
    }
    return "vector";
}

} // namespace

Result<Prepared> prepareImport(Context& context, const Tokens& tokens, std::string_view line)
{
    // The path, then the words after it. A quoted path is one word; an
    // unquoted one runs to the first option, flag, placement or scope word,
    // so a path with blanks still reads without quotes. With nothing after
    // it but a placement, the line reads exactly as the interpreter reads an
    // IMPORT line (CommandInterpreter::importArgument), whichever front end
    // it came from.
    if (tokens.size() < 2) {
        return makeError(ErrorCode::InvalidArgument, kUsage);
    }
    std::size_t first = 2;
    std::string path;
    if (tokens.quoted[1]) {
        path = tokens[1];
    } else {
        while (first < tokens.size() && !beginsOptions(tokens, first)) {
            ++first;
        }
        for (std::size_t i = 1; i < first; ++i) {
            path += (i > 1 ? " " : "") + tokens[i];
        }
    }
    ImportPlacement placement;
    const bool placementOnly =
        first == tokens.size() ||
        (first + 1 == tokens.size() &&
         katana::cad::parsePlacementWord(tokens[first]).valueOr(std::nullopt).has_value());
    if (placementOnly) {
        const std::string_view body = katana::core::trimmed(line);
        const std::size_t blank = body.find_first_of(" \t");
        auto argument = katana::cad::CommandInterpreter::importArgument(
            blank == std::string_view::npos ? std::string_view{} : body.substr(blank));
        if (!argument) {
            return argument.error();
        }
        path = argument->path;
        placement = argument->placement;
        first = tokens.size();
    }
    if (katana::core::trimmed(path).empty() || beginsOptions(tokens, 1)) {
        return makeError(ErrorCode::InvalidArgument, kUsage);
    }
    const std::filesystem::path file = pathFromText(path);

    // oo= may be given more than once: taken out before the rest are read.
    Tokens rest;
    std::vector<std::string> openOptions;
    for (std::size_t i = first; i < tokens.size(); ++i) {
        const std::string& word = tokens[i];
        if (word.size() > 3 && katana::core::lowered(word.substr(0, 3)) == "oo=") {
            openOptions.push_back(word.substr(3));
            continue;
        }
        rest.words.push_back(word);
        rest.quoted.push_back(tokens.quoted[i]);
    }
    vec::WordRules rules;
    for (const OptionUse& use : kOptions) {
        rules.options.emplace_back(use.key);
    }
    rules.flags = {"LOCAL", "ALONGSIDE", "CLIP", "PREVIEW"};
    rules.usage = kUsage;
    auto words = vec::readVerbWords(rest, 0, rest.size(), rules);
    if (!words) {
        return words.error();
    }

    // The placement, from its words.
    const int placements = (words->has("LOCAL") ? 1 : 0) + (words->has("ALONGSIDE") ? 1 : 0) +
                           (words->option("offset") != nullptr ? 1 : 0);
    if (placements > 1) {
        return makeError(ErrorCode::InvalidArgument,
                         "one placement per line: LOCAL, ALONGSIDE or OFFSET=dE,dN");
    }
    if (words->has("LOCAL")) {
        placement = ImportPlacement{ImportPlacementMode::Local, {}};
    } else if (words->has("ALONGSIDE")) {
        placement = ImportPlacement{ImportPlacementMode::Alongside, {}};
    } else if (const std::string* offset = words->option("offset")) {
        auto parsed = katana::cad::parsePlacementWord("OFFSET=" + *offset);
        if (!parsed) {
            return parsed.error();
        }
        if (*parsed) {
            placement = **parsed;
        }
    }

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
    // An option for another kind of data is refused by name, never ignored:
    // a band= on a shapefile would otherwise look as if it had been used.
    for (const auto& [key, text] : words->options) {
        if (key == "offset") {
            continue;
        }
        const auto use = std::ranges::find_if(
            kOptions, [&key](const OptionUse& each) { return key == each.key; });
        const bool fits = (kind == Kind::Vector && use->vector) ||
                          (kind == Kind::Raster && use->raster) ||
                          (kind == Kind::PointCloud && use->cloud);
        if (!fits) {
            return makeError(ErrorCode::InvalidArgument,
                             key + "= is not an option of a " + kindWord(kind) + " import",
                             key + "=" + text);
        }
    }
    if (!openOptions.empty() && kind != Kind::Vector) {
        return makeError(ErrorCode::InvalidArgument,
                         "oo= is an option of a vector import", "oo=" + openOptions.front());
    }
    if ((words->scopeGiven || words->has("CLIP")) && kind != Kind::Vector) {
        return makeError(ErrorCode::InvalidArgument,
                         "a scope limits a vector import; a raster or a point cloud is read "
                         "whole (RASTER CLIP cuts a raster to a scope)");
    }

    Request request;
    request.preview = words->has("PREVIEW");
    interop::VectorImportOptions& vector = request.vector;
    vector.openOptions = std::move(openOptions);
    if (const std::string* layers = words->option("layers")) {
        vector.layerNames = vec::listOf(*layers);
        request.reportMatch = true;
    }
    if (const std::string* where = words->option("where")) {
        vector.attributeFilter = *where;
        request.reportMatch = true;
    }
    if (const std::string* sql = words->option("sql")) {
        vector.sql = *sql;
        request.reportMatch = true;
    }
    if (words->option("dialect") != nullptr) {
        if (vector.sql.empty()) {
            return makeError(ErrorCode::InvalidArgument, "dialect= is the dialect of sql=");
        }
        auto dialect = vec::choiceOption(*words, "dialect", {"ogrsql", "sqlite"}, "");
        if (!dialect) {
            return dialect.error();
        }
        vector.sqlDialect = *dialect;
    }
    if (const std::string* fields = words->option("fields")) {
        vector.fields = vec::listOf(*fields);
    }
    if (const std::string* target = words->option("target")) {
        if (katana::core::trimmed(*target).empty()) {
            return makeError(ErrorCode::InvalidArgument, "target= names a layer");
        }
        vector.targetLayer = std::string(katana::core::trimmed(*target));
    }
    auto max = wholeOption(*words, "max", 1, 1e15);
    if (!max) {
        return max.error();
    }
    vector.maxFeatures = max->value_or(0);
    auto attributes = vec::choiceOption(*words, "attributes", {"yes", "no"}, "yes");
    if (!attributes) {
        return attributes.error();
    }
    vector.attributesAsProperties = *attributes == "yes";

    // crs= and srs=: the one question of every kind.
    auto crs = vec::choiceOption(*words, "crs", {"project", "adopt"}, "");
    if (!crs) {
        return crs.error();
    }
    const std::string project = context.document.metadata().coordinateSystem;
    if (*crs == "project") {
        if (kind != Kind::Vector) {
            return makeError(ErrorCode::Unsupported,
                             "crs=project moves vector data; a raster or a point cloud is drawn "
                             "at its own coordinates (RASTER REPROJECT makes a copy in another)");
        }
        if (project.empty()) {
            return makeError(ErrorCode::InvalidCRS,
                             "crs=project moves the data into the project's coordinate system, "
                             "and the project has none: CRS SET <code> first, or crs=adopt");
        }
        vector.targetCrs = project;
    }
    request.adopt = *crs == "adopt";
    if (const std::string* srs = words->option("srs")) {
        auto readable = katana::gis::crsToWkt(*srs);
        if (!readable) {
            return readable.error();
        }
        vector.sourceCrs = *srs;
        request.raster.assumedCrs = *srs;
    }

    // A raster's and a point cloud's own.
    auto band = wholeOption(*words, "band", 1, 65535);
    if (!band) {
        return band.error();
    }
    request.raster.band = static_cast<int>(band->value_or(0));
    if (const std::string* subdataset = words->option("subdataset")) {
        request.raster.subdataset = *subdataset;
    }
    auto maxPixels = wholeOption(*words, "maxpixels", 1, 65536);
    if (!maxPixels) {
        return maxPixels.error();
    }
    if (*maxPixels) {
        request.raster.maxPixels = static_cast<int>(**maxPixels);
    }
    if (const std::string* name = words->option("name")) {
        request.raster.name = *name;
        request.cloud.name = *name;
    }
    auto budget = wholeOption(*words, "budget", 1, 1e12);
    if (!budget) {
        return budget.error();
    }
    if (*budget) {
        request.cloud.budget = **budget;
    }
    if (const std::string* classes = words->option("class")) {
        // The reader filters by one classification (PointCloudReadOptions).
        if (vec::listOf(*classes).size() > 1) {
            return makeError(ErrorCode::Unsupported,
                             "class= takes one class: the point cloud reader filters by one",
                             "class=" + *classes);
        }
    }
    auto pointClass = wholeOption(*words, "class", 0, 255);
    if (!pointClass) {
        return pointClass.error();
    }
    if (*pointClass) {
        request.cloud.classification = static_cast<std::uint8_t>(**pointClass);
    }
    auto resolution = vec::numberOption(*words, "resolution");
    if (!resolution) {
        return resolution.error();
    }
    if (*resolution) {
        if (!(**resolution > 0.0)) {
            return makeError(ErrorCode::InvalidArgument, "resolution= is a spacing above 0");
        }
        request.cloud.resolution = **resolution;
    }

    // The scope, in the drawing's coordinates: its box is the driver's
    // spatial filter; with clip, the features are cut at its edge (AREA, a
    // VIEW's area) or by the closed shapes it took.
    if (words->has("CLIP") && !words->scopeGiven) {
        return makeError(ErrorCode::InvalidArgument,
                         "clip cuts at a scope's edge: give AREA, VIEW, DRAWING, SELECTION or "
                         "LAYERS with it");
    }
    if (words->scopeGiven) {
        if (placement.mode != ImportPlacementMode::Keep) {
            return makeError(ErrorCode::InvalidArgument,
                             "a scope is in the drawing's coordinates, which " +
                                 katana::cad::placementWord(placement) +
                                 " moves the data away from: give one or the other");
        }
        auto match = katana::cad::matchScope(context.document, words->scope,
                                             context.interpreter.scopeContext());
        if (!match) {
            return match.error();
        }
        const Box2 box = scopeBox(*match, context.document.model());
        const bool clip = words->has("CLIP");
        std::string record = "scope " + katana::cad::scopeRecord(*match) +
                             " box=" + (box.empty() ? std::string() : boundsText(box)) +
                             " clip=" + (clip ? "yes" : "no");
        if (box.empty()) {
            // Taking nothing is an answer, not a failure: said, nothing read.
            Prepared prepared;
            prepared.title = "IMPORT " + pathText(file.filename());
            prepared.reply = record + "\nimport file=" + value(pathText(file)) +
                             " kind=vector ran=no reason=" + value("the scope took nothing");
            return prepared;
        }
        vector.area = katana::gis::CrsBox{box.min.x, box.min.y, box.max.x, box.max.y};
        const auto source = match->resolved.words.source;
        const bool boxed = source == katana::cad::ScopeSource::Area ||
                           (source == katana::cad::ScopeSource::View &&
                            match->resolved.scope.area.has_value());
        if (clip && boxed) {
            vector.clipToArea = true;
        } else if (clip) {
            auto drawn = interop::geo::drawingDataset(context.document.model(), match->matched);
            if (!drawn) {
                return drawn.error();
            }
            katana::gis::processing::FeatureSet shapes;
            for (katana::gis::processing::FeatureTable& table : drawn->set.tables) {
                if (table.kind == katana::gis::GeometryKind::Polygon && !table.features.empty()) {
                    shapes.tables.push_back(std::move(table));
                }
            }
            if (shapes.tables.empty()) {
                return makeError(ErrorCode::InvalidArgument,
                                 "clip cuts by the closed shapes a scope takes, and this one took " +
                                     std::to_string(match->matched.size()) +
                                     " entities, none of them closed",
                                 record);
            }
            vector.clipShapes = std::move(shapes);
        }
        request.scopeRecord = std::move(record);
        request.reportMatch = true;
    }

    // ALONGSIDE and the far-apart advice are judged against the drawing as
    // it was when the line was given.
    const Box2 drawing = context.document.model().entities.bounds();
    Prepared prepared;
    prepared.title = "IMPORT " + pathText(file.filename());
    if (request.preview && kind != Kind::Vector) {
        // Only vector data has filters to count; the rest is read whole.
        prepared.reply = "import file=" + value(pathText(file)) + " kind=" + kindWord(kind) +
                         " preview=yes";
        return prepared;
    }
    prepared.work = [kind, file, placement, drawing,
                     request](const std::stop_token& stop, const Progress&) -> Result<Apply> {
        auto firstRead = readAt(kind, file, std::nullopt, request);
        if (!firstRead) {
            return firstRead.error();
        }
        if (stop.stop_requested()) {
            return makeError(ErrorCode::InvalidState, "cancelled");
        }
        if (request.preview) {
            // Read with every filter, counted, and nothing kept.
            const auto& read = std::get<interop::VectorImportResult>(*firstRead);
            std::string reply = "import file=" + value(pathText(file)) +
                                " kind=vector preview=yes features=" +
                                std::to_string(read.featuresRead) +
                                " of=" + std::to_string(read.featuresInFile) +
                                " entities=" + std::to_string(read.entities.size()) +
                                " layers=" + std::to_string(read.layersNeeded.size());
            if (!request.scopeRecord.empty()) {
                reply = request.scopeRecord + "\n" + reply;
            }
            for (const std::string& warning : read.warnings) {
                reply += "\n" + warningText(warning);
            }
            return Apply([reply](Context&) -> Result<std::string> { return reply; });
        }
        auto imported = std::make_shared<Imported>(Imported{
            kind, file, placement, ImportShift{}, drawing, std::move(*firstRead), request});
        if (placeable(kind)) {
            // Read at its own coordinates, then again with the shift the
            // placement resolves to (cad/import_placement.hpp).
            imported->placed = katana::cad::resolveImportShift(placement, drawing,
                                                               boundsOf(imported->read));
            if (imported->placed.shift) {
                auto again = readAt(kind, file, imported->placed.shift, request);
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
