#include "katana/cad/purge.hpp"

#include <algorithm>
#include <string_view>

#include "katana/entity/table_usage.hpp"

namespace katana::cad {

using katana::core::ErrorCode;
using katana::core::makeError;

namespace {

bool listed(const std::vector<std::string>& sorted, std::string_view name)
{
    return std::binary_search(sorted.begin(), sorted.end(), name, std::less<>{});
}

void insertSorted(std::vector<std::string>& sorted, const std::string& name)
{
    sorted.insert(std::lower_bound(sorted.begin(), sorted.end(), name), name);
}

} // namespace

katana::commands::TableItems planPurge(const katana::entity::Model& model,
                                       const PurgeOptions& options)
{
    const katana::entity::TableUsage usage = katana::entity::tableUsage(model);
    katana::commands::TableItems plan;

    // Free once the plan so far is gone: named by no layer, by no style
    // that is staying, and reached by no entity except through what is
    // going. Entities reach a linetype or hatch pattern only through a layer
    // or a style, and a style is only planned once no entity wears it, so
    // "no layer and no staying style" is the whole test for those two.
    const auto freeOnceThePlanIsGone = [&](const katana::entity::Users& users) {
        return users.layers.empty() &&
               std::ranges::all_of(users.styles, [&](const std::string& style) {
                   return listed(plan.styles, style);
               });
    };

    // Rounds until one adds nothing. Each kind is looked at every round, so
    // the order of the kinds below decides nothing, and an item freed by
    // one purged in the same round is found in the next.
    for (bool added = true; added;) {
        added = false;
        if (options.styles) {
            for (const auto& [name, users] : usage.styles) {
                if (!model.styles.contains(name) || listed(plan.styles, name) ||
                    std::ranges::find(options.keepStyles, name) != options.keepStyles.end()) {
                    continue;
                }
                // A style is held by the entities wearing it and nothing
                // else: no table names a style.
                if (users.entities == 0 && freeOnceThePlanIsGone(users)) {
                    insertSorted(plan.styles, name);
                    added = true;
                }
            }
        }
        if (options.linetypes) {
            for (const auto& [name, users] : usage.linetypes) {
                if (!model.linetypes.contains(name) ||
                    katana::entity::LinetypePolicy::isProtected(name) ||
                    listed(plan.linetypes, name)) {
                    continue;
                }
                if (freeOnceThePlanIsGone(users)) {
                    insertSorted(plan.linetypes, name);
                    added = true;
                }
            }
        }
        if (options.hatches) {
            for (const auto& [name, users] : usage.hatchPatterns) {
                if (!model.hatchPatterns.contains(name) ||
                    katana::entity::HatchPatternPolicy::isProtected(name) ||
                    listed(plan.hatchPatterns, name)) {
                    continue;
                }
                if (freeOnceThePlanIsGone(users)) {
                    insertSorted(plan.hatchPatterns, name);
                    added = true;
                }
            }
        }
    }
    return plan;
}

katana::core::Result<katana::commands::CommandPtr> purgeCommand(const katana::entity::Model& model,
                                                               const PurgeOptions& options)
{
    if (!options.styles && !options.linetypes && !options.hatches) {
        return makeError(ErrorCode::InvalidArgument,
                         "a purge has to be asked for styles, linetypes or hatch patterns");
    }
    katana::commands::TableItems plan = planPurge(model, options);
    if (plan.empty()) {
        return katana::commands::CommandPtr{};
    }
    return katana::commands::purgeTableItems(std::move(plan));
}

} // namespace katana::cad
