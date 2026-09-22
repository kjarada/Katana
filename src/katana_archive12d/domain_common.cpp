#include <algorithm>
#include <array>
#include <cstdint>

#include "katana/archive12d/domain.hpp"
#include "katana/entity/layer_path.hpp"
#include "plan_geometry.hpp"
#include "text_utilities.hpp"

namespace katana::archive12d {

namespace {

using katana::entity::Color;

struct NamedColour {
    std::string_view name;
    Color colour;
};

// A 12da carries colour NAMES; the RGB behind a name lives in the 12d project
// (colours.4d) and can be redefined there, so no table here can be "right".
// These are the names 12d Model ships with, at the RGB the same names have in
// the X11 / CSS colour list - the nearest thing to a public definition of what
// "orange" or "brown" means. Two departures: X11's DarkGray (169) is LIGHTER
// than its Gray (128), an accident of history that would draw "dark grey"
// paler than "grey", so dark and light grey are set either side of grey
// instead; and "green" is X11's Lime (0, 255, 0), the pure primary every CAD
// palette means by it, not X11's half-bright Green.
constexpr std::array<NamedColour, 27> kStandardColours = {{
    {"red", {255, 0, 0, 255}},
    {"green", {0, 255, 0, 255}},
    {"blue", {0, 0, 255, 255}},
    {"yellow", {255, 255, 0, 255}},
    {"cyan", {0, 255, 255, 255}},
    {"magenta", {255, 0, 255, 255}},
    {"white", {255, 255, 255, 255}},
    {"black", {0, 0, 0, 255}},
    {"grey", {128, 128, 128, 255}},
    {"orange", {255, 165, 0, 255}},
    {"brown", {165, 42, 42, 255}},
    {"purple", {128, 0, 128, 255}},
    {"pink", {255, 192, 203, 255}},
    {"violet", {238, 130, 238, 255}},
    {"dark red", {139, 0, 0, 255}},
    {"dark green", {0, 100, 0, 255}},
    {"dark blue", {0, 0, 139, 255}},
    {"dark cyan", {0, 139, 139, 255}},
    {"dark magenta", {139, 0, 139, 255}},
    {"dark orange", {255, 140, 0, 255}},
    {"dark grey", {64, 64, 64, 255}},
    {"light grey", {192, 192, 192, 255}},
    {"light blue", {173, 216, 230, 255}},
    {"light green", {144, 238, 144, 255}},
    {"light cyan", {224, 255, 255, 255}},
    {"light yellow", {255, 255, 224, 255}},
    {"light pink", {255, 182, 193, 255}},
}};

} // namespace

std::string_view symbolForLinestyle(std::string_view name)
{
    const std::string key = detail::lowered(name);
    const auto has = [&key](std::string_view word) { return key.find(word) != std::string::npos; };
    // Most specific first: "Pole - Light" is a pole, "Suspended Light" a
    // light, "Gully Pit Point" a pit and not a point.
    if (has("tree") || has("shrub") || has("palm")) {
        return "tree";
    }
    if (has("manhole") || has("pit") || has("chamber") || has("sump")) {
        return "manhole";
    }
    if (has("pole") || has("post") || has("column") || has("pier")) {
        return "pole";
    }
    if (has("mark") || has("station") || has("bench") || has("stns") || has("control")) {
        return "target";
    }
    if (has("sign") || has("flag")) {
        return "flag";
    }
    if (has("light") || has("lamp") || has("lantern")) {
        return "star";
    }
    if (has("valve") || has("hydrant") || has("tap") || has("meter")) {
        return "diamond";
    }
    if (has("bollard") || has("peg") || has("nail") || has("spike")) {
        return "dot";
    }
    if (has("point") || has("spot") || has("surface") || has("level") || has("invert") ||
        has("obvert")) {
        return "cross";
    }
    return "circle";
}

std::optional<Color> standardColour(std::string_view name)
{
    std::string key = detail::lowered(detail::trimmed(name));
    std::replace(key.begin(), key.end(), '_', ' ');
    std::replace(key.begin(), key.end(), '-', ' ');
    if (key == "gray") {
        key = "grey";
    } else if (key.ends_with(" gray")) {
        key.replace(key.size() - 4, 4, "grey");
    }
    for (const NamedColour& entry : kStandardColours) {
        if (entry.name == key) {
            return entry.colour;
        }
    }
    return std::nullopt;
}

std::string nearestStandardColour(const Color& colour)
{
    std::string_view best = kStandardColours.front().name;
    long bestDistance = -1;
    for (const NamedColour& entry : kStandardColours) {
        const long dr = static_cast<long>(entry.colour.r) - colour.r;
        const long dg = static_cast<long>(entry.colour.g) - colour.g;
        const long db = static_cast<long>(entry.colour.b) - colour.b;
        const long distance = dr * dr + dg * dg + db * db;
        if (bestDistance < 0 || distance < bestDistance) {
            bestDistance = distance;
            best = entry.name;
        }
    }
    return std::string(best);
}

std::string layerPathForModel(std::string_view modelName, std::string_view prefix)
{
    std::vector<std::string> segments;
    std::size_t start = 0;
    while (start <= modelName.size()) {
        std::size_t end = modelName.find(katana::entity::kLayerSeparator, start);
        if (end == std::string_view::npos) {
            end = modelName.size();
        }
        std::string segment(detail::trimmed(modelName.substr(start, end - start)));
        for (char& ch : segment) {
            // A backslash is how a Windows user types a tree; as a layer name
            // it would read as a path separator wherever the name reaches a
            // file system.
            if (static_cast<unsigned char>(ch) < 0x20 || ch == 0x7F || ch == '\\') {
                ch = '_';
            }
        }
        if (segment == "." || segment == "..") {
            segment = "_";
        }
        if (!segment.empty()) {
            segments.push_back(std::move(segment));
        }
        start = end + 1;
    }

    std::string path(prefix);
    const std::size_t prefixDepth = path.empty() ? 0 : katana::entity::layerDepth(path);
    for (std::size_t i = 0; i < segments.size(); ++i) {
        // Levels past the depth limit are folded into the last one that fits,
        // so a deep tree loses nesting rather than the names in it.
        const bool room = prefixDepth + i + 1 <= katana::entity::kMaximumLayerDepth;
        if (room || path.empty()) {
            path = katana::entity::joinLayerPath(path, segments[i]);
        } else {
            path += '_';
            path += segments[i];
        }
    }
    if (path.empty()) {
        path = "12d";
    }
    if (path.size() > katana::entity::kMaximumLayerNameLength) {
        path.resize(katana::entity::kMaximumLayerNameLength);
        // Cutting mid-way through a multi-byte character would leave invalid
        // UTF-8: find the last lead byte and drop the character if the cut
        // took any of its continuation bytes.
        std::size_t lead = path.size();
        while (lead > 0 && (static_cast<unsigned char>(path[lead - 1]) & 0xC0) == 0x80) {
            --lead;
        }
        if (lead > 0 && static_cast<unsigned char>(path[lead - 1]) >= 0xC0) {
            const auto byte = static_cast<unsigned char>(path[lead - 1]);
            const std::size_t continuation = byte >= 0xF0 ? 3 : byte >= 0xE0 ? 2 : 1;
            if (path.size() - lead < continuation) {
                path.resize(lead - 1);
            }
        }
        // ... and cutting at a separator or a blank would leave an empty level.
        while (!path.empty() &&
               (path.back() == katana::entity::kLayerSeparator || path.back() == ' ')) {
            path.pop_back();
        }
    }
    if (!katana::entity::validateLayerPath(path)) {
        return "12d";
    }
    return path;
}

std::vector<katana::geometry::Point2> chordedPlan(const std::vector<Vertex>& vertices,
                                                  const std::vector<Segment>& segments,
                                                  bool closed, double tolerance)
{
    detail::ChordReport report;
    std::vector<katana::geometry::Point2> points;
    for (const detail::PlanPoint& point :
         detail::chordPlan(vertices, segments, closed, tolerance, report)) {
        points.push_back(point.point);
    }
    return points;
}

} // namespace katana::archive12d
