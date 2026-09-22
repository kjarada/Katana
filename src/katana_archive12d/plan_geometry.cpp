#include "plan_geometry.hpp"

#include <algorithm>
#include <cmath>
#include <numbers>

#include "katana/geometry/chording.hpp"
#include "katana/geometry/spiral2.hpp"
#include "text_utilities.hpp"

namespace katana::archive12d::detail {

namespace {

using katana::geometry::Point2;
using katana::geometry::Spiral2;
using katana::geometry::Vec2;

constexpr double kPi = std::numbers::pi;

// A computed transition that ends further than this from the vertex 12d
// recorded is not an approximation of it, it is a misreading: the segment is
// drawn straight and counted. One metre, or 5% of the length if that is more -
// the worst honest case measured (a clothoid standing in for a cubic parabola
// of L = 80, R = 210) misses by 0.28 m, 0.35% - but never more than the chord
// itself: a curve that misses by more than the distance between its own ends
// is nowhere near, whatever its length claims.
[[nodiscard]] double closureLimit(double length, double chord)
{
    return std::min(std::max(1.0, 0.05 * length), chord);
}

// A transition between two vertices is at least as long as the chord between
// them, and not many times longer: a spiral turning through less than a half
// turn is under pi/2 chords long, and a transition is never a half turn. Ten
// chords is far outside anything real and inside nothing else.
[[nodiscard]] bool plausibleTransitionLength(double length, double chord, double tolerance)
{
    return std::isfinite(length) && std::isfinite(chord) && chord > 0.0 &&
           length >= chord - tolerance && length <= 10.0 * chord;
}

// A transition moved by less than this to meet its recorded end is not worth
// a warning: a millimetre is what the coordinates of a real export are good
// to, and the two types that are computed exactly - the cubic parabola, and
// the clothoid, of which 12d's own "clothoid" is a series approximation -
// close far inside it. What is left above it is a genuinely approximated type.
constexpr double kReportableAdjustment = 0.001;

[[nodiscard]] double radians(double degrees)
{
    return degrees * kPi / 180.0;
}

// 12d's radius to the curvature Spiral2 uses: a radius of 0 means a straight,
// and the hands are opposite - 12d's positive turns right, Spiral2's positive
// turns left.
[[nodiscard]] double curvatureOf(double radius)
{
    return radius == 0.0 ? 0.0 : -1.0 / radius;
}

// Arc length of y = m x^3 over [0, X], composite Simpson. The integrand
// sqrt(1 + 9 m^2 x^4) is smooth and within a few percent of 1 for any real
// transition, so 64 intervals are good to far beyond a double's digits.
[[nodiscard]] double cubicArcLength(double m, double X)
{
    constexpr int kIntervals = 64;
    const double h = X / kIntervals;
    const auto f = [m](double x) { return std::sqrt(1.0 + 9.0 * m * m * x * x * x * x); };
    double sum = f(0.0) + f(X);
    for (int i = 1; i < kIntervals; ++i) {
        sum += f(i * h) * (i % 2 == 1 ? 4.0 : 2.0);
    }
    return sum * h / 3.0;
}

// The m for which the true curvature of y = m x^3 at x = X is 1/R, on the
// rising branch (slope below 1/sqrt 2). Negative when there is none.
[[nodiscard]] double cubicCoefficient(double radius, double X)
{
    // In terms of the end slope t = 3 m X^2 the condition is
    // t / (1 + t^2)^1.5 = X / (2R), and the left side rises on [0, 1/sqrt 2].
    const double target = X / (2.0 * radius);
    const double tMax = 1.0 / std::numbers::sqrt2;
    if (target > tMax / std::pow(1.0 + tMax * tMax, 1.5)) {
        return -1.0;
    }
    double low = 0.0;
    double high = tMax;
    for (int i = 0; i < 100; ++i) {
        const double t = 0.5 * (low + high);
        (t / std::pow(1.0 + t * t, 1.5) < target ? low : high) = t;
    }
    return 0.5 * (low + high) / (3.0 * X * X);
}

void pushInterior(std::vector<PlanPoint>& out, std::vector<Point2> points, std::size_t segment,
                  bool reversed)
{
    // `points` runs from the described start to the described end, inclusive
    // of both; only what lies strictly between the vertices is new.
    const std::size_t count = points.size();
    if (count < 3) {
        return;
    }
    if (reversed) {
        std::reverse(points.begin(), points.end());
    }
    for (std::size_t i = 1; i + 1 < count; ++i) {
        out.push_back(PlanPoint{points[i], segment,
                                static_cast<double>(i) / static_cast<double>(count - 1)});
    }
}

// Moves a computed curve so that its last point is `target`, the correction
// growing linearly along it. The first point stays put.
void closeOnto(std::vector<Point2>& points, const Point2& target)
{
    const Vec2 miss = target - points.back();
    const double last = static_cast<double>(points.size() - 1);
    for (std::size_t i = 1; i < points.size(); ++i) {
        points[i] = points[i] + miss * (static_cast<double>(i) / last);
    }
}

void chordArcSegment(const Point2& a, const Point2& b, const Segment& segment, double tolerance,
                     std::size_t index, std::vector<PlanPoint>& out, ChordReport& report)
{
    ArcCircle arc;
    if (!solveArc(a, b, segment, tolerance, arc)) {
        // Coincident vertices or a radius of nothing are simply no arc; a
        // radius too small for its chord is a segment that was not reproduced.
        if (a.distanceTo(b) > 0.0 && segment.radius != 0.0) {
            ++report.straightened;
        }
        return;
    }
    ++report.arcs;
    const std::size_t chords =
        katana::geometry::sagittaChordCount(arc.radius, arc.sweep, tolerance);
    for (std::size_t i = 1; i < chords; ++i) {
        const double fraction = static_cast<double>(i) / static_cast<double>(chords);
        const double angle = arc.startAngle + arc.sweep * fraction;
        out.push_back(PlanPoint{Point2(arc.centre.x + arc.radius * std::cos(angle),
                                       arc.centre.y + arc.radius * std::sin(angle)),
                                index, fraction});
    }
}

void chordSpiralSegment(const Point2& a, const Point2& b, const Segment& segment, double tolerance,
                        std::size_t index, std::vector<PlanPoint>& out, ChordReport& report)
{
    const FieldList& p = segment.parameters;
    const auto a1 = p.real("a1");
    const double l1 = p.real("l1").value_or(0.0);
    const double l2 = p.real("l2").value_or(0.0);
    const double r1 = p.real("r1").value_or(0.0);
    const double r2 = p.real("r2").value_or(0.0);
    const double length = std::fabs(l2 - l1);
    if (!a1 || !(length > 0.0) || !std::isfinite(*a1) ||
        !plausibleTransitionLength(length, a.distanceTo(b), tolerance)) {
        ++report.straightened;
        return;
    }
    // See the header: the block runs from its (l1, r1, a1) end, and `leading`
    // says whether that end is the segment's first vertex.
    const bool leading = p.boolean("leading").value_or(true);
    const Point2& origin = leading ? a : b;
    const Point2& target = leading ? b : a;
    const std::string type = lowered(p.text("type", "clothoid"));
    const double direction = radians(*a1);

    std::vector<Point2> points;
    CubicParabolaEnd cubic;
    if (type == "cubic parabola" && l1 == 0.0 && r1 == 0.0 && r2 != 0.0 &&
        solveCubicParabola(std::fabs(r2), length, cubic)) {
        const double turn = std::atan(3.0 * cubic.coefficient * cubic.abscissa * cubic.abscissa);
        const std::size_t chords = std::max<std::size_t>(
            2, katana::geometry::sagittaChordCount(std::fabs(r2), turn, tolerance));
        const double hand = r2 > 0.0 ? -1.0 : 1.0; // positive radius: to the right of the tangent
        const Vec2 along(std::cos(direction), std::sin(direction));
        const Vec2 left(-along.y, along.x);
        for (std::size_t i = 0; i <= chords; ++i) {
            const double x = cubic.abscissa * static_cast<double>(i) / static_cast<double>(chords);
            points.push_back(origin + along * x + left * (hand * cubic.coefficient * x * x * x));
        }
    } else {
        Spiral2 spiral;
        spiral.start = origin;
        spiral.startDirection = direction;
        spiral.startCurvature = curvatureOf(r1);
        spiral.endCurvature = curvatureOf(r2);
        spiral.length = length;
        const std::size_t chords = std::max<std::size_t>(2, spiral.chordCountFor(tolerance));
        for (std::size_t i = 0; i <= chords; ++i) {
            points.push_back(
                spiral.pointAt(length * static_cast<double>(i) / static_cast<double>(chords)));
        }
    }

    const double miss = points.back().distanceTo(target);
    if (!std::isfinite(miss) || miss > closureLimit(length, a.distanceTo(b))) {
        ++report.straightened;
        return;
    }
    if (miss > kReportableAdjustment) {
        double& worst = report.adjustmentByType[type];
        worst = std::max(worst, miss);
    }
    closeOnto(points, target);
    ++report.transitions;
    pushInterior(out, std::move(points), index, !leading);
}

// An offset transition (manual 1.5.8.2.2.4). The manual's own comments for
// `start` and `end` contradict its prose, it does not fix the hand of `offset`
// for a trailing transition, and no sample file holds one - so rather than
// pick a reading, every reading is tried and the one that lands on BOTH
// recorded vertices is taken. None landing means the segment is drawn straight
// and counted: a wrong curve drawn confidently is worse than a chord.
void chordCurveSegment(const Point2& a, const Point2& b, const Segment& segment, double tolerance,
                       std::size_t index, std::vector<PlanPoint>& out, ChordReport& report)
{
    const FieldList& p = segment.parameters;
    const auto x = p.real("xorigin");
    const auto y = p.real("yorigin");
    const auto angle = p.real("angle");
    const auto radius = p.real("radius");
    const auto length = p.real("length");
    const auto from = p.real("start");
    const auto to = p.real("end");
    if (!x || !y || !angle || !radius || !length || !from || !to || !(*length > 0.0) ||
        *radius == 0.0 || *from == *to) {
        ++report.straightened;
        return;
    }
    const double offset = p.real("offset").value_or(0.0);

    Spiral2 base;
    base.start = Point2(*x, *y);
    base.startDirection = radians(*angle);
    base.startCurvature = 0.0;
    base.endCurvature = curvatureOf(*radius);
    base.length = *length;

    const double low = std::min(*from, *to);
    const double high = std::max(*from, *to);
    Spiral2 span = base;
    span.start = base.pointAt(low);
    span.startDirection = base.directionAt(low);
    span.startCurvature = base.curvatureAt(low);
    span.endCurvature = base.curvatureAt(high);
    span.length = high - low;
    const std::size_t chords = std::max<std::size_t>(2, span.chordCountFor(tolerance));

    // Both ends must land within a centimetre, or ten tolerances if that is
    // more: tight enough that a wrong reading (out by the offset, or by the
    // whole curve) cannot pass, loose enough for coordinates written to 3
    // places.
    const double limit = std::max(0.01, 10.0 * tolerance);
    for (const double hand : {1.0, -1.0}) {
        std::vector<Point2> points;
        for (std::size_t i = 0; i <= chords; ++i) {
            const double s = low + (high - low) * static_cast<double>(i) /
                                       static_cast<double>(chords);
            const double direction = base.directionAt(s);
            const Vec2 right(std::sin(direction), -std::cos(direction));
            points.push_back(base.pointAt(s) + right * (offset * hand));
        }
        for (const bool reversed : {false, true}) {
            const Point2& first = reversed ? points.back() : points.front();
            const Point2& last = reversed ? points.front() : points.back();
            if (first.distanceTo(a) <= limit && last.distanceTo(b) <= limit) {
                ++report.transitions;
                pushInterior(out, std::move(points), index, reversed);
                return;
            }
        }
        if (offset == 0.0) {
            break; // the hand of a zero offset changes nothing
        }
    }
    ++report.straightened;
}

} // namespace

bool solveArc(const Point2& a, const Point2& b, const Segment& segment, double tolerance,
              ArcCircle& arc)
{
    const double chord = a.distanceTo(b);
    double radius = std::fabs(segment.radius);
    if (!(chord > 0.0) || !(radius > 0.0) || !std::isfinite(chord) || !std::isfinite(radius)) {
        return false;
    }
    const double half = 0.5 * chord;
    if (radius < half) {
        // A radius written to 3 places can fall a hair short of half the chord
        // of the semicircle it describes. More than that and it is not an arc
        // between these two points at all.
        if (half - radius > std::max(tolerance, 1e-6 * half)) {
            return false;
        }
        radius = half;
    }
    const double height = std::sqrt(std::max(0.0, radius * radius - half * half));
    const Vec2 direction = (b - a) / chord;
    const Vec2 rightNormal(direction.y, -direction.x);
    arc.clockwise = segment.radius > 0.0;
    // Travelling clockwise the centre is on the right. For the minor arc that
    // is also the right of the CHORD; the major arc bulges so far that its
    // centre falls on the chord's other side.
    double side = arc.clockwise ? 1.0 : -1.0;
    if (segment.major) {
        side = -side;
    }
    arc.centre = a + (b - a) * 0.5 + rightNormal * (height * side);
    arc.radius = radius;
    arc.startAngle = std::atan2(a.y - arc.centre.y, a.x - arc.centre.x);
    const double endAngle = std::atan2(b.y - arc.centre.y, b.x - arc.centre.x);
    arc.sweep = arc.clockwise ? -std::fmod(arc.startAngle - endAngle + 4.0 * kPi, 2.0 * kPi)
                              : std::fmod(endAngle - arc.startAngle + 4.0 * kPi, 2.0 * kPi);
    return arc.sweep != 0.0;
}

double arcTangentAt(const ArcCircle& arc, const Point2& point)
{
    const Vec2 radial = point - arc.centre;
    // A quarter turn from the radius: to the right of it going clockwise, to
    // the left going counter-clockwise.
    return arc.clockwise ? std::atan2(-radial.x, radial.y) : std::atan2(radial.x, -radial.y);
}

void ChordReport::merge(const ChordReport& other)
{
    arcs += other.arcs;
    transitions += other.transitions;
    straightened += other.straightened;
    for (const auto& [type, adjustment] : other.adjustmentByType) {
        double& worst = adjustmentByType[type];
        worst = std::max(worst, adjustment);
    }
}

bool solveCubicParabola(double radius, double length, CubicParabolaEnd& end)
{
    if (!(radius > 0.0) || !(length > 0.0)) {
        return false;
    }
    // The abscissa is a little less than the arc length; bisect on it.
    double low = 0.5 * length;
    double high = length;
    if (cubicCoefficient(radius, low) < 0.0) {
        return false; // even the shortest candidate cannot reach this radius
    }
    for (int i = 0; i < 80; ++i) {
        const double X = 0.5 * (low + high);
        const double m = cubicCoefficient(radius, X);
        // No curve of this abscissa reaches the radius: it is too long.
        if (m < 0.0 || cubicArcLength(m, X) > length) {
            high = X;
        } else {
            low = X;
        }
    }
    const double X = 0.5 * (low + high);
    const double m = cubicCoefficient(radius, X);
    if (m < 0.0 || std::fabs(cubicArcLength(m, X) - length) > 1e-6 * length) {
        return false;
    }
    end.abscissa = X;
    end.coefficient = m;
    end.offset = m * X * X * X;
    return true;
}

std::vector<PlanPoint> chordPlan(const std::vector<Vertex>& vertices,
                                 const std::vector<Segment>& segments, bool closed,
                                 double tolerance, ChordReport& report)
{
    std::vector<PlanPoint> out;
    const std::size_t count = vertices.size();
    if (count == 0) {
        return out;
    }
    const std::size_t segmentCount = count < 2 ? 0 : (closed ? count : count - 1);
    out.reserve(count);
    for (std::size_t i = 0; i < count; ++i) {
        const Point2 a(vertices[i].x, vertices[i].y);
        out.push_back(PlanPoint{a, i, 0.0});
        if (i >= segmentCount || i >= segments.size()) {
            continue;
        }
        const Vertex& next = vertices[(i + 1) % count];
        const Point2 b(next.x, next.y);
        switch (segments[i].kind) {
        case SegmentKind::Arc:
            chordArcSegment(a, b, segments[i], tolerance, i, out, report);
            break;
        case SegmentKind::Spiral:
            chordSpiralSegment(a, b, segments[i], tolerance, i, out, report);
            break;
        case SegmentKind::Curve:
            chordCurveSegment(a, b, segments[i], tolerance, i, out, report);
            break;
        case SegmentKind::Straight:
        case SegmentKind::Parabola: // vertical geometry only; in plan it is a line
            break;
        }
    }
    return out;
}

} // namespace katana::archive12d::detail
