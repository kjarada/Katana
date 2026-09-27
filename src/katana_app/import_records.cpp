// The IMPORT and EXPORT reply records every build writes (import_records.hpp).

#include "import_records.hpp"

#include "katana/cad/scope_verbs.hpp"
#include "katana/core/text.hpp"

namespace katana::app {

std::string pathText(const std::filesystem::path& path)
{
    const std::u8string text = path.generic_u8string();
    return std::string(text.begin(), text.end());
}

std::filesystem::path pathFromText(std::string_view utf8)
{
    return std::filesystem::path(std::u8string(utf8.begin(), utf8.end()));
}

std::string restOfLine(std::string_view line)
{
    std::string_view body = katana::core::trimmed(line);
    const std::size_t blank = body.find_first_of(" \t");
    body = blank == std::string_view::npos ? std::string_view{}
                                           : katana::core::trimmed(body.substr(blank));
    if (body.size() >= 2 && body.front() == '"' && body.back() == '"') {
        body = body.substr(1, body.size() - 2);
    }
    return std::string(body);
}

std::string recordText(std::string_view value)
{
    return value.empty() ? std::string() : katana::cad::recordValue(value);
}

std::string recordNumber(double value)
{
    // + 0.0: a -0 written as "-0" reads as a move where there is none.
    return katana::core::formatExactReal(value + 0.0);
}

std::string boundsText(const katana::geometry::Box2& box)
{
    if (box.empty()) {
        return {};
    }
    return recordNumber(box.min.x) + "," + recordNumber(box.min.y) + "," +
           recordNumber(box.max.x) + "," + recordNumber(box.max.y);
}

std::string placedRecord(const katana::cad::ImportPlacement& placement,
                         const katana::cad::ImportShift& placed)
{
    if (placement.mode == katana::cad::ImportPlacementMode::Keep) {
        return {};
    }
    std::string mode;
    switch (placement.mode) {
    case katana::cad::ImportPlacementMode::Local:
        mode = "local";
        break;
    case katana::cad::ImportPlacementMode::Alongside:
        mode = "alongside";
        break;
    case katana::cad::ImportPlacementMode::Offset:
        mode = "offset";
        break;
    case katana::cad::ImportPlacementMode::Keep:
        break;
    }
    // The readers SUBTRACT the shift (cad/import_placement.hpp); the record
    // says the move, which is what was added.
    const std::string east = placed.shift ? recordNumber(-placed.shift->x) : std::string();
    const std::string north = placed.shift ? recordNumber(-placed.shift->y) : std::string();
    return "placed placement=" + mode + " east=" + east + " north=" + north +
           " text=" + recordText(placed.said);
}

std::string tallyRecord(std::string_view element, std::size_t read, std::size_t imported)
{
    return "tally element=" + recordText(element) + " read=" + std::to_string(read) +
           " imported=" + std::to_string(imported);
}

std::string warningText(std::string_view text)
{
    return "warning text=" + recordText(text);
}

} // namespace katana::app
