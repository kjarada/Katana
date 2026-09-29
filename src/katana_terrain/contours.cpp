#include "katana/terrain/contours.hpp"

#include "katana/core/task_pool.hpp"

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <iterator>
#include <span>
#include <string>

#include "katana/core/text.hpp"

namespace katana::terrain {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::geometry::Polyline2;

namespace {

// Marching triangles with the "z >= level is above" rule. A triangle is crossed
// when it has vertices on both sides; it then has exactly one edge running from
// above to below (in its counter-clockwise order) and one running from below to
// above. The contour enters through the first and leaves through the second,
// which puts the higher ground on its left; in the neighbour across the exit
// edge that same edge runs the other way and is therefore its entry edge.
class LevelTracer {
  public:
    LevelTracer(const TinSurface& surface, std::vector<std::uint32_t>& visitedStamp)
        : surface_(surface), visitedStamp_(visitedStamp)
    {
    }

    // Traces all contours of `level` through `candidates` (every crossed
    // triangle, ascending). `stamp` must be unique per call and non-zero.
    void trace(double level, bool major, std::span<const std::uint32_t> candidates,
               std::uint32_t stamp, std::vector<Contour>& output)
    {
        level_ = level;
        stamp_ = stamp;
        // Open contours start where the entry edge lies on the rim. Tracing them
        // first leaves only closed rings for the second sweep.
        for (const std::uint32_t t : candidates) {
            if (visitedStamp_[t] != stamp_ && surface_.neighbors()[t][entryEdge(t)] == kNoTriangle) {
                emit(follow(t), major, output);
            }
        }
        for (const std::uint32_t t : candidates) {
            if (visitedStamp_[t] != stamp_) {
                emit(follow(t), major, output);
            }
        }
    }

    [[nodiscard]] bool isCrossed(std::uint32_t t, double level) const
    {
        const TinTriangle& tri = surface_.triangles()[t];
        const bool a = surface_.vertices()[tri[0]].z >= level;
        const bool b = surface_.vertices()[tri[1]].z >= level;
        const bool c = surface_.vertices()[tri[2]].z >= level;
        return a != b || b != c;
    }

  private:
    [[nodiscard]] bool above(std::uint32_t vertex) const
    {
        return surface_.vertices()[vertex].z >= level_;
    }

    [[nodiscard]] std::size_t entryEdge(std::uint32_t t) const
    {
        const TinTriangle& tri = surface_.triangles()[t];
        for (std::size_t k = 0; k < 3; ++k) {
            if (above(tri[k]) && !above(tri[(k + 1) % 3])) {
                return k;
            }
        }
        return 0; // not reached for a crossed triangle
    }

    [[nodiscard]] std::size_t exitEdge(std::uint32_t t) const
    {
        const TinTriangle& tri = surface_.triangles()[t];
        for (std::size_t k = 0; k < 3; ++k) {
            if (!above(tri[k]) && above(tri[(k + 1) % 3])) {
                return k;
            }
        }
        return 0; // not reached for a crossed triangle
    }

    // Where the level meets edge k of triangle t. Always evaluated from the low
    // end to the high end, so both triangles sharing the edge get the identical
    // point; a high end exactly at the level is returned as is.
    [[nodiscard]] Point2 crossing(std::uint32_t t, std::size_t k) const
    {
        const TinTriangle& tri = surface_.triangles()[t];
        const Point3& first = surface_.vertices()[tri[k]];
        const Point3& second = surface_.vertices()[tri[(k + 1) % 3]];
        const Point3& low = first.z >= level_ ? second : first;
        const Point3& high = first.z >= level_ ? first : second;
        if (high.z == level_) {
            return Point2(high.x, high.y);
        }
        const double fraction = (level_ - low.z) / (high.z - low.z);
        return Point2(low.x + fraction * (high.x - low.x), low.y + fraction * (high.y - low.y));
    }

    [[nodiscard]] Polyline2 follow(std::uint32_t start)
    {
        Polyline2 line;
        line.vertices.push_back(crossing(start, entryEdge(start)));
        std::uint32_t t = start;
        while (true) {
            visitedStamp_[t] = stamp_;
            const std::size_t exit = exitEdge(t);
            const std::uint32_t next = surface_.neighbors()[t][exit];
            if (next == start) {
                line.closed = true; // the exit point would repeat the first point
                break;
            }
            line.vertices.push_back(crossing(t, exit));
            if (next == kNoTriangle || visitedStamp_[next] == stamp_) {
                break; // reached the rim (the second case cannot occur on a valid surface)
            }
            t = next;
        }
        return line;
    }

    // Drops the zero-length pieces that arise where the level passes exactly
    // through vertices, and contours that have no extent at all.
    void emit(Polyline2 line, bool major, std::vector<Contour>& output) const
    {
        auto& v = line.vertices;
        v.erase(std::unique(v.begin(), v.end()), v.end());
        if (line.closed) {
            while (v.size() > 1 && v.front() == v.back()) {
                v.pop_back();
            }
        }
        if (v.size() < (line.closed ? 3u : 2u)) {
            return;
        }
        output.push_back(Contour{level_, major, std::move(line)});
    }

    const TinSurface& surface_;
    std::vector<std::uint32_t>& visitedStamp_;
    double level_ = 0.0;
    std::uint32_t stamp_ = 0;
};

} // namespace

Result<std::vector<Contour>> contourAt(const TinSurface& surface, double elevation)
{
    if (!std::isfinite(elevation)) {
        return makeError(ErrorCode::InvalidArgument, "contour elevation must be finite");
    }
    std::vector<Contour> output;
    if (surface.empty() || !(elevation > surface.minElevation()) ||
        elevation > surface.maxElevation()) {
        return output; // { z >= level } is everything or nothing: no boundary
    }
    std::vector<std::uint32_t> visitedStamp(surface.triangleCount(), 0);
    LevelTracer tracer(surface, visitedStamp);
    std::vector<std::uint32_t> candidates;
    for (std::uint32_t t = 0; t < surface.triangleCount(); ++t) {
        if (tracer.isCrossed(t, elevation)) {
            candidates.push_back(t);
        }
    }
    tracer.trace(elevation, false, candidates, 1, output);
    return output;
}

Result<std::vector<Contour>> contours(const TinSurface& surface, double interval, double base,
                                      std::size_t majorEvery, katana::core::TaskPool* pool)
{
    // Levels closer than kGeometric would be the same geometric feature (and,
    // far below that, no longer distinct doubles at survey elevations).
    if (!(interval > katana::math::tolerance::kGeometric) || !std::isfinite(interval)) {
        return makeError(ErrorCode::InvalidArgument,
                         "contour interval must be finite and larger than tolerance::kGeometric",
                         "interval=" + katana::core::formatExactReal(interval));
    }
    if (!std::isfinite(base)) {
        return makeError(ErrorCode::InvalidArgument, "contour base elevation must be finite");
    }
    std::vector<Contour> output;
    if (surface.empty()) {
        return output;
    }

    // Level k is base + k * interval, always evaluated by this one expression so
    // that every comparison sees the same double. A triangle (or the surface)
    // with elevation range [lo, hi] is crossed by the levels in (lo, hi].
    const auto level = [&](std::int64_t k) { return base + static_cast<double>(k) * interval; };
    // The estimates from the division can be one off; the loops settle them
    // against the level values actually used.
    const auto firstLevelAbove = [&](double z) {
        auto k = static_cast<std::int64_t>(std::floor((z - base) / interval));
        while (level(k) > z) {
            --k;
        }
        while (!(level(k) > z)) {
            ++k;
        }
        return k;
    };
    const auto lastLevelAtOrBelow = [&](double z) { return firstLevelAbove(z) - 1; };

    // Beyond 2^52 the level index itself is no longer exact.
    constexpr double kMaxLevelIndex = 4.0e15;
    if (std::abs((surface.minElevation() - base) / interval) > kMaxLevelIndex ||
        std::abs((surface.maxElevation() - base) / interval) > kMaxLevelIndex) {
        return makeError(ErrorCode::InvalidArgument,
                         "contour interval is too small for the elevations of the surface",
                         "interval=" + katana::core::formatExactReal(interval));
    }
    const std::int64_t firstK = firstLevelAbove(surface.minElevation());
    const std::int64_t lastK = lastLevelAtOrBelow(surface.maxElevation());
    if (lastK < firstK) {
        return output;
    }
    const auto levelCount = static_cast<std::size_t>(lastK - firstK + 1);
    if (levelCount > kMaxContourLevels) {
        return makeError(ErrorCode::InvalidArgument,
                         "contour interval is too small for the relief of the surface",
                         "levels=" + std::to_string(levelCount) +
                             " limit=" + std::to_string(kMaxContourLevels));
    }

    katana::core::TaskPool& workers = pool != nullptr ? *pool : katana::core::TaskPool::shared();

    // The levels crossing each triangle, found once and kept: both bucketing
    // passes below need them, and the floor and the settling loops are most
    // of the bucketing's cost. Each triangle writes only its own slot, so the
    // parallel pass fills exactly what a serial one would.
    struct LevelRange {
        std::int64_t first = 0;
        std::int64_t last = -1; // empty when last < first
    };
    const auto triangleCount = static_cast<std::uint32_t>(surface.triangleCount());
    std::vector<LevelRange> ranges(triangleCount);
    workers.parallelRanges(0, triangleCount, 4096, [&](std::size_t lo, std::size_t hi) {
        for (std::size_t t = lo; t < hi; ++t) {
            const TinTriangle& tri = surface.triangles()[t];
            const double z0 = surface.vertices()[tri[0]].z;
            const double z1 = surface.vertices()[tri[1]].z;
            const double z2 = surface.vertices()[tri[2]].z;
            const double low = std::min({z0, z1, z2});
            const double high = std::max({z0, z1, z2});
            if (low < high) {
                ranges[t] = LevelRange{firstLevelAbove(low), lastLevelAtOrBelow(high)};
            }
        }
    });

    // Bucket the triangles by the levels that cross them (CSR), so that tracing a
    // level touches only its own triangles. Total size = number of contour pieces.
    // Serial, and in triangle order, so every bucket is ascending - the order
    // LevelTracer::trace documents and the output order depends on.
    std::vector<std::size_t> start(levelCount + 1, 0);
    for (std::uint32_t t = 0; t < triangleCount; ++t) {
        for (std::int64_t k = ranges[t].first; k <= ranges[t].last; ++k) {
            ++start[static_cast<std::size_t>(k - firstK) + 1];
        }
    }
    for (std::size_t i = 0; i < levelCount; ++i) {
        start[i + 1] += start[i];
    }
    std::vector<std::uint32_t> bucket(start[levelCount]);
    std::vector<std::size_t> cursor(start.begin(), start.end() - 1);
    for (std::uint32_t t = 0; t < triangleCount; ++t) {
        for (std::int64_t k = ranges[t].first; k <= ranges[t].last; ++k) {
            bucket[cursor[static_cast<std::size_t>(k - firstK)]++] = t;
        }
    }

    // Levels are independent: tracing one reads the immutable surface and its
    // own bucket, and writes its own list. They are traced in parallel into
    // one list per level, and the lists are joined in level order, so the
    // result is identical to the serial trace whatever the thread count
    // (Rule 7; test_contours.cpp compares against TaskPool(1)).
    //
    // A tracer's visited stamps must be unique per level WITHIN the array it
    // uses, not globally, so each chunk of levels has an array of its own and
    // numbers its levels from 1. Chunks are sized to give each thread a few
    // (the levels differ greatly in length), which bounds the arrays alive at
    // once by the thread count; the chunking cannot change a result, since no
    // level sees another level's stamps as its own.
    std::vector<std::vector<Contour>> perLevel(levelCount);
    const std::size_t levelsPerChunk =
        std::max<std::size_t>(1, levelCount / (workers.concurrency() * 4));
    workers.parallelRanges(0, levelCount, levelsPerChunk, [&](std::size_t lo, std::size_t hi) {
        std::vector<std::uint32_t> visitedStamp(triangleCount, 0);
        LevelTracer tracer(surface, visitedStamp);
        for (std::size_t i = lo; i < hi; ++i) {
            const std::int64_t k = firstK + static_cast<std::int64_t>(i);
            const bool major = majorEvery != 0 && k % static_cast<std::int64_t>(majorEvery) == 0;
            tracer.trace(level(k), major,
                         std::span<const std::uint32_t>(bucket.data() + start[i],
                                                        start[i + 1] - start[i]),
                         static_cast<std::uint32_t>(i - lo + 1), perLevel[i]);
        }
    });

    std::size_t total = 0;
    for (const auto& lines : perLevel) {
        total += lines.size();
    }
    output.reserve(total);
    for (auto& lines : perLevel) {
        std::move(lines.begin(), lines.end(), std::back_inserter(output));
    }
    return output;
}

} // namespace katana::terrain
