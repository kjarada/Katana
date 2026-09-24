// Distance, Area, ID Point, Angle, List (see families.hpp): the tools that
// measure and report. None changes the drawing - each finishes with no
// command and its answer as the step's message, which the window writes to the
// command log - so they are safe to run on any drawing, locked layers
// included.
//
// The answers are a surveyor's, not a drafter's: coordinates easting first,
// directions as azimuths and quadrant bearings in degrees, minutes and
// seconds, heights kept apart from lengths and never invented. Distance and an
// object's Area are the survey tools' own computation and wording
// (survey_tools.hpp: computeInverse, computeArea and their formatters), so the
// Survey menu's Inverse and this tool cannot describe one pair of points two
// ways. A pick that lands on a survey point (a snap puts it exactly there)
// speaks for that point: its number, and its height, so a distance between two
// levelled points reports their height difference.
//
// Distance, Area, ID Point and Angle start again when they have answered, as
// AutoCAD's MEASUREGEOM does, so a run of measurements is a run of picks; Esc
// ends them. List ends, since its next use wants a fresh selection.

#include <algorithm>
#include <cmath>
#include <cstddef>
#include <memory>
#include <optional>
#include <string>
#include <string_view>
#include <variant>
#include <vector>

#include "everyday_support.hpp"
#include "families.hpp"
#include "katana/cad/survey_import.hpp"
#include "katana/cad/survey_tools.hpp"
#include "katana/math/numerics.hpp"
#include "katana/survey/angles.hpp"
#include "katana/survey/cogo.hpp"
#include "modify_edit_support.hpp"

namespace katana::cad::tools {

namespace {

using everyday::areaText;
using everyday::azimuthOf;
using everyday::bearing;
using everyday::coordinates;
using everyday::counted;
using everyday::degrees;
using everyday::fixed;
using everyday::lengthUnit;
using everyday::positionAt;
using everyday::SelectionStep;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::geometry::Arc2;
using katana::geometry::Circle2;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;
using modify_edit::asSentence;
using modify_edit::isOption;
namespace tolerance = katana::math::tolerance;

constexpr std::string_view kCategory = "Tools";
constexpr std::string_view kGroup = "Inquiry";

bool coincident(const Point2& a, const Point2& b)
{
    return a.distanceTo(b) <= tolerance::kGeometric;
}

// The rubber band from `from` to the cursor, with `from` marked.
void addBand(ToolFeedback& feedback, const Point2& from, const Point2& cursor)
{
    if (!coincident(from, cursor)) {
        feedback.shapes.emplace_back(Segment2{from, cursor});
    }
    feedback.markers.push_back(from);
}

// ---- Distance ----------------------------------------------------------------------
//
// Two points, then the inverse between them: the differences in easting and
// northing, the horizontal distance, the azimuth both ways and the bearing,
// and - when both picks are survey points with heights - the height
// difference, slope distance and grade.
class DistanceTool final : public InteractiveTool {
  public:
    explicit DistanceTool(const ToolContext& context)
        : document_(context.document), tolerance_(context.pickTolerance)
    {
    }

    [[nodiscard]] std::string prompt() const override
    {
        return first_ ? "Specify second point" : "Specify first point";
    }
    [[nodiscard]] ToolInput expects() const override { return ToolInput::Point; }

    ToolStep point(const Point2& at) override
    {
        if (!first_) {
            first_ = at;
            return ToolStep::next();
        }
        const auto inverse = computeInverse(positionAt(document_, *first_, tolerance_),
                                            positionAt(document_, at, tolerance_));
        if (!inverse) {
            return ToolStep::rejected(asSentence(inverse.error().message));
        }
        return ToolStep::done(nullptr, formatInverseReport(*inverse), true);
    }

    ToolStep undo() override
    {
        if (!first_) {
            return ToolStep::rejected("nothing to undo; no point has been given yet");
        }
        first_.reset();
        return ToolStep::next();
    }

    [[nodiscard]] ToolFeedback preview(const Point2& cursor) const override
    {
        ToolFeedback feedback;
        if (first_) {
            addBand(feedback, *first_, cursor);
        }
        return feedback;
    }

    [[nodiscard]] std::optional<Point2> lastPoint() const override { return first_; }

  private:
    const Document* document_ = nullptr;
    double tolerance_ = 0.0;
    std::optional<Point2> first_;
};

// ---- Area --------------------------------------------------------------------------
//
// Corners picked in order, closed back to the first, and Enter for the area
// and perimeter; or [Object] and a closed polyline or circle, measured by the
// survey tools' own area (computeArea). Started with a selection, Enter at the
// first corner measures what is selected, as the command line's AREA does.
class AreaTool final : public InteractiveTool {
  public:
    explicit AreaTool(const ToolContext& context)
        : document_(context.document), selection_(context.selection)
    {
    }

    [[nodiscard]] std::string prompt() const override
    {
        if (object_) {
            return "Select a closed polyline or circle or [Points]";
        }
        if (corners_.empty()) {
            return selection_.empty() ? "Specify first corner point or [Object]"
                                      : "Specify first corner point or [Object] <Selection>";
        }
        return corners_.size() < 3
                   ? "Specify next corner point or [Undo]"
                   : "Specify next corner point or [Undo], or press Enter for the area";
    }

    [[nodiscard]] ToolInput expects() const override
    {
        return object_ ? ToolInput::Entity : ToolInput::Point;
    }

    ToolStep point(const Point2& at) override
    {
        if (object_) {
            return InteractiveTool::point(at);
        }
        if (!corners_.empty() && coincident(at, corners_.back())) {
            return ToolStep::rejected("the corner is where the last one is");
        }
        corners_.push_back(at);
        return ToolStep::next();
    }

    ToolStep entity(EntityId id, const Point2& at) override
    {
        if (!object_) {
            return InteractiveTool::entity(id, at);
        }
        return measure({id}, true);
    }

    ToolStep value(std::string_view text) override
    {
        if (!object_ && corners_.empty() && isOption(text, "Object", "O")) {
            object_ = true;
            return ToolStep::next();
        }
        if (object_ && isOption(text, "Points", "P")) {
            object_ = false;
            return ToolStep::next();
        }
        if (!object_ && isOption(text, "Undo", "U")) {
            return undo();
        }
        return ToolStep::rejected("'" + std::string(text) + "' is not an option here");
    }

    ToolStep enter() override
    {
        if (object_) {
            return ToolStep::done(nullptr);
        }
        if (corners_.empty()) {
            // The selection, once - a restart would measure it again.
            return selection_.empty() ? ToolStep::done(nullptr) : measure(selection_, false);
        }
        if (corners_.size() < 3) {
            return ToolStep::rejected("an area needs at least three corners");
        }
        return pickedArea();
    }

    ToolStep undo() override
    {
        if (object_ || corners_.empty()) {
            return ToolStep::rejected("nothing to undo; no corner has been given yet");
        }
        corners_.pop_back();
        return ToolStep::next();
    }

    [[nodiscard]] ToolFeedback preview(const Point2& cursor) const override
    {
        ToolFeedback feedback;
        if (object_ || corners_.empty()) {
            return feedback;
        }
        Polyline2 outline{corners_, false};
        if (!coincident(cursor, corners_.back())) {
            outline.vertices.push_back(cursor);
        }
        outline.closed = outline.vertices.size() >= 3;
        if (outline.vertices.size() >= 2) {
            feedback.shapes.emplace_back(std::move(outline));
        }
        feedback.markers = corners_;
        return feedback;
    }

    [[nodiscard]] std::optional<Point2> lastPoint() const override
    {
        return corners_.empty() ? std::nullopt : std::optional<Point2>(corners_.back());
    }

  private:
    ToolStep measure(const std::vector<EntityId>& ids, bool restart)
    {
        if (document_ == nullptr) {
            return ToolStep::rejected("there is no drawing to measure");
        }
        const auto area = computeArea(*document_, ids);
        if (!area) {
            return ToolStep::rejected(asSentence(area.error().message));
        }
        return ToolStep::done(nullptr, formatAreaReport(*area), restart);
    }

    ToolStep pickedArea()
    {
        // The survey library's polygon area and perimeter, which work from
        // the first vertex so coordinates of MGA size keep their precision.
        std::vector<katana::survey::Coordinate2> ring;
        ring.reserve(corners_.size());
        for (const Point2& corner : corners_) {
            ring.push_back({corner.y, corner.x}); // northing, easting
        }
        const auto area = katana::survey::polygonArea(ring);
        const auto perimeter = katana::survey::polygonPerimeter(ring);
        if (!area || !perimeter) {
            return ToolStep::rejected(
                asSentence(area ? perimeter.error().message : area.error().message));
        }
        std::string text = "Area of " + counted(corners_.size(), "picked corner", "picked corners") +
                            " - drawing unit '" + document_->metadata().linearUnit +
                            "' (project settings)\n";
        text += "  area " + areaText(*document_, *area) + "   perimeter " + fixed(*perimeter) +
                " " + lengthUnit(*document_);
        corners_.clear();
        return ToolStep::done(nullptr, std::move(text), true);
    }

    const Document* document_ = nullptr;
    std::vector<EntityId> selection_;
    std::vector<Point2> corners_;
    bool object_ = false;
};

// ---- ID Point ----------------------------------------------------------------------
//
// One point and its coordinates. On a survey point, the point's number, its
// height and its code as well - what a surveyor clicks a point to find out.
class IdPointTool final : public InteractiveTool {
  public:
    explicit IdPointTool(const ToolContext& context)
        : document_(context.document), tolerance_(context.pickTolerance)
    {
    }

    [[nodiscard]] std::string prompt() const override { return "Specify a point"; }
    [[nodiscard]] ToolInput expects() const override { return ToolInput::Point; }

    ToolStep point(const Point2& at) override
    {
        const SurveyPosition position = positionAt(document_, at, tolerance_);
        if (!position.entity) {
            return ToolStep::done(nullptr, coordinates(at), true);
        }
        std::string text = position.label() + "  " + coordinates(position.point);
        text += position.elevation ? "  Z " + fixed(*position.elevation)
                                   : std::string("  no elevation");
        // The code as the survey import writes it (SurveyImportOptions).
        const std::string codeKey = SurveyImportOptions{}.codeProperty;
        if (const Entity* entity = document_->model().entities.find(*position.entity)) {
            if (const auto code = entity->properties.find(codeKey);
                code != entity->properties.end()) {
                text += "  code " + katana::entity::toString(code->second);
            }
        }
        return ToolStep::done(nullptr, std::move(text), true);
    }

  private:
    const Document* document_ = nullptr;
    double tolerance_ = 0.0;
};

// ---- Angle -------------------------------------------------------------------------
//
// A vertex, a point on the first arm and a point on the second: the included
// angle, and the angle turned clockwise from the first arm to the second as a
// surveyor turns it from a backsight to a foresight, with both arms'
// directions.
class AngleTool final : public InteractiveTool {
  public:
    explicit AngleTool(const ToolContext& context)
        : document_(context.document), tolerance_(context.pickTolerance)
    {
    }

    [[nodiscard]] std::string prompt() const override
    {
        switch (points_.size()) {
        case 0:
            return "Specify the vertex of the angle";
        case 1:
            return "Specify a point on the first arm (the backsight)";
        default:
            return "Specify a point on the second arm (the foresight)";
        }
    }
    [[nodiscard]] ToolInput expects() const override { return ToolInput::Point; }

    ToolStep point(const Point2& at) override
    {
        if (!points_.empty() && coincident(at, points_.front())) {
            return ToolStep::rejected("an arm needs a point away from the vertex");
        }
        if (points_.size() < 2) {
            points_.push_back(at);
            return ToolStep::next();
        }
        const Point2 vertex = points_[0];
        const double first = azimuthOf(points_[1] - vertex);
        const double second = azimuthOf(at - vertex);
        const double clockwise = katana::survey::normalizeAzimuth(second - first);
        const double other = clockwise > 0.0 ? 2.0 * katana::math::kPi - clockwise : 0.0;
        std::string text =
            "Angle at " + positionAt(document_, vertex, tolerance_).label() + "\n";
        text += "  Included angle " + degrees(std::min(clockwise, other)) + "\n";
        text += "  Turned clockwise from the first arm to the second " + degrees(clockwise) +
                "   the other way " + degrees(other) + "\n";
        text += "  First arm  azimuth " + degrees(first) + "   bearing " + bearing(first) + "\n";
        text += "  Second arm azimuth " + degrees(second) + "   bearing " + bearing(second);
        points_.clear();
        return ToolStep::done(nullptr, std::move(text), true);
    }

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
        if (points_.size() == 2) {
            feedback.shapes.emplace_back(Segment2{points_[0], points_[1]});
        }
        if (!points_.empty()) {
            addBand(feedback, points_[0], cursor);
            feedback.markers = points_;
        }
        return feedback;
    }

    [[nodiscard]] std::optional<Point2> lastPoint() const override
    {
        return points_.empty() ? std::nullopt : std::optional<Point2>(points_.back());
    }

  private:
    const Document* document_ = nullptr;
    double tolerance_ = 0.0;
    std::vector<Point2> points_; // the vertex, then the first arm's point
};

// ---- List --------------------------------------------------------------------------
//
// What each selected entity is, where, and on what: its layer, style and
// colour, its geometry in survey terms, its heights and its properties.

// A thousand lines in the log is more than anyone reads; a selection of a
// whole survey is listed by its first entities and a count of the rest.
constexpr std::size_t kMostListed = 200;
// A polyline's vertices beyond this are counted, not listed.
constexpr std::size_t kMostVertices = 50;

std::string zText(const std::optional<double>& height)
{
    return height ? "  Z " + fixed(*height) : std::string();
}

std::string describe(const Document& document, const Entity& entity)
{
    const std::string unit = lengthUnit(document);
    const std::string metres = " " + unit;
    std::string text = std::to_string(entity.id) + "  " +
                       std::string(katana::entity::toString(entity.type())) + "  layer " +
                       entity.layer + "  style " +
                       (entity.style.empty() ? std::string("ByLayer") : entity.style) +
                       "  colour " + (entity.color ? entity.color->toHex() : "ByLayer");
    const auto heights = [&](std::size_t count) {
        return katana::entity::heightsOf(entity.properties, count);
    };
    std::visit(
        [&](const auto& g) {
            using G = std::decay_t<decltype(g)>;
            if constexpr (std::is_same_v<G, katana::entity::PointGeometry>) {
                text += "\n    " + coordinates(g.position) + zText(heights(1).front());
            } else if constexpr (std::is_same_v<G, Segment2>) {
                const auto z = heights(2);
                text += "\n    from " + coordinates(g.start) + zText(z[0]) + "  to " +
                        coordinates(g.end) + zText(z[1]);
                text += "\n    length " + fixed(g.start.distanceTo(g.end)) + metres;
                if (!coincident(g.start, g.end)) {
                    text += "   bearing " + bearing(azimuthOf(g.end - g.start));
                }
            } else if constexpr (std::is_same_v<G, Arc2>) {
                text += "\n    centre " + coordinates(g.center) + "  radius " + fixed(g.radius) +
                        metres;
                text += "\n    from " + coordinates(g.startPoint()) + "  to " +
                        coordinates(g.endPoint()) +
                        (g.sweep >= 0.0 ? "  anticlockwise" : "  clockwise");
                text += "\n    length " + fixed(g.radius * std::abs(g.sweep)) + metres +
                        "   delta " + degrees(std::abs(g.sweep)) + "   chord " +
                        fixed(g.startPoint().distanceTo(g.endPoint())) + metres;
            } else if constexpr (std::is_same_v<G, Circle2>) {
                const double pi = katana::math::kPi;
                text += "\n    centre " + coordinates(g.center) + zText(heights(1).front()) +
                        "  radius " + fixed(g.radius) + metres;
                text += "\n    circumference " + fixed(2.0 * pi * g.radius) + metres + "   area " +
                        areaText(document, pi * g.radius * g.radius);
            } else if constexpr (std::is_same_v<G, Polyline2>) {
                const auto path = everyday::Path::of(g);
                text += std::string("\n    ") + (g.closed ? "closed" : "open") + ", " +
                        counted(g.vertices.size(), "vertex", "vertices") + ", length " +
                        fixed(path ? path->length() : 0.0) + metres;
                if (g.closed && g.vertices.size() >= 3) {
                    std::vector<katana::survey::Coordinate2> ring;
                    for (const Point2& v : g.vertices) {
                        ring.push_back({v.y, v.x});
                    }
                    if (const auto area = katana::survey::polygonArea(ring)) {
                        text += "   area " + areaText(document, *area);
                    }
                }
                const auto z = heights(g.vertices.size());
                const std::size_t shown = std::min(g.vertices.size(), kMostVertices);
                for (std::size_t i = 0; i < shown; ++i) {
                    text += "\n    " + std::to_string(i + 1) + "  " + coordinates(g.vertices[i]) +
                            zText(z[i]);
                }
                if (shown < g.vertices.size()) {
                    text += "\n    ... and " + std::to_string(g.vertices.size() - shown) +
                            " more vertices";
                }
            } else if constexpr (std::is_same_v<G, katana::entity::TextGeometry>) {
                text += "\n    \"" + g.text + "\"  height " + fixed(g.height) + metres + "  at " +
                        coordinates(g.position) + "  rotation " + degrees(g.rotation);
            } else if constexpr (std::is_same_v<G, katana::entity::DimensionGeometry>) {
                text += "\n    measures " + fixed(g.measurement()) + metres + "  from " +
                        coordinates(g.start) + "  to " + coordinates(g.end);
                if (!g.textOverride.empty()) {
                    text += "  shown as \"" + g.textOverride + "\"";
                }
            }
        },
        entity.geometry);
    for (const auto& [key, value] : entity.properties) {
        // The heights are shown with the coordinates they belong to.
        if (key == katana::entity::kElevationProperty ||
            key == katana::entity::kElevationsProperty) {
            continue;
        }
        text += "\n    " + key + " = " + katana::entity::toString(value);
    }
    return text;
}

class ListTool final : public InteractiveTool {
  public:
    explicit ListTool(const ToolContext& context)
        : document_(context.document), selection_(context.document)
    {
    }

    [[nodiscard]] std::string prompt() const override
    {
        return "Select entities to list or [All], then press Enter";
    }
    [[nodiscard]] ToolInput expects() const override { return ToolInput::Selection; }

    ToolStep entity(EntityId id, const Point2& /*at*/) override { return selection_.pick(id); }

    ToolStep value(std::string_view text) override
    {
        if (isOption(text, "All", "A")) {
            return selection_.all();
        }
        return ToolStep::rejected("'" + std::string(text) +
                                  "' is not an option here: pick entities, or type All");
    }

    ToolStep enter() override
    {
        const std::vector<EntityId> ids = selection_.chosen();
        if (ids.empty() || document_ == nullptr) {
            return ToolStep::rejected("nothing is selected: select the entities to list, then "
                                      "press Enter");
        }
        std::string text = counted(ids.size(), "entity", "entities") + " listed";
        const std::size_t shown = std::min(ids.size(), kMostListed);
        for (std::size_t i = 0; i < shown; ++i) {
            if (const Entity* entity = document_->model().entities.find(ids[i])) {
                text += "\n" + describe(*document_, *entity);
            }
        }
        if (shown < ids.size()) {
            text += "\n... and " + std::to_string(ids.size() - shown) +
                    " more, not listed; select fewer to see them";
        }
        return ToolStep::done(nullptr, std::move(text));
    }

    ToolStep undo() override
    {
        return selection_.undo() ? ToolStep::next()
                                 : ToolStep::rejected("nothing to undo in this tool");
    }

  private:
    const Document* document_ = nullptr;
    SelectionStep selection_;
};

template <typename Tool>
ToolInfo inquiry(std::string id, std::string name, int order, std::vector<std::string> aliases,
                 std::string tip)
{
    ToolInfo info;
    info.id = std::move(id);
    info.name = std::move(name);
    info.category = std::string(kCategory);
    info.group = std::string(kGroup);
    info.order = order;
    info.aliases = std::move(aliases);
    info.tip = std::move(tip);
    info.make = [](const ToolContext& context) -> std::unique_ptr<InteractiveTool> {
        return std::make_unique<Tool>(context);
    };
    return info;
}

} // namespace

void addInquiryTools(ToolCatalog& catalog, const Report& report)
{
    // DIST, AA, ID and LI are AutoCAD's own short forms. The bare words
    // AREA and LIST stay the command line's (area of the selection; every
    // entity listed), which scripts and the headless checks type.
    report(catalog.add(inquiry<DistanceTool>(
        "inquiry.distance", "Distance", 10, {"DIST", "DI"},
        "Measures between two points: dE and dN, the horizontal distance, the azimuth and "
        "bearing, and the height difference when both are survey points with heights.")));
    report(catalog.add(inquiry<AreaTool>(
        "inquiry.area", "Area", 20, {"AA", "MEASUREAREA"},
        "Measures the area and perimeter of corners picked in turn (Enter to total), or of a "
        "closed polyline or circle (O for Object).")));
    report(catalog.add(inquiry<IdPointTool>(
        "inquiry.id", "ID Point", 30, {"ID"},
        "Reports a point's easting and northing; on a survey point, its number, height and "
        "code too.")));
    report(catalog.add(inquiry<AngleTool>(
        "inquiry.angle", "Angle", 40, {"ANGLE", "MEASUREANGLE"},
        "Measures the angle at a vertex between two arms, as included and as turned clockwise "
        "from the first arm to the second, with each arm's bearing.")));
    report(catalog.add(inquiry<ListTool>(
        "inquiry.list", "List", 50, {"LI", "LS"},
        "Lists the selected entities: layer, style and colour, coordinates, lengths, bearings, "
        "areas, heights and properties.")));
}

} // namespace katana::cad::tools
