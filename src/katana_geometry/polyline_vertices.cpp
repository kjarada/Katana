#include "katana/geometry/polyline_vertices.hpp"

#include <algorithm>
#include <cmath>
#include <limits>
#include <string>
#include <utility>

#include "katana/geometry/chording.hpp"
#include "katana/core/text.hpp"
#include "katana/math/numerics.hpp"

namespace katana::geometry {

namespace tol = katana::math::tolerance;
using katana::core::ErrorCode;
using katana::core::makeError;

namespace {

katana::core::Error invalid(std::string why)
{
    return katana::core::Error{ErrorCode::InvalidArgument, std::move(why), {}};
}

katana::core::Error degenerate(std::string why)
{
    return katana::core::Error{ErrorCode::InvalidGeometry, std::move(why), {}};
}

bool coincide(const Point2& a, const Point2& b) { return a.distanceTo(b) <= tol::kGeometric; }

bool straight(double bulge) { return std::abs(bulge) <= 1.0e-12; }

std::optional<double> lerpHeight(const std::optional<double>& a, const std::optional<double>& b,
                                 double t)
{
    if (!a || !b) {
        return std::nullopt;
    }
    return *a + (*b - *a) * t;
}

std::string vertexRange(const CurvePolyline2& polyline)
{
    return "0 to " + std::to_string(polyline.vertices.size() - 1);
}

katana::core::Status checkVertex(const CurvePolyline2& polyline, std::size_t index)
{
    if (polyline.vertices.empty()) {
        return invalid("the polyline has no vertices");
    }
    if (index >= polyline.vertices.size()) {
        return invalid("there is no vertex " + std::to_string(index) + "; the vertices are " +
                       vertexRange(polyline));
    }
    return {};
}

katana::core::Status checkSegment(const CurvePolyline2& polyline, std::size_t segment)
{
    const std::size_t count = polyline.segmentCount();
    if (segment >= count) {
        return invalid(count == 0 ? std::string("the polyline has no segments")
                                  : "there is no segment " + std::to_string(segment) +
                                        "; the segments are 0 to " + std::to_string(count - 1));
    }
    return {};
}

std::size_t previousVertex(const CurvePolyline2& polyline, std::size_t index)
{
    return index == 0 ? polyline.vertices.size() - 1 : index - 1;
}

// The fraction along a segment at which `p` lies: the projection onto a
// straight segment, the angle travelled on an arc.
double fractionAlong(const CurveSegment& piece, const Point2& p)
{
    if (const auto* line = std::get_if<Segment2>(&piece)) {
        return line->parameterOf(p);
    }
    const Arc2& arc = std::get<Arc2>(piece);
    return arc.parameterOfAngle((p - arc.center).angle());
}

} // namespace

std::size_t minimumVertices(const CurvePolyline2& polyline) { return polyline.closed ? 3 : 2; }

std::optional<std::size_t> nearestVertex(const CurvePolyline2& polyline, const Point2& p)
{
    std::optional<std::size_t> best;
    double bestDistance = std::numeric_limits<double>::infinity();
    for (std::size_t i = 0; i < polyline.vertices.size(); ++i) {
        const double d = polyline.vertices[i].position.distanceTo(p);
        if (d < bestDistance) {
            bestDistance = d;
            best = i;
        }
    }
    return best;
}

std::optional<std::size_t> nearestSegment(const CurvePolyline2& polyline, const Point2& p)
{
    std::optional<std::size_t> best;
    double bestDistance = std::numeric_limits<double>::infinity();
    for (std::size_t i = 0; i < polyline.segmentCount(); ++i) {
        const double d =
            std::visit([&p](const auto& piece) { return piece.distanceTo(p); }, polyline.segment(i));
        if (d < bestDistance) {
            bestDistance = d;
            best = i;
        }
    }
    return best;
}

// ---- one vertex ------------------------------------------------------------------------

PolylineResult insertVertex(const CurvePolyline2& polyline, std::size_t segment, const Point2& at,
                            std::optional<double> height)
{
    if (auto status = checkSegment(polyline, segment); !status) {
        return status.error();
    }
    if (!at.isFinite()) {
        return invalid("the new vertex is not a finite point");
    }
    CurvePolyline2 out = polyline;
    const std::size_t end = polyline.segmentEnd(segment);
    const CurveVertex& a = polyline.vertices[segment];
    const CurveVertex& b = polyline.vertices[end];
    const CurveSegment piece = polyline.segment(segment);

    CurveVertex added;
    double t = 0.0;
    if (const auto* arc = std::get_if<Arc2>(&piece)) {
        // On the arc, so the two halves are the same circle: each takes its
        // share of the sweep as its bulge.
        added.position = arc->closestPoint(at);
        t = fractionAlong(piece, added.position);
        out.vertices[segment].bulge = bulgeFromSweep(arc->sweep * t);
        added.bulge = bulgeFromSweep(arc->sweep * (1.0 - t));
    } else {
        added.position = at;
        t = std::clamp(fractionAlong(piece, at), 0.0, 1.0);
    }
    if (coincide(added.position, a.position) || coincide(added.position, b.position)) {
        return invalid("the point is on a vertex already; a new vertex must be between two");
    }
    added.height = height ? height : lerpHeight(a.height, b.height, t);
    out.vertices.insert(out.vertices.begin() + static_cast<std::ptrdiff_t>(segment + 1), added);
    return out;
}

PolylineResult deleteVertex(const CurvePolyline2& polyline, std::size_t index)
{
    if (auto status = checkVertex(polyline, index); !status) {
        return status.error();
    }
    if (polyline.vertices.size() <= minimumVertices(polyline)) {
        return degenerate(std::string("a ") + (polyline.closed ? "closed " : "") +
                          "polyline needs at least " + std::to_string(minimumVertices(polyline)) +
                          " vertices; this one has no vertex to spare");
    }
    CurvePolyline2 out = polyline;
    const std::size_t n = polyline.vertices.size();
    if (polyline.closed || (index > 0 && index + 1 < n)) {
        // The segments either side become one straight segment.
        out.vertices[previousVertex(polyline, index)].bulge = 0.0;
    }
    out.vertices.erase(out.vertices.begin() + static_cast<std::ptrdiff_t>(index));
    if (!out.closed) {
        out.vertices.back().bulge = 0.0;
    }
    return out;
}

PolylineResult deleteVertices(const CurvePolyline2& polyline, std::vector<std::size_t> indices)
{
    std::sort(indices.begin(), indices.end());
    indices.erase(std::unique(indices.begin(), indices.end()), indices.end());
    if (indices.empty()) {
        return invalid("no vertex was given to delete");
    }
    for (const std::size_t index : indices) {
        if (auto status = checkVertex(polyline, index); !status) {
            return status.error();
        }
    }
    if (polyline.vertices.size() < indices.size() + minimumVertices(polyline)) {
        return degenerate("deleting " + std::to_string(indices.size()) + " vertices would leave " +
                          std::to_string(polyline.vertices.size() - indices.size()) +
                          "; a polyline needs at least " +
                          std::to_string(minimumVertices(polyline)));
    }
    CurvePolyline2 out = polyline;
    // Highest first, so the indices still to delete do not shift.
    for (auto it = indices.rbegin(); it != indices.rend(); ++it) {
        auto next = deleteVertex(out, *it);
        if (!next) {
            return next.error();
        }
        out = std::move(*next);
    }
    return out;
}

PolylineResult moveVertex(const CurvePolyline2& polyline, std::size_t index, const Point2& to)
{
    if (auto status = checkVertex(polyline, index); !status) {
        return status.error();
    }
    if (!to.isFinite()) {
        return invalid("the vertex cannot be moved to a point that is not finite");
    }
    CurvePolyline2 out = polyline;
    out.vertices[index].position = to;
    return out;
}

PolylineResult setVertexHeight(const CurvePolyline2& polyline, std::size_t index,
                               std::optional<double> height)
{
    if (auto status = checkVertex(polyline, index); !status) {
        return status.error();
    }
    if (height && !std::isfinite(*height)) {
        return invalid("a height must be a finite number");
    }
    CurvePolyline2 out = polyline;
    out.vertices[index].height = height;
    return out;
}

PolylineResult setSegmentBulge(const CurvePolyline2& polyline, std::size_t segment, double bulge)
{
    if (auto status = checkSegment(polyline, segment); !status) {
        return status.error();
    }
    if (!std::isfinite(bulge)) {
        return invalid("a bulge must be a finite number");
    }
    CurvePolyline2 out = polyline;
    out.vertices[segment].bulge = bulge;
    return out;
}

// ---- segments ---------------------------------------------------------------------------

PolylineResult segmentToArc(const CurvePolyline2& polyline, std::size_t segment,
                            const Point2& through)
{
    if (auto status = checkSegment(polyline, segment); !status) {
        return status.error();
    }
    const Point2& a = polyline.vertices[segment].position;
    const Point2& b = polyline.vertices[polyline.segmentEnd(segment)].position;
    const double bulge = bulgeThrough(a, through, b);
    if (straight(bulge)) {
        return invalid("the point is in line with the segment's ends; an arc needs a point "
                       "off the line");
    }
    return setSegmentBulge(polyline, segment, bulge);
}

PolylineResult segmentToLine(const CurvePolyline2& polyline, std::size_t segment)
{
    if (auto status = checkSegment(polyline, segment); !status) {
        return status.error();
    }
    if (straight(polyline.vertices[segment].bulge)) {
        return invalid("segment " + std::to_string(segment) + " is already straight");
    }
    return setSegmentBulge(polyline, segment, 0.0);
}

PolylineResult moveSegment(const CurvePolyline2& polyline, std::size_t segment, const Vec2& delta)
{
    if (auto status = checkSegment(polyline, segment); !status) {
        return status.error();
    }
    if (!delta.isFinite()) {
        return invalid("the displacement is not finite");
    }
    CurvePolyline2 out = polyline;
    out.vertices[segment].position += delta;
    out.vertices[polyline.segmentEnd(segment)].position += delta;
    return out;
}

// ---- ranges ------------------------------------------------------------------------------

std::vector<std::size_t> verticesBetween(const CurvePolyline2& polyline, std::size_t from,
                                         std::size_t to)
{
    std::vector<std::size_t> out;
    const std::size_t n = polyline.vertices.size();
    if (from >= n || to >= n || from == to) {
        return out;
    }
    if (!polyline.closed) {
        const std::size_t lo = std::min(from, to);
        const std::size_t hi = std::max(from, to);
        for (std::size_t i = lo + 1; i < hi; ++i) {
            out.push_back(i);
        }
        return out;
    }
    for (std::size_t i = (from + 1) % n; i != to; i = (i + 1) % n) {
        out.push_back(i);
    }
    return out;
}

namespace {

// Checks the pair and orders an open polyline's so that `from` < `to`.
katana::core::Status checkRange(const CurvePolyline2& polyline, std::size_t& from,
                                std::size_t& to)
{
    if (auto status = checkVertex(polyline, from); !status) {
        return status;
    }
    if (auto status = checkVertex(polyline, to); !status) {
        return status;
    }
    if (from == to) {
        return invalid("the two vertices are the same vertex; pick two different ones");
    }
    if (!polyline.closed && from > to) {
        std::swap(from, to);
    }
    return {};
}

} // namespace

PolylineResult straighten(const CurvePolyline2& polyline, std::size_t from, std::size_t to)
{
    if (auto status = checkRange(polyline, from, to); !status) {
        return status.error();
    }
    const std::vector<std::size_t> between = verticesBetween(polyline, from, to);
    if (polyline.vertices.size() - between.size() < minimumVertices(polyline)) {
        return degenerate("straightening would leave fewer than " +
                          std::to_string(minimumVertices(polyline)) + " vertices");
    }
    const std::size_t segmentFrom = from;
    if (between.empty() && straight(polyline.vertices[segmentFrom].bulge)) {
        return invalid("the vertices are already joined by one straight segment");
    }
    CurvePolyline2 out;
    out.closed = polyline.closed;
    for (std::size_t i = 0; i < polyline.vertices.size(); ++i) {
        if (std::find(between.begin(), between.end(), i) != between.end()) {
            continue;
        }
        CurveVertex v = polyline.vertices[i];
        if (i == segmentFrom) {
            v.bulge = 0.0;
        }
        out.vertices.push_back(v);
    }
    return out;
}

namespace {

// The lengths walked forward from `from` to each vertex up to `to`, in walk
// order: first the vertex after `from`, last `to` itself.
std::vector<std::pair<std::size_t, double>> walk(const CurvePolyline2& polyline, std::size_t from,
                                                 std::size_t to)
{
    std::vector<std::pair<std::size_t, double>> out;
    const std::size_t n = polyline.vertices.size();
    double walked = 0.0;
    for (std::size_t i = from; i != to; i = (i + 1) % n) {
        walked += polyline.segmentLength(i);
        out.emplace_back((i + 1) % n, walked);
    }
    return out;
}

} // namespace

PolylineResult gradeBetween(const CurvePolyline2& polyline, std::size_t from, std::size_t to)
{
    if (auto status = checkRange(polyline, from, to); !status) {
        return status.error();
    }
    const auto& h0 = polyline.vertices[from].height;
    const auto& h1 = polyline.vertices[to].height;
    if (!h0 || !h1) {
        return invalid("both vertices need a height to grade between them; vertex " +
                       std::to_string(!h0 ? from : to) + " has none");
    }
    const auto steps = walk(polyline, from, to);
    const double total = steps.empty() ? 0.0 : steps.back().second;
    if (!(total > tol::kGeometric)) {
        return degenerate("the two vertices are at the same place along the polyline");
    }
    CurvePolyline2 out = polyline;
    for (const auto& [index, station] : steps) {
        if (index != to) {
            out.vertices[index].height = *h0 + (*h1 - *h0) * station / total;
        }
    }
    return out;
}

PolylineResult interpolateHeights(const CurvePolyline2& polyline)
{
    std::vector<std::size_t> known;
    for (std::size_t i = 0; i < polyline.vertices.size(); ++i) {
        if (polyline.vertices[i].height) {
            known.push_back(i);
        }
    }
    if (known.size() < 2) {
        return invalid("interpolating heights needs at least two vertices with heights");
    }
    CurvePolyline2 out = polyline;
    const auto fill = [&](std::size_t from, std::size_t to) {
        const auto steps = walk(polyline, from, to);
        const double total = steps.empty() ? 0.0 : steps.back().second;
        if (!(total > 0.0)) {
            return;
        }
        const double h0 = *polyline.vertices[from].height;
        const double h1 = *polyline.vertices[to].height;
        for (const auto& [index, station] : steps) {
            if (index != to) {
                out.vertices[index].height = h0 + (h1 - h0) * station / total;
            }
        }
    };
    for (std::size_t k = 1; k < known.size(); ++k) {
        fill(known[k - 1], known[k]);
    }
    if (polyline.closed) {
        fill(known.back(), known.front());
    }
    return out;
}

// ---- whole polyline ---------------------------------------------------------------------------

namespace {

// Douglas-Peucker over walk positions [first, last] of `order` (indices into
// the polyline's vertices), marking what survives in `kept`.
void peucker(const CurvePolyline2& polyline, const std::vector<std::size_t>& order,
             std::size_t first, std::size_t last, double tolerance, std::vector<bool>& kept)
{
    if (last <= first + 1) {
        return;
    }
    const CurveVertex& a = polyline.vertices[order[first]];
    const CurveVertex& b = polyline.vertices[order[last]];
    const Segment2 chord{a.position, b.position};
    double worst = -1.0;
    std::size_t worstAt = first;
    for (std::size_t k = first + 1; k < last; ++k) {
        const CurveVertex& v = polyline.vertices[order[k]];
        double deviation = chord.distanceTo(v.position);
        if (v.height && a.height && b.height) {
            const double t = chord.parameterOf(v.position);
            deviation = std::max(deviation, std::abs(*v.height - (*a.height + (*b.height - *a.height) * t)));
        }
        if (kept[order[k]]) {
            // A vertex that must stay splits the run whatever its deviation.
            deviation = std::numeric_limits<double>::infinity();
        }
        if (deviation > worst) {
            worst = deviation;
            worstAt = k;
        }
    }
    if (worst > tolerance) {
        kept[order[worstAt]] = true;
        peucker(polyline, order, first, worstAt, tolerance, kept);
        peucker(polyline, order, worstAt, last, tolerance, kept);
    }
}

} // namespace

PolylineResult weed(const CurvePolyline2& polyline, double tolerance, const std::vector<bool>& keep)
{
    if (!(tolerance >= 0.0) || !std::isfinite(tolerance)) {
        return invalid("the weed tolerance must be a number of zero or more");
    }
    const std::size_t n = polyline.vertices.size();
    if (n <= minimumVertices(polyline)) {
        return polyline;
    }
    std::vector<bool> kept(n, false);
    for (std::size_t i = 0; i < n && i < keep.size(); ++i) {
        kept[i] = keep[i];
    }
    // An arc's shape is its two ends and its bulge, so both ends stay.
    for (std::size_t i = 0; i < polyline.segmentCount(); ++i) {
        if (!straight(polyline.vertices[i].bulge)) {
            kept[i] = true;
            kept[polyline.segmentEnd(i)] = true;
        }
    }
    std::vector<std::size_t> order;
    if (!polyline.closed) {
        kept.front() = true;
        kept.back() = true;
        for (std::size_t i = 0; i < n; ++i) {
            order.push_back(i);
        }
    } else {
        // A ring has no ends: start from a vertex that stays, or from vertex
        // 0 and the vertex farthest from it, which any simplification keeps.
        std::size_t start = 0;
        const auto firstKept = std::find(kept.begin(), kept.end(), true);
        if (firstKept != kept.end()) {
            start = static_cast<std::size_t>(firstKept - kept.begin());
        }
        kept[start] = true;
        std::size_t far = start;
        double farDistance = -1.0;
        for (std::size_t i = 0; i < n; ++i) {
            const double d = polyline.vertices[i].position.distanceTo(polyline.vertices[start].position);
            if (d > farDistance) {
                farDistance = d;
                far = i;
            }
        }
        kept[far] = true;
        for (std::size_t k = 0; k <= n; ++k) {
            order.push_back((start + k) % n);
        }
    }
    // Split the walk at every vertex that stays and simplify each run.
    std::size_t runStart = 0;
    for (std::size_t k = 1; k < order.size(); ++k) {
        if (kept[order[k]] || k + 1 == order.size()) {
            peucker(polyline, order, runStart, k, tolerance, kept);
            runStart = k;
        }
    }
    if (polyline.closed && std::count(kept.begin(), kept.end(), true) < 3) {
        // Two vertices do not make a ring: put back the one that departs
        // most from the line between them.
        std::size_t best = 0;
        double bestDistance = -1.0;
        std::vector<std::size_t> stay;
        for (std::size_t i = 0; i < n; ++i) {
            if (kept[i]) {
                stay.push_back(i);
            }
        }
        const Segment2 chord{polyline.vertices[stay.front()].position,
                             polyline.vertices[stay.back()].position};
        for (std::size_t i = 0; i < n; ++i) {
            if (!kept[i] && chord.distanceTo(polyline.vertices[i].position) > bestDistance) {
                bestDistance = chord.distanceTo(polyline.vertices[i].position);
                best = i;
            }
        }
        kept[best] = true;
    }
    CurvePolyline2 out;
    out.closed = polyline.closed;
    for (std::size_t i = 0; i < n; ++i) {
        if (kept[i]) {
            out.vertices.push_back(polyline.vertices[i]);
        }
    }
    return out;
}

PolylineResult densify(const CurvePolyline2& polyline, double step, double chordTolerance)
{
    if (!(step >= 0.0) || !std::isfinite(step)) {
        return invalid("the densify interval must be a number of zero or more");
    }
    if (!(chordTolerance >= 0.0) || !std::isfinite(chordTolerance)) {
        return invalid("the chord tolerance must be a number of zero or more");
    }
    if (step == 0.0 && chordTolerance == 0.0) {
        return invalid("give an interval, a chord tolerance, or both");
    }
    CurvePolyline2 out;
    out.closed = polyline.closed;
    for (std::size_t i = 0; i < polyline.segmentCount(); ++i) {
        const CurveVertex& a = polyline.vertices[i];
        const CurveVertex& b = polyline.vertices[polyline.segmentEnd(i)];
        const CurveSegment piece = polyline.segment(i);
        const double length = std::visit([](const auto& s) { return s.length(); }, piece);
        if (const auto* arc = std::get_if<Arc2>(&piece)) {
            if (chordTolerance > 0.0) {
                // Chords, each then divided by the interval.
                const std::vector<Point2> chords = chordArc(*arc, chordTolerance);
                for (std::size_t k = 0; k + 1 < chords.size(); ++k) {
                    const Point2 p = k == 0 ? a.position : chords[k];
                    const Point2 q = k + 2 == chords.size() ? b.position : chords[k + 1];
                    const double t0 = static_cast<double>(k) / static_cast<double>(chords.size() - 1);
                    const double t1 = static_cast<double>(k + 1) / static_cast<double>(chords.size() - 1);
                    const double chordLength = p.distanceTo(q);
                    const std::size_t parts =
                        step > 0.0 ? std::max<std::size_t>(1, static_cast<std::size_t>(std::ceil(chordLength / step - 1e-9))) : 1;
                    for (std::size_t m = 0; m < parts; ++m) {
                        const double f = static_cast<double>(m) / static_cast<double>(parts);
                        CurveVertex v{p + (q - p) * f, 0.0,
                                      lerpHeight(a.height, b.height, t0 + (t1 - t0) * f)};
                        if (k == 0 && m == 0) {
                            v.position = a.position;
                            v.height = a.height;
                        }
                        out.vertices.push_back(v);
                    }
                }
                continue;
            }
            // Equal sub-arcs of the same circle, no longer than the interval.
            const std::size_t parts =
                step > 0.0 ? std::max<std::size_t>(1, static_cast<std::size_t>(std::ceil(length / step - 1e-9))) : 1;
            const double bulge = bulgeFromSweep(arc->sweep / static_cast<double>(parts));
            for (std::size_t m = 0; m < parts; ++m) {
                const double f = static_cast<double>(m) / static_cast<double>(parts);
                CurveVertex v{m == 0 ? a.position : arc->pointAt(f), bulge,
                              m == 0 ? a.height : lerpHeight(a.height, b.height, f)};
                out.vertices.push_back(v);
            }
            continue;
        }
        const std::size_t parts =
            step > 0.0 ? std::max<std::size_t>(1, static_cast<std::size_t>(std::ceil(length / step - 1e-9))) : 1;
        for (std::size_t m = 0; m < parts; ++m) {
            const double f = static_cast<double>(m) / static_cast<double>(parts);
            CurveVertex v{m == 0 ? a.position : a.position + (b.position - a.position) * f, 0.0,
                          m == 0 ? a.height : lerpHeight(a.height, b.height, f)};
            out.vertices.push_back(v);
        }
    }
    if (!polyline.closed && !polyline.vertices.empty()) {
        CurveVertex last = polyline.vertices.back();
        last.bulge = 0.0;
        out.vertices.push_back(last);
    }
    return out;
}

PolylineResult mergeNearVertices(const CurvePolyline2& polyline, double tolerance)
{
    if (!(tolerance >= 0.0) || !std::isfinite(tolerance)) {
        return invalid("the merge tolerance must be a number of zero or more");
    }
    CurvePolyline2 out;
    out.closed = polyline.closed;
    for (const CurveVertex& v : polyline.vertices) {
        if (!out.vertices.empty() &&
            out.vertices.back().position.distanceTo(v.position) <= tolerance) {
            // Merged into the first of the cluster, which takes the bulge of
            // the segment that now leaves the cluster.
            out.vertices.back().bulge = v.bulge;
            if (!out.vertices.back().height) {
                out.vertices.back().height = v.height;
            }
            continue;
        }
        out.vertices.push_back(v);
    }
    if (out.closed) {
        while (out.vertices.size() > 1 &&
               out.vertices.back().position.distanceTo(out.vertices.front().position) <= tolerance) {
            out.vertices.pop_back();
        }
    }
    if (out.vertices.size() < minimumVertices(polyline)) {
        return degenerate("merging within " + katana::core::formatExactReal(tolerance) +
                          " would leave fewer than " + std::to_string(minimumVertices(polyline)) +
                          " vertices");
    }
    return out;
}

PolylineResult snapToGrid(const CurvePolyline2& polyline, double spacing, const Point2& origin)
{
    if (!(spacing > tol::kGeometric) || !std::isfinite(spacing)) {
        return invalid("the grid spacing must be greater than zero");
    }
    CurvePolyline2 out = polyline;
    for (CurveVertex& v : out.vertices) {
        const Vec2 d = v.position - origin;
        v.position = origin + Vec2(std::round(d.x / spacing) * spacing,
                                   std::round(d.y / spacing) * spacing);
    }
    return mergeNearVertices(out, tol::kGeometric);
}

PolylineResult closePolyline(const CurvePolyline2& polyline)
{
    if (polyline.closed) {
        return invalid("the polyline is already closed");
    }
    CurvePolyline2 out = polyline;
    if (out.vertices.size() > 1 &&
        coincide(out.vertices.back().position, out.vertices.front().position)) {
        // The last vertex already sits on the first: closing means dropping it.
        const auto height = out.vertices.back().height;
        out.vertices.pop_back();
        if (!out.vertices.front().height) {
            out.vertices.front().height = height;
        }
    } else {
        out.vertices.back().bulge = 0.0;
    }
    out.closed = true;
    if (out.vertices.size() < 3) {
        return degenerate("a closed polyline needs three different vertices");
    }
    return out;
}

PolylineResult openPolyline(const CurvePolyline2& polyline)
{
    if (!polyline.closed) {
        return invalid("the polyline is already open");
    }
    CurvePolyline2 out = polyline;
    out.closed = false;
    out.vertices.back().bulge = 0.0;
    return out;
}

PolylineResult changeStartVertex(const CurvePolyline2& polyline, std::size_t index)
{
    if (!polyline.closed) {
        return invalid("only a closed polyline can start at another vertex; an open one "
                       "starts at its end (use Reverse to start at the other)");
    }
    if (auto status = checkVertex(polyline, index); !status) {
        return status.error();
    }
    if (index == 0) {
        return invalid("vertex 0 is already the start");
    }
    CurvePolyline2 out = polyline;
    std::rotate(out.vertices.begin(), out.vertices.begin() + static_cast<std::ptrdiff_t>(index),
                out.vertices.end());
    return out;
}

// ---- corners ----------------------------------------------------------------------------------

namespace {

struct Corner {
    std::size_t previous = 0;
    std::size_t next = 0;
    Vec2 back;    // unit, from the vertex towards the previous one
    Vec2 forward; // unit, from the vertex towards the next one
    double backLength = 0.0;
    double forwardLength = 0.0;
};

katana::core::Result<Corner> cornerAt(const CurvePolyline2& polyline, std::size_t index)
{
    if (auto status = checkVertex(polyline, index); !status) {
        return status.error();
    }
    const std::size_t n = polyline.vertices.size();
    if (!polyline.closed && (index == 0 || index + 1 == n)) {
        return invalid("vertex " + std::to_string(index) +
                       " is an end of the polyline; a corner is where two segments meet");
    }
    Corner corner;
    corner.previous = previousVertex(polyline, index);
    corner.next = (index + 1) % n;
    if (!straight(polyline.vertices[corner.previous].bulge) ||
        !straight(polyline.vertices[index].bulge)) {
        return invalid("vertex " + std::to_string(index) +
                       " is beside an arc; only a corner between two straight segments can be "
                       "rounded or cut");
    }
    const Point2& v = polyline.vertices[index].position;
    const Vec2 back = polyline.vertices[corner.previous].position - v;
    const Vec2 forward = polyline.vertices[corner.next].position - v;
    corner.backLength = back.length();
    corner.forwardLength = forward.length();
    if (!(corner.backLength > tol::kGeometric) || !(corner.forwardLength > tol::kGeometric)) {
        return degenerate("a segment at vertex " + std::to_string(index) + " has no length");
    }
    corner.back = back / corner.backLength;
    corner.forward = forward / corner.forwardLength;
    if (std::abs(corner.back.cross(corner.forward)) <= tol::kAngular) {
        return invalid("the segments at vertex " + std::to_string(index) +
                       " are in line; there is no corner to round or cut");
    }
    return corner;
}

// Replaces vertex `index` with `first` and `second`, `first` starting the
// segment `bulge` describes. A point that lands on a neighbouring vertex is
// not doubled: the neighbour takes its role.
CurvePolyline2 replaceCorner(const CurvePolyline2& polyline, std::size_t index,
                             const Corner& corner, const Point2& first, const Point2& second,
                             double bulge)
{
    const CurveVertex& v = polyline.vertices[index];
    const CurveVertex& previous = polyline.vertices[corner.previous];
    const CurveVertex& next = polyline.vertices[corner.next];
    const double t1 = 1.0 - v.position.distanceTo(first) / corner.backLength;
    const double t2 = v.position.distanceTo(second) / corner.forwardLength;

    CurvePolyline2 out = polyline;
    std::vector<CurveVertex> replacement;
    const bool firstOnPrevious = coincide(first, previous.position);
    const bool secondOnNext = coincide(second, next.position);
    if (firstOnPrevious) {
        out.vertices[corner.previous].bulge = bulge;
    } else {
        replacement.push_back(CurveVertex{first, bulge, lerpHeight(previous.height, v.height, t1)});
    }
    if (!secondOnNext) {
        replacement.push_back(CurveVertex{second, 0.0, lerpHeight(v.height, next.height, t2)});
    }
    out.vertices.erase(out.vertices.begin() + static_cast<std::ptrdiff_t>(index));
    out.vertices.insert(out.vertices.begin() + static_cast<std::ptrdiff_t>(index),
                        replacement.begin(), replacement.end());
    return out;
}

} // namespace

katana::core::Status checkCorner(const CurvePolyline2& polyline, std::size_t index)
{
    // cornerAt's own checks, so the pick and the edit refuse with one sentence.
    if (auto corner = cornerAt(polyline, index); !corner) {
        return corner.error();
    }
    return {};
}

PolylineResult filletVertex(const CurvePolyline2& polyline, std::size_t index, double radius)
{
    if (!(radius > tol::kGeometric) || !std::isfinite(radius)) {
        return invalid("the fillet radius must be greater than zero");
    }
    auto corner = cornerAt(polyline, index);
    if (!corner) {
        return corner.error();
    }
    // The tangent points lie r / tan(phi / 2) back along each segment, phi
    // the angle between them at the vertex.
    const double phi = std::acos(std::clamp(corner->back.dot(corner->forward), -1.0, 1.0));
    const double reach = radius / std::tan(0.5 * phi);
    if (reach > corner->backLength + tol::kGeometric ||
        reach > corner->forwardLength + tol::kGeometric) {
        return invalid("a radius of " + katana::core::formatExactReal(radius) +
                       " does not fit at vertex " + std::to_string(index) + "; it needs " +
                       katana::core::formatExactReal(reach) + " of each segment");
    }
    const Point2& v = polyline.vertices[index].position;
    const Point2 first = v + corner->back * std::min(reach, corner->backLength);
    const Point2 second = v + corner->forward * std::min(reach, corner->forwardLength);
    // The arc turns the way the path turns at the vertex, through the
    // exterior angle.
    const Vec2 incoming = corner->back * -1.0;
    const double turn = incoming.cross(corner->forward) > 0.0 ? 1.0 : -1.0;
    const double sweep = turn * (katana::math::kPi - phi);
    return replaceCorner(polyline, index, *corner, first, second, bulgeFromSweep(sweep));
}

PolylineResult chamferVertex(const CurvePolyline2& polyline, std::size_t index, double first,
                             double second)
{
    if (!(first > tol::kGeometric) || !(second > tol::kGeometric) || !std::isfinite(first) ||
        !std::isfinite(second)) {
        return invalid("both chamfer distances must be greater than zero");
    }
    auto corner = cornerAt(polyline, index);
    if (!corner) {
        return corner.error();
    }
    if (first > corner->backLength + tol::kGeometric ||
        second > corner->forwardLength + tol::kGeometric) {
        return invalid("the chamfer is longer than a segment at vertex " + std::to_string(index));
    }
    const Point2& v = polyline.vertices[index].position;
    return replaceCorner(polyline, index, *corner,
                         v + corner->back * std::min(first, corner->backLength),
                         v + corner->forward * std::min(second, corner->forwardLength), 0.0);
}

// ---- sub-paths ------------------------------------------------------------------------------

namespace {

// The height a fraction `t` along segment `i`: a vertex's own at either end,
// else interpolated when both ends have one.
std::optional<double> heightOnSegment(const CurvePolyline2& polyline, std::size_t i, double t)
{
    const auto& h0 = polyline.vertices[i].height;
    const auto& h1 = polyline.vertices[polyline.segmentEnd(i)].height;
    if (t <= 0.0) {
        return h0;
    }
    if (t >= 1.0) {
        return h1;
    }
    if (h0 && h1) {
        return *h0 + (*h1 - *h0) * t;
    }
    return std::nullopt;
}

Point2 pointOnSegment(const CurveSegment& piece, double t)
{
    return std::visit([t](const auto& s) { return s.pointAt(t); }, piece);
}

} // namespace

CurvePolyline2 subPath(const CurvePolyline2& polyline, double from, double to)
{
    CurvePolyline2 out;
    const double total = polyline.length();
    from = std::clamp(from, 0.0, total);
    to = std::clamp(to, from, total);
    double walked = 0.0;
    for (std::size_t i = 0; i < polyline.segmentCount(); ++i) {
        const double length = polyline.segmentLength(i);
        const double s0 = walked;
        const double s1 = walked + length;
        walked = s1;
        const double a = std::max(from, s0);
        const double b = std::min(to, s1);
        if (b - a <= tol::kGeometric || !(length > 0.0)) {
            continue;
        }
        // Snapped to the vertex within the tolerance, so a cut at a vertex
        // keeps the vertex exactly rather than a point a hair off it.
        const double ta = a - s0 <= tol::kGeometric ? 0.0 : (a - s0) / length;
        const double tb = s1 - b <= tol::kGeometric ? 1.0 : (b - s0) / length;
        const CurveSegment piece = polyline.segment(i);
        // The part's sweep is the fraction of the arc's: tan(atan(bulge) * f).
        const double bulge = std::tan(std::atan(polyline.vertices[i].bulge) * (tb - ta));
        if (out.vertices.empty()) {
            out.vertices.push_back(
                CurveVertex{pointOnSegment(piece, ta), bulge, heightOnSegment(polyline, i, ta)});
        } else {
            out.vertices.back().bulge = bulge;
        }
        const Point2 end = tb >= 1.0 ? polyline.vertices[polyline.segmentEnd(i)].position
                                     : pointOnSegment(piece, tb);
        out.vertices.push_back(CurveVertex{end, 0.0, heightOnSegment(polyline, i, tb)});
    }
    if (out.vertices.size() == 1) {
        out.vertices.clear();
    }
    return out;
}

CurvePolyline2 wrappingPath(const CurvePolyline2& polyline, double from, double to)
{
    CurvePolyline2 head = subPath(polyline, from, polyline.length());
    const CurvePolyline2 tail = subPath(polyline, 0.0, to);
    if (head.vertices.empty()) {
        return tail;
    }
    if (tail.vertices.empty()) {
        return head;
    }
    // head ends and tail starts at the first vertex: one vertex, carrying
    // the bulge of the segment tail starts.
    head.vertices.back().bulge = tail.vertices.front().bulge;
    head.vertices.insert(head.vertices.end(), tail.vertices.begin() + 1, tail.vertices.end());
    return head;
}

} // namespace katana::geometry
