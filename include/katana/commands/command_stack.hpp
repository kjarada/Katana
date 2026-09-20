#pragma once

// Executes commands against a model and keeps the undo / redo history.
//
// Ownership  The stack owns executed commands. The model is referenced and must
//            outlive the stack.
// Observer   The stack installs itself as the EntityDatabase observer of the
//            model so it can attach entity-level changes to CommandEvents.
//            Code that needs change notifications subscribes to the stack.
// Threading  Single-threaded: confined to the thread that owns the model.

#include <cstddef>
#include <functional>
#include <memory>
#include <string_view>
#include <vector>

#include "katana/commands/command.hpp"

namespace katana::commands {

class CommandStack {
  public:
    using Listener = std::function<void(const CommandEvent&)>;

    explicit CommandStack(katana::entity::Model& model, katana::core::Logger* logger = nullptr);
    ~CommandStack();

    CommandStack(const CommandStack&) = delete;
    CommandStack& operator=(const CommandStack&) = delete;

    // validate() then execute(). On success the command becomes the newest undo
    // step and the redo history is discarded. On failure the model is unchanged
    // and the command is dropped.
    [[nodiscard]] katana::core::Status execute(CommandPtr command);

    [[nodiscard]] katana::core::Status undo();
    [[nodiscard]] katana::core::Status redo();

    [[nodiscard]] bool canUndo() const { return !undoStack_.empty(); }
    [[nodiscard]] bool canRedo() const { return !redoStack_.empty(); }
    [[nodiscard]] std::size_t undoCount() const { return undoStack_.size(); }
    [[nodiscard]] std::size_t redoCount() const { return redoStack_.size(); }
    // Empty when there is nothing to undo / redo.
    [[nodiscard]] std::string_view undoName() const;
    [[nodiscard]] std::string_view redoName() const;

    // Entities created by the most recently executed or redone command.
    [[nodiscard]] std::vector<katana::entity::EntityId> lastCreatedEntities() const;

    // Forgets all history (after loading another document).
    void clear();

    // Save-point tracking: isModified() is true whenever the model differs from
    // the state at the last markSaved(), including after undoing past it.
    void markSaved();
    [[nodiscard]] bool isModified() const;

    void addListener(Listener listener);

  private:
    void publish(CommandEventKind kind, const Command& command);

    CommandContext context_;
    std::vector<CommandPtr> undoStack_;
    std::vector<CommandPtr> redoStack_;
    std::vector<Listener> listeners_;
    std::vector<katana::entity::ChangeEvent> pendingChanges_;

    // Identity of the command on top of the undo stack at the last save; null
    // when saved with an empty stack. `savedReachable_` turns false once that
    // state can no longer be reached by undo/redo.
    const Command* savedTop_ = nullptr;
    bool savedReachable_ = true;
};

// Executes several commands as one atomic, single-step-undoable command. If any
// part fails, the parts already executed are undone and the failure is returned.
class Transaction : public Command {
  public:
    explicit Transaction(std::string name);

    void add(CommandPtr command);
    [[nodiscard]] std::size_t size() const { return commands_.size(); }

    [[nodiscard]] std::string_view name() const override { return name_; }
    [[nodiscard]] bool isDestructive() const override;

    // Only the first part can be validated up front: later parts may depend on
    // the effects of earlier ones, so they are validated as they execute.
    [[nodiscard]] katana::core::Status validate(const CommandContext& context) const override;
    [[nodiscard]] katana::core::Status execute(CommandContext& context) override;
    [[nodiscard]] katana::core::Status undo(CommandContext& context) override;
    [[nodiscard]] katana::core::Status redo(CommandContext& context) override;
    [[nodiscard]] std::vector<katana::entity::EntityId> createdEntities() const override;

  private:
    std::string name_;
    std::vector<CommandPtr> commands_;
};

} // namespace katana::commands
