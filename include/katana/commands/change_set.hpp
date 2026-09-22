#pragma once

// A ChangeSet is the unit of atomic model mutation: a list of entities to add,
// modify and remove. Almost every command is "compute a ChangeSet from the
// current model, then apply it", which gives all of them the same validation,
// atomicity and undo behaviour for free.

#include <functional>
#include <optional>
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

    // What validate() built, handed on to the execute() that follows it.
    //
    // Every path that executes a command - CommandStack::execute() and
    // Transaction::execute() - calls validate() and then execute() with nothing
    // in between that can touch the model, so the set the two computed was
    // always identical and the first was thrown away. Building it twice costs a
    // full copy of every entity the command adds, which is the whole import on
    // a CREATE_ENTITIES of tens of thousands of entities. validate() still
    // rebuilds every time it is called, so it remains a pure function of the
    // model as it is now; execute() CONSUMES the cache and re-runs
    // validateChangeSet() against the live model regardless, so a set that
    // somehow went stale is rejected rather than applied.
    mutable std::optional<katana::core::Result<ChangeSet>> built_;

    // Recorded by execute(): exact images, so undo/redo never recompute anything.
    std::vector<katana::entity::EntityId> addedIds_; // created entities, in creation order
    // Images of the created entities, filled by undo() and used by redo().
    // execute() deliberately keeps no image: EntityDatabase::remove() hands
    // undo() the entity it removes, which is exactly what redo() must put back,
    // so a copy taken at execute() time duplicated every created Entity - two
    // std::maps each - for something that is only ever needed after an undo.
    std::vector<katana::entity::Entity> addedImages_;
    std::vector<katana::entity::Entity> before_;  // modified entities, prior state
    std::vector<katana::entity::Entity> after_;   // modified entities, new state
    std::vector<katana::entity::Entity> removed_; // removed entities, prior state
};

} // namespace katana::commands
