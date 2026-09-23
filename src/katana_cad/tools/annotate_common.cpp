#include "annotate_common.hpp"

#include <cmath>

#include "katana/cad/dimension_draw.hpp"
#include "katana/commands/change_set.hpp"
#include "katana/core/text.hpp"
#include "katana/math/numerics.hpp"

namespace katana::cad::tools::annotate {

using katana::core::Result;

bool isOption(std::string_view typed, std::string_view keyword, std::size_t shortest)
{
    const std::string_view word = katana::core::trimmed(typed);
    if (word.size() < shortest || word.size() > keyword.size()) {
        return false;
    }
    return katana::core::equalsIgnoringCase(word, keyword.substr(0, word.size()));
}

std::optional<double> typedNumber(std::string_view typed)
{
    return katana::core::parseFiniteDouble(katana::core::trimmed(typed));
}

bool coincident(const katana::geometry::Point2& a, const katana::geometry::Point2& b)
{
    return !(a.distanceTo(b) > katana::math::tolerance::kGeometric);
}

katana::entity::Entity newEntity(katana::entity::Geometry geometry,
                                 const katana::commands::EntityAttributes& attributes)
{
    katana::entity::Entity entity;
    entity.geometry = std::move(geometry);
    entity.layer = attributes.layer;
    entity.style = attributes.style;
    entity.color = attributes.color;
    return entity;
}

katana::commands::CommandPtr createAll(std::string name,
                                       std::vector<katana::entity::Entity> entities)
{
    // The builder copies rather than moves its entities out: validate() may
    // run it more than once (change_set.hpp), and a moved-from set would be
    // an empty command the second time.
    return std::make_unique<katana::commands::ChangeSetCommand>(
        std::move(name),
        [entities = std::move(entities)](
            const katana::commands::CommandContext&) -> Result<katana::commands::ChangeSet> {
            katana::commands::ChangeSet changes;
            changes.add = entities;
            return changes;
        });
}

katana::entity::DimensionStyle styleForNewAnnotation(const ToolContext& context)
{
    if (context.document == nullptr) {
        return katana::entity::DimensionStyle{};
    }
    // resolveDimensionStyle reads only the entity's layer, so an entity that
    // is not in the model yet resolves exactly as the one about to be made.
    katana::entity::Entity probe;
    probe.layer = context.attributes.layer;
    return resolveDimensionStyle(context.document->model(), probe);
}

double estimatedTextWidth(std::string_view text, double height)
{
    // A UTF-8 continuation byte is 10xxxxxx; every other byte starts a
    // character.
    std::size_t characters = 0;
    for (const char byte : text) {
        if ((static_cast<unsigned char>(byte) & 0xC0U) != 0x80U) {
            ++characters;
        }
    }
    constexpr double kAdvancePerCharacter = 0.6;
    return kAdvancePerCharacter * height * static_cast<double>(characters);
}

double lineSpacing(double height) { return height * 5.0 / 3.0; }

std::string formatNumber(double value)
{
    // Only where nine decimals are within a double's precision; a larger
    // value has no such noise to remove and would gain some from the scaling.
    constexpr double kScale = 1e9;
    const double rounded =
        std::abs(value) < 1e6 ? std::round(value * kScale) / kScale : value;
    // -0 would print as "-0".
    return katana::core::formatExactReal(rounded == 0.0 ? 0.0 : rounded);
}

} // namespace katana::cad::tools::annotate
