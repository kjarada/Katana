#include "tools/tool_host.hpp"

#include <algorithm>
#include <array>
#include <cstdint>
#include <utility>

namespace katana::qt::tools {

namespace cad = katana::cad;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Status;

bool escapeKeepsWork(std::string_view toolId)
{
    // Each checked against the tool's enter() at every step it can be in
    // when cancel() sends Enter: Enter commits what was collected, ends with
    // nothing, or only moves the tool on a step (Trim's edges, Offset's
    // distance), which cancel() then drops. The exceptions are Fillet's and
    // Chamfer's value prompts, whose Enter defaults a setting; cancel() steps
    // back out of those before it sends Enter, so Enter is never sent at a
    // value prompt. Copy is NOT here although its placed copies are collected work,
    // because at its second-point prompt with none placed yet Enter copies by
    // the base point as a displacement.
    static constexpr std::array<std::string_view, 7> kKeep = {
        "draw.line",     "draw.polyline", "modify.trim",   "modify.extend",
        "modify.offset", "modify.fillet", "modify.chamfer",
    };
    return std::ranges::find(kKeep, toolId) != kKeep.end();
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
    info_ = info;
    make();
    if (onStarted) {
        onStarted(info_->id);
    }
    report(onPrompt, prompt());
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
    return tool_ != nullptr ? tool_->preview(cursor) : cad::ToolFeedback{};
}

std::optional<ToolHost::Point2> ToolHost::lastPoint() const
{
    return tool_ != nullptr ? tool_->lastPoint() : std::nullopt;
}

ToolHost::Outcome ToolHost::point(const Point2& at)
{
    return tool_ != nullptr ? apply(tool_->point(at)) : idle();
}

ToolHost::Outcome ToolHost::entity(katana::entity::EntityId id, const Point2& at)
{
    return tool_ != nullptr ? apply(tool_->entity(id, at)) : idle();
}

ToolHost::Outcome ToolHost::typed(std::string_view text)
{
    return tool_ != nullptr ? apply(cad::routeTypedInput(*tool_, text)) : idle();
}

ToolHost::Outcome ToolHost::enter() { return tool_ != nullptr ? apply(tool_->enter()) : idle(); }

ToolHost::Outcome ToolHost::undo() { return tool_ != nullptr ? apply(tool_->undo()) : idle(); }

void ToolHost::cancel()
{
    if (tool_ == nullptr) {
        return;
    }
    if (escapeKeepsWork(info_->id)) {
        // Out of a value prompt first. Enter there takes the prompt's default
        // - at Chamfer's second distance it stores both distances for every
        // later Chamfer - or only returns to the lines (Fillet's radius), and
        // then the corners a Multiple session made would go with the tool.
        // Undo at those prompts only steps back towards the lines, never
        // touching the session or the defaults (CornerTool::undo); a value
        // prompt still showing after that is left rather than defaulted.
        constexpr int kMostStepsBack = 4; // SecondDistance -> FirstDistance -> First is 2
        for (int stepped = 0;
             stepped < kMostStepsBack && tool_->expects() == cad::ToolInput::Value; ++stepped) {
            if (tool_->undo().outcome != Outcome::Continue) {
                break;
            }
        }
        if (tool_->expects() == cad::ToolInput::Value) {
            end();
            return;
        }
        cad::ToolStep step = tool_->enter();
        // Only a finished step's command is work to keep. A Continue here is
        // Enter moving the tool on a step (Trim's edges, Offset's distance)
        // and a Rejected changed nothing; either way the tool is dropped next.
        if (step.outcome == Outcome::Done) {
            step.restart = false;
            apply(std::move(step));
            return; // apply ended the tool
        }
    }
    end();
}

void ToolHost::reset()
{
    if (tool_ == nullptr) {
        return;
    }
    make();
    report(onPrompt, prompt());
}

void ToolHost::make()
{
    cad::ToolContext context;
    context.document = &document_;
    context.attributes = document_.currentAttributes();
    context.selection = document_.selection().ids();
    context.pickTolerance = pickTolerance_;
    tool_ = info_->make(context);
    ++generation_;
}

ToolHost::Outcome ToolHost::apply(cad::ToolStep step)
{
    const Outcome outcome = step.outcome;
    // The tool this step came from. A hook raised below may stop it or start
    // another; what follows a hook applies only while it is still this one.
    // Counted, not compared by address: a tool started in a hook can be
    // allocated where the one it replaced was, and was, under ctest.
    const std::uint64_t from = generation_;
    switch (outcome) {
    case Outcome::Continue:
        report(onMessage, step.message);
        if (generation_ == from) {
            report(onPrompt, prompt());
        }
        break;
    case Outcome::Rejected:
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
        if (executed) {
            report(onMessage, step.message);
        }
        if (generation_ != from) {
            break;
        }
        if (step.restart) {
            make();
            report(onPrompt, prompt());
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
    ++generation_;
    if (onFinished) {
        onFinished(id);
    }
    if (onPrompt) {
        onPrompt({});
    }
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
