#pragma once

// A horizontal alignment: the plan centreline of a road, railway, pipe or
// channel (PLAN.MD Phase 21).
//
// DEFINED BY ITS PIs, NOT BY ITS ELEMENTS. An alignment is a sequence of
// points of intersection - the corners of the tangent polygon - each carrying
// the radius of the curve that rounds it and the lengths of the spirals into
// and out of that curve. The tangents, spirals and arcs are DERIVED from those
// by `solveAlignment`. This is how every civil design package works and there
// are two reasons for it here:
//
//   * Continuity is guaranteed by construction. An alignment stored as a list
//     of elements can be edited into one with a gap or a kink between two of
//     them, and then every stationing and section query has to decide what
//     that means. A PI cannot be edited into a discontinuity: move it and the
//     elements are recomputed to meet again.
//   * It is what the designer edits. Moving a corner, changing a radius or
//     lengthening a spiral are the operations; recomputing three elements
//     from them is the machine's job.
//
// The price is that a definition can be infeasible - a curve that does not
// fit between its neighbours, spirals that use up more deflection than the
// corner has - and `solveAlignment` reports exactly which PI is the problem
// rather than drawing something plausible.
//
// The first and last PI are the ends of the alignment and carry no curve.
//
// Stations are distances along the solved alignment from `startStation`,
// which is 0 unless a job numbers its chainage from somewhere else. There is
// no station equation support (a jump in the numbering); none of the work in
// this plan asks for one.

#include <cstddef>
#include <optional>
#include <variant>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/geometry/primitives2d.hpp"
#include "katana/geometry/spiral2.hpp"

namespace katana::geometry {

struct AlignmentPI {
    Point2 point;
    // Radius of the circular curve at this PI, in model units. 0 means no
    // curve: the alignment turns the corner as a kink, which is what an
    // alignment traced from a surveyed polyline is. The sign is ignored; the
    // hand of the turn comes from the PIs on either side.
    double radius = 0.0;
    // Lengths of the transition spirals into and out of the curve. 0 for a
    // simple circular curve. Ignored when radius is 0.
    double spiralIn = 0.0;
    double spiralOut = 0.0;

    friend bool operator==(const AlignmentPI&, const AlignmentPI&) = default;
};

struct HorizontalAlignment {
    std::vector<AlignmentPI> pis;
    double startStation = 0.0;

    friend bool operator==(const HorizontalAlignment&, const HorizontalAlignment&) = default;
};

// One piece of a solved alignment. `shape` is a Segment2 for a tangent, an
// Arc2 for a circular curve, a Spiral2 for a transition. `startStation` is
// where it begins; it ends at startStation + length.
struct AlignmentElement {
    std::variant<Segment2, Arc2, Spiral2> shape;
    double startStation = 0.0;
    double length = 0.0;
    // Index of the PI this element belongs to: the PI whose curve it is, or
    // for a tangent the PI it runs TOWARDS. Reported in errors and useful for
    // labelling.
    std::size_t pi = 0;
};

enum class AlignmentElementKind { Tangent, Spiral, Arc };
[[nodiscard]] AlignmentElementKind kindOf(const AlignmentElement& element);

// A solved alignment: the elements, and the queries a section, a profile and
// a renderer need. Built only by `solveAlignment`, so that one cannot exist
// that is not continuous.
class SolvedAlignment {
  public:
    [[nodiscard]] const std::vector<AlignmentElement>& elements() const { return elements_; }
    [[nodiscard]] double startStation() const { return startStation_; }
    [[nodiscard]] double endStation() const { return startStation_ + length_; }
    [[nodiscard]] double length() const { return length_; }
    [[nodiscard]] bool containsStation(double station) const
    {
        return station >= startStation_ && station <= endStation();
    }

    // nullopt for a station outside the alignment. On the boundary between
    // two elements either answer is the same point, which is the point.
    [[nodiscard]] std::optional<Point2> pointAtStation(double station) const;
    // Tangent direction, radians counter-clockwise from +x.
    [[nodiscard]] std::optional<double> directionAtStation(double station) const;
    // Signed curvature: positive turning left. 0 on a tangent.
    [[nodiscard]] std::optional<double> curvatureAtStation(double station) const;
    // The point `offset` to the LEFT of the centreline at `station` (negative
    // is right), which is what a cross section, a kerb line or a lane edge
    // is. Left is left looking along increasing station.
    [[nodiscard]] std::optional<Point2> pointAtStationOffset(double station,
                                                             double offset) const;

    // The element containing `station`, or nullptr outside the alignment.
    [[nodiscard]] const AlignmentElement* elementAt(double station) const;

    // The alignment as a polyline with every chord within `tolerance` of the
    // true curve, using the one sagitta rule in geometry::chording. Joints
    // between elements appear once. This is what a section is cut along, and
    // what a viewport draws.
    [[nodiscard]] Polyline2 toPolyline(double tolerance) const;

    // The stations of every element boundary, in order, starting with
    // startStation and ending with endStation: TS, SC, CS, ST for each curve.
    // These are where an alignment is labelled and where a section always
    // samples, because the geometry changes character there.
    [[nodiscard]] std::vector<double> keyStations() const;

  private:
    friend katana::core::Result<SolvedAlignment> solveAlignment(const HorizontalAlignment&);
    std::vector<AlignmentElement> elements_;
    double startStation_ = 0.0;
    double length_ = 0.0;
};

// Derives the elements. Fails with InvalidGeometry, naming the PI by index:
//   fewer than two PIs, or consecutive PIs closer than kCoordinate;
//   a reversal (the alignment doubles back through 180 degrees at a PI),
//   because no curve can round it;
//   spirals whose angles exceed the deflection at the PI, so no central arc
//   is left;
//   a curve whose tangent length overlaps its neighbour's, so the tangent
//   between them would be negative;
// and InvalidArgument for a negative radius, spiral length or non-finite
// value.
[[nodiscard]] katana::core::Result<SolvedAlignment>
solveAlignment(const HorizontalAlignment& definition);

} // namespace katana::geometry
