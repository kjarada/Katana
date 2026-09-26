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
//   Line       Start, End, Mid, Along
//   Arc        Start, End, Mid, Centre, Along
//   Circle     Centre, Along, Inside (its centre)
//   Polyline   Start, End, Vertex i, SegmentMid i (the closing segment of a
//              closed polyline is the last one), Along (segment i), Inside
//              (a closed one of three vertices or more)
//   Text       Position
//   Leader     Start (its tip), End (its last vertex)
//
// Along's parameter is clamped to [0, 1], so a reference is never resolved
// off the end of what it names; one that is not a number, or is infinite,
// reads as 0, the start - a stored reference is never refused for it.
[[nodiscard]] std::optional<katana::geometry::Point2> resolveAnchor(const Entity& entity,
                                                                    const AnchorRef& ref);

// The point of `entity` nearest `near`, NAMED so that it follows the entity:
// Along for a line, an arc, a circle, a polyline and a curve polyline (on a
// polyline, the segment it is nearest - the first of equals - and how far
// along it; on a curve polyline's arc segment, the fraction of its sweep),
// and an ellipse (the fraction of its sweep of eccentric anomaly), Position
// for a point and a text. nullopt for a dimension, a label or a leader,
// which offer no place to be attached to, for a spline (its parameter is not
// its length; its ends are Start and End), and for a polyline with no
// segment of any length. The reference names entity.id. What a leader's tip
// clicked or typed near a line becomes (docs/annotation.md, "Smart leaders").
[[nodiscard]] std::optional<AnchorRef> nearestAnchor(const Entity& entity,
                                                     const katana::geometry::Point2& near);

// A point inside a closed figure: the centroid when it is inside (every
// convex lot, most others), otherwise the middle of the widest run inside the
// figure along the horizontal through the centroid - an L-shaped lot's
// centroid can be in the notch. Where an area label and a leader's Inside
// tip go. Deterministic, and worked relative to the figure's first vertex as
// every area routine is (docs/architecture.md, "Numerical policy").
// `figure` is closed and has at least three vertices.
[[nodiscard]] katana::geometry::Point2 insidePoint(const katana::geometry::Polyline2& figure);

// "end", "vertex 3", "segment-mid 2", "along 1 0.25": what LIST and the
// command line say.
[[nodiscard]] std::string describe(const AnchorRef& ref);

} // namespace katana::entity
