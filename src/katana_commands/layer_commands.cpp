#include "katana/commands/entity_commands.hpp"

#include <utility>

namespace katana::commands {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Status;
using katana::entity::Layer;

namespace {

// Layer commands share one shape: a validated forward step and its exact inverse.
class CreateLayerCommand final : public Command {
  public:
    explicit CreateLayerCommand(Layer layer) : layer_(std::move(layer)) {}

    [[nodiscard]] std::string_view name() const override { return "CREATE_LAYER"; }

    [[nodiscard]] Status validate(const CommandContext& context) const override
    {
        if (layer_.name.empty()) {
            return makeError(ErrorCode::InvalidArgument, "layer name is empty");
        }
        if (context.model.layers.contains(layer_.name)) {
            return makeError(ErrorCode::AlreadyExists, "layer already exists", layer_.name);
        }
        return {};
    }
    [[nodiscard]] Status execute(CommandContext& context) override
    {
        return context.model.layers.add(layer_);
    }
    [[nodiscard]] Status undo(CommandContext& context) override
    {
        auto removed = context.model.layers.remove(layer_.name);
        return removed ? Status{} : Status{removed.error()};
    }
    [[nodiscard]] Status redo(CommandContext& context) override { return execute(context); }

  private:
    Layer layer_;
};

class UpdateLayerCommand final : public Command {
  public:
    explicit UpdateLayerCommand(Layer layer) : after_(std::move(layer)) {}

    [[nodiscard]] std::string_view name() const override { return "UPDATE_LAYER"; }

    [[nodiscard]] Status validate(const CommandContext& context) const override
    {
        const Layer* current = context.model.layers.find(after_.name);
        if (current == nullptr) {
            return makeError(ErrorCode::NotFound, "layer does not exist", after_.name);
        }
        if (*current == after_) {
            return makeError(ErrorCode::CommandRejected, "layer already has these attributes",
                             after_.name);
        }
        return {};
    }
    [[nodiscard]] Status execute(CommandContext& context) override
    {
        const Layer* current = context.model.layers.find(after_.name);
        if (current == nullptr) {
            return makeError(ErrorCode::NotFound, "layer does not exist", after_.name);
        }
        before_ = *current;
        return context.model.layers.update(after_);
    }
    [[nodiscard]] Status undo(CommandContext& context) override
    {
        return context.model.layers.update(before_);
    }
    [[nodiscard]] Status redo(CommandContext& context) override
    {
        return context.model.layers.update(after_);
    }

  private:
    Layer after_;
    Layer before_;
};

class DeleteLayerCommand final : public Command {
  public:
    explicit DeleteLayerCommand(std::string name) : layerName_(std::move(name)) {}

    [[nodiscard]] std::string_view name() const override { return "DELETE_LAYER"; }
    [[nodiscard]] bool isDestructive() const override { return true; }

    [[nodiscard]] Status validate(const CommandContext& context) const override
    {
        if (layerName_ == katana::entity::kDefaultLayerName) {
            return makeError(ErrorCode::CommandRejected, "the default layer cannot be deleted");
        }
        if (!context.model.layers.contains(layerName_)) {
            return makeError(ErrorCode::NotFound, "layer does not exist", layerName_);
        }
        const std::size_t inUse = context.model.entities.countOnLayer(layerName_);
        if (inUse > 0) {
            return makeError(ErrorCode::CommandRejected, "layer still contains entities",
                             "layer=" + layerName_ + " entities=" + std::to_string(inUse));
        }
        return {};
    }
    [[nodiscard]] Status execute(CommandContext& context) override
    {
        auto removed = context.model.layers.remove(layerName_);
        if (!removed) {
            return removed.error();
        }
        removed_ = std::move(*removed);
        return {};
    }
    [[nodiscard]] Status undo(CommandContext& context) override
    {
        return context.model.layers.add(removed_);
    }
    [[nodiscard]] Status redo(CommandContext& context) override { return execute(context); }

  private:
    std::string layerName_;
    Layer removed_;
};

// Deleting a whole branch. Separate from DeleteLayerCommand rather than a flag
// on it because the checks differ: a branch is only removable when EVERY layer
// in it is empty, and saying which one is not is the difference between a
// usable error and "cannot delete".
class DeleteLayerTreeCommand final : public Command {
  public:
    explicit DeleteLayerTreeCommand(std::string name) : name_(std::move(name)) {}

    [[nodiscard]] std::string_view name() const override { return "DeleteLayerTree"; }

    [[nodiscard]] Status validate(const CommandContext& context) const override
    {
        if (name_ == katana::entity::kDefaultLayerName) {
            return makeError(ErrorCode::CommandRejected, "the default layer cannot be deleted");
        }
        if (!context.model.layers.contains(name_)) {
            return makeError(ErrorCode::NotFound, "layer does not exist", name_);
        }
        for (const std::string& path : context.model.layers.subtree(name_)) {
            const std::size_t inUse = context.model.entities.countOnLayer(path);
            if (inUse > 0) {
                return makeError(ErrorCode::CommandRejected, "a nested layer still has entities",
                                 "layer=" + path + " entities=" + std::to_string(inUse));
            }
        }
        return {};
    }

    [[nodiscard]] Status execute(CommandContext& context) override
    {
        auto removed = context.model.layers.removeSubtree(name_);
        if (!removed) {
            return removed.error();
        }
        removed_ = std::move(*removed);
        return {};
    }

    [[nodiscard]] Status undo(CommandContext& context) override
    {
        // removeSubtree returns deepest first, so replaying it backwards
        // recreates each parent before its children.
        for (auto it = removed_.rbegin(); it != removed_.rend(); ++it) {
            if (context.model.layers.contains(it->name)) {
                // add() created it as an ancestor of a layer already restored;
                // update it so its real colour and flags come back.
                if (auto status = context.model.layers.update(*it); !status) {
                    return status;
                }
                continue;
            }
            if (auto status = context.model.layers.add(*it); !status) {
                return status;
            }
        }
        return {};
    }

    [[nodiscard]] Status redo(CommandContext& context) override { return execute(context); }

  private:
    std::string name_;
    std::vector<Layer> removed_;
};

// Renaming a branch has to move the entities with it, because an entity names
// its layer by full path: leaving them behind would point every one of them at
// a layer that no longer exists. Both halves are in ONE command so that undo
// restores both or neither.
class RenameLayerCommand final : public Command {
  public:
    RenameLayerCommand(std::string from, std::string to)
        : from_(std::move(from)), to_(std::move(to))
    {
    }

    [[nodiscard]] std::string_view name() const override { return "RenameLayer"; }

    [[nodiscard]] Status validate(const CommandContext& context) const override
    {
        if (from_ == katana::entity::kDefaultLayerName) {
            return makeError(ErrorCode::CommandRejected, "the default layer cannot be renamed");
        }
        if (!context.model.layers.contains(from_)) {
            return makeError(ErrorCode::NotFound, "layer does not exist", from_);
        }
        if (auto status = katana::entity::validateLayerPath(to_); !status) {
            return status;
        }
        if (katana::entity::isLayerUnder(to_, from_) && from_ != to_) {
            return makeError(ErrorCode::CommandRejected, "a layer cannot be moved inside itself",
                             from_ + " -> " + to_);
        }
        if (context.model.layers.contains(to_) && from_ != to_) {
            return makeError(ErrorCode::AlreadyExists, "a layer of that name already exists", to_);
        }
        return {};
    }

    [[nodiscard]] Status execute(CommandContext& context) override
    {
        auto mapping = context.model.layers.renameSubtree(from_, to_);
        if (!mapping) {
            return mapping.error();
        }
        mapping_ = std::move(*mapping);
        return moveEntities(context, /*forward=*/true);
    }

    [[nodiscard]] Status undo(CommandContext& context) override
    {
        auto mapping = context.model.layers.renameSubtree(to_, from_);
        if (!mapping) {
            return mapping.error();
        }
        return moveEntities(context, /*forward=*/false);
    }

    [[nodiscard]] Status redo(CommandContext& context) override { return execute(context); }

  private:
    [[nodiscard]] Status moveEntities(CommandContext& context, bool forward)
    {
        // Collected first, then applied: replace() while iterating would mutate
        // the container being walked.
        std::vector<katana::entity::Entity> changed;
        context.model.entities.forEach([&](const katana::entity::Entity& entity) {
            for (const auto& [before, after] : mapping_) {
                const std::string& source = forward ? before : after;
                const std::string& target = forward ? after : before;
                if (entity.layer == source) {
                    katana::entity::Entity moved = entity;
                    moved.layer = target;
                    changed.push_back(std::move(moved));
                    return;
                }
            }
        });
        for (katana::entity::Entity& entity : changed) {
            if (auto status = context.model.entities.replace(std::move(entity)); !status) {
                return status;
            }
        }
        return {};
    }

    std::string from_;
    std::string to_;
    std::vector<std::pair<std::string, std::string>> mapping_;
};

} // namespace

CommandPtr createLayer(Layer layer)
{
    return std::make_unique<CreateLayerCommand>(std::move(layer));
}

CommandPtr updateLayer(Layer layer)
{
    return std::make_unique<UpdateLayerCommand>(std::move(layer));
}

CommandPtr deleteLayer(std::string name)
{
    return std::make_unique<DeleteLayerCommand>(std::move(name));
}



CommandPtr deleteLayerTree(std::string name)
{
    return std::make_unique<DeleteLayerTreeCommand>(std::move(name));
}

CommandPtr renameLayer(std::string from, std::string to)
{
    return std::make_unique<RenameLayerCommand>(std::move(from), std::move(to));
}


namespace {

using katana::entity::Linetype;

class CreateLinetypeCommand final : public Command {
  public:
    explicit CreateLinetypeCommand(Linetype linetype) : linetype_(std::move(linetype)) {}
    [[nodiscard]] std::string_view name() const override { return "CreateLinetype"; }
    [[nodiscard]] Status validate(const CommandContext& context) const override
    {
        if (context.model.linetypes.contains(linetype_.name)) {
            return makeError(ErrorCode::AlreadyExists, "linetype already exists", linetype_.name);
        }
        return katana::entity::validate(linetype_);
    }
    [[nodiscard]] Status execute(CommandContext& context) override
    {
        return context.model.linetypes.add(linetype_);
    }
    [[nodiscard]] Status undo(CommandContext& context) override
    {
        auto removed = context.model.linetypes.remove(linetype_.name);
        return removed ? Status{} : removed.error();
    }
    [[nodiscard]] Status redo(CommandContext& context) override { return execute(context); }

  private:
    Linetype linetype_;
};

class UpdateLinetypeCommand final : public Command {
  public:
    explicit UpdateLinetypeCommand(Linetype linetype) : after_(std::move(linetype)) {}
    [[nodiscard]] std::string_view name() const override { return "UpdateLinetype"; }
    [[nodiscard]] Status validate(const CommandContext& context) const override
    {
        if (!context.model.linetypes.contains(after_.name)) {
            return makeError(ErrorCode::NotFound, "linetype does not exist", after_.name);
        }
        return katana::entity::validate(after_);
    }
    [[nodiscard]] Status execute(CommandContext& context) override
    {
        const Linetype* current = context.model.linetypes.find(after_.name);
        if (current == nullptr) {
            return makeError(ErrorCode::NotFound, "linetype does not exist", after_.name);
        }
        before_ = *current; // before-image, as every other edit command takes one
        return context.model.linetypes.update(after_);
    }
    [[nodiscard]] Status undo(CommandContext& context) override
    {
        return context.model.linetypes.update(before_);
    }
    [[nodiscard]] Status redo(CommandContext& context) override
    {
        return context.model.linetypes.update(after_);
    }

  private:
    Linetype after_;
    Linetype before_;
};

class DeleteLinetypeCommand final : public Command {
  public:
    explicit DeleteLinetypeCommand(std::string name) : name_(std::move(name)) {}
    [[nodiscard]] std::string_view name() const override { return "DeleteLinetype"; }
    [[nodiscard]] Status validate(const CommandContext& context) const override
    {
        if (name_ == katana::entity::kContinuousLinetype) {
            return makeError(ErrorCode::CommandRejected,
                             "the continuous linetype cannot be deleted");
        }
        if (!context.model.linetypes.contains(name_)) {
            return makeError(ErrorCode::NotFound, "linetype does not exist", name_);
        }
        // A layer or style left naming a deleted pattern would resolve to
        // continuous and draw solid with nothing to say why, so the reference
        // is reported instead - naming the layer, so it can be found.
        for (const std::string& layer : context.model.layers.names()) {
            const katana::entity::Layer* definition = context.model.layers.find(layer);
            if (definition != nullptr && definition->linetype == name_) {
                return makeError(ErrorCode::CommandRejected, "a layer still uses that linetype",
                                 "layer=" + layer);
            }
        }
        for (const katana::entity::Style& style : context.model.styles.all()) {
            if (style.linetype == name_) {
                return makeError(ErrorCode::CommandRejected, "a style still uses that linetype",
                                 "style=" + style.name);
            }
        }
        return {};
    }
    [[nodiscard]] Status execute(CommandContext& context) override
    {
        auto removed = context.model.linetypes.remove(name_);
        if (!removed) {
            return removed.error();
        }
        removed_ = std::move(*removed);
        return {};
    }
    [[nodiscard]] Status undo(CommandContext& context) override
    {
        return context.model.linetypes.add(removed_);
    }
    [[nodiscard]] Status redo(CommandContext& context) override { return execute(context); }

  private:
    std::string name_;
    Linetype removed_;
};

} // namespace

CommandPtr createLinetype(Linetype linetype)
{
    return std::make_unique<CreateLinetypeCommand>(std::move(linetype));
}

CommandPtr updateLinetype(Linetype linetype)
{
    return std::make_unique<UpdateLinetypeCommand>(std::move(linetype));
}

CommandPtr deleteLinetype(std::string name)
{
    return std::make_unique<DeleteLinetypeCommand>(std::move(name));
}

} // namespace katana::commands
