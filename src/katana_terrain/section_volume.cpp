#include "katana/terrain/section_volume.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <format>
#include <limits>
#include <optional>
#include <string>
#include <utility>
#include <vector>

#include "katana/core/task_pool.hpp"
#include "katana/core/text.hpp"
#include "katana/geometry/spatial_index.hpp"

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
            // How far each end of the segment stands from this edge's line,
            // positive on the inside.
            const double edgeLength = std::sqrt(ex * ex + ey * ey);
            const double sideFrom = (ex * (from.y - p.y) - ey * (from.x - p.x)) / edgeLength;
            const double sideTo = (ex * (to.y - p.y) - ey * (to.x - p.x)) / edgeLength;
            if (std::abs(sideFrom) <= tol::kGeometric && std::abs(sideTo) <= tol::kGeometric) {
                // The segment lies ON this edge: it is the surface's boundary
                // or the seam between two triangles, and a section cut there
                // is cut on the ground, not beside it. Its ends are computed
                // from a bearing, so a north-going section is tilted by the
                // rounding of cos(pi/2) = 6e-17 and an exact test would
                // clip away whichever half the tilt put outside. Inside the
                // half-plane, as the exactly parallel case was: the triangle's
                // other two edges still bound it, and the triangle on the
                // edge's other side reports the same span, which pieceAt
                // takes for one.
                continue;
            }
            if (rate == 0.0) {
                // Parallel: wholly inside this edge's half-plane, or wholly out.
                outside = sideFrom < 0.0;
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

SignedMoment signedTrapezoidMoments(double d0, double d1, double width, double offsetAtStart)
{
    SignedMoment moment;
    if (!(width > 0.0)) {
        return moment;
    }
    // A strip of width w whose depth runs linearly from a to b (magnitudes),
    // its first side at offset o and the offset falling with u across it:
    //   integral over u in [0, w] of (a + (b - a) u / w) (o - u) du
    //     = o w (a + b) / 2 - w^2 (a / 6 + b / 3).
    const auto whole = [&](double a, double b) {
        return offsetAtStart * width * 0.5 * (a + b) - width * width * (a / 6.0 + b / 3.0);
    };
    // The same three cases as signedTrapezoid, in the same order, so a part
    // that has an area has a moment and one that has none has none.
    if (d0 >= 0.0 && d1 >= 0.0) {
        moment.positive = whole(d0, d1);
    } else if (d0 <= 0.0 && d1 <= 0.0) {
        moment.negative = whole(-d0, -d1);
    } else {
        // Two triangles of opposite sign split where the depth is zero: the
        // first of width t w with its apex of depth |d0| at the first side,
        // the second of width (1 - t) w with |d1| at the far side. A
        // triangle's centroid stands a third of its base from its tall side.
        const double t = d0 / (d0 - d1);
        const double firstWidth = t * width;
        const double secondWidth = (1.0 - t) * width;
        const double firstArea = std::abs(d0) * firstWidth * 0.5;
        const double secondArea = std::abs(d1) * secondWidth * 0.5;
        (d0 > 0.0 ? moment.positive : moment.negative) +=
            firstArea * (offsetAtStart - firstWidth / 3.0);
        (d1 > 0.0 ? moment.positive : moment.negative) +=
            secondArea * (offsetAtStart - width + secondWidth / 3.0);
    }
    return moment;
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
                          const std::vector<SegmentPiece>& design, double length,
                          double offsetAtStart)
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
    detail::CompensatedSum cutMoment;
    detail::CompensatedSum fillMoment;
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
        const double d0 = heightOn(*over, ta) - heightOn(*under, ta);
        const double d1 = heightOn(*over, tb) - heightOn(*under, tb);
        const SignedArea area = signedTrapezoid(d0, d1, strip);
        fill.add(area.positive);
        cut.add(area.negative);
        width.add(strip);
        // The strip starts ta along the section, so its first side is that
        // much nearer the section's far end than the section's first end is.
        const SignedMoment moment =
            signedTrapezoidMoments(d0, d1, strip, offsetAtStart - ta * length);
        fillMoment.add(moment.positive);
        cutMoment.add(moment.negative);
    }
    areas.cut = cut.value();
    areas.fill = fill.value();
    areas.width = width.value();
    areas.cutMoment = cutMoment.value();
    areas.fillMoment = fillMoment.value();
    return areas;
}

// ---- along an alignment ---------------------------------------------------------------------

namespace {

struct Station {
    double station = 0.0;
    bool key = false;
};

// A box as a refusal names it: the corners to the millimetre.
std::string boxText(const Box2& box)
{
    return std::format("{:.3f},{:.3f},{:.3f},{:.3f}", box.min.x, box.min.y, box.max.x, box.max.y);
}

// A section's two ends: left looking along increasing station, then right.
struct SectionEnds {
    Point2 left{};
    Point2 right{};
    bool valid = false;
};

// Where two sections properly cross: the fraction of each from its left end.
// Meeting at an end within kGeometric is not crossing - sections cut as far as
// a curve's radius all end on its centre, and that is as far as they may go.
// Parallel sections (the sections of a straight run) never cross.
std::optional<std::pair<double, double>> crossingOf(const SectionEnds& a, const SectionEnds& b)
{
    const double rx = a.right.x - a.left.x;
    const double ry = a.right.y - a.left.y;
    const double sx = b.right.x - b.left.x;
    const double sy = b.right.y - b.left.y;
    const double denominator = rx * sy - ry * sx;
    if (denominator == 0.0) {
        return std::nullopt;
    }
    // a.left + t r = b.left + u s, crossed with s and with r.
    const double qx = b.left.x - a.left.x;
    const double qy = b.left.y - a.left.y;
    const double t = (qx * sy - qy * sx) / denominator;
    const double u = (qx * ry - qy * rx) / denominator;
    const double marginA = tol::kGeometric / std::hypot(rx, ry);
    const double marginB = tol::kGeometric / std::hypot(sx, sy);
    if (t > marginA && t < 1.0 - marginA && u > marginB && u < 1.0 - marginB) {
        return std::pair<double, double>(t, u);
    }
    return std::nullopt;
}

struct SectionCrossing {
    std::size_t first = 0;
    std::size_t second = 0;
    // The longest reach, the same each side, at which no pair of sections
    // crosses: for a crossing, the greater of its two distances from the
    // centreline (cut shorter than that, one of the two sections has ended
    // before the other is reached), and the least over every crossing.
    double nearest = std::numeric_limits<double>::infinity();
};

// The first pair of sections (in station order) that cross, and how short a
// reach clears every crossing. The end-area method takes the volume of a
// panel from the areas of the two sections that bound it, and is right only
// while the panels tile the ground: where sections cross, the ground beyond
// the crossing is swept by both, and counted by both.
std::optional<SectionCrossing> firstCrossing(const std::vector<SectionEnds>& ends,
                                             std::size_t count, double left, double right,
                                             katana::core::TaskPool& workers)
{
    const auto boxOf = [&](std::size_t i) {
        Box2 box;
        box.expand(ends[i].left);
        box.expand(ends[i].right);
        return box.inflated(tol::kGeometric);
    };
    std::vector<katana::geometry::SpatialEntry> entries;
    entries.reserve(count);
    for (std::size_t i = 0; i < count; ++i) {
        if (ends[i].valid) {
            entries.push_back({static_cast<katana::geometry::SpatialId>(i), boxOf(i)});
        }
    }
    katana::geometry::SpatialIndex index;
    index.rebuild(entries);

    struct Slot {
        bool found = false;
        std::size_t second = 0;
        double nearest = std::numeric_limits<double>::infinity();
    };
    std::vector<Slot> slots(count);
    const double reach = left + right;
    const auto scan = [&](std::size_t begin, std::size_t endIndex) {
        std::vector<katana::geometry::SpatialId> candidates;
        for (std::size_t i = begin; i < endIndex; ++i) {
            if (!ends[i].valid) {
                continue;
            }
            index.query(boxOf(i), candidates);
            for (const katana::geometry::SpatialId id : candidates) {
                const auto j = static_cast<std::size_t>(id);
                if (j <= i) {
                    continue;
                }
                const auto crossing = crossingOf(ends[i], ends[j]);
                if (!crossing) {
                    continue;
                }
                // The distance of the crossing from the centreline along each
                // section: a section runs from `left` on one side of it to
                // `right` on the other.
                const double onFirst = std::abs(left - crossing->first * reach);
                const double onSecond = std::abs(left - crossing->second * reach);
                Slot& slot = slots[i];
                if (!slot.found) {
                    slot.found = true;
                    slot.second = j;
                }
                slot.nearest = std::min(slot.nearest, std::max(onFirst, onSecond));
            }
        }
    };
    workers.parallelRanges(0, count, 16, scan);

    // Combined in station order, so the pair named and the reach given are the
    // same at any thread count.
    std::optional<SectionCrossing> result;
    for (std::size_t i = 0; i < count; ++i) {
        if (!slots[i].found) {
            continue;
        }
        if (!result) {
            result = SectionCrossing{i, slots[i].second, slots[i].nearest};
        } else {
            result->nearest = std::min(result->nearest, slots[i].nearest);
        }
    }
    return result;
}

// ---- stretches that run side by side ---------------------------------------------------------

using Tri = std::array<Point2, 3>;

// Twice the signed area of the triangle o, a, b.
double crossOf(const Point2& o, const Point2& a, const Point2& b)
{
    return (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);
}

// The ground one panel sweeps: the quadrilateral between the reach segments of
// its two sections, as two triangles that tile it. A panel across a corner can
// be concave, and the diagonal from a corner that turns the wrong way would
// cover ground outside it; so the diagonal is the one whose two triangles both
// turn the way the quadrilateral does, and the other when the first fails.
struct Strip {
    std::array<Tri, 2> parts{};
    Box2 box{};
    bool valid = false;
};

Strip stripOf(const SectionEnds& a, const SectionEnds& b)
{
    Strip strip;
    if (!a.valid || !b.valid) {
        return strip;
    }
    // Round the quadrilateral: a's left, a's right, b's right, b's left.
    const Point2& p = a.left;
    const Point2& q = a.right;
    const Point2& r = b.right;
    const Point2& s = b.left;
    const double whole = crossOf(p, q, r) + crossOf(p, r, s);
    const auto turnsWithIt = [&](double part) { return whole >= 0.0 ? part >= 0.0 : part <= 0.0; };
    if (turnsWithIt(crossOf(p, q, r)) && turnsWithIt(crossOf(p, r, s))) {
        strip.parts = {Tri{p, q, r}, Tri{p, r, s}};
    } else if (turnsWithIt(crossOf(q, r, s)) && turnsWithIt(crossOf(q, s, p))) {
        strip.parts = {Tri{q, r, s}, Tri{q, s, p}};
    } else {
        // Its sides cross, which sections that do not cross cannot make but a
        // fold of more than a half turn in one panel might: the ground it
        // covers is not defined, and the first diagonal is as good a reading.
        strip.parts = {Tri{p, q, r}, Tri{p, r, s}};
    }
    for (const Point2* corner : {&p, &q, &r, &s}) {
        strip.box.expand(*corner);
    }
    strip.box = strip.box.inflated(tol::kGeometric);
    strip.valid = true;
    return strip;
}

// Whether two triangles interpenetrate by more than `depth`: the separating
// axis test, whose least overlap of the two shadows over the normals of the
// edges is the shortest way one would have to move to clear the other. A
// triangle of no area has an axis along which it is a point, so it never
// overlaps by anything.
bool overlapsDeeperThan(const Tri& first, const Tri& second, double depth)
{
    for (const Tri* owner : {&first, &second}) {
        for (std::size_t k = 0; k < 3; ++k) {
            const Point2& from = (*owner)[k];
            const Point2& to = (*owner)[(k + 1) % 3];
            const double ex = to.x - from.x;
            const double ey = to.y - from.y;
            const double length = std::hypot(ex, ey);
            if (!(length > 0.0)) {
                continue;
            }
            const double nx = -ey / length;
            const double ny = ex / length;
            // Relative to the edge's own end, so a survey coordinate's size
            // does not cost the projection its digits.
            const auto shadow = [&](const Tri& triangle) {
                double low = std::numeric_limits<double>::infinity();
                double high = -low;
                for (const Point2& corner : triangle) {
                    const double at = (corner.x - from.x) * nx + (corner.y - from.y) * ny;
                    low = std::min(low, at);
                    high = std::max(high, at);
                }
                return std::pair<double, double>(low, high);
            };
            const auto [lowA, highA] = shadow(first);
            const auto [lowB, highB] = shadow(second);
            if (!(std::min(highA, highB) - std::max(lowA, lowB) > depth)) {
                return false;
            }
        }
    }
    return true;
}

struct SideBySide {
    // The first stretch: the panels from `first` to `last`, a run of
    // consecutive panels that each overlap some other panel but their
    // neighbours.
    std::size_t first = 0;
    std::size_t last = 0;
    // The panels they overlap, the lowest to the highest.
    std::size_t partnerFirst = 0;
    std::size_t partnerLast = 0;
    // Pairs of panels that overlap, all told.
    std::size_t pairs = 0;
};

// Whether two panels that are not neighbours sweep common ground. Neighbours
// share a section, so they touch along it; a panel and the one after next are
// tested like any other, and clear one another when the sections tile the
// ground. `count` sections give count - 1 panels.
std::optional<SideBySide> firstSideBySide(const std::vector<SectionEnds>& ends, std::size_t count,
                                          katana::core::TaskPool& workers)
{
    if (count < 4) {
        return std::nullopt; // three panels are the fewest with two that are not neighbours
    }
    const std::size_t panels = count - 1;
    std::vector<Strip> strips(panels);
    std::vector<katana::geometry::SpatialEntry> entries;
    entries.reserve(panels);
    for (std::size_t p = 0; p < panels; ++p) {
        strips[p] = stripOf(ends[p], ends[p + 1]);
        if (strips[p].valid) {
            entries.push_back({static_cast<katana::geometry::SpatialId>(p), strips[p].box});
        }
    }
    katana::geometry::SpatialIndex index;
    index.rebuild(entries);

    struct Slot {
        std::size_t pairs = 0;
        std::size_t first = 0; // the lowest and highest partner above p + 1
        std::size_t last = 0;
    };
    std::vector<Slot> slots(panels);
    const auto scan = [&](std::size_t begin, std::size_t endIndex) {
        std::vector<katana::geometry::SpatialId> candidates;
        for (std::size_t p = begin; p < endIndex; ++p) {
            if (!strips[p].valid) {
                continue;
            }
            index.query(strips[p].box, candidates);
            for (const katana::geometry::SpatialId id : candidates) {
                const auto q = static_cast<std::size_t>(id);
                if (q < p + 2) {
                    continue; // itself, or a neighbour; the pair is counted from the lower
                }
                bool overlap = false;
                for (const Tri& a : strips[p].parts) {
                    for (const Tri& b : strips[q].parts) {
                        overlap = overlap || overlapsDeeperThan(a, b, tol::kGeometric);
                    }
                }
                if (!overlap) {
                    continue;
                }
                Slot& slot = slots[p];
                slot.first = slot.pairs == 0 ? q : std::min(slot.first, q);
                slot.last = slot.pairs == 0 ? q : std::max(slot.last, q);
                ++slot.pairs;
            }
        }
    };
    workers.parallelRanges(0, panels, 16, scan);

    // Combined in station order, so the stretches named are the same at any
    // thread count. A flag and a plain struct, not an optional edited in place:
    // GCC at -O3 has raised -Wmaybe-uninitialized on that.
    SideBySide found;
    bool any = false;
    for (std::size_t p = 0; p < panels; ++p) {
        if (slots[p].pairs == 0) {
            continue;
        }
        if (!any) {
            found = SideBySide{p, p, slots[p].first, slots[p].last, slots[p].pairs};
            any = true;
            continue;
        }
        found.pairs += slots[p].pairs;
        if (p == found.last + 1) {
            // The run goes on only while the panels are consecutive; once a
            // panel is missed no later one is next after `last`.
            found.last = p;
            found.partnerFirst = std::min(found.partnerFirst, slots[p].first);
            found.partnerLast = std::max(found.partnerLast, slots[p].last);
        }
    }
    if (!any) {
        return std::nullopt;
    }
    return found;
}

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
        return makeError(ErrorCode::InvalidGeometry,
                         "the existing surface covers " + boxText(existing.bounds()) +
                             " and the design covers " + boxText(design.bounds()) +
                             ": they have no ground in common");
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

    // Both ends of every section, once: the crossing test and the cutting
    // both read them.
    std::vector<SectionEnds> ends(cuts.size());
    for (std::size_t i = 0; i < cuts.size(); ++i) {
        const auto leftEnd = alignment.pointAtStationOffset(cuts[i], left);
        const auto rightEnd = alignment.pointAtStationOffset(cuts[i], -right);
        if (leftEnd && rightEnd) { // inside the range by construction
            ends[i] = SectionEnds{*leftEnd, *rightEnd, true};
        }
    }
    katana::core::TaskPool& workers = pool != nullptr ? *pool : katana::core::TaskPool::shared();
    if (const auto crossing = firstCrossing(ends, merged.size(), left, right, workers)) {
        const std::string nearest = std::format("{:.3f}", crossing->nearest);
        return makeError(
            ErrorCode::InvalidArgument,
            "the sections at stations " + std::format("{:.3f}", merged[crossing->first].station) +
                " and " + std::format("{:.3f}", merged[crossing->second].station) +
                " cross each other: sections that overlap count the ground they share more than "
                "once, so no volume can be given. Give left= and right= of at most " + nearest +
                " m (on a curve, no more than its radius; at a corner with no radius, no more "
                "than the interval over the sine of its deflection)",
            "left=,right=");
    }
    // No section crosses another, but two stretches may still run side by
    // side, their sections parallel and the ground between them swept by both.
    if (const auto sideBySide = firstSideBySide(ends, merged.size(), workers)) {
        const auto station = [&](std::size_t index) {
            return std::format("{:.3f}", merged[index].station);
        };
        return makeError(
            ErrorCode::InvalidArgument,
            "the stretch of the alignment from station " + station(sideBySide->first) + " to " +
                station(sideBySide->last + 1) + " runs side by side with the stretch from station " +
                station(sideBySide->partnerFirst) + " to " + station(sideBySide->partnerLast + 1) +
                ": the sections of the two sweep the same ground (" +
                std::to_string(sideBySide->pairs) +
                " pairs of panels overlap), which would be counted once for each, so no volume can "
                "be given. Shorten left= and right= until the sections of the two stretches clear "
                "one another (with the same reach each side, under half the distance between "
                "the stretches)",
            "left=,right=");
    }

    const SegmentProfiler under(existing);
    const SegmentProfiler over(design);
    std::vector<StationSection> sections(cuts.size());
    const auto cutSections = [&](std::size_t begin, std::size_t endIndex) {
        for (std::size_t i = begin; i < endIndex; ++i) {
            StationSection& section = sections[i];
            section.station = cuts[i];
            const auto centre = alignment.pointAtStation(cuts[i]);
            if (!centre || !ends[i].valid) {
                continue; // inside the range by construction; nothing to cut otherwise
            }
            section.centre = *centre;
            const Point2& leftEnd = ends[i].left;
            const Point2& rightEnd = ends[i].right;
            const double length = leftEnd.distanceTo(rightEnd);
            // The section runs from the left end, `left` from the centreline,
            // to the right end: its moments are about the centreline.
            const SectionAreas areas = sectionAreas(under.along(leftEnd, rightEnd),
                                                    over.along(leftEnd, rightEnd), length, left);
            section.cutArea = areas.cut;
            section.fillArea = areas.fill;
            section.cutMoment = areas.cutMoment;
            section.fillMoment = areas.fillMoment;
            section.width = areas.width;
        }
    };
    // A section is a few hundred triangle clips; four to a task keeps the
    // dispatch small beside them. The grain cannot change a result: each
    // section is computed on its own.
    workers.parallelRanges(0, cuts.size(), 4, cutSections);

    // Every other VOLUME form refuses surfaces that share no ground under what
    // it measures. Boxes that meet over triangles that do not, or an alignment
    // that runs clear of the common ground, measured nothing and would
    // otherwise answer a cut of 0 and a fill of 0.
    if (std::none_of(sections.begin(),
                     sections.begin() + static_cast<std::ptrdiff_t>(merged.size()),
                     [](const StationSection& section) { return section.width > 0.0; })) {
        return makeError(ErrorCode::InvalidGeometry,
                         "no section of the range reaches ground that both surfaces have (the "
                         "existing surface covers " +
                             boxText(existing.bounds()) + ", the design " +
                             boxText(design.bounds()) +
                             "): the alignment runs clear of it, or the two share no triangle "
                             "there");
    }

    AlignmentVolumes result;
    result.fromStation = first;
    result.toStation = last;
    result.left = left;
    result.right = right;
    result.prismoidal = options.prismoidal;
    result.curvature = options.curvature;
    for (std::size_t i = 0; i < merged.size(); ++i) {
        sections[i].key = merged[i].key;
        result.sections.push_back(sections[i]);
    }
    detail::CompensatedSum endCut;
    detail::CompensatedSum endFill;
    detail::CompensatedSum prismCut;
    detail::CompensatedSum prismFill;
    detail::CompensatedSum endCutShift;
    detail::CompensatedSum endFillShift;
    detail::CompensatedSum prismCutShift;
    detail::CompensatedSum prismFillShift;
    for (std::size_t i = 0; i + 1 < result.sections.size(); ++i) {
        const StationSection& a = result.sections[i];
        const StationSection& b = result.sections[i + 1];
        SectionPanel panel;
        panel.fromStation = a.station;
        panel.toStation = b.station;
        const double length = b.station - a.station;
        // The curvature at the middle of the panel: a panel lies on one
        // element of the alignment (its key stations are all sectioned), so
        // that is the arc's own, 0 on a tangent, and on a spiral the mean of
        // its two ends'. Where the alignment kinks (a PI with no radius) there
        // is no curvature to speak of and 0 is what it has on either side.
        const double bend =
            options.curvature
                ? alignment.curvatureAtStation(0.5 * (a.station + b.station)).value_or(0.0)
                : 0.0;
        panel.curvature = bend;
        // Each section carries A - k M in the place of its area A (see the
        // header): plain when k is 0, which is the same expression as ever.
        panel.endAreaCut = length * (a.cutArea + b.cutArea) / 2.0;
        panel.endAreaFill = length * (a.fillArea + b.fillArea) / 2.0;
        if (bend != 0.0) {
            const double cut =
                length * ((a.cutArea + b.cutArea) - bend * (a.cutMoment + b.cutMoment)) / 2.0;
            const double fill =
                length * ((a.fillArea + b.fillArea) - bend * (a.fillMoment + b.fillMoment)) / 2.0;
            panel.endAreaShift = CurvatureShift{cut - panel.endAreaCut, fill - panel.endAreaFill};
            panel.endAreaCut = cut;
            panel.endAreaFill = fill;
        }
        endCut.add(panel.endAreaCut);
        endFill.add(panel.endAreaFill);
        endCutShift.add(panel.endAreaShift.cut);
        endFillShift.add(panel.endAreaShift.fill);
        if (options.prismoidal) {
            const StationSection& m = sections[merged.size() + i];
            panel.middle = m;
            panel.prismoidalCut = length * (a.cutArea + 4.0 * m.cutArea + b.cutArea) / 6.0;
            panel.prismoidalFill = length * (a.fillArea + 4.0 * m.fillArea + b.fillArea) / 6.0;
            if (bend != 0.0) {
                const double cut =
                    length *
                    ((a.cutArea + 4.0 * m.cutArea + b.cutArea) -
                     bend * (a.cutMoment + 4.0 * m.cutMoment + b.cutMoment)) /
                    6.0;
                const double fill =
                    length *
                    ((a.fillArea + 4.0 * m.fillArea + b.fillArea) -
                     bend * (a.fillMoment + 4.0 * m.fillMoment + b.fillMoment)) /
                    6.0;
                panel.prismoidalShift =
                    CurvatureShift{cut - panel.prismoidalCut, fill - panel.prismoidalFill};
                panel.prismoidalCut = cut;
                panel.prismoidalFill = fill;
            }
            prismCut.add(panel.prismoidalCut);
            prismFill.add(panel.prismoidalFill);
            prismCutShift.add(panel.prismoidalShift.cut);
            prismFillShift.add(panel.prismoidalShift.fill);
        }
        result.panels.push_back(panel);
    }
    result.endAreaCut = endCut.value();
    result.endAreaFill = endFill.value();
    result.prismoidalCut = prismCut.value();
    result.prismoidalFill = prismFill.value();
    result.endAreaShift = CurvatureShift{endCutShift.value(), endFillShift.value()};
    result.prismoidalShift = CurvatureShift{prismCutShift.value(), prismFillShift.value()};
    return result;
}

} // namespace katana::terrain
