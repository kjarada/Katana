#include "katana/geometry/chording.hpp"

#include <algorithm>
#include <cmath>

#include "katana/math/numerics.hpp"

namespace katana::geometry {

namespace tol = katana::math::tolerance;

std::size_t sagittaChordCount(double radius, double sweepRadians, double tolerance)
{
    if (!(radius > 0.0) || !std::isfinite(sweepRadians)) {
        return 1;
    }
    const double ratio = std::clamp(tolerance / radius, 1.0e-12, 0.5);
    const double step = 2.0 * std::acos(1.0 - ratio);
    const double sweep = std::abs(sweepRadians);
    // The 1e-9 floor on the step guards a ratio so small that acos returns a
    // denormal, which would otherwise divide to infinity before the clamp.
    return static_cast<std::size_t>(
        std::clamp(std::ceil(sweep / std::max(step, 1.0e-9)), 1.0, 8192.0));
}

std::vector<Point2> chordArc(const Arc2& arc, double tolerance)
{
    std::vector<Point2> out;
    if (!(arc.radius > 0.0) || !std::isfinite(arc.sweep) || std::abs(arc.sweep) <= tol::kAngular) {
        if (arc.radius > 0.0) {
            out.push_back(arc.startPoint());
            out.push_back(arc.endPoint());
        }
        return out;
    }
    const std::size_t count = sagittaChordCount(arc.radius, arc.sweep, tolerance);
    out.reserve(count + 1);
    for (std::size_t i = 0; i <= count; ++i) {
        out.push_back(arc.pointAt(static_cast<double>(i) / static_cast<double>(count)));
    }
    return out;
}

std::vector<Point2> chordCircle(const Circle2& circle, double tolerance)
{
    const Arc2 full{circle.center, circle.radius, 0.0, katana::math::kTwoPi};
    std::vector<Point2> out = chordArc(full, tolerance);
    if (!out.empty()) {
        out.pop_back(); // the closing point duplicates the first
    }
    return out;
}

} // namespace katana::geometry
