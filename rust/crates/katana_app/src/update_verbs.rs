//! The UPDATE verbs: checking for, downloading and applying a newer Katana
//! (docs/update.md), the port of src/katana_app/update_verbs.hpp/.cpp. One
//! grammar for katana_cli, the window and its headless form; katana_mcp
//! reaches the read-only half through its own tool.
//!
//! ```text
//! UPDATE CHECK
//!     -> update=available|current|unreachable|unsupported|blocked
//!        installed=<v> latest=<v> files=<changed>/<total> download_bytes=<n>
//!        notes_url=<url> reason=<token>
//! UPDATE STATUS
//!     -> installed=<v> target=<t> kind=installer|archive|development
//!        staged=<v|none> skipped=<v|none> auto=on|off last_check=<UTC|never>
//!        token=present|absent last=<status>[:<reason>]
//! UPDATE DOWNLOAD CONFIRM
//!     -> progress lines, then staged=<v>
//! UPDATE APPLY CONFIRM
//!     -> ready=<v> transaction=<tx>; the caller then ends its session and
//!        starts the helper (see UpdateReply::apply)
//! UPDATE SKIP <version> | UPDATE SKIP NONE
//! UPDATE BLOCKED CLEAR
//!     -> blocked=none; the releases that were put back here are offered
//!        again
//! UPDATE SET AUTO ON|OFF
//! UPDATE TOKEN STATUS | UPDATE TOKEN CLEAR
//! ```
//!
//! CONFIRM is a word of the line, not a question asked back: a line without it
//! is refused and says what the line with it would do. A token is never a word
//! of a line - lines are logged - so there is no UPDATE TOKEN SET; the window's
//! Settings page writes the credential store itself, and a script sets
//! KATANA_UPDATE_TOKEN.
//!
//! What each surface may do ([`UpdateSurface`]):
//!
//! ```text
//! Cli             everything; APPLY CONFIRM must be the last line of the run
//!                 and relaunches nothing.
//! Window          everything; APPLY CONFIRM is followed by the window's own
//!                 close (its save question included) and relaunches katana.
//! HeadlessWindow  CHECK and STATUS only, and only when typed: no automatic
//!                 check, no per-user state written, no dialog. DOWNLOAD and
//!                 APPLY reply reason=headless.
//! Mcp             CHECK and STATUS only, through the katana_check_update
//!                 tool. An UPDATE line inside katana_run_commands is refused
//!                 naming that tool: an agent never downloads or applies, so
//!                 it never replaces the program its client holds open.
//! ```
//!
//! A copy that packaging did not write (no share/katana/install.json: a build
//! tree, a plain install) is kind=development: CHECK replies unsupported and
//! nothing is ever downloaded or applied to it.
//!
//! WHAT THE REPLIES ADD TO THE LINES ABOVE. A key is written only when it has
//! a value, so a reply without reason= is one that was carried out, and a
//! value is quoted when it holds a blank (cad's record value).
//!
//! ```text
//! UPDATE CHECK      detail="<the failure's own sentence>" after reason=, and
//!                   information=old before it when the release's `expires`
//!                   has passed (it is offered all the same). files= and
//!                   download_bytes= are there when the installed files were
//!                   hashed, which is for update=available alone.
//!                   reason=, beyond check.rs's: no_transport (the context
//!                   has no fetch), no_network (this build has no HTTP stack:
//!                   update=unsupported), install_record (install.json does
//!                   not read: update=unsupported), notify_only (available,
//!                   and installed by hand).
//!                   update=blocked (reason=blocked_before) has a detail
//!                   that is no failure's: that this release was put back
//!                   here, and that UPDATE BLOCKED CLEAR (Settings > Updates
//!                   in the window) offers it again.
//!                   last_check is recorded when the feed ANSWERED - a
//!                   manifest verified: available, current or blocked - and
//!                   not for a check that could not read it, so that an
//!                   automatic check that failed is due again; nor for a
//!                   copy that asks nobody (unsupported). It is written into
//!                   the state as that is at the moment of writing, so what
//!                   another program of the install kept while the installed
//!                   files were hashed - a skip, the switch, a release the
//!                   helper blocked - is kept.
//! UPDATE STATUS     target=none for a development copy; last=none when no
//!                   apply's outcome is recorded. A development copy is
//!                   staged=none last=none whatever the update folder holds:
//!                   those are an install's, and it is none. staged= is the
//!                   newest release in the update folder whose two records
//!                   (stage.rs) verify and which is above what is installed;
//!                   APPLY applies that one. A copy whose install record does
//!                   not read (UpdateContext::install_problem) has no word of
//!                   its own in kind=: the record says target=none
//!                   kind=development, and a warning record after it says
//!                   that the install record could not be read, and why.
//! UPDATE DOWNLOAD CONFIRM
//!                   each progress line is
//!                       progress bytes=<done>/<total> files=<done>/<total>
//!                   (compressed bytes fetched; changed files staged). When
//!                   nothing is staged the reply is CHECK's record - which
//!                   says why - and then staged=none; with no update folder,
//!                   staged=none reason=no_state. A download that fails is an
//!                   error whose context begins reason=<token> (stage.rs).
//!                   It asks the feed first, as CHECK does (and records the
//!                   check), so UpdateReply::offer is set by it too.
//! UPDATE APPLY CONFIRM
//!                   refused, with reason= first in the error's context:
//!                   not_staged, blocked_before, elevated, development_tree,
//!                   install_record, no_state, platform, or the preflight's
//!                   token (stage.rs); and state_unreadable when the state
//!                   file does not read - it is where the releases that were
//!                   put back here are kept, so without it the line cannot
//!                   know that this one was not, and applies nothing.
//!                   elevated is a Katana that was started "as
//!                   administrator" where that is the elevated half of a
//!                   split token (UpdateContext::elevated; katana_update's
//!                   process_is_elevated): the helper would run with those
//!                   rights from a copy in the per-user update folder, which
//!                   the same person's ordinary programs can write. It is the
//!                   last of these refusals and comes before the preflight,
//!                   so no helper copy is made and the release stays
//!                   downloaded: Katana started the ordinary way applies it.
//!                   DOWNLOAD is not refused. The sentence of blocked_before
//!                   names the line that offers the release again, UPDATE
//!                   BLOCKED CLEAR.
//! UPDATE SKIP, UPDATE SET AUTO, UPDATE BLOCKED CLEAR
//!                   skipped=<v|none>; auto=on|off; blocked=none. With no
//!                   update folder: the same with reason=no_state (blocked=
//!                   none reason=no_state), and nothing is kept. With a state
//!                   file that does not read: refused (InvalidState), and the
//!                   file is left as it is. Each changes its one member of
//!                   the state as that is at the moment of writing.
//! UPDATE BLOCKED CLEAR
//!                   empties the list of releases whose apply was put back
//!                   here (katana_update's state, `blocked`), all of them:
//!                   one start too slow for the health check blocks a release
//!                   that is sound, and this is the way back that does not
//!                   need the state file deleted by hand. The next CHECK
//!                   offers the release again and APPLY CONFIRM applies it.
//!                   UPDATE BLOCKED with another word, or none, is
//!                   InvalidArgument and says the one line there is. UPDATE
//!                   STATUS does not say what is blocked: CHECK does, for the
//!                   release the feed offers (update=blocked).
//! UPDATE TOKEN STATUS | CLEAR
//!                   token=present|absent, and never the token. reason=
//!                   no_store, token_unusable or store_unavailable when the
//!                   store could not say; after CLEAR, token=present
//!                   reason=environment when KATANA_UPDATE_TOKEN still gives
//!                   one, which is not this program's to remove.
//! any line          a record `warning text="..."` after the reply's own, for
//!                   what did not stop the line: a state file that does not
//!                   read (the defaults are used and it is not written over).
//! ```
//!
//! A headless window's DOWNLOAD and APPLY are replies, staged=none or
//! ready=none with reason=headless and text="..." naming who runs them; every
//! other line a surface may not run is an Unsupported error whose context is
//! reason=headless or reason=mcp. Where this version cannot install an update
//! at all (it is written for Windows; elsewhere a release is told of and no
//! more) DOWNLOAD and APPLY are Unsupported, reason=platform. Every reply,
//! error and progress line passes through katana_update's redact_token with
//! the context's token and the store's.
//!
//! Nothing here is the engine's work - the feed, the signature, the plan, the
//! staging and the helper are its - and nothing here is a front end's: what a
//! line needs comes in the context, so that katana_cli, the window, its
//! headless form and katana_mcp's tool run these very functions, and a test
//! runs them on a fake feed and a scratch folder.
//!
//! THE RUST SHAPE. The C++'s UpdateContext holds raw pointers (the trust, the
//! credential store, the launcher) and std::functions (the fetch, the
//! progress). Here it is [`UpdateContext<'a>`], whose references and closures
//! borrow for `'a`, as katana_update's own [`Fetch`], [`StopToken`] and
//! [`ProgressFn`] do: a test's context borrows its fake feed, store and
//! launcher, and the running program's ([`make_update_context`]) is
//! `UpdateContext<'static>`, its stores made once per thread and kept for the
//! program's life (the C++'s function-local statics). It is covariant in
//! `'a`, so a session can lend a `'static` context a progress printer of its
//! own for one line.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::SystemTime;

use katana_core::civil_date::{SECONDS_PER_DAY, civil_from_days, unix_seconds};
use katana_core::error::make_error;
use katana_core::path_text::{path_from_utf8, read_file_bytes};
use katana_core::{Error, ErrorCode, Result, Status};
use katana_update::check::{CheckResult, CheckStatus, apply_update_state};
use katana_update::feed::{MAX_MANIFEST_BYTES, MAX_SIGNATURE_BYTES};
use katana_update::plan::Plan;
use katana_update::{
    ApplyRequest, CredentialStore, FeedConfig, Fetch, Install, InstallKind, LastResult, Launcher,
    Manifest, ProgressFn, RegistryOps, RetryPolicy, StopToken, TOKEN_ENVIRONMENT_VARIABLE, Trust,
    UpdateState, append_cleanup, default_feed_config, fetch_failure_reason,
    install_root_of_executable, install_root_of_running_program, is_expired, is_newer_than,
    is_valid_transaction_id, make_environment_credential_store, make_system_credential_store,
    make_system_fs_ops, make_system_launcher, make_system_registry_ops, parse_version, preflight,
    process_is_elevated, production_trust, program_version, read_install, read_last_result,
    read_state, redact_token, run_path, stage, stage_path, staged_manifest_path,
    staged_signature_path, state_directory, verify_manifest, write_state,
};

use crate::import_records::{record_text, warning_text};
use crate::update_transport::make_update_fetch;

const USAGE: &str = "UPDATE CHECK | UPDATE STATUS | UPDATE DOWNLOAD CONFIRM | UPDATE APPLY CONFIRM | \
                     UPDATE SKIP <version>|NONE | UPDATE BLOCKED CLEAR | UPDATE SET AUTO ON|OFF | \
                     UPDATE TOKEN STATUS|CLEAR";

// What a line without its CONFIRM is told: that nothing was done, and what
// the line with the word does - so that the word is typed knowing it.
const DOWNLOAD_NEEDS_CONFIRM: &str = "UPDATE DOWNLOAD needs the word CONFIRM, and nothing was \
     downloaded: UPDATE DOWNLOAD CONFIRM asks the release server for the newest release and \
     downloads the files of it that differ from the installed ones into the per-user update \
     folder; the install itself is not changed";
const APPLY_NEEDS_CONFIRM: &str = "UPDATE APPLY needs the word CONFIRM, and nothing was changed: \
     UPDATE APPLY CONFIRM ends this session and has the update helper replace the installed files \
     with the downloaded release, putting every one back if the new version does not start";

// No word after TOKEN is repeated in what is said back: a person who types a
// token there has typed it into a log, and this must not write it twice.
const TOKEN_USAGE: &str = "UPDATE TOKEN takes STATUS or CLEAR and no other word. A token is never \
     typed on a line, because lines are logged: the window's Settings > Updates stores one, and \
     the environment variable KATANA_UPDATE_TOKEN gives a run one";

// UPDATE BLOCKED has one line, and a refusal says which: a list of releases
// is not a thing to guess the other words of.
const BLOCKED_USAGE: &str = "UPDATE BLOCKED takes CLEAR and no other word: UPDATE BLOCKED CLEAR is \
     the one line there is, and it forgets which releases were put back here, so that the next \
     UPDATE CHECK offers them again";

// What UPDATE CHECK says of a release it does not offer because an apply of
// it was put back here (update=blocked reason=blocked_before), and the way
// back, which is a person's to take: one start that was too slow for the
// health check is enough to block a release that is sound.
const BLOCKED_DETAIL: &str = "this release was installed here before, did not start and was put \
     back, so it is not offered; UPDATE BLOCKED CLEAR (Settings > Updates in the window) offers \
     it again";

// What the transport of a build without an HTTP stack answers every request
// with (the C++'s update_transport.cpp without GDAL). No server can cure it,
// so it is this copy that cannot update, not a feed that cannot be reached.
const NO_NETWORK_REASON: &str = "no_network";

// A copy of Katana replaces its own files on Windows only in this version: the
// swap's file system and the helper's launcher are not written for Linux
// (katana_update's fsops), and a macOS bundle is sealed. Elsewhere a release
// is told of and no more. The window's own switch for its buttons is
// kAppliesHere (src/katana_qt/update_workbench.cpp).
const INSTALLS_HERE: bool = cfg!(windows);

// The most progress lines one download says: a line each time another tenth
// has arrived, so that a log shows it moving and a reply stays a screenful
// however the transport chunks its transfer.
const PROGRESS_STEPS: u64 = 10;

// The surfaces that run every line, for the words of a refusal.
const WHO_RUNS_EVERY_LINE: &str = "the Katana window or katana_cli";

/// Which front end runs a line, and so which lines it may run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum UpdateSurface {
    #[default]
    Cli,
    Window,
    HeadlessWindow,
    Mcp,
}

/// Everything a line needs from the program that runs it. Nothing here is
/// looked up behind the caller's back, so a test builds one with a fake
/// transport, a test trust and a scratch folder and runs the real verbs.
#[derive(Default)]
pub struct UpdateContext<'a> {
    pub surface: UpdateSurface,
    /// What is installed (read_install of the running program's root).
    pub install: Install,
    /// Whose signature a manifest must carry. None: production_trust().
    pub trust: Option<&'a Trust>,
    /// Where the releases are: default_feed_config(install.target), with the
    /// token of `credentials` put in by make_update_context when there is one.
    pub feed: FeedConfig,
    /// The https transport (update_transport.rs, make_update_fetch). None: the
    /// network verbs reply update=unreachable reason=no_transport.
    pub fetch: Fetch<'a>,
    /// U, the per-user update folder (state_directory()). EMPTY means "write
    /// no per-user state", which is what a headless run passes: CHECK then
    /// neither reads nor records skipped, blocked or last_check.
    pub state_directory: PathBuf,
    /// TOKEN STATUS and TOKEN CLEAR. None: both reply reason=no_store.
    pub credentials: Option<&'a dyn CredentialStore>,
    /// Runs the helper's --selftest in APPLY's preflight. None: the system's.
    pub launcher: Option<&'a dyn Launcher>,
    /// The process the helper waits for before it swaps anything: the
    /// caller's own. 0: none.
    pub pid: u64,
    /// What the helper starts once the swap is done. Empty: nothing.
    pub relaunch: PathBuf,
    /// Cancels a CHECK or a DOWNLOAD between files and between requests.
    pub stop: StopToken<'a>,
    /// Bytes fetched so far and to fetch, for a progress bar. May be None.
    pub progress: ProgressFn<'a>,
    /// DOWNLOAD's progress lines as they happen, for a log. May be None; the
    /// lines are in the reply either way.
    pub progress_line: Option<&'a dyn Fn(&str)>,
    /// The time CHECK records as last_check, UTC seconds. 0: the system clock.
    pub now_epoch: i64,
    /// Why `install` is not what the install's own records say: install.json
    /// was there and did not read (make_update_context). Empty: it read, or
    /// there was none. Such a copy is not called a development build, which it
    /// is not: CHECK replies update=unsupported reason=install_record with
    /// this as its detail, and nothing is downloaded or applied.
    pub install_problem: String,
    /// This process holds the elevated half of a split token: it was started
    /// "as administrator" (process_is_elevated, which make_update_context
    /// asks). APPLY CONFIRM is then refused, reason=elevated, before a helper
    /// copy is made. A test sets it; nothing here asks the system.
    pub elevated: bool,
}

/// What UPDATE CHECK found, for a caller that shows it rather than prints it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UpdateOffer {
    pub check: CheckResult,
    /// Of total_files; 0 unless check.status is Available.
    pub changed_files: u64,
    pub total_files: u64,
    /// What DOWNLOAD would fetch.
    pub download_bytes: u64,
}

/// What UPDATE STATUS says, for the same reason.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UpdateStatus {
    pub installed_version: String,
    /// "" for a development copy.
    pub target: String,
    pub kind: InstallKind,
    /// "" when nothing is staged.
    pub staged_version: String,
    /// Skipped, automatic, last check; the defaults when no state is kept.
    pub state: UpdateState,
    pub token_present: bool,
    /// The last apply's outcome, when one is recorded.
    pub last: Option<LastResult>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct UpdateReply {
    /// The reply as the grammar above gives it: what katana_cli prints and
    /// the window logs.
    pub text: String,
    /// Set by UPDATE CHECK.
    pub offer: Option<UpdateOffer>,
    /// Set by UPDATE STATUS.
    pub status: Option<UpdateStatus>,
    /// Set by UPDATE APPLY CONFIRM when the release is staged, verified and
    /// its helper has passed the preflight. The line itself changes nothing
    /// in the install: the CALLER ends its session first (the window after
    /// its save question; katana_cli at the end of its run) and only then
    /// calls launch_apply with it. A caller that does not end, or whose close
    /// was cancelled, simply does not call it, and the release stays staged.
    pub apply: Option<ApplyRequest>,
}

fn same_word(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

// The line's words. No quotes are read: no word of this grammar holds a blank.
fn words_of(line: &str) -> Vec<&str> {
    line.split([' ', '\t', '\r', '\n'])
        .filter(|word| !word.is_empty())
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Check,
    Status,
    Download,
    Apply,
    Skip,
    BlockedClear,
    SetAuto,
    TokenStatus,
    TokenClear,
}

// The line an action is, for the words of a refusal.
fn line_of(action: Action) -> &'static str {
    match action {
        Action::Check => "UPDATE CHECK",
        Action::Status => "UPDATE STATUS",
        Action::Download => "UPDATE DOWNLOAD",
        Action::Apply => "UPDATE APPLY",
        Action::Skip => "UPDATE SKIP",
        Action::BlockedClear => "UPDATE BLOCKED CLEAR",
        Action::SetAuto => "UPDATE SET AUTO",
        Action::TokenStatus => "UPDATE TOKEN STATUS",
        Action::TokenClear => "UPDATE TOKEN CLEAR",
    }
}

struct UpdateLine {
    action: Action,
    // SKIP: the version as katana_update's to_string writes it; empty for NONE.
    version: String,
    // SET AUTO.
    on: bool,
}

impl UpdateLine {
    fn of(action: Action) -> Self {
        Self {
            action,
            version: String::new(),
            on: false,
        }
    }
}

fn parse(line: &str) -> Result<UpdateLine> {
    let words = words_of(line);
    let refused =
        |why: String| Error::new(ErrorCode::InvalidArgument, format!("{why}; usage: {USAGE}"));
    if words.is_empty() || !same_word(words[0], "UPDATE") {
        return Err(Error::new(ErrorCode::InvalidArgument, "not an UPDATE line"));
    }
    if words.len() < 2 {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!("usage: {USAGE}"),
        ));
    }
    let action = words[1];
    if same_word(action, "CHECK") || same_word(action, "STATUS") {
        let action = if same_word(action, "CHECK") {
            Action::Check
        } else {
            Action::Status
        };
        if words.len() != 2 {
            return Err(refused(format!("{} takes no other word", line_of(action))));
        }
        return Ok(UpdateLine::of(action));
    }
    if same_word(action, "DOWNLOAD") || same_word(action, "APPLY") {
        let action = if same_word(action, "DOWNLOAD") {
            Action::Download
        } else {
            Action::Apply
        };
        if words.len() == 3 && same_word(words[2], "CONFIRM") {
            return Ok(UpdateLine::of(action));
        }
        if words.len() == 2 {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                if action == Action::Download {
                    DOWNLOAD_NEEDS_CONFIRM
                } else {
                    APPLY_NEEDS_CONFIRM
                },
            ));
        }
        return Err(refused(format!(
            "{} takes CONFIRM and no other word",
            line_of(action)
        )));
    }
    if same_word(action, "SKIP") {
        if words.len() != 3 {
            return Err(refused(String::from(
                "UPDATE SKIP takes one version, or NONE",
            )));
        }
        let mut parsed = UpdateLine::of(Action::Skip);
        if same_word(words[2], "NONE") {
            return Ok(parsed);
        }
        let Ok(version) = parse_version(words[2]) else {
            return Err(refused(format!(
                "'{}' is not a version (two to four numbers with dots between, as 0.3.0)",
                words[2]
            )));
        };
        parsed.version = katana_update::version::to_string(&version);
        return Ok(parsed);
    }
    if same_word(action, "BLOCKED") {
        if words.len() == 3 && same_word(words[2], "CLEAR") {
            return Ok(UpdateLine::of(Action::BlockedClear));
        }
        return Err(Error::new(ErrorCode::InvalidArgument, BLOCKED_USAGE));
    }
    if same_word(action, "SET") {
        if words.len() != 4
            || !same_word(words[2], "AUTO")
            || !(same_word(words[3], "ON") || same_word(words[3], "OFF"))
        {
            return Err(refused(String::from(
                "UPDATE SET takes AUTO ON or AUTO OFF",
            )));
        }
        let mut parsed = UpdateLine::of(Action::SetAuto);
        parsed.on = same_word(words[3], "ON");
        return Ok(parsed);
    }
    if same_word(action, "TOKEN") {
        if words.len() == 3 && same_word(words[2], "STATUS") {
            return Ok(UpdateLine::of(Action::TokenStatus));
        }
        if words.len() == 3 && same_word(words[2], "CLEAR") {
            return Ok(UpdateLine::of(Action::TokenClear));
        }
        return Err(Error::new(ErrorCode::InvalidArgument, TOKEN_USAGE));
    }
    Err(refused(format!("'{action}' is not an UPDATE action")))
}

// ---- what a reply is made of ----------------------------------------------------------------

// " key=value", the value quoted when it holds a blank; nothing for no value,
// so that a key that is there always says something.
fn add(record: &mut String, key: &str, value: &str) {
    if value.is_empty() {
        return;
    }
    record.push(' ');
    record.push_str(key);
    record.push('=');
    record.push_str(&record_text(value));
}

fn joined(lines: &[String]) -> String {
    let mut text = String::new();
    for line in lines {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(line);
    }
    text
}

// UTC seconds as the manifest writes a time: 2026-10-20T08:00:00Z.
fn utc_text(epoch: i64) -> String {
    let date = civil_from_days(epoch.div_euclid(SECONDS_PER_DAY));
    let of_day = epoch.rem_euclid(SECONDS_PER_DAY);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        date.year,
        date.month,
        date.day,
        of_day / 3_600,
        of_day / 60 % 60,
        of_day % 60
    )
}

fn now_of(context: &UpdateContext<'_>) -> i64 {
    if context.now_epoch != 0 {
        return context.now_epoch;
    }
    unix_seconds(SystemTime::now())
}

fn trust_of<'t>(context: &UpdateContext<'t>) -> &'t Trust {
    match context.trust {
        Some(trust) => trust,
        None => production_trust(),
    }
}

// std::stop_token::stop_requested.
fn stop_requested(stop: StopToken<'_>) -> bool {
    stop.is_some_and(|flag| flag.load(Ordering::Relaxed))
}

// A copy nothing is ever downloaded to or applied to: one packaging did not
// write, or one whose own record of what it is does not read.
fn is_installed_copy(context: &UpdateContext<'_>) -> bool {
    context.install.kind != InstallKind::Development && context.install_problem.is_empty()
}

// ---- the token is in nothing that is said ---------------------------------------------------

// Every token this line could meet: the one the feed would send, and the one
// the store holds now where that is another.
fn secrets_of(context: &UpdateContext<'_>) -> Vec<String> {
    let mut secrets: Vec<String> = Vec::new();
    if !context.feed.token.is_empty() {
        secrets.push(context.feed.token.clone());
    }
    if let Some(credentials) = context.credentials
        && let Ok(stored) = credentials.read()
        && !stored.is_empty()
        && !secrets.contains(&stored)
    {
        secrets.push(stored);
    }
    secrets
}

fn without_secrets(mut text: String, secrets: &[String]) -> String {
    for secret in secrets {
        text = redact_token(&text, secret);
    }
    text
}

// ---- the per-user state ---------------------------------------------------------------------

struct KeptState {
    // The defaults when none is kept, or it did not read.
    state: UpdateState,
    // The context has a state folder.
    kept: bool,
    // False: state.json is there and does not read.
    readable: bool,
    // Why not.
    problem: String,
}

fn read_kept_state(context: &UpdateContext<'_>) -> KeptState {
    let mut kept = KeptState {
        state: UpdateState::default(),
        kept: false,
        readable: true,
        problem: String::new(),
    };
    if context.state_directory.as_os_str().is_empty() {
        return kept;
    }
    kept.kept = true;
    match read_state(&context.state_directory) {
        Ok(state) => kept.state = state,
        Err(error) => {
            kept.readable = false;
            kept.problem = error.describe();
        }
    }
    kept
}

// What is said of a state file that does not read. It is not written over: a
// file from a newer Katana holds what this one cannot keep, and one that is
// damaged holds the releases that were rolled back here.
fn unreadable_state(kept: &KeptState) -> String {
    warning_text(&format!(
        "the update settings could not be read, so the defaults were used and nothing was \
         written over them: {}",
        kept.problem
    ))
}

// A setting changed in state.json: read at the moment it is written, so that
// what another program wrote meanwhile is kept but for this one member.
fn change_state(context: &UpdateContext<'_>, change: impl FnOnce(&mut UpdateState)) -> Status {
    let mut kept = read_kept_state(context);
    if !kept.readable {
        return Err(make_error(
            ErrorCode::InvalidState,
            "the update settings could not be read, and are not written over",
            kept.problem,
        ));
    }
    change(&mut kept.state);
    write_state(&context.state_directory, &kept.state)
}

// ---- what is staged -------------------------------------------------------------------------

struct Staged {
    manifest: Manifest,
    // U/stage-<sequence>.
    directory: PathBuf,
}

// What std::filesystem::symlink_status answers for `path`. On Windows the
// C++'s toolchain answers for what a junction or a symbolic link points at
// (src/katana_update/plan.cpp, look), and the stage's readers rely on nothing
// else (find_staged's comment), so there the link is followed as it was.
fn status_of(path: &Path) -> std::io::Result<std::fs::Metadata> {
    if cfg!(windows) {
        std::fs::metadata(path)
    } else {
        std::fs::symlink_metadata(path)
    }
}

// A file of at most `limit` bytes, whole; nothing for one that is not there,
// is not a file, or is longer (the stage folder is the user's to write: a
// "manifest" of a gigabyte is not read to learn that it is not one).
fn small_file(path: &Path, limit: u64) -> Option<Vec<u8>> {
    let status = status_of(path).ok()?;
    if !status.is_file() {
        return None;
    }
    let size = std::fs::metadata(path).ok()?.len();
    if size > limit {
        return None;
    }
    read_file_bytes(path).ok()
}

// The units of a file name as the file system holds it: bytes on POSIX, UTF-16
// on Windows. Never converted to text: on Windows a name need not be Unicode
// (an unpaired surrogate is a name there), and the update folder is the
// user's to write.
#[cfg(windows)]
fn name_units(name: &std::ffi::OsStr) -> Vec<u32> {
    use std::os::windows::ffi::OsStrExt;
    name.encode_wide().map(u32::from).collect()
}

#[cfg(not(windows))]
fn name_units(name: &std::ffi::OsStr) -> Vec<u32> {
    use std::os::unix::ffi::OsStrExt;
    name.as_bytes()
        .iter()
        .map(|&byte| u32::from(byte))
        .collect()
}

// The sequence a folder name stage-<n> carries. The name is read as the file
// system holds it and is never converted (name_units): one entry that could
// not be would otherwise end every STATUS and APPLY.
fn stage_sequence(name: &std::ffi::OsStr) -> Option<u64> {
    const PREFIX: &str = "stage-";
    let units = name_units(name);
    if units.len() <= PREFIX.len() || units.len() > 40 {
        return None;
    }
    let mut ascii = String::with_capacity(units.len());
    for unit in units {
        if !(0x20..=0x7E).contains(&unit) {
            return None; // no letter of stage-<digits> lies outside ASCII
        }
        ascii.push(char::from_u32(unit)?);
    }
    let digits = ascii.strip_prefix(PREFIX)?;
    // std::from_chars: digits and nothing else, no sign, every one of them
    // taken, and a number that does not fit is no number.
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits.parse::<u64>().ok()
}

// The release that is downloaded and ready: a folder U/stage-<sequence> that
// holds the two records stage() writes last (stage.rs), which verify with
// this program's keys, name that very sequence and are above what is
// installed. The newest when there are several. A folder without them is a
// download that did not finish; one that is not above the install is what a
// finished update left behind.
//
// A LINK to a folder is not told from a folder here, and need not be. Nothing
// is believed of a stage but what the signature covers, and the helper
// verifies the release again with its own keys and COPIES each file out of
// the stage, hashing the copy (apply.rs, steps 1 and 3): a stage that lies
// somewhere else is still only the user's own folder.
fn find_staged(context: &UpdateContext<'_>) -> Option<Staged> {
    let mut newest: Option<Staged> = None;
    if context.state_directory.as_os_str().is_empty() || !is_installed_copy(context) {
        return newest;
    }
    let Ok(entries) = std::fs::read_dir(&context.state_directory) else {
        return newest;
    };
    for entry in entries {
        // The C++'s directory_iterator stops at its first error.
        let Ok(entry) = entry else {
            break;
        };
        let folder = entry.path();
        let Some(sequence) = stage_sequence(&entry.file_name()) else {
            continue;
        };
        if !status_of(&folder).is_ok_and(|status| status.is_dir()) {
            continue; // a file of that name is not a stage
        }
        let json = small_file(&staged_manifest_path(&folder), MAX_MANIFEST_BYTES);
        let signature = small_file(&staged_signature_path(&folder), MAX_SIGNATURE_BYTES);
        let (Some(json), Some(signature)) = (json, signature) else {
            continue;
        };
        let Ok(manifest) = verify_manifest(json, signature, trust_of(context), &context.install)
        else {
            continue;
        };
        if manifest.sequence != sequence || !is_newer_than(&manifest, &context.install) {
            continue;
        }
        if newest
            .as_ref()
            .is_none_or(|found| manifest.sequence > found.manifest.sequence)
        {
            newest = Some(Staged {
                manifest,
                directory: folder,
            });
        }
    }
    newest
}

// ---- UPDATE CHECK ---------------------------------------------------------------------------

#[derive(Default)]
struct Checked {
    offer: UpdateOffer,
    // When the installed files were hashed.
    plan: Option<Plan>,
    // Why they could not be.
    plan_error: Option<Error>,
    // The release's `expires` has passed.
    information_is_old: bool,
    // Records that follow the reply's own.
    warnings: Vec<String>,
}

fn unsupported(mut checked: Checked, reason: &str, detail: &str) -> Checked {
    checked.offer.check.status = CheckStatus::Unsupported;
    checked.offer.check.reason = String::from(reason);
    checked.offer.check.detail = String::from(detail);
    checked
}

// Asks the feed, applies what the person has decided about the release it
// offers, works out what the update would fetch, and records when the feed
// answered. An error only where the question could not be put (a stop already
// asked for): a feed that cannot be read is an answer.
fn run_check(context: &UpdateContext<'_>) -> Result<Checked> {
    let mut checked = Checked {
        offer: UpdateOffer {
            check: CheckResult {
                installed_version: katana_update::version::to_string(&context.install.version),
                ..CheckResult::default()
            },
            ..UpdateOffer::default()
        },
        ..Checked::default()
    };

    if !context.install_problem.is_empty() {
        return Ok(unsupported(
            checked,
            "install_record",
            &context.install_problem,
        ));
    }
    // Before anything is asked of anyone: a development copy has no target to
    // ask a feed for, and a build tree must stay offline.
    if context.install.kind == InstallKind::Development {
        return Ok(unsupported(
            checked,
            "development_tree",
            "this copy of Katana is a development build, which is not updated in place",
        ));
    }
    if context.fetch.is_none() {
        checked.offer.check.status = CheckStatus::Unreachable;
        checked.offer.check.reason = String::from("no_transport");
        checked.offer.check.detail =
            String::from("this program was given no way to reach the release server");
        return Ok(checked);
    }

    checked.offer.check = katana_update::check::check(
        &context.fetch,
        &context.feed,
        &context.install,
        trust_of(context),
        context.stop,
    )?;
    if checked.offer.check.status == CheckStatus::Unreachable
        && checked.offer.check.reason == NO_NETWORK_REASON
    {
        checked.offer.check.status = CheckStatus::Unsupported;
    }
    // The feed answered when a manifest verified: available or current.
    let answered_by_feed = checked.offer.check.manifest.is_some();

    // Read now, not before the request: what another program wrote while this
    // one waited for the network is in it.
    let kept = read_kept_state(context);
    if kept.kept && !kept.readable {
        checked.warnings.push(unreadable_state(&kept));
    }
    if kept.kept && kept.readable {
        apply_update_state(&mut checked.offer.check, &kept.state);
        if checked.offer.check.status == CheckStatus::Blocked {
            checked.offer.check.detail = String::from(BLOCKED_DETAIL);
        }
    }

    if checked.offer.check.status == CheckStatus::Available
        && let Some(manifest) = &checked.offer.check.manifest
    {
        checked.information_is_old = is_expired(manifest, now_of(context));
        if manifest.notify_only {
            // An announcement: there is nothing to hash against and nothing to fetch.
            checked.offer.check.reason = String::from("notify_only");
        } else {
            match katana_update::plan::plan(manifest, &context.install, context.stop) {
                Ok(plan) => {
                    checked.offer.changed_files = plan.changed_files;
                    checked.offer.total_files = plan.total_files;
                    checked.offer.download_bytes = plan.download_bytes;
                    checked.plan = Some(plan);
                }
                Err(error) if stop_requested(context.stop) => return Err(error),
                Err(error) => {
                    // The release is known all the same; what it would fetch is not.
                    checked.warnings.push(warning_text(&format!(
                        "what the update would change could not be worked out: {}",
                        error.describe()
                    )));
                    checked.plan_error = Some(error);
                }
            }
        }
    }

    // Recorded through change_state, which reads the file again at the moment
    // it writes. `kept` is from before the installed files were hashed, which
    // takes seconds: written back as it was read, it would undo whatever
    // another program of this install kept meanwhile - a skip, the switch, or
    // the release the helper has just put back and blocked.
    if answered_by_feed && kept.kept && kept.readable {
        let now = now_of(context);
        if let Err(error) = change_state(context, |state| state.last_check_epoch = now) {
            checked.warnings.push(warning_text(&format!(
                "the time of this check could not be recorded: {}",
                error.describe()
            )));
        }
    }
    Ok(checked)
}

fn check_record(checked: &Checked) -> String {
    let check = &checked.offer.check;
    let mut record = format!("update={}", katana_update::check::to_string(check.status));
    add(&mut record, "installed", &check.installed_version);
    add(&mut record, "latest", &check.latest_version);
    if checked.plan.is_some() {
        record.push_str(&format!(
            " files={}/{} download_bytes={}",
            checked.offer.changed_files, checked.offer.total_files, checked.offer.download_bytes
        ));
    }
    add(&mut record, "notes_url", &check.notes_url);
    if checked.information_is_old {
        record.push_str(" information=old");
    }
    add(&mut record, "reason", &check.reason);
    add(&mut record, "detail", &check.detail);
    record
}

// The check's record and the warnings after it.
fn check_lines(checked: &Checked) -> Vec<String> {
    let mut lines = vec![check_record(checked)];
    lines.extend(checked.warnings.iter().cloned());
    lines
}

fn check(context: &UpdateContext<'_>) -> Result<UpdateReply> {
    let checked = run_check(context)?;
    Ok(UpdateReply {
        text: joined(&check_lines(&checked)),
        offer: Some(checked.offer),
        ..UpdateReply::default()
    })
}

// ---- UPDATE STATUS --------------------------------------------------------------------------

fn token_is_present(context: &UpdateContext<'_>) -> bool {
    if !context.feed.token.is_empty() {
        return true;
    }
    context
        .credentials
        .is_some_and(|credentials| credentials.read().is_ok())
}

fn status(context: &UpdateContext<'_>) -> Result<UpdateReply> {
    let mut warnings: Vec<String> = Vec::new();
    let mut said = UpdateStatus {
        installed_version: katana_update::version::to_string(&context.install.version),
        target: context.install.target.clone(),
        kind: context.install.kind,
        ..UpdateStatus::default()
    };
    if !context.install_problem.is_empty() {
        // kind= has three words and none of them is "an install whose record
        // does not read" (the module's head): the record says what the context
        // holds, and this says that it is not what the copy is.
        warnings.push(warning_text(&format!(
            "this copy's install record could not be read, so what kind of copy it is is not \
             known and nothing is downloaded or applied to it: {}",
            context.install_problem
        )));
    }
    let kept = read_kept_state(context);
    if kept.kept && !kept.readable {
        warnings.push(unreadable_state(&kept));
    }
    said.state = kept.state.clone();
    said.token_present = token_is_present(context);
    // What is downloaded and how the last apply ended are an install's: a
    // development copy shares the update folder with an installed one on the
    // same machine, and neither is its own.
    if kept.kept && is_installed_copy(context) {
        if let Some(staged) = find_staged(context) {
            said.staged_version = katana_update::version::to_string(&staged.manifest.version);
        }
        match read_last_result(&context.state_directory) {
            Ok(last) => said.last = last,
            Err(error) => warnings.push(warning_text(&format!(
                "how the last update ended could not be read: {}",
                error.describe()
            ))),
        }
    }

    let none_or = |value: &str| {
        if value.is_empty() {
            String::from("none")
        } else {
            record_text(value)
        }
    };
    let mut record = format!("installed={}", record_text(&said.installed_version));
    record.push_str(&format!(" target={}", none_or(said.target.as_str())));
    record.push_str(&format!(
        " kind={}",
        katana_update::install::to_string(said.kind)
    ));
    record.push_str(&format!(
        " staged={}",
        none_or(said.staged_version.as_str())
    ));
    record.push_str(&format!(
        " skipped={}",
        none_or(said.state.skipped_version.as_str())
    ));
    record.push_str(if said.state.automatic {
        " auto=on"
    } else {
        " auto=off"
    });
    record.push_str(&format!(
        " last_check={}",
        if said.state.last_check_epoch > 0 {
            utc_text(said.state.last_check_epoch)
        } else {
            String::from("never")
        }
    ));
    record.push_str(if said.token_present {
        " token=present"
    } else {
        " token=absent"
    });
    let mut last = String::from("none");
    if let Some(result) = &said.last
        && !result.status.is_empty()
    {
        last = result.status.clone();
        if !result.reason.is_empty() {
            last.push(':');
            last.push_str(&result.reason);
        }
    }
    record.push_str(&format!(" last={}", record_text(&last)));

    let mut lines = vec![record];
    lines.extend(warnings);
    Ok(UpdateReply {
        text: joined(&lines),
        status: Some(said),
        ..UpdateReply::default()
    })
}

// ---- UPDATE DOWNLOAD CONFIRM ----------------------------------------------------------------

// A failure of the engine's staging or preflight as this line's: its own
// sentence, and its reason token first in the context, where a caller reads it
// (fetch_failure_reason: the transport's failures carry a status there, the
// engine's own a reason= already).
fn engine_error(error: &Error) -> Error {
    let mut said = error.clone();
    if said.context.starts_with("reason=") {
        return said;
    }
    let reason = format!("reason={}", fetch_failure_reason(error));
    said.context = if said.context.is_empty() {
        reason
    } else {
        format!("{reason}; {}", said.context)
    };
    said
}

fn cannot_install_here(action: Action) -> Error {
    make_error(
        ErrorCode::Unsupported,
        format!(
            "{} is not available on this system: this version of Katana installs an update on \
             Windows only. UPDATE CHECK tells of a newer release, and its notes say where to get it",
            line_of(action)
        ),
        "reason=platform",
    )
}

// DOWNLOAD's progress, said as lines: the first report, one each time another
// tenth of the bytes has arrived, and - the bytes all there - one more when
// the files have followed them into the stage folder, so that the last line
// says every changed file is staged. (The bytes of a request arrive before
// its files are unpacked and verified: "all the bytes, none of the files" is
// a state a line can truthfully show.) The engine's progress is a Fn, so what
// changes between reports is in cells.
struct ProgressLines<'r> {
    progress: ProgressFn<'r>,
    progress_line: Option<&'r dyn Fn(&str)>,
    manifest: &'r Manifest,
    plan: &'r Plan,
    stage_directory: PathBuf,
    secrets: &'r [String],
    lines: RefCell<Vec<String>>,
    said: Cell<bool>,
    // The tenth the last line was said at.
    step: Cell<u64>,
    // The files it counted.
    files: Cell<u64>,
}

impl<'r> ProgressLines<'r> {
    fn new(
        context: &'r UpdateContext<'r>,
        manifest: &'r Manifest,
        plan: &'r Plan,
        stage_directory: PathBuf,
        secrets: &'r [String],
    ) -> Self {
        Self {
            progress: context.progress,
            progress_line: context.progress_line,
            manifest,
            plan,
            stage_directory,
            secrets,
            lines: RefCell::new(Vec::new()),
            said: Cell::new(false),
            step: Cell::new(0),
            files: Cell::new(0),
        }
    }

    fn report(&self, done: u64, total: u64) {
        if let Some(progress) = self.progress {
            progress(done, total);
        }
        // Nothing to fetch is all of it fetched. (The C++'s unsigned arithmetic
        // wraps where Rust's would stop; no payload is near it.)
        let step = done
            .min(total)
            .wrapping_mul(PROGRESS_STEPS)
            .checked_div(total)
            .unwrap_or(PROGRESS_STEPS);
        let complete = done >= total;
        // The transport reports many times a second, and the files are
        // counted on the disk: they are counted only for a line that is about
        // to be said, or once the bytes are all there.
        if self.said.get() && step <= self.step.get() && !complete {
            return;
        }
        let files = self.staged_files();
        if self.said.get() && step <= self.step.get() && files == self.files.get() {
            return;
        }
        self.said.set(true);
        self.step.set(self.step.get().max(step));
        self.files.set(files);
        let line = without_secrets(
            format!(
                "progress bytes={}/{total} files={files}/{}",
                done.min(total),
                self.plan.changed_files
            ),
            self.secrets,
        );
        if let Some(progress_line) = self.progress_line {
            progress_line(&line);
        }
        self.lines.borrow_mut().push(line);
    }

    fn take(&self) -> Vec<String> {
        self.lines.take()
    }

    // The changed files that are in the stage folder: a file is given its name
    // there only once it has hashed to the signed value (stage.rs), so being
    // there is being done.
    fn staged_files(&self) -> u64 {
        let mut count = 0;
        for planned in &self.plan.files {
            if !planned.changed || planned.index >= self.manifest.files.len() {
                continue;
            }
            let file = self
                .stage_directory
                .join(path_from_utf8(&self.manifest.files[planned.index].path));
            if status_of(&file).is_ok_and(|status| status.is_file()) {
                count += 1;
            }
        }
        count
    }
}

fn download(context: &UpdateContext<'_>, secrets: &[String]) -> Result<UpdateReply> {
    if !INSTALLS_HERE {
        return Err(cannot_install_here(Action::Download));
    }
    if context.state_directory.as_os_str().is_empty() {
        // Nowhere to put a download, and nothing was asked of anyone.
        return Ok(UpdateReply {
            text: String::from("staged=none reason=no_state"),
            ..UpdateReply::default()
        });
    }
    let checked = run_check(context)?;
    let mut reply = UpdateReply {
        offer: Some(checked.offer.clone()),
        ..UpdateReply::default()
    };
    let answer = &checked.offer.check;
    let offered = answer
        .manifest
        .as_ref()
        .filter(|manifest| answer.status == CheckStatus::Available && !manifest.notify_only);
    let Some(manifest) = offered else {
        // Nothing to download, and the check's own record says why: current,
        // skipped, blocked, unreachable, unsupported, or a release that is
        // only an announcement.
        let mut lines = check_lines(&checked);
        lines.push(String::from("staged=none"));
        reply.text = joined(&lines);
        return Ok(reply);
    };
    let Some(plan) = &checked.plan else {
        let mut error = checked.plan_error.clone().unwrap_or_else(|| {
            Error::new(ErrorCode::Internal, "the installed files were not compared")
        });
        error.message = format!(
            "what the update would change could not be worked out, so nothing was downloaded: {}",
            error.message
        );
        error.context = if error.context.is_empty() {
            String::from("reason=plan_failed")
        } else {
            format!("reason=plan_failed; {}", error.context)
        };
        return Err(error);
    };

    // The payload is fetched the way the manifest was (feed.rs).
    let mut feed = context.feed.clone();
    feed.route = answer.route;
    let stage_directory = stage_path(&context.state_directory, manifest.sequence);
    let progress = ProgressLines::new(context, manifest, plan, stage_directory.clone(), secrets);
    let report: &dyn Fn(u64, u64) = &|done: u64, total: u64| progress.report(done, total);
    stage(
        &context.fetch,
        &feed,
        manifest,
        plan,
        &stage_directory,
        context.stop,
        Some(report),
    )
    .map_err(|error| engine_error(&error))?;
    let mut lines = progress.take();
    lines.push(format!(
        "staged={}",
        record_text(&katana_update::version::to_string(&manifest.version))
    ));
    lines.extend(checked.warnings.iter().cloned());
    reply.text = joined(&lines);
    Ok(reply)
}

// ---- UPDATE APPLY CONFIRM -------------------------------------------------------------------

fn apply(context: &UpdateContext<'_>) -> Result<UpdateReply> {
    if !INSTALLS_HERE {
        return Err(cannot_install_here(Action::Apply));
    }
    if !context.install_problem.is_empty() {
        return Err(make_error(
            ErrorCode::Unsupported,
            format!(
                "this copy's install record could not be read, so nothing is applied to it: {}",
                context.install_problem
            ),
            "reason=install_record",
        ));
    }
    if context.install.kind == InstallKind::Development {
        return Err(make_error(
            ErrorCode::Unsupported,
            "this copy of Katana is a development build, which is never updated in place",
            "reason=development_tree",
        ));
    }
    if context.state_directory.as_os_str().is_empty() {
        return Err(make_error(
            ErrorCode::InvalidState,
            "no update is downloaded: this session keeps no update folder to download one into",
            "reason=no_state",
        ));
    }
    let Some(staged) = find_staged(context) else {
        return Err(make_error(
            ErrorCode::InvalidState,
            "no update is downloaded and ready: UPDATE DOWNLOAD CONFIRM downloads the newest \
             release",
            "reason=not_staged",
        ));
    };
    let version = katana_update::version::to_string(&staged.manifest.version);
    // A release whose apply was rolled back here is not tried again: the
    // helper found that it does not start on this machine (apply.rs, step 7).
    // Not by this line, that is: UPDATE BLOCKED CLEAR is the person's way of
    // saying that it is to be tried again, and the refusal names it.
    // Which releases those are is in the state file, so one that does not
    // read stops the line: its defaults say that none was ever put back,
    // and the file that is damaged, or was written by a newer Katana, is the
    // very one that holds them. The way out is said too, since nothing here
    // takes it for the person: the file is theirs, and is not written over.
    let kept = read_kept_state(context);
    if !kept.readable {
        return Err(make_error(
            ErrorCode::InvalidState,
            format!(
                "the update settings could not be read, so whether Katana {version} was put back \
                 here before is not known, and it is not applied ({}); with that file removed the \
                 settings are the defaults again and the update can be applied",
                kept.problem
            ),
            "reason=state_unreadable",
        ));
    }
    if kept
        .state
        .blocked_sequences
        .contains(&staged.manifest.sequence)
    {
        return Err(make_error(
            ErrorCode::InvalidState,
            format!(
                "Katana {version} could not be installed here before and was put back, so it is \
                 not applied again. A later release will be offered; UPDATE BLOCKED CLEAR \
                 (Settings > Updates in the window) offers this one again"
            ),
            "reason=blocked_before",
        ));
    }
    // Started "as administrator": the helper would run with those rights from
    // its copy in the update folder, which the same person's ordinary programs
    // can write (katana_update's process_is_elevated). The last refusal, and
    // before the preflight: no copy is made for a helper that will not be
    // started, and the release stays where it was downloaded to.
    if context.elevated {
        return Err(make_error(
            ErrorCode::Unsupported,
            format!(
                "this Katana was started as administrator, and an update is not installed from \
                 there: the update helper would run with administrator rights from a folder that \
                 ordinary programs can write. Start Katana the ordinary way and update again; \
                 Katana {version} stays downloaded and is not fetched a second time"
            ),
            "reason=elevated",
        ));
    }

    // The helper is copied out of the install and proves it starts now, while
    // this session is still here to say that it does not.
    let system: Box<dyn Launcher>;
    let launcher: &dyn Launcher = match context.launcher {
        Some(launcher) => launcher,
        None => {
            system = make_system_launcher();
            system.as_ref()
        }
    };
    let prepared = preflight(
        &staged.manifest,
        &context.install,
        &staged.directory,
        &context.state_directory,
        launcher,
    )
    .map_err(|error| engine_error(&error))?;

    let request = ApplyRequest {
        root: context.install.root.clone(),
        stage_directory: staged.directory,
        state_directory: context.state_directory.clone(),
        helper: prepared.helper,
        transaction: prepared.transaction,
        wait_pid: context.pid,
        relaunch: context.relaunch.clone(),
    };
    Ok(UpdateReply {
        text: format!(
            "ready={} transaction={}",
            record_text(&version),
            record_text(&request.transaction)
        ),
        apply: Some(request),
        ..UpdateReply::default()
    })
}

// ---- UPDATE SKIP, UPDATE SET AUTO -----------------------------------------------------------

fn text_reply(text: impl Into<String>) -> UpdateReply {
    UpdateReply {
        text: text.into(),
        ..UpdateReply::default()
    }
}

fn skip(context: &UpdateContext<'_>, line: &UpdateLine) -> Result<UpdateReply> {
    if context.state_directory.as_os_str().is_empty() {
        return Ok(text_reply("skipped=none reason=no_state"));
    }
    change_state(context, |state| {
        state.skipped_version = line.version.clone();
    })?;
    Ok(text_reply(format!(
        "skipped={}",
        if line.version.is_empty() {
            "none"
        } else {
            line.version.as_str()
        }
    )))
}

fn set_automatic(context: &UpdateContext<'_>, line: &UpdateLine) -> Result<UpdateReply> {
    if context.state_directory.as_os_str().is_empty() {
        // What an unkept setting is: the default, which is on (state.rs).
        return Ok(text_reply(format!(
            "{} reason=no_state",
            if UpdateState::default().automatic {
                "auto=on"
            } else {
                "auto=off"
            }
        )));
    }
    change_state(context, |state| state.automatic = line.on)?;
    Ok(text_reply(if line.on { "auto=on" } else { "auto=off" }))
}

// ---- UPDATE BLOCKED CLEAR -------------------------------------------------------------------

// Forgets which releases were put back here, so that the next check offers
// the newest one again and APPLY applies it. Every one of them: the list is
// kept so that a release that does not start is not tried at each start, and
// a person who asks for another try asks for the release that is offered now
// - there is no older one to be had from the feed. Taken out of the state as
// it is at the moment of writing (change_state), as a skip is.
fn blocked_clear(context: &UpdateContext<'_>) -> Result<UpdateReply> {
    if context.state_directory.as_os_str().is_empty() {
        return Ok(text_reply("blocked=none reason=no_state"));
    }
    change_state(context, |state| state.blocked_sequences.clear())?;
    Ok(text_reply("blocked=none"))
}

// ---- UPDATE TOKEN STATUS | CLEAR ------------------------------------------------------------

// Whether a token is stored: present or absent, and never the token. When the
// store cannot say - what it holds is no usable token, or it cannot be asked -
// its own sentence follows, which names the place and not the value
// (credential.rs).
fn token_record(store: &dyn CredentialStore) -> String {
    match store.read() {
        Ok(_) => String::from("token=present"),
        Err(error) if error.code == ErrorCode::NotFound => String::from("token=absent"),
        Err(error) => {
            let mut record = String::from("token=absent");
            add(
                &mut record,
                "reason",
                if error.code == ErrorCode::InvalidState {
                    "token_unusable"
                } else {
                    "store_unavailable"
                },
            );
            add(&mut record, "detail", &error.message);
            record
        }
    }
}

fn no_store_record(context: &UpdateContext<'_>) -> String {
    format!(
        "{} reason=no_store",
        if context.feed.token.is_empty() {
            "token=absent"
        } else {
            "token=present"
        }
    )
}

fn token_status(context: &UpdateContext<'_>) -> Result<UpdateReply> {
    Ok(text_reply(match context.credentials {
        None => no_store_record(context),
        Some(store) => token_record(store),
    }))
}

fn token_clear(context: &UpdateContext<'_>) -> Result<UpdateReply> {
    let Some(store) = context.credentials else {
        return Ok(text_reply(no_store_record(context)));
    };
    let cleared = store.clear();
    // A store that cannot be written (the environment's) has nothing of this
    // program's to remove: what it still gives is said below. Any other
    // failure is the store's, and nothing can be said of what it holds.
    if let Err(error) = &cleared
        && error.code != ErrorCode::Unsupported
    {
        return Err(error.clone());
    }
    let mut text = token_record(store);
    if text == "token=present" {
        let from_environment = store.r#where() == "environment";
        add(
            &mut text,
            "reason",
            if from_environment {
                "environment"
            } else {
                "store_unavailable"
            },
        );
        let detail = if from_environment {
            format!(
                "the environment variable {TOKEN_ENVIRONMENT_VARIABLE} still gives one, which \
                 Katana does not change: remove it from the environment that starts Katana"
            )
        } else {
            match &cleared {
                Ok(()) => String::from("the store still holds one"),
                Err(error) => error.message.clone(),
            }
        };
        add(&mut text, "detail", &detail);
    }
    Ok(text_reply(text))
}

// ---- what a surface may not run -------------------------------------------------------------

// A reply for the two lines a headless window answers rather than refuses
// (the module's head): the record's own key saying nothing was done, the
// reason, and who does run the line.
fn headless_reply(action: Action) -> UpdateReply {
    let mut text = String::from(if action == Action::Download {
        "staged=none"
    } else {
        "ready=none"
    });
    add(&mut text, "reason", "headless");
    add(
        &mut text,
        "text",
        &format!(
            "a headless run {}: {WHO_RUNS_EVERY_LINE} does",
            if action == Action::Download {
                "downloads no update"
            } else {
                "installs no update"
            }
        ),
    );
    text_reply(text)
}

// Nothing when `surface` may run the line; a reply or a refusal when it may not.
fn refused_by_surface(surface: UpdateSurface, action: Action) -> Option<Result<UpdateReply>> {
    if action == Action::Check || action == Action::Status {
        return None;
    }
    match surface {
        UpdateSurface::Cli | UpdateSurface::Window => None,
        UpdateSurface::HeadlessWindow => {
            if action == Action::Download || action == Action::Apply {
                return Some(Ok(headless_reply(action)));
            }
            Some(Err(make_error(
                ErrorCode::Unsupported,
                format!(
                    "{} reads or changes what Katana keeps for one user, and a headless run keeps \
                     nothing per user: {WHO_RUNS_EVERY_LINE} runs it",
                    line_of(action)
                ),
                "reason=headless",
            )))
        }
        UpdateSurface::Mcp => Some(Err(make_error(
            ErrorCode::Unsupported,
            format!(
                "{} is not katana_mcp's to run: an agent checks for an update and reads this \
                 copy's status (katana_check_update) and no more, so that it never replaces the \
                 program its client holds open; {WHO_RUNS_EVERY_LINE} runs it",
                line_of(action)
            ),
            "reason=mcp",
        ))),
    }
}

fn run(context: &UpdateContext<'_>, line: &str, secrets: &[String]) -> Result<UpdateReply> {
    let parsed = parse(line)?;
    if let Some(refused) = refused_by_surface(context.surface, parsed.action) {
        return refused;
    }
    match parsed.action {
        Action::Check => check(context),
        Action::Status => status(context),
        Action::Download => download(context, secrets),
        Action::Apply => apply(context),
        Action::Skip => skip(context, &parsed),
        Action::BlockedClear => blocked_clear(context),
        Action::SetAuto => set_automatic(context, &parsed),
        Action::TokenStatus => token_status(context),
        Action::TokenClear => token_clear(context),
    }
}

// ---- the running program's context ----------------------------------------------------------

// The token of a program a person runs: the environment variable first - it
// is an override everywhere (credential.rs) - and then the system's store,
// where the window's Settings page keeps one. Written and cleared in the
// system's store alone: a variable is not this program's to change.
struct ProgramCredentials {
    environment: Box<dyn CredentialStore>,
    system: Box<dyn CredentialStore>,
    from_environment: AtomicBool,
}

impl ProgramCredentials {
    fn new() -> Self {
        Self {
            environment: make_environment_credential_store(),
            system: make_system_credential_store(),
            from_environment: AtomicBool::new(false),
        }
    }
}

impl CredentialStore for ProgramCredentials {
    fn read(&self) -> Result<String> {
        let from_environment = match self.environment.read() {
            Ok(token) => {
                self.from_environment.store(true, Ordering::Relaxed);
                return Ok(token);
            }
            Err(error) => error,
        };
        // Whatever the variable's fault, the store is asked: a variable that
        // holds something unusable must not hide a good token kept there.
        let from_system = match self.system.read() {
            Ok(token) => {
                self.from_environment.store(false, Ordering::Relaxed);
                return Ok(token);
            }
            Err(error) => error,
        };
        // Neither gives one. "There is none" only when the variable says so
        // too: one that is set and unusable is said, so that it is corrected.
        if from_environment.code != ErrorCode::NotFound {
            return Err(from_environment);
        }
        Err(from_system)
    }

    fn write(&self, token: &[u8]) -> Status {
        let written = self.system.write(token);
        if written.is_ok() {
            self.from_environment.store(false, Ordering::Relaxed);
        }
        written
    }

    fn clear(&self) -> Status {
        self.system.clear()
    }

    // The place the last token read or written is in.
    fn r#where(&self) -> &str {
        if self.from_environment.load(Ordering::Relaxed) {
            self.environment.r#where()
        } else {
            self.system.r#where()
        }
    }
}

// The stores a program's contexts point at: made once and kept as long as the
// program, as the C++'s function-local statics are (a context holds
// references to them). Once per thread, since the stores are not Sync: one in
// katana_cli and katana_mcp, which run their lines on one thread, and one per
// test thread. A window that hands a context to a job's thread (step 7) needs
// them Sync, or a context that owns them.
struct ProgramStores {
    registry: Box<dyn RegistryOps>,
    environment: Box<dyn CredentialStore>,
    program: ProgramCredentials,
}

fn program_stores() -> &'static ProgramStores {
    thread_local! {
        static STORES: &'static ProgramStores = Box::leak(Box::new(ProgramStores {
            registry: make_system_registry_ops(),
            environment: make_environment_credential_store(),
            program: ProgramCredentials::new(),
        }));
    }
    STORES.with(|stores| *stores)
}

// What a program is, `root` being the install it was found in: that install's
// own records, or - with no install to speak of - the program as it was
// built, a development copy.
fn read_install_of(
    context: &mut UpdateContext<'_>,
    root: &Result<PathBuf>,
    registry: &dyn RegistryOps,
) {
    if let Ok(built) = program_version() {
        context.install.version = built;
    }
    let Ok(root) = root else {
        return; // not in a bin/ beside a share/: a test's program, a tool's
    };
    match read_install(root, registry) {
        Ok(install) => context.install = install,
        Err(error) => {
            context.install.root = root.clone();
            context.install_problem = error.describe();
        }
    }
}

// The context of the program found in `root` (make_update_context): the
// running one's, or the one a caller named.
fn context_of(surface: UpdateSurface, root: &Result<PathBuf>) -> UpdateContext<'static> {
    let stores = program_stores();

    // A headless window and katana_mcp keep and read nothing a PERSON chose:
    // no state folder, and no store but the variable their own run was given.
    // What the copy IS is read for every surface, and on Windows one part of
    // that is per user as well: the folder the per-user installer recorded
    // (HKCU, registry.rs), which is what tells kind=installer from
    // kind=archive. It is the install's own record, read and never written.
    let per_user = matches!(surface, UpdateSurface::Cli | UpdateSurface::Window);

    let mut context = UpdateContext {
        surface,
        ..UpdateContext::default()
    };
    read_install_of(&mut context, root, stores.registry.as_ref());
    context.feed = default_feed_config(&context.install.target);
    let credentials: &'static dyn CredentialStore = if per_user {
        &stores.program
    } else {
        stores.environment.as_ref()
    };
    context.credentials = Some(credentials);
    if let Ok(token) = credentials.read() {
        context.feed.token = token;
    }
    // In a build without an HTTP stack this was a transport that answers every
    // request with no_network (the C++'s update_transport.hpp), which CHECK
    // reports as update=unsupported; the Rust app always has its HTTP stack.
    context.fetch = make_update_fetch();
    if per_user {
        context.state_directory = state_directory();
    }
    // How the process was started is nothing a person keeps: asked for every
    // surface, of the system, and never of a file.
    context.elevated = process_is_elevated();
    context
}

/// True for a line whose verb is UPDATE.
pub fn is_update_line(line: &str) -> bool {
    words_of(line)
        .first()
        .is_some_and(|word| same_word(word, "UPDATE"))
}

/// True for an UPDATE APPLY line, with its CONFIRM or without: the line a
/// front end that runs many lines must run LAST (Session::is_update_apply).
/// The words are split as run_update_line splits them, so the line that is
/// held to be the last one is exactly the line that would prepare an apply.
pub fn is_update_apply_line(line: &str) -> bool {
    let words = words_of(line);
    words.len() >= 2 && same_word(words[0], "UPDATE") && same_word(words[1], "APPLY")
}

/// Runs an UPDATE line. InvalidArgument for a line the grammar does not take;
/// Unsupported for a line this surface may not run (the message names the
/// surface that may); otherwise a reply, whose records say what happened - a
/// feed that cannot be read is a reply (update=unreachable), not an error.
pub fn run_update_line(context: &UpdateContext<'_>, line: &str) -> Result<UpdateReply> {
    // Read before the line runs: the token a TOKEN CLEAR removes is still one
    // that nothing may show.
    let secrets = secrets_of(context);
    let reply = run(context, line, &secrets);
    if secrets.is_empty() {
        return reply;
    }
    let mut reply = match reply {
        Ok(reply) => reply,
        Err(mut error) => {
            error.message = without_secrets(error.message, &secrets);
            error.context = without_secrets(error.context, &secrets);
            return Err(error);
        }
    };
    reply.text = without_secrets(std::mem::take(&mut reply.text), &secrets);
    if let Some(offer) = &mut reply.offer {
        // What a window shows of a check is these two, not the text.
        offer.check.reason = without_secrets(std::mem::take(&mut offer.check.reason), &secrets);
        offer.check.detail = without_secrets(std::mem::take(&mut offer.check.detail), &secrets);
    }
    Ok(reply)
}

/// The context of the running program: its install record, the default feed,
/// the system credential store's token, the https transport, whether the
/// process was started as administrator (`elevated`), and - unless the
/// surface is HeadlessWindow or Mcp - the per-user state folder. `pid` and
/// `relaunch` are left for the caller. The stores it makes live as long as
/// the program.
pub fn make_update_context(surface: UpdateSurface) -> UpdateContext<'static> {
    context_of(surface, &install_root_of_running_program())
}

/// The same for the program whose executable is at `executable` (an absolute
/// path; install_root_of_executable finds its install) instead of the running
/// one: the install's records are that copy's, and everything else - the
/// feed, the token, the transport, the state folder - is as above. What a
/// test builds a scratch install for. (The C++'s second makeUpdateContext.)
pub fn make_update_context_for(
    surface: UpdateSurface,
    executable: &Path,
) -> UpdateContext<'static> {
    context_of(surface, &install_root_of_executable(executable))
}

/// What a caller does with an UpdateReply::apply it will NOT launch - its
/// close was cancelled, the helper could not be started, a later APPLY took
/// its place. UPDATE APPLY CONFIRM copied the helper to U/run-<tx> for it
/// (stage.rs, preflight), and only a helper that ran would have cleared that
/// copy away: this lists the folder for the sweep the next start makes
/// (state.rs, cleanup.list). The release stays staged, and nothing else is
/// touched. An error is the list's, which could not be written: the copy
/// then stays, a program's size in the user's own folder.
pub fn abandon_apply(request: &ApplyRequest) -> Status {
    if request.state_directory.as_os_str().is_empty()
        || !is_valid_transaction_id(&request.transaction)
    {
        return Ok(()); // no request the preflight made: there is no folder of its to list
    }
    let files = make_system_fs_ops(&RetryPolicy::default());
    append_cleanup(
        &request.state_directory,
        &run_path(&request.state_directory, &request.transaction),
        files.as_ref(),
    )
}

/// The lines HELP UPDATE and --help print.
pub fn update_help_text() -> &'static str {
    concat!(
        "Update    UPDATE CHECK  whether a newer Katana has been released:\n",
        "          update=available|current|unreachable|unsupported|blocked, with\n",
        "          installed=, latest=, files=<changed>/<total>, download_bytes=,\n",
        "          notes_url= and reason=\n",
        "          UPDATE STATUS  this copy: installed=, target=,\n",
        "          kind=installer|archive|development, staged=, skipped=, auto=,\n",
        "          last_check=, token=present|absent and last= (how the last update\n",
        "          ended)\n",
        "          UPDATE DOWNLOAD CONFIRM  downloads the files of the newest release that\n",
        "          differ from the installed ones into the per-user update folder:\n",
        "          progress lines, then staged=<version>. The install is not changed\n",
        "          UPDATE APPLY CONFIRM  installs the downloaded release: ready=<version>\n",
        "          transaction=<id>, the session ends, and the update helper replaces\n",
        "          the files, putting every one back if the new version does not\n",
        "          start. It must be the last line of a katana_cli run, which starts\n",
        "          nothing again; the window closes and reopens. A Katana that was\n",
        "          started as administrator installs no update (reason=elevated):\n",
        "          started the ordinary way it does, and the download is kept\n",
        "          UPDATE SKIP <version> | UPDATE SKIP NONE  do not offer that version;\n",
        "          offer it again\n",
        "          UPDATE BLOCKED CLEAR  offer again a release that was installed here,\n",
        "          did not start and was put back (update=blocked): blocked=none\n",
        "          UPDATE SET AUTO ON|OFF  whether the window checks by itself, once a day\n",
        "          UPDATE TOKEN STATUS | UPDATE TOKEN CLEAR  whether an access token for a\n",
        "          private release repository is stored (present or absent, never the\n",
        "          token); remove the stored one. No line takes a token, because lines\n",
        "          are logged: the window's Settings > Updates stores one, and the\n",
        "          environment variable KATANA_UPDATE_TOKEN gives a run one\n",
        "          A line without its CONFIRM is refused, saying what the line with it\n",
        "          does. A copy that was not installed from a release (a build tree) is\n",
        "          kind=development and is never updated. A headless window checks and\n",
        "          reports only when asked, and katana_mcp runs no UPDATE line: its tool\n",
        "          katana_check_update checks and reports, and nothing more\n",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // The C++'s std::format("{:04}-{:02}-...") of a sys_seconds: the test's
    // clock, 1792368000, is 2026-10-19T00:00:00Z (tests/app/test_update_verbs.cpp,
    // the head), and 1,700,000,000 s is 2023-11-14T22:13:20Z.
    #[test]
    fn a_time_is_written_as_the_manifest_writes_one() {
        assert_eq!(utc_text(1792368000), "2026-10-19T00:00:00Z");
        assert_eq!(utc_text(1700000000), "2023-11-14T22:13:20Z");
    }

    // stage-<digits> and nothing else, as std::from_chars reads the digits.
    #[test]
    fn a_stage_folder_is_named_by_its_sequence_alone() {
        use std::ffi::OsStr;
        assert_eq!(
            stage_sequence(OsStr::new("stage-1792312800")),
            Some(1792312800)
        );
        assert_eq!(stage_sequence(OsStr::new("stage-")), None);
        assert_eq!(stage_sequence(OsStr::new("stage-1792312800x")), None);
        assert_eq!(stage_sequence(OsStr::new("stage-+17")), None);
        assert_eq!(stage_sequence(OsStr::new("run-17")), None);
        assert_eq!(
            stage_sequence(OsStr::new("stage-99999999999999999999")),
            None
        );
    }
}
