#include "katana/survey/subsurface/utility_csv.hpp"

#include <cmath>
#include <map>
#include <optional>
#include <utility>

#include "katana/core/text.hpp"

namespace katana::survey::subsurface {

namespace {

using core::ErrorCode;
using core::makeError;

std::string key(std::string_view text)
{
    std::string out;
    for (const char ch : text) {
        if (!core::isAsciiSpace(ch) && ch != '-' && ch != '_') {
            out += core::asciiLower(ch);
        }
    }
    return out;
}

std::string at(std::size_t lineNumber)
{
    return "line " + std::to_string(lineNumber);
}

// The comma-separated fields of one line, trimmed, with double quotes
// removed. nullopt for an unterminated quote or text after a closing quote.
std::optional<std::vector<std::string>> splitFields(std::string_view line)
{
    std::vector<std::string> fields;
    std::size_t i = 0;
    while (true) {
        while (i < line.size() && core::isAsciiSpace(line[i])) {
            ++i;
        }
        std::string field;
        if (i < line.size() && line[i] == '"') {
            ++i;
            bool closed = false;
            while (i < line.size()) {
                if (line[i] == '"') {
                    if (i + 1 < line.size() && line[i + 1] == '"') {
                        field += '"';
                        i += 2;
                        continue;
                    }
                    ++i;
                    closed = true;
                    break;
                }
                field += line[i++];
            }
            if (!closed) {
                return std::nullopt;
            }
            while (i < line.size() && core::isAsciiSpace(line[i])) {
                ++i;
            }
            if (i < line.size() && line[i] != ',') {
                return std::nullopt;
            }
        } else {
            const std::size_t comma = line.find(',', i);
            const std::size_t end = comma == std::string_view::npos ? line.size() : comma;
            field = std::string(core::trimmed(line.substr(i, end - i)));
            i = end;
        }
        fields.push_back(std::move(field));
        if (i >= line.size()) {
            return fields;
        }
        ++i; // the comma
    }
}

struct Row {
    std::size_t lineNumber = 0;
    std::vector<std::string> fields;
};

struct Table {
    std::map<std::string, std::size_t> columns; // canonical name -> field index
    std::size_t width = 0;
    std::vector<Row> rows;

    // The trimmed cell, or empty when the column is absent.
    [[nodiscard]] std::string_view cell(const Row& row, std::string_view name) const
    {
        const auto found = columns.find(std::string(name));
        return found == columns.end() ? std::string_view{}
                                      : std::string_view(row.fields[found->second]);
    }
};

// The header and the rows of `text`, with columns resolved against `known`.
core::Result<Table> readTable(std::string_view text, const std::vector<UtilityCsvColumn>& known)
{
    if (text.starts_with("\xEF\xBB\xBF")) {
        text.remove_prefix(3);
    }
    Table table;
    bool haveHeader = false;
    const std::vector<std::string_view> lines = core::splitLines(text);
    for (std::size_t index = 0; index < lines.size(); ++index) {
        const std::size_t lineNumber = index + 1;
        const std::string_view line = core::trimmed(lines[index]);
        if (line.empty() || line.front() == '#') {
            continue;
        }
        auto fields = splitFields(line);
        if (!fields) {
            return makeError(ErrorCode::ParseFailure, "unbalanced quotes", at(lineNumber));
        }
        if (!haveHeader) {
            haveHeader = true;
            table.width = fields->size();
            for (std::size_t column = 0; column < fields->size(); ++column) {
                const std::string name = key((*fields)[column]);
                const UtilityCsvColumn* match = nullptr;
                for (const UtilityCsvColumn& candidate : known) {
                    if (name == key(candidate.name)) {
                        match = &candidate;
                    }
                    for (const std::string_view alias : candidate.aliases) {
                        if (name == key(alias)) {
                            match = &candidate;
                        }
                    }
                }
                if (!match) {
                    return makeError(ErrorCode::ParseFailure,
                                     "unknown column \"" + (*fields)[column] + "\"",
                                     at(lineNumber));
                }
                if (!table.columns.emplace(std::string(match->name), column).second) {
                    return makeError(ErrorCode::ParseFailure,
                                     "column \"" + std::string(match->name) + "\" given twice",
                                     at(lineNumber));
                }
            }
            for (const UtilityCsvColumn& column : known) {
                if (column.required && !table.columns.contains(std::string(column.name))) {
                    return makeError(ErrorCode::ParseFailure,
                                     "required column \"" + std::string(column.name) +
                                         "\" is missing",
                                     at(lineNumber));
                }
            }
            continue;
        }
        if (fields->size() != table.width) {
            return makeError(ErrorCode::ParseFailure,
                             std::to_string(fields->size()) + " fields where the header has " +
                                 std::to_string(table.width),
                             at(lineNumber));
        }
        table.rows.push_back({lineNumber, std::move(*fields)});
    }
    if (!haveHeader) {
        return makeError(ErrorCode::ParseFailure, "no header row");
    }
    return table;
}

core::Error bad(const Row& row, std::string_view column, std::string_view value,
                std::string_view what)
{
    return makeError(ErrorCode::ParseFailure,
                     std::string(column) + " \"" + std::string(value) + "\" is not " +
                         std::string(what),
                     at(row.lineNumber));
}

// Parses a present cell with `parse`; an empty cell leaves `out` as it was.
template <typename T, typename Parse>
core::Status optionalCell(const Table& table, const Row& row, std::string_view column,
                          std::string_view what, Parse parse, std::optional<T>& out)
{
    const std::string_view text = table.cell(row, column);
    if (text.empty()) {
        return {};
    }
    const std::optional<T> value = parse(text);
    if (!value) {
        return bad(row, column, text, what);
    }
    out = *value;
    return {};
}

const auto kNumber = [](std::string_view text) { return core::parseFiniteDouble(text); };

std::optional<PathEvidence> parsePathEvidence(std::string_view text)
{
    const std::string name = key(text);
    if (name == "detected" || name == "traced") {
        return PathEvidence::Detected;
    }
    if (name == "exposed" || name == "trench") {
        return PathEvidence::Exposed;
    }
    if (name == "assumed" || name == "inferred") {
        return PathEvidence::Assumed;
    }
    return std::nullopt;
}

// One line attribute from one row: set it if unset, refuse if it disagrees.
struct AttributeSource {
    std::string value;
    std::size_t lineNumber = 0;
};

} // namespace

const std::vector<UtilityCsvColumn>& utilityCsvColumns()
{
    static const std::vector<UtilityCsvColumn> kColumns{
        {"line", {"line_id", "utility", "utility_id", "service", "asset"}, true},
        {"point", {"point_id", "id", "vertex", "pt"}, true},
        {"easting", {"e", "mga_e"}, true},
        {"northing", {"n", "mga_n"}, true},
        {"method", {"location_method", "locating_method", "survey_method"}, true},
        {"level", {"rl", "z", "utility_level", "ahd"}, false},
        {"level_ref", {"level_reference", "rl_ref", "level_on"}, false},
        {"surface", {"surface_level", "surface_rl", "ground", "nsl"}, false},
        {"h_unc", {"horizontal_uncertainty", "h_accuracy", "hz_unc"}, false},
        {"v_unc", {"vertical_uncertainty", "v_accuracy", "vt_unc"}, false},
        {"ql", {"quality_level", "qualitylevel"}, false},
        {"path", {"path_evidence", "path_to_next"}, false},
        {"verifies", {"checks", "verifies_point"}, false},
        {"type", {"utility_type", "service_type"}, false},
        {"owner", {"asset_owner", "authority", "operator"}, false},
        {"material", {}, false},
        {"diameter_mm", {"size_mm", "dia_mm", "od_mm"}, false},
        {"status", {"utility_status"}, false},
        {"config", {"configuration"}, false},
        {"description", {"desc", "comment", "remarks"}, false},
    };
    return kColumns;
}

core::Result<std::vector<UtilityLine>> parseUtilityCsv(std::string_view text)
{
    auto table = readTable(text, utilityCsvColumns());
    if (!table) {
        return table.error();
    }

    std::vector<UtilityLine> lines;
    std::map<std::string, std::size_t> lineIndex;
    // Per line: attribute column -> the first row that gave it.
    std::map<std::string, std::map<std::string, AttributeSource>> attributeSources;
    // Per line: the text row each vertex came from, for path evidence.
    std::map<std::string, std::vector<std::string>> pathCells;

    for (const Row& row : table->rows) {
        const std::string lineId(table->cell(row, "line"));
        const std::string pointId(table->cell(row, "point"));
        if (lineId.empty() || pointId.empty()) {
            return makeError(ErrorCode::ParseFailure, "line and point must not be empty",
                             at(row.lineNumber));
        }
        auto [found, inserted] = lineIndex.emplace(lineId, lines.size());
        if (inserted) {
            UtilityLine line;
            line.id = lineId;
            lines.push_back(std::move(line));
        }
        UtilityLine& line = lines[found->second];
        for (const UtilityVertex& existing : line.vertices) {
            if (existing.id == pointId) {
                return makeError(ErrorCode::ParseFailure,
                                 "point " + pointId + " appears twice on line " + lineId,
                                 at(row.lineNumber));
            }
        }

        UtilityVertex vertex;
        vertex.id = pointId;
        std::optional<double> northing;
        std::optional<double> easting;
        std::optional<LocationMethod> method;
        for (const core::Status& status : {
                 optionalCell(*table, row, "northing", "a number", kNumber, northing),
                 optionalCell(*table, row, "easting", "a number", kNumber, easting),
                 optionalCell(*table, row, "method", "a location method", parseLocationMethod,
                              method),
                 optionalCell(*table, row, "level", "a number", kNumber, vertex.level),
                 optionalCell(*table, row, "surface", "a number", kNumber, vertex.surfaceLevel),
                 optionalCell(*table, row, "h_unc", "a number", kNumber,
                              vertex.evidence.horizontalUncertainty),
                 optionalCell(*table, row, "v_unc", "a number", kNumber,
                              vertex.evidence.verticalUncertainty),
                 optionalCell(*table, row, "ql", "a quality level", parseQualityLevel,
                              vertex.claimed),
             }) {
            if (!status) {
                return status.error();
            }
        }
        if (!northing || !easting || !method) {
            return makeError(ErrorCode::ParseFailure,
                             "easting, northing and method must be given for every point",
                             at(row.lineNumber));
        }
        vertex.position = {*northing, *easting};
        vertex.evidence.method = *method;
        vertex.evidence.hasLevel = vertex.level.has_value();
        std::optional<LevelReference> reference;
        if (auto status = optionalCell(*table, row, "level_ref", "a level reference",
                                       parseLevelReference, reference);
            !status) {
            return status.error();
        }
        vertex.levelReference = reference.value_or(LevelReference::Top);
        vertex.verifies = std::string(table->cell(row, "verifies"));

        const std::string_view path = table->cell(row, "path");
        if (!path.empty() && !parsePathEvidence(path)) {
            return bad(row, "path", path, "detected, exposed or assumed");
        }
        pathCells[lineId].emplace_back(path);

        // Line attributes: the first row to give one sets it; a later row
        // giving a different one is a contradiction in the schedule.
        auto& sources = attributeSources[lineId];
        for (const std::string_view column :
             {"type", "owner", "material", "diameter_mm", "status", "config", "description"}) {
            const std::string_view value = table->cell(row, column);
            if (value.empty()) {
                continue;
            }
            auto [source, fresh] = sources.emplace(
                std::string(column), AttributeSource{std::string(value), row.lineNumber});
            if (!fresh && source->second.value != value) {
                return makeError(ErrorCode::ParseFailure,
                                 "line " + lineId + " has " + std::string(column) + " \"" +
                                     source->second.value + "\" at " +
                                     at(source->second.lineNumber) + " and \"" +
                                     std::string(value) + "\" here",
                                 at(row.lineNumber));
            }
            if (!fresh) {
                continue;
            }
            UtilityAttributes& attributes = line.attributes;
            if (column == "type") {
                const auto type = parseUtilityType(value);
                if (!type) {
                    return bad(row, column, value, "a utility type");
                }
                attributes.type = *type;
            } else if (column == "status") {
                const auto status = parseUtilityStatus(value);
                if (!status) {
                    return bad(row, column, value, "a utility status");
                }
                attributes.status = *status;
            } else if (column == "diameter_mm") {
                const auto millimetres = core::parseFiniteDouble(value);
                if (!millimetres || *millimetres <= 0.0) {
                    return bad(row, column, value, "a positive number of millimetres");
                }
                attributes.diameter = *millimetres / 1000.0;
            } else if (column == "owner") {
                attributes.owner = std::string(value);
            } else if (column == "material") {
                attributes.material = std::string(value);
            } else if (column == "config") {
                attributes.configuration = std::string(value);
            } else {
                attributes.description = std::string(value);
            }
        }
        line.vertices.push_back(std::move(vertex));
    }

    for (UtilityLine& line : lines) {
        const std::vector<std::string>& cells = pathCells[line.id];
        bool any = false;
        for (std::size_t i = 0; i + 1 < cells.size(); ++i) {
            any = any || !cells[i].empty();
        }
        if (!any) {
            continue;
        }
        for (std::size_t i = 0; i + 1 < cells.size(); ++i) {
            line.pathEvidence.push_back(cells[i].empty() ? PathEvidence::Detected
                                                         : *parsePathEvidence(cells[i]));
        }
    }
    return lines;
}

core::Result<DesignAlignment> parseDesignCsv(std::string_view text, std::string id)
{
    std::vector<UtilityCsvColumn> columns;
    for (const UtilityCsvColumn& column : utilityCsvColumns()) {
        if (column.name == "easting" || column.name == "northing") {
            columns.push_back(column);
        } else if (column.name == "level" || column.name == "point") {
            UtilityCsvColumn optional = column;
            optional.required = false;
            columns.push_back(std::move(optional));
        }
    }
    auto table = readTable(text, columns);
    if (!table) {
        return table.error();
    }
    DesignAlignment design;
    design.id = std::move(id);
    for (const Row& row : table->rows) {
        std::optional<double> northing;
        std::optional<double> easting;
        DesignVertex vertex;
        for (const core::Status& status :
             {optionalCell(*table, row, "northing", "a number", kNumber, northing),
              optionalCell(*table, row, "easting", "a number", kNumber, easting),
              optionalCell(*table, row, "level", "a number", kNumber, vertex.level)}) {
            if (!status) {
                return status.error();
            }
        }
        if (!northing || !easting) {
            return makeError(ErrorCode::ParseFailure, "easting and northing must be given",
                             at(row.lineNumber));
        }
        vertex.position = {*northing, *easting};
        design.vertices.push_back(vertex);
    }
    return design;
}

} // namespace katana::survey::subsurface
