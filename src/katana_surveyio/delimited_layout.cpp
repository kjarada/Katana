// The column layout of a delimited point file: roles, presets, validation and the
// one-line template a layout is saved as. The reader, the writer and the proposal
// are in delimited_points.cpp and delimited_proposal.cpp.

#include <algorithm>
#include <array>
#include <cstdint>
#include <map>
#include <utility>

#include "delimited_internal.hpp"
#include "katana/core/text.hpp"
#include "katana/surveyio/delimited_points.hpp"

namespace katana::surveyio {

using katana::core::Error;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;

// ---- Names -------------------------------------------------------------------------

const char* toString(ColumnRole role)
{
    switch (role) {
    case ColumnRole::PointId:
        return "point id";
    case ColumnRole::Northing:
        return "northing";
    case ColumnRole::Easting:
        return "easting";
    case ColumnRole::Elevation:
        return "elevation";
    case ColumnRole::Code:
        return "code";
    case ColumnRole::Description:
        return "description";
    case ColumnRole::Ignore:
        return "ignored";
    }
    return "ignored";
}

char templateLetter(ColumnRole role)
{
    switch (role) {
    case ColumnRole::PointId:
        return 'P';
    case ColumnRole::Northing:
        return 'N';
    case ColumnRole::Easting:
        return 'E';
    case ColumnRole::Elevation:
        return 'Z';
    case ColumnRole::Code:
        return 'C';
    case ColumnRole::Description:
        return 'D';
    case ColumnRole::Ignore:
        return '-';
    }
    return '-';
}

const char* headerName(ColumnRole role)
{
    switch (role) {
    case ColumnRole::PointId:
        return "Point";
    case ColumnRole::Northing:
        return "Northing";
    case ColumnRole::Easting:
        return "Easting";
    case ColumnRole::Elevation:
        return "Elevation";
    case ColumnRole::Code:
        return "Code";
    case ColumnRole::Description:
        return "Description";
    case ColumnRole::Ignore:
        return "";
    }
    return "";
}

const char* toString(Delimiter delimiter)
{
    switch (delimiter) {
    case Delimiter::Comma:
        return "comma";
    case Delimiter::Tab:
        return "tab";
    case Delimiter::Semicolon:
        return "semicolon";
    case Delimiter::Whitespace:
        return "whitespace";
    }
    return "comma";
}

const char* toString(Quoting quoting)
{
    switch (quoting) {
    case Quoting::DoubleQuote:
        return "double";
    case Quoting::None:
        return "none";
    }
    return "double";
}

// ---- Validation --------------------------------------------------------------------

Status validateLayout(const DelimitedLayout& layout)
{
    std::map<ColumnRole, std::size_t> seen;
    for (std::size_t i = 0; i < layout.columns.size(); ++i) {
        const ColumnRole role = layout.columns[i];
        if (role == ColumnRole::Ignore) {
            continue;
        }
        const auto [where, inserted] = seen.emplace(role, i + 1);
        if (!inserted) {
            // Two easting columns is not a layout: which one is the easting?
            return makeError(ErrorCode::InvalidArgument,
                             std::string("the layout has two ") + toString(role) +
                                 " columns (" + std::to_string(where->second) + " and " +
                                 std::to_string(i + 1) + ")");
        }
    }
    if (!seen.contains(ColumnRole::Northing) || !seen.contains(ColumnRole::Easting)) {
        return makeError(ErrorCode::InvalidArgument,
                         "the layout needs a northing column and an easting column",
                         "a point is not a point without both");
    }

    const std::string& prefix = layout.commentPrefix;
    if (!prefix.empty()) {
        const char first = prefix.front();
        if ((first >= '0' && first <= '9') || first == '+' || first == '-' || first == '.') {
            // A data line can start with any of these, and would be skipped as
            // a comment: a lost point, not a failed import.
            return makeError(ErrorCode::InvalidArgument,
                             "comment prefix '" + prefix +
                                 "' could begin a number, so a line of data could be taken "
                                 "for a comment");
        }
        // Whitespace needs no check of its own: every blank is refused below.
        const char delimiter = delimited::delimiterCharacter(layout.delimiter);
        for (const char c : prefix) {
            if (katana::core::isAsciiSpace(c) || c == '"' || c == ';' || c == delimiter) {
                // A blank would be trimmed off the line it is looked for in; a
                // quote would open a field; ';' separates template parts; the
                // delimiter would make a line of empty first fields a comment.
                return makeError(ErrorCode::InvalidArgument,
                                 "comment prefix '" + prefix +
                                     "' may not contain a blank, a quote, ';' or the delimiter");
            }
        }
    }
    return {};
}

// ---- Presets -----------------------------------------------------------------------

namespace {

constexpr std::array<ColumnPreset, 8> kPresets = {
    ColumnPreset::PNEZD, ColumnPreset::PENZD, ColumnPreset::PNEZ, ColumnPreset::PENZ,
    ColumnPreset::NEZ,   ColumnPreset::ENZ,   ColumnPreset::PNE,  ColumnPreset::PEN};

Result<ColumnRole> roleForLetter(std::string_view token)
{
    if (token.size() == 1) {
        switch (katana::core::asciiLower(token.front())) {
        case 'p':
            return ColumnRole::PointId;
        case 'n':
            return ColumnRole::Northing;
        case 'e':
            return ColumnRole::Easting;
        case 'z':
            return ColumnRole::Elevation;
        case 'c':
            return ColumnRole::Code;
        case 'd':
            return ColumnRole::Description;
        case '-':
            return ColumnRole::Ignore;
        default:
            break;
        }
    }
    return makeError(ErrorCode::InvalidArgument,
                     "'" + std::string(token) +
                         "' is not a column letter (P, N, E, Z, C, D, or - to ignore a column)");
}

} // namespace

std::span<const ColumnPreset> allColumnPresets()
{
    return kPresets;
}

const char* toString(ColumnPreset preset)
{
    switch (preset) {
    case ColumnPreset::PNEZD:
        return "PNEZD";
    case ColumnPreset::PENZD:
        return "PENZD";
    case ColumnPreset::PNEZ:
        return "PNEZ";
    case ColumnPreset::PENZ:
        return "PENZ";
    case ColumnPreset::NEZ:
        return "NEZ";
    case ColumnPreset::ENZ:
        return "ENZ";
    case ColumnPreset::PNE:
        return "PNE";
    case ColumnPreset::PEN:
        return "PEN";
    }
    return "PNEZD";
}

std::vector<ColumnRole> presetColumns(ColumnPreset preset)
{
    // The name IS the layout: one letter per column, spelt as templates spell
    // them, so the two cannot drift apart.
    std::vector<ColumnRole> columns;
    for (const char letter : std::string_view(toString(preset))) {
        columns.push_back(roleForLetter(std::string_view(&letter, 1)).value());
    }
    return columns;
}

Result<ColumnPreset> columnPresetNamed(std::string_view name)
{
    for (const ColumnPreset preset : kPresets) {
        if (katana::core::equalsIgnoringCase(name, toString(preset))) {
            return preset;
        }
    }
    return makeError(ErrorCode::NotFound,
                     "'" + std::string(name) + "' is not a column preset",
                     "PNEZD, PENZD, PNEZ, PENZ, NEZ, ENZ, PNE or PEN");
}

DelimitedLayout presetLayout(ColumnPreset preset, Delimiter delimiter, std::size_t headerLines)
{
    DelimitedLayout layout;
    layout.columns = presetColumns(preset);
    layout.delimiter = delimiter;
    layout.headerLines = headerLines;
    return layout;
}

// ---- Templates ---------------------------------------------------------------------

std::string layoutTemplate(const DelimitedLayout& layout)
{
    std::string text;
    for (const ColumnRole role : layout.columns) {
        if (!text.empty()) {
            text += ',';
        }
        text += templateLetter(role);
    }
    text += ";delimiter=";
    text += toString(layout.delimiter);
    text += ";header=" + std::to_string(layout.headerLines);
    text += ";quote=";
    text += toString(layout.quoting);
    if (!layout.commentPrefix.empty()) {
        text += ";comment=" + layout.commentPrefix;
    }
    return text;
}

namespace {

std::vector<std::string_view> splitOn(std::string_view text, char separator)
{
    std::vector<std::string_view> parts;
    std::size_t start = 0;
    while (true) {
        const std::size_t end = text.find(separator, start);
        if (end == std::string_view::npos) {
            parts.push_back(text.substr(start));
            return parts;
        }
        parts.push_back(text.substr(start, end - start));
        start = end + 1;
    }
}

Error templateError(std::string_view text, std::string message)
{
    return makeError(ErrorCode::InvalidArgument, std::move(message),
                     "layout template '" + std::string(text) + "'");
}

} // namespace

Result<DelimitedLayout> parseLayoutTemplate(std::string_view text)
{
    using katana::core::equalsIgnoringCase;
    using katana::core::trimmed;

    const std::vector<std::string_view> parts = splitOn(text, ';');
    for (const std::string_view part : parts) {
        if (trimmed(part).empty()) {
            return templateError(text, "the template has an empty part");
        }
    }

    DelimitedLayout layout;
    for (const std::string_view token : splitOn(parts.front(), ',')) {
        const std::string_view letter = trimmed(token);
        if (letter.empty()) {
            return templateError(text, "the column list has an empty entry");
        }
        Result<ColumnRole> role = roleForLetter(letter);
        if (!role) {
            return templateError(text, role.error().message);
        }
        layout.columns.push_back(role.value());
    }

    std::map<std::string, std::string_view, std::less<>> values;
    for (std::size_t i = 1; i < parts.size(); ++i) {
        const std::string_view part = trimmed(parts[i]);
        const std::size_t equals = part.find('=');
        if (equals == std::string_view::npos) {
            return templateError(text, "'" + std::string(part) + "' is not key=value");
        }
        std::string key = katana::core::lowered(trimmed(part.substr(0, equals)));
        const std::string_view value = trimmed(part.substr(equals + 1));
        if (key != "delimiter" && key != "header" && key != "quote" && key != "comment") {
            return templateError(text, "unknown key '" + key +
                                           "' (the keys are delimiter, header, quote, comment)");
        }
        if (value.empty()) {
            return templateError(text, "'" + key + "' has no value");
        }
        if (values.contains(key)) {
            // Which one was meant? Taking either would be a guess.
            return templateError(text, "'" + key + "' is given twice");
        }
        values.emplace(std::move(key), value);
    }

    const auto delimiter = values.find("delimiter");
    if (delimiter == values.end()) {
        return templateError(text, "the template does not state its delimiter");
    }
    bool knownDelimiter = false;
    for (const Delimiter candidate :
         {Delimiter::Comma, Delimiter::Tab, Delimiter::Semicolon, Delimiter::Whitespace}) {
        if (equalsIgnoringCase(delimiter->second, toString(candidate))) {
            layout.delimiter = candidate;
            knownDelimiter = true;
        }
    }
    if (!knownDelimiter) {
        return templateError(text, "delimiter '" + std::string(delimiter->second) +
                                       "' is not comma, tab, semicolon or whitespace");
    }

    const auto header = values.find("header");
    if (header == values.end()) {
        return templateError(text, "the template does not state how many header lines to skip");
    }
    // A plain count: parseInteger refuses "1.5", "1e2" and trailing text, and a
    // negative count of lines is not one.
    const std::optional<std::int64_t> count = katana::core::parseInteger(header->second);
    if (!count || *count < 0) {
        return templateError(text, "header '" + std::string(header->second) +
                                       "' is not a count of lines");
    }
    layout.headerLines = static_cast<std::size_t>(*count);

    if (const auto quote = values.find("quote"); quote != values.end()) {
        if (equalsIgnoringCase(quote->second, toString(Quoting::DoubleQuote))) {
            layout.quoting = Quoting::DoubleQuote;
        } else if (equalsIgnoringCase(quote->second, toString(Quoting::None))) {
            layout.quoting = Quoting::None;
        } else {
            return templateError(text,
                                 "quote '" + std::string(quote->second) + "' is not double or none");
        }
    }
    if (const auto comment = values.find("comment"); comment != values.end()) {
        layout.commentPrefix = std::string(comment->second);
    }

    if (Status valid = validateLayout(layout); !valid.ok()) {
        return templateError(text, valid.error().message);
    }
    return layout;
}

} // namespace katana::surveyio
