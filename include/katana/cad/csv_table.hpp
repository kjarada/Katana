#pragma once

// The one CSV writer of the report verbs (VOLUME ... csv=, CORRIDOR QUANTITIES
// csv=): RFC 4180 records ending in CRLF, their fields quoted by cad::csvField
// (code_edit.hpp); a file already there replaced only with OVERWRITE, as every
// verb that writes a file does (docs/volumes.md); and a reply record that says
// what was written.

#include <string>
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"

namespace katana::cad {

struct CsvTable {
    std::vector<std::string> header;
    std::vector<std::vector<std::string>> rows;
};

// The table written to `path`: the header, then the rows, each ending in
// CRLF. AlreadyExists when the file is there and `overwrite` is not given
// (nothing is touched), FileExportFailure when it cannot be written. Replies
// "csv file=<path> rows=<n>".
[[nodiscard]] katana::core::Result<std::string> writeCsvTable(const CsvTable& table,
                                                              const std::string& path,
                                                              bool overwrite);

} // namespace katana::cad
