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
#include <cstdint>
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
#include "katana/entity/style_library.hpp"
#include "katana/entity/survey_map.hpp"
#include "katana/geometry/spatial_index.hpp"
#include "katana/storage/project_store.hpp"

namespace katana::cad {

class Document {
  public:
    using Listener = std::function<void()>;

    // Owns a registration made by addListener and ends it when destroyed.
    //
    // A listener captures the object it notifies, so it must not outlive it:
    // a viewport that registered `[this] { update(); }` and was then replaced
    // by a layout change left a dangling call in the document, and the next
    // command after a 12da import crashed on it. The handle holds the
    // registry weakly, so a widget that outlives the Document - Qt deletes
    // child widgets in ~QWidget, after the window's own members have gone -
    // finds nothing to remove and does nothing.
    class ListenerHandle {
      public:
        ListenerHandle() = default;
        ~ListenerHandle() { reset(); }
        ListenerHandle(ListenerHandle&& other) noexcept
            : registry_(std::move(other.registry_)), id_(other.id_)
        {
            other.id_ = 0;
        }
        ListenerHandle& operator=(ListenerHandle&& other) noexcept
        {
            if (this != &other) {
                reset();
                registry_ = std::move(other.registry_);
                id_ = other.id_;
                other.id_ = 0;
            }
            return *this;
        }
        ListenerHandle(const ListenerHandle&) = delete;
        ListenerHandle& operator=(const ListenerHandle&) = delete;

        void reset();
        [[nodiscard]] bool active() const { return id_ != 0 && !registry_.expired(); }

      private:
        friend class Document;
        struct Registry;
        ListenerHandle(std::weak_ptr<Registry> registry, std::uint64_t id)
            : registry_(std::move(registry)), id_(id)
        {
        }
        std::weak_ptr<Registry> registry_;
        std::uint64_t id_ = 0;
    };

    explicit Document(katana::core::Logger* logger = nullptr);
    ~Document();

    Document(const Document&) = delete;
    Document& operator=(const Document&) = delete;

    [[nodiscard]] const katana::entity::Model& model() const { return model_; }

    // ---- the survey customisation (PLAN.MD 20.3) -----------------------------
    //
    // The 12d linestyle and symbol definitions this drawing is drawn with, and
    // the mapfile that says what a survey code becomes. They are NOT part of
    // the model and are not saved inside the project: a customisation is a
    // site-wide thing a project NAMES rather than copies, which is 12d's own
    // arrangement and keeps 35,000 strokes out of every project file. See
    // docs/survey_coding.md.
    //
    // They are not undoable either, and deliberately: loading a library is not
    // an edit to the drawing, it is a change to what the drawing is looked at
    // through. Undoing a line should not silently unload a library.
    [[nodiscard]] const katana::entity::StyleLibrary& styleLibrary() const { return library_; }
    [[nodiscard]] const katana::entity::SurveyMap& surveyMap() const { return surveyMap_; }
    void setStyleLibrary(katana::entity::StyleLibrary library);
    void setSurveyMap(katana::entity::SurveyMap map);
    // Counters bumped by every setStyleLibrary / setSurveyMap. A cache of
    // flattened definitions, thumbnails or code lookups keys on these, never
    // on a LineStyle* or SurveyRule* (both dangle when the whole library or
    // map is replaced) and never on "a listener fired" (which a selection
    // click also does).
    [[nodiscard]] std::uint64_t libraryGeneration() const { return libraryGeneration_; }
    [[nodiscard]] std::uint64_t surveyMapGeneration() const { return surveyMapGeneration_; }
    // The definition a style's `symbol` or `linetype` names, or nullptr. One
    // place to ask, so that "which library does this name come from" is not a
    // question every caller answers for itself.
    [[nodiscard]] const katana::entity::LineStyle* definitionFor(std::string_view name) const;

    // Broad-phase index over the entities, kept in step with the model
    // (PLAN.MD Phase 18). Maintained incrementally from the per-entity changes
    // every command reports, and rebuilt whole whenever the model is replaced -
    // a rebuild is what chooses the cell size from the data.
    //
    // Pass it to snap(), pickEntity() and pickInBox(). They give the same
    // answer without it, only slower, so a caller that has no Document loses
    // nothing but speed.
    [[nodiscard]] const katana::geometry::SpatialIndex& spatialIndex() const { return index_; }
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
    // The registration lasts as long as the handle does - keep it as a member
    // of the object the listener captures, declared so that it dies first.
    [[nodiscard]] ListenerHandle addListener(Listener listener);

  private:
    void rebuildStack();
    // Whole index from the current model; picks the cell size from the data.
    void rebuildSpatialIndex();
    // Incremental maintenance from the changes one command reported.
    void applyToSpatialIndex(const std::vector<katana::entity::ChangeEvent>& changes);
    void pruneSelection();
    void notify();

    katana::core::Logger* logger_ = nullptr;
    katana::entity::Model model_;
    katana::entity::StyleLibrary library_;
    katana::entity::SurveyMap surveyMap_;
    std::uint64_t libraryGeneration_ = 0;
    std::uint64_t surveyMapGeneration_ = 0;
    katana::geometry::SpatialIndex index_;
    std::unique_ptr<katana::commands::CommandStack> stack_;
    std::unique_ptr<katana::storage::ProjectStore> store_;
    katana::storage::ProjectMetadata metadata_;
    SelectionSet selection_;
    std::string currentLayer_{katana::entity::kDefaultLayerName};
    std::shared_ptr<ListenerHandle::Registry> listeners_;
    bool metadataModified_ = false;
};

} // namespace katana::cad
