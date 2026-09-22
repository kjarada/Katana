#pragma once

// Grading: carrying the edge of a designed thing to the ground (PLAN.MD
// Phase 21, "feature lines, pads, slopes, daylight, grading surfaces").
//
// A FEATURE LINE is a string of 3D points - the edge of a pad, the crest of an
// embankment, the toe of a batter - at the elevations the design puts them.
// Grading it means, at every point along it, running a batter of the given
// slope outward until it meets the ground: the DAYLIGHT line. The pad (or
// the strip beside an open line) plus the batters between the feature line
// and the daylight line is the GRADING SURFACE, and the volume between that
// and the ground is the earthwork.
//
// The daylight search is the corridor's (corridor.cpp): a march outward in
// fixed steps until the batter and the ground change order, then a bisection
// to a millimetre. The ground is a TIN, piecewise planar, with no closed form
// for where a line meets it. What is new here is the DIRECTION of the march:
// perpendicular to the feature line along an edge, and along the bisector at
// a vertex - and at a vertex the batter is FLATTER in the direction of march
// by the cosine of half the corner, because the slope belongs to the edge's
// batter plane, not to the bisector. Without that a square pad on flat
// ground would daylight to a chamfered corner 29% short of where the two
// batter planes actually meet. Mitred corners are what a plane batter gives
// and what a machine builds; a rounded ("radial") corner is a later option.
//
// REJECTED: grading by offsetting the feature line in plan by a fixed width
// and reading the ground there. That is a bench, not a batter: the width to
// daylight depends on the ground, and on sloping ground it differs on every
// side of the same pad.

#include <cstddef>
#include <optional>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/geometry/primitives2d.hpp"
#include "katana/geometry/primitives3d.hpp"
#include "katana/terrain/tin_surface.hpp"

namespace katana::cad {

struct FeatureLine {
    std::vector<geometry::Point3> vertices;
    // Closed: a pad, graded outward all round. Open: graded on one side.
    bool closed = false;

    // A flat pad: the footprint at one elevation.
    [[nodiscard]] static FeatureLine pad(const geometry::Polyline2& footprint, double elevation);
};

enum class GradingSide { Left, Right }; // of an open feature line, looking along it

struct GradingSlopes {
    // Run per unit rise, as Assembly has them: 2.0 is 1 in 2 (1V:2H).
    double cutBatter = 2.0;
    double fillBatter = 3.0;
    // How far the batter may run before the search gives up on that sample.
    double maximumWidth = 50.0;
    // Spacing of the samples along an edge, in model units. Vertices are
    // always sampled; edges shorter than this get their ends only.
    double interval = 5.0;
    GradingSide side = GradingSide::Left; // open lines only
};

struct DaylightPoint {
    geometry::Point3 origin;   // on the feature line
    geometry::Point3 daylight; // on the ground
    bool cut = false;          // the ground was above the origin
};

struct Grading {
    // In order along the feature line. A sample whose batter never met the
    // ground is not here; it is counted below.
    std::vector<DaylightPoint> daylights;
    std::size_t samples = 0;
    std::size_t missed = 0; // no ground there, or none within maximumWidth
    // The daylight line as a plan polyline, closed for a closed feature line.
    geometry::Polyline2 daylightLine;
    // Feature line plus batters. Bounded by the daylight line when every sample
    // reached the ground; otherwise by the convex hull of what did, and
    // `boundedByDaylight` says which.
    terrain::TinSurface surface;
    bool boundedByDaylight = false;
    // Earthwork between the grading surface and the ground, from the exact
    // overlay in terrain::compareSurfaces. Cut and fill are both >= 0.
    double cut = 0.0;
    double fill = 0.0;
    double planArea = 0.0;
};

// Fails with InvalidArgument for a slope, width or interval that is not
// positive, a non-finite vertex, or a feature line with fewer than two
// vertices (three when closed); with InvalidGeometry when no sample reaches
// the ground, or the surface cannot be built. A self-intersecting daylight
// line (a sharp concave corner whose batters cross their neighbours') is not
// an error: the surface is built without it as a boundary and
// `boundedByDaylight` is false.
[[nodiscard]] core::Result<Grading> gradeToSurface(const FeatureLine& line,
                                                  const GradingSlopes& slopes,
                                                  const terrain::TinSurface& ground);

} // namespace katana::cad
