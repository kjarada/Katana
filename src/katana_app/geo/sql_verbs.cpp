// GIS SQL (docs/geoprocessing.md, "V5"): the drawing queried with SQL -
// SQLite with Spatialite's functions by default, or OGR's own SQL - through
// GDAL's `vector sql`, over the tables the scope becomes.
//
//   GIS SQL "<select>" [<scope>] [dialect=sqlite|ogrsql]
//           [AS REPORT | AS SELECT | AS LAYER <layer>] [csv=<file>] [OVERWRITE] [PREVIEW]
//
// The tables are drawingDataset's: points, lines and polygons, each with
// katana_id, layer, style, colour and type, then the entities' properties as
// typed columns. AS REPORT (the default) answers the rows; AS SELECT selects
// the entities the katana_id column names; AS LAYER draws the geometry the
// query returns, as one undo step. AS is TO: TO REPORT, TO SELECTION and TO
// LAYER say the same, and are the GDAL verb's too (bindings.cpp).
//
// A query reads; it never writes. Only a SELECT is run, one statement, and
// SPATIALITE_SECURITY is never set, so Spatialite's file functions -
// BlobToFile, ExportGeoJSON ... - stay disabled.

#include <chrono>
#include <filesystem>
#include <memory>
#include <string>
#include <system_error>
#include <utility>
#include <vector>

#include "bindings.hpp"
#include "geo_verbs.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/layer_path.hpp"
#include "replies.hpp"
#include "vector_support.hpp"
#include "verb_table.hpp"

namespace katana::app::geo {

namespace gp = katana::gis::processing;
namespace vec = katana::app::geo::vector;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

namespace {

constexpr const char* kUsage =
    "GIS SQL \"<select>\" [<scope>] [dialect=sqlite|ogrsql] [AS REPORT | AS SELECT | AS LAYER "
    "<layer>] [csv=<file>] [OVERWRITE] [PREVIEW]";

using Clock = std::chrono::steady_clock;

// Only one SELECT: a query reads, and a second statement after a ';' could
// be anything.
katana::core::Status checkStatement(const std::string& sql)
{
    std::string_view body = katana::core::trimmed(sql);
    while (!body.empty() && (body.back() == ';' || katana::core::isAsciiSpace(body.back()))) {
        body.remove_suffix(1);
    }
    if (body.empty()) {
        return makeError(ErrorCode::InvalidArgument, std::string("GIS SQL needs a statement: ") + kUsage);
    }
    const std::size_t blank = body.find_first_of(" \t\r\n(");
    const std::string first = katana::core::lowered(body.substr(0, blank));
    if (first != "select") {
        return makeError(ErrorCode::InvalidArgument,
                         "GIS SQL runs a SELECT and nothing else: a query reads the drawing, it "
                         "never changes it",
                         std::string(body.substr(0, blank)));
    }
    // A ';' outside a quoted literal begins a second statement.
    char quote = 0;
    for (const char c : body) {
        if (quote != 0) {
            quote = c == quote ? 0 : quote;
        } else if (c == '\'' || c == '"' || c == '`') {
            quote = c;
        } else if (c == ';') {
            return makeError(ErrorCode::InvalidArgument, "GIS SQL runs one statement", sql);
        }
    }
    return {};
}

Result<Prepared> prepareSql(Context& context, const Tokens& tokens, std::string_view line)
{
    if (tokens.size() < 3 || !tokens.quoted[2]) {
        return makeError(ErrorCode::InvalidArgument,
                         std::string("the statement is one quoted word: ") + kUsage,
                         tokens.size() > 2 ? tokens[2] : std::string());
    }
    const std::string sql = tokens[2];
    if (auto valid = checkStatement(sql); !valid) {
        return valid.error();
    }
    vec::WordRules rules{{"dialect", "csv"}, {"OVERWRITE", "PREVIEW"}, true, kUsage};
    rules.targetWords = {"AS", "TO"};
    // AS SELECT is the target parser's SELECTION: the word SQL speaks.
    Tokens read = tokens;
    for (std::size_t i = 4; i < read.size(); ++i) {
        if (read.is(i, "SELECT") && read.is(i - 1, "AS")) {
            read.words[i] = "SELECTION";
        }
    }
    auto words = vec::readVerbWords(read, 3, read.size(), rules);
    if (!words) {
        return words.error();
    }
    auto dialect = vec::choiceOption(*words, "dialect", {"sqlite", "ogrsql"}, "sqlite");
    if (!dialect) {
        return dialect.error();
    }
    Target target;
    target.kind = Target::Kind::Report;
    if (words->target) {
        target = *words->target;
        if (target.kind != Target::Kind::Report && target.kind != Target::Kind::Selection &&
            target.kind != Target::Kind::Layer) {
            return makeError(ErrorCode::Unsupported,
                             "GIS SQL answers AS REPORT, AS SELECT or AS LAYER <layer>");
        }
        if (target.kind == Target::Kind::Layer) {
            if (auto valid = katana::entity::validateLayerPath(target.name); !valid) {
                return makeError(ErrorCode::InvalidArgument,
                                 "AS LAYER needs a layer path: " + valid.error().message,
                                 target.name);
            }
        }
    }
    std::string csv;
    if (const std::string* file = words->option("csv")) {
        if (target.kind != Target::Kind::Report) {
            return makeError(ErrorCode::InvalidArgument, "csv= writes a report's rows: AS REPORT",
                             "csv=" + *file);
        }
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
    std::vector<std::string> records{scopeRecord("input", *bound)};
    for (const std::string& warning : bound->dataset.stats.warnings) {
        records.push_back(warningRecord(warning));
    }
    std::string tables;
    for (const gp::FeatureTable& table : bound->dataset.set.tables) {
        tables += (tables.empty() ? "" : ",") + table.name;
    }
    const std::string settings = "sql dialect=" + *dialect + " tables=" + value(tables);
    if (bound->dataset.set.tables.empty()) {
        records.insert(records.begin(), vec::notRunRecord("sql", ""));
        records.push_back(settings + " rows=0");
        return vec::answered("GIS SQL", vec::joined(records));
    }
    if (words->has("PREVIEW")) {
        records.insert(records.begin(), "gis op=sql preview=yes");
        records.push_back(settings);
        records.push_back("preview valid=yes changed=no");
        return vec::answered("GIS SQL", vec::joined(records));
    }

    const auto input = std::make_shared<const gp::FeatureSet>(bound->dataset.set);
    const std::vector<std::string> gdalWords{"--sql=" + sql,
                                             std::string("--dialect=") +
                                                 (*dialect == "sqlite" ? "SQLITE" : "OGRSQL")};
    const std::string commandName(katana::core::trimmed(line));
    Prepared prepared;
    prepared.title = "GIS SQL";
    prepared.work = [input, gdalWords, target, csv, settings, records, commandName](
                        const std::stop_token& stop, const Progress& progress) -> Result<Apply> {
        const Clock::time_point start = Clock::now();
        std::vector<std::string> warnings;
        auto result = vec::runVector({"vector", "sql"}, *input, gdalWords, stop, progress, warnings);
        if (!result) {
            return result.error();
        }
        std::size_t rows = 0;
        for (const gp::FeatureTable& table : result->tables) {
            rows += table.features.size();
        }
        const std::string summary = settings + " rows=" + std::to_string(rows) +
                                    (csv.empty() ? std::string() : " csv=" + value(csv));
        const double seconds = std::chrono::duration<double>(Clock::now() - start).count();
        auto kept = std::make_shared<gp::FeatureSet>(std::move(result).value());
        return Apply([kept, target, csv, summary, records, warnings, seconds,
                      commandName](Context& ctx) -> Result<std::string> {
            if (!csv.empty()) {
                std::vector<vec::Row> lines;
                std::vector<std::string> columns;
                for (const gp::FeatureTable& table : kept->tables) {
                    for (const gp::FieldDef& field : table.fields) {
                        columns.push_back(vec::rowKey(field.name));
                    }
                    const std::vector<vec::Row> some = vec::rowsOf(table);
                    lines.insert(lines.end(), some.begin(), some.end());
                }
                if (auto written = vec::writeCsv(csv, lines, columns); !written) {
                    return written.error();
                }
            }
            ApplyRequest request;
            request.target = target;
            request.defaultName = "sql";
            request.result.operation = "vector sql";
            request.result.commandName = commandName;
            if (target.kind == Target::Kind::Layer) {
                // A query of the drawing holds its katana_id: a row drawn
                // back names the entity it came from, not a copy of it.
                request.result.targetLayer = target.name;
            }
            gp::RunOutputs outputs;
            outputs.features = std::move(*kept);
            auto applied = applyOutputs(ctx, request, std::move(outputs));
            if (!applied) {
                return applied.error();
            }
            std::vector<std::string> reply{vec::gisRecord("sql", seconds)};
            reply.insert(reply.end(), records.begin(), records.end());
            if (!applied->empty()) {
                reply.push_back(*applied);
            }
            reply.push_back(summary);
            for (const std::string& warning : warnings) {
                reply.push_back(warningRecord(warning));
            }
            return vec::joined(reply);
        });
    };
    return prepared;
}

} // namespace

Result<Prepared> prepareGisSql(Context& context, const Tokens& tokens, std::string_view line)
{
    return prepareSql(context, tokens, line);
}

} // namespace katana::app::geo
