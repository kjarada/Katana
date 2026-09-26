// The SURFACE verbs (terrain_verbs.hpp, docs/terrain.md "Surfaces on every
// front end"): the session's named surfaces listed, described, removed, made
// from a raster, a point cloud or the drawing, and written out as a DEM - the
// same on katana_cli, katana_mcp and the window's command line, where the
// window's Surface From and Export Surface as DEM items build these lines.
//
//   SURFACE LIST [JSON]
//   SURFACE INFO <name>
//   SURFACE REMOVE <name>
//   SURFACE FROM RASTER <id|name> | FILE <path> [NAME <n>] [max=<points>] [AREA x0,y0,x1,y1]
//   SURFACE FROM CLOUD <id|name> [NAME <n>] [max=<points>] [classes=2,...]
//   SURFACE FROM <scope> [NAME <n>]
//   SURFACE EXPORT <name> <file> [cell=<m>] [type=Float32|Float64] [cog] [format=<driver>]
//                  [co=K=V]... [OVERWRITE]
//
// FROM reads its source with the one source parser (bindings.hpp), so the
// drawing is taken by the one scope grammar: SURFACE FROM DRAWING WHERE DRAWN
// is what the drawing shows, SURFACE FROM LAYERS ground ONLY one layer. As
// the design had it, DRAWING may also come before a narrower scope - FROM
// DRAWING LAYERS ground - where it names the source and the scope narrows it.
// A CLOUD is read here: a point cloud is no dataset GDAL binds.

#include <algorithm>
#include <cmath>
#include <filesystem>
#include <memory>
#include <string>
#include <system_error>
#include <utility>
#include <vector>

#include <nlohmann/json.hpp>

#include "katana/cad/geo/surface_input.hpp"
#include "katana/core/text.hpp"
#include "katana/gis/gdal_adapter.hpp"
#include "katana/interop/terrain_io.hpp"
#include "replies.hpp"
#include "terrain_verbs.hpp"

namespace katana::app::geo {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

namespace {

constexpr const char* kUsage =
    "SURFACE LIST [JSON] | SURFACE INFO <name> | SURFACE REMOVE <name> | "
    "SURFACE FROM RASTER <id|name> | FILE <path> | CLOUD <id|name> | <scope> [NAME <n>] "
    "[max=<points>] [AREA x0,y0,x1,y1] [classes=2,...] | "
    "SURFACE EXPORT <name> <file> [cell=<m>] [type=Float32|Float64] [cog] [co=K=V]... "
    "[OVERWRITE]";

std::string utf8Of(const std::filesystem::path& path)
{
    const std::u8string text = path.generic_u8string();
    return std::string(text.begin(), text.end());
}

std::filesystem::path pathOf(const std::string& utf8)
{
    return std::filesystem::path(std::u8string(utf8.begin(), utf8.end()));
}

katana::core::Error refusal(const std::string& why, const std::string& word = {})
{
    return makeError(ErrorCode::InvalidArgument, why, word);
}

// A point cloud of the reference data by id or name, as RASTER finds a raster.
const katana::interop::PointCloudLayer* findCloud(const katana::interop::ReferenceData& reference,
                                                  const std::string& word)
{
    for (const katana::interop::PointCloudLayer& cloud : reference.pointClouds()) {
        if (std::to_string(cloud.id) == word) {
            return &cloud;
        }
    }
    for (const katana::interop::PointCloudLayer& cloud : reference.pointClouds()) {
        if (katana::core::equalsIgnoringCase(cloud.name, word)) {
            return &cloud;
        }
    }
    return nullptr;
}

const katana::interop::RasterOverlay* findRaster(const katana::interop::ReferenceData& reference,
                                                 const std::string& word)
{
    for (const katana::interop::RasterOverlay& raster : reference.rasters()) {
        if (std::to_string(raster.id) == word) {
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

std::string boxText(const katana::geometry::Box2& box)
{
    return fixed3(box.min.x) + "," + fixed3(box.min.y) + "," + fixed3(box.max.x) + "," +
           fixed3(box.max.y);
}

// The cell of a raster when it is square and unrotated; nothing otherwise.
std::optional<double> squareCell(const katana::interop::RasterOverlay& raster)
{
    const auto& gt = raster.geotransform;
    if (!raster.hasGeotransform || gt[2] != 0.0 || gt[4] != 0.0 ||
        std::abs(gt[1]) != std::abs(gt[5])) {
        return std::nullopt;
    }
    return std::abs(gt[1]);
}

std::string rasterRecord(const katana::interop::RasterOverlay& raster)
{
    const std::optional<double> cell = squareCell(raster);
    return "raster id=" + std::to_string(raster.id) + " name=" + value(raster.name) +
           " kind=" + katana::interop::toString(raster.role) + " width=" +
           std::to_string(raster.width) + " height=" + std::to_string(raster.height) +
           " cell=" + (cell ? katana::core::formatExactReal(*cell) : std::string()) +
           " file=" + value(utf8Of(raster.source));
}

nlohmann::json surfaceJson(const katana::terrain::NamedSurface& named)
{
    const katana::terrain::TinSurface& surface = *named.surface;
    const katana::geometry::Box2& box = surface.bounds();
    return {{"name", named.name},
            {"triangles", surface.triangleCount()},
            {"points", surface.vertexCount()},
            {"bounds", {box.min.x, box.min.y, box.max.x, box.max.y}},
            {"zmin", surface.minElevation()},
            {"zmax", surface.maxElevation()},
            {"plan_area", surface.planArea()},
            {"source", named.source}};
}

nlohmann::json rasterJson(const katana::interop::RasterOverlay& raster)
{
    const std::optional<double> cell = squareCell(raster);
    nlohmann::json entry{{"id", raster.id},
                         {"name", raster.name},
                         {"role", katana::interop::toString(raster.role)},
                         {"width", raster.width},
                         {"height", raster.height},
                         {"cell", cell ? nlohmann::json(*cell) : nlohmann::json()},
                         {"crs", raster.projectionWkt.empty()
                                     ? nlohmann::json()
                                     : nlohmann::json(katana::gis::describeCrs(raster.projectionWkt))},
                         {"nodata", raster.facts.noData ? nlohmann::json(*raster.facts.noData)
                                                        : nlohmann::json()},
                         {"source", utf8Of(raster.source)},
                         {"derived_from", raster.derivation}};
    return entry;
}

// ---- LIST, INFO, REMOVE ----------------------------------------------------------------------

Result<Prepared> list(Context& context, const Tokens& tokens, std::size_t at)
{
    bool json = false;
    for (; at < tokens.size(); ++at) {
        if (!tokens.is(at, "JSON")) {
            return refusal("SURFACE LIST takes JSON or nothing", tokens[at]);
        }
        json = true;
    }
    if (json) {
        // What katana_terrain_list hands an agent: the surfaces, and the
        // rasters a terrain verb can read as RASTER <id|name>.
        nlohmann::json surfaces = nlohmann::json::array();
        for (const katana::terrain::NamedSurface& named : context.surfaces.all()) {
            surfaces.push_back(surfaceJson(named));
        }
        nlohmann::json rasters = nlohmann::json::array();
        for (const katana::interop::RasterOverlay& raster : context.reference.rasters()) {
            rasters.push_back(rasterJson(raster));
        }
        return answeredWith("SURFACE LIST",
                            nlohmann::json{{"surfaces", surfaces}, {"rasters", rasters}}.dump());
    }
    std::vector<std::string> records;
    for (const katana::terrain::NamedSurface& named : context.surfaces.all()) {
        records.push_back(surfaceRecord(named));
    }
    for (const katana::interop::RasterOverlay& raster : context.reference.rasters()) {
        records.push_back(rasterRecord(raster));
    }
    records.push_back("listed surfaces=" + std::to_string(context.surfaces.all().size()) +
                      " rasters=" + std::to_string(context.reference.rasters().size()));
    return answeredWith("SURFACE LIST", joinedRecords(records));
}

Result<Prepared> info(Context& context, const Tokens& tokens, std::size_t at)
{
    if (at + 1 != tokens.size()) {
        return refusal("SURFACE INFO names one surface: SURFACE INFO <name>");
    }
    const katana::terrain::NamedSurface* named = context.surfaces.find(tokens[at]);
    if (named == nullptr) {
        return makeError(ErrorCode::NotFound, "no surface has that name; SURFACE LIST names them",
                         tokens[at]);
    }
    const katana::terrain::TinSurface& surface = *named->surface;
    return answeredWith("SURFACE INFO",
                        surfaceRecord(*named) + "\narea plan=" + fixed3(surface.planArea()) +
                            " surface=" + fixed3(surface.surfaceArea()));
}

Result<Prepared> remove(Context& context, const Tokens& tokens, std::size_t at)
{
    if (at + 1 != tokens.size()) {
        return refusal("SURFACE REMOVE names one surface: SURFACE REMOVE <name>");
    }
    const katana::terrain::NamedSurface* named = context.surfaces.find(tokens[at]);
    if (named == nullptr) {
        return makeError(ErrorCode::NotFound, "no surface has that name; SURFACE LIST names them",
                         tokens[at]);
    }
    const std::string name = named->name;
    // Removed as the line runs, as a reference raster is: a surface is
    // session data, and there is nothing to wait for.
    (void)context.surfaces.remove(name);
    if (context.changed) {
        context.changed();
    }
    return answeredWith("SURFACE REMOVE", "removed surface=" + value(name) +
                                              " surfaces=" +
                                              std::to_string(context.surfaces.all().size()));
}

// ---- FROM ---------------------------------------------------------------------------------------

struct FromWords {
    std::optional<std::string> name;
    std::optional<std::size_t> maxPoints;
    std::optional<katana::geometry::Box2> area;
    std::vector<std::uint8_t> classes;
    bool classesGiven = false;
    bool preview = false;
};

Result<std::vector<std::uint8_t>> classList(const std::string& text)
{
    std::vector<std::uint8_t> classes;
    std::size_t start = 0;
    while (start <= text.size()) {
        const std::size_t comma = text.find(',', start);
        const std::string item =
            text.substr(start, comma == std::string::npos ? std::string::npos : comma - start);
        const auto number = katana::core::parseInteger(item);
        if (!number || *number < 0 || *number > 255) {
            return makeError(ErrorCode::InvalidArgument,
                             "classes is a list of ASPRS class numbers from 0 to 255: classes=2,8",
                             item);
        }
        classes.push_back(static_cast<std::uint8_t>(*number));
        if (comma == std::string::npos) {
            break;
        }
        start = comma + 1;
    }
    return classes;
}

// The words after the source, any order: NAME <n>, max=, classes=, AREA and
// PREVIEW. AREA is read by the one scope parser, so it is written as a scope's
// AREA is; for a raster it is a window, not a scope.
Result<FromWords> fromWords(const Tokens& tokens, std::size_t at)
{
    FromWords words;
    while (at < tokens.size()) {
        if (tokens.is(at, "NAME")) {
            if (at + 1 >= tokens.size() || tokens[at + 1].empty()) {
                return refusal("NAME needs the surface's name");
            }
            words.name = tokens[at + 1];
            at += 2;
        } else if (tokens.is(at, "PREVIEW")) {
            words.preview = true;
            ++at;
        } else if (tokens.is(at, "AREA")) {
            auto scope = katana::cad::parseScopeWords(tokens.words, at);
            if (!scope) {
                return scope.error();
            }
            if (scope->source != katana::cad::ScopeSource::Area || !scope->filter.empty()) {
                return refusal("a raster is cut by AREA x0,y0,x1,y1 alone");
            }
            words.area = scope->area;
        } else if (const auto option = keyValue(tokens, at)) {
            if (option->first == "max") {
                const auto number = katana::core::parseInteger(option->second);
                if (!number || *number < 3) {
                    return refusal("max is how many points at most, 3 or more", option->second);
                }
                words.maxPoints = static_cast<std::size_t>(*number);
            } else if (option->first == "classes") {
                auto classes = classList(option->second);
                if (!classes) {
                    return classes.error();
                }
                words.classes = std::move(classes).value();
                words.classesGiven = true;
            } else {
                return refusal("SURFACE FROM takes NAME, max=, classes=, AREA and PREVIEW",
                               tokens[at]);
            }
            ++at;
        } else {
            return refusal("SURFACE FROM takes NAME, max=, classes=, AREA and PREVIEW",
                           tokens[at]);
        }
    }
    return words;
}

// What a surface made at apply is called: the name asked for, which must be
// free, or the source's own made unique.
struct Naming {
    std::string name;
    bool unique = true;
};

Result<Naming> namingFor(const Context& context, const FromWords& words, std::string fallback)
{
    if (words.name) {
        if (context.surfaces.find(*words.name) != nullptr) {
            return makeError(ErrorCode::AlreadyExists,
                             "a surface already has that name; SURFACE REMOVE it first, or name "
                             "this one otherwise",
                             *words.name);
        }
        return Naming{*words.name, false};
    }
    return Naming{fallback.empty() ? std::string("surface") : std::move(fallback), true};
}

// The job that triangulates `input` and the apply that keeps it.
Prepared buildJob(std::string title, std::vector<std::string> records, Naming naming,
                  std::string source, std::function<Result<SurfaceBuild>(const std::stop_token&)> make)
{
    Prepared prepared;
    prepared.title = std::move(title);
    prepared.work = [records, naming, source, make](const std::stop_token& stop,
                                                    const Progress&) -> Result<Apply> {
        auto built = make(stop);
        if (!built) {
            return built.error();
        }
        auto kept = std::make_shared<SurfaceBuild>(std::move(built).value());
        return Apply([records, naming, source, kept](Context& context) -> Result<std::string> {
            auto reply = keepSurface(context, naming.name, source, naming.unique, std::move(*kept));
            if (!reply) {
                return reply.error();
            }
            std::vector<std::string> all = records;
            all.push_back(*reply);
            return joinedRecords(all);
        });
    };
    return prepared;
}

Result<Prepared> fromRaster(Context& context, const Source& source, const FromWords& words)
{
    if (words.classesGiven) {
        return refusal("classes= is a point cloud's; a raster has no classes");
    }
    std::filesystem::path path;
    std::string name;
    if (source.kind == Source::Kind::Raster) {
        const katana::interop::RasterOverlay* raster = findRaster(context.reference, source.raster);
        if (raster == nullptr) {
            return makeError(ErrorCode::NotFound,
                             "no reference raster has that id or name; SURFACE LIST lists them",
                             source.raster);
        }
        if (raster->source.empty()) {
            // A raster fetched from a web service and held only as pixels:
            // its heights were never kept.
            return makeError(ErrorCode::Unsupported,
                             "the reference raster has no file to read its true values from",
                             source.raster);
        }
        path = raster->source;
        name = raster->name;
    } else {
        path = pathOf(source.path);
        name = path.stem().string();
    }
    auto naming = namingFor(context, words, name);
    if (!naming) {
        return naming.error();
    }
    std::vector<std::string> records{inputRecord("input", source)};
    if (words.area) {
        records.push_back("window area=" + boxText(*words.area));
    }
    if (words.preview) {
        records.push_back("preview valid=yes changed=no");
        return answeredWith("SURFACE FROM", joinedRecords(records));
    }
    const std::size_t cap = words.maxPoints.value_or(kSurfacePointCap);
    const std::optional<katana::geometry::Box2> area = words.area;
    // Read on the job's thread, from the file: a path is all the job takes,
    // so the reference raster may go away meanwhile.
    return buildJob("Surface From Raster", records, *naming, "raster " + path.filename().string(),
                    [path, cap, area](const std::stop_token&) {
                        return surfaceFromRasterFile(path, cap, area);
                    });
}

// Every step-th point, the step the smallest that brings them under the cap;
// rounded up, so the result is at the cap or under it, never just over.
std::size_t thinningFor(std::size_t available, std::size_t cap)
{
    return available <= cap ? 1 : (available + cap - 1) / cap;
}

Result<Prepared> fromCloud(Context& context, const std::string& word, const FromWords& words)
{
    if (words.area) {
        return refusal("AREA cuts a raster; a point cloud is imported for the area it needs");
    }
    const katana::interop::PointCloudLayer* cloud = findCloud(context.reference, word);
    if (cloud == nullptr) {
        return makeError(ErrorCode::NotFound,
                         "no point cloud has that id or name; REFS lists the reference layers",
                         word);
    }
    auto naming = namingFor(context, words, cloud->name);
    if (!naming) {
        return naming.error();
    }
    // Copied here, on the calling thread, because the cloud belongs to the
    // reference data and could be removed while the job runs.
    auto points = std::make_shared<katana::interop::CloudSurfacePoints>(
        katana::interop::surfacePoints(*cloud, words.classes));
    std::string classes;
    for (const std::uint8_t each : words.classes) {
        classes += (classes.empty() ? "" : ",") + std::to_string(each);
    }
    std::vector<std::string> records{
        "input arg=input source=cloud name=" + value(cloud->name) + " id=" +
        std::to_string(cloud->id) + " points=" + std::to_string(points->points.size()) +
        " ground=" + (points->groundOnly ? "yes" : "no") +
        " excluded=" + std::to_string(points->excluded) + " classes=" +
        (words.classesGiven ? classes : points->groundOnly ? std::string("2") : std::string("all"))};
    if (!words.classesGiven && !points->groundOnly) {
        // Said, because a canopy surface presented as ground is the silent
        // failure audit QT-10 found.
        records.push_back(warningRecord(
            "the cloud has no return classified as ground (ASPRS class 2), so every return is "
            "triangulated: vegetation and buildings are part of the surface"));
    }
    const std::size_t cap = words.maxPoints.value_or(kSurfacePointCap);
    const std::size_t step = thinningFor(points->points.size(), cap);
    if (step > 1) {
        records.push_back("thinned from=" + std::to_string(points->points.size()) +
                          " step=" + std::to_string(step) +
                          " points=" + std::to_string((points->points.size() + step - 1) / step));
    }
    if (words.preview) {
        records.push_back("preview valid=yes changed=no");
        return answeredWith("SURFACE FROM", joinedRecords(records));
    }
    const std::string source = "point cloud " + cloud->name;
    return buildJob("Surface From Point Cloud", records, *naming, source,
                    [points, step](const std::stop_token&) -> Result<SurfaceBuild> {
                        katana::terrain::TinInput input;
                        input.points.reserve(points->points.size() / step + 1);
                        for (std::size_t i = 0; i < points->points.size(); i += step) {
                            input.points.push_back(points->points[i]);
                        }
                        katana::terrain::TinBuildOptions options;
                        // Scanned points land on one ground mark again and
                        // again, their heights millimetres apart: the mean is
                        // taken and the merges counted.
                        options.duplicatePoints = katana::terrain::DuplicatePointPolicy::Average;
                        return triangulate(std::move(input), options);
                    });
}

Result<Prepared> fromDrawing(Context& context, const Source& source, const FromWords& words)
{
    if (words.area || words.maxPoints || words.classesGiven) {
        return refusal("the drawing's surface is narrowed by its scope and filter; max=, "
                       "classes= and a window are a raster's and a cloud's");
    }
    auto match = katana::cad::matchScope(context.document, source.scope,
                                         context.interpreter.scopeContext());
    if (!match) {
        return match.error();
    }
    // Read here, on the calling thread: the document is single-threaded, and
    // from here the job owns what it triangulates.
    auto input = std::make_shared<katana::cad::geo::SurfaceInput>(
        katana::cad::geo::surfaceInput(context.document.model(), match->matched));
    std::string record = "scope arg=input " + katana::cad::scopeRecord(*match) +
                         " used=" + std::to_string(input->used) +
                         " points=" + std::to_string(input->input.points.size()) +
                         " breaklines=" + std::to_string(input->input.breaklines.size()) +
                         " vertices.heightless=" + std::to_string(input->heightlessVertices);
    for (const auto& [reason, count] : input->skipped) {
        record += " skipped." + reason + "=" + std::to_string(count);
    }
    std::vector<std::string> records{"input arg=input source=drawing", record};
    if (input->heightlessVertices != 0) {
        records.push_back(warningRecord(
            std::to_string(input->heightlessVertices) +
            " vertices have no height ('elevation' or 'elevations' property) and were left out"));
    }
    auto naming = namingFor(context, words, "drawing");
    if (!naming) {
        return naming.error();
    }
    if (words.preview) {
        records.push_back("preview valid=yes changed=no");
        return answeredWith("SURFACE FROM", joinedRecords(records));
    }
    if (input->input.points.size() < 3) {
        // Refused here, with what the scope took, rather than after a job:
        // there is nothing to triangulate.
        return makeError(ErrorCode::InvalidArgument,
                         "fewer than three points with a height to triangulate: " +
                             joinedRecords(records));
    }
    const std::string sourceText =
        "drawing (" + std::to_string(input->used) + " entities)";
    return buildJob("Surface From Drawing", records, *naming, sourceText,
                    [input](const std::stop_token&) -> Result<SurfaceBuild> {
                        katana::terrain::TinBuildOptions options;
                        options.duplicatePoints = katana::terrain::DuplicatePointPolicy::Average;
                        options.crossingBreaklines =
                            katana::terrain::CrossingBreaklinePolicy::Average;
                        return triangulate(std::move(input->input), options);
                    });
}

Result<Prepared> from(Context& context, const Tokens& tokens, std::size_t at)
{
    if (at >= tokens.size()) {
        return refusal("SURFACE FROM needs a source: RASTER <id|name>, FILE <path>, CLOUD "
                       "<id|name> or a scope of the drawing (DRAWING, SELECTION, LAYERS ...)");
    }
    if (tokens.is(at, "CLOUD")) {
        if (at + 1 >= tokens.size()) {
            return refusal("CLOUD needs the point cloud's id or name");
        }
        auto words = fromWords(tokens, at + 2);
        if (!words) {
            return words.error();
        }
        return fromCloud(context, tokens[at + 1], *words);
    }
    // DRAWING before a narrower scope names the source; the scope narrows it.
    if (tokens.is(at, "DRAWING") && at + 1 < tokens.size() && !tokens.quoted[at + 1] &&
        katana::cad::isScopeWord(tokens[at + 1]) && !tokens.is(at + 1, "WHERE")) {
        ++at;
    }
    if (tokens.is(at, "SURFACE")) {
        return refusal("a surface is made from a raster, a point cloud or the drawing");
    }
    auto source = parseSource(tokens, at);
    if (!source) {
        return source.error();
    }
    auto words = fromWords(tokens, at);
    if (!words) {
        return words.error();
    }
    if (source->kind == Source::Kind::Drawing) {
        return fromDrawing(context, *source, *words);
    }
    return fromRaster(context, *source, *words);
}

// ---- EXPORT -------------------------------------------------------------------------------------

Result<Prepared> exportDem(Context& context, const Tokens& tokens, std::size_t at)
{
    if (at + 2 > tokens.size()) {
        return refusal("SURFACE EXPORT <name> <file> [cell=<m>] [type=Float32|Float64] [cog] "
                       "[co=K=V]... [OVERWRITE]");
    }
    const katana::terrain::NamedSurface* named = context.surfaces.find(tokens[at]);
    if (named == nullptr) {
        return makeError(ErrorCode::NotFound, "no surface has that name; SURFACE LIST names them",
                         tokens[at]);
    }
    const std::filesystem::path path = pathOf(tokens[at + 1]);
    katana::interop::SurfaceRasterOptions options;
    options.cellSize = katana::interop::suggestedCellSize(named->surface->bounds());
    options.projectionWkt = projectCrs(context);
    bool preview = false;
    for (at += 2; at < tokens.size(); ++at) {
        if (tokens.is(at, "OVERWRITE")) {
            options.overwrite = true;
        } else if (tokens.is(at, "COG")) {
            options.cog = true;
        } else if (tokens.is(at, "PREVIEW")) {
            preview = true;
        } else if (const auto option = keyValue(tokens, at)) {
            if (option->first == "cell") {
                auto cell = positiveOption("cell", option->second);
                if (!cell) {
                    return cell.error();
                }
                options.cellSize = *cell;
            } else if (option->first == "type") {
                const std::string type = katana::core::lowered(option->second);
                if (type != "float32" && type != "float64") {
                    return refusal("type is Float32 or Float64", option->second);
                }
                options.dataType = type == "float32" ? "Float32" : "Float64";
            } else if (option->first == "format") {
                options.driver = option->second;
            } else if (option->first == "co") {
                if (option->second.find('=') == std::string::npos) {
                    return refusal("co= takes a creation option: co=KEY=VALUE", option->second);
                }
                options.creationOptions.push_back(option->second);
            } else {
                return refusal("SURFACE EXPORT takes cell=, type=, format=, co=, COG, OVERWRITE "
                               "and PREVIEW",
                               tokens[at]);
            }
        } else {
            return refusal("SURFACE EXPORT takes cell=, type=, format=, co=, COG, OVERWRITE and "
                           "PREVIEW",
                           tokens[at]);
        }
    }
    // Refused now rather than after a job: a file in the way is the line's
    // to allow, and every front end asks the same way (TO FILE's rule).
    std::error_code error;
    if (!options.overwrite && std::filesystem::exists(path, error)) {
        return makeError(ErrorCode::AlreadyExists, "the file exists; add OVERWRITE to replace it",
                         utf8Of(path));
    }
    const katana::geometry::Box2& bounds = named->surface->bounds();
    const double columns = std::max(1.0, std::ceil(bounds.width() / options.cellSize));
    const double rows = std::max(1.0, std::ceil(bounds.height() / options.cellSize));
    const std::string grid = katana::core::formatExactReal(columns) + "x" +
                             katana::core::formatExactReal(rows);
    if (preview) {
        return answeredWith("SURFACE EXPORT",
                            "export surface=" + value(named->name) + " file=" + value(utf8Of(path)) +
                                " raster=" + grid + " cell=" +
                                katana::core::formatExactReal(options.cellSize) +
                                " type=" + options.dataType + "\npreview valid=yes changed=no");
    }
    std::shared_ptr<const katana::terrain::TinSurface> surface = named->surface;
    const std::string name = named->name;
    Prepared prepared;
    prepared.title = "Export Surface as DEM";
    prepared.work = [surface, name, path, options](const std::stop_token& stop,
                                                   const Progress&) -> Result<Apply> {
        auto written = katana::interop::exportSurfaceRaster(*surface, path, options, stop);
        if (!written) {
            return written.error();
        }
        std::string record =
            "exported file=" + value(utf8Of(path)) + " driver=" + written->driver +
            " surface=" + value(name) + " raster=" + std::to_string(written->columns) + "x" +
            std::to_string(written->rows) +
            " cell=" + katana::core::formatExactReal(options.cellSize) + " type=" +
            written->dataType + " cells=" + std::to_string(written->cellsWithData) +
            " nodata=" + katana::core::formatExactReal(written->noDataValue) +
            " crs=" + value(options.projectionWkt.empty()
                                ? std::string("none")
                                : katana::gis::describeCrs(options.projectionWkt));
        return Apply([record](Context&) -> Result<std::string> { return record; });
    };
    return prepared;
}

} // namespace

// ---- shared with the other terrain verbs --------------------------------------------------------

std::string surfaceRecord(const katana::terrain::NamedSurface& named)
{
    const katana::terrain::TinSurface& surface = *named.surface;
    return "surface name=" + value(named.name) +
           " triangles=" + std::to_string(surface.triangleCount()) +
           " points=" + std::to_string(surface.vertexCount()) +
           " bounds=" + boxText(surface.bounds()) + " zmin=" + fixed3(surface.minElevation()) +
           " zmax=" + fixed3(surface.maxElevation()) + " source=" + value(named.source);
}

Result<SurfaceBuild> triangulate(katana::terrain::TinInput input,
                                 const katana::terrain::TinBuildOptions& options)
{
    if (input.points.size() < 3) {
        return makeError(ErrorCode::InvalidArgument,
                         "fewer than three points with a height to triangulate");
    }
    auto built = katana::terrain::buildTin(input, options);
    if (!built) {
        return built.error();
    }
    SurfaceBuild build;
    if (built->report.duplicatePointCount > 0) {
        build.records.push_back("merged duplicates=" +
                                std::to_string(built->report.duplicatePointCount) +
                                " policy=average");
    }
    build.surface =
        std::make_shared<const katana::terrain::TinSurface>(std::move(built->surface));
    return build;
}

Result<SurfaceBuild> surfaceFromRasterFile(const std::filesystem::path& path, std::size_t maxPoints,
                                           const std::optional<katana::geometry::Box2>& area)
{
    // The band's TRUE values, read again from the file through GDAL - never
    // the 8-bit display copy a reference raster holds (audit QT-23) - on a
    // stride that keeps the whole extent under the cap (audit QT-24).
    katana::interop::RasterElevationOptions options;
    options.maxPoints = maxPoints;
    options.area = area;
    auto elevations = katana::interop::readRasterElevations(path, options);
    if (!elevations) {
        return elevations.error();
    }
    std::vector<std::string> records{
        "sampled stride=" + std::to_string(elevations->stride) +
        " pixels=" + std::to_string(elevations->sampled) +
        " nodata=" + std::to_string(elevations->noData) +
        " points=" + std::to_string(elevations->points.size())};
    if (elevations->points.size() < 3) {
        return makeError(ErrorCode::InvalidArgument,
                         "fewer than three pixels with an elevation to triangulate",
                         records.front());
    }
    katana::terrain::TinInput input;
    input.points = std::move(elevations->points);
    katana::terrain::TinBuildOptions build;
    build.duplicatePoints = katana::terrain::DuplicatePointPolicy::Average;
    auto made = triangulate(std::move(input), build);
    if (!made) {
        return made.error();
    }
    made->records.insert(made->records.begin(), records.begin(), records.end());
    return made;
}

Result<std::string> keepSurface(Context& context, std::string name, std::string source,
                                bool unique, SurfaceBuild&& build)
{
    if (unique) {
        name = context.surfaces.uniqueName(name);
    }
    if (auto added = context.surfaces.add({name, build.surface, std::move(source)}); !added) {
        return added.error();
    }
    if (context.changed) {
        context.changed();
    }
    const katana::terrain::NamedSurface* kept = context.surfaces.find(name);
    std::vector<std::string> records = std::move(build.records);
    records.push_back(surfaceRecord(*kept));
    return joinedRecords(records);
}

std::optional<std::pair<std::string, std::string>> keyValue(const Tokens& tokens, std::size_t i)
{
    if (i >= tokens.size()) {
        return std::nullopt;
    }
    const std::string& word = tokens[i];
    const std::size_t equals = word.find('=');
    if (equals == std::string::npos || equals == 0 || word.front() == '-') {
        return std::nullopt;
    }
    return std::pair{katana::core::lowered(word.substr(0, equals)), word.substr(equals + 1)};
}

Result<double> numberOption(std::string_view key, std::string_view value)
{
    const auto number = katana::core::parseFiniteDouble(value);
    if (!number) {
        return makeError(ErrorCode::InvalidArgument, std::string(key) + " is a number",
                         std::string(value));
    }
    return *number;
}

Result<double> positiveOption(std::string_view key, std::string_view value)
{
    auto number = numberOption(key, value);
    if (!number) {
        return number;
    }
    if (!(*number > 0.0)) {
        return makeError(ErrorCode::InvalidArgument, std::string(key) + " is greater than zero",
                         std::string(value));
    }
    return number;
}

std::string joinedRecords(const std::vector<std::string>& records)
{
    std::string text;
    for (const std::string& record : records) {
        if (!record.empty()) {
            text += (text.empty() ? "" : "\n") + record;
        }
    }
    return text;
}

Prepared answeredWith(std::string title, std::string reply)
{
    Prepared prepared;
    prepared.title = std::move(title);
    prepared.reply = std::move(reply);
    return prepared;
}

Result<Prepared> prepareSurface(Context& context, const Tokens& tokens, std::string_view)
{
    constexpr std::size_t at = 1;
    if (at >= tokens.size()) {
        return refusal(std::string("usage: ") + kUsage);
    }
    if (tokens.is(at, "LIST")) {
        return list(context, tokens, at + 1);
    }
    if (tokens.is(at, "INFO")) {
        return info(context, tokens, at + 1);
    }
    if (tokens.is(at, "REMOVE")) {
        return remove(context, tokens, at + 1);
    }
    if (tokens.is(at, "FROM")) {
        return from(context, tokens, at + 1);
    }
    if (tokens.is(at, "EXPORT")) {
        return exportDem(context, tokens, at + 1);
    }
    return refusal(std::string("SURFACE takes LIST, INFO, REMOVE, FROM or EXPORT: ") + kUsage,
                   tokens[at]);
}

std::string surfaceUsage()
{
    return kUsage;
}

} // namespace katana::app::geo
