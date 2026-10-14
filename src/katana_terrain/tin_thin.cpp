// Thinning a surface (tin_edit.hpp, thinSurface; docs/surface_editing.md,
// "Thinning with a bound that holds").
//
// A vertex is removed by re-triangulating the polygon of its neighbours (its
// "link") with the one constrained Delaunay kernel. What the removal changes
// is measured exactly: the surfaces before and after are both linear on every
// piece of their overlay, so the largest difference is at a vertex of the
// overlay - the vertex removed itself, or where a new diagonal of the link
// crosses an old spoke to the vertex (the link's own corners do not move).
// Each triangle carries a bound on how far it lies from the ORIGINAL surface;
// a removal's new triangles inherit the largest bound of the star they
// replace plus the change it makes. So a vertex goes only when that sum is
// within the tolerance, and the bound holds for the whole result, not for
// each step alone.

#include "katana/terrain/tin_edit.hpp"

#include <algorithm>
#include <cmath>
#include <limits>
#include <optional>
#include <string>

#include "cdt_backend.hpp"
#include "katana/core/task_pool.hpp"
#include "katana/core/text.hpp"
#include "katana/math/numerics.hpp"
#include "tin_patch.hpp"

namespace katana::terrain {

namespace tol = katana::math::tolerance;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

namespace {

// Candidates per parallel chunk of the first measurement: each is a small
// triangulation (tens of microseconds), so 64 is a few milliseconds of work,
// far above the cost of handing a chunk to a thread.
constexpr std::size_t kThinGrain = 64;

Point2 planOf(const Point3& p) { return Point2(p.x, p.y); }

struct Removal {
    std::vector<TinTriangle> triangles; // the link re-triangulated
    std::vector<std::uint8_t> masks;
    // Per new triangle, a bit for each edge that is a side of the link (the
    // others are new diagonals).
    std::vector<std::uint8_t> sides;
    double change = 0.0; // the most the surface moves, vertically, anywhere in the star
};

// Removing `v`, whose live triangles are `star`, from the triangulation in
// `triangles`: nullopt when it cannot be removed cleanly - on the rim (its
// link does not close), a star that is not a simple fan, or a link the kernel
// has to split or add to.
std::optional<Removal> removalOf(std::uint32_t v, const std::vector<Point3>& vertices,
                                 const std::vector<TinTriangle>& triangles,
                                 const std::vector<std::uint8_t>& masks,
                                 std::span<const std::uint32_t> star)
{
    const std::size_t m = star.size();
    if (m < 3) {
        return std::nullopt;
    }
    struct LinkEdge {
        std::uint32_t from = 0;
        std::uint32_t to = 0;
        std::uint8_t flag = 0;
    };
    std::vector<LinkEdge> link;
    link.reserve(m);
    for (const std::uint32_t t : star) {
        const TinTriangle& tri = triangles[t];
        std::uint32_t r = 0;
        while (r < 3 && tri[r] != v) {
            ++r;
        }
        if (r == 3) {
            return std::nullopt;
        }
        const std::uint32_t edge = (r + 1) % 3; // the side opposite v
        link.push_back({tri[edge], tri[(edge + 1) % 3],
                        static_cast<std::uint8_t>((masks[t] >> edge) & 1u)});
    }
    // The link as a ring, in order: v lies on the left of every edge.
    std::vector<std::uint32_t> ring{link.front().from};
    std::vector<std::uint8_t> flags;
    std::vector<std::uint8_t> usedEdge(m, 0);
    for (std::size_t step = 0; step < m; ++step) {
        const std::uint32_t at = ring.back();
        std::size_t found = m;
        for (std::size_t e = 0; e < m; ++e) {
            if (usedEdge[e] == 0 && link[e].from == at) {
                found = e;
                break;
            }
        }
        if (found == m) {
            return std::nullopt; // the link does not close: v is on the rim
        }
        usedEdge[found] = 1;
        flags.push_back(link[found].flag);
        ring.push_back(link[found].to);
    }
    if (ring.back() != ring.front()) {
        return std::nullopt;
    }
    ring.pop_back();
    std::vector<std::uint32_t> sorted = ring;
    std::sort(sorted.begin(), sorted.end());
    if (std::adjacent_find(sorted.begin(), sorted.end()) != sorted.end()) {
        return std::nullopt; // a vertex twice round the link: not a simple fan
    }

    std::vector<Point2> positions;
    std::vector<detail::CdtConstraint> constraints;
    for (std::size_t i = 0; i < m; ++i) {
        positions.push_back(planOf(vertices[ring[i]]));
        constraints.push_back({static_cast<std::uint32_t>(i),
                               static_cast<std::uint32_t>((i + 1) % m), 1});
    }
    auto cdt = detail::triangulate({positions, constraints, true});
    if (!cdt || !cdt->generatedPoints.empty()) {
        return std::nullopt;
    }
    for (std::size_t c = 0; c < m; ++c) {
        if (cdt->constraintVertexOffsets[c + 1] - cdt->constraintVertexOffsets[c] != 2) {
            return std::nullopt;
        }
    }
    const std::size_t count = cdt->triangles.size();
    std::vector<std::uint8_t> inside(count, 0);
    std::vector<std::uint32_t> queue;
    for (std::uint32_t t = 0; t < count; ++t) {
        const auto& tri = cdt->triangles[t];
        for (std::size_t k = 0; k < 3; ++k) {
            if (cdt->edgeTags[t][k] != 0 && tri[(k + 1) % 3] == (tri[k] + 1) % m) {
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
            if (cdt->edgeTags[t][k] == 0 && next != detail::kNoIndex && inside[next] == 0) {
                inside[next] = 1;
                queue.push_back(next);
            }
        }
    }
    Removal removal;
    for (std::uint32_t t = 0; t < count; ++t) {
        if (inside[t] == 0) {
            continue;
        }
        const auto& tri = cdt->triangles[t];
        std::uint8_t mask = 0;
        std::uint8_t side = 0;
        for (std::size_t k = 0; k < 3; ++k) {
            if (cdt->edgeTags[t][k] != 0) {
                mask = static_cast<std::uint8_t>(mask | (flags[tri[k]] << k));
                side = static_cast<std::uint8_t>(side | (1u << k));
            }
        }
        removal.triangles.push_back({ring[tri[0]], ring[tri[1]], ring[tri[2]]});
        removal.masks.push_back(mask);
        removal.sides.push_back(side);
    }
    if (removal.triangles.size() + 2 != m) {
        return std::nullopt; // not a simple polygon's triangulation
    }

    // The change at v: its level against the new triangle it falls in.
    const Point3& centre = vertices[v];
    const Point2 at = planOf(centre);
    double bestWeight = -std::numeric_limits<double>::infinity();
    double level = centre.z;
    for (const TinTriangle& tri : removal.triangles) {
        const katana::geometry::Triangle2 plan{planOf(vertices[tri[0]]), planOf(vertices[tri[1]]),
                                               planOf(vertices[tri[2]])};
        const auto weights = plan.barycentric(at);
        if (!weights) {
            continue;
        }
        const double least = std::min({(*weights)[0], (*weights)[1], (*weights)[2]});
        if (least > bestWeight) {
            bestWeight = least;
            level = (*weights)[0] * vertices[tri[0]].z + (*weights)[1] * vertices[tri[1]].z +
                    (*weights)[2] * vertices[tri[2]].z;
        }
    }
    removal.change = std::abs(centre.z - level);

    // And where a new diagonal crosses an old spoke from v.
    for (std::size_t t = 0; t < removal.triangles.size(); ++t) {
        const TinTriangle& tri = removal.triangles[t];
        for (std::size_t k = 0; k < 3; ++k) {
            const std::uint32_t a = tri[k];
            const std::uint32_t b = tri[(k + 1) % 3];
            if (((removal.sides[t] >> k) & 1u) != 0 || a > b) {
                continue; // a side of the link, or a diagonal met from its other triangle
            }
            const Point2 pa = planOf(vertices[a]);
            const katana::geometry::Vec2 d1 = planOf(vertices[b]) - pa;
            for (const std::uint32_t r : ring) {
                if (r == a || r == b) {
                    continue;
                }
                const katana::geometry::Vec2 d2 = planOf(vertices[r]) - at;
                const double denominator = d1.cross(d2);
                if (denominator == 0.0) {
                    continue;
                }
                const double s = (at - pa).cross(d2) / denominator;
                const double u = (at - pa).cross(d1) / denominator;
                if (!(s > 0.0 && s < 1.0 && u > 0.0 && u < 1.0)) {
                    continue;
                }
                const double before = katana::math::lerp(centre.z, vertices[r].z, u);
                const double after = katana::math::lerp(vertices[a].z, vertices[b].z, s);
                removal.change = std::max(removal.change, std::abs(before - after));
            }
        }
    }
    return removal;
}

} // namespace

Result<TinEditResult> thinSurface(const TinSurface& surface, double tolerance,
                                  katana::core::TaskPool* pool)
{
    if (!(tolerance >= 0.0) || !std::isfinite(tolerance)) {
        return makeError(ErrorCode::InvalidArgument,
                         "the thinning tolerance must be a finite number, 0 or more",
                         "tolerance=" + katana::core::formatExactReal(tolerance));
    }
    if (surface.empty()) {
        return makeError(ErrorCode::InvalidArgument, "the surface is empty");
    }
    const auto& vertices = surface.vertices();
    const std::vector<std::uint8_t> held = heldVertices(surface);
    const detail::VertexStars stars(surface);
    std::vector<std::uint32_t> candidates;
    for (std::uint32_t v = 0; v < surface.vertexCount(); ++v) {
        if (held[v] == 0 && !stars.of(v).empty()) {
            candidates.push_back(v);
        }
    }

    // A difference of the length tolerance is no difference (math::lengthsEqual),
    // so a tolerance of 0 removes what changes nothing but the last bits.
    const double limit = tolerance + tol::kGeometric;

    // First measurement, on the original, in parallel: each candidate's answer
    // depends only on the surface, written by index, so any thread count
    // gives the same order.
    std::vector<double> first(candidates.size(), std::numeric_limits<double>::infinity());
    const auto measure = [&](std::size_t lo, std::size_t hi) {
        for (std::size_t i = lo; i < hi; ++i) {
            const std::uint32_t v = candidates[i];
            if (const auto removal = removalOf(v, vertices, surface.triangles(),
                                               surface.constrainedEdges(), stars.of(v))) {
                first[i] = removal->change;
            }
        }
    };
    katana::core::TaskPool& workers = pool != nullptr ? *pool : katana::core::TaskPool::shared();
    workers.parallelRanges(0, candidates.size(), kThinGrain, measure);

    std::vector<std::size_t> order;
    for (std::size_t i = 0; i < candidates.size(); ++i) {
        if (first[i] <= limit) {
            order.push_back(i);
        }
    }
    std::sort(order.begin(), order.end(), [&](std::size_t x, std::size_t y) {
        return first[x] != first[y] ? first[x] < first[y] : candidates[x] < candidates[y];
    });

    // Then one at a time, flattest first, on the surface as thinned so far.
    std::vector<TinTriangle> triangles = surface.triangles();
    std::vector<std::uint8_t> masks = surface.constrainedEdges();
    std::vector<std::uint8_t> alive(triangles.size(), 1);
    std::vector<double> bound(triangles.size(), 0.0);
    std::vector<std::vector<std::uint32_t>> around(surface.vertexCount());
    for (std::uint32_t v = 0; v < surface.vertexCount(); ++v) {
        const auto star = stars.of(v);
        around[v].assign(star.begin(), star.end());
    }
    std::size_t removed = 0;
    for (const std::size_t i : order) {
        const std::uint32_t v = candidates[i];
        const std::vector<std::uint32_t> star = around[v];
        const auto removal = removalOf(v, vertices, triangles, masks, star);
        if (!removal) {
            continue;
        }
        double prior = 0.0;
        for (const std::uint32_t t : star) {
            prior = std::max(prior, bound[t]);
        }
        if (prior + removal->change > limit) {
            continue;
        }
        for (const std::uint32_t t : star) {
            alive[t] = 0;
            for (const std::uint32_t x : triangles[t]) {
                if (x != v) {
                    auto& list = around[x];
                    list.erase(std::remove(list.begin(), list.end(), t), list.end());
                }
            }
        }
        around[v].clear();
        for (std::size_t n = 0; n < removal->triangles.size(); ++n) {
            const auto index = static_cast<std::uint32_t>(triangles.size());
            triangles.push_back(removal->triangles[n]);
            masks.push_back(removal->masks[n]);
            alive.push_back(1);
            bound.push_back(prior + removal->change);
            for (const std::uint32_t x : removal->triangles[n]) {
                around[x].push_back(index);
            }
        }
        ++removed;
    }

    std::vector<TinTriangle> kept;
    std::vector<std::uint8_t> keptMasks;
    double largest = 0.0;
    for (std::size_t t = 0; t < triangles.size(); ++t) {
        if (alive[t] != 0) {
            kept.push_back(triangles[t]);
            keptMasks.push_back(masks[t]);
            largest = std::max(largest, bound[t]);
        }
    }
    auto thinned = detail::finishSurface(vertices, std::move(kept), std::move(keptMasks));
    if (!thinned) {
        return thinned.error();
    }
    TinEditResult result;
    result.report = detail::reportOf(surface, *thinned);
    result.report.pointsRemoved = removed;
    result.report.largestChange = largest;
    result.surface = std::move(*thinned);
    return result;
}

} // namespace katana::terrain
