// The GIS verbs' shared records, and any record as JSON (gis_records.hpp).

#include "gis_records.hpp"

#include <array>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "../import_records.hpp"
#include "katana/core/text.hpp"

namespace katana::app::geo {

namespace {

std::string yesNo(bool flag)
{
    return flag ? "yes" : "no";
}

// Two or more finite numbers joined by commas; nullopt for anything else,
// one number included (that is a number, not a list).
std::optional<std::vector<double>> numberList(std::string_view text)
{
    if (text.find(',') == std::string_view::npos) {
        return std::nullopt;
    }
    std::vector<double> numbers;
    for (std::string_view rest = text;;) {
        const std::size_t comma = rest.find(',');
        const auto number = katana::core::parseFiniteDouble(rest.substr(0, comma));
        if (!number) {
            return std::nullopt;
        }
        numbers.push_back(*number);
        if (comma == std::string_view::npos) {
            return numbers;
        }
        rest = rest.substr(comma + 1);
    }
}

} // namespace

std::string referenceRecord(const katana::interop::RasterOverlay& raster)
{
    return "reference id=" + std::to_string(raster.id) + " kind=raster name=" +
           value(raster.name) + " width=" + std::to_string(raster.width) +
           " height=" + std::to_string(raster.height) +
           " bounds=" + boundsText(raster.worldBounds()) +
           " georeferenced=" + yesNo(raster.hasGeotransform) + " visible=" + yesNo(raster.visible) +
           " opacity=" + recordNumber(raster.opacity) +
           " role=" + std::string(katana::interop::toString(raster.role)) +
           " display=" + std::string(katana::interop::toWord(raster.displayStyle)) +
           " file=" + value(pathText(raster.source));
}

std::string referenceRecord(const katana::interop::PointCloudLayer& cloud)
{
    return "reference id=" + std::to_string(cloud.id) + " kind=pointcloud name=" +
           value(cloud.name) + " points=" + std::to_string(cloud.points.size()) +
           " source_points=" + std::to_string(cloud.sourcePointCount) +
           " bounds=" + boundsText(cloud.worldBounds()) + " visible=" + yesNo(cloud.visible) +
           " color=" + std::string(katana::interop::toWord(cloud.colorMode)) +
           " file=" + value(pathText(cloud.source));
}

nlohmann::json recordJson(const Record& record)
{
    // The fields that are words whatever they hold.
    static constexpr std::array<std::string_view, 21> kWords{
        "file",   "name",    "text",    "crs",     "format",  "driver",   "source",
        "kind",   "element", "layer",   "color",   "role",    "encoding", "version",
        "member", "path",    "display", "derivation", "placement", "reason", "state"};
    nlohmann::json object{{"record", record.kind}};
    for (const auto& [key, text] : record.fields) {
        bool word = false;
        for (const std::string_view each : kWords) {
            word = word || key == each;
        }
        if (word) {
            object[key] = text;
            continue;
        }
        // A list of numbers - bounds and a scope's area x0,y0,x1,y1, a cell
        // of 4,3, classes 2,6 - is an array of them, so an agent reads a
        // cell's width without splitting a string. bounds is always four,
        // or null.
        if (const std::optional<std::vector<double>> numbers = numberList(text)) {
            object[key] = key == "bounds" && numbers->size() != 4 ? nlohmann::json(nullptr)
                                                                  : nlohmann::json(*numbers);
            continue;
        }
        if (key == "bounds") {
            object[key] = nullptr;
            continue;
        }
        if (text == "yes" || text == "no") {
            object[key] = text == "yes";
        } else if (const auto whole = katana::core::parseInteger(text)) {
            object[key] = *whole;
        } else if (const auto real = katana::core::parseFiniteDouble(text)) {
            object[key] = *real;
        } else if (text.empty()) {
            object[key] = nullptr; // absent, not an empty word
        } else {
            object[key] = text;
        }
    }
    if (!record.body.empty()) {
        std::string body;
        for (const std::string& line : record.body) {
            body += (body.empty() ? "" : "\n") + line;
        }
        object["body"] = body;
    }
    return object;
}

nlohmann::json recordsJson(std::string_view reply)
{
    nlohmann::json array = nlohmann::json::array();
    for (const Record& record : parseRecords(reply)) {
        if (!record.kind.empty()) {
            array.push_back(recordJson(record));
        }
    }
    return array;
}

} // namespace katana::app::geo
