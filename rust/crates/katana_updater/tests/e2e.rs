//! The end-to-end tests of the update system, run on the Rust programs: the twins of what
//! tests/update/e2e.cmake registers for the C++ ones.
//!
//! ```text
//!   update_e2e_program_...   katana_update_e2e by itself: its usage, and that it cannot leave the
//!                            machine
//!   update_e2e_<sentence>    one case of tests/update/update_e2e.py each: the real producer makes and
//!                            signs two releases, the fake feed serves them on 127.0.0.1, and
//!                            katana_update_e2e and katana_updater_test are the engine and the helper
//!   update_e2e_every_case_of_the_driver_is_registered
//!                            the driver holds this file's list of cases to its own
//! ```
//!
//! The driver is run as e2e.cmake runs it - `python -B update_e2e.py --case <case> --engine
//! <katana_update_e2e>`, and where the swap is written (Windows) `--helper <katana_updater_test>
//! --stub <katana_updater_stub>` too - with the same environment and the same time limit, and is
//! judged as CTest judges it: by its exit code, or for the program's own cases by the
//! PASS_REGULAR_EXPRESSION of e2e.cmake on the whole of what it printed. The driver is not changed
//! for the Rust programs: they take its command lines and print its keys, as the C++ ones do.
//!
//! They reach the loopback address and nothing else (CLAUDE.md, section 4: a test may start its
//! own server on 127.0.0.1, port 0, with proxies disabled), so they run without
//! KATANA_ONLINE_TESTS. The proxy variables are taken away and NO_PROXY names the loopback:
//! Python's urllib reads them, and a request to 127.0.0.1 must never be handed to a proxy. Every
//! folder a case writes is under the system's temporary folder and is removed when the case ends.
//!
//! Python 3.10 or later and openssl 3 or later are conveniences, never requirements. e2e.cmake
//! registers no case without them; here a case is a test either way, and without them it says the
//! same sentence the configure says and returns, in that case. Where the swap is not written the
//! cases that start a program of the install are not tests at all (`#[cfg(windows)]`), as
//! e2e.cmake registers them on Windows alone: the five `portable` ones run everywhere.
//!
//! Not twinned here, because they test no program of this crate and CTest goes on running them as
//! they are: `update_tools_fake_feed` (the fake feed asked with urllib), the driver's
//! `--self-test` (its own readers), and
//! `update_every_end_to_end_test_has_the_runtime_first_on_its_path_and_a_timeout`, a check of
//! e2e.cmake's own registrations, which every test of this file has by construction: one runner,
//! with a time limit, and nothing to put on PATH (a Rust program loads no toolchain runtime).

mod support;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;
use support::{Ran, checkout, find_on_path, run, work_folder};

const ENGINE: &str = env!("CARGO_BIN_EXE_katana_update_e2e");
#[cfg(windows)]
const HELPER: &str = env!("CARGO_BIN_EXE_katana_updater_test");
#[cfg(windows)]
const STUB: &str = env!("CARGO_BIN_EXE_katana_updater_stub");

// e2e.cmake's TIMEOUTs: 300 s for the program's own cases and the registration check, 1,800 s for
// a case of the driver ("a case starts a dozen programs, and the whole suite runs beside it").
const PROGRAM_LIMIT: Duration = Duration::from_secs(300);
const CASE_LIMIT: Duration = Duration::from_secs(1800);

fn driver() -> PathBuf {
    checkout().join("tests/update/update_e2e.py")
}

// ---- the program's own rule, with no feed and no Python -----------------------------------------
//
// katana_update_e2e is what every case below fetches with, so that it cannot leave the machine is
// a promise worth a test of its own (src/bin/katana_update_e2e.rs, "THE WIRE REACHES 127.0.0.1
// AND NOTHING ELSE"): a feed that is not `http://127.0.0.1` is refused on the command line, a name
// is never looked up (localhost included), and GitHub's own addresses are not connected to unless
// --connect says which loopback port stands in for the host. None of these opens a socket. The
// install they name is the T3 fixtures' (read, never written); the state folder is never made.

fn engine(arguments: &[&str]) -> Ran {
    let mut command = Command::new(ENGINE);
    command.args(arguments);
    run(command, PROGRAM_LIMIT)
}

// `check --root <the fixtures' v1 install> --state <a folder that is never made> ...more`.
fn check_the_fixture_install(more: &[&str]) -> Ran {
    let root = checkout().join("tests/update/fixtures/install/v1");
    let state = work_folder("e2e-state-that-is-never-made");
    let root = root.to_string_lossy().into_owned();
    let state = state.to_string_lossy().into_owned();
    let mut arguments = vec!["check", "--root", root.as_str(), "--state", state.as_str()];
    arguments.extend_from_slice(more);
    engine(&arguments)
}

// A PASS_REGULAR_EXPRESSION that is a literal (each of e2e.cmake's for the program is one, with
// `[|]` standing for `|`): it passes on what was printed, whatever the exit code, as CTest does.
fn expect_printed(ran: &Ran, literal: &str) {
    assert!(
        ran.output().contains(literal),
        "expected '{literal}' in what the program printed:\n{}",
        ran.describe()
    );
}

#[test]
fn update_e2e_program_prints_its_usage_when_given_no_command() {
    expect_printed(&engine(&[]), "usage: katana_update_e2e check|plan|stage");
}

#[test]
fn update_e2e_program_refuses_an_https_feed() {
    expect_printed(
        &check_the_fixture_install(&[
            "--feed",
            "https://github.com/kjarada/Katana/releases/latest/download",
        ]),
        "--feed is an http address on the loopback",
    );
}

#[test]
fn update_e2e_program_refuses_a_feed_named_localhost_because_no_name_is_looked_up() {
    expect_printed(
        &check_the_fixture_install(&["--feed", "http://localhost:8080"]),
        "--feed is an http address on the loopback",
    );
}

#[test]
fn update_e2e_program_refuses_a_plain_http_feed_on_another_host() {
    expect_printed(
        &check_the_fixture_install(&["--feed", "http://example.com/feed"]),
        "--feed is an http address on the loopback",
    );
}

#[test]
fn update_e2e_program_connects_to_no_github_address_it_was_given_no_route_for() {
    expect_printed(
        &check_the_fixture_install(&["--github", "kjarada/Katana"]),
        "detail=the test client reaches the loopback address only",
    );
}

// ---- the driver ---------------------------------------------------------------------------------

// What the driver needs: a Python 3.10 or later (the release tools' tests need the same:
// sys.stdlib_module_names) and openssl 3 or later (pkeyutl -rawin) for the producer's signature.
struct Tools {
    python: PathBuf,
    openssl: PathBuf,
}

// "Python 3.12.3" (standard output since 3.4, standard error before): the major and minor numbers.
fn python_version(python: &Path) -> Option<(u32, u32)> {
    let mut command = Command::new(python);
    command.arg("--version");
    let ran = run(command, Duration::from_secs(60));
    let output = ran.output();
    let rest = output.trim().strip_prefix("Python ")?;
    let mut numbers = rest.split('.');
    let major = numbers.next()?.parse().ok()?;
    let minor = numbers
        .next()?
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .ok()?;
    Some((major, minor))
}

// e2e.cmake's reading of `openssl version`: exit 0 and "^OpenSSL ([0-9]+)[.]", the number 3 or
// more.
fn openssl_major(openssl: &Path) -> u32 {
    let mut command = Command::new(openssl);
    command.arg("version");
    let ran = run(command, Duration::from_secs(60));
    if ran.code != Some(0) {
        return 0;
    }
    let Some(rest) = ran.stdout.trim().strip_prefix("OpenSSL ") else {
        return 0;
    };
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() || !rest[digits.len()..].starts_with('.') {
        return 0;
    }
    digits.parse().unwrap_or(0)
}

// The tools, or the sentence e2e.cmake says at the configure where one is missing.
fn find_tools() -> Result<Tools, &'static str> {
    let python = ["python3", "python"]
        .into_iter()
        .filter_map(find_on_path)
        .find(|python| python_version(python).is_some_and(|version| version >= (3, 10)))
        .ok_or(
            "Update end-to-end tests: no Python 3.10 or later; the fake feed's test and the \
             end-to-end tests are not registered",
        )?;
    let openssl = find_on_path("openssl")
        .filter(|openssl| openssl_major(openssl) >= 3)
        .ok_or(
            "Update end-to-end tests: no openssl 3 or later on PATH to sign the test releases \
             with; the end-to-end tests are not registered",
        )?;
    Ok(Tools { python, openssl })
}

// The tools, looked for once; None, with the reason said in the case that asked, when one is
// missing.
fn tools() -> Option<&'static Tools> {
    static FOUND: OnceLock<Result<Tools, &'static str>> = OnceLock::new();
    match FOUND.get_or_init(find_tools) {
        Ok(tools) => Some(tools),
        Err(why) => {
            eprintln!("{why}");
            None
        }
    }
}

// `python -B update_e2e.py ...`. A case's environment is e2e.cmake's: the proxies away, NO_PROXY
// the loopback, and the openssl the producer signs with.
fn run_driver(tools: &Tools, arguments: &[OsString], for_a_case: bool) -> Ran {
    let mut command = Command::new(&tools.python);
    command.arg("-B").arg(driver()).args(arguments);
    if for_a_case {
        for proxy in [
            "HTTP_PROXY",
            "HTTPS_PROXY",
            "ALL_PROXY",
            "http_proxy",
            "https_proxy",
            "all_proxy",
        ] {
            command.env_remove(proxy);
        }
        command
            .env("NO_PROXY", "127.0.0.1")
            .env("no_proxy", "127.0.0.1")
            .env("KATANA_OPENSSL", &tools.openssl);
    }
    run(
        command,
        if for_a_case {
            CASE_LIMIT
        } else {
            PROGRAM_LIMIT
        },
    )
}

// e2e.cmake's _katana_e2e_programs: the engine, and where the swap is written the helper and the
// stand-in for Katana.
fn programs() -> Vec<OsString> {
    #[allow(
        unused_mut,
        reason = "Only the Windows build adds the helper and the stub."
    )]
    let mut programs: Vec<OsString> = vec!["--engine".into(), ENGINE.into()];
    #[cfg(windows)]
    {
        programs.push("--helper".into());
        programs.push(HELPER.into());
        programs.push("--stub".into());
        programs.push(STUB.into());
    }
    programs
}

// One case of the driver; `shard` (I, N) takes every N-th crash point from the I-th, so that the
// longest case is spread over the machine's cores.
fn run_case(case: &str, shard: Option<(u32, u32)>) {
    let Some(tools) = tools() else {
        return;
    };
    let mut arguments: Vec<OsString> = vec!["--case".into(), case.into()];
    if let Some((index, count)) = shard {
        arguments.push("--shard".into());
        arguments.push(format!("{index}/{count}").into());
    }
    arguments.extend(programs());
    let ran = run_driver(tools, &arguments, true);
    assert!(
        ran.succeeded(),
        "update_e2e.py --case {case} failed:\n{}",
        ran.describe()
    );
}

// The cases, as e2e.cmake's katana_add_update_e2e_test lists them: a test for each (one for each
// shard of a case that has shards), named `update_e2e_<sentence>`, and the list CASES of each case
// with whether it is portable, which the registration check below hands the driver. One
// invocation makes both, so that the tests and the list cannot differ.
macro_rules! is_portable {
    (portable) => {
        true
    };
    (windows) => {
        false
    };
}

macro_rules! shard {
    () => {
        None
    };
    ($index:literal, $count:literal) => {
        Some(($index, $count))
    };
}

macro_rules! registered_case {
    (portable $name:ident $case:literal $(, $index:literal, $count:literal)?) => {
        #[test]
        fn $name() {
            run_case($case, shard!($($index, $count)?));
        }
    };
    (windows $name:ident $case:literal $(, $index:literal, $count:literal)?) => {
        #[cfg(windows)]
        #[test]
        fn $name() {
            run_case($case, shard!($($index, $count)?));
        }
    };
}

macro_rules! registered_cases {
    ($($kind:ident $name:ident: $case:literal $(shard $index:literal of $count:literal)?;)*) => {
        $(registered_case!($kind $name $case $(, $index, $count)?);)*

        // Every case registered above, a shard of a case once each, with whether it is portable.
        const CASES: &[(&str, bool)] = &[$(($case, is_portable!($kind))),*];
    };
}

registered_cases! {
    windows update_e2e_v1_to_v2_installs_exactly_v2_and_the_feed_serves_only_what_changed:
        "v1_to_v2";
    windows update_e2e_a_member_that_is_not_the_signed_file_is_never_staged_and_the_download_resumes:
        "bad_member_hash";
    windows update_e2e_a_file_held_open_rolls_the_install_back_to_v1: "locked_file";
    windows update_e2e_a_helper_killed_before_any_change_leaves_v1_or_v2_after_recovery_1_of_4:
        "crash_at_every_step" shard 1 of 4;
    windows update_e2e_a_helper_killed_before_any_change_leaves_v1_or_v2_after_recovery_2_of_4:
        "crash_at_every_step" shard 2 of 4;
    windows update_e2e_a_helper_killed_before_any_change_leaves_v1_or_v2_after_recovery_3_of_4:
        "crash_at_every_step" shard 3 of 4;
    windows update_e2e_a_helper_killed_before_any_change_leaves_v1_or_v2_after_recovery_4_of_4:
        "crash_at_every_step" shard 4 of 4;
    windows update_e2e_a_downgrade_is_not_offered_and_the_helper_refuses_one: "downgrade";
    windows update_e2e_a_replayed_old_release_is_not_offered_and_the_helper_refuses_it:
        "replayed_release";
    portable update_e2e_a_manifest_with_one_bit_changed_is_not_believed: "tampered_byte";
    portable update_e2e_a_signature_of_another_manifest_is_not_believed: "swapped_signature";
    portable update_e2e_a_release_signed_by_another_key_is_not_believed: "other_key";
    portable update_e2e_a_release_of_another_target_is_not_offered: "wrong_target";
    windows update_e2e_a_running_program_of_the_install_blocks_the_swap_and_nothing_is_touched:
        "program_running";
    windows update_e2e_a_new_version_that_exits_with_1_is_rolled_back_and_not_offered_again:
        "new_version_exits_1";
    windows update_e2e_the_token_goes_to_the_api_and_to_no_address_a_redirect_names:
        "token_first_hop_only";
    portable update_e2e_a_feed_that_cannot_be_read_is_unreachable_with_its_reason:
        "feed_unreachable";
    windows update_e2e_a_download_that_fails_keeps_nothing_unverified_and_then_succeeds:
        "download_fails";
}

// A case the driver has and nothing here registers would never run, and nothing would say so. The
// driver compares its own list with what was registered above: on Windows all of its cases,
// elsewhere those it calls portable (`--portable-only`).
#[test]
fn update_e2e_every_case_of_the_driver_is_registered() {
    let Some(tools) = tools() else {
        return;
    };
    let swaps = cfg!(windows);
    let mut arguments: Vec<OsString> = vec!["--registered".into()];
    for &(case, portable) in CASES {
        let registered: OsString = case.into();
        if (portable || swaps) && !arguments.contains(&registered) {
            arguments.push(registered);
        }
    }
    if !swaps {
        arguments.push("--portable-only".into());
    }
    let ran = run_driver(tools, &arguments, false);
    assert!(
        ran.succeeded(),
        "update_e2e.py --registered refused the cases this file registers:\n{}",
        ran.describe()
    );
}
