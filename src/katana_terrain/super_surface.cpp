#include "katana/terrain/super_surface.hpp"

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <limits>
#include <span>

#include "katana/math/numerics.hpp"

namespace katana::terrain {

namespace tol = katana::math::tolerance;
using katana::core::Result;
using katana::geometry::Box2;
using katana::geometry::Point2;
using katana::geometry::Point3;

namespace {

// Does `point` lie in the triangle abc, seen in plan? Sign of the three
// cross products, accepting either winding, with the boundary counted as
// inside - a point on the seam belongs to the higher member.
bool inTriangle(const Point2& point, const Point3& a, const Point3& b, const Point3& c)
{
    const auto side = [&point](const Point3& from, const Point3& to) {
        return (to.x - from.x) * (point.y - from.y) - (to.y - from.y) * (point.x - from.x);
    };
    const double ab = side(a, b);
    const double bc = side(b, c);
    const double ca = side(c, a);
    return (ab >= 0.0 && bc >= 0.0 && ca >= 0.0) || (ab <= 0.0 && bc <= 0.0 && ca <= 0.0);
}

// Roughly one vertex per cell: the query walks whole cells, so fewer cells
// means longer lists to scan and more means a build that costs more than the
// scan it replaces. The surface's own locator (tin_surface.cpp) sets the same
// kind of target for the same reason.
constexpr double kVerticesPerCell = 1.0;

// Bucket grid over one member's vertices, in the CSR layout the surface
// locator uses: two flat arrays rather than a vector per cell.
//
// WHY. overlaps() asks "does any vertex of the higher surface fall in this
// triangle?", and used to answer it by scanning every vertex - for every
// triangle of every lower-ranked member, against every higher-ranked member.
// That is O(triangles x members x vertices): two 200 000-triangle members
// make about 2e10 point-in-triangle tests, and a 12d super-TIN import reaches
// it with real data. The whole-surface bounding-box test above it does not
// help, because members that genuinely overlap all pass it.
class VertexGrid {
  public:
    explicit VertexGrid(std::span<const Point3> vertices) : vertices_(vertices)
    {
        Box2 bounds;
        for (const Point3& vertex : vertices_) {
            bounds.expand(Point2(vertex.x, vertex.y));
        }
        if (bounds.empty() || !std::isfinite(bounds.min.x) || !std::isfinite(bounds.min.y) ||
            !std::isfinite(bounds.max.x) || !std::isfinite(bounds.max.y)) {
            return; // unusable; callers fall back to the exhaustive scan
        }
        origin_ = bounds.min;
        const double width = bounds.width();
        const double height = bounds.height();
        const double targetCells =
            std::max(1.0, static_cast<double>(vertices_.size()) / kVerticesPerCell);
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
        cellStart_.assign(cellCount + 1, 0);
        for (const Point3& vertex : vertices_) {
            ++cellStart_[cellOf(vertex) + 1];
        }
        for (std::size_t cell = 0; cell < cellCount; ++cell) {
            cellStart_[cell + 1] += cellStart_[cell];
        }
        cellVertices_.resize(vertices_.size());
        std::vector<std::uint32_t> cursor(cellStart_.begin(), cellStart_.end() - 1);
        for (std::size_t i = 0; i < vertices_.size(); ++i) {
            cellVertices_[cursor[cellOf(vertices_[i])]++] = static_cast<std::uint32_t>(i);
        }
        usable_ = true;
    }

    [[nodiscard]] bool usable() const { return usable_; }

    // True when `accept` says yes to any vertex in the cells [minX, maxX] x
    // [minY, maxY] touches. A superset of the box's own contents, never a
    // subset, so an `accept` that only ever says yes inside the box gets the
    // same answer it would from every vertex in the surface.
    template <typename Accept>
    bool anyIn(double minX, double minY, double maxX, double maxY, Accept&& accept) const
    {
        const std::uint32_t column0 = columnOf(minX);
        const std::uint32_t column1 = columnOf(maxX);
        const std::uint32_t row0 = rowOf(minY);
        const std::uint32_t row1 = rowOf(maxY);
        for (std::uint32_t row = row0; row <= row1; ++row) {
            const std::size_t rowBase = static_cast<std::size_t>(row) * columns_;
            for (std::uint32_t column = column0; column <= column1; ++column) {
                const std::size_t cell = rowBase + column;
                for (std::uint32_t i = cellStart_[cell]; i < cellStart_[cell + 1]; ++i) {
                    if (accept(vertices_[cellVertices_[i]])) {
                        return true;
                    }
                }
            }
        }
        return false;
    }

  private:
    [[nodiscard]] static std::uint32_t coordinate(double value, double origin, double cellSize,
                                                  std::uint32_t count)
    {
        const double cell = std::floor((value - origin) / cellSize);
        if (!(cell > 0.0)) {
            return 0; // also catches NaN, which belongs in the first cell rather than nowhere
        }
        return cell >= static_cast<double>(count) ? count - 1 : static_cast<std::uint32_t>(cell);
    }
    [[nodiscard]] std::uint32_t columnOf(double x) const
    {
        return coordinate(x, origin_.x, cellWidth_, columns_);
    }
    [[nodiscard]] std::uint32_t rowOf(double y) const
    {
        return coordinate(y, origin_.y, cellHeight_, rows_);
    }
    [[nodiscard]] std::size_t cellOf(const Point3& vertex) const
    {
        return static_cast<std::size_t>(rowOf(vertex.y)) * columns_ + columnOf(vertex.x);
    }

    std::span<const Point3> vertices_;
    Point2 origin_;
    double cellWidth_ = 1.0;
    double cellHeight_ = 1.0;
    std::uint32_t columns_ = 1;
    std::uint32_t rows_ = 1;
    std::vector<std::uint32_t> cellStart_;    // columns_ * rows_ + 1 offsets
    std::vector<std::uint32_t> cellVertices_; // ascending within each cell
    bool usable_ = false;
};

// Does `higher` cover any part of the triangle abc? Sampled rather than
// clipped: the triangle's corners and middle against the surface, and the
// surface's own vertices against the triangle. That misses only the case
// where the two overlap without either's points falling inside the other -
// a sliver crossing a corner - which needs a segment-by-segment clip to
// find, and which moves the seam by less than the triangle it is cut from.
bool overlaps(const TinSurface& higher, const VertexGrid& grid, const Point3& a, const Point3& b,
              const Point3& c)
{
    const Box2 box = higher.bounds();
    const double minX = std::min({a.x, b.x, c.x});
    const double maxX = std::max({a.x, b.x, c.x});
    const double minY = std::min({a.y, b.y, c.y});
    const double maxY = std::max({a.y, b.y, c.y});
    if (box.empty() || maxX < box.min.x || minX > box.max.x || maxY < box.min.y ||
        minY > box.max.y) {
        return false;
    }
    const Point2 corners[] = {Point2(a.x, a.y), Point2(b.x, b.y), Point2(c.x, c.y),
                              Point2((a.x + b.x + c.x) / 3.0, (a.y + b.y + c.y) / 3.0)};
    for (const Point2& corner : corners) {
        if (higher.elevationAt(corner).has_value()) {
            return true;
        }
    }

    const auto accepts = [&a, &b, &c](const Point3& vertex) {
        return inTriangle(Point2(vertex.x, vertex.y), a, b, c);
    };

    // The three sign tests sum identically to the doubled signed area, so when
    // that is non-zero every point they all accept has each term in
    // [0, twiceArea] and therefore lies in the triangle - inside this box.
    // When it is exactly zero the accepted set is a line or the whole plane
    // and no box bounds it, so that triangle keeps the exhaustive scan. It is
    // the fully collinear triangle, which a TIN does not contain.
    const double twiceArea = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
    if (!grid.usable() || twiceArea == 0.0) {
        for (const Point3& vertex : higher.vertices()) {
            if (accepts(vertex)) {
                return true;
            }
        }
        return false;
    }

    // Rounding lets a point just outside an edge be accepted. Each sign test
    // is two products differenced, so its absolute error is at most a dozen
    // ulps of |edge| * |point - vertex|, which displaces the accepted region
    // by a few dozen ulps of the triangle's own extent. kGeometric plus sixty
    // four ulps of the coordinates covers that with room to spare, and it is
    // far too small to matter to the cull.
    const double scale =
        std::max({std::abs(minX), std::abs(maxX), std::abs(minY), std::abs(maxY)});
    const double margin = tol::kGeometric + 64.0 * std::numeric_limits<double>::epsilon() * scale;
    return grid.anyIn(minX - margin, minY - margin, maxX + margin, maxY + margin, accepts);
}

} // namespace

Result<TinSurface> combineSurfaces(const std::vector<const TinSurface*>& membersInRankOrder)
{
    std::vector<const TinSurface*> members;
    members.reserve(membersInRankOrder.size());
    for (const TinSurface* member : membersInRankOrder) {
        if (member != nullptr && !member->empty()) {
            members.push_back(member);
        }
    }

    // One grid per member, built before the triangle loop rather than inside
    // it: every lower-ranked triangle queries the same higher-ranked member.
    std::vector<VertexGrid> grids;
    grids.reserve(members.size());
    for (std::size_t rank = 0; rank < members.size(); ++rank) {
        // Rank 0 is never a HIGHER member of anything, so its grid would never
        // be queried - and it is usually the survey tin, the biggest of them.
        grids.emplace_back(rank == 0 ? std::span<const Point3>()
                                     : std::span<const Point3>(members[rank]->vertices()));
    }

    std::vector<Point3> vertices;
    std::vector<TinTriangle> triangles;
    std::vector<std::uint8_t> constrained;
    for (std::size_t rank = 0; rank < members.size(); ++rank) {
        const TinSurface& member = *members[rank];
        const auto base = static_cast<std::uint32_t>(vertices.size());
        vertices.insert(vertices.end(), member.vertices().begin(), member.vertices().end());

        for (std::size_t t = 0; t < member.triangleCount(); ++t) {
            const TinTriangle& triangle = member.triangles()[t];
            const Point3& a = member.vertices()[triangle[0]];
            const Point3& b = member.vertices()[triangle[1]];
            const Point3& c = member.vertices()[triangle[2]];
            // Dropped if ANY higher-ranked member speaks for this ground -
            // not merely if it covers the middle of it. Two sheets over one
            // point would make elevationAt() answer with whichever triangle
            // it met first, which is the base as often as the pad; a
            // triangle removed a little too eagerly leaves no answer there,
            // and no answer is what Katana already says off a surface.
            bool overridden = false;
            for (std::size_t higher = rank + 1; higher < members.size(); ++higher) {
                if (overlaps(*members[higher], grids[higher], a, b, c)) {
                    overridden = true;
                    break;
                }
            }
            if (overridden) {
                continue;
            }
            triangles.push_back(TinTriangle{base + triangle[0], base + triangle[1],
                                            base + triangle[2]});
            // One bit per edge, rebuilt as the member had it so that a
            // breakline stays a breakline in the combined surface.
            std::uint8_t mask = 0;
            for (std::size_t e = 0; e < 3; ++e) {
                if (member.isEdgeConstrained(t, e)) {
                    mask |= static_cast<std::uint8_t>(1u << e);
                }
            }
            constrained.push_back(mask);
        }
    }

    // Vertices of a member whose triangles were all overridden are left in
    // the list: TinSurface::create accepts them, and compacting would cost a
    // pass over every vertex of every surface to save memory that the
    // triangles already dwarf.
    return TinSurface::create(std::move(vertices), std::move(triangles), std::move(constrained));
}

} // namespace katana::terrain
