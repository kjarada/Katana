#pragma once

// The delimited text both subsurface readers share: utility_csv.cpp, which
// resolves the header against the columns it interprets, and
// subsurface_delivery_schema.cpp, which checks it against a delivery schema's
// attribute names. Private to katana_survey.

#include <cstddef>
#include <string>
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"

namespace katana::survey::subsurface::detail {

// Lower case with blanks, '-' and '_' removed, so that "Asset Owner",
// "asset_owner" and "AssetOwner" are one name.
[[nodiscard]] std::string key(std::string_view text);

// "line 12": where an error is.
[[nodiscard]] std::string at(std::size_t lineNumber);

struct RawRow {
    std::size_t lineNumber = 0;      // 1-based line of the text
    std::vector<std::string> fields; // trimmed, unquoted
};

struct RawTable {
    std::size_t headerLine = 0;
    std::vector<std::string> header;
    std::vector<RawRow> rows;
};

// The header and rows of `text`: UTF-8, an optional BOM skipped, blank lines
// and lines starting with '#' skipped, fields separated by commas, a field
// optionally double-quoted with "" for a quote inside it. ParseFailure, naming
// the line, for unbalanced quotes, a row whose width differs from the
// header's, or no header at all.
[[nodiscard]] core::Result<RawTable> readRawTable(std::string_view text);

} // namespace katana::survey::subsurface::detail
