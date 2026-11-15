//! The update system in the window (docs/desktop.md, "Help > Check for Updates"): what only a
//! window has beside the UPDATE verbs it shares with katana_cli (katana_app's update_verbs). The
//! port of update_workbench.
//!
//! Built as the geoprocessing and utility workbenches are: the main window makes it, hands it the
//! few things the host does not give (`UpdateServices`) and asks it to run any line whose verb is
//! UPDATE - typed, or a dialog's through the window's one executor (`Host::run`). It adds:
//!
//! * Help > "Check for Updates..." (helpCheckForUpdates, which opens updateDialog) and the status
//!   bar's button (statusUpdateAvailable, "Update 0.3.0"), hidden until a release is known; both
//!   open the dialog;
//! * the dialog (update_dialog), which it owns and feeds: every sentence the dialog shows is an
//!   `UpdateView` this object made from a verb's reply;
//! * the verbs' context (katana_app's UpdateContext): made once, at first use, for the surface the
//!   window turned out to be - Window, or HeadlessWindow - with this process's id and this
//!   program's path, which the helper waits for and starts again;
//! * the lines that reach the network as BACKGROUND JOBS (host::jobs): UPDATE CHECK, UPDATE
//!   DOWNLOAD CONFIRM and UPDATE APPLY CONFIRM (whose preflight runs the helper's self-test) are
//!   run by run_update_line on a worker, and their reply is logged on the interface thread when
//!   they end, as a typed line's is. The other lines (STATUS, SKIP, BLOCKED, SET, TOKEN) read and
//!   write small files and run at once;
//! * the AUTOMATIC CHECK (`start`, then once an hour to see whether one is due): only in a window
//!   a person uses - never headless - of a copy that packaging installed (not a development
//!   tree), with the state's automatic flag on, when katana_update's check_is_due says so and
//!   KATANA_UPDATE_DISABLE is not set. It is quiet when it learns nothing, and a release the
//!   person skipped, or that was rolled back here, is not offered (apply_update_state) - and is
//!   taken back if the window still offered it. A person who asks for a check while it is on its
//!   way is given that one, aloud;
//! * ONE CLICK (the dialog's Update): the DOWNLOAD line, then the APPLY line, then the window's
//!   ordinary close - its unsaved-changes question included. Cancel there, and the window stays,
//!   the release stays staged and the dialog says Ready. A close that would cut a running job
//!   short waits for the job, and one that would close the program around a modal dialog the
//!   person opened meanwhile waits for that. ONLY when the close this object asked for succeeded
//!   does `take_pending_apply` hand the program the request to start the helper with, after the
//!   window has gone;
//! * what the last apply ended as (U/last.json), shown once at the next start and then cleared;
//!   and a release downloaded in an earlier session offered again as ready - unless it was
//!   skipped since, or rolled back here;
//! * THE SWEEP at a start, on Windows, where an update can have run: what a lock kept an earlier
//!   update from deleting (U/cleanup.list), and with it the helper's copy U/run-<tx> of each
//!   update of THIS install that needed a recovery and is over - which only the install's own
//!   R/.update/recovery.log names, since a recovery writes nothing per user;
//! * THE CONFIRMATION at a start's first idle turn, of an update that was installed and never said
//!   to start: its helper died - power, a shutdown, the process ended - while it waited for the
//!   new version to prove that it starts, so nothing recorded that it does. A window of that very
//!   version which has come idle IS that proof, and settles the update (katana_update's
//!   confirm_started). While a helper is alive - it holds the update lock, and settles its own
//!   update - the window asks again, every five seconds for a minute, and then leaves it to the
//!   next start or the next logon.
//!
//! WHAT IT DECIDES IS NOT HERE. Which view an answer is, what a line that was run at once does to
//! what is shown, what is offered at a start, when the automatic check is due, what its answer
//! does to what is shown, what the log may show, what Settings > Updates is told of the state
//! file, which helper copies a start may remove and whether a start may confirm an update are
//! functions of their arguments beside the dialog (update_dialog, "THE DECISIONS"), where the
//! tests drive them. This object gathers the facts - the disk, the clock, the environment, the job
//! runner - and does what they say.
//!
//! A HEADLESS RUN (docs/headless.md): no automatic check, no per-user state read or written (the
//! context has no state folder), nothing shown unless --dialog asks for the dialog. A typed UPDATE
//! CHECK waits for its job, so its reply is logged before the next step; DOWNLOAD and APPLY are
//! handed to the verbs as they are, which refuse them for this surface (reason=headless).
//!
//! THE RUST SHAPE.
//!
//! * THE ONE RE-ENTRY. The dialog's buttons are pressed while the workbench draws it, and the
//!   executor they hand their lines to comes back to this workbench's `run_line`. So everything
//!   `run_line` reads and writes is behind a shared handle (`UpdateLines`) the workbench, its
//!   dialog's callbacks, its jobs' ends and the window's dispatcher each hold, as the utilities
//!   workbench's lines are; the dialog is the workbench's own, and is told what to show by a post
//!   (update_dialog's `UpdateViewPost`). Nothing of the handle is borrowed while the window is
//!   called.
//! * TIMERS. The C++'s timers are the frames: what was a turn of the event loop
//!   (`QTimer::singleShot(0)`) is done at the start of the next frame (`UpdateLines::frame`), and
//!   the hourly probe, the wait for a job and the confirmation's retry are times a frame looks
//!   at, each asking egui for a frame when it is due.
//! * THE JOB'S CONTEXT. The C++ hands its worker a copy of the context. The Rust context borrows
//!   the program's stores, which are one a thread (katana_app's update_verbs, "A window that
//!   hands a context to a job's thread"), so the worker MAKES its context, on its own thread, for
//!   the same surface, with this process's id, this program's path and the route the last check
//!   came by.
//! * WHAT THE HOST DOES NOT GIVE is asked of `UpdateServices`: the window's close, whether a modal
//!   dialog is open over it, and the time as a person's clock shows it.

use std::cell::RefCell;
use std::hash::{BuildHasher, Hasher};
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime};

use egui::Ui;
use katana_app::update_verbs::{
    UpdateContext, UpdateOffer, UpdateReply, UpdateSurface, abandon_apply, is_update_line,
    make_update_context, run_update_line,
};
use katana_core::civil_date::{SECONDS_PER_DAY, unix_seconds};
use katana_core::error::make_error;
use katana_core::path_text::{environment_variable, path_from_utf8, path_to_utf8};
use katana_core::{ErrorCode, Result, Status};
use katana_update::check::{CheckStatus, apply_update_state};
use katana_update::{
    ApplyRequest, CHECK_JITTER_MAX, Confirmed, CredentialStore, EntryKind, FeedRoute, FsOps,
    InstallKind, RetryPolicy, UpdateState, append_cleanup, clear_last_result, confirm_started,
    make_system_fs_ops, make_system_registry_ops, process_cleanup_list, program_version,
    program_version_text, read_install, read_last_result, read_state, run_path, stage_path,
    staged_manifest_path, update_work_directory,
};

use crate::customisation::settings_dialog::SettingsUpdates;
use crate::drive::action::{Action, Menu, ToolBar};
use crate::drive::controls;
use crate::drive::registry::simplified;
use crate::host::{
    Host, JobControl, JobId, JobOutcome, JobReport, NO_JOB, job_finished, job_work,
};
use crate::icons::{IconMode, show_icon};
use crate::script_runner::to_native_separators;
use crate::shell::workbench::{DockSpec, Workbench};
use crate::survey::survey_dialogs::hidden;
use crate::update_dialog::{
    UPDATE_APPLY_LINE, UPDATE_CHECK_LINE, UPDATE_DOWNLOAD_LINE, UpdateAutoCheckFacts,
    UpdateConfirmFacts, UpdateDialog, UpdateDialogContext, UpdateDialogState, UpdateLineEffect,
    UpdateOfferFacts, UpdatePutBackFacts, UpdateQuietAnswer, UpdateView, UpdateViewPost,
    run_through_host,
    update_automatic_check_is_due, update_detail_text, update_effect_of_line,
    update_files_of_progress_line, update_logged_line, update_logged_reply,
    update_put_back_of_state, update_quiet_answer, update_reason_words,
    update_recovered_transactions, update_reply_value, update_skipped_of_state,
    update_staged_after, update_staged_of_reply, update_staged_to_offer,
    update_start_may_confirm, update_state_text, update_status_button_text,
    update_tells_of_set_aside, update_view_of_last_result, update_view_of_offer,
};

/// A copy of Katana can replace its own files on Windows only: the apply is not written for Linux,
/// and a macOS bundle is sealed (docs/update.md, the platform table). Elsewhere a release is told
/// of and nothing more.
const APPLIES_HERE: bool = cfg!(windows);

/// Set to anything, it switches the automatic check off for the session.
const DISABLE_VARIABLE: &str = "KATANA_UPDATE_DISABLE";
/// How often an open window asks whether an automatic check has come due. The check itself is
/// every 24 hours and up to 2 more (katana_update's CHECK_INTERVAL, CHECK_JITTER_MAX); an hour is
/// how late past due a window left open may be, and costs one small file read.
const DUE_PROBE: Duration = Duration::from_secs(60 * 60);
/// How often a close that waits for a job looks again.
const JOB_POLL: Duration = Duration::from_millis(250);
/// A start that would confirm an update and finds the update lock held asks again this often,
/// this many times, and then leaves it (`confirm_start`). The lock is held by a helper that is
/// alive, and the longest it holds it over an update it has installed is its wait for the new
/// version's health run - a minute (katana_update's ApplyTimings), which twelve times five seconds
/// is; after that the helper has settled the update itself. The two numbers are the hardening
/// pass's specification of 2026-10-14 for this call.
const CONFIRM_RETRY: Duration = Duration::from_secs(5);
const CONFIRM_RETRIES: u32 = 12;
/// The install's own record of its recoveries, R/.update/recovery.log (katana_update's recover,
/// "WHAT RECOVERY WRITES, AND WHERE").
const RECOVERY_LOG_NAME: &str = "recovery.log";
/// The most of that record a start reads. A recovery's line is its time, a few words and the ids
/// it found and left, 32 characters each: about a hundred bytes, written once for an update that a
/// crash interrupted. A megabyte is some ten thousand of them, and what cleanup.list is held to
/// (katana_update's state); a file beyond it is not one the helper wrote, and is not read.
const RECOVERY_LOG_LIMIT: u64 = 1024 * 1024;

/// The object name of the dialog the Help item and the status bar's button open: how --dialog
/// finds it.
const DIALOG: &str = "updateDialog";
/// The Help item's and the status bar button's object names.
const ACTION: &str = "helpCheckForUpdates";
const STATUS_BUTTON: &str = "statusUpdateAvailable";

/// The window's close, as a turn of the event loop asks for it: true when the window closed.
pub type CloseWindow = Box<dyn FnMut(&mut dyn Host) -> bool>;
/// The verbs' context of the running program for a surface, made on the thread that asks.
pub type MakeContext = Arc<dyn Fn(UpdateSurface) -> UpdateContext<'static> + Send + Sync>;

/// What the window hands the workbench beside the host it lends at each call. The C++'s services -
/// the action factory, the log, whether the session is headless - are the menu rows' and the
/// host's; these are what the host does not give.
#[derive(Default)]
pub struct UpdateServices {
    /// The window's ordinary close (QWidget::close): the Format managers' unapplied edits and the
    /// unsaved-changes question are asked as at any other close, and true answered when the window
    /// closed. None: the window cannot be closed from here, which is taken as a close that was
    /// cancelled.
    pub close_window: Option<CloseWindow>,
    /// Whether a modal dialog is open over the window (QApplication::activeModalWidget). None:
    /// none is.
    pub modal_open: Option<Box<dyn Fn() -> bool>>,
    /// The time now as a person's clock shows it, "14:02" (QTime::currentTime, "HH:mm"). None:
    /// the time in UTC, said to be so ("12:02 UTC"), since nothing here knows the time zone.
    pub clock_text: Option<Box<dyn Fn() -> String>>,
    /// The verbs' context for a surface. None: the running program's
    /// (katana_app's make_update_context). A test's stands in for the program and its feed.
    pub make_context: Option<MakeContext>,
}

/// Which line a running job is of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JobKind {
    None,
    Check,
    Download,
    Apply,
}

/// What a job's worker and the workbench share. The atomics are the download's progress, written
/// on the worker and read at each frame here; `reply` is written by the worker as its last act and
/// read on the interface thread once the job runner has delivered its completion.
#[derive(Default)]
struct Shared {
    done: AtomicU64,
    total: AtomicU64,
    files_done: AtomicU64,
    files_known: AtomicBool,
    reply: Mutex<Option<Result<UpdateReply>>>,
}

/// What a turn of the event loop was asked for (`QTimer::singleShot(0)`): done at the start of the
/// next frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Turn {
    /// The second line of the one click, once the job runner has finished with the download.
    ApplyLine,
    /// The window's close for a prepared apply.
    CloseWhenIdle,
    /// The first idle turn of a start: the confirmation, then the check when one is due.
    FirstIdle,
}

/// A store the program keeps for its whole life, as Settings > Updates holds one.
struct ProgramStore(&'static dyn CredentialStore);

impl CredentialStore for ProgramStore {
    fn read(&self) -> Result<String> {
        self.0.read()
    }

    fn write(&self, token: &[u8]) -> Status {
        self.0.write(token)
    }

    fn clear(&self) -> Status {
        self.0.clear()
    }

    fn r#where(&self) -> &str {
        self.0.r#where()
    }
}

/// Everything the workbench keeps, behind the one handle (`UpdateLines`).
struct Core {
    services: UpdateServices,
    context: Option<UpdateContext<'static>>,
    /// The route the last check's answer came by (katana_update's feed): put into every context
    /// made after it, so the download asks the way the check did.
    route: Option<FeedRoute>,
    /// What the dialog shows, kept here so that it is there before the dialog is.
    view: UpdateView,
    /// Where the dialog is told what to show, once it is made.
    post: Option<UpdateViewPost>,
    /// What it showed when the running job started: where a cancelled or failed job goes back to.
    view_before_job: UpdateView,
    /// The release the last check offered, for the download's sentence.
    offer: Option<UpdateOffer>,
    /// The version that is downloaded and waiting, empty when none is.
    staged_version: String,

    running: JobKind,
    job: JobId,
    job_title: String,
    shared: Option<Arc<Shared>>,
    /// The running check is the automatic one.
    automatic_check: bool,
    /// The next DOWNLOAD line is the one click's.
    apply_after_download: bool,
    /// The running download is followed by APPLY.
    then_apply: bool,

    turns: Vec<Turn>,
    /// When the hourly probe, the wait for a job and the confirmation's retry look next.
    due_at: Option<Instant>,
    wait_at: Option<Instant>,
    confirm_at: Option<Instant>,
    /// Drawn once for the process (katana_update's check_is_due), in seconds.
    jitter: i64,
    started: bool,
    /// How often `confirm_start` has asked again because the update lock was held.
    confirm_retries: u32,

    pending_apply: Option<ApplyRequest>,
    closed_for_apply: bool,

    /// The status bar's button: its words, kept while it is hidden, and whether it is shown.
    status_text: String,
    status_shown: bool,
    /// The dialog is to be shown: how the last update ended is said by a window that opens.
    wants_dialog: bool,
    /// The release notes' addresses to hand the system's browser at the next frame.
    notes_to_open: Vec<String>,
}

/// The update lines' runner and everything they keep, shared by the workbench, its dialog's
/// callbacks, its jobs' ends and the window's dispatcher (see "THE ONE RE-ENTRY" in the module
/// comment). Cloning it shares it.
#[derive(Clone)]
pub struct UpdateLines {
    core: Rc<RefCell<Core>>,
}

fn now_epoch() -> i64 {
    unix_seconds(SystemTime::now())
}

/// The time of day in UTC, "12:02 UTC": what is said where nothing gives a person's clock.
fn utc_clock_text() -> String {
    let seconds = now_epoch().rem_euclid(SECONDS_PER_DAY);
    format!("{:02}:{:02} UTC", seconds / 3600, seconds % 3600 / 60)
}

fn text_of(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// A reply as the log shows it: without the line breaks it may end with.
fn reply_text(text: &str) -> &str {
    text.trim_end_matches(['\n', '\r'])
}

/// What could not be done, for the sentence under the dialog's two: the refusal's own words, or
/// the words for the reason its reply gives.
fn why_not(reply: &Result<UpdateReply>) -> String {
    match reply {
        Err(error) => error.message.clone(),
        Ok(reply) => update_reason_words(&update_reply_value(&reply.text, "reason")),
    }
}

fn problem_sentence(what: &str, why: &str) -> String {
    if why.is_empty() {
        format!("{what}; the command log says why.")
    } else {
        format!("{what}: {why}.")
    }
}

fn automatic_check_disabled() -> bool {
    !environment_variable(Some(DISABLE_VARIABLE)).is_empty()
}

/// Whether the complete stage of the release with this sequence is in the update folder: its
/// manifest is the last thing a download writes there (katana_update's stage), so the file's
/// presence is the stage's.
fn stage_is_there(state_directory: &Path, sequence: u64) -> bool {
    staged_manifest_path(&stage_path(state_directory, sequence)).exists()
}

/// A jitter drawn once for the process, uniformly enough in [0, CHECK_JITTER_MAX] seconds, so that
/// a release does not meet every copy at the same minute. The standard library's hash keys are
/// drawn from the system's randomness, which is the one source of it the port has.
fn draw_jitter() -> i64 {
    let drawn = std::collections::hash_map::RandomState::new()
        .build_hasher()
        .finish();
    // CHECK_JITTER_MAX is two hours of seconds: the remainder fits.
    (drawn % (CHECK_JITTER_MAX as u64 + 1)) as i64
}

/// Whether `url` is an https address a signed manifest may give (katana_update's manifest):
/// nothing else is handed to the system to open. The scheme in any case, a host after it, and no
/// blank or control character anywhere, as a strict reading of a URL refuses them.
fn is_https_address(url: &str) -> bool {
    const SCHEME: &str = "https://";
    let Some(scheme) = url.get(..SCHEME.len()) else {
        return false;
    };
    if !scheme.eq_ignore_ascii_case(SCHEME) {
        return false;
    }
    let rest = &url[SCHEME.len()..];
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    !host.is_empty()
        && !url
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
}

/// The host of a feed's base address ("https://releases.example.org/katana" is
/// "releases.example.org"), as QUrl::host gives it; empty for text that names none.
fn host_of(address: &str) -> String {
    let Some((_, rest)) = address.split_once("://") else {
        return String::new();
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = authority.rsplit('@').next().unwrap_or("");
    // A bracketed IPv6 address keeps its colons; any other host ends at its port's.
    let host = match host.strip_prefix('[') {
        Some(bracketed) => bracketed.split(']').next().unwrap_or(""),
        None => host.split(':').next().unwrap_or(""),
    };
    host.to_ascii_lowercase()
}

/// Lists for the window's sweep (katana_update's state, cleanup.list) the helper's copy of each
/// update of THIS install that needed a recovery and is over. `katana_updater --recover` writes
/// nothing per user, whoever started it, so nothing in the update folder names that copy:
/// unlisted, one helper's size would stay there for every update a crash interrupted. Which
/// transactions those are is `update_recovered_transactions`', which also says why no other
/// `run-<tx>` is touched: the update folder is every install's.
///
/// The record is read as recovery reads the folder it lies in - never through a link - and a copy
/// that is gone already, which is every start but the first after a recovery, is not listed
/// again. Empty when there was nothing to list or it is listed; else why it could not be.
fn list_recovered_copies(state_directory: &Path, root: &Path, files: &dyn FsOps) -> String {
    if root.as_os_str().is_empty() {
        return String::new();
    }
    let work = update_work_directory(root);
    let folder = match files.stat(&work) {
        Ok(folder) => folder,
        Err(error) => return error.message,
    };
    if folder.kind != EntryKind::Directory || folder.reparse_point {
        // A copy that was never updated, or a folder that is not the install's own.
        return String::new();
    }
    let record = work.join(path_from_utf8(RECOVERY_LOG_NAME));
    let written = match files.stat(&record) {
        Ok(written) => written,
        Err(error) => return error.message,
    };
    if written.kind != EntryKind::File || written.reparse_point || written.size > RECOVERY_LOG_LIMIT
    {
        // No record: no recovery has run here, which is every install but the few a crash caught.
        // Or one the helper did not write - a link, a folder, a file too long: not read.
        return String::new();
    }
    let text = match files.read_file(&record) {
        Ok(text) => text,
        Err(error) => return error.message,
    };
    let mut is_over = |transaction: &str| {
        // A folder that cannot be examined is not a folder that is gone.
        files
            .stat(&work.join(path_from_utf8(transaction)))
            .is_ok_and(|left| left.kind == EntryKind::Missing)
    };
    let over = update_recovered_transactions(&text, Some(&mut is_over));
    let mut failed = String::new();
    for transaction in &over {
        let copy = run_path(state_directory, transaction);
        if files
            .stat(&copy)
            .is_ok_and(|there| there.kind == EntryKind::Missing)
        {
            continue;
        }
        // Everything that can be listed is; the first refusal is the one said.
        if let Err(error) = append_cleanup(state_directory, &copy, files)
            && failed.is_empty()
        {
            failed = error.message;
        }
    }
    failed
}

impl UpdateLines {
    fn new(services: UpdateServices) -> Self {
        Self {
            core: Rc::new(RefCell::new(Core {
                services,
                context: None,
                route: None,
                view: UpdateView::default(),
                post: None,
                view_before_job: UpdateView::default(),
                offer: None,
                staged_version: String::new(),
                running: JobKind::None,
                job: NO_JOB,
                job_title: String::new(),
                shared: None,
                automatic_check: false,
                apply_after_download: false,
                then_apply: false,
                turns: Vec::new(),
                due_at: None,
                wait_at: None,
                confirm_at: None,
                jitter: draw_jitter(),
                started: false,
                confirm_retries: 0,
                pending_apply: None,
                closed_for_apply: false,
                status_text: String::new(),
                status_shown: false,
                wants_dialog: false,
                notes_to_open: Vec::new(),
            })),
        }
    }

    /// The core, for one short look or change: never held while the window is called.
    fn with<R>(&self, look: impl FnOnce(&mut Core) -> R) -> R {
        look(&mut self.core.borrow_mut())
    }

    /// The verbs' context, made at first use. By then the window knows whether it is headless:
    /// every first use is after the program has said so. A headless run gets the surface that
    /// keeps no per-user state and refuses to download or apply.
    fn ensure_context(&self, host: &dyn Host) {
        if self.with(|core| core.context.is_some()) {
            return;
        }
        let surface = Self::surface(host);
        let make = self.with(|core| core.services.make_context.clone());
        let mut made = match make {
            Some(make) => make(surface),
            None => make_update_context(surface),
        };
        // What the helper waits for before it swaps anything, and what it starts again
        // afterwards: this process and this program.
        made.pid = u64::from(std::process::id());
        made.relaunch = std::env::current_exe().unwrap_or_default();
        self.with(|core| {
            if let Some(route) = core.route {
                made.feed.route = route;
            }
            core.context = Some(made);
        });
    }

    fn surface(host: &dyn Host) -> UpdateSurface {
        if host.headless() {
            UpdateSurface::HeadlessWindow
        } else {
            UpdateSurface::Window
        }
    }

    /// Something of the context, which is made first if it is not there yet.
    fn context<R>(&self, host: &dyn Host, read: impl FnOnce(&UpdateContext<'static>) -> R) -> R {
        self.ensure_context(host);
        let core = self.core.borrow();
        let context = core
            .context
            .as_ref()
            .expect("ensure_context makes the context");
        read(context)
    }

    /// The version this copy is, as the dialog names it.
    fn installed_version(&self, host: &dyn Host) -> String {
        self.context(host, |context| {
            // With no install folder found there is no record to read a version from: the copy
            // is the program as it was built.
            if context.install.root.as_os_str().is_empty() {
                String::from(program_version_text())
            } else {
                katana_update::version::to_string(&context.install.version)
            }
        })
    }

    fn view_of(&self, host: &dyn Host, state: UpdateDialogState) -> UpdateView {
        UpdateView {
            state,
            installed_version: self.installed_version(host),
            ..UpdateView::default()
        }
    }

    fn ready_view(&self, host: &dyn Host) -> UpdateView {
        let mut view = self.view_of(host, UpdateDialogState::Ready);
        view.latest_version = self.with(|core| core.staged_version.clone());
        view
    }

    /// What a check's answer is shown as. The facts only this object has; what they make of the
    /// answer is `update_view_of_offer`'s.
    fn view_of_offer(&self, host: &dyn Host, offer: &UpdateOffer) -> UpdateView {
        let facts = UpdateOfferFacts {
            installed_version: self.installed_version(host),
            staged_version: self.with(|core| core.staged_version.clone()),
            checked_at: self.with(|core| match &core.services.clock_text {
                Some(clock_text) => clock_text(),
                None => utc_clock_text(),
            }),
            now_epoch: now_epoch(),
            applies_here: APPLIES_HERE,
        };
        update_view_of_offer(offer, &facts)
    }

    /// Makes `view` what the dialog shows and the status-bar button follows.
    fn show(&self, view: UpdateView) {
        self.with(|core| {
            // The button is there while there is a release to act on, and says which
            // (`update_status_button_text`); it keeps its words while it is hidden.
            let offered = update_status_button_text(&view);
            core.status_shown = !offered.is_empty();
            if !offered.is_empty() {
                core.status_text = offered;
            }
            if let Some(post) = &core.post {
                post.post(view.clone());
            }
            core.view = view;
        });
    }

    fn shown(&self) -> UpdateView {
        self.with(|core| core.view.clone())
    }

    fn running(&self) -> JobKind {
        self.with(|core| core.running)
    }

    // ---- the lines ------------------------------------------------------------------------------

    /// True when `line` is an UPDATE line, which it then runs: answered in the log at once, or
    /// started as a job. False leaves the line to whoever asked.
    pub fn run_line(&self, host: &mut dyn Host, line: &str) -> bool {
        if !is_update_line(line.trim()) {
            return false;
        }
        // The three lines that take time are known by their whole text, which is the whole of
        // their grammar; anything else under those words is a line the verbs refuse at once (a
        // DOWNLOAD without its CONFIRM), and is run at once like the rest.
        let words = simplified(line).to_uppercase();
        let check = words == UPDATE_CHECK_LINE;
        let download = words == UPDATE_DOWNLOAD_LINE;
        let apply = words == UPDATE_APPLY_LINE;
        if check {
            self.start_check(host, false);
        } else if (download || apply) && !host.headless() {
            if download {
                let then_apply = self.with(|core| core.apply_after_download);
                self.start_download(host, then_apply);
            } else {
                self.start_apply(host);
            }
        } else {
            // In a headless run DOWNLOAD and APPLY come here too: the verbs refuse them for that
            // surface, in their own words (reason=headless).
            self.run_now(host, line);
        }
        true
    }

    /// The reply, or the refusal, in the log. `line` is the line it answers, when that was typed
    /// by a person: what stands in it where someone hoped UPDATE TOKEN would take a token is shown
    /// as "***" wherever the verbs' words repeat it, in any case (`update_logged_reply`): a
    /// refusal may quote the line it refuses.
    fn log_reply(host: &mut dyn Host, reply: &Result<UpdateReply>, line: &str) {
        let said = match reply {
            Ok(reply) => reply.text.clone(),
            Err(error) => error.describe(),
        };
        host.log(
            &update_logged_reply(reply_text(&said), line),
            reply.is_err(),
        );
    }

    /// False, said in the log, while another update job runs.
    fn may_start(&self, host: &mut dyn Host, line: &str) -> bool {
        let (running, title) = self.with(|core| (core.running, core.job_title.clone()));
        if running == JobKind::None {
            return true;
        }
        // The line as the log may show it: never what was typed after UPDATE TOKEN.
        let refusal = make_error(
            ErrorCode::InvalidState,
            "an update job is still running; wait for it to end, or cancel it",
            format!(
                "{} while {title}",
                update_logged_line(&simplified(line))
            ),
        );
        host.log(&refusal.describe(), true);
        false
    }

    /// A line that answers at once, on the interface thread.
    fn run_now(&self, host: &mut dyn Host, line: &str) {
        let simple = simplified(line);
        let action = simple
            .split(' ')
            .filter(|word| !word.is_empty())
            .nth(1)
            .map(str::to_uppercase)
            .unwrap_or_default();
        // STATUS only reads. Every other line may write the state file a running job writes too
        // (a check records when it ran), and two writers of one small file lose one of the two
        // changes.
        if action != "STATUS" && !self.may_start(host, line) {
            return;
        }
        // No UPDATE line takes a token, and a person may type one that does all the same. The
        // verbs refuse it; whatever they say back is logged without the words that may be one
        // (`log_reply`; `update_logged_line` hides them in the echo).
        let reply = self.context(&*host, |context| run_update_line(context, line.trim()));
        Self::log_reply(host, &reply, line);
        let Ok(reply) = reply else {
            return;
        };
        if action == "TOKEN" {
            // The context carries the token it was made with: made again at the next use, it
            // reads the store as the line left it.
            self.with(|core| core.context = None);
        }
        // What the line, carried out, does to what the dialog shows is `update_effect_of_line`'s.
        let shown = self.shown();
        match update_effect_of_line(&shown, line, &reply.text) {
            UpdateLineEffect::Forgotten => {
                // UPDATE SKIP NONE or UPDATE BLOCKED CLEAR: the release the dialog says "is not
                // offered" is set aside no longer, and a dialog that went on saying so would say
                // what is not so. It goes back to knowing nothing, and the next check - Help >
                // Check for Updates runs one - offers the release.
                let view = self.view_of(&*host, UpdateDialogState::Idle);
                self.show(view);
            }
            UpdateLineEffect::Skipped => {
                // What was offered is not any more: neither the dialog nor a download typed next
                // goes on about it.
                self.with(|core| core.offer = None);
                let mut view = self.view_of(&*host, UpdateDialogState::NotOffered);
                view.latest_version = shown.latest_version;
                view.reason = String::from("skipped");
                self.show(view);
            }
            UpdateLineEffect::Nothing => {}
        }
    }

    // ---- the jobs -------------------------------------------------------------------------------

    fn start_check(&self, host: &mut dyn Host, automatic: bool) {
        let (running, quiet) = self.with(|core| (core.running, core.automatic_check));
        if automatic {
            // Quiet in every way: it does not even say it could not start.
            if running != JobKind::None {
                return;
            }
        } else if running == JobKind::Check && quiet {
            // The check the window started by itself is on its way, and a person asks for one:
            // that one becomes theirs - answered aloud, in the log and in the dialog - and is not
            // started twice, nor are they told that a job they never started is in their way.
            // Cancelled, it goes back to what was shown before it began (view_before_job is still
            // that).
            self.with(|core| core.automatic_check = false);
            let view = self.view_of(&*host, UpdateDialogState::Checking);
            self.show(view);
            return;
        } else if !self.may_start(host, UPDATE_CHECK_LINE) {
            return;
        }
        self.with(|core| {
            core.view_before_job = core.view.clone();
            core.automatic_check = automatic;
        });
        if !automatic {
            let view = self.view_of(&*host, UpdateDialogState::Checking);
            self.show(view);
        }
        self.begin_job(
            host,
            JobKind::Check,
            "Checking for updates",
            UPDATE_CHECK_LINE,
        );
    }

    fn start_download(&self, host: &mut dyn Host, then_apply: bool) {
        if !self.may_start(host, UPDATE_DOWNLOAD_LINE) {
            return;
        }
        // What the last check said the download is; a download typed before any check has only
        // what its progress will say.
        let mut view = self.view_of(&*host, UpdateDialogState::Downloading);
        self.with(|core| {
            core.view_before_job = core.view.clone();
            core.then_apply = then_apply;
            view.latest_version = core.view.latest_version.clone();
            if let Some(offer) = &core.offer {
                view.latest_version = offer.check.latest_version.clone();
                view.changed_files = offer.changed_files;
                view.total_files = offer.total_files;
                view.download_bytes = offer.download_bytes;
            }
        });
        let title = if view.latest_version.is_empty() {
            String::from("Downloading the update")
        } else {
            format!("Downloading Katana {}", view.latest_version)
        };
        self.show(view);
        self.begin_job(host, JobKind::Download, &title, UPDATE_DOWNLOAD_LINE);
    }

    fn start_apply(&self, host: &mut dyn Host) {
        if !self.may_start(host, UPDATE_APPLY_LINE) {
            return;
        }
        // The press is made: until the helper has been copied and has proved it starts, and the
        // window has closed, only Later - which takes the press back - is left.
        let mut view = self.ready_view(&*host);
        self.with(|core| {
            core.view_before_job = core.view.clone();
            if view.latest_version.is_empty() {
                view.latest_version = core.view.latest_version.clone();
            }
        });
        view.waiting_for_jobs = true;
        self.show(view);
        self.begin_job(
            host,
            JobKind::Apply,
            "Preparing the update",
            UPDATE_APPLY_LINE,
        );
    }

    fn begin_job(&self, host: &mut dyn Host, kind: JobKind, title: &str, line: &'static str) {
        // The context is made here first, on the interface thread, as every use of it is: what
        // the worker makes is for the same surface, with the same process, program and route.
        self.ensure_context(&*host);
        let surface = Self::surface(&*host);
        let (make, pid, relaunch, route) = self.with(|core| {
            let context = core
                .context
                .as_ref()
                .expect("ensure_context makes the context");
            (
                core.services.make_context.clone(),
                context.pid,
                context.relaunch.clone(),
                core.route,
            )
        });
        let shared = Arc::new(Shared::default());
        self.with(|core| {
            core.running = kind;
            core.job_title = String::from(title);
            core.shared = Some(Arc::clone(&shared));
        });
        let worker = Arc::clone(&shared);
        let work = job_work(move |control: &JobControl| {
            let progress = |done: u64, total: u64| {
                worker.done.store(done, Ordering::Relaxed);
                worker.total.store(total, Ordering::Relaxed);
                if total > 0 {
                    control.set_progress(done as f64 / total as f64);
                }
            };
            let said = |line: &str| {
                if let Some((done, _)) = update_files_of_progress_line(line) {
                    worker.files_done.store(done, Ordering::Relaxed);
                    worker.files_known.store(true, Ordering::Relaxed);
                }
            };
            let mut context: UpdateContext<'_> = match &make {
                Some(make) => make(surface),
                None => make_update_context(surface),
            };
            context.pid = pid;
            context.relaunch = relaunch;
            if let Some(route) = route {
                context.feed.route = route;
            }
            context.stop = Some(control.stop_flag());
            context.progress = Some(&progress);
            context.progress_line = Some(&said);
            let reply = run_update_line(&context, line);
            *worker.reply.lock().unwrap_or_else(PoisonError::into_inner) = Some(reply);
            // Nothing to apply: the reply is read where the job ends.
            Ok(None)
        });
        let lines = self.clone();
        let ended = Arc::clone(&shared);
        let finished = job_finished(move |report: &JobReport, host: &mut dyn Host| {
            lines.finished(host, kind, &ended, report);
        });
        let job = host.jobs().start(title, work, Some(finished));
        self.with(|core| {
            // A job that ended inside its own start has already been taken note of.
            if core.running == kind {
                core.job = job;
            }
        });
        // A headless run has nobody to wait for the reply, and its next step may read it: the
        // line is over when its job is.
        if host.headless() {
            host.wait_for(job);
        }
    }

    fn finished(&self, host: &mut dyn Host, kind: JobKind, shared: &Shared, report: &JobReport) {
        let (automatic, then_apply, before) = self.with(|core| {
            core.running = JobKind::None;
            core.job = NO_JOB;
            core.job_title.clear();
            core.shared = None;
            (
                std::mem::take(&mut core.automatic_check),
                std::mem::take(&mut core.then_apply),
                core.view_before_job.clone(),
            )
        });
        let reply = shared
            .reply
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();

        if report.outcome == JobOutcome::Cancelled {
            // Whatever the worker had got to, a cancelled job changes nothing here: the dialog
            // goes back to what it showed.
            if kind == JobKind::Check && !automatic {
                host.log("The check for updates was cancelled.", false);
            } else if kind == JobKind::Download {
                host.log(
                    "The download was cancelled. What had arrived is kept, and the next download \
                     starts from it.",
                    false,
                );
            } else if kind == JobKind::Apply
                && let Some(Ok(prepared)) = &reply
                && let Some(request) = &prepared.apply
            {
                // Later was pressed while the helper was being prepared, and the preparation had
                // got as far as its copy: nobody will start it.
                Self::drop_prepared(host, request);
            }
            if !(kind == JobKind::Check && automatic) {
                self.show(before);
            }
            return;
        }
        let reply = match reply {
            Some(reply) if report.outcome != JobOutcome::Failed => reply,
            // The worker panicked, or ended with nothing: said as a refusal of the line.
            _ => Err(make_error(
                ErrorCode::Internal,
                "the update job failed",
                report.error.clone(),
            )),
        };
        match kind {
            JobKind::Check => self.check_finished(host, &reply, automatic),
            JobKind::Download => self.download_finished(host, &reply, then_apply),
            JobKind::Apply => self.apply_finished(host, &reply),
            JobKind::None => {}
        }
    }

    fn check_finished(&self, host: &mut dyn Host, reply: &Result<UpdateReply>, automatic: bool) {
        let offered = match reply {
            Ok(reply) => reply.offer.clone(),
            Err(_) => None,
        };
        let Some(mut offer) = offered else {
            // The line could not be run, or answered nothing a check answers.
            if automatic {
                return;
            }
            Self::log_reply(host, reply, "");
            let mut view = self.view_of(&*host, UpdateDialogState::CheckFailed);
            match reply {
                Ok(reply) => {
                    // A reply with no record of the offer is read by its words: the reason, and
                    // whether it is the copy that cannot update rather than the check that failed.
                    view.reason = update_reply_value(&reply.text, "reason");
                    if update_reply_value(&reply.text, "update") == "unsupported" {
                        view.state = UpdateDialogState::Unsupported;
                    }
                }
                Err(error) => view.problem = error.message.clone(),
            }
            self.show(view);
            return;
        };
        if automatic {
            // What the person already decided about this release is applied before anything is
            // offered: one they skipped, or one that was rolled back here, is not news.
            let state_directory = self.context(&*host, |context| context.state_directory.clone());
            if let Ok(state) = read_state(&state_directory) {
                apply_update_state(&mut offer.check, &state);
            }
        }
        let status = offer.check.status;
        let answer = self.view_of_offer(&*host, &offer);
        // Nobody asked for the automatic check's answer: whether it is said, shown without a
        // word, or dropped is `update_quiet_answer`'s.
        let quiet = if automatic {
            update_quiet_answer(&self.shown(), &answer)
        } else {
            UpdateQuietAnswer::Announce
        };
        if quiet == UpdateQuietAnswer::Nothing {
            return;
        }
        if status != CheckStatus::Unreachable && status != CheckStatus::Unsupported {
            // The feed answered. The download asks the way the check was answered (katana_update's
            // feed); the release offered is the one a download typed next is of, and an answer
            // that offers none leaves none - a check that failed un-knows nothing; and a
            // downloaded release the answer no longer offers is not ready any more
            // (`update_staged_after`).
            self.with(|core| {
                core.route = Some(offer.check.route);
                if let Some(context) = &mut core.context {
                    context.feed.route = offer.check.route;
                }
                core.offer = (status == CheckStatus::Available).then(|| offer.clone());
                core.staged_version = update_staged_after(&core.staged_version, &answer);
            });
        }
        if quiet == UpdateQuietAnswer::Announce {
            if automatic {
                // News: logged as the typed line would be, from its echo on.
                host.log(&format!("> {UPDATE_CHECK_LINE}"), false);
            }
            Self::log_reply(host, reply, "");
        }
        self.show(answer);
    }

    fn download_finished(
        &self,
        host: &mut dyn Host,
        reply: &Result<UpdateReply>,
        then_apply: bool,
    ) {
        Self::log_reply(host, reply, "");
        // The line's last record says what is staged (katana_app's update_verbs).
        let staged = match reply {
            Ok(reply) => update_staged_of_reply(&reply.text),
            Err(_) => String::new(),
        };
        if staged.is_empty() {
            let mut view = self.with(|core| core.view_before_job.clone());
            view.problem = problem_sentence("The download did not finish", &why_not(reply));
            self.show(view);
            return;
        }
        self.with(|core| core.staged_version = staged);
        let view = self.ready_view(&*host);
        self.show(view);
        if then_apply {
            // The second line of the one click, run as the first was - echoed and logged as a
            // typed line - once the job runner has finished with this job.
            self.with(|core| core.turns.push(Turn::ApplyLine));
        }
    }

    fn apply_finished(&self, host: &mut dyn Host, reply: &Result<UpdateReply>) {
        Self::log_reply(host, reply, "");
        let prepared = match reply {
            Ok(reply) => reply.apply.clone(),
            Err(_) => None,
        };
        let Some(request) = prepared else {
            let mut view = self.with(|core| core.view_before_job.clone());
            view.problem = problem_sentence("The update could not be started", &why_not(reply));
            self.show(view);
            return;
        };
        // Nothing in the install has changed yet. The window closes first - on the next turn, out
        // of the job runner's own completion.
        self.with(|core| {
            core.pending_apply = Some(request);
            core.turns.push(Turn::CloseWhenIdle);
        });
    }

    /// The download's bytes and files into the dialog, as the frames come.
    fn show_progress(&self) {
        let changed = self.with(|core| {
            let shared = core.shared.as_ref()?;
            if core.running != JobKind::Download
                || core.view.state != UpdateDialogState::Downloading
            {
                return None;
            }
            let mut view = core.view.clone();
            // The bytes the download itself says it will fetch, once it says.
            let total = shared.total.load(Ordering::Relaxed);
            if total > 0 {
                view.download_bytes = total;
            }
            view.downloaded_bytes = shared.done.load(Ordering::Relaxed);
            if shared.files_known.load(Ordering::Relaxed) {
                view.downloaded_files = Some(shared.files_done.load(Ordering::Relaxed));
            }
            (view != core.view).then_some(view)
        });
        if let Some(view) = changed {
            self.show(view);
        }
    }

    // ---- the dialog's callbacks -----------------------------------------------------------------

    /// The dialog's Update. One click, two lines and a close: the download's line now, marked so
    /// that the apply's follows when it has staged the release (`download_finished`).
    pub fn install_update(&self, host: &mut dyn Host) {
        self.with(|core| core.apply_after_download = true);
        let outcome = host.run(UPDATE_DOWNLOAD_LINE);
        self.with(|core| core.apply_after_download = false);
        // A line that was refused before it began - another update job is running - is in the
        // log; the press was made in the dialog, so it is said there too, unless the line's own
        // end has said why already.
        let mut view = self.shown();
        if !outcome.ok && view.problem.is_empty() {
            view.problem = if outcome.error.is_empty() {
                String::from("The update could not be started; the command log says why.")
            } else {
                outcome.error
            };
            self.show(view);
        }
    }

    /// The dialog's Cancel: the running check or download.
    pub fn cancel_job(&self, host: &mut dyn Host) {
        let job = self.with(|core| core.job);
        if job != NO_JOB {
            let _ = host.jobs().cancel(job);
        }
    }

    /// The dialog's Release notes.
    pub fn open_notes(&self, host: &mut dyn Host, url: &str) {
        // A signed manifest gives only an https address (katana_update's manifest); nothing else
        // is handed to the system to open.
        if !is_https_address(url) {
            let refusal = make_error(
                ErrorCode::InvalidArgument,
                "the release notes' address is not an https address",
                url,
            );
            host.log(&refusal.describe(), true);
            return;
        }
        if host.headless() {
            // Nobody to show a browser to: the address, for whoever reads the log.
            host.log(&format!("notes_url={url}"), false);
            return;
        }
        self.with(|core| core.notes_to_open.push(String::from(url)));
    }

    /// The dialog's Later. A press of Restart and update that has not closed the window yet is
    /// taken back: the helper's preparation is cancelled, or the close that was waiting for a job
    /// is given up. The release stays downloaded.
    pub fn later(&self, host: &mut dyn Host) {
        if self.running() == JobKind::Apply {
            self.cancel_job(host);
            return;
        }
        let dropped = self.with(|core| {
            if core.closed_for_apply {
                return None;
            }
            let request = core.pending_apply.take()?;
            core.wait_at = None;
            Some(request)
        });
        if let Some(request) = dropped {
            Self::drop_prepared(host, &request);
            let view = self.ready_view(&*host);
            self.show(view);
        }
    }

    /// A request UPDATE APPLY CONFIRM prepared and nobody will start - Later, or Cancel at the
    /// window's question. The one way a prepared apply is given up, which the session and
    /// katana_cli take too (katana_app's abandon_apply): the helper's copy U/run-<tx> goes on the
    /// list the next start sweeps. It is a program's size, and one would otherwise stay behind for
    /// every press that was taken back.
    fn drop_prepared(host: &mut dyn Host, request: &ApplyRequest) {
        if let Err(error) = abandon_apply(request) {
            // A leftover only: said, not an error of the session.
            host.log(
                &format!(
                    "Update: the helper's copy made for the update that was not started could \
                     not be listed for removal ({}).",
                    error.message
                ),
                false,
            );
        }
    }

    // ---- the close ------------------------------------------------------------------------------

    /// The window's close for a prepared apply, once no job would be cut short and no modal dialog
    /// is open over the window.
    fn close_when_idle(&self, host: &mut dyn Host) {
        let waiting = self.with(|core| {
            if core.pending_apply.is_none() || core.closed_for_apply {
                core.wait_at = None;
                false
            } else {
                true
            }
        });
        if !waiting {
            return;
        }
        // A window that closes stops every job it runs (the runner joins them as it goes): a
        // geoprocessing run is not cut short for an update. And this close comes from a turn of
        // the frames, not from a press: under a modal dialog the person opened meanwhile - a file
        // dialog, a question - it would close the program around that dialog. It waits for both.
        let jobs = host.jobs().active_count() > 0;
        let dialog_open = !jobs
            && self.with(|core| {
                core.services
                    .modal_open
                    .as_ref()
                    .is_some_and(|modal_open| modal_open())
            });
        if jobs || dialog_open {
            let shown = self.shown();
            if shown.state != UpdateDialogState::Ready
                || shown.waiting_for_jobs != jobs
                || shown.waiting_for_dialog != dialog_open
            {
                let mut view = self.ready_view(&*host);
                view.waiting_for_jobs = jobs;
                view.waiting_for_dialog = dialog_open;
                self.show(view);
            }
            self.with(|core| core.wait_at = Some(Instant::now() + JOB_POLL));
            return;
        }
        self.with(|core| core.wait_at = None);
        // The window's ordinary close: the Format managers' unapplied edits and the
        // unsaved-changes question are asked as at any other close. Taken out for the call, so
        // that nothing the close does can find the core borrowed.
        let close = self.with(|core| core.services.close_window.take());
        let closed = match close {
            Some(mut close) => {
                let closed = close(&mut *host);
                self.with(|core| core.services.close_window = Some(close));
                closed
            }
            None => false,
        };
        if closed {
            self.with(|core| core.closed_for_apply = true);
            return;
        }
        // Cancel there: the window stays, and so does the release, downloaded.
        if let Some(request) = self.with(|core| core.pending_apply.take()) {
            Self::drop_prepared(host, &request);
        }
        let view = self.ready_view(&*host);
        self.show(view);
        let staged = self.with(|core| core.staged_version.clone());
        host.log(
            &format!(
                "The window stayed open, so Katana {staged} was not installed. It is still \
                 downloaded: Help > Check for Updates installs it."
            ),
            false,
        );
    }

    /// The request UPDATE APPLY CONFIRM returned - once, and only when the close this object asked
    /// the window for succeeded. The program calls it after the window has gone and starts the
    /// helper with it.
    pub fn take_pending_apply(&self) -> Option<ApplyRequest> {
        self.with(|core| {
            if !core.closed_for_apply {
                return None;
            }
            core.closed_for_apply = false;
            core.pending_apply.take()
        })
    }

    // ---- the start ------------------------------------------------------------------------------

    /// What an interactive window does about updates as it starts, once, after it is shown: the
    /// leftovers of an earlier update removed - the helper's copy of one that a recovery put right
    /// among them - the last apply's result shown and cleared, a release already downloaded
    /// offered again, and, once the window has come idle, an update that was never said to start
    /// confirmed and the automatic check when one is due. Nothing in a headless run, and nothing
    /// for a development copy, which shares the per-user update folder with an installed copy and
    /// must leave it alone.
    pub fn start(&self, host: &mut dyn Host) {
        // A headless run reads and writes no per-user place and checks nothing by itself
        // (docs/headless.md).
        if host.headless() || self.with(|core| std::mem::replace(&mut core.started, true)) {
            return;
        }
        let (kind, state_directory, root) = self.context(&*host, |context| {
            (
                context.install.kind,
                context.state_directory.clone(),
                context.install.root.clone(),
            )
        });
        // A development copy is never updated, and it shares the per-user update folder with an
        // installed copy on the same machine: it must not show - and clear - how THAT copy's last
        // update ended, nor sweep what its helper left to be swept. It does nothing here, and asks
        // nobody anything.
        if kind == InstallKind::Development || state_directory.as_os_str().is_empty() {
            return;
        }
        if APPLIES_HERE {
            // What a lock kept an earlier update from deleting (katana_update's state); only where
            // an update can have run at all. A leftover that stays is tried again at the next
            // start: said, not an error of this session.
            let files = make_system_fs_ops(&RetryPolicy::default());
            // First what no list names yet - the helper's copy of an update that a recovery put
            // right (`list_recovered_copies`) - so that the sweep below takes it at this start
            // with the rest.
            let failed = list_recovered_copies(&state_directory, &root, files.as_ref());
            if !failed.is_empty() {
                host.log(
                    &format!(
                        "Update: the helper's copy that a recovered update left behind could not \
                         be listed for removal ({failed}); the next start tries again."
                    ),
                    false,
                );
            }
            if let Err(error) = process_cleanup_list(&state_directory, &root, files.as_ref()) {
                host.log(
                    &format!(
                        "Update: what an earlier update left behind could not all be removed \
                         ({}); the next start tries again.",
                        error.message
                    ),
                    false,
                );
            }
        }
        self.restore_staged(host);
        self.show_last_result(host);
        // On the first idle turn, so that the window is up before anything is asked - and because
        // a window that has come idle is what an update's health run exists to show: that first,
        // then the check.
        self.with(|core| {
            core.due_at = Some(Instant::now() + DUE_PROBE);
            core.turns.push(Turn::FirstIdle);
        });
    }

    /// Settles an update of this install that was installed and never said to start, when this
    /// window is the version it installed; asks again a few times while a live helper holds the
    /// update lock.
    fn confirm_start(&self, host: &mut dyn Host) {
        // An update whose helper died while it waited for the new version to prove that it starts
        // is installed, and nothing says that it works (katana_update's recover, "AN UPDATE THAT
        // IS INSTALLED AND WAS NEVER SAID TO START"). This window has just proved it - if it IS
        // that version, which `update_start_may_confirm` decides. Nothing in a headless run: the
        // confirmation writes per-user state.
        if host.headless() {
            return;
        }
        let (root, state_directory) = self.context(&*host, |context| {
            (
                context.install.root.clone(),
                context.state_directory.clone(),
            )
        });
        if root.as_os_str().is_empty() {
            return;
        }
        // The install's records as they are NOW, not as the context read them when the window was
        // built: a window of the old files that was started while an update was swapping them may
        // have read the old record, which names the version it is. By the time the update lock
        // can be had the record names the new one - a version above that window's own - and it
        // confirms nothing.
        let registry = make_system_registry_ops();
        // Records that do not read prove nothing; the check says why they do not, when it is
        // asked.
        let (Ok(install), Ok(program)) = (read_install(&root, registry.as_ref()), program_version())
        else {
            return;
        };
        let facts = UpdateConfirmFacts {
            applies_here: APPLIES_HERE,
            headless: false,
            keeps_state: !state_directory.as_os_str().is_empty(),
            kind: install.kind,
            has_installed_record: install.has_installed_record,
            // The reader's version: the higher of the helper's record and the build the package
            // names (install.json). In a copy that packaging and updates made it is not this
            // program's only when the helper's record names a version above it - the window of
            // the old files (`update_start_may_confirm`).
            installed: install.version,
            program,
        };
        if !update_start_may_confirm(&facts) {
            return;
        }
        // With nothing to settle - every start but the one after such a death - this is a listing
        // of one folder, and no lock is asked for.
        if confirm_started(&root, &state_directory) != Confirmed::Busy {
            return;
        }
        // A helper is alive: its own health run is still being waited for, and it settles the
        // update itself. Asked again, in case it dies instead.
        self.with(|core| {
            if core.confirm_retries < CONFIRM_RETRIES {
                core.confirm_retries += 1;
                core.confirm_at = Some(Instant::now() + CONFIRM_RETRY);
            }
        });
    }

    /// The automatic check, when every condition for one holds.
    fn check_if_due(&self, host: &mut dyn Host) {
        // A headless run is turned away before anything is made or read for it.
        if host.headless() {
            return;
        }
        let (kind, state_directory) = self.context(&*host, |context| {
            (context.install.kind, context.state_directory.clone())
        });
        // Every condition but the clock's (`update_automatic_check_is_due` names them):
        // development and test trees stay offline, and with no state folder there is no saying
        // when the last check was.
        let facts = UpdateAutoCheckFacts {
            headless: false,
            disabled: automatic_check_disabled(),
            job_running: self.running() != JobKind::None,
            kind,
            keeps_state: !state_directory.as_os_str().is_empty(),
        };
        // A state file that does not read is carried on from as the defaults are (katana_update's
        // state): the check the person never switched off still runs.
        let state = if facts.keeps_state {
            read_state(&state_directory).unwrap_or_default()
        } else {
            UpdateState::default()
        };
        let jitter = self.with(|core| core.jitter);
        if !update_automatic_check_is_due(&facts, &state, now_epoch(), jitter) {
            return;
        }
        self.start_check(host, true);
    }

    fn restore_staged(&self, host: &mut dyn Host) {
        // UPDATE STATUS, asked without a line in the log: nobody typed it. A release downloaded in
        // an earlier session ("Download only") is offered again as ready - but not one the person
        // skipped since, and never one that was rolled back here: the apply keeps the stage of a
        // release that did not start, and "ready" would install it again
        // (`update_staged_to_offer`).
        let (reply, state_directory, installed) = self.context(&*host, |context| {
            (
                run_update_line(context, "UPDATE STATUS"),
                context.state_directory.clone(),
                context.install.version,
            )
        });
        let Some(mut status) = reply.ok().and_then(|reply| reply.status) else {
            return;
        };
        // The skipped version and the blocked list as the engine's own file has them, read here
        // rather than trusted to have been copied whole into the reply: they are what this
        // decision turns on.
        if let Ok(read) = read_state(&state_directory) {
            status.state = read;
        }
        let there = |sequence: u64| stage_is_there(&state_directory, sequence);
        let staged = update_staged_to_offer(&status, &installed, Some(&there));
        if staged.is_empty() {
            return;
        }
        self.with(|core| core.staged_version = staged);
        let view = self.ready_view(&*host);
        self.show(view);
    }

    fn show_last_result(&self, host: &mut dyn Host) {
        let (state_directory, root) = self.context(&*host, |context| {
            (
                context.state_directory.clone(),
                context.install.root.clone(),
            )
        });
        let last = match read_last_result(&state_directory) {
            Err(error) => {
                host.log(
                    &format!(
                        "Update: how the last update ended could not be read ({}).",
                        error.message
                    ),
                    false,
                );
                return;
            }
            Ok(None) => return,
            Ok(Some(last)) => last,
        };
        // Shown once: cleared whatever it says, so the next start is not told again.
        let _ = clear_last_result(&state_directory);

        // What it is shown as is `update_view_of_last_result`'s; a success is no dialog.
        let installed = self.installed_version(&*host);
        let folder = to_native_separators(&text_of(&path_to_utf8(&root)));
        let Some(view) = update_view_of_last_result(&last, &installed, &folder) else {
            host.log(&format!("Katana was updated to {}.", last.version), false);
            return;
        };
        if view.state == UpdateDialogState::Blocked {
            // Nothing was touched and the release is still downloaded: Retry applies it again
            // once the programs named are closed.
            self.with(|core| core.staged_version = view.latest_version.clone());
        }
        let detail = update_detail_text(&view);
        let said = if detail.is_empty() {
            update_state_text(&view)
        } else {
            format!("{} {detail}", update_state_text(&view))
        };
        // In the log as well as in the dialog: an update that did not happen is not told by a
        // window alone. An error only when the install changed and was put back, or could not be.
        host.log(&said, view.state == UpdateDialogState::RolledBack);
        self.show(view);
        self.with(|core| core.wants_dialog = true);
    }

    // ---- Settings > Updates ---------------------------------------------------------------------

    /// What Settings > Updates shows and writes to, as this session has it.
    pub fn settings_updates(&self, host: &dyn Host) -> SettingsUpdates {
        let mut updates = SettingsUpdates::default();
        if host.headless() {
            // No per-user state and no credential store: the page says so, and its lines are
            // answered by the verbs for this surface.
            updates.note = String::from(
                "A headless run keeps no update settings and checks only when it is told to \
                 (UPDATE CHECK).",
            );
            return updates;
        }
        let (state_directory, has_record, sequence, credentials, user_agent, feed_base, kind) =
            self.context(host, |context| {
                (
                    context.state_directory.clone(),
                    context.install.has_installed_record,
                    context.install.sequence,
                    context.credentials,
                    context.feed.user_agent.clone(),
                    context.feed.feed_base.clone(),
                    context.install.kind,
                )
            });
        if !state_directory.as_os_str().is_empty() {
            let folder = state_directory.clone();
            updates.automatic = Some(Rc::new(move || {
                read_state(&folder).map_or(UpdateState::default().automatic, |state| {
                    state.automatic
                })
            }));
            // What each says of a state file that does not read - nothing, so that the page's
            // button stays on and the verb says why it refuses - is `update_skipped_of_state`'s
            // and `update_put_back_of_state`'s.
            let folder = state_directory.clone();
            updates.skipped_version =
                Some(Rc::new(move || update_skipped_of_state(&read_state(&folder))));
            // The releases that were put back here, as the engine's own file has them (UPDATE
            // STATUS does not say them), against the build this install's record names: the list
            // is never shortened but by UPDATE BLOCKED CLEAR, and the page is told only of a
            // build that can still be offered here (`update_put_back_of_state`) - one made after
            // the build installed, or the one the dialog is telling of when the page asks.
            let installed = UpdatePutBackFacts {
                has_installed_record: has_record,
                installed_sequence: sequence,
                dialog_tells_of_one: false,
            };
            let alive: Weak<RefCell<Core>> = Rc::downgrade(&self.core);
            let folder: PathBuf = state_directory;
            updates.put_back = Some(Rc::new(move || {
                let facts = UpdatePutBackFacts {
                    // A core that is borrowed is in the middle of a line, and tells of nothing
                    // yet.
                    dialog_tells_of_one: alive.upgrade().is_some_and(|core| {
                        core.try_borrow()
                            .is_ok_and(|core| update_tells_of_set_aside(&core.view))
                    }),
                    ..installed
                };
                update_put_back_of_state(&read_state(&folder), &facts)
            }));
        }
        updates.credentials = credentials.map(|store| -> Rc<dyn CredentialStore> {
            Rc::new(ProgramStore(store))
        });
        // The context carries the token it was made with: dropped, the next check makes one that
        // reads the store as Save left it.
        let alive: Weak<RefCell<Core>> = Rc::downgrade(&self.core);
        updates.token_changed = Some(Rc::new(move || {
            if let Some(core) = alive.upgrade()
                && let Ok(mut core) = core.try_borrow_mut()
            {
                core.context = None;
            }
        }));
        updates.user_agent = user_agent;
        updates.server = if feed_base.is_empty() {
            String::from("GitHub")
        } else {
            host_of(&feed_base)
        };
        if kind == InstallKind::Development {
            updates.note = String::from(
                "This copy of Katana is a development build: it is never updated, and it runs no \
                 automatic check.",
            );
        } else if automatic_check_disabled() {
            updates.note = format!(
                "The environment variable {DISABLE_VARIABLE} is set: no automatic check runs in \
                 this session."
            );
        }
        updates
    }

    // ---- the frames -----------------------------------------------------------------------------

    /// A frame's turn: what the C++'s timers did. The turns asked for since the last frame, the
    /// times that have come due, the download's progress, and the release notes handed to the
    /// system's browser.
    fn frame(&self, ctx: &egui::Context, host: &mut dyn Host) {
        for turn in self.with(|core| std::mem::take(&mut core.turns)) {
            match turn {
                Turn::ApplyLine => {
                    let _ = host.run(UPDATE_APPLY_LINE);
                }
                Turn::CloseWhenIdle => self.close_when_idle(host),
                Turn::FirstIdle => {
                    self.confirm_start(host);
                    self.check_if_due(host);
                }
            }
        }
        let now = Instant::now();
        let due = |at: &mut Option<Instant>| {
            let is_due = at.is_some_and(|at| at <= now);
            if is_due {
                *at = None;
            }
            is_due
        };
        if self.with(|core| due(&mut core.wait_at)) {
            self.close_when_idle(host);
        }
        if self.with(|core| due(&mut core.confirm_at)) {
            self.confirm_start(host);
        }
        if self.with(|core| due(&mut core.due_at)) {
            self.with(|core| core.due_at = Some(now + DUE_PROBE));
            self.check_if_due(host);
        }
        self.show_progress();
        for url in self.with(|core| std::mem::take(&mut core.notes_to_open)) {
            ctx.open_url(egui::OpenUrl::new_tab(url));
        }
        // A window with nothing to draw is asleep: it is woken for the next time that comes due,
        // and at once for a turn that was asked for meanwhile.
        let (next, turns) = self.with(|core| {
            let next = [core.wait_at, core.confirm_at, core.due_at]
                .into_iter()
                .flatten()
                .min();
            (next, !core.turns.is_empty())
        });
        if turns {
            ctx.request_repaint();
        } else if let Some(next) = next {
            ctx.request_repaint_after(next.saturating_duration_since(Instant::now()));
        }
    }
}

/// The update workbench (see the module comment).
pub struct UpdateWorkbench {
    lines: UpdateLines,
    /// The dialog, made the first time it is asked for.
    dialog: Option<UpdateDialog>,
}

impl UpdateWorkbench {
    pub fn new(services: UpdateServices) -> Self {
        Self {
            lines: UpdateLines::new(services),
            dialog: None,
        }
    }

    /// The UPDATE lines' runner, for the window's dispatcher to hold beside the workbench.
    pub fn lines(&self) -> UpdateLines {
        self.lines.clone()
    }

    /// The Help item: it looks and reads like every other, and names the dialog it opens.
    pub fn action(&self) -> Action {
        Action::new(ACTION, "Check for &Updates...")
            .icon("Download")
            .tip(
                "Ask whether a newer Katana has been released; download and install it from the \
                 dialog that opens",
            )
            .opens(DIALOG)
    }

    /// `UpdateLines::run_line`.
    pub fn run_line(&self, host: &mut dyn Host, line: &str) -> bool {
        self.lines.run_line(host, line)
    }

    /// `UpdateLines::start`, and the dialog shown when how the last update ended is to be said.
    pub fn start(&mut self, host: &mut dyn Host) {
        self.lines.start(host);
        self.show_if_wanted();
    }

    fn show_if_wanted(&mut self) {
        if self.lines.with(|core| std::mem::take(&mut core.wants_dialog)) {
            self.dialog().show();
        }
    }

    /// The dialog, made the first time it is asked for: its lines through the window's one
    /// executor, its callbacks this workbench's.
    pub fn dialog(&mut self) -> &mut UpdateDialog {
        let lines = &self.lines;
        self.dialog.get_or_insert_with(|| {
            let update = lines.clone();
            let cancel = lines.clone();
            let notes = lines.clone();
            let later = lines.clone();
            let context = UpdateDialogContext {
                run: Some(run_through_host()),
                update: Some(Box::new(move |host: &mut dyn Host| {
                    update.install_update(host);
                })),
                cancel: Some(Box::new(move |host: &mut dyn Host| cancel.cancel_job(host))),
                open_notes: Some(Box::new(move |host: &mut dyn Host, url: &str| {
                    notes.open_notes(host, url);
                })),
                later: Some(Box::new(move |host: &mut dyn Host| later.later(host))),
            };
            let mut dialog = UpdateDialog::new(context);
            lines.with(|core| {
                dialog.set_view(core.view.clone());
                core.post = Some(dialog.views());
            });
            dialog
        })
    }

    /// The dialog if it has been made.
    pub fn made_dialog(&mut self) -> Option<&mut UpdateDialog> {
        self.dialog.as_mut()
    }

    /// Shows the dialog; with nothing known yet it also says so, or asks: a development copy is
    /// told it cannot update, an interactive window runs UPDATE CHECK, a headless one waits to be
    /// asked.
    pub fn open(&mut self, host: &mut dyn Host) {
        let _ = self.dialog();
        let lines = self.lines.clone();
        // How the last update ended is shown once (`show_last_result`, which shows the dialog
        // itself): asked for again, the dialog starts over, or it would say "could not be
        // installed" for the rest of the session with nothing to press.
        if lines.running() == JobKind::None && lines.shown().state == UpdateDialogState::RolledBack
        {
            let view = lines.view_of(&*host, UpdateDialogState::Idle);
            lines.show(view);
        }
        // With nothing known yet, and nothing on its way - or only the check the window started
        // by itself, which the typed line below then takes over (`start_check`) rather than say
        // "not checked yet" while one is running.
        let (running, quiet) = lines.with(|core| (core.running, core.automatic_check));
        let quiet_check = running == JobKind::Check && quiet;
        if (running == JobKind::None || quiet_check)
            && lines.shown().state == UpdateDialogState::Idle
        {
            let kind = lines.context(&*host, |context| context.install.kind);
            if kind == InstallKind::Development {
                // Known without asking anyone: a copy packaging did not write is never updated
                // (katana_app's update_verbs), and the check would say this.
                let mut view = lines.view_of(&*host, UpdateDialogState::Unsupported);
                view.reason = String::from("development_tree");
                lines.show(view);
            } else if host.headless() {
                // A headless run checks only when it is told to: the dialog says what copy this
                // is and waits for Check now.
                let view = lines.view_of(&*host, UpdateDialogState::Idle);
                lines.show(view);
            } else {
                // What the item's name says it does, as the typed line would.
                let _ = host.run(UPDATE_CHECK_LINE);
            }
        }
        self.dialog().show();
    }

    /// `UpdateLines::settings_updates`.
    pub fn settings_updates(&self, host: &dyn Host) -> SettingsUpdates {
        self.lines.settings_updates(host)
    }

    /// `UpdateLines::take_pending_apply`.
    pub fn take_pending_apply(&self) -> Option<ApplyRequest> {
        self.lines.take_pending_apply()
    }

    /// The status bar's button (statusUpdateAvailable): "Update 0.3.0" beside the download icon,
    /// hidden until a release is known; a click opens the dialog. The window draws it in its
    /// status bar.
    pub fn status_button_ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        let (text, shown) = self
            .lines
            .with(|core| (core.status_text.clone(), core.status_shown));
        let mut clicked = false;
        let mut draw = |ui: &mut Ui| {
            ui.horizontal(|ui| {
                let side = ui.spacing().interact_size.y.min(16.0);
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
                show_icon(ui.painter(), rect, "Download", IconMode::Normal);
                clicked = controls::button_as(
                    ui,
                    STATUS_BUTTON,
                    &text,
                    egui::Button::new(text.as_str()).frame(false),
                )
                .on_hover_text("A newer Katana is available: click to see it")
                .clicked();
            });
        };
        if shown {
            draw(ui);
        } else {
            hidden(ui, STATUS_BUTTON, draw);
        }
        if clicked {
            self.open(host);
        }
    }
}

impl Drop for UpdateWorkbench {
    fn drop(&mut self) {
        // A request that was prepared and is still here was never started: the person closed the
        // window themselves while the close for it waited, and the program was handed nothing. The
        // helper's copy goes to the next start's sweep, as after Later - without a word, since the
        // log is the window's and the window is going. A job still running is stopped by the
        // window's runner as it goes, and its end is dropped with the runner.
        if let Ok(mut core) = self.lines.core.try_borrow_mut()
            && let Some(request) = core.pending_apply.take()
        {
            let _ = abandon_apply(&request);
        }
    }
}

impl Workbench for UpdateWorkbench {
    /// Help > "Check for Updates...", added to the Help menu after what is already there.
    fn menus(&self) -> Vec<Menu> {
        vec![Menu::new("helpMenu", "&Help").action(self.action())]
    }

    fn tool_bars(&self) -> Vec<ToolBar> {
        Vec::new()
    }

    fn docks(&self) -> Vec<DockSpec> {
        Vec::new()
    }

    fn run_line(&mut self, host: &mut dyn Host, line: &str) -> bool {
        self.lines.run_line(host, line)
    }

    fn trigger(&mut self, host: &mut dyn Host, name: &str) -> bool {
        if name != ACTION && name != STATUS_BUTTON {
            return false;
        }
        self.open(host);
        true
    }

    fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        self.lines.frame(ui.ctx(), host);
        self.show_if_wanted();
        if let Some(dialog) = &mut self.dialog {
            dialog.ui(ui, host);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{host_of, is_https_address, problem_sentence, reply_text};

    // QUrl::host of a feed's base address: the authority's host alone, without the user, the port
    // or the path.
    #[test]
    fn the_host_of_a_feed_address_is_its_authoritys_host_alone() {
        assert_eq!(
            host_of("https://releases.example.org/katana"),
            "releases.example.org"
        );
        assert_eq!(host_of("https://Example.ORG:8443/x?y#z"), "example.org");
        assert_eq!(host_of("https://user@example.org"), "example.org");
        assert_eq!(host_of("https://[::1]:8080/feed"), "::1");
        assert_eq!(host_of("releases.example.org"), "");
    }

    // Only an https address is handed to the system to open (update_workbench.cpp, openNotes).
    #[test]
    fn only_an_https_address_is_one_to_open() {
        assert!(is_https_address("https://example.org/releases/tag/v0.3.0"));
        assert!(is_https_address("HTTPS://example.org"));
        assert!(!is_https_address("http://example.org/releases"));
        assert!(!is_https_address("file:///C:/Windows/notepad.exe"));
        assert!(!is_https_address("https://"));
        assert!(!is_https_address("https://example.org/a b"));
        assert!(!is_https_address(""));
    }

    // The sentence under the dialog's two (update_workbench.cpp, problemSentence), and a reply as
    // the log shows it (replyText).
    #[test]
    fn a_problem_is_said_with_its_reason_or_sent_to_the_log() {
        assert_eq!(
            problem_sentence("The download did not finish", "it was cancelled"),
            "The download did not finish: it was cancelled."
        );
        assert_eq!(
            problem_sentence("The update could not be started", ""),
            "The update could not be started; the command log says why."
        );
        assert_eq!(reply_text("staged=0.3.0\r\n\n"), "staged=0.3.0");
    }
}
