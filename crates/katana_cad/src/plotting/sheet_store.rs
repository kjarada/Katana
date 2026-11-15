//! The Document's sheet-set members (document.rs;
//! src/katana_cad/plotting/sheet_store.cpp), kept here with the rest of the
//! sheet code rather than in document.rs: the document only carries the
//! JSON; everything that knows what a sheet is lives in plotting/.
//!
//! The parsed set and the JSON it was parsed from are one value in the
//! state's sheet cache (CacheSlot::parsed_key, the C++ `SheetCache`), shared
//! by the undo step, so an undo or redo puts the text back in the metadata and
//! the parsed set back in the cache without parsing - the parse was most of
//! what an edit of a large set cost (bench_sheets.cpp).

use std::sync::Arc;

use katana_core::{Result, Status};

use crate::document::{CacheSlot, Document, DocumentState, ParsedKey};
use crate::plotting::sheet_json::{sheet_set_from_json, sheet_set_to_json};
use crate::plotting::sheet_set::SheetSet;

/// The metadata key the sheet set is stored under. The storage layer keeps a
/// key it does not read and writes it back unchanged (ProjectMetadata::
/// unknown_keys), which is what lets sheets live in a project without a schema
/// migration.
pub(crate) const SHEETS_KEY: &str = "sheets";

fn sheet_slot(state: &DocumentState) -> &CacheSlot {
    &state.sheet_cache
}

fn write_sheets(set: &SheetSet) -> Result<String> {
    sheet_set_to_json(set)
}

impl Document {
    // The cache brought up to date with the metadata: compared by content, not
    // by a revision counter, because set_metadata replaces the whole metadata -
    // sheets key included - and must be seen.
    fn sheet_cache(&self) -> Arc<ParsedKey<SheetSet>> {
        self.state
            .sheet_cache
            .parsed_key(&self.state.metadata, SHEETS_KEY, sheet_set_from_json)
    }

    /// The project's sheets (plotting/sheet_set.rs), read from the metadata
    /// and parsed once per change of it. An empty set when there are none,
    /// and when the stored text cannot be read (sheet_set_status says why).
    pub fn sheet_set(&self) -> Arc<SheetSet> {
        Arc::clone(&self.sheet_cache().set)
    }

    /// Why sheet_set() is empty although the project has a "sheets" key:
    /// ParseFailure for text that is not a sheet set, Unsupported for one a
    /// newer Katana wrote.
    pub fn sheet_set_status(&self) -> Status {
        self.sheet_cache().status.clone()
    }

    /// Replaces the sheet set as ONE undoable step named `step_name` (the C++
    /// defaults it to Document::SET_SHEETS); no step when it is the set
    /// already. CommandRejected, changing nothing, when the project's sheets
    /// were written by a newer version - they are kept as they are; an empty
    /// set is stored as no key at all.
    pub fn set_sheet_set(&mut self, sheets: &SheetSet, step_name: impl Into<String>) -> Status {
        // Brings the cache up to date with the metadata: it is the step's
        // "before".
        let before = self.sheet_cache();
        let command = self.parsed_key_command(
            sheet_slot,
            SHEETS_KEY,
            before,
            sheets,
            write_sheets,
            step_name,
            "sheets",
        )?;
        match command {
            Some(command) => self.execute(command),
            None => Ok(()), // nothing changed, and nothing to undo
        }
    }
}
