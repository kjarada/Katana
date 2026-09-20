#include "katana/commands/command_stack.hpp"

#include <algorithm>
#include <chrono>
#include <string>
#include <utility>

namespace katana::commands {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Status;

namespace {

std::string_view toString(CommandEventKind kind)
{
    switch (kind) {
    case CommandEventKind::Executed:
        return "executed";
    case CommandEventKind::Undone:
        return "undone";
    case CommandEventKind::Redone:
        return "redone";
    }
    return "unknown";
}

} // namespace

CommandStack::CommandStack(katana::entity::Model& model, katana::core::Logger* logger)
    : context_{model, logger}
{
    context_.model.entities.setObserver(
        [this](const katana::entity::ChangeEvent& event) { pendingChanges_.push_back(event); });
}

CommandStack::~CommandStack()
{
    context_.model.entities.setObserver(nullptr);
}

Status CommandStack::execute(CommandPtr command)
{
    if (!command) {
        return makeError(ErrorCode::InvalidArgument, "command is null");
    }
    const auto started = std::chrono::steady_clock::now();
    pendingChanges_.clear();

    Status status = command->validate(context_);
    if (status) {
        status = command->execute(context_);
    }
    if (!status) {
        pendingChanges_.clear();
        if (context_.logger != nullptr) {
            context_.logger->warning("command", "rejected",
                                     {{"name", std::string(command->name())},
                                      {"reason", status.error().describe()}});
        }
        return status;
    }

    // The saved state becomes unreachable if it lived in the redo history.
    if (savedTop_ != nullptr &&
        std::any_of(redoStack_.begin(), redoStack_.end(),
                    [&](const CommandPtr& c) { return c.get() == savedTop_; })) {
        savedReachable_ = false;
        savedTop_ = nullptr;
    }
    redoStack_.clear();
    undoStack_.push_back(std::move(command));

    if (context_.logger != nullptr) {
        const auto elapsed = std::chrono::duration_cast<std::chrono::microseconds>(
            std::chrono::steady_clock::now() - started);
        context_.logger->info("command", "executed",
                              {{"name", std::string(undoStack_.back()->name())},
                               {"changes", std::to_string(pendingChanges_.size())},
                               {"micros", std::to_string(elapsed.count())}});
    }
    publish(CommandEventKind::Executed, *undoStack_.back());
    return {};
}

Status CommandStack::undo()
{
    if (undoStack_.empty()) {
        return makeError(ErrorCode::InvalidState, "nothing to undo");
    }
    pendingChanges_.clear();
    if (auto status = undoStack_.back()->undo(context_); !status) {
        pendingChanges_.clear();
        return status;
    }
    redoStack_.push_back(std::move(undoStack_.back()));
    undoStack_.pop_back();
    publish(CommandEventKind::Undone, *redoStack_.back());
    return {};
}

Status CommandStack::redo()
{
    if (redoStack_.empty()) {
        return makeError(ErrorCode::InvalidState, "nothing to redo");
    }
    pendingChanges_.clear();
    if (auto status = redoStack_.back()->redo(context_); !status) {
        pendingChanges_.clear();
        return status;
    }
    undoStack_.push_back(std::move(redoStack_.back()));
    redoStack_.pop_back();
    publish(CommandEventKind::Redone, *undoStack_.back());
    return {};
}

std::string_view CommandStack::undoName() const
{
    return undoStack_.empty() ? std::string_view{} : undoStack_.back()->name();
}

std::string_view CommandStack::redoName() const
{
    return redoStack_.empty() ? std::string_view{} : redoStack_.back()->name();
}

std::vector<katana::entity::EntityId> CommandStack::lastCreatedEntities() const
{
    return undoStack_.empty() ? std::vector<katana::entity::EntityId>{}
                              : undoStack_.back()->createdEntities();
}

void CommandStack::clear()
{
    undoStack_.clear();
    redoStack_.clear();
    pendingChanges_.clear();
    markSaved();
}

void CommandStack::markSaved()
{
    savedTop_ = undoStack_.empty() ? nullptr : undoStack_.back().get();
    savedReachable_ = true;
}

bool CommandStack::isModified() const
{
    if (!savedReachable_) {
        return true;
    }
    const Command* top = undoStack_.empty() ? nullptr : undoStack_.back().get();
    return top != savedTop_;
}

void CommandStack::addListener(Listener listener)
{
    listeners_.push_back(std::move(listener));
}

void CommandStack::publish(CommandEventKind kind, const Command& command)
{
    CommandEvent event;
    event.kind = kind;
    event.commandName = std::string(command.name());
    event.changes = std::move(pendingChanges_);
    pendingChanges_.clear();
    if (context_.logger != nullptr && kind != CommandEventKind::Executed) {
        context_.logger->info("command", std::string(toString(kind)),
                              {{"name", event.commandName}});
    }
    for (const Listener& listener : listeners_) {
        listener(event);
    }
}

// ---- Transaction -------------------------------------------------------------------

Transaction::Transaction(std::string name) : name_(std::move(name)) {}

void Transaction::add(CommandPtr command)
{
    if (command) {
        commands_.push_back(std::move(command));
    }
}

bool Transaction::isDestructive() const
{
    return std::any_of(commands_.begin(), commands_.end(),
                       [](const CommandPtr& c) { return c->isDestructive(); });
}

Status Transaction::validate(const CommandContext& context) const
{
    if (commands_.empty()) {
        return makeError(ErrorCode::CommandRejected, "transaction is empty", name_);
    }
    return commands_.front()->validate(context);
}

Status Transaction::execute(CommandContext& context)
{
    for (std::size_t i = 0; i < commands_.size(); ++i) {
        Status status = commands_[i]->validate(context);
        if (status) {
            status = commands_[i]->execute(context);
        }
        if (!status) {
            for (std::size_t done = i; done-- > 0;) {
                (void)commands_[done]->undo(context); // roll back in reverse order
            }
            return makeError(status.error().code, status.error().message,
                             "transaction=" + name_ + " step=" + std::to_string(i + 1) + " " +
                                 status.error().context);
        }
    }
    return {};
}

Status Transaction::undo(CommandContext& context)
{
    for (auto it = commands_.rbegin(); it != commands_.rend(); ++it) {
        if (auto status = (*it)->undo(context); !status) {
            return status;
        }
    }
    return {};
}

Status Transaction::redo(CommandContext& context)
{
    for (const CommandPtr& command : commands_) {
        if (auto status = command->redo(context); !status) {
            return status;
        }
    }
    return {};
}

std::vector<katana::entity::EntityId> Transaction::createdEntities() const
{
    std::vector<katana::entity::EntityId> ids;
    for (const CommandPtr& command : commands_) {
        const auto created = command->createdEntities();
        ids.insert(ids.end(), created.begin(), created.end());
    }
    return ids;
}

} // namespace katana::commands
