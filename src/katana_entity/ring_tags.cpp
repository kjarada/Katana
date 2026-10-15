// Rings that are holes (entity/ring_tags.hpp).

#include "katana/entity/ring_tags.hpp"

#include <algorithm>
#include <limits>
#include <utility>

#include "katana/core/text.hpp"

namespace katana::entity {

namespace {

// A property or a metadata value, as text: IMPORT writes source.ring into an
// entity's metadata, a result writes gis.ring into its properties.
std::optional<std::string> tagOf(const Entity& entity, const std::string& key)
{
    if (const auto found = entity.properties.find(key); found != entity.properties.end()) {
        return toString(found->second);
    }
    if (const auto found = entity.metadata.find(key); found != entity.metadata.end()) {
        return toString(found->second);
    }
    return std::nullopt;
}

} // namespace

bool isTaggedHole(const Entity& entity)
{
    const auto isHole = [](const std::optional<std::string>& tag) {
        return tag && katana::core::lowered(*tag) == "hole";
    };
    return isHole(tagOf(entity, "gis.ring")) || isHole(tagOf(entity, "source.ring"));
}

std::optional<std::string> ringPart(const Entity& entity)
{
    if (auto part = tagOf(entity, "gis.part")) {
        return part;
    }
    return tagOf(entity, "source.part");
}

HoleJoiner::HoleJoiner(std::vector<ExteriorCandidate> exteriors) : exteriors_(std::move(exteriors))
{
    areas_.reserve(exteriors_.size());
    for (const ExteriorCandidate& exterior : exteriors_) {
        areas_.push_back(exterior.ring.area());
    }
}

std::optional<std::size_t>
HoleJoiner::exteriorOf(const std::vector<katana::geometry::Point2>& hole, EntityId holeId,
                       const std::optional<std::string>& holePart) const
{
    // Made before the hole, and how long before: the nearer the better.
    const auto before = [holeId](EntityId id) {
        return id < holeId ? holeId - id : std::numeric_limits<EntityId>::max();
    };
    std::optional<std::size_t> best;
    bool bestSharesPart = false;
    for (std::size_t a = 0; a < exteriors_.size(); ++a) {
        const bool inside = std::ranges::all_of(
            hole, [&](const katana::geometry::Point2& point) { return exteriors_[a].ring.contains(point); });
        if (!inside) {
            continue;
        }
        const bool sharesPart = holePart && exteriors_[a].part == holePart;
        const bool better =
            !best || (sharesPart && !bestSharesPart) ||
            (sharesPart == bestSharesPart &&
             (areas_[a] < areas_[*best] ||
              (areas_[a] == areas_[*best] &&
               before(exteriors_[a].id) < before(exteriors_[*best].id))));
        if (better) {
            best = a;
            bestSharesPart = sharesPart;
        }
    }
    return best;
}

} // namespace katana::entity
