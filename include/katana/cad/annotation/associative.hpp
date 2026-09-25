#pragma once

// The associative update: annotation that follows the geometry it refers to
// (docs/annotation.md, "Associativity").
//
// A dimension whose points name other entities (AnchorRef), a leader whose
// tip does, and a label (which always names its target) must follow those
// entities through every edit - a MOVE, a grip drag, an OFFSET's undo, a
// SET_GEOMETRY from an agent. Rather than teach every command about them,
// Document::execute wraps every command in withAssociativeUpdate: after the
// command runs, associativeChanges works out what the annotations that
// follow it now are, and applies that as a second change set INSIDE the same
// undo step. One undo puts back the geometry and everything that followed it,
// exactly (before-images, as every change set undoes).
//
// What follows what:
//   * a dimension's start, end and vertex follow the anchors they name; a
//     radial dimension whose vertex names a circle or an arc keeps its
//     direction from the centre and takes the curve's new radius;
//   * a leader's tip follows its anchor;
//   * a label's anchor follows its target (a dragged label's position moves
//     by as much, so it keeps its place beside the target), and a label
//     whose target is gone is REMOVED - a label goes with what it labels, and
//     comes back with it on undo;
//   * a reference to an entity that is gone is dropped, leaving the point
//     where it last was: the dimension stays and is no longer associative,
//     as a CAD user expects when a measured line is erased.
// An annotation on a locked layer is left as it is: the lock is the user
// saying not to change it.

#include "katana/commands/change_set.hpp"
#include "katana/commands/command.hpp"
#include "katana/core/error.hpp"
#include "katana/entity/model.hpp"

namespace katana::cad::annotation {

// What the annotations that follow other entities must become for the model
// as it is now; empty when every one already agrees. Costs one pass over the
// entities, and the resolution of each annotation that names another entity.
[[nodiscard]] katana::commands::ChangeSet associativeChanges(const katana::entity::Model& model);

// `command`, followed within the same undo step by the associative update
// of what it changed. Its name, destructiveness and created entities are the
// command's own. If the update cannot be applied the command is undone and
// the update's error returned, so the model is never left half followed.
[[nodiscard]] katana::commands::CommandPtr withAssociativeUpdate(katana::commands::CommandPtr command);

} // namespace katana::cad::annotation
