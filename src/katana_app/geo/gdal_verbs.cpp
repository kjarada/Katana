// The GDAL verb (docs/geoprocessing.md, "The GDAL verb"): any algorithm of
// GDAL's, in GDAL's own argument vocabulary, with Katana's FROM and TO
// clauses binding drawing data, rasters, surfaces and files.
//
//   GDAL VERSION
//   GDAL LIST [raster|vector|mdim|dataset|vsi|pipeline|driver|convert|info|<text>] [JSON]
//   GDAL HELP <algorithm> [JSON]
//   GDAL [RUN] <algorithm> [<gdal word>...] [FROM [<arg>] <source>]... [TO [<arg>] <target>]
//        [CONFIRM] [OVERWRITE] [PREVIEW]
//
// The line is Katana's, not GDAL's serialised command: GDAL cannot write back
// a dataset it was handed as an object ("<input:unserialisable>"), and the
// line a person typed is what the history, the undo list and a script keep.

#include <algorithm>
#include <cctype>
#include <filesystem>
#include <iterator>
#include <map>
#include <memory>
#include <set>
#include <string>
#include <system_error>
#include <utility>
#include <vector>

#include <nlohmann/json.hpp>

#include "bindings.hpp"
#include "geo_verbs.hpp"
#include "katana/core/text.hpp"
#include "katana/interop/terrain_io.hpp"
#include "replies.hpp"
#include "schema.hpp"
#include "verb_table.hpp"

namespace katana::app::geo {

namespace gp = katana::gis::processing;
namespace igeo = katana::interop::geo;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

namespace {

constexpr const char* kUsage =
    "GDAL VERSION | GDAL LIST [<group>|<text>] [JSON] | GDAL HELP <algorithm> [JSON] | "
    "GDAL [RUN] <algorithm> [<gdal word>...] [FROM [<arg>] <source>]... [TO [<arg>] <target>] "
    "[CONFIRM] [OVERWRITE] [PREVIEW]";

bool clause(const Tokens& tokens, std::size_t i)
{
    for (const char* word : {"FROM", "TO", "CONFIRM", "OVERWRITE", "PREVIEW"}) {
        if (tokens.is(i, word)) {
            return true;
        }
    }
    return false;
}

const gp::AlgorithmInfo* infoOf(const std::vector<std::string>& path)
{
    for (const gp::AlgorithmInfo& info : gp::catalogue()) {
        if (info.path == path) {
            return &info;
        }
    }
    return nullptr;
}

bool under(const std::vector<std::string>& path, const std::vector<std::string>& group)
{
    return path.size() > group.size() && std::equal(group.begin(), group.end(), path.begin());
}

std::size_t leavesUnder(const std::vector<std::string>& group)
{
    std::size_t count = 0;
    for (const gp::AlgorithmInfo& info : gp::catalogue()) {
        count += !info.container && under(info.path, group) ? 1u : 0u;
    }
    return count;
}

std::string joinedRecords(const std::vector<std::string>& records)
{
    std::string text;
    for (const std::string& record : records) {
        text += (text.empty() ? "" : "\n") + record;
    }
    return text;
}

Prepared answered(std::string title, std::string reply)
{
    Prepared prepared;
    prepared.title = std::move(title);
    prepared.reply = std::move(reply);
    return prepared;
}

// ---- LIST ------------------------------------------------------------------------------------

Result<Prepared> list(const Tokens& tokens, std::size_t at)
{
    bool json = false;
    std::vector<std::string> words;
    for (; at < tokens.size(); ++at) {
        if (tokens.is(at, "JSON")) {
            json = true;
        } else {
            words.push_back(katana::core::lowered(tokens[at]));
        }
    }
    // A group named ("raster", "vector grid") lists what is in it; any other
    // text is looked for in the names and the descriptions.
    std::vector<std::string> group;
    bool isGroup = false;
    if (!words.empty()) {
        std::size_t consumed = 0;
        const auto resolved = gp::resolve(words, consumed);
        const gp::AlgorithmInfo* named = nullptr;
        if (!resolved && resolved.error().code == ErrorCode::InvalidArgument) {
            group = std::vector<std::string>(words.begin(),
                                             words.begin() + static_cast<std::ptrdiff_t>(consumed));
            named = infoOf(group);
        }
        isGroup = named != nullptr && named->container && consumed == words.size();
    }
    std::string text;
    for (const std::string& word : words) {
        text += (text.empty() ? "" : " ") + word;
    }
    std::vector<const gp::AlgorithmInfo*> chosen;
    for (const gp::AlgorithmInfo& info : gp::catalogue()) {
        const bool take =
            words.empty() || (isGroup ? under(info.path, group)
                                      : katana::core::lowered(gp::pathText(info.path)).find(text) !=
                                                std::string::npos ||
                                            katana::core::lowered(info.description).find(text) !=
                                                std::string::npos);
        if (take) {
            chosen.push_back(&info);
        }
    }
    if (json) {
        nlohmann::json array = nlohmann::json::array();
        for (const gp::AlgorithmInfo* info : chosen) {
            array.push_back(algorithmJson(*info));
        }
        return answered("GDAL LIST", array.dump());
    }
    std::vector<std::string> records;
    for (const gp::AlgorithmInfo* info : chosen) {
        records.push_back(info->container ? groupRecord(*info, leavesUnder(info->path))
                                          : algorithmRecord(*info));
    }
    std::size_t leaves = 0;
    for (const gp::AlgorithmInfo* info : chosen) {
        leaves += info->container ? 0u : 1u;
    }
    records.push_back("listed algorithms=" + std::to_string(leaves) +
                      " groups=" + std::to_string(chosen.size() - leaves) +
                      " filter=" + value(text));
    return answered("GDAL LIST", joinedRecords(records));
}

// ---- HELP -------------------------------------------------------------------------------------

Result<Prepared> help(const Tokens& tokens, std::size_t at)
{
    bool json = false;
    std::vector<std::string> words;
    for (; at < tokens.size(); ++at) {
        if (tokens.is(at, "JSON")) {
            json = true;
        } else {
            words.push_back(tokens[at]);
        }
    }
    if (words.empty()) {
        return makeError(ErrorCode::InvalidArgument, "GDAL HELP names an algorithm: " +
                                                         std::string(kUsage));
    }
    std::size_t consumed = 0;
    auto path = gp::resolve(words, consumed);
    if (!path) {
        return path.error();
    }
    if (consumed != words.size()) {
        return makeError(ErrorCode::InvalidArgument, "GDAL HELP takes one algorithm",
                         words[consumed]);
    }
    auto spec = gp::describe(*path);
    if (!spec) {
        return spec.error();
    }
    if (json) {
        return answered("GDAL HELP", describeJson(*spec).dump());
    }
    std::vector<std::string> records{algorithmRecord(spec->info)};
    for (const gp::ArgSpec& arg : spec->args) {
        records.push_back(argRecord(arg));
    }
    for (const gp::ArgSpec& arg : spec->args) {
        if (arg.isDataset() && arg.isInput && !arg.isOutput) {
            records.push_back(bindingRecord(arg));
        }
    }
    return answered("GDAL HELP", joinedRecords(records));
}

// ---- RUN ---------------------------------------------------------------------------------------

const gp::ArgSpec* argNamed(const gp::AlgorithmSpec& spec, const std::string& name)
{
    for (const gp::ArgSpec& arg : spec.args) {
        if (arg.name == name || (!arg.shortName.empty() && arg.shortName == name) ||
            std::ranges::find(arg.aliases, name) != arg.aliases.end()) {
            return &arg;
        }
    }
    return nullptr;
}

// name=value, when name is an argument's, as GDAL's --name=value: the form
// the other verbs' options take, so GDAL raster hillshade zfactor=2 reads as
// a person used to Katana would write it.
std::string rewritten(const gp::AlgorithmSpec& spec, const std::string& word, bool quoted)
{
    if (quoted || word.empty() || word.front() == '-') {
        return word;
    }
    const std::size_t equals = word.find('=');
    if (equals == std::string::npos || equals == 0) {
        return word;
    }
    const gp::ArgSpec* arg = argNamed(spec, word.substr(0, equals));
    return arg != nullptr ? "--" + arg->name + word.substr(equals) : word;
}

// The long names the tail's options name outright (--input, -i).
std::set<std::string> namedInTail(const gp::AlgorithmSpec& spec, const std::vector<std::string>& tail)
{
    std::set<std::string> names;
    for (const std::string& word : tail) {
        std::string name;
        if (word.size() > 2 && word.starts_with("--")) {
            name = word.substr(2, word.find('=') == std::string::npos ? std::string::npos
                                                                     : word.find('=') - 2);
        } else if (word.size() == 2 && word[0] == '-' &&
                   std::isalpha(static_cast<unsigned char>(word[1])) != 0) {
            name = word.substr(1);
        }
        if (const gp::ArgSpec* arg = name.empty() ? nullptr : argNamed(spec, name)) {
            names.insert(arg->name);
        }
    }
    return names;
}

struct FromClause {
    std::string arg;
    Source source;
};

// The inputs a run reads, bound at prepare and materialised by the work.
struct BoundArg {
    std::string arg;
    bool list = false;
    std::vector<DeferredDataset> datasets;
};

Result<Prepared> runLine(Context& context, const Tokens& tokens, std::size_t at,
                         std::string_view line)
{
    std::vector<std::string> words;
    for (std::size_t i = at; i < tokens.size() && !clause(tokens, i); ++i) {
        words.push_back(tokens[i]);
    }
    if (words.empty()) {
        return makeError(ErrorCode::InvalidArgument, std::string("usage: ") + kUsage);
    }
    std::size_t consumed = 0;
    auto path = gp::resolve(words, consumed);
    if (!path) {
        return path.error();
    }
    at += consumed;
    const gp::AlgorithmInfo* info = infoOf(*path);
    auto spec = gp::describe(*path);
    if (info == nullptr || !spec) {
        return spec ? makeError(ErrorCode::NotFound, "no GDAL algorithm has that path",
                                gp::pathText(*path))
                    : spec.error();
    }

    // GDAL's own words, up to the first clause.
    std::vector<std::string> tail;
    for (; at < tokens.size() && !clause(tokens, at); ++at) {
        tail.push_back(rewritten(*spec, tokens[at], tokens.quoted[at]));
    }
    if (auto refused = gp::checkTokens(*path, tail); !refused) {
        return refused.error();
    }

    std::vector<FromClause> froms;
    std::optional<Target> target;
    bool confirm = false, overwrite = false, preview = false;
    while (at < tokens.size()) {
        if (tokens.is(at, "FROM")) {
            ++at;
            FromClause from;
            if (at < tokens.size() && !tokens.quoted[at] && !isSourceKeyword(tokens, at) &&
                !clause(tokens, at)) {
                const gp::ArgSpec* arg = argNamed(*spec, tokens[at]);
                if (arg == nullptr || !arg->isDataset() || arg->isOutput) {
                    return makeError(ErrorCode::InvalidArgument,
                                     "FROM names an input dataset of " + gp::pathText(*path) +
                                         " (GDAL HELP lists them) or a source",
                                     tokens[at]);
                }
                from.arg = arg->name;
                ++at;
            }
            auto source = parseSource(tokens, at);
            if (!source) {
                return source.error();
            }
            from.source = std::move(source).value();
            froms.push_back(std::move(from));
        } else if (tokens.is(at, "TO")) {
            if (target) {
                return makeError(ErrorCode::InvalidArgument, "one TO per line");
            }
            ++at;
            if (at < tokens.size() && !tokens.quoted[at]) {
                if (const gp::ArgSpec* arg = argNamed(*spec, tokens[at]);
                    arg != nullptr && arg->isOutput && arg->isDataset()) {
                    ++at; // TO output LAYER ...: the only output there is
                }
            }
            auto parsed = parseTarget(tokens, at);
            if (!parsed) {
                return parsed.error();
            }
            target = std::move(parsed).value();
        } else if (tokens.is(at, "CONFIRM")) {
            confirm = true;
            ++at;
        } else if (tokens.is(at, "OVERWRITE")) {
            overwrite = true;
            ++at;
        } else if (tokens.is(at, "PREVIEW")) {
            preview = true;
            ++at;
        } else {
            return makeError(ErrorCode::InvalidArgument,
                             "GDAL's own words come before FROM and TO; after them only FROM, TO, "
                             "CONFIRM, OVERWRITE and PREVIEW",
                             tokens[at]);
        }
    }

    if (info->policy == gp::Policy::Confirm && !confirm) {
        return makeError(ErrorCode::Unsupported,
                         gp::pathText(*path) +
                             " changes or removes data that already exists; add CONFIRM to run it",
                         gp::pathText(*path));
    }
    // What GDAL's own words change, a pipeline's steps and options included
    // however the pipeline was quoted: the algorithm's policy alone would let
    // "pipeline read a ! update b" write into b.
    const gp::TailEffects effects = gp::tailEffects(*path, tail);
    if (!effects.confirmStep.empty() && !confirm) {
        return makeError(ErrorCode::Unsupported,
                         "the pipeline step " + effects.confirmStep +
                             " changes data that already exists; add CONFIRM to run it",
                         effects.confirmStep);
    }
    if (!effects.overwriteWord.empty() && !overwrite) {
        return makeError(ErrorCode::InvalidArgument,
                         effects.overwriteWord +
                             " changes a dataset that already exists; add OVERWRITE to the line "
                             "to allow it",
                         effects.overwriteWord);
    }

    // Each FROM to its dataset argument: named, or the first required input
    // not yet bound; a list takes every FROM that names it.
    std::vector<const gp::ArgSpec*> inputs;
    for (const gp::ArgSpec& arg : spec->args) {
        if (arg.isDataset() && arg.isInput && !arg.isOutput) {
            inputs.push_back(&arg);
        }
    }
    const std::set<std::string> named = namedInTail(*spec, tail);
    std::map<std::string, std::size_t> fromCount;
    for (FromClause& from : froms) {
        if (from.arg.empty()) {
            // The first required input no FROM has taken; one GDAL's words
            // name as well is refused below as given twice.
            for (const gp::ArgSpec* arg : inputs) {
                if (arg->required && !fromCount.contains(arg->name)) {
                    from.arg = arg->name;
                    break;
                }
            }
            if (from.arg.empty()) {
                return makeError(ErrorCode::InvalidArgument,
                                 "FROM has no required dataset left to bind; name one: FROM "
                                 "<arg> <source> (GDAL HELP " +
                                     gp::pathText(*path) + " lists them)");
            }
        }
        const gp::ArgSpec* arg = argNamed(*spec, from.arg);
        const std::size_t count = ++fromCount[from.arg];
        const bool list = arg->type == gp::ArgType::DatasetList && arg->maxCount != 1;
        if (count > 1 && !list) {
            return makeError(ErrorCode::InvalidArgument,
                             from.arg + " takes one dataset, and FROM gives it more", from.arg);
        }
        if (named.contains(from.arg)) {
            return makeError(ErrorCode::InvalidArgument,
                             from.arg + " is given twice: by GDAL's words and by FROM", from.arg);
        }
    }

    // Where the output goes.
    const gp::ArgSpec* output = nullptr;
    for (const gp::ArgSpec& arg : spec->args) {
        if (arg.isOutput && arg.type == gp::ArgType::Dataset) {
            output = &arg;
            break;
        }
    }
    std::vector<std::string> fromArgs;
    for (const auto& [arg, count] : fromCount) {
        fromArgs.push_back(arg);
    }
    auto given = gp::argumentsGiven(*path, tail, fromArgs);
    if (!given) {
        return given.error();
    }
    const bool tailNamesOutput =
        output != nullptr && std::ranges::find(*given, output->name) != given->end();
    if (tailNamesOutput && target) {
        return makeError(ErrorCode::InvalidArgument,
                         "the output is named twice: in GDAL's words and by TO");
    }
    if (target && output == nullptr) {
        return makeError(ErrorCode::InvalidArgument,
                         gp::pathText(*path) + " writes no dataset for TO to take");
    }
    Target to = target.value_or(Target{});
    if (tailNamesOutput) {
        to.kind = Target::Kind::File; // the name GDAL's words gave
    }
    const unsigned kinds = output != nullptr ? output->datasetKinds : 0u;
    const bool rasterOnly = kinds == gp::DatasetKind::Raster;
    const bool vectorOnly = kinds == gp::DatasetKind::Vector;
    if (to.kind == Target::Kind::Layer && rasterOnly) {
        return makeError(ErrorCode::Unsupported,
                         gp::pathText(*path) +
                             " makes a raster, which cannot go to a layer; TO REFERENCE [<name>] "
                             "keeps it as a reference raster, TO FILE <path> writes it");
    }
    if (to.kind == Target::Kind::Reference && vectorOnly) {
        return makeError(ErrorCode::Unsupported,
                         gp::pathText(*path) +
                             " makes features, which cannot be a reference raster; TO LAYER "
                             "<path> draws them, TO FILE <path> writes them");
    }
    if (to.kind == Target::Kind::File && !tailNamesOutput) {
        std::error_code error;
        const std::u8string name(to.name.begin(), to.name.end());
        if (std::filesystem::exists(std::filesystem::path(name), error) && !overwrite) {
            return makeError(ErrorCode::AlreadyExists,
                             "the file exists; add OVERWRITE to replace it", to.name);
        }
    }

    gp::RunRequest request;
    request.path = *path;
    request.tokens = tail;
    request.overwrite = overwrite;
    if (to.kind == Target::Kind::File && !tailNamesOutput) {
        request.outputTo = gp::OutputTo::File;
        request.outputPath = to.name;
        request.outputFormat = to.format;
    } else {
        // Every raster result is written where a derived raster is kept, so
        // it becomes a reference raster without passing through memory.
        request.outputTo = gp::OutputTo::Memory;
        request.maxMemoryCells = 0;
        const std::filesystem::path folder = derivedFolder(context);
        const std::u8string text = folder.generic_u8string();
        request.spillDirectory = std::string(text.begin(), text.end());
    }

    // The sources: the drawing's copied now, the rest made by the work.
    std::vector<std::string> records;
    std::vector<BoundArg> bound;
    bool tookNothing = false;
    for (FromClause& from : froms) {
        const gp::ArgSpec* arg = argNamed(*spec, from.arg);
        auto slot = std::ranges::find_if(bound, [&](const BoundArg& b) { return b.arg == from.arg; });
        if (slot == bound.end()) {
            bound.push_back(BoundArg{from.arg, arg->type == gp::ArgType::DatasetList, {}});
            slot = std::prev(bound.end());
        }
        if (from.source.kind == Source::Kind::Drawing) {
            igeo::DrawingDatasetOptions options;
            options.crsWkt = projectCrs(context);
            // An algorithm that reads Z is given only what has a height at
            // every vertex: GDAL reads a missing one as 0.
            options.requireHeights = gp::readsHeights(*path, tail);
            auto drawing = bindDrawing(context, from.source.scope, options);
            if (!drawing) {
                return drawing.error();
            }
            records.push_back(inputRecord(from.arg, from.source));
            records.push_back(scopeRecord(from.arg, *drawing));
            for (const std::string& warning : drawing->dataset.stats.warnings) {
                records.push_back(warningRecord(warning));
            }
            tookNothing = tookNothing || drawing->dataset.set.tables.empty();
            auto set = std::make_shared<const gp::FeatureSet>(std::move(drawing->dataset.set));
            slot->datasets.push_back([set]() -> Result<gp::DatasetValue> { return gp::DatasetValue(*set); });
            continue;
        }
        if (from.source.kind == Source::Kind::Surface && !from.source.cell) {
            if (const katana::terrain::NamedSurface* surface = context.surfaces.find(from.source.surface)) {
                from.source.cell = katana::interop::suggestedCellSize(surface->surface->bounds());
            }
        }
        auto dataset = bindRaster(context, from.source);
        if (!dataset) {
            return dataset.error();
        }
        records.push_back(inputRecord(from.arg, from.source));
        slot->datasets.push_back(std::move(dataset).value());
    }

    const auto materialise = [bound](gp::RunRequest& run) -> katana::core::Status {
        for (const BoundArg& arg : bound) {
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
    };

    // A scope that took nothing is said, as Global Modify's preview says it,
    // and nothing runs: GDAL has no dataset to be given.
    if (tookNothing) {
        std::vector<std::string> reply{"gdal algorithm=" + value(gp::pathText(*path)) + " policy=" +
                                       std::string(gp::toString(info->policy)) +
                                       " seconds=0.000 cancelled=no ran=no"};
        reply.insert(reply.end(), records.begin(), records.end());
        return answered("GDAL " + gp::pathText(*path), joinedRecords(reply));
    }

    if (preview) {
        gp::RunRequest check = request;
        if (auto made = materialise(check); !made) {
            return made.error();
        }
        if (auto valid = gp::validate(check); !valid) {
            return valid.error();
        }
        std::vector<std::string> reply{"gdal algorithm=" + value(gp::pathText(*path)) + " policy=" +
                                       std::string(gp::toString(info->policy)) + " preview=yes"};
        reply.insert(reply.end(), records.begin(), records.end());
        reply.push_back("preview valid=yes changed=no");
        return answered("GDAL " + gp::pathText(*path), joinedRecords(reply));
    }

    ApplyRequest apply;
    apply.target = to;
    apply.arg = output != nullptr ? output->name : "output";
    apply.defaultName = path->back();
    apply.result.operation = gp::pathText(*path);
    apply.result.commandName = std::string(katana::core::trimmed(line));
    const gp::AlgorithmInfo algorithm = *info;

    Prepared prepared;
    prepared.title = "GDAL " + gp::pathText(*path);
    prepared.work = [request, materialise, records, apply, algorithm](
                        const std::stop_token& stop, const Progress& progress) -> Result<Apply> {
        gp::RunRequest run = request;
        if (auto made = materialise(run); !made) {
            return made.error();
        }
        auto outputs = gp::run(run, stop, progress);
        if (!outputs) {
            return outputs.error();
        }
        auto kept = std::make_shared<gp::RunOutputs>(std::move(outputs).value());
        return Apply([kept, records, apply, algorithm](Context& ctx) -> Result<std::string> {
            const double seconds = kept->seconds;
            auto applied = applyOutputs(ctx, apply, std::move(*kept));
            if (!applied) {
                return applied.error();
            }
            std::vector<std::string> reply{gdalRecord(algorithm, seconds)};
            reply.insert(reply.end(), records.begin(), records.end());
            if (!applied->empty()) {
                reply.push_back(*applied);
            }
            return joinedRecords(reply);
        });
    };
    return prepared;
}

} // namespace

Result<Prepared> prepareGdal(Context& context, const Tokens& tokens, std::string_view line)
{
    std::size_t at = 1;
    if (at >= tokens.size()) {
        return makeError(ErrorCode::InvalidArgument, std::string("usage: ") + kUsage);
    }
    if (tokens.is(at, "VERSION")) {
        if (at + 1 != tokens.size()) {
            return makeError(ErrorCode::InvalidArgument, "GDAL VERSION takes nothing more",
                             tokens[at + 1]);
        }
        return answered("GDAL VERSION", versionRecord(gp::versions()));
    }
    if (tokens.is(at, "LIST")) {
        return list(tokens, at + 1);
    }
    if (tokens.is(at, "HELP")) {
        return help(tokens, at + 1);
    }
    if (tokens.is(at, "RUN")) {
        ++at;
    }
    return runLine(context, tokens, at, line);
}

std::string gdalUsage()
{
    return kUsage;
}

} // namespace katana::app::geo
