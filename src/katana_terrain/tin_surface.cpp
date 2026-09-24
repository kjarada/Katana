#include "katana/terrain/tin_surface.hpp"

#include <algorithm>
#include <cmath>
#include <string>
#include <utility>

#include "katana/core/task_pool.hpp"

#include "simd_dispatch.hpp"
#include "summation.hpp"

namespace katana::terrain {

namespace tol = katana::math::tolerance;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;
using katana::geometry::Segment2;
using katana::geometry::Triangle2;
using katana::geometry::Triangle3;
using katana::geometry::Vec2;

namespace {

// Reach of the locator grid. A position within kGeometric of a triangle must
// find that triangle in its cell, and the mapping from a coordinate to a cell
// is itself rounded (by less than 1e-8 m for coordinates up to 1e8 m). Listing
// every triangle in all cells within twice kGeometric of it covers both; this is
// kGeometric applied with a safety factor, not a new tolerance.
constexpr double kLocatorMargin = 2.0 * tol::kGeometric;

// Average number of triangles per grid cell the grid is sized for. Fewer means
// faster queries and more memory (each triangle is listed in every cell it
// reaches into); 2 gives ~4 list entries per triangle and ~8 candidates per query.
constexpr double kTrianglesPerCell = 2.0;

// Fewest candidates in a cell for which locate() hands the containment test to
// the kernel. MEASURED: see docs/terrain.md, "SIMD".
constexpr std::size_t kEnclosingKernelMinimum = 4;

// Monotonic in `value`, which is what makes the grid exact: a position inside a
// triangle's bounding box always maps into the cell range of that box.
std::uint32_t cellCoordinate(double value, double origin, double cellSize, std::uint32_t count)
{
    const double cell = std::floor((value - origin) / cellSize);
    if (!(cell > 0.0)) {
        return 0;
    }
    return cell >= static_cast<double>(count) ? count - 1 : static_cast<std::uint32_t>(cell);
}

// True when the cell rectangle grown by kLocatorMargin lies entirely outside one
// edge line of the counter-clockwise triangle a, b, c: the triangle cannot reach
// the cell. Conservative (never true for a cell the triangle touches). Long thin
// triangles along a diagonal hull would otherwise be listed in every cell of
// their bounding box.
bool cellIsSeparated(const Point2& a, const Point2& b, const Point2& c, const Point2& cellMin,
                     const Point2& cellMax)
{
    const Point2 corners[3] = {a, b, c};
    for (int k = 0; k < 3; ++k) {
        const Point2& p = corners[k];
        const Vec2 d = corners[(k + 1) % 3] - p;
        // Rectangle corner furthest to the left (inner side) of the edge.
        const Point2 inner(d.y >= 0.0 ? cellMin.x : cellMax.x, d.x >= 0.0 ? cellMax.y : cellMin.y);
        if (d.cross(inner - p) < -kLocatorMargin * (std::abs(d.x) + std::abs(d.y))) {
            return true;
        }
    }
    return false;
}

// Calls visit(cellIndex) for every grid cell the triangle may reach into.
template <typename Visit>
void visitCells(const detail::LocatorGrid& grid, const Point2& a, const Point2& b, const Point2& c,
                Visit&& visit)
{
    const double minX = std::min({a.x, b.x, c.x}) - kLocatorMargin;
    const double maxX = std::max({a.x, b.x, c.x}) + kLocatorMargin;
    const double minY = std::min({a.y, b.y, c.y}) - kLocatorMargin;
    const double maxY = std::max({a.y, b.y, c.y}) + kLocatorMargin;
    const std::uint32_t column0 = cellCoordinate(minX, grid.origin.x, grid.cellWidth, grid.columns);
    const std::uint32_t column1 = cellCoordinate(maxX, grid.origin.x, grid.cellWidth, grid.columns);
    const std::uint32_t row0 = cellCoordinate(minY, grid.origin.y, grid.cellHeight, grid.rows);
    const std::uint32_t row1 = cellCoordinate(maxY, grid.origin.y, grid.cellHeight, grid.rows);

    // The separation test needs a reliable orientation; slivers keep the box.
    const bool prune = (column1 > column0 || row1 > row0) && (b - a).cross(c - a) > 0.0;
    for (std::uint32_t row = row0; row <= row1; ++row) {
        for (std::uint32_t column = column0; column <= column1; ++column) {
            if (prune) {
                const Point2 cellMin(grid.origin.x + column * grid.cellWidth,
                                     grid.origin.y + row * grid.cellHeight);
                const Point2 cellMax(cellMin.x + grid.cellWidth, cellMin.y + grid.cellHeight);
                if (cellIsSeparated(a, b, c, cellMin, cellMax)) {
                    continue;
                }
            }
            visit(static_cast<std::size_t>(row) * grid.columns + column);
        }
    }
}

// Height over the longest edge <= kGeometric, the criterion of
// Triangle2::isDegenerate(), from the doubled signed area and without roots.
// Barycentric weights of such a triangle are dominated by rounding.
bool isSliver(const Point2& a, const Point2& b, const Point2& c, double twiceArea)
{
    const double longestSquared =
        std::max({(b - a).lengthSquared(), (c - b).lengthSquared(), (a - c).lengthSquared()});
    return !(twiceArea > 0.0) ||
           twiceArea * twiceArea <= tol::kGeometric * tol::kGeometric * longestSquared;
}

struct HalfEdge {
    std::uint32_t low = 0;  // smaller vertex index of the edge
    std::uint32_t high = 0; // larger vertex index
    std::uint32_t triangle = 0;
    std::uint8_t edge = 0;
    bool ascending = false; // the triangle runs the edge from low to high
};

} // namespace

// ---- construction ---------------------------------------------------------------

Result<TinSurface> TinSurface::create(std::vector<Point3> vertices,
                                      std::vector<TinTriangle> triangles,
                                      std::vector<std::uint8_t> constrainedEdges)
{
    if (vertices.size() >= kNoVertex || triangles.size() >= kNoTriangle) {
        return makeError(ErrorCode::InvalidArgument, "surface exceeds the 32-bit index range");
    }
    for (std::size_t i = 0; i < vertices.size(); ++i) {
        if (!vertices[i].isFinite()) {
            return makeError(ErrorCode::InvalidArgument, "vertex has a non-finite coordinate",
                             "vertex=" + std::to_string(i));
        }
    }
    if (!constrainedEdges.empty() && constrainedEdges.size() != triangles.size()) {
        return makeError(ErrorCode::InvalidArgument,
                         "constrainedEdges must be empty or hold one mask per triangle",
                         "masks=" + std::to_string(constrainedEdges.size()) +
                             " triangles=" + std::to_string(triangles.size()));
    }
    if (constrainedEdges.empty()) {
        constrainedEdges.assign(triangles.size(), std::uint8_t{0});
    }

    std::vector<HalfEdge> halfEdges;
    halfEdges.reserve(3 * triangles.size());
    for (std::size_t t = 0; t < triangles.size(); ++t) {
        const TinTriangle& tri = triangles[t];
        const std::string context = "triangle=" + std::to_string(t);
        for (const std::uint32_t v : tri) {
            if (v >= vertices.size()) {
                return makeError(ErrorCode::InvalidArgument, "triangle names a vertex out of range",
                                 context);
            }
        }
        if (tri[0] == tri[1] || tri[1] == tri[2] || tri[2] == tri[0]) {
            return makeError(ErrorCode::InvalidArgument, "triangle names a vertex twice", context);
        }
        if (constrainedEdges[t] > 7) {
            return makeError(ErrorCode::InvalidArgument, "constraint mask uses bits beyond 0..2",
                             context);
        }
        const Triangle2 plan{Point2(vertices[tri[0]].x, vertices[tri[0]].y),
                             Point2(vertices[tri[1]].x, vertices[tri[1]].y),
                             Point2(vertices[tri[2]].x, vertices[tri[2]].y)};
        if (plan.signedArea() < 0.0 && !plan.isDegenerate()) {
            return makeError(ErrorCode::InvalidGeometry,
                             "triangle is clockwise in plan view; triangles must be "
                             "counter-clockwise",
                             context);
        }
        for (std::uint8_t k = 0; k < 3; ++k) {
            const std::uint32_t from = tri[k];
            const std::uint32_t to = tri[(k + 1) % 3];
            halfEdges.push_back(HalfEdge{std::min(from, to), std::max(from, to),
                                         static_cast<std::uint32_t>(t), k, from < to});
        }
    }

    // Adjacency: the two uses of an interior edge are neighbours in the sorted list.
    std::sort(halfEdges.begin(), halfEdges.end(), [](const HalfEdge& a, const HalfEdge& b) {
        if (a.low != b.low) {
            return a.low < b.low;
        }
        if (a.high != b.high) {
            return a.high < b.high;
        }
        return a.triangle < b.triangle;
    });
    std::vector<TinNeighbors> neighbors(triangles.size(),
                                        TinNeighbors{kNoTriangle, kNoTriangle, kNoTriangle});
    for (std::size_t i = 0; i < halfEdges.size();) {
        std::size_t j = i + 1;
        while (j < halfEdges.size() && halfEdges[j].low == halfEdges[i].low &&
               halfEdges[j].high == halfEdges[i].high) {
            ++j;
        }
        const HalfEdge& first = halfEdges[i];
        const std::string context =
            "edge=" + std::to_string(first.low) + "-" + std::to_string(first.high);
        if (j - i > 2) {
            return makeError(ErrorCode::InvalidGeometry,
                             "edge is shared by more than two triangles", context);
        }
        if (j - i == 2) {
            const HalfEdge& second = halfEdges[i + 1];
            if (first.ascending == second.ascending) {
                return makeError(ErrorCode::InvalidGeometry,
                                 "two triangles run along an edge in the same direction "
                                 "(overlapping or inconsistently oriented triangles)",
                                 context);
            }
            const bool firstFlag = (constrainedEdges[first.triangle] >> first.edge) & 1u;
            const bool secondFlag = (constrainedEdges[second.triangle] >> second.edge) & 1u;
            if (firstFlag != secondFlag) {
                return makeError(ErrorCode::InvalidArgument,
                                 "constraint flags disagree across a shared edge", context);
            }
            neighbors[first.triangle][first.edge] = second.triangle;
            neighbors[second.triangle][second.edge] = first.triangle;
        }
        i = j;
    }

    TinSurface surface;
    surface.vertices_ = std::move(vertices);
    surface.triangles_ = std::move(triangles);
    surface.neighbors_ = std::move(neighbors);
    surface.constrainedEdges_ = std::move(constrainedEdges);
    if (const Status status = surface.finalize(); !status) {
        return status.error();
    }
    return surface;
}

Status TinSurface::finalize()
{
    bounds_ = Box2{};
    minElevation_ = 0.0;
    maxElevation_ = 0.0;
    planArea_ = 0.0;
    surfaceArea_ = 0.0;
    grid_ = detail::LocatorGrid{};
    if (triangles_.empty()) {
        return {};
    }

    double minZ = std::numeric_limits<double>::infinity();
    double maxZ = -std::numeric_limits<double>::infinity();
    detail::CompensatedSum planSum;
    detail::CompensatedSum surfaceSum;
    for (std::size_t t = 0; t < triangles_.size(); ++t) {
        for (const std::uint32_t v : triangles_[t]) {
            const Point3& p = vertices_[v];
            bounds_.expand(Point2(p.x, p.y));
            minZ = std::min(minZ, p.z);
            maxZ = std::max(maxZ, p.z);
        }
        planSum.add(trianglePlanArea(t));
        surfaceSum.add(triangleSurfaceArea(t));
    }
    minElevation_ = minZ;
    maxElevation_ = maxZ;
    planArea_ = planSum.value();
    surfaceArea_ = surfaceSum.value();

    // Grid of roughly square cells holding kTrianglesPerCell triangles on average.
    const double width = bounds_.width();
    const double height = bounds_.height();
    const double targetCells =
        std::max(1.0, static_cast<double>(triangles_.size()) / kTrianglesPerCell);
    grid_.origin = bounds_.min;
    grid_.columns = 1;
    grid_.rows = 1;
    if (width > 0.0 && height > 0.0) {
        const double cell = std::sqrt(width * height / targetCells);
        // An extremely elongated extent must not explode one dimension.
        const double limit = 2.0 * targetCells;
        grid_.columns = static_cast<std::uint32_t>(std::clamp(std::ceil(width / cell), 1.0, limit));
        grid_.rows = static_cast<std::uint32_t>(std::clamp(std::ceil(height / cell), 1.0, limit));
    }
    grid_.cellWidth = width > 0.0 ? width / grid_.columns : 1.0;
    grid_.cellHeight = height > 0.0 ? height / grid_.rows : 1.0;

    const std::size_t cellCount = static_cast<std::size_t>(grid_.columns) * grid_.rows;
    std::vector<std::uint64_t> counts(cellCount + 1, 0);
    const auto corner = [this](std::size_t t, std::size_t k) {
        const Point3& p = vertices_[triangles_[t][k]];
        return Point2(p.x, p.y);
    };
    for (std::size_t t = 0; t < triangles_.size(); ++t) {
        visitCells(grid_, corner(t, 0), corner(t, 1), corner(t, 2),
                   [&counts](std::size_t cell) { ++counts[cell + 1]; });
    }
    for (std::size_t cell = 0; cell < cellCount; ++cell) {
        counts[cell + 1] += counts[cell];
    }
    if (counts[cellCount] >= kNoTriangle) {
        return makeError(ErrorCode::InvalidArgument,
                         "surface is too large for the 32-bit locator index",
                         "entries=" + std::to_string(counts[cellCount]));
    }
    grid_.cellStart.assign(counts.begin(), counts.end());
    grid_.cellTriangles.resize(static_cast<std::size_t>(counts[cellCount]));
    std::vector<std::uint32_t> cursor(grid_.cellStart.begin(), grid_.cellStart.end() - 1);
    for (std::size_t t = 0; t < triangles_.size(); ++t) {
        visitCells(grid_, corner(t, 0), corner(t, 1), corner(t, 2), [&](std::size_t cell) {
            grid_.cellTriangles[cursor[cell]++] = static_cast<std::uint32_t>(t);
        });
    }
    return {};
}

// ---- per triangle ---------------------------------------------------------------

bool TinSurface::isEdgeConstrained(std::size_t triangle, std::size_t edge) const
{
    return edge < 3 && ((constrainedEdges_.at(triangle) >> edge) & 1u) != 0;
}

Triangle2 TinSurface::planTriangle(std::size_t triangle) const
{
    const TinTriangle& tri = triangles_.at(triangle);
    const Point3& a = vertices_[tri[0]];
    const Point3& b = vertices_[tri[1]];
    const Point3& c = vertices_[tri[2]];
    return Triangle2{Point2(a.x, a.y), Point2(b.x, b.y), Point2(c.x, c.y)};
}

Triangle3 TinSurface::spaceTriangle(std::size_t triangle) const
{
    const TinTriangle& tri = triangles_.at(triangle);
    return Triangle3{vertices_[tri[0]], vertices_[tri[1]], vertices_[tri[2]]};
}

double TinSurface::trianglePlanArea(std::size_t triangle) const
{
    // Slivers that are counter-clockwise by exact arithmetic can evaluate to a
    // rounding-sized negative area; they have no area.
    return std::max(0.0, planTriangle(triangle).signedArea());
}

double TinSurface::triangleSurfaceArea(std::size_t triangle) const
{
    return spaceTriangle(triangle).area();
}

std::optional<SlopeAspect> TinSurface::triangleSlopeAspect(std::size_t triangle) const
{
    if (planTriangle(triangle).isDegenerate()) {
        return std::nullopt;
    }
    // Normal n = (b - a) x (c - a) points up (n.z = twice the plan area > 0). The
    // plane is z = const - (n.x * x + n.y * y) / n.z, so the gradient is
    // -(n.x, n.y) / n.z and the steepest descent runs along +(n.x, n.y).
    const katana::geometry::Vec3 normal = spaceTriangle(triangle).scaledNormal();
    const double horizontal = std::hypot(normal.x, normal.y);
    SlopeAspect result;
    result.slope = horizontal / normal.z;
    result.slopeAngle = std::atan2(horizontal, normal.z);
    if (result.slopeAngle > tol::kAngular) {
        // Azimuth: clockwise from +y, i.e. atan2(east, north).
        result.aspect = katana::math::normalizeAngle(std::atan2(normal.x, normal.y));
    }
    return result;
}

// ---- point queries --------------------------------------------------------------

std::optional<SurfaceLocation> TinSurface::locate(const Point2& position) const
{
    if (triangles_.empty() || !position.isFinite() ||
        !bounds_.inflated(tol::kGeometric).contains(position)) {
        return std::nullopt;
    }
    const std::size_t cell =
        static_cast<std::size_t>(
            cellCoordinate(position.y, grid_.origin.y, grid_.cellHeight, grid_.rows)) *
            grid_.columns +
        cellCoordinate(position.x, grid_.origin.x, grid_.cellWidth, grid_.columns);
    const std::uint32_t first = grid_.cellStart[cell];
    const std::uint32_t last = grid_.cellStart[cell + 1];

    const auto corner = [this](const TinTriangle& tri, std::size_t k) {
        const Point3& p = vertices_[tri[k]];
        return Point2(p.x, p.y);
    };

    // Pass 1: containment by the three edge functions, evaluated on coordinate
    // differences so that large projected coordinates keep their precision.
    std::uint32_t scalarFirst = first;
#if defined(KATANA_HAVE_AVX2_KERNELS)
    // The kernel finds the next candidate the sign tests accept, four at a
    // time; the sliver test stays here, and a sliver resumes the search after
    // itself, so the triangle chosen is the one the loop below would choose.
    if (last - first >= kEnclosingKernelMinimum && detail::avx2Active() &&
        detail::kernelsMayRead(*this)) {
        const std::uint32_t* candidates = grid_.cellTriangles.data() + first;
        const std::size_t count = last - first;
        for (std::size_t i = 0;; ++i) {
            double d[3];
            i = katana_avx2_first_enclosing(detail::vertexData(*this), detail::triangleData(*this),
                                            candidates, i, count, position.x, position.y, d);
            if (i == count) {
                break;
            }
            const TinTriangle& tri = triangles_[candidates[i]];
            const double twiceArea = d[0] + d[1] + d[2];
            if (isSliver(corner(tri, 0), corner(tri, 1), corner(tri, 2), twiceArea)) {
                continue;
            }
            return SurfaceLocation{candidates[i], {d[1] / twiceArea, d[2] / twiceArea, d[0] / twiceArea},
                                   true};
        }
        scalarFirst = last; // every candidate has been tested
    }
#endif
    for (std::uint32_t i = scalarFirst; i < last; ++i) {
        const std::uint32_t t = grid_.cellTriangles[i];
        const TinTriangle& tri = triangles_[t];
        const Point2 a = corner(tri, 0);
        const Point2 b = corner(tri, 1);
        const Point2 c = corner(tri, 2);
        const double d0 = (b - a).cross(position - a); // weight of c, times twice the area
        const double d1 = (c - b).cross(position - b); // weight of a
        const double d2 = (a - c).cross(position - c); // weight of b
        if (d0 < 0.0 || d1 < 0.0 || d2 < 0.0) {
            continue;
        }
        const double twiceArea = d0 + d1 + d2;
        if (isSliver(a, b, c, twiceArea)) {
            continue; // handled by distance below
        }
        return SurfaceLocation{t, {d1 / twiceArea, d2 / twiceArea, d0 / twiceArea}, true};
    }

    // Pass 2: the position is on no triangle by the sign tests. It may still lie
    // within kGeometric of one (on the rim, in a rounding-sized crack between
    // two triangles, or on a sliver): take the closest edge, lowest triangle
    // index first, and interpolate along it.
    double bestDistance = tol::kGeometric;
    std::optional<SurfaceLocation> best;
    for (std::uint32_t i = first; i < last; ++i) {
        const std::uint32_t t = grid_.cellTriangles[i];
        const TinTriangle& tri = triangles_[t];
        for (std::size_t k = 0; k < 3; ++k) {
            const Segment2 edge{corner(tri, k), corner(tri, (k + 1) % 3)};
            const double parameter = edge.parameterOf(position);
            const double distance = edge.pointAt(parameter).distanceTo(position);
            if (distance < bestDistance || (distance == bestDistance && !best)) {
                bestDistance = distance;
                SurfaceLocation location{t, {0.0, 0.0, 0.0}, false};
                location.weights[k] = 1.0 - parameter;
                location.weights[(k + 1) % 3] = parameter;
                best = location;
            }
        }
    }
    return best;
}

std::optional<double> TinSurface::elevationAt(const Point2& position) const
{
    const auto location = locate(position);
    if (!location) {
        return std::nullopt;
    }
    const TinTriangle& tri = triangles_[location->triangle];
    return location->weights[0] * vertices_[tri[0]].z + location->weights[1] * vertices_[tri[1]].z +
           location->weights[2] * vertices_[tri[2]].z;
}

std::vector<std::optional<double>>
TinSurface::elevationsAt(std::span<const Point2> positions) const
{
    // Positions per parallel chunk. elevationAt() measured 1.09 us on the
    // 200 000 triangle surface of BM_ElevationAt (Release, GCC 16.2,
    // 16 x 2496 MHz), so a chunk is a quarter of a millisecond of work - far
    // above what publishing a job to the pool costs, and still small enough
    // that a 4096-position section leaves every thread a share.
    constexpr std::size_t kElevationGrain = 256;
    // Sized up front and written by index, never appended: each position's
    // answer depends on nothing but that position, so the bytes returned are
    // the same at any thread count (Rule 7). locate() is const and reads only
    // the immutable surface, which is this class's concurrency promise.
    std::vector<std::optional<double>> elevations(positions.size());
    const auto fill = [&](std::size_t lo, std::size_t hi) {
        for (std::size_t i = lo; i < hi; ++i) {
            elevations[i] = elevationAt(positions[i]);
        }
    };
    if (positions.size() <= kElevationGrain) {
        fill(0, positions.size()); // nothing to share out; waking the pool would cost more
    } else {
        katana::core::TaskPool::shared().parallelRanges(0, positions.size(), kElevationGrain,
                                                        fill);
    }
    return elevations;
}

std::optional<double> TinSurface::slopeAt(const Point2& position) const
{
    const auto location = locate(position);
    if (!location) {
        return std::nullopt;
    }
    const auto slopeAspect = triangleSlopeAspect(location->triangle);
    return slopeAspect ? std::optional<double>(slopeAspect->slope) : std::nullopt;
}

std::optional<double> TinSurface::aspectAt(const Point2& position) const
{
    const auto location = locate(position);
    if (!location) {
        return std::nullopt;
    }
    const auto slopeAspect = triangleSlopeAspect(location->triangle);
    return slopeAspect ? slopeAspect->aspect : std::nullopt;
}

} // namespace katana::terrain
