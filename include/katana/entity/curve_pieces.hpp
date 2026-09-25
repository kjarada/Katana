#pragma once

// A geometry as the primitive curves the editing kernel understands
// (geometry::Curve2: segments, arcs, circles), for the code that works on
// "the curves of an entity" without caring which kind it is - intersection
// and nearest snaps, section crossings, trim and extend boundaries, the
// surface builder's breaklines.
//
// It exists because docs/model.md lists those places as the ones where a new
// geometry kind is SILENTLY absent (a get_if chain that does not know it),
// and three kinds arrived at once (docs/drawing.md). Rather than teach each
// chain three more cases, each asks this, and a kind is added here once:
//
//   * a Polyline2's and a CurvePolyline2's segments and arcs, exactly;
//   * an Ellipse2 and a Spline2 as straight chords within `tolerance`, since
//     Curve2 has no ellipse or spline - an intersection with one is then
//     good to that tolerance, which the default (a millimetre) makes finer
//     than any drawing resolves;
//   * a segment, arc or circle as itself; everything else (points, text,
//     annotation) as nothing.

#include <vector>

#include "katana/entity/entity.hpp"
#include "katana/geometry/curves2d.hpp"
#include "katana/geometry/editing.hpp"

namespace katana::entity {

[[nodiscard]] std::vector<katana::geometry::Curve2>
curvePieces(const Geometry& geometry,
            double tolerance = katana::geometry::kCurveChordTolerance);

// True for the kinds whose pieces are exact (not chords).
[[nodiscard]] bool piecesAreExact(const Geometry& geometry);

// The points of a geometry as a walk, for code that wants a polyline: the
// vertices of a polyline, the chords of a curve polyline's arcs, of an
// ellipse and of a spline; empty for kinds that are not linework.
[[nodiscard]] std::vector<katana::geometry::Point2>
linework(const Geometry& geometry,
         double tolerance = katana::geometry::kCurveChordTolerance);

} // namespace katana::entity
