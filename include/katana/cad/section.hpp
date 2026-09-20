#pragma once

// Profiles and cross sections (PLAN.MD Phases 14 and 21).
//
// Both are the same operation: sample one or more surfaces along a plan
// alignment and report elevation against distance along it. A long section
// runs down a road centreline; a cross section runs across it at one station,
// on a line this module also builds. Everything here is headless and unit
// tested - the section VIEW just draws what these return (Rule 3).
//
// SAMPLING. A fixed interval alone is not good enough for engineering: it cuts
// the corners off every ridge and every breakline, so a volume taken from the
// profile disagrees with one taken from the surface. Samples are therefore
// placed at
//
//   * every alignment vertex, because the alignment itself bends there;
//   * every interval along each segment;
//   * every point where the alignment crosses a TIN triangle edge, which is
//     where the surface itself changes slope.
//
// The crossings are found by bisecting between consecutive samples whose
// located triangle differs, rather than by walking the triangulation. The walk
// is asymptotically better, but bisection needs nothing beyond TinSurface's
// public point location, converges to kGeometric in about forty iterations of
// an O(1) query, and cannot be derailed by the surface having holes or by the
// alignment leaving and re-entering it - all three of which happen constantly
// in real survey data. Revisit with a profiler, not with intuition (Rule 6).

#include <optional>
#include <string>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/entity/model.hpp"
#include "katana/geometry/primitives2d.hpp"
#include "katana/terrain/tin_surface.hpp"

namespace katana::cad {

using katana::geometry::Point2;
using katana::geometry::Polyline2;

// One station on the section.
struct SectionSample {
    double station = 0.0; // distance along the alignment from its start
    Point2 plan{};        // the world XY position of that station
    // nullopt where the alignment is off the surface, in a hole, or past its
    // rim. A gap in the profile is information, not an error, so it is
    // represented rather than interpolated across.
    std::optional<double> elevation;
};

// Why a sample was placed. Useful to the view, which draws breaks differently
// from interval ticks, and to a caller exporting the profile.
enum class SampleReason { Interval, AlignmentVertex, SurfaceBreak, Start, End };

struct SectionSurface {
    std::string name;
    std::vector<SectionSample> samples;   // ascending station, no duplicates
    std::vector<SampleReason> reasons;    // parallel to samples
    // Over the samples that have an elevation; nullopt when none do, which is
    // what an alignment that misses the surface entirely produces.
    std::optional<double> minElevation;
    std::optional<double> maxElevation;
};

// Where a drawing entity crosses the section line in plan. A pipe, a kerb or a
// boundary drawn in 2D shows on the section at the station it crosses.
struct SectionCrossing {
    katana::entity::EntityId entity = katana::entity::kInvalidEntityId;
    std::string layer;
    double station = 0.0;
    Point2 plan{};
    // Elevation taken from the entity's own geometry where it carries one,
    // otherwise from the first surface that covers the crossing.
    std::optional<double> elevation;
};

struct SectionOptions {
    // Interval along the alignment, in model units. Must be positive.
    double interval = 1.0;
    // Add a sample wherever the alignment crosses a triangle edge.
    bool includeSurfaceBreaks = true;
    // Include entities from the model that cross the alignment in plan.
    bool includeCrossings = true;
    // Caps the number of samples per surface so that a 20 km alignment at a
    // 1 mm interval fails cleanly instead of exhausting memory.
    std::size_t maximumSamples = 2'000'000;
};

// A named surface to cut. The pointer must outlive the call; nothing is copied.
struct SectionSurfaceInput {
    std::string name;
    const katana::terrain::TinSurface* surface = nullptr;
};

struct Section {
    Polyline2 alignment;
    double length = 0.0;
    std::vector<SectionSurface> surfaces;
    std::vector<SectionCrossing> crossings;
    // Station range x elevation range over everything sampled. Empty when no
    // sample anywhere had an elevation - which is what a viewport needs in
    // order to frame the drawing, and what tells it there is nothing to frame.
    [[nodiscard]] katana::geometry::Box2 extent() const;
};

// Cuts `surfaces` along `alignment`.
//
// Fails with InvalidArgument for an alignment of fewer than two distinct
// vertices, a non-positive or non-finite interval, a null surface, or a request
// that would exceed `maximumSamples`.
[[nodiscard]] katana::core::Result<Section>
extractSection(const Polyline2& alignment, const std::vector<SectionSurfaceInput>& surfaces,
               const katana::entity::Model* model = nullptr, const SectionOptions& options = {});

// Builds the plan line of a cross section: perpendicular to `alignment` at
// `station`, running from -halfWidth to +halfWidth offset. The returned line
// runs left to right looking along the alignment, so a cross section drawn from
// it reads the way a surveyor expects.
//
// Fails with InvalidArgument when the station is outside the alignment, when
// halfWidth is not positive, or when the alignment has no direction there.
[[nodiscard]] katana::core::Result<Polyline2>
crossSectionLine(const Polyline2& alignment, double station, double halfWidth);

// Stations at a regular interval along `alignment`, always including 0 and the
// full length. The set of cross sections a corridor is cut at.
[[nodiscard]] katana::core::Result<std::vector<double>>
sectionStations(const Polyline2& alignment, double interval);

} // namespace katana::cad
