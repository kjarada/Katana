#pragma once

// Resolving an AnchorRef (entity.hpp): the point on another entity that a
// dimension, a leader or a label follows. One function, because the
// associative update (cad/annotation/associative.hpp), the command line's
// "#12.end" points and the label values all ask the same question and must
// get the same answer.

#include <optional>

#include "katana/entity/entity.hpp"
#include "katana/geometry/primitives2d.hpp"

namespace katana::entity {

// The point `ref` names on `entity` (whose id is not checked against
// ref.entity), or nullopt when that kind of entity offers no such point - a
// circle has no start, a line no vertex 7 - so a dimension keeps its last
// point rather than jumping to an invented one.
//
//   Point      Position
//   Line       Start, End, Mid
//   Arc        Start, End, Mid, Centre
//   Circle     Centre
//   Polyline   Start, End, Vertex i, SegmentMid i (the closing segment of a
//              closed polyline is the last one)
//   Text       Position
//   Leader     Start (its tip), End (its last vertex)
[[nodiscard]] std::optional<katana::geometry::Point2> resolveAnchor(const Entity& entity,
                                                                    const AnchorRef& ref);

// "end", "vertex 3", "segment-mid 2": what LIST and the command line say.
[[nodiscard]] std::string describe(const AnchorRef& ref);

} // namespace katana::entity
