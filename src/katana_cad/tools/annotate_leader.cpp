// Leader: an arrow line to a feature with a note at its end, as AutoCAD's
// LEADER draws one.
//
//   Specify leader start point                          the arrow's tip
//   Specify next point or [Undo]                        the first bend or end
//   Specify next point or [Annotation/Undo] <Annotation>  more bends, or Enter
//   Enter the first line of annotation text             typed, line by line;
//                                                       an empty line finishes
//
// The model has no leader entity, so a leader is what it would be drawn as:
// the line as a POLYLINE, the arrowhead as its own entity, and each line of
// the note as a TEXT - added by ONE command, so one undo removes all of it.
// They are not grouped: moving the line afterwards leaves the text where it
// was, as it would with an AutoCAD leader exploded.
//
// Sizes come from the dimension style the current layer resolves to, as an
// AutoCAD leader takes DIMASZ, DIMTXT and DIMGAP from its dimension style:
// the arrowhead is the style's head at arrowSize, the note is textHeight tall
// and stands textGap off the end of the line. The heads are those
// dimension_draw.cpp draws, turned into entities: ClosedFilled becomes a
// CLOSED triangle - an outline, because the model cannot fill a polyline -
// Open two strokes, Tick one, Dot a circle, None nothing.
//
// The note follows the last segment: to the right of the end when the line
// arrives heading right (or straight up or down, within the geometric
// tolerance), to the left when it arrives heading left, its first line
// centred on the end. When the last segment is
// more than 15 degrees off horizontal a horizontal hook line one arrowhead
// long is added first, as AutoCAD does, so the note never hangs off a slope.
// Left of the end, each line is right-aligned by its ESTIMATED width
// (annotate_common.hpp): the model has no font metrics, so the right edge is
// where a 0.6-aspect font would put it.

#include <cmath>
#include <optional>
#include <string>
#include <vector>

#include "annotate_common.hpp"
#include "katana/entity/entity.hpp"
#include "katana/math/numerics.hpp"

namespace katana::cad::tools::annotate {

namespace {

using katana::entity::ArrowHead;
using katana::entity::DimensionStyle;
using katana::entity::Geometry;
using katana::entity::TextGeometry;
using katana::geometry::Circle2;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;
using katana::geometry::Vec2;
namespace tol = katana::math::tolerance;

// A closed head is three times as long as it is wide, the proportion
// dimension_draw.cpp draws (kArrowWidthFraction there), so a leader's arrow
// and a dimension's match.
constexpr double kArrowWidthFraction = 1.0 / 3.0;
constexpr double kHookAngle = 15.0 * katana::math::kDegToRad;

// The arrowhead at `tip`, pointing along `along` (a unit vector away from the
// line), as dimension_draw.cpp's appendArrow shapes it.
std::optional<Geometry> arrowhead(const Point2& tip, const Vec2& along, const DimensionStyle& style)
{
    const double size = style.arrowSize;
    if (!(size > tol::kGeometric)) {
        return std::nullopt;
    }
    const Vec2 normal = along.perpendicular();
    const Point2 back = tip - along * size;
    const Vec2 half = normal * (size * kArrowWidthFraction);
    switch (style.arrowHead) {
    case ArrowHead::None:
        return std::nullopt;
    case ArrowHead::Tick: {
        const Vec2 direction = (along + normal).normalized();
        return Geometry{Segment2{tip - direction * (size * 0.5), tip + direction * (size * 0.5)}};
    }
    case ArrowHead::Open:
        return Geometry{Polyline2{{back + half, tip, back - half}, false}};
    case ArrowHead::ClosedFilled:
        return Geometry{Polyline2{{tip, back + half, back - half}, true}};
    case ArrowHead::Dot:
        return Geometry{Circle2{tip, size * 0.25}};
    }
    return std::nullopt;
}

// Everything a leader through `vertices` with note `lines` is drawn as: the
// line first, then the arrowhead, then the text. `vertices` has at least two
// points, none coincident with the next.
std::vector<Geometry> leaderShapes(std::vector<Point2> vertices,
                                   const std::vector<std::string>& lines,
                                   const DimensionStyle& style)
{
    std::vector<Geometry> shapes;
    const Vec2 last = vertices.back() - vertices[vertices.size() - 2];
    // Straight up or down goes right, and "straight" is within the geometric
    // tolerance: a segment typed as @10<270 ends 1.8e-15 left of vertical,
    // because cos(3 pi / 2) is not 0 in binary, and must not send the note to
    // the other side from the same segment typed as @10<-90 or @0,-10.
    const bool rightward = !(last.x < -tol::kGeometric);
    if (!lines.empty() && style.arrowSize > tol::kGeometric &&
        std::atan2(std::abs(last.y), std::abs(last.x)) > kHookAngle) {
        vertices.push_back(vertices.back() + Vec2(rightward ? style.arrowSize : -style.arrowSize, 0.0));
    }
    const Point2 landing = vertices.back();
    const Point2 tip = vertices.front();
    const Vec2 along = (tip - vertices[1]).normalized();
    shapes.emplace_back(Polyline2{std::move(vertices), false});
    if (auto head = arrowhead(tip, along, style)) {
        shapes.push_back(std::move(*head));
    }

    const double height = style.textHeight;
    // The first line's middle is level with the end of the line, as a note
    // sits on a leader's landing.
    double baseline = landing.y - height * 0.5;
    for (const std::string& line : lines) {
        const double x = rightward ? landing.x + style.textGap
                                   : landing.x - style.textGap - estimatedTextWidth(line, height);
        shapes.emplace_back(TextGeometry{Point2(x, baseline), line, height, 0.0});
        baseline -= lineSpacing(height);
    }
    return shapes;
}

class LeaderTool final : public InteractiveTool {
  public:
    explicit LeaderTool(const ToolContext& context)
        : attributes_(context.attributes), style_(styleForNewAnnotation(context))
    {
    }

    [[nodiscard]] std::string prompt() const override
    {
        switch (step_) {
        case Step::Tip:
            return "Specify leader start point";
        case Step::Vertices:
            return vertices_.size() < 2 ? "Specify next point or [Undo]"
                                        : "Specify next point or [Annotation/Undo] <Annotation>";
        case Step::Annotation:
            return lines_.empty()
                       ? "Enter the first line of annotation text, or press Enter for none"
                       : "Enter the next line of annotation text, or press Enter to finish";
        }
        return {};
    }

    [[nodiscard]] ToolInput expects() const override
    {
        return step_ == Step::Annotation ? ToolInput::Value : ToolInput::Point;
    }

    ToolStep point(const Point2& at) override
    {
        switch (step_) {
        case Step::Tip:
            vertices_ = {at};
            step_ = Step::Vertices;
            return ToolStep::next();
        case Step::Vertices:
            if (coincident(vertices_.back(), at)) {
                return ToolStep::rejected(
                    "that point is on the previous one; each leader segment needs a length");
            }
            vertices_.push_back(at);
            return ToolStep::next();
        case Step::Annotation:
            break;
        }
        return InteractiveTool::point(at);
    }

    ToolStep value(std::string_view text) override
    {
        switch (step_) {
        case Step::Tip:
            if (isOption(text, "Undo")) {
                return undo();
            }
            return ToolStep::rejected("click the point the arrow touches, or type it as x,y");
        case Step::Vertices:
            if (isOption(text, "Undo")) {
                return undo();
            }
            if (isOption(text, "Annotation")) {
                return enter();
            }
            return ToolStep::rejected("click the next point, or type A or press Enter for the "
                                      "annotation");
        case Step::Annotation:
            if (text.empty()) {
                return enter();
            }
            if (!katana::entity::isValidUtf8(text)) {
                return ToolStep::rejected("the text is not valid UTF-8");
            }
            lines_.emplace_back(text);
            return ToolStep::next();
        }
        return InteractiveTool::value(text);
    }

    ToolStep enter() override
    {
        switch (step_) {
        case Step::Tip:
            return ToolStep::done(nullptr);
        case Step::Vertices:
            if (vertices_.size() < 2) {
                return ToolStep::rejected("a leader needs a second point");
            }
            step_ = Step::Annotation;
            return ToolStep::next();
        case Step::Annotation:
            break;
        }
        std::vector<katana::entity::Entity> entities;
        for (Geometry& shape : leaderShapes(vertices_, lines_, style_)) {
            entities.push_back(newEntity(std::move(shape), attributes_));
        }
        const std::size_t count = lines_.size();
        std::string message = count == 0   ? "leader with no text"
                              : count == 1 ? "leader with 1 line of text"
                                           : "leader with " + std::to_string(count) +
                                                 " lines of text";
        return ToolStep::done(createAll("CREATE_LEADER", std::move(entities)), std::move(message),
                              /*restart=*/true);
    }

    ToolStep undo() override
    {
        switch (step_) {
        case Step::Tip:
            return ToolStep::rejected("nothing to undo: no point has been given");
        case Step::Vertices:
            vertices_.pop_back();
            if (vertices_.empty()) {
                step_ = Step::Tip;
            }
            return ToolStep::next();
        case Step::Annotation:
            if (!lines_.empty()) {
                lines_.pop_back();
            } else {
                step_ = Step::Vertices;
            }
            return ToolStep::next();
        }
        return InteractiveTool::undo();
    }

    [[nodiscard]] ToolFeedback preview(const Point2& cursor) const override
    {
        ToolFeedback feedback;
        switch (step_) {
        case Step::Tip:
            break;
        case Step::Vertices: {
            // The leader as it would be with the cursor as its next point,
            // arrowhead included, so its size is seen before it is placed.
            std::vector<Point2> vertices = vertices_;
            if (!coincident(vertices.back(), cursor)) {
                vertices.push_back(cursor);
            }
            if (vertices.size() >= 2) {
                feedback.shapes = leaderShapes(std::move(vertices), {}, style_);
            }
            feedback.markers.push_back(vertices_.back());
            break;
        }
        case Step::Annotation:
            feedback.shapes = leaderShapes(vertices_, lines_, style_);
            break;
        }
        return feedback;
    }

    [[nodiscard]] std::optional<Point2> lastPoint() const override
    {
        if (vertices_.empty()) {
            return std::nullopt;
        }
        return vertices_.back();
    }

  private:
    enum class Step { Tip, Vertices, Annotation };

    katana::commands::EntityAttributes attributes_;
    DimensionStyle style_;

    Step step_ = Step::Tip;
    std::vector<Point2> vertices_;
    std::vector<std::string> lines_;
};

} // namespace

std::unique_ptr<InteractiveTool> makeLeaderTool(const ToolContext& context)
{
    return std::make_unique<LeaderTool>(context);
}

} // namespace katana::cad::tools::annotate
