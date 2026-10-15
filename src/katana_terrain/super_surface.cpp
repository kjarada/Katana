#include "katana/terrain/super_surface.hpp"

#include <algorithm>
#include <array>
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

// Roughly two triangles per cell: the query walks whole cells, so fewer cells
// means longer lists to scan and more means a build that costs more than the
// scan it replaces. The surface's own locator (tin_surface.cpp) sets the same
// kind of target for the same reason.
constexpr double kTrianglesPerCell = 2.0;

// A triangle of the plan, counter-clockwise.
using PlanTriangle = std::array<Point2, 3>;

// How far `p` lies to the inside of the directed edge from -> to of a
// counter-clockwise triangle: positive on its left, in model units.
double inward(const Point2& from, const Point2& to, const Point2& p)
{
    const double length = std::hypot(to.x - from.x, to.y - from.y);
    if (!(length > 0.0)) {
        return 0.0;
    }
    return ((to.x - from.x) * (p.y - from.y) - (to.y - from.y) * (p.x - from.x)) / length;
}

// Whether a line along an edge of `edgeOf` has all of `other` on its outer
// side or within `margin` of it: a line that parts the two triangles to within
// the margin, which is all a separating axis has to be.
bool separatesByAnEdge(const PlanTriangle& edgeOf, const PlanTriangle& other, double margin)
{
    for (std::size_t k = 0; k < 3; ++k) {
        const Point2& from = edgeOf[k];
        const Point2& to = edgeOf[(k + 1) % 3];
        if (inward(from, to, other[0]) <= margin && inward(from, to, other[1]) <= margin &&
            inward(from, to, other[2]) <= margin) {
            return true;
        }
    }
    return false;
}

// Do two counter-clockwise triangles share an AREA - more than a point or an
// edge? Convex shapes have no common interior exactly when one edge's line
// parts them (the separating axis theorem; for triangles the edge lines are
// all the axes there are), and "parts them" is read with the coincidence
// rule: a vertex of the one within `margin` of an edge line of the other is
// on the line, not beyond it. So triangles that abut - along an edge, at a
// corner, or by a sliver thinner than kGeometric that rounding of the
// coordinates left - touch; they do not overlap.
bool shareArea(const PlanTriangle& t, const PlanTriangle& u, double margin)
{
    return !separatesByAnEdge(t, u, margin) && !separatesByAnEdge(u, t, margin);
}

// Triangles of one member bucketed by plan extent in a uniform grid, in the
// CSR layout the surface locator uses: two flat arrays rather than a vector
// per cell.
//
// WHY. overlaps() asks "does this higher member share area with this
// triangle?", and answered it by scanning every vertex - for every triangle
// of every lower-ranked member, against every higher-ranked member. That is
// O(triangles x members x vertices): two 200 000-triangle members make about
// 2e10 tests, and a super-TIN import reaches it with real data. The
// whole-surface bounding-box test above it does not help, because members
// that genuinely overlap all pass it.
//
// Why triangles rather than the vertices it used to bucket: whether two
// triangles share an area is decided by the pair, exactly as far as the
// coincidence rule goes. A vertex of the higher member inside the lower
// triangle, or a corner of it inside the higher surface, finds most overlaps
// and none of those that merely touch - and misses a strip crossing a
// triangle whose vertices lie ON its edges, which share area all the same
// and left two sheets over one point.
class TriangleGrid {
  public:
    explicit TriangleGrid(const TinSurface& surface, bool build) : surface_(&surface)
    {
        if (!build || surface.empty()) {
            return;
        }
        const Box2 bounds = surface.bounds();
        if (bounds.empty() || !std::isfinite(bounds.min.x) || !std::isfinite(bounds.min.y) ||
            !std::isfinite(bounds.max.x) || !std::isfinite(bounds.max.y)) {
            return; // unusable: nothing is reported as overlapping
        }
        origin_ = bounds.min;
        const double width = bounds.width();
        const double height = bounds.height();
        const double targetCells =
            std::max(1.0, static_cast<double>(surface.triangleCount()) / kTrianglesPerCell);
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
        for (std::size_t t = 0; t < surface.triangleCount(); ++t) {
            const auto range = cellsOf(extentOf(t));
            for (std::uint32_t row = range[2]; row <= range[3]; ++row) {
                for (std::uint32_t column = range[0]; column <= range[1]; ++column) {
                    ++cellStart_[static_cast<std::size_t>(row) * columns_ + column + 1];
                }
            }
        }
        for (std::size_t cell = 0; cell < cellCount; ++cell) {
            cellStart_[cell + 1] += cellStart_[cell];
        }
        cellTriangles_.resize(cellStart_.back());
        std::vector<std::uint32_t> cursor(cellStart_.begin(), cellStart_.end() - 1);
        for (std::size_t t = 0; t < surface.triangleCount(); ++t) {
            const auto range = cellsOf(extentOf(t));
            for (std::uint32_t row = range[2]; row <= range[3]; ++row) {
                for (std::uint32_t column = range[0]; column <= range[1]; ++column) {
                    cellTriangles_[cursor[static_cast<std::size_t>(row) * columns_ + column]++] =
                        static_cast<std::uint32_t>(t);
                }
            }
        }
        usable_ = true;
    }

    [[nodiscard]] bool usable() const { return usable_; }

    // True when `accept` says yes to any triangle in the cells the box
    // [minX, maxX] x [minY, maxY] touches (ascending within a cell). A
    // superset of the box's own contents, never a subset, so an `accept` that
    // only ever says yes inside the box gets the same answer it would from
    // every triangle in the surface.
    template <typename Accept>
    bool anyIn(double minX, double minY, double maxX, double maxY, Accept&& accept) const
    {
        const auto range = cellsOf({minX, minY, maxX, maxY});
        for (std::uint32_t row = range[2]; row <= range[3]; ++row) {
            const std::size_t rowBase = static_cast<std::size_t>(row) * columns_;
            for (std::uint32_t column = range[0]; column <= range[1]; ++column) {
                const std::size_t cell = rowBase + column;
                for (std::uint32_t i = cellStart_[cell]; i < cellStart_[cell + 1]; ++i) {
                    if (accept(cellTriangles_[i])) {
                        return true;
                    }
                }
            }
        }
        return false;
    }

  private:
    struct Extent {
        double minX;
        double minY;
        double maxX;
        double maxY;
    };

    [[nodiscard]] Extent extentOf(std::size_t t) const
    {
        const TinTriangle& tri = surface_->triangles()[t];
        const Point3& a = surface_->vertices()[tri[0]];
        const Point3& b = surface_->vertices()[tri[1]];
        const Point3& c = surface_->vertices()[tri[2]];
        return {std::min({a.x, b.x, c.x}), std::min({a.y, b.y, c.y}), std::max({a.x, b.x, c.x}),
                std::max({a.y, b.y, c.y})};
    }
    [[nodiscard]] std::array<std::uint32_t, 4> cellsOf(const Extent& e) const
    {
        return {coordinate(e.minX, origin_.x, cellWidth_, columns_),
                coordinate(e.maxX, origin_.x, cellWidth_, columns_),
                coordinate(e.minY, origin_.y, cellHeight_, rows_),
                coordinate(e.maxY, origin_.y, cellHeight_, rows_)};
    }
    [[nodiscard]] static std::uint32_t coordinate(double value, double origin, double cellSize,
                                                  std::uint32_t count)
    {
        const double cell = std::floor((value - origin) / cellSize);
        if (!(cell > 0.0)) {
            return 0; // also catches NaN, which belongs in the first cell rather than nowhere
        }
        return cell >= static_cast<double>(count) ? count - 1 : static_cast<std::uint32_t>(cell);
    }

    const TinSurface* surface_ = nullptr;
    Point2 origin_;
    double cellWidth_ = 1.0;
    double cellHeight_ = 1.0;
    std::uint32_t columns_ = 1;
    std::uint32_t rows_ = 1;
    std::vector<std::uint32_t> cellStart_;     // columns_ * rows_ + 1 offsets
    std::vector<std::uint32_t> cellTriangles_; // ascending within each cell
    bool usable_ = false;
};

// Does `higher` cover any AREA of the triangle abc? Some triangle of it shares
// an area with abc (shareArea): touching along an edge or at a corner does not
// cover. Exact to the coincidence rule, not sampled, so a strip that crosses
// the triangle between its corners and its middle is found as surely as a pad
// over its middle.
bool overlaps(const TinSurface& higher, const TriangleGrid& grid, const Point3& a, const Point3& b,
              const Point3& c)
{
    const Box2 box = higher.bounds();
    const double minX = std::min({a.x, b.x, c.x});
    const double maxX = std::max({a.x, b.x, c.x});
    const double minY = std::min({a.y, b.y, c.y});
    const double maxY = std::max({a.y, b.y, c.y});
    if (!grid.usable() || box.empty() || maxX < box.min.x || minX > box.max.x ||
        maxY < box.min.y || minY > box.max.y) {
        return false;
    }
    const PlanTriangle lower{Point2(a.x, a.y), Point2(b.x, b.y), Point2(c.x, c.y)};
    const double twiceArea = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
    if (!(twiceArea > 0.0)) {
        return false; // a flat triangle covers nothing, and nothing covers it
    }

    // The inward distances are two products differenced, divided by a length:
    // their absolute error is a few dozen ulps of the coordinates, which
    // kGeometric plus sixty-four ulps of them covers with room to spare (the
    // rounding margin tin_patch.cpp and the cull used before it derive).
    const double scale =
        std::max({std::abs(minX), std::abs(maxX), std::abs(minY), std::abs(maxY)});
    const double margin = tol::kGeometric + 64.0 * std::numeric_limits<double>::epsilon() * scale;
    const auto& vertices = higher.vertices();
    const auto& triangles = higher.triangles();
    const auto accepts = [&](std::uint32_t t) {
        const Point3& p = vertices[triangles[t][0]];
        const Point3& q = vertices[triangles[t][1]];
        const Point3& r = vertices[triangles[t][2]];
        // Boxes that only touch share no area: the cheap rejection first.
        if (std::min({p.x, q.x, r.x}) >= maxX - margin ||
            std::max({p.x, q.x, r.x}) <= minX + margin ||
            std::min({p.y, q.y, r.y}) >= maxY - margin ||
            std::max({p.y, q.y, r.y}) <= minY + margin) {
            return false;
        }
        if (!((q.x - p.x) * (r.y - p.y) - (q.y - p.y) * (r.x - p.x) > 0.0)) {
            return false; // flat, or turned over: it holds no area of its own
        }
        return shareArea(lower, {Point2(p.x, p.y), Point2(q.x, q.y), Point2(r.x, r.y)}, margin);
    };
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
    std::vector<TriangleGrid> grids;
    grids.reserve(members.size());
    for (std::size_t rank = 0; rank < members.size(); ++rank) {
        // Rank 0 is never a HIGHER member of anything, so its grid would never
        // be queried - and it is usually the survey tin, the biggest of them.
        grids.emplace_back(*members[rank], rank != 0);
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
            // Dropped if ANY higher-ranked member covers any AREA of this
            // ground - not merely if it covers the middle of it. Two sheets
            // over one point would make elevationAt() answer with whichever
            // triangle it met first, which is the base as often as the pad; a
            // triangle removed a little too eagerly leaves no answer there,
            // and no answer is what Katana already says off a surface. But
            // a member that only ABUTS - shares an edge or a corner, or a
            // sliver thinner than kGeometric - covers none of it, and the
            // triangle stays: two tiles of one survey merge into both.
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
