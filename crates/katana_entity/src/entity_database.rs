//! Owning container of entities.
//!
//! Identity   Ids start at 1, increase monotonically and are never reused, even
//!            after removal, so an id recorded by an undo step or an external
//!            reference can never silently point at a different entity.
//! Ordering   Iteration is by ascending id: deterministic across runs.
//! Events     Every mutation reports a ChangeEvent to the single observer, which
//!            the owning document uses to feed transactions and views.
//! Threading  Owned and mutated by the document thread only.

use std::collections::BTreeMap;
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
        self.entities.insert(id, entity);
        self.notify(ChangeKind::EntityAdded, id);
        Ok(())
    }

    /// Replaces the stored entity that has entity.id.
    pub fn replace(&mut self, entity: Entity) -> Status {
        if !self.entities.contains_key(&entity.id) {
            return Err(make_error(
                ErrorCode::NotFound,
                "entity does not exist",
                id_context(entity.id),
            ));
        }
        validate_entity(&entity)?;
        let id = entity.id;
        self.entities.insert(id, entity);
        self.notify(ChangeKind::EntityModified, id);
        Ok(())
    }

    /// Removes and returns the entity so the caller can restore it later.
    pub fn remove(&mut self, id: EntityId) -> Result<Entity> {
        match self.entities.remove(&id) {
            Some(removed) => {
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

    pub fn count_on_layer(&self, layer: &str) -> usize {
        self.entities
            .values()
            .filter(|entity| entity.layer == layer)
            .count()
    }

    /// Visits entities in ascending id order.
    pub fn for_each(&self, mut visit: impl FnMut(&Entity)) {
        for entity in self.entities.values() {
            visit(entity);
        }
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
