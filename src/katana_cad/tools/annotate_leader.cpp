// Leader: an arrow line to a feature with a note at its end, as AutoCAD's
// LEADER draws one.
//
//   Specify leader start point                          the arrow's tip
//   Specify next point or [Undo]                        the first bend or end
//   Specify next point or [Annotation/Undo] <Annotation>  more bends, or Enter
//   Enter the first line of annotation text             typed, line by line;
//                                                       an empty line finishes
//
// The leader is ONE entity, a LeaderGeometry (docs/annotation.md,
// "Leaders and callouts"), added by one command: the arrow, the line, the
// landing and the note are drawn from it at the scale of the view or sheet
// looking at it, and move, copy and erase together. It is the LEADER verb's
// entity; the command line adds what the tool does not ask for - a boxed or
// circled callout, a text style, a tip that follows the entity it points at.
// (Before annotation had its own entities the tool drew a polyline, an
// arrowhead and a text per line, which drifted apart when one was moved.)
//
// Sizes come from the dimension style the current layer resolves to, as an
// AutoCAD leader takes DIMASZ and DIMTXT from its dimension style: the arrow
// is the style's head at arrowSize and the note textHeight tall. A leader's
// sizes are paper millimetres; a paper-sized style's are taken as they are,
// and a model-unit style's are what its model size is on paper at the
// document's annotation scale - the leader is made the size it would have
// been drawn, and from then on keeps its size on paper as the scale changes.
//
// The note follows the last segment (cad/annotation/leader_draw.cpp): to the
// right of the end when the line arrives heading right, or straight up or
// down within the geometric tolerance, to the left when it arrives heading
// left. When the last segment is more than 15 degrees off horizontal the
// leader gets a horizontal landing one arrowhead long first, as AutoCAD adds
// a hook, so the note never hangs off a slope; a leader within 15 degrees of
// level, or with no note, has none.

#include <cmath>
#include <optional>
#include <string>
#include <vector>

#include "annotate_common.hpp"
#include "katana/cad/document.hpp"
#include "katana/entity/annotation.hpp"
#include "katana/entity/entity.hpp"
#include "katana/math/numerics.hpp"

namespace katana::cad::tools::annotate {

namespace {

using katana::entity::DimensionStyle;
using katana::entity::LeaderGeometry;
using katana::geometry::Point2;
using katana::geometry::Vec2;
namespace tol = katana::math::tolerance;

constexpr double kHookAngle = 15.0 * katana::math::kDegToRad;

// The leader through `vertices` with note `lines`, sized from `style` at
// 1 : `scale`. `vertices` has at least two points, none coincident with the
// next.
LeaderGeometry leaderFor(std::vector<Point2> vertices, const std::vector<std::string>& lines,
                         const DimensionStyle& style, double scale)
{
    const auto paper = [&](double size) {
        return style.paperSized ? size : katana::entity::annotationPaperSize(size, scale);
    };
    LeaderGeometry leader;
    const Vec2 last = vertices.back() - vertices[vertices.size() - 2];
    const bool steep = std::atan2(std::abs(last.y), std::abs(last.x)) > kHookAngle;
    leader.vertices = std::move(vertices);
    for (const std::string& line : lines) {
        leader.text += leader.text.empty() ? line : "\n" + line;
    }
    leader.arrow = style.arrowHead;
    leader.arrowSize = paper(style.arrowSize);
    leader.paperHeight = paper(style.textHeight);
    leader.landing = !lines.empty() && steep ? leader.arrowSize : 0.0;
    return leader;
}

class LeaderTool final : public InteractiveTool {
  public:
    explicit LeaderTool(const ToolContext& context)
        : attributes_(context.attributes), style_(styleForNewAnnotation(context)),
          scale_(context.document != nullptr ? context.document->annotationScale()
                                             : katana::entity::kDefaultAnnotationScale)
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
        entities.push_back(newEntity(leaderFor(vertices_, lines_, style_, scale_), attributes_));
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
                feedback.shapes.emplace_back(leaderFor(std::move(vertices), {}, style_, scale_));
            }
            feedback.markers.push_back(vertices_.back());
            break;
        }
        case Step::Annotation:
            feedback.shapes.emplace_back(leaderFor(vertices_, lines_, style_, scale_));
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
    double scale_;

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
