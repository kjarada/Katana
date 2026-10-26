#include "katana/cad/annotation/leader_edit.hpp"

#include <algorithm>
#include <charconv>
#include <cmath>
#include <iterator>
#include <limits>
#include <memory>
#include <set>
#include <utility>

#include "katana/cad/survey_coding.hpp"
#include "katana/commands/change_set.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/anchor.hpp"
#include "katana/entity/annotation.hpp"
#include "katana/entity/entity_geometry.hpp"
#include "katana/entity/leader_values.hpp"

namespace katana::cad::annotation {

using katana::commands::ChangeSet;
using katana::commands::CommandPtr;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;
using katana::entity::AnchorPoint;
using katana::entity::AnchorRef;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::entity::LeaderGeometry;
using katana::entity::Model;
using katana::geometry::Point2;
using katana::geometry::Vec2;
namespace cmd = katana::commands;

namespace {

// `changes` as one step named `name`.
CommandPtr commandOf(const char* name, ChangeSet changes)
{
    return std::make_unique<cmd::ChangeSetCommand>(
        name, [changes = std::move(changes)](const cmd::CommandContext&) -> Result<ChangeSet> {
            return changes;
        });
}

const LeaderGeometry& leaderIn(const Model& model, EntityId id)
{
    return std::get<LeaderGeometry>(model.entities.find(id)->geometry);
}

// Whether a plain leader's note is a whole number: what a numbered balloon
// says.
bool isNumber(const std::string& text)
{
    long long value = 0;
    const auto [end, error] = std::from_chars(text.data(), text.data() + text.size(), value);
    return !text.empty() && error == std::errc{} && end == text.data() + text.size();
}

bool isNumberedBalloon(const LeaderGeometry& leader)
{
    return leader.callout == katana::entity::CalloutShape::Circle &&
           !katana::entity::isSmart(leader) && isNumber(leader.text);
}

// In id order, each once.
std::vector<EntityId> unique(std::vector<EntityId> ids)
{
    std::sort(ids.begin(), ids.end());
    ids.erase(std::unique(ids.begin(), ids.end()), ids.end());
    return ids;
}

// Where LEADER FOR puts a tip on `entity`; `angle` is the direction from the
// tip to the note, radians. nullopt for what offers no place (a dimension, a
// label, a leader).
std::optional<AnchorRef> naturalAnchor(const Entity& entity, double angle)
{
    AnchorRef ref;
    ref.entity = entity.id;
    ref.point = AnchorPoint::Along;
    const auto& geometry = entity.geometry;
    if (std::holds_alternative<katana::entity::PointGeometry>(geometry) ||
        std::holds_alternative<katana::entity::TextGeometry>(geometry)) {
        ref.point = AnchorPoint::Position;
        return ref;
    }
    if (std::holds_alternative<katana::geometry::Segment2>(geometry) ||
        std::holds_alternative<katana::geometry::Arc2>(geometry)) {
        ref.parameter = 0.5;
        return ref;
    }
    if (std::holds_alternative<katana::geometry::Circle2>(geometry)) {
        const double turn = katana::math::normalizeAngle(angle) / katana::math::kTwoPi;
        ref.parameter = turn < 1.0 ? turn : 0.0;
        return ref;
    }
    if (const auto* polyline = std::get_if<katana::geometry::Polyline2>(&geometry)) {
        const auto& v = polyline->vertices;
        if (polyline->closed && v.size() >= 3) {
            ref.point = AnchorPoint::Inside;
            return ref;
        }
        const double half = 0.5 * polyline->length();
        if (!(half > 0.0) || v.size() < 2) {
            return std::nullopt;
        }
        double before = 0.0;
        for (std::size_t i = 0; i + 1 < v.size(); ++i) {
            const double length = v[i].distanceTo(v[i + 1]);
            // A piece within the geometric tolerance has no "along", as for
            // nearestAnchor: the half landing in one goes to the next piece.
            if (!polyline->segment(i).isDegenerate() &&
                (before + length >= half || i + 2 == v.size())) {
                ref.index = static_cast<std::uint32_t>(i);
                ref.parameter = std::clamp((half - before) / length, 0.0, 1.0);
                return ref;
            }
            before += length;
        }
        return std::nullopt;
    }
    if (const auto* curve = std::get_if<katana::geometry::CurvePolyline2>(&geometry)) {
        // As a polyline: inside when closed, else halfway along - found by
        // nearestAnchor from the point halfway, so an arc segment's
        // fraction is of its sweep.
        if (curve->closed && curve->vertices.size() >= 3) {
            ref.point = AnchorPoint::Inside;
            return ref;
        }
        const double half = 0.5 * curve->length();
        if (!(half > 0.0)) {
            return std::nullopt;
        }
        return katana::entity::nearestAnchor(entity, curve->pointAtStation(half));
    }
    if (const auto* ellipse = std::get_if<katana::geometry::Ellipse2>(&geometry)) {
        if (ellipse->isFull()) {
            ref.point = AnchorPoint::Inside;
        } else {
            ref.parameter = 0.5;
        }
        return ref;
    }
    if (std::holds_alternative<katana::geometry::Spline2>(geometry)) {
        // No place along a spline (nearestAnchor): its start.
        ref.point = AnchorPoint::Start;
        return ref;
    }
    return std::nullopt;
}

// The context a refusal about leader `id` names.
std::string refusalContext(EntityId id, const std::string& context)
{
    return "leader=" + std::to_string(id) + (context.empty() ? std::string() : " " + context);
}

} // namespace

LeaderNote noteOf(const LeaderGeometry& leader)
{
    if (!leader.labelStyle.empty()) {
        return LeaderNote{LeaderNote::Kind::LabelStyle, leader.labelStyle};
    }
    return LeaderNote{leader.fields ? LeaderNote::Kind::Template : LeaderNote::Kind::Text,
                      leader.text};
}

Status checkLeaderChange(const Model& model, const LeaderChange& change)
{
    if (change.note && change.note->kind == LeaderNote::Kind::Template) {
        if (auto status = katana::entity::checkLeaderTemplate(change.note->text); !status) {
            return status;
        }
    }
    if (change.note && change.note->kind == LeaderNote::Kind::LabelStyle &&
        !model.labelStyles.contains(change.note->text)) {
        return makeError(ErrorCode::NotFound, "label style does not exist", change.note->text);
    }
    if (change.textStyle && !change.textStyle->empty() &&
        !model.textStyles.contains(*change.textStyle)) {
        return makeError(ErrorCode::NotFound, "text style does not exist", *change.textStyle);
    }
    if ((change.vertex && change.hang) ||
        (change.vertex && change.vertex->index == 0 && change.tip)) {
        return makeError(ErrorCode::InvalidArgument,
                         "a leader point cannot be assigned twice in one edit");
    }
    if (change.landingAngle && *change.landingAngle && !std::isfinite(**change.landingAngle)) {
        return makeError(ErrorCode::InvalidArgument, "landingangle= must be finite or auto");
    }
    // Millimetres on paper, where 0 already means something (the text
    // style's height, no landing): below it there is nothing to draw, and a
    // negative landing would hang the note back over its own line.
    for (const auto& [size, key] :
         {std::pair{change.paperHeight, "paper"}, std::pair{change.arrowSize, "arrowsize"},
          std::pair{change.landing, "landing"}}) {
        if (size && !(std::isfinite(*size) && *size >= 0.0)) {
            return makeError(ErrorCode::InvalidArgument,
                             std::string(key) + "= must be a size of 0 mm or more",
                             katana::core::formatExactReal(*size));
        }
    }
    return {};
}

Result<LeaderGeometry> moveLeaderVertices(const LeaderGeometry& leader,
                                         const std::vector<LeaderVertexMove>& moves)
{
    LeaderGeometry moved = leader;
    std::set<std::size_t> changed;
    for (const LeaderVertexMove& move : moves) {
        if (move.index >= moved.vertices.size()) {
            return makeError(ErrorCode::InvalidArgument, "leader vertex index is out of range",
                             std::to_string(move.index));
        }
        if (!move.position.isFinite()) {
            return makeError(ErrorCode::InvalidArgument, "leader point coordinates must be finite");
        }
        if (!changed.insert(move.index).second) {
            return makeError(ErrorCode::InvalidArgument, "a leader vertex is moved more than once",
                             std::to_string(move.index));
        }
        if (move.index == 0 && leader.tipRef.associated() &&
            move.position != leader.vertices.front()) {
            return makeError(ErrorCode::InvalidArgument,
                             "the leader tip is attached; use LEADER ATTACH or DETACH before moving it");
        }
        moved.vertices[move.index] = move.position;
    }
    if (!std::isfinite(katana::geometry::Polyline2{moved.vertices, false}.length())) {
        return makeError(ErrorCode::InvalidArgument, "the edited leader is too large");
    }
    if (const auto status = katana::entity::validate(moved); !status) {
        return status.error();
    }
    return moved;
}

Result<std::optional<double>> parseLeaderLandingAngle(std::string_view degrees)
{
    if (katana::core::equalsIgnoringCase(degrees, "auto")) {
        return std::optional<double>{};
    }
    const auto value = katana::core::parseFiniteDouble(degrees);
    if (!value) {
        return makeError(ErrorCode::InvalidArgument,
                         "landingangle= takes auto or a finite angle in degrees", std::string(degrees));
    }
    return std::optional<double>(std::remainder(*value, 360.0) * katana::math::kDegToRad);
}

Status applyLeaderChange(const Model& model, const LeaderChange& change, LeaderGeometry& leader,
                         EntityId id, const katana::entity::LeaderContext& context)
{
    if (auto status = checkLeaderChange(model, change); !status) {
        return status;
    }
    if (change.note) {
        switch (change.note->kind) {
        case LeaderNote::Kind::Text:
            leader.text = change.note->text;
            leader.fields = false;
            leader.labelStyle.clear();
            break;
        case LeaderNote::Kind::Template:
            leader.text = change.note->text;
            leader.fields = true;
            leader.labelStyle.clear();
            break;
        case LeaderNote::Kind::LabelStyle: {
            const katana::entity::LabelStyle* style = model.labelStyles.find(change.note->text);
            leader.labelStyle = change.note->text;
            leader.text.clear();
            leader.fields = false;
            // Lent so the leader looks as the style's labels do; after
            // that the look is the leader's own.
            if (!change.textStyle && !style->textStyle.empty()) {
                leader.style = style->textStyle;
            }
            if (!change.paperHeight && style->paperHeight > 0.0) {
                leader.paperHeight = style->paperHeight;
            }
            break;
        }
        }
    }
    if (change.arrow) {
        leader.arrow = *change.arrow;
    }
    if (change.callout) {
        leader.callout = *change.callout;
    }
    if (change.textStyle) {
        leader.style = *change.textStyle;
    }
    if (change.paperHeight) {
        leader.paperHeight = *change.paperHeight;
    }
    if (change.arrowSize) {
        leader.arrowSize = *change.arrowSize;
    }
    if (change.landing) {
        leader.landing = *change.landing;
    }
    if (change.landingAngle) {
        leader.landingAngle = *change.landingAngle;
    }
    if (change.tip) {
        if (id != 0 && change.tip->ref.entity == id) {
            return makeError(ErrorCode::InvalidArgument,
                             "a leader's tip cannot be on the leader itself",
                             "id=" + std::to_string(id));
        }
        if (leader.vertices.empty()) {
            leader.vertices.push_back(change.tip->point);
        } else {
            leader.vertices.front() = change.tip->point;
        }
        leader.tipRef = change.tip->ref;
    }
    if (change.hang || change.vertex) {
        if (leader.vertices.empty()) {
            return makeError(ErrorCode::InvalidGeometry, "the leader has no point to move");
        }
        const LeaderVertexMove move = change.vertex
                                          ? *change.vertex
                                          : LeaderVertexMove{leader.vertices.size() - 1, *change.hang};
        auto moved = moveLeaderVertices(leader, {move});
        if (!moved) {
            return moved.error();
        }
        leader = std::move(*moved);
    }
    return katana::entity::checkLeaderSaysSomething(model, leader, codePropertyCandidates(),
                                                    context);
}

Status requireLeaders(const Model& model, const std::vector<EntityId>& ids)
{
    for (const EntityId id : ids) {
        const Entity* entity = model.entities.find(id);
        if (entity == nullptr || !std::holds_alternative<LeaderGeometry>(entity->geometry)) {
            return makeError(ErrorCode::InvalidArgument, "that entity is not a leader",
                             "id=" + std::to_string(id));
        }
    }
    return {};
}

// Counted only while a circle: isNumberedBalloon.
Status checkNumberedBalloon(const LeaderChange& change, bool balloon)
{
    if (balloon && !change.note && change.callout &&
        *change.callout != katana::entity::CalloutShape::Circle) {
        return makeError(ErrorCode::InvalidArgument,
                         "a numbered balloon is a circle: leave callout= a circle, or give the "
                         "balloon a note",
                         "callout=" + std::string(katana::entity::toString(*change.callout)));
    }
    return {};
}

Result<LeaderGeometry> newLeader(const Model& model, const std::vector<AnchoredPoint>& points,
                                 const LeaderChange& change, bool balloon,
                                 const katana::entity::LeaderContext& context)
{
    if (auto status = checkNumberedBalloon(change, balloon); !status) {
        return status.error();
    }
    LeaderGeometry leader;
    for (const AnchoredPoint& point : points) {
        leader.vertices.push_back(point.point);
    }
    if (!points.empty()) {
        leader.tipRef = points.front().ref; // the tip follows what it points at
    }
    if (leader.tipRef.point == AnchorPoint::Inside) {
        // A leader ending inside an outline ends in a dot, one ending on it
        // in an arrowhead (ISO 128-22, leader lines).
        leader.arrow = katana::entity::ArrowHead::Dot;
    }
    if (balloon) {
        leader.callout = katana::entity::CalloutShape::Circle;
        if (!change.note) {
            auto number = nextBalloonNumber(model);
            if (!number) {
                return number.error();
            }
            leader.text = std::to_string(*number);
        }
    }
    LeaderChange rest = change;
    rest.tip.reset(); // the points are the leader's already
    if (auto status = applyLeaderChange(model, rest, leader, 0, context); !status) {
        return status.error();
    }
    return leader;
}

Result<CommandPtr> changeLeaders(const Model& model, std::vector<EntityId> ids,
                                 const LeaderChange& change,
                                 const katana::entity::LeaderContext& context)
{
    ids = unique(std::move(ids));
    if (ids.empty()) {
        return makeError(ErrorCode::InvalidArgument, "no leader to change");
    }
    if (auto status = requireLeaders(model, ids); !status) {
        return status.error();
    }
    if (change.hang && ids.size() != 1) {
        return makeError(ErrorCode::InvalidArgument,
                         "at= moves one leader's note; name one leader");
    }
    if (change.vertex && ids.size() != 1) {
        return makeError(ErrorCode::InvalidArgument,
                         "vertex= moves one leader's point; name one leader");
    }
    ChangeSet changes;
    for (const EntityId id : ids) {
        Entity changed = *model.entities.find(id);
        auto& leader = std::get<LeaderGeometry>(changed.geometry);
        if (auto status = applyLeaderChange(model, change, leader, id, context); !status) {
            return makeError(status.error().code, status.error().message,
                             refusalContext(id, status.error().context));
        }
        if (changed == *model.entities.find(id)) {
            continue;
        }
        changes.modify.push_back(std::move(changed));
    }
    if (changes.empty()) {
        return CommandPtr{};
    }
    return commandOf("SET_LEADER", std::move(changes));
}

Result<CommandPtr> attachLeader(const Model& model, EntityId id, const AnchoredPoint& place,
                                const katana::entity::LeaderContext& context)
{
    if (!place.ref.associated()) {
        return makeError(ErrorCode::InvalidArgument,
                         "ATTACH puts the tip on an entity: give #id, #id.end, #id.inside or "
                         "#id@x,y");
    }
    if (auto status = requireLeaders(model, {id}); !status) {
        return status.error();
    }
    LeaderChange change;
    change.tip = place;
    Entity changed = *model.entities.find(id);
    auto& leader = std::get<LeaderGeometry>(changed.geometry);
    if (auto status = applyLeaderChange(model, change, leader, id, context); !status) {
        return status.error();
    }
    if (changed == *model.entities.find(id)) {
        return CommandPtr{}; // on that place already: no step, as changeLeaders
    }
    ChangeSet changes;
    changes.modify.push_back(std::move(changed));
    return commandOf("ATTACH_LEADER", std::move(changes));
}

Result<LeaderRelease> releaseLeaders(const Model& model, std::vector<EntityId> ids, bool detach,
                                     const katana::entity::LeaderContext& context)
{
    ids = unique(std::move(ids));
    if (auto status = requireLeaders(model, ids); !status) {
        return status.error();
    }
    LeaderRelease release;
    ChangeSet changes;
    for (const EntityId id : ids) {
        Entity changed = *model.entities.find(id);
        auto& leader = std::get<LeaderGeometry>(changed.geometry);
        if (detach && !leader.tipRef.associated()) {
            continue;
        }
        if (katana::entity::isSmart(leader)) {
            // Frozen as it reads now, a surface's level and an alignment's
            // station included.
            leader.text = leaderSays(model, leader, context);
            leader.fields = false;
            leader.labelStyle.clear();
            ++release.frozen;
        } else if (!detach) {
            continue;
        }
        if (detach) {
            leader.tipRef = AnchorRef{};
        }
        changes.modify.push_back(std::move(changed));
    }
    release.changed = changes.modify.size();
    if (!changes.empty()) {
        release.command = commandOf(detach ? "DETACH_LEADER" : "FREEZE_LEADER", std::move(changes));
    }
    return release;
}

Result<LeadersProperty>
setLeadersTargetProperty(const Model& model, std::vector<EntityId> ids, const std::string& key,
                         const std::optional<katana::entity::PropertyValue>& value)
{
    ids = unique(std::move(ids));
    if (ids.empty()) {
        return makeError(ErrorCode::InvalidArgument, "no leader to set an attribute through");
    }
    if (auto status = requireLeaders(model, ids); !status) {
        return status.error();
    }
    LeadersProperty result;
    std::vector<EntityId> entities;
    // The first leader left out says why, a flag and not an optional: that
    // reason is what a single leader's refusal is.
    Status firstReason;
    for (const EntityId id : ids) {
        const AnchorRef tipRef = leaderIn(model, id).tipRef;
        if (!tipRef.associated()) {
            ++result.skipped;
            if (firstReason) {
                firstReason = makeError(
                    ErrorCode::InvalidArgument,
                    "that leader's tip is on no entity, so it has no attributes to set",
                    "id=" + std::to_string(id));
            }
            continue;
        }
        if (!model.entities.contains(tipRef.entity)) {
            ++result.skipped;
            if (firstReason) {
                firstReason = makeError(ErrorCode::NotFound,
                                        "the entity the leader's tip is on is gone",
                                        "id=" + std::to_string(tipRef.entity));
            }
            continue;
        }
        entities.push_back(tipRef.entity);
        ++result.leaders;
    }
    if (result.leaders == 0) {
        return firstReason.error();
    }
    if (key.empty()) {
        return makeError(ErrorCode::InvalidArgument, "an attribute needs a name");
    }
    entities = unique(std::move(entities));
    result.entities = entities.size();
    result.command = value ? cmd::setEntityProperty(entities, key, *value)
                           : cmd::removeEntityProperty(entities, key);
    return result;
}

Result<CommandPtr>
setLeaderTargetProperty(const Model& model, EntityId id, const std::string& key,
                        const std::optional<katana::entity::PropertyValue>& value)
{
    auto many = setLeadersTargetProperty(model, {id}, key, value);
    if (!many) {
        return many.error();
    }
    return std::move(many->command);
}

Result<LeadersFor> leadersFor(const Model& model, std::vector<EntityId> targets,
                              const LeadersForOptions& options, double scale,
                              const katana::commands::EntityAttributes& attributes,
                              const katana::entity::LeaderContext& context)
{
    targets = unique(std::move(targets));
    if (targets.empty()) {
        return makeError(ErrorCode::InvalidArgument, "no entity to make leaders for");
    }
    if (!(options.length > 0.0) || !std::isfinite(options.length)) {
        return makeError(ErrorCode::InvalidArgument, "length= must be more than zero",
                         katana::core::formatExactReal(options.length));
    }
    if (!std::isfinite(options.angle) || !(scale > 0.0)) {
        return makeError(ErrorCode::InvalidArgument, "the angle and the scale must be numbers");
    }
    const Vec2 toNote = Vec2(std::cos(options.angle), std::sin(options.angle)) *
                        katana::entity::annotationModelSize(options.length, scale);
    // What does not depend on the entity is refused once, for all of them.
    if (auto status = checkNumberedBalloon(options.change, options.balloon); !status) {
        return status.error();
    }
    if (auto status = checkLeaderChange(model, options.change); !status) {
        return status.error();
    }
    const bool numbered = options.balloon && !options.change.note;
    long long first = 0;
    if (numbered) {
        auto next = nextBalloonNumber(model);
        if (!next) {
            return next.error();
        }
        first = *next;
        // Each target may take a number: the last must still be one.
        if (!targets.empty() &&
            static_cast<unsigned long long>(targets.size() - 1) >
                static_cast<unsigned long long>(std::numeric_limits<long long>::max() - first)) {
            return makeError(ErrorCode::InvalidArgument,
                             "the balloons would be numbered past the largest number there is: "
                             "BALLOON RENUMBER first",
                             "from=" + std::to_string(first));
        }
    }
    LeadersFor result;
    ChangeSet changes;
    for (const EntityId id : targets) {
        const Entity* target = model.entities.find(id);
        if (target == nullptr) {
            return makeError(ErrorCode::NotFound, "entity does not exist",
                             "id=" + std::to_string(id));
        }
        const auto place = leaderPlaceOn(*target, options.angle);
        if (!place) {
            ++result.skipped;
            if (result.firstSkip.empty()) {
                result.firstSkip = "id=" + std::to_string(id) + " (a " +
                                   std::string(katana::entity::toString(target->type())) +
                                   ") has no place to attach a leader to";
            }
            continue;
        }
        LeaderGeometry leader;
        leader.vertices = {place->point, place->point + toNote};
        leader.tipRef = place->ref;
        if (place->ref.point == AnchorPoint::Inside) {
            leader.arrow = katana::entity::ArrowHead::Dot; // ISO 128-22, leader lines
        }
        if (options.balloon) {
            leader.callout = katana::entity::CalloutShape::Circle;
        }
        LeaderChange change = options.change;
        change.tip.reset();
        change.hang.reset();
        if (numbered) {
            // On from the first, by the balloons made so far.
            change.note =
                LeaderNote{LeaderNote::Kind::Text,
                           std::to_string(first + static_cast<long long>(changes.add.size()))};
        }
        if (auto status = applyLeaderChange(model, change, leader, 0, context); !status) {
            // Checked above for what does not depend on the entity, so this
            // is a note that would say nothing about this one: skipped.
            ++result.skipped;
            if (result.firstSkip.empty()) {
                result.firstSkip = status.error().message + " [" + status.error().context + "]";
            }
            continue;
        }
        Entity entity;
        entity.geometry = std::move(leader);
        entity.layer = attributes.layer;
        entity.style = attributes.style;
        entity.color = attributes.color;
        changes.add.push_back(std::move(entity));
    }
    if (changes.add.empty()) {
        return makeError(ErrorCode::InvalidArgument, "no leader made", result.firstSkip);
    }
    result.made = changes.add.size();
    result.command =
        commandOf(options.balloon ? "CREATE_BALLOONS" : "CREATE_LEADERS", std::move(changes));
    return result;
}

std::optional<AnchoredPoint> leaderPlaceOn(const Entity& entity, double angle)
{
    const auto ref = naturalAnchor(entity, angle);
    const auto point = ref ? katana::entity::resolveAnchor(entity, *ref) : std::nullopt;
    if (!point) {
        return std::nullopt;
    }
    return AnchoredPoint{*point, *ref};
}

std::optional<AnchoredPoint> tipPlaceOn(const Entity& entity, const Point2& tip)
{
    using katana::geometry::Containment;
    bool inside = false;
    if (const auto* outline = std::get_if<katana::geometry::Polyline2>(&entity.geometry)) {
        inside = outline->closed && outline->vertices.size() >= 3 &&
                 outline->classify(tip) == Containment::Inside;
    } else if (const auto* circle = std::get_if<katana::geometry::Circle2>(&entity.geometry)) {
        inside = circle->classify(tip) == Containment::Inside;
    } else if (const auto* curve =
                   std::get_if<katana::geometry::CurvePolyline2>(&entity.geometry)) {
        inside = curve->closed && curve->vertices.size() >= 3 &&
                 curve->toPolyline(katana::geometry::kCurveChordTolerance).classify(tip) ==
                     Containment::Inside;
    } else if (const auto* ellipse = std::get_if<katana::geometry::Ellipse2>(&entity.geometry)) {
        // Inside a whole ellipse: (u/a)^2 + (v/b)^2 < 1 in its own axes.
        const double a = ellipse->majorRadius();
        const double b = ellipse->minorRadius();
        if (ellipse->isFull() && a > 0.0 && b > 0.0) {
            const katana::geometry::Vec2 d = tip - ellipse->center;
            const double u = d.dot(ellipse->majorAxis) / a;
            const double v = d.dot(ellipse->majorAxis.perpendicular()) / a;
            inside = (u / a) * (u / a) + (v / b) * (v / b) < 1.0;
        }
    }
    std::optional<AnchorRef> ref;
    if (inside) {
        ref = AnchorRef{entity.id, AnchorPoint::Inside};
    } else if (std::holds_alternative<katana::geometry::Spline2>(entity.geometry)) {
        // No place along a spline (entity::nearestAnchor): its start, as
        // LEADER FOR puts one.
        ref = AnchorRef{entity.id, AnchorPoint::Start};
    } else {
        ref = katana::entity::nearestAnchor(entity, tip);
    }
    const auto point = ref ? katana::entity::resolveAnchor(entity, *ref) : std::nullopt;
    if (!point) {
        return std::nullopt;
    }
    return AnchoredPoint{*point, *ref};
}

Result<LeaderAlignment> alignLeaders(const Model& model, std::vector<EntityId> ids,
                                     std::optional<double> x, std::optional<double> spacing)
{
    ids = unique(std::move(ids));
    if (ids.empty()) {
        return makeError(ErrorCode::InvalidArgument, "no leader to align");
    }
    if (auto status = requireLeaders(model, ids); !status) {
        return status.error();
    }
    if (spacing && !(std::isfinite(*spacing) && *spacing > 0.0)) {
        return makeError(ErrorCode::InvalidArgument, "spacing= must be a size more than zero");
    }
    if (x && !std::isfinite(*x)) {
        return makeError(ErrorCode::InvalidArgument, "x= must be a number");
    }
    // Top to bottom by where each note hangs, then by id.
    std::stable_sort(ids.begin(), ids.end(), [&](EntityId a, EntityId b) {
        return leaderIn(model, a).vertices.back().y > leaderIn(model, b).vertices.back().y;
    });
    const Point2 top = leaderIn(model, ids.front()).vertices.back();
    // A spacing each finite can still stack the lowest note past any number.
    if (spacing && !std::isfinite(top.y - static_cast<double>(ids.size() - 1) * *spacing)) {
        return makeError(ErrorCode::InvalidArgument,
                         "spacing= stacks the notes further than a drawing reaches",
                         "spacing=" + katana::core::formatExactReal(*spacing));
    }
    LeaderAlignment alignment;
    alignment.x = x.value_or(top.x);
    alignment.count = ids.size();
    ChangeSet changes;
    for (std::size_t k = 0; k < ids.size(); ++k) {
        Entity changed = *model.entities.find(ids[k]);
        auto& leader = std::get<LeaderGeometry>(changed.geometry);
        Point2& hang = leader.vertices.back();
        const Point2 moved(alignment.x,
                           spacing ? top.y - static_cast<double>(k) * *spacing : hang.y);
        if (moved == hang) {
            continue;
        }
        hang = moved;
        changes.modify.push_back(std::move(changed));
    }
    if (!changes.empty()) {
        alignment.command = commandOf("ALIGN_LEADERS", std::move(changes));
    }
    return alignment;
}

BalloonRenumbering renumberBalloons(const Model& model, int start, BalloonOrder order,
                                    const std::optional<std::vector<EntityId>>& within)
{
    std::vector<EntityId> balloons;
    // The ids are searched as they are given: a scope's matches arrive in id
    // order, and a caller's own list need not.
    // A flag and a list, not the optional itself, in the lambda below.
    const bool limited = within.has_value();
    std::vector<EntityId> allowed;
    if (limited) {
        allowed = unique(*within);
    }
    model.entities.forEach([&](const Entity& entity) {
        const auto* leader = std::get_if<LeaderGeometry>(&entity.geometry);
        if (leader != nullptr && isNumberedBalloon(*leader) &&
            (!limited || std::binary_search(allowed.begin(), allowed.end(), entity.id))) {
            balloons.push_back(entity.id);
        }
    });
    if (order != BalloonOrder::Id) {
        std::stable_sort(balloons.begin(), balloons.end(), [&](EntityId a, EntityId b) {
            const Point2& p = leaderIn(model, a).vertices.front();
            const Point2& q = leaderIn(model, b).vertices.front();
            return order == BalloonOrder::X ? p.x < q.x : p.y > q.y;
        });
    }
    BalloonRenumbering result;
    result.balloons = balloons.size();
    ChangeSet changes;
    for (std::size_t k = 0; k < balloons.size(); ++k) {
        Entity changed = *model.entities.find(balloons[k]);
        auto& leader = std::get<LeaderGeometry>(changed.geometry);
        const std::string number =
            std::to_string(static_cast<long long>(start) + static_cast<long long>(k));
        if (leader.text == number) {
            continue;
        }
        leader.text = number;
        changes.modify.push_back(std::move(changed));
    }
    result.renumbered = changes.modify.size();
    if (!changes.empty()) {
        result.command = commandOf("RENUMBER_BALLOONS", std::move(changes));
    }
    return result;
}

Result<long long> nextBalloonNumber(const Model& model)
{
    long long highest = 0;
    model.entities.forEach([&](const Entity& entity) {
        const auto* leader = std::get_if<LeaderGeometry>(&entity.geometry);
        if (leader == nullptr || !isNumberedBalloon(*leader)) {
            return;
        }
        long long value = 0;
        std::from_chars(leader->text.data(), leader->text.data() + leader->text.size(), value);
        highest = std::max(highest, value);
    });
    if (highest == std::numeric_limits<long long>::max()) {
        return makeError(ErrorCode::InvalidArgument,
                         "the highest balloon is numbered as high as a number goes, so has no "
                         "next: BALLOON RENUMBER first",
                         "n=" + std::to_string(highest));
    }
    return highest + 1;
}

std::vector<LeaderValueRow> leaderValueRows(const katana::entity::LabelValues& values)
{
    std::vector<LeaderValueRow> rows;
    for (const std::string_view name : katana::entity::leaderValueNames()) {
        if (const auto found = values.find(name); found != values.end()) {
            rows.push_back({std::string(name), katana::entity::formatValue(found->second)});
        }
    }
    // The surfaces' and the alignments', kind by kind - every surface's
    // level, then every difference - each kind's in name order.
    for (const std::string_view kind : katana::entity::leaderContextKinds()) {
        for (const auto& [name, value] : values) {
            const auto parsed = katana::entity::contextValueName(name);
            if (parsed && parsed->kind == kind) {
                rows.push_back({name, katana::entity::formatValue(value)});
            }
        }
    }
    for (const auto& [name, value] : values) {
        if (name.starts_with("prop.")) {
            rows.push_back({name, katana::entity::formatValue(value)});
        }
    }
    return rows;
}

std::optional<katana::entity::LabelValues>
leaderTargetValues(const Model& model, const LeaderGeometry& leader,
                   const katana::entity::LeaderContext& context)
{
    return katana::entity::leaderValues(model, leader, codePropertyCandidates(), context);
}

std::string leaderSays(const Model& model, const LeaderGeometry& leader,
                       const katana::entity::LeaderContext& context)
{
    return katana::entity::leaderNote(model, leader, codePropertyCandidates(), context);
}

namespace {

// A surface is found ignoring case (SurfaceStore::find, which is what a
// note's {surface.NAME} reads through), an alignment exactly (ALIGN).
bool sameName(ReadKind kind, std::string_view a, std::string_view b)
{
    return kind == ReadKind::Surface ? katana::core::equalsIgnoringCase(a, b) : a == b;
}

// Whether the template field `field` ("dsurface.Existing Ground") reads
// `name` of `kind`.
bool fieldReads(ReadKind kind, std::string_view name, std::string_view field)
{
    const auto parsed = katana::entity::contextValueName(field);
    return parsed && katana::entity::isSurfaceContextKind(parsed->kind) == (kind == ReadKind::Surface) &&
           sameName(kind, parsed->name, name);
}

bool templateReads(ReadKind kind, std::string_view name, const std::string& templateText)
{
    for (const std::string& field : katana::entity::templateFields(templateText)) {
        if (fieldReads(kind, name, field)) {
            return true;
        }
    }
    return false;
}

} // namespace

std::size_t leadersReading(const Model& model, ReadKind kind, std::string_view name)
{
    std::size_t count = 0;
    model.entities.forEach([&](const Entity& entity) {
        const auto* leader = std::get_if<LeaderGeometry>(&entity.geometry);
        if (leader != nullptr && leader->fields && templateReads(kind, name, leader->text)) {
            ++count;
        }
    });
    return count;
}

LeaderReadsRenamed renameLeaderReads(const Model& model, ReadKind kind, std::string_view from,
                                     std::string_view to)
{
    LeaderReadsRenamed result;
    const bool writable = !to.empty() && to.find_first_of(":{}") == std::string_view::npos;
    ChangeSet changes;
    model.entities.forEach([&](const Entity& entity) {
        const auto* leader = std::get_if<LeaderGeometry>(&entity.geometry);
        if (leader == nullptr || !leader->fields || !templateReads(kind, from, leader->text)) {
            return;
        }
        // On a locked layer (its own or a parent's) a leader cannot be
        // changed, and one leader in the way would refuse the whole rename:
        // it is left reading the old name, counted.
        const katana::entity::ResolvedLayer layer = model.layers.resolve(entity.layer);
        if (!writable || layer.layer == nullptr || layer.locked) {
            ++result.unfollowed;
            return;
        }
        Entity changed = entity;
        auto& rewritten = std::get<LeaderGeometry>(changed.geometry);
        rewritten.text = katana::entity::renameTemplateFields(
            leader->text, [&](std::string_view field) -> std::optional<std::string> {
                if (!fieldReads(kind, from, field)) {
                    return std::nullopt;
                }
                const auto parsed = katana::entity::contextValueName(field);
                return std::string(parsed->kind) + "." + std::string(to);
            });
        if (rewritten.text == leader->text) {
            return; // renamed to its own spelling: nothing to say
        }
        changes.modify.push_back(std::move(changed));
        ++result.followed;
    });
    if (!changes.empty()) {
        result.command = commandOf("RENAME_LEADER_READS", std::move(changes));
    }
    return result;
}

} // namespace katana::cad::annotation
