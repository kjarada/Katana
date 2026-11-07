//! Executes commands against a model and keeps the undo / redo history.
//!
//! Ownership  The stack owns executed commands. The model is NOT held: the
//!            C++ stack references it and requires it to outlive the stack,
//!            which a Rust struct beside its model (the document) cannot say,
//!            so each execute, undo and redo is handed the model instead. It
//!            must be the model the stack was made with.
//! Observer   The stack installs itself as the EntityDatabase observer of the
//!            model so it can attach entity-level changes to CommandEvents.
//!            Code that needs change notifications subscribes to the stack.
//! Threading  Confined to the thread that owns the model.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use katana_core::error::{Error, ErrorCode, Status, make_error};
use katana_core::log::{LogField, Logger};
use katana_entity::entity_database::Observer;
use katana_entity::{ChangeEvent, EntityId, Model};

use crate::command::{Command, CommandContext, CommandEvent, CommandEventKind, CommandPtr};

/// The C++ `std::function<void(const CommandEvent&)>`.
pub type Listener = Box<dyn FnMut(&CommandEvent) + Send>;

fn to_string(kind: CommandEventKind) -> &'static str {
    match kind {
        CommandEventKind::Executed => "executed",
        CommandEventKind::Undone => "undone",
        CommandEventKind::Redone => "redone",
    }
}

fn lock(pending: &Mutex<Vec<ChangeEvent>>) -> MutexGuard<'_, Vec<ChangeEvent>> {
    pending.lock().unwrap_or_else(PoisonError::into_inner)
}

/// An executed command and the identity the save point is kept by. The C++
/// compares the command's address; a serial number is the same identity
/// without depending on where a box is put (every zero-sized command would
/// share one address).
struct Step {
    serial: u64,
    command: CommandPtr,
}

pub struct CommandStack {
    logger: Option<Arc<Logger>>,
    undo_stack: Vec<Step>,
    redo_stack: Vec<Step>,
    listeners: Vec<Listener>,
    // Shared with the model's observer, which holds it weakly: once the stack
    // is gone the observer records nothing, as the C++ destructor's clearing
    // of the observer ensures there.
    pending_changes: Arc<Mutex<Vec<ChangeEvent>>>,
    next_serial: u64,

    // Identity of the command on top of the undo stack at the last save; None
    // when saved with an empty stack. `saved_reachable` turns false once that
    // state can no longer be reached by undo/redo.
    saved_top: Option<u64>,
    saved_reachable: bool,
}

impl CommandStack {
    pub fn new(model: &mut Model, logger: Option<Arc<Logger>>) -> Self {
        let pending_changes = Arc::new(Mutex::new(Vec::new()));
        let recorder = Arc::downgrade(&pending_changes);
        let observer: Observer = Arc::new(move |event: &ChangeEvent| {
            if let Some(pending) = recorder.upgrade() {
                lock(&pending).push(*event);
            }
        });
        model.entities.set_observer(Some(observer));
        Self {
            logger,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            listeners: Vec::new(),
            pending_changes,
            next_serial: 0,
            saved_top: None,
            saved_reachable: true,
        }
    }

    /// validate() then execute(). On success the command becomes the newest
    /// undo step and the redo history is discarded. On failure the model is
    /// unchanged and the command is dropped.
    pub fn execute(&mut self, model: &mut Model, mut command: CommandPtr) -> Status {
        let started = Instant::now();
        lock(&self.pending_changes).clear();

        let mut context = CommandContext {
            model,
            logger: self.logger.as_deref(),
        };
        let mut status = command.validate(&context);
        if status.is_ok() {
            status = command.execute(&mut context);
        }
        if let Err(error) = status {
            lock(&self.pending_changes).clear();
            if let Some(logger) = &self.logger {
                logger.warning(
                    "command",
                    "rejected",
                    &[
                        LogField::new("name", command.name()),
                        LogField::new("reason", error.describe()),
                    ],
                );
            }
            return Err(error);
        }

        // The saved state becomes unreachable if it lived in the redo history.
        if let Some(saved) = self.saved_top
            && self.redo_stack.iter().any(|step| step.serial == saved)
        {
            self.saved_reachable = false;
            self.saved_top = None;
        }
        self.redo_stack.clear();
        let name = String::from(command.name());
        let serial = self.next_serial;
        self.next_serial += 1;
        self.undo_stack.push(Step { serial, command });

        if let Some(logger) = &self.logger {
            let changes = lock(&self.pending_changes).len();
            logger.info(
                "command",
                "executed",
                &[
                    LogField::new("name", name.as_str()),
                    LogField::new("changes", format!("{changes}")),
                    LogField::new("micros", format!("{}", started.elapsed().as_micros())),
                ],
            );
        }
        self.publish(CommandEventKind::Executed, name);
        Ok(())
    }

    pub fn undo(&mut self, model: &mut Model) -> Status {
        let Some(step) = self.undo_stack.last_mut() else {
            return Err(Error::new(ErrorCode::InvalidState, "nothing to undo"));
        };
        lock(&self.pending_changes).clear();
        let mut context = CommandContext {
            model,
            logger: self.logger.as_deref(),
        };
        if let Err(error) = step.command.undo(&mut context) {
            lock(&self.pending_changes).clear();
            return Err(error);
        }
        if let Some(step) = self.undo_stack.pop() {
            let name = String::from(step.command.name());
            self.redo_stack.push(step);
            self.publish(CommandEventKind::Undone, name);
        }
        Ok(())
    }

    pub fn redo(&mut self, model: &mut Model) -> Status {
        let Some(step) = self.redo_stack.last_mut() else {
            return Err(Error::new(ErrorCode::InvalidState, "nothing to redo"));
        };
        lock(&self.pending_changes).clear();
        let mut context = CommandContext {
            model,
            logger: self.logger.as_deref(),
        };
        if let Err(error) = step.command.redo(&mut context) {
            lock(&self.pending_changes).clear();
            return Err(error);
        }
        if let Some(step) = self.redo_stack.pop() {
            let name = String::from(step.command.name());
            self.undo_stack.push(step);
            self.publish(CommandEventKind::Redone, name);
        }
        Ok(())
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    pub fn undo_count(&self) -> usize {
        self.undo_stack.len()
    }

    pub fn redo_count(&self) -> usize {
        self.redo_stack.len()
    }

    /// Empty when there is nothing to undo.
    pub fn undo_name(&self) -> &str {
        self.undo_stack
            .last()
            .map_or("", |step| step.command.name())
    }

    /// Empty when there is nothing to redo.
    pub fn redo_name(&self) -> &str {
        self.redo_stack
            .last()
            .map_or("", |step| step.command.name())
    }

    /// Every step undo would take back, newest first - the first is
    /// undo_name() - and every step redo would put back, the next first: what
    /// the Edit toolbar's history lists, where the k-th entry is UNDO k or
    /// REDO k. The names are the commands' own, valid until the history next
    /// changes.
    pub fn undo_names(&self) -> Vec<&str> {
        names_from_the_top(&self.undo_stack)
    }

    pub fn redo_names(&self) -> Vec<&str> {
        names_from_the_top(&self.redo_stack)
    }

    /// Entities created by the most recently executed or redone command.
    pub fn last_created_entities(&self) -> Vec<EntityId> {
        self.undo_stack
            .last()
            .map_or_else(Vec::new, |step| step.command.created_entities())
    }

    /// Forgets all history (after loading another document).
    pub fn clear(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
        lock(&self.pending_changes).clear();
        self.mark_saved();
    }

    /// Save-point tracking: is_modified() is true whenever the model differs
    /// from the state at the last mark_saved(), including after undoing past it.
    pub fn mark_saved(&mut self) {
        self.saved_top = self.undo_stack.last().map(|step| step.serial);
        self.saved_reachable = true;
    }

    pub fn is_modified(&self) -> bool {
        if !self.saved_reachable {
            return true;
        }
        self.undo_stack.last().map(|step| step.serial) != self.saved_top
    }

    pub fn add_listener(&mut self, listener: impl FnMut(&CommandEvent) + Send + 'static) {
        self.listeners.push(Box::new(listener));
    }

    fn publish(&mut self, kind: CommandEventKind, command_name: String) {
        let changes = std::mem::take(&mut *lock(&self.pending_changes));
        let event = CommandEvent {
            kind,
            command_name,
            changes,
        };
        if let Some(logger) = &self.logger
            && kind != CommandEventKind::Executed
        {
            logger.info(
                "command",
                to_string(kind),
                &[LogField::new("name", event.command_name.as_str())],
            );
        }
        for listener in &mut self.listeners {
            listener(&event);
        }
    }
}

// The stacks keep their next step at the back, so the history reads from the
// back.
fn names_from_the_top(stack: &[Step]) -> Vec<&str> {
    stack.iter().rev().map(|step| step.command.name()).collect()
}

// ---- Transaction -------------------------------------------------------------------

/// Executes several commands as one atomic, single-step-undoable command. If
/// any part fails, the parts already executed are undone and the failure is
/// returned.
pub struct Transaction {
    name: String,
    commands: Vec<CommandPtr>,
}

impl Transaction {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            commands: Vec::new(),
        }
    }

    pub fn add(&mut self, command: CommandPtr) {
        self.commands.push(command);
    }

    pub fn size(&self) -> usize {
        self.commands.len()
    }
}

impl Command for Transaction {
    fn name(&self) -> &str {
        &self.name
    }

    fn is_destructive(&self) -> bool {
        self.commands.iter().any(|command| command.is_destructive())
    }

    /// Only the first part can be validated up front: later parts may depend
    /// on the effects of earlier ones, so they are validated as they execute.
    fn validate(&self, context: &CommandContext<'_>) -> Status {
        match self.commands.first() {
            None => Err(make_error(
                ErrorCode::CommandRejected,
                "transaction is empty",
                self.name.as_str(),
            )),
            Some(first) => first.validate(context),
        }
    }

    fn execute(&mut self, context: &mut CommandContext<'_>) -> Status {
        let mut failed = None;
        for (i, command) in self.commands.iter_mut().enumerate() {
            let mut status = command.validate(context);
            if status.is_ok() {
                status = command.execute(context);
            }
            if let Err(error) = status {
                failed = Some((i, error));
                break;
            }
        }
        let Some((i, error)) = failed else {
            return Ok(());
        };
        for done in self.commands[..i].iter_mut().rev() {
            let _ = done.undo(context); // roll back in reverse order
        }
        Err(make_error(
            error.code,
            error.message,
            format!("transaction={} step={} {}", self.name, i + 1, error.context),
        ))
    }

    fn undo(&mut self, context: &mut CommandContext<'_>) -> Status {
        for command in self.commands.iter_mut().rev() {
            command.undo(context)?;
        }
        Ok(())
    }

    fn redo(&mut self, context: &mut CommandContext<'_>) -> Status {
        for command in &mut self.commands {
            command.redo(context)?;
        }
        Ok(())
    }

    fn created_entities(&self) -> Vec<EntityId> {
        let mut ids = Vec::new();
        for command in &self.commands {
            ids.extend(command.created_entities());
        }
        ids
    }
}
