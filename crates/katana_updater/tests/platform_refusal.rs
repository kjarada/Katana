//! Where a copy of Katana does not replace its own files, the helper program refuses a swap and a
//! recovery FOR THE PLATFORM, and writes nothing: the twins of the CTest cases
//! `katana_updater_refuses_for_the_platform_and_writes_nothing` and
//! `katana_updater_test_refuses_for_the_platform_and_writes_nothing` (tests/update/CMakeLists.txt),
//! which run tests/update/check_platform_refusal.cmake on each of the two helpers. This file does
//! what that script does, step for step, on the Rust programs.
//!
//! Registered where the system is not Windows: Linux, until its apply back end is written (task
//! T18, katana_update's fsops_posix.rs), and macOS for good, where a file changed inside
//! Katana.app breaks the seal of the bundle. There the helper is started with the two command lines
//! it has for changing an install - the swap, and --recover - and each must end
//!
//! ```text
//!   outcome=refused   (the swap)   or   recover=refused   (--recover)
//!   reason=platform
//!   exit=13
//!   detail=<a sentence>
//! ```
//!
//! with the exit code 13, having made no file and no folder anywhere.
//!
//! Why it matters: before this was held, the helper on these systems answered a swap with
//! `root_not_writable` AFTER writing a last.json into the state folder, which the window reads at
//! its next start and reports as an update that failed; and it answered --recover with "failed",
//! exit 12, the code that tells a person their install may be half swapped.
//!
//! WHERE THE EXPECTATIONS COME FROM - none is the program's output read back:
//!
//! * 13 is "refused: nothing was touched" in the helper's table of exit codes, which its own
//!   --help prints (src/lib.rs, top);
//! * `platform` is the reason the UPDATE verbs give for a system that does not install an update,
//!   and what these systems get is settled outside the code: they are told of a release, and an
//!   install of it is refused as unsupported, for the platform, changing nothing;
//! * "wrote nothing" is THIS file's listing of its folder after each run, never the helper's word.
//!
//! WHEN T18 WRITES THE LINUX BACK END this stays for macOS alone, and holds there whatever that
//! back end answers: it is what fails if writing it let the helper swap files inside a bundle.

#![cfg(not(windows))]

mod support;

use std::path::Path;
use std::process::Command;
use std::time::Duration;
use support::{Ran, run, work_folder};

// What is under `work` and in its install folder, by name, sorted (the script's file(GLOB) with
// LIST_DIRECTORIES, which matches a name that begins with a dot as well).
fn list_work(work: &Path) -> Vec<String> {
    let mut inside = Vec::new();
    for (folder, prefix) in [(work.to_path_buf(), ""), (work.join("install"), "install/")] {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            inside.push(format!("{prefix}{}", entry.file_name().to_string_lossy()));
        }
    }
    inside.sort();
    inside
}

// Nothing is under `work` but the empty install folder. The three names a helper that went on
// would make first are asked for one by one - the state folder (last.json), the stage folder and
// the install's .update (the lock, the journal, recovery.log) - and then everything is listed.
fn expect_nothing_written(work: &Path, after: &str) {
    for made in [
        work.join("state"),
        work.join("stage"),
        work.join("install").join(".update"),
    ] {
        assert!(
            std::fs::symlink_metadata(&made).is_err(),
            "After {after} there is {}: the helper wrote something on a system where it changes \
             nothing.",
            made.display()
        );
    }
    let inside = list_work(work);
    assert_eq!(
        inside,
        vec![String::from("install")],
        "After {after} the test's folder holds more than its empty install folder"
    );
}

// The script's expression `^<first>\r?\nreason=platform\r?\nexit=13\r?\ndetail=[^\r\n]+\r?\n$`.
fn is_the_refusal(said: &str, first: &str) -> bool {
    let lines = said.replace("\r\n", "\n");
    let Some(rest) = lines.strip_prefix(&format!("{first}\nreason=platform\nexit=13\ndetail="))
    else {
        return false;
    };
    let Some(detail) = rest.strip_suffix('\n') else {
        return false;
    };
    !detail.is_empty() && !detail.contains(['\r', '\n'])
}

// Runs the helper and holds it to the four lines and the exit code. `first` is the first line.
fn expect_refusal(helper: &Path, work: &Path, what: &str, first: &str, arguments: &[&str]) {
    let mut command = Command::new(helper);
    command.args(arguments);
    let ran: Ran = run(command, Duration::from_secs(120));
    assert_eq!(
        ran.code,
        Some(13),
        "The helper, asked for {what}, did not exit with 13 (refused).\n{}",
        ran.describe()
    );
    assert!(
        is_the_refusal(&ran.stdout, first),
        "The helper, asked for {what}, did not print exactly {first}, reason=platform, exit=13 \
         and a detail= line.\n{}",
        ran.describe()
    );
    expect_nothing_written(work, what);
}

fn refuses_for_the_platform_and_writes_nothing(helper: &Path, work: &Path) {
    // The folder, made new: an install folder that is there and empty (--recover needs one that
    // exists), and a stage and a state folder that are NOT there.
    let _ = std::fs::remove_dir_all(work);
    std::fs::create_dir_all(work.join("install")).expect("the test's install folder");
    let text = |path: &Path| path.to_string_lossy().into_owned();
    let root = text(&work.join("install"));
    let stage = text(&work.join("stage"));
    let state = text(&work.join("state"));

    // The listing can see what it is there to find: a check that cannot see a violation passes as
    // quietly as one that finds none. A file put into the install folder must show in it.
    let probe = work.join("install").join("probe");
    std::fs::write(&probe, b"").expect("the probe");
    let with_probe = list_work(work);
    std::fs::remove_file(&probe).expect("the probe goes");
    assert_eq!(
        with_probe,
        vec![String::from("install"), String::from("install/probe")],
        "The listing of the test's folder does not show a file put into its install folder, so it \
         could not show one the helper made."
    );

    expect_nothing_written(work, "the folder was made");
    expect_refusal(
        helper,
        work,
        "a swap",
        "outcome=refused",
        &[
            "--root",
            root.as_str(),
            "--stage",
            stage.as_str(),
            "--state",
            state.as_str(),
            "--tx",
            "a1b2c3d4",
            "--wait-pid",
            "0",
            "--relaunch",
            "none",
        ],
    );
    expect_refusal(
        helper,
        work,
        "a recovery",
        "recover=refused",
        &["--recover", root.as_str()],
    );

    let _ = std::fs::remove_dir_all(work);
}

#[test]
fn katana_updater_refuses_for_the_platform_and_writes_nothing() {
    refuses_for_the_platform_and_writes_nothing(
        Path::new(env!("CARGO_BIN_EXE_katana_updater")),
        &work_folder("platform_refusal").join("katana_updater"),
    );
}

#[test]
fn katana_updater_test_refuses_for_the_platform_and_writes_nothing() {
    refuses_for_the_platform_and_writes_nothing(
        Path::new(env!("CARGO_BIN_EXE_katana_updater_test")),
        &work_folder("platform_refusal").join("katana_updater_test"),
    );
}
