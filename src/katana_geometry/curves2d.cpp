#include "katana/geometry/curves2d.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <limits>
#include <string>
#include <utility>

#include "katana/geometry/chording.hpp"
#include "katana/math/numerics.hpp"

namespace katana::geometry {

namespace tol = katana::math::tolerance;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;
using katana::math::kHalfPi;
using katana::math::kPi;
using katana::math::kTwoPi;
using katana::math::normalizeAngle;

namespace {

// A bulge smaller than this is a straight segment. tan(sweep/4) = 1e-12 is an
// arc whose sagitta on a 1 km chord is a quarter of a nanometre.
constexpr double kStraightBulge = 1.0e-12;

// Chops [t0, t1] of a parametric curve into chords whose midpoint and quarter
// points lie within `tolerance` of the curve, appending every point after
// the first to `out`. The quarter points are asked as well as the midpoint
// because an S-shaped piece can pass through its own chord's middle while
// departing from it on either side. The depth cap bounds the work on a
// pathological curve (a cusp) to 2^14 chords per starting piece.
template <typename Evaluate>
void subdivide(const Evaluate& at, double t0, const Point2& p0, double t1, const Point2& p1,
               double tolerance, int depth, std::vector<Point2>& out)
{
    const double tm = 0.5 * (t0 + t1);
    const Point2 pm = at(tm);
    const Segment2 chord{p0, p1};
    bool flat = chord.distanceTo(pm) <= tolerance;
    if (flat) {
        flat = chord.distanceTo(at(0.5 * (t0 + tm))) <= tolerance &&
               chord.distanceTo(at(0.5 * (tm + t1))) <= tolerance;
    }
    if (flat || depth >= 14) {
        out.push_back(p1);
        return;
    }
    subdivide(at, t0, p0, tm, pm, tolerance, depth + 1, out);
    subdivide(at, tm, pm, t1, p1, tolerance, depth + 1, out);
}

template <typename Evaluate>
void tessellateRange(const Evaluate& at, double t0, double t1, int pieces, double tolerance,
                     std::vector<Point2>& out)
{
    const double safeTolerance = std::max(tolerance, 1.0e-9);
    Point2 previous = at(t0);
    if (out.empty()) {
        out.push_back(previous);
    }
    for (int i = 1; i <= pieces; ++i) {
        const double a = t0 + (t1 - t0) * (i - 1) / pieces;
        const double b = i == pieces ? t1 : t0 + (t1 - t0) * i / pieces;
        const Point2 next = at(b);
        subdivide(at, a, previous, b, next, safeTolerance, 0, out);
        previous = next;
    }
}

Point2 nearestOnChain(const std::vector<Point2>& chain, const Point2& p)
{
    if (chain.size() == 1) {
        return chain.front();
    }
    Point2 best = chain.front();
    double bestDistance = std::numeric_limits<double>::infinity();
    for (std::size_t i = 1; i < chain.size(); ++i) {
        const Segment2 piece{chain[i - 1], chain[i]};
        const Point2 candidate = piece.closestPoint(p);
        const double d = candidate.distanceTo(p);
        if (d < bestDistance) {
            bestDistance = d;
            best = candidate;
        }
    }
    return best;
}

double chainLength(const std::vector<Point2>& chain)
{
    double total = 0.0;
    for (std::size_t i = 1; i < chain.size(); ++i) {
        total += chain[i - 1].distanceTo(chain[i]);
    }
    return total;
}

Box2 boxOf(const CurveSegment& segment)
{
    return std::visit([](const auto& piece) { return piece.boundingBox(); }, segment);
}

} // namespace

// ---- bulges ------------------------------------------------------------------------

double bulgeFromSweep(double sweep) { return std::tan(sweep / 4.0); }

std::optional<Arc2> arcFromBulge(const Point2& start, const Point2& end, double bulge)
{
    const Vec2 chord = end - start;
    const double c = chord.length();
    if (!(std::abs(bulge) > kStraightBulge) || !(c > tol::kGeometric) || !std::isfinite(bulge)) {
        return std::nullopt;
    }
    // sin(sweep/2) = 2B / (1 + B^2), so r = c / (2 sin(sweep/2)); the centre
    // is r cos(sweep/2) from the chord's middle, to the left of the chord for
    // a counter-clockwise arc of less than half a turn.
    const double radius = c * (1.0 + bulge * bulge) / (4.0 * std::abs(bulge));
    const double offset = c * (1.0 - bulge * bulge) / (4.0 * bulge);
    const Point2 centre = (start + end) * 0.5 + chord.perpendicular() * (offset / c);
    return Arc2{centre, radius, (start - centre).angle(), 4.0 * std::atan(bulge)};
}

double bulgeThrough(const Point2& start, const Point2& through, const Point2& end)
{
    const auto arc = Arc2::throughPoints(start, through, end);
    if (!arc) {
        return 0.0;
    }
    return bulgeFromSweep(arc->sweep);
}

// ---- CurvePolyline2 -------------------------------------------------------------------

CurvePolyline2 CurvePolyline2::fromPolyline(const Polyline2& polyline,
                                            const std::vector<std::optional<double>>& heights)
{
    CurvePolyline2 out;
    out.closed = polyline.closed;
    out.vertices.reserve(polyline.vertices.size());
    const bool withHeights = heights.size() == polyline.vertices.size();
    for (std::size_t i = 0; i < polyline.vertices.size(); ++i) {
        out.vertices.push_back(
            CurveVertex{polyline.vertices[i], 0.0, withHeights ? heights[i] : std::nullopt});
    }
    return out;
}

CurvePolyline2 CurvePolyline2::fromPoints(const std::vector<Point2>& points, bool isClosed)
{
    CurvePolyline2 out;
    out.closed = isClosed;
    out.vertices.reserve(points.size());
    for (const Point2& p : points) {
        out.vertices.push_back(CurveVertex{p, 0.0, std::nullopt});
    }
    return out;
}

std::size_t CurvePolyline2::segmentCount() const
{
    if (vertices.size() < 2) {
        return 0;
    }
    return closed ? vertices.size() : vertices.size() - 1;
}

std::size_t CurvePolyline2::segmentEnd(std::size_t index) const
{
    return index + 1 < vertices.size() ? index + 1 : 0;
}

bool CurvePolyline2::isArc(std::size_t index) const
{
    return arcFromBulge(vertices[index].position, vertices[segmentEnd(index)].position,
                        vertices[index].bulge)
        .has_value();
}

CurveSegment CurvePolyline2::segment(std::size_t index) const
{
    const Point2& a = vertices[index].position;
    const Point2& b = vertices[segmentEnd(index)].position;
    if (const auto arc = arcFromBulge(a, b, vertices[index].bulge)) {
        return *arc;
    }
    return Segment2{a, b};
}

double CurvePolyline2::segmentLength(std::size_t index) const
{
    return std::visit([](const auto& piece) { return piece.length(); }, segment(index));
}

bool CurvePolyline2::hasArcs() const
{
    for (std::size_t i = 0; i < segmentCount(); ++i) {
        if (isArc(i)) {
            return true;
        }
    }
    return false;
}

bool CurvePolyline2::hasHeights() const
{
    return std::any_of(vertices.begin(), vertices.end(),
                       [](const CurveVertex& v) { return v.height.has_value(); });
}

std::vector<Point2> CurvePolyline2::positions() const
{
    std::vector<Point2> out;
    out.reserve(vertices.size());
    for (const CurveVertex& v : vertices) {
        out.push_back(v.position);
    }
    return out;
}

std::vector<std::optional<double>> CurvePolyline2::heights() const
{
    std::vector<std::optional<double>> out;
    out.reserve(vertices.size());
    for (const CurveVertex& v : vertices) {
        out.push_back(v.height);
    }
    return out;
}

double CurvePolyline2::length() const
{
    double total = 0.0;
    for (std::size_t i = 0; i < segmentCount(); ++i) {
        total += segmentLength(i);
    }
    return total;
}

double CurvePolyline2::signedArea() const
{
    if (!closed || vertices.size() < 2) {
        return 0.0;
    }
    // Shoelace relative to the first vertex (as Polyline2 does, for the
    // precision of projected coordinates), plus each arc's circular segment:
    // r^2 / 2 (sweep - sin sweep), positive for a counter-clockwise arc,
    // which bulges to the right of its chord - outward of a counter-
    // clockwise ring.
    const Point2 origin = vertices.front().position;
    double twice = 0.0;
    for (std::size_t i = 0; i < vertices.size(); ++i) {
        const Vec2 a = vertices[i].position - origin;
        const Vec2 b = vertices[segmentEnd(i)].position - origin;
        twice += a.cross(b);
    }
    double area = 0.5 * twice;
    for (std::size_t i = 0; i < segmentCount(); ++i) {
        const CurveSegment piece = segment(i);
        if (const auto* arc = std::get_if<Arc2>(&piece)) {
            area += 0.5 * arc->radius * arc->radius * (arc->sweep - std::sin(arc->sweep));
        }
    }
    return area;
}

double CurvePolyline2::area() const { return std::abs(signedArea()); }

std::optional<CurvePolyline2::Nearest> CurvePolyline2::nearest(const Point2& p) const
{
    if (vertices.empty()) {
        return std::nullopt;
    }
    Nearest best{vertices.front().position, 0.0, 0, vertices.front().position.distanceTo(p)};
    double station = 0.0;
    for (std::size_t i = 0; i < segmentCount(); ++i) {
        const CurveSegment piece = segment(i);
        Point2 candidate;
        double along = 0.0;
        if (const auto* line = std::get_if<Segment2>(&piece)) {
            const double t = line->parameterOf(p);
            candidate = line->pointAt(t);
            along = t * line->length();
        } else {
            const Arc2& arc = std::get<Arc2>(piece);
            candidate = arc.closestPoint(p);
            along = arc.parameterOfAngle((candidate - arc.center).angle()) * arc.length();
        }
        const double d = candidate.distanceTo(p);
        if (d < best.distance) {
            best = Nearest{candidate, station + along, i, d};
        }
        station += std::visit([](const auto& s) { return s.length(); }, piece);
    }
    return best;
}

std::optional<Point2> CurvePolyline2::closestPoint(const Point2& p) const
{
    const auto found = nearest(p);
    return found ? std::optional<Point2>(found->point) : std::nullopt;
}

std::optional<double> CurvePolyline2::distanceTo(const Point2& p) const
{
    const auto found = nearest(p);
    return found ? std::optional<double>(found->distance) : std::nullopt;
}

double CurvePolyline2::stationOfVertex(std::size_t index) const
{
    double station = 0.0;
    for (std::size_t i = 0; i < std::min(index, segmentCount()); ++i) {
        station += segmentLength(i);
    }
    return station;
}

Point2 CurvePolyline2::pointAtStation(double station) const
{
    if (vertices.empty()) {
        return Point2{};
    }
    double walked = 0.0;
    for (std::size_t i = 0; i < segmentCount(); ++i) {
        const CurveSegment piece = segment(i);
        const double length = std::visit([](const auto& s) { return s.length(); }, piece);
        if (station <= walked + length || i + 1 == segmentCount()) {
            const double t = length > 0.0 ? std::clamp((station - walked) / length, 0.0, 1.0) : 0.0;
            return std::visit([t](const auto& s) { return s.pointAt(t); }, piece);
        }
        walked += length;
    }
    return vertices.front().position;
}

std::optional<double> CurvePolyline2::heightAtStation(double station) const
{
    if (vertices.empty()) {
        return std::nullopt;
    }
    if (segmentCount() == 0) {
        return vertices.front().height;
    }
    double walked = 0.0;
    for (std::size_t i = 0; i < segmentCount(); ++i) {
        const double length = segmentLength(i);
        if (station <= walked + length || i + 1 == segmentCount()) {
            const auto& h0 = vertices[i].height;
            const auto& h1 = vertices[segmentEnd(i)].height;
            if (!h0 || !h1) {
                return std::nullopt;
            }
            const double t = length > 0.0 ? std::clamp((station - walked) / length, 0.0, 1.0) : 0.0;
            return *h0 + (*h1 - *h0) * t;
        }
        walked += length;
    }
    return std::nullopt;
}

Box2 CurvePolyline2::boundingBox() const
{
    Box2 box;
    for (const CurveVertex& v : vertices) {
        box.expand(v.position);
    }
    for (std::size_t i = 0; i < segmentCount(); ++i) {
        if (isArc(i)) {
            box.expand(boxOf(segment(i)));
        }
    }
    return box;
}

CurvePolyline2 CurvePolyline2::reversed() const
{
    CurvePolyline2 out;
    out.closed = closed;
    const std::size_t n = vertices.size();
    out.vertices.reserve(n);
    for (std::size_t k = 0; k < n; ++k) {
        CurveVertex v = vertices[n - 1 - k];
        if (k + 1 < n) {
            // The segment now starting at new vertex k is the old segment
            // n-2-k walked backwards.
            v.bulge = -vertices[n - 2 - k].bulge;
        } else {
            // The closing segment now runs from the old first vertex to the
            // old last: the old closing segment backwards.
            v.bulge = closed ? -vertices[n - 1].bulge : 0.0;
        }
        out.vertices.push_back(v);
    }
    return out;
}

std::vector<Point2> CurvePolyline2::tessellate(double tolerance) const
{
    std::vector<Point2> out;
    if (vertices.empty()) {
        return out;
    }
    out.push_back(vertices.front().position);
    for (std::size_t i = 0; i < segmentCount(); ++i) {
        const CurveSegment piece = segment(i);
        if (const auto* arc = std::get_if<Arc2>(&piece)) {
            const std::vector<Point2> chords = chordArc(*arc, tolerance);
            for (std::size_t k = 1; k < chords.size(); ++k) {
                out.push_back(chords[k]);
            }
            // The arc's own end is computed from its centre; the vertex is
            // the truth, so the last chord ends exactly on it.
            if (chords.size() > 1) {
                out.back() = vertices[segmentEnd(i)].position;
            }
        } else {
            out.push_back(vertices[segmentEnd(i)].position);
        }
    }
    return out;
}

Polyline2 CurvePolyline2::toPolyline(double tolerance) const
{
    if (!hasArcs()) {
        return Polyline2{positions(), closed};
    }
    std::vector<Point2> points = tessellate(tolerance);
    if (closed && points.size() > 1) {
        points.pop_back();
    }
    return Polyline2{std::move(points), closed};
}

std::vector<CurveSegment> CurvePolyline2::segments() const
{
    std::vector<CurveSegment> out;
    out.reserve(segmentCount());
    for (std::size_t i = 0; i < segmentCount(); ++i) {
        out.push_back(segment(i));
    }
    return out;
}

// ---- Ellipse2 -------------------------------------------------------------------------

bool Ellipse2::isFull() const { return sweep >= kTwoPi - tol::kAngular; }

Point2 Ellipse2::pointAtParameter(double t) const
{
    return center + majorAxis * std::cos(t) + minorAxis() * std::sin(t);
}

Vec2 Ellipse2::derivativeAtParameter(double t) const
{
    return majorAxis * -std::sin(t) + minorAxis() * std::cos(t);
}

double Ellipse2::parameterTowards(const Point2& p) const
{
    const double a = majorRadius();
    const double b = minorRadius();
    if (!(a > 0.0) || !(b > 0.0)) {
        return 0.0;
    }
    const Vec2 d = p - center;
    const Vec2 unitMajor = majorAxis / a;
    const double x = d.dot(unitMajor);
    const double y = d.dot(unitMajor.perpendicular());
    return std::atan2(y / b, x / a);
}

bool Ellipse2::containsParameter(double t, double tolerance) const
{
    if (isFull()) {
        return true;
    }
    const double travelled = normalizeAngle(t - startParameter);
    return travelled <= sweep + tolerance || travelled >= kTwoPi - tolerance;
}

std::vector<Point2> Ellipse2::quadrants() const
{
    std::vector<Point2> out;
    for (int k = 0; k < 4; ++k) {
        const double t = kHalfPi * k;
        if (containsParameter(t, tol::kAngular)) {
            out.push_back(pointAtParameter(t));
        }
    }
    return out;
}

double Ellipse2::length() const
{
    // Five-point Gauss-Legendre on 64 panels: the integrand |P'(t)| is smooth
    // and periodic, and this agrees with the Gauss-Kummer series to 1e-9 m
    // on a 10 m ellipse of ratio 0.3 (test_curves2d.cpp).
    static constexpr std::array<double, 5> nodes = {
        0.0, -0.5384693101056831, 0.5384693101056831, -0.9061798459386640, 0.9061798459386640};
    static constexpr std::array<double, 5> weightsGl = {
        0.5688888888888889, 0.4786286704993665, 0.4786286704993665, 0.2369268850561891,
        0.2369268850561891};
    constexpr int panels = 64;
    const double h = sweep / panels;
    double total = 0.0;
    for (int i = 0; i < panels; ++i) {
        const double mid = startParameter + h * (i + 0.5);
        for (std::size_t k = 0; k < nodes.size(); ++k) {
            total += weightsGl[k] * derivativeAtParameter(mid + 0.5 * h * nodes[k]).length();
        }
    }
    return total * 0.5 * h;
}

Box2 Ellipse2::boundingBox() const
{
    Box2 box;
    box.expand(startPoint());
    box.expand(endPoint());
    const Vec2 minor = minorAxis();
    // x(t) = cx + Mx cos t + mx sin t is extreme where tan t = mx / Mx, and
    // likewise for y; each gives two opposite parameters.
    const double tx = std::atan2(minor.x, majorAxis.x);
    const double ty = std::atan2(minor.y, majorAxis.y);
    for (const double t : {tx, tx + kPi, ty, ty + kPi}) {
        if (containsParameter(t)) {
            box.expand(pointAtParameter(t));
        }
    }
    return box;
}

double Ellipse2::closestParameter(const Point2& p) const
{
    constexpr int samples = 72;
    double bestT = startParameter;
    double bestD = std::numeric_limits<double>::infinity();
    for (int i = 0; i <= samples; ++i) {
        const double t = startParameter + sweep * i / samples;
        const double d = pointAtParameter(t).distanceTo(p);
        if (d < bestD) {
            bestD = d;
            bestT = t;
        }
    }
    // Newton on f(t) = (P(t) - p) . P'(t), the tangency condition, kept inside
    // the arc; a step that does not improve the distance ends the search.
    const double lo = startParameter;
    const double hi = startParameter + sweep;
    double t = bestT;
    for (int iteration = 0; iteration < 30; ++iteration) {
        const Vec2 r = pointAtParameter(t) - p;
        const Vec2 d1 = derivativeAtParameter(t);
        const Vec2 d2 = (pointAtParameter(t) - center) * -1.0;
        const double f = r.dot(d1);
        const double df = d1.dot(d1) + r.dot(d2);
        if (!(std::abs(df) > 0.0)) {
            break;
        }
        double next = t - f / df;
        if (!isFull()) {
            next = std::clamp(next, lo, hi);
        }
        if (pointAtParameter(next).distanceTo(p) > pointAtParameter(t).distanceTo(p) + 1e-15) {
            break;
        }
        const bool converged = std::abs(next - t) < 1e-14;
        t = next;
        if (converged) {
            break;
        }
    }
    return t;
}

Point2 Ellipse2::closestPoint(const Point2& p) const
{
    return pointAtParameter(closestParameter(p));
}

double Ellipse2::distanceTo(const Point2& p) const { return closestPoint(p).distanceTo(p); }

std::vector<Point2> Ellipse2::tessellate(double tolerance) const
{
    std::vector<Point2> out;
    const auto at = [this](double t) { return pointAtParameter(t); };
    const int pieces = std::max(4, static_cast<int>(std::ceil(sweep / (kHalfPi / 4.0))));
    tessellateRange(at, startParameter, startParameter + sweep, pieces, tolerance, out);
    return out;
}

std::optional<Ellipse2> Ellipse2::fromAxes(const Point2& centre, const Point2& axisEnd,
                                           double otherRadius)
{
    const Vec2 axis = axisEnd - centre;
    const double a = axis.length();
    if (!(a > tol::kGeometric) || !(otherRadius > tol::kGeometric) || !std::isfinite(otherRadius)) {
        return std::nullopt;
    }
    Ellipse2 out;
    out.center = centre;
    if (otherRadius <= a) {
        out.majorAxis = axis;
        out.ratio = otherRadius / a;
    } else {
        out.majorAxis = axis.perpendicular() * (otherRadius / a);
        out.ratio = a / otherRadius;
    }
    return out;
}

// ---- Spline2 --------------------------------------------------------------------------

namespace {

// The knot span index k with knots[k] <= u < knots[k+1], within [p, n-1]
// (The NURBS Book, A2.1), n the control point count.
std::size_t findSpan(const std::vector<double>& knots, int p, std::size_t n, double u)
{
    const std::size_t pp = static_cast<std::size_t>(p);
    if (u >= knots[n]) {
        return n - 1;
    }
    if (u <= knots[pp]) {
        // The first span that is not empty.
        std::size_t k = pp;
        while (k + 1 < n && knots[k + 1] <= u) {
            ++k;
        }
        return k;
    }
    std::size_t low = pp;
    std::size_t high = n;
    std::size_t mid = (low + high) / 2;
    while (u < knots[mid] || u >= knots[mid + 1]) {
        if (u < knots[mid]) {
            high = mid;
        } else {
            low = mid;
        }
        mid = (low + high) / 2;
    }
    return mid;
}

// The p+1 non-zero basis functions at u in span k (The NURBS Book, A2.2).
std::vector<double> basisFunctions(const std::vector<double>& knots, int p, std::size_t k,
                                   double u)
{
    const std::size_t pp = static_cast<std::size_t>(p);
    std::vector<double> basis(pp + 1, 0.0);
    std::vector<double> left(pp + 1, 0.0);
    std::vector<double> right(pp + 1, 0.0);
    basis[0] = 1.0;
    for (std::size_t j = 1; j <= pp; ++j) {
        left[j] = u - knots[k + 1 - j];
        right[j] = knots[k + j] - u;
        double saved = 0.0;
        for (std::size_t r = 0; r < j; ++r) {
            const double denominator = right[r + 1] + left[j - r];
            const double temp = denominator != 0.0 ? basis[r] / denominator : 0.0;
            basis[r] = saved + right[r + 1] * temp;
            saved = left[j - r] * temp;
        }
        basis[j] = saved;
    }
    return basis;
}

std::vector<double> clampedUniformKnots(std::size_t count, int p)
{
    const std::size_t pp = static_cast<std::size_t>(p);
    std::vector<double> knots;
    knots.reserve(count + pp + 1);
    for (std::size_t i = 0; i <= pp; ++i) {
        knots.push_back(0.0);
    }
    const std::size_t inner = count - pp - 1;
    for (std::size_t i = 1; i <= inner; ++i) {
        knots.push_back(static_cast<double>(i) / static_cast<double>(inner + 1));
    }
    for (std::size_t i = 0; i <= pp; ++i) {
        knots.push_back(1.0);
    }
    return knots;
}

std::vector<Point2> withoutRepeats(std::vector<Point2> points)
{
    std::vector<Point2> out;
    out.reserve(points.size());
    for (const Point2& p : points) {
        if (out.empty() || out.back().distanceTo(p) > tol::kGeometric) {
            out.push_back(p);
        }
    }
    return out;
}

} // namespace

Result<Spline2> Spline2::fromControlPoints(std::vector<Point2> points, int order)
{
    points = withoutRepeats(std::move(points));
    if (points.size() < 2) {
        return makeError(ErrorCode::InvalidArgument,
                         "a spline needs at least two different control points");
    }
    if (order < 1) {
        return makeError(ErrorCode::InvalidArgument, "a spline's degree must be at least 1");
    }
    Spline2 out;
    out.degree = std::min(order, static_cast<int>(points.size()) - 1);
    out.knots = clampedUniformKnots(points.size(), out.degree);
    out.controlPoints = std::move(points);
    return out;
}

Result<Spline2> Spline2::throughPoints(std::vector<Point2> points, int order)
{
    points = withoutRepeats(std::move(points));
    const std::size_t m = points.size();
    if (m < 2) {
        return makeError(ErrorCode::InvalidArgument,
                         "a spline needs at least two different points to pass through");
    }
    if (order < 1) {
        return makeError(ErrorCode::InvalidArgument, "a spline's degree must be at least 1");
    }
    const int p = std::min(order, static_cast<int>(m) - 1);
    const std::size_t pp = static_cast<std::size_t>(p);

    // Chord-length parameters, then knots averaged over them, so that every
    // knot span holds at least one parameter and the system is non-singular
    // (Schoenberg-Whitney).
    std::vector<double> params(m, 0.0);
    double total = 0.0;
    for (std::size_t k = 1; k < m; ++k) {
        total += points[k].distanceTo(points[k - 1]);
    }
    double walked = 0.0;
    for (std::size_t k = 1; k + 1 < m; ++k) {
        walked += points[k].distanceTo(points[k - 1]);
        params[k] = walked / total;
    }
    params[m - 1] = 1.0;

    std::vector<double> knots(m + pp + 1, 0.0);
    for (std::size_t i = m; i < m + pp + 1; ++i) {
        knots[i] = 1.0;
    }
    for (std::size_t j = 1; j + pp < m; ++j) {
        double sum = 0.0;
        for (std::size_t i = j; i < j + pp; ++i) {
            sum += params[i];
        }
        knots[j + pp] = sum / static_cast<double>(p);
    }

    // The collocation matrix is banded (each row's non-zeros lie in the p+1
    // columns of its span) and totally positive, so elimination without
    // pivoting is stable and stays within the band (de Boor). Stored as
    // rows of width 2p+1 centred on the diagonal.
    const std::size_t width = 2 * pp + 1;
    std::vector<std::vector<double>> band(m, std::vector<double>(width, 0.0));
    const auto at = [&](std::size_t row, std::size_t column) -> double& {
        return band[row][column + pp - row];
    };
    for (std::size_t k = 0; k < m; ++k) {
        const std::size_t span = findSpan(knots, p, m, params[k]);
        const std::vector<double> basis = basisFunctions(knots, p, span, params[k]);
        for (std::size_t i = 0; i <= pp; ++i) {
            const std::size_t column = span - pp + i;
            if (column + pp >= k && column <= k + pp) {
                at(k, column) = basis[i];
            }
        }
    }
    std::vector<Point2> rhs = points;
    for (std::size_t k = 0; k < m; ++k) {
        const double pivot = at(k, k);
        if (!(std::abs(pivot) > 1e-300)) {
            return makeError(ErrorCode::InvalidGeometry,
                             "the points cannot be interpolated by a spline");
        }
        for (std::size_t i = k + 1; i < std::min(m, k + pp + 1); ++i) {
            const double factor = at(i, k) / pivot;
            if (factor == 0.0) {
                continue;
            }
            for (std::size_t j = k; j < std::min(m, k + pp + 1); ++j) {
                at(i, j) -= factor * at(k, j);
            }
            rhs[i] = rhs[i] - rhs[k] * factor;
        }
    }
    std::vector<Point2> control(m);
    for (std::size_t kk = m; kk-- > 0;) {
        Point2 sum = rhs[kk];
        for (std::size_t j = kk + 1; j < std::min(m, kk + pp + 1); ++j) {
            sum = sum - control[j] * at(kk, j);
        }
        control[kk] = sum / at(kk, kk);
    }

    Spline2 out;
    out.degree = p;
    out.controlPoints = std::move(control);
    out.knots = std::move(knots);
    out.fitPoints = std::move(points);
    return out;
}

double Spline2::domainStart() const
{
    return knots.empty() ? 0.0 : knots[static_cast<std::size_t>(std::max(degree, 0))];
}

double Spline2::domainEnd() const
{
    return knots.size() > controlPoints.size() ? knots[controlPoints.size()] : 0.0;
}

Point2 Spline2::pointAt(double u) const
{
    const std::size_t n = controlPoints.size();
    if (n == 0) {
        return Point2{};
    }
    if (!checkStructure()) {
        return controlPoints.front();
    }
    u = std::clamp(u, domainStart(), domainEnd());
    const std::size_t span = findSpan(knots, degree, n, u);
    const std::vector<double> basis = basisFunctions(knots, degree, span, u);
    const std::size_t pp = static_cast<std::size_t>(degree);
    double x = 0.0;
    double y = 0.0;
    double w = 0.0;
    for (std::size_t i = 0; i <= pp; ++i) {
        const std::size_t index = span - pp + i;
        const double weight = weights.empty() ? 1.0 : weights[index];
        x += basis[i] * weight * controlPoints[index].x;
        y += basis[i] * weight * controlPoints[index].y;
        w += basis[i] * weight;
    }
    return w != 0.0 ? Point2(x / w, y / w) : controlPoints[span];
}

bool Spline2::isClosedShape() const
{
    return !controlPoints.empty() && startPoint().distanceTo(endPoint()) <= tol::kGeometric;
}

std::vector<Point2> Spline2::tessellate(double tolerance) const
{
    std::vector<Point2> out;
    if (!checkStructure()) {
        return out;
    }
    const auto at = [this](double u) { return pointAt(u); };
    const std::size_t n = controlPoints.size();
    const std::size_t pp = static_cast<std::size_t>(degree);
    for (std::size_t k = pp; k < n; ++k) {
        if (knots[k + 1] > knots[k]) {
            tessellateRange(at, knots[k], knots[k + 1], degree + 1, tolerance, out);
        }
    }
    return out;
}

Polyline2 Spline2::toPolyline(double tolerance) const
{
    std::vector<Point2> points = tessellate(tolerance);
    const bool shut = points.size() > 3 && points.front().distanceTo(points.back()) <= tol::kGeometric;
    if (shut) {
        points.pop_back();
    }
    return Polyline2{std::move(points), shut};
}

double Spline2::length() const { return chainLength(tessellate(kCurveChordTolerance * 1.0e-3)); }

Box2 Spline2::boundingBox() const
{
    Box2 box;
    for (const Point2& p : controlPoints) {
        box.expand(p);
    }
    return box;
}

Point2 Spline2::closestPoint(const Point2& p) const
{
    const std::vector<Point2> chain = tessellate(kCurveChordTolerance * 1.0e-2);
    if (chain.empty()) {
        return controlPoints.empty() ? Point2{} : controlPoints.front();
    }
    return nearestOnChain(chain, p);
}

double Spline2::distanceTo(const Point2& p) const { return closestPoint(p).distanceTo(p); }

Spline2 Spline2::reversed() const
{
    Spline2 out = *this;
    std::reverse(out.controlPoints.begin(), out.controlPoints.end());
    std::reverse(out.weights.begin(), out.weights.end());
    std::reverse(out.fitPoints.begin(), out.fitPoints.end());
    if (!knots.empty()) {
        const double a = knots.front();
        const double b = knots.back();
        for (std::size_t i = 0; i < knots.size(); ++i) {
            out.knots[i] = a + b - knots[knots.size() - 1 - i];
        }
    }
    return out;
}

Status Spline2::checkStructure() const
{
    if (degree < 1 || degree > 10) {
        return makeError(ErrorCode::InvalidGeometry, "spline degree must be from 1 to 10",
                         std::to_string(degree));
    }
    const std::size_t pp = static_cast<std::size_t>(degree);
    if (controlPoints.size() < pp + 1) {
        return makeError(ErrorCode::InvalidGeometry,
                         "spline has too few control points for its degree");
    }
    if (knots.size() != controlPoints.size() + pp + 1) {
        return makeError(ErrorCode::InvalidGeometry,
                         "spline knot count must be control points + degree + 1");
    }
    if (!weights.empty() && weights.size() != controlPoints.size()) {
        return makeError(ErrorCode::InvalidGeometry,
                         "spline must have no weights or one per control point");
    }
    for (std::size_t i = 0; i < knots.size(); ++i) {
        if (!std::isfinite(knots[i]) || (i > 0 && knots[i] < knots[i - 1])) {
            return makeError(ErrorCode::InvalidGeometry, "spline knots must be finite and "
                                                         "non-decreasing");
        }
    }
    for (const double w : weights) {
        if (!(std::isfinite(w) && w > 0.0)) {
            return makeError(ErrorCode::InvalidGeometry, "spline weights must be positive");
        }
    }
    if (!(domainEnd() > domainStart())) {
        return makeError(ErrorCode::InvalidGeometry, "spline has an empty parameter range");
    }
    return {};
}

} // namespace katana::geometry
