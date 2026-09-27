// The RASTER ZONAL verb (docs/terrain.md, "Statistics by area"): the
// statistics of a raster inside each closed shape a scope takes, written on
// the shapes themselves as properties in ONE undo step - the same on
// katana_cli, katana_mcp and the window's command line, where Terrain >
// Analysis > Statistics by Area builds these lines.
//
//   RASTER ZONAL SURFACE <name> [CELL <m>] | RASTER <id|name> | FILE <path>
//                [<scope>] [stats=mean,min,max,count,sum] [prefix=zone]
//                [pixels=fractional|centre|all-touched] [csv=<file>] [OVERWRITE]
//                [PREVIEW]
//
// GDAL computes them (`raster zonal-stats`, exactextract's method): the
// zones are the scope's closed shapes - closed polylines and circles, a
// tagged hole joined to its area - handed over as one table carrying
// katana_id, which is how each result finds its entity again. Open lines and
// points bound no area: GDAL logs "Non-polygonal geometry" for one and the
// other zones then came back with zero counts (the investigators' finding),
// so they are left out before GDAL sees them, and counted.
//
// With pixels=fractional (the default) each cell counts by the part of it
// the shape covers, so `count` is an area in cells: a 40 x 30 m box on 1.5 m
// cells counts 1200 / 2.25 = 533.333 wherever it lies. centre takes a cell
// whose centre is inside, all-touched every cell the shape touches.
//
// The results become <prefix>_<stat> properties of each zone - zone_mean,
// zone_count ... - through the one result writer (ResultMode::SetProperties),
// after the zones are compared with the copies taken when the line was
// prepared: a zone edited while the job ran is not written over. A statistic
// GDAL has no number for (the mean of a zone off the raster) is no property,
// never 0.

#include <algorithm>
#include <cctype>
#include <chrono>
#include <cmath>
#include <cstdint>
#include <filesystem>
#include <memory>
#include <optional>
#include <string>
#include <system_error>
#include <utility>
#include <variant>
#include <vector>

#include "katana/commands/command_stack.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/model.hpp"
#include "replies.hpp"
#include "terrain_verbs.hpp"
#include "vector_support.hpp"

namespace katana::app::geo {

namespace gp = katana::gis::processing;
namespace igeo = katana::interop::geo;
namespace vec = katana::app::geo::vector;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

namespace {

using Clock = std::chrono::steady_clock;

constexpr const char* kUsage =
    "RASTER ZONAL SURFACE <name> [CELL <m>] | RASTER <id|name> | FILE <path> [<scope>] "
    "[stats=mean,min,max,count,sum] [prefix=zone] [pixels=fractional|centre|all-touched] "
    "[csv=<file>] [OVERWRITE] [PREVIEW]";

// The statistics of GDAL's that are one number per zone. The rest of its
// choices are lists (values, frac, unique, coverage) or need a weighting
// raster (weighted_*), and have no single property to become.
constexpr const char* kStats[] = {"count",        "sum",          "mean",         "min",
                                  "max",          "median",       "stdev",        "variance",
                                  "mode",         "minority",     "variety",      "center_x",
                                  "center_y",     "min_center_x", "min_center_y", "max_center_x",
                                  "max_center_y"};

const std::vector<std::string> kDefaultStats{"mean", "min", "max", "count", "sum"};

katana::core::Error refusal(const std::string& why, const std::string& word = {})
{
    return makeError(ErrorCode::InvalidArgument, why, word);
}

Result<std::vector<std::string>> statList(const std::string* text)
{
    if (text == nullptr) {
        return kDefaultStats;
    }
    std::vector<std::string> stats;
    for (const std::string& item : vec::listOf(*text)) {
        const std::string stat = katana::core::lowered(item);
        if (std::ranges::find_if(kStats, [&](const char* each) { return stat == each; }) ==
            std::end(kStats)) {
            std::string choices;
            for (const char* each : kStats) {
                choices += (choices.empty() ? "" : ",") + std::string(each);
            }
            return refusal("stats are among " + choices, item);
        }
        if (std::ranges::find(stats, stat) != stats.end()) {
            return refusal("a statistic is named twice", item);
        }
        stats.push_back(stat);
    }
    if (stats.empty()) {
        return refusal("stats= names at least one statistic: stats=mean,count", *text);
    }
    return stats;
}

// A property key's first part: letters, digits, '_', '.' and '-', so that
// zone_mean is a key a WHERE condition and a label can name.
bool validPrefix(const std::string& prefix)
{
    return !prefix.empty() && std::ranges::all_of(prefix, [](char c) {
        return std::isalnum(static_cast<unsigned char>(c)) != 0 || c == '_' || c == '.' ||
               c == '-';
    });
}

// What the work hands the apply.
struct Zonal {
    gp::FeatureSet properties; // one table: katana_id, then <prefix>_<stat>
    std::vector<vec::Row> rows;
    std::vector<std::string> warnings;
    double seconds = 0.0;
};

// GDAL's result table as the properties table and the rows: a field per
// statistic asked for, renamed <prefix>_<stat>; a non-finite number (the
// mean of nothing) a null, so it becomes no property.
Result<Zonal> zonalOf(const gp::FeatureSet& result, const std::vector<std::string>& stats,
                      const std::string& prefix)
{
    Zonal zonal;
    gp::FeatureTable table;
    table.name = "zones";
    table.fields.push_back({"katana_id", gp::FieldType::Integer64});
    const gp::FeatureTable* from = result.tables.empty() ? nullptr : &result.tables.front();
    if (from == nullptr) {
        zonal.properties.tables.push_back(std::move(table));
        return zonal;
    }
    const auto id = vec::fieldIndex(*from, "katana_id");
    if (!id) {
        return makeError(ErrorCode::CommandRejected,
                         "raster zonal-stats gave no katana_id to find the zones by");
    }
    std::vector<std::optional<std::size_t>> columns;
    for (const std::string& stat : stats) {
        const auto column = vec::fieldIndex(*from, stat);
        columns.push_back(column);
        if (!column) {
            zonal.warnings.push_back("raster zonal-stats gave no " + stat + " field");
            continue;
        }
        table.fields.push_back({prefix + "_" + stat, from->fields[*column].type});
    }
    for (const gp::Feature& feature : from->features) {
        gp::Feature made;
        made.values.push_back(*id < feature.values.size() ? feature.values[*id]
                                                          : gp::FieldValue(std::monostate{}));
        vec::Row row;
        row.cells.emplace_back("entity", vec::cellText(made.values.front()));
        for (std::size_t s = 0; s < stats.size(); ++s) {
            if (!columns[s]) {
                continue;
            }
            gp::FieldValue held = *columns[s] < feature.values.size() ? feature.values[*columns[s]]
                                                                      : gp::FieldValue{};
            if (const auto* number = std::get_if<double>(&held); number && !std::isfinite(*number)) {
                held = std::monostate{};
            }
            if (!std::holds_alternative<std::monostate>(held)) {
                row.cells.emplace_back(stats[s], vec::cellText(held));
            }
            made.values.push_back(std::move(held));
        }
        table.features.push_back(std::move(made));
        zonal.rows.push_back(std::move(row));
    }
    zonal.properties.tables.push_back(std::move(table));
    return zonal;
}

} // namespace

Result<Prepared> prepareRasterZonal(Context& context, const Tokens& tokens, std::string_view line)
{
    std::size_t at = 2;
    auto source = bindTerrainSource(context, tokens, at, "RASTER ZONAL");
    if (!source) {
        return source.error();
    }
    const vec::WordRules rules{{"stats", "prefix", "pixels", "csv"},
                               {"OVERWRITE", "PREVIEW"},
                               false,
                               kUsage};
    auto words = vec::readVerbWords(tokens, at, tokens.size(), rules);
    if (!words) {
        return words.error();
    }
    auto stats = statList(words->option("stats"));
    if (!stats) {
        return stats.error();
    }
    const std::string prefix = words->option("prefix") ? *words->option("prefix") : "zone";
    if (!validPrefix(prefix)) {
        return refusal("prefix is letters, digits, '_', '.' and '-': prefix=zone", prefix);
    }
    auto pixels = vec::choiceOption(*words, "pixels", {"fractional", "centre", "all-touched"},
                                    "fractional");
    if (!pixels) {
        return pixels.error();
    }
    // GDAL's own word for a cell whose centre is inside.
    const std::string gdalPixels = *pixels == "centre" ? "default" : *pixels;
    std::string csv;
    if (const std::string* file = words->option("csv")) {
        csv = *file;
        std::error_code error;
        if (std::filesystem::exists(std::filesystem::path(std::u8string(csv.begin(), csv.end())),
                                    error) &&
            !words->has("OVERWRITE")) {
            return makeError(ErrorCode::AlreadyExists, "the file exists; add OVERWRITE to replace it",
                             csv);
        }
    }

    auto bound = vec::bindScope(context, words->scope);
    if (!bound) {
        return bound.error();
    }
    std::vector<std::string> records{source->record, scopeRecord("zones", *bound)};
    const igeo::DrawingDatasetStats& taken = bound->dataset.stats;
    gp::FeatureSet zones;
    std::vector<katana::entity::EntityId> ids;
    if (const gp::FeatureTable* polygons = vec::tableNamed(bound->dataset.set, "polygons")) {
        // katana_id alone: GDAL copies the fields it is asked to include, and
        // the zones' other properties are theirs already.
        gp::FeatureTable table;
        table.name = polygons->name;
        table.kind = polygons->kind;
        table.hasZ = false;
        table.crsWkt = polygons->crsWkt;
        table.fields.push_back({"katana_id", gp::FieldType::Integer64});
        for (const gp::Feature& feature : polygons->features) {
            const auto id = vec::featureId(*polygons, feature);
            if (!id) {
                continue;
            }
            gp::Feature zone;
            zone.parts = feature.parts;
            for (katana::gis::VectorGeometry& part : zone.parts) {
                part.hasZ = false;
            }
            zone.values.push_back(static_cast<std::int64_t>(*id));
            table.features.push_back(std::move(zone));
            ids.push_back(*id);
        }
        if (!table.features.empty()) {
            zones.tables.push_back(std::move(table));
        }
    }
    records.push_back("zones used=" + std::to_string(ids.size()) +
                      " skipped.open=" + std::to_string(taken.lines) +
                      " skipped.points=" + std::to_string(taken.points));
    if (taken.lines + taken.points != 0) {
        records.push_back(warningRecord(std::to_string(taken.lines + taken.points) +
                                        " of what the scope took are not closed shapes, so bound "
                                        "no zone, and were left out"));
    }
    for (const std::string& warning : taken.warnings) {
        records.push_back(warningRecord(warning));
    }
    std::string settings = "zonal stats=";
    for (std::size_t s = 0; s < stats->size(); ++s) {
        settings += (s == 0 ? "" : ",") + (*stats)[s];
    }
    settings += " prefix=" + value(prefix) + " pixels=" + *pixels;
    if (ids.empty()) {
        records.insert(records.begin(), vec::notRunRecord("zonal", "zones=0"));
        records.push_back(settings);
        return vec::answered("RASTER ZONAL", vec::joined(records));
    }
    if (words->has("PREVIEW")) {
        records.insert(records.begin(), "gis op=zonal preview=yes");
        records.push_back(settings);
        records.push_back("preview valid=yes changed=no");
        return vec::answered("RASTER ZONAL", vec::joined(records));
    }

    Prepared prepared;
    prepared.title = "Statistics by Area";
    const DeferredDataset dataset = source->dataset;
    const auto shared = std::make_shared<const gp::FeatureSet>(std::move(zones));
    const auto copies = std::make_shared<const std::vector<katana::entity::Entity>>(
        vec::copiesOf(context.document, ids));
    const std::vector<std::string> statNames = *stats;
    const std::string commandName(katana::core::trimmed(line));
    prepared.work = [dataset, shared, copies, statNames, prefix, gdalPixels, csv, settings, records,
                     commandName](const std::stop_token& stop,
                                  const Progress& progress) -> Result<Apply> {
        const Clock::time_point start = Clock::now();
        auto input = dataset();
        if (!input) {
            return input.error();
        }
        gp::RunRequest request;
        request.path = {"raster", "zonal-stats"};
        request.values.emplace_back("input", gp::ArgValue(std::move(input).value()));
        request.values.emplace_back("zones", gp::ArgValue(gp::DatasetValue(*shared)));
        request.values.emplace_back("stat", gp::ArgValue(gp::Scalar(statNames)));
        request.values.emplace_back(
            "include-field", gp::ArgValue(gp::Scalar(std::vector<std::string>{"katana_id"})));
        request.values.emplace_back("pixels", gp::ArgValue(gp::Scalar(gdalPixels)));
        request.outputTo = gp::OutputTo::Memory;
        auto outputs = gp::run(request, stop, progress);
        if (!outputs) {
            return outputs.error();
        }
        auto zonal = zonalOf(outputs->features.value_or(gp::FeatureSet{}), statNames, prefix);
        if (!zonal) {
            return zonal.error();
        }
        for (const gp::Diagnostic& warning : outputs->diagnostics) {
            zonal->warnings.push_back(warning.message);
        }
        zonal->seconds = std::chrono::duration<double>(Clock::now() - start).count();
        auto kept = std::make_shared<const Zonal>(std::move(zonal).value());
        return Apply([kept, copies, csv, settings, records,
                      commandName](Context& ctx) -> Result<std::string> {
            // In place: the zones as they were when the line was read.
            if (auto same = unchangedSince(ctx, *copies); !same) {
                return same.error();
            }
            igeo::ResultOptions options;
            options.mode = igeo::ResultMode::SetProperties;
            options.commandName = commandName;
            auto plan = igeo::resultCommand(ctx.document.model(), kept->properties, options);
            if (!plan) {
                return plan.error();
            }
            // A statistic with no number this time takes away the number an
            // earlier run left, in the same step: a zone moved off the raster
            // must not keep its old mean.
            auto step = std::make_unique<katana::commands::Transaction>(commandName);
            if (plan->command) {
                step->add(std::move(plan->command));
            }
            std::size_t changed = 0;
            for (const gp::FeatureTable& table : kept->properties.tables) {
                for (const gp::Feature& feature : table.features) {
                    const auto id = vec::featureId(table, feature);
                    const katana::entity::Entity* entity =
                        id ? ctx.document.model().entities.find(*id) : nullptr;
                    if (entity == nullptr) {
                        continue;
                    }
                    // Whether this zone changes at all, for the output record.
                    bool any = false;
                    for (std::size_t f = 1; f < table.fields.size() && f < feature.values.size();
                         ++f) {
                        const std::string& key = table.fields[f].name;
                        const auto current = entity->properties.find(key);
                        if (std::holds_alternative<std::monostate>(feature.values[f])) {
                            if (current != entity->properties.end()) {
                                step->add(katana::commands::removeEntityProperty({entity->id}, key));
                                any = true;
                            }
                        } else if (const auto now = vec::propertyOf(feature.values[f])) {
                            any = any || current == entity->properties.end() ||
                                  current->second != *now;
                        }
                    }
                    changed += any ? 1U : 0U;
                }
            }
            vec::Applied applied;
            applied.updated = changed;
            applied.skipped = plan->skipped;
            katana::commands::CommandPtr command;
            if (step->size() != 0) {
                command = std::move(step);
            }
            // The file before the drawing, as GIS OVERLAY and GIS SQL write
            // theirs: a CSV that cannot be written is a failed reply, and a
            // failed reply must leave the drawing as it was.
            if (!csv.empty()) {
                if (auto written = vec::writeCsv(csv, kept->rows); !written) {
                    return written.error();
                }
            }
            auto executed = vec::executeStep(ctx, std::move(command));
            if (!executed) {
                return executed.error();
            }
            std::vector<std::string> reply{vec::gisRecord("zonal", kept->seconds)};
            reply.insert(reply.end(), records.begin(), records.end());
            reply.push_back(vec::outputRecord("", applied));
            for (const vec::Row& row : kept->rows) {
                std::string record = "zone";
                for (const auto& [key, text] : row.cells) {
                    record += " " + key + "=" + value(text);
                }
                reply.push_back(record);
            }
            reply.push_back(settings + " zones=" + std::to_string(kept->rows.size()) +
                            (csv.empty() ? std::string() : " csv=" + value(csv)));
            for (const std::string& warning : plan->warnings) {
                reply.push_back(warningRecord(warning));
            }
            for (const std::string& warning : kept->warnings) {
                reply.push_back(warningRecord(warning));
            }
            return vec::joined(reply);
        });
    };
    return prepared;
}

} // namespace katana::app::geo
