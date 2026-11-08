//! Finishing what a crash interrupted (recover.rs), and the rollback under it (rollback() in
//! apply.rs): the twin of tests/update/test_recover.cpp.
//!
//! The interrupted states are not written by hand where a crash can make them: the apply is run on
//! a FaultFs that dies before a chosen operation, and the operation is chosen from a log of an
//! apply that went right (LogFs), so "the COMMITTING record" or "the third rename" is found, not
//! guessed. The forged journals ARE written by hand - nothing but a person with a text editor makes
//! them - and every expectation is that the root stays as it was, and that nothing outside it is
//! touched, which is a fact about the disk, not about the code.
//!
//! HOW THE C++'S TEST CLASSES BECAME RUST. The file systems the C++ derives from ForwardingFs for
//! one case (a removeFile that is refused, a stat that throws) are the harness's ForwardingFs with
//! that one operation replaced by a closure; the three that many cases share (WatchedFs,
//! UnlistableFrom, JournalHeld) are structs of their own, whose counters are Cells because every
//! FsOps method takes `&self`. A C++ exception thrown by the file system is a panic here, which the
//! library turns into an error as the C++ catches a std::exception (apply.rs, catch_unexpected). The
//! fixtures that hold a Loop (Unproved, Interrupted) are borrowed through the Loop's field, so that
//! the transaction, the marker and the helper beside it can be read while the scenario is borrowed.

pub mod test_key;

pub mod apply_harness;

use apply_harness::{
    Answer, FakeLauncher, FakeRegistry, FileSpec, ForwardingFs, HookFs, INSTALL_JSON, LogFs, Loop,
    ReleaseSpec, Rig, Scenario, ScriptedProcesses, StdFs, apply_until_crash,
    build_fixture_scenario, difference, fixture_folder, injected, is_present, journal_state_of,
    leftovers_in_update, no_answer, nobody, os_error, quiet_simulated_crashes, read_text,
    recover_clean, recover_until_crash, snapshot, somebody, text, tree_hash, write_install,
    write_stage, write_text,
};
use katana_core::error::make_error;
use katana_core::path_text::{path_from_utf8, path_to_utf8};
use katana_core::sha256::sha256_hex;
use katana_core::{Error, ErrorCode, Result, Status};
use katana_update::apply::{ApplyOutcome, ApplyResult, ApplyServices, installed_program, rollback};
use katana_update::command_line::quote_argument;
use katana_update::fault_fs::{FaultFs, SimulatedCrash};
use katana_update::fsops::{EntryInfo, FileLock, FsOps};
use katana_update::journal::{
    JournalOp, JournalRecord, JournalState, encode_record, parse_journal,
};
use katana_update::manifest::FileClass;
use katana_update::process::{RunningProgram, SpawnResult};
use katana_update::recover::{
    Confirmed, RECOVER_WAIT, Settled, recover_in_process, settle_unproved,
};
use katana_update::recover_internal as detail;
use katana_update::registry::{RUN_ONCE_NAME, RegistryOps};
use katana_update::state::health_marker_path;
use katana_update::trust::Trust;
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::io::ErrorKind;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use test_key::test_trust;

// The last argument of detail::recover_with: whether the program that calls recover() was started
// as administrator (process.rs, process_is_elevated). A test SAYS which; how this run of the suite
// was itself started is nothing a test may rest on.
const ORDINARY_START: bool = false;
const STARTED_AS_ADMINISTRATOR: bool = true;

// ---- small things --------------------------------------------------------------------------------

// The operations a wrapper below passes on as they are, written once.
macro_rules! forward {
    ($($name:ident($($arg:ident: $ty:ty),*) -> $ret:ty;)*) => {
        $(
            fn $name(&self, $($arg: $ty),*) -> $ret {
                self.inner.$name($($arg),*)
            }
        )*
    };
}

// A failure's description, for a message; nothing for a success.
fn describe(status: &Status) -> String {
    status
        .as_ref()
        .err()
        .map(Error::describe)
        .unwrap_or_default()
}

fn parent_of(path: &Path) -> PathBuf {
    path.parent().map(Path::to_path_buf).unwrap_or_default()
}

// std::filesystem::remove: a file, or an empty folder; nothing there is no error, and any other
// failure ends the case (the C++'s throws).
fn remove(path: &Path) {
    let removed = match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == ErrorKind::NotFound => return,
        Err(error) => Err(error),
        Ok(metadata) if metadata.is_dir() => std::fs::remove_dir(path),
        Ok(_) => std::fs::remove_file(path),
    };
    if let Err(error) = removed {
        panic!("{}: {error}", text(path));
    }
}

// std::filesystem::remove_all: a folder with everything in it, or a file; nothing there is no
// error.
fn remove_all(path: &Path) {
    let removed = match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == ErrorKind::NotFound => return,
        Err(error) => Err(error),
        Ok(metadata) if metadata.is_dir() => std::fs::remove_dir_all(path),
        Ok(_) => std::fs::remove_file(path),
    };
    if let Err(error) = removed {
        panic!("{}: {error}", text(path));
    }
}

fn make_directories(path: &Path) {
    std::fs::create_dir_all(path).unwrap_or_else(|error| panic!("{}: {error}", text(path)));
}

// std::filesystem's relative_path: the path without its root name and root directory.
fn relative_path(path: &Path) -> PathBuf {
    path.components()
        .filter(|component| !matches!(component, Component::Prefix(_) | Component::RootDir))
        .collect()
}

// Runs `work`, and gives back the SimulatedCrash it died of, if it did; any other panic goes on.
fn crash_of<T>(work: impl FnOnce() -> T) -> std::result::Result<T, SimulatedCrash> {
    quiet_simulated_crashes();
    match catch_unwind(AssertUnwindSafe(work)) {
        Ok(value) => Ok(value),
        Err(payload) => match payload.downcast::<SimulatedCrash>() {
            Ok(crash) => Err(*crash),
            Err(other) => resume_unwind(other),
        },
    }
}

// EXPECT_NO_THROW: what the call returns - and the case fails when it panics instead.
fn no_panic<T>(work: impl FnOnce() -> T) -> T {
    catch_unwind(AssertUnwindSafe(work)).expect("it returned, and did not panic")
}

fn record(op: JournalOp, args: &[&str]) -> JournalRecord {
    JournalRecord {
        op,
        args: args.iter().map(|arg| arg.as_bytes().to_vec()).collect(),
    }
}

// The records as a journal holds them, numbered from 1.
fn encode_all(records: &[JournalRecord]) -> String {
    let mut out = String::new();
    for (n, one) in (1u64..).zip(records) {
        out.push_str(&encode_record(n, one));
    }
    out
}

// ---- the clean apply, and a crash in it ----------------------------------------------------------

// The operations of a clean apply of the fixture release, by number.
#[derive(Clone, Debug, Default)]
struct Points {
    plan: u64,       // the PLAN group is appended
    committing: u64, // the COMMITTING record
    renames: Vec<u64>,
    pending: u64, // COMMITTED_PENDING_HEALTH
    healthy: u64, // HEALTHY
    #[allow(
        dead_code,
        reason = "the C++'s Points measures it, and no case of the file reads it"
    )]
    total: u64,
}

fn measure_clean_apply(lp: &mut Loop) -> Points {
    lp.reset();
    let s = lp.scenario();
    let bottom = StdFs;
    let faults = FaultFs::new(&bottom);
    let log = LogFs::new(&faults);
    let mut result = ApplyResult::default();
    let crash = apply_until_crash(s, &log, Some(&mut result));
    assert!(crash.is_none(), "{crash:?}");
    assert_eq!(
        result.outcome,
        ApplyOutcome::Committed,
        "{}{}",
        result.reason,
        result.detail
    );
    let points = Points {
        plan: log.find("appendDurable", "PLAN", 1),
        committing: log.find("appendDurable", "COMMITTING", 1),
        renames: log.all("renameFile"),
        pending: log.find("appendDurable", "COMMITTED_PENDING_HEALTH", 1),
        healthy: log.find("appendDurable", " HEALTHY ", 1),
        total: log.log.borrow().len() as u64,
    };
    assert!(points.plan > 0);
    assert!(points.committing > points.plan);
    assert!(points.pending > points.committing);
    assert!(points.healthy > points.pending);
    assert!(!points.renames.is_empty());
    points
}

// Dies before operation `k`, with a rename failed on the way if `fail_rename` is not 0.
fn crash_at(lp: &mut Loop, k: u64, fail_rename: u64) {
    lp.reset();
    let bottom = StdFs;
    let faults = FaultFs::new(&bottom);
    faults.crash_before(k);
    if fail_rename != 0 {
        faults.fail_rename_with(fail_rename, 32);
    }
    assert!(
        apply_until_crash(lp.scenario(), &faults, None).is_some(),
        "no crash before {k}"
    );
}

fn tiny(version: &str, sequence: u64) -> ReleaseSpec {
    ReleaseSpec {
        version: String::from(version),
        tag: format!("v{version}"),
        sequence,
        files: vec![
            FileSpec::new(
                "bin/katana.exe",
                format!("katana {version}\n"),
                FileClass::Program,
                true,
            ),
            FileSpec::new(
                "bin/lib.dll",
                format!("lib {version}\n"),
                FileClass::Lib,
                false,
            ),
            FileSpec::new(
                "share/katana/data.txt",
                format!("data {version}\n"),
                FileClass::Data,
                false,
            ),
        ],
        ..ReleaseSpec::default()
    }
}

// ---- file systems that answer otherwise ----------------------------------------------------------

// A watching FsOps for "nothing to do costs next to nothing" - and, for the cases that need the
// update lock to be somebody else's, one whose lock is "held" for the first `held_for` attempts
// (always, when it is -1): a helper that is alive, or a scanner that has the lock file open for a
// moment. `locks` counts every attempt, so one attempt can be told from a lock that was polled.
struct WatchedFs<'a> {
    inner: &'a dyn FsOps,
    held_for: i32, // 0: the lock is free, as far as this wrapper goes
    locks: Cell<i32>,
    listings: Cell<i32>,
}

impl<'a> WatchedFs<'a> {
    fn new(inner: &'a dyn FsOps) -> Self {
        Self {
            inner,
            held_for: 0,
            locks: Cell::new(0),
            listings: Cell::new(0),
        }
    }
}

impl FsOps for WatchedFs<'_> {
    forward! {
        stat(path: &Path) -> Result<EntryInfo>;
        read_file(path: &Path) -> Result<Vec<u8>>;
        resolve(path: &Path) -> Result<PathBuf>;
        free_space(path: &Path) -> Result<u64>;
        make_directories(directory: &Path) -> Status;
        copy_file(from: &Path, to: &Path) -> Status;
        rename_file(from: &Path, to: &Path) -> Status;
        remove_file(path: &Path) -> Status;
        remove_empty_directory(path: &Path) -> Status;
        append_durable(path: &Path, bytes: &[u8]) -> Status;
        write_durable(path: &Path, bytes: &[u8]) -> Status;
        set_executable(path: &Path) -> Status;
        sync_directory(directory: &Path) -> Status;
    }

    fn list_directory(&self, directory: &Path) -> Result<Vec<PathBuf>> {
        self.listings.set(self.listings.get() + 1);
        self.inner.list_directory(directory)
    }

    fn try_lock(&self, path: &Path) -> Result<Box<dyn FileLock>> {
        let locks = self.locks.get() + 1;
        self.locks.set(locks);
        if self.held_for < 0 || locks <= self.held_for {
            return Err(make_error(
                ErrorCode::InvalidState,
                "held",
                format!("os_error=32 {}", text(path)),
            ));
        }
        self.inner.try_lock(path)
    }
}

// A file system on which the install's working folder R/.update cannot be listed from the
// `from`-th time it is asked for (from 1): a folder that will not be read through just now. Every
// other folder lists as it does.
struct UnlistableFrom<'a> {
    inner: &'a dyn FsOps,
    from: i32,
    asked: Cell<i32>,
}

impl<'a> UnlistableFrom<'a> {
    fn new(inner: &'a dyn FsOps, from: i32) -> Self {
        Self {
            inner,
            from,
            asked: Cell::new(0),
        }
    }
}

impl FsOps for UnlistableFrom<'_> {
    forward! {
        stat(path: &Path) -> Result<EntryInfo>;
        read_file(path: &Path) -> Result<Vec<u8>>;
        resolve(path: &Path) -> Result<PathBuf>;
        free_space(path: &Path) -> Result<u64>;
        make_directories(directory: &Path) -> Status;
        copy_file(from: &Path, to: &Path) -> Status;
        rename_file(from: &Path, to: &Path) -> Status;
        remove_file(path: &Path) -> Status;
        remove_empty_directory(path: &Path) -> Status;
        append_durable(path: &Path, bytes: &[u8]) -> Status;
        write_durable(path: &Path, bytes: &[u8]) -> Status;
        set_executable(path: &Path) -> Status;
        sync_directory(directory: &Path) -> Status;
        try_lock(path: &Path) -> Result<Box<dyn FileLock>>;
    }

    fn list_directory(&self, directory: &Path) -> Result<Vec<PathBuf>> {
        if directory.file_name() == Some(OsStr::new(".update")) {
            let asked = self.asked.get() + 1;
            self.asked.set(asked);
            if asked >= self.from {
                return Err(os_error(
                    ErrorCode::FileImportFailure,
                    "access denied",
                    directory,
                    5,
                ));
            }
        }
        self.inner.list_directory(directory)
    }
}

// A file system on which the journal of a transaction is THERE and cannot be read: a scanner or a
// backup has it open. That is a failure to read, with the system's code for a sharing violation
// (32), and not "no such file" - the real file system answers NotFound only for a file that is
// absent (src/katana_update/fsops_win.cpp, readFile). The folder lists and every other file reads
// as it does. `held_for` reads of a journal are refused, then they succeed (-1: all of them).
struct JournalHeld<'a> {
    inner: &'a dyn FsOps,
    held_for: i32,
    asked: Cell<i32>,
}

impl<'a> JournalHeld<'a> {
    fn new(inner: &'a dyn FsOps) -> Self {
        Self {
            inner,
            held_for: -1,
            asked: Cell::new(0),
        }
    }
}

impl FsOps for JournalHeld<'_> {
    forward! {
        stat(path: &Path) -> Result<EntryInfo>;
        list_directory(directory: &Path) -> Result<Vec<PathBuf>>;
        resolve(path: &Path) -> Result<PathBuf>;
        free_space(path: &Path) -> Result<u64>;
        make_directories(directory: &Path) -> Status;
        copy_file(from: &Path, to: &Path) -> Status;
        rename_file(from: &Path, to: &Path) -> Status;
        remove_file(path: &Path) -> Status;
        remove_empty_directory(path: &Path) -> Status;
        append_durable(path: &Path, bytes: &[u8]) -> Status;
        write_durable(path: &Path, bytes: &[u8]) -> Status;
        set_executable(path: &Path) -> Status;
        sync_directory(directory: &Path) -> Status;
        try_lock(path: &Path) -> Result<Box<dyn FileLock>>;
    }

    fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        if path.file_name() == Some(OsStr::new("journal")) {
            let asked = self.asked.get() + 1;
            self.asked.set(asked);
            if self.held_for < 0 || asked <= self.held_for {
                return Err(os_error(
                    ErrorCode::FileImportFailure,
                    "the file cannot be opened",
                    path,
                    32,
                ));
            }
        }
        self.inner.read_file(path)
    }
}

// ---- every state a journal can be left in --------------------------------------------------------

mod recover {
    use super::*;

    #[test]
    fn every_interrupted_state_is_put_back_and_every_finished_one_is_left_alone() {
        let mut lp = Loop::new("recover-states", &build_fixture_scenario);
        let p = measure_clean_apply(&mut lp);

        // The log of an update whose seventh rename fails, to find the rollback's own records.
        let (rolling, rolled) = {
            lp.reset();
            let s = lp.scenario();
            let bottom = StdFs;
            let faults = FaultFs::new(&bottom);
            faults.fail_rename_with(7, 32);
            let log = LogFs::new(&faults);
            let mut result = ApplyResult::default();
            assert!(apply_until_crash(s, &log, Some(&mut result)).is_none());
            assert_eq!(result.outcome, ApplyOutcome::RolledBack);
            let rolling = log.find("appendDurable", "ROLLING_BACK", 1);
            let rolled = log.find("appendDurable", "ROLLED_BACK", 1);
            assert!(rolling > 0);
            assert!(rolled > rolling);
            assert!(
                log.log.borrow().len() as u64 > rolled,
                "the rollback has cleaning to do after its last record"
            );
            (rolling, rolled)
        };

        struct Row {
            name: &'static str,
            crash_before: u64,
            fail_rename: u64,
            state: JournalState, // what the journal must say after the crash
            recovery_rolls_back: bool, // else: the tree is a whole version and recovery leaves it
            new_tree: bool,      // the tree after recovery is the new version
            folder_stays: bool,  // the transaction folder is still there after recovery
        }
        let rows = [
            Row {
                name: "planning, before the PLAN group",
                crash_before: p.plan,
                fail_rename: 0,
                state: JournalState::Empty,
                recovery_rolls_back: true,
                new_tree: false,
                folder_stays: false,
            },
            Row {
                name: "PLAN written, COMMITTING not",
                crash_before: p.committing,
                fail_rename: 0,
                state: JournalState::Planned,
                recovery_rolls_back: true,
                new_tree: false,
                folder_stays: false,
            },
            Row {
                name: "COMMITTING written, nothing moved",
                crash_before: p.renames[0],
                fail_rename: 0,
                state: JournalState::Committing,
                recovery_rolls_back: true,
                new_tree: false,
                folder_stays: false,
            },
            Row {
                name: "half the renames done",
                crash_before: p.renames[p.renames.len() / 2],
                fail_rename: 0,
                state: JournalState::Committing,
                recovery_rolls_back: true,
                new_tree: false,
                folder_stays: false,
            },
            Row {
                name: "every rename done, the pending record not",
                crash_before: p.pending,
                fail_rename: 0,
                state: JournalState::Committing,
                recovery_rolls_back: true,
                new_tree: false,
                folder_stays: false,
            },
            Row {
                name: "the apply's own rollback, part way",
                crash_before: rolling + 4,
                fail_rename: 7,
                state: JournalState::RollingBack,
                recovery_rolls_back: true,
                new_tree: false,
                folder_stays: false,
            },
            // The new version is in and not yet proved: whole, and the folder is what a health
            // rollback needs, so it is left exactly as it is.
            Row {
                name: "pending health, nothing more",
                crash_before: p.healthy,
                fail_rename: 0,
                state: JournalState::CommittedPendingHealth,
                recovery_rolls_back: false,
                new_tree: true,
                folder_stays: true,
            },
            // Over, with the clean-up cut short: the tree is whole and what is left is swept.
            Row {
                name: "healthy, the clean-up part way",
                crash_before: p.healthy + 2,
                fail_rename: 0,
                state: JournalState::Healthy,
                recovery_rolls_back: false,
                new_tree: true,
                folder_stays: false,
            },
            Row {
                name: "rolled back, the clean-up part way",
                crash_before: rolled + 2,
                fail_rename: 7,
                state: JournalState::RolledBack,
                recovery_rolls_back: false,
                new_tree: false,
                folder_stays: false,
            },
        ];
        for row in &rows {
            let name = row.name;
            crash_at(&mut lp, row.crash_before, row.fail_rename);
            let s = lp.scenario();
            assert_eq!(
                journal_state_of(&s.root),
                row.state,
                "{name}: the crash left another state than the row names"
            );
            let crashed_hash = tree_hash(&s.root);

            let bottom = StdFs;
            let counting = FaultFs::new(&bottom);
            let mut status: Status = Ok(());
            assert!(
                recover_until_crash(s, &counting, Some(&mut status)).is_none(),
                "{name}"
            );
            assert!(status.is_ok(), "{name}: {}", describe(&status));
            if row.recovery_rolls_back {
                assert_eq!(
                    tree_hash(&s.root),
                    s.v1_hash,
                    "{name}: {}",
                    difference(&s.v1_snapshot, &snapshot(&s.root))
                );
                assert!(
                    leftovers_in_update(&s.root).is_empty(),
                    "{name}: its folder is gone"
                );
            } else {
                let wanted = if row.new_tree { &s.v2_hash } else { &s.v1_hash };
                assert_eq!(&tree_hash(&s.root), wanted, "{name}");
                assert_eq!(
                    tree_hash(&s.root),
                    crashed_hash,
                    "{name}: the tree is left as it was"
                );
                assert_eq!(
                    leftovers_in_update(&s.root).is_empty(),
                    !row.folder_stays,
                    "{name}"
                );
                if row.folder_stays {
                    assert_eq!(
                        counting.mutating_operations(),
                        0,
                        "{name}: and not so much as written to"
                    );
                    assert_eq!(
                        journal_state_of(&s.root),
                        row.state,
                        "{name}: nor its journal"
                    );
                }
            }
        }
    }

    #[test]
    fn a_healthy_transaction_whose_clean_up_was_cut_short_is_swept_and_its_new_version_is_not_touched()
     {
        let mut lp = Loop::new("recover-sweep", &build_fixture_scenario);
        let p = measure_clean_apply(&mut lp);
        crash_at(&mut lp, p.healthy + 1, 0);
        let s = lp.scenario();
        assert_eq!(journal_state_of(&s.root), JournalState::Healthy);
        assert!(!leftovers_in_update(&s.root).is_empty());
        assert!(recover_clean(s).is_ok());
        assert!(leftovers_in_update(&s.root).is_empty());
        assert_eq!(tree_hash(&s.root), s.v2_hash);
    }

    #[test]
    fn the_recovery_is_idempotent_twice_changes_nothing_the_second_time() {
        let mut lp = Loop::new("recover-idempotent", &build_fixture_scenario);
        let p = measure_clean_apply(&mut lp);
        crash_at(&mut lp, p.renames[p.renames.len() / 2], 0);
        let s = lp.scenario();
        assert!(recover_clean(s).is_ok());
        let first = snapshot(&s.root);

        let bottom = StdFs;
        let counting = FaultFs::new(&bottom);
        let mut status: Status = Ok(());
        assert!(recover_until_crash(s, &counting, Some(&mut status)).is_none());
        assert!(status.is_ok());
        assert_eq!(counting.mutating_operations(), 0);
        assert_eq!(snapshot(&s.root), first);
    }
}

// ---- the lock ------------------------------------------------------------------------------------

mod recover_lock {
    use super::*;

    #[test]
    fn a_live_holder_is_waited_for_and_its_files_are_not_touched() {
        let mut lp = Loop::new("recover-lock-held", &build_fixture_scenario);
        let p = measure_clean_apply(&mut lp);
        crash_at(&mut lp, p.renames[p.renames.len() / 2], 0);
        let s = lp.scenario();
        let crashed_hash = tree_hash(&s.root);

        let disk = StdFs;
        let holder = disk.try_lock(&s.root.join(".update").join("lock"));
        assert!(holder.is_ok());
        let mut rig = Rig::with_transaction(s, &disk, "eeeeeeee");
        rig.timings.wait_for_holders = Duration::from_millis(100);
        let start = Instant::now();
        let status = recover_in_process(&s.root, &rig.services());
        let took = start.elapsed();
        assert!(status.is_ok(), "a live update is not a failure of recovery");
        assert!(took >= Duration::from_millis(90), "it waited");
        assert_eq!(tree_hash(&s.root), crashed_hash, "and touched nothing");
        assert_eq!(journal_state_of(&s.root), JournalState::Committing);
        drop(holder);
    }

    #[test]
    fn a_recovery_that_waited_for_the_holder_does_its_job_when_the_holder_lets_go() {
        let mut lp = Loop::new("recover-lock-released", &build_fixture_scenario);
        let p = measure_clean_apply(&mut lp);
        crash_at(&mut lp, p.renames[p.renames.len() / 2], 0);
        let s = lp.scenario();

        let disk = StdFs;
        // The lock's own type, which can be let go on another thread.
        let holder = disk
            .try_lock_owned(&s.root.join(".update").join("lock"))
            .expect("the lock can be taken");
        let mut rig = Rig::with_transaction(s, &disk, "eeeeeeee");
        rig.timings.wait_for_holders = Duration::from_millis(5000);
        let status = std::thread::scope(|scope| {
            scope.spawn(move || {
                std::thread::sleep(Duration::from_millis(60));
                drop(holder);
            });
            recover_in_process(&s.root, &rig.services())
        });
        assert!(status.is_ok(), "{}", describe(&status));
        assert_eq!(tree_hash(&s.root), s.v1_hash);
    }

    #[test]
    fn the_lock_is_free_again_when_recovery_ends() {
        let mut lp = Loop::new("recover-lock-free", &build_fixture_scenario);
        let p = measure_clean_apply(&mut lp);
        crash_at(&mut lp, p.renames[p.renames.len() / 2], 0);
        let s = lp.scenario();
        assert!(recover_clean(s).is_ok());
        let disk = StdFs;
        assert!(disk.try_lock(&s.root.join(".update").join("lock")).is_ok());
    }

    #[test]
    fn nothing_to_do_takes_no_lock_and_looks_at_one_folder() {
        let mut s = Scenario::new("recover-nothing");
        s.v1 = tiny("0.1.0", 1);
        write_install(&s.root, &s.v1);
        let disk = StdFs;
        {
            // An install that never updated has no .update at all.
            let watched = WatchedFs::new(&disk);
            let counting = FaultFs::new(&watched);
            let rig = Rig::with_transaction(&s, &counting, "eeeeeeee");
            assert!(recover_in_process(&s.root, &rig.services()).is_ok());
            assert_eq!(counting.mutating_operations(), 0);
            assert_eq!(watched.locks.get(), 0);
            assert!(
                !is_present(&s.root.join(".update")),
                "recovery does not make the folder"
            );
        }
        {
            // One that did has the lock file and nothing else.
            write_text(&s.root.join(".update").join("lock"), "");
            let watched = WatchedFs::new(&disk);
            let counting = FaultFs::new(&watched);
            let rig = Rig::with_transaction(&s, &counting, "eeeeeeee");
            assert!(recover_in_process(&s.root, &rig.services()).is_ok());
            assert_eq!(counting.mutating_operations(), 0);
            assert_eq!(
                watched.locks.get(),
                0,
                "nothing interrupted, so the lock is not even asked for"
            );
            assert_eq!(watched.listings.get(), 1, "one directory listing");
        }
    }
}

// ---- a recovery that cannot finish ---------------------------------------------------------------

mod recover_failure {
    use super::*;

    #[test]
    fn a_recovery_that_cannot_finish_says_the_command_to_run_by_hand() {
        let mut lp = Loop::new("recover-cannot", &build_fixture_scenario);
        let p = measure_clean_apply(&mut lp);
        crash_at(&mut lp, p.renames[p.renames.len() / 2], 0);
        let s = lp.scenario();

        let disk = StdFs;
        let hooked = HookFs::new(&disk);
        hooked.set_before_rename(|from, _| Some(injected(32, from))); // a file that stays locked
        let rig = Rig::with_transaction(s, &hooked, "eeeeeeee");
        let status = recover_in_process(&s.root, &rig.services());
        let error = status.expect_err("the recovery cannot finish");
        assert!(
            error.message.contains("katana_updater --recover"),
            "{}",
            error.describe()
        );
        assert!(error.message.contains(&text(&s.root)));
        // The folder with the old files is still there for the next try.
        assert_eq!(journal_state_of(&s.root), JournalState::RollingBack);
        assert!(!leftovers_in_update(&s.root).is_empty());
        assert_ne!(tree_hash(&s.root), s.v1_hash);

        // And the next try, with the file free, finishes it.
        assert!(recover_clean(s).is_ok());
        assert_eq!(
            tree_hash(&s.root),
            s.v1_hash,
            "{}",
            difference(&s.v1_snapshot, &snapshot(&s.root))
        );
        assert!(leftovers_in_update(&s.root).is_empty());
    }

    #[test]
    fn the_run_once_entry_is_cleared_when_a_recovery_succeeds_and_kept_when_it_fails() {
        let mut lp = Loop::new("recover-runonce", &build_fixture_scenario);
        let p = measure_clean_apply(&mut lp);
        crash_at(&mut lp, p.renames[p.renames.len() / 2], 0);
        let s = lp.scenario();

        let disk = StdFs;
        let hooked = HookFs::new(&disk);
        hooked.set_before_rename(|from, _| Some(injected(5, from)));
        {
            let rig = Rig::with_transaction(s, &hooked, "eeeeeeee");
            rig.registry
                .run_once
                .borrow_mut()
                .insert(String::from(RUN_ONCE_NAME), String::from("x"));
            assert!(recover_in_process(&s.root, &rig.services()).is_err());
            assert!(rig.registry.run_once.borrow().contains_key(RUN_ONCE_NAME));
        }
        {
            let rig = Rig::with_transaction(s, &disk, "eeeeeeee");
            rig.registry
                .run_once
                .borrow_mut()
                .insert(String::from(RUN_ONCE_NAME), String::from("x"));
            assert!(recover_in_process(&s.root, &rig.services()).is_ok());
            assert!(rig.registry.run_once.borrow().is_empty());
        }
    }
}

// ---- journals nobody trusts ----------------------------------------------------------------------

// A root with v1 installed, and a transaction folder `deadbeef` that holds whatever the test puts
// in it, for a hand-made journal.
struct Forgery {
    s: Scenario,
    work: PathBuf,
}

impl Forgery {
    fn new(label: &str) -> Self {
        let mut s = Scenario::new(label);
        s.v1 = tiny("0.1.0", 100);
        write_install(&s.root, &s.v1);
        let work = s.root.join(".update").join("deadbeef");
        // The legitimate part: bin/lib.dll was moved aside and the new one is in place.
        write_text(&s.root.join("bin/lib.dll"), "the new lib\n");
        write_text(&work.join("old/bin/lib.dll"), "lib 0.1.0\n");
        Self { s, work }
    }

    fn journal(&self, records: &[JournalRecord]) {
        write_text(&self.work.join("journal"), encode_all(records));
    }

    // Everything under the scratch folder but the transaction's own working folder.
    fn outside(&self) -> BTreeMap<String, String> {
        let mut entries = snapshot(self.s.scratch.path());
        entries.retain(|name, _| !name.starts_with("install/.update"));
        entries
    }
}

fn file_entry(bytes: &[u8]) -> String {
    format!("file {} {}", bytes.len(), sha256_hex(bytes))
}

mod recover_forgery {
    use super::*;

    #[test]
    fn a_path_that_leaves_the_install_is_skipped_and_nothing_outside_is_touched() {
        let f = Forgery::new("recover-forged-paths");
        let outside = f.s.scratch.path().to_path_buf();
        write_text(&outside.join("victim.txt"), "victim bytes");
        make_directories(&outside.join("emptydir"));
        // Planted for a naive restore to carry out: old/../planted.txt is work/planted.txt.
        write_text(&f.work.join("planted.txt"), "attacker bytes");
        write_text(
            &f.work.join("old/share/katana/INSTALL.JSON"),
            "evil install record",
        );
        write_text(&f.work.join("old/.update/lock"), "evil");
        write_text(&f.work.join("planted2.txt"), "attacker bytes");
        let absolute = text(&outside.join("abs_target.txt"));
        write_text(
            &f.work
                .join("old")
                .join(relative_path(&path_from_utf8(&absolute))),
            "attacker bytes",
        );

        f.journal(&[
            record(JournalOp::Plan, &["deadbeef", "100", "200"]),
            record(JournalOp::MakeDirectory, &["../emptydir"]),
            record(JournalOp::Replace, &["bin/lib.dll"]), // the one honest record
            record(JournalOp::Replace, &["../planted.txt"]),
            record(JournalOp::Add, &["../victim.txt"]),
            record(JournalOp::Replace, &["share/katana/INSTALL.JSON"]),
            record(JournalOp::Replace, &[".update/lock"]),
            record(JournalOp::Replace, &[absolute.as_str()]),
            record(JournalOp::Replace, &["bin\\..\\..\\planted2.txt"]),
            record(JournalOp::Committing, &[]),
        ]);
        let before = f.outside();

        let disk = StdFs;
        let rig = Rig::with_transaction(&f.s, &disk, "eeeeeeee");
        let status = recover_in_process(&f.s.root, &rig.services());
        assert!(status.is_ok(), "{}", describe(&status));

        // The honest record was carried out.
        assert_eq!(read_text(&f.s.root.join("bin/lib.dll")), b"lib 0.1.0\n");
        // Nothing outside bin, share, etc and lib of the root changed, and nothing was made.
        let mut expected = before;
        expected.insert(
            String::from("install/bin/lib.dll"),
            file_entry(b"lib 0.1.0\n"),
        );
        assert_eq!(
            f.outside(),
            expected,
            "{}",
            difference(&expected, &f.outside())
        );
        assert!(!is_present(&outside.join("planted.txt")));
        assert!(!is_present(&outside.join("planted2.txt")));
        assert!(!is_present(&outside.join("abs_target.txt")));
        assert_eq!(read_text(&outside.join("victim.txt")), b"victim bytes");
        assert!(outside.join("emptydir").is_dir());
        assert_eq!(
            read_text(&f.s.root.join("share/katana/install.json")),
            INSTALL_JSON.as_bytes()
        );
        assert!(
            !(is_present(&f.s.root.join("share/katana/INSTALL.JSON"))
                && read_text(&f.s.root.join("share/katana/INSTALL.JSON"))
                    == b"evil install record")
        );

        // Each skipped record is reported, in a file beside the lock.
        let log =
            String::from_utf8_lossy(&read_text(&f.s.root.join(".update").join("recovery.log")))
                .into_owned();
        for refused in [
            "../planted.txt",
            "../victim.txt",
            "INSTALL.JSON",
            ".update/lock",
            "../emptydir",
        ] {
            assert!(log.contains(refused), "not reported: {refused}\n{log}");
        }
        assert!(log.contains("deadbeef"), "and by transaction");
    }

    #[test]
    fn a_path_with_a_dot_dot_inside_a_good_first_component_is_not_followed_either() {
        let f = Forgery::new("recover-forged-dotdot");
        write_text(&f.s.scratch.join("victim2.txt"), "victim bytes");
        // = work/planted3.txt
        write_text(
            &f.work.join("old/bin/x/../../../planted3.txt"),
            "attacker bytes",
        );
        f.journal(&[
            record(JournalOp::Plan, &["deadbeef", "1", "2"]),
            record(JournalOp::Replace, &["bin/../../planted3.txt"]),
            record(JournalOp::Add, &["share/../../victim2.txt"]),
            record(JournalOp::Committing, &[]),
        ]);
        let disk = StdFs;
        let rig = Rig::with_transaction(&f.s, &disk, "eeeeeeee");
        assert!(recover_in_process(&f.s.root, &rig.services()).is_ok());
        assert!(!is_present(&f.s.scratch.join("planted3.txt")));
        assert_eq!(read_text(&f.s.scratch.join("victim2.txt")), b"victim bytes");
    }

    #[test]
    fn a_folder_whose_name_is_not_a_transaction_id_is_never_entered_whatever_its_journal_says() {
        let mut f = Forgery::new("recover-forged-folder");
        // The honest content, evil name.
        let evil = f.s.root.join(".update").join("EVIL");
        std::fs::rename(&f.work, &evil).expect("the folder can be renamed");
        f.work = evil;
        f.journal(&[
            record(JournalOp::Plan, &["EVIL", "1", "2"]),
            record(JournalOp::Replace, &["bin/lib.dll"]),
            record(JournalOp::Committing, &[]),
        ]);
        for name in [
            "..",
            "a b",
            "g1",
            "not-a-transaction",
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0",
        ] {
            let other = f.s.root.join(".update").join(name);
            if name == ".." {
                continue; // not a name a directory listing yields
            }
            write_text(&other.join("old/bin/lib.dll"), "lib 0.1.0\n");
            write_text(&other.join("journal"), read_text(&f.work.join("journal")));
        }
        let disk = StdFs;
        let rig = Rig::with_transaction(&f.s, &disk, "eeeeeeee");
        assert!(recover_in_process(&f.s.root, &rig.services()).is_ok());
        assert_eq!(
            read_text(&f.s.root.join("bin/lib.dll")),
            b"the new lib\n",
            "none of them was acted on"
        );
        assert!(is_present(&f.work.join("old/bin/lib.dll")));
    }

    #[test]
    fn a_transaction_id_passed_to_rollback_directly_is_checked_too() {
        let f = Forgery::new("recover-forged-direct");
        let disk = StdFs;
        let rig = Rig::with_transaction(&f.s, &disk, "eeeeeeee");
        for id in ["../x", "A", "", "a/b"] {
            let status = rollback(&f.s.root, id, &rig.services());
            let error = status.expect_err(id);
            assert_eq!(error.code, ErrorCode::InvalidArgument, "{id}");
        }
    }

    #[test]
    fn a_folder_the_journal_names_that_is_a_link_is_not_restored_through_and_the_failure_is_reported()
     {
        let f = Forgery::new("recover-forged-link");
        f.journal(&[
            record(JournalOp::Plan, &["deadbeef", "1", "2"]),
            record(JournalOp::Replace, &["bin/lib.dll"]),
            record(JournalOp::Committing, &[]),
        ]);
        let disk = StdFs;
        let hooked = HookFs::new(&disk);
        let bin = f.s.root.join("bin");
        hooked.set_adjust_stat(move |path, info| {
            if path == bin.as_path() {
                info.reparse_point = true; // a junction to somewhere else
            }
        });
        let rig = Rig::with_transaction(&f.s, &hooked, "eeeeeeee");
        let status = recover_in_process(&f.s.root, &rig.services());
        let error = status.expect_err("the link is not restored through");
        assert!(error.message.contains("katana_updater --recover"));
        assert_eq!(
            read_text(&f.s.root.join("bin/lib.dll")),
            b"the new lib\n",
            "nothing was moved through the link"
        );
    }

    #[test]
    fn a_link_inside_old_or_new_is_not_moved_through_either_and_the_failure_is_reported() {
        // old/<dir> and new/<dir> are in the transaction's folder, which is in the install, but a
        // junction there leads anywhere: a file renamed from, or into, one comes from or goes to a
        // place outside the install. The check on the install's side alone does not see it.
        for side in ["old", "new"] {
            let f = Forgery::new(&format!("recover-forged-link-{side}"));
            make_directories(&f.work.join("new/bin")); // as the plan made it
            f.journal(&[
                record(JournalOp::Plan, &["deadbeef", "1", "2"]),
                record(JournalOp::Replace, &["bin/lib.dll"]),
                record(JournalOp::Committing, &[]),
            ]);
            let disk = StdFs;
            let hooked = HookFs::new(&disk);
            let linked = f.work.join(side).join("bin");
            hooked.set_adjust_stat(move |path, info| {
                if path == linked.as_path() {
                    info.reparse_point = true; // a junction to somewhere else
                }
            });
            let rig = Rig::with_transaction(&f.s, &hooked, "eeeeeeee");
            let status = recover_in_process(&f.s.root, &rig.services());
            let error = status.expect_err(side);
            assert!(error.message.contains("katana_updater --recover"), "{side}");
            assert_eq!(
                read_text(&f.s.root.join("bin/lib.dll")),
                b"the new lib\n",
                "{side}: nothing was moved through the link"
            );
            assert!(
                is_present(&f.work.join("old/bin/lib.dll")),
                "{side}: and the old file is where it was"
            );
            assert!(!is_present(&f.work.join("new/bin/lib.dll")), "{side}");
        }
    }

    #[test]
    fn a_control_character_in_a_refused_path_cannot_make_the_log_say_something_else() {
        let f = Forgery::new("recover-forged-logline");
        // A line feed and a carriage return, percent-encoded as the journal writes them.
        f.journal(&[
            record(JournalOp::Plan, &["deadbeef", "1", "2"]),
            record(JournalOp::Replace, &["bin/lib.dll"]),
            record(
                JournalOp::Replace,
                &["../a\ndeadbeef journal record refused: FORGED\r\nmore"],
            ),
            record(JournalOp::Committing, &[]),
        ]);
        let disk = StdFs;
        let rig = Rig::with_transaction(&f.s, &disk, "eeeeeeee");
        assert!(recover_in_process(&f.s.root, &rig.services()).is_ok());
        assert_eq!(read_text(&f.s.root.join("bin/lib.dll")), b"lib 0.1.0\n");

        let log = read_text(&f.s.root.join(".update").join("recovery.log"));
        let shown = String::from_utf8_lossy(&log).into_owned();
        // One refused record is one line: the path's own line feed and carriage return are shown
        // as %0A and %0D, from the byte values.
        assert_eq!(
            log.iter().filter(|&&byte| byte == b'\n').count(),
            1,
            "{shown}"
        );
        assert!(!log.contains(&b'\r'), "{shown}");
        assert!(
            log.starts_with(
                b"deadbeef journal record refused: REPLACE ../a%0Adeadbeef journal record \
                  refused: FORGED%0D%0Amore: "
            ),
            "{shown}"
        );
    }

    #[test]
    fn a_torn_journal_and_an_empty_one_are_junk_not_a_reason_to_touch_the_install() {
        {
            // A crash during the PLAN group's write: the journal has a good prefix and a torn line.
            let f = Forgery::new("recover-torn");
            let whole = encode_record(1, &record(JournalOp::Plan, &["deadbeef", "1", "2"]))
                + &encode_record(2, &record(JournalOp::Replace, &["bin/lib.dll"]));
            write_text(
                &f.work.join("journal"),
                &whole.as_bytes()[..whole.len() - 7],
            );
            // The honest tree: nothing had moved, so the new lib is NOT in place; the old one is.
            write_text(&f.s.root.join("bin/lib.dll"), "lib 0.1.0\n");
            remove(&f.work.join("old/bin/lib.dll"));
            let before = tree_hash(&f.s.root);
            assert!(recover_clean(&f.s).is_ok());
            assert_eq!(tree_hash(&f.s.root), before);
            assert!(
                leftovers_in_update(&f.s.root).is_empty(),
                "its folder is junk and goes"
            );
        }
        {
            let f = Forgery::new("recover-empty-journal");
            write_text(&f.work.join("journal"), "");
            write_text(&f.s.root.join("bin/lib.dll"), "lib 0.1.0\n");
            let before = tree_hash(&f.s.root);
            assert!(recover_clean(&f.s).is_ok());
            assert_eq!(tree_hash(&f.s.root), before);
            assert!(leftovers_in_update(&f.s.root).is_empty());
        }
        {
            let f = Forgery::new("recover-no-journal");
            write_text(&f.s.root.join("bin/lib.dll"), "lib 0.1.0\n");
            let before = tree_hash(&f.s.root);
            assert!(recover_clean(&f.s).is_ok());
            assert_eq!(tree_hash(&f.s.root), before);
            assert!(leftovers_in_update(&f.s.root).is_empty());
        }
    }
}

mod recover_boundary {
    use super::*;

    #[test]
    fn an_exception_from_the_file_system_is_an_error_to_report_and_not_a_crash() {
        let f = Forgery::new("recover-throws");
        f.journal(&[
            record(JournalOp::Plan, &["deadbeef", "1", "2"]),
            record(JournalOp::Replace, &["bin/lib.dll"]),
            record(JournalOp::Committing, &[]),
        ]);
        let disk = StdFs;
        // The C++ throws a std::runtime_error from stat; what throws there panics here.
        let throwing = ForwardingFs::new(&disk);
        throwing.on_stat(|_, _| panic!("injected: a conversion or an allocation failed"));

        {
            let rig = Rig::with_transaction(&f.s, &throwing, "eeeeeeee");
            let in_process = no_panic(|| recover_in_process(&f.s.root, &rig.services()));
            assert!(in_process.is_err());
            let rolled = no_panic(|| rollback(&f.s.root, "deadbeef", &rig.services()));
            assert!(rolled.is_err());
        }
        let launcher = FakeLauncher::default();
        let with_helper = no_panic(|| {
            detail::recover_with(
                &f.s.root,
                &throwing,
                &launcher,
                &f.s.state,
                Duration::from_millis(50),
                ORDINARY_START,
            )
        });
        assert!(
            with_helper.is_err(),
            "recover() says so rather than dying: the caller reports it"
        );
        assert!(launcher.ran().is_empty());
        assert_eq!(
            read_text(&f.s.root.join("bin/lib.dll")),
            b"the new lib\n",
            "and nothing was touched"
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_name_that_is_not_text_in_the_update_folder_or_in_the_old_files_cannot_stop_a_program_starting()
     {
        use std::os::windows::ffi::OsStringExt;

        // NTFS allows a name with an unpaired surrogate; converting it to UTF-8 panics here (it
        // throws in the C++).
        let f = Forgery::new("recover-odd-name");
        f.journal(&[
            record(JournalOp::Plan, &["deadbeef", "1", "2"]),
            record(JournalOp::Replace, &["bin/lib.dll"]),
            record(JournalOp::Committing, &[]),
        ]);
        let odd_name = PathBuf::from(std::ffi::OsString::from_wide(&[
            u16::from(b'a'),
            0xD800,
            u16::from(b'b'),
        ]));
        let odd_folder = f.s.root.join(".update").join(&odd_name);
        let odd_file = f.work.join("old/bin").join(&odd_name);
        std::fs::create_dir(&odd_folder).expect("the folder can be made");
        write_text(&odd_file, "a file nobody can name in text");
        assert!(
            catch_unwind(AssertUnwindSafe(|| path_to_utf8(&odd_name))).is_err(),
            "the premise of the test"
        );

        let disk = StdFs;
        let rig = Rig::with_transaction(&f.s, &disk, "eeeeeeee");
        let status = no_panic(|| recover_in_process(&f.s.root, &rig.services()));
        assert!(status.is_ok(), "{}", describe(&status));
        assert_eq!(
            read_text(&f.s.root.join("bin/lib.dll")),
            b"lib 0.1.0\n",
            "the interrupted update is still put back"
        );

        // The system seams too: recover() is the first thing a program does.
        let launcher = FakeLauncher::default();
        let first = no_panic(|| {
            detail::recover_with(
                &f.s.root,
                &disk,
                &launcher,
                &f.s.state,
                Duration::from_millis(50),
                ORDINARY_START,
            )
        });
        assert!(first.is_ok(), "{}", describe(&first));
        let _ = std::fs::remove_dir(&odd_folder);
    }

    #[test]
    fn a_folder_planted_far_deeper_than_an_install_is_left_and_recovery_still_returns() {
        // A finished transaction's folder is swept by every program's first lines. A nest planted
        // to make that recursion run out of stack must not be entered without a bound.
        let f = Forgery::new("recover-deep");
        f.journal(&[
            record(JournalOp::Plan, &["deadbeef", "1", "2"]),
            record(JournalOp::Replace, &["bin/lib.dll"]),
            record(JournalOp::Committing, &[]),
            record(JournalOp::CommittedPendingHealth, &[]),
            record(JournalOp::Healthy, &[]),
        ]);
        let mut deep = f.work.join("new");
        for _ in 0..66 {
            // more than the 64 the clean-up goes down
            deep.push("d");
        }
        make_directories(&deep);

        let disk = StdFs;
        let rig = Rig::with_transaction(&f.s, &disk, "eeeeeeee");
        let status = no_panic(|| recover_in_process(&f.s.root, &rig.services()));
        assert!(status.is_ok(), "{}", describe(&status));
        assert!(deep.is_dir(), "past the bound it is left, not walked");
        assert!(
            !is_present(&f.work.join("old")),
            "and what is within the bound still goes"
        );
        assert_eq!(
            read_text(&f.s.root.join("bin/lib.dll")),
            b"the new lib\n",
            "a finished transaction is not rolled back"
        );
        // Nothing of an ordinary size is affected: the fixture-sized folders in the other tests
        // are swept whole (every_interrupted_state_is...).
    }
}

// ---- rollback, called directly -------------------------------------------------------------------

// The id of the one transaction under R/.update: its folder.
fn only_transaction(s: &Scenario) -> String {
    let mut tx = String::new();
    if let Ok(entries) = std::fs::read_dir(s.root.join(".update")) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                tx = text(Path::new(&entry.file_name()));
            }
        }
    }
    tx
}

mod rollback {
    use super::*;

    #[test]
    fn a_rollback_of_a_half_swapped_tree_is_exact_and_running_it_again_changes_nothing() {
        let mut lp = Loop::new("rollback-direct", &build_fixture_scenario);
        let p = measure_clean_apply(&mut lp);
        crash_at(&mut lp, p.renames[p.renames.len() / 2], 0);
        let s = lp.scenario();
        // The transaction's id: its folder.
        let tx = only_transaction(s);
        assert!(!tx.is_empty());

        let disk = StdFs;
        let rig = Rig::with_transaction(s, &disk, "eeeeeeee");
        assert!(rollback(&s.root, &tx, &rig.services()).is_ok());
        assert_eq!(
            tree_hash(&s.root),
            s.v1_hash,
            "{}",
            difference(&s.v1_snapshot, &snapshot(&s.root))
        );
        assert!(!is_present(&s.root.join(".update").join(&tx)));

        // Again: the folder is gone, so there is nothing to do, and that is success.
        let counting = FaultFs::new(&disk);
        let again = Rig::with_transaction(s, &counting, "eeeeeeee");
        assert!(rollback(&s.root, &tx, &again.services()).is_ok());
        assert_eq!(counting.mutating_operations(), 0);
        assert_eq!(tree_hash(&s.root), s.v1_hash);
    }

    #[test]
    fn a_transaction_that_is_healthy_is_never_rolled_back() {
        let mut lp = Loop::new("rollback-healthy", &build_fixture_scenario);
        let p = measure_clean_apply(&mut lp);
        crash_at(&mut lp, p.healthy + 2, 0);
        let s = lp.scenario();
        let tx = only_transaction(s);
        assert!(!tx.is_empty());
        let disk = StdFs;
        let rig = Rig::with_transaction(s, &disk, "eeeeeeee");
        assert!(rollback(&s.root, &tx, &rig.services()).is_ok());
        assert_eq!(
            tree_hash(&s.root),
            s.v2_hash,
            "the new version proved it starts"
        );
    }

    #[test]
    fn a_running_new_program_is_moved_out_of_the_way_not_deleted() {
        // A health-check rollback happens while the new version may still be running: a running
        // program cannot be deleted, so rollback renames it. A file system that refuses
        // remove_file (as Windows does for a running image) must therefore not stop the rollback.
        let mut lp = Loop::new("rollback-running", &build_fixture_scenario);
        let p = measure_clean_apply(&mut lp);
        crash_at(&mut lp, p.pending, 0); // every file is in place; the pending record is not written
        let s = lp.scenario();
        let disk = StdFs;
        let refusing = ForwardingFs::new(&disk);
        refusing.on_remove_file(|inner, path| {
            // Only the files of the install are "running": the clean-up of the folder is free.
            if text(path).contains(".update") {
                inner.remove_file(path)
            } else {
                Err(os_error(
                    ErrorCode::FileExportFailure,
                    "access denied",
                    path,
                    5,
                ))
            }
        });
        let rig = Rig::with_transaction(s, &refusing, "eeeeeeee");
        let status = recover_in_process(&s.root, &rig.services());
        assert!(status.is_ok(), "{}", describe(&status));
        assert_eq!(
            tree_hash(&s.root),
            s.v1_hash,
            "{}",
            difference(&s.v1_snapshot, &snapshot(&s.root))
        );
    }
}

// ---- an update started over an interrupted one ---------------------------------------------------

mod recover_before_apply {
    use super::*;

    #[test]
    fn an_update_started_over_an_interrupted_one_first_puts_the_old_version_back() {
        // tx1 died in the middle of its commit and nothing has run recovery yet. A second update
        // planned over that half-swapped tree would succeed, and the next start would roll the OLD
        // files of tx1 over the new version: a mix, with the wrong installed.json.
        let mut lp = Loop::new("apply-over-interrupted", &build_fixture_scenario);
        let p = measure_clean_apply(&mut lp);
        crash_at(&mut lp, p.renames[p.renames.len() / 2], 0);
        let s = lp.scenario();
        let crashed = tree_hash(&s.root);
        assert_ne!(crashed, s.v1_hash);
        assert_ne!(crashed, s.v2_hash, "half swapped");

        let disk = StdFs;
        let second = Rig::with_transaction(s, &disk, "bbbbbbbb");
        let result = second.run();
        assert_eq!(
            result.outcome,
            ApplyOutcome::Committed,
            "{}: {}",
            result.reason,
            result.detail
        );
        assert_eq!(
            tree_hash(&s.root),
            s.v2_hash,
            "{}",
            difference(&snapshot(&s.expected), &snapshot(&s.root))
        );
        assert!(
            leftovers_in_update(&s.root).is_empty(),
            "and the interrupted transaction's folder is gone"
        );
    }

    #[test]
    fn an_interrupted_update_that_cannot_be_put_back_stops_the_new_one_before_it_touches_anything()
    {
        let mut lp = Loop::new("apply-over-stuck", &build_fixture_scenario);
        let p = measure_clean_apply(&mut lp);
        crash_at(&mut lp, p.renames[p.renames.len() / 2], 0);
        let s = lp.scenario();
        let crashed = tree_hash(&s.root);

        let disk = StdFs;
        let hooked = HookFs::new(&disk);
        hooked.set_before_rename(|from, _| Some(injected(32, from))); // a file that stays locked
        let second = Rig::with_transaction(s, &hooked, "bbbbbbbb");
        let result = second.run();
        assert_eq!(result.outcome, ApplyOutcome::Refused);
        assert_eq!(result.reason, "interrupted_update", "{}", result.detail);
        assert!(
            result.detail.contains("katana_updater --recover"),
            "{}",
            result.detail
        );
        assert_eq!(
            tree_hash(&s.root),
            crashed,
            "nothing of the new update was done"
        );
        assert!(
            !is_present(&s.root.join(".update").join("bbbbbbbb")),
            "and it made no folder of its own"
        );
        assert!(second.registry.run_once.borrow().is_empty());

        // The old files are still in the first transaction's folder, and the next try finishes it.
        assert!(recover_clean(s).is_ok());
        assert_eq!(
            tree_hash(&s.root),
            s.v1_hash,
            "{}",
            difference(&s.v1_snapshot, &snapshot(&s.root))
        );
    }
}

// ---- a pending folder whose helper died, met by the next update ----------------------------------
//
// A swap settles, in its first step, an earlier update that is in place and was never said to
// start (apply.rs, step 1; the rule is recover_internal.rs's settle_pending): healthy when the
// marker of that update is there, and otherwise nothing new is started over it. So such a folder
// never outlives the next update, and there is one at most.

// The versions of the releases an update is APPLIED from and to in this file's own scenarios. They
// are versions no build of Katana has, on purpose, and these tests do not know which build they are
// for two reasons (the twin of test_apply.cpp, at its OLD_VERSION, has both and the cases that hold
// them). The harness's install.json names the version of its package (apply_harness, INSTALL_JSON),
// and the program that reads such an install is not asked for its own (install.rs): what is
// installed is what the records say. And should that record say its target and no more again - the
// installed version is then the higher of installed.json's and the asking program's, so a record
// BELOW the version this suite was built as reads as the program's version, and a staged release is
// the newer one only while the project's version has not passed it; after that every such update is
// refused as a `downgrade`, by a suite that had passed the day before - above any project version
// installed.json is still the higher. (The installs that are only recovered, never updated, keep
// their small versions: nothing compares them. The rule itself, both halves of it, is
// tests/update/test_state.cpp's.)
const FIRST_VERSION: &str = "900.1.0";
const SECOND_VERSION: &str = "900.2.0";
const THIRD_VERSION: &str = "900.3.0";

// v1 installed and v2 staged, from small specifications; `expected` is v2's tree.
fn build_tiny_loop(s: &mut Scenario) {
    s.v1 = tiny(FIRST_VERSION, 100);
    s.v2 = tiny(SECOND_VERSION, 200);
    write_install(&s.root, &s.v1);
    write_stage(&s.stage, &s.v2);
    write_install(&s.expected, &s.v2);
    s.v1_hash = tree_hash(&s.root);
    s.v1_snapshot = snapshot(&s.root);
    s.v2_hash = tree_hash(&s.expected);
}

// Every entry of the install folder, its working folder .update INCLUDED (snapshot() leaves that one
// out): what "the install is exactly as it was" compares, journal and old files too. Only the lock
// file is left out: asking for the lock makes it, and an empty lock file is no state.
fn whole_install(s: &Scenario) -> BTreeMap<String, String> {
    let mut entries = snapshot(s.root.parent().unwrap_or(Path::new("")));
    let name = s
        .root
        .file_name()
        .map(|name| text(Path::new(name)))
        .unwrap_or_default();
    let prefix = format!("{name}/");
    let lock = format!("{prefix}.update/lock");
    entries.retain(|entry, _| entry.starts_with(&prefix) && *entry != lock);
    entries
}

mod recover_superseded {
    use super::*;

    #[test]
    fn an_update_that_proves_healthy_deletes_the_pending_folder_an_earlier_helper_left() {
        crate::require_can_sign!();
        let mut lp = Loop::new("superseded", &build_tiny_loop);
        let p = measure_clean_apply(&mut lp);

        // tx1 committed and passed its health check, and its helper died before it wrote HEALTHY.
        crash_at(&mut lp, p.healthy, 0);
        let s = lp.scenario();
        assert_eq!(
            journal_state_of(&s.root),
            JournalState::CommittedPendingHealth
        );
        assert_eq!(tree_hash(&s.root), s.v2_hash);
        let first = only_transaction(s);
        assert_eq!(first, "a1b2c3d4");
        write_text(
            &s.state
                .join(format!("run-{first}"))
                .join("katana_updater.exe"),
            "the dead helper's copy",
        );
        assert!(
            is_present(&health_marker_path(&s.state, &first)),
            "written when it was started"
        );

        // A later release, installed by a window that was running tx1's version.
        let v3 = tiny(THIRD_VERSION, 300);
        remove_all(&s.stage);
        write_stage(&s.stage, &v3);
        let expected3 = s.scratch.join("expected3");
        write_install(&expected3, &v3);

        let disk = StdFs;
        let second = Rig::with_transaction(s, &disk, "bbbbbbbb");
        let result = second.run();
        assert_eq!(
            result.outcome,
            ApplyOutcome::Committed,
            "{}: {}",
            result.reason,
            result.detail
        );
        assert_eq!(tree_hash(&s.root), tree_hash(&expected3));
        assert!(
            leftovers_in_update(&s.root).is_empty(),
            "the earlier folder, with the old files of the release before, is gone with its own"
        );
        assert!(
            !is_present(&s.state.join(format!("run-{first}"))),
            "and so is its helper copy"
        );
        assert!(
            !is_present(&health_marker_path(&s.state, &first)),
            "and its marker"
        );
    }

    #[test]
    fn an_earlier_update_whose_marker_is_there_is_settled_first_whatever_becomes_of_the_next_one() {
        // Until 2026-10-14 this case was named AnUpdateThatDoesNotProveHealthyLeavesThePendingFolder
        // WhereItWas and expected the earlier folder to stay: a later update swept it only when the
        // later one proved healthy. The rule changed, not the code's output: an earlier update with
        // its marker is settled healthy in the next swap's first step, on that marker alone, and
        // what the later release then does - here it does not start - is another matter.
        crate::require_can_sign!();
        let mut lp = Loop::new("superseded-not", &build_tiny_loop);
        let p = measure_clean_apply(&mut lp);
        crash_at(&mut lp, p.healthy, 0);
        let s = lp.scenario();
        assert_eq!(
            journal_state_of(&s.root),
            JournalState::CommittedPendingHealth
        );
        let first = only_transaction(s);
        write_text(
            &s.state
                .join(format!("run-{first}"))
                .join("katana_updater.exe"),
            "the dead helper's copy",
        );
        assert!(
            is_present(&health_marker_path(&s.state, &first)),
            "the premise: its marker is there"
        );

        remove_all(&s.stage);
        write_stage(&s.stage, &tiny(THIRD_VERSION, 300));
        let disk = StdFs;
        let second = Rig::with_transaction(s, &disk, "bbbbbbbb");
        second.launcher.set_on_run(|_| {
            // the third release does not start
            Ok(SpawnResult {
                exit_code: 1,
                ..SpawnResult::default()
            })
        });
        let result = second.run();
        assert_eq!(
            result.outcome,
            ApplyOutcome::RolledBack,
            "{}: {}",
            result.reason,
            result.detail
        );
        assert_eq!(tree_hash(&s.root), s.v2_hash, "back to the second release");
        assert!(
            leftovers_in_update(&s.root).is_empty(),
            "the second release had said that it starts: its folder went before the third was \
             planned"
        );
        assert!(
            !is_present(&s.state.join(format!("run-{first}"))),
            "and with it its dead helper's copy"
        );
        assert!(
            !is_present(&health_marker_path(&s.state, &first)),
            "and its marker"
        );
    }

    #[test]
    fn an_earlier_update_that_was_never_said_to_start_stops_the_next_one_before_it_touches_anything()
     {
        crate::require_can_sign!();
        let mut lp = Loop::new("superseded-unproved", &build_tiny_loop);
        let p = measure_clean_apply(&mut lp);
        crash_at(&mut lp, p.healthy, 0);
        let s = lp.scenario();
        assert_eq!(
            journal_state_of(&s.root),
            JournalState::CommittedPendingHealth
        );
        assert_eq!(tree_hash(&s.root), s.v2_hash);
        let first = only_transaction(s);
        write_text(
            &s.state
                .join(format!("run-{first}"))
                .join("katana_updater.exe"),
            "the dead helper's copy",
        );
        // Nothing says that the second release ever started: its marker is not there.
        remove(&health_marker_path(&s.state, &first));

        let v3 = tiny(THIRD_VERSION, 300);
        remove_all(&s.stage);
        write_stage(&s.stage, &v3);
        let install_before = whole_install(s);
        let stage_before = snapshot(&s.stage);

        let disk = StdFs;
        let second = Rig::with_transaction(s, &disk, "bbbbbbbb");
        // The logon entry the helper that died had registered, which names its copy in U.
        second.registry.run_once.borrow_mut().insert(
            String::from(RUN_ONCE_NAME),
            String::from("the dead helper's entry"),
        );
        let result = second.run();
        assert_eq!(result.outcome, ApplyOutcome::Refused);
        assert_eq!(result.reason, "interrupted_update", "{}", result.detail);
        assert_eq!(
            tree_hash(&s.root),
            s.v2_hash,
            "the second release is still what is installed"
        );
        assert!(
            !is_present(&s.root.join(".update").join("bbbbbbbb")),
            "the new update made no folder of its own"
        );
        // Nothing touched: the earlier update's folder, journal and old files are as they were -
        // it was not put back, which a swap never does to a release somebody may be running ...
        assert_eq!(difference(&install_before, &whole_install(s)), "");
        assert_eq!(
            journal_state_of(&s.root),
            JournalState::CommittedPendingHealth
        );
        // ... nothing was started to find out, no RunOnce entry was registered, and what its dead
        // helper left - its copy in U, and the logon entry that runs it - is still there for
        // whoever settles it.
        assert!(second.launcher.ran().is_empty());
        assert!(second.registry.run_once_history.borrow().is_empty());
        assert_eq!(
            second.registry.run_once_cleared.get(),
            0,
            "the logon entry of an update that is not settled stays"
        );
        assert!(
            second
                .registry
                .run_once
                .borrow()
                .contains_key(RUN_ONCE_NAME)
        );
        assert!(is_present(
            &s.state
                .join(format!("run-{first}"))
                .join("katana_updater.exe")
        ));
        assert_eq!(
            difference(&stage_before, &snapshot(&s.stage)),
            "",
            "and the download is kept"
        );

        // Once the second release has said that it starts - its window writes the marker - the very
        // same update goes through.
        write_text(&health_marker_path(&s.state, &first), "ok\n");
        let expected3 = s.scratch.join("expected3");
        write_install(&expected3, &v3);
        let third = Rig::with_transaction(s, &disk, "cccccccc");
        let again = third.run();
        assert_eq!(
            again.outcome,
            ApplyOutcome::Committed,
            "{}: {}",
            again.reason,
            again.detail
        );
        assert_eq!(tree_hash(&s.root), tree_hash(&expected3));
        assert!(leftovers_in_update(&s.root).is_empty());
    }

    #[test]
    fn the_logon_entry_of_an_earlier_update_goes_with_its_helper_copy_even_when_the_next_update_ends_blocked()
     {
        // The helper that died had registered a RunOnce entry that runs ITS copy, U/run-<tx>. When
        // the next swap settles that update healthy it deletes the copy; an entry left behind would
        // name a file that is not there. A swap that goes on registers an entry of its own when its
        // journal says COMMITTING and removes it at its end - but one that ends before that
        // (Blocked, here) registers none and removes none. So the entry goes when the copy does.
        crate::require_can_sign!();
        let mut lp = Loop::new("superseded-entry", &build_tiny_loop);
        let p = measure_clean_apply(&mut lp);
        crash_at(&mut lp, p.healthy, 0);
        let s = lp.scenario();
        assert_eq!(
            journal_state_of(&s.root),
            JournalState::CommittedPendingHealth
        );
        let first = only_transaction(s);
        write_text(
            &s.state
                .join(format!("run-{first}"))
                .join("katana_updater.exe"),
            "the dead helper's copy",
        );
        assert!(
            is_present(&health_marker_path(&s.state, &first)),
            "the premise: its marker is there"
        );

        remove_all(&s.stage);
        write_stage(&s.stage, &tiny(THIRD_VERSION, 300));
        let disk = StdFs;
        let mut second = Rig::with_transaction(s, &disk, "bbbbbbbb");
        second.registry.run_once.borrow_mut().insert(
            String::from(RUN_ONCE_NAME),
            String::from("the dead helper's entry"),
        );
        // A program of the install is running and stays: the wait of step 2 ends Blocked.
        second.processes.holders = vec![RunningProgram {
            pid: 4412,
            image: path_from_utf8("C:/k/bin/katana_cli.exe"),
        }];
        second.processes.holders_for = -1;
        let result = second.run();
        assert_eq!(
            result.outcome,
            ApplyOutcome::Blocked,
            "{}: {}",
            result.reason,
            result.detail
        );
        assert_eq!(
            tree_hash(&s.root),
            s.v2_hash,
            "nothing of the third release was installed"
        );

        // The second release had said that it starts: it was settled in the swap's first step ...
        assert!(leftovers_in_update(&s.root).is_empty());
        assert!(!is_present(&health_marker_path(&s.state, &first)));
        assert!(
            !is_present(&s.state.join(format!("run-{first}"))),
            "and its dead helper's copy is gone"
        );
        // ... and the entry that named the copy went with it.
        assert!(
            second.registry.run_once.borrow().is_empty(),
            "a RunOnce entry was left that names a helper copy that is gone"
        );
        assert!(
            second.registry.run_once_history.borrow().is_empty(),
            "the blocked update registered none: it never said COMMITTING"
        );
    }

    #[test]
    fn an_update_folder_that_cannot_be_read_through_stops_the_next_update_as_an_unsettled_one_does()
    {
        // "Could not look" is not "there is none". R/.update is read when the swap looks for an
        // update that a crash cut off (there is none here), and cannot be read a moment later when
        // it looks for one that was never said to start (there is one). Taken for "nothing to
        // settle", that would plan a third release over a second that nobody has shown to start.
        crate::require_can_sign!();
        let mut lp = Loop::new("superseded-unread", &build_tiny_loop);
        let p = measure_clean_apply(&mut lp);
        crash_at(&mut lp, p.healthy, 0);
        let s = lp.scenario();
        assert_eq!(
            journal_state_of(&s.root),
            JournalState::CommittedPendingHealth
        );
        let first = only_transaction(s);
        write_text(
            &s.state
                .join(format!("run-{first}"))
                .join("katana_updater.exe"),
            "the dead helper's copy",
        );
        remove(&health_marker_path(&s.state, &first)); // nothing says the second release starts

        remove_all(&s.stage);
        write_stage(&s.stage, &tiny(THIRD_VERSION, 300));
        let install_before = whole_install(s);
        let stage_before = snapshot(&s.stage);

        let disk = StdFs;
        let unread = UnlistableFrom::new(&disk, 2); // the first look succeeds, the second does not
        let second = Rig::with_transaction(s, &unread, "bbbbbbbb");
        let result = second.run();
        assert_eq!(result.outcome, ApplyOutcome::Refused);
        assert_eq!(result.reason, "interrupted_update", "{}", result.detail);
        // The sentence of THIS refusal (an update that could not be put back has another, and so
        // has one that is known to be unsettled): the case is the one it says it is.
        assert!(
            result.detail.contains("could not be read"),
            "{}",
            result.detail
        );
        assert!(unread.asked.get() >= 2);
        assert_eq!(
            tree_hash(&s.root),
            s.v2_hash,
            "the second release is still what is installed"
        );
        assert!(
            !is_present(&s.root.join(".update").join("bbbbbbbb")),
            "the new update made no folder of its own"
        );
        assert_eq!(
            difference(&install_before, &whole_install(s)),
            "",
            "the install was changed"
        );
        assert!(second.launcher.ran().is_empty());
        assert!(second.registry.run_once_history.borrow().is_empty());
        assert_eq!(
            difference(&stage_before, &snapshot(&s.stage)),
            "",
            "and the download is kept"
        );
    }

    #[test]
    fn an_earlier_update_whose_journal_cannot_be_read_stops_the_next_update_as_an_unsettled_one_does()
     {
        // Here R/.update lists, and it is the JOURNAL of the earlier update that will not be read: a
        // scanner or a backup has it open. A folder that was dropped from the list for that was in
        // nobody's count - not "left", not "could not look" - and a third release was planned over
        // a second that nobody had shown to start; after which `--recover` could not put the
        // second back either, the tree being no longer its release. The rule (recover_internal.rs):
        // a folder that could not be read may be such an update, and the swap does not go on.
        crate::require_can_sign!();
        let mut lp = Loop::new("superseded-journal-held", &build_tiny_loop);
        let p = measure_clean_apply(&mut lp);
        crash_at(&mut lp, p.healthy, 0);
        let s = lp.scenario();
        assert_eq!(
            journal_state_of(&s.root),
            JournalState::CommittedPendingHealth
        );
        let first = only_transaction(s);
        write_text(
            &s.state
                .join(format!("run-{first}"))
                .join("katana_updater.exe"),
            "the dead helper's copy",
        );
        remove(&health_marker_path(&s.state, &first)); // nothing says the second release starts

        let v3 = tiny(THIRD_VERSION, 300);
        remove_all(&s.stage);
        write_stage(&s.stage, &v3);
        let install_before = whole_install(s);
        let stage_before = snapshot(&s.stage);

        let disk = StdFs;
        {
            let held = JournalHeld::new(&disk);
            let second = Rig::with_transaction(s, &held, "bbbbbbbb");
            let result = second.run();
            assert!(
                held.asked.get() >= 1,
                "the premise: the journal was asked for, and would not be read"
            );
            assert_eq!(result.outcome, ApplyOutcome::Refused);
            assert_eq!(result.reason, "interrupted_update", "{}", result.detail);
            // The sentence of the refusal that could not look, as when the folder will not list.
            assert!(
                result.detail.contains("could not be read"),
                "{}",
                result.detail
            );
            assert_eq!(
                tree_hash(&s.root),
                s.v2_hash,
                "the second release is still what is installed"
            );
            assert!(
                !is_present(&s.root.join(".update").join("bbbbbbbb")),
                "the new update made no folder of its own"
            );
            assert_eq!(
                difference(&install_before, &whole_install(s)),
                "",
                "the install was changed"
            );
            assert!(second.launcher.ran().is_empty());
            assert!(second.registry.run_once_history.borrow().is_empty());
            assert_eq!(second.registry.run_once_cleared.get(), 0);
            assert_eq!(
                difference(&stage_before, &snapshot(&s.stage)),
                "",
                "and the download is kept"
            );
        }

        // The scanner gone, and the second release having said that it starts (its window writes
        // the marker): the very same update goes through, over a tree that nothing had touched.
        write_text(&health_marker_path(&s.state, &first), "ok\n");
        let expected3 = s.scratch.join("expected3");
        write_install(&expected3, &v3);
        let third = Rig::with_transaction(s, &disk, "cccccccc");
        let again = third.run();
        assert_eq!(
            again.outcome,
            ApplyOutcome::Committed,
            "{}: {}",
            again.reason,
            again.detail
        );
        assert_eq!(tree_hash(&s.root), tree_hash(&expected3));
        assert!(leftovers_in_update(&s.root).is_empty());
    }
}

// ---- an update that is in place and was never said to start --------------------------------------
//
// A helper that dies between its COMMITTED_PENDING_HEALTH record and its HEALTHY one leaves a whole
// new version that nothing has settled (recover.rs, "AN UPDATE THAT IS INSTALLED AND WAS NEVER SAID
// TO START"). What follows holds the rule that settles it (recover_internal.rs, settle_pending) and
// two of its callers - settle_unproved, which is `katana_updater --recover`, and
// confirm_started_with, the window - to the specification, worked out by hand:
//
//   WHICH TREE: the new version's, built from the release in a folder of its own (s.v2_hash), or
//     the old one's, the snapshot taken before the update (s.v1_hash). Never what the code left
//     behind.
//   "NOTHING WRITTEN" is zero changes counted by a FaultFs under the code, and the install (its
//     .update included) and the per-user folder compared entry by entry, byte by byte.
//   THE ORDER of the registry against the disk is read off a registry that notes, at each call,
//     how many changes to the disk had been made by then.
//   THE RUNONCE COMMAND is written out as apply.rs's step 4 gives it.
//
// The state itself is not written by hand: the fixture update is applied on a FaultFs that dies
// before the operation a clean run's log names as the HEALTHY append.

struct Unproved {
    lp: Loop,
    points: Points,
    transaction: String,
    marker: PathBuf,
    helper: PathBuf,
}

impl Unproved {
    fn new(label: &str) -> Self {
        let mut lp = Loop::new(label, &build_fixture_scenario);
        let points = measure_clean_apply(&mut lp);
        let mut u = Self {
            lp,
            points,
            transaction: String::new(),
            marker: PathBuf::new(),
            helper: PathBuf::new(),
        };
        u.again();
        u
    }

    // That state once more, from the pristine install (a Loop puts back the install and the stage,
    // and empties the per-user folder).
    fn again(&mut self) {
        crash_at(&mut self.lp, self.points.healthy, 0);
        let s = self.lp.scenario();
        self.transaction = only_transaction(s);
        self.marker = health_marker_path(&s.state, &self.transaction);
        self.helper = s
            .state
            .join(format!("run-{}", self.transaction))
            .join("katana_updater.exe");
        // What the dead helper ran from. It is only ever looked for and deleted here, never
        // started: a test starts no file that is not a program.
        write_text(&self.helper, "the dead helper's copy");
    }

    // What every case starts from; one that did not would prove nothing.
    fn expect_premise(&mut self) {
        let s = self.lp.scenario();
        assert_eq!(
            journal_state_of(&s.root),
            JournalState::CommittedPendingHealth
        );
        assert_eq!(
            tree_hash(&s.root),
            s.v2_hash,
            "every file is the new version's"
        );
        assert!(
            is_present(&self.marker),
            "the health run wrote its marker before the helper died"
        );
        assert_eq!(leftovers_in_update(&s.root), vec![self.transaction.clone()]);
    }

    fn work(&mut self) -> PathBuf {
        self.lp
            .scenario()
            .root
            .join(".update")
            .join(&self.transaction)
    }
}

// A path's text with the system's separators (std::filesystem's make_preferred).
fn preferred(path: &Path) -> String {
    let shown = text(path);
    #[cfg(windows)]
    {
        shown.replace('/', "\\")
    }
    #[cfg(not(windows))]
    {
        shown
    }
}

// `"<helper>" --recover <R>`: the helper in quotes, and the install folder quoted when it needs it,
// both with the separators of the system - it is the shell that reads the command at the next logon
// (apply.rs, step 4; TheRunOnceEntryIsRegisteredBeforeTheSwap... in test_apply.cpp holds the swap to
// the same text).
fn command(helper: &Path, root: &Path) -> String {
    format!(
        "\"{}\" --recover {}",
        preferred(helper),
        quote_argument(&preferred(root))
    )
}

// settle_unproved as `katana_updater --recover` calls it: the helper is the copy the swap ran from,
// and the marker is looked for in the per-user folder the swap used.
fn settle(s: &Scenario, helper: &Path, services: &ApplyServices<'_>) -> Settled {
    settle_unproved(&s.root, helper, &s.state, services)
}

// The same, on `fs`, until it ends or "dies".
fn settle_until_crash(
    s: &Scenario,
    helper: &Path,
    fs: &dyn FsOps,
    settled: Option<&mut Settled>,
) -> Option<SimulatedCrash> {
    let rig = Rig::with_transaction(s, fs, "eeeeeeee");
    match crash_of(|| settle(s, helper, &rig.services())) {
        Ok(ended) => {
            if let Some(settled) = settled {
                *settled = ended;
            }
            None
        }
        Err(crash) => Some(crash),
    }
}

// What a machine does after a program died while it was settling: the next
// `katana_updater --recover`, which is a recovery and then a settlement (the helper program's
// runRecover), both on a file system that works.
fn recover_then_settle(s: &Scenario, helper: &Path) {
    assert!(recover_clean(s).is_ok());
    let clean = StdFs;
    assert!(settle_until_crash(s, helper, &clean, None).is_none());
}

// One call of a TimedRegistry.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Call {
    what: String,
    name: String,
    value: String,
    changes_before: usize,
}

// A registry that also writes down, for each call, how many changes to the disk had been made by
// then (the length of a LogFs's log): the order of the registry against the file system.
struct TimedRegistry<'a> {
    log: &'a LogFs<'a>,
    location: Option<PathBuf>,
    calls: RefCell<Vec<Call>>,
}

impl<'a> TimedRegistry<'a> {
    fn new(log: &'a LogFs<'a>) -> Self {
        Self {
            log,
            location: None,
            calls: RefCell::new(Vec::new()),
        }
    }

    fn note(&self, what: &str, name: &str, value: &str) {
        let changes_before = self.log.log.borrow().len();
        self.calls.borrow_mut().push(Call {
            what: String::from(what),
            name: String::from(name),
            value: String::from(value),
            changes_before,
        });
    }
}

impl RegistryOps for TimedRegistry<'_> {
    fn installer_location(&self) -> Result<Option<PathBuf>> {
        Ok(self.location.clone())
    }

    fn set_run_once(&self, name: &str, command: &str) -> Status {
        self.note("setRunOnce", name, command);
        Ok(())
    }

    fn clear_run_once(&self, name: &str) -> Status {
        self.note("clearRunOnce", name, "");
        Ok(())
    }

    fn set_installed_version(&self, display_version: &str, display_name: &str) -> Status {
        self.note("setInstalledVersion", display_name, display_version);
        Ok(())
    }
}

// The path of a LogFs entry that is `<operation> <path>` and nothing more.
fn path_of(entry: &str, operation: &str) -> PathBuf {
    path_from_utf8(&entry.as_bytes()[operation.len() + 1..])
}

// The RunOnce value as the registry double holds it when the logon net is in place.
fn armed(helper: &Path, root: &Path) -> BTreeMap<String, String> {
    BTreeMap::from([(String::from(RUN_ONCE_NAME), command(helper, root))])
}

// A file system on which a file a scanner reads cannot be deleted: katana.exe in the transaction's
// folder.
fn holding_katana_exe(disk: &dyn FsOps) -> ForwardingFs<'_> {
    let holding = ForwardingFs::new(disk);
    holding.on_remove_file(|inner, path| {
        if path.file_name() == Some(OsStr::new("katana.exe")) && text(path).contains(".update") {
            Err(os_error(
                ErrorCode::FileExportFailure,
                "sharing violation",
                path,
                32,
            ))
        } else {
            inner.remove_file(path)
        }
    });
    holding
}

mod settle_unproved {
    use super::*;

    #[test]
    fn an_update_whose_marker_is_there_is_settled_healthy_and_the_per_user_folder_is_only_read() {
        let mut u = Unproved::new("settle-healthy");
        u.expect_premise();
        let work = u.work();
        let journal = work.join("journal");
        let s = u.lp.scenario();
        let user_before = snapshot(&s.state);
        // The one record a settlement appends: HEALTHY, numbered after the records that are there.
        let healthy_record = encode_record(
            parse_journal(read_text(&journal)).records.len() as u64 + 1,
            &record(JournalOp::Healthy, &[]),
        );

        let disk = StdFs;
        let log = LogFs::new(&disk);
        let rig = Rig::with_transaction(s, &log, "eeeeeeee");
        assert_eq!(settle(s, &u.helper, &rig.services()), Settled::Healthy);

        assert_eq!(
            tree_hash(&s.root),
            s.v2_hash,
            "{}",
            difference(&snapshot(&s.expected), &snapshot(&s.root))
        );
        assert!(
            leftovers_in_update(&s.root).is_empty(),
            "its folder is gone"
        );
        // `--recover` is started by headless programs too: U is read, for the marker, and that is
        // all. The marker itself and the dead helper's copy are still there.
        assert_eq!(
            difference(&user_before, &snapshot(&s.state)),
            "",
            "the per-user folder was changed"
        );
        assert!(is_present(&u.marker));
        // The logon net: put back before anything else, and taken away once no folder is left.
        assert_eq!(
            *rig.registry.run_once_history.borrow(),
            vec![command(&u.helper, &s.root)]
        );
        assert!(rig.registry.run_once.borrow().is_empty());
        assert_eq!(rig.registry.run_once_cleared.get(), 1);
        assert!(
            rig.registry.versions.borrow().is_empty(),
            "an archive install has no entry in Settings > Apps"
        );
        assert!(rig.launcher.ran().is_empty(), "no settler starts a program");
        assert!(rig.launcher.started().is_empty());

        // On the disk: one flushed record first, then the folder, its journal the last file to go.
        {
            let entries = log.log.borrow();
            assert!(!entries.is_empty());
            let first = &entries[0];
            assert!(
                first.starts_with("appendDurable ")
                    && first.ends_with(&format!(" {healthy_record}")),
                "{first}"
            );
            assert_eq!(
                path_from_utf8(&first.as_bytes()[14..first.len() - 1 - healthy_record.len()]),
                journal
            );
            let removals = log.all("removeFile");
            let last_removal = *removals.last().expect("a file was removed");
            assert_eq!(
                path_of(&entries[(last_removal - 1) as usize], "removeFile"),
                journal,
                "a crash part way leaves a folder that still says what it is"
            );
            assert_eq!(
                path_of(entries.last().expect("an entry"), "removeEmptyDirectory"),
                work
            );
        }

        // Again: there is nothing to settle, so nothing is asked for and nothing written.
        let watched = WatchedFs::new(&disk);
        let counting = FaultFs::new(&watched);
        let again = Rig::with_transaction(s, &counting, "eeeeeeee");
        assert_eq!(settle(s, &u.helper, &again.services()), Settled::Nothing);
        assert_eq!(counting.mutating_operations(), 0);
        assert_eq!(watched.locks.get(), 0, "not even the lock");
        assert!(again.registry.run_once_history.borrow().is_empty());
        assert_eq!(again.registry.run_once_cleared.get(), 0);
        assert_eq!(tree_hash(&s.root), s.v2_hash);
    }

    #[test]
    fn the_logon_net_is_put_back_first_and_the_version_is_rewritten_before_the_record_is_written() {
        let mut u = Unproved::new("settle-order");
        u.expect_premise();
        let s = u.lp.scenario();
        let disk = StdFs;
        let log = LogFs::new(&disk);
        let rig = Rig::with_transaction(s, &log, "eeeeeeee");
        let mut registry = TimedRegistry::new(&log);
        // HKCU Software\Jarada\Katana equals R: an installer's install.
        registry.location = Some(s.root.clone());
        let services = ApplyServices {
            fs: &log,
            processes: &rig.processes,
            launcher: &rig.launcher,
            registry: &registry,
            trust: test_trust(),
            timings: rig.timings,
        };
        assert_eq!(settle(s, &u.helper, &services), Settled::Healthy);
        {
            let entries = log.log.borrow();
            assert!(!entries.is_empty());
            assert!(entries[0].contains(" HEALTHY "), "{}", entries[0]);
        }

        let calls = registry.calls.borrow();
        assert_eq!(calls.len(), 3);
        // FIRST the RunOnce entry, with nothing on the disk changed yet: when Windows started this
        // run from the entry it had already taken the entry away.
        assert_eq!(calls[0].what, "setRunOnce");
        assert_eq!(calls[0].name, RUN_ONCE_NAME);
        assert_eq!(calls[0].value, command(&u.helper, &s.root));
        assert_eq!(calls[0].changes_before, 0);
        // Then the version Settings > Apps shows - the fixtures' v2 is 0.10.0 - before HEALTHY is
        // written (the first change to the disk, above): the swap itself rewrites it only once the
        // proof is there, and it may have died before it did.
        assert_eq!(calls[1].what, "setInstalledVersion");
        assert_eq!(calls[1].value, "0.10.0");
        assert_eq!(calls[1].name, "Katana 0.10.0");
        assert_eq!(calls[1].changes_before, 0);
        // And the entry is taken away last, when the folder is gone.
        assert_eq!(calls[2].what, "clearRunOnce");
        assert_eq!(calls[2].name, RUN_ONCE_NAME);
        assert_eq!(calls[2].changes_before, log.log.borrow().len());
        assert!(leftovers_in_update(&s.root).is_empty());
    }

    #[test]
    fn an_update_with_no_marker_and_nobody_running_is_put_back_and_nothing_per_user_says_so() {
        let mut u = Unproved::new("settle-rollback");
        u.expect_premise();
        remove(&u.marker); // the new version never said that it starts
        let s = u.lp.scenario();
        // The tree's seed is the person's (v1's), not the one the release lists: a seed is written
        // only when absent. The tree is that release all the same - or nothing here would be put
        // back.
        let seed = read_text(&s.root.join("etc/katana/settings.ini"));
        assert_eq!(
            seed,
            read_text(&fixture_folder().join("install/v1/etc/katana/settings.ini"))
        );
        assert_ne!(
            seed,
            read_text(&fixture_folder().join("install/v2/etc/katana/settings.ini"))
        );
        let user_before = snapshot(&s.state);

        let disk = StdFs;
        let log = LogFs::new(&disk);
        let rig = Rig::with_transaction(s, &log, "eeeeeeee");
        assert_eq!(settle(s, &u.helper, &rig.services()), Settled::RolledBack);

        assert_eq!(
            tree_hash(&s.root),
            s.v1_hash,
            "{}",
            difference(&s.v1_snapshot, &snapshot(&s.root))
        );
        assert!(
            leftovers_in_update(&s.root).is_empty(),
            "its folder is gone"
        );
        assert_eq!(
            rig.processes.scans.load(Ordering::SeqCst),
            2,
            "asked before the files were hashed, and again as the last thing"
        );
        // The rollback is the one every interrupted update gets: its two records, the first of
        // them before anything moves.
        assert_eq!(log.find("appendDurable", "ROLLING_BACK", 1), 1);
        assert!(log.find("appendDurable", "ROLLED_BACK", 1) > 1);
        assert_eq!(
            *rig.registry.run_once_history.borrow(),
            vec![command(&u.helper, &s.root)]
        );
        assert!(rig.registry.run_once.borrow().is_empty());
        assert_eq!(rig.registry.run_once_cleared.get(), 1);
        assert!(rig.registry.versions.borrow().is_empty());
        assert!(
            rig.launcher.ran().is_empty(),
            "the old version is not started either: no settler starts a program"
        );
        assert!(rig.launcher.started().is_empty());
        // What such a run cannot record is not recorded: no last.json, and the release is not
        // added to `blocked` (no state.json is made). U is as it was.
        assert_eq!(
            difference(&user_before, &snapshot(&s.state)),
            "",
            "the per-user folder was changed"
        );
        assert!(!is_present(&s.state.join("last.json")));
        assert!(!is_present(&s.state.join("state.json")));

        let counting = FaultFs::new(&disk);
        let again = Rig::with_transaction(s, &counting, "eeeeeeee");
        assert_eq!(settle(s, &u.helper, &again.services()), Settled::Nothing);
        assert_eq!(counting.mutating_operations(), 0);
        assert_eq!(tree_hash(&s.root), s.v1_hash);
    }

    // A manifest that names the keys of no release: nothing verifies against it.
    static NO_KEYS: Trust = Trust {
        keys: Vec::new(),
        placeholder: false,
    };

    struct Row {
        name: &'static str,
        arrange: Option<fn(&mut Unproved)>, // on top of the marker being gone
        answers: Vec<Answer>,
        trust: &'static Trust, // the keys the release is signed with, unless a row says otherwise
        evidence: bool,        // false: no per-user folder is named at all
    }

    fn row_of(name: &'static str, arrange: Option<fn(&mut Unproved)>, answers: Vec<Answer>) -> Row {
        Row {
            name,
            arrange,
            answers,
            trust: test_trust(),
            evidence: true,
        }
    }

    fn overwrite_one_file_of_the_release(u: &mut Unproved) {
        // Deleted and written anew, never written in place: a Loop's install shares the bytes of
        // the files an update keeps with its pristine copy (replace_tree).
        let s = u.lp.scenario();
        remove(&s.root.join("share/katana/readme.txt"));
        write_text(&s.root.join("share/katana/readme.txt"), "changed by hand\n");
    }

    fn remove_one_file_of_the_release(u: &mut Unproved) {
        remove(&u.lp.scenario().root.join("bin/libnew-2.dll"));
    }

    fn name_another_release_in_the_plan(u: &mut Unproved) {
        // The same journal with another `toSequence` (the fixtures' v2 is 1792312800), written
        // record by record so that every number and check is right.
        let journal = u.work().join("journal");
        let mut parsed = parse_journal(read_text(&journal));
        assert!(!parsed.records.is_empty());
        assert_eq!(parsed.records[0].args.len(), 3);
        assert_eq!(parsed.records[0].args[2], b"1792312800");
        parsed.records[0].args[2] = b"999".to_vec();
        write_text(&journal, encode_all(&parsed.records));
    }

    fn put_a_folder_under_the_markers_name(u: &mut Unproved) {
        make_directories(&u.marker);
    }

    fn remove_one_of_the_old_files(u: &mut Unproved) {
        // The first file the journal says was replaced: the commit moved its old bytes to
        // <tx>/old/<path> (journal.rs, REPLACE).
        let work = u.work();
        let parsed = parse_journal(read_text(&work.join("journal")));
        let replaced = parsed
            .records
            .iter()
            .find(|one| one.op == JournalOp::Replace)
            .expect("a REPLACE record");
        assert_eq!(replaced.args.len(), 1);
        let kept = work
            .join("old")
            .join(path_from_utf8(replaced.args[0].as_slice()));
        assert!(is_present(&kept), "the premise: the commit moved it there");
        remove(&kept);
    }

    fn forge_a_record_of_the_journal(u: &mut Unproved) {
        // The same journal with the path of its first REPLACE record forged, every number and
        // check right. What <tx>/old holds is as it was.
        let journal = u.work().join("journal");
        let mut parsed = parse_journal(read_text(&journal));
        let replaced = parsed
            .records
            .iter_mut()
            .find(|one| one.op == JournalOp::Replace)
            .expect("a REPLACE record");
        replaced.args = vec![b"../outside.txt".to_vec()];
        write_text(&journal, encode_all(&parsed.records));
    }

    fn remove_the_helper_copy(u: &mut Unproved) {
        let run = u.lp.scenario().state.join(format!("run-{}", u.transaction));
        remove_all(&run);
    }

    #[test]
    fn nothing_is_put_back_under_a_running_program_or_over_files_that_are_no_longer_that_release() {
        let mut u = Unproved::new("settle-left");
        let cli = somebody(4412, "C:/k/bin/katana_cli.exe");
        let rows = [
            row_of(
                "a program of the install is running at the first scan",
                None,
                vec![cli.clone()],
            ),
            row_of(
                "one was started while the files were hashed: at the second scan only",
                None,
                vec![nobody(), cli],
            ),
            row_of(
                "the first scan fails, which is not 'nobody'",
                None,
                vec![no_answer()],
            ),
            row_of("the second scan fails", None, vec![nobody(), no_answer()]),
            row_of(
                "one file of the release was overwritten",
                Some(overwrite_one_file_of_the_release),
                Vec::new(),
            ),
            row_of(
                "one file of the release is gone",
                Some(remove_one_file_of_the_release),
                Vec::new(),
            ),
            row_of(
                "the journal's PLAN names another release than the manifest in the tree",
                Some(name_another_release_in_the_plan),
                Vec::new(),
            ),
            Row {
                trust: &NO_KEYS,
                ..row_of(
                    "the manifest in the tree is signed by a key this helper does not trust",
                    None,
                    Vec::new(),
                )
            },
            row_of(
                "something that is not a file has the marker's name",
                Some(put_a_folder_under_the_markers_name),
                Vec::new(),
            ),
            Row {
                evidence: false,
                ..row_of(
                    "no per-user folder is named, so there is nowhere to look for a marker",
                    None,
                    Vec::new(),
                )
            },
            // A rollback puts back what <tx>/old holds. With one old file gone it would leave the
            // new version of that file among the old ones: a mix, and no "put back".
            row_of(
                "one of the old files the update moved aside is gone from its folder",
                Some(remove_one_of_the_old_files),
                Vec::new(),
            ),
            row_of(
                "a record of the journal names a place outside the install, so the journal is \
                 not believed",
                Some(forge_a_record_of_the_journal),
                Vec::new(),
            ),
            // The marker is written where the NEW VERSION looks for its per-user folder. A folder
            // that does not hold the copy the update's helper ran from is not that folder
            // (KATANA_UPDATE_STATE_DIR set for Katana and not at the logon; another account), and a
            // marker missing from it was not shown never to have been written.
            row_of(
                "the per-user folder is not the one the update was applied from: its helper copy \
                 is not in it",
                Some(remove_the_helper_copy),
                Vec::new(),
            ),
        ];
        for row in &rows {
            let name = row.name;
            u.again();
            u.expect_premise();
            remove(&u.marker);
            if let Some(arrange) = row.arrange {
                arrange(&mut u);
            }
            let s = u.lp.scenario();
            assert_eq!(
                journal_state_of(&s.root),
                JournalState::CommittedPendingHealth,
                "{name}"
            );
            let install_before = whole_install(s);
            let user_before = snapshot(&s.state);

            let disk = StdFs;
            let counting = FaultFs::new(&disk);
            let rig = Rig::with_transaction(s, &counting, "eeeeeeee");
            let processes = ScriptedProcesses::new(row.answers.clone());
            let services = ApplyServices {
                fs: &counting,
                processes: &processes,
                launcher: &rig.launcher,
                registry: &rig.registry,
                trust: row.trust,
                timings: rig.timings,
            };
            let evidence = if row.evidence {
                s.state.clone()
            } else {
                PathBuf::new()
            };
            assert_eq!(
                settle_unproved(&s.root, &u.helper, &evidence, &services),
                Settled::Left,
                "{name}"
            );

            assert_eq!(
                counting.mutating_operations(),
                0,
                "{name}: nothing is written for an update that is left"
            );
            assert_eq!(
                difference(&install_before, &whole_install(s)),
                "",
                "{name}: the install was changed"
            );
            assert_eq!(
                journal_state_of(&s.root),
                JournalState::CommittedPendingHealth,
                "{name}"
            );
            assert_eq!(
                difference(&user_before, &snapshot(&s.state)),
                "",
                "{name}: the per-user folder was changed"
            );
            // The logon net is in place, and stays: the next logon comes back to decide.
            assert_eq!(
                *rig.registry.run_once.borrow(),
                armed(&u.helper, &s.root),
                "{name}"
            );
            assert_eq!(rig.registry.run_once_cleared.get(), 0, "{name}");
            assert!(rig.launcher.ran().is_empty(), "{name}");
            assert!(rig.launcher.started().is_empty(), "{name}");
        }
    }

    #[test]
    fn a_held_lock_is_a_live_helper_and_nothing_is_written_not_even_the_run_once_entry() {
        let mut u = Unproved::new("settle-busy");
        u.expect_premise();
        // With no marker and nobody running this update would be put back - were the lock free.
        // It is held: its helper is alive, and in the middle of the very health run that would
        // write it.
        remove(&u.marker);
        let s = u.lp.scenario();
        let install_before = whole_install(s);
        let user_before = snapshot(&s.state);

        let disk = StdFs;
        let mut busy = WatchedFs::new(&disk);
        busy.held_for = -1;
        let counting = FaultFs::new(&busy);
        let mut rig = Rig::with_transaction(s, &counting, "eeeeeeee");
        // How long it goes on asking decides nothing here: the lock is never let go, so the answer
        // is Busy however many attempts fit into the wait. (That the lock is asked for more than
        // once is the next case's, which counts the attempts and does not time them.)
        rig.timings.wait_for_holders = Duration::from_millis(300);
        assert_eq!(settle(s, &u.helper, &rig.services()), Settled::Busy);

        assert!(busy.locks.get() >= 1, "the lock was asked for");
        assert_eq!(counting.mutating_operations(), 0);
        assert_eq!(
            difference(&install_before, &whole_install(s)),
            "",
            "the install was changed"
        );
        assert_eq!(
            difference(&user_before, &snapshot(&s.state)),
            "",
            "the per-user folder was changed"
        );
        assert!(
            rig.registry.run_once_history.borrow().is_empty(),
            "the registry too is the live helper's"
        );
        assert_eq!(rig.registry.run_once_cleared.get(), 0);
        assert_eq!(
            rig.processes.scans.load(Ordering::SeqCst),
            0,
            "nobody is even asked who is running"
        );
    }

    #[test]
    fn a_lock_that_is_held_only_for_a_moment_is_waited_for_and_the_update_is_then_settled() {
        let mut u = Unproved::new("settle-lock-blip");
        u.expect_premise();
        let s = u.lp.scenario();
        let disk = StdFs;
        let mut busy = WatchedFs::new(&disk);
        busy.held_for = 3; // a scanner that has the lock file open for a moment
        let mut rig = Rig::with_transaction(s, &busy, "eeeeeeee");
        // The whole wait a recovery ever gives a holder (it is capped at RECOVER_WAIT whatever the
        // timings say). It ends at the fourth attempt, three polls of a millisecond in: the figure
        // bounds a machine that stands still, not a run that passes.
        rig.timings.wait_for_holders = RECOVER_WAIT;
        assert_eq!(settle(s, &u.helper, &rig.services()), Settled::Healthy);
        assert_eq!(
            busy.locks.get(),
            4,
            "three refusals are not believed: the lock is polled, as a recovery polls it"
        );
        assert_eq!(tree_hash(&s.root), s.v2_hash);
        assert!(leftovers_in_update(&s.root).is_empty());
    }

    #[test]
    fn a_putting_back_that_cannot_finish_says_so_and_the_next_recovery_finishes_it() {
        let mut u = Unproved::new("settle-incomplete");
        u.expect_premise();
        remove(&u.marker);
        let s = u.lp.scenario();

        let disk = StdFs;
        let hooked = HookFs::new(&disk);
        hooked.set_before_rename(|from, _| Some(injected(32, from))); // a file that stays locked
        let rig = Rig::with_transaction(s, &hooked, "eeeeeeee");
        assert_eq!(
            settle(s, &u.helper, &rig.services()),
            Settled::RollbackIncomplete
        );
        // The journal says what every later start finishes (journal.rs, needs_recovery), the
        // folder with the old files is still there, and the logon net stays.
        assert_eq!(journal_state_of(&s.root), JournalState::RollingBack);
        assert_eq!(leftovers_in_update(&s.root), vec![u.transaction.clone()]);
        assert_eq!(*rig.registry.run_once.borrow(), armed(&u.helper, &s.root));
        assert_eq!(rig.registry.run_once_cleared.get(), 0);

        // The next start of anything, with the file free: an ordinary recovery, which needs no
        // marker and asks nobody who is running.
        assert!(recover_clean(s).is_ok());
        assert_eq!(
            tree_hash(&s.root),
            s.v1_hash,
            "{}",
            difference(&s.v1_snapshot, &snapshot(&s.root))
        );
        assert!(leftovers_in_update(&s.root).is_empty());
    }

    #[test]
    fn the_process_that_settles_is_not_a_program_it_gives_way_to() {
        // `katana_updater --recover` may be the install's own bin/katana_updater: recover() falls
        // back to it when the copy in U is gone, and it is what a person types. The scan then names
        // this very process, at both of its questions, and nobody else: that is "nobody is
        // running".
        let mut u = Unproved::new("settle-self");
        u.expect_premise();
        remove(&u.marker);
        let s = u.lp.scenario();
        let disk = StdFs;
        let mut rig = Rig::with_transaction(s, &disk, "eeeeeeee");
        rig.processes.pid = 999;
        rig.processes.holders = vec![RunningProgram {
            pid: 999,
            image: path_from_utf8("C:/k/bin/katana_updater.exe"),
        }];
        rig.processes.holders_for = -1; // whenever it is asked
        assert_eq!(settle(s, &u.helper, &rig.services()), Settled::RolledBack);
        assert_eq!(rig.processes.scans.load(Ordering::SeqCst), 2);
        assert_eq!(
            tree_hash(&s.root),
            s.v1_hash,
            "{}",
            difference(&s.v1_snapshot, &snapshot(&s.root))
        );
        assert!(leftovers_in_update(&s.root).is_empty());
    }

    #[test]
    fn a_folder_that_would_not_go_keeps_its_journal_and_the_logon_entry_and_nothing_is_queued_per_user()
     {
        let mut u = Unproved::new("settle-held-file");
        u.expect_premise();
        let s = u.lp.scenario();
        let user_before = snapshot(&s.state);

        let disk = StdFs;
        // A scanner is reading one of the old files in the transaction's folder: it cannot be
        // deleted.
        let holding = holding_katana_exe(&disk);
        let rig = Rig::with_transaction(s, &holding, "eeeeeeee");
        assert_eq!(
            settle(s, &u.helper, &rig.services()),
            Settled::Healthy,
            "the update is settled: a folder that will not go is not an update that is not"
        );
        assert_eq!(tree_hash(&s.root), s.v2_hash);
        // The journal is the last thing to go, and something stayed: the folder still says what
        // it is.
        assert_eq!(leftovers_in_update(&s.root), vec![u.transaction.clone()]);
        assert_eq!(journal_state_of(&s.root), JournalState::Healthy);
        // A transaction folder is left, so the logon entry is: that logon sweeps it. And what would
        // not go is NOT queued in U/cleanup.list - this run only reads the per-user folder.
        assert_eq!(*rig.registry.run_once.borrow(), armed(&u.helper, &s.root));
        assert_eq!(rig.registry.run_once_cleared.get(), 0);
        assert_eq!(
            difference(&user_before, &snapshot(&s.state)),
            "",
            "the per-user folder was changed"
        );
        assert!(!is_present(&s.state.join("cleanup.list")));

        // The next start of anything, with the file free, sweeps what is left.
        assert!(recover_clean(s).is_ok());
        assert!(leftovers_in_update(&s.root).is_empty());
        assert_eq!(tree_hash(&s.root), s.v2_hash);
    }

    #[test]
    fn with_no_such_update_it_asks_for_no_lock_and_writes_nothing() {
        {
            // An install that never updated.
            let mut s = Scenario::new("settle-nothing");
            s.v1 = tiny("0.1.0", 1);
            write_install(&s.root, &s.v1);
            let disk = StdFs;
            let watched = WatchedFs::new(&disk);
            let counting = FaultFs::new(&watched);
            let rig = Rig::with_transaction(&s, &counting, "eeeeeeee");
            assert_eq!(
                settle_unproved(&s.root, &rig.request.helper, &s.state, &rig.services()),
                Settled::Nothing
            );
            assert_eq!(counting.mutating_operations(), 0);
            assert_eq!(watched.locks.get(), 0);
            assert!(rig.registry.run_once_history.borrow().is_empty());
            assert_eq!(rig.registry.run_once_cleared.get(), 0);
            assert!(
                !is_present(&s.root.join(".update")),
                "it does not make the folder"
            );
            assert_eq!(
                settle_unproved(
                    Path::new(""),
                    &rig.request.helper,
                    &s.state,
                    &rig.services()
                ),
                Settled::Nothing,
                "no install folder is not the .update of the current folder"
            );
            assert_eq!(watched.locks.get(), 0);
        }
        {
            // An update that a crash cut off in the middle of its commit is recovery's to put back,
            // not this function's to judge: it is as it was.
            let mut lp = Loop::new("settle-not-mine", &build_fixture_scenario);
            let p = measure_clean_apply(&mut lp);
            crash_at(&mut lp, p.renames[p.renames.len() / 2], 0);
            let s = lp.scenario();
            assert_eq!(journal_state_of(&s.root), JournalState::Committing);
            let install_before = whole_install(s);
            let disk = StdFs;
            let watched = WatchedFs::new(&disk);
            let counting = FaultFs::new(&watched);
            let rig = Rig::with_transaction(s, &counting, "eeeeeeee");
            assert_eq!(
                settle_unproved(&s.root, &rig.request.helper, &s.state, &rig.services()),
                Settled::Nothing
            );
            assert_eq!(counting.mutating_operations(), 0);
            assert_eq!(watched.locks.get(), 0);
            assert!(rig.registry.run_once_history.borrow().is_empty());
            assert_eq!(difference(&install_before, &whole_install(s)), "");
        }
    }

    #[test]
    fn a_marker_missing_from_another_per_user_folder_is_not_proof_that_the_release_never_started() {
        // The release said that it starts: its marker is in the per-user folder the update was
        // applied from. `katana_updater --recover` looks where ITS environment points, and at a
        // logon or under another account that can be another folder (KATANA_UPDATE_STATE_DIR set
        // where Katana is started, and not there). Nothing is in that folder, so no marker is;
        // nobody is running; the tree is the release. A release that proved itself must not be put
        // back for that: it is left, and settled by the run that looks in the right folder.
        let mut u = Unproved::new("settle-other-folder");
        u.expect_premise();
        let s = u.lp.scenario();
        let elsewhere = s.scratch.join("elsewhere");
        make_directories(&elsewhere);
        let install_before = whole_install(s);
        let user_before = snapshot(&s.state);

        let disk = StdFs;
        {
            let counting = FaultFs::new(&disk);
            let rig = Rig::with_transaction(s, &counting, "eeeeeeee");
            assert_eq!(
                settle_unproved(&s.root, &u.helper, &elsewhere, &rig.services()),
                Settled::Left
            );
            assert_eq!(counting.mutating_operations(), 0);
            assert_eq!(
                tree_hash(&s.root),
                s.v2_hash,
                "a release that had proved itself was put back: {}",
                difference(&snapshot(&s.expected), &snapshot(&s.root))
            );
            assert_eq!(
                difference(&install_before, &whole_install(s)),
                "",
                "the install was changed"
            );
            assert_eq!(
                journal_state_of(&s.root),
                JournalState::CommittedPendingHealth
            );
            assert!(
                snapshot(&elsewhere).is_empty(),
                "nothing is made in the folder it looked in"
            );
            assert_eq!(
                difference(&user_before, &snapshot(&s.state)),
                "",
                "the per-user folder was changed"
            );
            assert_eq!(
                *rig.registry.run_once.borrow(),
                armed(&u.helper, &s.root),
                "the next logon comes back to it"
            );
            assert_eq!(rig.registry.run_once_cleared.get(), 0);
            assert_eq!(
                rig.processes.scans.load(Ordering::SeqCst),
                0,
                "nobody is asked who is running: nothing was going to be moved"
            );
        }
        // The same run, looking in the folder the update used, finds the marker.
        let rig = Rig::with_transaction(s, &disk, "eeeeeeee");
        assert_eq!(settle(s, &u.helper, &rig.services()), Settled::Healthy);
        assert_eq!(tree_hash(&s.root), s.v2_hash);
        assert!(leftovers_in_update(&s.root).is_empty());
    }

    #[test]
    fn a_putting_back_whose_first_record_cannot_be_written_moves_nothing_and_the_next_run_decides_again()
     {
        // COMMITTED_PENDING_HEALTH is the one state that tells every program's recovery "the tree
        // is a whole version: leave it" (journal.rs, needs_recovery). So a rollback out of it has
        // its ROLLING_BACK record on disk before the first file moves - a tree half put back under
        // a journal that still says it is whole would be finished by nobody. Here the journal
        // cannot be written (a full disk): nothing moves, and the journal goes on saying what is
        // true.
        let mut u = Unproved::new("settle-no-record");
        u.expect_premise();
        remove(&u.marker); // never said to start, nobody running: it would be put back
        let s = u.lp.scenario();
        let install_before = whole_install(s);
        let user_before = snapshot(&s.state);

        let disk = StdFs;
        {
            let counting = FaultFs::new(&disk);
            let full = HookFs::new(&counting);
            full.set_before_append(|path, _| {
                if path.file_name() == Some(OsStr::new("journal")) {
                    Some(injected(112, path)) // ERROR_DISK_FULL
                } else {
                    None
                }
            });
            let rig = Rig::with_transaction(s, &full, "eeeeeeee");
            assert_eq!(
                settle(s, &u.helper, &rig.services()),
                Settled::RollbackIncomplete,
                "it is not put back, and it is not called put back"
            );
            assert_eq!(
                counting.mutating_operations(),
                0,
                "not a file was moved without the record"
            );
            assert_eq!(
                tree_hash(&s.root),
                s.v2_hash,
                "{}",
                difference(&snapshot(&s.expected), &snapshot(&s.root))
            );
            assert_eq!(
                difference(&install_before, &whole_install(s)),
                "",
                "the install was changed"
            );
            assert_eq!(
                journal_state_of(&s.root),
                JournalState::CommittedPendingHealth,
                "the journal says of the tree what is true of it"
            );
            assert_eq!(
                difference(&user_before, &snapshot(&s.state)),
                "",
                "the per-user folder was changed"
            );
            assert_eq!(
                *rig.registry.run_once.borrow(),
                armed(&u.helper, &s.root),
                "the next logon comes back to it"
            );
            assert_eq!(rig.registry.run_once_cleared.get(), 0);
        }

        // That logon, with room on the disk: decided again from the same evidence, and put back.
        let rig = Rig::with_transaction(s, &disk, "eeeeeeee");
        assert_eq!(settle(s, &u.helper, &rig.services()), Settled::RolledBack);
        assert_eq!(
            tree_hash(&s.root),
            s.v1_hash,
            "{}",
            difference(&s.v1_snapshot, &snapshot(&s.root))
        );
        assert!(leftovers_in_update(&s.root).is_empty());
    }

    #[test]
    fn a_lock_that_cannot_be_opened_leaves_the_update_as_it_is_and_puts_the_logon_entry_back() {
        // Not a lock somebody HOLDS - that is a helper that is alive, and Busy, and its registry is
        // not this run's - but a lock file that cannot be opened at all: a scanner that answers
        // "access denied" (the file system calls only the two sharing violations "held"). Without
        // the lock nothing is judged. But Windows took the RunOnce entry away before it started
        // this run, and a run that left it away would be the last one that ever came for this
        // update.
        let mut u = Unproved::new("settle-lock-denied");
        u.expect_premise();
        remove(&u.marker); // with the lock, this update would be put back
        let s = u.lp.scenario();
        let install_before = whole_install(s);
        let user_before = snapshot(&s.state);

        let disk = StdFs;
        let denied = ForwardingFs::new(&disk);
        denied.on_try_lock(|_, path| {
            Err(os_error(
                ErrorCode::FileExportFailure,
                "the lock file cannot be opened",
                path,
                5,
            ))
        });
        let counting = FaultFs::new(&denied);
        let rig = Rig::with_transaction(s, &counting, "eeeeeeee");
        assert_eq!(settle(s, &u.helper, &rig.services()), Settled::Left);

        assert_eq!(counting.mutating_operations(), 0);
        assert_eq!(
            difference(&install_before, &whole_install(s)),
            "",
            "the install was changed"
        );
        assert_eq!(
            journal_state_of(&s.root),
            JournalState::CommittedPendingHealth
        );
        assert_eq!(
            difference(&user_before, &snapshot(&s.state)),
            "",
            "the per-user folder was changed"
        );
        assert_eq!(
            rig.processes.scans.load(Ordering::SeqCst),
            0,
            "nothing is judged without the lock"
        );
        assert_eq!(
            *rig.registry.run_once.borrow(),
            armed(&u.helper, &s.root),
            "no logon would come back to an update that is not settled"
        );
        assert_eq!(rig.registry.run_once_cleared.get(), 0);
    }

    #[test]
    fn a_folder_that_cannot_be_read_through_is_not_a_folder_with_nothing_to_settle() {
        let mut u = Unproved::new("settle-unread");
        u.expect_premise();
        let s = u.lp.scenario();
        let install_before = whole_install(s);

        // The rule itself: it says that it could not look, and has settled nothing.
        {
            let disk = StdFs;
            let unread = UnlistableFrom::new(&disk, 1);
            let counting = FaultFs::new(&unread);
            let rig = Rig::with_transaction(s, &counting, "eeeeeeee");
            let outcome = detail::settle_pending(&s.root, &rig.services(), "", &s.state, true);
            assert!(outcome.unread);
            assert!(outcome.healthy.is_empty());
            assert!(outcome.rolled_back.is_empty());
            assert!(
                outcome.left.is_empty(),
                "which is why `left` alone does not say that all is settled"
            );
            assert!(outcome.incomplete.is_empty());
            assert_eq!(counting.mutating_operations(), 0);
        }
        // `katana_updater --recover`: the folder could be read when it looked first and when it had
        // the lock, and not when the rule came to it. That is an update left for the next logon -
        // with the logon entry back, and kept - and not an install with nothing to settle.
        {
            let disk = StdFs;
            let unread = UnlistableFrom::new(&disk, 3);
            let counting = FaultFs::new(&unread);
            let rig = Rig::with_transaction(s, &counting, "eeeeeeee");
            assert_eq!(settle(s, &u.helper, &rig.services()), Settled::Left);
            assert!(unread.asked.get() >= 3, "the premise: a look failed");
            assert_eq!(counting.mutating_operations(), 0);
            assert_eq!(*rig.registry.run_once.borrow(), armed(&u.helper, &s.root));
            assert_eq!(rig.registry.run_once_cleared.get(), 0);
        }
        assert_eq!(
            difference(&install_before, &whole_install(s)),
            "",
            "the install was changed"
        );
        assert_eq!(
            journal_state_of(&s.root),
            JournalState::CommittedPendingHealth
        );

        // The window: nothing is settled, so it is told to ask again, and the logon entry and the
        // helper copy the entry runs both stay. Asked again with the folder readable, it is done.
        {
            let disk = StdFs;
            let unread = UnlistableFrom::new(&disk, 3);
            let registry = FakeRegistry::default();
            registry
                .run_once
                .borrow_mut()
                .insert(String::from(RUN_ONCE_NAME), command(&u.helper, &s.root));
            assert_eq!(
                detail::confirm_started_with(&s.root, &s.state, &unread, &registry),
                Confirmed::Busy
            );
            assert!(unread.asked.get() >= 3, "the premise: a look failed");
            assert_eq!(
                journal_state_of(&s.root),
                JournalState::CommittedPendingHealth
            );
            assert_eq!(*registry.run_once.borrow(), armed(&u.helper, &s.root));
            assert_eq!(registry.run_once_cleared.get(), 0);
            assert!(is_present(&u.helper));

            assert_eq!(
                detail::confirm_started_with(&s.root, &s.state, &disk, &registry),
                Confirmed::Done
            );
            assert_eq!(tree_hash(&s.root), s.v2_hash);
            assert!(leftovers_in_update(&s.root).is_empty());
            assert!(registry.run_once.borrow().is_empty());
        }
    }

    // ---- "could not look" at the FIRST look --------------------------------------------------------
    //
    // The case above fails a look that comes AFTER the first one. These fail the first: R/.update
    // will not be listed, or it lists and the journal of the update will not be read (a scanner or
    // a backup at the logon has it open). Until 2026-10-14 the first look of `--recover` and of the
    // window answered "this install holds no such update" for both, and a folder whose journal
    // could not be read was not even counted. The rule they are held to now is the one of H1 ("no
    // update stays unproved for ever ... or is left for a start that can decide, and such a start
    // comes"): a look that could not be made decides nothing, so `--recover` leaves the update WITH
    // the logon entry registered again - Windows took it away before it started this run - and the
    // window is told to ask again. Nothing of the update is written in either.

    #[test]
    fn a_journal_that_cannot_be_read_is_an_update_that_may_be_there_it_is_left_and_the_logon_entry_is_put_back()
     {
        let mut u = Unproved::new("settle-journal-held");
        u.expect_premise();
        remove(&u.marker); // once it can be read, this update is put back: nobody is running
        let s = u.lp.scenario();
        let install_before = whole_install(s);
        let user_before = snapshot(&s.state);

        let disk = StdFs;
        {
            let held = JournalHeld::new(&disk);
            let counting = FaultFs::new(&held);
            // Its registry holds no entry: Windows has consumed it.
            let rig = Rig::with_transaction(s, &counting, "eeeeeeee");
            // `katana_updater --recover` is a recovery and then a settlement. The recovery can do
            // nothing with a folder it cannot read, and that is no failure of it.
            let recovered = recover_in_process(&s.root, &rig.services());
            assert!(recovered.is_ok(), "{}", describe(&recovered));
            assert_eq!(settle(s, &u.helper, &rig.services()), Settled::Left);

            assert!(
                held.asked.get() >= 2,
                "the premise: both asked for the journal, and it would not be read"
            );
            assert_eq!(
                counting.mutating_operations(),
                0,
                "nothing is written for a folder that was not read"
            );
            assert_eq!(
                difference(&install_before, &whole_install(s)),
                "",
                "the install was changed"
            );
            assert_eq!(
                journal_state_of(&s.root),
                JournalState::CommittedPendingHealth
            );
            assert_eq!(
                difference(&user_before, &snapshot(&s.state)),
                "",
                "the per-user folder was changed"
            );
            assert_eq!(
                rig.processes.scans.load(Ordering::SeqCst),
                0,
                "nothing is judged of an update that was not read"
            );
            assert_eq!(
                *rig.registry.run_once.borrow(),
                armed(&u.helper, &s.root),
                "no logon would come back to an update that may be there"
            );
            assert_eq!(rig.registry.run_once_cleared.get(), 0);
            assert!(rig.launcher.ran().is_empty());
            assert!(rig.launcher.started().is_empty());
        }

        // That logon, the scanner gone: the journal is read, and the update - no marker, the files
        // that release's, nobody running - is put back.
        let rig = Rig::with_transaction(s, &disk, "eeeeeeee");
        assert_eq!(settle(s, &u.helper, &rig.services()), Settled::RolledBack);
        assert_eq!(
            tree_hash(&s.root),
            s.v1_hash,
            "{}",
            difference(&s.v1_snapshot, &snapshot(&s.root))
        );
        assert!(leftovers_in_update(&s.root).is_empty());
        assert!(
            rig.registry.run_once.borrow().is_empty(),
            "nothing is left for a logon to do"
        );
    }

    #[test]
    fn a_journal_held_only_at_the_first_look_is_read_under_the_lock_and_the_update_is_settled() {
        // The scanner has the journal for one read: the first look does not see what the folder
        // is. That is no reason to go away - the look under the lock reads it, and the rule is
        // applied to what it says. With the marker there the update is confirmed.
        let mut u = Unproved::new("settle-journal-blip");
        u.expect_premise();
        let s = u.lp.scenario();
        let disk = StdFs;
        let mut held = JournalHeld::new(&disk);
        held.held_for = 1;
        let watched = WatchedFs::new(&held);
        let rig = Rig::with_transaction(s, &watched, "eeeeeeee");
        assert_eq!(settle(s, &u.helper, &rig.services()), Settled::Healthy);
        assert!(
            held.asked.get() >= 2,
            "the premise: refused once, then read"
        );
        assert_eq!(
            watched.locks.get(),
            1,
            "the lock was asked for, though the first look saw no pending update"
        );
        assert_eq!(
            tree_hash(&s.root),
            s.v2_hash,
            "{}",
            difference(&snapshot(&s.expected), &snapshot(&s.root))
        );
        assert!(leftovers_in_update(&s.root).is_empty());
        assert_eq!(
            *rig.registry.run_once_history.borrow(),
            vec![command(&u.helper, &s.root)]
        );
        assert!(rig.registry.run_once.borrow().is_empty());
    }

    #[test]
    fn an_update_folder_that_cannot_be_listed_at_the_first_look_is_not_an_install_with_nothing_to_settle()
     {
        let mut u = Unproved::new("settle-unlisted-first");
        u.expect_premise();
        let s = u.lp.scenario();
        let install_before = whole_install(s);
        let user_before = snapshot(&s.state);

        // `katana_updater --recover`'s settlement: no look at R/.update succeeds, the first
        // included.
        {
            let disk = StdFs;
            let unread = UnlistableFrom::new(&disk, 1);
            let counting = FaultFs::new(&unread);
            let rig = Rig::with_transaction(s, &counting, "eeeeeeee");
            assert_eq!(settle(s, &u.helper, &rig.services()), Settled::Left);
            assert!(
                unread.asked.get() >= 1,
                "the premise: the first look failed"
            );
            assert_eq!(counting.mutating_operations(), 0);
            assert_eq!(rig.processes.scans.load(Ordering::SeqCst), 0);
            assert_eq!(
                *rig.registry.run_once.borrow(),
                armed(&u.helper, &s.root),
                "no logon would come back to an update that may be there"
            );
            assert_eq!(rig.registry.run_once_cleared.get(), 0);
        }
        // The window: told to ask again, with nothing written - its marker neither - and the logon
        // entry as the dead helper left it.
        {
            let disk = StdFs;
            let unread = UnlistableFrom::new(&disk, 1);
            let counting = FaultFs::new(&unread);
            let registry = FakeRegistry::default();
            registry
                .run_once
                .borrow_mut()
                .insert(String::from(RUN_ONCE_NAME), command(&u.helper, &s.root));
            assert_eq!(
                detail::confirm_started_with(&s.root, &s.state, &counting, &registry),
                Confirmed::Busy
            );
            assert!(
                unread.asked.get() >= 1,
                "the premise: the first look failed"
            );
            assert_eq!(counting.mutating_operations(), 0);
            assert_eq!(*registry.run_once.borrow(), armed(&u.helper, &s.root));
            assert_eq!(registry.run_once_cleared.get(), 0);
            assert!(is_present(&u.helper));
        }
        assert_eq!(
            difference(&install_before, &whole_install(s)),
            "",
            "the install was changed"
        );
        assert_eq!(
            journal_state_of(&s.root),
            JournalState::CommittedPendingHealth
        );
        assert_eq!(
            difference(&user_before, &snapshot(&s.state)),
            "",
            "the per-user folder was changed"
        );
    }

    #[test]
    fn the_programs_are_asked_for_before_the_release_in_the_tree_is_read_and_again_after_it_and_nothing_changes_until_then()
     {
        // "Asked before the hashing, and again as the last thing" (recover_internal.rs). The
        // hashing itself goes to the disk directly and is not seen through the seams. What is
        // seen: the read of the release's manifest in the tree, which begins it (it is the list of
        // what to hash); every change to the disk; and the two questions. So: when the first
        // question was asked the manifest had not been read; when the second was, it had; and
        // until the second was answered nothing on the disk had changed - the first change after
        // it is the rollback's own first record.
        let mut u = Unproved::new("settle-scan-order");
        u.expect_premise();
        remove(&u.marker);
        let s = u.lp.scenario();

        let disk = StdFs;
        let log = LogFs::new(&disk);
        // Counts the reads of the installed manifest: the one in the tree (install.rs), not the
        // copy of the old one that the transaction keeps under .update. (Path equality is by
        // component, as the C++'s comparison of lexically normal paths is.)
        let manifest = s
            .root
            .join("share")
            .join("katana")
            .join("installed.manifest.json");
        let manifest_reads = Cell::new(0);
        let reads = ForwardingFs::new(&log);
        reads.on_read_file(|inner, path| {
            if path == manifest.as_path() {
                manifest_reads.set(manifest_reads.get() + 1);
            }
            inner.read_file(path)
        });
        let rig = Rig::with_transaction(s, &reads, "eeeeeeee");

        struct Moment {
            manifest_reads: i32,
            changes: usize,
        }
        let moments: RefCell<Vec<Moment>> = RefCell::new(Vec::new());
        let processes = ScriptedProcesses::new(Vec::new()); // nobody, whenever it is asked
        processes.set_on_scan(|_| {
            moments.borrow_mut().push(Moment {
                manifest_reads: manifest_reads.get(),
                changes: log.log.borrow().len(),
            });
        });
        let services = ApplyServices {
            fs: &reads,
            processes: &processes,
            launcher: &rig.launcher,
            registry: &rig.registry,
            trust: test_trust(),
            timings: rig.timings,
        };
        assert_eq!(settle(s, &u.helper, &services), Settled::RolledBack);

        let moments = moments.borrow();
        assert_eq!(moments.len(), 2);
        assert_eq!(
            moments[0].manifest_reads, 0,
            "the first question comes before the release is read, so before it is hashed"
        );
        assert!(moments[1].manifest_reads >= 1, "and the second after");
        assert_eq!(moments[0].changes, 0);
        assert_eq!(
            moments[1].changes, 0,
            "nothing on the disk changes before the second answer"
        );
        let entries = log.log.borrow();
        assert!(!entries.is_empty());
        assert!(
            entries[0].contains("ROLLING_BACK"),
            "the first change after it is the rollback's first record: {}",
            entries[0]
        );
        assert_eq!(
            tree_hash(&s.root),
            s.v1_hash,
            "{}",
            difference(&s.v1_snapshot, &snapshot(&s.root))
        );
    }
}

// ---- the same, cut off anywhere ------------------------------------------------------------------
//
// Every step is safe against a crash or a power loss at any point. Each loop dies before EVERY
// change to the disk of one settlement, then does what the machine does next - a recovery and a
// settlement, on a file system that works - and requires the whole tree to be one version's:
//
//   a healthy settlement   always the new one (before HEALTHY is on disk the marker decides again;
//                          after it any start sweeps);
//   a putting back         always the old one (before ROLLING_BACK it is decided again, with the
//                          same answer; after it every start finishes the rollback);
//   a window's confirming  the old one while its marker is not yet written - nothing says the
//                          release starts, and nobody is running - and the new one from then on.
//
// A loop that died nowhere would pass, so each requires a death at every k, at operation k.

mod settle_loops {
    use super::*;

    #[test]
    fn dying_before_every_change_of_a_healthy_settlement_always_ends_at_the_new_tree() {
        let mut u = Unproved::new("settle-loop-healthy");
        u.expect_premise();
        let total = {
            let s = u.lp.scenario();
            let disk = StdFs;
            let counting = FaultFs::new(&disk);
            let mut settled = Settled::Nothing;
            assert!(settle_until_crash(s, &u.helper, &counting, Some(&mut settled)).is_none());
            assert_eq!(settled, Settled::Healthy);
            counting.mutating_operations()
        };
        assert!(
            total >= 3,
            "a record, at least one file of the folder, the folder"
        );

        for k in 1..=total {
            let trace = format!("dying before change {k} of {total}");
            u.again();
            u.expect_premise();
            let s = u.lp.scenario();
            let user_before = snapshot(&s.state);
            let disk = StdFs;
            let faults = FaultFs::new(&disk);
            faults.crash_before(k);
            let Some(crash) = settle_until_crash(s, &u.helper, &faults, None) else {
                panic!("{trace}: it did not die");
            };
            assert_eq!(crash.operation, k, "{trace}");

            recover_then_settle(s, &u.helper);
            assert_eq!(
                tree_hash(&s.root),
                s.v2_hash,
                "{trace}: {}",
                difference(&snapshot(&s.expected), &snapshot(&s.root))
            );
            assert!(leftovers_in_update(&s.root).is_empty(), "{trace}");
            assert_eq!(
                difference(&user_before, &snapshot(&s.state)),
                "",
                "{trace}: the per-user folder was changed"
            );
        }
        // Past the last change there is nothing to die before.
        u.again();
        let s = u.lp.scenario();
        let disk = StdFs;
        let faults = FaultFs::new(&disk);
        faults.crash_before(total + 1);
        let mut settled = Settled::Nothing;
        assert!(settle_until_crash(s, &u.helper, &faults, Some(&mut settled)).is_none());
        assert_eq!(settled, Settled::Healthy);
        assert_eq!(faults.mutating_operations(), total);
    }

    #[test]
    fn dying_before_every_change_of_a_putting_back_always_ends_at_the_old_tree() {
        let mut u = Unproved::new("settle-loop-rollback");
        u.expect_premise();
        let total = {
            remove(&u.marker);
            let s = u.lp.scenario();
            let disk = StdFs;
            let counting = FaultFs::new(&disk);
            let mut settled = Settled::Nothing;
            assert!(settle_until_crash(s, &u.helper, &counting, Some(&mut settled)).is_none());
            assert_eq!(settled, Settled::RolledBack);
            assert_eq!(tree_hash(&s.root), s.v1_hash);
            counting.mutating_operations()
        };
        assert!(
            total >= 10,
            "two records, two renames for each file that changed, the folder"
        );

        for k in 1..=total {
            let trace = format!("dying before change {k} of {total}");
            u.again();
            u.expect_premise();
            remove(&u.marker);
            let s = u.lp.scenario();
            let user_before = snapshot(&s.state);
            let disk = StdFs;
            let faults = FaultFs::new(&disk);
            faults.crash_before(k);
            let Some(crash) = settle_until_crash(s, &u.helper, &faults, None) else {
                panic!("{trace}: it did not die");
            };
            assert_eq!(crash.operation, k, "{trace}");

            recover_then_settle(s, &u.helper);
            assert_eq!(
                tree_hash(&s.root),
                s.v1_hash,
                "{trace}: {}",
                difference(&s.v1_snapshot, &snapshot(&s.root))
            );
            assert!(leftovers_in_update(&s.root).is_empty(), "{trace}");
            assert_eq!(
                difference(&user_before, &snapshot(&s.state)),
                "",
                "{trace}: the per-user folder was changed"
            );
        }
    }

    #[test]
    fn dying_before_every_change_of_a_windows_confirming_ends_at_the_old_tree_before_its_marker_and_the_new_one_after()
     {
        let mut u = Unproved::new("settle-loop-confirm");
        u.expect_premise();
        // marker_written: the change that puts the window's marker on disk
        let (total, marker_written) = {
            remove(&u.marker);
            let s = u.lp.scenario();
            let disk = StdFs;
            let counting = FaultFs::new(&disk);
            let log = LogFs::new(&counting);
            let registry = FakeRegistry::default();
            assert_eq!(
                detail::confirm_started_with(&s.root, &s.state, &log, &registry),
                Confirmed::Done
            );
            let total = counting.mutating_operations();
            assert_eq!(log.log.borrow().len() as u64, total);
            let marker_written = log.find("writeDurable", "healthy-", 1);
            assert!(marker_written > 0);
            assert!(marker_written < total);
            (total, marker_written)
        };

        let mut old = 0u64;
        let mut fresh = 0u64;
        for k in 1..=total {
            let trace = format!("dying before change {k} of {total}");
            u.again();
            u.expect_premise();
            remove(&u.marker);
            let s = u.lp.scenario();
            let disk = StdFs;
            let faults = FaultFs::new(&disk);
            faults.crash_before(k);
            let registry = FakeRegistry::default();
            let died =
                crash_of(|| detail::confirm_started_with(&s.root, &s.state, &faults, &registry));
            let Err(crash) = died else {
                panic!("{trace}: it did not die");
            };
            assert_eq!(crash.operation, k, "{trace}");

            recover_then_settle(s, &u.helper);
            // The changes before k were made: the marker's write is one of them, or it is not.
            let said = k > marker_written;
            let wanted = if said { &s.v2_hash } else { &s.v1_hash };
            assert_eq!(
                &tree_hash(&s.root),
                wanted,
                "{trace}: versus the old: {}versus the new: {}",
                difference(&s.v1_snapshot, &snapshot(&s.root)),
                difference(&snapshot(&s.expected), &snapshot(&s.root))
            );
            assert!(leftovers_in_update(&s.root).is_empty(), "{trace}");
            if said {
                fresh += 1;
            } else {
                old += 1;
            }
        }
        // The loop reached both.
        assert_eq!(old, marker_written);
        assert_eq!(fresh, total - marker_written);
        assert!(old > 0);
        assert!(fresh > 0);
    }
}

// ---- the window of the new version ---------------------------------------------------------------

mod confirm_started {
    use super::*;

    #[test]
    fn the_window_of_the_new_version_settles_the_update_healthy_and_clears_what_the_dead_helper_left()
     {
        for marker_there in [true, false] {
            let trace = if marker_there {
                "the health run had written the marker"
            } else {
                "no marker: the window writes it"
            };
            let mut u = Unproved::new(&format!(
                "confirm-done-{}",
                if marker_there { "marker" } else { "none" }
            ));
            u.expect_premise();
            if !marker_there {
                remove(&u.marker);
            }
            let s = u.lp.scenario();
            let disk = StdFs;
            let log = LogFs::new(&disk);
            let registry = FakeRegistry::default();
            // as the swap that died left it
            registry
                .run_once
                .borrow_mut()
                .insert(String::from(RUN_ONCE_NAME), command(&u.helper, &s.root));

            assert_eq!(
                detail::confirm_started_with(&s.root, &s.state, &log, &registry),
                Confirmed::Done,
                "{trace}"
            );
            assert_eq!(
                tree_hash(&s.root),
                s.v2_hash,
                "{trace}: {}",
                difference(&snapshot(&s.expected), &snapshot(&s.root))
            );
            assert!(
                leftovers_in_update(&s.root).is_empty(),
                "{trace}: its folder is gone"
            );
            assert!(
                !is_present(&u.marker),
                "{trace}: the marker has done its work"
            );
            assert!(
                !is_present(&s.state.join(format!("run-{}", u.transaction))),
                "{trace}: nothing starts the dead helper's copy again"
            );
            assert!(
                !is_present(&s.state.join("cleanup.list")),
                "{trace}: nothing stayed, so nothing is listed"
            );
            assert!(
                registry.run_once.borrow().is_empty(),
                "{trace}: the entry named the copy that is gone"
            );
            assert_eq!(registry.run_once_cleared.get(), 1, "{trace}");
            assert!(
                registry.run_once_history.borrow().is_empty(),
                "{trace}: a window registers nothing"
            );

            // The proof is on disk before anything is settled by it.
            let healthy = log.find("appendDurable", " HEALTHY ", 1);
            let written = log.find("writeDurable", "healthy-", 1);
            assert!(healthy > 0, "{trace}");
            if marker_there {
                assert_eq!(
                    written, 0,
                    "{trace}: a marker that is there is not written again"
                );
            } else {
                assert!(written > 0, "{trace}");
                assert!(written < healthy, "{trace}");
                assert!(
                    log.log.borrow()[(written - 1) as usize].ends_with(" ok\n"),
                    "{trace}: what --update-health writes"
                );
            }
        }
    }

    #[test]
    fn a_held_lock_is_a_live_helper_the_window_asks_once_and_changes_nothing() {
        let mut u = Unproved::new("confirm-busy");
        u.expect_premise();
        remove(&u.marker);
        let s = u.lp.scenario();
        let install_before = whole_install(s);
        let user_before = snapshot(&s.state);

        let disk = StdFs;
        let mut busy = WatchedFs::new(&disk);
        busy.held_for = -1;
        let counting = FaultFs::new(&busy);
        let registry = FakeRegistry::default();
        registry
            .run_once
            .borrow_mut()
            .insert(String::from(RUN_ONCE_NAME), command(&u.helper, &s.root));
        assert_eq!(
            detail::confirm_started_with(&s.root, &s.state, &counting, &registry),
            Confirmed::Busy
        );

        assert_eq!(
            busy.locks.get(),
            1,
            "a window does not wait for the lock: it asks again later"
        );
        assert_eq!(
            counting.mutating_operations(),
            0,
            "not even its marker is written under a live helper"
        );
        assert_eq!(
            difference(&install_before, &whole_install(s)),
            "",
            "the install was changed"
        );
        assert_eq!(
            difference(&user_before, &snapshot(&s.state)),
            "",
            "the per-user folder was changed"
        );
        assert_eq!(*registry.run_once.borrow(), armed(&u.helper, &s.root));
        assert_eq!(registry.run_once_cleared.get(), 0);
    }

    #[test]
    fn with_no_such_update_the_window_asks_for_no_lock_and_writes_nothing() {
        {
            // Every start but the one after a helper died: an install with nothing pending.
            let mut s = Scenario::new("confirm-nothing");
            s.v1 = tiny("0.1.0", 1);
            write_install(&s.root, &s.v1);
            let disk = StdFs;
            let watched = WatchedFs::new(&disk);
            let counting = FaultFs::new(&watched);
            let registry = FakeRegistry::default();
            registry
                .run_once
                .borrow_mut()
                .insert(String::from(RUN_ONCE_NAME), String::from("somebody's"));
            assert_eq!(
                detail::confirm_started_with(&s.root, &s.state, &counting, &registry),
                Confirmed::Nothing
            );
            assert_eq!(counting.mutating_operations(), 0);
            assert_eq!(watched.locks.get(), 0);
            assert_eq!(registry.run_once_cleared.get(), 0);
            assert_eq!(registry.run_once.borrow().len(), 1);
            assert!(!is_present(&s.root.join(".update")));
            assert!(
                snapshot(&s.state).is_empty(),
                "nothing is made in the per-user folder"
            );
        }
        {
            // An update cut off in the middle of its commit is not a version that starts: the
            // window confirms nothing of it (its own recover() has it put back).
            let mut lp = Loop::new("confirm-not-mine", &build_fixture_scenario);
            let p = measure_clean_apply(&mut lp);
            crash_at(&mut lp, p.renames[p.renames.len() / 2], 0);
            let s = lp.scenario();
            assert_eq!(journal_state_of(&s.root), JournalState::Committing);
            let install_before = whole_install(s);
            let disk = StdFs;
            let watched = WatchedFs::new(&disk);
            let counting = FaultFs::new(&watched);
            let registry = FakeRegistry::default();
            assert_eq!(
                detail::confirm_started_with(&s.root, &s.state, &counting, &registry),
                Confirmed::Nothing
            );
            assert_eq!(counting.mutating_operations(), 0);
            assert_eq!(watched.locks.get(), 0);
            assert_eq!(registry.run_once_cleared.get(), 0);
            assert_eq!(difference(&install_before, &whole_install(s)), "");
        }
    }

    #[test]
    fn what_would_not_go_of_the_dead_helpers_copy_is_listed_for_the_next_start() {
        let mut u = Unproved::new("confirm-leftover");
        u.expect_premise();
        let s = u.lp.scenario();
        let disk = StdFs;
        // The copy is held - a scanner is reading it - so it cannot be deleted, and neither can the
        // folder it is in.
        let holding = ForwardingFs::new(&disk);
        holding.on_remove_file(|inner, path| {
            if path.file_name() == Some(OsStr::new("katana_updater.exe")) {
                Err(os_error(
                    ErrorCode::FileExportFailure,
                    "sharing violation",
                    path,
                    32,
                ))
            } else {
                inner.remove_file(path)
            }
        });
        let registry = FakeRegistry::default();
        assert_eq!(
            detail::confirm_started_with(&s.root, &s.state, &holding, &registry),
            Confirmed::Done,
            "the update is settled all the same"
        );
        assert!(leftovers_in_update(&s.root).is_empty());
        assert!(!is_present(&u.marker));
        assert!(is_present(&u.helper), "the premise: it was held");

        // U/cleanup.list, one path to a line (state.rs): the file that would not go, and its
        // folder.
        let list = read_text(&s.state.join("cleanup.list"));
        let listed: BTreeSet<PathBuf> = list
            .split(|&byte| byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(path_from_utf8)
            .collect();
        assert_eq!(
            listed,
            BTreeSet::from([u.helper.clone(), parent_of(&u.helper)])
        );
    }

    #[test]
    fn a_window_that_cannot_write_its_marker_settles_nothing_and_never_puts_anything_back() {
        let mut u = Unproved::new("confirm-no-marker");
        u.expect_premise();
        remove(&u.marker);
        let s = u.lp.scenario();
        let install_before = whole_install(s);

        let disk = StdFs;
        let refusing = ForwardingFs::new(&disk);
        refusing.on_write_durable(|inner, path, bytes| {
            let marker = path
                .file_name()
                .is_some_and(|name| text(Path::new(name)).starts_with("healthy-"));
            if marker {
                Err(os_error(
                    ErrorCode::FileExportFailure,
                    "access denied",
                    path,
                    5,
                ))
            } else {
                inner.write_durable(path, bytes)
            }
        });
        let registry = FakeRegistry::default();
        registry
            .run_once
            .borrow_mut()
            .insert(String::from(RUN_ONCE_NAME), command(&u.helper, &s.root));
        // With no marker the rule has nothing to call it healthy by, and a window is never the one
        // to put an update back: it is left, and the window is told to ask again.
        assert_eq!(
            detail::confirm_started_with(&s.root, &s.state, &refusing, &registry),
            Confirmed::Busy
        );
        assert_eq!(tree_hash(&s.root), s.v2_hash, "nothing was put back");
        assert_eq!(
            difference(&install_before, &whole_install(s)),
            "",
            "the install was changed"
        );
        assert_eq!(
            journal_state_of(&s.root),
            JournalState::CommittedPendingHealth
        );
        assert_eq!(
            *registry.run_once.borrow(),
            armed(&u.helper, &s.root),
            "the logon net stays for an update that is not settled"
        );
        assert_eq!(registry.run_once_cleared.get(), 0);
        assert!(
            is_present(&u.helper),
            "and so does the helper the entry names"
        );
    }

    #[test]
    fn a_journal_that_cannot_be_read_just_now_is_asked_about_again_and_no_marker_is_written_for_it()
    {
        // The journal of the update is there and will not be read (a scanner has it open). The
        // window does not know what the folder is, so it is not "nothing to confirm" - and it is
        // not a thing to write a marker for either: a folder that was not read may be an update a
        // crash cut off in its commit, of which a window that starts is no proof. Asked again
        // later, with the journal readable, it is the proof and settles the update (recover.rs,
        // Confirmed).
        let mut u = Unproved::new("confirm-journal-held");
        u.expect_premise();
        remove(&u.marker); // so that a marker this call wrote would be seen
        let s = u.lp.scenario();
        let install_before = whole_install(s);
        let user_before = snapshot(&s.state);

        let disk = StdFs;
        let registry = FakeRegistry::default();
        registry
            .run_once
            .borrow_mut()
            .insert(String::from(RUN_ONCE_NAME), command(&u.helper, &s.root));
        {
            let held = JournalHeld::new(&disk);
            let counting = FaultFs::new(&held);
            assert_eq!(
                detail::confirm_started_with(&s.root, &s.state, &counting, &registry),
                Confirmed::Busy
            );
            assert!(
                held.asked.get() >= 1,
                "the premise: the journal was asked for, and would not be read"
            );
            assert_eq!(counting.mutating_operations(), 0, "its marker included");
        }
        assert_eq!(
            difference(&install_before, &whole_install(s)),
            "",
            "the install was changed"
        );
        assert_eq!(
            journal_state_of(&s.root),
            JournalState::CommittedPendingHealth
        );
        assert_eq!(
            difference(&user_before, &snapshot(&s.state)),
            "",
            "the per-user folder was changed"
        );
        assert_eq!(
            *registry.run_once.borrow(),
            armed(&u.helper, &s.root),
            "the logon net stays for an update that is not settled"
        );
        assert_eq!(registry.run_once_cleared.get(), 0);

        assert_eq!(
            detail::confirm_started_with(&s.root, &s.state, &disk, &registry),
            Confirmed::Done
        );
        assert_eq!(
            tree_hash(&s.root),
            s.v2_hash,
            "{}",
            difference(&snapshot(&s.expected), &snapshot(&s.root))
        );
        assert!(leftovers_in_update(&s.root).is_empty());
        assert!(registry.run_once.borrow().is_empty());
    }
}

// ---- a journal that is not whole -----------------------------------------------------------------

fn tear_journal(journal: &Path, bytes: usize) {
    let mut whole = read_text(journal);
    assert!(whole.len() > bytes);
    whole.truncate(whole.len() - bytes);
    write_text(journal, whole);
}

mod recover_torn {
    use super::*;

    #[test]
    fn a_journal_cut_off_in_the_middle_of_any_record_of_the_commit_is_put_back_exactly() {
        let mut lp = Loop::new("recover-torn-commit", &build_fixture_scenario);
        let p = measure_clean_apply(&mut lp);
        let n = p.renames.len();
        let mut cases = 0usize;
        for at in [n / 4, n / 2, 3 * n / 4, n - 1] {
            for cut in [1usize, 2, 5, 12, 40] {
                let trace =
                    format!("crash before rename {at}, last {cut} bytes of the journal lost");
                crash_at(&mut lp, p.renames[at], 0);
                let s = lp.scenario();
                tear_journal(
                    &s.root
                        .join(".update")
                        .join(only_transaction(s))
                        .join("journal"),
                    cut,
                );
                assert!(recover_clean(s).is_ok(), "{trace}");
                assert_eq!(
                    tree_hash(&s.root),
                    s.v1_hash,
                    "{trace}: {}",
                    difference(&s.v1_snapshot, &snapshot(&s.root))
                );
                assert!(leftovers_in_update(&s.root).is_empty(), "{trace}");
                cases += 1;
            }
        }
        assert_eq!(cases, 20);
    }

    #[test]
    fn a_journal_lost_beyond_its_first_record_still_puts_every_old_file_back() {
        // Damage in the middle of the file (the disk, not a torn write) leaves a journal that says
        // PLAN and nothing else, over a tree whose files have all moved. The journal cannot say
        // what to put back, but the folder can: every file under old/ was an old file of this
        // install, and deleting the folder without it would lose the file for good. Files the
        // release ADDED cannot be known from nothing, so they stay: the tree is the old version's
        // files and some new ones, never the old version's files missing.
        let mut lp = Loop::new("recover-damaged-plan", &build_fixture_scenario);
        let p = measure_clean_apply(&mut lp);
        crash_at(&mut lp, p.pending, 0); // every file has moved, the pending record is not written
        let s = lp.scenario();
        let journal = s
            .root
            .join(".update")
            .join(only_transaction(s))
            .join("journal");
        let whole = read_text(&journal);
        let first_line = whole
            .iter()
            .position(|&byte| byte == b'\n')
            .map_or(0, |at| at + 1);
        write_text(&journal, &whole[..first_line]);
        assert_eq!(
            parse_journal(read_text(&journal)).state,
            JournalState::Planned
        );

        assert!(recover_clean(s).is_ok());
        let after = snapshot(&s.root);
        for (name, what) in &s.v1_snapshot {
            let Some(found) = after.get(name) else {
                panic!("lost: {name}");
            };
            assert_eq!(found, what, "not the old bytes: {name}");
        }
    }

    #[test]
    fn what_is_under_old_and_no_update_would_put_there_is_not_restored_and_not_an_error() {
        let f = Forgery::new("recover-old-junk");
        // Names no update writes: a letter outside ASCII (the path rules are ASCII only), and a
        // first folder that is not bin, share, etc or lib.
        let cafe = path_from_utf8("bin/caf\u{e9}.dll");
        write_text(&f.work.join("old").join(&cafe), "a name no update writes");
        write_text(
            &f.work.join("old/docs/readme.txt"),
            "a folder no update has",
        );
        f.journal(&[
            record(JournalOp::Plan, &["deadbeef", "1", "2"]),
            record(JournalOp::Replace, &["bin/lib.dll"]),
            record(JournalOp::Committing, &[]),
        ]);
        let disk = StdFs;
        let rig = Rig::with_transaction(&f.s, &disk, "eeeeeeee");
        assert!(recover_in_process(&f.s.root, &rig.services()).is_ok());
        assert_eq!(
            read_text(&f.s.root.join("bin/lib.dll")),
            b"lib 0.1.0\n",
            "the honest one was restored"
        );
        assert!(!is_present(&f.s.root.join(&cafe)));
        assert!(!is_present(&f.s.root.join("docs")));
        let log =
            String::from_utf8_lossy(&read_text(&f.s.root.join(".update").join("recovery.log")))
                .into_owned();
        assert!(log.contains("caf"), "{log}");
        assert!(log.contains("readme.txt"), "{log}");
    }

    #[test]
    fn a_double_dot_inside_a_name_is_an_ordinary_name_to_rollback() {
        let f = Forgery::new("recover-dots-in-name");
        write_text(&f.s.root.join("share/katana/a..b.dat"), "the new a..b\n");
        write_text(&f.work.join("old/share/katana/a..b.dat"), "the old a..b\n");
        f.journal(&[
            record(JournalOp::Plan, &["deadbeef", "1", "2"]),
            record(JournalOp::Replace, &["bin/lib.dll"]),
            record(JournalOp::Replace, &["share/katana/a..b.dat"]),
            record(JournalOp::Committing, &[]),
        ]);
        let disk = StdFs;
        let rig = Rig::with_transaction(&f.s, &disk, "eeeeeeee");
        assert!(recover_in_process(&f.s.root, &rig.services()).is_ok());
        assert_eq!(
            read_text(&f.s.root.join("share/katana/a..b.dat")),
            b"the old a..b\n"
        );
        assert!(
            !is_present(&f.s.root.join(".update").join("recovery.log")),
            "no record was refused"
        );
    }

    #[test]
    fn a_bare_install_folder_is_a_folder_the_transaction_may_have_made_but_never_a_file_to_restore()
    {
        let f = Forgery::new("recover-bare-folder");
        make_directories(&f.s.root.join("etc"));
        f.journal(&[
            record(JournalOp::Plan, &["deadbeef", "1", "2"]),
            record(JournalOp::MakeDirectory, &["etc"]),
            record(JournalOp::Replace, &["bin/lib.dll"]),
            record(JournalOp::Remove, &["lib"]), // a forged "delete the whole folder"
            record(JournalOp::Committing, &[]),
        ]);
        let disk = StdFs;
        let rig = Rig::with_transaction(&f.s, &disk, "eeeeeeee");
        assert!(recover_in_process(&f.s.root, &rig.services()).is_ok());
        assert!(
            !is_present(&f.s.root.join("etc")),
            "the empty folder the transaction made is gone"
        );
        let log =
            String::from_utf8_lossy(&read_text(&f.s.root.join(".update").join("recovery.log")))
                .into_owned();
        assert!(log.contains("REMOVE lib"), "{log}");
        assert!(
            !log.contains("MKDIR etc"),
            "and the folder was not refused: {log}"
        );
    }

    #[test]
    fn the_install_folder_may_be_given_with_a_trailing_separator() {
        let mut lp = Loop::new("recover-trailing-separator", &build_fixture_scenario);
        let p = measure_clean_apply(&mut lp);
        let mut spelled = path_to_utf8(&lp.scenario().root);
        spelled.push(b'/');
        let spelled = path_from_utf8(spelled);
        let disk = StdFs;
        {
            crash_at(&mut lp, p.renames[p.renames.len() / 2], 0);
            let s = lp.scenario();
            let rig = Rig::with_transaction(s, &disk, "eeeeeeee");
            assert!(recover_in_process(&spelled, &rig.services()).is_ok());
            assert_eq!(
                tree_hash(&s.root),
                s.v1_hash,
                "{}",
                difference(&s.v1_snapshot, &snapshot(&s.root))
            );
        }
        {
            crash_at(&mut lp, p.renames[p.renames.len() / 2], 0);
            let s = lp.scenario();
            let rig = Rig::with_transaction(s, &disk, "eeeeeeee");
            assert!(rollback(&spelled, &only_transaction(s), &rig.services()).is_ok());
            assert_eq!(
                tree_hash(&s.root),
                s.v1_hash,
                "{}",
                difference(&s.v1_snapshot, &snapshot(&s.root))
            );
        }
    }
}

// ---- recover(): which helper it runs, with what, and what it sweeps ------------------------------

struct Interrupted {
    lp: Loop,
    transaction: String,
    helper_name: PathBuf,
}

impl Interrupted {
    fn new(label: &str) -> Self {
        let mut lp = Loop::new(label, &build_fixture_scenario);
        let p = measure_clean_apply(&mut lp);
        crash_at(&mut lp, p.renames[p.renames.len() / 2], 0);
        let transaction = only_transaction(lp.scenario());
        let helper_name = installed_program(Path::new(""), "katana_updater")
            .file_name()
            .map(PathBuf::from)
            .unwrap_or_default();
        Self {
            lp,
            transaction,
            helper_name,
        }
    }

    fn copy(&mut self) -> PathBuf {
        self.lp
            .scenario()
            .state
            .join(format!("run-{}", self.transaction))
            .join(&self.helper_name)
    }

    fn own(&mut self) -> PathBuf {
        self.lp.scenario().root.join("bin").join(&self.helper_name)
    }
}

const SHORT_WAIT: Duration = Duration::from_millis(100);

mod recover_with {
    use super::*;

    #[test]
    fn the_helper_copy_is_run_with_recover_and_the_root_from_its_own_folder_and_the_state_folder_is_left_as_it_was()
     {
        let mut x = Interrupted::new("recoverwith-args");
        let copy = x.copy();
        let s = x.lp.scenario();
        write_text(&copy, "a stand-in for the helper copy");
        // The per-user folder, entry by entry and byte by byte, before the call.
        let before = snapshot(&s.state);
        assert!(
            before.contains_key(&format!("run-{}", x.transaction)),
            "the premise: the copy's folder is in U"
        );
        let disk = StdFs;
        let launcher = FakeLauncher::default();
        let status = detail::recover_with(
            &s.root,
            &disk,
            &launcher,
            &s.state,
            SHORT_WAIT,
            ORDINARY_START,
        );
        assert!(status.is_ok(), "{}", describe(&status));

        let ran = launcher.ran();
        assert_eq!(ran.len(), 1);
        assert_eq!(ran[0].executable, copy);
        assert_eq!(
            ran[0].arguments,
            vec![String::from("--recover"), text(&s.root)]
        );
        assert_eq!(ran[0].working_directory, parent_of(&copy));
        assert!(
            ran[0].timeout > Duration::from_secs(30),
            "a bound, and not a short one"
        );
        assert!(launcher.started().is_empty());
        // recover() is the first call of katana_cli and katana_mcp too, and a headless program
        // writes no per-user state (recover.rs, "WHAT RECOVERY WRITES, AND WHERE";
        // docs/headless.md). The state folder is read, to find the copy, and that is all: the copy
        // that was run is still there, no cleanup.list is begun, and nothing else in U is made,
        // changed or removed.
        assert!(
            is_present(&copy),
            "the helper copy is per-user state: it is not recover()'s to delete"
        );
        assert!(!is_present(&s.state.join("cleanup.list")));
        assert_eq!(
            difference(&before, &snapshot(&s.state)),
            "",
            "recover() changed the per-user state folder"
        );
    }

    #[test]
    fn the_root_is_passed_without_a_trailing_separator() {
        let mut x = Interrupted::new("recoverwith-trailing");
        let copy = x.copy();
        let s = x.lp.scenario();
        write_text(&copy, "stand-in");
        let disk = StdFs;
        let launcher = FakeLauncher::default();
        let mut spelled = path_to_utf8(&s.root);
        spelled.push(b'/');
        assert!(
            detail::recover_with(
                &path_from_utf8(spelled),
                &disk,
                &launcher,
                &s.state,
                SHORT_WAIT,
                ORDINARY_START
            )
            .is_ok()
        );
        let ran = launcher.ran();
        assert_eq!(ran.len(), 1);
        assert_eq!(
            ran[0].arguments,
            vec![String::from("--recover"), text(&s.root)]
        );
    }

    #[test]
    fn the_copy_outside_the_install_is_preferred_and_the_installs_own_is_the_fallback() {
        {
            let mut x = Interrupted::new("recoverwith-prefers-copy");
            let copy = x.copy();
            let own = x.own();
            let s = x.lp.scenario();
            write_text(&copy, "the copy");
            write_text(&own, "the install's own");
            let disk = StdFs;
            let launcher = FakeLauncher::default();
            assert!(
                detail::recover_with(
                    &s.root,
                    &disk,
                    &launcher,
                    &s.state,
                    SHORT_WAIT,
                    ORDINARY_START
                )
                .is_ok()
            );
            let ran = launcher.ran();
            assert_eq!(ran.len(), 1);
            assert_eq!(ran[0].executable, copy);
            assert!(
                is_present(&own),
                "the install's own helper is not the copy's to delete"
            );
        }
        {
            let mut x = Interrupted::new("recoverwith-falls-back");
            let own = x.own();
            let s = x.lp.scenario();
            write_text(&own, "the install's own");
            let disk = StdFs;
            let launcher = FakeLauncher::default();
            assert!(
                detail::recover_with(
                    &s.root,
                    &disk,
                    &launcher,
                    &s.state,
                    SHORT_WAIT,
                    ORDINARY_START
                )
                .is_ok()
            );
            let ran = launcher.ran();
            assert_eq!(ran.len(), 1);
            assert_eq!(ran[0].executable, own);
            assert_eq!(ran[0].working_directory, parent_of(&own));
        }
    }

    #[test]
    fn a_program_started_as_administrator_runs_the_installs_own_helper_and_never_the_copy_in_the_per_user_folder()
     {
        // The ruling this holds recover() to is H4's: no helper runs with administrator rights from
        // the per-user folder, which the same person's programs WITHOUT those rights can write.
        // `UPDATE APPLY CONFIRM` is refused in such a Katana for it; recover() is the other way to a
        // helper in U, and all that sends it there is writable by those programs too - a journal
        // under R/.update, a file named katana_updater in U/run-<tx>. So, started as administrator:
        // the install's own helper, or nothing and the command to run by hand.
        {
            // Both are there. The ordinary start prefers the copy (the case above); this one may
            // not.
            let mut x = Interrupted::new("recoverwith-elevated-own");
            let copy = x.copy();
            let own = x.own();
            let s = x.lp.scenario();
            write_text(&copy, "the copy in the per-user folder");
            write_text(&own, "the install's own");
            let user_before = snapshot(&s.state);
            let disk = StdFs;
            let launcher = FakeLauncher::default();
            let status = detail::recover_with(
                &s.root,
                &disk,
                &launcher,
                &s.state,
                SHORT_WAIT,
                STARTED_AS_ADMINISTRATOR,
            );
            assert!(status.is_ok(), "{}", describe(&status));
            let ran = launcher.ran();
            assert_eq!(ran.len(), 1);
            assert_eq!(ran[0].executable, own);
            assert_eq!(
                ran[0].arguments,
                vec![String::from("--recover"), text(&s.root)]
            );
            assert_eq!(ran[0].working_directory, parent_of(&own));
            assert!(launcher.started().is_empty());
            assert_eq!(
                difference(&user_before, &snapshot(&s.state)),
                "",
                "the per-user folder was changed"
            );
        }
        {
            // Only the copy is there: it is NOT run, and the answer is the error that names the
            // command to run by hand. Nothing of the install is touched.
            let mut x = Interrupted::new("recoverwith-elevated-none");
            let copy = x.copy();
            let own = x.own();
            let s = x.lp.scenario();
            write_text(&copy, "the copy in the per-user folder");
            assert!(
                !is_present(&own),
                "the premise: the install has no helper of its own"
            );
            let crashed = tree_hash(&s.root);
            let disk = StdFs;
            let launcher = FakeLauncher::default();
            let status = detail::recover_with(
                &s.root,
                &disk,
                &launcher,
                &s.state,
                SHORT_WAIT,
                STARTED_AS_ADMINISTRATOR,
            );
            let error = status.expect_err("the helper copy in the per-user folder was taken");
            assert_eq!(error.code, ErrorCode::NotFound);
            assert!(
                error.message.contains("katana_updater --recover"),
                "{}",
                error.describe()
            );
            assert!(error.message.contains(&text(&s.root)));
            assert!(
                launcher.ran().is_empty(),
                "a file of the per-user folder was run with administrator rights"
            );
            assert!(launcher.started().is_empty());
            assert_eq!(tree_hash(&s.root), crashed);
            assert!(
                is_present(&copy),
                "and it is not this program's to delete either"
            );

            // The same install and the same copy, from a program started the ordinary way: the
            // copy is what runs. The refusal above was the administrator's, not the copy's.
            assert!(
                detail::recover_with(
                    &s.root,
                    &disk,
                    &launcher,
                    &s.state,
                    SHORT_WAIT,
                    ORDINARY_START
                )
                .is_ok()
            );
            let ran = launcher.ran();
            assert_eq!(ran.len(), 1);
            assert_eq!(ran[0].executable, copy);
        }
    }

    #[test]
    fn a_helper_that_fails_leaves_its_copy_for_the_run_once_entry_and_says_the_command_by_hand() {
        let mut x = Interrupted::new("recoverwith-fails");
        let copy = x.copy();
        let s = x.lp.scenario();
        write_text(&copy, "stand-in");
        let disk = StdFs;
        let launcher = FakeLauncher::default();
        launcher.set_on_run(|_| {
            Ok(SpawnResult {
                exit_code: 2,
                ..SpawnResult::default()
            })
        });
        let status = detail::recover_with(
            &s.root,
            &disk,
            &launcher,
            &s.state,
            SHORT_WAIT,
            ORDINARY_START,
        );
        let error = status.expect_err("the helper failed");
        assert!(error.message.contains("katana_updater --recover"));
        assert!(error.context.contains('2'), "{}", error.describe());
        assert!(
            is_present(&copy),
            "the RunOnce entry runs --recover from it at the next logon"
        );

        let timed_out_launcher = FakeLauncher::default();
        timed_out_launcher.set_on_run(|_| {
            Ok(SpawnResult {
                timed_out: true,
                ..SpawnResult::default()
            })
        });
        assert!(
            detail::recover_with(
                &s.root,
                &disk,
                &timed_out_launcher,
                &s.state,
                SHORT_WAIT,
                ORDINARY_START
            )
            .is_err()
        );
        assert!(is_present(&copy));
    }

    #[test]
    fn what_a_lock_kept_from_being_deleted_after_a_rollback_is_swept_at_the_next_start_without_a_helper()
     {
        let mut lp = Loop::new("recoverwith-sweep", &build_fixture_scenario);
        let p = measure_clean_apply(&mut lp);
        crash_at(&mut lp, p.pending, 0); // every file is in place; the pending record is not written
        let s = lp.scenario();

        // The rollback moves the new katana.exe back into the transaction's folder; something
        // holds it, so the folder cannot be deleted, and nothing here can queue it for later.
        let disk = StdFs;
        let holding = holding_katana_exe(&disk);
        {
            let rig = Rig::with_transaction(s, &holding, "eeeeeeee");
            assert!(recover_in_process(&s.root, &rig.services()).is_ok());
        }
        assert_eq!(tree_hash(&s.root), s.v1_hash);
        assert!(
            !leftovers_in_update(&s.root).is_empty(),
            "the folder stays: a file in it is held"
        );
        assert_eq!(journal_state_of(&s.root), JournalState::RolledBack);

        // The next start: the file is free. recover() sweeps it without starting anything.
        let launcher = FakeLauncher::default();
        assert!(
            detail::recover_with(
                &s.root,
                &disk,
                &launcher,
                &s.state,
                SHORT_WAIT,
                ORDINARY_START
            )
            .is_ok()
        );
        assert!(leftovers_in_update(&s.root).is_empty());
        assert!(launcher.ran().is_empty());
        assert_eq!(tree_hash(&s.root), s.v1_hash);
    }

    #[test]
    fn what_is_still_held_at_the_next_start_stays_under_its_journal_and_is_not_queued_in_the_state_folder()
     {
        let mut lp = Loop::new("recoverwith-sweep-held", &build_fixture_scenario);
        let p = measure_clean_apply(&mut lp);
        crash_at(&mut lp, p.pending, 0); // every file is in place; the pending record is not written
        let s = lp.scenario();

        // As in the test above: the rollback leaves a folder that cannot be deleted, because
        // something holds the katana.exe in it.
        let disk = StdFs;
        let holding = holding_katana_exe(&disk);
        {
            let rig = Rig::with_transaction(s, &holding, "eeeeeeee");
            assert!(recover_in_process(&s.root, &rig.services()).is_ok());
        }
        assert!(
            !leftovers_in_update(&s.root).is_empty(),
            "the premise: the folder stays"
        );
        assert_eq!(journal_state_of(&s.root), JournalState::RolledBack);
        let before = snapshot(&s.state);

        // The next start, and the file is STILL held. recover() may be katana_cli's or katana_mcp's
        // first call, and a headless program writes no per-user state (recover.rs): what the sweep
        // cannot delete is not queued in U/cleanup.list, and U is as it was. Nothing is lost by
        // that: the journal is the last thing a sweep deletes (remove_tree.rs), so the folder still
        // says that its transaction is over ...
        let launcher = FakeLauncher::default();
        assert!(
            detail::recover_with(
                &s.root,
                &holding,
                &launcher,
                &s.state,
                SHORT_WAIT,
                ORDINARY_START
            )
            .is_ok()
        );
        assert!(launcher.ran().is_empty());
        assert!(
            !leftovers_in_update(&s.root).is_empty(),
            "a file in it is held"
        );
        assert_eq!(journal_state_of(&s.root), JournalState::RolledBack);
        assert!(!is_present(&s.state.join("cleanup.list")));
        assert_eq!(
            difference(&before, &snapshot(&s.state)),
            "",
            "recover() changed the per-user state folder"
        );

        // ... and the start after that, with the file free, finds it and sweeps it.
        assert!(
            detail::recover_with(
                &s.root,
                &disk,
                &launcher,
                &s.state,
                SHORT_WAIT,
                ORDINARY_START
            )
            .is_ok()
        );
        assert!(leftovers_in_update(&s.root).is_empty());
        assert!(launcher.ran().is_empty());
        assert_eq!(difference(&before, &snapshot(&s.state)), "");
        assert_eq!(tree_hash(&s.root), s.v1_hash);
    }

    #[test]
    fn a_sweep_is_left_to_the_update_that_is_running_if_there_is_one() {
        let mut lp = Loop::new("recoverwith-sweep-locked", &build_fixture_scenario);
        let p = measure_clean_apply(&mut lp);
        crash_at(&mut lp, p.healthy + 1, 0);
        let s = lp.scenario();
        assert_eq!(journal_state_of(&s.root), JournalState::Healthy);
        let disk = StdFs;
        let holder = disk
            .try_lock(&s.root.join(".update").join("lock"))
            .expect("the lock can be taken");
        let launcher = FakeLauncher::default();
        assert!(
            detail::recover_with(
                &s.root,
                &disk,
                &launcher,
                &s.state,
                SHORT_WAIT,
                ORDINARY_START
            )
            .is_ok()
        );
        assert!(
            !leftovers_in_update(&s.root).is_empty(),
            "an update is running: it is cleaning up after itself"
        );
        drop(holder);
        assert!(
            detail::recover_with(
                &s.root,
                &disk,
                &launcher,
                &s.state,
                SHORT_WAIT,
                ORDINARY_START
            )
            .is_ok()
        );
        assert!(leftovers_in_update(&s.root).is_empty());
    }

    #[test]
    fn nothing_to_do_starts_nothing_and_takes_no_lock() {
        let mut s = Scenario::new("recoverwith-nothing");
        s.v1 = tiny("0.1.0", 1);
        write_install(&s.root, &s.v1);
        let disk = StdFs;
        let watched = WatchedFs::new(&disk);
        let launcher = FakeLauncher::default();
        assert!(
            detail::recover_with(
                &s.root,
                &watched,
                &launcher,
                &s.state,
                SHORT_WAIT,
                ORDINARY_START
            )
            .is_ok()
        );
        assert!(launcher.ran().is_empty());
        assert_eq!(watched.locks.get(), 0);
        write_text(&s.root.join(".update").join("lock"), "");
        assert!(
            detail::recover_with(
                &s.root,
                &watched,
                &launcher,
                &s.state,
                SHORT_WAIT,
                ORDINARY_START
            )
            .is_ok()
        );
        assert_eq!(watched.locks.get(), 0);
    }

    #[test]
    fn an_update_that_was_never_said_to_start_is_left_exactly_as_it_is_with_no_lock_and_no_helper()
    {
        // recover() is the first call of every program - the health run of a live helper among
        // them, and katana_cli and katana_mcp, which write no per-user state. Over an update that
        // is in place and not yet proved it must therefore do NOTHING: take no lock (the helper
        // that owns the update may be alive and waiting for this very program to start), run no
        // helper (no recursion: the helper would be starting what started it), and leave the
        // install and the per-user folder as they are - with the marker there and without it.
        for marker_there in [true, false] {
            let trace = if marker_there {
                "its marker is there"
            } else {
                "its marker is not there"
            };
            let mut u = Unproved::new(&format!(
                "recoverwith-unproved-{}",
                if marker_there { "marker" } else { "none" }
            ));
            u.expect_premise();
            if !marker_there {
                remove(&u.marker);
            }
            assert!(
                is_present(&u.helper),
                "{trace}: the premise: there is a helper copy it could run"
            );
            let s = u.lp.scenario();
            let install_before = whole_install(s);
            let user_before = snapshot(&s.state);

            let disk = StdFs;
            let watched = WatchedFs::new(&disk);
            let counting = FaultFs::new(&watched);
            let launcher = FakeLauncher::default();
            assert!(
                detail::recover_with(
                    &s.root,
                    &counting,
                    &launcher,
                    &s.state,
                    SHORT_WAIT,
                    ORDINARY_START
                )
                .is_ok(),
                "{trace}"
            );
            assert_eq!(watched.locks.get(), 0, "{trace}");
            assert!(launcher.ran().is_empty(), "{trace}");
            assert!(launcher.started().is_empty(), "{trace}");
            assert_eq!(counting.mutating_operations(), 0, "{trace}");
            assert_eq!(
                difference(&install_before, &whole_install(s)),
                "",
                "{trace}: the install was changed"
            );
            assert_eq!(
                difference(&user_before, &snapshot(&s.state)),
                "",
                "{trace}: the per-user folder was changed"
            );

            // And the recovery the helper itself runs first under `--recover` is the same: it is
            // what comes AFTER it there, settle_unproved, that settles such an update.
            let rig = Rig::with_transaction(s, &counting, "eeeeeeee");
            assert!(
                recover_in_process(&s.root, &rig.services()).is_ok(),
                "{trace}"
            );
            assert_eq!(watched.locks.get(), 0, "{trace}");
            assert_eq!(counting.mutating_operations(), 0, "{trace}");
            assert!(rig.registry.run_once_history.borrow().is_empty(), "{trace}");
            assert_eq!(rig.registry.run_once_cleared.get(), 0, "{trace}");
            assert_eq!(
                difference(&install_before, &whole_install(s)),
                "",
                "{trace}: the install was changed"
            );
            assert_eq!(
                difference(&user_before, &snapshot(&s.state)),
                "",
                "{trace}: the per-user folder was changed"
            );
        }
    }
}

mod recover_empty_root {
    use super::*;

    #[test]
    fn no_install_folder_is_not_the_dot_update_of_the_current_folder() {
        // update_work_directory("") is ".update" under whatever folder the program was started in.
        let s = Scenario::new("recover-empty-root");
        let disk = StdFs;
        let calls = Cell::new(0);
        let watching = ForwardingFs::new(&disk);
        watching.on_stat(|inner, path| {
            calls.set(calls.get() + 1);
            inner.stat(path)
        });
        watching.on_list_directory(|inner, directory| {
            calls.set(calls.get() + 1);
            inner.list_directory(directory)
        });
        watching.on_try_lock(|inner, path| {
            calls.set(calls.get() + 1);
            inner.try_lock(path)
        });
        let launcher = FakeLauncher::default();
        let rig = Rig::new(&s, &watching);

        // recover(): nothing to recover where there is no install.
        assert!(
            detail::recover_with(
                Path::new(""),
                &watching,
                &launcher,
                &s.state,
                SHORT_WAIT,
                ORDINARY_START
            )
            .is_ok()
        );
        assert!(launcher.ran().is_empty());
        // The helper's --recover and a rollback are told a folder explicitly: an empty one is a
        // mistake, and an error.
        let in_process = recover_in_process(Path::new(""), &rig.services());
        assert_eq!(
            in_process.expect_err("no install folder").code,
            ErrorCode::InvalidArgument
        );
        let rolled = rollback(Path::new(""), "deadbeef", &rig.services());
        assert_eq!(
            rolled.expect_err("no install folder").code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(calls.get(), 0, "not one question was asked of the disk");
    }
}

// ---- recover(), the system seams, on Windows -----------------------------------------------------

#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "Edition 2024 marks changing the environment unsafe: the cases that change it hold \
              ENVIRONMENT"
)]
mod recover_on_windows {
    use super::*;
    use katana_core::path_text::environment_variable;
    use katana_update::fsops::{RetryPolicy, make_system_fs_ops};
    use katana_update::process::process_is_elevated;
    use katana_update::recover::recover;
    use std::ffi::OsString;
    use std::sync::{Mutex, MutexGuard, PoisonError};

    // The environment is the process's, and the cases of this program run on threads of one
    // process, where the C++'s ran each in its own: every case here, which reads it through
    // recover(), takes this lock first.
    static ENVIRONMENT: Mutex<()> = Mutex::new(());

    fn environment_lock() -> MutexGuard<'static, ()> {
        ENVIRONMENT.lock().unwrap_or_else(PoisonError::into_inner)
    }

    // Sets the environment variable for the life of the object (the state folder of state.rs). The
    // caller holds environment_lock().
    struct EnvironmentVariable {
        name: &'static str,
        previous: Option<OsString>,
    }

    impl EnvironmentVariable {
        fn new(name: &'static str, value: &Path) -> Self {
            let previous = std::env::var_os(name);
            // SAFETY: the environment is changed only while ENVIRONMENT is held, and the standard
            // library's own lock orders it with every read through std::env.
            unsafe { std::env::set_var(name, value) };
            Self { name, previous }
        }
    }

    impl Drop for EnvironmentVariable {
        fn drop(&mut self) {
            match &self.previous {
                // SAFETY: as in new: the lock is still held (the guard is dropped after this one).
                Some(value) => unsafe { std::env::set_var(self.name, value) },
                // SAFETY: as above.
                None => unsafe { std::env::remove_var(self.name) },
            }
        }
    }

    fn system_program(name: &str) -> PathBuf {
        path_from_utf8(environment_variable(Some("SystemRoot")))
            .join("System32")
            .join(name)
    }

    // A crashed update on the real disk, and a stand-in for the helper that exits as told: cmd.exe
    // (reads nothing from its null input, exits 0) or where.exe (refuses the unknown option, exits
    // 2).
    struct SystemRecovery {
        lp: Loop,
        transaction: String,
    }

    impl SystemRecovery {
        fn new(label: &str) -> Self {
            let mut lp = Loop::new(label, &build_fixture_scenario);
            let p = measure_clean_apply(&mut lp);
            crash_at(&mut lp, p.renames[p.renames.len() / 2], 0);
            let transaction = only_transaction(lp.scenario());
            Self { lp, transaction }
        }
    }

    fn helper_in(folder: &Path, program: &str) {
        make_directories(folder);
        std::fs::copy(system_program(program), folder.join("katana_updater.exe"))
            .expect("the stand-in can be copied");
    }

    // recover() itself asks the system whether THIS process was started as administrator, and such
    // a process never runs the helper copy in the per-user folder (recover.rs). How this run of the
    // suite was started is not a test's to assume (docs/testing.md, "How the suite was started"),
    // so the cases below that put a helper in that folder say what recover() must do for either
    // answer. WHICH helper each answer picks is held through the seam, where a test says the
    // answer: recover_with::a_program_started_as_administrator_runs_the_installs_own_helper...
    fn this_run_was_started_as_administrator() -> bool {
        process_is_elevated()
    }

    #[test]
    fn nothing_interrupted_is_ok_without_running_anything() {
        let _environment = environment_lock();
        let mut s = Scenario::new("recover-sys-nothing");
        s.v1 = tiny("0.1.0", 1);
        write_install(&s.root, &s.v1);
        assert!(recover(&s.root).is_ok(), "no .update");
        write_text(&s.root.join(".update").join("lock"), "");
        assert!(recover(&s.root).is_ok(), "a lock and nothing else");
        assert!(
            recover(&s.scratch.join("no").join("such").join("root")).is_ok(),
            "not even an install"
        );
    }

    #[test]
    fn an_interrupted_update_runs_the_helper_copy_with_recover_and_the_root() {
        let _environment = environment_lock();
        let mut r = SystemRecovery::new("recover-sys-helper");
        let s = r.lp.scenario();
        let _state = EnvironmentVariable::new("KATANA_UPDATE_STATE_DIR", &s.state);
        helper_in(&s.state.join(format!("run-{}", r.transaction)), "cmd.exe"); // exits 0
        let status = recover(&s.root);
        if this_run_was_started_as_administrator() {
            // The copy is the only helper there is, and it is not this process's to run.
            let error = status
                .expect_err("a helper in the per-user folder was run with administrator rights");
            assert_eq!(error.code, ErrorCode::NotFound);
            assert!(error.message.contains("katana_updater --recover"));
        } else {
            assert!(status.is_ok(), "{}", describe(&status));
        }
    }

    #[test]
    fn the_helper_copy_outside_the_install_is_preferred_to_the_installs_own() {
        let _environment = environment_lock();
        {
            let mut r = SystemRecovery::new("recover-sys-prefers-copy");
            let s = r.lp.scenario();
            let _state = EnvironmentVariable::new("KATANA_UPDATE_STATE_DIR", &s.state);
            helper_in(&s.state.join(format!("run-{}", r.transaction)), "cmd.exe"); // exits 0
            helper_in(&s.root.join("bin"), "where.exe"); // exits 2
            let status = recover(&s.root);
            if this_run_was_started_as_administrator() {
                // The other way round there: the install's own ran, and that one fails.
                let error = status
                    .expect_err("the copy in the per-user folder ran with administrator rights");
                assert_eq!(error.code, ErrorCode::InvalidState, "{}", error.describe());
            } else {
                assert!(status.is_ok(), "the copy ran, not bin/katana_updater");
            }
        }
        {
            let mut r = SystemRecovery::new("recover-sys-falls-back");
            let s = r.lp.scenario();
            let _state = EnvironmentVariable::new("KATANA_UPDATE_STATE_DIR", &s.state);
            helper_in(&s.root.join("bin"), "where.exe");
            let status = recover(&s.root);
            let error =
                status.expect_err("with no copy the install's own helper ran, and it failed");
            assert!(error.message.contains("katana_updater --recover"));
            assert!(error.message.contains(&text(&s.root)));
        }
    }

    #[test]
    fn a_helper_that_fails_or_is_missing_is_an_error_that_names_the_command_to_run_by_hand() {
        let _environment = environment_lock();
        {
            let mut r = SystemRecovery::new("recover-sys-fails");
            let s = r.lp.scenario();
            let _state = EnvironmentVariable::new("KATANA_UPDATE_STATE_DIR", &s.state);
            helper_in(&s.state.join(format!("run-{}", r.transaction)), "where.exe");
            let status = recover(&s.root);
            let error = status.expect_err("the helper failed");
            assert!(
                error.message.contains("katana_updater --recover \""),
                "{}",
                error.describe()
            );
        }
        {
            let mut r = SystemRecovery::new("recover-sys-missing");
            let s = r.lp.scenario();
            let _state = EnvironmentVariable::new("KATANA_UPDATE_STATE_DIR", &s.state);
            let status = recover(&s.root);
            let error = status.expect_err("there is no helper");
            assert_eq!(error.code, ErrorCode::NotFound);
            assert!(error.message.contains("katana_updater --recover"));
        }
    }

    #[test]
    fn a_live_holder_of_the_lock_is_waited_for_before_the_helper_is_run() {
        let _environment = environment_lock();
        let mut r = SystemRecovery::new("recover-sys-lock");
        let s = r.lp.scenario();
        let _state = EnvironmentVariable::new("KATANA_UPDATE_STATE_DIR", &s.state);
        // The same stand-in in both places a helper is looked for, so that the wait is what is
        // measured whichever of them this process may run (above).
        helper_in(&s.state.join(format!("run-{}", r.transaction)), "cmd.exe");
        helper_in(&s.root.join("bin"), "cmd.exe");
        let disk = make_system_fs_ops(&RetryPolicy::default());
        let holder = disk.try_lock(&s.root.join(".update").join("lock"));
        assert!(holder.is_ok());
        // The C++ lets go of the lock on another thread; the lock of the system's file system is
        // not Send, so here the recovery is the other thread, and this one lets go of the lock
        // 400 ms after it started.
        let root = s.root.as_path();
        let (status, took) = std::thread::scope(|scope| {
            let recovering = scope.spawn(move || {
                let start = Instant::now();
                let status = recover(root);
                (status, start.elapsed())
            });
            std::thread::sleep(Duration::from_millis(400));
            drop(holder);
            recovering.join().expect("the recovery returned")
        });
        assert!(status.is_ok(), "{}", describe(&status));
        assert!(
            took >= Duration::from_millis(350),
            "it waited for the holder first"
        );
    }
}
