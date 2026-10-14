// Document's road-object members (include/katana/cad/document.hpp), kept
// here with the rest of the road-object code as the sheet set's are kept
// with the sheets (plotting/sheet_store.cpp): the document only carries the
// JSON; everything that knows what a road object is lives in road/.

#include <functional>
#include <memory>
#include <set>
#include <string>
#include <utility>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/cad/road/road_objects.hpp"
#include "katana/commands/entity_commands.hpp"

namespace katana::cad {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Status;

namespace {

// The metadata key the objects are stored under. The storage layer keeps a
// key it does not read and writes it back unchanged (ProjectMetadata::
// unknownKeys), so road objects live in a project with no schema migration,
// as the sheets do.
constexpr std::string_view kRoadObjectsKey = "road_objects";

// One change of the set: the layers it makes first, then the document's
// state before and after, applied by the document. ONE step, so one undo
// takes away the objects and the layer made for them together - a layer left
// behind by an undone ADD would be a layer nobody asked for.
class RoadObjectsCommand final : public katana::commands::Command {
  public:
    // Called with true to apply the state after the change, false before.
    using Apply = std::function<void(bool after)>;

    RoadObjectsCommand(std::string name, std::vector<katana::commands::CommandPtr> layers,
                       Apply apply)
        : name_(std::move(name)), layers_(std::move(layers)), apply_(std::move(apply))
    {
    }

    [[nodiscard]] std::string_view name() const override { return name_; }
    [[nodiscard]] Status validate(const katana::commands::CommandContext& context) const override
    {
        for (const auto& layer : layers_) {
            if (auto status = layer->validate(context); !status) {
                return status;
            }
        }
        return {};
    }
    [[nodiscard]] Status execute(katana::commands::CommandContext& context) override
    {
        for (std::size_t i = 0; i < layers_.size(); ++i) {
            if (auto status = layers_[i]->execute(context); !status) {
                // Nothing half done: the layers made so far go again.
                for (std::size_t j = i; j-- > 0;) {
                    (void)layers_[j]->undo(context);
                }
                return status;
            }
        }
        apply_(true);
        return {};
    }
    [[nodiscard]] Status undo(katana::commands::CommandContext& context) override
    {
        apply_(false);
        for (std::size_t j = layers_.size(); j-- > 0;) {
            if (auto status = layers_[j]->undo(context); !status) {
                return status;
            }
        }
        return {};
    }
    [[nodiscard]] Status redo(katana::commands::CommandContext& context) override
    {
        for (const auto& layer : layers_) {
            if (auto status = layer->redo(context); !status) {
                return status;
            }
        }
        apply_(true);
        return {};
    }

  private:
    std::string name_;
    std::vector<katana::commands::CommandPtr> layers_;
    Apply apply_;
};

} // namespace

struct Document::RoadObjectCache {
    // What the set was parsed from: whether the key was there, and its text.
    // A state with no key is one too, so undoing the first object ever
    // placed leaves the metadata exactly as it was.
    bool present = false;
    std::string text;
    road::RoadObjectSet set;
    Status status;
};

const road::RoadObjectSet& Document::roadObjects() const
{
    const auto found = metadata_.unknownKeys.find(std::string(kRoadObjectsKey));
    const bool present = found != metadata_.unknownKeys.end();
    // Compared by content, not by a counter, because setMetadata, newDocument
    // and open replace the whole metadata - this key included - and must be
    // seen.
    if (roadObjectCache_ && roadObjectCache_->present == present &&
        (!present || roadObjectCache_->text == found->second)) {
        return roadObjectCache_->set;
    }
    auto cache = std::make_shared<RoadObjectCache>();
    cache->present = present;
    if (present) {
        cache->text = found->second;
        auto parsed = road::roadObjectsFromJson(cache->text);
        if (parsed) {
            cache->set = std::move(*parsed);
        } else {
            cache->status = parsed.error();
        }
    }
    roadObjectCache_ = std::move(cache);
    return roadObjectCache_->set;
}

Status Document::roadObjectsStatus() const
{
    (void)roadObjects();
    return roadObjectCache_->status;
}

Status Document::setRoadObjects(const road::RoadObjectSet& objects, std::string stepName)
{
    // Brings the cache up to date with the metadata: it is the step's "before".
    if (const Status status = roadObjectsStatus();
        !status && status.error().code == ErrorCode::Unsupported) {
        return makeError(ErrorCode::CommandRejected,
                         "the road objects were written by a newer version of Katana; they are "
                         "kept as they are",
                         status.error().describe());
    }
    auto after = std::make_shared<RoadObjectCache>();
    // No objects and no number taken is stored as no key at all; a set whose
    // objects were all deleted keeps its next number, so an id is never
    // given twice.
    if (!(objects == road::RoadObjectSet{})) {
        auto json = road::roadObjectsToJson(objects);
        if (!json) {
            return json.error();
        }
        after->present = true;
        after->text = std::move(*json);
        after->set = objects;
    }
    std::shared_ptr<const RoadObjectCache> before = roadObjectCache_;
    if (before->present == after->present && before->text == after->text) {
        return {}; // nothing changed, and nothing to undo
    }
    // The layers the objects are shown on, made where the drawing lacks one,
    // in the colour of the first object on it.
    std::vector<katana::commands::CommandPtr> layers;
    std::set<std::string> made;
    for (const road::RoadObject& object : objects.objects) {
        std::string name = road::layerOf(object);
        if (model_.layers.contains(name) || !made.insert(name).second) {
            continue;
        }
        katana::entity::Layer layer;
        layer.name = std::move(name);
        layer.color = road::colourOf(object);
        if (auto status = model_.layers.checkAdd(layer); !status) {
            return makeError(ErrorCode::InvalidArgument,
                             "road object " + object.id + " names a layer that cannot be made",
                             status.error().describe());
        }
        layers.push_back(katana::commands::createLayer(std::move(layer)));
    }
    auto apply = [this, before = std::move(before),
                  after = std::shared_ptr<const RoadObjectCache>(std::move(after))](bool toAfter) {
        const std::shared_ptr<const RoadObjectCache>& state = toAfter ? after : before;
        if (state->present) {
            metadata_.unknownKeys.insert_or_assign(std::string(kRoadObjectsKey), state->text);
        } else {
            metadata_.unknownKeys.erase(std::string(kRoadObjectsKey));
        }
        roadObjectCache_ = state;
        // The stack's own notification says History and the tables; what
        // the road model is made from is said here, on the step, its undo
        // and its redo alike, so a view rebuilds the model for exactly these.
        notify(DocumentChange::RoadModel);
    };
    return execute(std::make_unique<RoadObjectsCommand>(std::move(stepName), std::move(layers),
                                                        std::move(apply)));
}

} // namespace katana::cad
