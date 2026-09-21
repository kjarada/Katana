#pragma once

// Corridor cross sections and earthwork quantities along an alignment
// (PLAN.MD Phase 21).
//
// A corridor is what a horizontal alignment, a design profile and a template
// make together: at every station the template is set out across the
// centreline at the design elevation, its outer edges are carried to the
// ground by batter slopes, and the area between the finished shape and the
// ground is what has to be dug out or filled in. Summed along the alignment
// by the end-area method, that is the earthwork quantity - the number an
// earthworks estimate, a tender and a claim are all built on.
//
// THE TEMPLATE IS DELIBERATELY SIMPLE. One carriageway of `halfWidth` each
// side of the centreline, falling away at `crossfall`, with a cut batter and
// a fill batter to daylight. Real assemblies carry kerbs, verges, benches and
// subgrades; those are a later need and belong in a table of their own, not
// in four more fields here. What this one already answers is the quantity to
// within the accuracy of the ground model, which is the accuracy anyone has.
//
// AVERAGE END AREA, and why. The volume between two stations is taken as the
// mean of their cross-section areas times the distance between them. That is
// the method every earthworks specification and every measurement standard
// names (it is the basis of payment in most road contracts), so it is the one
// a quantity from this software can be compared with. The prismoidal
// correction is more accurate where areas change quickly and is not applied;
// a caller wanting it shortens the interval, which is what practice does.
//
// GROUND IS QUERIED, NOT SECTIONED. Each cross section asks the surface for
// its elevation at a set of offsets directly, rather than cutting a section
// with extractSection and then reading it. A section carries breaks and
// crossings this does not need, and the daylight search wants the ground at
// arbitrary offsets rather than at a fixed set of samples.

#include <cstddef>
#include <optional>
#include <string>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/geometry/alignment.hpp"
#include "katana/geometry/primitives2d.hpp"
#include "katana/geometry/profile.hpp"
#include "katana/terrain/tin_surface.hpp"

namespace katana::cad {

struct Assembly {
    double halfWidth = 3.5;   // metres from the centreline to each edge
    double crossfall = 0.025; // rise/run, positive falls AWAY from the centreline
    // Batter slopes as horizontal run per unit of vertical rise: 2.0 is "2:1",
    // two metres out for every metre up or down. Cut batters are usually
    // steeper than fill ones, which is why there are two.
    double cutBatter = 1.5;
    double fillBatter = 2.0;
    // How far beyond the edge the daylight search will look before giving up.
    // A batter that has not met the ground within this is reported as a
    // station with no daylight rather than extended indefinitely.
    double maximumBatterWidth = 50.0;
};

// One vertex of a cross section: offset from the centreline (positive left,
// looking along increasing station) and elevation.
struct SectionVertex {
    double offset = 0.0;
    double elevation = 0.0;
};

struct CorridorSection {
    double station = 0.0;
    geometry::Point2 centre{};     // world position of the centreline
    double designElevation = 0.0;  // from the profile
    // The finished shape from left daylight to right daylight, and the ground
    // it sits on over the same offsets. Both left to right.
    std::vector<SectionVertex> design;
    std::vector<SectionVertex> ground;
    double cutArea = 0.0;  // ground above design, square metres (>= 0)
    double fillArea = 0.0; // design above ground (>= 0)
    // False when a batter failed to meet the ground within
    // Assembly::maximumBatterWidth, or the ground has no elevation at an
    // offset the section needs. Such a station contributes no area, and the
    // count of them is reported so that a quantity with holes in it says so.
    bool complete = true;
};

struct CorridorQuantities {
    std::vector<CorridorSection> sections; // ascending station
    double cut = 0.0;  // cubic metres
    double fill = 0.0;
    double net = 0.0;  // fill - cut: positive means material must be brought in
    std::size_t incompleteSections = 0;
    // The station range actually quantified: the overlap of the alignment, the
    // profile and the requested range.
    double startStation = 0.0;
    double endStation = 0.0;
};

// The cross section at one station. Fails with InvalidArgument when the
// station is outside the alignment or the profile, or the assembly is not
// sensible (non-positive width or batters, non-finite anything).
[[nodiscard]] core::Result<CorridorSection>
corridorSection(const geometry::SolvedAlignment& alignment, const geometry::SolvedProfile& profile,
                const Assembly& assembly, const terrain::TinSurface& ground, double station);

// Sections at `interval` along the part of the alignment the profile covers,
// always including the profile's key stations and both ends of the range, and
// the end-area volumes between them. Fails with InvalidArgument for a
// non-positive interval or an assembly that corridorSection rejects, and with
// InvalidGeometry when the alignment and the profile do not overlap at all.
[[nodiscard]] core::Result<CorridorQuantities>
corridorQuantities(const geometry::SolvedAlignment& alignment,
                   const geometry::SolvedProfile& profile, const Assembly& assembly,
                   const terrain::TinSurface& ground, double interval);

// The area between two piecewise-linear lines over a common offset range,
// split into the part where `upper` is above `lower` and the part where it is
// below. Exposed because it is where the arithmetic is, and worth testing on
// its own: both lines are sampled at the union of their vertices and at every
// crossing, so each piece is a trapezoid of one sign. Returns {above, below}.
struct SplitArea {
    double above = 0.0;
    double below = 0.0;
};
[[nodiscard]] SplitArea areaBetween(const std::vector<SectionVertex>& upper,
                                    const std::vector<SectionVertex>& lower);

} // namespace katana::cad
