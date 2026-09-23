// Move, Copy, Rotate, Scale, Mirror, Stretch, Array (rectangular and polar)
// and Erase: the tools that act on a selection as a whole (see families.hpp).
//
// Each follows its AutoCAD namesake's dialogue, because that is what a CAD
// user's hands already know: the same prompts, the same option letters, the
// same defaults in <angle brackets>, Enter to accept one. Where AutoCAD and
// the command interpreter's verb of the same name differ, the interpreter
// wins on MEANING (ARRAY is rectangular there, so it is here) and AutoCAD on
// DIALOGUE (Mirror keeps the source unless told otherwise).
//
// A tool never touches the document. It reads the selection's geometry from
// it for the rubber band and for a few decisions (what a window catches, the
// middle of what a polar array turns), and when it completes it returns ONE
// command, so one undo takes the whole operation back - a Copy to five
// places included.

#include <algorithm>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <memory>
#include <optional>
#include <span>
#include <string>
#include <string_view>
#include <utility>
#include <variant>
#include <vector>

#include "families.hpp"
#include "katana/cad/layer_overrides.hpp"
#include "katana/cad/selection.hpp"
#include "katana/commands/change_set.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/entity_geometry.hpp"
#include "katana/math/mat3.hpp"
#include "katana/math/numerics.hpp"

namespace katana::cad::tools {

namespace {

namespace cmd = katana::commands;

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::entity::Geometry;
using katana::geometry::Arc2;
using katana::geometry::Box2;
using katana::geometry::Circle2;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;
using katana::geometry::Vec2;
using katana::math::Mat3;
using katana::math::kDegToRad;
namespace tolerance = katana::math::tolerance;

// The rubber band is redrawn on every mouse move, so it shows at most this many
// shapes; a selection bigger than that is shown by its base point and the band
// alone. A thousand lines is far more than the eye follows while dragging, and
// few enough that transforming them per move costs nothing noticeable.
constexpr std::size_t kPreviewLimit = 1000;

// The array command's own guard against a runaway typo (entity_commands.cpp).
// The tools refuse the same count up front, with a sentence, rather than
// handing the document a command it will reject.
constexpr std::int64_t kMaximumArrayItems = 100000;

// ---- small helpers -----------------------------------------------------------------

std::string counted(std::size_t count)
{
    return std::to_string(count) + (count == 1 ? " entity" : " entities");
}

std::optional<double> numberIn(std::string_view text)
{
    return katana::core::parseFiniteDouble(katana::core::trimmed(text));
}

std::optional<std::int64_t> integerIn(std::string_view text)
{
    return katana::core::parseInteger(katana::core::trimmed(text));
}

// An option as a prompt offers it: its key letters or the whole word, in any
// case - "C" or "copy" for [Copy].
bool isOption(std::string_view text, std::string_view key, std::string_view word)
{
    const std::string_view input = katana::core::trimmed(text);
    return katana::core::equalsIgnoringCase(input, key) ||
           katana::core::equalsIgnoringCase(input, word);
}

bool coincident(const Point2& a, const Point2& b)
{
    return a.distanceTo(b) <= tolerance::kGeometric;
}

ToolStep notUnderstood(std::string_view text, std::string_view wanted)
{
    return ToolStep::rejected("'" + std::string(katana::core::trimmed(text)) + "' is not " +
                              std::string(wanted));
}

// AutoCAD's direct distance entry: a bare number where a point is wanted means
// that far from `from` towards the cursor. The direction is the rubber band's,
// which is why the tools remember the last cursor they previewed.
std::optional<Point2> alongCursor(double distance, const Point2& from,
                                  const std::optional<Point2>& cursor)
{
    if (!cursor || coincident(*cursor, from)) {
        return std::nullopt;
    }
    return from + (*cursor - from).normalized() * distance;
}

constexpr std::string_view kNoDirection =
    "a typed distance goes towards the cursor: move the cursor off the base point first, "
    "or type @dx,dy";

// Copies of `ids`, one set per transform, as ONE command: Copy to several
// places, Rotate and Scale with their Copy option, and the polar array all
// come to this. The same construction as duplicateEach in entity_commands.cpp,
// which is private there; a copy keeps everything but its id, which the model
// assigns.
cmd::CommandPtr duplicated(std::string name, std::vector<EntityId> ids,
                           std::vector<Mat3> transforms)
{
    return std::make_unique<cmd::ChangeSetCommand>(
        std::move(name),
        [ids = std::move(ids), transforms = std::move(transforms)](
            const cmd::CommandContext& context) -> Result<cmd::ChangeSet> {
            if (ids.empty() || transforms.empty()) {
                return makeError(ErrorCode::InvalidArgument, "nothing to copy");
            }
            cmd::ChangeSet changes;
            for (const Mat3& transform : transforms) {
                for (const EntityId id : ids) {
                    const Entity* source = context.model.entities.find(id);
                    if (source == nullptr) {
                        return makeError(ErrorCode::NotFound, "entity does not exist",
                                         "id=" + std::to_string(id));
                    }
                    auto geometry = katana::entity::transformed(source->geometry, transform);
                    if (!geometry) {
                        return makeError(geometry.error().code, geometry.error().message,
                                         "id=" + std::to_string(id));
                    }
                    Entity copy = *source;
                    copy.geometry = std::move(*geometry);
                    changes.add.push_back(std::move(copy));
                }
            }
            return changes;
        });
}

// The selection's geometry through each transform, into the rubber band, up to
// the preview limit.
void addTransformed(ToolFeedback& feedback, const Document* document,
                    const std::vector<EntityId>& ids, std::span<const Mat3> transforms)
{
    if (document == nullptr || ids.size() * transforms.size() > kPreviewLimit) {
        return;
    }
    for (const Mat3& transform : transforms) {
        for (const EntityId id : ids) {
            const Entity* entity = document->model().entities.find(id);
            if (entity == nullptr) {
                continue;
            }
            if (auto moved = katana::entity::transformed(entity->geometry, transform)) {
                feedback.shapes.push_back(std::move(*moved));
            }
        }
    }
}

void addTransformed(ToolFeedback& feedback, const Document* document,
                    const std::vector<EntityId>& ids, const Mat3& transform)
{
    addTransformed(feedback, document, ids, std::span<const Mat3>(&transform, 1));
}

// A tool's undo history: the state before each accepted input. Undo inside a
// tool puts the previous one back, so every step - a picked point, an option
// switched on - is undone the same way and none can be forgotten.
template <typename State>
class History {
  public:
    State now{};

    void remember() { past_.push_back(now); }
    bool back()
    {
        if (past_.empty()) {
            return false;
        }
        now = std::move(past_.back());
        past_.pop_back();
        return true;
    }

  private:
    std::vector<State> past_;
};

// ---- the selection every tool here acts on ------------------------------------------
//
// Taken from the context when there is one. Otherwise the tool asks for it
// first (ToolInput::Selection): the view selects as the Select tool does, and
// may also hand single picks to entity(); Enter takes both - the document's
// selection at that moment and the picks - so the tool works whichever way the
// view reports them. "All" selects everything selectable.
class SelectionTool : public InteractiveTool {
  public:
    // `verb` completes "Select entities to ...". A tool that `confirms` starts
    // in the selection step even when there is a selection, holding it, so one
    // Enter carries it out: Erase, which has nothing else to ask.
    SelectionTool(const ToolContext& context, std::string verb, bool confirms = false)
        : document_(context.document), verb_(std::move(verb))
    {
        if (context.selection.empty() || confirms) {
            selecting_ = true;
            asked_ = true;
            picked_ = context.selection;
        } else {
            ids_ = context.selection;
        }
    }

    [[nodiscard]] std::string prompt() const final
    {
        return selecting_ ? "Select entities to " + verb_ + " or [All], then press Enter"
                          : stepPrompt();
    }

    [[nodiscard]] ToolInput expects() const final
    {
        return selecting_ ? ToolInput::Selection : stepExpects();
    }

    [[nodiscard]] ToolStep point(const Point2& at) final
    {
        if (selecting_) {
            return ToolStep::rejected("select the entities to " + verb_ +
                                      " first, then press Enter");
        }
        return stepPoint(at);
    }

    [[nodiscard]] ToolStep entity(EntityId id, const Point2& at) final
    {
        if (!selecting_) {
            return InteractiveTool::entity(id, at);
        }
        if (document_ == nullptr || !document_->model().entities.contains(id)) {
            return ToolStep::rejected("that entity is not in the drawing");
        }
        if (std::ranges::find(picked_, id) != picked_.end()) {
            return ToolStep::rejected("that entity is already selected");
        }
        pickHistory_.push_back(picked_);
        picked_.push_back(id);
        return ToolStep::next();
    }

    [[nodiscard]] ToolStep value(std::string_view text) final
    {
        if (!selecting_) {
            return stepValue(text);
        }
        if (!isOption(text, "A", "ALL")) {
            return notUnderstood(text, "an option here: pick entities, or type All");
        }
        if (document_ == nullptr) {
            return ToolStep::rejected("there is no drawing to select from");
        }
        std::vector<EntityId> all;
        const katana::entity::Model& model = document_->model();
        model.entities.forEach([&](const Entity& candidate) {
            if (isSelectable(model, candidate, kNoLayerOverrides)) {
                all.push_back(candidate.id);
            }
        });
        pickHistory_.push_back(picked_);
        picked_ = std::move(all);
        return ToolStep::next(counted(picked_.size()) + " selected");
    }

    [[nodiscard]] ToolStep enter() final
    {
        if (!selecting_) {
            return stepEnter();
        }
        std::vector<EntityId> chosen = picked_;
        if (document_ != nullptr) {
            for (const EntityId id : document_->selection().ids()) {
                chosen.push_back(id);
            }
            std::erase_if(chosen, [&](EntityId id) {
                return !document_->model().entities.contains(id);
            });
        }
        std::ranges::sort(chosen);
        chosen.erase(std::unique(chosen.begin(), chosen.end()), chosen.end());
        if (chosen.empty()) {
            return ToolStep::rejected("nothing is selected: select the entities to " + verb_ +
                                      ", then press Enter");
        }
        ids_ = std::move(chosen);
        selecting_ = false;
        return selected();
    }

    [[nodiscard]] ToolStep undo() final
    {
        if (selecting_) {
            if (pickHistory_.empty()) {
                return ToolStep::rejected("nothing to undo in this tool");
            }
            picked_ = std::move(pickHistory_.back());
            pickHistory_.pop_back();
            return ToolStep::next();
        }
        if (stepUndo()) {
            return ToolStep::next();
        }
        if (asked_) {
            // Back to the selection the tool asked for, with the picks kept.
            selecting_ = true;
            ids_.clear();
            return ToolStep::next();
        }
        return ToolStep::rejected("nothing to undo in this tool");
    }

    [[nodiscard]] ToolFeedback preview(const Point2& cursor) const final
    {
        cursor_ = cursor;
        return selecting_ ? ToolFeedback{} : stepPreview(cursor);
    }

    [[nodiscard]] std::optional<Point2> lastPoint() const final
    {
        return selecting_ ? std::nullopt : stepLastPoint();
    }

  protected:
    // After the selection: the tool's own dialogue.
    [[nodiscard]] virtual std::string stepPrompt() const = 0;
    [[nodiscard]] virtual ToolInput stepExpects() const { return ToolInput::Point; }
    [[nodiscard]] virtual ToolStep stepPoint(const Point2& at)
    {
        return InteractiveTool::point(at);
    }
    [[nodiscard]] virtual ToolStep stepValue(std::string_view text)
    {
        return InteractiveTool::value(text);
    }
    [[nodiscard]] virtual ToolStep stepEnter() { return InteractiveTool::enter(); }
    // Steps back one input; false when the tool is at its first step.
    [[nodiscard]] virtual bool stepUndo() { return false; }
    [[nodiscard]] virtual ToolFeedback stepPreview(const Point2& /*cursor*/) const { return {}; }
    [[nodiscard]] virtual std::optional<Point2> stepLastPoint() const { return std::nullopt; }
    // The selection is complete. Erase finishes here; the others go on.
    [[nodiscard]] virtual ToolStep selected()
    {
        return ToolStep::next(counted(ids_.size()) + " selected");
    }

    [[nodiscard]] const std::vector<EntityId>& ids() const { return ids_; }
    [[nodiscard]] const Document* document() const { return document_; }
    // The last cursor the view previewed, for direct distance entry. preview()
    // is the only place a tool sees the cursor, and it is const, hence mutable.
    [[nodiscard]] const std::optional<Point2>& cursor() const { return cursor_; }

    void addPreview(ToolFeedback& feedback, const Mat3& transform) const
    {
        addTransformed(feedback, document_, ids_, transform);
    }

  private:
    const Document* document_ = nullptr;
    std::string verb_;
    bool selecting_ = false;
    bool asked_ = false;
    std::vector<EntityId> ids_;
    std::vector<EntityId> picked_;
    std::vector<std::vector<EntityId>> pickHistory_;
    mutable std::optional<Point2> cursor_;
};

// The rubber band line from a base point to the cursor, with the base marked.
void addBand(ToolFeedback& feedback, const Point2& from, const Point2& cursor)
{
    feedback.markers.push_back(from);
    if (!coincident(from, cursor)) {
        feedback.shapes.emplace_back(Segment2{from, cursor});
    }
}

// ---- Move ----------------------------------------------------------------------------

// Where Move, Copy and Stretch are told to go: a base point then a second point,
// or a displacement typed as dx,dy. Enter at the base point asks for the
// displacement, and Enter at the second point uses the base point AS the
// displacement - AutoCAD's two defaults.
enum class Displace { Base, Typed, Second };

std::string displacementPrompt(Displace step)
{
    switch (step) {
    case Displace::Base:
        return "Specify base point or [Displacement] <Displacement>";
    case Displace::Typed:
        return "Specify displacement as dx,dy";
    case Displace::Second:
        return "Specify second point or <use first point as displacement>";
    }
    return {};
}

class MoveTool final : public SelectionTool {
  public:
    explicit MoveTool(const ToolContext& context) : SelectionTool(context, "move") {}

  private:
    struct State {
        Displace step = Displace::Base;
        Point2 base;
    };

    [[nodiscard]] std::string stepPrompt() const override
    {
        return displacementPrompt(state_.now.step);
    }

    [[nodiscard]] ToolStep stepPoint(const Point2& at) override
    {
        switch (state_.now.step) {
        case Displace::Base:
            state_.remember();
            state_.now.base = at;
            state_.now.step = Displace::Second;
            return ToolStep::next();
        case Displace::Typed:
            return finish(at - Point2());
        case Displace::Second:
            return finish(at - state_.now.base);
        }
        return ToolStep::rejected("unexpected point");
    }

    [[nodiscard]] ToolStep stepValue(std::string_view text) override
    {
        if (state_.now.step == Displace::Base && isOption(text, "D", "DISPLACEMENT")) {
            state_.remember();
            state_.now.step = Displace::Typed;
            return ToolStep::next();
        }
        if (state_.now.step == Displace::Second) {
            if (const auto distance = numberIn(text)) {
                const auto to = alongCursor(*distance, state_.now.base, cursor());
                if (!to) {
                    return ToolStep::rejected(std::string(kNoDirection));
                }
                return finish(*to - state_.now.base);
            }
        }
        return notUnderstood(text, "a point, a distance or an option here");
    }

    [[nodiscard]] ToolStep stepEnter() override
    {
        switch (state_.now.step) {
        case Displace::Base:
            state_.remember();
            state_.now.step = Displace::Typed;
            return ToolStep::next();
        case Displace::Typed:
            return ToolStep::rejected("type the displacement as dx,dy");
        case Displace::Second:
            return finish(state_.now.base - Point2());
        }
        return ToolStep::rejected("unexpected Enter");
    }

    [[nodiscard]] bool stepUndo() override { return state_.back(); }

    [[nodiscard]] ToolFeedback stepPreview(const Point2& cursor) const override
    {
        ToolFeedback feedback;
        if (state_.now.step == Displace::Second) {
            addPreview(feedback, Mat3::translation(cursor - state_.now.base));
            addBand(feedback, state_.now.base, cursor);
        }
        return feedback;
    }

    [[nodiscard]] std::optional<Point2> stepLastPoint() const override
    {
        switch (state_.now.step) {
        case Displace::Typed:
            // "@3,4" typed as a displacement means 3,4 from nowhere in particular.
            return Point2();
        case Displace::Second:
            return state_.now.base;
        case Displace::Base:
            break;
        }
        return std::nullopt;
    }

    ToolStep finish(const Vec2& delta)
    {
        if (delta.length() <= tolerance::kGeometric) {
            return ToolStep::rejected("a displacement of zero would move nothing");
        }
        return ToolStep::done(cmd::moveEntities(ids(), delta), counted(ids().size()) + " moved");
    }

    History<State> state_;
};

// ---- Copy ----------------------------------------------------------------------------

// One base point, then as many second points as the user wants, each a copy;
// Enter ends. ALL the copies are one command, so one undo removes them all -
// the old Copy tool made one command per copy.
class CopyTool final : public SelectionTool {
  public:
    explicit CopyTool(const ToolContext& context) : SelectionTool(context, "copy") {}

  private:
    struct State {
        Displace step = Displace::Base;
        Point2 base;
        std::vector<Vec2> offsets; // one per copy placed so far
    };

    [[nodiscard]] std::string stepPrompt() const override
    {
        if (state_.now.step == Displace::Second && !state_.now.offsets.empty()) {
            return "Specify second point or [Exit/Undo] <Exit>";
        }
        return displacementPrompt(state_.now.step);
    }

    [[nodiscard]] ToolStep stepPoint(const Point2& at) override
    {
        switch (state_.now.step) {
        case Displace::Base:
            state_.remember();
            state_.now.base = at;
            state_.now.step = Displace::Second;
            return ToolStep::next();
        case Displace::Typed: {
            const Vec2 offset = at - Point2();
            if (offset.length() <= tolerance::kGeometric) {
                return ToolStep::rejected(
                    "a displacement of zero would put the copy on the original");
            }
            return finish({offset});
        }
        case Displace::Second:
            return place(at);
        }
        return ToolStep::rejected("unexpected point");
    }

    [[nodiscard]] ToolStep stepValue(std::string_view text) override
    {
        const Displace step = state_.now.step;
        if (step == Displace::Base && isOption(text, "D", "DISPLACEMENT")) {
            state_.remember();
            state_.now.step = Displace::Typed;
            return ToolStep::next();
        }
        if (step == Displace::Second) {
            if (isOption(text, "E", "EXIT")) {
                return exit();
            }
            if (isOption(text, "U", "UNDO")) {
                return state_.back() ? ToolStep::next()
                                     : ToolStep::rejected("nothing to undo in this tool");
            }
            if (const auto distance = numberIn(text)) {
                const auto to = alongCursor(*distance, state_.now.base, cursor());
                if (!to) {
                    return ToolStep::rejected(std::string(kNoDirection));
                }
                return place(*to);
            }
        }
        return notUnderstood(text, "a point, a distance or an option here");
    }

    [[nodiscard]] ToolStep stepEnter() override
    {
        switch (state_.now.step) {
        case Displace::Base:
            state_.remember();
            state_.now.step = Displace::Typed;
            return ToolStep::next();
        case Displace::Typed:
            return ToolStep::rejected("type the displacement as dx,dy");
        case Displace::Second:
            if (state_.now.offsets.empty()) {
                const Vec2 offset = state_.now.base - Point2();
                if (offset.length() <= tolerance::kGeometric) {
                    return ToolStep::rejected(
                        "the base point is the origin, so as a displacement it would put the "
                        "copy on the original");
                }
                return finish({offset});
            }
            return exit();
        }
        return ToolStep::rejected("unexpected Enter");
    }

    [[nodiscard]] bool stepUndo() override { return state_.back(); }

    [[nodiscard]] ToolFeedback stepPreview(const Point2& cursor) const override
    {
        ToolFeedback feedback;
        if (state_.now.step != Displace::Second) {
            return feedback;
        }
        // The copies placed so far are not in the drawing until Enter, so the
        // rubber band shows them too, or the user would lose sight of them.
        std::vector<Mat3> transforms;
        for (const Vec2& offset : state_.now.offsets) {
            transforms.push_back(Mat3::translation(offset));
        }
        transforms.push_back(Mat3::translation(cursor - state_.now.base));
        addTransformed(feedback, document(), ids(), transforms);
        addBand(feedback, state_.now.base, cursor);
        return feedback;
    }

    [[nodiscard]] std::optional<Point2> stepLastPoint() const override
    {
        switch (state_.now.step) {
        case Displace::Typed:
            return Point2();
        case Displace::Second:
            return state_.now.base;
        case Displace::Base:
            break;
        }
        return std::nullopt;
    }

    ToolStep place(const Point2& at)
    {
        if (coincident(at, state_.now.base)) {
            return ToolStep::rejected(
                "the second point is the base point: the copy would lie on the original");
        }
        state_.remember();
        state_.now.offsets.push_back(at - state_.now.base);
        return ToolStep::next();
    }

    ToolStep exit()
    {
        if (state_.now.offsets.empty()) {
            return ToolStep::done(nullptr, "nothing copied");
        }
        return finish(state_.now.offsets);
    }

    ToolStep finish(const std::vector<Vec2>& offsets)
    {
        std::vector<Mat3> transforms;
        for (const Vec2& offset : offsets) {
            transforms.push_back(Mat3::translation(offset));
        }
        const std::string copies =
            std::to_string(offsets.size()) + (offsets.size() == 1 ? " copy" : " copies");
        return ToolStep::done(duplicated("COPY", ids(), std::move(transforms)),
                              copies + " of " + counted(ids().size()));
    }

    History<State> state_;
};

// ---- Rotate --------------------------------------------------------------------------

// A base point, then the angle - typed in degrees counter-clockwise, or picked
// as the direction from the base point. Reference: the angle the selection is
// at now (typed, or two points along it), then the angle it should be at
// (typed, picked from the base point, or two Points); it turns by the
// difference - "make this edge run due north". Copy rotates a copy instead.
class RotateTool final : public SelectionTool {
  public:
    explicit RotateTool(const ToolContext& context) : SelectionTool(context, "rotate") {}

  private:
    enum class Step { Base, Angle, ReferenceFirst, ReferenceSecond, NewAngle, NewFirst, NewSecond };
    struct State {
        Step step = Step::Base;
        Point2 base;
        bool copy = false;
        double reference = 0.0; // radians
        Point2 first;           // of a two-point reference or new angle
    };

    [[nodiscard]] std::string stepPrompt() const override
    {
        switch (state_.now.step) {
        case Step::Base:
            return "Specify base point";
        case Step::Angle:
            return "Specify rotation angle or [Copy/Reference]";
        case Step::ReferenceFirst:
            return "Specify the reference angle or its first point <0>";
        case Step::ReferenceSecond:
            return "Specify second point of the reference angle";
        case Step::NewAngle:
            return "Specify the new angle or [Points]";
        case Step::NewFirst:
            return "Specify first point of the new angle";
        case Step::NewSecond:
            return "Specify second point of the new angle";
        }
        return {};
    }

    [[nodiscard]] ToolStep stepPoint(const Point2& at) override
    {
        State& now = state_.now;
        switch (now.step) {
        case Step::Base:
            return advance([&](State& next) {
                next.base = at;
                next.step = Step::Angle;
            });
        case Step::Angle:
            if (coincident(at, now.base)) {
                return ToolStep::rejected("the point is on the base point, so it gives no angle");
            }
            return finish((at - now.base).angle());
        case Step::ReferenceFirst:
            return advance([&](State& next) {
                next.first = at;
                next.step = Step::ReferenceSecond;
            });
        case Step::ReferenceSecond:
            if (coincident(at, now.first)) {
                return ToolStep::rejected("the two points coincide, so they give no angle");
            }
            return advance([&](State& next) {
                next.reference = (at - next.first).angle();
                next.step = Step::NewAngle;
            });
        case Step::NewAngle:
            if (coincident(at, now.base)) {
                return ToolStep::rejected("the point is on the base point, so it gives no angle");
            }
            return finish((at - now.base).angle() - now.reference);
        case Step::NewFirst:
            return advance([&](State& next) {
                next.first = at;
                next.step = Step::NewSecond;
            });
        case Step::NewSecond:
            if (coincident(at, now.first)) {
                return ToolStep::rejected("the two points coincide, so they give no angle");
            }
            return finish((at - now.first).angle() - now.reference);
        }
        return ToolStep::rejected("unexpected point");
    }

    [[nodiscard]] ToolStep stepValue(std::string_view text) override
    {
        State& now = state_.now;
        const auto degrees = numberIn(text);
        switch (now.step) {
        case Step::Angle:
            if (degrees) {
                return finish(*degrees * kDegToRad);
            }
            if (isOption(text, "C", "COPY")) {
                state_.remember();
                now.copy = !now.copy;
                return ToolStep::next(now.copy ? "Rotating a copy of the selection."
                                               : "Rotating the selection itself.");
            }
            if (isOption(text, "R", "REFERENCE")) {
                return advance([](State& next) { next.step = Step::ReferenceFirst; });
            }
            break;
        case Step::ReferenceFirst:
            if (degrees) {
                return advance([&](State& next) {
                    next.reference = *degrees * kDegToRad;
                    next.step = Step::NewAngle;
                });
            }
            break;
        case Step::NewAngle:
            if (degrees) {
                return finish(*degrees * kDegToRad - now.reference);
            }
            if (isOption(text, "P", "POINTS")) {
                return advance([](State& next) { next.step = Step::NewFirst; });
            }
            break;
        case Step::Base:
        case Step::ReferenceSecond:
        case Step::NewFirst:
        case Step::NewSecond:
            break;
        }
        return notUnderstood(text, "an angle, a point or an option here");
    }

    [[nodiscard]] ToolStep stepEnter() override
    {
        if (state_.now.step == Step::ReferenceFirst) {
            // <0>: the reference is due east.
            return advance([](State& next) {
                next.reference = 0.0;
                next.step = Step::NewAngle;
            });
        }
        return ToolStep::rejected("this step has no default: " + stepPrompt());
    }

    [[nodiscard]] bool stepUndo() override { return state_.back(); }

    [[nodiscard]] ToolFeedback stepPreview(const Point2& cursor) const override
    {
        ToolFeedback feedback;
        const State& now = state_.now;
        const auto turn = [&](const Point2& from, double minus) {
            if (!coincident(cursor, from)) {
                addPreview(feedback,
                           Mat3::rotationAbout(now.base, (cursor - from).angle() - minus));
            }
            addBand(feedback, from, cursor);
        };
        switch (now.step) {
        case Step::Angle:
            turn(now.base, 0.0);
            break;
        case Step::NewAngle:
            turn(now.base, now.reference);
            break;
        case Step::NewSecond:
            turn(now.first, now.reference);
            feedback.markers.push_back(now.base);
            break;
        case Step::ReferenceSecond:
            addBand(feedback, now.first, cursor);
            feedback.markers.push_back(now.base);
            break;
        case Step::ReferenceFirst:
        case Step::NewFirst:
            feedback.markers.push_back(now.base);
            break;
        case Step::Base:
            break;
        }
        return feedback;
    }

    [[nodiscard]] std::optional<Point2> stepLastPoint() const override
    {
        switch (state_.now.step) {
        case Step::Base:
            return std::nullopt;
        case Step::ReferenceSecond:
        case Step::NewSecond:
            return state_.now.first;
        case Step::Angle:
        case Step::ReferenceFirst:
        case Step::NewAngle:
        case Step::NewFirst:
            break;
        }
        return state_.now.base;
    }

    template <typename Change>
    ToolStep advance(Change change)
    {
        state_.remember();
        change(state_.now);
        return ToolStep::next();
    }

    ToolStep finish(double radians)
    {
        if (katana::math::anglesEqual(radians, 0.0)) {
            return ToolStep::rejected(
                "a rotation of 0 degrees, or of whole turns, changes nothing");
        }
        const State& now = state_.now;
        auto command = now.copy
                           ? duplicated("ROTATE", ids(), {Mat3::rotationAbout(now.base, radians)})
                           : cmd::rotateEntities(ids(), now.base, radians);
        return ToolStep::done(std::move(command),
                              counted(ids().size()) + (now.copy ? " copied rotated" : " rotated"));
    }

    History<State> state_;
};

// ---- Scale ---------------------------------------------------------------------------

// A base point, then the factor - typed, or picked as the distance from the
// base point. Reference: the length something is now (typed, or two points),
// then the length it should be (typed, picked from the base point, or two
// Points); the factor is their ratio - "make this 7.3 m edge 8 m". Copy scales
// a copy instead.
class ScaleTool final : public SelectionTool {
  public:
    explicit ScaleTool(const ToolContext& context) : SelectionTool(context, "scale") {}

  private:
    enum class Step {
        Base,
        Factor,
        ReferenceFirst,
        ReferenceSecond,
        NewLength,
        NewFirst,
        NewSecond
    };
    struct State {
        Step step = Step::Base;
        Point2 base;
        bool copy = false;
        double reference = 1.0;
        Point2 first;
    };

    [[nodiscard]] std::string stepPrompt() const override
    {
        switch (state_.now.step) {
        case Step::Base:
            return "Specify base point";
        case Step::Factor:
            return "Specify scale factor or [Copy/Reference]";
        case Step::ReferenceFirst:
            return "Specify reference length or its first point <1>";
        case Step::ReferenceSecond:
            return "Specify second point of the reference length";
        case Step::NewLength:
            return "Specify new length or [Points]";
        case Step::NewFirst:
            return "Specify first point of the new length";
        case Step::NewSecond:
            return "Specify second point of the new length";
        }
        return {};
    }

    [[nodiscard]] ToolStep stepPoint(const Point2& at) override
    {
        State& now = state_.now;
        switch (now.step) {
        case Step::Base:
            return advance([&](State& next) {
                next.base = at;
                next.step = Step::Factor;
            });
        case Step::Factor:
            return finish(now.base.distanceTo(at));
        case Step::ReferenceFirst:
            return advance([&](State& next) {
                next.first = at;
                next.step = Step::ReferenceSecond;
            });
        case Step::ReferenceSecond: {
            const double length = now.first.distanceTo(at);
            if (length <= tolerance::kGeometric) {
                return ToolStep::rejected("the two points coincide: a reference length of zero "
                                          "cannot be scaled from");
            }
            return advance([&](State& next) {
                next.reference = length;
                next.step = Step::NewLength;
            });
        }
        case Step::NewLength:
            return finish(now.base.distanceTo(at) / now.reference);
        case Step::NewFirst:
            return advance([&](State& next) {
                next.first = at;
                next.step = Step::NewSecond;
            });
        case Step::NewSecond:
            return finish(now.first.distanceTo(at) / now.reference);
        }
        return ToolStep::rejected("unexpected point");
    }

    [[nodiscard]] ToolStep stepValue(std::string_view text) override
    {
        State& now = state_.now;
        const auto number = numberIn(text);
        switch (now.step) {
        case Step::Factor:
            if (number) {
                return finish(*number);
            }
            if (isOption(text, "C", "COPY")) {
                state_.remember();
                now.copy = !now.copy;
                return ToolStep::next(now.copy ? "Scaling a copy of the selection."
                                               : "Scaling the selection itself.");
            }
            if (isOption(text, "R", "REFERENCE")) {
                return advance([](State& next) { next.step = Step::ReferenceFirst; });
            }
            break;
        case Step::ReferenceFirst:
            if (number) {
                if (!(*number > tolerance::kGeometric)) {
                    return ToolStep::rejected("the reference length must be greater than zero");
                }
                return advance([&](State& next) {
                    next.reference = *number;
                    next.step = Step::NewLength;
                });
            }
            break;
        case Step::NewLength:
            if (number) {
                return finish(*number / now.reference);
            }
            if (isOption(text, "P", "POINTS")) {
                return advance([](State& next) { next.step = Step::NewFirst; });
            }
            break;
        case Step::Base:
        case Step::ReferenceSecond:
        case Step::NewFirst:
        case Step::NewSecond:
            break;
        }
        return notUnderstood(text, "a factor, a length, a point or an option here");
    }

    [[nodiscard]] ToolStep stepEnter() override
    {
        if (state_.now.step == Step::ReferenceFirst) {
            return advance([](State& next) {
                next.reference = 1.0;
                next.step = Step::NewLength;
            });
        }
        return ToolStep::rejected("this step has no default: " + stepPrompt());
    }

    [[nodiscard]] bool stepUndo() override { return state_.back(); }

    [[nodiscard]] ToolFeedback stepPreview(const Point2& cursor) const override
    {
        ToolFeedback feedback;
        const State& now = state_.now;
        const auto grow = [&](const Point2& from) {
            const double per = now.step == Step::Factor ? 1.0 : now.reference;
            const double factor = from.distanceTo(cursor) / per;
            if (factor > tolerance::kGeometric) {
                addPreview(feedback, Mat3::scalingAbout(now.base, factor, factor));
            }
            addBand(feedback, from, cursor);
        };
        switch (now.step) {
        case Step::Factor:
        case Step::NewLength:
            grow(now.base);
            break;
        case Step::NewSecond:
            grow(now.first);
            feedback.markers.push_back(now.base);
            break;
        case Step::ReferenceSecond:
            addBand(feedback, now.first, cursor);
            feedback.markers.push_back(now.base);
            break;
        case Step::ReferenceFirst:
        case Step::NewFirst:
            feedback.markers.push_back(now.base);
            break;
        case Step::Base:
            break;
        }
        return feedback;
    }

    [[nodiscard]] std::optional<Point2> stepLastPoint() const override
    {
        switch (state_.now.step) {
        case Step::Base:
            return std::nullopt;
        case Step::ReferenceSecond:
        case Step::NewSecond:
            return state_.now.first;
        case Step::Factor:
        case Step::ReferenceFirst:
        case Step::NewLength:
        case Step::NewFirst:
            break;
        }
        return state_.now.base;
    }

    template <typename Change>
    ToolStep advance(Change change)
    {
        state_.remember();
        change(state_.now);
        return ToolStep::next();
    }

    ToolStep finish(double factor)
    {
        // Zero collapses the selection onto the base point and a negative factor
        // is a half-turn in disguise; the scale command refuses both, and so
        // does the tool, before it gets there.
        if (!(std::isfinite(factor) && factor > tolerance::kGeometric)) {
            return ToolStep::rejected("the scale factor must be greater than zero");
        }
        if (katana::math::nearlyEqual(factor, 1.0)) {
            return ToolStep::rejected("a scale factor of 1 changes nothing");
        }
        const State& now = state_.now;
        auto command =
            now.copy ? duplicated("SCALE", ids(), {Mat3::scalingAbout(now.base, factor, factor)})
                     : cmd::scaleEntities(ids(), now.base, factor);
        return ToolStep::done(std::move(command),
                              counted(ids().size()) + (now.copy ? " copied scaled" : " scaled"));
    }

    History<State> state_;
};

// ---- Mirror --------------------------------------------------------------------------

// Two points on the mirror line, then whether to erase the source - No by
// default, as in AutoCAD, so the usual result is the symmetric pair. (The
// command line's MIRROR replaces unless told KEEP; its default is the
// opposite dialogue, not a different operation.)
class MirrorTool final : public SelectionTool {
  public:
    explicit MirrorTool(const ToolContext& context) : SelectionTool(context, "mirror") {}

  private:
    enum class Step { First, Second, EraseSource };
    struct State {
        Step step = Step::First;
        Point2 first;
        Point2 second;
    };

    [[nodiscard]] std::string stepPrompt() const override
    {
        switch (state_.now.step) {
        case Step::First:
            return "Specify first point of mirror line";
        case Step::Second:
            return "Specify second point of mirror line";
        case Step::EraseSource:
            return "Erase source entities? [Yes/No] <No>";
        }
        return {};
    }

    [[nodiscard]] ToolInput stepExpects() const override
    {
        return state_.now.step == Step::EraseSource ? ToolInput::Value : ToolInput::Point;
    }

    [[nodiscard]] ToolStep stepPoint(const Point2& at) override
    {
        switch (state_.now.step) {
        case Step::First:
            state_.remember();
            state_.now.first = at;
            state_.now.step = Step::Second;
            return ToolStep::next();
        case Step::Second:
            if (coincident(at, state_.now.first)) {
                return ToolStep::rejected("the mirror line needs two different points");
            }
            state_.remember();
            state_.now.second = at;
            state_.now.step = Step::EraseSource;
            return ToolStep::next();
        case Step::EraseSource:
            break;
        }
        return ToolStep::rejected("answer Yes or No");
    }

    [[nodiscard]] ToolStep stepValue(std::string_view text) override
    {
        if (state_.now.step == Step::EraseSource) {
            if (isOption(text, "Y", "YES")) {
                return finish(false);
            }
            if (isOption(text, "N", "NO")) {
                return finish(true);
            }
            return notUnderstood(text, "an answer: type Yes or No");
        }
        return notUnderstood(text, "a point");
    }

    [[nodiscard]] ToolStep stepEnter() override
    {
        if (state_.now.step == Step::EraseSource) {
            return finish(true);
        }
        return ToolStep::rejected("this step has no default: " + stepPrompt());
    }

    [[nodiscard]] bool stepUndo() override { return state_.back(); }

    [[nodiscard]] ToolFeedback stepPreview(const Point2& cursor) const override
    {
        ToolFeedback feedback;
        const State& now = state_.now;
        const auto reflect = [&](const Point2& second) {
            if (!coincident(now.first, second)) {
                addPreview(feedback, Mat3::reflection(now.first, second - now.first));
            }
            addBand(feedback, now.first, second);
        };
        if (now.step == Step::Second) {
            reflect(cursor);
        } else if (now.step == Step::EraseSource) {
            reflect(now.second);
        }
        return feedback;
    }

    [[nodiscard]] std::optional<Point2> stepLastPoint() const override
    {
        switch (state_.now.step) {
        case Step::First:
            return std::nullopt;
        case Step::Second:
            return state_.now.first;
        case Step::EraseSource:
            return state_.now.second;
        }
        return std::nullopt;
    }

    ToolStep finish(bool keepSource)
    {
        return ToolStep::done(
            cmd::mirrorEntities(ids(), state_.now.first, state_.now.second, keepSource),
            counted(ids().size()) + (keepSource ? " mirrored, source kept" : " mirrored"));
    }

    History<State> state_;
};

// ---- Array: rectangular --------------------------------------------------------------

// Rows, columns, then the spacing - typed, or picked as a unit cell whose
// corners give the column and row spacing at once. A negative spacing builds
// the array to the left or downwards. The originals are cell (0, 0), as the
// command line's ARRAY has them.
class ArrayRectangularTool final : public SelectionTool {
  public:
    explicit ArrayRectangularTool(const ToolContext& context) : SelectionTool(context, "array") {}

  private:
    enum class Step { Rows, Columns, RowSpacing, CellCorner, ColumnSpacing, ColumnSecond };
    struct State {
        Step step = Step::Rows;
        int rows = 1;
        int columns = 1;
        double rowSpacing = 0.0;
        Point2 first; // unit cell corner, or first point of the column spacing
    };

    [[nodiscard]] std::string stepPrompt() const override
    {
        switch (state_.now.step) {
        case Step::Rows:
            return "Enter the number of rows <1>";
        case Step::Columns:
            return "Enter the number of columns <1>";
        case Step::RowSpacing:
            return "Enter the distance between rows or specify unit cell";
        case Step::CellCorner:
            return "Specify opposite corner of the unit cell";
        case Step::ColumnSpacing:
            return "Enter the distance between columns or its first point";
        case Step::ColumnSecond:
            return "Specify second point of the distance between columns";
        }
        return {};
    }

    [[nodiscard]] ToolInput stepExpects() const override
    {
        const Step step = state_.now.step;
        return step == Step::Rows || step == Step::Columns ? ToolInput::Value : ToolInput::Point;
    }

    [[nodiscard]] ToolStep stepValue(std::string_view text) override
    {
        State& now = state_.now;
        switch (now.step) {
        case Step::Rows:
        case Step::Columns: {
            const auto count = integerIn(text);
            if (!count || *count < 1) {
                return ToolStep::rejected("the number of " +
                                          std::string(now.step == Step::Rows ? "rows" : "columns") +
                                          " is a whole number, 1 or more");
            }
            return takeCount(*count);
        }
        case Step::RowSpacing:
        case Step::ColumnSpacing: {
            const auto distance = numberIn(text);
            if (!distance) {
                return notUnderstood(text, "a distance or a point");
            }
            if (std::abs(*distance) <= tolerance::kGeometric) {
                return ToolStep::rejected(
                    "a spacing of zero would stack every copy on the original");
            }
            if (now.step == Step::RowSpacing && now.columns > 1) {
                state_.remember();
                now.rowSpacing = *distance;
                now.step = Step::ColumnSpacing;
                return ToolStep::next();
            }
            return now.step == Step::RowSpacing ? finish(Vec2(0.0, *distance))
                                                : finish(Vec2(*distance, now.rowSpacing));
        }
        case Step::CellCorner:
        case Step::ColumnSecond:
            break;
        }
        return notUnderstood(text, "a point");
    }

    [[nodiscard]] ToolStep stepPoint(const Point2& at) override
    {
        State& now = state_.now;
        switch (now.step) {
        case Step::RowSpacing:
        case Step::ColumnSpacing:
            state_.remember();
            now.first = at;
            now.step = now.step == Step::RowSpacing ? Step::CellCorner : Step::ColumnSecond;
            return ToolStep::next();
        case Step::CellCorner: {
            const Vec2 cell = at - now.first;
            if (now.rows > 1 && std::abs(cell.y) <= tolerance::kGeometric) {
                return ToolStep::rejected(
                    "the unit cell has no height: the rows would stack on each other");
            }
            if (now.columns > 1 && std::abs(cell.x) <= tolerance::kGeometric) {
                return ToolStep::rejected(
                    "the unit cell has no width: the columns would stack on each other");
            }
            return finish(cell);
        }
        case Step::ColumnSecond: {
            // The distance between columns is measured across them, along x;
            // any rise between the two points is not part of it.
            const double dx = at.x - now.first.x;
            if (std::abs(dx) <= tolerance::kGeometric) {
                return ToolStep::rejected("the points are level across the columns: a spacing of "
                                          "zero would stack them");
            }
            return finish(Vec2(dx, now.rowSpacing));
        }
        case Step::Rows:
        case Step::Columns:
            break;
        }
        return ToolStep::rejected("type a whole number");
    }

    [[nodiscard]] ToolStep stepEnter() override
    {
        if (state_.now.step == Step::Rows || state_.now.step == Step::Columns) {
            return takeCount(1); // <1>
        }
        return ToolStep::rejected("this step has no default: " + stepPrompt());
    }

    [[nodiscard]] bool stepUndo() override { return state_.back(); }

    [[nodiscard]] ToolFeedback stepPreview(const Point2& cursor) const override
    {
        ToolFeedback feedback;
        const State& now = state_.now;
        if (now.step == Step::CellCorner) {
            addArray(feedback, cursor - now.first);
            feedback.markers.push_back(now.first);
        } else if (now.step == Step::ColumnSecond) {
            addArray(feedback, Vec2(cursor.x - now.first.x, now.rowSpacing));
            addBand(feedback, now.first, cursor);
        }
        return feedback;
    }

    [[nodiscard]] std::optional<Point2> stepLastPoint() const override
    {
        const Step step = state_.now.step;
        if (step == Step::CellCorner || step == Step::ColumnSecond) {
            return state_.now.first;
        }
        return std::nullopt;
    }

    ToolStep takeCount(std::int64_t count)
    {
        State& now = state_.now;
        if (now.step == Step::Rows) {
            if (count > kMaximumArrayItems) {
                return ToolStep::rejected("an array is at most 100000 items");
            }
            state_.remember();
            now.rows = static_cast<int>(count);
            now.step = Step::Columns;
            return ToolStep::next();
        }
        if (count * now.rows > kMaximumArrayItems) {
            return ToolStep::rejected("an array is at most 100000 items");
        }
        if (count == 1 && now.rows == 1) {
            return ToolStep::rejected(
                "one row of one column is the original alone: ask for more of either");
        }
        state_.remember();
        now.columns = static_cast<int>(count);
        now.step = now.rows > 1 ? Step::RowSpacing : Step::ColumnSpacing;
        return ToolStep::next();
    }

    [[nodiscard]] std::vector<Mat3> cells(const Vec2& spacing) const
    {
        std::vector<Mat3> out;
        for (int row = 0; row < state_.now.rows; ++row) {
            for (int column = 0; column < state_.now.columns; ++column) {
                if (row != 0 || column != 0) {
                    out.push_back(Mat3::translation(Vec2(spacing.x * column, spacing.y * row)));
                }
            }
        }
        return out;
    }

    void addArray(ToolFeedback& feedback, const Vec2& spacing) const
    {
        // Checked before the cells are built: a 1000 x 100 array would
        // otherwise make a hundred thousand matrices per mouse move only for
        // addTransformed to decline them.
        const auto copies = static_cast<std::size_t>(state_.now.rows * state_.now.columns - 1);
        if (copies * ids().size() > kPreviewLimit) {
            return;
        }
        const std::vector<Mat3> transforms = cells(spacing);
        addTransformed(feedback, document(), ids(), transforms);
    }

    ToolStep finish(const Vec2& spacing)
    {
        const State& now = state_.now;
        return ToolStep::done(cmd::arrayEntities(ids(), now.rows, now.columns, spacing),
                              std::to_string(now.rows) + " x " + std::to_string(now.columns) +
                                  " array of " + counted(ids().size()));
    }

    History<State> state_;
};

// ---- Array: polar --------------------------------------------------------------------

// A centre, the number of items (the original is one of them), the angle to
// fill - 360 by default, counter-clockwise, negative for clockwise - and
// whether the items turn as they go round. A full circle spaces the items by
// 360/n, since the last would otherwise land on the first; a part circle puts
// the first and last items at its ends, spacing them by angle/(n - 1), as
// AutoCAD does.
//
// Items that do not turn are carried round by the middle of the selection's
// extent. (AutoCAD carries them by the base point of the last object selected,
// which a selection of several made in any order does not make obvious.)
class ArrayPolarTool final : public SelectionTool {
  public:
    explicit ArrayPolarTool(const ToolContext& context) : SelectionTool(context, "array") {}

  private:
    enum class Step { Centre, Count, Fill, RotateItems };
    struct State {
        Step step = Step::Centre;
        Point2 centre;
        int count = 0;
        double fill = 360.0; // degrees
    };

    [[nodiscard]] std::string stepPrompt() const override
    {
        switch (state_.now.step) {
        case Step::Centre:
            return "Specify center point of array";
        case Step::Count:
            return "Enter the number of items in the array";
        case Step::Fill:
            return "Specify the angle to fill (+=ccw, -=cw) <360>";
        case Step::RotateItems:
            return "Rotate arrayed entities? [Yes/No] <Yes>";
        }
        return {};
    }

    [[nodiscard]] ToolInput stepExpects() const override
    {
        return state_.now.step == Step::Centre ? ToolInput::Point : ToolInput::Value;
    }

    [[nodiscard]] ToolStep stepPoint(const Point2& at) override
    {
        if (state_.now.step != Step::Centre) {
            return ToolStep::rejected("type the value: " + stepPrompt());
        }
        state_.remember();
        state_.now.centre = at;
        state_.now.step = Step::Count;
        return ToolStep::next();
    }

    [[nodiscard]] ToolStep stepValue(std::string_view text) override
    {
        State& now = state_.now;
        switch (now.step) {
        case Step::Count: {
            const auto count = integerIn(text);
            if (!count || *count < 2) {
                return ToolStep::rejected(
                    "the number of items counts the original, so it is a whole number, 2 or more");
            }
            if (*count > kMaximumArrayItems) {
                return ToolStep::rejected("an array is at most 100000 items");
            }
            state_.remember();
            now.count = static_cast<int>(*count);
            now.step = Step::Fill;
            return ToolStep::next();
        }
        case Step::Fill: {
            const auto degrees = numberIn(text);
            if (!degrees) {
                return notUnderstood(text, "an angle in degrees");
            }
            return fill(*degrees);
        }
        case Step::RotateItems:
            if (isOption(text, "Y", "YES")) {
                return finish(true);
            }
            if (isOption(text, "N", "NO")) {
                return finish(false);
            }
            return notUnderstood(text, "an answer: type Yes or No");
        case Step::Centre:
            break;
        }
        return notUnderstood(text, "a point");
    }

    [[nodiscard]] ToolStep stepEnter() override
    {
        switch (state_.now.step) {
        case Step::Fill:
            return fill(360.0);
        case Step::RotateItems:
            return finish(true);
        case Step::Centre:
        case Step::Count:
            break;
        }
        return ToolStep::rejected("this step has no default: " + stepPrompt());
    }

    [[nodiscard]] bool stepUndo() override { return state_.back(); }

    [[nodiscard]] ToolFeedback stepPreview(const Point2& cursor) const override
    {
        ToolFeedback feedback;
        const State& now = state_.now;
        switch (now.step) {
        case Step::Centre:
            break;
        case Step::Count:
            addBand(feedback, now.centre, cursor);
            break;
        case Step::Fill:
        case Step::RotateItems: {
            // What Enter would make: the angle's default, turning the items.
            feedback.markers.push_back(now.centre);
            if (static_cast<std::size_t>(now.count - 1) * ids().size() > kPreviewLimit) {
                break; // as the rectangular array: not built only to be declined
            }
            const std::vector<Mat3> transforms = items(now.fill, true);
            addTransformed(feedback, document(), ids(), transforms);
            break;
        }
        }
        return feedback;
    }

    [[nodiscard]] std::optional<Point2> stepLastPoint() const override
    {
        if (state_.now.step == Step::Centre) {
            return std::nullopt;
        }
        return state_.now.centre;
    }

    ToolStep fill(double degrees)
    {
        if (std::abs(degrees) <= tolerance::kAngular) {
            return ToolStep::rejected(
                "an angle to fill of 0 would stack every item on the original");
        }
        if (std::abs(degrees) > 360.0) {
            return ToolStep::rejected("the angle to fill is at most 360 degrees either way");
        }
        state_.remember();
        state_.now.fill = degrees;
        state_.now.step = Step::RotateItems;
        return ToolStep::next();
    }

    [[nodiscard]] std::optional<Point2> middleOfSelection() const
    {
        if (document() == nullptr) {
            return std::nullopt;
        }
        Box2 extent;
        for (const EntityId id : ids()) {
            if (const Entity* entity = document()->model().entities.find(id)) {
                extent.expand(katana::entity::boundingBox(entity->geometry));
            }
        }
        if (extent.empty()) {
            return std::nullopt;
        }
        return extent.center();
    }

    [[nodiscard]] std::vector<Mat3> items(double fillDegrees, bool rotate) const
    {
        const State& now = state_.now;
        const bool wholeTurn = std::abs(std::abs(fillDegrees) - 360.0) <= tolerance::kAngular;
        const double step = fillDegrees * kDegToRad / (wholeTurn ? now.count : now.count - 1);
        const Point2 carried = middleOfSelection().value_or(now.centre);
        std::vector<Mat3> out;
        for (int item = 1; item < now.count; ++item) {
            const Mat3 turn = Mat3::rotationAbout(now.centre, step * item);
            out.push_back(rotate ? turn
                                 : Mat3::translation(katana::math::transformPoint(turn, carried) -
                                                     carried));
        }
        return out;
    }

    ToolStep finish(bool rotate)
    {
        const State& now = state_.now;
        return ToolStep::done(duplicated("ARRAY", ids(), items(now.fill, rotate)),
                              "polar array of " + std::to_string(now.count) + " items of " +
                                  counted(ids().size()));
    }

    History<State> state_;
};

// ---- Erase ---------------------------------------------------------------------------

class EraseTool final : public SelectionTool {
  public:
    explicit EraseTool(const ToolContext& context)
        : SelectionTool(context, "erase", /*confirms=*/true)
    {
    }

  private:
    [[nodiscard]] std::string stepPrompt() const override { return {}; }

    [[nodiscard]] ToolStep selected() override
    {
        return ToolStep::done(cmd::deleteEntities(ids()), counted(ids().size()) + " erased");
    }
};

// ---- Stretch -------------------------------------------------------------------------
//
// A crossing window, then a displacement. What lies wholly inside the window
// moves; what crosses it has only the points inside moved - a line's ends, a
// polyline's vertices, a dimension's definition points. A circle, a point and
// a text move when their centre, position or insertion point is inside. An
// arc with one end inside keeps its chord height (the sagitta) and is rebuilt
// through its new ends, as AutoCAD stretches one.
//
// This is expressible without a new command: the stretch is one change set
// modifying each entity it touches, built from the model when it executes.

// The geometry with its defining points inside `window` moved by `delta`, or
// nullopt when none is inside and it would not change.
struct Stretcher {
    Box2 window;
    Vec2 delta;

    [[nodiscard]] Point2 moved(const Point2& p) const { return window.contains(p) ? p + delta : p; }

    Result<std::optional<Geometry>> operator()(const katana::entity::PointGeometry& point) const
    {
        if (!window.contains(point.position)) {
            return std::optional<Geometry>();
        }
        return std::optional<Geometry>(katana::entity::PointGeometry{point.position + delta});
    }

    Result<std::optional<Geometry>> operator()(const Segment2& segment) const
    {
        if (!window.contains(segment.start) && !window.contains(segment.end)) {
            return std::optional<Geometry>();
        }
        return std::optional<Geometry>(Segment2{moved(segment.start), moved(segment.end)});
    }

    Result<std::optional<Geometry>> operator()(const Arc2& arc) const
    {
        const Point2 start = arc.startPoint();
        const Point2 end = arc.endPoint();
        const bool startInside = window.contains(start);
        const bool endInside = window.contains(end);
        if (!startInside && !endInside) {
            return std::optional<Geometry>();
        }
        if (startInside && endInside) {
            return std::optional<Geometry>(
                Arc2{arc.center + delta, arc.radius, arc.startAngle, arc.sweep});
        }
        // One end moves. The height of the arc above its chord, signed by the
        // side it bulges to, is kept, and the arc is the one through the new
        // ends and the point that height above the new chord's middle.
        const Vec2 chord = end - start;
        const double height = chord.normalized().cross(arc.midpoint() - start);
        const Point2 newStart = moved(start);
        const Point2 newEnd = moved(end);
        const Vec2 newChord = newEnd - newStart;
        if (newChord.length() <= tolerance::kGeometric) {
            return makeError(ErrorCode::InvalidArgument,
                             "stretching would bring the arc's ends together");
        }
        const Point2 middle =
            (newStart + newEnd) * 0.5 + newChord.normalized().perpendicular() * height;
        const auto rebuilt = Arc2::throughPoints(newStart, middle, newEnd);
        if (!rebuilt) {
            return makeError(ErrorCode::InvalidArgument, "the stretched arc would be degenerate");
        }
        return std::optional<Geometry>(*rebuilt);
    }

    Result<std::optional<Geometry>> operator()(const Polyline2& polyline) const
    {
        if (std::ranges::none_of(polyline.vertices,
                                 [&](const Point2& p) { return window.contains(p); })) {
            return std::optional<Geometry>();
        }
        Polyline2 out = polyline;
        for (Point2& vertex : out.vertices) {
            vertex = moved(vertex);
        }
        return std::optional<Geometry>(std::move(out));
    }

    Result<std::optional<Geometry>> operator()(const Circle2& circle) const
    {
        if (!window.contains(circle.center)) {
            return std::optional<Geometry>();
        }
        return std::optional<Geometry>(Circle2{circle.center + delta, circle.radius});
    }

    Result<std::optional<Geometry>> operator()(const katana::entity::TextGeometry& text) const
    {
        if (!window.contains(text.position)) {
            return std::optional<Geometry>();
        }
        katana::entity::TextGeometry out = text;
        out.position = text.position + delta;
        return std::optional<Geometry>(std::move(out));
    }

    Result<std::optional<Geometry>>
    operator()(const katana::entity::DimensionGeometry& dimension) const
    {
        if (!window.contains(dimension.start) && !window.contains(dimension.end)) {
            return std::optional<Geometry>();
        }
        katana::entity::DimensionGeometry out = dimension;
        out.start = moved(dimension.start);
        out.end = moved(dimension.end);
        if (out.start.distanceTo(out.end) <= tolerance::kGeometric) {
            return makeError(ErrorCode::InvalidArgument,
                             "stretching would bring the dimension's points together");
        }
        return std::optional<Geometry>(std::move(out));
    }
};

// The change set of a stretch: every entity of `ids` the window touches.
Result<cmd::ChangeSet> stretchChanges(const katana::entity::Model& model,
                                      const std::vector<EntityId>& ids, const Box2& window,
                                      const Vec2& delta)
{
    cmd::ChangeSet changes;
    const Stretcher stretcher{window, delta};
    for (const EntityId id : ids) {
        const Entity* entity = model.entities.find(id);
        if (entity == nullptr) {
            return makeError(ErrorCode::NotFound, "entity does not exist",
                             "id=" + std::to_string(id));
        }
        auto stretched = std::visit(stretcher, entity->geometry);
        if (!stretched) {
            return makeError(stretched.error().code, stretched.error().message,
                             "id=" + std::to_string(id));
        }
        if (*stretched) {
            Entity changed = *entity;
            changed.geometry = std::move(**stretched);
            changes.modify.push_back(std::move(changed));
        }
    }
    if (changes.empty()) {
        return makeError(ErrorCode::InvalidArgument,
                         "nothing in the window has a point inside it to stretch");
    }
    return changes;
}

cmd::CommandPtr stretchCommand(std::vector<EntityId> ids, const Box2& window, const Vec2& delta)
{
    return std::make_unique<cmd::ChangeSetCommand>(
        "STRETCH", [ids = std::move(ids), window, delta](const cmd::CommandContext& context) {
            return stretchChanges(context.model, ids, window, delta);
        });
}

class StretchTool final : public InteractiveTool {
  public:
    // The window is the selection, so Stretch never asks for one; a selection
    // made beforehand limits what the window can catch.
    explicit StretchTool(const ToolContext& context)
        : document_(context.document), limit_(context.selection)
    {
    }

    [[nodiscard]] std::string prompt() const override
    {
        switch (state_.now.step) {
        case Step::FirstCorner:
            return "Specify first corner of the crossing window";
        case Step::OppositeCorner:
            return "Specify opposite corner";
        case Step::Base:
            return displacementPrompt(Displace::Base);
        case Step::Typed:
            return displacementPrompt(Displace::Typed);
        case Step::Second:
            return displacementPrompt(Displace::Second);
        }
        return {};
    }

    [[nodiscard]] ToolInput expects() const override { return ToolInput::Point; }

    [[nodiscard]] ToolStep point(const Point2& at) override
    {
        State& now = state_.now;
        switch (now.step) {
        case Step::FirstCorner:
            state_.remember();
            now.corner = at;
            now.step = Step::OppositeCorner;
            return ToolStep::next();
        case Step::OppositeCorner:
            return window(at);
        case Step::Base:
            state_.remember();
            now.base = at;
            now.step = Step::Second;
            return ToolStep::next();
        case Step::Typed:
            return finish(at - Point2());
        case Step::Second:
            return finish(at - now.base);
        }
        return ToolStep::rejected("unexpected point");
    }

    [[nodiscard]] ToolStep value(std::string_view text) override
    {
        State& now = state_.now;
        if (now.step == Step::Base && isOption(text, "D", "DISPLACEMENT")) {
            state_.remember();
            now.step = Step::Typed;
            return ToolStep::next();
        }
        if (now.step == Step::Second) {
            if (const auto distance = numberIn(text)) {
                const auto to = alongCursor(*distance, now.base, cursor_);
                if (!to) {
                    return ToolStep::rejected(std::string(kNoDirection));
                }
                return finish(*to - now.base);
            }
        }
        return notUnderstood(text, "a point, a distance or an option here");
    }

    [[nodiscard]] ToolStep enter() override
    {
        State& now = state_.now;
        switch (now.step) {
        case Step::Base:
            state_.remember();
            now.step = Step::Typed;
            return ToolStep::next();
        case Step::Second:
            return finish(now.base - Point2());
        case Step::Typed:
            return ToolStep::rejected("type the displacement as dx,dy");
        case Step::FirstCorner:
        case Step::OppositeCorner:
            break;
        }
        return ToolStep::rejected("pick the corners of the crossing window first");
    }

    [[nodiscard]] ToolStep undo() override
    {
        return state_.back() ? ToolStep::next()
                             : ToolStep::rejected("nothing to undo in this tool");
    }

    [[nodiscard]] ToolFeedback preview(const Point2& cursor) const override
    {
        cursor_ = cursor;
        ToolFeedback feedback;
        const State& now = state_.now;
        switch (now.step) {
        case Step::FirstCorner:
            break;
        case Step::OppositeCorner:
            feedback.shapes.emplace_back(outline(Box2(now.corner, now.corner), cursor));
            feedback.markers.push_back(now.corner);
            break;
        case Step::Base:
        case Step::Typed:
            feedback.shapes.emplace_back(outline(now.window, now.window.max));
            break;
        case Step::Second: {
            feedback.shapes.emplace_back(outline(now.window, now.window.max));
            if (document_ != nullptr && now.caught.size() <= kPreviewLimit &&
                !coincident(cursor, now.base)) {
                if (auto changes = stretchChanges(document_->model(), now.caught, now.window,
                                                  cursor - now.base)) {
                    for (Entity& entity : changes->modify) {
                        feedback.shapes.push_back(std::move(entity.geometry));
                    }
                }
            }
            addBand(feedback, now.base, cursor);
            break;
        }
        }
        return feedback;
    }

    [[nodiscard]] std::optional<Point2> lastPoint() const override
    {
        switch (state_.now.step) {
        case Step::FirstCorner:
            return std::nullopt;
        case Step::OppositeCorner:
            return state_.now.corner;
        case Step::Typed:
            return Point2();
        case Step::Base:
            return std::nullopt;
        case Step::Second:
            return state_.now.base;
        }
        return std::nullopt;
    }

  private:
    enum class Step { FirstCorner, OppositeCorner, Base, Typed, Second };
    struct State {
        Step step = Step::FirstCorner;
        Point2 corner;
        Box2 window;
        std::vector<EntityId> caught; // what the window touches
        Point2 base;
    };

    // The window's outline as a closed polyline: `box` grown to take in `to`.
    static Polyline2 outline(Box2 box, const Point2& to)
    {
        box.expand(to);
        return Polyline2{
            {box.min, Point2(box.max.x, box.min.y), box.max, Point2(box.min.x, box.max.y)}, true};
    }

    ToolStep window(const Point2& opposite)
    {
        Box2 box(state_.now.corner, state_.now.corner);
        box.expand(opposite);
        if (box.width() <= tolerance::kGeometric || box.height() <= tolerance::kGeometric) {
            return ToolStep::rejected("the window has no area: pick a corner diagonally opposite");
        }
        if (document_ == nullptr) {
            return ToolStep::rejected("there is no drawing to stretch");
        }
        std::vector<EntityId> caught = pickInBox(
            document_->model(), box, BoxSelectionMode::Crossing, {}, &document_->spatialIndex());
        if (!limit_.empty()) {
            std::erase_if(caught,
                          [&](EntityId id) { return !std::ranges::binary_search(limit_, id); });
        }
        // Only what has a point inside can stretch: a line merely crossing the
        // window has neither end in it, and stays as it is.
        const Stretcher probe{box, Vec2(1.0, 0.0)};
        std::erase_if(caught, [&](EntityId id) {
            const Entity* entity = document_->model().entities.find(id);
            if (entity == nullptr) {
                return true;
            }
            const auto stretched = std::visit(probe, entity->geometry);
            return stretched && !*stretched;
        });
        if (caught.empty()) {
            return ToolStep::rejected(
                "nothing has a point inside the window: a line or polyline stretches by the "
                "ends or vertices the window contains");
        }
        state_.remember();
        state_.now.window = box;
        state_.now.caught = std::move(caught);
        state_.now.step = Step::Base;
        return ToolStep::next(counted(state_.now.caught.size()) + " to stretch");
    }

    ToolStep finish(const Vec2& delta)
    {
        if (delta.length() <= tolerance::kGeometric) {
            return ToolStep::rejected("a displacement of zero would stretch nothing");
        }
        const State& now = state_.now;
        // Built once here to refuse, with its reason, a stretch the command
        // would refuse - an arc whose ends would meet.
        if (document_ != nullptr) {
            if (auto changes = stretchChanges(document_->model(), now.caught, now.window, delta);
                !changes) {
                return ToolStep::rejected(changes.error().message);
            }
        }
        return ToolStep::done(stretchCommand(now.caught, now.window, delta),
                              counted(now.caught.size()) + " stretched");
    }

    const Document* document_ = nullptr;
    std::vector<EntityId> limit_; // ascending, as ToolContext gives it
    History<State> state_;
    mutable std::optional<Point2> cursor_;
};

// ---- the catalogue -------------------------------------------------------------------

template <typename Tool>
ToolInfo transformTool(std::string id, std::string name, int order,
                       std::vector<std::string> aliases, std::string tip)
{
    ToolInfo info;
    info.id = std::move(id);
    info.name = std::move(name);
    info.category = "Modify";
    info.group = "Transform";
    info.order = order;
    info.aliases = std::move(aliases);
    info.tip = std::move(tip);
    info.make = [](const ToolContext& context) -> std::unique_ptr<InteractiveTool> {
        return std::make_unique<Tool>(context);
    };
    return info;
}

} // namespace

void addModifyTransformTools(ToolCatalog& catalog, const Report& report)
{
    report(catalog.add(transformTool<MoveTool>(
        "modify.move", "Move", 10, {"MOVE", "M"},
        "Moves the selection from a base point to a second point, or by a typed displacement.")));
    report(catalog.add(transformTool<CopyTool>(
        "modify.copy", "Copy", 20, {"COPY", "CO", "CP"},
        "Copies the selection from a base point to as many second points as you pick; Enter "
        "ends.")));
    report(catalog.add(transformTool<RotateTool>(
        "modify.rotate", "Rotate", 30, {"ROTATE", "RO"},
        "Rotates the selection about a base point by a typed or picked angle, or from a "
        "reference angle to a new one.")));
    report(catalog.add(transformTool<ScaleTool>(
        "modify.scale", "Scale", 40, {"SCALE", "SC"},
        "Scales the selection about a base point by a factor, or from a reference length to a "
        "new one.")));
    report(catalog.add(transformTool<MirrorTool>(
        "modify.mirror", "Mirror", 50, {"MIRROR", "MI"},
        "Mirrors the selection about a line through two points, keeping or erasing the "
        "source.")));
    report(catalog.add(transformTool<StretchTool>(
        "modify.stretch", "Stretch", 60, {"STRETCH", "S"},
        "Moves the ends and vertices inside a crossing window, stretching what crosses it and "
        "moving what lies within.")));
    report(catalog.add(transformTool<ArrayRectangularTool>(
        "modify.array_rectangular", "Rectangular Array", 70, {"ARRAYRECT", "ARRAY", "AR"},
        "Copies the selection into rows and columns at a typed spacing or a picked unit "
        "cell.")));
    report(catalog.add(transformTool<ArrayPolarTool>(
        "modify.array_polar", "Polar Array", 80, {"ARRAYPOLAR"},
        "Copies the selection round a centre point: a number of items over an angle, turning "
        "them or not.")));
    report(catalog.add(transformTool<EraseTool>(
        "modify.erase", "Erase", 90, {"ERASE", "E", "DELETE", "DEL"},
        "Erases the selection.")));
}

} // namespace katana::cad::tools
