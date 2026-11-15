//! A Document listener for a customisation dialog: one delivery per frame, however many
//! notifications arrived, saying WHAT changed. The port of customisation/document_watcher.
//!
//! Why a dialog must not simply add a listener: a view reacting to a Document listener DEFERS and
//! COALESCES its reload (docs/cad.md). A table rebuilt inside the listener is rebuilt inside the
//! command that notified, possibly inside its own item's change - which is how the layer panel
//! once crashed - and a script of 500 commands would rebuild it 500 times. The watcher records
//! each notification and delivers once, from the frame: the C++ from the event loop
//! (`QTimer::singleShot(0)`), here when the dialog draws next and asks (`deliver`), never inside
//! the notification.
//!
//! What changed is worked out, not guessed from "a listener fired" (which a selection click also
//! does):
//!
//! ```text
//! library / survey_map  Document::library_generation / survey_map_generation differ from the
//!                       last delivery; they only ever count up
//! selection             the selected ids differ from the last delivery
//! current               the current layer or current style differs
//! model                 Document::model_revision differs from the last delivery: it counts
//!                       every execute, undo and redo and every new or opened drawing, and only
//!                       ever counts up, so "undo, then a new command" in one frame - which leaves
//!                       the history's counts where they began - and a reopened project that looks
//!                       exactly like the drawing it replaced are both seen
//! ```
//!
//! Safe if the Document dies first: the registration is a ListenerHandle, which holds the
//! Document's registry weakly, and a delivery asked for when the Document has gone checks that
//! registry before touching any Document - it then delivers nothing. Safe if the watcher dies
//! first: there is nothing queued anywhere but in the watcher, and its registration goes first.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use katana_cad::{Document, ListenerHandle, SelectionSet};

/// What changed since the last delivery.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct DocumentChanges {
    /// An execute, undo or redo; a new or opened drawing.
    pub model: bool,
    /// set_style_library: library_generation moved.
    pub library: bool,
    /// set_survey_map: survey_map_generation moved.
    pub survey_map: bool,
    /// The selected entity ids differ.
    pub selection: bool,
    /// The current layer or current style differs.
    pub current: bool,
}

impl DocumentChanges {
    pub fn any(&self) -> bool {
        self.model || self.library || self.survey_map || self.selection || self.current
    }
}

/// What a delivery compares with the delivery before.
#[derive(Clone, Debug, Default, PartialEq)]
struct Baseline {
    model: u64,
    library: u64,
    survey_map: u64,
    selection: SelectionSet,
    current_layer: String,
    current_style: String,
}

impl Baseline {
    fn of(document: &Document) -> Self {
        Self {
            model: document.model_revision(),
            library: document.library_generation(),
            survey_map: document.survey_map_generation(),
            selection: document.selection().clone(),
            current_layer: String::from(document.current_layer()),
            current_style: String::from(document.current_style()),
        }
    }
}

/// See the module comment. One frame's delivery a call of `deliver`.
pub struct DocumentWatcher {
    // First, so it is dropped first: no notification can arrive at a half-dropped watcher.
    registration: ListenerHandle,
    queued: Arc<AtomicBool>,
    baseline: Baseline,
}

impl DocumentWatcher {
    /// Registers with `document` at once; the changes are counted from now.
    pub fn new(document: &Document) -> Self {
        let queued = Arc::new(AtomicBool::new(false));
        let notified = Arc::clone(&queued);
        let registration = document.add_listener(move || notified.store(true, Ordering::Release));
        Self {
            registration,
            queued,
            baseline: Baseline::of(document),
        }
    }

    /// False once the Document has been dropped. A dialog that outlives its Document asks this
    /// before touching it anywhere else too.
    pub fn document_alive(&self) -> bool {
        self.registration.active()
    }

    /// A delivery is due: the Document notified since the last one.
    pub fn pending(&self) -> bool {
        self.queued.load(Ordering::Acquire)
    }

    /// The frame's delivery: when the Document notified since the last one, what changed since
    /// then - possibly no flag at all, since the Document also notifies for what none of the flags
    /// name (a save, the project's metadata); a dialog that does not care asks `any()`. None when
    /// nothing notified, and when the Document the watcher was made with has gone, which is then
    /// never read. `document` is the Document the watcher was made with.
    pub fn deliver(&mut self, document: &Document) -> Option<DocumentChanges> {
        if !self.queued.swap(false, Ordering::AcqRel) {
            return None;
        }
        // The Document can have gone between the notification and this frame: a dialog closing
        // with the application, a test's Document leaving scope. Its registry went with it, and
        // nothing of it may be read.
        if !self.document_alive() {
            return None;
        }
        let now = Baseline::of(document);
        let changes = DocumentChanges {
            model: now.model != self.baseline.model,
            library: now.library != self.baseline.library,
            survey_map: now.survey_map != self.baseline.survey_map,
            selection: now.selection != self.baseline.selection,
            current: now.current_layer != self.baseline.current_layer
                || now.current_style != self.baseline.current_style,
        };
        self.baseline = now;
        Some(changes)
    }
}
