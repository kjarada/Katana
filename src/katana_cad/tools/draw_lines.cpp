// Point, Line, Polyline, Rectangle, Polygon (see families.hpp): the Draw menu's
// Lines group, the tools that make straight-edged geometry.
//
// They follow AutoCAD's commands of the same names, because that is what a
// CAD user's hands already know: the same prompts, the same option letters,
// Close and Undo inside LINE and PLINE, Enter to finish, @dx,dy and
// @distance<angle measured from the last point. Each one hands the document a
// single command, so one undo takes back a whole chain of lines rather than
// its last segment.

#include <cmath>
#include <cstddef>
#include <cstdint>
#include <memory>
#include <optional>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include "families.hpp"
#include "katana/cad/drawing/draw_shapes.hpp"
#include "katana/commands/change_set.hpp"
#include "katana/core/text.hpp"
#include "katana/math/numerics.hpp"

namespace katana::cad::tools {

namespace {

namespace cmd = katana::commands;
namespace tol = katana::math::tolerance;
using katana::core::Result;
using katana::entity::Entity;
using katana::entity::PointGeometry;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Rectangle2;
using katana::geometry::Segment2;
using katana::geometry::Vec2;

// ---- shared by the tools -----------------------------------------------------------

// An option typed at a prompt: the whole keyword or any leading part of it, in
// any case, as AutoCAD takes "C", "Cl" or "close" for [Close]. Every prompt
// here offers options with different first letters, so one letter is never
// ambiguous.
bool isOption(std::string_view text, std::string_view keyword)
{
    const std::string_view typed = katana::core::trimmed(text);
    return !typed.empty() && typed.size() <= keyword.size() &&
           katana::core::equalsIgnoringCase(typed, keyword.substr(0, typed.size()));
}

// The geometry kernel's own test for "the same point" (primitives2d.hpp), so a
// tool never accepts a point that would make geometry validation refuse the
// command it returns.
bool coincide(const Point2& a, const Point2& b) { return a.distanceTo(b) <= tol::kGeometric; }

std::string quoted(std::string_view text)
{
    return "'" + std::string(katana::core::trimmed(text)) + "'";
}

// A typed length (a radius, a rectangle's side): a finite number greater than
// the geometric tolerance, or a sentence saying why not.
Result<double> parseLength(std::string_view text, std::string_view what)
{
    const auto value = katana::core::parseFiniteDouble(katana::core::trimmed(text));
    if (!value) {
        return katana::core::makeError(katana::core::ErrorCode::ParseFailure,
                                       quoted(text) + " is not a number; type the " +
                                           std::string(what) + " as a number");
    }
    if (!(*value > tol::kGeometric)) {
        return katana::core::makeError(katana::core::ErrorCode::InvalidArgument,
                                       "the " + std::string(what) + " must be greater than zero");
    }
    return *value;
}

// Why typed text was not taken at a Line or Polyline prompt, naming what
// would be: the relative forms only once there is a point to measure them
// from, and Close only once the prompt offers it.
ToolStep notAChainPoint(std::string_view text, bool started, bool canClose)
{
    std::string why = quoted(text) + " is not a point or an option; type ";
    why += started ? "x,y, @dx,dy or @distance<angle" : "the point as x,y";
    if (canClose) {
        why += ", or C to close";
    }
    return ToolStep::rejected(std::move(why));
}

Entity makeEntity(katana::entity::Geometry geometry, const cmd::EntityAttributes& attributes)
{
    Entity entity;
    entity.geometry = std::move(geometry);
    entity.layer = attributes.layer;
    entity.style = attributes.style;
    entity.color = attributes.color;
    return entity;
}

// Several entities created by ONE command, so a single undo removes all of
// them. createEntities would do, but it is named CREATE_ENTITIES (an import's
// name); the command log and the undo menu should say what was drawn.
cmd::CommandPtr createAll(std::string name, std::vector<Entity> entities)
{
    return std::make_unique<cmd::ChangeSetCommand>(
        std::move(name),
        // Copied, not moved, out of the capture: validate() may run the
        // builder more than once (see createEntities in entity_commands.cpp).
        [entities = std::move(entities)](const cmd::CommandContext&) -> Result<cmd::ChangeSet> {
            cmd::ChangeSet changes;
            changes.add = entities;
            return changes;
        });
}

// ---- Point -------------------------------------------------------------------------

// A point with a height (the drawing system's, docs/drawing.md): typed as
// x,y,z, or placed at the current height set with [Height], which the
// program remembers as it remembers a fillet radius (the tool restarts after
// every point, so the tool itself cannot keep it); its height goes in the
// elevation property every survey point already carries.
std::optional<double>& currentPointHeight()
{
    static std::optional<double> height;
    return height;
}

class PointTool final : public InteractiveTool {
  public:
    explicit PointTool(const ToolContext& context)
        : attributes_(context.attributes), height_(currentPointHeight())
    {
    }

    [[nodiscard]] std::string prompt() const override
    {
        if (askingHeight_) {
            return "Specify the height for the next points, or [None]";
        }
        return height_ ? "Specify a point (x,y,z) or [Height] <height " +
                             katana::core::formatExactReal(*height_) + ">"
                       : std::string("Specify a point (x,y,z) or [Height]");
    }
    [[nodiscard]] ToolInput expects() const override
    {
        return askingHeight_ ? ToolInput::Value : ToolInput::Point;
    }

    ToolStep point(const Point2& at) override { return place(at, height_); }

    ToolStep point3d(const Point2& at, double z) override { return place(at, z); }
    [[nodiscard]] bool takesHeights() const override { return true; }

    ToolStep value(std::string_view text) override
    {
        if (askingHeight_) {
            askingHeight_ = false;
            if (isOption(text, "None")) {
                height_.reset();
                currentPointHeight() = height_;
                return ToolStep::next();
            }
            const auto z = katana::core::parseFiniteDouble(katana::core::trimmed(text));
            if (!z) {
                askingHeight_ = true;
                return ToolStep::rejected(quoted(text) + " is not a height; type a number or None");
            }
            height_ = *z;
            currentPointHeight() = height_;
            return ToolStep::next();
        }
        if (isOption(text, "Height")) {
            askingHeight_ = true;
            return ToolStep::next();
        }
        return ToolStep::rejected(quoted(text) +
                                  " is not a point; type it as x,y or x,y,z, or H for a height");
    }

    [[nodiscard]] ToolFeedback preview(const Point2& cursor) const override
    {
        ToolFeedback feedback;
        feedback.shapes.emplace_back(PointGeometry{cursor});
        return feedback;
    }

  private:
    ToolStep place(const Point2& at, std::optional<double> height)
    {
        // One command per point, so each is its own undo, and a restart so the
        // tool goes on placing points until Esc - survey marks come in runs.
        if (!height) {
            return ToolStep::done(cmd::createPoint(at, attributes_), {}, true);
        }
        Entity point = makeEntity(PointGeometry{at}, attributes_);
        katana::entity::setHeights(point.properties, {height});
        return ToolStep::done(createAll("CREATE_POINT", {std::move(point)}), {}, true);
    }

    cmd::EntityAttributes attributes_;
    std::optional<double> height_;
    bool askingHeight_ = false;
};

// ---- Line --------------------------------------------------------------------------
//
// A chain of SEPARATE lines, one per pair of consecutive points, as LINE
// draws; Polyline is the tool for one connected entity. The chain is kept
// here until Enter or Close and then created by one command.

class LineTool final : public InteractiveTool {
  public:
    explicit LineTool(const ToolContext& context) : attributes_(context.attributes) {}

    [[nodiscard]] std::string prompt() const override
    {
        if (points_.empty()) {
            return "Specify first point";
        }
        // Close is offered once there are two segments to close: with one,
        // closing would draw the same line back again.
        return canClose() ? "Specify next point or [Close/Undo]" : "Specify next point or [Undo]";
    }
    [[nodiscard]] ToolInput expects() const override { return ToolInput::Point; }

    ToolStep point(const Point2& at) override
    {
        if (!points_.empty() && coincide(at, points_.back())) {
            return ToolStep::rejected(
                "the point is where the last one is; a line needs two different points");
        }
        points_.push_back(at);
        return ToolStep::next();
    }

    ToolStep value(std::string_view text) override
    {
        if (!points_.empty() && isOption(text, "Close")) {
            return close();
        }
        if (isOption(text, "Undo")) {
            return undo();
        }
        return notAChainPoint(text, !points_.empty(), canClose());
    }

    ToolStep enter() override
    {
        if (points_.size() < 2) {
            // No segment yet: Enter ends the tool, as it ends LINE.
            return ToolStep::done(nullptr);
        }
        return finish();
    }

    // Esc keeps the chain drawn so far, as AutoCAD keeps a LINE's segments.
    ToolStep cancel() override { return keepWorkOnEscape(*this); }

    ToolStep undo() override
    {
        if (points_.empty()) {
            return ToolStep::rejected("nothing to undo; no point has been given yet");
        }
        points_.pop_back();
        return ToolStep::next();
    }

    [[nodiscard]] ToolFeedback preview(const Point2& cursor) const override
    {
        ToolFeedback feedback;
        // The segments given so far are not in the drawing until the chain is
        // finished, so the preview draws them as well as the rubber band.
        for (std::size_t i = 1; i < points_.size(); ++i) {
            feedback.shapes.emplace_back(Segment2{points_[i - 1], points_[i]});
        }
        if (!points_.empty() && !coincide(cursor, points_.back())) {
            feedback.shapes.emplace_back(Segment2{points_.back(), cursor});
        }
        feedback.markers = points_;
        return feedback;
    }

    [[nodiscard]] std::optional<Point2> lastPoint() const override
    {
        return points_.empty() ? std::nullopt : std::optional<Point2>(points_.back());
    }

  private:
    [[nodiscard]] bool canClose() const { return points_.size() >= 3; }

    ToolStep close()
    {
        if (!canClose()) {
            return ToolStep::rejected(
                "Close needs two lines first; there is nothing to close back to yet");
        }
        if (coincide(points_.back(), points_.front())) {
            return ToolStep::rejected(
                "the chain already ends at its first point; press Enter to finish");
        }
        points_.push_back(points_.front());
        return finish();
    }

    ToolStep finish()
    {
        const std::size_t count = points_.size() - 1;
        cmd::CommandPtr command;
        if (count == 1) {
            command = cmd::createLine(points_[0], points_[1], attributes_);
        } else {
            std::vector<Entity> lines;
            lines.reserve(count);
            for (std::size_t i = 1; i < points_.size(); ++i) {
                lines.push_back(makeEntity(Segment2{points_[i - 1], points_[i]}, attributes_));
            }
            // The interpreter's LINE names its chain the same way.
            command = createAll("CREATE_LINES", std::move(lines));
        }
        points_.clear();
        return ToolStep::done(std::move(command),
                              count == 1 ? "1 line" : std::to_string(count) + " lines", true);
    }

    cmd::EntityAttributes attributes_;
    std::vector<Point2> points_;
};

// ---- Polyline ----------------------------------------------------------------------
//
// PLINE with its segment modes (the drawing system's, docs/drawing.md): in
// Line mode a point is the end of a straight segment; in Arc mode it is the
// end of an arc TANGENT to the segment before (to the east for the first),
// as a CAD user draws a road or a kerb, and [Second] makes the next arc pass
// through a point instead, then end at the next. Close, Undo and Enter as
// before. A polyline with any arc is a CurvePolyline2; one with none is the
// Polyline2 it always was.

class PolylineTool final : public InteractiveTool {
  public:
    explicit PolylineTool(const ToolContext& context) : attributes_(context.attributes) {}

    [[nodiscard]] std::string prompt() const override
    {
        if (vertices_.empty()) {
            return "Specify start point";
        }
        if (through_) {
            return "Specify end point of arc";
        }
        std::string options = arc_ ? "Line/Second" : "Arc";
        if (canClose()) {
            options += "/Close";
        }
        options += "/Undo";
        return std::string(arc_ ? "Specify end point of arc" : "Specify next point") + " or [" +
               options + "]";
    }
    [[nodiscard]] ToolInput expects() const override { return ToolInput::Point; }

    ToolStep point(const Point2& at) override
    {
        if (!vertices_.empty() && coincide(at, vertices_.back().position)) {
            return ToolStep::rejected(
                "the point is where the last vertex is; the next vertex must be somewhere else");
        }
        if (arc_ && !vertices_.empty()) {
            if (secondPending_) {
                through_ = at;
                secondPending_ = false;
                return ToolStep::next();
            }
            double bulge = 0.0;
            if (through_) {
                bulge = katana::geometry::bulgeThrough(vertices_.back().position, *through_, at);
                through_.reset();
            } else {
                bulge = tangentBulge(at);
            }
            vertices_.back().bulge = bulge;
        }
        vertices_.push_back(katana::geometry::CurveVertex{at, 0.0, std::nullopt});
        history_.push_back(arc_);
        return ToolStep::next();
    }

    ToolStep value(std::string_view text) override
    {
        if (!vertices_.empty() && isOption(text, "Close")) {
            return close();
        }
        if (isOption(text, "Undo")) {
            return undo();
        }
        if (isOption(text, "Arc")) {
            arc_ = true;
            return ToolStep::next();
        }
        if (isOption(text, "Line")) {
            arc_ = false;
            through_.reset();
            secondPending_ = false;
            return ToolStep::next();
        }
        if (arc_ && isOption(text, "Second") && !vertices_.empty()) {
            secondPending_ = true;
            return ToolStep::next();
        }
        return notAChainPoint(text, !vertices_.empty(), canClose());
    }

    ToolStep enter() override
    {
        if (vertices_.size() < 2) {
            return ToolStep::done(nullptr);
        }
        return finish(false);
    }

    // Esc keeps the chain drawn so far, as AutoCAD keeps a LINE's segments.
    ToolStep cancel() override { return keepWorkOnEscape(*this); }

    ToolStep undo() override
    {
        if (secondPending_ || through_) {
            secondPending_ = false;
            through_.reset();
            return ToolStep::next();
        }
        if (vertices_.empty()) {
            return ToolStep::rejected("nothing to undo; no vertex has been given yet");
        }
        vertices_.pop_back();
        history_.pop_back();
        if (!vertices_.empty()) {
            vertices_.back().bulge = 0.0; // the segment it started is gone
        }
        return ToolStep::next();
    }

    [[nodiscard]] ToolFeedback preview(const Point2& cursor) const override
    {
        ToolFeedback feedback;
        for (const auto& vertex : vertices_) {
            feedback.markers.push_back(vertex.position);
        }
        if (vertices_.empty()) {
            return feedback;
        }
        katana::geometry::CurvePolyline2 shape;
        shape.vertices = vertices_;
        if (through_) {
            feedback.markers.push_back(*through_);
        }
        if (!coincide(cursor, vertices_.back().position) && !secondPending_) {
            if (arc_) {
                shape.vertices.back().bulge =
                    through_ ? katana::geometry::bulgeThrough(vertices_.back().position,
                                                              *through_, cursor)
                             : tangentBulge(cursor);
            }
            shape.vertices.push_back(katana::geometry::CurveVertex{cursor, 0.0, std::nullopt});
        }
        if (shape.vertices.size() >= 2) {
            if (shape.hasArcs()) {
                feedback.shapes.emplace_back(std::move(shape));
            } else {
                feedback.shapes.emplace_back(Polyline2{shape.positions(), false});
            }
        }
        return feedback;
    }

    [[nodiscard]] std::optional<Point2> lastPoint() const override
    {
        return vertices_.empty() ? std::nullopt
                                 : std::optional<Point2>(vertices_.back().position);
    }

  private:
    // A closed polyline needs three vertices; with two, Close would only
    // double the one segment back on itself.
    [[nodiscard]] bool canClose() const { return vertices_.size() >= 3; }

    // The direction the chain is heading at its last vertex: the last
    // segment's tangent at its end, east before there is one.
    // The bulge of the arc from the last vertex to `to`, tangent to the
    // segment before it (cad::tangentBulge, shared with PLINE ... ARC).
    [[nodiscard]] double tangentBulge(const Point2& to) const
    {
        katana::geometry::CurvePolyline2 path;
        path.vertices = vertices_;
        return katana::cad::tangentBulge(path, to);
    }

    ToolStep close()
    {
        if (!canClose()) {
            return ToolStep::rejected("Close needs three vertices; give another point first");
        }
        // A user who snapped back onto the start and then chose Close means
        // one closed shape, not a closed shape with a doubled first vertex.
        // Checked on a copy, so a refusal leaves the chain as it was.
        if (coincide(vertices_.back().position, vertices_.front().position)) {
            if (vertices_.size() - 1 < 3) {
                return ToolStep::rejected(
                    "a closed polyline needs three different vertices; give another point first");
            }
            vertices_.pop_back();
            history_.pop_back();
            vertices_.back().bulge = 0.0;
        } else if (arc_) {
            // In Arc mode the closing segment is a tangent arc too.
            vertices_.back().bulge = tangentBulge(vertices_.front().position);
        }
        return finish(true);
    }

    ToolStep finish(bool closed)
    {
        katana::geometry::CurvePolyline2 shape;
        shape.vertices = std::move(vertices_);
        shape.closed = closed;
        if (!closed) {
            shape.vertices.back().bulge = 0.0;
        }
        vertices_.clear();
        history_.clear();
        through_.reset();
        secondPending_ = false;
        const std::size_t count = shape.vertices.size();
        cmd::CommandPtr command;
        if (shape.hasArcs()) {
            command = createAll("CREATE_POLYLINE", {makeEntity(shape, attributes_)});
        } else {
            command = cmd::createPolyline(Polyline2{shape.positions(), closed}, attributes_);
        }
        return ToolStep::done(std::move(command),
                              std::string(closed ? "closed polyline of " : "polyline of ") +
                                  std::to_string(count) + " vertices",
                              true);
    }

    cmd::EntityAttributes attributes_;
    std::vector<katana::geometry::CurveVertex> vertices_;
    std::vector<bool> history_; // the mode each vertex was given in
    bool arc_ = false;
    bool secondPending_ = false;
    std::optional<Point2> through_;
};

// ---- Rectangle ---------------------------------------------------------------------
//
// Two opposite corners, as the interpreter's RECT takes them, so the second
// is also typed as @width,height from the first. [Dimensions] is RECTANG's
// other way: type the length along x and the width along y, then click on
// the side of the first corner the rectangle goes.

class RectangleTool final : public InteractiveTool {
  public:
    explicit RectangleTool(const ToolContext& context) : attributes_(context.attributes) {}

    [[nodiscard]] std::string prompt() const override
    {
        switch (step_) {
        case Step::FirstCorner:
            return "Specify first corner point";
        case Step::OtherCorner:
            return "Specify other corner point or [Dimensions/Undo]";
        case Step::Length:
            return "Specify length of the rectangle along x or [Undo]";
        case Step::Width:
            return "Specify width of the rectangle along y or [Undo]";
        case Step::Side:
            return "Specify a point on the side the rectangle goes or [Undo] <up and right>";
        }
        return {};
    }

    [[nodiscard]] ToolInput expects() const override
    {
        return step_ == Step::Length || step_ == Step::Width ? ToolInput::Value : ToolInput::Point;
    }

    ToolStep point(const Point2& at) override
    {
        switch (step_) {
        case Step::FirstCorner:
            corner_ = at;
            step_ = Step::OtherCorner;
            return ToolStep::next();
        case Step::OtherCorner:
            return make(Rectangle2::fromCorners(corner_, at));
        case Step::Side:
            return make(sized(at));
        case Step::Length:
        case Step::Width:
            break;
        }
        return InteractiveTool::point(at);
    }

    ToolStep value(std::string_view text) override
    {
        if (isOption(text, "Undo")) {
            return undo();
        }
        switch (step_) {
        case Step::FirstCorner:
            return ToolStep::rejected(quoted(text) + " is not a point; type the corner as x,y");
        case Step::OtherCorner:
            if (isOption(text, "Dimensions")) {
                step_ = Step::Length;
                return ToolStep::next();
            }
            return ToolStep::rejected(quoted(text) +
                                      " is not a point; type the other corner as x,y or as "
                                      "@width,height from the first, or D to give the sizes");
        case Step::Length: {
            auto length = parseLength(text, "length");
            if (!length) {
                return ToolStep::rejected(length.error().message);
            }
            length_ = *length;
            step_ = Step::Width;
            return ToolStep::next();
        }
        case Step::Width: {
            auto width = parseLength(text, "width");
            if (!width) {
                return ToolStep::rejected(width.error().message);
            }
            width_ = *width;
            step_ = Step::Side;
            return ToolStep::next();
        }
        case Step::Side:
            break;
        }
        return ToolStep::rejected(quoted(text) +
                                  " is not a point; click on the side the rectangle goes, or "
                                  "press Enter for up and right");
    }

    ToolStep enter() override
    {
        if (step_ == Step::Side) {
            // The default side, as the prompt says: the first corner is the
            // lower left one.
            return make(Rectangle2{corner_, length_, width_});
        }
        if (step_ == Step::Length || step_ == Step::Width) {
            return ToolStep::rejected(std::string("type the ") +
                                      (step_ == Step::Length ? "length" : "width") +
                                      " of the rectangle, or U to go back");
        }
        return ToolStep::done(nullptr);
    }

    ToolStep undo() override
    {
        switch (step_) {
        case Step::FirstCorner:
            return ToolStep::rejected("nothing to undo; no corner has been given yet");
        case Step::OtherCorner:
            step_ = Step::FirstCorner;
            break;
        case Step::Length:
            step_ = Step::OtherCorner;
            break;
        case Step::Width:
            step_ = Step::Length;
            break;
        case Step::Side:
            step_ = Step::Width;
            break;
        }
        return ToolStep::next();
    }

    [[nodiscard]] ToolFeedback preview(const Point2& cursor) const override
    {
        ToolFeedback feedback;
        if (step_ == Step::FirstCorner) {
            return feedback;
        }
        feedback.markers.push_back(corner_);
        if (step_ == Step::OtherCorner) {
            const Rectangle2 rectangle = Rectangle2::fromCorners(corner_, cursor);
            if (hasArea(rectangle)) {
                feedback.shapes.emplace_back(rectangle.toPolyline());
            } else if (!coincide(corner_, cursor)) {
                // In line with the corner: the rectangle has collapsed to its
                // one visible side, which is still worth seeing.
                feedback.shapes.emplace_back(Segment2{corner_, cursor});
            }
        } else if (step_ == Step::Side) {
            feedback.shapes.emplace_back(sized(cursor).toPolyline());
        }
        return feedback;
    }

    [[nodiscard]] std::optional<Point2> lastPoint() const override
    {
        return step_ == Step::FirstCorner ? std::nullopt : std::optional<Point2>(corner_);
    }

  private:
    enum class Step { FirstCorner, OtherCorner, Length, Width, Side };

    [[nodiscard]] static bool hasArea(const Rectangle2& rectangle)
    {
        return rectangle.width > tol::kGeometric && rectangle.height > tol::kGeometric;
    }

    // The typed length x width rectangle at the first corner, on the side of
    // it that `towards` is. On the corner's own x or y the side is taken as
    // up or right, the same as Enter.
    [[nodiscard]] Rectangle2 sized(const Point2& towards) const
    {
        const Vec2 size(towards.x < corner_.x ? -length_ : length_,
                        towards.y < corner_.y ? -width_ : width_);
        return Rectangle2::fromCorners(corner_, corner_ + size);
    }

    ToolStep make(const Rectangle2& rectangle)
    {
        if (!hasArea(rectangle)) {
            return ToolStep::rejected(
                "the corners are in line; a rectangle needs both a width and a height");
        }
        // Rectangle2's outline, as the interpreter's RECT makes it:
        // counter-clockwise from the lower left corner.
        step_ = Step::FirstCorner;
        return ToolStep::done(cmd::createPolyline(rectangle.toPolyline(), attributes_),
                              "rectangle", true);
    }

    cmd::EntityAttributes attributes_;
    Step step_ = Step::FirstCorner;
    Point2 corner_;
    double length_ = 0.0;
    double width_ = 0.0;
};

// ---- Polygon -----------------------------------------------------------------------
//
// A regular polygon, as POLYGON draws it: the number of sides, then either
// the centre and a radius or [Edge] and the two ends of one side. The radius
// is to a vertex when the polygon is Inscribed in the circle (the default)
// and to the midpoint of a side when it is Circumscribed about it; a picked
// radius point is ON the polygon either way - the first vertex, or the middle
// of the first side - so it can be snapped to something that the polygon
// must touch. A typed radius draws the bottom side level, as AutoCAD does.
//
// Unlike POLYGON, the number of sides has no <4> default taken by Enter. The
// tool restarts after each polygon, as the other tools here do, so its first
// prompt is where a user lands when done; Enter there has to end the tool,
// as it does in Point, Line, Polyline and Rectangle, or a right-click to
// leave would arm the next two clicks to draw a square.

constexpr int kMinimumSides = 3;
constexpr int kMaximumSides = 1024;

// The vertices, counter-clockwise, of the regular `sides`-gon about `centre`
// whose first vertex is at centre + toFirst. Each is the first rotated about
// the centre, rather than the previous one rotated again, so rounding does not
// accumulate around a 1024-gon, and the first is exactly the point it was
// derived from.
std::vector<Point2> regularPolygon(const Point2& centre, const Vec2& toFirst, int sides)
{
    std::vector<Point2> vertices;
    vertices.reserve(static_cast<std::size_t>(sides));
    const double step = katana::math::kTwoPi / sides;
    vertices.push_back(centre + toFirst);
    for (int k = 1; k < sides; ++k) {
        vertices.push_back(centre + toFirst.rotated(step * k));
    }
    return vertices;
}

// The same polygon given the centre-to-midpoint vector of its first side: the
// first vertex is half a side's turn clockwise of it and 1/cos(pi/n) farther
// out (the circumradius of a polygon whose apothem is |toMidpoint|).
Vec2 firstVertexFromMidpoint(const Vec2& toMidpoint, int sides)
{
    const double half = katana::math::kPi / sides;
    return toMidpoint.rotated(-half) * (1.0 / std::cos(half));
}

// The polygon on the LEFT of the directed side a->b, counter-clockwise from a:
// each side is the first turned by one exterior angle more than the last.
std::vector<Point2> polygonOnEdge(const Point2& a, const Point2& b, int sides)
{
    std::vector<Point2> vertices;
    vertices.reserve(static_cast<std::size_t>(sides));
    const Vec2 edge = b - a;
    const double step = katana::math::kTwoPi / sides;
    vertices.push_back(a);
    vertices.push_back(b);
    for (int k = 1; k + 1 < sides; ++k) {
        vertices.push_back(vertices.back() + edge.rotated(step * k));
    }
    return vertices;
}

class PolygonTool final : public InteractiveTool {
  public:
    explicit PolygonTool(const ToolContext& context) : attributes_(context.attributes) {}

    [[nodiscard]] std::string prompt() const override
    {
        switch (step_) {
        case Step::Sides:
            return "Enter number of sides";
        case Step::Centre:
            return "Specify centre of polygon or [Edge/Undo]";
        case Step::Radius:
            return inscribed_ ? "Specify a vertex or type the radius, or [Circumscribed/Undo]"
                              : "Specify the middle of a side or type the radius, or "
                                "[Inscribed/Undo]";
        case Step::EdgeStart:
            return "Specify first endpoint of edge or [Undo]";
        case Step::EdgeEnd:
            return "Specify second endpoint of edge or [Undo]";
        }
        return {};
    }

    [[nodiscard]] ToolInput expects() const override
    {
        return step_ == Step::Sides ? ToolInput::Value : ToolInput::Point;
    }

    ToolStep point(const Point2& at) override
    {
        switch (step_) {
        case Step::Sides:
            break;
        case Step::Centre:
            centre_ = at;
            step_ = Step::Radius;
            return ToolStep::next();
        case Step::Radius: {
            if (coincide(at, centre_)) {
                return ToolStep::rejected(
                    "the point is at the centre; the polygon needs a radius greater than zero");
            }
            return make(regularPolygon(centre_, toFirstVertex(at - centre_), sides_));
        }
        case Step::EdgeStart:
            edgeStart_ = at;
            step_ = Step::EdgeEnd;
            return ToolStep::next();
        case Step::EdgeEnd:
            if (coincide(at, edgeStart_)) {
                return ToolStep::rejected(
                    "the endpoints are the same point; an edge needs two different points");
            }
            return make(polygonOnEdge(edgeStart_, at, sides_));
        }
        return InteractiveTool::point(at);
    }

    ToolStep value(std::string_view text) override
    {
        if (isOption(text, "Undo")) {
            return undo();
        }
        switch (step_) {
        case Step::Sides: {
            const auto sides = katana::core::parseInteger(katana::core::trimmed(text));
            if (!sides || *sides < kMinimumSides || *sides > kMaximumSides) {
                return ToolStep::rejected(quoted(text) +
                                          " is not a number of sides; a polygon has a whole number "
                                          "of sides from 3 to 1024");
            }
            sides_ = static_cast<int>(*sides);
            step_ = Step::Centre;
            return ToolStep::next();
        }
        case Step::Centre:
            if (isOption(text, "Edge")) {
                step_ = Step::EdgeStart;
                return ToolStep::next();
            }
            return ToolStep::rejected(quoted(text) +
                                      " is not a point; type the centre as x,y, or E to draw the "
                                      "polygon on an edge");
        case Step::Radius: {
            if (isOption(text, "Inscribed")) {
                return setInscribed(true);
            }
            if (isOption(text, "Circumscribed")) {
                return setInscribed(false);
            }
            auto radius = parseLength(text, "radius");
            if (!radius) {
                return ToolStep::rejected(radius.error().message);
            }
            // The bottom side level: its midpoint straight below the centre.
            const Vec2 toBottom(0.0, -*radius);
            const double half = katana::math::kPi / sides_;
            return make(regularPolygon(centre_,
                                       inscribed_ ? toBottom.rotated(-half)
                                                  : firstVertexFromMidpoint(toBottom, sides_),
                                       sides_));
        }
        case Step::EdgeStart:
        case Step::EdgeEnd:
            break;
        }
        return ToolStep::rejected(quoted(text) + " is not a point; type the endpoint as x,y");
    }

    ToolStep enter() override
    {
        // At the sides prompt nothing is in progress, so this ends the tool
        // (see above); anywhere else it abandons the polygon, as Esc does.
        return ToolStep::done(nullptr);
    }

    ToolStep undo() override
    {
        switch (step_) {
        case Step::Sides:
            return ToolStep::rejected("nothing to undo; the number of sides has not been given");
        case Step::Centre:
            step_ = Step::Sides;
            break;
        case Step::Radius:
            // An Inscribed or Circumscribed typed since the centre is a later
            // input than the centre, so it is the one taken back.
            if (!modesBefore_.empty()) {
                inscribed_ = modesBefore_.back();
                modesBefore_.pop_back();
                break;
            }
            step_ = Step::Centre;
            break;
        case Step::EdgeStart:
            step_ = Step::Centre;
            break;
        case Step::EdgeEnd:
            step_ = Step::EdgeStart;
            break;
        }
        return ToolStep::next();
    }

    [[nodiscard]] ToolFeedback preview(const Point2& cursor) const override
    {
        ToolFeedback feedback;
        if (step_ == Step::Radius) {
            feedback.markers.push_back(centre_);
            if (!coincide(cursor, centre_)) {
                feedback.shapes.emplace_back(Polyline2{
                    regularPolygon(centre_, toFirstVertex(cursor - centre_), sides_), true});
            }
        } else if (step_ == Step::EdgeEnd) {
            feedback.markers.push_back(edgeStart_);
            if (!coincide(cursor, edgeStart_)) {
                feedback.shapes.emplace_back(
                    Polyline2{polygonOnEdge(edgeStart_, cursor, sides_), true});
            }
        }
        return feedback;
    }

    [[nodiscard]] std::optional<Point2> lastPoint() const override
    {
        switch (step_) {
        case Step::Radius:
            return centre_;
        case Step::EdgeEnd:
            return edgeStart_;
        case Step::Sides:
        case Step::Centre:
        case Step::EdgeStart:
            break;
        }
        return std::nullopt;
    }

  private:
    enum class Step { Sides, Centre, Radius, EdgeStart, EdgeEnd };

    // From the centre to the first vertex, given the centre-to-radius-point
    // vector: that point IS the first vertex when inscribed, and the middle of
    // the first side when circumscribed.
    [[nodiscard]] Vec2 toFirstVertex(const Vec2& toPoint) const
    {
        return inscribed_ ? toPoint : firstVertexFromMidpoint(toPoint, sides_);
    }

    // Every I or C typed counts as an input for Undo, even one that leaves
    // the mode as it was: Undo then takes back what was typed, rather than
    // skipping it and throwing away the centre.
    ToolStep setInscribed(bool inscribed)
    {
        modesBefore_.push_back(inscribed_);
        inscribed_ = inscribed;
        return ToolStep::next();
    }

    ToolStep make(std::vector<Point2> vertices)
    {
        const int sides = sides_;
        step_ = Step::Sides;
        modesBefore_.clear();
        return ToolStep::done(
            cmd::createPolyline(Polyline2{std::move(vertices), true}, attributes_),
            "polygon of " + std::to_string(sides) + " sides", true);
    }

    cmd::EntityAttributes attributes_;
    Step step_ = Step::Sides;
    int sides_ = kMinimumSides; // always typed before a step that uses it
    bool inscribed_ = true;
    // The mode before each Inscribed or Circumscribed typed at the radius
    // prompt, latest last; Undo pops them before it gives up the centre.
    std::vector<bool> modesBefore_;
    Point2 centre_;
    Point2 edgeStart_;
};

// ---- registration ------------------------------------------------------------------

template <typename Tool>
ToolInfo info(std::string id, std::string name, int order, std::vector<std::string> aliases,
              std::string tip)
{
    ToolInfo tool;
    tool.id = std::move(id);
    tool.name = std::move(name);
    tool.category = "Draw";
    tool.group = "Lines";
    tool.order = order;
    tool.aliases = std::move(aliases);
    tool.tip = std::move(tip);
    tool.make = [](const ToolContext& context) -> std::unique_ptr<InteractiveTool> {
        return std::make_unique<Tool>(context);
    };
    return tool;
}

} // namespace

void addDrawLineTools(ToolCatalog& catalog, const Report& report)
{
    // The aliases are AutoCAD's, plus the interpreter's own spellings of the
    // same verbs (RECT, RECTANGLE, POLYLINE; command_interpreter.cpp), so a
    // word typed at the command line means one thing whichever reads it.
    report(catalog.add(info<PointTool>("draw.point", "Point", 10, {"POINT", "PO"},
                                       "Places a point at each click or typed x,y until you "
                                       "press Esc.")));
    report(catalog.add(info<LineTool>("draw.line", "Line", 20, {"LINE", "L"},
                                      "Draws a chain of separate lines point by point; C closes "
                                      "it, U takes back a point and Enter finishes.")));
    report(catalog.add(info<PolylineTool>("draw.polyline", "Polyline", 30,
                                          {"PLINE", "PL", "POLYLINE"},
                                          "Draws one polyline through the points you give; C "
                                          "closes it, U takes back a vertex and Enter "
                                          "finishes.")));
    report(catalog.add(info<RectangleTool>("draw.rectangle", "Rectangle", 40,
                                           {"RECTANG", "REC", "RECT", "RECTANGLE"},
                                           "Draws a rectangle from two opposite corners, or from "
                                           "one corner and @width,height or its typed sizes.")));
    report(catalog.add(info<PolygonTool>("draw.polygon", "Polygon", 50, {"POLYGON", "POL"},
                                         "Draws a regular polygon from its number of sides and "
                                         "either its centre and radius or one of its edges.")));
}

} // namespace katana::cad::tools
