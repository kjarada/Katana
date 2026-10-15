#include "katana/cad/csv_table.hpp"

#include <filesystem>
#include <fstream>
#include <system_error>

#include "katana/cad/scope_verbs.hpp"
#include "katana/core/path_text.hpp"

namespace katana::cad {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

std::string csvField(std::string_view text)
{
    if (text.find_first_of(",\"\r\n") == std::string_view::npos) {
        return std::string(text);
    }
    std::string out = "\"";
    for (const char c : text) {
        out += c;
        if (c == '"') {
            out += '"';
        }
    }
    return out + "\"";
}

Result<std::string> writeCsvTable(const CsvTable& table, const std::string& path, bool overwrite)
{
    const std::filesystem::path file = katana::core::pathFromUtf8(path);
    std::error_code error;
    if (std::filesystem::exists(file, error) && !overwrite) {
        return makeError(ErrorCode::AlreadyExists,
                         "the file is already there; OVERWRITE replaces it", path);
    }
    std::ofstream out(file, std::ios::binary | std::ios::trunc);
    if (!out) {
        return makeError(ErrorCode::FileExportFailure, "the file cannot be written", path);
    }
    const auto line = [&out](const std::vector<std::string>& fields) {
        for (std::size_t i = 0; i < fields.size(); ++i) {
            out << (i == 0 ? "" : ",") << csvField(fields[i]);
        }
        out << "\r\n";
    };
    line(table.header);
    for (const auto& row : table.rows) {
        line(row);
    }
    out.close();
    if (!out) {
        return makeError(ErrorCode::FileExportFailure, "the file could not be finished", path);
    }
    return "csv file=" + recordValue(path) + " rows=" + std::to_string(table.rows.size());
}

} // namespace katana::cad
