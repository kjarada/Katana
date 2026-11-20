//! Owning container of entities.
//!
//! Identity   Ids start at 1, increase monotonically and are never reused, even
//!            after removal, so an id recorded by an undo step or an external
//!            reference can never silently point at a different entity.
//! Ordering   Iteration is by ascending id: deterministic across runs.
//! Events     Every mutation reports a ChangeEvent to the single observer, which
//!            the owning document uses to feed transactions and views.
//! Threading  Owned and mutated by the document thread only.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::sync::Arc;

use katana_core::error::{Error, ErrorCode, Result, Status, make_error};
use katana_geometry::Box2;

use crate::entity::{Entity, EntityId, INVALID_ENTITY_ID};
use crate::entity_geometry::{bounding_box, validate};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ChangeKind {
    EntityAdded,
    #[default]
    EntityModified,
    EntityRemoved,
    Cleared,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ChangeEvent {
    pub kind: ChangeKind,
    /// INVALID_ENTITY_ID for Cleared.
    pub id: EntityId,
}

/// The C++ `std::function<void(const ChangeEvent&)>`. Shared rather than boxed
/// because the C++ database is copyable observer and all - a copy of the model
/// (a worker's snapshot) carries it until the copy clears it - and Send and
/// Sync because such a copy is handed to a worker thread.
pub type Observer = Arc<dyn Fn(&ChangeEvent) + Send + Sync>;

#[derive(Clone)]
pub struct EntityDatabase {
    entities: BTreeMap<EntityId, Entity>,
    /// How many entities sit on each layer, kept in step by every member that
    /// changes `entities`; a layer with none has no entry. count_on_layer is
    /// asked once a layer row by the layers panel on every frame it is shown,
    /// and by LAYER LIST and the layer manager: answered by a scan, that was
    /// the whole drawing walked once per layer (docs/performance.md, "Layer
    /// counts").
    layer_counts: HashMap<String, usize>,
    next_id: EntityId,
    observer: Option<Observer>,
}

impl fmt::Debug for EntityDatabase {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EntityDatabase")
            .field("entities", &self.entities)
            .field("next_id", &self.next_id)
            .field("observer", &self.observer.is_some())
            .finish()
    }
}

impl Default for EntityDatabase {
    fn default() -> Self {
        Self {
            entities: BTreeMap::new(),
            layer_counts: HashMap::new(),
            next_id: 1,
            observer: None,
        }
    }
}

fn id_context(id: EntityId) -> String {
    format!("id={id}")
}

fn validate_entity(entity: &Entity) -> Status {
    if entity.layer.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "entity layer name is empty",
        ));
    }
    validate(&entity.geometry)
}

impl EntityDatabase {
    pub fn new() -> Self {
        Self::default()
    }

    /// Validates the geometry, assigns a fresh id (ignoring entity.id) and
    /// stores it.
    pub fn add(&mut self, mut entity: Entity) -> Result<EntityId> {
        validate_entity(&entity)?;
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        entity.id = id;
        self.count_layer(&entity.layer);
        self.entities.insert(id, entity);
        self.notify(ChangeKind::EntityAdded, id);
        Ok(id)
    }

    /// Stores an entity under the id it already carries: undo of a removal and
    /// loading from storage. Fails when the id is invalid or taken.
    pub fn insert(&mut self, entity: Entity) -> Status {
        if entity.id == INVALID_ENTITY_ID {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "cannot insert an entity without an id",
            ));
        }
        if self.contains(entity.id) {
            return Err(make_error(
                ErrorCode::AlreadyExists,
                "entity id is already in use",
                id_context(entity.id),
            ));
        }
        validate_entity(&entity)?;
        let id = entity.id;
        // Wrapping as the C++ unsigned id does: the largest id leaves the
        // counter where it was rather than failing.
        self.reserve_ids_below(id.wrapping_add(1));
        self.count_layer(&entity.layer);
        self.entities.insert(id, entity);
        self.notify(ChangeKind::EntityAdded, id);
        Ok(())
    }

    /// Replaces the stored entity that has entity.id.
    pub fn replace(&mut self, entity: Entity) -> Status {
        let Some(stored) = self.entities.get(&entity.id) else {
            return Err(make_error(
                ErrorCode::NotFound,
                "entity does not exist",
                id_context(entity.id),
            ));
        };
        // Most replacements leave the entity on its layer (a move, a restyle),
        // and those leave the counts alone.
        let changes_layer = stored.layer != entity.layer;
        validate_entity(&entity)?;
        let id = entity.id;
        if changes_layer {
            self.count_layer(&entity.layer);
        }
        if let Some(previous) = self.entities.insert(id, entity)
            && changes_layer
        {
            self.uncount_layer(&previous.layer);
        }
        self.notify(ChangeKind::EntityModified, id);
        Ok(())
    }

    /// Removes and returns the entity so the caller can restore it later.
    pub fn remove(&mut self, id: EntityId) -> Result<Entity> {
        match self.entities.remove(&id) {
            Some(removed) => {
                self.uncount_layer(&removed.layer);
                self.notify(ChangeKind::EntityRemoved, id);
                Ok(removed)
            }
            None => Err(make_error(
                ErrorCode::NotFound,
                "entity does not exist",
                id_context(id),
            )),
        }
    }

    pub fn clear(&mut self) {
        self.entities.clear(); // next_id is deliberately kept: ids are never reused
        self.layer_counts.clear();
        self.notify(ChangeKind::Cleared, INVALID_ENTITY_ID);
    }

    pub fn find(&self, id: EntityId) -> Option<&Entity> {
        self.entities.get(&id)
    }

    pub fn contains(&self, id: EntityId) -> bool {
        self.find(id).is_some()
    }

    pub fn size(&self) -> usize {
        self.entities.len()
    }

    pub fn empty(&self) -> bool {
        self.entities.is_empty()
    }

    pub fn ids(&self) -> Vec<EntityId> {
        self.entities.keys().copied().collect()
    }

    pub fn ids_on_layer(&self, layer: &str) -> Vec<EntityId> {
        self.entities
            .iter()
            .filter(|(_, entity)| entity.layer == layer)
            .map(|(id, _)| *id)
            .collect()
    }

    /// How many entities are on exactly `layer` (not its sublayers): what
    /// `ids_on_layer(layer).len()` is, from the kept counts instead of a walk
    /// of every entity.
    pub fn count_on_layer(&self, layer: &str) -> usize {
        self.layer_counts.get(layer).copied().unwrap_or(0)
    }

    fn count_layer(&mut self, layer: &str) {
        // Looked up before it is inserted: the layer is nearly always there
        // already, and `entry` would copy its name to ask.
        match self.layer_counts.get_mut(layer) {
            Some(count) => *count += 1,
            None => {
                self.layer_counts.insert(String::from(layer), 1);
            }
        }
    }

    fn uncount_layer(&mut self, layer: &str) {
        if let Some(count) = self.layer_counts.get_mut(layer) {
            *count -= 1;
            // No entry for an empty layer, so the map is no larger than the
            // layers in use however many names have come and gone.
            if *count == 0 {
                self.layer_counts.remove(layer);
            }
        }
    }

    /// Visits entities in ascending id order.
    pub fn for_each(&self, mut visit: impl FnMut(&Entity)) {
        for entity in self.entities.values() {
            visit(entity);
        }
    }

    /// The entities in ascending id order, as for_each visits them, each
    /// borrowed for as long as the database is: for a caller that keeps some
    /// of what it walks, which for_each's visit cannot. Walking this is one
    /// pass over the map; asking ids() and then find() for each is a lookup an
    /// entity (docs/performance.md, "The labels' walk").
    pub fn iter(&self) -> impl Iterator<Item = &Entity> {
        self.entities.values()
    }

    /// Union of the bounding boxes of all entities (visible or not).
    pub fn bounds(&self) -> Box2 {
        let mut bounds = Box2::default();
        for entity in self.entities.values() {
            bounds.expand_box(bounding_box(&entity.geometry));
        }
        bounds
    }

    /// The id the next add() will assign. Persisted so ids stay unique across
    /// sessions.
    pub fn next_id(&self) -> EntityId {
        self.next_id
    }

    /// Only raises the counter; lowering it could resurrect retired ids.
    pub fn reserve_ids_below(&mut self, next_id: EntityId) {
        if next_id > self.next_id {
            self.next_id = next_id;
        }
    }

    /// The C++ `setObserver`; `None` is its `nullptr`.
    pub fn set_observer(&mut self, observer: Option<Observer>) {
        self.observer = observer;
    }

    /// Takes over the entities and the id counter of `other`, leaving THIS
    /// database's observer in place, and reports a single Cleared event
    /// exactly as clear() does.
    ///
    /// Loading needs this. A loader must build the contents aside and commit
    /// them only once every insertion has succeeded, or a failure halfway
    /// leaves the caller's drawing destroyed - but a plain assignment would
    /// also carry `other`'s (empty) observer across, silently stopping every
    /// change notification the Document depends on.
    pub fn adopt_contents(&mut self, other: EntityDatabase) {
        self.entities = other.entities;
        self.layer_counts = other.layer_counts;
        self.next_id = other.next_id;
        // The observer is deliberately NOT touched: it belongs to this
        // database's owner, not to the contents being adopted.
        self.notify(ChangeKind::Cleared, INVALID_ENTITY_ID);
    }

    fn notify(&self, kind: ChangeKind, id: EntityId) {
        if let Some(observer) = &self.observer {
            observer(&ChangeEvent { kind, id });
        }
    }
}
