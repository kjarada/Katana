// Join and Explode, the two halves of one idea: Join chains lines and open
// polylines that meet end to end into one polyline, Explode takes a polyline
// (a rectangle is one) apart into its lines. Each acts on a selection - the
// one the tool was started with, or one made when it asks - and is ONE
// command.
//
// Join carries LINES only. A Katana polyline (geometry::Polyline2) has
// straight segments and nothing else - no bulges - so an arc cannot become
// part of one without being replaced by chords, which would change the
// drawing rather than join it. Arcs in the selection are left as they are and
// the tool says so.

#include <algorithm>
#include <cmath>
#include <limits>

#include "katana/entity/entity_geometry.hpp"
#include "modify_edit_support.hpp"

namespace katana::cad::tools::modify_edit {

namespace {

namespace tol = katana::math::tolerance;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

using Heights = std::vector<std::optional<double>>;

// One line or open polyline, as a run of vertices that can be turned round.
struct Run {
    EntityId id = katana::entity::kInvalidEntityId;
    std::vector<Point2> vertices;
    Heights heights;

    void reverse()
    {
        std::ranges::reverse(vertices);
        std::ranges::reverse(heights);
    }
};

struct Chain {
    std::vector<EntityId> ids; // the runs joined, the first is the one kept
    Polyline2 polyline;
    Heights heights;
};

struct JoinPlan {
    std::vector<Chain> chains; // two or more runs each
    std::size_t arcs = 0;      // left out: a polyline cannot carry them
    std::size_t other = 0;     // left out: circles, closed polylines, points, text
    std::size_t locked = 0;    // left out: on a locked layer
    std::size_t runs = 0;      // lines and open polylines considered
};

std::string plural(std::size_t count, const std::string& one, const std::string& many)
{
    return std::to_string(count) + " " + (count == 1 ? one : many);
}

// Appends `run` to the end of the chain (its first vertex is the chain's
// joint), or puts it before the start (its last vertex is the joint). The
// vertex already in the chain is the one kept, so the first run's geometry
// never moves and a later run's end moves by at most the tolerance; its height
// is kept too, unless it has none and the run's has.
void attach(Chain& chain, Run run, bool atEnd)
{
    auto& v = chain.polyline.vertices;
    auto& h = chain.heights;
    if (atEnd) {
        if (!h.back()) {
            h.back() = run.heights.front();
        }
        v.insert(v.end(), run.vertices.begin() + 1, run.vertices.end());
        h.insert(h.end(), run.heights.begin() + 1, run.heights.end());
    } else {
        if (!h.front()) {
            h.front() = run.heights.back();
        }
        v.insert(v.begin(), run.vertices.begin(), run.vertices.end() - 1);
        h.insert(h.begin(), run.heights.begin(), run.heights.end() - 1);
    }
    chain.ids.push_back(run.id);
}

JoinPlan planJoin(const Document& document, const std::vector<EntityId>& ids, double tolerance)
{
    JoinPlan plan;
    std::vector<Run> runs;
    for (const EntityId id : ids) {
        const Entity* entity = document.model().entities.find(id);
        if (entity == nullptr) {
            continue;
        }
        const Geometry& geometry = entity->geometry;
        const auto* segment = std::get_if<Segment2>(&geometry);
        const auto* polyline = std::get_if<Polyline2>(&geometry);
        if (std::holds_alternative<Arc2>(geometry)) {
            ++plan.arcs;
            continue;
        }
        if (segment == nullptr && (polyline == nullptr || polyline->closed)) {
            ++plan.other;
            continue;
        }
        if (refusalToEdit(document, id)) {
            ++plan.locked;
            continue;
        }
        Run run;
        run.id = id;
        run.vertices = segment != nullptr ? std::vector<Point2>{segment->start, segment->end}
                                          : polyline->vertices;
        run.heights = katana::entity::heightsOf(entity->properties, run.vertices.size());
        runs.push_back(std::move(run));
    }
    plan.runs = runs.size();

    // Chains grow from the lowest id, first at their end and then at their
    // start, taking the nearest free end within the tolerance each time (the
    // lower id on a tie), so the result is the same on every run.
    std::vector<bool> used(runs.size(), false);
    for (std::size_t seed = 0; seed < runs.size(); ++seed) {
        if (used[seed]) {
            continue;
        }
        used[seed] = true;
        Chain chain;
        chain.ids = {runs[seed].id};
        chain.polyline.vertices = runs[seed].vertices;
        chain.heights = runs[seed].heights;
        for (const bool atEnd : {true, false}) {
            while (true) {
                const Point2 joint =
                    atEnd ? chain.polyline.vertices.back() : chain.polyline.vertices.front();
                std::optional<std::size_t> best;
                bool bestReversed = false;
                double bestDistance = std::numeric_limits<double>::infinity();
                for (std::size_t i = 0; i < runs.size(); ++i) {
                    if (used[i]) {
                        continue;
                    }
                    // At the end the run must start at the joint; at the
                    // start it must finish there. Either way round will do.
                    const Point2& meets = atEnd ? runs[i].vertices.front() : runs[i].vertices.back();
                    const Point2& turned = atEnd ? runs[i].vertices.back() : runs[i].vertices.front();
                    for (const auto& [end, reversed] :
                         {std::pair{meets, false}, std::pair{turned, true}}) {
                        const double d = end.distanceTo(joint);
                        if (d <= tolerance && d < bestDistance) {
                            bestDistance = d;
                            best = i;
                            bestReversed = reversed;
                        }
                    }
                }
                if (!best) {
                    break;
                }
                used[*best] = true;
                Run run = runs[*best];
                if (bestReversed) {
                    run.reverse();
                }
                attach(chain, std::move(run), atEnd);
            }
        }
        if (chain.ids.size() < 2) {
            continue;
        }
        // A chain whose two ends meet is a closed polyline - when it has the
        // three corners a closed shape needs.
        auto& v = chain.polyline.vertices;
        if (v.size() >= 4 && v.front().distanceTo(v.back()) <= tolerance) {
            if (!chain.heights.front()) {
                chain.heights.front() = chain.heights.back();
            }
            v.pop_back();
            chain.heights.pop_back();
            chain.polyline.closed = true;
        }
        plan.chains.push_back(std::move(chain));
    }
    return plan;
}

// What was left out, as a sentence to add to the report, or nothing.
std::string leftOut(const JoinPlan& plan, std::size_t lone)
{
    std::string out;
    if (plan.arcs > 0) {
        out += " " + plural(plan.arcs, "arc was", "arcs were") +
               " left out: a polyline has straight segments only.";
    }
    if (plan.locked > 0) {
        out += " " + plural(plan.locked, "object on a locked layer was",
                            "objects on locked layers were") +
               " left out.";
    }
    if (plan.other > 0) {
        out += " " + plural(plan.other, "object that is not a line or open polyline was",
                            "objects that are not lines or open polylines were") +
               " left out.";
    }
    if (lone > 0) {
        out += " " + plural(lone, "line or polyline met nothing and was",
                            "lines or polylines met nothing and were") +
               " left as it was.";
    }
    return out;
}

// ---- Join --------------------------------------------------------------------------

class JoinTool final : public InteractiveTool {
  public:
    JoinTool(const ToolContext& context, Defaults defaults)
        : context_(context), defaults_(std::move(defaults)), session_(context.document)
    {
    }

    [[nodiscard]] std::string prompt() const override
    {
        if (step_ == Step::Tolerance) {
            return "Specify how far apart two ends may be and still join <" +
                   formatNumber(defaults_->joinTolerance) + ">";
        }
        const std::size_t count = selectionNow(context_, picked_).size();
        return "Select lines and polylines to join or [Tolerance] (" +
               plural(count, "selected", "selected") + ", ends within " +
               formatNumber(defaults_->joinTolerance) + ")";
    }

    [[nodiscard]] ToolInput expects() const override
    {
        return step_ == Step::Tolerance ? ToolInput::Value : ToolInput::Selection;
    }

    ToolStep entity(EntityId id, const Point2& at) override
    {
        if (step_ != Step::Select) {
            return InteractiveTool::entity(id, at);
        }
        if (std::ranges::find(picked_, id) == picked_.end()) {
            picked_.push_back(id);
        }
        return ToolStep::next();
    }

    ToolStep value(std::string_view text) override
    {
        if (step_ == Step::Tolerance) {
            const auto tolerance = parseNumber(text);
            if (!tolerance) {
                return ToolStep::rejected("Type the tolerance as a distance.");
            }
            if (*tolerance < 0.0) {
                return ToolStep::rejected("The tolerance cannot be negative.");
            }
            defaults_->joinTolerance = *tolerance;
            step_ = Step::Select;
            return ToolStep::next();
        }
        if (isOption(text, "Tolerance", "T")) {
            step_ = Step::Tolerance;
            return ToolStep::next();
        }
        if (isOption(text, "Undo", "U")) {
            return undo();
        }
        return ToolStep::rejected("'" + std::string(text) + "' is not an option here.");
    }

    ToolStep enter() override
    {
        if (step_ == Step::Tolerance) {
            step_ = Step::Select;
            return ToolStep::next();
        }
        const auto ids = selectionNow(context_, picked_);
        const JoinPlan plan = planJoin(session_.document(), ids, tolerance());
        if (plan.chains.empty()) {
            if (plan.runs < 2) {
                return ToolStep::rejected(
                    "Select at least two lines or open polylines to join." + leftOut(plan, 0));
            }
            return ToolStep::rejected("None of the selected lines and polylines meet end to end "
                                      "within " +
                                      formatNumber(tolerance()) + "." + leftOut(plan, 0));
        }
        std::size_t joined = 0;
        session_.begin();
        for (const Chain& chain : plan.chains) {
            // The lowest id is kept, as AutoCAD keeps the source object: its
            // layer, style, colour and properties are the joined polyline's.
            const EntityId keep = *std::ranges::min_element(chain.ids);
            Entity kept = *session_.original(keep);
            kept.geometry = chain.polyline;
            katana::entity::setHeights(kept.properties, chain.heights);
            session_.replace(keep, {kept});
            for (const EntityId id : chain.ids) {
                if (id != keep) {
                    session_.replace(id, {});
                }
            }
            joined += chain.ids.size();
        }
        const std::size_t lone = plan.runs - joined;
        return ToolStep::done(session_.commit("JOIN"),
                              "Joined " + plural(joined, "object", "objects") + " into " +
                                  plural(plan.chains.size(), "polyline", "polylines") + "." +
                                  leftOut(plan, lone));
    }

    ToolStep undo() override
    {
        if (step_ == Step::Tolerance) {
            step_ = Step::Select;
            return ToolStep::next();
        }
        if (picked_.empty()) {
            return ToolStep::rejected("Nothing to undo.");
        }
        picked_.pop_back();
        return ToolStep::next();
    }

    [[nodiscard]] ToolFeedback preview(const Point2& /*cursor*/) const override
    {
        // The polylines Enter would make from what is selected now.
        ToolFeedback feedback;
        const JoinPlan plan =
            planJoin(session_.document(), selectionNow(context_, picked_), tolerance());
        for (const Chain& chain : plan.chains) {
            feedback.shapes.emplace_back(chain.polyline);
        }
        return feedback;
    }

  private:
    enum class Step { Select, Tolerance };

    // A tolerance of zero still has to forgive the last bit of a computed end.
    [[nodiscard]] double tolerance() const
    {
        return std::max(defaults_->joinTolerance, tol::kGeometric);
    }

    ToolContext context_;
    Defaults defaults_;
    EditSession session_;
    Step step_ = Step::Select;
    std::vector<EntityId> picked_;
};

// ---- Explode -----------------------------------------------------------------------

class ExplodeTool final : public InteractiveTool {
  public:
    explicit ExplodeTool(const ToolContext& context)
        : context_(context), session_(context.document)
    {
    }

    [[nodiscard]] std::string prompt() const override
    {
        const std::size_t count = selectionNow(context_, picked_).size();
        return "Select polylines to explode (" + plural(count, "selected", "selected") + ")";
    }

    [[nodiscard]] ToolInput expects() const override { return ToolInput::Selection; }

    ToolStep entity(EntityId id, const Point2& /*at*/) override
    {
        if (std::ranges::find(picked_, id) == picked_.end()) {
            picked_.push_back(id);
        }
        return ToolStep::next();
    }

    ToolStep value(std::string_view text) override
    {
        if (isOption(text, "Undo", "U")) {
            return undo();
        }
        return ToolStep::rejected("Select the polylines to explode and press Enter.");
    }

    ToolStep enter() override
    {
        const auto ids = selectionNow(context_, picked_);
        if (ids.empty()) {
            return ToolStep::rejected("Select the polylines to explode first.");
        }
        std::size_t polylines = 0;
        std::size_t lines = 0;
        std::size_t locked = 0;
        std::size_t other = 0;
        std::vector<std::pair<EntityId, std::vector<Entity>>> exploded;
        for (const EntityId id : ids) {
            const Entity& entity = *session_.original(id);
            const auto* polyline = std::get_if<Polyline2>(&entity.geometry);
            if (polyline == nullptr) {
                ++other;
                continue;
            }
            if (refusalToEdit(session_.document(), id)) {
                ++locked;
                continue;
            }
            auto pieces = explode(entity, *polyline);
            if (pieces.empty()) {
                ++other; // every vertex in one place: there is no line to make
                continue;
            }
            ++polylines;
            lines += pieces.size();
            exploded.emplace_back(id, std::move(pieces));
        }
        std::string note;
        if (locked > 0) {
            note += " " + plural(locked, "polyline on a locked layer was",
                                 "polylines on locked layers were") +
                    " left as it was.";
        }
        if (other > 0) {
            note += " " + plural(other, "object that is not a polyline was",
                                 "objects that are not polylines were") +
                    " left as it was.";
        }
        if (exploded.empty()) {
            return ToolStep::rejected("Only polylines can be exploded, and the selection has "
                                      "none that can be." +
                                      note);
        }
        session_.begin();
        for (auto& [id, pieces] : exploded) {
            session_.replace(id, {});
            for (Entity& piece : pieces) {
                session_.add(std::move(piece));
            }
        }
        return ToolStep::done(session_.commit("EXPLODE"),
                              "Exploded " + plural(polylines, "polyline", "polylines") + " into " +
                                  plural(lines, "line", "lines") + "." + note);
    }

    ToolStep undo() override
    {
        if (picked_.empty()) {
            return ToolStep::rejected("Nothing to undo.");
        }
        picked_.pop_back();
        return ToolStep::next();
    }

    [[nodiscard]] ToolFeedback preview(const Point2& /*cursor*/) const override
    {
        // Where the polylines will come apart: every vertex of each.
        ToolFeedback feedback;
        for (const EntityId id : selectionNow(context_, picked_)) {
            const Entity* entity = session_.original(id);
            if (const auto* polyline = std::get_if<Polyline2>(&entity->geometry)) {
                feedback.markers.insert(feedback.markers.end(), polyline->vertices.begin(),
                                        polyline->vertices.end());
            }
        }
        return feedback;
    }

  private:
    // A line per segment - the closing one too - wearing the polyline's
    // layer, style, colour and properties, with the heights of its two ends.
    // A segment with no length (a repeated vertex) makes no line: the model
    // would refuse it.
    static std::vector<Entity> explode(const Entity& entity, const Polyline2& polyline)
    {
        const auto heights = katana::entity::heightsOf(entity.properties, polyline.vertices.size());
        std::vector<Entity> lines;
        for (std::size_t i = 0; i < polyline.segmentCount(); ++i) {
            const Segment2 segment = polyline.segment(i);
            if (segment.isDegenerate()) {
                continue;
            }
            Entity line = entity;
            line.geometry = segment;
            katana::entity::setHeights(line.properties,
                                       {heights[i], heights[(i + 1) % polyline.vertices.size()]});
            lines.push_back(std::move(line));
        }
        return lines;
    }

    ToolContext context_;
    EditSession session_;
    std::vector<EntityId> picked_;
};

} // namespace

ToolPtr makeJoinTool(const ToolContext& context, Defaults defaults)
{
    return std::make_unique<JoinTool>(context, std::move(defaults));
}

ToolPtr makeExplodeTool(const ToolContext& context)
{
    return std::make_unique<ExplodeTool>(context);
}

} // namespace katana::cad::tools::modify_edit
