#include <algorithm>
#include <array>
#include <cstdint>

#include "katana/archive12d/domain.hpp"
#include "katana/entity/colour_names.hpp"
#include "katana/entity/layer_path.hpp"
#include "plan_geometry.hpp"
#include "text_utilities.hpp"

namespace katana::archive12d {

using katana::entity::Color;

std::string_view symbolForLinestyle(std::string_view name)
{
    return katana::entity::builtInSymbolFor(name);
}

// The standard colour names, their fold and their RGB are the entity layer's
// (entity/colour_names.hpp): a customisation's own colour table is resolved
// beside them there, by cad, which may not see this module. These three keep
// the names the archive import and export have always called them by.
std::vector<std::string> standardColourNames()
{
    return katana::entity::standardColourNames();
}

std::optional<Color> standardColour(std::string_view name)
{
    return katana::entity::standardColour(name);
}

std::string nearestStandardColour(const Color& colour)
{
    return katana::entity::nearestStandardColour(colour);
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
