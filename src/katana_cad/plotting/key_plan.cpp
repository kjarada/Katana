#include "katana/cad/plotting/key_plan.hpp"

#include <algorithm>
#include <cmath>

#include "katana/cad/plot.hpp"
#include "katana/cad/selection.hpp"
#include "katana/entity/model.hpp"
#include "katana/geometry/alignment.hpp"

namespace katana::cad::plotting {

namespace {

Point2 turned(const Point2& v, double radians)
{
    const double c = std::cos(radians);
    const double s = std::sin(radians);
    return Point2(c * v.x - s * v.y, s * v.x + c * v.y);
}

// Twice the signed area of a ring: positive counter-clockwise.
double doubleArea(const std::vector<Point2>& ring)
{
    double sum = 0.0;
    for (std::size_t i = 0; i < ring.size(); ++i) {
        const Point2& a = ring[i];
        const Point2& b = ring[(i + 1) % ring.size()];
        sum += a.x * b.y - b.x * a.y;
    }
    return sum;
}

// Whether `p` is inside the convex ring `ring` (a footprint, counter-clockwise).
bool insideConvex(const std::vector<Point2>& ring, const Point2& p)
{
    if (ring.size() < 3) {
        return false;
    }
    const double sense = doubleArea(ring) < 0.0 ? -1.0 : 1.0;
    for (std::size_t i = 0; i < ring.size(); ++i) {
        const Point2& a = ring[i];
        const Point2& b = ring[(i + 1) % ring.size()];
        if (sense * ((b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x)) < 0.0) {
            return false;
        }
    }
    return true;
}

Point2 centroidOf(const std::vector<Point2>& ring)
{
    Point2 sum;
    for (const Point2& p : ring) {
        sum = sum + p;
    }
    return sum * (1.0 / static_cast<double>(ring.size()));
}

// What a plan of `model` draws with `layers` hidden, as the painter bounds it:
// the entities (drawnExtent) and every alignment, which is drawn but is no
// entity, to within a metre of its curves.
Box2 drawingExtent(const entity::Model& model, const LayerOverrides& layers)
{
    Box2 box = drawnExtent(model, layers);
    for (const entity::Alignment& alignment : model.alignments.all()) {
        if (const auto solved = geometry::solveAlignment(alignment.horizontal)) {
            for (const Point2& vertex : solved->toPolyline(1.0).vertices) {
                box.expand(vertex);
            }
        }
    }
    return box;
}

} // namespace

std::string printedSheetNumber(const SheetSet& set, std::size_t index)
{
    if (index >= set.sheets.size()) {
        return {};
    }
    const Sheet& sheet = set.sheets[index];
    if (const auto typed = sheet.fields.find("sheet_number"); typed != sheet.fields.end()) {
        return typed->second;
    }
    return formatSheetNumber(set.numbering, index + 1, set.sheets.size(), set.defaults.setNumber);
}

std::vector<KeyPlanOutline> keyPlanOutlines(const SheetSet& set, std::size_t sheetIndex,
                                            const PlanPlacer& place)
{
    std::vector<KeyPlanOutline> outlines;
    for (std::size_t i = 0; i < set.sheets.size(); ++i) {
        const Sheet& sheet = set.sheets[i];
        const std::string number = printedSheetNumber(set, i);
        const std::size_t first = outlines.size();
        for (const Viewport& viewport : sheet.viewports) {
            if (viewport.kind != ViewportKind::Plan || viewport.rect.empty()) {
                continue;
            }
            const bool automatic = viewport.autoScale || viewport.autoCentre;
            const PlanPlacement at =
                automatic && place ? place(viewport) : storedPlacement(viewport);
            std::vector<Point2> corners = planFootprint(viewport, at);
            if (corners.empty() ||
                !std::ranges::all_of(corners, [](const Point2& p) {
                    return std::isfinite(p.x) && std::isfinite(p.y);
                })) {
                continue;
            }
            outlines.push_back({sheet.id, i, viewport.id, number, std::move(corners),
                                i == sheetIndex, true});
        }
        // A plan inside a larger plan of its own sheet - a detail at a larger
        // scale, an inset - takes the number from it rather than repeating it.
        for (std::size_t a = first; a < outlines.size(); ++a) {
            const Point2 middle = centroidOf(outlines[a].corners);
            const double area = std::abs(doubleArea(outlines[a].corners));
            for (std::size_t b = first; b < outlines.size(); ++b) {
                if (b != a && std::abs(doubleArea(outlines[b].corners)) > area &&
                    insideConvex(outlines[b].corners, middle)) {
                    outlines[a].labelled = false;
                    break;
                }
            }
        }
    }
    return outlines;
}

PlanPlacement fitPlanPlacement(const Viewport& viewport, std::span<const Point2> points)
{
    PlanPlacement at = storedPlacement(viewport);
    if (points.empty() || viewport.rect.empty()) {
        return at;
    }
    if (viewport.autoCentre) {
        Box2 box;
        for (const Point2& p : points) {
            box.expand(turned(p, -viewport.rotation));
        }
        at.centre = turned(box.center(), viewport.rotation);
    }
    if (viewport.autoScale) {
        double halfW = 0.0;
        double halfH = 0.0;
        for (const Point2& p : points) {
            const Point2 d = turned(p - at.centre, -viewport.rotation);
            halfW = std::max(halfW, std::abs(d.x));
            halfH = std::max(halfH, std::abs(d.y));
        }
        // 4% to spare, so nothing touches the viewport's edge.
        const double needed = std::max(2.0 * halfW * 1000.0 / viewport.rect.width(),
                                       2.0 * halfH * 1000.0 / viewport.rect.height()) *
                              1.04;
        if (needed > 0.0) {
            if (auto scale = sheetScaleAtLeast(needed)) {
                at.scale = *scale;
            }
        }
    }
    return at;
}

PlanPlacement fitKeyPlan(const Viewport& keyPlan, std::span<const KeyPlanOutline> outlines,
                         const Box2& drawing)
{
    std::vector<Point2> points;
    for (const KeyPlanOutline& outline : outlines) {
        points.insert(points.end(), outline.corners.begin(), outline.corners.end());
    }
    if (!drawing.empty()) {
        points.insert(points.end(), {drawing.min, Point2(drawing.max.x, drawing.min.y), drawing.max,
                                     Point2(drawing.min.x, drawing.max.y)});
    }
    return fitPlanPlacement(keyPlan, points);
}

PlanPlacement placePlan(const entity::Model& model, const Viewport& viewport)
{
    if ((!viewport.autoScale && !viewport.autoCentre) || viewport.rect.empty()) {
        return storedPlacement(viewport);
    }
    // What the viewport shows: its stretch of an alignment, sampled as the
    // painter samples it, or the drawing.
    std::vector<Point2> points;
    const ViewportSource& from = viewport.source;
    if (!from.alignment.empty()) {
        if (const auto* alignment = model.alignments.find(from.alignment)) {
            if (auto solved = geometry::solveAlignment(alignment->horizontal)) {
                double a = from.chainageFrom;
                double b = from.chainageTo;
                if (!(b > a)) {
                    a = solved->startStation();
                    b = solved->endStation();
                }
                for (int i = 0; i <= 64; ++i) {
                    if (auto p = solved->pointAtStation(a + (b - a) * i / 64.0)) {
                        points.push_back(*p);
                    }
                }
            }
        }
    }
    if (points.empty()) {
        const Box2 box = drawingExtent(model, viewport.hiddenLayers);
        if (box.empty()) {
            return storedPlacement(viewport);
        }
        points = {box.min, box.max, Point2(box.min.x, box.max.y), Point2(box.max.x, box.min.y)};
    }
    return fitPlanPlacement(viewport, points);
}

PlanPlacement placePlan(const entity::Model& model, const SheetSet& set, std::size_t sheetIndex,
                        const Viewport& viewport)
{
    if (viewport.kind != ViewportKind::KeyPlan || (!viewport.autoScale && !viewport.autoCentre) ||
        viewport.rect.empty()) {
        return placePlan(model, viewport);
    }
    const auto outlines = keyPlanOutlines(set, sheetIndex, modelPlacer(model));
    return fitKeyPlan(viewport, outlines, drawingExtent(model, viewport.hiddenLayers));
}

PlanPlacer modelPlacer(const entity::Model& model)
{
    return [&model](const Viewport& viewport) { return placePlan(model, viewport); };
}

} // namespace katana::cad::plotting
