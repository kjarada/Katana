#include "tin_patch.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <limits>
#include <string>
#include <utility>

#include "cdt_backend.hpp"
#include "katana/core/text.hpp"
#include "katana/math/numerics.hpp"
#include "point_merge.hpp"

namespace katana::terrain::detail {

namespace tol = katana::math::tolerance;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;
using katana::geometry::Segment2;

namespace {

constexpr double kNaN = std::numeric_limits<double>::quiet_NaN();

// Rounds of growth before the whole surface becomes the region. A round adds
// the neighbours across every ring edge something split, so a run of
// vertices lying exactly on one line across the ring (a degenerate stretch
// of an imported surface) costs a round per vertex; 32 covers any such run
// met in practice, and the fallback after it is always correct, only slower.
constexpr int kGrowthRounds = 32;

// The triangle grid is sized like the surface's own locator
// (tin_surface.cpp, kTrianglesPerCell): about two triangles a cell.
constexpr double kGridTrianglesPerCell = 2.0;

// kGeometric with a safety factor for rounding: sixty-four ulps of the
// coordinates' magnitude cover the error of the cross products below (a few
// ulps of |edge| * |offset| each), as super_surface.cpp's margin does.
double roundingMargin(double scale)
{
    return tol::kGeometric + 64.0 * std::numeric_limits<double>::epsilon() * scale;
}

Point2 planOf(const Point3& p) { return Point2(p.x, p.y); }

std::uint64_t edgeKey(std::uint32_t from, std::uint32_t to)
{
    return (static_cast<std::uint64_t>(from) << 32) | to;
}

// A set of directed edges, as sorted keys.
class EdgeSet {
  public:
    void add(std::uint32_t from, std::uint32_t to) { keys_.push_back(edgeKey(from, to)); }
    void seal()
    {
        std::sort(keys_.begin(), keys_.end());
        keys_.erase(std::unique(keys_.begin(), keys_.end()), keys_.end());
    }
    [[nodiscard]] bool has(std::uint32_t from, std::uint32_t to) const
    {
        return std::binary_search(keys_.begin(), keys_.end(), edgeKey(from, to));
    }
    [[nodiscard]] bool empty() const { return keys_.empty(); }

  private:
    std::vector<std::uint64_t> keys_;
};

std::uint32_t cellIndex(double value, double origin, double size, std::uint32_t count)
{
    const double cell = std::floor((value - origin) / size);
    if (!(cell > 0.0)) {
        return 0;
    }
    return cell >= static_cast<double>(count) ? count - 1 : static_cast<std::uint32_t>(cell);
}

// Whether the segment p-q may reach the triangle a, b, c: false only when a
// line separates them by more than `margin`. Inexact on purpose and
// conservative (see TriangleGrid::markAlong).
bool segmentNearTriangle(const Point2& p, const Point2& q, const Point2& a, const Point2& b,
                         const Point2& c, double margin)
{
    const double twiceArea = (b - a).cross(c - a);
    const double side = twiceArea >= 0.0 ? 1.0 : -1.0;
    const Point2 corners[3] = {a, b, c};
    for (int k = 0; k < 3; ++k) {
        const Point2& from = corners[k];
        const katana::geometry::Vec2 d = corners[(k + 1) % 3] - from;
        const double reach = margin * (std::abs(d.x) + std::abs(d.y));
        if (side * d.cross(p - from) < -reach && side * d.cross(q - from) < -reach) {
            return false;
        }
    }
    const katana::geometry::Vec2 d = q - p;
    const double reach = margin * (std::abs(d.x) + std::abs(d.y));
    if (reach > 0.0) {
        const double sa = d.cross(a - p);
        const double sb = d.cross(b - p);
        const double sc = d.cross(c - p);
        if ((sa > reach && sb > reach && sc > reach) || (sa < -reach && sb < -reach && sc < -reach)) {
            return false;
        }
    }
    return true;
}

// Whether the open triangle a, b, c (counter-clockwise) and the closed
// segment p-q share a point, exactly: they are disjoint when an edge line of
// the triangle has the whole segment on its closed outer side, or the
// segment's line has the whole triangle on one closed side (the separating
// axes of two convex sets in the plane).
bool segmentMeetsInterior(const Point2& a, const Point2& b, const Point2& c, const Point2& p,
                          const Point2& q)
{
    const Point2 corners[3] = {a, b, c};
    for (int k = 0; k < 3; ++k) {
        if (orientation(corners[k], corners[(k + 1) % 3], p) <= 0 &&
            orientation(corners[k], corners[(k + 1) % 3], q) <= 0) {
            return false;
        }
    }
    if (p == q) {
        return true; // strictly inside every edge: inside the triangle
    }
    const int oa = orientation(p, q, a);
    const int ob = orientation(p, q, b);
    const int oc = orientation(p, q, c);
    return !((oa >= 0 && ob >= 0 && oc >= 0) || (oa <= 0 && ob <= 0 && oc <= 0));
}

// Whether `p` lies strictly inside the circumcircle of the counter-clockwise
// triangle a, b, c. Inexact (it only chooses which triangles an inserted
// point re-triangulates; the kernel decides the triangles), evaluated on
// differences from p so that large coordinates keep their precision.
bool inCircumcircle(const Point2& a, const Point2& b, const Point2& c, const Point2& p)
{
    const double adx = a.x - p.x;
    const double ady = a.y - p.y;
    const double bdx = b.x - p.x;
    const double bdy = b.y - p.y;
    const double cdx = c.x - p.x;
    const double cdy = c.y - p.y;
    const double ad = adx * adx + ady * ady;
    const double bd = bdx * bdx + bdy * bdy;
    const double cd = cdx * cdx + cdy * cdy;
    const double determinant = adx * (bdy * cd - bd * cdy) - ady * (bdx * cd - bd * cdx) +
                               ad * (bdx * cdy - bdy * cdx);
    return determinant > 0.0;
}

// What a constraint of the patch is, for the level of a vertex made on it.
struct ConstraintInfo {
    std::uint8_t tag = 0;
    Point2 from;
    Point2 to;
    double zFrom = kNaN;
    double zTo = kNaN;
    // A ring edge: the region's triangle and edge it is, and whether the
    // surface ends there.
    std::uint32_t triangle = kNoTriangle;
    std::uint32_t edge = 0;
    bool rim = false;
    bool draped = false;
};

double levelAlong(const ConstraintInfo& info, const Point2& position)
{
    const double t = std::clamp(Segment2{info.from, info.to}.parameterOf(position), 0.0, 1.0);
    return katana::math::lerp(info.zFrom, info.zTo, t);
}

// Which level holds at a vertex made where constraints cross: the lower the
// rank the stronger (docs/surface_editing.md, "Levels of new vertices").
int levelRank(const ConstraintInfo& info, bool cutHasLevels)
{
    if ((info.tag & kTagCut) != 0) {
        return cutHasLevels ? 0 : 4;
    }
    if ((info.tag & (kTagKept | kTagRegion)) != 0) {
        return 1;
    }
    if ((info.tag & kTagLine) != 0) {
        return info.draped ? 4 : 2;
    }
    return 3; // a shape edge
}

bool allCollinear(std::span<const Point2> positions)
{
    for (std::size_t i = 2; i < positions.size(); ++i) {
        if (orientation(positions[0], positions[1], positions[i]) != 0) {
            return false;
        }
    }
    return true;
}

struct Attempt {
    std::optional<PatchOutput> output;
    std::vector<std::uint32_t> grow;
};

Result<Attempt> tryPatch(const TinSurface& surface, const PatchRequest& request,
                         const std::vector<std::uint8_t>& region)
{
    const auto& sourceVertices = surface.vertices();
    const auto& triangles = surface.triangles();
    const auto& neighbors = surface.neighbors();
    const auto& masks = surface.constrainedEdges();
    const std::size_t vertexCount = sourceVertices.size();
    const auto removed = [&request](std::uint32_t v) {
        return !request.removed.empty() && request.removed[v] != 0;
    };
    const auto inRegion = [&region](std::uint32_t t) { return t != kNoTriangle && region[t] != 0; };

    std::vector<std::uint32_t> regionTriangles;
    for (std::uint32_t t = 0; t < triangles.size(); ++t) {
        if (region[t] != 0) {
            regionTriangles.push_back(t);
        }
    }

    // ---- the points of the sub-problem: the region's vertices, then the request's
    std::vector<Point3> vertices = sourceVertices;
    PointMerger merger(3 * regionTriangles.size() + request.points.size() + 1, tol::kGeometric);
    std::vector<std::uint32_t> subOfVertex(vertexCount, kNoIndex);
    std::vector<std::uint32_t> vertexOfSub;
    std::vector<std::uint32_t> pointOfSub;
    for (const std::uint32_t t : regionTriangles) {
        for (const std::uint32_t v : triangles[t]) {
            if (removed(v) || subOfVertex[v] != kNoIndex) {
                continue;
            }
            const auto entry = merger.add(planOf(sourceVertices[v]));
            if (!entry.inserted) {
                return makeError(ErrorCode::InvalidGeometry,
                                 "the surface has two vertices closer than the coincidence "
                                 "tolerance; an edit there cannot tell them apart",
                                 "vertices " + std::to_string(vertexOfSub[entry.index]) + " and " +
                                     std::to_string(v) + " at " +
                                     describePosition(planOf(sourceVertices[v])));
            }
            subOfVertex[v] = entry.index;
            vertexOfSub.push_back(v);
            pointOfSub.push_back(kNoIndex);
        }
    }
    const std::size_t surfaceSubCount = vertexOfSub.size();

    std::size_t relevelled = 0;
    std::vector<std::uint32_t> pointSub(request.points.size(), kNoIndex);
    // The level a request point gave a surface vertex, to refuse two that disagree.
    std::vector<std::optional<double>> givenLevel(surfaceSubCount);
    std::vector<std::uint8_t> relevelledSub(surfaceSubCount, 0);
    for (std::size_t i = 0; i < request.points.size(); ++i) {
        const PatchPoint& point = request.points[i];
        const auto entry = merger.add(point.position);
        pointSub[i] = entry.index;
        if (entry.inserted) {
            vertexOfSub.push_back(kNoVertex);
            pointOfSub.push_back(static_cast<std::uint32_t>(i));
            continue;
        }
        const auto conflict = [&point](double other) {
            return point.z && std::abs(*point.z - other) > tol::kCoordinate;
        };
        if (entry.index < surfaceSubCount) {
            const std::uint32_t v = vertexOfSub[entry.index];
            if (point.z) {
                if (givenLevel[entry.index] && conflict(*givenLevel[entry.index])) {
                    return makeError(ErrorCode::InvalidGeometry,
                                     "two points at one position with different levels",
                                     "at " + describePosition(point.position) + ": " +
                                         katana::core::formatExactReal(*givenLevel[entry.index]) +
                                         " and " + katana::core::formatExactReal(*point.z));
                }
                givenLevel[entry.index] = point.z;
                if (vertices[v].z != *point.z) {
                    vertices[v].z = *point.z;
                    if (relevelledSub[entry.index] == 0) {
                        relevelledSub[entry.index] = 1;
                        ++relevelled;
                    }
                }
            }
            if (request.pointsMoveVertices) {
                vertices[v].x = point.position.x;
                vertices[v].y = point.position.y;
            }
            continue;
        }
        const PatchPoint& earlier = request.points[pointOfSub[entry.index]];
        if (earlier.z && conflict(*earlier.z)) {
            return makeError(ErrorCode::InvalidGeometry,
                             "two points at one position with different levels",
                             "at " + describePosition(point.position) + ": " +
                                 katana::core::formatExactReal(*earlier.z) + " and " +
                                 katana::core::formatExactReal(*point.z));
        }
    }
    const std::vector<Point2>& positions = merger.positions();

    // Every sub-problem point's level: a surface vertex's own (as relevelled),
    // a point's given one, else the surface's under it (none off the surface).
    std::vector<double> subLevel(positions.size(), kNaN);
    for (std::size_t s = 0; s < positions.size(); ++s) {
        if (vertexOfSub[s] != kNoVertex) {
            subLevel[s] = vertices[vertexOfSub[s]].z;
        } else {
            const PatchPoint& point = request.points[pointOfSub[s]];
            if (point.z) {
                subLevel[s] = *point.z;
            } else if (const auto z = surface.elevationAt(point.position)) {
                subLevel[s] = *z;
            }
        }
    }

    // ---- constraints ----------------------------------------------------------
    std::vector<CdtConstraint> constraints;
    std::vector<ConstraintInfo> infos;
    std::vector<std::pair<Point2, Point2>> openRing; // ring edges at a removed vertex
    const auto addConstraint = [&](std::uint32_t from, std::uint32_t to, std::uint8_t tag,
                                   ConstraintInfo info) {
        constraints.push_back({from, to, tag});
        info.tag = tag;
        info.from = positions[from];
        info.to = positions[to];
        if (std::isnan(info.zFrom)) {
            info.zFrom = subLevel[from];
            info.zTo = subLevel[to];
        }
        infos.push_back(info);
    };
    for (const std::uint32_t t : regionTriangles) {
        for (std::uint32_t k = 0; k < 3; ++k) {
            const std::uint32_t a = triangles[t][k];
            const std::uint32_t b = triangles[t][(k + 1) % 3];
            const std::uint32_t across = neighbors[t][k];
            const bool constrained = ((masks[t] >> k) & 1u) != 0;
            if (!inRegion(across)) {
                if (removed(a) || removed(b)) {
                    if (across != kNoTriangle) {
                        return makeError(ErrorCode::Internal,
                                         "a removed vertex's triangles are not all rebuilt",
                                         "triangle=" + std::to_string(t));
                    }
                    openRing.emplace_back(planOf(sourceVertices[a]), planOf(sourceVertices[b]));
                    continue;
                }
                ConstraintInfo info;
                info.triangle = t;
                info.edge = k;
                info.rim = across == kNoTriangle;
                addConstraint(subOfVertex[a], subOfVertex[b],
                              static_cast<std::uint8_t>(kTagRegion | (constrained ? kTagKept : 0)),
                              info);
            } else if (t < across && !removed(a) && !removed(b)) {
                if (constrained) {
                    addConstraint(subOfVertex[a], subOfVertex[b], kTagKept, {});
                } else if (request.keepShape) {
                    addConstraint(subOfVertex[a], subOfVertex[b], kTagShape, {});
                }
            }
        }
    }
    const std::size_t lineBegin = constraints.size();
    for (const PatchSegment& segment : request.lines) {
        const std::uint32_t from = pointSub[segment.from];
        const std::uint32_t to = pointSub[segment.to];
        if (from != to) {
            ConstraintInfo info;
            info.draped = segment.draped;
            addConstraint(from, to, kTagLine, info);
        }
    }
    const std::size_t lineEnd = constraints.size();
    std::vector<std::size_t> cutConstraint(request.cuts.size(), kNoIndex);
    for (std::size_t s = 0; s < request.cuts.size(); ++s) {
        const std::uint32_t from = pointSub[request.cuts[s].from];
        const std::uint32_t to = pointSub[request.cuts[s].to];
        if (from != to) {
            cutConstraint[s] = constraints.size();
            addConstraint(from, to, kTagCut, {});
        }
    }

    // ---- the triangulation ------------------------------------------------------
    CdtOutput cdt;
    bool triangulated = false;
    if (!regionTriangles.empty() && positions.size() >= 3 && !allCollinear(positions)) {
        auto result = triangulate({positions, constraints, true});
        if (!result) {
            return result.error();
        }
        cdt = std::move(*result);
        triangulated = true;
    }
    const std::size_t inputCount = positions.size();
    const std::size_t generatedCount = cdt.generatedPoints.size();
    const auto positionOf = [&](std::uint32_t sub) {
        return sub < inputCount ? positions[sub] : cdt.generatedPoints[sub - inputCount];
    };
    // The vertices along constraint c, from its `from` to its `to`; without a
    // triangulation, its two ends.
    const auto along = [&](std::size_t c) -> std::vector<std::uint32_t> {
        if (!triangulated) {
            return {constraints[c].from, constraints[c].to};
        }
        return {cdt.constraintVertices.begin() + cdt.constraintVertexOffsets[c],
                cdt.constraintVertices.begin() + cdt.constraintVertexOffsets[c + 1]};
    };

    // Which constraints each made vertex lies on.
    std::vector<std::vector<std::uint32_t>> onConstraints(generatedCount);
    std::vector<std::uint32_t> cutAppearances(inputCount + generatedCount, 0);
    if (triangulated) {
        for (std::size_t c = 0; c < constraints.size(); ++c) {
            const auto list = along(c);
            for (const std::uint32_t x : list) {
                if (x >= inputCount) {
                    onConstraints[x - inputCount].push_back(static_cast<std::uint32_t>(c));
                }
                if ((infos[c].tag & kTagCut) != 0) {
                    ++cutAppearances[x];
                }
            }
        }
    }

    // ---- a split ring edge grows the region; a line across the rim is refused
    std::vector<std::uint32_t> grow;
    for (std::size_t c = 0; c < constraints.size() && triangulated; ++c) {
        if ((infos[c].tag & kTagRegion) == 0) {
            continue;
        }
        const auto list = along(c);
        if (list.size() <= 2) {
            continue;
        }
        if (!infos[c].rim) {
            grow.push_back(neighbors[infos[c].triangle][infos[c].edge]);
            continue;
        }
        for (std::size_t i = 1; i + 1 < list.size(); ++i) {
            if (list[i] < inputCount) {
                continue;
            }
            for (const std::uint32_t other : onConstraints[list[i] - inputCount]) {
                if ((infos[other].tag & kTagLine) != 0) {
                    return makeError(ErrorCode::InvalidGeometry,
                                     "the breakline leaves the surface: it crosses the rim",
                                     "at " + describePosition(positionOf(list[i])));
                }
            }
        }
    }
    if (!grow.empty()) {
        Attempt attempt;
        attempt.grow = std::move(grow);
        return attempt;
    }
    for (std::size_t s = 0; s < request.cuts.size() && triangulated; ++s) {
        if (cutConstraint[s] == kNoIndex) {
            continue;
        }
        const auto list = along(cutConstraint[s]);
        for (std::size_t i = 1; i + 1 < list.size(); ++i) {
            if (cutAppearances[list[i]] > 1) {
                return makeError(ErrorCode::InvalidGeometry,
                                 "the polygon touches or crosses itself",
                                 "at " + describePosition(positionOf(list[i])));
            }
        }
    }

    // ---- which new triangles are inside the region ---------------------------
    const std::size_t newCount = cdt.triangles.size();
    std::vector<std::uint8_t> state(newCount, 0); // 0 not known, 1 inside, 2 outside
    if (triangulated) {
        EdgeSet ring;
        for (std::size_t c = 0; c < constraints.size(); ++c) {
            if ((infos[c].tag & kTagRegion) != 0) {
                const auto list = along(c);
                for (std::size_t i = 0; i + 1 < list.size(); ++i) {
                    ring.add(list[i], list[i + 1]);
                }
            }
        }
        ring.seal();
        std::vector<std::uint32_t> queue;
        for (std::uint32_t t = 0; t < newCount; ++t) {
            const auto& tri = cdt.triangles[t];
            bool inside = false;
            bool outside = false;
            for (std::size_t k = 0; k < 3; ++k) {
                if ((cdt.edgeTags[t][k] & kTagRegion) != 0) {
                    (ring.has(tri[k], tri[(k + 1) % 3]) ? inside : outside) = true;
                }
            }
            for (const auto& [p, q] : openRing) {
                if (!outside && segmentMeetsInterior(positionOf(tri[0]), positionOf(tri[1]),
                                                     positionOf(tri[2]), p, q)) {
                    outside = true;
                }
            }
            if (outside) {
                state[t] = 2;
            } else if (inside) {
                state[t] = 1;
                queue.push_back(t);
            }
        }
        while (!queue.empty()) {
            const std::uint32_t t = queue.back();
            queue.pop_back();
            for (std::size_t k = 0; k < 3; ++k) {
                const std::uint32_t next = cdt.neighbors[t][k];
                if ((cdt.edgeTags[t][k] & kTagRegion) == 0 && next != kNoIndex && state[next] == 0) {
                    state[next] = 1;
                    queue.push_back(next);
                }
            }
        }
    }
    const auto centroidOf = [&](std::uint32_t t) {
        const auto& tri = cdt.triangles[t];
        return katana::geometry::Triangle2{positionOf(tri[0]), positionOf(tri[1]),
                                           positionOf(tri[2])}
            .centroid();
    };

    // ---- which of them the cut keeps --------------------------------------------
    std::vector<std::uint8_t> keep(newCount, 0);
    for (std::uint32_t t = 0; t < newCount; ++t) {
        keep[t] = state[t] == 1 ? 1 : 0;
    }
    if (!request.cuts.empty() && triangulated) {
        EdgeSet cutEdges;
        for (const std::size_t c : cutConstraint) {
            if (c != kNoIndex) {
                const auto list = along(c);
                for (std::size_t i = 0; i + 1 < list.size(); ++i) {
                    cutEdges.add(list[i], list[i + 1]);
                }
            }
        }
        cutEdges.seal();
        std::vector<std::uint8_t> side(newCount, 0); // 0 not known, 1 inside the cut, 2 outside
        std::vector<std::uint32_t> queue;
        for (std::uint32_t t = 0; t < newCount; ++t) {
            if (state[t] != 1) {
                continue;
            }
            const auto& tri = cdt.triangles[t];
            bool inside = false;
            bool outside = false;
            for (std::size_t k = 0; k < 3; ++k) {
                if ((cdt.edgeTags[t][k] & kTagCut) != 0) {
                    (cutEdges.has(tri[k], tri[(k + 1) % 3]) ? inside : outside) = true;
                }
            }
            if (inside != outside) {
                side[t] = inside ? 1 : 2;
                queue.push_back(t);
            }
        }
        const auto spread = [&](std::vector<std::uint32_t>& seeds) {
            while (!seeds.empty()) {
                const std::uint32_t t = seeds.back();
                seeds.pop_back();
                for (std::size_t k = 0; k < 3; ++k) {
                    const std::uint32_t next = cdt.neighbors[t][k];
                    if ((cdt.edgeTags[t][k] & (kTagCut | kTagRegion)) == 0 && next != kNoIndex &&
                        state[next] == 1 && side[next] == 0) {
                        side[next] = side[t];
                        seeds.push_back(next);
                    }
                }
            }
        };
        spread(queue);
        // A piece no cut edge reaches is inside or outside as a whole: asked at
        // the centroid of its largest triangle, the point of it farthest from
        // any edge the piece allows.
        for (std::uint32_t seed = 0; seed < newCount; ++seed) {
            if (state[seed] != 1 || side[seed] != 0) {
                continue;
            }
            std::vector<std::uint32_t> piece{seed};
            std::vector<std::uint32_t> stack{seed};
            side[seed] = 3; // being gathered
            while (!stack.empty()) {
                const std::uint32_t t = stack.back();
                stack.pop_back();
                for (std::size_t k = 0; k < 3; ++k) {
                    const std::uint32_t next = cdt.neighbors[t][k];
                    if ((cdt.edgeTags[t][k] & (kTagCut | kTagRegion)) == 0 && next != kNoIndex &&
                        state[next] == 1 && side[next] == 0) {
                        side[next] = 3;
                        piece.push_back(next);
                        stack.push_back(next);
                    }
                }
            }
            std::uint32_t largest = piece.front();
            double largestArea = -1.0;
            for (const std::uint32_t t : piece) {
                const auto& tri = cdt.triangles[t];
                const double area = katana::geometry::Triangle2{positionOf(tri[0]),
                                                                positionOf(tri[1]),
                                                                positionOf(tri[2])}
                                        .signedArea();
                if (area > largestArea) {
                    largestArea = area;
                    largest = t;
                }
            }
            const std::uint8_t answer =
                request.insideCut && request.insideCut(centroidOf(largest)) ? 1 : 2;
            for (const std::uint32_t t : piece) {
                side[t] = answer;
            }
        }
        for (std::uint32_t t = 0; t < newCount; ++t) {
            keep[t] = state[t] == 1 && ((side[t] == 1) == request.keepInsideCut) ? 1 : 0;
        }
    }

    // ---- levels of the vertices the kernel made ------------------------------
    std::vector<double> generatedLevel(generatedCount, kNaN);
    for (std::size_t g = 0; g < generatedCount; ++g) {
        const Point2& position = cdt.generatedPoints[g];
        int bestRank = std::numeric_limits<int>::max();
        std::optional<std::uint32_t> best;
        for (const std::uint32_t c : onConstraints[g]) {
            const int rank = levelRank(infos[c], request.cutHasLevels);
            if (rank < bestRank) {
                bestRank = rank;
                best = c;
            }
        }
        if (!best || bestRank == 4) {
            if (const auto z = surface.elevationAt(position)) {
                generatedLevel[g] = *z;
            }
            continue;
        }
        const double level = levelAlong(infos[*best], position);
        if (bestRank == 2) {
            // Two new 3D lines crossing: they must agree there.
            for (const std::uint32_t c : onConstraints[g]) {
                if (levelRank(infos[c], request.cutHasLevels) == 2 &&
                    std::abs(levelAlong(infos[c], position) - level) > tol::kCoordinate) {
                    return makeError(ErrorCode::InvalidGeometry,
                                     "the breakline crosses itself at two different levels",
                                     "at " + describePosition(position) + ": " +
                                         katana::core::formatExactReal(level) + " and " +
                                         katana::core::formatExactReal(
                                             levelAlong(infos[c], position)));
                }
            }
        }
        generatedLevel[g] = level;
    }

    // ---- the result -----------------------------------------------------------
    PatchOutput output;
    output.relevelled = relevelled;
    std::vector<std::uint32_t> resultOf(inputCount + generatedCount, kNoVertex);
    for (std::size_t s = 0; s < inputCount; ++s) {
        if (vertexOfSub[s] != kNoVertex) {
            resultOf[s] = vertexOfSub[s];
        } else {
            resultOf[s] = static_cast<std::uint32_t>(vertices.size());
            vertices.emplace_back(positions[s].x, positions[s].y, subLevel[s]);
            ++output.newVertices;
        }
    }
    for (std::size_t g = 0; g < generatedCount; ++g) {
        resultOf[inputCount + g] = static_cast<std::uint32_t>(vertices.size());
        vertices.emplace_back(cdt.generatedPoints[g].x, cdt.generatedPoints[g].y,
                              generatedLevel[g]);
    }
    // A paste's seam: a surface vertex lying on the top's rim takes its level.
    if (request.cutHasLevels && triangulated) {
        for (const std::size_t c : cutConstraint) {
            if (c == kNoIndex) {
                continue;
            }
            const auto list = along(c);
            for (std::size_t i = 1; i + 1 < list.size(); ++i) {
                if (list[i] < inputCount && vertexOfSub[list[i]] != kNoVertex) {
                    const std::uint32_t v = vertexOfSub[list[i]];
                    const double level = levelAlong(infos[c], positions[list[i]]);
                    if (vertices[v].z != level) {
                        vertices[v].z = level;
                        ++output.relevelled;
                    }
                }
            }
        }
    }

    // The triangles outside the region stay as they were, those a cut drops
    // apart: not crossed by the cut, each is wholly on one side of it.
    for (std::uint32_t t = 0; t < triangles.size(); ++t) {
        if (region[t] != 0) {
            continue;
        }
        if (!request.cuts.empty()) {
            const Point2 centroid = surface.planTriangle(t).centroid();
            const bool inside = request.insideCut && request.insideCut(centroid);
            if (inside != request.keepInsideCut) {
                continue;
            }
        }
        output.triangles.push_back(triangles[t]);
        output.masks.push_back(masks[t]);
    }
    std::vector<std::uint8_t> usedByNew(vertices.size(), 0);
    EdgeSet newEdges;
    for (std::uint32_t t = 0; t < newCount; ++t) {
        if (keep[t] == 0) {
            continue;
        }
        const auto& tri = cdt.triangles[t];
        TinTriangle mapped{resultOf[tri[0]], resultOf[tri[1]], resultOf[tri[2]]};
        std::uint8_t mask = 0;
        for (std::size_t k = 0; k < 3; ++k) {
            if ((cdt.edgeTags[t][k] & (kTagKept | kTagLine | kTagCut)) != 0) {
                mask = static_cast<std::uint8_t>(mask | (1u << k));
            }
            usedByNew[mapped[k]] = 1;
            const std::uint32_t u = tri[k];
            const std::uint32_t w = tri[(k + 1) % 3];
            newEdges.add(std::min(u, w), std::max(u, w));
        }
        output.triangles.push_back(mapped);
        output.masks.push_back(mask);
    }
    newEdges.seal();
    for (std::size_t g = 0; g < generatedCount; ++g) {
        if (usedByNew[resultOf[inputCount + g]] != 0) {
            ++output.crossings;
        }
    }

    // ---- what had to land, did -------------------------------------------------
    for (std::size_t i = 0; i < request.points.size(); ++i) {
        const std::uint32_t s = pointSub[i];
        if (request.points[i].required && vertexOfSub[s] == kNoVertex &&
            usedByNew[resultOf[s]] == 0) {
            return makeError(ErrorCode::InvalidGeometry,
                             "the point could not be joined to the surface: it lies off it",
                             "at " + describePosition(request.points[i].position));
        }
    }
    for (std::size_t c = lineBegin; c < lineEnd && triangulated; ++c) {
        const auto list = along(c);
        for (std::size_t i = 0; i + 1 < list.size(); ++i) {
            if (!newEdges.has(std::min(list[i], list[i + 1]), std::max(list[i], list[i + 1]))) {
                return makeError(ErrorCode::InvalidGeometry,
                                 "the breakline leaves the surface",
                                 "near " + describePosition(positionOf(list[i])));
            }
        }
    }

    output.pointVertex.resize(request.points.size());
    for (std::size_t i = 0; i < request.points.size(); ++i) {
        output.pointVertex[i] = resultOf[pointSub[i]];
    }
    output.cutVertices.resize(request.cuts.size());
    for (std::size_t s = 0; s < request.cuts.size(); ++s) {
        if (cutConstraint[s] != kNoIndex) {
            for (const std::uint32_t x : along(cutConstraint[s])) {
                output.cutVertices[s].push_back(resultOf[x]);
            }
        }
    }
    output.vertices = std::move(vertices);
    Attempt attempt;
    attempt.output = std::move(output);
    return attempt;
}

} // namespace

std::string describePosition(const Point2& position)
{
    return "(" + katana::core::formatExactReal(position.x) + ", " +
           katana::core::formatExactReal(position.y) + ")";
}

// ---- VertexStars ----------------------------------------------------------------------

VertexStars::VertexStars(const TinSurface& surface)
{
    const auto& triangles = surface.triangles();
    start_.assign(surface.vertexCount() + 1, 0);
    for (const TinTriangle& tri : triangles) {
        for (const std::uint32_t v : tri) {
            ++start_[v + 1];
        }
    }
    for (std::size_t v = 0; v < surface.vertexCount(); ++v) {
        start_[v + 1] += start_[v];
    }
    triangles_.resize(start_.back());
    std::vector<std::uint32_t> cursor(start_.begin(), start_.end() - 1);
    for (std::uint32_t t = 0; t < triangles.size(); ++t) {
        for (const std::uint32_t v : triangles[t]) {
            triangles_[cursor[v]++] = t;
        }
    }
}

std::span<const std::uint32_t> VertexStars::of(std::uint32_t vertex) const
{
    return {triangles_.data() + start_[vertex], start_[vertex + 1] - start_[vertex]};
}

// ---- TriangleGrid -----------------------------------------------------------------------

TriangleGrid::TriangleGrid(const TinSurface& surface) : surface_(&surface)
{
    cellStart_.assign(2, 0);
    if (surface.empty()) {
        return;
    }
    const Box2& bounds = surface.bounds();
    origin_ = bounds.min;
    const double width = bounds.width();
    const double height = bounds.height();
    const double targetCells =
        std::max(1.0, static_cast<double>(surface.triangleCount()) / kGridTrianglesPerCell);
    if (width > 0.0 && height > 0.0) {
        const double cell = std::sqrt(width * height / targetCells);
        // An extremely elongated extent must not explode one dimension.
        const double limit = 2.0 * targetCells;
        columns_ = static_cast<std::uint32_t>(std::clamp(std::ceil(width / cell), 1.0, limit));
        rows_ = static_cast<std::uint32_t>(std::clamp(std::ceil(height / cell), 1.0, limit));
    }
    cellWidth_ = width > 0.0 ? width / columns_ : 1.0;
    cellHeight_ = height > 0.0 ? height / rows_ : 1.0;

    const std::size_t cellCount = static_cast<std::size_t>(columns_) * rows_;
    const auto range = [this, &surface](std::uint32_t t) {
        const auto tri = surface.planTriangle(t);
        const double minX = std::min({tri.a.x, tri.b.x, tri.c.x});
        const double maxX = std::max({tri.a.x, tri.b.x, tri.c.x});
        const double minY = std::min({tri.a.y, tri.b.y, tri.c.y});
        const double maxY = std::max({tri.a.y, tri.b.y, tri.c.y});
        return std::array<std::uint32_t, 4>{cellIndex(minX, origin_.x, cellWidth_, columns_),
                                            cellIndex(maxX, origin_.x, cellWidth_, columns_),
                                            cellIndex(minY, origin_.y, cellHeight_, rows_),
                                            cellIndex(maxY, origin_.y, cellHeight_, rows_)};
    };
    std::vector<std::uint32_t> counts(cellCount + 1, 0);
    for (std::uint32_t t = 0; t < surface.triangleCount(); ++t) {
        const auto r = range(t);
        for (std::uint32_t row = r[2]; row <= r[3]; ++row) {
            for (std::uint32_t column = r[0]; column <= r[1]; ++column) {
                ++counts[static_cast<std::size_t>(row) * columns_ + column + 1];
            }
        }
    }
    for (std::size_t cell = 0; cell < cellCount; ++cell) {
        counts[cell + 1] += counts[cell];
    }
    cellStart_ = counts;
    cellTriangles_.resize(counts[cellCount]);
    std::vector<std::uint32_t> cursor(counts.begin(), counts.end() - 1);
    for (std::uint32_t t = 0; t < surface.triangleCount(); ++t) {
        const auto r = range(t);
        for (std::uint32_t row = r[2]; row <= r[3]; ++row) {
            for (std::uint32_t column = r[0]; column <= r[1]; ++column) {
                cellTriangles_[cursor[static_cast<std::size_t>(row) * columns_ + column]++] = t;
            }
        }
    }
}

void TriangleGrid::markAlong(const Point2& a, const Point2& b,
                             std::vector<std::uint8_t>& marks) const
{
    if (surface_ == nullptr || surface_->empty()) {
        return;
    }
    const Box2& bounds = surface_->bounds();
    const double scale = std::max({std::abs(a.x), std::abs(a.y), std::abs(b.x), std::abs(b.y),
                                   std::abs(bounds.min.x), std::abs(bounds.min.y),
                                   std::abs(bounds.max.x), std::abs(bounds.max.y)});
    const double margin = roundingMargin(scale);
    const double minX = std::min(a.x, b.x) - margin;
    const double maxX = std::max(a.x, b.x) + margin;
    const double minY = std::min(a.y, b.y) - margin;
    const double maxY = std::max(a.y, b.y) + margin;
    if (maxX < bounds.min.x || minX > bounds.max.x || maxY < bounds.min.y || minY > bounds.max.y) {
        return;
    }
    const std::uint32_t column0 = cellIndex(minX, origin_.x, cellWidth_, columns_);
    const std::uint32_t column1 = cellIndex(maxX, origin_.x, cellWidth_, columns_);
    const std::uint32_t row0 = cellIndex(minY, origin_.y, cellHeight_, rows_);
    const std::uint32_t row1 = cellIndex(maxY, origin_.y, cellHeight_, rows_);
    for (std::uint32_t row = row0; row <= row1; ++row) {
        for (std::uint32_t column = column0; column <= column1; ++column) {
            const std::size_t cell = static_cast<std::size_t>(row) * columns_ + column;
            for (std::uint32_t i = cellStart_[cell]; i < cellStart_[cell + 1]; ++i) {
                const std::uint32_t t = cellTriangles_[i];
                if (marks[t] != 0) {
                    continue;
                }
                const auto tri = surface_->planTriangle(t);
                if (segmentNearTriangle(a, b, tri.a, tri.b, tri.c, margin)) {
                    marks[t] = 1;
                }
            }
        }
    }
}

// ---- placeOf ------------------------------------------------------------------------------

Place placeOf(const TinSurface& surface, const VertexStars& stars, const Point2& position)
{
    Place place;
    const auto location = surface.locate(position);
    if (!location) {
        return place;
    }
    std::vector<std::uint32_t> candidates;
    for (const std::uint32_t v : surface.triangles()[location->triangle]) {
        const auto around = stars.of(v);
        candidates.insert(candidates.end(), around.begin(), around.end());
    }
    std::sort(candidates.begin(), candidates.end());
    candidates.erase(std::unique(candidates.begin(), candidates.end()), candidates.end());

    const auto& vertices = surface.vertices();
    double nearest = std::numeric_limits<double>::infinity();
    std::uint32_t nearestVertex = kNoVertex;
    for (const std::uint32_t t : candidates) {
        for (const std::uint32_t v : surface.triangles()[t]) {
            const double distance = planOf(vertices[v]).distanceTo(position);
            if (distance <= tol::kGeometric &&
                (distance < nearest || (distance == nearest && v < nearestVertex))) {
                nearest = distance;
                nearestVertex = v;
            }
        }
    }
    if (nearestVertex != kNoVertex) {
        place.kind = Place::Kind::Vertex;
        place.vertex = nearestVertex;
        return place;
    }

    for (const std::uint32_t t : candidates) {
        const TinTriangle& tri = surface.triangles()[t];
        const Point2 corners[3] = {planOf(vertices[tri[0]]), planOf(vertices[tri[1]]),
                                   planOf(vertices[tri[2]])};
        if (orientation(corners[0], corners[1], corners[2]) <= 0) {
            continue; // a triangle flat in plan holds no position of its own
        }
        int sides[3];
        bool outside = false;
        for (int k = 0; k < 3; ++k) {
            sides[k] = orientation(corners[k], corners[(k + 1) % 3], position);
            outside = outside || sides[k] < 0;
        }
        if (outside) {
            continue;
        }
        const int zeros = (sides[0] == 0) + (sides[1] == 0) + (sides[2] == 0);
        if (zeros == 0) {
            place.kind = Place::Kind::Inside;
            place.triangle = t;
            return place;
        }
        if (zeros == 1) {
            place.kind = Place::Kind::Edge;
            place.triangle = t;
            place.edge = sides[0] == 0 ? 0u : (sides[1] == 0 ? 1u : 2u);
            return place;
        }
        // On two edge lines: exactly at a corner (which the distance test
        // above would have found; kept so the answer is never wrong).
        place.kind = Place::Kind::Vertex;
        place.vertex = tri[sides[0] == 0 && sides[1] == 0 ? 1 : (sides[1] == 0 ? 2 : 0)];
        return place;
    }
    return place;
}

void markInsertionCavity(const TinSurface& surface, const Place& place, const Point2& position,
                         std::vector<std::uint8_t>& region)
{
    if (place.kind != Place::Kind::Inside && place.kind != Place::Kind::Edge) {
        return;
    }
    std::vector<std::uint32_t> cavity{place.triangle};
    if (place.kind == Place::Kind::Edge) {
        const std::uint32_t across = surface.neighbors()[place.triangle][place.edge];
        if (across != kNoTriangle) {
            cavity.push_back(across);
        }
    }
    for (std::size_t i = 0; i < cavity.size(); ++i) {
        const std::uint32_t t = cavity[i];
        for (std::uint32_t k = 0; k < 3; ++k) {
            const std::uint32_t next = surface.neighbors()[t][k];
            if (next == kNoTriangle || surface.isEdgeConstrained(t, k) ||
                std::find(cavity.begin(), cavity.end(), next) != cavity.end()) {
                continue;
            }
            const auto corners = surface.planTriangle(next);
            if (inCircumcircle(corners.a, corners.b, corners.c, position)) {
                cavity.push_back(next);
            }
        }
    }
    for (const std::uint32_t t : cavity) {
        region[t] = 1;
    }
}

// ---- rebuildPatch -------------------------------------------------------------------------

Result<PatchOutput> rebuildPatch(const TinSurface& surface, const VertexStars& stars,
                                 PatchRequest request)
{
    const std::size_t triangleCount = surface.triangleCount();
    if (request.region.size() != triangleCount) {
        request.region.resize(triangleCount, 0);
    }
    for (const PatchPoint& point : request.points) {
        if (!point.position.isFinite() || (point.z && !std::isfinite(*point.z))) {
            return makeError(ErrorCode::InvalidArgument, "a point has a non-finite coordinate",
                             "at " + describePosition(point.position));
        }
    }
    for (const auto* segments : {&request.lines, &request.cuts}) {
        for (const PatchSegment& segment : *segments) {
            if (segment.from >= request.points.size() || segment.to >= request.points.size()) {
                return makeError(ErrorCode::Internal, "a patch segment names no point");
            }
        }
    }

    // The triangles everything named reaches: around a removed vertex, under
    // a point, along a segment.
    std::vector<std::uint8_t>& region = request.region;
    if (!request.removed.empty()) {
        for (std::uint32_t v = 0; v < request.removed.size(); ++v) {
            if (request.removed[v] != 0) {
                for (const std::uint32_t t : stars.of(v)) {
                    region[t] = 1;
                }
            }
        }
    }
    for (const PatchPoint& point : request.points) {
        const Place place = placeOf(surface, stars, point.position);
        switch (place.kind) {
        case Place::Kind::Inside:
            region[place.triangle] = 1;
            break;
        case Place::Kind::Edge: {
            region[place.triangle] = 1;
            const std::uint32_t across = surface.neighbors()[place.triangle][place.edge];
            if (across != kNoTriangle) {
                region[across] = 1;
            }
            break;
        }
        case Place::Kind::Vertex:
            for (const std::uint32_t t : stars.of(place.vertex)) {
                region[t] = 1;
            }
            break;
        case Place::Kind::Outside:
            break;
        }
    }
    if (!request.lines.empty() || !request.cuts.empty()) {
        const TriangleGrid grid(surface);
        for (const auto* segments : {&request.lines, &request.cuts}) {
            for (const PatchSegment& segment : *segments) {
                grid.markAlong(request.points[segment.from].position,
                               request.points[segment.to].position, region);
            }
        }
    }

    for (int round = 0; round <= kGrowthRounds + 1; ++round) {
        if (round == kGrowthRounds + 1) {
            std::fill(region.begin(), region.end(), std::uint8_t{1});
        }
        auto attempt = tryPatch(surface, request, region);
        if (!attempt) {
            return attempt.error();
        }
        if (attempt->output) {
            return std::move(*attempt->output);
        }
        for (const std::uint32_t t : attempt->grow) {
            region[t] = 1;
        }
    }
    return makeError(ErrorCode::TriangulationFailure,
                     "the edit could not be confined to the surface: a ring edge was split "
                     "with every triangle rebuilt");
}

// ---- extendSurface ----------------------------------------------------------------------

Result<PatchOutput> extendSurface(const TinSurface& surface, std::span<const Point3> points)
{
    const auto& triangles = surface.triangles();
    const auto& neighbors = surface.neighbors();
    const auto& masks = surface.constrainedEdges();
    const auto& sourceVertices = surface.vertices();

    struct RimEdge {
        std::uint32_t from = 0;
        std::uint32_t to = 0;
        bool constrained = false;
    };
    std::vector<RimEdge> rim;
    std::vector<std::uint8_t> onRim(surface.vertexCount(), 0);
    for (std::uint32_t t = 0; t < triangles.size(); ++t) {
        for (std::uint32_t k = 0; k < 3; ++k) {
            if (neighbors[t][k] == kNoTriangle) {
                const std::uint32_t a = triangles[t][k];
                const std::uint32_t b = triangles[t][(k + 1) % 3];
                rim.push_back({a, b, ((masks[t] >> k) & 1u) != 0});
                onRim[a] = 1;
                onRim[b] = 1;
            }
        }
    }

    std::vector<Point3> vertices = sourceVertices;
    PointMerger merger(surface.vertexCount() + points.size() + 1, tol::kGeometric);
    std::vector<std::uint32_t> subOfVertex(surface.vertexCount(), kNoIndex);
    std::vector<std::uint32_t> vertexOfSub;
    for (std::uint32_t v = 0; v < surface.vertexCount(); ++v) {
        if (onRim[v] == 0) {
            continue;
        }
        const auto entry = merger.add(planOf(sourceVertices[v]));
        if (!entry.inserted) {
            return makeError(ErrorCode::InvalidGeometry,
                             "the surface's rim has two vertices closer than the coincidence "
                             "tolerance; a point cannot be joined to it",
                             "at " + describePosition(planOf(sourceVertices[v])));
        }
        subOfVertex[v] = entry.index;
        vertexOfSub.push_back(v);
    }
    const std::size_t rimCount = vertexOfSub.size();
    std::vector<std::uint32_t> pointSub(points.size());
    std::vector<double> pointLevel;
    PatchOutput output;
    for (std::size_t i = 0; i < points.size(); ++i) {
        const Point2 position(points[i].x, points[i].y);
        const auto entry = merger.add(position);
        pointSub[i] = entry.index;
        if (entry.inserted) {
            vertexOfSub.push_back(kNoVertex);
            pointLevel.push_back(points[i].z);
            continue;
        }
        if (entry.index < rimCount) {
            Point3& vertex = vertices[vertexOfSub[entry.index]];
            if (vertex.z != points[i].z) {
                vertex.z = points[i].z;
                ++output.relevelled;
            }
            continue;
        }
        const double earlier = pointLevel[entry.index - rimCount];
        if (std::abs(earlier - points[i].z) > tol::kCoordinate) {
            return makeError(ErrorCode::InvalidGeometry,
                             "two points at one position with different levels",
                             "at " + describePosition(position) + ": " +
                                 katana::core::formatExactReal(earlier) + " and " +
                                 katana::core::formatExactReal(points[i].z));
        }
    }
    const std::vector<Point2>& positions = merger.positions();

    std::vector<CdtConstraint> constraints;
    for (const RimEdge& edge : rim) {
        constraints.push_back({subOfVertex[edge.from], subOfVertex[edge.to],
                               static_cast<std::uint8_t>(kTagRegion |
                                                         (edge.constrained ? kTagKept : 0))});
    }
    auto cdt = triangulate({positions, constraints, true});
    if (!cdt) {
        return cdt.error();
    }
    if (!cdt->generatedPoints.empty()) {
        return makeError(ErrorCode::InvalidGeometry,
                         "the surface's rim crosses itself; a point cannot be joined to it",
                         "at " + describePosition(cdt->generatedPoints.front()));
    }
    EdgeSet ring;
    for (std::size_t c = 0; c < constraints.size(); ++c) {
        const std::uint32_t begin = cdt->constraintVertexOffsets[c];
        const std::uint32_t end = cdt->constraintVertexOffsets[c + 1];
        if (end - begin != 2) {
            return makeError(ErrorCode::InvalidGeometry,
                             "the surface's rim runs through one of its own vertices; a point "
                             "cannot be joined to it",
                             "near " + describePosition(positions[constraints[c].from]));
        }
        ring.add(constraints[c].from, constraints[c].to);
    }
    ring.seal();

    const std::size_t newCount = cdt->triangles.size();
    std::vector<std::uint8_t> inside(newCount, 0);
    std::vector<std::uint32_t> queue;
    for (std::uint32_t t = 0; t < newCount; ++t) {
        const auto& tri = cdt->triangles[t];
        for (std::size_t k = 0; k < 3; ++k) {
            if ((cdt->edgeTags[t][k] & kTagRegion) != 0 && ring.has(tri[k], tri[(k + 1) % 3])) {
                inside[t] = 1;
            }
        }
        if (inside[t] != 0) {
            queue.push_back(t);
        }
    }
    while (!queue.empty()) {
        const std::uint32_t t = queue.back();
        queue.pop_back();
        for (std::size_t k = 0; k < 3; ++k) {
            const std::uint32_t next = cdt->neighbors[t][k];
            if ((cdt->edgeTags[t][k] & kTagRegion) == 0 && next != kNoIndex && inside[next] == 0) {
                inside[next] = 1;
                queue.push_back(next);
            }
        }
    }

    std::vector<std::uint32_t> resultOf(positions.size(), kNoVertex);
    for (std::size_t s = 0; s < positions.size(); ++s) {
        if (vertexOfSub[s] != kNoVertex) {
            resultOf[s] = vertexOfSub[s];
        } else {
            resultOf[s] = static_cast<std::uint32_t>(vertices.size());
            vertices.emplace_back(positions[s].x, positions[s].y, pointLevel[s - rimCount]);
            ++output.newVertices;
        }
    }
    output.triangles = triangles;
    output.masks = masks;
    std::vector<std::uint8_t> joined(positions.size(), 0);
    for (std::uint32_t t = 0; t < newCount; ++t) {
        const auto& tri = cdt->triangles[t];
        if (inside[t] != 0 ||
            (tri[0] < rimCount && tri[1] < rimCount && tri[2] < rimCount)) {
            continue;
        }
        std::uint8_t mask = 0;
        for (std::size_t k = 0; k < 3; ++k) {
            if ((cdt->edgeTags[t][k] & kTagKept) != 0) {
                mask = static_cast<std::uint8_t>(mask | (1u << k));
            }
            joined[tri[k]] = 1;
        }
        output.triangles.push_back({resultOf[tri[0]], resultOf[tri[1]], resultOf[tri[2]]});
        output.masks.push_back(mask);
    }
    for (std::size_t i = 0; i < points.size(); ++i) {
        if (pointSub[i] >= rimCount && joined[pointSub[i]] == 0) {
            return makeError(ErrorCode::InvalidGeometry,
                             "the point could not be joined to the surface: it lies in a crack "
                             "of the rim",
                             "at " + describePosition(Point2(points[i].x, points[i].y)));
        }
    }
    output.pointVertex.resize(points.size());
    for (std::size_t i = 0; i < points.size(); ++i) {
        output.pointVertex[i] = resultOf[pointSub[i]];
    }
    output.vertices = std::move(vertices);
    return output;
}

// ---- finishSurface ------------------------------------------------------------------------

Result<TinSurface> finishSurface(std::vector<Point3> vertices, std::vector<TinTriangle> triangles,
                                 std::vector<std::uint8_t> masks)
{
    if (triangles.empty()) {
        return makeError(ErrorCode::InvalidState, "nothing of the surface would be left");
    }
    // A constrained edge is constrained on both its sides.
    struct HalfEdge {
        std::uint32_t low = 0;
        std::uint32_t high = 0;
        std::uint32_t triangle = 0;
        std::uint8_t edge = 0;
    };
    std::vector<HalfEdge> halfEdges;
    halfEdges.reserve(3 * triangles.size());
    for (std::uint32_t t = 0; t < triangles.size(); ++t) {
        for (std::uint8_t k = 0; k < 3; ++k) {
            const std::uint32_t a = triangles[t][k];
            const std::uint32_t b = triangles[t][(k + 1) % 3];
            halfEdges.push_back({std::min(a, b), std::max(a, b), t, k});
        }
    }
    std::sort(halfEdges.begin(), halfEdges.end(), [](const HalfEdge& x, const HalfEdge& y) {
        return x.low != y.low ? x.low < y.low
                              : (x.high != y.high ? x.high < y.high : x.triangle < y.triangle);
    });
    for (std::size_t i = 0; i + 1 < halfEdges.size(); ++i) {
        const HalfEdge& x = halfEdges[i];
        const HalfEdge& y = halfEdges[i + 1];
        if (x.low != y.low || x.high != y.high) {
            continue;
        }
        const bool flagged = ((masks[x.triangle] >> x.edge) & 1u) != 0 ||
                             ((masks[y.triangle] >> y.edge) & 1u) != 0;
        if (flagged) {
            masks[x.triangle] = static_cast<std::uint8_t>(masks[x.triangle] | (1u << x.edge));
            masks[y.triangle] = static_cast<std::uint8_t>(masks[y.triangle] | (1u << y.edge));
        }
    }

    // Vertices no triangle uses are dropped, keeping the order of the rest.
    std::vector<std::uint32_t> renumber(vertices.size(), kNoVertex);
    for (const TinTriangle& tri : triangles) {
        for (const std::uint32_t v : tri) {
            renumber[v] = 0;
        }
    }
    std::uint32_t kept = 0;
    for (std::size_t v = 0; v < vertices.size(); ++v) {
        if (renumber[v] == kNoVertex) {
            continue;
        }
        if (!vertices[v].isFinite()) {
            return makeError(ErrorCode::TriangulationFailure,
                             "a vertex of the edited surface has no level: it lies off the "
                             "surface",
                             "at " + describePosition(planOf(vertices[v])));
        }
        renumber[v] = kept;
        vertices[kept++] = vertices[v]; // monotone: never overwrites what is still to be read
    }
    vertices.resize(kept);

    // Canonical: each triangle starts at its smallest vertex, the triangles sorted.
    struct Entry {
        TinTriangle triangle;
        std::uint8_t mask = 0;
    };
    std::vector<Entry> entries(triangles.size());
    for (std::size_t t = 0; t < triangles.size(); ++t) {
        const TinTriangle mapped{renumber[triangles[t][0]], renumber[triangles[t][1]],
                                 renumber[triangles[t][2]]};
        const std::size_t smallest = static_cast<std::size_t>(
            std::min_element(mapped.begin(), mapped.end()) - mapped.begin());
        Entry entry;
        for (std::size_t j = 0; j < 3; ++j) {
            entry.triangle[j] = mapped[(j + smallest) % 3];
            if (((masks[t] >> ((j + smallest) % 3)) & 1u) != 0) {
                entry.mask = static_cast<std::uint8_t>(entry.mask | (1u << j));
            }
        }
        entries[t] = entry;
    }
    std::sort(entries.begin(), entries.end(),
              [](const Entry& x, const Entry& y) { return x.triangle < y.triangle; });
    triangles.resize(entries.size());
    masks.resize(entries.size());
    for (std::size_t t = 0; t < entries.size(); ++t) {
        triangles[t] = entries[t].triangle;
        masks[t] = entries[t].mask;
    }
    return TinSurface::create(std::move(vertices), std::move(triangles), std::move(masks));
}

TinEditReport reportOf(const TinSurface& before, const TinSurface& after)
{
    TinEditReport report;
    report.trianglesBefore = before.triangleCount();
    report.trianglesAfter = after.triangleCount();
    report.verticesBefore = before.vertexCount();
    report.verticesAfter = after.vertexCount();
    return report;
}

} // namespace katana::terrain::detail
