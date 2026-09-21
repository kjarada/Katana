#include "katana/geometry/spiral2.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <limits>

#include "katana/math/numerics.hpp"

namespace katana::geometry {

namespace tol = katana::math::tolerance;

namespace {

// Ten-point Gauss-Legendre nodes and weights on [-1, 1], Abramowitz & Stegun
// Table 25.4. Exact for polynomials up to degree 19; on a panel that turns
// through at most kMaxPanelTurn the integrand is so nearly polynomial that the
// error is far below double precision - see spiral2.hpp for the estimate.
constexpr std::array<double, 5> kNodes{0.1488743389816312, 0.4333953941292472,
                                       0.6794095682990244, 0.8650633666889845,
                                       0.9739065285171717};
constexpr std::array<double, 5> kWeights{0.2955242247147529, 0.2692667193099963,
                                         0.2190863625159820, 0.1494513491505806,
                                         0.0666713443086881};

// The most a single quadrature panel is allowed to turn through. Half a radian
// keeps ten-point Gauss-Legendre exact to better than 1e-20 relative on the
// trigonometric integrand; the number could be several times larger and still
// be far inside double precision, and is kept small only so that the estimate
// in the header is comfortably true rather than marginally so.
constexpr double kMaxPanelTurn = 0.5;

// Cap on panels, so a spiral with an absurd curvature-length product cannot
// ask for millions of evaluations. At this cap and kMaxPanelTurn a spiral has
// turned through 2048 radians, which is 326 full circles: not an alignment.
constexpr std::size_t kMaxPanels = 4096;

} // namespace

Spiral2 Spiral2::fromStraightToRadius(const Point2& start, double direction, double radius,
                                      double length)
{
    Spiral2 spiral;
    spiral.start = start;
    spiral.startDirection = direction;
    spiral.startCurvature = 0.0;
    spiral.endCurvature = radius != 0.0 ? 1.0 / radius : 0.0;
    spiral.length = length;
    return spiral;
}

double Spiral2::flatness() const
{
    const double rate = std::abs(endCurvature - startCurvature);
    if (!(rate > 0.0) || !(length > 0.0)) {
        return std::numeric_limits<double>::infinity();
    }
    return std::sqrt(length / rate);
}

double Spiral2::curvatureAt(double distance) const
{
    if (!(length > 0.0)) {
        return startCurvature;
    }
    return startCurvature + (endCurvature - startCurvature) * (distance / length);
}

double Spiral2::directionAt(double distance) const
{
    // Integral of the linear curvature: k0 * s + (k1 - k0) * s^2 / (2L).
    if (!(length > 0.0)) {
        return startDirection;
    }
    return startDirection + startCurvature * distance +
           (endCurvature - startCurvature) * distance * distance / (2.0 * length);
}

Point2 Spiral2::pointAt(double distance) const
{
    if (distance == 0.0 || !std::isfinite(distance)) {
        return start;
    }
    // Panels sized by how much the tangent can turn over [0, distance]. The
    // bound uses the larger absolute curvature over the interval rather than
    // the signed total turn, so a spiral whose curvature passes through zero -
    // turning one way and then the other - is still divided finely enough.
    const double farCurvature = curvatureAt(distance);
    const double worstCurvature = std::max(std::abs(startCurvature), std::abs(farCurvature));
    const double turnBound = worstCurvature * std::abs(distance);
    const auto panels = static_cast<std::size_t>(
        std::clamp(std::ceil(turnBound / kMaxPanelTurn), 1.0, static_cast<double>(kMaxPanels)));

    const double panelLength = distance / static_cast<double>(panels);
    const double halfPanel = 0.5 * panelLength;
    double x = 0.0;
    double y = 0.0;
    for (std::size_t p = 0; p < panels; ++p) {
        const double centre = (static_cast<double>(p) + 0.5) * panelLength;
        for (std::size_t i = 0; i < kNodes.size(); ++i) {
            // Nodes come in symmetric pairs; both members of the pair share a
            // weight, and evaluating them together keeps the sum symmetric so
            // that a spiral and its mirror image agree exactly.
            const double offset = kNodes[i] * halfPanel;
            const double thetaPlus = directionAt(centre + offset);
            const double thetaMinus = directionAt(centre - offset);
            x += kWeights[i] * (std::cos(thetaPlus) + std::cos(thetaMinus));
            y += kWeights[i] * (std::sin(thetaPlus) + std::sin(thetaMinus));
        }
    }
    return Point2(start.x + x * halfPanel, start.y + y * halfPanel);
}

katana::math::Vec2 Spiral2::tangentAt(double distance) const
{
    const double theta = directionAt(distance);
    return katana::math::Vec2(std::cos(theta), std::sin(theta));
}

double Spiral2::radiusAt(double distance) const
{
    const double curvature = curvatureAt(distance);
    return curvature != 0.0 ? 1.0 / curvature : std::numeric_limits<double>::infinity();
}

bool Spiral2::isDegenerate() const { return startCurvature == endCurvature; }

std::optional<Arc2> Spiral2::asArc() const
{
    if (!isDegenerate() || startCurvature == 0.0 || !(length > 0.0)) {
        return std::nullopt;
    }
    const double radius = 1.0 / startCurvature; // signed: negative turns right
    // The centre lies on the left normal at the signed radius, which for a
    // right-hand turn puts it on the right.
    const Point2 center(start.x - std::sin(startDirection) * radius,
                        start.y + std::cos(startDirection) * radius);
    Arc2 arc;
    arc.center = center;
    arc.radius = std::abs(radius);
    arc.startAngle = std::atan2(start.y - center.y, start.x - center.x);
    arc.sweep = startCurvature * length;
    return arc;
}

std::size_t Spiral2::chordCountFor(double tolerance) const
{
    const double worstCurvature = std::max(std::abs(startCurvature), std::abs(endCurvature));
    if (!(worstCurvature > 0.0) || !(length > 0.0)) {
        return 2; // a straight line needs its two ends
    }
    // The sagitta rule cad::chordArc uses, applied at the tightest radius so
    // that every chord along the spiral is within tolerance, not only the
    // average one.
    const double radius = 1.0 / worstCurvature;
    const double ratio = std::clamp(tolerance / radius, 1.0e-12, 0.5);
    const double step = 2.0 * std::acos(1.0 - ratio);
    const double chordLength = radius * step;
    const double segments =
        std::clamp(std::ceil(length / std::max(chordLength, 1.0e-9)), 1.0, 8192.0);
    return static_cast<std::size_t>(segments) + 1;
}

} // namespace katana::geometry
