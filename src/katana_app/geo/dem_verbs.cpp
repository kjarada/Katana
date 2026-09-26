// The DEM tools: RASTER MOSAIC, CLIP, FILL, FOOTPRINT, REPROJECT and
// DIFFERENCE (docs/terrain.md, "The DEM tools"; docs/geoprocessing.md, "T7").
//
//   RASTER MOSAIC <raster> [<raster>...] [resolution=same|highest|lowest|average|<x>,<y>]
//                 [SAVE <file>]
//   RASTER CLIP <raster> <scope>              to AREA's box, or to the closed
//                                             boundaries any other scope takes
//   RASTER FILL <raster> [distance=<cells>] [smoothing=<n>] [strategy=invdist|nearest]
//   RASTER FOOTPRINT <raster>                 [TO LAYER <path> | TO FILE <path>]
//   RASTER REPROJECT <raster> [crs=<crs> | like=<raster id|name>] [from=<crs>]
//                    [resampling=<method>] [cell=<m>]
//   RASTER DIFFERENCE <raster> <raster> [<scope>] [resampling=<method>]
//   and on each: [NAME <name>] [TO REFERENCE [<name>] | TO FILE <path> [FORMAT <driver>]]
//                [OVERWRITE] [PREVIEW]
//
//   <raster> := RASTER <id|name> | SURFACE <name> [CELL <m>] | FILE <path>
//
// Each is a thin module over the one executor: its sources and targets are
// read by the ONE source and target parsers (bindings.hpp), a scope by the ONE
// scope parser, and its result applied by applyOutputs - a raster as a derived
// reference raster, features as one undo step. The rasters are GDAL's
// algorithms (`raster mosaic`, `clip`, `fill-nodata`, `footprint`,
// `reproject`, `calc`), pinned by tests/geo/test_dem_verbs.cpp's contract
// test.
//
// Decisions (docs/terrain.md has the reasons):
//   - A mosaic is a VRT: light, and its tiles are read where they are. SAVE
//     writes it out as a file of its own, kept as the reference raster. A
//     keyword, not save=: an option's value is one unquoted word, and a path
//     may hold blanks.
//   - CLIP's AREA is the box itself; any other scope clips to the closed
//     boundaries it takes (GDAL's cutline: a cell is kept when its centre is
//     inside).
//   - REPROJECT refuses a raster with no coordinate system rather than
//     reprojecting from nowhere: GDAL does so without a word (measured: a
//     plane with no CRS "reprojected" to EPSG:28356 came back unchanged).
//   - DIFFERENCE aligns the second raster to the first's grid (`raster
//     reproject`, bilinear) only when their grids differ, subtracts with `raster
//     calc`'s builtin diff (this GDAL has neither muparser nor ExprTk), and
//     sums cut and fill with a compensated sum times the cell area.

#include <algorithm>
#include <array>
#include <atomic>
#include <chrono>
#include <cmath>
#include <cstdint>
#include <filesystem>
#include <map>
#include <memory>
#include <optional>
#include <span>
#include <string>
#include <string_view>
#include <system_error>
#include <utility>
#include <variant>
#include <vector>

#include "bindings.hpp"
#include "dem_support.hpp"
#include "geo_verbs.hpp"
#include "katana/cad/global_modify.hpp"
#include "katana/core/text.hpp"
#include "katana/interop/geo/raster_products.hpp"
#include "katana/interop/terrain_io.hpp"
#include "katana/math/summation.hpp"
#include "replies.hpp"
#include "verb_table.hpp"

namespace katana::app::geo {

namespace gp = katana::gis::processing;
namespace igeo = katana::interop::geo;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;

namespace {

constexpr const char* kRasterSources = "RASTER <id|name>, SURFACE <name> [CELL <m>] or FILE <path>";

// What one DEM verb takes.
struct DemGrammar {
    std::string_view verb;                  // "RASTER CLIP"
    std::span<const std::string_view> keys; // its key=value options
    std::size_t minSources = 1;
    std::size_t maxSources = 1;
    bool takesScope = false;
    bool takesSurface = true;
    bool takesSave = false;
};

// A DEM verb's line, read.
struct DemLine {
    std::map<std::string, std::string> options;
    std::vector<Source> sources;
    std::optional<katana::cad::ScopeWords> scope;
    std::optional<Target> target;
    std::string name;
    std::optional<std::string> save;
    bool preview = false;
    bool overwrite = false;

    [[nodiscard]] std::optional<std::string> option(const char* key) const
    {
        const auto found = options.find(key);
        return found == options.end() ? std::nullopt : std::optional(found->second);
    }
};

Result<DemLine> readLine(const Tokens& tokens, const DemGrammar& grammar)
{
    auto words = splitOptions(tokens, 2, grammar.keys, grammar.verb);
    if (!words) {
        return words.error();
    }
    DemLine line;
    line.options = std::move(words->options);
    const Tokens& rest = words->rest;
    const std::string verb(grammar.verb);
    for (std::size_t at = 0; at < rest.size();) {
        if (rest.is(at, "RASTER") || rest.is(at, "SURFACE") || rest.is(at, "FILE")) {
            if (line.sources.size() == grammar.maxSources) {
                return makeError(ErrorCode::InvalidArgument,
                                 verb + " takes " + std::to_string(grammar.maxSources) +
                                     (grammar.maxSources == 1 ? " raster" : " rasters"),
                                 rest[at]);
            }
            if (rest.is(at, "SURFACE") && !grammar.takesSurface) {
                return makeError(ErrorCode::InvalidArgument,
                                 verb + " takes rasters and files; a surface is no tile - "
                                        "RASTER <id|name> or FILE <path|folder|pattern>",
                                 rest[at]);
            }
            auto source = parseSource(rest, at);
            if (!source) {
                return source.error();
            }
            line.sources.push_back(std::move(source).value());
        } else if (rest.is(at, "NAME")) {
            if (at + 1 >= rest.size() || !line.name.empty()) {
                return makeError(ErrorCode::InvalidArgument,
                                 "NAME takes the reference raster's name, once", rest[at]);
            }
            line.name = rest[at + 1];
            at += 2;
        } else if (grammar.takesSave && rest.is(at, "SAVE")) {
            if (at + 1 >= rest.size() || line.save) {
                return makeError(ErrorCode::InvalidArgument, "SAVE takes the file to write, once",
                                 rest[at]);
            }
            line.save = rest[at + 1];
            at += 2;
        } else if (rest.is(at, "TO")) {
            if (line.target) {
                return makeError(ErrorCode::InvalidArgument, "one TO per line");
            }
            ++at;
            auto target = parseTarget(rest, at);
            if (!target) {
                return target.error();
            }
            line.target = std::move(target).value();
        } else if (rest.is(at, "PREVIEW")) {
            line.preview = true;
            ++at;
        } else if (rest.is(at, "OVERWRITE")) {
            line.overwrite = true;
            ++at;
        } else if (grammar.takesScope && !line.scope && !rest.quoted[at] &&
                   katana::cad::isScopeWord(rest[at])) {
            auto scope = katana::cad::parseScopeWords(rest.words, at);
            if (!scope) {
                return scope.error();
            }
            line.scope = std::move(scope).value();
        } else {
            std::string takes = verb + " takes " + kRasterSources;
            if (grammar.takesScope) {
                takes += ", a scope";
            }
            for (const std::string_view key : grammar.keys) {
                takes += ", " + std::string(key) + "=";
            }
            if (grammar.takesSave) {
                takes += ", SAVE";
            }
            return makeError(ErrorCode::InvalidArgument,
                             takes + ", NAME, TO, OVERWRITE and PREVIEW", rest[at]);
        }
    }
    if (line.sources.size() < grammar.minSources) {
        return makeError(ErrorCode::InvalidArgument,
                         verb + " needs " +
                             (grammar.minSources == 1 ? std::string("a raster")
                                                      : std::to_string(grammar.minSources) +
                                                            " rasters") +
                             ": " + kRasterSources);
    }
    return line;
}

std::string real(double number)
{
    return katana::core::formatExactReal(number);
}

// A cell count option: a whole number from `least`.
Result<long long> wholeOption(std::string_view key, const std::string& text, long long least)
{
    const auto number = katana::core::parseFiniteDouble(text);
    if (!number || *number != std::floor(*number) || *number < static_cast<double>(least) ||
        *number > 1e9) {
        return makeError(ErrorCode::InvalidArgument,
                         std::string(key) + "= is a whole number from " + std::to_string(least),
                         std::string(key) + "=" + text);
    }
    return static_cast<long long>(*number);
}

// The rasters a line reads, bound as the datasets GDAL is given; a surface
// takes the cell its extent suggests when the line gives none, so its input
// record says what was used.
Result<std::vector<DeferredDataset>> bindSources(Context& context, std::vector<Source>& sources)
{
    std::vector<DeferredDataset> bound;
    for (Source& source : sources) {
        if (source.kind == Source::Kind::Drawing) {
            return makeError(ErrorCode::InvalidArgument,
                             "the drawing is no raster here; give " + std::string(kRasterSources));
        }
        if (source.kind == Source::Kind::Surface && !source.cell) {
            if (const katana::terrain::NamedSurface* surface =
                    context.surfaces.find(source.surface)) {
                source.cell = katana::interop::suggestedCellSize(surface->surface->bounds());
            }
        }
        auto dataset = bindRaster(context, source);
        if (!dataset) {
            return dataset.error();
        }
        bound.push_back(std::move(dataset).value());
    }
    return bound;
}

std::vector<std::string> inputRecords(const std::vector<Source>& sources,
                                      const std::vector<std::string>& args)
{
    std::vector<std::string> records;
    for (std::size_t i = 0; i < sources.size(); ++i) {
        records.push_back(inputRecord(i < args.size() ? args[i] : args.back(), sources[i]));
    }
    return records;
}

bool fileInTheWay(const std::string& path)
{
    std::error_code error;
    return std::filesystem::exists(pathOfUtf8(path), error);
}

// Where a raster result goes: a derived reference raster unless TO says
// otherwise, written once where it is kept (the GDAL verb's rule). A layer
// is refused, and so is a file in the way without OVERWRITE.
Result<ApplyRequest> rasterTarget(Context& context, const DemLine& line, std::string_view verb,
                                  const char* defaultName, gp::RunRequest& request,
                                  std::string_view text)
{
    const Target to = line.target.value_or(Target{});
    if (to.kind == Target::Kind::Layer || to.kind == Target::Kind::Selection ||
        to.kind == Target::Kind::Report) {
        return makeError(ErrorCode::Unsupported,
                         std::string(verb) + " makes a raster: TO REFERENCE [<name>] keeps it as "
                                             "a reference raster, TO FILE <path> writes it");
    }
    if (!line.name.empty() && to.kind == Target::Kind::Reference && !to.name.empty()) {
        return makeError(ErrorCode::InvalidArgument,
                         "NAME and TO REFERENCE both name the raster; give one");
    }
    if (to.kind == Target::Kind::File) {
        if (fileInTheWay(to.name) && !line.overwrite) {
            return makeError(ErrorCode::AlreadyExists, "the file exists; add OVERWRITE to replace it",
                             to.name);
        }
        request.outputTo = gp::OutputTo::File;
        request.outputPath = to.name;
        request.outputFormat = to.format;
    } else {
        request.outputTo = gp::OutputTo::Memory;
        request.maxMemoryCells = 0;
        request.spillDirectory = utf8OfPath(derivedFolder(context));
    }
    request.overwrite = line.overwrite;
    ApplyRequest apply;
    apply.target = to;
    if (to.kind == Target::Kind::Default || to.kind == Target::Kind::Reference) {
        apply.target.kind = Target::Kind::Reference;
        apply.target.name = !to.name.empty() ? to.name : line.name;
    }
    apply.defaultName = defaultName;
    apply.result.operation = gp::pathText(request.path);
    apply.result.commandName = std::string(katana::core::trimmed(text));
    return apply;
}

Prepared answered(std::string title, std::string reply)
{
    Prepared prepared;
    prepared.title = std::move(title);
    prepared.reply = std::move(reply);
    return prepared;
}

// The datasets of a run, made on the worker, bound by argument: a list
// argument takes all of its datasets.
struct Bound {
    std::string arg;
    bool list = false;
    std::vector<DeferredDataset> datasets;
};

Status materialise(const std::vector<Bound>& bound, gp::RunRequest& run)
{
    for (const Bound& arg : bound) {
        std::vector<gp::DatasetValue> values;
        for (const DeferredDataset& make : arg.datasets) {
            auto made = make();
            if (!made) {
                return made.error();
            }
            values.push_back(std::move(made).value());
        }
        if (arg.list) {
            run.values.emplace_back(arg.arg, gp::ArgValue(std::move(values)));
        } else {
            run.values.emplace_back(arg.arg, gp::ArgValue(std::move(values.front())));
        }
    }
    return {};
}

// A check of the materialised request, on the worker, before the run.
using PreRun = std::function<Status(const gp::RunRequest& run)>;

// The phases of a verb of one run: PREVIEW validates it here; otherwise the
// work makes its datasets, runs it and hands the outputs to applyOutputs,
// the head record first.
Result<Prepared> oneRun(std::string title, gp::RunRequest request, std::vector<Bound> bound,
                        ApplyRequest apply, std::string head, std::vector<std::string> records,
                        bool preview, PreRun check = {})
{
    if (preview) {
        gp::RunRequest trial = request;
        if (auto made = materialise(bound, trial); !made) {
            return made.error();
        }
        if (check) {
            if (auto checked = check(trial); !checked) {
                return checked.error();
            }
        }
        if (auto valid = gp::validate(trial); !valid) {
            return valid.error();
        }
        records.insert(records.begin(), head + " preview=yes");
        records.push_back("preview valid=yes changed=no");
        return answered(std::move(title), joinRecords(records));
    }
    Prepared prepared;
    prepared.title = std::move(title);
    prepared.work = [request, bound, apply, head, records, check](
                        const std::stop_token& stop, const Progress& progress) -> Result<Apply> {
        gp::RunRequest run = request;
        if (auto made = materialise(bound, run); !made) {
            return made.error();
        }
        if (check) {
            if (auto checked = check(run); !checked) {
                return checked.error();
            }
        }
        auto outputs = gp::run(run, stop, progress);
        if (!outputs) {
            return outputs.error();
        }
        auto kept = std::make_shared<gp::RunOutputs>(std::move(outputs).value());
        return Apply([kept, apply, head, records](Context& context) -> Result<std::string> {
            const double seconds = kept->seconds;
            auto applied = applyOutputs(context, apply, std::move(*kept));
            if (!applied) {
                return applied.error();
            }
            std::vector<std::string> reply{head + " seconds=" + fixed3(seconds)};
            reply.insert(reply.end(), records.begin(), records.end());
            reply.push_back(*applied);
            return joinRecords(reply);
        });
    };
    return prepared;
}

Status cancelled()
{
    return makeError(ErrorCode::InvalidState, "cancelled");
}

// ---- MOSAIC ---------------------------------------------------------------------------------

constexpr std::string_view kMosaicOptions[] = {"resolution"};

bool hasWildcard(const std::string& text)
{
    return text.find_first_of("*?") != std::string::npos;
}

bool passesThrough(const std::string& path)
{
    return path.starts_with("/vsi") || path.find("://") != std::string::npos;
}

// The files a FILE source of a mosaic names, on the worker: a folder is the
// rasters in it (what GDAL opens as one; a sidecar or a .prj is not), a
// pattern the files its name matches ('*', '?'), anything else itself. In
// name order, so a mosaic of a folder is the same whatever order the file
// system lists it in. NotFound for a folder or pattern that names none.
Result<std::vector<std::string>> tilesOf(const std::string& path)
{
    if (passesThrough(path)) {
        return std::vector<std::string>{path};
    }
    const std::filesystem::path given = pathOfUtf8(path);
    std::error_code error;
    std::vector<std::filesystem::path> found;
    if (std::filesystem::is_directory(given, error)) {
        for (const auto& entry : std::filesystem::directory_iterator(given, error)) {
            if (!entry.is_regular_file(error)) {
                continue;
            }
            auto opened = katana::gis::GdalDataset::open(entry.path());
            if (opened && (*opened)->hasRaster()) {
                found.push_back(entry.path());
            }
        }
    } else if (hasWildcard(given.filename().string())) {
        const std::filesystem::path folder =
            given.has_parent_path() ? given.parent_path() : std::filesystem::path(".");
        const std::string pattern = utf8OfPath(given.filename());
        for (const auto& entry : std::filesystem::directory_iterator(folder, error)) {
            if (entry.is_regular_file(error) &&
                katana::cad::matchesPattern(utf8OfPath(entry.path().filename()), pattern)) {
                found.push_back(entry.path());
            }
        }
    } else {
        return std::vector<std::string>{path};
    }
    if (found.empty()) {
        return makeError(ErrorCode::NotFound, "no raster there to mosaic", path);
    }
    std::ranges::sort(found);
    std::vector<std::string> tiles;
    for (const std::filesystem::path& file : found) {
        tiles.push_back(utf8OfPath(file));
    }
    return tiles;
}

// A name no scratch file of another run has: the derived folder is shared by
// every job of the front end, and by other processes on the same project.
std::string scratchName(std::string_view stem, std::string_view extension)
{
    static std::atomic<std::uint64_t> made{0};
    const auto now = std::chrono::steady_clock::now().time_since_epoch().count();
    return "katana-" + std::string(stem) + "-" + std::to_string(now) + "-" +
           std::to_string(++made) + std::string(extension);
}

Result<Prepared> prepareMosaic(Context& context, const Tokens& tokens, std::string_view text)
{
    const DemGrammar grammar{"RASTER MOSAIC", kMosaicOptions, 1, 4096, false, false, true};
    auto line = readLine(tokens, grammar);
    if (!line) {
        return line.error();
    }
    const std::optional<std::string> save = line->save;
    const Target to = line->target.value_or(Target{});
    if (save && line->target) {
        return makeError(ErrorCode::InvalidArgument,
                         "SAVE writes the mosaic and keeps it as a reference raster; TO says where "
                         "else it goes - give one");
    }
    if (to.kind != Target::Kind::Default && to.kind != Target::Kind::Reference &&
        to.kind != Target::Kind::File) {
        return makeError(ErrorCode::Unsupported,
                         "a mosaic is a reference raster (TO REFERENCE [<name>], or SAVE <file> to "
                         "keep it in a file of its own) or a file (TO FILE <path>)");
    }
    if (!line->name.empty() && to.kind == Target::Kind::Reference && !to.name.empty()) {
        return makeError(ErrorCode::InvalidArgument,
                         "NAME and TO REFERENCE both name the raster; give one");
    }
    const std::optional<std::string> written =
        save ? save : (to.kind == Target::Kind::File ? std::optional(to.name) : std::nullopt);
    if (written && fileInTheWay(*written) && !line->overwrite) {
        return makeError(ErrorCode::AlreadyExists, "the file exists; add OVERWRITE to replace it",
                         *written);
    }

    auto bound = bindSources(context, line->sources);
    if (!bound) {
        return bound.error();
    }
    std::vector<std::string> records = inputRecords(line->sources, {"input"});

    gp::RunRequest request;
    request.path = {"raster", "mosaic"};
    request.overwrite = line->overwrite;
    if (const auto resolution = line->option("resolution")) {
        request.tokens.push_back("--resolution=" + *resolution);
    }
    const bool virtualMosaic = !written;
    std::filesystem::path vrt;
    if (virtualMosaic) {
        // A VRT names its tiles; with absolute paths it reads them wherever
        // the VRT itself is kept.
        request.tokens.push_back("--absolute-path");
        vrt = derivedFolder(context) / scratchName("mosaic", ".vrt");
        request.outputTo = gp::OutputTo::File;
        request.outputPath = utf8OfPath(vrt);
        request.outputFormat = "VRT";
    } else {
        request.outputTo = gp::OutputTo::File;
        request.outputPath = *written;
        request.outputFormat = to.kind == Target::Kind::File ? to.format : std::string();
    }
    const std::string name =
        !line->name.empty() ? line->name : (!to.name.empty() && !save ? to.name : "mosaic");
    const std::string derivation(katana::core::trimmed(text));
    const std::optional<std::filesystem::path> project = context.document.projectDirectory();
    const std::vector<Source> sources = line->sources;
    const std::string head = "mosaic algorithm=\"raster mosaic\"";

    if (line->preview) {
        records.insert(records.begin(), head + " preview=yes");
        records.push_back("preview valid=yes changed=no");
        return answered("RASTER MOSAIC", joinRecords(records));
    }

    const std::vector<DeferredDataset> datasets = std::move(bound).value();
    const bool keptAsFile = save.has_value();
    Prepared prepared;
    prepared.title = "RASTER MOSAIC";
    prepared.work = [request, datasets, sources, records, head, vrt, virtualMosaic, keptAsFile,
                     name, derivation, project](const std::stop_token& stop,
                                                const Progress& progress) -> Result<Apply> {
        // The folder a VRT is written to is made when the first is.
        if (virtualMosaic) {
            std::error_code error;
            std::filesystem::create_directories(vrt.parent_path(), error);
        }
        // The tiles: rasters as their files, folders and patterns expanded.
        std::vector<gp::DatasetValue> tiles;
        for (std::size_t i = 0; i < sources.size(); ++i) {
            if (stop.stop_requested()) {
                return cancelled().error();
            }
            if (sources[i].kind == Source::Kind::File) {
                auto files = tilesOf(sources[i].path);
                if (!files) {
                    return files.error();
                }
                for (const std::string& file : *files) {
                    tiles.emplace_back(gp::DatasetPath{file, {}, {}});
                }
                continue;
            }
            auto made = datasets[i]();
            if (!made) {
                return made.error();
            }
            tiles.push_back(std::move(made).value());
        }
        gp::RunRequest run = request;
        const std::size_t count = tiles.size();
        run.values.emplace_back("input", gp::ArgValue(std::move(tiles)));
        auto outputs = gp::run(run, stop, progress);
        if (!outputs) {
            return outputs.error();
        }
        const std::string first = head + " tiles=" + std::to_string(count) +
                                  " virtual=" + (virtualMosaic ? "yes" : "no") +
                                  " seconds=" + fixed3(outputs->seconds);
        const std::string file = run.outputPath;
        return Apply([first, records, file, virtualMosaic, keptAsFile, name, derivation,
                      project, vrt](Context& ctx) -> Result<std::string> {
            std::vector<std::string> reply{first};
            reply.insert(reply.end(), records.begin(), records.end());
            if (!virtualMosaic && !keptAsFile) {
                reply.push_back("output arg=output kind=file target=file file=" + value(file));
                return joinRecords(reply);
            }
            std::filesystem::path kept = pathOfUtf8(file);
            std::string chosen = name;
            if (virtualMosaic) {
                // Into the derived folder under a name no raster has, as a
                // derived raster is kept.
                for (int copy = 2;; ++copy) {
                    const bool taken = std::ranges::any_of(
                        ctx.reference.rasters(), [&](const katana::interop::RasterOverlay& r) {
                            return katana::core::equalsIgnoringCase(r.name, chosen);
                        });
                    kept = igeo::derivedRasterPath(project, ctx.scratch, chosen)
                               .replace_extension(".vrt");
                    std::error_code error;
                    if (!taken && !std::filesystem::exists(kept, error)) {
                        break;
                    }
                    chosen = name + "-" + std::to_string(copy);
                }
                if (auto moved = moveFile(vrt, kept); !moved) {
                    return moved.error();
                }
            }
            auto record = keepInPlace(ctx, kept, chosen, derivation,
                                      keptAsFile || project.has_value());
            if (!record) {
                return record.error();
            }
            reply.push_back(*record);
            return joinRecords(reply);
        });
    };
    return prepared;
}

// ---- CLIP -----------------------------------------------------------------------------------

Result<Prepared> prepareClip(Context& context, const Tokens& tokens, std::string_view text)
{
    const DemGrammar grammar{"RASTER CLIP", {}, 1, 1, true, true};
    auto line = readLine(tokens, grammar);
    if (!line) {
        return line.error();
    }
    if (!line->scope) {
        return makeError(ErrorCode::InvalidArgument,
                         "RASTER CLIP needs what to clip to: AREA x0,y0,x1,y1 for a box, or a "
                         "scope whose closed boundaries clip it (SELECTION, LAYERS a,b, DRAWING "
                         "WHERE ...)");
    }
    gp::RunRequest request;
    request.path = {"raster", "clip"};
    auto apply = rasterTarget(context, *line, "RASTER CLIP", "clip", request, text);
    if (!apply) {
        return apply.error();
    }
    auto bound = bindSources(context, line->sources);
    if (!bound) {
        return bound.error();
    }
    std::vector<Bound> args{{"input", true, std::move(bound).value()}};
    std::vector<std::string> records = inputRecords(line->sources, {"input"});
    std::string head = "clip algorithm=\"raster clip\"";

    const katana::cad::ScopeWords& scope = *line->scope;
    if (scope.source == katana::cad::ScopeSource::Area) {
        // AREA is the box itself; a filter would pick boundaries, which a
        // box has none of.
        if (!scope.filter.empty()) {
            return makeError(ErrorCode::InvalidArgument,
                             "RASTER CLIP clips to AREA's box itself; a WHERE filter picks "
                             "boundaries - give DRAWING WHERE ... or LAYERS ... instead");
        }
        const std::string box = real(scope.area.min.x) + "," + real(scope.area.min.y) + "," +
                                real(scope.area.max.x) + "," + real(scope.area.max.y);
        request.tokens.push_back("--bbox=" + box);
        head += " by=area area=" + box;
    } else {
        igeo::DrawingDatasetOptions options;
        options.crsWkt = projectCrs(context);
        auto drawing = bindDrawing(context, scope, options);
        if (!drawing) {
            return drawing.error();
        }
        // The closed boundaries alone: points and open lines bound nothing.
        gp::FeatureSet boundaries;
        for (gp::FeatureTable& table : drawing->dataset.set.tables) {
            if (table.kind == katana::gis::GeometryKind::Polygon) {
                boundaries.tables.push_back(std::move(table));
            }
        }
        records.push_back(inputRecord("like", Source{}));
        records.push_back(scopeRecord("like", *drawing));
        const std::size_t count = drawing->dataset.stats.polygons;
        head += " by=boundaries boundaries=" + std::to_string(count);
        if (boundaries.tables.empty()) {
            records.insert(records.begin(), head + " ran=no");
            return answered("RASTER CLIP", joinRecords(records));
        }
        auto set = std::make_shared<const gp::FeatureSet>(std::move(boundaries));
        args.push_back(
            {"like", false, {[set]() -> Result<gp::DatasetValue> { return gp::DatasetValue(*set); }}});
    }
    return oneRun("RASTER CLIP", std::move(request), std::move(args), std::move(apply).value(), head,
                  std::move(records), line->preview);
}

// ---- FILL -----------------------------------------------------------------------------------

constexpr std::string_view kFillOptions[] = {"distance", "smoothing", "strategy"};

std::size_t emptyCells(const BandValues& band)
{
    return static_cast<std::size_t>(std::ranges::count_if(
        band.values, [&band](double cell) { return !holdsValue(band, cell); }));
}

Result<Prepared> prepareFill(Context& context, const Tokens& tokens, std::string_view text)
{
    const DemGrammar grammar{"RASTER FILL", kFillOptions, 1, 1, false, true};
    auto line = readLine(tokens, grammar);
    if (!line) {
        return line.error();
    }
    gp::RunRequest request;
    request.path = {"raster", "fill-nodata"};
    if (const auto distance = line->option("distance")) {
        auto cells = wholeOption("distance", *distance, 1);
        if (!cells) {
            return cells.error();
        }
        request.tokens.push_back("--max-distance=" + std::to_string(*cells));
    }
    if (const auto smoothing = line->option("smoothing")) {
        auto passes = wholeOption("smoothing", *smoothing, 0);
        if (!passes) {
            return passes.error();
        }
        request.tokens.push_back("--smoothing-iterations=" + std::to_string(*passes));
    }
    if (const auto strategy = line->option("strategy")) {
        // GDAL's own choices; it refuses any other in its own words.
        request.tokens.push_back("--strategy=" + *strategy);
    }
    auto apply = rasterTarget(context, *line, "RASTER FILL", "filled", request, text);
    if (!apply) {
        return apply.error();
    }
    auto bound = bindSources(context, line->sources);
    if (!bound) {
        return bound.error();
    }
    const std::vector<Bound> args{{"input", true, std::move(bound).value()}};
    const std::vector<std::string> records = inputRecords(line->sources, {"input"});
    const std::string head = "fill algorithm=\"raster fill-nodata\"";
    if (line->preview) {
        return oneRun("RASTER FILL", std::move(request), args, std::move(apply).value(), head,
                      records, true);
    }
    const ApplyRequest applied = std::move(apply).value();
    Prepared prepared;
    prepared.title = "RASTER FILL";
    prepared.work = [request, args, applied, head, records](
                        const std::stop_token& stop, const Progress& progress) -> Result<Apply> {
        gp::RunRequest run = request;
        if (auto made = materialise(args, run); !made) {
            return made.error();
        }
        // What there is to fill, counted from the input as GDAL reads it.
        const auto& inputs = std::get<std::vector<gp::DatasetValue>>(run.values.front().second);
        auto before = readBandOne(inputs.front());
        if (!before) {
            return before.error();
        }
        const std::size_t empty = emptyCells(*before);
        if (empty == 0) {
            const std::string first = head + " empty=0 filled=0 remaining=0 ran=no";
            return Apply([first, records](Context&) -> Result<std::string> {
                std::vector<std::string> reply{first};
                reply.insert(reply.end(), records.begin(), records.end());
                return joinRecords(reply);
            });
        }
        auto outputs = gp::run(run, stop, progress);
        if (!outputs) {
            return outputs.error();
        }
        std::size_t remaining = 0;
        if (outputs->file) {
            auto after = readBandOne(pathOfUtf8(*outputs->file));
            if (!after) {
                return after.error();
            }
            remaining = emptyCells(*after);
        }
        const std::string first = head + " empty=" + std::to_string(empty) +
                                  " filled=" + std::to_string(empty - std::min(empty, remaining)) +
                                  " remaining=" + std::to_string(remaining) +
                                  " seconds=" + fixed3(outputs->seconds);
        auto kept = std::make_shared<gp::RunOutputs>(std::move(outputs).value());
        return Apply([kept, applied, first, records](Context& ctx) -> Result<std::string> {
            auto output = applyOutputs(ctx, applied, std::move(*kept));
            if (!output) {
                return output.error();
            }
            std::vector<std::string> reply{first};
            reply.insert(reply.end(), records.begin(), records.end());
            reply.push_back(*output);
            return joinRecords(reply);
        });
    };
    return prepared;
}

// ---- FOOTPRINT ------------------------------------------------------------------------------

Result<Prepared> prepareFootprint(Context& context, const Tokens& tokens, std::string_view text)
{
    const DemGrammar grammar{"RASTER FOOTPRINT", {}, 1, 1, false, true};
    auto line = readLine(tokens, grammar);
    if (!line) {
        return line.error();
    }
    const Target to = line->target.value_or(Target{});
    if (to.kind != Target::Kind::Default && to.kind != Target::Kind::Layer &&
        to.kind != Target::Kind::File) {
        return makeError(ErrorCode::Unsupported,
                         "a footprint is areas: TO LAYER <path> draws them, TO FILE <path> writes "
                         "them");
    }
    if (!line->name.empty()) {
        return makeError(ErrorCode::InvalidArgument,
                         "a footprint is drawn on a layer, not named: TO LAYER <path>");
    }
    gp::RunRequest request;
    request.path = {"raster", "footprint"};
    request.overwrite = line->overwrite;
    if (to.kind == Target::Kind::File) {
        if (fileInTheWay(to.name) && !line->overwrite) {
            return makeError(ErrorCode::AlreadyExists, "the file exists; add OVERWRITE to replace it",
                             to.name);
        }
        request.outputTo = gp::OutputTo::File;
        request.outputPath = to.name;
        request.outputFormat = to.format;
    }
    auto bound = bindSources(context, line->sources);
    if (!bound) {
        return bound.error();
    }
    ApplyRequest apply;
    apply.target = to;
    apply.defaultName = "footprint";
    apply.result.operation = gp::pathText(request.path);
    apply.result.commandName = std::string(katana::core::trimmed(text));
    return oneRun("RASTER FOOTPRINT", std::move(request), {{"input", true, std::move(bound).value()}},
                  std::move(apply), "footprint algorithm=\"raster footprint\"",
                  inputRecords(line->sources, {"input"}), line->preview);
}

// ---- REPROJECT ------------------------------------------------------------------------------

constexpr std::string_view kReprojectOptions[] = {"crs", "from", "like", "resampling", "cell"};

Result<Prepared> prepareReproject(Context& context, const Tokens& tokens, std::string_view text)
{
    const DemGrammar grammar{"RASTER REPROJECT", kReprojectOptions, 1, 1, false, true};
    auto line = readLine(tokens, grammar);
    if (!line) {
        return line.error();
    }
    const std::optional<std::string> crs = line->option("crs");
    const std::optional<std::string> like = line->option("like");
    const std::optional<std::string> from = line->option("from");
    if (crs && like) {
        return makeError(ErrorCode::InvalidArgument,
                         "crs= and like= both say where the raster goes; give one");
    }
    const std::string project = projectCrs(context);
    if (!crs && !like && project.empty()) {
        return makeError(ErrorCode::InvalidCRS,
                         "the drawing has no coordinate system to reproject to; give crs=<code> "
                         "or like=<raster>");
    }
    gp::RunRequest request;
    request.path = {"raster", "reproject"};
    std::string head = "reproject algorithm=\"raster reproject\"";
    if (!like) {
        request.tokens.push_back("--output-crs=" + (crs ? *crs : project));
        head += " crs=" + (crs ? value(*crs) : std::string("project"));
    }
    if (from) {
        request.tokens.push_back("--input-crs=" + *from);
        head += " from=" + value(*from);
    }
    if (const auto resampling = line->option("resampling")) {
        request.tokens.push_back("--resampling=" + *resampling);
        head += " resampling=" + value(*resampling);
    }
    if (const auto cell = line->option("cell")) {
        auto length = positiveOption("cell", *cell);
        if (!length) {
            return length.error();
        }
        request.tokens.push_back("--resolution=" + real(*length) + "," + real(*length));
        head += " cell=" + real(*length);
    }
    auto apply = rasterTarget(context, *line, "RASTER REPROJECT", "reprojected", request, text);
    if (!apply) {
        return apply.error();
    }
    auto bound = bindSources(context, line->sources);
    if (!bound) {
        return bound.error();
    }
    std::vector<Bound> args{{"input", true, std::move(bound).value()}};
    std::vector<std::string> records = inputRecords(line->sources, {"input"});
    if (like) {
        Source likeSource;
        likeSource.kind = Source::Kind::Raster;
        likeSource.raster = *like;
        auto likeDataset = bindRaster(context, likeSource);
        if (!likeDataset) {
            return likeDataset.error();
        }
        args.push_back({"like", false, {std::move(likeDataset).value()}});
        records.push_back(inputRecord("like", likeSource));
        head += " like=" + value(*like);
    }
    // A raster that says nothing of where it is cannot be reprojected: GDAL
    // would hand it back unchanged and say nothing.
    PreRun check;
    if (!from) {
        check = [](const gp::RunRequest& run) -> Status {
            const auto& inputs = std::get<std::vector<gp::DatasetValue>>(
                std::ranges::find_if(run.values, [](const auto& v) { return v.first == "input"; })
                    ->second);
            auto info = rasterInfoOf(inputs.front());
            if (!info) {
                return info.error();
            }
            if (info->projectionWkt.empty()) {
                return makeError(ErrorCode::InvalidCRS,
                                 "the raster has no coordinate system to reproject from; "
                                 "from=<crs> says what it is");
            }
            return {};
        };
    }
    return oneRun("RASTER REPROJECT", std::move(request), std::move(args), std::move(apply).value(),
                  head, std::move(records), line->preview, check);
}

// ---- DIFFERENCE -----------------------------------------------------------------------------

constexpr std::string_view kDifferenceOptions[] = {"resampling"};

bool sameGrid(const katana::gis::RasterInfo& a, const katana::gis::RasterInfo& b)
{
    return a.width == b.width && a.height == b.height && a.geotransform == b.geotransform &&
           a.projectionWkt == b.projectionWkt;
}

// One raster run on the worker whose raster result is wanted as a dataset:
// kept in memory, or in a file in `folder` when it is large.
Result<gp::DatasetValue> intermediate(gp::RunRequest run, const std::filesystem::path& folder,
                                      const std::stop_token& stop, std::vector<std::string>& files)
{
    run.outputTo = gp::OutputTo::Memory;
    run.spillDirectory = utf8OfPath(folder);
    auto outputs = gp::run(run, stop, {});
    if (!outputs) {
        return outputs.error();
    }
    if (outputs->raster) {
        return gp::DatasetValue(std::move(*outputs->raster));
    }
    if (outputs->file) {
        files.push_back(*outputs->file);
        return gp::DatasetValue(gp::DatasetPath{*outputs->file, {}, {}});
    }
    return makeError(ErrorCode::CommandRejected, gp::pathText(run.path) + " made no raster");
}

struct Volumes {
    std::size_t cells = 0;
    double cut = 0.0, fill = 0.0, area = 0.0;
};

// Cut and fill of a difference (first minus second): a positive cell is the
// first above the second - fill - and a negative one cut, each its depth
// times the cell's area. Summed compensated, so a million cells of 0.3 add
// up to what they are.
Volumes volumesOf(const BandValues& band)
{
    katana::math::CompensatedSum cut;
    katana::math::CompensatedSum fill;
    Volumes volumes;
    for (const double cell : band.values) {
        if (!holdsValue(band, cell)) {
            continue;
        }
        ++volumes.cells;
        if (cell > 0.0) {
            fill.add(cell);
        } else if (cell < 0.0) {
            cut.add(-cell);
        }
    }
    const double area = cellArea(band.geotransform);
    volumes.cut = cut.value() * area;
    volumes.fill = fill.value() * area;
    volumes.area = static_cast<double>(volumes.cells) * area;
    return volumes;
}

std::string cellText(const std::array<double, 6>& geotransform)
{
    const double x = std::abs(geotransform[1]);
    const double y = std::abs(geotransform[5]);
    return x == y ? real(x) : real(x) + "," + real(y);
}

Result<Prepared> prepareDifference(Context& context, const Tokens& tokens, std::string_view text)
{
    const DemGrammar grammar{"RASTER DIFFERENCE", kDifferenceOptions, 2, 2, true, true};
    auto line = readLine(tokens, grammar);
    if (!line) {
        return line.error();
    }
    gp::RunRequest request;
    request.path = {"raster", "calc"};
    request.tokens = {"--dialect=builtin", "--calc=diff"};
    auto apply = rasterTarget(context, *line, "RASTER DIFFERENCE", "difference", request, text);
    if (!apply) {
        return apply.error();
    }
    auto bound = bindSources(context, line->sources);
    if (!bound) {
        return bound.error();
    }
    std::vector<std::string> records = inputRecords(line->sources, {"first", "second"});
    const std::string resampling = line->option("resampling").value_or("bilinear");

    // An optional scope limits the area: its closed boundaries clip the
    // difference before it is summed.
    std::shared_ptr<const gp::FeatureSet> boundaries;
    if (line->scope) {
        igeo::DrawingDatasetOptions options;
        options.crsWkt = projectCrs(context);
        auto drawing = bindDrawing(context, *line->scope, options);
        if (!drawing) {
            return drawing.error();
        }
        gp::FeatureSet polygons;
        for (gp::FeatureTable& table : drawing->dataset.set.tables) {
            if (table.kind == katana::gis::GeometryKind::Polygon) {
                polygons.tables.push_back(std::move(table));
            }
        }
        records.push_back(scopeRecord("within", *drawing));
        if (polygons.tables.empty()) {
            records.insert(records.begin(),
                           "difference algorithm=\"raster calc\" within=boundaries boundaries=0 "
                           "ran=no");
            return answered("RASTER DIFFERENCE", joinRecords(records));
        }
        boundaries = std::make_shared<const gp::FeatureSet>(std::move(polygons));
    }
    const std::vector<DeferredDataset> datasets = std::move(bound).value();
    const std::string head = "difference algorithm=\"raster calc\"";

    // Both rasters must say where they are, or both say nothing.
    const auto checkCrs = [](const katana::gis::RasterInfo& a,
                             const katana::gis::RasterInfo& b) -> Status {
        if (a.projectionWkt.empty() != b.projectionWkt.empty()) {
            return makeError(ErrorCode::InvalidCRS,
                             "one raster has a coordinate system and the other none, so they "
                             "cannot be aligned; reproject the other first (RASTER REPROJECT "
                             "... from=<crs>)");
        }
        return {};
    };
    if (line->preview) {
        auto first = datasets[0]();
        if (!first) {
            return first.error();
        }
        auto second = datasets[1]();
        if (!second) {
            return second.error();
        }
        auto a = rasterInfoOf(*first);
        if (!a) {
            return a.error();
        }
        auto b = rasterInfoOf(*second);
        if (!b) {
            return b.error();
        }
        if (auto crs = checkCrs(*a, *b); !crs) {
            return crs.error();
        }
        records.insert(records.begin(), head + " aligned=" + (sameGrid(*a, *b) ? "yes" : "no") +
                                            " preview=yes");
        records.push_back("preview valid=yes changed=no");
        return answered("RASTER DIFFERENCE", joinRecords(records));
    }

    const ApplyRequest applied = std::move(apply).value();
    const std::filesystem::path scratch = context.scratch;
    Prepared prepared;
    prepared.title = "RASTER DIFFERENCE";
    prepared.work = [request, datasets, boundaries, applied, head, records, resampling, scratch,
                     checkCrs](const std::stop_token& stop,
                               const Progress& progress) -> Result<Apply> {
        std::vector<std::string> intermediates;
        const auto tidy = [&intermediates] {
            std::error_code error;
            for (const std::string& file : intermediates) {
                std::filesystem::remove(pathOfUtf8(file), error);
            }
        };
        auto first = datasets[0]();
        if (!first) {
            return first.error();
        }
        auto second = datasets[1]();
        if (!second) {
            return second.error();
        }
        auto a = rasterInfoOf(*first);
        if (!a) {
            return a.error();
        }
        auto b = rasterInfoOf(*second);
        if (!b) {
            return b.error();
        }
        if (auto crs = checkCrs(*a, *b); !crs) {
            return crs.error();
        }
        const bool aligned = sameGrid(*a, *b);
        gp::DatasetValue under = std::move(second).value();
        if (!aligned) {
            // The second onto the first's grid: its extent, its cells and its
            // CRS, said outright. GDAL's --like says the same in one word but
            // is ignored, without a word, when the rasters have no CRS
            // (measured: a 2 m grid "aligned" to a 1 m one stayed 2 m).
            const std::array<double, 6>& gt = a->geotransform;
            const double x0 = gt[0];
            const double x1 = gt[0] + a->width * gt[1];
            const double y0 = gt[3] + a->height * gt[5];
            const double y1 = gt[3];
            gp::RunRequest align;
            align.path = {"raster", "reproject"};
            align.tokens = {"--resampling=" + resampling,
                            "--bbox=" + real(std::min(x0, x1)) + "," + real(std::min(y0, y1)) +
                                "," + real(std::max(x0, x1)) + "," + real(std::max(y0, y1)),
                            "--size=" + std::to_string(a->width) + "," +
                                std::to_string(a->height)};
            if (!a->projectionWkt.empty()) {
                align.tokens.push_back("--output-crs=" + a->projectionWkt);
            }
            align.values.emplace_back("input",
                                      gp::ArgValue(std::vector<gp::DatasetValue>{std::move(under)}));
            auto moved = intermediate(std::move(align), scratch, stop, intermediates);
            if (!moved) {
                tidy();
                return moved.error();
            }
            under = std::move(moved).value();
        }
        if (stop.stop_requested()) {
            tidy();
            return cancelled().error();
        }
        gp::RunRequest run = request;
        const std::string keptSpill = run.spillDirectory;
        const bool clipAfter = boundaries != nullptr;
        if (clipAfter) {
            // The whole difference is an intermediate: the clipped one is kept.
            run.outputTo = gp::OutputTo::Memory;
            run.maxMemoryCells = 0;
            run.spillDirectory = utf8OfPath(scratch);
        }
        run.values.emplace_back("input", gp::ArgValue(std::vector<gp::DatasetValue>{
                                             std::move(first).value(), std::move(under)}));
        auto outputs = gp::run(run, stop, progress);
        if (!outputs) {
            tidy();
            return outputs.error();
        }
        if (clipAfter) {
            if (!outputs->file) {
                tidy();
                return makeError(ErrorCode::CommandRejected, "raster calc made no raster");
            }
            intermediates.push_back(*outputs->file);
            gp::RunRequest clip;
            clip.path = {"raster", "clip"};
            clip.outputTo = request.outputTo;
            clip.outputPath = request.outputPath;
            clip.outputFormat = request.outputFormat;
            clip.overwrite = request.overwrite;
            clip.maxMemoryCells = 0;
            clip.spillDirectory = keptSpill;
            clip.values.emplace_back(
                "input", gp::ArgValue(std::vector<gp::DatasetValue>{
                             gp::DatasetValue(gp::DatasetPath{*outputs->file, {}, {}})}));
            clip.values.emplace_back("like", gp::ArgValue(gp::DatasetValue(*boundaries)));
            const double seconds = outputs->seconds;
            outputs = gp::run(clip, stop, {});
            if (!outputs) {
                tidy();
                return outputs.error();
            }
            outputs->seconds += seconds;
        }
        tidy();
        if (!outputs->file) {
            return makeError(ErrorCode::CommandRejected, "the difference made no raster");
        }
        auto band = readBandOne(pathOfUtf8(*outputs->file));
        if (!band) {
            return band.error();
        }
        const Volumes volumes = volumesOf(*band);
        const std::string cell = cellText(band->geotransform);
        const std::string first3 =
            head + " aligned=" + (aligned ? "yes" : "no") +
            (aligned ? std::string() : " resampling=" + value(resampling)) + " cell=" + cell +
            " cells=" + std::to_string(volumes.cells) + " area=" + fixed3(volumes.area) +
            " cut=" + fixed3(volumes.cut) + " fill=" + fixed3(volumes.fill) +
            " net=" + fixed3(volumes.fill - volumes.cut) + " method=grid label=" +
            value("grid method, cell " + cell + " m") + " seconds=" + fixed3(outputs->seconds);
        auto kept = std::make_shared<gp::RunOutputs>(std::move(outputs).value());
        return Apply([kept, applied, first3, records](Context& ctx) -> Result<std::string> {
            auto output = applyOutputs(ctx, applied, std::move(*kept));
            if (!output) {
                return output.error();
            }
            std::vector<std::string> reply{first3};
            reply.insert(reply.end(), records.begin(), records.end());
            reply.push_back(*output);
            return joinRecords(reply);
        });
    };
    return prepared;
}

} // namespace

Result<Prepared> prepareRasterMosaic(Context& context, const Tokens& tokens, std::string_view line)
{
    return prepareMosaic(context, tokens, line);
}

Result<Prepared> prepareRasterClip(Context& context, const Tokens& tokens, std::string_view line)
{
    return prepareClip(context, tokens, line);
}

Result<Prepared> prepareRasterFill(Context& context, const Tokens& tokens, std::string_view line)
{
    return prepareFill(context, tokens, line);
}

Result<Prepared> prepareRasterFootprint(Context& context, const Tokens& tokens,
                                        std::string_view line)
{
    return prepareFootprint(context, tokens, line);
}

Result<Prepared> prepareRasterReproject(Context& context, const Tokens& tokens,
                                        std::string_view line)
{
    return prepareReproject(context, tokens, line);
}

Result<Prepared> prepareRasterDifference(Context& context, const Tokens& tokens,
                                         std::string_view line)
{
    return prepareDifference(context, tokens, line);
}

} // namespace katana::app::geo
