#include "katana/cad/document.hpp"

#include "katana/cad/annotation/associative.hpp"
#include "katana/cad/spatial_query.hpp"

#include <algorithm>
#include <utility>

namespace katana::cad {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Status;

namespace {

// A new or opened drawing: everything but the customisation, which a new or
// an open keeps (DocumentChange::Replaced).
constexpr std::uint32_t kReplaced = DocumentChange::Replaced | DocumentChange::Entities |
                                    DocumentChange::Selection | DocumentChange::kTables |
                                    DocumentChange::CurrentAttributes | DocumentChange::History |
                                    DocumentChange::Saved | DocumentChange::Metadata;

} // namespace

Document::Document(katana::core::Logger* logger) : logger_(logger)
{
    rebuildStack();
    // The tables' revisions start wherever constructing them left them; the
    // first command must not report every table as changed.
    rememberTables();
}

Document::~Document() = default;

void Document::rebuildStack()
{
    // Destroy the old stack first: its destructor clears the model's entity
    // observer, which must not happen after the new stack has installed its own.
    stack_.reset();
    stack_ = std::make_unique<katana::commands::CommandStack>(model_, logger_);
    stack_->addListener([this](const katana::commands::CommandEvent& event) {
        applyToSpatialIndex(event.changes);
        std::uint32_t parts = DocumentChange::History | tablesChanged();
        if (!event.changes.empty()) {
            parts |= DocumentChange::Entities;
        }
        // Undo can delete selected entities and the current layer.
        if (pruneSelection()) {
            parts |= DocumentChange::Selection;
        }
        if (!model_.layers.contains(currentLayer_)) {
            currentLayer_ = std::string(katana::entity::kDefaultLayerName);
            parts |= DocumentChange::CurrentAttributes;
        }
        // And the current style: a delete, merge or rename of it - or the
        // undo of its creation - must not leave new work naming nothing.
        if (!currentStyle_.empty() && !model_.styles.contains(currentStyle_)) {
            currentStyle_.clear();
            parts |= DocumentChange::CurrentAttributes;
        }
        // The stack publishes only a step that succeeded, so a refused
        // command moves nothing.
        ++modelRevision_;
        notify(DocumentChange{.parts = parts, .entities = event.changes});
    });
}

std::uint32_t Document::tablesChanged()
{
    const TableRevisions now{
        .layers = model_.layers.revision(),
        .styles = model_.styles.revision(),
        .linetypes = model_.linetypes.revision(),
        .dimensionStyles = model_.dimensionStyles.revision(),
        .hatchPatterns = model_.hatchPatterns.revision(),
        .alignments = model_.alignments.revision(),
        .properties = model_.properties.revision(),
        .textStyles = model_.textStyles.revision(),
        .labelStyles = model_.labelStyles.revision(),
        .labelRules = model_.labelRules.revision(),
    };
    std::uint32_t parts = 0;
    const auto compare = [&](std::uint64_t before, std::uint64_t after, DocumentChange::Part part) {
        if (before != after) {
            parts |= part;
        }
    };
    compare(tablesSeen_.layers, now.layers, DocumentChange::Layers);
    compare(tablesSeen_.styles, now.styles, DocumentChange::Styles);
    compare(tablesSeen_.linetypes, now.linetypes, DocumentChange::Linetypes);
    compare(tablesSeen_.dimensionStyles, now.dimensionStyles, DocumentChange::DimensionStyles);
    compare(tablesSeen_.hatchPatterns, now.hatchPatterns, DocumentChange::HatchPatterns);
    compare(tablesSeen_.alignments, now.alignments, DocumentChange::Alignments);
    compare(tablesSeen_.properties, now.properties, DocumentChange::PropertyDefinitions);
    compare(tablesSeen_.textStyles, now.textStyles, DocumentChange::AnnotationStyles);
    compare(tablesSeen_.labelStyles, now.labelStyles, DocumentChange::AnnotationStyles);
    compare(tablesSeen_.labelRules, now.labelRules, DocumentChange::AnnotationStyles);
    tablesSeen_ = now;
    return parts;
}

void Document::rememberTables()
{
    (void)tablesChanged();
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
    indexChosenFor_ = model_.entities.size();
    indexOversizedAtRebuild_ = index_.oversizedCount();
}

// WHEN THE INDEX IS REBUILT RATHER THAN UPDATED.
//
// Incrementally, the index is only ever as good as the cell size it was last
// given, and the cell size is chosen by a rebuild alone. An index that has
// never been rebuilt keeps the default one-unit cell, so a 12 km survey
// imported into a new drawing was filed into 722,715 buckets with 4,889 boxes
// on the oversized list every query scans: the import's execute took about a
// second against 28 ms for the create itself, and every snap, pick and
// repaint was 36x slower until the project was reopened - opening being the
// one thing that rebuilt it.
//
// So the index is rebuilt, re-choosing the cell, when
//   * THE SIZE RULE: the drawing has grown to twice, or shrunk to half, the
//     number of entities the cell was chosen for. An empty drawing's cell was
//     chosen for none, so the first command into it - an import - always
//     rebuilds, and without first filing everything into the default cell.
//   * THE OVERFLOW RULE: after a command is applied incrementally, the
//     oversized list holds more than twice what the last rebuild left there
//     plus a hundredth of the drawing. Boxes too big for the cell are what
//     make every query slow, and they come from a command that brings in or
//     scales up geometry the cell was not chosen for.
// NOT when a command merely touches many entities. That was the first rule
// tried (a tenth of the drawing), and measured on the 27,886-entity corridor
// stand-in (BM_MoveATenthOfAnImport*, bench_cad.cpp) it made a MOVE of a
// tenth, with its undo, about 4x slower than the same move one entity short
// of the threshold (101 against 24 ms, min of 9) - two rebuilds of the whole
// drawing for a move that changes no box's size and so no cell choice.
//
// Neither rule fires on an ordinary click on a drawing of any size, which
// stays an O(1) update. A rebuild costs O(n), and each rule fires only after
// Theta(n) entities were added, removed or pushed onto the oversized list since
// the last one, so a sequence of commands costs a constant factor over the
// edits in it, never O(n) per edit.
void Document::applyToSpatialIndex(const std::vector<katana::entity::ChangeEvent>& changes)
{
    if (changes.empty()) {
        return; // a table-only command: nothing the index holds moved
    }
    const std::size_t entities = model_.entities.size();
    const bool resized = entities >= 2 * indexChosenFor_ || 2 * entities <= indexChosenFor_;
    // After a Cleared the data is about to be different altogether.
    const bool cleared =
        std::ranges::any_of(changes, [](const katana::entity::ChangeEvent& change) {
            return change.kind == katana::entity::ChangeKind::Cleared;
        });
    if (resized || cleared) {
        rebuildSpatialIndex();
        return;
    }
    for (const katana::entity::ChangeEvent& change : changes) {
        switch (change.kind) {
        case katana::entity::ChangeKind::Cleared:
            break; // handled above, before any incremental update
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
    // The overflow rule, above.
    if (index_.oversizedCount() > 2 * indexOversizedAtRebuild_ + entities / 100) {
        rebuildSpatialIndex();
    }
}

Status Document::execute(katana::commands::CommandPtr command)
{
    // Every command carries the associative update with it: the dimensions,
    // leaders and labels that follow what it changed are moved in the SAME
    // undo step (annotation/associative.hpp), so one undo puts back the
    // geometry and everything that followed it.
    return stack_->execute(annotation::withAssociativeUpdate(std::move(command)));
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
    notify(DocumentChange::Selection);
}

void Document::notifyDraftingChanged() { notify(DocumentChange::Drafting); }

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
    notify(DocumentChange::CurrentAttributes);
    return {};
}

Status Document::setCurrentStyle(const std::string& name)
{
    if (!name.empty() && !model_.styles.contains(name)) {
        return makeError(ErrorCode::NotFound, "style does not exist", name);
    }
    if (name != currentStyle_) {
        currentStyle_ = name;
        notify(DocumentChange::CurrentAttributes);
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
    rememberTables();
    // The customisation is kept; what the LAST project lacked of its own
    // record is not: a new drawing recorded nothing. Left standing, a bare
    // CUSTOMISE went on naming the old project's files.
    const bool hadMissing = !customisation_.missingAtOpen.empty();
    if (hadMissing) {
        customisation_.missingAtOpen.clear();
        ++customisationGeneration_;
    }
    notify(kReplaced | (hadMissing ? std::uint32_t{DocumentChange::Customisation} : 0u));
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
    rememberTables();
    // What the project was drawn with that this session does not have. Worked
    // out here, at the open, so that every front end is told the same names
    // and the next save knows which it could not judge.
    std::vector<std::string> missing = customisationNotLoaded(
        metadata_.customisation, library_, customisation_.sources, customisationRenames());
    const bool missingChanged = missing != customisation_.missingAtOpen;
    if (missingChanged) {
        customisation_.missingAtOpen = std::move(missing);
        ++customisationGeneration_;
    }
    notify(kReplaced | (missingChanged ? std::uint32_t{DocumentChange::Customisation} : 0u));
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
    const auto saved = saveContents(*store_);
    if (!saved) {
        return saved.error();
    }
    stack_->markSaved();
    metadataModified_ = false;
    if (logger_ != nullptr) {
        logger_->info("storage", "project saved",
                      {{"entities", std::to_string(model_.entities.size())}});
    }
    // The record the save wrote is the metadata's now, when it differs.
    notify(DocumentChange::Saved | (*saved ? std::uint32_t{DocumentChange::Metadata} : 0u));
    return {};
}

katana::core::Result<bool> Document::saveContents(katana::storage::ProjectStore& store)
{
    katana::storage::ProjectContents contents =
        katana::storage::captureModel(model_, metadata_);
    // The record of what the drawing is drawn with, as the session stands at
    // this save. Written into what is SAVED and not through setMetadata: that
    // marks the drawing modified, and a save that then could not go ahead
    // left a drawing nobody had touched asking to be saved - which is why the
    // front ends, when they wrote it, first had to work out whether the save
    // had anywhere to go.
    contents.metadata.customisation =
        customisationRecordToSave(metadata_.customisation, customisation_.missingAtOpen, library_,
                                  customisation_.sources, customisationRenames());
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
    if (auto status = store.save(contents); !status) {
        return status.error();
    }
    // Only now: a failed save leaves the metadata as it was.
    const bool changed = metadata_.customisation != contents.metadata.customisation;
    metadata_.customisation = std::move(contents.metadata.customisation);
    return changed;
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
    if (const auto saved = saveContents(*store); !saved) {
        return saved.error();
    }
    store_ = std::make_unique<katana::storage::ProjectStore>(std::move(*store));
    stack_->markSaved();
    metadataModified_ = false;
    // The name and the creation time above may have been filled in.
    notify(DocumentChange::Saved | DocumentChange::Metadata);
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
        notify(DocumentChange::Metadata);
    }
}

struct Document::ListenerHandle::Registry {
    // Exactly one of the two is set. Both are kept rather than the plain one
    // wrapped in a ChangeListener, which would cost an allocation per call
    // wherever a caller copies it.
    struct Entry {
        std::uint64_t id;
        Listener plain;
        ChangeListener typed;
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
    listeners_->entries.push_back({id, std::move(listener), {}});
    return ListenerHandle(listeners_, id);
}

Document::ListenerHandle Document::addListener(ChangeListener listener)
{
    if (!listeners_) {
        listeners_ = std::make_shared<ListenerHandle::Registry>();
    }
    const std::uint64_t id = listeners_->nextId++;
    listeners_->entries.push_back({id, {}, std::move(listener)});
    return ListenerHandle(listeners_, id);
}

bool Document::pruneSelection()
{
    return selection_.prune(model_.entities);
}

void Document::notify(const DocumentChange& change)
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
        if (found == listeners_->entries.end()) {
            continue;
        }
        if (found->typed) {
            found->typed(change);
        } else if (found->plain) {
            found->plain();
        }
    }
}

void Document::setStyleLibrary(katana::entity::StyleLibrary library)
{
    library_ = std::move(library);
    ++libraryGeneration_;
    markCustomisationEdited();
    notify(DocumentChange::StyleLibrary);
}

void Document::setSurveyMap(katana::entity::SurveyMap map)
{
    surveyMap_ = std::move(map);
    ++surveyMapGeneration_;
    markCustomisationEdited();
    notify(DocumentChange::SurveyMap);
}

// ---- the customisation's session state ------------------------------------------------------

const char* toString(CustomisationOrigin origin)
{
    switch (origin) {
    case CustomisationOrigin::None:
        return "none";
    case CustomisationOrigin::BuiltIn:
        return "builtIn";
    case CustomisationOrigin::Kept:
        return "kept";
    case CustomisationOrigin::Loaded:
        return "loaded";
    case CustomisationOrigin::Edited:
        return "edited";
    }
    return "none";
}

void Document::markCustomisationEdited()
{
    if (customisation_.origin == CustomisationOrigin::Edited && !customisation_.kept) {
        return;
    }
    customisation_.origin = CustomisationOrigin::Edited;
    customisation_.kept = false;
    ++customisationGeneration_;
}

std::vector<RenamedSource> Document::customisationRenames() const
{
    return builtinRenames(customisation_.builtIn);
}

std::vector<katana::core::Error>
customisationFaults(const katana::entity::Customisation& customisation)
{
    std::vector<katana::core::Error> faults;
    const auto fault = [&](std::string part, const katana::core::Error& refusal) {
        faults.push_back(makeError(ErrorCode::InvalidArgument,
                                   std::move(part) + ": " + refusal.message, refusal.context));
    };
    if (auto status = katana::entity::validateCustomisationName(customisation.name); !status) {
        fault("its name", status.error());
    }
    for (const CustomisationSource& source : customisation.sources) {
        if (auto status = katana::entity::validateCustomisationName(source.name); !status) {
            fault("the name of one of its sources", status.error());
        }
    }
    // A definition's source reaches a project's record exactly as the
    // customisation's own name does (customisationRecord); one made in a
    // session has none, and that is no fault.
    customisation.library.forEach([&](const katana::entity::LineStyle& definition) {
        if (definition.source.empty()) {
            return;
        }
        if (auto status = katana::entity::validateCustomisationName(definition.source);
            !status) {
            fault("the source of its definition \"" + definition.name + "\"", status.error());
        }
    });
    if (customisation.linework) {
        if (auto status = katana::entity::validate(*customisation.linework); !status) {
            fault("its linework codes", status.error());
        }
    }
    if (customisation.basedOn) {
        // Judged by the format's own writer, which is what a KEEP hands it
        // to: what a digest looks like is the format's to say, and is said
        // there, once. A customisation of nothing but this basedOn, under a
        // name that is fine, can be refused for nothing else; the refusal
        // already begins "basedOn: ".
        katana::entity::Customisation probe;
        probe.name = "basedOn";
        probe.basedOn = customisation.basedOn;
        if (const auto written = katana::entity::customisationToJson(probe); !written) {
            faults.push_back(makeError(ErrorCode::InvalidArgument,
                                       "its " + written.error().message,
                                       written.error().context));
        }
    }
    return faults;
}

Status Document::installCustomisation(katana::entity::Customisation customisation,
                                      CustomisationOrigin origin, bool kept)
{
    // Everything is checked before anything is taken: a refusal leaves the
    // session exactly as it was.
    if (origin == CustomisationOrigin::None) {
        return makeError(ErrorCode::InvalidArgument,
                         "a customisation is installed with where it came from, which is never "
                         "\"none\"");
    }
    if (const std::vector<katana::core::Error> faults = customisationFaults(customisation);
        !faults.empty()) {
        // The first is enough to refuse on; a load that wants them all asks
        // for them all (mergeCustomisation).
        return makeError(ErrorCode::InvalidArgument,
                         "the customisation could not be kept or recorded: " +
                             faults.front().message,
                         faults.front().context);
    }

    library_ = std::move(customisation.library);
    surveyMap_ = std::move(customisation.map);
    customisation_.origin = origin;
    customisation_.kept = kept;
    customisation_.description = std::move(customisation.description);
    customisation_.notice = std::move(customisation.notice);
    customisation_.basedOn = std::move(customisation.basedOn);
    customisation_.colours = std::move(customisation.colours);
    // Said, or the DEFAULTS - never what the session had. An install takes
    // the place of the whole session, and `kept` promises it is the session
    // the next start would give: that start has no earlier session to have
    // left its control codes behind. Left alone, a reset to the built-in kept
    // the spellings and the switches it was meant to reset. (A load that
    // says nothing leaves the session's alone through mergeCustomisation,
    // which hands over a customisation that says both.)
    customisation_.linework = customisation.linework
                                  ? std::move(*customisation.linework)
                                  : katana::entity::LineworkCodes{};
    customisation_.automation =
        customisation.automation.value_or(katana::entity::CustomisationAutomation{});
    customisation_.sources = std::move(customisation.sources);
    if (customisation_.sources.empty()) {
        // A customisation that lists none is its own one source. Its notice
        // stays the customisation's, above; a source's own notice is for one
        // merged into another's.
        customisation_.sources.push_back(
            {customisation.name, !library_.empty(), !surveyMap_.empty(), {}});
    }
    customisation_.name = std::move(customisation.name);
    noteCustomisationLoaded(customisation_.missingAtOpen, customisation_.sources,
                            customisationRenames());
    ++libraryGeneration_;
    ++surveyMapGeneration_;
    ++customisationGeneration_;
    notify(DocumentChange::StyleLibrary | DocumentChange::SurveyMap |
           DocumentChange::Customisation);
    return {};
}

katana::entity::Customisation Document::customisation() const
{
    katana::entity::Customisation session;
    session.name = customisation_.name;
    session.description = customisation_.description;
    session.notice = customisation_.notice;
    session.sources = customisation_.sources;
    session.basedOn = customisation_.basedOn;
    session.colours = customisation_.colours;
    session.linework = customisation_.linework;
    session.automation = customisation_.automation;
    session.library = library_;
    session.map = surveyMap_;
    return session;
}

void Document::setColourTable(katana::entity::ColourTable colours)
{
    if (colours == customisation_.colours) {
        return;
    }
    customisation_.colours = std::move(colours);
    customisation_.origin = CustomisationOrigin::Edited;
    customisation_.kept = false;
    ++customisationGeneration_;
    // What a pen inside a definition resolves to has changed, so whatever
    // was baked from the library is stale.
    ++libraryGeneration_;
    notify(DocumentChange::Customisation | DocumentChange::StyleLibrary);
}

Status Document::setLineworkCodes(katana::entity::LineworkCodes codes)
{
    if (auto status = katana::entity::validate(codes); !status) {
        return status;
    }
    if (codes == customisation_.linework) {
        return {};
    }
    customisation_.linework = std::move(codes);
    customisation_.origin = CustomisationOrigin::Edited;
    customisation_.kept = false;
    ++customisationGeneration_;
    notify(DocumentChange::Customisation);
    return {};
}

void Document::setAutomation(katana::entity::CustomisationAutomation automation)
{
    if (automation == customisation_.automation) {
        return;
    }
    customisation_.automation = automation;
    customisation_.origin = CustomisationOrigin::Edited;
    customisation_.kept = false;
    ++customisationGeneration_;
    notify(DocumentChange::Customisation);
}

void Document::setCustomisationKept(bool kept)
{
    if (kept == customisation_.kept) {
        return;
    }
    customisation_.kept = kept;
    ++customisationGeneration_;
    notify(DocumentChange::Customisation);
}

void Document::setBuiltInCustomisationName(std::string name)
{
    if (name == customisation_.builtIn) {
        return;
    }
    customisation_.builtIn = std::move(name);
    ++customisationGeneration_;
    notify(DocumentChange::Customisation);
}

void Document::recordCustomisationLoad(const std::vector<CustomisationSource>& load,
                                       bool replacedDefinitions, bool replacedRules)
{
    katana::cad::recordCustomisationLoad(customisation_.sources, load, replacedDefinitions,
                                         replacedRules);
    noteCustomisationLoaded(customisation_.missingAtOpen, load, customisationRenames());
    customisation_.origin = CustomisationOrigin::Loaded;
    customisation_.kept = false;
    ++customisationGeneration_;
    notify(DocumentChange::Customisation);
}

const katana::entity::LineStyle* Document::definitionFor(std::string_view name) const
{
    return name.empty() ? nullptr : library_.find(name);
}

} // namespace katana::cad
