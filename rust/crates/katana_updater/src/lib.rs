//! katana_updater: the fourth program, the helper that swaps the install's files while Katana is
//! not running, and puts them back if anything goes wrong (katana_update's apply.rs has the
//! algorithm; this crate is the program around it: its command line, its environment, its exit
//! codes). The port of src/katana_updater/main.cpp. It is a library so that the program is one
//! source for its two builds - the helper that ships (src/main.rs) and the helper with the test key
//! (src/bin/katana_updater_test.rs) - as the C++ compiles main.cpp twice; [`Build`] is what the
//! C++'s KATANA_UPDATER_TEST_BUILD decides.
//!
//! IT NEVER RUNS FROM bin/. The engine copies it to <U>/run-<tx>/ and runs that copy (stage.rs,
//! preflight), so at commit nothing under the install root is loaded; it is a program that
//! imports only the operating system's DLLs (the tests check the import table and run a copy of it
//! with no toolchain on PATH).
//!
//! NO ELEVATION, ANYWHERE. Nothing here asks for administrator rights and no step needs them: the
//! installer is per-user (HKCU, a folder the person owns), an archive install is where the person
//! unpacked it, and an install that cannot be written as it stands is refused
//! ("root_not_writable"), never retried with more rights. (The file name holds "update", which
//! Windows' installer detection answers with an elevation prompt - for a 32-bit program that
//! carries no manifest. Katana is built for 64-bit targets only, so none appears.)
//!
//! COMMAND LINES (the arguments arrive as UTF-8, read with katana_update's utf8_arguments and never
//! from the standard library's argv, which on Windows is the ANSI code page's version of them:
//! this is the program that is always started with paths under a user profile, whose name may hold
//! a letter that code page cannot carry):
//!
//! ```text
//!   katana_updater --root R --stage S --state U --tx T --wait-pid P --relaunch <path|none>
//!       the swap. The window has already exited when this starts. R, S, U and the relaunch path
//!       must be absolute: the program that starts the helper knows them, and a relative one
//!       would depend on a working directory nobody chose. T is 1 to 64 characters of 0-9 a-f.
//!       The relaunch path is NOT held to lie under R, though it is R/bin/katana in every real
//!       run: R is as free on this command line as the path is, so the rule would bind nobody
//!       who wrote both; the helper has no right its caller lacks (no elevation), so it starts
//!       nothing the caller could not start; and a rule that took two spellings of one folder
//!       for two folders would end with Katana closed and nothing started again.
//!   katana_updater --recover R
//!       puts back whatever a crash or a failed rollback left half swapped under R (the RunOnce
//!       entry runs this at the next logon; recover() runs it at the next start of any program;
//!       a person runs it by hand when the install no longer starts). R may be relative here
//!       (a person typed it) and must be a folder.
//!       With nothing half swapped it then SETTLES an update that is in place and was never said
//!       to start - one whose helper died while the new version was proving itself, which no
//!       program's recover() touches (recover.rs, settle_unproved): confirmed when the new
//!       version's marker U/healthy-<tx> is there; else put back, when the files are still that
//!       release's, the old ones it moved aside are all kept, U is the folder the update was
//!       applied from (it holds that update's helper copy, so a marker missing from it was never
//!       written) and no program of the install is running; else left as it is, with the RunOnce
//!       entry registered again so that the next logon comes back - which is also what becomes of
//!       an install whose update folder, or a journal in it, could not be read: it MAY hold such
//!       an update, and "could not look" is not "there is none". It starts no program.
//!   katana_updater --selftest
//!       proves this binary starts, links and can verify an Ed25519 signature: it checks a
//!       published known-answer vector (below), prints what it found with its version, and exits
//!       0 - or 21, naming each thing that is wrong. The engine runs it on the copy before the
//!       window closes.
//!   katana_updater --version
//!       one line: the version and the protocol (apply.rs, APPLIER_PROTOCOL).
//!   katana_updater --help
//!       the usage text and the exit codes below on standard output, exit 0.
//! ```
//!
//! Anything else (an unknown or repeated option, a missing value or option, modes mixed together)
//! is a usage error: the usage text on standard error and exit 20.
//!
//! EXIT CODES. They are the program's whole interface to a script and to a person at a prompt (the
//! window reads last.json, not these). --help prints the same table (HELP below: a code added here
//! is added there, and tests/helper.rs reads it from the program):
//!
//! ```text
//!    0  ok: the update is installed and healthy; or --recover put everything right, or had
//!       nothing to do, or met an update that was never said to start and confirmed it, put it
//!       back, or left it for a start that can decide (the `detail=` line says which, below);
//!       or --selftest and --version succeeded
//!   10  blocked: a process under R did not exit (or the window did not), or one was started
//!       while the files were being hashed and copied; NOTHING was touched, the old version was
//!       started again. apply.rs, steps 2 and 4
//!   11  rolled back: the update failed after the plan and the old version is back (the commit
//!       failed, or the new version did not start well enough: it exited non-zero or never
//!       wrote its health marker). The sequence is added to `blocked` in state.json
//!   12  rollback incomplete: putting the old version back failed too. The tree may be half
//!       swapped, nothing is started, and the command to run is `katana_updater --recover R`.
//!       Also what --recover itself returns when it could not finish - a half-swapped tree it
//!       could not put back, or an update that was never said to start whose putting back did
//!       not finish, or could not begin (then nothing was moved, and the next run decides again)
//!   13  refused: nothing was touched, for a reason that is not about the release's signature:
//!       another update holds the lock, the install cannot be written, a staged file is missing
//!       or wrong, ... (the `reason=` line says which; apply.rs lists them). Also the swap AND
//!       --recover on a system this program swaps nothing on: `reason=platform`, which is this
//!       crate's and not apply()'s (refused_by_the_platform, below)
//!   20  usage: the command line is wrong
//!   21  verification failed: the staged release is not trusted by THIS program's keys (a
//!       missing or unsigned manifest, a signature that does not match, another product or
//!       target, a path rule, a downgrade, a release meant only to be announced, one that needs
//!       a newer helper) - or --selftest found that this binary cannot verify a signature
//!   30  internal error: an unexpected error (a panic), or the environment could not be hardened
//!   99  (the test build only) a simulated crash, from --die-at
//! ```
//!
//! WHAT IT PRINTS (standard output, key=value, one per line - it is a console program started
//! without a window, so nobody sees it unless it is run from a prompt or by a test):
//!
//! ```text
//!   the swap:    outcome= reason= exit= detail= and a holder= line for each blocking process
//!   --recover:   recover=ok|nothing_to_do|failed, exit=, detail=. `ok` is a run that found a
//!                transaction folder and did not fail; when it met an update that was never said
//!                to start, the detail BEGINS with what became of it, a word and a colon:
//!                  confirmed:  the marker was there; the update stands and its folder is gone
//!                  put back:   no marker; the previous version is in place again
//!                  left:       it could not be judged in this run; it is as it was
//!                  busy:       another helper holds the update lock; the update and the
//!                              registry were not touched
//!                and `failed`, exit 12, when putting it back did not finish. (The line this
//!                run keeps in R/.update/recovery.log is written in every one of these cases,
//!                busy included: it records the run, and changes nothing of the update.)
//!                Where this program swaps nothing (not Windows): recover=refused, a reason=
//!                line (`platform`), exit=13, detail= - and for the swap outcome=refused with the
//!                same three lines (refused_by_the_platform, below)
//!   --selftest:  selftest=ok|failed, version=, applier=, ed25519=, openssl_environment=,
//!                openssl_removed= (the names of the variables below that were set when the
//!                program started, or none), keys=, and a problem= line for each thing wrong
//! ```
//!
//! THE ENVIRONMENT. The program hardens its environment FIRST - before an argument is read: it
//! removes OPENSSL_CONF, OPENSSL_MODULES and OPENSSL_ENGINES from this process
//! (signature.rs, harden_open_ssl_environment), so that nothing in the environment can change what
//! "verified" means. The one statement that stands before the call reads those three values and
//! touches nothing else, as signature.rs says a helper must that means to hand them on: the
//! program the helper starts afterwards - the new version for its health check, the old or new
//! one at the end - is the person's program and must see the person's environment, so each launch
//! lends the values back for its duration (HandOnLauncher). --selftest runs after the hardening
//! and says whether the variables are really gone, so it fails on a binary whose hardening is
//! missing.
//!
//! The helper also turns the system's error boxes off for ITSELF (SetErrorMode: no "there is no
//! disk in the drive", no crash box): it runs with no window, on a desktop nobody may be watching,
//! and a box would hold the swap until somebody clicked it. The programs it starts do not inherit
//! that. The launcher (katana_update's spawn_win.rs) starts each with the default error mode, so
//! the person's Katana reports a crash the way it always does, and it is the launcher too that
//! makes a file Windows cannot run an error instead of a message box. A new version that starts
//! and cannot LOAD (a missing entry point) therefore shows the loader's own box, and is rolled
//! back when somebody closes it or when the health wait ends, whichever comes first (apply.rs,
//! step 7).
//!
//! WHICH PER-USER STATE IT WRITES.
//!
//! ```text
//!   the swap:    --state U is given, so last.json, state.json (`blocked`), the health marker's
//!                removal and U/cleanup.list are written there - this is the window's own path.
//!   --recover:   nothing in U, ever, and no last.json: recover() is called first by katana_cli
//!                and katana_mcp too, and a headless program writes no per-user state
//!                (docs/headless.md). It writes only R/.update/recovery.log, one line for each
//!                run that found something to put right - and that only where R/.update is a
//!                folder of the install itself: a link of that name (a junction somebody put
//!                there) is neither listed nor written through, as recovery itself reads
//!                nothing through one. The window learns from the install itself
//!                (installed.json says which version is there), and from last.json when the
//!                swap's own helper got to write one.
//!                U is READ, and for one question only: whether the marker U/healthy-<tx> of an
//!                update that was never said to start is there (and, before a missing marker
//!                is believed, whether that update's helper copy U/run-<tx> is: where it is
//!                not, this U is not the folder the marker would be in, and nothing is put
//!                back). U is state_directory() - KATANA_UPDATE_STATE_DIR, else the per-user
//!                folder - and it is not even looked into unless the install holds such an
//!                update. So an update this mode puts back is recorded nowhere per-user: no
//!                last.json, and its release is not added to `blocked` (recover.rs says why no
//!                loop follows).
//! ```
//!
//! TRUST. The keys come from the [`Build`] and from nowhere else: katana_updater (installed) is
//! [`Build::SHIPPED`], with production_trust(), the keys pinned at build time;
//! katana_updater_test (never installed) is [`Build::test`] with the test key, and four more
//! options exist there only, so that the end-to-end tests can drive the real program:
//!
//! ```text
//!   --die-at N            a FaultFs: the N-th change to the disk is not made and the process
//!                         ends there (exit 99), as if it had been killed
//!   --registry-base K     every registry key the helper uses is re-rooted under HKCU\K
//!   --timings a,b,c,d,e   milliseconds: wait for the window, wait for the programs under R,
//!                         poll interval, wait for the health check, and how long a rename that
//!                         Windows refuses is retried - so a test need not wait a minute
//!   --stand-in P          every program this run would start (the new version for its health
//!                         check, the program started at the end) is P instead, with the same
//!                         arguments and working folder: a release whose katana is not a
//!                         program - the fixtures' is a text file - can then be applied to its
//!                         end, with a real program playing its part
//! ```
//!
//! A shipped helper rejects all four as unknown options (a test checks that). THE TEST BUILD NEVER
//! TOUCHES THE MACHINE'S OWN REGISTRY KEYS. Its registry is the one --registry-base names; without
//! the option, the one the environment variable KATANA_UPDATER_TEST_REGISTRY_BASE names (for a run
//! whose command line the engine writes: launch_apply and recover() pass no test option); and
//! without either, a registry that has nothing - no installer folder, and every write succeeds
//! doing nothing. So a test that forgets the option leaves no RunOnce entry on the machine it ran
//! on.
//!
//! So no test key exists in a shipped helper - this library holds none: only the test build's
//! own source names the test keys - and nothing in the environment moves the trust root.
//! tests/update/check_no_test_keys.cmake looks at the bytes (tests/shipped.rs runs it on these
//! programs). The self-test's known-answer signature is RFC 8032's TEST 3, a published signature
//! of a 2-byte message; it is not one of the test keys, it is never given to the trust gate, and
//! verifying it proves the code path, not any release.

use katana_core::error::make_error;
use katana_core::path_text::{path_from_utf8, path_to_utf8};
use katana_core::text_encoding::utf8_or_windows1252;
use katana_core::{Error, ErrorCode, Result, Status};
use katana_update::apply::{
    APPLIER_PROTOCOL, ApplyOutcome, ApplyRequest, ApplyResult, ApplyServices, ApplyTimings,
    is_valid_transaction_id, parse_helper_arguments,
};
use katana_update::fault_fs::{FaultFs, SimulatedCrash};
use katana_update::fsops::{EntryKind, FsOps, RetryPolicy, make_system_fs_ops};
use katana_update::install::update_work_directory;
use katana_update::process::{
    Launcher, SpawnRequest, SpawnResult, make_system_launcher, make_system_process_ops,
    utf8_arguments,
};
use katana_update::recover::{Settled, recover_in_process, settle_unproved};
use katana_update::recover_internal::{name_as_text, one_line};
use katana_update::registry::{RegistryOps, make_system_registry_ops};
use katana_update::signature::{
    SignatureBytes, check_public_key, harden_open_ssl_environment, parse_public_key_hex,
    verify_ed25519,
};
use katana_update::state::state_directory;
use katana_update::trust::{Trust, production_trust};
use katana_update::version::{program_version, program_version_text};
use std::any::Any;
use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// ---- the build ----------------------------------------------------------------------------------

/// Which of the two programs this is: the helper that ships, or the helper built for the tests
/// (the C++'s KATANA_UPDATER_TEST_BUILD). The test build trusts the key it is given, takes the
/// four test options and says what it is in --help; nothing else differs.
#[derive(Clone, Copy, Debug)]
pub struct Build {
    trust: fn() -> &'static Trust,
    test: bool,
}

impl Build {
    /// katana_updater, the program that ships: the keys pinned at build time.
    pub const SHIPPED: Self = Self {
        trust: production_trust,
        test: false,
    };

    /// katana_updater_test: `trust` (the test key, which only the test programs name) in the
    /// place of the pinned keys, and the test options.
    pub const fn test(trust: fn() -> &'static Trust) -> Self {
        Self { trust, test: true }
    }

    fn trust(self) -> &'static Trust {
        (self.trust)()
    }

    fn trust_kind(self) -> &'static str {
        if self.test { "test" } else { "pinned" }
    }
}

// ---- exit codes (the table at the top) ----------------------------------------------------------

const EXIT_OK: i32 = 0;
const EXIT_BLOCKED: i32 = 10;
const EXIT_ROLLED_BACK: i32 = 11;
const EXIT_ROLLBACK_INCOMPLETE: i32 = 12;
const EXIT_REFUSED: i32 = 13;
const EXIT_USAGE: i32 = 20;
const EXIT_VERIFICATION: i32 = 21;
const EXIT_INTERNAL: i32 = 30;
const EXIT_SIMULATED_CRASH: i32 = 99;

// The ApplyResult reasons that mean "this program does not trust what was staged" (apply.rs lists
// every reason; the rest are refusals about the machine or the files).
fn is_verification_reason(reason: &str) -> bool {
    reason == "staged_manifest"
        || reason == "notify_only"
        || reason == "applier_too_new"
        || reason == "downgrade"
}

fn exit_code_for(result: &ApplyResult) -> i32 {
    match result.outcome {
        ApplyOutcome::Committed => EXIT_OK,
        ApplyOutcome::Blocked => EXIT_BLOCKED,
        ApplyOutcome::RolledBack => EXIT_ROLLED_BACK,
        ApplyOutcome::RollbackIncomplete => EXIT_ROLLBACK_INCOMPLETE,
        ApplyOutcome::Refused => {
            if is_verification_reason(&result.reason) {
                EXIT_VERIFICATION
            } else {
                EXIT_REFUSED
            }
        }
    }
}

// ---- text ---------------------------------------------------------------------------------------

const USAGE: &str = concat!(
    "usage: katana_updater --root R --stage S --state U --tx T --wait-pid P ",
    "--relaunch <path|none>\n",
    "       katana_updater --recover R\n",
    "       katana_updater --selftest | --version | --help\n"
);

// What --help adds to the usage: what each form is for, and the exit codes (the table at the top
// of this file, in a person's words).
const HELP: &str = concat!(
    "\n",
    "The first form is how Katana starts this program to install a downloaded update: it\n",
    "replaces the files of the install folder R with those staged in S once the process P has\n",
    "exited, and starts <path> again. It is not meant to be typed.\n",
    "  --recover R   put back an update of the install folder R that was interrupted, and settle\n",
    "                one that is installed and was never confirmed to start: confirm it, put it\n",
    "                back, or leave it for later (the detail= line says which)\n",
    "  --selftest    check that this program starts and can verify a signature; print its version\n",
    "  --version     print the version and the update protocol\n",
    "  --help        print this text\n",
    "\n",
    "exit codes:\n",
    "   0  done: the update is installed; or --recover had nothing to do, put an update back,\n",
    "      confirmed one, or left one for later\n",
    "  10  blocked: a program of the install is still running; nothing was changed\n",
    "  11  rolled back: the update failed and the previous version is back\n",
    "  12  rollback incomplete: the install may be half updated; run katana_updater --recover R\n",
    "  13  refused: nothing was changed (the reason= line says why)\n",
    "  20  the command line is wrong\n",
    "  21  the staged release is not trusted by this program, or --selftest failed\n",
    "  30  internal error\n"
);

// The end of --help in the test build alone.
const TEST_HELP: &str = concat!(
    "\n",
    "This is the test build: it trusts the test key, never the release keys, and also takes\n",
    "--die-at N, --registry-base K, --timings a,b,c,d,e and --stand-in P (exit 99: --die-at).\n"
);

// Standard output and standard error, written whole and flushed. A stream that cannot be written
// (a closed pipe) is not this program's failure, as it is none for the C++'s streams: the exit
// code says what happened.
fn write_out(bytes: &[u8]) {
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(bytes);
    let _ = out.flush();
}

fn write_error(bytes: &[u8]) {
    let mut error = std::io::stderr().lock();
    let _ = error.write_all(bytes);
    let _ = error.flush();
}

// A path as a message carries it: UTF-8, a byte that is not read as Windows-1252, as apply.rs
// shows a value it refuses.
fn path_text(path: &Path) -> String {
    utf8_or_windows1252(path_to_utf8(path))
}

// The time as a log line begins with it, 2026-10-13T08:40:00Z: UTC, to the second (the C++'s
// std::format of the system clock floored to seconds). The days are made a civil date by
// H. Hinnant's civil_from_days, the algorithm std::chrono's year_month_day is specified by.
fn utc_now() -> String {
    let seconds = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(since) => i64::try_from(since.as_secs()).unwrap_or(i64::MAX),
        // Floored, as std::chrono::floor floors a time before the epoch.
        Err(before) => {
            let before = before.duration();
            -(i64::try_from(before.as_secs()).unwrap_or(i64::MAX)
                + i64::from(before.subsec_nanos() > 0))
        }
    };
    let days = seconds.div_euclid(86_400);
    let of_day = seconds.rem_euclid(86_400);
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let march_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * march_month + 2) / 5 + 1;
    let month = if march_month < 10 {
        march_month + 3
    } else {
        march_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        of_day / 3_600,
        of_day / 60 % 60,
        of_day % 60
    )
}

// The last component of a path as UTF-8, or "?" for a name that is not text (an unpaired
// surrogate, which Windows file names allow).
fn name_of(path: &Path) -> Vec<u8> {
    name_as_text(path).unwrap_or_else(|| b"?".to_vec())
}

// What a panic says, as the C++'s what() says what an exception does.
fn panic_text(payload: &(dyn Any + Send)) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        return (*text).to_owned();
    }
    if let Some(text) = payload.downcast_ref::<String>() {
        return text.clone();
    }
    String::from("an unexpected error")
}

// ---- the environment ----------------------------------------------------------------------------

const OPENSSL_VARIABLES: [&str; 3] = ["OPENSSL_CONF", "OPENSSL_MODULES", "OPENSSL_ENGINES"];

// One of the three, as the person had it set when the helper started.
struct UserVariable {
    // An element of OPENSSL_VARIABLES.
    name: &'static str,
    value: OsString,
}

// Not set, or set to nothing - which Windows reads as not set - is None.
fn read_variable(name: &str) -> Option<OsString> {
    std::env::var_os(name).filter(|value| !value.is_empty())
}

#[allow(
    unsafe_code,
    reason = "edition 2024 marks changing the environment unsafe; the program's one thread is the \
              promise"
)]
fn write_variable(name: &str, value: Option<&OsStr>) {
    // SAFETY: the helper does one thing at a time on its one thread, and no thread of it reads
    // the environment while a variable is written: the values are lent before a launch starts and
    // taken back after it has returned (HandOnLauncher).
    unsafe {
        match value {
            Some(value) => std::env::set_var(name, value),
            None => std::env::remove_var(name),
        }
    }
}

// The person's OPENSSL_* values, read BEFORE harden_open_ssl_environment removes them.
fn capture_open_ssl_variables() -> Vec<UserVariable> {
    OPENSSL_VARIABLES
        .iter()
        .filter_map(|&name| read_variable(name).map(|value| UserVariable { name, value }))
        .collect()
}

fn open_ssl_variable_is_set(name: &str) -> bool {
    read_variable(name).is_some()
}

#[allow(
    unsafe_code,
    reason = "the hardening changes the environment, which edition 2024 marks unsafe"
)]
fn harden_environment() -> Status {
    // SAFETY: called first thing in the program, before any thread is started, so no other thread
    // reads or writes the environment while it runs (harden_open_ssl_environment's contract).
    unsafe { harden_open_ssl_environment() }
}

// The values lent back to this process for the length of one launch: written when it is made,
// removed when it goes, also when the launch panics.
struct Lent<'a>(&'a [UserVariable]);

impl<'a> Lent<'a> {
    fn new(variables: &'a [UserVariable]) -> Self {
        for variable in variables {
            write_variable(variable.name, Some(variable.value.as_os_str()));
        }
        Self(variables)
    }
}

impl Drop for Lent<'_> {
    fn drop(&mut self) {
        for variable in self.0 {
            write_variable(variable.name, None);
        }
    }
}

// The Launcher the swap uses to start the person's Katana (the health check, the relaunch): the
// system one, with the person's OPENSSL_* values lent back to the process for the length of the
// call, because the child inherits the environment of the moment it is started. The helper does
// one thing at a time and verifies nothing while a child is being started or waited for (every
// signature was checked before the first file moved: apply.rs, step 1), so nothing of this
// process reads the variables while they are back.
struct HandOnLauncher {
    inner: Box<dyn Launcher>,
    variables: Vec<UserVariable>,
}

impl Launcher for HandOnLauncher {
    fn run(&self, request: &SpawnRequest) -> Result<SpawnResult> {
        let _lent = Lent::new(&self.variables);
        self.inner.run(request)
    }

    fn start_detached(&self, request: &SpawnRequest) -> Status {
        let _lent = Lent::new(&self.variables);
        self.inner.start_detached(request)
    }
}

// --stand-in (see the top): a Launcher that starts one given program whatever it is asked to
// start, with the arguments and the working folder it was asked for.
struct StandInLauncher {
    inner: Box<dyn Launcher>,
    program: PathBuf,
}

impl StandInLauncher {
    fn in_its_place(&self, request: &SpawnRequest) -> SpawnRequest {
        SpawnRequest {
            executable: self.program.clone(),
            ..request.clone()
        }
    }
}

impl Launcher for StandInLauncher {
    fn run(&self, request: &SpawnRequest) -> Result<SpawnResult> {
        self.inner.run(&self.in_its_place(request))
    }

    fn start_detached(&self, request: &SpawnRequest) -> Status {
        self.inner.start_detached(&self.in_its_place(request))
    }
}

/// A registry that has nothing: no installer folder, and a write succeeds doing nothing, so
/// nothing of the machine's is read or changed. The registry of a test build that was named none
/// (see the top), and of the engine's end-to-end client (src/bin/katana_update_e2e.rs), whose
/// install is then of kind `archive` whatever the machine has installed. (The C++ writes it out
/// in both programs.)
#[derive(Clone, Copy, Debug, Default)]
pub struct NoRegistry;

impl RegistryOps for NoRegistry {
    fn installer_location(&self) -> Result<Option<PathBuf>> {
        Ok(None)
    }

    fn set_run_once(&self, _name: &str, _command: &str) -> Status {
        Ok(())
    }

    fn clear_run_once(&self, _name: &str) -> Status {
        Ok(())
    }

    fn set_installed_version(&self, _display_version: &str, _display_name: &str) -> Status {
        Ok(())
    }
}

// The registry of a test run: the one --registry-base (or the environment) names, else one that
// has nothing - never the machine's own keys. A base that names nothing was refused on the
// command line already.
#[cfg(windows)]
fn test_registry(base: &str) -> Box<dyn RegistryOps> {
    if !base.is_empty()
        && let Some(registry) = katana_update::registry::make_registry_ops_under(base)
    {
        return registry;
    }
    Box::new(NoRegistry)
}

#[cfg(not(windows))]
fn test_registry(_base: &str) -> Box<dyn RegistryOps> {
    Box::new(NoRegistry)
}

// ---- where this program is ----------------------------------------------------------------------

// The absolute path of the running program, which becomes ApplyRequest::helper (the RunOnce
// command names it, and the clean-up list names its folder). The operating system's answer is
// preferred to argv[0]: that is what the caller typed, a bare name found through PATH, in any
// case; the file is what is running (current_exe asks GetModuleFileNameW on Windows and reads
// /proc/self/exe on Linux, as the C++ does). argv[0], made absolute, is the fallback.
fn own_executable(argv0: &[u8]) -> PathBuf {
    if let Ok(path) = std::env::current_exe() {
        return path;
    }
    let given = path_from_utf8(argv0);
    std::path::absolute(&given).unwrap_or(given)
}

// ---- the command line ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Mode {
    Version,
    SelfTest,
    #[default]
    Help,
    Recover,
    Apply,
}

// Only the test build fills these in (see the top); a shipped helper never sees them.
#[derive(Debug, Default)]
struct TestOptions {
    // 0: never.
    die_at: u64,
    // Empty: a registry that has nothing (never the machine's keys).
    registry_base: String,
    timings: Option<ApplyTimings>,
    retry: Option<RetryPolicy>,
    // Empty: every program is started as asked.
    stand_in: PathBuf,
}

#[derive(Debug, Default)]
struct Command {
    mode: Mode,
    recover_root: PathBuf,
    request: ApplyRequest,
    test: TestOptions,
}

fn usage_error(message: impl Into<String>, context: impl Into<String>) -> Error {
    make_error(ErrorCode::InvalidArgument, message, context)
}

// An argument as an error's context shows it.
fn argument_text(argument: &[u8]) -> String {
    utf8_or_windows1252(argument)
}

// A count of up to 12 decimal digits and nothing else; None for anything else.
fn parse_count(text: &[u8]) -> Option<u64> {
    if text.is_empty() || text.len() > 12 {
        return None;
    }
    let mut value: u64 = 0;
    for &c in text {
        if !c.is_ascii_digit() {
            return None;
        }
        value = value * 10 + u64::from(c - b'0');
    }
    Some(value)
}

// The environment's stand-in for --registry-base (see the top). Read in the test build only.
#[cfg(windows)]
const REGISTRY_BASE_VARIABLE: &str = "KATANA_UPDATER_TEST_REGISTRY_BASE";

// A registry base must name a folder of keys of its own under HKCU: text, not empty, with no
// leading or trailing backslash - what make_registry_ops_under accepts (registry_win.rs).
fn check_registry_base(base: &str, from: &str) -> Status {
    #[cfg(windows)]
    {
        if katana_update::registry::make_registry_ops_under(base).is_none() {
            return Err(usage_error(
                format!("{from} is an HKCU subkey with no leading or trailing backslash"),
                base,
            ));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        Err(usage_error(format!("{from} exists on Windows only"), base))
    }
}

const TIMINGS_FORM: &str = "--timings is five numbers of milliseconds, comma separated";

// The five numbers of --timings: wait for the window, wait for the programs under R, the poll
// interval, the health wait, and how long a refused rename is retried.
fn parse_timings(value: &[u8], options: &mut TestOptions) -> Status {
    let refused = || usage_error(TIMINGS_FORM, argument_text(value));
    let mut milliseconds = [0u64; 5];
    let count = milliseconds.len();
    let mut begin = 0usize;
    for (index, slot) in milliseconds.iter_mut().enumerate() {
        let comma = value[begin..]
            .iter()
            .position(|&c| c == b',')
            .map(|at| begin + at);
        let last = index + 1 == count;
        if comma.is_none() != last {
            return Err(refused());
        }
        let part = match comma {
            Some(comma) => &value[begin..comma],
            None => &value[begin..],
        };
        *slot = parse_count(part).ok_or_else(refused)?;
        if let Some(comma) = comma {
            begin = comma + 1;
        }
    }
    let ms = Duration::from_millis;
    options.timings = Some(ApplyTimings {
        wait_for_parent: ms(milliseconds[0]),
        wait_for_holders: ms(milliseconds[1]),
        poll_interval: ms(milliseconds[2]),
        health_wait: ms(milliseconds[3]),
    });
    let total = ms(milliseconds[4]);
    options.retry = Some(RetryPolicy {
        interval: RetryPolicy::default().interval.min(total.max(ms(1))),
        total,
    });
    Ok(())
}

// Takes --die-at, --registry-base, --timings and --stand-in out of `arguments`, wherever they
// stand.
fn extract_test_options(arguments: &mut Vec<Vec<u8>>, options: &mut TestOptions) -> Status {
    let mut rest: Vec<Vec<u8>> = Vec::new();
    let mut saw_die_at = false;
    let mut saw_base = false;
    let mut saw_timings = false;
    let mut saw_stand_in = false;
    let mut i = 0;
    while i < arguments.len() {
        let argument = arguments[i].as_slice();
        let is_test_option = matches!(
            argument,
            b"--die-at" | b"--registry-base" | b"--timings" | b"--stand-in"
        );
        if !is_test_option {
            rest.push(argument.to_vec());
            i += 1;
            continue;
        }
        if i + 1 >= arguments.len() {
            return Err(usage_error(
                "option without a value",
                argument_text(argument),
            ));
        }
        let value = arguments[i + 1].as_slice();
        i += 2;
        let twice = || usage_error("option given twice", argument_text(argument));
        match argument {
            b"--die-at" => {
                if saw_die_at {
                    return Err(twice());
                }
                saw_die_at = true;
                match parse_count(value) {
                    Some(count) if count != 0 => options.die_at = count,
                    _ => {
                        return Err(usage_error(
                            "--die-at is the number of a change to the disk, 1 or more",
                            argument_text(value),
                        ));
                    }
                }
            }
            b"--registry-base" => {
                if saw_base {
                    return Err(twice());
                }
                saw_base = true;
                let base = String::from_utf8_lossy(value).into_owned();
                check_registry_base(&base, "--registry-base")?;
                options.registry_base = base;
            }
            b"--stand-in" => {
                if saw_stand_in {
                    return Err(twice());
                }
                saw_stand_in = true;
                options.stand_in = path_from_utf8(value);
                if !options.stand_in.is_absolute() {
                    return Err(usage_error(
                        "--stand-in is the absolute path of a program",
                        argument_text(value),
                    ));
                }
            }
            _ => {
                if saw_timings {
                    return Err(twice());
                }
                saw_timings = true;
                parse_timings(value, options)?;
            }
        }
    }
    #[cfg(windows)]
    {
        // A run whose command line the engine wrote (launch_apply, recover()) carries no test
        // option; the test that started the engine names the registry in the environment.
        if !saw_base {
            let named = katana_core::path_text::environment_variable(Some(REGISTRY_BASE_VARIABLE));
            if !named.is_empty() {
                let named = String::from_utf8_lossy(&named).into_owned();
                check_registry_base(&named, REGISTRY_BASE_VARIABLE)?;
                options.registry_base = named;
            }
        }
    }
    *arguments = rest;
    Ok(())
}

// `arguments` is the command line without argv[0].
fn parse_command(mut arguments: Vec<Vec<u8>>, build: Build) -> Result<Command> {
    let mut command = Command::default();
    if build.test {
        extract_test_options(&mut arguments, &mut command.test)?;
    }
    let Some(first) = arguments.first() else {
        return Err(usage_error("no command: say what to do", ""));
    };
    let first = first.as_slice();
    if matches!(first, b"--version" | b"--selftest" | b"--help") {
        if arguments.len() != 1 {
            return Err(usage_error(
                "this option takes no arguments and no other option",
                argument_text(first),
            ));
        }
        command.mode = match first {
            b"--version" => Mode::Version,
            b"--selftest" => Mode::SelfTest,
            _ => Mode::Help,
        };
        return Ok(command);
    }
    if first == b"--recover" {
        if arguments.len() != 2 || arguments[1].is_empty() {
            return Err(usage_error(
                "--recover takes one folder: the install folder",
                "",
            ));
        }
        command.mode = Mode::Recover;
        let given = path_from_utf8(&arguments[1]);
        command.recover_root = match std::path::absolute(&given) {
            Ok(absolute) => absolute,
            Err(_) => {
                return Err(usage_error(
                    "the folder cannot be made absolute",
                    argument_text(&arguments[1]),
                ));
            }
        };
        if !command.recover_root.is_dir() {
            return Err(usage_error(
                "--recover needs an existing folder",
                argument_text(&arguments[1]),
            ));
        }
        return Ok(command);
    }

    let request = parse_helper_arguments(arguments.as_slice())?;
    command.mode = Mode::Apply;
    let folders = [
        ("--root", &request.root),
        ("--stage", &request.stage_directory),
        ("--state", &request.state_directory),
    ];
    for (name, path) in folders {
        if !path.is_absolute() {
            return Err(usage_error(
                format!("{name} must be an absolute path"),
                path_text(path),
            ));
        }
    }
    if !request.relaunch.as_os_str().is_empty() && !request.relaunch.is_absolute() {
        return Err(usage_error(
            "--relaunch must be an absolute path or none",
            path_text(&request.relaunch),
        ));
    }
    command.request = request;
    Ok(command)
}

// ---- --selftest ---------------------------------------------------------------------------------

// RFC 8032, section 7.1, TEST 3: the public key, the 2-byte message and the signature, as the RFC
// prints them. Checked against the RFC's own text and against an independent derivation
// (tests/update/tool_tests/rfc8032_oracle.py).
const KNOWN_PUBLIC_KEY: &str = "fc51cd8e6218a1a38da47ed00230f0580816ed13ba3303ac5deb911548908025";
const KNOWN_MESSAGE: &str = "af82";
const KNOWN_SIGNATURE: &str = concat!(
    "6291d657deec24024827e69c3abe01a30ce548a284743a445e3680d7db5ac3ac",
    "18ff9b538d16f290ae67f760984dc6594a7c15e9716ed28dc027beceea1ec40a"
);

// Lower-case hexadecimal, an even number of digits; None for anything else.
fn bytes_from_hex(hex: &str) -> Option<Vec<u8>> {
    let digits = hex.as_bytes();
    if !digits.len().is_multiple_of(2) {
        return None;
    }
    let nibble = |c: u8| match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        _ => None,
    };
    let (pairs, _) = digits.as_chunks::<2>();
    pairs
        .iter()
        .map(|&[high, low]| Some(nibble(high)? * 16 + nibble(low)?))
        .collect()
}

// `removed` is what run() read before the hardening: the variables that were set when the
// program started. Their names are printed (never their values), so that whoever runs the
// self-test under a hostile environment can see that the variables reached this process and were
// taken away, and not merely that none is set now.
fn run_self_test(removed: &[UserVariable], build: Build) -> i32 {
    let mut problems: Vec<String> = Vec::new();

    // The variables are gone from this process, not merely cleared in a copy of it.
    let mut environment_cleared = true;
    for name in OPENSSL_VARIABLES {
        if open_ssl_variable_is_set(name) {
            environment_cleared = false;
            problems.push(format!(
                "{name} is still set: the environment was not hardened"
            ));
        }
    }
    let removed_names = removed
        .iter()
        .map(|variable| variable.name)
        .collect::<Vec<_>>()
        .join(",");

    // The signature code, on a published vector: a good signature verifies, and the same
    // signature over a message that differs in one bit is refused for being wrong - not for the
    // check failing to run, which would also "refuse" it.
    let mut signature_ok = false;
    let key = parse_public_key_hex(KNOWN_PUBLIC_KEY);
    let signature_bytes = bytes_from_hex(KNOWN_SIGNATURE);
    let message_bytes = bytes_from_hex(KNOWN_MESSAGE);
    match (key, signature_bytes, message_bytes) {
        (Ok(key), Some(signature_bytes), Some(mut message)) if signature_bytes.len() == 64 => {
            let mut signature: SignatureBytes = [0; 64];
            signature.copy_from_slice(&signature_bytes);
            let good = verify_ed25519(&key, &message, &signature);
            if let Some(last) = message.last_mut() {
                *last ^= 0x01;
            }
            let altered = verify_ed25519(&key, &message, &signature);
            match (good, altered) {
                (Err(error), _) => problems.push(format!(
                    "a known good Ed25519 signature did not verify: {}",
                    error.describe()
                )),
                (Ok(()), Ok(())) => problems.push(String::from(
                    "an Ed25519 signature over an altered message was accepted",
                )),
                (Ok(()), Err(error)) if error.code != ErrorCode::InvalidArgument => {
                    problems.push(format!(
                        "an altered message failed for the wrong reason: {}",
                        error.describe()
                    ))
                }
                (Ok(()), Err(_)) => signature_ok = true,
            }
        }
        _ => problems.push(String::from("the built-in test vector is malformed")),
    }

    // The keys this build pins can be used: a mistyped pin is found here, not at the first update.
    // The placeholders are not keys (trust.rs) and are reported as what they are.
    let trust = build.trust();
    let mut keys = String::from(build.trust_kind());
    if trust.placeholder {
        keys = String::from("placeholder");
    } else {
        for pinned in &trust.keys {
            if let Err(error) = check_public_key(&pinned.key) {
                problems.push(format!(
                    "the pinned key {} is not usable: {}",
                    pinned.id,
                    error.describe()
                ));
            }
        }
        if trust.keys.is_empty() {
            problems.push(String::from("no key is pinned"));
        }
    }

    if let Err(error) = program_version() {
        problems.push(format!(
            "the program's version is not a version: {}",
            error.describe()
        ));
    }

    let mut out = format!(
        "selftest={}\nversion={}\napplier={APPLIER_PROTOCOL}\ned25519={}\nopenssl_environment={}\n\
         openssl_removed={}\nkeys={keys}\n",
        if problems.is_empty() { "ok" } else { "failed" },
        program_version_text(),
        if signature_ok { "ok" } else { "failed" },
        if environment_cleared {
            "cleared"
        } else {
            "set"
        },
        if removed_names.is_empty() {
            "none"
        } else {
            removed_names.as_str()
        },
    )
    .into_bytes();
    for text in &problems {
        out.extend_from_slice(b"problem=");
        out.extend(one_line(text));
        out.push(b'\n');
    }
    write_out(&out);
    if problems.is_empty() {
        EXIT_OK
    } else {
        EXIT_VERIFICATION
    }
}

// ---- the swap -----------------------------------------------------------------------------------

fn run_apply(mut request: ApplyRequest, argv0: &[u8], services: &ApplyServices<'_>) -> i32 {
    request.helper = own_executable(argv0);
    let result = katana_update::apply::apply(&request, services, None);
    let code = exit_code_for(&result);

    let mut out = format!(
        "outcome={}\nreason={}\nexit={code}\n",
        katana_update::apply::to_string(result.outcome),
        result.reason
    )
    .into_bytes();
    for holder in &result.holders {
        let name = name_of(&holder.image);
        let shown = if name == b"?" || name.is_empty() {
            b"Katana".to_vec()
        } else {
            name
        };
        out.extend_from_slice(b"holder=");
        out.extend(one_line(shown.as_slice()));
        out.extend_from_slice(format!(" ({})\n", holder.pid).as_bytes());
    }
    out.extend_from_slice(b"detail=");
    out.extend(one_line(result.detail.as_bytes()));
    out.push(b'\n');
    write_out(&out);
    code
}

// ---- --recover ----------------------------------------------------------------------------------

// What R/.update is. Recovery works in it only when it is an ordinary folder of the install
// (recover.rs reads nothing through a link), and this program holds itself to the same rule with
// the same question - FsOps::stat, which describes the entry itself: a junction named .update
// would otherwise have the folder it points to listed as this install's transactions, and be
// handed the recovery log, somewhere outside R.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WorkFolder {
    // An install that was never updated.
    Missing,
    // A folder of the install's own.
    Ordinary,
    // A link, a file, or something that cannot be examined: not looked into.
    Foreign,
}

fn work_folder_of(fs: &dyn FsOps, root: &Path) -> WorkFolder {
    let Ok(info) = fs.stat(&update_work_directory(root)) else {
        return WorkFolder::Foreign;
    };
    if info.kind == EntryKind::Missing {
        return WorkFolder::Missing;
    }
    if info.kind == EntryKind::Directory && !info.reparse_point {
        WorkFolder::Ordinary
    } else {
        WorkFolder::Foreign
    }
}

// The names of the transaction folders under R/.update: what an interrupted update leaves, and
// what recovery clears. A folder is looked at by name only; recover_in_process reads the
// journals. None when R/.update is not an ordinary folder (see above).
fn transaction_folders(fs: &dyn FsOps, root: &Path) -> Vec<String> {
    if work_folder_of(fs, root) != WorkFolder::Ordinary {
        return Vec::new();
    }
    let Ok(entries) = fs.list_directory(&update_work_directory(root)) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .iter()
        .map(|entry| name_of(entry.as_path()))
        .filter(|name| is_valid_transaction_id(name.as_slice()))
        // Lower-case hexadecimal, checked: the bytes are text.
        .map(|name| String::from_utf8_lossy(&name).into_owned())
        .collect();
    names.sort();
    names
}

fn joined(names: &[String]) -> String {
    names.join(",")
}

fn run_recover(root: &Path, argv0: &[u8], services: &ApplyServices<'_>) -> i32 {
    let fs = services.fs;
    let before = work_folder_of(fs, root);
    let found = transaction_folders(fs, root);
    let mut status = recover_in_process(root, services);
    // Then, and only with nothing half swapped, an update that is in place and was never said to
    // start (see the top). The per-user folder is named here and nowhere else in this mode, and
    // settle_unproved only reads it: the marker of that update, if it is there.
    let mut settled = Settled::Nothing;
    if status.is_ok() {
        settled = settle_unproved(root, &own_executable(argv0), &state_directory(), services);
        if settled == Settled::RollbackIncomplete {
            // The next run of this mode - the RunOnce entry is still there - comes back to it
            // either way. Where the putting back began, the journal says ROLLING_BACK, and that
            // run finishes it as it finishes any rollback that was cut off. Where it could not
            // begin (the record could not be written, the folder not read) nothing was moved, the
            // journal says what it said, and that run decides again.
            status = Err(Error::new(
                ErrorCode::InvalidState,
                format!(
                    "an update that was never said to start could not be put back; run: \
                     katana_updater --recover \"{}\"",
                    path_text(root)
                ),
            ));
        }
    }
    let left = transaction_folders(fs, root);

    // One line for a run that had something to put right, in the install's own folder and
    // nowhere else (see the top). A run that found nothing writes nothing: it is what the RunOnce
    // entry does at every logon after a clean update, and a log is not a clock. Asked again now,
    // because the line goes into the folder as it is at this moment: where it is not an ordinary
    // folder there is no place in R to keep a line, and what happened is on standard output
    // alone.
    //
    // THE FORM OF THE LINE IS READ BY THE WINDOW: `updateRecoveredTransactions`
    // (src/katana_qt/update_dialog.cpp) takes the ids of `found=` from it to know which helper
    // copies U/run-<tx> are finished with. It reads a line only when the time is its first word
    // (no space in it), `recover` its second, `found=<ids, commas, no space>` its third and
    // `result=...` its fourth, and when a line feed ends it. Do not change the order, the words,
    // the separators or the line feed without changing that function and
    // tests/qt_widgets/test_update_dialog.cpp with it. What follows `result=` (`left=`,
    // `detail=`) is not read there.
    if (!found.is_empty() || status.is_err()) && work_folder_of(fs, root) == WorkFolder::Ordinary {
        let mut line = format!(
            "{} recover found={} result={} left={}",
            utc_now(),
            joined(&found),
            if status.is_ok() { "ok" } else { "failed" },
            joined(&left)
        )
        .into_bytes();
        if let Err(error) = &status {
            line.extend_from_slice(b" detail=");
            line.extend(one_line(error.describe()));
        }
        line.push(b'\n');
        let _ = fs.append_durable(&update_work_directory(root).join("recovery.log"), &line);
    }

    if let Err(error) = &status {
        let mut out =
            format!("recover=failed\nexit={EXIT_ROLLBACK_INCOMPLETE}\ndetail=").into_bytes();
        out.extend(one_line(error.describe()));
        out.push(b'\n');
        write_out(&out);
        return EXIT_ROLLBACK_INCOMPLETE;
    }
    // OK is also what a live update holding the lock gets (recover.rs: not ours to touch): say
    // what stays. What became of an update that was never said to start is the first word of the
    // detail (the table at the top; tests/helper.rs reads the words).
    let mut detail = String::from("nothing was interrupted");
    if before == WorkFolder::Foreign {
        // Nothing of an update can be in it: the swap refuses to work through a link too.
        detail = String::from(
            "the install's .update is not a folder of its own (a link, or a file): nothing in it \
             was read or changed",
        );
    } else if !found.is_empty() {
        detail = if left.is_empty() {
            String::from("the install is whole")
        } else {
            format!(
                "left alone (another update holds the lock, or a file of a finished one is still \
                 held): {}",
                joined(&left)
            )
        };
    }
    match settled {
        // RollbackIncomplete: said above, as a failure.
        Settled::Nothing | Settled::RollbackIncomplete => {}
        Settled::Healthy => {
            detail = String::from(
                "confirmed: an update whose helper was cut off had said that it starts; it stands",
            );
        }
        Settled::RolledBack => {
            detail = String::from(
                "put back: an update whose helper was cut off never said that it starts; the \
                 previous version is in place again",
            );
        }
        Settled::Left => {
            // "Or may be": where the update folder, or the journal of a transaction in it, could
            // not be read, it is not known what is there, and that is left like one that is
            // known.
            detail = format!(
                "left: an update that never said it starts is in place, or may be, and could not \
                 be judged now (a program of the install is running; its files, or the old ones \
                 kept to put it back from, are not as the update left them; this run looks in \
                 another per-user folder than the update used; or something - the update folder, \
                 a journal in it - could not be examined); the next logon, or Katana's window, \
                 settles it: {}",
                joined(&left)
            );
        }
        Settled::Busy => {
            detail = format!(
                "busy: another update holds the lock; the update was not touched: {}",
                joined(&left)
            );
        }
    }
    let mut out = format!(
        "recover={}\nexit={EXIT_OK}\ndetail=",
        if found.is_empty() {
            "nothing_to_do"
        } else {
            "ok"
        }
    )
    .into_bytes();
    out.extend(one_line(detail.as_bytes()));
    out.push(b'\n');
    write_out(&out);
    EXIT_OK
}

// ---- a platform this program does not swap on ---------------------------------------------------
//
// Where this program swaps nothing, a swap and a recovery are refused before either is begun:
//
//   outcome=refused (the swap) or recover=refused (--recover), reason=platform, exit=13, detail=
//
// (for --recover that is a value, `refused`, and a line, `reason=`, which it prints nowhere
// else), and NOTHING is written anywhere: no last.json, no log, no folder. Two cases, and they do
// not hang on each other:
//
//   macOS, for good   A file changed inside Katana.app breaks the seal of the bundle
//                     (docs/update.md), so this program replaces nothing there WHATEVER the file
//                     system answers. katana_update's fsops_posix.rs is compiled for macOS too:
//                     the day it is written for Linux it works on a Mac as well, and this refusal
//                     must still stand. So it asks the file system nothing.
//   Linux, for now    The update has no file system of its own there yet (fsops_posix.rs: every
//                     operation is one refusal, with the code Unsupported). Given that file
//                     system, apply() refuses too - but as `root_not_writable`, after writing a
//                     last.json that tells the window so, and without starting Katana again; and
//                     recover_in_process answers "failed", exit 12, of an install nothing here can
//                     have half swapped. So the file system is asked one question before either is
//                     called. Where it is written stat() does not answer Unsupported, this returns
//                     nothing, and the swap and the recovery run as they do on Windows.
#[cfg(not(windows))]
fn refused_by_the_platform(command: &Command, fs: &dyn FsOps) -> Option<i32> {
    let recovering = command.mode == Mode::Recover;
    #[cfg(target_vendor = "apple")]
    let why = {
        let _ = fs; // asked nothing, for good (see above)
        String::from(
            "this program does not replace the files of Katana on macOS, where a changed file \
             would break the seal of the application",
        )
    };
    #[cfg(not(target_vendor = "apple"))]
    let why = {
        let asked = fs.stat(if recovering {
            &command.recover_root
        } else {
            &command.request.root
        });
        match asked {
            Err(error) if error.code == ErrorCode::Unsupported => error.message,
            _ => return None,
        }
    };
    let mut out = format!(
        "{}\nreason=platform\nexit={EXIT_REFUSED}\ndetail=",
        if recovering {
            "recover=refused"
        } else {
            "outcome=refused"
        }
    )
    .into_bytes();
    out.extend(one_line(why.as_bytes()));
    out.push(b'\n');
    write_out(&out);
    Some(EXIT_REFUSED)
}

// ---- the services of a run, and main ------------------------------------------------------------

// No box of the system's for THIS process to wait behind (see the top). The programs it starts
// get the default mode from the launcher.
#[allow(
    unsafe_code,
    reason = "SetErrorMode is a Win32 call, a foreign function"
)]
fn turn_error_boxes_off() {
    #[cfg(windows)]
    {
        // The values of the Windows SDK's winbase.h.
        const SEM_FAILCRITICALERRORS: u32 = 0x1;
        const SEM_NOGPFAULTERRORBOX: u32 = 0x2;
        const SEM_NOOPENFILEERRORBOX: u32 = 0x8000;
        // SAFETY: SetErrorMode only sets the process's error mode, and returns the old one, which
        // is not wanted back.
        unsafe {
            windows_sys::Win32::System::Diagnostics::Debug::SetErrorMode(
                SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX | SEM_NOOPENFILEERRORBOX,
            );
        }
    }
}

// The real file system, process scan, launcher and registry - in the test build optionally
// wrapped or re-rooted (see the top) - for one swap or one recovery.
fn run_with_services(
    command: &Command,
    argv0: &[u8],
    build: Build,
    user_variables: Vec<UserVariable>,
) -> i32 {
    let test = &command.test;
    let system = make_system_fs_ops(&test.retry.unwrap_or_default());
    let faulty = if test.die_at != 0 {
        let faulty = FaultFs::new(system.as_ref());
        faulty.crash_before(test.die_at);
        Some(faulty)
    } else {
        None
    };
    let fs: &dyn FsOps = match &faulty {
        Some(faulty) => faulty,
        None => system.as_ref(),
    };
    let system_launcher = make_system_launcher();
    let launcher: Box<dyn Launcher> = if test.stand_in.as_os_str().is_empty() {
        system_launcher
    } else {
        Box::new(StandInLauncher {
            inner: system_launcher,
            program: test.stand_in.clone(),
        })
    };
    // Never the machine's own keys in a test run: no base, or one that names nothing (the command
    // line refused that already), is a registry that has nothing.
    let registry: Box<dyn RegistryOps> = if build.test {
        test_registry(&test.registry_base)
    } else {
        make_system_registry_ops()
    };
    let processes = make_system_process_ops();
    let launcher = HandOnLauncher {
        inner: launcher,
        variables: user_variables,
    };
    let services = ApplyServices {
        fs,
        processes: processes.as_ref(),
        launcher: &launcher,
        registry: registry.as_ref(),
        trust: build.trust(),
        timings: test.timings.unwrap_or_default(),
    };

    #[cfg(not(windows))]
    {
        if let Some(refused) = refused_by_the_platform(command, fs) {
            return refused;
        }
    }
    if command.mode == Mode::Recover {
        run_recover(&command.recover_root, argv0, &services)
    } else {
        run_apply(command.request.clone(), argv0, &services)
    }
}

fn run(build: Build) -> i32 {
    // FIRST: nothing in the environment moves the trust root (see the top). The values are read on
    // the way, because the call removes them for good and the person's own Katana, started later,
    // is owed them (signature.rs says to read them before the call).
    let user_variables = capture_open_ssl_variables();
    if let Err(error) = harden_environment() {
        write_error(format!("katana_updater: {}\n", error.describe()).as_bytes());
        return EXIT_INTERNAL;
    }

    turn_error_boxes_off();

    // The command line, as UTF-8 and never from the standard library's argv.
    let mut arguments = utf8_arguments();
    let argv0 = if arguments.is_empty() {
        Vec::new()
    } else {
        arguments.remove(0)
    };
    let command = match parse_command(arguments, build) {
        Ok(command) => command,
        Err(error) => {
            write_error(format!("katana_updater: {}\n{USAGE}", error.describe()).as_bytes());
            return EXIT_USAGE;
        }
    };

    match command.mode {
        Mode::Version => {
            write_out(
                format!(
                    "katana_updater {} applier={APPLIER_PROTOCOL}\n",
                    program_version_text()
                )
                .as_bytes(),
            );
            return EXIT_OK;
        }
        Mode::Help => {
            let mut text = format!("{USAGE}{HELP}");
            if build.test {
                text.push_str(TEST_HELP);
            }
            write_out(text.as_bytes());
            return EXIT_OK;
        }
        Mode::SelfTest => return run_self_test(&user_variables, build),
        Mode::Recover | Mode::Apply => {}
    }
    run_with_services(&command, &argv0, build, user_variables)
}

/// The program: what main() of katana_updater and of katana_updater_test does, the exit code
/// included. A panic is what an exception is in the C++: it ends the run as "internal error",
/// except the simulated crash of the test build's --die-at, which ends it as a process that was
/// killed would end (exit 99).
pub fn run_program(build: Build) -> ExitCode {
    // Standard error says what the C++ says there, and no more: the runtime's panic message is
    // not printed (a panic is caught where the C++ catches an exception, and said below).
    std::panic::set_hook(Box::new(|_| {}));
    let code = match catch_unwind(AssertUnwindSafe(|| run(build))) {
        Ok(code) => code,
        Err(payload) => {
            if build.test
                && let Some(crash) = payload.downcast_ref::<SimulatedCrash>()
            {
                // A process that was killed leaves what it had written and nothing else. The stack
                // has been unwound to here, which is harmless because nothing in the swap does disk
                // or registry work when it is dropped (apply.rs, rule 2; the lock it let go is the
                // one the process ending would have let go); standard output holds nothing yet,
                // since every mode prints after its work.
                write_error(
                    format!(
                        "katana_updater: simulated crash before change {}: {}\n",
                        crash.operation, crash.description
                    )
                    .as_bytes(),
                );
                EXIT_SIMULATED_CRASH
            } else {
                write_error(
                    format!(
                        "katana_updater: unexpected error: {}\n",
                        panic_text(payload.as_ref())
                    )
                    .as_bytes(),
                );
                EXIT_INTERNAL
            }
        }
    };
    ExitCode::from(u8::try_from(code).unwrap_or(u8::MAX))
}
