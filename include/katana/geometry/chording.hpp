#pragma once

// Approximating curves by straight chords to a stated accuracy.
//
// One rule, used everywhere a curve becomes a polyline: the 3D scene builder,
// the 2D viewport, the alignment that chords itself for a section cut, and
// the spiral that reports how many points drawing it will take. It lived in
// `cad::scene` first, and the spiral then grew its own copy of the arithmetic
// - which is the second way of doing something that CLAUDE.md section 1 says
// is a defect. It is here, in the layer both can see, so there is one.
//
// The rule is the sagitta: a chord subtending angle phi on radius r departs
// from the arc by at most r * (1 - cos(phi / 2)). Holding that at the
// tolerance gives phi = 2 * acos(1 - tolerance / r), and the chord count
// follows from the sweep.

#include <cstddef>
#include <vector>

#include "katana/geometry/primitives2d.hpp"

namespace katana::geometry {

// Chords needed for an arc of `radius` sweeping `sweepRadians` (sign ignored)
// so that no chord departs from the arc by more than `tolerance`.
//
// Never fewer than one. Capped at 8192, so that a hairline tolerance on a
// large radius cannot ask for a million chords: at that cap on a full circle
// each chord subtends 0.044 degrees, which is beyond what any screen or
// plotter resolves. The tolerance-to-radius ratio is floored at 1e-12 - a
// tolerance of zero would ask for infinitely many - and capped at 0.5, since
// a tolerance at or beyond the radius would ask for acos of a value <= 0.
[[nodiscard]] std::size_t sagittaChordCount(double radius, double sweepRadians,
                                            double tolerance);

// The arc as a polyline of chords, first point to last, within `tolerance`.
// An arc with no radius yields nothing; one with no sweep yields its two
// ends.
[[nodiscard]] std::vector<Point2> chordArc(const Arc2& arc, double tolerance);

// The circle as a closed ring of chords: the last point does NOT repeat the
// first, so the result is a `Polyline2` with `closed = true` waiting to
// happen.
[[nodiscard]] std::vector<Point2> chordCircle(const Circle2& circle, double tolerance);

} // namespace katana::geometry
