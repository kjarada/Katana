// Delimited point files: the reader, the writer, and the format's registration.
// The layout type and its template are in delimited_layout.cpp, the proposal in
// delimited_proposal.cpp, and the record splitter all three share in
// delimited_records.cpp.

#include <array>
#include <charconv>
#include <cmath>
#include <map>
#include <system_error>
#include <utility>

#include "delimited_internal.hpp"
#include "katana/core/text.hpp"
#include "katana/math/unit_ratio.hpp"
#include "katana/surveyio/delimited_points.hpp"
#include "katana/surveyio/detect.hpp"

namespace katana::surveyio {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;
using katana::survey::SurveyPoint;

namespace {

// The version of THIS code (format.hpp). Bump it when a file would import
// differently, so the SourceRecords of old imports say which reading they had.
constexpr const char* kParserVersion = "1.0";

// A specific format that clears the identification bar must also beat this one
// by the margin, whatever this probe says; see probeDelimitedPoints.
static_assert(kDelimitedHeaderConfidence < kIdentifiedConfidence - kIdentificationMargin,
              "the delimited probe must never be close enough to a confident specific format "
              "to make the detection ambiguous");
static_assert(kDelimitedHeaderlessConfidence < kDelimitedHeaderConfidence);

std::string where(std::size_t line, std::size_t column, ColumnRole role)
{
    return "line " + std::to_string(line) + ", column " + std::to_string(column) + " (" +
           toString(role) + ")";
}

bool isRequired(ColumnRole role)
{
    return role == ColumnRole::PointId || role == ColumnRole::Northing ||
           role == ColumnRole::Easting;
}

bool hasRole(const DelimitedLayout& layout, ColumnRole role)
{
    for (const ColumnRole column : layout.columns) {
        if (column == role) {
            return true;
        }
    }
    return false;
}

} // namespace

// ---- Import ------------------------------------------------------------------------

Result<ImportResult> parseDelimitedPoints(std::string_view bytes, const DelimitedLayout& layout,
                                          std::string_view fileName,
                                          const DelimitedImportOptions& options)
{
    const std::string name = katana::survey::sourceFileName(fileName);
    if (Status valid = validateLayout(layout); !valid.ok()) {
        return makeError(valid.error().code, valid.error().message, name);
    }
    const Result<katana::math::UnitRatio> unit = katana::survey::metresPer(options.unit);
    if (!unit) {
        return makeError(ErrorCode::InvalidArgument,
                         "state the unit the coordinates are in: a delimited file does not say, "
                         "and none is assumed",
                         name);
    }
    Result<katana::core::DecodedText> decoded = options.encoding
                                                    ? katana::core::decodeTextAs(bytes,
                                                                                 *options.encoding)
                                                    : katana::core::decodeText(bytes);
    if (!decoded) {
        return makeError(ErrorCode::ParseFailure, decoded.error().message, name);
    }

    const FormatDescriptor descriptor = delimitedPointsFormat();
    katana::survey::SourceRecord fileSource;
    fileSource.manufacturer = toString(descriptor.manufacturer);
    fileSource.format = descriptor.humanName;
    // The layout, not the parser version: it is what says how THIS file was
    // read, and with it the import can be repeated exactly.
    fileSource.formatVersion = layoutTemplate(layout);
    fileSource.fileName = name;

    ImportResult result;
    result.formatId = descriptor.id;
    katana::survey::SurveyProject& project = result.project;
    project.name = name;
    project.source = fileSource;
    // A delimited file cannot declare a coordinate system: the default,
    // explicitly unknown, is the truth. The unit is the caller's statement.
    project.units.linear = options.unit;
    project.metadata.emplace("text encoding", katana::core::toString(decoded->encoding));

    const bool idColumn = hasRole(layout, ColumnRole::PointId);
    std::map<std::string, std::size_t, std::less<>> lineOfId;
    std::size_t shortRows = 0;
    std::size_t firstShortRow = 0;
    std::size_t withoutElevation = 0;
    std::size_t firstWithoutElevation = 0;

    delimited::RecordReader reader(decoded->text, layout);
    while (true) {
        Result<std::optional<delimited::Record>> next = reader.next();
        if (!next) {
            return makeError(next.error().code, next.error().message, name);
        }
        if (!next.value()) {
            break;
        }
        const delimited::Record& record = *next.value();

        // Text past the last column is data this layout has nowhere to put - most
        // often a description with an unquoted delimiter in it, which would
        // otherwise be cut short. Empty trailing fields ("a,b,c,") are nothing.
        for (std::size_t i = layout.columns.size(); i < record.fields.size(); ++i) {
            const std::string_view extra = delimited::valueOf(record.fields[i]);
            if (!extra.empty()) {
                return makeError(ErrorCode::ParseFailure,
                                 "line " + std::to_string(record.line) + ": " +
                                     std::to_string(record.fields.size()) +
                                     " fields where the layout has " +
                                     std::to_string(layout.columns.size()) + "; column " +
                                     std::to_string(i + 1) + " holds " +
                                     delimited::quotedForMessage(extra),
                                 name);
            }
        }

        SurveyPoint point;
        bool shortRow = false;
        for (std::size_t c = 0; c < layout.columns.size(); ++c) {
            const ColumnRole role = layout.columns[c];
            if (role == ColumnRole::Ignore) {
                continue;
            }
            if (c >= record.fields.size()) {
                if (isRequired(role)) {
                    return makeError(ErrorCode::ParseFailure,
                                     where(record.line, c + 1, role) +
                                         ": missing; the line has only " +
                                         std::to_string(record.fields.size()) + " fields",
                                     name);
                }
                shortRow = true;
                continue;
            }
            const std::string_view value = delimited::valueOf(record.fields[c]);
            switch (role) {
            case ColumnRole::PointId:
                if (value.empty()) {
                    // No id is invented for a point whose id column is empty: an
                    // invented one would collide with, or be mistaken for, a real
                    // mark's number.
                    return makeError(ErrorCode::ParseFailure,
                                     where(record.line, c + 1, role) + ": empty", name);
                }
                point.id = std::string(value);
                break;
            case ColumnRole::Northing:
            case ColumnRole::Easting:
            case ColumnRole::Elevation: {
                if (value.empty()) {
                    if (role == ColumnRole::Elevation) {
                        break; // absent, and absent is not zero
                    }
                    return makeError(ErrorCode::ParseFailure,
                                     where(record.line, c + 1, role) + ": empty", name);
                }
                const std::optional<double> number = katana::core::parseFiniteDouble(value);
                if (!number) {
                    return makeError(ErrorCode::ParseFailure,
                                     where(record.line, c + 1, role) + ": " +
                                         delimited::quotedForMessage(value) + " is not a number",
                                     name);
                }
                const double metres = katana::math::toMetres(*number, unit.value());
                if (role == ColumnRole::Northing) {
                    point.northing = metres;
                } else if (role == ColumnRole::Easting) {
                    point.easting = metres;
                } else {
                    point.elevation = metres;
                }
                break;
            }
            case ColumnRole::Code:
                point.code = std::string(value);
                break;
            case ColumnRole::Description:
                point.description = std::string(value);
                break;
            case ColumnRole::Ignore:
                break;
            }
        }

        if (!idColumn) {
            point.id = std::to_string(record.line);
        }
        const auto [previous, inserted] = lineOfId.emplace(point.id, record.line);
        if (!inserted) {
            const std::size_t column = [&] {
                for (std::size_t c = 0; c < layout.columns.size(); ++c) {
                    if (layout.columns[c] == ColumnRole::PointId) {
                        return c + 1;
                    }
                }
                return std::size_t{0};
            }();
            return makeError(ErrorCode::AlreadyExists,
                             where(record.line, column, ColumnRole::PointId) + ": " +
                                 delimited::quotedForMessage(point.id) +
                                 " is already the id of the point on line " +
                                 std::to_string(previous->second),
                             name);
        }
        if (shortRow) {
            firstShortRow = shortRows == 0 ? record.line : firstShortRow;
            ++shortRows;
        }
        if (!point.elevation) {
            firstWithoutElevation = withoutElevation == 0 ? record.line : firstWithoutElevation;
            ++withoutElevation;
        }
        point.source = fileSource;
        point.source.recordNumber = record.line;
        project.points.push_back(std::move(point));
    }
    result.recordsRead = project.points.size();

    // ---- What a person should know about how the file was read ----
    std::vector<std::string>& warnings = result.warnings;
    if (decoded->guessed) {
        warnings.push_back(std::string("the text encoding was not stated and was inferred as ") +
                           katana::core::toString(decoded->encoding) +
                           "; if a name or code shows the wrong characters, state the encoding "
                           "and import again");
    }
    if (project.points.empty()) {
        warnings.push_back("the file holds no points");
    }
    if (!idColumn && !project.points.empty()) {
        warnings.push_back("the layout has no point id column, so each point is named by the "
                           "number of the line it is on (the first is '" +
                           project.points.front().id + "')");
    }
    if (reader.commentLines() != 0) {
        warnings.push_back(std::to_string(reader.commentLines()) +
                           " comment line(s) starting '" + layout.commentPrefix +
                           "' were skipped");
    }
    if (shortRows != 0) {
        warnings.push_back(std::to_string(shortRows) +
                           " line(s) end before the layout's last column, and what they lack "
                           "is left empty (the first is line " +
                           std::to_string(firstShortRow) + ")");
    }
    if (!hasRole(layout, ColumnRole::Elevation)) {
        if (!project.points.empty()) {
            warnings.push_back("the layout has no elevation column, so no point has an "
                               "elevation");
        }
    } else if (withoutElevation != 0) {
        warnings.push_back(std::to_string(withoutElevation) + " of " +
                           std::to_string(project.points.size()) +
                           " point(s) have no elevation and are imported without one, not at "
                           "zero (the first is on line " +
                           std::to_string(firstWithoutElevation) + ")");
    }

    if (Status valid = katana::survey::validateProject(project); !valid.ok()) {
        return valid.error();
    }
    return result;
}

// ---- Export ------------------------------------------------------------------------

namespace {

// Fixed notation with `decimals` places, from to_chars: the locale is never
// consulted, so a GUI that set a decimal comma cannot put one in the file.
Result<std::string> fixedDecimals(double value, int decimals)
{
    // The longest fixed form of a finite double is 309 integer digits, a sign,
    // a point and kMaximumExportDecimals places.
    std::array<char, 400> buffer{};
    const auto [end, error] = std::to_chars(buffer.data(), buffer.data() + buffer.size(), value,
                                            std::chars_format::fixed, decimals);
    if (error != std::errc{}) {
        return makeError(ErrorCode::Internal, "a coordinate could not be formatted");
    }
    std::string text(buffer.data(), end);
    // A value that rounds to zero keeps its sign ("-0.000"). There is no such
    // coordinate, and a person reading the file would wonder what it meant.
    if (text.size() > 1 && text.front() == '-' &&
        text.find_first_not_of("0.", 1) == std::string::npos) {
        text.erase(0, 1);
    }
    return text;
}

bool isPadding(char c)
{
    return c == ' ' || c == '\t' || c == '\v' || c == '\f';
}

// `value` as it has to be written for the reader to give it back exactly, or an
// error saying why it cannot be written with this layout. `what` names the
// value for that error.
Result<std::string> encodeField(std::string_view value, bool firstColumn,
                                const DelimitedLayout& layout, const std::string& what)
{
    const char delimiter = delimited::delimiterCharacter(layout.delimiter);
    const bool runs = layout.delimiter == Delimiter::Whitespace;
    std::string_view reason;
    if (value.empty()) {
        if (runs) {
            reason = "is empty, and an empty field between blanks vanishes";
        }
    } else {
        if (katana::core::isAsciiSpace(value.front()) || katana::core::isAsciiSpace(value.back())) {
            reason = "has blanks at an end, which a reader trims";
        }
        for (const char c : value) {
            if (c == '\n' || c == '\r') {
                reason = "holds a line break";
            } else if (runs ? isPadding(c) : c == delimiter) {
                reason = "holds the delimiter";
            } else if (c == '"' && layout.quoting == Quoting::DoubleQuote) {
                // Only needed at the start, but quoting every field that holds a
                // quote is what RFC 4180 asks and what other readers expect.
                reason = "holds a quote";
            }
        }
        if (firstColumn && !layout.commentPrefix.empty() &&
            katana::core::trimmed(value).starts_with(layout.commentPrefix)) {
            reason = "begins with the comment prefix, so its line would be skipped";
        }
    }
    if (reason.empty()) {
        return std::string(value);
    }
    if (layout.quoting == Quoting::None) {
        return makeError(ErrorCode::InvalidArgument,
                         what + " " + std::string(reason) +
                             ", and the layout does not allow quoting");
    }
    std::string quoted = "\"";
    for (const char c : value) {
        if (c == '"') {
            quoted += '"';
        }
        quoted += c;
    }
    quoted += '"';
    return quoted;
}

} // namespace

Result<std::string> writeDelimitedPoints(std::span<const SurveyPoint> points,
                                         const DelimitedLayout& layout,
                                         const DelimitedExportOptions& options)
{
    if (Status valid = validateLayout(layout); !valid.ok()) {
        return valid.error();
    }
    if (layout.headerLines > 1) {
        return makeError(ErrorCode::InvalidArgument,
                         "the layout skips " + std::to_string(layout.headerLines) +
                             " header lines, and this writer writes one; a file it wrote would "
                             "lose its first points on reading back");
    }
    if (options.decimals < 0 || options.decimals > kMaximumExportDecimals) {
        return makeError(ErrorCode::InvalidArgument,
                         "choose between 0 and " + std::to_string(kMaximumExportDecimals) +
                             " decimal places (" + std::to_string(options.decimals) +
                             " was given)");
    }
    const Result<katana::math::UnitRatio> unit = katana::survey::metresPer(options.unit);
    if (!unit) {
        return makeError(ErrorCode::InvalidArgument,
                         "state the unit to write coordinates in; none is assumed");
    }

    // Checked before anything is written, so a refusal names the first bad
    // point rather than leaving half a file.
    if (hasRole(layout, ColumnRole::PointId)) {
        std::map<std::string_view, std::size_t> firstIndex;
        for (std::size_t i = 0; i < points.size(); ++i) {
            if (points[i].id.empty()) {
                return makeError(ErrorCode::InvalidArgument,
                                 "point " + std::to_string(i + 1) +
                                     " in the list has no id, and the layout has an id column");
            }
            if (const auto [at, inserted] = firstIndex.emplace(points[i].id, i); !inserted) {
                return makeError(ErrorCode::InvalidArgument,
                                 "point id '" + points[i].id + "' is used twice (points " +
                                     std::to_string(at->second + 1) + " and " +
                                     std::to_string(i + 1) +
                                     "), and a file with a repeated id cannot be read back");
            }
        }
    }

    const std::string separator(1, delimited::delimiterCharacter(layout.delimiter));
    std::string out;

    if (layout.headerLines == 1) {
        for (std::size_t c = 0; c < layout.columns.size(); ++c) {
            Result<std::string> cell =
                encodeField(headerName(layout.columns[c]), c == 0, layout,
                            std::string("the header of column ") + std::to_string(c + 1));
            if (!cell) {
                return cell.error();
            }
            out += c == 0 ? "" : separator;
            out += cell.value();
        }
        out += "\r\n";
    }

    for (std::size_t i = 0; i < points.size(); ++i) {
        const SurveyPoint& point = points[i];
        const std::string label =
            point.id.empty() ? "point " + std::to_string(i + 1) : "point '" + point.id + "'";
        for (std::size_t c = 0; c < layout.columns.size(); ++c) {
            const ColumnRole role = layout.columns[c];
            std::string value;
            std::optional<double> metres;
            switch (role) {
            case ColumnRole::PointId:
                value = point.id;
                break;
            case ColumnRole::Northing:
                metres = point.northing;
                break;
            case ColumnRole::Easting:
                metres = point.easting;
                break;
            case ColumnRole::Elevation:
                metres = point.elevation; // absent stays absent: an empty field
                break;
            case ColumnRole::Code:
                value = point.code;
                break;
            case ColumnRole::Description:
                value = point.description;
                break;
            case ColumnRole::Ignore:
                break;
            }
            if (metres) {
                const double converted = katana::math::fromMetres(*metres, unit.value());
                if (!std::isfinite(converted)) {
                    return makeError(ErrorCode::InvalidArgument,
                                     label + ": its " + toString(role) + " is not a finite number");
                }
                Result<std::string> text = fixedDecimals(converted, options.decimals);
                if (!text) {
                    return text.error();
                }
                value = std::move(text).value();
            }
            Result<std::string> cell =
                encodeField(value, c == 0, layout, label + ": its " + toString(role));
            if (!cell) {
                return cell.error();
            }
            out += c == 0 ? "" : separator;
            out += cell.value();
        }
        out += "\r\n";
    }
    return out;
}

// ---- The format --------------------------------------------------------------------

FormatDescriptor delimitedPointsFormat()
{
    FormatDescriptor format;
    format.id = std::string(kDelimitedPointsFormatId);
    format.humanName = "Delimited text points (CSV, TXT)";
    format.manufacturer = Manufacturer::Generic;
    format.reads.points = true;
    format.canImport = true;
    format.canExport = true;
    format.parserVersion = kParserVersion;
    // What such files are usually called. The probe does not consult them - a
    // .txt is anything at all - so they are for file dialogs, not detection.
    format.extensions = {"csv", "tsv", "txt"};
    return format;
}

FormatSignature probeDelimitedPoints(const ProbeInput& input)
{
    Result<std::string> text = delimited::decodeSample(input.bytes, input.truncated);
    if (!text) {
        return ruledOut();
    }
    const delimited::Analysis analysis = delimited::analyse(text.value());
    if (analysis.proposal.candidates().empty() || !analysis.delimiter) {
        return ruledOut();
    }
    std::string evidence =
        std::string(toString(*analysis.delimiter)) + "-delimited rows of numbers";
    if (analysis.headerNamesCoordinates) {
        return {kDelimitedHeaderConfidence,
                evidence + " under a header naming the coordinate columns; a generic reading, "
                           "which any specific format outranks"};
    }
    return {kDelimitedHeaderlessConfidence,
            evidence + " with no header naming the columns, so the coordinate order is not "
                       "stated"};
}

namespace {

const FormatRegistration kRegistration{delimitedPointsFormat(), &probeDelimitedPoints};

} // namespace

} // namespace katana::surveyio
