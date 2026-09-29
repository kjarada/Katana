#include "katana/geometry/profile.hpp"

#include <algorithm>
#include <cmath>
#include <string>

#include "katana/core/text.hpp"
#include "katana/math/numerics.hpp"

namespace katana::geometry {

namespace tol = katana::math::tolerance;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

namespace {

std::string pviLabel(std::size_t index) { return "PVI " + std::to_string(index); }

} // namespace

double ProfileElement::elevationAt(double distance) const
{
    // z = z0 + g1 x + (g2 - g1) x^2 / (2L). On a tangent g2 == g1 and the
    // quadratic term vanishes, so one formula serves both kinds.
    if (!(length > 0.0)) {
        return startElevation;
    }
    return startElevation + startGrade * distance +
           (endGrade - startGrade) * distance * distance / (2.0 * length);
}

double ProfileElement::gradeAt(double distance) const
{
    if (!(length > 0.0)) {
        return startGrade;
    }
    return startGrade + (endGrade - startGrade) * (distance / length);
}

Result<SolvedProfile> solveProfile(const VerticalAlignment& definition)
{
    const auto& pvis = definition.pvis;
    const std::size_t count = pvis.size();
    if (count < 2) {
        return makeError(ErrorCode::InvalidGeometry, "a profile needs at least two PVIs");
    }
    for (std::size_t i = 0; i < count; ++i) {
        const ProfilePVI& pvi = pvis[i];
        if (!std::isfinite(pvi.station) || !std::isfinite(pvi.elevation) ||
            !std::isfinite(pvi.curveLength)) {
            return makeError(ErrorCode::InvalidArgument, "a PVI has a non-finite value",
                             pviLabel(i));
        }
        if (pvi.curveLength < 0.0) {
            return makeError(ErrorCode::InvalidArgument, "a curve length cannot be negative",
                             pviLabel(i));
        }
        if (i > 0 && pvi.station - pvis[i - 1].station < tol::kCoordinate) {
            return makeError(ErrorCode::InvalidGeometry,
                             "PVI stations must strictly increase", pviLabel(i - 1) + " and " +
                                                                        pviLabel(i));
        }
    }
    if (pvis.front().curveLength > 0.0 || pvis.back().curveLength > 0.0) {
        return makeError(ErrorCode::InvalidGeometry,
                         "the first and last PVI cannot carry a curve: there is no grade on "
                         "the far side to blend into",
                         pvis.front().curveLength > 0.0 ? pviLabel(0) : pviLabel(count - 1));
    }
    // Half-lengths must fit between neighbouring PVIs. The ends count as
    // zero-length curves, so a curve reaching past the start or end is caught
    // by the same test.
    for (std::size_t i = 0; i + 1 < count; ++i) {
        const double available =
            (pvis[i + 1].station - pvis[i].station) - 0.5 * pvis[i].curveLength -
            0.5 * pvis[i + 1].curveLength;
        if (available < -tol::kCoordinate) {
            return makeError(ErrorCode::InvalidGeometry,
                             "the vertical curves overlap: their half-lengths exceed the "
                             "distance between the PVIs",
                             pviLabel(i) + " and " + pviLabel(i + 1) + ", short by " +
                                 katana::core::formatExactReal(-available) + " m");
        }
    }

    // Grade of each tangent between consecutive PVIs.
    std::vector<double> grades(count - 1);
    for (std::size_t i = 0; i + 1 < count; ++i) {
        grades[i] = (pvis[i + 1].elevation - pvis[i].elevation) /
                    (pvis[i + 1].station - pvis[i].station);
    }

    SolvedProfile solved;
    solved.startStation_ = pvis.front().station;
    double station = pvis.front().station;
    double elevation = pvis.front().elevation;
    for (std::size_t i = 1; i < count; ++i) {
        const double grade = grades[i - 1];
        const bool curve = i + 1 < count && pvis[i].curveLength > 0.0;
        const double half = curve ? 0.5 * pvis[i].curveLength : 0.0;
        const double tangentEnd = pvis[i].station - half; // PVC, or the PVI itself
        const double tangentLength = tangentEnd - station;
        // Back-to-back curves leave nothing between them; a zero-length
        // tangent has no business being an element (see alignment.cpp).
        if (tangentLength > tol::kGeometric) {
            solved.elements_.push_back({ProfileElementKind::Tangent, station, tangentLength,
                                        elevation, grade, grade, i});
            elevation += grade * tangentLength;
            station = tangentEnd;
        }
        if (curve) {
            // The parabola starts at the PVC with the incoming grade and ends
            // at the PVT with the outgoing one. Its elevation at the PVT is
            // z_PVI + g2 * L / 2, which is exactly where the next tangent
            // starts, so the joint is continuous by identity and not by
            // adjustment.
            solved.elements_.push_back({ProfileElementKind::Curve, station,
                                        pvis[i].curveLength, elevation, grade, grades[i], i});
            elevation = solved.elements_.back().endElevation();
            station += pvis[i].curveLength;
        }
    }
    solved.length_ = station - solved.startStation_;
    return solved;
}

const ProfileElement* SolvedProfile::elementAt(double station) const
{
    if (elements_.empty() || !containsStation(station)) {
        return nullptr;
    }
    auto after = std::upper_bound(elements_.begin(), elements_.end(), station,
                                  [](double s, const ProfileElement& element) {
                                      return s < element.startStation;
                                  });
    if (after == elements_.begin()) {
        return &elements_.front();
    }
    return &*(after - 1);
}

std::optional<double> SolvedProfile::elevationAt(double station) const
{
    const ProfileElement* element = elementAt(station);
    if (element == nullptr) {
        return std::nullopt;
    }
    return element->elevationAt(station - element->startStation);
}

std::optional<double> SolvedProfile::gradeAt(double station) const
{
    const ProfileElement* element = elementAt(station);
    if (element == nullptr) {
        return std::nullopt;
    }
    return element->gradeAt(station - element->startStation);
}

std::vector<double> SolvedProfile::keyStations() const
{
    std::vector<double> stations;
    stations.push_back(startStation_);
    for (const ProfileElement& element : elements_) {
        stations.push_back(element.startStation + element.length);
    }
    return stations;
}

std::vector<ProfileExtremum> SolvedProfile::highLowPoints() const
{
    std::vector<ProfileExtremum> points;
    for (const ProfileElement& element : elements_) {
        if (element.kind != ProfileElementKind::Curve) {
            continue;
        }
        // The grade is linear across the curve, so it passes through zero
        // inside it exactly when the entry and exit grades differ in sign.
        // x* = g1 L / (g1 - g2) is where.
        if (!(element.startGrade * element.endGrade < 0.0)) {
            continue;
        }
        const double x = element.startGrade * element.length /
                         (element.startGrade - element.endGrade);
        points.push_back({element.startStation + x, element.elevationAt(x),
                          element.startGrade > 0.0, element.pvi});
    }
    return points;
}

} // namespace katana::geometry
