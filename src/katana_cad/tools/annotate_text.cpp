// Text: single-line text, line after line, as AutoCAD's DTEXT places it.
//
//   Specify start point of text                  a point
//   Specify height or [Undo] <2.5>               a number, a point (its distance
//                                                from the start), or Enter
//   Specify rotation angle of text or [Undo] <0> degrees counter-clockwise from
//                                                east, a point (the direction to
//                                                it), or Enter
//   Enter text                                   typed; each line goes below
//                                                the last; an empty line ends
//
// All the lines of one use are ONE command, so one undo removes the whole
// text - as one DTEXT command undoes as one in AutoCAD.
//
// The defaults come from the drawing: the newest text in it gives the height
// and rotation offered, and Enter at the first prompt continues directly
// below it (DTEXT's own shortcut). Reading them from the drawing rather than
// from memory in the tool means they follow the drawing across sessions and
// through undo, as TEXTSIZE does in a DWG, and a test sees the same default
// whatever ran before it. With no text in the drawing the height is 2.5, the
// metric default of AutoCAD's TEXTSIZE and of this model's TextGeometry.

#include <optional>
#include <string>
#include <variant>
#include <vector>

#include "annotate_common.hpp"
#include "katana/entity/entity.hpp"
#include "katana/math/numerics.hpp"

namespace katana::cad::tools::annotate {

namespace {

using katana::entity::TextGeometry;
using katana::geometry::Point2;
using katana::geometry::Segment2;
using katana::geometry::Vec2;

constexpr double kDefaultHeight = 2.5;

struct TypedLine {
    Point2 position;
    std::string text;
};

class TextTool final : public InteractiveTool {
  public:
    explicit TextTool(const ToolContext& context) : attributes_(context.attributes)
    {
        if (context.document != nullptr) {
            // Ascending id order, so the last one seen is the newest.
            context.document->model().entities.forEach([&](const katana::entity::Entity& entity) {
                if (const auto* text = std::get_if<TextGeometry>(&entity.geometry)) {
                    newest_ = *text;
                }
            });
        }
        if (newest_) {
            defaultHeight_ = newest_->height;
            defaultRotation_ = newest_->rotation;
        }
    }

    [[nodiscard]] std::string prompt() const override
    {
        switch (step_) {
        case Step::Start:
            return newest_ ? "Specify start point of text, or press Enter to continue below the "
                             "last text"
                           : "Specify start point of text";
        case Step::Height:
            return "Specify height or [Undo] <" + formatNumber(defaultHeight_) + ">";
        case Step::Rotation:
            return "Specify rotation angle of text or [Undo] <" +
                   formatNumber(defaultRotation_ * katana::math::kRadToDeg) + ">";
        case Step::Content:
            return lines_.empty() ? "Enter text"
                                  : "Enter the next line of text, or press Enter to finish";
        }
        return {};
    }

    [[nodiscard]] ToolInput expects() const override
    {
        // The text itself is a Value so that "Road, north side" stays text
        // rather than being read as a malformed point.
        return step_ == Step::Content ? ToolInput::Value : ToolInput::Point;
    }

    ToolStep point(const Point2& at) override
    {
        switch (step_) {
        case Step::Start:
            start_ = at;
            step_ = Step::Height;
            return ToolStep::next();
        case Step::Height: {
            // A picked height is the distance from the start point, as in
            // AutoCAD: the user shows how tall the letters are.
            if (coincident(start_, at)) {
                return ToolStep::rejected(
                    "that point is on the start point, which gives a height of zero");
            }
            height_ = start_.distanceTo(at);
            step_ = Step::Rotation;
            return ToolStep::next();
        }
        case Step::Rotation:
            if (coincident(start_, at)) {
                return ToolStep::rejected(
                    "that point is on the start point, so it gives no direction for the text");
            }
            rotation_ = (at - start_).angle();
            step_ = Step::Content;
            return ToolStep::next();
        case Step::Content:
            break;
        }
        return InteractiveTool::point(at);
    }

    ToolStep value(std::string_view text) override
    {
        switch (step_) {
        case Step::Start:
            if (isOption(text, "Undo")) {
                return undo();
            }
            return ToolStep::rejected("'" + std::string(text) +
                                      "' is not a point; click the start point or type it as x,y");
        case Step::Height: {
            if (isOption(text, "Undo")) {
                return undo();
            }
            const auto height = typedNumber(text);
            if (!height) {
                return ToolStep::rejected("the height must be a number, such as 2.5");
            }
            if (!(*height > katana::math::tolerance::kGeometric)) {
                return ToolStep::rejected("a text height must be greater than zero");
            }
            height_ = *height;
            step_ = Step::Rotation;
            return ToolStep::next();
        }
        case Step::Rotation: {
            if (isOption(text, "Undo")) {
                return undo();
            }
            const auto degrees = typedNumber(text);
            if (!degrees) {
                return ToolStep::rejected(
                    "the rotation must be an angle in degrees, counter-clockwise from east");
            }
            rotation_ = *degrees * katana::math::kDegToRad;
            step_ = Step::Content;
            return ToolStep::next();
        }
        case Step::Content:
            // The router trims, so a line of blanks arrives empty and ends the
            // text as an empty line does.
            if (text.empty()) {
                return enter();
            }
            if (!katana::entity::isValidUtf8(text)) {
                // Refused here, where the user can retype it, rather than by
                // the model when the whole text is committed.
                return ToolStep::rejected("the text is not valid UTF-8");
            }
            lines_.push_back({nextPosition(), std::string(text)});
            return ToolStep::next();
        }
        return InteractiveTool::value(text);
    }

    ToolStep enter() override
    {
        switch (step_) {
        case Step::Start:
            if (!newest_) {
                return ToolStep::done(nullptr);
            }
            // DTEXT's Enter at the first prompt: carry on below the newest
            // text, in its height and rotation, without asking again.
            height_ = newest_->height;
            rotation_ = newest_->rotation;
            start_ = newest_->position + below(rotation_) * lineSpacing(height_);
            continued_ = true;
            step_ = Step::Content;
            return ToolStep::next();
        case Step::Height:
            height_ = defaultHeight_;
            step_ = Step::Rotation;
            return ToolStep::next();
        case Step::Rotation:
            rotation_ = defaultRotation_;
            step_ = Step::Content;
            return ToolStep::next();
        case Step::Content:
            break;
        }
        if (lines_.empty()) {
            // Enter with nothing typed ends the tool, so a second Enter is
            // how a user leaves Text without reaching for Esc.
            return ToolStep::done(nullptr);
        }
        std::vector<katana::entity::Entity> entities;
        entities.reserve(lines_.size());
        for (const TypedLine& line : lines_) {
            entities.push_back(newEntity(TextGeometry{line.position, line.text, height_, rotation_},
                                         attributes_));
        }
        const std::size_t count = lines_.size();
        std::string message =
            count == 1 ? "1 line of text" : std::to_string(count) + " lines of text";
        return ToolStep::done(createAll("CREATE_TEXT", std::move(entities)), std::move(message),
                              /*restart=*/true);
    }

    ToolStep undo() override
    {
        switch (step_) {
        case Step::Start:
            return ToolStep::rejected("nothing to undo: no start point has been given");
        case Step::Height:
            step_ = Step::Start;
            return ToolStep::next();
        case Step::Rotation:
            step_ = Step::Height;
            return ToolStep::next();
        case Step::Content:
            if (!lines_.empty()) {
                lines_.pop_back();
            } else if (continued_) {
                continued_ = false;
                step_ = Step::Start;
            } else {
                step_ = Step::Rotation;
            }
            return ToolStep::next();
        }
        return InteractiveTool::undo();
    }

    [[nodiscard]] ToolFeedback preview(const Point2& cursor) const override
    {
        ToolFeedback feedback;
        switch (step_) {
        case Step::Start:
            break;
        case Step::Height:
        case Step::Rotation:
            // The rubber band the height or the direction is measured along.
            feedback.markers.push_back(start_);
            if (!coincident(start_, cursor)) {
                feedback.shapes.emplace_back(Segment2{start_, cursor});
            }
            break;
        case Step::Content:
            // The lines typed so far, which are not in the drawing until the
            // text is finished, and where the next one will start.
            for (const TypedLine& line : lines_) {
                feedback.shapes.emplace_back(
                    TextGeometry{line.position, line.text, height_, rotation_});
            }
            feedback.markers.push_back(nextPosition());
            break;
        }
        return feedback;
    }

    [[nodiscard]] std::optional<Point2> lastPoint() const override
    {
        if (step_ == Step::Start) {
            return std::nullopt;
        }
        return start_;
    }

  private:
    enum class Step { Start, Height, Rotation, Content };

    // Down the page in the text's own frame: a rotated text's next line is
    // below it as the text reads, not below it on the drawing.
    [[nodiscard]] static Vec2 below(double rotation) { return Vec2(0.0, -1.0).rotated(rotation); }

    [[nodiscard]] Point2 nextPosition() const
    {
        if (lines_.empty()) {
            return start_;
        }
        return lines_.back().position + below(rotation_) * lineSpacing(height_);
    }

    katana::commands::EntityAttributes attributes_;
    std::optional<TextGeometry> newest_;
    double defaultHeight_ = kDefaultHeight;
    double defaultRotation_ = 0.0;

    Step step_ = Step::Start;
    Point2 start_;
    double height_ = kDefaultHeight;
    double rotation_ = 0.0;
    // Entered by Enter at the first prompt, so Undo goes back there rather
    // than to a rotation prompt the user never saw.
    bool continued_ = false;
    std::vector<TypedLine> lines_;
};

} // namespace

std::unique_ptr<InteractiveTool> makeTextTool(const ToolContext& context)
{
    return std::make_unique<TextTool>(context);
}

} // namespace katana::cad::tools::annotate
