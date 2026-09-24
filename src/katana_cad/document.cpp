#include "katana/cad/document.hpp"

#include "katana/cad/spatial_query.hpp"

#include <algorithm>
#include <utility>

namespace katana::cad {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Status;

Document::Document(katana::core::Logger* logger) : logger_(logger)
{
    rebuildStack();
}

Document::~Document() = default;

void Document::rebuildStack()
{
    // Destroy the old stack first: its destructor clears the model's entity
    // observer, which must not happen after the new stack has installed its own.
    stack_.reset();
    stack_ = std::make_unique<katana::commands::CommandStack>(model_, logger_);
    stack_->addListener([this](const katana::commands::CommandEvent& event) {
        // The index is maintained from the per-entity changes the command
        // reported rather than rebuilt: a single click on a 250 000-entity
        // drawing must not pay for a whole rebuild.
        applyToSpatialIndex(event.changes);
        // Undo can delete selected entities and the current layer.
        pruneSelection();
        if (!model_.layers.contains(currentLayer_)) {
            currentLayer_ = std::string(katana::entity::kDefaultLayerName);
        }
        // And the current style: a delete, merge or rename of it - or the
        // undo of its creation - must not leave new work naming nothing.
        if (!currentStyle_.empty() && !model_.styles.contains(currentStyle_)) {
            currentStyle_.clear();
        }
        // The stack publishes only a step that succeeded, so a refused
        // command moves nothing.
        ++modelRevision_;
        notify();
    });
}

void Document::rebuildSpatialIndex()
{
    std::vector<katana::geometry::SpatialEntry> entries;
    entries.reserve(model_.entities.size());
    model_.entities.forEach([&](const katana::entity::Entity& entity) {
        entries.push_back(
            katana::geometry::SpatialEntry{static_cast<katana::geometry::SpatialId>(entity.id),
                                           detail::queryExtents(model_, entity)});
    });
    index_.rebuild(entries);
}

void Document::applyToSpatialIndex(const std::vector<katana::entity::ChangeEvent>& changes)
{
    for (const katana::entity::ChangeEvent& change : changes) {
        switch (change.kind) {
        case katana::entity::ChangeKind::Cleared:
            // Everything went at once; a full rebuild also re-chooses the cell
            // size, which is right because the data is about to be different.
            rebuildSpatialIndex();
            return;
        case katana::entity::ChangeKind::EntityRemoved:
            index_.remove(static_cast<katana::geometry::SpatialId>(change.id));
            break;
        case katana::entity::ChangeKind::EntityAdded:
        case katana::entity::ChangeKind::EntityModified: {
            const katana::entity::Entity* entity = model_.entities.find(change.id);
            if (entity == nullptr) {
                // Added then removed within one command. insert() would have
                // nothing to index; remove() keeps the index from holding an
                // id the model no longer has.
                index_.remove(static_cast<katana::geometry::SpatialId>(change.id));
                break;
            }
            // insert() replaces an existing entry, so Modified needs no
            // separate remove.
            index_.insert(static_cast<katana::geometry::SpatialId>(entity->id),
                          detail::queryExtents(model_, *entity));
            break;
        }
        }
    }
}

Status Document::execute(katana::commands::CommandPtr command)
{
    return stack_->execute(std::move(command));
}

Status Document::undo()
{
    return stack_->undo();
}

Status Document::redo()
{
    return stack_->redo();
}

std::vector<katana::entity::EntityId> Document::lastCreatedEntities() const
{
    return stack_->lastCreatedEntities();
}

void Document::notifySelectionChanged()
{
    notify();
}

Status Document::setCurrentLayer(const std::string& name)
{
    const katana::entity::Layer* layer = model_.layers.find(name);
    if (layer == nullptr) {
        return makeError(ErrorCode::NotFound, "layer does not exist", name);
    }
    if (layer->locked) {
        return makeError(ErrorCode::CommandRejected, "a locked layer cannot be the current layer",
                         name);
    }
    currentLayer_ = name;
    notify();
    return {};
}

Status Document::setCurrentStyle(const std::string& name)
{
    if (!name.empty() && !model_.styles.contains(name)) {
        return makeError(ErrorCode::NotFound, "style does not exist", name);
    }
    if (name != currentStyle_) {
        currentStyle_ = name;
        notify();
    }
    return {};
}

katana::commands::EntityAttributes Document::currentAttributes() const
{
    katana::commands::EntityAttributes attributes;
    attributes.layer = currentLayer_;
    attributes.style = currentStyle_;
    return attributes;
}

void Document::newDocument()
{
    store_.reset();
    model_.reset();
    rebuildStack(); // fresh history; also re-installs the entity observer
    rebuildSpatialIndex();
    metadata_ = {};
    metadataModified_ = false;
    surveyJobs_.clear();
    ++surveyJobsGeneration_;
    selection_.clear();
    currentLayer_ = std::string(katana::entity::kDefaultLayerName);
    currentStyle_.clear();
    ++modelRevision_;
    notify();
}

Status Document::open(const std::filesystem::path& projectDirectory)
{
    auto store = katana::storage::ProjectStore::open(projectDirectory);
    if (!store) {
        return store.error();
    }
    auto contents = store->load();
    if (!contents) {
        return contents.error();
    }
    // Taken out BEFORE the contents are consumed below, and installed only
    // once the apply has succeeded - a failed open must leave this document
    // exactly as it was, metadata included.
    auto metadata = std::move(contents->metadata);
    // The jobs likewise: applyToModel does not read them, and they are the
    // largest thing in a project that has any (the raw field files).
    auto surveyJobs = std::move(contents->surveyJobs);
    // applyToModel validates before it touches the model, so a bad project
    // leaves the current drawing as it was. The contents are consumed: they are
    // a local that dies either way, and copying every entity into the model was
    // the single largest allocator of the whole open.
    if (auto status = katana::storage::applyToModel(std::move(*contents), model_); !status) {
        return status;
    }
    store_ = std::make_unique<katana::storage::ProjectStore>(std::move(*store));
    metadata_ = std::move(metadata);
    metadataModified_ = false;
    surveyJobs_ = std::move(surveyJobs);
    ++surveyJobsGeneration_;
    rebuildStack();
    // Whole rebuild rather than incremental: the model was replaced, and a
    // rebuild is what picks a cell size suited to the data just loaded.
    rebuildSpatialIndex();
    selection_.clear();
    currentLayer_ = std::string(katana::entity::kDefaultLayerName);
    currentStyle_.clear();
    // Even when the project opened is the drawing already shown: the
    // history, selection and current layer went, and a view cannot know the
    // tables are the same without comparing them.
    ++modelRevision_;
    if (logger_ != nullptr) {
        logger_->info("storage", "project opened",
                      {{"entities", std::to_string(model_.entities.size())}});
    }
    notify();
    return {};
}

Status Document::save()
{
    if (!store_) {
        return makeError(ErrorCode::InvalidState,
                         "the drawing has not been saved to a project yet; use saveAs()");
    }
    // Snapshot of the state about to be replaced, so a bad save is recoverable.
    if (auto backup = store_->backup(); !backup) {
        return backup.error();
    }
    if (auto status = saveContents(*store_); !status) {
        return status;
    }
    stack_->markSaved();
    metadataModified_ = false;
    if (logger_ != nullptr) {
        logger_->info("storage", "project saved",
                      {{"entities", std::to_string(model_.entities.size())}});
    }
    notify();
    return {};
}

Status Document::saveContents(katana::storage::ProjectStore& store)
{
    katana::storage::ProjectContents contents =
        katana::storage::captureModel(model_, metadata_);
    // LENT to the contents for the save and taken back after, rather than
    // copied: a job holds its field file whole, and copying tens of megabytes
    // on every save to hand it to a function that only reads it is waste.
    // The guard hands the jobs back however the save ends.
    struct GiveBack {
        std::vector<katana::storage::SurveyJob>& owner;
        std::vector<katana::storage::SurveyJob>& lent;
        ~GiveBack() { owner = std::move(lent); }
    } giveBack{surveyJobs_, contents.surveyJobs};
    contents.surveyJobs = std::move(surveyJobs_);
    return store.save(contents);
}

Status Document::saveAs(const std::filesystem::path& projectDirectory)
{
    if (metadata_.name.empty() || metadata_.name == "Untitled") {
        metadata_.name = projectDirectory.stem().string();
    }
    auto store = katana::storage::ProjectStore::create(projectDirectory, metadata_);
    if (!store) {
        return store.error();
    }
    auto created = store->load(); // picks up the creation timestamp
    if (created) {
        metadata_.createdUtc = created->metadata.createdUtc;
    }
    if (auto status = saveContents(*store); !status) {
        return status;
    }
    store_ = std::make_unique<katana::storage::ProjectStore>(std::move(*store));
    stack_->markSaved();
    metadataModified_ = false;
    notify();
    return {};
}

bool Document::isModified() const
{
    return stack_->isModified() || metadataModified_;
}

std::optional<std::filesystem::path> Document::projectDirectory() const
{
    if (!store_) {
        return std::nullopt;
    }
    return store_->directory();
}

void Document::setMetadata(katana::storage::ProjectMetadata metadata)
{
    if (metadata != metadata_) {
        metadata_ = std::move(metadata);
        metadataModified_ = true;
        notify();
    }
}

struct Document::ListenerHandle::Registry {
    struct Entry {
        std::uint64_t id;
        Listener listener;
    };
    std::vector<Entry> entries;
    std::uint64_t nextId = 1;
};

void Document::ListenerHandle::reset()
{
    if (const auto registry = registry_.lock(); registry && id_ != 0) {
        std::erase_if(registry->entries, [this](const auto& entry) { return entry.id == id_; });
    }
    id_ = 0;
    registry_.reset();
}

Document::ListenerHandle Document::addListener(Listener listener)
{
    if (!listeners_) {
        listeners_ = std::make_shared<ListenerHandle::Registry>();
    }
    const std::uint64_t id = listeners_->nextId++;
    listeners_->entries.push_back({id, std::move(listener)});
    return ListenerHandle(listeners_, id);
}

void Document::pruneSelection()
{
    selection_.prune(model_.entities);
}

void Document::notify()
{
    if (!listeners_) {
        return;
    }
    // By id, not by iterator: a listener may end a registration - its own,
    // or another's - while it runs, and the vector then moves under a loop.
    std::vector<std::uint64_t> ids;
    ids.reserve(listeners_->entries.size());
    for (const auto& entry : listeners_->entries) {
        ids.push_back(entry.id);
    }
    for (const std::uint64_t id : ids) {
        const auto found = std::find_if(listeners_->entries.begin(), listeners_->entries.end(),
                                        [id](const auto& entry) { return entry.id == id; });
        if (found != listeners_->entries.end()) {
            found->listener();
        }
    }
}

void Document::setStyleLibrary(katana::entity::StyleLibrary library)
{
    library_ = std::move(library);
    ++libraryGeneration_;
    notify();
}

void Document::setSurveyMap(katana::entity::SurveyMap map)
{
    surveyMap_ = std::move(map);
    ++surveyMapGeneration_;
    notify();
}

const katana::entity::LineStyle* Document::definitionFor(std::string_view name) const
{
    return name.empty() ? nullptr : library_.find(name);
}

} // namespace katana::cad
