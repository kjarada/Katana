// The Draw > Vertices tools (see families.hpp): vertex control of polylines,
// the owner's request of 2026-09-25, as catalogue tools - insert, delete and
// move a vertex, straighten, weed, densify, close or open, change the start,
// heights, segments to arcs and back, fillet and chamfer one corner, merge
// near vertices, snap to the grid - and Edit Vertices, which hands a
// polyline to the Vertices panel.
//
// Every tool is a short SCRIPT of steps - pick a vertex, a segment or a
// polyline, click a point on a polyline, a point, a value, or several
// polylines - run by one state machine here, and ends in ONE command over the
// arithmetic of geometry/polyline_vertices.hpp. So the tools, the grips, the
// panel and the VERTEX verbs cannot disagree about what an edit does: the
// tools only collect its arguments. A tool that works on one pick restarts,
// as Point and Circle do, for a run of edits; each edit is its own undo step.
//
// What a pick takes, and what a tool shows before the click, are the rules
// of docs/drawing.md, "What a vertex tool acts on" - the owner's request of
// 2026-09-30, that inserting a vertex gave "no visual clue where the vertex
// is going" and that a tool asked for a polyline "but what about the already
// selected vertex":
//   - ONE function, resolve(), decides what a pick takes, for the preview,
//     the click and a typed point alike: the grips hot when the tool started
//     (its handles), else the nearest SELECTED polyline within reach, else
//     the polyline under the cursor - never another kind of entity, so a
//     survey point on a vertex cannot take the pick from its string;
//   - a vertex is picked within the view's 12 px, a segment or a point on a
//     polyline within its 8 px: never "the nearest vertex" at any distance;
//   - each one-polyline tool PLANS its edit with the geo:: call the commit
//     makes, and the preview draws the plan by role (tool_feedback.hpp): what
//     a click takes, adds and removes. A plan refused for the pick itself (an
//     end vertex for Fillet) is refused at the pick, before a value is asked;
//   - an edit is made from the polyline as it was picked, and one changed
//     since is refused, never silently reverted.

#include <algorithm>
#include <cmath>
#include <cstddef>
#include <functional>
#include <limits>
#include <map>
#include <memory>
#include <optional>
#include <set>
#include <string>
#include <string_view>
#include <tuple>
#include <utility>
#include <vector>

#include "everyday_support.hpp"
#include "families.hpp"
#include "katana/cad/drawing/drafting.hpp"
#include "katana/cad/drawing/vertex_editing.hpp"
#include "katana/cad/selection.hpp"
#include "katana/core/text.hpp"
#include "katana/math/numerics.hpp"

namespace katana::cad::tools {

namespace {

namespace geo = katana::geometry;
namespace tol = katana::math::tolerance;
using everyday::fixed;
using katana::core::ErrorCode;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::entity::EntityType;
using katana::entity::Geometry;
using katana::entity::kInvalidEntityId;
using katana::entity::PointGeometry;
using katana::geometry::CurvePolyline2;
using katana::geometry::Point2;
using katana::geometry::Segment2;

// Only polylines are picked: a survey point or a text on a vertex - where
// the prompts send the cursor - must not take the pick from the string.
const std::set<EntityType>& polylineKinds()
{
    static const std::set<EntityType> kinds{EntityType::Polyline, EntityType::CurvePolyline};
    return kinds;
}

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

std::string plural(std::size_t count, std::string_view noun)
{
    if (noun == "vertex") {
        return std::to_string(count) + (count == 1 ? " vertex" : " vertices");
    }
    return std::to_string(count) + " " + std::string(noun) + (count == 1 ? "" : "s");
}

std::string heightLabel(const std::optional<double>& height)
{
    return height ? "z " + fixed(*height) : std::string("no height");
}

// A whole number typed at a pick: vertex (or segment) N of the polyline in
// play. Only digits, so "1.5" and "-1" are refused rather than rounded.
std::optional<std::size_t> wholeNumber(std::string_view text)
{
    if (text.empty() || text.size() > 9 ||
        !std::ranges::all_of(text, [](char c) { return c >= '0' && c <= '9'; })) {
        return std::nullopt;
    }
    std::size_t value = 0;
    for (const char c : text) {
        value = value * 10 + static_cast<std::size_t>(c - '0');
    }
    return value;
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

// ---- pieces of a polyline -------------------------------------------------------------------

Geometry pieceOf(const CurvePolyline2& shape, std::size_t segment)
{
    return std::visit([](const auto& piece) { return Geometry{piece}; }, shape.segment(segment));
}

Geometry wholeOf(const CurvePolyline2& shape)
{
    if (shape.hasArcs()) {
        return Geometry{shape};
    }
    return Geometry{geo::Polyline2{shape.positions(), shape.closed}};
}

double distanceToPiece(const CurvePolyline2& shape, std::size_t segment, const Point2& at)
{
    return std::visit([&at](const auto& piece) { return piece.distanceTo(at); },
                      shape.segment(segment));
}

Point2 closestOnPiece(const CurvePolyline2& shape, std::size_t segment, const Point2& at)
{
    return std::visit([&at](const auto& piece) { return piece.closestPoint(at); },
                      shape.segment(segment));
}

Point2 middleOfPiece(const CurvePolyline2& shape, std::size_t segment)
{
    return std::visit([](const auto& piece) { return piece.pointAt(0.5); }, shape.segment(segment));
}

// The segment that ends at vertex `v`, and the one that starts at it, where
// the polyline has them.
std::optional<std::size_t> segmentBefore(const CurvePolyline2& shape, std::size_t v)
{
    if (v > 0) {
        return v - 1;
    }
    if (shape.closed && shape.vertices.size() > 1) {
        return shape.vertices.size() - 1;
    }
    return std::nullopt;
}

std::optional<std::size_t> segmentAfter(const CurvePolyline2& shape, std::size_t v)
{
    return v < shape.segmentCount() ? std::optional<std::size_t>(v) : std::nullopt;
}

// The segments walked forward from vertex `from` to vertex `to`.
std::vector<std::size_t> segmentsBetween(const CurvePolyline2& shape, std::size_t from,
                                         std::size_t to)
{
    std::vector<std::size_t> out;
    const std::size_t n = shape.vertices.size();
    for (std::size_t i = from; i != to && out.size() < n; i = (i + 1) % n) {
        out.push_back(i);
    }
    return out;
}

FeedbackMark vertexMark(FeedbackRole role, const Point2& at, std::string label = {})
{
    return FeedbackMark{role, Geometry{PointGeometry{at}}, std::move(label)};
}

FeedbackMark pieceMark(FeedbackRole role, Geometry piece, std::string label = {})
{
    return FeedbackMark{role, std::move(piece), std::move(label)};
}

// ---- what a pick chose, and what the tool will do with it ------------------------------------

// What one pick chose, kept as it was chosen - the polyline's shape then
// included - so the plan and the edit work from what the user saw, and the
// edit can tell that the polyline has changed since (stale()).
struct Chosen {
    EntityId polyline = kInvalidEntityId;
    CurvePolyline2 shape;
    std::optional<std::size_t> vertex;
    std::optional<std::size_t> segment;
    Point2 at; // the vertex, or where on the segment or the polyline
    bool fromHandle = false; // a grip made hot before the tool, not a click
};

struct Collected {
    const Document* document = nullptr;
    // One per pick step, except that Delete's hot vertices are several picks
    // answering its one step.
    std::vector<Chosen> picks;
    std::vector<Point2> points;
    // The z of a point typed as x,y,z: Insert's new vertex's height, Move's.
    std::optional<double> height;
    std::vector<std::string> values;
    std::vector<EntityId> selection;
    // Straighten and Grade on a closed polyline: the side typed O chose.
    bool otherSide = false;
    // Insert: the hot vertex or segment the new vertex goes beside.
    std::optional<Chosen> anchor;
    // Said before the first prompt: why what was chosen before the tool is
    // not what it uses - "2 vertices are chosen", an end vertex for Fillet,
    // a selected polyline on a locked layer. A script's answer() gives the
    // reason; the tool adds the prompt.
    std::string note;
};

// Insert beside a hot handle: the handle's segment, or whichever of a hot
// vertex's two segments is nearer `at` - at any distance, since the user
// chose it - with the point put on that segment.
Chosen besideAnchor(const Chosen& anchor, const Point2& at)
{
    Chosen chosen = anchor;
    chosen.fromHandle = true;
    std::size_t segment = anchor.segment.value_or(0);
    if (anchor.vertex) {
        const auto before = segmentBefore(anchor.shape, *anchor.vertex);
        const auto after = segmentAfter(anchor.shape, *anchor.vertex);
        if (before && after) {
            segment = distanceToPiece(anchor.shape, *before, at) <
                              distanceToPiece(anchor.shape, *after, at)
                          ? *before
                          : *after;
        } else {
            segment = before ? *before : after.value_or(0);
        }
    }
    chosen.vertex.reset();
    chosen.segment = segment;
    chosen.at = closestOnPiece(anchor.shape, segment, at);
    return chosen;
}

// Enter beside a hot handle: the middle of the handle's segment, or of the
// segment after a hot vertex (before it, at an open polyline's end).
Chosen middleBeside(const Chosen& anchor)
{
    Chosen chosen = anchor;
    chosen.fromHandle = true;
    std::size_t segment = anchor.segment.value_or(0);
    if (anchor.vertex) {
        const auto after = segmentAfter(anchor.shape, *anchor.vertex);
        segment = after ? *after : segmentBefore(anchor.shape, *anchor.vertex).value_or(0);
    }
    chosen.vertex.reset();
    chosen.segment = segment;
    chosen.at = middleOfPiece(anchor.shape, segment);
    return chosen;
}

// An edit of one polyline, from the shape it was picked as.
struct Edit {
    EntityId polyline = kInvalidEntityId;
    CurvePolyline2 before;
    CurvePolyline2 after;
};

// What a tool would do with what it has collected - the one answer the
// preview draws and the commit executes (docs/drawing.md, R3).
struct Plan {
    // The pick (or point) cannot be taken: said in the preview, and a click
    // there is refused with it.
    std::optional<std::string> refusal;
    // The edit fails at a value's default (a radius that does not fit): said
    // in the preview, but the pick is taken and the value asked for.
    std::optional<std::string> warning;
    std::string command; // the undo name: VERTEX_INSERT, STRAIGHTEN, ...
    std::vector<Edit> edits;
    // Edit Vertices' answer: the polyline to select.
    std::optional<std::vector<EntityId>> selection;
    // Done with nothing to do (a height set to what it is): no undo step.
    std::optional<std::string> noChange;
    std::string message;
    ToolFeedback feedback;
};

using Planner = std::function<Plan(const Collected&, const ToolContext&)>;

struct Step {
    enum class Kind {
        Vertex,    // one vertex of a polyline, within the vertex reach
        Segment,   // one segment of a polyline, within the pick reach
        Polyline,  // one polyline, within the pick reach
        OnLine,    // a point ON a polyline (snapped, then put on it): where a vertex goes
        Point,     // a point (snapped): the vertex's new place, the arc's point
        Value,     // typed text; Enter takes the default when there is one
        Selection, // several polylines, then Enter
    };
    Kind kind = Kind::Vertex;
    // What the step asks, given the one polyline in play, if any.
    std::function<std::string(const Collected&, std::optional<EntityId> inPlay)> prompt;
    // Value: what Enter takes, shown as <default>.
    std::function<std::optional<std::string>(const Collected&)> fallback;
    // Vertex: on the polyline the pick before chose, as it was then.
    bool samePolyline = false;
    // Vertex: [Other side] is offered when the polyline is closed.
    bool otherSide = false;
    // A pick refused whatever follows (Grade's vertex with no height).
    std::function<std::optional<std::string>(const Chosen&)> check;
    // The label of the Target the pick shows before the tool can plan.
    std::function<std::string(const Chosen&)> label;
};

bool isPick(Step::Kind kind)
{
    return kind == Step::Kind::Vertex || kind == Step::Kind::Segment ||
           kind == Step::Kind::Polyline || kind == Step::Kind::OnLine;
}

struct Script {
    std::vector<Step> steps;
    // One-polyline tools: the plan, drawn and committed.
    Planner plan;
    // Selection tools: the finish, as it always was.
    std::function<ToolStep(const Collected&)> finish;
    // What the tool's handles answer: fills `collected` and says how many
    // leading steps that answers (0: none, the tool asks as usual).
    std::function<std::size_t(const ToolContext&, Collected&)> answer;
    // The prompt when the handles have answered every step: "Press Enter to
    // delete vertex 2 of polyline 12, or click another vertex".
    std::function<std::string(const Collected&)> ready;
    // Set Height: a click at the value step takes the height there.
    std::function<std::optional<std::string>(const Collected&, const ToolContext&, const Point2&)>
        valueFromClick;
    // What an edit made is remembered for the next use (VertexDefaults):
    // Fillet's radius, Chamfer's distances. Called once the plan is
    // committed, never by a preview, which plans at the remembered values.
    std::function<void(const Collected&)> remember;
    bool restarts = false;
};

// A second pick of the vertex the pick before took (Straighten's, Grade's):
// the cursor is still on it just after that pick, so it is shown as taken,
// not refused in red, and a click there says so.
std::optional<std::string> alreadyPicked(const Collected& c, const Step& step, const Chosen& chosen)
{
    if (!step.samePolyline || c.picks.empty() || !chosen.vertex ||
        c.picks.back().polyline != chosen.polyline || c.picks.back().vertex != chosen.vertex) {
        return std::nullopt;
    }
    return "vertex " + std::to_string(*chosen.vertex) + " is picked already; click the other one";
}

std::string staleSentence(EntityId id)
{
    return "polyline " + std::to_string(id) + " changed since it was picked; pick it again";
}

std::optional<CurvePolyline2> polylineNow(const Document* document, EntityId id)
{
    const Entity* entity = document != nullptr ? document->model().entities.find(id) : nullptr;
    return entity != nullptr ? readPolyline(*entity) : std::nullopt;
}

std::vector<EntityId> polylinesAmong(const Document* document, const std::vector<EntityId>& ids)
{
    std::vector<EntityId> out;
    if (document == nullptr) {
        return out;
    }
    for (const EntityId id : ids) {
        if (const Entity* entity = document->model().entities.find(id);
            entity != nullptr && isPolylineEntity(*entity)) {
            out.push_back(id);
        }
    }
    return out;
}

// ONE command for a plan's edits, each checked against the shape it was
// planned from: a polyline changed since (by the panel, an agent, another
// view) is refused, never overwritten with the old shape.
katana::commands::CommandPtr commitEdits(std::string name, std::vector<Edit> edits)
{
    std::vector<EntityId> ids;
    std::map<EntityId, Edit> byId;
    for (Edit& edit : edits) {
        ids.push_back(edit.polyline);
        byId.emplace(edit.polyline, std::move(edit));
    }
    return editEachPolyline(
        std::move(ids), std::move(name),
        [byId = std::move(byId)](const katana::entity::Model&, EntityId id,
                                 const CurvePolyline2& now) -> geo::PolylineResult {
            const Edit& edit = byId.at(id);
            if (!(now == edit.before)) {
                return katana::core::Error{ErrorCode::InvalidState, staleSentence(id), {}};
            }
            return edit.after;
        });
}

// ---- the state machine ---------------------------------------------------------------------

class ScriptTool final : public InteractiveTool {
  public:
    ScriptTool(const ToolContext& context, Script script)
        : context_(context), script_(std::move(script))
    {
        collected_.document = context.document;
        own_ = polylinesAmong(context.document, context.selection);
        const std::vector<Step>& steps = script_.steps;
        if (!steps.empty() && steps.front().kind == Step::Kind::Selection) {
            // A selection step is answered by the selection the tool started
            // on, when it holds polylines.
            if (!own_.empty()) {
                collected_.selection = own_;
                step_ = answered_ = 1;
                answeredBy_ = AnsweredBy::Selection;
            }
            return;
        }
        std::string why;
        if (script_.answer && context.document != nullptr) {
            Collected answered = collected_;
            const std::size_t count = script_.answer(context_, answered);
            const auto refused = count > 0 ? refusalOf(answered, count) : std::nullopt;
            if (count > 0 && !refused) {
                collected_ = std::move(answered);
                step_ = answered_ = count;
                answeredBy_ = AnsweredBy::Handles;
                return;
            }
            collected_.anchor = std::move(answered.anchor);
            why = refused ? *refused : std::move(answered.note);
        }
        if (why.empty()) {
            why = lockedNote();
        }
        if (!why.empty() && !steps.empty()) {
            // What was chosen before the tool and cannot be used is never
            // dropped without a word: the reason, then the prompt.
            collected_.note = why + ". " + steps.front().prompt(collected_, inPlay());
        }
    }

    [[nodiscard]] std::string prompt() const override
    {
        const std::vector<Step>& steps = script_.steps;
        if (step_ >= steps.size()) {
            if (answeredBy_ == AnsweredBy::Handles && script_.ready) {
                return script_.ready(collected_);
            }
            return "Press Enter to apply to the " +
                   plural(collected_.selection.size(), "selected polyline");
        }
        const Step& step = steps[step_];
        std::string text = step_ == 0 && !collected_.note.empty()
                               ? collected_.note
                               : step.prompt(collected_, inPlay());
        if (step.fallback) {
            if (const auto shown = step.fallback(collected_)) {
                text += " <" + *shown + ">";
            }
        }
        if (step.otherSide && sideOffered()) {
            text += " [Other side]";
        }
        return text;
    }

    [[nodiscard]] ToolInput expects() const override
    {
        const std::vector<Step>& steps = script_.steps;
        if (step_ >= steps.size()) {
            // Answered by handles, a click picks anew: an entity pick.
            return answeredBy_ == AnsweredBy::Handles ? ToolInput::Entity : ToolInput::Value;
        }
        switch (steps[step_].kind) {
        case Step::Kind::Vertex:
        case Step::Kind::Segment:
        case Step::Kind::Polyline:
            return ToolInput::Entity;
        case Step::Kind::OnLine:
        case Step::Kind::Point:
            return ToolInput::Point;
        case Step::Kind::Value:
            return ToolInput::Value;
        case Step::Kind::Selection:
            return ToolInput::Selection;
        }
        return ToolInput::Value;
    }

    // The view picks any kind of entity at its own aperture; what a pick
    // takes is resolve()'s, so the preview and the click cannot disagree.
    ToolStep entity(EntityId /*id*/, const Point2& at) override { return pointAt(at, std::nullopt); }
    ToolStep point(const Point2& at) override { return pointAt(at, std::nullopt); }
    ToolStep point3d(const Point2& at, double z) override { return pointAt(at, z); }

    ToolStep value(std::string_view text) override
    {
        const std::string typed(katana::core::trimmed(text));
        const std::vector<Step>& steps = script_.steps;
        const bool atValue = step_ < steps.size() && steps[step_].kind == Step::Kind::Value;
        const bool atPick = step_ < steps.size() ? isPick(steps[step_].kind)
                                                 : answeredBy_ == AnsweredBy::Handles;
        if (!atValue && sideOffered() && isOption(typed, "Other")) {
            collected_.otherSide = !collected_.otherSide;
            return ToolStep::next("the other side");
        }
        if (atValue) {
            collected_.values.push_back(typed);
            return advance();
        }
        if (atPick) {
            if (const auto index = wholeNumber(typed)) {
                return pickByNumber(*index);
            }
        }
        return ToolStep::rejected("'" + typed + "' is not taken here; " + prompt());
    }

    ToolStep enter() override
    {
        const std::vector<Step>& steps = script_.steps;
        if (step_ >= steps.size()) {
            if (answeredBy_ != AnsweredBy::None) {
                return finish();
            }
            return ToolStep::done(nullptr);
        }
        const Step& step = steps[step_];
        if (step.kind == Step::Kind::Value && step.fallback) {
            if (const auto fallback = step.fallback(collected_)) {
                collected_.values.push_back(*fallback);
                return advance();
            }
        }
        if (step.kind == Step::Kind::Selection) {
            const auto polylines =
                context_.document != nullptr
                    ? polylinesAmong(context_.document, context_.document->selection().ids())
                    : std::vector<EntityId>{};
            if (polylines.empty()) {
                return ToolStep::rejected("no polyline is selected; select polylines, then press "
                                          "Enter");
            }
            collected_.selection = polylines;
            return advance();
        }
        if (step.kind == Step::Kind::OnLine && collected_.anchor) {
            return take(middleBeside(*collected_.anchor), std::nullopt);
        }
        if (step_ == answered_) {
            return ToolStep::done(nullptr); // nothing collected: Enter ends the tool
        }
        return ToolStep::rejected(prompt());
    }

    ToolStep undo() override
    {
        if (step_ <= answered_) {
            return ToolStep::rejected("nothing to undo in this tool");
        }
        stepBack();
        return ToolStep::next();
    }

    [[nodiscard]] ToolFeedback preview(const Point2& cursor) const override
    {
        if (!script_.plan) {
            return {};
        }
        const std::vector<Step>& steps = script_.steps;
        if (step_ >= steps.size()) {
            // The handles answered every step, and a click picks anew (R3):
            // where one would take something the preview is THAT click's -
            // it drew the handles' plan, and a click deleted a vertex nobody
            // had marked. Elsewhere a click takes nothing, and the preview is
            // what Enter does, said as Enter's.
            if (auto click = pickPreview(fresh(), 0, cursor)) {
                return *click;
            }
            ToolFeedback enter = feedbackOf(script_.plan(collected_, context_));
            if (!enter.caption.empty()) {
                enter.caption = "Enter: " + enter.caption;
            }
            return enter;
        }
        const Step& step = steps[step_];
        if (step.kind == Step::Kind::Selection) {
            return {};
        }
        if (isPick(step.kind)) {
            if (auto shown = pickPreview(collected_, step_, cursor)) {
                return *shown;
            }
            return idle();
        }
        Collected c = collected_;
        std::size_t from = step_;
        if (step.kind == Step::Kind::Point) {
            c.points.push_back(cursor);
            ++from;
        }
        if (!canPlan(c, from)) {
            return idle();
        }
        return feedbackOf(script_.plan(withDefaults(std::move(c), from), context_));
    }

    [[nodiscard]] std::optional<Point2> lastPoint() const override
    {
        // Relative input (@dx,dy) is from the picked vertex: "move this
        // vertex 2 m east" is @2,0.
        if (collected_.picks.empty()) {
            return std::nullopt;
        }
        const Chosen& last = collected_.picks.back();
        return last.vertex ? last.shape.vertices[*last.vertex].position : last.at;
    }

    // The picked vertex's height, which Move Vertex's @dx,dy,dz changes.
    [[nodiscard]] std::optional<double> lastHeight() const override
    {
        if (collected_.picks.empty() || !collected_.picks.back().vertex) {
            return std::nullopt;
        }
        const Chosen& last = collected_.picks.back();
        return last.shape.vertices[*last.vertex].height;
    }

    // A point ON a polyline (Insert's) takes a snap only where it lands on
    // the line the pick takes there, within a pixel, and the click there
    // would be taken. So an arc segment's Center - offered while the arc is
    // hovered - another string's end or middle beside the line, and the
    // line's own vertices (always too close to insert at) are passed over,
    // and the cursor stays on the line; its Midpoint and a crossing are kept.
    [[nodiscard]] bool takesSnap(const SnapResult& snap) const override
    {
        const std::vector<Step>& steps = script_.steps;
        if (step_ >= steps.size() || steps[step_].kind != Step::Kind::OnLine) {
            return true;
        }
        const Step& step = steps[step_];
        auto chosen = resolve(collected_, step, snap.point);
        // One pixel: pickReach is the view's 8 px (kPickAperturePixels).
        const double pixel = pickReach(context_) / 8.0;
        if (!chosen || chosen->at.distanceTo(snap.point) > pixel) {
            return false;
        }
        Collected c = collected_;
        c.picks.push_back(std::move(*chosen));
        if (script_.plan && canPlan(c, step_ + 1)) {
            return !script_.plan(withDefaults(std::move(c), step_ + 1), context_).refusal;
        }
        return true;
    }

  private:
    enum class AnsweredBy { None, Selection, Handles };

    // The one polyline known without a pick: the last pick's, the anchor's,
    // or the one selected polyline, when a pick may take it.
    [[nodiscard]] std::optional<EntityId> inPlay() const
    {
        if (!collected_.picks.empty()) {
            return collected_.picks.back().polyline;
        }
        if (collected_.anchor) {
            return collected_.anchor->polyline;
        }
        if (own_.size() == 1 && pickableId(own_.front())) {
            return own_.front();
        }
        return std::nullopt;
    }

    [[nodiscard]] bool pickableId(EntityId id) const
    {
        const Entity* entity =
            context_.document != nullptr ? context_.document->model().entities.find(id) : nullptr;
        return entity != nullptr && pickable(context_, *entity);
    }

    // Why a selected polyline will not be taken: it is drawn, but on a
    // locked layer (THE rule, pickable). A hidden one says nothing - it is
    // not there to be clicked. Empty when there is none.
    [[nodiscard]] std::string lockedNote() const
    {
        const Document* document = context_.document;
        if (document == nullptr) {
            return {};
        }
        for (const EntityId id : own_) {
            const Entity* entity = document->model().entities.find(id);
            if (entity != nullptr && !pickable(context_, *entity) &&
                isDrawn(document->model(), *entity, viewOf(context_))) {
                return "polyline " + std::to_string(id) + " is on the locked layer " +
                       entity->layer + "; unlock it to edit it";
            }
        }
        return {};
    }

    // Nothing collected, as a click after the handles picks from.
    [[nodiscard]] Collected fresh() const
    {
        Collected c;
        c.document = context_.document;
        return c;
    }

    // What the preview shows where the cursor finds nothing new: the
    // vertices of the polyline in play, and what the picks before took -
    // Straighten's first "keep" stays marked while the cursor is off the line.
    [[nodiscard]] ToolFeedback idle() const
    {
        ToolFeedback feedback;
        feedback.focus = inPlay();
        std::size_t step = 0;
        for (const Chosen& pick : collected_.picks) {
            while (step + 1 < script_.steps.size() && !isPick(script_.steps[step].kind)) {
                ++step;
            }
            const ToolFeedback taken = targetOf(script_.steps[step], pick);
            feedback.marks.insert(feedback.marks.end(), taken.marks.begin(), taken.marks.end());
            if (step + 1 < script_.steps.size()) {
                ++step;
            }
        }
        return feedback;
    }

    // The pick a click at `cursor` would make for step `at` after `c`
    // collected, and the plan it leads to; nullopt when nothing is in reach.
    [[nodiscard]] std::optional<ToolFeedback> pickPreview(Collected c, std::size_t at,
                                                          const Point2& cursor) const
    {
        const Step& step = script_.steps[at];
        auto chosen = resolve(c, step, cursor);
        if (!chosen) {
            return std::nullopt;
        }
        if (auto already = alreadyPicked(c, step, *chosen)) {
            ToolFeedback taken = idle();
            taken.caption = *already;
            return taken;
        }
        if (step.check) {
            if (auto why = step.check(*chosen)) {
                ToolFeedback refused = targetOf(step, *chosen);
                refused.refused = true;
                refused.caption = *why;
                return refused;
            }
        }
        c.picks.push_back(std::move(*chosen));
        if (!canPlan(c, at + 1)) {
            return targetOf(step, c.picks.back());
        }
        return feedbackOf(script_.plan(withDefaults(std::move(c), at + 1), context_));
    }

    [[nodiscard]] bool sideOffered() const
    {
        const bool offers = std::ranges::any_of(script_.steps,
                                                [](const Step& step) { return step.otherSide; });
        return offers && !collected_.picks.empty() && collected_.picks.front().shape.closed;
    }

    // Every step from `from` on is a value with a default: the tool can plan
    // (and so preview, and refuse) now, at those defaults.
    [[nodiscard]] bool canPlan(const Collected& c, std::size_t from) const
    {
        for (std::size_t i = from; i < script_.steps.size(); ++i) {
            const Step& step = script_.steps[i];
            if (step.kind != Step::Kind::Value || !step.fallback || !step.fallback(c)) {
                return false;
            }
        }
        return true;
    }

    [[nodiscard]] Collected withDefaults(Collected c, std::size_t from) const
    {
        for (std::size_t i = from; i < script_.steps.size(); ++i) {
            const Step& step = script_.steps[i];
            if (!step.fallback) {
                continue;
            }
            if (const auto fallback = step.fallback(c)) {
                c.values.push_back(*fallback);
            }
        }
        return c;
    }

    // Why what the handles answered cannot be taken - a pick its step's check
    // refuses, or the plan's refusal where the plan can be made - or nullopt
    // when it can be.
    [[nodiscard]] std::optional<std::string> refusalOf(const Collected& c, std::size_t count) const
    {
        for (std::size_t i = 0; i < count && i < script_.steps.size(); ++i) {
            const Step& step = script_.steps[i];
            if (step.check && i < c.picks.size()) {
                if (auto why = step.check(c.picks[i])) {
                    return why;
                }
            }
        }
        if (script_.plan && canPlan(c, count)) {
            return script_.plan(withDefaults(c, count), context_).refusal;
        }
        return std::nullopt;
    }

    [[nodiscard]] static ToolFeedback feedbackOf(const Plan& plan)
    {
        ToolFeedback feedback = plan.feedback;
        if (plan.refusal) {
            feedback.refused = true;
            feedback.caption = *plan.refusal;
        } else if (plan.warning) {
            feedback.caption = *plan.warning;
        }
        return feedback;
    }

    // What a pick shows before the tool can plan: only what it takes.
    [[nodiscard]] static ToolFeedback targetOf(const Step& step, const Chosen& chosen)
    {
        ToolFeedback feedback;
        feedback.focus = chosen.polyline;
        if (chosen.vertex) {
            feedback.marks.push_back(vertexMark(
                FeedbackRole::Target, chosen.shape.vertices[*chosen.vertex].position,
                step.label ? step.label(chosen) : std::to_string(*chosen.vertex)));
        } else if (chosen.segment && step.kind == Step::Kind::Segment) {
            feedback.marks.push_back(pieceMark(FeedbackRole::Target,
                                               pieceOf(chosen.shape, *chosen.segment)));
        } else {
            feedback.marks.push_back(pieceMark(FeedbackRole::Target, wholeOf(chosen.shape)));
        }
        return feedback;
    }

    // ---- what a pick takes (docs/drawing.md, R1 and R2) ----

    // `shape` (polyline `id`) as a candidate for `step` at `at`, with its
    // distance; nullopt when nothing of it is within `reach`.
    [[nodiscard]] static std::optional<std::pair<Chosen, double>>
    candidate(const Step& step, EntityId id, const CurvePolyline2& shape, const Point2& at,
              double reach)
    {
        Chosen chosen;
        chosen.polyline = id;
        chosen.shape = shape;
        double distance = std::numeric_limits<double>::infinity();
        switch (step.kind) {
        case Step::Kind::Vertex: {
            const auto vertex = geo::nearestVertex(shape, at);
            if (!vertex) {
                return std::nullopt;
            }
            chosen.vertex = *vertex;
            chosen.at = shape.vertices[*vertex].position;
            distance = chosen.at.distanceTo(at);
            break;
        }
        case Step::Kind::Segment: {
            const auto segment = geo::nearestSegment(shape, at);
            if (!segment) {
                return std::nullopt;
            }
            chosen.segment = *segment;
            chosen.at = closestOnPiece(shape, *segment, at);
            distance = distanceToPiece(shape, *segment, at);
            break;
        }
        case Step::Kind::Polyline:
        case Step::Kind::OnLine: {
            const auto nearest = shape.nearest(at);
            if (!nearest || shape.segmentCount() == 0) {
                return std::nullopt;
            }
            chosen.segment = nearest->segment;
            chosen.at = nearest->point;
            distance = nearest->distance;
            break;
        }
        case Step::Kind::Point:
        case Step::Kind::Value:
        case Step::Kind::Selection:
            return std::nullopt;
        }
        if (!(distance <= reach)) {
            return std::nullopt;
        }
        return std::pair<Chosen, double>(std::move(chosen), distance);
    }

    // What a pick at `at` takes for `step` after `c` collected: the anchor's
    // segment (Insert beside a hot vertex), else the pick before's polyline
    // (Straighten's second vertex), else the nearest selected polyline within
    // reach, else the polyline the view would pick there. Nothing else: never
    // another kind of entity, never a vertex out of reach, and never what the
    // view may not pick - a selected polyline on a hidden or locked layer was
    // previewed and edited through the selection, round the view's own pick.
    [[nodiscard]] std::optional<Chosen> resolve(const Collected& c, const Step& step,
                                                const Point2& at) const
    {
        if (step.kind == Step::Kind::OnLine && c.anchor) {
            return besideAnchor(*c.anchor, at);
        }
        const double reach =
            step.kind == Step::Kind::Vertex ? vertexReach(context_) : pickReach(context_);
        if (step.samePolyline && !c.picks.empty()) {
            const Chosen& first = c.picks.back();
            auto found = candidate(step, first.polyline, first.shape, at, reach);
            return found ? std::optional<Chosen>(std::move(found->first)) : std::nullopt;
        }
        const Document* document = context_.document;
        if (document == nullptr) {
            return std::nullopt;
        }
        std::optional<std::pair<Chosen, double>> best;
        if (!own_.empty()) {
            std::vector<katana::geometry::SpatialId> nearby;
            document->spatialIndex().query(at, reach, nearby);
            for (const auto spatial : nearby) {
                const EntityId id = static_cast<EntityId>(spatial);
                if (!std::ranges::binary_search(own_, id) || !pickableId(id)) {
                    continue;
                }
                const auto shape = polylineNow(document, id);
                if (!shape) {
                    continue;
                }
                auto found = candidate(step, id, *shape, at, reach);
                if (found && (!best || found->second < best->second)) {
                    best = std::move(found);
                }
            }
        }
        if (best) {
            return std::move(best->first);
        }
        if (const auto id = pickUnder(context_, at, polylineKinds(), reach)) {
            if (const auto shape = polylineNow(document, *id)) {
                if (auto found = candidate(step, *id, *shape, at, reach)) {
                    return std::move(found->first);
                }
            }
        }
        return std::nullopt;
    }

    // ---- inputs ----

    // A click or a number after the handles answered every step picks anew,
    // from the first step. Refused, it leaves the handles as they were: a
    // Rejected step leaves the tool unchanged, and a click on nothing had
    // dropped them.
    template <typename Pick>
    ToolStep anew(Pick pick)
    {
        const auto saved = std::tuple{step_, answered_, answeredBy_, collected_};
        dropAnswers();
        ToolStep step = pick();
        if (step.outcome == ToolStep::Outcome::Rejected) {
            std::tie(step_, answered_, answeredBy_, collected_) = saved;
        }
        return step;
    }

    ToolStep pointAt(const Point2& at, std::optional<double> z)
    {
        const std::vector<Step>& steps = script_.steps;
        if (step_ >= steps.size()) {
            if (answeredBy_ != AnsweredBy::Handles) {
                return InteractiveTool::point(at);
            }
            if (!resolve(fresh(), steps.front(), at)) {
                return ToolStep::rejected(prompt());
            }
            return anew([&] { return pointAt(at, z); });
        }
        const Step& step = steps[step_];
        switch (step.kind) {
        case Step::Kind::Vertex:
        case Step::Kind::Segment:
        case Step::Kind::Polyline:
        case Step::Kind::OnLine: {
            auto chosen = resolve(collected_, step, at);
            if (!chosen) {
                return ToolStep::rejected(prompt());
            }
            return take(std::move(*chosen), step.kind == Step::Kind::OnLine ? z : std::nullopt);
        }
        case Step::Kind::Point:
            collected_.points.push_back(at);
            collected_.height = z;
            return advance();
        case Step::Kind::Value:
            if (script_.valueFromClick) {
                const auto taken = script_.valueFromClick(collected_, context_, at);
                if (!taken) {
                    return ToolStep::rejected("nothing with a height is within reach there; " +
                                              prompt());
                }
                collected_.values.push_back(*taken);
                return advance();
            }
            return ToolStep::rejected("type a value: " + prompt());
        case Step::Kind::Selection:
            return InteractiveTool::point(at);
        }
        return InteractiveTool::point(at);
    }

    // A typed whole number at a pick: vertex (or segment) N of the polyline
    // in play - a headless run can drive every pick tool by typing.
    ToolStep pickByNumber(std::size_t index)
    {
        if (step_ >= script_.steps.size()) {
            return anew([&] { return pickByNumber(index); });
        }
        const Step& step = script_.steps[step_];
        EntityId id = kInvalidEntityId;
        std::optional<CurvePolyline2> shape;
        if (step.samePolyline && !collected_.picks.empty()) {
            id = collected_.picks.back().polyline;
            shape = collected_.picks.back().shape;
        } else if (collected_.anchor) {
            id = collected_.anchor->polyline;
            shape = collected_.anchor->shape;
        } else if (own_.size() == 1 && pickableId(own_.front())) {
            id = own_.front();
            shape = polylineNow(context_.document, id);
        }
        if (!shape) {
            return ToolStep::rejected("a number names a vertex of the one selected polyline; "
                                      "select one polyline, or click instead");
        }
        Chosen chosen;
        chosen.polyline = id;
        chosen.shape = *shape;
        switch (step.kind) {
        case Step::Kind::Vertex:
            if (index >= shape->vertices.size()) {
                return ToolStep::rejected("polyline " + std::to_string(id) + " has no vertex " +
                                          std::to_string(index) + "; its vertices are 0 to " +
                                          std::to_string(shape->vertices.size() - 1));
            }
            chosen.vertex = index;
            chosen.at = shape->vertices[index].position;
            break;
        case Step::Kind::Segment:
        case Step::Kind::OnLine:
            if (index >= shape->segmentCount()) {
                return ToolStep::rejected("polyline " + std::to_string(id) + " has no segment " +
                                          std::to_string(index));
            }
            chosen.segment = index;
            chosen.at = middleOfPiece(*shape, index);
            break;
        default:
            return ToolStep::rejected("'" + std::to_string(index) + "' is not taken here; " +
                                      prompt());
        }
        return take(std::move(chosen), std::nullopt);
    }

    // A pick, taken unless its step or the plan refuses it (R3): the refusal
    // comes at the pick, before any value it would be followed by.
    ToolStep take(Chosen chosen, std::optional<double> z)
    {
        const Step& step = script_.steps[step_];
        if (auto already = alreadyPicked(collected_, step, chosen)) {
            return ToolStep::rejected(*already);
        }
        if (step.check) {
            if (auto why = step.check(chosen)) {
                return ToolStep::rejected(*why);
            }
        }
        Collected next = collected_;
        next.picks.push_back(std::move(chosen));
        if (z) {
            next.height = z;
        }
        if (script_.plan && canPlan(next, step_ + 1)) {
            const Plan plan = script_.plan(withDefaults(next, step_ + 1), context_);
            if (plan.refusal) {
                return ToolStep::rejected(*plan.refusal);
            }
        }
        collected_ = std::move(next);
        return advance();
    }

    ToolStep advance()
    {
        ++step_;
        if (step_ < script_.steps.size()) {
            return ToolStep::next();
        }
        return finish();
    }

    ToolStep finish()
    {
        stale_ = false;
        ToolStep done = script_.plan ? commit() : script_.finish(collected_);
        if (done.outcome == ToolStep::Outcome::Rejected) {
            if (stale_) {
                // The pick is out of date: start again from it.
                dropAnswers();
            } else if (step_ > answered_) {
                // The last input was not acceptable (a radius that does not
                // fit): step back to it, keeping everything before.
                stepBack();
            }
            return done;
        }
        done.restart = script_.restarts;
        step_ = 0;
        answered_ = 0;
        answeredBy_ = AnsweredBy::None;
        collected_ = Collected{};
        collected_.document = context_.document;
        return done;
    }

    ToolStep commit()
    {
        const Plan plan = script_.plan(collected_, context_);
        if (plan.refusal) {
            return ToolStep::rejected(*plan.refusal);
        }
        if (plan.warning) {
            return ToolStep::rejected(*plan.warning);
        }
        if (plan.noChange) {
            return ToolStep::done(nullptr, *plan.noChange);
        }
        for (const Edit& edit : plan.edits) {
            const auto now = polylineNow(context_.document, edit.polyline);
            if (!now || !(*now == edit.before)) {
                stale_ = true;
                return ToolStep::rejected(staleSentence(edit.polyline));
            }
        }
        if (plan.edits.empty()) {
            ToolStep step = ToolStep::done(nullptr, plan.message);
            step.selection = plan.selection;
            return step;
        }
        if (script_.remember) {
            script_.remember(collected_);
        }
        ToolStep step = ToolStep::done(commitEdits(plan.command, plan.edits), plan.message);
        step.selection = plan.selection ? plan.selection : stayOn(plan);
        return step;
    }

    // R5: after an edit the tool stays on the polyline - it becomes the
    // selection, so the restarted tool has it as its own and the next edit
    // is one click - unless the user had selected several things. One thing
    // selected moves to the polyline just edited: that one was mostly the
    // tool's own choice after an earlier edit, and kept, it had the prompt
    // naming the first polyline while the user worked on another.
    [[nodiscard]] std::optional<std::vector<EntityId>> stayOn(const Plan& plan) const
    {
        if (!script_.restarts || plan.edits.size() != 1) {
            return std::nullopt;
        }
        const EntityId id = plan.edits.front().polyline;
        if (context_.selection.size() <= 1) {
            return std::vector<EntityId>{id};
        }
        return std::nullopt;
    }

    void stepBack()
    {
        if (step_ == 0) {
            return;
        }
        --step_;
        switch (script_.steps[step_].kind) {
        case Step::Kind::Vertex:
        case Step::Kind::Segment:
        case Step::Kind::Polyline:
        case Step::Kind::OnLine:
            if (!collected_.picks.empty()) {
                collected_.picks.pop_back();
            }
            collected_.height.reset();
            break;
        case Step::Kind::Point:
            if (!collected_.points.empty()) {
                collected_.points.pop_back();
            }
            collected_.height.reset();
            break;
        case Step::Kind::Value:
            if (!collected_.values.empty()) {
                collected_.values.pop_back();
            }
            break;
        case Step::Kind::Selection:
            break;
        }
    }

    // Back to the first step with nothing collected: the handles (and the
    // note about them) are the user's choice before the tool, and a new
    // pick replaces them.
    void dropAnswers()
    {
        step_ = 0;
        answered_ = 0;
        answeredBy_ = AnsweredBy::None;
        collected_ = Collected{};
        collected_.document = context_.document;
    }

    ToolContext context_;
    Script script_;
    std::vector<EntityId> own_; // the polylines selected when the tool started
    std::size_t step_ = 0;
    std::size_t answered_ = 0; // the leading steps the selection or the handles answered
    AnsweredBy answeredBy_ = AnsweredBy::None;
    bool stale_ = false;
    Collected collected_;
};

// ---- handles: what was chosen before the tool ------------------------------------------------

// The hot grips of `kind` on polylines, as picks, each checked against the
// polyline as it is now: a grip whose vertex has moved since is not the
// vertex it names any more, and one whose layer has been hidden or locked
// since may not be taken (pickable); both are left out.
std::vector<Chosen> handlesOf(const ToolContext& context, GripKind kind)
{
    std::vector<Chosen> out;
    for (const Grip& grip : context.handles) {
        if (grip.kind != kind) {
            continue;
        }
        const Entity* entity = context.document != nullptr
                                   ? context.document->model().entities.find(grip.entity)
                                   : nullptr;
        if (entity == nullptr || !pickable(context, *entity)) {
            continue;
        }
        const auto shape = readPolyline(*entity);
        if (!shape) {
            continue;
        }
        Chosen chosen;
        chosen.polyline = grip.entity;
        chosen.shape = *shape;
        chosen.fromHandle = true;
        if (kind == GripKind::Vertex) {
            if (grip.index >= shape->vertices.size()) {
                continue;
            }
            chosen.vertex = grip.index;
            chosen.at = shape->vertices[grip.index].position;
        } else {
            if (grip.index >= shape->segmentCount()) {
                continue;
            }
            chosen.segment = grip.index;
            chosen.at = middleOfPiece(*shape, grip.index);
        }
        if (chosen.at.distanceTo(grip.position) > tol::kGeometric) {
            continue;
        }
        out.push_back(std::move(chosen));
    }
    return out;
}

// The segment between two hot vertices that are neighbours on one polyline.
std::optional<Chosen> segmentBetween(const Chosen& a, const Chosen& b)
{
    if (a.polyline != b.polyline || !a.vertex || !b.vertex) {
        return std::nullopt;
    }
    const CurvePolyline2& shape = a.shape;
    std::optional<std::size_t> segment;
    if (*a.vertex < shape.segmentCount() && shape.segmentEnd(*a.vertex) == *b.vertex) {
        segment = *a.vertex;
    } else if (*b.vertex < shape.segmentCount() && shape.segmentEnd(*b.vertex) == *a.vertex) {
        segment = *b.vertex;
    }
    if (!segment) {
        return std::nullopt;
    }
    Chosen chosen = a;
    chosen.vertex.reset();
    chosen.segment = segment;
    chosen.at = middleOfPiece(shape, *segment);
    return chosen;
}

// Why two hot vertices name no segment, for a tool that wanted one.
std::string noSegmentBetween(const Chosen& a, const Chosen& b)
{
    if (a.polyline != b.polyline) {
        return "the two chosen vertices are on different polylines, so they name no segment";
    }
    return "vertices " + std::to_string(*a.vertex) + " and " + std::to_string(*b.vertex) +
           " are not neighbours, so they name no segment";
}

std::size_t oneVertex(const ToolContext& context, Collected& c)
{
    auto hot = handlesOf(context, GripKind::Vertex);
    if (hot.size() > 1) {
        c.note = std::to_string(hot.size()) + " vertices are chosen, and this tool takes one";
    }
    if (hot.size() != 1) {
        return 0;
    }
    c.picks = std::move(hot);
    return 1;
}

std::size_t twoVertices(const ToolContext& context, Collected& c)
{
    auto hot = handlesOf(context, GripKind::Vertex);
    if (hot.size() == 2 && hot[0].polyline == hot[1].polyline && hot[0].vertex != hot[1].vertex) {
        c.picks = std::move(hot);
        return 2;
    }
    if (hot.size() == 1) {
        c.picks = std::move(hot);
        return 1;
    }
    if (hot.size() == 2) {
        c.note = "the two chosen vertices are on different polylines, and this tool takes two "
                 "on one";
    } else if (hot.size() > 2) {
        c.note = std::to_string(hot.size()) +
                 " vertices are chosen, and this tool takes two on one polyline";
    }
    return 0;
}

std::size_t oneSegment(const ToolContext& context, Collected& c)
{
    auto segments = handlesOf(context, GripKind::SegmentMid);
    if (segments.size() == 1) {
        c.picks = std::move(segments);
        return 1;
    }
    const auto hot = handlesOf(context, GripKind::Vertex);
    if (segments.empty() && hot.size() == 2) {
        if (auto between = segmentBetween(hot[0], hot[1])) {
            c.picks = {std::move(*between)};
            return 1;
        }
        c.note = noSegmentBetween(hot[0], hot[1]);
    } else if (segments.size() > 1) {
        c.note = std::to_string(segments.size()) +
                 " segment middles are chosen, and this tool takes one";
    } else if (!hot.empty()) {
        c.note = "a segment is chosen by its middle grip, or by the two vertices at its ends";
    }
    return 0;
}

std::string pickedVertex(const Collected& c, std::size_t i = 0)
{
    return "vertex " + std::to_string(*c.picks.at(i).vertex) + " of polyline " +
           std::to_string(c.picks.at(i).polyline);
}

// ---- prompts ---------------------------------------------------------------------------------

using Prompt = std::function<std::string(const Collected&, std::optional<EntityId>)>;

// "Click the vertex to delete", or with a polyline in play "Click the vertex
// of polyline 12 to delete": never "select the polyline" when it is known.
Prompt clickThe(std::string thing, std::string purpose)
{
    return [thing = std::move(thing), purpose = std::move(purpose)](
               const Collected&, std::optional<EntityId> inPlay) {
        return "Click the " + thing +
               (inPlay ? " of polyline " + std::to_string(*inPlay) : std::string()) + " " + purpose;
    };
}

Prompt fixedPrompt(std::string text)
{
    return [text = std::move(text)](const Collected&, std::optional<EntityId>) { return text; };
}

Step vertexStep(Prompt prompt)
{
    Step step;
    step.kind = Step::Kind::Vertex;
    step.prompt = std::move(prompt);
    return step;
}

Step segmentStep(Prompt prompt)
{
    Step step;
    step.kind = Step::Kind::Segment;
    step.prompt = std::move(prompt);
    return step;
}

Step pointStep(std::string prompt)
{
    Step step;
    step.kind = Step::Kind::Point;
    step.prompt = fixedPrompt(std::move(prompt));
    return step;
}

Step valueStep(Prompt prompt, std::function<std::optional<std::string>(const Collected&)> fallback = {})
{
    Step step;
    step.kind = Step::Kind::Value;
    step.prompt = std::move(prompt);
    step.fallback = std::move(fallback);
    return step;
}

Step selectPolylines(const std::string& verb)
{
    Step step;
    step.kind = Step::Kind::Selection;
    step.prompt = fixedPrompt("Select polylines to " + verb + ", then press Enter");
    return step;
}

std::function<std::optional<std::string>(const Collected&)> remembered(const double& value)
{
    return [&value](const Collected&) -> std::optional<std::string> { return number(value); };
}

// ---- the plans --------------------------------------------------------------------------------

Plan planInsert(const Collected& c, const ToolContext& context)
{
    const Chosen& on = c.picks.front();
    const CurvePolyline2& shape = on.shape;
    const std::size_t s = *on.segment;
    const std::size_t a = s;
    const std::size_t b = shape.segmentEnd(s);
    Plan plan;
    plan.command = "VERTEX_INSERT";
    ToolFeedback& f = plan.feedback;
    f.focus = on.polyline;
    f.marks.push_back(pieceMark(FeedbackRole::Target, pieceOf(shape, s)));
    if (c.anchor && c.anchor->vertex) {
        f.marks.push_back(vertexMark(FeedbackRole::Target,
                                     shape.vertices[*c.anchor->vertex].position,
                                     std::to_string(*c.anchor->vertex)));
    }
    if (c.anchor) {
        // Where Enter puts it - the middle of a segment the cursor may not be
        // on: it inserted on an edge the preview never marked.
        f.marks.push_back(vertexMark(FeedbackRole::Enter, middleBeside(*c.anchor).at, "Enter"));
    }
    // A point this near a vertex is taken as meant for the vertex: at the
    // view's pick aperture a new vertex there could not be told from it.
    const double reach = pickReach(context);
    const double toA = on.at.distanceTo(shape.vertices[a].position);
    const double toB = on.at.distanceTo(shape.vertices[b].position);
    if (std::min(toA, toB) <= reach) {
        plan.refusal = "too close to vertex " + std::to_string(toA <= toB ? a : b) +
                       "; zoom in to add a vertex this near it";
        return plan;
    }
    auto result = geo::insertVertex(shape, s, on.at, c.height);
    if (!result) {
        plan.refusal = result.error().message;
        return plan;
    }
    // Read from the RESULT: on an arc the vertex is the point put on the arc.
    const geo::CurveVertex& added = result->vertices[s + 1];
    f.marks.push_back(vertexMark(FeedbackRole::Added, added.position));
    // Numbered as the vertices on screen are now: "vertex 2 between 1 and
    // 2" mixed the new vertex's number after the insert with its neighbours'
    // before it. The message after the click gives the new number.
    f.caption = "new vertex between " + std::to_string(a) + " and " + std::to_string(b) + " · " +
                fixed(result->segmentLength(s)) + " from " + std::to_string(a);
    if (added.height) {
        f.caption += " · height " + fixed(*added.height);
    }
    plan.message = "vertex " + std::to_string(s + 1) + " added to polyline " +
                   std::to_string(on.polyline) + " on segment " + std::to_string(s) + " (" +
                   plural(result->vertices.size(), "vertex") + ")";
    plan.edits.push_back(Edit{on.polyline, shape, std::move(*result)});
    return plan;
}

Plan planDelete(const Collected& c, const ToolContext&)
{
    Plan plan;
    plan.command = "VERTEX_DELETE";
    ToolFeedback& f = plan.feedback;
    f.focus = c.picks.front().polyline;
    if (c.picks.size() > 1) {
        // Several hot vertices, on one polyline or more: each polyline's
        // together, all in one step.
        std::map<EntityId, std::pair<CurvePolyline2, std::vector<std::size_t>>> byPolyline;
        for (const Chosen& pick : c.picks) {
            auto& [shape, indices] = byPolyline[pick.polyline];
            if (indices.empty()) {
                shape = pick.shape;
            }
            indices.push_back(*pick.vertex);
            f.marks.push_back(
                vertexMark(FeedbackRole::Removed, pick.shape.vertices[*pick.vertex].position));
        }
        for (auto& [id, entry] : byPolyline) {
            auto result = geo::deleteVertices(entry.first, entry.second);
            if (!result) {
                plan.refusal = "polyline " + std::to_string(id) + ": " + result.error().message;
                return plan;
            }
            plan.edits.push_back(Edit{id, entry.first, std::move(*result)});
        }
        f.caption = "delete " + plural(c.picks.size(), "vertex");
        plan.message = plural(c.picks.size(), "vertex") + " deleted";
        return plan;
    }
    const Chosen& pick = c.picks.front();
    const CurvePolyline2& shape = pick.shape;
    const std::size_t v = *pick.vertex;
    auto result = geo::deleteVertex(shape, v);
    if (!result) {
        f.marks.push_back(
            vertexMark(FeedbackRole::Target, shape.vertices[v].position, std::to_string(v)));
        plan.refusal = result.error().message;
        return plan;
    }
    const auto before = segmentBefore(shape, v);
    const auto after = segmentAfter(shape, v);
    if (before) {
        f.marks.push_back(pieceMark(FeedbackRole::Removed, pieceOf(shape, *before)));
    }
    if (after) {
        f.marks.push_back(pieceMark(FeedbackRole::Removed, pieceOf(shape, *after)));
    }
    f.marks.push_back(vertexMark(FeedbackRole::Removed, shape.vertices[v].position));
    if (before && after) {
        // The segment that now joins the neighbours, read from the result:
        // it starts at the vertex before, which is the last one when the
        // vertex deleted was a closed polyline's first.
        const std::size_t joined = v == 0 ? result->vertices.size() - 1 : v - 1;
        f.shapes.push_back(pieceOf(*result, joined));
        f.caption = "delete vertex " + std::to_string(v) + " · segments " +
                    std::to_string(*before) + " and " + std::to_string(*after) + " become one";
    } else {
        f.caption = "delete vertex " + std::to_string(v) + " · segment " +
                    std::to_string(before ? *before : *after) + " goes";
    }
    plan.message = "vertex " + std::to_string(v) + " deleted";
    plan.edits.push_back(Edit{pick.polyline, shape, std::move(*result)});
    return plan;
}

Plan planMove(const Collected& c, const ToolContext&)
{
    const Chosen& pick = c.picks.front();
    const std::size_t v = *pick.vertex;
    const Point2 from = pick.shape.vertices[v].position;
    const Point2 to = c.points.front();
    Plan plan;
    plan.command = "VERTEX_MOVE";
    ToolFeedback& f = plan.feedback;
    f.focus = pick.polyline;
    f.marks.push_back(vertexMark(FeedbackRole::Target, from, std::to_string(v)));
    auto result = geo::moveVertex(pick.shape, v, to);
    if (result && c.height) {
        result = geo::setVertexHeight(*result, v, c.height);
    }
    if (!result) {
        plan.refusal = result.error().message;
        return plan;
    }
    f.marks.push_back(vertexMark(FeedbackRole::Added, result->vertices[v].position));
    // The segments either side as they will be: an arc keeps its bulge.
    if (const auto before = segmentBefore(*result, v)) {
        f.shapes.push_back(pieceOf(*result, *before));
    }
    if (const auto after = segmentAfter(*result, v)) {
        f.shapes.push_back(pieceOf(*result, *after));
    }
    const double distance = from.distanceTo(to);
    f.caption = "vertex " + std::to_string(v) + " · moves " + fixed(distance);
    if (distance > tol::kGeometric) {
        f.caption += " at " + formatBearing((to - from).angle());
    }
    if (c.height) {
        f.caption += " · height " + fixed(*c.height);
    }
    plan.message = "vertex " + std::to_string(v) + " moved";
    plan.edits.push_back(Edit{pick.polyline, pick.shape, std::move(*result)});
    return plan;
}

Plan planEdit(const Collected& c, const ToolContext&)
{
    const Chosen& pick = c.picks.front();
    Plan plan;
    ToolFeedback& f = plan.feedback;
    f.focus = pick.polyline;
    f.marks.push_back(pieceMark(FeedbackRole::Target, wholeOf(pick.shape)));
    f.caption = "polyline " + std::to_string(pick.polyline) + " · " +
                plural(pick.shape.vertices.size(), "vertex") + ", " +
                (pick.shape.closed ? "closed" : "open");
    plan.selection = std::vector<EntityId>{pick.polyline};
    plan.message = plural(pick.shape.vertices.size(), "vertex") + " in the Vertices panel";
    return plan;
}

// Straighten and Grade walk their range by cad::vertexRange, the rule the
// STRAIGHTEN and VERTEXZ GRADE verbs walk too (vertex_editing.hpp).

Plan planStraighten(const Collected& c, const ToolContext&)
{
    const Chosen& first = c.picks.at(0);
    const Chosen& second = c.picks.at(1);
    const CurvePolyline2& shape = first.shape;
    const std::size_t a = *first.vertex;
    const std::size_t b = *second.vertex;
    Plan plan;
    plan.command = "STRAIGHTEN";
    ToolFeedback& f = plan.feedback;
    f.focus = first.polyline;
    f.marks.push_back(vertexMark(FeedbackRole::Target, shape.vertices[a].position, "keep"));
    f.marks.push_back(vertexMark(FeedbackRole::Target, shape.vertices[b].position, "keep"));
    const auto [from, to] = vertexRange(shape, a, b, c.otherSide);
    auto result = geo::straighten(shape, from, to);
    if (!result) {
        plan.refusal = result.error().message;
        return plan;
    }
    const auto removed = geo::verticesBetween(shape, from, to);
    for (const std::size_t segment : segmentsBetween(shape, from, to)) {
        f.marks.push_back(pieceMark(FeedbackRole::Removed, pieceOf(shape, segment)));
    }
    for (const std::size_t vertex : removed) {
        f.marks.push_back(vertexMark(FeedbackRole::Removed, shape.vertices[vertex].position));
    }
    // The one straight segment left, from the result: it starts where
    // `from` now is, which is earlier by the vertices removed before it.
    const std::size_t shift = static_cast<std::size_t>(
        std::ranges::count_if(removed, [from](std::size_t i) { return i < from; }));
    f.shapes.push_back(pieceOf(*result, from - shift));
    f.caption = "straighten " + std::to_string(from) + " to " + std::to_string(to) +
                " · removes " + plural(removed.size(), "vertex");
    plan.message = plural(removed.size(), "vertex") + " removed";
    plan.edits.push_back(Edit{first.polyline, shape, std::move(*result)});
    return plan;
}

Plan planGrade(const Collected& c, const ToolContext&)
{
    const Chosen& first = c.picks.at(0);
    const Chosen& second = c.picks.at(1);
    const CurvePolyline2& shape = first.shape;
    const std::size_t a = *first.vertex;
    const std::size_t b = *second.vertex;
    Plan plan;
    plan.command = "GRADE";
    ToolFeedback& f = plan.feedback;
    f.focus = first.polyline;
    f.marks.push_back(vertexMark(FeedbackRole::Target, shape.vertices[a].position,
                                 heightLabel(shape.vertices[a].height)));
    f.marks.push_back(vertexMark(FeedbackRole::Target, shape.vertices[b].position,
                                 heightLabel(shape.vertices[b].height)));
    const auto [from, to] = vertexRange(shape, a, b, c.otherSide);
    auto result = geo::gradeBetween(shape, from, to);
    if (!result) {
        plan.refusal = result.error().message;
        return plan;
    }
    const auto between = geo::verticesBetween(shape, from, to);
    for (const std::size_t vertex : between) {
        f.marks.push_back(vertexMark(FeedbackRole::Target, shape.vertices[vertex].position,
                                     "→ " + fixed(*result->vertices[vertex].height)));
    }
    double length = 0.0;
    for (const std::size_t segment : segmentsBetween(shape, from, to)) {
        length += shape.segmentLength(segment);
    }
    const double za = *shape.vertices[a].height;
    const double zb = *shape.vertices[b].height;
    f.caption = "grade " + std::to_string(a) + " → " + std::to_string(b) + ": " +
                plural(between.size(), "vertex") + ", " + fixed(za) + " to " + fixed(zb);
    if (length > tol::kGeometric) {
        f.caption += " (" + fixed((zb - za) / length * 100.0) + " %)";
    }
    plan.message = "graded from vertex " + std::to_string(a) + " to " + std::to_string(b);
    plan.edits.push_back(Edit{first.polyline, shape, std::move(*result)});
    return plan;
}

Plan planStart(const Collected& c, const ToolContext&)
{
    const Chosen& pick = c.picks.front();
    const std::size_t v = *pick.vertex;
    Plan plan;
    plan.command = "START_VERTEX";
    ToolFeedback& f = plan.feedback;
    f.focus = pick.polyline;
    f.marks.push_back(vertexMark(FeedbackRole::Target, pick.shape.vertices[v].position, "new 0"));
    auto result = geo::changeStartVertex(pick.shape, v);
    if (!result) {
        plan.refusal = result.error().message;
        return plan;
    }
    f.caption = "vertex " + std::to_string(v) + " becomes vertex 0";
    plan.message = "vertex " + std::to_string(v) + " is now the start";
    plan.edits.push_back(Edit{pick.polyline, pick.shape, std::move(*result)});
    return plan;
}

Plan planHeight(const Collected& c, const ToolContext&)
{
    const Chosen& pick = c.picks.front();
    const std::size_t v = *pick.vertex;
    const std::optional<double> current = pick.shape.vertices[v].height;
    Plan plan;
    plan.command = "VERTEX_Z";
    ToolFeedback& f = plan.feedback;
    f.focus = pick.polyline;
    f.marks.push_back(
        vertexMark(FeedbackRole::Target, pick.shape.vertices[v].position, heightLabel(current)));
    f.caption = "vertex " + std::to_string(v) + " · " +
                (current ? "height " + fixed(*current) : std::string("no height"));
    const std::string& text = c.values.at(0);
    std::optional<double> height;
    if (!isOption(text, "None")) {
        height = katana::core::parseFiniteDouble(text);
        if (!height) {
            plan.warning = "'" + text + "' is not a height; type a number or None";
            return plan;
        }
    }
    if (height == current) {
        // Enter at a vertex's own height: nothing to do, and no undo step.
        plan.noChange = "vertex " + std::to_string(v) + " keeps " +
                        (current ? "its height " + number(*current) : std::string("no height"));
        return plan;
    }
    auto result = geo::setVertexHeight(pick.shape, v, height);
    if (!result) {
        plan.warning = result.error().message;
        return plan;
    }
    plan.message = "vertex " + std::to_string(v) +
                   (height ? " at " + number(*height) : std::string(" has no height"));
    plan.edits.push_back(Edit{pick.polyline, pick.shape, std::move(*result)});
    return plan;
}

Plan planSegmentArc(const Collected& c, const ToolContext&)
{
    const Chosen& pick = c.picks.front();
    const std::size_t s = *pick.segment;
    Plan plan;
    plan.command = "SEGMENT_ARC";
    ToolFeedback& f = plan.feedback;
    f.focus = pick.polyline;
    f.marks.push_back(pieceMark(FeedbackRole::Target, pieceOf(pick.shape, s)));
    auto result = geo::segmentToArc(pick.shape, s, c.points.front());
    if (!result) {
        plan.refusal = result.error().message;
        return plan;
    }
    f.shapes.push_back(pieceOf(*result, s));
    const auto arc = geo::arcFromBulge(result->vertices[s].position,
                                       result->vertices[result->segmentEnd(s)].position,
                                       result->vertices[s].bulge);
    f.caption = "segment " + std::to_string(s) + (arc ? " · radius " + fixed(arc->radius) : "");
    plan.message = "segment " + std::to_string(s) + " is an arc";
    plan.edits.push_back(Edit{pick.polyline, pick.shape, std::move(*result)});
    return plan;
}

Plan planSegmentLine(const Collected& c, const ToolContext&)
{
    const Chosen& pick = c.picks.front();
    const std::size_t s = *pick.segment;
    Plan plan;
    plan.command = "SEGMENT_LINE";
    ToolFeedback& f = plan.feedback;
    f.focus = pick.polyline;
    f.marks.push_back(pieceMark(FeedbackRole::Target, pieceOf(pick.shape, s)));
    auto result = geo::segmentToLine(pick.shape, s);
    if (!result) {
        plan.refusal = result.error().message;
        return plan;
    }
    f.shapes.push_back(pieceOf(*result, s));
    f.caption = "segment " + std::to_string(s) + " becomes straight";
    plan.message = "segment " + std::to_string(s) + " is straight";
    plan.edits.push_back(Edit{pick.polyline, pick.shape, std::move(*result)});
    return plan;
}

// The piece of `result` that replaced corner `v` of `shape`: it starts at
// the vertex before the corner when the cut reached that vertex, else where
// the corner was (polyline_vertices.cpp, replaceCorner, which drops a cut
// point that lands on a neighbour rather than doubling it).
std::size_t cornerPiece(const CurvePolyline2& shape, const CurvePolyline2& result, std::size_t v,
                        bool arc)
{
    const std::size_t atCorner = v;
    const std::size_t beforeCorner = v > 0 ? v - 1 : result.vertices.size() - 1;
    if (arc) {
        return atCorner < result.segmentCount() && result.isArc(atCorner) ? atCorner
                                                                           : beforeCorner;
    }
    // A bevel: both cut points new (one more vertex than before), or the
    // first one new (a vertex on the incoming segment where the corner was).
    const std::size_t replacement = result.vertices.size() + 1 - shape.vertices.size();
    if (replacement == 2) {
        return atCorner;
    }
    if (replacement == 1) {
        const std::size_t previous = segmentBefore(shape, v).value_or(0);
        const Segment2 incoming{shape.vertices[previous].position, shape.vertices[v].position};
        return incoming.distanceTo(result.vertices[atCorner].position) <= tol::kGeometric
                   ? atCorner
                   : beforeCorner;
    }
    return beforeCorner;
}

Plan planFillet(const Collected& c, const ToolContext&)
{
    const Chosen& pick = c.picks.front();
    const std::size_t v = *pick.vertex;
    const CurvePolyline2& shape = pick.shape;
    Plan plan;
    plan.command = "FILLET_VERTEX";
    ToolFeedback& f = plan.feedback;
    f.focus = pick.polyline;
    f.marks.push_back(
        vertexMark(FeedbackRole::Target, shape.vertices[v].position, std::to_string(v)));
    if (auto corner = geo::checkCorner(shape, v); !corner) {
        plan.refusal = corner.error().message;
        return plan;
    }
    const std::string& text = c.values.at(0);
    const auto radius = parsePositive(text);
    if (!radius) {
        plan.warning = "'" + text + "' is not a radius greater than zero";
        return plan;
    }
    auto result = geo::filletVertex(shape, v, *radius);
    if (!result) {
        plan.warning = result.error().message;
        return plan;
    }
    const std::size_t piece = cornerPiece(shape, *result, v, true);
    f.marks.push_back(vertexMark(FeedbackRole::Removed, shape.vertices[v].position));
    f.marks.push_back(vertexMark(FeedbackRole::Added, result->vertices[piece].position));
    f.marks.push_back(
        vertexMark(FeedbackRole::Added, result->vertices[result->segmentEnd(piece)].position));
    f.shapes.push_back(pieceOf(*result, piece));
    f.caption = "fillet vertex " + std::to_string(v) + " · radius " + fixed(*radius);
    plan.message = "vertex " + std::to_string(v) + " filleted at " + number(*radius);
    plan.edits.push_back(Edit{pick.polyline, shape, std::move(*result)});
    return plan;
}

Plan planChamfer(const Collected& c, const ToolContext&)
{
    const Chosen& pick = c.picks.front();
    const std::size_t v = *pick.vertex;
    const CurvePolyline2& shape = pick.shape;
    Plan plan;
    plan.command = "CHAMFER_VERTEX";
    ToolFeedback& f = plan.feedback;
    f.focus = pick.polyline;
    const auto incoming = segmentBefore(shape, v);
    const auto outgoing = segmentAfter(shape, v);
    // "d1" and "d2", the distances measured along each: a bare "1" and "2"
    // on the segments read as segment numbers beside the vertex numbers.
    if (incoming) {
        f.marks.push_back(pieceMark(FeedbackRole::Target, pieceOf(shape, *incoming), "d1"));
    }
    if (outgoing) {
        f.marks.push_back(pieceMark(FeedbackRole::Target, pieceOf(shape, *outgoing), "d2"));
    }
    if (auto corner = geo::checkCorner(shape, v); !corner) {
        plan.refusal = corner.error().message;
        return plan;
    }
    const auto first = parsePositive(c.values.at(0));
    const auto second = parsePositive(c.values.at(1));
    if (!first || !second) {
        plan.warning = "type two distances greater than zero";
        return plan;
    }
    auto result = geo::chamferVertex(shape, v, *first, *second);
    if (!result) {
        plan.warning = result.error().message;
        return plan;
    }
    const std::size_t piece = cornerPiece(shape, *result, v, false);
    f.marks.push_back(vertexMark(FeedbackRole::Removed, shape.vertices[v].position));
    f.marks.push_back(vertexMark(FeedbackRole::Added, result->vertices[piece].position));
    f.marks.push_back(
        vertexMark(FeedbackRole::Added, result->vertices[result->segmentEnd(piece)].position));
    f.shapes.push_back(pieceOf(*result, piece));
    f.caption = "chamfer vertex " + std::to_string(v) + " · " + fixed(*first) + " and " +
                fixed(*second);
    plan.message = "vertex " + std::to_string(v) + " chamfered";
    plan.edits.push_back(Edit{pick.polyline, shape, std::move(*result)});
    return plan;
}

// ---- registration -------------------------------------------------------------------------

ToolInfo info(std::string id, std::string name, int order, std::vector<std::string> aliases,
              std::string tip, Script script)
{
    ToolInfo tool;
    tool.id = std::move(id);
    // The prompt names the tool by what it does; the menus gather the family
    // by its name's "Vertices, " (tool_menus.hpp).
    tool.title = name;
    tool.name = "Vertices, " + std::move(name);
    tool.category = "Draw";
    tool.group = "Vertices";
    tool.order = order;
    tool.aliases = std::move(aliases);
    tool.tip = std::move(tip);
    tool.make = [script = std::move(script)](const ToolContext& context)
        -> std::unique_ptr<InteractiveTool> { return std::make_unique<ScriptTool>(context, script); };
    return tool;
}

// A selection tool's script: select, then the values, then `finish`.
Script selectionScript(std::vector<Step> steps, std::function<ToolStep(const Collected&)> finish)
{
    Script script;
    script.steps = std::move(steps);
    script.finish = std::move(finish);
    return script;
}

} // namespace

void addModifyVertexTools(ToolCatalog& catalog, const Report& report)
{
    VertexDefaults& kept = defaults();

    {
        Script script;
        Step on;
        on.kind = Step::Kind::OnLine;
        on.prompt = [](const Collected& c, std::optional<EntityId> inPlay) -> std::string {
            if (c.anchor && c.anchor->vertex) {
                const CurvePolyline2& shape = c.anchor->shape;
                const std::size_t v = *c.anchor->vertex;
                const auto after = segmentAfter(shape, v);
                const std::size_t middle = after ? *after : segmentBefore(shape, v).value_or(0);
                return "Click beside vertex " + std::to_string(v) + " of polyline " +
                       std::to_string(c.anchor->polyline) +
                       " where the new vertex goes, or press Enter for the marked middle of "
                       "segment " +
                       std::to_string(middle);
            }
            if (c.anchor && c.anchor->segment) {
                return "Click on segment " + std::to_string(*c.anchor->segment) + " of polyline " +
                       std::to_string(c.anchor->polyline) +
                       " where the new vertex goes, or press Enter for its marked middle";
            }
            return inPlay ? "Click on polyline " + std::to_string(*inPlay) +
                                " where the new vertex goes"
                          : std::string("Click on a polyline where the new vertex goes");
        };
        script.steps = {on};
        script.plan = planInsert;
        script.answer = [](const ToolContext& context, Collected& c) -> std::size_t {
            // Nothing is answered: the handle says WHERE (beside a hot vertex,
            // on a hot segment) and the click or Enter says the point.
            auto segments = handlesOf(context, GripKind::SegmentMid);
            auto hot = handlesOf(context, GripKind::Vertex);
            if (segments.size() == 1 && hot.empty()) {
                c.anchor = std::move(segments.front());
            } else if (segments.empty() && hot.size() == 1) {
                c.anchor = std::move(hot.front());
            } else if (segments.empty() && hot.size() == 2) {
                c.anchor = segmentBetween(hot[0], hot[1]);
                if (!c.anchor) {
                    c.note = noSegmentBetween(hot[0], hot[1]);
                }
            } else if (segments.size() + hot.size() > 1) {
                c.note = std::to_string(segments.size() + hot.size()) +
                         " grips are chosen; a new vertex goes beside one vertex, or on one "
                         "segment";
            }
            return 0;
        };
        script.restarts = true;
        report(catalog.add(info(
            "draw.vertex.insert", "Insert Vertex", 10, {"INSERTVERTEX"},
            "Adds a vertex where you click on a polyline - always on the line, on an arc on the "
            "arc - shown before the click; with a vertex chosen first, beside it.",
            std::move(script))));
    }

    {
        Script script;
        script.steps = {vertexStep(clickThe("vertex", "to delete"))};
        script.plan = planDelete;
        script.answer = [](const ToolContext& context, Collected& c) -> std::size_t {
            auto hot = handlesOf(context, GripKind::Vertex);
            if (hot.empty()) {
                return 0;
            }
            c.picks = std::move(hot);
            return 1;
        };
        script.ready = [](const Collected& c) {
            if (c.picks.size() == 1) {
                return "Press Enter to delete " + pickedVertex(c) + ", or click another vertex";
            }
            return "Press Enter to delete the " + std::to_string(c.picks.size()) +
                   " chosen vertices, or click another vertex";
        };
        script.restarts = true;
        report(catalog.add(info(
            "draw.vertex.delete", "Delete Vertex", 20, {"DELETEVERTEX"},
            "Removes the vertex you click, or the vertices chosen before; its two segments "
            "become one.",
            std::move(script))));
    }

    {
        Script script;
        script.steps = {vertexStep(clickThe("vertex", "to move")),
                        pointStep("Specify its new position")};
        script.plan = planMove;
        script.answer = [](const ToolContext& context, Collected& c) -> std::size_t {
            auto hot = handlesOf(context, GripKind::Vertex);
            if (hot.size() == 1) {
                c.picks = std::move(hot);
                return 1;
            }
            if (hot.size() > 1) {
                c.note = std::to_string(hot.size()) + " vertices are chosen, and this tool moves one";
            }
            return 0;
        };
        script.restarts = true;
        report(catalog.add(info(
            "draw.vertex.move", "Move Vertex", 30, {"MOVEVERTEX"},
            "Moves the vertex you click, or the one chosen before, to a picked or typed point; "
            "@dx,dy is from the vertex, and @dx,dy,dz raises it by dz.",
            std::move(script))));
    }

    {
        Script script;
        Step whole;
        whole.kind = Step::Kind::Polyline;
        whole.prompt = fixedPrompt("Click the polyline whose vertices to edit");
        script.steps = {whole};
        script.plan = planEdit;
        script.answer = [](const ToolContext& context, Collected& c) -> std::size_t {
            // A grip chosen, else the one polyline selected: it is in the
            // panel already, so the tool does not ask for it again.
            EntityId id = kInvalidEntityId;
            for (const Grip& grip : context.handles) {
                if (polylineNow(context.document, grip.entity)) {
                    id = grip.entity;
                    break;
                }
            }
            if (id == kInvalidEntityId) {
                if (const auto own = polylinesAmong(context.document, context.selection);
                    own.size() == 1) {
                    id = own.front();
                }
            }
            const auto shape = polylineNow(context.document, id);
            if (!shape) {
                return 0;
            }
            Chosen chosen;
            chosen.polyline = id;
            chosen.shape = *shape;
            chosen.at = shape->vertices.front().position;
            chosen.fromHandle = true;
            c.picks = {std::move(chosen)};
            return 1;
        };
        script.ready = [](const Collected& c) {
            return "Polyline " + std::to_string(c.picks.front().polyline) +
                   " is in the Vertices panel; click another polyline, or press Enter";
        };
        report(catalog.add(info(
            "draw.vertex.edit", "Edit Vertices", 40, {"EDITVERTICES", "VERTEX"},
            "Shows a polyline's vertices in the Vertices panel, to edit in place: the one "
            "selected, or the one you click.",
            std::move(script))));
    }

    {
        Script script;
        Step first = vertexStep(clickThe("first vertex", "to keep"));
        first.label = [](const Chosen&) { return std::string("keep"); };
        Step second = vertexStep(fixedPrompt("Click the other vertex to keep"));
        second.samePolyline = true;
        second.otherSide = true;
        script.steps = {first, second};
        script.plan = planStraighten;
        script.answer = twoVertices;
        script.ready = [](const Collected& c) {
            return "Press Enter to straighten polyline " + std::to_string(c.picks[0].polyline) +
                   " from vertex " + std::to_string(*c.picks[0].vertex) + " to " +
                   std::to_string(*c.picks[1].vertex) + ", or click another vertex" +
                   (c.picks[0].shape.closed ? " [Other side]" : "");
        };
        script.restarts = true;
        report(catalog.add(info(
            "draw.vertex.straighten", "Straighten", 50, {"STRAIGHTEN"},
            "Removes every vertex between two you click, leaving one straight segment between "
            "them; on a closed polyline the side with fewer, or the other side with O.",
            std::move(script))));
    }

    report(catalog.add(info(
        "draw.vertex.weed", "Weed", 60, {"WEED", "SIMPLIFY"},
        "Removes vertices that lie within a tolerance of the line through their neighbours "
        "(Douglas-Peucker), in plan and height, optionally keeping vertices on survey points.",
        selectionScript(
            {selectPolylines("weed"),
             valueStep(fixedPrompt("Specify the weed tolerance"), remembered(kept.weedTolerance)),
             valueStep(fixedPrompt("Keep vertices on survey points? [Yes/No]"),
                       [](const Collected&) -> std::optional<std::string> {
                           return defaults().weedKeepsSurveyPoints ? "Yes" : "No";
                       })},
            [](const Collected& c) {
                const auto tolerance = parseNonNegative(c.values[0]);
                if (!tolerance) {
                    return ToolStep::rejected("'" + c.values[0] +
                                              "' is not a tolerance; type a distance of zero or "
                                              "more");
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
                                         return geo::weed(
                                             polyline, t,
                                             keep ? verticesOnSurveyPoints(model, polyline)
                                                  : std::vector<bool>{});
                                     }),
                    plural(c.selection.size(), "polyline") + " weeded to " + number(t));
            }))));

    report(catalog.add(info(
        "draw.vertex.densify", "Densify", 70, {"DENSIFY"},
        "Adds vertices so no straight segment is longer than an interval, and optionally turns "
        "arcs into chords within a tolerance.",
        selectionScript(
            {selectPolylines("densify"),
             valueStep(fixedPrompt("Specify the interval, or 0 to leave straight segments"),
                       remembered(kept.densifyInterval)),
             valueStep(fixedPrompt("Specify the chord tolerance for arcs, or 0 to keep them as "
                                   "arcs"),
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
            }))));

    report(catalog.add(info(
        "draw.vertex.close", "Close or Open", 80, {"CLOSEOPEN"},
        "Closes each open polyline selected and opens each closed one.",
        selectionScript({selectPolylines("close or open")}, [](const Collected& c) {
            return ToolStep::done(editPolylines(c.selection, "CLOSE_OPEN",
                                                [](const CurvePolyline2& polyline) {
                                                    return polyline.closed
                                                               ? geo::openPolyline(polyline)
                                                               : geo::closePolyline(polyline);
                                                }),
                                  plural(c.selection.size(), "polyline") + " closed or opened");
        }))));

    {
        Script script;
        script.steps = {vertexStep([](const Collected&, std::optional<EntityId> inPlay) {
            return inPlay ? "Click the vertex of polyline " + std::to_string(*inPlay) +
                                " to be its start"
                          : std::string("Click the vertex to be the start of a closed polyline");
        })};
        script.steps.front().label = [](const Chosen&) { return std::string("new 0"); };
        script.plan = planStart;
        script.answer = oneVertex;
        script.ready = [](const Collected& c) {
            return "Press Enter to make vertex " + std::to_string(*c.picks.front().vertex) +
                   " the start of polyline " + std::to_string(c.picks.front().polyline) +
                   ", or click another vertex";
        };
        script.restarts = true;
        report(catalog.add(info(
            "draw.vertex.start", "Change Start Vertex", 90, {"STARTVERTEX"},
            "Makes the vertex you click the first of a closed polyline; its shape is unchanged.",
            std::move(script))));
    }

    {
        Script script;
        Step vertex = vertexStep(clickThe("vertex", "whose height to set"));
        vertex.label = [](const Chosen& chosen) {
            return heightLabel(chosen.shape.vertices[*chosen.vertex].height);
        };
        script.steps = {
            vertex,
            valueStep(
                [](const Collected& c, std::optional<EntityId>) {
                    return "Specify the height of vertex " +
                           std::to_string(*c.picks.front().vertex) + " or [None]";
                },
                [](const Collected& c) -> std::optional<std::string> {
                    // Its own height, so Enter changes nothing; else the
                    // height interpolated along the polyline; else none.
                    if (c.picks.empty()) {
                        return std::nullopt;
                    }
                    const Chosen& pick = c.picks.front();
                    const auto& height = pick.shape.vertices[*pick.vertex].height;
                    if (height) {
                        return number(*height);
                    }
                    if (const auto filled = geo::interpolateHeights(pick.shape);
                        filled && filled->vertices[*pick.vertex].height) {
                        return number(*filled->vertices[*pick.vertex].height);
                    }
                    return std::string("None");
                })};
        script.plan = planHeight;
        script.answer = oneVertex;
        script.valueFromClick = [](const Collected& c, const ToolContext& context,
                                   const Point2& at) -> std::optional<std::string> {
            if (c.document == nullptr) {
                return std::nullopt;
            }
            // Only what the view draws: a hidden point's height is not one
            // the user can see to click.
            const auto height =
                heightAtPoint(*c.document, at, vertexReach(context), viewOf(context));
            return height ? std::optional<std::string>(number(*height)) : std::nullopt;
        };
        script.restarts = true;
        report(catalog.add(info(
            "draw.vertex.height", "Set Vertex Height", 100, {"VERTEXZ"},
            "Sets the height of the vertex you click - typed, or clicked from a point that has "
            "one - or clears it with None.",
            std::move(script))));
    }

    report(catalog.add(info(
        "draw.vertex.interpolate", "Interpolate Heights", 110, {"INTERPOLATEZ"},
        "Gives every vertex without a height the height interpolated by length between the "
        "vertices either side that have one.",
        selectionScript({selectPolylines("interpolate the heights of")}, [](const Collected& c) {
            return ToolStep::done(editPolylines(c.selection, "INTERPOLATE_Z",
                                                [](const CurvePolyline2& polyline) {
                                                    return geo::interpolateHeights(polyline);
                                                }),
                                  plural(c.selection.size(), "polyline") + " interpolated");
        }))));

    {
        Script script;
        const auto hasHeight = [](const Chosen& chosen) -> std::optional<std::string> {
            const std::size_t v = *chosen.vertex;
            if (chosen.shape.vertices[v].height) {
                return std::nullopt;
            }
            return "vertex " + std::to_string(v) +
                   " has no height; grading needs one at both ends";
        };
        Step first = vertexStep(clickThe("vertex", "to grade from"));
        first.check = hasHeight;
        first.label = [](const Chosen& chosen) {
            return heightLabel(chosen.shape.vertices[*chosen.vertex].height);
        };
        Step second = vertexStep(fixedPrompt("Click the vertex to grade to"));
        second.samePolyline = true;
        second.otherSide = true;
        second.check = hasHeight;
        script.steps = {first, second};
        script.plan = planGrade;
        script.answer = twoVertices;
        script.ready = [](const Collected& c) {
            return "Press Enter to grade polyline " + std::to_string(c.picks[0].polyline) +
                   " from vertex " + std::to_string(*c.picks[0].vertex) + " to " +
                   std::to_string(*c.picks[1].vertex) + ", or click another vertex" +
                   (c.picks[0].shape.closed ? " [Other side]" : "");
        };
        script.restarts = true;
        report(catalog.add(info(
            "draw.vertex.grade", "Grade Between Vertices", 120, {"GRADE"},
            "Sets the heights of the vertices between two you click on the straight grade "
            "between their heights.",
            std::move(script))));
    }

    {
        Script script;
        script.steps = {segmentStep(clickThe("segment", "to make an arc")),
                        pointStep("Specify a point the arc passes through")};
        script.plan = planSegmentArc;
        script.answer = oneSegment;
        script.restarts = true;
        report(catalog.add(info("draw.vertex.arc", "Segment to Arc", 130, {"SEGMENTARC"},
                                "Makes the segment you click an arc through a point you give.",
                                std::move(script))));
    }

    {
        Script script;
        script.steps = {segmentStep(clickThe("arc segment", "to straighten"))};
        script.plan = planSegmentLine;
        script.answer = oneSegment;
        script.ready = [](const Collected& c) {
            return "Press Enter to straighten segment " +
                   std::to_string(*c.picks.front().segment) + " of polyline " +
                   std::to_string(c.picks.front().polyline) + ", or click another segment";
        };
        script.restarts = true;
        report(catalog.add(info("draw.vertex.line", "Segment to Line", 140, {"SEGMENTLINE"},
                                "Makes the arc segment you click straight.", std::move(script))));
    }

    {
        Script script;
        script.steps = {vertexStep(clickThe("corner vertex", "to round")),
                        valueStep(
                            [](const Collected& c, std::optional<EntityId>) {
                                return "Specify the fillet radius at vertex " +
                                       std::to_string(*c.picks.front().vertex);
                            },
                            remembered(kept.filletRadius))};
        script.plan = planFillet;
        script.answer = oneVertex;
        script.remember = [](const Collected& c) {
            if (const auto radius = parsePositive(c.values.at(0))) {
                defaults().filletRadius = *radius;
            }
        };
        script.restarts = true;
        report(catalog.add(info(
            "draw.vertex.fillet", "Fillet Vertex", 150, {"FILLETVERTEX"},
            "Rounds the corner at the vertex you click with a tangent arc of a radius, shown "
            "at the last radius before you click.",
            std::move(script))));
    }

    {
        Script script;
        const auto along = [](std::string which, bool incoming) {
            return [which = std::move(which), incoming](const Collected& c,
                                                        std::optional<EntityId>) {
                const Chosen& pick = c.picks.front();
                const auto segment = incoming ? segmentBefore(pick.shape, *pick.vertex)
                                              : segmentAfter(pick.shape, *pick.vertex);
                return "Specify the " + which + " distance, along segment " +
                       std::to_string(segment.value_or(0));
            };
        };
        script.steps = {vertexStep(clickThe("corner vertex", "to cut")),
                        valueStep(along("first", true), remembered(kept.chamferFirst)),
                        valueStep(along("second", false), remembered(kept.chamferSecond))};
        script.plan = planChamfer;
        script.answer = oneVertex;
        script.remember = [](const Collected& c) {
            const auto first = parsePositive(c.values.at(0));
            const auto second = parsePositive(c.values.at(1));
            if (first && second) {
                defaults().chamferFirst = *first;
                defaults().chamferSecond = *second;
            }
        };
        script.restarts = true;
        report(catalog.add(info(
            "draw.vertex.chamfer", "Chamfer Vertex", 160, {"CHAMFERVERTEX"},
            "Cuts the corner at the vertex you click with a straight bevel: the first distance "
            "back along the segment marked d1, the second along the one marked d2.",
            std::move(script))));
    }

    report(catalog.add(info(
        "draw.vertex.merge", "Merge Near Vertices", 170, {"MERGEVERTICES"},
        "Merges consecutive vertices closer together than a tolerance into the first of them.",
        selectionScript(
            {selectPolylines("merge the near vertices of"),
             valueStep(fixedPrompt("Specify the tolerance"), remembered(kept.mergeTolerance))},
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
            }))));

    report(catalog.add(info(
        "draw.vertex.grid", "Snap Vertices to Grid", 180, {"SNAPVERTICES"},
        "Moves every vertex of the selected polylines to the nearest node of a grid.",
        selectionScript(
            {selectPolylines("snap to the grid"),
             valueStep(fixedPrompt("Specify the grid spacing"), remembered(kept.gridSpacing))},
            [](const Collected& c) {
                const auto spacing = parsePositive(c.values[0]);
                if (!spacing) {
                    return ToolStep::rejected("'" + c.values[0] +
                                              "' is not a spacing greater than zero");
                }
                defaults().gridSpacing = *spacing;
                const double s = *spacing;
                return ToolStep::done(editPolylines(c.selection, "SNAP_VERTICES",
                                                    [s](const CurvePolyline2& polyline) {
                                                        return geo::snapToGrid(polyline, s);
                                                    }),
                                      plural(c.selection.size(), "polyline") + " snapped to " +
                                          number(s));
            }))));
}

} // namespace katana::cad::tools
