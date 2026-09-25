#include "katana/cad/plotting/plan_grid.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <format>
#include <limits>
#include <utility>

#include "katana/cad/plotting/sheet_commands.hpp"

namespace katana::cad::plotting {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;

namespace {

constexpr std::array<std::pair<std::string_view, GridStyle>, 4> kGridStyles{{
    {"none", GridStyle::None},
    {"ticks", GridStyle::Ticks},
    {"crosses", GridStyle::Crosses},
    {"lines", GridStyle::Lines},
}};

// More lines than this on one axis is not a grid anybody reads; it is refused
// with the spacing message long before a real sheet gets near it (an A0 sheet
// at the closest spacing has about 600).
constexpr double kMaximumLinesPerAxis = 5000.0;

Point2 turned(const Point2& v, double radians)
{
    const double c = std::cos(radians);
    const double s = std::sin(radians);
    return Point2(c * v.x - s * v.y, s * v.x + c * v.y);
}

bool finite(double value) { return std::isfinite(value); }

// The part of the infinite line through `origin` along `direction` that lies
// inside `rect`, and the edges its ends are on (Liang-Barsky). Nothing when
// the line misses the rectangle, only touches a corner, or lies along an
// edge - that line is the border itself.
struct Clipped {
    Point2 from;
    Point2 to;
    ViewportEdge fromEdge = ViewportEdge::Bottom;
    ViewportEdge toEdge = ViewportEdge::Top;
};

std::optional<Clipped> clipLine(const Point2& origin, const Point2& direction, const Box2& rect)
{
    constexpr double kParallel = 1e-12;
    constexpr double kOnEdgeMm = 1e-9;
    double t0 = -std::numeric_limits<double>::infinity();
    double t1 = std::numeric_limits<double>::infinity();
    ViewportEdge e0 = ViewportEdge::Bottom;
    ViewportEdge e1 = ViewportEdge::Top;
    const auto slab = [&](double p, double d, double lo, double hi, ViewportEdge loEdge,
                          ViewportEdge hiEdge) {
        if (std::abs(d) < kParallel) {
            return p > lo + kOnEdgeMm && p < hi - kOnEdgeMm;
        }
        double ta = (lo - p) / d;
        double tb = (hi - p) / d;
        ViewportEdge ea = loEdge;
        ViewportEdge eb = hiEdge;
        if (ta > tb) {
            std::swap(ta, tb);
            std::swap(ea, eb);
        }
        if (ta > t0) {
            t0 = ta;
            e0 = ea;
        }
        if (tb < t1) {
            t1 = tb;
            e1 = eb;
        }
        return true;
    };
    if (!slab(origin.x, direction.x, rect.min.x, rect.max.x, ViewportEdge::Left,
              ViewportEdge::Right) ||
        !slab(origin.y, direction.y, rect.min.y, rect.max.y, ViewportEdge::Bottom,
              ViewportEdge::Top) ||
        !(t1 - t0 > kOnEdgeMm)) {
        return std::nullopt;
    }
    return Clipped{origin + direction * t0, origin + direction * t1, e0, e1};
}

// "E 305 200", "N 6 250 400".
std::string labelText(const GridLine& line, int decimals)
{
    return (line.axis == GridAxis::Easting ? "E " : "N ") + groupedCoordinate(line.value, decimals);
}

// The label `text`, `widthMm` wide, for the line `line` where it meets `edge`
// at `at`: set along the edge, `inset` in from it, centred on the line.
GridLabel labelAt(const GridLine& line, std::string text, double widthMm, ViewportEdge edge,
                  const Point2& at, const Box2& rect, double inset, const PlanGridOptions& options)
{
    GridLabel label;
    label.axis = line.axis;
    label.value = line.value;
    label.text = std::move(text);
    label.edge = edge;
    label.horizontal = HorizontalJustify::Centre;
    const double cap = options.labelCapMm;
    const double k = options.knockOutMm;
    const double half = widthMm / 2.0;
    switch (edge) {
    case ViewportEdge::Bottom:
        label.anchor = Point2(at.x, rect.min.y + inset);
        label.vertical = VerticalJustify::Bottom;
        label.box = Box2(Point2(at.x - half - k, label.anchor.y - k),
                         Point2(at.x + half + k, label.anchor.y + cap + k));
        break;
    case ViewportEdge::Top:
        label.anchor = Point2(at.x, rect.max.y - inset);
        label.vertical = VerticalJustify::Top;
        label.box = Box2(Point2(at.x - half - k, label.anchor.y - cap - k),
                         Point2(at.x + half + k, label.anchor.y + k));
        break;
    // Up the sides the text reads from the bottom, its head to the left: on
    // the left edge the head is at the border and the text hangs inward from
    // the anchor, on the right edge the text stands inward on it.
    case ViewportEdge::Left:
        label.anchor = Point2(rect.min.x + inset, at.y);
        label.angleDegrees = 90.0;
        label.vertical = VerticalJustify::Top;
        label.box = Box2(Point2(label.anchor.x - k, at.y - half - k),
                         Point2(label.anchor.x + cap + k, at.y + half + k));
        break;
    case ViewportEdge::Right:
        label.anchor = Point2(rect.max.x - inset, at.y);
        label.angleDegrees = 90.0;
        label.vertical = VerticalJustify::Bottom;
        label.box = Box2(Point2(label.anchor.x - cap - k, at.y - half - k),
                         Point2(label.anchor.x + k, at.y + half + k));
        break;
    }
    return label;
}

// Where along its edge a label is, for ordering them.
double alongEdge(const GridLabel& label)
{
    return label.edge == ViewportEdge::Bottom || label.edge == ViewportEdge::Top
               ? label.anchor.x
               : label.anchor.y;
}

// The edge order labels are taken in: bottom and left first, where a reader
// looks for coordinates, then top and right.
int edgeRank(ViewportEdge edge)
{
    switch (edge) {
    case ViewportEdge::Bottom:
        return 0;
    case ViewportEdge::Left:
        return 1;
    case ViewportEdge::Top:
        return 2;
    case ViewportEdge::Right:
        return 3;
    }
    return 4;
}

} // namespace

std::string_view toString(GridStyle style)
{
    for (const auto& [name, value] : kGridStyles) {
        if (value == style) {
            return name;
        }
    }
    return "none";
}

std::optional<GridStyle> gridStyleFrom(std::string_view name)
{
    for (const auto& [text, value] : kGridStyles) {
        if (text == name) {
            return value;
        }
    }
    return std::nullopt;
}

PlanPlacement storedPlacement(const Viewport& viewport)
{
    return {viewport.scale, viewport.centre};
}

Point2 planPaperToWorld(const Viewport& viewport, const PlanPlacement& at, const Point2& paper)
{
    const Point2 offset = (paper - viewport.rect.center()) * (at.scale / 1000.0);
    return at.centre + turned(offset, viewport.rotation);
}

Point2 planWorldToPaper(const Viewport& viewport, const PlanPlacement& at, const Point2& world)
{
    const Point2 offset = turned(world - at.centre, -viewport.rotation);
    return viewport.rect.center() + offset * (1000.0 / at.scale);
}

std::vector<Point2> planFootprint(const Viewport& viewport, const PlanPlacement& at)
{
    if (viewport.rect.empty() || !finite(at.scale) || !(at.scale > 0.0)) {
        return {};
    }
    const Box2& r = viewport.rect;
    return {planPaperToWorld(viewport, at, r.min),
            planPaperToWorld(viewport, at, Point2(r.max.x, r.min.y)),
            planPaperToWorld(viewport, at, r.max),
            planPaperToWorld(viewport, at, Point2(r.min.x, r.max.y))};
}

double automaticGridInterval(double scale)
{
    if (!finite(scale) || !(scale > 0.0)) {
        return 0.0;
    }
    // The spacing in metres that would fall exactly on the target, and the
    // round values either side of it. A power of ten below 1 divides rather
    // than multiplies, so 0.05 m is the double nearest 0.05, as a label and a
    // stored interval write it.
    const double target = kGridTargetSpacingMm * scale / 1000.0;
    const double exponent = std::floor(std::log10(target));
    const double power = std::pow(10.0, std::abs(exponent));
    const auto stepOf = [&](double multiple) {
        return exponent < 0.0 ? multiple / power : multiple * power;
    };
    double best = stepOf(1.0);
    double bestDistance = std::numeric_limits<double>::infinity();
    for (const double multiple : {1.0, 2.0, 5.0, 10.0}) {
        const double candidate = stepOf(multiple);
        const double distance = std::abs(std::log(candidate / target));
        if (distance < bestDistance - 1e-12) {
            best = candidate;
            bestDistance = distance;
        }
    }
    return best;
}

int gridDecimals(double interval)
{
    if (!finite(interval) || !(interval > 0.0)) {
        return 0;
    }
    for (int decimals = 0; decimals < 3; ++decimals) {
        const double scaled = interval * std::pow(10.0, decimals);
        if (std::abs(scaled - std::round(scaled)) < 1e-6 * std::max(1.0, scaled)) {
            return decimals;
        }
    }
    return 3;
}

std::string groupedCoordinate(double value, int decimals)
{
    decimals = std::clamp(decimals, 0, 6);
    std::string text = std::format("{:.{}f}", value, decimals);
    bool negative = false;
    if (!text.empty() && text.front() == '-') {
        negative = text.find_first_not_of("-0.") != std::string::npos;
        text.erase(0, 1);
    }
    const std::size_t point = text.find('.');
    const std::string whole = text.substr(0, point);
    const std::string fraction = point == std::string::npos ? std::string{} : text.substr(point);
    std::string grouped;
    for (std::size_t i = 0; i < whole.size(); ++i) {
        if (i > 0 && (whole.size() - i) % 3 == 0) {
            grouped += ' ';
        }
        grouped += whole[i];
    }
    return (negative ? "-" : "") + grouped + fraction;
}

Result<PlanGrid> planGrid(const Viewport& viewport, const PlanPlacement& at,
                          const PlanGridOptions& options)
{
    PlanGrid grid;
    grid.style = viewport.gridStyle;
    if (viewport.gridStyle == GridStyle::None) {
        return grid;
    }
    const Box2& rect = viewport.rect;
    if (rect.empty()) {
        return makeError(ErrorCode::InvalidArgument, "the viewport is not placed on the paper",
                         viewport.id);
    }
    if (!finite(at.scale) || !(at.scale > 0.0) || !finite(at.centre.x) ||
        !finite(at.centre.y) || !finite(viewport.rotation)) {
        return makeError(ErrorCode::InvalidArgument,
                         "the viewport's scale, centre or rotation is not a finite number",
                         viewport.id);
    }
    if (!finite(viewport.gridInterval) || viewport.gridInterval < 0.0) {
        return makeError(ErrorCode::InvalidArgument,
                         "a grid interval is a distance in metres, 0 for automatic",
                         std::format("{}: {}", viewport.id, viewport.gridInterval));
    }
    const double interval =
        viewport.gridInterval > 0.0 ? viewport.gridInterval : automaticGridInterval(at.scale);
    const double spacingMm = interval * 1000.0 / at.scale;
    if (!(spacingMm >= kGridMinimumSpacingMm)) {
        return makeError(
            ErrorCode::InvalidArgument,
            std::format("a {} m grid at 1:{} is {:.2g} mm apart on the paper; the closest a grid "
                        "may be is {} mm - choose a larger interval, or 0 for automatic",
                        groupedCoordinate(interval, gridDecimals(interval)), at.scale, spacingMm,
                        kGridMinimumSpacingMm),
            viewport.id);
    }
    grid.interval = interval;
    grid.decimals = gridDecimals(interval);

    // The round coordinates the ground under the viewport spans, on each axis.
    Box2 ground;
    for (const Point2& corner : planFootprint(viewport, at)) {
        ground.expand(corner);
    }
    const auto multiples = [interval](double from, double to) -> std::pair<double, double> {
        return {std::ceil(from / interval), std::floor(to / interval)};
    };
    const auto [e0, e1] = multiples(ground.min.x, ground.max.x);
    const auto [n0, n1] = multiples(ground.min.y, ground.max.y);
    if (e1 - e0 > kMaximumLinesPerAxis || n1 - n0 > kMaximumLinesPerAxis) {
        return makeError(ErrorCode::InvalidArgument,
                         "the grid would have too many lines to draw", viewport.id);
    }
    // World east and north on the paper.
    const Point2 east = turned(Point2(1.0, 0.0), -viewport.rotation);
    const Point2 north = turned(Point2(0.0, 1.0), -viewport.rotation);
    const auto addLine = [&](GridAxis axis, double value) {
        const Point2 world = axis == GridAxis::Easting ? Point2(value, at.centre.y)
                                                       : Point2(at.centre.x, value);
        const Point2 along = axis == GridAxis::Easting ? north : east;
        if (auto clipped = clipLine(planWorldToPaper(viewport, at, world), along, rect)) {
            grid.lines.push_back(
                {axis, value, clipped->from, clipped->to, clipped->fromEdge, clipped->toEdge});
        }
    };
    for (double k = e0; k <= e1; k += 1.0) {
        addLine(GridAxis::Easting, k * interval);
    }
    for (double k = n0; k <= n1; k += 1.0) {
        addLine(GridAxis::Northing, k * interval);
    }

    switch (grid.style) {
    case GridStyle::None:
        break;
    case GridStyle::Lines:
        for (const GridLine& line : grid.lines) {
            grid.strokes.push_back({line.from, line.to});
        }
        break;
    case GridStyle::Ticks:
        for (const GridLine& line : grid.lines) {
            const Point2 d = line.to - line.from;
            const double run = d.length();
            const Point2 unit = d * (1.0 / run);
            const double tick = std::min(options.tickMm, run / 2.0);
            grid.strokes.push_back({line.from, line.from + unit * tick});
            grid.strokes.push_back({line.to, line.to - unit * tick});
        }
        break;
    case GridStyle::Crosses: {
        // Each cross whole inside the viewport: one cut by the border reads as
        // a tick that is not there.
        const double arm = options.crossMm / 2.0;
        const Box2 inside(Point2(rect.min.x + arm, rect.min.y + arm),
                          Point2(rect.max.x - arm, rect.max.y - arm));
        for (double i = e0; i <= e1; i += 1.0) {
            for (double j = n0; j <= n1; j += 1.0) {
                const Point2 crossing =
                    planWorldToPaper(viewport, at, Point2(i * interval, j * interval));
                if (!inside.contains(crossing)) {
                    continue;
                }
                grid.crossings.push_back(crossing);
                grid.strokes.push_back({crossing - east * arm, crossing + east * arm});
                grid.strokes.push_back({crossing - north * arm, crossing + north * arm});
            }
        }
        break;
    }
    }

    // The labels: one where each line meets each edge, set along the edge and
    // centred on the line - past the tick, for ticks - and kept only where it
    // fits inside the viewport, clear of the furniture and of the labels
    // already placed.
    const double inset =
        grid.style == GridStyle::Ticks ? options.tickMm + options.labelGapMm : options.labelGapMm;
    const auto widthOf = [&options](std::string_view text) {
        return options.labelWidthMm ? options.labelWidthMm(text)
                                    : 0.75 * options.labelCapMm * static_cast<double>(text.size());
    };
    std::vector<GridLabel> candidates;
    for (const GridLine& line : grid.lines) {
        std::string text = labelText(line, grid.decimals);
        const double width = widthOf(text);
        candidates.push_back(
            labelAt(line, text, width, line.fromEdge, line.from, rect, inset, options));
        candidates.push_back(
            labelAt(line, std::move(text), width, line.toEdge, line.to, rect, inset, options));
    }
    std::stable_sort(candidates.begin(), candidates.end(),
                     [](const GridLabel& a, const GridLabel& b) {
                         const int ra = edgeRank(a.edge);
                         const int rb = edgeRank(b.edge);
                         return ra != rb ? ra < rb : alongEdge(a) < alongEdge(b);
                     });
    const auto clear = [](const Box2& a, const Box2& b) {
        return a.max.x <= b.min.x || b.max.x <= a.min.x || a.max.y <= b.min.y ||
               b.max.y <= a.min.y;
    };
    for (GridLabel& label : candidates) {
        if (!rect.contains(label.box)) {
            continue;
        }
        const auto covers = [&label, &clear](const Box2& box) {
            return !box.empty() && !clear(box, label.box);
        };
        const double spacing = std::max(options.labelSpacingMm, 0.0);
        const bool blocked =
            std::ranges::any_of(options.keepOut, covers) ||
            std::ranges::any_of(grid.labels, [&](const GridLabel& placed) {
                return !clear(placed.box.inflated(spacing), label.box);
            });
        if (!blocked) {
            grid.labels.push_back(std::move(label));
        }
    }
    return grid;
}

Status setPlanGrid(Document& document, std::string_view viewportId, GridStyle style,
                   double intervalM)
{
    if (!finite(intervalM) || intervalM < 0.0) {
        return makeError(ErrorCode::InvalidArgument,
                         "a grid interval is a distance in metres, 0 for automatic",
                         std::format("{}", intervalM));
    }
    return editViewport(
        document, viewportId,
        [style, intervalM](Viewport& viewport) -> Status {
            if (viewport.kind != ViewportKind::Plan && viewport.kind != ViewportKind::KeyPlan) {
                return makeError(ErrorCode::InvalidArgument,
                                 "only a plan or a key plan has a coordinate grid",
                                 std::string(toString(viewport.kind)));
            }
            viewport.gridStyle = style;
            viewport.gridInterval = intervalM;
            return {};
        },
        "SET_PLAN_GRID");
}

} // namespace katana::cad::plotting
