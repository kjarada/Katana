//! A DXF import as the ONE undoable step both front ends execute, built here
//! so that the window and the command line cannot differ about it.

use std::collections::BTreeSet;

use katana_commands::{
    CommandPtr, Transaction, create_entities, create_layer, create_linetype, update_layer,
};
use katana_entity::layer_path::layer_ancestors;
use katana_entity::{Layer, Model};

use crate::reader::DxfImport;

/// The linetypes the model lacks, then the layers it lacks (parents before
/// children), then the entities - moved out of `imported` - and last the lock
/// put back on each new layer the file locked: a locked layer refuses new
/// entities, so it is created open and locked once they are on it, all within
/// the one step that undo takes back whole. A layer the model already has
/// locked (the entities' own, or a parent of it) is opened first and locked
/// again last in the same way, so that the same file imported twice, or two
/// sheets that share a locked layer, both come in.
///
/// None when there is nothing to add: every layer and linetype is already in
/// the model and the file drew nothing.
pub fn import_command(imported: &mut DxfImport, model: &Model) -> Option<CommandPtr> {
    let mut transaction = Transaction::new("IMPORT");
    // The layers the model already has that would refuse the new entities:
    // locked themselves, or the parent of a layer the entities land on. A
    // second import of the same file lands on the layers the first one
    // locked, and a sheet that shares a locked layer with the last is the
    // same case, so the lock is lifted for the entities and put back after
    // them - the model's lock, not the file's, since the layer is the
    // model's. Ordered, so that the step is the same every time.
    let receiving: BTreeSet<&str> = imported
        .entities
        .iter()
        .map(|entity| entity.layer.as_str())
        .collect();
    let mut holding: BTreeSet<String> = BTreeSet::new();
    for name in receiving {
        let mut lineage = layer_ancestors(name);
        lineage.push(name.to_owned());
        for layer in lineage {
            if model
                .layers
                .find(&layer)
                .is_some_and(|existing| existing.locked)
            {
                holding.insert(layer);
            }
        }
    }
    let mut to_relock: Vec<Layer> = Vec::new();
    for name in &holding {
        let Some(found) = model.layers.find(name) else {
            continue;
        };
        let mut open = found.clone();
        to_relock.push(open.clone());
        open.locked = false;
        transaction.add(update_layer(open));
    }
    for linetype in &imported.linetypes {
        if !model.linetypes.contains(&linetype.name) {
            transaction.add(create_linetype(linetype.clone()));
        }
    }
    // By name, so that "a" comes before "a/b": adding "a/b" first would make
    // "a" with the defaults, and adding "a" after it would then fail.
    let mut layers = imported.layers.clone();
    layers.sort_by(|a, b| a.name.as_str().cmp(b.name.as_str()));
    let mut to_lock: Vec<Layer> = Vec::new();
    for mut layer in layers {
        if model.layers.contains(&layer.name) {
            continue;
        }
        if layer.locked {
            to_lock.push(layer.clone());
            layer.locked = false;
        }
        transaction.add(create_layer(layer));
    }
    if !imported.entities.is_empty() {
        transaction.add(create_entities(std::mem::take(&mut imported.entities)));
    }
    for layer in to_lock {
        transaction.add(update_layer(layer));
    }
    for layer in to_relock {
        transaction.add(update_layer(layer));
    }
    if transaction.size() == 0 {
        return None;
    }
    let command: CommandPtr = Box::new(transaction);
    Some(command)
}
