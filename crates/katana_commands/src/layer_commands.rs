//! The layer commands, declared in entity_commands.rs as the C++ declares
//! them in entity_commands.hpp. Layer commands share one shape: a validated
//! forward step and its exact inverse.

use katana_core::error::{Error, ErrorCode, Status, make_error};
use katana_entity::layer_path::{is_layer_under, validate_layer_path};
use katana_entity::tables::DEFAULT_LAYER_NAME;
use katana_entity::{Entity, Layer};

use crate::command::{Command, CommandContext, CommandPtr};

struct CreateLayerCommand {
    layer: Layer,
}

impl Command for CreateLayerCommand {
    fn name(&self) -> &str {
        "CREATE_LAYER"
    }

    // The table's own rule - the path, the weight, the names it references,
    // and that it is new - so validate() refuses what execute() would (audit
    // MOD-09: "a//b" and a negative weight passed here and failed there).
    fn validate(&self, context: &CommandContext<'_>) -> Status {
        context.model.layers.check_add(&self.layer)
    }

    fn execute(&mut self, context: &mut CommandContext<'_>) -> Status {
        context.model.layers.add(self.layer.clone())
    }

    fn undo(&mut self, context: &mut CommandContext<'_>) -> Status {
        context.model.layers.remove(&self.layer.name).map(|_| ())
    }

    fn redo(&mut self, context: &mut CommandContext<'_>) -> Status {
        self.execute(context)
    }
}

struct UpdateLayerCommand {
    after: Layer,
    before: Layer,
}

impl Command for UpdateLayerCommand {
    fn name(&self) -> &str {
        "UPDATE_LAYER"
    }

    fn validate(&self, context: &CommandContext<'_>) -> Status {
        let Some(current) = context.model.layers.find(&self.after.name) else {
            return Err(make_error(
                ErrorCode::NotFound,
                "layer does not exist",
                self.after.name.as_str(),
            ));
        };
        if *current == self.after {
            return Err(make_error(
                ErrorCode::CommandRejected,
                "layer already has these attributes",
                self.after.name.as_str(),
            ));
        }
        context.model.layers.check_update(&self.after)
    }

    fn execute(&mut self, context: &mut CommandContext<'_>) -> Status {
        let Some(current) = context.model.layers.find(&self.after.name) else {
            return Err(make_error(
                ErrorCode::NotFound,
                "layer does not exist",
                self.after.name.as_str(),
            ));
        };
        self.before = current.clone();
        context.model.layers.update(&self.after)
    }

    fn undo(&mut self, context: &mut CommandContext<'_>) -> Status {
        context.model.layers.update(&self.before)
    }

    fn redo(&mut self, context: &mut CommandContext<'_>) -> Status {
        context.model.layers.update(&self.after)
    }
}

struct DeleteLayerCommand {
    layer_name: String,
    removed: Layer,
}

impl Command for DeleteLayerCommand {
    fn name(&self) -> &str {
        "DELETE_LAYER"
    }

    fn is_destructive(&self) -> bool {
        true
    }

    fn validate(&self, context: &CommandContext<'_>) -> Status {
        if self.layer_name == DEFAULT_LAYER_NAME {
            return Err(Error::new(
                ErrorCode::CommandRejected,
                "the default layer cannot be deleted",
            ));
        }
        if !context.model.layers.contains(&self.layer_name) {
            return Err(make_error(
                ErrorCode::NotFound,
                "layer does not exist",
                self.layer_name.as_str(),
            ));
        }
        let in_use = context.model.entities.count_on_layer(&self.layer_name);
        if in_use > 0 {
            return Err(make_error(
                ErrorCode::CommandRejected,
                "layer still contains entities",
                format!("layer={} entities={in_use}", self.layer_name),
            ));
        }
        // And the table's: a layer with nested layers is refused by remove()
        // (audit MOD-09: validate() said OK, execute() said no).
        context.model.layers.check_remove(&self.layer_name)
    }

    fn execute(&mut self, context: &mut CommandContext<'_>) -> Status {
        self.removed = context.model.layers.remove(&self.layer_name)?;
        Ok(())
    }

    fn undo(&mut self, context: &mut CommandContext<'_>) -> Status {
        context.model.layers.add(self.removed.clone())
    }

    fn redo(&mut self, context: &mut CommandContext<'_>) -> Status {
        self.execute(context)
    }
}

// Deleting a whole branch. Separate from DeleteLayerCommand rather than a flag
// on it because the checks differ: a branch is only removable when EVERY layer
// in it is empty, and saying which one is not is the difference between a
// usable error and "cannot delete".
struct DeleteLayerTreeCommand {
    name: String,
    removed: Vec<Layer>,
}

impl Command for DeleteLayerTreeCommand {
    fn name(&self) -> &str {
        "DeleteLayerTree"
    }

    // A whole branch goes, every layer's colour, linetype and flags with it:
    // at least as destructive as DELETE_LAYER of one empty leaf, which asks
    // (audit MOD-08).
    fn is_destructive(&self) -> bool {
        true
    }

    fn validate(&self, context: &CommandContext<'_>) -> Status {
        if self.name == DEFAULT_LAYER_NAME {
            return Err(Error::new(
                ErrorCode::CommandRejected,
                "the default layer cannot be deleted",
            ));
        }
        if !context.model.layers.contains(&self.name) {
            return Err(make_error(
                ErrorCode::NotFound,
                "layer does not exist",
                self.name.as_str(),
            ));
        }
        for path in context.model.layers.subtree(&self.name) {
            let in_use = context.model.entities.count_on_layer(&path);
            if in_use > 0 {
                return Err(make_error(
                    ErrorCode::CommandRejected,
                    "a nested layer still has entities",
                    format!("layer={path} entities={in_use}"),
                ));
            }
        }
        Ok(())
    }

    fn execute(&mut self, context: &mut CommandContext<'_>) -> Status {
        self.removed = context.model.layers.remove_subtree(&self.name)?;
        Ok(())
    }

    fn undo(&mut self, context: &mut CommandContext<'_>) -> Status {
        // remove_subtree returns deepest first, so replaying it backwards
        // recreates each parent before its children.
        for layer in self.removed.iter().rev() {
            if context.model.layers.contains(&layer.name) {
                // add() created it as an ancestor of a layer already restored;
                // update it so its real colour and flags come back.
                context.model.layers.update(layer)?;
                continue;
            }
            context.model.layers.add(layer.clone())?;
        }
        Ok(())
    }

    fn redo(&mut self, context: &mut CommandContext<'_>) -> Status {
        self.execute(context)
    }
}

// Renaming a branch has to move the entities with it, because an entity names
// its layer by full path: leaving them behind would point every one of them at
// a layer that no longer exists. Both halves are in ONE command so that undo
// restores both or neither.
struct RenameLayerCommand {
    from: String,
    to: String,
    mapping: Vec<(String, String)>,
}

impl RenameLayerCommand {
    fn move_entities(&self, context: &mut CommandContext<'_>, forward: bool) -> Status {
        // Collected first, then applied: replace() while iterating would
        // mutate the container being walked.
        let mut changed: Vec<Entity> = Vec::new();
        context.model.entities.for_each(|entity| {
            for (before, after) in &self.mapping {
                let (source, target) = if forward {
                    (before, after)
                } else {
                    (after, before)
                };
                if entity.layer == *source {
                    let mut moved = entity.clone();
                    moved.layer = target.clone();
                    changed.push(moved);
                    return;
                }
            }
        });
        for entity in changed {
            context.model.entities.replace(entity)?;
        }
        Ok(())
    }
}

impl Command for RenameLayerCommand {
    fn name(&self) -> &str {
        "RenameLayer"
    }

    fn validate(&self, context: &CommandContext<'_>) -> Status {
        if self.from == DEFAULT_LAYER_NAME {
            return Err(Error::new(
                ErrorCode::CommandRejected,
                "the default layer cannot be renamed",
            ));
        }
        if !context.model.layers.contains(&self.from) {
            return Err(make_error(
                ErrorCode::NotFound,
                "layer does not exist",
                self.from.as_str(),
            ));
        }
        validate_layer_path(&self.to)?;
        if is_layer_under(&self.to, &self.from) && self.from != self.to {
            return Err(make_error(
                ErrorCode::CommandRejected,
                "a layer cannot be moved inside itself",
                format!("{} -> {}", self.from, self.to),
            ));
        }
        if context.model.layers.contains(&self.to) && self.from != self.to {
            return Err(make_error(
                ErrorCode::AlreadyExists,
                "a layer of that name already exists",
                self.to.as_str(),
            ));
        }
        Ok(())
    }

    fn execute(&mut self, context: &mut CommandContext<'_>) -> Status {
        self.mapping = context.model.layers.rename_subtree(&self.from, &self.to)?;
        self.move_entities(context, true)
    }

    fn undo(&mut self, context: &mut CommandContext<'_>) -> Status {
        context.model.layers.rename_subtree(&self.to, &self.from)?;
        self.move_entities(context, false)
    }

    fn redo(&mut self, context: &mut CommandContext<'_>) -> Status {
        self.execute(context)
    }
}

pub fn create_layer(layer: Layer) -> CommandPtr {
    Box::new(CreateLayerCommand { layer })
}

/// Changes the attributes of the layer named layer.name.
pub fn update_layer(layer: Layer) -> CommandPtr {
    Box::new(UpdateLayerCommand {
        after: layer,
        before: Layer::default(),
    })
}

/// Fails while entities still use the layer, and while it has nested layers
/// beneath it (use delete_layer_tree for that).
pub fn delete_layer(name: impl Into<String>) -> CommandPtr {
    Box::new(DeleteLayerCommand {
        layer_name: name.into(),
        removed: Layer::default(),
    })
}

/// Deletes a layer and everything nested under it. Fails when ANY layer in the
/// subtree still holds entities - reported with the layer named, so the user is
/// told which one rather than left to find it.
pub fn delete_layer_tree(name: impl Into<String>) -> CommandPtr {
    Box::new(DeleteLayerTreeCommand {
        name: name.into(),
        removed: Vec::new(),
    })
}

/// Renames a layer and every layer nested under it, moving the entities on
/// them to match. One command, so undo puts back both the tree and the
/// entities.
pub fn rename_layer(from: impl Into<String>, to: impl Into<String>) -> CommandPtr {
    Box::new(RenameLayerCommand {
        from: from.into(),
        to: to.into(),
        mapping: Vec::new(),
    })
}
