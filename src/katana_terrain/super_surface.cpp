#include "katana/terrain/super_surface.hpp"

#include <algorithm>
#include <cstdint>
#include <limits>

namespace katana::terrain {

using katana::core::Result;
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

// Does `higher` cover any part of the triangle abc? Sampled rather than
// clipped: the triangle's corners and middle against the surface, and the
// surface's own vertices against the triangle. That misses only the case
// where the two overlap without either's points falling inside the other -
// a sliver crossing a corner - which needs a segment-by-segment clip to
// find, and which moves the seam by less than the triangle it is cut from.
bool overlaps(const TinSurface& higher, const Point3& a, const Point3& b, const Point3& c)
{
    const katana::geometry::Box2 box = higher.bounds();
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
    for (const Point3& vertex : higher.vertices()) {
        if (inTriangle(Point2(vertex.x, vertex.y), a, b, c)) {
            return true;
        }
    }
    return false;
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
            const bool overridden =
                std::any_of(members.begin() + static_cast<std::ptrdiff_t>(rank) + 1, members.end(),
                            [&](const TinSurface* higher) { return overlaps(*higher, a, b, c); });
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
