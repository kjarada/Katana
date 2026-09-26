// The Draw > Vertices tools (see families.hpp): vertex control of polylines,
// the owner's request of 2026-09-25, as catalogue tools - insert, delete and
// move a vertex, straighten, weed, densify, close or open, change the start,
// heights, segments to arcs and back, fillet and chamfer one corner, merge
// near vertices, snap to the grid - and Edit Vertices, which hands a
// polyline to the Vertices panel.
//
// Every tool is a short SCRIPT of steps - pick the polyline (at the vertex or
// segment that matters), pick or type a point, type a value, or select
// several polylines - run by one state machine here, and ends in ONE command
// built by cad/drawing/vertex_editing.hpp over the arithmetic of
// geometry/polyline_vertices.hpp. So the tools, the grips, the panel and the
// VERTEX verbs cannot disagree about what an edit does: the tools only
// collect its arguments. A tool that works on one pick restarts, as Point
// and Circle do, for a run of edits; each edit is its own undo step.

#include <algorithm>
#include <functional>
#include <map>
#include <memory>
#include <optional>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include "families.hpp"
#include "katana/cad/drawing/vertex_editing.hpp"
#include "katana/core/text.hpp"
#include "katana/math/numerics.hpp"

namespace katana::cad::tools {

namespace {

namespace cmd = katana::commands;
namespace geo = katana::geometry;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::geometry::CurvePolyline2;
using katana::geometry::Point2;

// What the tools remember between uses, as a CAD program keeps the last
// fillet radius: one object for the program, nothing of it in the drawing.
struct VertexDefaults {
    double weedTolerance = 0.01;
    bool weedKeepsSurveyPoints = true;
    double densifyInterval = 1.0;
    double densifyChord = 0.0;
    double filletRadius = 1.0;
    double chamferFirst = 1.0;
    double chamferSecond = 1.0;
    double mergeTolerance = 0.001;
    double gridSpacing = 1.0;
};

VertexDefaults& defaults()
{
    static VertexDefaults kept;
    return kept;
}

bool isOption(std::string_view text, std::string_view keyword)
{
    const std::string_view typed = katana::core::trimmed(text);
    return !typed.empty() && typed.size() <= keyword.size() &&
           katana::core::equalsIgnoringCase(typed, keyword.substr(0, typed.size()));
}

std::string number(double value) { return katana::core::formatExactReal(value); }

// ---- the script ------------------------------------------------------------------------

struct Step {
    enum class Kind {
        Polyline,  // a pick of one polyline, near the vertex or segment that matters
        Point,     // a point (snapped), for the new vertex, the arc's point, the second vertex
        Value,     // typed text; Enter takes the default when there is one
        Selection, // several polylines, then Enter
    };
    Kind kind = Kind::Polyline;
    std::string prompt;
    // Value: what Enter takes, shown in the prompt as <default>.
    std::function<std::optional<std::string>()> fallback;
};

// Everything the steps collected, for the script's finish.
struct Collected {
    const Document* document = nullptr;
    EntityId polyline = katana::entity::kInvalidEntityId;
    CurvePolyline2 shape;
    Point2 pick; // where the polyline was picked
    std::vector<Point2> points;
    std::vector<std::string> values;
    std::vector<EntityId> selection;

    [[nodiscard]] std::size_t pickedVertex() const { return *geo::nearestVertex(shape, pick); }
    [[nodiscard]] std::size_t pickedSegment() const { return *geo::nearestSegment(shape, pick); }
};

using Finish = std::function<ToolStep(const Collected&)>;

class ScriptTool final : public InteractiveTool {
  public:
    ScriptTool(const ToolContext& context, std::vector<Step> script, Finish finish, bool restarts)
        : document_(context.document), script_(std::move(script)), finish_(std::move(finish)),
          restarts_(restarts)
    {
        collected_.document = context.document;
        // A selection step is answered by the selection the tool started on,
        // when it holds polylines.
        if (!script_.empty() && script_.front().kind == Step::Kind::Selection) {
            const auto polylines = polylinesAmong(context.selection);
            if (!polylines.empty()) {
                collected_.selection = polylines;
                step_ = 1;
                preselected_ = true;
            }
        }
    }

    [[nodiscard]] std::string prompt() const override
    {
        if (step_ >= script_.size()) {
            // Everything was given by the selection the tool started on.
            return "Press Enter to apply to the " +
                   std::to_string(collected_.selection.size()) + " selected polylines";
        }
        const Step& step = script_[step_];
        std::string text = step.prompt;
        if (step.fallback) {
            if (const auto shown = step.fallback()) {
                text += " <" + *shown + ">";
            }
        }
        return text;
    }

    [[nodiscard]] ToolInput expects() const override
    {
        if (step_ >= script_.size()) {
            return ToolInput::Value;
        }
        switch (script_[step_].kind) {
        case Step::Kind::Polyline:
            return ToolInput::Entity;
        case Step::Kind::Point:
            return ToolInput::Point;
        case Step::Kind::Value:
            return ToolInput::Value;
        case Step::Kind::Selection:
            return ToolInput::Selection;
        }
        return ToolInput::Value;
    }

    ToolStep entity(EntityId id, const Point2& at) override
    {
        if (step_ >= script_.size() || script_[step_].kind != Step::Kind::Polyline) {
            return InteractiveTool::entity(id, at);
        }
        const Entity* found = document_ != nullptr ? document_->model().entities.find(id) : nullptr;
        const auto shape = found != nullptr ? readPolyline(*found) : std::nullopt;
        if (!shape) {
            return ToolStep::rejected("that is not a polyline; pick a polyline");
        }
        collected_.polyline = id;
        collected_.shape = *shape;
        collected_.pick = at;
        return advance();
    }

    ToolStep point(const Point2& at) override
    {
        if (step_ < script_.size() && script_[step_].kind == Step::Kind::Polyline) {
            // A click on nothing: say what is wanted.
            return ToolStep::rejected("pick a polyline, on or near the vertex or segment");
        }
        if (step_ >= script_.size() || script_[step_].kind != Step::Kind::Point) {
            return InteractiveTool::point(at);
        }
        collected_.points.push_back(at);
        return advance();
    }

    ToolStep value(std::string_view text) override
    {
        if (step_ >= script_.size() || script_[step_].kind != Step::Kind::Value) {
            return ToolStep::rejected("'" + std::string(katana::core::trimmed(text)) +
                                      "' is not taken here; " + prompt());
        }
        collected_.values.emplace_back(katana::core::trimmed(text));
        return advance();
    }

    ToolStep enter() override
    {
        if (step_ >= script_.size()) {
            if (preselected_) {
                --step_; // advance() steps onto the end and finishes
                return advance();
            }
            return ToolStep::done(nullptr);
        }
        const Step& step = script_[step_];
        if (step.kind == Step::Kind::Value && step.fallback) {
            if (const auto fallback = step.fallback()) {
                collected_.values.push_back(*fallback);
                return advance();
            }
        }
        if (step.kind == Step::Kind::Selection) {
            const auto polylines =
                document_ != nullptr ? polylinesAmong(document_->selection().ids())
                                     : std::vector<EntityId>{};
            if (polylines.empty()) {
                return ToolStep::rejected("no polyline is selected; select polylines, then press "
                                          "Enter");
            }
            collected_.selection = polylines;
            return advance();
        }
        if (step_ == 0 || (preselected_ && step_ == 1)) {
            return ToolStep::done(nullptr); // nothing collected: Enter ends the tool
        }
        return ToolStep::rejected(prompt());
    }

    ToolStep undo() override
    {
        if (step_ == 0 || (preselected_ && step_ == 1)) {
            return ToolStep::rejected("nothing to undo in this tool");
        }
        --step_;
        switch (script_[step_].kind) {
        case Step::Kind::Point:
            collected_.points.pop_back();
            break;
        case Step::Kind::Value:
            collected_.values.pop_back();
            break;
        case Step::Kind::Polyline:
        case Step::Kind::Selection:
            break;
        }
        return ToolStep::next();
    }

    [[nodiscard]] ToolFeedback preview(const Point2& cursor) const override
    {
        ToolFeedback feedback;
        if (collected_.polyline != katana::entity::kInvalidEntityId && step_ > 0) {
            // The vertex the pick chose, so the user sees which one.
            feedback.markers.push_back(
                collected_.shape.vertices[collected_.pickedVertex()].position);
            if (step_ < script_.size() && script_[step_].kind == Step::Kind::Point) {
                feedback.shapes.emplace_back(geo::Segment2{feedback.markers.back(), cursor});
            }
        }
        return feedback;
    }

    [[nodiscard]] std::optional<Point2> lastPoint() const override
    {
        // Relative input (@dx,dy) is from the picked vertex: "move this
        // vertex 2 m east" is @2,0.
        if (collected_.polyline != katana::entity::kInvalidEntityId && step_ > 0) {
            return collected_.shape.vertices[collected_.pickedVertex()].position;
        }
        return std::nullopt;
    }

  private:
    [[nodiscard]] std::vector<EntityId> polylinesAmong(const std::vector<EntityId>& ids) const
    {
        std::vector<EntityId> out;
        if (document_ == nullptr) {
            return out;
        }
        for (const EntityId id : ids) {
            if (const Entity* entity = document_->model().entities.find(id);
                entity != nullptr && isPolylineEntity(*entity)) {
                out.push_back(id);
            }
        }
        return out;
    }

    ToolStep advance()
    {
        ++step_;
        if (step_ < script_.size()) {
            return ToolStep::next();
        }
        ToolStep done = finish_(collected_);
        if (done.outcome == ToolStep::Outcome::Rejected) {
            // The last input was not acceptable (a radius that does not fit):
            // step back to it, keeping everything before.
            (void)undo();
            return done;
        }
        done.restart = restarts_;
        step_ = 0;
        collected_ = Collected{};
        collected_.document = document_;
        return done;
    }

    const Document* document_ = nullptr;
    std::vector<Step> script_;
    Finish finish_;
    bool restarts_ = false;
    bool preselected_ = false;
    std::size_t step_ = 0;
    Collected collected_;
};

// ---- the edits ----------------------------------------------------------------------------

// The finish of a one-polyline tool: `edit` on the picked polyline as one
// command named `name`, refused up front when the edit would fail so that
// the tool keeps its inputs and says why.
ToolStep editOne(const Collected& c, std::string name, geo::PolylineResult edited,
                 std::string message)
{
    if (!edited) {
        return ToolStep::rejected(edited.error().message);
    }
    CurvePolyline2 result = std::move(*edited);
    return ToolStep::done(editPolyline(c.polyline, std::move(name),
                                       [result](const CurvePolyline2&) -> geo::PolylineResult {
                                           return result;
                                       }),
                          std::move(message));
}

std::optional<double> parsePositive(std::string_view text)
{
    const auto value = katana::core::parseFiniteDouble(katana::core::trimmed(text));
    return value && *value > 0.0 ? value : std::nullopt;
}

std::optional<double> parseNonNegative(std::string_view text)
{
    const auto value = katana::core::parseFiniteDouble(katana::core::trimmed(text));
    return value && *value >= 0.0 ? value : std::nullopt;
}

std::string plural(std::size_t count, std::string_view noun)
{
    if (noun == "vertex") {
        return std::to_string(count) + (count == 1 ? " vertex" : " vertices");
    }
    return std::to_string(count) + " " + std::string(noun) + (count == 1 ? "" : "s");
}

Step pickPolyline(std::string prompt) { return Step{Step::Kind::Polyline, std::move(prompt), {}}; }
Step pickPoint(std::string prompt) { return Step{Step::Kind::Point, std::move(prompt), {}}; }
Step selectPolylines(std::string verb)
{
    return Step{Step::Kind::Selection, "Select polylines to " + std::move(verb) + ", then press Enter",
                {}};
}
Step typeValue(std::string prompt, std::function<std::optional<std::string>()> fallback = {})
{
    return Step{Step::Kind::Value, std::move(prompt), std::move(fallback)};
}
std::function<std::optional<std::string>()> remembered(const double& value)
{
    return [&value]() -> std::optional<std::string> { return number(value); };
}

// ---- registration -------------------------------------------------------------------------

ToolInfo info(std::string id, std::string name, int order, std::vector<std::string> aliases,
              std::string tip, std::vector<Step> script, Finish finish, bool restarts)
{
    ToolInfo tool;
    tool.id = std::move(id);
    tool.name = "Vertices, " + std::move(name);
    tool.category = "Draw";
    tool.group = "Vertices";
    tool.order = order;
    tool.aliases = std::move(aliases);
    tool.tip = std::move(tip);
    tool.make = [script = std::move(script), finish = std::move(finish),
                 restarts](const ToolContext& context) -> std::unique_ptr<InteractiveTool> {
        return std::make_unique<ScriptTool>(context, script, finish, restarts);
    };
    return tool;
}

} // namespace

void addModifyVertexTools(ToolCatalog& catalog, const Report& report)
{
    VertexDefaults& kept = defaults();

    report(catalog.add(info(
        "draw.vertex.insert", "Insert Vertex", 10, {"INSERTVERTEX"},
        "Adds a vertex to a polyline: pick the polyline, then the new vertex; on an arc it stays "
        "on the arc.",
        {pickPolyline("Select the polyline to add a vertex to"),
         pickPoint("Specify the new vertex")},
        [](const Collected& c) {
            const std::size_t segment = *geo::nearestSegment(c.shape, c.points[0]);
            return editOne(c, "VERTEX_INSERT", geo::insertVertex(c.shape, segment, c.points[0]),
                           "vertex added on segment " + std::to_string(segment));
        },
        true)));

    report(catalog.add(info(
        "draw.vertex.delete", "Delete Vertex", 20, {"DELETEVERTEX"},
        "Removes the vertex nearest where you pick a polyline; its two segments become one.",
        {pickPolyline("Select the vertex to delete (pick the polyline near it)")},
        [](const Collected& c) {
            const std::size_t vertex = c.pickedVertex();
            return editOne(c, "VERTEX_DELETE", geo::deleteVertex(c.shape, vertex),
                           "vertex " + std::to_string(vertex) + " deleted");
        },
        true)));

    report(catalog.add(info(
        "draw.vertex.move", "Move Vertex", 30, {"MOVEVERTEX"},
        "Moves the vertex nearest where you pick a polyline to a picked or typed point; @dx,dy "
        "is from the vertex.",
        {pickPolyline("Select the vertex to move (pick the polyline near it)"),
         pickPoint("Specify its new position")},
        [](const Collected& c) {
            const std::size_t vertex = c.pickedVertex();
            return editOne(c, "VERTEX_MOVE", geo::moveVertex(c.shape, vertex, c.points[0]),
                           "vertex " + std::to_string(vertex) + " moved");
        },
        true)));

    report(catalog.add(info(
        "draw.vertex.edit", "Edit Vertices", 40, {"EDITVERTICES", "VERTEX"},
        "Selects a polyline and shows its vertices in the Vertices panel, to edit in place.",
        {pickPolyline("Select the polyline whose vertices to edit")},
        [](const Collected& c) {
            ToolStep step = ToolStep::done(nullptr, plural(c.shape.vertices.size(), "vertex") +
                                                        " in the Vertices panel");
            step.selection = std::vector<EntityId>{c.polyline};
            return step;
        },
        false)));

    report(catalog.add(info(
        "draw.vertex.straighten", "Straighten", 50, {"STRAIGHTEN"},
        "Removes every vertex between two you pick, leaving one straight segment between them.",
        {pickPolyline("Select the polyline at the first vertex to keep"),
         pickPoint("Specify the other vertex to keep")},
        [](const Collected& c) {
            const std::size_t from = c.pickedVertex();
            const std::size_t to = *geo::nearestVertex(c.shape, c.points[0]);
            const std::size_t removed = geo::verticesBetween(c.shape, from, to).size();
            return editOne(c, "STRAIGHTEN", geo::straighten(c.shape, from, to),
                           plural(removed, "vertex") + " removed");
        },
        true)));

    report(catalog.add(info(
        "draw.vertex.weed", "Weed", 60, {"WEED", "SIMPLIFY"},
        "Removes vertices that lie within a tolerance of the line through their neighbours "
        "(Douglas-Peucker), in plan and height, optionally keeping vertices on survey points.",
        {selectPolylines("weed"), typeValue("Specify the weed tolerance", remembered(kept.weedTolerance)),
         typeValue("Keep vertices on survey points? [Yes/No]",
                   []() -> std::optional<std::string> {
                       return defaults().weedKeepsSurveyPoints ? "Yes" : "No";
                   })},
        [](const Collected& c) {
            const auto tolerance = parseNonNegative(c.values[0]);
            if (!tolerance) {
                return ToolStep::rejected("'" + c.values[0] +
                                          "' is not a tolerance; type a distance of zero or more");
            }
            bool keep = true;
            if (isOption(c.values[1], "No")) {
                keep = false;
            } else if (!isOption(c.values[1], "Yes")) {
                return ToolStep::rejected("type Yes or No");
            }
            defaults().weedTolerance = *tolerance;
            defaults().weedKeepsSurveyPoints = keep;
            const double t = *tolerance;
            return ToolStep::done(
                editEachPolyline(c.selection, "WEED",
                                 [t, keep](const katana::entity::Model& model, EntityId,
                                           const CurvePolyline2& polyline) {
                                     return geo::weed(polyline, t,
                                                      keep ? verticesOnSurveyPoints(model, polyline)
                                                           : std::vector<bool>{});
                                 }),
                plural(c.selection.size(), "polyline") + " weeded to " + number(t));
        },
        false)));

    report(catalog.add(info(
        "draw.vertex.densify", "Densify", 70, {"DENSIFY"},
        "Adds vertices so no straight segment is longer than an interval, and optionally turns "
        "arcs into chords within a tolerance.",
        {selectPolylines("densify"),
         typeValue("Specify the interval, or 0 to leave straight segments", remembered(kept.densifyInterval)),
         typeValue("Specify the chord tolerance for arcs, or 0 to keep them as arcs",
                   remembered(kept.densifyChord))},
        [](const Collected& c) {
            const auto step = parseNonNegative(c.values[0]);
            const auto chord = parseNonNegative(c.values[1]);
            if (!step || !chord) {
                return ToolStep::rejected("type distances of zero or more");
            }
            defaults().densifyInterval = *step;
            defaults().densifyChord = *chord;
            const double s = *step;
            const double t = *chord;
            return ToolStep::done(editPolylines(c.selection, "DENSIFY",
                                                [s, t](const CurvePolyline2& polyline) {
                                                    return geo::densify(polyline, s, t);
                                                }),
                                  plural(c.selection.size(), "polyline") + " densified");
        },
        false)));

    report(catalog.add(info(
        "draw.vertex.close", "Close or Open", 80, {"CLOSEOPEN"},
        "Closes each open polyline selected and opens each closed one.",
        {selectPolylines("close or open")},
        [](const Collected& c) {
            return ToolStep::done(editPolylines(c.selection, "CLOSE_OPEN",
                                                [](const CurvePolyline2& polyline) {
                                                    return polyline.closed
                                                               ? geo::openPolyline(polyline)
                                                               : geo::closePolyline(polyline);
                                                }),
                                  plural(c.selection.size(), "polyline") + " closed or opened");
        },
        false)));

    report(catalog.add(info(
        "draw.vertex.start", "Change Start Vertex", 90, {"STARTVERTEX"},
        "Makes the vertex you pick the first of a closed polyline; its shape is unchanged.",
        {pickPolyline("Select the closed polyline at its new start vertex")},
        [](const Collected& c) {
            const std::size_t vertex = c.pickedVertex();
            return editOne(c, "START_VERTEX", geo::changeStartVertex(c.shape, vertex),
                           "vertex " + std::to_string(vertex) + " is now the start");
        },
        true)));

    report(catalog.add(info(
        "draw.vertex.height", "Set Vertex Height", 100, {"VERTEXZ"},
        "Sets the height of the vertex you pick, or clears it with None.",
        {pickPolyline("Select the vertex (pick the polyline near it)"),
         typeValue("Specify its height or [None]")},
        [](const Collected& c) {
            const std::size_t vertex = c.pickedVertex();
            std::optional<double> height;
            if (!isOption(c.values[0], "None")) {
                height = katana::core::parseFiniteDouble(c.values[0]);
                if (!height) {
                    return ToolStep::rejected("'" + c.values[0] +
                                              "' is not a height; type a number or None");
                }
            }
            return editOne(c, "VERTEX_Z", geo::setVertexHeight(c.shape, vertex, height),
                           "vertex " + std::to_string(vertex) +
                               (height ? " at " + number(*height) : std::string(" has no height")));
        },
        true)));

    report(catalog.add(info(
        "draw.vertex.interpolate", "Interpolate Heights", 110, {"INTERPOLATEZ"},
        "Gives every vertex without a height the height interpolated by length between the "
        "vertices either side that have one.",
        {selectPolylines("interpolate the heights of")},
        [](const Collected& c) {
            return ToolStep::done(editPolylines(c.selection, "INTERPOLATE_Z",
                                                [](const CurvePolyline2& polyline) {
                                                    return geo::interpolateHeights(polyline);
                                                }),
                                  plural(c.selection.size(), "polyline") + " interpolated");
        },
        false)));

    report(catalog.add(info(
        "draw.vertex.grade", "Grade Between Vertices", 120, {"GRADE"},
        "Sets the heights of the vertices between two you pick on the straight grade between "
        "their heights.",
        {pickPolyline("Select the polyline at the first vertex"),
         pickPoint("Specify the second vertex")},
        [](const Collected& c) {
            const std::size_t from = c.pickedVertex();
            const std::size_t to = *geo::nearestVertex(c.shape, c.points[0]);
            return editOne(c, "GRADE", geo::gradeBetween(c.shape, from, to),
                           "graded from vertex " + std::to_string(from) + " to " +
                               std::to_string(to));
        },
        true)));

    report(catalog.add(info(
        "draw.vertex.arc", "Segment to Arc", 130, {"SEGMENTARC"},
        "Makes the segment you pick an arc through a point you give.",
        {pickPolyline("Select the segment to make an arc"),
         pickPoint("Specify a point the arc passes through")},
        [](const Collected& c) {
            const std::size_t segment = c.pickedSegment();
            return editOne(c, "SEGMENT_ARC", geo::segmentToArc(c.shape, segment, c.points[0]),
                           "segment " + std::to_string(segment) + " is an arc");
        },
        true)));

    report(catalog.add(info(
        "draw.vertex.line", "Segment to Line", 140, {"SEGMENTLINE"},
        "Makes the arc segment you pick straight.",
        {pickPolyline("Select the arc segment to straighten")},
        [](const Collected& c) {
            const std::size_t segment = c.pickedSegment();
            return editOne(c, "SEGMENT_LINE", geo::segmentToLine(c.shape, segment),
                           "segment " + std::to_string(segment) + " is straight");
        },
        true)));

    report(catalog.add(info(
        "draw.vertex.fillet", "Fillet Vertex", 150, {"FILLETVERTEX"},
        "Rounds the corner at the vertex you pick with a tangent arc of a radius.",
        {pickPolyline("Select the corner vertex (pick the polyline near it)"),
         typeValue("Specify the fillet radius", remembered(kept.filletRadius))},
        [](const Collected& c) {
            const auto radius = parsePositive(c.values[0]);
            if (!radius) {
                return ToolStep::rejected("'" + c.values[0] + "' is not a radius greater than zero");
            }
            defaults().filletRadius = *radius;
            const std::size_t vertex = c.pickedVertex();
            return editOne(c, "FILLET_VERTEX", geo::filletVertex(c.shape, vertex, *radius),
                           "vertex " + std::to_string(vertex) + " filleted at " + number(*radius));
        },
        true)));

    report(catalog.add(info(
        "draw.vertex.chamfer", "Chamfer Vertex", 160, {"CHAMFERVERTEX"},
        "Cuts the corner at the vertex you pick with a straight bevel.",
        {pickPolyline("Select the corner vertex (pick the polyline near it)"),
         typeValue("Specify the first distance", remembered(kept.chamferFirst)),
         typeValue("Specify the second distance", remembered(kept.chamferSecond))},
        [](const Collected& c) {
            const auto first = parsePositive(c.values[0]);
            const auto second = parsePositive(c.values[1]);
            if (!first || !second) {
                return ToolStep::rejected("type two distances greater than zero");
            }
            defaults().chamferFirst = *first;
            defaults().chamferSecond = *second;
            const std::size_t vertex = c.pickedVertex();
            return editOne(c, "CHAMFER_VERTEX", geo::chamferVertex(c.shape, vertex, *first, *second),
                           "vertex " + std::to_string(vertex) + " chamfered");
        },
        true)));

    report(catalog.add(info(
        "draw.vertex.merge", "Merge Near Vertices", 170, {"MERGEVERTICES"},
        "Merges consecutive vertices closer together than a tolerance into the first of them.",
        {selectPolylines("merge the near vertices of"),
         typeValue("Specify the tolerance", remembered(kept.mergeTolerance))},
        [](const Collected& c) {
            const auto tolerance = parseNonNegative(c.values[0]);
            if (!tolerance) {
                return ToolStep::rejected("'" + c.values[0] + "' is not a tolerance");
            }
            defaults().mergeTolerance = *tolerance;
            const double t = *tolerance;
            return ToolStep::done(editPolylines(c.selection, "MERGE_VERTICES",
                                                [t](const CurvePolyline2& polyline) {
                                                    return geo::mergeNearVertices(polyline, t);
                                                }),
                                  plural(c.selection.size(), "polyline") + " merged within " +
                                      number(t));
        },
        false)));

    report(catalog.add(info(
        "draw.vertex.grid", "Snap Vertices to Grid", 180, {"SNAPVERTICES"},
        "Moves every vertex of the selected polylines to the nearest node of a grid.",
        {selectPolylines("snap to the grid"),
         typeValue("Specify the grid spacing", remembered(kept.gridSpacing))},
        [](const Collected& c) {
            const auto spacing = parsePositive(c.values[0]);
            if (!spacing) {
                return ToolStep::rejected("'" + c.values[0] + "' is not a spacing greater than zero");
            }
            defaults().gridSpacing = *spacing;
            const double s = *spacing;
            return ToolStep::done(editPolylines(c.selection, "SNAP_VERTICES",
                                                [s](const CurvePolyline2& polyline) {
                                                    return geo::snapToGrid(polyline, s);
                                                }),
                                  plural(c.selection.size(), "polyline") + " snapped to " +
                                      number(s));
        },
        false)));
}

} // namespace katana::cad::tools
