//! A ChangeSet is the unit of atomic model mutation: a list of entities to
//! add, modify and remove. Almost every command is "compute a ChangeSet from
//! the current model, then apply it", which gives all of them the same
//! validation, atomicity and undo behaviour for free.

use std::cell::RefCell;
use std::collections::BTreeSet;

use katana_core::error::{Error, ErrorCode, Result, Status, make_error};
use katana_entity::layer_path::layer_ancestors;
use katana_entity::{Entity, EntityId, Geometry, Model};

use crate::command::{Command, CommandContext};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChangeSet {
    /// Ids are assigned on apply.
    pub add: Vec<Entity>,
    /// Matched by id.
    pub modify: Vec<Entity>,
    pub remove: Vec<EntityId>,
}

impl ChangeSet {
    pub fn empty(&self) -> bool {
        self.add.is_empty() && self.modify.is_empty() && self.remove.is_empty()
    }
}

fn id_context(id: EntityId) -> String {
    format!("id={id}")
}

// Locked by itself OR by an ancestor: locking "design" protects
// "design/surface/tin1" too. It used to test the layer's own flag only, so
// SELECT ALL then ERASE erased what a locked parent was protecting (audit
// MOD-03). The refusal names the layer that holds the lock, because "layer is
// locked" about a layer whose own lock box is clear is a puzzle, not an answer.
fn require_unlocked_layer(model: &Model, layer_name: &str, id: EntityId) -> Status {
    let layer = model.layers.resolve(layer_name);
    let Some(own) = layer.layer else {
        return Err(make_error(
            ErrorCode::NotFound,
            "layer does not exist",
            format!("layer={layer_name} {}", id_context(id)),
        ));
    };
    if layer.locked {
        let mut holder = String::from(layer_name);
        if !own.locked {
            for ancestor in layer_ancestors(layer_name) {
                if let Some(above) = model.layers.find(&ancestor)
                    && above.locked
                {
                    holder = ancestor;
                    break;
                }
            }
        }
        let message = if holder == layer_name {
            String::from("layer is locked")
        } else {
            format!("layer is locked by its parent '{holder}'")
        };
        return Err(make_error(
            ErrorCode::CommandRejected,
            message,
            format!("layer={layer_name} {}", id_context(id)),
        ));
    }
    Ok(())
}

// Rules for an entity as it will exist after the change.
fn validate_resulting_entity(model: &Model, entity: &Entity) -> Status {
    katana_entity::validate(&entity.geometry)?;
    require_unlocked_layer(model, &entity.layer, entity.id)?;
    if !entity.style.is_empty() && model.styles.find(&entity.style).is_none() {
        return Err(make_error(
            ErrorCode::NotFound,
            "style does not exist",
            format!("style={}", entity.style),
        ));
    }
    // The annotation tables an annotation names (annotation.rs): a text or
    // leader in a text style that is not there would draw in the default face
    // with nothing to say why, and a label in a missing label style would draw
    // nothing at all.
    let text_style = match &entity.geometry {
        Geometry::Text(text) => Some(text.style.as_str()),
        Geometry::Leader(leader) => Some(leader.style.as_str()),
        _ => None,
    };
    if let Some(text_style) = text_style
        && !text_style.is_empty()
        && !model.text_styles.contains(text_style)
    {
        return Err(make_error(
            ErrorCode::NotFound,
            "text style does not exist",
            format!("text style={text_style} {}", id_context(entity.id)),
        ));
    }
    if let Geometry::Label(label) = &entity.geometry
        && !model.label_styles.contains(&label.style)
    {
        return Err(make_error(
            ErrorCode::NotFound,
            "label style does not exist",
            format!("label style={} {}", label.style, id_context(entity.id)),
        ));
    }
    if let Geometry::Leader(leader) = &entity.geometry
        && !leader.label_style.is_empty()
        && !model.label_styles.contains(&leader.label_style)
    {
        return Err(make_error(
            ErrorCode::NotFound,
            "label style does not exist",
            format!(
                "label style={} {}",
                leader.label_style,
                id_context(entity.id)
            ),
        ));
    }
    for (key, value) in &entity.properties {
        model.properties.validate(key, value)?;
    }
    // Metadata is validated too. It is free-form provenance rather than
    // schema'd data, and PropertyDatabase::validate treats an unknown key
    // leniently, but the FINITENESS check has to apply here as well: the
    // serializer writes a non-finite double as JSON null, and the loader
    // rejects null, so a single NaN reaching metadata produces a project that
    // saves without complaint and can then never be opened again.
    // create_entities() takes caller-supplied entities including metadata -
    // that is how an importer builds them - so without this loop the import
    // path could write an unopenable file.
    for (key, value) in &entity.metadata {
        model.properties.validate(key, value)?;
    }
    Ok(())
}

// Rules for touching an entity that is currently in the model.
fn validate_existing_entity(model: &Model, id: EntityId, seen: &mut BTreeSet<EntityId>) -> Status {
    let Some(current) = model.entities.find(id) else {
        return Err(make_error(
            ErrorCode::NotFound,
            "entity does not exist",
            id_context(id),
        ));
    };
    if !seen.insert(id) {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "entity is listed more than once in the same change",
            id_context(id),
        ));
    }
    require_unlocked_layer(model, &current.layer, id)
}

/// Checks a ChangeSet against the model without applying it:
///   * modified / removed entities exist and sit on unlocked layers,
///   * no entity is both modified and removed, or listed twice,
///   * added / modified entities have valid geometry, an existing unlocked
///     target layer, an existing style (when named) and well-typed properties.
pub fn validate_change_set(changes: &ChangeSet, model: &Model) -> Status {
    let mut seen = BTreeSet::new();
    for &id in &changes.remove {
        validate_existing_entity(model, id, &mut seen)?;
    }
    for entity in &changes.modify {
        validate_existing_entity(model, entity.id, &mut seen)?;
        validate_resulting_entity(model, entity)?;
    }
    for entity in &changes.add {
        validate_resulting_entity(model, entity)?;
    }
    Ok(())
}

/// The C++ `std::function<Result<ChangeSet>(const CommandContext&)>`.
pub type Builder = Box<dyn Fn(&CommandContext<'_>) -> Result<ChangeSet> + Send>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Pending,
    Applied,
    Reverted,
}

/// Command whose effect is described by a ChangeSet built on demand.
pub struct ChangeSetCommand {
    name: String,
    builder: Builder,
    destructive: bool,
    state: State,

    // What validate() built, handed on to the execute() that follows it.
    //
    // Every path that executes a command - CommandStack::execute() and
    // Transaction::execute() - calls validate() and then execute() with
    // nothing in between that can touch the model, so the set the two
    // computed was always identical and the first was thrown away. Building
    // it twice costs a full copy of every entity the command adds, which is
    // the whole import on a CREATE_ENTITIES of tens of thousands of entities.
    // validate() still rebuilds every time it is called, so it remains a pure
    // function of the model as it is now; execute() CONSUMES the cache and
    // re-runs validate_change_set() against the live model regardless, so a
    // set that somehow went stale is rejected rather than applied.
    built: RefCell<Option<Result<ChangeSet>>>,

    // Recorded by execute(): exact images, so undo/redo never recompute
    // anything.
    /// Created entities, in creation order.
    added_ids: Vec<EntityId>,
    // Images of the created entities, filled by undo() and used by redo().
    // execute() deliberately keeps no image: EntityDatabase::remove() hands
    // undo() the entity it removes, which is exactly what redo() must put
    // back, so a copy taken at execute() time duplicated every created Entity
    // for something that is only ever needed after an undo.
    added_images: Vec<Entity>,
    /// Modified entities, prior state.
    before: Vec<Entity>,
    /// Modified entities, new state.
    after: Vec<Entity>,
    /// Removed entities, prior state.
    removed: Vec<Entity>,
}

impl ChangeSetCommand {
    pub fn new<F>(name: impl Into<String>, builder: F, destructive: bool) -> Self
    where
        F: Fn(&CommandContext<'_>) -> Result<ChangeSet> + Send + 'static,
    {
        Self {
            name: name.into(),
            builder: Box::new(builder),
            destructive,
            state: State::Pending,
            built: RefCell::new(None),
            added_ids: Vec::new(),
            added_images: Vec::new(),
            before: Vec::new(),
            after: Vec::new(),
            removed: Vec::new(),
        }
    }

    fn nothing_to_change(&self) -> Error {
        make_error(
            ErrorCode::CommandRejected,
            "command would not change anything",
            self.name.as_str(),
        )
    }

    // A failure after validation is a bug; the partial work is rolled back
    // before reporting it.
    fn fail(&mut self, context: &mut CommandContext<'_>, cause: &Error) -> Status {
        let _ = self.undo(context); // the state is Applied; restores whatever was recorded
        self.state = State::Pending;
        self.added_ids.clear();
        self.added_images.clear();
        self.before.clear();
        self.after.clear();
        self.removed.clear();
        Err(make_error(
            ErrorCode::Internal,
            "validated change could not be applied",
            cause.describe(),
        ))
    }
}

impl Command for ChangeSetCommand {
    fn name(&self) -> &str {
        &self.name
    }

    fn is_destructive(&self) -> bool {
        self.destructive
    }

    fn validate(&self, context: &CommandContext<'_>) -> Status {
        // Rebuilt on every call so validate() keeps answering for the model as
        // it is now; kept so the execute() that follows need not repeat the
        // work.
        let built = (self.builder)(context);
        let status = match &built {
            Err(error) => Err(error.clone()),
            Ok(changes) if changes.empty() => Err(self.nothing_to_change()),
            Ok(changes) => validate_change_set(changes, context.model),
        };
        *self.built.borrow_mut() = Some(built);
        status
    }

    fn execute(&mut self, context: &mut CommandContext<'_>) -> Status {
        if self.state != State::Pending {
            return Err(make_error(
                ErrorCode::InvalidState,
                "command was already executed",
                self.name.as_str(),
            ));
        }
        // Single use: a later execute() builds afresh.
        let built = match self.built.get_mut().take() {
            Some(built) => built,
            None => (self.builder)(context),
        };
        let changes = built?;
        if changes.empty() {
            return Err(self.nothing_to_change());
        }
        validate_change_set(&changes, context.model)?;

        // Everything was validated, so the mutations below cannot fail for any
        // reason the model knows about. They are still checked: a failure here
        // is a bug, and the partial work is rolled back before reporting it.
        self.state = State::Applied;
        let ChangeSet {
            add,
            modify,
            remove,
        } = changes;
        self.removed.reserve(remove.len());
        self.before.reserve(modify.len());
        self.after.reserve(modify.len());
        self.added_ids.reserve(add.len());

        for id in remove {
            match context.model.entities.remove(id) {
                Ok(removed) => self.removed.push(removed),
                Err(cause) => return self.fail(context, &cause),
            }
        }
        for entity in modify {
            // Validated as present; the error is the one replace() would give.
            let Some(previous) = context.model.entities.find(entity.id).cloned() else {
                let cause = make_error(
                    ErrorCode::NotFound,
                    "entity does not exist",
                    id_context(entity.id),
                );
                return self.fail(context, &cause);
            };
            if let Err(cause) = context.model.entities.replace(entity.clone()) {
                return self.fail(context, &cause);
            }
            self.before.push(previous);
            self.after.push(entity);
        }
        for entity in add {
            // `changes` is this command's own build, so the entity can be
            // handed straight over; the image undo()/redo() need comes back out
            // of the database when it is removed again.
            match context.model.entities.add(entity) {
                Ok(id) => self.added_ids.push(id),
                Err(cause) => return self.fail(context, &cause),
            }
        }
        Ok(())
    }

    fn undo(&mut self, context: &mut CommandContext<'_>) -> Status {
        if self.state != State::Applied {
            return Err(make_error(
                ErrorCode::InvalidState,
                "command is not applied",
                self.name.as_str(),
            ));
        }
        let entities = &mut context.model.entities;
        let mut first_failure: Status = Ok(());
        let mut note = |status: Status| {
            if status.is_err() && first_failure.is_ok() {
                first_failure = status;
            }
        };
        // Reverse order of execute(). remove() returns the entity it took out,
        // which is the image redo() has to put back, so it is captured here
        // rather than copied when the entity was created.
        self.added_images
            .resize(self.added_ids.len(), Entity::default());
        for (id, image) in self
            .added_ids
            .iter()
            .zip(self.added_images.iter_mut())
            .rev()
        {
            match entities.remove(*id) {
                Ok(removed) => *image = removed,
                Err(error) => note(Err(error)),
            }
        }
        for entity in self.before.iter().rev() {
            note(entities.replace(entity.clone()));
        }
        for entity in self.removed.iter().rev() {
            note(entities.insert(entity.clone()));
        }
        self.state = State::Reverted;
        first_failure
    }

    fn redo(&mut self, context: &mut CommandContext<'_>) -> Status {
        if self.state != State::Reverted {
            return Err(make_error(
                ErrorCode::InvalidState,
                "command is not undone",
                self.name.as_str(),
            ));
        }
        let entities = &mut context.model.entities;
        let mut first_failure: Status = Ok(());
        let mut note = |status: Status| {
            if status.is_err() && first_failure.is_ok() {
                first_failure = status;
            }
        };
        for entity in &self.removed {
            note(entities.remove(entity.id).map(|_| ()));
        }
        for entity in &self.after {
            note(entities.replace(entity.clone()));
        }
        for entity in &self.added_images {
            note(entities.insert(entity.clone())); // same ids as the first execution
        }
        self.state = State::Applied;
        first_failure
    }

    fn created_entities(&self) -> Vec<EntityId> {
        if self.state == State::Applied {
            self.added_ids.clone()
        } else {
            Vec::new()
        }
    }
}
