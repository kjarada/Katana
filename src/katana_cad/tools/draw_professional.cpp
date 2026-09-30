// The drawing system's draw tools (see families.hpp): the shapes a survey and
// civil drafting package draws beyond lines and circles - the 3D polyline,
// the spline, the ellipse and elliptical arc, construction lines and rays,
// the double line, freehand sketch and the revision cloud (docs/drawing.md,
// "Draw tools"). The Polyline tool's arc segments are in draw_lines.cpp,
// beside the tool they extend.
//
// Each follows the drafting habits of the commands of the same names -
// prompts, option letters, Undo and Enter - and hands the document ONE
// command, so one undo takes back the whole shape. What they make is stored
// by the drawing system's rule (docs/drawing.md, "Which kind a polyline
// is"): a straight 3D string is a Polyline2 with its heights in the
// elevation properties, exactly as an imported one.

#include <algorithm>
#include <array>
#include <limits>
#include <cmath>
#include <memory>
#include <optional>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include "families.hpp"
#include "katana/cad/drawing/construction.hpp"
#include "katana/cad/drawing/draw_shapes.hpp"
#include "katana/cad/drawing/vertex_editing.hpp"
#include "katana/commands/change_set.hpp"
#include "katana/commands/command_stack.hpp"
#include "katana/entity/entity_geometry.hpp"
#include "katana/core/text.hpp"
#include "katana/geometry/editing.hpp"
#include "katana/geometry/polyline_vertices.hpp"
#include "katana/math/numerics.hpp"

namespace katana::cad::tools {

namespace {

namespace cmd = katana::commands;
namespace geo = katana::geometry;
namespace tol = katana::math::tolerance;
using katana::core::Result;
using katana::entity::Entity;
using katana::entity::Geometry;
using katana::geometry::CurvePolyline2;
using katana::geometry::CurveVertex;
using katana::geometry::Ellipse2;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;
using katana::geometry::Spline2;
using katana::geometry::Vec2;

bool isOption(std::string_view text, std::string_view keyword)
{
    const std::string_view typed = katana::core::trimmed(text);
    return !typed.empty() && typed.size() <= keyword.size() &&
           katana::core::equalsIgnoringCase(typed, keyword.substr(0, typed.size()));
}

bool coincide(const Point2& a, const Point2& b) { return a.distanceTo(b) <= tol::kGeometric; }

std::string quoted(std::string_view text) { return "'" + std::string(katana::core::trimmed(text)) + "'"; }

std::string number(double value) { return katana::core::formatExactReal(value); }

Entity makeEntity(Geometry geometry, const cmd::EntityAttributes& attributes)
{
    return drawnEntity(std::move(geometry), attributes);
}

// Entities created by one command named for what was drawn.
cmd::CommandPtr createNamed(std::string name, std::vector<Entity> entities)
{
    return createDrawn(std::move(name), std::move(entities));
}

// A polyline entity stored as the simplest kind that holds it.
Result<Entity> polylineEntity(const CurvePolyline2& polyline, const cmd::EntityAttributes& attributes)
{
    return writePolyline(makeEntity(Geometry{}, attributes), polyline);
}

// ---- 3D Polyline ---------------------------------------------------------------------------
//
// A polyline whose every vertex may have a height: typed as x,y,z, taken from
// what the point snaps to (a surveyed point, a string's vertex, a line's end
// that has one), or the current height set with [Height]. A vertex with none
// has none - never zero.

// The current height, remembered by the program across the tool's restarts.
std::optional<double>& currentPolylineHeight()
{
    static std::optional<double> height;
    return height;
}

class Polyline3dTool final : public InteractiveTool {
  public:
    explicit Polyline3dTool(const ToolContext& context)
        : document_(context.document), view_(&viewOf(context)), attributes_(context.attributes),
          current_(currentPolylineHeight())
    {
    }

    [[nodiscard]] std::string prompt() const override
    {
        if (askingHeight_) {
            return "Specify the height for the next vertices, or [None]";
        }
        const std::string height =
            current_ ? " <height " + number(*current_) + ">" : std::string(" <no height>");
        if (vertices_.empty()) {
            return "Specify start point (x,y,z) or [Height]" + height;
        }
        return std::string("Specify next point (x,y,z) or [Height/") +
               (vertices_.size() >= 3 ? "Close/" : "") + "Undo]" + height;
    }
    [[nodiscard]] ToolInput expects() const override
    {
        return askingHeight_ ? ToolInput::Value : ToolInput::Point;
    }

    ToolStep point(const Point2& at) override
    {
        std::optional<double> height = current_;
        if (document_ != nullptr) {
            if (const auto found = heightAtPoint(*document_, at, 1.0e-6, *view_)) {
                height = found;
            }
        }
        return add(at, height);
    }

    ToolStep point3d(const Point2& at, double z) override { return add(at, z); }

    ToolStep value(std::string_view text) override
    {
        if (askingHeight_) {
            askingHeight_ = false;
            if (isOption(text, "None")) {
                current_.reset();
                currentPolylineHeight() = current_;
                return ToolStep::next("new vertices have no height");
            }
            const auto z = katana::core::parseFiniteDouble(katana::core::trimmed(text));
            if (!z) {
                askingHeight_ = true;
                return ToolStep::rejected(quoted(text) + " is not a height; type a number or None");
            }
            current_ = *z;
            currentPolylineHeight() = current_;
            return ToolStep::next("new vertices at height " + number(*z));
        }
        if (isOption(text, "Height")) {
            askingHeight_ = true;
            return ToolStep::next();
        }
        if (!vertices_.empty() && isOption(text, "Close")) {
            return finish(true);
        }
        if (isOption(text, "Undo")) {
            return undo();
        }
        return ToolStep::rejected(quoted(text) +
                                  " is not a point or an option; type x,y or x,y,z, H for the "
                                  "height, C to close or U to undo");
    }

    ToolStep enter() override
    {
        if (askingHeight_) {
            askingHeight_ = false;
            return ToolStep::next();
        }
        if (vertices_.size() < 2) {
            return ToolStep::done(nullptr);
        }
        return finish(false);
    }

    ToolStep cancel() override { return keepWorkOnEscape(*this); }

    ToolStep undo() override
    {
        if (askingHeight_) {
            askingHeight_ = false;
            return ToolStep::next();
        }
        if (vertices_.empty()) {
            return ToolStep::rejected("nothing to undo; no vertex has been given yet");
        }
        vertices_.pop_back();
        return ToolStep::next();
    }

    [[nodiscard]] ToolFeedback preview(const Point2& cursor) const override
    {
        ToolFeedback feedback;
        std::vector<Point2> points;
        for (const auto& v : vertices_) {
            points.push_back(v.position);
            feedback.markers.push_back(v.position);
        }
        if (!vertices_.empty() && !coincide(cursor, vertices_.back().position)) {
            points.push_back(cursor);
        }
        if (points.size() >= 2) {
            feedback.shapes.emplace_back(Polyline2{std::move(points), false});
        }
        return feedback;
    }

    [[nodiscard]] std::optional<Point2> lastPoint() const override
    {
        return vertices_.empty() ? std::nullopt : std::optional<Point2>(vertices_.back().position);
    }

    // The last vertex's height, which @dx,dy,dz climbs from.
    [[nodiscard]] std::optional<double> lastHeight() const override
    {
        return vertices_.empty() ? std::nullopt : vertices_.back().height;
    }

  private:
    ToolStep add(const Point2& at, std::optional<double> height)
    {
        if (!vertices_.empty() && coincide(at, vertices_.back().position)) {
            return ToolStep::rejected("the point is where the last vertex is");
        }
        vertices_.push_back(CurveVertex{at, 0.0, height});
        return ToolStep::next(height ? "height " + number(*height) : std::string("no height"));
    }

    ToolStep finish(bool closed)
    {
        CurvePolyline2 polyline;
        polyline.vertices = vertices_;
        polyline.closed = closed;
        if (closed && polyline.vertices.size() < 3) {
            return ToolStep::rejected("a closed polyline needs three vertices");
        }
        auto entity = polylineEntity(polyline, attributes_);
        if (!entity) {
            return ToolStep::rejected(entity.error().message);
        }
        const std::size_t count = vertices_.size();
        vertices_.clear();
        return ToolStep::done(createNamed("CREATE_POLYLINE_3D", {std::move(*entity)}),
                              "3D polyline of " + std::to_string(count) + " vertices", true);
    }

    const Document* document_ = nullptr;
    // Whose hidden layers a snapped point's height is not read through.
    const LayerOverrides* view_ = nullptr;
    cmd::EntityAttributes attributes_;
    std::vector<CurveVertex> vertices_;
    std::optional<double> current_;
    bool askingHeight_ = false;
};

// ---- Spline -----------------------------------------------------------------------------------
//
// Through the points given (Fit, the default) or with them as its control
// polygon (Control); [Degree] sets the degree, 3 unless changed.

class SplineTool final : public InteractiveTool {
  public:
    explicit SplineTool(const ToolContext& context) : attributes_(context.attributes) {}

    [[nodiscard]] std::string prompt() const override
    {
        if (askingDegree_) {
            return "Specify the degree (1 to 10) <" + std::to_string(degree_) + ">";
        }
        const std::string mode = control_ ? "control point" : "fit point";
        if (points_.empty()) {
            return "Specify first " + mode + " or [" + (control_ ? "Fit" : "Control") + "/Degree]";
        }
        return "Specify next " + mode + " or [Undo], Enter to finish";
    }
    [[nodiscard]] ToolInput expects() const override
    {
        return askingDegree_ ? ToolInput::Value : ToolInput::Point;
    }

    ToolStep point(const Point2& at) override
    {
        if (!points_.empty() && coincide(at, points_.back())) {
            return ToolStep::rejected("the point is where the last one is");
        }
        points_.push_back(at);
        return ToolStep::next();
    }

    ToolStep value(std::string_view text) override
    {
        if (askingDegree_) {
            const auto degree = katana::core::parseInteger(katana::core::trimmed(text));
            if (!degree || *degree < 1 || *degree > 10) {
                return ToolStep::rejected(quoted(text) + " is not a degree from 1 to 10");
            }
            degree_ = static_cast<int>(*degree);
            askingDegree_ = false;
            return ToolStep::next();
        }
        if (points_.empty() && isOption(text, "Control")) {
            control_ = true;
            return ToolStep::next();
        }
        if (points_.empty() && isOption(text, "Fit")) {
            control_ = false;
            return ToolStep::next();
        }
        if (isOption(text, "Degree")) {
            askingDegree_ = true;
            return ToolStep::next();
        }
        if (isOption(text, "Undo")) {
            return undo();
        }
        return ToolStep::rejected(quoted(text) + " is not a point or an option");
    }

    ToolStep enter() override
    {
        if (askingDegree_) {
            askingDegree_ = false;
            return ToolStep::next();
        }
        if (points_.size() < 2) {
            return ToolStep::done(nullptr);
        }
        auto spline = build(points_);
        if (!spline) {
            return ToolStep::rejected(spline.error().message);
        }
        const std::size_t count = points_.size();
        points_.clear();
        return ToolStep::done(
            createNamed("CREATE_SPLINE", {makeEntity(std::move(*spline), attributes_)}),
            "spline through " + std::to_string(count) + " points", true);
    }

    ToolStep cancel() override { return keepWorkOnEscape(*this); }

    ToolStep undo() override
    {
        if (askingDegree_) {
            askingDegree_ = false;
            return ToolStep::next();
        }
        if (points_.empty()) {
            return ToolStep::rejected("nothing to undo");
        }
        points_.pop_back();
        return ToolStep::next();
    }

    [[nodiscard]] ToolFeedback preview(const Point2& cursor) const override
    {
        ToolFeedback feedback;
        feedback.markers = points_;
        std::vector<Point2> points = points_;
        if (!points.empty() && !coincide(points.back(), cursor)) {
            points.push_back(cursor);
        }
        if (points.size() >= 2) {
            if (auto spline = build(points)) {
                feedback.shapes.emplace_back(std::move(*spline));
            }
            if (control_) {
                feedback.shapes.emplace_back(Polyline2{points, false});
            }
        }
        return feedback;
    }

    [[nodiscard]] std::optional<Point2> lastPoint() const override
    {
        return points_.empty() ? std::nullopt : std::optional<Point2>(points_.back());
    }

  private:
    [[nodiscard]] Result<Spline2> build(const std::vector<Point2>& points) const
    {
        return control_ ? Spline2::fromControlPoints(points, degree_)
                        : Spline2::throughPoints(points, degree_);
    }

    cmd::EntityAttributes attributes_;
    std::vector<Point2> points_;
    int degree_ = 3;
    bool control_ = false;
    bool askingDegree_ = false;
};

// ---- Ellipse, and the elliptical arc -----------------------------------------------------------
//
// Axis and End: the two ends of one axis, then the other axis's half length
// (a point - its distance from the first axis - or a typed number). Centre:
// the centre, one axis's end, the other half length. Arc: an ellipse as Axis
// and End, then the start and end, each a point (its direction from the
// centre) or an angle typed in degrees of the ellipse's own parameter.

class EllipseTool final : public InteractiveTool {
  public:
    enum class Variant { AxisEnd, Centre, Arc };

    EllipseTool(const ToolContext& context, Variant variant)
        : attributes_(context.attributes), variant_(variant)
    {
    }

    [[nodiscard]] std::string prompt() const override
    {
        switch (points_.size()) {
        case 0:
            return variant_ == Variant::Centre ? "Specify centre of ellipse"
                                               : "Specify axis endpoint of ellipse";
        case 1:
            return variant_ == Variant::Centre ? "Specify endpoint of axis"
                                               : "Specify other endpoint of axis";
        default:
            break;
        }
        if (!ellipse_) {
            return "Specify distance to other axis (a point or a length)";
        }
        return start_ ? "Specify end of arc (a point or an angle)"
                      : "Specify start of arc (a point or an angle)";
    }
    [[nodiscard]] ToolInput expects() const override { return ToolInput::Point; }

    ToolStep point(const Point2& at) override
    {
        if (points_.size() < 2) {
            if (!points_.empty() && coincide(at, points_.back())) {
                return ToolStep::rejected("an axis needs two different points");
            }
            points_.push_back(at);
            return ToolStep::next();
        }
        if (!ellipse_) {
            const auto [centre, axisEnd] = axis();
            const auto line = geo::Line2::through(centre, axisEnd);
            return other(line ? line->distanceTo(at) : 0.0);
        }
        return arcEnd(ellipse_->parameterTowards(at));
    }

    ToolStep value(std::string_view text) override
    {
        if (isOption(text, "Undo")) {
            return undo();
        }
        const auto typed = katana::core::parseFiniteDouble(katana::core::trimmed(text));
        if (points_.size() == 2 && !ellipse_ && typed) {
            return other(*typed);
        }
        if (ellipse_ && typed) {
            return arcEnd(*typed * katana::math::kDegToRad);
        }
        return ToolStep::rejected(quoted(text) + " is not a point or a number here");
    }

    ToolStep undo() override
    {
        if (start_) {
            start_.reset();
        } else if (ellipse_) {
            ellipse_.reset();
        } else if (!points_.empty()) {
            points_.pop_back();
        } else {
            return ToolStep::rejected("nothing to undo");
        }
        return ToolStep::next();
    }

    [[nodiscard]] ToolFeedback preview(const Point2& cursor) const override
    {
        ToolFeedback feedback;
        feedback.markers = points_;
        if (points_.size() == 1) {
            feedback.shapes.emplace_back(Segment2{points_[0], cursor});
        } else if (points_.size() == 2 && !ellipse_) {
            const auto [centre, axisEnd] = axis();
            if (const auto line = geo::Line2::through(centre, axisEnd)) {
                if (const auto shape = Ellipse2::fromAxes(centre, axisEnd, line->distanceTo(cursor))) {
                    feedback.shapes.emplace_back(*shape);
                }
            }
        } else if (ellipse_) {
            Ellipse2 shape = *ellipse_;
            const double t = ellipse_->parameterTowards(cursor);
            if (start_) {
                shape.startParameter = *start_;
                shape.sweep = katana::math::normalizeAngle(t - *start_);
                if (shape.sweep > tol::kAngular) {
                    feedback.shapes.emplace_back(shape);
                }
            } else {
                feedback.shapes.emplace_back(shape);
                feedback.shapes.emplace_back(Segment2{shape.center, shape.pointAtParameter(t)});
            }
        }
        return feedback;
    }

    [[nodiscard]] std::optional<Point2> lastPoint() const override
    {
        return points_.empty() ? std::nullopt : std::optional<Point2>(points_.back());
    }

  private:
    [[nodiscard]] std::pair<Point2, Point2> axis() const
    {
        if (variant_ == Variant::Centre) {
            return {points_[0], points_[1]};
        }
        return {(points_[0] + points_[1]) * 0.5, points_[1]};
    }

    ToolStep other(double radius)
    {
        const auto [centre, axisEnd] = axis();
        const auto shape = Ellipse2::fromAxes(centre, axisEnd, radius);
        if (!shape) {
            return ToolStep::rejected("the other axis needs a length greater than zero");
        }
        if (variant_ != Variant::Arc) {
            return make(*shape, "ellipse");
        }
        ellipse_ = *shape;
        return ToolStep::next();
    }

    ToolStep arcEnd(double parameter)
    {
        if (!start_) {
            start_ = parameter;
            return ToolStep::next();
        }
        Ellipse2 shape = *ellipse_;
        shape.startParameter = *start_;
        shape.sweep = katana::math::normalizeAngle(parameter - *start_);
        if (!(shape.sweep > tol::kAngular)) {
            return ToolStep::rejected("the arc's end is its start; pick another end");
        }
        return make(shape, "elliptical arc");
    }

    ToolStep make(const Ellipse2& shape, const std::string& what)
    {
        points_.clear();
        ellipse_.reset();
        start_.reset();
        return ToolStep::done(createNamed("CREATE_ELLIPSE", {makeEntity(shape, attributes_)}), what,
                              true);
    }

    cmd::EntityAttributes attributes_;
    Variant variant_;
    std::vector<Point2> points_;
    std::optional<Ellipse2> ellipse_;
    std::optional<double> start_;
};

// ---- Construction Line and Ray -----------------------------------------------------------------
//
// A base point, then through points, each making one line (both ways) or ray
// (away from the base), until Enter - as XLINE and RAY do. [Hor] and [Ver]
// make horizontal and vertical ones through each point picked. They go on
// the construction layer (drawing/construction.hpp), made if the drawing has
// none, which is never plotted.

class ConstructionTool final : public InteractiveTool {
  public:
    ConstructionTool(const ToolContext& context, bool ray)
        : document_(context.document), attributes_(context.attributes), ray_(ray)
    {
    }

    [[nodiscard]] std::string prompt() const override
    {
        if (!base_ && fixed_) {
            return std::string("Specify a point for the ") +
                   (*fixed_ == 0.0 ? "horizontal" : "vertical") + " line, Enter to finish";
        }
        if (!base_) {
            return ray_ ? "Specify start point" : "Specify a point or [Hor/Ver]";
        }
        return "Specify through point, Enter to finish";
    }
    [[nodiscard]] ToolInput expects() const override { return ToolInput::Point; }

    ToolStep point(const Point2& at) override
    {
        if (fixed_) {
            lines_.push_back(line(at, Vec2(std::cos(*fixed_), std::sin(*fixed_))));
            return ToolStep::next();
        }
        if (!base_) {
            base_ = at;
            return ToolStep::next();
        }
        if (coincide(at, *base_)) {
            return ToolStep::rejected("the through point is the base point");
        }
        lines_.push_back(line(*base_, (at - *base_).normalized()));
        return ToolStep::next();
    }

    ToolStep value(std::string_view text) override
    {
        if (!ray_ && !base_ && isOption(text, "Hor")) {
            fixed_ = 0.0;
            return ToolStep::next();
        }
        if (!ray_ && !base_ && isOption(text, "Ver")) {
            fixed_ = katana::math::kHalfPi;
            return ToolStep::next();
        }
        return ToolStep::rejected(quoted(text) + " is not a point or an option");
    }

    // Every line made since the tool started, as ONE command.
    ToolStep enter() override
    {
        if (lines_.empty()) {
            return ToolStep::done(nullptr);
        }
        return make();
    }

    ToolStep cancel() override { return keepWorkOnEscape(*this); }

    ToolStep undo() override
    {
        if (!lines_.empty()) {
            lines_.pop_back();
            return ToolStep::next();
        }
        if (base_) {
            base_.reset();
            return ToolStep::next();
        }
        if (fixed_) {
            fixed_.reset();
            return ToolStep::next();
        }
        return ToolStep::rejected("nothing to undo");
    }

    [[nodiscard]] ToolFeedback preview(const Point2& cursor) const override
    {
        ToolFeedback feedback;
        for (const Segment2& made : lines_) {
            feedback.shapes.emplace_back(made);
        }
        if (fixed_) {
            const Vec2 d(std::cos(*fixed_), std::sin(*fixed_));
            feedback.shapes.emplace_back(line(cursor, d));
        } else if (base_ && !coincide(cursor, *base_)) {
            feedback.markers.push_back(*base_);
            feedback.shapes.emplace_back(line(*base_, (cursor - *base_).normalized()));
        }
        return feedback;
    }

    [[nodiscard]] std::optional<Point2> lastPoint() const override { return base_; }

  private:
    [[nodiscard]] Segment2 line(const Point2& through, const Vec2& direction) const
    {
        return constructionSegment(through, direction, ray_);
    }

    ToolStep make()
    {
        const katana::entity::Model none;
        auto transaction = createConstruction(document_ != nullptr ? document_->model() : none,
                                              attributes_, lines_, ray_);
        const std::size_t count = lines_.size();
        lines_.clear();
        return ToolStep::done(std::move(transaction),
                              std::to_string(count) + (ray_ ? " ray" : " construction line") +
                                  (count == 1 ? "" : "s"));
    }

    const Document* document_ = nullptr;
    cmd::EntityAttributes attributes_;
    bool ray_ = false;
    std::optional<Point2> base_;
    std::optional<double> fixed_;
    std::vector<Segment2> lines_;
};

// ---- Double Line --------------------------------------------------------------------------------
//
// A path picked as a polyline is, drawn as the two polylines either side of
// it at half the width; [Width] sets the width (remembered). Enter or Close
// finishes.

double& doubleLineWidth()
{
    static double width = 1.0;
    return width;
}

class DoubleLineTool final : public InteractiveTool {
  public:
    explicit DoubleLineTool(const ToolContext& context) : attributes_(context.attributes) {}

    [[nodiscard]] std::string prompt() const override
    {
        if (askingWidth_) {
            return "Specify the width <" + number(doubleLineWidth()) + ">";
        }
        return std::string(points_.empty() ? "Specify start point" : "Specify next point") +
               " or [Width" + (points_.size() >= 3 ? "/Close" : "") + "/Undo] <width " +
               number(doubleLineWidth()) + ">";
    }
    [[nodiscard]] ToolInput expects() const override
    {
        return askingWidth_ ? ToolInput::Value : ToolInput::Point;
    }

    ToolStep point(const Point2& at) override
    {
        if (!points_.empty() && coincide(at, points_.back())) {
            return ToolStep::rejected("the point is where the last one is");
        }
        points_.push_back(at);
        return ToolStep::next();
    }

    ToolStep value(std::string_view text) override
    {
        if (askingWidth_) {
            const auto width = katana::core::parseFiniteDouble(katana::core::trimmed(text));
            if (!width || !(*width > tol::kGeometric)) {
                return ToolStep::rejected(quoted(text) + " is not a width greater than zero");
            }
            doubleLineWidth() = *width;
            askingWidth_ = false;
            return ToolStep::next();
        }
        if (isOption(text, "Width")) {
            askingWidth_ = true;
            return ToolStep::next();
        }
        if (points_.size() >= 3 && isOption(text, "Close")) {
            return finish(true);
        }
        if (isOption(text, "Undo")) {
            return undo();
        }
        return ToolStep::rejected(quoted(text) + " is not a point or an option");
    }

    ToolStep enter() override
    {
        if (askingWidth_) {
            askingWidth_ = false;
            return ToolStep::next();
        }
        if (points_.size() < 2) {
            return ToolStep::done(nullptr);
        }
        return finish(false);
    }

    ToolStep cancel() override { return keepWorkOnEscape(*this); }

    ToolStep undo() override
    {
        if (askingWidth_) {
            askingWidth_ = false;
            return ToolStep::next();
        }
        if (points_.empty()) {
            return ToolStep::rejected("nothing to undo");
        }
        points_.pop_back();
        return ToolStep::next();
    }

    [[nodiscard]] ToolFeedback preview(const Point2& cursor) const override
    {
        ToolFeedback feedback;
        std::vector<Point2> path = points_;
        if (!path.empty() && !coincide(path.back(), cursor)) {
            path.push_back(cursor);
        }
        for (const auto& side : sides(Polyline2{path, false})) {
            feedback.shapes.emplace_back(side);
        }
        feedback.markers = points_;
        return feedback;
    }

    [[nodiscard]] std::optional<Point2> lastPoint() const override
    {
        return points_.empty() ? std::nullopt : std::optional<Point2>(points_.back());
    }

  private:
    [[nodiscard]] static std::vector<Polyline2> sides(const Polyline2& path)
    {
        return doubleLineSides(path, doubleLineWidth());
    }

    ToolStep finish(bool closed)
    {
        const auto made = sides(Polyline2{points_, closed});
        if (made.size() != 2) {
            return ToolStep::rejected("the path is too tight for a double line of this width");
        }
        std::vector<Entity> entities;
        for (const auto& side : made) {
            entities.push_back(makeEntity(side, attributes_));
        }
        points_.clear();
        return ToolStep::done(createNamed("CREATE_DOUBLE_LINE", std::move(entities)),
                              "double line of width " + number(doubleLineWidth()), true);
    }

    cmd::EntityAttributes attributes_;
    std::vector<Point2> points_;
    bool askingWidth_ = false;
};

// ---- Freehand Sketch ----------------------------------------------------------------------------
//
// A click puts the pen down and the next lifts it; between them the cursor's
// path is recorded, and each stroke is weeded to the tolerance ([Tolerance],
// by default a quarter of the view's pick aperture, so a sketch is as fine as
// the zoom it was drawn at). Enter keeps the strokes, one polyline each.
//
// The path is recorded from preview(), which the view calls with the cursor
// on every move: the tool interface has no move event of its own, and
// preview is the one call that sees the cursor between clicks. The recording
// is the tool's own state, so it is mutable there.

class SketchTool final : public InteractiveTool {
  public:
    explicit SketchTool(const ToolContext& context)
        : attributes_(context.attributes),
          tolerance_(context.pickTolerance > 0.0 ? context.pickTolerance * 0.25 : 0.01)
    {
    }

    [[nodiscard]] std::string prompt() const override
    {
        if (askingTolerance_) {
            return "Specify the sketch tolerance <" + number(tolerance_) + ">";
        }
        return penDown_ ? "Move to sketch; click to lift the pen"
                        : "Click to put the pen down, or [Tolerance]; Enter keeps the sketch";
    }
    [[nodiscard]] ToolInput expects() const override
    {
        return askingTolerance_ ? ToolInput::Value : ToolInput::Point;
    }

    ToolStep point(const Point2& at) override
    {
        if (!penDown_) {
            penDown_ = true;
            stroke_ = {at};
            return ToolStep::next();
        }
        record(at);
        penDown_ = false;
        if (stroke_.size() >= 2) {
            strokes_.push_back(stroke_);
        }
        stroke_.clear();
        return ToolStep::next();
    }

    ToolStep value(std::string_view text) override
    {
        if (askingTolerance_) {
            const auto t = katana::core::parseFiniteDouble(katana::core::trimmed(text));
            if (!t || !(*t > 0.0)) {
                return ToolStep::rejected(quoted(text) + " is not a tolerance greater than zero");
            }
            tolerance_ = *t;
            askingTolerance_ = false;
            return ToolStep::next();
        }
        if (isOption(text, "Tolerance")) {
            askingTolerance_ = true;
            return ToolStep::next();
        }
        return ToolStep::rejected(quoted(text) + " is not an option here");
    }

    ToolStep enter() override
    {
        if (askingTolerance_) {
            askingTolerance_ = false;
            return ToolStep::next();
        }
        if (penDown_ && stroke_.size() >= 2) {
            strokes_.push_back(stroke_);
        }
        penDown_ = false;
        stroke_.clear();
        std::vector<Entity> made;
        for (const auto& stroke : strokes_) {
            const auto weeded = geo::weed(CurvePolyline2::fromPoints(stroke), tolerance_);
            if (weeded && weeded->vertices.size() >= 2) {
                Polyline2 line{weeded->positions(), false};
                if (katana::entity::validate(Geometry{line})) {
                    made.push_back(makeEntity(std::move(line), attributes_));
                }
            }
        }
        strokes_.clear();
        if (made.empty()) {
            return ToolStep::done(nullptr);
        }
        const std::size_t count = made.size();
        return ToolStep::done(createNamed("CREATE_SKETCH", std::move(made)),
                              std::to_string(count) + (count == 1 ? " stroke" : " strokes"), false);
    }

    ToolStep cancel() override { return keepWorkOnEscape(*this); }

    ToolStep undo() override
    {
        if (penDown_) {
            penDown_ = false;
            stroke_.clear();
            return ToolStep::next();
        }
        if (strokes_.empty()) {
            return ToolStep::rejected("nothing to undo");
        }
        strokes_.pop_back();
        return ToolStep::next();
    }

    [[nodiscard]] ToolFeedback preview(const Point2& cursor) const override
    {
        if (penDown_) {
            record(cursor);
        }
        ToolFeedback feedback;
        for (const auto& stroke : strokes_) {
            feedback.shapes.emplace_back(Polyline2{stroke, false});
        }
        if (stroke_.size() >= 2) {
            feedback.shapes.emplace_back(Polyline2{stroke_, false});
        }
        return feedback;
    }

    [[nodiscard]] std::optional<Point2> lastPoint() const override
    {
        return stroke_.empty() ? std::nullopt : std::optional<Point2>(stroke_.back());
    }

    // For the tests, which have no mouse: the pen moving through `at`.
    void recordMove(const Point2& at) { record(at); }

  private:
    void record(const Point2& at) const
    {
        if (stroke_.empty() || stroke_.back().distanceTo(at) > tol::kGeometric) {
            stroke_.push_back(at);
        }
    }

    cmd::EntityAttributes attributes_;
    double tolerance_;
    bool penDown_ = false;
    bool askingTolerance_ = false;
    mutable std::vector<Point2> stroke_;
    std::vector<std::vector<Point2>> strokes_;
};

// ---- Revision Cloud -----------------------------------------------------------------------------
//
// The outline picked point by point (or a [Rectangle] by two corners), then
// drawn as a closed curve polyline of arcs about [Arc] length long, bulging
// outwards. Enter or Close finishes.

double& cloudArcLength()
{
    static double length = 1.0;
    return length;
}

CurvePolyline2 cloudAround(const std::vector<Point2>& outline, double arcLength)
{
    CurvePolyline2 ring;
    ring.closed = true;
    const Polyline2 polygon{outline, true};
    // Outwards is to the right of a counter-clockwise ring, which a positive
    // bulge bulges to; to the left of a clockwise one.
    const double bulge = polygon.signedArea() >= 0.0 ? 0.6 : -0.6;
    for (std::size_t i = 0; i < outline.size(); ++i) {
        const Point2& a = outline[i];
        const Point2& b = outline[(i + 1) % outline.size()];
        const double length = a.distanceTo(b);
        const std::size_t pieces =
            std::max<std::size_t>(1, static_cast<std::size_t>(std::round(length / arcLength)));
        for (std::size_t k = 0; k < pieces; ++k) {
            ring.vertices.push_back(
                CurveVertex{a + (b - a) * (static_cast<double>(k) / static_cast<double>(pieces)),
                            bulge, std::nullopt});
        }
    }
    return ring;
}

class RevisionCloudTool final : public InteractiveTool {
  public:
    explicit RevisionCloudTool(const ToolContext& context) : attributes_(context.attributes) {}

    [[nodiscard]] std::string prompt() const override
    {
        if (askingLength_) {
            return "Specify the arc length <" + number(cloudArcLength()) + ">";
        }
        if (rectangle_) {
            return points_.empty() ? "Specify first corner" : "Specify other corner";
        }
        return std::string(points_.empty() ? "Specify start point or [Arc/Rectangle]"
                                           : "Specify next point or [Arc/Undo]") +
               (points_.size() >= 3 ? ", Enter or C to close" : "");
    }
    [[nodiscard]] ToolInput expects() const override
    {
        return askingLength_ ? ToolInput::Value : ToolInput::Point;
    }

    ToolStep point(const Point2& at) override
    {
        if (!points_.empty() && coincide(at, points_.back())) {
            return ToolStep::rejected("the point is where the last one is");
        }
        if (rectangle_ && !points_.empty()) {
            const auto box = geo::Rectangle2::fromCorners(points_[0], at);
            if (!(box.width > tol::kGeometric && box.height > tol::kGeometric)) {
                return ToolStep::rejected("the corners are in line");
            }
            const auto corners = box.corners();
            points_.assign(corners.begin(), corners.end());
            return finish();
        }
        points_.push_back(at);
        return ToolStep::next();
    }

    ToolStep value(std::string_view text) override
    {
        if (askingLength_) {
            const auto length = katana::core::parseFiniteDouble(katana::core::trimmed(text));
            if (!length || !(*length > tol::kGeometric)) {
                return ToolStep::rejected(quoted(text) + " is not a length greater than zero");
            }
            cloudArcLength() = *length;
            askingLength_ = false;
            return ToolStep::next();
        }
        if (isOption(text, "Arc")) {
            askingLength_ = true;
            return ToolStep::next();
        }
        if (points_.empty() && isOption(text, "Rectangle")) {
            rectangle_ = true;
            return ToolStep::next();
        }
        if (points_.size() >= 3 && isOption(text, "Close")) {
            return finish();
        }
        if (isOption(text, "Undo")) {
            return undo();
        }
        return ToolStep::rejected(quoted(text) + " is not a point or an option");
    }

    ToolStep enter() override
    {
        if (askingLength_) {
            askingLength_ = false;
            return ToolStep::next();
        }
        if (points_.size() >= 3) {
            return finish();
        }
        return ToolStep::done(nullptr);
    }

    ToolStep undo() override
    {
        if (askingLength_) {
            askingLength_ = false;
        } else if (!points_.empty()) {
            points_.pop_back();
        } else if (rectangle_) {
            rectangle_ = false;
        } else {
            return ToolStep::rejected("nothing to undo");
        }
        return ToolStep::next();
    }

    [[nodiscard]] ToolFeedback preview(const Point2& cursor) const override
    {
        ToolFeedback feedback;
        feedback.markers = points_;
        std::vector<Point2> outline = points_;
        if (rectangle_ && points_.size() == 1) {
            const auto corners = geo::Rectangle2::fromCorners(points_[0], cursor).corners();
            outline.assign(corners.begin(), corners.end());
        } else if (!outline.empty() && !coincide(outline.back(), cursor)) {
            outline.push_back(cursor);
        }
        if (outline.size() >= 3) {
            feedback.shapes.emplace_back(cloudAround(outline, cloudArcLength()));
        } else if (outline.size() == 2) {
            feedback.shapes.emplace_back(Segment2{outline[0], outline[1]});
        }
        return feedback;
    }

    [[nodiscard]] std::optional<Point2> lastPoint() const override
    {
        return points_.empty() ? std::nullopt : std::optional<Point2>(points_.back());
    }

  private:
    ToolStep finish()
    {
        CurvePolyline2 cloud = cloudAround(points_, cloudArcLength());
        points_.clear();
        rectangle_ = false;
        if (auto status = katana::entity::validate(Geometry{cloud}); !status) {
            return ToolStep::rejected(status.error().message);
        }
        return ToolStep::done(createNamed("CREATE_REVISION_CLOUD", {makeEntity(cloud, attributes_)}),
                              "revision cloud of " + std::to_string(cloud.vertices.size()) + " arcs",
                              true);
    }

    cmd::EntityAttributes attributes_;
    std::vector<Point2> points_;
    bool rectangle_ = false;
    bool askingLength_ = false;
};

// ---- Circle, Tangent Tangent Tangent; Arc, Start End Direction -------------------------------------
//
// The two constructions the Circle and Arc families lacked, named into those
// families so the menus put them in the same submenus.
//
// TTT: three lines picked (lines, or the straight segment of a polyline
// nearest the pick); of the four circles touching all three (the triangle's
// incircle and its three excircles) the one whose touching points lie
// nearest the three picks.

std::optional<Segment2> pickedLine(const Document* document, katana::entity::EntityId id,
                                   const Point2& at)
{
    const Entity* entity = document != nullptr ? document->model().entities.find(id) : nullptr;
    if (entity == nullptr) {
        return std::nullopt;
    }
    if (const auto* line = std::get_if<Segment2>(&entity->geometry)) {
        return *line;
    }
    if (const auto polyline = readPolyline(*entity)) {
        if (const auto segment = geo::nearestSegment(*polyline, at)) {
            if (const auto* straight = std::get_if<Segment2>(&std::as_const(*polyline).segments()[*segment])) {
                return *straight;
            }
        }
    }
    return std::nullopt;
}

std::optional<katana::geometry::Circle2> tangentToThree(const std::array<Segment2, 3>& lines,
                                                        const std::array<Point2, 3>& picks)
{
    std::optional<katana::geometry::Circle2> best;
    double bestScore = std::numeric_limits<double>::infinity();
    for (int signs = 0; signs < 8; ++signs) {
        // n_i . c + k_i = s_i r for unit normals n_i: three equations in
        // (cx, cy, r), solved by Cramer's rule.
        double m[3][3];
        double rhs[3];
        for (int i = 0; i < 3; ++i) {
            const Vec2 n = lines[static_cast<std::size_t>(i)].delta().normalized().perpendicular();
            const double k = -n.dot(lines[static_cast<std::size_t>(i)].start);
            const double s = (signs >> i) & 1 ? 1.0 : -1.0;
            m[i][0] = n.x;
            m[i][1] = n.y;
            m[i][2] = -s;
            rhs[i] = -k;
        }
        const auto det = [](double a[3][3]) {
            return a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1]) -
                   a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0]) +
                   a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0]);
        };
        const double d = det(m);
        if (std::abs(d) < 1e-12) {
            continue;
        }
        double solution[3];
        for (int column = 0; column < 3; ++column) {
            double replaced[3][3];
            for (int i = 0; i < 3; ++i) {
                for (int j = 0; j < 3; ++j) {
                    replaced[i][j] = j == column ? rhs[i] : m[i][j];
                }
            }
            solution[column] = det(replaced) / d;
        }
        const double r = solution[2];
        if (!(r > tol::kGeometric)) {
            continue;
        }
        const Point2 c(solution[0], solution[1]);
        double score = 0.0;
        for (std::size_t i = 0; i < 3; ++i) {
            const geo::Line2 line{lines[i].start, lines[i].delta()};
            score += line.closestPoint(c).distanceTo(picks[i]);
        }
        if (score < bestScore) {
            bestScore = score;
            best = katana::geometry::Circle2{c, r};
        }
    }
    return best;
}

class CircleTangentsTool final : public InteractiveTool {
  public:
    explicit CircleTangentsTool(const ToolContext& context)
        : document_(context.document), attributes_(context.attributes)
    {
    }

    [[nodiscard]] std::string prompt() const override
    {
        static const char* const ordinal[] = {"first", "second", "third"};
        return std::string("Select the ") + ordinal[std::min<std::size_t>(lines_.size(), 2)] +
               " line the circle touches";
    }
    [[nodiscard]] ToolInput expects() const override { return ToolInput::Entity; }

    ToolStep entity(katana::entity::EntityId id, const Point2& at) override
    {
        const auto line = pickedLine(document_, id, at);
        if (!line || line->isDegenerate()) {
            return ToolStep::rejected("pick a line, or a straight segment of a polyline");
        }
        lines_.push_back(*line);
        picks_.push_back(at);
        if (lines_.size() < 3) {
            return ToolStep::next();
        }
        const auto circle = tangentToThree({lines_[0], lines_[1], lines_[2]},
                                           {picks_[0], picks_[1], picks_[2]});
        lines_.clear();
        picks_.clear();
        if (!circle) {
            return ToolStep::rejected("no circle touches those three lines (two are parallel?)");
        }
        return ToolStep::done(createNamed("CREATE_CIRCLE", {makeEntity(*circle, attributes_)}),
                              "circle of radius " + number(circle->radius), true);
    }

    ToolStep undo() override
    {
        if (lines_.empty()) {
            return ToolStep::rejected("nothing to undo");
        }
        lines_.pop_back();
        picks_.pop_back();
        return ToolStep::next();
    }

  private:
    const Document* document_ = nullptr;
    cmd::EntityAttributes attributes_;
    std::vector<Segment2> lines_;
    std::vector<Point2> picks_;
};

// Start End Direction: the start, the end, then the direction the arc leaves
// its start - a point it heads towards, or an angle typed in degrees
// counter-clockwise from east.
class ArcDirectionTool final : public InteractiveTool {
  public:
    explicit ArcDirectionTool(const ToolContext& context) : attributes_(context.attributes) {}

    [[nodiscard]] std::string prompt() const override
    {
        switch (points_.size()) {
        case 0:
            return "Specify start point of arc";
        case 1:
            return "Specify end point of arc";
        default:
            return "Specify tangent direction at the start (a point or an angle)";
        }
    }
    [[nodiscard]] ToolInput expects() const override { return ToolInput::Point; }

    ToolStep point(const Point2& at) override
    {
        if (!points_.empty() && coincide(at, points_.back())) {
            return ToolStep::rejected("the point is where the last one is");
        }
        if (points_.size() < 2) {
            points_.push_back(at);
            return ToolStep::next();
        }
        return make((at - points_[0]).angle());
    }

    ToolStep value(std::string_view text) override
    {
        const auto degrees = katana::core::parseFiniteDouble(katana::core::trimmed(text));
        if (points_.size() == 2 && degrees) {
            return make(*degrees * katana::math::kDegToRad);
        }
        return ToolStep::rejected(quoted(text) + " is not taken here");
    }

    ToolStep undo() override
    {
        if (points_.empty()) {
            return ToolStep::rejected("nothing to undo");
        }
        points_.pop_back();
        return ToolStep::next();
    }

    [[nodiscard]] ToolFeedback preview(const Point2& cursor) const override
    {
        ToolFeedback feedback;
        feedback.markers = points_;
        if (points_.size() == 1) {
            feedback.shapes.emplace_back(Segment2{points_[0], cursor});
        } else if (points_.size() == 2 && !coincide(cursor, points_[0])) {
            if (const auto arc = arcFor((cursor - points_[0]).angle())) {
                feedback.shapes.emplace_back(*arc);
            }
        }
        return feedback;
    }

    [[nodiscard]] std::optional<Point2> lastPoint() const override
    {
        return points_.empty() ? std::nullopt : std::optional<Point2>(points_.back());
    }

  private:
    [[nodiscard]] std::optional<katana::geometry::Arc2> arcFor(double direction) const
    {
        const Vec2 tangent(std::cos(direction), std::sin(direction));
        const Vec2 chord = points_[1] - points_[0];
        const double half = std::atan2(tangent.cross(chord), tangent.dot(chord));
        return geo::arcFromBulge(points_[0], points_[1], geo::bulgeFromSweep(2.0 * half));
    }

    ToolStep make(double direction)
    {
        const auto arc = arcFor(direction);
        if (!arc) {
            return ToolStep::rejected("that direction points straight at the end; there is no arc");
        }
        points_.clear();
        return ToolStep::done(createNamed("CREATE_ARC", {makeEntity(*arc, attributes_)}), "arc", true);
    }

    cmd::EntityAttributes attributes_;
    std::vector<Point2> points_;
};

// ---- registration ---------------------------------------------------------------------------------

template <typename Make>
ToolInfo info(std::string id, std::string name, std::string group, int order,
              std::vector<std::string> aliases, std::string tip, Make make)
{
    ToolInfo tool;
    tool.id = std::move(id);
    tool.name = std::move(name);
    tool.category = "Draw";
    tool.group = std::move(group);
    tool.order = order;
    tool.aliases = std::move(aliases);
    tool.tip = std::move(tip);
    tool.make = std::move(make);
    return tool;
}

} // namespace

// For the tests: the Sketch tool's pen moving through a point.
void sketchMoveForTests(InteractiveTool& tool, const Point2& at)
{
    if (auto* sketch = dynamic_cast<SketchTool*>(&tool)) {
        sketch->recordMove(at);
    }
}

void addDrawProfessionalTools(ToolCatalog& catalog, const Report& report)
{
    using Tool = std::unique_ptr<InteractiveTool>;
    report(catalog.add(info(
        "draw.polyline3d", "3D Polyline", "Lines", 35, {"PLINE3D", "3DPOLY", "3DPOLYLINE"},
        "Draws a polyline with a height at each vertex: typed as x,y,z, taken from what the point "
        "snaps to, or the current height set with H.",
        [](const ToolContext& c) -> Tool { return std::make_unique<Polyline3dTool>(c); })));
    report(catalog.add(info(
        "draw.spline", "Spline", "Curves", 70, {"SPLINE", "SPL"},
        "Draws a spline through the points you give, or with them as its control points (C); "
        "Enter finishes.",
        [](const ToolContext& c) -> Tool { return std::make_unique<SplineTool>(c); })));
    report(catalog.add(info(
        "draw.ellipse", "Ellipse, Axis and End", "Curves", 80, {"ELLIPSE", "EL"},
        "Draws an ellipse from the two ends of one axis and the other axis's half length.",
        [](const ToolContext& c) -> Tool {
            return std::make_unique<EllipseTool>(c, EllipseTool::Variant::AxisEnd);
        })));
    report(catalog.add(info(
        "draw.ellipse.centre", "Ellipse, Centre", "Curves", 81, {},
        "Draws an ellipse from its centre, one axis's end and the other axis's half length.",
        [](const ToolContext& c) -> Tool {
            return std::make_unique<EllipseTool>(c, EllipseTool::Variant::Centre);
        })));
    report(catalog.add(info(
        "draw.ellipse.arc", "Ellipse, Arc", "Curves", 82, {"ELLIPSEARC"},
        "Draws an elliptical arc: an ellipse by its axis, then the arc's start and end.",
        [](const ToolContext& c) -> Tool {
            return std::make_unique<EllipseTool>(c, EllipseTool::Variant::Arc);
        })));
    report(catalog.add(info(
        "draw.xline", "Construction Line", "Lines", 60, {"XLINE", "XL"},
        "Draws construction lines through a base point, on the construction layer, which is "
        "never plotted; H and V make them horizontal or vertical.",
        [](const ToolContext& c) -> Tool { return std::make_unique<ConstructionTool>(c, false); })));
    report(catalog.add(info(
        "draw.ray", "Ray", "Lines", 61, {"RAY"},
        "Draws rays from a start point through each point you pick, on the construction layer, "
        "which is never plotted.",
        [](const ToolContext& c) -> Tool { return std::make_unique<ConstructionTool>(c, true); })));
    report(catalog.add(info(
        "draw.dline", "Double Line", "Lines", 70, {"DLINE", "DL"},
        "Draws both sides of a path at a width (W sets it) as two polylines.",
        [](const ToolContext& c) -> Tool { return std::make_unique<DoubleLineTool>(c); })));
    report(catalog.add(info(
        "draw.sketch", "Freehand Sketch", "Lines", 80, {"SKETCH"},
        "Records the cursor's path between two clicks and keeps it, weeded to a tolerance, as "
        "polylines when you press Enter.",
        [](const ToolContext& c) -> Tool { return std::make_unique<SketchTool>(c); })));
    report(catalog.add(info(
        "draw.circle.ttt", "Circle, Tangent Tangent Tangent", "Curves", 55, {},
        "Draws the circle touching three lines, the one whose touching points are nearest your "
        "picks.",
        [](const ToolContext& c) -> Tool { return std::make_unique<CircleTangentsTool>(c); })));
    report(catalog.add(info(
        "draw.arc.sed", "Arc, Start End Direction", "Curves", 95, {},
        "Draws an arc from its start to its end leaving the start in a direction you give.",
        [](const ToolContext& c) -> Tool { return std::make_unique<ArcDirectionTool>(c); })));
    report(catalog.add(info(
        "draw.revcloud", "Revision Cloud", "Lines", 90, {"REVCLOUD"},
        "Draws a cloud of arcs round an outline you pick, or round a rectangle (R), to mark a "
        "change.",
        [](const ToolContext& c) -> Tool { return std::make_unique<RevisionCloudTool>(c); })));
}

} // namespace katana::cad::tools
