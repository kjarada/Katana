//! The Document's road-object members (document.rs;
//! src/katana_cad/road/road_object_store.cpp), kept here with the rest of the
//! road-object code as the sheet set's are kept with the sheets: the document
//! only carries the JSON; everything that knows what a road object is lives in
//! road/.
//!
//! The parsed set is the document's road_object_cache (CacheSlot::parsed_key):
//! what the set was parsed from - whether the key was there, and its text - the
//! set, and why it could not be read. A state with no key is one too: the
//! drawing before its first object. (Undoing back to it still keeps the next
//! number the drawing has reached, so the key stays, empty - an id is never
//! given twice.)

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::sync::Arc;

use katana_commands::{Command, CommandContext, CommandPtr, create_layer};
use katana_core::error::make_error;
use katana_core::{ErrorCode, Result, Status};
use katana_entity::Layer;
use katana_entity::layer_path::layer_ancestors;

use crate::document::{Document, ParsedKey, ROAD_OBJECTS_KEY, StateLink};
use crate::road::road_objects::{
    RoadObjectSet, colour_of, layer_of, road_objects_from_json, road_objects_to_json,
};

// The set's state as a step keeps it, shared with the document's cache.
type ObjectsState = Arc<ParsedKey<RoadObjectSet>>;

// `state`, or a copy of it whose next number is `number` when that is higher.
// The copy is present even when it holds no objects: a set that has taken
// numbers keeps them, and the JSON carries "next". A state that could not be
// read is returned as it is, so undoing back to a damaged record restores its
// text and does not replace it with a tidy empty one.
fn with_number_at_least(state: &ObjectsState, number: u64) -> ObjectsState {
    if state.status.is_err() || state.set.next_number >= number {
        return Arc::clone(state);
    }
    let set = RoadObjectSet {
        next_number: number,
        ..(*state.set).clone()
    };
    let Ok(text) = road_objects_to_json(&set) else {
        return Arc::clone(state);
    };
    Arc::new(ParsedKey {
        present: true,
        text,
        set: Arc::new(set),
        status: Ok(()),
    })
}

// One change of the set: the layers it makes first, then the document's state
// before and after, applied by the document. ONE step, so one undo takes away
// the objects and the layer made for them together - a layer left behind by
// an undone ADD would be a layer nobody asked for.
struct RoadObjectsCommand {
    name: String,
    layers: Vec<CommandPtr>,
    link: StateLink,
    before: ObjectsState,
    after: ObjectsState,
}

impl RoadObjectsCommand {
    // The state after the change (true) or before it (false), put in the
    // metadata and the document's cache.
    fn apply(&self, to_after: bool) -> Status {
        self.link.with(|state| {
            // An id is never given twice - the IFC GlobalId derives from it -
            // and undo is no exception: a number the drawing has handed out
            // stays taken when the object it named is undone, and again when
            // an earlier step is redone, so the state restored keeps the
            // highest next number the drawing has reached. Stored with the
            // state, it is saved with the drawing too: the key stays, empty,
            // where it would have gone.
            let reached = state
                .road_object_cache
                .with(|cache: &mut Option<ObjectsState>| {
                    cache.as_ref().map_or(1, |cached| cached.set.next_number)
                });
            let chosen =
                with_number_at_least(if to_after { &self.after } else { &self.before }, reached);
            if chosen.present {
                state
                    .metadata
                    .unknown_keys
                    .insert(String::from(ROAD_OBJECTS_KEY), chosen.text.clone());
            } else {
                state.metadata.unknown_keys.remove(ROAD_OBJECTS_KEY);
            }
            state.road_object_cache.store(chosen);
            // Not notified here: the document's notification of this step,
            // its undo and its redo carries DocumentChange::ROAD_MODEL, found
            // by comparing the road model's metadata keys - this one among
            // them - with what the last notification saw
            // (Document::road_model_changed).
        })
    }

    // The layers made so far go again: nothing is left half done.
    fn take_back(layers: &mut [CommandPtr], context: &mut CommandContext<'_>) {
        for layer in layers.iter_mut().rev() {
            let _ = layer.undo(context);
        }
    }
}

impl Command for RoadObjectsCommand {
    fn name(&self) -> &str {
        &self.name
    }

    fn validate(&self, context: &CommandContext<'_>) -> Status {
        for layer in &self.layers {
            layer.validate(context)?;
        }
        Ok(())
    }

    fn execute(&mut self, context: &mut CommandContext<'_>) -> Status {
        let mut made = 0;
        let mut failure = None;
        for layer in &mut self.layers {
            match layer.execute(context) {
                Ok(()) => made += 1,
                Err(error) => {
                    failure = Some(error);
                    break;
                }
            }
        }
        if let Some(error) = failure {
            Self::take_back(&mut self.layers[..made], context);
            return Err(error);
        }
        // The state is lent to this step only when it runs through the
        // document that made it (StateLink); run anywhere else, it changes
        // nothing - its layers neither.
        if let Err(error) = self.apply(true) {
            Self::take_back(&mut self.layers, context);
            return Err(error);
        }
        Ok(())
    }

    fn undo(&mut self, context: &mut CommandContext<'_>) -> Status {
        self.apply(false)?;
        for layer in self.layers.iter_mut().rev() {
            layer.undo(context)?;
        }
        Ok(())
    }

    fn redo(&mut self, context: &mut CommandContext<'_>) -> Status {
        for layer in &mut self.layers {
            layer.redo(context)?;
        }
        if let Err(error) = self.apply(true) {
            Self::take_back(&mut self.layers, context);
            return Err(error);
        }
        Ok(())
    }
}

impl Document {
    // ---- road objects (docs/road_objects.md) ------------------------------------
    //
    // The objects placed along an alignment or a line - barriers, guide posts,
    // signs, light poles, culverts, walls, fences, line markings
    // (road_objects.rs) - kept as versioned JSON under the metadata key
    // "road_objects", the sheets' pattern: carried by the storage layer as a
    // key it does not read, parsed once per change. Their 3D objects are made
    // from them every time (bim::bim_object::placed_objects). An empty set
    // when there are none or they cannot be read; road_objects_status() then
    // says why, and set_road_objects refuses to overwrite objects a newer
    // Katana wrote.

    // The key, parsed when its text has changed since: compared by content,
    // not by a counter, because set_metadata, new_document and open replace
    // the whole metadata - this key included - and must be seen.
    fn road_object_key(&self) -> ObjectsState {
        self.state.road_object_cache.parsed_key(
            &self.state.metadata,
            ROAD_OBJECTS_KEY,
            road_objects_from_json,
        )
    }

    pub fn road_objects(&self) -> Arc<RoadObjectSet> {
        Arc::clone(&self.road_object_key().set)
    }

    pub fn road_objects_status(&self) -> Status {
        self.road_object_key().status.clone()
    }

    /// Replaces the set as ONE undoable step named `step_name` (the C++
    /// defaults it to Document::SET_ROAD_OBJECTS), which also makes any layer
    /// an object names that the drawing lacks; the step, its undo and its redo
    /// are reported as DocumentChange::ROAD_MODEL, as every change of a
    /// road-model key is. A set equal to the current one records nothing.
    pub fn set_road_objects(
        &mut self,
        objects: &RoadObjectSet,
        step_name: impl Into<String>,
    ) -> Status {
        match self.road_objects_command(objects, step_name)? {
            None => Ok(()), // nothing changed, and nothing to undo
            Some(command) => self.execute(command),
        }
    }

    /// The same change as a command, for a step that does more besides - a
    /// surface renamed with the corridors and objects that name it is ONE undo
    /// step. None when `objects` equals the current set; the errors of
    /// set_road_objects otherwise.
    pub fn road_objects_command(
        &self,
        objects: &RoadObjectSet,
        step_name: impl Into<String>,
    ) -> Result<Option<CommandPtr>> {
        // Brings the cache up to date with the metadata: it is the step's
        // "before".
        let before = self.road_object_key();
        if let Err(error) = &before.status
            && error.code == ErrorCode::Unsupported
        {
            return Err(make_error(
                ErrorCode::CommandRejected,
                "the road objects were written by a newer version of Katana; they are kept as \
                 they are",
                error.describe(),
            ));
        }
        let mut after = ParsedKey::<RoadObjectSet>::default();
        // No objects and no number taken is stored as no key at all; a set
        // whose objects were all deleted keeps its next number, so an id is
        // never given twice.
        if *objects != RoadObjectSet::default() {
            after.text = road_objects_to_json(objects)?;
            after.present = true;
            after.set = Arc::new(objects.clone());
        }
        if before.present == after.present && before.text == after.text {
            return Ok(None); // nothing changed, and nothing to undo
        }
        // The layers the objects are shown on, made where the drawing lacks
        // one, in the colour of the first object on it - and the parents of
        // those, which LayerDatabase::add would make on its own but the layer
        // command's undo would leave behind: "road/objects/barrier" made with
        // "road" and "road/objects" must take all three away again. Planned
        // by path, so a parent sorts before its children (a path is a prefix
        // of its children's), and a layer that is both an object's and
        // another's parent keeps the object's colour.
        let mut planned: BTreeMap<String, Layer> = BTreeMap::new();
        let mut named_by: BTreeMap<String, String> = BTreeMap::new();
        for object in &objects.objects {
            let name = layer_of(object);
            if self.model.layers.contains(&name) {
                continue;
            }
            if let Entry::Vacant(slot) = planned.entry(name.clone()) {
                slot.insert(Layer {
                    name: name.clone(),
                    color: colour_of(object),
                    ..Layer::default()
                });
                named_by.insert(name, object.id.clone());
            }
        }
        let wanted: Vec<String> = planned.keys().cloned().collect();
        for name in &wanted {
            for ancestor in layer_ancestors(name) {
                if self.model.layers.contains(&ancestor) {
                    continue;
                }
                if let Entry::Vacant(slot) = planned.entry(ancestor.clone()) {
                    slot.insert(Layer {
                        name: ancestor.clone(),
                        ..Layer::default()
                    });
                    let by = named_by[name].clone();
                    named_by.insert(ancestor, by);
                }
            }
        }
        let mut layers: Vec<CommandPtr> = Vec::new();
        for (name, layer) in planned {
            if let Err(error) = self.model.layers.check_add(&layer) {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    format!(
                        "road object {} names a layer that cannot be made",
                        named_by[&name]
                    ),
                    error.describe(),
                ));
            }
            layers.push(create_layer(layer));
        }
        let command: CommandPtr = Box::new(RoadObjectsCommand {
            name: step_name.into(),
            layers,
            link: self.state_link(),
            before,
            after: Arc::new(after),
        });
        Ok(Some(command))
    }
}
