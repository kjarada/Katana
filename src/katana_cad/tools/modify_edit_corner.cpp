// Fillet and Chamfer: two lines, and the corner between them rounded by an arc
// or cut by a bevel. One tool with two modes, as they differ only in what goes
// into the corner and the values that size it. Each finishes after one corner,
// as AutoCAD's do; [Multiple] keeps going until Enter, as one command.
//
// Each line keeps the side the user PICKED it on. geometry::fillet keeps the
// end of each line farther from the corner, which is the same thing for lines
// that stop short of each other or just overshoot, but not for two lines that
// cross: there the pick is the only thing that says which quarter to keep, so
// the line is first cut at the corner on the picked side.

#include <cmath>

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
using katana::geometry::Line2;

enum class Kind { Fillet, Chamfer };

// The part of `line` on the picked side of `corner`: all of it when the corner
// is not inside it.
Segment2 pickedSide(const Segment2& line, const Point2& corner, const Point2& pick)
{
    const Line2 carrier{line.start, line.delta()};
    const double atCorner = carrier.parameterOf(corner);
    const double tiny = tol::kGeometric / line.length();
    if (atCorner <= tiny || atCorner >= 1.0 - tiny) {
        return line;
    }
    return carrier.parameterOf(pick) < atCorner ? Segment2{line.start, corner}
                                                : Segment2{corner, line.end};
}

struct Corner {
    Entity first;                 // the first line, rebuilt
    Entity second;                // the second line, rebuilt
    std::optional<Entity> joiner; // the fillet arc or chamfer bevel
};

class CornerTool final : public InteractiveTool {
  public:
    CornerTool(const ToolContext& context, Defaults defaults, Kind kind)
        : context_(context), defaults_(std::move(defaults)), kind_(kind),
          session_(context.document)
    {
    }

    [[nodiscard]] std::string prompt() const override
    {
        const bool fillet = kind_ == Kind::Fillet;
        switch (step_) {
        case Step::First: {
            std::string options = fillet ? "Radius" : "Distance";
            options += multiple_ ? (session_.operations() > 0 ? "/Undo" : "") : "/Multiple";
            return "Select first line or [" + options + "] (" + settings() + ")";
        }
        case Step::Second:
            return "Select second line or [Undo] (" + settings() + ")";
        case Step::Radius:
            return "Specify fillet radius <" + formatNumber(defaults_->filletRadius) + ">";
        case Step::FirstDistance:
            return "Specify first chamfer distance <" + formatNumber(defaults_->chamferFirst) +
                   ">";
        case Step::SecondDistance:
            return "Specify second chamfer distance <" + formatNumber(typedFirst_) + ">";
        }
        return {};
    }

    [[nodiscard]] ToolInput expects() const override
    {
        return step_ == Step::First || step_ == Step::Second ? ToolInput::Entity
                                                             : ToolInput::Value;
    }

    ToolStep entity(EntityId id, const Point2& at) override
    {
        if (step_ != Step::First && step_ != Step::Second) {
            return InteractiveTool::entity(id, at);
        }
        if (auto refusal = refusalToEdit(session_.document(), id)) {
            return ToolStep::rejected(*refusal);
        }
        const auto pieces = session_.pieces(id);
        if (pieces.size() != 1 || !std::holds_alternative<Segment2>(pieces.front().geometry)) {
            const std::string kind =
                pieces.empty() ? std::string("object") : kindName(pieces.front().geometry);
            const std::string verb = kind_ == Kind::Fillet ? "filleted" : "chamfered";
            if (kind == "polyline" || kind == "closed polyline") {
                return ToolStep::rejected("Only lines can be " + verb +
                                          "; explode the polyline into lines first.");
            }
            return ToolStep::rejected("Only lines can be " + verb + ", and that is " +
                                      withArticle(kind) + ".");
        }
        if (step_ == Step::First) {
            first_ = id;
            firstPick_ = at;
            step_ = Step::Second;
            return ToolStep::next();
        }
        if (id == first_) {
            return ToolStep::rejected("Pick a different line for the second.");
        }
        auto corner = attempt(id, at);
        if (!corner) {
            return ToolStep::rejected(corner.error().message);
        }
        session_.begin();
        katana::core::Status taken = session_.replace(first_, {corner->first});
        if (taken) {
            taken = session_.replace(id, {corner->second});
        }
        if (taken && corner->joiner) {
            taken = session_.add(std::move(*corner->joiner));
        }
        if (!taken) {
            return ToolStep::rejected(taken.error().message);
        }
        step_ = Step::First;
        if (multiple_) {
            return ToolStep::next();
        }
        return finish();
    }

    ToolStep value(std::string_view text) override
    {
        if (isOption(text, "Undo", "U")) {
            return undo();
        }
        const bool fillet = kind_ == Kind::Fillet;
        switch (step_) {
        case Step::First:
            if (fillet && isOption(text, "Radius", "R")) {
                step_ = Step::Radius;
                return ToolStep::next();
            }
            if (!fillet && isOption(text, "Distance", "D")) {
                step_ = Step::FirstDistance;
                return ToolStep::next();
            }
            if (!multiple_ && isOption(text, "Multiple", "M")) {
                multiple_ = true;
                return ToolStep::next(std::string("Multiple: ") +
                                      (fillet ? "fillet" : "chamfer") +
                                      " corners until Enter.");
            }
            // A number here is the radius or the first distance, typed
            // without the option letter first.
            if (const auto number = parseNumber(text)) {
                if (*number < 0.0) {
                    return ToolStep::rejected(fillet ? "The fillet radius cannot be negative."
                                                     : "A chamfer distance cannot be negative.");
                }
                if (fillet) {
                    defaults_->filletRadius = *number;
                    return ToolStep::next();
                }
                typedFirst_ = *number;
                step_ = Step::SecondDistance;
                return ToolStep::next();
            }
            return ToolStep::rejected(fillet ? "Pick the first line, or type R to set the radius."
                                             : "Pick the first line, or type D to set the "
                                               "distances.");
        case Step::Second:
            return ToolStep::rejected("Pick the second line, or type U to pick the first again.");
        case Step::Radius: {
            const auto radius = parseNumber(text);
            if (!radius) {
                return ToolStep::rejected("Type the fillet radius as a number.");
            }
            if (*radius < 0.0) {
                return ToolStep::rejected("The fillet radius cannot be negative.");
            }
            defaults_->filletRadius = *radius;
            step_ = Step::First;
            return ToolStep::next();
        }
        case Step::FirstDistance:
        case Step::SecondDistance: {
            const auto distance = parseNumber(text);
            if (!distance) {
                return ToolStep::rejected("Type the chamfer distance as a number.");
            }
            if (*distance < 0.0) {
                return ToolStep::rejected("A chamfer distance cannot be negative.");
            }
            return chooseDistance(*distance);
        }
        }
        return InteractiveTool::value(text);
    }

    // Esc keeps what the session has done: its Enter only ever commits.
    ToolStep cancel() override { return keepWorkOnEscape(*this); }

    ToolStep enter() override
    {
        switch (step_) {
        case Step::Radius:
            step_ = Step::First; // keep the radius
            return ToolStep::next();
        case Step::FirstDistance:
            return chooseDistance(defaults_->chamferFirst);
        case Step::SecondDistance:
            return chooseDistance(typedFirst_);
        case Step::First:
        case Step::Second:
            break;
        }
        if (session_.operations() == 0) {
            return ToolStep::done(nullptr);
        }
        return finish();
    }

    ToolStep undo() override
    {
        switch (step_) {
        case Step::Second:
            step_ = Step::First;
            return ToolStep::next();
        case Step::First:
            if (session_.undo()) {
                return ToolStep::next(kind_ == Kind::Fillet ? "Undid the last fillet."
                                                            : "Undid the last chamfer.");
            }
            return ToolStep::rejected("Nothing to undo.");
        case Step::Radius:
        case Step::FirstDistance:
            step_ = Step::First;
            return ToolStep::next();
        case Step::SecondDistance:
            step_ = Step::FirstDistance;
            return ToolStep::next();
        }
        return ToolStep::rejected("Nothing to undo.");
    }

    [[nodiscard]] ToolFeedback preview(const Point2& cursor) const override
    {
        ToolFeedback feedback;
        feedback.shapes = session_.shapes();
        if (step_ != Step::Second) {
            return feedback;
        }
        // The corner a click on the line under the cursor would make.
        const auto under = pickEntity(session_.document().model(), cursor,
                                      context_.pickTolerance, {},
                                      &session_.document().spatialIndex());
        if (under && *under != first_) {
            if (auto corner = attempt(*under, cursor)) {
                feedback.shapes.push_back(corner->first.geometry);
                feedback.shapes.push_back(corner->second.geometry);
                if (corner->joiner) {
                    feedback.shapes.push_back(corner->joiner->geometry);
                }
            }
        }
        return feedback;
    }

  private:
    enum class Step { First, Second, Radius, FirstDistance, SecondDistance };

    [[nodiscard]] std::string settings() const
    {
        if (kind_ == Kind::Fillet) {
            return "radius " + formatNumber(defaults_->filletRadius);
        }
        return "distances " + formatNumber(defaults_->chamferFirst) + ", " +
               formatNumber(defaults_->chamferSecond);
    }

    ToolStep chooseDistance(double distance)
    {
        if (step_ == Step::FirstDistance) {
            typedFirst_ = distance;
            step_ = Step::SecondDistance;
            return ToolStep::next();
        }
        defaults_->chamferFirst = typedFirst_;
        defaults_->chamferSecond = distance;
        step_ = Step::First;
        return ToolStep::next();
    }

    ToolStep finish()
    {
        const std::size_t count = session_.operations();
        const bool fillet = kind_ == Kind::Fillet;
        std::string message = fillet ? "Filleted " : "Chamfered ";
        message += count == 1 ? "1 corner" : std::to_string(count) + " corners";
        message += fillet ? " with radius " + formatNumber(defaults_->filletRadius) + "."
                          : " at " + formatNumber(defaults_->chamferFirst) + " and " +
                                formatNumber(defaults_->chamferSecond) + ".";
        return ToolStep::done(session_.commit(fillet ? "FILLET" : "CHAMFER"), std::move(message));
    }

    // The corner of the first line and `id` picked at `at`, without making it.
    [[nodiscard]] Result<Corner> attempt(EntityId id, const Point2& at) const
    {
        const auto firstPieces = session_.pieces(first_);
        const auto secondPieces = session_.pieces(id);
        if (firstPieces.size() != 1 || secondPieces.size() != 1) {
            return makeError(ErrorCode::InvalidState, "Pick two lines.");
        }
        const Entity& a = firstPieces.front();
        const Entity& b = secondPieces.front();
        const auto* lineA = std::get_if<Segment2>(&a.geometry);
        const auto* lineB = std::get_if<Segment2>(&b.geometry);
        if (lineA == nullptr || lineB == nullptr) {
            return makeError(ErrorCode::Unsupported, "Pick two lines.");
        }
        const bool fillet = kind_ == Kind::Fillet;
        const auto meet = katana::geometry::intersect(Line2{lineA->start, lineA->delta()},
                                                      Line2{lineB->start, lineB->delta()});
        if (meet.kind != IntersectionKind::Points) {
            return makeError(ErrorCode::InvalidGeometry,
                             std::string("The lines are parallel, so there is no corner to ") +
                                 (fillet ? "fillet." : "chamfer."));
        }
        const Segment2 keptA = pickedSide(*lineA, meet.points[0], firstPick_);
        const Segment2 keptB = pickedSide(*lineB, meet.points[0], at);

        Corner corner{a, b, std::nullopt};
        if (fillet) {
            auto made = katana::geometry::fillet(keptA, keptB, defaults_->filletRadius);
            if (!made) {
                return makeError(ErrorCode::InvalidGeometry,
                                 made.error().message.find("too large") != std::string::npos
                                     ? "A radius of " + formatNumber(defaults_->filletRadius) +
                                           " is too large for these lines."
                                     : asSentence(made.error().message));
            }
            corner.first.geometry = made->first;
            corner.second.geometry = made->second;
            if (made->arc) {
                corner.joiner = a; // as filletEntities: the arc wears the first line's attributes
                corner.joiner->geometry = *made->arc;
            }
        } else {
            auto made = katana::geometry::chamfer(keptA, keptB, defaults_->chamferFirst,
                                                  defaults_->chamferSecond);
            if (!made) {
                return makeError(ErrorCode::InvalidGeometry,
                                 made.error().message.find("leaves no segment") !=
                                         std::string::npos
                                     ? std::string("The chamfer distances are too large for these "
                                                   "lines.")
                                     : asSentence(made.error().message));
            }
            corner.first.geometry = made->first;
            corner.second.geometry = made->second;
            if (made->bevel) {
                corner.joiner = a;
                corner.joiner->geometry = *made->bevel;
            }
        }
        carryHeights(a, corner.first);
        carryHeights(b, corner.second);
        if (corner.joiner) {
            // The joiner starts on the first line and ends on the second, so
            // each end takes its height from the line it touches.
            const auto ends = heightVertices(corner.joiner->geometry);
            katana::entity::setHeights(corner.joiner->properties,
                                       {heightAt(a, ends.front()), heightAt(b, ends.back())});
        }
        return corner;
    }

    ToolContext context_;
    Defaults defaults_;
    Kind kind_;
    EditSession session_;
    Step step_ = Step::First;
    bool multiple_ = false;
    EntityId first_ = katana::entity::kInvalidEntityId;
    Point2 firstPick_;
    double typedFirst_ = 0.0; // the first chamfer distance, while the second is asked
};

} // namespace

ToolPtr makeFilletTool(const ToolContext& context, Defaults defaults)
{
    return std::make_unique<CornerTool>(context, std::move(defaults), Kind::Fillet);
}

ToolPtr makeChamferTool(const ToolContext& context, Defaults defaults)
{
    return std::make_unique<CornerTool>(context, std::move(defaults), Kind::Chamfer);
}

} // namespace katana::cad::tools::modify_edit
