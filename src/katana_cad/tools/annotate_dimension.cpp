// Aligned and linear dimensions, as AutoCAD's DIMALIGNED and DIMLINEAR take
// them:
//
//   Specify first extension line origin or <select object>
//                                        a point, or Enter to pick a line or a
//                                        polyline segment instead
//   Specify second extension line origin or [Undo]
//   Specify dimension line location ...  a point; T gives the text; Linear
//                                        also takes H, V or R (Rotated: a
//                                        typed angle, or two points on the
//                                        line)
//
// ALIGNED is a DimensionGeometry of the Aligned kind, made by
// annotation::alignedDimension as DIM ALIGNED makes it: |end - start|, its
// dimension line parallel to start->end and displaced by `offset` to its
// LEFT (negative: to its right). The offset is the signed distance of the
// picked location from the line through the origins, or a typed number.
//
// LINEAR is one of two things, and the model can hold both:
//
//   * The Linear kind (DimensionKind::Linear, DIM LINEAR's, made by
//     annotation::linearDimension), measuring along a direction from the
//     two origins themselves. The tool makes it for a Rotated direction, for
//     a dimension line BETWEEN the two origins' levels (each extension line
//     then leaves its own origin, in opposite directions, as AutoCAD's
//     DIMLINEAR draws them), and whenever an origin was snapped to an
//     entity's end, middle, centre or vertex - the only kind that can follow
//     its origins, since its start and end ARE them.
//   * Otherwise the PROJECTION this tool has always stored: an aligned
//     dimension of the origins projected onto the chosen axis. A horizontal
//     dimension of (x1, y1) and (x2, y2) placed at height yd is stored as
//     start (x1, yb), end (x2, yb) - so it measures |x2 - x1| - with yb the
//     level of whichever origin is FARTHER from the dimension line. That
//     choice decides the extension lines, because the model draws both from
//     its start and end towards the dimension line, each starting DIMEXO
//     (extensionOffset) clear of its point:
//       - the farther origin's extension line is exactly AutoCAD's, with the
//         origin in the DIMEXO gap at its foot;
//       - the nearer origin's starts DIMEXO beyond the farther one's level.
//         When the two levels differ by more than DIMEXO it runs THROUGH its
//         origin and on past it, away from the dimension line, by the
//         difference less DIMEXO; when they differ by less, it starts beyond
//         its origin, which sits in a gap of DIMEXO less the difference.
//     Every origin therefore lies on the line of its own extension line.
//     Kept for placements it can draw because every drawing made before the
//     Linear kind (2026-09-25) holds it, and a plain click still makes what
//     it made then.
//
// Angular, radius, diameter and ordinate dimensions are in
// annotate_dimension_kinds.cpp; baseline and continued chains in
// annotate_dimension_chain.cpp.

#include <algorithm>
#include <cmath>
#include <limits>
#include <optional>
#include <string>
#include <variant>
#include <vector>

#include "annotate_common.hpp"
#include "katana/cad/annotation/dimension_build.hpp"
#include "katana/core/error.hpp"
#include "katana/entity/entity.hpp"
#include "katana/math/numerics.hpp"

namespace katana::cad::tools::annotate {

namespace {

namespace ann = katana::cad::annotation;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::entity::AnchorRef;
using katana::entity::DimensionGeometry;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;
using katana::geometry::Vec2;
namespace tol = katana::math::tolerance;

enum class Kind { Aligned, Linear };
enum class Orientation { Horizontal, Vertical };

// What the dimension line prompt has been told so far. Each option typed there
// pushes the state it replaces, so Undo takes back the last option first and
// only then the second origin.
struct LocationState {
    std::string textOverride; // empty: the measured distance
    std::optional<Orientation> orientation; // Linear only; empty: from the location
    std::optional<double> angle; // Linear only: Rotated, radians from east
};

class DimensionTool final : public InteractiveTool {
  public:
    DimensionTool(const ToolContext& context, Kind kind)
        : document_(context.document), attributes_(context.attributes), kind_(kind)
    {
    }

    [[nodiscard]] std::string prompt() const override
    {
        switch (step_) {
        case Step::First:
            return "Specify first extension line origin or <select object>";
        case Step::Object:
            return "Select a line or a polyline segment to dimension";
        case Step::Second:
            return "Specify second extension line origin or [Undo]";
        case Step::Location:
            if (kind_ == Kind::Aligned) {
                return "Specify dimension line location or its offset, or [Text/Undo]";
            }
            // Once H, V or R is typed the prompt says so, since nothing else
            // on screen shows that the cursor no longer chooses.
            if (state_.angle) {
                return "Specify location of the dimension line rotated " +
                       formatNumber(*state_.angle * katana::math::kRadToDeg) +
                       " degrees or [Text/Horizontal/Vertical/Rotated/Undo]";
            }
            if (state_.orientation) {
                return std::string("Specify location of the ") +
                       (*state_.orientation == Orientation::Horizontal ? "horizontal"
                                                                       : "vertical") +
                       " dimension line or [Text/Horizontal/Vertical/Rotated/Undo]";
            }
            return "Specify dimension line location or [Text/Horizontal/Vertical/Rotated/Undo]";
        case Step::Text:
            return "Enter dimension text, or press Enter for the measured distance";
        case Step::Angle:
            return "Specify angle of dimension line or two points on it <" +
                   formatNumber(state_.angle.value_or(0.0) * katana::math::kRadToDeg) + ">";
        case Step::AngleSecond:
            return "Specify second point";
        }
        return {};
    }

    [[nodiscard]] ToolInput expects() const override
    {
        switch (step_) {
        case Step::Object:
            return ToolInput::Entity;
        case Step::Text:
            return ToolInput::Value;
        case Step::First:
        case Step::Second:
        case Step::Location:
        case Step::Angle:
        case Step::AngleSecond:
            break;
        }
        return ToolInput::Point;
    }

    ToolStep point(const Point2& at) override
    {
        switch (step_) {
        case Step::First:
            first_ = at;
            firstRef_ = pending_;
            step_ = Step::Second;
            return ToolStep::next();
        case Step::Second:
            if (coincident(first_, at)) {
                return ToolStep::rejected(
                    "the second origin is on the first; a dimension needs two distinct points");
            }
            second_ = at;
            secondRef_ = pending_;
            picked_ = false;
            step_ = Step::Location;
            return ToolStep::next();
        case Step::Location: {
            auto dimension = place(at);
            if (!dimension) {
                return ToolStep::rejected(dimension.error().message);
            }
            return finish(*dimension);
        }
        case Step::Angle:
            angleFrom_ = at;
            step_ = Step::AngleSecond;
            return ToolStep::next();
        case Step::AngleSecond:
            if (coincident(angleFrom_, at)) {
                return ToolStep::rejected("the second point is on the first; two points give a "
                                          "direction only when they are apart");
            }
            return rotate((at - angleFrom_).angle());
        case Step::Object:
        case Step::Text:
            break;
        }
        return InteractiveTool::point(at);
    }

    // An origin snapped to an entity's point keeps its reference, so the
    // dimension follows the entity (DIM ALIGNED #id.end does the same).
    ToolStep anchoredPoint(const Point2& at, const AnchorRef& anchor) override
    {
        pending_ = anchor;
        ToolStep step = point(at);
        pending_ = {};
        return step;
    }

    ToolStep entity(katana::entity::EntityId id, const Point2& at) override
    {
        if (step_ != Step::Object) {
            return InteractiveTool::entity(id, at);
        }
        const katana::entity::Entity* picked =
            document_ == nullptr ? nullptr : document_->model().entities.find(id);
        if (picked == nullptr) {
            return ToolStep::rejected("that entity is not in the drawing");
        }
        std::optional<Segment2> measured;
        if (const auto* line = std::get_if<Segment2>(&picked->geometry)) {
            measured = *line;
        } else if (const auto* polyline = std::get_if<Polyline2>(&picked->geometry)) {
            measured = nearestSegment(*polyline, at);
        } else {
            return ToolStep::rejected(
                "only a line or a polyline segment can be dimensioned by picking it; pick "
                "another, or press Enter to pick the two origins instead");
        }
        if (!measured || coincident(measured->start, measured->end)) {
            return ToolStep::rejected("that segment has no length to dimension");
        }
        first_ = measured->start;
        second_ = measured->end;
        firstRef_ = {};
        secondRef_ = {};
        picked_ = true;
        step_ = Step::Location;
        return ToolStep::next();
    }

    ToolStep value(std::string_view text) override
    {
        switch (step_) {
        case Step::First:
            if (isOption(text, "Undo")) {
                return undo();
            }
            return ToolStep::rejected("click the first origin or type it as x,y, or press Enter "
                                      "to pick a line instead");
        case Step::Object:
            return ToolStep::rejected("pick a line or a polyline segment in the drawing, or press "
                                      "Enter to pick the two origins instead");
        case Step::Second:
            if (isOption(text, "Undo")) {
                return undo();
            }
            return ToolStep::rejected("click the second origin or type it as x,y");
        case Step::Location:
            return locationValue(text);
        case Step::Text:
            if (text.empty()) {
                return enter();
            }
            if (!katana::entity::isValidUtf8(text)) {
                return ToolStep::rejected("the text is not valid UTF-8");
            }
            remember();
            state_.textOverride = std::string(text);
            step_ = Step::Location;
            return ToolStep::next();
        case Step::Angle: {
            if (isOption(text, "Undo")) {
                return undo();
            }
            const auto degrees = typedNumber(text);
            if (!degrees) {
                return ToolStep::rejected("type the angle in degrees counter-clockwise from east, "
                                          "or click two points on the dimension line");
            }
            return rotate(*degrees * katana::math::kDegToRad);
        }
        case Step::AngleSecond:
            if (isOption(text, "Undo")) {
                return undo();
            }
            return ToolStep::rejected("click the second point on the dimension line");
        }
        return InteractiveTool::value(text);
    }

    ToolStep enter() override
    {
        switch (step_) {
        case Step::First:
            step_ = Step::Object;
            return ToolStep::next();
        case Step::Object:
            step_ = Step::First;
            return ToolStep::next();
        case Step::Second:
            return ToolStep::rejected("specify the second origin");
        case Step::Location:
            return ToolStep::rejected("specify where the dimension line goes");
        case Step::Text:
            // Enter at the text prompt keeps (or goes back to) the measured
            // distance, as AutoCAD's <measured> default does.
            if (!state_.textOverride.empty()) {
                remember();
                state_.textOverride.clear();
            }
            step_ = Step::Location;
            return ToolStep::next();
        case Step::Angle:
            // The angle offered, as AutoCAD's <0> is taken by Enter.
            return rotate(state_.angle.value_or(0.0));
        case Step::AngleSecond:
            return ToolStep::rejected("click the second point on the dimension line");
        }
        return InteractiveTool::enter();
    }

    ToolStep undo() override
    {
        switch (step_) {
        case Step::First:
            return ToolStep::rejected("nothing to undo: no origin has been given");
        case Step::Object:
        case Step::Second:
            step_ = Step::First;
            return ToolStep::next();
        case Step::Location:
            if (!history_.empty()) {
                state_ = history_.back();
                history_.pop_back();
            } else {
                step_ = picked_ ? Step::Object : Step::Second;
            }
            return ToolStep::next();
        case Step::Text:
        case Step::Angle:
            step_ = Step::Location;
            return ToolStep::next();
        case Step::AngleSecond:
            step_ = Step::Angle;
            return ToolStep::next();
        }
        return InteractiveTool::undo();
    }

    [[nodiscard]] ToolFeedback preview(const Point2& cursor) const override
    {
        ToolFeedback feedback;
        switch (step_) {
        case Step::First:
        case Step::Object:
            break;
        case Step::Second:
            feedback.markers.push_back(first_);
            if (!coincident(first_, cursor)) {
                feedback.shapes.emplace_back(Segment2{first_, cursor});
            }
            break;
        case Step::Location:
        case Step::Text:
        case Step::Angle:
            feedback.markers.push_back(first_);
            feedback.markers.push_back(second_);
            // Where the cursor cannot place a dimension (inside the box of a
            // linear dimension's origins), only the origins are marked:
            // drawing a dimension there would promise one the click will
            // refuse.
            if (step_ != Step::Angle) {
                if (auto dimension = place(cursor)) {
                    feedback.shapes.emplace_back(*dimension);
                }
            }
            break;
        case Step::AngleSecond:
            feedback.markers.push_back(angleFrom_);
            if (!coincident(angleFrom_, cursor)) {
                feedback.shapes.emplace_back(Segment2{angleFrom_, cursor});
            }
            break;
        }
        return feedback;
    }

    [[nodiscard]] std::optional<Point2> lastPoint() const override
    {
        switch (step_) {
        case Step::Second:
            return first_;
        case Step::Location:
        case Step::Text:
        case Step::Angle:
            return second_;
        case Step::AngleSecond:
            return angleFrom_;
        case Step::First:
        case Step::Object:
            break;
        }
        return std::nullopt;
    }

  private:
    enum class Step { First, Object, Second, Location, Text, Angle, AngleSecond };

    [[nodiscard]] static std::optional<Segment2> nearestSegment(const Polyline2& polyline,
                                                                const Point2& at)
    {
        std::optional<Segment2> nearest;
        double best = std::numeric_limits<double>::infinity();
        for (std::size_t i = 0; i < polyline.segmentCount(); ++i) {
            const Segment2 segment = polyline.segment(i);
            const double distance = segment.distanceTo(at);
            if (distance < best) {
                best = distance;
                nearest = segment;
            }
        }
        return nearest;
    }

    void remember() { history_.push_back(state_); }

    [[nodiscard]] ann::AnchoredPoint firstOrigin() const { return {first_, firstRef_}; }
    [[nodiscard]] ann::AnchoredPoint secondOrigin() const { return {second_, secondRef_}; }
    [[nodiscard]] bool anchored() const
    {
        return firstRef_.associated() || secondRef_.associated();
    }

    // Rotated: the direction is `angle`, which H and V give up.
    ToolStep rotate(double angle)
    {
        remember();
        state_.angle = angle;
        state_.orientation.reset();
        step_ = Step::Location;
        return ToolStep::next();
    }

    ToolStep locationValue(std::string_view text)
    {
        if (isOption(text, "Undo")) {
            return undo();
        }
        if (isOption(text, "Text")) {
            step_ = Step::Text;
            return ToolStep::next();
        }
        if (kind_ == Kind::Linear) {
            if (isOption(text, "Horizontal") || isOption(text, "Vertical")) {
                remember();
                state_.orientation = isOption(text, "Horizontal") ? Orientation::Horizontal
                                                                   : Orientation::Vertical;
                state_.angle.reset();
                return ToolStep::next();
            }
            if (isOption(text, "Rotated")) {
                step_ = Step::Angle;
                return ToolStep::next();
            }
            return ToolStep::rejected("place the dimension line with a point, or type T, H, V "
                                      "or R");
        }
        // Aligned: a typed number is the offset itself, for a dimension line
        // at an exact distance - the interpreter's DIM takes the same.
        const auto offset = typedNumber(text);
        if (!offset) {
            return ToolStep::rejected("place the dimension line with a point, type its offset "
                                      "(positive to the left of first to second), or type T");
        }
        DimensionGeometry dimension{first_, second_, *offset, state_.textOverride};
        dimension.startRef = firstRef_;
        dimension.endRef = secondRef_;
        return finish(dimension);
    }

    [[nodiscard]] Result<DimensionGeometry> place(const Point2& location) const
    {
        Result<DimensionGeometry> made =
            kind_ == Kind::Aligned ? ann::alignedDimension(firstOrigin(), secondOrigin(), location)
                                   : placeLinear(location);
        if (made) {
            made->textOverride = state_.textOverride;
        }
        return made;
    }

    [[nodiscard]] Result<DimensionGeometry> placeLinear(const Point2& location) const
    {
        if (state_.angle) {
            return ann::linearDimension(firstOrigin(), secondOrigin(), location, *state_.angle);
        }
        // Whether each orientation measures anything: level origins have no
        // vertical distance, one above the other no horizontal one.
        const bool measuresAcross = std::abs(second_.x - first_.x) > tol::kGeometric;
        const bool measuresUp = std::abs(second_.y - first_.y) > tol::kGeometric;

        Orientation orientation = Orientation::Horizontal;
        if (state_.orientation) {
            orientation = *state_.orientation;
        } else {
            // As DIMLINEAR decides: a line placed above or below the origins
            // measures across (horizontal), one beside them measures up
            // (vertical). Off a corner, the direction the cursor is farther
            // out wins; an exact diagonal is horizontal, so the choice is
            // never left to rounding.
            const double outX = std::max({0.0, std::min(first_.x, second_.x) - location.x,
                                          location.x - std::max(first_.x, second_.x)});
            const double outY = std::max({0.0, std::min(first_.y, second_.y) - location.y,
                                          location.y - std::max(first_.y, second_.y)});
            if (outX == 0.0 && outY == 0.0) {
                return makeError(ErrorCode::InvalidArgument,
                                 "the dimension line is within the rectangle the two origins "
                                 "span, so it could be horizontal or vertical; place it above, "
                                 "below or beside them, or type H or V");
            }
            orientation = outY >= outX ? Orientation::Horizontal : Orientation::Vertical;
            // Off the corner of a level pair - past the end of a horizontal
            // edge, the commonest thing dimensioned - "farther out" can pick
            // the orientation that measures nothing although the line is
            // also above or below the pair. Unless the user forced one, the
            // orientation that does measure wins wherever the line is
            // outside the origins on its side; with neither, the refusal
            // below says why.
            if (orientation == Orientation::Vertical && !measuresUp && measuresAcross &&
                outY > 0.0) {
                orientation = Orientation::Horizontal;
            } else if (orientation == Orientation::Horizontal && !measuresAcross && measuresUp &&
                       outX > 0.0) {
                orientation = Orientation::Vertical;
            }
        }
        const bool horizontal = orientation == Orientation::Horizontal;

        // `across` is the coordinate the dimension line is placed at.
        const auto across = [&](const Point2& p) { return horizontal ? p.y : p.x; };

        if (!(horizontal ? measuresAcross : measuresUp)) {
            return makeError(ErrorCode::InvalidArgument,
                             horizontal ? "the two origins are one above the other, so a "
                                          "horizontal dimension would measure nothing; place the "
                                          "line beside them or type V"
                                        : "the two origins are level, so a vertical dimension "
                                          "would measure nothing; place the line above or below "
                                          "them or type H");
        }
        const double firstOut = across(first_) - across(location);
        const double secondOut = across(second_) - across(location);
        const bool between = (firstOut > tol::kGeometric && secondOut < -tol::kGeometric) ||
                             (firstOut < -tol::kGeometric && secondOut > tol::kGeometric);
        // What the projection cannot be (the top of the file): a line whose
        // extension lines leave in opposite directions, or origins that
        // follow an entity.
        if (between || anchored()) {
            return ann::linearDimension(firstOrigin(), secondOrigin(), location,
                                        horizontal ? 0.0 : 0.5 * katana::math::kPi);
        }
        // The farther origin's level (see the top of the file). A tie keeps
        // the first origin's, so the choice is never left to rounding.
        const double base =
            std::abs(firstOut) >= std::abs(secondOut) ? across(first_) : across(second_);
        const Point2 start = horizontal ? Point2(first_.x, base) : Point2(base, first_.y);
        const Point2 end = horizontal ? Point2(second_.x, base) : Point2(base, second_.y);
        const Vec2 normal = (end - start).normalized().perpendicular();
        return DimensionGeometry{start, end, (location - start).dot(normal), state_.textOverride};
    }

    ToolStep finish(const DimensionGeometry& dimension)
    {
        std::string message = std::string(kind_ == Kind::Aligned ? "aligned" : "linear") +
                              " dimension measuring " + formatNumber(dimension.measurement());
        return ToolStep::done(katana::commands::createDimension(dimension, attributes_),
                              std::move(message), /*restart=*/true);
    }

    const Document* document_ = nullptr;
    katana::commands::EntityAttributes attributes_;
    Kind kind_;

    Step step_ = Step::First;
    Point2 first_;
    Point2 second_;
    // The entity points the origins were snapped to; unassociated when they
    // were clicked, typed or taken from a picked line.
    AnchorRef firstRef_{};
    AnchorRef secondRef_{};
    // Set only while anchoredPoint hands a snapped point to point().
    AnchorRef pending_{};
    // Rotated's first point, when the angle is given by two.
    Point2 angleFrom_;
    // The origins came from a picked line, so Undo at the dimension line goes
    // back to picking rather than to a second-origin prompt never shown.
    bool picked_ = false;
    LocationState state_;
    std::vector<LocationState> history_;
};

} // namespace

std::unique_ptr<InteractiveTool> makeAlignedDimensionTool(const ToolContext& context)
{
    return std::make_unique<DimensionTool>(context, Kind::Aligned);
}

std::unique_ptr<InteractiveTool> makeLinearDimensionTool(const ToolContext& context)
{
    return std::make_unique<DimensionTool>(context, Kind::Linear);
}

} // namespace katana::cad::tools::annotate
