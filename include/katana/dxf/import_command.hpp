#pragma once

// A DXF import as the ONE undoable step both front ends execute, built here
// so that the window and the command line cannot differ about it.

#include "katana/commands/command.hpp"
#include "katana/dxf/reader.hpp"
#include "katana/entity/model.hpp"

namespace katana::dxf {

// The linetypes the model lacks, then the layers it lacks (parents before
// children), then the entities - moved out of `imported` - and last the lock
// put back on each new layer the file locked: a locked layer refuses new
// entities, so it is created open and locked once they are on it, all within
// the one step that undo takes back whole. A layer the model already has
// locked - the entities' own, or a parent of it - is opened first and locked
// again last in the same way, so that the same file imported twice, or two
// sheets that share a locked layer, both come in.
//
// Null when there is nothing to add: every layer and linetype is already in
// the model and the file drew nothing.
[[nodiscard]] katana::commands::CommandPtr importCommand(DxfImport& imported,
                                                         const katana::entity::Model& model);

} // namespace katana::dxf
