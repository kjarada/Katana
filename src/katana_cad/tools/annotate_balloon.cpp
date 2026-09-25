// Balloon: a numbered circle callout at the end of a leader, as the BALLOON
// verb makes one - the item numbers of a schedule or a detail's parts.
//
//   Specify the balloon's arrow point or [Number]          the tip
//   Specify next point or [Number/Undo]                    a bend, or where the
//                                                          circle sits
//   Specify next point or [Number/Undo] <done>             more bends, or Enter
//
// The number is one more than the highest balloon's in the drawing
// (annotation::nextBalloonNumber, the verb's own rule), or what Number was
// told. The balloon is ONE leader entity with a circle callout and the
// verb's sizes - `BALLOON p p [p...] n=` typed and this tool given the same
// points make the same entity - and a tip snapped to an entity's end,
// middle, centre or vertex follows it, as BALLOON #id.end does.

#include <optional>
#include <string>
#include <vector>

#include "annotate_common.hpp"
#include "katana/cad/annotation/leader_build.hpp"
#include "katana/cad/document.hpp"
#include "katana/core/text.hpp"
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
        if (numbering_) {
            return "Enter the balloon's number <" + number() + ">";
        }
        if (vertices_.empty()) {
            return "Specify the balloon's arrow point or [Number]";
        }
        return vertices_.size() < 2 ? "Specify next point or [Number/Undo]"
                                    : "Specify next point or [Number/Undo] <done>";
    }

    [[nodiscard]] ToolInput expects() const override
    {
        return numbering_ ? ToolInput::Value : ToolInput::Point;
    }

    ToolStep point(const Point2& at) override { return place(at, {}); }

    ToolStep anchoredPoint(const Point2& at, const AnchorRef& anchor) override
    {
        return place(at, anchor);
    }

    ToolStep value(std::string_view typed) override
    {
        if (numbering_) {
            const std::string_view text = katana::core::trimmed(typed);
            if (text.empty()) {
                return enter();
            }
            if (!katana::entity::isValidUtf8(text)) {
                return ToolStep::rejected("the number is not valid UTF-8");
            }
            number_ = std::string(text);
            numbering_ = false;
            return ToolStep::next();
        }
        if (!vertices_.empty() && isOption(typed, "Undo")) {
            return undo();
        }
        if (isOption(typed, "Number")) {
            numbering_ = true;
            return ToolStep::next();
        }
        return ToolStep::rejected(vertices_.empty()
                                      ? "click the point the arrow touches, or type N for the number"
                                      : "click the next point, type N for the number, or press "
                                        "Enter to finish");
    }

    ToolStep enter() override
    {
        if (numbering_) {
            numbering_ = false; // keeps the number shown
            return ToolStep::next();
        }
        if (vertices_.empty()) {
            return ToolStep::done(nullptr);
        }
        if (vertices_.size() < 2) {
            return ToolStep::rejected("a balloon needs a second point, where its circle sits");
        }
        const LeaderGeometry balloon = balloonThrough(vertices_);
        std::vector<katana::entity::Entity> entities;
        entities.push_back(newEntity(balloon, attributes_));
        return ToolStep::done(createAll("CREATE_BALLOON", std::move(entities)),
                              "balloon " + balloon.text, /*restart=*/true);
    }

    ToolStep undo() override
    {
        if (numbering_) {
            numbering_ = false;
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
        if (numbering_) {
            return ToolStep::rejected("type the balloon's number, or press Enter to keep " +
                                      number());
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

    // What Number was told, else the drawing's next.
    [[nodiscard]] std::string number() const
    {
        if (number_) {
            return *number_;
        }
        return document_ != nullptr ? katana::cad::annotation::nextBalloonNumber(document_->model())
                                    : std::string("1");
    }

    // The verb's balloon: a leader with the entity's default sizes, a circle
    // callout and the number as its note.
    [[nodiscard]] LeaderGeometry balloonThrough(std::vector<Point2> vertices) const
    {
        LeaderGeometry balloon;
        balloon.vertices = std::move(vertices);
        balloon.tipRef = tipRef_;
        balloon.callout = katana::entity::CalloutShape::Circle;
        balloon.text = number();
        return balloon;
    }

    const Document* document_ = nullptr;
    katana::commands::EntityAttributes attributes_;
    std::vector<Point2> vertices_;
    AnchorRef tipRef_{};
    std::optional<std::string> number_;
    bool numbering_ = false;
};

} // namespace

std::unique_ptr<InteractiveTool> makeBalloonTool(const ToolContext& context)
{
    return std::make_unique<BalloonTool>(context);
}

} // namespace katana::cad::tools::annotate
