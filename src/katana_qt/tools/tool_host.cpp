#include "tools/tool_host.hpp"

#include <algorithm>
#include <cstdint>
#include <utility>

#include "katana/core/text.hpp"

namespace katana::qt::tools {

namespace cad = katana::cad;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Status;

bool isTransparentCommand(std::string_view text, bool wantsValue)
{
    std::string_view word = katana::core::trimmed(text);
    const bool apostrophe = !word.empty() && word.front() == '\'';
    if (apostrophe) {
        word.remove_prefix(1);
    } else if (wantsValue) {
        return false;
    }
    word = word.substr(0, word.find(' '));
    const auto is = [&](std::string_view verb) {
        return katana::core::equalsIgnoringCase(word, verb);
    };
    // A bare P is Rotate's and Scale's [Points] option, so only 'P - the
    // apostrophe AutoCAD marks a transparent command with - is PAN. No tool
    // offers Z.
    return is("ZOOM") || is("Z") || is("PAN") || (apostrophe && is("P"));
}

ToolHost::ToolHost(cad::Document& document) : document_(document) {}

// Out of line so that the tool's destructor runs here, with InteractiveTool
// complete, and so that hooks are never raised from a destructor.
ToolHost::~ToolHost() = default;

Status ToolHost::start(std::string_view id)
{
    const cad::ToolInfo* info = cad::toolCatalog().find(id);
    if (info == nullptr) {
        return makeError(ErrorCode::NotFound, "there is no tool '" + std::string(id) + "'");
    }
    cancel();
    heldAt_.reset();
    info_ = info;
    make();
    if (onStarted) {
        onStarted(info_->id);
    }
    reportPrompt();
    return {};
}

std::string ToolHost::activeId() const { return info_ != nullptr ? info_->id : std::string(); }

std::optional<cad::ToolInput> ToolHost::expects() const
{
    return tool_ != nullptr ? std::optional<cad::ToolInput>(tool_->expects()) : std::nullopt;
}

std::string ToolHost::prompt() const { return tool_ != nullptr ? tool_->prompt() : std::string(); }

cad::ToolFeedback ToolHost::feedback(const Point2& cursor) const
{
    if (tool_ == nullptr) {
        return {};
    }
    cad::ToolFeedback shown = tool_->preview(cursor);
    if (shown.refused && heldAt_ && cursor.distanceTo(*heldAt_) <= holdReach()) {
        // What the tool shows there less its refusal: the marks it flags as
        // the refusal's reason go (the vertex Insert has just made, too near
        // for another), the rest stay as drawn - Straighten's first "keep",
        // which the click has just taken - and the caption is what the
        // click did.
        cad::ToolFeedback held = std::move(shown);
        held.refused = false;
        std::erase_if(held.marks, [](const cad::FeedbackMark& mark) { return mark.refused; });
        held.caption = heldMessage_;
        return held;
    }
    return shown;
}

bool ToolHost::takesSnap(const cad::SnapResult& snap) const
{
    return tool_ == nullptr || tool_->takesSnap(snap);
}

std::optional<ToolHost::Point2> ToolHost::lastPoint() const
{
    return tool_ != nullptr ? tool_->lastPoint() : std::nullopt;
}

void ToolHost::setCursor(const Point2& at)
{
    cursor_ = at;
    if (heldAt_ && at.distanceTo(*heldAt_) > holdReach()) {
        heldAt_.reset();
    }
}

double ToolHost::holdReach() const { return pickAperture_ ? pickAperture_() : pickTolerance_; }

ToolHost::Outcome ToolHost::point(const Point2& at, const std::optional<cad::SnapResult>& snap)
{
    return tool_ != nullptr ? click(at, cad::routeSnappedPoint(*tool_, document_, at, snap))
                            : idle();
}

ToolHost::Outcome ToolHost::entity(katana::entity::EntityId id, const Point2& at)
{
    return tool_ != nullptr ? click(at, tool_->entity(id, at)) : idle();
}

ToolHost::Outcome ToolHost::click(const Point2& at, cad::ToolStep step)
{
    heldAt_.reset();
    const Outcome outcome = apply(std::move(step));
    if (lastTaken_ && tool_ != nullptr) {
        heldAt_ = at;
        heldMessage_ = lastMessage_;
    }
    return outcome;
}

ToolHost::Outcome ToolHost::typed(std::string_view text)
{
    heldAt_.reset();
    if (tool_ == nullptr) {
        return idle();
    }
    if (isTransparentCommand(text, tool_->expects() == cad::ToolInput::Value)) {
        // The view's, not the tool's: the tool stays at its step and its
        // prompt is shown again, as AutoCAD resumes LINE after 'ZOOM.
        const std::string command(katana::core::trimmed(text));
        if (onTransparent && onTransparent(command)) {
            reportPrompt();
            return Outcome::Continue;
        }
        report(onRejected, command + " cannot run inside " + info_->name +
                               "; press Esc to end the tool first.");
        return Outcome::Rejected;
    }
    return apply(cad::routeTypedInput(*tool_, text, document_.drafting(), cursor_));
}

ToolHost::Outcome ToolHost::enter()
{
    heldAt_.reset();
    return tool_ != nullptr ? apply(tool_->enter()) : idle();
}

ToolHost::Outcome ToolHost::undo()
{
    heldAt_.reset();
    return tool_ != nullptr ? apply(tool_->undo()) : idle();
}

bool ToolHost::takesDelete() const { return tool_ != nullptr && tool_->takesDelete(); }

ToolHost::Outcome ToolHost::deleteKey()
{
    heldAt_.reset();
    return tool_ != nullptr ? apply(tool_->deleteKey()) : idle();
}

void ToolHost::cancel()
{
    if (tool_ == nullptr) {
        return;
    }
    // The tool says what of its work stays (InteractiveTool::cancel). A
    // Continue or a Rejected keeps nothing; the tool ends either way.
    cad::ToolStep step = tool_->cancel();
    if (step.outcome == Outcome::Done && (step.command || step.selection)) {
        step.restart = false;
        apply(std::move(step)); // ends the tool, as any finished step does
        return;
    }
    end();
}

void ToolHost::abandon()
{
    if (tool_ != nullptr) {
        end();
    }
}

void ToolHost::make()
{
    cad::ToolContext context;
    context.document = &document_;
    context.attributes = document_.currentAttributes();
    context.selection = document_.selection().ids();
    context.pickTolerance = pickTolerance_;
    // Once: taken by this tool, so a restart after its edit has none.
    context.handles = std::exchange(handles_, {});
    context.pick = pick_;
    context.pickAperture = pickAperture_;
    context.vertexAperture = vertexAperture_;
    context.view = view_;
    tool_ = info_->make(context);
    ++generation_;
}

ToolHost::Outcome ToolHost::apply(cad::ToolStep step)
{
    const Outcome outcome = step.outcome;
    lastTaken_ = outcome == Outcome::Continue;
    lastMessage_ = step.message;
    // The tool this step came from. A hook raised below may stop it or start
    // another; what follows a hook applies only while it is still this one.
    // Counted, not compared by address: a tool started in a hook can be
    // allocated where the one it replaced was, and was, under ctest.
    const std::uint64_t from = generation_;
    switch (outcome) {
    case Outcome::Continue:
        report(onMessage, step.message);
        if (generation_ == from) {
            reportPrompt();
        }
        break;
    case Outcome::Rejected:
        // A refusal leaves the prompt as it was, but one that drops what the
        // tool held - a pick made stale by another edit - asks afresh, and
        // the command line's placeholder must say so. Before the refusal, so
        // the status bar ends on the refusal, as it does for every other.
        if (prompt() != reportedPrompt_) {
            reportPrompt();
        }
        report(onRejected, step.message);
        break;
    case Outcome::Done: {
        bool executed = true;
        if (step.command) {
            const Status status = document_.execute(std::move(step.command));
            if (!status) {
                executed = false;
                report(onRejected, status.error().describe());
            }
        }
        lastTaken_ = executed;
        if (executed) {
            report(onMessage, step.message);
            if (step.selection) {
                // What a selecting tool found, left selected: pruned, so an
                // id the step's command removed is not held.
                cad::SelectionSet& selection = document_.selection();
                selection.set(std::move(*step.selection));
                (void)selection.prune(document_.model().entities);
                document_.notifySelectionChanged();
            }
        }
        if (generation_ != from) {
            break;
        }
        if (step.restart) {
            make();
            reportPrompt();
        } else {
            end();
        }
        break;
    }
    }
    return outcome;
}

void ToolHost::end()
{
    const std::string id = activeId();
    // Cleared before the hooks run, so a hook that starts another tool finds
    // this one gone rather than cancelling it a second time.
    tool_.reset();
    info_ = nullptr;
    heldAt_.reset();
    ++generation_;
    if (onFinished) {
        onFinished(id);
    }
    reportedPrompt_.clear();
    if (onPrompt) {
        onPrompt({});
    }
}

void ToolHost::reportPrompt()
{
    reportedPrompt_ = prompt();
    report(onPrompt, reportedPrompt_);
}

ToolHost::Outcome ToolHost::idle()
{
    report(onRejected, "No tool is running.");
    return Outcome::Rejected;
}

void ToolHost::report(const std::function<void(const std::string&)>& hook, const std::string& text)
{
    if (hook && !text.empty()) {
        hook(text);
    }
}

} // namespace katana::qt::tools
