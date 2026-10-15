#include "katana/geometry/primitives2d.hpp"

#include "katana/geometry/point_batch.hpp"

#include <algorithm>
#include <cmath>
#include <utility>

namespace katana::geometry {

namespace tol = katana::math::tolerance;
using katana::math::kHalfPi;
using katana::math::kPi;
using katana::math::kTwoPi;
using katana::math::normalizeAngle;

namespace {

// One edge's vote in the crossing number of Polyline2::classify, with the
// half-open edge rule: an edge counts when it straddles the horizontal through
// p, the upper endpoint excluded, so that a ray through a vertex is counted
// exactly once. Shared with PolygonClassifier so the two cannot come to
// disagree about a polygon.
bool edgeCrossesRay(const Point2& a, const Point2& b, const Point2& p)
{
    if ((a.y > p.y) == (b.y > p.y)) {
        return false;
    }
    const double xAtY = a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x);
    return p.x < xAtY;
}

} // namespace

// ---- Line2 -------------------------------------------------------------------

std::optional<Line2> Line2::through(const Point2& a, const Point2& b)
{
    if (a.distanceTo(b) <= tol::kGeometric) {
        return std::nullopt;
    }
    return Line2{a, b - a};
}

double Line2::parameterOf(const Point2& p) const
{
    const double lengthSquared = direction.lengthSquared();
    return lengthSquared > 0.0 ? (p - origin).dot(direction) / lengthSquared : 0.0;
}

double Line2::signedDistanceTo(const Point2& p) const
{
    const double length = direction.length();
    return length > 0.0 ? direction.cross(p - origin) / length : 0.0;
}

double Line2::distanceTo(const Point2& p) const
{
    if (direction.lengthSquared() > 0.0) {
        return std::abs(signedDistanceTo(p));
    }
    return origin.distanceTo(p); // a zero direction degenerates to the origin point
}

// ---- Segment2 ----------------------------------------------------------------

bool Segment2::isDegenerate() const
{
    return length() <= tol::kGeometric;
}

double Segment2::parameterOf(const Point2& p) const
{
    const Vec2 d = delta();
    const double lengthSquared = d.lengthSquared();
    if (!(lengthSquared > 0.0)) {
        return 0.0;
    }
    return std::clamp((p - start).dot(d) / lengthSquared, 0.0, 1.0);
}

double Segment2::distanceTo(const Point2& p) const
{
    return closestPoint(p).distanceTo(p);
}

Box2 Segment2::boundingBox() const
{
    Box2 box;
    box.expand(start);
    box.expand(end);
    return box;
}

// ---- Circle2 -----------------------------------------------------------------

Point2 Circle2::pointAtAngle(double radians) const
{
    return center + Vec2(std::cos(radians), std::sin(radians)) * radius;
}

Containment Circle2::classify(const Point2& p) const
{
    const double offset = center.distanceTo(p) - radius;
    if (std::abs(offset) <= tol::kGeometric) {
        return Containment::OnBoundary;
    }
    return offset < 0.0 ? Containment::Inside : Containment::Outside;
}

Point2 Circle2::closestPoint(const Point2& p) const
{
    const Vec2 direction = (p - center).normalized();
    if (direction == Vec2{}) {
        return pointAtAngle(0.0);
    }
    return center + direction * radius;
}

double Circle2::distanceTo(const Point2& p) const
{
    return std::abs(center.distanceTo(p) - radius);
}

Box2 Circle2::boundingBox() const
{
    return Box2(Point2(center.x - radius, center.y - radius),
                Point2(center.x + radius, center.y + radius));
}

// ---- Arc2 --------------------------------------------------------------------

std::optional<Arc2> Arc2::throughPoints(const Point2& a, const Point2& b, const Point2& c)
{
    const auto circle = Triangle2{a, b, c}.circumcircle();
    if (!circle) {
        return std::nullopt;
    }
    const double angleA = (a - circle->center).angle();
    const double angleC = (c - circle->center).angle();
    const bool counterClockwise = (b - a).cross(c - a) > 0.0;
    const double sweep = counterClockwise ? normalizeAngle(angleC - angleA)
                                          : -normalizeAngle(angleA - angleC);
    return Arc2{circle->center, circle->radius, angleA, sweep};
}

double Arc2::length() const
{
    return std::abs(sweep) * radius;
}

Point2 Arc2::pointAtAngle(double radians) const
{
    return center + Vec2(std::cos(radians), std::sin(radians)) * radius;
}

namespace {

// Angle travelled from the arc start to `radians`, measured in the sweep direction.
double travelledAngle(const Arc2& arc, double radians)
{
    return arc.sweep >= 0.0 ? normalizeAngle(radians - arc.startAngle)
                            : normalizeAngle(arc.startAngle - radians);
}

} // namespace

bool Arc2::containsAngle(double radians, double angularTolerance) const
{
    const double travelled = travelledAngle(*this, radians);
    return travelled <= std::abs(sweep) + angularTolerance ||
           travelled >= kTwoPi - angularTolerance;
}

double Arc2::parameterOfAngle(double radians) const
{
    const double span = std::abs(sweep);
    if (!(span > 0.0)) {
        return 0.0;
    }
    const double travelled = travelledAngle(*this, radians);
    if (travelled <= span) {
        return travelled / span;
    }
    // Outside the sweep: clamp to whichever end is angularly closer.
    return (travelled - span) < (kTwoPi - travelled) ? 1.0 : 0.0;
}

Point2 Arc2::closestPoint(const Point2& p) const
{
    const Vec2 offset = p - center;
    if (offset == Vec2{}) {
        return startPoint();
    }
    return pointAt(parameterOfAngle(offset.angle()));
}

double Arc2::distanceTo(const Point2& p) const
{
    return closestPoint(p).distanceTo(p);
}

Box2 Arc2::boundingBox() const
{
    Box2 box;
    box.expand(startPoint());
    box.expand(endPoint());
    for (int quadrant = 0; quadrant < 4; ++quadrant) {
        const double cardinal = kHalfPi * quadrant;
        if (containsAngle(cardinal)) {
            box.expand(pointAtAngle(cardinal));
        }
    }
    return box;
}

// ---- Polyline2 ---------------------------------------------------------------

std::size_t Polyline2::segmentCount() const
{
    if (vertices.size() < 2) {
        return 0;
    }
    return closed ? vertices.size() : vertices.size() - 1;
}

Segment2 Polyline2::segment(std::size_t index) const
{
    return Segment2{vertices.at(index), vertices.at((index + 1) % vertices.size())};
}

double Polyline2::length() const
{
    double total = 0.0;
    for (std::size_t i = 0; i < segmentCount(); ++i) {
        total += segment(i).length();
    }
    return total;
}

double Polyline2::signedArea() const
{
    if (!closed || vertices.size() < 3) {
        return 0.0;
    }
    const Point2 origin = vertices.front();
    double twiceArea = 0.0;
    for (std::size_t i = 0; i < vertices.size(); ++i) {
        const Vec2 current = vertices[i] - origin;
        const Vec2 next = vertices[(i + 1) % vertices.size()] - origin;
        twiceArea += current.cross(next);
    }
    return 0.5 * twiceArea;
}

double Polyline2::area() const
{
    return std::abs(signedArea());
}

std::optional<Point2> Polyline2::centroid() const
{
    if (!closed || vertices.size() < 3) {
        return std::nullopt;
    }
    const Point2 origin = vertices.front();
    double twiceArea = 0.0;
    Vec2 moment;
    double scale = 0.0;
    for (std::size_t i = 0; i < vertices.size(); ++i) {
        const Vec2 current = vertices[i] - origin;
        const Vec2 next = vertices[(i + 1) % vertices.size()] - origin;
        const double cross = current.cross(next);
        twiceArea += cross;
        moment += (current + next) * cross;
        scale = std::max(scale, current.lengthSquared());
    }
    // Degenerate when the area is negligible against the polygon's own extent.
    if (!(std::abs(twiceArea) > tol::kAbsolute * scale)) {
        return std::nullopt;
    }
    return origin + moment / (3.0 * twiceArea);
}

Containment Polyline2::classify(const Point2& p) const
{
    if (!closed || vertices.size() < 3) {
        return Containment::Outside;
    }
    if (const auto distance = distanceTo(p); distance && *distance <= tol::kGeometric) {
        return Containment::OnBoundary;
    }
    bool inside = false;
    for (std::size_t i = 0; i < vertices.size(); ++i) {
        inside = inside != edgeCrossesRay(vertices[i], vertices[(i + 1) % vertices.size()], p);
    }
    return inside ? Containment::Inside : Containment::Outside;
}

// ---- PolygonClassifier ---------------------------------------------------------

PolygonClassifier::PolygonClassifier(const Polyline2& polygon) : vertices_(polygon.vertices)
{
    const std::size_t count = vertices_.size();
    if (count < 3) {
        vertices_.clear();
        return;
    }
    for (const Point2& vertex : vertices_) {
        bounds_.expand(vertex);
    }
    // About four edges a band; a thousand bands at most, so an edge that spans
    // the polygon's whole height is entered a thousand times and not a
    // hundred thousand.
    constexpr std::size_t kMostBands = 1024;
    bands_ = std::clamp<std::size_t>(count / 4, 1, kMostBands);
    const double height = bounds_.height();
    bandHeight_ = height > 0.0 ? height / static_cast<double>(bands_) : 1.0;

    // Counting pass, then fill: the layout the surface locator uses.
    const auto range = [this, count](std::size_t edge) {
        const Point2& a = vertices_[edge];
        const Point2& b = vertices_[(edge + 1) % count];
        return std::pair{bandOf(std::min(a.y, b.y) - tol::kGeometric),
                         bandOf(std::max(a.y, b.y) + tol::kGeometric)};
    };
    bandStart_.assign(bands_ + 1, 0);
    for (std::size_t edge = 0; edge < count; ++edge) {
        const auto [first, last] = range(edge);
        for (std::size_t band = first; band <= last; ++band) {
            ++bandStart_[band + 1];
        }
    }
    for (std::size_t band = 0; band < bands_; ++band) {
        bandStart_[band + 1] += bandStart_[band];
    }
    edges_.resize(bandStart_.back());
    std::vector<std::uint32_t> cursor(bandStart_.begin(), bandStart_.end() - 1);
    for (std::size_t edge = 0; edge < count; ++edge) {
        const auto [first, last] = range(edge);
        for (std::size_t band = first; band <= last; ++band) {
            edges_[cursor[band]++] = static_cast<std::uint32_t>(edge);
        }
    }
}

std::size_t PolygonClassifier::bandOf(double y) const
{
    const double band = std::floor((y - bounds_.min.y) / bandHeight_);
    if (!(band > 0.0)) {
        return 0; // also NaN
    }
    return band >= static_cast<double>(bands_) ? bands_ - 1 : static_cast<std::size_t>(band);
}

Containment PolygonClassifier::classify(const Point2& p) const
{
    const std::size_t count = vertices_.size();
    if (count < 3 || !(p.y >= bounds_.min.y - tol::kGeometric &&
                       p.y <= bounds_.max.y + tol::kGeometric)) {
        return Containment::Outside;
    }
    const std::size_t band = bandOf(p.y);
    bool inside = false;
    for (std::uint32_t i = bandStart_[band]; i < bandStart_[band + 1]; ++i) {
        const Point2& a = vertices_[edges_[i]];
        const Point2& b = vertices_[(edges_[i] + 1) % count];
        if (Segment2{a, b}.distanceTo(p) <= tol::kGeometric) {
            return Containment::OnBoundary;
        }
    }
    for (std::uint32_t i = bandStart_[band]; i < bandStart_[band + 1]; ++i) {
        inside = inside != edgeCrossesRay(vertices_[edges_[i]],
                                          vertices_[(edges_[i] + 1) % count], p);
    }
    return inside ? Containment::Inside : Containment::Outside;
}

std::optional<Point2> Polyline2::closestPoint(const Point2& p) const
{
    if (vertices.empty()) {
        return std::nullopt;
    }
    Point2 best = vertices.front();
    double bestDistance = best.distanceTo(p);
    for (std::size_t i = 0; i < segmentCount(); ++i) {
        const Point2 candidate = segment(i).closestPoint(p);
        const double distance = candidate.distanceTo(p);
        if (distance < bestDistance) {
            bestDistance = distance;
            best = candidate;
        }
    }
    return best;
}

std::optional<double> Polyline2::distanceTo(const Point2& p) const
{
    const auto closest = closestPoint(p);
    if (!closest) {
        return std::nullopt;
    }
    return closest->distanceTo(p);
}

Box2 Polyline2::boundingBox() const { return boundsOf(vertices); }

Polyline2 Polyline2::reversed() const
{
    Polyline2 result{vertices, closed};
    std::reverse(result.vertices.begin(), result.vertices.end());
    return result;
}

Polyline2 Polyline2::withoutDuplicateVertices() const
{
    Polyline2 result;
    result.closed = closed;
    for (const Point2& vertex : vertices) {
        if (result.vertices.empty() ||
            result.vertices.back().distanceTo(vertex) > tol::kGeometric) {
            result.vertices.push_back(vertex);
        }
    }
    if (closed && result.vertices.size() > 1 &&
        result.vertices.front().distanceTo(result.vertices.back()) <= tol::kGeometric) {
        result.vertices.pop_back();
    }
    return result;
}

// ---- Rectangle2 --------------------------------------------------------------

Rectangle2 Rectangle2::fromCorners(const Point2& a, const Point2& b)
{
    const Point2 lo(std::min(a.x, b.x), std::min(a.y, b.y));
    const Point2 hi(std::max(a.x, b.x), std::max(a.y, b.y));
    return Rectangle2{lo, hi.x - lo.x, hi.y - lo.y};
}

std::array<Point2, 4> Rectangle2::corners() const
{
    return {origin, origin + Vec2(width, 0.0), origin + Vec2(width, height),
            origin + Vec2(0.0, height)};
}

Polyline2 Rectangle2::toPolyline() const
{
    const auto c = corners();
    return Polyline2{{c.begin(), c.end()}, true};
}

Box2 Rectangle2::boundingBox() const
{
    return Box2(origin, origin + Vec2(width, height));
}

Containment Rectangle2::classify(const Point2& p) const
{
    if (distanceTo(p) <= tol::kGeometric) {
        return Containment::OnBoundary;
    }
    return boundingBox().contains(p) ? Containment::Inside : Containment::Outside;
}

Point2 Rectangle2::closestPoint(const Point2& p) const
{
    // The same walk toPolyline().closestPoint(p) performs - first corner, then
    // the four edges in order, keeping a strictly nearer candidate - on the
    // stack. Building the Polyline2 heap-allocated a four-point vector, and
    // classify() calls this for every containment test.
    const std::array<Point2, 4> c = corners();
    Point2 best = c[0];
    double bestDistance = best.distanceTo(p);
    for (std::size_t i = 0; i < 4; ++i) {
        const Point2 candidate = Segment2{c[i], c[(i + 1) % 4]}.closestPoint(p);
        const double distance = candidate.distanceTo(p);
        if (distance < bestDistance) {
            bestDistance = distance;
            best = candidate;
        }
    }
    return best;
}

double Rectangle2::distanceTo(const Point2& p) const
{
    return closestPoint(p).distanceTo(p);
}

// ---- Triangle2 ---------------------------------------------------------------

double Triangle2::twiceSignedArea() const
{
    return (b - a).cross(c - a);
}

double Triangle2::signedArea() const
{
    return 0.5 * twiceSignedArea();
}

double Triangle2::area() const
{
    return std::abs(signedArea());
}

double Triangle2::perimeter() const
{
    return a.distanceTo(b) + b.distanceTo(c) + c.distanceTo(a);
}

Point2 Triangle2::centroid() const
{
    // Relative to `a` so large coordinates keep their low-order digits.
    return a + ((b - a) + (c - a)) / 3.0;
}

bool Triangle2::isDegenerate() const
{
    // Height over the longest edge below the geometric tolerance.
    const double longest = std::max({a.distanceTo(b), b.distanceTo(c), c.distanceTo(a)});
    if (longest <= tol::kGeometric) {
        return true;
    }
    return 2.0 * area() / longest <= tol::kGeometric;
}

Box2 Triangle2::boundingBox() const
{
    Box2 box;
    box.expand(a);
    box.expand(b);
    box.expand(c);
    return box;
}

Containment Triangle2::classify(const Point2& p) const
{
    const double edgeDistance = std::min({Segment2{a, b}.distanceTo(p), Segment2{b, c}.distanceTo(p),
                                          Segment2{c, a}.distanceTo(p)});
    if (edgeDistance <= tol::kGeometric) {
        return Containment::OnBoundary;
    }
    const double d1 = (b - a).cross(p - a);
    const double d2 = (c - b).cross(p - b);
    const double d3 = (a - c).cross(p - c);
    const bool hasNegative = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    const bool hasPositive = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    return (hasNegative && hasPositive) ? Containment::Outside : Containment::Inside;
}

std::array<double, 3> Triangle2::barycentric(const Point2& p, double twiceArea) const
{
    const double wb = (p - a).cross(c - a) / twiceArea;
    const double wc = (b - a).cross(p - a) / twiceArea;
    return std::array<double, 3>{1.0 - wb - wc, wb, wc};
}

std::optional<std::array<double, 3>> Triangle2::barycentric(const Point2& p) const
{
    if (isDegenerate()) {
        return std::nullopt;
    }
    return barycentric(p, twiceSignedArea());
}

std::optional<Circle2> Triangle2::circumcircle() const
{
    if (isDegenerate()) {
        return std::nullopt;
    }
    // Solved relative to `a` for conditioning.
    const Vec2 ab = b - a;
    const Vec2 ac = c - a;
    const double d = 2.0 * ab.cross(ac);
    const double abSq = ab.lengthSquared();
    const double acSq = ac.lengthSquared();
    const Vec2 offset((ac.y * abSq - ab.y * acSq) / d, (ab.x * acSq - ac.x * abSq) / d);
    return Circle2{a + offset, offset.length()};
}

} // namespace katana::geometry
