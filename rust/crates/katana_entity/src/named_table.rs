//! One implementation of "a table of things looked up by name".
//!
//! Linetypes, dimension styles and styles are the same container: an ordered
//! map from name to record, with add / update / remove / find / all,
//! `AlreadyExists` and `NotFound` reported the same way, and in two of the
//! three a built-in entry that cannot be removed. They were three hand-written
//! copies of that, and the next such table would have been a fourth.
//!
//! What a table is allowed to differ in is expressed as a policy, so that the
//! differences are visible in one place:
//!
//!   * the noun used in its error messages, so they still read as English;
//!   * what makes a record valid;
//!   * an extra rule for updates only (the continuous linetype may be renamed
//!     in every respect except being given a pattern);
//!   * a built-in name that `remove` refuses;
//!   * what `reset` seeds.
//!
//! `LayerDatabase` is deliberately NOT built on this. Layer names are paths
//! and the table derives a tree from them, so it is a different data
//! structure that happens to be keyed by name (layer_path.rs).
//!
//! The map is ordered, so that `all()` and `names()` come back in name order
//! without sorting. The project store writes tables in that order, and a
//! stable order is what makes a saved file diffable.

use std::collections::BTreeMap;
use std::marker::PhantomData;

use katana_core::error::{ErrorCode, Result, Status, make_error};

/// The map every table is built on; a policy's `seed` receives one.
pub type NamedMap<T> = BTreeMap<String, T>;

/// A record a table keys by its name (the C++ reads `item.name`).
pub trait Named {
    fn name(&self) -> &str;
}

/// What a table differs in. The defaults are the C++ NamedTablePolicy<T>'s:
/// a table with no built-in entry and no update-only rule says nothing about
/// either.
pub trait NamedTablePolicy<T> {
    const NOUN: &'static str;
    fn validate(item: &T) -> Status;
    fn check_update(_item: &T) -> Status {
        Ok(())
    }
    fn is_protected(_name: &str) -> bool {
        false
    }
    fn seed(_items: &mut NamedMap<T>) {}
}

#[derive(Clone, Debug)]
pub struct NamedTable<T, P> {
    items: NamedMap<T>,
    revision: u64,
    policy: PhantomData<P>,
}

impl<T: Named + Clone, P: NamedTablePolicy<T>> Default for NamedTable<T, P> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Named + Clone, P: NamedTablePolicy<T>> NamedTable<T, P> {
    pub fn new() -> Self {
        let mut table = Self {
            items: NamedMap::new(),
            revision: 0,
            policy: PhantomData,
        };
        table.reset();
        table
    }

    pub fn reset(&mut self) {
        self.items.clear();
        P::seed(&mut self.items);
        self.revision += 1;
    }

    pub fn add(&mut self, item: T) -> Status {
        P::validate(&item)?;
        if self.contains(item.name()) {
            return Err(make_error(
                ErrorCode::AlreadyExists,
                format!("{} already exists", P::NOUN),
                item.name(),
            ));
        }
        self.items.insert(item.name().to_owned(), item);
        self.revision += 1;
        Ok(())
    }

    pub fn update(&mut self, item: &T) -> Status {
        if !self.items.contains_key(item.name()) {
            return Err(make_error(
                ErrorCode::NotFound,
                format!("{} does not exist", P::NOUN),
                item.name(),
            ));
        }
        // The update-only rule runs before validation, so that a record which
        // is both invalid and forbidden reports the more specific reason.
        P::check_update(item)?;
        P::validate(item)?;
        if let Some(found) = self.items.get_mut(item.name()) {
            *found = item.clone();
        }
        self.revision += 1;
        Ok(())
    }

    pub fn remove(&mut self, name: &str) -> Result<T> {
        if P::is_protected(name) {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!("the built-in {} cannot be removed", P::NOUN),
                name,
            ));
        }
        match self.items.remove(name) {
            Some(removed) => {
                self.revision += 1;
                Ok(removed)
            }
            None => Err(make_error(
                ErrorCode::NotFound,
                format!("{} does not exist", P::NOUN),
                name,
            )),
        }
    }

    pub fn find(&self, name: &str) -> Option<&T> {
        self.items.get(name)
    }

    pub fn contains(&self, name: &str) -> bool {
        self.find(name).is_some()
    }

    /// Ascending.
    pub fn names(&self) -> Vec<String> {
        self.items.keys().cloned().collect()
    }

    /// Visits every record in name order WITHOUT copying it. `all()` returns a
    /// vector by value, which is convenient for a caller that wants to keep
    /// the answer and wasteful for one that only wants to read it.
    pub fn for_each(&self, mut visit: impl FnMut(&T)) {
        for item in self.items.values() {
            visit(item);
        }
    }

    /// Ascending by name.
    pub fn all(&self) -> Vec<T> {
        self.items.values().cloned().collect()
    }

    pub fn size(&self) -> usize {
        self.items.len()
    }

    pub fn empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Counts up by one for every change that succeeds - add, update, remove,
    /// reset - and never down. A table has no observer of its own, so this is
    /// how the Document tells which tables a command touched. A copy carries
    /// it along; only a change moves it.
    pub fn revision(&self) -> u64 {
        self.revision
    }
}
