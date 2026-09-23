#pragma once

// Who uses each named thing in a model: every style, linetype name, symbol
// name, hatch pattern name and layer, and the layers, styles and entities
// that reach it - found in ONE pass over the entities.
//
// One function serves three readers that must not disagree:
//
//   * the delete guards (commands' TablePolicy<T>::inUse): an item still
//     used cannot be deleted, and the refusal names how many and who first;
//   * purge (cad::planPurge): what is used by nothing can go;
//   * the managers: a "Used" column, "select the entities using this", and
//     the names used that resolve to nothing.
//
// "Reaches" is resolveDisplay's chain, read through the same two functions
// resolveDisplay reads (resolvedLinetype, resolvedHatchPattern), so an entity
// is counted against exactly the linetype it is drawn with: its style's, or
// its layer's when it has no style, a missing one, or one whose linetype is
// ByLayer. A per-row scan would be one pass per row - 800 styles over 250 000
// entities - which is why this is one pass for everything.
//
// Names are kept whether or not a table defines them: a 12d linestyle a
// style names lives in the library, not the model, and a name nothing
// defines is exactly what a manager has to show. The caller asks the tables
// (or the library) which is which.

#include <cstddef>
#include <map>
#include <string>
#include <string_view>
#include <vector>

#include "katana/entity/model.hpp"

namespace katana::entity {

struct UsageOptions {
    // Collect the entities' ids as well as counting them. Off by default: a
    // count is all a column or a guard needs, and a vector per name over a
    // quarter of a million entities is not free.
    bool entityIds = false;
};

// Everything that reaches one name.
struct Users {
    std::vector<std::string> layers{}; // naming it directly, ascending
    std::vector<std::string> styles{}; // naming it directly, ascending
    // The entities reaching it: for a style, the ones wearing it; for a
    // layer, the ones on it (not its subtree); for a linetype, symbol or
    // hatch pattern, the ones resolveDisplay resolves to it.
    std::size_t entities = 0;
    EntityId firstEntity = kInvalidEntityId; // the lowest of their ids
    std::vector<EntityId> entityIds{};       // ascending; UsageOptions::entityIds only

    // Named by a layer or a style. What holds a linetype or hatch pattern in
    // place: an entity reaches one only through a layer or a style.
    [[nodiscard]] bool named() const { return !layers.empty() || !styles.empty(); }
    [[nodiscard]] bool used() const { return named() || entities > 0; }
    // "used by 2 layers, 1 style and 418 entities, e.g. layer=survey" - the
    // count and the first holder, for a refusal a person can act on. Layers
    // come first because a layer is what a person fixes first; an entity id
    // is the last resort. Empty when nothing uses it.
    [[nodiscard]] std::string describe() const;

    friend bool operator==(const Users&, const Users&) = default;
};

using UsageMap = std::map<std::string, Users, std::less<>>;

struct TableUsage {
    // Every style in the table, and every name an entity wears that is not.
    UsageMap styles{};
    // Every model linetype, and every other name a layer or style gives as
    // its linetype - a library linestyle, or a name nothing defines. ByLayer
    // is not a name: an entity reaching a linetype through it is counted
    // under its layer's linetype, which is the one it is drawn with.
    UsageMap linetypes{};
    // Every non-empty Style::symbol.
    UsageMap symbols{};
    // Every hatch pattern in the table, and every other name.
    UsageMap hatchPatterns{};
    // Every layer, and every name an entity is on that is not a layer.
    UsageMap layers{};

    // The users of one name; an empty Users for a name nothing uses.
    [[nodiscard]] static const Users& of(const UsageMap& map, std::string_view name);
};

[[nodiscard]] TableUsage tableUsage(const Model& model, UsageOptions options = {});

} // namespace katana::entity
