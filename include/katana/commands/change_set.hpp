#pragma once

// A ChangeSet is the unit of atomic model mutation: a list of entities to add,
// modify and remove. Almost every command is "compute a ChangeSet from the
// current model, then apply it", which gives all of them the same validation,
// atomicity and undo behaviour for free.

#include <functional>
#include <string>
#include <vector>

#include "katana/commands/command.hpp"

namespace katana::commands {

struct ChangeSet {
    std::vector<katana::entity::Entity> add;      // ids are assigned on apply
    std::vector<katana::entity::Entity> modify;   // matched by id
    std::vector<katana::entity::EntityId> remove;

    [[nodiscard]] bool empty() const { return add.empty() && modify.empty() && remove.empty(); }
};

// Checks a ChangeSet against the model without applying it:
//   * modified / removed entities exist and sit on unlocked layers,
//   * no entity is both modified and removed, or listed twice,
//   * added / modified entities have valid geometry, an existing unlocked
//     target layer, an existing style (when named) and well-typed properties.
[[nodiscard]] katana::core::Status validateChangeSet(const ChangeSet& changes,
                                                     const katana::entity::Model& model);

// Command whose effect is described by a ChangeSet built on demand.
class ChangeSetCommand : public Command {
  public:
    using Builder = std::function<katana::core::Result<ChangeSet>(const CommandContext&)>;

    ChangeSetCommand(std::string name, Builder builder, bool destructive = false);

    [[nodiscard]] std::string_view name() const override { return name_; }
    [[nodiscard]] bool isDestructive() const override { return destructive_; }

    [[nodiscard]] katana::core::Status validate(const CommandContext& context) const override;
    [[nodiscard]] katana::core::Status execute(CommandContext& context) override;
    [[nodiscard]] katana::core::Status undo(CommandContext& context) override;
    [[nodiscard]] katana::core::Status redo(CommandContext& context) override;
    [[nodiscard]] std::vector<katana::entity::EntityId> createdEntities() const override;

  private:
    enum class State { Pending, Applied, Reverted };

    std::string name_;
    Builder builder_;
    bool destructive_ = false;
    State state_ = State::Pending;

    // Recorded by execute(): exact images, so undo/redo never recompute anything.
    std::vector<katana::entity::Entity> added_;   // with their assigned ids
    std::vector<katana::entity::Entity> before_;  // modified entities, prior state
    std::vector<katana::entity::Entity> after_;   // modified entities, new state
    std::vector<katana::entity::Entity> removed_; // removed entities, prior state
};

} // namespace katana::commands
