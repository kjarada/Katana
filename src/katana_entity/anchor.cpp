#include "katana/entity/anchor.hpp"

#include <algorithm>
#include <cmath>
#include <limits>
#include <string>
#include <vector>

#include "katana/core/text.hpp"
#include "katana/math/numerics.hpp"

namespace katana::entity {

using katana::geometry::Arc2;
using katana::geometry::Circle2;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;
using katana::geometry::Vec2;

namespace {

double clampedParameter(const AnchorRef& ref)
{
    return std::isfinite(ref.parameter) ? std::clamp(ref.parameter, 0.0, 1.0) : 0.0;
}

} // namespace

Point2 insidePoint(const Polyline2& figure)
{
    const std::optional<Point2> centroid = figure.centroid();
    if (centroid && figure.contains(*centroid)) {
        return *centroid;
    }
    const Point2 through = centroid.value_or(figure.vertices.front());
    std::vector<double> crossings;
    const std::size_t n = figure.vertices.size();
    for (std::size_t i = 0; i < n; ++i) {
        const Point2& a = figure.vertices[i];
        const Point2& b = figure.vertices[(i + 1) % n];
        if ((a.y > through.y) != (b.y > through.y)) {
            const double t = (through.y - a.y) / (b.y - a.y);
            crossings.push_back(a.x + t * (b.x - a.x));
        }
    }
    std::sort(crossings.begin(), crossings.end());
    double bestWidth = -1.0;
    Point2 best = through;
    for (std::size_t i = 0; i + 1 < crossings.size(); i += 2) {
        const double width = crossings[i + 1] - crossings[i];
        if (width > bestWidth) {
            bestWidth = width;
            best = Point2(0.5 * (crossings[i] + crossings[i + 1]), through.y);
        }
    }
    return best;
}

std::optional<Point2> resolveAnchor(const Entity& entity, const AnchorRef& ref)
{
    struct Visitor {
        const AnchorRef& ref;
        std::optional<Point2> operator()(const PointGeometry& point) const
        {
            return ref.point == AnchorPoint::Position ? std::optional(point.position) : std::nullopt;
        }
        std::optional<Point2> operator()(const Segment2& line) const
        {
            switch (ref.point) {
            case AnchorPoint::Start:
                return line.start;
            case AnchorPoint::End:
                return line.end;
            case AnchorPoint::Mid:
                return line.start + (line.end - line.start) * 0.5;
            case AnchorPoint::Along:
                return line.pointAt(clampedParameter(ref));
            default:
                return std::nullopt;
            }
        }
        std::optional<Point2> operator()(const Arc2& arc) const
        {
            switch (ref.point) {
            case AnchorPoint::Start:
                return arc.pointAt(0.0);
            case AnchorPoint::End:
                return arc.pointAt(1.0);
            case AnchorPoint::Mid:
                return arc.pointAt(0.5);
            case AnchorPoint::Centre:
                return arc.center;
            case AnchorPoint::Along:
                return arc.pointAt(clampedParameter(ref));
            default:
                return std::nullopt;
            }
        }
        std::optional<Point2> operator()(const Circle2& circle) const
        {
            switch (ref.point) {
            case AnchorPoint::Centre:
            case AnchorPoint::Inside:
                return circle.center;
            case AnchorPoint::Along:
                return circle.pointAtAngle(clampedParameter(ref) * katana::math::kTwoPi);
            default:
                return std::nullopt;
            }
        }
        std::optional<Point2> operator()(const Polyline2& polyline) const
        {
            const auto& v = polyline.vertices;
            if (v.empty()) {
                return std::nullopt;
            }
            const std::size_t segments = polyline.segmentCount();
            switch (ref.point) {
            case AnchorPoint::Start:
                return v.front();
            case AnchorPoint::End:
                return v.back();
            case AnchorPoint::Vertex:
                return ref.index < v.size() ? std::optional(v[ref.index]) : std::nullopt;
            case AnchorPoint::SegmentMid:
            case AnchorPoint::Along: {
                if (ref.index >= segments) {
                    return std::nullopt;
                }
                const double t = ref.point == AnchorPoint::SegmentMid ? 0.5 : clampedParameter(ref);
                return polyline.segment(ref.index).pointAt(t);
            }
            case AnchorPoint::Inside:
                if (!polyline.closed || v.size() < 3) {
                    return std::nullopt;
                }
                return insidePoint(polyline);
            default:
                return std::nullopt;
            }
        }
        std::optional<Point2> operator()(const TextGeometry& text) const
        {
            return ref.point == AnchorPoint::Position ? std::optional(text.position) : std::nullopt;
        }
        std::optional<Point2> operator()(const DimensionGeometry&) const { return std::nullopt; }
        std::optional<Point2> operator()(const LabelGeometry&) const { return std::nullopt; }
        std::optional<Point2> operator()(const LeaderGeometry& leader) const
        {
            switch (ref.point) {
            case AnchorPoint::Start:
                return leader.vertices.front();
            case AnchorPoint::End:
                return leader.vertices.back();
            default:
                return std::nullopt;
            }
        }
        // The drawing system's kinds (docs/drawing.md): a curve polyline as
        // a polyline, its segment middles on the arcs; an ellipse's and a
        // spline's ends and an ellipse's centre.
        std::optional<Point2> operator()(const katana::geometry::CurvePolyline2& polyline) const
        {
            const auto& v = polyline.vertices;
            if (v.empty()) {
                return std::nullopt;
            }
            switch (ref.point) {
            case AnchorPoint::Start:
                return v.front().position;
            case AnchorPoint::End:
                return v.back().position;
            case AnchorPoint::Vertex:
                return ref.index < v.size() ? std::optional(v[ref.index].position) : std::nullopt;
            case AnchorPoint::SegmentMid:
                if (ref.index >= polyline.segmentCount()) {
                    return std::nullopt;
                }
                return std::visit([](const auto& piece) { return piece.pointAt(0.5); },
                                  polyline.segment(ref.index));
            default:
                return std::nullopt;
            }
        }
        std::optional<Point2> operator()(const katana::geometry::Ellipse2& ellipse) const
        {
            switch (ref.point) {
            case AnchorPoint::Centre:
                return ellipse.center;
            case AnchorPoint::Start:
                return ellipse.startPoint();
            case AnchorPoint::End:
                return ellipse.endPoint();
            default:
                return std::nullopt;
            }
        }
        std::optional<Point2> operator()(const katana::geometry::Spline2& spline) const
        {
            if (!spline.checkStructure()) {
                return std::nullopt;
            }
            switch (ref.point) {
            case AnchorPoint::Start:
                return spline.startPoint();
            case AnchorPoint::End:
                return spline.endPoint();
            default:
                return std::nullopt;
            }
        }
    };
    return std::visit(Visitor{ref}, entity.geometry);
}

std::optional<AnchorRef> nearestAnchor(const Entity& entity, const Point2& near)
{
    AnchorRef ref;
    ref.entity = entity.id;
    ref.point = AnchorPoint::Along;
    if (std::holds_alternative<PointGeometry>(entity.geometry) ||
        std::holds_alternative<TextGeometry>(entity.geometry)) {
        ref.point = AnchorPoint::Position;
        return ref;
    }
    if (const auto* line = std::get_if<Segment2>(&entity.geometry)) {
        ref.parameter = line->parameterOf(near);
        return ref;
    }
    if (const auto* arc = std::get_if<Arc2>(&entity.geometry)) {
        const Vec2 radial = near - arc->center;
        ref.parameter = arc->parameterOfAngle(std::atan2(radial.y, radial.x));
        return ref;
    }
    if (const auto* circle = std::get_if<Circle2>(&entity.geometry)) {
        const Vec2 radial = near - circle->center;
        // A turn is [0, 1): the angle 2 pi is the angle 0, and a fraction of
        // exactly 1 would describe the same point twice.
        const double turn =
            katana::math::normalizeAngle(std::atan2(radial.y, radial.x)) / katana::math::kTwoPi;
        ref.parameter = turn < 1.0 ? turn : 0.0;
        return ref;
    }
    if (const auto* polyline = std::get_if<Polyline2>(&entity.geometry)) {
        double best = std::numeric_limits<double>::infinity();
        bool found = false;
        for (std::size_t i = 0; i < polyline->segmentCount(); ++i) {
            const Segment2 segment = polyline->segment(i);
            if (segment.isDegenerate()) {
                continue; // a repeated vertex has no "along"
            }
            const double distance = segment.distanceTo(near);
            if (distance < best) {
                best = distance;
                found = true;
                ref.index = static_cast<std::uint32_t>(i);
                ref.parameter = segment.parameterOf(near);
            }
        }
        return found ? std::optional(ref) : std::nullopt;
    }
    return std::nullopt;
}

std::string describe(const AnchorRef& ref)
{
    std::string text(toString(ref.point));
    if (ref.point == AnchorPoint::Vertex || ref.point == AnchorPoint::SegmentMid) {
        text += " " + std::to_string(ref.index);
    } else if (ref.point == AnchorPoint::Along) {
        // Exact, so what LIST prints reads back as the same place.
        text +=
            " " + std::to_string(ref.index) + " " + katana::core::formatExactReal(ref.parameter);
    }
    return text;
}

} // namespace katana::entity
