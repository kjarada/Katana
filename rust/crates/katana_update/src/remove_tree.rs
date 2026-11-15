//! Deleting a transaction's folder, through FsOps. Internal to the update library (the C++'s
//! private header remove_tree.hpp). The apply deletes the folder of a transaction that ended
//! (healthy, or rolled back), and recovery sweeps what a crash left of one; both go through this.
//!
//! It never follows a link: a link is removed, not entered, so a junction planted inside the
//! folder cannot make the clean-up delete what the junction points at. Whatever cannot be deleted,
//! such as a file something holds open, is reported in `leftovers`, not as a failure: a folder
//! that will not go is not a half-swapped tree.
//!
//! `last` names one top-level entry to delete after all the rest. The journal goes last, so that
//! a crash part way leaves a folder that says what it was (a journal in HEALTHY or ROLLED_BACK)
//! and not an anonymous heap.

use crate::fsops::{EntryKind, FsOps};
use katana_core::path_text::path_from_utf8;
use std::path::{Path, PathBuf};

/// How many folders deep remove_tree goes. A transaction folder is new/<top>/<path>, and the path
/// rules (paths.rs) keep a manifest path far shallower than this.
pub const REMOVE_TREE_MAX_DEPTH: i32 = 64;

/// Deletes `directory` and everything below it, `last` (a top-level name, empty for none) after
/// the rest. The C++'s defaults are an empty `last` and depth 0, which is where this starts.
pub fn remove_tree(fs: &dyn FsOps, directory: &Path, leftovers: &mut Vec<PathBuf>, last: &str) {
    remove_tree_at(fs, directory, leftovers, last, 0);
}

fn remove_tree_at(
    fs: &dyn FsOps,
    directory: &Path,
    leftovers: &mut Vec<PathBuf>,
    last: &str,
    depth: i32,
) {
    let info = match fs.stat(directory) {
        Ok(info) => info,
        Err(_) => {
            leftovers.push(directory.to_path_buf());
            return;
        }
    };
    if info.kind == EntryKind::Missing {
        return;
    }
    if info.kind != EntryKind::Directory {
        if fs.remove_file(directory).is_err() {
            leftovers.push(directory.to_path_buf());
        }
        return;
    }
    if info.reparse_point {
        if fs.remove_empty_directory(directory).is_err() && fs.remove_file(directory).is_err() {
            leftovers.push(directory.to_path_buf());
        }
        return;
    }
    // Deeper than any install is: a folder somebody planted to make the clean-up recurse until
    // the stack is gone, in the first lines of every program. It is left, and reported.
    if depth >= REMOVE_TREE_MAX_DEPTH {
        leftovers.push(directory.to_path_buf());
        return;
    }
    let children = match fs.list_directory(directory) {
        Ok(children) => children,
        Err(_) => {
            leftovers.push(directory.to_path_buf());
            return;
        }
    };
    let before = leftovers.len();
    let last_name = path_from_utf8(last);
    let mut deferred: Option<PathBuf> = None;
    for child in children {
        if !last.is_empty() && child.file_name() == Some(last_name.as_os_str()) {
            deferred = Some(child);
            continue;
        }
        remove_tree_at(fs, &child, leftovers, "", depth + 1);
    }
    // The journal goes only when everything else has: while a file is still held, it stays and
    // keeps saying what the folder is.
    if let Some(deferred) = deferred
        && leftovers.len() == before
    {
        remove_tree_at(fs, &deferred, leftovers, "", 0);
    }
    if fs.remove_empty_directory(directory).is_err() {
        leftovers.push(directory.to_path_buf());
    }
}
