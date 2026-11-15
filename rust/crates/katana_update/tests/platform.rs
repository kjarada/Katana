//! What Linux and macOS do with an update where a copy of Katana does not replace its own files: a
//! download, a preflight and the start of the helper are REFUSED for the platform, nothing is
//! fetched and nothing is written - and what a program may still ask of the process seam there is
//! answered. The twin of tests/update/test_platform.cpp, which Windows does not compile: its seams
//! are the real ones and have tests of their own.
//!
//! WHERE THE EXPECTATIONS COME FROM (CLAUDE.md, section 4) - none is the program's output read
//! back. What these platforms get is settled outside the code: they check for a release and tell
//! of it, and an install of it is refused as unsupported, for the platform, changing nothing. The
//! code of that refusal is ErrorCode::Unsupported, and its token is `platform`, the one the UPDATE
//! verbs say for a system that does not install (reason=platform); the token is read with
//! fetch_failure_reason, the function a caller reads it with. "Nothing was fetched" is a count
//! kept by the transport this file hands over, "no program was run" a count kept by its launcher,
//! and "nothing was written" is std::fs asked directly - never the file system under test. This
//! process's id is the operating system's own answer; process 1 exists on every such system
//! (init, launchd), and kill(2) says EPERM of it to anyone who is not root; a child that has
//! exited and has been waited for is gone. 0 and a number wider than a process id are not alive by
//! the contract (process.rs).
//!
//! WHEN THE FILE SYSTEM AND THE LAUNCHER ARE WRITTEN FOR LINUX (fsops_posix.rs, spawn_posix.rs),
//! these cases part ways: platform_without_apply's then fail on Linux, as they should, and STAY
//! FOR macOS (a Mac refuses for good, whatever that module answers, and these are the cases that
//! fail if writing it turned downloads on inside a sealed bundle); seams_not_written's state what
//! a seam does while it is unwritten, and give way to the tests of what was written;
//! process_seam's hold on both systems, before and after.

#![cfg(not(windows))]

use katana_core::Status;
use katana_core::{Error, ErrorCode, Result};
use katana_update::apply::{ApplyRequest, launch_apply};
use katana_update::feed::{FeedConfig, fetch_failure_reason, fetch_fn};
use katana_update::fsops::{RetryPolicy, make_system_fs_ops};
use katana_update::install::Install;
use katana_update::manifest::Manifest;
use katana_update::plan::Plan;
use katana_update::process::{
    Launcher, SpawnRequest, SpawnResult, make_system_launcher, make_system_process_ops,
    running_programs,
};
use katana_update::stage::{preflight, stage};
use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

// A path under the temporary folder that does NOT exist: the cases prove that it still does not
// when they end. Removed all the same, should a case fail by making it.
struct Nowhere {
    path: PathBuf,
}

impl Nowhere {
    fn new() -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let since_epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "katana_px_{}_{since_epoch}_{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn was_made(&self) -> bool {
        std::fs::symlink_metadata(&self.path).is_ok()
    }
}

impl Drop for Nowhere {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

// The code a call was refused with; nothing when it was not refused.
fn refusal_of<T>(result: &Result<T>) -> Option<ErrorCode> {
    result.as_ref().err().map(|error| error.code)
}

// The reason token of a refusal, as a caller reads it; a word no token is when there was none.
fn reason_of<T>(result: &Result<T>) -> String {
    match result {
        Ok(_) => String::from("not refused"),
        Err(error) => fetch_failure_reason(error),
    }
}

const UNSUPPORTED: Option<ErrorCode> = Some(ErrorCode::Unsupported);

// A launcher that would let anything through, and counts what it was asked to start.
#[derive(Default)]
struct CountingLauncher {
    runs: Cell<i32>,
}

impl Launcher for CountingLauncher {
    fn run(&self, _request: &SpawnRequest) -> Result<SpawnResult> {
        self.runs.set(self.runs.get() + 1);
        Ok(SpawnResult::default())
    }

    fn start_detached(&self, _request: &SpawnRequest) -> Status {
        self.runs.set(self.runs.get() + 1);
        Ok(())
    }
}

// ---- the engine: no download and no preflight -------------------------------------------------

mod platform_without_apply {
    use super::*;

    #[test]
    fn a_download_is_refused_for_the_platform_before_anything_is_fetched_or_written() {
        let nowhere = Nowhere::new();
        let requests = Cell::new(0);
        let fetch = fetch_fn(|_request, _stop, _progress| {
            requests.set(requests.get() + 1);
            Err(Error::new(ErrorCode::Internal, "the transport was asked"))
        });
        // Neither is looked at: the refusal comes before either could matter.
        let manifest = Manifest::default();
        let plan = Plan::default();

        let staged = stage(
            &fetch,
            &FeedConfig::default(),
            &manifest,
            &plan,
            &nowhere.path().join("stage-1"),
            None,
            None,
        );
        assert_eq!(refusal_of(&staged), UNSUPPORTED);
        assert_eq!(reason_of(&staged), "platform");
        assert_eq!(requests.get(), 0);
        assert!(!nowhere.was_made());
    }

    #[test]
    fn a_preflight_is_refused_for_the_platform_and_runs_no_program() {
        let nowhere = Nowhere::new();
        let launcher = CountingLauncher::default();
        let install = Install {
            root: nowhere.path().join("Katana"),
            ..Install::default()
        };

        let prepared = preflight(
            &Manifest::default(),
            &install,
            &nowhere.path().join("stage-1"),
            &nowhere.path().join("state"),
            &launcher,
        );
        assert_eq!(refusal_of(&prepared), UNSUPPORTED);
        assert_eq!(reason_of(&prepared), "platform");
        assert_eq!(launcher.runs.get(), 0);
        assert!(!nowhere.was_made(), "no run folder is left, nor begun");
    }
}

// ---- the seams that are not written -----------------------------------------------------------

mod seams_not_written {
    use super::*;

    #[test]
    fn every_operation_of_the_system_file_system_is_refused_and_makes_nothing() {
        let nowhere = Nowhere::new();
        let system = make_system_fs_ops(&RetryPolicy::default());
        let folder = nowhere.path().join("folder");
        let file = nowhere.path().join("file");

        assert_eq!(refusal_of(&system.stat(&file)), UNSUPPORTED);
        assert_eq!(
            refusal_of(&system.list_directory(nowhere.path())),
            UNSUPPORTED
        );
        assert_eq!(refusal_of(&system.read_file(&file)), UNSUPPORTED);
        assert_eq!(refusal_of(&system.resolve(&file)), UNSUPPORTED);
        assert_eq!(refusal_of(&system.free_space(nowhere.path())), UNSUPPORTED);
        assert_eq!(refusal_of(&system.make_directories(&folder)), UNSUPPORTED);
        assert_eq!(
            refusal_of(&system.copy_file(&file, &folder.join("copy"))),
            UNSUPPORTED
        );
        assert_eq!(
            refusal_of(&system.rename_file(&file, &folder.join("moved"))),
            UNSUPPORTED
        );
        assert_eq!(refusal_of(&system.remove_file(&file)), UNSUPPORTED);
        assert_eq!(
            refusal_of(&system.remove_empty_directory(&folder)),
            UNSUPPORTED
        );
        assert_eq!(refusal_of(&system.append_durable(&file, b"a")), UNSUPPORTED);
        assert_eq!(refusal_of(&system.write_durable(&file, b"w")), UNSUPPORTED);
        assert_eq!(refusal_of(&system.set_executable(&file)), UNSUPPORTED);
        assert_eq!(
            refusal_of(&system.sync_directory(nowhere.path())),
            UNSUPPORTED
        );
        assert_eq!(
            refusal_of(&system.try_lock(&nowhere.path().join("lock"))),
            UNSUPPORTED
        );
        assert!(
            !nowhere.was_made(),
            "a refusal that made a folder, a file or a lock"
        );
    }

    #[test]
    fn the_system_launcher_starts_nothing_and_says_it_is_the_platform() {
        let launcher = make_system_launcher();
        // A program that is there on every such system: nothing is missing but the launcher.
        let request = SpawnRequest {
            executable: PathBuf::from("/bin/sh"),
            arguments: vec![String::from("-c"), String::from("exit 0")],
            ..SpawnRequest::default()
        };
        let ran = launcher.run(&request);
        assert_eq!(refusal_of(&ran), UNSUPPORTED);
        assert_eq!(reason_of(&ran), "platform");
        let started = launcher.start_detached(&request);
        assert_eq!(refusal_of(&started), UNSUPPORTED);
        assert_eq!(reason_of(&started), "platform");
    }

    #[test]
    fn starting_the_helper_is_refused_for_the_platform_and_not_as_a_download_too_large() {
        // launch_apply holds a launcher and no file system, so it is the launcher's refusal that
        // reaches its caller - under the token stage() and preflight() give, and not
        // `too_large`, which is what an Unsupported with no token of its own reads as (feed.rs).
        let apply = ApplyRequest {
            helper: PathBuf::from("/bin/sh"),
            transaction: String::from("a1b2c3d4"),
            ..ApplyRequest::default()
        };
        let launched = launch_apply(&apply);
        assert_eq!(refusal_of(&launched), UNSUPPORTED);
        assert_eq!(reason_of(&launched), "platform");
    }

    #[test]
    fn the_programs_running_from_an_install_are_not_guessed_at() {
        // "None" from a scan that did not look would be read as "nothing is running, go on".
        let running = running_programs(&std::env::temp_dir());
        assert_eq!(refusal_of(&running), UNSUPPORTED);
        assert_eq!(reason_of(&running), "platform");
        let processes = make_system_process_ops();
        assert_eq!(
            refusal_of(&processes.holders_under(&std::env::temp_dir())),
            UNSUPPORTED
        );
    }
}

// ---- the process seam's two answers -----------------------------------------------------------

mod process_seam {
    use super::*;

    #[test]
    fn a_program_is_told_its_own_process_id_and_which_processes_are_alive() {
        let processes = make_system_process_ops();
        assert_eq!(processes.current_pid(), u64::from(std::process::id()));
        assert!(processes.is_alive(processes.current_pid()));
        assert!(
            processes.is_alive(1),
            "a process this user may not signal is alive too"
        );
        assert!(
            !processes.is_alive(0),
            "0 names a group to kill(), not a process"
        );
        assert!(!processes.is_alive(1u64 << 40), "wider than any process id");
    }

    #[test]
    fn a_process_that_has_ended_and_been_waited_for_is_not_alive() {
        // The C++ forks a child that exits at once; a child the standard library starts is the
        // same to the seam: a process of ours that ends and is then waited for.
        let mut child = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg("exit 0")
            .spawn()
            .expect("a child process");
        let processes = make_system_process_ops();
        // Until it is waited for it is there: running still, or ended and not yet reaped, which
        // counts as alive (process_scan_posix.rs says why that is left so).
        assert!(processes.is_alive(u64::from(child.id())));

        let status = child.wait().expect("the child is waited for");
        assert!(status.success());
        assert!(!processes.is_alive(u64::from(child.id())));
    }
}
