// Angular, radius, diameter and ordinate dimensions, as AutoCAD's DIMANGULAR,
// DIMRADIUS, DIMDIAMETER and DIMORDINATE take them:
//
//   Angular   Select the first line, or press Enter to specify the vertex
//               Select the second line                 two lines picked, or
//               Specify angle vertex                   three points given
//               Specify first angle endpoint
//               Specify second angle endpoint
//             Specify dimension arc line location      the side measured
//   Radius    Select an arc or circle
//             Specify dimension line location          its direction and reach
//   Diameter  the same
//   Ordinate  Specify feature location or [Datum]
//             Specify leader endpoint or [Xdatum/Ydatum]
//
// Each makes the dimension the DIM verb makes (DIM ANGULAR, RADIUS, DIAMETER,
// ORDINATE), through the same builders (cad/annotation/dimension_build.hpp),
// so a tool and a typed verb cannot disagree about what was measured: one
// DimensionGeometry of its kind, drawn by dimension_draw.cpp in the layer's
// dimension style, added by ONE command. A dimension made from picked
// entities - two lines, an arc or a circle - follows them when they are
// edited (the associative update), and so does one whose points were snapped
// to an entity's end, middle, centre or vertex (anchoredPoint), as DIM's #id
// points do; one made from typed or plainly clicked points measures those
// points.
//
// The ordinate's datum is the drawing's origin, as AutoCAD's is the UCS
// origin, until Datum gives another - DIM ORDINATE ... datum=x,y. The tool
// starts again after each dimension, and measures from the datum of the
// drawing's newest ordinate dimension: the datum given is kept for the rest
// of the run, and the drawing, not the program, keeps it, as it keeps the
// height the Text tool offers.

#include <cmath>
#include <optional>
#include <string>
#include <variant>
#include <vector>

#include "annotate_common.hpp"
#include "katana/cad/annotation/dimension_build.hpp"
#include "katana/cad/document.hpp"
#include "katana/entity/entity.hpp"
#include "katana/math/numerics.hpp"

namespace katana::cad::tools::annotate {

namespace {

using katana::cad::annotation::AnchoredPoint;
using katana::core::Result;
using katana::entity::AnchorRef;
using katana::entity::DimensionGeometry;
using katana::entity::DimensionKind;
using katana::entity::EntityId;
using katana::geometry::Point2;
using katana::geometry::Segment2;

// What the finished dimension says it measured, as the other dimension
// tools do: "angular dimension measuring 90 degrees", "radius dimension
// measuring 3".
std::string measuredMessage(const DimensionGeometry& dimension)
{
    const double value = dimension.measurement();
    switch (dimension.kind) {
    case DimensionKind::Angular:
        return "angular dimension measuring " +
               formatNumber(value * katana::math::kRadToDeg) + " degrees";
    case DimensionKind::Radius:
        return "radius dimension measuring " + formatNumber(value);
    case DimensionKind::Diameter:
        return "diameter dimension measuring " + formatNumber(value);
    case DimensionKind::OrdinateX:
        return "X ordinate dimension measuring " + formatNumber(value);
    case DimensionKind::OrdinateY:
        return "Y ordinate dimension measuring " + formatNumber(value);
    case DimensionKind::Aligned:
    case DimensionKind::Linear:
        break;
    }
    return "dimension measuring " + formatNumber(value);
}

ToolStep finished(const Result<DimensionGeometry>& dimension,
                  const katana::commands::EntityAttributes& attributes)
{
    if (!dimension) {
        return ToolStep::rejected(dimension.error().message);
    }
    return ToolStep::done(katana::commands::createDimension(*dimension, attributes),
                          measuredMessage(*dimension), /*restart=*/true);
}

// ---- Angular ------------------------------------------------------------------------

class AngularDimensionTool final : public InteractiveTool {
  public:
    explicit AngularDimensionTool(const ToolContext& context)
        : document_(context.document), attributes_(context.attributes)
    {
    }

    [[nodiscard]] std::string prompt() const override
    {
        switch (step_) {
        case Step::FirstLine:
            return "Select the first line, or press Enter to specify the vertex";
        case Step::SecondLine:
            return "Select the second line or [Undo]";
        case Step::Vertex:
            return "Specify angle vertex or [Undo]";
        case Step::FirstPoint:
            return "Specify first angle endpoint or [Undo]";
        case Step::SecondPoint:
            return "Specify second angle endpoint or [Undo]";
        case Step::Location:
            return "Specify dimension arc line location or [Undo]";
        }
        return {};
    }

    [[nodiscard]] ToolInput expects() const override
    {
        return step_ == Step::FirstLine || step_ == Step::SecondLine ? ToolInput::Entity
                                                                     : ToolInput::Point;
    }

    ToolStep entity(EntityId id, const Point2& at) override
    {
        if (step_ != Step::FirstLine && step_ != Step::SecondLine) {
            return InteractiveTool::entity(id, at);
        }
        const katana::entity::Entity* picked =
            document_ == nullptr ? nullptr : document_->model().entities.find(id);
        if (picked == nullptr || !std::holds_alternative<Segment2>(picked->geometry)) {
            return ToolStep::rejected("pick a line; press Enter to give the vertex and two "
                                      "points instead");
        }
        if (step_ == Step::FirstLine) {
            firstLine_ = id;
            step_ = Step::SecondLine;
            return ToolStep::next();
        }
        if (id == firstLine_) {
            return ToolStep::rejected("that is the first line; pick the other one");
        }
        secondLine_ = id;
        byLines_ = true;
        step_ = Step::Location;
        return ToolStep::next();
    }

    ToolStep point(const Point2& at) override
    {
        switch (step_) {
        case Step::Vertex:
            vertex_ = {at, pending_};
            step_ = Step::FirstPoint;
            return ToolStep::next();
        case Step::FirstPoint:
            if (coincident(vertex_.point, at)) {
                return ToolStep::rejected("that point is the vertex; an angle's arm needs a length");
            }
            first_ = {at, pending_};
            step_ = Step::SecondPoint;
            return ToolStep::next();
        case Step::SecondPoint:
            if (coincident(vertex_.point, at)) {
                return ToolStep::rejected("that point is the vertex; an angle's arm needs a length");
            }
            second_ = {at, pending_};
            byLines_ = false;
            step_ = Step::Location;
            return ToolStep::next();
        case Step::Location:
            return finished(place(at), attributes_);
        case Step::FirstLine:
        case Step::SecondLine:
            break;
        }
        return InteractiveTool::point(at);
    }

    ToolStep anchoredPoint(const Point2& at, const AnchorRef& anchor) override
    {
        pending_ = anchor;
        ToolStep step = point(at);
        pending_ = {};
        return step;
    }

    ToolStep value(std::string_view text) override
    {
        if (isOption(text, "Undo")) {
            return undo();
        }
        return ToolStep::rejected(step_ == Step::FirstLine || step_ == Step::SecondLine
                                      ? "pick a line in the drawing"
                                      : "click the point or type it as x,y");
    }

    ToolStep enter() override
    {
        if (step_ == Step::FirstLine) {
            step_ = Step::Vertex;
            return ToolStep::next();
        }
        if (step_ == Step::Vertex) {
            step_ = Step::FirstLine;
            return ToolStep::next();
        }
        return ToolStep::rejected(expects() == ToolInput::Entity ? "pick the second line"
                                                                 : "specify the point asked for");
    }

    ToolStep undo() override
    {
        switch (step_) {
        case Step::FirstLine:
            return ToolStep::rejected("nothing to undo: nothing has been picked");
        case Step::SecondLine:
        case Step::Vertex:
            step_ = Step::FirstLine;
            return ToolStep::next();
        case Step::FirstPoint:
            step_ = Step::Vertex;
            return ToolStep::next();
        case Step::SecondPoint:
            step_ = Step::FirstPoint;
            return ToolStep::next();
        case Step::Location:
            step_ = byLines_ ? Step::SecondLine : Step::SecondPoint;
            return ToolStep::next();
        }
        return InteractiveTool::undo();
    }

    [[nodiscard]] ToolFeedback preview(const Point2& cursor) const override
    {
        ToolFeedback feedback;
        switch (step_) {
        case Step::FirstPoint:
            feedback.markers.push_back(vertex_.point);
            if (!coincident(vertex_.point, cursor)) {
                feedback.shapes.emplace_back(Segment2{vertex_.point, cursor});
            }
            break;
        case Step::SecondPoint:
            feedback.markers.push_back(vertex_.point);
            feedback.shapes.emplace_back(Segment2{vertex_.point, first_.point});
            if (!coincident(vertex_.point, cursor)) {
                feedback.shapes.emplace_back(Segment2{vertex_.point, cursor});
            }
            break;
        case Step::Location:
            if (auto dimension = place(cursor)) {
                feedback.shapes.emplace_back(*dimension);
            }
            break;
        case Step::FirstLine:
        case Step::SecondLine:
        case Step::Vertex:
            break;
        }
        return feedback;
    }

    [[nodiscard]] std::optional<Point2> lastPoint() const override
    {
        switch (step_) {
        case Step::FirstPoint:
            return vertex_.point;
        case Step::SecondPoint:
            return first_.point;
        case Step::Location:
            return byLines_ ? std::nullopt : std::optional<Point2>(second_.point);
        case Step::FirstLine:
        case Step::SecondLine:
        case Step::Vertex:
            break;
        }
        return std::nullopt;
    }

  private:
    enum class Step { FirstLine, SecondLine, Vertex, FirstPoint, SecondPoint, Location };

    [[nodiscard]] Result<DimensionGeometry> place(const Point2& at) const
    {
        if (byLines_) {
            return katana::cad::annotation::angularBetweenLines(document_->model(), firstLine_,
                                                                secondLine_, at);
        }
        return katana::cad::annotation::angularDimension(vertex_, first_, second_, at);
    }

    const Document* document_ = nullptr;
    katana::commands::EntityAttributes attributes_;
    Step step_ = Step::FirstLine;
    EntityId firstLine_ = 0;
    EntityId secondLine_ = 0;
    bool byLines_ = false;
    // Each with the entity point it was snapped to, if any.
    AnchoredPoint vertex_;
    AnchoredPoint first_;
    AnchoredPoint second_;
    // Set only while anchoredPoint hands a snapped point to point().
    AnchorRef pending_{};
};

// ---- Radius and diameter ---------------------------------------------------------------

class RadialDimensionTool final : public InteractiveTool {
  public:
    RadialDimensionTool(const ToolContext& context, bool diameter)
        : document_(context.document), attributes_(context.attributes), diameter_(diameter)
    {
    }

    [[nodiscard]] std::string prompt() const override
    {
        return curve_ == 0 ? "Select an arc or circle"
                           : "Specify dimension line location or [Undo]";
    }

    [[nodiscard]] ToolInput expects() const override
    {
        return curve_ == 0 ? ToolInput::Entity : ToolInput::Point;
    }

    ToolStep entity(EntityId id, const Point2& at) override
    {
        if (curve_ != 0) {
            return InteractiveTool::entity(id, at);
        }
        const katana::entity::Entity* picked =
            document_ == nullptr ? nullptr : document_->model().entities.find(id);
        if (picked == nullptr ||
            !(std::holds_alternative<katana::geometry::Arc2>(picked->geometry) ||
              std::holds_alternative<katana::geometry::Circle2>(picked->geometry))) {
            return ToolStep::rejected("pick an arc or a circle");
        }
        curve_ = id;
        return ToolStep::next();
    }

    ToolStep point(const Point2& at) override
    {
        if (curve_ == 0) {
            return InteractiveTool::point(at);
        }
        return finished(place(at), attributes_);
    }

    ToolStep value(std::string_view text) override
    {
        if (isOption(text, "Undo")) {
            return undo();
        }
        return ToolStep::rejected(curve_ == 0 ? "pick an arc or a circle in the drawing"
                                              : "click where the dimension line goes");
    }

    ToolStep undo() override
    {
        if (curve_ == 0) {
            return ToolStep::rejected("nothing to undo: nothing has been picked");
        }
        curve_ = 0;
        return ToolStep::next();
    }

    [[nodiscard]] ToolFeedback preview(const Point2& cursor) const override
    {
        ToolFeedback feedback;
        if (curve_ != 0) {
            if (auto dimension = place(cursor)) {
                feedback.shapes.emplace_back(*dimension);
            }
        }
        return feedback;
    }

  private:
    [[nodiscard]] Result<DimensionGeometry> place(const Point2& at) const
    {
        return katana::cad::annotation::radialDimension(document_->model(), curve_, at, diameter_);
    }

    const Document* document_ = nullptr;
    katana::commands::EntityAttributes attributes_;
    bool diameter_ = false;
    EntityId curve_ = 0;
};

// ---- Ordinate --------------------------------------------------------------------------

// The datum of the drawing's newest ordinate dimension, with the entity
// point it follows; the drawing's origin when there is none.
AnchoredPoint newestOrdinateDatum(const katana::entity::Model& model)
{
    AnchoredPoint datum{Point2(0.0, 0.0), {}};
    model.entities.forEach([&](const katana::entity::Entity& entity) {
        const auto* dimension = std::get_if<DimensionGeometry>(&entity.geometry);
        if (dimension != nullptr && (dimension->kind == DimensionKind::OrdinateX ||
                                     dimension->kind == DimensionKind::OrdinateY)) {
            datum = {dimension->vertex, dimension->vertexRef};
        }
    });
    return datum;
}

std::string pointText(const Point2& point)
{
    return formatNumber(point.x) + "," + formatNumber(point.y);
}

class OrdinateDimensionTool final : public InteractiveTool {
  public:
    explicit OrdinateDimensionTool(const ToolContext& context)
        : attributes_(context.attributes),
          datum_(context.document != nullptr ? newestOrdinateDatum(context.document->model())
                                             : AnchoredPoint{Point2(0.0, 0.0), {}})
    {
    }

    [[nodiscard]] std::string prompt() const override
    {
        if (askingDatum_) {
            return "Specify the datum point <" + pointText(datum_.point) + ">";
        }
        if (!feature_) {
            // The datum is said while it is not the origin, since nothing on
            // screen shows what the ordinates are measured from.
            const bool origin = datum_.point == Point2(0.0, 0.0) && !datum_.ref.associated();
            return origin ? "Specify feature location or [Datum]"
                          : "Specify feature location or [Datum] (datum " +
                                pointText(datum_.point) + ")";
        }
        if (axis_) {
            return std::string("Specify leader endpoint of the ") + (*axis_ ? "X" : "Y") +
                   " ordinate or [Xdatum/Ydatum/Undo]";
        }
        return "Specify leader endpoint or [Xdatum/Ydatum/Undo]";
    }

    [[nodiscard]] ToolInput expects() const override { return ToolInput::Point; }

    ToolStep point(const Point2& at) override
    {
        if (askingDatum_) {
            datum_ = {at, pending_};
            askingDatum_ = false;
            return ToolStep::next();
        }
        if (!feature_) {
            feature_ = AnchoredPoint{at, pending_};
            return ToolStep::next();
        }
        return finished(place(at), attributes_);
    }

    // A feature or a datum snapped to an entity's point follows it, as DIM
    // ORDINATE #id.end ... datum=#id.centre does; the leader's end does not.
    ToolStep anchoredPoint(const Point2& at, const AnchorRef& anchor) override
    {
        pending_ = anchor;
        ToolStep step = point(at);
        pending_ = {};
        return step;
    }

    ToolStep value(std::string_view text) override
    {
        if (isOption(text, "Undo")) {
            return undo();
        }
        if (askingDatum_) {
            return ToolStep::rejected("click the datum or type it as x,y, or press Enter to keep "
                                      "it");
        }
        if (!feature_ && isOption(text, "Datum")) {
            askingDatum_ = true;
            return ToolStep::next();
        }
        if (feature_ && isOption(text, "Xdatum")) {
            axis_ = true;
            return ToolStep::next();
        }
        if (feature_ && isOption(text, "Ydatum")) {
            axis_ = false;
            return ToolStep::next();
        }
        return ToolStep::rejected(!feature_ ? "click the feature or type it as x,y, or type D for "
                                              "the datum"
                                            : "click the leader's end, or type X or Y to choose "
                                              "the ordinate");
    }

    ToolStep enter() override
    {
        if (askingDatum_) {
            askingDatum_ = false; // the datum shown is kept
            return ToolStep::next();
        }
        return InteractiveTool::enter();
    }

    ToolStep undo() override
    {
        if (askingDatum_) {
            askingDatum_ = false;
            return ToolStep::next();
        }
        if (axis_) {
            axis_.reset();
            return ToolStep::next();
        }
        if (feature_) {
            feature_.reset();
            return ToolStep::next();
        }
        return ToolStep::rejected("nothing to undo: no feature has been given");
    }

    [[nodiscard]] ToolFeedback preview(const Point2& cursor) const override
    {
        ToolFeedback feedback;
        if (feature_ && !askingDatum_) {
            feedback.markers.push_back(feature_->point);
            if (auto dimension = place(cursor)) {
                feedback.shapes.emplace_back(*dimension);
            }
        }
        return feedback;
    }

    [[nodiscard]] std::optional<Point2> lastPoint() const override
    {
        return feature_ ? std::optional<Point2>(feature_->point) : std::nullopt;
    }

  private:
    [[nodiscard]] Result<DimensionGeometry> place(const Point2& at) const
    {
        return katana::cad::annotation::ordinateDimension(datum_, *feature_, at, axis_);
    }

    katana::commands::EntityAttributes attributes_;
    AnchoredPoint datum_;
    bool askingDatum_ = false;
    std::optional<AnchoredPoint> feature_;
    std::optional<bool> axis_; // true: X; empty: from the leader's direction
    // Set only while anchoredPoint hands a snapped point to point().
    AnchorRef pending_{};
};

} // namespace

std::unique_ptr<InteractiveTool> makeAngularDimensionTool(const ToolContext& context)
{
    return std::make_unique<AngularDimensionTool>(context);
}

std::unique_ptr<InteractiveTool> makeRadiusDimensionTool(const ToolContext& context)
{
    return std::make_unique<RadialDimensionTool>(context, false);
}

std::unique_ptr<InteractiveTool> makeDiameterDimensionTool(const ToolContext& context)
{
    return std::make_unique<RadialDimensionTool>(context, true);
}

std::unique_ptr<InteractiveTool> makeOrdinateDimensionTool(const ToolContext& context)
{
    return std::make_unique<OrdinateDimensionTool>(context);
}

} // namespace katana::cad::tools::annotate
