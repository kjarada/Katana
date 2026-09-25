#include "katana/entity/anchor.hpp"

#include <string>

namespace katana::entity {

using katana::geometry::Arc2;
using katana::geometry::Circle2;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;

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
            default:
                return std::nullopt;
            }
        }
        std::optional<Point2> operator()(const Circle2& circle) const
        {
            return ref.point == AnchorPoint::Centre ? std::optional(circle.center) : std::nullopt;
        }
        std::optional<Point2> operator()(const Polyline2& polyline) const
        {
            const auto& v = polyline.vertices;
            if (v.empty()) {
                return std::nullopt;
            }
            switch (ref.point) {
            case AnchorPoint::Start:
                return v.front();
            case AnchorPoint::End:
                return v.back();
            case AnchorPoint::Vertex:
                return ref.index < v.size() ? std::optional(v[ref.index]) : std::nullopt;
            case AnchorPoint::SegmentMid: {
                const std::size_t segments = polyline.closed ? v.size() : v.size() - 1;
                if (ref.index >= segments) {
                    return std::nullopt;
                }
                const Point2& a = v[ref.index];
                const Point2& b = v[(ref.index + 1) % v.size()];
                return a + (b - a) * 0.5;
            }
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
    };
    return std::visit(Visitor{ref}, entity.geometry);
}

std::string describe(const AnchorRef& ref)
{
    std::string text(toString(ref.point));
    if (ref.point == AnchorPoint::Vertex || ref.point == AnchorPoint::SegmentMid) {
        text += " " + std::to_string(ref.index);
    }
    return text;
}

} // namespace katana::entity
