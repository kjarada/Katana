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
// pointing at a style that no longer exists. The guard itself reads
// entity::tableUsage - the one pass the managers' "Used" column and purge
// read too - so a refusal says how many hold the item and names the first,
// and no manager can call an item unused that a delete would then refuse.
//
// Command names ("CreateLinetype", "DeleteStyle" ...) are unchanged, since a
// history shows them.

#include <algorithm>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include "katana/commands/entity_commands.hpp"
#include "katana/entity/table_usage.hpp"

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

// The refusal every guard gives: how many hold the item and the first of
// them, so "used by 418 entities, e.g. id=12" can be acted on where "an
// entity still uses that style" could not.
Status refuseIfUsed(const katana::entity::Users& users, std::string_view noun)
{
    if (!users.used()) {
        return {};
    }
    return makeError(ErrorCode::CommandRejected, "that " + std::string(noun) + " is still used",
                     users.describe());
}

// Before-images of everything a merge repoints, so its undo puts back
// exactly the holders it moved - and not the ones that already named the
// target before it ran.
struct HolderImages {
    std::vector<katana::entity::Layer> layers;
    std::vector<katana::entity::Style> styles;
    std::vector<katana::entity::Entity> entities;
};

Status restoreHolders(katana::entity::Model& model, const HolderImages& images)
{
    for (const katana::entity::Layer& layer : images.layers) {
        if (auto status = model.layers.update(layer); !status) {
            return status;
        }
    }
    for (const katana::entity::Style& style : images.styles) {
        if (auto status = model.styles.update(style); !status) {
            return status;
        }
    }
    for (const katana::entity::Entity& entity : images.entities) {
        if (auto status = model.entities.replace(entity); !status) {
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
    // The table's own rule, asked rather than restated: validate() said a
    // pattern on "continuous" was fine and execute() then refused it
    // (audit MOD-09).
    static Status updatable(const katana::entity::Linetype& after)
    {
        return katana::entity::LinetypePolicy::checkUpdate(after);
    }
    // Held by the layers and styles naming it; the entities reaching it
    // through them are counted too, since that is what a person weighs.
    static Status inUse(const katana::entity::Model& model, std::string_view name)
    {
        return refuseIfUsed(
            katana::entity::TableUsage::of(katana::entity::tableUsage(model).linetypes, name),
            kNoun);
    }
    static HolderImages holders(const katana::entity::Model& model, std::string_view name)
    {
        const katana::entity::TableUsage usage = katana::entity::tableUsage(model);
        const katana::entity::Users& users = katana::entity::TableUsage::of(usage.linetypes, name);
        HolderImages images;
        for (const std::string& layer : users.layers) {
            if (const katana::entity::Layer* found = model.layers.find(layer); found != nullptr) {
                images.layers.push_back(*found);
            }
        }
        for (const std::string& style : users.styles) {
            if (const katana::entity::Style* found = model.styles.find(style); found != nullptr) {
                images.styles.push_back(*found);
            }
        }
        return images;
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
        return refuseIfUsed(
            katana::entity::TableUsage::of(katana::entity::tableUsage(model).hatchPatterns, name),
            kNoun);
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
        // nothing to say why; the count and the first holder are given so
        // they can be found.
        return refuseIfUsed(
            katana::entity::TableUsage::of(katana::entity::tableUsage(model).styles, name), kNoun);
    }
    static HolderImages holders(const katana::entity::Model& model, std::string_view name)
    {
        const katana::entity::TableUsage usage =
            katana::entity::tableUsage(model, katana::entity::UsageOptions{.entityIds = true});
        HolderImages images;
        for (const katana::entity::EntityId id :
             katana::entity::TableUsage::of(usage.styles, name).entityIds) {
            if (const katana::entity::Entity* found = model.entities.find(id); found != nullptr) {
                images.entities.push_back(*found);
            }
        }
        return images;
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
    // A deletion discards the definition - a dash pattern, an alignment's PI
    // and PVI design - so it asks for the same confirmation DELETE_LAYER does
    // (audit MOD-08).
    [[nodiscard]] bool isDestructive() const override { return true; }
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

// Repoint every holder of `from` to `into`, then delete `from`: what a rename
// onto an existing name has to be, and why RenameItemCommand refuses one.
// One command, so one undo puts back the item AND every holder it moved.
template <typename T> class MergeItemCommand final : public Command {
  public:
    MergeItemCommand(std::string from, std::string into)
        : from_(std::move(from)), into_(std::move(into)), name_(commandName<T>("Merge"))
    {
    }
    [[nodiscard]] std::string_view name() const override { return name_; }
    // The merged item's own definition is discarded.
    [[nodiscard]] bool isDestructive() const override { return true; }
    [[nodiscard]] Status validate(const CommandContext& context) const override
    {
        const std::string noun(TablePolicy<T>::kNoun);
        if (from_ == into_) {
            return makeError(ErrorCode::InvalidArgument, "a " + noun + " cannot be merged into itself",
                             from_);
        }
        // Protected for the reason a rename is: everything that resolves to
        // it by name would silently change what it draws.
        if (auto status = TablePolicy<T>::deletable(from_); !status) {
            return status;
        }
        if (!TablePolicy<T>::table(context.model).contains(from_)) {
            return makeError(ErrorCode::NotFound, noun + " does not exist", from_);
        }
        if (!TablePolicy<T>::table(context.model).contains(into_)) {
            return makeError(ErrorCode::NotFound, "the " + noun + " to merge into does not exist",
                             into_);
        }
        return {};
    }
    [[nodiscard]] Status execute(CommandContext& context) override
    {
        katana::entity::Model& model = context.model;
        holders_ = TablePolicy<T>::holders(model, from_);
        auto removed = TablePolicy<T>::table(model).remove(from_);
        if (!removed) {
            return removed.error();
        }
        removed_ = std::move(*removed);
        Status status = TablePolicy<T>::repoint(model, from_, into_);
        if (status) {
            // The rename's own check: the guard and the repoint read the
            // same references, so anything still naming `from` is a bug to
            // report, not an entity to leave pointing at nothing.
            status = TablePolicy<T>::inUse(model, from_);
        }
        if (!status) {
            (void)rollBack(model); // execute() leaves the model as it found it
            return status;
        }
        return {};
    }
    [[nodiscard]] Status undo(CommandContext& context) override { return rollBack(context.model); }
    [[nodiscard]] Status redo(CommandContext& context) override { return execute(context); }

  private:
    Status rollBack(katana::entity::Model& model)
    {
        if (auto status = restoreHolders(model, holders_); !status) {
            return status;
        }
        return TablePolicy<T>::table(model).add(removed_);
    }

    std::string from_;
    std::string into_;
    std::string name_;
    HolderImages holders_{};
    T removed_{};
};

// A copy under a new name: the start of "a style like that one, but red".
template <typename T> class DuplicateItemCommand final : public Command {
  public:
    DuplicateItemCommand(std::string from, std::string to)
        : from_(std::move(from)), to_(std::move(to)), name_(commandName<T>("Duplicate"))
    {
    }
    [[nodiscard]] std::string_view name() const override { return name_; }
    [[nodiscard]] Status validate(const CommandContext& context) const override
    {
        const T* source = TablePolicy<T>::table(context.model).find(from_);
        if (source == nullptr) {
            return makeError(ErrorCode::NotFound,
                             std::string(TablePolicy<T>::kNoun) + " does not exist", from_);
        }
        if (TablePolicy<T>::table(context.model).contains(to_)) {
            return makeError(ErrorCode::AlreadyExists,
                             std::string(TablePolicy<T>::kNoun) + " already exists", to_);
        }
        T copy = *source;
        copy.name = to_;
        return katana::entity::validate(copy);
    }
    [[nodiscard]] Status execute(CommandContext& context) override
    {
        const T* source = TablePolicy<T>::table(context.model).find(from_);
        if (source == nullptr) {
            return makeError(ErrorCode::NotFound,
                             std::string(TablePolicy<T>::kNoun) + " does not exist", from_);
        }
        copy_ = *source;
        copy_.name = to_;
        return TablePolicy<T>::table(context.model).add(copy_);
    }
    [[nodiscard]] Status undo(CommandContext& context) override
    {
        auto removed = TablePolicy<T>::table(context.model).remove(to_);
        return removed ? Status{} : removed.error();
    }
    // The copy taken at execute(), not a fresh one: redo reproduces what
    // execute did even if `from` has been edited in between by a command
    // that has since been undone.
    [[nodiscard]] Status redo(CommandContext& context) override
    {
        return TablePolicy<T>::table(context.model).add(copy_);
    }

  private:
    std::string from_;
    std::string to_;
    std::string name_;
    T copy_{};
};

template <typename T> CommandPtr updateIfChanged(const katana::entity::Model& model, T item)
{
    // Saving a form nobody edited is not an edit: no undo step, and the
    // project is not marked modified by it.
    if (const T* current = TablePolicy<T>::table(model).find(item.name);
        current != nullptr && *current == item) {
        return nullptr;
    }
    return std::make_unique<UpdateItemCommand<T>>(std::move(item));
}

// A purge: many unused items as ONE step. Its guard is one usage pass for
// the whole set - one DeleteItemCommand per item would be one pass over the
// entities per item - and it is judged as the set: a linetype named only
// by styles this purge also removes is free.
class PurgeItemsCommand final : public Command {
  public:
    explicit PurgeItemsCommand(TableItems items) : items_(std::move(items)) {}
    [[nodiscard]] std::string_view name() const override { return "PurgeTables"; }
    [[nodiscard]] bool isDestructive() const override { return true; }
    [[nodiscard]] Status validate(const CommandContext& context) const override
    {
        const katana::entity::Model& model = context.model;
        if (items_.empty()) {
            // Never an empty undo step (the shape of audit QT-01).
            return makeError(ErrorCode::InvalidArgument, "there is nothing to purge");
        }
        if (auto status = allPresent<katana::entity::Style>(model, items_.styles); !status) {
            return status;
        }
        if (auto status = allPresent<katana::entity::Linetype>(model, items_.linetypes); !status) {
            return status;
        }
        if (auto status = allPresent<katana::entity::HatchPattern>(model, items_.hatchPatterns);
            !status) {
            return status;
        }
        const katana::entity::TableUsage usage = katana::entity::tableUsage(model);
        for (const std::string& name : items_.styles) {
            if (auto status = refuseIfUsed(katana::entity::TableUsage::of(usage.styles, name),
                                           "style");
                !status) {
                return withName(status, name);
            }
        }
        // An entity reaches a linetype or hatch only through a layer or a
        // style, and the styles going have no entities (checked above), so
        // what holds one is a layer, or a style that is staying.
        const auto heldByWhatStays = [&](const katana::entity::Users& users) {
            if (!users.layers.empty()) {
                return true;
            }
            return std::ranges::any_of(users.styles, [&](const std::string& style) {
                return std::ranges::find(items_.styles, style) == items_.styles.end();
            });
        };
        for (const std::string& name : items_.linetypes) {
            const auto& users = katana::entity::TableUsage::of(usage.linetypes, name);
            if (heldByWhatStays(users)) {
                return withName(refuseIfUsed(users, "linetype"), name);
            }
        }
        for (const std::string& name : items_.hatchPatterns) {
            const auto& users = katana::entity::TableUsage::of(usage.hatchPatterns, name);
            if (heldByWhatStays(users)) {
                return withName(refuseIfUsed(users, "hatch pattern"), name);
            }
        }
        return {};
    }
    [[nodiscard]] Status execute(CommandContext& context) override
    {
        katana::entity::Model& model = context.model;
        removed_ = {};
        Status status = removeAll(model.styles, items_.styles, removed_.styles);
        if (status) {
            status = removeAll(model.linetypes, items_.linetypes, removed_.linetypes);
        }
        if (status) {
            status = removeAll(model.hatchPatterns, items_.hatchPatterns, removed_.hatchPatterns);
        }
        if (!status) {
            (void)undo(context);
            return status;
        }
        return {};
    }
    [[nodiscard]] Status undo(CommandContext& context) override
    {
        katana::entity::Model& model = context.model;
        for (const auto& pattern : removed_.hatchPatterns) {
            if (auto status = model.hatchPatterns.add(pattern); !status) {
                return status;
            }
        }
        for (const auto& linetype : removed_.linetypes) {
            if (auto status = model.linetypes.add(linetype); !status) {
                return status;
            }
        }
        for (const auto& style : removed_.styles) {
            if (auto status = model.styles.add(style); !status) {
                return status;
            }
        }
        removed_ = {};
        return {};
    }
    [[nodiscard]] Status redo(CommandContext& context) override { return execute(context); }

  private:
    struct Removed {
        std::vector<katana::entity::Style> styles;
        std::vector<katana::entity::Linetype> linetypes;
        std::vector<katana::entity::HatchPattern> hatchPatterns;
    };

    template <typename T>
    static Status allPresent(const katana::entity::Model& model,
                             const std::vector<std::string>& names)
    {
        for (const std::string& name : names) {
            if (auto status = TablePolicy<T>::deletable(name); !status) {
                return status;
            }
            if (!TablePolicy<T>::table(model).contains(name)) {
                return makeError(ErrorCode::NotFound,
                                 std::string(TablePolicy<T>::kNoun) + " does not exist", name);
            }
        }
        return {};
    }
    static Status withName(const Status& status, const std::string& name)
    {
        if (status) {
            return status;
        }
        return makeError(status.error().code, status.error().message,
                         name + ": " + status.error().context);
    }
    template <typename Table, typename T>
    static Status removeAll(Table& table, const std::vector<std::string>& names,
                            std::vector<T>& removed)
    {
        for (const std::string& name : names) {
            auto item = table.remove(name);
            if (!item) {
                return item.error();
            }
            removed.push_back(std::move(*item));
        }
        return {};
    }

    TableItems items_;
    Removed removed_{};
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
CommandPtr mergeLinetype(std::string from, std::string into)
{
    return std::make_unique<MergeItemCommand<katana::entity::Linetype>>(std::move(from),
                                                                       std::move(into));
}
CommandPtr duplicateLinetype(std::string from, std::string to)
{
    return std::make_unique<DuplicateItemCommand<katana::entity::Linetype>>(std::move(from),
                                                                           std::move(to));
}
CommandPtr updateLinetypeIfChanged(const katana::entity::Model& model,
                                   katana::entity::Linetype linetype)
{
    return updateIfChanged(model, std::move(linetype));
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
CommandPtr mergeStyle(std::string from, std::string into)
{
    return std::make_unique<MergeItemCommand<katana::entity::Style>>(std::move(from),
                                                                    std::move(into));
}
CommandPtr duplicateStyle(std::string from, std::string to)
{
    return std::make_unique<DuplicateItemCommand<katana::entity::Style>>(std::move(from),
                                                                        std::move(to));
}
CommandPtr updateStyleIfChanged(const katana::entity::Model& model, katana::entity::Style style)
{
    return updateIfChanged(model, std::move(style));
}

CommandPtr purgeTableItems(TableItems items)
{
    return std::make_unique<PurgeItemsCommand>(std::move(items));
}

} // namespace katana::commands
