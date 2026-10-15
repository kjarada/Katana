#pragma once

// An open drawing: the domain model plus everything a session needs around it
// (command history, selection, current layer, the project it is stored in).
//
// The Document is UI independent. Qt widgets, the CLI and (later) the
// application API all drive the same object, and all mutations go through
// execute() so they are validated, undoable and observable.
//
// Threading: single-threaded, owned by the application's main thread.

#include <cstdint>
#include <filesystem>
#include <functional>
#include <memory>
#include <optional>
#include <span>
#include <string>
#include <string_view>
#include <vector>

#include "katana/cad/customisation_state.hpp"
#include "katana/cad/drawing/drafting.hpp"
#include "katana/cad/selection.hpp"
#include "katana/cad/terrain/surface_style.hpp"
#include "katana/commands/command_stack.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/core/error.hpp"
#include "katana/core/log.hpp"
#include "katana/entity/model.hpp"
#include "katana/entity/style_library.hpp"
#include "katana/entity/survey_map.hpp"
#include "katana/geometry/spatial_index.hpp"
#include "katana/storage/project_store.hpp"
#include "katana/terrain/surface_store.hpp"

namespace katana::cad::plotting {
struct SheetSet;
}
// Road objects (docs/road_objects.md): see Document::roadObjects.
namespace katana::cad::road {
struct RoadObjectSet;
}

// ---- road model (docs/road_design.md) ----
namespace katana::cad::road {
struct RoadTemplateSet;
struct CorridorSet;
} // namespace katana::cad::road
namespace katana::cad::bim {
class BimCacheAccess;
}
// ---- end road model ----

namespace katana::cad {

// What one Document notification is about.
//
// A listener that only learns "something changed" must assume everything
// did: the 3D view rebuilt its whole scene - 217-768 ms on a real TIN
// archive - for a selection click, and every plan view repainted the drawing
// for it. So each notification says which parts moved, and a listener
// rebuilds only what is built from them.
//
// The parts are bits because one notification can carry several: a command
// that deletes selected entities changes the entities AND the selection, an
// import creates layers and styles AND entities. Ask has(mask) with every
// part the listener depends on rather than comparing `parts` for equality:
// a listener written for today's parts then keeps working when a later
// change carries one more alongside.
struct DocumentChange {
    enum Part : std::uint32_t {
        // Entities added, modified or removed; `entities` lists which.
        Entities = 1u << 0,
        // The selection set. Alone (selectionOnly()) it is a click: nothing
        // the drawing or a scene is built from has changed, only what is
        // highlighted.
        Selection = 1u << 1,
        // One bit per model table. A command sets the bit of every table it
        // changed, found by comparing the tables' revisions before and after.
        Layers = 1u << 2,
        Styles = 1u << 3,
        Linetypes = 1u << 4,
        DimensionStyles = 1u << 5,
        HatchPatterns = 1u << 6,
        Alignments = 1u << 7,
        PropertyDefinitions = 1u << 8,
        // The current layer or style new work is drawn in.
        CurrentAttributes = 1u << 9,
        // The undo history moved: every command executed, undone or redone.
        History = 1u << 10,
        // Saved, or saved somewhere else: the modified flag and the project
        // path may read differently now.
        Saved = 1u << 11,
        // The project metadata (setMetadata, or the name saveAs gives it).
        Metadata = 1u << 12,
        // The customisation the drawing is looked at through: the library
        // linestyles and symbols, and the survey code rules. Not part of the
        // model; see Document::styleLibrary.
        StyleLibrary = 1u << 13,
        SurveyMap = 1u << 14,
        // A new or opened drawing: the model, history, selection, current
        // attributes and metadata are all different. Set together with every
        // one of those bits, so a listener that tests has(Entities) rebuilds
        // on an open without having to know this bit exists. Not with the
        // customisation bits: the customisation is kept across a new or an
        // open.
        Replaced = 1u << 15,
        // The annotation tables: text styles, label styles and auto-label
        // rules (entity/annotation.hpp). One bit for the three, since
        // everything drawn from one is drawn from the others.
        AnnotationStyles = 1u << 16,
        // The drafting aids (Document::drafting): ortho, polar, snaps, locks.
        // Session state, not drawing: nothing is redrawn for it, but the
        // toggles that show it are re-read.
        Drafting = 1u << 17,
        // The customisation's session state (Document::customisationState):
        // its name, origin, sources, colours, linework codes, automation,
        // whether it is kept, and what the open project is missing. Not in
        // kDrawing - a name or a switch draws nothing; what does change the
        // drawing is reported with it (a whole customisation installed also
        // names StyleLibrary and SurveyMap, a new colour table StyleLibrary)
        // - and not in a Replaced drawing, which keeps its customisation.
        //
        // setStyleLibrary and setSurveyMap report their own part ALONE, as
        // they always have, although they also mark the origin edited: a
        // listener that shows the origin tests those two bits as well, or
        // compares customisationGeneration().
        Customisation = 1u << 18,
        // The drawing's surfaces (Document::surfaces): one added, removed,
        // edited, renamed, restyled, shown or hidden.
        Surfaces = 1u << 19,
        // The surface display styles (Document::surfaceStyles).
        SurfaceStyles = 1u << 20,
        // The road model's definitions - road templates, corridors, objects
        // placed along an alignment (cad/bim/bim_object.hpp) - whose 3D
        // objects are made again from them.
        RoadModel = 1u << 21,
    };

    // Every table bit.
    static constexpr std::uint32_t kTables = Layers | Styles | Linetypes | DimensionStyles |
                                             HatchPatterns | Alignments | PropertyDefinitions |
                                             AnnotationStyles;
    // What a drawing is DRAWN from - its entities, the tables that say how
    // they look, the customisation, its surfaces and road model - as against
    // what is only highlighted (the selection) or only bookkeeping (history,
    // saved, metadata).
    static constexpr std::uint32_t kDrawing =
        Entities | kTables | StyleLibrary | SurveyMap | Surfaces | SurfaceStyles | RoadModel;

    std::uint32_t parts = 0;
    // The entity-level detail of a command, undo or redo, in the order the
    // model reported it; empty for anything else. A Replaced drawing does not
    // list its entities: every one of them changed. Valid only for the
    // duration of the call - copy what must outlive it.
    std::span<const katana::entity::ChangeEvent> entities{};

    [[nodiscard]] constexpr bool has(std::uint32_t mask) const { return (parts & mask) != 0; }
    // Only the selection changed.
    [[nodiscard]] constexpr bool selectionOnly() const { return parts == Selection; }
    // Something the drawing is drawn from changed (kDrawing).
    [[nodiscard]] constexpr bool changesDrawing() const { return has(kDrawing); }
};

class Document {
  public:
    // A listener told only that something changed, kept for the callers that
    // redraw whatever happens. A listener that can skip work takes a
    // ChangeListener instead.
    using Listener = std::function<void()>;
    // A listener told what changed.
    using ChangeListener = std::function<void(const DocumentChange&)>;

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
    // The library linestyle and symbol definitions this drawing is drawn
    // with, and the survey code rules that say what a survey code becomes.
    // They are NOT part of the model and are not saved inside the project: a
    // customisation is a site-wide thing a project NAMES rather than copies,
    // the usual arrangement, and it keeps 35,000 strokes out of every project
    // file. See docs/survey_coding.md.
    //
    // They are not undoable either, and deliberately: loading a library is not
    // an edit to the drawing, it is a change to what the drawing is looked at
    // through. Undoing a line should not silently unload a library.
    [[nodiscard]] const katana::entity::StyleLibrary& styleLibrary() const { return library_; }
    [[nodiscard]] const katana::entity::SurveyMap& surveyMap() const { return surveyMap_; }
    // What an editor calls with the library or the map it changed. Either is
    // an EDIT of the session's customisation: its origin becomes Edited and
    // it is no longer kept (customisationState). A whole customisation is
    // installed with installCustomisation instead, which says where it came
    // from.
    void setStyleLibrary(katana::entity::StyleLibrary library);
    void setSurveyMap(katana::entity::SurveyMap map);
    // Counters bumped by every change of the library or of the survey map. A
    // cache of flattened definitions, thumbnails or code lookups keys on
    // these, never on a LineStyle* or SurveyRule* (both dangle when the whole
    // library or map is replaced) and never on "a listener fired" (which a
    // selection click also does). A change of the colour table bumps
    // libraryGeneration too: a sprite or a thumbnail of a definition bakes in
    // the colours its pens resolved to, and is dropped only on this.
    [[nodiscard]] std::uint64_t libraryGeneration() const { return libraryGeneration_; }
    [[nodiscard]] std::uint64_t surveyMapGeneration() const { return surveyMapGeneration_; }
    // The definition a style's `symbol` or `linetype` names, or nullptr. One
    // place to ask, so that "which library does this name come from" is not a
    // question every caller answers for itself.
    [[nodiscard]] const katana::entity::LineStyle* definitionFor(std::string_view name) const;

    // ---- the customisation's session state (customisation_state.hpp) ---------
    //
    // Everything else the session knows of its customisation: its name and
    // where it came from, the sources that went into it, its colours, linework
    // codes and automation switches, whether it is kept, and which names the
    // open project recorded that are not loaded. Session data, as the library
    // and the map are: not undoable, not in the project, kept across a new or
    // an opened drawing. A default Document has none of it (an empty state,
    // origin None), and nothing here installs a customisation unasked: a front
    // end does, with startCustomisation (customisation_host.hpp).
    [[nodiscard]] const CustomisationState& customisationState() const { return customisation_; }
    // Counts every change of the state above, the raw setters' included (they
    // mark the origin edited). What a panel showing the state compares.
    [[nodiscard]] std::uint64_t customisationGeneration() const
    {
        return customisationGeneration_;
    }
    // Installs a WHOLE customisation in the place of the session's: library,
    // map, colours, name, description, notice, basedOn, sources - a
    // customisation that lists no sources is its own one source - linework
    // codes and automation, with `origin` as where it came from and `kept` as
    // whether it is what the next start would give. Linework codes or
    // automation it does not say are the DEFAULTS, not the session's: the
    // session it leaves is the one a start with it would give, whatever was
    // there before - a reset to the built-in resets the control codes too.
    // (Keeping the session's where a load says nothing is the merge's rule,
    // and mergeCustomisation hands over a customisation that says both.) What
    // the open project is missing loses the names the new sources bring. One
    // notification: StyleLibrary | SurveyMap | Customisation.
    //
    // InvalidArgument, and nothing changes, for origin None and for whatever
    // customisationFaults finds (customisation_state.hpp) - a name, a source's
    // name, a definition's source, linework codes or a basedOn: the session
    // is what a KEEP writes and a project records, and neither could take
    // them.
    [[nodiscard]] katana::core::Status
    installCustomisation(katana::entity::Customisation customisation, CustomisationOrigin origin,
                         bool kept = false);
    // The session as one customisation - what an export or a KEEP writes, and
    // what a merge starts from: the library, the map and the state above.
    // Linework and automation are always said. installCustomisation of it
    // gives the session back.
    [[nodiscard]] katana::entity::Customisation customisation() const;
    // The three parts of the state an editor or a setting changes on their
    // own. Each is an edit of the session, as the raw setters above are:
    // origin Edited, no longer kept. Setting the value it already has is
    // nothing at all. setLineworkCodes refuses (InvalidArgument) what
    // entity::validate refuses. A colour table also bumps libraryGeneration
    // and reports StyleLibrary with Customisation.
    void setColourTable(katana::entity::ColourTable colours);
    [[nodiscard]] katana::core::Status setLineworkCodes(katana::entity::LineworkCodes codes);
    void setAutomation(katana::entity::CustomisationAutomation automation);
    // Whether the session is what the next start would give: true after it
    // was written to the kept file, false once it differs again.
    void setCustomisationKept(bool kept);
    // The name the host's built-in customisation declares (empty: none), which
    // is what CUSTOMISE BUILTIN marks as the default. startCustomisation sets
    // it.
    void setBuiltInCustomisationName(std::string name);
    // What a start found: what went wrong, a sentence each, and whether the
    // kept customisation was made from another built-in than the host has
    // (CustomisationState::startProblems, keptFromAnotherBuiltIn).
    // startCustomisation sets it, so that a report asked for later still has
    // what was said once at the start. It is no edit of the session: origin
    // and `kept` stand, and the value it already has is nothing at all.
    void setCustomisationStart(std::vector<std::string> problems, bool keptFromAnotherBuiltIn);
    // For a load installed through the raw setters - a front end that read
    // the files itself: adds its sources to the session's
    // (cad::recordCustomisationLoad) and takes them off what the open project
    // is missing (noteCustomisationLoaded). The origin becomes Loaded.
    // installCustomisation needs no such call; it takes the sources with the
    // customisation.
    void recordCustomisationLoad(const std::vector<CustomisationSource>& load,
                                 bool replacedDefinitions, bool replacedRules);

    // Broad-phase index over the entities, kept in step with the model
    // (PLAN.MD Phase 18). Maintained incrementally from the per-entity changes
    // an ordinary command reports, and rebuilt whole - which is what chooses
    // the cell size from the data - when the model is replaced, when the
    // drawing has doubled or halved since the cell was chosen, and when boxes
    // too big for the cell pile up (applyToSpatialIndex has the rules), so
    // that an import leaves the same index as opening the saved result would.
    //
    // Pass it to snap(), pickEntity() and pickInBox(). They give the same
    // answer without it, only slower, so a caller that has no Document loses
    // nothing but speed.
    [[nodiscard]] const katana::geometry::SpatialIndex& spatialIndex() const { return index_; }
    [[nodiscard]] const katana::commands::CommandStack& history() const { return *stack_; }
    // Counts up by one for every command executed, undone or redone, and for
    // every new or opened drawing; never down, and never for anything else -
    // a selection, the current layer or style, a save, the metadata, a
    // library or a survey map (those two have their own generations). "Has
    // the drawing changed since I last looked" is then one comparison, where
    // the history's counts could not tell "undo, then a new command" from
    // nothing, nor a reopened project from the drawing it replaced.
    [[nodiscard]] std::uint64_t modelRevision() const { return modelRevision_; }

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
    // The drafting aids - ortho, polar, object snaps, angle convention,
    // locks (drawing/drafting.hpp): session state shared by every view, the
    // command line and the verbs an agent drives them with. Not part of the
    // drawing, so not saved and not undone.
    [[nodiscard]] DraftingSettings& drafting() { return drafting_; }
    [[nodiscard]] const DraftingSettings& drafting() const { return drafting_; }
    // Call after changing the drafting settings so their toggles refresh.
    void notifyDraftingChanged();

    [[nodiscard]] const std::string& currentLayer() const { return currentLayer_; }
    [[nodiscard]] katana::core::Status setCurrentLayer(const std::string& name);
    // The style new work is drawn in - AutoCAD's CELTYPE, or a current point
    // style - so a symbol or a library linestyle can be drawn with, not only
    // applied afterwards. Empty means ByLayer, and is the default.
    //
    // Refused (NotFound) for a style the model does not have; "" clears it.
    // Kept consistent the way the current layer is: after any command, undo
    // or redo that leaves it naming no style - a delete, a merge, a rename -
    // it is CLEARED, not followed. Following a rename would need the command
    // to say what it renamed to, which no command event carries, and a
    // guess could put new work in the wrong style; ByLayer never can.
    [[nodiscard]] const std::string& currentStyle() const { return currentStyle_; }
    [[nodiscard]] katana::core::Status setCurrentStyle(const std::string& name);
    // Attributes for newly drawn entities: the current layer and style, and
    // ByLayer colour.
    [[nodiscard]] katana::commands::EntityAttributes currentAttributes() const;

    // ---- persistence ---------------------------------------------------------
    //
    // The customisation is not saved, but a RECORD of it is: the names the
    // drawing is drawn with (storage::ProjectMetadata::customisation;
    // customisation_record.hpp has the rules). The Document keeps that record
    // itself. A save writes it INTO WHAT IT
    // SAVES, and metadata() reads it back only once the save has succeeded:
    // it never goes through setMetadata, so a save that cannot go ahead
    // leaves an untouched drawing unmodified. open() works out which recorded
    // names this session lacks (customisationState().missingAtOpen), for the
    // front end to warn of and for the next save to keep; newDocument() has
    // none.
    //
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
    // The project's coordinate system (include/katana/cad/project_crs.hpp), set
    // as ONE undoable step named `stepName`: an EPSG code, WKT or a PROJ
    // string, stored as "EPSG:<code>" when the system has one; empty text for
    // local coordinates. InvalidCRS for text that names no system, and then
    // nothing changes; setting the value it already has is no step at all.
    [[nodiscard]] katana::core::Status setCoordinateSystem(std::string_view text,
                                                           std::string stepName = "SET_CRS");
    // The same change as a command, for a step that does more besides - an
    // IMPORT that adopts the file's coordinate system with its entities is
    // ONE undo step (docs/interop.md, "Import options"). Null when the
    // system is already `text`'s; InvalidCRS as setCoordinateSystem.
    [[nodiscard]] katana::core::Result<katana::commands::CommandPtr>
    coordinateSystemCommand(std::string_view text, std::string stepName = "SET_CRS");

    // ---- sheets (docs/plotting.md) -------------------------------------------
    //
    // The project's sheet set, kept as versioned JSON under the metadata key
    // "sheets" - the storage layer carries it as a key it does not read, so a
    // project gains sheets with no schema change - and parsed once per
    // change. An empty set when the project has none, or when its sheets
    // cannot be read; sheetSetStatus() then says why, and setSheetSet refuses
    // to overwrite sheets a newer Katana wrote.
    [[nodiscard]] const plotting::SheetSet& sheetSet() const;
    [[nodiscard]] katana::core::Status sheetSetStatus() const;
    // Replaces the sheet set as ONE undoable step named `stepName`; every
    // sheet edit (include/katana/cad/plotting/sheet_commands.hpp) is one.
    // Undo returns the exact set before it, and a save in between keeps
    // isModified() honest the way any other command does.
    [[nodiscard]] katana::core::Status setSheetSet(const plotting::SheetSet& sheets,
                                                   std::string stepName = "SET_SHEETS");
    // ---- surfaces (docs/terrain.md, "Surfaces in the drawing") -----------------
    //
    // The drawing's TIN surfaces, by name: made by SURFACE FROM, an import
    // or a corridor, edited by the surface edits, read by the volumes, the
    // views and a leader's surface values, and saved with the project. A
    // change is made on a COPY of the store - a copy costs a pointer per
    // surface, every surface being shared and immutable - and committed
    // here as ONE undoable step named `stepName`; undo puts the exact store
    // before it back. Committing a store equal to the current one records
    // nothing. Cleared by newDocument(), replaced by open().
    [[nodiscard]] const katana::terrain::SurfaceStore& surfaces() const;
    // Counts every change to the surfaces, undo and redo included, and never
    // goes back: a view compares it with the generation it last drew.
    [[nodiscard]] std::uint64_t surfacesGeneration() const;
    // InvalidArgument for a surface whose style is not in surfaceStyles().
    [[nodiscard]] katana::core::Status setSurfaces(katana::terrain::SurfaceStore surfaces,
                                                   std::string stepName = "SET_SURFACES");
    // The same change as a command, for a step that does more besides - an
    // IMPORT that brings entities and surfaces is ONE undo step. Null when
    // `surfaces` equals the current store. Const: it builds the command and
    // changes nothing until the command is executed through execute(), so a
    // tool holding a const Document can make one.
    [[nodiscard]] katana::core::Result<katana::commands::CommandPtr>
    surfacesCommand(katana::terrain::SurfaceStore surfaces,
                    std::string stepName = "SET_SURFACES") const;

    // The display styles (cad/terrain/surface_style.hpp): the project's, or
    // builtInSurfaceStyles() when it saved none. Replaced as ONE undoable
    // step; checkSurfaceStyle refuses a style that cannot be drawn, and a
    // style a surface uses cannot be removed (InvalidArgument naming it).
    [[nodiscard]] const SurfaceStyleSet& surfaceStyles() const;
    [[nodiscard]] katana::core::Status setSurfaceStyles(SurfaceStyleSet styles,
                                                        std::string stepName = "SET_SURFACE_STYLES");
    // Counts every change to the styles, undo and redo included, and never
    // goes back: what a view's cache of a style's drawing keys on.
    [[nodiscard]] std::uint64_t surfaceStylesGeneration() const;
    // Why the project's styles could not be read when it was opened - a
    // newer Katana's (Unsupported), or damaged (ParseFailure) - while the
    // built-ins stand in for them; success otherwise. Styles a newer Katana
    // wrote are kept as they were and written back by a save, and
    // setSurfaceStyles refuses (CommandRejected) to overwrite them; damaged
    // ones are replaced by the first set given.
    [[nodiscard]] katana::core::Status surfaceStylesStatus() const;
    // The surfaces and the styles as ONE undoable step, checked together:
    // what renaming a style a surface uses needs, the style renamed in the
    // set and in every surface that names it at once.
    [[nodiscard]] katana::core::Status setSurfacesAndStyles(katana::terrain::SurfaceStore surfaces,
                                                            SurfaceStyleSet styles,
                                                            std::string stepName);

    // ---- the road model (docs/road_design.md) -----------------------------------
    //
    // The drawing's road templates and corridors, each kept as versioned JSON
    // under a project metadata key of its own ("road_templates",
    // "corridors"), as the sheets are - the storage layer carries a key it
    // does not read, so no schema change - and parsed once per change. Empty
    // when the project has none, or when what it has cannot be read; the
    // status then says why, and the setters refuse to overwrite what a newer
    // Katana wrote. Each is replaced as ONE undoable step named `stepName`;
    // InvalidArgument for a set checkTemplate or checkCorridor refuses, or
    // two of one name. A set equal to the current one records nothing.
    //
    // Every change of the road model's keys - these two and the placed
    // objects' "road_objects" - is reported as DocumentChange::RoadModel with
    // the step that made it (src/katana_cad/road/road_document.cpp).
    [[nodiscard]] const road::RoadTemplateSet& roadTemplates() const;
    [[nodiscard]] katana::core::Status roadTemplatesStatus() const;
    [[nodiscard]] katana::core::Status
    setRoadTemplates(const road::RoadTemplateSet& templates,
                     std::string stepName = "SET_ROAD_TEMPLATES");
    // As a command, for a step that does more besides (a template renamed
    // with the corridors that name it); null when nothing changes.
    [[nodiscard]] katana::core::Result<katana::commands::CommandPtr>
    roadTemplatesCommand(const road::RoadTemplateSet& templates,
                         std::string stepName = "SET_ROAD_TEMPLATES");
    [[nodiscard]] const road::CorridorSet& corridors() const;
    [[nodiscard]] katana::core::Status corridorsStatus() const;
    [[nodiscard]] katana::core::Status setCorridors(const road::CorridorSet& corridors,
                                                    std::string stepName = "SET_CORRIDORS");
    // The same change as a command, for a step that does more besides -
    // CORRIDOR NEW makes the layers its objects are shown on in the same
    // step. Null when `corridors` equals the current set.
    [[nodiscard]] katana::core::Result<katana::commands::CommandPtr>
    corridorsCommand(const road::CorridorSet& corridors, std::string stepName = "SET_CORRIDORS");

    // ---- annotation (docs/annotation.md) ---------------------------------------
    //
    // The scale the plan view draws paper-sized annotation at: text, labels,
    // leaders and paper-sized dimensions are drawn paperMm x scale / 1000
    // model units tall there (a sheet viewport uses its own scale instead).
    // 1 : entity::kDefaultAnnotationScale until one is chosen. Kept, like the
    // sheets, under a project metadata key ("annotation_scale") the storage
    // layer carries without a schema change, so it travels with the project.
    [[nodiscard]] double annotationScale() const;
    // Sets it as ONE undoable step; refuses a scale that is not finite and
    // positive. Choosing the scale it already has records nothing.
    [[nodiscard]] katana::core::Status setAnnotationScale(double scale);

    // ---- survey jobs (cad/survey_job.hpp) --------------------------------------
    //
    // The field files imported as jobs, in id order, kept so their reduction
    // and adjustment can be revisited. Changed ONLY by the survey job
    // commands, which are undoable - there is deliberately no setter; the
    // commands come in through SurveyJobAccess. Saved with the project
    // (storage::ProjectContents::surveyJobs); cleared by newDocument() and
    // replaced by open().
    [[nodiscard]] const std::vector<katana::storage::SurveyJob>& surveyJobs() const
    {
        return surveyJobs_;
    }
    // nullptr when there is no job with that id.
    [[nodiscard]] const katana::storage::SurveyJob* findSurveyJob(std::string_view id) const;
    // Counts every change to the job list, for a panel that lists the jobs.
    [[nodiscard]] std::uint64_t surveyJobsGeneration() const { return surveyJobsGeneration_; }

    // ---- road objects (docs/road_objects.md) ------------------------------------
    //
    // The objects placed along an alignment or a line - barriers, guide
    // posts, signs, light poles, culverts, walls, fences, line markings
    // (cad/road/road_objects.hpp) - kept as versioned JSON under the metadata
    // key "road_objects", the sheets' pattern: carried by the storage layer
    // as a key it does not read, parsed once per change. Their 3D objects are
    // made from them every time (bim::placedObjects). An empty set when there
    // are none or they cannot be read; roadObjectsStatus() then says why, and
    // setRoadObjects refuses to overwrite objects a newer Katana wrote.
    // Kept in src/katana_cad/road/road_object_store.cpp.
    [[nodiscard]] const road::RoadObjectSet& roadObjects() const;
    [[nodiscard]] katana::core::Status roadObjectsStatus() const;
    // Replaces the set as ONE undoable step named `stepName`, which also
    // makes any layer an object names that the drawing lacks; the step, its
    // undo and its redo are reported as DocumentChange::RoadModel, as every
    // change of a road-model key is. A set equal to the current one records
    // nothing.
    [[nodiscard]] katana::core::Status setRoadObjects(const road::RoadObjectSet& objects,
                                                      std::string stepName = "SET_ROAD_OBJECTS");
    // The same change as a command, for a step that does more besides - a
    // surface renamed with the corridors and objects that name it is ONE undo
    // step. Null when `objects` equals the current set; the errors of
    // setRoadObjects otherwise.
    [[nodiscard]] katana::core::Result<katana::commands::CommandPtr>
    roadObjectsCommand(const road::RoadObjectSet& objects,
                       std::string stepName = "SET_ROAD_OBJECTS");

    // Called after anything observable changed: model, selection, current
    // layer, project. Listeners must not mutate the document re-entrantly.
    // The registration lasts as long as the handle does - keep it as a member
    // of the object the listener captures, declared so that it dies first.
    //
    // Both kinds are called for every notification, in the order they were
    // added. A ChangeListener is told what changed; a Listener is not, and
    // so has to treat every call as a change to everything.
    [[nodiscard]] ListenerHandle addListener(Listener listener);
    [[nodiscard]] ListenerHandle addListener(ChangeListener listener);

  private:
    friend class SurveyJobAccess;

    void rebuildStack();
    // The model, the metadata and the survey jobs into `store` in one save,
    // with the customisation record as it stands now; on success the record
    // saved becomes the metadata's, and the return says whether that changed
    // it.
    [[nodiscard]] katana::core::Result<bool> saveContents(katana::storage::ProjectStore& store);
    // The origin is Edited and the session no longer kept; counted only when
    // that changes something.
    void markCustomisationEdited();
    // The record's renames: the early file names, answered by the NSW
    // customisation (builtinRenames()).
    [[nodiscard]] std::vector<RenamedSource> customisationRenames() const;
    // Whole index from the current model; picks the cell size from the data.
    void rebuildSpatialIndex();
    // The index after one command: incremental for an ordinary edit, a
    // whole rebuild when the drawing has doubled or halved since the cell was
    // chosen, or when boxes too big for the cell have piled up.
    void applyToSpatialIndex(const std::vector<katana::entity::ChangeEvent>& changes);
    // The DocumentChange table bits of every table whose revision moved
    // since the last call (or rememberTables), and remembers the new ones.
    [[nodiscard]] std::uint32_t tablesChanged();
    void rememberTables();
    // True when it removed anything from the selection.
    bool pruneSelection();
    // ---- the surfaces' share of new, open and save (surface_document.cpp) ----
    //
    // The surfaces and their styles as one value: what open() reads out of a
    // project and checks before it installs anything, and what newDocument()
    // installs empty.
    struct SurfaceState {
        katana::terrain::SurfaceStore surfaces;
        SurfaceStyleSet styles = builtInSurfaceStyles();
        katana::core::Status stylesStatus{};
    };
    // Decodes the project's surface records and reads its styles' metadata
    // key, taking the key out of `metadata` when it was read (it is the
    // Document's to write) and leaving it when it could not be (written back
    // as it was). Fails, and the open with it, on a surface it cannot decode.
    [[nodiscard]] static katana::core::Result<SurfaceState>
    readSurfaceState(std::vector<katana::storage::SurfaceRecord>& records,
                     katana::storage::ProjectMetadata& metadata);
    void installSurfaceState(SurfaceState state);
    // The surfaces and the styles into what a save writes.
    void writeSurfaceState(katana::storage::ProjectContents& contents) const;
    // DocumentChange::Surfaces and SurfaceStyles for what moved since the
    // last call; the stack's listener adds them to a step's notification.
    [[nodiscard]] std::uint32_t surfacePartsChanged();
    void notify(const DocumentChange& change);
    void notify(std::uint32_t parts) { notify(DocumentChange{.parts = parts}); }

    katana::core::Logger* logger_ = nullptr;
    katana::entity::Model model_;
    katana::entity::StyleLibrary library_;
    katana::entity::SurveyMap surveyMap_;
    CustomisationState customisation_{};
    std::uint64_t libraryGeneration_ = 0;
    std::uint64_t surveyMapGeneration_ = 0;
    std::uint64_t customisationGeneration_ = 0;
    std::uint64_t modelRevision_ = 0;
    katana::geometry::SpatialIndex index_;
    // How many entities the model held when the index last chose its cell
    // size (its last rebuild). 0 until the first rebuild: an index that has
    // never been rebuilt is on the default cell, which was chosen for no data.
    std::size_t indexChosenFor_ = 0;
    // How many boxes that rebuild had to put on the oversized list: some data
    // needs a few whatever the cell (applyToSpatialIndex, the overflow rule).
    std::size_t indexOversizedAtRebuild_ = 0;
    // Each table's revision when tablesChanged() last looked.
    struct TableRevisions {
        std::uint64_t layers = 0;
        std::uint64_t styles = 0;
        std::uint64_t linetypes = 0;
        std::uint64_t dimensionStyles = 0;
        std::uint64_t hatchPatterns = 0;
        std::uint64_t alignments = 0;
        std::uint64_t properties = 0;
        std::uint64_t textStyles = 0;
        std::uint64_t labelStyles = 0;
        std::uint64_t labelRules = 0;
    };
    TableRevisions tablesSeen_{};
    std::unique_ptr<katana::commands::CommandStack> stack_;
    std::unique_ptr<katana::storage::ProjectStore> store_;
    katana::storage::ProjectMetadata metadata_;
    SelectionSet selection_;
    DraftingSettings drafting_;
    std::string currentLayer_{katana::entity::kDefaultLayerName};
    std::string currentStyle_{};
    std::shared_ptr<ListenerHandle::Registry> listeners_;
    bool metadataModified_ = false;
    // The parsed sheet set and the JSON it was parsed from (sheet_store.cpp).
    // Never changed once made: an undo step shares it, so undo and redo swap
    // a pointer instead of parsing the set again.
    struct SheetCache;
    mutable std::shared_ptr<const SheetCache> sheetCache_;
    // The parsed road objects and their JSON (road/road_object_store.cpp),
    // shared by the undo steps as the sheet cache is.
    struct RoadObjectCache;
    mutable std::shared_ptr<const RoadObjectCache> roadObjectCache_;
    std::vector<katana::storage::SurveyJob> surveyJobs_;
    std::uint64_t surveyJobsGeneration_ = 0;
    // The surfaces and their display styles (surfaces(), surfaceStyles()).
    // Their implementation, persistence and undo are in
    // src/katana_cad/terrain/surface_document.cpp.
    katana::terrain::SurfaceStore surfaces_;
    std::uint64_t surfacesGeneration_ = 0;
    SurfaceStyleSet surfaceStyles_ = builtInSurfaceStyles();
    std::uint64_t surfaceStylesGeneration_ = 0;
    katana::core::Status surfaceStylesStatus_{};
    // The generations the last notification reported (surfacePartsChanged).
    std::uint64_t surfacesNotified_ = 0;
    std::uint64_t surfaceStylesNotified_ = 0;
    // ---- road model (src/katana_cad/road/road_document.cpp) ----
    // The parsed road templates and corridors and the JSON each was parsed
    // from, shared by an undo step as the sheet set's are.
    struct RoadTemplateCache;
    struct CorridorCache;
    mutable std::shared_ptr<const RoadTemplateCache> roadTemplateCache_;
    mutable std::shared_ptr<const CorridorCache> corridorCache_;
    // The road model's metadata keys as the last command's notification saw
    // them: what roadModelChanged compares, to report RoadModel.
    std::vector<std::optional<std::string>> roadModelSeen_;
    [[nodiscard]] std::uint32_t roadModelChanged();
    // The road model's 3D objects, made once per change of what they are
    // made from (bim::bimModel, src/katana_cad/bim/bim_object.cpp).
    friend class bim::BimCacheAccess;
    struct BimCache;
    mutable std::shared_ptr<BimCache> bimCache_;
    // ---- end road model ----
};

} // namespace katana::cad
