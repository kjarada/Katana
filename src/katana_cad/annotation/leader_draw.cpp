#include "katana/cad/annotation/leader_draw.hpp"

#include <algorithm>
#include <cmath>

#include "katana/cad/annotation/text_layout.hpp"
#include "katana/entity/annotation.hpp"
#include "katana/math/numerics.hpp"

namespace katana::cad::annotation {

using katana::entity::ArrowHead;
using katana::entity::CalloutShape;
using katana::entity::TextJustify;
namespace tol = katana::math::tolerance;

namespace {

// A closed head is three times as long as it is wide - dimension_draw.cpp's
// proportion (kArrowWidthFraction there), so a leader's arrow and a
// dimension's match.
constexpr double kArrowWidthFraction = 1.0 / 3.0;
// A callout's margin around its note, as a fraction of the text height.
constexpr double kCalloutMargin = 1.0 / 3.0;
// The gap between the landing's end and the note, as a fraction of the text
// height: AutoCAD's MLEADER default landing gap is 2 units against 4 of text.
constexpr double kLandingGap = 0.5;

} // namespace

void appendArrowhead(Drawing& drawing, ArrowHead head, const Point2& tip, const Vec2& along,
                     double size)
{
    if (!(size > tol::kGeometric)) {
        return;
    }
    const Vec2 normal = along.perpendicular();
    const Point2 back = tip - along * size;
    const Vec2 half = normal * (size * kArrowWidthFraction);
    switch (head) {
    case ArrowHead::None:
        break;
    case ArrowHead::Tick: {
        const Vec2 direction = (along + normal).normalized();
        drawing.strokes.push_back({tip - direction * (size * 0.5), tip + direction * (size * 0.5)});
        break;
    }
    case ArrowHead::Open:
        drawing.strokes.push_back({back + half, tip, back - half});
        break;
    case ArrowHead::ClosedFilled:
        drawing.fills.push_back({tip, back + half, back - half});
        break;
    case ArrowHead::Dot:
        drawing.fills.push_back(circlePolygon(tip, size * 0.25, 16));
        break;
    }
}

Drawing buildLeader(const katana::entity::Model& model, const katana::entity::LeaderGeometry& leader,
                    double scale, const TextMeasure& measure)
{
    Drawing drawing;
    const auto& v = leader.vertices;
    if (v.size() < 2) {
        return drawing;
    }
    const std::string styleName = leader.style.empty()
                                      ? std::string(katana::entity::kDefaultTextStyleName)
                                      : leader.style;
    TextAppearance appearance =
        resolveTextAppearance(model, styleName, leader.paperHeight,
                              katana::entity::annotationModelSize(2.5, scale), scale);
    appearance.readable = false;
    drawing.colour = appearance.colour;
    const double height = appearance.height;

    // The line, from the arrow's back when the head is closed so the line's
    // end does not show through its point.
    std::vector<Point2> line(v.begin(), v.end());
    const Vec2 first = v[1] - v[0];
    const double firstLength = first.length();
    const double arrow = katana::entity::annotationModelSize(leader.arrowSize, scale);
    if (firstLength > tol::kGeometric) {
        const Vec2 along = -first / firstLength;
        appendArrowhead(drawing, leader.arrow, v[0], along, std::min(arrow, 0.5 * firstLength));
    }

    // The landing's direction: away from the line's last stretch.
    const Vec2 last = v.back() - v[v.size() - 2];
    const double sign = last.x < -tol::kGeometric * std::max(1.0, last.length()) ? -1.0 : 1.0;
    const Vec2 landingDirection(sign, 0.0);
    const double landing = katana::entity::annotationModelSize(leader.landing, scale);
    Point2 end = v.back();
    if (landing > 0.0) {
        end = end + landingDirection * landing;
        line.push_back(end);
    }

    if (!leader.text.empty()) {
        const double gap = kLandingGap * height;
        const double margin = kCalloutMargin * height;
        if (leader.callout == CalloutShape::Circle) {
            // Measure first, then centre the note in the circle that clears it.
            Drawing probe = layoutText(leader.text, Point2(0.0, 0.0), 0.0,
                                       TextJustify::MiddleCentre, appearance, measure);
            double radius = 0.0;
            for (const auto& box : probe.textBoxes) {
                for (const Point2& corner : box) {
                    radius = std::max(radius, Vec2(corner.x, corner.y).length());
                }
            }
            radius += margin;
            const Point2 centre = end + landingDirection * radius;
            Drawing note = layoutText(leader.text, centre, 0.0, TextJustify::MiddleCentre,
                                      appearance, measure);
            note.masks.push_back(circlePolygon(centre, radius));
            note.outlines.push_back(circlePolygon(centre, radius));
            drawing.append(note);
        } else {
            const Point2 at = end + landingDirection * (leader.callout == CalloutShape::Box
                                                            ? gap + margin
                                                            : gap);
            const TextJustify justify =
                sign > 0.0 ? TextJustify::MiddleLeft : TextJustify::MiddleRight;
            Drawing note = layoutText(leader.text, at, 0.0, justify, appearance, measure);
            if (leader.callout == CalloutShape::Box && !note.textBoxes.empty()) {
                const Box2 box = [&] {
                    Box2 b;
                    for (const Point2& p : note.textBoxes.front()) {
                        b.expand(p);
                    }
                    return b.inflated(margin);
                }();
                const std::vector<Point2> frame = {box.min, Point2(box.max.x, box.min.y), box.max,
                                                   Point2(box.min.x, box.max.y)};
                note.masks.push_back(frame);
                note.outlines.push_back(frame);
            }
            drawing.append(note);
        }
    }
    drawing.strokes.insert(drawing.strokes.begin(), std::move(line));
    drawing.updateExtent();
    return drawing;
}

} // namespace katana::cad::annotation
