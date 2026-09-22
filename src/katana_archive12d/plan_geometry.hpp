#pragma once

// The plan shape of a 12d string: vertices joined by straights, arcs and
// transitions, turned into the straight chords a Polyline2 can hold. Internal
// to the module.
//
// THE MANUAL IS NOT ENOUGH TO DRAW A TRANSITION, AND IN ONE PLACE IT IS WRONG.
// Three conventions below were settled by measurement against 12d Model's own
// output (`test Super Alignment.12da`, written by 12d Model 15.0C1t, which
// holds 32 arcs and 16 transitions whose end points are all recorded):
//
//   1. A POSITIVE RADIUS TURNS RIGHT. The manual says "+ve is above the line
//      connecting the vertices", which does not say which way is up. All 18
//      arcs that follow a straight agree: 10 positive, all turning clockwise;
//      8 negative, all counter-clockwise.
//
//   2. A TRAILING TRANSITION IS DESCRIBED BACKWARDS. The manual defines l1, r1,
//      a1 as the values "at the start vertex" and says a full trailing
//      transition has r2 = 0 and l2 = 0. What 12d writes is l1 = 0, r1 = 0 for
//      trailing transitions too, with a1 the tangent at the segment's SECOND
//      vertex pointing back along the alignment: the block always runs from
//      the (l1, r1, a1) end to the (l2, r2, a2) end, and `leading` says
//      whether that is the direction of the string (1) or against it (0).
//      Read that way, all 16 transitions land on their recorded end points;
//      read the manual's way, every trailing one misses by 40 to 195 m.
//
//   3. "CUBIC PARABOLA" IS y = m x^3 IN THE FRAME OF THE TANGENT, with m chosen
//      so that the TRUE curvature at the end, 6mX / (1 + 9m^2 X^4)^(3/2), is
//      1/R and the ARC LENGTH to the end is L. Under that definition all 16
//      close to under 0.01 mm and reproduce a2 to the four decimals written.
//      The textbook form y = x^3 / (6RL), and a clothoid, both miss by up to
//      0.3 m on the same data - a railway transition of L = 80, R = 210.
//
// Angles are degrees counter-clockwise from +x.
//
// The other transition types (Westrail cubic, cubic spiral, Bloss, sinusoidal,
// cosinusoidal) are not defined by the manual and no sample holds one. They are
// drawn as the clothoid with the same end radii, then sheared so that the far
// end meets the vertex 12d recorded - both ends exact, the shape between them
// an approximation - and the size of that adjustment is reported, because it
// IS the error.

#include <cstddef>
#include <map>
#include <string>
#include <vector>

#include "katana/archive12d/archive.hpp"
#include "katana/geometry/primitives2d.hpp"

namespace katana::archive12d::detail {

struct PlanPoint {
    katana::geometry::Point2 point;
    // Which segment the point lies on and how far along it, 0 at the segment's
    // first vertex. A vertex of the string has fraction 0. This is what lets a
    // height be interpolated for a point that chording invented.
    std::size_t segment = 0;
    double fraction = 0.0;
};

struct ChordReport {
    std::size_t arcs = 0;
    std::size_t transitions = 0;
    // Segments drawn as a straight because their parameters made no sense: an
    // arc whose radius cannot span its chord by more than rounding, a
    // transition with no length or that missed its end by metres.
    std::size_t straightened = 0;
    // Transition type -> the largest distance an approximated transition had
    // to be moved to meet its recorded end.
    std::map<std::string, double> adjustmentByType;

    void merge(const ChordReport& other);
};

[[nodiscard]] std::vector<PlanPoint> chordPlan(const std::vector<Vertex>& vertices,
                                               const std::vector<Segment>& segments, bool closed,
                                               double tolerance, ChordReport& report);

// The circle an arc segment lies on, and the part of it the segment is.
struct ArcCircle {
    katana::geometry::Point2 centre;
    double radius = 0.0; // unsigned
    bool clockwise = false;
    double startAngle = 0.0;
    double sweep = 0.0; // signed: negative when clockwise
};

// False when the segment is not an arc between these two points: no radius, no
// chord, or a radius that cannot span the chord by more than `tolerance`.
[[nodiscard]] bool solveArc(const katana::geometry::Point2& a, const katana::geometry::Point2& b,
                            const Segment& segment, double tolerance, ArcCircle& arc);

// The direction of travel, radians counter-clockwise from +x, at `point` on the
// arc. EXACT - perpendicular to the radius - which a chord of the arc is not:
// a PI reconstructed from the chords of an arc drawn to a millionth of a unit
// still lands 9 mm out at R = 40, because a chord's direction is off by half
// the angle it subtends and the error is multiplied by the tangent length.
[[nodiscard]] double arcTangentAt(const ArcCircle& arc, const katana::geometry::Point2& point);

// The end of 12d's cubic parabola in the frame of its tangent: abscissa X,
// offset Y (unsigned) and the coefficient m, for end radius `radius` (> 0) and
// arc length `length`. False when no such curve exists - the true curvature of
// y = m x^3 peaks where the slope is 1/sqrt(2), so a transition much longer
// than 0.77 R cannot reach radius R.
struct CubicParabolaEnd {
    double abscissa = 0.0;
    double offset = 0.0;
    double coefficient = 0.0;
};
[[nodiscard]] bool solveCubicParabola(double radius, double length, CubicParabolaEnd& end);

} // namespace katana::archive12d::detail
