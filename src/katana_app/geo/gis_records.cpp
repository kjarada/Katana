// The GIS verbs' shared records, and any record as JSON (gis_records.hpp).

#include "gis_records.hpp"

#include <array>
#include <string>
#include <string_view>

#include "../import_records.hpp"
#include "katana/core/text.hpp"

namespace katana::app::geo {

namespace {

std::string yesNo(bool flag)
{
    return flag ? "yes" : "no";
}

} // namespace

const char* colorWord(katana::interop::PointColorMode mode)
{
    switch (mode) {
    case katana::interop::PointColorMode::Elevation:
        return "elevation";
    case katana::interop::PointColorMode::Intensity:
        return "intensity";
    case katana::interop::PointColorMode::Classification:
        return "classification";
    case katana::interop::PointColorMode::SourceColor:
        return "rgb";
    case katana::interop::PointColorMode::Flat:
        return "flat";
    }
    return "elevation";
}

std::string referenceRecord(const katana::interop::RasterOverlay& raster)
{
    return "reference id=" + std::to_string(raster.id) + " kind=raster name=" +
           value(raster.name) + " width=" + std::to_string(raster.width) +
           " height=" + std::to_string(raster.height) +
           " bounds=" + boundsText(raster.worldBounds()) +
           " georeferenced=" + yesNo(raster.hasGeotransform) + " visible=" + yesNo(raster.visible) +
           " opacity=" + recordNumber(raster.opacity) +
           " role=" + std::string(katana::interop::toString(raster.role)) +
           " file=" + value(pathText(raster.source));
}

std::string referenceRecord(const katana::interop::PointCloudLayer& cloud)
{
    return "reference id=" + std::to_string(cloud.id) + " kind=pointcloud name=" +
           value(cloud.name) + " points=" + std::to_string(cloud.points.size()) +
           " source_points=" + std::to_string(cloud.sourcePointCount) +
           " bounds=" + boundsText(cloud.worldBounds()) + " visible=" + yesNo(cloud.visible) +
           " color=" + std::string(colorWord(cloud.colorMode)) +
           " file=" + value(pathText(cloud.source));
}

std::string surfaceRecord(const katana::terrain::NamedSurface& surface)
{
    const katana::terrain::TinSurface& tin = *surface.surface;
    return "surface name=" + value(surface.name) +
           " triangles=" + std::to_string(tin.triangleCount()) +
           " points=" + std::to_string(tin.vertexCount()) + " bounds=" + boundsText(tin.bounds()) +
           " zmin=" + recordNumber(tin.minElevation()) + " zmax=" + recordNumber(tin.maxElevation()) +
           " source=" + value(surface.source);
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
        if (key == "bounds") {
            std::array<double, 4> corners{};
            std::size_t found = 0;
            std::string_view rest = text;
            while (found < corners.size()) {
                const std::size_t comma = rest.find(',');
                const auto number = katana::core::parseFiniteDouble(rest.substr(0, comma));
                if (!number) {
                    break;
                }
                corners[found++] = *number;
                if (comma == std::string_view::npos) {
                    break;
                }
                rest = rest.substr(comma + 1);
            }
            object[key] = found == corners.size() ? nlohmann::json(corners) : nlohmann::json(nullptr);
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
