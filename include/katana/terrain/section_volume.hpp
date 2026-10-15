#pragma once

// Volumes from cross sections along an alignment (docs/volumes.md, "Along an
// alignment").
//
// The earthwork between two surfaces measured the way a road job is measured
// and paid: cross sections cut square to the alignment at an interval and at
// its key stations, the area between the two surfaces on each, and the volume
// between consecutive sections from their areas.
//
// THE AREAS ARE EXACT. A section is a straight plan segment; each surface
// along it is the run of pieces where the segment crosses one of its
// triangles, so the surface is linear on each, and where a piece of one
// surface meets a piece of the other the difference between them is linear
// too. Each such span is a trapezoid - two triangles of opposite sign where
// the surfaces cross inside it - in closed form. Nothing is sampled.
//
// THE VOLUMES ARE THE METHODS', AS STATED. Between stations s1 and s2,
// L = s2 - s1 apart, with areas A1 and A2:
//
//   average end area  V = L (A1 + A2) / 2
//   prismoidal        V = L (A1 + 4 Am + A2) / 6, Am the area of the section
//                     cut at the middle station - not the mean of A1 and A2
//
// both as Ghilani and Wolf give them (Elementary Surveying: An Introduction
// to Geomatics, the chapter "Volumes": the average-end-area and prismoidal
// formulas). The prismoidal formula is Simpson's rule applied to the area as
// a function of station, so it is exact when that function is a polynomial
// of degree three or less (Burden and Faires, Numerical Analysis, "Elements
// of Numerical Integration": Simpson's rule has degree of precision three) -
// a section whose width and depth both change linearly along the job, whose
// area is then quadratic, is measured exactly by it and overstated by the
// average end area. The difference of the two is the prismoidal correction.
//
// THE CURVATURE CORRECTION (Pappus). L is measured along the centreline, but
// on a curve the ground at offset o (left positive) is swept along an arc of
// (1 - k o) times the centreline's, k the signed curvature (positive turning
// left): a point on the inside of a curve travels less than the centreline,
// one on the outside more. A section of area A whose centroid stands e from
// the centreline therefore carries A (1 - k e) = A - k M for every metre of
// station, M = A e the first moment of the section about the centreline, and
// the volume between two normals is the integral of A(s) - k(s) M(s) ds.
// Both formulas above are applied to A - k M in the place of A, the cut and
// the fill of each section on their own (each has its own centroid), with k
// the curvature at the middle of the panel: exact on an arc, and on a spiral,
// where k is linear in station, exact for a section that does not change
// along the panel. On a straight k is 0 and the figures are the plain ones;
// so they are for a section symmetric about the centreline on any curve.
// Left off, a quarter circle of radius 50 with a strip 10 m wide on its
// inside measures 4.6 % over, and one on its outside 4.2 % under: the error
// is the fixed factor R / (R -/+ e) of every panel, and a shorter interval
// does not shrink it (docs/volumes.md, "Along an alignment").

#include <cstddef>
#include <optional>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/geometry/alignment.hpp"
#include "katana/geometry/primitives2d.hpp"
#include "katana/geometry/spatial_index.hpp"
#include "katana/terrain/tin_surface.hpp"

namespace katana::core {
class TaskPool;
}

namespace katana::terrain {

// Where a surface has a height along a plan segment: from t0 to t1 of the
// segment's length (0 at its start, 1 at its end), inside one triangle, so the
// height is linear between z0 and z1.
struct SegmentPiece {
    double t0 = 0.0;
    double t1 = 0.0;
    double z0 = 0.0;
    double z1 = 0.0;
};

// A surface read along straight plan segments, exactly: the segment is
// clipped by each triangle it crosses. Where it crosses an edge, the crossing
// and its height are computed from the edge's own vertices (in a fixed
// order), so the pieces on either side of an edge meet at the same bits.
//
// A segment that lies ON an edge - both its ends within kGeometric of the
// edge's line, which is where a section cut at the end of a corridor lies on
// the corridor surface's boundary - is on the ground along the whole of the
// edge: the triangle beside the edge reports it, and so does the triangle on
// the other side. The ends of a section are computed from a bearing, so a
// north-going one is tilted by 1e-16 of its length, and an exact test would
// have kept whichever half the tilt left inside.
class SegmentProfiler {
  public:
    // Indexes the surface's triangles once; the surface must outlive this.
    explicit SegmentProfiler(const TinSurface& surface);

    // The pieces along `from` to `to`, ascending t; a gap is where the
    // segment is off the surface or in a hole. A segment of no length has
    // none. Safe to call from several threads at once.
    [[nodiscard]] std::vector<SegmentPiece> along(const Point2& from, const Point2& to) const;

  private:
    const TinSurface* surface_ = nullptr;
    katana::geometry::SpatialIndex index_;
};

// A trapezoid of width `width` whose heights at its two sides are d0 and d1,
// its positive and negative parts kept apart: where d changes sign inside it,
// two triangles split at the zero. The one sign-split of a linear strip.
struct SignedArea {
    double positive = 0.0;
    double negative = 0.0; // as a magnitude
};
[[nodiscard]] SignedArea signedTrapezoid(double d0, double d1, double width);

// The first moments of the same two parts about a line in the plan of the
// strip, in closed form: the integral of the depth times the offset from that
// line. `offsetAtStart` is the offset of the strip's first side from the line,
// and the offset falls by one for every unit of width across the strip, so
// the strip runs from the line's positive side towards its negative. The
// negative part's depth is taken as a magnitude, so each moment is signed by
// the offset alone: a part on the line's positive side has a positive moment
// whichever part it is. The parts are the ones signedTrapezoid splits, the
// same way, and a moment divided by its area is the part's centroid offset.
struct SignedMoment {
    double positive = 0.0;
    double negative = 0.0;
};
[[nodiscard]] SignedMoment signedTrapezoidMoments(double d0, double d1, double width,
                                                  double offsetAtStart);

// The area between two surfaces on one section, of plan length `length`:
// fill where `design` is above `existing`, cut where it is below, as
// compareSurfaces names them. `width` is the plan length on which both have a
// height - what was measured. `cutMoment` and `fillMoment` are the areas'
// first moments about a line on the section: the point `offsetAtStart` from
// the line at its first end, the offset falling along the section
// (signedTrapezoidMoments). 0 is the first end itself.
struct SectionAreas {
    double cut = 0.0;
    double fill = 0.0;
    double width = 0.0;
    double cutMoment = 0.0;
    double fillMoment = 0.0;
};
[[nodiscard]] SectionAreas sectionAreas(const std::vector<SegmentPiece>& existing,
                                        const std::vector<SegmentPiece>& design, double length,
                                        double offsetAtStart = 0.0);

// The more sections than this is a unit mistake, not a wish: the cap
// terrain::contours puts on its levels (docs/terrain.md, "Contours").
inline constexpr std::size_t kMaxVolumeSections = 100'000;

struct AlignmentVolumeOptions {
    // Sections at every whole multiple of this station (10, 20, 30 ... for
    // 10), plus the range's ends and the alignment's key stations inside it.
    double interval = 10.0;
    // The station range; absent: the alignment's start and end.
    std::optional<double> from{};
    std::optional<double> to{};
    // How far each section reaches left and right of the centreline (left is
    // left looking along increasing station). Absent: the diagonal of the
    // ground the two surfaces have in common, so a section reaches across
    // all of it wherever it is cut - which on a straight run is right, and
    // on a curve or a corner is refused: see volumesAlongAlignment.
    std::optional<double> left{};
    std::optional<double> right{};
    // Also cut the section at the middle of each pair, for the prismoidal
    // formula.
    bool prismoidal = false;
    // The curvature correction above. On by default: it is the standard
    // method, and it changes nothing on a straight or for a section that is
    // symmetric about the centreline. Off gives the plain figure, the areas
    // along the centreline.
    bool curvature = true;
};

// One cross section.
struct StationSection {
    double station = 0.0;
    Point2 centre{};
    double cutArea = 0.0;
    double fillArea = 0.0;
    // The areas' first moments about the centreline, left positive: the cut
    // area times its centroid's offset, and the fill area times its own.
    double cutMoment = 0.0;
    double fillMoment = 0.0;
    double width = 0.0; // plan length on which both surfaces have a height
    // An end of the range or a key station of the alignment (TS, SC, CS,
    // ST): where the geometry changes, so a section is always cut there.
    bool key = false;
};

// What the curvature correction added to a volume (negative: took away), cut
// and fill separately: the volume as reported less the plain one, the areas
// along the centreline.
struct CurvatureShift {
    double cut = 0.0;
    double fill = 0.0;
};

// Between two consecutive sections.
struct SectionPanel {
    double fromStation = 0.0;
    double toStation = 0.0;
    // As reported: with the curvature correction when it is on.
    double endAreaCut = 0.0;
    double endAreaFill = 0.0;
    // With AlignmentVolumeOptions::prismoidal: the middle section and the
    // prismoidal volumes.
    std::optional<StationSection> middle{};
    double prismoidalCut = 0.0;
    double prismoidalFill = 0.0;
    // The signed curvature at the middle of the panel, 1 / m, positive
    // turning left; 0 on a straight and when the correction is off.
    double curvature = 0.0;
    CurvatureShift endAreaShift{};
    CurvatureShift prismoidalShift{}; // with prismoidal
};

struct AlignmentVolumes {
    std::vector<StationSection> sections; // ascending station
    std::vector<SectionPanel> panels;     // sections.size() - 1 of them
    // As reported: with the curvature correction when it is on.
    double endAreaCut = 0.0;
    double endAreaFill = 0.0;
    double prismoidalCut = 0.0;  // with prismoidal
    double prismoidalFill = 0.0;
    // The panels' shifts, summed: what the correction did to each total.
    CurvatureShift endAreaShift{};
    CurvatureShift prismoidalShift{};
    bool curvature = true; // the correction was made
    double fromStation = 0.0;
    double toStation = 0.0;
    double left = 0.0;
    double right = 0.0;
    bool prismoidal = false;
};

// Sections between `existing` and `design` along `alignment`. Fails with
// InvalidArgument for an empty surface, an interval that is not finite and
// positive, a range outside the alignment or of no length, a negative or
// non-finite reach or none at all, more than kMaxVolumeSections sections, or
// sections that cross one another; with InvalidGeometry when the surfaces
// have no ground in common (their boxes do not meet, or no section of the
// range reaches ground that both have).
//
// The average end area is the volume of a panel from the areas of the two
// sections that bound it, and is right only while the panels tile the ground.
// Where two sections cross - the inside of a curve cut deeper than its radius,
// the inside of a corner cut deeper than the interval over the sine of its
// deflection, the legs either side of a bend - the ground beyond the crossing
// is swept by both and counted by both (a 1 m slab of 40 000 m3 measured
// 62 101 m3 round a curve of radius 40 with the default reach). That is
// refused, naming the stations and the longest reach that clears every
// crossing, rather than measured; sections that only touch at their ends (a
// reach equal to the radius) are not crossing.
//
// Two stretches of the alignment running side by side nearer than the reach -
// the legs of a U-turn made of corners, two roads laid in one alignment -
// sweep the same ground twice with no section of one crossing a section of
// the other (their sections are parallel). So the strip each panel sweeps,
// the quadrilateral between the reach segments of its two sections, is also
// tested against the strip of every panel but its neighbours (which share a
// section); a pair that overlaps by more than kGeometric is refused
// (InvalidArgument), naming the stations of the two stretches, and nothing is
// measured. Strips that only touch are not overlapping.
//
// Sections are cut on `pool` (null: TaskPool::shared()), each on its own, and
// summed in station order: the same bits at any thread count, as the two
// tests above find the same pair at any thread count.
[[nodiscard]] katana::core::Result<AlignmentVolumes>
volumesAlongAlignment(const TinSurface& existing, const TinSurface& design,
                      const katana::geometry::SolvedAlignment& alignment,
                      const AlignmentVolumeOptions& options,
                      katana::core::TaskPool* pool = nullptr);

} // namespace katana::terrain
