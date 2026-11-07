//! Command system core.
//!
//!   Command -> Validation -> Transaction -> Domain Model -> Change Events
//!
//! Every modification of the model is a Command executed through a
//! CommandStack. This is also the only door the AI layer will get, so commands
//! are named, validated before they touch anything, atomic, and reversible.
//!
//! Contract for implementers
//!   validate()  has no side effects and may be called at any time.
//!   execute()   either fully succeeds or leaves the model exactly as it found it.
//!   undo()      restores the exact prior state (before-images, not inverse maths).
//!   redo()      reproduces the result of execute(), including entity ids.

use katana_core::Status;
use katana_core::log::Logger;
use katana_entity::{ChangeEvent, EntityId, Model};

/// What a command runs against. The C++ holds the model by reference and the
/// logger as an optional pointer; here both are borrows for the one call.
pub struct CommandContext<'a> {
    pub model: &'a mut Model,
    /// Optional.
    pub logger: Option<&'a Logger>,
}

impl<'a> CommandContext<'a> {
    /// The C++ `CommandContext{model}`: no logger.
    pub fn new(model: &'a mut Model) -> Self {
        Self {
            model,
            logger: None,
        }
    }
}

/// One undoable edit. Send, so that a history (and the document holding it)
/// can be handed to another thread whole.
pub trait Command: Send {
    /// Stable identifier, e.g. "CREATE_LINE".
    fn name(&self) -> &str;

    /// True when the command discards user data (DELETE...). The AI safety
    /// model requires user confirmation for destructive commands.
    fn is_destructive(&self) -> bool {
        false
    }

    fn validate(&self, context: &CommandContext<'_>) -> Status;
    fn execute(&mut self, context: &mut CommandContext<'_>) -> Status;
    fn undo(&mut self, context: &mut CommandContext<'_>) -> Status;
    fn redo(&mut self, context: &mut CommandContext<'_>) -> Status;

    /// Entities created by the last execute()/redo(); empty for other commands.
    fn created_entities(&self) -> Vec<EntityId> {
        Vec::new()
    }
}

/// The C++ `std::unique_ptr<Command>`. It cannot be null: a factory that may
/// have no command to give returns `Option<CommandPtr>`.
pub type CommandPtr = Box<dyn Command>;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CommandEventKind {
    #[default]
    Executed,
    Undone,
    Redone,
}

/// Published by the CommandStack after the model changed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommandEvent {
    pub kind: CommandEventKind,
    pub command_name: String,
    /// Entity-level detail, in order.
    pub changes: Vec<ChangeEvent>,
}
