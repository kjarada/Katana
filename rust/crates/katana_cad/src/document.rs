//! An open drawing: the domain model plus everything a session needs around
//! it - command history, selection, current layer, the project it is stored
//! in (include/katana/cad/document.hpp, src/katana_cad/document.cpp).
//!
//! The Document is UI independent. The window, the CLI and the application API
//! all drive the same object, and all mutations go through execute() so they
//! are validated, undoable and observable.
//!
//! Threading: single-threaded, owned by the application's main thread.
//!
//! # The shape of the Rust Document, and why
//!
//! This is the decision every module of the cad layer builds on (the port's
//! cad plan, section 7, risk 1). The C++'s own commands capture `Document&`
//! (or `this`) and change what is not in the model - the metadata and its
//! keys, the surfaces and their styles, the survey jobs, the parsed caches -
//! in their execute, undo and redo. A Rust Command sees only
//! `CommandContext { model, logger }` and must be Send, and the undo stack
//! takes the model per call (katana_commands' CommandStack). So:
//!
//! * **The Document owns its model and its stack**, and hands the model to
//!   the stack on every execute, undo and redo.
//! * **Everything a command may read or change that is not the model is one
//!   value, [`DocumentState`]**, which lives in `Document::state` between
//!   steps, so every accessor returns a plain reference as the C++ does
//!   (`metadata() -> &ProjectMetadata`, `surfaces() -> &SurfaceStore`,
//!   `survey_jobs() -> &[SurveyJob]`). For the length of one execute, undo or
//!   redo it is LENT to a shared slot, `Arc<Mutex<Option<DocumentState>>>`
//!   ([`StateLink`]), which the document's own commands capture: a command
//!   reaches the state there, reads it and changes it in order (a transaction
//!   of two such steps sees the first's change), and it comes home when the
//!   stack returns. The plan recommended keeping that state in the
//!   `Arc<Mutex>` for good, with accessors handing out `Arc` snapshots; the
//!   C++ answered against that: its readers hold references (a survey job is
//!   found by pointer, and holds its field file whole - tens of megabytes a
//!   snapshot would copy on the next change), so the state rests in the
//!   Document and only the step borrows it. Rejected too: commands that leave
//!   their changes in a mailbox for the Document to apply after the step - a
//!   survey job command reads the job list while it runs (its new job's id).
//! * **A command run anywhere but through the Document that made it finds the
//!   slot empty** and fails with InvalidState, changing nothing: the C++'s
//!   `requireDocumentModel` guard, made structural.
//! * **The parsed caches** (the sheet set, road templates, corridors, road
//!   objects) and the BIM model are of types later modules define, so each is
//!   a type-erased [`CacheSlot`], filled from `&self` as the C++'s `mutable`
//!   members are. [`CacheSlot::parsed_key`] is the C++'s `upToDate` (a cache
//!   of one metadata key, compared by content) and
//!   [`Document::parsed_key_command`] its `changeCommand` (the undoable "key,
//!   text, value" step); an accessor of a parsed set returns `Arc<T>`.
//! * **What a step changed** is worked out after the stack returns: the stack
//!   cannot reach the Document, so its one listener keeps the event it
//!   publishes, and the Document then does what the C++ listener did (the
//!   spatial index, the table and surface bits, the road model, the
//!   selection, the current layer and style) and notifies.
//! * **Listeners are `'static + Send` closures** in an `Arc<Mutex>` registry; a
//!   [`ListenerHandle`] holds it weakly and ends its registration when
//!   dropped. A notification calls a snapshot of the list, and skips one
//!   ended since, because a listener may end another's registration while it
//!   runs.
//! * **Members implemented in other C++ files are `impl Document` blocks in
//!   the modules that port those files**, over the `pub(crate)` fields
//!   declared here, so nothing edits this file again: surface_document.rs and
//!   document_annotation.rs here, sheet_store (G2), road_document and
//!   road_object_store (G4), project_crs (G5), survey_job (H1), view_set (G1)
//!   and bim_object (G4) later.
//!
//! The C++ default arguments are explicit here; a step name the C++ defaults
//! is one of the `Document::SET_*` constants.

use std::any::Any;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use katana_commands::{
    Command, CommandContext, CommandEvent, CommandPtr, CommandStack, EntityAttributes,
};
use katana_core::error::make_error;
use katana_core::log::{LogField, Logger};
use katana_core::path_text::path_to_utf8;
use katana_core::text_encoding::utf8_or_windows1252;
use katana_core::{Error, ErrorCode, Result, Status};
use katana_entity::tables::DEFAULT_LAYER_NAME;
use katana_entity::{
    ChangeEvent, ChangeKind, ColourTable, Customisation, CustomisationAutomation, EntityId,
    LineStyle, LineworkCodes, Model, StyleLibrary, SurveyMap, validate,
};
use katana_geometry::{SpatialEntry, SpatialIndex};
use katana_storage::{
    ProjectMetadata, ProjectStore, SurveyJob, apply_to_model_consuming, capture_model,
};
use katana_terrain::SurfaceStore;

use crate::annotation::associative::with_associative_update;
use crate::customisation_record::{
    CustomisationSource, RenamedSource, builtin_renames_default, customisation_not_loaded,
    customisation_record_to_save, note_customisation_loaded, record_customisation_load,
};
use crate::customisation_state::{CustomisationOrigin, CustomisationState, customisation_faults};
use crate::drawing::drafting::DraftingSettings;
use crate::selection::SelectionSet;
use crate::spatial_query::query_extents;
use crate::terrain::surface_document::SurfaceState;
use crate::terrain::surface_style::{SurfaceStyleSet, built_in_surface_styles};

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

// ---- what a notification is about --------------------------------------------------------

/// What one Document notification is about.
///
/// A listener that only learns "something changed" must assume everything
/// did: the 3D view rebuilt its whole scene - 217-768 ms on a real TIN
/// archive - for a selection click, and every plan view repainted the drawing
/// for it. So each notification says which parts moved, and a listener
/// rebuilds only what is built from them.
///
/// The parts are bits because one notification can carry several: a command
/// that deletes selected entities changes the entities AND the selection, an
/// import creates layers and styles AND entities. Ask has(mask) with every
/// part the listener depends on rather than comparing `parts` for equality: a
/// listener written for today's parts then keeps working when a later change
/// carries one more alongside. (The C++'s `Part` enumerators are these
/// constants: a mask is combined with `|`, which a Rust enum would not be.)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DocumentChange<'a> {
    pub parts: u32,
    /// The entity-level detail of a command, undo or redo, in the order the
    /// model reported it; empty for anything else. A Replaced drawing does not
    /// list its entities: every one of them changed. Borrowed for the call -
    /// copy what must outlive it.
    pub entities: &'a [ChangeEvent],
}

impl DocumentChange<'_> {
    /// Entities added, modified or removed; `entities` lists which.
    pub const ENTITIES: u32 = 1 << 0;
    /// The selection set. Alone (selection_only()) it is a click: nothing the
    /// drawing or a scene is built from has changed, only what is highlighted.
    pub const SELECTION: u32 = 1 << 1;
    // One bit per model table. A command sets the bit of every table it
    // changed, found by comparing the tables' revisions before and after.
    pub const LAYERS: u32 = 1 << 2;
    pub const STYLES: u32 = 1 << 3;
    pub const LINETYPES: u32 = 1 << 4;
    pub const DIMENSION_STYLES: u32 = 1 << 5;
    pub const HATCH_PATTERNS: u32 = 1 << 6;
    pub const ALIGNMENTS: u32 = 1 << 7;
    pub const PROPERTY_DEFINITIONS: u32 = 1 << 8;
    /// The current layer or style new work is drawn in.
    pub const CURRENT_ATTRIBUTES: u32 = 1 << 9;
    /// The undo history moved: every command executed, undone or redone.
    pub const HISTORY: u32 = 1 << 10;
    /// Saved, or saved somewhere else: the modified flag and the project path
    /// may read differently now.
    pub const SAVED: u32 = 1 << 11;
    /// The project metadata (set_metadata, or the name save_as gives it).
    pub const METADATA: u32 = 1 << 12;
    /// The customisation the drawing is looked at through: the library
    /// linestyles and symbols, and the survey code rules. Not part of the
    /// model; see Document::style_library.
    pub const STYLE_LIBRARY: u32 = 1 << 13;
    pub const SURVEY_MAP: u32 = 1 << 14;
    /// A new or opened drawing: the model, history, selection, current
    /// attributes and metadata are all different. Set together with every one
    /// of those bits, so a listener that tests has(ENTITIES) rebuilds on an
    /// open without having to know this bit exists. Not with the
    /// customisation bits: the customisation is kept across a new or an open.
    pub const REPLACED: u32 = 1 << 15;
    /// The annotation tables: text styles, label styles and auto-label rules
    /// (entity's annotation.rs). One bit for the three, since everything drawn
    /// from one is drawn from the others.
    pub const ANNOTATION_STYLES: u32 = 1 << 16;
    /// The drafting aids (Document::drafting): ortho, polar, snaps, locks.
    /// Session state, not drawing: nothing is redrawn for it, but the toggles
    /// that show it are re-read.
    pub const DRAFTING: u32 = 1 << 17;
    /// The customisation's session state (Document::customisation_state): its
    /// name, origin, sources, colours, linework codes, automation, whether it
    /// is kept, and what the open project is missing. Not in DRAWING (a name
    /// or a switch draws nothing; what does change the drawing is reported
    /// with it: a whole customisation installed also names STYLE_LIBRARY and
    /// SURVEY_MAP, a new colour table STYLE_LIBRARY) and not in a Replaced
    /// drawing, which keeps its customisation.
    ///
    /// set_style_library and set_survey_map report their own part ALONE, as
    /// they always have, although they also mark the origin edited: a listener
    /// that shows the origin tests those two bits as well, or compares
    /// customisation_generation().
    pub const CUSTOMISATION: u32 = 1 << 18;
    /// The drawing's surfaces (Document::surfaces): one added, removed,
    /// edited, renamed, restyled, shown or hidden.
    pub const SURFACES: u32 = 1 << 19;
    /// The surface display styles (Document::surface_styles).
    pub const SURFACE_STYLES: u32 = 1 << 20;
    /// The road model's definitions - road templates, corridors, objects
    /// placed along an alignment (bim_object) - whose 3D objects are made
    /// again from them.
    pub const ROAD_MODEL: u32 = 1 << 21;

    /// Every table bit (the C++ `kTables`).
    pub const TABLES: u32 = Self::LAYERS
        | Self::STYLES
        | Self::LINETYPES
        | Self::DIMENSION_STYLES
        | Self::HATCH_PATTERNS
        | Self::ALIGNMENTS
        | Self::PROPERTY_DEFINITIONS
        | Self::ANNOTATION_STYLES;
    /// What a drawing is DRAWN from - its entities, the tables that say how
    /// they look, the customisation, its surfaces and road model - as against
    /// what is only highlighted (the selection) or only bookkeeping (history,
    /// saved, metadata). The C++ `kDrawing`.
    pub const DRAWING: u32 = Self::ENTITIES
        | Self::TABLES
        | Self::STYLE_LIBRARY
        | Self::SURVEY_MAP
        | Self::SURFACES
        | Self::SURFACE_STYLES
        | Self::ROAD_MODEL;

    /// A change of `parts` and no entities: the C++ `DocumentChange{.parts = parts}`.
    pub const fn of(parts: u32) -> DocumentChange<'static> {
        DocumentChange {
            parts,
            entities: &[],
        }
    }

    pub const fn has(&self, mask: u32) -> bool {
        (self.parts & mask) != 0
    }

    /// Only the selection changed.
    pub const fn selection_only(&self) -> bool {
        self.parts == Self::SELECTION
    }

    /// Something the drawing is drawn from changed (DRAWING).
    pub const fn changes_drawing(&self) -> bool {
        self.has(Self::DRAWING)
    }
}

/// A new or opened drawing: everything but the customisation, which a new or
/// an open keeps (DocumentChange::REPLACED).
const REPLACED_PARTS: u32 = DocumentChange::REPLACED
    | DocumentChange::ENTITIES
    | DocumentChange::SELECTION
    | DocumentChange::TABLES
    | DocumentChange::CURRENT_ATTRIBUTES
    | DocumentChange::HISTORY
    | DocumentChange::SAVED
    | DocumentChange::METADATA
    | DocumentChange::SURFACES
    | DocumentChange::SURFACE_STYLES;

// ---- listeners ---------------------------------------------------------------------------

/// A listener told only that something changed, kept for the callers that
/// redraw whatever happens. A listener that can skip work takes a
/// ChangeListener instead.
pub type Listener = Box<dyn FnMut() + Send>;
/// A listener told what changed.
pub type ChangeListener = Box<dyn FnMut(&DocumentChange<'_>) + Send>;

// Both kinds are kept rather than the plain one wrapped in a ChangeListener,
// which would cost an allocation per call wherever a caller copies it.
enum Callback {
    Plain(Listener),
    Typed(ChangeListener),
}

struct Entry {
    id: u64,
    // Shared with a notification's snapshot, so a listener whose registration
    // ends while it runs is not freed under it.
    call: Arc<Mutex<Callback>>,
}

/// The registrations a Document notifies, in the order they were added.
pub(crate) struct Registry {
    entries: Vec<Entry>,
    next_id: u64,
}

impl Registry {
    fn new() -> Self {
        Self {
            entries: Vec::new(),
            next_id: 1,
        }
    }
}

/// Owns a registration made by add_listener and ends it when dropped.
///
/// A listener captures the object it notifies, so it must not outlive it: a
/// viewport that registered `[this] { update(); }` and was then replaced by a
/// layout change left a dangling call in the document, and the next command
/// after an archive import crashed on it. The handle holds the registry
/// weakly, so a widget that outlives the Document finds nothing to remove and
/// does nothing. Moved, the registration follows the handle; assigned over,
/// the registration the old one held ends.
#[derive(Default)]
#[must_use = "the registration lasts as long as the handle does"]
pub struct ListenerHandle {
    registry: Weak<Mutex<Registry>>,
    id: u64,
}

impl ListenerHandle {
    pub fn reset(&mut self) {
        if self.id != 0
            && let Some(registry) = self.registry.upgrade()
        {
            let id = self.id;
            // Taken out under the lock and let go after it: a listener may own
            // a handle of its own, whose drop comes back here.
            let removed = {
                let mut registry = lock(&registry);
                registry
                    .entries
                    .iter()
                    .position(|entry| entry.id == id)
                    .map(|at| registry.entries.remove(at))
            };
            drop(removed);
        }
        self.id = 0;
        self.registry = Weak::new();
    }

    pub fn active(&self) -> bool {
        self.id != 0 && self.registry.strong_count() > 0
    }
}

impl Drop for ListenerHandle {
    fn drop(&mut self) {
        self.reset();
    }
}

// ---- the state the document's own commands reach ------------------------------------------

/// A cache of a type this file does not know - the C++'s `mutable
/// std::shared_ptr<...Cache>` members, whose structs are defined where they
/// are used (sheet_store, road_document, road_object_store, bim_object) - kept
/// with the state and filled from `&self`.
#[derive(Default)]
pub(crate) struct CacheSlot(Mutex<Option<Box<dyn Any + Send>>>);

impl CacheSlot {
    /// `visit` with the slot's value as a T, made `T::default()` first when
    /// the slot is empty or holds another type.
    pub(crate) fn with<T: Any + Send + Default, R>(&self, visit: impl FnOnce(&mut T) -> R) -> R {
        let mut slot = lock(&self.0);
        match slot
            .as_mut()
            .and_then(|value| (**value).downcast_mut::<T>())
        {
            Some(value) => visit(value),
            None => {
                let mut made = T::default();
                let result = visit(&mut made);
                *slot = Some(Box::new(made));
                result
            }
        }
    }

    /// The C++ `upToDate`: the parsed value of the metadata key `key`, parsed
    /// again only when the key's text differs from what the cache was parsed
    /// from - compared by content, not by a revision, because set_metadata
    /// replaces the whole metadata, keys included, and must be seen.
    #[allow(
        dead_code,
        reason = "Read by the sheet set's and the road model's members (waves G2 and G4)."
    )]
    pub(crate) fn parsed_key<T: Default + Send + Sync + 'static>(
        &self,
        metadata: &ProjectMetadata,
        key: &str,
        parse: impl FnOnce(&str) -> Result<T>,
    ) -> Arc<ParsedKey<T>> {
        self.with(|cache: &mut Option<Arc<ParsedKey<T>>>| {
            let found = metadata.unknown_keys.get(key);
            if let Some(current) = cache.as_ref()
                && current.present == found.is_some()
                && found.is_none_or(|text| current.text == *text)
            {
                return Arc::clone(current);
            }
            let mut made = ParsedKey::<T>::default();
            if let Some(text) = found {
                made.present = true;
                made.text = text.clone();
                match parse(&made.text) {
                    Ok(set) => made.set = Arc::new(set),
                    Err(error) => made.status = Err(error),
                }
            }
            let made = Arc::new(made);
            *cache = Some(Arc::clone(&made));
            made
        })
    }

    /// Puts `value` in the slot as the parsed key's state: what an undo step
    /// does, so undo and redo swap a pointer instead of parsing again.
    #[allow(
        dead_code,
        reason = "Called by the sheet set's and the road model's steps (waves G2 and G4)."
    )]
    pub(crate) fn store<T: Send + Sync + 'static>(&self, value: Arc<ParsedKey<T>>) {
        self.with(|cache: &mut Option<Arc<ParsedKey<T>>>| *cache = Some(value));
    }
}

/// What a metadata key held when it was parsed: whether it was there, its
/// text, the value read from it, and why it could not be read. A state with no
/// key is one too, so undoing the first sheet or template ever made leaves the
/// metadata exactly as it was. The C++ `ParsedKey`, `SheetCache` and
/// `RoadObjectCache`.
#[derive(Debug)]
pub(crate) struct ParsedKey<T> {
    pub(crate) present: bool,
    pub(crate) text: String,
    pub(crate) set: Arc<T>,
    pub(crate) status: Status,
}

impl<T: Default> Default for ParsedKey<T> {
    fn default() -> Self {
        Self {
            present: false,
            text: String::new(),
            set: Arc::new(T::default()),
            status: Ok(()),
        }
    }
}

/// The metadata keys of the road model, whose every change is reported as
/// DocumentChange::ROAD_MODEL with the step that made it (road_model_changed).
/// The storage layer keeps a key it does not read and writes it back
/// unchanged (ProjectMetadata::unknown_keys), which is what lets the road
/// model live in a project without a schema migration.
pub(crate) const ROAD_TEMPLATES_KEY: &str = "road_templates";
pub(crate) const CORRIDORS_KEY: &str = "corridors";
pub(crate) const ROAD_OBJECTS_KEY: &str = "road_objects";
const ROAD_MODEL_KEYS: [&str; 3] = [ROAD_TEMPLATES_KEY, CORRIDORS_KEY, ROAD_OBJECTS_KEY];

/// Everything a step of the document's own may read or change that is not the
/// model (the module's comment says why it is one value). Session data and
/// drawing data alike: the customisation is here because a survey import's
/// finishing step reads the survey map while it runs.
pub(crate) struct DocumentState {
    // ---- the survey customisation (PLAN.MD 20.3) ----
    pub(crate) library: StyleLibrary,
    pub(crate) survey_map: SurveyMap,
    pub(crate) customisation: CustomisationState,
    pub(crate) library_generation: u64,
    pub(crate) survey_map_generation: u64,
    pub(crate) customisation_generation: u64,

    // ---- the project ----
    pub(crate) metadata: ProjectMetadata,
    /// The parsed sheet set and the JSON it was parsed from (sheet_store,
    /// wave G2), shared by an undo step so undo and redo swap a pointer
    /// instead of parsing the set again.
    #[allow(
        dead_code,
        reason = "The sheet set's members are wave G2's (sheet_store)."
    )]
    pub(crate) sheet_cache: CacheSlot,
    /// The parsed road objects and their JSON (road_object_store, wave G4),
    /// shared by the undo steps as the sheet cache is.
    #[allow(
        dead_code,
        reason = "The road objects' members are wave G4's (road_object_store)."
    )]
    pub(crate) road_object_cache: CacheSlot,
    /// The field files imported as jobs, in id order (survey_job, wave H1).
    pub(crate) survey_jobs: Vec<SurveyJob>,
    pub(crate) survey_jobs_generation: u64,

    // ---- the surfaces and their display styles (surface_document.rs) ----
    pub(crate) surfaces: SurfaceStore,
    pub(crate) surfaces_generation: u64,
    pub(crate) surface_styles: SurfaceStyleSet,
    pub(crate) surface_styles_generation: u64,
    pub(crate) surface_styles_status: Status,
    /// The generations the last notification reported (surface_parts_changed).
    pub(crate) surfaces_notified: u64,
    pub(crate) surface_styles_notified: u64,

    // ---- the road model (road_document, wave G4) ----
    /// The parsed road templates and corridors and the JSON each was parsed
    /// from, shared by an undo step as the sheet set's are.
    #[allow(
        dead_code,
        reason = "The road templates' members are wave G4's (road_document)."
    )]
    pub(crate) road_template_cache: CacheSlot,
    #[allow(
        dead_code,
        reason = "The corridors' members are wave G4's (road_document)."
    )]
    pub(crate) corridor_cache: CacheSlot,
    /// The road model's metadata keys as the last command's notification saw
    /// them: what road_model_changed compares, to report ROAD_MODEL.
    pub(crate) road_model_seen: Vec<Option<String>>,
    /// The road model's 3D objects, made once per change of what they are
    /// made from (bim_object's bim_model, wave G4).
    #[allow(dead_code, reason = "The BIM cache is wave G4's (bim_object).")]
    pub(crate) bim_cache: CacheSlot,
}

impl DocumentState {
    /// What a Document starts with: no customisation, an untitled project,
    /// no surfaces and the built-in surface styles.
    fn initial() -> Self {
        Self {
            surface_styles: built_in_surface_styles(),
            ..Self::vacant()
        }
    }

    /// What stands in the Document while its state is lent to a step: cheap
    /// to make, and never read.
    fn vacant() -> Self {
        Self {
            library: StyleLibrary::default(),
            survey_map: SurveyMap::default(),
            customisation: CustomisationState::default(),
            library_generation: 0,
            survey_map_generation: 0,
            customisation_generation: 0,
            metadata: ProjectMetadata::default(),
            sheet_cache: CacheSlot::default(),
            road_object_cache: CacheSlot::default(),
            survey_jobs: Vec::new(),
            survey_jobs_generation: 0,
            surfaces: SurfaceStore::default(),
            surfaces_generation: 0,
            surface_styles: SurfaceStyleSet::default(),
            surface_styles_generation: 0,
            surface_styles_status: Ok(()),
            surfaces_notified: 0,
            surface_styles_notified: 0,
            road_template_cache: CacheSlot::default(),
            corridor_cache: CacheSlot::default(),
            road_model_seen: Vec::new(),
            bim_cache: CacheSlot::default(),
        }
    }
}

/// Where a step of the document's own reaches the DocumentState: the slot the
/// Document lends its state to for the length of one execute, undo or redo.
/// Captured by every such command; empty at any other time.
#[derive(Clone, Default)]
pub(crate) struct StateLink(Arc<Mutex<Option<DocumentState>>>);

impl StateLink {
    /// `change` with the state, while it is lent. InvalidState, and nothing
    /// done, when it is not: the command is being run somewhere other than
    /// through the Document that made it. Never call it from inside `change`.
    pub(crate) fn with<R>(&self, change: impl FnOnce(&mut DocumentState) -> R) -> Result<R> {
        let mut lent = lock(&self.0);
        match lent.as_mut() {
            Some(state) => Ok(change(state)),
            None => Err(Error::new(
                ErrorCode::InvalidState,
                "a step that changes the drawing's own state runs only through the execute, \
                 undo and redo of the Document that made it",
            )),
        }
    }
}

/// The state lent for one step, and put back however the step ends.
struct Lent<'a> {
    home: &'a mut DocumentState,
    link: &'a StateLink,
}

impl<'a> Lent<'a> {
    fn new(home: &'a mut DocumentState, link: &'a StateLink) -> Self {
        *lock(&link.0) = Some(std::mem::replace(home, DocumentState::vacant()));
        Self { home, link }
    }
}

impl Drop for Lent<'_> {
    fn drop(&mut self) {
        let state = lock(&self.link.0).take();
        if let Some(state) = state {
            *self.home = state;
        }
    }
}

/// What a command of the document's own does to its state: called with true
/// to install the state after the change, false the state before.
pub(crate) type StateApply = Box<dyn FnMut(&mut DocumentState, bool) + Send>;

/// One change of the document's own state, applied by the document - the
/// C++'s AnnotationScaleCommand, SurfaceStateCommand, SheetSetCommand,
/// RoadModelCommand and ProjectValueCommand, which were one shape: the state
/// before and after, put back by undo and redo.
pub(crate) struct StateCommand {
    name: String,
    link: StateLink,
    apply: StateApply,
}

impl Command for StateCommand {
    fn name(&self) -> &str {
        &self.name
    }

    fn validate(&self, _context: &CommandContext<'_>) -> Status {
        Ok(())
    }

    fn execute(&mut self, _context: &mut CommandContext<'_>) -> Status {
        self.link.with(|state| (self.apply)(state, true))
    }

    fn undo(&mut self, _context: &mut CommandContext<'_>) -> Status {
        self.link.with(|state| (self.apply)(state, false))
    }

    fn redo(&mut self, _context: &mut CommandContext<'_>) -> Status {
        self.link.with(|state| (self.apply)(state, true))
    }
}

/// Each table's revision when tables_changed() last looked.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TableRevisions {
    pub(crate) layers: u64,
    pub(crate) styles: u64,
    pub(crate) linetypes: u64,
    pub(crate) dimension_styles: u64,
    pub(crate) hatch_patterns: u64,
    pub(crate) alignments: u64,
    pub(crate) properties: u64,
    pub(crate) text_styles: u64,
    pub(crate) label_styles: u64,
    pub(crate) label_rules: u64,
}

/// What the stack published during one step: its one listener keeps it here,
/// and the Document reads it when the stack returns.
pub(crate) type Published = Arc<Mutex<Option<CommandEvent>>>;

fn new_stack(model: &mut Model, logger: Option<Arc<Logger>>) -> (CommandStack, Published) {
    let published: Published = Arc::default();
    let mut stack = CommandStack::new(model, logger);
    let sink = Arc::clone(&published);
    stack.add_listener(move |event: &CommandEvent| {
        *lock(&sink) = Some(event.clone());
    });
    (stack, published)
}

/// The parts of a document a caller reads and changes at once: the model and
/// its index beside a mutable selection and drafting settings, which Rust's
/// borrows give only through one call.
pub struct DocumentParts<'a> {
    pub model: &'a Model,
    pub spatial_index: &'a SpatialIndex,
    pub selection: &'a mut SelectionSet,
    pub drafting: &'a mut DraftingSettings,
}

// ---- the Document --------------------------------------------------------------------------

pub struct Document {
    pub(crate) logger: Option<Arc<Logger>>,
    pub(crate) model: Model,
    pub(crate) stack: CommandStack,
    pub(crate) published: Published,
    /// At home between steps; lent to `lender` for each.
    pub(crate) state: DocumentState,
    pub(crate) lender: StateLink,
    pub(crate) model_revision: u64,
    pub(crate) index: SpatialIndex,
    /// How many entities the model held when the index last chose its cell
    /// size (its last rebuild). 0 until the first rebuild: an index that has
    /// never been rebuilt is on the default cell, which was chosen for no
    /// data.
    pub(crate) index_chosen_for: usize,
    /// How many boxes that rebuild had to put on the oversized list: some data
    /// needs a few whatever the cell (apply_to_spatial_index, the overflow
    /// rule).
    pub(crate) index_oversized_at_rebuild: usize,
    pub(crate) tables_seen: TableRevisions,
    pub(crate) store: Option<ProjectStore>,
    pub(crate) selection: SelectionSet,
    pub(crate) drafting: DraftingSettings,
    pub(crate) current_layer: String,
    pub(crate) current_style: String,
    pub(crate) listeners: Arc<Mutex<Registry>>,
    pub(crate) metadata_modified: bool,
}

impl Default for Document {
    fn default() -> Self {
        Self::new(None)
    }
}

impl Document {
    /// The step names the C++ gives by default.
    pub const SET_CRS: &str = "SET_CRS";
    pub const SET_SHEETS: &str = "SET_SHEETS";
    pub const SET_SURFACES: &str = "SET_SURFACES";
    pub const SET_SURFACE_STYLES: &str = "SET_SURFACE_STYLES";
    pub const SET_ROAD_TEMPLATES: &str = "SET_ROAD_TEMPLATES";
    pub const SET_CORRIDORS: &str = "SET_CORRIDORS";
    pub const SET_ROAD_OBJECTS: &str = "SET_ROAD_OBJECTS";

    pub fn new(logger: Option<Arc<Logger>>) -> Self {
        let mut model = Model::default();
        let (stack, published) = new_stack(&mut model, logger.clone());
        let mut document = Self {
            logger,
            model,
            stack,
            published,
            state: DocumentState::initial(),
            lender: StateLink::default(),
            model_revision: 0,
            index: SpatialIndex::default(),
            index_chosen_for: 0,
            index_oversized_at_rebuild: 0,
            tables_seen: TableRevisions::default(),
            store: None,
            selection: SelectionSet::default(),
            drafting: DraftingSettings::default(),
            current_layer: String::from(DEFAULT_LAYER_NAME),
            current_style: String::new(),
            listeners: Arc::new(Mutex::new(Registry::new())),
            metadata_modified: false,
        };
        // The tables' revisions start wherever constructing them left them;
        // the first command must not report every table as changed.
        document.remember_tables();
        document
    }

    pub fn model(&self) -> &Model {
        &self.model
    }

    // ---- the survey customisation (PLAN.MD 20.3) ----------------------------------------
    //
    // The library linestyle and symbol definitions this drawing is drawn with,
    // and the survey code rules that say what a survey code becomes. They are
    // NOT part of the model and are not saved inside the project: a
    // customisation is a site-wide thing a project NAMES rather than copies,
    // the usual arrangement, and it keeps 35,000 strokes out of every project
    // file. See docs/survey_coding.md.
    //
    // They are not undoable either, and deliberately: loading a library is not
    // an edit to the drawing, it is a change to what the drawing is looked at
    // through. Undoing a line should not silently unload a library.

    pub fn style_library(&self) -> &StyleLibrary {
        &self.state.library
    }

    pub fn survey_map(&self) -> &SurveyMap {
        &self.state.survey_map
    }

    /// What an editor calls with the library it changed: an EDIT of the
    /// session's customisation, whose origin becomes Edited and which is no
    /// longer kept (customisation_state). A whole customisation is installed
    /// with install_customisation instead, which says where it came from.
    pub fn set_style_library(&mut self, library: StyleLibrary) {
        self.state.library = library;
        self.state.library_generation += 1;
        self.mark_customisation_edited();
        self.notify_parts(DocumentChange::STYLE_LIBRARY);
    }

    /// As set_style_library, for the survey map.
    pub fn set_survey_map(&mut self, map: SurveyMap) {
        self.state.survey_map = map;
        self.state.survey_map_generation += 1;
        self.mark_customisation_edited();
        self.notify_parts(DocumentChange::SURVEY_MAP);
    }

    /// Counters bumped by every change of the library or of the survey map. A
    /// cache of flattened definitions, thumbnails or code lookups keys on
    /// these, never on a definition's address (it moves when the whole library
    /// or map is replaced) and never on "a listener fired" (which a selection
    /// click also does). A change of the colour table bumps library_generation
    /// too: a sprite or a thumbnail of a definition bakes in the colours its
    /// pens resolved to, and is dropped only on this.
    pub fn library_generation(&self) -> u64 {
        self.state.library_generation
    }

    pub fn survey_map_generation(&self) -> u64 {
        self.state.survey_map_generation
    }

    /// The definition a style's `symbol` or `linetype` names, or None. One
    /// place to ask, so that "which library does this name come from" is not a
    /// question every caller answers for itself.
    pub fn definition_for(&self, name: &str) -> Option<&LineStyle> {
        if name.is_empty() {
            return None;
        }
        self.state.library.find(name)
    }

    // ---- the customisation's session state (customisation_state.rs) ------------------
    //
    // Everything else the session knows of its customisation: its name and
    // where it came from, the sources that went into it, its colours, linework
    // codes and automation switches, whether it is kept, and which names the
    // open project recorded that are not loaded. Session data, as the library
    // and the map are: not undoable, not in the project, kept across a new or
    // an opened drawing. A default Document has none of it (an empty state,
    // origin None), and nothing here installs a customisation unasked: a front
    // end does, with start_customisation (customisation_host).

    pub fn customisation_state(&self) -> &CustomisationState {
        &self.state.customisation
    }

    /// Counts every change of the state above, the raw setters' included (they
    /// mark the origin edited). What a panel showing the state compares.
    pub fn customisation_generation(&self) -> u64 {
        self.state.customisation_generation
    }

    /// Installs a WHOLE customisation in the place of the session's: library,
    /// map, colours, name, description, notice, based_on, sources - a
    /// customisation that lists no sources is its own one source - linework
    /// codes and automation, with `origin` as where it came from and `kept` as
    /// whether it is what the next start would give (the C++ defaults it to
    /// false). Linework codes or automation it does not say are the DEFAULTS,
    /// not the session's: the session it leaves is the one a start with it
    /// would give, whatever was there before - a reset to the built-in resets
    /// the control codes too. (Keeping the session's where a load says nothing
    /// is the merge's rule, and merge_customisation hands over a customisation
    /// that says both.) What the open project is missing loses the names the
    /// new sources bring. One notification: STYLE_LIBRARY | SURVEY_MAP |
    /// CUSTOMISATION.
    ///
    /// InvalidArgument, and nothing changes, for origin None and for whatever
    /// customisation_faults finds - a name, a source's name, a definition's
    /// source, linework codes or a based_on: the session is what a KEEP writes
    /// and a project records, and neither could take them.
    pub fn install_customisation(
        &mut self,
        customisation: Customisation,
        origin: CustomisationOrigin,
        kept: bool,
    ) -> Status {
        // Everything is checked before anything is taken: a refusal leaves
        // the session exactly as it was.
        if origin == CustomisationOrigin::None {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "a customisation is installed with where it came from, which is never \"none\"",
            ));
        }
        let faults = customisation_faults(&customisation);
        if let Some(first) = faults.first() {
            // The first is enough to refuse on; a load that wants them all asks
            // for them all (merge_customisation).
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!(
                    "the customisation could not be kept or recorded: {}",
                    first.message
                ),
                first.context.clone(),
            ));
        }

        let Customisation {
            name,
            description,
            notice,
            sources,
            based_on,
            colours,
            linework,
            automation,
            library,
            map,
        } = customisation;
        let state = &mut self.state;
        state.library = library;
        state.survey_map = map;
        let session = &mut state.customisation;
        session.origin = origin;
        session.kept = kept;
        session.description = description;
        session.notice = notice;
        session.based_on = based_on;
        session.colours = colours;
        // Said, or the DEFAULTS - never what the session had. An install takes
        // the place of the whole session, and `kept` promises it is the
        // session the next start would give: that start has no earlier session
        // to have left its control codes behind. Left alone, a reset to the
        // built-in kept the spellings and the switches it was meant to reset.
        // (A load that says nothing leaves the session's alone through
        // merge_customisation, which hands over a customisation that says
        // both.)
        session.linework = linework.unwrap_or_default();
        session.automation = automation.unwrap_or_default();
        session.sources = sources;
        if session.sources.is_empty() {
            // A customisation that lists none is its own one source. Its notice
            // stays the customisation's, above; a source's own notice is for
            // one merged into another's.
            session.sources.push(CustomisationSource {
                name: name.clone(),
                definitions: !state.library.empty(),
                rules: !state.survey_map.empty(),
                notice: Vec::new(),
            });
        }
        session.name = name;
        note_customisation_loaded(
            &mut session.missing_at_open,
            &session.sources,
            &builtin_renames_default(),
        );
        state.library_generation += 1;
        state.survey_map_generation += 1;
        state.customisation_generation += 1;
        self.notify_parts(
            DocumentChange::STYLE_LIBRARY
                | DocumentChange::SURVEY_MAP
                | DocumentChange::CUSTOMISATION,
        );
        Ok(())
    }

    /// The session as one customisation - what an export or a KEEP writes, and
    /// what a merge starts from: the library, the map and the state above.
    /// Linework and automation are always said. install_customisation of it
    /// gives the session back.
    pub fn customisation(&self) -> Customisation {
        let session = &self.state.customisation;
        Customisation {
            name: session.name.clone(),
            description: session.description.clone(),
            notice: session.notice.clone(),
            sources: session.sources.clone(),
            based_on: session.based_on.clone(),
            colours: session.colours.clone(),
            linework: Some(session.linework.clone()),
            automation: Some(session.automation),
            library: self.state.library.clone(),
            map: self.state.survey_map.clone(),
        }
    }

    /// The colour table, as an edit of the session (origin Edited, no longer
    /// kept). Setting the value it already has is nothing at all. A colour
    /// table also bumps library_generation and reports STYLE_LIBRARY with
    /// CUSTOMISATION.
    pub fn set_colour_table(&mut self, colours: ColourTable) {
        let state = &mut self.state;
        if colours == state.customisation.colours {
            return;
        }
        state.customisation.colours = colours;
        state.customisation.origin = CustomisationOrigin::Edited;
        state.customisation.kept = false;
        state.customisation_generation += 1;
        // What a pen inside a definition resolves to has changed, so whatever
        // was baked from the library is stale.
        state.library_generation += 1;
        self.notify_parts(DocumentChange::CUSTOMISATION | DocumentChange::STYLE_LIBRARY);
    }

    /// The linework control codes, as an edit of the session. InvalidArgument
    /// for what entity's validate refuses.
    pub fn set_linework_codes(&mut self, codes: LineworkCodes) -> Status {
        validate(&codes)?;
        let state = &mut self.state;
        if codes == state.customisation.linework {
            return Ok(());
        }
        state.customisation.linework = codes;
        state.customisation.origin = CustomisationOrigin::Edited;
        state.customisation.kept = false;
        state.customisation_generation += 1;
        self.notify_parts(DocumentChange::CUSTOMISATION);
        Ok(())
    }

    /// The automation switches, as an edit of the session.
    pub fn set_automation(&mut self, automation: CustomisationAutomation) {
        let state = &mut self.state;
        if automation == state.customisation.automation {
            return;
        }
        state.customisation.automation = automation;
        state.customisation.origin = CustomisationOrigin::Edited;
        state.customisation.kept = false;
        state.customisation_generation += 1;
        self.notify_parts(DocumentChange::CUSTOMISATION);
    }

    /// Whether the session is what the next start would give: true after it
    /// was written to the kept file, false once it differs again.
    pub fn set_customisation_kept(&mut self, kept: bool) {
        if kept == self.state.customisation.kept {
            return;
        }
        self.state.customisation.kept = kept;
        self.state.customisation_generation += 1;
        self.notify_parts(DocumentChange::CUSTOMISATION);
    }

    /// The name the host's built-in customisation declares (empty: none),
    /// which is what CUSTOMISE BUILTIN marks as the default.
    /// start_customisation sets it.
    pub fn set_built_in_customisation_name(&mut self, name: String) {
        if name == self.state.customisation.built_in {
            return;
        }
        self.state.customisation.built_in = name;
        self.state.customisation_generation += 1;
        self.notify_parts(DocumentChange::CUSTOMISATION);
    }

    /// What a start found: what went wrong, a sentence each, and whether the
    /// kept customisation was made from another built-in than the host has
    /// (CustomisationState::start_problems, kept_from_another_built_in).
    /// start_customisation sets it, so that a report asked for later still has
    /// what was said once at the start. It is no edit of the session: origin
    /// and `kept` stand, and the value it already has is nothing at all.
    pub fn set_customisation_start(
        &mut self,
        problems: Vec<String>,
        kept_from_another_built_in: bool,
    ) {
        let session = &mut self.state.customisation;
        if problems == session.start_problems
            && kept_from_another_built_in == session.kept_from_another_built_in
        {
            return;
        }
        session.start_problems = problems;
        session.kept_from_another_built_in = kept_from_another_built_in;
        self.state.customisation_generation += 1;
        self.notify_parts(DocumentChange::CUSTOMISATION);
    }

    /// For a load installed through the raw setters - a front end that read
    /// the files itself: adds its sources to the session's
    /// (record_customisation_load) and takes them off what the open project
    /// is missing (note_customisation_loaded). The origin becomes Loaded.
    /// install_customisation needs no such call; it takes the sources with
    /// the customisation.
    pub fn record_customisation_load(
        &mut self,
        load: &[CustomisationSource],
        replaced_definitions: bool,
        replaced_rules: bool,
    ) {
        let session = &mut self.state.customisation;
        record_customisation_load(
            &mut session.sources,
            load,
            replaced_definitions,
            replaced_rules,
        );
        note_customisation_loaded(
            &mut session.missing_at_open,
            load,
            &builtin_renames_default(),
        );
        session.origin = CustomisationOrigin::Loaded;
        session.kept = false;
        self.state.customisation_generation += 1;
        self.notify_parts(DocumentChange::CUSTOMISATION);
    }

    /// The origin is Edited and the session no longer kept; counted only when
    /// that changes something.
    pub(crate) fn mark_customisation_edited(&mut self) {
        let session = &mut self.state.customisation;
        if session.origin == CustomisationOrigin::Edited && !session.kept {
            return;
        }
        session.origin = CustomisationOrigin::Edited;
        session.kept = false;
        self.state.customisation_generation += 1;
    }

    /// The record's renames: the early file names, answered by the NSW
    /// customisation, whichever built-in the host has - the early file names
    /// were its files (customisation_record.rs).
    pub(crate) fn customisation_renames(&self) -> Vec<RenamedSource> {
        builtin_renames_default()
    }

    // ---- the index and the history ------------------------------------------------------

    /// Broad-phase index over the entities, kept in step with the model
    /// (PLAN.MD Phase 18). Maintained incrementally from the per-entity
    /// changes an ordinary command reports, and rebuilt whole - which is what
    /// chooses the cell size from the data - when the model is replaced, when
    /// the drawing has doubled or halved since the cell was chosen, and when
    /// boxes too big for the cell pile up (apply_to_spatial_index has the
    /// rules), so that an import leaves the same index as opening the saved
    /// result would.
    ///
    /// Pass it to snap(), pick_entity() and pick_in_box(). They give the same
    /// answer without it, only slower, so a caller that has no Document loses
    /// nothing but speed.
    pub fn spatial_index(&self) -> &SpatialIndex {
        &self.index
    }

    pub fn history(&self) -> &CommandStack {
        &self.stack
    }

    /// Counts up by one for every command executed, undone or redone, and for
    /// every new or opened drawing; never down, and never for anything else -
    /// a selection, the current layer or style, a save, the metadata, a
    /// library or a survey map (those two have their own generations). "Has
    /// the drawing changed since I last looked" is then one comparison, where
    /// the history's counts could not tell "undo, then a new command" from
    /// nothing, nor a reopened project from the drawing it replaced.
    pub fn model_revision(&self) -> u64 {
        self.model_revision
    }

    // ---- editing --------------------------------------------------------------------------

    pub fn execute(&mut self, command: CommandPtr) -> Status {
        // Every command carries the associative update with it: the
        // dimensions, leaders and labels that follow what it changed are
        // moved in the SAME undo step (annotation/associative.rs), so one
        // undo puts back the geometry and everything that followed it.
        let command = with_associative_update(command);
        self.step(|stack, model| stack.execute(model, command))
    }

    pub fn undo(&mut self) -> Status {
        self.step(|stack, model| stack.undo(model))
    }

    pub fn redo(&mut self) -> Status {
        self.step(|stack, model| stack.redo(model))
    }

    /// Entities created by the most recent execute() / redo().
    pub fn last_created_entities(&self) -> Vec<EntityId> {
        self.stack.last_created_entities()
    }

    /// One step of the stack, with the state lent for its length; then what
    /// the C++'s stack listener did, when the stack published the step.
    fn step(&mut self, run: impl FnOnce(&mut CommandStack, &mut Model) -> Status) -> Status {
        let status = {
            let _lent = Lent::new(&mut self.state, &self.lender);
            run(&mut self.stack, &mut self.model)
        };
        // The stack publishes only a step that succeeded, so a refused
        // command moves nothing.
        let published = lock(&self.published).take();
        if let Some(event) = published {
            self.after_step(&event);
        }
        status
    }

    fn after_step(&mut self, event: &CommandEvent) {
        self.apply_to_spatial_index(&event.changes);
        // The surfaces and their styles are not tables of the model: a step
        // that changed them says so by their generations.
        let mut parts =
            DocumentChange::HISTORY | self.tables_changed() | self.surface_parts_changed();
        // The road model's definitions.
        parts |= self.road_model_changed();
        if !event.changes.is_empty() {
            parts |= DocumentChange::ENTITIES;
        }
        // Undo can delete selected entities and the current layer.
        if self.prune_selection() {
            parts |= DocumentChange::SELECTION;
        }
        if !self.model.layers.contains(&self.current_layer) {
            self.current_layer = String::from(DEFAULT_LAYER_NAME);
            parts |= DocumentChange::CURRENT_ATTRIBUTES;
        }
        // And the current style: a delete, merge or rename of it - or the
        // undo of its creation - must not leave new work naming nothing.
        if !self.current_style.is_empty() && !self.model.styles.contains(&self.current_style) {
            self.current_style.clear();
            parts |= DocumentChange::CURRENT_ATTRIBUTES;
        }
        self.model_revision += 1;
        self.notify(&DocumentChange {
            parts,
            entities: &event.changes,
        });
    }

    /// The DocumentChange table bits of every table whose revision moved since
    /// the last call (or remember_tables), and remembers the new ones.
    pub(crate) fn tables_changed(&mut self) -> u32 {
        let model = &self.model;
        let now = TableRevisions {
            layers: model.layers.revision(),
            styles: model.styles.revision(),
            linetypes: model.linetypes.revision(),
            dimension_styles: model.dimension_styles.revision(),
            hatch_patterns: model.hatch_patterns.revision(),
            alignments: model.alignments.revision(),
            properties: model.properties.revision(),
            text_styles: model.text_styles.revision(),
            label_styles: model.label_styles.revision(),
            label_rules: model.label_rules.revision(),
        };
        let seen = self.tables_seen;
        let mut parts = 0;
        let mut compare = |before: u64, after: u64, part: u32| {
            if before != after {
                parts |= part;
            }
        };
        compare(seen.layers, now.layers, DocumentChange::LAYERS);
        compare(seen.styles, now.styles, DocumentChange::STYLES);
        compare(seen.linetypes, now.linetypes, DocumentChange::LINETYPES);
        compare(
            seen.dimension_styles,
            now.dimension_styles,
            DocumentChange::DIMENSION_STYLES,
        );
        compare(
            seen.hatch_patterns,
            now.hatch_patterns,
            DocumentChange::HATCH_PATTERNS,
        );
        compare(seen.alignments, now.alignments, DocumentChange::ALIGNMENTS);
        compare(
            seen.properties,
            now.properties,
            DocumentChange::PROPERTY_DEFINITIONS,
        );
        compare(
            seen.text_styles,
            now.text_styles,
            DocumentChange::ANNOTATION_STYLES,
        );
        compare(
            seen.label_styles,
            now.label_styles,
            DocumentChange::ANNOTATION_STYLES,
        );
        compare(
            seen.label_rules,
            now.label_rules,
            DocumentChange::ANNOTATION_STYLES,
        );
        self.tables_seen = now;
        parts
    }

    pub(crate) fn remember_tables(&mut self) {
        let _ = self.tables_changed();
    }

    /// Whole index from the current model; picks the cell size from the data.
    pub(crate) fn rebuild_spatial_index(&mut self) {
        let mut entries = Vec::with_capacity(self.model.entities.size());
        let model = &self.model;
        model.entities.for_each(|entity| {
            entries.push(SpatialEntry {
                id: entity.id,
                box_value: query_extents(model, entity),
            });
        });
        self.index.rebuild(&entries);
        self.index_chosen_for = self.model.entities.size();
        self.index_oversized_at_rebuild = self.index.oversized_count();
    }

    // WHEN THE INDEX IS REBUILT RATHER THAN UPDATED.
    //
    // Incrementally, the index is only ever as good as the cell size it was
    // last given, and the cell size is chosen by a rebuild alone. An index that
    // has never been rebuilt keeps the default one-unit cell, so a 12 km survey
    // imported into a new drawing was filed into 722,715 buckets with 4,889
    // boxes on the oversized list every query scans: the import's execute took
    // about a second against 28 ms for the create itself, and every snap, pick
    // and repaint was 36x slower until the project was reopened - opening being
    // the one thing that rebuilt it.
    //
    // So the index is rebuilt, re-choosing the cell, when
    //   * THE SIZE RULE: the drawing has grown to twice, or shrunk to half, the
    //     number of entities the cell was chosen for. An empty drawing's cell
    //     was chosen for none, so the first command into it - an import -
    //     always rebuilds, and without first filing everything into the
    //     default cell.
    //   * THE OVERFLOW RULE: after a command is applied incrementally, the
    //     oversized list holds more than twice what the last rebuild left there
    //     plus a hundredth of the drawing. Boxes too big for the cell are what
    //     make every query slow, and they come from a command that brings in or
    //     scales up geometry the cell was not chosen for.
    // NOT when a command merely touches many entities. That was the first rule
    // tried (a tenth of the drawing), and measured on the 27,886-entity
    // corridor stand-in (BM_MoveATenthOfAnImport*, bench_cad.cpp) it made a
    // MOVE of a tenth, with its undo, about 4x slower than the same move one
    // entity short of the threshold (101 against 24 ms, min of 9) - two
    // rebuilds of the whole drawing for a move that changes no box's size and
    // so no cell choice.
    //
    // Neither rule fires on an ordinary click on a drawing of any size, which
    // stays an O(1) update. A rebuild costs O(n), and each rule fires only
    // after Theta(n) entities were added, removed or pushed onto the oversized
    // list since the last one, so a sequence of commands costs a constant
    // factor over the edits in it, never O(n) per edit.

    /// The index after one command: incremental for an ordinary edit, a whole
    /// rebuild when the drawing has doubled or halved since the cell was
    /// chosen, or when boxes too big for the cell have piled up.
    pub(crate) fn apply_to_spatial_index(&mut self, changes: &[ChangeEvent]) {
        if changes.is_empty() {
            return; // a table-only command: nothing the index holds moved
        }
        let entities = self.model.entities.size();
        let resized =
            entities >= 2 * self.index_chosen_for || 2 * entities <= self.index_chosen_for;
        // After a Cleared the data is about to be different altogether.
        let cleared = changes
            .iter()
            .any(|change| change.kind == ChangeKind::Cleared);
        if resized || cleared {
            self.rebuild_spatial_index();
            return;
        }
        for change in changes {
            match change.kind {
                ChangeKind::Cleared => {} // handled above, before any incremental update
                ChangeKind::EntityRemoved => {
                    self.index.remove(change.id);
                }
                ChangeKind::EntityAdded | ChangeKind::EntityModified => {
                    match self.model.entities.find(change.id) {
                        // Added then removed within one command. insert()
                        // would have nothing to index; remove() keeps the index
                        // from holding an id the model no longer has.
                        None => {
                            self.index.remove(change.id);
                        }
                        // insert() replaces an existing entry, so Modified
                        // needs no separate remove.
                        Some(entity) => {
                            self.index
                                .insert(entity.id, query_extents(&self.model, entity));
                        }
                    }
                }
            }
        }
        // The overflow rule, above.
        if self.index.oversized_count() > 2 * self.index_oversized_at_rebuild + entities / 100 {
            self.rebuild_spatial_index();
        }
    }

    /// True when it removed anything from the selection.
    pub(crate) fn prune_selection(&mut self) -> bool {
        self.selection.prune(&self.model.entities)
    }

    /// The road model's keys as they are, against what the last command's
    /// notification saw: ROAD_MODEL when any moved. It compares three
    /// metadata keys and nothing else, so it is here rather than with the road
    /// model's other members (road_document, wave G4).
    pub(crate) fn road_model_changed(&mut self) -> u32 {
        let state = &mut self.state;
        state.road_model_seen.resize(ROAD_MODEL_KEYS.len(), None);
        let mut changed = false;
        for (key, seen) in ROAD_MODEL_KEYS.iter().zip(state.road_model_seen.iter_mut()) {
            match state.metadata.unknown_keys.get(*key) {
                None => {
                    if seen.is_some() {
                        *seen = None;
                        changed = true;
                    }
                }
                Some(text) => {
                    if seen.as_ref() != Some(text) {
                        *seen = Some(text.clone());
                        changed = true;
                    }
                }
            }
        }
        if changed {
            DocumentChange::ROAD_MODEL
        } else {
            0
        }
    }

    // ---- session state ----------------------------------------------------------------------

    pub fn selection(&self) -> &SelectionSet {
        &self.selection
    }

    pub fn selection_mut(&mut self) -> &mut SelectionSet {
        &mut self.selection
    }

    /// Call after changing the selection so views refresh.
    pub fn notify_selection_changed(&self) {
        self.notify_parts(DocumentChange::SELECTION);
    }

    /// The drafting aids - ortho, polar, object snaps, angle convention, locks
    /// (drawing/drafting.rs): session state shared by every view, the command
    /// line and the verbs an agent drives them with. Not part of the drawing,
    /// so not saved and not undone.
    pub fn drafting(&self) -> &DraftingSettings {
        &self.drafting
    }

    pub fn drafting_mut(&mut self) -> &mut DraftingSettings {
        &mut self.drafting
    }

    /// Call after changing the drafting settings so their toggles refresh.
    pub fn notify_drafting_changed(&self) {
        self.notify_parts(DocumentChange::DRAFTING);
    }

    /// The model and the index beside a mutable selection and drafting
    /// settings, for a caller that picks into the selection or constrains
    /// against the settings in one go.
    pub fn parts_mut(&mut self) -> DocumentParts<'_> {
        DocumentParts {
            model: &self.model,
            spatial_index: &self.index,
            selection: &mut self.selection,
            drafting: &mut self.drafting,
        }
    }

    pub fn current_layer(&self) -> &str {
        &self.current_layer
    }

    pub fn set_current_layer(&mut self, name: &str) -> Status {
        let Some(layer) = self.model.layers.find(name) else {
            return Err(make_error(
                ErrorCode::NotFound,
                "layer does not exist",
                name,
            ));
        };
        if layer.locked {
            return Err(make_error(
                ErrorCode::CommandRejected,
                "a locked layer cannot be the current layer",
                name,
            ));
        }
        self.current_layer = String::from(name);
        self.notify_parts(DocumentChange::CURRENT_ATTRIBUTES);
        Ok(())
    }

    /// The style new work is drawn in - AutoCAD's CELTYPE, or a current point
    /// style - so a symbol or a library linestyle can be drawn with, not only
    /// applied afterwards. Empty means ByLayer, and is the default.
    ///
    /// Refused (NotFound) for a style the model does not have; "" clears it.
    /// Kept consistent the way the current layer is: after any command, undo
    /// or redo that leaves it naming no style - a delete, a merge, a rename -
    /// it is CLEARED, not followed. Following a rename would need the command
    /// to say what it renamed to, which no command event carries, and a guess
    /// could put new work in the wrong style; ByLayer never can.
    pub fn current_style(&self) -> &str {
        &self.current_style
    }

    pub fn set_current_style(&mut self, name: &str) -> Status {
        if !name.is_empty() && !self.model.styles.contains(name) {
            return Err(make_error(
                ErrorCode::NotFound,
                "style does not exist",
                name,
            ));
        }
        if name != self.current_style {
            self.current_style = String::from(name);
            self.notify_parts(DocumentChange::CURRENT_ATTRIBUTES);
        }
        Ok(())
    }

    /// Attributes for newly drawn entities: the current layer and style, and
    /// ByLayer colour.
    pub fn current_attributes(&self) -> EntityAttributes {
        EntityAttributes {
            layer: self.current_layer.clone(),
            style: self.current_style.clone(),
            color: None,
        }
    }

    // ---- persistence ----------------------------------------------------------------------
    //
    // The customisation is not saved, but a RECORD of it is: the names the
    // drawing is drawn with (storage's ProjectMetadata::customisation;
    // customisation_record.rs has the rules). The Document keeps that record
    // itself. A save writes it INTO WHAT IT SAVES, and metadata() reads it
    // back only once the save has succeeded: it never goes through
    // set_metadata, so a save that cannot go ahead leaves an untouched drawing
    // unmodified. open() works out which recorded names this session lacks
    // (customisation_state().missing_at_open), for the front end to warn of
    // and for the next save to keep; new_document() has none.

    /// Discards the drawing and starts an empty, unsaved one.
    pub fn new_document(&mut self) {
        self.store = None;
        self.model.reset();
        self.rebuild_stack(); // fresh history; also re-installs the entity observer
        self.rebuild_spatial_index();
        self.state.metadata = ProjectMetadata::default();
        self.metadata_modified = false;
        self.state.survey_jobs.clear();
        self.state.survey_jobs_generation += 1;
        self.install_surface_state(SurfaceState::default());
        let _ = self.surface_parts_changed(); // reported below, as part of the replaced drawing
        self.selection.clear();
        self.current_layer = String::from(DEFAULT_LAYER_NAME);
        self.current_style.clear();
        self.model_revision += 1;
        self.remember_tables();
        // The customisation is kept; what the LAST project lacked of its own
        // record is not: a new drawing recorded nothing. Left standing, a bare
        // CUSTOMISE would name the old project's files.
        let had_missing = !self.state.customisation.missing_at_open.is_empty();
        if had_missing {
            self.state.customisation.missing_at_open.clear();
            self.state.customisation_generation += 1;
        }
        self.notify_parts(
            REPLACED_PARTS
                | if had_missing {
                    DocumentChange::CUSTOMISATION
                } else {
                    0
                },
        );
    }

    pub fn open(&mut self, project_directory: &Path) -> Status {
        let store = ProjectStore::open(project_directory)?;
        let mut contents = store.load()?;
        // Taken out BEFORE the contents are consumed below, and installed only
        // once the apply has succeeded - a failed open must leave this document
        // exactly as it was, metadata included.
        let mut metadata = std::mem::take(&mut contents.metadata);
        // The jobs likewise: applying the contents does not read them, and
        // they are the largest thing in a project that has any (the raw field
        // files).
        let survey_jobs = std::mem::take(&mut contents.survey_jobs);
        // And the surfaces, decoded and checked now: one that cannot be read
        // refuses the open before anything is installed, rather than open a
        // drawing whose next save would leave the surface out.
        let surface_state = Self::read_surface_state(&mut contents.surfaces, &mut metadata)?;
        // Applying validates before it touches the model, so a bad project
        // leaves the current drawing as it was. The contents are consumed:
        // they are a local that dies either way, and copying every entity into
        // the model was the single largest allocator of the whole open.
        apply_to_model_consuming(contents, &mut self.model)?;
        self.store = Some(store);
        self.state.metadata = metadata;
        self.metadata_modified = false;
        self.state.survey_jobs = survey_jobs;
        self.state.survey_jobs_generation += 1;
        if let Some(logger) = &self.logger {
            for note in &surface_state.notes {
                logger.warning("storage", note, &[]);
            }
        }
        self.install_surface_state(surface_state);
        let _ = self.surface_parts_changed(); // reported below, as part of the replaced drawing
        self.rebuild_stack();
        // Whole rebuild rather than incremental: the model was replaced, and a
        // rebuild is what picks a cell size suited to the data just loaded.
        self.rebuild_spatial_index();
        self.selection.clear();
        self.current_layer = String::from(DEFAULT_LAYER_NAME);
        self.current_style.clear();
        // Even when the project opened is the drawing already shown: the
        // history, selection and current layer went, and a view cannot know
        // the tables are the same without comparing them.
        self.model_revision += 1;
        if let Some(logger) = &self.logger {
            logger.info(
                "storage",
                "project opened",
                &[LogField::new(
                    "entities",
                    format!("{}", self.model.entities.size()),
                )],
            );
        }
        self.remember_tables();
        // What the project was drawn with that this session does not have.
        // Worked out here, at the open, so that every front end is told the
        // same names and the next save knows which it could not judge.
        let missing = customisation_not_loaded(
            &self.state.metadata.customisation,
            &self.state.library,
            &self.state.customisation.sources,
            &self.customisation_renames(),
        );
        let missing_changed = missing != self.state.customisation.missing_at_open;
        if missing_changed {
            self.state.customisation.missing_at_open = missing;
            self.state.customisation_generation += 1;
        }
        self.notify_parts(
            REPLACED_PARTS
                | if missing_changed {
                    DocumentChange::CUSTOMISATION
                } else {
                    0
                },
        );
        Ok(())
    }

    /// Saves to the project this document was opened from / last saved to.
    pub fn save(&mut self) -> Status {
        let Some(mut store) = self.store.take() else {
            return Err(Error::new(
                ErrorCode::InvalidState,
                "the drawing has not been saved to a project yet; use saveAs()",
            ));
        };
        // Snapshot of the state about to be replaced, so a bad save is
        // recoverable.
        let saved = store
            .backup(ProjectStore::DEFAULT_BACKUPS_TO_KEEP)
            .and_then(|_| self.save_contents(&mut store));
        self.store = Some(store);
        let changed = saved?;
        self.stack.mark_saved();
        self.metadata_modified = false;
        if let Some(logger) = &self.logger {
            logger.info(
                "storage",
                "project saved",
                &[LogField::new(
                    "entities",
                    format!("{}", self.model.entities.size()),
                )],
            );
        }
        // The record the save wrote is the metadata's now, when it differs.
        self.notify_parts(
            DocumentChange::SAVED | if changed { DocumentChange::METADATA } else { 0 },
        );
        Ok(())
    }

    /// The model, the metadata and the survey jobs into `store` in one save,
    /// with the customisation record as it stands now; on success the record
    /// saved becomes the metadata's, and the return says whether that changed
    /// it.
    pub(crate) fn save_contents(&mut self, store: &mut ProjectStore) -> Result<bool> {
        let mut contents = capture_model(&self.model, self.state.metadata.clone(), Vec::new());
        // The record of what the drawing is drawn with, as the session stands
        // at this save. Written into what is SAVED and not through
        // set_metadata: that marks the drawing modified, and a save that then
        // could not go ahead left a drawing nobody had touched asking to be
        // saved - which is why the front ends, when they wrote it, first had to
        // work out whether the save had anywhere to go.
        contents.metadata.customisation = customisation_record_to_save(
            &self.state.metadata.customisation,
            &self.state.customisation.missing_at_open,
            &self.state.library,
            &self.state.customisation.sources,
            &self.customisation_renames(),
        );
        // The surfaces and their styles, likewise into what is saved.
        self.write_surface_state(&mut contents)?;
        // LENT to the contents for the save and taken back after, rather than
        // copied: a job holds its field file whole, and copying tens of
        // megabytes on every save to hand it to a function that only reads it
        // is waste. Handed back however the save ends.
        contents.survey_jobs = std::mem::take(&mut self.state.survey_jobs);
        let saved = store.save(&contents);
        self.state.survey_jobs = std::mem::take(&mut contents.survey_jobs);
        saved?;
        // Only now: a failed save leaves the metadata as it was.
        let changed = self.state.metadata.customisation != contents.metadata.customisation;
        self.state.metadata.customisation = std::mem::take(&mut contents.metadata.customisation);
        Ok(changed)
    }

    /// Creates a new project directory (must not exist as a project yet).
    pub fn save_as(&mut self, project_directory: &Path) -> Status {
        if self.state.metadata.name.is_empty() || self.state.metadata.name == "Untitled" {
            self.state.metadata.name = project_directory
                .file_stem()
                .map(|stem| utf8_or_windows1252(path_to_utf8(Path::new(stem))))
                .unwrap_or_default();
        }
        let mut store = ProjectStore::create(project_directory, &self.state.metadata)?;
        // Picks up the creation timestamp.
        if let Ok(created) = store.load() {
            self.state.metadata.created_utc = created.metadata.created_utc;
        }
        self.save_contents(&mut store)?;
        self.store = Some(store);
        self.stack.mark_saved();
        self.metadata_modified = false;
        // The name and the creation time above may have been filled in.
        self.notify_parts(DocumentChange::SAVED | DocumentChange::METADATA);
        Ok(())
    }

    pub fn is_modified(&self) -> bool {
        self.stack.is_modified() || self.metadata_modified
    }

    pub fn has_project(&self) -> bool {
        self.store.is_some()
    }

    pub fn project_directory(&self) -> Option<PathBuf> {
        self.store
            .as_ref()
            .map(|store| store.directory().to_path_buf())
    }

    pub fn metadata(&self) -> &ProjectMetadata {
        &self.state.metadata
    }

    pub fn set_metadata(&mut self, metadata: ProjectMetadata) {
        if metadata != self.state.metadata {
            self.state.metadata = metadata;
            self.metadata_modified = true;
            self.notify_parts(DocumentChange::METADATA);
        }
    }

    // ---- survey jobs (survey_job, wave H1) ----------------------------------------------
    //
    // The field files imported as jobs, in id order, kept so their reduction
    // and adjustment can be revisited. Changed ONLY by the survey job
    // commands, which are undoable - there is deliberately no setter; the
    // commands reach the list through the state link. Saved with the project
    // (storage's ProjectContents::survey_jobs); cleared by new_document() and
    // replaced by open(). find_survey_job is survey_job's.

    pub fn survey_jobs(&self) -> &[SurveyJob] {
        &self.state.survey_jobs
    }

    /// Counts every change to the job list, for a panel that lists the jobs.
    pub fn survey_jobs_generation(&self) -> u64 {
        self.state.survey_jobs_generation
    }

    // ---- listeners --------------------------------------------------------------------------
    //
    // Called after anything observable changed: model, selection, current
    // layer, project. The registration lasts as long as the handle does -
    // keep it as a member of the object the listener captures, declared so
    // that it dies first. Both kinds are called for every notification, in
    // the order they were added. A change listener is told what changed; a
    // plain one is not, and so has to treat every call as a change to
    // everything.

    pub fn add_listener(&self, listener: impl FnMut() + Send + 'static) -> ListenerHandle {
        self.register(Callback::Plain(Box::new(listener)))
    }

    pub fn add_change_listener(
        &self,
        listener: impl FnMut(&DocumentChange<'_>) + Send + 'static,
    ) -> ListenerHandle {
        self.register(Callback::Typed(Box::new(listener)))
    }

    fn register(&self, callback: Callback) -> ListenerHandle {
        let mut registry = lock(&self.listeners);
        let id = registry.next_id;
        registry.next_id += 1;
        registry.entries.push(Entry {
            id,
            call: Arc::new(Mutex::new(callback)),
        });
        ListenerHandle {
            registry: Arc::downgrade(&self.listeners),
            id,
        }
    }

    pub(crate) fn notify(&self, change: &DocumentChange<'_>) {
        // By id, not by position: a listener may end a registration - its
        // own, or another's - while it runs, and the list then moves under a
        // loop.
        let snapshot: Vec<(u64, Arc<Mutex<Callback>>)> = lock(&self.listeners)
            .entries
            .iter()
            .map(|entry| (entry.id, Arc::clone(&entry.call)))
            .collect();
        for (id, call) in snapshot {
            let registered = lock(&self.listeners)
                .entries
                .iter()
                .any(|entry| entry.id == id);
            if !registered {
                continue;
            }
            let mut callback = lock(&call);
            match &mut *callback {
                Callback::Typed(listener) => listener(change),
                Callback::Plain(listener) => listener(),
            }
        }
    }

    pub(crate) fn notify_parts(&self, parts: u32) {
        self.notify(&DocumentChange::of(parts));
    }

    pub(crate) fn rebuild_stack(&mut self) {
        // The C++ destroys the old stack first, its destructor clearing the
        // model's entity observer; the Rust stack clears nothing when it goes,
        // and the new one's observer replaces the old.
        let (stack, published) = new_stack(&mut self.model, self.logger.clone());
        self.stack = stack;
        self.published = published;
    }

    // ---- primitives for the document's own steps -----------------------------------------

    /// What a command of the document's own captures to reach its state.
    pub(crate) fn state_link(&self) -> StateLink {
        self.lender.clone()
    }

    /// A step named `name` that changes the document's state by `apply`
    /// (true: to the state after; false: back to the state before). Making it
    /// changes nothing; running it does, and only through this Document's
    /// execute(), so a tool holding `&Document` can make one.
    pub(crate) fn state_command(
        &self,
        name: impl Into<String>,
        apply: impl FnMut(&mut DocumentState, bool) + Send + 'static,
    ) -> CommandPtr {
        Box::new(StateCommand {
            name: name.into(),
            link: self.state_link(),
            apply: Box::new(apply),
        })
    }

    /// A step that sets the metadata key `key` to `after`, None taking the key
    /// away, and undoes to `before`: the C++'s AnnotationScaleCommand and
    /// ViewPaperScaleCommand.
    pub(crate) fn metadata_key_command(
        &self,
        name: impl Into<String>,
        key: &str,
        before: Option<String>,
        after: Option<String>,
    ) -> CommandPtr {
        let key = String::from(key);
        self.state_command(name, move |state, to_after| {
            match if to_after { &after } else { &before } {
                Some(text) => {
                    state
                        .metadata
                        .unknown_keys
                        .insert(key.clone(), text.clone());
                }
                None => {
                    state.metadata.unknown_keys.remove(&key);
                }
            }
        })
    }

    /// The C++ road model's `changeCommand` (and setSheetSet's body): the step
    /// that replaces the value a metadata key holds, as one undo step named
    /// `step_name`, None when it changes nothing. `before` is the slot's
    /// parsed key brought up to date (CacheSlot::parsed_key), `slot` says
    /// which of the state's slots it is, `write` the value's JSON. CommandRejected
    /// when the key was written by a newer Katana (`what` names it: "sheets",
    /// "road templates"); an empty value is stored as no key at all.
    #[allow(
        dead_code,
        reason = "Called by the sheet set's and the road model's setters (waves G2 and G4)."
    )]
    #[allow(
        clippy::too_many_arguments,
        reason = "The C++ changeCommand's arguments, with the step name and what the key holds."
    )]
    pub(crate) fn parsed_key_command<T: Default + PartialEq + Clone + Send + Sync + 'static>(
        &self,
        slot: fn(&DocumentState) -> &CacheSlot,
        key: &str,
        before: Arc<ParsedKey<T>>,
        set: &T,
        write: impl FnOnce(&T) -> Result<String>,
        step_name: impl Into<String>,
        what: &str,
    ) -> Result<Option<CommandPtr>> {
        if let Err(error) = &before.status
            && error.code == ErrorCode::Unsupported
        {
            return Err(make_error(
                ErrorCode::CommandRejected,
                format!(
                    "the {what} were written by a newer version of Katana; they are kept as they are"
                ),
                error.describe(),
            ));
        }
        let mut after = ParsedKey::<T>::default();
        // An empty set is stored as no key at all.
        if *set != T::default() {
            after.text = write(set)?;
            after.present = true;
            after.set = Arc::new(set.clone());
        }
        if before.present == after.present && before.text == after.text {
            return Ok(None); // nothing changed, and nothing to undo
        }
        let after = Arc::new(after);
        let key = String::from(key);
        Ok(Some(self.state_command(
            step_name,
            move |state, to_after| {
                let chosen = if to_after { &after } else { &before };
                if chosen.present {
                    state
                        .metadata
                        .unknown_keys
                        .insert(key.clone(), chosen.text.clone());
                } else {
                    state.metadata.unknown_keys.remove(&key);
                }
                slot(state).store(Arc::clone(chosen));
            },
        )))
    }
}
