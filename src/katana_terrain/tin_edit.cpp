#include "katana/terrain/tin_edit.hpp"

#include <algorithm>
#include <cmath>
#include <limits>
#include <memory>
#include <string>
#include <utility>

#include "cdt_backend.hpp"
#include "katana/core/text.hpp"
#include "katana/geometry/polygon.hpp"
#include "katana/math/numerics.hpp"
#include "katana/terrain/super_surface.hpp"
#include "point_merge.hpp"
#include "tin_patch.hpp"
#include "tin_surface_factory.hpp"

namespace katana::terrain {

namespace tol = katana::math::tolerance;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;
using katana::geometry::Containment;
using katana::geometry::Segment2;
using detail::describePosition;

namespace {

Point2 planOf(const Point3& p) { return Point2(p.x, p.y); }

Status requireSurface(const TinSurface& surface, const char* which = "the surface")
{
    if (surface.empty()) {
        return makeError(ErrorCode::InvalidArgument, std::string(which) + " is empty");
    }
    return {};
}

// One flag per vertex: some triangle uses it.
std::vector<std::uint8_t> referencedVertices(const TinSurface& surface)
{
    std::vector<std::uint8_t> used(surface.vertexCount(), 0);
    for (const TinTriangle& tri : surface.triangles()) {
        for (const std::uint32_t v : tri) {
            used[v] = 1;
        }
    }
    return used;
}

// One flag per vertex: an end of a constrained edge.
std::vector<std::uint8_t> constrainedVertices(const TinSurface& surface)
{
    std::vector<std::uint8_t> flags(surface.vertexCount(), 0);
    for (std::uint32_t t = 0; t < surface.triangleCount(); ++t) {
        for (std::uint32_t k = 0; k < 3; ++k) {
            if (surface.isEdgeConstrained(t, k)) {
                flags[surface.triangles()[t][k]] = 1;
                flags[surface.triangles()[t][(k + 1) % 3]] = 1;
            }
        }
    }
    return flags;
}

// Whether two closed segments meet, decided exactly (the kernel's predicate).
bool segmentsMeet(const Point2& a, const Point2& b, const Point2& c, const Point2& d)
{
    const int o1 = detail::orientation(a, b, c);
    const int o2 = detail::orientation(a, b, d);
    const int o3 = detail::orientation(c, d, a);
    const int o4 = detail::orientation(c, d, b);
    if (o1 * o2 < 0 && o3 * o4 < 0) {
        return true;
    }
    // A point on the line of the other segment meets it when it lies within it.
    const auto within = [](const Point2& p, const Point2& s, const Point2& e) {
        return p.x >= std::min(s.x, e.x) && p.x <= std::max(s.x, e.x) &&
               p.y >= std::min(s.y, e.y) && p.y <= std::max(s.y, e.y);
    };
    return (o1 == 0 && within(c, a, b)) || (o2 == 0 && within(d, a, b)) ||
           (o3 == 0 && within(a, c, d)) || (o4 == 0 && within(b, c, d));
}

// Whether two edges of the closed ring that share no corner meet: a bow-tie,
// or a corner on another edge. Only asked of a ring with no signed area (a
// figure of eight whose lobes cancel), to say why, so it is not worth
// anything faster than every pair - and is not asked of a ring too long for
// that to be a moment's work.
bool ringTouchesItself(const std::vector<Point2>& ring)
{
    constexpr std::size_t kMostCorners = 2000;
    const std::size_t n = ring.size();
    if (n > kMostCorners) {
        return false;
    }
    for (std::size_t i = 0; i < n; ++i) {
        for (std::size_t j = i + 2; j < n; ++j) {
            if (i == 0 && j + 1 == n) {
                continue; // joined by the closing edge
            }
            if (segmentsMeet(ring[i], ring[(i + 1) % n], ring[j], ring[(j + 1) % n])) {
                return true;
            }
        }
    }
    return false;
}

// A polygon as the edits take it: every coordinate finite and within
// kMostMagnitude, repeated vertices (within kGeometric) and a repeated
// closing vertex dropped, at least three distinct vertices with an area,
// closed, counter-clockwise.
Result<Polyline2> cleanPolygon(const Polyline2& polygon, const std::string& what)
{
    Polyline2 ring;
    ring.closed = true;
    for (const Point2& vertex : polygon.vertices) {
        if (!vertex.isFinite()) {
            return makeError(ErrorCode::InvalidArgument, what + " has a non-finite coordinate");
        }
        if (!detail::withinMagnitude(vertex.x) || !detail::withinMagnitude(vertex.y)) {
            return detail::tooLarge(describePosition(vertex));
        }
        if (ring.vertices.empty() || ring.vertices.back().distanceTo(vertex) > tol::kGeometric) {
            ring.vertices.push_back(vertex);
        }
    }
    while (ring.vertices.size() > 1 &&
           ring.vertices.front().distanceTo(ring.vertices.back()) <= tol::kGeometric) {
        ring.vertices.pop_back();
    }
    if (ring.vertices.size() < 3) {
        return makeError(ErrorCode::InvalidGeometry,
                         what + " needs at least 3 distinct vertices",
                         "distinct vertices=" + std::to_string(ring.vertices.size()));
    }
    const auto turn = katana::geometry::orientation(ring);
    if (turn == katana::geometry::Orientation::Degenerate) {
        // A figure of eight whose lobes cancel has no signed area either, and
        // "no area" would send a person looking for a flat polygon.
        return makeError(ErrorCode::InvalidGeometry,
                         ringTouchesItself(ring.vertices) ? what + " touches or crosses itself"
                                                          : what + " has no area");
    }
    if (turn == katana::geometry::Orientation::Clockwise) {
        std::reverse(ring.vertices.begin(), ring.vertices.end());
    }
    return ring;
}

// How many of a patch's new vertices the triangles use, and how many of the
// surface's used vertices they no longer use.
struct Use {
    std::size_t added = 0;
    std::size_t dropped = 0;
};
Use useOf(const detail::PatchOutput& output, const TinSurface& before)
{
    std::vector<std::uint8_t> used(output.vertices.size(), 0);
    for (const TinTriangle& tri : output.triangles) {
        for (const std::uint32_t v : tri) {
            used[v] = 1;
        }
    }
    const std::vector<std::uint8_t> usedBefore = referencedVertices(before);
    Use use;
    for (std::size_t v = 0; v < used.size(); ++v) {
        if (v < before.vertexCount()) {
            use.dropped += (usedBefore[v] != 0 && used[v] == 0) ? 1 : 0;
        } else {
            use.added += used[v] != 0 ? 1 : 0;
        }
    }
    return use;
}

Result<TinEditResult> finishPatch(const TinSurface& before, detail::PatchOutput output)
{
    const Use use = useOf(output, before);
    const std::size_t relevelled = output.relevelled;
    const std::size_t crossings = output.crossings;
    auto surface = detail::finishSurface(std::move(output.vertices), std::move(output.triangles),
                                         std::move(output.masks));
    if (!surface) {
        return surface.error();
    }
    TinEditResult result;
    result.report = detail::reportOf(before, *surface);
    result.report.pointsAdded = use.added;
    result.report.pointsRemoved = use.dropped;
    result.report.pointsRelevelled = relevelled;
    result.report.crossings = crossings;
    result.surface = std::move(*surface);
    return result;
}

// The surface with new levels and nothing else changed: the same triangles,
// so the trusted path assembles it without deriving the adjacency again.
Result<TinSurface> withLevels(const TinSurface& surface, std::vector<Point3> vertices)
{
    return detail::TinSurfaceFactory::assemble(std::move(vertices), surface.triangles(),
                                               surface.neighbors(), surface.constrainedEdges());
}

} // namespace

// ---- finders ------------------------------------------------------------------------------

std::optional<std::uint32_t> triangleAt(const TinSurface& surface, const Point2& position)
{
    const auto location = surface.locate(position);
    if (!location) {
        return std::nullopt;
    }
    return location->triangle;
}

std::optional<TinEdgeRef> edgeAt(const TinSurface& surface, const Point2& position)
{
    const auto triangle = triangleAt(surface, position);
    if (!triangle) {
        return std::nullopt;
    }
    const auto plan = surface.planTriangle(*triangle);
    const Point2 corners[3] = {plan.a, plan.b, plan.c};
    TinEdgeRef nearest{*triangle, 0};
    double best = std::numeric_limits<double>::infinity();
    for (std::uint32_t k = 0; k < 3; ++k) {
        const double distance = Segment2{corners[k], corners[(k + 1) % 3]}.distanceTo(position);
        if (distance < best) {
            best = distance;
            nearest.edge = k;
        }
    }
    return nearest;
}

std::optional<std::uint32_t> vertexAt(const TinSurface& surface, const Point2& position,
                                      std::optional<double> reach)
{
    const auto triangle = triangleAt(surface, position);
    std::uint32_t nearest = kNoVertex;
    double best = std::numeric_limits<double>::infinity();
    if (triangle) {
        for (const std::uint32_t v : surface.triangles()[*triangle]) {
            const double distance = planOf(surface.vertices()[v]).distanceTo(position);
            if (distance < best) {
                best = distance;
                nearest = v;
            }
        }
        if (!reach || best <= *reach) {
            return nearest;
        }
    } else if (!reach) {
        return std::nullopt;
    }
    // Off the surface, or in a triangle none of whose corners is within reach:
    // the aperture is a disc round the position, and a vertex in it need not
    // be a corner of the triangle under its centre. One click's worth of
    // scanning, only on this path (a corner of the triangle under the
    // position is the common answer and costs nothing).
    best = std::numeric_limits<double>::infinity();
    nearest = kNoVertex;
    const std::vector<std::uint8_t> used = referencedVertices(surface);
    for (std::uint32_t v = 0; v < surface.vertexCount(); ++v) {
        if (used[v] == 0) {
            continue;
        }
        const double distance = planOf(surface.vertices()[v]).distanceTo(position);
        if (distance <= *reach && distance < best) {
            best = distance;
            nearest = v;
        }
    }
    if (nearest == kNoVertex) {
        return std::nullopt;
    }
    return nearest;
}

bool onConstrainedEdge(const TinSurface& surface, std::uint32_t vertex)
{
    for (std::uint32_t t = 0; t < surface.triangleCount(); ++t) {
        for (std::uint32_t k = 0; k < 3; ++k) {
            if (surface.isEdgeConstrained(t, k) &&
                (surface.triangles()[t][k] == vertex ||
                 surface.triangles()[t][(k + 1) % 3] == vertex)) {
                return true;
            }
        }
    }
    return false;
}

std::vector<std::uint32_t> trianglesByPolygon(const TinSurface& surface, const Polyline2& polygon,
                                              bool inside)
{
    const katana::geometry::PolygonClassifier ring(polygon);
    std::vector<std::uint32_t> found;
    for (std::uint32_t t = 0; t < surface.triangleCount(); ++t) {
        const bool in = ring.classify(surface.planTriangle(t).centroid()) != Containment::Outside;
        if (in == inside) {
            found.push_back(t);
        }
    }
    return found;
}

std::vector<std::uint32_t> trianglesLongerThan(const TinSurface& surface, double length)
{
    std::vector<std::uint32_t> found;
    for (std::uint32_t t = 0; t < surface.triangleCount(); ++t) {
        const auto plan = surface.planTriangle(t);
        const double longest =
            std::max({plan.a.distanceTo(plan.b), plan.b.distanceTo(plan.c), plan.c.distanceTo(plan.a)});
        if (longest > length) {
            found.push_back(t);
        }
    }
    return found;
}

std::vector<std::uint32_t> trianglesThinnerThan(const TinSurface& surface, double radians)
{
    std::vector<std::uint32_t> found;
    for (std::uint32_t t = 0; t < surface.triangleCount(); ++t) {
        const auto plan = surface.planTriangle(t);
        const Point2 corners[3] = {plan.a, plan.b, plan.c};
        double smallest = std::numeric_limits<double>::infinity();
        for (int k = 0; k < 3; ++k) {
            const auto u = corners[(k + 1) % 3] - corners[k];
            const auto w = corners[(k + 2) % 3] - corners[k];
            // atan2(|u x w|, u . w) is the angle between them, well conditioned
            // near 0 and pi where acos of a cosine is not; 0 for a zero-length side.
            smallest = std::min(smallest, std::atan2(std::abs(u.cross(w)), u.dot(w)));
        }
        if (smallest < radians) {
            found.push_back(t);
        }
    }
    return found;
}

std::vector<std::uint32_t> verticesInside(const TinSurface& surface, const Polyline2& polygon)
{
    const katana::geometry::PolygonClassifier ring(polygon);
    const std::vector<std::uint8_t> used = referencedVertices(surface);
    std::vector<std::uint32_t> found;
    for (std::uint32_t v = 0; v < surface.vertexCount(); ++v) {
        if (used[v] != 0 &&
            ring.classify(planOf(surface.vertices()[v])) != Containment::Outside) {
            found.push_back(v);
        }
    }
    return found;
}

std::vector<std::uint8_t> heldVertices(const TinSurface& surface)
{
    std::vector<std::uint8_t> held = constrainedVertices(surface);
    for (std::uint32_t t = 0; t < surface.triangleCount(); ++t) {
        for (std::uint32_t k = 0; k < 3; ++k) {
            if (surface.neighbors()[t][k] == kNoTriangle) {
                held[surface.triangles()[t][k]] = 1;
                held[surface.triangles()[t][(k + 1) % 3]] = 1;
            }
        }
    }
    return held;
}

// ---- deleting -------------------------------------------------------------------------------

Result<TinEditResult> deleteTriangles(const TinSurface& surface,
                                      std::span<const std::uint32_t> triangles)
{
    if (triangles.empty()) {
        return makeError(ErrorCode::InvalidArgument, "no triangle to delete was given");
    }
    std::vector<std::uint8_t> drop(surface.triangleCount(), 0);
    for (const std::uint32_t t : triangles) {
        if (t >= surface.triangleCount()) {
            return makeError(ErrorCode::InvalidArgument, "triangle index out of range",
                             "triangle=" + std::to_string(t) +
                                 " triangles=" + std::to_string(surface.triangleCount()));
        }
        drop[t] = 1;
    }
    detail::PatchOutput output;
    output.vertices = surface.vertices();
    for (std::uint32_t t = 0; t < surface.triangleCount(); ++t) {
        if (drop[t] == 0) {
            output.triangles.push_back(surface.triangles()[t]);
            output.masks.push_back(surface.constrainedEdges()[t]);
        }
    }
    if (output.triangles.empty()) {
        return makeError(ErrorCode::InvalidState,
                         "every triangle of the surface would be deleted; nothing would be left",
                         "triangles=" + std::to_string(surface.triangleCount()));
    }
    return finishPatch(surface, std::move(output));
}

Result<TinEditResult> deletePoints(const TinSurface& surface,
                                   std::span<const std::uint32_t> vertices, HeldPointRule held)
{
    if (const Status status = requireSurface(surface); !status) {
        return status.error();
    }
    if (vertices.empty()) {
        return makeError(ErrorCode::InvalidArgument, "no point to delete was given");
    }
    const std::vector<std::uint8_t> constrained = constrainedVertices(surface);
    const std::vector<std::uint8_t> used = referencedVertices(surface);
    detail::PatchRequest request;
    request.removed.assign(surface.vertexCount(), 0);
    std::size_t heldCount = 0;
    std::size_t removing = 0;
    for (const std::uint32_t v : vertices) {
        if (v >= surface.vertexCount()) {
            return makeError(ErrorCode::InvalidArgument, "vertex index out of range",
                             "vertex=" + std::to_string(v) +
                                 " vertices=" + std::to_string(surface.vertexCount()));
        }
        if (constrained[v] != 0) {
            if (held == HeldPointRule::Refuse) {
                return makeError(ErrorCode::InvalidGeometry,
                                 "the point is on a breakline or boundary edge, which is kept; "
                                 "delete the triangles around it instead",
                                 "vertex=" + std::to_string(v) + " at " +
                                     describePosition(planOf(surface.vertices()[v])));
            }
            ++heldCount;
            continue;
        }
        if (used[v] != 0 && request.removed[v] == 0) {
            request.removed[v] = 1;
            ++removing;
        }
    }
    if (removing == 0) {
        TinEditResult result = detail::unchanged(surface);
        result.report.pointsHeld = heldCount;
        return result;
    }
    const detail::VertexStars stars(surface);
    auto output = detail::rebuildPatch(surface, stars, std::move(request));
    if (!output) {
        return output.error();
    }
    auto result = finishPatch(surface, std::move(*output));
    if (result) {
        result->report.pointsHeld = heldCount;
    }
    return result;
}

// ---- changing the triangulation -----------------------------------------------------------

Result<TinEditResult> flipEdge(const TinSurface& surface, std::uint32_t triangle,
                               std::uint32_t edge)
{
    if (triangle >= surface.triangleCount() || edge > 2) {
        return makeError(ErrorCode::InvalidArgument, "no such edge of the surface",
                         "triangle=" + std::to_string(triangle) + " edge=" + std::to_string(edge));
    }
    const TinTriangle& tri = surface.triangles()[triangle];
    const std::uint32_t a = tri[edge];
    const std::uint32_t b = tri[(edge + 1) % 3];
    const std::uint32_t c = tri[(edge + 2) % 3];
    const Point2 at = Segment2{planOf(surface.vertices()[a]), planOf(surface.vertices()[b])}.pointAt(0.5);
    const std::uint32_t across = surface.neighbors()[triangle][edge];
    if (across == kNoTriangle) {
        return makeError(ErrorCode::InvalidGeometry,
                         "the edge is on the rim of the surface: there is no triangle across it "
                         "to flip with",
                         "edge at " + describePosition(at));
    }
    if (surface.isEdgeConstrained(triangle, edge)) {
        return makeError(ErrorCode::InvalidGeometry,
                         "the edge is a breakline or boundary edge, which is kept; it cannot be "
                         "flipped",
                         "edge at " + describePosition(at));
    }
    const TinTriangle& other = surface.triangles()[across];
    std::uint32_t j = 0;
    while (j < 3 && !(other[j] == b && other[(j + 1) % 3] == a)) {
        ++j;
    }
    if (j == 3) {
        return makeError(ErrorCode::Internal, "the triangle across the edge does not share it");
    }
    const std::uint32_t d = other[(j + 2) % 3];
    const auto& v = surface.vertices();
    // The quadrilateral a, d, b, c is strictly convex exactly when both
    // triangles of the other diagonal turn the right way.
    if (detail::orientation(planOf(v[a]), planOf(v[d]), planOf(v[c])) <= 0 ||
        detail::orientation(planOf(v[d]), planOf(v[b]), planOf(v[c])) <= 0) {
        return makeError(ErrorCode::InvalidGeometry,
                         "the two triangles do not make a convex quadrilateral; flipping the edge "
                         "would fold the surface over itself",
                         "edge at " + describePosition(at));
    }
    const auto bit = [](std::uint8_t mask, std::uint32_t k) {
        return static_cast<std::uint8_t>((mask >> k) & 1u);
    };
    const std::uint8_t maskT = surface.constrainedEdges()[triangle];
    const std::uint8_t maskO = surface.constrainedEdges()[across];
    detail::PatchOutput output;
    output.vertices = v;
    output.triangles = surface.triangles();
    output.masks = surface.constrainedEdges();
    // (a, d, c): a->d is the other triangle's edge j+1, d->c the new
    // diagonal, c->a this triangle's edge k+2.
    output.triangles[triangle] = {a, d, c};
    output.masks[triangle] = static_cast<std::uint8_t>(bit(maskO, (j + 1) % 3) |
                                                       (bit(maskT, (edge + 2) % 3) << 2));
    // (d, b, c): d->b the other's edge j+2, b->c this one's edge k+1.
    output.triangles[across] = {d, b, c};
    output.masks[across] = static_cast<std::uint8_t>(bit(maskO, (j + 2) % 3) |
                                                     (bit(maskT, (edge + 1) % 3) << 1));
    return finishPatch(surface, std::move(output));
}

Result<TinEditResult> addPoints(const TinSurface& surface, std::span<const Point3> points)
{
    if (const Status status = requireSurface(surface); !status) {
        return status.error();
    }
    if (points.empty()) {
        return makeError(ErrorCode::InvalidArgument, "no point to add was given");
    }
    for (const Point3& point : points) {
        if (!point.isFinite()) {
            return makeError(ErrorCode::InvalidArgument, "a point has a non-finite coordinate",
                             "at " + describePosition(planOf(point)));
        }
        if (!detail::withinMagnitude(point.x) || !detail::withinMagnitude(point.y) ||
            !detail::withinMagnitude(point.z)) {
            return detail::tooLarge(describePosition(planOf(point)));
        }
    }

    // Where each point lies, on the surface as it is.
    const detail::VertexStars stars(surface);
    std::vector<Point3> levels = surface.vertices();
    std::vector<std::optional<double>> given(surface.vertexCount());
    std::size_t relevelled = 0;
    detail::PatchRequest request;
    request.region.assign(surface.triangleCount(), 0);
    std::vector<Point3> outside;
    for (const Point3& point : points) {
        const Point2 position = planOf(point);
        const detail::Place place = detail::placeOf(surface, stars, position);
        switch (place.kind) {
        case detail::Place::Kind::Vertex: {
            // A point at a vertex gives it its level: no triangle changes.
            auto& level = given[place.vertex];
            if (level && std::abs(*level - point.z) > tol::kCoordinate) {
                return makeError(ErrorCode::InvalidGeometry,
                                 "two points at one position with different levels",
                                 "at " + describePosition(position) + ": " +
                                     katana::core::formatExactReal(*level) + " and " +
                                     katana::core::formatExactReal(point.z));
            }
            level = point.z;
            levels[place.vertex].z = point.z;
            break;
        }
        case detail::Place::Kind::Inside:
        case detail::Place::Kind::Edge:
            detail::markInsertionCavity(surface, place, position, request.region);
            request.points.push_back({position, point.z, true});
            break;
        case detail::Place::Kind::Outside:
            outside.push_back(point);
            break;
        }
    }

    for (std::size_t v = 0; v < levels.size(); ++v) {
        relevelled += levels[v].z != surface.vertices()[v].z ? 1 : 0;
    }
    TinSurface current = surface;
    if (relevelled > 0) {
        auto relevel = withLevels(surface, std::move(levels));
        if (!relevel) {
            return relevel.error();
        }
        current = std::move(*relevel);
    }
    TinEditReport report;
    report.pointsRelevelled = relevelled;
    if (!request.points.empty()) {
        // The relevelled surface has the same triangles, so the stars and the
        // cavities found on the original hold for it.
        auto output = detail::rebuildPatch(current, stars, std::move(request));
        if (!output) {
            return output.error();
        }
        auto step = finishPatch(current, std::move(*output));
        if (!step) {
            return step.error();
        }
        report.pointsAdded += step->report.pointsAdded;
        report.pointsRelevelled += step->report.pointsRelevelled;
        report.crossings += step->report.crossings;
        current = std::move(step->surface);
    }
    if (!outside.empty()) {
        auto output = detail::extendSurface(current, outside);
        if (!output) {
            return output.error();
        }
        auto step = finishPatch(current, std::move(*output));
        if (!step) {
            return step.error();
        }
        report.pointsAdded += step->report.pointsAdded;
        report.pointsRelevelled += step->report.pointsRelevelled;
        current = std::move(step->surface);
    }
    TinEditResult result;
    const TinEditReport counts = detail::reportOf(surface, current);
    report.trianglesBefore = counts.trianglesBefore;
    report.trianglesAfter = counts.trianglesAfter;
    report.verticesBefore = counts.verticesBefore;
    report.verticesAfter = counts.verticesAfter;
    result.report = report;
    result.surface = std::move(current);
    return result;
}

Result<TinEditResult> addBreakline(const TinSurface& surface,
                                   std::span<const BreaklineVertex> vertices, bool closed)
{
    const BreaklineInput line{std::vector<BreaklineVertex>(vertices.begin(), vertices.end()), closed};
    return addBreaklines(surface, std::span<const BreaklineInput>(&line, 1));
}

Result<TinEditResult> addBreaklines(const TinSurface& surface, std::span<const BreaklineInput> lines)
{
    if (const Status status = requireSurface(surface); !status) {
        return status.error();
    }
    if (lines.empty()) {
        return makeError(ErrorCode::InvalidArgument, "no breakline was given");
    }
    const detail::VertexStars stars(surface);
    detail::PatchRequest request;
    request.region.assign(surface.triangleCount(), 0);
    // Which line a refusal is about, when there are several.
    const auto where = [&lines](std::size_t line, std::size_t vertex) {
        return (lines.size() > 1 ? "line=" + std::to_string(line) + " " : std::string{}) +
               "vertex=" + std::to_string(vertex);
    };
    for (std::size_t l = 0; l < lines.size(); ++l) {
        // The levels given to a position this line visits more than once are
        // one: each visit takes the level either gives it, and the segments
        // are judged draped or not by the levels they then have (a line that
        // comes back to a point and gives its level on the second visit is
        // straight 3D to it from the first, as the same line written the
        // other way round is).
        std::vector<BreaklineVertex> vertices = lines[l].vertices;
        if (vertices.size() < 2) {
            return makeError(ErrorCode::InvalidArgument, "a breakline needs at least two vertices",
                             "vertices=" + std::to_string(vertices.size()) +
                                 (lines.size() > 1 ? " line=" + std::to_string(l) : std::string{}));
        }
        detail::PointMerger merger(vertices.size(), tol::kGeometric);
        std::vector<std::uint32_t> position(vertices.size());
        std::vector<std::optional<double>> levelAt;
        for (std::size_t i = 0; i < vertices.size(); ++i) {
            const BreaklineVertex& vertex = vertices[i];
            if (!vertex.position.isFinite() || (vertex.z && !std::isfinite(*vertex.z))) {
                return makeError(ErrorCode::InvalidArgument,
                                 "a breakline vertex has a non-finite coordinate", where(l, i));
            }
            if (!detail::withinMagnitude(vertex.position.x) ||
                !detail::withinMagnitude(vertex.position.y) ||
                (vertex.z && !detail::withinMagnitude(*vertex.z))) {
                return detail::tooLarge(describePosition(vertex.position));
            }
            const auto entry = merger.add(vertex.position);
            position[i] = entry.index;
            if (entry.inserted) {
                levelAt.emplace_back();
            }
            if (vertex.z) {
                auto& level = levelAt[entry.index];
                if (level && std::abs(*level - *vertex.z) > tol::kCoordinate) {
                    return makeError(ErrorCode::InvalidGeometry,
                                     "two points at one position with different levels",
                                     "at " + describePosition(vertex.position) + ": " +
                                         katana::core::formatExactReal(*level) + " and " +
                                         katana::core::formatExactReal(*vertex.z));
                }
                if (!level) {
                    level = vertex.z;
                }
            }
        }
        for (std::size_t i = 0; i < vertices.size(); ++i) {
            if (!vertices[i].z) {
                vertices[i].z = levelAt[position[i]];
            }
        }

        const std::uint32_t base = static_cast<std::uint32_t>(request.points.size());
        bool hasLength = false;
        for (std::size_t i = 0; i < vertices.size(); ++i) {
            const BreaklineVertex& vertex = vertices[i];
            const detail::Place place = detail::placeOf(surface, stars, vertex.position);
            if (place.kind == detail::Place::Kind::Outside) {
                return makeError(ErrorCode::InvalidGeometry,
                                 "a breakline vertex lies off the surface",
                                 where(l, i) + " at " + describePosition(vertex.position));
            }
            // A draped vertex goes in without a level: the patch gives it the
            // surface's, and one at an existing vertex is that vertex, untouched.
            request.points.push_back({vertex.position, vertex.z, true});
            hasLength = hasLength ||
                        vertex.position.distanceTo(vertices.front().position) > tol::kGeometric;
        }
        if (!hasLength) {
            return makeError(ErrorCode::InvalidGeometry,
                             "the breakline has no length: its vertices coincide",
                             lines.size() > 1 ? "line=" + std::to_string(l) : std::string{});
        }
        const std::size_t segments =
            lines[l].closed && vertices.size() > 2 ? vertices.size() : vertices.size() - 1;
        for (std::size_t s = 0; s < segments; ++s) {
            const std::size_t from = s;
            const std::size_t to = (s + 1) % vertices.size();
            const bool draped = !vertices[from].z && !vertices[to].z;
            request.lines.push_back({base + static_cast<std::uint32_t>(from),
                                     base + static_cast<std::uint32_t>(to), draped});
            request.keepShape = request.keepShape || draped;
        }
    }
    auto output = detail::rebuildPatch(surface, stars, std::move(request));
    if (!output) {
        return output.error();
    }
    return finishPatch(surface, std::move(*output));
}

namespace {

Result<TinEditResult> cutBy(const TinSurface& surface, const Polyline2& polygon, bool keepInside)
{
    if (const Status status = requireSurface(surface); !status) {
        return status.error();
    }
    const std::string what = keepInside ? "the boundary" : "the hole";
    auto ring = cleanPolygon(polygon, what);
    if (!ring) {
        return ring.error();
    }
    const detail::VertexStars stars(surface);
    detail::PatchRequest request;
    request.region.assign(surface.triangleCount(), 0);
    request.keepShape = true;
    request.keepInsideCut = keepInside;
    request.simpleCut = true;
    const auto shape = std::make_shared<const katana::geometry::PolygonClassifier>(*ring);
    request.insideCut = [shape](const Point2& p) {
        return shape->classify(p) != Containment::Outside;
    };
    const std::size_t count = ring->vertices.size();
    for (std::size_t i = 0; i < count; ++i) {
        request.points.push_back({ring->vertices[i], std::nullopt, false});
        request.cuts.push_back(
            {static_cast<std::uint32_t>(i), static_cast<std::uint32_t>((i + 1) % count), false});
    }
    auto output = detail::rebuildPatch(surface, stars, std::move(request));
    if (!output) {
        return output.error();
    }
    if (output->triangles.empty()) {
        return makeError(ErrorCode::InvalidState,
                         keepInside ? "nothing of the surface lies inside the boundary"
                                    : "the hole covers the whole surface; nothing would be left");
    }
    // Nothing rebuilt and nothing dropped - the polygon is round the whole
    // surface, or beside it: the surface as it was, not a reordered copy.
    if (output->triangles == surface.triangles()) {
        return detail::unchanged(surface);
    }
    return finishPatch(surface, std::move(*output));
}

} // namespace

Result<TinEditResult> clipToBoundary(const TinSurface& surface, const Polyline2& boundary)
{
    return cutBy(surface, boundary, true);
}

Result<TinEditResult> cutHole(const TinSurface& surface, const Polyline2& hole)
{
    return cutBy(surface, hole, false);
}

// ---- changing levels ------------------------------------------------------------------------

Result<TinEditResult> changeHeights(const TinSurface& surface, const HeightChange& change,
                                    const std::optional<Polyline2>& inside)
{
    if (const Status status = requireSurface(surface); !status) {
        return status.error();
    }
    if (!std::isfinite(change.value) || (change.base && !std::isfinite(*change.base))) {
        return makeError(ErrorCode::InvalidArgument, "a level change must be a finite number");
    }
    if (!detail::withinMagnitude(change.value) ||
        (change.base && !detail::withinMagnitude(*change.base))) {
        return detail::tooLarge("the level change " + katana::core::formatExactReal(change.value));
    }
    std::optional<katana::geometry::PolygonClassifier> ring;
    if (inside) {
        auto cleaned = cleanPolygon(*inside, "the polygon");
        if (!cleaned) {
            return cleaned.error();
        }
        ring.emplace(*cleaned);
    }
    const std::vector<std::uint8_t> used = referencedVertices(surface);
    std::vector<std::uint32_t> affected;
    for (std::uint32_t v = 0; v < surface.vertexCount(); ++v) {
        if (used[v] != 0 &&
            (!ring || ring->classify(planOf(surface.vertices()[v])) != Containment::Outside)) {
            affected.push_back(v);
        }
    }
    if (affected.empty()) {
        return makeError(ErrorCode::InvalidArgument, "no vertex of the surface lies inside the polygon");
    }
    double base = 0.0;
    if (change.operation == HeightOperation::Scale) {
        if (change.base) {
            base = *change.base;
        } else {
            base = std::numeric_limits<double>::infinity();
            for (const std::uint32_t v : affected) {
                base = std::min(base, surface.vertices()[v].z);
            }
        }
    }
    std::vector<Point3> vertices = surface.vertices();
    TinEditReport report;
    for (const std::uint32_t v : affected) {
        const double z = vertices[v].z;
        double next = z;
        switch (change.operation) {
        case HeightOperation::Raise:
            next = z + change.value;
            break;
        case HeightOperation::SetLevel:
            next = change.value;
            break;
        case HeightOperation::Scale:
            next = base + change.value * (z - base);
            break;
        }
        if (!std::isfinite(next)) {
            return makeError(ErrorCode::InvalidArgument, "the change makes a level non-finite",
                             "vertex=" + std::to_string(v));
        }
        if (!detail::withinMagnitude(next)) {
            return detail::tooLarge("vertex=" + std::to_string(v) + " would be at level " +
                                    katana::core::formatExactReal(next));
        }
        if (next != z) {
            ++report.pointsMoved;
            report.largestChange = std::max(report.largestChange, std::abs(next - z));
        }
        vertices[v].z = next;
    }
    auto changed = withLevels(surface, std::move(vertices));
    if (!changed) {
        return changed.error();
    }
    TinEditResult result;
    const TinEditReport counts = detail::reportOf(surface, *changed);
    report.trianglesBefore = counts.trianglesBefore;
    report.trianglesAfter = counts.trianglesAfter;
    report.verticesBefore = counts.verticesBefore;
    report.verticesAfter = counts.verticesAfter;
    result.report = report;
    result.surface = std::move(*changed);
    return result;
}

Result<TinEditResult> smoothSurface(const TinSurface& surface, const SmoothOptions& options,
                                    const std::optional<Polyline2>& inside)
{
    if (const Status status = requireSurface(surface); !status) {
        return status.error();
    }
    // A thousand passes of the mildest weight move a vertex further than any
    // smoothing a surveyor asks for; more is a typing slip, not a request.
    constexpr int kMostPasses = 1000;
    if (options.passes < 1 || options.passes > kMostPasses) {
        return makeError(ErrorCode::InvalidArgument, "passes must be from 1 to 1000",
                         "passes=" + std::to_string(options.passes));
    }
    if (!(options.weight > 0.0 && options.weight <= 1.0)) {
        return makeError(ErrorCode::InvalidArgument, "weight must be above 0 and at most 1",
                         "weight=" + katana::core::formatExactReal(options.weight));
    }
    std::optional<katana::geometry::PolygonClassifier> ring;
    if (inside) {
        auto cleaned = cleanPolygon(*inside, "the polygon");
        if (!cleaned) {
            return cleaned.error();
        }
        ring.emplace(*cleaned);
    }

    // Each vertex's neighbours along an edge, once each (two flat arrays).
    const std::size_t count = surface.vertexCount();
    std::vector<std::pair<std::uint32_t, std::uint32_t>> pairs;
    pairs.reserve(6 * surface.triangleCount());
    for (const TinTriangle& tri : surface.triangles()) {
        for (std::size_t k = 0; k < 3; ++k) {
            pairs.emplace_back(tri[k], tri[(k + 1) % 3]);
            pairs.emplace_back(tri[(k + 1) % 3], tri[k]);
        }
    }
    std::sort(pairs.begin(), pairs.end());
    pairs.erase(std::unique(pairs.begin(), pairs.end()), pairs.end());
    std::vector<std::uint32_t> start(count + 1, 0);
    for (const auto& pair : pairs) {
        ++start[pair.first + 1];
    }
    for (std::size_t v = 0; v < count; ++v) {
        start[v + 1] += start[v];
    }

    const std::vector<std::uint8_t> held = heldVertices(surface);
    const std::vector<std::uint8_t> used = referencedVertices(surface);
    std::vector<std::uint8_t> free(count, 0);
    TinEditReport report;
    std::size_t freeCount = 0;
    for (std::uint32_t v = 0; v < count; ++v) {
        if (used[v] == 0 ||
            (ring && ring->classify(planOf(surface.vertices()[v])) == Containment::Outside)) {
            continue;
        }
        if (held[v] != 0) {
            ++report.pointsHeld;
            continue;
        }
        free[v] = 1;
        ++freeCount;
    }
    if (freeCount == 0 && ring) {
        // The same words changeHeights uses for a polygon with no vertex in
        // it: "every one is held" is a claim about vertices that exist.
        if (report.pointsHeld == 0) {
            return makeError(ErrorCode::InvalidArgument,
                             "no vertex of the surface lies inside the polygon");
        }
        return makeError(ErrorCode::InvalidArgument,
                         "no vertex inside the polygon is free to move: every one is on the rim "
                         "or a breakline",
                         "held=" + std::to_string(report.pointsHeld));
    }

    std::vector<double> z(count);
    for (std::size_t v = 0; v < count; ++v) {
        z[v] = surface.vertices()[v].z;
    }
    std::vector<double> next = z;
    for (int pass = 0; pass < options.passes; ++pass) {
        for (std::size_t v = 0; v < count; ++v) {
            if (free[v] == 0 || start[v + 1] == start[v]) {
                continue;
            }
            double sum = 0.0;
            for (std::uint32_t i = start[v]; i < start[v + 1]; ++i) {
                sum += z[pairs[i].second];
            }
            const double mean = sum / static_cast<double>(start[v + 1] - start[v]);
            next[v] = z[v] + options.weight * (mean - z[v]);
        }
        z = next;
    }
    std::vector<Point3> vertices = surface.vertices();
    for (std::size_t v = 0; v < count; ++v) {
        if (z[v] != vertices[v].z) {
            ++report.pointsMoved;
            report.largestChange = std::max(report.largestChange, std::abs(z[v] - vertices[v].z));
            vertices[v].z = z[v];
        }
    }
    auto smoothed = withLevels(surface, std::move(vertices));
    if (!smoothed) {
        return smoothed.error();
    }
    TinEditResult result;
    const TinEditReport counts = detail::reportOf(surface, *smoothed);
    report.trianglesBefore = counts.trianglesBefore;
    report.trianglesAfter = counts.trianglesAfter;
    report.verticesBefore = counts.verticesBefore;
    report.verticesAfter = counts.verticesAfter;
    result.report = report;
    result.surface = std::move(*smoothed);
    return result;
}

// ---- surfaces made of others ----------------------------------------------------------------

Result<TinEditResult> pasteSurface(const TinSurface& base, const TinSurface& top)
{
    if (const Status status = requireSurface(base, "the base surface"); !status) {
        return status.error();
    }
    if (const Status status = requireSurface(top, "the top surface"); !status) {
        return status.error();
    }
    const auto& topVertices = top.vertices();
    const auto& topTriangles = top.triangles();

    // The top's rim, each edge directed with the top on its left: the cut.
    std::vector<std::uint32_t> pointOfVertex(top.vertexCount(), kNoVertex);
    std::vector<std::uint32_t> cutOfEdge(3 * top.triangleCount(), kNoVertex);
    detail::PatchRequest request;
    request.region.assign(base.triangleCount(), 0);
    for (std::uint32_t t = 0; t < top.triangleCount(); ++t) {
        for (std::uint32_t k = 0; k < 3; ++k) {
            if (top.neighbors()[t][k] != kNoTriangle) {
                continue;
            }
            std::uint32_t ends[2] = {topTriangles[t][k], topTriangles[t][(k + 1) % 3]};
            for (std::uint32_t& end : ends) {
                if (pointOfVertex[end] == kNoVertex) {
                    pointOfVertex[end] = static_cast<std::uint32_t>(request.points.size());
                    request.points.push_back(
                        {planOf(topVertices[end]), topVertices[end].z, false});
                }
                end = pointOfVertex[end];
            }
            cutOfEdge[3 * t + k] = static_cast<std::uint32_t>(request.cuts.size());
            request.cuts.push_back({ends[0], ends[1], false});
        }
    }
    request.keepShape = true;
    request.keepInsideCut = false;
    request.cutHasLevels = true;
    request.pointsMoveVertices = true;
    request.insideCut = [&top](const Point2& p) { return top.locate(p).has_value(); };

    const detail::VertexStars stars(base);
    auto patched = detail::rebuildPatch(base, stars, std::move(request));
    if (!patched) {
        return patched.error();
    }
    detail::PatchOutput output = std::move(*patched);

    // The top's vertices: its rim's are the seam's, the rest are its own.
    std::vector<std::uint32_t> resultOf(top.vertexCount(), kNoVertex);
    for (std::uint32_t v = 0; v < top.vertexCount(); ++v) {
        if (pointOfVertex[v] != kNoVertex) {
            resultOf[v] = output.pointVertex[pointOfVertex[v]];
        }
    }
    const auto mapped = [&](std::uint32_t v) {
        if (resultOf[v] == kNoVertex) {
            resultOf[v] = static_cast<std::uint32_t>(output.vertices.size());
            output.vertices.push_back(topVertices[v]);
        }
        return resultOf[v];
    };
    for (std::uint32_t t = 0; t < top.triangleCount(); ++t) {
        const TinTriangle& tri = topTriangles[t];
        const std::uint8_t mask = top.constrainedEdges()[t];
        // The vertices along each edge: a rim edge split where the seam met
        // the base's edges, the others as they are.
        std::vector<std::uint32_t> sides[3];
        std::size_t split = 0;
        std::uint32_t splitEdge = 0;
        for (std::uint32_t k = 0; k < 3; ++k) {
            const std::uint32_t cut = cutOfEdge[3 * t + k];
            if (cut != kNoVertex && output.cutVertices[cut].size() > 2) {
                sides[k] = output.cutVertices[cut];
                ++split;
                splitEdge = k;
            } else {
                sides[k] = {mapped(tri[k]), mapped(tri[(k + 1) % 3])};
            }
        }
        const auto flag = [mask](std::uint32_t k) {
            return static_cast<std::uint8_t>((mask >> k) & 1u);
        };
        if (split == 0) {
            output.triangles.push_back({sides[0][0], sides[1][0], sides[2][0]});
            output.masks.push_back(mask);
        } else if (split == 1) {
            // A fan from the corner opposite the split edge.
            const std::vector<std::uint32_t>& run = sides[splitEdge];
            const std::uint32_t opposite = mapped(tri[(splitEdge + 2) % 3]);
            for (std::size_t i = 0; i + 1 < run.size(); ++i) {
                std::uint8_t m = flag(splitEdge);
                if (i + 2 == run.size()) {
                    m = static_cast<std::uint8_t>(m | (flag((splitEdge + 1) % 3) << 1));
                }
                if (i == 0) {
                    m = static_cast<std::uint8_t>(m | (flag((splitEdge + 2) % 3) << 2));
                }
                output.triangles.push_back({run[i], run[i + 1], opposite});
                output.masks.push_back(m);
            }
        } else {
            // Two or three sides split: a fan from the centroid, which lies on
            // the top's own plane there, so the top keeps its shape.
            const Point3& a = topVertices[tri[0]];
            const Point3& b = topVertices[tri[1]];
            const Point3& c = topVertices[tri[2]];
            const auto centre = static_cast<std::uint32_t>(output.vertices.size());
            output.vertices.emplace_back((a.x + b.x + c.x) / 3.0, (a.y + b.y + c.y) / 3.0,
                                         (a.z + b.z + c.z) / 3.0);
            for (std::uint32_t k = 0; k < 3; ++k) {
                for (std::size_t i = 0; i + 1 < sides[k].size(); ++i) {
                    output.triangles.push_back({sides[k][i], sides[k][i + 1], centre});
                    output.masks.push_back(flag(k));
                }
            }
        }
    }
    // The seam is constrained on the base's side (its cut edges); finishSurface
    // carries the flag across to the top's. A centroid is a vertex the fans
    // use, so finishPatch counts it as added with the others: counting it
    // again here made vertices_after - vertices_before + points_removed differ
    // from points_added by one for each fan.
    return finishPatch(base, std::move(output));
}

Result<TinEditResult> offsetSurface(const TinSurface& surface, double dz)
{
    return changeHeights(surface, HeightChange{HeightOperation::Raise, dz, std::nullopt});
}

Result<TinEditResult> mergeSurfaces(const std::vector<const TinSurface*>& inRankOrder)
{
    if (inRankOrder.size() < 2) {
        return makeError(ErrorCode::InvalidArgument, "a merge needs at least two surfaces",
                         "surfaces=" + std::to_string(inRankOrder.size()));
    }
    std::size_t trianglesBefore = 0;
    std::size_t verticesBefore = 0;
    for (std::size_t i = 0; i < inRankOrder.size(); ++i) {
        if (inRankOrder[i] == nullptr || inRankOrder[i]->empty()) {
            return makeError(ErrorCode::InvalidArgument, "a surface to merge is empty",
                             "surface=" + std::to_string(i + 1));
        }
        trianglesBefore += inRankOrder[i]->triangleCount();
        verticesBefore += inRankOrder[i]->vertexCount();
    }
    auto combined = combineSurfaces(inRankOrder);
    if (!combined) {
        return combined.error();
    }
    if (combined->empty()) {
        return makeError(ErrorCode::InvalidState, "nothing is left of the surfaces merged");
    }
    // combineSurfaces keeps the vertices of dropped triangles; a merged
    // surface kept in the drawing has no use for them.
    auto finished = detail::finishSurface(combined->vertices(), combined->triangles(),
                                          combined->constrainedEdges());
    if (!finished) {
        return finished.error();
    }
    TinEditResult result;
    result.report.trianglesBefore = trianglesBefore;
    result.report.verticesBefore = verticesBefore;
    result.report.trianglesAfter = finished->triangleCount();
    result.report.verticesAfter = finished->vertexCount();
    result.surface = std::move(*finished);
    return result;
}

} // namespace katana::terrain
