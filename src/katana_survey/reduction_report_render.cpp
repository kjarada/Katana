// renderText and renderHtml: the reduction report for people.
//
// One walk over the report feeds both: each section is built as a table of
// cells (header, rows), then written as fixed-width text or as an HTML table.
// The text pads by displayed characters, not bytes, so a degree sign or an
// accented point name does not push its column out of line.

#include <algorithm>
#include <cmath>
#include <charconv>
#include <string>
#include <string_view>
#include <vector>

#include "katana/math/numerics.hpp"
#include "katana/survey/reduction_report.hpp"
#include "katana/survey/reduction_settings.hpp"

namespace katana::survey {

namespace {

using katana::math::kPi;

// std::to_chars rather than snprintf: a report of 100 000 observations
// formats about a million numbers, and the C library's printf was most of
// the time it took.
std::string fixed(double value, int decimals)
{
    char buffer[64];
    const auto result =
        std::to_chars(buffer, buffer + sizeof buffer, value, std::chars_format::fixed, decimals);
    return std::string(buffer, result.ptr);
}

// "+0.0" for anything that rounds to zero, whatever its sign: a residual of
// -1e-15 is not a negative millimetre.
std::string signedFixed(double value, int decimals)
{
    const std::string digits = fixed(std::abs(value), decimals);
    const bool zero = digits.find_first_not_of("0.") == std::string::npos;
    return (value < 0.0 && !zero ? "-" : "+") + digits;
}

// 123°45'06.7" (seconds to `decimals`), rounding carried into the minutes.
std::string dms(double radians, int decimals = 1)
{
    if (!std::isfinite(radians)) {
        return "-";
    }
    const bool negative = radians < 0.0;
    const double scale = std::pow(10.0, decimals);
    const double totalTenths = std::round(std::abs(radians) * 648000.0 / kPi * scale);
    const long long whole = static_cast<long long>(totalTenths);
    const long long unitsPerMinute = static_cast<long long>(60 * scale);
    const long long unitsPerDegree = unitsPerMinute * 60;
    const long long degrees = whole / unitsPerDegree;
    const long long minutes = (whole % unitsPerDegree) / unitsPerMinute;
    const double seconds = static_cast<double>(whole % unitsPerMinute) / scale;
    std::string text = negative ? "-" : "";
    text += std::to_string(degrees);
    text += "\xC2\xB0";
    text += minutes < 10 ? "0" : "";
    text += std::to_string(minutes);
    text += '\'';
    text += seconds < 10.0 ? "0" : "";
    text += fixed(seconds, decimals);
    text += '"';
    return text;
}

std::string seconds(double radians, int decimals = 1)
{
    return signedFixed(radians * 648000.0 / kPi, decimals) + "\"";
}

std::string millimetres(double metres, int decimals = 1)
{
    return signedFixed(metres * 1000.0, decimals) + " mm";
}

std::string metres(double value, int decimals = 3)
{
    return fixed(value, decimals);
}

std::string optionalMetres(const std::optional<double>& value, int decimals = 3)
{
    return value ? metres(*value, decimals) : std::string("-");
}

std::string optionalMillimetres(const std::optional<double>& value)
{
    return value ? millimetres(*value) : std::string("-");
}

std::string yesNo(bool value)
{
    return value ? "on" : "off";
}

std::string sourceText(const SourceRecord& source)
{
    if (!source.known()) {
        return {};
    }
    std::string text = source.fileName;
    if (source.recordNumber != 0) {
        text += (text.empty() ? "record " : ":") + std::to_string(source.recordNumber);
    }
    return text;
}

std::string correctionText(const AppliedCorrection& correction, bool angular)
{
    std::string text = toString(correction.kind);
    text += ' ';
    text += angular ? seconds(correction.amount) : millimetres(correction.amount);
    if (correction.factor) {
        text += " (x" + fixed(*correction.factor, 8) + ")";
    }
    if (!correction.note.empty()) {
        text += " [" + correction.note + "]";
    }
    return text;
}

std::string valueText(double value, bool angular)
{
    return angular ? dms(value) : metres(value, 4);
}

// ---- Sections as tables -----------------------------------------------------------------

struct Table {
    std::string title;
    std::vector<std::string> header;
    std::vector<std::vector<std::string>> rows;
    // Key/value sections are rendered without a header line.
    bool keyValue = false;
    std::string empty = "none";
};

Table inputTable(const ReductionReport& report)
{
    const ReportInput& input = report.input;
    Table table{"Input", {"", ""}, {}, true};
    table.rows.push_back({"File", input.fileName});
    std::string format = input.formatName;
    if (!input.formatId.empty()) {
        format += " (" + input.formatId + ")";
    }
    if (!input.parserVersion.empty()) {
        format += ", parser " + input.parserVersion;
    }
    table.rows.push_back({"Format", format});
    table.rows.push_back({"Records", std::to_string(input.recordsRead) + " read, " +
                                         std::to_string(input.recordsSkipped) + " skipped"});
    for (const std::string& sibling : input.siblingFiles) {
        table.rows.push_back({"Also read", sibling});
    }
    for (const std::string& missing : input.notCarried) {
        table.rows.push_back({"Not in the file", missing});
    }
    for (const ReportMessage& warning : input.warnings) {
        const std::string where = sourceText(warning.source);
        table.rows.push_back({"Reader warning", (where.empty() ? "" : where + ": ") + warning.text});
    }
    table.rows.push_back({"Created", report.createdUtc});
    return table;
}

std::string constraintText(const ControlComponent& component)
{
    switch (component.constraint) {
    case ControlConstraint::Free:
        return "free";
    case ControlConstraint::Fixed:
        return "fixed";
    case ControlConstraint::Weighted:
        return "weighted " + fixed(component.sigma * 1000.0, 1) + " mm";
    }
    return "free";
}

Table settingsTable(const ReductionSettings& s)
{
    Table table{"Settings used", {"", ""}, {}, true};
    std::string atmospheric = toString(s.atmospheric);
    if (s.atmospheric == AtmosphericCorrection::Fixed) {
        atmospheric += " " + signedFixed(s.fixedPpm, 1) + " ppm";
    }
    table.rows.push_back({"Atmospheric correction", atmospheric});
    std::string prism = toString(s.prismConstantPolicy);
    if (s.prismConstantPolicy == PrismConstantPolicy::Override) {
        prism += " " + millimetres(s.prismConstant);
    }
    table.rows.push_back({"Prism constant", prism});
    table.rows.push_back(
        {"Faces", std::string(toString(s.faces)) + "; tolerances " +
                      fixed(s.faceTolerances.horizontal * 648000.0 / kPi, 1) + "\" horizontal, " +
                      fixed(s.faceTolerances.zenith * 648000.0 / kPi, 1) + "\" zenith, " +
                      fixed(s.faceTolerances.distance * 1000.0, 1) + " mm; outside: " +
                      (s.faceTolerances.excludeOutside ? "excluded" : "used and flagged")});
    table.rows.push_back({"Slope to horizontal", yesNo(s.slopeToHorizontal)});
    table.rows.push_back({"Curvature and refraction",
                          yesNo(s.curvatureAndRefraction) + ", k = " +
                              fixed(s.refractionCoefficient, 3) + ", R = " +
                              fixed(s.earthRadius, 0) + " m"});
    if (s.useCombinedFactor) {
        table.rows.push_back({"Combined scale factor", fixed(s.combinedFactor, 8) +
                                                           " (in place of height and grid)"});
    } else {
        table.rows.push_back({"Height reduction", toString(s.heightReduction)});
        std::string grid = toString(s.gridScale);
        if (s.gridScale == GridScale::Fixed) {
            grid += " " + fixed(s.fixedGridScaleFactor, 8);
        }
        table.rows.push_back({"Grid scale", grid});
    }
    std::string method = toString(s.method);
    if (s.method == AdjustmentMethod::None) {
        method += " (radiation)";
    } else if (s.method == AdjustmentMethod::Traverse) {
        method += std::string(", ") + toString(s.traverseRule);
    } else {
        method += std::string(", ") + toString(s.networkDimension);
    }
    table.rows.push_back({"Adjustment", method});
    const ObservationPrecision& a = s.apriori;
    table.rows.push_back(
        {"A-priori precision",
         "direction " + fixed(a.direction * 648000.0 / kPi, 1) + "\", zenith " +
             fixed(a.zenith * 648000.0 / kPi, 1) + "\", distance " +
             fixed(a.distanceConstant * 1000.0, 1) + " mm + " + fixed(a.distancePpm, 1) +
             " ppm, centring " + fixed(a.instrumentCentring * 1000.0, 1) + " / " +
             fixed(a.targetCentring * 1000.0, 1) + " mm, heights " +
             fixed(a.heightMeasurement * 1000.0, 1) + " mm"});
    for (const ControlSelection& control : s.control) {
        table.rows.push_back({"Control", control.point.pointId + " (" + toString(control.origin) +
                                             "): N " + constraintText(control.point.northing) +
                                             ", E " + constraintText(control.point.easting) +
                                             ", H " + constraintText(control.point.elevation)});
    }
    if (s.control.empty()) {
        table.rows.push_back({"Control", "none chosen"});
    }
    table.rows.push_back({"Confidence level", fixed(s.confidenceLevel * 100.0, 1) + " %"});
    table.rows.push_back({"Outlier test", std::string(toString(s.outlierTest)) + " at " +
                                              fixed(s.outlierSignificance, 4) + ", auto-reject " +
                                              yesNo(s.autoRejectOutliers)});
    return table;
}

Table setupTable(const ReductionReport& report)
{
    Table table{"Setups",
                {"Setup", "Point", "HI", "Instrument", "Backsight", "BS azimuth", "BS reading",
                 "Orientation", "BS dist check", "BS height check", "Source"},
                {}};
    for (const ReportSetup& setup : report.setups) {
        std::string instrument = setup.instrument.make;
        if (!setup.instrument.model.empty()) {
            instrument += (instrument.empty() ? "" : " ") + setup.instrument.model;
        }
        table.rows.push_back(
            {setup.stationId, setup.pointId, metres(setup.instrumentHeight), instrument,
             setup.backsightPointId,
             setup.backsightAzimuth ? dms(*setup.backsightAzimuth) : "-",
             setup.backsightReading ? dms(*setup.backsightReading) : "-",
             setup.orientationCorrection ? dms(*setup.orientationCorrection) : "not oriented",
             optionalMillimetres(setup.backsightDistanceDifference),
             optionalMillimetres(setup.backsightHeightDifference), sourceText(setup.source)});
    }
    return table;
}

Table observationTable(const ReductionReport& report)
{
    Table table{"Observations: raw value, each correction, reduced value",
                {"Setup", "Kind", "From", "To", "Pointing", "Raw", "Corrections", "Reduced",
                 "Source"},
                {}};
    table.rows.reserve(report.observations.size());
    for (const ReportObservation& observation : report.observations) {
        std::string corrections;
        for (const AppliedCorrection& correction : observation.corrections) {
            corrections += corrections.empty() ? "" : "; ";
            corrections += correctionText(correction, observation.angular);
        }
        std::string pointing;
        if (observation.pointing.index != 0 || observation.pointing.face != Face::Unknown) {
            pointing = std::to_string(observation.pointing.index);
            if (observation.pointing.face != Face::Unknown) {
                pointing += observation.pointing.face == Face::Left ? " FL" : " FR";
            }
        }
        std::string reduced;
        if (observation.rejected) {
            reduced = "REJECTED: " + observation.rejectionReason;
        } else if (observation.reduced) {
            reduced = valueText(*observation.reduced, observation.angular);
        } else {
            reduced = "-";
        }
        table.rows.push_back({observation.stationId, observation.kind, observation.from,
                              observation.to, pointing,
                              valueText(observation.raw, observation.angular), corrections,
                              reduced, sourceText(observation.source)});
    }
    return table;
}

Table facePairTable(const ReductionReport& report)
{
    Table table{"Face left / face right pairs",
                {"Setup", "Target", "FL", "FR", "Horizontal", "Zenith (2 x index)", "Distance",
                 "Check"},
                {}};
    for (const FacePairCheck& pair : report.facePairs) {
        table.rows.push_back(
            {pair.stationId, pair.targetId, std::to_string(pair.leftPointing),
             std::to_string(pair.rightPointing),
             pair.horizontalSpread ? seconds(*pair.horizontalSpread) : "-",
             pair.zenithSpread ? seconds(*pair.zenithSpread) : "-",
             optionalMillimetres(pair.distanceSpread),
             pair.withinTolerance ? "within tolerance" : "OUTSIDE TOLERANCE"});
    }
    return table;
}

Table misclosureTable(const ReductionReport& report)
{
    Table table{"Misclosures and checks",
                {"Name", "Angular", "Allowable", "dN", "dE", "Linear", "Length", "1 : N",
                 "Height", "Check"},
                {}};
    for (const MisclosureReport& m : report.misclosures) {
        table.rows.push_back(
            {m.name, m.angular ? seconds(*m.angular) : "-",
             m.angularAllowable ? seconds(*m.angularAllowable) : "-",
             optionalMillimetres(m.northing), optionalMillimetres(m.easting),
             optionalMillimetres(m.linear), metres(m.length),
             m.precisionRatio ? "1 : " + fixed(*m.precisionRatio, 0) : "-",
             optionalMillimetres(m.height),
             m.withinTolerance ? (*m.withinTolerance ? "within" : "OUTSIDE") : "-"});
    }
    return table;
}

std::vector<Table> adjustmentTables(const ReductionReport& report)
{
    std::vector<Table> tables;
    for (const AdjustmentReport& adjustment : report.adjustments) {
        Table summary{"Adjustment: " + adjustment.method, {"", ""}, {}, true};
        summary.rows.push_back({"Unknowns", std::to_string(adjustment.unknowns)});
        summary.rows.push_back({"Observations", std::to_string(adjustment.observations)});
        summary.rows.push_back({"Redundancy", std::to_string(adjustment.redundancy)});
        summary.rows.push_back({"Iterations", std::to_string(adjustment.iterations)});
        summary.rows.push_back(
            {"Variance factor",
             adjustment.varianceFactor
                 ? fixed(*adjustment.varianceFactor, 3) + " (standard error of unit weight " +
                       fixed(std::sqrt(*adjustment.varianceFactor), 3) + ")"
                 : "undefined (no redundancy)"});
        if (adjustment.globalTest) {
            const ReportGlobalTest& test = *adjustment.globalTest;
            summary.rows.push_back(
                {"Chi-square global test",
                 fixed(test.statistic, 3) + " against [" + fixed(test.lowerCritical, 3) + ", " +
                     fixed(test.upperCritical, 3) + "] at " + fixed(test.significanceLevel, 3) +
                     ": " + (test.passed ? "passed" : "FAILED")});
        }
        for (const std::string& flagged : adjustment.flaggedOutliers) {
            summary.rows.push_back({"Flagged outlier", flagged});
        }
        for (const std::string& rejected : adjustment.rejectedOutliers) {
            summary.rows.push_back({"Rejected outlier", rejected});
        }
        tables.push_back(std::move(summary));

        Table residuals{"Residuals: " + adjustment.method,
                        {"Observation", "Residual", "A-priori sigma", "Redundancy",
                         "Standardised", "Flag", "Source"},
                        {}};
        for (const ReportResidual& residual : adjustment.residuals) {
            residuals.rows.push_back(
                {residual.observation,
                 residual.angular ? seconds(residual.residual) : millimetres(residual.residual),
                 residual.angular ? seconds(residual.sigma) : millimetres(residual.sigma),
                 fixed(residual.redundancyNumber, 2),
                 residual.standardised ? signedFixed(*residual.standardised, 2) : "-",
                 residual.rejected ? "REJECTED" : (residual.flagged ? "FLAGGED" : ""),
                 sourceText(residual.source)});
        }
        tables.push_back(std::move(residuals));

        if (!adjustment.ellipses.empty()) {
            Table ellipses{"Error ellipses: " + adjustment.method,
                           {"Point", "Semi-major (1 sigma)", "Semi-minor (1 sigma)",
                            "Orientation", "Confidence scale", "Semi-major at confidence",
                            "Semi-minor at confidence"},
                           {}};
            for (const ReportEllipse& ellipse : adjustment.ellipses) {
                ellipses.rows.push_back(
                    {ellipse.pointId, millimetres(ellipse.standard.semiMajor),
                     millimetres(ellipse.standard.semiMinor), dms(ellipse.standard.orientation, 0),
                     fixed(ellipse.confidenceScale, 4),
                     millimetres(ellipse.standard.semiMajor * ellipse.confidenceScale),
                     millimetres(ellipse.standard.semiMinor * ellipse.confidenceScale)});
            }
            tables.push_back(std::move(ellipses));
        }
    }
    return tables;
}

Table coordinateTable(const ReductionReport& report)
{
    Table table{"Coordinates",
                {"Point", "Northing", "Easting", "Height", "sN", "sE", "sH", "Method",
                 "Shift N", "Shift E", "Shift H"},
                {}};
    table.rows.reserve(report.coordinates.size());
    for (const CoordinateReport& c : report.coordinates) {
        table.rows.push_back(
            {c.pointId, metres(c.northing), metres(c.easting), optionalMetres(c.elevation),
             c.sigmaNorthing ? fixed(*c.sigmaNorthing * 1000.0, 1) : "-",
             c.sigmaEasting ? fixed(*c.sigmaEasting * 1000.0, 1) : "-",
             c.sigmaElevation ? fixed(*c.sigmaElevation * 1000.0, 1) : "-", toString(c.method),
             optionalMillimetres(c.shiftNorthing), optionalMillimetres(c.shiftEasting),
             optionalMillimetres(c.shiftElevation)});
    }
    return table;
}

Table warningTable(const ReductionReport& report)
{
    Table table{"Warnings", {"Warning", "Source"}, {}};
    for (const ReportMessage& warning : report.warnings) {
        table.rows.push_back({warning.text, sourceText(warning.source)});
    }
    return table;
}

std::vector<Table> allTables(const ReductionReport& report)
{
    std::vector<Table> tables;
    tables.push_back(inputTable(report));
    tables.push_back(settingsTable(report.settings));
    tables.push_back(warningTable(report));
    tables.push_back(setupTable(report));
    tables.push_back(facePairTable(report));
    tables.push_back(observationTable(report));
    tables.push_back(misclosureTable(report));
    for (Table& table : adjustmentTables(report)) {
        tables.push_back(std::move(table));
    }
    tables.push_back(coordinateTable(report));
    return tables;
}

// ---- Text ---------------------------------------------------------------------------

// Displayed characters: UTF-8 continuation bytes do not count.
std::size_t displayWidth(std::string_view text)
{
    std::size_t width = 0;
    for (const char c : text) {
        if ((static_cast<unsigned char>(c) & 0xC0) != 0x80) {
            ++width;
        }
    }
    return width;
}

void pad(std::string& out, std::string_view text, std::size_t width)
{
    out += text;
    for (std::size_t w = displayWidth(text); w < width; ++w) {
        out += ' ';
    }
}

// A cell wider than this is not allowed to widen its whole column: the long
// notes (corrections, warnings) go last in their row and run on.
constexpr std::size_t kMaxColumn = 28;

void renderTextTable(std::string& out, const Table& table)
{
    out += table.title;
    out += '\n';
    out.append(displayWidth(table.title), '=');
    out += '\n';
    if (table.rows.empty()) {
        out += "  " + table.empty + "\n\n";
        return;
    }
    const std::size_t columns = table.header.size();
    std::vector<std::size_t> widths(columns, 0);
    for (std::size_t c = 0; c < columns; ++c) {
        if (!table.keyValue) {
            widths[c] = displayWidth(table.header[c]);
        }
        for (const auto& cells : table.rows) {
            widths[c] = std::max(widths[c], std::min(displayWidth(cells[c]), kMaxColumn));
        }
    }
    const auto line = [&](const std::vector<std::string>& cells) {
        std::string text = "  ";
        for (std::size_t c = 0; c < columns; ++c) {
            if (c + 1 == columns) {
                text += cells[c];
            } else {
                pad(text, cells[c], widths[c]);
                text += "  ";
            }
        }
        while (!text.empty() && text.back() == ' ') {
            text.pop_back();
        }
        out += text;
        out += '\n';
    };
    if (!table.keyValue) {
        line(table.header);
        std::string rule = "  ";
        for (std::size_t c = 0; c < columns; ++c) {
            rule.append(c + 1 == columns ? displayWidth(table.header[c]) : widths[c], '-');
            rule += c + 1 == columns ? "" : "  ";
        }
        out += rule + '\n';
    }
    for (const auto& cells : table.rows) {
        line(cells);
    }
    out += '\n';
}

// ---- HTML ---------------------------------------------------------------------------

void escape(std::string& out, std::string_view text)
{
    for (const char c : text) {
        switch (c) {
        case '&':
            out += "&amp;";
            break;
        case '<':
            out += "&lt;";
            break;
        case '>':
            out += "&gt;";
            break;
        case '"':
            out += "&quot;";
            break;
        case '\'':
            out += "&#39;";
            break;
        default:
            out += c;
        }
    }
}

bool isAlarm(std::string_view cell)
{
    return cell.starts_with("REJECTED") || cell == "FLAGGED" || cell == "OUTSIDE TOLERANCE" ||
           cell == "OUTSIDE" || cell.find("FAILED") != std::string_view::npos;
}

void renderHtmlTable(std::string& out, const Table& table)
{
    out += "<section><h2>";
    escape(out, table.title);
    out += "</h2>\n";
    if (table.rows.empty()) {
        out += "<p class=\"empty\">";
        escape(out, table.empty);
        out += "</p></section>\n";
        return;
    }
    out += table.keyValue ? "<table class=\"kv\">\n" : "<table>\n<thead><tr>";
    if (!table.keyValue) {
        for (const std::string& heading : table.header) {
            out += "<th>";
            escape(out, heading);
            out += "</th>";
        }
        out += "</tr></thead>\n";
    }
    out += "<tbody>\n";
    for (const auto& cells : table.rows) {
        out += "<tr>";
        for (std::size_t c = 0; c < cells.size(); ++c) {
            const bool head = table.keyValue && c == 0;
            out += head ? "<th>" : (isAlarm(cells[c]) ? "<td class=\"alarm\">" : "<td>");
            escape(out, cells[c]);
            out += head ? "</th>" : "</td>";
        }
        out += "</tr>\n";
    }
    out += "</tbody></table></section>\n";
}

} // namespace

std::string renderText(const ReductionReport& report)
{
    std::string out;
    out.reserve(256 + report.observations.size() * 160);
    out += "REDUCTION REPORT\n\n";
    for (const Table& table : allTables(report)) {
        renderTextTable(out, table);
    }
    return out;
}

std::string renderHtml(const ReductionReport& report)
{
    std::string out;
    out.reserve(2048 + report.observations.size() * 260);
    out += "<!DOCTYPE html>\n<html lang=\"en\"><head><meta charset=\"utf-8\">"
           "<title>Reduction report";
    if (!report.input.fileName.empty()) {
        out += " - ";
        escape(out, report.input.fileName);
    }
    out += "</title>\n<style>\n"
           "body{font-family:Segoe UI,Helvetica,Arial,sans-serif;font-size:10pt;margin:1.5em;"
           "color:#1a1a1a;background:#fff}\n"
           "h1{font-size:16pt;margin:0 0 .3em}h2{font-size:12pt;margin:1.4em 0 .4em;"
           "border-bottom:1px solid #999}\n"
           "table{border-collapse:collapse;font-variant-numeric:tabular-nums;width:auto}\n"
           "th,td{border:1px solid #ccc;padding:2px 6px;text-align:left;vertical-align:top;"
           "white-space:nowrap}\n"
           "td:nth-last-child(2),td:last-child{white-space:normal}\n"
           "thead th{background:#eee}table.kv th{background:#f5f5f5;font-weight:600}\n"
           "td.alarm{color:#a00;font-weight:600}p.empty{color:#666}\n"
           "@media print{body{margin:0;font-size:8pt}section{break-inside:auto}"
           "thead{display:table-header-group}tr{break-inside:avoid}}\n"
           "</style></head>\n<body>\n<h1>Reduction report";
    if (!report.input.fileName.empty()) {
        out += ": ";
        escape(out, report.input.fileName);
    }
    out += "</h1>\n";
    for (const Table& table : allTables(report)) {
        renderHtmlTable(out, table);
    }
    out += "</body></html>\n";
    return out;
}

} // namespace katana::survey
