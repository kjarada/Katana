#include "katana/survey/subsurface/utility_csv.hpp"

#include <algorithm>
#include <cmath>
#include <limits>
#include <map>
#include <optional>
#include <set>
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

// ---- writing a schedule --------------------------------------------------------------------

namespace {

// The reader's own spelling of each word value: what toString says, except
// an unknown method, whose "unknown method" the reader would not take back.
std::string ownSpelling(LocationMethod method)
{
    return method == LocationMethod::Unknown ? std::string("unknown") : toString(method);
}

// A cell as the reader splits it back: quoted when it holds a comma or a
// quote, starts or ends with a blank (the reader trims an unquoted cell), or
// starts with '#' (a row whose first cell does is a comment).
std::string csvCell(std::string_view value)
{
    const bool quote = value.find_first_of(",\"") != std::string_view::npos ||
                       (!value.empty() && (core::isAsciiSpace(value.front()) ||
                                           core::isAsciiSpace(value.back()) ||
                                           value.front() == '#'));
    if (!quote) {
        return std::string(value);
    }
    std::string out = "\"";
    for (const char c : value) {
        out += c;
        if (c == '"') {
            out += '"';
        }
    }
    return out + "\"";
}

// Millimetre text that the reader's division by 1000 brings back to exactly
// `metres`. The product is tried first, then its neighbours a few units in
// the last place either side: a diameter read from a schedule came from such
// a text, so one of them divides back; four is ample, since the product and
// the quotient are each within half a unit of exact.
std::string millimetres(double metres)
{
    const double product = metres * 1000.0;
    double below = product;
    double above = product;
    for (int step = 0; step < 4; ++step) {
        for (const double candidate : {below, above}) {
            std::string text = core::formatExactReal(candidate);
            if (const auto back = core::parseFiniteDouble(text); back && *back / 1000.0 == metres) {
                return text;
            }
        }
        below = std::nextafter(below, -std::numeric_limits<double>::infinity());
        above = std::nextafter(above, std::numeric_limits<double>::infinity());
    }
    return core::formatExactReal(product);
}

const UtilityCsvColumn* columnNamed(std::string_view name)
{
    for (const UtilityCsvColumn& column : utilityCsvColumns()) {
        if (column.name == name) {
            return &column;
        }
    }
    return nullptr;
}

core::Error unwritable(const UtilityLine& line, const UtilityVertex* vertex, std::string what)
{
    return makeError(ErrorCode::InvalidArgument,
                     "line " + line.id + (vertex ? " point " + vertex->id : std::string()) + ": " +
                         std::move(what) + "; a schedule cannot hold it");
}

} // namespace

core::Result<std::string> writeUtilityCsv(const std::vector<UtilityLine>& lines,
                                          const UtilityCsvDialect& dialect)
{
    const auto spelled = [&dialect](std::string_view column, std::string own) {
        if (const auto found = dialect.spellings.find(column); found != dialect.spellings.end()) {
            if (const auto word = found->second.find(own); word != found->second.end()) {
                return word->second;
            }
        }
        return own;
    };
    const auto real = [](double value) { return core::formatExactReal(value); };

    // Each row as column -> text; a column absent from every row is not
    // written unless the reader requires it.
    std::vector<std::map<std::string, std::string, std::less<>>> rows;
    std::set<std::string, std::less<>> used;
    for (const UtilityLine& line : lines) {
        if (line.id.empty()) {
            return makeError(ErrorCode::InvalidArgument, "a line with no id; a schedule cannot hold it");
        }
        const std::size_t count = line.vertices.size();
        if (!line.pathEvidence.empty() && line.pathEvidence.size() + 1 != count) {
            return unwritable(line, nullptr,
                              std::to_string(line.pathEvidence.size()) +
                                  " path evidences for " + std::to_string(count) + " vertices");
        }
        const UtilityAttributes& attributes = line.attributes;
        if (!std::isfinite(attributes.diameter) || attributes.diameter < 0.0) {
            return unwritable(line, nullptr, "a diameter that is not a positive number");
        }
        std::set<std::string, std::less<>> pointIds;
        for (std::size_t i = 0; i < count; ++i) {
            const UtilityVertex& vertex = line.vertices[i];
            std::map<std::string, std::string, std::less<>> row;
            const auto put = [&row](std::string_view column, std::string value) {
                if (!value.empty()) {
                    row.insert_or_assign(std::string(column), std::move(value));
                }
            };
            if (vertex.id.empty() || !pointIds.insert(vertex.id).second) {
                return unwritable(line, &vertex,
                                  vertex.id.empty() ? "a point with no id" : "a point id twice");
            }
            for (const auto value : {vertex.position.easting, vertex.position.northing,
                                     vertex.level.value_or(0.0), vertex.depth.value_or(0.0),
                                     vertex.surfaceLevel.value_or(0.0),
                                     vertex.evidence.horizontalUncertainty.value_or(0.0),
                                     vertex.evidence.verticalUncertainty.value_or(0.0)}) {
                if (!std::isfinite(value)) {
                    return unwritable(line, &vertex, "a number that is not finite");
                }
            }
            if (vertex.depth && *vertex.depth < 0.0) {
                return unwritable(line, &vertex, "a depth above the surface");
            }
            put("line", line.id);
            put("point", vertex.id);
            put("easting", real(vertex.position.easting));
            put("northing", real(vertex.position.northing));
            put("method", spelled("method", ownSpelling(vertex.evidence.method)));
            if (vertex.level) {
                put("level", real(*vertex.level));
            }
            // Written wherever it bears on something, and wherever it is not
            // what an empty cell reads as.
            if (vertex.level || vertex.depth || vertex.levelReference != LevelReference::Top) {
                put("level_ref", spelled("level_ref", toString(vertex.levelReference)));
            }
            if (vertex.depth) {
                put("depth", real(*vertex.depth));
            }
            if (vertex.surfaceLevel) {
                put("surface", real(*vertex.surfaceLevel));
            }
            if (vertex.evidence.horizontalUncertainty) {
                put("h_unc", real(*vertex.evidence.horizontalUncertainty));
            }
            if (vertex.evidence.verticalUncertainty) {
                put("v_unc", real(*vertex.evidence.verticalUncertainty));
            }
            if (vertex.claimed) {
                put("ql", spelled("ql", toString(*vertex.claimed)));
            }
            if (!line.pathEvidence.empty() && i + 1 < count) {
                put("path", spelled("path", toString(line.pathEvidence[i])));
            }
            put("verifies", vertex.verifies);
            if (attributes.type != UtilityType::Unknown) {
                put("type", spelled("type", toString(attributes.type)));
            }
            put("owner", attributes.owner);
            put("material", attributes.material);
            if (attributes.diameter > 0.0) {
                put(attributes.diameterIsInside ? "size" : "diameter_mm",
                    millimetres(attributes.diameter));
            }
            if (attributes.status != UtilityStatus::Unknown) {
                put("status", spelled("status", toString(attributes.status)));
            }
            put("config", attributes.configuration);
            put("description", attributes.description);
            for (const auto& [fields, carried] :
                 {std::pair{&attributes.fields, CarriedOn::Line},
                  std::pair{&vertex.fields, CarriedOn::Vertex}}) {
                for (const auto& [name, value] : *fields) {
                    const UtilityCsvColumn* column = columnNamed(name);
                    if (column == nullptr || column->carried != carried) {
                        return unwritable(line, &vertex,
                                          "kept attribute " + name +
                                              (column == nullptr
                                                   ? " is no column the schedule has"
                                                   : " is carried by the other of a service "
                                                     "and a point"));
                    }
                    put(name, value);
                }
            }
            for (const auto& [column, value] : row) {
                if (value.find_first_of("\r\n") != std::string::npos) {
                    return unwritable(line, &vertex, column + " holds a line break");
                }
                used.insert(column);
            }
            rows.push_back(std::move(row));
        }
    }

    std::vector<std::string_view> columns;
    for (const UtilityCsvColumn& column : utilityCsvColumns()) {
        if (column.required || used.contains(column.name)) {
            columns.push_back(column.name);
        }
    }
    std::string text;
    for (std::size_t c = 0; c < columns.size(); ++c) {
        const auto header = dialect.headers.find(columns[c]);
        text += (c == 0 ? "" : ",") +
                csvCell(header == dialect.headers.end() ? columns[c] : header->second);
    }
    text += '\n';
    for (const auto& row : rows) {
        for (std::size_t c = 0; c < columns.size(); ++c) {
            const auto value = row.find(columns[c]);
            text += (c == 0 ? "" : ",") + (value == row.end() ? std::string() : csvCell(value->second));
        }
        text += '\n';
    }
    return text;
}

UtilityCsvDialect utilityCsvDialect(const DeliverySchema& schema)
{
    UtilityCsvDialect dialect;
    std::set<std::string, std::less<>> taken;
    for (const UtilityCsvColumn& column : utilityCsvColumns()) {
        // A kept column is written by its own name, which is the schema's.
        if (column.carried != CarriedOn::Interpreted) {
            continue;
        }
        std::set<std::string, std::less<>> names{key(column.name)};
        for (const std::string_view alias : column.aliases) {
            names.insert(key(alias));
        }
        const SchemaField* field = nullptr;
        std::string header;
        for (const SchemaField& candidate : schema.fields) {
            // The attribute when it is one of the reader's names for the
            // column, else the label: the reader must take the header back,
            // and the check finds a column by either.
            if (names.contains(key(candidate.attribute))) {
                header = candidate.attribute;
            } else if (!candidate.label.empty() && names.contains(key(candidate.label))) {
                header = candidate.label;
            } else {
                continue;
            }
            if (taken.insert(header).second) {
                field = &candidate;
                break;
            }
        }
        if (field == nullptr) {
            continue;
        }
        dialect.headers.emplace(std::string(column.name), header);

        const auto domain = schema.domains.find(field->attribute);
        if (domain == schema.domains.end()) {
            continue;
        }
        // The first listed spelling of each value, in the reader's reading.
        const auto spellingOf = [&](const std::string& listed) -> std::optional<std::string> {
            const std::string_view name = column.name;
            if (name == "type") {
                const auto type = parseUtilityType(listed);
                return type ? std::optional<std::string>(toString(*type)) : std::nullopt;
            }
            if (name == "status") {
                const auto status = parseUtilityStatus(listed);
                return status ? std::optional<std::string>(toString(*status)) : std::nullopt;
            }
            if (name == "method") {
                const auto method = parseLocationMethod(listed);
                return method ? std::optional<std::string>(ownSpelling(*method)) : std::nullopt;
            }
            if (name == "level_ref") {
                const auto reference = parseLevelReference(listed);
                return reference ? std::optional<std::string>(toString(*reference)) : std::nullopt;
            }
            if (name == "ql") {
                const auto level = parseQualityLevel(listed);
                return level ? std::optional<std::string>(toString(*level)) : std::nullopt;
            }
            if (name == "path") {
                const auto path = parsePathEvidence(listed);
                return path ? std::optional<std::string>(toString(*path)) : std::nullopt;
            }
            return std::nullopt;
        };
        for (const SchemaValue& listed : domain->second.values) {
            if (const auto own = spellingOf(listed.value)) {
                dialect.spellings[std::string(column.name)].emplace(*own, listed.value);
            }
        }
    }
    return dialect;
}

} // namespace katana::survey::subsurface
