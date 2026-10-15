// A surface in the plan view, in its display style (cad/terrain/surface_plan.hpp,
// docs/surface_display.md "In plan").

#include "katana/cad/terrain/surface_plan.hpp"

#include <algorithm>
#include <array>
#include <bit>
#include <cmath>
#include <limits>
#include <utility>

#include "katana/core/task_pool.hpp"
#include "katana/core/text.hpp"
#include "katana/geometry/polygon.hpp"
#include "katana/math/numerics.hpp"
#include "katana/terrain/contours.hpp"

namespace katana::cad {

namespace tol = katana::math::tolerance;
using katana::entity::Color;
using katana::geometry::Box2;
using katana::geometry::Point2;
using katana::geometry::Segment2;
using katana::terrain::TinSurface;

namespace {

// ---- what is kept, per surface -------------------------------------------------------------

// A line drawn in plan, worked out once: the rim, a run of breaklines, a
// contour.
struct KeptLine {
    std::vector<Point2> points;
    bool closed = false;
    Box2 box;
    double length = 0.0;
    double level = 0.0; // a contour's
    bool major = false; // a contour's
};

struct ContourSet {
    double interval = 0.0;
    double base = 0.0;
    int majorEvery = 0;
    std::size_t levels = 0;
    std::vector<KeptLine> lines;
    std::string problem;
};

struct SurfaceEntry {
    std::shared_ptr<const TinSurface> surface;

    // The cull grid: a uniform grid over the surface's plan box, each cell
    // listing the triangles whose box reaches into it (CSR, ascending).
    Box2 gridBox;
    double cellWidth = 1.0;
    double cellHeight = 1.0;
    std::uint32_t columns = 0;
    std::uint32_t rows = 0;
    std::vector<std::uint32_t> cellStart;
    std::vector<std::uint32_t> cellTriangles;

    // The area-weighted mean slope (rise over run) and the steepest face in
    // percent; the mean distance between vertices.
    double meanSlope = 0.0;
    double steepestPercent = 0.0;
    double vertexSpacing = 0.0;

    bool linesBuilt = false;
    std::vector<KeptLine> rim;
    std::vector<KeptLine> breaklines;

    std::optional<ContourSet> contours;
};

Box2 lineBox(const std::vector<Point2>& points)
{
    Box2 box;
    for (const Point2& p : points) {
        box.expand(p);
    }
    return box;
}

double lineLength(const std::vector<Point2>& points, bool closed)
{
    double length = 0.0;
    for (std::size_t i = 1; i < points.size(); ++i) {
        length += points[i - 1].distanceTo(points[i]);
    }
    if (closed && points.size() > 2) {
        length += points.back().distanceTo(points.front());
    }
    return length;
}

Box2 triangleBox(const TinSurface& surface, std::size_t t)
{
    const auto& corners = surface.triangles()[t];
    const auto& vertices = surface.vertices();
    Box2 box;
    for (const std::uint32_t v : corners) {
        box.expand(Point2(vertices[v].x, vertices[v].y));
    }
    return box;
}

void buildGrid(SurfaceEntry& entry)
{
    const TinSurface& surface = *entry.surface;
    const std::size_t count = surface.triangleCount();
    entry.gridBox = surface.bounds();
    // About four triangles a cell, at most 1024 cells a side: a query for a
    // view then visits about the triangles in it, and the grid costs two
    // integers a triangle and one a cell.
    const double width = std::max(entry.gridBox.width(), tol::kGeometric);
    const double height = std::max(entry.gridBox.height(), tol::kGeometric);
    const double cells = std::max(1.0, static_cast<double>(count) / 4.0);
    const double aspect = width / height;
    entry.columns = static_cast<std::uint32_t>(std::clamp(std::ceil(std::sqrt(cells * aspect)), 1.0, 1024.0));
    entry.rows = static_cast<std::uint32_t>(std::clamp(std::ceil(cells / entry.columns), 1.0, 1024.0));
    entry.cellWidth = width / entry.columns;
    entry.cellHeight = height / entry.rows;

    const auto cellRange = [&entry](const Box2& box) {
        const auto column = [&entry](double x) {
            return static_cast<std::uint32_t>(std::clamp(
                std::floor((x - entry.gridBox.min.x) / entry.cellWidth), 0.0, entry.columns - 1.0));
        };
        const auto row = [&entry](double y) {
            return static_cast<std::uint32_t>(std::clamp(
                std::floor((y - entry.gridBox.min.y) / entry.cellHeight), 0.0, entry.rows - 1.0));
        };
        return std::array<std::uint32_t, 4>{column(box.min.x), column(box.max.x), row(box.min.y),
                                            row(box.max.y)};
    };
    const std::size_t cellCount = static_cast<std::size_t>(entry.columns) * entry.rows;
    entry.cellStart.assign(cellCount + 1, 0);
    for (std::size_t t = 0; t < count; ++t) {
        const auto [c0, c1, r0, r1] = cellRange(triangleBox(surface, t));
        for (std::uint32_t r = r0; r <= r1; ++r) {
            for (std::uint32_t c = c0; c <= c1; ++c) {
                ++entry.cellStart[static_cast<std::size_t>(r) * entry.columns + c + 1];
            }
        }
    }
    for (std::size_t i = 0; i < cellCount; ++i) {
        entry.cellStart[i + 1] += entry.cellStart[i];
    }
    entry.cellTriangles.assign(entry.cellStart.back(), 0);
    std::vector<std::uint32_t> fill(entry.cellStart.begin(), entry.cellStart.end() - 1);
    for (std::size_t t = 0; t < count; ++t) {
        const auto [c0, c1, r0, r1] = cellRange(triangleBox(surface, t));
        for (std::uint32_t r = r0; r <= r1; ++r) {
            for (std::uint32_t c = c0; c <= c1; ++c) {
                entry.cellTriangles[fill[static_cast<std::size_t>(r) * entry.columns + c]++] =
                    static_cast<std::uint32_t>(t);
            }
        }
    }

    double weighted = 0.0;
    double area = 0.0;
    for (std::size_t t = 0; t < count; ++t) {
        const auto slope = surface.triangleSlopeAspect(t);
        if (!slope.has_value() || !std::isfinite(slope->slope)) {
            continue;
        }
        const double a = surface.trianglePlanArea(t);
        weighted += slope->slope * a;
        area += a;
        entry.steepestPercent = std::max(entry.steepestPercent, slope->slope * 100.0);
    }
    entry.meanSlope = area > 0.0 ? weighted / area : 0.0;
    entry.vertexSpacing = surface.vertexCount() > 0
                              ? std::sqrt(surface.planArea() / static_cast<double>(surface.vertexCount()))
                              : 0.0;
}

// Joins edges (vertex pairs) into lines: from each end of a run (a vertex
// met by other than two edges) along to the next, then whatever closed loops
// are left. In vertex order, so the same edges always give the same lines.
std::vector<KeptLine> joinEdges(const TinSurface& surface,
                                std::vector<std::pair<std::uint32_t, std::uint32_t>> edges)
{
    std::vector<KeptLine> lines;
    if (edges.empty()) {
        return lines;
    }
    // Each edge from both ends, by vertex.
    std::vector<std::pair<std::uint32_t, std::uint32_t>> ends;
    ends.reserve(2 * edges.size());
    for (std::size_t i = 0; i < edges.size(); ++i) {
        ends.emplace_back(edges[i].first, static_cast<std::uint32_t>(i));
        ends.emplace_back(edges[i].second, static_cast<std::uint32_t>(i));
    }
    std::ranges::sort(ends);
    const auto range = [&ends](std::uint32_t vertex) {
        const auto lo = std::ranges::lower_bound(ends, std::pair{vertex, std::uint32_t{0}});
        auto hi = lo;
        while (hi != ends.end() && hi->first == vertex) {
            ++hi;
        }
        return std::pair{lo, hi};
    };
    std::vector<bool> used(edges.size(), false);
    const auto& vertices = surface.vertices();
    const auto walk = [&](std::uint32_t start, std::uint32_t firstEdge) {
        KeptLine line;
        std::uint32_t at = start;
        std::uint32_t edge = firstEdge;
        line.points.emplace_back(vertices[at].x, vertices[at].y);
        while (true) {
            used[edge] = true;
            const std::uint32_t next = edges[edge].first == at ? edges[edge].second : edges[edge].first;
            line.points.emplace_back(vertices[next].x, vertices[next].y);
            at = next;
            const auto [lo, hi] = range(at);
            // Go on only through a vertex two edges meet at: three or more is
            // where runs part, and each is a line of its own.
            if (hi - lo != 2) {
                break;
            }
            const auto unused = std::find_if(lo, hi, [&used](const auto& end) { return !used[end.second]; });
            if (unused == hi) {
                break;
            }
            edge = unused->second;
        }
        if (line.points.size() > 2 && at == start) {
            line.points.pop_back();
            line.closed = true;
        }
        line.box = lineBox(line.points);
        line.length = lineLength(line.points, line.closed);
        lines.push_back(std::move(line));
    };
    // Runs first, from each vertex not met by exactly two edges.
    for (std::size_t i = 0; i < ends.size();) {
        const std::uint32_t vertex = ends[i].first;
        const auto [lo, hi] = range(vertex);
        if (hi - lo != 2) {
            for (auto end = lo; end != hi; ++end) {
                if (!used[end->second]) {
                    walk(vertex, end->second);
                }
            }
        }
        i = static_cast<std::size_t>(hi - ends.begin());
    }
    // Then the loops.
    for (const auto& [vertex, edge] : ends) {
        if (!used[edge]) {
            walk(vertex, edge);
        }
    }
    return lines;
}

void buildLines(SurfaceEntry& entry)
{
    const TinSurface& surface = *entry.surface;
    std::vector<std::pair<std::uint32_t, std::uint32_t>> rim;
    std::vector<std::pair<std::uint32_t, std::uint32_t>> breaklines;
    for (std::size_t t = 0; t < surface.triangleCount(); ++t) {
        const auto& corners = surface.triangles()[t];
        for (std::size_t e = 0; e < 3; ++e) {
            const std::uint32_t a = corners[e];
            const std::uint32_t b = corners[(e + 1) % 3];
            const std::uint32_t across = surface.neighbors()[t][e];
            if (across == katana::terrain::kNoTriangle) {
                rim.emplace_back(a, b);
            } else if (across > t && surface.isEdgeConstrained(t, e)) {
                // Once, from the lower-numbered triangle of the two.
                breaklines.emplace_back(a, b);
            }
        }
    }
    entry.rim = joinEdges(surface, std::move(rim));
    entry.breaklines = joinEdges(surface, std::move(breaklines));
    entry.linesBuilt = true;
}

// How many contour points tracing at these settings would make: one a level
// crossing a triangle, near enough (a crossing is two points shared with
// the triangles either side).
std::size_t estimateContourPoints(const TinSurface& surface, double interval, double base)
{
    std::size_t points = 0;
    const auto& vertices = surface.vertices();
    for (const auto& corners : surface.triangles()) {
        const double lo = std::min({vertices[corners[0]].z, vertices[corners[1]].z, vertices[corners[2]].z});
        const double hi = std::max({vertices[corners[0]].z, vertices[corners[1]].z, vertices[corners[2]].z});
        const double crossed = std::floor((hi - base) / interval) - std::floor((lo - base) / interval);
        if (crossed > 0.0) {
            points += static_cast<std::size_t>(std::min(crossed, 1.0e9));
        }
        if (points > kPlanMaximumContourPoints) {
            break;
        }
    }
    return points;
}

ContourSet traceContours(const TinSurface& surface, const SurfaceContours& settings)
{
    ContourSet set;
    set.interval = settings.interval;
    set.base = settings.base;
    set.majorEvery = settings.majorEvery;
    set.problem = contourRefusal(surface, settings);
    if (!set.problem.empty()) {
        return set;
    }
    auto traced = katana::terrain::contours(surface, settings.interval, settings.base,
                                            static_cast<std::size_t>(std::max(settings.majorEvery, 0)));
    if (!traced.ok()) {
        set.problem = "contours: " + traced.error().describe();
        return set;
    }
    double previous = std::numeric_limits<double>::quiet_NaN();
    for (katana::terrain::Contour& contour : *traced) {
        if (contour.elevation != previous) {
            ++set.levels;
            previous = contour.elevation;
        }
        KeptLine line;
        line.points = std::move(contour.line.vertices);
        line.closed = contour.line.closed;
        line.level = contour.elevation;
        line.major = contour.major;
        line.box = lineBox(line.points);
        line.length = lineLength(line.points, line.closed);
        set.lines.push_back(std::move(line));
    }
    return set;
}

// Keeps a vertex only when it is at least `tolerance` from the one kept
// before it, and always the last: radial-distance thinning, which never
// moves a vertex and never drops a line's ends.
void thinInto(std::span<const Point2> points, double tolerance, std::vector<Point2>& out)
{
    out.clear();
    if (points.empty()) {
        return;
    }
    out.push_back(points.front());
    const double squared = tolerance * tolerance;
    for (std::size_t i = 1; i + 1 < points.size(); ++i) {
        const double dx = points[i].x - out.back().x;
        const double dy = points[i].y - out.back().y;
        if (dx * dx + dy * dy >= squared) {
            out.push_back(points[i]);
        }
    }
    if (points.size() > 1) {
        out.push_back(points.back());
    }
}

// `line` as far as `box` shows it, thinned: the whole line when the box holds
// it (a closed one repeating its first point), else its runs of whole
// segments that reach the box (geometry::clipPolyline).
template <typename Emit>
void visibleRuns(const KeptLine& line, const Box2& box, double tolerance,
                 katana::geometry::PolylineRuns& scratch, std::vector<Point2>& closedScratch,
                 Emit&& emit)
{
    if (line.points.size() < 2 || !line.box.intersects(box)) {
        return;
    }
    std::vector<Point2> thinned;
    if (box.contains(line.box)) {
        if (line.closed) {
            closedScratch.assign(line.points.begin(), line.points.end());
            closedScratch.push_back(line.points.front());
            thinInto(closedScratch, tolerance, thinned);
        } else {
            thinInto(line.points, tolerance, thinned);
        }
        if (thinned.size() >= 2) {
            emit(std::move(thinned));
        }
        return;
    }
    katana::geometry::clipPolyline(line.points, line.closed, box, scratch,
                                   katana::geometry::PolylineClip::WholeSegments);
    for (std::size_t run = 0; run < scratch.size(); ++run) {
        const std::span<const Point2> points(scratch.points.data() + scratch.begin(run),
                                             scratch.ends[run] - scratch.begin(run));
        thinInto(points, tolerance, thinned);
        if (thinned.size() >= 2) {
            emit(std::move(thinned));
            thinned = {};
        }
    }
}

// ---- the faces' image ----------------------------------------------------------------------

std::uint32_t premultiplied(const Color& colour, double opacity)
{
    const double alpha = std::clamp(opacity, 0.0, 1.0) * (static_cast<double>(colour.a) / 255.0);
    const auto a = static_cast<std::uint32_t>(std::lround(alpha * 255.0));
    const auto channel = [a](std::uint8_t c) {
        return (static_cast<std::uint32_t>(c) * a + 127u) / 255u;
    };
    return (a << 24) | (channel(colour.r) << 16) | (channel(colour.g) << 8) | channel(colour.b);
}

// `top` over `under`, both premultiplied.
std::uint32_t over(std::uint32_t top, std::uint32_t under)
{
    const std::uint32_t inverse = 255u - (top >> 24);
    const auto channel = [&](int shift) {
        const std::uint32_t t = (top >> shift) & 0xffu;
        const std::uint32_t u = (under >> shift) & 0xffu;
        return std::min<std::uint32_t>(255u, t + (u * inverse + 127u) / 255u) << shift;
    };
    return channel(24) | channel(16) | channel(8) | channel(0);
}

// What one paint asks of the image, for every triangle.
struct FaceRule {
    SurfaceFaces faces = SurfaceFaces::None;
    const SurfaceColouring* colouring = nullptr;
    double opacity = 1.0;
    bool tint = false;            // dense triangles are tinted
    std::uint32_t tintColour = 0; // premultiplied
};

class FaceImage {
  public:
    FaceImage(const TinSurface& surface, const SurfacePlanView& view, SurfacePlanImage& image)
        : surface_(surface), image_(image), x0_(view.imageBox.min.x), y0_(view.imageBox.max.y),
          size_(view.imageBox.width() / std::max(view.imageWidth, 1))
    {
    }

    // Paints `triangles` (ascending) by `rule`; `dense` says which are tinted.
    // In bands of rows across the pool: each band writes its own rows, and
    // takes its triangles in ascending order, so the image is the same at
    // any thread count.
    std::size_t paint(std::span<const std::uint32_t> triangles, const std::vector<bool>& dense,
                      const FaceRule& rule, katana::core::TaskPool& pool)
    {
        const int height = image_.height;
        constexpr int kBandRows = 32;
        const int bandCount = (height + kBandRows - 1) / kBandRows;
        // Bin each triangle into the bands of rows its pixel box reaches.
        std::vector<std::uint32_t> start(static_cast<std::size_t>(bandCount) + 1, 0);
        std::vector<std::array<int, 2>> spans(triangles.size());
        for (std::size_t i = 0; i < triangles.size(); ++i) {
            const auto& corners = surface_.triangles()[triangles[i]];
            double top = std::numeric_limits<double>::infinity();
            double bottom = -std::numeric_limits<double>::infinity();
            for (const std::uint32_t v : corners) {
                const double y = pixelY(surface_.vertices()[v].y);
                top = std::min(top, y);
                bottom = std::max(bottom, y);
            }
            // The rows whose centres (j + 0.5) the triangle can reach,
            // clamped as doubles first: zoomed far in, a corner can be more
            // pixels away than an int holds.
            const int first = toPixel(std::ceil(top - 0.5), height);
            const int last = toPixel(std::floor(bottom - 0.5), height);
            if (!(top - 0.5 <= height - 1.0) || !(bottom - 0.5 >= 0.0)) {
                spans[i] = {1, 0};
                continue;
            }
            spans[i] = {first, last};
            if (first > last) {
                continue;
            }
            for (int band = first / kBandRows; band <= last / kBandRows; ++band) {
                ++start[static_cast<std::size_t>(band) + 1];
            }
        }
        for (int band = 0; band < bandCount; ++band) {
            start[static_cast<std::size_t>(band) + 1] += start[static_cast<std::size_t>(band)];
        }
        std::vector<std::uint32_t> binned(start.back());
        std::vector<std::uint32_t> fill(start.begin(), start.end() - 1);
        for (std::size_t i = 0; i < triangles.size(); ++i) {
            if (spans[i][0] > spans[i][1]) {
                continue;
            }
            for (int band = spans[i][0] / kBandRows; band <= spans[i][1] / kBandRows; ++band) {
                binned[fill[static_cast<std::size_t>(band)]++] = static_cast<std::uint32_t>(i);
            }
        }
        std::vector<std::size_t> covered(static_cast<std::size_t>(bandCount), 0);
        pool.parallelRanges(0, static_cast<std::size_t>(bandCount), 1, [&](std::size_t lo, std::size_t hi) {
            for (std::size_t band = lo; band < hi; ++band) {
                const int rowLo = static_cast<int>(band) * kBandRows;
                const int rowHi = std::min(height - 1, rowLo + kBandRows - 1);
                for (std::uint32_t k = start[band]; k < start[band + 1]; ++k) {
                    const std::uint32_t i = binned[k];
                    const int first = std::max(rowLo, spans[i][0]);
                    const int last = std::min(rowHi, spans[i][1]);
                    covered[band] += rasterise(triangles[i], first, last, dense[i], rule);
                }
            }
        });
        std::size_t total = 0;
        for (const std::size_t count : covered) {
            total += count;
        }
        return total;
    }

    // One pixel of `colour`, opaque, at a model point: a vertex too dense to
    // mark. False when the point is off the image.
    bool dot(const Point2& at, std::uint32_t colour)
    {
        const double x = (at.x - x0_) / size_;
        const double y = pixelY(at.y);
        if (!(x >= 0.0) || !(y >= 0.0) || x >= image_.width || y >= image_.height) {
            return false;
        }
        image_.pixels[static_cast<std::size_t>(y) * static_cast<std::size_t>(image_.width) +
                      static_cast<std::size_t>(x)] = colour;
        return true;
    }

  private:
    [[nodiscard]] double pixelX(double x) const { return (x - x0_) / size_; }
    [[nodiscard]] double pixelY(double y) const { return (y0_ - y) / size_; }
    // A pixel index from a double that may lie far outside [0, count).
    [[nodiscard]] static int toPixel(double value, int count)
    {
        return static_cast<int>(std::clamp(std::isfinite(value) ? value : 0.0, 0.0,
                                           static_cast<double>(std::max(count - 1, 0))));
    }

    // The pixels of triangle `t` whose centres it covers, in rows first to
    // last. Each edge's function is worked out from its lower-numbered vertex
    // to its higher, the same arithmetic in both triangles that share it, so
    // a centre exactly on the edge (exactly zero) is a tie they both see, and
    // the tie goes by the edge's direction in the image, never by vertex
    // numbers: the top-left rule of the graphics APIs. A tie is the
    // triangle's when its edge runs up the image (a left edge), or along it
    // with the inside below (a top edge) - the inside being on the edge's
    // positive side. That is the same as testing the centre moved by an
    // infinitesimal (e, e^2), which no edge passes through, so a centre on an
    // edge goes to one of the two triangles and one on a VERTEX to exactly
    // one of those round it: no pinholes, nothing twice. (Deciding by vertex
    // numbers settled an edge, but a vertex only for a fan whose centre was
    // numbered between its neighbours: a vertex numbered below all of them,
    // or above all, was no triangle's, and one whose neighbours alternated
    // was two's.)
    std::size_t rasterise(std::uint32_t t, int first, int last, bool dense, const FaceRule& rule)
    {
        if (first > last) {
            return 0;
        }
        const auto& corners = surface_.triangles()[t];
        const auto& vertices = surface_.vertices();
        std::array<double, 3> px{};
        std::array<double, 3> py{};
        std::array<double, 3> pz{};
        for (std::size_t k = 0; k < 3; ++k) {
            px[k] = pixelX(vertices[corners[k]].x);
            py[k] = pixelY(vertices[corners[k]].y);
            pz[k] = vertices[corners[k]].z;
        }
        const double area2 = (px[1] - px[0]) * (py[2] - py[0]) - (py[1] - py[0]) * (px[2] - px[0]);
        if (!(std::abs(area2) > 0.0) || !std::isfinite(area2)) {
            return 0;
        }
        const double orientation = area2 > 0.0 ? 1.0 : -1.0;
        // Edge k runs from corner k to corner k + 1; its canonical way round
        // is from the lower vertex number to the higher.
        struct Edge {
            double ax, ay, dx, dy; // from the canonical start, along it
            double sign;           // orientation times +1 (canonical) or -1
            bool owns;             // a centre exactly on it is this triangle's
        };
        std::array<Edge, 3> edges{};
        for (std::size_t k = 0; k < 3; ++k) {
            const std::size_t from = k;
            const std::size_t to = (k + 1) % 3;
            const bool forward = corners[from] < corners[to];
            const std::size_t lo = forward ? from : to;
            const std::size_t hi = forward ? to : from;
            const double dx = px[hi] - px[lo];
            const double dy = py[hi] - py[lo];
            const double sign = orientation * (forward ? 1.0 : -1.0);
            // The edge run so the inside is on its positive side; the image's
            // y runs down. Up is a left edge, along with the inside below a
            // top edge. The neighbour across it sees the exact opposite.
            const double upX = sign * dx;
            const double upY = sign * dy;
            edges[k] = {px[lo], py[lo], dx, dy, sign, upY < 0.0 || (upY == 0.0 && upX > 0.0)};
        }
        const auto edgeValue = [](const Edge& edge, double x, double y) {
            return edge.sign * (edge.dx * (y - edge.ay) - edge.dy * (x - edge.ax));
        };
        const auto inside = [](const Edge& edge, double value) {
            return value > 0.0 || (value == 0.0 && edge.owns);
        };

        // The colour of the whole triangle, when it does not vary across it.
        std::uint32_t faceColour = 0;
        bool perPixel = false;
        if (rule.colouring != nullptr) {
            switch (rule.faces) {
            case SurfaceFaces::None:
                break;
            case SurfaceFaces::Flat:
                faceColour = premultiplied(rule.colouring->flat, rule.opacity);
                break;
            case SurfaceFaces::Elevation:
                perPixel = true;
                break;
            case SurfaceFaces::Slope:
            case SurfaceFaces::Aspect: {
                const auto slope = surface_.triangleSlopeAspect(t);
                Color colour = rule.colouring->flat;
                if (slope.has_value() && rule.faces == SurfaceFaces::Slope) {
                    colour = rule.colouring->colourAt(slope->slope * 100.0);
                } else if (slope.has_value() && slope->aspect.has_value()) {
                    colour = aspectSectorColour(aspectSector(*slope->aspect));
                }
                faceColour = premultiplied(colour, rule.opacity);
                break;
            }
            }
        }
        const bool tinted = dense && rule.tint;
        if (rule.faces == SurfaceFaces::None && !tinted) {
            return 0;
        }
        const double zLow = std::min({pz[0], pz[1], pz[2]});
        const double zHigh = std::max({pz[0], pz[1], pz[2]});
        const double left = std::min({px[0], px[1], px[2]});
        const double right = std::max({px[0], px[1], px[2]});
        if (!(right - 0.5 >= 0.0) || !(left - 0.5 <= image_.width - 1.0)) {
            return 0;
        }
        const int columnLo = toPixel(std::ceil(left - 0.5), image_.width);
        const int columnHi = toPixel(std::floor(right - 0.5), image_.width);
        std::size_t covered = 0;
        for (int j = first; j <= last; ++j) {
            const double y = static_cast<double>(j) + 0.5;
            // The row's span from each edge's line, widened by a pixel, then
            // every pixel in it tested exactly.
            double spanLo = static_cast<double>(columnLo);
            double spanHi = static_cast<double>(columnHi);
            bool empty = false;
            for (const Edge& edge : edges) {
                // value(x) = a + b x along the row.
                const double b = -edge.sign * edge.dy;
                const double a = edge.sign * (edge.dx * (y - edge.ay) + edge.dy * edge.ax);
                if (b > 0.0) {
                    spanLo = std::max(spanLo, std::floor(-a / b - 0.5) - 1.0);
                } else if (b < 0.0) {
                    spanHi = std::min(spanHi, std::ceil(-a / b - 0.5) + 1.0);
                } else if (a < 0.0) {
                    empty = true;
                }
            }
            if (empty || spanLo > spanHi) {
                continue;
            }
            std::uint32_t* row = image_.pixels.data() + static_cast<std::size_t>(j) * static_cast<std::size_t>(image_.width);
            for (int i = static_cast<int>(spanLo); i <= static_cast<int>(spanHi); ++i) {
                const double x = static_cast<double>(i) + 0.5;
                const double v0 = edgeValue(edges[0], x, y);
                const double v1 = edgeValue(edges[1], x, y);
                const double v2 = edgeValue(edges[2], x, y);
                if (!inside(edges[0], v0) || !inside(edges[1], v1) || !inside(edges[2], v2)) {
                    continue;
                }
                std::uint32_t colour = faceColour;
                if (perPixel) {
                    // The height at the pixel's centre: each corner weighted
                    // by the edge opposite it, the values summing to the
                    // doubled area. Held inside the corners' range, which a
                    // centre on an edge may round past.
                    const double sum = v0 + v1 + v2;
                    const double z =
                        sum > 0.0 ? std::clamp((v1 * pz[0] + v2 * pz[1] + v0 * pz[2]) / sum, zLow, zHigh)
                                  : pz[0];
                    colour = premultiplied(rule.colouring->colourAt(z), rule.opacity);
                }
                if (tinted) {
                    colour = rule.faces == SurfaceFaces::None ? rule.tintColour
                                                              : over(rule.tintColour, colour);
                }
                row[i] = colour;
                ++covered;
            }
        }
        return covered;
    }

    const TinSurface& surface_;
    SurfacePlanImage& image_;
    double x0_;
    double y0_;
    double size_;
};

} // namespace

// ---- the cache -------------------------------------------------------------------------------

struct SurfacePlanCache::Impl {
    std::vector<std::unique_ptr<SurfaceEntry>> entries;
    std::size_t tracings = 0;
    std::size_t grids = 0;

    // Every surface asked about is kept until retain() lets it go: the
    // caller bounds the cache to the drawing. A count cap evicting the least
    // recently used would be wrong for a paint, which asks for every visible
    // surface in the same order each time: with one more surface than the cap
    // the one wanted next is always the one just evicted, and every paint
    // rebuilds every grid and traces every contour again (measured with 33
    // surfaces: 61-69 ms a repaint against 3-7 ms with 32).
    SurfaceEntry& entryFor(const std::shared_ptr<const TinSurface>& surface)
    {
        for (const auto& entry : entries) {
            if (entry->surface.get() == surface.get()) {
                return *entry;
            }
        }
        auto entry = std::make_unique<SurfaceEntry>();
        entry->surface = surface;
        buildGrid(*entry);
        ++grids;
        entries.push_back(std::move(entry));
        return *entries.back();
    }
};

SurfacePlanCache::Impl& planCacheImpl(SurfacePlanCache& cache) { return *cache.impl_; }

SurfacePlanCache::SurfacePlanCache() : impl_(std::make_unique<Impl>()) {}
SurfacePlanCache::~SurfacePlanCache() = default;
SurfacePlanCache::SurfacePlanCache(const SurfacePlanCache&) : impl_(std::make_unique<Impl>()) {}
SurfacePlanCache& SurfacePlanCache::operator=(const SurfacePlanCache& other)
{
    if (this != &other) {
        impl_ = std::make_unique<Impl>();
    }
    return *this;
}
SurfacePlanCache::SurfacePlanCache(SurfacePlanCache&& other) noexcept : impl_(std::move(other.impl_))
{
    other.impl_ = std::make_unique<Impl>();
}
SurfacePlanCache& SurfacePlanCache::operator=(SurfacePlanCache&& other) noexcept
{
    if (this != &other) {
        impl_ = std::move(other.impl_);
        other.impl_ = std::make_unique<Impl>();
    }
    return *this;
}

void SurfacePlanCache::retain(std::span<const TinSurface* const> alive)
{
    std::erase_if(impl_->entries, [alive](const std::unique_ptr<SurfaceEntry>& entry) {
        return std::ranges::find(alive, entry->surface.get()) == alive.end();
    });
}

void SurfacePlanCache::clear() { impl_->entries.clear(); }
std::size_t SurfacePlanCache::surfaceCount() const { return impl_->entries.size(); }
std::size_t SurfacePlanCache::contourTracings() const { return impl_->tracings; }
std::size_t SurfacePlanCache::gridsBuilt() const { return impl_->grids; }

// ---- culling ---------------------------------------------------------------------------------

namespace {

std::vector<std::uint32_t> trianglesInBox(const SurfaceEntry& entry, const Box2& box,
                                          std::vector<std::uint64_t>& marks)
{
    const TinSurface& surface = *entry.surface;
    const std::size_t count = surface.triangleCount();
    std::vector<std::uint32_t> found;
    marks.assign((count + 63) / 64, 0);
    if (count == 0 || box.empty() || !box.intersects(surface.bounds())) {
        return found;
    }
    if (box.contains(surface.bounds())) {
        // Every triangle's box is inside the surface's, so all of them.
        found.resize(count);
        for (std::size_t t = 0; t < count; ++t) {
            found[t] = static_cast<std::uint32_t>(t);
        }
        std::ranges::fill(marks, ~std::uint64_t{0});
        return found;
    }
    const auto column = [&entry](double x) {
        return static_cast<std::uint32_t>(
            std::clamp(std::floor((x - entry.gridBox.min.x) / entry.cellWidth), 0.0, entry.columns - 1.0));
    };
    const auto row = [&entry](double y) {
        return static_cast<std::uint32_t>(
            std::clamp(std::floor((y - entry.gridBox.min.y) / entry.cellHeight), 0.0, entry.rows - 1.0));
    };
    const std::uint32_t c0 = column(box.min.x);
    const std::uint32_t c1 = column(box.max.x);
    const std::uint32_t r0 = row(box.min.y);
    const std::uint32_t r1 = row(box.max.y);
    for (std::uint32_t r = r0; r <= r1; ++r) {
        for (std::uint32_t c = c0; c <= c1; ++c) {
            const std::size_t cell = static_cast<std::size_t>(r) * entry.columns + c;
            for (std::uint32_t k = entry.cellStart[cell]; k < entry.cellStart[cell + 1]; ++k) {
                const std::uint32_t t = entry.cellTriangles[k];
                std::uint64_t& word = marks[t / 64];
                const std::uint64_t bit = std::uint64_t{1} << (t % 64);
                if ((word & bit) == 0 && triangleBox(surface, t).intersects(box)) {
                    word |= bit;
                }
            }
        }
    }
    // In ascending order, from the marks.
    for (std::size_t w = 0; w < marks.size(); ++w) {
        std::uint64_t word = marks[w];
        while (word != 0) {
            const int bit = std::countr_zero(word);
            found.push_back(static_cast<std::uint32_t>(w * 64 + static_cast<std::size_t>(bit)));
            word &= word - 1;
        }
    }
    return found;
}

bool marked(const std::vector<std::uint64_t>& marks, std::uint32_t t)
{
    return t / 64 < marks.size() && (marks[t / 64] >> (t % 64) & 1u) != 0;
}

} // namespace

std::vector<std::uint32_t> planTrianglesInBox(const std::shared_ptr<const TinSurface>& surface,
                                              const Box2& box, SurfacePlanCache& cache)
{
    if (surface == nullptr) {
        return {};
    }
    std::vector<std::uint64_t> marks;
    return trianglesInBox(planCacheImpl(cache).entryFor(surface), box, marks);
}

std::vector<std::uint32_t> planTrianglesInBoxExhaustive(const TinSurface& surface, const Box2& box)
{
    std::vector<std::uint32_t> found;
    for (std::size_t t = 0; t < surface.triangleCount(); ++t) {
        if (triangleBox(surface, t).intersects(box)) {
            found.push_back(static_cast<std::uint32_t>(t));
        }
    }
    return found;
}

// ---- the grid of the image -------------------------------------------------------------------

SurfacePlanImageGrid planImageGrid(const Box2& visible, const Box2& bounds, double unitsPerPixel,
                                   std::size_t pixelCap)
{
    SurfacePlanImageGrid grid;
    if (visible.empty() || bounds.empty() || !visible.intersects(bounds) ||
        !(unitsPerPixel > 0.0) || !std::isfinite(unitsPerPixel)) {
        return grid;
    }
    const Box2 meet(Point2(std::max(visible.min.x, bounds.min.x), std::max(visible.min.y, bounds.min.y)),
                    Point2(std::min(visible.max.x, bounds.max.x), std::min(visible.max.y, bounds.max.y)));
    double size = unitsPerPixel;
    for (int attempt = 0; attempt < 4; ++attempt) {
        const double c0 = std::floor((meet.min.x - visible.min.x) / size);
        const double c1 = std::ceil((meet.max.x - visible.min.x) / size);
        const double r0 = std::floor((visible.max.y - meet.max.y) / size);
        const double r1 = std::ceil((visible.max.y - meet.min.y) / size);
        const double width = std::max(1.0, c1 - c0);
        const double height = std::max(1.0, r1 - r0);
        const double pixels = width * height;
        const double cap = static_cast<double>(std::max<std::size_t>(pixelCap, 1));
        if (pixels > cap) {
            // Coarser evenly, a hair past the ratio so the rounding outward
            // above cannot leave it just over.
            size *= std::sqrt(pixels / cap) * 1.0001;
            continue;
        }
        grid.width = static_cast<int>(width);
        grid.height = static_cast<int>(height);
        grid.box = Box2(Point2(visible.min.x + c0 * size, visible.max.y - (r0 + height) * size),
                        Point2(visible.min.x + (c0 + width) * size, visible.max.y - r0 * size));
        return grid;
    }
    return grid;
}

// ---- why a style could not be drawn ----------------------------------------------------------

std::string contourRefusal(const TinSurface& surface, const SurfaceContours& contours)
{
    if (!(contours.interval > tol::kGeometric) || !std::isfinite(contours.interval) ||
        !std::isfinite(contours.base)) {
        return "contours: the interval must be a finite number greater than zero";
    }
    if (estimateContourPoints(surface, contours.interval, contours.base) > kPlanMaximumContourPoints) {
        return "contours: an interval of " + formatLegendValue(contours.interval, 6) +
               " crosses this surface's triangles more than " +
               std::to_string(kPlanMaximumContourPoints) + " times; not drawn - set a larger interval";
    }
    return {};
}

std::string SurfaceProblem::text() const
{
    return surface + " (style " + style + "): " + reason;
}

std::string SurfaceProblem::record() const
{
    return "warning surface=" + katana::core::replyQuoted(surface) +
           " style=" + katana::core::replyQuoted(style) + " reason=" + katana::core::replyQuoted(reason);
}

std::vector<SurfaceStyleProblem> surfaceStyleProblems(const katana::terrain::SurfaceStore& surfaces,
                                                      const SurfaceStyleSet& styles)
{
    std::vector<SurfaceStyleProblem> found;
    for (const katana::terrain::NamedSurface& named : surfaces.all()) {
        // The painter's own conditions for drawing a surface at all.
        if (!named.visible || named.surface == nullptr || named.surface->empty()) {
            continue;
        }
        const SurfaceDisplayStyle& style = styleOf(styles, named);
        if (!style.contours.show) {
            continue;
        }
        std::string reason = contourRefusal(*named.surface, style.contours);
        if (!reason.empty()) {
            found.push_back({SurfaceProblem{named.name, style.name, std::move(reason)},
                             named.surface->bounds()});
        }
    }
    return found;
}

// ---- the plan --------------------------------------------------------------------------------

SurfaceColouring planColouring(const TinSurface& surface, const SurfaceDisplayStyle& style)
{
    return surfaceColouring(style, style.planFaces, surface);
}

SurfacePlan surfacePlan(const katana::terrain::NamedSurface& named, const SurfaceDisplayStyle& style,
                        const SurfacePlanView& view, SurfacePlanCache& cache)
{
    SurfacePlan plan;
    plan.name = named.name;
    plan.triangleColour = style.triangleColour;
    plan.minorColour = style.contours.minorColour;
    plan.majorColour = style.contours.majorColour;
    plan.breaklineColour = style.breaklineColour;
    plan.boundaryColour = style.boundaryColour;
    plan.pointColour = style.pointColour;
    if (named.surface == nullptr || named.surface->empty() || view.visible.empty() ||
        !(view.unitsPerPixel > 0.0) || !std::isfinite(view.unitsPerPixel)) {
        return plan;
    }
    SurfacePlanCache::Impl& kept = planCacheImpl(cache);
    SurfaceEntry& entry = kept.entryFor(named.surface);
    const TinSurface& surface = *named.surface;
    const double pixel = view.unitsPerPixel;

    // The colouring, with the range the surface spans where the style gives
    // none - the steepest face from the cache, not a pass a paint.
    const SurfaceColouring colouring =
        style.planFaces == SurfaceFaces::Slope
            ? surfaceColouring(style, SurfaceFaces::Slope, 0.0, entry.steepestPercent)
            : surfaceColouring(style, style.planFaces, surface.minElevation(), surface.maxElevation());
    if (colouring.coloursFaces()) {
        plan.legend = surfaceLegend(colouring);
    }

    // Culled to the view grown by a few pixels: a line's width, a mark.
    const Box2 reach = view.visible.inflated(4.0 * pixel);
    std::vector<std::uint64_t> marks;
    const std::vector<std::uint32_t> triangles = trianglesInBox(entry, reach, marks);
    plan.stats.trianglesInView = triangles.size();

    // ---- triangles: stroked, or too dense and tinted
    std::vector<bool> dense(triangles.size(), false);
    if (style.showTriangles) {
        const auto& vertices = surface.vertices();
        const double shortest = kPlanEdgePixels * pixel;
        for (std::size_t i = 0; i < triangles.size(); ++i) {
            const std::uint32_t t = triangles[i];
            const auto& corners = surface.triangles()[t];
            bool anyStroked = false;
            for (std::size_t e = 0; e < 3; ++e) {
                const auto& a = vertices[corners[e]];
                const auto& b = vertices[corners[(e + 1) % 3]];
                const Segment2 edge{Point2(a.x, a.y), Point2(b.x, b.y)};
                if (!(edge.length() >= shortest)) {
                    continue;
                }
                anyStroked = true;
                // Once: from the lower-numbered of the two triangles, unless
                // the other is not being drawn.
                const std::uint32_t across = surface.neighbors()[t][e];
                if (across != katana::terrain::kNoTriangle && across < t && marked(marks, across)) {
                    continue;
                }
                Box2 box;
                box.expand(edge.start);
                box.expand(edge.end);
                if (box.intersects(reach)) {
                    plan.triangles.push_back(edge);
                }
            }
            dense[i] = !anyStroked;
            plan.stats.trianglesTinted += dense[i] ? 1u : 0u;
        }
        plan.stats.edgesStroked = plan.triangles.size();
    }

    // ---- points: marks, or pixels when too dense to tell apart
    const bool densePoints = style.showPoints && entry.vertexSpacing < kPlanPointPixels * pixel;
    std::vector<Point2> pointPixels;
    if (style.showPoints) {
        std::vector<bool> seen(surface.vertexCount(), false);
        const auto& vertices = surface.vertices();
        for (const std::uint32_t t : triangles) {
            for (const std::uint32_t v : surface.triangles()[t]) {
                if (seen[v]) {
                    continue;
                }
                seen[v] = true;
                const Point2 at(vertices[v].x, vertices[v].y);
                if (!reach.contains(at)) {
                    continue;
                }
                (densePoints ? pointPixels : plan.points).push_back(at);
            }
        }
        plan.stats.pointsMarked = plan.points.size();
    }

    // ---- the image: faces, dense triangles, dense points
    const bool tint = style.showTriangles && plan.stats.trianglesTinted > 0;
    const bool wantImage = colouring.coloursFaces() || tint || !pointPixels.empty();
    if (wantImage && view.imageWidth > 0 && view.imageHeight > 0 && !view.imageBox.empty()) {
        plan.image.box = view.imageBox;
        plan.image.width = view.imageWidth;
        plan.image.height = view.imageHeight;
        plan.image.pixels.assign(static_cast<std::size_t>(view.imageWidth) *
                                     static_cast<std::size_t>(view.imageHeight),
                                 0u);
        plan.stats.imagePixels = plan.image.pixels.size();
        FaceRule rule;
        rule.faces = colouring.faces;
        rule.colouring = &colouring;
        rule.opacity = style.planOpacity;
        rule.tint = tint;
        rule.tintColour = premultiplied(style.triangleColour, kPlanDenseTriangleAlpha);
        FaceImage image(surface, view, plan.image);
        katana::core::TaskPool& pool =
            view.pool != nullptr ? *view.pool : katana::core::TaskPool::shared();
        plan.stats.pixelsCovered = image.paint(triangles, dense, rule, pool);
        const std::uint32_t pointColour = premultiplied(style.pointColour, 1.0);
        for (const Point2& at : pointPixels) {
            plan.stats.pointsInImage += image.dot(at, pointColour) ? 1u : 0u;
        }
    }

    katana::geometry::PolylineRuns scratch;
    std::vector<Point2> closedScratch;
    const double thin = kPlanSimplifyPixels * pixel;

    // ---- contours, traced once per surface and settings
    if (style.contours.show) {
        const SurfaceContours& settings = style.contours;
        if (!entry.contours.has_value() || entry.contours->interval != settings.interval ||
            entry.contours->base != settings.base || entry.contours->majorEvery != settings.majorEvery) {
            entry.contours = traceContours(surface, settings);
            ++kept.tracings;
        }
        const ContourSet& set = *entry.contours;
        plan.problem = set.problem;
        plan.stats.contourLevels = set.levels;
        // How far apart a kind of contour lies on screen, at the surface's
        // mean slope: interval over slope is the mean spacing in the model.
        const auto spacingPixels = [&entry, pixel](double interval) {
            return entry.meanSlope > 0.0 ? interval / entry.meanSlope / pixel
                                         : std::numeric_limits<double>::infinity();
        };
        plan.stats.minorContoursHidden = spacingPixels(settings.interval) < kPlanContourPixels;
        plan.stats.majorContoursHidden =
            settings.majorEvery <= 0 ||
            spacingPixels(settings.interval * settings.majorEvery) < kPlanContourPixels;
        // Minors first, then majors, so a major is drawn over the minors.
        for (const bool major : {false, true}) {
            if (major ? plan.stats.majorContoursHidden : plan.stats.minorContoursHidden) {
                continue;
            }
            for (const KeptLine& line : set.lines) {
                if (line.major != major) {
                    continue;
                }
                // Too small to see: a speck.
                if (std::max(line.box.width(), line.box.height()) < pixel) {
                    continue;
                }
                visibleRuns(line, reach, thin, scratch, closedScratch, [&](std::vector<Point2> run) {
                    plan.stats.contourVertices += run.size();
                    plan.contours.push_back({line.level, line.major, std::move(run)});
                });
            }
        }
        plan.stats.contourRuns = plan.contours.size();

        // Labels on the majors drawn, along each whole contour from its own
        // start - so a pan does not slide them along the line - inside the
        // view by a margin.
        if (settings.labelMajor && !plan.stats.majorContoursHidden) {
            const double values[2] = {settings.interval, settings.base};
            const int decimals = legendDecimals(values);
            const double spacing = std::max(view.labelSpacingPixels, 1.0) * pixel;
            const Box2 inside = view.visible.inflated(-kPlanLabelMarginPixels * pixel);
            for (const KeptLine& line : set.lines) {
                if (!line.major || line.points.size() < 2 ||
                    line.length < kPlanLabelMinimumPixels * pixel ||
                    !line.box.intersects(inside) || plan.labels.size() >= kPlanMaximumLabels) {
                    continue;
                }
                const std::string text = formatLegendValue(line.level, decimals);
                double next = std::min(0.5 * spacing, 0.5 * line.length);
                double walked = 0.0;
                const std::size_t segments = line.closed ? line.points.size() : line.points.size() - 1;
                for (std::size_t s = 0; s < segments && plan.labels.size() < kPlanMaximumLabels; ++s) {
                    const Point2& a = line.points[s];
                    const Point2& b = line.points[(s + 1) % line.points.size()];
                    const double length = a.distanceTo(b);
                    while (length > 0.0 && next <= walked + length &&
                           plan.labels.size() < kPlanMaximumLabels) {
                        const double f = (next - walked) / length;
                        const Point2 at(a.x + (b.x - a.x) * f, a.y + (b.y - a.y) * f);
                        if (inside.contains(at)) {
                            plan.labels.push_back({at, std::atan2(b.y - a.y, b.x - a.x), text});
                        }
                        next += spacing;
                    }
                    walked += length;
                }
            }
            plan.stats.labels = plan.labels.size();
        }
    }

    // ---- breaklines and the boundary, joined once
    if (style.showBreaklines || style.showBoundary) {
        if (!entry.linesBuilt) {
            buildLines(entry);
        }
        if (style.showBreaklines) {
            for (const KeptLine& line : entry.breaklines) {
                visibleRuns(line, reach, thin, scratch, closedScratch,
                            [&](std::vector<Point2> run) { plan.breaklines.push_back(std::move(run)); });
            }
            plan.stats.breaklineRuns = plan.breaklines.size();
        }
        if (style.showBoundary) {
            for (const KeptLine& line : entry.rim) {
                visibleRuns(line, reach, thin, scratch, closedScratch,
                            [&](std::vector<Point2> run) { plan.boundary.push_back(std::move(run)); });
            }
            plan.stats.boundaryRuns = plan.boundary.size();
        }
    }
    return plan;
}

} // namespace katana::cad
