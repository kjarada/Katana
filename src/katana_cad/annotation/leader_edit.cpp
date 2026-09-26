#include "katana/cad/annotation/leader_edit.hpp"

#include <algorithm>
#include <charconv>
#include <cmath>
#include <iterator>
#include <limits>
#include <memory>
#include <utility>

#include "katana/cad/survey_coding.hpp"
#include "katana/commands/change_set.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/anchor.hpp"
#include "katana/entity/annotation.hpp"
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
std::string leaderContext(EntityId id, const std::string& context)
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
    // Millimetres on paper, where 0 already means something (the text
    // style's height, no landing): below it there is nothing to draw, and a
    // negative landing would hang the note back over its own line.
    for (const auto& [size, key] :
         {std::pair{change.paperHeight, "paper"}, std::pair{change.arrowSize, "arrowsize"},
          std::pair{change.landing, "landing"}}) {
        if (size && !(std::isfinite(*size) && *size >= 0.0)) {
            return makeError(ErrorCode::InvalidArgument,
                             std::string(key) + "= must be a size of 0 mm or more",
                             std::to_string(*size));
        }
    }
    return {};
}

Status applyLeaderChange(const Model& model, const LeaderChange& change, LeaderGeometry& leader,
                         EntityId id)
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
    if (change.hang && !leader.vertices.empty()) {
        leader.vertices.back() = *change.hang;
    }
    return katana::entity::checkLeaderSaysSomething(model, leader, codePropertyCandidates());
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
                                 const LeaderChange& change, bool balloon)
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
    if (auto status = applyLeaderChange(model, rest, leader); !status) {
        return status.error();
    }
    return leader;
}

Result<CommandPtr> changeLeaders(const Model& model, std::vector<EntityId> ids,
                                 const LeaderChange& change)
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
    ChangeSet changes;
    for (const EntityId id : ids) {
        Entity changed = *model.entities.find(id);
        auto& leader = std::get<LeaderGeometry>(changed.geometry);
        if (auto status = applyLeaderChange(model, change, leader, id); !status) {
            return makeError(status.error().code, status.error().message,
                             leaderContext(id, status.error().context));
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

Result<CommandPtr> attachLeader(const Model& model, EntityId id, const AnchoredPoint& place)
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
    if (auto status = applyLeaderChange(model, change, leader, id); !status) {
        return status.error();
    }
    if (changed == *model.entities.find(id)) {
        return CommandPtr{}; // on that place already: no step, as changeLeaders
    }
    ChangeSet changes;
    changes.modify.push_back(std::move(changed));
    return commandOf("ATTACH_LEADER", std::move(changes));
}

Result<LeaderRelease> releaseLeaders(const Model& model, std::vector<EntityId> ids, bool detach)
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
            leader.text = leaderSays(model, leader);
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

Result<CommandPtr>
setLeaderTargetProperty(const Model& model, EntityId id, const std::string& key,
                        const std::optional<katana::entity::PropertyValue>& value)
{
    if (auto status = requireLeaders(model, {id}); !status) {
        return status.error();
    }
    const AnchorRef tipRef = leaderIn(model, id).tipRef;
    if (!tipRef.associated()) {
        return makeError(ErrorCode::InvalidArgument,
                         "that leader's tip is on no entity, so it has no attributes to set",
                         "id=" + std::to_string(id));
    }
    if (!model.entities.contains(tipRef.entity)) {
        return makeError(ErrorCode::NotFound, "the entity the leader's tip is on is gone",
                         "id=" + std::to_string(tipRef.entity));
    }
    if (key.empty()) {
        return makeError(ErrorCode::InvalidArgument, "an attribute needs a name");
    }
    return value ? cmd::setEntityProperty({tipRef.entity}, key, *value)
                 : cmd::removeEntityProperty({tipRef.entity}, key);
}

Result<LeadersFor> leadersFor(const Model& model, std::vector<EntityId> targets,
                              const LeadersForOptions& options, double scale,
                              const katana::commands::EntityAttributes& attributes)
{
    targets = unique(std::move(targets));
    if (targets.empty()) {
        return makeError(ErrorCode::InvalidArgument, "no entity to make leaders for");
    }
    if (!(options.length > 0.0) || !std::isfinite(options.length)) {
        return makeError(ErrorCode::InvalidArgument, "length= must be more than zero",
                         std::to_string(options.length));
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
        if (auto status = applyLeaderChange(model, change, leader); !status) {
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

BalloonRenumbering renumberBalloons(const Model& model, int start, BalloonOrder order)
{
    std::vector<EntityId> balloons;
    model.entities.forEach([&](const Entity& entity) {
        const auto* leader = std::get_if<LeaderGeometry>(&entity.geometry);
        if (leader != nullptr && isNumberedBalloon(*leader)) {
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
    for (const auto& [name, value] : values) {
        if (name.starts_with("prop.")) {
            rows.push_back({name, katana::entity::formatValue(value)});
        }
    }
    return rows;
}

std::optional<katana::entity::LabelValues> leaderTargetValues(const Model& model,
                                                              const LeaderGeometry& leader)
{
    return katana::entity::leaderValues(model, leader, codePropertyCandidates());
}

std::string leaderSays(const Model& model, const LeaderGeometry& leader)
{
    return katana::entity::leaderNote(model, leader, codePropertyCandidates());
}

} // namespace katana::cad::annotation
