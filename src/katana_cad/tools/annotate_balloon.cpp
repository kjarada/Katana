// Balloon: a numbered circle callout at the end of a leader, as the BALLOON
// verb makes one - the item numbers of a schedule or a detail's parts.
//
//   Specify the balloon's arrow point or [Number/Style/Paper]     the tip
//   Specify next point or [Number/Style/Paper/Undo]               a bend, or
//                                                                 where the
//                                                                 circle sits
//   Specify next point or [Number/Style/Paper/Undo] <done>        more bends,
//                                                                 or Enter
//
// The number is one more than the highest balloon's in the drawing
// (annotation::nextBalloonNumber, the verb's own rule), or what Number was
// told; Style and Paper are its text style and height on paper, BALLOON's
// style= and paper=, which the tool once had no way to give. The balloon is
// ONE leader entity with a circle callout and the verb's sizes -
// `BALLOON p p [p...] n= style= paper=` typed and this tool given the same
// points and options make the same entity - and a tip snapped to an
// entity's end, middle, centre or vertex follows it, as BALLOON #id.end
// does. The options are for the balloon being drawn, as the Leader's are.

#include <optional>
#include <string>
#include <utility>
#include <vector>

#include "annotate_common.hpp"
#include "katana/cad/annotation/leader_edit.hpp"
#include "katana/cad/document.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/annotation.hpp"
#include "katana/entity/entity.hpp"

namespace katana::cad::tools::annotate {

namespace {

using katana::entity::AnchorRef;
using katana::entity::LeaderGeometry;
using katana::geometry::Point2;

class BalloonTool final : public InteractiveTool {
  public:
    explicit BalloonTool(const ToolContext& context)
        : document_(context.document), attributes_(context.attributes)
    {
    }

    [[nodiscard]] std::string prompt() const override
    {
        switch (asking_) {
        case Asking::Number:
            return "Enter the balloon's number <" + number().value_or(std::string("?")) + ">";
        case Asking::Style:
            return "Enter text style name <" +
                   style_.value_or(std::string(katana::entity::kDefaultTextStyleName)) + ">";
        case Asking::Paper:
            // Unset, the leader's 0: its text style's height, as the verb's.
            return "Enter the number's height on paper, mm <" +
                   (paper_ ? formatNumber(*paper_) : std::string("the style's")) + ">";
        case Asking::Nothing:
            break;
        }
        if (vertices_.empty()) {
            return "Specify the balloon's arrow point or [Number/Style/Paper]";
        }
        return vertices_.size() < 2 ? "Specify next point or [Number/Style/Paper/Undo]"
                                    : "Specify next point or [Number/Style/Paper/Undo] <done>";
    }

    [[nodiscard]] ToolInput expects() const override
    {
        return asking_ != Asking::Nothing ? ToolInput::Value : ToolInput::Point;
    }

    ToolStep point(const Point2& at) override { return place(at, {}); }

    ToolStep anchoredPoint(const Point2& at, const AnchorRef& anchor) override
    {
        return place(at, anchor);
    }

    ToolStep value(std::string_view typed) override
    {
        const std::string_view text = katana::core::trimmed(typed);
        switch (asking_) {
        case Asking::Number:
            if (text.empty()) {
                return enter();
            }
            if (!katana::entity::isValidUtf8(text)) {
                return ToolStep::rejected("the number is not valid UTF-8");
            }
            number_ = std::string(text);
            asking_ = Asking::Nothing;
            return ToolStep::next();
        case Asking::Style: {
            if (text.empty()) {
                return enter();
            }
            auto found = textStyleNamed(document_, text);
            if (!found) {
                return ToolStep::rejected(found.error().message);
            }
            style_ = std::move(*found);
            asking_ = Asking::Nothing;
            return ToolStep::next();
        }
        case Asking::Paper: {
            if (text.empty()) {
                return enter();
            }
            const auto height = typedNumber(text);
            if (!height || !(*height > 0.0)) {
                return ToolStep::rejected("the number's height on paper is millimetres greater "
                                          "than 0");
            }
            paper_ = *height;
            asking_ = Asking::Nothing;
            return ToolStep::next();
        }
        case Asking::Nothing:
            break;
        }
        if (!vertices_.empty() && isOption(typed, "Undo")) {
            return undo();
        }
        if (isOption(typed, "Number")) {
            asking_ = Asking::Number;
            return ToolStep::next();
        }
        if (isOption(typed, "Style")) {
            asking_ = Asking::Style;
            return ToolStep::next();
        }
        if (isOption(typed, "Paper")) {
            asking_ = Asking::Paper;
            return ToolStep::next();
        }
        return ToolStep::rejected(vertices_.empty()
                                      ? "click the point the arrow touches, or type N, S or P for "
                                        "the number, its style or its height on paper"
                                      : "click the next point, type N, S or P for the number, its "
                                        "style or its height on paper, or press Enter to finish");
    }

    ToolStep enter() override
    {
        if (asking_ != Asking::Nothing) {
            asking_ = Asking::Nothing; // keeps what the prompt showed
            return ToolStep::next();
        }
        if (vertices_.empty()) {
            return ToolStep::done(nullptr);
        }
        if (vertices_.size() < 2) {
            return ToolStep::rejected("a balloon needs a second point, where its circle sits");
        }
        if (!number()) {
            return ToolStep::rejected("the highest balloon is numbered as high as a number goes, so "
                                      "has no next: type N and give one, or BALLOON RENUMBER first");
        }
        const LeaderGeometry balloon = balloonThrough(vertices_);
        std::vector<katana::entity::Entity> entities;
        entities.push_back(newEntity(balloon, attributes_));
        return ToolStep::done(createAll("CREATE_BALLOON", std::move(entities)),
                              "balloon " + balloon.text, /*restart=*/true);
    }

    ToolStep undo() override
    {
        if (asking_ != Asking::Nothing) {
            asking_ = Asking::Nothing;
            return ToolStep::next();
        }
        if (vertices_.empty()) {
            return ToolStep::rejected("nothing to undo: no point has been given");
        }
        vertices_.pop_back();
        if (vertices_.empty()) {
            tipRef_ = {};
        }
        return ToolStep::next();
    }

    [[nodiscard]] ToolFeedback preview(const Point2& cursor) const override
    {
        ToolFeedback feedback;
        if (vertices_.empty()) {
            return feedback;
        }
        std::vector<Point2> vertices = vertices_;
        if (!coincident(vertices.back(), cursor)) {
            vertices.push_back(cursor);
        }
        if (vertices.size() >= 2) {
            feedback.shapes.emplace_back(balloonThrough(std::move(vertices)));
        }
        feedback.markers.push_back(vertices_.back());
        return feedback;
    }

    [[nodiscard]] std::optional<Point2> lastPoint() const override
    {
        return vertices_.empty() ? std::nullopt : std::optional<Point2>(vertices_.back());
    }

  private:
    ToolStep place(const Point2& at, const AnchorRef& anchor)
    {
        if (asking_ != Asking::Nothing) {
            return ToolStep::rejected("type what the prompt asks, or press Enter to keep what it "
                                      "shows");
        }
        if (!vertices_.empty() && coincident(vertices_.back(), at)) {
            return ToolStep::rejected(
                "that point is on the previous one; each leader segment needs a length");
        }
        if (vertices_.empty()) {
            tipRef_ = anchor; // only the tip follows what it points at
        }
        vertices_.push_back(at);
        return ToolStep::next();
    }

    // What Number was told, else the drawing's next by the verb's own rule;
    // nullopt only when the drawing's highest balloon has no next.
    [[nodiscard]] std::optional<std::string> number() const
    {
        if (number_) {
            return *number_;
        }
        if (document_ == nullptr) {
            return std::string("1");
        }
        const auto next = katana::cad::annotation::nextBalloonNumber(document_->model());
        return next ? std::optional<std::string>(std::to_string(*next)) : std::nullopt;
    }

    // The verb's balloon: a leader with the entity's default sizes, a circle
    // callout and the number as its note, in the style and at the height on
    // paper the options gave.
    [[nodiscard]] LeaderGeometry balloonThrough(std::vector<Point2> vertices) const
    {
        LeaderGeometry balloon;
        balloon.vertices = std::move(vertices);
        balloon.tipRef = tipRef_;
        balloon.callout = katana::entity::CalloutShape::Circle;
        balloon.text = number().value_or(std::string());
        balloon.style = style_.value_or(std::string());
        balloon.paperHeight = paper_.value_or(LeaderGeometry{}.paperHeight);
        return balloon;
    }

    // What the typed line is for, when it is not a point.
    enum class Asking { Nothing, Number, Style, Paper };

    const Document* document_ = nullptr;
    katana::commands::EntityAttributes attributes_;
    std::vector<Point2> vertices_;
    AnchorRef tipRef_{};
    std::optional<std::string> number_;
    std::optional<std::string> style_;
    std::optional<double> paper_; // mm
    Asking asking_ = Asking::Nothing;
};

} // namespace

std::unique_ptr<InteractiveTool> makeBalloonTool(const ToolContext& context)
{
    return std::make_unique<BalloonTool>(context);
}

} // namespace katana::cad::tools::annotate
