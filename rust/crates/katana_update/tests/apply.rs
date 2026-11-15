//! The swap (apply.rs), one case at a time. The twin of tests/update/test_apply.cpp. The loops that
//! break it at every file system operation are in the twin of test_fault_loops.cpp; rollback and
//! recovery are in the twin of test_recover.cpp.
//!
//! Where the expectations come from (CLAUDE.md, section 4):
//!
//! * the tree an update must leave is built from the specification of the release, in its own
//!   folder (the harness), or from the T3 fixtures made by the Python producer;
//! * the signatures are made with ed25519-dalek's signer, or are the T3 fixtures' (`openssl
//!   pkeyutl`);
//! * the command-line quoting is checked against the operating system's own parser
//!   (CommandLineToArgvW) and against the examples Microsoft publishes.
//!
//! HOW THE C++'S DOUBLES BECAME RUST. A C++ case that sets the rig's launcher to read the rig's
//! own registry while the new version is checked cannot be written so in Rust: a closure kept in
//! the rig cannot borrow the rig. Those cases make the launcher and the registry beside the rig,
//! as the rig makes them, and run the swap with them (`run_with_doubles`), which is the C++'s
//! services with the same doubles. A case that runs a helper on another thread builds that
//! helper's rig on that thread (a rig holds a file system by a shared reference, which does not
//! cross threads), and shares the scan count and the release flag through the Arcs the harness
//! gives them.

pub mod test_key;

pub mod apply_harness;

use apply_harness::{
    FakeLauncher, FakeRegistry, FileSpec, ForwardingFs, HookFs, INSTALL_JSON, LogFs, Loop,
    ReleaseSpec, Rig, Scenario, ScriptedProcesses, StdFs, build_fixture_scenario,
    build_fixture_scenario_of, build_forty_file_scenario, copy_tree, difference, fixture_folder,
    healthy_start, injected, is_present, journal_state_of, leftovers_in_update, manifest_json,
    no_answer, nobody, os_error, read_text, recover_clean, sign_line, snapshot, somebody, text,
    tree_hash, write_install, write_stage, write_text,
};
use katana_core::error::make_error;
use katana_core::path_text::{path_from_utf8, path_to_utf8};
use katana_core::{Error, ErrorCode};
use katana_update::apply::{
    ApplyOutcome, ApplyRequest, ApplyResult, ApplyServices, apply, helper_arguments,
    installed_program, is_valid_transaction_id, launch_apply_with, new_transaction_id,
    parse_helper_arguments,
};
use katana_update::command_line::{command_line, quote_argument};
use katana_update::fault_fs::FaultFs;
use katana_update::fsops::{EntryInfo, FsOps};
use katana_update::install::read_install;
use katana_update::journal::{JournalOp, JournalState, parse_journal, to_string as op_name};
use katana_update::manifest::FileClass;
use katana_update::process::{Launcher, ProcessOps, RunningProgram, SpawnRequest, SpawnResult};
use katana_update::recover::{Settled, recover_in_process, settle_unproved};
use katana_update::recover_internal::may_replace_unreadable_state;
use katana_update::registry::{RUN_ONCE_NAME, RegistryOps};
use katana_update::state::{LastResult, health_marker_path, read_last_result, read_state};
use katana_update::trust::Trust;
use katana_update::version::{
    parse_version, program_version, program_version_text, to_string as version_text,
};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::io::ErrorKind;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::time::{Duration, Instant};
use test_key::test_trust;

// ---- helpers ------------------------------------------------------------------------------------

fn expect_untouched(s: &Scenario, result: &ApplyResult) {
    assert_eq!(
        tree_hash(&s.root),
        s.v1_hash,
        "{}{}",
        difference(&s.v1_snapshot, &snapshot(&s.root)),
        result.detail
    );
    assert!(
        leftovers_in_update(&s.root).is_empty(),
        "a refused update leaves no transaction folder"
    );
}

fn expect_refused(s: &Scenario, result: &ApplyResult, reason: &str) {
    assert_eq!(result.outcome, ApplyOutcome::Refused, "{}", result.detail);
    assert_eq!(result.reason, reason, "{}", result.detail);
    expect_untouched(s, result);
}

fn last_of(s: &Scenario) -> LastResult {
    match read_last_result(&s.state) {
        Ok(Some(last)) => last,
        Ok(None) => panic!("last.json was not written"),
        Err(error) => panic!("last.json was not read: {}", error.describe()),
    }
}

// The versions of the releases this file writes out itself, for an update from the one to the
// other. They are versions no build of Katana has, on purpose, and that is the second of two
// things that keep these cases from knowing which build they are.
//
// The first is the harness's: every install it writes has an install.json that names the version
// its package was built as (INSTALL_JSON), and where the package names its version the program
// that reads the install is not asked for its own (install.rs). What is installed is then what the
// records say - also for the fixture and the forty-file updates, 0.9.0 to 0.10.0, which a build
// above 0.10.0 would otherwise refuse as a `downgrade`.
// an_install_the_harness_writes_is_read_as_its_records_say_and_not_as_this_build holds that.
//
// The second is for a record that says its target and no more, as the harness's did: the
// installed version is then the HIGHER of installed.json's and the asking program's, and a release
// below this build's version reads as the build's. At 0.1.0 installed and 0.2.0 staged these
// updates went through while the project was 0.2.0 - by their sequences - and every one of them
// would have been a `downgrade` at 0.3.0. Above any project version the record is the higher
// whoever is asked (the_releases_written_out_here_are_above_the_version_of_any_build holds that).
// The rule itself, both halves of it: the twin of tests/update/test_state.cpp.
const OLD_VERSION: &str = "900.1.0";
const NEW_VERSION: &str = "900.2.0";

fn small_release(version: &str, sequence: u64) -> ReleaseSpec {
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

// A scenario from two small releases written out by the harness (the fixtures are the other
// source). Everything in the install is v1's; the stage is v2's.
fn build_small_scenario(s: &mut Scenario) {
    s.v1 = small_release(OLD_VERSION, 100);
    s.v2 = small_release(NEW_VERSION, 200);
    write_install(&s.root, &s.v1);
    write_stage(&s.stage, &s.v2);
    s.v1_hash = tree_hash(&s.root);
    s.v1_snapshot = snapshot(&s.root);
}

/// The C++'s `Scenario s(label); buildFixtureScenario(s);`.
fn fixture_scenario(label: &str) -> Scenario {
    let mut s = Scenario::new(label);
    build_fixture_scenario(&mut s);
    s
}

/// The C++'s `buildSmallScenario` on a new scenario.
fn small_scenario(label: &str) -> Scenario {
    let mut s = Scenario::new(label);
    build_small_scenario(&mut s);
    s
}

/// std::filesystem::remove: a file that is not there is no error.
fn remove(path: &Path) {
    if let Err(error) = std::fs::remove_file(path)
        && error.kind() != ErrorKind::NotFound
    {
        panic!("{}: {error}", text(path));
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    needle.is_empty()
        || haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// A relative path as std::filesystem's generic_string gives it: '/'-separated.
fn generic(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<String>>()
        .join("/")
}

/// A launcher's answer: the program ran and exited with `exit_code`.
fn exited(exit_code: i32) -> SpawnResult {
    SpawnResult {
        exit_code,
        ..SpawnResult::default()
    }
}

/// The C++'s `request.helper.parent_path()` as text with its line feed: what cleanup.list holds.
fn queued(helper: &Path) -> Vec<u8> {
    let mut line = path_to_utf8(helper.parent().unwrap_or(Path::new("")));
    line.push(b'\n');
    line
}

/// One run of the swap with doubles of the test's own in the place of the rig's.
fn run_with_doubles(
    rig: &Rig<'_>,
    processes: &dyn ProcessOps,
    launcher: &dyn Launcher,
    registry: &dyn RegistryOps,
) -> ApplyResult {
    let services = ApplyServices {
        fs: rig.fs,
        processes,
        launcher,
        registry,
        trust: test_trust(),
        timings: rig.timings,
    };
    apply(&rig.request, &services, None)
}

// One run of the swap with a process seam of the test's own in the place of the rig's: the
// harness's ScriptedProcesses, which gives each question about the programs under the root its own
// answer (beside FakeProcesses in the harness).
fn run_with(rig: &Rig<'_>, processes: &dyn ProcessOps) -> ApplyResult {
    run_with_doubles(rig, processes, &rig.launcher, &rig.registry)
}

fn good_arguments() -> Vec<String> {
    [
        "--root",
        "/r",
        "--stage",
        "/s",
        "--state",
        "/u",
        "--tx",
        "ab12",
        "--wait-pid",
        "7",
        "--relaunch",
        "none",
    ]
    .iter()
    .map(|argument| String::from(*argument))
    .collect()
}

// Answers "held" to the first `held_for` attempts to take the lock (always, when it is -1): what a
// virus scanner or an indexer that has the lock file open for a moment looks like to a program
// that asks for it with share mode 0. The C++'s BusyLockFs; `attempts` counts every attempt.
fn busy_lock_fs<'a>(
    below: &'a dyn FsOps,
    held_for: i32,
    attempts: &'a Cell<i32>,
) -> ForwardingFs<'a> {
    let busy = ForwardingFs::new(below);
    busy.on_try_lock(move |inner, p: &Path| {
        attempts.set(attempts.get() + 1);
        if held_for < 0 || attempts.get() <= held_for {
            return Err(make_error(
                ErrorCode::InvalidState,
                "held",
                format!("os_error=32 {}", text(p)),
            ));
        }
        inner.try_lock(p)
    });
    busy
}

// ---- the helper's command line ------------------------------------------------------------------

mod helper_arguments {
    use super::*;

    #[test]
    fn a_request_is_the_six_options_in_the_documented_order() {
        let request = ApplyRequest {
            root: path_from_utf8("/opt/katana"),
            stage_directory: path_from_utf8("/var/u/stage-5"),
            state_directory: path_from_utf8("/var/u"),
            transaction: String::from("abc123"),
            wait_pid: 4412,
            relaunch: path_from_utf8("/opt/katana/bin/katana"),
            ..ApplyRequest::default()
        };
        let expected = [
            "--root",
            "/opt/katana",
            "--stage",
            "/var/u/stage-5",
            "--state",
            "/var/u",
            "--tx",
            "abc123",
            "--wait-pid",
            "4412",
            "--relaunch",
            "/opt/katana/bin/katana",
        ];
        assert_eq!(helper_arguments(&request), expected);
    }

    #[test]
    fn a_request_with_a_space_and_a_non_ascii_letter_in_its_paths_comes_back_whole() {
        let request = ApplyRequest {
            root: path_from_utf8("C:/Users/Jos\u{e9} M\u{fc}ller/Programs/Katana Install"),
            stage_directory: path_from_utf8(
                "C:/Users/Jos\u{e9} M\u{fc}ller/AppData/Katana/update/stage-1792312800",
            ),
            state_directory: path_from_utf8("C:/Users/Jos\u{e9} M\u{fc}ller/AppData/Katana/update"),
            transaction: String::from("0123456789abcdef0123456789abcdef"),
            wait_pid: 4294967295,
            relaunch: path_from_utf8(
                "C:/Users/Jos\u{e9} M\u{fc}ller/Programs/Katana Install/bin/katana.exe",
            ),
            ..ApplyRequest::default()
        };

        let arguments = helper_arguments(&request);
        let parsed = parse_helper_arguments(&arguments)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(parsed.root, request.root);
        assert_eq!(parsed.stage_directory, request.stage_directory);
        assert_eq!(parsed.state_directory, request.state_directory);
        assert_eq!(parsed.transaction, request.transaction);
        assert_eq!(parsed.wait_pid, request.wait_pid);
        assert_eq!(parsed.relaunch, request.relaunch);
        assert!(
            parsed.helper.as_os_str().is_empty(),
            "the helper is argv[0]'s copy, not on the command line"
        );
        // The UTF-8 is carried as it is.
        assert_eq!(
            arguments[1],
            "C:/Users/Jos\u{e9} M\u{fc}ller/Programs/Katana Install"
        );
    }

    #[test]
    fn no_relaunch_is_the_word_none_and_comes_back_empty() {
        let request = ApplyRequest {
            root: path_from_utf8("/r"),
            stage_directory: path_from_utf8("/s"),
            state_directory: path_from_utf8("/u"),
            transaction: String::from("1"),
            ..ApplyRequest::default()
        };
        let arguments = helper_arguments(&request);
        assert_eq!(arguments.len(), 12);
        assert_eq!(arguments[10], "--relaunch");
        assert_eq!(arguments[11], "none");
        assert_eq!(arguments[9], "0");
        let parsed = parse_helper_arguments(&arguments)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(parsed.relaunch.as_os_str().is_empty());
        assert_eq!(parsed.wait_pid, 0);
    }

    #[test]
    fn a_transaction_id_that_is_not_lower_case_hexadecimal_is_refused() {
        // It names a folder under R/.update and one in the state folder, and a command line is
        // not trusted to name only that.
        let refused = [
            String::new(),
            String::from(".."),
            String::from("../x"),
            String::from("ABC"),
            String::from("g1"),
            String::from("a b"),
            String::from("a/b"),
            String::from("a\\b"),
            String::from("C:"),
            "a".repeat(65),
            String::from("caf\u{e9}"),
        ];
        for tx in refused {
            let mut arguments = good_arguments();
            arguments[7] = tx.clone();
            let Err(error) = parse_helper_arguments(&arguments) else {
                panic!("accepted '{tx}'");
            };
            assert_eq!(error.code, ErrorCode::InvalidArgument);
        }
        let mut longest = good_arguments();
        longest[7] = "f".repeat(64);
        assert!(
            parse_helper_arguments(&longest).is_ok(),
            "64 characters is the longest id"
        );
    }

    #[test]
    fn a_missing_an_unknown_a_repeated_and_a_valueless_option_are_refused() {
        let good = good_arguments();
        assert!(parse_helper_arguments(&good).is_ok());
        // Each option left out in turn.
        for i in (0..good.len()).step_by(2) {
            let mut fewer = good[..i].to_vec();
            fewer.extend_from_slice(&good[i + 2..]);
            let Err(error) = parse_helper_arguments(&fewer) else {
                panic!("without {}", good[i]);
            };
            assert!(error.message.contains("missing option"));
            assert_eq!(error.context, good[i]);
        }
        // An option nobody defined.
        let mut unknown = good.clone();
        unknown.extend([String::from("--elevate"), String::from("yes")]);
        assert!(parse_helper_arguments(&unknown).is_err());
        // The same option twice, even with the same value.
        let mut twice = good.clone();
        twice.extend([String::from("--root"), String::from("/r")]);
        let Err(repeated) = parse_helper_arguments(&twice) else {
            panic!("an option given twice was accepted");
        };
        assert!(repeated.message.contains("twice"));
        // An option with nothing after it.
        let mut valueless = good.clone();
        valueless.push(String::from("--root"));
        assert!(parse_helper_arguments(&valueless).is_err());
        // A bare word.
        let mut bare = good.clone();
        bare.insert(0, String::from("stray"));
        assert!(parse_helper_arguments(&bare).is_err());
        let none: [&str; 0] = [];
        assert!(parse_helper_arguments(&none).is_err());
    }

    #[test]
    fn a_process_id_that_is_not_a_number_and_an_empty_folder_are_refused() {
        for pid in [
            "",
            "abc",
            "-1",
            "12x",
            " 5",
            "5 ",
            "99999999999999999999999",
        ] {
            let mut arguments = good_arguments();
            arguments[9] = String::from(pid);
            assert!(
                parse_helper_arguments(&arguments).is_err(),
                "accepted '{pid}'"
            );
        }
        for position in [1usize, 3, 5] {
            let mut arguments = good_arguments();
            arguments[position] = String::new();
            assert!(
                parse_helper_arguments(&arguments).is_err(),
                "{}",
                good_arguments()[position - 1]
            );
        }
    }

    #[test]
    fn the_helper_is_started_detached_with_the_arguments_and_from_its_own_folder() {
        let request = ApplyRequest {
            root: path_from_utf8("/r"),
            stage_directory: path_from_utf8("/s"),
            state_directory: path_from_utf8("/u"),
            transaction: String::from("ab"),
            helper: path_from_utf8("/u/run-ab/katana_updater"),
            ..ApplyRequest::default()
        };
        let launcher = FakeLauncher::default();
        assert!(launch_apply_with(&request, &launcher).is_ok());
        let started = launcher.started();
        assert_eq!(started.len(), 1);
        assert!(launcher.ran().is_empty(), "detached, not waited for");
        assert_eq!(started[0].executable, request.helper);
        assert_eq!(started[0].arguments, helper_arguments(&request));
        // Not the install folder: a program whose working directory is inside the install keeps
        // that folder from being moved.
        assert_eq!(
            Some(started[0].working_directory.as_path()),
            request.helper.parent()
        );

        let none = ApplyRequest {
            helper: PathBuf::new(),
            ..request.clone()
        };
        let refused = launch_apply_with(&none, &launcher);
        assert!(refused.is_err());
        assert_eq!(launcher.started().len(), 1);
    }

    #[test]
    fn installed_program_is_bin_under_the_root_with_the_extension_of_the_system() {
        let root = path_from_utf8("/opt/katana");
        let expected = if cfg!(windows) {
            root.join("bin").join("katana.exe")
        } else {
            root.join("bin").join("katana")
        };
        assert_eq!(installed_program(&root, "katana"), expected);
    }
}

mod transaction_id {
    use super::*;

    #[test]
    fn a_new_id_is_valid_as_long_as_the_documentation_says_and_never_repeats() {
        let mut seen = BTreeSet::new();
        for _ in 0..2000 {
            let id = new_transaction_id();
            assert!(is_valid_transaction_id(&id), "{id}");
            assert!(id.len() >= 16, "enough that two runs do not collide");
            assert!(seen.insert(id.clone()), "repeated: {id}");
        }
    }

    #[test]
    fn ids_made_at_the_same_time_by_many_threads_are_all_different() {
        // The counter that is part of each id is shared by the threads.
        const THREADS: usize = 8;
        const EACH: usize = 250;
        let made: Vec<Vec<String>> = std::thread::scope(|scope| {
            let mut threads = Vec::new();
            for _ in 0..THREADS {
                threads.push(scope.spawn(|| {
                    (0..EACH)
                        .map(|_| new_transaction_id())
                        .collect::<Vec<String>>()
                }));
            }
            let mut made = Vec::new();
            for thread in threads {
                made.push(thread.join().expect("a thread that makes ids"));
            }
            made
        });
        let mut seen = BTreeSet::new();
        for ids in &made {
            for id in ids {
                assert!(is_valid_transaction_id(id), "{id}");
                assert!(seen.insert(id.clone()), "repeated: {id}");
            }
        }
        assert_eq!(seen.len(), THREADS * EACH);
    }
}

// ---- command-line quoting -----------------------------------------------------------------------

mod command_line {
    use super::*;

    #[test]
    fn arguments_are_quoted_by_the_rules_windows_parses_them_with() {
        // Microsoft's rules ("Parsing C++ command-line arguments"): an ordinary word is bare.
        assert_eq!(quote_argument("plain"), "plain");
        assert_eq!(quote_argument("--root"), "--root");
        // A space (or tab) wraps it in quotes; an empty argument is two quotes.
        assert_eq!(quote_argument("a b"), "\"a b\"");
        assert_eq!(quote_argument("a\tb"), "\"a\tb\"");
        assert_eq!(quote_argument(""), "\"\"");
        // A quote inside gets a backslash.
        assert_eq!(quote_argument("say \"hi\""), "\"say \\\"hi\\\"\"");
        assert_eq!(quote_argument("a\"b"), "\"a\\\"b\"");
        // Backslashes are literal unless they end at a quote: then they double.
        assert_eq!(quote_argument("C:\\dir\\file"), "C:\\dir\\file");
        assert_eq!(quote_argument("C:\\my dir\\file"), "\"C:\\my dir\\file\"");
        // A trailing backslash doubled.
        assert_eq!(quote_argument("C:\\my dir\\"), "\"C:\\my dir\\\\\"");
        // One backslash before a quote: 2n+1.
        assert_eq!(quote_argument("a\\\"b"), "\"a\\\\\\\"b\"");
        assert_eq!(
            quote_argument("\\\\server\\share x"),
            "\"\\\\server\\share x\""
        );
    }

    #[test]
    fn a_program_is_always_quoted_because_a_path_may_hold_a_space() {
        assert_eq!(
            command_line(
                "C:\\Program Files\\k\\katana_updater.exe",
                &[String::from("--recover"), String::from("C:\\My Dir")]
            ),
            "\"C:\\Program Files\\k\\katana_updater.exe\" --recover \"C:\\My Dir\""
        );
        assert_eq!(command_line("C:\\k\\u.exe", &[]), "\"C:\\k\\u.exe\"");
    }

    #[cfg(windows)]
    #[test]
    #[allow(
        unsafe_code,
        reason = "the operating system's own parser is CommandLineToArgvW, called directly"
    )]
    fn the_operating_systems_own_parser_reads_back_what_was_written() {
        use katana_update::process::utf8_from_wide;
        use windows_sys::Win32::Foundation::LocalFree;
        use windows_sys::Win32::UI::Shell::CommandLineToArgvW;

        let arguments: Vec<String> = [
            "plain",
            "a b",
            "",
            "say \"hi\"",
            "C:\\dir\\file",
            "C:\\my dir\\",
            "a\\\"b",
            "\\\\server\\share x",
            "--relaunch",
            "none",
            "tab\there",
            "trailing\\\\",
            "two  spaces",
            "quote\"",
            "\"",
            "\\",
            "\\\\\"",
        ]
        .iter()
        .map(|argument| String::from(*argument))
        .collect();
        let line = command_line("C:\\Program Files\\Katana\\bin\\katana.exe", &arguments);
        // ASCII only.
        let wide: Vec<u16> = line.encode_utf16().chain(Some(0)).collect();
        let mut count = 0i32;
        // SAFETY: `wide` is a NUL-terminated wide string; the call allocates the array it returns.
        let parsed = unsafe { CommandLineToArgvW(wide.as_ptr(), &raw mut count) };
        assert!(!parsed.is_null());
        assert_eq!(usize::try_from(count).unwrap_or(0), arguments.len() + 1);
        let argument = |i: usize| -> String {
            // SAFETY: the array holds `count` pointers to NUL-terminated wide strings, and `i` is
            // below it.
            unsafe {
                let pointer = *parsed.add(i);
                let length = (0..).take_while(|&at| *pointer.add(at) != 0).count();
                utf8_from_wide(std::slice::from_raw_parts(pointer, length))
            }
        };
        assert_eq!(argument(0), "C:\\Program Files\\Katana\\bin\\katana.exe");
        for (i, expected) in arguments.iter().enumerate() {
            assert_eq!(&argument(i + 1), expected, "argument {i}");
        }
        // SAFETY: the array was allocated by CommandLineToArgvW, freed once with LocalFree.
        unsafe { LocalFree(parsed.cast()) };
    }
}

// ---- a good update ------------------------------------------------------------------------------

mod apply {
    use super::*;

    #[test]
    fn the_fixture_release_replaces_the_install_and_leaves_exactly_the_tree_of_the_new_version() {
        let s = fixture_scenario("apply-fixture");
        assert_ne!(s.v1_hash, s.v2_hash, "the fixtures differ");
        let disk = StdFs;
        let rig = Rig::new(&s, &disk);
        let result = rig.run();

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
            "a healthy transaction deletes its folder"
        );
    }

    #[test]
    fn the_fixture_update_does_what_the_release_says_file_by_file() {
        let s = fixture_scenario("apply-fixture-facts");
        let disk = StdFs;
        let rig = Rig::new(&s, &disk);
        assert_eq!(rig.run().outcome, ApplyOutcome::Committed);

        let root = &s.root;
        let v1 = fixture_folder().join("install/v1");
        let v2 = fixture_folder().join("install/v2");
        // Changed: the new bytes.
        assert_eq!(
            read_text(&root.join("bin/katana.exe")),
            read_text(&v2.join("bin/katana.exe"))
        );
        assert_ne!(
            read_text(&root.join("bin/katana.exe")),
            read_text(&v1.join("bin/katana.exe"))
        );
        // Unchanged (the same hash in both releases): untouched, and still the same bytes.
        assert_eq!(
            read_text(&root.join("bin/katana_cli.exe")),
            read_text(&v1.join("bin/katana_cli.exe"))
        );
        assert_eq!(
            read_text(&root.join("share/katana/random.bin")),
            read_text(&v2.join("share/katana/random.bin"))
        );
        // Added, with the folder it needs.
        assert_eq!(
            read_text(&root.join("bin/libnew-2.dll")),
            read_text(&v2.join("bin/libnew-2.dll"))
        );
        assert_eq!(
            read_text(&root.join("share/katana/help/index.txt")),
            read_text(&v2.join("share/katana/help/index.txt"))
        );
        // Removed: v1 listed them, v2 does not, and nobody had touched them; the folder the
        // removal emptied goes too.
        assert!(!is_present(&root.join("bin/libold-1.dll")));
        assert!(!is_present(&root.join("share/katana/legacy/old.txt")));
        assert!(!is_present(&root.join("share/katana/legacy")));
        // The seed was there, so it was not rewritten: v1's bytes, not v2's.
        assert_eq!(
            read_text(&root.join("etc/katana/settings.ini")),
            read_text(&v1.join("etc/katana/settings.ini"))
        );
        assert_ne!(
            read_text(&root.join("etc/katana/settings.ini")),
            read_text(&v2.join("etc/katana/settings.ini"))
        );
        // The records: installed.json by hand, the manifest and signature byte for byte.
        assert_eq!(
            read_text(&root.join("share/katana/installed.json")),
            b"{\"product\":\"katana\",\"schema\":1,\"sequence\":1792312800,\"tag\":\"v0.10.0\",\
              \"target\":\"win64\",\"version\":\"0.10.0\"}\n"
        );
        assert_eq!(
            read_text(&root.join("share/katana/installed.manifest.json")),
            read_text(&fixture_folder().join("release/v2/Katana-update-win64.json"))
        );
        assert_eq!(
            read_text(&root.join("share/katana/installed.manifest.json.sig")),
            read_text(&fixture_folder().join("release/v2/Katana-update-win64.json.sig"))
        );
        // install.json is the installer's and the update never writes it: it is still the record
        // the scenario gave this install - the harness's, which names its package's version, and
        // not the fixture tree's own, which says its target and no more.
        assert_eq!(
            read_text(&root.join("share/katana/install.json")),
            INSTALL_JSON.as_bytes()
        );
    }

    #[test]
    fn the_stage_is_cleaned_after_a_healthy_update_and_the_marker_goes() {
        let s = fixture_scenario("apply-clean");
        let disk = StdFs;
        let rig = Rig::new(&s, &disk);
        assert_eq!(rig.run().outcome, ApplyOutcome::Committed);
        assert!(!is_present(&s.stage.join("bin/katana.exe")));
        assert!(!is_present(&s.stage.join("manifest.json")));
        assert!(!is_present(&s.stage.join("manifest.json.sig")));
        assert!(!is_present(&s.stage), "the folder goes once it is empty");
        assert!(!is_present(&health_marker_path(&s.state, &rig.tx)));
    }

    #[test]
    fn only_files_the_manifest_names_are_deleted_from_the_stage_because_its_path_came_from_a_command_line()
     {
        let s = fixture_scenario("apply-stage-careful");
        write_text(
            &s.stage.join("mine.txt"),
            "the person's file, which happens to be in the folder named as the stage",
        );
        write_text(&s.stage.join("sub/also-mine.txt"), "x");
        let disk = StdFs;
        let rig = Rig::new(&s, &disk);
        assert_eq!(rig.run().outcome, ApplyOutcome::Committed);
        assert_eq!(
            read_text(&s.stage.join("mine.txt")),
            b"the person's file, which happens to be in the folder named as the stage"
        );
        assert_eq!(read_text(&s.stage.join("sub/also-mine.txt")), b"x");
        assert!(
            !is_present(&s.stage.join("bin")),
            "the manifest's own folders emptied"
        );
    }

    #[test]
    fn it_starts_the_new_version_to_check_it_and_then_relaunches_the_program_it_was_told_to() {
        let s = fixture_scenario("apply-launches");
        let disk = StdFs;
        let rig = Rig::new(&s, &disk);
        assert_eq!(rig.run().outcome, ApplyOutcome::Committed);

        // The health check: R/bin/katana --update-health <tx>, waited for, with the health wait.
        let ran = rig.launcher.ran();
        assert_eq!(ran.len(), 1);
        assert_eq!(ran[0].executable, installed_program(&s.root, "katana"));
        assert_eq!(
            ran[0].arguments,
            [String::from("--update-health"), rig.tx.clone()]
        );
        assert_eq!(ran[0].timeout, rig.timings.health_wait);
        // And then the relaunch, detached, exactly once.
        let started = rig.launcher.started();
        assert_eq!(started.len(), 1);
        assert_eq!(started[0].executable, rig.request.relaunch);
    }

    #[test]
    fn no_relaunch_path_means_nothing_is_started_afterwards() {
        let s = fixture_scenario("apply-no-relaunch");
        let disk = StdFs;
        let mut rig = Rig::new(&s, &disk);
        rig.request.relaunch = PathBuf::new();
        assert_eq!(rig.run().outcome, ApplyOutcome::Committed);
        assert!(rig.launcher.started().is_empty());
        assert_eq!(rig.launcher.ran().len(), 1, "the health check still runs");
    }

    #[test]
    fn the_outcome_is_written_to_last_json() {
        let s = fixture_scenario("apply-last");
        let disk = StdFs;
        let rig = Rig::new(&s, &disk);
        assert_eq!(rig.run().outcome, ApplyOutcome::Committed);
        let last = last_of(&s);
        assert_eq!(last.status, "committed");
        assert_eq!(last.version, "0.10.0");
        assert_eq!(last.transaction, rig.tx);
        assert!(last.holder.is_empty());
    }

    #[test]
    fn the_run_once_entry_is_registered_before_the_swap_and_cleared_afterwards() {
        let s = fixture_scenario("apply-runonce");
        let disk = StdFs;
        let rig = Rig::new(&s, &disk);
        // The launcher reads the registry as it stands while the new version is being checked:
        // both are made beside the rig (the file's top says why).
        let registry = FakeRegistry::default();
        let during: RefCell<BTreeMap<String, String>> = RefCell::new(BTreeMap::new());
        let healthy = healthy_start(&s.state, &rig.tx);
        let launcher = FakeLauncher::default();
        launcher.set_on_run(|request: &SpawnRequest| {
            *during.borrow_mut() = registry.run_once.borrow().clone();
            healthy(request)
        });
        assert_eq!(
            run_with_doubles(&rig, &rig.processes, &launcher, &registry).outcome,
            ApplyOutcome::Committed
        );
        let during = during.borrow();
        assert!(during.contains_key(RUN_ONCE_NAME));
        // The helper copy re-run in recovery mode at the next logon, root quoted when it needs
        // it.
        let root = text(&s.root);
        let helper = text(&rig.request.helper);
        assert_eq!(
            during[RUN_ONCE_NAME],
            format!("\"{helper}\" --recover {}", quote_argument(&root))
        );
        assert!(
            registry.run_once.borrow().is_empty(),
            "cleared when the transaction ended"
        );
    }

    #[test]
    fn a_root_with_a_space_in_it_is_quoted_in_the_run_once_command() {
        let mut s = Scenario::new("apply-runonce-space");
        // Move the whole scenario under a folder with a space in its name.
        let spaced = s.scratch.join("Program Files").join("Katana Install");
        build_fixture_scenario(&mut s);
        std::fs::create_dir_all(spaced.parent().unwrap_or(Path::new("")))
            .expect("the folder can be made");
        std::fs::rename(&s.root, &spaced).expect("the install can be moved");
        s.root = spaced.clone();
        let disk = StdFs;
        let rig = Rig::new(&s, &disk);
        rig.launcher.set_on_run(healthy_start(&s.state, &rig.tx));
        assert_eq!(rig.run().outcome, ApplyOutcome::Committed);
        let history = rig.registry.run_once_history.borrow();
        assert_eq!(history.len(), 1);
        assert_eq!(
            history[0],
            format!(
                "\"{}\" --recover \"{}\"",
                text(&rig.request.helper),
                text(&spaced)
            )
        );
    }

    #[test]
    fn an_installer_kind_install_has_its_uninstall_entry_rewritten_an_archive_does_not() {
        {
            let s = fixture_scenario("apply-registry-installer");
            let disk = StdFs;
            let mut rig = Rig::new(&s, &disk);
            // HKCU Software\Jarada\Katana equals R.
            rig.registry.location = Some(s.root.clone());
            assert_eq!(rig.run().outcome, ApplyOutcome::Committed);
            let versions = rig.registry.versions.borrow();
            assert_eq!(versions.len(), 1);
            assert_eq!(versions[0].0, "0.10.0");
            assert_eq!(versions[0].1, "Katana 0.10.0");
        }
        {
            let s = fixture_scenario("apply-registry-archive");
            let disk = StdFs;
            let rig = Rig::new(&s, &disk);
            assert_eq!(rig.run().outcome, ApplyOutcome::Committed);
            assert!(rig.registry.versions.borrow().is_empty());
        }
    }

    #[test]
    fn the_forty_file_update_keeps_what_the_person_changed_and_moves_the_rest() {
        crate::require_can_sign!();
        let mut s = Scenario::new("apply-forty");
        build_forty_file_scenario(&mut s);
        assert_eq!(s.v2.files.len(), 40);
        assert_eq!(s.v1.files.len(), 38);
        let disk = StdFs;
        let rig = Rig::new(&s, &disk);
        let result = rig.run();
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

        // The same, one fact at a time, from the specification.
        assert_eq!(
            read_text(&s.root.join("etc/katana/settings.ini")),
            b"user=edited\n",
            "a seed is never rewritten"
        );
        assert_eq!(
            read_text(&s.root.join("bin/old2.dll")),
            b"the person's own old2\n",
            "removed from the release but changed by the person: left"
        );
        assert!(
            !is_present(&s.root.join("bin/old1.dll")),
            "removed from the release, unchanged: removed"
        );
        assert!(
            !is_present(&s.root.join("share/katana/legacy")),
            "and its emptied folder with it"
        );
        assert_eq!(
            read_text(&s.root.join("share/katana/data/f05.dat")),
            s.v2.find("share/katana/data/f05.dat")
                .expect("v2 lists f05")
                .content,
            "a file the release lists and the person edited is replaced"
        );
        assert_eq!(
            read_text(&s.root.join("share/katana/user_notes.txt")),
            b"notes the update never heard of\n"
        );
        assert_eq!(
            read_text(&s.root.join("bin/user_tool.dll")),
            b"a plug-in nobody listed\n"
        );
        assert!(s.root.join("share/katana/more").is_dir());
        assert!(s.root.join("lib/plugins/extra").is_dir());
    }

    #[test]
    fn a_repeated_update_to_the_same_release_is_refused_as_not_newer() {
        let s = fixture_scenario("apply-twice");
        let disk = StdFs;
        {
            let first = Rig::with_transaction(&s, &disk, "aaaa");
            assert_eq!(first.run().outcome, ApplyOutcome::Committed);
        }
        // The install is v2 now, with a record that says so; the stage is gone, so put it back.
        let installed = tree_hash(&s.root);
        let _ = std::fs::remove_dir_all(&s.stage);
        let fixtures = fixture_folder();
        copy_tree(&fixtures.join("install/v2"), &s.stage);
        write_text(
            &s.stage.join("manifest.json"),
            read_text(&fixtures.join("release/v2/Katana-update-win64.json")),
        );
        write_text(
            &s.stage.join("manifest.json.sig"),
            read_text(&fixtures.join("release/v2/Katana-update-win64.json.sig")),
        );
        let second = Rig::with_transaction(&s, &disk, "bbbb");
        let result = second.run();
        assert_eq!(result.outcome, ApplyOutcome::Refused);
        assert_eq!(result.reason, "downgrade", "{}", result.detail);
        assert_eq!(tree_hash(&s.root), installed);
    }

    #[test]
    fn the_releases_written_out_here_are_above_the_version_of_any_build() {
        // The second premise at the top (OLD_VERSION): no build of Katana is as high as the
        // releases this file writes out, so installed.json is the higher of the two even where
        // the asking program IS asked - under an install.json that says its target and no more.
        // The harness's names its version, and the reading below is the record's for that reason
        // already (the next case); this is what is left should it stop naming one. Without
        // either, the cases that update these releases would rest on the version this suite
        // happened to be built as, and say nothing when it stopped holding.
        let s = small_scenario("apply-small-versions");
        let own = program_version().unwrap_or_else(|error| panic!("{}", error.describe()));
        let old = parse_version(OLD_VERSION).expect("the old version reads");
        let next = parse_version(NEW_VERSION).expect("the new version reads");
        assert!(own < old, "this build is {}", program_version_text());
        assert!(old < next);

        let registry = FakeRegistry::default();
        let install =
            read_install(&s.root, &registry).unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(
            version_text(&install.version),
            OLD_VERSION,
            "not the version installed.json records"
        );
        assert_eq!(install.sequence, 100);
    }

    #[test]
    fn an_install_the_harness_writes_is_read_as_its_records_say_and_not_as_this_build() {
        // The first premise at the top, and the only one the fixture and the forty-file cases
        // have: the harness's install.json names the version of its package (INSTALL_JSON), so
        // the build that reads the install is not asked for its own. Shown where that makes a
        // difference whichever build this is: a release BELOW this build's version - 0.1.0, and
        // the project has been above it since 0.2.0 - which an install.json that said its target
        // alone would read as the build's, the higher of the two. What is expected is what the
        // release below is given: its version and its sequence, as installed.json records them.
        let own = program_version().unwrap_or_else(|error| panic!("{}", error.describe()));
        let below = parse_version("0.1.0").expect("0.1.0 reads");
        assert!(below < own, "this build is {}", program_version_text());

        let mut s = Scenario::new("apply-harness-record");
        s.v1 = small_release("0.1.0", 100);
        write_install(&s.root, &s.v1);
        let registry = FakeRegistry::default();
        let install =
            read_install(&s.root, &registry).unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(
            version_text(&install.version),
            "0.1.0",
            "read as this build's version, not the record's"
        );
        assert_eq!(install.sequence, 100);
        assert!(install.has_installed_record);
    }

    // ---- what is left alone, what is planned, and where a rollback must reach ----------------

    #[test]
    fn files_that_are_already_right_are_neither_copied_nor_moved_and_the_plan_names_exactly_what_changes()
     {
        let s = fixture_scenario("apply-keep");
        let disk = StdFs;
        let log = LogFs::new(&disk);
        let rig = Rig::new(&s, &log);
        assert_eq!(rig.run().outcome, ApplyOutcome::Committed);

        // Read off the T3 fixtures' two manifests by hand: katana_cli.exe, libcore-1.dll,
        // empty.txt and random.bin have the same SHA-256 in v1 and v2, and settings.ini is a seed
        // that is already there. Not one of them may be copied or moved - this is the whole point
        // of deciding by hash, and the tree comes out the same if it is not done.
        let entries = log.log.borrow();
        for kept in [
            "katana_cli.exe",
            "libcore-1.dll",
            "empty.txt",
            "random.bin",
            "settings.ini",
        ] {
            for operation in entries.iter() {
                if operation.starts_with("copyFile") || operation.starts_with("renameFile") {
                    assert!(!operation.contains(kept), "{kept} was touched: {operation}");
                }
            }
        }

        // The PLAN group lists exactly the files that change, are added or are removed, and the
        // folder that has to be made - and the three records the helper writes.
        let at = log.find("appendDurable", " 1 PLAN ", 1);
        assert!(at > 0);
        let line = &entries[(at - 1) as usize];
        let start = line
            .find(" 1 PLAN ")
            .expect("the PLAN group is in the entry")
            + 1;
        let plan = parse_journal(&line.as_bytes()[start..]);
        let mut planned = BTreeSet::new();
        for record in &plan.records {
            if record.op != JournalOp::Plan {
                assert_eq!(record.args.len(), 1);
                planned.insert(format!(
                    "{} {}",
                    op_name(record.op),
                    String::from_utf8_lossy(&record.args[0])
                ));
            }
        }
        let expected: BTreeSet<String> = [
            "REPLACE bin/katana.exe",
            "REPLACE share/katana/readme.txt",
            "ADD bin/libnew-2.dll",
            "ADD share/katana/help/index.txt",
            "ADD share/katana/news.txt",
            "REMOVE bin/libold-1.dll",
            "REMOVE share/katana/legacy/old.txt",
            "REPLACE share/katana/installed.json",
            "REPLACE share/katana/installed.manifest.json",
            "REPLACE share/katana/installed.manifest.json.sig",
            "MKDIR share/katana/help",
        ]
        .iter()
        .map(|entry| String::from(*entry))
        .collect();
        assert_eq!(planned, expected);
        // And only those were copied from the stage: five files and nothing else (the records are
        // written, not copied).
        drop(entries);
        assert_eq!(log.all("copyFile").len(), 5);
    }

    #[test]
    fn an_install_folder_given_with_a_trailing_separator_is_the_same_folder() {
        // A shell's tab completion adds the separator: katana_updater --recover "C:\...\Katana\".
        let s = fixture_scenario("apply-trailing-separator");
        let disk = StdFs;
        let mut rig = Rig::new(&s, &disk);
        let mut spelled = path_to_utf8(&s.root);
        spelled.push(b'/');
        rig.request.root = path_from_utf8(spelled);
        let result = rig.run();
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
    }

    #[test]
    fn the_uninstall_entry_names_the_new_version_only_once_it_has_proved_it_starts() {
        {
            let s = fixture_scenario("apply-registry-after-health");
            let disk = StdFs;
            let rig = Rig::new(&s, &disk);
            // The launcher reads the registry while the new version is being checked: both are
            // made beside the rig (the file's top says why).
            let registry = FakeRegistry {
                location: Some(s.root.clone()),
                ..FakeRegistry::default()
            };
            let written_while_checking = Cell::new(99usize);
            let healthy = healthy_start(&s.state, &rig.tx);
            let launcher = FakeLauncher::default();
            launcher.set_on_run(|request: &SpawnRequest| {
                written_while_checking.set(registry.versions.borrow().len());
                healthy(request)
            });
            assert_eq!(
                run_with_doubles(&rig, &rig.processes, &launcher, &registry).outcome,
                ApplyOutcome::Committed
            );
            assert_eq!(
                written_while_checking.get(),
                0,
                "not before the new version has started"
            );
            let versions = registry.versions.borrow();
            assert_eq!(versions.len(), 1);
            assert_eq!(versions[0].0, "0.10.0");
        }
        {
            // A release that is rolled back must not be the one Settings > Apps names.
            let s = fixture_scenario("apply-registry-rolled-back");
            let disk = StdFs;
            let mut rig = Rig::new(&s, &disk);
            rig.registry.location = Some(s.root.clone());
            rig.launcher
                .set_on_run(|_request: &SpawnRequest| Ok(exited(3)));
            assert_eq!(rig.run().outcome, ApplyOutcome::RolledBack);
            assert!(rig.registry.versions.borrow().is_empty());
        }
    }
}

// ---- refusals: the signature, the release, the install ------------------------------------------

mod apply_refusals {
    use super::*;

    #[test]
    fn a_tree_that_is_not_an_install_is_never_updated() {
        let mut s = fixture_scenario("apply-development");
        // The mark of the installer's CPack step.
        remove(&s.root.join("share/katana/install.json"));
        s.v1_hash = tree_hash(&s.root);
        s.v1_snapshot = snapshot(&s.root);
        let disk = StdFs;
        let rig = Rig::new(&s, &disk);
        let result = rig.run();
        assert_eq!(result.outcome, ApplyOutcome::Refused);
        assert_eq!(result.reason, "development_tree");
        assert_eq!(tree_hash(&s.root), s.v1_hash);
        assert!(
            !is_present(&s.root.join(".update")),
            "not even the work folder is made"
        );
    }

    #[test]
    fn a_missing_staged_manifest_or_signature_is_refused_and_nothing_is_touched() {
        {
            let s = fixture_scenario("apply-no-manifest");
            remove(&s.stage.join("manifest.json"));
            let disk = StdFs;
            let rig = Rig::new(&s, &disk);
            expect_refused(&s, &rig.run(), "staged_manifest");
        }
        {
            let s = fixture_scenario("apply-no-signature");
            remove(&s.stage.join("manifest.json.sig"));
            let disk = StdFs;
            let rig = Rig::new(&s, &disk);
            expect_refused(&s, &rig.run(), "staged_manifest");
        }
    }

    #[test]
    fn the_helper_verifies_the_staged_manifest_with_its_own_keys_and_refuses_what_fails_them() {
        // Each of the fixtures' bad releases, staged as if the engine had been fooled into
        // staging it. They are the cases the engine's own tests refuse; the helper must refuse
        // them too, because it does not trust the engine's answer.
        for bad in [
            "flipped-manifest-bit",
            "swapped-sig",
            "wrong-key",
            "wrong-key-claims-active",
            "path-traversal",
        ] {
            let mut s = Scenario::new(&format!("apply-bad-{bad}"));
            build_fixture_scenario_of(&mut s, "v1", &format!("bad/{bad}"));
            let disk = StdFs;
            let rig = Rig::new(&s, &disk);
            let result = rig.run();
            assert_eq!(result.outcome, ApplyOutcome::Refused, "{bad}");
            assert_eq!(result.reason, "staged_manifest", "{bad}: {}", result.detail);
            expect_untouched(&s, &result);
            // And the person gets the program back.
            assert_eq!(rig.launcher.started().len(), 1, "{bad}");
        }
    }

    #[test]
    fn a_manifest_signed_by_a_key_this_build_does_not_trust_is_refused() {
        let s = fixture_scenario("apply-untrusted");
        // The real production keys are placeholders in a development build; the test trust is
        // the one in the services. Swap in a trust that holds only the spare key: the active key
        // signed this release, so it no longer verifies.
        let only_spare = Trust {
            keys: vec![test_trust().keys[1].clone()],
            ..Trust::default()
        };
        let disk = StdFs;
        let rig = Rig::new(&s, &disk);
        let services = rig.services();
        let narrowed = ApplyServices {
            trust: &only_spare,
            ..services
        };
        let result = apply(&rig.request, &narrowed, None);
        expect_refused(&s, &result, "staged_manifest");
    }

    #[test]
    fn a_release_that_is_not_newer_than_the_install_is_refused_on_version_and_on_sequence() {
        for bad in ["downgrade-version", "downgrade-sequence"] {
            let mut s = Scenario::new(&format!("apply-{bad}"));
            // v2 is installed.
            build_fixture_scenario_of(&mut s, "v2", &format!("bad/{bad}"));
            let disk = StdFs;
            let rig = Rig::new(&s, &disk);
            let result = rig.run();
            assert_eq!(result.outcome, ApplyOutcome::Refused, "{bad}");
            assert_eq!(result.reason, "downgrade", "{bad}: {}", result.detail);
            expect_untouched(&s, &result);
        }
    }

    #[test]
    fn a_manifest_for_another_target_is_refused() {
        crate::require_can_sign!();
        let s = small_scenario("apply-other-target");
        let mut other = small_release(NEW_VERSION, 200);
        other.target = String::from("linux-x86_64");
        write_stage(&s.stage, &other);
        let disk = StdFs;
        let rig = Rig::new(&s, &disk);
        let result = rig.run();
        // verify_manifest (trust.rs) is what asks "is this signed for this install's target", with
        // the helper's own code, so that is the token and that is the sentence.
        assert_eq!(result.outcome, ApplyOutcome::Refused);
        assert_eq!(result.reason, "staged_manifest");
        assert!(
            result.detail.contains("another target"),
            "{}",
            result.detail
        );
        expect_untouched(&s, &result);
    }

    #[test]
    fn an_announcement_only_manifest_is_not_an_update() {
        crate::require_can_sign!();
        let s = small_scenario("apply-notify-only");
        let version = NEW_VERSION;
        let json = format!(
            "{{\"applier\":1,\"channel\":\"stable\",\"expires\":\"2030-01-01T00:00:00Z\",\
             \"files\":[],\"notes_url\":\"https://github.com/kjarada/Katana/releases/tag/v{version}\",\
             \"notify_only\":true,\"product\":\"katana\",\"published\":\"2026-10-01T00:00:00Z\",\
             \"schema\":1,\"sequence\":200,\"tag\":\"v{version}\",\"target\":\"win64\",\
             \"version\":\"{version}\"}}\n"
        );
        write_text(&s.stage.join("manifest.json"), &json);
        write_text(
            &s.stage.join("manifest.json.sig"),
            sign_line(json.as_bytes()),
        );
        let disk = StdFs;
        let rig = Rig::new(&s, &disk);
        expect_refused(&s, &rig.run(), "notify_only");
    }

    #[test]
    fn a_manifest_that_needs_a_newer_helper_is_refused() {
        crate::require_can_sign!();
        let s = small_scenario("apply-applier");
        let json = String::from_utf8(manifest_json(&s.v2)).expect("the manifest is text");
        assert!(json.contains("\"applier\":1"));
        let json = json.replacen("\"applier\":1", "\"applier\":2", 1);
        write_text(&s.stage.join("manifest.json"), &json);
        write_text(
            &s.stage.join("manifest.json.sig"),
            sign_line(json.as_bytes()),
        );
        let disk = StdFs;
        let rig = Rig::new(&s, &disk);
        expect_refused(&s, &rig.run(), "applier_too_new");
    }

    #[test]
    fn an_invalid_transaction_id_or_no_root_is_refused_before_anything_is_made() {
        let s = small_scenario("apply-bad-request");
        let disk = StdFs;
        {
            let mut rig = Rig::new(&s, &disk);
            rig.request.transaction = String::from("../../evil");
            let result = rig.run();
            assert_eq!(result.outcome, ApplyOutcome::Refused);
            assert_eq!(result.reason, "invalid_request");
            assert!(!is_present(&s.root.join(".update")));
            assert!(!is_present(&s.scratch.join("evil")));
        }
        {
            let mut rig = Rig::new(&s, &disk);
            rig.request.root = PathBuf::new();
            assert_eq!(rig.run().reason, "invalid_request");
        }
    }

    #[test]
    fn an_empty_state_folder_is_refused_because_the_health_marker_could_not_be_looked_for() {
        let s = fixture_scenario("apply-no-state-folder");
        let disk = StdFs;
        let mut rig = Rig::new(&s, &disk);
        rig.request.state_directory = PathBuf::new();
        let result = rig.run();
        assert_eq!(result.outcome, ApplyOutcome::Refused);
        assert_eq!(result.reason, "invalid_request");
        assert!(
            !is_present(&s.root.join(".update")),
            "refused before anything is made"
        );
        assert_eq!(tree_hash(&s.root), s.v1_hash);
        assert!(
            rig.launcher.ran().is_empty(),
            "no health check without a place for its marker"
        );
    }

    #[test]
    fn a_refused_update_starts_the_old_version_again_and_says_so_in_last_json() {
        let s = fixture_scenario("apply-refused-relaunch");
        remove(&s.stage.join("manifest.json"));
        let disk = StdFs;
        let rig = Rig::new(&s, &disk);
        assert_eq!(rig.run().outcome, ApplyOutcome::Refused);
        let started = rig.launcher.started();
        assert_eq!(started.len(), 1);
        assert_eq!(started[0].executable, rig.request.relaunch);
        assert!(
            rig.launcher.ran().is_empty(),
            "no health check: nothing was installed"
        );
        let last = last_of(&s);
        assert_eq!(last.status, "refused");
        assert_eq!(last.reason, "staged_manifest");
    }
}

// ---- planning: what must be in the stage --------------------------------------------------------

mod apply_plan {
    use super::*;

    #[test]
    fn a_staged_file_that_is_missing_refuses_the_update_and_leaves_nothing_behind() {
        let s = fixture_scenario("apply-staged-missing");
        remove(&s.stage.join("bin/libnew-2.dll"));
        let disk = StdFs;
        let rig = Rig::new(&s, &disk);
        expect_refused(&s, &rig.run(), "staged_missing");
    }

    #[test]
    fn a_staged_file_with_the_wrong_bytes_or_the_wrong_size_is_refused() {
        {
            let s = fixture_scenario("apply-staged-hash");
            let mut bytes = read_text(&s.stage.join("bin/libnew-2.dll"));
            // One bit.
            bytes[3] ^= 0x01;
            write_text(&s.stage.join("bin/libnew-2.dll"), bytes);
            let disk = StdFs;
            let rig = Rig::new(&s, &disk);
            expect_refused(&s, &rig.run(), "staged_hash");
        }
        {
            let s = fixture_scenario("apply-staged-size");
            let mut bytes = read_text(&s.stage.join("bin/libnew-2.dll"));
            bytes.push(b'x');
            write_text(&s.stage.join("bin/libnew-2.dll"), bytes);
            let disk = StdFs;
            let rig = Rig::new(&s, &disk);
            expect_refused(&s, &rig.run(), "staged_hash");
        }
    }

    #[test]
    fn the_hash_is_of_the_copy_not_of_what_the_stage_holds_once_the_copy_is_made() {
        // The stage folder is writable by anything running as the user. A file that is wrong only
        // while it is being copied, and right again before anyone looks, must not be installed:
        // hashing the stage file after the copy would see the right bytes and pass the wrong
        // copy. The copy is what is hashed. Here the stage file holds other bytes of the same
        // length for the length of the copy, and its own again after.
        let s = fixture_scenario("apply-staged-race");
        let disk = StdFs;
        let original: RefCell<Vec<u8>> = RefCell::new(Vec::new());
        let hooked = HookFs::new(&disk);
        hooked.set_before_copy(|from: &Path, _to: &Path| {
            if from.file_name() == Some(OsStr::new("libnew-2.dll")) {
                *original.borrow_mut() = read_text(from);
                // Same length, other bytes.
                let mut swapped = original.borrow().clone();
                swapped.reverse();
                write_text(from, swapped);
            }
            None
        });
        hooked.set_after_copy(|from: &Path, _to: &Path| {
            if from.file_name() == Some(OsStr::new("libnew-2.dll")) {
                write_text(from, original.borrow().as_slice());
            }
        });
        let rig = Rig::new(&s, &hooked);
        expect_refused(&s, &rig.run(), "staged_hash");
    }

    #[test]
    fn the_install_is_not_touched_while_the_plan_copies_and_hashes() {
        // Whatever refusal ends the plan, R is as it was: the same entries, bytes and folders.
        let s = fixture_scenario("apply-plan-readonly");
        // The last file the plan reaches fails.
        remove(&s.stage.join("share/katana/news.txt"));
        let disk = StdFs;
        let rig = Rig::new(&s, &disk);
        expect_refused(&s, &rig.run(), "staged_missing");
        assert_eq!(snapshot(&s.root), s.v1_snapshot);
    }

    #[test]
    fn a_directory_where_a_file_must_go_or_a_file_where_a_directory_must_go_is_refused() {
        {
            let mut s = fixture_scenario("apply-conflict-dir");
            remove(&s.root.join("bin/libcore-1.dll"));
            // A folder in the file's place.
            std::fs::create_dir_all(s.root.join("bin/libcore-1.dll"))
                .expect("the folder can be made");
            s.v1_hash = tree_hash(&s.root);
            s.v1_snapshot = snapshot(&s.root);
            // libcore is unchanged in v2, but the folder is not the file the manifest names.
            let disk = StdFs;
            let rig = Rig::new(&s, &disk);
            expect_refused(&s, &rig.run(), "path_conflict");
        }
        {
            let mut s = fixture_scenario("apply-conflict-file");
            write_text(
                &s.root.join("share/katana/help"),
                "a file where the new folder must be",
            );
            s.v1_hash = tree_hash(&s.root);
            s.v1_snapshot = snapshot(&s.root);
            let disk = StdFs;
            let rig = Rig::new(&s, &disk);
            expect_refused(&s, &rig.run(), "path_conflict");
        }
    }

    #[test]
    fn a_folder_that_is_a_link_or_resolves_outside_the_install_is_refused() {
        {
            let s = fixture_scenario("apply-reparse-parent");
            let disk = StdFs;
            let linked = s.root.join("share/katana");
            let hooked = HookFs::new(&disk);
            hooked.set_adjust_stat(|p: &Path, info: &mut EntryInfo| {
                if p == linked.as_path() {
                    info.reparse_point = true;
                }
            });
            let rig = Rig::new(&s, &hooked);
            expect_refused(&s, &rig.run(), "reparse_point");
        }
        {
            let s = fixture_scenario("apply-resolves-outside");
            let disk = StdFs;
            let bin = s.root.join("bin");
            // What a junction would give.
            let elsewhere = s.scratch.join("somewhere").join("else");
            let hooked = HookFs::new(&disk);
            hooked.set_override_resolve(|p: &Path| {
                if p == bin.as_path() {
                    return Some(elsewhere.clone());
                }
                None
            });
            let rig = Rig::new(&s, &hooked);
            expect_refused(&s, &rig.run(), "reparse_point");
        }
        {
            let s = fixture_scenario("apply-reparse-file");
            let disk = StdFs;
            let linked = s.root.join("bin/katana.exe");
            let hooked = HookFs::new(&disk);
            hooked.set_adjust_stat(|p: &Path, info: &mut EntryInfo| {
                if p == linked.as_path() {
                    info.reparse_point = true;
                }
            });
            let rig = Rig::new(&s, &hooked);
            expect_refused(&s, &rig.run(), "reparse_point");
        }
        {
            let s = fixture_scenario("apply-reparse-update-folder");
            let disk = StdFs;
            let linked = s.root.join(".update");
            let hooked = HookFs::new(&disk);
            hooked.set_adjust_stat(|p: &Path, info: &mut EntryInfo| {
                if p == linked.as_path() {
                    info.reparse_point = true;
                }
            });
            let rig = Rig::new(&s, &hooked);
            let result = rig.run();
            assert_eq!(result.outcome, ApplyOutcome::Refused);
            assert_eq!(result.reason, "reparse_point");
            assert_eq!(tree_hash(&s.root), s.v1_hash);
        }
    }

    #[test]
    fn not_enough_free_space_refuses_before_anything_is_copied() {
        let s = fixture_scenario("apply-space");
        let disk = StdFs;
        let log = LogFs::new(&disk);
        let hooked = HookFs::new(&log);
        // The copies need more than a megabyte of margin alone.
        hooked.free_bytes.set(Some(100));
        let rig = Rig::new(&s, &hooked);
        expect_refused(&s, &rig.run(), "space");
        // No copy was started, and nothing was made in a transaction folder.
        assert_eq!(log.find("copyFile", "", 1), 0);
        assert_eq!(log.find("renameFile", "", 1), 0);
        assert_eq!(log.find("writeDurable", "", 1), 0);
        assert_eq!(log.find("makeDirectories", "new", 1), 0);
    }

    #[test]
    fn a_transaction_folder_that_is_already_there_is_refused_and_never_deleted() {
        // The same id again would be a folder some other run made - maybe a half swap waiting for
        // its recovery. It is not this run's to touch.
        let s = fixture_scenario("apply-tx-exists");
        let work = s.root.join(".update").join("a1b2c3d4");
        write_text(&work.join("old/bin/katana.exe"), "somebody's old file");
        write_text(&work.join("journal"), "");
        let disk = StdFs;
        let rig = Rig::new(&s, &disk);
        let result = rig.run();
        assert_eq!(result.outcome, ApplyOutcome::Refused);
        assert_eq!(result.reason, "transaction_exists");
        assert_eq!(tree_hash(&s.root), s.v1_hash);
        assert_eq!(
            read_text(&work.join("old/bin/katana.exe")),
            b"somebody's old file"
        );
    }

    #[test]
    fn a_full_disk_during_a_copy_refuses_the_update_and_removes_what_was_copied() {
        let s = fixture_scenario("apply-disk-full");
        let disk = StdFs;
        let copies = Cell::new(0);
        let hooked = HookFs::new(&disk);
        hooked.set_before_copy(|_from: &Path, to: &Path| {
            copies.set(copies.get() + 1);
            if copies.get() == 3 {
                // ERROR_DISK_FULL
                return Some(injected(112, to));
            }
            None
        });
        let rig = Rig::new(&s, &hooked);
        let result = rig.run();
        expect_refused(&s, &result, "copy_failed");
        assert!(result.detail.contains("112"), "{}", result.detail);
    }

    #[test]
    fn a_journal_that_cannot_be_written_refuses_the_update_before_anything_moves() {
        let s = fixture_scenario("apply-journal-plan");
        let disk = StdFs;
        let hooked = HookFs::new(&disk);
        hooked.set_before_append(|p: &Path, _bytes: &[u8]| {
            if p.file_name() == Some(OsStr::new("journal")) {
                return Some(injected(112, p));
            }
            None
        });
        let rig = Rig::new(&s, &hooked);
        expect_refused(&s, &rig.run(), "journal");
    }

    #[test]
    fn a_cancel_before_the_commit_touches_nothing() {
        {
            let s = fixture_scenario("apply-cancel-early");
            let disk = StdFs;
            let rig = Rig::new(&s, &disk);
            let stop = AtomicBool::new(true);
            expect_refused(&s, &rig.run_stopped(Some(&stop)), "cancelled");
        }
        {
            // Asked for in the middle of the plan: after the third file has been examined.
            let s = fixture_scenario("apply-cancel-mid");
            let disk = StdFs;
            let stop = AtomicBool::new(false);
            let examined = Cell::new(0);
            let bin = s.root.join("bin");
            let hooked = HookFs::new(&disk);
            hooked.set_adjust_stat(|p: &Path, _info: &mut EntryInfo| {
                if p.parent() == Some(bin.as_path()) {
                    examined.set(examined.get() + 1);
                    if examined.get() == 3 {
                        stop.store(true, Ordering::SeqCst);
                    }
                }
            });
            let rig = Rig::new(&s, &hooked);
            expect_refused(&s, &rig.run_stopped(Some(&stop)), "cancelled");
        }
    }

    #[test]
    fn a_stop_requested_once_the_commit_has_begun_is_ignored_and_the_swap_is_finished() {
        let s = fixture_scenario("apply-cancel-late");
        let disk = StdFs;
        let stop = AtomicBool::new(false);
        let hooked = HookFs::new(&disk);
        hooked.set_before_rename(|_from: &Path, _to: &Path| {
            // At the first rename of the commit.
            stop.store(true, Ordering::SeqCst);
            None
        });
        let rig = Rig::new(&s, &hooked);
        let result = rig.run_stopped(Some(&stop));
        assert_eq!(
            result.outcome,
            ApplyOutcome::Committed,
            "a half swap is finished or rolled back, never abandoned"
        );
        assert_eq!(tree_hash(&s.root), s.v2_hash);
    }

    #[test]
    fn the_folder_of_the_records_is_held_to_the_same_rule_as_every_other_folder() {
        crate::require_can_sign!();
        // No file of either release lives in share/katana, so only the three records the helper
        // itself writes there would go through a link at share/katana.
        let mut s = Scenario::new("apply-reparse-records");
        s.v1 = small_release(OLD_VERSION, 100);
        s.v2 = small_release(NEW_VERSION, 200);
        for release in [&mut s.v1, &mut s.v2] {
            for file in &mut release.files {
                if file.path == "share/katana/data.txt" {
                    file.path = String::from("share/other/data.txt");
                }
            }
        }
        write_install(&s.root, &s.v1);
        write_stage(&s.stage, &s.v2);
        s.v1_hash = tree_hash(&s.root);
        s.v1_snapshot = snapshot(&s.root);
        let disk = StdFs;
        let linked = s.root.join("share/katana");
        let hooked = HookFs::new(&disk);
        hooked.set_adjust_stat(|p: &Path, info: &mut EntryInfo| {
            if p == linked.as_path() {
                info.reparse_point = true;
            }
        });
        let rig = Rig::new(&s, &hooked);
        expect_refused(&s, &rig.run(), "reparse_point");
    }
}

// ---- the lock -----------------------------------------------------------------------------------

mod apply_lock {
    use super::*;

    #[test]
    fn a_held_lock_refuses_the_update_and_leaves_the_other_helpers_outcome_alone() {
        let s = fixture_scenario("apply-locked");
        let disk = StdFs;
        std::fs::create_dir_all(s.root.join(".update")).expect("the folder can be made");
        let other = disk.try_lock(&s.root.join(".update").join("lock"));
        assert!(other.is_ok());

        let rig = Rig::new(&s, &disk);
        let result = rig.run();
        assert_eq!(result.outcome, ApplyOutcome::Refused);
        assert_eq!(result.reason, "locked");
        assert_eq!(tree_hash(&s.root), s.v1_hash);
        assert!(
            rig.launcher.started().is_empty(),
            "the other helper starts the program"
        );
        assert!(rig.launcher.ran().is_empty());
        let last = read_last_result(&s.state).expect("last.json is read");
        assert!(last.is_none(), "it is the other helper's to write");
        assert!(rig.registry.run_once.borrow().is_empty());
    }

    #[test]
    fn two_helpers_at_once_one_updates_and_the_other_is_refused() {
        let s = fixture_scenario("apply-contend");
        let release = Arc::new(AtomicBool::new(false));
        let scans = Arc::new(AtomicI32::new(0));

        let (first_result, second_result) = std::thread::scope(|scope| {
            // The first helper's rig is made on its own thread (the file's top says why).
            let running = scope.spawn(|| {
                let disk = StdFs;
                let mut first = Rig::with_transaction(&s, &disk, "aaaa");
                first.processes.holders = vec![RunningProgram {
                    pid: 4412,
                    image: path_from_utf8("katana_mcp.exe"),
                }];
                // Keeps the first helper waiting, lock held, until released.
                first.processes.holders_for = -1;
                first.processes.release = Arc::clone(&release);
                first.processes.scans = Arc::clone(&scans);
                first.timings.wait_for_holders = Duration::from_secs(60);
                first.timings.poll_interval = Duration::from_millis(2);
                first.run()
            });

            // Once the first is scanning for holders it holds the lock.
            for _ in 0..5000 {
                if scans.load(Ordering::SeqCst) != 0 {
                    break;
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            let looked = scans.load(Ordering::SeqCst) > 0;
            let disk = StdFs;
            let second = Rig::with_transaction(&s, &disk, "bbbb");
            let second_result = second.run();
            // Only now does the first helper go on.
            release.store(true, Ordering::SeqCst);
            let first_result = running.join().expect("the first helper's thread");
            assert!(looked, "the first helper never scanned for holders");
            (first_result, second_result)
        });

        assert_eq!(second_result.outcome, ApplyOutcome::Refused);
        assert_eq!(second_result.reason, "locked");
        assert_eq!(
            first_result.outcome,
            ApplyOutcome::Committed,
            "{}{}",
            first_result.reason,
            first_result.detail
        );
        assert_eq!(tree_hash(&s.root), s.v2_hash);
    }

    #[test]
    fn the_lock_is_free_again_after_the_update() {
        let s = fixture_scenario("apply-lock-free");
        let disk = StdFs;
        let rig = Rig::new(&s, &disk);
        assert_eq!(rig.run().outcome, ApplyOutcome::Committed);
        assert!(
            disk.try_lock(&s.root.join(".update").join("lock")).is_ok(),
            "released before the relaunch, whose first act is recover()"
        );
    }

    #[test]
    fn a_root_that_cannot_be_written_is_reported_as_such() {
        let s = fixture_scenario("apply-unwritable");
        let disk = StdFs;
        // The C++ subclasses its forwarding file system to refuse every folder.
        let refusing = ForwardingFs::new(&disk);
        refusing.on_make_directories(|_inner, p: &Path| {
            Err(os_error(
                ErrorCode::FileExportFailure,
                "access denied",
                p,
                5,
            ))
        });
        let rig = Rig::new(&s, &refusing);
        let result = rig.run();
        assert_eq!(result.outcome, ApplyOutcome::Refused);
        assert_eq!(result.reason, "root_not_writable", "{}", result.detail);
        assert_eq!(tree_hash(&s.root), s.v1_hash);
    }

    #[test]
    fn a_lock_that_is_held_only_for_a_moment_is_waited_for_not_mistaken_for_another_helper() {
        let s = fixture_scenario("apply-lock-blip");
        let disk = StdFs;
        let attempts = Cell::new(0);
        let busy = busy_lock_fs(&disk, 3, &attempts);
        let mut rig = Rig::new(&s, &busy);
        // The grace is the shorter of this and 2 s.
        rig.timings.wait_for_holders = Duration::from_secs(5);
        let result = rig.run();
        assert_eq!(
            result.outcome,
            ApplyOutcome::Committed,
            "{}: {}",
            result.reason,
            result.detail
        );
        assert_eq!(attempts.get(), 4, "three refusals, then the lock");
        assert_eq!(tree_hash(&s.root), s.v2_hash);
    }

    #[test]
    fn a_lock_that_stays_held_is_looked_at_again_and_then_left_to_its_holder_in_silence() {
        let s = fixture_scenario("apply-lock-stays");
        let disk = StdFs;
        let attempts = Cell::new(0);
        let busy = busy_lock_fs(&disk, -1, &attempts);
        let rig = Rig::new(&s, &busy);
        let result = rig.run();
        assert_eq!(result.outcome, ApplyOutcome::Refused);
        assert_eq!(result.reason, "locked");
        assert!(
            attempts.get() > 1,
            "it looked again before it believed the lock was held"
        );
        assert_eq!(tree_hash(&s.root), s.v1_hash);
        assert!(
            rig.launcher.started().is_empty(),
            "the holder starts the program"
        );
        let last = read_last_result(&s.state).expect("last.json is read");
        assert!(last.is_none(), "it is the other helper's to write");
    }
}

// ---- waiting for the window and for everyone under the root -------------------------------------

mod apply_wait {
    use super::*;

    #[test]
    fn programs_still_running_under_the_root_block_the_update_and_nothing_is_touched() {
        let s = fixture_scenario("apply-blocked");
        let disk = StdFs;
        let mut rig = Rig::new(&s, &disk);
        rig.processes.holders = vec![RunningProgram {
            pid: 4412,
            image: path_from_utf8("C:/k/bin/katana_mcp.exe"),
        }];
        // Never goes away.
        rig.processes.holders_for = -1;

        let result = rig.run();
        assert_eq!(result.outcome, ApplyOutcome::Blocked);
        assert_eq!(result.reason, "blocked");
        assert_eq!(result.holders.len(), 1);
        assert_eq!(result.holders[0].pid, 4412);
        assert_eq!(
            result.detail,
            "Close these first: katana_mcp.exe (4412). Then press Retry."
        );
        expect_untouched(&s, &result);
        assert!(
            rig.processes.scans.load(Ordering::SeqCst) > 1,
            "it waited, polling, before it gave up"
        );

        // The old version is started again, nothing else is run, and the outcome is recorded.
        let started = rig.launcher.started();
        assert_eq!(started.len(), 1);
        assert_eq!(started[0].executable, rig.request.relaunch);
        assert!(rig.launcher.ran().is_empty());
        let last = last_of(&s);
        assert_eq!(last.status, "blocked");
        assert_eq!(last.holder, "katana_mcp.exe (4412)");
    }

    #[test]
    fn every_holder_is_named_and_none_is_killed() {
        let s = fixture_scenario("apply-blocked-many");
        let disk = StdFs;
        let mut rig = Rig::new(&s, &disk);
        rig.processes.holders = vec![
            RunningProgram {
                pid: 10,
                image: path_from_utf8("C:/k/bin/katana_cli.exe"),
            },
            RunningProgram {
                pid: 20,
                image: path_from_utf8("C:/k/bin/katana_mcp.exe"),
            },
        ];
        rig.processes.holders_for = -1;
        let result = rig.run();
        assert_eq!(result.outcome, ApplyOutcome::Blocked);
        assert_eq!(
            result.detail,
            "Close these first: katana_cli.exe (10), katana_mcp.exe (20). Then press Retry."
        );
        assert_eq!(
            last_of(&s).holder,
            "katana_cli.exe (10), katana_mcp.exe (20)"
        );
        // The seam has no way to kill: ProcessOps is "who is running" and nothing more.
    }

    #[test]
    fn a_program_that_exits_while_the_update_waits_lets_it_go_ahead() {
        let s = fixture_scenario("apply-holder-leaves");
        let disk = StdFs;
        let mut rig = Rig::new(&s, &disk);
        rig.processes.holders = vec![RunningProgram {
            pid: 4412,
            image: path_from_utf8("C:/k/bin/katana_mcp.exe"),
        }];
        rig.processes.holders_for = 5;
        rig.timings.wait_for_holders = Duration::from_millis(5000);
        let result = rig.run();
        assert_eq!(
            result.outcome,
            ApplyOutcome::Committed,
            "{}{}",
            result.reason,
            result.detail
        );
        // Five answers that name the program, a sixth that names nobody and ends the wait - and
        // one more: the same question is asked again when the plan is made, before the journal
        // says COMMITTING (apply.rs, step 4; the cases of that second look are further down).
        assert_eq!(rig.processes.scans.load(Ordering::SeqCst), 7);
        assert_eq!(tree_hash(&s.root), s.v2_hash);
    }

    #[test]
    fn the_helpers_own_process_is_not_a_holder() {
        let s = fixture_scenario("apply-self");
        let disk = StdFs;
        let mut rig = Rig::new(&s, &disk);
        rig.processes.pid = 999;
        rig.processes.holders = vec![RunningProgram {
            pid: 999,
            image: path_from_utf8("C:/k/bin/katana_updater.exe"),
        }];
        rig.processes.holders_for = -1;
        assert_eq!(rig.run().outcome, ApplyOutcome::Committed);
    }

    #[test]
    fn a_window_that_does_not_exit_blocks_the_update() {
        let s = fixture_scenario("apply-parent-stays");
        let disk = StdFs;
        let mut rig = Rig::new(&s, &disk);
        rig.request.wait_pid = 77;
        rig.processes.parent_alive_for.set(-1);
        let result = rig.run();
        assert_eq!(result.outcome, ApplyOutcome::Blocked);
        assert_eq!(result.holders.len(), 1);
        assert_eq!(result.holders[0].pid, 77);
        expect_untouched(&s, &result);
        assert_eq!(
            rig.processes.scans.load(Ordering::SeqCst),
            0,
            "the files are not even looked at before the window is gone"
        );
    }

    #[test]
    fn a_window_that_is_still_running_is_not_started_a_second_time_beside_itself() {
        let s = fixture_scenario("apply-parent-no-second-window");
        let disk = StdFs;
        let mut rig = Rig::new(&s, &disk);
        rig.request.wait_pid = 77;
        // Hung: it never exits.
        rig.processes.parent_alive_for.set(-1);
        let result = rig.run();
        assert_eq!(result.outcome, ApplyOutcome::Blocked);
        assert!(
            rig.launcher.started().is_empty(),
            "a second Katana would open beside the hung one"
        );
        assert!(rig.launcher.ran().is_empty());
        assert_eq!(
            last_of(&s).status,
            "blocked",
            "and the outcome is still recorded"
        );
        expect_untouched(&s, &result);
    }

    #[test]
    fn a_window_that_exits_after_a_while_is_waited_for() {
        let s = fixture_scenario("apply-parent-exits");
        let disk = StdFs;
        let mut rig = Rig::new(&s, &disk);
        rig.request.wait_pid = 77;
        rig.processes.parent_alive_for.set(10);
        rig.timings.wait_for_parent = Duration::from_millis(5000);
        assert_eq!(rig.run().outcome, ApplyOutcome::Committed);
    }

    #[test]
    fn a_scan_that_fails_is_a_refusal_not_a_guess() {
        let s = fixture_scenario("apply-scan-fails");
        let disk = StdFs;
        let mut rig = Rig::new(&s, &disk);
        rig.processes.scan_fails = true;
        expect_refused(&s, &rig.run(), "scan_failed");
    }

    #[test]
    fn a_cancel_while_waiting_ends_the_wait_without_touching_anything() {
        let s = fixture_scenario("apply-cancel-waiting");
        let disk = StdFs;
        let mut rig = Rig::new(&s, &disk);
        rig.processes.holders = vec![RunningProgram {
            pid: 4412,
            image: path_from_utf8("C:/k/bin/katana_mcp.exe"),
        }];
        rig.processes.holders_for = -1;
        rig.timings.wait_for_holders = Duration::from_millis(60000);
        let stop = AtomicBool::new(false);
        let start = Instant::now();
        let result = std::thread::scope(|scope| {
            let canceller = scope.spawn(|| {
                std::thread::sleep(Duration::from_millis(30));
                stop.store(true, Ordering::SeqCst);
            });
            let result = rig.run_stopped(Some(&stop));
            canceller.join().expect("the canceller's thread");
            result
        });
        assert!(start.elapsed() < Duration::from_secs(10));
        expect_refused(&s, &result, "cancelled");
    }

    // ---- step 2's second look: a program started while the plan was made ---------------------
    //
    // The first look is over before the plan, and the plan hashes and copies every changed file.
    // A program of the install started in that time ran old code over the new tree, unseen. The
    // rule now (apply.rs, step 4): the scan is asked once more, with the PLAN on disk and
    // immediately before the journal goes to COMMITTING, and a holder found then is the Blocked
    // of the first look - the install untouched and the download kept. The expectations below are
    // that rule's: the outcome and its sentence are the ones
    // programs_still_running_under_the_root... holds the first look to, and "untouched" and
    // "kept" are facts about the disk.

    #[test]
    fn a_program_started_while_the_plan_was_made_blocks_the_update_and_the_install_and_the_download_are_as_before()
     {
        let s = fixture_scenario("apply-late-holder");
        let stage_before = snapshot(&s.stage);
        let disk = StdFs;
        let rig = Rig::new(&s, &disk);
        // Nobody at the first look. At the second, katana_cli: started while the files were hashed
        // and copied.
        let processes =
            ScriptedProcesses::new(vec![nobody(), somebody(4412, "C:/k/bin/katana_cli.exe")]);

        let result = run_with(&rig, &processes);
        assert_eq!(result.outcome, ApplyOutcome::Blocked);
        assert_eq!(result.reason, "blocked");
        assert_eq!(result.holders.len(), 1);
        assert_eq!(result.holders[0].pid, 4412);
        assert_eq!(
            result.detail,
            "Close these first: katana_cli.exe (4412). Then press Retry."
        );
        assert_eq!(
            processes.scans.get(),
            2,
            "the second look is one question, not another wait"
        );

        // The install is as it was: not a file of it moved, and no folder of the transaction
        // left.
        expect_untouched(&s, &result);
        // The download is kept: every staged file is where it was, with the bytes it had.
        let changed = difference(&stage_before, &snapshot(&s.stage));
        assert!(changed.is_empty(), "the stage was changed: {changed}");
        // The journal never said COMMITTING, so no RunOnce entry was ever registered.
        assert!(rig.registry.run_once_history.borrow().is_empty());
        // As after the first look: the old version is started again, nothing is run for its
        // health, and the outcome is recorded for the window to show.
        let started = rig.launcher.started();
        assert_eq!(started.len(), 1);
        assert_eq!(started[0].executable, rig.request.relaunch);
        assert!(rig.launcher.ran().is_empty());
        let last = last_of(&s);
        assert_eq!(last.status, "blocked");
        assert_eq!(last.holder, "katana_cli.exe (4412)");

        // "Kept" in the sense that matters: with the program closed, the Retry installs from that
        // same stage, with nothing downloaded again.
        let retry = Rig::with_transaction(&s, &disk, "b2b2b2b2");
        let again = retry.run();
        assert_eq!(
            again.outcome,
            ApplyOutcome::Committed,
            "{}: {}",
            again.reason,
            again.detail
        );
        assert_eq!(
            tree_hash(&s.root),
            s.v2_hash,
            "{}",
            difference(&snapshot(&s.expected), &snapshot(&s.root))
        );
    }

    #[test]
    fn the_second_look_is_the_last_thing_before_the_journal_says_committing() {
        let s = fixture_scenario("apply-late-look-order");
        let disk = StdFs;
        let log = LogFs::new(&disk);
        let rig = Rig::new(&s, &log);
        // The changes to the disk made before the second look.
        let changes_before = Cell::new(0usize);
        let journal_at_the_look = Cell::new(JournalState::Empty);
        let tree_at_the_look = RefCell::new(String::new());
        // Nobody, whenever it is asked.
        let processes = ScriptedProcesses::new(Vec::new());
        processes.set_on_scan(|asked| {
            if asked == 2 {
                changes_before.set(log.log.borrow().len());
                journal_at_the_look.set(journal_state_of(&s.root));
                *tree_at_the_look.borrow_mut() = tree_hash(&s.root);
            }
        });

        let result = run_with(&rig, &processes);
        assert_eq!(
            result.outcome,
            ApplyOutcome::Committed,
            "{}: {}",
            result.reason,
            result.detail
        );
        assert_eq!(
            processes.scans.get(),
            2,
            "asked once before the plan and once before the commit"
        );
        // When it was asked the second time the PLAN group was on disk and nothing had moved ...
        assert_eq!(journal_at_the_look.get(), JournalState::Planned);
        assert_eq!(*tree_at_the_look.borrow(), s.v1_hash);
        // ... the PLAN group was the last change before it, and the COMMITTING record the first
        // one after it: nothing is done between the answer and the commit, in which a program
        // could be started and not seen.
        let changes_before = changes_before.get();
        let entries = log.log.borrow().len();
        assert!(changes_before > 0);
        assert!(changes_before < entries);
        assert_eq!(log.find("appendDurable", "PLAN", 1), changes_before as u64);
        assert_eq!(
            log.find("appendDurable", "COMMITTING", 1),
            changes_before as u64 + 1,
            "{}",
            log.log.borrow()[changes_before]
        );
        assert_eq!(tree_hash(&s.root), s.v2_hash);
    }

    #[test]
    fn a_scan_that_fails_at_the_second_look_is_a_refusal_too_and_nothing_is_touched() {
        let s = fixture_scenario("apply-late-scan-fails");
        let stage_before = snapshot(&s.stage);
        let disk = StdFs;
        let rig = Rig::new(&s, &disk);
        // "Nobody" the first time; the second time the scan gives no answer, which is not
        // "nobody".
        let processes = ScriptedProcesses::new(vec![nobody(), no_answer()]);
        let result = run_with(&rig, &processes);
        expect_refused(&s, &result, "scan_failed");
        assert_eq!(processes.scans.get(), 2);
        let changed = difference(&stage_before, &snapshot(&s.stage));
        assert!(changed.is_empty(), "the stage was changed: {changed}");
        assert!(rig.registry.run_once_history.borrow().is_empty());
    }
}

// ---- removals and seeds, the rules --------------------------------------------------------------

mod apply_removals {
    use super::*;

    #[test]
    fn a_first_update_with_no_installed_manifest_deletes_nothing() {
        let s = fixture_scenario("apply-first-update");
        remove(&s.root.join("share/katana/installed.manifest.json"));
        remove(&s.root.join("share/katana/installed.manifest.json.sig"));
        let disk = StdFs;
        let rig = Rig::new(&s, &disk);
        assert_eq!(rig.run().outcome, ApplyOutcome::Committed);
        // v1's own files that v2 dropped are not listed in any record, so they stay.
        assert!(is_present(&s.root.join("bin/libold-1.dll")));
        assert!(is_present(&s.root.join("share/katana/legacy/old.txt")));
        // And the records are written, the first time.
        assert!(is_present(
            &s.root.join("share/katana/installed.manifest.json")
        ));
        assert!(is_present(
            &s.root.join("share/katana/installed.manifest.json.sig")
        ));
    }

    #[test]
    fn an_installed_manifest_that_does_not_verify_decides_nothing() {
        let s = fixture_scenario("apply-tampered-record");
        let mut json = read_text(&s.root.join("share/katana/installed.manifest.json"));
        // A listed name, changed: the signature no longer fits.
        let at = find_bytes(&json, b"libold").expect("the record lists libold");
        json[at + 3] = b'X';
        write_text(&s.root.join("share/katana/installed.manifest.json"), json);
        let disk = StdFs;
        let rig = Rig::new(&s, &disk);
        assert_eq!(rig.run().outcome, ApplyOutcome::Committed);
        assert!(
            is_present(&s.root.join("bin/libold-1.dll")),
            "no record can be trusted, so nothing is removed"
        );
    }

    #[test]
    fn a_file_the_old_manifest_lists_and_the_person_changed_is_not_removed() {
        let s = fixture_scenario("apply-edited-removal");
        write_text(
            &s.root.join("share/katana/legacy/old.txt"),
            "edited by the person",
        );
        let disk = StdFs;
        let rig = Rig::new(&s, &disk);
        assert_eq!(rig.run().outcome, ApplyOutcome::Committed);
        assert_eq!(
            read_text(&s.root.join("share/katana/legacy/old.txt")),
            b"edited by the person"
        );
        assert!(
            !is_present(&s.root.join("bin/libold-1.dll")),
            "the unchanged one still goes"
        );
    }

    #[test]
    fn a_file_that_was_already_deleted_is_not_an_error() {
        let s = fixture_scenario("apply-gone-removal");
        remove(&s.root.join("bin/libold-1.dll"));
        let disk = StdFs;
        let rig = Rig::new(&s, &disk);
        assert_eq!(rig.run().outcome, ApplyOutcome::Committed);
    }

    #[test]
    fn a_removal_that_is_a_link_is_left_alone() {
        let s = fixture_scenario("apply-link-removal");
        let disk = StdFs;
        let linked = s.root.join("bin/libold-1.dll");
        let hooked = HookFs::new(&disk);
        hooked.set_adjust_stat(|p: &Path, info: &mut EntryInfo| {
            if p == linked.as_path() {
                info.reparse_point = true;
            }
        });
        let rig = Rig::new(&s, &hooked);
        assert_eq!(rig.run().outcome, ApplyOutcome::Committed);
        assert!(is_present(&s.root.join("bin/libold-1.dll")));
    }

    #[test]
    fn a_folder_the_removal_emptied_is_removed_but_never_one_of_the_four_top_folders() {
        crate::require_can_sign!();
        let mut s = Scenario::new("apply-prune");
        // v1 has bin/only.dll and share/katana/a/b/deep.txt; v2 has neither (and keeps
        // bin/other.dll).
        s.v1.version = String::from(OLD_VERSION);
        s.v1.tag = format!("v{OLD_VERSION}");
        s.v1.sequence = 1;
        s.v2.version = String::from(NEW_VERSION);
        s.v2.tag = format!("v{NEW_VERSION}");
        s.v2.sequence = 2;
        s.v1.files = vec![
            FileSpec::new("bin/only.dll", "x", FileClass::Lib, false),
            FileSpec::new("share/katana/a/b/deep.txt", "y", FileClass::Data, false),
            FileSpec::new("share/katana/kept.txt", "z", FileClass::Data, false),
        ];
        s.v2.files = vec![
            FileSpec::new("bin/other.dll", "o", FileClass::Lib, false),
            FileSpec::new("share/katana/kept.txt", "z", FileClass::Data, false),
        ];
        write_install(&s.root, &s.v1);
        write_stage(&s.stage, &s.v2);
        let disk = StdFs;
        let rig = Rig::new(&s, &disk);
        assert_eq!(rig.run().outcome, ApplyOutcome::Committed);
        assert!(
            !is_present(&s.root.join("share/katana/a")),
            "a and a/b were emptied by the removal"
        );
        assert!(
            s.root.join("share/katana").is_dir(),
            "kept.txt and the records are in it"
        );
        assert!(
            s.root.join("bin").is_dir(),
            "bin stays though only.dll was its first file"
        );
        assert!(is_present(&s.root.join("bin/other.dll")));
    }
}

mod apply_seeds {
    use super::*;

    #[test]
    fn a_seed_that_is_absent_is_written_and_one_that_is_present_is_never_replaced() {
        {
            let s = fixture_scenario("apply-seed-absent");
            remove(&s.root.join("etc/katana/settings.ini"));
            let disk = StdFs;
            let rig = Rig::new(&s, &disk);
            assert_eq!(rig.run().outcome, ApplyOutcome::Committed);
            assert_eq!(
                read_text(&s.root.join("etc/katana/settings.ini")),
                read_text(&fixture_folder().join("install/v2/etc/katana/settings.ini"))
            );
        }
        {
            let s = fixture_scenario("apply-seed-edited");
            write_text(&s.root.join("etc/katana/settings.ini"), "theme=mine\n");
            let disk = StdFs;
            let rig = Rig::new(&s, &disk);
            assert_eq!(rig.run().outcome, ApplyOutcome::Committed);
            assert_eq!(
                read_text(&s.root.join("etc/katana/settings.ini")),
                b"theme=mine\n"
            );
        }
    }
}

// ---- the commit: order, and what a failure does -------------------------------------------------

mod apply_commit {
    use super::*;

    #[test]
    fn classes_are_committed_data_then_libs_then_programs_then_the_helper_then_the_records() {
        crate::require_can_sign!();
        let mut s = Scenario::new("apply-order");
        build_forty_file_scenario(&mut s);
        let disk = StdFs;
        // R/<path> as each new file is moved in.
        let into_place: RefCell<Vec<String>> = RefCell::new(Vec::new());
        let hooked = HookFs::new(&disk);
        hooked.set_before_rename(|_from: &Path, to: &Path| {
            // Under the root but not in the transaction's folder: the new file in its place.
            if let Ok(relative) = to.strip_prefix(&s.root)
                && relative
                    .components()
                    .next()
                    .is_some_and(|first| first.as_os_str() != ".update")
            {
                into_place.borrow_mut().push(generic(relative));
            }
            None
        });
        let rig = Rig::new(&s, &hooked);
        assert_eq!(rig.run().outcome, ApplyOutcome::Committed);

        let class_of = |path: &str| -> i32 {
            if path == "bin/katana_updater.exe" {
                return 3;
            }
            if path.starts_with("share/katana/install") {
                // The records.
                return 4;
            }
            match s.v2.find(path).map(|file| file.file_class) {
                Some(FileClass::Data | FileClass::Seed) => 0,
                Some(FileClass::Lib) => 1,
                Some(FileClass::Program) => 2,
                None => -1,
            }
        };
        let moved_in = into_place.borrow();
        let mut previous = 0;
        let mut moved = 0usize;
        for path in moved_in.iter() {
            let rank = class_of(path.as_str());
            if rank < 0 {
                continue;
            }
            assert!(rank >= previous, "{path} committed out of order");
            previous = previous.max(rank);
            moved += 1;
        }
        assert!(moved >= 17);
        assert!(moved_in.len() >= 3);
        // installed.json, then the manifest, then its signature: the very last three.
        let n = moved_in.len();
        assert_eq!(moved_in[n - 3], "share/katana/installed.json");
        assert_eq!(moved_in[n - 2], "share/katana/installed.manifest.json");
        assert_eq!(moved_in[n - 1], "share/katana/installed.manifest.json.sig");
    }

    #[test]
    fn only_the_copies_of_files_the_manifest_marks_executable_are_made_executable() {
        crate::require_can_sign!();
        let mut s = Scenario::new("apply-exec-bit");
        build_forty_file_scenario(&mut s);
        let disk = StdFs;
        let log = LogFs::new(&disk);
        let rig = Rig::new(&s, &log);
        assert_eq!(rig.run().outcome, ApplyOutcome::Committed);

        // The path below new/, '/'-separated.
        let mut marked: Vec<String> = Vec::new();
        for entry in log.log.borrow().iter() {
            if !entry.starts_with("setExecutable ") {
                continue;
            }
            let entry = entry.replace('\\', "/");
            let at = entry.find("/new/").unwrap_or_else(|| panic!("{entry}"));
            marked.push(String::from(&entry[at + 5..]));
        }
        marked.sort();
        // From the specification (the harness, build_forty_file_scenario): the three programs are
        // the only files with exec set, and all three change between the releases.
        assert_eq!(
            marked,
            [
                "bin/katana.exe",
                "bin/katana_cli.exe",
                "bin/katana_updater.exe"
            ]
        );
    }

    #[test]
    fn the_stage_is_copied_from_and_never_renamed_out_of() {
        let s = fixture_scenario("apply-copy-not-rename");
        let disk = StdFs;
        let renamed_from: RefCell<Vec<PathBuf>> = RefCell::new(Vec::new());
        let hooked = HookFs::new(&disk);
        hooked.set_before_rename(|from: &Path, _to: &Path| {
            renamed_from.borrow_mut().push(from.to_path_buf());
            None
        });
        let rig = Rig::new(&s, &hooked);
        // Fail the health check so the stage is kept: a rolled-back update can be tried again.
        rig.launcher
            .set_on_run(|_request: &SpawnRequest| Ok(exited(1)));
        let result = rig.run();
        assert_eq!(result.outcome, ApplyOutcome::RolledBack);
        for from in renamed_from.borrow().iter() {
            assert!(
                from.strip_prefix(&s.stage).is_err(),
                "renamed out of the stage: {}",
                text(from)
            );
        }
        assert!(
            is_present(&s.stage.join("bin/katana.exe")),
            "the stage is whole for another try"
        );
        assert!(is_present(&s.stage.join("manifest.json")));
    }

    #[test]
    fn a_rename_that_fails_rolls_the_whole_swap_back_to_exactly_the_old_tree() {
        let s = fixture_scenario("apply-rename-fails");
        let disk = StdFs;
        let faults = FaultFs::new(&disk);
        faults.fail_rename_with(3, 32);
        let rig = Rig::new(&s, &faults);
        let result = rig.run();
        assert_eq!(
            result.outcome,
            ApplyOutcome::RolledBack,
            "{}: {}",
            result.reason,
            result.detail
        );
        assert_eq!(result.reason, "rename_failed");
        assert!(result.detail.contains("32"), "{}", result.detail);
        assert_eq!(
            tree_hash(&s.root),
            s.v1_hash,
            "{}",
            difference(&s.v1_snapshot, &snapshot(&s.root))
        );
        assert!(leftovers_in_update(&s.root).is_empty());

        // The old version is started again, no health check ran, and the outcome says so.
        assert_eq!(rig.launcher.started().len(), 1);
        assert!(rig.launcher.ran().is_empty());
        let last = last_of(&s);
        assert_eq!(last.status, "rolled_back");
        assert_eq!(last.reason, "rename_failed");
        assert!(rig.registry.run_once.borrow().is_empty());
        assert!(rig.registry.versions.borrow().is_empty());
    }

    #[test]
    fn a_rollback_that_itself_fails_is_rollback_incomplete_never_rolled_back() {
        let s = fixture_scenario("apply-rollback-fails");
        let disk = StdFs;
        // From the sixth rename on every rename fails with a sharing violation: the commit stops,
        // and so does the rollback that tries to move the files back.
        let renames = Cell::new(0);
        let hooked = HookFs::new(&disk);
        hooked.set_before_rename(|from: &Path, _to: &Path| {
            renames.set(renames.get() + 1);
            if renames.get() >= 6 {
                return Some(injected(32, from));
            }
            None
        });
        let rig = Rig::new(&s, &hooked);
        let result = rig.run();
        assert_eq!(result.outcome, ApplyOutcome::RollbackIncomplete);
        assert_eq!(result.reason, "rollback_failed");
        assert!(
            result.detail.contains("katana_updater --recover"),
            "{}",
            result.detail
        );
        assert!(
            result.detail.contains(&text(&s.root)),
            "the command to run by hand names the root"
        );

        // It is said in last.json, and nothing pretends the tree is whole.
        assert_eq!(last_of(&s).status, "rollback_incomplete");
        assert_eq!(last_of(&s).reason, "rollback_failed");
        assert_ne!(tree_hash(&s.root), s.v1_hash);
        // The old files are still in the transaction folder, and the RunOnce entry is still set so
        // that the next logon tries again; and a half-swapped program is not relaunched.
        assert!(!leftovers_in_update(&s.root).is_empty());
        assert!(rig.registry.run_once.borrow().contains_key(RUN_ONCE_NAME));
        assert!(rig.launcher.started().is_empty());
    }

    #[test]
    fn a_journal_that_cannot_be_written_mid_commit_is_rolled_back() {
        let s = fixture_scenario("apply-journal-commit");
        let disk = StdFs;
        let appends = Cell::new(0);
        let hooked = HookFs::new(&disk);
        hooked.set_before_append(|p: &Path, _bytes: &[u8]| {
            // The PLAN group is the first, COMMITTING the second, the first progress group the
            // third.
            if p.file_name() == Some(OsStr::new("journal")) {
                appends.set(appends.get() + 1);
                if appends.get() == 3 {
                    return Some(injected(112, p));
                }
            }
            None
        });
        let rig = Rig::new(&s, &hooked);
        let result = rig.run();
        assert_eq!(
            result.outcome,
            ApplyOutcome::RolledBack,
            "{}{}",
            result.reason,
            result.detail
        );
        assert_eq!(result.reason, "journal");
        assert_eq!(tree_hash(&s.root), s.v1_hash);
    }

    #[test]
    fn a_file_with_two_dots_inside_its_name_is_put_back_like_any_other() {
        crate::require_can_sign!();
        // "a..b.dat" is a legal name (only "." and ".." as a whole component are not), and a
        // release may ship one. If rollback refused it, its old file would be lost with the
        // folder.
        let mut s = Scenario::new("apply-dots-in-name");
        s.v1 = small_release(OLD_VERSION, 100);
        s.v2 = small_release(NEW_VERSION, 200);
        s.v1.files.push(FileSpec::new(
            "share/katana/a..b.dat",
            "a..b of the old version\n",
            FileClass::Data,
            false,
        ));
        s.v2.files.push(FileSpec::new(
            "share/katana/a..b.dat",
            "a..b of the new version\n",
            FileClass::Data,
            false,
        ));
        write_install(&s.root, &s.v1);
        write_stage(&s.stage, &s.v2);
        s.v1_hash = tree_hash(&s.root);
        s.v1_snapshot = snapshot(&s.root);

        let disk = StdFs;
        let faults = FaultFs::new(&disk);
        // The data class goes first, in path order: a..b.dat (renames 1 and 2), then data.txt.
        // The fourth rename, data.txt's new file into place, fails - a..b.dat is the new one by
        // then.
        faults.fail_rename_with(4, 32);
        let rig = Rig::new(&s, &faults);
        let result = rig.run();
        assert_eq!(
            result.outcome,
            ApplyOutcome::RolledBack,
            "{}: {}",
            result.reason,
            result.detail
        );
        assert_eq!(
            tree_hash(&s.root),
            s.v1_hash,
            "{}",
            difference(&s.v1_snapshot, &snapshot(&s.root))
        );
        assert_eq!(
            read_text(&s.root.join("share/katana/a..b.dat")),
            b"a..b of the old version\n"
        );
        assert!(
            !is_present(&s.root.join(".update").join("recovery.log")),
            "no record was refused"
        );
    }

    #[test]
    fn a_folder_the_release_brings_for_the_first_time_is_gone_again_after_a_rollback() {
        crate::require_can_sign!();
        // etc and lib are folders of their own in the journal (a bare name that
        // check_journal_path refuses for a file); rollback must remove them, or the tree is not
        // the old version's.
        let mut s = Scenario::new("apply-new-top-folders");
        s.v1 = small_release(OLD_VERSION, 100);
        s.v2 = small_release(NEW_VERSION, 200);
        s.v2.files.push(FileSpec::new(
            "etc/katana/new.cfg",
            "a new setting\n",
            FileClass::Data,
            false,
        ));
        s.v2.files.push(FileSpec::new(
            "lib/plugins/x.dat",
            "a new plug-in\n",
            FileClass::Data,
            false,
        ));
        write_install(&s.root, &s.v1);
        write_stage(&s.stage, &s.v2);
        s.v1_hash = tree_hash(&s.root);
        s.v1_snapshot = snapshot(&s.root);
        assert!(!is_present(&s.root.join("etc")));
        assert!(!is_present(&s.root.join("lib")));

        let disk = StdFs;
        let faults = FaultFs::new(&disk);
        // Data class in path order: etc/katana/new.cfg (rename 1), lib/plugins/x.dat (2), then
        // data.txt: its old file moves aside (3) - and that one fails.
        faults.fail_rename_with(3, 32);
        let rig = Rig::new(&s, &faults);
        let result = rig.run();
        assert_eq!(
            result.outcome,
            ApplyOutcome::RolledBack,
            "{}: {}",
            result.reason,
            result.detail
        );
        assert_eq!(
            tree_hash(&s.root),
            s.v1_hash,
            "{}",
            difference(&s.v1_snapshot, &snapshot(&s.root))
        );
        assert!(!is_present(&s.root.join("etc")));
        assert!(!is_present(&s.root.join("lib")));
        assert!(
            !is_present(&s.root.join(".update").join("recovery.log")),
            "no record was refused"
        );
    }
}

// ---- health -------------------------------------------------------------------------------------

mod apply_health {
    use super::*;

    #[test]
    fn a_new_version_that_exits_non_zero_without_its_marker_is_rolled_back_and_not_offered_again() {
        let s = fixture_scenario("apply-health-exit");
        let disk = StdFs;
        let rig = Rig::new(&s, &disk);
        rig.launcher
            .set_on_run(|_request: &SpawnRequest| Ok(exited(3)));
        let result = rig.run();
        assert_eq!(result.outcome, ApplyOutcome::RolledBack);
        assert_eq!(result.reason, "health_exit");
        assert!(result.detail.contains('3'));
        assert_eq!(
            tree_hash(&s.root),
            s.v1_hash,
            "{}",
            difference(&s.v1_snapshot, &snapshot(&s.root))
        );
        assert!(leftovers_in_update(&s.root).is_empty());

        // The release goes to `blocked` so the dialog does not offer it again.
        let state = read_state(&s.state).expect("state.json is read");
        assert_eq!(state.blocked_sequences, [1792312800]);
        assert_eq!(last_of(&s).status, "rolled_back");
        assert_eq!(last_of(&s).reason, "health_exit");
        // The old version is started.
        assert_eq!(rig.launcher.started().len(), 1);
    }

    #[test]
    fn a_new_version_that_never_writes_its_marker_is_rolled_back_whether_it_exited_or_timed_out() {
        {
            let s = fixture_scenario("apply-health-no-marker");
            let disk = StdFs;
            let rig = Rig::new(&s, &disk);
            rig.launcher
                .set_on_run(|_request: &SpawnRequest| Ok(SpawnResult::default()));
            let result = rig.run();
            assert_eq!(result.outcome, ApplyOutcome::RolledBack);
            assert_eq!(
                result.reason, "health_exit",
                "exit 0 without the marker is not a start"
            );
            assert_eq!(tree_hash(&s.root), s.v1_hash);
        }
        {
            let s = fixture_scenario("apply-health-timeout");
            let disk = StdFs;
            let rig = Rig::new(&s, &disk);
            rig.launcher.set_on_run(|_request: &SpawnRequest| {
                Ok(SpawnResult {
                    timed_out: true,
                    exit_code: 1,
                    ..SpawnResult::default()
                })
            });
            let result = rig.run();
            assert_eq!(result.outcome, ApplyOutcome::RolledBack);
            assert_eq!(result.reason, "health_timeout");
            assert_eq!(tree_hash(&s.root), s.v1_hash);
        }
    }

    #[test]
    fn a_new_version_that_cannot_be_started_at_all_is_rolled_back() {
        // A missing entry point fails before main(): the program does not start.
        let s = fixture_scenario("apply-health-no-start");
        let disk = StdFs;
        let rig = Rig::new(&s, &disk);
        rig.launcher.set_on_run(|request: &SpawnRequest| {
            Err(make_error(
                ErrorCode::NotFound,
                "the program could not be started",
                text(&request.executable),
            ))
        });
        let result = rig.run();
        assert_eq!(result.outcome, ApplyOutcome::RolledBack);
        assert_eq!(result.reason, "health_exit");
        assert_eq!(tree_hash(&s.root), s.v1_hash);
    }

    #[test]
    fn a_marker_followed_by_a_non_zero_exit_is_not_healthy_but_a_hung_exit_after_the_marker_is() {
        {
            let s = fixture_scenario("apply-health-marker-crash");
            let disk = StdFs;
            let rig = Rig::new(&s, &disk);
            let marker = health_marker_path(&s.state, &rig.tx);
            let written = marker.clone();
            rig.launcher.set_on_run(move |_request: &SpawnRequest| {
                write_text(&written, "ok");
                Ok(exited(139))
            });
            let result = rig.run();
            assert_eq!(result.outcome, ApplyOutcome::RolledBack);
            assert_eq!(result.reason, "health_exit");
            assert_eq!(tree_hash(&s.root), s.v1_hash);
            assert!(
                !is_present(&marker),
                "the marker of a start that then failed is not left in the state folder"
            );
        }
        {
            let s = fixture_scenario("apply-health-marker-hang");
            let disk = StdFs;
            let rig = Rig::new(&s, &disk);
            let marker = health_marker_path(&s.state, &rig.tx);
            rig.launcher.set_on_run(move |_request: &SpawnRequest| {
                write_text(&marker, "ok");
                // It started, said so, and did not exit: the launcher killed it.
                Ok(SpawnResult {
                    timed_out: true,
                    ..SpawnResult::default()
                })
            });
            let result = rig.run();
            assert_eq!(result.outcome, ApplyOutcome::Committed);
            assert_eq!(tree_hash(&s.root), s.v2_hash);
        }
    }

    // ---- the two records that follow COMMITTED_PENDING_HEALTH, when the journal will not take
    // them
    //
    // A transaction whose journal says COMMITTED_PENDING_HEALTH is not left for ever: whoever meets
    // it later settles it (recover.rs) - healthy when the new version's marker is there, put back
    // from the old files in its folder when it is not. So while the journal says that, three
    // things must be true of the disk, and a record that cannot be written must not make any of
    // them false: the tree is the whole new version, the old files are all in the folder, and the
    // marker is there exactly when the new version started well. Each case below expects those
    // three, by the rule, and then lets `katana_updater --recover`'s settlement run and expects
    // the tree the rule gives.

    #[test]
    fn an_update_whose_healthy_record_cannot_be_written_keeps_its_folder_and_its_marker_and_is_settled_by_them()
     {
        // The new version started and said so; then the journal would not take the HEALTHY
        // record (a full disk). The update stands: it proved itself. But were the clean-up to go
        // on - the folder deleted as far as it will go, the marker removed - a later `--recover`
        // would find a pending update with no marker, take it for a release that never started,
        // and put back whatever was left of the old files.
        let s = fixture_scenario("apply-healthy-unrecorded");
        let disk = StdFs;
        // The transaction's folder when the record was refused.
        let folder_then: RefCell<BTreeMap<String, String>> = RefCell::new(BTreeMap::new());
        let full = HookFs::new(&disk);
        let rig = Rig::new(&s, &full);
        let work = s.root.join(".update").join(&rig.tx);
        {
            let work = work.clone();
            let folder_then = &folder_then;
            full.set_before_append(move |p: &Path, bytes: &[u8]| {
                if p.file_name() == Some(OsStr::new("journal")) && contains(bytes, b" HEALTHY ") {
                    *folder_then.borrow_mut() = snapshot(&work);
                    // ERROR_DISK_FULL
                    return Some(injected(112, p));
                }
                None
            });
        }
        let result = rig.run();
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
            !folder_then.borrow().is_empty(),
            "the premise: the HEALTHY record was asked for, and refused"
        );
        assert_eq!(
            journal_state_of(&s.root),
            JournalState::CommittedPendingHealth
        );
        let cleared = difference(&folder_then.borrow(), &snapshot(&work));
        assert!(
            cleared.is_empty(),
            "the folder of an update that is not recorded as healthy was cleared: its old files \
             are what it is put back from\n{cleared}"
        );
        assert!(
            is_present(&health_marker_path(&s.state, &rig.tx)),
            "the marker of an update that is not recorded as healthy was removed: it is the proof \
             that it starts"
        );

        // Whoever settles it next finds it healthy, by that marker.
        let later = Rig::with_transaction(&s, &disk, "eeeeeeee");
        assert_eq!(
            settle_unproved(&s.root, &rig.request.helper, &s.state, &later.services()),
            Settled::Healthy
        );
        assert_eq!(
            tree_hash(&s.root),
            s.v2_hash,
            "{}",
            difference(&snapshot(&s.expected), &snapshot(&s.root))
        );
        assert!(leftovers_in_update(&s.root).is_empty());
    }

    #[test]
    fn a_failed_health_run_whose_putting_back_cannot_begin_leaves_the_whole_new_tree_and_no_marker_to_call_it_healthy_by()
     {
        // The new version wrote its marker and then crashed: not healthy (the case further up).
        // The rollback's first record, ROLLING_BACK, cannot be written (a full disk). Nothing may
        // move without it - a tree half put back under a journal that says "whole, leave it" is
        // finished by nobody - so the tree is still the new version, the journal still says so,
        // and the outcome is the truth: it could not be put back. And the marker of that failed
        // start is gone ALREADY: beside a journal that still says pending, it would have the next
        // `--recover` confirm as healthy the very release this helper saw fail.
        let s = fixture_scenario("apply-health-no-record");
        let disk = StdFs;
        let full = HookFs::new(&disk);
        full.set_before_append(|p: &Path, bytes: &[u8]| {
            if p.file_name() == Some(OsStr::new("journal")) && contains(bytes, b" ROLLING_BACK ") {
                // ERROR_DISK_FULL
                return Some(injected(112, p));
            }
            None
        });
        let rig = Rig::new(&s, &full);
        // The copy the swap's helper runs from, where preflight puts it: U/run-<tx>.
        write_text(&rig.request.helper, "the helper's copy");
        let marker = health_marker_path(&s.state, &rig.tx);
        {
            let marker = marker.clone();
            rig.launcher.set_on_run(move |_request: &SpawnRequest| {
                write_text(&marker, "ok");
                Ok(exited(139))
            });
        }
        let result = rig.run();
        assert_eq!(
            result.outcome,
            ApplyOutcome::RollbackIncomplete,
            "{}: {}",
            result.reason,
            result.detail
        );
        assert_eq!(result.reason, "rollback_failed");
        assert_eq!(
            tree_hash(&s.root),
            s.v2_hash,
            "a file was moved under a journal that still says the tree is whole: {}",
            difference(&snapshot(&s.expected), &snapshot(&s.root))
        );
        assert_eq!(
            journal_state_of(&s.root),
            JournalState::CommittedPendingHealth
        );
        assert!(
            !is_present(&marker),
            "the marker of a start that then failed is left beside a pending update"
        );
        assert!(
            rig.registry.run_once.borrow().contains_key(RUN_ONCE_NAME),
            "the next logon tries again"
        );
        assert!(
            rig.launcher.started().is_empty(),
            "nothing is started after an update that could not be put back"
        );
        assert_eq!(last_of(&s).status, "rollback_incomplete");

        // That logon's `katana_updater --recover`, with room on the disk: nothing is half
        // swapped, and the update that never proved itself is put back - no marker, the files are
        // that release's, nobody is running.
        let later = Rig::with_transaction(&s, &disk, "eeeeeeee");
        let recovered = recover_in_process(&s.root, &later.services());
        assert!(
            recovered.is_ok(),
            "{}",
            recovered
                .as_ref()
                .err()
                .map(Error::describe)
                .unwrap_or_default()
        );
        assert_eq!(
            settle_unproved(&s.root, &rig.request.helper, &s.state, &later.services()),
            Settled::RolledBack
        );
        assert_eq!(
            tree_hash(&s.root),
            s.v1_hash,
            "{}",
            difference(&s.v1_snapshot, &snapshot(&s.root))
        );
        assert!(leftovers_in_update(&s.root).is_empty());
    }

    #[test]
    fn the_helper_copy_that_cannot_delete_itself_is_queued_for_the_next_start() {
        let s = fixture_scenario("apply-helper-copy");
        let disk = StdFs;
        let rig = Rig::new(&s, &disk);
        assert_eq!(rig.run().outcome, ApplyOutcome::Committed);
        let list = read_text(&s.state.join("cleanup.list"));
        assert_eq!(list, queued(&rig.request.helper));
    }

    #[test]
    fn the_helper_copy_is_queued_after_every_outcome_but_a_failed_rollback() {
        {
            let s = fixture_scenario("apply-helper-copy-refused");
            remove(&s.stage.join("manifest.json"));
            let disk = StdFs;
            let rig = Rig::new(&s, &disk);
            assert_eq!(rig.run().outcome, ApplyOutcome::Refused);
            assert_eq!(
                read_text(&s.state.join("cleanup.list")),
                queued(&rig.request.helper)
            );
        }
        {
            let s = fixture_scenario("apply-helper-copy-rolled-back");
            let disk = StdFs;
            let faults = FaultFs::new(&disk);
            faults.fail_rename_with(3, 32);
            let rig = Rig::new(&s, &faults);
            assert_eq!(rig.run().outcome, ApplyOutcome::RolledBack);
            assert_eq!(
                read_text(&s.state.join("cleanup.list")),
                queued(&rig.request.helper)
            );
        }
        {
            // The RunOnce entry re-runs katana_updater --recover from this copy: it stays.
            let s = fixture_scenario("apply-helper-copy-incomplete");
            let disk = StdFs;
            let renames = Cell::new(0);
            let hooked = HookFs::new(&disk);
            hooked.set_before_rename(|from: &Path, _to: &Path| {
                renames.set(renames.get() + 1);
                if renames.get() >= 4 {
                    Some(injected(32, from))
                } else {
                    None
                }
            });
            let rig = Rig::new(&s, &hooked);
            assert_eq!(rig.run().outcome, ApplyOutcome::RollbackIncomplete);
            assert!(!is_present(&s.state.join("cleanup.list")));
        }
    }

    #[test]
    fn what_a_lock_keeps_from_being_deleted_after_a_healthy_update_is_queued_for_the_next_start() {
        let s = fixture_scenario("apply-leftovers");
        let disk = StdFs;
        // A scanner or a program holds a file of the old version: it cannot be deleted. (The C++
        // subclasses its forwarding file system to refuse that one removal.)
        let holding = ForwardingFs::new(&disk);
        holding.on_remove_file(|inner, path: &Path| {
            if path.file_name() == Some(OsStr::new("katana.exe")) && text(path).contains("old") {
                return Err(os_error(
                    ErrorCode::FileExportFailure,
                    "sharing violation",
                    path,
                    32,
                ));
            }
            inner.remove_file(path)
        });
        let rig = Rig::new(&s, &holding);
        let result = rig.run();
        assert_eq!(
            result.outcome,
            ApplyOutcome::Committed,
            "the update is done; only the clean-up is not"
        );
        assert_eq!(tree_hash(&s.root), s.v2_hash);
        // The file is still there, and written to the list the next start works through.
        let work = s.root.join(".update").join(&rig.tx);
        let stuck = work.join("old").join("bin").join("katana.exe");
        assert!(is_present(&stuck));
        let list = read_text(&s.state.join("cleanup.list"));
        assert!(
            contains(&list, b"katana.exe"),
            "{}",
            String::from_utf8_lossy(&list)
        );
        assert!(
            is_present(&work.join("journal")),
            "and the journal says what the folder was"
        );
        // The next start sweeps the folder once the file is free.
        assert!(recover_clean(&s).is_ok());
        assert!(leftovers_in_update(&s.root).is_empty());
        assert_eq!(tree_hash(&s.root), s.v2_hash);
    }

    #[test]
    fn only_a_state_file_that_is_not_ours_to_parse_may_be_replaced_by_the_defaults() {
        // A scanner holding state.json for a moment is a failure to READ, and the person's
        // "automatic" and "skipped" in it are not to be forgotten for the sake of one blocked
        // release. (A real hold cannot be told from a failed write by a test: the rename that
        // writes the defaults fails on a held file as well, so the rule is checked on its own.)
        assert!(may_replace_unreadable_state(&Error::new(
            ErrorCode::ParseFailure,
            "the record is not a JSON object"
        )));
        for code in [
            ErrorCode::FileImportFailure,
            ErrorCode::NotFound,
            ErrorCode::Unsupported,
            ErrorCode::InvalidState,
            ErrorCode::Internal,
        ] {
            assert!(
                !may_replace_unreadable_state(&Error::new(code, "x")),
                "{code:?}"
            );
        }
    }

    #[test]
    fn a_state_file_that_is_not_json_is_replaced_by_the_defaults_and_the_blocked_release() {
        // state.rs: an unreadable record is carried on from with the defaults. It is already
        // lost.
        let s = fixture_scenario("apply-health-corrupt-state");
        write_text(&s.state.join("state.json"), "this is not json");
        let disk = StdFs;
        let rig = Rig::new(&s, &disk);
        rig.launcher
            .set_on_run(|_request: &SpawnRequest| Ok(exited(3)));
        assert_eq!(rig.run().outcome, ApplyOutcome::RolledBack);
        let state = read_state(&s.state).expect("state.json is read");
        assert_eq!(state.blocked_sequences, [1792312800]);
        assert!(state.automatic);
    }
}

// ---- an exception from below --------------------------------------------------------------------

mod apply_unexpected {
    use super::*;

    // Panics - what a conversion, an allocation or a library does, and a crash injected by FaultFs
    // does NOT (SimulatedCrash is the one panic apply() lets through) - in place of the k-th
    // mutating operation: once, or (`sticky`) at that one and every one after it, as when a disk
    // has gone. The operations are the eight FaultFs counts. The C++'s ThrowingFs throws a
    // std::runtime_error, which is what apply() catches there.
    struct Throwing {
        throw_before: u64,
        sticky: bool,
        count: Cell<u64>,
        thrown: Cell<bool>,
    }

    impl Throwing {
        fn new(throw_before: u64, sticky: bool) -> Self {
            Self {
                throw_before,
                sticky,
                count: Cell::new(0),
                thrown: Cell::new(false),
            }
        }

        fn step(&self) {
            let count = self.count.get() + 1;
            self.count.set(count);
            if count == self.throw_before || (self.sticky && count > self.throw_before) {
                self.thrown.set(true);
                panic!("injected: not a core::Error and not a crash");
            }
        }
    }

    fn throwing_fs<'a>(below: &'a dyn FsOps, throwing: &'a Throwing) -> ForwardingFs<'a> {
        let fs = ForwardingFs::new(below);
        fs.on_make_directories(move |inner, p: &Path| {
            throwing.step();
            inner.make_directories(p)
        });
        fs.on_copy_file(move |inner, a: &Path, b: &Path| {
            throwing.step();
            inner.copy_file(a, b)
        });
        fs.on_rename_file(move |inner, a: &Path, b: &Path| {
            throwing.step();
            inner.rename_file(a, b)
        });
        fs.on_remove_file(move |inner, p: &Path| {
            throwing.step();
            inner.remove_file(p)
        });
        fs.on_remove_empty_directory(move |inner, p: &Path| {
            throwing.step();
            inner.remove_empty_directory(p)
        });
        fs.on_append_durable(move |inner, p: &Path, bytes: &[u8]| {
            throwing.step();
            inner.append_durable(p, bytes)
        });
        fs.on_write_durable(move |inner, p: &Path, bytes: &[u8]| {
            throwing.step();
            inner.write_durable(p, bytes)
        });
        fs.on_set_executable(move |inner, p: &Path| {
            throwing.step();
            inner.set_executable(p)
        });
        fs
    }

    fn build_throwing_scenario(s: &mut Scenario) {
        build_small_scenario(s);
        // The tree a good update leaves, from the specification.
        write_install(&s.expected, &s.v2);
        s.v2_hash = tree_hash(&s.expected);
    }

    // For every operation k of a clean small update: the update runs on a file system that panics
    // at k (and, if `sticky`, at every operation after it); apply() must return, never panic, and
    // the tree must end as the algorithm says - then recovery on a clean disk must make it v1 or
    // v2.
    fn exception_at_every_operation(sticky: bool) {
        let mut scenarios = Loop::new("apply-throws", &build_throwing_scenario);

        // The operations of a clean run, and where its PLAN group and its HEALTHY record fall.
        let (total, plan, healthy) = {
            let s: &Scenario = scenarios.scenario();
            let bottom = StdFs;
            let log = LogFs::new(&bottom);
            let rig = Rig::new(s, &log);
            assert_eq!(rig.run().outcome, ApplyOutcome::Committed);
            let total = log.log.borrow().len() as u64;
            let plan = log.find("appendDurable", "PLAN", 1);
            let healthy = log.find("appendDurable", " HEALTHY ", 1);
            (total, plan, healthy)
        };
        assert!(plan > 0);
        assert!(healthy > plan);
        assert!(total > healthy);

        for k in 1..=total {
            let trace = format!(
                "{}{k} of {total}",
                if sticky {
                    "every operation from "
                } else {
                    "exception before operation "
                }
            );
            scenarios.reset();
            let s: &Scenario = scenarios.scenario();
            let bottom = StdFs;
            let throwing = Throwing::new(k, sticky);
            let fs = throwing_fs(&bottom, &throwing);
            let rig = Rig::new(s, &fs);
            let Ok(result) = catch_unwind(AssertUnwindSafe(|| rig.run())) else {
                panic!("an exception escaped apply(): {trace}");
            };
            assert!(
                throwing.thrown.get(),
                "the operation was never reached: {trace}"
            );

            // From the algorithm: before the PLAN group is written nothing under the install has
            // moved; between that and the HEALTHY record the tree is put back from the disk; the
            // HEALTHY record comes after the new version proved itself, so only the clean-up is
            // lost and the update stands.
            let expected_after_recovery = if k <= plan {
                assert_eq!(
                    result.outcome,
                    ApplyOutcome::Refused,
                    "{}: {} ({trace})",
                    result.reason,
                    result.detail
                );
                assert_eq!(result.reason, "internal_error", "{trace}");
                assert_eq!(
                    tree_hash(&s.root),
                    s.v1_hash,
                    "{}({trace})",
                    difference(&s.v1_snapshot, &snapshot(&s.root))
                );
                if !sticky {
                    assert!(
                        leftovers_in_update(&s.root).is_empty(),
                        "the folder of copies is deleted ({trace})"
                    );
                }
                s.v1_hash.clone()
            } else if k < healthy {
                if sticky {
                    // The disk is gone for the rollback too: said so, with the command that puts
                    // it right.
                    assert_eq!(
                        result.outcome,
                        ApplyOutcome::RollbackIncomplete,
                        "{}: {} ({trace})",
                        result.reason,
                        result.detail
                    );
                    assert_eq!(result.reason, "rollback_failed", "{trace}");
                    assert!(
                        result.detail.contains("katana_updater --recover"),
                        "{trace}"
                    );
                } else {
                    assert_eq!(
                        result.outcome,
                        ApplyOutcome::RolledBack,
                        "{}: {} ({trace})",
                        result.reason,
                        result.detail
                    );
                    assert_eq!(result.reason, "internal_error", "{trace}");
                    assert_eq!(
                        tree_hash(&s.root),
                        s.v1_hash,
                        "{}({trace})",
                        difference(&s.v1_snapshot, &snapshot(&s.root))
                    );
                    assert!(leftovers_in_update(&s.root).is_empty(), "{trace}");
                }
                s.v1_hash.clone()
            } else {
                assert_eq!(
                    result.outcome,
                    ApplyOutcome::Committed,
                    "{}: {} ({trace})",
                    result.reason,
                    result.detail
                );
                assert_eq!(
                    tree_hash(&s.root),
                    s.v2_hash,
                    "{}({trace})",
                    difference(&snapshot(&s.expected), &snapshot(&s.root))
                );
                s.v2_hash.clone()
            };

            // Whatever it was, the lock is free, and recovery on a disk that works makes the tree
            // one whole version: the old one before the commit and during it, the new one after.
            assert!(
                StdFs.try_lock(&s.root.join(".update").join("lock")).is_ok(),
                "{trace}"
            );
            assert!(recover_clean(s).is_ok(), "{trace}");
            assert_eq!(tree_hash(&s.root), expected_after_recovery, "{trace}");
        }
    }

    #[test]
    fn an_exception_at_any_operation_ends_in_the_old_tree_or_the_new_one_and_never_escapes() {
        crate::require_can_sign!();
        exception_at_every_operation(false);
    }

    #[test]
    fn a_file_system_that_keeps_throwing_from_any_operation_on_ends_the_same_way_or_says_the_rollback_failed()
     {
        crate::require_can_sign!();
        exception_at_every_operation(true);
    }
}

// ---- the real Windows file system ---------------------------------------------------------------

#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "a case holds a file open through CreateFileW, as a running program does"
)]
mod apply_on_windows {
    use super::*;
    use katana_core::path_text::environment_variable;
    use katana_update::fsops::{RetryPolicy, make_system_fs_ops};
    use katana_update::process::make_system_launcher;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::CreateFileW;

    // The values of the Windows SDK's headers (winnt.h, fileapi.h).
    const GENERIC_READ: u32 = 0x8000_0000;
    const FILE_SHARE_READ: u32 = 0x1;
    const OPEN_EXISTING: u32 = 3;
    const FILE_ATTRIBUTE_NORMAL: u32 = 0x80;

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }

    fn wide_length(path: &Path) -> usize {
        path.as_os_str().encode_wide().count()
    }

    fn make_junction(link: &Path, target: &Path) -> bool {
        let launcher = make_system_launcher();
        let request = SpawnRequest {
            executable: path_from_utf8(environment_variable(Some("SystemRoot")))
                .join("System32")
                .join("cmd.exe"),
            arguments: vec![
                String::from("/c"),
                String::from("mklink"),
                String::from("/J"),
                text(link),
                text(target),
            ],
            ..SpawnRequest::default()
        };
        matches!(launcher.run(&request), Ok(ran) if ran.exit_code == 0)
    }

    fn real_fs() -> Box<dyn FsOps> {
        make_system_fs_ops(&RetryPolicy {
            interval: Duration::from_millis(5),
            total: Duration::from_millis(100),
        })
    }

    // The path with the long-path prefix, for the test's own file calls (the system's long-path
    // setting must not decide whether the test can build its tree).
    fn prefixed(path: &Path) -> PathBuf {
        PathBuf::from(format!(r"\\?\{}", text(path)))
    }

    #[test]
    fn the_fixture_update_through_the_real_file_system_leaves_the_new_tree() {
        let s = fixture_scenario("apply-real");
        let disk = real_fs();
        let rig = Rig::new(&s, disk.as_ref());
        let result = rig.run();
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
        assert!(leftovers_in_update(&s.root).is_empty());
    }

    #[test]
    fn a_file_a_program_holds_open_rolls_the_swap_back_after_the_retry_window() {
        let s = fixture_scenario("apply-real-locked");
        let disk = real_fs();
        // bin/katana.exe is changed by the update. Held open for reading with no delete share, as
        // a running image or a scanner holds it: it can be read (hashed) but not moved.
        let name = wide(&s.root.join("bin/katana.exe"));
        // SAFETY: `name` is a NUL-terminated wide path; no security attributes and no template.
        let held = unsafe {
            CreateFileW(
                name.as_ptr(),
                GENERIC_READ,
                FILE_SHARE_READ,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                std::ptr::null_mut(),
            )
        };
        assert_ne!(held, INVALID_HANDLE_VALUE);
        let rig = Rig::new(&s, disk.as_ref());
        let result = rig.run();
        // SAFETY: the handle is ours and open; it is closed once.
        unsafe { CloseHandle(held) };

        assert_eq!(
            result.outcome,
            ApplyOutcome::RolledBack,
            "{}: {}",
            result.reason,
            result.detail
        );
        assert_eq!(result.reason, "rename_failed");
        assert!(result.detail.contains("os_error=32"), "{}", result.detail);
        assert_eq!(
            tree_hash(&s.root),
            s.v1_hash,
            "{}",
            difference(&s.v1_snapshot, &snapshot(&s.root))
        );
        assert!(leftovers_in_update(&s.root).is_empty());
    }

    #[test]
    fn two_helpers_contend_for_the_lock_through_the_real_file_system() {
        let s = fixture_scenario("apply-real-contend");
        let release = Arc::new(AtomicBool::new(false));
        let scans = Arc::new(AtomicI32::new(0));
        let (first_result, second_result) = std::thread::scope(|scope| {
            // Each helper has a file system of its own, as each process would; the lock is the
            // file's, whoever asks.
            let running = scope.spawn(|| {
                let disk = make_system_fs_ops(&RetryPolicy::default());
                let mut first = Rig::with_transaction(&s, disk.as_ref(), "aaaa");
                first.processes.holders = vec![RunningProgram {
                    pid: 4412,
                    image: path_from_utf8("katana_mcp.exe"),
                }];
                first.processes.holders_for = -1;
                first.processes.release = Arc::clone(&release);
                first.processes.scans = Arc::clone(&scans);
                first.timings.wait_for_holders = Duration::from_secs(60);
                first.timings.poll_interval = Duration::from_millis(3);
                first.run()
            });
            for _ in 0..5000 {
                if scans.load(Ordering::SeqCst) != 0 {
                    break;
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            let looked = scans.load(Ordering::SeqCst) > 0;
            let disk = make_system_fs_ops(&RetryPolicy::default());
            let second = Rig::with_transaction(&s, disk.as_ref(), "bbbb");
            let second_result = second.run();
            release.store(true, Ordering::SeqCst);
            let first_result = running.join().expect("the first helper's thread");
            assert!(looked, "the first helper never scanned for holders");
            (first_result, second_result)
        });
        assert_eq!(second_result.reason, "locked");
        assert_eq!(
            first_result.outcome,
            ApplyOutcome::Committed,
            "{}{}",
            first_result.reason,
            first_result.detail
        );
    }

    // ---- what only the real file system can show ---------------------------------------------

    #[test]
    fn an_install_under_a_very_deep_folder_is_updated_through_the_long_path_prefix() {
        // The root is 216 characters, so that what the library reads by the plain name stays
        // under the 260 of the ordinary calls (the install's own files: 242 to 252) while a copy
        // made under R/.update/<tx>/new/ does not (share/katana/readme.txt there: 261, the new
        // help index: 265). The copy's hash must be taken through the same prefix the file system
        // uses, or the right bytes are refused as "staged_hash".
        //
        // std::filesystem cannot walk a tree by its prefixed name (probed with that library: the
        // iteration and remove_all go wrong), so the test builds and checks the install at a
        // short place and MOVES it to the deep one for the update and back afterwards.
        let s = fixture_scenario("apply-long-root");
        let top = s.scratch.join(&"a".repeat(60));
        if wide_length(&top) > 150 {
            eprintln!("the temporary folder is too deep to build a root of 216 characters in it");
            return;
        }
        let mut deep = top.clone();
        let mut index = 0u8;
        while wide_length(&deep) + 61 <= 190 {
            deep.push(String::from(char::from(b'b' + index % 20)).repeat(60));
            index += 1;
        }
        deep.push("z".repeat(216 - wide_length(&deep) - 1));
        assert_eq!(wide_length(&deep), 216);

        std::fs::create_dir_all(prefixed(deep.parent().unwrap_or(Path::new(""))))
            .expect("the deep folder can be made");
        std::fs::rename(&s.root, prefixed(&deep)).expect("the install can be moved there");
        // Whatever happens next, the install comes back to where the test can look at it, and the
        // empty folders it stood in are removed one by one.
        struct Restore<'r> {
            root: &'r Path,
            deep: &'r Path,
            top: &'r Path,
        }
        impl Drop for Restore<'_> {
            fn drop(&mut self) {
                let _ = std::fs::rename(prefixed(self.deep), self.root);
                let mut folder = self.deep.parent();
                while let Some(at) = folder {
                    if wide_length(at) < wide_length(self.top) {
                        break;
                    }
                    let _ = std::fs::remove_dir(prefixed(at));
                    folder = at.parent();
                }
            }
        }
        let _restore = Restore {
            root: &s.root,
            deep: &deep,
            top: &top,
        };

        let disk = real_fs();
        let mut rig = Rig::new(&s, disk.as_ref());
        // The ordinary spelling, as a command line carries it.
        rig.request.root = deep.clone();
        let result = rig.run();
        assert_eq!(
            result.outcome,
            ApplyOutcome::Committed,
            "{}: {}",
            result.reason,
            result.detail
        );
        std::fs::rename(prefixed(&deep), &s.root).expect("the install can be moved back");
        assert_eq!(
            tree_hash(&s.root),
            s.v2_hash,
            "{}",
            difference(&snapshot(&s.expected), &snapshot(&s.root))
        );
        assert!(leftovers_in_update(&s.root).is_empty());
    }

    #[test]
    fn an_install_under_a_folder_with_a_space_and_non_ascii_letters_is_updated() {
        let mut s = Scenario::new("apply-non-ascii-root");
        s.root = s
            .scratch
            .path()
            .join(path_from_utf8("K\u{e4}se \u{65e5}\u{672c}"))
            .join("install");
        build_fixture_scenario(&mut s);
        let disk = real_fs();
        let rig = Rig::new(&s, disk.as_ref());
        let result = rig.run();
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
        assert!(leftovers_in_update(&s.root).is_empty());
    }

    #[test]
    fn an_install_folder_given_in_another_case_is_the_same_folder() {
        let s = fixture_scenario("apply-root-case");
        let shouting = text(&s.root).to_ascii_uppercase();
        let disk = real_fs();
        let mut rig = Rig::new(&s, disk.as_ref());
        rig.request.root = path_from_utf8(shouting);
        assert_ne!(rig.request.root.as_os_str(), s.root.as_os_str());
        let result = rig.run();
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
    }

    #[test]
    fn a_name_that_differs_only_in_case_is_one_file_and_is_not_removed_after_it_is_installed() {
        crate::require_can_sign!();
        // v1 lists bin/Old.dll and v2 bin/old.dll: on NTFS one file. v1's manifest lists a name
        // v2's does not (to a byte-wise comparison), but removing "Old.dll" after "old.dll" was
        // installed would delete what was just put in place.
        let mut s = Scenario::new("apply-case-removal");
        s.v1 = small_release(OLD_VERSION, 100);
        s.v2 = small_release(NEW_VERSION, 200);
        s.v1.files.push(FileSpec::new(
            "bin/Old.dll",
            "the old Old.dll\n",
            FileClass::Lib,
            false,
        ));
        s.v2.files.push(FileSpec::new(
            "bin/old.dll",
            "the new old.dll\n",
            FileClass::Lib,
            false,
        ));
        write_install(&s.root, &s.v1);
        write_stage(&s.stage, &s.v2);
        let disk = real_fs();
        let rig = Rig::new(&s, disk.as_ref());
        let result = rig.run();
        assert_eq!(
            result.outcome,
            ApplyOutcome::Committed,
            "{}: {}",
            result.reason,
            result.detail
        );
        assert_eq!(read_text(&s.root.join("bin/old.dll")), b"the new old.dll\n");
        let names: BTreeSet<String> = std::fs::read_dir(s.root.join("bin"))
            .expect("bin is listed")
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            BTreeSet::from([
                String::from("katana.exe"),
                String::from("lib.dll"),
                String::from("old.dll")
            ])
        );
    }

    #[test]
    fn a_real_junction_inside_the_install_refuses_the_update_and_nothing_beyond_it_is_written() {
        crate::require_can_sign!();
        let mut s = Scenario::new("apply-junction");
        s.v1 = small_release(OLD_VERSION, 100);
        s.v2 = small_release(NEW_VERSION, 200);
        s.v2.files.push(FileSpec::new(
            "share/katana/ext/new.dat",
            "must not leave the install\n",
            FileClass::Data,
            false,
        ));
        write_install(&s.root, &s.v1);
        write_stage(&s.stage, &s.v2);
        let outside = s.scratch.join("outside");
        std::fs::create_dir_all(&outside).expect("the folder can be made");
        assert!(
            make_junction(&s.root.join("share").join("katana").join("ext"), &outside),
            "mklink /J"
        );
        let before = tree_hash(&s.root);

        let disk = real_fs();
        let rig = Rig::new(&s, disk.as_ref());
        let result = rig.run();
        assert_eq!(
            result.outcome,
            ApplyOutcome::Refused,
            "{}: {}",
            result.reason,
            result.detail
        );
        assert_eq!(result.reason, "reparse_point");
        assert!(
            std::fs::read_dir(&outside)
                .expect("the folder is listed")
                .next()
                .is_none(),
            "nothing was written through the junction"
        );
        assert_eq!(tree_hash(&s.root), before);
        assert!(leftovers_in_update(&s.root).is_empty());
    }
}

// ---- the Windows launcher -----------------------------------------------------------------------

#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "the cases detach from the console and read the process error mode through the \
              system's own calls, as the C++ does"
)]
mod launcher {
    use super::*;
    use apply_harness::Scratch;
    use katana_core::path_text::environment_variable;
    use katana_update::process::make_system_launcher;
    use windows_sys::Win32::System::Diagnostics::Debug::SetErrorMode;

    // wincon.h: the console functions, declared here because the crate's windows-sys features do
    // not include the console's.
    #[link(name = "kernel32")]
    #[allow(non_snake_case, reason = "the system's own names")]
    unsafe extern "system" {
        fn FreeConsole() -> i32;
        fn AttachConsole(process: u32) -> i32;
    }
    // (DWORD)-1, wincon.h.
    const ATTACH_PARENT_PROCESS: u32 = u32::MAX;

    fn system_program(relative: &str) -> PathBuf {
        path_from_utf8(environment_variable(Some("SystemRoot"))).join(relative)
    }

    fn cmd_exe() -> PathBuf {
        system_program("System32/cmd.exe")
    }

    fn powershell() -> PathBuf {
        system_program("System32/WindowsPowerShell/v1.0/powershell.exe")
    }

    fn cmd(command: &str) -> SpawnRequest {
        SpawnRequest {
            executable: cmd_exe(),
            arguments: vec![String::from("/c"), String::from(command)],
            timeout: Duration::from_secs(30),
            ..SpawnRequest::default()
        }
    }

    fn trim_line_ends(mut bytes: Vec<u8>) -> Vec<u8> {
        while bytes
            .last()
            .is_some_and(|&byte| byte == b'\r' || byte == b'\n')
        {
            bytes.pop();
        }
        bytes
    }

    // Detaches the test program from its console for the length of a test, so that a console
    // program it starts has no console to inherit: WITHOUT CREATE_NO_WINDOW Windows then opens a
    // new console window for it (a black one, in life), and with the flag it does not. Run from a
    // terminal or by the test harness the child would otherwise share the parent's console and
    // the flag would make no difference to what it sees.
    struct WithoutConsole {
        detached: bool,
    }

    impl WithoutConsole {
        fn new() -> Self {
            // SAFETY: FreeConsole only detaches this process from its console.
            Self {
                detached: unsafe { FreeConsole() } != 0,
            }
        }
    }

    impl Drop for WithoutConsole {
        fn drop(&mut self) {
            if self.detached {
                // SAFETY: AttachConsole only attaches this process to its parent's console.
                unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
            }
        }
    }

    // The window handle GetConsoleWindow gives a process: 0 for a console program started without
    // a window (CREATE_NO_WINDOW), the window of a console it was given otherwise. Asked of
    // PowerShell, which can call it, because nothing else installed can.
    const CONSOLE_WINDOW_SCRIPT: &str = "Add-Type -Namespace K -Name W -MemberDefinition \
        '[DllImport(\"kernel32.dll\")] public static extern System.IntPtr GetConsoleWindow();'; \
        [K.W]::GetConsoleWindow().ToInt64()";

    #[test]
    fn run_waits_for_the_program_and_reports_its_exit_code_and_its_merged_output() {
        let launcher = make_system_launcher();
        let result = launcher
            .run(&cmd("echo to-stdout & echo to-stderr 1>&2 & exit 7"))
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(result.exit_code, 7);
        assert!(!result.timed_out);
        assert!(
            contains(&result.output, b"to-stdout"),
            "{}",
            String::from_utf8_lossy(&result.output)
        );
        assert!(
            contains(&result.output, b"to-stderr"),
            "standard error is merged in"
        );

        let fine = launcher.run(&cmd("exit 0")).expect("cmd runs");
        assert_eq!(fine.exit_code, 0);
    }

    #[test]
    fn a_run_that_takes_too_long_is_killed_and_said_to_have_timed_out() {
        let launcher = make_system_launcher();
        let request = SpawnRequest {
            timeout: Duration::from_millis(300),
            ..cmd("ping -n 6 127.0.0.1 >nul")
        };
        let start = Instant::now();
        let result = launcher.run(&request);
        let took = start.elapsed();
        let result = result.unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(result.timed_out);
        assert!(took >= Duration::from_millis(280));
        assert!(
            took < Duration::from_secs(4),
            "and it did not wait for the program to end"
        );
    }

    #[test]
    fn the_output_of_a_chatty_program_is_capped_at_sixty_four_kibi_bytes_without_blocking_it() {
        let launcher = make_system_launcher();
        // About 130 KB of output: twice the cap.
        let result = launcher
            .run(&cmd(
                "for /l %i in (1,1,3000) do @echo xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
            ))
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(
            result.exit_code, 0,
            "the program was read to its end, not left blocked on a full pipe"
        );
        assert!(!result.timed_out);
        assert_eq!(result.output.len(), 64 * 1024);
    }

    #[test]
    fn a_program_that_is_not_there_is_an_error_not_an_exit_code() {
        let launcher = make_system_launcher();
        let request = SpawnRequest {
            executable: PathBuf::from("C:/definitely/not/here/nothing.exe"),
            ..SpawnRequest::default()
        };
        let error = launcher
            .run(&request)
            .expect_err("a program that is not there is not run");
        assert_eq!(error.code, ErrorCode::NotFound);
        // ERROR_PATH_NOT_FOUND
        assert!(
            error.context.starts_with("os_error=3 "),
            "{}",
            error.describe()
        );
        assert!(launcher.start_detached(&request).is_err());
    }

    // A file that is no program (a text file named .exe, as a stub in a test tree or a truncated
    // download can be) used to make Windows raise a system message box - "This app can't run on
    // your PC", or "Unsupported 16-Bit Application" - and wait for a click: in an unattended
    // helper that is a hang. The launcher must say it as an error and show nothing. If this test
    // ever hangs, the dialog is back; the test runner's timeout turns that into a failure.
    #[test]
    fn a_file_that_is_no_program_is_an_error_and_never_a_system_dialog() {
        let launcher = make_system_launcher();
        let scratch = Scratch::new("launcher-no-program");
        let not_a_program = scratch.join("not-a-program.exe");
        write_text(&not_a_program, "this is text, not a program\n");
        let request = SpawnRequest {
            executable: not_a_program,
            ..SpawnRequest::default()
        };

        // SAFETY: SetErrorMode sets the process's error mode and returns the old one, which is put
        // straight back: read without changing it.
        let before = unsafe { SetErrorMode(0) };
        // SAFETY: as above.
        unsafe { SetErrorMode(before) };
        let start = Instant::now();
        let waited = launcher.run(&request);
        let detached = launcher.start_detached(&request);
        let took = start.elapsed();
        // SAFETY: as above.
        let after = unsafe { SetErrorMode(0) };
        // SAFETY: as above.
        unsafe { SetErrorMode(after) };

        let error = waited.expect_err("a text file is not run");
        // ERROR_BAD_EXE_FORMAT (193) on older Windows, ERROR_EXE_MACHINE_TYPE_MISMATCH (216) on
        // newer ones for a file this short: both say "this file is no program for this machine".
        let context = &error.context;
        assert!(
            context.starts_with("os_error=193 ") || context.starts_with("os_error=216 "),
            "{}",
            error.describe()
        );
        assert!(detached.is_err());
        assert!(
            took < Duration::from_secs(10),
            "it failed at once instead of waiting for a click"
        );
        assert_eq!(
            after, before,
            "the process error mode is put back as it was"
        );
    }

    #[test]
    fn the_working_directory_is_the_one_asked_for() {
        let launcher = make_system_launcher();
        let scratch = Scratch::new("launcher-cwd");
        let request = SpawnRequest {
            working_directory: scratch.path().to_path_buf(),
            ..cmd("cd")
        };
        let result = launcher.run(&request).expect("cmd runs");
        let printed = String::from_utf8_lossy(&trim_line_ends(result.output)).into_owned();
        assert!(
            printed.eq_ignore_ascii_case(&text(scratch.path())),
            "{printed} vs {}",
            text(scratch.path())
        );
    }

    #[test]
    fn arguments_reach_the_program_whatever_they_hold() {
        if !powershell().exists() {
            crate::skip_unless_available_or_fail_in_ci!(
                "no powershell.exe to print the arguments it was given"
            );
        }
        let scratch = Scratch::new("launcher-args");
        write_text(
            &scratch.join("args.ps1"),
            "$args | ForEach-Object { '[' + $_ + ']' }\n",
        );
        let script = text(&scratch.join("args.ps1"));
        let request = SpawnRequest {
            executable: powershell(),
            arguments: [
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                script.as_str(),
                "plain",
                "two words",
                "say \"hi\"",
                "ends\\",
                "has space\\",
                "--root",
                "C:\\Program Files\\Katana Install",
            ]
            .iter()
            .map(|argument| String::from(*argument))
            .collect(),
            timeout: Duration::from_secs(60),
            ..SpawnRequest::default()
        };
        let launcher = make_system_launcher();
        let result = launcher
            .run(&request)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(
            result.exit_code,
            0,
            "{}",
            String::from_utf8_lossy(&result.output)
        );
        let expected = "[plain]\r\n[two words]\r\n[say \"hi\"]\r\n[ends\\]\r\n[has space\\]\r\n\
                        [--root]\r\n[C:\\Program Files\\Katana Install]\r\n";
        assert_eq!(String::from_utf8_lossy(&result.output), expected);
    }

    #[test]
    fn a_run_starts_a_console_program_with_no_console_window() {
        // CREATE_NO_WINDOW: katana_updater is a console program started from a program with no
        // console, and without the flag Windows opens a black one for as long as it runs.
        if !powershell().exists() {
            crate::skip_unless_available_or_fail_in_ci!("no powershell.exe to ask");
        }
        let request = SpawnRequest {
            executable: powershell(),
            arguments: vec![
                String::from("-NoProfile"),
                String::from("-NonInteractive"),
                String::from("-Command"),
                String::from(CONSOLE_WINDOW_SCRIPT),
            ],
            timeout: Duration::from_secs(60),
            ..SpawnRequest::default()
        };
        let launcher = make_system_launcher();
        let _no_console = WithoutConsole::new();
        let result = launcher
            .run(&request)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        if result.exit_code != 0 {
            crate::skip_unless_available_or_fail_in_ci!(format!(
                "PowerShell cannot compile the probe here: {}",
                String::from_utf8_lossy(&result.output)
            ));
        }
        assert_eq!(
            result.output, b"0\r\n",
            "a console window handle: the child was given a window"
        );
    }

    #[test]
    fn a_detached_start_returns_at_once_and_the_child_has_no_console_window_either() {
        if !powershell().exists() {
            crate::skip_unless_available_or_fail_in_ci!("no powershell.exe to ask");
        }
        let scratch = Scratch::new("launcher-detached");
        let answer = scratch.join("answer.txt");
        let request = SpawnRequest {
            executable: powershell(),
            arguments: vec![
                String::from("-NoProfile"),
                String::from("-NonInteractive"),
                String::from("-Command"),
                format!(
                    "{CONSOLE_WINDOW_SCRIPT} | Out-File -Encoding ascii '{}'",
                    text(&answer)
                ),
            ],
            ..SpawnRequest::default()
        };
        let launcher = make_system_launcher();
        let _no_console = WithoutConsole::new();
        assert!(launcher.start_detached(&request).is_ok());
        // Compiling the probe takes the child a second or more; if it has already answered, the
        // call waited for it.
        assert!(!is_present(&answer), "it must not wait for the child");
        for _ in 0..600 {
            if is_present(&answer) {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        if !is_present(&answer) {
            crate::skip_unless_available_or_fail_in_ci!(
                "the child wrote no answer in a minute (PowerShell cannot compile the probe here?)"
            );
        }
        std::thread::sleep(Duration::from_millis(200));
        let answered = trim_line_ends(read_text(&answer));
        assert_eq!(answered, b"0");
    }

    #[test]
    fn a_detached_child_outlives_the_call_that_started_it() {
        let scratch = Scratch::new("launcher-outlives");
        let marker = scratch.join("done.txt");
        let launcher = make_system_launcher();
        // cmd.exe does not read the backslash-quote of the usual quoting, so the file is named
        // relative to the working directory and nothing in the command needs a quote.
        let request = SpawnRequest {
            working_directory: scratch.path().to_path_buf(),
            ..cmd("ping -n 2 127.0.0.1 >nul & echo finished> done.txt")
        };
        assert!(launcher.start_detached(&request).is_ok());
        assert!(
            !is_present(&marker),
            "not done yet when start_detached returns"
        );
        for _ in 0..100 {
            if is_present(&marker) {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        assert!(is_present(&marker), "the child kept running by itself");
    }

    #[test]
    fn a_path_with_a_non_ascii_letter_reaches_the_child_intact() {
        if !powershell().exists() {
            crate::skip_unless_available_or_fail_in_ci!(
                "no powershell.exe to print the arguments it was given"
            );
        }
        let scratch = Scratch::new("launcher-unicode");
        write_text(
            &scratch.join("args.ps1"),
            "[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding $false\n\
             $args | ForEach-Object { [Console]::Out.Write('[' + $_ + ']') }\n",
        );
        let request = SpawnRequest {
            executable: powershell(),
            arguments: vec![
                String::from("-NoProfile"),
                String::from("-NonInteractive"),
                String::from("-ExecutionPolicy"),
                String::from("Bypass"),
                String::from("-File"),
                text(&scratch.join("args.ps1")),
                String::from("C:\\Users\\Jos\u{e9} M\u{fc}ller"),
            ],
            timeout: Duration::from_secs(60),
            ..SpawnRequest::default()
        };
        let launcher = make_system_launcher();
        let result = launcher
            .run(&request)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(
            result.exit_code,
            0,
            "{}",
            String::from_utf8_lossy(&result.output)
        );
        assert_eq!(
            result.output,
            "[C:\\Users\\Jos\u{e9} M\u{fc}ller]".as_bytes()
        );
    }
}
