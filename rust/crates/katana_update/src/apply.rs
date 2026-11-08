//! The swap: replacing the install's files with the staged ones, and putting every file back if
//! it goes wrong. It runs in the helper program (katana_updater), after the window has closed,
//! and in-process in the tests through the seams ([`FsOps`], [`ProcessOps`], [`Launcher`],
//! [`RegistryOps`]).
//!
//! The algorithm (every step is a test, and the loops of the fault_fs module break it at each
//! file-system operation):
//!
//! 1. LOCK AND VERIFY. Take R/.update/lock (FsOps::try_lock; a live holder: Refused "locked").
//!    Under it, first settle what an earlier swap left: one that a crash cut off is put back, and
//!    one that is in place and was never said to start (below, after step 7) is confirmed when
//!    its marker is there; when either cannot be - or R/.update cannot be read to know - Refused
//!    "interrupted_update" and nothing new starts. Verify the installed manifest's signature and
//!    the staged manifest's (stage.rs: missing or unverified is Refused "staged_manifest") - with
//!    THE HELPER'S OWN KEYS, not the engine's answer: product and target equal install.json's,
//!    is_newer_than (trust.rs) over installed.json's pair, the path rules (paths.rs), the free
//!    space.
//! 2. WAIT for the window (request.wait_pid, 60 s), then for every process with an image under R
//!    (ProcessOps::holders_under). After 30 s: Blocked, naming them; the old version is relaunched
//!    and nothing was touched. NEVER kill. The same question is asked once more in step 4, when
//!    the plan is made.
//! 3. PLAN, changing nothing under R. Hash every manifest file in R. Equal: KEEP. Else the staged
//!    file must exist and hash right, and it is COPIED (not renamed: the stage folder is
//!    user-writable, and a file in R must take R's ACL) into R/.update/<tx>/new/<path>. Every
//!    parent must not be a reparse point and must resolve under R. REMOVE only a file listed in
//!    installed.manifest.json, absent from the new manifest, whose disk hash equals the old
//!    manifest's; a file nobody listed is never touched. Any shortfall aborts with R untouched.
//! 4. JOURNAL. Append the PLAN group (journal.rs) with one flush. Then step 2's SECOND LOOK: the
//!    plan hashed and copied for seconds, and a program started in them - Katana opened again, a
//!    script's katana_cli - would go on running old code over the new tree. One scan and no wait;
//!    a holder now is the Blocked of step 2: the transaction's folder is deleted, the install is as
//!    it was, and the stage (the download) stays for the Retry. Only then COMMITTING, and the
//!    RunOnce entry (RegistryOps) is registered for `<run copy>\katana_updater --recover R`.
//! 5. COMMIT, renames only: the classes in the order data, lib, program, then the helper's own
//!    file, then installed.json and installed.manifest.json(.sig). Each file: R/p to
//!    .update/<tx>/old/p, then .update/<tx>/new/p to R/p. Errors 5, 32, 33 retry inside
//!    FsOps::rename_file.
//! 6. ROLLBACK (a failure, a timeout, recover()). Journal ROLLING_BACK, then decide FROM THE DISK:
//!    each existing old/p goes back over R/p, files the transaction added are deleted, directories
//!    it created go when empty; journal ROLLED_BACK; relaunch the old version; write last.json. It
//!    is idempotent after a crash anywhere, including inside itself. From
//!    COMMITTED_PENDING_HEALTH - a health check that failed - NOTHING MOVES until ROLLING_BACK is on
//!    disk: that state alone tells recovery to leave the tree, so the journal must stop saying it
//!    before the tree stops being whole.
//! 7. SUCCESS, once the new version has shown that it starts. Journal COMMITTED_PENDING_HEALTH;
//!    then RUN `R/bin/katana --update-health <tx>` TO ITS END, for up to 60 s. That run is a whole
//!    start of the window that shows nothing: it writes U/healthy-<tx> on the first idle turn of
//!    its event loop (about 3 s) and exits. Healthy is: the marker is there, AND the run ended with
//!    0 or was still running when the wait ended (it is then stopped). A start that could not be
//!    made, that ends non-zero (a missing entry point fails before main), or that never writes the
//!    marker - however it ended - means rollback as in 6, and the sequence is added to `blocked`; a
//!    marker such a start wrote is deleted BEFORE the rollback, so that it can never be read as
//!    proof by whoever meets the transaction later. Healthy: for an installer-kind install rewrite
//!    DisplayVersion and DisplayName (only now: a release that is put back must never be the one
//!    Settings > Apps names); journal HEALTHY; delete R/.update/<tx>, the stage and the marker;
//!    what a lock keeps from being deleted goes to U/cleanup.list for the next start. The folder
//!    and the marker go only once HEALTHY is on disk: until then they are what the update is
//!    settled by.
//!
//! If the ROLLBACK ITSELF fails (a file that stays locked, a full or failing disk), the tree may
//! be half swapped and nothing more can be done from here: the outcome is RollbackIncomplete,
//! last.json says so, and the by-hand command is `katana_updater --recover <R>` (the RunOnce entry
//! and the next start of any program try it too: recover.rs). It is never reported as RolledBack.
//!
//! If THE HELPER DIES in step 7 - between COMMITTED_PENDING_HEALTH and HEALTHY: power, a shutdown,
//! the process ended - the tree is a whole new version that nobody has said starts. No program's
//! recover() touches it (it may be a live helper's, waiting for that very program). It is settled,
//! under the lock, by `katana_updater --recover` (the RunOnce entry, or by hand), by the window of
//! the new version at its first idle turn, or by step 1 of the next swap: recover.rs says which of
//! them may do what.
//!
//! A program started during the commit (about a second) may fail to load; it is started again.
//! That residual is stated in docs/update.md.
//!
//! Three rules shape every line of the implementation:
//! 1. EVERY change to the disk goes through FsOps, so the fault loops see it. Nothing here uses
//!    std::fs to write, rename or delete. (Reads that only look - hashing a file - may go to the
//!    disk directly: FaultFs counts changes, not reads.)
//! 2. NOTHING catches a SimulatedCrash and nothing does disk work in a destructor, so a crash
//!    injected between two operations leaves exactly what a dead process would.
//! 3. ROLLBACK DOES NOT REPLAY THE JOURNAL. It looks at the disk: where old/p exists, p goes back;
//!    where the transaction put a new file in place, it is moved back out. So it is the same code
//!    after a crash at any point, including inside itself.
//!
//! Where the C++ catches a std::exception (one from a file system or a conversion: nothing there
//! throws on purpose), the port catches a panic, and lets a FaultFs's SimulatedCrash go through
//! as the C++ does, which is no std::exception (catch_unexpected, below).

use crate::fault_fs::SimulatedCrash;
use crate::feed::{StopToken, stop_requested};
use crate::fsops::{EntryKind, FileLock, FsOps, path_text};
use crate::install::{
    Install, InstallKind, install_record_directory, read_install, update_work_directory,
};
use crate::journal::{self, Journal, JournalOp, JournalRecord, JournalState, ParsedJournal};
use crate::long_path::openable;
use crate::manifest::{FileClass, Manifest};
use crate::paths::{
    RECORD_PATHS, check_journal_directory, check_journal_path, check_manifest_path,
    is_reserved_name, lower_ascii,
};
use crate::process::{Launcher, ProcessOps, RunningProgram, SpawnRequest, make_system_launcher};
use crate::recover_internal::{
    forget_settled, may_replace_unreadable_state, name_as_text, normalized_root, recover_locked,
    run_once_command, settle_pending,
};
use crate::registry::{RUN_ONCE_NAME, RegistryOps};
use crate::remove_tree::remove_tree;
use crate::stage::{staged_manifest_path, staged_signature_path};
use crate::state::{
    LastResult, append_cleanup, health_marker_path, read_state, write_last_result, write_state,
};
use crate::trust::{Trust, is_newer_than, verify_manifest};
use crate::version;
use katana_core::error::make_error;
use katana_core::path_text::{path_from_utf8, path_to_utf8};
use katana_core::sha256::{sha256_file_hex, sha256_hex};
use katana_core::text_encoding::utf8_or_windows1252;
use katana_core::{Error, ErrorCode, Result, Status};
use std::any::Any;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::hash::{BuildHasher, Hash, Hasher};
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The helper protocol this build speaks. A manifest's `applier` is the least protocol that can
/// apply it; a helper older than that is replaced from the stage first (stage.rs, preflight).
pub const APPLIER_PROTOCOL: u32 = 1;

/// What the helper is told (its command line). The window has already exited when it runs, after
/// close() succeeded.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ApplyRequest {
    /// R.
    pub root: PathBuf,
    /// S: U/stage-<sequence>.
    pub stage_directory: PathBuf,
    /// U: last.json, state.json, healthy-<tx>.
    pub state_directory: PathBuf,
    /// The preflighted copy, U/run-<tx>/katana_updater.
    pub helper: PathBuf,
    /// T.
    pub transaction: String,
    /// The window's pid; 0: none to wait for.
    pub wait_pid: u64,
    /// What to start afterwards; empty: relaunch nothing.
    pub relaunch: PathBuf,
}

/// The switch `katana` is relaunched with to prove it starts: `katana --update-health <tx>`
/// (src/katana_qt/main.cpp). It writes health_marker_path(U, tx) after its first idle turn and
/// exits normally.
pub const HEALTH_SWITCH: &str = "--update-health";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ApplyOutcome {
    /// The new version is in place and healthy.
    Committed,
    /// Processes under R did not exit, or one was started while the plan was made; nothing was
    /// touched.
    Blocked,
    /// A failure after the plan; the old version is back.
    RolledBack,
    /// Verification or planning failed; nothing was touched.
    #[default]
    Refused,
    /// A failure after the plan AND the rollback failed too: the tree may be half swapped. reason
    /// "rollback_failed"; detail names `katana_updater --recover <R>`.
    RollbackIncomplete,
}

/// The token last.json's `status` carries for an outcome (state.rs, LastResult): "committed"
/// "blocked" "rolled_back" "refused" "rollback_incomplete".
pub const fn to_string(outcome: ApplyOutcome) -> &'static str {
    match outcome {
        ApplyOutcome::Committed => "committed",
        ApplyOutcome::Blocked => "blocked",
        ApplyOutcome::RolledBack => "rolled_back",
        ApplyOutcome::Refused => "refused",
        ApplyOutcome::RollbackIncomplete => "rollback_incomplete",
    }
}

/// How a swap ended.
///
/// `reason` is a short machine-readable token. Everything apply() produces:
///
/// ```text
/// before anything is read: "locked", "interrupted_update" (no new update starts over an earlier
///   one that is not settled. Either that one was cut off and could not be put back - run
///   katana_updater --recover - or it is in place and was never said to start, and its marker is
///   not there: the window of that version settles it at its start, and so does
///   katana_updater --recover, which may put it back. Or R/.update could not be read through, so
///   that neither is known. `detail` says which), "invalid_request", "root_not_writable",
///   "install_record", "development_tree";
/// the staged release: "staged_manifest" (missing, unsigned, or refused by verify_manifest - which
///   also covers another product, target or channel and the path rules, so there is no separate
///   token for them), "notify_only", "applier_too_new", "downgrade";
/// waiting for the window and the programs under R: "blocked" (the Blocked outcome, from step 2's
///   wait and from its second look in step 4 alike), "scan_failed" (from either), "cancelled";
/// the plan: "staged_missing", "staged_hash", "reparse_point", "path_conflict", "copy_failed",
///   "space", "journal", "io_error", "transaction_exists";
/// the commit and after: "rename_failed", "health_timeout", "health_exit", "rollback_failed";
/// anywhere: "internal_error" (an unexpected error from the file system or a conversion cut the
///   run short; it was ended like any other failure at that point, and "rollback_failed" replaces
///   it when putting the tree back failed so as well).
/// ```
///
/// And one token that apply() never produces and that stands in the same `reason=` line of the
/// helper's output: "platform". On a system where katana_updater swaps nothing it refuses the
/// swap, and --recover, before apply() is called (stage() and preflight() refuse under the same
/// token, stage.rs). A dialog switches on the token, never on `detail`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ApplyResult {
    pub outcome: ApplyOutcome,
    pub reason: String,
    /// One sentence for a person.
    pub detail: String,
    /// Blocked: who holds the files.
    pub holders: Vec<RunningProgram>,
}

/// Every wait the algorithm makes, injectable so a test does not sit for a minute. The defaults
/// are the design's numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ApplyTimings {
    pub wait_for_parent: Duration,
    pub wait_for_holders: Duration,
    pub poll_interval: Duration,
    pub health_wait: Duration,
}

impl Default for ApplyTimings {
    fn default() -> Self {
        Self {
            wait_for_parent: Duration::from_millis(60_000),
            wait_for_holders: Duration::from_millis(30_000),
            poll_interval: Duration::from_millis(250),
            health_wait: Duration::from_millis(60_000),
        }
    }
}

/// The seams a swap is run through. The helper's trust is a parameter: production in
/// katana_updater, a test key in the helper built for the tests.
#[derive(Clone, Copy)]
pub struct ApplyServices<'a> {
    pub fs: &'a dyn FsOps,
    pub processes: &'a dyn ProcessOps,
    pub launcher: &'a dyn Launcher,
    pub registry: &'a dyn RegistryOps,
    pub trust: &'a Trust,
    pub timings: ApplyTimings,
}

impl<'a> ApplyServices<'a> {
    /// The services with the design's timings (the C++'s `ApplyTimings timings{}`).
    pub fn new(
        fs: &'a dyn FsOps,
        processes: &'a dyn ProcessOps,
        launcher: &'a dyn Launcher,
        registry: &'a dyn RegistryOps,
        trust: &'a Trust,
    ) -> Self {
        Self {
            fs,
            processes,
            launcher,
            registry,
            trust,
            timings: ApplyTimings::default(),
        }
    }
}

const JOURNAL_NAME: &str = "journal";
const NEW_NAME: &str = "new";
const OLD_NAME: &str = "old";
const LOCK_NAME: &str = "lock";
const RECOVERY_LOG_NAME: &str = "recovery.log";
const HELPER_STEM: &str = "katana_updater";
const PROGRAM_STEM: &str = "katana";
// Room for the journal, the three records and the directory entries, on top of the bytes the
// changed files take under new/ (old/ is a rename and takes none).
const SPACE_MARGIN: u64 = 1024 * 1024;
// A lock that is held is looked at again for this long before it is believed. A virus scanner or
// an indexer that has the lock file open for a moment fails the share-0 open exactly as another
// helper holding it does, and a helper that gave up at once would close the window and start
// nothing: the person would see Katana vanish.
const LOCK_GRACE: Duration = Duration::from_millis(2000);

// ---- what the C++ catches -------------------------------------------------------------------

/// Runs `work`, and turns a panic into its message - except a FaultFs's SimulatedCrash, which goes
/// through as the C++ lets it (it is no std::exception there). The port's twin of the C++'s
/// `catch (const std::exception&)`: what throws there (a conversion of a name, the file system)
/// panics here.
pub(crate) fn catch_unexpected<T>(work: impl FnOnce() -> T) -> std::result::Result<T, String> {
    match catch_unwind(AssertUnwindSafe(work)) {
        Ok(value) => Ok(value),
        Err(payload) => {
            if payload.is::<SimulatedCrash>() {
                resume_unwind(payload);
            }
            Err(panic_text(payload.as_ref()))
        }
    }
}

// The C++'s what(): a panic's message where it has one.
fn panic_text(payload: &(dyn Any + Send)) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        return (*text).to_owned();
    }
    if let Some(text) = payload.downcast_ref::<String>() {
        return text.clone();
    }
    String::from("an unexpected error")
}

// ---- text and paths ---------------------------------------------------------------------------

// "bin/<stem>" with ".exe" on Windows: the manifest path of an installed program.
fn installed_program_relative(stem: &str) -> String {
    let path = format!("bin/{stem}");
    #[cfg(windows)]
    {
        format!("{path}.exe")
    }
    #[cfg(not(windows))]
    {
        path
    }
}

// A manifest path (always '/') joined under a base.
fn under(base: &Path, relative: impl AsRef<[u8]>) -> PathBuf {
    base.join(path_from_utf8(relative))
}

// Windows and macOS file systems fold case, so "is it under the root" must too. Only ASCII is
// folded: every name the update system writes is ASCII (paths.rs).
fn folded(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::{OsStrExt, OsStringExt};
        let native: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .map(|unit| {
                if (u16::from(b'A')..=u16::from(b'Z')).contains(&unit) {
                    unit - u16::from(b'A') + u16::from(b'a')
                } else {
                    unit
                }
            })
            .collect();
        PathBuf::from(std::ffi::OsString::from_wide(&native))
    }
    #[cfg(not(windows))]
    {
        path.to_path_buf()
    }
}

// The root name of a path (a drive or a server's share), as std::filesystem's root_name().
fn root_name<'p>(components: &[Component<'p>]) -> Option<&'p std::ffi::OsStr> {
    match components.first() {
        Some(Component::Prefix(prefix)) => Some(prefix.as_os_str()),
        _ => None,
    }
}

// std::filesystem's `!path.lexically_relative(base).empty() && *relative.begin() != ".."`: the
// path is the base or below it, by the text alone.
fn is_under_or_equal(base: &Path, path: &Path) -> bool {
    let base = folded(base);
    let path = folded(path);
    let base_parts: Vec<Component<'_>> = base.components().collect();
    let path_parts: Vec<Component<'_>> = path.components().collect();
    // lexically_relative is empty for another root name, another absoluteness, or a path without
    // a root directory under a base with one.
    if root_name(&path_parts) != root_name(&base_parts)
        || path.is_absolute() != base.is_absolute()
        || (!path.has_root() && base.has_root())
    {
        return false;
    }
    let common = path_parts
        .iter()
        .zip(&base_parts)
        .take_while(|(a, b)| a == b)
        .count();
    // What is left of the base: each name is a "..", each ".." takes one back.
    let mut climbs: i64 = 0;
    for part in &base_parts[common..] {
        match part {
            Component::ParentDir => climbs -= 1,
            Component::CurDir => {}
            _ => climbs += 1,
        }
    }
    if climbs != 0 {
        // Negative: empty; positive: it starts with "..".
        return false;
    }
    // The base itself ("."), or what is left of the path - unless that starts with "..".
    !matches!(path_parts.get(common), Some(Component::ParentDir))
}

// The SHA-256 of a file, named the way the file system names it (long paths: long_path.rs).
// Hashing goes to the disk directly, not through FsOps: it only looks, and a file can be large.
fn hash_file(path: &Path) -> Result<String> {
    sha256_file_hex(&openable(path))
}

fn json_string(text: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if u32::from(c) < 0x20 => {
                let byte = u32::from(c) as u8;
                out.push_str("\\u00");
                out.push(char::from(HEX[usize::from((byte >> 4) & 0x0F)]));
                out.push(char::from(HEX[usize::from(byte & 0x0F)]));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

// installed.json: what the helper records at commit (install.rs). Keys sorted, one LF.
fn installed_record(manifest: &Manifest) -> String {
    let mut out = format!("{{\"product\":{}", json_string(&manifest.product));
    out.push_str(&format!(",\"schema\":1,\"sequence\":{}", manifest.sequence));
    out.push_str(&format!(",\"tag\":{}", json_string(&manifest.tag)));
    out.push_str(&format!(",\"target\":{}", json_string(&manifest.target)));
    out.push_str(&format!(
        ",\"version\":{}}}\n",
        json_string(&version::to_string(&manifest.version))
    ));
    out
}

// A file name as the C++'s pathToUtf8 gives it, for text that is not a path.
fn utf8_text(path: &Path) -> String {
    String::from_utf8_lossy(&path_to_utf8(path)).into_owned()
}

fn holder_text(holders: &[RunningProgram]) -> String {
    let mut text = String::new();
    for holder in holders {
        if !text.is_empty() {
            text.push_str(", ");
        }
        let mut name = holder
            .image
            .file_name()
            .map(|name| path_text(Path::new(name)))
            .unwrap_or_default();
        if name.is_empty() {
            name = String::from("Katana");
        }
        text.push_str(&format!("{name} ({})", holder.pid));
    }
    text
}

// The shape every "this failed" has before it becomes an ApplyResult.
#[derive(Clone, Debug, Default)]
struct Failure {
    outcome: ApplyOutcome,
    reason: String,
    detail: String,
    holders: Vec<RunningProgram>,
    // A refusal the other helper owns (the lock is held): no last.json, no relaunch.
    silent: bool,
}

fn refuse(reason: &str, detail: impl Into<String>) -> Failure {
    Failure {
        reason: String::from(reason),
        detail: detail.into(),
        ..Failure::default()
    }
}

fn refuse_with(reason: &str, what: &str, error: &Error) -> Failure {
    refuse(reason, format!("{what}: {}", error.describe()))
}

// Programs of the install are running and the update gives way to them: the Blocked outcome, the
// same whichever of step 2's two looks found them.
fn blocked_by(holders: Vec<RunningProgram>) -> Failure {
    Failure {
        outcome: ApplyOutcome::Blocked,
        reason: String::from("blocked"),
        detail: format!(
            "Close these first: {}. Then press Retry.",
            holder_text(&holders)
        ),
        holders,
        silent: false,
    }
}

fn is_held(error: &Error) -> bool {
    error.code == ErrorCode::InvalidState && error.message == "held"
}

// ---- keeping every write inside the install ---------------------------------------------------

// "Every parent must not be a reparse point and must resolve under R": a folder that is a junction
// or a symbolic link could send a write outside the install. The answer for a folder is
// remembered, so the hundreds of files of an install cost its few dozen folders.
struct PathGuard<'f> {
    fs: &'f dyn FsOps,
    root: PathBuf,
    resolved_root: PathBuf,
    verified: HashSet<PathBuf>,
}

impl<'f> PathGuard<'f> {
    fn new(fs: &'f dyn FsOps, root: PathBuf, resolved_root: PathBuf) -> Self {
        Self {
            fs,
            root,
            resolved_root,
            verified: HashSet::new(),
        }
    }

    // The folders from the root down to `directory` that exist are real folders under the root.
    // Folders that do not exist yet are fine (they will be made by us, below one that was
    // checked).
    fn check(&mut self, directory: &Path) -> Option<Failure> {
        let mut chain: Vec<PathBuf> = Vec::new();
        let mut walk = directory.to_path_buf();
        while !walk.as_os_str().is_empty() && walk != self.root {
            if self.verified.contains(&walk) {
                break;
            }
            if !is_under_or_equal(&self.root, &walk) {
                return Some(refuse(
                    "reparse_point",
                    format!("a path leaves the install folder: {}", path_text(&walk)),
                ));
            }
            chain.push(walk.clone());
            match walk.parent() {
                Some(parent) => walk = parent.to_path_buf(),
                None => break,
            }
        }
        for folder in chain.iter().rev() {
            let info = match self.fs.stat(folder) {
                Ok(info) => info,
                Err(error) => {
                    return Some(refuse_with(
                        "io_error",
                        "a folder of the install cannot be examined",
                        &error,
                    ));
                }
            };
            if info.kind == EntryKind::Missing {
                return None; // below this nothing exists yet
            }
            if info.reparse_point {
                return Some(refuse(
                    "reparse_point",
                    format!("a folder of the install is a link: {}", path_text(folder)),
                ));
            }
            if info.kind != EntryKind::Directory {
                return Some(refuse(
                    "path_conflict",
                    format!("a file is where a folder is needed: {}", path_text(folder)),
                ));
            }
            let resolved = match self.fs.resolve(folder) {
                Ok(resolved) => resolved,
                Err(error) => {
                    return Some(refuse_with(
                        "io_error",
                        "a folder of the install cannot be resolved",
                        &error,
                    ));
                }
            };
            if !is_under_or_equal(&self.resolved_root, &resolved) {
                return Some(refuse(
                    "reparse_point",
                    format!(
                        "a folder of the install resolves outside it: {}",
                        path_text(folder)
                    ),
                ));
            }
            self.verified.insert(folder.clone());
        }
        None
    }
}

// ---- the plan a journal carries ---------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Add,
    Replace,
    Remove,
}

#[derive(Clone, Debug)]
struct PlanEntry {
    action: Action,
    // A path that passed acceptable_path: printable ASCII (paths.rs).
    path: Vec<u8>,
}

#[derive(Clone, Debug, Default)]
struct JournalPlan {
    // In the order the journal lists them (commit order).
    entries: Vec<PlanEntry>,
    // Created, shallowest first.
    directories: Vec<Vec<u8>>,
    // "<op> <path>: why", for the recovery log: bytes, as the journal held them.
    refused: Vec<Vec<u8>>,
}

// True when a '/'-separated path has "." or ".." as a whole component - the only forms of dot a
// path may not contain. "a..b.dat" is a file name like any other, and a rule that refused it
// would leave the old file of a release that ships one unrestored.
fn has_dot_component(path: &[u8]) -> bool {
    path.split(|byte| *byte == b'/')
        .any(|part| part == b"." || part == b"..")
}

// What a path read back from a journal, or found under old/, must pass before anything is done
// with it: check_journal_path (check_journal_directory for the folder an MKDIR record names: a
// release that first brings etc or lib makes that folder bare) AND a second look that stays inside
// the root when joined, which does not depend on the first being right.
fn acceptable_path(path: &[u8], directory: bool) -> Status {
    if directory {
        check_journal_directory(path)?;
    } else {
        check_journal_path(path)?;
    }
    let parsed = path_from_utf8(path);
    let has_root_name = matches!(parsed.components().next(), Some(Component::Prefix(_)));
    let contained = !path.is_empty()
        && !has_dot_component(path)
        && !path.contains(&b'\\')
        && !path.contains(&b':')
        && !parsed.is_absolute()
        && !has_root_name
        && is_under_or_equal(Path::new("root"), &under(Path::new("root"), path));
    if !contained {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "leaves the install",
            utf8_or_windows1252(path),
        ));
    }
    Ok(())
}

// "<OP> <rest>" as the recovery log quotes a refused record: bytes.
fn quoted(op: JournalOp, rest: &[u8]) -> Vec<u8> {
    let mut line = journal::to_string(op).as_bytes().to_vec();
    line.extend_from_slice(rest);
    line
}

// Reads the PLAN group of a journal. The journal is not authenticated (journal.rs), so a path in
// it is acted on only if acceptable_path says so.
fn read_plan(journal: &ParsedJournal) -> JournalPlan {
    let mut plan = JournalPlan::default();
    for record in &journal.records {
        let (action, directory) = match record.op {
            JournalOp::Add => (Some(Action::Add), false),
            JournalOp::Replace => (Some(Action::Replace), false),
            JournalOp::Remove => (Some(Action::Remove), false),
            JournalOp::MakeDirectory => (None, true),
            _ => continue,
        };
        if record.args.len() != 1 {
            plan.refused.push(quoted(record.op, b": not one path"));
            continue;
        }
        let path = &record.args[0];
        if let Err(error) = acceptable_path(path, directory) {
            let mut rest = b" ".to_vec();
            rest.extend_from_slice(path);
            rest.extend_from_slice(b": ");
            rest.extend_from_slice(error.describe().as_bytes());
            plan.refused.push(quoted(record.op, &rest));
            continue;
        }
        match action {
            Some(action) => plan.entries.push(PlanEntry {
                action,
                path: path.clone(),
            }),
            None => plan.directories.push(path.clone()),
        }
    }
    plan
}

// prefix + "/" + name, or the name alone at the top.
fn joined(prefix: &[u8], name: &[u8]) -> Vec<u8> {
    if prefix.is_empty() {
        return name.to_vec();
    }
    let mut out = prefix.to_vec();
    out.push(b'/');
    out.extend_from_slice(name);
    out
}

// Every file under `folder`, as the '/'-joined path below it (`prefix` is what comes before).
// Nothing is followed or entered that is a link; what is not an ordinary file or folder, and what
// is too deep to be an install's, is named in `skipped`. A folder that is not there holds nothing;
// one that cannot be read is `unreadable`, never "empty".
fn collect_files(
    fs: &dyn FsOps,
    folder: &Path,
    prefix: &[u8],
    depth: i32,
    files: &mut Vec<Vec<u8>>,
    skipped: &mut Vec<Vec<u8>>,
    unreadable: &mut Option<Error>,
) {
    const MAX_DEPTH: i32 = 32;
    let this = match fs.stat(folder) {
        Ok(info) => info,
        Err(error) => {
            *unreadable = Some(error);
            return;
        }
    };
    if this.kind == EntryKind::Missing {
        return;
    }
    if this.kind != EntryKind::Directory || this.reparse_point {
        skipped.push(if prefix.is_empty() {
            b"(the folder itself)".to_vec()
        } else {
            prefix.to_vec()
        });
        return;
    }
    let children = match fs.list_directory(folder) {
        Ok(children) => children,
        Err(error) => {
            *unreadable = Some(error);
            return;
        }
    };
    for child in &children {
        let Some(name) = name_as_text(child) else {
            // A name that is not valid text (an unpaired surrogate) is no name an update writes.
            skipped.push(joined(prefix, b"(a name that is not text)"));
            continue;
        };
        let relative = joined(prefix, &name);
        let info = match fs.stat(child) {
            Ok(info) => info,
            Err(error) => {
                *unreadable = Some(error);
                continue;
            }
        };
        let ordinary = info.kind == EntryKind::File || info.kind == EntryKind::Directory;
        if info.reparse_point || !ordinary {
            skipped.push(relative);
        } else if info.kind == EntryKind::File {
            files.push(relative);
        } else if depth + 1 >= MAX_DEPTH {
            skipped.push(relative);
        } else {
            collect_files(fs, child, &relative, depth + 1, files, skipped, unreadable);
        }
    }
}

// The first failure is the one returned; the others are still tried.
fn first_failure(failure: &mut Option<Error>, error: Error) {
    failure.get_or_insert(error);
}

// Step 6, the rollback, decided from the disk. `work` is R/.update/<tx>.
//
// For every file the plan names:
//   REPLACE, REMOVE  old/p exists: it goes back over R/p. If R/p holds the NEW file it is moved
//                    out first (to new/p - a rename, because deleting a program that is running is
//                    refused and moving it is not).
//   ADD              R/p exists, new/p does not: the transaction moved it in, so move it back to
//                    new/p. If new/p still exists R/p is not ours.
// then the directories the transaction created go, deepest first, when they are empty. Nothing
// here depends on what the journal says happened - only on what is there. That includes WHICH
// files there are: every file under old/ whose name an update could have written goes back,
// whether or not the journal lists it, because the caller deletes the folder afterwards and a file
// left in old/ is a file lost (a journal damaged in the middle, or one with a record the path
// rules refused, must not turn into a v1/v2 mix). A name under old/ that no update writes is not
// ours: it is added to plan.refused and goes with the folder.
//
// Every file in old/ is either put back, reported as a failure, or - for a REMOVE whose file has
// been made again since - left to the person's newer file; so an OK means old/ holds nothing that
// was meant to be kept.
fn restore_from_disk(fs: &dyn FsOps, root: &Path, work: &Path, plan: &mut JournalPlan) -> Status {
    let resolved_root = fs.resolve(root)?;
    let mut guard = PathGuard::new(fs, root.to_path_buf(), resolved_root);
    if let Some(bad) = guard.check(work) {
        return Err(make_error(ErrorCode::InvalidState, bad.detail, bad.reason));
    }
    let old_root = work.join(OLD_NAME);
    let new_root = work.join(NEW_NAME);

    let mut failure: Option<Error> = None;

    // The journal's entries, then the files old/ holds that it does not list.
    let mut entries = plan.entries.clone();
    {
        let listed: BTreeSet<Vec<u8>> = entries.iter().map(|entry| entry.path.clone()).collect();
        let mut files: Vec<Vec<u8>> = Vec::new();
        let mut skipped: Vec<Vec<u8>> = Vec::new();
        let mut unreadable: Option<Error> = None;
        collect_files(
            fs,
            &old_root,
            b"",
            0,
            &mut files,
            &mut skipped,
            &mut unreadable,
        );
        if let Some(error) = unreadable {
            first_failure(&mut failure, error);
        }
        for relative in files {
            if listed.contains(&relative) {
                continue;
            }
            if acceptable_path(&relative, false).is_err() {
                skipped.push(relative);
                continue;
            }
            entries.push(PlanEntry {
                action: Action::Replace,
                path: relative,
            });
        }
        for name in skipped {
            let mut line = b"old/".to_vec();
            line.extend_from_slice(&name);
            line.extend_from_slice(b": not a file an update puts there");
            plan.refused.push(line);
        }
    }

    for entry in entries.iter().rev() {
        let target = under(root, &entry.path);
        let old_path = under(&old_root, &entry.path);
        let new_path = under(&new_root, &entry.path);
        // All three sides of a move are held to the rule, not only the install's: old/<dir> and
        // new/<dir> can be links as well, and a file renamed through one comes from, or goes to,
        // a place outside the install.
        let mut unsafe_place = guard.check(parent_of(&target));
        if unsafe_place.is_none() {
            unsafe_place = guard.check(parent_of(&old_path));
        }
        if unsafe_place.is_none() {
            unsafe_place = guard.check(parent_of(&new_path));
        }
        if let Some(bad) = unsafe_place {
            first_failure(
                &mut failure,
                make_error(
                    ErrorCode::InvalidState,
                    bad.detail,
                    format!("{} {}", bad.reason, utf8_or_windows1252(&entry.path)),
                ),
            );
            continue;
        }
        let in_tree = match fs.stat(&target) {
            Ok(info) => info,
            Err(error) => {
                first_failure(&mut failure, error);
                continue;
            }
        };

        if entry.action == Action::Add {
            if in_tree.kind != EntryKind::File {
                continue; // never moved in (or already moved back)
            }
            match fs.stat(&new_path) {
                Err(error) => first_failure(&mut failure, error),
                Ok(waiting) if waiting.kind == EntryKind::Missing => {
                    if let Err(error) = fs.make_directories(parent_of(&new_path)) {
                        first_failure(&mut failure, error);
                    } else if let Err(error) = fs.rename_file(&target, &new_path) {
                        first_failure(&mut failure, error);
                    }
                }
                Ok(_) => {}
            }
            continue;
        }

        let saved = match fs.stat(&old_path) {
            Ok(info) => info,
            Err(error) => {
                first_failure(&mut failure, error);
                continue;
            }
        };
        if saved.kind != EntryKind::File {
            continue; // the old file was never moved aside, or is back already
        }
        if in_tree.kind == EntryKind::Directory || in_tree.kind == EntryKind::Other {
            first_failure(
                &mut failure,
                make_error(
                    ErrorCode::InvalidState,
                    "something other than a file is where a file must go back",
                    utf8_or_windows1252(&entry.path),
                ),
            );
            continue;
        }
        if in_tree.kind == EntryKind::File && entry.action == Action::Remove {
            continue; // a removed file has been made again since; that one is the person's
        }
        if in_tree.kind == EntryKind::File {
            // The new file (or one that appeared since). Out of the way, not deleted.
            let waiting = match fs.stat(&new_path) {
                Ok(info) => info,
                Err(error) => {
                    first_failure(&mut failure, error);
                    continue;
                }
            };
            let cleared = if waiting.kind == EntryKind::Missing && entry.action == Action::Replace {
                fs.make_directories(parent_of(&new_path))
                    .and_then(|()| fs.rename_file(&target, &new_path))
            } else {
                fs.remove_file(&target)
            };
            if let Err(error) = cleared {
                first_failure(&mut failure, error);
                continue;
            }
        }
        if let Err(error) = fs.make_directories(parent_of(&target)) {
            first_failure(&mut failure, error);
            continue;
        }
        if let Err(error) = fs.rename_file(&old_path, &target) {
            first_failure(&mut failure, error);
        }
    }

    if let Some(error) = failure {
        return Err(error);
    }
    // Only now, with every file where it was, are the folders the transaction made safe to
    // remove. A folder that is not empty holds something that is not ours; it stays, and so does
    // one that cannot be removed: an empty folder is not a half-swapped tree.
    for directory in plan.directories.iter().rev() {
        let _ = fs.remove_empty_directory(&under(root, directory));
    }
    Ok(())
}

// std::filesystem's parent_path: the path without its last name ("" for a bare name).
fn parent_of(path: &Path) -> &Path {
    path.parent().unwrap_or(Path::new(""))
}

// What a skipped journal record is reported in. A line per record, appended; nothing is created
// when nothing was skipped. The window reads this file for the transactions a recovery found
// (recover.rs, "THE FORM OF THOSE LINES IS READ"), and a record's text is anybody's: it names
// nothing there because the line's second word is `journal`, and because it stays ONE line
// (one_line), so that a forged record cannot begin a line of its own.
fn report_refusals(fs: &dyn FsOps, root: &Path, transaction: &str, plan: &JournalPlan) {
    if plan.refused.is_empty() {
        return;
    }
    let mut text: Vec<u8> = Vec::new();
    for line in &plan.refused {
        text.extend_from_slice(transaction.as_bytes());
        text.extend_from_slice(b" journal record refused: ");
        text.extend_from_slice(&crate::recover_internal::one_line(line));
        text.push(b'\n');
    }
    let _ = fs.append_durable(&update_work_directory(root).join(RECOVERY_LOG_NAME), &text);
}

// ---- the apply --------------------------------------------------------------------------------

// The order the classes are committed in (step 5), the helper's own file after the other
// programs, the removals after every new file is in place, and the records last: installed.json
// first - the claim "this version is installed" - then the signed manifest and its signature
// (they sort in that order, and rollback does not depend on it).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Phase {
    Data,
    Lib,
    Program,
    Helper,
    Remove,
    Record,
}

// The C++'s Entry also carries the file's `executable` flag, which nothing reads: the copy is
// marked from the manifest's entry as it is made (plan below).
#[derive(Clone, Debug)]
struct Entry {
    action: Action,
    phase: Phase,
    path: String,
}

// Which parents of a new file do not exist yet; `folder_exists` remembers each folder's answer.
fn need_folder(
    fs: &dyn FsOps,
    root: &Path,
    folder_exists: &mut BTreeMap<String, bool>,
    to_create: &mut BTreeSet<String>,
    relative_file: &str,
) -> Option<Failure> {
    let prefixes: Vec<&str> = relative_file
        .match_indices('/')
        .map(|(slash, _)| &relative_file[..slash])
        .collect();
    for prefix in prefixes {
        let exists = match folder_exists.get(prefix) {
            Some(&exists) => exists,
            None => {
                let info = match fs.stat(&under(root, prefix)) {
                    Ok(info) => info,
                    Err(error) => {
                        return Some(refuse_with(
                            "io_error",
                            "a folder of the install cannot be examined",
                            &error,
                        ));
                    }
                };
                if info.kind != EntryKind::Missing && info.kind != EntryKind::Directory {
                    return Some(refuse(
                        "path_conflict",
                        format!("a file is where a folder is needed: {prefix}"),
                    ));
                }
                let exists = info.kind == EntryKind::Directory;
                folder_exists.insert(String::from(prefix), exists);
                exists
            }
        };
        if !exists {
            to_create.insert(String::from(prefix));
        }
    }
    None
}

struct Applier<'a> {
    request: &'a ApplyRequest,
    services: &'a ApplyServices<'a>,
    fs: &'a dyn FsOps,
    stop: StopToken<'a>,

    root: PathBuf,
    update_dir: PathBuf,
    work: PathBuf,
    install: Install,
    lock: Option<Box<dyn FileLock>>,
    staged_json: Vec<u8>,
    staged_signature: Vec<u8>,
    manifest: Option<Manifest>,
    installed_manifest: Option<Manifest>,
    entries: Vec<Entry>,
    // Created by the transaction, shallowest first.
    directories: Vec<String>,
    // The same, as a set, for the commit.
    made_beforehand: BTreeSet<String>,
    run_once_set: bool,
    journal_written: bool,
    // This run made R/.update/<tx>, so it is this run's to delete if the plan does not finish.
    work_made: bool,
    // The window that asked for the update did not exit: it is still there, and it must not be
    // started a second time beside itself.
    window_still_running: bool,
    // The new version proved it starts. What is left is the clean-up, and a failure there does
    // not undo an update that is good.
    proved_healthy: bool,
    journal: Option<Journal<'a>>,
}

impl<'a> Applier<'a> {
    fn new(
        request: &'a ApplyRequest,
        services: &'a ApplyServices<'a>,
        stop: StopToken<'a>,
    ) -> Self {
        Self {
            request,
            services,
            fs: services.fs,
            stop,
            root: PathBuf::new(),
            update_dir: PathBuf::new(),
            work: PathBuf::new(),
            install: Install::default(),
            lock: None,
            staged_json: Vec::new(),
            staged_signature: Vec::new(),
            manifest: None,
            installed_manifest: None,
            entries: Vec::new(),
            directories: Vec::new(),
            made_beforehand: BTreeSet::new(),
            run_once_set: false,
            journal_written: false,
            work_made: false,
            window_still_running: false,
            proved_healthy: false,
            journal: None,
        }
    }

    fn cancelled(&self) -> bool {
        stop_requested(self.stop)
    }

    fn pause(&self) {
        std::thread::sleep(self.services.timings.poll_interval);
    }

    fn new_root(&self) -> PathBuf {
        self.work.join(NEW_NAME)
    }

    fn old_root(&self) -> PathBuf {
        self.work.join(OLD_NAME)
    }

    fn manifest_version(&self) -> String {
        self.manifest
            .as_ref()
            .map(|manifest| version::to_string(&manifest.version))
            .unwrap_or_default()
    }

    fn run(&mut self) -> ApplyResult {
        self.root = if self.request.root.as_os_str().is_empty() {
            PathBuf::new()
        } else {
            normalized_root(&self.request.root)
        };
        // The state folder is where the health marker is looked for: without it a start that
        // hung, or exited without saying it had started, could not be told from one that was
        // fine.
        if self.root.as_os_str().is_empty()
            || self.request.state_directory.as_os_str().is_empty()
            || !is_valid_transaction_id(&self.request.transaction)
        {
            let failed = self.fail(
                &refuse(
                    "invalid_request",
                    "the helper was started with an empty install or state folder or a \
                     transaction id that is not lower-case hexadecimal",
                ),
                false,
            );
            return self.finish(failed, false);
        }
        self.update_dir = update_work_directory(&self.root);
        self.work = self
            .update_dir
            .join(path_from_utf8(&self.request.transaction));

        if let Some(failure) = self.begin() {
            let failed = self.fail(&failure, false);
            return self.finish(failed, failure.silent);
        }
        if let Some(failure) = self.verify() {
            let failed = self.fail(&failure, false);
            return self.finish(failed, false);
        }
        if let Some(failure) = self.wait_for_quiet() {
            let failed = self.fail(&failure, false);
            return self.finish(failed, false);
        }
        if let Some(failure) = self.plan() {
            // A folder that was there before this run is not this run's to delete.
            if failure.reason != "transaction_exists" {
                self.discard_work();
            }
            let failed = self.fail(&failure, false);
            return self.finish(failed, false);
        }
        // From here the journal exists, and everything that goes wrong is a rollback.
        if let Some(failure) = self.write_journal_plan() {
            let written = self.journal_written;
            let failed = self.fail(&failure, written);
            return self.finish(failed, false);
        }
        if let Some(failure) = self.commit() {
            let failed = self.fail(&failure, true);
            return self.finish(failed, false);
        }
        let pending = JournalRecord {
            op: JournalOp::CommittedPendingHealth,
            args: Vec::new(),
        };
        if let Err(error) = self.append(&[pending]) {
            let failed = self.fail(
                &refuse_with("journal", "the journal cannot be written", &error),
                true,
            );
            return self.finish(failed, false);
        }
        if let Some(failure) = self.watch_health() {
            let failed = self.fail(&failure, true);
            return self.finish(failed, false);
        }
        self.proved_healthy = true;
        // Settings > Apps names the new version only once it has proved it starts: written
        // before, a release that is rolled back would still be the one the person is told they
        // have.
        if self.install.kind == InstallKind::Installer {
            let version = self.manifest_version();
            let _ = self
                .services
                .registry
                .set_installed_version(&version, &format!("Katana {version}"));
        }
        self.finish_healthy();

        let result = ApplyResult {
            outcome: ApplyOutcome::Committed,
            detail: format!("Katana {} is installed.", self.manifest_version()),
            ..ApplyResult::default()
        };
        self.finish(result, false)
    }

    fn append(&mut self, group: &[JournalRecord]) -> Status {
        match self.journal.as_mut() {
            Some(journal) => journal.append(group),
            None => Err(Error::new(
                ErrorCode::Internal,
                "the journal was never opened",
            )),
        }
    }

    // Step 1 up to the lock: what the install is, that its folder can be written, and that no
    // other helper is running.
    fn begin(&mut self) -> Option<Failure> {
        if let Some(bad) = self.read_install_record() {
            return Some(bad);
        }

        if let Err(error) = self.fs.make_directories(&self.update_dir) {
            return Some(refuse_with(
                "root_not_writable",
                "the install folder cannot be written",
                &error,
            ));
        }
        let ordinary = matches!(
            self.fs.stat(&self.update_dir),
            Ok(info) if info.kind == EntryKind::Directory && !info.reparse_point
        );
        if !ordinary {
            return Some(refuse(
                "reparse_point",
                format!(
                    ".update is not an ordinary folder: {}",
                    path_text(&self.update_dir)
                ),
            ));
        }
        let lock_path = self.update_dir.join(LOCK_NAME);
        let mut lock = self.fs.try_lock(&lock_path);
        // The grace is cut to the holders' wait, so the tests that shorten that shorten this too.
        let grace = LOCK_GRACE.min(self.services.timings.wait_for_holders);
        let lock_start = Instant::now();
        while matches!(&lock, Err(error) if is_held(error))
            && lock_start.elapsed() < grace
            && !self.cancelled()
        {
            self.pause();
            lock = self.fs.try_lock(&lock_path);
        }
        match lock {
            Ok(lock) => self.lock = Some(lock),
            Err(error) => {
                if is_held(&error) {
                    return Some(Failure {
                        silent: true,
                        ..refuse("locked", "another update is running")
                    });
                }
                return Some(refuse_with(
                    "root_not_writable",
                    "the update lock cannot be taken",
                    &error,
                ));
            }
        }

        // An earlier transaction that a crash left unfinished, or whose rollback failed, is put
        // back BEFORE this one reads the tree: planned over a half-swapped tree, this update
        // would succeed, and at the next start recovery would roll the old files of that
        // transaction over the new version - a mix of both with the wrong installed.json. If it
        // cannot be put back, nothing new is started on top of it.
        let root_text = path_text(&self.root);
        if let Err(error) = recover_locked(
            &self.root,
            self.services,
            &self.request.transaction,
            &self.request.state_directory,
        ) {
            return Some(refuse_with(
                "interrupted_update",
                &format!(
                    "an earlier update was interrupted and could not be put back, so no new one \
                     is started. Run: katana_updater --recover \"{root_text}\""
                ),
                &error,
            ));
        }

        // An earlier update that is in place and was never settled - its helper died while the
        // new version was proving that it starts - is settled BEFORE this one too, by the one
        // rule (recover_internal.rs, settle_pending), and never by putting it back: a swap is not
        // the place to judge a release that somebody may be running from. With its marker there
        // it is healthy, and what its dead helper left in U goes. Without, nothing is touched and
        // no new update is started: over a release that was never shown to start, a failure of
        // this one would put back a tree nobody proved, and its folder - the old files of the
        // release before - would have no owner. So one such folder exists at most.
        let earlier = settle_pending(
            &self.root,
            self.services,
            &self.request.transaction,
            &self.request.state_directory,
            false,
        );
        forget_settled(self.fs, &self.request.state_directory, &earlier);
        if !earlier.left.is_empty() {
            return Some(refuse(
                "interrupted_update",
                format!(
                    "an earlier update is in place and has not been confirmed to start, so no new \
                     one is started over it. Katana's window confirms it when it starts - or run: \
                     katana_updater --recover \"{root_text}\", which confirms it or puts it back - \
                     then update again"
                ),
            ));
        }
        if earlier.unread {
            // Not "nothing is left": the folder could not be looked through - it would not be
            // listed, or the journal of a transaction in it would not be read (a scanner has it
            // open) - so an update that is not settled may be in it, and planning over one is
            // what this step exists to prevent. That covers the step before as well:
            // recover_locked passes over a folder whose journal it cannot read, and such a folder
            // may be a half swap.
            return Some(refuse(
                "interrupted_update",
                format!(
                    "the install's update folder could not be read, so it is not known whether an \
                     earlier update is still to be put back or confirmed, and no new one is \
                     started. Update again; if it stays so, run: katana_updater --recover \
                     \"{root_text}\""
                ),
            ));
        }
        if !earlier.healthy.is_empty() {
            // The RunOnce entry the dead helper left names its copy U/run-<tx>, which has just
            // been deleted, and nothing of that update is left for a logon to do. This swap
            // registers its own entry when its journal says COMMITTING - and one that ends before
            // that (Blocked, Refused) registers none and removes none (run_once_set), so the
            // entry goes here.
            let _ = self.services.registry.clear_run_once(RUN_ONCE_NAME);
        }
        // What is installed may be another version now (the rollback put it back).
        self.read_install_record()
    }

    // What the install is (install.json, installed.json, the registry), and that it is an install.
    fn read_install_record(&mut self) -> Option<Failure> {
        match read_install(&self.root, self.services.registry) {
            Ok(install) => self.install = install,
            Err(error) => {
                return Some(refuse_with(
                    "install_record",
                    "the install's records cannot be read",
                    &error,
                ));
            }
        }
        if self.install.kind == InstallKind::Development || self.install.target.is_empty() {
            return Some(refuse(
                "development_tree",
                "this is a development tree, not an install: it has no share/katana/install.json, \
                 and is never updated",
            ));
        }
        None
    }

    // Step 1, verify: with the helper's own keys, never the engine's word.
    fn verify(&mut self) -> Option<Failure> {
        let json = self
            .fs
            .read_file(&staged_manifest_path(&self.request.stage_directory));
        let signature = self
            .fs
            .read_file(&staged_signature_path(&self.request.stage_directory));
        let (json, signature) = match (json, signature) {
            (Ok(json), Ok(signature)) => (json, signature),
            (Ok(_), Err(error)) | (Err(error), _) => {
                return Some(refuse_with(
                    "staged_manifest",
                    "the staged manifest or its signature is missing",
                    &error,
                ));
            }
        };
        let verified = match verify_manifest(&json, &signature, self.services.trust, &self.install)
        {
            Ok(manifest) => manifest,
            Err(error) => {
                return Some(refuse_with(
                    "staged_manifest",
                    "the staged manifest is not trusted",
                    &error,
                ));
            }
        };
        self.staged_json = json;
        self.staged_signature = signature;
        self.manifest = Some(verified);
        let manifest = self.manifest.as_ref()?;

        // verify_manifest has already refused, with the helper's own code, a manifest of another
        // product, target or channel (trust.rs) and one that breaks the path rules (the parse:
        // check_manifest_paths), so they are not asked a second time here.
        if manifest.notify_only {
            return Some(refuse(
                "notify_only",
                "this release is announced only; it cannot be installed here",
            ));
        }
        if manifest.applier > APPLIER_PROTOCOL {
            return Some(refuse(
                "applier_too_new",
                "this update needs a newer update helper than this one",
            ));
        }
        if !is_newer_than(manifest, &self.install) {
            return Some(refuse(
                "downgrade",
                format!(
                    "the staged release ({}) is not newer than the installed one ({})",
                    version::to_string(&manifest.version),
                    version::to_string(&self.install.version)
                ),
            ));
        }
        // The installed manifest decides what may be removed. One that does not verify (no
        // record on a fresh install; a key since dropped; a tampered file) decides nothing:
        // nothing is removed, which is the safe way to be wrong.
        if self.install.has_installed_manifest {
            let directory = install_record_directory(&self.root);
            let old_json = self
                .fs
                .read_file(&directory.join("installed.manifest.json"));
            let old_signature = self
                .fs
                .read_file(&directory.join("installed.manifest.json.sig"));
            if let (Ok(old_json), Ok(old_signature)) = (old_json, old_signature)
                && let Ok(old) =
                    verify_manifest(old_json, old_signature, self.services.trust, &self.install)
            {
                self.installed_manifest = Some(old);
            }
        }
        None
    }

    // Step 2: the window, then everyone with an image under R. Never kills.
    fn wait_for_quiet(&mut self) -> Option<Failure> {
        let timings = self.services.timings;
        if self.request.wait_pid != 0 {
            let start = Instant::now();
            while self.services.processes.is_alive(self.request.wait_pid) {
                if self.cancelled() {
                    return Some(refuse("cancelled", "the update was cancelled"));
                }
                if start.elapsed() >= timings.wait_for_parent {
                    self.window_still_running = true;
                    return Some(Failure {
                        outcome: ApplyOutcome::Blocked,
                        reason: String::from("blocked"),
                        holders: vec![RunningProgram {
                            pid: self.request.wait_pid,
                            image: PathBuf::new(),
                        }],
                        detail: format!(
                            "Katana is still running ({}). Close it, then press Retry.",
                            self.request.wait_pid
                        ),
                        silent: false,
                    });
                }
                self.pause();
            }
        }
        let start = Instant::now();
        loop {
            let holders = match self.others_under_root() {
                Ok(holders) => holders,
                Err(error) => {
                    return Some(refuse_with(
                        "scan_failed",
                        "the running programs cannot be listed",
                        &error,
                    ));
                }
            };
            if holders.is_empty() {
                return None;
            }
            if self.cancelled() {
                return Some(refuse("cancelled", "the update was cancelled"));
            }
            if start.elapsed() >= timings.wait_for_holders {
                return Some(blocked_by(holders));
            }
            self.pause();
        }
    }

    // Every process but this one that was started from a file under R (step 2 asks, twice).
    fn others_under_root(&self) -> Result<Vec<RunningProgram>> {
        let scanned = self.services.processes.holders_under(&self.root)?;
        let current = self.services.processes.current_pid();
        Ok(scanned
            .into_iter()
            .filter(|program| program.pid != current)
            .collect())
    }

    // Step 2's second look, at the last moment an update can still end with nothing touched. The
    // first look is over before the plan, and the plan hashes and copies every changed file of a
    // release: seconds, in which a person starts Katana again or a script runs katana_cli. A
    // program started then is seen by nobody and goes on running old code over the new tree. So
    // the question is asked once more, with the PLAN on disk and COMMITTING not yet: a holder now
    // is the same Blocked as a holder then. It is ONE question and not a second wait: the first
    // look already waited for whoever was closing, and a wait here would let the files the plan
    // hashed grow old under a program that is running.
    fn look_again_for_holders(&self) -> Option<Failure> {
        match self.others_under_root() {
            Err(error) => Some(refuse_with(
                "scan_failed",
                "the running programs cannot be listed",
                &error,
            )),
            Ok(holders) if holders.is_empty() => None,
            Ok(holders) => Some(blocked_by(holders)),
        }
    }

    // Step 3: hash, decide, copy. Nothing under the install changes; what is written goes to
    // R/.update/<tx>/new.
    fn plan(&mut self) -> Option<Failure> {
        let fs = self.fs;
        let Some(manifest) = self.manifest.clone() else {
            return Some(refuse(
                "internal_error",
                "the plan was asked for before the manifest was verified",
            ));
        };
        let root = self.root.clone();
        let resolved_root = match fs.resolve(&root) {
            Ok(resolved) => resolved,
            Err(error) => {
                return Some(refuse_with(
                    "io_error",
                    "the install folder cannot be resolved",
                    &error,
                ));
            }
        };
        let mut guard = PathGuard::new(fs, root.clone(), resolved_root);

        // Which parents of the new files do not exist yet (shallowest first, no repeats).
        let mut folder_exists: BTreeMap<String, bool> = BTreeMap::new();
        let mut to_create: BTreeSet<String> = BTreeSet::new();

        // ---- the new manifest's files: keep, add or replace
        let mut to_copy: Vec<usize> = Vec::new();
        let mut in_new_manifest: BTreeSet<Vec<u8>> = BTreeSet::new();
        for (index, file) in manifest.files.iter().enumerate() {
            if self.cancelled() {
                return Some(refuse("cancelled", "the update was cancelled"));
            }
            in_new_manifest.insert(lower_ascii(&file.path));
            let target = under(&root, &file.path);
            if let Some(unsafe_place) = guard.check(parent_of(&target)) {
                return Some(unsafe_place);
            }
            let info = match fs.stat(&target) {
                Ok(info) => info,
                Err(error) => {
                    return Some(refuse_with(
                        "io_error",
                        "a file of the install cannot be examined",
                        &error,
                    ));
                }
            };
            let phase = match file.file_class {
                FileClass::Data | FileClass::Seed => Phase::Data,
                FileClass::Lib => Phase::Lib,
                FileClass::Program => {
                    if file.path == installed_program_relative(HELPER_STEM) {
                        Phase::Helper
                    } else {
                        Phase::Program
                    }
                }
            };
            let action = if info.kind == EntryKind::Missing {
                Action::Add
            } else if info.kind != EntryKind::File {
                return Some(refuse(
                    "path_conflict",
                    format!("a folder is where a file must go: {}", file.path),
                ));
            } else if info.reparse_point {
                return Some(refuse(
                    "reparse_point",
                    format!("a file of the install is a link: {}", file.path),
                ));
            } else if file.file_class == FileClass::Seed {
                continue; // written only when absent, so an edit by the person survives
            } else {
                if hash_file(&target).is_ok_and(|hash| hash == file.sha256) {
                    continue; // KEEP
                }
                Action::Replace // an unreadable file counts as changed
            };
            if let Some(folders) =
                need_folder(fs, &root, &mut folder_exists, &mut to_create, &file.path)
            {
                return Some(folders);
            }
            self.entries.push(Entry {
                action,
                phase,
                path: file.path.clone(),
            });
            to_copy.push(index);
        }

        // ---- removals: only a file the installed manifest lists, the new one does not, and whose
        // bytes are still the ones that manifest signed. Anything else is not ours.
        if let Some(installed) = &self.installed_manifest {
            for old in &installed.files {
                if in_new_manifest.contains(&lower_ascii(&old.path))
                    || is_reserved_name(&old.path)
                    || check_manifest_path(&old.path).is_err()
                {
                    continue;
                }
                let target = under(&root, &old.path);
                if guard.check(parent_of(&target)).is_some() {
                    continue;
                }
                let plain = matches!(
                    fs.stat(&target),
                    Ok(info) if info.kind == EntryKind::File && !info.reparse_point
                );
                if !plain {
                    continue;
                }
                if !hash_file(&target).is_ok_and(|hash| hash == old.sha256) {
                    continue; // changed by the person, or unreadable: left alone
                }
                self.entries.push(Entry {
                    action: Action::Remove,
                    phase: Phase::Remove,
                    path: old.path.clone(),
                });
            }
        }

        // ---- the three records (installed.json, the signed manifest and its signature)
        let records: [(&str, Vec<u8>); 3] = [
            (RECORD_PATHS[0], installed_record(&manifest).into_bytes()),
            (RECORD_PATHS[1], self.staged_json.clone()),
            (RECORD_PATHS[2], self.staged_signature.clone()),
        ];
        for (path, _) in &records {
            // The records are written like any other file, so their folder is held to the same
            // rule: share/katana may be a link even when no file of the release lives in it.
            let record_path = under(&root, path);
            if let Some(unsafe_place) = guard.check(parent_of(&record_path)) {
                return Some(unsafe_place);
            }
            let info = match fs.stat(&record_path) {
                Ok(info) => info,
                Err(error) => {
                    return Some(refuse_with(
                        "io_error",
                        "a record of the install cannot be examined",
                        &error,
                    ));
                }
            };
            if info.kind != EntryKind::Missing && info.kind != EntryKind::File {
                return Some(refuse(
                    "path_conflict",
                    format!("a folder is where a record must go: {path}"),
                ));
            }
            if info.reparse_point {
                return Some(refuse(
                    "reparse_point",
                    format!("a record of the install is a link: {path}"),
                ));
            }
            if let Some(folders) = need_folder(fs, &root, &mut folder_exists, &mut to_create, path)
            {
                return Some(folders);
            }
            self.entries.push(Entry {
                action: if info.kind == EntryKind::File {
                    Action::Replace
                } else {
                    Action::Add
                },
                phase: Phase::Record,
                path: String::from(*path),
            });
        }
        // Stable, as std::stable_sort: the phases in order, then the paths.
        self.entries
            .sort_by(|a, b| a.phase.cmp(&b.phase).then_with(|| a.path.cmp(&b.path)));

        // Shallowest first: a folder is made before the one inside it, and rolled back after it.
        self.directories = to_create.into_iter().collect();
        self.directories.sort_by(|a, b| {
            let depth_a = a.matches('/').count();
            let depth_b = b.matches('/').count();
            depth_a.cmp(&depth_b).then_with(|| a.cmp(b))
        });
        self.made_beforehand
            .extend(self.directories.iter().cloned());

        // ---- space for the copies
        let mut needed = SPACE_MARGIN;
        for &index in &to_copy {
            needed += manifest.files[index].size;
        }
        for (_, bytes) in &records {
            needed += bytes.len() as u64;
        }
        let space = match fs.free_space(&root) {
            Ok(space) => space,
            Err(error) => {
                return Some(refuse_with(
                    "io_error",
                    "the free space cannot be read",
                    &error,
                ));
            }
        };
        if space < needed {
            return Some(refuse(
                "space",
                format!(
                    "the install folder's disk has {space} bytes free and the update needs \
                     {needed}"
                ),
            ));
        }

        // ---- the copies. The stage folder is user-writable, so a file in R is a COPY of it,
        // made here and hashed after it was made (what is hashed is what will be installed),
        // never a rename out of it.
        let existing = match fs.stat(&self.work) {
            Ok(info) => info,
            Err(error) => {
                return Some(refuse_with(
                    "io_error",
                    "the transaction folder cannot be examined",
                    &error,
                ));
            }
        };
        if existing.kind != EntryKind::Missing {
            return Some(refuse(
                "transaction_exists",
                format!(
                    "the transaction folder exists already: {}",
                    path_text(&self.work)
                ),
            ));
        }
        self.work_made = true; // from here a failure leaves a folder of ours, even half made
        let new_root = self.new_root();
        if let Err(error) = fs.make_directories(&new_root) {
            return Some(refuse_with(
                "root_not_writable",
                "the transaction folder cannot be made",
                &error,
            ));
        }
        if let Some(unsafe_place) = guard.check(&self.work) {
            return Some(unsafe_place);
        }
        let mut made_new: BTreeSet<PathBuf> = BTreeSet::new();
        for &index in &to_copy {
            let file = &manifest.files[index];
            if self.cancelled() {
                return Some(refuse("cancelled", "the update was cancelled"));
            }
            let source = under(&self.request.stage_directory, &file.path);
            let staged = match fs.stat(&source) {
                Ok(info) if info.kind == EntryKind::File => info,
                _ => {
                    return Some(refuse(
                        "staged_missing",
                        format!("the staged file is missing: {}", file.path),
                    ));
                }
            };
            if staged.size != file.size {
                return Some(refuse(
                    "staged_hash",
                    format!("the staged file has the wrong size: {}", file.path),
                ));
            }
            let copy = under(&new_root, &file.path);
            if made_new.insert(parent_of(&copy).to_path_buf())
                && let Err(error) = fs.make_directories(parent_of(&copy))
            {
                return Some(refuse_with(
                    "root_not_writable",
                    "a staging folder cannot be made",
                    &error,
                ));
            }
            if let Err(error) = fs.copy_file(&source, &copy) {
                return Some(refuse_with(
                    "copy_failed",
                    &format!("a staged file cannot be copied: {}", file.path),
                    &error,
                ));
            }
            if !hash_file(&copy).is_ok_and(|hash| hash == file.sha256) {
                return Some(refuse(
                    "staged_hash",
                    format!(
                        "the staged file does not hash to the manifest's value: {}",
                        file.path
                    ),
                ));
            }
            if file.executable
                && let Err(error) = fs.set_executable(&copy)
            {
                return Some(refuse_with(
                    "copy_failed",
                    &format!("a file cannot be made executable: {}", file.path),
                    &error,
                ));
            }
        }
        for (path, bytes) in &records {
            let copy = under(&new_root, path);
            if made_new.insert(parent_of(&copy).to_path_buf())
                && let Err(error) = fs.make_directories(parent_of(&copy))
            {
                return Some(refuse_with(
                    "root_not_writable",
                    "a staging folder cannot be made",
                    &error,
                ));
            }
            if let Err(error) = fs.write_durable(&copy, bytes) {
                return Some(refuse_with(
                    "copy_failed",
                    &format!("a record cannot be written: {path}"),
                    &error,
                ));
            }
        }
        None
    }

    // Step 4: the PLAN group, step 2's second look, then COMMITTING, and the RunOnce entry that
    // re-runs recovery at the next logon should the machine die from here.
    fn write_journal_plan(&mut self) -> Option<Failure> {
        self.journal = Some(Journal::new(self.fs, self.work.join(JOURNAL_NAME)));

        let mut group: Vec<JournalRecord> = Vec::new();
        let to_sequence = self
            .manifest
            .as_ref()
            .map_or(0, |manifest| manifest.sequence);
        group.push(JournalRecord {
            op: JournalOp::Plan,
            args: vec![
                self.request.transaction.as_bytes().to_vec(),
                format!("{}", self.install.sequence).into_bytes(),
                format!("{to_sequence}").into_bytes(),
            ],
        });
        for directory in &self.directories {
            group.push(JournalRecord {
                op: JournalOp::MakeDirectory,
                args: vec![directory.as_bytes().to_vec()],
            });
        }
        for entry in &self.entries {
            let op = match entry.action {
                Action::Add => JournalOp::Add,
                Action::Replace => JournalOp::Replace,
                Action::Remove => JournalOp::Remove,
            };
            group.push(JournalRecord {
                op,
                args: vec![entry.path.as_bytes().to_vec()],
            });
        }
        if let Err(error) = self.append(&group) {
            // Nothing has moved: the folder is junk, not a half swap.
            self.discard_work();
            return Some(refuse_with(
                "journal",
                "the journal cannot be written",
                &error,
            ));
        }
        self.journal_written = true;
        if self.cancelled() {
            self.discard_work();
            self.journal_written = false;
            return Some(refuse("cancelled", "the update was cancelled"));
        }
        // Nothing has moved yet, so whoever is found now is given way to as at the first look:
        // the folder of copies goes, the install is as it was, and the stage - the download -
        // stays for the Retry.
        if let Some(late) = self.look_again_for_holders() {
            self.discard_work();
            self.journal_written = false;
            return Some(late);
        }
        let committing = JournalRecord {
            op: JournalOp::Committing,
            args: Vec::new(),
        };
        if let Err(error) = self.append(&[committing]) {
            return Some(refuse_with(
                "journal",
                "the journal cannot be written",
                &error,
            ));
        }
        if !self.request.helper.as_os_str().is_empty() {
            // Best effort: it is a second safety net behind the next start of any program.
            self.run_once_set = self
                .services
                .registry
                .set_run_once(
                    RUN_ONCE_NAME,
                    &run_once_command(&self.request.helper, &self.root),
                )
                .is_ok();
        }
        None
    }

    // Step 5: renames only. Per file R/p to old/p, then new/p to R/p; a failure stops the commit
    // and the caller rolls back.
    fn commit(&mut self) -> Option<Failure> {
        let fs = self.fs;
        let root = self.root.clone();
        let old_root = self.old_root();
        let new_root = self.new_root();
        let entries = self.entries.clone();
        let mut made_old: BTreeSet<PathBuf> = BTreeSet::new();
        let mut made_root: BTreeSet<String> = BTreeSet::new();
        let mut index = 0;
        while index < entries.len() {
            let phase = entries[index].phase;
            let mut progress: Vec<JournalRecord> = Vec::new();
            let mut touched: BTreeSet<PathBuf> = BTreeSet::new();
            let mut removed: Vec<&Entry> = Vec::new();
            while index < entries.len() && entries[index].phase == phase {
                let entry = &entries[index];
                index += 1;
                let target = under(&root, &entry.path);
                let old_path = under(&old_root, &entry.path);
                let new_path = under(&new_root, &entry.path);
                if entry.action != Action::Add {
                    if made_old.insert(parent_of(&old_path).to_path_buf())
                        && let Err(error) = fs.make_directories(parent_of(&old_path))
                    {
                        return Some(refuse_with(
                            "rename_failed",
                            "a folder for the old file cannot be made",
                            &error,
                        ));
                    }
                    if let Err(error) = fs.rename_file(&target, &old_path) {
                        return Some(refuse_with(
                            "rename_failed",
                            &format!("the old file cannot be moved aside: {}", entry.path),
                            &error,
                        ));
                    }
                    progress.push(JournalRecord {
                        op: JournalOp::MovedOld,
                        args: vec![entry.path.as_bytes().to_vec()],
                    });
                    touched.insert(parent_of(&old_path).to_path_buf());
                }
                if entry.action != Action::Remove {
                    let parent = match entry.path.rfind('/') {
                        Some(slash) => &entry.path[..slash],
                        None => entry.path.as_str(),
                    };
                    if self.made_beforehand.contains(parent)
                        && made_root.insert(String::from(parent))
                        && let Err(error) = fs.make_directories(parent_of(&target))
                    {
                        return Some(refuse_with(
                            "rename_failed",
                            "a folder of the install cannot be made",
                            &error,
                        ));
                    }
                    if let Err(error) = fs.rename_file(&new_path, &target) {
                        return Some(refuse_with(
                            "rename_failed",
                            &format!("the new file cannot be moved into place: {}", entry.path),
                            &error,
                        ));
                    }
                    progress.push(JournalRecord {
                        op: JournalOp::MovedNew,
                        args: vec![entry.path.as_bytes().to_vec()],
                    });
                } else {
                    removed.push(entry);
                }
                touched.insert(parent_of(&target).to_path_buf());
            }
            // A folder the removals emptied goes too, so the tree is the new version's and not the
            // new version's plus empty folders. Best effort, and never one of the four top
            // folders; rollback makes the folder again before it puts a file back.
            for entry in removed {
                let mut folder = entry.path.clone();
                while let Some(slash) = folder.rfind('/') {
                    folder.truncate(slash);
                    if !folder.contains('/') {
                        break; // bin, share, etc and lib stay
                    }
                    if fs.remove_empty_directory(&under(&root, &folder)).is_err() {
                        break;
                    }
                }
            }
            if !progress.is_empty()
                && let Err(error) = self.append(&progress)
            {
                return Some(refuse_with(
                    "journal",
                    "the journal cannot be written",
                    &error,
                ));
            }
            for directory in &touched {
                let _ = fs.sync_directory(directory);
            }
        }
        None
    }

    // Step 7, the health check. The new katana is started with --update-health <tx>; it writes the
    // marker after its first idle turn and exits. A start that fails, exits non-zero, never writes
    // the marker or does not finish in time is not a working release.
    fn watch_health(&mut self) -> Option<Failure> {
        let executable = installed_program(&self.root, PROGRAM_STEM);
        let start = SpawnRequest {
            working_directory: parent_of(&executable).to_path_buf(),
            executable,
            arguments: vec![
                String::from(HEALTH_SWITCH),
                self.request.transaction.clone(),
            ],
            timeout: self.services.timings.health_wait,
        };

        let ran = match self.services.launcher.run(&start) {
            Ok(ran) => ran,
            Err(error) => {
                return Some(refuse_with(
                    "health_exit",
                    "the new version could not be started",
                    &error,
                ));
            }
        };
        let marker = matches!(
            self.fs.stat(&health_marker_path(
                &self.request.state_directory,
                &self.request.transaction
            )),
            Ok(info) if info.kind == EntryKind::File
        );
        if marker && (ran.timed_out || ran.exit_code == 0) {
            return None;
        }
        if marker {
            return Some(refuse(
                "health_exit",
                format!(
                    "the new version exited with code {} after starting",
                    ran.exit_code
                ),
            ));
        }
        if ran.timed_out {
            return Some(refuse(
                "health_timeout",
                "the new version did not finish starting in time",
            ));
        }
        Some(refuse(
            "health_exit",
            if ran.exit_code == 0 {
                String::from("the new version exited without saying it had started")
            } else {
                format!("the new version exited with code {}", ran.exit_code)
            },
        ))
    }

    // What a healthy transaction leaves behind goes: its folder, the staged files (only those the
    // manifest names - the stage path came from a command line), the marker, the RunOnce entry.
    // What a lock keeps from being deleted goes to cleanup.list for the next start.
    //
    // The folder and the marker go ONLY ONCE THE JOURNAL SAYS HEALTHY. While it still says
    // COMMITTED_PENDING_HEALTH the folder is an update that somebody will settle
    // (recover_internal.rs, settle_pending), and they settle it by exactly these two: the marker
    // is the proof that it starts, and old/ is what it would be put back from. A folder deleted in
    // part with its marker gone would read, to the next `--recover`, as a release that never
    // started - and be "put back" from the half of old/ that was left. So when the record cannot
    // be written both stay whole: the update stands, as it does when this helper dies before the
    // record, and the window it starts next, or the next swap, settles it healthy on that marker.
    fn finish_healthy(&mut self) {
        let healthy = JournalRecord {
            op: JournalOp::Healthy,
            args: Vec::new(),
        };
        let recorded = self.append(&[healthy]).is_ok();

        let fs = self.fs;
        let mut leftovers: Vec<PathBuf> = Vec::new();
        if recorded {
            remove_tree(fs, &self.work, &mut leftovers, JOURNAL_NAME);
        }

        let stage = self.request.stage_directory.clone();
        if let Some(manifest) = &self.manifest {
            for file in &manifest.files {
                let staged = under(&stage, &file.path);
                if fs.remove_file(&staged).is_err() {
                    leftovers.push(staged.clone());
                }
                let mut folder = parent_of(&staged).to_path_buf();
                while folder != stage
                    && !folder.as_os_str().is_empty()
                    && is_under_or_equal(&stage, &folder)
                {
                    if fs.remove_empty_directory(&folder).is_err() {
                        break;
                    }
                    folder = parent_of(&folder).to_path_buf();
                }
            }
        }
        let _ = fs.remove_file(&staged_manifest_path(&stage));
        let _ = fs.remove_file(&staged_signature_path(&stage));
        let _ = fs.remove_empty_directory(&stage);
        let state = &self.request.state_directory;
        if !state.as_os_str().is_empty() {
            if recorded {
                let _ = fs.remove_file(&health_marker_path(state, &self.request.transaction));
            }
            for leftover in &leftovers {
                let _ = append_cleanup(state, leftover, fs);
            }
        }
    }

    // The work folder of a transaction that never reached a journal: nothing under R changed, so
    // it is only junk.
    fn discard_work(&mut self) {
        let mut leftovers: Vec<PathBuf> = Vec::new();
        remove_tree(self.fs, &self.work, &mut leftovers, "");
        let state = &self.request.state_directory;
        if !state.as_os_str().is_empty() {
            for leftover in &leftovers {
                let _ = append_cleanup(state, leftover, self.fs);
            }
        }
    }

    fn rolled_back(&mut self, reason: &str, detail: &str) -> ApplyResult {
        if let Err(undone) = rollback(&self.root, &self.request.transaction, self.services) {
            return ApplyResult {
                outcome: ApplyOutcome::RollbackIncomplete,
                reason: String::from("rollback_failed"),
                detail: format!(
                    "The update failed ({reason}: {detail}) and putting the old version back \
                     failed too ({}). The install may be half updated: run katana_updater \
                     --recover \"{}\".",
                    undone.describe(),
                    path_text(&self.root)
                ),
                holders: Vec::new(),
            };
        }
        ApplyResult {
            outcome: ApplyOutcome::RolledBack,
            reason: String::from(reason),
            detail: String::from(detail),
            holders: Vec::new(),
        }
    }

    fn fail(&mut self, failure: &Failure, rollback_first: bool) -> ApplyResult {
        if rollback_first {
            let unhealthy = failure.reason == "health_exit" || failure.reason == "health_timeout";
            let state = self.request.state_directory.clone();
            // The marker a start that then failed wrote is evidence of nothing, and it goes
            // BEFORE the rollback. Should the rollback not even begin (its first record cannot be
            // written) or this helper die in the next lines, the journal still says the update is
            // pending - and a marker left beside it would have whoever settles it later call
            // healthy a release this helper has just seen fail (recover_internal.rs,
            // settle_pending).
            if unhealthy && !state.as_os_str().is_empty() {
                let _ = self
                    .fs
                    .remove_file(&health_marker_path(&state, &self.request.transaction));
            }
            let result = self.rolled_back(&failure.reason, &failure.detail);
            // A release that does not start must not be offered again as it was (state.rs).
            if result.outcome == ApplyOutcome::RolledBack
                && unhealthy
                && !state.as_os_str().is_empty()
                && let Some(manifest) = &self.manifest
            {
                let read = read_state(&state);
                // A file that is not ours to read is replaced; one that failed to read for another
                // reason is left alone (recover_internal.rs).
                let replaceable = match &read {
                    Ok(_) => true,
                    Err(error) => may_replace_unreadable_state(error),
                };
                let mut updated = read.unwrap_or_default();
                if replaceable && !updated.blocked_sequences.contains(&manifest.sequence) {
                    updated.blocked_sequences.push(manifest.sequence);
                    let _ = write_state(&state, &updated);
                }
            }
            return result;
        }
        ApplyResult {
            outcome: failure.outcome,
            reason: failure.reason.clone(),
            detail: failure.detail.clone(),
            holders: failure.holders.clone(),
        }
    }

    // The end of every path that is not a crash: say how it ended, let the lock go, and start the
    // program again. The lock goes BEFORE the relaunch, because the program's first act is
    // recover(), which waits for a live holder.
    fn finish(&mut self, result: ApplyResult, silent: bool) -> ApplyResult {
        drop(self.lock.take());
        let state = &self.request.state_directory;
        // The helper runs from a copy in U/run-<tx> and cannot delete the program it is running:
        // the folder goes on the list the next start works through. Not after a rollback that
        // failed - the RunOnce entry runs `--recover` from that very copy.
        if result.outcome != ApplyOutcome::RollbackIncomplete
            && !self.request.helper.as_os_str().is_empty()
            && !state.as_os_str().is_empty()
        {
            let _ = append_cleanup(state, parent_of(&self.request.helper), self.fs);
        }
        if silent {
            return result;
        }
        if result.outcome != ApplyOutcome::RollbackIncomplete && self.run_once_set {
            let _ = self.services.registry.clear_run_once(RUN_ONCE_NAME);
        }
        if !state.as_os_str().is_empty() {
            let last = LastResult {
                status: String::from(to_string(result.outcome)),
                reason: result.reason.clone(),
                holder: holder_text(&result.holders),
                version: self.manifest_version(),
                transaction: self.request.transaction.clone(),
            };
            let _ = write_last_result(state, &last);
        }
        // A half-swapped tree is not started: its programs may not load, and a person who sees
        // nothing happen reads last.json at the next start instead. Nor a window that is still
        // running: a second one would open beside the one that hung.
        if !self.request.relaunch.as_os_str().is_empty()
            && result.outcome != ApplyOutcome::RollbackIncomplete
            && !self.window_still_running
        {
            let again = SpawnRequest {
                executable: self.request.relaunch.clone(),
                working_directory: parent_of(&self.request.relaunch).to_path_buf(),
                ..SpawnRequest::default()
            };
            let _ = self.services.launcher.start_detached(&again);
        }
        result
    }

    // The end of a run that an unexpected error cut short (one from a file system or a
    // conversion: nothing here fails so on purpose). It is ended the way a failure at that point
    // would have been: before the journal there is only a folder of copies to delete; after it
    // the tree is put back from the disk, which is what every other failure does; once the new
    // version has proved itself only the clean-up was lost, and the update stands. If even that
    // fails, the outcome says what is known and names the command that puts a half-swapped tree
    // right.
    fn unexpected(&mut self, what: &str) -> ApplyResult {
        let reason = "internal_error";
        let message = format!("the update stopped on an unexpected error: {what}");
        let ended = catch_unexpected(|| {
            if self.proved_healthy {
                let result = ApplyResult {
                    outcome: ApplyOutcome::Committed,
                    detail: format!(
                        "Katana {} is installed; clearing up after the update was cut short.",
                        self.manifest_version()
                    ),
                    ..ApplyResult::default()
                };
                return self.finish(result, false);
            }
            if self.work_made && !self.journal_written {
                self.discard_work();
            }
            let written = self.journal_written;
            let failed = self.fail(&refuse(reason, message.clone()), written);
            self.finish(failed, false)
        });
        match ended {
            Ok(result) => result,
            Err(again) => {
                drop(self.lock.take());
                if self.proved_healthy {
                    // Only the clean-up and the report failed; the new version is in and works.
                    ApplyResult {
                        outcome: ApplyOutcome::Committed,
                        detail: format!(
                            "the update is installed; clearing up after it stopped on an error: \
                             {again}"
                        ),
                        ..ApplyResult::default()
                    }
                } else if self.journal_written {
                    ApplyResult {
                        outcome: ApplyOutcome::RollbackIncomplete,
                        reason: String::from("rollback_failed"),
                        detail: format!(
                            "{message}, and then putting the old version back stopped on another \
                             ({again}). The install may be half updated: run katana_updater \
                             --recover <the install folder>."
                        ),
                        holders: Vec::new(),
                    }
                } else {
                    ApplyResult {
                        outcome: ApplyOutcome::Refused,
                        reason: String::from(reason),
                        detail: message,
                        holders: Vec::new(),
                    }
                }
            }
        }
    }
}

// ---- the command line ---------------------------------------------------------------------------

/// The helper's command line, without argv[0]:
/// `--root R --stage S --state U --tx T --wait-pid P --relaunch <path|none>`. `helper` is not on
/// the command line (it is argv[0]'s copy). The arguments are UTF-8, as a SpawnRequest carries
/// them (process.rs).
pub fn helper_arguments(request: &ApplyRequest) -> Vec<String> {
    vec![
        String::from("--root"),
        utf8_text(&request.root),
        String::from("--stage"),
        utf8_text(&request.stage_directory),
        String::from("--state"),
        utf8_text(&request.state_directory),
        String::from("--tx"),
        request.transaction.clone(),
        String::from("--wait-pid"),
        format!("{}", request.wait_pid),
        String::from("--relaunch"),
        if request.relaunch.as_os_str().is_empty() {
            String::from("none")
        } else {
            utf8_text(&request.relaunch)
        },
    ]
}

// A decimal number and nothing else, as std::from_chars reads one into a u64 that must take the
// whole text: no sign, no space, no overflow.
fn whole_number(text: &[u8]) -> Option<u64> {
    if text.is_empty() || !text.iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(text).ok()?.parse::<u64>().ok()
}

/// helper_arguments' inverse. The arguments arrive as UTF-8 (process.rs, utf8_arguments), or as
/// the bytes a POSIX command line holds. Refuses (InvalidArgument) a `tx` for which
/// is_valid_transaction_id is false - it names a folder under R/.update and U, and a command line
/// is not trusted to name only that - a missing option, an unknown option and a repeated one.
pub fn parse_helper_arguments(arguments: &[impl AsRef<[u8]>]) -> Result<ApplyRequest> {
    const OPTIONS: [&str; 6] = [
        "--root",
        "--stage",
        "--state",
        "--tx",
        "--wait-pid",
        "--relaunch",
    ];
    let mut values: BTreeMap<&str, &[u8]> = BTreeMap::new();
    let mut i = 0;
    while i < arguments.len() {
        let option = arguments[i].as_ref();
        let Some(&known) = OPTIONS.iter().find(|name| name.as_bytes() == option) else {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "unknown option",
                utf8_or_windows1252(option),
            ));
        };
        if i + 1 >= arguments.len() {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "option without a value",
                known,
            ));
        }
        if values.insert(known, arguments[i + 1].as_ref()).is_some() {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "option given twice",
                known,
            ));
        }
        i += 2;
    }
    for option in OPTIONS {
        if !values.contains_key(option) {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "missing option",
                option,
            ));
        }
    }
    let value = |option: &str| values.get(option).copied().unwrap_or_default();
    let (root, stage, state) = (value("--root"), value("--stage"), value("--state"));
    if root.is_empty() || stage.is_empty() || state.is_empty() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "--root, --stage and --state need a folder",
            "",
        ));
    }
    let transaction = value("--tx");
    if !is_valid_transaction_id(transaction) {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "a transaction id is 1 to 64 characters of 0-9 and a-f",
            utf8_or_windows1252(transaction),
        ));
    }
    let pid = value("--wait-pid");
    let Some(wait_pid) = whole_number(pid) else {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "--wait-pid is a process id (a number; 0 for none)",
            utf8_or_windows1252(pid),
        ));
    };
    let relaunch = value("--relaunch");
    Ok(ApplyRequest {
        root: path_from_utf8(root),
        stage_directory: path_from_utf8(stage),
        state_directory: path_from_utf8(state),
        helper: PathBuf::new(),
        // Lower-case hexadecimal, checked above.
        transaction: String::from_utf8_lossy(transaction).into_owned(),
        wait_pid,
        relaunch: if relaunch == b"none" {
            PathBuf::new()
        } else {
            path_from_utf8(relaunch)
        },
    })
}

/// R/bin/<stem>, with ".exe" on Windows.
pub fn installed_program(root: &Path, stem: &str) -> PathBuf {
    #[cfg(windows)]
    {
        root.join("bin").join(path_from_utf8(format!("{stem}.exe")))
    }
    #[cfg(not(windows))]
    {
        root.join("bin").join(path_from_utf8(stem))
    }
}

/// A fresh transaction id: lower-case hex and digits only, safe as a file name and in a command
/// line, unique across runs.
pub fn new_transaction_id() -> String {
    // Unique across runs without relying on one source of randomness alone: the clocks, a
    // counter, the address of a local and the thread, hashed.
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let marker = 0u8;
    // The steady clock has no count since an epoch here; its reading is written as the standard
    // library shows it.
    let mut seed = format!("{:?}", Instant::now());
    let since_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    seed.push_str(&format!(":{since_epoch}"));
    seed.push_str(&format!(":{}", COUNTER.fetch_add(1, Ordering::Relaxed) + 1));
    seed.push_str(&format!(":{}", std::ptr::from_ref(&marker) as usize));
    let mut thread = std::collections::hash_map::DefaultHasher::new();
    std::thread::current().id().hash(&mut thread);
    seed.push_str(&format!(":{}", thread.finish()));
    // An extra, not the only source, as the C++'s random_device: the standard library's hash
    // keys are drawn from the system's randomness.
    seed.push_str(&format!(
        ":{}",
        std::collections::hash_map::RandomState::new().hash_one(since_epoch)
    ));
    sha256_hex(seed)[..32].to_owned()
}

/// What an id must look like wherever one is read (the helper's command line, a folder name under
/// R/.update or U): 1 to 64 characters, each 0-9 or a-f. Nothing else - no separator, dot, space or
/// drive letter - so an id can never name a place outside the folders it is joined to. In the C++
/// it is one of the finished functions of contract.cpp, with its tests in
/// tests/update/test_contract.cpp.
pub fn is_valid_transaction_id(text: impl AsRef<[u8]>) -> bool {
    let text = text.as_ref();
    if text.is_empty() || text.len() > 64 {
        return false;
    }
    text.iter()
        .all(|&c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

// ---- the entry points -----------------------------------------------------------------------------

/// Steps 1 to 7. Never panics except with SimulatedCrash from a FaultFs. `stop` cancels before the
/// commit; after COMMITTING it is ignored (a half swap is finished or rolled back, never
/// abandoned).
pub fn apply(
    request: &ApplyRequest,
    services: &ApplyServices<'_>,
    stop: StopToken<'_>,
) -> ApplyResult {
    let mut applier = Applier::new(request, services, stop);
    // The one place an unexpected error is caught: a SimulatedCrash injected by a FaultFs still
    // goes through to the test that injected it.
    let ran = catch_unexpected(|| applier.run());
    match ran {
        Ok(result) => result,
        Err(what) => applier.unexpected(&what),
    }
}

fn rollback_transaction(
    root_given: &Path,
    transaction: &str,
    services: &ApplyServices<'_>,
) -> Status {
    if root_given.as_os_str().is_empty() {
        // update_work_directory("") is ".update" under the current folder: not an install.
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "no install folder to roll back in",
            transaction,
        ));
    }
    if !is_valid_transaction_id(transaction) {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "a transaction id is 1 to 64 characters of 0-9 and a-f",
            transaction,
        ));
    }
    let fs = services.fs;
    let root = normalized_root(root_given);
    let work = update_work_directory(&root).join(path_from_utf8(transaction));
    let folder = fs.stat(&work)?;
    if folder.kind == EntryKind::Missing {
        return Ok(()); // nothing to undo
    }
    if folder.kind != EntryKind::Directory || folder.reparse_point {
        return Err(make_error(
            ErrorCode::InvalidState,
            "the transaction folder is not an ordinary folder",
            path_text(&work),
        ));
    }

    let mut journal = Journal::new(fs, work.join(JOURNAL_NAME));
    let parsed = journal.load()?;
    if parsed.state == JournalState::Healthy {
        return Ok(()); // the new version proved it starts; it is not ours to undo
    }

    let mut leftovers: Vec<PathBuf> = Vec::new();
    if parsed.state != JournalState::RolledBack {
        let mut plan = read_plan(&parsed);
        if parsed.state != JournalState::Empty {
            let rolling = JournalRecord {
                op: JournalOp::RollingBack,
                args: Vec::new(),
            };
            let said = journal.append(&[rolling]);
            // Of the states a rollback starts from, COMMITTED_PENDING_HEALTH alone says "the tree
            // is a whole version: leave it" (journal.rs, needs_recovery). The first file moved
            // makes that untrue, and this record is what tells every later start so. When it is
            // not on disk NOTHING IS MOVED: the tree stays the whole new version its journal says
            // it is, and whoever comes next decides again from a journal that is true. From every
            // other state the journal asks for a rollback already, with the record or without it,
            // so there the record stays best effort: a disk too full for one more line is still a
            // disk the renames work on.
            if let Err(error) = said
                && parsed.state == JournalState::CommittedPendingHealth
            {
                return Err(make_error(
                    ErrorCode::InvalidState,
                    "the journal cannot be written, so nothing was put back",
                    error.describe(),
                ));
            }
        }
        let restored = restore_from_disk(fs, &root, &work, &mut plan);
        // After the restore, which adds what it found under old/ and would not touch.
        report_refusals(fs, &root, transaction, &plan);
        restored?; // the folder stays: it holds the old files
        if parsed.state != JournalState::Empty {
            let done = JournalRecord {
                op: JournalOp::RolledBack,
                args: Vec::new(),
            };
            let _ = journal.append(&[done]);
        }
    }
    remove_tree(fs, &work, &mut leftovers, JOURNAL_NAME);
    Ok(())
}

/// Step 6 alone: roll the transaction back from the disk. For a failure inside apply() and for
/// recover. An error means the rollback did not finish (the tree may be half swapped): apply()
/// then reports RollbackIncomplete.
pub fn rollback(root: &Path, transaction: &str, services: &ApplyServices<'_>) -> Status {
    // An unexpected error becomes an error: recovery runs first in every program, and a name or a
    // folder somebody planted must not be able to stop one from starting.
    match catch_unexpected(|| rollback_transaction(root, transaction, services)) {
        Ok(status) => status,
        Err(what) => Err(make_error(
            ErrorCode::Internal,
            "the rollback stopped on an unexpected error",
            what,
        )),
    }
}

/// Whether the tree under `root` is still the release whose manifest sequence is `sequence`:
/// share/katana/installed.manifest.json verifies against its signature with `services.trust` (and
/// names this install's product and target), its sequence is `sequence`, and every file it lists
/// that is not a seed is an ordinary file whose SHA-256 is the listed one. A seed is left out
/// because the swap never replaces one that is there: the person's edit of it is not a changed
/// release. False for whatever cannot be read. Changes nothing.
///
/// It answers the rule that settles an update whose helper died before the new version was
/// proved, before it puts that update back (recover_internal.rs). A rollback moves the files the
/// journal names whatever they hold now, so a tree that somebody has since changed - another zip
/// unpacked over it, a file edited - is not put back: only the very release the transaction
/// installed is.
#[doc(hidden)]
pub fn tree_is_release(root_given: &Path, sequence: u64, services: &ApplyServices<'_>) -> bool {
    catch_unexpected(|| tree_is_release_unguarded(root_given, sequence, services)).unwrap_or(false) // what cannot be read is not known to be the release
}

fn tree_is_release_unguarded(
    root_given: &Path,
    sequence: u64,
    services: &ApplyServices<'_>,
) -> bool {
    let fs = services.fs;
    let root = normalized_root(root_given);
    if root.as_os_str().is_empty() {
        return false;
    }
    // What the install is decides which manifest it may carry (product and target), as for the
    // manifest a swap is asked to install.
    let Ok(install) = read_install(&root, services.registry) else {
        return false;
    };
    let directory = install_record_directory(&root);
    let (Ok(json), Ok(signature)) = (
        fs.read_file(&directory.join("installed.manifest.json")),
        fs.read_file(&directory.join("installed.manifest.json.sig")),
    ) else {
        return false;
    };
    let manifest = match verify_manifest(json, signature, services.trust, &install) {
        Ok(manifest) if manifest.sequence == sequence => manifest,
        _ => return false,
    };
    for file in &manifest.files {
        if file.file_class == FileClass::Seed {
            continue; // written only when absent: what is there is the person's
        }
        let target = under(&root, &file.path);
        let plain = matches!(
            fs.stat(&target),
            Ok(info) if info.kind == EntryKind::File && !info.reparse_point
        );
        if !plain || !hash_file(&target).is_ok_and(|hash| hash == file.sha256) {
            return false;
        }
    }
    true
}

/// Starts the preflighted helper copy detached, with helper_arguments(request), and returns. The
/// caller then exits: the helper is waiting for it. [`launch_apply_with`] takes the launcher, for
/// tests.
pub fn launch_apply(request: &ApplyRequest) -> Status {
    let launcher = make_system_launcher();
    launch_apply_with(request, launcher.as_ref())
}

pub fn launch_apply_with(request: &ApplyRequest, launcher: &dyn Launcher) -> Status {
    if request.helper.as_os_str().is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "no helper program to start (preflight has not run)",
        ));
    }
    let start = SpawnRequest {
        executable: request.helper.clone(),
        arguments: helper_arguments(request),
        // Not the install folder: a program whose working directory is inside it keeps that
        // folder from being renamed or removed.
        working_directory: parent_of(&request.helper).to_path_buf(),
        ..SpawnRequest::default()
    };
    launcher.start_detached(&start)
}
