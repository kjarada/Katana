// Trim and Extend: choose the edges, then pick the objects, as many as wanted,
// Enter to finish. They are one tool with two modes because everything but
// the geometry is the same - the edges step, "every object" when Enter is
// pressed at once, the repeated picks and their Undo.

#include <algorithm>
#include <cmath>
#include <span>

#include "katana/cad/selection.hpp"
#include "katana/entity/entity_geometry.hpp"
#include "families.hpp"
#include "modify_edit_support.hpp"

namespace katana::cad::tools::modify_edit {

namespace {

namespace tol = katana::math::tolerance;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::geometry::IntersectionKind;
using katana::math::kTwoPi;

// ---- trimming one piece ------------------------------------------------------------

struct Cut {
    std::vector<Geometry> remaining;
    std::vector<Geometry> removed; // for the preview: what the pick takes away
};

// The gaps a trim left in [0, 1] once `kept` (fractions along the original)
// are taken out.
std::vector<std::pair<double, double>> gaps(std::vector<std::pair<double, double>> kept,
                                            double tiny)
{
    std::ranges::sort(kept);
    std::vector<std::pair<double, double>> out;
    double cursor = 0.0;
    for (const auto& [from, to] : kept) {
        if (from - cursor > tiny) {
            out.emplace_back(cursor, from);
        }
        cursor = std::max(cursor, to);
    }
    if (1.0 - cursor > tiny) {
        out.emplace_back(cursor, 1.0);
    }
    return out;
}

std::vector<Geometry> removedParts(const Curve2& original, const std::vector<Curve2>& remaining)
{
    std::vector<Geometry> out;
    if (const auto* segment = std::get_if<Segment2>(&original)) {
        std::vector<std::pair<double, double>> kept;
        for (const Curve2& piece : remaining) {
            const auto& part = std::get<Segment2>(piece);
            kept.emplace_back(segment->parameterOf(part.start), segment->parameterOf(part.end));
        }
        for (const auto& [from, to] : gaps(kept, tol::kGeometric / segment->length())) {
            out.emplace_back(Segment2{segment->pointAt(from), segment->pointAt(to)});
        }
    } else if (const auto* arc = std::get_if<Arc2>(&original)) {
        std::vector<std::pair<double, double>> kept;
        for (const Curve2& piece : remaining) {
            const auto& part = std::get<Arc2>(piece);
            const double from = (part.startAngle - arc->startAngle) / arc->sweep;
            kept.emplace_back(from, from + part.sweep / arc->sweep);
        }
        for (const auto& [from, to] : gaps(kept, tol::kGeometric / arc->length())) {
            out.emplace_back(Arc2{arc->center, arc->radius, arc->startAngle + arc->sweep * from,
                                  arc->sweep * (to - from)});
        }
    } else if (const auto* circle = std::get_if<Circle2>(&original)) {
        // A trimmed circle is the one arc it keeps; the rest of the turn went.
        if (remaining.size() == 1) {
            const auto& kept = std::get<Arc2>(remaining.front());
            out.emplace_back(Arc2{circle->center, circle->radius, kept.startAngle + kept.sweep,
                                  kTwoPi - kept.sweep});
        } else {
            out.emplace_back(*circle);
        }
    }
    return out;
}

void sortUnique(std::vector<double>& values)
{
    std::ranges::sort(values);
    values.erase(std::unique(values.begin(), values.end(),
                             [](double a, double b) { return b - a <= tol::kGeometric; }),
                 values.end());
}

// geometry::trim for a polyline, which it does not take: the crossings are
// measured along the polyline and the span between the two either side of the
// pick goes, exactly as trim() does for a line.
Result<Cut> trimPolyline(const Polyline2& polyline, std::span<const Curve2> cutters,
                         const Point2& pick)
{
    const PolylinePath path(polyline);
    std::vector<double> cuts;
    for (std::size_t i = 0; i < path.segmentCount(); ++i) {
        const Segment2 segment = path.segment(i);
        if (segment.isDegenerate()) {
            continue;
        }
        for (const Curve2& cutter : cutters) {
            const auto hit = katana::geometry::intersect(Curve2{segment}, cutter);
            if (hit.kind != IntersectionKind::Points) {
                continue;
            }
            for (std::size_t k = 0; k < hit.count; ++k) {
                cuts.push_back(path.stationOf(i, hit.points[k]));
            }
        }
    }
    const double length = path.length();
    const double at = path.stationOf(pick);
    Cut out;
    if (!path.closed()) {
        std::erase_if(cuts, [&](double s) {
            return s <= tol::kGeometric || s >= length - tol::kGeometric;
        });
        sortUnique(cuts);
        if (cuts.empty()) {
            return makeError(ErrorCode::InvalidGeometry, "no cutting edge crosses that polyline");
        }
        double lo = 0.0;
        double hi = length;
        for (const double cut : cuts) {
            if (cut <= at) {
                lo = cut;
            } else {
                hi = cut;
                break;
            }
        }
        if (lo > 0.0) {
            out.remaining.emplace_back(path.between(0.0, lo));
        }
        if (hi < length) {
            out.remaining.emplace_back(path.between(hi, length));
        }
        out.removed.emplace_back(path.between(lo, hi));
        return out;
    }
    // Closed: a crossing at the end of the closing segment is the one at the
    // start, seen from the other side of the seam.
    for (double& cut : cuts) {
        if (cut >= length - tol::kGeometric) {
            cut = 0.0;
        }
    }
    sortUnique(cuts);
    if (cuts.size() < 2) {
        return makeError(ErrorCode::InvalidGeometry,
                         "a closed polyline needs two crossings with the cutting edges to be "
                         "trimmed");
    }
    const auto above = std::ranges::upper_bound(cuts, at);
    if (above == cuts.begin() || above == cuts.end()) {
        // The pick is in the span that runs through the first vertex.
        out.remaining.emplace_back(path.between(cuts.front(), cuts.back()));
        out.removed.emplace_back(path.wrapping(cuts.back(), cuts.front()));
    } else {
        const double lo = *(above - 1);
        const double hi = *above;
        out.remaining.emplace_back(path.wrapping(hi, lo));
        out.removed.emplace_back(path.between(lo, hi));
    }
    return out;
}

Result<Cut> trimGeometry(const Geometry& piece, std::span<const Curve2> cutters,
                         const Point2& pick)
{
    if (const auto* polyline = std::get_if<Polyline2>(&piece)) {
        return trimPolyline(*polyline, cutters, pick);
    }
    const auto curve = asCurve(piece);
    if (!curve) {
        return makeError(ErrorCode::Unsupported, "only lines, arcs, circles and polylines trim");
    }
    auto trimmed = katana::geometry::trim(*curve, cutters, pick);
    if (!trimmed) {
        return trimmed.error();
    }
    Cut out;
    for (const Curve2& remaining : trimmed->remaining) {
        out.remaining.push_back(asGeometry(remaining));
    }
    out.removed = removedParts(*curve, trimmed->remaining);
    return out;
}

// ---- extending one piece -----------------------------------------------------------

struct Grow {
    Geometry geometry;
    Geometry added; // for the preview: the new length
};

Result<Grow> extendGeometry(const Geometry& piece, std::span<const Curve2> boundaries,
                            const Point2& pick)
{
    if (const auto* segment = std::get_if<Segment2>(&piece)) {
        auto extended = katana::geometry::extend(Curve2{*segment}, boundaries, pick);
        if (!extended) {
            return extended.error();
        }
        const auto& grown = std::get<Segment2>(*extended);
        const bool endMoved = grown.start == segment->start;
        return Grow{grown, endMoved ? Segment2{segment->end, grown.end}
                                    : Segment2{grown.start, segment->start}};
    }
    if (const auto* arc = std::get_if<Arc2>(&piece)) {
        auto extended = katana::geometry::extend(Curve2{*arc}, boundaries, pick);
        if (!extended) {
            return extended.error();
        }
        const auto& grown = std::get<Arc2>(*extended);
        const double extra = grown.sweep - arc->sweep;
        const bool endMoved = std::abs(grown.startAngle - arc->startAngle) <= tol::kAngular;
        return Grow{grown, Arc2{arc->center, arc->radius,
                                endMoved ? arc->endAngle() : grown.startAngle, extra}};
    }
    if (const auto* polyline = std::get_if<Polyline2>(&piece)) {
        // The end nearer the pick along the polyline, whatever vertex the pick
        // happens to be closest to: an interior vertex is not an end.
        const PolylinePath path(*polyline);
        const bool atStart = path.stationOf(pick) < 0.5 * path.length();
        const std::size_t n = polyline->vertices.size();
        const Segment2 end = atStart ? Segment2{polyline->vertices[1], polyline->vertices[0]}
                                     : Segment2{polyline->vertices[n - 2], polyline->vertices[n - 1]};
        if (end.isDegenerate()) {
            return makeError(ErrorCode::Unsupported,
                             "the end segment of that polyline has no length");
        }
        auto extended = katana::geometry::extend(Curve2{end}, boundaries, end.end);
        if (!extended) {
            return extended.error();
        }
        const Point2 reached = std::get<Segment2>(*extended).end;
        Polyline2 grown = *polyline;
        grown.vertices[atStart ? 0 : n - 1] = reached;
        return Grow{grown, Segment2{end.end, reached}};
    }
    return makeError(ErrorCode::Unsupported, "only lines, arcs and open polylines extend");
}

// ---- the tool ----------------------------------------------------------------------

enum class Mode { Trim, Extend };

class EdgeTool final : public InteractiveTool {
  public:
    EdgeTool(const ToolContext& context, Mode mode)
        : context_(context), mode_(mode), session_(context.document)
    {
    }

    [[nodiscard]] std::string prompt() const override
    {
        const bool trim = mode_ == Mode::Trim;
        if (step_ == Step::Edges) {
            const std::size_t count = selectionNow(context_, picked_).size();
            const std::string what = trim ? "Select cutting edges" : "Select boundary edges";
            if (count == 0) {
                return what + " or <all objects>";
            }
            return what + (picked_.empty() ? "" : " or [Undo]") + " <use " +
                   std::to_string(count) + (count == 1 ? " edge>" : " edges>");
        }
        return std::string(trim ? "Select object to trim" : "Select object to extend") +
               " or [Undo]";
    }

    [[nodiscard]] ToolInput expects() const override
    {
        return step_ == Step::Edges ? ToolInput::Selection : ToolInput::Entity;
    }

    ToolStep entity(EntityId id, const Point2& at) override
    {
        if (step_ == Step::Edges) {
            if (std::ranges::find(picked_, id) == picked_.end()) {
                picked_.push_back(id);
            }
            return ToolStep::next();
        }
        return pickTarget(id, at);
    }

    ToolStep value(std::string_view text) override
    {
        if (isOption(text, "Undo", "U")) {
            return undo();
        }
        if (step_ == Step::Edges) {
            return ToolStep::rejected(
                mode_ == Mode::Trim
                    ? "Pick the cutting edges and press Enter, or press Enter to cut with every "
                      "object."
                    : "Pick the boundary edges and press Enter, or press Enter to extend to every "
                      "object.");
        }
        return ToolStep::rejected(mode_ == Mode::Trim
                                      ? "Pick the part of an object to trim away, or press Enter "
                                        "to finish."
                                      : "Pick an object near the end to extend, or press Enter "
                                        "to finish.");
    }

    // Esc keeps what the session has done: its Enter only ever commits.
    ToolStep cancel() override { return keepWorkOnEscape(*this); }

    ToolStep enter() override
    {
        if (step_ == Step::Edges) {
            edges_ = selectionNow(context_, picked_);
            all_ = edges_.empty();
            if (all_) {
                cacheEveryEdge();
            }
            step_ = Step::Targets;
            return ToolStep::next(all_ ? "Every object is an edge."
                                       : std::to_string(edges_.size()) +
                                             (edges_.size() == 1 ? " edge." : " edges."));
        }
        const std::size_t count = session_.operations();
        if (count == 0) {
            return ToolStep::done(nullptr);
        }
        const std::string noun = mode_ == Mode::Trim ? (count == 1 ? " part" : " parts")
                                                     : (count == 1 ? " end" : " ends");
        return ToolStep::done(session_.commit(mode_ == Mode::Trim ? "TRIM" : "EXTEND"),
                              (mode_ == Mode::Trim ? "Trimmed " : "Extended ") +
                                  std::to_string(count) + noun + ".");
    }

    ToolStep undo() override
    {
        if (step_ == Step::Edges) {
            if (picked_.empty()) {
                return ToolStep::rejected("Nothing to undo: no edge has been picked.");
            }
            picked_.pop_back();
            return ToolStep::next();
        }
        if (session_.undo()) {
            marks_.pop_back();
            return ToolStep::next(mode_ == Mode::Trim ? "Undid the last trim."
                                                      : "Undid the last extend.");
        }
        // Nothing trimmed yet: step back to choosing the edges.
        step_ = Step::Edges;
        all_ = false;
        edges_.clear();
        everyEdge_.clear();
        return ToolStep::next();
    }

    [[nodiscard]] ToolFeedback preview(const Point2& cursor) const override
    {
        ToolFeedback feedback;
        if (step_ == Step::Edges) {
            // The edges chosen so far, so the user sees what will cut.
            for (const EntityId id : selectionNow(context_, picked_)) {
                if (const Entity* entity = session_.original(id)) {
                    feedback.shapes.push_back(entity->geometry);
                }
            }
            return feedback;
        }
        for (const auto& mark : marks_) {
            feedback.shapes.insert(feedback.shapes.end(), mark.begin(), mark.end());
        }
        // What a click here would do, found as the view would pick it.
        const Model& model = session_.document().model();
        const auto under = pickEntity(model, cursor, std::max(context_.pickTolerance, 0.0), {},
                                      &session_.document().spatialIndex());
        if (under) {
            if (auto outcome = attempt(*under, cursor)) {
                feedback.shapes.insert(feedback.shapes.end(), outcome->mark.begin(),
                                       outcome->mark.end());
            }
        }
        return feedback;
    }

  private:
    using Model = katana::entity::Model;
    enum class Step { Edges, Targets };

    struct Outcome {
        std::vector<Entity> pieces; // the entity's pieces after the operation
        std::vector<Geometry> mark; // what the preview shows of it
    };

    ToolStep pickTarget(EntityId id, const Point2& at)
    {
        if (auto refusal = refusalToEdit(session_.document(), id)) {
            return ToolStep::rejected(*refusal);
        }
        auto outcome = attempt(id, at);
        if (!outcome) {
            return ToolStep::rejected(outcome.error().message);
        }
        session_.begin();
        if (auto status = session_.replace(id, std::move(outcome->pieces)); !status) {
            return ToolStep::rejected(status.error().message);
        }
        marks_.push_back(std::move(outcome->mark));
        return ToolStep::next();
    }

    // The operation a pick at `at` on `id` would do, without doing it. The
    // error's message is the sentence to refuse the pick with.
    [[nodiscard]] Result<Outcome> attempt(EntityId id, const Point2& at) const
    {
        const bool trim = mode_ == Mode::Trim;
        const Entity* original = session_.original(id);
        std::vector<Entity> pieces = session_.pieces(id);
        if (original == nullptr || pieces.empty()) {
            return makeError(ErrorCode::NotFound, "That object has already been trimmed away.");
        }
        const auto index = nearestPiece(pieces, at);
        const Entity& piece = pieces[*index];
        const std::string kind = kindName(piece.geometry);
        // A pick on a part an earlier pick removed: the document still draws
        // it, so the view picked the entity, but nothing of it is left there.
        const double pieceDistance = katana::entity::distanceTo(piece.geometry, at);
        const double originalDistance = katana::entity::distanceTo(original->geometry, at);
        if (pieceDistance > std::max(context_.pickTolerance, originalDistance) + tol::kGeometric) {
            return makeError(ErrorCode::NotFound,
                             "That part of the " + kind + " has already been trimmed away.");
        }
        const auto type = piece.type();
        const auto* polyline = std::get_if<Polyline2>(&piece.geometry);
        const bool closed = polyline != nullptr && polyline->closed;
        using katana::entity::EntityType;
        if (trim && type != EntityType::Line && type != EntityType::Arc &&
            type != EntityType::Circle && type != EntityType::Polyline) {
            return makeError(ErrorCode::Unsupported,
                             "A " + kind +
                                 " cannot be trimmed; pick a line, arc, circle or polyline.");
        }
        if (!trim) {
            if (type == EntityType::Circle) {
                return makeError(ErrorCode::Unsupported, "A circle has no end to extend.");
            }
            if (closed) {
                return makeError(ErrorCode::Unsupported, "A closed polyline has no end to extend.");
            }
            if (type != EntityType::Line && type != EntityType::Arc &&
                type != EntityType::Polyline) {
                return makeError(ErrorCode::Unsupported,
                                 "A " + kind + " cannot be extended; pick a line, arc or polyline.");
            }
        }

        const std::vector<Curve2> edges = edgesFor(id, piece.geometry);
        Outcome outcome;
        std::vector<Entity> replacement(pieces.begin(), pieces.begin() + *index);
        if (trim) {
            auto cut = trimGeometry(piece.geometry, edges, at);
            if (!cut) {
                return makeError(ErrorCode::InvalidGeometry,
                                 type == EntityType::Circle
                                     ? "A circle needs two crossings with the cutting edges to be "
                                       "trimmed."
                                     : (closed
                                            ? "A closed polyline needs two crossings with the "
                                              "cutting edges to be trimmed."
                                            : "No cutting edge crosses that " + kind + "."));
            }
            for (Geometry& remaining : cut->remaining) {
                Entity part = piece;
                part.geometry = std::move(remaining);
                carryHeights(*original, part);
                replacement.push_back(std::move(part));
            }
            outcome.mark = std::move(cut->removed);
        } else {
            auto grown = extendGeometry(piece.geometry, edges, at);
            if (!grown) {
                // Unsupported is extendGeometry's own refusal, already a
                // reason; anything else is the geometry finding nothing ahead.
                return makeError(ErrorCode::InvalidGeometry,
                                 grown.error().code == ErrorCode::Unsupported
                                     ? asSentence(grown.error().message)
                                     : "No boundary lies ahead of that end of the " + kind + ".");
            }
            Entity part = piece;
            part.geometry = std::move(grown->geometry);
            carryHeights(*original, part);
            replacement.push_back(std::move(part));
            outcome.mark.push_back(std::move(grown->added));
        }
        replacement.insert(replacement.end(), pieces.begin() + *index + 1, pieces.end());
        outcome.pieces = std::move(replacement);
        return outcome;
    }

    // The curves that cut or bound `target`: the chosen edges as the session
    // has made them, never the target itself.
    [[nodiscard]] std::vector<Curve2> edgesFor(EntityId target, const Geometry& piece) const
    {
        std::vector<Curve2> curves;
        const auto fromSession = [&](EntityId id) {
            if (id == target) {
                return;
            }
            for (const Entity& part : session_.pieces(id)) {
                appendEdges(part.geometry, curves);
            }
        };
        if (!all_) {
            for (const EntityId id : edges_) {
                fromSession(id);
            }
            return curves;
        }
        // Every object: the cache of the drawing's edges, except where the
        // session has changed an entity, which it answers for instead. A trim
        // cuts only where edges cross the piece, so edges nowhere near it are
        // passed over; an extend reaches out along the piece, so it takes all.
        const auto near = katana::entity::boundingBox(piece).inflated(tol::kGeometric);
        for (const auto& [id, curve] : everyEdge_) {
            if (id == target || session_.edited(id)) {
                continue;
            }
            if (mode_ == Mode::Trim && !katana::geometry::boundingBox(curve).intersects(near)) {
                continue;
            }
            curves.push_back(curve);
        }
        for (const EntityId id : session_.editedIds()) {
            fromSession(id);
        }
        return curves;
    }

    void cacheEveryEdge()
    {
        const Model& model = session_.document().model();
        everyEdge_.clear();
        model.entities.forEach([&](const Entity& entity) {
            // What the user can see: a layer turned off does not cut.
            if (!isDrawn(model, entity, kNoLayerOverrides)) {
                return;
            }
            std::vector<Curve2> curves;
            appendEdges(entity.geometry, curves);
            for (Curve2& curve : curves) {
                everyEdge_.emplace_back(entity.id, std::move(curve));
            }
        });
    }

    ToolContext context_;
    Mode mode_;
    Step step_ = Step::Edges;
    std::vector<EntityId> picked_; // edges picked in the Edges step
    std::vector<EntityId> edges_;  // the edges chosen; empty when all_
    bool all_ = false;
    std::vector<std::pair<EntityId, Curve2>> everyEdge_; // when all_
    EditSession session_;
    std::vector<std::vector<Geometry>> marks_; // per operation, for the preview
};

} // namespace

ToolPtr makeTrimTool(const ToolContext& context)
{
    return std::make_unique<EdgeTool>(context, Mode::Trim);
}

ToolPtr makeExtendTool(const ToolContext& context)
{
    return std::make_unique<EdgeTool>(context, Mode::Extend);
}

} // namespace katana::cad::tools::modify_edit
