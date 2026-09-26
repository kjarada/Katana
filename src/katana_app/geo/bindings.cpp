// What every geoprocessing verb shares (bindings.hpp, docs/geoprocessing.md
// "Bindings").
//
// Reserved blocks, one per package that adds to this file, so the lanes that
// build on it edit only their own:
//   T0  TO SURFACE: a raster result kept as a named surface.
//   V5  TO SELECTION and TO REPORT: a query's rows selected or reported.

#include "bindings.hpp"

#include <algorithm>
#include <atomic>
#include <filesystem>
#include <string>
#include <system_error>
#include <utility>
#include <vector>

#include "katana/core/text.hpp"
#include "katana/entity/entity_geometry.hpp"
#include "katana/gis/gdal_adapter.hpp"
#include "katana/interop/geo/raster_products.hpp"
#include "katana/interop/terrain_io.hpp"
#include "replies.hpp"
#include "terrain_verbs.hpp"

namespace katana::app::geo {

namespace gp = katana::gis::processing;
namespace igeo = katana::interop::geo;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;

namespace {

std::string utf8Of(const std::filesystem::path& path)
{
    const std::u8string text = path.generic_u8string();
    return std::string(text.begin(), text.end());
}

std::filesystem::path pathOf(const std::string& utf8)
{
    return std::filesystem::path(std::u8string(utf8.begin(), utf8.end()));
}

bool clauseWord(const Tokens& tokens, std::size_t i)
{
    for (const char* word : {"FROM", "TO", "CONFIRM", "OVERWRITE", "PREVIEW"}) {
        if (tokens.is(i, word)) {
            return true;
        }
    }
    return false;
}

const katana::interop::RasterOverlay* findRaster(const katana::interop::ReferenceData& reference,
                                                 const std::string& word)
{
    const bool digits = !word.empty() && std::ranges::all_of(word, [](char c) {
        return c >= '0' && c <= '9';
    });
    for (const katana::interop::RasterOverlay& raster : reference.rasters()) {
        if (digits && std::to_string(raster.id) == word) {
            return &raster;
        }
    }
    for (const katana::interop::RasterOverlay& raster : reference.rasters()) {
        if (katana::core::equalsIgnoringCase(raster.name, word)) {
            return &raster;
        }
    }
    return nullptr;
}

bool referenceNameTaken(const katana::interop::ReferenceData& reference, const std::string& name)
{
    return std::ranges::any_of(reference.rasters(), [&](const katana::interop::RasterOverlay& raster) {
        return katana::core::equalsIgnoringCase(raster.name, name);
    });
}

// A derived raster's file into its place: renamed, or copied and removed
// when the scratch folder and the project are on different volumes.
Status moveFile(const std::filesystem::path& from, const std::filesystem::path& to)
{
    std::error_code error;
    std::filesystem::create_directories(to.parent_path(), error);
    std::filesystem::rename(from, to, error);
    if (!error) {
        return {};
    }
    error.clear();
    std::filesystem::copy_file(from, to, std::filesystem::copy_options::overwrite_existing, error);
    if (error) {
        return makeError(ErrorCode::FileExportFailure,
                         "could not keep the derived raster: " + error.message(), utf8Of(to));
    }
    std::filesystem::remove(from, error);
    return {};
}

katana::geometry::Box2 boundsOf(const katana::cad::Document& document,
                                const std::vector<katana::entity::EntityId>& ids)
{
    katana::geometry::Box2 box;
    for (const katana::entity::EntityId id : ids) {
        if (const katana::entity::Entity* entity = document.model().entities.find(id)) {
            const katana::geometry::Box2 one = katana::entity::boundingBox(entity->geometry);
            if (!one.empty()) {
                box.expand(one.min);
                box.expand(one.max);
            }
        }
    }
    return box;
}

// A raster result to the reference data, under a name no raster has.
Result<std::string> keepDerived(Context& context, const ApplyRequest& request,
                                const std::filesystem::path& file)
{
    const std::string base =
        request.target.name.empty() ? request.defaultName : request.target.name;
    const std::optional<std::filesystem::path> project = context.document.projectDirectory();
    std::string name = base;
    std::filesystem::path destination;
    for (int copy = 2;; ++copy) {
        destination = igeo::derivedRasterPath(project, context.scratch, name);
        std::error_code error;
        if (!referenceNameTaken(context.reference, name) &&
            !std::filesystem::exists(destination, error)) {
            break;
        }
        name = base + "-" + std::to_string(copy);
    }
    if (auto moved = moveFile(file, destination); !moved) {
        return moved.error();
    }
    auto overlay = igeo::derivedOverlay(destination, name, request.result.commandName);
    if (!overlay) {
        return overlay.error();
    }
    const int width = overlay->width;
    const int height = overlay->height;
    const katana::geometry::Box2 bounds = overlay->worldBounds();
    const katana::interop::ReferenceId id = context.reference.add(std::move(*overlay));
    if (context.changed) {
        context.changed();
    }
    if (context.frame && !bounds.empty()) {
        context.frame(bounds);
    }
    return "output arg=" + value(request.arg) + " kind=raster target=reference id=" +
           std::to_string(id) + " name=" + value(name) + " raster=" + std::to_string(width) + "x" +
           std::to_string(height) + " file=" + value(utf8Of(destination)) +
           " persisted=" + (project ? "yes" : "no");
}

// A grid a run kept in memory, written where a derived raster is kept.
Result<std::filesystem::path> writeGrid(const Context& context, const gp::RasterGrid& grid)
{
    if (grid.bands.size() != 1) {
        return makeError(ErrorCode::Unsupported,
                         "a result of several bands in memory cannot be kept as a reference raster");
    }
    katana::gis::RasterExportOptions options;
    options.width = grid.info.width;
    options.height = grid.info.height;
    options.driver = "GTiff";
    options.projectionWkt = grid.info.projectionWkt;
    options.geotransform = grid.info.geotransform;
    options.noDataValue = grid.noData.empty() ? std::nullopt : grid.noData.front();
    static std::atomic<std::uint64_t> written{0};
    const std::filesystem::path file =
        context.scratch / ("katana-grid-" + std::to_string(++written) + ".tif");
    std::error_code error;
    std::filesystem::create_directories(context.scratch, error);
    if (auto status = katana::gis::GdalDataset::writeRaster(file, options, grid.bands.front());
        !status) {
        return status.error();
    }
    return file;
}

} // namespace

bool Tokens::is(std::size_t i, std::string_view keyword) const
{
    return i < words.size() && !quoted[i] && katana::core::equalsIgnoringCase(words[i], keyword);
}

Result<Tokens> tokenize(std::string_view line)
{
    // The interpreter's rules (cad::tokenize): blanks separate, double quotes
    // group and are removed, "" is an empty word. Kept here with the one
    // thing it drops - which words were quoted.
    Tokens tokens;
    std::string current;
    bool inQuotes = false;
    bool hasToken = false;
    bool wasQuoted = false;
    for (const char ch : line) {
        if (ch == '"') {
            inQuotes = !inQuotes;
            hasToken = true;
            wasQuoted = true;
        } else if (!inQuotes && katana::core::isAsciiSpace(ch)) {
            if (hasToken) {
                tokens.words.push_back(std::move(current));
                tokens.quoted.push_back(wasQuoted);
                current.clear();
                hasToken = false;
                wasQuoted = false;
            }
        } else {
            current += ch;
            hasToken = true;
        }
    }
    if (inQuotes) {
        return makeError(ErrorCode::ParseFailure, "unterminated quoted string");
    }
    if (hasToken) {
        tokens.words.push_back(std::move(current));
        tokens.quoted.push_back(wasQuoted);
    }
    return tokens;
}

bool isSourceKeyword(const Tokens& tokens, std::size_t i)
{
    return tokens.is(i, "RASTER") || tokens.is(i, "SURFACE") || tokens.is(i, "FILE") ||
           (i < tokens.size() && !tokens.quoted[i] && katana::cad::isScopeWord(tokens[i]));
}

Result<Source> parseSource(const Tokens& tokens, std::size_t& at)
{
    const auto refuse = [&](const std::string& why) {
        return makeError(ErrorCode::InvalidArgument, why,
                         at < tokens.size() ? tokens[at] : std::string());
    };
    if (at >= tokens.size() || clauseWord(tokens, at)) {
        return refuse("FROM needs a source: SELECTION, DRAWING, VIEW, AREA, LAYERS or WHERE for "
                      "the drawing; RASTER <id|name>; SURFACE <name> [CELL <m>]; FILE <path>");
    }
    Source source;
    const auto needs = [&](const char* what) -> Result<std::string> {
        if (at + 1 >= tokens.size() || clauseWord(tokens, at + 1)) {
            return makeError(ErrorCode::InvalidArgument, std::string(what), tokens[at]);
        }
        at += 2;
        return tokens[at - 1];
    };
    if (tokens.is(at, "RASTER")) {
        auto name = needs("RASTER needs the reference raster's id or name");
        if (!name) {
            return name.error();
        }
        source.kind = Source::Kind::Raster;
        source.raster = *name;
        return source;
    }
    if (tokens.is(at, "SURFACE")) {
        auto name = needs("SURFACE needs the surface's name");
        if (!name) {
            return name.error();
        }
        source.kind = Source::Kind::Surface;
        source.surface = *name;
        if (tokens.is(at, "CELL")) {
            auto cell = needs("CELL needs a length");
            if (!cell) {
                return cell.error();
            }
            const auto length = katana::core::parseFiniteDouble(*cell);
            if (!length || !(*length > 0.0)) {
                return makeError(ErrorCode::InvalidArgument, "CELL is a positive length", *cell);
            }
            source.cell = *length;
        }
        return source;
    }
    if (tokens.is(at, "FILE")) {
        auto path = needs("FILE needs a path");
        if (!path) {
            return path.error();
        }
        source.kind = Source::Kind::File;
        source.path = *path;
        if (tokens.is(at, "LAYER")) {
            auto layer = needs("LAYER needs the file's layer name");
            if (!layer) {
                return layer.error();
            }
            source.layer = *layer;
        }
        return source;
    }
    if (tokens.quoted[at] || !katana::cad::isScopeWord(tokens[at])) {
        return refuse("not a source; FROM takes SELECTION, DRAWING, VIEW, AREA, LAYERS or WHERE "
                      "for the drawing, RASTER, SURFACE or FILE");
    }
    // The ONE scope parser: it stops at the first word that is neither a
    // scope word nor a WHERE condition, which is where the line goes on.
    const std::size_t start = at;
    auto scope = katana::cad::parseScopeWords(tokens.words, at);
    if (!scope) {
        return scope.error();
    }
    if (at == start) {
        return refuse("not a source");
    }
    source.kind = Source::Kind::Drawing;
    source.scope = std::move(scope).value();
    return source;
}

Result<Target> parseTarget(const Tokens& tokens, std::size_t& at)
{
    Target target;
    const auto refuse = [&]() {
        return makeError(ErrorCode::InvalidArgument,
                         "TO takes LAYER <path>, REFERENCE [<name>], FILE <path> [FORMAT "
                         "<driver>], SURFACE <name>, SELECTION or REPORT",
                         at < tokens.size() ? tokens[at] : std::string());
    };
    const auto named = [&](Target::Kind kind, const char* why) -> Result<Target> {
        if (at + 1 >= tokens.size() || clauseWord(tokens, at + 1)) {
            return makeError(ErrorCode::InvalidArgument, why, tokens[at]);
        }
        target.kind = kind;
        target.name = tokens[at + 1];
        at += 2;
        return target;
    };
    if (at >= tokens.size()) {
        return refuse();
    }
    if (tokens.is(at, "LAYER") || tokens.is(at, "LAYERS")) {
        return named(Target::Kind::Layer, "TO LAYER needs the layer's path");
    }
    if (tokens.is(at, "SURFACE")) {
        return named(Target::Kind::Surface, "TO SURFACE needs a name");
    }
    if (tokens.is(at, "FILE")) {
        auto file = named(Target::Kind::File, "TO FILE needs a path");
        if (!file) {
            return file;
        }
        if (tokens.is(at, "FORMAT")) {
            if (at + 1 >= tokens.size()) {
                return makeError(ErrorCode::InvalidArgument, "FORMAT needs a GDAL driver's name");
            }
            target.format = tokens[at + 1];
            at += 2;
        }
        return target;
    }
    if (tokens.is(at, "REFERENCE")) {
        target.kind = Target::Kind::Reference;
        ++at;
        if (at < tokens.size() && !clauseWord(tokens, at)) {
            target.name = tokens[at];
            ++at;
        }
        return target;
    }
    if (tokens.is(at, "SELECTION")) {
        target.kind = Target::Kind::Selection;
        ++at;
        return target;
    }
    if (tokens.is(at, "REPORT")) {
        target.kind = Target::Kind::Report;
        ++at;
        return target;
    }
    return refuse();
}

std::string projectCrs(const Context& context)
{
    return context.document.metadata().coordinateSystem;
}

std::filesystem::path derivedFolder(const Context& context)
{
    const std::optional<std::filesystem::path> project = context.document.projectDirectory();
    return igeo::derivedRasterPath(project, context.scratch, "x").parent_path();
}

Result<BoundDrawing> bindDrawing(Context& context, const katana::cad::ScopeWords& scope,
                                 const igeo::DrawingDatasetOptions& options)
{
    auto match = katana::cad::matchScope(context.document, scope,
                                         context.interpreter.scopeContext());
    if (!match) {
        return match.error();
    }
    auto dataset = igeo::drawingDataset(context.document.model(), match->matched, options);
    if (!dataset) {
        return dataset.error();
    }
    return BoundDrawing{std::move(match).value(), std::move(dataset).value()};
}

Result<DeferredDataset> bindRaster(Context& context, const Source& source)
{
    switch (source.kind) {
    case Source::Kind::Raster: {
        const katana::interop::RasterOverlay* raster = findRaster(context.reference, source.raster);
        if (raster == nullptr) {
            return makeError(ErrorCode::NotFound,
                             "no reference raster has that id or name; REFS lists them",
                             source.raster);
        }
        if (raster->source.empty()) {
            return makeError(ErrorCode::Unsupported,
                             "the reference raster has no file to read at full precision",
                             source.raster);
        }
        gp::DatasetPath path;
        path.path = utf8Of(raster->source);
        return DeferredDataset([path]() -> Result<gp::DatasetValue> { return gp::DatasetValue(path); });
    }
    case Source::Kind::Surface: {
        const katana::terrain::NamedSurface* named = context.surfaces.find(source.surface);
        if (named == nullptr) {
            return makeError(ErrorCode::NotFound, "no surface has that name", source.surface);
        }
        std::shared_ptr<const katana::terrain::TinSurface> surface = named->surface;
        const double cell =
            source.cell.value_or(katana::interop::suggestedCellSize(surface->bounds()));
        const std::string crs = projectCrs(context);
        return DeferredDataset([surface, cell, crs]() -> Result<gp::DatasetValue> {
            auto grid = igeo::surfaceGrid(*surface, cell);
            if (!grid) {
                return grid.error();
            }
            grid->info.projectionWkt = crs;
            return gp::DatasetValue(std::move(grid).value());
        });
    }
    case Source::Kind::File: {
        gp::DatasetPath path;
        path.path = source.path;
        path.layer = source.layer;
        return DeferredDataset([path]() -> Result<gp::DatasetValue> { return gp::DatasetValue(path); });
    }
    case Source::Kind::Drawing:
        break;
    }
    return makeError(ErrorCode::InvalidArgument, "the drawing is bound by its scope, not as a raster");
}

std::string inputRecord(std::string_view arg, const Source& source)
{
    std::string record = "input arg=" + value(arg);
    switch (source.kind) {
    case Source::Kind::Drawing:
        record += " source=drawing";
        break;
    case Source::Kind::Raster:
        record += " source=raster name=" + value(source.raster);
        break;
    case Source::Kind::Surface:
        record += " source=surface name=" + value(source.surface) +
                  " cell=" + (source.cell ? katana::core::formatExactReal(*source.cell) : "auto");
        break;
    case Source::Kind::File:
        record += " source=file file=" + value(source.path);
        if (!source.layer.empty()) {
            record += " layer=" + value(source.layer);
        }
        break;
    }
    return record;
}

std::string scopeRecord(std::string_view arg, const BoundDrawing& bound)
{
    const igeo::DrawingDatasetStats& stats = bound.dataset.stats;
    std::string record = "scope arg=" + value(arg) + " " + katana::cad::scopeRecord(bound.match) +
                         " used=" + std::to_string(stats.used) +
                         " points=" + std::to_string(stats.points) +
                         " lines=" + std::to_string(stats.lines) +
                         " polygons=" + std::to_string(stats.polygons);
    for (const auto& [reason, count] : stats.skipped) {
        record += " skipped." + reason + "=" + std::to_string(count);
    }
    return record;
}

Result<std::string> applyOutputs(Context& context, const ApplyRequest& request,
                                 gp::RunOutputs&& outputs)
{
    const Target& target = request.target;
    std::vector<std::string> records;
    const auto unsupported = [](const std::string& why) {
        return makeError(ErrorCode::Unsupported, why);
    };
    switch (target.kind) {
    case Target::Kind::Surface: {
        // ---- T0: Terrain session: TO SURFACE ----
        // A raster result triangulated as SURFACE FROM RASTER triangulates a
        // DEM - its true values on the stride that keeps it under the cap -
        // and kept under the name TO gives, which must be free. The raster
        // was spilled to a file of the run's (maxMemoryCells 0), removed once
        // read; a grid kept in memory is written first, as for REFERENCE.
        if (outputs.features) {
            return unsupported("features cannot be a surface; SURFACE FROM LAYERS <layer> "
                               "triangulates what TO LAYER draws");
        }
        if (outputs.raster) {
            auto file = writeGrid(context, *outputs.raster);
            if (!file) {
                return file.error();
            }
            outputs.file = utf8Of(*file);
            outputs.raster.reset();
        }
        if (!outputs.file) {
            return unsupported("the run made no raster to triangulate");
        }
        const std::filesystem::path raster = pathOf(*outputs.file);
        if (target.name.empty() || context.surfaces.find(target.name) != nullptr) {
            std::error_code removed;
            std::filesystem::remove(raster, removed);
            return makeError(ErrorCode::AlreadyExists,
                             "TO SURFACE needs a name no surface has; SURFACE LIST names them",
                             target.name);
        }
        auto built = surfaceFromRasterFile(raster, kSurfacePointCap, std::nullopt);
        std::error_code removed;
        std::filesystem::remove(raster, removed);
        if (!built) {
            return built.error();
        }
        auto kept = keepSurface(context, target.name, request.result.commandName, false,
                                std::move(built).value());
        if (!kept) {
            return kept.error();
        }
        std::string reply = "output arg=" + value(request.arg) +
                            " kind=raster target=surface name=" + value(target.name) + "\n" + *kept;
        for (const gp::Diagnostic& diagnostic : outputs.diagnostics) {
            if (!diagnostic.failure) {
                reply += "\n" + warningRecord(diagnostic.message);
            }
        }
        return reply;
    }
    case Target::Kind::Selection:
    case Target::Kind::Report:
        // ---- V5: GIS SQL: TO SELECTION and TO REPORT ----
        return unsupported("TO SELECTION and TO REPORT are not available yet: a result goes to "
                           "a layer, a reference raster or a file");
    case Target::Kind::Default:
    case Target::Kind::Layer:
    case Target::Kind::Reference:
    case Target::Kind::File:
        break;
    }

    // A grid kept in memory goes where a raster result is kept.
    if (outputs.raster) {
        if (target.kind == Target::Kind::Layer || target.kind == Target::Kind::File) {
            return unsupported("a raster result cannot go to a layer; TO REFERENCE keeps it as a "
                               "reference raster");
        }
        auto file = writeGrid(context, *outputs.raster);
        if (!file) {
            return file.error();
        }
        outputs.file = utf8Of(*file);
        outputs.raster.reset();
    }
    if (outputs.file) {
        if (target.kind == Target::Kind::File) {
            records.push_back("output arg=" + value(request.arg) +
                              " kind=file target=file file=" + value(*outputs.file) +
                              (target.format.empty() ? std::string() : " driver=" + value(target.format)));
        } else if (target.kind == Target::Kind::Layer) {
            return unsupported("a raster result cannot go to a layer; TO REFERENCE keeps it as a "
                               "reference raster");
        } else {
            auto kept = keepDerived(context, request, pathOf(*outputs.file));
            if (!kept) {
                return kept.error();
            }
            records.push_back(*kept);
        }
    }
    if (outputs.features) {
        if (target.kind == Target::Kind::Reference) {
            return unsupported("a vector result cannot be a reference raster; TO LAYER <path> "
                               "draws it");
        }
        igeo::ResultOptions options = request.result;
        if (options.targetLayer.empty()) {
            options.targetLayer =
                target.kind == Target::Kind::Layer ? target.name : "gis/" + request.defaultName;
        }
        auto plan = igeo::resultCommand(context.document.model(), *outputs.features, options);
        if (!plan) {
            return plan.error();
        }
        std::vector<katana::entity::EntityId> created;
        if (plan->command) {
            if (auto status = context.document.execute(std::move(plan->command)); !status) {
                return status.error();
            }
            created = context.document.lastCreatedEntities();
        }
        std::string record = "output arg=" + value(request.arg) + " kind=vector target=layer layer=" +
                             value(options.targetLayer);
        record += " created=" + std::to_string(plan->created) +
                  " updated=" + std::to_string(plan->updated) +
                  " deleted=" + std::to_string(plan->deleted) +
                  " skipped=" + std::to_string(plan->skipped);
        records.push_back(record);
        for (const std::string& warning : plan->warnings) {
            records.push_back(warningRecord(warning));
        }
        if (context.frame && !created.empty()) {
            const katana::geometry::Box2 bounds = boundsOf(context.document, created);
            if (!bounds.empty()) {
                context.frame(bounds);
            }
        }
    }
    if (outputs.text) {
        records.push_back(textRecord(*outputs.text));
    }
    if (outputs.returnCode) {
        records.push_back("return code=" + std::to_string(*outputs.returnCode));
    }
    for (const gp::Diagnostic& diagnostic : outputs.diagnostics) {
        if (!diagnostic.failure) {
            records.push_back(warningRecord(diagnostic.message));
        }
    }
    std::string reply;
    for (const std::string& record : records) {
        reply += (reply.empty() ? "" : "\n") + record;
    }
    return reply;
}

Status unchangedSince(const Context& context, const std::vector<katana::entity::Entity>& copies)
{
    for (const katana::entity::Entity& copy : copies) {
        const katana::entity::Entity* now = context.document.model().entities.find(copy.id);
        if (now == nullptr || !(*now == copy)) {
            return makeError(ErrorCode::InvalidState, "the drawing changed while the job ran",
                             "entity " + std::to_string(copy.id));
        }
    }
    return {};
}

} // namespace katana::app::geo
