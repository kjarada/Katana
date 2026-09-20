#pragma once

// An open drawing: the domain model plus everything a session needs around it
// (command history, selection, current layer, the project it is stored in).
//
// The Document is UI independent. Qt widgets, the CLI and (later) the
// application API all drive the same object, and all mutations go through
// execute() so they are validated, undoable and observable.
//
// Threading: single-threaded, owned by the application's main thread.

#include <filesystem>
#include <functional>
#include <memory>
#include <optional>
#include <string>
#include <vector>

#include "katana/cad/selection.hpp"
#include "katana/commands/command_stack.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/core/error.hpp"
#include "katana/core/log.hpp"
#include "katana/entity/model.hpp"
#include "katana/storage/project_store.hpp"

namespace katana::cad {

class Document {
  public:
    using Listener = std::function<void()>;

    explicit Document(katana::core::Logger* logger = nullptr);
    ~Document();

    Document(const Document&) = delete;
    Document& operator=(const Document&) = delete;

    [[nodiscard]] const katana::entity::Model& model() const { return model_; }
    [[nodiscard]] const katana::commands::CommandStack& history() const { return *stack_; }

    // ---- editing -------------------------------------------------------------
    [[nodiscard]] katana::core::Status execute(katana::commands::CommandPtr command);
    [[nodiscard]] katana::core::Status undo();
    [[nodiscard]] katana::core::Status redo();
    // Entities created by the most recent execute() / redo().
    [[nodiscard]] std::vector<katana::entity::EntityId> lastCreatedEntities() const;

    // ---- session state -------------------------------------------------------
    [[nodiscard]] SelectionSet& selection() { return selection_; }
    [[nodiscard]] const SelectionSet& selection() const { return selection_; }
    // Call after changing the selection so views refresh.
    void notifySelectionChanged();

    [[nodiscard]] const std::string& currentLayer() const { return currentLayer_; }
    [[nodiscard]] katana::core::Status setCurrentLayer(const std::string& name);
    // Attributes for newly drawn entities: current layer, ByLayer colour and style.
    [[nodiscard]] katana::commands::EntityAttributes currentAttributes() const;

    // ---- persistence ---------------------------------------------------------
    // Discards the drawing and starts an empty, unsaved one.
    void newDocument();
    [[nodiscard]] katana::core::Status open(const std::filesystem::path& projectDirectory);
    // Saves to the project this document was opened from / last saved to.
    [[nodiscard]] katana::core::Status save();
    // Creates a new project directory (must not exist as a project yet).
    [[nodiscard]] katana::core::Status saveAs(const std::filesystem::path& projectDirectory);

    [[nodiscard]] bool isModified() const;
    [[nodiscard]] bool hasProject() const { return store_ != nullptr; }
    [[nodiscard]] std::optional<std::filesystem::path> projectDirectory() const;
    [[nodiscard]] const katana::storage::ProjectMetadata& metadata() const { return metadata_; }
    void setMetadata(katana::storage::ProjectMetadata metadata);

    // Called after anything observable changed: model, selection, current
    // layer, project. Listeners must not mutate the document re-entrantly.
    void addListener(Listener listener);

  private:
    void rebuildStack();
    void pruneSelection();
    void notify();

    katana::core::Logger* logger_ = nullptr;
    katana::entity::Model model_;
    std::unique_ptr<katana::commands::CommandStack> stack_;
    std::unique_ptr<katana::storage::ProjectStore> store_;
    katana::storage::ProjectMetadata metadata_;
    SelectionSet selection_;
    std::string currentLayer_{katana::entity::kDefaultLayerName};
    std::vector<Listener> listeners_;
    bool metadataModified_ = false;
};

} // namespace katana::cad
