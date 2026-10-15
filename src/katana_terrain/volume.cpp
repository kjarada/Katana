#include "katana/terrain/volume.hpp"

#include "katana/core/task_pool.hpp"
#include "katana/core/text.hpp"
#include "katana/terrain/section_volume.hpp"
#include "katana/terrain/tin_builder.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <cstdint>
#include <limits>
#include <optional>
#include <string>
#include <vector>

#include "summation.hpp"

namespace katana::terrain {

namespace tol = katana::math::tolerance;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::geometry::Polyline2;
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

// The overlay's pieces summed one block at a time, each block into its own
// slot, and the slots combined in block order: the same sequence of
// floating-point operations whatever the thread count (overlay.hpp).
class ComparisonSums final : public OverlayVisitor {
  public:
    explicit ComparisonSums(std::size_t blocks) : blocks_(blocks) {}

    void piece(std::size_t block, const OverlayPiece& piece) override
    {
        BlockSums& sums = blocks_[block];
        sums.integral.add(piece.area, {piece.zTop[0] - piece.zBase[0],
                                       piece.zTop[1] - piece.zBase[1],
                                       piece.zTop[2] - piece.zBase[2]});
        sums.planArea.add(piece.area);
        ++sums.pieces;
    }

    [[nodiscard]] SurfaceComparison result() const
    {
        SignedIntegral integral;
        detail::CompensatedSum planArea;
        SurfaceComparison comparison;
        for (const BlockSums& sums : blocks_) {
            integral.merge(sums.integral);
            planArea.add(sums.planArea.value());
            comparison.overlayTriangleCount += sums.pieces;
        }
        comparison.fill = integral.positive();
        comparison.cut = integral.negative();
        comparison.net = comparison.fill - comparison.cut;
        comparison.planArea = planArea.value();
        comparison.fillArea = integral.positiveArea();
        comparison.cutArea = integral.negativeArea();
        return comparison;
    }

  private:
    // One cache line each: neighbouring blocks are written by different
    // threads, and sharing a line would have them invalidate each other.
    struct alignas(64) BlockSums {
        SignedIntegral integral;
        detail::CompensatedSum planArea;
        std::size_t pieces = 0;
    };
    std::vector<BlockSums> blocks_;
};

Result<SurfaceComparison> compare(const TinSurface& existing, const TinSurface& design,
                                  const OverlayRegion* region, katana::core::TaskPool* pool)
{
    ComparisonSums sums(overlayBlockCount(existing));
    if (auto status = visitOverlay(existing, design, sums, region, pool); !status) {
        return status.error();
    }
    return sums.result();
}

// A comparison read as a volume to a level: the surface is the existing one,
// the level the design, so what stands above the level is the cut.
DatumVolume asDatumVolume(const SurfaceComparison& comparison)
{
    DatumVolume volume;
    volume.above = comparison.cut;
    volume.below = comparison.fill;
    volume.net = volume.above - volume.below;
    volume.planAreaAbove = comparison.cutArea;
    volume.planAreaBelow = comparison.fillArea;
    volume.planArea = comparison.planArea;
    return volume;
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
    detail::CompensatedSum planArea;
    for (std::size_t t = 0; t < surface.triangleCount(); ++t) {
        const TinTriangle& tri = surface.triangles()[t];
        // Heights above the datum first: subtracting the datum from the summed
        // prism volume instead would cancel most digits for elevations far from 0.
        const double area = surface.trianglePlanArea(t);
        integral.add(area, {surface.vertices()[tri[0]].z - datum,
                            surface.vertices()[tri[1]].z - datum,
                            surface.vertices()[tri[2]].z - datum});
        planArea.add(area);
    }
    DatumVolume result;
    result.above = integral.positive();
    result.below = integral.negative();
    result.net = result.above - result.below;
    result.planAreaAbove = integral.positiveArea();
    result.planAreaBelow = integral.negativeArea();
    result.planArea = planArea.value();
    return result;
}

Result<DatumVolume> volumeToDatum(const TinSurface& surface, double datum,
                                  const OverlayRegion& region, katana::core::TaskPool* pool)
{
    if (!std::isfinite(datum)) {
        return makeError(ErrorCode::InvalidArgument, "datum elevation must be finite");
    }
    if (surface.empty()) {
        return makeError(ErrorCode::InvalidArgument, "surface has no triangles");
    }
    if (region.empty()) {
        return makeError(ErrorCode::InvalidArgument, "the region has no area");
    }
    auto level = region.atLevel(datum);
    if (!level) {
        return level.error();
    }
    auto comparison = compare(surface, *level, nullptr, pool);
    if (!comparison) {
        return comparison.error();
    }
    return asDatumVolume(*comparison);
}

Result<SurfaceComparison> compareSurfaces(const TinSurface& existing, const TinSurface& design,
                                          katana::core::TaskPool* pool)
{
    return compare(existing, design, nullptr, pool);
}

Result<SurfaceComparison> compareSurfaces(const TinSurface& existing, const TinSurface& design,
                                          const OverlayRegion& region,
                                          katana::core::TaskPool* pool)
{
    if (region.empty()) {
        return makeError(ErrorCode::InvalidArgument, "the region has no area");
    }
    return compare(existing, design, &region, pool);
}

// ---- stockpiles and pits ------------------------------------------------------------------

Result<StockpileVolume> stockpileVolume(const TinSurface& surface, const Polyline2& boundary,
                                        const StockpileOptions& options,
                                        katana::core::TaskPool* pool)
{
    if (surface.empty()) {
        return makeError(ErrorCode::InvalidArgument, "surface has no triangles");
    }
    if (options.base == StockpileBase::Level && !std::isfinite(options.level)) {
        return makeError(ErrorCode::InvalidArgument, "the base level must be finite");
    }
    // A ring drawn back to its start repeats its first vertex at its end:
    // the same ring without it, and its height list with it.
    Polyline2 ring = boundary;
    ring.closed = true;
    std::vector<std::optional<double>> ringHeights = options.boundaryHeights;
    while (ring.vertices.size() > 1 &&
           ring.vertices.front().distanceTo(ring.vertices.back()) <= tol::kGeometric) {
        ring.vertices.pop_back();
        if (ringHeights.size() > ring.vertices.size()) {
            ringHeights.pop_back();
        }
    }
    auto region = OverlayRegion::fromPolygon(ring);
    if (!region) {
        return region.error();
    }

    StockpileVolume result;
    result.base = options.base;
    result.boundaryArea = region->area();
    TinSurface base;
    if (options.base == StockpileBase::Boundary) {
        // The toe as it stands: each vertex's own height, or the ground's
        // there. A vertex with neither is refused - its height would be a
        // guess, and a base at 0 there would add a cone of nothing.
        const bool own = !ringHeights.empty() && ringHeights.size() == ring.vertices.size() &&
                         std::all_of(ringHeights.begin(), ringHeights.end(),
                                     [](const std::optional<double>& h) {
                                         return h.has_value() && std::isfinite(*h);
                                     });
        result.boundaryHeights = own;
        TinInput input;
        for (std::size_t v = 0; v < ring.vertices.size(); ++v) {
            const Point2& at = ring.vertices[v];
            const std::optional<double> height = own ? ringHeights[v] : surface.elevationAt(at);
            if (!height) {
                return makeError(ErrorCode::InvalidGeometry,
                                 "boundary vertex " + std::to_string(v + 1) +
                                     " is off the surface and has no height of its own, so the "
                                     "base has no height there; give the boundary heights, or "
                                     "use the lowest point or a level",
                                 "vertex=" + std::to_string(v + 1));
            }
            input.points.emplace_back(at.x, at.y, *height);
        }
        input.boundary = ring;
        // Two vertices of the ring at one place (a ring that doubles back
        // to touch itself is refused by the triangulator anyway) keep the
        // first's height.
        TinBuildOptions buildOptions;
        buildOptions.duplicatePoints = DuplicatePointPolicy::KeepFirst;
        auto built = buildTin(input, buildOptions);
        if (!built) {
            return makeError(ErrorCode::InvalidGeometry,
                             "the boundary cannot be made a base: " + built.error().message,
                             built.error().context);
        }
        base = std::move(built->surface);
    } else {
        double level = options.level;
        if (options.base == StockpileBase::Lowest) {
            // The lowest point along the boundary is at a vertex of it or
            // where an edge of the surface crosses it: the ends of the
            // pieces the profiler gives, which are exact.
            const SegmentProfiler profiler(surface);
            std::optional<double> lowest;
            const std::size_t count = ring.vertices.size();
            for (std::size_t v = 0; v < count; ++v) {
                const Point2& from = ring.vertices[v];
                const Point2& to = ring.vertices[(v + 1) % count];
                for (const SegmentPiece& piece : profiler.along(from, to)) {
                    const double low = std::min(piece.z0, piece.z1);
                    lowest = lowest ? std::min(*lowest, low) : low;
                }
                if (const auto at = surface.elevationAt(from)) {
                    lowest = lowest ? std::min(*lowest, *at) : *at;
                }
            }
            if (!lowest) {
                return makeError(ErrorCode::InvalidGeometry,
                                 "the boundary does not cross the surface anywhere, so it has no "
                                 "lowest point on it");
            }
            level = *lowest;
        }
        result.level = level;
        auto flat = region->atLevel(level);
        if (!flat) {
            return flat.error();
        }
        base = std::move(*flat);
    }

    auto comparison = compare(surface, base, nullptr, pool);
    if (!comparison) {
        return comparison.error();
    }
    result.volume = asDatumVolume(*comparison);
    return result;
}

// ---- the grid method -----------------------------------------------------------------------

Result<GridVolume> gridVolume(const TinSurface& existing, const TinSurface& design, double cell,
                              const OverlayRegion* region, katana::core::TaskPool* pool,
                              std::span<const OverlayRegion> claimed)
{
    if (existing.empty() || design.empty()) {
        return makeError(ErrorCode::InvalidArgument, "both surfaces must have triangles");
    }
    if (!(cell > 0.0) || !std::isfinite(cell)) {
        return makeError(ErrorCode::InvalidArgument, "the cell must be a positive length");
    }
    GridVolume result;
    result.cell = cell;
    if (!existing.bounds().intersects(design.bounds())) {
        return result;
    }
    Box2 common(Point2(std::max(existing.bounds().min.x, design.bounds().min.x),
                       std::max(existing.bounds().min.y, design.bounds().min.y)),
                Point2(std::min(existing.bounds().max.x, design.bounds().max.x),
                       std::min(existing.bounds().max.y, design.bounds().max.y)));
    if (region != nullptr) {
        if (region->empty() || !common.intersects(region->bounds())) {
            return result;
        }
        common = Box2(Point2(std::max(common.min.x, region->bounds().min.x),
                             std::max(common.min.y, region->bounds().min.y)),
                      Point2(std::min(common.max.x, region->bounds().max.x),
                             std::min(common.max.y, region->bounds().max.y)));
    }
    // Every cell whose centre can be inside the common box: lines at whole
    // multiples of the cell, from the one at or below the box's lower edge.
    const double firstColumn = std::floor(common.min.x / cell);
    const double firstRow = std::floor(common.min.y / cell);
    const double columnsReal = std::ceil(common.max.x / cell) - firstColumn;
    const double rowsReal = std::ceil(common.max.y / cell) - firstRow;
    if (!(columnsReal * rowsReal <= static_cast<double>(kMaxGridVolumeCells))) {
        const double fits = std::sqrt(common.width() * common.height() /
                                      static_cast<double>(kMaxGridVolumeCells));
        return makeError(ErrorCode::InvalidArgument,
                         "a grid of " + std::to_string(static_cast<std::uint64_t>(columnsReal)) +
                             " x " + std::to_string(static_cast<std::uint64_t>(rowsReal)) +
                             " cells is more than the " +
                             std::to_string(kMaxGridVolumeCells) +
                             " allowed; a cell of at least " +
                             katana::core::formatExactReal(std::ceil(fits * 1000.0) / 1000.0) +
                             " fits");
    }
    const auto columns = static_cast<std::size_t>(std::max(columnsReal, 0.0));
    const auto rows = static_cast<std::size_t>(std::max(rowsReal, 0.0));
    const double cellArea = cell * cell;

    // Rows in blocks of a fixed size, each summed in order into its own slot,
    // and the slots combined in order: the same bits at any thread count.
    constexpr std::size_t kRowsPerBlock = 16;
    const std::size_t blockCount = (rows + kRowsPerBlock - 1) / kRowsPerBlock;
    struct alignas(64) BlockSums {
        detail::CompensatedSum cut;
        detail::CompensatedSum fill;
        std::size_t cells = 0;
    };
    std::vector<BlockSums> blocks(blockCount);
    const auto sumBlocks = [&](std::size_t firstBlock, std::size_t endBlock) {
        for (std::size_t block = firstBlock; block < endBlock; ++block) {
            BlockSums& sums = blocks[block];
            const std::size_t rowEnd = std::min((block + 1) * kRowsPerBlock, rows);
            for (std::size_t row = block * kRowsPerBlock; row < rowEnd; ++row) {
                const double y = (firstRow + static_cast<double>(row) + 0.5) * cell;
                for (std::size_t column = 0; column < columns; ++column) {
                    const Point2 centre((firstColumn + static_cast<double>(column) + 0.5) * cell,
                                        y);
                    if (!common.contains(centre)) {
                        continue;
                    }
                    if (region != nullptr && !region->contains(centre)) {
                        continue;
                    }
                    // Counted with an earlier polygon already: a centre on an
                    // edge two polygons share is inside both.
                    if (std::any_of(claimed.begin(), claimed.end(),
                                    [&](const OverlayRegion& earlier) {
                                        return earlier.bounds()
                                                   .inflated(tol::kGeometric)
                                                   .contains(centre) &&
                                               earlier.contains(centre);
                                    })) {
                        continue;
                    }
                    const auto zExisting = existing.elevationAt(centre);
                    if (!zExisting) {
                        continue;
                    }
                    const auto zDesign = design.elevationAt(centre);
                    if (!zDesign) {
                        continue;
                    }
                    const double depth = *zDesign - *zExisting;
                    if (depth > 0.0) {
                        sums.fill.add(depth * cellArea);
                    } else if (depth < 0.0) {
                        sums.cut.add(-depth * cellArea);
                    }
                    ++sums.cells;
                }
            }
        }
    };
    katana::core::TaskPool& workers = pool != nullptr ? *pool : katana::core::TaskPool::shared();
    workers.parallelRanges(0, blockCount, 1, sumBlocks);

    detail::CompensatedSum cut;
    detail::CompensatedSum fill;
    for (const BlockSums& sums : blocks) {
        cut.add(sums.cut.value());
        fill.add(sums.fill.value());
        result.cells += sums.cells;
    }
    result.cut = cut.value();
    result.fill = fill.value();
    result.net = result.fill - result.cut;
    result.planArea = static_cast<double>(result.cells) * cellArea;
    return result;
}

} // namespace katana::terrain
