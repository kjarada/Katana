#include "katana/geometry/alignment.hpp"

#include <algorithm>
#include <cmath>
#include <string>

#include "katana/core/text.hpp"
#include "katana/geometry/chording.hpp"
#include "katana/math/numerics.hpp"

namespace katana::geometry {

namespace tol = katana::math::tolerance;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

namespace {

// What a spiral of `length` leading into radius `radius` does to the circular
// curve it joins, in the frame of the incoming tangent:
//   angle      the deflection the spiral itself uses up, L / 2R;
//   shift      how far the curve is pushed inward, off the tangent it would
//              have touched with no spiral (the "p" of the design texts);
//   extension  how far along the tangent the curve's shifted tangent point
//              advances (the "k").
// All three are 0 for no spiral, which is how the simple curve falls out of
// the same code with nothing special-cased.
struct SpiralOffsets {
    double angle = 0.0;
    double shift = 0.0;
    double extension = 0.0;
};

SpiralOffsets spiralOffsets(double radius, double length)
{
    if (!(length > 0.0)) {
        return {};
    }
    const double angle = length / (2.0 * radius);
    const Spiral2 spiral = Spiral2::fromStraightToRadius(Point2(0.0, 0.0), 0.0, radius, length);
    const Point2 end = spiral.endPoint();
    return {angle, end.y - radius * (1.0 - std::cos(angle)), end.x - radius * std::sin(angle)};
}

double directionOf(const Point2& from, const Point2& to)
{
    return std::atan2(to.y - from.y, to.x - from.x);
}

Vec2 unitTowards(const Point2& from, const Point2& to)
{
    return (to - from).normalized();
}

std::string piLabel(std::size_t index) { return "PI " + std::to_string(index); }

// Everything solved at one PI, before the elements are laid out.
struct CurveAtPI {
    bool present = false;
    double sign = 1.0; // +1 turns left
    double radius = 0.0;
    double spiralIn = 0.0;
    double spiralOut = 0.0;
    double arcAngle = 0.0;     // of the central circular arc
    double tangentIn = 0.0;    // PI back to TS
    double tangentOut = 0.0;   // PI forward to ST
    double directionIn = 0.0;  // of the incoming tangent
    Point2 ts;                 // tangent-to-spiral (or tangent-to-curve)
};

} // namespace

AlignmentElementKind kindOf(const AlignmentElement& element)
{
    switch (element.shape.index()) {
    case 0:
        return AlignmentElementKind::Tangent;
    case 1:
        return AlignmentElementKind::Arc;
    default:
        return AlignmentElementKind::Spiral;
    }
}

Result<SolvedAlignment> solveAlignment(const HorizontalAlignment& definition)
{
    const auto& pis = definition.pis;
    const std::size_t count = pis.size();
    if (count < 2) {
        return makeError(ErrorCode::InvalidGeometry, "an alignment needs at least two PIs");
    }
    if (!std::isfinite(definition.startStation)) {
        return makeError(ErrorCode::InvalidArgument, "the start station must be finite");
    }
    for (std::size_t i = 0; i < count; ++i) {
        const AlignmentPI& pi = pis[i];
        if (!std::isfinite(pi.point.x) || !std::isfinite(pi.point.y) || !std::isfinite(pi.radius) ||
            !std::isfinite(pi.spiralIn) || !std::isfinite(pi.spiralOut)) {
            return makeError(ErrorCode::InvalidArgument, "a PI has a non-finite value",
                             piLabel(i));
        }
        if (pi.spiralIn < 0.0 || pi.spiralOut < 0.0) {
            return makeError(ErrorCode::InvalidArgument, "a spiral length cannot be negative",
                             piLabel(i));
        }
        if (i > 0 && pis[i - 1].point.distanceTo(pi.point) < tol::kCoordinate) {
            return makeError(ErrorCode::InvalidGeometry,
                             "consecutive PIs coincide, so the tangent between them has no "
                             "direction",
                             piLabel(i - 1) + " and " + piLabel(i));
        }
    }

    // ---- solve each corner ---------------------------------------------------------
    std::vector<CurveAtPI> curves(count);
    for (std::size_t i = 1; i + 1 < count; ++i) {
        CurveAtPI& curve = curves[i];
        curve.directionIn = directionOf(pis[i - 1].point, pis[i].point);
        const double directionOut = directionOf(pis[i].point, pis[i + 1].point);
        const double deflection =
            katana::math::normalizeAngleSigned(directionOut - curve.directionIn);
        const double radius = std::abs(pis[i].radius);
        if (!(radius > 0.0) || std::abs(deflection) <= tol::kAngular) {
            continue; // a kink, or no corner at all: the tangents simply meet
        }
        if (katana::math::kPi - std::abs(deflection) <= tol::kAngular) {
            return makeError(ErrorCode::InvalidGeometry,
                             "the alignment reverses on itself, and no curve can round a 180 "
                             "degree corner",
                             piLabel(i));
        }
        const SpiralOffsets in = spiralOffsets(radius, pis[i].spiralIn);
        const SpiralOffsets out = spiralOffsets(radius, pis[i].spiralOut);
        double arcAngle = std::abs(deflection) - in.angle - out.angle;
        if (arcAngle < -tol::kAngular) {
            return makeError(
                ErrorCode::InvalidGeometry,
                "the spirals use more deflection than the corner has, leaving no "
                "room for the curve",
                piLabel(i) + ": deflection " +
                    katana::core::formatExactReal(std::abs(deflection) * katana::math::kRadToDeg) +
                    " deg, spirals " +
                    katana::core::formatExactReal((in.angle + out.angle) *
                                                  katana::math::kRadToDeg) +
                    " deg");
        }
        arcAngle = std::max(arcAngle, 0.0);

        // Tangent lengths, derived by placing the shifted circle: its centre
        // sits (R + shift_in) off the back tangent and (R + shift_out) off
        // the forward tangent, and each tangent point is `extension` short of
        // the foot of that perpendicular. With equal spirals this reduces to
        // the textbook T = k + (R + p) tan(D / 2), and with none to
        // T = R tan(D / 2).
        const double sinD = std::sin(std::abs(deflection));
        const double cosD = std::cos(std::abs(deflection));
        curve.present = true;
        curve.sign = deflection > 0.0 ? 1.0 : -1.0;
        curve.radius = radius;
        curve.spiralIn = pis[i].spiralIn;
        curve.spiralOut = pis[i].spiralOut;
        curve.arcAngle = arcAngle;
        curve.tangentIn = in.extension + ((radius + out.shift) - (radius + in.shift) * cosD) / sinD;
        curve.tangentOut = out.extension + ((radius + in.shift) - (radius + out.shift) * cosD) / sinD;
        curve.ts = pis[i].point - unitTowards(pis[i - 1].point, pis[i].point) * curve.tangentIn;
    }

    // ---- check that neighbouring curves leave room for the tangent between them ----
    for (std::size_t i = 0; i + 1 < count; ++i) {
        const double available = pis[i].point.distanceTo(pis[i + 1].point) -
                                 (curves[i].present ? curves[i].tangentOut : 0.0) -
                                 (curves[i + 1].present ? curves[i + 1].tangentIn : 0.0);
        if (available < -tol::kCoordinate) {
            return makeError(ErrorCode::InvalidGeometry,
                             "the curves overlap: their tangent lengths exceed the distance "
                             "between the PIs",
                             piLabel(i) + " and " + piLabel(i + 1) + ", short by " +
                                 katana::core::formatExactReal(-available) + " m");
        }
    }

    // ---- lay the elements out --------------------------------------------------------
    SolvedAlignment solved;
    solved.startStation_ = definition.startStation;
    double station = definition.startStation;
    Point2 cursor = pis[0].point;
    for (std::size_t i = 1; i < count; ++i) {
        const CurveAtPI& curve = curves[i];
        const Point2 target = curve.present ? curve.ts : pis[i].point;
        const double tangentLength = cursor.distanceTo(target);
        // A tangent shorter than the geometric tolerance is two curves back to
        // back, and is not stored: an element of zero length has no direction
        // to report and would make every station query at its joint ambiguous.
        if (tangentLength > tol::kGeometric) {
            solved.elements_.push_back({Segment2{cursor, target}, station, tangentLength, i});
            station += tangentLength;
            cursor = target;
        }
        if (!curve.present) {
            continue;
        }
        const double curvature = curve.sign / curve.radius;
        double direction = curve.directionIn;
        if (curve.spiralIn > 0.0) {
            Spiral2 spiral;
            spiral.start = cursor;
            spiral.startDirection = direction;
            spiral.startCurvature = 0.0;
            spiral.endCurvature = curvature;
            spiral.length = curve.spiralIn;
            solved.elements_.push_back({spiral, station, curve.spiralIn, i});
            station += curve.spiralIn;
            cursor = spiral.endPoint();
            direction = spiral.endDirection();
        }
        if (curve.arcAngle > tol::kAngular) {
            // Built as a constant-curvature spiral and converted, so the arc's
            // centre and start angle follow exactly the conventions the
            // spirals on either side of it use.
            Spiral2 constant;
            constant.start = cursor;
            constant.startDirection = direction;
            constant.startCurvature = curvature;
            constant.endCurvature = curvature;
            constant.length = curve.radius * curve.arcAngle;
            const Arc2 arc = *constant.asArc();
            solved.elements_.push_back({arc, station, constant.length, i});
            station += constant.length;
            cursor = arc.endPoint();
            direction += curve.sign * curve.arcAngle;
        }
        if (curve.spiralOut > 0.0) {
            Spiral2 spiral;
            spiral.start = cursor;
            spiral.startDirection = direction;
            spiral.startCurvature = curvature;
            spiral.endCurvature = 0.0;
            spiral.length = curve.spiralOut;
            solved.elements_.push_back({spiral, station, curve.spiralOut, i});
            station += curve.spiralOut;
            cursor = spiral.endPoint();
        }
        // `cursor` is now where the curve chain actually ended. It is NOT
        // snapped to the computed ST: if the tangent-length derivation were
        // wrong the chain would end off the forward tangent, and the next
        // tangent would visibly kink - which the tests measure. Snapping would
        // hide exactly that.
    }
    solved.length_ = station - definition.startStation;
    return solved;
}

// ---- queries ------------------------------------------------------------------------

const AlignmentElement* SolvedAlignment::elementAt(double station) const
{
    if (elements_.empty() || !containsStation(station)) {
        return nullptr;
    }
    // First element that starts after `station`, then step back one. A station
    // exactly at a joint lands on the element that begins there, except the
    // end station, which belongs to the last element.
    auto after = std::upper_bound(elements_.begin(), elements_.end(), station,
                                  [](double s, const AlignmentElement& element) {
                                      return s < element.startStation;
                                  });
    if (after == elements_.begin()) {
        return &elements_.front();
    }
    return &*(after - 1);
}

std::optional<Point2> SolvedAlignment::pointAtStation(double station) const
{
    const AlignmentElement* element = elementAt(station);
    if (element == nullptr) {
        return std::nullopt;
    }
    const double local = station - element->startStation;
    return std::visit(
        [&](const auto& shape) -> Point2 {
            using Shape = std::decay_t<decltype(shape)>;
            if constexpr (std::is_same_v<Shape, Spiral2>) {
                return shape.pointAt(local);
            } else {
                return shape.pointAt(local / element->length);
            }
        },
        element->shape);
}

std::optional<double> SolvedAlignment::directionAtStation(double station) const
{
    const AlignmentElement* element = elementAt(station);
    if (element == nullptr) {
        return std::nullopt;
    }
    const double local = station - element->startStation;
    return std::visit(
        [&](const auto& shape) -> double {
            using Shape = std::decay_t<decltype(shape)>;
            if constexpr (std::is_same_v<Shape, Segment2>) {
                return directionOf(shape.start, shape.end);
            } else if constexpr (std::is_same_v<Shape, Arc2>) {
                // The tangent is a quarter turn ahead of the radius for a
                // counter-clockwise arc and a quarter turn behind for a
                // clockwise one.
                const double radial = shape.startAngle + shape.sweep * (local / element->length);
                return radial + (shape.sweep > 0.0 ? 0.5 : -0.5) * katana::math::kPi;
            } else {
                return shape.directionAt(local);
            }
        },
        element->shape);
}

std::optional<double> SolvedAlignment::curvatureAtStation(double station) const
{
    const AlignmentElement* element = elementAt(station);
    if (element == nullptr) {
        return std::nullopt;
    }
    const double local = station - element->startStation;
    return std::visit(
        [&](const auto& shape) -> double {
            using Shape = std::decay_t<decltype(shape)>;
            if constexpr (std::is_same_v<Shape, Segment2>) {
                return 0.0;
            } else if constexpr (std::is_same_v<Shape, Arc2>) {
                return (shape.sweep > 0.0 ? 1.0 : -1.0) / shape.radius;
            } else {
                return shape.curvatureAt(local);
            }
        },
        element->shape);
}

std::optional<Point2> SolvedAlignment::pointAtStationOffset(double station, double offset) const
{
    const auto point = pointAtStation(station);
    const auto direction = directionAtStation(station);
    if (!point || !direction) {
        return std::nullopt;
    }
    // Left normal of the tangent: rotate the direction a quarter turn
    // counter-clockwise.
    return Point2(point->x - std::sin(*direction) * offset, point->y + std::cos(*direction) * offset);
}

Polyline2 SolvedAlignment::toPolyline(double tolerance) const
{
    Polyline2 polyline;
    polyline.closed = false;
    for (const AlignmentElement& element : elements_) {
        std::vector<Point2> points = std::visit(
            [&](const auto& shape) -> std::vector<Point2> {
                using Shape = std::decay_t<decltype(shape)>;
                if constexpr (std::is_same_v<Shape, Segment2>) {
                    return {shape.start, shape.end};
                } else if constexpr (std::is_same_v<Shape, Arc2>) {
                    return chordArc(shape, tolerance);
                } else {
                    const std::size_t n = shape.chordCountFor(tolerance);
                    std::vector<Point2> out;
                    out.reserve(n);
                    for (std::size_t k = 0; k < n; ++k) {
                        const double t = static_cast<double>(k) / static_cast<double>(n - 1);
                        out.push_back(shape.pointAt(t * shape.length));
                    }
                    return out;
                }
            },
            element.shape);
        // Each element begins where the previous one ended, so its first point
        // is the joint already emitted.
        const std::size_t first = polyline.vertices.empty() ? 0 : 1;
        polyline.vertices.insert(polyline.vertices.end(), points.begin() + static_cast<std::ptrdiff_t>(std::min(first, points.size())), points.end());
    }
    return polyline;
}

std::vector<double> SolvedAlignment::keyStations() const
{
    std::vector<double> stations;
    stations.push_back(startStation_);
    for (const AlignmentElement& element : elements_) {
        stations.push_back(element.startStation + element.length);
    }
    return stations;
}

} // namespace katana::geometry
