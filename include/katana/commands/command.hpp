#pragma once

// Command system core (PLAN.MD Phase 06).
//
//   Command -> Validation -> Transaction -> Domain Model -> Change Events
//
// Every modification of the model is a Command executed through a CommandStack.
// This is also the only door the AI layer will get (Rule 2), so commands are
// named, validated before they touch anything, atomic, and reversible.
//
// Contract for implementers
//   validate()  has no side effects and may be called at any time.
//   execute()   either fully succeeds or leaves the model exactly as it found it.
//   undo()      restores the exact prior state (before-images, not inverse maths).
//   redo()      reproduces the result of execute(), including entity ids.

#include <memory>
#include <string>
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/core/log.hpp"
#include "katana/entity/model.hpp"

namespace katana::commands {

struct CommandContext {
    katana::entity::Model& model;
    katana::core::Logger* logger = nullptr; // optional
};

class Command {
  public:
    virtual ~Command() = default;

    // Stable upper-case identifier, e.g. "CREATE_LINE".
    [[nodiscard]] virtual std::string_view name() const = 0;

    // True when the command discards user data (DELETE...). The AI safety model
    // requires user confirmation for destructive commands (PLAN.MD section 31).
    [[nodiscard]] virtual bool isDestructive() const { return false; }

    [[nodiscard]] virtual katana::core::Status validate(const CommandContext& context) const = 0;
    [[nodiscard]] virtual katana::core::Status execute(CommandContext& context) = 0;
    [[nodiscard]] virtual katana::core::Status undo(CommandContext& context) = 0;
    [[nodiscard]] virtual katana::core::Status redo(CommandContext& context) = 0;

    // Entities created by the last execute()/redo(); empty for other commands.
    [[nodiscard]] virtual std::vector<katana::entity::EntityId> createdEntities() const
    {
        return {};
    }
};

using CommandPtr = std::unique_ptr<Command>;

enum class CommandEventKind { Executed, Undone, Redone };

// Published by the CommandStack after the model changed.
struct CommandEvent {
    CommandEventKind kind = CommandEventKind::Executed;
    std::string commandName;
    std::vector<katana::entity::ChangeEvent> changes; // entity-level detail, in order
};

} // namespace katana::commands
