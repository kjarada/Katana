#include "katana/cad/annotation/leader_build.hpp"

#include <algorithm>
#include <charconv>
#include <variant>

#include "katana/entity/entity.hpp"

namespace katana::cad::annotation {

std::string nextBalloonNumber(const katana::entity::Model& model)
{
    long long highest = 0;
    model.entities.forEach([&](const katana::entity::Entity& entity) {
        const auto* leader = std::get_if<katana::entity::LeaderGeometry>(&entity.geometry);
        if (leader == nullptr || leader->callout != katana::entity::CalloutShape::Circle) {
            return;
        }
        long long value = 0;
        const char* end = leader->text.data() + leader->text.size();
        const auto [parsed, error] = std::from_chars(leader->text.data(), end, value);
        if (error == std::errc{} && parsed == end) {
            highest = std::max(highest, value);
        }
    });
    return std::to_string(highest + 1);
}

} // namespace katana::cad::annotation
