// Break and Break at Point. Break takes the object at the point it is picked
// - the first break point, as in AutoCAD, or [First point] to give it exactly -
// and a second point, and removes what lies between; typing @0,0 for the
// second point breaks at the first without a gap. Break at Point splits a
// line, arc or open polyline in two at one point; a circle or closed polyline
// has no ends, so one point cannot split it in two, and it is refused when
// picked. Each is ONE command: the object keeps its id for the first piece
// and the second piece is new.

#include <cmath>

#include "katana/cad/drawing/vertex_editing.hpp"
#include "katana/entity/entity_geometry.hpp"
#include "katana/geometry/polyline_vertices.hpp"
#include "modify_edit_support.hpp"

namespace katana::cad::tools::modify_edit {

namespace {

namespace tol = katana::math::tolerance;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::geometry::CurvePolyline2;
using katana::math::kTwoPi;
using katana::math::normalizeAngle;

struct Broken {
    std::vector<Geometry> pieces;
    std::vector<Geometry> removed; // for the preview
};

// The point of `geometry` nearest `p`; `p` itself when it is on it already,
// so a snapped point is kept to the last bit rather than re-projected.
Point2 onto(const Geometry& geometry, const Point2& p)
{
    if (katana::entity::distanceTo(geometry, p) <= tol::kGeometric) {
        return p;
    }
    if (const auto* segment = std::get_if<Segment2>(&geometry)) {
        return segment->closestPoint(p);
    }
    if (const auto* arc = std::get_if<Arc2>(&geometry)) {
        return arc->closestPoint(p);
    }
    if (const auto* circle = std::get_if<Circle2>(&geometry)) {
        return circle->closestPoint(p);
    }
    if (const auto* polyline = std::get_if<Polyline2>(&geometry)) {
        return polyline->closestPoint(p).value_or(p);
    }
    if (const auto* curved = std::get_if<CurvePolyline2>(&geometry)) {
        return curved->closestPoint(p).value_or(p);
    }
    return p;
}

// Breaks an open curve measured by fractions or stations from 0 to `length`:
// `piece(from, to)` makes the part between two of them.
template <typename Piece>
Result<Broken> breakOpen(double first, double second, double length, const std::string& kind,
                         Piece piece)
{
    const double lo = std::min(first, second);
    const double hi = std::max(first, second);
    Broken out;
    if (hi - lo <= tol::kGeometric) {
        if (lo <= tol::kGeometric || lo >= length - tol::kGeometric) {
            return makeError(ErrorCode::InvalidArgument,
                             "The break point is at an end of the " + kind +
                                 ", so there is nothing to split.");
        }
        out.pieces = {piece(0.0, lo), piece(lo, length)};
        return out;
    }
    if (lo <= tol::kGeometric && hi >= length - tol::kGeometric) {
        return makeError(ErrorCode::InvalidArgument,
                         "The two points take in the whole " + kind + "; use Erase to remove it.");
    }
    if (lo > tol::kGeometric) {
        out.pieces.push_back(piece(0.0, lo));
    }
    if (hi < length - tol::kGeometric) {
        out.pieces.push_back(piece(hi, length));
    }
    out.removed.push_back(piece(lo, hi));
    return out;
}

// `a` and `b` are on the geometry already (onto()).
Result<Broken> breakGeometry(const Geometry& geometry, const Point2& a, const Point2& b)
{
    const std::string kind = kindName(geometry);
    if (const auto* segment = std::get_if<Segment2>(&geometry)) {
        // Measured in length, so "no gap" and "at an end" mean the same
        // distance on every line; the break points themselves go in as given.
        const double length = segment->length();
        const double ta = segment->parameterOf(a) * length;
        const double tb = segment->parameterOf(b) * length;
        const auto at = [&](double s) {
            if (std::abs(s - ta) <= tol::kGeometric) {
                return a;
            }
            if (std::abs(s - tb) <= tol::kGeometric) {
                return b;
            }
            return s <= 0.0 ? segment->start : segment->end;
        };
        return breakOpen(ta, tb, length, kind, [&](double from, double to) {
            return Geometry{Segment2{at(from), at(to)}};
        });
    }
    if (const auto* arc = std::get_if<Arc2>(&geometry)) {
        const double length = arc->length();
        const double sa = arc->parameterOfAngle((a - arc->center).angle()) * length;
        const double sb = arc->parameterOfAngle((b - arc->center).angle()) * length;
        return breakOpen(sa, sb, length, kind, [&](double from, double to) {
            return Geometry{Arc2{arc->center, arc->radius,
                                 arc->startAngle + arc->sweep * (from / length),
                                 arc->sweep * ((to - from) / length)}};
        });
    }
    if (const auto* circle = std::get_if<Circle2>(&geometry)) {
        // AutoCAD's rule: the part removed runs counter-clockwise from the
        // first point to the second.
        const double from = normalizeAngle((a - circle->center).angle());
        const double to = normalizeAngle((b - circle->center).angle());
        const double gap = normalizeAngle(to - from);
        if (gap * circle->radius <= tol::kGeometric ||
            (kTwoPi - gap) * circle->radius <= tol::kGeometric) {
            return makeError(ErrorCode::InvalidArgument,
                             "A circle cannot be broken at a single point; pick two points.");
        }
        Broken out;
        out.pieces.emplace_back(Arc2{circle->center, circle->radius, to, kTwoPi - gap});
        out.removed.emplace_back(Arc2{circle->center, circle->radius, from, gap});
        return out;
    }
    if (const auto* polyline = std::get_if<Polyline2>(&geometry)) {
        const PolylinePath path(*polyline);
        const double sa = path.stationOf(a);
        const double sb = path.stationOf(b);
        // The break points exactly as given where a piece ends at one.
        const auto exact = [&](Polyline2 piece, double from, double to) {
            for (const auto& [station, point] : {std::pair{sa, a}, std::pair{sb, b}}) {
                if (std::abs(from - station) <= tol::kGeometric) {
                    piece.vertices.front() = point;
                }
                if (std::abs(to - station) <= tol::kGeometric) {
                    piece.vertices.back() = point;
                }
            }
            return Geometry{std::move(piece)};
        };
        if (!path.closed()) {
            return breakOpen(sa, sb, path.length(), kind, [&](double from, double to) {
                return exact(path.between(from, to), from, to);
            });
        }
        // Closed: the part removed runs from the first point to the second in
        // the direction the polyline was drawn, and what is left is one open
        // polyline from the second point round to the first.
        if (std::abs(sa - sb) <= tol::kGeometric ||
            std::abs(std::abs(sa - sb) - path.length()) <= tol::kGeometric) {
            return makeError(ErrorCode::InvalidArgument,
                             "A closed polyline cannot be broken at a single point; pick two "
                             "points.");
        }
        Broken out;
        if (sa < sb) {
            out.pieces.push_back(exact(path.wrapping(sb, sa), sb, sa));
            out.removed.push_back(exact(path.between(sa, sb), sa, sb));
        } else {
            out.pieces.push_back(exact(path.between(sb, sa), sb, sa));
            out.removed.push_back(exact(path.wrapping(sa, sb), sa, sb));
        }
        return out;
    }
    if (const auto* curved = std::get_if<CurvePolyline2>(&geometry)) {
        // As a straight polyline, measured along its arcs (geometry::subPath);
        // the pieces are curve polylines, stored by the drawing system's rule
        // when the tool makes them entities.
        const double sa = curved->nearest(a)->station;
        const double sb = curved->nearest(b)->station;
        const double length = curved->length();
        const auto exact = [&](CurvePolyline2 piece, double from, double to) {
            for (const auto& [station, point] : {std::pair{sa, a}, std::pair{sb, b}}) {
                if (!piece.vertices.empty() && std::abs(from - station) <= tol::kGeometric) {
                    piece.vertices.front().position = point;
                }
                if (!piece.vertices.empty() && std::abs(to - station) <= tol::kGeometric) {
                    piece.vertices.back().position = point;
                }
            }
            return Geometry{std::move(piece)};
        };
        if (!curved->closed) {
            return breakOpen(sa, sb, length, kind, [&](double from, double to) {
                return exact(katana::geometry::subPath(*curved, from, to), from, to);
            });
        }
        if (std::abs(sa - sb) <= tol::kGeometric ||
            std::abs(std::abs(sa - sb) - length) <= tol::kGeometric) {
            return makeError(ErrorCode::InvalidArgument,
                             "A closed polyline cannot be broken at a single point; pick two "
                             "points.");
        }
        Broken out;
        if (sa < sb) {
            out.pieces.push_back(exact(katana::geometry::wrappingPath(*curved, sb, sa), sb, sa));
            out.removed.push_back(exact(katana::geometry::subPath(*curved, sa, sb), sa, sb));
        } else {
            out.pieces.push_back(exact(katana::geometry::subPath(*curved, sb, sa), sb, sa));
            out.removed.push_back(exact(katana::geometry::wrappingPath(*curved, sa, sb), sa, sb));
        }
        return out;
    }
    return makeError(ErrorCode::Unsupported, "A " + kind +
                                                 " cannot be broken; pick a line, arc, circle or "
                                                 "polyline.");
}

class BreakTool final : public InteractiveTool {
  public:
    BreakTool(const ToolContext& context, bool atPoint)
        : context_(context), atPoint_(atPoint), session_(context.document)
    {
    }

    [[nodiscard]] std::string prompt() const override
    {
        switch (step_) {
        case Step::Object:
            return "Select object to break";
        case Step::First:
            return "Specify first break point";
        case Step::Second:
            return atPoint_ ? "Specify break point" : "Specify second break point or [First point]";
        }
        return {};
    }

    [[nodiscard]] ToolInput expects() const override
    {
        return step_ == Step::Object ? ToolInput::Entity : ToolInput::Point;
    }

    ToolStep entity(EntityId id, const Point2& at) override
    {
        if (step_ != Step::Object) {
            return InteractiveTool::entity(id, at);
        }
        if (auto refusal = refusalToEdit(session_.document(), id)) {
            return ToolStep::rejected(*refusal);
        }
        const Entity* entity = session_.original(id);
        const auto type = entity->type();
        using katana::entity::EntityType;
        if (type != EntityType::Line && type != EntityType::Arc && type != EntityType::Circle &&
            type != EntityType::Polyline && type != EntityType::CurvePolyline) {
            return ToolStep::rejected("A " + kindName(entity->geometry) +
                                      " cannot be broken; pick a line, arc, circle or polyline.");
        }
        const auto* polyline = std::get_if<Polyline2>(&entity->geometry);
        const auto* curved = std::get_if<CurvePolyline2>(&entity->geometry);
        if (atPoint_ && (type == EntityType::Circle || (polyline != nullptr && polyline->closed) ||
                         (curved != nullptr && curved->closed))) {
            // Refused at the pick: every point after it would be refused as
            // "one point", and a one-point tool cannot take a second.
            return ToolStep::rejected(asSentence(withArticle(kindName(entity->geometry)) +
                                                 " has no ends, so one point cannot split it; "
                                                 "use Break with two points"));
        }
        target_ = id;
        first_ = onto(entity->geometry, at);
        step_ = Step::Second;
        return ToolStep::next();
    }

    ToolStep point(const Point2& at) override
    {
        if (step_ == Step::First) {
            first_ = onto(session_.original(target_)->geometry, at);
            step_ = Step::Second;
            return ToolStep::next();
        }
        if (step_ != Step::Second) {
            return InteractiveTool::point(at);
        }
        const Entity& original = *session_.original(target_);
        const Point2 second = onto(original.geometry, at);
        auto broken = breakGeometry(original.geometry, atPoint_ ? second : first_, second);
        if (!broken) {
            return ToolStep::rejected(broken.error().message);
        }
        std::vector<Entity> pieces;
        for (Geometry& geometry : broken->pieces) {
            if (const auto* curved = std::get_if<CurvePolyline2>(&geometry)) {
                // Its heights are in the geometry; stored as the simplest kind.
                auto piece = writePolyline(original, *curved);
                if (!piece) {
                    return ToolStep::rejected(asSentence(piece.error().message));
                }
                pieces.push_back(std::move(*piece));
                continue;
            }
            Entity piece = original;
            piece.geometry = std::move(geometry);
            carryHeights(original, piece);
            pieces.push_back(std::move(piece));
        }
        session_.begin();
        if (auto status = session_.replace(target_, std::move(pieces)); !status) {
            return ToolStep::rejected(status.error().message);
        }
        return ToolStep::done(session_.commit("BREAK"),
                              broken->pieces.size() == 2 ? "Broken in two." : "Broken.");
    }

    ToolStep value(std::string_view text) override
    {
        if (isOption(text, "Undo", "U")) {
            return undo();
        }
        if (step_ == Step::Second && !atPoint_ && isOption(text, "First", "F")) {
            step_ = Step::First;
            return ToolStep::next();
        }
        return ToolStep::rejected("'" + std::string(text) + "' is not an option here.");
    }

    ToolStep undo() override
    {
        switch (step_) {
        case Step::Object:
            return ToolStep::rejected("Nothing to undo.");
        case Step::First:
            step_ = Step::Second;
            return ToolStep::next();
        case Step::Second:
            step_ = Step::Object;
            return ToolStep::next();
        }
        return ToolStep::rejected("Nothing to undo.");
    }

    [[nodiscard]] ToolFeedback preview(const Point2& cursor) const override
    {
        ToolFeedback feedback;
        if (step_ == Step::Object) {
            return feedback;
        }
        const Geometry& geometry = session_.original(target_)->geometry;
        const Point2 here = onto(geometry, cursor);
        if (step_ == Step::First || atPoint_) {
            feedback.markers.push_back(here);
            return feedback;
        }
        feedback.markers = {first_, here};
        if (auto broken = breakGeometry(geometry, first_, here)) {
            feedback.shapes = std::move(broken->removed);
        }
        return feedback;
    }

    // The first break point, so "@0,0" breaks there and "@2<0" measures from it.
    [[nodiscard]] std::optional<Point2> lastPoint() const override
    {
        return step_ == Step::Second && !atPoint_ ? std::optional<Point2>(first_) : std::nullopt;
    }

  private:
    enum class Step { Object, First, Second };

    ToolContext context_;
    bool atPoint_;
    EditSession session_;
    Step step_ = Step::Object;
    EntityId target_ = katana::entity::kInvalidEntityId;
    Point2 first_;
};

} // namespace

ToolPtr makeBreakTool(const ToolContext& context)
{
    return std::make_unique<BreakTool>(context, false);
}

ToolPtr makeBreakAtPointTool(const ToolContext& context)
{
    return std::make_unique<BreakTool>(context, true);
}

} // namespace katana::cad::tools::modify_edit
