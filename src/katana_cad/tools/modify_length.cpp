// Lengthen and Reverse (see families.hpp).
//
// Lengthen follows AutoCAD's LENGTHEN dialogue: pick an object to be told its
// length, or choose how to change one - DElta (by so much; negative
// shortens), Percent (to so much of what it is), Total (to so long) or
// DYnamic (to where the cursor is) - then pick objects near the end to move.
// Lines, arcs and open polylines have ends; a closed outline or a circle has
// none and is refused. A polyline grows along its end segment and shrinks
// back along its path, dropping the vertices it passes; an arc keeps its
// centre and radius and changes its sweep. Every pick is one step of one
// session (modify_edit::EditSession), so one undo takes back the lot, and Esc
// keeps what was already changed.
//
// Reverse turns lines, arcs and polylines round - the same geometry, drawn
// from the other end - with the heights of a 3D string turned round with
// them. What has a direction matters wherever order does: a linestyle's
// ticks, a chainage, a string's heights, an export.

#include <algorithm>
#include <cmath>
#include <map>
#include <memory>
#include <optional>
#include <string>
#include <string_view>
#include <utility>
#include <variant>
#include <vector>

#include "everyday_support.hpp"
#include "families.hpp"
#include "katana/commands/change_set.hpp"
#include "katana/math/numerics.hpp"
#include "katana/survey/angles.hpp"
#include "modify_edit_support.hpp"

namespace katana::cad::tools {

namespace {

namespace cmd = katana::commands;
using everyday::fixed;
using everyday::Path;
using everyday::SelectionStep;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::entity::Geometry;
using katana::geometry::Arc2;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;
using katana::geometry::Vec2;
using modify_edit::asSentence;
using modify_edit::EditSession;
using modify_edit::formatNumber;
using modify_edit::isOption;
using modify_edit::kindName;
using modify_edit::parseNumber;
using modify_edit::PolylinePath;
using modify_edit::withArticle;
namespace tolerance = katana::math::tolerance;

constexpr double kTwoPi = 2.0 * katana::math::kPi;

// ---- Lengthen ----------------------------------------------------------------------

// What Lengthen remembers from one use to the next, as AutoCAD remembers the
// last delta, percentage and total: one object per program, shared by the
// tool's factory; nothing is stored in the drawing.
struct LengthenDefaults {
    double delta = 0.0;
    double percent = 100.0;
    double total = 1.0;
};

// Why `geometry` has no end to lengthen, or nullopt when it has.
std::optional<std::string> withoutEnds(const Geometry& geometry)
{
    if (std::holds_alternative<Segment2>(geometry) || std::holds_alternative<Arc2>(geometry)) {
        return std::nullopt;
    }
    if (const auto* polyline = std::get_if<Polyline2>(&geometry)) {
        if (polyline->closed) {
            return std::string("a closed polyline has no end to lengthen");
        }
        return std::nullopt;
    }
    return withArticle(kindName(geometry)) + " has no end to lengthen";
}

// True when the end of `geometry` nearer to `at` is its start.
bool startIsNearer(const Geometry& geometry, const Point2& at)
{
    if (const auto* line = std::get_if<Segment2>(&geometry)) {
        return at.distanceTo(line->start) < at.distanceTo(line->end);
    }
    if (const auto* arc = std::get_if<Arc2>(&geometry)) {
        return at.distanceTo(arc->startPoint()) < at.distanceTo(arc->endPoint());
    }
    // Along the polyline, not as the crow flies: a pick on the first leg of
    // a hairpin is nearer the start whatever the straight distances say.
    const PolylinePath path(std::get<Polyline2>(geometry));
    return path.stationOf(at) < 0.5 * path.length();
}

// `geometry` with the end at its start (atStart) or its end moved so that it
// is `length` long, the other end staying where it is.
Result<Geometry> lengthened(const Geometry& geometry, bool atStart, double length)
{
    if (!std::isfinite(length) || length <= tolerance::kGeometric) {
        return makeError(ErrorCode::InvalidArgument, "the new length must be more than zero");
    }
    if (const auto* line = std::get_if<Segment2>(&geometry)) {
        const Point2 fixedEnd = atStart ? line->end : line->start;
        const Point2 moving = atStart ? line->start : line->end;
        const Vec2 along = (moving - fixedEnd).normalized();
        const Point2 moved = fixedEnd + along * length;
        return Geometry(atStart ? Segment2{moved, line->end} : Segment2{line->start, moved});
    }
    if (const auto* arc = std::get_if<Arc2>(&geometry)) {
        const double sweep = std::copysign(length / arc->radius, arc->sweep);
        if (std::abs(sweep) >= kTwoPi) {
            return makeError(ErrorCode::InvalidArgument,
                             "an arc that long would go all the way round its circle");
        }
        return Geometry(atStart ? Arc2{arc->center, arc->radius, arc->endAngle() - sweep, sweep}
                                : Arc2{arc->center, arc->radius, arc->startAngle, sweep});
    }
    const Polyline2& polyline = std::get<Polyline2>(geometry);
    const PolylinePath path(polyline);
    const double current = path.length();
    if (length < current) {
        return Geometry(atStart ? path.between(current - length, current)
                                : path.between(0.0, length));
    }
    // Longer: the end segment goes on in its own direction.
    Polyline2 longer = polyline;
    auto& v = longer.vertices;
    const std::size_t end = atStart ? 0 : v.size() - 1;
    const std::size_t inner = atStart ? 1 : v.size() - 2;
    const Vec2 outward = v[end] - v[inner];
    if (outward.length() <= tolerance::kGeometric) {
        return makeError(ErrorCode::InvalidGeometry,
                         "the polyline's end segment has no length, so no direction to go on in");
    }
    v[end] = v[end] + outward.normalized() * (length - current);
    return Geometry(std::move(longer));
}

// The length that puts the moving end where `cursor` says (DYnamic): along a
// line's own direction, round an arc's circle, along a polyline's path - or
// on along its end segment when the cursor is beyond the end.
double lengthTo(const Geometry& geometry, bool atStart, const Point2& cursor)
{
    if (const auto* line = std::get_if<Segment2>(&geometry)) {
        const Point2 fixedEnd = atStart ? line->end : line->start;
        const Point2 moving = atStart ? line->start : line->end;
        return (cursor - fixedEnd).dot((moving - fixedEnd).normalized());
    }
    if (const auto* arc = std::get_if<Arc2>(&geometry)) {
        const double direction = std::atan2(cursor.y - arc->center.y, cursor.x - arc->center.x);
        const double sign = arc->sweep >= 0.0 ? 1.0 : -1.0;
        // Turned from the fixed end in the arc's own sense, in [0, 2*pi).
        const double from = atStart ? arc->endAngle() : arc->startAngle;
        double turned = atStart ? sign * (from - direction) : sign * (direction - from);
        turned = std::fmod(turned, kTwoPi);
        if (turned < 0.0) {
            turned += kTwoPi;
        }
        return arc->radius * turned;
    }
    const Polyline2& polyline = std::get<Polyline2>(geometry);
    const PolylinePath path(polyline);
    const auto& v = polyline.vertices;
    const Point2 end = atStart ? v.front() : v.back();
    const Vec2 outward = atStart ? v.front() - v[1] : v.back() - v[v.size() - 2];
    if (outward.length() > tolerance::kGeometric) {
        const double beyond = (cursor - end).dot(outward.normalized());
        if (beyond > 0.0) {
            return path.length() + beyond;
        }
    }
    const double station = path.stationOf(cursor);
    return atStart ? path.length() - station : station;
}

double lengthOf(const Geometry& geometry)
{
    const auto path = Path::of(geometry);
    return path ? path->length() : 0.0;
}

class LengthenTool final : public InteractiveTool {
  public:
    LengthenTool(const ToolContext& context, std::shared_ptr<LengthenDefaults> defaults)
        : session_(context.document), defaults_(std::move(defaults))
    {
    }

    [[nodiscard]] std::string prompt() const override
    {
        switch (step_) {
        case Step::Measure:
            return "Select an object to measure or [DElta/Percent/Total/DYnamic]";
        case Step::Amount:
            switch (mode_) {
            case Mode::Delta:
                return "Enter delta length <" + formatNumber(defaults_->delta) + ">";
            case Mode::Percent:
                return "Enter percentage length <" + formatNumber(defaults_->percent) + ">";
            case Mode::Total:
            case Mode::Dynamic:
                return "Specify total length <" + formatNumber(defaults_->total) + ">";
            }
            break;
        case Step::Change:
            return "Select an object to change or [Undo], Enter to finish";
        case Step::DynamicEnd:
            return "Specify new end point";
        }
        return {};
    }

    [[nodiscard]] ToolInput expects() const override
    {
        switch (step_) {
        case Step::Measure:
        case Step::Change:
            return ToolInput::Entity;
        case Step::Amount:
            return ToolInput::Value;
        case Step::DynamicEnd:
            return ToolInput::Point;
        }
        return ToolInput::Entity;
    }

    ToolStep entity(EntityId id, const Point2& at) override
    {
        if (step_ != Step::Measure && step_ != Step::Change) {
            return InteractiveTool::entity(id, at);
        }
        if (auto refusal = modify_edit::refusalToEdit(session_.document(), id);
            refusal && step_ == Step::Change) {
            return ToolStep::rejected(*refusal);
        }
        const std::vector<Entity> pieces = session_.pieces(id);
        if (pieces.size() != 1) {
            return ToolStep::rejected("that entity is not in the drawing");
        }
        const Entity& current = pieces.front();
        if (step_ == Step::Measure) {
            // Measuring asks nothing of the object's ends: a circle has a
            // length too.
            std::string text = "Current length " + fixed(lengthOf(current.geometry));
            if (const auto* arc = std::get_if<Arc2>(&current.geometry)) {
                text += ", included angle " + everyday::degrees(std::abs(arc->sweep));
            }
            return ToolStep::next(std::move(text));
        }
        if (auto why = withoutEnds(current.geometry)) {
            return ToolStep::rejected(*why);
        }
        const bool atStart = startIsNearer(current.geometry, at);
        if (mode_ == Mode::Dynamic) {
            target_ = id;
            targetAtStart_ = atStart;
            step_ = Step::DynamicEnd;
            return ToolStep::next();
        }
        const double now = lengthOf(current.geometry);
        double length = now;
        switch (mode_) {
        case Mode::Delta:
            if (defaults_->delta == 0.0) {
                return ToolStep::rejected("a delta of zero changes nothing; type DE to set one");
            }
            length = now + defaults_->delta;
            break;
        case Mode::Percent:
            length = now * defaults_->percent / 100.0;
            break;
        case Mode::Total:
            length = defaults_->total;
            break;
        case Mode::Dynamic:
            break;
        }
        return change(id, current, atStart, length);
    }

    ToolStep point(const Point2& at) override
    {
        if (step_ != Step::DynamicEnd) {
            return InteractiveTool::point(at);
        }
        const std::vector<Entity> pieces = session_.pieces(target_);
        if (pieces.size() != 1) {
            step_ = Step::Change;
            return ToolStep::rejected("that entity is not in the drawing");
        }
        const double length = lengthTo(pieces.front().geometry, targetAtStart_, at);
        ToolStep step = change(target_, pieces.front(), targetAtStart_, length);
        if (step.outcome != ToolStep::Outcome::Rejected) {
            step_ = Step::Change;
        }
        return step;
    }

    ToolStep value(std::string_view text) override
    {
        if (step_ == Step::Amount) {
            const auto number = parseNumber(text);
            if (!number) {
                return ToolStep::rejected("type a number, or press Enter for the default");
            }
            return chooseAmount(*number);
        }
        if (step_ == Step::Measure || step_ == Step::Change) {
            if (step_ == Step::Change && isOption(text, "Undo", "U")) {
                return undo();
            }
            if (isOption(text, "DElta", "DE")) {
                return chooseMode(Mode::Delta);
            }
            if (isOption(text, "Percent", "P")) {
                return chooseMode(Mode::Percent);
            }
            if (isOption(text, "Total", "T")) {
                return chooseMode(Mode::Total);
            }
            if (isOption(text, "DYnamic", "DY")) {
                return chooseMode(Mode::Dynamic);
            }
        }
        return ToolStep::rejected("'" + std::string(text) + "' is not an option here");
    }

    ToolStep enter() override
    {
        switch (step_) {
        case Step::Amount:
            // The default shown in the prompt.
            switch (mode_) {
            case Mode::Delta:
                return chooseAmount(defaults_->delta);
            case Mode::Percent:
                return chooseAmount(defaults_->percent);
            case Mode::Total:
            case Mode::Dynamic:
                return chooseAmount(defaults_->total);
            }
            break;
        case Step::DynamicEnd:
            return ToolStep::rejected("click or type the new end point");
        case Step::Measure:
        case Step::Change:
            break;
        }
        return finish();
    }

    ToolStep undo() override
    {
        switch (step_) {
        case Step::DynamicEnd:
            step_ = Step::Change;
            return ToolStep::next();
        case Step::Change:
            if (session_.undo()) {
                return ToolStep::next("Undid the last change.");
            }
            step_ = Step::Measure;
            return ToolStep::next();
        case Step::Amount:
            step_ = Step::Measure;
            return ToolStep::next();
        case Step::Measure:
            break;
        }
        return ToolStep::rejected("nothing to undo in this tool");
    }

    // Esc keeps every change already made; it never applies a default.
    ToolStep cancel() override
    {
        return session_.operations() == 0 ? ToolStep::done(nullptr) : finish();
    }

    [[nodiscard]] ToolFeedback preview(const Point2& cursor) const override
    {
        ToolFeedback feedback{session_.shapes(), {}};
        if (step_ == Step::DynamicEnd) {
            const std::vector<Entity> pieces = session_.pieces(target_);
            if (pieces.size() == 1) {
                const double length = lengthTo(pieces.front().geometry, targetAtStart_, cursor);
                if (auto shape = lengthened(pieces.front().geometry, targetAtStart_, length)) {
                    feedback.shapes.push_back(std::move(*shape));
                }
            }
        }
        return feedback;
    }

  private:
    enum class Mode { Delta, Percent, Total, Dynamic };
    enum class Step { Measure, Amount, Change, DynamicEnd };

    ToolStep chooseMode(Mode mode)
    {
        mode_ = mode;
        step_ = mode == Mode::Dynamic ? Step::Change : Step::Amount;
        return ToolStep::next();
    }

    ToolStep chooseAmount(double number)
    {
        switch (mode_) {
        case Mode::Delta:
            defaults_->delta = number;
            break;
        case Mode::Percent:
            if (number <= 0.0) {
                return ToolStep::rejected("a percentage length must be more than zero");
            }
            defaults_->percent = number;
            break;
        case Mode::Total:
        case Mode::Dynamic:
            if (number <= 0.0) {
                return ToolStep::rejected("a total length must be more than zero");
            }
            defaults_->total = number;
            break;
        }
        step_ = Step::Change;
        return ToolStep::next();
    }

    ToolStep change(EntityId id, const Entity& current, bool atStart, double length)
    {
        auto geometry = lengthened(current.geometry, atStart, length);
        if (!geometry) {
            return ToolStep::rejected(asSentence(geometry.error().message));
        }
        Entity piece = current;
        piece.geometry = std::move(*geometry);
        modify_edit::carryHeights(current, piece);
        session_.begin();
        if (const auto status = session_.replace(id, {std::move(piece)}); !status) {
            return ToolStep::rejected(asSentence(status.error().message));
        }
        return ToolStep::next("Length " + fixed(length));
    }

    ToolStep finish()
    {
        const std::size_t count = session_.editedIds().size();
        if (session_.operations() == 0) {
            return ToolStep::done(nullptr);
        }
        return ToolStep::done(session_.commit("LENGTHEN"),
                              "Lengthened " + everyday::counted(count, "object", "objects"));
    }

    EditSession session_;
    std::shared_ptr<LengthenDefaults> defaults_;
    Mode mode_ = Mode::Delta;
    Step step_ = Step::Measure;
    EntityId target_ = katana::entity::kInvalidEntityId;
    bool targetAtStart_ = false;
};

// ---- Reverse -----------------------------------------------------------------------

// `entity` drawn from its other end, heights and all; nullopt for a kind with
// no direction.
std::optional<Entity> reversed(const Entity& entity)
{
    Entity out = entity;
    std::size_t count = 0;
    if (const auto* line = std::get_if<Segment2>(&entity.geometry)) {
        out.geometry = Segment2{line->end, line->start};
        count = 2;
    } else if (const auto* arc = std::get_if<Arc2>(&entity.geometry)) {
        out.geometry = arc->reversed();
        count = 2;
    } else if (const auto* polyline = std::get_if<Polyline2>(&entity.geometry)) {
        Polyline2 turned = *polyline;
        std::ranges::reverse(turned.vertices);
        count = turned.vertices.size();
        out.geometry = std::move(turned);
    } else {
        return std::nullopt;
    }
    auto heights = katana::entity::heightsOf(entity.properties, count);
    // Rewritten only when there are heights: an entity with none keeps its
    // properties exactly as they were.
    if (std::ranges::any_of(heights, [](const auto& h) { return h.has_value(); })) {
        std::ranges::reverse(heights);
        katana::entity::setHeights(out.properties, heights);
    }
    return out;
}

class ReverseTool final : public InteractiveTool {
  public:
    explicit ReverseTool(const ToolContext& context)
        : document_(context.document), selection_(context.document)
    {
    }

    [[nodiscard]] std::string prompt() const override
    {
        return "Select lines, arcs and polylines to reverse or [All], then press Enter";
    }
    [[nodiscard]] ToolInput expects() const override { return ToolInput::Selection; }

    ToolStep entity(EntityId id, const Point2& /*at*/) override { return selection_.pick(id); }

    ToolStep value(std::string_view text) override
    {
        if (isOption(text, "All", "A")) {
            return selection_.all();
        }
        return ToolStep::rejected("'" + std::string(text) +
                                  "' is not an option here: pick entities, or type All");
    }

    ToolStep undo() override
    {
        return selection_.undo() ? ToolStep::next()
                                 : ToolStep::rejected("nothing to undo in this tool");
    }

    ToolStep enter() override
    {
        if (document_ == nullptr) {
            return ToolStep::rejected("there is no drawing");
        }
        const std::vector<EntityId> ids = selection_.chosen();
        if (ids.empty()) {
            return ToolStep::rejected(
                "nothing is selected: select the objects to reverse, then press Enter");
        }
        cmd::ChangeSet changes;
        // Why each skipped entity was skipped, counted by reason.
        std::map<std::string, std::size_t> skipped;
        for (const EntityId id : ids) {
            const Entity* entity = document_->model().entities.find(id);
            if (entity == nullptr) {
                continue;
            }
            if (modify_edit::refusalToEdit(*document_, id)) {
                ++skipped["on a locked layer"];
                continue;
            }
            if (auto turned = reversed(*entity)) {
                changes.modify.push_back(std::move(*turned));
            } else {
                ++skipped[kindName(entity->geometry) + " has no direction"];
            }
        }
        std::string notes;
        for (const auto& [reason, count] : skipped) {
            notes += "; " + std::to_string(count) + " skipped: " + reason;
        }
        if (changes.modify.empty()) {
            return ToolStep::rejected("nothing selected can be reversed" + notes);
        }
        const std::size_t count = changes.modify.size();
        auto command = std::make_unique<cmd::ChangeSetCommand>(
            "REVERSE", [changes = std::move(changes)](const cmd::CommandContext&) {
                return Result<cmd::ChangeSet>(changes);
            });
        return ToolStep::done(std::move(command),
                              "Reversed " + everyday::counted(count, "object", "objects") + notes);
    }

  private:
    const Document* document_ = nullptr;
    SelectionStep selection_;
};

} // namespace

void addModifyLengthTools(ToolCatalog& catalog, const Report& report)
{
    const auto add = [&](std::string id, std::string name, int order,
                         std::vector<std::string> aliases, std::string tip,
                         std::function<std::unique_ptr<InteractiveTool>(const ToolContext&)> make) {
        ToolInfo info;
        info.id = std::move(id);
        info.name = std::move(name);
        info.category = "Modify";
        info.group = "Edit";
        info.order = order;
        info.aliases = std::move(aliases);
        info.tip = std::move(tip);
        info.make = std::move(make);
        report(catalog.add(std::move(info)));
    };
    auto defaults = std::make_shared<LengthenDefaults>();
    add("modify.lengthen", "Lengthen", 100, {"LENGTHEN", "LEN"},
        "Changes the length of lines, arcs and open polylines at the end nearer the pick: DE by "
        "a delta, P to a percentage, T to a total, DY to the cursor; pick an object first to be "
        "told its length.",
        [defaults](const ToolContext& context) -> std::unique_ptr<InteractiveTool> {
            return std::make_unique<LengthenTool>(context, defaults);
        });
    add("modify.reverse", "Reverse", 110, {"REVERSE", "REV"},
        "Turns lines, arcs and polylines round so they run from the other end, with a 3D "
        "string's heights turned round too.",
        [](const ToolContext& context) -> std::unique_ptr<InteractiveTool> {
            return std::make_unique<ReverseTool>(context);
        });
}

} // namespace katana::cad::tools
