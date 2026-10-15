// The solids a road object is made of (include/katana/cad/road/road_object_mesh.hpp).

#include "katana/cad/road/road_object_mesh.hpp"

#include <algorithm>
#include <cmath>
#include <map>
#include <numbers>
#include <utility>

namespace katana::cad::road {

using katana::geometry::Point2;
using katana::geometry::Point3;
using katana::geometry::TriangleMesh;
using katana::geometry::Vec2;
using katana::geometry::Vec3;
using Face = std::array<std::uint32_t, 3>;

namespace {

// Points of a path closer than this in plan are one point: a millimetre, far
// below anything a road object is set out to, and far above the noise of a
// station computed twice.
constexpr double kSamePoint = 1e-3;

// A mitre longer than this many times the section's half-width is not cut:
// at a hairpin of the path the true mitre runs off to infinity. Past it the
// joint is a bevel - the section ends square to the leg that arrives and
// starts square to the leg that leaves, the two rings joined side to side -
// as SVG and PostScript draw a join past the same limit (stroke-miterlimit's
// default of four). Shortening the mitre instead would narrow the section
// along the whole leg, because the leg's sides run from a full-width ring at
// its far end to the narrowed one at the joint.
constexpr double kMitreLimit = 4.0;

double signedArea(std::span<const Point2> polygon)
{
    if (polygon.size() < 3) {
        return 0.0;
    }
    // Relative to the first vertex, so a polygon far from the origin keeps
    // its digits.
    const Point2 origin = polygon.front();
    double twice = 0.0;
    for (std::size_t i = 1; i + 1 < polygon.size(); ++i) {
        twice += (polygon[i] - origin).cross(polygon[i + 1] - origin);
    }
    return twice / 2.0;
}

// Inside or on the boundary of the counter-clockwise triangle a, b, c.
bool inTriangle(const Point2& p, const Point2& a, const Point2& b, const Point2& c)
{
    return (b - a).cross(p - a) >= 0.0 && (c - b).cross(p - b) >= 0.0 &&
           (a - c).cross(p - c) >= 0.0;
}

std::uint32_t index(std::size_t value)
{
    return static_cast<std::uint32_t>(value);
}

// The cleaned path: points closer than kSamePoint in plan to the one before
// left out.
std::vector<PathPoint> distinct(std::span<const PathPoint> path)
{
    std::vector<PathPoint> out;
    for (const PathPoint& point : path) {
        if (!out.empty() && out.back().position.distanceTo(point.position) < kSamePoint) {
            continue;
        }
        out.push_back(point);
    }
    return out;
}

// The path a sweep follows, cleaned: when it comes back to its start (the
// same plan position and level, within kSamePoint) the closing point is
// dropped and `loop` says so, so the last segment runs on to the first point
// and the sweep wraps round with no ends. A fence, a wall or a line of
// barrier round a closed polyline then has its start corner mitred like every
// other, instead of two square end faces that leave a notch outside the
// corner and overlap inside it - or, closed midway along a side, two
// coincident caps.
struct SweepPath {
    std::vector<PathPoint> points;
    bool loop = false;
};

SweepPath sweepPathOf(std::span<const PathPoint> path)
{
    SweepPath out{distinct(path), false};
    std::vector<PathPoint>& points = out.points;
    // Four points at least: three distinct corners and the one that closes them.
    if (points.size() >= 4) {
        const PathPoint& first = points.front();
        const PathPoint& last = points.back();
        const PathPoint& before = points[points.size() - 2];
        if (first.position.distanceTo(last.position) < kSamePoint &&
            std::abs(first.z - last.z) < kSamePoint &&
            first.position.distanceTo(before.position) >= kSamePoint) {
            points.pop_back();
            out.loop = true;
        }
    }
    return out;
}

// One ring of a swept section: the path point it stands at, and the
// horizontal vector its u is measured along there.
struct Ring {
    std::size_t point = 0;
    Vec2 axis{};
};

// The rings of a sweep, in order. A point of the path is one ring - its axis
// the left normal of the path there, mitred at a bend so that a section u to
// the left of both segments stays u from each - except where the bend is past
// kMitreLimit (or the path doubles back): there it is two, one square to the
// segment that arrives and one square to the segment that leaves, so each leg
// keeps its full width right up to the joint. An open path's ends are square
// to their one segment.
std::vector<Ring> ringsOf(const std::vector<PathPoint>& path, bool loop)
{
    const std::size_t count = path.size();
    const std::size_t segments = loop ? count : count - 1;
    std::vector<Vec2> normals;
    normals.reserve(segments);
    for (std::size_t i = 0; i < segments; ++i) {
        normals.push_back(
            (path[(i + 1) % count].position - path[i].position).normalized().perpendicular());
    }
    std::vector<Ring> rings;
    rings.reserve(count + 1);
    for (std::size_t k = 0; k < count; ++k) {
        if (!loop && k == 0) {
            rings.push_back({k, normals.front()});
            continue;
        }
        if (!loop && k + 1 == count) {
            rings.push_back({k, normals.back()});
            continue;
        }
        const Vec2& arriving = normals[(k + segments - 1) % segments];
        const Vec2& leaving = normals[k % segments];
        const Vec2 bisector = arriving + leaving;
        const double length = bisector.length();
        // Two unit normals sum to 2 cos(half the turn): the mitre's length
        // over the half-width, 1 / cos, is within the limit exactly when the
        // sum is at least 2 / limit. A path that doubles back sums to 0.
        if (length >= 2.0 / kMitreLimit) {
            const Vec2 unit = bisector / length;
            rings.push_back({k, unit * (1.0 / unit.dot(leaving))});
        } else {
            rings.push_back({k, arriving});
            rings.push_back({k, leaving});
        }
    }
    return rings;
}

// Appends the rings of `profile` at each ring of the sweep, P vertices a
// ring; returns the index of the first.
std::uint32_t addRings(TriangleMesh& mesh, const std::vector<PathPoint>& path,
                       const std::vector<Ring>& rings, std::span<const Point2> profile)
{
    const std::uint32_t first = index(mesh.vertices.size());
    for (const Ring& ring : rings) {
        const PathPoint& at = path[ring.point];
        for (const Point2& corner : profile) {
            const Point2 plan = at.position + ring.axis * corner.x;
            mesh.vertices.emplace_back(plan.x, plan.y, at.z + corner.y);
        }
    }
    return first;
}

// The quads joining ring k to ring k + 1 round the whole profile, facing
// out of a counter-clockwise profile - or into it, for the inside of a tube.
// A loop's last ring joins the first.
void addSides(TriangleMesh& mesh, std::uint32_t first, std::size_t rings, std::size_t corners,
              bool facingIn, bool loop)
{
    const std::size_t pairs = loop ? rings : rings - 1;
    for (std::size_t k = 0; k < pairs; ++k) {
        const std::size_t following = (k + 1) % rings;
        for (std::size_t j = 0; j < corners; ++j) {
            const std::size_t next = (j + 1) % corners;
            const std::uint32_t a = first + index(k * corners + j);
            const std::uint32_t b = first + index(k * corners + next);
            const std::uint32_t c = first + index(following * corners + next);
            const std::uint32_t d = first + index(following * corners + j);
            if (facingIn) {
                mesh.faces.push_back({a, c, b});
                mesh.faces.push_back({a, d, c});
            } else {
                mesh.faces.push_back({a, b, c});
                mesh.faces.push_back({a, c, d});
            }
        }
    }
}

} // namespace

std::vector<Point2> counterClockwise(std::span<const Point2> polygon)
{
    std::vector<Point2> out(polygon.begin(), polygon.end());
    if (signedArea(out) < 0.0) {
        std::ranges::reverse(out);
    }
    return out;
}

std::vector<Face> triangulatePolygon(std::span<const Point2> polygon)
{
    std::vector<Face> triangles;
    if (polygon.size() < 3) {
        return triangles;
    }
    const double area = signedArea(polygon);
    if (area == 0.0 || !std::isfinite(area)) {
        return triangles;
    }
    // Indices in counter-clockwise order, whichever way the polygon runs.
    std::vector<std::uint32_t> ring(polygon.size());
    for (std::size_t i = 0; i < ring.size(); ++i) {
        ring[i] = index(i);
    }
    const bool reversed = area < 0.0;
    if (reversed) {
        std::ranges::reverse(ring);
    }
    // An ear is cut as the triangle the polygon's own order makes, which is
    // counter-clockwise once the ring is.
    while (ring.size() > 3) {
        const std::size_t m = ring.size();
        bool cut = false;
        // A strictly convex ear first; only when there is none a straight
        // one, whose triangle has no area but keeps the polygon's own edges
        // in the triangulation - which a cap must, to meet the sides it caps.
        for (const bool straightAllowed : {false, true}) {
            for (std::size_t i = 0; i < m && !cut; ++i) {
                const std::uint32_t a = ring[(i + m - 1) % m];
                const std::uint32_t b = ring[i];
                const std::uint32_t c = ring[(i + 1) % m];
                const double turn = (polygon[b] - polygon[a]).cross(polygon[c] - polygon[b]);
                if (turn < 0.0 || (turn == 0.0 && !straightAllowed)) {
                    continue;
                }
                if (turn == 0.0 && (polygon[b] - polygon[a]).dot(polygon[c] - polygon[b]) < 0.0) {
                    continue; // a spike back along itself, not a straight run
                }
                bool clear = true;
                for (const std::uint32_t other : ring) {
                    if (other == a || other == b || other == c) {
                        continue;
                    }
                    const Point2& p = polygon[other];
                    if (p == polygon[a] || p == polygon[b] || p == polygon[c]) {
                        continue;
                    }
                    if (turn > 0.0 && inTriangle(p, polygon[a], polygon[b], polygon[c])) {
                        clear = false;
                        break;
                    }
                }
                if (!clear) {
                    continue;
                }
                triangles.push_back({a, b, c});
                ring.erase(ring.begin() + static_cast<std::ptrdiff_t>(i));
                cut = true;
            }
            if (cut) {
                break;
            }
        }
        if (!cut) {
            return {}; // no ear: the polygon crosses itself
        }
    }
    triangles.push_back({ring[0], ring[1], ring[2]});
    return triangles;
}

std::vector<Point2> regularPolygon(Point2 centre, double radius, int sides, double startRadians)
{
    std::vector<Point2> out;
    if (sides < 3) {
        return out;
    }
    out.reserve(static_cast<std::size_t>(sides));
    for (int k = 0; k < sides; ++k) {
        const double angle = startRadians + 2.0 * std::numbers::pi * k / sides;
        out.emplace_back(centre.x + radius * std::cos(angle), centre.y + radius * std::sin(angle));
    }
    return out;
}

void addExtrusion(TriangleMesh& mesh, std::span<const Point2> polygon, const Point3& origin,
                  const Vec3& u, const Vec3& v, double depth)
{
    if (!(depth > 0.0)) {
        return;
    }
    const std::vector<Point2> profile = counterClockwise(polygon);
    const std::vector<Face> caps = triangulatePolygon(profile);
    if (caps.empty()) {
        return;
    }
    // (u, v, w) right-handed: the same frame as a sweep's (t, u, v) with the
    // extrusion as t, so the faces are wound as a sweep's are.
    const Vec3 w = u.cross(v);
    const std::uint32_t first = index(mesh.vertices.size());
    for (const double along : {0.0, depth}) {
        for (const Point2& corner : profile) {
            mesh.vertices.push_back(origin + u * corner.x + v * corner.y + w * along);
        }
    }
    const std::uint32_t top = first + index(profile.size());
    for (const Face& face : caps) {
        mesh.faces.push_back({first + face[0], first + face[2], first + face[1]});
        mesh.faces.push_back({top + face[0], top + face[1], top + face[2]});
    }
    addSides(mesh, first, 2, profile.size(), false, false);
}

void addPrism(TriangleMesh& mesh, std::span<const Point2> outline, double bottom, double top)
{
    addExtrusion(mesh, outline, Point3(0.0, 0.0, bottom), Vec3(1.0, 0.0, 0.0),
                 Vec3(0.0, 1.0, 0.0), top - bottom);
}

void addBox(TriangleMesh& mesh, const Point3& baseCentre, const Vec2& along, double length,
            double width, double height)
{
    const Vec2 a = along.normalized();
    const Vec2 left = a.perpendicular();
    const Point2 centre(baseCentre.x, baseCentre.y);
    const std::array<Point2, 4> corners{
        centre - a * (length / 2.0) - left * (width / 2.0),
        centre + a * (length / 2.0) - left * (width / 2.0),
        centre + a * (length / 2.0) + left * (width / 2.0),
        centre - a * (length / 2.0) + left * (width / 2.0),
    };
    addPrism(mesh, corners, baseCentre.z, baseCentre.z + height);
}

void addSweep(TriangleMesh& mesh, std::span<const PathPoint> path, std::span<const Point2> profile)
{
    const SweepPath swept = sweepPathOf(path);
    const std::vector<PathPoint>& points = swept.points;
    if (points.size() < 2) {
        return;
    }
    const std::vector<Point2> section = counterClockwise(profile);
    const std::vector<Face> caps = triangulatePolygon(section);
    if (caps.empty()) {
        return;
    }
    const std::vector<Ring> rings = ringsOf(points, swept.loop);
    const std::uint32_t first = addRings(mesh, points, rings, section);
    const std::size_t corners = section.size();
    if (!swept.loop) {
        // The end faces forward, along the path, which in the (t, u, v) frame
        // is the side a counter-clockwise profile is seen counter-clockwise
        // from; the start faces back.
        const std::uint32_t last = first + index((rings.size() - 1) * corners);
        for (const Face& face : caps) {
            mesh.faces.push_back({first + face[0], first + face[2], first + face[1]});
            mesh.faces.push_back({last + face[0], last + face[1], last + face[2]});
        }
    }
    addSides(mesh, first, rings.size(), corners, false, swept.loop);
}

void addHollowSweep(TriangleMesh& mesh, std::span<const PathPoint> path,
                    std::span<const Point2> outer, std::span<const Point2> inner)
{
    const SweepPath swept = sweepPathOf(path);
    const std::vector<PathPoint>& points = swept.points;
    if (points.size() < 2 || outer.size() != inner.size() || outer.size() < 3) {
        return;
    }
    const std::vector<Ring> rings = ringsOf(points, swept.loop);
    const std::size_t corners = outer.size();
    const std::uint32_t outside = addRings(mesh, points, rings, outer);
    const std::uint32_t inside = addRings(mesh, points, rings, inner);
    addSides(mesh, outside, rings.size(), corners, false, swept.loop);
    addSides(mesh, inside, rings.size(), corners, true, swept.loop);
    if (swept.loop) {
        return; // a ring of tube with no ends
    }
    // The ends: the ring between the outer and the inner loop, a quad an
    // edge, wound as the solid sweep's ends are.
    const std::size_t lastRing = (rings.size() - 1) * corners;
    for (std::size_t j = 0; j < corners; ++j) {
        const std::size_t next = (j + 1) % corners;
        const std::uint32_t o0 = outside + index(j);
        const std::uint32_t o1 = outside + index(next);
        const std::uint32_t i0 = inside + index(j);
        const std::uint32_t i1 = inside + index(next);
        mesh.faces.push_back({o0, i1, o1});
        mesh.faces.push_back({o0, i0, i1});
        const std::uint32_t e0 = outside + index(lastRing + j);
        const std::uint32_t e1 = outside + index(lastRing + next);
        const std::uint32_t f0 = inside + index(lastRing + j);
        const std::uint32_t f1 = inside + index(lastRing + next);
        mesh.faces.push_back({e0, e1, f1});
        mesh.faces.push_back({e0, f1, f0});
    }
}

bool isClosedMesh(const TriangleMesh& mesh)
{
    if (mesh.faces.empty()) {
        return false;
    }
    std::map<std::pair<std::uint32_t, std::uint32_t>, int> directed;
    for (const Face& face : mesh.faces) {
        for (std::size_t e = 0; e < 3; ++e) {
            ++directed[{face[e], face[(e + 1) % 3]}];
        }
    }
    for (const auto& [edge, count] : directed) {
        if (count != 1) {
            return false;
        }
        const auto reverse = directed.find({edge.second, edge.first});
        if (reverse == directed.end() || reverse->second != 1) {
            return false;
        }
    }
    return true;
}

} // namespace katana::cad::road
