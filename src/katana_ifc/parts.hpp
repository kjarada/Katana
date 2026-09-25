#pragma once

// The parts of an IFC export, each adding its objects through the Builder.
// Internal to katana_ifc; export.cpp calls them in this order.

#include <vector>

#include "builder.hpp"
#include "katana/entity/model.hpp"
#include "katana/ifc/export.hpp"

namespace katana::ifc::detail {

// Every alignment of the model: alignment.cpp.
void exportAlignments(Builder& builder, const entity::Model& model);

// The services of a subsurface utility investigation: utilities.cpp.
void exportUtilities(Builder& builder, const UtilityInput& utilities);

// The drawing's entities, by classification: drawing.cpp.
void exportEntities(Builder& builder, const entity::Model& model,
                    const std::vector<ClassificationRule>& rules);

// Surfaces, as terrain: surfaces.cpp.
void exportSurfaces(Builder& builder, const std::vector<SurfaceInput>& surfaces);

} // namespace katana::ifc::detail
