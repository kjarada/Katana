#include "katana/terrain/section_volume.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <cstdint>
#include <string>
#include <utility>

#include "katana/core/task_pool.hpp"
#include "katana/core/text.hpp"

#include "summation.hpp"

namespace katana::terrain {

namespace tol = katana::math::tolerance;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::geometry::SpatialEntry;
using katana::geometry::SpatialId;
using katana::geometry::Triangle2;

// ---- along a segment -----------------------------------------------------------------------

SegmentProfiler::SegmentProfiler(const TinSurface& surface) : surface_(&surface)
{
    std::vector<SpatialEntry> entries;
    entries.reserve(surface.triangleCount());
    for (std::size_t t = 0; t < surface.triangleCount(); ++t) {
        const Triangle2 plan = surface.planTriangle(t);
        // A degenerate triangle has no plane, and no length of a segment
        // lies inside it.
        if (!plan.isDegenerate()) {
            entries.push_back({static_cast<SpatialId>(t), plan.boundingBox()});
        }
    }
    index_.rebuild(entries);
}

namespace {

// Where the segment from `a` along `d` crosses the line of the edge from u to
// v: its parameter along the segment and the height along the edge there.
// Computed from the edge's two vertices in a fixed order (the lower index
// first), so both triangles that share the edge get the same bits.
struct EdgeCrossing {
    double t = 0.0;
    double z = 0.0;
};

EdgeCrossing crossEdge(const TinSurface& surface, std::uint32_t first, std::uint32_t second,
                       const Point2& a, const Point2& d)
{
    const std::uint32_t low = std::min(first, second);
    const std::uint32_t high = std::max(first, second);
    const Point3& u = surface.vertices()[low];
    const Point3& v = surface.vertices()[high];
    const double ex = v.x - u.x;
    const double ey = v.y - u.y;
    // A + t D = U + s (V - U): crossed with (V - U) gives t, with D gives s.
    const double denominator = d.x * ey - d.y * ex;
    const double t = ((u.x - a.x) * ey - (u.y - a.y) * ex) / denominator;
    const double s = ((u.x - a.x) * d.y - (u.y - a.y) * d.x) / denominator;
    const double along = std::clamp(s, 0.0, 1.0);
    return EdgeCrossing{t, u.z * (1.0 - along) + v.z * along};
}

} // namespace

std::vector<SegmentPiece> SegmentProfiler::along(const Point2& from, const Point2& to) const
{
    std::vector<SegmentPiece> pieces;
    const Point2 d(to.x - from.x, to.y - from.y);
    if (d.x == 0.0 && d.y == 0.0) {
        return pieces;
    }
    Box2 box;
    box.expand(from);
    box.expand(to);
    std::vector<SpatialId> candidates;
    index_.query(box.inflated(tol::kGeometric), candidates);
    const TinSurface& surface = *surface_;
    for (const SpatialId id : candidates) {
        const auto t = static_cast<std::size_t>(id);
        const TinTriangle& tri = surface.triangles()[t];
        std::array<std::uint32_t, 3> corner{tri[0], tri[1], tri[2]};
        const auto planOf = [&](std::uint32_t v) {
            return Point2(surface.vertices()[v].x, surface.vertices()[v].y);
        };
        if (Triangle2{planOf(corner[0]), planOf(corner[1]), planOf(corner[2])}.signedArea() <
            0.0) {
            std::swap(corner[1], corner[2]); // counter-clockwise, so inside is left
        }
        // Cyrus-Beck: each edge bounds the segment's parameter from below
        // where the segment enters across it and from above where it leaves.
        double t0 = 0.0;
        double t1 = 1.0;
        std::optional<double> z0;
        std::optional<double> z1;
        bool outside = false;
        for (std::size_t k = 0; k < 3 && !outside; ++k) {
            const Point2 p = planOf(corner[k]);
            const Point2 q = planOf(corner[(k + 1) % 3]);
            const double ex = q.x - p.x;
            const double ey = q.y - p.y;
            const double rate = ex * d.y - ey * d.x; // how the side changes along the segment
            if (rate == 0.0) {
                // Parallel: wholly inside this edge's half-plane, or wholly out.
                outside = ex * (from.y - p.y) - ey * (from.x - p.x) < 0.0;
                continue;
            }
            const EdgeCrossing crossing = crossEdge(surface, corner[k], corner[(k + 1) % 3], from, d);
            if (rate > 0.0) {
                if (crossing.t > t0) {
                    t0 = crossing.t;
                    z0 = crossing.z;
                }
            } else if (crossing.t < t1) {
                t1 = crossing.t;
                z1 = crossing.z;
            }
        }
        if (outside || !(t1 > t0)) {
            continue;
        }
        // An end inside the triangle (the segment starts or ends in it) is
        // read off the triangle's plane.
        const auto plane = [&](double at) {
            const Point2 position(from.x + at * d.x, from.y + at * d.y);
            const Triangle2 plan = surface.planTriangle(t);
            const std::array<double, 3> w = plan.barycentric(position, plan.twiceSignedArea());
            return w[0] * surface.vertices()[tri[0]].z + w[1] * surface.vertices()[tri[1]].z +
                   w[2] * surface.vertices()[tri[2]].z;
        };
        pieces.push_back(SegmentPiece{t0, t1, z0 ? *z0 : plane(t0), z1 ? *z1 : plane(t1)});
    }
    std::sort(pieces.begin(), pieces.end(), [](const SegmentPiece& a, const SegmentPiece& b) {
        return a.t0 < b.t0 || (a.t0 == b.t0 && a.t1 < b.t1);
    });
    return pieces;
}

// ---- the area on one section ----------------------------------------------------------------

SignedArea signedTrapezoid(double d0, double d1, double width)
{
    SignedArea area;
    if (!(width > 0.0)) {
        return area;
    }
    if (d0 >= 0.0 && d1 >= 0.0) {
        area.positive = 0.5 * (d0 + d1) * width;
    } else if (d0 <= 0.0 && d1 <= 0.0) {
        area.negative = -0.5 * (d0 + d1) * width;
    } else {
        // The two cross inside the strip: two triangles of opposite sign,
        // split where the difference is zero.
        const double t = d0 / (d0 - d1);
        const double left = std::abs(d0) * t * width * 0.5;
        const double right = std::abs(d1) * (1.0 - t) * width * 0.5;
        (d0 > 0.0 ? area.positive : area.negative) += left;
        (d1 > 0.0 ? area.positive : area.negative) += right;
    }
    return area;
}

namespace {

// The piece of `pieces` (ascending t0) that holds `t`, or null in a gap.
const SegmentPiece* pieceAt(const std::vector<SegmentPiece>& pieces, double t)
{
    auto after = std::upper_bound(pieces.begin(), pieces.end(), t,
                                  [](double value, const SegmentPiece& piece) {
                                      return value < piece.t0;
                                  });
    // Every piece before `after` starts at or before `t`. Pieces of one
    // surface overlap by rounding at most (and repeat where the segment runs
    // along an edge, which both triangles beside it report), so the last two
    // are the only ones that can still reach it.
    for (int back = 0; back < 2 && after != pieces.begin(); ++back) {
        --after;
        if (t <= after->t1) {
            return &*after;
        }
    }
    return nullptr;
}

// Linear on the piece; exact at both of its ends.
double heightOn(const SegmentPiece& piece, double t)
{
    if (t <= piece.t0) {
        return piece.z0;
    }
    if (t >= piece.t1) {
        return piece.z1;
    }
    const double w = (t - piece.t0) / (piece.t1 - piece.t0);
    return piece.z0 * (1.0 - w) + piece.z1 * w;
}

} // namespace

SectionAreas sectionAreas(const std::vector<SegmentPiece>& existing,
                          const std::vector<SegmentPiece>& design, double length)
{
    SectionAreas areas;
    if (existing.empty() || design.empty() || !(length > 0.0)) {
        return areas;
    }
    // Every place either surface changes slope; between two of them both are
    // linear, and so is their difference.
    std::vector<double> breaks;
    breaks.reserve(2 * (existing.size() + design.size()));
    for (const std::vector<SegmentPiece>* pieces : {&existing, &design}) {
        for (const SegmentPiece& piece : *pieces) {
            breaks.push_back(piece.t0);
            breaks.push_back(piece.t1);
        }
    }
    std::sort(breaks.begin(), breaks.end());
    breaks.erase(std::unique(breaks.begin(), breaks.end()), breaks.end());

    detail::CompensatedSum cut;
    detail::CompensatedSum fill;
    detail::CompensatedSum width;
    for (std::size_t i = 0; i + 1 < breaks.size(); ++i) {
        const double ta = breaks[i];
        const double tb = breaks[i + 1];
        const double middle = 0.5 * (ta + tb);
        const SegmentPiece* under = pieceAt(existing, middle);
        const SegmentPiece* over = pieceAt(design, middle);
        if (under == nullptr || over == nullptr) {
            continue; // one of them has no height here: nothing is measured
        }
        const double strip = (tb - ta) * length;
        const SignedArea area = signedTrapezoid(heightOn(*over, ta) - heightOn(*under, ta),
                                                heightOn(*over, tb) - heightOn(*under, tb), strip);
        fill.add(area.positive);
        cut.add(area.negative);
        width.add(strip);
    }
    areas.cut = cut.value();
    areas.fill = fill.value();
    areas.width = width.value();
    return areas;
}

// ---- along an alignment ---------------------------------------------------------------------

namespace {

struct Station {
    double station = 0.0;
    bool key = false;
};

} // namespace

Result<AlignmentVolumes> volumesAlongAlignment(const TinSurface& existing,
                                               const TinSurface& design,
                                               const katana::geometry::SolvedAlignment& alignment,
                                               const AlignmentVolumeOptions& options,
                                               katana::core::TaskPool* pool)
{
    using katana::core::formatExactReal;
    if (existing.empty() || design.empty()) {
        return makeError(ErrorCode::InvalidArgument, "both surfaces must have triangles");
    }
    if (!(options.interval > 0.0) || !std::isfinite(options.interval)) {
        return makeError(ErrorCode::InvalidArgument, "the interval must be a positive length");
    }
    const double start = alignment.startStation();
    const double end = alignment.endStation();
    const double from = options.from.value_or(start);
    const double to = options.to.value_or(end);
    if (!std::isfinite(from) || !std::isfinite(to)) {
        return makeError(ErrorCode::InvalidArgument, "the stations must be finite");
    }
    if (from < start - tol::kCoordinate || to > end + tol::kCoordinate) {
        return makeError(ErrorCode::InvalidArgument,
                         "the range " + formatExactReal(from) + " to " + formatExactReal(to) +
                             " is outside the alignment, which runs " + formatExactReal(start) +
                             " to " + formatExactReal(end));
    }
    if (!(to - from > tol::kCoordinate)) {
        return makeError(ErrorCode::InvalidArgument,
                         "the range has no length: from must be before to");
    }
    const double first = std::max(from, start);
    const double last = std::min(to, end);
    if ((last - first) / options.interval > static_cast<double>(kMaxVolumeSections)) {
        return makeError(ErrorCode::InvalidArgument,
                         "an interval of " + formatExactReal(options.interval) + " over " +
                             formatExactReal(last - first) + " makes more than " +
                             std::to_string(kMaxVolumeSections) + " sections");
    }
    if (!existing.bounds().intersects(design.bounds())) {
        return makeError(ErrorCode::InvalidGeometry, "the surfaces have no ground in common");
    }
    const Box2 common(Point2(std::max(existing.bounds().min.x, design.bounds().min.x),
                             std::max(existing.bounds().min.y, design.bounds().min.y)),
                      Point2(std::min(existing.bounds().max.x, design.bounds().max.x),
                             std::min(existing.bounds().max.y, design.bounds().max.y)));
    const double reach = std::hypot(common.width(), common.height());
    const double left = options.left.value_or(reach);
    const double right = options.right.value_or(reach);
    if (!std::isfinite(left) || !std::isfinite(right) || left < 0.0 || right < 0.0) {
        return makeError(ErrorCode::InvalidArgument,
                         "left= and right= are distances from the centreline, 0 or more");
    }
    if (!(left + right > 0.0)) {
        return makeError(ErrorCode::InvalidArgument,
                         "a section must reach to one side at least: left= or right=");
    }

    // The stations: the ends, every whole multiple of the interval between,
    // and the alignment's key stations; two closer than kCoordinate (0.1 mm)
    // are one section, a key one if either was.
    std::vector<Station> stations{{first, true}, {last, true}};
    for (double k = std::floor(first / options.interval) + 1.0; k * options.interval < last;
         k += 1.0) {
        stations.push_back({k * options.interval, false});
    }
    for (const double key : alignment.keyStations()) {
        if (key > first && key < last) {
            stations.push_back({key, true});
        }
    }
    std::sort(stations.begin(), stations.end(),
              [](const Station& a, const Station& b) { return a.station < b.station; });
    std::vector<Station> merged;
    for (const Station& station : stations) {
        if (!merged.empty() && station.station - merged.back().station < tol::kCoordinate) {
            merged.back().key = merged.back().key || station.key;
            continue;
        }
        merged.push_back(station);
    }
    if (merged.size() > kMaxVolumeSections) {
        return makeError(ErrorCode::InvalidArgument,
                         "more than " + std::to_string(kMaxVolumeSections) + " sections");
    }

    // Every section to cut: the stations, then the middles for the
    // prismoidal formula.
    std::vector<double> cuts;
    cuts.reserve(2 * merged.size());
    for (const Station& station : merged) {
        cuts.push_back(station.station);
    }
    if (options.prismoidal) {
        for (std::size_t i = 0; i + 1 < merged.size(); ++i) {
            cuts.push_back(0.5 * (merged[i].station + merged[i + 1].station));
        }
    }

    const SegmentProfiler under(existing);
    const SegmentProfiler over(design);
    std::vector<StationSection> sections(cuts.size());
    const auto cutSections = [&](std::size_t begin, std::size_t endIndex) {
        for (std::size_t i = begin; i < endIndex; ++i) {
            StationSection& section = sections[i];
            section.station = cuts[i];
            const auto centre = alignment.pointAtStation(cuts[i]);
            const auto leftEnd = alignment.pointAtStationOffset(cuts[i], left);
            const auto rightEnd = alignment.pointAtStationOffset(cuts[i], -right);
            if (!centre || !leftEnd || !rightEnd) {
                continue; // inside the range by construction; nothing to cut otherwise
            }
            section.centre = *centre;
            const double length = leftEnd->distanceTo(*rightEnd);
            const SectionAreas areas =
                sectionAreas(under.along(*leftEnd, *rightEnd), over.along(*leftEnd, *rightEnd), length);
            section.cutArea = areas.cut;
            section.fillArea = areas.fill;
            section.width = areas.width;
        }
    };
    katana::core::TaskPool& workers = pool != nullptr ? *pool : katana::core::TaskPool::shared();
    // A section is a few hundred triangle clips; four to a task keeps the
    // dispatch small beside them. The grain cannot change a result: each
    // section is computed on its own.
    workers.parallelRanges(0, cuts.size(), 4, cutSections);

    AlignmentVolumes result;
    result.fromStation = first;
    result.toStation = last;
    result.left = left;
    result.right = right;
    result.prismoidal = options.prismoidal;
    for (std::size_t i = 0; i < merged.size(); ++i) {
        sections[i].key = merged[i].key;
        result.sections.push_back(sections[i]);
    }
    detail::CompensatedSum endCut;
    detail::CompensatedSum endFill;
    detail::CompensatedSum prismCut;
    detail::CompensatedSum prismFill;
    for (std::size_t i = 0; i + 1 < result.sections.size(); ++i) {
        const StationSection& a = result.sections[i];
        const StationSection& b = result.sections[i + 1];
        SectionPanel panel;
        panel.fromStation = a.station;
        panel.toStation = b.station;
        const double length = b.station - a.station;
        panel.endAreaCut = length * (a.cutArea + b.cutArea) / 2.0;
        panel.endAreaFill = length * (a.fillArea + b.fillArea) / 2.0;
        endCut.add(panel.endAreaCut);
        endFill.add(panel.endAreaFill);
        if (options.prismoidal) {
            const StationSection& m = sections[merged.size() + i];
            panel.middle = m;
            panel.prismoidalCut = length * (a.cutArea + 4.0 * m.cutArea + b.cutArea) / 6.0;
            panel.prismoidalFill = length * (a.fillArea + 4.0 * m.fillArea + b.fillArea) / 6.0;
            prismCut.add(panel.prismoidalCut);
            prismFill.add(panel.prismoidalFill);
        }
        result.panels.push_back(panel);
    }
    result.endAreaCut = endCut.value();
    result.endAreaFill = endFill.value();
    result.prismoidalCut = prismCut.value();
    result.prismoidalFill = prismFill.value();
    return result;
}

} // namespace katana::terrain
