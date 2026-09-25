#include "katana/cad/plotting/arrange.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <format>
#include <limits>
#include <numbers>
#include <type_traits>
#include <utility>
#include <variant>

#include "katana/cad/selection.hpp"
#include "katana/entity/entity_geometry.hpp"
#include "katana/geometry/alignment.hpp"
#include "katana/geometry/polygon.hpp"

namespace katana::cad::plotting {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;
using katana::geometry::Vec2;

namespace {

constexpr double kPi = std::numbers::pi;
constexpr double kHalfPi = kPi / 2.0;
constexpr double kSnapRadians = kRotationSnapDegrees * kPi / 180.0;
// Two needed scales this close, relatively, are the same scale: they differ
// by rounding, not by anything a sheet could show.
constexpr double kScaleTie = 1e-9;
// Paper millimetres below which two edges are the same edge.
constexpr double kPaperEpsilon = 1e-9;
// How far apart, at most, the points an arc or circle is traced by are.
constexpr double kArcStep = 2.0 * kPi / 32.0;
// How far an alignment's chords may stray from it in the drawing's outline,
// and how far apart a view's stretch of one is sampled, metres; a stretch is
// never sampled at more points than this.
constexpr double kAlignmentChordM = 0.05;
constexpr double kAlignmentStepM = 0.5;
constexpr double kMostAlignmentSteps = 20000.0;
// autoArrange's steps: the views that give way shrink by 5% at a time, the
// main view by 10% of its size at a time.
constexpr double kShrinkStep = 0.95;
constexpr double kMainShrinkStep = 0.1;
constexpr int kMostShrinkSteps = 200;

// `angle` turned by whole half turns into (-pi/2, pi/2]: a drawing turned
// half a turn needs the same room upside down.
double halfTurnNormal(double angle)
{
    double a = std::remainder(angle, kPi);
    if (a <= -kHalfPi) {
        a += kPi;
    }
    return a;
}

// Whether `a` is nearer 0 than `b`, the positive one winning a tie: the
// rotation a person expects when two turn the drawing as well.
bool nearerZero(double a, double b)
{
    if (std::abs(a) != std::abs(b)) {
        return std::abs(a) < std::abs(b);
    }
    return a > b;
}

// The first of kSheetScales at or above `scale`, a needed scale a rounding
// above a step counting as that step.
double standardScaleFor(double scale)
{
    if (!(scale > 0.0)) {
        return 0.0;
    }
    if (scale > kSheetScales.back() * (1.0 + kScaleTie)) {
        return scale; // beyond the ladder, as sheetScaleAtLeast says
    }
    return sheetScaleAtLeast(scale / (1.0 + kScaleTie)).valueOr(scale);
}

// The content's hull about a point in its middle, so the millions of metres
// of a projected coordinate do not cost the digits a rotation needs.
struct Hull {
    Point2 origin{};
    std::vector<Point2> points; // counter-clockwise, relative to origin
};

Result<Hull> hullOf(std::span<const Point2> content)
{
    Box2 box;
    for (const Point2& point : content) {
        if (!point.isFinite()) {
            return makeError(ErrorCode::InvalidArgument, "a point of the content is not finite");
        }
        box.expand(point);
    }
    if (box.empty()) {
        return makeError(ErrorCode::InvalidArgument, "there is no content to fit");
    }
    Hull hull;
    hull.origin = box.center();
    std::vector<Point2> relative;
    relative.reserve(content.size());
    for (const Point2& point : content) {
        relative.push_back(point - hull.origin);
    }
    hull.points = geometry::convexHull(std::move(relative));
    if (hull.points.size() < 2) {
        return makeError(ErrorCode::InvalidArgument,
                         "the content is a single point and has no size to fit");
    }
    return hull;
}

Status checkRectangle(SizeMm rectangle)
{
    if (!(rectangle.width > 0.0) || !(rectangle.height > 0.0) ||
        !std::isfinite(rectangle.width) || !std::isfinite(rectangle.height)) {
        return makeError(ErrorCode::InvalidArgument, "the rectangle to fit has no size");
    }
    return {};
}

Vec2 across(double rotation)
{
    return Vec2(std::cos(rotation), std::sin(rotation));
}

Vec2 up(double rotation)
{
    return Vec2(-std::sin(rotation), std::cos(rotation));
}

RotationFit fitOf(const Hull& hull, SizeMm rectangle, double rotation)
{
    const Vec2 u = across(rotation);
    const Vec2 v = up(rotation);
    double minU = std::numeric_limits<double>::infinity();
    double maxU = -minU;
    double minV = minU;
    double maxV = -minU;
    for (const Point2& point : hull.points) {
        minU = std::min(minU, point.dot(u));
        maxU = std::max(maxU, point.dot(u));
        minV = std::min(minV, point.dot(v));
        maxV = std::max(maxV, point.dot(v));
    }
    RotationFit fit;
    fit.rotation = rotation;
    fit.width = maxU - minU;
    fit.height = maxV - minV;
    fit.centre = hull.origin + u * ((minU + maxU) / 2.0) + v * ((minV + maxV) / 2.0);
    fit.scale = std::max(fit.width * 1000.0 / rectangle.width,
                         fit.height * 1000.0 / rectangle.height);
    fit.standardScale = standardScaleFor(fit.scale);
    return fit;
}

// One stretch of the rotating calipers: between two orientations at which a
// hull edge lies along or square to the paper, the same pair of vertices is
// widest across the paper and the same pair up it, so the width and height
// are sinusoids of the rotation there.
struct Stretch {
    double from = 0.0;
    double to = 0.0; // > from; the last stretch runs past pi/2 to wrap round
    Vec2 across;     // the widest vertex along the rotation less the narrowest
    Vec2 up;         // the same square to it
};

double neededScale(const Stretch& stretch, SizeMm rectangle, double rotation)
{
    return std::max(stretch.across.dot(across(rotation)) * 1000.0 / rectangle.width,
                    stretch.up.dot(up(rotation)) * 1000.0 / rectangle.height);
}

// The vertex furthest along `direction`, climbing counter-clockwise from
// `start`: the hull is convex and the direction only ever turns
// counter-clockwise, so the furthest vertex only ever moves that way.
std::size_t furthest(const std::vector<Point2>& hull, Vec2 direction, std::size_t start)
{
    std::size_t at = start;
    for (std::size_t step = 0; step < hull.size(); ++step) {
        const std::size_t next = (at + 1) % hull.size();
        if (!(hull[next].dot(direction) > hull[at].dot(direction))) {
            break;
        }
        at = next;
    }
    return at;
}

std::size_t furthestOfAll(const std::vector<Point2>& hull, Vec2 direction)
{
    std::size_t best = 0;
    for (std::size_t i = 1; i < hull.size(); ++i) {
        if (hull[i].dot(direction) > hull[best].dot(direction)) {
            best = i;
        }
    }
    return best;
}

std::vector<Stretch> calipers(const std::vector<Point2>& hull)
{
    // Every orientation at which an edge lies along or square to the paper,
    // in (-pi/2, pi/2].
    std::vector<double> angles;
    for (std::size_t i = 0; i < hull.size(); ++i) {
        const Vec2 edge = hull[(i + 1) % hull.size()] - hull[i];
        const double base = edge.angle() - std::floor(edge.angle() / kHalfPi) * kHalfPi;
        angles.push_back(halfTurnNormal(base));
        angles.push_back(halfTurnNormal(base - kHalfPi));
    }
    std::sort(angles.begin(), angles.end());
    angles.erase(std::unique(angles.begin(), angles.end(),
                             [](double a, double b) { return b - a <= 1e-12; }),
                 angles.end());
    std::vector<Stretch> stretches;
    std::array<std::size_t, 4> support{};
    for (std::size_t i = 0; i < angles.size(); ++i) {
        Stretch stretch;
        stretch.from = angles[i];
        stretch.to = i + 1 < angles.size() ? angles[i + 1] : angles.front() + kPi;
        if (!(stretch.to > stretch.from)) {
            continue;
        }
        const double middle = (stretch.from + stretch.to) / 2.0;
        const std::array<Vec2, 4> directions{across(middle), across(middle) * -1.0, up(middle),
                                             up(middle) * -1.0};
        for (std::size_t d = 0; d < 4; ++d) {
            support[d] = stretches.empty() ? furthestOfAll(hull, directions[d])
                                           : furthest(hull, directions[d], support[d]);
        }
        stretch.across = hull[support[0]] - hull[support[1]];
        stretch.up = hull[support[2]] - hull[support[3]];
        stretches.push_back(stretch);
    }
    return stretches;
}

struct Optimum {
    double rotation = 0.0;
    double scale = 0.0;
};

// The least scale any rotation needs, and the rotation nearest 0 that needs
// it. In a stretch the width and height are concave in the rotation (a
// cosine near its crest), so their larger is least at an end of the stretch
// or where the two ask the same scale.
Optimum optimum(const std::vector<Stretch>& stretches, SizeMm rectangle)
{
    Optimum best{0.0, std::numeric_limits<double>::infinity()};
    const auto consider = [&](const Stretch& stretch, double rotation) {
        const double scale = neededScale(stretch, rectangle, rotation);
        const double normal = halfTurnNormal(rotation);
        if (scale < best.scale * (1.0 - kScaleTie) ||
            (scale <= best.scale * (1.0 + kScaleTie) && nearerZero(normal, best.rotation))) {
            best = {normal, std::min(scale, best.scale)};
        }
    };
    for (const Stretch& stretch : stretches) {
        consider(stretch, stretch.from);
        // H * (across . u) = W * (up . v): A cos t + B sin t = 0.
        const double a = rectangle.height * stretch.across.x - rectangle.width * stretch.up.y;
        const double b = rectangle.height * stretch.across.y + rectangle.width * stretch.up.x;
        if (a == 0.0 && b == 0.0) {
            continue;
        }
        const double root = std::atan2(-a, b);
        for (int k = -2; k <= 2; ++k) {
            const double rotation = root + k * kPi;
            if (rotation > stretch.from && rotation < stretch.to) {
                consider(stretch, rotation);
            }
        }
    }
    return best;
}

// The rotation nearest 0 at which the content needs no more than `scale`.
// The rotations that fit make a closed set, so the nearest to 0 is 0 itself
// or a rotation where the width or the height just reaches its limit: those
// are solved in each stretch, where each is a cosine.
std::optional<double> leastFeasible(const Hull& hull, const std::vector<Stretch>& stretches,
                                    SizeMm rectangle, double scale, const Optimum& best)
{
    const double limit = scale * (1.0 + kScaleTie);
    if (fitOf(hull, rectangle, 0.0).scale <= limit) {
        return 0.0;
    }
    std::optional<double> chosen;
    const auto consider = [&](double rotation, double needed) {
        const double normal = halfTurnNormal(rotation);
        if (needed <= limit && (!chosen || nearerZero(normal, *chosen))) {
            chosen = normal;
        }
    };
    if (best.scale <= limit) {
        consider(best.rotation, best.scale);
    }
    const auto roots = [](Vec2 pair, double reach, double shift, auto&& found) {
        // pair . across(t + shift) = reach: |pair| cos(t + shift - phase) = reach.
        const double length = pair.length();
        if (!(length > 0.0) || reach > length) {
            return;
        }
        const double phase = pair.angle();
        const double half = std::acos(std::clamp(reach / length, -1.0, 1.0));
        for (const double root : {phase - shift + half, phase - shift - half}) {
            for (int k = -2; k <= 2; ++k) {
                found(root + 2.0 * kPi * k);
            }
        }
    };
    for (const Stretch& stretch : stretches) {
        const auto inStretch = [&](double rotation) {
            if (rotation >= stretch.from - 1e-12 && rotation <= stretch.to + 1e-12) {
                consider(rotation, neededScale(stretch, rectangle, rotation));
            }
        };
        roots(stretch.across, rectangle.width * scale / 1000.0, 0.0, inStretch);
        roots(stretch.up, rectangle.height * scale / 1000.0, kHalfPi, inStretch);
    }
    // A turn within a degree of square is squared when it still fits.
    if (chosen && std::abs(halfTurnNormal(*chosen - kHalfPi)) <= kSnapRadians &&
        fitOf(hull, rectangle, kHalfPi).scale <= limit) {
        chosen = kHalfPi;
    }
    return chosen;
}

// ---- autoArrange ----------------------------------------------------------------------

// Whether the insides of two rectangles meet; touching edges do not.
bool overlaps(const Box2& a, const Box2& b)
{
    return !a.empty() && !b.empty() && a.min.x < b.max.x - kPaperEpsilon &&
           b.min.x < a.max.x - kPaperEpsilon && a.min.y < b.max.y - kPaperEpsilon &&
           b.min.y < a.max.y - kPaperEpsilon;
}

bool overlapsAny(const Box2& box, const std::vector<Box2>& others)
{
    return std::ranges::any_of(others, [&box](const Box2& other) { return overlaps(box, other); });
}

// `rect` moved - and cut, where it is bigger - to lie inside `box`.
Box2 clampInto(const Box2& rect, const Box2& box)
{
    const double width = std::min(rect.width(), box.width());
    const double height = std::min(rect.height(), box.height());
    // std::clamp needs its low end at or below its high end; a rectangle as
    // wide as the box can leave box.max.x - width a rounding below box.min.x.
    const double x = std::clamp(rect.min.x, box.min.x, std::max(box.min.x, box.max.x - width));
    const double y = std::clamp(rect.min.y, box.min.y, std::max(box.min.y, box.max.y - height));
    return Box2(Point2(x, y), Point2(x + width, y + height));
}

// The free space as maximal empty rectangles, each carved round what is
// taken: the "maximal rectangles" of a bin packer. A rectangle inside another
// is dropped; of two equal ones, the first is kept.
void carve(std::vector<Box2>& rooms, const Box2& taken)
{
    std::vector<Box2> pieces;
    for (const Box2& room : rooms) {
        if (!overlaps(room, taken)) {
            pieces.push_back(room);
            continue;
        }
        if (taken.min.x > room.min.x + kPaperEpsilon) {
            pieces.push_back(Box2(room.min, Point2(taken.min.x, room.max.y)));
        }
        if (taken.max.x < room.max.x - kPaperEpsilon) {
            pieces.push_back(Box2(Point2(taken.max.x, room.min.y), room.max));
        }
        if (taken.min.y > room.min.y + kPaperEpsilon) {
            pieces.push_back(Box2(room.min, Point2(room.max.x, taken.min.y)));
        }
        if (taken.max.y < room.max.y - kPaperEpsilon) {
            pieces.push_back(Box2(Point2(room.min.x, taken.max.y), room.max));
        }
    }
    rooms.clear();
    for (std::size_t i = 0; i < pieces.size(); ++i) {
        bool inside = false;
        for (std::size_t j = 0; j < pieces.size() && !inside; ++j) {
            inside = j != i && pieces[j].contains(pieces[i]) && (pieces[j] != pieces[i] || j < i);
        }
        if (!inside) {
            rooms.push_back(pieces[i]);
        }
    }
}

// Where a `width` x `height` rectangle goes in the free space: the top-left
// corner of the free rectangle that holds it highest up, then furthest left -
// the order a sheet is read in.
std::optional<Box2> placeIn(const std::vector<Box2>& rooms, double width, double height)
{
    const Box2* best = nullptr;
    for (const Box2& room : rooms) {
        if (width > room.width() + kPaperEpsilon || height > room.height() + kPaperEpsilon) {
            continue;
        }
        if (best == nullptr || room.max.y > best->max.y + kPaperEpsilon ||
            (room.max.y >= best->max.y - kPaperEpsilon && room.min.x < best->min.x - kPaperEpsilon)) {
            best = &room;
        }
    }
    if (best == nullptr) {
        return std::nullopt;
    }
    return Box2(Point2(best->min.x, best->max.y - height), Point2(best->min.x + width, best->max.y));
}

struct PackItem {
    std::size_t index = 0; // into the sheet's viewports
    SizeMm size;           // what it would like
    SizeMm least;          // never smaller than this
};

// An item's size at `factor`, no larger in area than `cap` (when positive)
// and never below its least.
SizeMm sizedAt(const PackItem& item, double factor, double cap)
{
    double width = item.size.width * factor;
    double height = item.size.height * factor;
    if (cap > 0.0 && width * height > cap) {
        const double k = std::sqrt(cap / (width * height));
        width *= k;
        height *= k;
    }
    return {std::max(width, item.least.width), std::max(height, item.least.height)};
}

// Every item placed at `factor`, or nothing when one does not fit.
// `capByFirst`: the first item is the main view, and no later one may be
// larger in area than it is placed - how the main view stays the largest
// when it is packed with the rest.
std::optional<std::vector<Box2>> packAt(const std::vector<PackItem>& items,
                                        const std::vector<Box2>& taken, const Box2& space,
                                        double gutter, double factor, double cap,
                                        bool capByFirst = false)
{
    std::vector<Box2> rooms{space};
    for (const Box2& box : taken) {
        carve(rooms, box.inflated(gutter));
    }
    std::vector<Box2> placed;
    for (const PackItem& item : items) {
        const double limit = capByFirst && !placed.empty()
                                 ? placed.front().width() * placed.front().height()
                                 : cap;
        const SizeMm size = sizedAt(item, factor, limit);
        const std::optional<Box2> at = placeIn(rooms, size.width, size.height);
        if (!at) {
            return std::nullopt;
        }
        carve(rooms, at->inflated(gutter));
        placed.push_back(*at);
    }
    return placed;
}

// The items packed at the largest of 1, 0.95, 0.95^2 ... that fits, down to
// each one's least size.
std::optional<std::vector<Box2>> packShrinking(const std::vector<PackItem>& items,
                                               const std::vector<Box2>& taken, const Box2& space,
                                               double gutter, double cap, bool capByFirst = false)
{
    double factor = 1.0;
    for (int step = 0; step < kMostShrinkSteps; ++step) {
        if (auto placed = packAt(items, taken, space, gutter, factor, cap, capByFirst)) {
            return placed;
        }
        const bool atLeast = std::ranges::all_of(items, [factor](const PackItem& item) {
            return item.size.width * factor <= item.least.width &&
                   item.size.height * factor <= item.least.height;
        });
        if (atLeast) {
            break;
        }
        factor *= kShrinkStep;
    }
    return std::nullopt;
}

// The preset tiling would use for `count` views, for the size an unplaced
// one would be given.
TilingPreset presetFor(std::size_t count)
{
    switch (count) {
    case 1: return TilingPreset::Full;
    case 2: return TilingPreset::MainRight;
    case 3: return TilingPreset::MainTwoRight;
    default: return TilingPreset::Quad;
    }
}

Viewport* findViewport(Sheet& sheet, std::string_view id)
{
    for (Viewport& viewport : sheet.viewports) {
        if (viewport.id == id) {
            return &viewport;
        }
    }
    return nullptr;
}

// The ids asked for, each once, in the order given; NotFound for one not on
// the sheet.
Result<std::vector<Viewport*>> viewportsOf(Sheet& sheet, std::span<const std::string> ids)
{
    std::vector<Viewport*> found;
    for (const std::string& id : ids) {
        Viewport* viewport = findViewport(sheet, id);
        if (viewport == nullptr) {
            return makeError(ErrorCode::NotFound, "no viewport of that id on the sheet", id);
        }
        if (std::ranges::find(found, viewport) == found.end()) {
            found.push_back(viewport);
        }
    }
    return found;
}

// The ids of `sheet`'s viewports whose rectangle differs from `before`, in
// the sheet's order.
std::vector<std::string> movedSince(const Sheet& sheet, const std::vector<Box2>& before)
{
    std::vector<std::string> moved;
    for (std::size_t i = 0; i < sheet.viewports.size(); ++i) {
        if (sheet.viewports[i].rect != before[i]) {
            moved.push_back(sheet.viewports[i].id);
        }
    }
    return moved;
}

std::vector<Box2> rectsOf(const Sheet& sheet)
{
    std::vector<Box2> rects;
    rects.reserve(sheet.viewports.size());
    for (const Viewport& viewport : sheet.viewports) {
        rects.push_back(viewport.rect);
    }
    return rects;
}

bool isSection(ViewportKind kind)
{
    return kind == ViewportKind::LongSection || kind == ViewportKind::CrossSections;
}

bool isPlan(ViewportKind kind)
{
    return kind == ViewportKind::Plan || kind == ViewportKind::KeyPlan;
}

std::string scaleWords(double scale)
{
    // A whole denominator as a scale rule reads it, never 1:1e+06.
    return scale == std::round(scale) ? std::format("1:{:.0f}", scale) : std::format("1:{}", scale);
}

// A kind as a message names it.
std::string_view kindWords(ViewportKind kind)
{
    switch (kind) {
    case ViewportKind::Plan: return "plan";
    case ViewportKind::LongSection: return "long section";
    case ViewportKind::CrossSections: return "cross-section";
    case ViewportKind::Model3D: return "3D snapshot";
    case ViewportKind::Legend: return "legend";
    case ViewportKind::Notes: return "notes";
    case ViewportKind::Image: return "image";
    case ViewportKind::KeyPlan: return "key plan";
    case ViewportKind::SheetIndex: return "drawing register";
    case ViewportKind::Revisions: return "revision table";
    }
    return "plan";
}

} // namespace

// ---- Rotation -------------------------------------------------------------------------

Result<RotationFit> fitAtRotation(std::span<const Point2> content, SizeMm rectangle, double rotation)
{
    if (auto checked = checkRectangle(rectangle); !checked) {
        return checked.error();
    }
    if (!std::isfinite(rotation)) {
        return makeError(ErrorCode::InvalidArgument, "the rotation is not finite");
    }
    auto hull = hullOf(content);
    if (!hull) {
        return hull.error();
    }
    return fitOf(*hull, rectangle, rotation);
}

Result<RotationFit> bestFitRotation(std::span<const Point2> content, SizeMm rectangle)
{
    if (auto checked = checkRectangle(rectangle); !checked) {
        return checked.error();
    }
    auto hull = hullOf(content);
    if (!hull) {
        return hull.error();
    }
    const Optimum best = optimum(calipers(hull->points), rectangle);
    double rotation = best.rotation;
    for (const double square : {0.0, kHalfPi}) {
        if (std::abs(halfTurnNormal(rotation - square)) <= kSnapRadians) {
            rotation = square;
        }
    }
    return fitOf(*hull, rectangle, rotation);
}

Result<RotationFit> leastRotationToFit(std::span<const Point2> content, SizeMm rectangle,
                                       double scale)
{
    if (auto checked = checkRectangle(rectangle); !checked) {
        return checked.error();
    }
    if (!(scale > 0.0) || !std::isfinite(scale)) {
        return makeError(ErrorCode::InvalidArgument, "the scale must be a positive number");
    }
    auto hull = hullOf(content);
    if (!hull) {
        return hull.error();
    }
    const std::vector<Stretch> stretches = calipers(hull->points);
    const Optimum best = optimum(stretches, rectangle);
    const std::optional<double> rotation = leastFeasible(*hull, stretches, rectangle, scale, best);
    if (!rotation) {
        return makeError(ErrorCode::InvalidArgument,
                         std::format("the content does not fit {} at any rotation; turned its "
                                     "best it needs {}",
                                     scaleWords(scale), scaleWords(standardScaleFor(best.scale))));
    }
    return fitOf(*hull, rectangle, *rotation);
}

Result<RotationFit> bestStandardRotation(std::span<const Point2> content, SizeMm rectangle)
{
    if (auto checked = checkRectangle(rectangle); !checked) {
        return checked.error();
    }
    auto hull = hullOf(content);
    if (!hull) {
        return hull.error();
    }
    const std::vector<Stretch> stretches = calipers(hull->points);
    const Optimum best = optimum(stretches, rectangle);
    // The best rotation needs best.scale, so its standard scale always fits
    // at that rotation at least.
    const double target = standardScaleFor(best.scale);
    const std::optional<double> rotation = leastFeasible(*hull, stretches, rectangle, target, best);
    return fitOf(*hull, rectangle, rotation.value_or(best.rotation));
}

std::vector<Point2> drawnOutline(const entity::Model& model, const LayerOverrides& view)
{
    std::vector<Point2> points;
    // An arc by its ends and points on it no more than kArcStep apart, and
    // between each two the corner of the polygon whose sides touch the arc at
    // them - half-way round, radius / cos(half the step) out - so the hull
    // holds the curve itself and not only its chords.
    const auto traceArc = [&points](const geometry::Arc2& arc) {
        const int steps =
            std::max(static_cast<int>(std::ceil(std::abs(arc.sweep) / kArcStep)), 1);
        const double half = arc.sweep / steps / 2.0;
        const double reach = arc.radius / std::cos(half);
        for (int i = 0; i <= steps; ++i) {
            points.push_back(arc.pointAt(static_cast<double>(i) / steps));
            if (i < steps) {
                const double angle = arc.startAngle + arc.sweep * i / steps + half;
                points.push_back(arc.center + Vec2(std::cos(angle), std::sin(angle)) * reach);
            }
        }
    };
    model.entities.forEach([&](const entity::Entity& entity) {
        if (!isDrawn(model, entity, view)) {
            return;
        }
        std::visit(
            [&](const auto& shape) {
                using Shape = std::decay_t<decltype(shape)>;
                if constexpr (std::is_same_v<Shape, entity::PointGeometry>) {
                    points.push_back(shape.position);
                } else if constexpr (std::is_same_v<Shape, geometry::Segment2>) {
                    points.push_back(shape.start);
                    points.push_back(shape.end);
                } else if constexpr (std::is_same_v<Shape, geometry::Arc2>) {
                    traceArc(shape);
                } else if constexpr (std::is_same_v<Shape, geometry::Polyline2>) {
                    points.insert(points.end(), shape.vertices.begin(), shape.vertices.end());
                } else if constexpr (std::is_same_v<Shape, geometry::Circle2>) {
                    traceArc(geometry::Arc2{shape.center, shape.radius, 0.0, 2.0 * kPi});
                } else {
                    // Texts and dimensions by their boxes: small beside what
                    // they label.
                    const Box2 box = entity::boundingBox(entity.geometry);
                    if (!box.empty()) {
                        points.insert(points.end(), {box.min, Point2(box.max.x, box.min.y), box.max,
                                                     Point2(box.min.x, box.max.y)});
                    }
                }
            },
            entity.geometry);
    });
    model.alignments.forEach([&points](const entity::Alignment& alignment) {
        if (const auto solved = geometry::solveAlignment(alignment.horizontal)) {
            // Chords within kAlignmentChordM of the curve: a hair on paper at
            // any scale a sheet is drawn at.
            const geometry::Polyline2 line = solved->toPolyline(kAlignmentChordM);
            points.insert(points.end(), line.vertices.begin(), line.vertices.end());
        }
    });
    std::erase_if(points, [](const Point2& point) { return !point.isFinite(); });
    return geometry::convexHull(std::move(points));
}

std::vector<Point2> viewportContent(const entity::Model& model, const Viewport& viewport)
{
    if (!isPlan(viewport.kind)) {
        return {};
    }
    std::vector<Point2> points;
    const ViewportSource& source = viewport.source;
    std::optional<geometry::SolvedAlignment> solved;
    if (!source.alignment.empty()) {
        if (const entity::Alignment* alignment = model.alignments.find(source.alignment)) {
            if (auto found = geometry::solveAlignment(alignment->horizontal)) {
                solved = std::move(*found);
            }
        }
    }
    if (solved) {
        // The stretch the view shows - the whole alignment for an empty range -
        // at points no more than kAlignmentStepM apart, and every point where
        // one element gives way to the next.
        double from = solved->startStation();
        double to = solved->endStation();
        if (source.chainageTo > source.chainageFrom) {
            from = std::max(source.chainageFrom, from);
            to = std::min(source.chainageTo, to);
        }
        if (to > from) {
            const auto steps = static_cast<int>(std::clamp(std::ceil((to - from) / kAlignmentStepM),
                                                           1.0, kMostAlignmentSteps));
            for (int i = 0; i <= steps; ++i) {
                if (const auto point = solved->pointAtStation(from + (to - from) * i / steps)) {
                    points.push_back(*point);
                }
            }
            for (const double station : solved->keyStations()) {
                if (station > from && station < to) {
                    if (const auto point = solved->pointAtStation(station)) {
                        points.push_back(*point);
                    }
                }
            }
        }
    }
    // No alignment, or a range wholly off it, which shows none of it: the
    // painter then fits the drawing (resolvePlanViewport), and so does this.
    if (points.empty()) {
        points = drawnOutline(model, viewport.hiddenLayers);
    }
    std::erase_if(points, [](const Point2& point) { return !point.isFinite(); });
    return geometry::convexHull(std::move(points));
}

std::vector<Point2> viewportContent(const entity::Model& model, const SheetSet& set,
                                    const Viewport& viewport, const PlanPlacer& place)
{
    if (viewport.kind != ViewportKind::KeyPlan) {
        return viewportContent(model, viewport);
    }
    std::optional<std::size_t> onSheet;
    for (std::size_t i = 0; i < set.sheets.size() && !onSheet; ++i) {
        for (const Viewport& each : set.sheets[i].viewports) {
            if (each.id == viewport.id) {
                onSheet = i;
                break;
            }
        }
    }
    std::vector<Point2> points;
    for (const KeyPlanOutline& outline :
         keyPlanOutlines(set, onSheet.value_or(set.sheets.size()), place ? place : modelPlacer(model))) {
        points.insert(points.end(), outline.corners.begin(), outline.corners.end());
    }
    std::erase_if(points, [](const Point2& point) { return !point.isFinite(); });
    if (points.empty()) {
        // No plan to outline: the painter fits the drawing (fitKeyPlan).
        return viewportContent(model, viewport);
    }
    return geometry::convexHull(std::move(points));
}

// ---- Paper and scale ------------------------------------------------------------------

Result<PaperAdvice> suggestPaper(std::span<const Point2> content, const PaperRequest& request)
{
    if (!(request.scale > 0.0) || !std::isfinite(request.scale)) {
        return makeError(ErrorCode::InvalidArgument, "the scale must be a positive number");
    }
    if (!(request.shareAcross > 0.0) || !(request.shareAcross <= 1.0) ||
        !(request.shareUp > 0.0) || !(request.shareUp <= 1.0)) {
        return makeError(ErrorCode::InvalidArgument,
                         "the share of the sheet the content may use must be more than 0 and at "
                         "most 1");
    }
    auto fit = fitAtRotation(content, {1.0, 1.0}, request.rotation);
    if (!fit) {
        return fit.error();
    }
    const double width = fit->width * 1000.0 / request.scale;
    const double height = fit->height * 1000.0 / request.scale;
    double bestOnA0 = std::numeric_limits<double>::infinity();
    for (const PaperSize paper :
         {PaperSize::A4, PaperSize::A3, PaperSize::A2, PaperSize::A1, PaperSize::A0}) {
        for (const bool landscape : {true, false}) {
            const Sheet sheet = blankSheet({paper, landscape, request.frame}, {});
            const Box2 area = tilingArea(sheet);
            const double roomAcross = area.width() * request.shareAcross;
            const double roomUp = area.height() * request.shareUp;
            if (!(roomAcross > 0.0) || !(roomUp > 0.0)) {
                continue;
            }
            const double fill = std::max(width / roomAcross, height / roomUp);
            if (fill <= 1.0 + kScaleTie) {
                return PaperAdvice{paper, landscape, sheet.frame, area, fill};
            }
            if (paper == PaperSize::A0) {
                bestOnA0 = std::min(bestOnA0, request.scale * fill);
            }
        }
    }
    return makeError(ErrorCode::NotFound,
                     std::format("at {} the content is {:.0f} x {:.0f} mm and not even A0 holds "
                                 "it; A0 would at {}",
                                 scaleWords(request.scale), width, height,
                                 scaleWords(standardScaleFor(bestOnA0))));
}

Result<PaperAdvice> suggestPaper(const Box2& extent, const PaperRequest& request)
{
    if (extent.empty()) {
        return makeError(ErrorCode::InvalidArgument, "there is no content to fit");
    }
    const std::array<Point2, 4> corners{extent.min, Point2(extent.max.x, extent.min.y), extent.max,
                                        Point2(extent.min.x, extent.max.y)};
    return suggestPaper(std::span<const Point2>(corners), request);
}

Result<double> suggestScale(std::span<const Point2> content, const Box2& rectangle, double rotation)
{
    auto fit = fitAtRotation(content, {rectangle.width(), rectangle.height()}, rotation);
    if (!fit) {
        return fit.error();
    }
    return fit->standardScale;
}

Result<double> suggestScale(std::span<const Point2> content, const Sheet& sheet, double rotation)
{
    return suggestScale(content, tilingArea(sheet), rotation);
}

std::vector<std::string> applyPaper(Sheet& sheet, const PaperAdvice& advice)
{
    const std::vector<Box2> before = rectsOf(sheet);
    const Box2 from = tilingArea(sheet);
    sheet.paper = advice.paper;
    sheet.landscape = advice.landscape;
    sheet.frame = advice.landscape ? advice.frame : std::string{};
    const Box2 to = tilingArea(sheet);
    const auto map = [&](Point2 point) {
        return Point2(to.min.x + (point.x - from.min.x) * to.width() / from.width(),
                      to.min.y + (point.y - from.min.y) * to.height() / from.height());
    };
    if (from != to && from.width() > 0.0 && from.height() > 0.0) {
        for (Viewport& viewport : sheet.viewports) {
            if (!viewport.rect.empty()) {
                viewport.rect = Box2(map(viewport.rect.min), map(viewport.rect.max));
            }
        }
    }
    return movedSince(sheet, before);
}

Result<PaperChange> fitPaperToViewport(Sheet& sheet, std::string_view viewportId,
                                       std::span<const Point2> content, double scale)
{
    Viewport* viewport = findViewport(sheet, viewportId);
    if (viewport == nullptr) {
        return makeError(ErrorCode::NotFound, "no viewport of that id on the sheet",
                         std::string(viewportId));
    }
    if (!isPlan(viewport->kind)) {
        return makeError(ErrorCode::InvalidArgument,
                         std::format("the paper is chosen for a plan or key plan; this is a {} "
                                     "view",
                                     kindWords(viewport->kind)),
                         viewport->id);
    }
    if (viewport->rect.empty()) {
        return makeError(ErrorCode::InvalidArgument, "the view is not placed on the paper",
                         viewport->id);
    }
    if (!(scale > 0.0) || !std::isfinite(scale)) {
        return makeError(ErrorCode::InvalidArgument, "the scale must be a positive number");
    }
    // The share of the room the view takes now: a main view beside panels
    // keeps its share, and the panels theirs, on the new paper. The content
    // is measured with the painter's room to spare, so it does not touch the
    // view's edge.
    const Box2 area = tilingArea(sheet);
    PaperRequest request;
    request.scale = scale / kAutoScaleSpare;
    request.rotation = viewport->rotation;
    request.frame = sheet.landscape ? sheet.frame : std::string(kBuiltInFrameId);
    if (area.width() > 0.0 && area.height() > 0.0) {
        request.shareAcross = std::clamp(viewport->rect.width() / area.width(), 0.0, 1.0);
        request.shareUp = std::clamp(viewport->rect.height() / area.height(), 0.0, 1.0);
    }
    auto advice = suggestPaper(content, request);
    if (!advice) {
        return advice.error();
    }
    // Measured centred, so drawn centred.
    const auto fit = fitAtRotation(content, {1.0, 1.0}, viewport->rotation);
    if (!fit) {
        return fit.error();
    }
    viewport->scale = scale;
    viewport->autoScale = false;
    viewport->centre = fit->centre;
    viewport->autoCentre = false;
    PaperChange change;
    change.advice = *advice;
    change.moved = applyPaper(sheet, *advice);
    return change;
}

// ---- Arranging viewports --------------------------------------------------------------

ArrangeResult autoArrange(Sheet& sheet, double gutterMm)
{
    ArrangeResult result;
    const Box2 space = tilingArea(sheet).inflated(-gutterMm / 2.0);
    // Inside the packing space, give or take a rounding: a tiled cell lies
    // exactly on its edge.
    const Box2 within = space.inflated(kPaperEpsilon);
    const std::vector<Box2> before = rectsOf(sheet);
    std::vector<Box2> locked;
    std::vector<std::size_t> order;
    for (std::size_t i = 0; i < sheet.viewports.size(); ++i) {
        if (sheet.viewports[i].locked) {
            if (!sheet.viewports[i].rect.empty()) {
                locked.push_back(sheet.viewports[i].rect);
            }
        } else {
            order.push_back(i);
        }
    }
    if (order.empty() || space.empty()) {
        return result;
    }
    std::ranges::stable_sort(order, [&sheet](std::size_t a, std::size_t b) {
        return tilingRank(sheet.viewports[a].kind) < tilingRank(sheet.viewports[b].kind);
    });

    // Where each view is now: brought inside when it has strayed out of the
    // drawing area; unplaced ones are sized as tiling would size them.
    const std::vector<Box2> cells = presetCells(presetFor(order.size()), tilingArea(sheet), gutterMm);
    std::vector<Box2> at(sheet.viewports.size());
    std::vector<PackItem> items(sheet.viewports.size());
    for (std::size_t rank = 0; rank < order.size(); ++rank) {
        const std::size_t i = order[rank];
        const Viewport& viewport = sheet.viewports[i];
        if (!viewport.rect.empty()) {
            at[i] = within.contains(viewport.rect) ? viewport.rect : clampInto(viewport.rect, space);
        }
        const Box2& cell = cells[std::min(rank, cells.size() - 1)];
        const SizeMm size = at[i].empty() ? SizeMm{cell.width(), cell.height()}
                                          : SizeMm{at[i].width(), at[i].height()};
        const SizeMm minimum = minimumSize(viewport.kind);
        items[i] = {i, size,
                    {std::min(minimum.width, size.width), std::min(minimum.height, size.height)}};
    }
    const std::size_t main = order.front();

    const auto place = [&sheet](const std::vector<std::size_t>& which, const std::vector<Box2>& rects) {
        for (std::size_t k = 0; k < which.size(); ++k) {
            sheet.viewports[which[k]].rect = rects[k];
        }
    };
    const auto itemsOf = [&items](const std::vector<std::size_t>& which) {
        std::vector<PackItem> list;
        for (const std::size_t i : which) {
            list.push_back(items[i]);
        }
        return list;
    };
    const auto areaOf = [](const Box2& box) { return box.width() * box.height(); };

    // Keep every view that overlaps nothing kept before it - the main view
    // first - and pack the rest into what is left. A main view in the way of
    // a locked one, or not placed, is packed first, with the rest, so it
    // still gets the most room. A view larger than the main one gives way
    // too: the main view is the largest on the sheet.
    std::vector<Box2> taken = locked;
    std::vector<std::size_t> kept;
    std::vector<std::size_t> displaced;
    const bool mainKept = !at[main].empty() && !overlapsAny(at[main], locked);
    const double mainArea = mainKept ? areaOf(at[main]) : 0.0;
    for (const std::size_t i : order) {
        if (mainKept && !at[i].empty() && !overlapsAny(at[i], taken) &&
            (i == main || areaOf(at[i]) <= mainArea + kPaperEpsilon)) {
            taken.push_back(at[i]);
            kept.push_back(i);
        } else {
            displaced.push_back(i);
        }
    }
    const double cap = mainArea;
    for (const std::size_t i : kept) {
        sheet.viewports[i].rect = at[i];
    }
    bool arranged = displaced.empty();
    if (!arranged && mainKept) {
        if (auto packed = packShrinking(itemsOf(displaced), taken, space, gutterMm, cap)) {
            place(displaced, *packed);
            arranged = true;
        }
    }
    // Then every view but the main one, packed again round it.
    const std::vector<std::size_t> others(order.begin() + (mainKept ? 1 : 0), order.end());
    std::vector<Box2> round = locked;
    if (mainKept) {
        round.push_back(at[main]);
    }
    if (!arranged) {
        if (auto packed = packShrinking(itemsOf(others), round, space, gutterMm, cap, !mainKept)) {
            place(others, *packed);
            arranged = true;
        }
    }
    // Then the main view made smaller from its top-left corner, a tenth at a
    // time, never below its least size.
    if (!arranged && mainKept) {
        const PackItem& item = items[main];
        for (int step = 1; !arranged && step < 10; ++step) {
            const SizeMm size = sizedAt(item, 1.0 - kMainShrinkStep * step, 0.0);
            const Box2 smaller(Point2(at[main].min.x, at[main].max.y - size.height),
                               Point2(at[main].min.x + size.width, at[main].max.y));
            std::vector<Box2> aroundSmaller = locked;
            aroundSmaller.push_back(smaller);
            if (auto packed = packShrinking(itemsOf(others), aroundSmaller, space, gutterMm,
                                            areaOf(smaller))) {
                sheet.viewports[main].rect = smaller;
                place(others, *packed);
                result.mainShrunk = true;
                arranged = true;
            }
            if (size.width <= item.least.width && size.height <= item.least.height) {
                break;
            }
        }
    }
    // Last, the views that were in the way are placed one at a time where
    // each still fits, shrinking in the same steps; the rest stay where they
    // were, and are reported.
    if (!arranged) {
        for (const std::size_t i : kept) {
            sheet.viewports[i].rect = at[i];
        }
        std::vector<Box2> rooms{space};
        for (const Box2& box : taken) {
            carve(rooms, box.inflated(gutterMm));
        }
        for (const std::size_t i : displaced) {
            sheet.viewports[i].rect = at[i].empty() ? before[i] : at[i];
            // No larger than the main view as it now stands.
            const Box2& mainRect = sheet.viewports[main].rect;
            const double limit = i == main ? 0.0 : mainKept ? cap : areaOf(mainRect);
            bool placed = false;
            double factor = 1.0;
            for (int step = 0; step < kMostShrinkSteps; ++step, factor *= kShrinkStep) {
                const SizeMm size = sizedAt(items[i], factor, limit);
                if (const auto spot = placeIn(rooms, size.width, size.height)) {
                    sheet.viewports[i].rect = *spot;
                    carve(rooms, spot->inflated(gutterMm));
                    placed = true;
                    break;
                }
                if (size.width <= items[i].least.width && size.height <= items[i].least.height) {
                    break;
                }
            }
            // One with no room stays where it was, and is reported; the
            // views placed after it keep clear of it rather than land on it.
            if (!placed && !sheet.viewports[i].rect.empty()) {
                carve(rooms, sheet.viewports[i].rect.inflated(gutterMm));
            }
        }
    }

    result.moved = movedSince(sheet, before);
    for (std::size_t i = 0; i < sheet.viewports.size(); ++i) {
        const Viewport& viewport = sheet.viewports[i];
        if (viewport.locked) {
            continue;
        }
        if (viewport.rect.empty()) {
            result.unplaced.push_back(viewport.id);
            continue;
        }
        for (std::size_t j = 0; j < sheet.viewports.size(); ++j) {
            if (j != i && overlaps(viewport.rect, sheet.viewports[j].rect)) {
                result.overlapping.push_back(viewport.id);
                break;
            }
        }
    }
    return result;
}

std::string_view toString(AlignEdge edge)
{
    switch (edge) {
    case AlignEdge::Left: return "left";
    case AlignEdge::Right: return "right";
    case AlignEdge::Top: return "top";
    case AlignEdge::Bottom: return "bottom";
    case AlignEdge::HorizontalCentre: return "hcentre";
    case AlignEdge::VerticalCentre: return "vcentre";
    }
    return "left";
}

std::optional<AlignEdge> alignEdgeFrom(std::string_view name)
{
    for (const AlignEdge edge : {AlignEdge::Left, AlignEdge::Right, AlignEdge::Top, AlignEdge::Bottom,
                                 AlignEdge::HorizontalCentre, AlignEdge::VerticalCentre}) {
        if (toString(edge) == name) {
            return edge;
        }
    }
    return std::nullopt;
}

Result<std::vector<std::string>> alignViewports(Sheet& sheet, std::span<const std::string> ids,
                                                AlignEdge edge)
{
    auto found = viewportsOf(sheet, ids);
    if (!found) {
        return found.error();
    }
    std::vector<Viewport*> placed;
    for (Viewport* viewport : *found) {
        if (!viewport->rect.empty()) {
            placed.push_back(viewport);
        }
    }
    if (placed.empty()) {
        return makeError(ErrorCode::InvalidArgument,
                         "there is nothing to align: none of the views is placed on the paper");
    }
    // The box the edges are taken from: the views' own, or the tiling area
    // for one alone.
    Box2 reference;
    if (placed.size() == 1) {
        reference = tilingArea(sheet);
    } else {
        for (const Viewport* viewport : placed) {
            reference.expand(viewport->rect);
        }
    }
    const std::vector<Box2> before = rectsOf(sheet);
    for (Viewport* viewport : placed) {
        if (viewport->locked) {
            continue;
        }
        Box2& rect = viewport->rect;
        Vec2 shift{};
        switch (edge) {
        case AlignEdge::Left: shift.x = reference.min.x - rect.min.x; break;
        case AlignEdge::Right: shift.x = reference.max.x - rect.max.x; break;
        case AlignEdge::Top: shift.y = reference.max.y - rect.max.y; break;
        case AlignEdge::Bottom: shift.y = reference.min.y - rect.min.y; break;
        case AlignEdge::HorizontalCentre: shift.x = reference.center().x - rect.center().x; break;
        case AlignEdge::VerticalCentre: shift.y = reference.center().y - rect.center().y; break;
        }
        rect = Box2(rect.min + shift, rect.max + shift);
    }
    return movedSince(sheet, before);
}

std::string_view toString(DistributeAxis axis)
{
    return axis == DistributeAxis::Horizontal ? "horizontal" : "vertical";
}

std::optional<DistributeAxis> distributeAxisFrom(std::string_view name)
{
    if (name == "horizontal") {
        return DistributeAxis::Horizontal;
    }
    if (name == "vertical") {
        return DistributeAxis::Vertical;
    }
    return std::nullopt;
}

Result<std::vector<std::string>> distributeViewports(Sheet& sheet, std::span<const std::string> ids,
                                                     DistributeAxis axis)
{
    auto found = viewportsOf(sheet, ids);
    if (!found) {
        return found.error();
    }
    std::vector<Viewport*> moving;
    for (Viewport* viewport : *found) {
        if (!viewport->rect.empty() && !viewport->locked) {
            moving.push_back(viewport);
        }
    }
    if (moving.size() < 3) {
        return makeError(ErrorCode::InvalidArgument,
                         std::format("spacing views out needs three or more placed, unlocked "
                                     "views; there {} {}",
                                     moving.size() == 1 ? "is" : "are", moving.size()));
    }
    const bool horizontal = axis == DistributeAxis::Horizontal;
    const auto low = [horizontal](const Viewport* v) {
        return horizontal ? v->rect.min.x : v->rect.min.y;
    };
    const auto high = [horizontal](const Viewport* v) {
        return horizontal ? v->rect.max.x : v->rect.max.y;
    };
    // Ties keep the sheet's order, whatever order the ids came in, so the
    // same views always space out the same way.
    std::ranges::sort(moving);
    std::ranges::stable_sort(moving, [&](const Viewport* a, const Viewport* b) {
        return low(a) < low(b) || (low(a) == low(b) && high(a) < high(b));
    });
    double inside = 0.0;
    for (std::size_t k = 1; k + 1 < moving.size(); ++k) {
        inside += high(moving[k]) - low(moving[k]);
    }
    const double gap =
        (low(moving.back()) - high(moving.front()) - inside) / static_cast<double>(moving.size() - 1);
    // Wider together than the room between the first and the last, equal
    // gaps would be negative: the views pushed into each other. Refused, and
    // nothing moves.
    if (gap < -kPaperEpsilon) {
        return makeError(ErrorCode::InvalidArgument,
                         std::format("the views between the first and the last are {:.1f} mm {} "
                                     "together, and there is {:.1f} mm between those two: spaced "
                                     "out they would overlap; move the outer ones apart first",
                                     inside, horizontal ? "wide" : "high",
                                     low(moving.back()) - high(moving.front())));
    }
    const std::vector<Box2> before = rectsOf(sheet);
    double next = high(moving.front()) + gap;
    for (std::size_t k = 1; k + 1 < moving.size(); ++k) {
        Box2& rect = moving[k]->rect;
        const double shift = next - low(moving[k]);
        rect = horizontal ? Box2(rect.min + Vec2(shift, 0.0), rect.max + Vec2(shift, 0.0))
                          : Box2(rect.min + Vec2(0.0, shift), rect.max + Vec2(0.0, shift));
        next = high(moving[k]) + gap;
    }
    return movedSince(sheet, before);
}

bool hasScale(ViewportKind kind)
{
    return isPlan(kind) || isSection(kind);
}

Result<std::vector<std::string>> matchScale(SheetSet& set, std::span<const std::string> ids,
                                            std::string_view fromId, std::optional<double> fromScale,
                                            std::optional<double> fromExaggeration)
{
    const auto find = [&set](std::string_view id) -> Viewport* {
        for (Sheet& sheet : set.sheets) {
            if (Viewport* viewport = findViewport(sheet, id)) {
                return viewport;
            }
        }
        return nullptr;
    };
    const Viewport* from = find(fromId);
    if (from == nullptr) {
        return makeError(ErrorCode::NotFound, "no viewport of that id to take the scale from",
                         std::string(fromId));
    }
    if (!hasScale(from->kind)) {
        return makeError(ErrorCode::InvalidArgument,
                         std::format("a {} view has no scale to match", kindWords(from->kind)),
                         std::string(fromId));
    }
    const double scale = fromScale.value_or(from->scale);
    if (!(scale > 0.0) || !std::isfinite(scale)) {
        return makeError(ErrorCode::InvalidArgument, "the scale must be a positive number");
    }
    const ViewportKind fromKind = from->kind;
    const double exaggeration = fromExaggeration.value_or(from->verticalExaggeration);
    std::vector<Viewport*> targets;
    for (const std::string& id : ids) {
        Viewport* viewport = find(id);
        if (viewport == nullptr) {
            return makeError(ErrorCode::NotFound, "no viewport of that id", id);
        }
        targets.push_back(viewport);
    }
    std::vector<std::string> changed;
    for (Viewport* viewport : targets) {
        if (viewport->id == fromId || !hasScale(viewport->kind)) {
            continue;
        }
        const Viewport was = *viewport;
        viewport->scale = scale;
        viewport->autoScale = false;
        if (isSection(fromKind) && isSection(viewport->kind)) {
            viewport->verticalExaggeration = exaggeration;
        }
        if (!(*viewport == was) && std::ranges::find(changed, viewport->id) == changed.end()) {
            changed.push_back(viewport->id);
        }
    }
    return changed;
}

Status fitViewportToContent(Viewport& viewport, std::span<const Point2> content,
                            const Box2& drawingArea)
{
    if (!hasScale(viewport.kind)) {
        return makeError(ErrorCode::InvalidArgument,
                         std::format("a {} view is not drawn to a scale, so it has no size to fit",
                                     kindWords(viewport.kind)),
                         viewport.id);
    }
    if (viewport.rect.empty()) {
        return makeError(ErrorCode::InvalidArgument, "the view is not placed on the paper",
                         viewport.id);
    }
    if (!(viewport.scale > 0.0) || !std::isfinite(viewport.scale)) {
        return makeError(ErrorCode::InvalidArgument, "the view has no scale", viewport.id);
    }
    const bool section = isSection(viewport.kind);
    const double exaggeration = section && viewport.verticalExaggeration > 0.0
                                    ? viewport.verticalExaggeration
                                    : 1.0;
    auto fit = fitAtRotation(content, {1.0, 1.0}, section ? 0.0 : viewport.rotation);
    if (!fit) {
        return fit.error();
    }
    const SizeMm minimum = minimumSize(viewport.kind);
    double width = std::max(fit->width * 1000.0 / viewport.scale * kFitSpare, minimum.width);
    double height =
        std::max(fit->height * 1000.0 / viewport.scale * exaggeration * kFitSpare, minimum.height);
    if (!drawingArea.empty()) {
        width = std::min(width, drawingArea.width());
        height = std::min(height, drawingArea.height());
    }
    const Point2 middle = viewport.rect.center();
    Box2 rect(Point2(middle.x - width / 2.0, middle.y - height / 2.0),
              Point2(middle.x + width / 2.0, middle.y + height / 2.0));
    if (!drawingArea.empty()) {
        rect = clampInto(rect, drawingArea);
    }
    viewport.rect = rect;
    viewport.centre = fit->centre;
    viewport.autoScale = false;
    viewport.autoCentre = false;
    return {};
}

Result<RotationFit> rotateToBestFit(Viewport& viewport, std::span<const Point2> content)
{
    if (!isPlan(viewport.kind)) {
        return makeError(ErrorCode::InvalidArgument,
                         std::format("only a plan or key plan is turned to fit; this is a {} view",
                                     kindWords(viewport.kind)),
                         viewport.id);
    }
    if (viewport.rect.empty()) {
        return makeError(ErrorCode::InvalidArgument, "the view is not placed on the paper",
                         viewport.id);
    }
    auto fit = bestStandardRotation(content, {viewport.rect.width() / kAutoScaleSpare,
                                              viewport.rect.height() / kAutoScaleSpare});
    if (!fit) {
        return fit.error();
    }
    viewport.rotation = fit->rotation;
    viewport.centre = fit->centre;
    viewport.scale = fit->standardScale;
    viewport.autoScale = false;
    viewport.autoCentre = false;
    return fit;
}

Result<std::vector<Sheet>> smartLayoutRotated(const entity::Model& model,
                                              const LayoutRequest& request,
                                              std::span<const Point2> content)
{
    if (!request.planArea) {
        return makeError(ErrorCode::InvalidArgument,
                         "turning the drawing to fill the sheet needs a plan of an area");
    }
    if (request.planAlongAlignment || request.longSection || request.crossSectionInterval > 0.0) {
        return makeError(ErrorCode::InvalidArgument,
                         "only a plan of an area is turned to fill the sheet; a strip already "
                         "follows its alignment");
    }
    auto sheets = smartLayout(model, request);
    if (!sheets) {
        return sheets.error();
    }
    const auto areaPlan = [](Sheet& sheet) -> Viewport* {
        for (Viewport& viewport : sheet.viewports) {
            if (viewport.kind == ViewportKind::Plan && viewport.source.alignment.empty()) {
                return &viewport;
            }
        }
        return nullptr;
    };
    if (!hullOf(content)) {
        return sheets;
    }
    if (!(request.scale > 0.0)) {
        // Automatic: turned when that buys a larger standard scale.
        Viewport* plan = sheets->empty() ? nullptr : areaPlan(sheets->front());
        if (plan == nullptr || plan->rect.empty()) {
            return sheets;
        }
        auto fit = bestStandardRotation(content, {plan->rect.width(), plan->rect.height()});
        if (fit && fit->rotation != 0.0 && fit->standardScale < plan->scale) {
            plan->rotation = fit->rotation;
            plan->centre = fit->centre;
            plan->scale = fit->standardScale;
        }
        return sheets;
    }
    if (sheets->size() <= 1) {
        return sheets; // it fits square at the scale asked
    }
    // At a fixed scale: on one sheet, turned, when it fits there.
    LayoutRequest automatic = request;
    automatic.scale = 0.0;
    auto compact = smartLayout(model, automatic);
    if (!compact || compact->size() != 1) {
        return sheets;
    }
    Viewport* plan = areaPlan(compact->front());
    if (plan == nullptr || plan->rect.empty()) {
        return sheets;
    }
    auto fit = leastRotationToFit(content, {plan->rect.width(), plan->rect.height()}, request.scale);
    if (!fit) {
        return sheets;
    }
    plan->rotation = fit->rotation;
    plan->centre = fit->centre;
    plan->scale = request.scale;
    return compact;
}

} // namespace katana::cad::plotting
