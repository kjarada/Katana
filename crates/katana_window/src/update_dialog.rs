//! Help > Check for Updates: what the update system knows, in two sentences, and the buttons that
//! fit it (docs/desktop.md, "Help > Check for Updates"). The port of update_dialog.
//!
//! THE DIALOG DOES NONE OF THE WORK and reads nothing of the update engine. It shows an
//! `UpdateView` - plain data its owner sets (UpdateWorkbench in the window, a test's own) - and
//! every button either builds the UPDATE line a person would type (katana_app's update_verbs) and
//! hands it to the window's one executor (host::command_runner), or calls one callback of its
//! context. What the line did comes back as the next view, never as a guess of the dialog's: a
//! press changes no text here by itself.
//!
//! WHAT TURNS THE ENGINE'S RECORDS INTO A VIEW is beside it, in the second half of this file ("THE
//! DECISIONS"): functions of their arguments, with no window, no disk and no clock, which the
//! owner calls and the tests drive with records written by hand.
//!
//! Non-modal, kept by its owner, and no button is a default: egui has no default button, so Enter
//! presses nothing.
//!
//! THE STATES, each with the first sentence in updateStateText, the rest in updateDetailText, and
//! exactly these buttons shown (the others are drawn hidden, which a press refuses):
//!
//! ```text
//! Idle         "Katana 0.2.0" / "Not checked for updates yet."  [Check now]. Before the first
//!              answer, and what a headless run shows: it checks only when asked.
//! Checking     "Checking for updates..." / "You can keep working."  [Cancel]
//! Current      "Katana 0.2.0 is up to date." / "Last checked 14:02."  [Check now]
//! NotOffered   a newer release this copy does not offer, by `reason`: "skipped": "Katana 0.3.0
//!              is available, and you chose to skip it." / "Settings > Updates offers it again."
//!              "blocked_before": "Katana 0.3.0 could not be installed here before, and is not
//!              offered again." / "A later release will be offered, and Settings > Updates offers
//!              this one again." (its button settingsUpdateUnblock runs UPDATE BLOCKED CLEAR)
//!              [Check now]
//! Available    "Update Available" / "Katana 0.3.0 (you have 0.2.0). 23 of 411 files changed,
//!              71 MB to download."  [Update] [Download only] [Skip this version] [Release notes]
//!              [Later]. With `information_is_old` the detail adds "The update information is
//!              old: a newer release may exist." With `notify_only` (a copy that cannot replace
//!              its own files) the detail's second sentence is "This copy cannot update itself:
//!              the release notes say where to download it." and Update and Download only are
//!              hidden.
//! Downloading  "Downloading 31 of 71 MB (12 of 23 files)." / "You can keep working."  the
//!              progress bar, [Cancel]. The files are said only when the owner knows how many are
//!              done.
//! Ready        "Katana 0.3.0 is ready." / "Katana will close, update and reopen in a few
//!              seconds."  [Restart and update] [Later]. With `waiting_for_jobs` the detail is
//!              "Katana will close when the running job has finished, then update and reopen.";
//!              with `waiting_for_dialog`, "Katana will close when the dialog that is open has
//!              been closed, then update and reopen."; and Restart and update is disabled in
//!              both: the press is made.
//! Blocked      "Close these first: katana_mcp.exe (4412)." / "Then press Retry."  [Retry]
//! RolledBack   what the last apply ended as, by `last_status` (katana_update's LastResult):
//!              "rolled_back": "Katana 0.3.0 could not be installed, so 0.2.0 was restored." /
//!              "Reason: <reason>." - and, when the reason is that the new version did not start
//!              (health_exit, health_timeout), which is when the helper sets the release aside,
//!              "A later release will be offered, and Settings > Updates offers this one again."
//!              after it; "refused": "Katana 0.3.0 could not be installed; nothing was changed."
//!              - and, when the reason is that an earlier update is not settled yet
//!              (interrupted_update), "Update again; if this is said again, close Katana and run:
//!              <recover_command>" after the reason; "rollback_incomplete": "Katana 0.3.0 could
//!              not be installed, and 0.2.0 could not be put back completely." with "Close Katana
//!              and run: <recover_command>" after the reason. No button: it is read and closed.
//! CheckFailed  "Could not check for updates: <reason>."  [Retry]; for the reason http_404, "No
//!              update information found." / "If the releases are private, add an access token in
//!              Settings > Updates."
//! Unsupported  "Updates are not available for this copy of Katana: <reason>."  No button.
//! ```
//!
//! <reason> is a person's words for the engine's token (`update_reason_words`); a megabyte is a
//! million bytes, rounded to the nearest (`update_megabytes`).
//!
//! WHAT EACH BUTTON DOES:
//!
//! ```text
//! updateCheckNow       the line UPDATE CHECK
//! updateRetry          CheckFailed: UPDATE CHECK; Blocked: UPDATE APPLY CONFIRM
//! updateInstall        context.update: download, then apply, then close - one click, which is
//!                      more than one line
//! updateDownloadOnly   the line UPDATE DOWNLOAD CONFIRM
//! updateSkip           the line UPDATE SKIP <latest version>
//! updateNotes          context.open_notes with the release's notes URL
//! updateRestart        the line UPDATE APPLY CONFIRM
//! updateCancel         context.cancel: the running check or download
//! updateLater          context.later, then the dialog is hidden
//! ```
//!
//! A button whose line has no executor, or whose callback is empty, is disabled. A line the
//! executor refused is said under the two sentences, in the error colour (updateProblemText), as
//! is `UpdateView::problem` - what the owner could not do at the last press; both go with the
//! next view.
//!
//! Object names: updateDialog; updateStateText, updateDetailText, updateProblemText (hidden while
//! empty), updateProgress (shown while checking, busy, and while downloading), and the nine
//! buttons above.
//!
//! THE RUST SHAPE. The C++'s owner holds a pointer to the dialog and sets its view from inside
//! its own line; here the dialog is borrowed while it runs a line, so the owner POSTS the view
//! (`UpdateViewPost`, a handle the dialog gives out) and the dialog takes it at its next look:
//! the start of its frame, the start of each of its methods, and straight after a line it ran,
//! before it says the line's refusal - the order the C++ has (docs/rust.md, "A dialog follows the
//! document at its next look"). The callbacks are handed the window as `&mut dyn Host` when they
//! run, as every dialog's executor is.
//!
//! LIFETIME. The dialog copies what it shows, so the view it was given may go. Its callbacks are
//! its owner's: an owner that goes first calls `detach`, after which every button but Later is
//! disabled and nothing is called.

use std::cell::RefCell;
use std::rc::Rc;

use egui::{RichText, Ui};
use katana_app::update_verbs::{UpdateOffer, UpdateStatus};
use katana_core::Result;
use katana_update::check::CheckStatus;
use katana_update::{
    InstallKind, LastResult, UpdateState, Version, check_is_due, is_expired,
    is_valid_transaction_id, parse_version,
};

use crate::command_log::{
    folded, is_update_token_verb_word, same_ignoring_case, update_token_line_words,
};
use crate::dialog_layout as layout;
use crate::drive::controls;
use crate::drive::registry::{ControlKind, ControlRecord, simplified};
use crate::host::{Host, VerbOutcome};
use crate::shell::DialogFrame;
use crate::survey::survey_dialogs::hidden;
use crate::theme;

pub use crate::command_log::update_logged_line;

/// A million bytes, the megabyte of a download's size as a browser and a release page say it (SI;
/// the binary unit is the mebibyte).
const MEGABYTE: u64 = 1_000_000;
/// The progress bar's range: thousandths, as the status bar's job progress is.
const PROGRESS_STEPS: i32 = 1000;

/// A person's words for each token the engine gives as a reason: what a check could not do
/// (katana_update's check), what the transport under it and under the download refused (feed,
/// fetch_failure_reason), why a download or the helper's preflight stopped (stage) and how an
/// apply ended (apply, ApplyResult::reason), with the verbs' own (update_verbs: no_transport,
/// no_store, and the two refusals of a copy that may not install here and now, platform and
/// elevated). Each stands after a colon and before a full stop, and where there is something a
/// person can do about it, says what.
const REASON_WORDS: &[(&str, &str)] = &[
    // A check.
    (
        "offline",
        "this computer is offline, or the release server did not answer",
    ),
    (
        "http_403",
        "the release server refused the request (error 403)",
    ),
    ("http_404", "no update information was found (error 404)"),
    (
        "http_429",
        "the release server is limiting requests (error 429); try again later",
    ),
    (
        "bad_signature",
        "the update information is not signed by Katana's release key",
    ),
    (
        "wrong_product",
        "the update information is for another product",
    ),
    (
        "wrong_target",
        "the update information is for another kind of computer",
    ),
    (
        "wrong_channel",
        "the update information is for another release channel",
    ),
    ("malformed", "the update information could not be read"),
    (
        "too_large",
        "the update information is longer than update information can be",
    ),
    (
        "html_page",
        "a web page answered in the place of the release server; this network may want you to \
         sign in first",
    ),
    (
        "asset_missing",
        "the release is missing a file the update needs",
    ),
    (
        "verify_unavailable",
        "the signature of the update information could not be checked on this computer",
    ),
    (
        "untrusted_key",
        "the update information is signed by a key this copy does not trust",
    ),
    (
        "placeholder_keys",
        "this build carries placeholder update keys and accepts no release",
    ),
    (
        "partial_downloads",
        "this network does not allow partial downloads",
    ),
    ("cancelled", "it was cancelled"),
    ("disabled_build", "this build has no update feed"),
    (
        "development_tree",
        "it is a development build, not an installed release",
    ),
    ("root_not_writable", "its folder cannot be written by you"),
    (
        "headless",
        "a headless run does not download or install updates",
    ),
    ("skipped", "you chose to skip that version"),
    (
        "blocked_before",
        "that version could not be installed here before",
    ),
    ("no_transport", "this build has no network access"),
    ("no_store", "this session has no credential store"),
    // A copy that may not install: where this version installs nothing (it is written for
    // Windows; the download, the apply and the helper refuse alike), and a Katana started as
    // administrator, whose helper would run with those rights from a folder ordinary programs can
    // write (katana_update's process_is_elevated).
    (
        "platform",
        "this copy of Katana cannot install an update on this kind of computer; the release \
         notes say where to download it",
    ),
    (
        "elevated",
        "Katana was started as administrator; start it the ordinary way, not as administrator, \
         and update again - the download is kept",
    ),
    // The transport, under a check and under a download.
    (
        "bad_range",
        "the release server sent another part of the download than was asked for",
    ),
    (
        "bad_redirect",
        "the release server sent the request on to an address that cannot be used",
    ),
    (
        "too_many_redirects",
        "the release server sent the request on too many times",
    ),
    ("error", "the release server's answer could not be used"),
    // A download.
    ("staged_corrupt", "a downloaded file could not be unpacked"),
    (
        "write_failed",
        "the folder the download is kept in could not be written",
    ),
    (
        "stage_busy",
        "another Katana is downloading this update now",
    ),
    (
        "low_disk_space",
        "there is not enough free disk space for the download",
    ),
    (
        "short_range",
        "the release server sent less of the download than was asked for",
    ),
    (
        "unverified_manifest",
        "the update information was not verified",
    ),
    (
        "no_token",
        "the releases are private and no access token is stored",
    ),
    (
        "payload_mismatch",
        "the release's download is not the one its update information names",
    ),
    // The helper's preflight.
    ("helper_missing", "the update helper program is missing"),
    (
        "helper_not_started",
        "the update helper could not be started",
    ),
    (
        "helper_timeout",
        "the update helper did not answer in time",
    ),
    (
        "helper_failed",
        "the update helper did not pass its own test",
    ),
    // An apply.
    ("locked", "another update was already running"),
    // An earlier update of this install is still open: one a crash interrupted that could not be
    // put back, or one whose helper died while the new version was proving that it starts -
    // installed, and neither confirmed nor put back yet, which a start of the window or the next
    // logon settles (katana_update's recover).
    (
        "interrupted_update",
        "an earlier update is not settled yet",
    ),
    (
        "invalid_request",
        "the update was started with a request it could not read",
    ),
    ("install_record", "the install record could not be read"),
    (
        "staged_manifest",
        "the downloaded update is missing or not signed by Katana's release key",
    ),
    ("notify_only", "this release can only be installed by hand"),
    (
        "applier_too_new",
        "the release needs a newer update helper than it brought",
    ),
    (
        "downgrade",
        "the downloaded version is not newer than the installed one",
    ),
    ("blocked", "other Katana programs were still open"),
    ("scan_failed", "the running programs could not be listed"),
    ("staged_missing", "a downloaded file was missing"),
    (
        "staged_hash",
        "a downloaded file did not match the release's signature",
    ),
    (
        "reparse_point",
        "a folder of the install is a link to somewhere else",
    ),
    (
        "path_conflict",
        "a file stands where a folder is needed, or a folder where a file is",
    ),
    ("copy_failed", "a file could not be copied into the install"),
    ("space", "there is not enough free disk space"),
    ("journal", "the update's journal could not be written"),
    ("io_error", "a file could not be read or written"),
    (
        "transaction_exists",
        "an update with the same number was already there",
    ),
    (
        "rename_failed",
        "a file was in use and could not be replaced",
    ),
    (
        "health_timeout",
        "the new version did not finish starting in time",
    ),
    ("health_exit", "the new version did not start"),
    (
        "rollback_failed",
        "the old version could not be put back completely",
    ),
    (
        "internal_error",
        "the update stopped on an unexpected error",
    ),
];

/// What the dialog is showing: one of the states of the module comment.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum UpdateDialogState {
    #[default]
    Idle,
    Checking,
    Current,
    NotOffered,
    Available,
    Downloading,
    Ready,
    Blocked,
    RolledBack,
    CheckFailed,
    Unsupported,
}

/// What the dialog shows. Every member has a default; a state reads only the members its sentences
/// name (the list in the module comment).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UpdateView {
    pub state: UpdateDialogState,
    /// "0.2.0"
    pub installed_version: String,
    /// "0.3.0": the release offered, staged, blocked or rolled back.
    pub latest_version: String,
    /// Current: the time as shown, "14:02"; empty leaves the sentence out.
    pub last_checked: String,
    /// Available: how many of the install's files the release changes. Downloading: the changed
    /// files are the "23" of "12 of 23 files".
    pub changed_files: u64,
    pub total_files: u64,
    /// Available: what Update would fetch. Downloading: the same, with what has arrived.
    pub download_bytes: u64,
    pub downloaded_bytes: u64,
    /// Downloading: the files staged so far, when the download says. None: bytes alone.
    pub downloaded_files: Option<u64>,
    pub notes_url: String,
    /// The engine's token (katana_update's check and apply): CheckFailed, Unsupported, NotOffered,
    /// RolledBack.
    pub reason: String,
    /// Blocked: "katana_mcp.exe (4412)"
    pub holders: String,
    /// RolledBack: "rolled_back", "refused" or "rollback_incomplete".
    pub last_status: String,
    /// RolledBack, for the two endings the helper's recovery settles (rollback_incomplete, and
    /// refused over an earlier update that is not settled): the command to run by hand.
    pub recover_command: String,
    /// One sentence: what the owner could not do at the last press. Shown in updateProblemText in
    /// any state; empty hides it.
    pub problem: String,
    /// Available.
    pub information_is_old: bool,
    /// Available.
    pub notify_only: bool,
    /// Ready: the close waits for a running job.
    pub waiting_for_jobs: bool,
    /// Ready: the close waits for a dialog that is open.
    pub waiting_for_dialog: bool,
}

/// A line handed to the window's one executor, with the window: what `Host::run` is, as a value a
/// test or an owner can stand in for.
pub type UpdateRun = Box<dyn FnMut(&mut dyn Host, &str) -> VerbOutcome>;
/// One of the owner's callbacks, handed the window when it runs.
pub type UpdateCall = Box<dyn FnMut(&mut dyn Host)>;
/// Release notes: the URL opened in the person's browser.
pub type UpdateOpenNotes = Box<dyn FnMut(&mut dyn Host, &str)>;

/// What the dialog is given by whoever owns it. Every member may be empty, and a button with
/// nothing to call is disabled, so a test builds the dialog alone.
#[derive(Default)]
pub struct UpdateDialogContext {
    /// The window's one executor: the lines the buttons build go here.
    pub run: Option<UpdateRun>,
    /// Update: download, apply and close, in one click.
    pub update: Option<UpdateCall>,
    /// Cancel: the check or the download that is running.
    pub cancel: Option<UpdateCall>,
    pub open_notes: Option<UpdateOpenNotes>,
    /// Later: told before the dialog hides (an owner waiting to close stops waiting).
    pub later: Option<UpdateCall>,
}

/// The executor every window gives its dialogs: `Host::run`.
pub fn run_through_host() -> UpdateRun {
    Box::new(|host: &mut dyn Host, line: &str| host.run(line))
}

// The lines the buttons build, for the owner that must know them again when they come back through
// the executor.
pub const UPDATE_CHECK_LINE: &str = "UPDATE CHECK";
pub const UPDATE_DOWNLOAD_LINE: &str = "UPDATE DOWNLOAD CONFIRM";
pub const UPDATE_APPLY_LINE: &str = "UPDATE APPLY CONFIRM";

/// "UPDATE SKIP 0.3.0"
pub fn update_skip_line(version: &str) -> String {
    format!("UPDATE SKIP {version}")
}

// ---- the words ----------------------------------------------------------------------------------

/// "Katana 0.3.0", or "Katana" alone for a version nobody gave.
fn product(version: &str) -> String {
    if version.is_empty() {
        String::from("Katana")
    } else {
        format!("Katana {version}")
    }
}

fn nearest_megabytes(bytes: u64) -> u64 {
    bytes / MEGABYTE + u64::from(bytes % MEGABYTE >= MEGABYTE / 2)
}

/// The megabytes that have arrived, never more than the whole is said to be.
fn downloaded_megabytes(view: &UpdateView) -> String {
    if view.download_bytes > 0 && view.downloaded_bytes >= view.download_bytes {
        return update_megabytes(view.download_bytes);
    }
    format!("{}", nearest_megabytes(view.downloaded_bytes))
}

/// `sentence` after `text`, a blank between them when both are there.
fn with(text: &str, sentence: &str) -> String {
    if text.is_empty() || sentence.is_empty() {
        return format!("{text}{sentence}");
    }
    format!("{text} {sentence}")
}

/// What is said of a release that is set aside because it did not start here: that it is not the
/// end of updates, and where the way back is. One start too slow for the health check sets a
/// sound release aside, so the person must be told how to have it offered again: Settings >
/// Updates has the button (settingsUpdateUnblock, which runs UPDATE BLOCKED CLEAR).
const SET_ASIDE_WORDS: &str =
    "A later release will be offered, and Settings > Updates offers this one again.";

/// Whether the apply this view tells of left its release set aside. The helper puts a release on
/// the state's blocked list when it was installed, did not prove that it starts and was put back -
/// a rollback for either of the health check's two reasons (katana_update's apply) - and for no
/// other ending: a swap that failed for want of disk space is offered again as it was.
fn was_set_aside(view: &UpdateView) -> bool {
    view.last_status == "rolled_back"
        && (view.reason == "health_exit" || view.reason == "health_timeout")
}

/// Whether the apply this view tells of was refused because an earlier update of the install is
/// still open, neither confirmed nor put back (no new update starts over one).
fn was_refused_over_an_earlier_update(view: &UpdateView) -> bool {
    view.last_status == "refused" && view.reason == "interrupted_update"
}

/// What is said after the reason of such a refusal. The helper names the way out in its own
/// `detail`, on a console nobody sees, and the result it leaves keeps the token alone - so the
/// dialog says it. The start that shows this is itself what settles most such updates: every
/// start puts back what a crash interrupted, and a window of the installed version confirms an
/// update that was never said to start (`update_start_may_confirm`). So the first thing to do is
/// to update again; where that is refused again - a window that may not confirm, an update that
/// does not start - nothing but the helper's recovery, run by hand with Katana closed, settles it.
fn unsettled_words(recover_command: &str) -> String {
    format!("Update again; if this is said again, close Katana and run: {recover_command}")
}

/// `line` split at ASCII white space, as a regular expression's `\s` splits it.
fn ascii_words(text: &str) -> impl Iterator<Item = &str> {
    text.split([' ', '\t', '\n', '\u{b}', '\u{c}', '\r'])
        .filter(|word| !word.is_empty())
}

/// `text` with every `word` in it, in any case, written `with`: QString::replace with
/// Qt::CaseInsensitive. The fold is one character for one, so the folded text is searched and the
/// text itself rewritten at the same places.
fn replace_ignoring_case(text: &str, word: &str, with: &str) -> String {
    let needle: Vec<char> = folded(word).chars().collect();
    if needle.is_empty() {
        return String::from(text);
    }
    let shown: Vec<char> = text.chars().collect();
    let searched: Vec<char> = folded(text).chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    while at < shown.len() {
        if searched.len() - at >= needle.len() && searched[at..at + needle.len()] == needle[..] {
            out.push_str(with);
            at += needle.len();
        } else {
            out.push(shown[at]);
            at += 1;
        }
    }
    out
}

/// What the verbs said back to `line`, as the command log may show it: every word of the line
/// that `update_logged_line` hides is hidden in `reply` too, IN ANY CASE - a refusal may quote the
/// line it refuses, and may have folded its case first. A reply to any other line comes back
/// unchanged.
pub fn update_logged_reply(reply: &str, line: &str) -> String {
    let words = update_token_line_words(line);
    let mut shown = String::from(reply);
    for word in words.iter().skip(2) {
        if !is_update_token_verb_word(word) {
            shown = replace_ignoring_case(&shown, word, "***");
        }
    }
    shown
}

/// A person's words for a reason token of the update engine, to stand after a colon and before a
/// full stop: "offline" is "this computer is offline, or the release server did not answer";
/// "http_503" is "the release server answered with error 503"; a token it does not know is given
/// back as it came, which is still better than nothing said.
pub fn update_reason_words(token: &str) -> String {
    if let Some((_, words)) = REASON_WORDS.iter().find(|(known, _)| *known == token) {
        return String::from(*words);
    }
    // Any other answer of the server's: the engine writes it http_<status>, three digits.
    if let Some(status) = token.strip_prefix("http_")
        && status.len() == 3
        && status.bytes().all(|digit| digit.is_ascii_digit())
    {
        return format!("the release server answered with error {status}");
    }
    String::from(token)
}

/// Bytes as whole megabytes of a million bytes, rounded to the nearest - "71" for 71 000 000 -
/// and never "0" for a download that is not empty: one byte is "1".
pub fn update_megabytes(bytes: u64) -> String {
    if bytes == 0 {
        return String::from("0");
    }
    format!("{}", nearest_megabytes(bytes).max(1))
}

/// One to nine ASCII digits, as a number.
fn count_of(digits: &str) -> Option<u64> {
    if digits.is_empty() || digits.len() > 9 || !digits.bytes().all(|digit| digit.is_ascii_digit())
    {
        return None;
    }
    digits.parse().ok()
}

/// The files a line of UPDATE DOWNLOAD's progress says are staged: the two numbers of a word
/// `files=<done>/<all>`, wherever it stands in the line. None for a line that has no such word,
/// which leaves the files unsaid.
pub fn update_files_of_progress_line(line: &str) -> Option<(u64, u64)> {
    ascii_words(line).find_map(|word| {
        let (done, all) = word.strip_prefix("files=")?.split_once('/')?;
        Some((count_of(done)?, count_of(all)?))
    })
}

/// The first of a view's two sentences, as the dialog shows it: the owner logs the same words
/// where a person is told outside the dialog.
pub fn update_state_text(view: &UpdateView) -> String {
    match view.state {
        UpdateDialogState::Idle => product(&view.installed_version),
        UpdateDialogState::Checking => String::from("Checking for updates..."),
        UpdateDialogState::Current => {
            format!("{} is up to date.", product(&view.installed_version))
        }
        UpdateDialogState::NotOffered => {
            if view.reason == "blocked_before" {
                format!(
                    "{} could not be installed here before, and is not offered again.",
                    product(&view.latest_version)
                )
            } else {
                format!(
                    "{} is available, and you chose to skip it.",
                    product(&view.latest_version)
                )
            }
        }
        UpdateDialogState::Available => String::from("Update Available"),
        UpdateDialogState::Downloading => {
            let mut text = format!(
                "Downloading {} of {} MB",
                downloaded_megabytes(view),
                update_megabytes(view.download_bytes)
            );
            if let Some(files) = view.downloaded_files
                && view.changed_files > 0
            {
                text.push_str(&format!(
                    " ({} of {} files)",
                    files.min(view.changed_files),
                    view.changed_files
                ));
            }
            text.push('.');
            text
        }
        UpdateDialogState::Ready => format!("{} is ready.", product(&view.latest_version)),
        UpdateDialogState::Blocked => {
            if view.holders.is_empty() {
                String::from("Close the other Katana programs that are open.")
            } else {
                format!("Close these first: {}.", view.holders)
            }
        }
        UpdateDialogState::RolledBack => {
            if view.last_status == "refused" {
                format!(
                    "{} could not be installed; nothing was changed.",
                    product(&view.latest_version)
                )
            } else if view.last_status == "rollback_incomplete" {
                format!(
                    "{} could not be installed, and {} could not be put back completely.",
                    product(&view.latest_version),
                    view.installed_version
                )
            } else {
                format!(
                    "{} could not be installed, so {} was restored.",
                    product(&view.latest_version),
                    view.installed_version
                )
            }
        }
        UpdateDialogState::CheckFailed => {
            if view.reason == "http_404" {
                String::from("No update information found.")
            } else if view.reason.is_empty() {
                String::from("Could not check for updates.")
            } else {
                format!(
                    "Could not check for updates: {}.",
                    update_reason_words(&view.reason)
                )
            }
        }
        UpdateDialogState::Unsupported => {
            if view.reason.is_empty() {
                String::from("Updates are not available for this copy of Katana.")
            } else {
                format!(
                    "Updates are not available for this copy of Katana: {}.",
                    update_reason_words(&view.reason)
                )
            }
        }
    }
}

/// The second of a view's sentences; empty where the state has none.
pub fn update_detail_text(view: &UpdateView) -> String {
    match view.state {
        UpdateDialogState::Idle => String::from("Not checked for updates yet."),
        UpdateDialogState::Checking | UpdateDialogState::Downloading => {
            String::from("You can keep working.")
        }
        UpdateDialogState::Current => {
            if view.last_checked.is_empty() {
                String::new()
            } else {
                format!("Last checked {}.", view.last_checked)
            }
        }
        UpdateDialogState::NotOffered => {
            if view.reason == "blocked_before" {
                String::from(SET_ASIDE_WORDS)
            } else {
                String::from("Settings > Updates offers it again.")
            }
        }
        UpdateDialogState::Available => {
            let mut text = format!(
                "{} (you have {}).",
                product(&view.latest_version),
                view.installed_version
            );
            let second = if view.notify_only {
                String::from(
                    "This copy cannot update itself: the release notes say where to download it.",
                )
            } else {
                format!(
                    "{} of {} files changed, {} MB to download.",
                    view.changed_files,
                    view.total_files,
                    update_megabytes(view.download_bytes)
                )
            };
            text = with(&text, &second);
            if view.information_is_old {
                text = with(
                    &text,
                    "The update information is old: a newer release may exist.",
                );
            }
            text
        }
        UpdateDialogState::Ready => {
            if view.waiting_for_jobs {
                String::from(
                    "Katana will close when the running job has finished, then update and reopen.",
                )
            } else if view.waiting_for_dialog {
                String::from(
                    "Katana will close when the dialog that is open has been closed, then update \
                     and reopen.",
                )
            } else {
                String::from("Katana will close, update and reopen in a few seconds.")
            }
        }
        UpdateDialogState::Blocked => String::from("Then press Retry."),
        UpdateDialogState::RolledBack => {
            let reason = if view.reason.is_empty() {
                String::new()
            } else {
                format!("Reason: {}.", update_reason_words(&view.reason))
            };
            if view.last_status == "rollback_incomplete" && !view.recover_command.is_empty() {
                return with(
                    &reason,
                    &format!("Close Katana and run: {}", view.recover_command),
                );
            }
            if was_refused_over_an_earlier_update(view) && !view.recover_command.is_empty() {
                return with(&reason, &unsettled_words(&view.recover_command));
            }
            // The release that was put back is not offered again by itself.
            if was_set_aside(view) {
                with(&reason, SET_ASIDE_WORDS)
            } else {
                reason
            }
        }
        UpdateDialogState::CheckFailed => {
            if view.reason == "http_404" {
                String::from(
                    "If the releases are private, add an access token in Settings > Updates.",
                )
            } else {
                String::new()
            }
        }
        UpdateDialogState::Unsupported => String::new(),
    }
}

// ---- THE DECISIONS ------------------------------------------------------------------------------
//
// What the dialog's owner decides from the update engine's records: which view an answer is, what
// a line that was run at once does to what is shown, what is offered when the window starts, when
// the window checks by itself, what the log may show, what Settings > Updates is told of the state
// file, which helper copies a start may remove, whether a start confirms an update that was never
// said to start - and, the program's own, whether an update that could not be started is said in a
// message box. They are here - beside the dialog - because their owner in the program
// (UpdateWorkbench) needs the whole window. Each is a function of its arguments: the owner gathers
// the facts (the disk, the clock, the environment) and decides nothing of this itself.

/// "0.3" and "0.3.0" are one release (katana_update's version); text that is no version is the
/// same only as itself.
pub fn update_same_version(a: &str, b: &str) -> bool {
    match (parse_version(a), parse_version(b)) {
        (Ok(first), Ok(second)) => first == second,
        _ => a == b,
    }
}

/// The value of `key` in a reply's records (`key=value`, blanks or line breaks between them) - of
/// the LAST record that has it, since a download's reply ends with the record that says what is
/// staged. Quotes around a value are taken off. Empty when no record of the reply is `key=...`.
pub fn update_reply_value(reply: &str, key: &str) -> String {
    // A record is a word: the two keys read here, reason and staged, have values without a blank
    // in them.
    let prefix = format!("{key}=");
    let mut value = "";
    for word in ascii_words(reply) {
        if let Some(rest) = word.strip_prefix(prefix.as_str()) {
            value = rest;
        }
    }
    if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
        value = &value[1..value.len() - 1];
    }
    String::from(value)
}

/// The version UPDATE DOWNLOAD CONFIRM's reply says is staged: the value of its last `staged=`,
/// and empty for none at all and for `staged=none`.
pub fn update_staged_of_reply(reply: &str) -> String {
    let staged = update_reply_value(reply, "staged");
    if same_ignoring_case(&staged, "none") {
        String::new()
    } else {
        staged
    }
}

/// The status bar's button for a view: "Update 0.3.0" while there is a release to act on -
/// offered, on its way, downloaded, or waiting for other programs to close - and EMPTY when the
/// button is hidden: in every other state, and for a view that names no version.
pub fn update_status_button_text(view: &UpdateView) -> String {
    if view.latest_version.is_empty() {
        return String::new();
    }
    match view.state {
        UpdateDialogState::Available
        | UpdateDialogState::Downloading
        | UpdateDialogState::Ready
        | UpdateDialogState::Blocked => format!("Update {}", view.latest_version),
        UpdateDialogState::Idle
        | UpdateDialogState::Checking
        | UpdateDialogState::Current
        | UpdateDialogState::NotOffered
        | UpdateDialogState::RolledBack
        | UpdateDialogState::CheckFailed
        | UpdateDialogState::Unsupported => String::new(),
    }
}

/// What the owner knows beside a check's answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateOfferFacts {
    /// This copy's, for an answer that names none.
    pub installed_version: String,
    /// The release downloaded and waiting; empty when none is.
    pub staged_version: String,
    /// The time as shown, "14:02".
    pub checked_at: String,
    /// UTC seconds, for "the update information is old".
    pub now_epoch: i64,
    /// False where a copy cannot replace its own files.
    pub applies_here: bool,
}

impl Default for UpdateOfferFacts {
    fn default() -> Self {
        Self {
            installed_version: String::new(),
            staged_version: String::new(),
            checked_at: String::new(),
            now_epoch: 0,
            applies_here: true,
        }
    }
}

/// What UPDATE CHECK's answer is shown as, by its status (katana_update's check):
///
/// ```text
/// available    Available, with the files and the bytes - or Ready, when the version offered is
///              the one already downloaded. notify_only for a notify-only manifest and where the
///              copy cannot apply; information_is_old past the manifest's `expires`
/// current      Current with the time; NotOffered "skipped" when the reason says the person
///              skipped the release the feed has
/// blocked      NotOffered "blocked_before"
/// unreachable  CheckFailed with the reason
/// unsupported  Unsupported with the reason
/// ```
pub fn update_view_of_offer(offer: &UpdateOffer, facts: &UpdateOfferFacts) -> UpdateView {
    let check = &offer.check;
    let mut view = UpdateView {
        installed_version: if check.installed_version.is_empty() {
            facts.installed_version.clone()
        } else {
            check.installed_version.clone()
        },
        latest_version: check.latest_version.clone(),
        notes_url: check.notes_url.clone(),
        reason: check.reason.clone(),
        ..UpdateView::default()
    };
    match check.status {
        CheckStatus::Available => {
            // The release that is already downloaded is ready, not to be downloaded again.
            if !facts.staged_version.is_empty()
                && update_same_version(&facts.staged_version, &view.latest_version)
            {
                view.state = UpdateDialogState::Ready;
            } else {
                view.state = UpdateDialogState::Available;
                view.changed_files = offer.changed_files;
                view.total_files = offer.total_files;
                view.download_bytes = offer.download_bytes;
                // `expires` is advisory: said, and the update still offered.
                view.information_is_old = check
                    .manifest
                    .as_ref()
                    .is_some_and(|manifest| is_expired(manifest, facts.now_epoch));
                view.notify_only = !facts.applies_here
                    || check
                        .manifest
                        .as_ref()
                        .is_some_and(|manifest| manifest.notify_only);
            }
        }
        CheckStatus::Current => {
            if check.reason == "skipped" {
                view.state = UpdateDialogState::NotOffered;
            } else {
                view.state = UpdateDialogState::Current;
                view.last_checked = facts.checked_at.clone();
            }
        }
        CheckStatus::Blocked => {
            view.state = UpdateDialogState::NotOffered;
            view.reason = String::from("blocked_before");
        }
        CheckStatus::Unreachable => view.state = UpdateDialogState::CheckFailed,
        CheckStatus::Unsupported => view.state = UpdateDialogState::Unsupported,
    }
    view
}

/// What the AUTOMATIC check does with its answer, which nobody asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UpdateQuietAnswer {
    /// It learnt nothing, or nothing that changes what is shown: no word anywhere.
    Nothing,
    /// Nothing to tell, something to show: the view changes and the log does not.
    Show,
    /// A release to offer: logged as a typed check is, and shown.
    Announce,
}

/// `shown` is what the dialog shows now, `answer` the view of the check's answer
/// (`update_view_of_offer`, after katana_update's apply_update_state):
///
/// - a release to offer (Available, Ready) is announced - unless that very version is already
///   shown in that state, which is no news;
/// - a check that failed, or that this copy cannot make, says nothing;
/// - "up to date" and "not offered" are shown without a word - which is what takes back a release
///   the view still offers once the feed no longer has it, or once it was skipped or rolled back
///   here - except over a view that must stay until it is read (RolledBack) or that a job owns
///   (Checking, Downloading).
pub fn update_quiet_answer(shown: &UpdateView, answer: &UpdateView) -> UpdateQuietAnswer {
    // What must stay until it is read, and what a running job owns.
    if matches!(
        shown.state,
        UpdateDialogState::RolledBack | UpdateDialogState::Checking | UpdateDialogState::Downloading
    ) {
        return UpdateQuietAnswer::Nothing;
    }
    match answer.state {
        UpdateDialogState::Available | UpdateDialogState::Ready => {
            // A release the view already offers - to download, downloaded, or waiting for other
            // programs to close - is no news.
            if !update_status_button_text(shown).is_empty()
                && update_same_version(&shown.latest_version, &answer.latest_version)
            {
                UpdateQuietAnswer::Nothing
            } else {
                UpdateQuietAnswer::Announce
            }
        }
        UpdateDialogState::Current | UpdateDialogState::NotOffered => UpdateQuietAnswer::Show,
        UpdateDialogState::Idle
        | UpdateDialogState::Checking
        | UpdateDialogState::Downloading
        | UpdateDialogState::Blocked
        | UpdateDialogState::RolledBack
        | UpdateDialogState::CheckFailed
        | UpdateDialogState::Unsupported => UpdateQuietAnswer::Nothing,
    }
}

/// What a line the window ran at once, and the verbs carried out, does to what the dialog shows -
/// beside what the line itself wrote to the state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UpdateLineEffect {
    /// The view stays as it is.
    Nothing,
    /// The release the view says is not offered is set aside no longer: the view goes back to
    /// knowing nothing (Idle), and the next check offers it.
    Forgotten,
    /// The release the view offers was skipped: NotOffered "skipped" with that version, which is
    /// no longer the release a download typed next is of.
    Skipped,
}

/// `shown` is what the dialog shows, `line` the line as it was typed or built, and `reply` what
/// the verbs answered it - the text of a reply; a line that was refused changed nothing and is not
/// asked about. A reply that gives a `reason=` was not carried out either (`skipped=none
/// reason=no_state` and `blocked=none reason=no_state`: a session with no update folder keeps
/// nothing) and is Nothing. Otherwise:
///
/// ```text
/// UPDATE SKIP NONE       over NotOffered "skipped": Forgotten
/// UPDATE SKIP <version>  over Available of that version ("0.3" and "0.3.0" are one): Skipped
/// UPDATE BLOCKED CLEAR   over NotOffered "blocked_before": Forgotten - the dialog said the
///                        release "is not offered again", which is so no longer
/// ```
///
/// Each list is its own: SKIP NONE leaves a release that was put back as it is shown, and BLOCKED
/// CLEAR a skipped one. Every other line and every other view is Nothing. The words of a line are
/// taken in any case, with any blanks between them.
pub fn update_effect_of_line(shown: &UpdateView, line: &str, reply: &str) -> UpdateLineEffect {
    // A reply that gives a reason is one that was not carried out.
    if !update_reply_value(reply, "reason").is_empty() {
        return UpdateLineEffect::Nothing;
    }
    // The lines read here have three words each: UPDATE SKIP NONE, UPDATE SKIP <version>, UPDATE
    // BLOCKED CLEAR.
    let simple = simplified(line);
    let words: Vec<&str> = simple.split(' ').filter(|word| !word.is_empty()).collect();
    if words.len() != 3 || !same_ignoring_case(words[0], "UPDATE") {
        return UpdateLineEffect::Nothing;
    }
    let not_offered = shown.state == UpdateDialogState::NotOffered;
    if same_ignoring_case(words[1], "SKIP") {
        if same_ignoring_case(words[2], "NONE") {
            return if not_offered && shown.reason == "skipped" {
                UpdateLineEffect::Forgotten
            } else {
                UpdateLineEffect::Nothing
            };
        }
        return if shown.state == UpdateDialogState::Available
            && update_same_version(&shown.latest_version, words[2])
        {
            UpdateLineEffect::Skipped
        } else {
            UpdateLineEffect::Nothing
        };
    }
    if same_ignoring_case(words[1], "BLOCKED") && same_ignoring_case(words[2], "CLEAR") {
        return if not_offered && shown.reason == "blocked_before" {
            UpdateLineEffect::Forgotten
        } else {
            UpdateLineEffect::Nothing
        };
    }
    UpdateLineEffect::Nothing
}

/// The downloaded release that is still one to install after a check answered `answer`: none once
/// the feed has nothing above this copy (Current: the release was withdrawn), and none once the
/// answer says that very version was skipped or rolled back here (NotOffered); otherwise `staged`
/// as it was - a check that failed un-knows nothing.
pub fn update_staged_after(staged: &str, answer: &UpdateView) -> String {
    if answer.state == UpdateDialogState::Current {
        return String::new();
    }
    if answer.state == UpdateDialogState::NotOffered
        && update_same_version(staged, &answer.latest_version)
    {
        return String::new();
    }
    String::from(staged)
}

/// The release to offer as Ready when the window starts, from what UPDATE STATUS says is
/// downloaded: its version, or empty when none is to be offered -
///
/// - nothing is staged, or what is staged is no version above `installed` (the stage a finished
///   update left behind);
/// - the person skipped that version (status.state.skipped_version);
/// - a release that was ROLLED BACK HERE still has its stage. The apply keeps the stage of a
///   release it rolled back and STATUS names what is staged by version alone, so the complete
///   stage of a blocked sequence in the update folder - which `stage_is_there(sequence)` says -
///   means what is staged is, or may be, the release that does not start here. Offered as ready,
///   it would be installed again.
pub fn update_staged_to_offer(
    status: &UpdateStatus,
    installed: &Version,
    stage_is_there: Option<&dyn Fn(u64) -> bool>,
) -> String {
    if status.staged_version.is_empty() {
        return String::new();
    }
    match parse_version(&status.staged_version) {
        Ok(staged) if staged > *installed => {}
        _ => return String::new(),
    }
    let version = status.staged_version.clone();
    if !status.state.skipped_version.is_empty()
        && update_same_version(&version, &status.state.skipped_version)
    {
        return String::new();
    }
    if let Some(stage_is_there) = stage_is_there
        && status
            .state
            .blocked_sequences
            .iter()
            .any(|sequence| stage_is_there(*sequence))
    {
        return String::new();
    }
    version
}

/// How the last apply ended (U/last.json, katana_update's LastResult) as the dialog shows it at
/// the next start. None for "committed": a success is one line in the log, not a dialog.
/// "blocked" is the Blocked state with the programs to close; every other status is RolledBack
/// with that status and the reason. Two of those carry the command that puts the install right by
/// hand, which names `install_root` (the folder as the platform writes it): "rollback_incomplete",
/// and "refused" for the reason interrupted_update - an earlier update of the install is neither
/// confirmed nor put back, and no new one starts over it. The helper names the command for that
/// refusal in its own `detail` and nowhere else, and the result it leaves for the next start keeps
/// the token alone, so the view is where a person is told it.
pub fn update_view_of_last_result(
    last: &LastResult,
    installed_version: &str,
    install_root: &str,
) -> Option<UpdateView> {
    if last.status == "committed" {
        return None;
    }
    let mut view = UpdateView {
        installed_version: String::from(installed_version),
        latest_version: last.version.clone(),
        reason: last.reason.clone(),
        ..UpdateView::default()
    };
    if last.status == "blocked" {
        // Nothing was touched and the release is still downloaded: Retry applies it again once
        // the programs named are closed.
        view.state = UpdateDialogState::Blocked;
        view.holders = last.holder.clone();
        return Some(view);
    }
    view.state = UpdateDialogState::RolledBack;
    view.last_status = last.status.clone();
    // The two endings the helper's recovery settles: a tree that could not be put back
    // completely, and a refusal over an earlier update that is still open (update_detail_text says
    // what to do with the command for each).
    if last.status == "rollback_incomplete" || was_refused_over_an_earlier_update(&view) {
        view.recover_command = format!("katana_updater --recover \"{install_root}\"");
    }
    Some(view)
}

/// What the window's automatic check depends on beside the state file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UpdateAutoCheckFacts {
    /// A headless run checks nothing by itself.
    pub headless: bool,
    /// KATANA_UPDATE_DISABLE is set.
    pub disabled: bool,
    /// An update job is running already.
    pub job_running: bool,
    pub kind: InstallKind,
    /// The session has a per-user update folder.
    pub keeps_state: bool,
}

/// Whether the window checks by itself now. Only when ALL of these hold: the run is not headless;
/// KATANA_UPDATE_DISABLE is not set; no update job runs; the copy is one packaging installed (a
/// development tree stays offline); the session keeps per-user state (with none there is no
/// saying when the last check was); and katana_update's check_is_due says so for `state` - its
/// automatic flag on, and the last check 24 hours and `jitter` seconds ago, or never, or in the
/// future.
pub fn update_automatic_check_is_due(
    facts: &UpdateAutoCheckFacts,
    state: &UpdateState,
    now_epoch: i64,
    jitter: i64,
) -> bool {
    if facts.headless
        || facts.disabled
        || facts.job_running
        || !facts.keeps_state
        || facts.kind == InstallKind::Development
    {
        return false;
    }
    check_is_due(state, now_epoch, jitter)
}

/// Whether `view` tells of a release that is set aside because it did not start here - the two
/// views whose second sentence ends "Settings > Updates offers this one again.": a check's
/// NotOffered "blocked_before", and the last apply's ending, RolledBack, when the release was put
/// back for not starting (rolled_back for health_exit or health_timeout, the endings after which
/// the helper blocks it).
pub fn update_tells_of_set_aside(view: &UpdateView) -> bool {
    match view.state {
        UpdateDialogState::NotOffered => view.reason == "blocked_before",
        UpdateDialogState::RolledBack => was_set_aside(view),
        UpdateDialogState::Idle
        | UpdateDialogState::Checking
        | UpdateDialogState::Current
        | UpdateDialogState::Available
        | UpdateDialogState::Downloading
        | UpdateDialogState::Ready
        | UpdateDialogState::Blocked
        | UpdateDialogState::CheckFailed
        | UpdateDialogState::Unsupported => false,
    }
}

/// What the window knows beside the state file when Settings > Updates asks whether a release is
/// set aside (`update_put_back_of_state`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UpdatePutBackFacts {
    /// The install has a record of the build an update put there (installed.json: katana_update's
    /// Install::has_installed_record), and that build's sequence (Install::sequence). Without a
    /// record the sequence is a placeholder, and is not read.
    pub has_installed_record: bool,
    pub installed_sequence: u64,
    /// The update dialog is telling of a release that is set aside now
    /// (`update_tells_of_set_aside` of the view it shows).
    pub dialog_tells_of_one: bool,
}

/// What Settings > Updates is told of the state file (U/state.json), from a read of it
/// (katana_update's read_state): the version UPDATE SKIP set aside, empty when none is. None FOR A
/// FILE THAT DID NOT READ. Nobody can say then, and "none" would be a guess that switches the
/// page's button off: with None the page says nothing of it and leaves the button on, so the line
/// is run and the verbs say why they refuse it (a state file that does not read is not written
/// over). A file that is not there reads as the defaults - none skipped, none put back.
pub fn update_skipped_of_state(state: &Result<UpdateState>) -> Option<String> {
    // Nobody can say: never "none".
    state.as_ref().ok().map(|state| state.skipped_version.clone())
}

/// Whether a release is set aside because it was installed here, did not start and was put back;
/// None for a state file that did not read, as `update_skipped_of_state`.
///
/// WHICH BLOCKED BUILDS THE PAGE IS TOLD OF. The state's list of blocked releases is only ever
/// added to, until UPDATE BLOCKED CLEAR empties it, and it is one list for every copy of Katana a
/// person runs (U is per user). So it goes on holding builds that have stopped mattering to this
/// install: 0.3.0 was put back, then 0.4.0 was installed and started, and no feed offers 0.3.0
/// over it. Told of that one, the page said for good that a version "is not offered", with a
/// button that changed nothing a check could show. The list keeps each build by its sequence - the
/// time it was made - and no version, so a release is said to be set aside when the list holds
///
/// - a build made AFTER the one installed here: a sequence above `facts.installed_sequence` - or
///   any build at all where the install has no record of its own (a fresh install: nothing says
///   when its build was made);
/// - or any build, while the update dialog is telling of a release that is set aside
///   (`facts.dialog_tells_of_one`). A build made before the installed one can still be of a higher
///   version - a release line that went back to an older version, which was installed, and
///   forward again - and only a check that met it knows. The dialog's sentence then sends the
///   person to the page's button, which must be on.
///
/// An empty list is "none" whatever the dialog shows: the list is what the button empties.
pub fn update_put_back_of_state(
    state: &Result<UpdateState>,
    facts: &UpdatePutBackFacts,
) -> Option<bool> {
    // Nobody can say: never "none".
    let state = state.as_ref().ok()?;
    let blocked = &state.blocked_sequences;
    if blocked.is_empty() {
        return Some(false);
    }
    // Nothing says when this install's build was made, or the dialog is telling of one of them
    // now: any build on the list counts.
    if !facts.has_installed_record || facts.dialog_tells_of_one {
        return Some(true);
    }
    // Otherwise only a build made after the one installed here: an earlier one is, as far as the
    // list can say, a release this install has passed.
    Some(
        blocked
            .iter()
            .any(|sequence| *sequence > facts.installed_sequence),
    )
}

/// What a window knows when it asks whether its own start settles an update that was installed and
/// never said to start (`update_start_may_confirm`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UpdateConfirmFacts {
    /// False where a copy cannot replace its own files.
    pub applies_here: bool,
    /// A headless run writes no per-user state.
    pub headless: bool,
    /// The session has a per-user update folder.
    pub keeps_state: bool,
    pub kind: InstallKind,
    /// The install has a record of an update that committed (installed.json):
    /// Install::has_installed_record.
    pub has_installed_record: bool,
    /// The install's version as katana_update's read_install gives it (Install::version). That is
    /// the HIGHER of the build the helper recorded (installed.json) and the build the package
    /// names of itself (install.json): the hardening pass's ruling H3, 2026-10-14, as it was
    /// settled - the helper's record is never taken to say less than the package does
    /// (docs/update.md, "The installed version"). So in a copy that packaging and updates made,
    /// the reader's version differs from `program` only when the helper's record names a version
    /// ABOVE the program's.
    pub installed: Version,
    /// The version this program was built as (katana_update's program_version).
    pub program: Version,
}

/// Whether this window, having reached its first idle turn, is the proof an update's health run
/// was to give - so that it may settle an update whose helper died while it waited for that run
/// (katana_update's confirm_started). Only when ALL of these hold: the copy can replace its own
/// files; the run is not headless (the confirmation writes per-user state); the session has a
/// per-user update folder; the copy is one packaging installed; and the install has a record of
/// its own and its version (`installed`) is this program's own.
///
/// WHAT THE LAST CONDITION REFUSES, with the facts the reader gives, is a helper's record that
/// names a version ABOVE the program's: a window of the OLD files that was started while the
/// files were being swapped. That one runs old code over the new tree, its record names a version
/// it is not, and its start proves nothing about the release. It does NOT refuse a helper's record
/// BELOW the program's - a newer copy unpacked by hand over one that had updated itself - because
/// that copy brought its own install.json, and the reader has raised the version to the
/// package's, which is the program's: such a window confirms. The function itself asks for the two
/// to be equal and relies on no reader: handed a version below the program's it refuses.
pub fn update_start_may_confirm(facts: &UpdateConfirmFacts) -> bool {
    if !facts.applies_here
        || facts.headless
        || !facts.keeps_state
        || facts.kind == InstallKind::Development
    {
        return false;
    }
    // As versions: "0.3" and "0.3.0" are one release. Equal, and not "not above": what `installed`
    // can be is the reader's rule, and this one stands without it.
    facts.has_installed_record && facts.installed == facts.program
}

/// Whether an update that could not be started - the window it closed is gone, the helper did not
/// start, and nothing will happen - is said in a message box as well as on stderr, where the
/// program says it always. `step_kinds` is the kind of each step the run was given on its command
/// line, as the program keeps them: "--command" (an --enter is kept as one), "--run-line",
/// "--dialog", "--press" and the rest. Only a window nobody drove gets the box. A run that was
/// given a line to run (--command, --run-line) is a window all the same - it is not headless - and
/// may have nobody at it to close a box, on which it would wait for ever.
pub fn update_failure_is_boxed<S: AsRef<str>>(step_kinds: &[S]) -> bool {
    // The two kinds of step that run a line in a window that goes on to its event loop.
    !step_kinds
        .iter()
        .any(|kind| kind.as_ref() == "--command" || kind.as_ref() == "--run-line")
}

/// The transactions whose helper copy U/run-<tx> the window's start lists for removal
/// (katana_update's state, cleanup.list). A swap that ends lists its own copy. One that a crash
/// ended, or whose rollback failed, is put right later by `katana_updater --recover`, which writes
/// nothing per user, so nothing in U names its copy. The INSTALL names it: every run of --recover
/// that found something appends a line
///
/// ```text
/// <UTC time> recover found=<tx,...> result=ok|failed left=<tx,...>
/// ```
///
/// to R/.update/recovery.log. `recovery_log` is that file's bytes, and the answer is each id of a
/// `found=` list, once, in the order first named, for which `is_over(id)` holds - the install has
/// no folder R/.update/<id> any more. A transaction that a recovery found and whose folder is
/// gone is over, rolled back or healthy, and nothing starts its helper again. One whose folder is
/// still there keeps its copy: a rollback that failed runs from it at the next try. `is_over` is
/// asked once for an id however often it is named, and with none given nothing is known to be
/// over.
///
/// WHY THE INSTALL'S LOG AND NOT THE NAMES IN U. U is one folder for every copy of Katana a person
/// runs, and a `run-<tx>` with no transaction folder in THIS install is also what UPDATE APPLY
/// CONFIRM has just made, here or in another install, for a helper that has not started yet. Only
/// an id that this install's own recovery named is known to be this install's, and begun.
///
/// THE LOG IS NOT TRUSTED: a process of the same user can write R/.update. A line counts only when
/// it is whole - ended by its line feed, so a torn last write names nothing - and has the form
/// above: its second word `recover`, its third `found=...`, its fourth `result=...`. The same
/// file quotes journal records a rollback refused (`<tx> journal record refused: ...`), whose text
/// is anybody's and therefore names nothing. And an id counts only when katana_update's
/// is_valid_transaction_id says it is one, so it names no place but U/run-<id>.
pub fn update_recovered_transactions(
    recovery_log: &[u8],
    is_over: Option<&mut dyn FnMut(&str) -> bool>,
) -> Vec<String> {
    // A line of `katana_updater --recover` after its first word, the time, and the word that
    // follows its list of ids.
    const FOUND: &[u8] = b"recover found=";
    const RESULT: &[u8] = b"result=";
    let mut over = Vec::new();
    let Some(is_over) = is_over else {
        return over;
    };
    // Every id met so far: a recovery that failed and the one that then put the install right both
    // name the transaction, and it is asked about once.
    let mut met: Vec<String> = Vec::new();
    // Line by line, each ended by its line feed: what is left after the last one is a write that
    // was cut short, and is not read.
    let mut rest = recovery_log;
    while let Some(line_end) = rest.iter().position(|byte| *byte == b'\n') {
        let line = &rest[..line_end];
        rest = &rest[line_end + 1..];
        let Some(time_end) = line.iter().position(|byte| *byte == b' ') else {
            continue;
        };
        let Some(line) = line[time_end + 1..].strip_prefix(FOUND) else {
            continue;
        };
        let Some(list_end) = line.iter().position(|byte| *byte == b' ') else {
            continue;
        };
        if !line[list_end + 1..].starts_with(RESULT) {
            continue;
        }
        for id in line[..list_end].split(|byte| *byte == b',') {
            if !is_valid_transaction_id(id) {
                continue;
            }
            // A valid id is hexadecimal digits, so it is text.
            let Ok(id) = std::str::from_utf8(id) else {
                continue;
            };
            if met.iter().any(|known| known == id) {
                continue;
            }
            if is_over(id) {
                over.push(String::from(id));
            }
            met.push(String::from(id));
        }
    }
    over
}

// ---- the dialog ---------------------------------------------------------------------------------

/// The dialog's nine buttons, in the order the states list them: what acts first, Later last.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UpdateButton {
    CheckNow,
    Retry,
    Install,
    DownloadOnly,
    Skip,
    Notes,
    Restart,
    Cancel,
    Later,
}

/// The buttons in the order of their row.
pub const UPDATE_BUTTONS: [UpdateButton; 9] = [
    UpdateButton::CheckNow,
    UpdateButton::Retry,
    UpdateButton::Install,
    UpdateButton::DownloadOnly,
    UpdateButton::Skip,
    UpdateButton::Notes,
    UpdateButton::Restart,
    UpdateButton::Cancel,
    UpdateButton::Later,
];

impl UpdateButton {
    /// Whether it is the button that does what the state offers (check, retry, update, restart):
    /// the dialog's primary, drawn in the accent.
    pub const fn acts(self) -> bool {
        matches!(
            self,
            Self::CheckNow | Self::Retry | Self::Install | Self::Restart
        )
    }

    /// The C++ object name.
    pub const fn name(self) -> &'static str {
        match self {
            Self::CheckNow => "updateCheckNow",
            Self::Retry => "updateRetry",
            Self::Install => "updateInstall",
            Self::DownloadOnly => "updateDownloadOnly",
            Self::Skip => "updateSkip",
            Self::Notes => "updateNotes",
            Self::Restart => "updateRestart",
            Self::Cancel => "updateCancel",
            Self::Later => "updateLater",
        }
    }

    /// The words on it.
    pub const fn text(self) -> &'static str {
        match self {
            Self::CheckNow => "Check now",
            Self::Retry => "Retry",
            Self::Install => "Update",
            Self::DownloadOnly => "Download only",
            Self::Skip => "Skip this version",
            Self::Notes => "Release notes",
            Self::Restart => "Restart and update",
            Self::Cancel => "Cancel",
            Self::Later => "Later",
        }
    }

    /// Its tooltip: what it does, and by which line when it runs one.
    pub const fn tip(self) -> &'static str {
        match self {
            Self::CheckNow => "Ask the release server whether a newer Katana exists (UPDATE CHECK)",
            Self::Retry => "Try again",
            Self::Install => {
                "Download what changed, then close Katana, update it and open it again"
            }
            Self::DownloadOnly => {
                "Download what changed now and install it later (UPDATE DOWNLOAD CONFIRM)"
            }
            Self::Skip => "Do not offer this version again; the next one is offered (UPDATE SKIP)",
            Self::Notes => "Open the release notes in your browser",
            Self::Restart => {
                "Close Katana, install the downloaded version and open Katana again (UPDATE APPLY \
                 CONFIRM)"
            }
            Self::Cancel => {
                "Stop the check or the download; what has been downloaded is kept for the next try"
            }
            Self::Later => "Close this window and change nothing",
        }
    }

    /// The button of that object name.
    pub fn of_name(name: &str) -> Option<Self> {
        UPDATE_BUTTONS
            .into_iter()
            .find(|button| button.name() == name)
    }
}

/// Where an owner posts the view its dialog shows next (see "THE RUST SHAPE" in the module
/// comment). Cloning it shares it; a view posted over one not yet taken takes its place.
#[derive(Clone, Debug, Default)]
pub struct UpdateViewPost {
    posted: Rc<RefCell<Option<UpdateView>>>,
}

impl UpdateViewPost {
    /// `view` for the dialog's next look.
    pub fn post(&self, view: UpdateView) {
        *self.posted.borrow_mut() = Some(view);
    }

    fn take(&self) -> Option<UpdateView> {
        self.posted.borrow_mut().take()
    }
}

/// Help > Check for Updates (see the module comment).
pub struct UpdateDialog {
    frame: DialogFrame,
    visible: bool,
    context: UpdateDialogContext,
    view: UpdateView,
    /// What the executor refused at the last press; cleared by the next view.
    refusal: String,
    posted: UpdateViewPost,
}

/// The dialog and the window, lent together to the dialog frame's body and footer.
struct Lent<'a, 'h> {
    dialog: &'a mut UpdateDialog,
    host: &'a mut (dyn Host + 'h),
}

impl UpdateDialog {
    pub fn new(context: UpdateDialogContext) -> Self {
        Self {
            frame: DialogFrame::new("updateDialog", "Check for Updates"),
            visible: false,
            context,
            view: UpdateView::default(),
            refusal: String::new(),
            posted: UpdateViewPost::default(),
        }
    }

    /// The C++ object name.
    pub fn object_name(&self) -> &str {
        self.frame.name()
    }

    pub fn window_title(&self) -> &str {
        self.frame.title()
    }

    pub fn is_modal(&self) -> bool {
        self.frame.is_modal()
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// QDialog::show. Whether the dialog is on screen is the owner's to say.
    pub fn show(&mut self) {
        self.visible = true;
    }

    /// QDialog::close: hidden, and kept with what it showed.
    pub fn close(&mut self) {
        self.visible = false;
    }

    /// Where the owner posts the next view.
    pub fn views(&self) -> UpdateViewPost {
        self.posted.clone()
    }

    /// The dialog's look at what its owner posted: the posted view shown, as `set_view` shows one.
    fn look(&mut self) {
        if let Some(view) = self.posted.take() {
            self.view = view;
            self.refusal.clear();
        }
    }

    /// Shows `view`: its sentences, its buttons, its progress. The dialog keeps a copy. A view
    /// posted and not yet taken is dropped: this one is the later.
    pub fn set_view(&mut self, view: UpdateView) {
        let _ = self.posted.take();
        self.view = view;
        self.refusal.clear();
    }

    pub fn view(&mut self) -> &UpdateView {
        self.look();
        &self.view
    }

    /// Drops the callbacks: what an owner that goes before the dialog does.
    pub fn detach(&mut self) {
        self.look();
        self.context = UpdateDialogContext::default();
    }

    /// updateStateText.
    pub fn state_text(&mut self) -> String {
        self.look();
        update_state_text(&self.view)
    }

    /// updateDetailText.
    pub fn detail_text(&mut self) -> String {
        self.look();
        update_detail_text(&self.view)
    }

    /// updateProblemText: the executor's refusal at the last press, else what the owner could not
    /// do; shown in the error colour, hidden while empty.
    pub fn problem_text(&mut self) -> String {
        self.look();
        self.problem().to_owned()
    }

    fn problem(&self) -> &str {
        if self.refusal.is_empty() {
            &self.view.problem
        } else {
            &self.refusal
        }
    }

    /// The colour updateProblemText is written in: the theme's error colour.
    pub fn problem_colour(&self) -> crate::canvas::Color {
        theme::error()
    }

    /// Whether updateProgress is shown: while checking and while downloading.
    pub fn progress_shown(&mut self) -> bool {
        self.look();
        matches!(
            self.view.state,
            UpdateDialogState::Downloading | UpdateDialogState::Checking
        )
    }

    /// QProgressBar's minimum and maximum: 0 to 1000 for a download whose size is known, and 0 to
    /// 0 - busy - for a check, which cannot say how far it is, and a download of no known size.
    pub fn progress_range(&mut self) -> (i32, i32) {
        self.look();
        if self.measures_progress() {
            (0, PROGRESS_STEPS)
        } else {
            (0, 0)
        }
    }

    /// QProgressBar's value: the thousandths of the download that have arrived, to the nearest;
    /// None while the bar is busy.
    pub fn progress_value(&mut self) -> Option<i32> {
        self.look();
        self.progress_steps()
    }

    fn measures_progress(&self) -> bool {
        self.view.state == UpdateDialogState::Downloading && self.view.download_bytes > 0
    }

    fn progress_steps(&self) -> Option<i32> {
        if !self.measures_progress() {
            return None;
        }
        let whole = u128::from(self.view.download_bytes);
        let done = u128::from(self.view.downloaded_bytes.min(self.view.download_bytes));
        let steps = (done * PROGRESS_STEPS as u128 + whole / 2) / whole;
        // At most the bar's thousand steps, since done is at most the whole.
        Some(steps as i32)
    }

    /// Whether the state shows `button`; the others are hidden.
    pub fn button_shown(&mut self, button: UpdateButton) -> bool {
        self.look();
        self.shows(button)
    }

    fn shows(&self, button: UpdateButton) -> bool {
        let view = &self.view;
        match view.state {
            UpdateDialogState::Idle | UpdateDialogState::Current | UpdateDialogState::NotOffered => {
                button == UpdateButton::CheckNow
            }
            UpdateDialogState::Checking | UpdateDialogState::Downloading => {
                button == UpdateButton::Cancel
            }
            UpdateDialogState::Available => match button {
                // A copy that cannot replace its own files is told of the release and sent to its
                // notes; nothing here can install it.
                UpdateButton::Install | UpdateButton::DownloadOnly => !view.notify_only,
                UpdateButton::Skip | UpdateButton::Notes | UpdateButton::Later => true,
                UpdateButton::CheckNow
                | UpdateButton::Retry
                | UpdateButton::Restart
                | UpdateButton::Cancel => false,
            },
            UpdateDialogState::Ready => {
                matches!(button, UpdateButton::Restart | UpdateButton::Later)
            }
            UpdateDialogState::Blocked | UpdateDialogState::CheckFailed => {
                button == UpdateButton::Retry
            }
            UpdateDialogState::RolledBack | UpdateDialogState::Unsupported => false,
        }
    }

    /// Whether `button` can be pressed: one with nothing to call is disabled - a dialog whose
    /// owner has gone, or a test's that gave no executor.
    pub fn button_enabled(&mut self, button: UpdateButton) -> bool {
        self.look();
        self.enables(button)
    }

    fn enables(&self, button: UpdateButton) -> bool {
        let lines = self.context.run.is_some();
        let view = &self.view;
        match button {
            UpdateButton::CheckNow | UpdateButton::Retry | UpdateButton::DownloadOnly => lines,
            UpdateButton::Install => self.context.update.is_some(),
            UpdateButton::Skip => lines && !view.latest_version.is_empty(),
            UpdateButton::Notes => self.context.open_notes.is_some() && !view.notes_url.is_empty(),
            // While the window waits - for a job, for a dialog that is open - before it closes,
            // the press is already made.
            UpdateButton::Restart => {
                lines && !view.waiting_for_jobs && !view.waiting_for_dialog
            }
            UpdateButton::Cancel => self.context.cancel.is_some(),
            // Later needs nobody: it only hides the dialog.
            UpdateButton::Later => true,
        }
    }

    /// `line` through the executor; a refusal is said in updateProblemText.
    fn run_line(&mut self, host: &mut dyn Host, line: &str) {
        let Some(run) = self.context.run.as_mut() else {
            return;
        };
        self.refusal.clear();
        let outcome = run(host, line);
        // The owner may have set a view from inside its own line: it is shown first, and the
        // line's refusal is said over it.
        self.look();
        if !outcome.ok {
            self.refusal = if outcome.error.is_empty() {
                String::from("The command was refused.")
            } else {
                outcome.error
            };
        }
    }

    /// What a click on `button` does, whether or not the button is shown or enabled: a click that
    /// reaches a button with nothing to call calls nothing.
    pub fn click(&mut self, host: &mut dyn Host, button: UpdateButton) {
        self.look();
        match button {
            UpdateButton::CheckNow => self.run_line(host, UPDATE_CHECK_LINE),
            // What failed is what is tried again: the swap for a copy that was blocked, the check
            // otherwise.
            UpdateButton::Retry => {
                let line = if self.view.state == UpdateDialogState::Blocked {
                    UPDATE_APPLY_LINE
                } else {
                    UPDATE_CHECK_LINE
                };
                self.run_line(host, line);
            }
            UpdateButton::Install => {
                if let Some(update) = self.context.update.as_mut() {
                    update(host);
                }
            }
            UpdateButton::DownloadOnly => self.run_line(host, UPDATE_DOWNLOAD_LINE),
            UpdateButton::Skip => {
                if !self.view.latest_version.is_empty() {
                    let line = update_skip_line(&self.view.latest_version);
                    self.run_line(host, &line);
                }
            }
            UpdateButton::Notes => {
                if !self.view.notes_url.is_empty()
                    && let Some(open_notes) = self.context.open_notes.as_mut()
                {
                    open_notes(host, &self.view.notes_url);
                }
            }
            UpdateButton::Restart => self.run_line(host, UPDATE_APPLY_LINE),
            UpdateButton::Cancel => {
                if let Some(cancel) = self.context.cancel.as_mut() {
                    cancel(host);
                }
            }
            UpdateButton::Later => {
                if let Some(later) = self.context.later.as_mut() {
                    later(host);
                }
                self.close();
            }
        }
        self.look();
    }

    /// One frame of the dialog, when it is shown.
    pub fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        self.look();
        if !self.visible {
            return;
        }
        let ctx = ui.ctx().clone();
        let mut frame = self.frame.clone();
        let mut lent = Lent {
            dialog: &mut *self,
            host: &mut *host,
        };
        let shown = frame.show(
            &ctx,
            &mut lent,
            |ui, lent| lent.dialog.body(ui),
            |ui, lent| lent.dialog.footer(ui, &mut *lent.host),
        );
        self.frame = frame;
        if shown.closed {
            self.close();
        }
    }

    fn body(&mut self, ui: &mut Ui) {
        // Wide enough for Available's five buttons in one row and its sentence in two lines; the
        // height follows the text.
        ui.set_width(560.0);
        // The first sentence is the answer; it is read before anything else. A version, a
        // program's name and a reason are data, never markup: egui draws a label's text as it is.
        let state = update_state_text(&self.view);
        controls::label_as(
            ui,
            "updateStateText",
            &state,
            egui::Label::new(RichText::new(state.as_str()).strong())
                .wrap()
                .selectable(true),
        );
        let detail = update_detail_text(&self.view);
        controls::label_as(
            ui,
            "updateDetailText",
            &detail,
            egui::Label::new(detail.as_str()).wrap().selectable(true),
        );
        let problem = String::from(self.problem());
        let colour = theme::error().to_egui();
        let draw_problem = |ui: &mut Ui| {
            controls::label_as(
                ui,
                "updateProblemText",
                &problem,
                egui::Label::new(RichText::new(problem.as_str()).color(colour))
                    .wrap()
                    .selectable(true),
            );
        };
        if problem.is_empty() {
            hidden(ui, "updateProblemText", draw_problem);
        } else {
            draw_problem(ui);
        }

        let steps = self.progress_steps();
        let draw_progress = |ui: &mut Ui| {
            let bar = match steps {
                Some(steps) => {
                    egui::ProgressBar::new(steps as f32 / PROGRESS_STEPS as f32).desired_height(14.0)
                }
                None => {
                    // Busy: a check cannot say how far it is, nor a download of no known size.
                    // The bar sweeps with the frames the running job keeps coming.
                    let time = ui.input(|input| input.time);
                    egui::ProgressBar::new((time * 0.5).fract() as f32).desired_height(14.0)
                }
            };
            let response = ui.add(bar);
            let mut record = ControlRecord::new("updateProgress", ControlKind::Other);
            record.value = steps.map_or_else(|| String::from("busy"), |steps| format!("{steps}"));
            controls::custom(ui, &response, record);
        };
        if matches!(
            self.view.state,
            UpdateDialogState::Downloading | UpdateDialogState::Checking
        ) {
            draw_progress(ui);
        } else {
            hidden(ui, "updateProgress", draw_progress);
        }
    }

    fn footer(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        let mut clicked = None;
        // At the right, drawn from the right edge in: the button that acts (check, retry, update,
        // restart) first, in the accent, so it sits last; then the rest from Later back, so the
        // row reads as the states list them with what acts moved to the end.
        let order = UPDATE_BUTTONS
            .into_iter()
            .filter(|button| button.acts())
            .chain(UPDATE_BUTTONS.into_iter().rev().filter(|button| !button.acts()));
        layout::footer_buttons(ui, |ui| {
            for button in order {
                let enabled = self.enables(button);
                let mut draw = |ui: &mut Ui| {
                    ui.add_enabled_ui(enabled, |ui| {
                        let shown = if button.acts() {
                            layout::primary(ui, button.name(), button.text())
                        } else {
                            layout::button(ui, button.name(), button.text())
                        };
                        if shown.on_hover_text(button.tip()).clicked() {
                            clicked = Some(button);
                        }
                    });
                };
                if self.shows(button) {
                    draw(ui);
                } else {
                    hidden(ui, button.name(), draw);
                }
            }
        });
        if let Some(button) = clicked {
            self.click(host, button);
            ui.ctx().request_repaint();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{REASON_WORDS, replace_ignoring_case};

    // QString::replace(before, after, Qt::CaseInsensitive): every occurrence, in any case, and
    // the rest of the text as it was written.
    #[test]
    fn a_word_is_replaced_in_any_case_and_the_rest_is_kept_as_written() {
        assert_eq!(
            replace_ignoring_case("Token TOKEN token ToKeN", "token", "*"),
            "* * * *"
        );
        assert_eq!(replace_ignoring_case("No Such Word", "token", "*"), "No Such Word");
        assert_eq!(replace_ignoring_case("aXbxc", "x", "--"), "a--b--c");
        assert_eq!(replace_ignoring_case("kept", "", "*"), "kept");
    }

    // A token said twice would have two sentences, and the first would hide the second.
    #[test]
    fn each_reason_token_has_one_row() {
        for (at, (token, _)) in REASON_WORDS.iter().enumerate() {
            assert!(
                REASON_WORDS[..at].iter().all(|(known, _)| known != token),
                "{token}"
            );
        }
    }
}
