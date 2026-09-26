#include "katana/commands/entity_commands.hpp"

#include "katana/entity/curve_pieces.hpp"

#include <cmath>
#include <limits>
#include <map>
#include <utility>

#include "katana/commands/change_set.hpp"
#include "katana/entity/entity_geometry.hpp"
#include "katana/geometry/editing.hpp"

namespace katana::commands {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;
using katana::entity::Entity;
using katana::entity::Geometry;
using katana::geometry::Arc2;
using katana::geometry::Circle2;
using katana::geometry::Curve2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;
using katana::math::Mat3;

namespace {

CommandPtr makeCommand(std::string name, ChangeSetCommand::Builder builder,
                       bool destructive = false)
{
    return std::make_unique<ChangeSetCommand>(std::move(name), std::move(builder), destructive);
}

std::string idContext(EntityId id)
{
    return "id=" + std::to_string(id);
}

Result<Entity> findEntity(const CommandContext& context, EntityId id)
{
    const Entity* entity = context.model.entities.find(id);
    if (entity == nullptr) {
        return makeError(ErrorCode::NotFound, "entity does not exist", idContext(id));
    }
    return *entity;
}

Status requireSelection(const std::vector<EntityId>& ids)
{
    if (ids.empty()) {
        return makeError(ErrorCode::InvalidArgument, "no entities were given");
    }
    return {};
}

CommandPtr createOne(std::string name, Geometry geometry, EntityAttributes attributes)
{
    Entity entity;
    entity.geometry = std::move(geometry);
    entity.layer = std::move(attributes.layer);
    entity.style = std::move(attributes.style);
    entity.color = attributes.color;
    return makeCommand(std::move(name),
                       [entity = std::move(entity)](const CommandContext&) -> Result<ChangeSet> {
                           ChangeSet changes;
                           changes.add.push_back(entity);
                           return changes;
                       });
}

// Command that rewrites each selected entity through `edit`.
template <typename Edit>
CommandPtr modifyEach(std::string name, std::vector<EntityId> ids, Edit edit)
{
    return makeCommand(
        std::move(name),
        [ids = std::move(ids), edit = std::move(edit)](
            const CommandContext& context) -> Result<ChangeSet> {
            if (auto status = requireSelection(ids); !status) {
                return status.error();
            }
            ChangeSet changes;
            for (const EntityId id : ids) {
                auto entity = findEntity(context, id);
                if (!entity) {
                    return entity.error();
                }
                if (auto status = edit(*entity); !status) {
                    return status.error();
                }
                changes.modify.push_back(std::move(*entity));
            }
            return changes;
        });
}

Status applyTransform(Entity& entity, const Mat3& transform)
{
    auto moved = katana::entity::transformed(entity.geometry, transform);
    if (!moved) {
        return makeError(moved.error().code, moved.error().message, idContext(entity.id));
    }
    entity.geometry = std::move(*moved);
    return {};
}

// Adds transformed duplicates of the selection; the originals stay.
// An annotation copied WITH the entity it refers to - a leader's tip, a
// dimension's points, a label's target - refers to that entity's copy, so a
// pit copied with its callout gets a callout of its own rather than a second
// one of the original (docs/annotation.md, "Associativity"). A reference to
// an entity not copied is kept: the copy is another annotation of it.
void referToCopies(Entity& entity, const std::map<EntityId, EntityId>& copyOf)
{
    const auto remap = [&](std::uint64_t& id) {
        if (const auto found = copyOf.find(id); found != copyOf.end()) {
            id = found->second;
        }
    };
    if (auto* dimension = std::get_if<katana::entity::DimensionGeometry>(&entity.geometry)) {
        for (katana::entity::AnchorRef* ref :
             {&dimension->startRef, &dimension->endRef, &dimension->vertexRef}) {
            if (ref->associated()) {
                remap(ref->entity);
            }
        }
    } else if (auto* leader = std::get_if<katana::entity::LeaderGeometry>(&entity.geometry)) {
        if (leader->tipRef.associated()) {
            remap(leader->tipRef.entity);
        }
    } else if (auto* label = std::get_if<katana::entity::LabelGeometry>(&entity.geometry)) {
        if (label->target != 0) {
            remap(label->target);
        }
    }
}

CommandPtr duplicateEach(std::string name, std::vector<EntityId> ids,
                         std::vector<Mat3> transforms)
{
    return makeCommand(std::move(name),
                       [ids = std::move(ids), transforms = std::move(transforms)](
                           const CommandContext& context) -> Result<ChangeSet> {
                           if (auto status = requireSelection(ids); !status) {
                               return status.error();
                           }
                           // The ids the copies will have: an add is given the
                           // next id, in the order the change set lists them
                           // (EntityDatabase::add), and a redo puts each back
                           // under the id it had.
                           const EntityId first = context.model.entities.nextId();
                           ChangeSet changes;
                           for (const Mat3& transform : transforms) {
                               std::map<EntityId, EntityId> copyOf;
                               const EntityId base = first + changes.add.size();
                               for (std::size_t k = 0; k < ids.size(); ++k) {
                                   copyOf[ids[k]] = base + k;
                               }
                               for (const EntityId id : ids) {
                                   auto entity = findEntity(context, id);
                                   if (!entity) {
                                       return entity.error();
                                   }
                                   if (auto status = applyTransform(*entity, transform); !status) {
                                       return status.error();
                                   }
                                   referToCopies(*entity, copyOf);
                                   changes.add.push_back(std::move(*entity));
                               }
                           }
                           return changes;
                       });
}

// ---- curve editing helpers ---------------------------------------------------------

Result<Curve2> asCurve(const Entity& entity)
{
    if (const auto* segment = std::get_if<Segment2>(&entity.geometry)) {
        return Curve2{*segment};
    }
    if (const auto* arc = std::get_if<Arc2>(&entity.geometry)) {
        return Curve2{*arc};
    }
    if (const auto* circle = std::get_if<Circle2>(&entity.geometry)) {
        return Curve2{*circle};
    }
    return makeError(ErrorCode::Unsupported,
                     "only lines, arcs and circles can be edited this way",
                     idContext(entity.id) + " type=" + std::string(toString(entity.type())));
}

Geometry asGeometry(const Curve2& curve)
{
    return std::visit([](const auto& c) -> Geometry { return c; }, curve);
}

// Curves that act as cutting edges / boundaries. Polylines contribute segments.
Result<std::vector<Curve2>> edgeCurves(const CommandContext& context,
                                       const std::vector<EntityId>& ids)
{
    std::vector<Curve2> curves;
    for (const EntityId id : ids) {
        auto entity = findEntity(context, id);
        if (!entity) {
            return entity.error();
        }
        if (const auto* polyline = std::get_if<Polyline2>(&entity->geometry)) {
            for (std::size_t i = 0; i < polyline->segmentCount(); ++i) {
                curves.emplace_back(polyline->segment(i));
            }
            continue;
        }
        // A curve polyline, an ellipse and a spline cut and bound through
        // their pieces (entity/curve_pieces.hpp).
        if (std::holds_alternative<katana::geometry::CurvePolyline2>(entity->geometry) ||
            std::holds_alternative<katana::geometry::Ellipse2>(entity->geometry) ||
            std::holds_alternative<katana::geometry::Spline2>(entity->geometry)) {
            for (auto& piece : katana::entity::curvePieces(entity->geometry)) {
                curves.push_back(std::move(piece));
            }
            continue;
        }
        auto curve = asCurve(*entity);
        if (!curve) {
            return curve.error();
        }
        curves.push_back(std::move(*curve));
    }
    return curves;
}

Result<Segment2> asSegment(const Entity& entity)
{
    if (const auto* segment = std::get_if<Segment2>(&entity.geometry)) {
        return *segment;
    }
    return makeError(ErrorCode::Unsupported, "only lines can be filleted or chamfered",
                     idContext(entity.id));
}

} // namespace

// ---- creation ----------------------------------------------------------------------------

CommandPtr createPoint(const Point2& position, EntityAttributes attributes)
{
    return createOne("CREATE_POINT", katana::entity::PointGeometry{position},
                     std::move(attributes));
}

CommandPtr createLine(const Point2& start, const Point2& end, EntityAttributes attributes)
{
    return createOne("CREATE_LINE", Segment2{start, end}, std::move(attributes));
}

CommandPtr createCircle(const Point2& center, double radius, EntityAttributes attributes)
{
    return createOne("CREATE_CIRCLE", Circle2{center, radius}, std::move(attributes));
}

CommandPtr createArc(const Arc2& arc, EntityAttributes attributes)
{
    return createOne("CREATE_ARC", arc, std::move(attributes));
}

CommandPtr createPolyline(Polyline2 polyline, EntityAttributes attributes)
{
    return createOne("CREATE_POLYLINE", std::move(polyline), std::move(attributes));
}

CommandPtr createText(katana::entity::TextGeometry text, EntityAttributes attributes)
{
    return createOne("CREATE_TEXT", std::move(text), std::move(attributes));
}

CommandPtr createDimension(katana::entity::DimensionGeometry dimension, EntityAttributes attributes)
{
    return createOne("CREATE_DIMENSION", std::move(dimension), std::move(attributes));
}

CommandPtr createEntities(std::vector<Entity> entities)
{
    // The copy here is deliberate and cannot become a move. A builder is a pure
    // function of the model (command.hpp: "validate() has no side effects and
    // may be called at any time"), and validate() really is called twice on the
    // first command of a Transaction - once by Transaction::validate and again
    // by Transaction::execute. A builder that moved its captured entities out
    // would hand the second call an empty set, and the command would be
    // rejected as one that "would not change anything". ChangeSetCommand keeps
    // what validate() built and hands it to execute(), so this copy is made once
    // per command rather than once per call, and execute() then moves the
    // entities into the database instead of copying them again.
    return makeCommand("CREATE_ENTITIES",
                       [entities = std::move(entities)](const CommandContext&) -> Result<ChangeSet> {
                           ChangeSet changes;
                           changes.add = entities;
                           return changes;
                       });
}

// ---- removal / transformation ------------------------------------------------------------

CommandPtr deleteEntities(std::vector<EntityId> ids)
{
    return makeCommand(
        "DELETE",
        [ids = std::move(ids)](const CommandContext&) -> Result<ChangeSet> {
            if (auto status = requireSelection(ids); !status) {
                return status.error();
            }
            ChangeSet changes;
            changes.remove = ids;
            return changes;
        },
        /*destructive=*/true);
}

CommandPtr transformEntities(std::string name, std::vector<EntityId> ids, const Mat3& transform)
{
    return modifyEach(std::move(name), std::move(ids),
                      [transform](Entity& entity) { return applyTransform(entity, transform); });
}

CommandPtr moveEntities(std::vector<EntityId> ids, const Vec2& delta)
{
    return transformEntities("MOVE", std::move(ids), Mat3::translation(delta));
}

CommandPtr rotateEntities(std::vector<EntityId> ids, const Point2& center, double radians)
{
    return transformEntities("ROTATE", std::move(ids), Mat3::rotationAbout(center, radians));
}

CommandPtr scaleEntities(std::vector<EntityId> ids, const Point2& center, double factor)
{
    if (!(std::isfinite(factor) && factor > 0.0)) {
        return makeCommand("SCALE", [factor](const CommandContext&) -> Result<ChangeSet> {
            return makeError(ErrorCode::InvalidArgument, "scale factor must be positive",
                             std::to_string(factor));
        });
    }
    return transformEntities("SCALE", std::move(ids), Mat3::scalingAbout(center, factor, factor));
}

CommandPtr mirrorEntities(std::vector<EntityId> ids, const Point2& a, const Point2& b,
                          bool keepOriginal)
{
    if (a.distanceTo(b) <= katana::math::tolerance::kGeometric) {
        return makeCommand("MIRROR", [](const CommandContext&) -> Result<ChangeSet> {
            return makeError(ErrorCode::InvalidArgument,
                             "mirror line needs two distinct points");
        });
    }
    const Mat3 reflection = Mat3::reflection(a, b - a);
    if (keepOriginal) {
        return duplicateEach("MIRROR", std::move(ids), {reflection});
    }
    return transformEntities("MIRROR", std::move(ids), reflection);
}

CommandPtr copyEntities(std::vector<EntityId> ids, const Vec2& delta)
{
    return duplicateEach("COPY", std::move(ids), {Mat3::translation(delta)});
}

CommandPtr duplicateEntities(std::string name, std::vector<EntityId> ids,
                             std::vector<Mat3> transforms)
{
    if (transforms.empty()) {
        return makeCommand(std::move(name), [](const CommandContext&) -> Result<ChangeSet> {
            return makeError(ErrorCode::InvalidArgument, "nothing to copy");
        });
    }
    return duplicateEach(std::move(name), std::move(ids), std::move(transforms));
}

CommandPtr arrayEntities(std::vector<EntityId> ids, int rows, int columns, const Vec2& spacing)
{
    constexpr int kMaximumCopies = 100000; // guards against a runaway typo, not a design limit
    if (rows < 1 || columns < 1 || rows * static_cast<long long>(columns) > kMaximumCopies ||
        (rows == 1 && columns == 1)) {
        return makeCommand("ARRAY", [rows, columns](const CommandContext&) -> Result<ChangeSet> {
            return makeError(ErrorCode::InvalidArgument,
                             "array needs at least 1x2 cells and at most 100000",
                             std::to_string(rows) + "x" + std::to_string(columns));
        });
    }
    std::vector<Mat3> transforms;
    for (int row = 0; row < rows; ++row) {
        for (int column = 0; column < columns; ++column) {
            if (row != 0 || column != 0) {
                transforms.push_back(Mat3::translation(Vec2(spacing.x * column, spacing.y * row)));
            }
        }
    }
    return duplicateEach("ARRAY", std::move(ids), std::move(transforms));
}

// ---- curve editing --------------------------------------------------------------------------

CommandPtr trimEntity(EntityId target, std::vector<EntityId> cutters, const Point2& pick)
{
    return makeCommand(
        "TRIM",
        [target, cutters = std::move(cutters), pick](
            const CommandContext& context) -> Result<ChangeSet> {
            auto entity = findEntity(context, target);
            if (!entity) {
                return entity.error();
            }
            auto curve = asCurve(*entity);
            if (!curve) {
                return curve.error();
            }
            auto edges = edgeCurves(context, cutters);
            if (!edges) {
                return edges.error();
            }
            auto trimmed = katana::geometry::trim(*curve, *edges, pick);
            if (!trimmed) {
                return trimmed.error();
            }
            ChangeSet changes;
            if (trimmed->remaining.empty()) {
                changes.remove.push_back(target);
                return changes;
            }
            // The first piece keeps the identity; further pieces are new entities
            // inheriting every attribute of the original.
            Entity first = *entity;
            first.geometry = asGeometry(trimmed->remaining.front());
            changes.modify.push_back(std::move(first));
            for (std::size_t i = 1; i < trimmed->remaining.size(); ++i) {
                Entity piece = *entity;
                piece.geometry = asGeometry(trimmed->remaining[i]);
                changes.add.push_back(std::move(piece));
            }
            return changes;
        });
}

CommandPtr extendEntity(EntityId target, std::vector<EntityId> boundaries, const Point2& pick)
{
    return makeCommand("EXTEND",
                       [target, boundaries = std::move(boundaries), pick](
                           const CommandContext& context) -> Result<ChangeSet> {
                           auto entity = findEntity(context, target);
                           if (!entity) {
                               return entity.error();
                           }
                           auto curve = asCurve(*entity);
                           if (!curve) {
                               return curve.error();
                           }
                           auto edges = edgeCurves(context, boundaries);
                           if (!edges) {
                               return edges.error();
                           }
                           auto extended = katana::geometry::extend(*curve, *edges, pick);
                           if (!extended) {
                               return extended.error();
                           }
                           ChangeSet changes;
                           entity->geometry = asGeometry(*extended);
                           changes.modify.push_back(std::move(*entity));
                           return changes;
                       });
}

CommandPtr offsetEntity(EntityId source, double distance, const Point2& side)
{
    return makeCommand(
        "OFFSET", [source, distance, side](const CommandContext& context) -> Result<ChangeSet> {
            auto entity = findEntity(context, source);
            if (!entity) {
                return entity.error();
            }
            ChangeSet changes;
            if (const auto* polyline = std::get_if<Polyline2>(&entity->geometry)) {
                // Side of the nearest segment decides the sign (left is positive).
                double sign = 1.0;
                double best = std::numeric_limits<double>::infinity();
                for (std::size_t i = 0; i < polyline->segmentCount(); ++i) {
                    const Segment2 segment = polyline->segment(i);
                    const double d = segment.distanceTo(side);
                    if (d < best && !segment.isDegenerate()) {
                        best = d;
                        const katana::geometry::Line2 line{segment.start, segment.delta()};
                        sign = line.signedDistanceTo(side) >= 0.0 ? 1.0 : -1.0;
                    }
                }
                auto moved = katana::geometry::offset(*polyline, sign * std::abs(distance));
                if (!moved) {
                    return moved.error();
                }
                entity->geometry = std::move(*moved);
            } else if (const auto* curved =
                           std::get_if<katana::geometry::CurvePolyline2>(&entity->geometry)) {
                // The side of the nearest piece decides the sign: left of a
                // line, or - on an arc - inside a counter-clockwise one.
                const auto near = curved->nearest(side);
                if (!near) {
                    return makeError(ErrorCode::InvalidGeometry, "the polyline has no vertices");
                }
                double sign = 1.0;
                const auto piece = curved->segment(near->segment);
                if (const auto* line = std::get_if<Segment2>(&piece)) {
                    sign = katana::geometry::Line2{line->start, line->delta()}.signedDistanceTo(side) >= 0.0
                               ? 1.0
                               : -1.0;
                } else {
                    const auto& arc = std::get<Arc2>(piece);
                    const bool inside = side.distanceTo(arc.center) < arc.radius;
                    sign = (inside == (arc.sweep > 0.0)) ? 1.0 : -1.0;
                }
                auto moved = katana::geometry::offset(*curved, sign * std::abs(distance));
                if (!moved) {
                    return moved.error();
                }
                entity->geometry = std::move(*moved);
            } else {
                auto curve = asCurve(*entity);
                if (!curve) {
                    return curve.error();
                }
                auto moved = katana::geometry::offsetTowards(*curve, distance, side);
                if (!moved) {
                    return moved.error();
                }
                entity->geometry = asGeometry(*moved);
            }
            changes.add.push_back(std::move(*entity));
            return changes;
        });
}

CommandPtr filletEntities(EntityId first, EntityId second, double radius)
{
    return makeCommand(
        "FILLET", [first, second, radius](const CommandContext& context) -> Result<ChangeSet> {
            if (first == second) {
                return makeError(ErrorCode::InvalidArgument, "fillet needs two different lines");
            }
            auto a = findEntity(context, first);
            auto b = findEntity(context, second);
            if (!a) {
                return a.error();
            }
            if (!b) {
                return b.error();
            }
            auto segmentA = asSegment(*a);
            auto segmentB = asSegment(*b);
            if (!segmentA) {
                return segmentA.error();
            }
            if (!segmentB) {
                return segmentB.error();
            }
            auto result = katana::geometry::fillet(*segmentA, *segmentB, radius);
            if (!result) {
                return result.error();
            }
            ChangeSet changes;
            if (result->arc) {
                Entity arc = *a; // the fillet arc inherits the first line's attributes
                arc.geometry = *result->arc;
                changes.add.push_back(std::move(arc));
            }
            a->geometry = result->first;
            b->geometry = result->second;
            changes.modify.push_back(std::move(*a));
            changes.modify.push_back(std::move(*b));
            return changes;
        });
}

CommandPtr chamferEntities(EntityId first, EntityId second, double distanceFirst,
                           double distanceSecond)
{
    return makeCommand(
        "CHAMFER",
        [first, second, distanceFirst, distanceSecond](
            const CommandContext& context) -> Result<ChangeSet> {
            if (first == second) {
                return makeError(ErrorCode::InvalidArgument, "chamfer needs two different lines");
            }
            auto a = findEntity(context, first);
            auto b = findEntity(context, second);
            if (!a) {
                return a.error();
            }
            if (!b) {
                return b.error();
            }
            auto segmentA = asSegment(*a);
            auto segmentB = asSegment(*b);
            if (!segmentA) {
                return segmentA.error();
            }
            if (!segmentB) {
                return segmentB.error();
            }
            auto result =
                katana::geometry::chamfer(*segmentA, *segmentB, distanceFirst, distanceSecond);
            if (!result) {
                return result.error();
            }
            ChangeSet changes;
            if (result->bevel) {
                Entity bevel = *a;
                bevel.geometry = *result->bevel;
                changes.add.push_back(std::move(bevel));
            }
            a->geometry = result->first;
            b->geometry = result->second;
            changes.modify.push_back(std::move(*a));
            changes.modify.push_back(std::move(*b));
            return changes;
        });
}

// ---- attributes -------------------------------------------------------------------------------

CommandPtr setEntityLayer(std::vector<EntityId> ids, std::string layer)
{
    return modifyEach("SET_LAYER", std::move(ids), [layer = std::move(layer)](Entity& entity) {
        entity.layer = layer;
        return Status{};
    });
}

CommandPtr setEntityColor(std::vector<EntityId> ids, std::optional<katana::entity::Color> color)
{
    return modifyEach("SET_COLOR", std::move(ids), [color](Entity& entity) {
        entity.color = color;
        return Status{};
    });
}

CommandPtr setEntityStyle(std::vector<EntityId> ids, std::string style)
{
    return modifyEach("SET_STYLE", std::move(ids), [style = std::move(style)](Entity& entity) {
        entity.style = style;
        return Status{};
    });
}

CommandPtr setEntityVisible(std::vector<EntityId> ids, bool visible)
{
    return modifyEach("SET_VISIBLE", std::move(ids), [visible](Entity& entity) {
        entity.visible = visible;
        return Status{};
    });
}

CommandPtr setEntityProperty(std::vector<EntityId> ids, std::string key,
                             katana::entity::PropertyValue value)
{
    return modifyEach("SET_PROPERTY", std::move(ids),
                      [key = std::move(key), value = std::move(value)](Entity& entity) -> Status {
                          if (key.empty()) {
                              return makeError(ErrorCode::InvalidArgument,
                                               "property name is empty");
                          }
                          entity.properties[key] = value;
                          return {};
                      });
}

CommandPtr removeEntityProperty(std::vector<EntityId> ids, std::string key)
{
    return modifyEach("REMOVE_PROPERTY", std::move(ids),
                      [key = std::move(key)](Entity& entity) -> Status {
                          if (entity.properties.erase(key) == 0) {
                              return makeError(ErrorCode::NotFound,
                                               "entity does not have this property",
                                               idContext(entity.id) + " key=" + key);
                          }
                          return {};
                      });
}

CommandPtr renameEntityProperty(std::vector<EntityId> ids, std::string from, std::string to)
{
    return modifyEach("RENAME_PROPERTY", std::move(ids),
                      [from = std::move(from), to = std::move(to)](Entity& entity) -> Status {
                          if (to.empty()) {
                              return makeError(ErrorCode::InvalidArgument,
                                               "property name is empty");
                          }
                          const auto found = entity.properties.find(from);
                          if (found == entity.properties.end()) {
                              return makeError(ErrorCode::NotFound,
                                               "entity does not have this property",
                                               idContext(entity.id) + " key=" + from);
                          }
                          if (from != to && entity.properties.contains(to)) {
                              return makeError(ErrorCode::AlreadyExists,
                                               "the entity already has a property of that name",
                                               idContext(entity.id) + " key=" + to);
                          }
                          katana::entity::PropertyValue value = found->second;
                          entity.properties.erase(found);
                          entity.properties[to] = std::move(value);
                          return {};
                      });
}

CommandPtr setEntityGeometry(EntityId id, Geometry geometry)
{
    return modifyEach("SET_GEOMETRY", {id}, [geometry = std::move(geometry)](Entity& entity) {
        entity.geometry = geometry;
        return Status{};
    });
}

} // namespace katana::commands
