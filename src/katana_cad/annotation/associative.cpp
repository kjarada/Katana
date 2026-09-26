#include "katana/cad/annotation/associative.hpp"

#include <cmath>
#include <memory>
#include <optional>
#include <set>
#include <utility>

#include "katana/entity/anchor.hpp"
#include "katana/entity/entity_geometry.hpp"
#include "katana/entity/label_values.hpp"
#include "katana/entity/leader_values.hpp"
#include "katana/math/numerics.hpp"

namespace katana::cad::annotation {

using katana::commands::ChangeSet;
using katana::commands::CommandContext;
using katana::commands::CommandPtr;
using katana::core::Status;
using katana::entity::AnchorRef;
using katana::entity::Entity;
using katana::entity::Model;
using katana::geometry::Point2;
using katana::geometry::Vec2;

namespace {

// Follows one reference: the point it names now, or the point unchanged
// with the reference dropped when its entity is gone. `changed` is set when
// either moved.
void follow(const Model& model, AnchorRef& ref, Point2& point, bool& changed)
{
    if (!ref.associated()) {
        return;
    }
    const Entity* target = model.entities.find(ref.entity);
    if (target == nullptr) {
        ref = AnchorRef{};
        changed = true;
        return;
    }
    if (const auto resolved = katana::entity::resolveAnchor(*target, ref)) {
        if (!(*resolved == point)) {
            point = *resolved;
            changed = true;
        }
    }
}

std::optional<katana::entity::DimensionGeometry>
followDimension(const Model& model, const katana::entity::DimensionGeometry& dimension)
{
    using katana::entity::DimensionKind;
    if (!dimension.startRef.associated() && !dimension.endRef.associated() &&
        !dimension.vertexRef.associated()) {
        return std::nullopt;
    }
    katana::entity::DimensionGeometry result = dimension;
    bool changed = false;
    const bool radial =
        dimension.kind == DimensionKind::Radius || dimension.kind == DimensionKind::Diameter;
    // A radial dimension on a circle or an arc keeps its direction from the
    // centre and takes the curve's radius: the point it was put at is not an
    // anchor the curve offers, but "the curve, in this direction" is.
    if (radial && dimension.vertexRef.associated() && !dimension.startRef.associated()) {
        if (const Entity* curve = model.entities.find(dimension.vertexRef.entity)) {
            std::optional<std::pair<Point2, double>> circle;
            if (const auto* c = std::get_if<katana::geometry::Circle2>(&curve->geometry)) {
                circle = std::pair{c->center, c->radius};
            } else if (const auto* a = std::get_if<katana::geometry::Arc2>(&curve->geometry)) {
                circle = std::pair{a->center, a->radius};
            }
            if (circle) {
                Vec2 direction = dimension.start - dimension.vertex;
                direction = direction.length() > 0.0 ? direction.normalized() : Vec2(1.0, 0.0);
                const Point2 start = circle->first + direction * circle->second;
                if (!(start == result.start) || !(circle->first == result.vertex)) {
                    result.start = start;
                    result.vertex = circle->first;
                    changed = true;
                }
            } else {
                follow(model, result.vertexRef, result.vertex, changed);
            }
        } else {
            result.vertexRef = AnchorRef{};
            changed = true;
        }
    } else {
        follow(model, result.vertexRef, result.vertex, changed);
    }
    follow(model, result.startRef, result.start, changed);
    follow(model, result.endRef, result.end, changed);
    // An angle between two lines (dimension_build.hpp, angularBetweenLines)
    // has its vertex where the lines meet, which no anchor names: it is
    // worked out again from the lines whenever they move.
    if (dimension.kind == DimensionKind::Angular && !result.vertexRef.associated() &&
        result.startRef.associated() && result.endRef.associated()) {
        const Entity* first = model.entities.find(result.startRef.entity);
        const Entity* second = model.entities.find(result.endRef.entity);
        const auto* a = first != nullptr ? std::get_if<katana::geometry::Segment2>(&first->geometry)
                                         : nullptr;
        const auto* b = second != nullptr
                            ? std::get_if<katana::geometry::Segment2>(&second->geometry)
                            : nullptr;
        if (a != nullptr && b != nullptr) {
            const Vec2 da = a->end - a->start;
            const Vec2 db = b->end - b->start;
            const double denominator = da.cross(db);
            if (std::abs(denominator) >
                katana::math::tolerance::kAngular * da.length() * db.length()) {
                const Point2 vertex =
                    a->start + da * ((b->start - a->start).cross(db) / denominator);
                if (!(vertex == result.vertex)) {
                    result.vertex = vertex;
                    changed = true;
                }
            }
        }
    }
    if (!changed) {
        return std::nullopt;
    }
    // Geometry the dimension cannot measure - two anchors brought together -
    // keeps the last good shape, only dropping references that are gone.
    if (!katana::entity::validate(result)) {
        katana::entity::DimensionGeometry kept = dimension;
        for (auto [ref, now] : {std::pair{&kept.startRef, &result.startRef},
                                std::pair{&kept.endRef, &result.endRef},
                                std::pair{&kept.vertexRef, &result.vertexRef}}) {
            if (!now->associated()) {
                *ref = AnchorRef{};
            }
        }
        return kept == dimension ? std::nullopt : std::optional(kept);
    }
    return result;
}

// A smart leader whose target this update removes - a label, or another
// smart leader - goes too, however long the chain: removed ids are gathered
// until a pass adds none, and a leader that was to follow a target now
// removed is removed instead. Only smart leaders chain, since only they read
// an annotation as their target; the extra passes run only when the first
// removed something.
void removeSmartLeadersOfRemoved(const Model& model, ChangeSet& changes)
{
    if (changes.remove.empty()) {
        return;
    }
    std::set<katana::entity::EntityId> removed(changes.remove.begin(), changes.remove.end());
    bool grew = true;
    while (grew) {
        grew = false;
        model.entities.forEach([&](const Entity& entity) {
            const auto* leader = std::get_if<katana::entity::LeaderGeometry>(&entity.geometry);
            if (leader == nullptr || !katana::entity::isSmart(*leader) ||
                !leader->tipRef.associated() || removed.contains(entity.id) ||
                !removed.contains(leader->tipRef.entity) ||
                model.layers.resolve(entity.layer).locked) {
                return;
            }
            removed.insert(entity.id);
            changes.remove.push_back(entity.id);
            grew = true;
        });
    }
    std::erase_if(changes.modify,
                  [&](const Entity& entity) { return removed.contains(entity.id); });
}

} // namespace

ChangeSet associativeChanges(const Model& model)
{
    ChangeSet changes;
    model.entities.forEach([&](const Entity& entity) {
        const bool follows =
            std::holds_alternative<katana::entity::LabelGeometry>(entity.geometry) ||
            std::holds_alternative<katana::entity::DimensionGeometry>(entity.geometry) ||
            std::holds_alternative<katana::entity::LeaderGeometry>(entity.geometry);
        if (!follows || model.layers.resolve(entity.layer).locked) {
            return;
        }
        if (const auto* label = std::get_if<katana::entity::LabelGeometry>(&entity.geometry)) {
            const bool targetGone =
                label->target != 0 ? !model.entities.contains(label->target)
                                   : !model.alignments.contains(label->alignment);
            if (targetGone) {
                changes.remove.push_back(entity.id);
                return;
            }
            const katana::entity::LabelStyle* style = model.labelStyles.find(label->style);
            if (style == nullptr) {
                return;
            }
            const auto anchor = katana::entity::labelAnchor(model, *label, *style);
            if (!anchor || *anchor == label->anchor) {
                return;
            }
            Entity moved = entity;
            auto& updated = std::get<katana::entity::LabelGeometry>(moved.geometry);
            if (updated.position) {
                *updated.position = *updated.position + (*anchor - label->anchor);
            }
            updated.anchor = *anchor;
            changes.modify.push_back(std::move(moved));
            return;
        }
        if (const auto* dimension =
                std::get_if<katana::entity::DimensionGeometry>(&entity.geometry)) {
            if (auto followed = followDimension(model, *dimension)) {
                Entity moved = entity;
                moved.geometry = std::move(*followed);
                changes.modify.push_back(std::move(moved));
            }
            return;
        }
        const auto& leader = std::get<katana::entity::LeaderGeometry>(entity.geometry);
        if (!leader.tipRef.associated()) {
            return;
        }
        // A smart leader's note is read off its target (docs/annotation.md,
        // "Smart leaders"): it goes with it, as a label does, and comes back
        // with it on undo. A plain one stays and lets the reference go.
        if (katana::entity::isSmart(leader) && !model.entities.contains(leader.tipRef.entity)) {
            changes.remove.push_back(entity.id);
            return;
        }
        katana::entity::LeaderGeometry followed = leader;
        bool changed = false;
        follow(model, followed.tipRef, followed.vertices.front(), changed);
        if (changed) {
            if (!katana::entity::validate(followed)) {
                // A tip brought onto the next vertex: keep the leader as it was.
                followed = leader;
                followed.tipRef = model.entities.contains(leader.tipRef.entity) ? leader.tipRef
                                                                                : AnchorRef{};
                if (followed == leader) {
                    return;
                }
            }
            Entity moved = entity;
            moved.geometry = std::move(followed);
            changes.modify.push_back(std::move(moved));
        }
    });
    removeSmartLeadersOfRemoved(model, changes);
    return changes;
}

namespace {

class AssociativeCommand final : public katana::commands::Command {
  public:
    explicit AssociativeCommand(CommandPtr inner) : inner_(std::move(inner)) {}

    [[nodiscard]] std::string_view name() const override { return inner_->name(); }
    [[nodiscard]] bool isDestructive() const override { return inner_->isDestructive(); }
    [[nodiscard]] Status validate(const CommandContext& context) const override
    {
        return inner_->validate(context);
    }
    [[nodiscard]] Status execute(CommandContext& context) override
    {
        if (auto status = inner_->execute(context); !status) {
            return status;
        }
        ChangeSet changes = associativeChanges(context.model);
        if (changes.empty()) {
            return {};
        }
        follow_ = std::make_unique<katana::commands::ChangeSetCommand>(
            "ASSOCIATIVE_UPDATE",
            [changes = std::move(changes)](const CommandContext&)
                -> katana::core::Result<ChangeSet> { return changes; });
        if (auto status = follow_->execute(context); !status) {
            follow_.reset();
            (void)inner_->undo(context);
            return status;
        }
        return {};
    }
    [[nodiscard]] Status undo(CommandContext& context) override
    {
        if (follow_) {
            if (auto status = follow_->undo(context); !status) {
                return status;
            }
        }
        return inner_->undo(context);
    }
    [[nodiscard]] Status redo(CommandContext& context) override
    {
        if (auto status = inner_->redo(context); !status) {
            return status;
        }
        return follow_ ? follow_->redo(context) : Status{};
    }
    [[nodiscard]] std::vector<katana::entity::EntityId> createdEntities() const override
    {
        return inner_->createdEntities();
    }

  private:
    CommandPtr inner_;
    std::unique_ptr<katana::commands::ChangeSetCommand> follow_;
};

} // namespace

CommandPtr withAssociativeUpdate(CommandPtr command)
{
    return std::make_unique<AssociativeCommand>(std::move(command));
}

} // namespace katana::cad::annotation
