// REFS (docs/interop.md, "Reference layers"): the reference layers - rasters
// and point clouds drawn under the drawing - listed, shown and hidden,
// restyled, renamed, removed, given overviews, and read again from the
// sources a project records.
//
//   REFS [LIST] [JSON]
//   REFS SHOW|HIDE|REMOVE|INFO <ref>
//   REFS OPACITY <ref> <0..1>
//   REFS COLOR <ref> elevation|intensity|classification|rgb|flat
//   REFS RENAME <ref> <name>
//   REFS OVERVIEWS <ref> [levels=2,4,8] CONFIRM
//   REFS RESTORE
//   <ref> := a layer's id, or its name
//
// A change to a layer is session data: not undoable, as reference data never
// was (interop/reference_data.hpp). What answers from what the session holds
// answers at once; OVERVIEWS writes beside the raster's file and RESTORE
// reads every source again, so they are work, run as jobs in the window.

#include <algorithm>
#include <filesystem>
#include <memory>
#include <optional>
#include <string>
#include <utility>
#include <vector>

#include "../import_records.hpp"
#include "gis_records.hpp"
#include "katana/core/text.hpp"
#include "katana/gis/gdal_adapter.hpp"
#include "katana/gis/processing.hpp"
#include "katana/interop/dataset_info.hpp"
#include "verb_table.hpp"

namespace katana::app::geo {

namespace gp = katana::gis::processing;
namespace interop = katana::interop;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

namespace {

constexpr const char* kUsage =
    "usage: REFS [LIST] [JSON] | REFS SHOW|HIDE|REMOVE|INFO <ref> | REFS OPACITY <ref> <0..1> | "
    "REFS COLOR <ref> elevation|intensity|classification|rgb|flat | REFS RENAME <ref> <name> | "
    "REFS OVERVIEWS <ref> [levels=2,4,8] CONFIRM | REFS RESTORE";

Prepared answered(std::string title, std::string reply)
{
    Prepared prepared;
    prepared.title = std::move(title);
    prepared.reply = std::move(reply);
    return prepared;
}

std::string joined(const std::vector<std::string>& records)
{
    std::string text;
    for (const std::string& record : records) {
        text += (text.empty() ? "" : "\n") + record;
    }
    return text;
}

// A missing record as REFS lists it: the layer a project names whose source
// could not be read when it opened.
std::string missingRecord(const std::string& record)
{
    auto source = interop::parseReferenceRecord(record);
    if (!source) {
        return "missing name= kind= file= reason=" + value(source.error().message);
    }
    return "missing name=" + value(source->name) + " kind=" +
           (source->kind == interop::ReferenceSource::Kind::Raster ? "raster" : "pointcloud") +
           " file=" + value(pathText(source->source));
}

std::vector<std::string> listed(const interop::ReferenceData& reference)
{
    std::vector<std::string> records;
    for (const interop::RasterOverlay& raster : reference.rasters()) {
        records.push_back(referenceRecord(raster));
    }
    for (const interop::PointCloudLayer& cloud : reference.pointClouds()) {
        records.push_back(referenceRecord(cloud));
    }
    for (const std::string& record : reference.missing()) {
        records.push_back(missingRecord(record));
    }
    // Said when there are none too: an empty reply reads as a line that did
    // nothing.
    records.push_back("references rasters=" + std::to_string(reference.rasters().size()) +
                      " clouds=" + std::to_string(reference.pointClouds().size()) +
                      " missing=" + std::to_string(reference.missing().size()));
    return records;
}

// ---- which layer -----------------------------------------------------------------------------

struct Found {
    interop::RasterOverlay* raster = nullptr;
    interop::PointCloudLayer* cloud = nullptr;
    [[nodiscard]] interop::ReferenceId id() const { return raster ? raster->id : cloud->id; }
    [[nodiscard]] std::string record() const
    {
        return raster ? referenceRecord(*raster) : referenceRecord(*cloud);
    }
};

// A layer by its id, or by its name in any case. NotFound for neither;
// InvalidArgument for a name several layers have, naming their ids.
Result<Found> find(interop::ReferenceData& reference, const std::string& word)
{
    const bool digits = !word.empty() && std::ranges::all_of(word, [](char c) {
        return c >= '0' && c <= '9';
    });
    if (const auto number = digits ? katana::core::parseInteger(word) : std::nullopt;
        number && *number > 0) {
        const auto id = static_cast<interop::ReferenceId>(*number);
        if (auto* raster = reference.findRaster(id)) {
            return Found{raster, nullptr};
        }
        if (auto* cloud = reference.findPointCloud(id)) {
            return Found{nullptr, cloud};
        }
    }
    std::vector<Found> named;
    for (const interop::RasterOverlay& raster : reference.rasters()) {
        if (katana::core::equalsIgnoringCase(raster.name, word)) {
            named.push_back({reference.findRaster(raster.id), nullptr});
        }
    }
    for (const interop::PointCloudLayer& cloud : reference.pointClouds()) {
        if (katana::core::equalsIgnoringCase(cloud.name, word)) {
            named.push_back({nullptr, reference.findPointCloud(cloud.id)});
        }
    }
    if (named.size() == 1) {
        return named.front();
    }
    if (named.empty()) {
        return makeError(ErrorCode::NotFound, "no reference layer has that id or name; REFS lists them",
                         word);
    }
    std::string ids;
    for (const Found& each : named) {
        ids += (ids.empty() ? "" : ", ") + std::to_string(each.id());
    }
    return makeError(ErrorCode::InvalidArgument,
                     "several reference layers are called that; name one by its id: " + ids, word);
}

// The layer the line's third word names, after checking the line's length.
Result<Found> target(Context& context, const Tokens& tokens, std::size_t words, const char* usage)
{
    if (tokens.size() != words) {
        return makeError(ErrorCode::InvalidArgument, std::string("usage: ") + usage);
    }
    return find(context.reference, tokens[2]);
}

void changed(Context& context)
{
    if (context.changed) {
        context.changed();
    }
}

// ---- the edits --------------------------------------------------------------------------------

Result<Prepared> showOrHide(Context& context, const Tokens& tokens, bool visible)
{
    auto found = target(context, tokens, 3, visible ? "REFS SHOW <ref>" : "REFS HIDE <ref>");
    if (!found) {
        return found.error();
    }
    (found->raster ? found->raster->visible : found->cloud->visible) = visible;
    changed(context);
    return answered(visible ? "REFS SHOW" : "REFS HIDE", found->record());
}

Result<Prepared> remove(Context& context, const Tokens& tokens)
{
    if (tokens.size() != 3) {
        return makeError(ErrorCode::InvalidArgument, "usage: REFS REMOVE <ref>");
    }
    auto found = find(context.reference, tokens[2]);
    if (!found) {
        // A layer the project names that could not be read is dropped from
        // the record by its name.
        if (found.error().code == ErrorCode::NotFound &&
            context.reference.forgetMissing(tokens[2])) {
            return answered("REFS REMOVE", "removed id= kind= name=" + value(tokens[2]) +
                                               " missing=yes");
        }
        return found.error();
    }
    const std::string record = "removed id=" + std::to_string(found->id()) +
                               " kind=" + (found->raster ? "raster" : "pointcloud") + " name=" +
                               value(found->raster ? found->raster->name : found->cloud->name);
    context.reference.remove(found->id());
    changed(context);
    return answered("REFS REMOVE", record);
}

Result<Prepared> info(Context& context, const Tokens& tokens)
{
    auto found = target(context, tokens, 3, "REFS INFO <ref>");
    if (!found) {
        return found.error();
    }
    std::vector<std::string> records{found->record()};
    if (found->raster) {
        const interop::RasterOverlay& raster = *found->raster;
        records.push_back("source file=" + value(pathText(raster.source)) + " url=" +
                          value(raster.sourceUrl) + " licence=" + value(raster.licence) +
                          " attribution=" + value(raster.attribution) +
                          " crs=" + value(raster.projectionWkt.empty()
                                              ? std::string()
                                              : katana::gis::describeCrs(raster.projectionWkt)));
        if (!raster.derivation.empty()) {
            // The line that made it: run again, it makes it again.
            records.push_back("derivation line=" + value(raster.derivation));
        }
    } else {
        const interop::PointCloudLayer& cloud = *found->cloud;
        records.push_back("source file=" + value(pathText(cloud.source)) +
                          " decimation=" + std::to_string(cloud.decimationStep) + " crs=" +
                          value(cloud.projectionWkt.empty()
                                    ? std::string()
                                    : katana::gis::describeCrs(cloud.projectionWkt)));
    }
    return answered("REFS INFO", joined(records));
}

Result<Prepared> opacity(Context& context, const Tokens& tokens)
{
    auto found = target(context, tokens, 4, "REFS OPACITY <ref> <0..1>");
    if (!found) {
        return found.error();
    }
    if (!found->raster) {
        return makeError(ErrorCode::Unsupported,
                         "a point cloud is drawn opaque; REFS COLOR chooses how it is coloured",
                         tokens[2]);
    }
    const auto amount = katana::core::parseFiniteDouble(tokens[3]);
    if (!amount || *amount < 0.0 || *amount > 1.0) {
        return makeError(ErrorCode::InvalidArgument,
                         "opacity is a number from 0 (clear) to 1 (opaque)", tokens[3]);
    }
    found->raster->opacity = *amount;
    changed(context);
    return answered("REFS OPACITY", found->record());
}

Result<Prepared> colour(Context& context, const Tokens& tokens)
{
    auto found = target(context, tokens, 4,
                        "REFS COLOR <ref> elevation|intensity|classification|rgb|flat");
    if (!found) {
        return found.error();
    }
    if (!found->cloud) {
        return makeError(ErrorCode::Unsupported,
                         "a raster is drawn in its own colours; REFS OPACITY sets how much of it "
                         "shows",
                         tokens[2]);
    }
    const auto mode = interop::pointColorModeFromWord(tokens[3]);
    if (!mode) {
        return makeError(ErrorCode::InvalidArgument,
                         "a point cloud is coloured by elevation, intensity, classification, rgb "
                         "or flat",
                         tokens[3]);
    }
    found->cloud->colorMode = *mode;
    changed(context);
    return answered("REFS COLOR", found->record());
}

Result<Prepared> rename(Context& context, const Tokens& tokens)
{
    auto found = target(context, tokens, 4, "REFS RENAME <ref> <name>");
    if (!found) {
        return found.error();
    }
    const std::string name(katana::core::trimmed(tokens[3]));
    if (name.empty()) {
        return makeError(ErrorCode::InvalidArgument, "a reference layer's name is not empty");
    }
    (found->raster ? found->raster->name : found->cloud->name) = name;
    changed(context);
    return answered("REFS RENAME", found->record());
}

// ---- OVERVIEWS ------------------------------------------------------------------------------

Result<Prepared> overviews(Context& context, const Tokens& tokens)
{
    bool confirm = false;
    std::string levels;
    std::size_t at = 3;
    if (tokens.size() < 3) {
        return makeError(ErrorCode::InvalidArgument,
                         "usage: REFS OVERVIEWS <ref> [levels=2,4,8] CONFIRM");
    }
    for (; at < tokens.size(); ++at) {
        const std::string& word = tokens[at];
        if (tokens.is(at, "CONFIRM")) {
            confirm = true;
        } else if (!tokens.quoted[at] && katana::core::lowered(word).starts_with("levels=")) {
            levels = word.substr(7);
        } else {
            return makeError(ErrorCode::InvalidArgument,
                             "REFS OVERVIEWS takes levels=2,4,8 and CONFIRM", word);
        }
    }
    auto found = find(context.reference, tokens[2]);
    if (!found) {
        return found.error();
    }
    if (!found->raster) {
        return makeError(ErrorCode::Unsupported, "overviews are a raster's", tokens[2]);
    }
    const std::filesystem::path source = found->raster->source;
    if (source.empty() || interop::isVirtualPath(source)) {
        return makeError(ErrorCode::Unsupported,
                         "overviews are written beside a raster's file, and this layer has none "
                         "on the disk",
                         found->raster->name);
    }
    std::filesystem::path ovr = source;
    ovr += ".ovr";
    // Writing beside a person's file is what Katana otherwise never does
    // (docs/geoprocessing.md, "Safety"): asked for by name.
    if (!confirm) {
        return makeError(ErrorCode::Unsupported,
                         "REFS OVERVIEWS writes " + pathText(ovr) +
                             " beside the raster's file; add CONFIRM to write it",
                         found->raster->name);
    }
    const interop::ReferenceId id = found->id();
    Prepared prepared;
    prepared.title = "REFS OVERVIEWS " + found->raster->name;
    prepared.work = [id, source, ovr, levels](const std::stop_token& stop,
                                             const Progress& progress) -> Result<Apply> {
        gp::RunRequest request;
        request.path = {"raster", "overview", "add"};
        request.values.emplace_back(
            "input", gp::ArgValue(gp::DatasetValue(gp::DatasetPath{pathText(source), {}, {}})));
        // External: an .ovr beside the file, never the file itself changed.
        request.tokens = {"--external", "--resampling=average"};
        if (!levels.empty()) {
            request.tokens.push_back("--levels=" + levels);
        }
        auto ran = gp::run(request, stop, progress);
        if (!ran) {
            return ran.error();
        }
        // What was made, as GDAL now reads the file.
        auto described = interop::describeSource(source);
        std::vector<std::string> records;
        std::size_t made = 0;
        if (described && described->raster && !described->raster->bands.empty()) {
            const interop::BandDescription& band = described->raster->bands.front();
            made = band.overviews.size();
            for (std::size_t k = 0; k < band.overviews.size(); ++k) {
                records.push_back("overview band=1 index=" + std::to_string(k + 1) +
                                  " width=" + std::to_string(band.overviews[k].first) +
                                  " height=" + std::to_string(band.overviews[k].second));
            }
        }
        const std::string head = "overviews id=" + std::to_string(id) + " file=" +
                                 value(pathText(ovr)) + " count=" + std::to_string(made) +
                                 " levels=" + value(levels);
        records.insert(records.begin(), head);
        return Apply([text = joined(records)](Context&) -> Result<std::string> { return text; });
    };
    return prepared;
}

// ---- RESTORE --------------------------------------------------------------------------------

// What RESTORE read of one recorded layer.
struct Restored {
    std::string record;
    std::optional<interop::ReferenceLayer> layer;
    std::string why; // when it could not be read
};

Result<Prepared> restore(Context& context, const Tokens& tokens)
{
    if (tokens.size() != 2) {
        return makeError(ErrorCode::InvalidArgument, "usage: REFS RESTORE");
    }
    const std::vector<std::string> records = context.document.metadata().referenceLayers;
    const std::optional<std::filesystem::path> project = context.document.projectDirectory();
    Prepared prepared;
    prepared.title = "REFS RESTORE";
    prepared.work = [records, project](const std::stop_token& stop,
                                       const Progress& progress) -> Result<Apply> {
        auto read = std::make_shared<std::vector<Restored>>();
        for (std::size_t k = 0; k < records.size(); ++k) {
            if (stop.stop_requested()) {
                return makeError(ErrorCode::InvalidState, "cancelled");
            }
            Restored one{records[k], std::nullopt, {}};
            auto source = interop::parseReferenceRecord(records[k]);
            if (!source) {
                one.why = source.error().describe();
            } else if (auto layer = interop::readReference(*source, project)) {
                one.layer = std::move(layer).value();
            } else {
                one.why = layer.error().describe();
            }
            read->push_back(std::move(one));
            if (progress) {
                progress(static_cast<double>(k + 1) / static_cast<double>(records.size()));
            }
        }
        return Apply([read](Context& ctx) -> Result<std::string> {
            // The layers become those the project records: what was held is
            // let go first, so a RESTORE typed twice does not double them.
            ctx.reference.clear();
            std::vector<std::string> reply;
            std::size_t restored = 0;
            for (Restored& one : *read) {
                if (!one.layer) {
                    // Warned of, not fatal: the drawing opens without it, and
                    // the record is kept, so a save does not drop the layer
                    // because its drive was not there today.
                    ctx.reference.keepMissing(one.record);
                    reply.push_back(warningRecord(one.why));
                    continue;
                }
                ++restored;
                if (auto* raster = std::get_if<interop::RasterOverlay>(&*one.layer)) {
                    const interop::ReferenceId id = ctx.reference.add(std::move(*raster));
                    reply.push_back(referenceRecord(*ctx.reference.findRaster(id)));
                } else {
                    const interop::ReferenceId id =
                        ctx.reference.add(std::get<interop::PointCloudLayer>(std::move(*one.layer)));
                    reply.push_back(referenceRecord(*ctx.reference.findPointCloud(id)));
                }
            }
            reply.push_back("restored layers=" + std::to_string(restored) +
                            " missing=" + std::to_string(read->size() - restored));
            if (ctx.changed) {
                ctx.changed();
            }
            return joined(reply);
        });
    };
    return prepared;
}

} // namespace

Result<Prepared> prepareRefs(Context& context, const Tokens& tokens, std::string_view)
{
    if (tokens.size() == 1 || tokens.is(1, "LIST") || tokens.is(1, "JSON")) {
        bool json = false;
        for (std::size_t at = 1; at < tokens.size(); ++at) {
            if (tokens.is(at, "JSON")) {
                json = true;
            } else if (!(at == 1 && tokens.is(at, "LIST"))) {
                return makeError(ErrorCode::InvalidArgument, kUsage, tokens[at]);
            }
        }
        const std::vector<std::string> records = listed(context.reference);
        if (!json) {
            return answered("REFS", joined(records));
        }
        nlohmann::json layers = nlohmann::json::array();
        nlohmann::json missing = nlohmann::json::array();
        for (const Record& record : parseRecords(joined(records))) {
            if (record.kind == "reference") {
                layers.push_back(recordJson(record));
            } else if (record.kind == "missing") {
                missing.push_back(recordJson(record));
            }
        }
        return answered("REFS", nlohmann::json{{"references", layers}, {"missing", missing}}.dump());
    }
    if (tokens.is(1, "SHOW") || tokens.is(1, "HIDE")) {
        return showOrHide(context, tokens, tokens.is(1, "SHOW"));
    }
    if (tokens.is(1, "REMOVE")) {
        return remove(context, tokens);
    }
    if (tokens.is(1, "INFO")) {
        return info(context, tokens);
    }
    if (tokens.is(1, "OPACITY")) {
        return opacity(context, tokens);
    }
    if (tokens.is(1, "COLOR") || tokens.is(1, "COLOUR")) {
        return colour(context, tokens);
    }
    if (tokens.is(1, "RENAME")) {
        return rename(context, tokens);
    }
    if (tokens.is(1, "OVERVIEWS")) {
        return overviews(context, tokens);
    }
    if (tokens.is(1, "RESTORE")) {
        return restore(context, tokens);
    }
    return makeError(ErrorCode::InvalidArgument, kUsage, tokens[1]);
}

} // namespace katana::app::geo
