#pragma once

// Purge: delete the styles, linetypes and hatch patterns nothing uses.
//
// What "uses" means is entity::tableUsage's answer - the one the delete
// guards and the managers' "Used" column read - so purge can never offer an
// item a delete would then refuse, nor keep one a manager shows as unused.
//
// The plan is iterated to a FIXPOINT: purging a style can free the linetype
// and hatch pattern only it named, and those go in the same purge rather
// than needing a second one. Protected items ("continuous", "none") are
// never in it: the model resolves to them when nothing else is said.

#include <string>
#include <vector>

#include "katana/commands/entity_commands.hpp"
#include "katana/core/error.hpp"
#include "katana/entity/model.hpp"

namespace katana::cad {

struct PurgeOptions {
    bool styles = true;
    bool linetypes = true;
    bool hatches = true;
    // Styles to keep although no entity wears them yet: the Document's
    // current style, which new work is about to be drawn in. What such a
    // style names is kept with it.
    std::vector<std::string> keepStyles{};
};

// The unused items, ascending within each table. Empty when nothing is.
[[nodiscard]] katana::commands::TableItems planPurge(const katana::entity::Model& model,
                                                     const PurgeOptions& options = {});

// ONE command deleting planPurge's items, so one undo restores them all; or
// nullptr when nothing is unused - never an empty undo step (the shape of
// audit QT-01). Fails with InvalidArgument when the options ask for nothing.
[[nodiscard]] katana::core::Result<katana::commands::CommandPtr>
purgeCommand(const katana::entity::Model& model, const PurgeOptions& options = {});

} // namespace katana::cad
