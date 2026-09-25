#include "subsurface_table.hpp"

#include <optional>
#include <utility>

#include "katana/core/text.hpp"

namespace katana::survey::subsurface::detail {

namespace {

using core::ErrorCode;
using core::makeError;

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

} // namespace

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

core::Result<RawTable> readRawTable(std::string_view text)
{
    if (text.starts_with("\xEF\xBB\xBF")) {
        text.remove_prefix(3);
    }
    RawTable table;
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
            table.headerLine = lineNumber;
            table.header = std::move(*fields);
            continue;
        }
        if (fields->size() != table.header.size()) {
            return makeError(ErrorCode::ParseFailure,
                             std::to_string(fields->size()) + " fields where the header has " +
                                 std::to_string(table.header.size()),
                             at(lineNumber));
        }
        table.rows.push_back({lineNumber, std::move(*fields)});
    }
    if (!haveHeader) {
        return makeError(ErrorCode::ParseFailure, "no header row");
    }
    return table;
}

} // namespace katana::survey::subsurface::detail
