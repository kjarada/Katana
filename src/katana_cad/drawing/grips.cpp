#include "katana/cad/drawing/grips.hpp"

#include <algorithm>
#include <cmath>
#include <limits>
#include <map>
#include <memory>
#include <set>
#include <utility>

#include "katana/cad/drawing/vertex_editing.hpp"
#include "katana/cad/selection.hpp"
#include "katana/commands/change_set.hpp"
#include "katana/entity/entity_geometry.hpp"
#include "katana/math/numerics.hpp"

namespace katana::cad {

namespace cmd = katana::commands;
namespace tol = katana::math::tolerance;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::geometry::Arc2;
using katana::geometry::Circle2;
using katana::geometry::CurvePolyline2;
using katana::geometry::Ellipse2;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;
using katana::geometry::Spline2;
using katana::geometry::Vec2;

const char* toString(GripKind kind)
{
    switch (kind) {
    case GripKind::Vertex:
        return "vertex";
    case GripKind::SegmentMid:
        return "segment middle";
    case GripKind::End:
        return "end";
    case GripKind::Mid:
        return "middle";
    case GripKind::Centre:
        return "centre";
    case GripKind::Quadrant:
        return "quadrant";
    case GripKind::Insertion:
        return "insertion point";
    case GripKind::FitPoint:
        return "fit point";
    case GripKind::ControlPoint:
        return "control point";
    }
    return "grip";
}

// ---- enumeration ----------------------------------------------------------------------

std::vector<Grip> gripsOf(const Entity& entity)
{
    std::vector<Grip> out;
    const auto add = [&](GripKind kind, std::size_t index, const Point2& at) {
        out.push_back(Grip{entity.id, kind, index, at});
    };
    std::visit(
        [&](const auto& shape) {
            using Shape = std::decay_t<decltype(shape)>;
            if constexpr (std::is_same_v<Shape, katana::entity::PointGeometry>) {
                add(GripKind::Insertion, 0, shape.position);
            } else if constexpr (std::is_same_v<Shape, Segment2>) {
                add(GripKind::End, 0, shape.start);
                add(GripKind::End, 1, shape.end);
                add(GripKind::Mid, 0, shape.midpoint());
            } else if constexpr (std::is_same_v<Shape, Arc2>) {
                add(GripKind::End, 0, shape.startPoint());
                add(GripKind::End, 1, shape.endPoint());
                add(GripKind::Mid, 0, shape.midpoint());
                add(GripKind::Centre, 0, shape.center);
            } else if constexpr (std::is_same_v<Shape, Circle2>) {
                add(GripKind::Centre, 0, shape.center);
                for (std::size_t k = 0; k < 4; ++k) {
                    add(GripKind::Quadrant, k,
                        shape.pointAtAngle(katana::math::kHalfPi * static_cast<double>(k)));
                }
            } else if constexpr (std::is_same_v<Shape, Polyline2> ||
                                 std::is_same_v<Shape, CurvePolyline2>) {
                const auto polyline = readPolyline(entity);
                for (std::size_t i = 0; i < polyline->vertices.size(); ++i) {
                    add(GripKind::Vertex, i, polyline->vertices[i].position);
                }
                for (std::size_t i = 0; i < polyline->segmentCount(); ++i) {
                    add(GripKind::SegmentMid, i,
                        std::visit([](const auto& piece) { return piece.pointAt(0.5); },
                                   polyline->segment(i)));
                }
            } else if constexpr (std::is_same_v<Shape, katana::entity::TextGeometry>) {
                add(GripKind::Insertion, 0, shape.position);
            } else if constexpr (std::is_same_v<Shape, Ellipse2>) {
                add(GripKind::Centre, 0, shape.center);
                for (std::size_t k = 0; k < 4; ++k) {
                    const double t = katana::math::kHalfPi * static_cast<double>(k);
                    if (shape.containsParameter(t, tol::kAngular)) {
                        add(GripKind::Quadrant, k, shape.pointAtParameter(t));
                    }
                }
                if (!shape.isFull()) {
                    add(GripKind::End, 0, shape.startPoint());
                    add(GripKind::End, 1, shape.endPoint());
                }
            } else if constexpr (std::is_same_v<Shape, Spline2>) {
                if (shape.hasFitPoints()) {
                    for (std::size_t i = 0; i < shape.fitPoints.size(); ++i) {
                        add(GripKind::FitPoint, i, shape.fitPoints[i]);
                    }
                } else {
                    for (std::size_t i = 0; i < shape.controlPoints.size(); ++i) {
                        add(GripKind::ControlPoint, i, shape.controlPoints[i]);
                    }
                }
            } else {
                // Dimensions, labels and leaders are the annotation system's
                // to give handles to (docs/tools.md); they offer none here.
            }
        },
        entity.geometry);
    return out;
}

std::vector<Grip> gripsOfSelection(const Document& document, const std::vector<EntityId>& ids,
                                   std::size_t limit)
{
    std::vector<Grip> out;
    for (const EntityId id : ids) {
        const Entity* entity = document.model().entities.find(id);
        if (entity == nullptr || !isSelectable(document.model(), *entity, kNoLayerOverrides)) {
            continue;
        }
        auto grips = gripsOf(*entity);
        if (out.size() + grips.size() > limit) {
            return {};
        }
        out.insert(out.end(), grips.begin(), grips.end());
    }
    return out;
}

std::optional<Grip> gripAt(const std::vector<Grip>& grips, const Point2& at, double aperture)
{
    std::optional<Grip> best;
    double bestDistance = std::numeric_limits<double>::infinity();
    for (const Grip& grip : grips) {
        const double d = grip.position.distanceTo(at);
        if (d <= aperture && d < bestDistance) {
            bestDistance = d;
            best = grip;
        }
    }
    return best;
}

// ---- editing ---------------------------------------------------------------------------

namespace {

// Each of this entity's hot grips with the displacement the drag gives it:
// the grabbed one goes to the target, the rest move as far as it does.
struct Moves {
    std::vector<std::pair<Grip, Vec2>> grips;

    [[nodiscard]] Vec2 of(GripKind kind, std::size_t index = 0) const
    {
        for (const auto& [grip, delta] : grips) {
            if (grip.kind == kind && grip.index == index) {
                return delta;
            }
        }
        return Vec2{};
    }
    [[nodiscard]] bool has(GripKind kind, std::size_t index = 0) const
    {
        return std::any_of(grips.begin(), grips.end(), [&](const auto& item) {
            return item.first.kind == kind && item.first.index == index;
        });
    }
};

Moves movesFor(EntityId id, const GripDrag& drag)
{
    Moves moves;
    const Vec2 delta = drag.target - drag.grabbed.position;
    const auto push = [&](const Grip& grip) {
        if (grip.entity != id) {
            return;
        }
        for (const auto& item : moves.grips) {
            if (item.first.sameHandle(grip)) {
                return;
            }
        }
        moves.grips.emplace_back(grip, delta);
    };
    push(drag.grabbed);
    for (const Grip& grip : drag.alsoHot) {
        push(grip);
    }
    return moves;
}

katana::core::Error refused(std::string why)
{
    return katana::core::Error{ErrorCode::InvalidArgument, std::move(why), {}};
}

Result<Entity> dragPolyline(const Entity& entity, const GripDrag& drag, const Moves& moves)
{
    auto polyline = *readPolyline(entity);
    if (drag.insertVertex && drag.grabbed.entity == entity.id &&
        drag.grabbed.kind == GripKind::SegmentMid) {
        auto inserted = katana::geometry::insertVertex(polyline, drag.grabbed.index, drag.target);
        if (!inserted) {
            return inserted.error();
        }
        return writePolyline(entity, *inserted);
    }
    // Every vertex moves once, however many hot grips name it (its own and
    // those of the two segments either side).
    std::map<std::size_t, Vec2> vertexMoves;
    std::vector<std::pair<std::size_t, Vec2>> arcMids;
    for (const auto& [grip, delta] : moves.grips) {
        if (grip.kind == GripKind::Vertex && grip.index < polyline.vertices.size()) {
            vertexMoves[grip.index] = delta;
        } else if (grip.kind == GripKind::SegmentMid && grip.index < polyline.segmentCount()) {
            if (polyline.isArc(grip.index)) {
                arcMids.emplace_back(grip.index, delta);
            } else {
                vertexMoves.emplace(grip.index, delta);
                vertexMoves.emplace(polyline.segmentEnd(grip.index), delta);
            }
        }
    }
    // The arcs' middles as they were, before any vertex moves.
    std::vector<std::pair<std::size_t, Point2>> throughPoints;
    for (const auto& [segment, delta] : arcMids) {
        const Point2 middle =
            std::visit([](const auto& piece) { return piece.pointAt(0.5); }, polyline.segment(segment));
        throughPoints.emplace_back(segment, middle + delta);
    }
    for (const auto& [index, delta] : vertexMoves) {
        polyline.vertices[index].position += delta;
    }
    for (const auto& [segment, through] : throughPoints) {
        auto reshaped = katana::geometry::segmentToArc(polyline, segment, through);
        if (!reshaped) {
            return refused("the arc cannot pass through that point: it is in line with the "
                           "segment's ends");
        }
        polyline = std::move(*reshaped);
    }
    if (auto status = katana::entity::validate(polyline); !status) {
        return refused("the polyline would have no length");
    }
    return writePolyline(entity, polyline);
}

Result<katana::entity::Geometry> dragEllipse(Ellipse2 ellipse, const Moves& moves)
{
    if (moves.has(GripKind::Centre)) {
        ellipse.center += moves.of(GripKind::Centre);
        return katana::entity::Geometry{ellipse};
    }
    const Point2 c = ellipse.center;
    for (std::size_t k = 0; k < 4; ++k) {
        if (!moves.has(GripKind::Quadrant, k)) {
            continue;
        }
        const Point2 p = ellipse.pointAtParameter(katana::math::kHalfPi * static_cast<double>(k)) +
                         moves.of(GripKind::Quadrant, k);
        const double major = ellipse.majorRadius();
        const double minor = ellipse.minorRadius();
        if (k % 2 == 0) {
            const Vec2 axis = (p - c) * (k == 0 ? 1.0 : -1.0);
            const double length = axis.length();
            if (!(length > tol::kGeometric)) {
                return refused("an axis end cannot be dragged onto the centre");
            }
            if (minor <= length) {
                ellipse.majorAxis = axis;
                ellipse.ratio = minor / length;
            } else if (ellipse.isFull()) {
                ellipse.majorAxis = axis.perpendicular().normalized() * minor;
                ellipse.ratio = length / minor;
            } else {
                return refused("on an elliptical arc the major axis cannot be made shorter than "
                               "the minor one");
            }
        } else {
            const Vec2 unitMinor = ellipse.majorAxis.perpendicular() / major;
            const double length = std::abs((p - c).dot(unitMinor));
            if (!(length > tol::kGeometric)) {
                return refused("the minor axis cannot be dragged to nothing");
            }
            if (length <= major) {
                ellipse.ratio = length / major;
            } else if (ellipse.isFull()) {
                ellipse.majorAxis = unitMinor * length;
                ellipse.ratio = major / length;
            } else {
                return refused("on an elliptical arc the minor axis cannot be made longer than "
                               "the major one");
            }
        }
    }
    if (!ellipse.isFull()) {
        double start = ellipse.startParameter;
        double end = ellipse.endParameter();
        if (moves.has(GripKind::End, 0)) {
            start = ellipse.parameterTowards(ellipse.startPoint() + moves.of(GripKind::End, 0));
        }
        if (moves.has(GripKind::End, 1)) {
            end = ellipse.parameterTowards(ellipse.endPoint() + moves.of(GripKind::End, 1));
        }
        const double sweep = katana::math::normalizeAngle(end - start);
        if (!(sweep > tol::kAngular)) {
            return refused("the elliptical arc's ends would meet");
        }
        ellipse.startParameter = start;
        ellipse.sweep = sweep;
    }
    return katana::entity::Geometry{ellipse};
}

} // namespace

Result<Entity> applyGripDrag(const Entity& entity, const GripDrag& drag)
{
    const Moves moves = movesFor(entity.id, drag);
    if (moves.grips.empty()) {
        return entity;
    }
    if (isPolylineEntity(entity)) {
        return dragPolyline(entity, drag, moves);
    }
    Entity out = entity;
    Result<katana::entity::Geometry> geometry = std::visit(
        [&](const auto& shape) -> Result<katana::entity::Geometry> {
            using Shape = std::decay_t<decltype(shape)>;
            if constexpr (std::is_same_v<Shape, katana::entity::PointGeometry>) {
                return katana::entity::Geometry{
                    katana::entity::PointGeometry{shape.position + moves.of(GripKind::Insertion)}};
            } else if constexpr (std::is_same_v<Shape, Segment2>) {
                const Vec2 whole = moves.of(GripKind::Mid);
                return katana::entity::Geometry{
                    Segment2{shape.start + moves.of(GripKind::End, 0) + whole,
                             shape.end + moves.of(GripKind::End, 1) + whole}};
            } else if constexpr (std::is_same_v<Shape, Arc2>) {
                if (moves.has(GripKind::Centre)) {
                    Arc2 moved = shape;
                    moved.center += moves.of(GripKind::Centre);
                    return katana::entity::Geometry{moved};
                }
                const auto rebuilt =
                    Arc2::throughPoints(shape.startPoint() + moves.of(GripKind::End, 0),
                                        shape.midpoint() + moves.of(GripKind::Mid),
                                        shape.endPoint() + moves.of(GripKind::End, 1));
                if (!rebuilt) {
                    return refused("the arc's ends and middle would be in line");
                }
                return katana::entity::Geometry{*rebuilt};
            } else if constexpr (std::is_same_v<Shape, Circle2>) {
                Circle2 moved = shape;
                moved.center += moves.of(GripKind::Centre);
                for (std::size_t k = 0; k < 4; ++k) {
                    if (moves.has(GripKind::Quadrant, k)) {
                        const Point2 p =
                            shape.pointAtAngle(katana::math::kHalfPi * static_cast<double>(k)) +
                            moves.of(GripKind::Quadrant, k);
                        moved.radius = p.distanceTo(moved.center);
                    }
                }
                return katana::entity::Geometry{moved};
            } else if constexpr (std::is_same_v<Shape, katana::entity::TextGeometry>) {
                katana::entity::TextGeometry moved = shape;
                moved.position += moves.of(GripKind::Insertion);
                return katana::entity::Geometry{moved};
            } else if constexpr (std::is_same_v<Shape, Ellipse2>) {
                return dragEllipse(shape, moves);
            } else if constexpr (std::is_same_v<Shape, Spline2>) {
                const bool fit = shape.hasFitPoints();
                std::vector<Point2> points = fit ? shape.fitPoints : shape.controlPoints;
                const GripKind kind = fit ? GripKind::FitPoint : GripKind::ControlPoint;
                for (std::size_t i = 0; i < points.size(); ++i) {
                    points[i] += moves.of(kind, i);
                }
                if (!fit) {
                    Spline2 moved = shape;
                    moved.controlPoints = std::move(points);
                    return katana::entity::Geometry{std::move(moved)};
                }
                auto solved = Spline2::throughPoints(std::move(points), shape.degree);
                if (!solved) {
                    return solved.error();
                }
                return katana::entity::Geometry{std::move(*solved)};
            } else {
                return refused("this kind of object has no grips to drag");
            }
        },
        entity.geometry);
    if (!geometry) {
        return geometry.error();
    }
    if (auto status = katana::entity::validate(*geometry); !status) {
        return refused("the drag would leave " + std::string(katana::entity::toString(entity.type())) +
                       " " + std::to_string(entity.id) + " degenerate: " + status.error().message);
    }
    out.geometry = std::move(*geometry);
    return out;
}

namespace {

std::vector<EntityId> touched(const GripDrag& drag)
{
    std::set<EntityId> ids{drag.grabbed.entity};
    for (const Grip& grip : drag.alsoHot) {
        ids.insert(grip.entity);
    }
    return {ids.begin(), ids.end()};
}

} // namespace

cmd::CommandPtr gripDragCommand(GripDrag drag)
{
    return std::make_unique<cmd::ChangeSetCommand>(
        "GRIP_EDIT", [drag = std::move(drag)](const cmd::CommandContext& context) -> Result<cmd::ChangeSet> {
            cmd::ChangeSet changes;
            for (const EntityId id : touched(drag)) {
                const Entity* entity = context.model.entities.find(id);
                if (entity == nullptr) {
                    return makeError(ErrorCode::NotFound, "there is no entity " + std::to_string(id));
                }
                auto edited = applyGripDrag(*entity, drag);
                if (!edited) {
                    return edited.error();
                }
                changes.modify.push_back(std::move(*edited));
            }
            return changes;
        });
}

cmd::CommandPtr deleteHotVertices(std::vector<Grip> hot)
{
    return std::make_unique<cmd::ChangeSetCommand>(
        "VERTEX_DELETE", [hot = std::move(hot)](const cmd::CommandContext& context) -> Result<cmd::ChangeSet> {
            std::map<EntityId, std::vector<std::size_t>> byEntity;
            for (const Grip& grip : hot) {
                if (grip.kind == GripKind::Vertex) {
                    byEntity[grip.entity].push_back(grip.index);
                }
            }
            if (byEntity.empty()) {
                return makeError(ErrorCode::InvalidArgument,
                                 "no vertex grip is hot; click a polyline's vertex grip first");
            }
            cmd::ChangeSet changes;
            for (const auto& [id, indices] : byEntity) {
                const Entity* entity = context.model.entities.find(id);
                if (entity == nullptr) {
                    return makeError(ErrorCode::NotFound, "there is no entity " + std::to_string(id));
                }
                const auto polyline = readPolyline(*entity);
                if (!polyline) {
                    return makeError(ErrorCode::InvalidArgument,
                                     "entity " + std::to_string(id) + " is not a polyline");
                }
                auto remaining = katana::geometry::deleteVertices(*polyline, indices);
                if (!remaining) {
                    return remaining.error();
                }
                auto written = writePolyline(*entity, *remaining);
                if (!written) {
                    return written.error();
                }
                changes.modify.push_back(std::move(*written));
            }
            return changes;
        });
}

std::vector<katana::entity::Geometry> gripPreview(const Document& document, const GripDrag& drag)
{
    std::vector<katana::entity::Geometry> out;
    for (const EntityId id : touched(drag)) {
        const Entity* entity = document.model().entities.find(id);
        if (entity == nullptr) {
            continue;
        }
        if (auto edited = applyGripDrag(*entity, drag)) {
            out.push_back(std::move(edited->geometry));
        }
    }
    return out;
}

} // namespace katana::cad
