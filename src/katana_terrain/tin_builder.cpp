#include "katana/terrain/tin_builder.hpp"

#include <algorithm>
#include <cmath>
#include <limits>
#include <optional>
#include <sstream>
#include <string>
#include <utility>

#include "cdt_backend.hpp"
#include "katana/geometry/polygon.hpp"
#include "point_merge.hpp"
#include "tin_surface_factory.hpp"

namespace katana::terrain {

namespace tol = katana::math::tolerance;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;
using katana::geometry::Containment;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;
using katana::geometry::Triangle2;

namespace {

// Constraint tags handed to the triangulation and reported back per edge.
constexpr std::uint8_t kBreaklineTag = 1;
constexpr std::uint8_t kBoundaryTag = 2;
constexpr std::uint8_t kHoleTag = 4;
constexpr std::uint8_t kRingTags = kBoundaryTag | kHoleTag;

// Lets the triangulation's adjacency array become the surface's without a pass.
static_assert(detail::kNoIndex == kNoTriangle && detail::kNoIndex == kNoVertex);

std::string describe(const Point2& p)
{
    std::ostringstream text;
    text.precision(17);
    text << "(" << p.x << ", " << p.y << ")";
    return text.str();
}

std::string describe(double value)
{
    std::ostringstream text;
    text.precision(17);
    text << value;
    return text.str();
}

// A boundary or hole after its vertices were merged into the vertex table.
struct Ring {
    std::string name;                 // "boundary" or "hole <i>", for messages
    std::uint8_t tag = kBoundaryTag;
    std::vector<std::uint32_t> ids;   // vertex ids, no consecutive repeats
    Polyline2 polygon;                // the positions of `ids`, closed
    std::size_t firstConstraint = 0;  // its edges in the constraint list
};

// All vertices known before triangulation: positions merged within kGeometric,
// one elevation each (NaN while a ring vertex still waits for its elevation).
class VertexTable {
  public:
    VertexTable(std::size_t capacity, const TinBuildOptions& options, TinBuildReport& report)
        : merger_(capacity, tol::kGeometric), options_(options), report_(report)
    {
        elevations_.reserve(capacity);
    }

    // Adds a survey point or breakline vertex. `what` names it for messages.
    [[nodiscard]] Result<std::uint32_t> addElevated(const Point3& p, bool isBreaklineVertex,
                                                    const std::string& what)
    {
        const auto entry = merger_.add(Point2(p.x, p.y));
        if (entry.inserted) {
            elevations_.push_back(p.z);
            if (options_.duplicatePoints == DuplicatePointPolicy::Average) {
                sums_.push_back(p.z);
                counts_.push_back(1);
            }
            return entry.index;
        }

        ++(isBreaklineVertex ? report_.sharedBreaklineVertexCount : report_.duplicatePointCount);
        const double kept = elevations_[entry.index];
        if (std::abs(p.z - kept) > options_.elevationTolerance) {
            if (options_.duplicatePoints == DuplicatePointPolicy::ErrorOnConflict) {
                return makeError(ErrorCode::InvalidGeometry,
                                 "two elevations at one plan position (DuplicatePointPolicy::"
                                 "ErrorOnConflict)",
                                 what + " at " + describe(Point2(p.x, p.y)) + " has z=" +
                                     describe(p.z) + ", the earlier vertex has z=" +
                                     describe(kept));
            }
            ++report_.elevationConflictCount;
        }
        if (options_.duplicatePoints == DuplicatePointPolicy::Average) {
            sums_[entry.index] += p.z;
            ++counts_[entry.index];
        }
        return entry.index;
    }

    // Adds a boundary/hole vertex, which has no elevation of its own.
    [[nodiscard]] std::uint32_t addPlanOnly(const Point2& p)
    {
        const auto entry = merger_.add(p);
        if (entry.inserted) {
            elevations_.push_back(std::numeric_limits<double>::quiet_NaN());
        }
        return entry.index;
    }

    // Call once after the last addElevated(): settles the Average policy.
    void finishElevated()
    {
        for (std::size_t i = 0; i < sums_.size(); ++i) {
            if (counts_[i] > 1) {
                elevations_[i] = sums_[i] / static_cast<double>(counts_[i]);
            }
        }
    }

    [[nodiscard]] std::size_t size() const { return elevations_.size(); }
    [[nodiscard]] const std::vector<Point2>& positions() const { return merger_.positions(); }
    [[nodiscard]] std::vector<double>& elevations() { return elevations_; }

  private:
    detail::PointMerger merger_;
    const TinBuildOptions& options_;
    TinBuildReport& report_;
    std::vector<double> elevations_;
    std::vector<double> sums_; // Average policy only
    std::vector<std::uint32_t> counts_;
};

// Elevations of the vertices the triangulation created on breaklines.
//
// The vertices along each breakline segment are known from the triangulation. A
// created vertex takes the elevation the breakline has at its position (linear
// between the segment's end vertices); where several breaklines meet in it their
// mean, subject to the crossing policy. Created vertices that lie on no breakline
// (rings crossing each other) are returned in `unresolved`.
[[nodiscard]] Status resolveCrossingElevations(const detail::CdtOutput& cdt,
                                               std::span<const detail::CdtConstraint> constraints,
                                               std::span<const Point2> positions,
                                               const TinBuildOptions& options,
                                               std::vector<double>& elevations,
                                               TinBuildReport& report,
                                               std::vector<std::uint32_t>& unresolved)
{
    const std::size_t inputCount = positions.size();
    const std::size_t createdCount = cdt.generatedPoints.size();
    if (createdCount == 0) {
        return {};
    }
    struct Accumulator {
        double sum = 0.0;
        double lowest = std::numeric_limits<double>::infinity();
        double highest = -std::numeric_limits<double>::infinity();
        std::uint32_t count = 0;
    };
    std::vector<Accumulator> created(createdCount);

    for (std::size_t c = 0; c < constraints.size(); ++c) {
        if (constraints[c].tag != kBreaklineTag) {
            continue;
        }
        const std::uint32_t begin = cdt.constraintVertexOffsets[c];
        const std::uint32_t end = cdt.constraintVertexOffsets[c + 1];
        const Segment2 segment{positions[constraints[c].from], positions[constraints[c].to]};
        const double zFrom = elevations[constraints[c].from];
        const double zTo = elevations[constraints[c].to];
        for (std::uint32_t i = begin; i < end; ++i) {
            const std::uint32_t vertex = cdt.constraintVertices[i];
            if (vertex < inputCount) {
                continue; // a survey point on the breakline keeps its own elevation
            }
            const Point2& position = cdt.generatedPoints[vertex - inputCount];
            const double z = katana::math::lerp(zFrom, zTo, segment.parameterOf(position));
            Accumulator& accumulator = created[vertex - inputCount];
            accumulator.sum += z;
            accumulator.lowest = std::min(accumulator.lowest, z);
            accumulator.highest = std::max(accumulator.highest, z);
            ++accumulator.count;
        }
    }

    elevations.resize(inputCount + createdCount, std::numeric_limits<double>::quiet_NaN());
    for (std::size_t i = 0; i < createdCount; ++i) {
        const Accumulator& accumulator = created[i];
        if (accumulator.count == 0) {
            unresolved.push_back(static_cast<std::uint32_t>(inputCount + i));
            continue;
        }
        if (accumulator.count > 1) {
            ++report.breaklineCrossingCount;
            if (accumulator.highest - accumulator.lowest > options.elevationTolerance) {
                if (options.crossingBreaklines == CrossingBreaklinePolicy::ErrorOnConflict) {
                    return makeError(ErrorCode::InvalidGeometry,
                                     "breaklines cross at different elevations "
                                     "(CrossingBreaklinePolicy::ErrorOnConflict)",
                                     "crossing at " + describe(cdt.generatedPoints[i]) +
                                         ", elevations from " + describe(accumulator.lowest) +
                                         " to " + describe(accumulator.highest));
                }
                ++report.crossingConflictCount;
            }
        }
        elevations[inputCount + i] = accumulator.sum / static_cast<double>(accumulator.count);
    }
    return {};
}

[[nodiscard]] std::vector<Point3> makeVertices(std::span<const Point2> positions,
                                               std::span<const Point2> created,
                                               std::span<const double> elevations)
{
    std::vector<Point3> vertices;
    vertices.reserve(positions.size() + created.size());
    for (std::size_t i = 0; i < positions.size(); ++i) {
        vertices.emplace_back(positions[i].x, positions[i].y, elevations[i]);
    }
    for (std::size_t i = 0; i < created.size(); ++i) {
        vertices.emplace_back(created[i].x, created[i].y, elevations[positions.size() + i]);
    }
    return vertices;
}

[[nodiscard]] std::vector<std::uint8_t>
constraintMasks(const std::vector<std::array<std::uint8_t, 3>>& edgeTags)
{
    std::vector<std::uint8_t> masks(edgeTags.size(), std::uint8_t{0});
    for (std::size_t t = 0; t < edgeTags.size(); ++t) {
        for (std::size_t k = 0; k < 3; ++k) {
            if (edgeTags[t][k] != 0) {
                masks[t] = static_cast<std::uint8_t>(masks[t] | (1u << k));
            }
        }
    }
    return masks;
}

// A ring is simple exactly when the closed walk along its edges - including the
// vertices that split them - visits no vertex twice: crossings, touching
// vertices and overlapping edges all show up as a repeated vertex. Exact, and
// linear in the size of the ring (geometry::isSimple is quadratic).
[[nodiscard]] Status requireSimpleRing(const Ring& ring, const detail::CdtOutput& cdt,
                                       std::span<const Point2> positions,
                                       std::vector<std::uint8_t>& visited)
{
    std::vector<std::uint32_t> walk;
    for (std::size_t edge = 0; edge < ring.ids.size(); ++edge) {
        const std::size_t c = ring.firstConstraint + edge;
        const std::uint32_t begin = cdt.constraintVertexOffsets[c];
        const std::uint32_t end = cdt.constraintVertexOffsets[c + 1];
        for (std::uint32_t i = begin; i + 1 < end; ++i) { // the last vertex opens the next edge
            walk.push_back(cdt.constraintVertices[i]);
        }
    }
    std::optional<std::uint32_t> repeated;
    for (const std::uint32_t vertex : walk) {
        if (visited[vertex] != 0 && !repeated) {
            repeated = vertex;
        }
        visited[vertex] = 1;
    }
    for (const std::uint32_t vertex : walk) {
        visited[vertex] = 0;
    }
    if (repeated) {
        const Point2& where = *repeated < positions.size()
                                  ? positions[*repeated]
                                  : cdt.generatedPoints[*repeated - positions.size()];
        return makeError(ErrorCode::InvalidGeometry, ring.name + " touches or crosses itself",
                         "at " + describe(where));
    }
    return {};
}

// Marks the triangles to keep. Ring edges partition the triangulation into
// regions; a region is inside or outside as a whole, so one point per region is
// classified against the polygons: the centroid of the region's largest
// triangle, which is as far from every ring edge as the region allows.
[[nodiscard]] std::vector<std::uint8_t> selectTriangles(const detail::CdtOutput& cdt,
                                                        std::span<const Point3> vertices,
                                                        std::span<const Ring> rings)
{
    const std::size_t triangleCount = cdt.triangles.size();
    std::vector<std::uint8_t> keep(triangleCount, std::uint8_t{0});
    std::vector<std::uint8_t> assigned(triangleCount, std::uint8_t{0});
    std::vector<std::uint32_t> region;
    std::vector<std::uint32_t> stack;

    const auto planTriangle = [&](std::uint32_t t) {
        const auto& tri = cdt.triangles[t];
        return Triangle2{Point2(vertices[tri[0]].x, vertices[tri[0]].y),
                         Point2(vertices[tri[1]].x, vertices[tri[1]].y),
                         Point2(vertices[tri[2]].x, vertices[tri[2]].y)};
    };

    for (std::uint32_t seed = 0; seed < triangleCount; ++seed) {
        if (assigned[seed] != 0) {
            continue;
        }
        region.clear();
        stack.assign(1, seed);
        assigned[seed] = 1;
        std::uint32_t largest = seed;
        double largestArea = -1.0;
        while (!stack.empty()) {
            const std::uint32_t t = stack.back();
            stack.pop_back();
            region.push_back(t);
            const double area = planTriangle(t).signedArea();
            if (area > largestArea || (area == largestArea && t < largest)) {
                largestArea = area;
                largest = t;
            }
            for (std::size_t k = 0; k < 3; ++k) {
                const std::uint32_t next = cdt.neighbors[t][k];
                if (next != detail::kNoIndex && (cdt.edgeTags[t][k] & kRingTags) == 0 &&
                    assigned[next] == 0) {
                    assigned[next] = 1;
                    stack.push_back(next);
                }
            }
        }

        const Point2 probe = planTriangle(largest).centroid();
        bool inside = true;
        for (const Ring& ring : rings) {
            const Containment where = ring.polygon.classify(probe);
            // OnBoundary means the whole region is thinner than kGeometric.
            inside = ring.tag == kBoundaryTag ? where == Containment::Inside
                                              : where == Containment::Outside;
            if (!inside) {
                break;
            }
        }
        if (inside) {
            for (const std::uint32_t t : region) {
                keep[t] = 1;
            }
        }
    }
    return keep;
}

} // namespace

Result<TinBuildResult> buildTin(const TinInput& input, const TinBuildOptions& options)
{
    if (!(options.elevationTolerance >= 0.0) || !std::isfinite(options.elevationTolerance)) {
        return makeError(ErrorCode::InvalidArgument,
                         "elevationTolerance must be finite and not negative",
                         describe(options.elevationTolerance));
    }

    // ---- validate coordinates ----------------------------------------------------
    std::size_t capacity = input.points.size() + input.boundary.vertices.size();
    for (std::size_t i = 0; i < input.points.size(); ++i) {
        if (!input.points[i].isFinite()) {
            return makeError(ErrorCode::InvalidArgument, "point has a non-finite coordinate",
                             "point=" + std::to_string(i));
        }
    }
    for (std::size_t b = 0; b < input.breaklines.size(); ++b) {
        const Breakline& breakline = input.breaklines[b];
        if (breakline.vertices.size() < 2) {
            return makeError(ErrorCode::InvalidGeometry, "breakline has fewer than 2 vertices",
                             "breakline=" + std::to_string(b));
        }
        for (const Point3& vertex : breakline.vertices) {
            if (!vertex.isFinite()) {
                return makeError(ErrorCode::InvalidArgument,
                                 "breakline vertex has a non-finite coordinate",
                                 "breakline=" + std::to_string(b));
            }
        }
        capacity += breakline.vertices.size();
    }
    struct RingInput {
        const Polyline2* polygon = nullptr;
        std::string name;
        std::uint8_t tag = kBoundaryTag;
    };
    std::vector<RingInput> ringInputs;
    if (!input.boundary.vertices.empty()) {
        ringInputs.push_back({&input.boundary, "boundary", kBoundaryTag});
    }
    for (std::size_t h = 0; h < input.holes.size(); ++h) {
        ringInputs.push_back({&input.holes[h], "hole " + std::to_string(h), kHoleTag});
        capacity += input.holes[h].vertices.size();
    }
    for (const RingInput& ringInput : ringInputs) {
        for (const Point2& vertex : ringInput.polygon->vertices) {
            if (!vertex.isFinite()) {
                return makeError(ErrorCode::InvalidArgument,
                                 ringInput.name + " vertex has a non-finite coordinate");
            }
        }
    }
    if (capacity >= kNoVertex - 1) {
        return makeError(ErrorCode::InvalidArgument, "too many vertices for 32-bit indices",
                         "vertices=" + std::to_string(capacity));
    }

    // ---- vertex table: points, then breakline vertices, then ring vertices -------
    TinBuildResult result;
    TinBuildReport& report = result.report;
    VertexTable table(capacity, options, report);

    std::vector<std::uint32_t> pointIds(input.points.size());
    for (std::size_t i = 0; i < input.points.size(); ++i) {
        auto id = table.addElevated(input.points[i], false, "point " + std::to_string(i));
        if (!id) {
            return id.error();
        }
        pointIds[i] = *id;
    }

    std::vector<detail::CdtConstraint> constraints;
    for (std::size_t b = 0; b < input.breaklines.size(); ++b) {
        const Breakline& breakline = input.breaklines[b];
        std::vector<std::uint32_t> ids;
        ids.reserve(breakline.vertices.size());
        for (std::size_t v = 0; v < breakline.vertices.size(); ++v) {
            auto id = table.addElevated(breakline.vertices[v], true,
                                        "breakline " + std::to_string(b) + " vertex " +
                                            std::to_string(v));
            if (!id) {
                return id.error();
            }
            ids.push_back(*id);
        }
        const std::size_t before = constraints.size();
        const std::size_t segments = breakline.closed ? ids.size() : ids.size() - 1;
        for (std::size_t s = 0; s < segments; ++s) {
            const std::uint32_t from = ids[s];
            const std::uint32_t to = ids[(s + 1) % ids.size()];
            if (from != to) {
                constraints.push_back({from, to, kBreaklineTag});
            }
        }
        if (constraints.size() == before) {
            return makeError(ErrorCode::InvalidGeometry,
                             "breakline has no length (all vertices coincide)",
                             "breakline=" + std::to_string(b));
        }
    }
    table.finishElevated();
    const std::size_t dataVertexCount = table.size();
    const std::size_t breaklineConstraintCount = constraints.size();

    std::vector<Ring> rings;
    for (const RingInput& ringInput : ringInputs) {
        Ring ring;
        ring.name = ringInput.name;
        ring.tag = ringInput.tag;
        for (const Point2& vertex : ringInput.polygon->vertices) {
            const std::uint32_t id = table.addPlanOnly(vertex);
            if (ring.ids.empty() || ring.ids.back() != id) {
                ring.ids.push_back(id);
            }
        }
        if (ring.ids.size() > 1 && ring.ids.front() == ring.ids.back()) {
            ring.ids.pop_back(); // the first vertex repeated to close the ring
        }
        ring.polygon.closed = true;
        for (const std::uint32_t id : ring.ids) {
            ring.polygon.vertices.push_back(table.positions()[id]);
        }
        if (katana::geometry::orientation(ring.polygon) ==
            katana::geometry::Orientation::Degenerate) {
            return makeError(ErrorCode::InvalidGeometry,
                             ring.name + " must be a polygon with at least 3 distinct vertices "
                                         "and a non-zero area",
                             "distinct vertices=" + std::to_string(ring.ids.size()));
        }
        ring.firstConstraint = constraints.size();
        for (std::size_t e = 0; e < ring.ids.size(); ++e) {
            constraints.push_back({ring.ids[e], ring.ids[(e + 1) % ring.ids.size()], ring.tag});
        }
        rings.push_back(std::move(ring));
    }

    const std::vector<Point2>& positions = table.positions();
    std::vector<double>& elevations = table.elevations();

    // ---- reference surface ---------------------------------------------------------
    // Points and breaklines only, not clipped. It supplies the elevation of ring
    // vertices that are not data vertices, and of vertices where rings cross each
    // other. Built only when such a vertex exists.
    std::optional<TinSurface> reference;
    const auto referenceElevation = [&](const Point2& position,
                                        const std::string& what) -> Result<double> {
        if (!reference) {
            const std::span<const Point2> dataPositions(positions.data(), dataVertexCount);
            const std::span<const detail::CdtConstraint> breaklineConstraints(
                constraints.data(), breaklineConstraintCount);
            auto cdt = detail::triangulate({dataPositions, breaklineConstraints, true});
            if (!cdt) {
                return cdt.error();
            }
            std::vector<double> dataElevations(elevations.begin(),
                                               elevations.begin() +
                                                   static_cast<std::ptrdiff_t>(dataVertexCount));
            TinBuildReport scratch; // crossings are reported by the final pass
            std::vector<std::uint32_t> unresolved;
            if (const Status status =
                    resolveCrossingElevations(*cdt, breaklineConstraints, dataPositions, options,
                                              dataElevations, scratch, unresolved);
                !status) {
                return status.error();
            }
            auto surface = detail::TinSurfaceFactory::assemble(
                makeVertices(dataPositions, cdt->generatedPoints, dataElevations),
                std::move(cdt->triangles), std::move(cdt->neighbors),
                constraintMasks(cdt->edgeTags));
            if (!surface) {
                return surface.error();
            }
            reference = std::move(*surface);
        }
        const auto z = reference->elevationAt(position);
        if (!z) {
            return makeError(ErrorCode::InvalidGeometry,
                             what + " lies off the triangulated data and is not a data vertex; "
                                    "it cannot be given an elevation",
                             "at " + describe(position));
        }
        return *z;
    };

    for (std::size_t id = dataVertexCount; id < table.size(); ++id) {
        auto z = referenceElevation(positions[id], "boundary or hole vertex");
        if (!z) {
            return z.error();
        }
        elevations[id] = *z;
    }

    // ---- triangulation -------------------------------------------------------------
    auto cdt = detail::triangulate({positions, constraints, true});
    if (!cdt) {
        return cdt.error();
    }

    std::vector<std::uint32_t> unresolved;
    if (const Status status = resolveCrossingElevations(*cdt, constraints, positions, options,
                                                        elevations, report, unresolved);
        !status) {
        return status.error();
    }
    for (const std::uint32_t vertex : unresolved) {
        auto z = referenceElevation(cdt->generatedPoints[vertex - positions.size()],
                                    "crossing of boundary/hole edges");
        if (!z) {
            return z.error();
        }
        elevations[vertex] = *z;
    }

    std::vector<std::uint8_t> visited(positions.size() + cdt->generatedPoints.size(),
                                      std::uint8_t{0});
    for (const Ring& ring : rings) {
        if (const Status status = requireSimpleRing(ring, *cdt, positions, visited); !status) {
            return status.error();
        }
    }

    std::vector<Point3> vertices = makeVertices(positions, cdt->generatedPoints, elevations);
    std::vector<std::uint8_t> masks = constraintMasks(cdt->edgeTags);

    // ---- clip and compact ----------------------------------------------------------
    if (!rings.empty()) {
        const std::vector<std::uint8_t> keep = selectTriangles(*cdt, vertices, rings);

        std::vector<std::uint32_t> newTriangle(cdt->triangles.size(), kNoTriangle);
        std::vector<std::uint32_t> newVertex(vertices.size(), kNoVertex);
        std::uint32_t keptTriangles = 0;
        for (std::size_t t = 0; t < keep.size(); ++t) {
            if (keep[t] != 0) {
                newTriangle[t] = keptTriangles++;
                for (const std::uint32_t v : cdt->triangles[t]) {
                    newVertex[v] = 0; // mark as used
                }
            }
        }
        if (keptTriangles == 0) {
            return makeError(ErrorCode::TriangulationFailure,
                             "no triangle is left inside the boundary and outside the holes");
        }
        std::uint32_t keptVertices = 0;
        for (std::size_t v = 0; v < vertices.size(); ++v) {
            if (newVertex[v] != kNoVertex) {
                newVertex[v] = keptVertices;
                vertices[keptVertices++] = vertices[v]; // monotone: never overwrites unread data
            }
        }
        report.clippedTriangleCount = keep.size() - keptTriangles;
        report.droppedVertexCount = vertices.size() - keptVertices;
        vertices.resize(keptVertices);

        // A monotone renumbering keeps every triangle's rotation and the
        // lexicographic order, so the compacted arrays are still canonical.
        for (std::size_t t = 0; t < keep.size(); ++t) {
            if (keep[t] == 0) {
                continue;
            }
            const std::uint32_t target = newTriangle[t];
            for (std::size_t k = 0; k < 3; ++k) {
                const std::uint32_t neighbor = cdt->neighbors[t][k];
                cdt->triangles[target][k] = newVertex[cdt->triangles[t][k]];
                cdt->neighbors[target][k] =
                    neighbor == detail::kNoIndex ? kNoTriangle : newTriangle[neighbor];
            }
            masks[target] = masks[t];
        }
        cdt->triangles.resize(keptTriangles);
        cdt->neighbors.resize(keptTriangles);
        masks.resize(keptTriangles);

        report.pointVertex.resize(pointIds.size());
        for (std::size_t i = 0; i < pointIds.size(); ++i) {
            report.pointVertex[i] = newVertex[pointIds[i]];
        }
    } else {
        report.pointVertex = std::move(pointIds);
    }

    auto surface = detail::TinSurfaceFactory::assemble(std::move(vertices),
                                                       std::move(cdt->triangles),
                                                       std::move(cdt->neighbors), std::move(masks));
    if (!surface) {
        return surface.error();
    }
    result.surface = std::move(*surface);
    return result;
}

} // namespace katana::terrain
