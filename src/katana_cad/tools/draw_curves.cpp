// Circle and Arc in their construction variants (see families.hpp).
//
// Two state machines, one per curve, each able to run every construction of
// its curve. The catalogue lists each construction as its own tool - a menu
// entry and an icon per variant, as a CAD user finds them - and every entry
// starts the same machine in a different construction. The plain CIRCLE and
// ARC verbs start the general form, which also takes the other constructions
// as options at its first prompts, as AutoCAD's commands do; the variants
// start in theirs and offer only what belongs to it.
//
// DIRECTION. An arc these tools make always runs counter-clockwise from its
// start angle to its end angle: a positive Arc2::sweep, with the start angle
// in [0, 2*pi). A 3-point arc picked clockwise is the same set of points and
// is stored reversed, from the third point to the first. One direction for
// every arc the tools make is the convention DXF and AutoCAD keep, and it lets
// every construction that is not 3-point state its meaning in one phrase:
// "counter-clockwise from the start".
//
// Both tools restart after each curve, as a CAD user expects of Circle and
// Arc; Esc ends them.

#include "families.hpp"

#include <algorithm>
#include <cmath>
#include <format>
#include <functional>
#include <memory>
#include <optional>
#include <string>
#include <string_view>
#include <utility>
#include <variant>
#include <vector>

#include "katana/core/text.hpp"
#include "katana/math/numerics.hpp"

namespace katana::cad::tools {

namespace {

namespace cmd = katana::commands;
namespace tol = katana::math::tolerance;

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::entity::EntityId;
using katana::geometry::Arc2;
using katana::geometry::Circle2;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;
using katana::geometry::Triangle2;
using katana::geometry::Vec2;
using katana::math::kDegToRad;
using katana::math::kRadToDeg;
using katana::math::kTwoPi;
using katana::math::normalizeAngle;

// ---- typed input -------------------------------------------------------------------

// An option keyword from a prompt: its capitalised short form or the whole
// word, in any case ("d", "D", "Diameter").
bool isOption(std::string_view text, std::string_view shortForm, std::string_view word = {})
{
    const std::string_view typed = katana::core::trimmed(text);
    return katana::core::equalsIgnoringCase(typed, shortForm) ||
           (!word.empty() && katana::core::equalsIgnoringCase(typed, word));
}

// The U of LINE: a typed Undo steps back exactly as the Undo button does.
bool isUndo(std::string_view text) { return isOption(text, "U", "UNDO"); }

std::optional<double> typedNumber(std::string_view text)
{
    return katana::core::parseFiniteDouble(katana::core::trimmed(text));
}

// A length or an angle for the command log: three decimals, trailing zeros
// dropped, so a radius of 5 reads "5" and one of 12.5 reads "12.5".
std::string figure(double value)
{
    std::string text = std::format("{:.3f}", value);
    while (text.back() == '0') {
        text.pop_back();
    }
    if (text.back() == '.') {
        text.pop_back();
    }
    return text == "-0" ? "0" : text;
}

// "Specify radius" + {"Diameter", "Undo"} -> "Specify radius or [Diameter/Undo]".
std::string withOptions(std::string what, const std::vector<std::string_view>& options)
{
    if (options.empty()) {
        return what;
    }
    what += " or [";
    for (std::size_t i = 0; i < options.size(); ++i) {
        what += (i == 0 ? "" : "/");
        what += options[i];
    }
    return what + "]";
}

bool coincide(const Point2& a, const Point2& b) { return a.distanceTo(b) <= tol::kGeometric; }

Point2 midway(const Point2& a, const Point2& b) { return (a + b) * 0.5; }

// ---- arcs --------------------------------------------------------------------------

// The same arc running counter-clockwise, its start angle in [0, 2*pi): the one
// form every arc these tools make is stored in (the header says why).
Arc2 counterClockwise(Arc2 arc)
{
    if (arc.sweep < 0.0) {
        arc = arc.reversed();
    }
    arc.startAngle = normalizeAngle(arc.startAngle);
    return arc;
}

// The arc about `centre` that starts at `start` and runs counter-clockwise to
// the direction of `towards`. `towards` only aims the end, as in AutoCAD: it
// need not lie on the arc, so a user can pick the end anywhere on its ray.
Result<Arc2> arcFromCentre(const Point2& centre, const Point2& start, const Point2& towards)
{
    const double radius = centre.distanceTo(start);
    if (!(radius > tol::kGeometric)) {
        return makeError(ErrorCode::InvalidGeometry,
                         "the start is on the centre, so the arc would have no radius");
    }
    if (coincide(towards, centre)) {
        return makeError(ErrorCode::InvalidGeometry,
                         "the end point is on the centre, so it gives the arc no direction to "
                         "end in");
    }
    const double startAngle = (start - centre).angle();
    const double sweep = normalizeAngle((towards - centre).angle() - startAngle);
    // A sweep that rounds to nothing is the end aimed along the start: the arc
    // would be a point. A full turn is a circle, which Circle draws.
    if (!(sweep * radius > tol::kGeometric) || !((kTwoPi - sweep) * radius > tol::kGeometric)) {
        return makeError(ErrorCode::InvalidGeometry,
                         "the end is in the start's direction from the centre, so the arc would "
                         "be empty; draw a circle for a full turn");
    }
    return Arc2{centre, radius, normalizeAngle(startAngle), sweep};
}

// The arc about `centre` from `start` through `degrees`, counter-clockwise
// when positive and clockwise when negative (stored reversed, so still
// counter-clockwise from its own start).
Result<Arc2> arcFromAngle(const Point2& centre, const Point2& start, double degrees)
{
    const double radius = centre.distanceTo(start);
    if (!(radius > tol::kGeometric)) {
        return makeError(ErrorCode::InvalidGeometry,
                         "the start is on the centre, so the arc would have no radius");
    }
    if (!(std::abs(degrees) * kDegToRad * radius > tol::kGeometric)) {
        return makeError(ErrorCode::InvalidGeometry,
                         "an included angle of zero draws nothing; give the angle in degrees");
    }
    if (std::abs(degrees) >= 360.0) {
        return makeError(ErrorCode::InvalidGeometry,
                         "an arc turns through less than 360 degrees; draw a circle for a full "
                         "turn");
    }
    return counterClockwise(Arc2{centre, radius, (start - centre).angle(), degrees * kDegToRad});
}

// The centre of an arc from `start` to `end` of signed `radius`. Every centre
// of an arc through both ends lies on their perpendicular bisector; a POSITIVE
// radius puts it on the left of start->end, where the counter-clockwise arc
// from start to end is the minor one, and a NEGATIVE radius on the right,
// where that arc is the major one. So the sign alone chooses the arc, and the
// arc is still counter-clockwise from start to end.
Result<Point2> centreForRadius(const Point2& start, const Point2& end, double radius)
{
    const double half = start.distanceTo(end) / 2.0;
    if (!(std::abs(radius) > tol::kGeometric)) {
        return makeError(ErrorCode::InvalidGeometry, "an arc's radius must not be zero");
    }
    if (std::abs(radius) < half - tol::kGeometric) {
        return makeError(ErrorCode::InvalidGeometry,
                         "a radius of " + figure(std::abs(radius)) +
                             " is less than half the distance between the ends (" + figure(half) +
                             "), so no arc of that radius joins them");
    }
    const Vec2 left = (end - start).normalized().perpendicular();
    // max(): a radius within tolerance of the half chord is the semicircle,
    // and must not become the square root of a tiny negative number.
    const double rise = std::sqrt(std::max(0.0, radius * radius - half * half));
    const double side = radius > 0.0 ? 1.0 : -1.0;
    return midway(start, end) + left * (rise * side);
}

// The counter-clockwise arc from `start` to `end` about `centre`, for a
// centre already on their bisector.
Arc2 arcBetween(const Point2& centre, const Point2& start, const Point2& end)
{
    const double startAngle = (start - centre).angle();
    const double sweep = normalizeAngle((end - centre).angle() - startAngle);
    return Arc2{centre, centre.distanceTo(start), normalizeAngle(startAngle), sweep};
}

// Where a picked point puts the centre of an arc from `start` to `end`: the
// point of their perpendicular bisector nearest it. The side it is on chooses
// the arc exactly as a radius's sign does (centreForRadius).
Point2 centreNear(const Point2& start, const Point2& end, const Point2& picked)
{
    const Point2 middle = midway(start, end);
    const Vec2 left = (end - start).normalized().perpendicular();
    return middle + left * (picked - middle).dot(left);
}

// ---- tangent circles ---------------------------------------------------------------

// A straight line the circle is to touch: a line entity, or the segment of a
// polyline nearest where it was picked.
struct TangentLine {
    EntityId id = katana::entity::kInvalidEntityId;
    std::size_t segment = 0; // the polyline segment; 0 for a line entity
    Segment2 line;
    Point2 pick;
};

// The corner two picked lines make: where they cross, and the unit direction
// of each ARM - the half of the line on the side it was picked.
struct Corner {
    Point2 apex;
    Vec2 first;
    Vec2 second;
};

Result<Corner> cornerOf(const TangentLine& a, const TangentLine& b)
{
    const Vec2 da = a.line.delta().normalized();
    const Vec2 db = b.line.delta().normalized();
    const double sine = da.cross(db);
    if (std::abs(sine) <= tol::kAngular) {
        return makeError(ErrorCode::InvalidGeometry,
                         "the two lines are parallel, so there is no corner between them for a "
                         "circle to sit in");
    }
    const double along = (b.line.start - a.line.start).cross(db) / sine;
    const Point2 apex = a.line.start + da * along;
    const double ta = (a.pick - apex).dot(da);
    const double tb = (b.pick - apex).dot(db);
    if (std::abs(ta) <= tol::kGeometric || std::abs(tb) <= tol::kGeometric) {
        return makeError(ErrorCode::InvalidGeometry,
                         "a line was picked where the two cross, which does not say which corner "
                         "the circle goes in; pick each line on the side the circle should touch");
    }
    return Corner{apex, da * (ta > 0.0 ? 1.0 : -1.0), db * (tb > 0.0 ? 1.0 : -1.0)};
}

// The circle of `radius` touching both lines, in the corner whose arms were
// picked.
//
// Two crossing lines make four corners, and a circle of a given radius fits
// in each, so there are four answers. The one taken is the circle in the
// corner between the arm of each line on the side it was picked - the corner
// a user means by picking near it, the reading Fillet gives its picks. Its
// centre is on that corner's bisector: c = apex + (u1 + u2) r / |u1 x u2|,
// since c - apex = a u1 + b u2 is r from the line along u1 exactly when
// |b (u1 x u2)| = r, and likewise a, so a = b = r / |sin| (u1, u2 unit).
// The circle may touch a line beyond the end of its segment: it is tangent
// to the LINE, as AutoCAD's Ttr is.
Circle2 circleInCorner(const Corner& corner, double radius)
{
    const double sine = std::abs(corner.first.cross(corner.second));
    return Circle2{corner.apex + (corner.first + corner.second) * (radius / sine), radius};
}

// The line of entity `id` near `at`, or why it is not one.
Result<TangentLine> tangentLineOf(const Document* document, EntityId id, const Point2& at)
{
    const katana::entity::Entity* entity =
        document == nullptr ? nullptr : document->model().entities.find(id);
    if (entity == nullptr) {
        return makeError(ErrorCode::NotFound, "that entity is not in the drawing");
    }
    TangentLine out;
    out.id = id;
    out.pick = at;
    if (const auto* line = std::get_if<Segment2>(&entity->geometry)) {
        out.line = *line;
    } else if (const auto* polyline = std::get_if<Polyline2>(&entity->geometry)) {
        // The segment the pick is nearest, skipping zero-length ones, which
        // have no direction to be tangent to.
        double best = 0.0;
        bool found = false;
        for (std::size_t i = 0; i < polyline->segmentCount(); ++i) {
            const Segment2 segment = polyline->segment(i);
            if (segment.isDegenerate()) {
                continue;
            }
            const double distance = segment.distanceTo(at);
            if (!found || distance < best) {
                best = distance;
                found = true;
                out.segment = i;
                out.line = segment;
            }
        }
        if (!found) {
            return makeError(ErrorCode::InvalidGeometry, "that polyline has no length");
        }
    } else {
        return makeError(ErrorCode::InvalidArgument,
                         "the circle is fitted to straight lines; pick a line or a polyline");
    }
    if (out.line.isDegenerate()) {
        return makeError(ErrorCode::InvalidGeometry,
                         "that line has no length, so it has no direction to touch");
    }
    return out;
}

// ---- Circle ------------------------------------------------------------------------

enum class CircleMode { CentreRadius, CentreDiameter, TwoPoint, ThreePoint, TangentTangentRadius };

class CircleTool final : public InteractiveTool {
  public:
    // `general`: the CIRCLE verb's form, which offers the other constructions
    // as options at its first prompt.
    CircleTool(const ToolContext& context, CircleMode mode, bool general)
        : document_(context.document), attributes_(context.attributes), general_(general)
    {
        state_.mode = mode;
    }

    [[nodiscard]] std::string prompt() const override
    {
        const std::size_t n = state_.points.size();
        switch (state_.mode) {
        case CircleMode::CentreRadius:
        case CircleMode::CentreDiameter:
            if (n == 0) {
                return withOptions("Specify centre point",
                                   general_ ? std::vector<std::string_view>{"3P", "2P", "Ttr"}
                                            : undoOption());
            }
            return state_.mode == CircleMode::CentreRadius
                       ? withOptions("Specify radius", withUndo({"Diameter"}))
                       : withOptions("Specify diameter", withUndo({"Radius"}));
        case CircleMode::TwoPoint:
            return withOptions(n == 0 ? "Specify first end of the circle's diameter"
                                      : "Specify second end of the circle's diameter",
                               undoOption());
        case CircleMode::ThreePoint:
            return withOptions(n == 0   ? "Specify first point on the circle"
                               : n == 1 ? "Specify second point on the circle"
                                        : "Specify third point on the circle",
                               undoOption());
        case CircleMode::TangentTangentRadius:
            return withOptions(state_.tangents.empty()        ? "Select the first line to touch"
                               : state_.tangents.size() == 1 ? "Select the second line to touch"
                                                              : "Specify radius of circle",
                               undoOption());
        }
        return {};
    }

    [[nodiscard]] ToolInput expects() const override
    {
        if (state_.mode == CircleMode::TangentTangentRadius) {
            return state_.tangents.size() < 2 ? ToolInput::Entity : ToolInput::Value;
        }
        return ToolInput::Point;
    }

    ToolStep point(const Point2& at) override
    {
        const std::vector<Point2>& points = state_.points;
        switch (state_.mode) {
        case CircleMode::CentreRadius:
        case CircleMode::CentreDiameter: {
            if (points.empty()) {
                return acceptPoint(at);
            }
            // A picked point is on the circumference for a radius, and at the
            // far end of the diameter for a diameter, as the rubber band shows.
            const double length = points[0].distanceTo(at);
            return finish(Circle2{points[0], state_.mode == CircleMode::CentreRadius
                                                 ? length
                                                 : length / 2.0},
                          "the point is on the centre");
        }
        case CircleMode::TwoPoint:
            if (points.empty()) {
                return acceptPoint(at);
            }
            if (coincide(points[0], at)) {
                return ToolStep::rejected(
                    "the second end is on the first, so the circle would have no diameter");
            }
            return finish(Circle2{midway(points[0], at), points[0].distanceTo(at) / 2.0}, {});
        case CircleMode::ThreePoint: {
            for (const Point2& earlier : points) {
                if (coincide(earlier, at)) {
                    return ToolStep::rejected("that point is on one already picked; a circle "
                                              "needs three separate points");
                }
            }
            if (points.size() < 2) {
                return acceptPoint(at);
            }
            const auto circle = Triangle2{points[0], points[1], at}.circumcircle();
            if (!circle) {
                return ToolStep::rejected(
                    "the three points are in a line, so no circle passes through them");
            }
            return finish(*circle, {});
        }
        case CircleMode::TangentTangentRadius:
            return ToolStep::rejected(state_.tangents.size() < 2
                                          ? "select a line in the drawing: the circle is fitted "
                                            "to two lines, not placed at a point"
                                          : "type the radius as a number");
        }
        return ToolStep::rejected("the tool is in no construction");
    }

    ToolStep entity(EntityId id, const Point2& at) override
    {
        if (state_.mode != CircleMode::TangentTangentRadius || state_.tangents.size() >= 2) {
            return InteractiveTool::entity(id, at);
        }
        auto picked = tangentLineOf(document_, id, at);
        if (!picked) {
            return ToolStep::rejected(picked.error().message);
        }
        if (!state_.tangents.empty()) {
            const TangentLine& first = state_.tangents.front();
            if (first.id == picked->id && first.segment == picked->segment) {
                return ToolStep::rejected("that line is already picked; pick the other line");
            }
            if (auto corner = cornerOf(first, *picked); !corner) {
                return ToolStep::rejected(corner.error().message);
            }
        }
        CircleState next = state_;
        next.tangents.push_back(*picked);
        return accept(std::move(next));
    }

    ToolStep value(std::string_view text) override
    {
        if (isUndo(text)) {
            return undo();
        }
        const std::size_t n = state_.points.size();
        const bool centred = state_.mode == CircleMode::CentreRadius ||
                             state_.mode == CircleMode::CentreDiameter;
        if (centred && n == 0 && general_) {
            if (isOption(text, "3P")) {
                return switchTo(CircleMode::ThreePoint);
            }
            if (isOption(text, "2P")) {
                return switchTo(CircleMode::TwoPoint);
            }
            if (isOption(text, "T", "TTR")) {
                return switchTo(CircleMode::TangentTangentRadius);
            }
        }
        if (centred && n == 1) {
            if (state_.mode == CircleMode::CentreRadius && isOption(text, "D", "DIAMETER")) {
                return switchTo(CircleMode::CentreDiameter);
            }
            if (state_.mode == CircleMode::CentreDiameter && isOption(text, "R", "RADIUS")) {
                return switchTo(CircleMode::CentreRadius);
            }
            const auto number = typedNumber(text);
            if (!number) {
                return ToolStep::rejected(
                    "'" + std::string(text) + "' is not a " + measureName() +
                    "; type a number, pick a point on the circle, or " +
                    (state_.mode == CircleMode::CentreRadius ? "D" : "R"));
            }
            if (!(*number > 0.0)) {
                return ToolStep::rejected("a " + measureName() + " must be greater than zero");
            }
            return finish(Circle2{state_.points[0], state_.mode == CircleMode::CentreRadius
                                                        ? *number
                                                        : *number / 2.0},
                          {});
        }
        if (state_.mode == CircleMode::TangentTangentRadius && state_.tangents.size() == 2) {
            const auto number = typedNumber(text);
            if (!number) {
                return ToolStep::rejected("'" + std::string(text) +
                                          "' is not a radius; type a number");
            }
            if (!(*number > 0.0)) {
                return ToolStep::rejected("a radius must be greater than zero");
            }
            auto corner = cornerOf(state_.tangents[0], state_.tangents[1]);
            if (!corner) {
                return ToolStep::rejected(corner.error().message);
            }
            return finish(circleInCorner(*corner, *number), {});
        }
        if (state_.mode == CircleMode::TangentTangentRadius) {
            return ToolStep::rejected("select a line in the drawing for the circle to touch");
        }
        return ToolStep::rejected("'" + std::string(text) +
                                  "' is not a point; type one as x,y or pick it" +
                                  (centred && n == 0 && general_ ? ", or type 3P, 2P or Ttr"
                                                                 : ""));
    }

    ToolStep undo() override
    {
        if (history_.empty()) {
            return InteractiveTool::undo();
        }
        state_ = std::move(history_.back());
        history_.pop_back();
        return ToolStep::next();
    }

    [[nodiscard]] ToolFeedback preview(const Point2& cursor) const override
    {
        ToolFeedback feedback;
        const std::vector<Point2>& points = state_.points;
        switch (state_.mode) {
        case CircleMode::CentreRadius:
        case CircleMode::CentreDiameter:
            if (points.size() == 1) {
                const double length = points[0].distanceTo(cursor);
                feedback.shapes.emplace_back(Circle2{
                    points[0], state_.mode == CircleMode::CentreRadius ? length : length / 2.0});
                feedback.shapes.emplace_back(Segment2{points[0], cursor});
                feedback.markers.push_back(points[0]);
            }
            break;
        case CircleMode::TwoPoint:
            if (points.size() == 1) {
                feedback.shapes.emplace_back(
                    Circle2{midway(points[0], cursor), points[0].distanceTo(cursor) / 2.0});
                feedback.shapes.emplace_back(Segment2{points[0], cursor});
                feedback.markers.push_back(points[0]);
            }
            break;
        case CircleMode::ThreePoint:
            if (points.size() == 1) {
                feedback.shapes.emplace_back(Segment2{points[0], cursor});
            } else if (points.size() == 2) {
                if (const auto circle = Triangle2{points[0], points[1], cursor}.circumcircle()) {
                    feedback.shapes.emplace_back(*circle);
                } else {
                    feedback.shapes.emplace_back(Segment2{points[1], cursor});
                }
            }
            feedback.markers = points;
            break;
        case CircleMode::TangentTangentRadius:
            for (const TangentLine& line : state_.tangents) {
                feedback.markers.push_back(line.line.closestPoint(line.pick));
            }
            if (state_.tangents.size() == 2) {
                if (const auto corner = cornerOf(state_.tangents[0], state_.tangents[1])) {
                    feedback.markers.push_back(corner->apex);
                }
            }
            break;
        }
        return feedback;
    }

    [[nodiscard]] std::optional<Point2> lastPoint() const override
    {
        if (state_.points.empty()) {
            return std::nullopt;
        }
        return state_.points.back();
    }

  private:
    struct CircleState {
        CircleMode mode = CircleMode::CentreRadius;
        std::vector<Point2> points;
        std::vector<TangentLine> tangents;
    };

    [[nodiscard]] std::vector<std::string_view> undoOption() const
    {
        return history_.empty() ? std::vector<std::string_view>{}
                                : std::vector<std::string_view>{"Undo"};
    }
    [[nodiscard]] std::vector<std::string_view> withUndo(std::vector<std::string_view> options) const
    {
        if (!history_.empty()) {
            options.emplace_back("Undo");
        }
        return options;
    }
    [[nodiscard]] std::string measureName() const
    {
        return state_.mode == CircleMode::CentreDiameter ? "diameter" : "radius";
    }

    // Every accepted input is recorded, so Undo steps back over an option as
    // readily as over a point.
    ToolStep accept(CircleState next)
    {
        history_.push_back(std::move(state_));
        state_ = std::move(next);
        return ToolStep::next();
    }
    ToolStep acceptPoint(const Point2& at)
    {
        CircleState next = state_;
        next.points.push_back(at);
        return accept(std::move(next));
    }
    ToolStep switchTo(CircleMode mode)
    {
        CircleState next = state_;
        next.mode = mode;
        return accept(std::move(next));
    }

    // `zeroWhy`: the refusal for a circle of no size, when this input can
    // produce one.
    ToolStep finish(const Circle2& circle, std::string_view zeroWhy)
    {
        if (!(circle.radius > tol::kGeometric) || !std::isfinite(circle.radius)) {
            return ToolStep::rejected(
                zeroWhy.empty() ? std::string("the circle would have no radius")
                                : std::string(zeroWhy) + ", so the circle would have no radius");
        }
        return ToolStep::done(cmd::createCircle(circle.center, circle.radius, attributes_),
                              "circle, radius " + figure(circle.radius), true);
    }

    const Document* document_;
    cmd::EntityAttributes attributes_;
    bool general_;
    CircleState state_;
    std::vector<CircleState> history_;
};

// ---- Arc ---------------------------------------------------------------------------

enum class ArcMode { ThreePoint, StartCentreEnd, CentreStartEnd, StartEndRadius };

class ArcTool final : public InteractiveTool {
  public:
    // `general`: the ARC verb's form, which offers the other constructions as
    // options at its first two prompts.
    ArcTool(const ToolContext& context, ArcMode mode, bool general)
        : attributes_(context.attributes), general_(general)
    {
        state_.mode = mode;
    }

    [[nodiscard]] std::string prompt() const override
    {
        const std::size_t n = state_.points.size();
        switch (state_.mode) {
        case ArcMode::ThreePoint:
            if (n == 0) {
                return withOptions("Specify start point of arc",
                                   general_ ? std::vector<std::string_view>{"Centre"}
                                            : std::vector<std::string_view>{});
            }
            if (n == 1) {
                return withOptions("Specify second point of arc",
                                   general_ ? withUndo({"Centre", "End"}) : withUndo({}));
            }
            return withOptions("Specify end point of arc", withUndo({}));
        case ArcMode::StartCentreEnd:
            if (n == 0) {
                return withOptions("Specify start point of arc", withUndo({}));
            }
            if (n == 1) {
                return withOptions("Specify centre point of arc", withUndo({}));
            }
            return endPrompt();
        case ArcMode::CentreStartEnd:
            if (n == 0) {
                return withOptions("Specify centre point of arc", withUndo({}));
            }
            if (n == 1) {
                return withOptions("Specify start point of arc", withUndo({}));
            }
            return endPrompt();
        case ArcMode::StartEndRadius:
            if (n == 0) {
                return withOptions("Specify start point of arc", withUndo({}));
            }
            if (n == 1) {
                return withOptions("Specify end point of arc", withUndo({}));
            }
            return withOptions("Specify radius of arc (negative for the major arc) or pick its "
                               "centre",
                               withUndo({}));
        }
        return {};
    }

    [[nodiscard]] ToolInput expects() const override
    {
        return state_.byAngle ? ToolInput::Value : ToolInput::Point;
    }

    ToolStep point(const Point2& at) override
    {
        const std::vector<Point2>& points = state_.points;
        if (state_.byAngle) {
            return ToolStep::rejected("type the included angle in degrees");
        }
        switch (state_.mode) {
        case ArcMode::ThreePoint: {
            for (const Point2& earlier : points) {
                if (coincide(earlier, at)) {
                    return ToolStep::rejected("that point is on one already picked; an arc "
                                              "through three points needs three separate points");
                }
            }
            if (points.size() < 2) {
                return acceptPoint(at);
            }
            const auto arc = Arc2::throughPoints(points[0], points[1], at);
            if (!arc) {
                return ToolStep::rejected(
                    "the three points are in a line, so no arc passes through them");
            }
            return finish(counterClockwise(*arc));
        }
        case ArcMode::StartCentreEnd:
        case ArcMode::CentreStartEnd: {
            if (points.size() == 1 && coincide(points[0], at)) {
                return ToolStep::rejected(
                    "the start and the centre coincide, so the arc would have no radius");
            }
            if (points.size() < 2) {
                return acceptPoint(at);
            }
            auto arc = arcFromCentre(centre(), start(), at);
            if (!arc) {
                return ToolStep::rejected(arc.error().message);
            }
            return finish(*arc);
        }
        case ArcMode::StartEndRadius:
            if (points.size() == 1 && coincide(points[0], at)) {
                return ToolStep::rejected(
                    "the end is on the start, so there is no arc between them");
            }
            if (points.size() < 2) {
                return acceptPoint(at);
            }
            return finish(arcBetween(centreNear(points[0], points[1], at), points[0], points[1]));
        }
        return ToolStep::rejected("the tool is in no construction");
    }

    ToolStep value(std::string_view text) override
    {
        if (isUndo(text)) {
            return undo();
        }
        const std::size_t n = state_.points.size();
        if (state_.mode == ArcMode::ThreePoint && general_ && n == 0 &&
            isOption(text, "C", "CENTRE")) {
            return switchTo(ArcMode::CentreStartEnd);
        }
        if (state_.mode == ArcMode::ThreePoint && general_ && n == 1) {
            if (isOption(text, "C", "CENTRE")) {
                return switchTo(ArcMode::StartCentreEnd);
            }
            if (isOption(text, "E", "END")) {
                return switchTo(ArcMode::StartEndRadius);
            }
        }
        const bool centred =
            state_.mode == ArcMode::StartCentreEnd || state_.mode == ArcMode::CentreStartEnd;
        if (centred && n == 2 && !state_.byAngle && isOption(text, "A", "ANGLE")) {
            ArcState next = state_;
            next.byAngle = true;
            return accept(std::move(next));
        }
        if (state_.byAngle) {
            const auto degrees = typedNumber(text);
            if (!degrees) {
                return ToolStep::rejected("'" + std::string(text) +
                                          "' is not an angle; type it in degrees");
            }
            auto arc = arcFromAngle(centre(), start(), *degrees);
            if (!arc) {
                return ToolStep::rejected(arc.error().message);
            }
            return finish(*arc);
        }
        if (state_.mode == ArcMode::StartEndRadius && n == 2) {
            const auto radius = typedNumber(text);
            if (!radius) {
                return ToolStep::rejected("'" + std::string(text) +
                                          "' is not a radius; type a number or pick the centre");
            }
            auto middle = centreForRadius(state_.points[0], state_.points[1], *radius);
            if (!middle) {
                return ToolStep::rejected(middle.error().message);
            }
            return finish(arcBetween(*middle, state_.points[0], state_.points[1]));
        }
        return ToolStep::rejected("'" + std::string(text) +
                                  "' is not a point; type one as x,y or pick it");
    }

    ToolStep undo() override
    {
        if (history_.empty()) {
            return InteractiveTool::undo();
        }
        state_ = std::move(history_.back());
        history_.pop_back();
        return ToolStep::next();
    }

    [[nodiscard]] ToolFeedback preview(const Point2& cursor) const override
    {
        ToolFeedback feedback;
        const std::vector<Point2>& points = state_.points;
        feedback.markers = points;
        if (points.empty() || state_.byAngle) {
            return feedback;
        }
        switch (state_.mode) {
        case ArcMode::ThreePoint:
            if (points.size() == 2) {
                if (const auto arc = Arc2::throughPoints(points[0], points[1], cursor)) {
                    feedback.shapes.emplace_back(counterClockwise(*arc));
                    break;
                }
            }
            feedback.shapes.emplace_back(Segment2{points.back(), cursor});
            break;
        case ArcMode::StartCentreEnd:
        case ArcMode::CentreStartEnd:
            if (points.size() == 2) {
                if (const auto arc = arcFromCentre(centre(), start(), cursor)) {
                    feedback.shapes.emplace_back(*arc);
                }
                feedback.shapes.emplace_back(Segment2{centre(), cursor});
            } else {
                feedback.shapes.emplace_back(Segment2{points[0], cursor});
            }
            break;
        case ArcMode::StartEndRadius:
            if (points.size() == 2) {
                const Point2 middle = centreNear(points[0], points[1], cursor);
                feedback.shapes.emplace_back(arcBetween(middle, points[0], points[1]));
                feedback.markers.push_back(middle);
            } else {
                feedback.shapes.emplace_back(Segment2{points[0], cursor});
            }
            break;
        }
        return feedback;
    }

    [[nodiscard]] std::optional<Point2> lastPoint() const override
    {
        if (state_.points.empty()) {
            return std::nullopt;
        }
        return state_.points.back();
    }

  private:
    struct ArcState {
        ArcMode mode = ArcMode::ThreePoint;
        std::vector<Point2> points;
        // Start-centre-end: the included angle is being typed instead of an
        // end point being picked (the Angle option).
        bool byAngle = false;
    };

    [[nodiscard]] std::vector<std::string_view> withUndo(std::vector<std::string_view> options) const
    {
        if (!history_.empty()) {
            options.emplace_back("Undo");
        }
        return options;
    }
    [[nodiscard]] std::string endPrompt() const
    {
        if (state_.byAngle) {
            return withOptions("Specify included angle in degrees (negative for clockwise)",
                               withUndo({}));
        }
        return withOptions("Specify end point of arc, counter-clockwise from the start",
                           withUndo({"Angle"}));
    }
    // The centre and the start, in whichever order the construction took them.
    [[nodiscard]] const Point2& centre() const
    {
        return state_.points[state_.mode == ArcMode::StartCentreEnd ? 1 : 0];
    }
    [[nodiscard]] const Point2& start() const
    {
        return state_.points[state_.mode == ArcMode::StartCentreEnd ? 0 : 1];
    }

    ToolStep accept(ArcState next)
    {
        history_.push_back(std::move(state_));
        state_ = std::move(next);
        return ToolStep::next();
    }
    ToolStep acceptPoint(const Point2& at)
    {
        ArcState next = state_;
        next.points.push_back(at);
        return accept(std::move(next));
    }
    ToolStep switchTo(ArcMode mode)
    {
        ArcState next = state_;
        next.mode = mode;
        return accept(std::move(next));
    }

    ToolStep finish(const Arc2& arc)
    {
        // The model refuses these too; refusing here keeps the tool where it
        // is, with a sentence, instead of failing the command.
        if (!(arc.radius > tol::kGeometric) || !std::isfinite(arc.radius) ||
            !(arc.length() > tol::kGeometric)) {
            return ToolStep::rejected("the arc would have no length");
        }
        return ToolStep::done(cmd::createArc(arc, attributes_),
                              "arc, radius " + figure(arc.radius) + ", " +
                                  figure(arc.sweep * kRadToDeg) + " degrees",
                              true);
    }

    cmd::EntityAttributes attributes_;
    bool general_;
    ArcState state_;
    std::vector<ArcState> history_;
};

// ---- the catalogue -----------------------------------------------------------------

ToolInfo curveTool(std::string id, std::string name, int order, std::vector<std::string> aliases,
                   std::string tip,
                   std::function<std::unique_ptr<InteractiveTool>(const ToolContext&)> make)
{
    ToolInfo info;
    info.id = std::move(id);
    info.name = std::move(name);
    info.category = "Draw";
    info.group = "Curves";
    info.order = order;
    info.aliases = std::move(aliases);
    info.tip = std::move(tip);
    info.make = std::move(make);
    return info;
}

std::function<std::unique_ptr<InteractiveTool>(const ToolContext&)> circleTool(CircleMode mode,
                                                                                bool general)
{
    return [mode, general](const ToolContext& context) {
        return std::make_unique<CircleTool>(context, mode, general);
    };
}

std::function<std::unique_ptr<InteractiveTool>(const ToolContext&)> arcTool(ArcMode mode,
                                                                             bool general)
{
    return [mode, general](const ToolContext& context) {
        return std::make_unique<ArcTool>(context, mode, general);
    };
}

} // namespace

void addDrawCurveTools(ToolCatalog& catalog, const Report& report)
{
    // CIRCLE and ARC mean here what they mean to the command interpreter
    // (centre and radius; start, a point on the arc, end), and what they mean
    // in AutoCAD. The variants have no AutoCAD verb of their own - there they
    // are options of CIRCLE and ARC, as they are of the general tools here.
    report(catalog.add(curveTool(
        "draw.circle", "Circle, Centre Radius", 10, {"CIRCLE", "C"},
        "Draws a circle from its centre and radius; type D for a diameter, or 3P, 2P or Ttr for "
        "the other constructions.",
        circleTool(CircleMode::CentreRadius, true))));
    report(catalog.add(curveTool("draw.circle.diameter", "Circle, Centre Diameter", 20, {},
                                 "Draws a circle from its centre and diameter.",
                                 circleTool(CircleMode::CentreDiameter, false))));
    report(catalog.add(curveTool("draw.circle.2p", "Circle, 2 Points", 30, {},
                                 "Draws a circle on two points at the ends of its diameter.",
                                 circleTool(CircleMode::TwoPoint, false))));
    report(catalog.add(curveTool("draw.circle.3p", "Circle, 3 Points", 40, {},
                                 "Draws the circle through three points.",
                                 circleTool(CircleMode::ThreePoint, false))));
    report(catalog.add(curveTool(
        "draw.circle.ttr", "Circle, Tangent Tangent Radius", 50, {},
        "Draws a circle of a given radius touching two lines, in the corner whose sides you pick.",
        circleTool(CircleMode::TangentTangentRadius, false))));

    report(catalog.add(curveTool(
        "draw.arc", "Arc, 3 Points", 60, {"ARC", "A"},
        "Draws an arc through its start, a second point and its end; type C or E for the "
        "centre and radius constructions.",
        arcTool(ArcMode::ThreePoint, true))));
    report(catalog.add(curveTool(
        "draw.arc.sce", "Arc, Start Centre End", 70, {},
        "Draws an arc counter-clockwise about a centre, from a start point to the direction of "
        "an end point or through an angle.",
        arcTool(ArcMode::StartCentreEnd, false))));
    report(catalog.add(curveTool(
        "draw.arc.cse", "Arc, Centre Start End", 80, {},
        "Draws an arc about a centre picked first, counter-clockwise from a start point to the "
        "direction of an end point or through an angle.",
        arcTool(ArcMode::CentreStartEnd, false))));
    report(catalog.add(curveTool(
        "draw.arc.ser", "Arc, Start End Radius", 90, {},
        "Draws an arc counter-clockwise between two ends with a given radius: positive for the "
        "minor arc, negative for the major one.",
        arcTool(ArcMode::StartEndRadius, false))));
}

} // namespace katana::cad::tools
