// Document's road-model members (include/katana/cad/document.hpp), kept here
// with the rest of the road model as the sheet set's are kept with the sheets
// (plotting/sheet_store.cpp): the document carries only the JSON; everything
// that knows what a template or a corridor is lives in road/.

#include <array>
#include <functional>
#include <memory>
#include <set>
#include <string>
#include <utility>

#include "katana/cad/document.hpp"
#include "katana/cad/road/corridor_model.hpp"
#include "katana/cad/road/road_template.hpp"
#include "katana/core/text.hpp"

namespace katana::cad {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;

namespace {

// The metadata keys. The storage layer keeps a key it does not read and
// writes it back unchanged (ProjectMetadata::unknownKeys), which is what lets
// the road model live in a project without a schema migration. The third is
// the placed objects' (cad/road/road_objects.hpp), whose change is the road
// model's too.
constexpr std::string_view kTemplatesKey = "road_templates";
constexpr std::string_view kCorridorsKey = "corridors";
constexpr std::string_view kObjectsKey = "road_objects";
constexpr std::array<std::string_view, 3> kRoadModelKeys{kTemplatesKey, kCorridorsKey, kObjectsKey};

// One change of a road-model key: the state before and after, applied by the
// document - the SheetSetCommand shape (plotting/sheet_store.cpp).
class RoadModelCommand final : public katana::commands::Command {
  public:
    using Apply = std::function<void(bool after)>;

    RoadModelCommand(std::string name, Apply apply)
        : name_(std::move(name)), apply_(std::move(apply))
    {
    }

    [[nodiscard]] std::string_view name() const override { return name_; }
    [[nodiscard]] Status validate(const katana::commands::CommandContext&) const override
    {
        return {};
    }
    [[nodiscard]] Status execute(katana::commands::CommandContext&) override
    {
        apply_(true);
        return {};
    }
    [[nodiscard]] Status undo(katana::commands::CommandContext&) override
    {
        apply_(false);
        return {};
    }
    [[nodiscard]] Status redo(katana::commands::CommandContext&) override
    {
        apply_(true);
        return {};
    }

  private:
    std::string name_;
    Apply apply_;
};

// What a key held when it was parsed: whether it was there, its text, the
// set read from it, and why it could not be read. A state with no key is one
// too, so undoing the first template ever made leaves the metadata exactly as
// it was.
template <typename Set> struct ParsedKey {
    bool present = false;
    std::string text;
    Set set;
    Status status;
};

// The cache brought up to date with the metadata, compared by content - not
// by a revision - because setMetadata replaces the whole metadata, keys
// included, and must be seen.
template <typename Cache, typename Parse>
const Cache& upToDate(std::shared_ptr<const Cache>& cache,
                      const katana::storage::ProjectMetadata& metadata, std::string_view key,
                      Parse parse)
{
    const auto found = metadata.unknownKeys.find(std::string(key));
    const bool present = found != metadata.unknownKeys.end();
    if (cache && cache->present == present && (!present || cache->text == found->second)) {
        return *cache;
    }
    auto made = std::make_shared<Cache>();
    made->present = present;
    if (present) {
        made->text = found->second;
        auto parsed = parse(made->text);
        if (parsed) {
            made->set = std::move(*parsed);
        } else {
            made->status = parsed.error();
        }
    }
    cache = std::move(made);
    return *cache;
}

// The change to `set` as a command, null when it changes nothing. `cache`
// must be up to date: it is the step's "before".
template <typename Cache, typename Set, typename Write>
Result<katana::commands::CommandPtr>
changeCommand(std::shared_ptr<const Cache>& cache, katana::storage::ProjectMetadata& metadata,
              std::string_view key, const Set& set, Write write, std::string stepName,
              std::string_view what)
{
    if (!cache->status && cache->status.error().code == ErrorCode::Unsupported) {
        return makeError(ErrorCode::CommandRejected,
                         "the " + std::string(what) +
                             " were written by a newer version of Katana; they are kept as they "
                             "are",
                         cache->status.error().describe());
    }
    auto after = std::make_shared<Cache>();
    // An empty set is stored as no key at all.
    if (!(set == Set{})) {
        auto json = write(set);
        if (!json) {
            return json.error();
        }
        after->present = true;
        after->text = std::move(*json);
        after->set = set;
    }
    std::shared_ptr<const Cache> before = cache;
    if (before->present == after->present && before->text == after->text) {
        return katana::commands::CommandPtr{};
    }
    auto apply = [&metadata, &cache, name = std::string(key), before = std::move(before),
                  after = std::shared_ptr<const Cache>(std::move(after))](bool toAfter) {
        const std::shared_ptr<const Cache>& state = toAfter ? after : before;
        if (state->present) {
            metadata.unknownKeys.insert_or_assign(name, state->text);
        } else {
            metadata.unknownKeys.erase(name);
        }
        cache = state;
    };
    return katana::commands::CommandPtr(
        std::make_unique<RoadModelCommand>(std::move(stepName), std::move(apply)));
}

Status checkTemplates(const road::RoadTemplateSet& set)
{
    std::set<std::string> names;
    for (const road::RoadTemplate& each : set.templates) {
        if (auto status = road::checkTemplate(each); !status) {
            return status;
        }
        if (!names.insert(katana::core::lowered(each.name)).second) {
            return makeError(ErrorCode::InvalidArgument, "two road templates have one name",
                             each.name);
        }
    }
    return {};
}

Status checkCorridors(const road::CorridorSet& set)
{
    std::set<std::string> names;
    for (const road::Corridor& each : set.corridors) {
        if (auto status = road::checkCorridor(each); !status) {
            return status;
        }
        if (!names.insert(katana::core::lowered(each.name)).second) {
            return makeError(ErrorCode::InvalidArgument, "two corridors have one name", each.name);
        }
    }
    return {};
}

} // namespace

struct Document::RoadTemplateCache : ParsedKey<road::RoadTemplateSet> {};
struct Document::CorridorCache : ParsedKey<road::CorridorSet> {};

const road::RoadTemplateSet& Document::roadTemplates() const
{
    return upToDate(roadTemplateCache_, metadata_, kTemplatesKey, road::roadTemplatesFromJson).set;
}

Status Document::roadTemplatesStatus() const
{
    return upToDate(roadTemplateCache_, metadata_, kTemplatesKey, road::roadTemplatesFromJson)
        .status;
}

Result<katana::commands::CommandPtr>
Document::roadTemplatesCommand(const road::RoadTemplateSet& templates, std::string stepName)
{
    if (auto status = checkTemplates(templates); !status) {
        return status.error();
    }
    (void)roadTemplates();
    return changeCommand(roadTemplateCache_, metadata_, kTemplatesKey, templates,
                         road::roadTemplatesToJson, std::move(stepName), "road templates");
}

Status Document::setRoadTemplates(const road::RoadTemplateSet& templates, std::string stepName)
{
    auto command = roadTemplatesCommand(templates, std::move(stepName));
    if (!command) {
        return command.error();
    }
    if (*command == nullptr) {
        return {}; // nothing changed, and nothing to undo
    }
    return execute(std::move(*command));
}

const road::CorridorSet& Document::corridors() const
{
    return upToDate(corridorCache_, metadata_, kCorridorsKey, road::corridorsFromJson).set;
}

Status Document::corridorsStatus() const
{
    return upToDate(corridorCache_, metadata_, kCorridorsKey, road::corridorsFromJson).status;
}

Result<katana::commands::CommandPtr> Document::corridorsCommand(const road::CorridorSet& corridors,
                                                                std::string stepName)
{
    if (auto status = checkCorridors(corridors); !status) {
        return status.error();
    }
    (void)this->corridors();
    return changeCommand(corridorCache_, metadata_, kCorridorsKey, corridors, road::corridorsToJson,
                         std::move(stepName), "corridors");
}

Status Document::setCorridors(const road::CorridorSet& corridors, std::string stepName)
{
    auto command = corridorsCommand(corridors, std::move(stepName));
    if (!command) {
        return command.error();
    }
    if (*command == nullptr) {
        return {};
    }
    return execute(std::move(*command));
}

std::uint32_t Document::roadModelChanged()
{
    roadModelSeen_.resize(kRoadModelKeys.size());
    bool changed = false;
    for (std::size_t i = 0; i < kRoadModelKeys.size(); ++i) {
        const auto found = metadata_.unknownKeys.find(std::string(kRoadModelKeys[i]));
        std::optional<std::string>& seen = roadModelSeen_[i];
        if (found == metadata_.unknownKeys.end()) {
            if (seen) {
                seen.reset();
                changed = true;
            }
        } else if (!seen || *seen != found->second) {
            seen = found->second;
            changed = true;
        }
    }
    return changed ? std::uint32_t{DocumentChange::RoadModel} : 0u;
}

} // namespace katana::cad
