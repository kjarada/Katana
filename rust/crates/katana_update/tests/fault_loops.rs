//! The deterministic fault injection of the update (docs/update.md): FaultFs, and the three loops
//! that the swap's safety rests on. The twin of tests/update/test_fault_loops.cpp.
//!
//! ```text
//! 1. The apply of a 40-file release is crashed before EVERY one of its file system operations;
//!    then recover() runs on what the crash left, and the whole tree must hash to the old
//!    version's or the new one's - never a mix.
//! 2. The same, with the crash INSIDE the recovery, at every one of its operations, from several
//!    kinds of interrupted state (and again inside the second recovery).
//! 3. An error 32 (a locked file) injected at every rename of the commit: the update must end
//!    rolled back, and the tree must be the old one exactly.
//! ```
//!
//! The expectation of each loop is not read from the swap. The two hashes are of trees built from
//! the specification of the releases, in their own folders (apply_harness). A loop that crashed
//! nowhere would pass trivially, so each shows that it reached what it claims: every k crashed, at
//! the operation numbered k, and the crashes cover every state the journal can be in.
//!
//! A crash is a panic whose payload is a SimulatedCrash (fault_fs.rs), taken back with
//! catch_unwind; the harness's quiet_simulated_crashes keeps the hundreds of them a loop raises out
//! of the output. What the C++'s threads report with ADD_FAILURE is collected and checked once they
//! have all stopped (Failures).

pub mod test_key;

pub mod apply_harness;

use apply_harness::{
    CountingFs, LogFs, Loop, Rig, Scenario, Scratch, StdFs, apply_until_crash,
    build_fixture_scenario, build_forty_file_scenario, difference, is_present, journal_state_of,
    leftovers_in_update, quiet_simulated_crashes, read_text, recover_clean, recover_until_crash,
    replace_tree, snapshot, state_name, tree_hash, write_text,
};
use katana_core::{Error, Status};
use katana_update::apply::{ApplyOutcome, ApplyResult};
use katana_update::fault_fs::{FaultFs, SimulatedCrash};
use katana_update::fsops::FsOps;
use katana_update::journal::{JournalState, needs_recovery};
use katana_update::state::read_last_result;
use std::collections::BTreeMap;
use std::num::NonZeroUsize;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};

// Runs `work`, and gives back the SimulatedCrash it died of, if it did; any other panic goes on.
fn crash_of<T>(work: impl FnOnce() -> T) -> Result<T, SimulatedCrash> {
    quiet_simulated_crashes();
    match catch_unwind(AssertUnwindSafe(work)) {
        Ok(value) => Ok(value),
        Err(payload) => match payload.downcast::<SimulatedCrash>() {
            Ok(crash) => Err(*crash),
            Err(other) => resume_unwind(other),
        },
    }
}

// ---- FaultFs itself ------------------------------------------------------------------------------

mod fault_fs {
    use super::*;

    #[test]
    fn the_eight_changing_operations_are_counted_and_nothing_else_is() {
        let scratch = Scratch::new("faultfs-count");
        let disk = StdFs;
        let faults = FaultFs::new(&disk);
        let dir = scratch.join("d");
        write_text(&scratch.join("existing"), "x");

        // Looking is not counted.
        let _ = faults.stat(&scratch.join("existing"));
        let _ = faults.list_directory(scratch.path());
        let _ = faults.read_file(&scratch.join("existing"));
        let _ = faults.resolve(scratch.path());
        let _ = faults.free_space(scratch.path());
        let _ = faults.sync_directory(scratch.path());
        {
            let lock = faults.try_lock(&scratch.join("lock"));
            assert!(lock.is_ok());
        }
        assert_eq!(faults.mutating_operations(), 0);

        // The eight that change.
        assert!(faults.make_directories(&dir).is_ok());
        assert_eq!(faults.mutating_operations(), 1);
        assert!(faults.write_durable(&dir.join("a"), b"A").is_ok());
        assert_eq!(faults.mutating_operations(), 2);
        assert!(faults.append_durable(&dir.join("a"), b"B").is_ok());
        assert_eq!(faults.mutating_operations(), 3);
        assert!(faults.copy_file(&dir.join("a"), &dir.join("b")).is_ok());
        assert_eq!(faults.mutating_operations(), 4);
        assert!(faults.rename_file(&dir.join("b"), &dir.join("c")).is_ok());
        assert_eq!(faults.mutating_operations(), 5);
        assert!(faults.set_executable(&dir.join("c")).is_ok());
        assert_eq!(faults.mutating_operations(), 6);
        assert!(faults.remove_file(&dir.join("c")).is_ok());
        assert_eq!(faults.mutating_operations(), 7);
        assert!(
            faults
                .remove_empty_directory(&scratch.join("nothing-here"))
                .is_ok()
        );
        assert_eq!(faults.mutating_operations(), 8);
        assert_eq!(read_text(&dir.join("a")), b"AB", "and each did its work");
    }

    #[test]
    fn a_crash_before_the_kth_operation_performs_neither_it_nor_anything_after_and_keeps_what_came_before()
     {
        let scratch = Scratch::new("faultfs-crash");
        let disk = StdFs;
        let faults = FaultFs::new(&disk);
        faults.crash_before(3);
        assert!(faults.make_directories(&scratch.join("d")).is_ok()); // 1
        assert!(
            faults
                .write_durable(&scratch.join("d").join("a"), b"A")
                .is_ok()
        ); // 2
        match crash_of(|| faults.write_durable(&scratch.join("d").join("b"), b"B")) {
            // 3: dies here
            Ok(_) => panic!("no crash"),
            Err(crash) => {
                assert_eq!(crash.operation, 3);
                assert!(
                    crash.description.contains("writeDurable"),
                    "{}",
                    crash.description
                );
                assert!(crash.description.contains('b'));
            }
        }
        assert_eq!(
            faults.mutating_operations(),
            2,
            "the one that crashed is not counted as performed"
        );
        assert!(
            is_present(&scratch.join("d").join("a")),
            "earlier work stays on disk"
        );
        assert!(
            !is_present(&scratch.join("d").join("b")),
            "the crashed operation did not happen"
        );
    }

    #[test]
    fn every_kind_of_change_can_be_the_one_that_crashes() {
        let scratch = Scratch::new("faultfs-kinds");
        let disk = StdFs;
        write_text(&scratch.join("from"), "F");
        std::fs::create_dir_all(scratch.join("emptydir")).expect("the folder can be made");
        // One mutating operation by its C++ name, and how to make it.
        type Operation = (&'static str, fn(&FaultFs<'_>, &Path));
        let operations: [Operation; 8] = [
            ("makeDirectories", |f, base| {
                let _ = f.make_directories(&base.join("newdir"));
            }),
            ("copyFile", |f, base| {
                let _ = f.copy_file(&base.join("from"), &base.join("copy"));
            }),
            ("renameFile", |f, base| {
                let _ = f.rename_file(&base.join("from"), &base.join("moved"));
            }),
            ("removeFile", |f, base| {
                let _ = f.remove_file(&base.join("from"));
            }),
            ("removeEmptyDirectory", |f, base| {
                let _ = f.remove_empty_directory(&base.join("emptydir"));
            }),
            ("appendDurable", |f, base| {
                let _ = f.append_durable(&base.join("from"), b"more");
            }),
            ("writeDurable", |f, base| {
                let _ = f.write_durable(&base.join("from"), b"over");
            }),
            ("setExecutable", |f, base| {
                let _ = f.set_executable(&base.join("from"));
            }),
        ];
        for (name, operation) in operations {
            let faults = FaultFs::new(&disk);
            faults.crash_before(1);
            match crash_of(|| operation(&faults, scratch.path())) {
                Ok(()) => panic!("{name} did not crash"),
                Err(crash) => assert!(crash.description.starts_with(name), "{}", crash.description),
            }
        }
        // Nothing happened to the disk in any of them.
        assert_eq!(read_text(&scratch.join("from")), b"F");
        assert!(!is_present(&scratch.join("newdir")));
        assert!(!is_present(&scratch.join("copy")));
        assert!(!is_present(&scratch.join("moved")));
        assert!(scratch.join("emptydir").is_dir());
    }

    #[test]
    fn a_crash_is_not_an_error_a_caller_could_catch_and_carry_on() {
        // The C++ asserts at compile time that a SimulatedCrash is no std::exception and does not
        // convert to an Error, so that a catch (const std::exception&) cannot swallow a simulated
        // death. Here the operation that crashes returns no Status a caller could take with `?`,
        // and what it raises is a panic whose payload is the crash - not an Error, and not the
        // message of an ordinary panic, which is what the library's catch of the unexpected turns
        // into an error (apply.rs, catch_unexpected) and what a caller could catch and carry on
        // from.
        let scratch = Scratch::new("faultfs-uncatchable");
        let disk = StdFs;
        let faults = FaultFs::new(&disk);
        faults.crash_before(1);
        quiet_simulated_crashes();
        let mut swallowed = false;
        let mut reached = false;
        match catch_unwind(AssertUnwindSafe(|| {
            faults.make_directories(&scratch.join("d"))
        })) {
            // A Status came back: a caller could carry on from it.
            Ok(_) => swallowed = true,
            Err(payload) => {
                if payload.is::<Error>() || payload.is::<String>() || payload.is::<&str>() {
                    swallowed = true;
                }
                reached = payload.is::<SimulatedCrash>();
            }
        }
        assert!(!swallowed);
        assert!(reached);
    }

    #[test]
    fn the_kth_rename_fails_with_the_os_error_and_is_not_performed() {
        let scratch = Scratch::new("faultfs-rename");
        let disk = StdFs;
        let faults = FaultFs::new(&disk);
        faults.fail_rename_with(2, 32);
        write_text(&scratch.join("a"), "A");
        write_text(&scratch.join("b"), "B");
        write_text(&scratch.join("c"), "C");

        // the 1st
        assert!(
            faults
                .rename_file(&scratch.join("a"), &scratch.join("a2"))
                .is_ok()
        );
        // Other operations in between are not renames and do not count as one.
        assert!(faults.write_durable(&scratch.join("w"), b"w").is_ok());
        // the 2nd
        let failed = faults.rename_file(&scratch.join("b"), &scratch.join("b2"));
        let error = failed.expect_err("the second rename fails");
        assert!(
            error.context.starts_with("os_error=32"),
            "{}",
            error.describe()
        );
        assert!(is_present(&scratch.join("b")), "not performed");
        assert!(!is_present(&scratch.join("b2")));
        // the 3rd
        assert!(
            faults
                .rename_file(&scratch.join("c"), &scratch.join("c2"))
                .is_ok()
        );
        assert!(is_present(&scratch.join("c2")));
        // The failed rename was an operation like the others for the count of operations.
        assert_eq!(faults.mutating_operations(), 4);
    }

    #[test]
    fn another_os_error_number_is_carried_through() {
        let scratch = Scratch::new("faultfs-rename-5");
        let disk = StdFs;
        let faults = FaultFs::new(&disk);
        faults.fail_rename_with(1, 5);
        write_text(&scratch.join("a"), "A");
        let failed = faults.rename_file(&scratch.join("a"), &scratch.join("z"));
        let error = failed.expect_err("the rename fails");
        assert!(error.context.starts_with("os_error=5 "));
    }

    #[test]
    fn reset_clears_both_settings_and_the_counters() {
        let scratch = Scratch::new("faultfs-reset");
        let disk = StdFs;
        let faults = FaultFs::new(&disk);
        faults.crash_before(2);
        faults.fail_rename_with(1, 32);
        assert!(faults.make_directories(&scratch.join("d")).is_ok());
        assert_eq!(faults.mutating_operations(), 1);
        faults.reset();
        assert_eq!(faults.mutating_operations(), 0);
        write_text(&scratch.join("a"), "A");
        assert!(
            faults
                .write_durable(&scratch.join("d").join("x"), b"x")
                .is_ok(),
            "no crash after the reset"
        );
        assert!(
            faults
                .rename_file(&scratch.join("a"), &scratch.join("b"))
                .is_ok(),
            "no failure after the reset"
        );
    }

    #[test]
    fn a_log_above_it_numbers_the_operations_the_way_it_does() {
        // The loops name "the k-th operation" from a LogFs over a FaultFs: the two must agree.
        let scratch = Scratch::new("faultfs-log");
        let disk = StdFs;
        let faults = FaultFs::new(&disk);
        let log = LogFs::new(&faults);
        write_text(&scratch.join("a"), "A");
        let d = scratch.join("d");
        assert!(log.make_directories(&d).is_ok());
        assert!(log.copy_file(&scratch.join("a"), &d.join("b")).is_ok());
        assert!(log.rename_file(&d.join("b"), &d.join("c")).is_ok());
        assert!(log.set_executable(&d.join("c")).is_ok());
        assert!(log.append_durable(&d.join("j"), b"r").is_ok());
        assert!(log.write_durable(&d.join("w"), b"w").is_ok());
        assert!(log.remove_file(&d.join("c")).is_ok());
        assert!(log.remove_empty_directory(&scratch.join("d2")).is_ok());
        assert_eq!(log.log.borrow().len() as u64, faults.mutating_operations());
        assert_eq!(log.find("renameFile", "", 1), 3);
        assert_eq!(log.find("appendDurable", "r", 1), 5);
    }
}

// ---- running a loop on several threads -----------------------------------------------------------

fn worker_count() -> usize {
    std::thread::available_parallelism()
        .map_or(2, NonZeroUsize::get)
        .clamp(2, 8)
}

// How a loop's scenario is built: shared by the threads, each of which builds its own from it.
type Build<'a> = &'a (dyn Fn(&mut Scenario) + Sync);

// What the C++'s ADD_FAILURE records from the threads of a loop: the messages, read once every
// thread has stopped.
#[derive(Default)]
struct Failures(Mutex<Vec<String>>);

impl Failures {
    fn add(&self, message: String) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(message);
    }

    fn check(self) {
        let messages = self.0.into_inner().unwrap_or_else(PoisonError::into_inner);
        assert!(messages.is_empty(), "{}", messages.join("\n"));
    }
}

// Runs body(loop, item, state) for item = first, first + stride, ... up to last, spread over
// several threads. Each thread has a scenario of its own - its own install, stage and state
// folders, built from the same specification, so every iteration starts from the same bytes - and
// a state object of the caller's; nothing is shared but the counter that hands out the next item
// (and the locks StdFs holds, which are per path). The iterations are independent by
// construction, which is what lets the several hundred of a loop take a minute and not five.
// `body` returns false after a failure and everyone stops; a body that panics stops everyone too,
// and its panic is the case's.
fn for_each_item<State, Body>(
    label: &str,
    build: Build<'_>,
    first: u64,
    last: u64,
    stride: u64,
    states: &mut [State],
    body: Body,
) where
    State: Send,
    Body: Fn(&mut Loop, u64, &mut State) -> bool + Sync,
{
    let next = AtomicU64::new(first);
    let failed = AtomicBool::new(false);
    let (next, failed, body) = (&next, &failed, &body);
    std::thread::scope(|scope| {
        for (worker, state) in states.iter_mut().enumerate() {
            scope.spawn(move || {
                let mut lp = Loop::new(&format!("{label}-{worker}"), build);
                while !failed.load(Ordering::SeqCst) {
                    let item = next.fetch_add(stride, Ordering::SeqCst);
                    if item > last {
                        return;
                    }
                    let went = catch_unwind(AssertUnwindSafe(|| body(&mut lp, item, &mut *state)));
                    match went {
                        Ok(true) => {}
                        Ok(false) => failed.store(true, Ordering::SeqCst),
                        Err(payload) => {
                            failed.store(true, Ordering::SeqCst);
                            resume_unwind(payload);
                        }
                    }
                }
            });
        }
    });
}

// ---- loop 1: crash before every operation --------------------------------------------------------

#[derive(Clone, Debug, Default)]
struct CrashCounts {
    by_state: BTreeMap<String, i32>, // what the journal said at each crash
    crashes: u64,
    old: u64,
    fresh: u64,
}

// Crashes the apply before operation k, for every k, and requires recover() to leave the old tree
// or the new one - and, the strong form, the one the clean run's log of operations says: the old
// one unless the crash came after the COMMITTED_PENDING_HEALTH record was written.
fn crash_loop(label: &str, build: Build<'_>, stride: u64, minimum_operations: u64) {
    let mut sizing = Loop::new(&format!("{label}-sizing"), build);
    {
        let s = sizing.scenario();
        assert_ne!(s.v1_hash, s.v2_hash);
    }

    // Size the loop with one apply that has nothing wrong with it. It also proves the expectation:
    // the update, run to its end, leaves the tree built from the specification.
    let (total, pending_record) = {
        sizing.reset();
        let s = sizing.scenario();
        let disk = StdFs;
        let counting = FaultFs::new(&disk);
        let log = LogFs::new(&counting);
        let mut result = ApplyResult::default();
        assert!(apply_until_crash(s, &log, Some(&mut result)).is_none());
        assert_eq!(
            result.outcome,
            ApplyOutcome::Committed,
            "{}: {}",
            result.reason,
            result.detail
        );
        let total = counting.mutating_operations();
        assert_eq!(log.log.borrow().len() as u64, total);
        // the operation that writes COMMITTED_PENDING_HEALTH
        let pending_record = log.find("appendDurable", "COMMITTED_PENDING_HEALTH", 1);
        assert!(pending_record > 0);
        assert_eq!(
            tree_hash(&s.root),
            s.v2_hash,
            "{}",
            difference(&snapshot(&s.expected), &snapshot(&s.root))
        );
        (total, pending_record)
    };
    assert!(
        total >= minimum_operations,
        "an apply of this size does more than that"
    );

    let failures = Failures::default();
    let mut counts: Vec<CrashCounts> = (0..worker_count())
        .map(|_| CrashCounts::default())
        .collect();
    for_each_item(
        label,
        build,
        1,
        total,
        stride,
        &mut counts,
        |lp: &mut Loop, k: u64, c: &mut CrashCounts| -> bool {
            lp.reset();
            let t = lp.scenario();
            let disk = StdFs;
            let faults = FaultFs::new(&disk);
            faults.crash_before(k);
            let Some(crash) = apply_until_crash(t, &faults, None) else {
                failures.add(format!("no crash before operation {k} of {total}"));
                return false;
            };
            c.crashes += 1;
            if crash.operation != k || faults.mutating_operations() != k - 1 {
                failures.add(format!(
                    "the crash was at operation {}, not {k}",
                    crash.operation
                ));
                return false;
            }
            let state = journal_state_of(&t.root);
            *c.by_state
                .entry(String::from(state_name(state)))
                .or_default() += 1;

            let recovered = recover_clean(t);
            if let Err(error) = &recovered {
                failures.add(format!(
                    "recovery failed after the crash before operation {k} ({}): {}",
                    crash.description,
                    error.describe()
                ));
                return false;
            }
            let hash = tree_hash(&t.root);
            // Operations before k were performed; the pending record is one of them or it is not.
            let committed = k > pending_record;
            if hash != t.v1_hash && hash != t.v2_hash {
                failures.add(format!(
                    "after the crash before operation {k} ({}, journal {}) the tree is neither \
                     version:\nversus the old: {}versus the new: {}",
                    crash.description,
                    state_name(state),
                    difference(&t.v1_snapshot, &snapshot(&t.root)),
                    difference(&snapshot(&t.expected), &snapshot(&t.root))
                ));
                return false;
            }
            let wanted = if committed { &t.v2_hash } else { &t.v1_hash };
            if hash != *wanted {
                failures.add(format!(
                    "after the crash before operation {k} ({}, journal {}) the tree is the {} \
                     version, not the {} one",
                    crash.description,
                    state_name(state),
                    if committed { "old" } else { "new" },
                    if committed { "new" } else { "old" }
                ));
                return false;
            }
            if hash == t.v1_hash {
                c.old += 1;
            } else {
                c.fresh += 1;
            }

            // Recovery is idempotent: again changes nothing, and does not even write.
            let clean = StdFs;
            let counting = FaultFs::new(&clean);
            let mut again: Status = Ok(());
            let crashed = recover_until_crash(t, &counting, Some(&mut again)).is_some();
            if crashed
                || again.is_err()
                || counting.mutating_operations() != 0
                || tree_hash(&t.root) != hash
            {
                failures.add(format!(
                    "a second recovery found work, or changed the tree, at k={k}"
                ));
                return false;
            }
            if state != JournalState::CommittedPendingHealth
                && !leftovers_in_update(&t.root).is_empty()
            {
                failures.add(format!(
                    "only a transaction waiting for its health check keeps its folder (k={k})"
                ));
                return false;
            }
            true
        },
    );
    failures.check();

    // Past the last operation there is nothing to crash before: the apply finishes.
    {
        sizing.reset();
        let s = sizing.scenario();
        let disk = StdFs;
        let faults = FaultFs::new(&disk);
        faults.crash_before(total + 1);
        let mut result = ApplyResult::default();
        assert!(apply_until_crash(s, &faults, Some(&mut result)).is_none());
        assert_eq!(result.outcome, ApplyOutcome::Committed);
        assert_eq!(faults.mutating_operations(), total);
    }

    let mut all = CrashCounts::default();
    for c in &counts {
        all.crashes += c.crashes;
        all.old += c.old;
        all.fresh += c.fresh;
        for (state, n) in &c.by_state {
            *all.by_state.entry(state.clone()).or_default() += n;
        }
    }
    let reached = |state: &str| all.by_state.get(state).copied().unwrap_or_default();
    // The loop reached what it claims.
    let expected_crashes = total.div_ceil(stride);
    assert_eq!(all.crashes, expected_crashes, "a crash at every k");
    assert_eq!(all.old + all.fresh, all.crashes);
    assert!(all.old > 0, "some crashes ended at the old tree");
    assert!(all.fresh > 0, "and some at the new one");
    for state in ["Empty", "Committing", "Healthy"] {
        assert!(reached(state) > 0, "no crash left the journal {state}");
    }
    if stride == 1 {
        assert!(reached("Planned") > 0);
        assert!(reached("CommittedPendingHealth") > 0);
    }
    let states: Vec<String> = all
        .by_state
        .iter()
        .map(|(state, n)| format!(" {state}={n}"))
        .collect();
    println!(
        "  {label}: {total} operations, {} crashes, {} ended at the old tree and {} at the new; \
         journal states:{}",
        all.crashes,
        all.old,
        all.fresh,
        states.concat()
    );
}

mod fault_loops {
    use super::*;

    #[test]
    fn crashing_before_every_operation_of_a_forty_file_update_leaves_the_old_tree_or_the_new_never_a_mix()
     {
        crate::require_can_sign!();
        crash_loop("loop-crash-forty", &build_forty_file_scenario, 1, 200);
    }

    #[test]
    fn the_same_loop_over_the_fixture_releases_made_by_the_producer() {
        crash_loop("loop-crash-fixture", &build_fixture_scenario, 1, 50);
    }

    #[test]
    fn crashing_inside_recovery_at_every_operation_still_ends_at_the_old_tree() {
        crate::require_can_sign!();
        // The half-way state of the big update: the largest recovery there is. All six states are
        // run, with every operation, over the smaller update below; what a recovery does does not
        // depend on how many files there are.
        recovery_loop("loop-recovery-forty", &build_forty_file_scenario, 16, &[2]);
    }

    #[test]
    fn the_recovery_loop_over_the_fixture_releases() {
        recovery_loop("loop-recovery-fixture", &build_fixture_scenario, 8, &[]);
    }

    #[test]
    fn an_error_thirty_two_at_every_rename_of_a_forty_file_update_ends_rolled_back_to_exactly_the_old_tree()
     {
        crate::require_can_sign!();
        rename_failure_loop("loop-error-forty", &build_forty_file_scenario, 36);
    }

    #[test]
    fn the_same_loop_over_the_fixture_releases() {
        rename_failure_loop("loop-error-fixture", &build_fixture_scenario, 12);
    }
}

// ---- loop 2: crash inside recovery ---------------------------------------------------------------

#[derive(Clone, Debug)]
struct FirstFault {
    name: String,
    crash_before: u64, // the apply's own operation to die before (0: none)
    fail_rename: u64,  // the commit's rename to fail with error 32 (0: none)
}

impl FirstFault {
    fn new(name: &str, crash_before: u64, fail_rename: u64) -> Self {
        Self {
            name: String::from(name),
            crash_before,
            fail_rename,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct RecoveryItem {
    point: usize,
    operation: u64, // the recovery operation to die before
}

#[derive(Clone, Debug)]
struct RecoveryState {
    built_for: usize, // the point whose crashed state is kept
    crashed: PathBuf,
    crashes_in_recovery: u64,
}

impl Default for RecoveryState {
    fn default() -> Self {
        Self {
            built_for: usize::MAX,
            crashed: PathBuf::new(),
            crashes_in_recovery: 0,
        }
    }
}

// Brings the loop's scenario to the state the first death of `point` leaves, and keeps a copy.
fn build_crashed_state(lp: &mut Loop, point: &FirstFault, keep: &Path) -> Result<(), String> {
    lp.reset();
    let s = lp.scenario();
    let bottom = StdFs;
    let faults = FaultFs::new(&bottom);
    faults.crash_before(point.crash_before);
    if point.fail_rename != 0 {
        faults.fail_rename_with(point.fail_rename, 32);
    }
    if apply_until_crash(s, &faults, None).is_none() {
        return Err(format!("the apply did not die at {}", point.name));
    }
    replace_tree(&s.root, keep);
    Ok(())
}

// `only` names the interrupted states to run (indices into the list below); empty: all six.
fn recovery_loop(label: &str, build: Build<'_>, second_level_stride: u64, only: &[usize]) {
    let mut sizing = Loop::new(&format!("{label}-sizing"), build);

    // Name the apply's operations from a log of one that goes right. The log is above the FaultFs,
    // the disk below it, so every operation the FaultFs counts has an entry.
    let mut points: Vec<FirstFault> = Vec::new();
    {
        sizing.reset();
        let s = sizing.scenario();
        let bottom = StdFs;
        let faults = FaultFs::new(&bottom);
        let log = LogFs::new(&faults);
        let mut result = ApplyResult::default();
        assert!(apply_until_crash(s, &log, Some(&mut result)).is_none());
        assert_eq!(result.outcome, ApplyOutcome::Committed);
        let committing = log.find("appendDurable", "COMMITTING", 1);
        let pending = log.find("appendDurable", "COMMITTED_PENDING_HEALTH", 1);
        let renames = log.all("renameFile");
        assert!(committing > 0);
        assert!(pending > committing);
        assert!(renames.len() >= 10);
        points.push(FirstFault::new(
            "before COMMITTING is written (state Planned)",
            committing,
            0,
        ));
        points.push(FirstFault::new("before the first rename", renames[0], 0));
        points.push(FirstFault::new(
            "half way through the renames",
            renames[renames.len() / 2],
            0,
        ));
        points.push(FirstFault::new(
            "after the last rename",
            renames[renames.len() - 1] + 1,
            0,
        ));
        points.push(FirstFault::new(
            "before COMMITTED_PENDING_HEALTH is written",
            pending,
            0,
        ));
    }
    {
        // A rename that fails makes the apply roll itself back; die a few operations into THAT.
        sizing.reset();
        let s = sizing.scenario();
        let bottom = StdFs;
        let faults = FaultFs::new(&bottom);
        faults.fail_rename_with(7, 32);
        let log = LogFs::new(&faults);
        let mut result = ApplyResult::default();
        assert!(apply_until_crash(s, &log, Some(&mut result)).is_none());
        assert_eq!(result.outcome, ApplyOutcome::RolledBack);
        let rolling = log.find("appendDurable", "ROLLING_BACK", 1);
        assert!(rolling > 0);
        assert!(
            log.log.borrow().len() as u64 > rolling + 6,
            "the rollback has operations to die in"
        );
        points.push(FirstFault::new(
            "inside the apply's own rollback",
            rolling + 5,
            7,
        ));
    }

    // How many operations a recovery from each of those states has.
    let mut items: Vec<RecoveryItem> = Vec::new();
    for (i, point) in points.iter().enumerate() {
        if !only.is_empty() && !only.contains(&i) {
            continue;
        }
        let name = &point.name;
        let keep = sizing.scenario().scratch.join("crashed-state");
        if let Err(message) = build_crashed_state(&mut sizing, point, &keep) {
            panic!("{name}: {message}");
        }
        let s = sizing.scenario();
        // Whatever the files look like - after the last rename they are the new version's - the
        // journal has not said the update is complete, so recovery must put the old back.
        let state = journal_state_of(&s.root);
        assert!(
            needs_recovery(state) || state == JournalState::Empty,
            "{name}: the crash left a journal that needs nothing"
        );
        let bottom = StdFs;
        let counting = FaultFs::new(&bottom);
        let mut status: Status = Ok(());
        assert!(
            recover_until_crash(s, &counting, Some(&mut status)).is_none(),
            "{name}"
        );
        assert!(
            status.is_ok(),
            "{name}: {}",
            status
                .as_ref()
                .err()
                .map(Error::describe)
                .unwrap_or_default()
        );
        assert_eq!(
            tree_hash(&s.root),
            s.v1_hash,
            "{name}: {}",
            difference(&s.v1_snapshot, &snapshot(&s.root))
        );
        let operations = counting.mutating_operations();
        assert!(operations > 10, "{name}");
        for j in 1..=operations {
            items.push(RecoveryItem {
                point: i,
                operation: j,
            });
        }
    }

    let failures = Failures::default();
    let mut states: Vec<RecoveryState> = (0..worker_count())
        .map(|_| RecoveryState::default())
        .collect();
    for_each_item(
        label,
        build,
        1,
        items.len() as u64,
        1,
        &mut states,
        |lp: &mut Loop, index: u64, state: &mut RecoveryState| -> bool {
            let item = items[(index - 1) as usize];
            let point = &points[item.point];
            if state.built_for != item.point {
                state.crashed = lp.scenario().scratch.join("crashed-state");
                if let Err(message) = build_crashed_state(lp, point, &state.crashed) {
                    failures.add(message);
                    return false;
                }
                state.built_for = item.point;
            }
            let j = item.operation;
            let t = lp.scenario();
            replace_tree(&state.crashed, &t.root);
            let bottom = StdFs;
            let faults = FaultFs::new(&bottom);
            faults.crash_before(j);
            let crash = recover_until_crash(t, &faults, None);
            let Some(crash) = crash.filter(|crash| crash.operation == j) else {
                failures.add(format!(
                    "{}: recovery did not crash before its operation {j}",
                    point.name
                ));
                return false;
            };
            state.crashes_in_recovery += 1;

            // A second death, inside the second recovery (at a few of its operations, from a
            // sample of the first): the third is the one that finishes.
            if j.is_multiple_of(second_level_stride) {
                for i in [1u64, 4, 17] {
                    let between = t.scratch.join("between");
                    replace_tree(&t.root, &between);
                    let again = StdFs;
                    let second = FaultFs::new(&again);
                    second.crash_before(i);
                    let _ = recover_until_crash(t, &second, None);
                    let finished = recover_clean(t);
                    if finished.is_err() || tree_hash(&t.root) != t.v1_hash {
                        failures.add(format!(
                            "{}: after deaths at recovery operations {j} and {i} the tree is not \
                             the old version: {}",
                            point.name,
                            difference(&t.v1_snapshot, &snapshot(&t.root))
                        ));
                        return false;
                    }
                    replace_tree(&between, &t.root);
                }
            }

            let finished = recover_clean(t);
            if let Err(error) = &finished {
                failures.add(format!(
                    "{}: recovery after dying at its operation {j} ({}) failed: {}",
                    point.name,
                    crash.description,
                    error.describe()
                ));
                return false;
            }
            if tree_hash(&t.root) != t.v1_hash {
                failures.add(format!(
                    "{}: after dying at recovery operation {j} ({}) the tree is not the old \
                     version:\n{}",
                    point.name,
                    crash.description,
                    difference(&t.v1_snapshot, &snapshot(&t.root))
                ));
                return false;
            }
            if !leftovers_in_update(&t.root).is_empty() {
                failures.add(format!(
                    "{}: a folder is left after recovery operation {j}",
                    point.name
                ));
                return false;
            }
            true
        },
    );
    failures.check();

    let crashes: u64 = states.iter().map(|state| state.crashes_in_recovery).sum();
    assert_eq!(
        crashes,
        items.len() as u64,
        "a crash at every operation of every recovery"
    );
    println!(
        "  {label}: {} interrupted states, {crashes} crashes inside recovery",
        if only.is_empty() {
            points.len()
        } else {
            only.len()
        }
    );
}

// ---- loop 3: a locked file at every rename -------------------------------------------------------

#[derive(Clone, Debug, Default)]
struct RenameCounts {
    rolled_back: u64,
}

fn rename_failure_loop(label: &str, build: Build<'_>, minimum_renames: u64) {
    let mut sizing = Loop::new(&format!("{label}-sizing"), build);

    // How many renames a good update makes.
    let renames = {
        sizing.reset();
        let s = sizing.scenario();
        let disk = StdFs;
        let counting = CountingFs::new(&disk);
        let rig = Rig::new(s, &counting);
        assert_eq!(rig.run().outcome, ApplyOutcome::Committed);
        counting.renames.get()
    };
    assert!(renames >= minimum_renames);

    let stage_pristine = tree_hash(&sizing.pristine_root().join("stage"));
    let failures = Failures::default();
    let mut counts: Vec<RenameCounts> = (0..worker_count())
        .map(|_| RenameCounts::default())
        .collect();
    for_each_item(
        label,
        build,
        1,
        renames,
        1,
        &mut counts,
        |lp: &mut Loop, k: u64, c: &mut RenameCounts| -> bool {
            lp.reset();
            let t = lp.scenario();
            let disk = StdFs;
            let faults = FaultFs::new(&disk);
            faults.fail_rename_with(k, 32);
            let rig = Rig::new(t, &faults);
            let result = rig.run();
            if result.outcome != ApplyOutcome::RolledBack || result.reason != "rename_failed" {
                failures.add(format!(
                    "error 32 at rename {k} of {renames} ended {} / {}: {}",
                    katana_update::apply::to_string(result.outcome),
                    result.reason,
                    result.detail
                ));
                return false;
            }
            c.rolled_back += 1;
            if tree_hash(&t.root) != t.v1_hash {
                failures.add(format!(
                    "after error 32 at rename {k} the tree is not the old version:\n{}",
                    difference(&t.v1_snapshot, &snapshot(&t.root))
                ));
                return false;
            }
            // The C++'s EXPECTs: a failure each, and the loop goes on.
            if !leftovers_in_update(&t.root).is_empty() {
                failures.add(format!("k={k}"));
            }
            if tree_hash(&t.stage) != stage_pristine {
                failures.add(format!("the stage is untouched, for another try (k={k})"));
            }
            if rig.launcher.started().len() != 1 {
                failures.add(format!("the old version is started again (k={k})"));
            }
            if !rig.launcher.ran().is_empty() {
                failures.add(String::from(
                    "no health check of an update that did not happen",
                ));
            }
            if !rig.registry.run_once.borrow().is_empty() {
                failures.add(format!("the RunOnce entry is gone (k={k})"));
            }
            let last = match read_last_result(&t.state) {
                Ok(Some(last)) => last,
                _ => {
                    failures.add(format!("no last.json (k={k})"));
                    return false;
                }
            };
            if last.status != "rolled_back" {
                failures.add(format!("last.json says {} (k={k})", last.status));
            }
            if last.reason != "rename_failed" {
                failures.add(format!("last.json's reason is {} (k={k})", last.reason));
            }
            true
        },
    );
    failures.check();
    let rolled_back: u64 = counts.iter().map(|c| c.rolled_back).sum();
    assert_eq!(
        rolled_back, renames,
        "every rename was failed once, and every update was rolled back"
    );

    // One past the last rename there is none to fail: the update goes through.
    sizing.reset();
    let s = sizing.scenario();
    let disk = StdFs;
    let faults = FaultFs::new(&disk);
    faults.fail_rename_with(renames + 1, 32);
    let rig = Rig::new(s, &faults);
    assert_eq!(rig.run().outcome, ApplyOutcome::Committed);
    assert_eq!(tree_hash(&s.root), s.v2_hash);
    println!("  {label}: {renames} renames, each failed once with error 32");
}

// ---- the same, through the real Windows file system, a sample of the operations ------------------

#[cfg(windows)]
mod fault_loops_on_windows {
    use super::*;
    use katana_update::fsops::{RetryPolicy, make_system_fs_ops};
    use katana_update::recover::recover_in_process;
    use std::time::Duration;

    #[test]
    fn the_real_file_system_under_the_faults_ends_in_the_same_two_trees() {
        let mut lp = Loop::new("loop-real", &build_fixture_scenario);
        let real = make_system_fs_ops(&RetryPolicy {
            interval: Duration::from_millis(1),
            total: Duration::from_millis(10),
        });

        let (total, pending_record) = {
            lp.reset();
            let s = lp.scenario();
            let counting = FaultFs::new(real.as_ref());
            let log = LogFs::new(&counting);
            let mut result = ApplyResult::default();
            assert!(apply_until_crash(s, &log, Some(&mut result)).is_none());
            assert_eq!(
                result.outcome,
                ApplyOutcome::Committed,
                "{}{}",
                result.reason,
                result.detail
            );
            let total = counting.mutating_operations();
            let pending_record = log.find("appendDurable", "COMMITTED_PENDING_HEALTH", 1);
            assert_eq!(tree_hash(&s.root), s.v2_hash);
            (total, pending_record)
        };
        // Every third operation; each one is checked, or the case fails on it.
        let sample: Vec<u64> = (1..=total).step_by(3).collect();
        for &k in &sample {
            lp.reset();
            let s = lp.scenario();
            let faults = FaultFs::new(real.as_ref());
            faults.crash_before(k);
            assert!(apply_until_crash(s, &faults, None).is_some(), "{k}");
            let rig = Rig::with_transaction(s, real.as_ref(), "eeeeeeee");
            let status = recover_in_process(&s.root, &rig.services());
            assert!(
                status.is_ok(),
                "k={k}: {}",
                status
                    .as_ref()
                    .err()
                    .map(Error::describe)
                    .unwrap_or_default()
            );
            let wanted = if k > pending_record {
                &s.v2_hash
            } else {
                &s.v1_hash
            };
            assert_eq!(&tree_hash(&s.root), wanted, "k={k}");
        }
        assert!(sample.len() >= 15);

        // And an error 32 at some renames, through the real file system.
        for k in (1..=12u64).step_by(2) {
            lp.reset();
            let s = lp.scenario();
            let faults = FaultFs::new(real.as_ref());
            faults.fail_rename_with(k, 32);
            let rig = Rig::new(s, &faults);
            let result = rig.run();
            assert_eq!(
                result.outcome,
                ApplyOutcome::RolledBack,
                "{k}{}",
                result.detail
            );
            assert_eq!(tree_hash(&s.root), s.v1_hash, "k={k}");
        }
    }
}
