#pragma once

// The parts of an IFC export, each adding its objects through the Builder.
// Internal to katana_ifc; export.cpp calls them in this order.

#include <map>
#include <string>
#include <vector>

#include "builder.hpp"
#include "katana/entity/model.hpp"
#include "katana/ifc/export.hpp"
#include "katana/survey/subsurface/quality_level.hpp"
#include "katana/survey/subsurface/utility_network.hpp"

namespace katana::ifc::detail {

// Every alignment of the model: alignment.cpp.
void exportAlignments(Builder& builder, const entity::Model& model);

// The services of a subsurface utility investigation: utilities.cpp.
void exportUtilities(Builder& builder, const UtilityInput& utilities);

// The reference "QL-A" .. "QL-D" into AS 5488.1-2019, the classification
// written once whichever part asks first - the schedule's services
// (utilities.cpp) or a services plan drawn from one (drawing.cpp) - so that
// a file holding both classifies them in one standard.
Id qualityLevelReference(Builder& builder, survey::subsurface::QualityLevel level);

// A service's system LongName - its owner, when known, and its type
// ("WaterCo water") - the same from the schedule's export and a plan drawn
// from it.
std::string serviceLongName(const survey::subsurface::UtilityAttributes& service);

// The drawing's entities, by classification: drawing.cpp.
void exportEntities(Builder& builder, const entity::Model& model,
                    const std::vector<ClassificationRule>& rules);

// Surfaces, as terrain: surfaces.cpp. The element each surface was written
// as, by its name (the first of a name), for the road model's cuts to void.
std::map<std::string, Id> exportSurfaces(Builder& builder,
                                         const std::vector<SurfaceInput>& surfaces);

// The road model: road.cpp. `terrain` is what exportSurfaces wrote.
void exportRoad(Builder& builder, const RoadModelInput& road,
                const std::map<std::string, Id>& terrain);

} // namespace katana::ifc::detail
