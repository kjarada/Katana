#pragma once

// The scope and filter of the ROADOBJECT verbs (docs/road_objects.md, "Scope
// and filter"): LIST, QUANTITIES and DELETE, the manager and the
// katana_road_objects tool act on the same choices as Global Modify
// (CLAUDE.md 1.1) - the selection, what a view shows, named layers, the whole
// drawing, an AREA - and the same "only those that match" filter, read by the
// one parser (cad/scope_verbs.hpp) and resolved here the way
// cad::matchEntities resolves it for entities, with the same pieces
// (cad/global_modify.hpp: the layer scope, the layer patterns, the property
// conditions; cad/selection.hpp: whether a layer is drawn).
//
// A road object is a DEFINITION kept in the drawing, not an entity, so each
// choice says what it means for one:
//
//   DRAWING / ALL       every object.
//   LAYERS a,b [ONLY]   the objects whose layer (layerOf) is one of those, or
//                       beneath one unless ONLY.
//   AREA x0,y0,x1,y1    the objects whose plan extent meets the box. The plan
//                       extent is the box round the x and y of the vertices of
//                       the solids the object is made of (planExtent), the
//                       same as the ones the plan and 3D views draw.
//   VIEW [id] [EXTENTS] what a view shows: the objects on a layer the drawing
//                       shows and the view does not hide, and (without
//                       EXTENTS) whose plan extent meets the view's visible
//                       area.
//   SELECTION           the objects placed ALONG a selected line - the object
//                       remembers the entity it was copied from (`source=#12`)
//                       - and only those. A road object is not itself
//                       selectable, and one placed along an alignment or at
//                       points has no entity to select, so the selection
//                       never takes it. *Rejected: the objects whose extent
//                       touches the selected entities' boxes*, which for one
//                       long diagonal line is every object in its box, and
//                       DELETE acts on the answer.
//   (no scope word)     the selection, as everywhere in the grammar.
//
// WHERE conditions read an object as follows. LAYER=pat[,pat] is the object's
// layer path; PROP=key[:pat] reads the keys ROADOBJECT SET writes, as the object
// has them (filterProperties) - id, kind, name, layer, colour, material,
// facility, placement, alignment, station, to, offset, spacing, heights,
// surface, crossfall, dz, points and source, then the kind's own keys with the
// value the object is made with (type, height, ...) - so the kind of an
// object is PROP=kind:barrier and a barrier of one type PROP=type:thriebeam,
// the key in any case and the value a pattern with * and ?; COLOUR=#RRGGBB is
// the colour the object is drawn in, and COLOUR=ByLayer an object with no
// colour= of its own; DRAWN leaves out an object on a layer the drawing hides
// or one that is not made. TYPE=, STYLE= and TEXT= are for entities, and are
// REFUSED for a road object, naming the word - never ignored.
//
// A scope that takes nothing is reported (matched=0), not refused. A Layers
// scope naming a layer the drawing lacks is refused, as it is for entities.

#include <cstddef>
#include <optional>
#include <string>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/cad/global_modify.hpp"
#include "katana/cad/road/road_objects.hpp"
#include "katana/cad/scope_verbs.hpp"
#include "katana/core/error.hpp"
#include "katana/entity/entity.hpp"
#include "katana/geometry/primitives2d.hpp"

namespace katana::cad::road {

// What a scope and a filter took of the drawing's road objects.
struct RoadObjectMatch {
    ResolvedScope resolved{};
    // Their ids, in the order the drawing keeps the objects.
    std::vector<std::string> ids{};
    // Objects the scope and the filter would take but for having no plan
    // extent, which an AREA, a VIEW and DRAWN ask for: one that is not made
    // (its alignment gone, its surface removed - ROADOBJECT PROBLEMS says why)
    // draws nothing, so it is nowhere.
    std::size_t unmade = 0;

    // The record that leads a reply: cad::scopeRecord(resolved, ids.size()),
    // then unmade= when there are any.
    [[nodiscard]] std::string record() const;
};

// The road objects `words` take. Fails with Unsupported (the reason) for a
// record of road objects that cannot be read, NotFound for a Layers scope
// naming a layer the drawing lacks, InvalidState for a VIEW with no window
// (`views` answers it, as for entities) and InvalidArgument naming the word for
// a TYPE=, STYLE= or TEXT= condition.
[[nodiscard]] katana::core::Result<RoadObjectMatch>
matchRoadObjects(const Document& document, const ScopeWords& words,
                 const ScopeViewProvider& views = {});

// Why `filter` cannot be put to road objects, or ok. TYPE, STYLE and TEXT are
// about entities (see the top of this file).
[[nodiscard]] katana::core::Status checkFilterFits(const ModifyFilter& filter);

// The box round the x and y of the vertices of the solids `object` is made of,
// as it is made now; nullopt when it is not made.
[[nodiscard]] std::optional<katana::geometry::Box2> planExtent(const PlacementContext& context,
                                                               const RoadObject& object);

// What WHERE PROP= reads of `object`, key by key (see the top of this file).
[[nodiscard]] katana::entity::PropertyMap filterProperties(const RoadObject& object);

} // namespace katana::cad::road
