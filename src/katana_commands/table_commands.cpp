// Create, update and delete for every NamedTable in the model: linetypes,
// dimension styles, hatch patterns, alignments and styles.
//
// ONE implementation, parameterised by a policy per table. There used to be
// four hand-written copies of the same three classes (and a fifth was about
// to be written for styles), each thirty lines that differed only in the
// noun, the table and the guards - which is the "second way of doing
// something" CLAUDE.md section 6 says to collapse. What genuinely differs
// between the tables is in `TablePolicy<T>`:
//
//   * the item that may not be deleted (continuous, Standard, none);
//   * an update that may not be made (the "none" hatch given a fill);
//   * who still uses an item, so that a deletion is refused NAMING the holder
//     rather than leaving a layer to draw solid with nothing to say why;
//   * how to repoint those holders, which is what a rename is.
//
// The guard and the repoint are two readings of the same references, so a
// rename ENDS by asking the guard whether anything still names the old item:
// if the two ever disagree it fails loudly instead of leaving an entity
// pointing at a style that no longer exists.
//
// Command names ("CreateLinetype", "DeleteStyle" ...) are unchanged, since a
// history shows them.

#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include "katana/commands/entity_commands.hpp"

namespace katana::commands {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Status;

namespace {

// Repointing helpers for the renames below. Each walks one table and
// rewrites the named field of every holder of `from`, through that table's
// own update/replace so that validation and change notification happen
// exactly as they would for a user edit.
template <typename Member>
Status repointLayers(katana::entity::Model& model, Member member, std::string_view from,
                     const std::string& to)
{
    for (const std::string& name : model.layers.names()) {
        const katana::entity::Layer* current = model.layers.find(name);
        if (current == nullptr || current->*member != from) {
            continue;
        }
        katana::entity::Layer updated = *current;
        updated.*member = to;
        if (auto status = model.layers.update(updated); !status) {
            return status;
        }
    }
    return {};
}

template <typename Member>
Status repointStyles(katana::entity::Model& model, Member member, std::string_view from,
                     const std::string& to)
{
    for (const katana::entity::Style& style : model.styles.all()) {
        if (style.*member != from) {
            continue;
        }
        katana::entity::Style updated = style;
        updated.*member = to;
        if (auto status = model.styles.update(updated); !status) {
            return status;
        }
    }
    return {};
}

Status repointEntityStyles(katana::entity::Model& model, std::string_view from,
                           const std::string& to)
{
    // Collected first: forEach may not mutate the database underneath itself.
    std::vector<katana::entity::Entity> wearers;
    model.entities.forEach([&](const katana::entity::Entity& entity) {
        if (entity.style == from) {
            wearers.push_back(entity);
        }
    });
    for (katana::entity::Entity& entity : wearers) {
        entity.style = to;
        if (auto status = model.entities.replace(std::move(entity)); !status) {
            return status;
        }
    }
    return {};
}

template <typename T> struct TablePolicy;

template <> struct TablePolicy<katana::entity::Linetype> {
    static constexpr std::string_view kNoun = "linetype";
    static constexpr std::string_view kStem = "Linetype";
    static auto& table(katana::entity::Model& model) { return model.linetypes; }
    static const auto& table(const katana::entity::Model& model) { return model.linetypes; }
    static Status deletable(std::string_view name)
    {
        if (name == katana::entity::kContinuousLinetype) {
            return makeError(ErrorCode::CommandRejected, "the continuous linetype cannot be deleted");
        }
        return {};
    }
    static Status updatable(const katana::entity::Linetype&) { return {}; }
    static Status inUse(const katana::entity::Model& model, std::string_view name)
    {
        for (const std::string& layer : model.layers.names()) {
            const katana::entity::Layer* definition = model.layers.find(layer);
            if (definition != nullptr && definition->linetype == name) {
                return makeError(ErrorCode::CommandRejected, "a layer still uses that linetype",
                                 "layer=" + layer);
            }
        }
        for (const katana::entity::Style& style : model.styles.all()) {
            if (style.linetype == name) {
                return makeError(ErrorCode::CommandRejected, "a style still uses that linetype",
                                 "style=" + style.name);
            }
        }
        return {};
    }
    static Status repoint(katana::entity::Model& model, std::string_view from,
                          const std::string& to)
    {
        if (auto status = repointLayers(model, &katana::entity::Layer::linetype, from, to);
            !status) {
            return status;
        }
        return repointStyles(model, &katana::entity::Style::linetype, from, to);
    }
};

template <> struct TablePolicy<katana::entity::DimensionStyle> {
    static constexpr std::string_view kNoun = "dimension style";
    static constexpr std::string_view kStem = "DimensionStyle";
    static auto& table(katana::entity::Model& model) { return model.dimensionStyles; }
    static const auto& table(const katana::entity::Model& model) { return model.dimensionStyles; }
    static Status deletable(std::string_view name)
    {
        if (name == katana::entity::kDefaultDimensionStyleName) {
            return makeError(ErrorCode::CommandRejected,
                             "the default dimension style cannot be deleted");
        }
        return {};
    }
    static Status updatable(const katana::entity::DimensionStyle&) { return {}; }
    static Status inUse(const katana::entity::Model& model, std::string_view name)
    {
        // A layer left naming a deleted style falls back to the default and
        // its dimensions silently change size.
        for (const std::string& layer : model.layers.names()) {
            const katana::entity::Layer* definition = model.layers.find(layer);
            if (definition != nullptr && definition->dimensionStyle == name) {
                return makeError(ErrorCode::CommandRejected,
                                 "a layer still uses that dimension style", "layer=" + layer);
            }
        }
        return {};
    }
};

template <> struct TablePolicy<katana::entity::HatchPattern> {
    static constexpr std::string_view kNoun = "hatch pattern";
    static constexpr std::string_view kStem = "HatchPattern";
    static auto& table(katana::entity::Model& model) { return model.hatchPatterns; }
    static const auto& table(const katana::entity::Model& model) { return model.hatchPatterns; }
    static Status deletable(std::string_view name)
    {
        if (name == katana::entity::kNoHatch) {
            return makeError(ErrorCode::CommandRejected,
                             "the \"none\" hatch pattern cannot be deleted");
        }
        return {};
    }
    static Status updatable(const katana::entity::HatchPattern& after)
    {
        if (after.name == katana::entity::kNoHatch && !after.drawsNothing()) {
            return makeError(ErrorCode::CommandRejected,
                             "the \"none\" hatch pattern cannot be given a fill");
        }
        return {};
    }
    static Status inUse(const katana::entity::Model& model, std::string_view name)
    {
        for (const std::string& layer : model.layers.names()) {
            const katana::entity::Layer* definition = model.layers.find(layer);
            if (definition != nullptr && definition->hatchPattern == name) {
                return makeError(ErrorCode::CommandRejected,
                                 "a layer still uses that hatch pattern", "layer=" + layer);
            }
        }
        for (const katana::entity::Style& style : model.styles.all()) {
            if (style.hatchPattern == name) {
                return makeError(ErrorCode::CommandRejected,
                                 "a style still uses that hatch pattern", "style=" + style.name);
            }
        }
        return {};
    }
};

template <> struct TablePolicy<katana::entity::Alignment> {
    static constexpr std::string_view kNoun = "alignment";
    static constexpr std::string_view kStem = "Alignment";
    static auto& table(katana::entity::Model& model) { return model.alignments; }
    static const auto& table(const katana::entity::Model& model) { return model.alignments; }
    static Status deletable(std::string_view) { return {}; }
    static Status updatable(const katana::entity::Alignment&) { return {}; }
    // Nothing in the model references an alignment by name yet - sections
    // are cut and shown, not stored. When profiles or corridors are stored
    // they will, and this is where the guard belongs.
    static Status inUse(const katana::entity::Model&, std::string_view) { return {}; }
};

template <> struct TablePolicy<katana::entity::Style> {
    static constexpr std::string_view kNoun = "style";
    static constexpr std::string_view kStem = "Style";
    static auto& table(katana::entity::Model& model) { return model.styles; }
    static const auto& table(const katana::entity::Model& model) { return model.styles; }
    static Status deletable(std::string_view) { return {}; }
    static Status updatable(const katana::entity::Style&) { return {}; }
    static Status inUse(const katana::entity::Model& model, std::string_view name)
    {
        // An entity left naming a deleted style would draw ByLayer with
        // nothing to say why; the first holder is named so it can be found.
        katana::entity::EntityId holder = katana::entity::kInvalidEntityId;
        model.entities.forEach([&](const katana::entity::Entity& entity) {
            if (holder == katana::entity::kInvalidEntityId && entity.style == name) {
                holder = entity.id;
            }
        });
        if (holder != katana::entity::kInvalidEntityId) {
            return makeError(ErrorCode::CommandRejected, "an entity still uses that style",
                             "id=" + std::to_string(holder));
        }
        return {};
    }
    static Status repoint(katana::entity::Model& model, std::string_view from,
                          const std::string& to)
    {
        return repointEntityStyles(model, from, to);
    }
};

template <typename T> std::string commandName(std::string_view verb)
{
    return std::string(verb) + std::string(TablePolicy<T>::kStem);
}

template <typename T> class CreateItemCommand final : public Command {
  public:
    explicit CreateItemCommand(T item) : item_(std::move(item)), name_(commandName<T>("Create")) {}
    [[nodiscard]] std::string_view name() const override { return name_; }
    [[nodiscard]] Status validate(const CommandContext& context) const override
    {
        if (TablePolicy<T>::table(context.model).contains(item_.name)) {
            return makeError(ErrorCode::AlreadyExists,
                             std::string(TablePolicy<T>::kNoun) + " already exists", item_.name);
        }
        return katana::entity::validate(item_);
    }
    [[nodiscard]] Status execute(CommandContext& context) override
    {
        return TablePolicy<T>::table(context.model).add(item_);
    }
    [[nodiscard]] Status undo(CommandContext& context) override
    {
        auto removed = TablePolicy<T>::table(context.model).remove(item_.name);
        return removed ? Status{} : removed.error();
    }
    [[nodiscard]] Status redo(CommandContext& context) override { return execute(context); }

  private:
    T item_;
    std::string name_;
};

template <typename T> class UpdateItemCommand final : public Command {
  public:
    explicit UpdateItemCommand(T item) : after_(std::move(item)), name_(commandName<T>("Update")) {}
    [[nodiscard]] std::string_view name() const override { return name_; }
    [[nodiscard]] Status validate(const CommandContext& context) const override
    {
        if (!TablePolicy<T>::table(context.model).contains(after_.name)) {
            return makeError(ErrorCode::NotFound,
                             std::string(TablePolicy<T>::kNoun) + " does not exist", after_.name);
        }
        if (auto status = TablePolicy<T>::updatable(after_); !status) {
            return status;
        }
        return katana::entity::validate(after_);
    }
    [[nodiscard]] Status execute(CommandContext& context) override
    {
        const T* current = TablePolicy<T>::table(context.model).find(after_.name);
        if (current == nullptr) {
            return makeError(ErrorCode::NotFound,
                             std::string(TablePolicy<T>::kNoun) + " does not exist", after_.name);
        }
        before_ = *current; // before-image, as every other edit command takes one
        return TablePolicy<T>::table(context.model).update(after_);
    }
    [[nodiscard]] Status undo(CommandContext& context) override
    {
        return TablePolicy<T>::table(context.model).update(before_);
    }
    [[nodiscard]] Status redo(CommandContext& context) override
    {
        return TablePolicy<T>::table(context.model).update(after_);
    }

  private:
    T after_;
    T before_;
    std::string name_;
};

template <typename T> class DeleteItemCommand final : public Command {
  public:
    explicit DeleteItemCommand(std::string name)
        : itemName_(std::move(name)), name_(commandName<T>("Delete"))
    {
    }
    [[nodiscard]] std::string_view name() const override { return name_; }
    [[nodiscard]] Status validate(const CommandContext& context) const override
    {
        if (auto status = TablePolicy<T>::deletable(itemName_); !status) {
            return status;
        }
        if (!TablePolicy<T>::table(context.model).contains(itemName_)) {
            return makeError(ErrorCode::NotFound,
                             std::string(TablePolicy<T>::kNoun) + " does not exist", itemName_);
        }
        return TablePolicy<T>::inUse(context.model, itemName_);
    }
    [[nodiscard]] Status execute(CommandContext& context) override
    {
        auto removed = TablePolicy<T>::table(context.model).remove(itemName_);
        if (!removed) {
            return removed.error();
        }
        removed_ = std::move(*removed);
        return {};
    }
    [[nodiscard]] Status undo(CommandContext& context) override
    {
        return TablePolicy<T>::table(context.model).add(removed_);
    }
    [[nodiscard]] Status redo(CommandContext& context) override { return execute(context); }

  private:
    std::string itemName_;
    T removed_;
    std::string name_;
};

// Remove, re-add under the new name, repoint every holder. NOT an update of
// the name field: a NamedTable is keyed by name, so a rename is a move, and
// remove+add keeps the table's own validation of the new name rather than a
// second copy of those rules here.
template <typename T> class RenameItemCommand final : public Command {
  public:
    RenameItemCommand(std::string from, std::string to)
        : from_(std::move(from)), to_(std::move(to)), name_(commandName<T>("Rename"))
    {
    }
    [[nodiscard]] std::string_view name() const override { return name_; }
    [[nodiscard]] Status validate(const CommandContext& context) const override
    {
        // A protected item is protected from being renamed away as much as
        // from being deleted: everything that resolves to it by name would
        // silently change what it draws.
        if (auto status = TablePolicy<T>::deletable(from_); !status) {
            return status;
        }
        const T* current = TablePolicy<T>::table(context.model).find(from_);
        if (current == nullptr) {
            return makeError(ErrorCode::NotFound,
                             std::string(TablePolicy<T>::kNoun) + " does not exist", from_);
        }
        if (TablePolicy<T>::table(context.model).contains(to_)) {
            return makeError(ErrorCode::AlreadyExists,
                             std::string(TablePolicy<T>::kNoun) + " already exists", to_);
        }
        T renamed = *current;
        renamed.name = to_;
        return katana::entity::validate(renamed);
    }
    [[nodiscard]] Status execute(CommandContext& context) override
    {
        return move(context.model, from_, to_);
    }
    [[nodiscard]] Status undo(CommandContext& context) override
    {
        return move(context.model, to_, from_);
    }
    [[nodiscard]] Status redo(CommandContext& context) override { return execute(context); }

  private:
    static Status move(katana::entity::Model& model, const std::string& from,
                       const std::string& to)
    {
        auto removed = TablePolicy<T>::table(model).remove(from);
        if (!removed) {
            return removed.error();
        }
        T item = std::move(*removed);
        item.name = to;
        if (auto status = TablePolicy<T>::table(model).add(std::move(item)); !status) {
            return status;
        }
        if (auto status = TablePolicy<T>::repoint(model, from, to); !status) {
            return status;
        }
        return TablePolicy<T>::inUse(model, from);
    }

    std::string from_;
    std::string to_;
    std::string name_;
};

} // namespace

CommandPtr createLinetype(katana::entity::Linetype linetype)
{
    return std::make_unique<CreateItemCommand<katana::entity::Linetype>>(std::move(linetype));
}
CommandPtr updateLinetype(katana::entity::Linetype linetype)
{
    return std::make_unique<UpdateItemCommand<katana::entity::Linetype>>(std::move(linetype));
}
CommandPtr deleteLinetype(std::string name)
{
    return std::make_unique<DeleteItemCommand<katana::entity::Linetype>>(std::move(name));
}
CommandPtr renameLinetype(std::string from, std::string to)
{
    return std::make_unique<RenameItemCommand<katana::entity::Linetype>>(std::move(from),
                                                                        std::move(to));
}

CommandPtr createDimensionStyle(katana::entity::DimensionStyle style)
{
    return std::make_unique<CreateItemCommand<katana::entity::DimensionStyle>>(std::move(style));
}
CommandPtr updateDimensionStyle(katana::entity::DimensionStyle style)
{
    return std::make_unique<UpdateItemCommand<katana::entity::DimensionStyle>>(std::move(style));
}
CommandPtr deleteDimensionStyle(std::string name)
{
    return std::make_unique<DeleteItemCommand<katana::entity::DimensionStyle>>(std::move(name));
}

CommandPtr createHatchPattern(katana::entity::HatchPattern pattern)
{
    return std::make_unique<CreateItemCommand<katana::entity::HatchPattern>>(std::move(pattern));
}
CommandPtr updateHatchPattern(katana::entity::HatchPattern pattern)
{
    return std::make_unique<UpdateItemCommand<katana::entity::HatchPattern>>(std::move(pattern));
}
CommandPtr deleteHatchPattern(std::string name)
{
    return std::make_unique<DeleteItemCommand<katana::entity::HatchPattern>>(std::move(name));
}

CommandPtr createAlignment(katana::entity::Alignment alignment)
{
    return std::make_unique<CreateItemCommand<katana::entity::Alignment>>(std::move(alignment));
}
CommandPtr updateAlignment(katana::entity::Alignment alignment)
{
    return std::make_unique<UpdateItemCommand<katana::entity::Alignment>>(std::move(alignment));
}
CommandPtr deleteAlignment(std::string name)
{
    return std::make_unique<DeleteItemCommand<katana::entity::Alignment>>(std::move(name));
}

CommandPtr createStyle(katana::entity::Style style)
{
    return std::make_unique<CreateItemCommand<katana::entity::Style>>(std::move(style));
}
CommandPtr updateStyle(katana::entity::Style style)
{
    return std::make_unique<UpdateItemCommand<katana::entity::Style>>(std::move(style));
}
CommandPtr deleteStyle(std::string name)
{
    return std::make_unique<DeleteItemCommand<katana::entity::Style>>(std::move(name));
}
CommandPtr renameStyle(std::string from, std::string to)
{
    return std::make_unique<RenameItemCommand<katana::entity::Style>>(std::move(from),
                                                                     std::move(to));
}

} // namespace katana::commands
