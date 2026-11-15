//! What every customisation manager dialog is given by whoever opens it - the style and linestyle
//! manager, the symbol library, the survey-code manager - so the three are built the same way and
//! none reaches into the main window. The port of customisation/customisation_context.hpp.
//!
//! The Rust shape. The C++ context is an aggregate of borrowed pointers and callbacks: the
//! Document, the log, the executor, the picture cache and what only the maker can do. Here the
//! window is lent to a dialog as `&mut dyn Host` while it draws (host::command_runner), so the
//! drawing, the log and the one executor are the host's - `Host::document`, `Host::log`,
//! `Host::run` - and are not kept a second time in the context. What is left is what the host does
//! not carry: the shared picture cache, and the three things only the maker of the dialogs knows
//! how to do. The Document may still die before a dialog: a dialog watches it through
//! DocumentWatcher and asks it whether the Document is there.
//!
//! Nothing here is required: a test, or a manager built on its own, leaves every member empty.

use std::cell::RefCell;
use std::rc::Rc;

use katana_entity::EntityId;

use super::definition_thumbnails::DefinitionThumbnails;
use crate::host::Host;

/// What a manager asks the definition editor (definition_editor) to do with a library definition.
/// A new one is of the kind the manager that asked lists: the symbol library's is a symbol, the
/// Linetypes tab's a linestyle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DefinitionEdit {
    NewSymbol,
    NewLinestyle,
    Edit,
    Duplicate,
    Delete,
}

/// The picture cache shared by every picker and browser the dialogs open, so a symbol painted for
/// one list is not painted again for the next. Owned by the maker of the dialogs; it keys itself
/// on the library generation, so it needs no clearing when a library is loaded.
pub type SharedThumbnails = Rc<RefCell<DefinitionThumbnails>>;

/// "Show me what uses this": select these entities and frame them in the active view.
pub type SelectAndShow = Rc<dyn Fn(&mut dyn Host, &[EntityId])>;

/// Opens the one definition editor on the library definition named - which a New does not read.
pub type EditDefinition = Rc<dyn Fn(DefinitionEdit, &str)>;

/// What an editor is handed back by `begin_commit`, to call after its commit succeeded.
pub type AfterCommit = Box<dyn FnOnce(&mut dyn Host)>;

/// The session's customisation is about to be committed by an editor.
pub type BeginCommit = Rc<dyn Fn(&mut dyn Host) -> Option<AfterCommit>>;

/// See the module comment.
#[derive(Clone, Default)]
pub struct CustomisationContext {
    /// May be None: a picker then draws no pictures.
    pub thumbnails: Option<SharedThumbnails>,

    /// The main window supplies it, since only it knows which view is active. May be None (a
    /// test, the headless driver): the dialog then only selects, through the drawing's selection
    /// and its notification, which is still what a "Select users" button must do.
    pub select_and_show: Option<SelectAndShow>,

    /// The maker owns the editor, as it owns the managers, so a manager only asks. May be None (a
    /// test, a manager built on its own): the buttons that would ask are then disabled.
    pub edit_definition: Option<EditDefinition>,

    /// A definition saved or deleted: an editor calls this BEFORE it commits and, when it is
    /// handed a function back, calls that AFTER the commit succeeded (never after one that was
    /// refused). It is how a session whose customisation is the kept one stays kept across an
    /// editor's own change, where a change typed on the command line lasts the session only: what
    /// "before" has to remember, and what "after" then does, is the maker's business, and an
    /// editor knows neither. May be None, and may hand back nothing.
    pub begin_commit: Option<BeginCommit>,

    /// False when the dialog was built with no executor (the C++'s empty `run`: a test, a dialog
    /// built before the window's command line): the dialog then says it cannot run the line
    /// rather than doing the work itself. True by `with_runner`, which a maker with a host to
    /// lend calls.
    pub has_runner: bool,
}

impl CustomisationContext {
    /// A context whose dialogs run their lines through the host they are drawn with.
    pub fn with_runner() -> Self {
        Self {
            has_runner: true,
            ..Self::default()
        }
    }

    /// The same context sharing `thumbnails`.
    pub fn sharing(mut self, thumbnails: SharedThumbnails) -> Self {
        self.thumbnails = Some(thumbnails);
        self
    }
}
