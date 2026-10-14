#pragma once

// How the views show the road model's objects (cad/bim/bim_object.hpp;
// docs/road_design.md, "In the views"): which are drawn, and the 3D view's
// meshes of them.
//
// AN OBJECT FOLLOWS ITS LAYER. Its layer is a path ("road/earthworks/cut");
// the deepest part of it the drawing has decides - an object on
// "road/pavement" is hidden when "road" is switched off, even before
// "road/pavement" is made - and a path the drawing has none of is drawn,
// since an object is not an entity and is drawn in its own colour. CORRIDOR
// NEW makes the layers its objects are on (road/earthworks/cut switched
// off: a cut is the ground taken away, and drawn solid it would bury the
// road it was taken away for), so after that the Layers panel shows and
// hides them like anything else. A view that hides a layer of its own hides
// its objects there too.

#include <string>
#include <vector>

#include "katana/cad/bim/bim_object.hpp"
#include "katana/cad/layer_overrides.hpp"
#include "katana/cad/scene.hpp"
#include "katana/entity/model.hpp"
#include "katana/geometry/primitives2d.hpp"

namespace katana::cad::bim {

[[nodiscard]] bool isShown(const BimObject& object, const katana::entity::Model& model,
                           const LayerOverrides& view);

// A 3D view's mesh for each object shown: its name, a pointer to its mesh -
// so `objects` must outlive them - its colour, Shaded, and planFootprint
// off (the plan view draws the object's own outline, not its hull).
[[nodiscard]] std::vector<SceneMesh> sceneMeshes(const BimModel& objects,
                                                 const katana::entity::Model& model,
                                                 const LayerOverrides& view);

// The box round the plan outlines of the objects shown (and round their
// meshes for an object with no outline). Empty when none is shown.
[[nodiscard]] katana::geometry::Box2
planBounds(const BimModel& objects, const katana::entity::Model& model, const LayerOverrides& view);

} // namespace katana::cad::bim
