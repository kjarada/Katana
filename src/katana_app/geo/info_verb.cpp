// INFO (docs/interop.md, "Dataset information"): what a file, a folder or a
// URL holds, read without importing it, as records - or as GDAL's own JSON.
//
//   INFO <file|folder|url> [JSON] [STATS] [CHECK] [LAYER <name>]
//
// A dataset is interop::describeSource's: GDAL's own `raster info` and
// `vector info` through the bridge, a point cloud PDAL's. The records are
// read from the very JSON that INFO ... JSON gives, so the two cannot
// disagree. STATS computes every band's statistics without writing an
// .aux.xml beside the file; CHECK reads everything with GDAL's `dataset
// check` and says what failed; a folder is GDAL's `dataset identify`,
// recursive and detailed, beside the point clouds in it, which GDAL does not
// read. The read is the work, so the window reads a large file on a worker.
//
// INFO <id> stays the interpreter's - an entity - unless a file of that name
// exists; INFO #<id> is always the entity (takesInfo).

#include <filesystem>
#include <string>
#include <system_error>
#include <vector>

#include <nlohmann/json.hpp>

#include "../import_records.hpp"
#include "katana/core/text.hpp"
#include "katana/gis/processing.hpp"
#include "katana/interop/dataset_info.hpp"
#include "katana/interop/import.hpp"
#include "replies.hpp"
#include "verb_table.hpp"

namespace katana::app::geo {

namespace gp = katana::gis::processing;
namespace interop = katana::interop;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

namespace {

// GDAL's JSON keeps its own order, and INFO's wrapper the order it is
// written in: an ordered object, not a sorted one.
using Json = nlohmann::ordered_json;

constexpr const char* kUsage =
    "usage: INFO <file|folder|url> [JSON] [STATS] [CHECK] [LAYER <name>] | INFO <id>";

struct InfoRequest {
    std::filesystem::path path;
    bool json = false;
    bool stats = false;
    bool check = false;
    std::string layer;
};

bool keyword(const Tokens& tokens, std::size_t i)
{
    return tokens.is(i, "JSON") || tokens.is(i, "STATS") || tokens.is(i, "CHECK") ||
           tokens.is(i, "LAYER");
}

// The path is one quoted word, or - as INFO has always read it - the
// unquoted words up to the first keyword, with a blank between each.
Result<InfoRequest> parse(const Tokens& tokens)
{
    InfoRequest request;
    std::size_t at = 1;
    std::string path;
    if (at < tokens.size() && tokens.quoted[at]) {
        path = tokens[at++];
    } else {
        for (; at < tokens.size() && !keyword(tokens, at); ++at) {
            path += (path.empty() ? "" : " ") + tokens[at];
        }
    }
    if (path.empty()) {
        return makeError(ErrorCode::InvalidArgument, kUsage);
    }
    request.path = pathFromText(path);
    for (; at < tokens.size(); ++at) {
        if (tokens.is(at, "JSON")) {
            request.json = true;
        } else if (tokens.is(at, "STATS")) {
            request.stats = true;
        } else if (tokens.is(at, "CHECK")) {
            request.check = true;
        } else if (tokens.is(at, "LAYER")) {
            if (at + 1 >= tokens.size()) {
                return makeError(ErrorCode::InvalidArgument, "LAYER names a layer of the file");
            }
            request.layer = tokens[++at];
        } else {
            return makeError(ErrorCode::InvalidArgument,
                             "after the path INFO takes JSON, STATS, CHECK and LAYER <name>",
                             tokens[at]);
        }
    }
    return request;
}

std::string number(const std::optional<double>& value)
{
    return value ? recordNumber(*value) : std::string();
}

std::string yesNo(bool flag)
{
    return flag ? "yes" : "no";
}

// ---- a dataset -----------------------------------------------------------------------------

std::string datasetKind(const interop::SourceDescription& description)
{
    if (description.pointCloud) {
        return "pointcloud";
    }
    const bool raster = description.raster.has_value();
    const bool vector = !description.vectorLayers.empty() || !description.vectorJson.empty();
    return raster && vector ? "raster,vector" : raster ? "raster" : vector ? "vector" : "none";
}

std::vector<std::string> datasetRecords(const interop::SourceDescription& description,
                                        const InfoRequest& request)
{
    std::vector<std::string> records{"dataset file=" + value(pathText(request.path)) +
                                     " kind=" + datasetKind(description) +
                                     " driver=" + value(description.driver) +
                                     " crs=" + value(description.crs)};
    if (description.raster) {
        const interop::RasterDescription& raster = *description.raster;
        records.push_back("raster width=" + std::to_string(raster.width) +
                          " height=" + std::to_string(raster.height) +
                          " bands=" + std::to_string(raster.bandCount) +
                          " georeferenced=" + yesNo(raster.georeferenced) + " cell=" +
                          (raster.georeferenced ? recordNumber(raster.pixelWidth) + "," +
                                                      recordNumber(raster.pixelHeight)
                                                : std::string()) +
                          " bounds=" + boundsText(raster.bounds));
        for (const interop::BandDescription& band : raster.bands) {
            records.push_back("band band=" + std::to_string(band.band) + " type=" +
                              value(band.dataType) + " nodata=" + number(band.noData) +
                              " min=" + number(band.min) + " max=" + number(band.max) +
                              " mean=" + number(band.mean) + " stddev=" + number(band.stdDev) +
                              " color=" + value(band.colour) +
                              " overviews=" + std::to_string(band.overviews.size()));
        }
        for (const interop::BandDescription& band : raster.bands) {
            for (std::size_t k = 0; k < band.overviews.size(); ++k) {
                records.push_back("overview band=" + std::to_string(band.band) +
                                  " index=" + std::to_string(k + 1) +
                                  " width=" + std::to_string(band.overviews[k].first) +
                                  " height=" + std::to_string(band.overviews[k].second));
            }
        }
    }
    for (std::size_t k = 0; k < description.subdatasets.size(); ++k) {
        records.push_back("subdataset index=" + std::to_string(k + 1) +
                          " name=" + value(description.subdatasets[k].name) +
                          " description=" + value(description.subdatasets[k].description));
    }
    for (const interop::VectorLayerDescription& layer : description.vectorLayers) {
        records.push_back("layer name=" + value(layer.name) +
                          " features=" + std::to_string(layer.featureCount) +
                          " geometry=" + value(layer.geometryType) + " crs=" + value(layer.crs) +
                          " bounds=" + boundsText(layer.extent) +
                          " fields=" + std::to_string(layer.fields.size()));
    }
    for (const interop::VectorLayerDescription& layer : description.vectorLayers) {
        for (const interop::FieldDescription& field : layer.fields) {
            records.push_back("field layer=" + value(layer.name) + " name=" + value(field.name) +
                              " type=" + value(field.type) + " subtype=" + value(field.subtype) +
                              " width=" + (field.width > 0 ? std::to_string(field.width) : "") +
                              " precision=" +
                              (field.precision > 0 ? std::to_string(field.precision) : ""));
        }
    }
    if (description.pointCloud) {
        const interop::PointCloudDescription& cloud = *description.pointCloud;
        katana::geometry::Box2 plan;
        std::string zmin, zmax;
        if (!cloud.bounds.empty()) {
            plan.expand({cloud.bounds.minX, cloud.bounds.minY});
            plan.expand({cloud.bounds.maxX, cloud.bounds.maxY});
            zmin = recordNumber(cloud.bounds.minZ);
            zmax = recordNumber(cloud.bounds.maxZ);
        }
        records.push_back("pointcloud points=" + std::to_string(cloud.pointCount) +
                          " bounds=" + boundsText(plan) + " zmin=" + zmin + " zmax=" + zmax +
                          " color=" + yesNo(cloud.hasColor) + " copc=" + yesNo(cloud.copc));
    }
    if (request.stats && !description.raster) {
        records.push_back(warningText("STATS is a raster's: this file has no bands"));
    }
    return records;
}

Json parsedOrNull(const std::string& text)
{
    if (text.empty()) {
        return nullptr;
    }
    // GDAL's own words, verbatim; text GDAL wrote that is not JSON is kept
    // as text rather than lost.
    auto parsed = Json::parse(text, nullptr, false);
    return parsed.is_discarded() ? Json(text) : parsed;
}

Json pointCloudJson(const interop::SourceDescription& description)
{
    const interop::PointCloudDescription& cloud = *description.pointCloud;
    Json bounds = nullptr;
    if (!cloud.bounds.empty()) {
        bounds = {cloud.bounds.minX, cloud.bounds.minY, cloud.bounds.minZ,
                  cloud.bounds.maxX, cloud.bounds.maxY, cloud.bounds.maxZ};
    }
    return {{"driver", description.driver}, {"points", cloud.pointCount},
            {"bounds", bounds},             {"color", cloud.hasColor},
            {"copc", cloud.copc},           {"crs", description.crs}};
}

// ---- CHECK -----------------------------------------------------------------------------------

struct Checked {
    int code = 0;
    std::vector<std::string> problems;
};

// GDAL's `dataset check`: every value read, and what failed. A file GDAL
// cannot open at all is a failure of the line, not a problem found.
Result<Checked> check(const std::filesystem::path& path)
{
    gp::RunRequest request;
    request.path = {"dataset", "check"};
    request.values.emplace_back("input",
                                gp::ArgValue(gp::DatasetValue(gp::DatasetPath{pathText(path), {}, {}})));
    auto outputs = gp::run(request);
    if (!outputs) {
        return outputs.error();
    }
    Checked checked;
    checked.code = outputs->returnCode.value_or(0);
    for (const gp::Diagnostic& diagnostic : outputs->diagnostics) {
        checked.problems.push_back(diagnostic.message);
    }
    return checked;
}

// ---- a folder ------------------------------------------------------------------------------

struct Folder {
    std::string identified; // GDAL's JSON
    std::vector<interop::SourceDescription> clouds;
};

// GDAL's `dataset identify`, recursive and detailed, and the point clouds
// beside what it found: GDAL reads no LAS, so a survey folder's scans would
// otherwise be missing from what it holds.
Result<Folder> identify(const std::filesystem::path& folder)
{
    gp::RunRequest request;
    request.path = {"dataset", "identify"};
    request.tokens = {"--recursive", "--detailed", "--format=json", pathText(folder)};
    auto outputs = gp::run(request);
    if (!outputs) {
        return outputs.error();
    }
    Folder found;
    found.identified = outputs->text.value_or("[]");
    std::error_code error;
    for (auto entry = std::filesystem::recursive_directory_iterator(folder, error);
         !error && entry != std::filesystem::recursive_directory_iterator();
         entry.increment(error)) {
        if (entry->is_regular_file(error) &&
            interop::kindForPath(entry->path()) == interop::SourceKind::PointCloud) {
            if (auto cloud = interop::describeSource(entry->path())) {
                found.clouds.push_back(std::move(cloud).value());
            }
        }
    }
    return found;
}

std::vector<std::string> folderRecords(const std::filesystem::path& folder, const Folder& found)
{
    std::vector<std::string> files;
    const Json identified = parsedOrNull(found.identified);
    if (identified.is_array()) {
        for (const Json& each : identified) {
            // GDAL names a file with the separator this system uses; a record
            // names it as a line would.
            std::string record = "found file=" +
                                 value(pathText(pathFromText(each.value("name", std::string())))) +
                                 " driver=" + value(each.value("driver", std::string()));
            for (const auto& [key, flag] : each.items()) {
                if (flag.is_boolean()) {
                    record += " " + key + "=" + yesNo(flag.get<bool>());
                }
            }
            files.push_back(std::move(record));
        }
    }
    for (const interop::SourceDescription& cloud : found.clouds) {
        files.push_back("found file=" + value(pathText(cloud.path)) +
                        " driver=" + value(cloud.driver) + " has_crs=" + yesNo(!cloud.crs.empty()));
    }
    std::vector<std::string> records{"dataset file=" + value(pathText(folder)) +
                                     " kind=folder driver= crs= datasets=" +
                                     std::to_string(files.size())};
    records.insert(records.end(), files.begin(), files.end());
    return records;
}

std::string joined(const std::vector<std::string>& records)
{
    std::string text;
    for (const std::string& record : records) {
        text += (text.empty() ? "" : "\n") + record;
    }
    return text;
}

// The whole of INFO's work: pure, reading only the file.
Result<std::string> describe(const InfoRequest& request)
{
    std::error_code error;
    const bool folder = !interop::isVirtualPath(request.path) &&
                        std::filesystem::is_directory(request.path, error) &&
                        katana::core::lowered(pathText(request.path.extension())) != ".gdb";
    if (folder) {
        auto found = identify(request.path);
        if (!found) {
            return found.error();
        }
        if (request.json) {
            Json clouds = Json::array();
            for (const interop::SourceDescription& cloud : found->clouds) {
                Json entry = pointCloudJson(cloud);
                entry["file"] = pathText(cloud.path);
                clouds.push_back(std::move(entry));
            }
            return Json{{"path", pathText(request.path)},
                                  {"identify", parsedOrNull(found->identified)},
                                  {"pointclouds", clouds}}
                .dump();
        }
        return joined(folderRecords(request.path, *found));
    }

    interop::DescribeOptions options;
    options.statistics = request.stats;
    options.layer = request.layer;
    options.multidim = request.json;
    options.anyFormat = true;
    auto description = interop::describeSource(request.path, options);
    if (!description) {
        return description.error();
    }
    std::optional<Checked> checked;
    if (request.check) {
        auto result = check(request.path);
        if (!result) {
            return result.error();
        }
        checked = std::move(result).value();
    }

    if (request.json) {
        Json json{{"path", pathText(request.path)}};
        if (description->pointCloud) {
            json["pointcloud"] = pointCloudJson(*description);
        } else {
            json["raster"] = parsedOrNull(description->rasterJson);
            json["vector"] = parsedOrNull(description->vectorJson);
            json["multidim"] = parsedOrNull(description->multidimJson);
        }
        if (checked) {
            json["check"] = {{"return_code", checked->code}, {"problems", checked->problems}};
        }
        return json.dump();
    }
    std::vector<std::string> records = datasetRecords(*description, request);
    if (checked) {
        records.push_back("check code=" + std::to_string(checked->code) +
                          " problems=" + std::to_string(checked->problems.size()));
        for (const std::string& problem : checked->problems) {
            records.push_back("problem text=" + value(problem));
        }
    }
    return joined(records);
}

} // namespace

bool takesInfo(const Tokens& tokens)
{
    // INFO 12 is the interpreter's: an entity, described. Every INFO was once
    // taken for a file, so katana_describe_entity answered "file does not
    // exist" in every build with GDAL. A file that is really called 12 is
    // still described - but INFO #12 is always the entity: Entity
    // Information and katana_describe_entity send it, and a file called 1 or
    // #1 beside the program (a mistyped shell 2>1 makes one) once turned both
    // into "no importer reads files named ''".
    if (tokens.size() == 2 && katana::cad::CommandInterpreter::isEntityId(tokens[1])) {
        if (tokens[1].starts_with('#')) {
            return false;
        }
        std::error_code error;
        return std::filesystem::exists(pathFromText(tokens[1]), error);
    }
    return true;
}

Result<Prepared> prepareInfo(Context&, const Tokens& tokens, std::string_view)
{
    auto request = parse(tokens);
    if (!request) {
        return request.error();
    }
    Prepared prepared;
    prepared.title = "INFO " + pathText(request->path.filename());
    prepared.work = [request = *request](const std::stop_token& stop,
                                         const Progress&) -> Result<Apply> {
        auto reply = describe(request);
        if (!reply) {
            return reply.error();
        }
        if (stop.stop_requested()) {
            return makeError(ErrorCode::InvalidState, "cancelled");
        }
        return Apply([text = *reply](Context&) -> Result<std::string> { return text; });
    };
    return prepared;
}

} // namespace katana::app::geo
