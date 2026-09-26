// GIS OVERLAY (docs/geoprocessing.md, "V2"): the polygon booleans Katana's
// geometry did not have (include/katana/geometry/polygon.hpp) - easement
// area per lot, net developable area, lots split by zone or flood extent,
// pipe length per lot - through GDAL's `vector layer-algebra`, between the
// scope and a second scope or a file.
//
//   GIS OVERLAY intersection|difference|union|symdifference|identity|update|clip
//       <scope> WITH (<scope> | FILE <path> [LAYER <name>] [where="<sql>"])
//       [keep=a,b|all|none] [keepwith=a,b|all|none] [TO LAYER <path>]
//       [csv=<file>] [OVERWRITE] [PREVIEW]
//
// The second clause is read by the same scope parser as the first: the
// subject's words end at WITH, which is no scope word, and the overlay's
// begin after it (vector_support.hpp's reading of a clause).
//
// What was measured, and so is relied on here:
//   - layer-algebra reads ONE input layer and ONE method layer ("Cannot get
//     input layer ''" for a dataset of several), so each of the subject's
//     tables is its own run, and the overlay is its areas;
//   - it names the fields it carries input_<field> and method_<field>;
//   - a line against areas comes back cut at their edges, which is how a
//     pipe's length per lot is measured.

#include <algorithm>
#include <chrono>
#include <filesystem>
#include <fstream>
#include <map>
#include <memory>
#include <set>
#include <string>
#include <system_error>
#include <utility>
#include <vector>

#include "bindings.hpp"
#include "geo_verbs.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/layer_path.hpp"
#include "katana/interop/geo/drawing_dataset.hpp"
#include "replies.hpp"
#include "vector_support.hpp"
#include "verb_table.hpp"

namespace katana::app::geo {

namespace gp = katana::gis::processing;
namespace igeo = katana::interop::geo;
namespace vec = katana::app::geo::vector;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::gis::GeometryKind;

namespace {

constexpr const char* kUsage =
    "GIS OVERLAY intersection|difference|union|symdifference|identity|update|clip <scope> WITH "
    "(<scope> | FILE <path> [LAYER <name>] [where=\"<sql>\"]) [keep=a,b|all|none] "
    "[keepwith=a,b|all|none] [TO LAYER <path>] [csv=<file>] [OVERWRITE] [PREVIEW]";

using Clock = std::chrono::steady_clock;

// Katana's word for an operation, and GDAL's. GDAL's own are taken too.
const std::vector<std::pair<std::string, std::string>>& operations()
{
    static const std::vector<std::pair<std::string, std::string>> table{
        {"intersection", "intersection"}, {"difference", "erase"},
        {"union", "union"},               {"symdifference", "sym-difference"},
        {"identity", "identity"},         {"update", "update"},
        {"clip", "clip"},                 {"erase", "erase"},
        {"sym-difference", "sym-difference"}};
    return table;
}

// The operations whose result holds pieces of the overlay the subject does
// not reach: run once per subject table they would repeat those pieces, so
// they take the subject's areas only.
bool areasOnly(const std::string& gdal)
{
    return gdal == "union" || gdal == "sym-difference" || gdal == "update";
}

// --input-field / --method-field words for keep= / keepwith=. katana_id is
// always carried: it is how a piece names the entities it came from.
Result<std::vector<std::string>> fieldWords(const std::string* keep, const std::string& side)
{
    if (keep == nullptr || katana::core::equalsIgnoringCase(*keep, "all")) {
        return std::vector<std::string>{};
    }
    std::vector<std::string> fields = katana::core::equalsIgnoringCase(*keep, "none")
                                          ? std::vector<std::string>{}
                                          : vec::listOf(*keep);
    if (fields.empty() && !katana::core::equalsIgnoringCase(*keep, "none")) {
        return makeError(ErrorCode::InvalidArgument,
                         "keep= and keepwith= name the properties to carry, or all or none",
                         *keep);
    }
    if (std::ranges::find(fields, std::string("katana_id")) == fields.end()) {
        fields.insert(fields.begin(), "katana_id");
    }
    std::vector<std::string> words;
    for (const std::string& field : fields) {
        words.push_back("--" + side + "-field=" + field);
    }
    return words;
}

// The fields a piece carries, as the drawing's properties: input_owner is
// owner where only one side has an owner; katana_id names the subject
// (gis.source when drawn) and the overlay's becomes gis.with; the drawing's
// bookkeeping (layer, style, colour, type) is dropped.
gp::FeatureTable renamed(const gp::FeatureTable& table)
{
    std::map<std::string, int> sides; // base name -> sides it appears on
    const auto split = [](const std::string& name) -> std::pair<std::string, std::string> {
        if (name.starts_with("input_")) {
            return {"input", name.substr(6)};
        }
        if (name.starts_with("method_")) {
            return {"method", name.substr(7)};
        }
        return {"", name};
    };
    for (const gp::FieldDef& field : table.fields) {
        ++sides[split(field.name).second];
    }
    gp::FeatureTable out;
    out.name = table.name;
    out.kind = table.kind;
    out.hasZ = table.hasZ;
    out.crsWkt = table.crsWkt;
    std::vector<std::size_t> keptFrom;
    for (std::size_t f = 0; f < table.fields.size(); ++f) {
        const auto [side, base] = split(table.fields[f].name);
        std::string name = table.fields[f].name;
        if (base == "katana_id") {
            name = side == "method" ? "gis.with" : "katana_id";
        } else if (igeo::isBookkeepingField(base) && !side.empty()) {
            continue;
        } else if (!side.empty() && sides[base] == 1) {
            name = base;
        }
        out.fields.push_back({name, table.fields[f].type});
        keptFrom.push_back(f);
    }
    for (const gp::Feature& feature : table.features) {
        gp::Feature copy;
        copy.parts = feature.parts;
        for (const std::size_t f : keptFrom) {
            copy.values.push_back(f < feature.values.size() ? feature.values[f]
                                                            : gp::FieldValue(std::monostate{}));
        }
        out.features.push_back(std::move(copy));
    }
    return out;
}

std::string cellText(const gp::FieldValue& value)
{
    if (const auto* text = std::get_if<std::string>(&value)) {
        return *text;
    }
    if (const auto* whole = std::get_if<std::int64_t>(&value)) {
        return std::to_string(*whole);
    }
    if (const auto* real = std::get_if<double>(&value)) {
        return katana::core::formatExactReal(*real);
    }
    if (const auto* flag = std::get_if<bool>(&value)) {
        return *flag ? "true" : "false";
    }
    return {};
}

// One row per piece: which entities it came from, what it carries, and its
// area or length.
struct Row {
    std::vector<std::pair<std::string, std::string>> cells;
};

std::vector<Row> rowsOf(const gp::FeatureTable& table)
{
    std::vector<Row> rows;
    for (const gp::Feature& feature : table.features) {
        Row row;
        for (std::size_t f = 0; f < table.fields.size() && f < feature.values.size(); ++f) {
            const std::string& name = table.fields[f].name;
            const std::string key = name == "katana_id" ? "entity" : name == "gis.with" ? "with" : name;
            row.cells.emplace_back(key, cellText(feature.values[f]));
        }
        const double area = vec::featureArea(feature);
        const bool polygon = std::ranges::any_of(feature.parts, [](const katana::gis::VectorGeometry& part) {
            return part.kind == GeometryKind::Polygon;
        });
        if (polygon) {
            row.cells.emplace_back("area", fixed3(area));
        } else if (std::ranges::any_of(feature.parts, [](const katana::gis::VectorGeometry& part) {
                       return part.kind == GeometryKind::LineString;
                   })) {
            row.cells.emplace_back("length", fixed3(vec::featureLength(feature)));
        }
        rows.push_back(std::move(row));
    }
    return rows;
}

std::string rowRecord(const Row& row)
{
    std::string record = "row";
    for (const auto& [key, text] : row.cells) {
        record += " " + key + "=" + value(text);
    }
    return record;
}

std::string csvCell(const std::string& text)
{
    if (text.find_first_of(",\"\r\n") == std::string::npos) {
        return text;
    }
    std::string quoted = "\"";
    for (const char c : text) {
        quoted += c == '"' ? std::string("\"\"") : std::string(1, c);
    }
    return quoted + "\"";
}

// The rows as CSV, the columns in first-seen order.
katana::core::Status writeCsv(const std::string& path, const std::vector<Row>& rows)
{
    std::vector<std::string> columns;
    for (const Row& row : rows) {
        for (const auto& [key, text] : row.cells) {
            if (std::ranges::find(columns, key) == columns.end()) {
                columns.push_back(key);
            }
        }
    }
    std::ofstream out(std::filesystem::path(std::u8string(path.begin(), path.end())),
                      std::ios::binary | std::ios::trunc);
    if (!out) {
        return makeError(ErrorCode::FileExportFailure, "cannot write the rows", path);
    }
    std::string header;
    for (const std::string& column : columns) {
        header += (header.empty() ? "" : ",") + csvCell(column);
    }
    out << header << "\n";
    for (const Row& row : rows) {
        std::string line;
        for (std::size_t c = 0; c < columns.size(); ++c) {
            std::string cell;
            for (const auto& [key, text] : row.cells) {
                cell = key == columns[c] ? text : cell;
            }
            line += (c == 0 ? "" : ",") + csvCell(cell);
        }
        out << line << "\n";
    }
    if (!out) {
        return makeError(ErrorCode::FileExportFailure, "cannot write the rows", path);
    }
    return {};
}

// Where the overlay comes from: a second scope, or a file read on the worker.
struct Overlay {
    std::shared_ptr<const gp::FeatureSet> drawn; // the second scope's areas
    std::string path, layer, where;              // FILE
};

Result<Prepared> prepareOverlay(Context& context, const Tokens& tokens, std::string_view line)
{
    if (tokens.size() < 3) {
        return makeError(ErrorCode::InvalidArgument, std::string("usage: ") + kUsage);
    }
    const std::string asked = katana::core::lowered(tokens[2]);
    const auto operation = std::ranges::find_if(operations(), [&](const auto& entry) {
        return entry.first == asked;
    });
    if (tokens.quoted[2] || operation == operations().end()) {
        return makeError(ErrorCode::InvalidArgument,
                         "GIS OVERLAY takes intersection, difference, union, symdifference, "
                         "identity, update or clip first",
                         tokens[2]);
    }
    const std::string gdalOperation = operation->second;
    std::size_t with = tokens.size();
    for (std::size_t i = 3; i < tokens.size(); ++i) {
        if (tokens.is(i, "WITH")) {
            with = i;
            break;
        }
    }
    if (with == tokens.size()) {
        return makeError(ErrorCode::InvalidArgument,
                         std::string("GIS OVERLAY needs WITH and what to overlay: ") + kUsage);
    }
    const vec::WordRules rules{{"keep", "keepwith", "csv", "where"},
                               {"OVERWRITE", "PREVIEW"},
                               true,
                               kUsage};
    auto subjectWords = vec::readVerbWords(tokens, 3, with, rules);
    if (!subjectWords) {
        return subjectWords.error();
    }
    // After WITH: FILE <path> [LAYER <name>], or a scope; options anywhere.
    std::size_t from = with + 1;
    Overlay overlay;
    if (tokens.is(from, "FILE")) {
        if (from + 1 >= tokens.size()) {
            return makeError(ErrorCode::InvalidArgument, "FILE needs a path", "FILE");
        }
        overlay.path = tokens[from + 1];
        from += 2;
        if (tokens.is(from, "LAYER")) {
            if (from + 1 >= tokens.size()) {
                return makeError(ErrorCode::InvalidArgument, "LAYER needs the file's layer name",
                                 "LAYER");
            }
            overlay.layer = tokens[from + 1];
            from += 2;
        }
    }
    auto withWords = vec::readVerbWords(tokens, from, tokens.size(), rules);
    if (!withWords) {
        return withWords.error();
    }
    if (!overlay.path.empty() && withWords->scopeGiven) {
        return makeError(ErrorCode::InvalidArgument,
                         "WITH takes a scope or FILE <path>, not both");
    }
    if (overlay.path.empty() && !withWords->scopeGiven) {
        return makeError(ErrorCode::InvalidArgument,
                         std::string("WITH needs what to overlay: a scope or FILE <path>: ") +
                             kUsage);
    }
    // One set of options for the line, wherever they stood.
    vec::VerbWords words = std::move(subjectWords).value();
    for (const auto& [key, text] : withWords->options) {
        if (!words.options.emplace(key, text).second) {
            return makeError(ErrorCode::InvalidArgument, key + "= is given twice", key);
        }
    }
    for (const std::string& flag : withWords->flags) {
        if (!words.flags.insert(flag).second) {
            return makeError(ErrorCode::InvalidArgument, flag + " is given twice", flag);
        }
    }
    if (withWords->target) {
        if (words.target) {
            return makeError(ErrorCode::InvalidArgument, "one TO per line");
        }
        words.target = withWords->target;
    }
    if (const std::string* where = words.option("where")) {
        if (overlay.path.empty()) {
            return makeError(ErrorCode::InvalidArgument,
                             "where= filters a FILE overlay; a drawn one takes WHERE conditions",
                             "where=" + *where);
        }
        overlay.where = *where;
    }
    auto input = fieldWords(words.option("keep"), "input");
    if (!input) {
        return input.error();
    }
    auto method = fieldWords(words.option("keepwith"), "method");
    if (!method) {
        return method.error();
    }
    auto layer = vec::layerTarget(words, "GIS OVERLAY", "gis/overlay");
    if (!layer) {
        return layer.error();
    }
    if (auto valid = katana::entity::validateLayerPath(*layer); !valid) {
        return makeError(ErrorCode::InvalidArgument,
                         "TO LAYER needs a layer path: " + valid.error().message, *layer);
    }
    std::string csv;
    if (const std::string* file = words.option("csv")) {
        csv = *file;
        std::error_code error;
        if (std::filesystem::exists(std::filesystem::path(std::u8string(csv.begin(), csv.end())),
                                    error) &&
            !words.has("OVERWRITE")) {
            return makeError(ErrorCode::AlreadyExists,
                             "the file exists; add OVERWRITE to replace it", csv);
        }
    }

    auto subject = vec::bindScope(context, words.scope);
    if (!subject) {
        return subject.error();
    }
    std::vector<std::string> records{scopeRecord("input", *subject)};
    for (const std::string& warning : subject->dataset.stats.warnings) {
        records.push_back(warningRecord(warning));
    }
    bool tookNothing = subject->dataset.set.tables.empty();
    if (overlay.path.empty()) {
        auto drawn = vec::bindScope(context, withWords->scope);
        if (!drawn) {
            return drawn.error();
        }
        records.push_back(scopeRecord("method", *drawn));
        gp::FeatureSet areas;
        if (const gp::FeatureTable* polygons = vec::tableNamed(drawn->dataset.set, "polygons")) {
            areas.tables.push_back(*polygons);
        }
        const std::size_t notAreas = drawn->dataset.stats.points + drawn->dataset.stats.lines;
        if (notAreas != 0) {
            records.push_back(warningRecord(std::to_string(notAreas) +
                                            " points and lines WITH takes are not areas and are "
                                            "not overlaid"));
        }
        tookNothing = tookNothing || areas.tables.empty();
        overlay.drawn = std::make_shared<const gp::FeatureSet>(std::move(areas));
    } else {
        std::string record = "input arg=method source=file file=" + value(overlay.path);
        if (!overlay.layer.empty()) {
            record += " layer=" + value(overlay.layer);
        }
        if (!overlay.where.empty()) {
            record += " where=" + value(overlay.where);
        }
        records.push_back(record);
    }
    // The subject's tables, one run each; the area operations take areas.
    gp::FeatureSet subjectSet;
    std::size_t leftOut = 0;
    for (const gp::FeatureTable& table : subject->dataset.set.tables) {
        if (areasOnly(gdalOperation) && table.kind != GeometryKind::Polygon) {
            leftOut += table.features.size();
            continue;
        }
        subjectSet.tables.push_back(table);
    }
    if (leftOut != 0) {
        records.push_back(warningRecord(std::to_string(leftOut) + " points and lines are left out: " +
                                        asked + " is of areas"));
    }
    tookNothing = tookNothing || subjectSet.tables.empty();
    const std::string settings = "overlay operation=" + asked;
    if (tookNothing) {
        records.insert(records.begin(), vec::notRunRecord("overlay", ""));
        records.push_back(settings + " features=0");
        return vec::answered("GIS OVERLAY", vec::joined(records));
    }
    if (words.has("PREVIEW")) {
        records.insert(records.begin(), "gis op=overlay preview=yes");
        records.push_back(settings);
        records.push_back("preview valid=yes changed=no");
        return vec::answered("GIS OVERLAY", vec::joined(records));
    }

    std::vector<std::string> gdalWords{"--operation=" + gdalOperation};
    gdalWords.insert(gdalWords.end(), input->begin(), input->end());
    gdalWords.insert(gdalWords.end(), method->begin(), method->end());
    const auto subjects = std::make_shared<const gp::FeatureSet>(std::move(subjectSet));
    const std::string target = *layer;
    const std::string commandName(katana::core::trimmed(line));
    Prepared prepared;
    prepared.title = "GIS OVERLAY";
    prepared.work = [subjects, overlay, gdalWords, target, csv, settings, records, commandName](
                        const std::stop_token& stop, const Progress& progress) -> Result<Apply> {
        const Clock::time_point start = Clock::now();
        std::vector<std::string> warnings;
        gp::FeatureSet methodSet;
        if (overlay.drawn) {
            methodSet = *overlay.drawn;
        } else {
            // The file read here, on the worker, through GDAL's own filter:
            // one layer, the where= clause GDAL's SQL reads.
            gp::RunRequest read;
            read.path = {"vector", "filter"};
            gp::DatasetPath file;
            file.path = overlay.path;
            file.layer = overlay.layer;
            read.values.emplace_back("input", gp::ArgValue(gp::DatasetValue(file)));
            if (!overlay.where.empty()) {
                read.tokens.push_back("--where=" + overlay.where);
            }
            auto outputs = gp::run(read, stop, vec::progressSpan(progress, 0.0, 0.2));
            if (!outputs) {
                return outputs.error();
            }
            gp::FeatureSet fromFile = outputs->features.value_or(gp::FeatureSet{});
            if (fromFile.tables.size() > 1) {
                warnings.push_back("the file has " + std::to_string(fromFile.tables.size()) +
                                   " layers; the first, " + fromFile.tables.front().name +
                                   ", is overlaid - LAYER <name> names another");
                fromFile.tables.resize(1);
            }
            if (!fromFile.tables.empty() && fromFile.tables.front().kind != GeometryKind::Polygon) {
                return makeError(ErrorCode::InvalidArgument,
                                 "the file's layer holds no areas to overlay",
                                 fromFile.tables.front().name);
            }
            methodSet = std::move(fromFile);
        }
        std::vector<gp::FeatureTable> pieces;
        if (!methodSet.tables.empty() && !methodSet.tables.front().features.empty()) {
            std::size_t done = 0;
            for (const gp::FeatureTable& table : subjects->tables) {
                gp::FeatureSet one;
                one.tables.push_back(table);
                const double span = 0.8 / static_cast<double>(subjects->tables.size());
                auto out = vec::runVector(
                    {"vector", "layer-algebra"}, one, gdalWords, stop,
                    vec::progressSpan(progress, 0.2 + span * static_cast<double>(done),
                                      0.2 + span * static_cast<double>(done + 1)),
                    warnings, {{"method", methodSet}});
                if (!out) {
                    return out.error();
                }
                for (gp::FeatureTable& result : out->tables) {
                    pieces.push_back(renamed(result));
                }
                ++done;
            }
        } else {
            warnings.push_back("nothing to overlay: the file's layer holds no feature");
        }
        std::vector<const gp::FeatureTable*> tables;
        for (const gp::FeatureTable& table : pieces) {
            tables.push_back(&table);
        }
        gp::FeatureSet result;
        if (!tables.empty()) {
            result.tables.push_back(vec::mergedTable(tables, "overlay"));
        }
        result = vec::withoutEmpty(std::move(result));
        const vec::Measures measures = vec::measure(result);
        auto rows = std::make_shared<const std::vector<Row>>(
            result.tables.empty() ? std::vector<Row>{} : rowsOf(result.tables.front()));
        const std::string summary = settings + " features=" + std::to_string(measures.features) +
                                    " area=" + fixed3(measures.area) +
                                    " length=" + fixed3(measures.length) +
                                    (csv.empty() ? std::string() : " csv=" + value(csv));
        const double seconds = std::chrono::duration<double>(Clock::now() - start).count();
        auto kept = std::make_shared<const gp::FeatureSet>(std::move(result));
        return Apply([kept, rows, target, csv, summary, records, warnings, seconds,
                      commandName](Context& ctx) -> Result<std::string> {
            if (!csv.empty()) {
                if (auto written = writeCsv(csv, *rows); !written) {
                    return written.error();
                }
            }
            igeo::ResultOptions options;
            options.targetLayer = target;
            options.operation = "vector layer-algebra";
            options.commandName = commandName;
            auto made = igeo::resultCommand(ctx.document.model(), *kept, options);
            if (!made) {
                return made.error();
            }
            vec::Applied applied;
            applied.created = made->created;
            applied.skipped = made->skipped;
            auto created = vec::executeStep(ctx, std::move(made->command));
            if (!created) {
                return created.error();
            }
            std::vector<std::string> reply{vec::gisRecord("overlay", seconds)};
            reply.insert(reply.end(), records.begin(), records.end());
            reply.push_back(vec::outputRecord(target, applied));
            for (const Row& row : *rows) {
                reply.push_back(rowRecord(row));
            }
            reply.push_back(summary);
            for (const std::string& warning : made->warnings) {
                reply.push_back(warningRecord(warning));
            }
            for (const std::string& warning : warnings) {
                reply.push_back(warningRecord(warning));
            }
            return vec::joined(reply);
        });
    };
    return prepared;
}

} // namespace

Result<Prepared> prepareGisOverlay(Context& context, const Tokens& tokens, std::string_view line)
{
    return prepareOverlay(context, tokens, line);
}

} // namespace katana::app::geo
