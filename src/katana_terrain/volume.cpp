#include "katana/terrain/volume.hpp"

#include "katana/core/task_pool.hpp"
#include "katana/geometry/spatial_index.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <cstdint>
#include <optional>
#include <vector>

#include "summation.hpp"

namespace katana::terrain {

namespace tol = katana::math::tolerance;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::geometry::Triangle2;

namespace {

// Integral of a function that is linear over a triangle, positive and negative
// part kept apart.
//
// With vertex values f0, f1, f2 and plan area A the integral is A * (f0+f1+f2)/3.
// When the signs differ, exactly one vertex p stands alone against q and r. The
// zero line meets edges pq and pr at the fractions tq = fp/(fp-fq) and
// tr = fp/(fp-fr), cutting off the triangle (p, mq, mr) of area A*tq*tr with
// values (fp, 0, 0). The rest is the quadrilateral (mq, q, r, mr), split into the
// triangles (mq, q, r) of area A*(1-tq) and (mq, r, mr) of area A*tq*(1-tr).
// Every term of a part has the same sign, so nothing cancels, and no coordinates
// are constructed.
class SignedIntegral {
  public:
    void add(double area, const std::array<double, 3>& f)
    {
        if (!(area > 0.0)) {
            return;
        }
        const int positives = (f[0] > 0.0) + (f[1] > 0.0) + (f[2] > 0.0);
        const int negatives = (f[0] < 0.0) + (f[1] < 0.0) + (f[2] < 0.0);
        if (negatives == 0 || positives == 0) {
            const double volume = area * ((f[0] + f[1] + f[2]) / 3.0);
            if (positives != 0) {
                positive_.add(volume);
                positiveArea_.add(area);
            } else if (negatives != 0) {
                negative_.add(-volume);
                negativeArea_.add(area);
            }
            return;
        }

        // Mixed signs: the lone vertex is the only positive or the only negative one.
        const bool lonePositive = positives == 1;
        std::size_t p = 0;
        for (std::size_t i = 0; i < 3; ++i) {
            if (lonePositive ? f[i] > 0.0 : f[i] < 0.0) {
                p = i;
            }
        }
        const double fp = std::abs(f[p]);
        const double fq = std::abs(f[(p + 1) % 3]); // the others lie on the opposite
        const double fr = std::abs(f[(p + 2) % 3]); // side or on the zero line
        const double tq = fp / (fp + fq);
        const double tr = fp / (fp + fr);
        const double tipArea = area * tq * tr;
        const double tipVolume = tipArea * fp / 3.0;
        const double restVolume =
            area * (1.0 - tq) * (fq + fr) / 3.0 + area * tq * (1.0 - tr) * fr / 3.0;
        (lonePositive ? positive_ : negative_).add(tipVolume);
        (lonePositive ? positiveArea_ : negativeArea_).add(tipArea);
        (lonePositive ? negative_ : positive_).add(restVolume);
        (lonePositive ? negativeArea_ : positiveArea_).add(area - tipArea);
    }

    // Folds another integral's four sums into this one's. Used to combine the
    // per-block integrals of compareSurfaces in block order; each block's sum
    // arrives as its compensated value, so the combination is itself a
    // compensated sum of a fixed sequence of terms.
    void merge(const SignedIntegral& other)
    {
        positive_.add(other.positive());
        negative_.add(other.negative());
        positiveArea_.add(other.positiveArea());
        negativeArea_.add(other.negativeArea());
    }

    [[nodiscard]] double positive() const { return positive_.value(); }
    [[nodiscard]] double negative() const { return negative_.value(); } // as a magnitude
    [[nodiscard]] double positiveArea() const { return positiveArea_.value(); }
    [[nodiscard]] double negativeArea() const { return negativeArea_.value(); }

  private:
    detail::CompensatedSum positive_;
    detail::CompensatedSum negative_;
    detail::CompensatedSum positiveArea_;
    detail::CompensatedSum negativeArea_;
};

// A triangle's corners counter-clockwise. The clipper below needs a known
// winding to decide which side of a clip edge is inside, and a TIN triangle's
// stored order is whatever the triangulation produced.
std::array<Point2, 3> orientedCorners(const Triangle2& triangle)
{
    std::array<Point2, 3> corners{triangle.a, triangle.b, triangle.c};
    if (triangle.signedArea() < 0.0) {
        std::swap(corners[1], corners[2]);
    }
    return corners;
}

// Sutherland-Hodgman clip of one triangle by another. Both are convex, so the
// intersection is convex with at most six corners; `out` holds them counter-
// clockwise and the count is returned. Fewer than three means the triangles
// meet in at most a segment and contribute no area.
//
// WHY THIS IS EXACT WHERE IT MATTERS. The inside test is `cross >= 0`, not
// `> 0`. For a subject corner lying exactly on a clip edge - which is the
// normal case where two triangles share an edge or a vertex - the cross
// product is exactly zero in IEEE arithmetic, because it evaluates to a
// quantity of the form `a - a` when the point is one of the two that define
// the edge. Counting it as inside means no crossing point is constructed for
// it, so the corner is reproduced bit for bit rather than recomputed. Two
// triangles that share an edge therefore clip to exactly that segment, whose
// fan has signed area exactly 0.0 and is rejected by the caller's `area > 0`
// guard. With `> 0` the same case would construct a near-degenerate polygon of
// some tiny arbitrary area, and comparing a surface with itself would no
// longer be exactly zero.
std::size_t clipTriangleToTriangle(const std::array<Point2, 3>& subject,
                                   const std::array<Point2, 3>& window,
                                   std::array<Point2, 8>& out)
{
    // Two buffers of eight: each clip edge can add at most one corner, and
    // three edges applied to a triangle cannot exceed six.
    std::array<Point2, 8> buffer{};
    std::size_t count = 3;
    for (std::size_t i = 0; i < 3; ++i) {
        out[i] = subject[i];
    }

    for (std::size_t edge = 0; edge < 3 && count >= 3; ++edge) {
        const Point2& p = window[edge];
        const Point2& q = window[(edge + 1) % 3];
        const double ex = q.x - p.x;
        const double ey = q.y - p.y;
        const auto side = [&](const Point2& v) { return ex * (v.y - p.y) - ey * (v.x - p.x); };

        std::size_t produced = 0;
        for (std::size_t i = 0; i < count; ++i) {
            const Point2& current = out[i];
            const Point2& next = out[(i + 1) % count];
            const double sideCurrent = side(current);
            const double sideNext = side(next);
            if (sideCurrent >= 0.0) {
                buffer[produced++] = current;
            }
            // A crossing only when the two strictly straddle the line. When
            // either is exactly on it that endpoint has already been kept (or
            // will be), so constructing a point here would duplicate it.
            if ((sideCurrent > 0.0 && sideNext < 0.0) || (sideCurrent < 0.0 && sideNext > 0.0)) {
                const double t = sideCurrent / (sideCurrent - sideNext);
                buffer[produced++] = Point2(current.x + t * (next.x - current.x),
                                            current.y + t * (next.y - current.y));
            }
            if (produced >= buffer.size()) {
                break;
            }
        }
        count = produced;
        for (std::size_t i = 0; i < count; ++i) {
            out[i] = buffer[i];
        }
    }
    return count < 3 ? 0 : count;
}

// Elevation of the plane of `triangle` at the three given positions. The
// weights are exact at the triangle's own vertices (1, 0, 0), which is what makes
// the comparison of identical surfaces exactly zero.
//
// The plan triangle and its doubled area are passed in rather than looked up:
// compareSurfaces evaluates a fan of pieces against the same pair of surface
// triangles, so planTriangle() and the degeneracy test (three hypots and an
// area, inside Triangle2::barycentric) are loop invariants. The caller asks
// isDegenerate() once per surface triangle and skips it entirely when it says
// yes, which is what the nullopt return used to do a piece at a time.
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

} // namespace

Result<DatumVolume> volumeToDatum(const TinSurface& surface, double datum)
{
    if (!std::isfinite(datum)) {
        return makeError(ErrorCode::InvalidArgument, "datum elevation must be finite");
    }
    if (surface.empty()) {
        return makeError(ErrorCode::InvalidArgument, "surface has no triangles");
    }
    SignedIntegral integral;
    for (std::size_t t = 0; t < surface.triangleCount(); ++t) {
        const TinTriangle& tri = surface.triangles()[t];
        // Heights above the datum first: subtracting the datum from the summed
        // prism volume instead would cancel most digits for elevations far from 0.
        integral.add(surface.trianglePlanArea(t), {surface.vertices()[tri[0]].z - datum,
                                                   surface.vertices()[tri[1]].z - datum,
                                                   surface.vertices()[tri[2]].z - datum});
    }
    DatumVolume result;
    result.above = integral.positive();
    result.below = integral.negative();
    result.net = result.above - result.below;
    result.planAreaAbove = integral.positiveArea();
    result.planAreaBelow = integral.negativeArea();
    return result;
}

Result<SurfaceComparison> compareSurfaces(const TinSurface& existing, const TinSurface& design,
                                          katana::core::TaskPool* pool)
{
    if (existing.empty() || design.empty()) {
        return makeError(ErrorCode::InvalidArgument, "both surfaces must have triangles");
    }
    SurfaceComparison result;
    if (!existing.bounds().intersects(design.bounds())) {
        return result;
    }
    const Box2 common(Point2(std::max(existing.bounds().min.x, design.bounds().min.x),
                             std::max(existing.bounds().min.y, design.bounds().min.y)),
                      Point2(std::min(existing.bounds().max.x, design.bounds().max.x),
                             std::min(existing.bounds().max.y, design.bounds().max.y)));
    const Box2 region = common.inflated(tol::kGeometric);

    // Broad phase over the design triangles that reach the common area. Only
    // these can contribute, and only they are worth indexing.
    std::vector<katana::geometry::SpatialEntry> entries;
    entries.reserve(design.triangleCount());
    for (std::size_t t = 0; t < design.triangleCount(); ++t) {
        const Box2 box = design.planTriangle(t).boundingBox();
        if (box.intersects(region)) {
            entries.push_back({static_cast<katana::geometry::SpatialId>(t), box});
        }
    }
    if (entries.empty()) {
        return result;
    }
    katana::geometry::SpatialIndex index;
    index.rebuild(entries);

    // The existing triangles are cut into blocks of a FIXED size, and the
    // block, not the thread, is the unit of summation: each block sums its own
    // pieces in index order - existing triangles ascending, and for each its
    // design candidates in ascending id order (SpatialIndex sorts them) - and
    // the block sums are then combined in block order. The sequence of
    // floating-point operations is therefore fixed by the data alone, and the
    // answer is the same bits on one thread or sixteen (Rule 7; the
    // TaskPool(1) tests in test_volume.cpp hold it to that). The partition
    // must never be derived from the thread count, or it would not be.
    //
    // 256 triangles is a few hundred microseconds of clipping, so dispatch is
    // noise, and a 200k-triangle surface still makes ~800 blocks for sixteen
    // threads to balance over.
    constexpr std::size_t kBlockTriangles = 256;
    // One cache line each: neighbouring blocks are written by different
    // threads, and sharing a line would have them invalidate each other.
    struct alignas(64) BlockSums {
        SignedIntegral integral;
        detail::CompensatedSum planArea;
        std::size_t pieces = 0;
    };
    const std::size_t existingCount = existing.triangleCount();
    const std::size_t blockCount = (existingCount + kBlockTriangles - 1) / kBlockTriangles;
    std::vector<BlockSums> blocks(blockCount);

    const auto compareBlocks = [&](std::size_t firstBlock, std::size_t endBlock) {
        // Per call, not shared: the index query and the clipper each need a
        // buffer, and a chunk of blocks runs on one thread.
        std::vector<katana::geometry::SpatialId> candidates;
        std::array<Point2, 8> piece{};
        for (std::size_t block = firstBlock; block < endBlock; ++block) {
            BlockSums& sums = blocks[block];
            const std::size_t first = block * kBlockTriangles;
            const std::size_t end = std::min(first + kBlockTriangles, existingCount);
            for (std::size_t te = first; te < end; ++te) {
                const Triangle2 planExisting = existing.planTriangle(te);
                const Box2 boxExisting = planExisting.boundingBox();
                if (!boxExisting.intersects(region)) {
                    continue;
                }
                // Hoisted out of the candidate and fan loops below: a
                // degenerate triangle has no plane to evaluate, so every piece
                // cut from it was already being dropped one at a time.
                if (planExisting.isDegenerate()) {
                    continue;
                }
                const double twiceAreaExisting = planExisting.twiceSignedArea();
                const std::array<Point2, 3> subject = orientedCorners(planExisting);
                index.query(boxExisting, candidates);
                for (const katana::geometry::SpatialId id : candidates) {
                    const auto td = static_cast<std::uint32_t>(id);
                    const Triangle2 planDesign = design.planTriangle(td);
                    if (planDesign.isDegenerate()) {
                        continue;
                    }
                    const double twiceAreaDesign = planDesign.twiceSignedArea();
                    const std::array<Point2, 3> window = orientedCorners(planDesign);
                    const std::size_t corners = clipTriangleToTriangle(subject, window, piece);
                    if (corners < 3) {
                        continue;
                    }
                    // The clipped region is convex, so a fan from its first
                    // vertex covers it exactly once with no coordinates that
                    // are not already on its boundary.
                    for (std::size_t i = 1; i + 1 < corners; ++i) {
                        const std::array<Point2, 3> part{piece[0], piece[i], piece[i + 1]};
                        const double area = Triangle2{part[0], part[1], part[2]}.signedArea();
                        if (!(area > 0.0)) {
                            continue;
                        }
                        // `part` lies inside both triangles by construction, so
                        // both planes are evaluated with barycentric weights in
                        // [0, 1] and neither surface has to be searched for the
                        // triangle to use.
                        const std::array<double, 3> zExisting =
                            planeElevations(existing, static_cast<std::uint32_t>(te),
                                            planExisting, twiceAreaExisting, part);
                        const std::array<double, 3> zDesign =
                            planeElevations(design, td, planDesign, twiceAreaDesign, part);
                        sums.integral.add(area, {zDesign[0] - zExisting[0],
                                                 zDesign[1] - zExisting[1],
                                                 zDesign[2] - zExisting[2]});
                        sums.planArea.add(area);
                        ++sums.pieces;
                    }
                }
            }
        }
    };
    // Everything the blocks read - both surfaces and the index - is immutable
    // here, and each block writes only its own BlockSums.
    katana::core::TaskPool& workers = pool != nullptr ? *pool : katana::core::TaskPool::shared();
    workers.parallelRanges(0, blockCount, 1, compareBlocks);

    SignedIntegral integral;
    detail::CompensatedSum planArea;
    for (const BlockSums& sums : blocks) {
        integral.merge(sums.integral);
        planArea.add(sums.planArea.value());
        result.overlayTriangleCount += sums.pieces;
    }
    result.fill = integral.positive();
    result.cut = integral.negative();
    result.net = result.fill - result.cut;
    result.planArea = planArea.value();
    result.fillArea = integral.positiveArea();
    result.cutArea = integral.negativeArea();
    return result;
}

} // namespace katana::terrain
