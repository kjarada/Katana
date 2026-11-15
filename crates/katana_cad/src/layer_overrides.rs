//! Per-view layer visibility (the user's request of 2026-09-23: "per view
//! controls to add and remove layers"; include/katana/cad/layer_overrides.hpp).
//!
//! A layer's visibility is document state: it is saved, it is undoable, and
//! every view obeys it. A view may additionally hide layers of its own - a plan
//! with the survey hidden beside a 3D view that shows it - and that is VIEW
//! state: not saved in the project, not undoable, and never seen by another
//! view. This type is that second half.
//!
//! It is SUBTRACTIVE. A view can hide what the document shows; it can never
//! show what the document hides. That keeps the one visibility rule in
//! selection.rs a plain conjunction - drawn = document says drawn AND this
//! view does not hide it - and means a layer the user switched off in the
//! Layers panel cannot reappear in some view they forgot about.
//!
//! Hiding a path hides everything beneath it in that view, exactly as the
//! document rule does (PLAN.MD 5.1): hiding "design" hides "design/surface/tin1".
//! The test walks the path's ancestors, which is a slice per segment and a set
//! lookup each - no allocation, and no LayerDatabase needed, which is what lets
//! every consumer ask it without being handed the model. docs/model.md records
//! why the document rule does NOT walk ancestors per query (it stores them
//! resolved); a view's set is a handful of paths, and resolving it per view
//! would need a notification the document does not send.

use std::collections::BTreeSet;

use katana_core::error::make_error;
use katana_core::{ErrorCode, Status};
use katana_entity::LayerDatabase;
use katana_entity::layer_path::{is_layer_under, layer_parent};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LayerOverrides {
    // Ordered, as the C++'s std::set is: hidden() is listed in name order.
    hidden: BTreeSet<String>,
}

/// What a caller passes when it means the document rule alone: Surface From
/// Drawing, the command-line SELECT and the section cut are shared results and
/// must not depend on which view happened to be active.
pub static NO_LAYER_OVERRIDES: LayerOverrides = LayerOverrides {
    hidden: BTreeSet::new(),
};

impl LayerOverrides {
    pub fn new() -> Self {
        Self::default()
    }

    /// Hides `path` and everything beneath it in this view. False when it was
    /// already hidden directly, so a caller can tell whether to repaint.
    pub fn hide(&mut self, path: &str) -> bool {
        if path.is_empty() {
            return false;
        }
        self.hidden.insert(String::from(path))
    }

    /// Removes this view's hide of exactly `path`. An ANCESTOR hidden in this
    /// view still hides it; showing a child of a hidden parent is the same
    /// impossibility the Layers panel greys out for the document rule.
    pub fn show(&mut self, path: &str) -> bool {
        self.hidden.remove(path)
    }

    /// Shows everything the document shows.
    pub fn clear(&mut self) {
        self.hidden.clear();
    }

    /// Hides everything that is not `path`, one of its ancestors, or beneath
    /// it, using the fewest entries: at each level of `path`, the siblings of
    /// the next segment. `all_paths` is every layer name in the document (the
    /// tree is derived from the names, entity's layer_path.rs). Replaces
    /// whatever this view hid before. Fails with NotFound when no layer is
    /// `path` or lies beneath it - isolating nothing would hide the whole
    /// drawing, which is never what was meant.
    pub fn isolate(&mut self, path: &str, all_paths: &[String]) -> Status {
        let exists = all_paths.iter().any(|name| is_layer_under(name, path));
        if path.is_empty() || !exists {
            return Err(make_error(ErrorCode::NotFound, "no layer to isolate", path));
        }

        // Every node of the derived tree, not only the names that have a
        // record: "design" is a node when only "design/surface" is a layer, and
        // hiding it is how its siblings of "keep" get hidden with one entry.
        let mut nodes: BTreeSet<&str> = BTreeSet::new();
        for name in all_paths {
            let mut at = name.as_str();
            while !at.is_empty() {
                nodes.insert(at);
                at = layer_parent(at);
            }
        }

        let mut next = BTreeSet::new();
        for node in nodes {
            if is_layer_under(node, path) || is_layer_under(path, node) {
                continue; // the isolated branch itself, or on the way down to it
            }
            // Hide the highest node off the path: one whose parent IS on the
            // path (an ancestor of it), or a root. Anything deeper is already
            // hidden by that entry.
            let parent = layer_parent(node);
            if parent.is_empty() || is_layer_under(path, parent) {
                next.insert(String::from(node));
            }
        }
        self.hidden = next;
        Ok(())
    }

    /// Drops entries that name no layer: neither a layer of that name nor any
    /// layer beneath it. Document listeners carry no payload, so a view cannot
    /// follow a rename or a delete; pruning after every change is what stops a
    /// later layer that happens to reuse the name being born hidden here.
    /// Returns how many entries were dropped.
    pub fn prune_missing(&mut self, layers: &LayerDatabase) -> usize {
        if self.hidden.is_empty() {
            return 0;
        }
        let names = layers.names();
        let before = self.hidden.len();
        self.hidden
            .retain(|entry| names.iter().any(|name| is_layer_under(name, entry)));
        before - self.hidden.len()
    }

    /// True when `path` or any ancestor of it is hidden in this view. Returns at
    /// once for an empty set, which is every view the user has not touched -
    /// this is asked once per entity per frame.
    pub fn hides(&self, path: &str) -> bool {
        if self.hidden.is_empty() {
            return false;
        }
        let mut at = path;
        while !at.is_empty() {
            if self.hidden.contains(at) {
                return true;
            }
            at = layer_parent(at);
        }
        false
    }

    /// True only when `path` itself is an entry, as a check box shows it.
    pub fn hides_directly(&self, path: &str) -> bool {
        self.hidden.contains(path)
    }

    pub fn empty(&self) -> bool {
        self.hidden.is_empty()
    }

    pub fn size(&self) -> usize {
        self.hidden.len()
    }

    pub fn hidden(&self) -> &BTreeSet<String> {
        &self.hidden
    }
}
