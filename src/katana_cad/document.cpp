#include "katana/cad/document.hpp"

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
    stack_->addListener([this](const katana::commands::CommandEvent&) {
        // Undo can delete selected entities and the current layer.
        pruneSelection();
        if (!model_.layers.contains(currentLayer_)) {
            currentLayer_ = std::string(katana::entity::kDefaultLayerName);
        }
        notify();
    });
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

katana::commands::EntityAttributes Document::currentAttributes() const
{
    katana::commands::EntityAttributes attributes;
    attributes.layer = currentLayer_;
    return attributes;
}

void Document::newDocument()
{
    store_.reset();
    model_.reset();
    rebuildStack(); // fresh history; also re-installs the entity observer
    metadata_ = {};
    metadataModified_ = false;
    selection_.clear();
    currentLayer_ = std::string(katana::entity::kDefaultLayerName);
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
    // applyToModel validates before it touches the model, so a bad project
    // leaves the current drawing as it was.
    if (auto status = katana::storage::applyToModel(*contents, model_); !status) {
        return status;
    }
    store_ = std::make_unique<katana::storage::ProjectStore>(std::move(*store));
    metadata_ = std::move(contents->metadata);
    metadataModified_ = false;
    rebuildStack();
    selection_.clear();
    currentLayer_ = std::string(katana::entity::kDefaultLayerName);
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
    if (auto status = store_->save(katana::storage::captureModel(model_, metadata_)); !status) {
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
    if (auto status = store->save(katana::storage::captureModel(model_, metadata_)); !status) {
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

void Document::addListener(Listener listener)
{
    listeners_.push_back(std::move(listener));
}

void Document::pruneSelection()
{
    selection_.prune(model_.entities);
}

void Document::notify()
{
    for (const Listener& listener : listeners_) {
        listener();
    }
}

} // namespace katana::cad
