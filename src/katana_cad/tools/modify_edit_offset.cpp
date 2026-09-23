// Offset: a distance (typed, measured between two points, or Through a point),
// then an object and the side to put the copy on - as many as wanted, Enter to
// finish. [Multiple] keeps offsetting the copy just made, which is how a set
// of parallel lines is drawn in one go.

#include <algorithm>
#include <cmath>
#include <limits>

#include "katana/entity/entity_geometry.hpp"
#include "modify_edit_support.hpp"

namespace katana::cad::tools::modify_edit {

namespace {

namespace tol = katana::math::tolerance;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::geometry::Line2;

// The segment of `polyline` nearest to `p` that has a direction, or nullopt.
std::optional<Segment2> nearestSegment(const Polyline2& polyline, const Point2& p)
{
    std::optional<Segment2> best;
    double bestDistance = std::numeric_limits<double>::infinity();
    for (std::size_t i = 0; i < polyline.segmentCount(); ++i) {
        const Segment2 segment = polyline.segment(i);
        const double d = segment.distanceTo(p);
        if (!segment.isDegenerate() && d < bestDistance) {
            bestDistance = d;
            best = segment;
        }
    }
    return best;
}

// How far `side` is from `source` the way an offset measures it - from the
// line a segment lies on, from an arc's or circle's circumference - so that a
// Through offset passes exactly through the point.
std::optional<double> throughDistance(const Geometry& source, const Point2& side)
{
    if (const auto* segment = std::get_if<Segment2>(&source)) {
        return Line2{segment->start, segment->delta()}.distanceTo(side);
    }
    if (const auto* arc = std::get_if<Arc2>(&source)) {
        return std::abs(arc->center.distanceTo(side) - arc->radius);
    }
    if (const auto* circle = std::get_if<Circle2>(&source)) {
        return std::abs(circle->center.distanceTo(side) - circle->radius);
    }
    if (const auto* polyline = std::get_if<Polyline2>(&source)) {
        if (const auto segment = nearestSegment(*polyline, side)) {
            return Line2{segment->start, segment->delta()}.distanceTo(side);
        }
    }
    return std::nullopt;
}

// True when every side of `moved` still runs the way its side of `source`
// does. geometry::offset mitres each vertex and is only right while the
// distance is below the polyline's local feature size (editing.hpp): beyond
// it the moved sides pass each other, and a mitre lands past the far end of a
// short side, which then runs backwards - an inward offset of an 8x8 square by
// 5 comes out as a 2x2 square drawn the other way round, when no such offset
// exists. Each side of the result lies on its source side's line moved
// sideways, so it can only run the same way, shrink to nothing or run back.
// geometry::offset works from the polyline without repeated vertices, so its
// sides are compared with those.
bool keepsItsSides(const Polyline2& source, const Polyline2& moved)
{
    const Polyline2 clean = source.withoutDuplicateVertices();
    if (clean.segmentCount() != moved.segmentCount()) {
        return false;
    }
    for (std::size_t i = 0; i < clean.segmentCount(); ++i) {
        const Segment2 side = moved.segment(i);
        if (side.isDegenerate() || side.delta().dot(clean.segment(i).delta()) <= 0.0) {
            return false;
        }
    }
    return true;
}

// The offset of `source` by `distance` towards `side`. A polyline takes the
// side of its segment nearest the point, as offsetEntity decides it.
Result<Geometry> offsetGeometry(const Geometry& source, double distance, const Point2& side)
{
    if (const auto* polyline = std::get_if<Polyline2>(&source)) {
        const auto segment = nearestSegment(*polyline, side);
        if (!segment) {
            return makeError(ErrorCode::InvalidGeometry, "That polyline has no length to offset.");
        }
        const double sign =
            Line2{segment->start, segment->delta()}.signedDistanceTo(side) >= 0.0 ? 1.0 : -1.0;
        auto moved = katana::geometry::offset(*polyline, sign * distance);
        if (!moved) {
            return makeError(ErrorCode::InvalidGeometry, asSentence(moved.error().message));
        }
        if (!keepsItsSides(*polyline, *moved)) {
            return makeError(ErrorCode::InvalidGeometry,
                             "An offset of " + formatNumber(distance) + " is too large for that " +
                                 kindName(source) +
                                 ": a side of the copy would turn back on itself or shrink to "
                                 "nothing.");
        }
        return Geometry{std::move(*moved)};
    }
    const auto curve = asCurve(source);
    if (!curve) {
        return makeError(ErrorCode::Unsupported, "Only lines, arcs, circles and polylines offset.");
    }
    auto moved = katana::geometry::offsetTowards(*curve, distance, side);
    if (!moved) {
        if (std::holds_alternative<Circle2>(*curve) || std::holds_alternative<Arc2>(*curve)) {
            return makeError(ErrorCode::InvalidGeometry,
                             "The distance is not smaller than the radius, so there is no " +
                                 kindName(source) + " on that side.");
        }
        return makeError(ErrorCode::InvalidGeometry, asSentence(moved.error().message));
    }
    return asGeometry(*moved);
}

// An offset copies its source, as offsetEntity and AutoCAD's default
// (Layer = Source) make it: the same layer, style, colour and properties. Its
// vertices are the source's moved sideways, so each keeps its height - unless
// the offset dropped a repeated vertex, when only a level string's single
// height still describes it.
Entity offsetCopy(const Entity& source, Geometry geometry)
{
    Entity copy = source;
    copy.geometry = std::move(geometry);
    const std::size_t before = heightVertices(source.geometry).size();
    const std::size_t after = heightVertices(copy.geometry).size();
    if (before != after) {
        const auto heights = katana::entity::heightsOf(source.properties, before);
        const bool level = !heights.empty() && std::ranges::all_of(heights, [&](const auto& h) {
            return h == heights.front();
        });
        katana::entity::setHeights(copy.properties,
                                   std::vector<std::optional<double>>(
                                       after, level ? heights.front() : std::nullopt));
    }
    return copy;
}

class OffsetTool final : public InteractiveTool {
  public:
    OffsetTool(const ToolContext& context, Defaults defaults)
        : context_(context), defaults_(std::move(defaults)), session_(context.document)
    {
    }

    [[nodiscard]] std::string prompt() const override
    {
        switch (step_) {
        case Step::Distance:
            return "Specify offset distance or [Through] <" +
                   (defaults_->offsetDistance ? formatNumber(*defaults_->offsetDistance)
                                              : std::string("Through")) +
                   ">";
        case Step::SecondPoint:
            return "Specify second point";
        case Step::Source:
            return "Select object to offset or [Exit/Undo]";
        case Step::Side:
            if (multiple_) {
                return through_ ? "Specify through point for the next offset or [Undo]"
                                : "Specify point on side for the next offset or [Undo]";
            }
            return through_ ? "Specify through point or [Multiple/Undo]"
                            : "Specify point on side to offset or [Multiple/Undo]";
        }
        return {};
    }

    [[nodiscard]] ToolInput expects() const override
    {
        return step_ == Step::Source ? ToolInput::Entity : ToolInput::Point;
    }

    ToolStep point(const Point2& at) override
    {
        switch (step_) {
        case Step::Distance:
            // A distance measured between two points, as AutoCAD allows.
            first_ = at;
            step_ = Step::SecondPoint;
            return ToolStep::next();
        case Step::SecondPoint: {
            const double distance = first_->distanceTo(at);
            if (!(distance > tol::kGeometric)) {
                return ToolStep::rejected(
                    "The two points are the same; the offset distance must be greater than "
                    "zero.");
            }
            return chooseDistance(distance);
        }
        case Step::Side:
            return offsetTowards(at);
        case Step::Source:
            break;
        }
        return InteractiveTool::point(at);
    }

    ToolStep entity(EntityId id, const Point2& at) override
    {
        if (step_ != Step::Source) {
            return InteractiveTool::entity(id, at);
        }
        if (auto refusal = refusalToEdit(session_.document(), id)) {
            // Offsetting only reads its source, but the copy lands on the
            // source's layer, which is locked.
            return ToolStep::rejected(*refusal);
        }
        const Entity* source = session_.original(id);
        const auto type = source->type();
        using katana::entity::EntityType;
        if (type != EntityType::Line && type != EntityType::Arc && type != EntityType::Circle &&
            type != EntityType::Polyline) {
            return ToolStep::rejected("A " + kindName(source->geometry) +
                                      " cannot be offset; pick a line, arc, circle or polyline.");
        }
        chain_ = {*source};
        step_ = Step::Side;
        return ToolStep::next();
    }

    ToolStep value(std::string_view text) override
    {
        if (isOption(text, "Undo", "U")) {
            return undo();
        }
        if (step_ == Step::Distance) {
            if (isOption(text, "Through", "T")) {
                through_ = true;
                defaults_->offsetDistance.reset();
                step_ = Step::Source;
                return ToolStep::next();
            }
            const auto distance = parseNumber(text);
            if (!distance) {
                return ToolStep::rejected("Type the offset distance, or T to offset through a "
                                          "point.");
            }
            if (!(*distance > tol::kGeometric)) {
                return ToolStep::rejected("The offset distance must be greater than zero.");
            }
            return chooseDistance(*distance);
        }
        if (step_ == Step::Side && !multiple_ && isOption(text, "Multiple", "M")) {
            multiple_ = true;
            return ToolStep::next("Multiple: each point offsets the last offset again.");
        }
        if (step_ == Step::Source && isOption(text, "Exit", "E")) {
            return enter();
        }
        return ToolStep::rejected("'" + std::string(text) + "' is not an option here.");
    }

    ToolStep enter() override
    {
        if (step_ == Step::Distance) {
            // Enter takes the default: the last distance, or Through.
            if (defaults_->offsetDistance) {
                return chooseDistance(*defaults_->offsetDistance);
            }
            through_ = true;
            step_ = Step::Source;
            return ToolStep::next();
        }
        if (step_ == Step::SecondPoint) {
            return ToolStep::rejected("Specify the second point of the offset distance.");
        }
        const std::size_t count = session_.added().size();
        if (count == 0) {
            return ToolStep::done(nullptr);
        }
        return ToolStep::done(session_.commit("OFFSET"),
                              "Made " + std::to_string(count) +
                                  (count == 1 ? " offset." : " offsets."));
    }

    ToolStep undo() override
    {
        switch (step_) {
        case Step::Distance:
            return ToolStep::rejected("Nothing to undo yet.");
        case Step::SecondPoint:
            first_.reset();
            step_ = Step::Distance;
            return ToolStep::next();
        case Step::Source:
            if (session_.undo()) {
                return ToolStep::next("Undid the last offset.");
            }
            step_ = Step::Distance;
            through_ = false;
            return ToolStep::next();
        case Step::Side:
            if (chain_.size() > 1) {
                // In Multiple: take back the last copy; the one before it is
                // the source again.
                (void)session_.undo();
                chain_.pop_back();
                return ToolStep::next("Undid the last offset.");
            }
            chain_.clear();
            multiple_ = false;
            step_ = Step::Source;
            return ToolStep::next();
        }
        return ToolStep::rejected("Nothing to undo.");
    }

    [[nodiscard]] ToolFeedback preview(const Point2& cursor) const override
    {
        ToolFeedback feedback;
        feedback.shapes = session_.shapes();
        if (step_ == Step::SecondPoint) {
            feedback.shapes.emplace_back(Segment2{*first_, cursor});
            feedback.markers.push_back(*first_);
        }
        if (step_ == Step::Side) {
            if (auto made = attempt(cursor)) {
                feedback.shapes.push_back(made->geometry);
            }
        }
        return feedback;
    }

    [[nodiscard]] std::optional<Point2> lastPoint() const override
    {
        return step_ == Step::SecondPoint ? first_ : std::nullopt;
    }

  private:
    enum class Step { Distance, SecondPoint, Source, Side };

    ToolStep chooseDistance(double distance)
    {
        distance_ = distance;
        through_ = false;
        defaults_->offsetDistance = distance;
        first_.reset();
        step_ = Step::Source;
        return ToolStep::next();
    }

    [[nodiscard]] Result<Entity> attempt(const Point2& side) const
    {
        const Entity& source = chain_.back();
        double distance = distance_;
        if (through_) {
            const auto measured = throughDistance(source.geometry, side);
            if (!measured || !(*measured > tol::kGeometric)) {
                return makeError(ErrorCode::InvalidArgument,
                                 "The through point lies on the " + kindName(source.geometry) +
                                     "; pick a point off it.");
            }
            distance = *measured;
        }
        auto moved = offsetGeometry(source.geometry, distance, side);
        if (!moved) {
            return moved.error();
        }
        // Refused here rather than at Enter, where the drawing would refuse
        // the session's one command and every offset made in it with this.
        if (auto status = drawable(*moved); !status) {
            return status.error();
        }
        return offsetCopy(source, std::move(*moved));
    }

    ToolStep offsetTowards(const Point2& side)
    {
        auto made = attempt(side);
        if (!made) {
            return ToolStep::rejected(made.error().message);
        }
        session_.begin();
        if (auto status = session_.add(*made); !status) {
            return ToolStep::rejected(status.error().message);
        }
        if (multiple_) {
            chain_.push_back(std::move(*made));
            return ToolStep::next();
        }
        chain_.clear();
        step_ = Step::Source;
        return ToolStep::next();
    }

    ToolContext context_;
    Defaults defaults_;
    EditSession session_;
    Step step_ = Step::Distance;
    double distance_ = 0.0;
    bool through_ = false;
    bool multiple_ = false;
    std::optional<Point2> first_;
    // The object being offset and, in Multiple, every copy made from it since:
    // the last is the next source.
    std::vector<Entity> chain_;
};

} // namespace

ToolPtr makeOffsetTool(const ToolContext& context, Defaults defaults)
{
    return std::make_unique<OffsetTool>(context, std::move(defaults));
}

} // namespace katana::cad::tools::modify_edit
