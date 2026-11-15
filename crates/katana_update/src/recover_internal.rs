//! What apply.rs and recover.rs share, and what the tests reach: INTERNAL to the update library
//! (the C++'s private header recover_internal.hpp, namespace `detail`), public only so that the
//! tests can call it. The functions it declares that are defined beside the code they share
//! (recover_locked, recover_with, settle_pending, forget_settled, confirm_started_with in
//! recover.rs; tree_is_release in apply.rs) are re-exported here, so that this module is the one
//! place they are reached by.
//!
//! AN UPDATE THAT IS INSTALLED AND WAS NEVER SAID TO START. A helper that dies between its
//! COMMITTED_PENDING_HEALTH record and its HEALTHY one (power, a shutdown, the process ended)
//! leaves a tree that is a whole new version and a folder that nothing ever finished. recover()
//! leaves both alone (recover.rs). What settles them is ONE rule, settle_pending, with three
//! callers that differ only in what they may do: `katana_updater --recover` (settle_unproved,
//! recover.rs), the window of the new version (confirm_started_with) and the next swap (step 1 of
//! apply.rs).
//!
//! settle_pending, THE RULE. For every folder under R/.update whose journal says
//! COMMITTED_PENDING_HEALTH, but the one named `except` (the caller's own). THE CALLER HOLDS THE
//! UPDATE LOCK, which is how it knows that no helper is alive and waiting on any of them.
//!
//! ```text
//! HEALTHY      when U/healthy-<tx> is a file - the proof the swap itself accepts (apply.rs, step
//!              7), whoever wrote it. For an installer-kind install the version Settings > Apps
//!              shows is rewritten (the swap does that only once the proof is there, and it may
//!              have died before); then HEALTHY is appended, one flushed record; then the folder
//!              is deleted, its journal last.
//! ROLLED_BACK  when `may_roll_back`, and there is no marker, and the tree is still that release
//!              (tree_is_release, with the `toSequence` of the journal's PLAN record), and the
//!              process scan - asked before the hashing, and again as the last thing - answers
//!              and names nobody but this process: rollback() (apply.rs), as it is. Two more
//!              things are looked at first, because "no marker" and "put back" are claims too.
//!              `state_directory` must be the folder the update was applied from - it holds the
//!              dead helper's copy, U/run-<tx> - or a marker missing from it says nothing
//!              (KATANA_UPDATE_STATE_DIR set for Katana and not for this run; another account).
//!              And every file the PLAN's REPLACE and REMOVE records moved aside must still be a
//!              file under <tx>/old, or the rollback would put back some of the old version and
//!              call the mix whole.
//! left         in every other case - a marker that cannot be looked for, a scan that fails, a
//!              tree somebody changed, a program of the install running - and then NOTHING is
//!              written: not a journal record, not a file, not a registry value. (One case writes
//!              a value: a marker that IS there and a HEALTHY record that cannot be appended. The
//!              version Settings > Apps shows was rewritten before the append, and it names what
//!              is installed either way.)
//! ```
//!
//! A folder whose journal could not be READ is none of the three: what it holds is not known, so
//! nothing is done to it, and the outcome says `unread` - it may be such an update, and no caller
//! may take the pass for one that saw everything. `state_directory` (U, or empty: then there is no
//! evidence, and nothing is decided) is only READ. Starts no program. Panics with nothing but a
//! FaultFs's SimulatedCrash.
//!
//! recover_locked puts back every transaction folder under R/.update that a crash (or a rollback
//! that could not finish) left unfinished, and sweeps the folders of the finished ones - except
//! the one named `except` (the caller's own, which may not exist yet). The CALLER HOLDS THE UPDATE
//! LOCK. `state_directory` (U, or empty) is where what a lock keeps from being deleted is queued.
//! An error is the first rollback that failed; the others were still tried. A folder whose journal
//! says COMMITTED_PENDING_HEALTH is neither: it is not looked at there (settle_pending is what
//! decides one).
//!
//! recover_with is recover() (recover.rs) with its seams given: the file system, the launcher that
//! runs the helper copy, the state folder U, how long a live holder of the lock is waited for, and
//! whether the calling program was started as administrator (process.rs, process_is_elevated:
//! recover() hands its answer on, a test says which). `state_directory` (U, or empty) is only
//! READ, to find the helper copy U/run-<tx>/: nothing in it is made, changed or deleted, whichever
//! program calls. With `elevated` the copy in U is not even looked for: U is a folder the same
//! person's programs without administrator rights can write, and nothing is run with those rights
//! from it. The install's own R/bin/katana_updater is run, or - where that is not there - the
//! answer is NotFound with the command to run by hand.
//!
//! forget_settled is what a caller that MAY write the per-user folder does once the rule has
//! spoken: for each transaction settled HEALTHY its marker U/healthy-<tx> and the helper copy
//! U/run-<tx> are deleted (the helper that ran from the copy is dead, and nothing starts it again),
//! and what would not go, with `outcome.leftovers`, is put on U/cleanup.list for the window's next
//! start.
//!
//! confirm_started_with is confirm_started (recover.rs) with its seams given.
//!
//! tree_is_release says whether the tree under `root` is still the release whose manifest
//! sequence is `sequence` (apply.rs says how it is judged).

use crate::command_line::command_line;
use katana_core::path_text::path_to_utf8;
use katana_core::{Error, ErrorCode};
use std::path::{Path, PathBuf};

pub use crate::apply::tree_is_release;
pub use crate::recover::{
    confirm_started_with, forget_settled, recover_locked, recover_with, settle_pending,
};

/// The install folder as every comparison in the swap wants it: lexically normal and without a
/// trailing separator. `C:\x\R\` (a shell's tab completion adds the backslash) and `C:\x\R` name
/// one folder, and a walk up from a file towards the root stops only at the second spelling. An
/// empty path stays empty; a drive root keeps its separator.
pub fn normalized_root(root: &Path) -> PathBuf {
    if root.as_os_str().is_empty() {
        return PathBuf::new();
    }
    let normal = crate::install::bare(root);
    // std::filesystem's lexically_normal: a path that normalises to nothing is ".".
    if normal.as_os_str().is_empty() {
        return PathBuf::from(".");
    }
    normal
}

/// The last component of `path` as UTF-8 text, or nothing when it is not text: a name with an
/// unpaired surrogate, which the file systems of Windows allow, cannot be converted. Every name
/// read from a folder somebody else can write goes through this, so one odd name cannot stop a
/// program from starting. Bytes, as the C++'s pathToUtf8 gives a POSIX name: whatever bytes it
/// holds, which the rules that read it then refuse or accept.
pub fn name_as_text(path: &Path) -> Option<Vec<u8>> {
    let name = path.file_name().unwrap_or_default();
    #[cfg(unix)]
    {
        Some(path_to_utf8(Path::new(name)))
    }
    #[cfg(not(unix))]
    {
        name.to_str().map(|text| text.as_bytes().to_vec())
    }
}

/// `text` on one line: a control character (a line feed in a path that came from a journal)
/// becomes %XX, so that a line written to a log cannot be made to look like two.
pub fn one_line(text: impl AsRef<[u8]>) -> Vec<u8> {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let text = text.as_ref();
    let mut out = Vec::with_capacity(text.len());
    for &byte in text {
        if byte < 0x20 || byte == 0x7F {
            out.push(b'%');
            out.push(HEX[usize::from(byte >> 4)]);
            out.push(HEX[usize::from(byte & 0x0F)]);
        } else {
            out.push(byte);
        }
    }
    out
}

// A path in the platform's preferred separators, as text (std::filesystem's make_preferred, then
// pathToUtf8).
fn preferred_text(path: &Path) -> String {
    let text = String::from_utf8_lossy(&path_to_utf8(path)).into_owned();
    #[cfg(windows)]
    {
        text.replace('/', "\\")
    }
    #[cfg(not(windows))]
    {
        text
    }
}

/// The RunOnce command that runs recovery at the next logon: `"<helper>" --recover <R>`, both
/// paths with the native separators, because it is the shell that parses it then. The ONE place it
/// is written: the swap registers it when its journal says COMMITTING (apply.rs), and
/// settle_unproved registers it again before it settles anything (recover.rs) - Windows takes the
/// entry away before it runs the command, so the run that is to keep the logon net has to put the
/// very same text back.
pub fn run_once_command(helper: &Path, root: &Path) -> String {
    command_line(
        &preferred_text(helper),
        &[String::from("--recover"), preferred_text(root)],
    )
}

/// A state.json that could not be read, and whether the helper may write the defaults over it (it
/// records a release whose health check failed). One that is not JSON of ours (ParseFailure) is
/// carried on from with the defaults, as state.rs says: it is already lost. Any other failure to
/// read it - a scanner holds the file for a moment, the disk hiccups - is no reason to write the
/// defaults over the person's "automatic" and "skipped"; and a newer schema (Unsupported) is
/// another version's file.
pub fn may_replace_unreadable_state(error: &Error) -> bool {
    error.code == ErrorCode::ParseFailure
}

/// What settle_pending did, by transaction id, in the order of the names.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PendingOutcome {
    /// Proved: HEALTHY written, its folder deleted.
    pub healthy: Vec<String>,
    /// Never proved and put back: the tree is the old version's.
    pub rolled_back: Vec<String>,
    /// Could not be decided now: it is as it was.
    pub left: Vec<String>,
    /// The rollback did not finish, or could not begin: put back in part, or not at all.
    pub incomplete: Vec<String>,
    /// What a lock kept from being deleted under R/.update (each such folder keeps its journal,
    /// which says HEALTHY, so every later start sweeps it: remove_tree.rs). Reported, not queued:
    /// of the three callers one may not write the per-user folder.
    pub leftovers: Vec<PathBuf>,
    /// The pass could not look at every folder: R/.update could not be listed, a transaction's
    /// folder could not be examined or its journal is there and would not be read (a scanner or a
    /// backup has it open), or the pass was cut short. The lists above are then what it got to,
    /// and an empty `left` is NOT "none is left": a caller that goes on only when everything is
    /// settled must not go on.
    pub unread: bool,
}
