//! The `commands` layer: undoable commands.
//!
//! The port of src/katana_commands and include/katana/commands (docs/rust.md).
//! Every edit of the model is a Command run through a CommandStack: one user
//! edit is one undo step, and the cad layer, the command line, MCP and the
//! window all drive the model through it.

pub mod change_set;
pub mod command;
pub mod command_stack;
pub mod entity_commands;
mod layer_commands;
mod table_commands;

pub use change_set::{Builder, ChangeSet, ChangeSetCommand, validate_change_set};
pub use command::{Command, CommandContext, CommandEvent, CommandEventKind, CommandPtr};
pub use command_stack::{CommandStack, Listener, Transaction};
pub use entity_commands::*;
