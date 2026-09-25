// Baseline and continued dimensions, as AutoCAD's DIMBASELINE and
// DIMCONTINUE take them:
//
//   Specify next extension line origin (from dimension 7) or
//   [Select/Spacing/Undo] <done>          each point one more dimension; Enter
//                                         (or Esc) makes them all, and Enter
//                                         with none placed ends the tool
//   Select base dimension                 after Select, or when the drawing has
//                                         no linear or aligned dimension yet
//
// Baseline measures every point from the base's first origin, each dimension
// line a spacing further out; Continue chains them end to end from the
// base's second origin, every line on the base's. The chain is made by
// annotation::dimensionChain - DIM BASELINE's and DIM CONTINUE's own door -
// as ONE command, on the base's layer and in its style: "DIM BASELINE 7 p p
// spacing=5" typed and this tool given the same points make the same
// dimensions. Baseline's spacing is a text height and a half of the base's
// style unless Spacing says otherwise (Continue has no Spacing).
//
// The base is the drawing's newest linear or aligned dimension until Select
// picks another: the one the last dimension tool made, as AutoCAD goes on
// from the last dimension drawn. The drawing, not the program, remembers it:
// after a chain its newest is the chain's last, so the tool, starting again,
// goes on from there - Enter, Enter makes a chain and ends, as in AutoCAD. A
// point snapped to an entity's end, middle, centre or vertex follows it, as
// DIM BASELINE's #id points do.

#include <optional>
#include <string>
#include <variant>
#include <vector>

#include "annotate_common.hpp"
#include "katana/cad/annotation/dimension_build.hpp"
#include "katana/cad/document.hpp"
#include "katana/entity/entity.hpp"

namespace katana::cad::tools::annotate {

namespace {

namespace ann = katana::cad::annotation;
using katana::entity::AnchorRef;
using katana::entity::DimensionGeometry;
using katana::entity::DimensionKind;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::geometry::Point2;

// The dimension geometry a chain can go on from: a linear or aligned one.
const DimensionGeometry* chainable(const Entity* entity)
{
    const auto* dimension =
        entity != nullptr ? std::get_if<DimensionGeometry>(&entity->geometry) : nullptr;
    if (dimension == nullptr ||
        (dimension->kind != DimensionKind::Aligned && dimension->kind != DimensionKind::Linear)) {
        return nullptr;
    }
    return dimension;
}

// The newest linear or aligned dimension in the drawing; 0 when there is none.
EntityId newestChainable(const katana::entity::Model& model)
{
    EntityId newest = 0;
    model.entities.forEach([&](const Entity& entity) {
        if (chainable(&entity) != nullptr) {
            newest = entity.id;
        }
    });
    return newest;
}

class ChainTool final : public InteractiveTool {
  public:
    ChainTool(const ToolContext& context, ann::DimensionChain kind)
        : document_(context.document), kind_(kind),
          scale_(context.document != nullptr ? context.document->annotationScale()
                                             : katana::entity::kDefaultAnnotationScale)
    {
        base_ = document_ != nullptr ? newestChainable(document_->model()) : 0;
        step_ = base_ != 0 ? Step::Next : Step::Base;
    }

    [[nodiscard]] std::string prompt() const override
    {
        switch (step_) {
        case Step::Base:
            return "Select base dimension";
        case Step::Next:
            return "Specify next extension line origin (from dimension " + std::to_string(base_) +
                   ") or " +
                   (baseline() ? "[Select/Spacing/Undo] <done>" : "[Select/Undo] <done>");
        case Step::Spacing:
            return "Specify baseline spacing <" + formatNumber(spacing()) + ">";
        }
        return {};
    }

    [[nodiscard]] ToolInput expects() const override
    {
        switch (step_) {
        case Step::Base:
            return ToolInput::Entity;
        case Step::Next:
            return ToolInput::Point;
        case Step::Spacing:
            break;
        }
        return ToolInput::Value;
    }

    ToolStep entity(EntityId id, const Point2& at) override
    {
        if (step_ != Step::Base) {
            return InteractiveTool::entity(id, at);
        }
        const Entity* picked = document_ != nullptr ? document_->model().entities.find(id) : nullptr;
        if (chainable(picked) == nullptr) {
            return ToolStep::rejected("pick a linear or aligned dimension to go on from");
        }
        base_ = id;
        step_ = Step::Next;
        return ToolStep::next();
    }

    ToolStep point(const Point2& at) override { return place(at, {}); }

    ToolStep anchoredPoint(const Point2& at, const AnchorRef& anchor) override
    {
        return place(at, anchor);
    }

    ToolStep value(std::string_view text) override
    {
        switch (step_) {
        case Step::Base:
            return ToolStep::rejected("pick the linear or aligned dimension to go on from");
        case Step::Next:
            if (isOption(text, "Undo")) {
                return undo();
            }
            if (isOption(text, "Select")) {
                if (!points_.empty()) {
                    return ToolStep::rejected("press Enter to make the dimensions placed first, "
                                              "then choose another base");
                }
                chose_ = true;
                step_ = Step::Base;
                return ToolStep::next();
            }
            if (baseline() && isOption(text, "Spacing", 2)) {
                step_ = Step::Spacing;
                return ToolStep::next();
            }
            return ToolStep::rejected(baseline() ? "click the next origin, type SP for the spacing "
                                                   "or S to choose another base, or press Enter "
                                                   "to finish"
                                                 : "click the next origin, type S to choose "
                                                   "another base, or press Enter to finish");
        case Step::Spacing: {
            if (text.empty()) {
                return enter();
            }
            const auto spacing = typedNumber(text);
            if (!spacing || !(*spacing > 0.0)) {
                return ToolStep::rejected("the spacing is a distance greater than 0");
            }
            spacing_ = *spacing;
            step_ = Step::Next;
            return ToolStep::next();
        }
        }
        return InteractiveTool::value(text);
    }

    ToolStep enter() override
    {
        switch (step_) {
        case Step::Base:
            // As AutoCAD's Select prompt: Enter ends the tool.
            return ToolStep::done(nullptr);
        case Step::Next:
            return finish(/*restart=*/true);
        case Step::Spacing:
            step_ = Step::Next;
            return ToolStep::next();
        }
        return InteractiveTool::enter();
    }

    ToolStep undo() override
    {
        switch (step_) {
        case Step::Base:
            if (chose_ && base_ != 0) {
                step_ = Step::Next; // back to the base it had
                return ToolStep::next();
            }
            return ToolStep::rejected("nothing to undo: no dimension has been chosen");
        case Step::Next:
            if (points_.empty()) {
                return ToolStep::rejected("nothing to undo: no origin has been given");
            }
            points_.pop_back();
            return ToolStep::next();
        case Step::Spacing:
            step_ = Step::Next;
            return ToolStep::next();
        }
        return InteractiveTool::undo();
    }

    // Esc keeps the dimensions already placed, as a LINE keeps its segments.
    ToolStep cancel() override
    {
        if (points_.empty()) {
            return ToolStep::done(nullptr);
        }
        return finish(/*restart=*/false);
    }

    [[nodiscard]] ToolFeedback preview(const Point2& cursor) const override
    {
        ToolFeedback feedback;
        if (step_ != Step::Next) {
            return feedback;
        }
        std::vector<ann::AnchoredPoint> points = points_;
        points.push_back({cursor, {}});
        if (auto chain = chainThrough(points)) {
            for (const DimensionGeometry& dimension : *chain) {
                feedback.shapes.emplace_back(dimension);
            }
        }
        for (const ann::AnchoredPoint& point : points_) {
            feedback.markers.push_back(point.point);
        }
        return feedback;
    }

    [[nodiscard]] std::optional<Point2> lastPoint() const override
    {
        if (!points_.empty()) {
            return points_.back().point;
        }
        if (const DimensionGeometry* base = baseGeometry()) {
            return base->end;
        }
        return std::nullopt;
    }

  private:
    enum class Step { Base, Next, Spacing };

    [[nodiscard]] bool baseline() const { return kind_ == ann::DimensionChain::Baseline; }

    [[nodiscard]] const Entity* baseEntity() const
    {
        return document_ != nullptr && base_ != 0 ? document_->model().entities.find(base_)
                                                  : nullptr;
    }

    [[nodiscard]] const DimensionGeometry* baseGeometry() const { return chainable(baseEntity()); }

    // Spacing's value, else what a chain from the base takes by default.
    [[nodiscard]] double spacing() const
    {
        if (spacing_) {
            return *spacing_;
        }
        const Entity* base = baseEntity();
        return base != nullptr ? ann::baselineSpacing(document_->model(), *base, scale_) : 0.0;
    }

    [[nodiscard]] katana::core::Result<std::vector<DimensionGeometry>>
    chainThrough(const std::vector<ann::AnchoredPoint>& points) const
    {
        const DimensionGeometry* base = baseGeometry();
        if (base == nullptr) {
            return katana::core::makeError(katana::core::ErrorCode::NotFound,
                                           "the base dimension is no longer in the drawing");
        }
        return baseline() ? ann::baselineDimensions(*base, points, spacing())
                          : ann::continuedDimensions(*base, points);
    }

    ToolStep place(const Point2& at, const AnchorRef& anchor)
    {
        if (step_ == Step::Base) {
            return ToolStep::rejected("pick the linear or aligned dimension to go on from");
        }
        if (step_ == Step::Spacing) {
            return ToolStep::rejected("type the spacing, or press Enter to keep it");
        }
        std::vector<ann::AnchoredPoint> points = points_;
        points.push_back({at, anchor});
        // Each point is tried as it is given, so one that would make a
        // dimension of nothing is refused here and not at the end.
        if (auto chain = chainThrough(points); !chain) {
            return ToolStep::rejected(chain.error().message);
        }
        points_ = std::move(points);
        return ToolStep::next();
    }

    ToolStep finish(bool restart)
    {
        if (points_.empty()) {
            return ToolStep::done(nullptr);
        }
        if (document_ == nullptr) {
            return ToolStep::rejected("there is no drawing to dimension");
        }
        auto built = ann::dimensionChain(document_->model(), base_, points_, kind_, spacing_, scale_);
        if (!built) {
            return ToolStep::rejected(built.error().message);
        }
        const std::size_t count = points_.size();
        return ToolStep::done(std::move(*built),
                              std::to_string(count) + (baseline() ? " baseline" : " continued") +
                                  (count == 1 ? " dimension" : " dimensions"),
                              restart);
    }

    const Document* document_ = nullptr;
    ann::DimensionChain kind_;
    double scale_;
    Step step_ = Step::Base;
    EntityId base_ = 0;
    // Select was typed: Undo at the base prompt goes back to the base it had.
    bool chose_ = false;
    std::vector<ann::AnchoredPoint> points_;
    std::optional<double> spacing_;
};

} // namespace

std::unique_ptr<InteractiveTool> makeBaselineDimensionTool(const ToolContext& context)
{
    return std::make_unique<ChainTool>(context, ann::DimensionChain::Baseline);
}

std::unique_ptr<InteractiveTool> makeContinueDimensionTool(const ToolContext& context)
{
    return std::make_unique<ChainTool>(context, ann::DimensionChain::Continued);
}

} // namespace katana::cad::tools::annotate
