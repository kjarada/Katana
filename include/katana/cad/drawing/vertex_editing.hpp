#pragma once

// Vertex editing of polyline ENTITIES: the layer between the pure arithmetic
// of geometry/polyline_vertices.hpp and everything that edits a drawing's
// polylines vertex by vertex - the grips, the Draw > Vertices tools, the
// Vertices panel and the VERTEX verbs (docs/drawing.md).
//
// A polyline entity is either kind (docs/drawing.md, "Which kind a polyline
// is"): a Polyline2 with its heights in the elevation properties, or a
// CurvePolyline2 holding bulges and heights itself. readPolyline() makes
// either one CurvePolyline2; writePolyline() puts an edited one back as the
// simplest kind that holds it, moving the heights to wherever that kind
// keeps them. So an edit never needs to know which kind it was given, and
// straightening the last arc of a string gives back the Polyline2 every
// other tool works on.
//
// Every command here is ONE ChangeSetCommand - one undo step - named by
// what it does (VERTEX_INSERT, WEED, ...), because the undo menu and the
// command log show the name.

#include <cstddef>
#include <functional>
#include <optional>
#include <string>
#include <utility>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/cad/layer_overrides.hpp"
#include "katana/commands/command.hpp"
#include "katana/core/error.hpp"
#include "katana/entity/entity.hpp"
#include "katana/geometry/polyline_vertices.hpp"

namespace katana::cad {

// True for the two polyline kinds.
[[nodiscard]] bool isPolylineEntity(const katana::entity::Entity& entity);

// The entity's polyline as one CurvePolyline2 (a Polyline2's heights read
// through entity::heightsOf); nullopt for any other kind.
[[nodiscard]] std::optional<katana::geometry::CurvePolyline2>
readPolyline(const katana::entity::Entity& entity);

// `entity` with `polyline` as its geometry, stored as the simplest kind:
// a Polyline2 (heights to the properties through entity::setHeights) when
// no segment is an arc, else a CurvePolyline2 (the elevation properties
// removed, since the geometry holds the heights). Fails with InvalidGeometry
// when the polyline is not one the model accepts.
[[nodiscard]] katana::core::Result<katana::entity::Entity>
writePolyline(katana::entity::Entity entity, const katana::geometry::CurvePolyline2& polyline);

// An edit of one polyline as a function of its vertices.
using PolylineEdit = std::function<katana::geometry::PolylineResult(
    const katana::geometry::CurvePolyline2&)>;

// ONE undoable command that applies `edit` to polyline `id`. Refused, with
// the edit's own sentence, when the entity is not a polyline or the edit
// fails; the drawing is untouched either way.
[[nodiscard]] katana::commands::CommandPtr editPolyline(katana::entity::EntityId id,
                                                        std::string name, PolylineEdit edit);

// The same edit on several polylines as ONE command (Weed on a selection).
[[nodiscard]] katana::commands::CommandPtr
editPolylines(std::vector<katana::entity::EntityId> ids, std::string name, PolylineEdit edit);

// An edit that depends on which polyline it is given (Weed keeping each
// one's own survey-point vertices), on several polylines as ONE command.
using PolylineEditOf = std::function<katana::geometry::PolylineResult(
    const katana::entity::Model&, katana::entity::EntityId,
    const katana::geometry::CurvePolyline2&)>;
[[nodiscard]] katana::commands::CommandPtr
editEachPolyline(std::vector<katana::entity::EntityId> ids, std::string name, PolylineEditOf edit);

// Which vertices of polyline `id` carry survey point data: a survey point (a
// point entity with the survey import's point-number property) lies on the
// vertex within `tolerance`. What Weed's "keep survey points" option keeps.
[[nodiscard]] std::vector<bool> verticesOnSurveyPoints(const katana::entity::Model& model,
                                                       const katana::geometry::CurvePolyline2& polyline,
                                                       double tolerance = 1.0e-4);
[[nodiscard]] std::vector<bool> verticesOnSurveyPoints(const Document& document,
                                                       const katana::geometry::CurvePolyline2& polyline,
                                                       double tolerance = 1.0e-4);

// The height the drawing has at `at`: a point's, a polyline's vertex's or a
// line's end's lying within `tolerance`, the nearest first, of what `view`
// DRAWS (selection.hpp, isDrawn: a locked layer's heights are still read);
// nullopt when nothing with a height is there. What a 3D polyline takes from
// a snapped point and Set Vertex Height from a click. The view has no default,
// as isDrawn's has none: a hidden point's height once went into a vertex the
// user could see, from a point they could not.
[[nodiscard]] std::optional<double> heightAtPoint(const Document& document,
                                                  const katana::geometry::Point2& at,
                                                  double tolerance, const LayerOverrides& view);

// The two vertices of a Straighten or a Grade in the order the arithmetic
// walks them (geometry::straighten and gradeBetween walk FORWARD from the
// first). An open polyline's lower first. A closed polyline's either way
// round: forward from `a` unless the other way takes in fewer vertices (a tie
// walks forward), and the other way round with `otherSide`. The one rule of
// the window's Straighten and Grade tools (their O is `otherSide`) and of
// the STRAIGHTEN and VERTEXZ GRADE verbs' side=short and side=long, so picks
// 3 then 1 of a hexagon and "STRAIGHTEN id 3 1 side=short" take out the same
// vertex. The verbs' default stays their forward walk from the first number.
[[nodiscard]] std::pair<std::size_t, std::size_t>
vertexRange(const katana::geometry::CurvePolyline2& polyline, std::size_t a, std::size_t b,
            bool otherSide);

// A vertex's height as the vertex tools' labels and the grips' hover hint
// write it: "z 101.500", or "no height". One writer, so a vertex never reads
// "z -0.000" in the band and "z 0.000" in a tool's label.
[[nodiscard]] std::string heightText(const std::optional<double>& height);

} // namespace katana::cad
