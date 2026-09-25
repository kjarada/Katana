// Leader: an arrow line to a feature with a note at its end, as AutoCAD's
// LEADER draws one.
//
//   Specify leader start point or [Arrow/Callout/Style/Paper]
//                                                       the arrow's tip, or an
//                                                       option for this leader
//   Specify next point or [Undo]                        the first bend or end
//   Specify next point or [Annotation/Undo] <Annotation>  more bends, or Enter
//   Enter the first line of annotation text             typed, line by line;
//                                                       an empty line finishes
//
// The leader is ONE entity, a LeaderGeometry (docs/annotation.md,
// "Leaders and callouts"), added by one command: the arrow, the line, the
// landing and the note are drawn from it at the scale of the view or sheet
// looking at it, and move, copy and erase together. It is the LEADER verb's
// entity. (Before annotation had its own entities the tool drew a polyline,
// an arrowhead and a text per line, which drifted apart when one was moved.)
//
// The options at the first prompt are LEADER's arrow=, callout=, style= and
// paper=: the arrowhead (closed, open, tick, dot or none), a box or circle
// round the note, the note's text style, and its height on paper. They are
// for the leader being drawn, as AutoCAD's LEADER options are, so the next
// takes the dimension style's again. A tip snapped to an entity's end,
// middle, centre or vertex follows it (tipRef), as LEADER #id.end ... does.
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
#include "katana/core/text.hpp"
#include "katana/entity/annotation.hpp"
#include "katana/entity/entity.hpp"
#include "katana/math/numerics.hpp"

namespace katana::cad::tools::annotate {

namespace {

using katana::entity::AnchorRef;
using katana::entity::ArrowHead;
using katana::entity::CalloutShape;
using katana::entity::DimensionStyle;
using katana::entity::LeaderGeometry;
using katana::geometry::Point2;
using katana::geometry::Vec2;
namespace tol = katana::math::tolerance;

constexpr double kHookAngle = 15.0 * katana::math::kDegToRad;

// What the options at the first prompt were told; unset takes the dimension
// style's arrow and height, no callout and the default text style.
struct Options {
    std::optional<ArrowHead> arrow;
    std::optional<CalloutShape> callout;
    std::optional<std::string> textStyle;
    std::optional<double> paper; // mm
};

// The arrowheads and callouts by the words their options offer, in order.
constexpr std::pair<ArrowHead, std::string_view> kArrows[] = {
    {ArrowHead::ClosedFilled, "Closed"}, {ArrowHead::Open, "Open"}, {ArrowHead::Tick, "Tick"},
    {ArrowHead::Dot, "Dot"},           {ArrowHead::None, "None"}};
constexpr std::pair<CalloutShape, std::string_view> kCallouts[] = {
    {CalloutShape::None, "None"}, {CalloutShape::Box, "Box"}, {CalloutShape::Circle, "Circle"}};

template <class Value, std::size_t N>
std::string wordFor(const std::pair<Value, std::string_view> (&words)[N], Value value)
{
    for (const auto& [each, word] : words) {
        if (each == value) {
            return std::string(word);
        }
    }
    return {};
}

// "Closed, Open, Tick, Dot, None" as an option list: "Closed/Open/...".
template <class Value, std::size_t N>
std::string optionList(const std::pair<Value, std::string_view> (&words)[N])
{
    std::string list;
    for (const auto& [each, word] : words) {
        list += list.empty() ? "" : "/";
        list += word;
    }
    return list;
}

// The leader through `vertices` with note `lines`, sized from `style` at
// 1 : `scale` unless `options` say otherwise, its tip following `tip`.
// `vertices` has at least two points, none coincident with the next.
LeaderGeometry leaderFor(std::vector<Point2> vertices, const std::vector<std::string>& lines,
                         const DimensionStyle& style, double scale, const Options& options,
                         const AnchorRef& tip)
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
    leader.arrow = options.arrow.value_or(style.arrowHead);
    leader.callout = options.callout.value_or(CalloutShape::None);
    leader.style = options.textStyle.value_or(std::string());
    leader.arrowSize = paper(style.arrowSize);
    leader.paperHeight = options.paper.value_or(paper(style.textHeight));
    leader.landing = !lines.empty() && steep ? leader.arrowSize : 0.0;
    leader.tipRef = tip;
    return leader;
}

class LeaderTool final : public InteractiveTool {
  public:
    explicit LeaderTool(const ToolContext& context)
        : document_(context.document), attributes_(context.attributes),
          style_(styleForNewAnnotation(context)),
          scale_(context.document != nullptr ? context.document->annotationScale()
                                             : katana::entity::kDefaultAnnotationScale)
    {
    }

    [[nodiscard]] std::string prompt() const override
    {
        switch (step_) {
        case Step::Tip:
            return "Specify leader start point or [Arrow/Callout/Style/Paper]" + settings();
        case Step::Vertices:
            return vertices_.size() < 2 ? "Specify next point or [Undo]"
                                        : "Specify next point or [Annotation/Undo] <Annotation>";
        case Step::Annotation:
            return lines_.empty()
                       ? "Enter the first line of annotation text, or press Enter for none"
                       : "Enter the next line of annotation text, or press Enter to finish";
        case Step::Arrow:
            return "Enter arrowhead [" + optionList(kArrows) + "] <" +
                   wordFor(kArrows, options_.arrow.value_or(style_.arrowHead)) + ">";
        case Step::Callout:
            return "Enter callout [" + optionList(kCallouts) + "] <" +
                   wordFor(kCallouts, options_.callout.value_or(CalloutShape::None)) + ">";
        case Step::Style:
            return "Enter text style name <" +
                   options_.textStyle.value_or(std::string(katana::entity::kDefaultTextStyleName)) +
                   ">";
        case Step::Paper:
            return "Enter the note's height on paper, mm <" + formatNumber(paperHeight()) + ">";
        }
        return {};
    }

    [[nodiscard]] ToolInput expects() const override
    {
        return step_ == Step::Tip || step_ == Step::Vertices ? ToolInput::Point : ToolInput::Value;
    }

    ToolStep point(const Point2& at) override
    {
        switch (step_) {
        case Step::Tip:
            vertices_ = {at};
            tipRef_ = pending_;
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
        case Step::Arrow:
        case Step::Callout:
        case Step::Style:
        case Step::Paper:
            break;
        }
        return InteractiveTool::point(at);
    }

    // Only the tip follows what it was snapped to; the bends are where they
    // were put.
    ToolStep anchoredPoint(const Point2& at, const AnchorRef& anchor) override
    {
        pending_ = anchor;
        ToolStep step = point(at);
        pending_ = {};
        return step;
    }

    ToolStep value(std::string_view text) override
    {
        switch (step_) {
        case Step::Tip:
            if (isOption(text, "Arrow")) {
                return ask(Step::Arrow);
            }
            if (isOption(text, "Callout")) {
                return ask(Step::Callout);
            }
            if (isOption(text, "Style")) {
                return ask(Step::Style);
            }
            if (isOption(text, "Paper")) {
                return ask(Step::Paper);
            }
            if (isOption(text, "Undo")) {
                return undo();
            }
            return ToolStep::rejected("click the point the arrow touches or type it as x,y, or "
                                      "type A, C, S or P for this leader's options");
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
        case Step::Arrow:
            for (const auto& [head, word] : kArrows) {
                if (isOption(text, word)) {
                    options_.arrow = head;
                    step_ = Step::Tip;
                    return ToolStep::next();
                }
            }
            return ToolStep::rejected("type C, O, T, D or N: closed, open, tick, dot or none");
        case Step::Callout:
            for (const auto& [shape, word] : kCallouts) {
                if (isOption(text, word)) {
                    options_.callout = shape;
                    step_ = Step::Tip;
                    return ToolStep::next();
                }
            }
            return ToolStep::rejected("type N, B or C: none, a box or a circle");
        case Step::Style:
            return styleValue(text);
        case Step::Paper: {
            const auto height = typedNumber(text);
            if (!height || !(*height > 0.0)) {
                return ToolStep::rejected("the note's height on paper is millimetres greater "
                                          "than 0");
            }
            options_.paper = *height;
            step_ = Step::Tip;
            return ToolStep::next();
        }
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
        case Step::Arrow:
        case Step::Callout:
        case Step::Style:
        case Step::Paper:
            // What the option shows is kept.
            step_ = Step::Tip;
            return ToolStep::next();
        case Step::Annotation:
            break;
        }
        std::vector<katana::entity::Entity> entities;
        entities.push_back(newEntity(leader(vertices_, lines_), attributes_));
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
                tipRef_ = {};
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
        case Step::Arrow:
        case Step::Callout:
        case Step::Style:
        case Step::Paper:
            step_ = Step::Tip;
            return ToolStep::next();
        }
        return InteractiveTool::undo();
    }

    [[nodiscard]] ToolFeedback preview(const Point2& cursor) const override
    {
        ToolFeedback feedback;
        switch (step_) {
        case Step::Tip:
        case Step::Arrow:
        case Step::Callout:
        case Step::Style:
        case Step::Paper:
            break;
        case Step::Vertices: {
            // The leader as it would be with the cursor as its next point,
            // arrowhead included, so its size is seen before it is placed.
            std::vector<Point2> vertices = vertices_;
            if (!coincident(vertices.back(), cursor)) {
                vertices.push_back(cursor);
            }
            if (vertices.size() >= 2) {
                feedback.shapes.emplace_back(leader(std::move(vertices), {}));
            }
            feedback.markers.push_back(vertices_.back());
            break;
        }
        case Step::Annotation:
            feedback.shapes.emplace_back(leader(vertices_, lines_));
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
    enum class Step { Tip, Vertices, Annotation, Arrow, Callout, Style, Paper };

    ToolStep ask(Step option)
    {
        step_ = option;
        return ToolStep::next();
    }

    [[nodiscard]] LeaderGeometry leader(std::vector<Point2> vertices,
                                        const std::vector<std::string>& lines) const
    {
        return leaderFor(std::move(vertices), lines, style_, scale_, options_, tipRef_);
    }

    [[nodiscard]] double paperHeight() const
    {
        return options_.paper.value_or(
            style_.paperSized ? style_.textHeight
                              : katana::entity::annotationPaperSize(style_.textHeight, scale_));
    }

    // The options given so far, after the first prompt: nothing else on
    // screen shows them before the leader is drawn.
    [[nodiscard]] std::string settings() const
    {
        std::string said;
        const auto add = [&](const std::string& part) {
            said += said.empty() ? " (" : ", ";
            said += part;
        };
        if (options_.arrow) {
            add(wordFor(kArrows, *options_.arrow) + " arrow");
        }
        if (options_.callout) {
            add(wordFor(kCallouts, *options_.callout) + " callout");
        }
        if (options_.textStyle) {
            add("style " + *options_.textStyle);
        }
        if (options_.paper) {
            add(formatNumber(*options_.paper) + " mm");
        }
        return said.empty() ? said : said + ")";
    }

    ToolStep styleValue(std::string_view typed)
    {
        const std::string_view text = katana::core::trimmed(typed);
        if (text.empty()) {
            return enter();
        }
        if (document_ == nullptr) {
            return ToolStep::rejected("there is no drawing to take the style from");
        }
        const auto& styles = document_->model().textStyles;
        std::string found;
        if (styles.contains(text)) {
            found = std::string(text);
        } else {
            for (const std::string& name : styles.names()) {
                if (katana::core::equalsIgnoringCase(name, text)) {
                    found = name;
                    break;
                }
            }
        }
        if (found.empty()) {
            return ToolStep::rejected("there is no text style \"" + std::string(text) +
                                      "\"; Format > Text Styles lists them");
        }
        options_.textStyle = found;
        step_ = Step::Tip;
        return ToolStep::next();
    }

    const Document* document_ = nullptr;
    katana::commands::EntityAttributes attributes_;
    DimensionStyle style_;
    double scale_;

    Step step_ = Step::Tip;
    Options options_;
    std::vector<Point2> vertices_;
    // The entity point the tip was snapped to; unassociated when it was
    // clicked or typed.
    AnchorRef tipRef_{};
    // Set only while anchoredPoint hands a snapped point to point().
    AnchorRef pending_{};
    std::vector<std::string> lines_;
};

} // namespace

std::unique_ptr<InteractiveTool> makeLeaderTool(const ToolContext& context)
{
    return std::make_unique<LeaderTool>(context);
}

} // namespace katana::cad::tools::annotate
