#include "katana/survey/subsurface/utility_csv.hpp"

#include <algorithm>
#include <cmath>
#include <map>
#include <optional>
#include <utility>

#include "katana/core/text.hpp"
#include "subsurface_table.hpp"

namespace katana::survey::subsurface {

namespace {

using core::ErrorCode;
using core::makeError;

using detail::at;
using detail::key;
using Row = detail::RawRow;

struct Table {
    std::map<std::string, std::size_t> columns; // canonical name -> field index
    std::vector<Row> rows;

    // The trimmed cell, or empty when the column is absent.
    [[nodiscard]] std::string_view cell(const Row& row, std::string_view name) const
    {
        const auto found = columns.find(std::string(name));
        return found == columns.end() ? std::string_view{}
                                      : std::string_view(row.fields[found->second]);
    }
};

// The rows of `text`, with its header's columns resolved against `known`.
core::Result<Table> readTable(std::string_view text, const std::vector<UtilityCsvColumn>& known)
{
    auto raw = detail::readRawTable(text);
    if (!raw) {
        return raw.error();
    }
    Table table;
    const std::string where = at(raw->headerLine);
    for (std::size_t column = 0; column < raw->header.size(); ++column) {
        const std::string name = key(raw->header[column]);
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
                             "unknown column \"" + raw->header[column] + "\"", where);
        }
        if (!table.columns.emplace(std::string(match->name), column).second) {
            return makeError(ErrorCode::ParseFailure,
                             "column \"" + std::string(match->name) + "\" given twice", where);
        }
    }
    for (const UtilityCsvColumn& column : known) {
        if (column.required && !table.columns.contains(std::string(column.name))) {
            return makeError(ErrorCode::ParseFailure,
                             "required column \"" + std::string(column.name) + "\" is missing",
                             where);
        }
    }
    table.rows = std::move(raw->rows);
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

// A quality level a schedule claims; "Unknown" parses (as no claim, which the
// caller makes of it) so that it is not refused as a typo.
std::optional<QualityLevel> parseClaim(std::string_view text)
{
    if (key(text) == "unknown") {
        return QualityLevel::D; // replaced by "no claim" by the caller
    }
    return parseQualityLevel(text);
}

// A size in millimetres, as metres: "150", or "1200 x 900" (a culvert's or a
// duct bank's width by height), whose larger side is taken - the choice that
// places a top from an invert higher and a service wider, both of which err
// towards less cover and less clearance. 0 for "Not Applicable" and
// "Unknown", which record that there is none to give.
std::optional<double> parseSize(std::string_view text)
{
    const std::string name = key(text);
    if (name == "notapplicable" || name == "unknown") {
        return 0.0;
    }
    double largest = 0.0;
    std::size_t start = 0;
    const std::string lower = core::lowered(text);
    while (true) {
        const std::size_t cross = lower.find('x', start);
        const auto part = core::parseFiniteDouble(core::trimmed(std::string_view(lower).substr(
            start, cross == std::string::npos ? std::string::npos : cross - start)));
        if (!part || *part <= 0.0) {
            return std::nullopt;
        }
        largest = std::max(largest, *part);
        if (cross == std::string::npos) {
            return largest / 1000.0;
        }
        start = cross + 1;
    }
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
        {"line", {"line_id", "utility", "utility_id", "service", "asset", "AssetIdentifier"}, true},
        {"point", {"point_id", "id", "vertex", "pt"}, true},
        {"easting", {"e", "mga_e"}, true},
        {"northing", {"n", "mga_n"}, true},
        {"method", {"location_method", "locating_method", "survey_method", "LocateMethod"}, true},
        {"level", {"rl", "z", "utility_level", "ahd"}, false},
        {"level_ref", {"level_reference", "rl_ref", "level_on", "DepthLocation"}, false},
        {"depth", {"depth_m"}, false},
        {"surface", {"surface_level", "surface_rl", "ground", "nsl"}, false},
        {"h_unc", {"horizontal_uncertainty", "h_accuracy", "hz_unc"}, false},
        {"v_unc", {"vertical_uncertainty", "v_accuracy", "vt_unc"}, false},
        {"ql", {"quality_level", "QualityLevel"}, false},
        {"path", {"path_evidence", "path_to_next"}, false},
        {"verifies", {"checks", "verifies_point"}, false},
        {"type", {"utility_type", "service_type", "AssetTypeCode"}, false},
        {"owner", {"AssetOwner", "authority", "operator"}, false},
        {"material", {}, false},
        {"diameter_mm", {"dia_mm", "od_mm"}, false},
        {"size", {"size_mm"}, false},
        {"status", {"utility_status", "AssetStatus"}, false},
        {"config", {"configuration"}, false},
        {"description", {"desc", "comment", "remarks"}, false},
        // The rest of the AS 5488 attribute set as the TfNSW Utility Schema
        // names it: read, kept by that name, never interpreted here. Per
        // service, so that two rows of one service may not disagree ...
        {"AssetType", {}, false, CarriedOn::Line},
        {"AssetSubtypeCode", {}, false, CarriedOn::Line},
        {"AssetSubtype", {}, false, CarriedOn::Line},
        {"AssetSubtypeDescription", {}, false, CarriedOn::Line},
        {"AssetFeature", {}, false, CarriedOn::Line},
        {"AssetFeatureDescription", {}, false, CarriedOn::Line},
        {"Capacity", {}, false, CarriedOn::Line},
        {"SizeDescription", {}, false, CarriedOn::Line},
        {"ConfigurationDescription", {}, false, CarriedOn::Line},
        {"MaterialDescription", {}, false, CarriedOn::Line},
        {"Limitation", {}, false, CarriedOn::Line},
        {"Condition", {}, false, CarriedOn::Line},
        {"UtilityInstallDate", {}, false, CarriedOn::Line},
        {"SourceofInformation", {}, false, CarriedOn::Line},
        {"Location", {}, false, CarriedOn::Line},
        {"Clash", {}, false, CarriedOn::Line},
        {"ClashDescription", {}, false, CarriedOn::Line},
        {"ClashID", {}, false, CarriedOn::Line},
        {"ClashRisk", {}, false, CarriedOn::Line},
        {"Treatment", {}, false, CarriedOn::Line},
        {"TreatmentDescription", {}, false, CarriedOn::Line},
        {"TreatmentReference", {}, false, CarriedOn::Line},
        {"TreatmentLength", {}, false, CarriedOn::Line},
        {"TreatmentRisk", {}, false, CarriedOn::Line},
        {"tbCoordSys", {}, false, CarriedOn::Line},
        {"TfNSW_ContractOrgCode", {}, false, CarriedOn::Line},
        {"TfNSW_ContractOrgName", {}, false, CarriedOn::Line},
        // ... and per vertex, where each point may say something of its own.
        {"DepthDescription", {}, false, CarriedOn::Vertex},
        {"DateInfoObtained", {}, false, CarriedOn::Vertex},
        {"PotholeReport", {}, false, CarriedOn::Vertex},
        {"PitReport", {}, false, CarriedOn::Vertex},
        {"Notes", {}, false, CarriedOn::Vertex},
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
                 optionalCell(*table, row, "depth", "a number of metres", kNumber, vertex.depth),
                 optionalCell(*table, row, "surface", "a number", kNumber, vertex.surfaceLevel),
                 optionalCell(*table, row, "h_unc", "a number", kNumber,
                              vertex.evidence.horizontalUncertainty),
                 optionalCell(*table, row, "v_unc", "a number", kNumber,
                              vertex.evidence.verticalUncertainty),
                 optionalCell(*table, row, "ql", "a quality level", parseClaim, vertex.claimed),
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
        vertex.evidence.hasLevel = hasVerticalMeasurement(vertex);
        if (vertex.depth && *vertex.depth < 0.0) {
            return bad(row, "depth", table->cell(row, "depth"), "a depth below the surface");
        }
        if (auto claim = table->cell(row, "ql"); key(claim) == "unknown") {
            vertex.claimed.reset(); // a schedule's "Unknown" claims nothing
        }
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
        std::vector<std::string_view> lineColumns{"type", "owner",  "material", "diameter_mm",
                                                  "size", "status", "config",   "description"};
        for (const UtilityCsvColumn& carried : utilityCsvColumns()) {
            if (carried.carried == CarriedOn::Line) {
                lineColumns.push_back(carried.name);
            } else if (carried.carried == CarriedOn::Vertex) {
                if (const std::string_view value = table->cell(row, carried.name); !value.empty()) {
                    vertex.fields.emplace(std::string(carried.name), std::string(value));
                }
            }
        }
        for (const std::string_view column : lineColumns) {
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
                attributes.diameterIsInside = false;
            } else if (column == "size") {
                const auto size = parseSize(value);
                if (!size) {
                    return bad(row, column, value,
                               "millimetres, W x H millimetres, Not Applicable or Unknown");
                }
                // An outside diameter, where both are given, is the better
                // one to find a top from.
                if (*size > 0.0 && !sources.contains("diameter_mm")) {
                    attributes.diameter = *size;
                    attributes.diameterIsInside = true;
                }
            } else if (column == "owner") {
                attributes.owner = std::string(value);
            } else if (column == "material") {
                attributes.material = std::string(value);
            } else if (column == "config") {
                attributes.configuration = std::string(value);
            } else if (column == "description") {
                attributes.description = std::string(value);
            } else {
                attributes.fields.emplace(std::string(column), std::string(value));
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
