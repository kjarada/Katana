//! Finishing what a crash interrupted. recover(R) is the FIRST thing main() does in katana,
//! katana_cli and katana_mcp, and what `katana_updater --recover R` does (the RunOnce entry points
//! there too), so a machine that lost power or a helper that was killed in the middle of a swap is
//! put right the next time anything starts - or by hand, when the program itself cannot start
//! because its own files are half swapped.
//!
//! * A live holder of R/.update/lock: an update is running now. Wait for it up to
//!   [`RECOVER_WAIT`], then return (it finished, or it is not ours to touch).
//! * No holder and a journal in state Planned, Committing or RollingBack (journal.rs,
//!   needs_recovery): run the helper copy (U/run-<tx>/, else R/bin/katana_updater) with
//!   `--recover R`, which calls recover_in_process. A PROGRAM STARTED AS ADMINISTRATOR
//!   (process.rs, process_is_elevated) runs R/bin/katana_updater and never the copy: U is a folder
//!   the same person's programs without those rights can write, and no helper is run with
//!   administrator rights from it - the reason `UPDATE APPLY CONFIRM` is refused there
//!   (src/katana_app/update_verbs.hpp, reason=elevated). Where the install's own helper is not
//!   there either, the answer is the error that names the command to run by hand.
//! * Otherwise nothing: a cheap check (one directory listing) that costs a start no measurable
//!   time when there is nothing to do.
//!
//! A recovery that fails does not end in silence: the Error's message names the command to run by
//! hand, `katana_updater --recover <R>`, and the apply's last.json says "rollback_incomplete"
//! (state.rs) so the next start of the window shows it.
//!
//! recover_in_process is the work itself, rolling every interrupted transaction back from the disk
//! (apply.rs, step 6); the helper's --recover mode calls it and the fault loops call it with a
//! clean FsOps after a simulated crash, and again after a crash INSIDE a recovery.
//!
//! AN UPDATE THAT IS INSTALLED AND WAS NEVER SAID TO START. A helper that dies while the new
//! version is proving that it starts (apply.rs, step 7: power, a shutdown, the process ended)
//! leaves a journal that says COMMITTED_PENDING_HEALTH for good. That is none of the states above:
//! the tree is a whole version, so recover() and recover_in_process leave it exactly as it is,
//! take no lock for it and start nothing - which is what keeps the health run's own recover() from
//! waiting on, or starting again, the helper that started it. Three other callers settle it, each
//! under the update lock, which a helper that is alive and waiting holds:
//!
//! ```text
//! settle_unproved   `katana_updater --recover R` - the RunOnce entry at the next logon, or a
//!                   person. Healthy when the marker U/healthy-<tx> is there; else PUT BACK, when
//!                   the tree is still that release, its old files are all kept, U is the folder
//!                   the update was applied from and no program of the install is running; else
//!                   left for a start that can decide.
//! confirm_started   the window of the new version, on its first idle turn: it IS the proof, so
//!                   it writes the marker and settles it healthy.
//! the next swap     step 1 of apply.rs: healthy when the marker is there; otherwise no new update
//!                   is started over one that is not settled ("interrupted_update").
//! ```
//!
//! None of them starts a program. What they may each write is said at the function.
//!
//! What it does with a transaction folder depends on where its journal stands:
//!
//! ```text
//! Empty, Planned, Committing, RollingBack   the crash left it half done: rolled back.
//! CommittedPendingHealth                    the new version is in place and not yet proved: left
//!                                           as it is by recover() and by recover_in_process -
//!                                           the tree is a whole version, and the helper that owns
//!                                           the folder may be alive and waiting for the very
//!                                           program that is asking. When that helper died, the
//!                                           folder is settled by the second half of this module
//!                                           (settle_pending and its callers), never by recover().
//! RolledBack, Healthy                       over. The tree is whole; what is left of the folder
//!                                           (a crash part way through deleting it, a file a
//!                                           scanner held at the time) is swept, so a crash in the
//!                                           clean-up does not leak the copies of a release for
//!                                           ever. recover() sweeps these itself, without starting
//!                                           the helper: there is no tree to put back.
//! a journal that is there and cannot be     not known to be any of these. recover() and
//! read (a scanner, a backup, the disk)      recover_in_process pass over it - they can do no
//!                                           more, and a start is not held up - but it is NOT
//!                                           taken for "no such folder": to the settlers of the
//!                                           second half it is an update that may be there.
//! ```
//!
//! recover()'s cost when there is nothing to do is the point: one stat of R/.update, and a listing
//! and one small file per transaction folder in it - nothing when the folder is absent, which is
//! every install that has never updated. (And one question of the process's own token, which
//! touches no disk: whether this program was started as administrator, for then the helper copy in
//! the per-user folder is not the one it runs.)
//!
//! recover() is called by katana_cli and katana_mcp too, and a headless program writes no per-user
//! state ("WHAT RECOVERY WRITES, AND WHERE", at [`recover`]). So the state folder U is READ here,
//! to find the helper copy U/run-<tx>/, and nothing in it is made, changed or deleted: the copy
//! that was run stays where it is, and what a sweep cannot delete is not queued in U/cleanup.list.
//! A folder under R/.update that would not go needs no list: it keeps its journal (remove_tree.rs)
//! and is found and swept again at the next start. The copy is left for the program that sweeps U:
//! the window a person uses, whose start lists it for removal from what the install's own
//! R/.update/recovery.log names (UpdateWorkbench::start, src/katana_qt/update_workbench.cpp). Until
//! that start one helper's size stays in U for each update that needed a recovery - one that a
//! crash interrupted, and one whose rollback failed and which `--recover` put right later (the
//! swap's finish, apply.rs, keeps that copy off cleanup.list because the RunOnce entry runs
//! `--recover` from it).
//!
//! Where the C++ catches a std::exception, the port catches a panic and lets a SimulatedCrash
//! through (apply.rs, catch_unexpected).

use crate::apply::{
    ApplyServices, ApplyTimings, catch_unexpected, installed_program, is_valid_transaction_id,
    rollback,
};
use crate::fsops::{EntryKind, FileLock, FsOps, RetryPolicy, make_system_fs_ops, path_text};
use crate::install::{InstallKind, read_install, update_work_directory};
use crate::journal::{
    Journal, JournalOp, JournalRecord, JournalState, needs_recovery, parse_journal,
};
use crate::paths::check_journal_path;
use crate::process::{
    Launcher, ProcessOps, RunningProgram, SpawnRequest, SpawnResult, make_system_launcher,
    process_is_elevated,
};
use crate::recover_internal::{PendingOutcome, name_as_text, normalized_root, run_once_command};
use crate::registry::{RUN_ONCE_NAME, RegistryOps, make_system_registry_ops};
use crate::remove_tree::remove_tree;
use crate::state::{append_cleanup, health_marker_path, run_path, state_directory};
use crate::trust::Trust;
use crate::version;
use katana_core::error::make_error;
use katana_core::path_text::path_from_utf8;
use katana_core::{Error, ErrorCode, Result, Status};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const RECOVER_WAIT: Duration = Duration::from_secs(20);

const POLL: Duration = Duration::from_millis(250);
const HELPER_RUN_LIMIT: Duration = Duration::from_secs(120);
const HELPER_STEM: &str = "katana_updater";
const JOURNAL_NAME: &str = "journal";
const OLD_NAME: &str = "old";
const LOCK_NAME: &str = "lock";

// What to run by hand, in every message that says recovery did not finish.
fn by_hand(root: &Path) -> String {
    format!("run: katana_updater --recover \"{}\"", path_text(root))
}

fn is_held(error: &Error) -> bool {
    error.code == ErrorCode::InvalidState && error.message == "held"
}

#[derive(Clone, Debug)]
struct Found {
    name: String,
    // Where the journal stands - or nothing, when that could not be found out: the folder could
    // not be examined, or its journal is there and would not be read (a scanner or a backup has it
    // open at a logon, the disk would not answer). That is NOT a journal that is absent, which is
    // Empty. Of such a folder nothing is known, so nothing here acts on it and nothing here takes
    // it for a folder that is not there: it may be an update that is not settled.
    state: Option<JournalState>,
}

// A folder with no complete record at all is what a crash during planning leaves: copies and
// nothing moved. It is junk to clear, and counts as work.
fn needs_work(one: &Found) -> bool {
    one.state
        .is_some_and(|state| state == JournalState::Empty || needs_recovery(state))
}

fn is_finished(one: &Found) -> bool {
    one.state == Some(JournalState::RolledBack) || one.state == Some(JournalState::Healthy)
}

// Installed, and never said to start (the second half of this module).
fn is_unproved(one: &Found) -> bool {
    one.state == Some(JournalState::CommittedPendingHealth)
}

// Could not be looked at (see Found): it may be any of the three above.
fn is_unread(one: &Found) -> bool {
    one.state.is_none()
}

// The transaction folders under R/.update, with where each journal stands, in the order of their
// names. A folder that could not be read is among them, and says so (Found::state).
fn transactions(fs: &dyn FsOps, root: &Path) -> Result<Vec<Found>> {
    let directory = update_work_directory(root);
    let info = fs.stat(&directory)?;
    if info.kind != EntryKind::Directory || info.reparse_point {
        return Ok(Vec::new());
    }
    let entries = fs.list_directory(&directory)?;
    let mut found: Vec<Found> = Vec::new();
    for entry in &entries {
        let name = name_as_text(entry).unwrap_or_default();
        if !is_valid_transaction_id(&name) {
            // The lock, the recovery log, anything that is not a transaction's folder - and a name
            // that is not even text.
            continue;
        }
        // Lower-case hexadecimal, checked above.
        let name = String::from_utf8_lossy(&name).into_owned();
        let folder = match fs.stat(entry) {
            Ok(folder) => folder,
            Err(_) => {
                found.push(Found { name, state: None }); // it cannot be examined: see Found
                continue;
            }
        };
        if folder.kind != EntryKind::Directory || folder.reparse_point {
            continue;
        }
        let state = match fs.read_file(&entry.join(JOURNAL_NAME)) {
            Ok(text) => Some(parse_journal(text).state),
            Err(error) if error.code == ErrorCode::NotFound => Some(JournalState::Empty),
            // There, and not to be read just now. Reported, not dropped: dropped, an update that
            // was never said to start read as "this install holds none" - to the logon's
            // `--recover`, which then left its RunOnce entry away for good, and to the next swap,
            // which then planned a new update over it.
            Err(_) => None,
        };
        found.push(Found { name, state });
    }
    found.sort_by(|a, b| a.name.as_str().cmp(b.name.as_str()));
    Ok(found)
}

// Only the ones recovery has something to do with (see the top of the module): the ones to put
// back and the ones to sweep. An update that is installed and not yet proved is not among them.
// Nor is a folder that could not be read: neither recover() nor the helper it would start can do
// more with one, and a start is not held up by it. (It is the settlers of the second half of this
// module that must not take it for nothing: settle_pending and its callers.)
fn actionable(fs: &dyn FsOps, root: &Path) -> Result<Vec<Found>> {
    let mut all = transactions(fs, root)?;
    all.retain(|one| needs_work(one) || is_finished(one));
    Ok(all)
}

fn any_needs_work(found: &[Found]) -> bool {
    found.iter().any(needs_work)
}

fn any_unproved(found: &[Found]) -> bool {
    found.iter().any(is_unproved)
}

fn any_unread(found: &[Found]) -> bool {
    found.iter().any(is_unread)
}

// Whether a look found nothing for a settler to do: it could be made, and it shows no update that
// was never said to start and no folder that could not be read. A look that FAILED is not that,
// and neither is a folder whose journal would not be read: "could not look" is "it may hold one",
// for whoever must not leave such an update without somebody to come back to it.
fn holds_none_to_settle(looked: &Result<Vec<Found>>) -> bool {
    matches!(looked, Ok(found) if !any_unproved(found) && !any_unread(found))
}

// Deletes a folder through FsOps; what cannot be deleted now (a file something holds open) is
// queued for the next start when there is a state folder to queue it in. Only the helper run for
// the window gives one (apply.rs, with its `--state U`): recover() and `--recover` give an empty
// path, and write nothing per-user.
fn sweep(fs: &dyn FsOps, folder: &Path, state_directory: &Path, last: &str) {
    let mut leftovers: Vec<PathBuf> = Vec::new();
    remove_tree(fs, folder, &mut leftovers, last);
    if !state_directory.as_os_str().is_empty() {
        for leftover in &leftovers {
            let _ = append_cleanup(state_directory, leftover, fs);
        }
    }
}

// ---- what the rule asks -----------------------------------------------------------------------

// Whether the new version of a transaction said that it starts: the marker U/healthy-<tx>
// (state.rs), judged as the swap judges it (apply.rs, the health check: it is a file). Unknown is
// neither answer - no state folder to look in, a folder that cannot be examined, something that is
// not a file under the marker's name - and decides nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Proof {
    There,
    Absent,
    Unknown,
}

fn proof_of(fs: &dyn FsOps, state_directory: &Path, transaction: &str) -> Proof {
    if state_directory.as_os_str().is_empty() {
        return Proof::Unknown;
    }
    match fs.stat(&health_marker_path(state_directory, transaction)) {
        Ok(info) if info.kind == EntryKind::File => Proof::There,
        Ok(info) if info.kind == EntryKind::Missing => Proof::Absent,
        _ => Proof::Unknown,
    }
}

// What the rule reads in a transaction's journal before it puts anything back.
struct Planned {
    // The sequence of the release the transaction installed: the third argument of the PLAN record
    // (journal.rs), read as a whole number and as nothing else.
    sequence: u64,
    // The files the commit moved aside, to old/<path>: the paths of the REPLACE and REMOVE
    // records. They are what a rollback puts back.
    set_aside: Vec<Vec<u8>>,
}

// A decimal number and nothing else, as std::from_chars reads one into a u64 that must take the
// whole text.
fn whole_number(text: &[u8]) -> Option<u64> {
    if text.is_empty() || !text.iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(text).ok()?.parse::<u64>().ok()
}

// The journal is not authenticated, and nothing here acts on it. All the number can do is fail to
// match the signed manifest in the tree; all a path is used for is to look whether old/<path> is a
// file; and a record the path rules refuse (paths.rs, check_journal_path - the rule a rollback
// holds the same records to) makes the whole reading nothing. With nothing read, nothing is rolled
// back.
fn planned_by(fs: &dyn FsOps, work: &Path) -> Option<Planned> {
    let text = fs.read_file(&work.join(JOURNAL_NAME)).ok()?;
    let journal = parse_journal(text);
    let first = journal.records.first()?;
    if first.op != JournalOp::Plan || first.args.len() != 3 {
        return None;
    }
    let sequence = whole_number(&first.args[2])?;
    let mut set_aside: Vec<Vec<u8>> = Vec::new();
    for record in &journal.records {
        if record.op != JournalOp::Replace && record.op != JournalOp::Remove {
            continue;
        }
        if record.args.len() != 1 || check_journal_path(&record.args[0]).is_err() {
            return None;
        }
        set_aside.push(record.args[0].clone());
    }
    Some(Planned {
        sequence,
        set_aside,
    })
}

// Whether every file the commit moved aside is still in the transaction's folder, an ordinary file
// under old/. A rollback puts back what is there (apply.rs, restore_from_disk), so with one of them
// gone - a clean-up that was cut off, a folder somebody emptied - it would leave the new version of
// that file among the old ones and call the mix "put back".
fn old_files_are_kept(fs: &dyn FsOps, work: &Path, planned: &Planned) -> bool {
    let old = work.join(OLD_NAME);
    planned.set_aside.iter().all(|path| {
        matches!(
            fs.stat(&old.join(path_from_utf8(path))),
            Ok(info) if info.kind == EntryKind::File && !info.reparse_point
        )
    })
}

// Whether `state_directory` is the per-user folder the update was applied from: the helper that
// made the swap ran from a copy in it, U/run-<tx> (state.rs), and that copy stays there until the
// transaction is settled. Where it is not, this run is looking in another folder than the one the
// new version writes its marker to - KATANA_UPDATE_STATE_DIR set for Katana and not for this run,
// another account - and a marker that is absent THERE says nothing about whether the release
// started.
fn is_where_the_update_ran_from(fs: &dyn FsOps, state_directory: &Path, transaction: &str) -> bool {
    matches!(
        fs.stat(&run_path(state_directory, transaction)),
        Ok(info) if info.kind == EntryKind::Directory
    )
}

// True when the scan ANSWERS and names no process but this one. A scan that fails is not
// "nobody": nothing is ever put back under a program that might be running.
fn nobody_else_runs(processes: &dyn ProcessOps, root: &Path) -> bool {
    let Ok(scanned) = processes.holders_under(root) else {
        return false;
    };
    let current = processes.current_pid();
    scanned.iter().all(|program| program.pid == current)
}

// The rule for one transaction (recover_internal.rs, settle_pending).
fn settle_one(
    root: &Path,
    services: &ApplyServices<'_>,
    name: &str,
    state_directory: &Path,
    may_roll_back: bool,
    outcome: &mut PendingOutcome,
) {
    let fs = services.fs;
    let work = update_work_directory(root).join(path_from_utf8(name));
    let proof = proof_of(fs, state_directory, name);

    if proof == Proof::There {
        // Settings > Apps names the new version only once it has proved it starts (the swap's
        // run, apply.rs): the helper that died may not have got that far, and writing the same
        // two values again changes nothing.
        if let Ok(install) = read_install(root, services.registry)
            && install.kind == InstallKind::Installer
        {
            let version = version::to_string(&install.version);
            let _ = services
                .registry
                .set_installed_version(&version, &format!("Katana {version}"));
        }
        let mut journal = Journal::new(fs, work.join(JOURNAL_NAME));
        let healthy = JournalRecord {
            op: JournalOp::Healthy,
            args: Vec::new(),
        };
        if journal.append(&[healthy]).is_err() {
            // Not on disk, so not settled: the next start decides again, from the same marker.
            outcome.left.push(String::from(name));
            return;
        }
        // From here any start sweeps what a crash leaves of the folder (the top of the module).
        remove_tree(fs, &work, &mut outcome.leftovers, JOURNAL_NAME);
        outcome.healthy.push(String::from(name));
        return;
    }

    if !may_roll_back || proof != Proof::Absent {
        outcome.left.push(String::from(name));
        return;
    }
    // "No marker" is evidence only in the folder the marker would have been written to, and a
    // rollback is whole only while every file it puts back is still kept: without either the
    // update is left as it is. Both are looked at, not assumed.
    let planned = match planned_by(fs, &work) {
        Some(planned)
            if is_where_the_update_ran_from(fs, state_directory, name)
                && old_files_are_kept(fs, &work, &planned) =>
        {
            planned
        }
        _ => {
            outcome.left.push(String::from(name));
            return;
        }
    };
    // The scan is asked twice: hashing a whole install takes seconds, and a program started in
    // them must not have the files moved from under it.
    if !nobody_else_runs(services.processes, root)
        || !crate::apply::tree_is_release(root, planned.sequence, services)
        || !nobody_else_runs(services.processes, root)
    {
        outcome.left.push(String::from(name));
        return;
    }
    if rollback(root, name, services).is_err() {
        // Either way somebody comes back to it. A rollback moves nothing from this state until its
        // ROLLING_BACK record is on disk (apply.rs, the rollback): when it could not begin - the
        // record or the folder would not be written or read - the tree is still the whole new
        // version and the next `--recover` decides again; once the record is there every start
        // finishes it, as it finishes any rollback that was cut off (journal.rs, needs_recovery).
        outcome.incomplete.push(String::from(name));
        return;
    }
    outcome.rolled_back.push(String::from(name));
}

// Whether any folder under R/.update still has a transaction's name, whatever is in it. A listing
// that fails is "yes": the RunOnce entry is not removed on a guess.
fn any_transaction_folder(fs: &dyn FsOps, root: &Path) -> bool {
    let directory = update_work_directory(root);
    let Ok(info) = fs.stat(&directory) else {
        return true;
    };
    if info.kind != EntryKind::Directory || info.reparse_point {
        return false;
    }
    let Ok(entries) = fs.list_directory(&directory) else {
        return true;
    };
    for entry in &entries {
        let name = name_as_text(entry).unwrap_or_default();
        if !is_valid_transaction_id(name) {
            continue;
        }
        if !matches!(fs.stat(entry), Ok(folder) if folder.kind == EntryKind::Missing) {
            return true;
        }
    }
    false
}

// ---- the shared pieces (recover_internal.rs) ----------------------------------------------------

/// Puts back every transaction folder under R/.update that a crash (or a rollback that could not
/// finish) left unfinished, and sweeps the folders of the finished ones, but `except`
/// (recover_internal.rs says the rest). Reached by the tests through recover_internal.
#[doc(hidden)]
pub fn recover_locked(
    root_given: &Path,
    services: &ApplyServices<'_>,
    except: &str,
    state_directory: &Path,
) -> Status {
    let fs = services.fs;
    let root = normalized_root(root_given);
    let pending = actionable(fs, &root)?;
    let mut failure: Option<Error> = None;
    for one in &pending {
        if one.name == except {
            continue;
        }
        if is_finished(one) {
            sweep(
                fs,
                &update_work_directory(&root).join(path_from_utf8(&one.name)),
                state_directory,
                JOURNAL_NAME,
            );
            continue;
        }
        if let Err(error) = rollback(&root, &one.name, services) {
            failure.get_or_insert(error);
        }
    }
    match failure {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// THE RULE (recover_internal.rs says it whole). Reached by the tests through recover_internal.
#[doc(hidden)]
pub fn settle_pending(
    root_given: &Path,
    services: &ApplyServices<'_>,
    except: &str,
    state_directory: &Path,
    may_roll_back: bool,
) -> PendingOutcome {
    let mut outcome = PendingOutcome::default();
    let root = normalized_root(root_given);
    if root.as_os_str().is_empty() {
        return outcome; // ".update" of the current folder is not an install's
    }
    // An unexpected error - a name that is not text, a conversion - ends the pass with what was
    // settled until then: whatever it did not reach is as it was, and a later start comes back to
    // it. (A FaultFs's SimulatedCrash goes through.) A pass that was cut short, like one whose
    // listing failed, says so: `unread` is not "none left".
    let passed = catch_unexpected(|| {
        let Ok(all) = transactions(services.fs, &root) else {
            outcome.unread = true;
            return;
        };
        for one in &all {
            if one.name == except {
                continue;
            }
            if is_unread(one) {
                // It may be an update that is not settled. Nothing is done to it, and the caller
                // hears what it hears when the listing fails: not everything was seen.
                outcome.unread = true;
            } else if is_unproved(one) {
                settle_one(
                    &root,
                    services,
                    &one.name,
                    state_directory,
                    may_roll_back,
                    &mut outcome,
                );
            }
        }
    });
    if passed.is_err() {
        outcome.unread = true;
    }
    outcome
}

/// For each transaction the rule settled HEALTHY: its marker and its helper copy go, and what
/// would not go is listed in U/cleanup.list (recover_internal.rs). Reached by the tests through
/// recover_internal.
#[doc(hidden)]
pub fn forget_settled(fs: &dyn FsOps, state_directory: &Path, outcome: &PendingOutcome) {
    if state_directory.as_os_str().is_empty() {
        return;
    }
    let mut leftovers = outcome.leftovers.clone();
    for transaction in &outcome.healthy {
        let marker = health_marker_path(state_directory, transaction);
        if fs.remove_file(&marker).is_err() {
            leftovers.push(marker);
        }
        remove_tree(
            fs,
            &run_path(state_directory, transaction),
            &mut leftovers,
            "",
        );
    }
    for leftover in &leftovers {
        let _ = append_cleanup(state_directory, leftover, fs);
    }
}

fn recover_with_unguarded(
    root_given: &Path,
    fs: &dyn FsOps,
    launcher: &dyn Launcher,
    state_directory: &Path,
    lock_wait: Duration,
    elevated: bool,
) -> Status {
    let root = normalized_root(root_given);
    if root.as_os_str().is_empty() {
        // No install folder: nothing to recover, and ".update" of the current folder is not an
        // install's.
        return Ok(());
    }
    let Ok(first) = actionable(fs, &root) else {
        // No way to tell: a start must not be held up by a question it cannot answer.
        return Ok(());
    };
    let lock_path = update_work_directory(&root).join(LOCK_NAME);
    if !any_needs_work(&first) {
        if first.is_empty() {
            return Ok(());
        }
        // Only folders of transactions that are over: no tree to put back, so no helper to run.
        // Swept here, if no update is running, so that a clean-up cut short (a crash, a file a
        // scanner held) does not leave the copies of a release behind for ever.
        let Ok(_taken) = fs.try_lock(&lock_path) else {
            return Ok(()); // an update is running; it is cleaning up after itself
        };
        if let Ok(again) = actionable(fs, &root) {
            for one in again.iter().filter(|one| is_finished(one)) {
                // No state folder to queue in: this may be a headless program, which begins no
                // cleanup.list. What is still held stays, under its journal, and the next start
                // comes here again.
                sweep(
                    fs,
                    &update_work_directory(&root).join(path_from_utf8(&one.name)),
                    Path::new(""),
                    JOURNAL_NAME,
                );
            }
        }
        return Ok(());
    }

    // A live update holds the lock: wait for it, and look again when it lets go.
    let mut pending = first;
    let start = Instant::now();
    loop {
        match fs.try_lock(&lock_path) {
            // Released at once: the helper takes it itself.
            Ok(_taken) => break,
            Err(error) => {
                if !is_held(&error) {
                    return Err(make_error(
                        ErrorCode::InvalidState,
                        format!(
                            "an interrupted update was found and the update lock cannot be \
                             taken; {}",
                            by_hand(&root)
                        ),
                        error.describe(),
                    ));
                }
            }
        }
        if start.elapsed() >= lock_wait {
            return Ok(());
        }
        std::thread::sleep(POLL.min(lock_wait));
        match actionable(fs, &root) {
            Ok(again) if any_needs_work(&again) => pending = again,
            _ => return Ok(()),
        }
    }

    // The helper copy the interrupted update ran from, if it is still there: it is outside R, so
    // it works when R's own bin/ is half swapped. Else the install's own. Looking for the copy is
    // all that is done in the state folder.
    //
    // NOT THE COPY, when this program was started as administrator (`elevated`: process.rs,
    // process_is_elevated). The copy is in the per-user folder, which the same person's programs
    // WITHOUT those rights can write - and so is all that sends this function there: a journal with
    // one PLAN record under R/.update/<tx> (its check is no key's) and a file named katana_updater
    // in U/run-<tx>. Run from here, that file would have administrator rights before this program
    // has done anything else. `UPDATE APPLY CONFIRM` is refused in such a program for the same
    // reason (src/katana_app/update_verbs.hpp, reason=elevated); this is the other way to a helper
    // in U, and it is closed the same way. The install's own helper is no new risk: it is beside
    // the very program that is running with those rights. Where that one is not there (a commit
    // cut off between the two renames of that very file), the answer is the error that names the
    // command to run by hand.
    let helper_file = installed_program(Path::new(""), HELPER_STEM);
    let stem = helper_file.file_name().unwrap_or_default();
    let mut helper = PathBuf::new();
    if !state_directory.as_os_str().is_empty() && !elevated {
        // No state folder means no copies in it - and an elevated program may run none.
        for one in &pending {
            let copy = run_path(state_directory, &one.name).join(stem);
            if matches!(fs.stat(&copy), Ok(info) if info.kind == EntryKind::File) {
                helper = copy;
                break;
            }
        }
    }
    if helper.as_os_str().is_empty() {
        let own = installed_program(&root, HELPER_STEM);
        if matches!(fs.stat(&own), Ok(info) if info.kind == EntryKind::File) {
            helper = own;
        }
    }
    if helper.as_os_str().is_empty() {
        return Err(Error::new(
            ErrorCode::NotFound,
            if elevated {
                format!(
                    "an interrupted update was found, and Katana was started as administrator, so \
                     only the install's own update helper may put it back, and that one is not \
                     there; start Katana the ordinary way, or {}",
                    by_hand(&root)
                )
            } else {
                format!(
                    "an interrupted update was found and there is no update helper to put it back \
                     with; {}",
                    by_hand(&root)
                )
            },
        ));
    }

    let request = SpawnRequest {
        working_directory: helper.parent().unwrap_or(Path::new("")).to_path_buf(),
        executable: helper,
        arguments: vec![
            String::from("--recover"),
            String::from_utf8_lossy(&katana_core::path_text::path_to_utf8(&root)).into_owned(),
        ],
        timeout: HELPER_RUN_LIMIT,
    };
    let ran = launcher.run(&request).map_err(|error| {
        make_error(
            ErrorCode::InvalidState,
            format!(
                "an interrupted update was found and the update helper could not be started; {}",
                by_hand(&root)
            ),
            error.describe(),
        )
    })?;
    if ran.timed_out || ran.exit_code != 0 {
        return Err(make_error(
            ErrorCode::InvalidState,
            format!(
                "an interrupted update could not be put back; {}",
                by_hand(&root)
            ),
            if ran.timed_out {
                String::from("the helper did not finish in time")
            } else {
                format!("the helper exited with code {}", ran.exit_code)
            },
        ));
    }

    // The helper has exited and put everything back. Its copy U/run-<tx>/ is of no use any more,
    // and it STAYS: deleting it, or listing it in U/cleanup.list, would be a headless program
    // changing per-user state ([`recover`]). The window's start lists it (the top of this module
    // says from what).
    Ok(())
}

// The window that confirms has no process scan, no launcher and no keys: it never puts anything
// back (settle_pending is told so), and these stand in the services it hands the rule so that,
// were the rule ever to ask, the answer is a refusal and not a guess.
struct NoProcesses;

impl ProcessOps for NoProcesses {
    fn current_pid(&self) -> u64 {
        0
    }
    fn is_alive(&self, _pid: u64) -> bool {
        true
    }
    fn holders_under(&self, _root: &Path) -> Result<Vec<RunningProgram>> {
        Err(Error::new(
            ErrorCode::Unsupported,
            "a confirmation asks nobody who is running",
        ))
    }
}

struct NoLauncher;

impl Launcher for NoLauncher {
    fn run(&self, _request: &SpawnRequest) -> Result<SpawnResult> {
        Err(Error::new(
            ErrorCode::Unsupported,
            "a confirmation starts no program",
        ))
    }
    fn start_detached(&self, _request: &SpawnRequest) -> Status {
        Err(Error::new(
            ErrorCode::Unsupported,
            "a confirmation starts no program",
        ))
    }
}

fn confirm_started_unguarded(
    root_given: &Path,
    state_directory: &Path,
    fs: &dyn FsOps,
    registry: &dyn RegistryOps,
) -> Confirmed {
    let root = normalized_root(root_given);
    if root.as_os_str().is_empty() || state_directory.as_os_str().is_empty() {
        return Confirmed::Nothing;
    }
    let first = transactions(fs, &root);
    if first.is_err() {
        // Could not look, which is not "there is none": a folder that will not be listed just now
        // may hold the update this window is the proof of. It asks again.
        return Confirmed::Busy;
    }
    if holds_none_to_settle(&first) {
        return Confirmed::Nothing; // every start but the one after a helper died
    }
    // One attempt. A lock that is held is a helper that is alive and doing its own health run (or
    // a scanner, for a moment): the caller asks again, it does not wait.
    let _lock: Box<dyn FileLock> = match fs.try_lock(&update_work_directory(&root).join(LOCK_NAME))
    {
        Ok(lock) => lock,
        Err(error) => {
            return if is_held(&error) {
                Confirmed::Busy
            } else {
                Confirmed::Nothing
            };
        }
    };

    // Under the lock, look again: the helper that held it may have settled its own.
    let pending = transactions(fs, &root);
    let Ok(found) = &pending else {
        return Confirmed::Busy; // as above: not seen is not settled
    };
    if holds_none_to_settle(&pending) {
        return Confirmed::Nothing;
    }
    // The proof first, and on disk before anything is settled by it: this window is at its first
    // idle turn, which is what the health run is started to reach. Cut off after this line, the
    // update is settled healthy by whoever comes next - the logon's `--recover` too, which would
    // otherwise put a working release back. Only for a journal that was READ and says the update
    // is pending: a folder whose journal would not be read may be one a crash cut off in its
    // commit, and this window is no proof of that. The rule below says of such a folder that it
    // was not seen, and the window asks again.
    for one in found {
        if !is_unproved(one) || proof_of(fs, state_directory, &one.name) != Proof::Absent {
            continue; // written already, by the health run of the helper that died
        }
        if fs.make_directories(state_directory).is_ok() {
            let _ = fs.write_durable(&health_marker_path(state_directory, &one.name), b"ok\n");
        }
    }

    let processes = NoProcesses;
    let launcher = NoLauncher;
    let nobody = Trust::default();
    let services = ApplyServices {
        fs,
        processes: &processes,
        launcher: &launcher,
        registry,
        trust: &nobody,
        timings: ApplyTimings::default(),
    };
    let outcome = settle_pending(&root, &services, "", state_directory, false);
    forget_settled(fs, state_directory, &outcome);
    if !outcome.left.is_empty() || outcome.unread || outcome.healthy.is_empty() {
        // The marker or the record could not be written just now, or the folder could not be
        // read again: something is not settled, or may not be, so the logon entry stays, and the
        // window asks again.
        return Confirmed::Busy;
    }
    // The command it names runs the helper copy that has just been deleted.
    let _ = registry.clear_run_once(RUN_ONCE_NAME);
    Confirmed::Done
}

/// recover() with its seams given (recover_internal.rs says the rest). An unexpected error from a
/// name or a folder somebody else can write becomes an error to report, never a program that
/// dies in its first lines at every start. Reached by the tests through recover_internal.
#[doc(hidden)]
pub fn recover_with(
    root: &Path,
    fs: &dyn FsOps,
    launcher: &dyn Launcher,
    state_directory: &Path,
    lock_wait: Duration,
    elevated: bool,
) -> Status {
    match catch_unexpected(|| {
        recover_with_unguarded(root, fs, launcher, state_directory, lock_wait, elevated)
    }) {
        Ok(status) => status,
        Err(what) => Err(make_error(
            ErrorCode::Internal,
            "looking for an interrupted update stopped on an unexpected error",
            what,
        )),
    }
}

/// confirm_started with its seams given. A window that cannot confirm goes on being a window: an
/// unexpected error is "nothing was done". Reached by the tests through recover_internal.
#[doc(hidden)]
pub fn confirm_started_with(
    root: &Path,
    state_directory: &Path,
    fs: &dyn FsOps,
    registry: &dyn RegistryOps,
) -> Confirmed {
    catch_unexpected(|| confirm_started_unguarded(root, state_directory, fs, registry))
        .unwrap_or(Confirmed::Nothing)
}

// ---- the public answers -------------------------------------------------------------------------

/// How settle_unproved ended. With several such updates under one install - a swap refuses to
/// start beside one, so there is one at most - the answer is the first of RollbackIncomplete,
/// Left, RolledBack, Healthy that fits any of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Settled {
    /// The install holds no such update, and nothing was written. Mostly without the lock being
    /// asked for; also when the helper whose lock was waited for had settled its own by then.
    Nothing,
    /// The marker was there: the update stands (its folder is deleted, or kept under a journal
    /// that says HEALTHY while a file in it is held).
    Healthy,
    /// No marker, and it could be put back: the old version is in place.
    RolledBack,
    /// It could not be decided in this run, and it is as it was: a program of the install is
    /// running; the files are no longer that release's, or the old ones it would be put back from
    /// are not all kept; `evidence_directory` is not the folder the update was applied from (it
    /// holds no U/run-<tx>), so a marker missing there proves nothing; the scan, the marker or the
    /// folder could not be asked; or the lock could not be taken for a reason other than a holder.
    /// Also when it is not even KNOWN whether the install holds one: R/.update would not be
    /// listed, or a transaction's folder or its journal would not be read (a scanner or a backup
    /// has the journal open at the logon) - "could not look" is never "there is none". In each of
    /// those the RunOnce entry is registered, so a logon comes back to it. (Also the answer when
    /// the run stopped on an unexpected error, before anything was done.)
    Left,
    /// The update lock stayed held: a helper is alive. Nothing was written.
    Busy,
    /// Putting it back did not finish, or could not begin (the ROLLING_BACK record could not be
    /// written, and then nothing was moved): the tree may be half swapped.
    RollbackIncomplete,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Confirmed {
    /// Nothing to confirm: the install holds no update that was never said to start (or the lock
    /// file cannot be opened at all, which asking again does not mend).
    Nothing,
    /// It is settled healthy.
    Done,
    /// Not now: the update lock is held - a helper is alive, or a scanner has the lock file for a
    /// moment - or the marker or the record could not be written, or R/.update could not be
    /// listed, or a transaction's folder or journal in it could not be read (at the first look as
    /// at any later one: a folder that was not seen may hold the update). Nothing is settled, the
    /// RunOnce entry is as it was, and the caller asks again later (the window: every 5 s, 12
    /// times).
    Busy,
}

/// The system seams. Returns OK when there was nothing to do, when recovery ran and succeeded, and
/// when a live update holds the lock past the wait (the caller carries on: that is not a failure
/// of recovery). An error only when a recovery was needed and could not be run or did not
/// complete; the message says the by-hand command (`katana_updater --recover <R>`).
///
/// WHAT RECOVERY WRITES, AND WHERE. recover() is called first by katana_cli and katana_mcp too, and
/// a headless program writes no per-user state (docs/headless.md). So the rule, which the helper
/// program keeps and its tests hold it to:
///
/// * `katana_updater --recover <R>` - what this function runs, what the RunOnce entry runs and
///   what a person types - writes NOTHING per-user, whoever started it: no last.json, no
///   state.json, no cleanup.list, no health marker. It READS the per-user folder
///   (state_directory(), so KATANA_UPDATE_STATE_DIR counts) for one thing: whether the marker
///   U/healthy-<tx> of an update that was never said to start is there - and, so that a marker
///   missing from the wrong folder is not taken for one that was never written, whether that
///   update's helper copy U/run-<tx> is (settle_unproved, below) - and only when the install holds
///   such an update. Its only record is in the install itself, R/.update/recovery.log: a line
///   appended for each run that found something to put right (`<UTC time> recover
///   found=<tx,...> result=ok|failed left=<tx,...>`, and `detail=` after a failure), and one for
///   each journal record the rollback would not follow (`<tx> journal record refused: <record>`,
///   the record on one line). A run that found nothing writes nothing, and where R/.update is not
///   a folder of the install itself (a link) nothing is read or written through it. When
///   everything is back it removes the RunOnce entry the interrupted swap left.
///
///   THE FORM OF THOSE LINES IS READ, by the window (the last point below says what for): a
///   transaction is named to it by a whole line whose second word is `recover`, whose third is
///   `found=<tx,...>` and whose fourth begins `result=` - which the line of a refused record, its
///   second word `journal`, never is. Neither form changes without updateRecoveredTransactions
///   (src/katana_qt/update_dialog.cpp) and its tests (tests/qt_widgets/test_update_dialog.cpp).
/// * last.json is written by ONE program in ONE mode: the helper run for the window, the swap
///   itself (`--state U`, apply.rs). A recovery started from katana_cli or katana_mcp therefore
///   never leaves a "the last update ..." message for the window; the window learns which version
///   is installed from the install's own records (install.rs), and from last.json only when the
///   swap's own helper got to write one.
/// * In the calling program this function READS the state folder, to find the helper copy
///   U/run-<tx>/ (it is outside R, so it still runs when R's own bin/ is half swapped; a program
///   started as administrator does not look for it, see the top) - and that is all it does there.
///   It makes, changes and deletes nothing in U, whichever program calls it: a recovery started
///   from katana_cli or katana_mcp leaves the per-user folder byte for byte as it found it, the
///   helper copy it ran included, and no cleanup.list is begun. What a sweep of R/.update/<tx>
///   could not delete is found there again at the next start. The copy stays, here: removing it is
///   for the program that sweeps U, and that is the window a person uses - katana_cli keeps
///   per-user state too, and sweeps nothing. Its start (UpdateWorkbench::start,
///   src/katana_qt/update_workbench.cpp: on Windows, not a headless run, not a development tree)
///   reads THIS install's recovery.log, puts on U/cleanup.list the copy U/run-<tx> of each
///   transaction a recovery found and whose folder R/.update/<tx> is gone, and sweeps the list
///   (listRecoveredCopies there; which transactions, and why the install's log and not the names
///   in U, is updateRecoveredTransactions', src/katana_qt/update_dialog.hpp). Until that start one
///   helper's size is left in U for each update that needed a recovery - which is for as long as
///   only katana_cli and katana_mcp are started. That is an update a crash interrupted (its helper
///   died before it could list its own copy), and equally one whose rollback failed and which
///   `--recover` put right later: the swap's finish (apply.rs) keeps the copy off cleanup.list
///   after a rollback that failed, because the RunOnce entry runs `--recover` from that very copy,
///   and the recovery then lists nothing.
pub fn recover(root: &Path) -> Status {
    match catch_unexpected(|| {
        let fs = make_system_fs_ops(&RetryPolicy::default());
        let launcher = make_system_launcher();
        recover_with(
            root,
            fs.as_ref(),
            launcher.as_ref(),
            &state_directory(),
            RECOVER_WAIT,
            process_is_elevated(),
        )
    }) {
        Ok(status) => status,
        Err(what) => Err(make_error(
            ErrorCode::Internal,
            "looking for an interrupted update stopped on an unexpected error",
            what,
        )),
    }
}

fn recover_in_process_unguarded(root_given: &Path, services: &ApplyServices<'_>) -> Status {
    if root_given.as_os_str().is_empty() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "no install folder to recover: --recover needs one",
            "",
        ));
    }
    let fs = services.fs;
    let root = normalized_root(root_given);
    let first = actionable(fs, &root)?;
    if first.is_empty() {
        return Ok(()); // the cheap answer, without touching the lock
    }

    // A live holder is an update running now: wait for it, never touch its files. The wait is
    // RECOVER_WAIT, shortened by a test through the timings.
    let wait = RECOVER_WAIT.min(services.timings.wait_for_holders);
    let lock_path = update_work_directory(&root).join(LOCK_NAME);
    let start = Instant::now();
    let _lock: Box<dyn FileLock> = loop {
        match fs.try_lock(&lock_path) {
            Ok(lock) => break lock,
            Err(error) => {
                if !is_held(&error) {
                    return Err(make_error(
                        ErrorCode::InvalidState,
                        format!(
                            "an interrupted update was found and the update lock cannot be \
                             taken; {}",
                            by_hand(&root)
                        ),
                        error.describe(),
                    ));
                }
            }
        }
        if start.elapsed() >= wait {
            return Ok(()); // not ours to touch
        }
        std::thread::sleep(services.timings.poll_interval);
    };

    // Under the lock, look again: the holder we waited for may have finished the job.
    if let Err(undone) = recover_locked(&root, services, "", Path::new("")) {
        return Err(make_error(
            ErrorCode::InvalidState,
            format!(
                "an interrupted update could not be put back; {}",
                by_hand(&root)
            ),
            undone.describe(),
        ));
    }
    let _ = services.registry.clear_run_once(RUN_ONCE_NAME);
    Ok(())
}

/// Roll back every transaction under R/.update whose journal needs it, in process. Idempotent. OK
/// when none needed it. A transaction whose journal says COMMITTED_PENDING_HEALTH is not one of
/// them (see the top): it is left as it is, and with nothing else to do no lock is taken.
pub fn recover_in_process(root: &Path, services: &ApplyServices<'_>) -> Status {
    match catch_unexpected(|| recover_in_process_unguarded(root, services)) {
        Ok(status) => status,
        Err(what) => Err(make_error(
            ErrorCode::Internal,
            "looking for an interrupted update stopped on an unexpected error",
            what,
        )),
    }
}

fn settle_unproved_unguarded(
    root_given: &Path,
    helper: &Path,
    evidence_directory: &Path,
    services: &ApplyServices<'_>,
) -> Settled {
    let fs = services.fs;
    let root = normalized_root(root_given);
    if root.as_os_str().is_empty() {
        return Settled::Nothing;
    }
    let first = transactions(fs, &root);
    if holds_none_to_settle(&first) {
        return Settled::Nothing; // the cheap answer, without touching the lock
    }
    // From here the install holds such an update - or MAY: R/.update would not be listed, or a
    // folder in it, or its journal, would not be read (a scanner or a backup at the logon). That
    // is not "none". Started by the RunOnce entry, this run is the last one Windows starts unless
    // the entry is put back; taken for "none", an update whose release does not start would have
    // nobody come back to it. So it goes the way of one that is there: the lock, the entry, the
    // rule - which says that it could not look, and the answer is Left.

    // The logon net: `<helper> --recover <R>`, the very text the swap registered. Best effort, like
    // the swap's own registration. (And one value for every copy of Katana a person runs: the name
    // is RUN_ONCE_NAME whatever R is, so a swap or a confirmation in ANOTHER copy replaces or
    // removes this entry, and this copy's net is then gone until its next `--recover`. Known, and
    // not changed here: the uninstaller removes the value by that name.)
    let rearm = || {
        if !helper.as_os_str().is_empty() {
            let _ = services
                .registry
                .set_run_once(RUN_ONCE_NAME, &run_once_command(helper, &root));
        }
    };

    // The lock is how a dead helper is told from a live one: a helper holds it from its first step
    // to its last, and it goes with the process. Polled as recover_in_process polls it - one
    // attempt would lose to a scanner that has the lock file open for a moment - and a lock that
    // stays held is a health run in progress, which is not this run's to judge.
    let wait = RECOVER_WAIT.min(services.timings.wait_for_holders);
    let lock_path = update_work_directory(&root).join(LOCK_NAME);
    let start = Instant::now();
    let _lock: Box<dyn FileLock> = loop {
        match fs.try_lock(&lock_path) {
            Ok(lock) => break lock,
            Err(error) => {
                if !is_held(&error) {
                    // No lock, so no way to know that nobody is working on it: it is left. But not
                    // without the logon net - a lock that is HELD is a helper that is alive and
                    // whose registry this is not, and this is not that: the lock file cannot be
                    // opened at all (a scanner that answers "access denied", a folder that will not
                    // be written just now). Started by the RunOnce entry, this run is the last one
                    // Windows starts unless the entry is put back, and then nothing would ever come
                    // to settle the update.
                    rearm();
                    return Settled::Left;
                }
            }
        }
        if start.elapsed() >= wait {
            return Settled::Busy;
        }
        std::thread::sleep(services.timings.poll_interval);
    };

    // Under the lock, look again: the helper that was waited for may have settled its own. A look
    // that fails now, or a folder that will not be read, is not "it has": the rule below looks
    // once more and says so.
    let pending = transactions(fs, &root);
    if holds_none_to_settle(&pending) {
        return Settled::Nothing;
    }

    // FIRST the logon net. When this run was started by the RunOnce entry, Windows has taken the
    // entry away already; were the machine to lose power in the lines below, no logon would come
    // back here.
    rearm();

    let outcome = settle_pending(&root, services, "", evidence_directory, true);

    // The entry goes when there is nothing left for a logon to do; a folder that is still there -
    // left undecided, half put back, or held by a scanner while it was deleted - is what the next
    // logon comes for.
    if !any_transaction_folder(fs, &root) {
        let _ = services.registry.clear_run_once(RUN_ONCE_NAME);
    }

    if !outcome.incomplete.is_empty() {
        return Settled::RollbackIncomplete;
    }
    if !outcome.left.is_empty() || outcome.unread {
        return Settled::Left;
    }
    if !outcome.rolled_back.is_empty() {
        return Settled::RolledBack;
    }
    if outcome.healthy.is_empty() {
        Settled::Nothing
    } else {
        Settled::Healthy
    }
}

/// What `katana_updater --recover R` does once recover_in_process has returned OK, with the
/// helper's seams. `helper` is the running helper's own file, and `evidence_directory` the
/// per-user folder U.
///
/// With no folder under R/.update whose journal says COMMITTED_PENDING_HEALTH: Nothing, and the
/// lock is not asked for - when that could be SEEN. Where R/.update would not be listed, or a
/// transaction's folder or journal would not be read, the install may hold such an update, and
/// the run goes on as if it did (the rule then says that it could not look, and the answer is
/// Left, with the RunOnce entry registered). Otherwise the lock is taken, polled the way
/// recover_in_process polls it (RECOVER_WAIT, shortened by the timings); a lock that stays held is
/// Busy, with nothing written, the registry included. A lock that cannot be opened at all is no
/// helper's: Left, with the RunOnce entry registered (step 1) so that a logon comes back. Then:
///
/// 1. FIRST the RunOnce entry is registered again, `<helper> --recover <R>` (the text the swap
///    registered). Windows removes an entry before it runs it, so a run that lost power here would
///    otherwise leave no logon to come back at.
/// 2. Each such transaction is settled by the one rule (recover_internal.rs, settle_pending),
///    rollback allowed. U/healthy-<tx> is a file: the new version said that it starts, HEALTHY is
///    written and the folder deleted (evidence, not assumption: it is the proof the swap itself
///    asks for, without the exit code of the health run, which died with the helper). No marker,
///    and the absence is evidence: U holds the copy the update's helper ran from, U/run-<tx>, so
///    it is the folder the marker would be in; the tree is still that release, every old file the
///    update moved aside is still kept, and no program of the install is running (asked before
///    the files are hashed and again as the last thing): the update is put back by rollback()
///    (apply.rs). Nothing is rolled back under a running program. Otherwise it is left, and
///    nothing is written for it.
/// 3. The RunOnce entry is removed when no transaction folder is left under R/.update. Left, or a
///    rollback that did not finish, keeps it: the next logon comes here again.
///
/// WHAT IT WRITES: the install (the journal, the files a rollback moves, the folder it deletes),
/// the RunOnce value, and - for an installer's install that is settled healthy - DisplayVersion
/// and DisplayName, the version Settings > Apps shows (apply.rs, step 7: the swap writes them only
/// once the proof is there, and it may have died before). U is only READ - the marker stays where
/// it is - so the rule of recover() holds for the headless programs that start `--recover`. And
/// what such a run CANNOT record is NOT recorded: a rollback made here writes no last.json and does
/// not add the release to `blocked` (state.rs), so the next check offers that release again. No
/// apply-and-put-back loop follows from that: nothing applies an update but a person asking for
/// it, and an update a person applies runs under a live helper, whose own health check blocks a
/// release that does not start.
///
/// Starts no program. Idempotent, and safe to cut off anywhere: before HEALTHY is on disk the next
/// run decides again from the same evidence, after it any start sweeps the folder, and a rollback
/// that was cut off is finished by every start (journal.rs, needs_recovery).
pub fn settle_unproved(
    root: &Path,
    helper: &Path,
    evidence_directory: &Path,
    services: &ApplyServices<'_>,
) -> Settled {
    // An unexpected error before anything was decided leaves the update as it was. (One inside a
    // rollback never arrives here: rollback() turns it into its own error.)
    catch_unexpected(|| settle_unproved_unguarded(root, helper, evidence_directory, services))
        .unwrap_or(Settled::Left)
}

/// What the window of a NEW version does on its first idle turn, when its install may hold an
/// update that was never said to start. THE CALLER has made sure that it is that version (the
/// install has a record of its own, and the version it names is the program's own: a window of the
/// OLD files, started during the commit, proves nothing), that the run is interactive, and that
/// the tree is an install: UpdateWorkbench::start, src/katana_qt/update_workbench.cpp.
///
/// A window that has reached its first idle turn is what the health run exists to show, so: with
/// no such update, Nothing. One attempt at the lock - a window does not wait - and Busy when it is
/// held. Else, for each: the marker `state_directory`/healthy-<tx> is written (so a crash from here
/// on is settled healthy by the next `--recover` too), the rule settles it healthy, the marker and
/// the dead helper's copy U/run-<tx> are removed (what stays goes on U/cleanup.list), and the
/// RunOnce entry is removed. It never rolls back, never starts a program, and writes per-user
/// state: it is for an interactive window, never for a headless program.
pub fn confirm_started(root: &Path, state_directory: &Path) -> Confirmed {
    catch_unexpected(|| {
        let fs = make_system_fs_ops(&RetryPolicy::default());
        let registry = make_system_registry_ops();
        confirm_started_with(root, state_directory, fs.as_ref(), registry.as_ref())
    })
    .unwrap_or(Confirmed::Nothing)
}
