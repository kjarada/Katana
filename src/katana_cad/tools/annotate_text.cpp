// Text and Multiline Text: single-line text, line after line, as AutoCAD's
// DTEXT places it, and one text of several lines, as its MTEXT makes one.
//
//   Specify start point of text or [Style/Justify/Paper]
//                                                a point, or an option:
//     Style    a text style's name, or . for none
//     Justify  TL TC TR ML MC MR BL BC BR (entity.hpp, TextJustify)
//     Paper    a height on paper in mm; 0 for a model height
//   Specify height or [Undo] <2.5>               a number, a point (its distance
//                                                from the start), or Enter;
//                                                not asked of a paper-sized text
//   Specify rotation angle of text or [Undo] <0> degrees counter-clockwise from
//                                                east, a point (the direction to
//                                                it), or Enter
//   Enter text                                   typed; each line goes below
//                                                the last; an empty line ends
//
// All the lines of one use are ONE command, so one undo removes the whole
// text - as one DTEXT command undoes as one in AutoCAD. Text makes an entity
// a line; Multiline Text makes ONE entity of every line, broken by '\n', as
// MTEXT p "a\nb" does.
//
// The style, justification and paper height are the TEXT verb's options
// (docs/annotation.md), and a text is made from them by the rule the verb
// uses (annotation::fitModelHeight): a paper-sized one - a paper height, or a
// style with one - is drawn at its paper size at every scale, so its model
// height is worked out for the drawing's annotation scale and no height is
// asked for.
//
// The defaults come from the drawing: the newest text in it gives the height,
// rotation, style, justification and paper height offered, and Enter at
// Text's first prompt continues directly below it (DTEXT's own shortcut).
// Reading them from the drawing rather than from memory in the tool means
// they follow the drawing across sessions and through undo, as TEXTSIZE does
// in a DWG, and a test sees the same default whatever ran before it. With no
// text in the drawing the height is 2.5, the metric default of AutoCAD's
// TEXTSIZE and of this model's TextGeometry. Multiline Text is paper-sized
// unless told otherwise, as MTEXT is: where the newest text would leave it
// in model units it starts in the Standard style.

#include <optional>
#include <string>
#include <variant>
#include <vector>

#include "annotate_common.hpp"
#include "katana/cad/annotation/text_layout.hpp"
#include "katana/cad/document.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/annotation.hpp"
#include "katana/entity/entity.hpp"
#include "katana/math/numerics.hpp"

namespace katana::cad::tools::annotate {

namespace {

using katana::entity::TextGeometry;
using katana::entity::TextJustify;
using katana::geometry::Point2;
using katana::geometry::Segment2;
using katana::geometry::Vec2;

constexpr double kDefaultHeight = 2.5;

struct TypedLine {
    Point2 position;
    std::string text;
};

// Text: an entity a line. Multiline: one entity of all the lines.
enum class Mode { Lines, Block };

class TextTool final : public InteractiveTool {
  public:
    TextTool(const ToolContext& context, Mode mode)
        : mode_(mode), attributes_(context.attributes), document_(context.document)
    {
        if (document_ != nullptr) {
            scale_ = document_->annotationScale();
            // Ascending id order, so the last one seen is the newest.
            document_->model().entities.forEach([&](const katana::entity::Entity& entity) {
                if (const auto* text = std::get_if<TextGeometry>(&entity.geometry)) {
                    newest_ = *text;
                }
            });
        }
        if (newest_) {
            defaultHeight_ = newest_->height;
            defaultRotation_ = newest_->rotation;
            style_ = newest_->style;
            paper_ = newest_->paperHeight;
            justify_ = newest_->justify;
        }
        if (mode_ == Mode::Block && !paperSized()) {
            style_ = std::string(katana::entity::kDefaultTextStyleName);
            paper_ = 0.0;
        }
    }

    [[nodiscard]] std::string prompt() const override
    {
        switch (step_) {
        case Step::Start: {
            std::string text = mode_ == Mode::Block
                                   ? "Specify insertion point of the text or [Style/Justify/Paper]"
                                   : "Specify start point of text or [Style/Justify/Paper]";
            if (mode_ == Mode::Lines && newest_) {
                text += ", or press Enter to continue below the last text";
            }
            return text + " <" + settings() + ">";
        }
        case Step::Style:
            return "Enter a text style name, or . for none <" + (style_.empty() ? "." : style_) +
                   ">";
        case Step::Justify:
            return "Enter a justification [TL/TC/TR/ML/MC/MR/BL/BC/BR] <" +
                   std::string(katana::entity::toString(justify_)) + ">";
        case Step::Paper:
            return "Specify the height on paper in mm, or 0 for a model height <" +
                   formatNumber(paper_) + ">";
        case Step::Height:
            return "Specify height or [Undo] <" + formatNumber(defaultHeight_) + ">";
        case Step::Rotation:
            return "Specify rotation angle of text or [Undo] <" +
                   formatNumber(defaultRotation_ * katana::math::kRadToDeg) + ">";
        case Step::Content:
            if (mode_ == Mode::Block) {
                return lines_.empty() ? "Enter the first line of text"
                                      : "Enter the next line, or press Enter to finish the text";
            }
            return lines_.empty() ? "Enter text"
                                  : "Enter the next line of text, or press Enter to finish";
        }
        return {};
    }

    [[nodiscard]] ToolInput expects() const override
    {
        // The text itself is a Value so that "Road, north side" stays text
        // rather than being read as a malformed point; so is a style's name.
        switch (step_) {
        case Step::Style:
        case Step::Justify:
        case Step::Paper:
        case Step::Content:
            return ToolInput::Value;
        case Step::Start:
        case Step::Height:
        case Step::Rotation:
            break;
        }
        return ToolInput::Point;
    }

    ToolStep point(const Point2& at) override
    {
        switch (step_) {
        case Step::Start:
            start_ = at;
            // A paper-sized text has its height from the paper: nothing to ask.
            if (paperSized()) {
                height_ = paperModelHeight();
                step_ = Step::Rotation;
            } else {
                step_ = Step::Height;
            }
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
        case Step::Style:
        case Step::Justify:
        case Step::Paper:
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
            if (isOption(text, "Style")) {
                step_ = Step::Style;
                return ToolStep::next();
            }
            if (isOption(text, "Justify")) {
                step_ = Step::Justify;
                return ToolStep::next();
            }
            if (isOption(text, "Paper")) {
                step_ = Step::Paper;
                return ToolStep::next();
            }
            return ToolStep::rejected(
                "'" + std::string(text) +
                "' is not a point; click the start point or type it as x,y (or S, J or P for the "
                "style, justification or paper height)");
        case Step::Style:
            return chooseStyle(text);
        case Step::Justify: {
            const auto justify = katana::entity::textJustifyFromString(text);
            if (!justify) {
                return ToolStep::rejected("'" + std::string(text) +
                                          "' is not a justification: TL, TC, TR, ML, MC, MR, BL, "
                                          "BC or BR");
            }
            justify_ = *justify;
            step_ = Step::Start;
            return ToolStep::next();
        }
        case Step::Paper: {
            const auto paper = typedNumber(text);
            if (!paper || *paper < 0.0) {
                return ToolStep::rejected(
                    "the height on paper is a number of millimetres, or 0 for a model height");
            }
            paper_ = *paper;
            step_ = Step::Start;
            return ToolStep::next();
        }
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
            if (mode_ == Mode::Block || !newest_) {
                return ToolStep::done(nullptr);
            }
            // DTEXT's Enter at the first prompt: carry on below the newest
            // text, in its height and rotation, without asking again.
            height_ = paperSized() ? paperModelHeight() : newest_->height;
            rotation_ = newest_->rotation;
            start_ = newest_->position + below(rotation_) * lineSpacing(height_);
            continued_ = true;
            step_ = Step::Content;
            return ToolStep::next();
        case Step::Style:
        case Step::Justify:
        case Step::Paper:
            // Enter keeps the choice shown in the prompt.
            step_ = Step::Start;
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
        const std::size_t count = lines_.size();
        if (mode_ == Mode::Block) {
            entities.push_back(newEntity(blockText(), attributes_));
            std::string message =
                count == 1 ? "a text of 1 line" : "a text of " + std::to_string(count) + " lines";
            return ToolStep::done(createAll("CREATE_TEXT", std::move(entities)),
                                  std::move(message), /*restart=*/true);
        }
        entities.reserve(count);
        for (const TypedLine& line : lines_) {
            entities.push_back(newEntity(made(line.position, line.text), attributes_));
        }
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
        case Step::Style:
        case Step::Justify:
        case Step::Paper:
            step_ = Step::Start;
            return ToolStep::next();
        case Step::Height:
            step_ = Step::Start;
            return ToolStep::next();
        case Step::Rotation:
            // A paper-sized text was never asked its height.
            step_ = paperSized() ? Step::Start : Step::Height;
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
        case Step::Style:
        case Step::Justify:
        case Step::Paper:
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
            // text is finished, and where the next one will start - for a
            // multiline text, where its block is anchored.
            if (mode_ == Mode::Block) {
                if (!lines_.empty()) {
                    feedback.shapes.emplace_back(blockText());
                }
                feedback.markers.push_back(start_);
                break;
            }
            for (const TypedLine& line : lines_) {
                feedback.shapes.emplace_back(made(line.position, line.text));
            }
            feedback.markers.push_back(nextPosition());
            break;
        }
        return feedback;
    }

    [[nodiscard]] std::optional<Point2> lastPoint() const override
    {
        if (step_ == Step::Start || step_ == Step::Style || step_ == Step::Justify ||
            step_ == Step::Paper) {
            return std::nullopt;
        }
        return start_;
    }

  private:
    enum class Step { Start, Style, Justify, Paper, Height, Rotation, Content };

    // Down the page in the text's own frame: a rotated text's next line is
    // below it as the text reads, not below it on the drawing.
    [[nodiscard]] static Vec2 below(double rotation) { return Vec2(0.0, -1.0).rotated(rotation); }

    [[nodiscard]] Point2 nextPosition() const
    {
        if (mode_ == Mode::Block || lines_.empty()) {
            return start_;
        }
        return lines_.back().position + below(rotation_) * lineSpacing(height_);
    }

    // The text the choices make at `at`, sized by the verb's rule.
    [[nodiscard]] TextGeometry made(const Point2& at, std::string text) const
    {
        TextGeometry geometry{at, std::move(text), height_, rotation_};
        geometry.style = style_;
        geometry.paperHeight = paper_;
        geometry.justify = justify_;
        if (document_ != nullptr) {
            katana::cad::annotation::fitModelHeight(document_->model(), scale_, geometry);
        }
        return geometry;
    }

    [[nodiscard]] TextGeometry blockText() const
    {
        std::string joined;
        for (const TypedLine& line : lines_) {
            joined += joined.empty() ? "" : "\n";
            joined += line.text;
        }
        return made(start_, std::move(joined));
    }

    // Whether the text as chosen is paper-sized: a paper height, or a style
    // with one.
    [[nodiscard]] bool paperSized() const
    {
        if (paper_ > 0.0) {
            return true;
        }
        if (document_ == nullptr || style_.empty()) {
            return false;
        }
        TextGeometry probe;
        probe.style = style_;
        return katana::cad::annotation::isPaperSized(document_->model(), probe);
    }

    // A paper-sized text's model height at the drawing's annotation scale.
    [[nodiscard]] double paperModelHeight() const { return made(start_, {}).height; }

    // What the options are now, for the first prompt: "style Notes, MC, 3.5 mm".
    [[nodiscard]] std::string settings() const
    {
        std::string text = "style " + (style_.empty() ? std::string("none") : style_) + ", " +
                           std::string(katana::entity::toString(justify_));
        if (paper_ > 0.0) {
            text += ", " + formatNumber(paper_) + " mm on paper";
        }
        return text;
    }

    ToolStep chooseStyle(std::string_view typed)
    {
        const std::string name(katana::core::trimmed(typed));
        if (name == ".") {
            style_.clear();
        } else if (document_ != nullptr && document_->model().textStyles.contains(name)) {
            style_ = name;
        } else {
            return ToolStep::rejected("there is no text style '" + name +
                                      "'; type a style's name, or . for none");
        }
        step_ = Step::Start;
        return ToolStep::next();
    }

    Mode mode_;
    katana::commands::EntityAttributes attributes_;
    const Document* document_ = nullptr;
    double scale_ = katana::entity::kDefaultAnnotationScale;
    std::optional<TextGeometry> newest_;
    double defaultHeight_ = kDefaultHeight;
    double defaultRotation_ = 0.0;

    // The options, offered at the first prompt.
    std::string style_;
    double paper_ = 0.0;
    TextJustify justify_ = TextJustify::BottomLeft;

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
    return std::make_unique<TextTool>(context, Mode::Lines);
}

std::unique_ptr<InteractiveTool> makeMultilineTextTool(const ToolContext& context)
{
    return std::make_unique<TextTool>(context, Mode::Block);
}

} // namespace katana::cad::tools::annotate
