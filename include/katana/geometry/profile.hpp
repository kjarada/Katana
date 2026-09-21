#pragma once

// A vertical alignment, or design profile: elevation against station along a
// horizontal alignment (PLAN.MD Phase 21).
//
// The vertical counterpart of `alignment.hpp`, built the same way and for the
// same reasons. It is DEFINED BY ITS PVIs - points of vertical intersection,
// each a station, an elevation and the length of the parabolic curve that
// rounds the grade change there - and the tangents and curves are DERIVED.
// A PVI cannot be edited into a discontinuity; a definition that cannot be
// built is refused naming the PVI.
//
// Vertical curves are symmetric parabolas, which is what every road and rail
// standard specifies: on a parabola the rate of change of grade is constant,
// so the vertical acceleration a vehicle feels is constant through the curve,
// and sight distance is a closed form. Asymmetric (unequal-tangent) curves are
// not provided; they are rare and none of the work in this plan asks for one.
//
// Grades are dimensionless rise over run, positive uphill in the direction of
// increasing station: 0.03 is 3%.

#include <cstddef>
#include <optional>
#include <vector>

#include "katana/core/error.hpp"

namespace katana::geometry {

struct ProfilePVI {
    double station = 0.0;
    double elevation = 0.0;
    // Total length of the parabolic curve centred on this PVI, half before it
    // and half after. 0 means the grade simply changes here - a kink, which is
    // what a profile traced from a surveyed ground line is. The first and
    // last PVI carry no curve.
    double curveLength = 0.0;

    friend bool operator==(const ProfilePVI&, const ProfilePVI&) = default;
};

struct VerticalAlignment {
    std::vector<ProfilePVI> pvis; // ascending station

    friend bool operator==(const VerticalAlignment&, const VerticalAlignment&) = default;
};

enum class ProfileElementKind { Tangent, Curve };

// One piece of a solved profile. On a tangent startGrade == endGrade; on a
// curve the grade runs linearly from one to the other, which is what makes the
// elevation a parabola.
struct ProfileElement {
    ProfileElementKind kind = ProfileElementKind::Tangent;
    double startStation = 0.0;
    double length = 0.0;
    double startElevation = 0.0;
    double startGrade = 0.0;
    double endGrade = 0.0;
    std::size_t pvi = 0; // the PVI this curve belongs to, or a tangent runs towards

    [[nodiscard]] double elevationAt(double distance) const; // from the element start
    [[nodiscard]] double gradeAt(double distance) const;
    [[nodiscard]] double endElevation() const { return elevationAt(length); }
};

// A high or low point: where the grade passes through zero inside a curve.
// A crest gives a high point, a sag a low one. These are what drainage
// design needs - a low point on a sag is where water collects - and what a
// profile is labelled with.
struct ProfileExtremum {
    double station = 0.0;
    double elevation = 0.0;
    bool high = false;
    std::size_t pvi = 0;
};

class SolvedProfile {
  public:
    [[nodiscard]] const std::vector<ProfileElement>& elements() const { return elements_; }
    [[nodiscard]] double startStation() const { return startStation_; }
    [[nodiscard]] double endStation() const { return startStation_ + length_; }
    [[nodiscard]] double length() const { return length_; }
    [[nodiscard]] bool containsStation(double station) const
    {
        return station >= startStation_ && station <= endStation();
    }

    // nullopt outside the profile.
    [[nodiscard]] std::optional<double> elevationAt(double station) const;
    [[nodiscard]] std::optional<double> gradeAt(double station) const;
    [[nodiscard]] const ProfileElement* elementAt(double station) const;

    // Start, every PVC and PVT, every kink, end - in order, once each.
    [[nodiscard]] std::vector<double> keyStations() const;
    // High and low points, in station order.
    [[nodiscard]] std::vector<ProfileExtremum> highLowPoints() const;

  private:
    friend katana::core::Result<SolvedProfile> solveProfile(const VerticalAlignment&);
    std::vector<ProfileElement> elements_;
    double startStation_ = 0.0;
    double length_ = 0.0;
};

// Derives the elements. Fails with InvalidGeometry, naming the PVI:
//   fewer than two PVIs, or stations not strictly increasing;
//   a curve on the first or last PVI, which has nothing to blend into;
//   neighbouring curves whose half-lengths overlap, so the tangent between
//   them would be negative;
// and InvalidArgument for a negative curve length or a non-finite value.
[[nodiscard]] katana::core::Result<SolvedProfile> solveProfile(const VerticalAlignment& definition);

} // namespace katana::geometry
