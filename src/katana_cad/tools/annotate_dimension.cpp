// Aligned and linear dimensions, as AutoCAD's DIMALIGNED and DIMLINEAR take
// them:
//
//   Specify first extension line origin or <select object>
//                                        a point, or Enter to pick a line or a
//                                        polyline segment instead
//   Specify second extension line origin or [Undo]
//   Specify dimension line location ...  a point; T gives the text; Linear
//                                        also takes H or V
//
// BOTH TOOLS MAKE AN ALIGNED DIMENSION: a DimensionGeometry of the Aligned
// kind, measuring |end - start|, its dimension line parallel to start->end
// and displaced by `offset` to its LEFT (negative: to its right). The model
// has had a Linear kind since 2026-09-25 (DimensionKind; DIM LINEAR makes
// one), but Linear here stays the projection below: an aligned dimension is
// what every drawing made before then holds and what the DXF writer exports,
// and the projection draws the same picture.
//
// Aligned maps onto it directly: start and end are the two origins, and the
// offset is the signed distance of the picked location from the line through
// them, positive when it is to the left of first->second.
//
// Linear is represented by PROJECTING the origins onto the chosen axis. A
// horizontal dimension of (x1, y1) and (x2, y2) placed at height yd is stored
// as start (x1, yb), end (x2, yb) - so it measures |x2 - x1| - with yb the
// level of whichever origin is FARTHER from the dimension line. That choice
// decides the extension lines, because the model draws both from its start
// and end towards the dimension line, each starting DIMEXO (extensionOffset)
// clear of its point:
//
//   * the farther origin's extension line is exactly AutoCAD's, with the
//     origin in the DIMEXO gap at its foot;
//   * the nearer origin's starts DIMEXO beyond the farther one's level. When
//     the two levels differ by more than DIMEXO it runs THROUGH its origin
//     and on past it, away from the dimension line, by the difference less
//     DIMEXO; when they differ by less, it starts beyond its origin, which
//     sits in a gap of DIMEXO less the difference - a smaller gap than the
//     farther origin's.
//
// Every origin therefore lies on the line of its own extension line, either
// on the stroke or in the gap at its foot, which is how a reader sees what
// was measured; projecting onto the nearer level instead would leave the
// farther origin short of its line by the whole difference and DIMEXO,
// pointing at nothing. The one placement the projection cannot draw at all
// is a dimension line BETWEEN the two levels, whose extension lines would
// have to leave in opposite directions: that is refused with a sentence
// rather than drawn wrongly (a Linear-kind dimension, DIM LINEAR, draws it).
//
// Angular, radius, diameter and ordinate dimensions are in
// annotate_dimension_kinds.cpp.

#include <algorithm>
#include <cmath>
#include <limits>
#include <optional>
#include <string>
#include <variant>
#include <vector>

#include "annotate_common.hpp"
#include "katana/core/error.hpp"
#include "katana/entity/entity.hpp"
#include "katana/math/numerics.hpp"

namespace katana::cad::tools::annotate {

namespace {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
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
            // Once H or V is typed the prompt says so, since nothing else on
            // screen shows that the cursor no longer chooses.
            if (state_.orientation) {
                return std::string("Specify location of the ") +
                       (*state_.orientation == Orientation::Horizontal ? "horizontal"
                                                                       : "vertical") +
                       " dimension line or [Text/Horizontal/Vertical/Undo]";
            }
            return "Specify dimension line location or [Text/Horizontal/Vertical/Undo]";
        case Step::Text:
            return "Enter dimension text, or press Enter for the measured distance";
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
            break;
        }
        return ToolInput::Point;
    }

    ToolStep point(const Point2& at) override
    {
        switch (step_) {
        case Step::First:
            first_ = at;
            step_ = Step::Second;
            return ToolStep::next();
        case Step::Second:
            if (coincident(first_, at)) {
                return ToolStep::rejected(
                    "the second origin is on the first; a dimension needs two distinct points");
            }
            second_ = at;
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
        case Step::Object:
        case Step::Text:
            break;
        }
        return InteractiveTool::point(at);
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
            step_ = Step::Location;
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
            feedback.markers.push_back(first_);
            feedback.markers.push_back(second_);
            // Where the cursor cannot place a dimension (between a linear
            // dimension's levels), only the origins are marked: drawing a
            // dimension there would promise one the click will refuse.
            if (auto dimension = place(cursor)) {
                feedback.shapes.emplace_back(*dimension);
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
            return second_;
        case Step::First:
        case Step::Object:
            break;
        }
        return std::nullopt;
    }

  private:
    enum class Step { First, Object, Second, Location, Text };

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
                return ToolStep::next();
            }
            return ToolStep::rejected("place the dimension line with a point, or type T, H or V");
        }
        // Aligned: a typed number is the offset itself, for a dimension line
        // at an exact distance - the interpreter's DIM takes the same.
        const auto offset = typedNumber(text);
        if (!offset) {
            return ToolStep::rejected("place the dimension line with a point, type its offset "
                                      "(positive to the left of first to second), or type T");
        }
        return finish(DimensionGeometry{first_, second_, *offset, state_.textOverride});
    }

    [[nodiscard]] Result<DimensionGeometry> place(const Point2& location) const
    {
        if (kind_ == Kind::Aligned) {
            // Left of first->second is positive, as DimensionGeometry defines.
            const Vec2 normal = (second_ - first_).normalized().perpendicular();
            return DimensionGeometry{first_, second_, (location - first_).dot(normal),
                                     state_.textOverride};
        }
        return placeLinear(location);
    }

    [[nodiscard]] Result<DimensionGeometry> placeLinear(const Point2& location) const
    {
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
        if ((firstOut > tol::kGeometric && secondOut < -tol::kGeometric) ||
            (firstOut < -tol::kGeometric && secondOut > tol::kGeometric)) {
            return makeError(ErrorCode::InvalidArgument,
                             horizontal ? "the dimension line is between the two origins' "
                                          "levels; a dimension here draws both extension lines "
                                          "from one side, so place it above or below both"
                                        : "the dimension line is between the two origins; a "
                                          "dimension here draws both extension lines from one "
                                          "side, so place it to the left or right of both");
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
