#include "katana/terrain/overlay.hpp"

#include <algorithm>
#include <cmath>
#include <compare>
#include <cstdint>
#include <map>
#include <set>
#include <string>
#include <unordered_map>
#include <utility>

#include "katana/core/task_pool.hpp"
#include "katana/geometry/spatial_index.hpp"
#include "katana/terrain/tin_builder.hpp"

#include "summation.hpp"

namespace katana::terrain {

namespace tol = katana::math::tolerance;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;
using katana::geometry::Polyline2;
using katana::geometry::SpatialEntry;
using katana::geometry::SpatialId;
using katana::geometry::SpatialIndex;
using katana::geometry::Triangle2;

// ---- the region ------------------------------------------------------------------------

Result<OverlayRegion> OverlayRegion::fromPolygon(const Polyline2& ring,
                                                 const std::vector<Polyline2>& holes)
{
    // A ring drawn back to its start repeats its first vertex at its end: a
    // closing segment of no length, which the triangulator would refuse as a
    // degenerate ring. It is the same ring without it.
    const auto opened = [](Polyline2 polygon) {
        while (polygon.vertices.size() > 1 &&
               polygon.vertices.front().distanceTo(polygon.vertices.back()) <= tol::kGeometric) {
            polygon.vertices.pop_back();
        }
        polygon.closed = true;
        return polygon;
    };
    OverlayRegion region;
    region.ring_ = opened(ring);
    for (const Polyline2& hole : holes) {
        region.holes_.push_back(opened(hole));
    }

    // The polygon's own vertices are the only points, so the triangulation
    // adds none and its triangles tile the polygon exactly. Heights do not
    // matter here; 0 for all, so a point given twice merges without a
    // conflict.
    TinInput input;
    for (const Point2& vertex : region.ring_.vertices) {
        input.points.emplace_back(vertex.x, vertex.y, 0.0);
    }
    for (const Polyline2& hole : region.holes_) {
        for (const Point2& vertex : hole.vertices) {
            input.points.emplace_back(vertex.x, vertex.y, 0.0);
        }
    }
    input.boundary = region.ring_;
    input.holes = region.holes_;
    auto built = buildTin(input);
    if (!built) {
        return makeError(ErrorCode::InvalidGeometry,
                         "the polygon cannot be used as an area: " + built.error().message,
                         built.error().context);
    }
    region.plan_ = std::move(built->surface);
    detail::CompensatedSum area;
    for (std::size_t t = 0; t < region.plan_.triangleCount(); ++t) {
        const Triangle2 triangle = region.plan_.planTriangle(t);
        if (!(triangle.signedArea() > 0.0)) {
            continue; // a sliver the exact triangulator kept; it covers nothing
        }
        region.triangles_.push_back(triangle);
        area.add(triangle.signedArea());
        region.bounds_.expand(triangle.boundingBox());
    }
    region.area_ = area.value();
    if (region.triangles_.empty()) {
        return makeError(ErrorCode::InvalidGeometry, "the polygon has no area");
    }
    return region;
}

bool OverlayRegion::contains(const Point2& position) const
{
    if (triangles_.empty() || !ring_.contains(position)) {
        return false;
    }
    return std::none_of(holes_.begin(), holes_.end(), [&](const Polyline2& hole) {
        return hole.classify(position) == katana::geometry::Containment::Inside;
    });
}

Result<TinSurface> OverlayRegion::atLevel(double level) const
{
    if (!std::isfinite(level)) {
        return makeError(ErrorCode::InvalidArgument, "the level must be finite");
    }
    std::vector<Point3> vertices = plan_.vertices();
    for (Point3& vertex : vertices) {
        vertex.z = level;
    }
    return TinSurface::create(std::move(vertices), plan_.triangles(), plan_.constrainedEdges());
}

// ---- the cut ---------------------------------------------------------------------------

namespace {

// Two buffers of twelve: a triangle clipped by a triangle has at most six
// corners, and three more clip edges (a region's triangle) add at most one
// each.
constexpr std::size_t kMaxCorners = 12;

// Which surface's triangulation a line of a piece lies on.
enum Family : std::uint8_t { kBaseFamily = 0, kTopFamily = 1, kRegionFamily = 2 };

// An edge of one family's triangulation, by its two vertex indices, lower first.
struct Line {
    std::uint8_t family = 0;
    std::uint32_t i = 0;
    std::uint32_t j = 0;

    friend bool operator==(const Line&, const Line&) = default;
    friend auto operator<=>(const Line&, const Line&) = default;
};

Line lineOf(std::uint8_t family, std::uint32_t a, std::uint32_t b)
{
    return Line{family, std::min(a, b), std::max(a, b)};
}

// A corner of the overlay named by WHAT it is, so two pieces cutting it from
// different triangles name it alike: a vertex of one family (`crossing`
// false; `a.family`, `a.i`), or the point where line `a` crosses line `b`
// (a < b).
struct Key {
    bool crossing = false;
    Line a{};
    Line b{};

    friend bool operator==(const Key&, const Key&) = default;
};

struct KeyHash {
    std::size_t operator()(const Key& key) const noexcept
    {
        // FNV-1a over the fields; a hash only spreads the keys, equality
        // decides.
        std::uint64_t hash = 1469598103934665603ULL;
        const auto mix = [&hash](std::uint64_t value) {
            hash ^= value;
            hash *= 1099511628211ULL;
        };
        mix(key.crossing ? 1U : 0U);
        mix(key.a.family);
        mix(key.a.i);
        mix(key.a.j);
        mix(key.b.family);
        mix(key.b.i);
        mix(key.b.j);
        return static_cast<std::size_t>(hash);
    }
};

Key vertexKey(std::uint8_t family, std::uint32_t index)
{
    Key key;
    key.a = Line{family, index, index};
    return key;
}

// A convex polygon being clipped: its corners and, when the corners are named
// (kLabels), what each is and the line each edge (corner k to k + 1) lies on.
template <bool kLabels>
struct Poly {
    std::array<Point2, kMaxCorners> p{};
    std::size_t n = 0;
    std::array<Key, kLabels ? kMaxCorners : 1> key{};
    std::array<Line, kLabels ? kMaxCorners : 1> edge{};
};

// A clip edge, directed so that the inside is on its left.
struct ClipEdge {
    Point2 from;
    Point2 to;
    Line line{};
};

// Where the families' vertices are, to place a crossing from its two lines.
struct Vertices {
    const TinSurface* base = nullptr;
    const TinSurface* top = nullptr;

    [[nodiscard]] Point2 at(std::uint8_t family, std::uint32_t index) const
    {
        const Point3& v =
            (family == kBaseFamily ? base : top)->vertices()[index];
        return Point2(v.x, v.y);
    }
};

// The crossing of polygon edge `edge` with clip line `line`, named and placed
// the same way whichever piece finds it. `interpolated` is the point the
// clipper computed along the polygon's edge, kept when the lines are too
// nearly parallel to place it any other way.
//
// Two edges of one triangulation meet only at a shared vertex, which is
// then the corner, exactly. Otherwise the lines are put in a fixed order and
// intersected from their own end points - never from the corners of the
// polygon being clipped, which differ from piece to piece - so the pieces on
// either side of an edge compute the same bits for the same crossing.
void nameCrossing(const Line& edge, const Line& line, const Vertices& vertices,
                  const Point2& interpolated, Key& key, Point2& point)
{
    if (edge.family == line.family) {
        std::uint32_t shared = kNoVertex;
        if (edge.i == line.i || edge.i == line.j) {
            shared = edge.i;
        } else if (edge.j == line.i || edge.j == line.j) {
            shared = edge.j;
        }
        if (shared != kNoVertex) {
            key = vertexKey(edge.family, shared);
            point = vertices.at(edge.family, shared);
            return;
        }
    }
    const auto [first, second] = edge < line ? std::pair(edge, line) : std::pair(line, edge);
    key.crossing = true;
    key.a = first;
    key.b = second;
    const Point2 a = vertices.at(first.family, first.i);
    const Point2 b = vertices.at(first.family, first.j);
    const Point2 c = vertices.at(second.family, second.i);
    const Point2 d = vertices.at(second.family, second.j);
    const double rx = b.x - a.x;
    const double ry = b.y - a.y;
    const double sx = d.x - c.x;
    const double sy = d.y - c.y;
    const double denominator = rx * sy - ry * sx;
    const double t = ((c.x - a.x) * sy - (c.y - a.y) * sx) / denominator;
    if (denominator != 0.0 && std::isfinite(t)) {
        point = Point2(a.x + t * rx, a.y + t * ry);
    } else {
        point = interpolated;
    }
}

// One Sutherland-Hodgman step: `in` clipped to the left of `clip`, into `out`.
//
// The inside test is `side >= 0`, not `> 0`, and that is load-bearing. A
// corner lying exactly on a clip edge - the normal case where two triangles
// share an edge or a vertex - has a cross product of exactly zero in IEEE
// arithmetic, because it reduces to `a - a` for a point that defines the
// edge. Counting it inside keeps it bit for bit instead of recomputing it, so
// two triangles sharing an edge clip to exactly that segment, whose fan has
// signed area exactly 0.0 and is rejected - and a surface compared with
// itself is exactly zero (docs/terrain.md, "The fix").
template <bool kLabels>
void clipStep(const Poly<kLabels>& in, const ClipEdge& clip, const Vertices& vertices,
              Poly<kLabels>& out)
{
    const double ex = clip.to.x - clip.from.x;
    const double ey = clip.to.y - clip.from.y;
    const auto side = [&](const Point2& v) {
        return ex * (v.y - clip.from.y) - ey * (v.x - clip.from.x);
    };
    out.n = 0;
    for (std::size_t i = 0; i < in.n && out.n < kMaxCorners; ++i) {
        const Point2& current = in.p[i];
        const Point2& next = in.p[(i + 1) % in.n];
        const double sideCurrent = side(current);
        const double sideNext = side(next);
        if (sideCurrent >= 0.0) {
            out.p[out.n] = current;
            if constexpr (kLabels) {
                out.key[out.n] = in.key[i];
                // A corner on the clip line whose next corner is cut away
                // runs along the clip line to the next corner kept.
                out.edge[out.n] = sideCurrent == 0.0 && sideNext < 0.0 ? clip.line : in.edge[i];
            }
            ++out.n;
        }
        // A crossing only when the two strictly straddle the line: an end
        // exactly on it has been kept (or will be), and a point constructed
        // here would duplicate it.
        if (out.n < kMaxCorners &&
            ((sideCurrent > 0.0 && sideNext < 0.0) || (sideCurrent < 0.0 && sideNext > 0.0))) {
            const double t = sideCurrent / (sideCurrent - sideNext);
            const Point2 interpolated(current.x + t * (next.x - current.x),
                                      current.y + t * (next.y - current.y));
            if constexpr (kLabels) {
                nameCrossing(in.edge[i], clip.line, vertices, interpolated, out.key[out.n],
                             out.p[out.n]);
                // Leaving: the edge from here runs along the clip line.
                // Entering: along the polygon's edge to `next`.
                out.edge[out.n] = sideCurrent > 0.0 ? clip.line : in.edge[i];
            } else {
                (void)vertices;
                out.p[out.n] = interpolated;
            }
            ++out.n;
        }
    }
}

// `subject` clipped by the three edges of a counter-clockwise triangle, into
// `out`; fewer than three corners left means they meet in a segment at most.
template <bool kLabels>
void clipByTriangle(const Poly<kLabels>& subject, const std::array<ClipEdge, 3>& window,
                    const Vertices& vertices, Poly<kLabels>& scratch, Poly<kLabels>& out)
{
    const Poly<kLabels>* from = &subject;
    Poly<kLabels>* buffers[2] = {&out, &scratch};
    // Three steps alternate between the two buffers and end in `out`:
    // subject -> scratch -> out -> ... is arranged so the last write is out.
    Poly<kLabels>* order[3] = {buffers[0], buffers[1], buffers[0]};
    for (std::size_t edge = 0; edge < 3; ++edge) {
        clipStep(*from, window[edge], vertices, *order[edge]);
        from = order[edge];
        if (from->n < 3) {
            out.n = 0;
            return;
        }
    }
}

// The triangle's corners counter-clockwise, with their vertex indices. The
// clipper needs a known winding to know which side of an edge is inside, and
// a stored triangle's winding is whatever made it.
struct OrientedTriangle {
    std::array<Point2, 3> corners;
    std::array<std::uint32_t, 3> vertices;
};

OrientedTriangle oriented(const TinSurface& surface, std::size_t triangle)
{
    const TinTriangle& tri = surface.triangles()[triangle];
    OrientedTriangle out;
    for (std::size_t k = 0; k < 3; ++k) {
        const Point3& v = surface.vertices()[tri[k]];
        out.corners[k] = Point2(v.x, v.y);
        out.vertices[k] = tri[k];
    }
    if (Triangle2{out.corners[0], out.corners[1], out.corners[2]}.signedArea() < 0.0) {
        std::swap(out.corners[1], out.corners[2]);
        std::swap(out.vertices[1], out.vertices[2]);
    }
    return out;
}

std::array<ClipEdge, 3> windowOf(const OrientedTriangle& triangle, std::uint8_t family)
{
    std::array<ClipEdge, 3> edges{};
    for (std::size_t k = 0; k < 3; ++k) {
        const std::size_t next = (k + 1) % 3;
        edges[k] = ClipEdge{triangle.corners[k], triangle.corners[next],
                            lineOf(family, triangle.vertices[k], triangle.vertices[next])};
    }
    return edges;
}

std::array<ClipEdge, 3> windowOf(const Triangle2& triangle)
{
    const std::array<Point2, 3> corners{triangle.a, triangle.b, triangle.c};
    std::array<ClipEdge, 3> edges{};
    for (std::size_t k = 0; k < 3; ++k) {
        // A region's edges never name a crossing (the region is cut only on
        // the unlabelled path), so their line is left blank.
        edges[k] = ClipEdge{corners[k], corners[(k + 1) % 3], Line{kRegionFamily, 0, 0}};
    }
    return edges;
}

// The plane of `triangle` at three positions inside it. The weights are exact
// at the triangle's own vertices (1, 0, 0), which is what makes the comparison
// of identical surfaces exactly zero. The plan triangle and its doubled area
// are the caller's loop invariants (Triangle2::barycentric).
std::array<double, 3> planeElevations(const TinSurface& surface, std::uint32_t triangle,
                                      const Triangle2& plan, double twiceArea,
                                      const std::array<Point2, 3>& positions)
{
    const TinTriangle& tri = surface.triangles()[triangle];
    std::array<double, 3> elevations{};
    for (std::size_t i = 0; i < 3; ++i) {
        const std::array<double, 3> weights = plan.barycentric(positions[i], twiceArea);
        elevations[i] = weights[0] * surface.vertices()[tri[0]].z +
                        weights[1] * surface.vertices()[tri[1]].z +
                        weights[2] * surface.vertices()[tri[2]].z;
    }
    return elevations;
}

Box2 boxOf(const std::array<Point2, kMaxCorners>& points, std::size_t count)
{
    Box2 box;
    for (std::size_t i = 0; i < count; ++i) {
        box.expand(points[i]);
    }
    return box;
}

// Every piece, block by block, handed to `emit(block, piece, keys)`; the keys
// are meaningful only with kLabels. The one loop both the visitor and the
// surface builders run.
template <bool kLabels, class Emit>
void cutOverlay(const TinSurface& base, const TinSurface& top, const OverlayRegion* region,
                katana::core::TaskPool* pool, Emit& emit)
{
    if (!base.bounds().intersects(top.bounds())) {
        return;
    }
    Box2 common(Point2(std::max(base.bounds().min.x, top.bounds().min.x),
                       std::max(base.bounds().min.y, top.bounds().min.y)),
                Point2(std::min(base.bounds().max.x, top.bounds().max.x),
                       std::min(base.bounds().max.y, top.bounds().max.y)));
    if (region != nullptr) {
        if (region->empty() || !common.intersects(region->bounds())) {
            return;
        }
        common = Box2(Point2(std::max(common.min.x, region->bounds().min.x),
                             std::max(common.min.y, region->bounds().min.y)),
                      Point2(std::min(common.max.x, region->bounds().max.x),
                             std::min(common.max.y, region->bounds().max.y)));
    }
    const Box2 area = common.inflated(tol::kGeometric);

    // Broad phase over the top triangles that reach the common area: only
    // these can contribute, and only they are worth indexing.
    std::vector<SpatialEntry> entries;
    entries.reserve(top.triangleCount());
    for (std::size_t t = 0; t < top.triangleCount(); ++t) {
        const Box2 box = top.planTriangle(t).boundingBox();
        if (box.intersects(area)) {
            entries.push_back({static_cast<SpatialId>(t), box});
        }
    }
    if (entries.empty()) {
        return;
    }
    SpatialIndex index;
    index.rebuild(entries);
    SpatialIndex regionIndex;
    if (region != nullptr) {
        std::vector<SpatialEntry> regionEntries;
        regionEntries.reserve(region->triangles().size());
        for (std::size_t t = 0; t < region->triangles().size(); ++t) {
            regionEntries.push_back(
                {static_cast<SpatialId>(t), region->triangles()[t].boundingBox()});
        }
        regionIndex.rebuild(regionEntries);
    }

    const Vertices vertices{&base, &top};
    const std::size_t baseCount = base.triangleCount();
    const std::size_t blockCount = overlayBlockCount(base);

    const auto cutBlocks = [&](std::size_t firstBlock, std::size_t endBlock) {
        // Per call, not shared: the queries and the clipper each need a
        // buffer, and a chunk of blocks runs on one thread.
        std::vector<SpatialId> candidates;
        std::vector<SpatialId> regionCandidates;
        Poly<kLabels> subject;
        Poly<kLabels> scratch;
        Poly<kLabels> clipped;
        Poly<kLabels> inside;
        std::array<Key, 3> keys{};
        for (std::size_t block = firstBlock; block < endBlock; ++block) {
            const std::size_t first = block * kOverlayBlockTriangles;
            const std::size_t end = std::min(first + kOverlayBlockTriangles, baseCount);
            for (std::size_t tb = first; tb < end; ++tb) {
                const Triangle2 planBase = base.planTriangle(tb);
                const Box2 boxBase = planBase.boundingBox();
                if (!boxBase.intersects(area)) {
                    continue;
                }
                // A degenerate triangle has no plane to read a height off, so
                // every piece cut from it would be dropped one at a time.
                if (planBase.isDegenerate()) {
                    continue;
                }
                const double twiceAreaBase = planBase.twiceSignedArea();
                const OrientedTriangle orientedBase = oriented(base, tb);
                subject.n = 3;
                for (std::size_t k = 0; k < 3; ++k) {
                    subject.p[k] = orientedBase.corners[k];
                    if constexpr (kLabels) {
                        subject.key[k] = vertexKey(kBaseFamily, orientedBase.vertices[k]);
                        subject.edge[k] = lineOf(kBaseFamily, orientedBase.vertices[k],
                                                 orientedBase.vertices[(k + 1) % 3]);
                    }
                }
                index.query(boxBase, candidates);
                for (const SpatialId id : candidates) {
                    const auto tt = static_cast<std::uint32_t>(id);
                    const Triangle2 planTop = top.planTriangle(tt);
                    if (planTop.isDegenerate()) {
                        continue;
                    }
                    const double twiceAreaTop = planTop.twiceSignedArea();
                    clipByTriangle(subject, windowOf(oriented(top, tt), kTopFamily), vertices,
                                   scratch, clipped);
                    if (clipped.n < 3) {
                        continue;
                    }
                    // A convex polygon, so a fan from its first corner covers
                    // it exactly once with no corner not already on it.
                    const auto fan = [&](const Poly<kLabels>& polygon) {
                        for (std::size_t i = 1; i + 1 < polygon.n; ++i) {
                            OverlayPiece piece;
                            piece.corners = {polygon.p[0], polygon.p[i], polygon.p[i + 1]};
                            piece.area = Triangle2{piece.corners[0], piece.corners[1],
                                                   piece.corners[2]}
                                             .signedArea();
                            if (!(piece.area > 0.0)) {
                                continue;
                            }
                            // Inside both triangles by construction, so both
                            // planes are read with weights in [0, 1] and
                            // neither surface is searched.
                            piece.zBase = planeElevations(base, static_cast<std::uint32_t>(tb),
                                                          planBase, twiceAreaBase, piece.corners);
                            piece.zTop =
                                planeElevations(top, tt, planTop, twiceAreaTop, piece.corners);
                            piece.baseTriangle = static_cast<std::uint32_t>(tb);
                            piece.topTriangle = tt;
                            if constexpr (kLabels) {
                                keys = {polygon.key[0], polygon.key[i], polygon.key[i + 1]};
                            }
                            emit(block, piece, keys);
                        }
                    };
                    if (region == nullptr) {
                        fan(clipped);
                        continue;
                    }
                    regionIndex.query(boxOf(clipped.p, clipped.n), regionCandidates);
                    for (const SpatialId r : regionCandidates) {
                        clipByTriangle(clipped,
                                       windowOf(region->triangles()[static_cast<std::size_t>(r)]),
                                       vertices, scratch, inside);
                        if (inside.n >= 3) {
                            fan(inside);
                        }
                    }
                }
            }
        }
    };
    // Everything the blocks read is immutable here, and each block's pieces
    // go only to that block's slot.
    katana::core::TaskPool& workers = pool != nullptr ? *pool : katana::core::TaskPool::shared();
    workers.parallelRanges(0, blockCount, 1, cutBlocks);
}

Status checkSurfaces(const TinSurface& base, const TinSurface& top)
{
    if (base.empty() || top.empty()) {
        return makeError(ErrorCode::InvalidArgument, "both surfaces must have triangles");
    }
    return {};
}

} // namespace

std::size_t overlayBlockCount(const TinSurface& base)
{
    return (base.triangleCount() + kOverlayBlockTriangles - 1) / kOverlayBlockTriangles;
}

Status visitOverlay(const TinSurface& base, const TinSurface& top, OverlayVisitor& visitor,
                    const OverlayRegion* region, katana::core::TaskPool* pool)
{
    if (auto status = checkSurfaces(base, top); !status) {
        return status;
    }
    const auto emit = [&visitor](std::size_t block, const OverlayPiece& piece,
                                 const std::array<Key, 3>&) { visitor.piece(block, piece); };
    cutOverlay<false>(base, top, region, pool, emit);
    return {};
}

// ---- surfaces made of the overlay --------------------------------------------------------

namespace {

struct NamedPiece {
    OverlayPiece piece;
    std::array<Key, 3> keys;
};

// The overlay's pieces joined into one mesh: a vertex per distinct corner, in
// the order the pieces first name it (block, then piece order: the same at any
// thread count), with both surfaces' heights there as that first piece read
// them.
struct OverlayMesh {
    std::vector<Point2> positions;
    std::vector<double> zBase;
    std::vector<double> zTop;
    std::vector<TinTriangle> triangles;
    std::size_t pieces = 0;
};

OverlayMesh joinPieces(const TinSurface& base, const TinSurface& top,
                       katana::core::TaskPool* pool)
{
    std::vector<std::vector<NamedPiece>> blocks(overlayBlockCount(base));
    const auto emit = [&blocks](std::size_t block, const OverlayPiece& piece,
                                const std::array<Key, 3>& keys) {
        blocks[block].push_back(NamedPiece{piece, keys});
    };
    cutOverlay<true>(base, top, nullptr, pool, emit);

    OverlayMesh mesh;
    std::unordered_map<Key, std::uint32_t, KeyHash> byKey;
    for (const std::vector<NamedPiece>& block : blocks) {
        for (const NamedPiece& named : block) {
            TinTriangle triangle{};
            for (std::size_t c = 0; c < 3; ++c) {
                const auto [found, added] =
                    byKey.try_emplace(named.keys[c], static_cast<std::uint32_t>(mesh.positions.size()));
                if (added) {
                    mesh.positions.push_back(named.piece.corners[c]);
                    mesh.zBase.push_back(named.piece.zBase[c]);
                    mesh.zTop.push_back(named.piece.zTop[c]);
                }
                triangle[c] = found->second;
            }
            mesh.triangles.push_back(triangle);
            ++mesh.pieces;
        }
    }

    // Two names for one place - a vertex of the base where the top has one
    // too, which is common where a design was built on the ground's own
    // points - are one vertex: the first.
    std::vector<std::uint32_t> order(mesh.positions.size());
    for (std::uint32_t i = 0; i < order.size(); ++i) {
        order[i] = i;
    }
    std::sort(order.begin(), order.end(), [&](std::uint32_t a, std::uint32_t b) {
        const Point2& pa = mesh.positions[a];
        const Point2& pb = mesh.positions[b];
        if (pa.x != pb.x) {
            return pa.x < pb.x;
        }
        if (pa.y != pb.y) {
            return pa.y < pb.y;
        }
        return a < b;
    });
    std::vector<std::uint32_t> same(mesh.positions.size());
    for (std::size_t i = 0; i < order.size();) {
        std::size_t j = i;
        while (j < order.size() && mesh.positions[order[j]].x == mesh.positions[order[i]].x &&
               mesh.positions[order[j]].y == mesh.positions[order[i]].y) {
            same[order[j]] = order[i]; // the lowest index of the run, by the sort
            ++j;
        }
        i = j;
    }
    for (TinTriangle& triangle : mesh.triangles) {
        for (std::uint32_t& v : triangle) {
            v = same[v];
        }
    }
    return mesh;
}

// The vertices `triangles` use, renumbered densely in order of first use, and
// the triangles that still have three distinct corners and an area.
struct CompactMesh {
    std::vector<Point3> vertices;
    std::vector<TinTriangle> triangles;
};

CompactMesh compact(const std::vector<Point3>& vertices, const std::vector<TinTriangle>& triangles)
{
    CompactMesh mesh;
    std::vector<std::uint32_t> renumber(vertices.size(), kNoVertex);
    for (const TinTriangle& triangle : triangles) {
        if (triangle[0] == triangle[1] || triangle[1] == triangle[2] ||
            triangle[0] == triangle[2]) {
            continue;
        }
        const auto plan = [&](std::size_t c) {
            return Point2(vertices[triangle[c]].x, vertices[triangle[c]].y);
        };
        if (!(Triangle2{plan(0), plan(1), plan(2)}.twiceSignedArea() > 0.0)) {
            continue;
        }
        TinTriangle out{};
        for (std::size_t c = 0; c < 3; ++c) {
            std::uint32_t& to = renumber[triangle[c]];
            if (to == kNoVertex) {
                to = static_cast<std::uint32_t>(mesh.vertices.size());
                mesh.vertices.push_back(vertices[triangle[c]]);
            }
            out[c] = to;
        }
        mesh.triangles.push_back(out);
    }
    return mesh;
}

Result<OverlaySurface> finishSurface(const std::vector<Point2>& positions,
                                     const std::vector<double>& heights,
                                     const std::vector<TinTriangle>& triangles,
                                     std::size_t pieces, bool retriangulate)
{
    std::vector<Point3> all;
    all.reserve(positions.size());
    for (std::size_t v = 0; v < positions.size(); ++v) {
        all.emplace_back(positions[v].x, positions[v].y, heights[v]);
    }
    CompactMesh joined = compact(all, triangles);
    const std::vector<Point3>& vertices = joined.vertices;
    const std::vector<TinTriangle>& kept = joined.triangles;
    if (kept.empty()) {
        return makeError(ErrorCode::InvalidGeometry, "the surfaces have no ground in common");
    }

    OverlaySurface result;
    result.pieces = pieces;
    if (!retriangulate) {
        auto direct = TinSurface::create(vertices, kept);
        if (direct) {
            result.surface = std::move(*direct);
            return result;
        }
    }

    // The fallback: the same vertices and every edge of every piece given to
    // the constrained triangulation, whose exact predicates settle a sliver
    // that floating point left two pieces sharing. Every edge is a breakline,
    // so every triangle it makes lies inside one piece, where the height is
    // linear; it triangulates the hull, so the triangles outside every piece
    // are left out again.
    TinInput input;
    input.points = vertices;
    std::set<std::pair<std::uint32_t, std::uint32_t>> edges;
    for (const TinTriangle& triangle : kept) {
        for (std::size_t k = 0; k < 3; ++k) {
            const std::uint32_t a = triangle[k];
            const std::uint32_t b = triangle[(k + 1) % 3];
            edges.emplace(std::min(a, b), std::max(a, b));
        }
    }
    for (const auto& [from, to] : edges) {
        input.breaklines.push_back(Breakline{{vertices[from], vertices[to]}, false});
    }
    TinBuildOptions options;
    options.duplicatePoints = DuplicatePointPolicy::Average;
    options.crossingBreaklines = CrossingBreaklinePolicy::Average;
    auto built = buildTin(input, options);
    if (!built) {
        return built.error();
    }
    const TinSurface& hull = built->surface;
    std::vector<SpatialEntry> entries;
    entries.reserve(kept.size());
    for (std::size_t t = 0; t < kept.size(); ++t) {
        const Triangle2 plan{Point2(vertices[kept[t][0]].x, vertices[kept[t][0]].y),
                             Point2(vertices[kept[t][1]].x, vertices[kept[t][1]].y),
                             Point2(vertices[kept[t][2]].x, vertices[kept[t][2]].y)};
        entries.push_back({static_cast<SpatialId>(t), plan.boundingBox()});
    }
    SpatialIndex index;
    index.rebuild(entries);
    std::vector<SpatialId> candidates;
    std::vector<TinTriangle> inside;
    for (std::size_t t = 0; t < hull.triangleCount(); ++t) {
        const Point2 centroid = hull.planTriangle(t).centroid();
        index.query(Box2(centroid, centroid), candidates);
        const bool isCovered = std::any_of(candidates.begin(), candidates.end(), [&](SpatialId id) {
            const TinTriangle& piece = kept[static_cast<std::size_t>(id)];
            return Triangle2{Point2(vertices[piece[0]].x, vertices[piece[0]].y),
                             Point2(vertices[piece[1]].x, vertices[piece[1]].y),
                             Point2(vertices[piece[2]].x, vertices[piece[2]].y)}
                .contains(centroid);
        });
        if (isCovered) {
            inside.push_back(hull.triangles()[t]);
        }
    }
    CompactMesh covered = compact(hull.vertices(), inside);
    if (covered.triangles.empty()) {
        return makeError(ErrorCode::InvalidGeometry, "the surfaces have no ground in common");
    }
    auto rebuilt = TinSurface::create(std::move(covered.vertices), std::move(covered.triangles));
    if (!rebuilt) {
        return rebuilt.error();
    }
    result.surface = std::move(*rebuilt);
    result.retriangulated = true;
    return result;
}

} // namespace

Result<OverlaySurface> differenceSurface(const TinSurface& base, const TinSurface& top,
                                         const OverlaySurfaceOptions& options)
{
    if (auto status = checkSurfaces(base, top); !status) {
        return status.error();
    }
    const OverlayMesh mesh = joinPieces(base, top, options.pool);
    if (mesh.triangles.empty()) {
        return makeError(ErrorCode::InvalidGeometry, "the surfaces have no ground in common");
    }
    std::vector<double> heights(mesh.positions.size());
    for (std::size_t v = 0; v < heights.size(); ++v) {
        heights[v] = mesh.zTop[v] - mesh.zBase[v];
    }
    return finishSurface(mesh.positions, heights, mesh.triangles, mesh.pieces,
                         options.retriangulate);
}

Result<OverlaySurface> envelopeSurface(const TinSurface& first, const TinSurface& second,
                                       Envelope which, const OverlaySurfaceOptions& options)
{
    if (auto status = checkSurfaces(first, second); !status) {
        return status.error();
    }
    const OverlayMesh mesh = joinPieces(first, second, options.pool);
    if (mesh.triangles.empty()) {
        return makeError(ErrorCode::InvalidGeometry, "the surfaces have no ground in common");
    }
    std::vector<Point2> positions = mesh.positions;
    std::vector<double> heights(positions.size());
    std::vector<double> difference(positions.size());
    for (std::size_t v = 0; v < positions.size(); ++v) {
        difference[v] = mesh.zTop[v] - mesh.zBase[v];
        heights[v] = which == Envelope::Upper ? std::max(mesh.zBase[v], mesh.zTop[v])
                                              : std::min(mesh.zBase[v], mesh.zTop[v]);
    }

    // Where the two meet on an edge whose ends lie on opposite sides: one
    // vertex per edge, placed from the edge's ends in a fixed order (the lower
    // index first), so both pieces that share the edge share the point.
    std::map<std::pair<std::uint32_t, std::uint32_t>, std::uint32_t> meetings;
    const auto meet = [&](std::uint32_t u, std::uint32_t v) -> std::uint32_t {
        if (difference[u] == 0.0) {
            return u;
        }
        if (difference[v] == 0.0) {
            return v;
        }
        const std::uint32_t low = std::min(u, v);
        const std::uint32_t high = std::max(u, v);
        const auto found = meetings.find({low, high});
        if (found != meetings.end()) {
            return found->second;
        }
        const double t = difference[low] / (difference[low] - difference[high]);
        const Point2& a = positions[low];
        const Point2& b = positions[high];
        positions.emplace_back(a.x + t * (b.x - a.x), a.y + t * (b.y - a.y));
        // Both surfaces have this height there; the second's is read.
        heights.push_back(mesh.zTop[low] + t * (mesh.zTop[high] - mesh.zTop[low]));
        difference.push_back(0.0);
        const auto index = static_cast<std::uint32_t>(positions.size() - 1);
        meetings.emplace(std::pair(low, high), index);
        return index;
    };

    std::vector<TinTriangle> triangles;
    triangles.reserve(mesh.triangles.size());
    for (const TinTriangle& triangle : mesh.triangles) {
        const std::array<double, 3> d{difference[triangle[0]], difference[triangle[1]],
                                      difference[triangle[2]]};
        const int positives = (d[0] > 0.0) + (d[1] > 0.0) + (d[2] > 0.0);
        const int negatives = (d[0] < 0.0) + (d[1] < 0.0) + (d[2] < 0.0);
        if (positives == 0 || negatives == 0) {
            triangles.push_back(triangle);
            continue;
        }
        // The two cross inside: the lone vertex is the only positive or the
        // only negative one. Rotating it to the front keeps the winding.
        const bool lonePositive = positives == 1;
        std::size_t p = 0;
        for (std::size_t i = 0; i < 3; ++i) {
            if (lonePositive ? d[i] > 0.0 : d[i] < 0.0) {
                p = i;
            }
        }
        const std::uint32_t vp = triangle[p];
        const std::uint32_t vq = triangle[(p + 1) % 3];
        const std::uint32_t vr = triangle[(p + 2) % 3];
        const std::uint32_t mq = meet(vp, vq);
        const std::uint32_t mr = meet(vp, vr);
        // The tip on the lone vertex's side, and the rest split in two; a
        // part that collapses (a corner exactly where the two meet) names a
        // vertex twice and is dropped when the surface is finished.
        triangles.push_back({vp, mq, mr});
        triangles.push_back({mq, vq, vr});
        triangles.push_back({mq, vr, mr});
    }
    return finishSurface(positions, heights, triangles, mesh.pieces, options.retriangulate);
}

} // namespace katana::terrain
