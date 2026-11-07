//! Per-user update state, in the state directory U, and nothing else: the install's own records
//! are install.rs's.
//!
//! ```text
//! U = %LOCALAPPDATA%\Katana\update          (Windows)
//!     $XDG_STATE_HOME/katana/update         (else ~/.local/state/katana/update)
//! KATANA_UPDATE_STATE_DIR, when set, is U instead - how tests and a person
//! with a roaming profile keep it elsewhere. It moves no trust (nothing in U
//! is believed without a signature check) and is read only here.
//!
//! U/state.json      {"schema":1,"last_check":<epoch seconds>,"auto":true,
//!                    "skipped":"0.3.0","blocked":[<sequence>,...]}
//! U/last.json       {"schema":1,"status":"rolled_back","reason":"...",
//!                    "holder":"katana_mcp.exe (4412)","version":"0.3.0","tx":"..."}
//!                   what the last apply ended as, shown once at the next start and then
//!                   cleared
//! U/stage-<seq>/    staged files, one per changed manifest file, at their root-relative paths
//! U/run-<tx>/       the preflighted helper copy (and its self-test output)
//! U/healthy-<tx>    the marker the new version writes after its first idle turn
//! U/cleanup.list    what the next start of the window deletes: leftovers a locked file kept
//!                   from being deleted, and helper copies (U/run-<tx>) that nobody will run
//!                   again - the one a helper ran from, which it cannot delete itself, and
//!                   those the verbs and the window list for an apply that was prepared and
//!                   never started (append_cleanup, process_cleanup_list below)
//! U/cleanup.list.work  the list while a run works on it: process_cleanup_list renames the list
//!                   to this name (atomic, so a helper that appends meanwhile starts a new
//!                   list), acts on it, appends what must stay to the list and removes it. A
//!                   run that died leaves it, and the next start finishes it first.
//! ```
//!
//! Headless runs read and write no per-user place (docs/headless.md): the callers that are
//! headless do not call these.
//!
//! state.json and last.json are written as nlohmann::json's `dump(2)` writes them in the C++ (the
//! window and the helper share them, so either program reads what the other wrote): the members
//! sorted by key, as nlohmann's std::map holds them, two spaces of indent, `": "` between a name
//! and its value, and a line feed at the end. The records hold integers, strings, a flag and a
//! list of integers, no real number, so serde_json's pretty printer gives that text once the
//! members are put in sorted order: it escapes exactly what nlohmann escapes with ensure_ascii
//! off (the quotation mark, the backslash, \b \t \n \f \r, and every other byte below 0x20 as
//! \u00xx in lower-case hex; never '/', DEL or a byte beyond ASCII), writes an empty list as `[]`,
//! and integers in plain decimal.

use crate::fsops::{EntryKind, FsOps, path_text};
use crate::install::{bare, update_work_directory};
use crate::json::Json;
use crate::records::{
    MAX_RECORD_BYTES, MAX_STATE_BYTES, parse_failure, parse_record, read_optional_text,
    string_member,
};
use katana_core::error::make_error;
use katana_core::path_text::{environment_variable, path_from_utf8, path_to_utf8};
use katana_core::{Error, ErrorCode, Result, Status};
use std::fs::File;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::{ErrorKind, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateState {
    /// 0: never.
    pub last_check_epoch: i64,
    /// "Check for updates automatically" (default on).
    pub automatic: bool,
    /// "UPDATE SKIP <version>": that version is not offered.
    pub skipped_version: String,
    /// Releases whose health check failed here, by sequence: not offered and not applied again
    /// until the person empties the list ("UPDATE BLOCKED CLEAR").
    pub blocked_sequences: Vec<u64>,
}

impl Default for UpdateState {
    fn default() -> Self {
        Self {
            last_check_epoch: 0,
            automatic: true,
            skipped_version: String::new(),
            blocked_sequences: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LastResult {
    /// "committed" | "rolled_back" | "blocked" | "refused" | "rollback_incomplete" (the apply's
    /// outcome as text). The last is the half-swapped tree: the next start names
    /// `katana_updater --recover <R>` and runs it.
    pub status: String,
    /// Machine-readable token.
    pub reason: String,
    /// "katana_mcp.exe (4412)" for blocked; else empty.
    pub holder: String,
    /// The version the apply was for.
    pub version: String,
    pub transaction: String,
}

const CLEANUP_LIST_NAME: &str = "cleanup.list";
// The list a run is working on: cleanup.list renamed. The list is never read and rewritten in
// place, because the helper appends to it (append_cleanup) while the window may be processing it,
// and an append between the read and the rewrite would be lost with the old text. A rename is
// atomic and moves the file away from the appenders: from then on they start a new list, and what
// this run keeps is appended to that one.
const CLEANUP_WORK_NAME: &str = "cleanup.list.work";
// A list of paths, a line each: at about 200 bytes a line this is some five thousand leftovers,
// where a healthy install has a handful.
const MAX_CLEANUP_LIST_BYTES: u64 = 1024 * 1024;
// A folder is removed by walking it; one nested deeper than anything the updater creates (a
// transaction folder is about six levels) is not walked.
const MAX_CLEANUP_DEPTH: i32 = 64;

// ---- writing a record whole or not at all ------------------------------------------------------

struct Replaced {
    done: bool,
    /// The cause may pass: another program has the record open.
    worth_retrying: bool,
    why: String,
}

// `from` renamed over `to`, and the rename itself on the disk before this returns: the data was
// flushed already, but a rename that a power failure can undo would take last.json's note that an
// update was rolled back with it. Windows: MoveFileExW with MOVEFILE_WRITE_THROUGH, so that the
// reason is also the system's own error number and the cases worth waiting for can be told from
// the rest; elsewhere the folder is fsynced after the rename, and a rename that fails fails for
// good.
#[cfg(not(windows))]
fn replace_file(from: &Path, to: &Path) -> Replaced {
    if let Err(error) = std::fs::rename(from, to) {
        return Replaced {
            done: false,
            worth_retrying: false,
            why: format!("{error}"),
        };
    }
    // Best effort: the record is in place whatever this says, and a file system that cannot
    // fsync a folder (some network ones) is no reason to call the write failed.
    if let Some(folder) = to.parent()
        && let Ok(folder) = File::open(folder)
    {
        let _ = folder.sync_all();
    }
    Replaced {
        done: true,
        worth_retrying: false,
        why: String::new(),
    }
}

#[cfg(windows)]
fn replace_file(from: &Path, to: &Path) -> Replaced {
    windows::replace_file(from, to)
}

// `text` written to a new file `path` and pushed to stable storage before it is closed
// (FlushFileBuffers; fsync). Every step is taken whatever the one before it said, as the C++'s
// fwrite, fflush and _commit or fsync are.
fn write_and_flush(path: &Path, text: &[u8]) -> bool {
    let Ok(mut file) = File::create(path) else {
        return false;
    };
    let written = file.write_all(text).is_ok();
    let flushed = file.sync_all().is_ok();
    written && flushed
}

// A number for the temporary file's name that no other writer of the same record is using at the
// same moment. The C++ mixes the steady clock's count, a counter and the thread; Rust's monotonic
// clock has no count to read, so the wall clock's nanoseconds stand in for it: the number only
// has to differ, not to mean anything.
fn unique_number() -> u64 {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let clock = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos() as u64);
    let mut thread = DefaultHasher::new();
    std::thread::current().id().hash(&mut thread);
    clock ^ (COUNTER.fetch_add(1, Ordering::Relaxed) << 48) ^ thread.finish()
}

// A reader that has the old record open, or a virus scanner that has just seen the new one, can
// make the replacement fail for a while on Windows (sharing violation, lock violation, access
// denied). last.json is the only way the person learns that an update was rolled back, so it is
// worth waiting for: the tries back off from 2 ms to 100 ms and stop after about two seconds.
const REPLACE_FOR: Duration = Duration::from_secs(2);
const LONGEST_PAUSE: Duration = Duration::from_millis(100);
const FIRST_PAUSE: Duration = Duration::from_millis(2);

// The text goes to a file of its own beside the record, is flushed to the disk, and is renamed
// over it: a reader sees the old record or the new one, never half of either, and a crash or a
// full disk leaves the old one in place. The flush comes BEFORE the rename: a rename that reaches
// the disk ahead of the data it names leaves a record of zero length, and last.json is the one
// place a person learns that an update was rolled back. (The swap's own files go through the
// FsOps seam; these two records are written here, with no FsOps to hand, so the fault loops do
// not see them.) A record that is unreadable anyway is ParseFailure to the reader, which carries
// on with the defaults and says so.
fn write_record(directory: &Path, name: &str, text: &[u8]) -> Status {
    if let Err(error) = std::fs::create_dir_all(directory) {
        return Err(make_error(
            ErrorCode::FileExportFailure,
            "the update state folder cannot be created",
            format!("{error}; {}", path_text(directory)),
        ));
    }

    let temporary = directory.join(format!("{name}.{}.tmp", unique_number()));
    let target = directory.join(name);

    if !write_and_flush(&temporary, text) {
        let _ = std::fs::remove_file(&temporary);
        return Err(make_error(
            ErrorCode::FileExportFailure,
            "the record cannot be written",
            path_text(&temporary),
        ));
    }

    // A failure that waiting cannot cure is reported at once.
    let give_up = Instant::now() + REPLACE_FOR;
    let mut pause = FIRST_PAUSE;
    let why = loop {
        let replaced = replace_file(&temporary, &target);
        if replaced.done {
            return Ok(());
        }
        if !replaced.worth_retrying || Instant::now() + pause > give_up {
            break replaced.why;
        }
        std::thread::sleep(pause);
        pause = (pause * 2).min(LONGEST_PAUSE);
    };
    let _ = std::fs::remove_file(&temporary);
    Err(make_error(
        ErrorCode::FileExportFailure,
        "the record cannot be replaced",
        format!("{why}; {}", path_text(&target)),
    ))
}

// state_directory() is an empty path on a machine with no home to put the state in, and an empty
// path joined with a name is a name in the WORKING folder: wherever the program was started from.
// Every function below that takes the state folder refuses an empty one, so that state.json,
// last.json and cleanup.list are never read, written or acted on there.
fn require_state_directory(state_dir: &Path) -> Status {
    if state_dir.as_os_str().is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "there is no update state folder (no home folder to put it in)",
        ));
    }
    Ok(())
}

// A record's text as nlohmann::json's dump(2) writes it, and a line feed (the module's comment).
// nlohmann keeps an object's members in a std::map, sorted by key whatever order they were set
// in, so they are sorted here before serde_json writes them in the order it is given (its map
// keeps the order of insertion once any crate of the workspace asks for preserve_order). The
// C++'s error handler `replace` writes U+FFFD for a byte that is not UTF-8; a Rust string holds
// none.
fn dump(mut members: Vec<(&'static str, serde_json::Value)>) -> Result<Vec<u8>> {
    members.sort_unstable_by_key(|member| member.0);
    let mut object = serde_json::Map::new();
    for (name, value) in members {
        object.insert(String::from(name), value);
    }
    let mut text =
        serde_json::to_vec_pretty(&serde_json::Value::Object(object)).map_err(|error| {
            make_error(
                ErrorCode::FileExportFailure,
                "the record cannot be written",
                format!("{error}"),
            )
        })?;
    text.push(b'\n');
    Ok(text)
}

// ---- the state folder --------------------------------------------------------------------------

/// U, as above. Not created.
pub fn state_directory() -> PathBuf {
    let chosen = environment_variable(Some("KATANA_UPDATE_STATE_DIR"));
    if !chosen.is_empty() {
        // Absolute now, so that a later change of the working folder cannot move it.
        let given = path_from_utf8(&chosen);
        return std::path::absolute(&given).unwrap_or(given);
    }
    #[cfg(windows)]
    {
        let local_app_data = environment_variable(Some("LOCALAPPDATA"));
        if !local_app_data.is_empty() {
            return path_from_utf8(&local_app_data)
                .join("Katana")
                .join("update");
        }
        let profile = environment_variable(Some("USERPROFILE"));
        if profile.is_empty() {
            return PathBuf::new();
        }
        path_from_utf8(&profile)
            .join("AppData")
            .join("Local")
            .join("Katana")
            .join("update")
    }
    #[cfg(not(windows))]
    {
        // The XDG base directory specification: a value that is not an absolute path is invalid
        // and must be ignored.
        let xdg = environment_variable(Some("XDG_STATE_HOME"));
        if !xdg.is_empty() {
            let base = path_from_utf8(&xdg);
            if base.is_absolute() {
                return base.join("katana").join("update");
            }
        }
        let home = environment_variable(Some("HOME"));
        if home.is_empty() {
            return PathBuf::new();
        }
        path_from_utf8(&home)
            .join(".local")
            .join("state")
            .join("katana")
            .join("update")
    }
}

// The three names below take their identifiers from the caller: `transaction` has been checked as
// a transaction id where it came from a command line or a folder name, and a sequence is a number.

/// U/stage-<sequence>.
pub fn stage_path(state_dir: &Path, sequence: u64) -> PathBuf {
    state_dir.join(format!("stage-{sequence}"))
}

/// U/run-<transaction>.
pub fn run_path(state_dir: &Path, transaction: &str) -> PathBuf {
    state_dir.join(format!("run-{transaction}"))
}

/// U/healthy-<transaction>.
pub fn health_marker_path(state_dir: &Path, transaction: &str) -> PathBuf {
    state_dir.join(format!("healthy-{transaction}"))
}

// ---- state.json --------------------------------------------------------------------------------

/// state.json: absent is the defaults (not an error); unreadable or from a newer schema is
/// ParseFailure / Unsupported, and the caller carries on with defaults and says so.
pub fn read_state(state_dir: &Path) -> Result<UpdateState> {
    require_state_directory(state_dir)?;
    let file = state_dir.join("state.json");
    let mut state = UpdateState::default();
    let Some(text) = read_optional_text(&file, MAX_STATE_BYTES)? else {
        return Ok(state);
    };
    let record = parse_record(&text, &file, 1)?;

    if let Some(member) = record.get("last_check") {
        // A whole number, signed or not, that fits 64 bits with a sign.
        let seconds = match member {
            Json::Integer(seconds) => Some(*seconds),
            Json::Unsigned(seconds) => i64::try_from(*seconds).ok(),
            _ => None,
        };
        let Some(seconds) = seconds else {
            return Err(parse_failure(
                "\"last_check\" is not a whole number of seconds",
                &file,
            ));
        };
        state.last_check_epoch = seconds;
    }
    if let Some(member) = record.get("auto") {
        let Some(automatic) = member.as_bool() else {
            return Err(parse_failure("\"auto\" is not true or false", &file));
        };
        state.automatic = automatic;
    }
    state.skipped_version = string_member(&record, "skipped", false, &file, "")?;
    if let Some(member) = record.get("blocked") {
        let Some(entries) = member.as_array() else {
            return Err(parse_failure("\"blocked\" is not a list", &file));
        };
        for entry in entries {
            let Some(sequence) = entry.as_unsigned() else {
                return Err(parse_failure(
                    "\"blocked\" holds something that is not a release sequence",
                    &file,
                ));
            };
            state.blocked_sequences.push(sequence);
        }
    }
    Ok(state)
}

/// Written through a temporary file and a rename.
pub fn write_state(state_dir: &Path, state: &UpdateState) -> Status {
    require_state_directory(state_dir)?;
    let text = dump(vec![
        ("schema", serde_json::Value::from(1)),
        (
            "last_check",
            serde_json::Value::from(state.last_check_epoch),
        ),
        ("auto", serde_json::Value::Bool(state.automatic)),
        (
            "skipped",
            serde_json::Value::String(state.skipped_version.clone()),
        ),
        (
            "blocked",
            serde_json::Value::Array(
                state
                    .blocked_sequences
                    .iter()
                    .map(|&sequence| serde_json::Value::from(sequence))
                    .collect(),
            ),
        ),
    ])?;
    write_record(state_dir, "state.json", &text)
}

// ---- last.json ---------------------------------------------------------------------------------

/// last.json: None when there is none.
pub fn read_last_result(state_dir: &Path) -> Result<Option<LastResult>> {
    require_state_directory(state_dir)?;
    let file = state_dir.join("last.json");
    let Some(text) = read_optional_text(&file, MAX_RECORD_BYTES)? else {
        return Ok(None);
    };
    let record = parse_record(&text, &file, 1)?;
    let mut result = LastResult::default();
    for (name, required, into) in [
        ("status", true, &mut result.status),
        ("reason", false, &mut result.reason),
        ("holder", false, &mut result.holder),
        ("version", false, &mut result.version),
        ("tx", false, &mut result.transaction),
    ] {
        *into = string_member(&record, name, required, &file, "")?;
    }
    if result.status.is_empty() {
        return Err(parse_failure("\"status\" is empty", &file));
    }
    Ok(Some(result))
}

pub fn write_last_result(state_dir: &Path, result: &LastResult) -> Status {
    require_state_directory(state_dir)?;
    let text = dump(vec![
        ("schema", serde_json::Value::from(1)),
        ("status", serde_json::Value::String(result.status.clone())),
        ("reason", serde_json::Value::String(result.reason.clone())),
        ("holder", serde_json::Value::String(result.holder.clone())),
        ("version", serde_json::Value::String(result.version.clone())),
        ("tx", serde_json::Value::String(result.transaction.clone())),
    ])?;
    write_record(state_dir, "last.json", &text)
}

pub fn clear_last_result(state_dir: &Path) -> Status {
    require_state_directory(state_dir)?;
    let file = state_dir.join("last.json");
    // std::filesystem::remove: a file, or a folder that is empty; nothing there is no error.
    let removed = match std::fs::symlink_metadata(&file) {
        Ok(metadata) if metadata.is_dir() => std::fs::remove_dir(&file),
        _ => std::fs::remove_file(&file),
    };
    match removed {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(make_error(
            ErrorCode::FileExportFailure,
            "last.json cannot be removed",
            format!("{error}; {}", path_text(&file)),
        )),
    }
}

// ---- the automatic check's clock ---------------------------------------------------------------

/// An automatic check is due every 24 hours plus a jitter of 0 to 2 hours, so that a release does
/// not meet every copy at the same minute. In seconds, as the C++'s std::chrono::seconds count
/// them: the jitter may be handed in negative, and is then clamped.
pub const CHECK_INTERVAL: i64 = 24 * 3600;
/// The jitter is drawn ONCE per process by the caller (uniformly in [0, CHECK_JITTER_MAX]) and
/// passed in, which keeps check_is_due a function of its arguments.
pub const CHECK_JITTER_MAX: i64 = 2 * 3600;

/// Whether the window's automatic check is due at `now_epoch` (UTC seconds): never when the person
/// turned the check off (`automatic` false); always when there was none yet (last_check_epoch
/// <= 0), and when the last one lies in the FUTURE (the clock was set back: a stored time that
/// cannot be right must not silence the check until the real time catches up); otherwise when
/// now_epoch - last_check_epoch >= CHECK_INTERVAL + jitter, `jitter` (seconds) being clamped to
/// [0, CHECK_JITTER_MAX]. A typed `UPDATE CHECK` does not ask: it checks. In the C++ it is one of
/// the finished functions of contract.cpp, with its tests in tests/update/test_contract.cpp.
pub fn check_is_due(state: &UpdateState, now_epoch: i64, jitter: i64) -> bool {
    if !state.automatic {
        return false;
    }
    if state.last_check_epoch <= 0 || state.last_check_epoch > now_epoch {
        return true;
    }
    let clamped = jitter.clamp(0, CHECK_JITTER_MAX);
    let wait = CHECK_INTERVAL + clamped;
    now_epoch - state.last_check_epoch >= wait
}

// ---- U/cleanup.list ----------------------------------------------------------------------------

/// Appends `leftover` (an absolute path) to U/cleanup.list, one path per line, UTF-8. Two kinds of
/// thing are listed. What a held lock kept the helper from deleting: the helper copy it runs from,
/// a locked file under R/.update/<tx>. And a helper copy that will never be started: the
/// U/run-<tx> of an apply that was prepared and then not launched (katana_cli's session, and the
/// window when its restart is taken back), and the one a recovery left behind, which the window
/// lists at its start. Written through `fs` so the fault loops see it.
pub fn append_cleanup(state_dir: &Path, leftover: &Path, fs: &dyn FsOps) -> Status {
    require_state_directory(state_dir)?;
    let mut text = path_to_utf8(leftover);
    // One path to a line: a line break inside a name would turn one leftover into two entries,
    // the second of them anywhere. (A relative path is refused for the same reason it is dropped
    // when read: it means something else in another working folder.)
    if !leftover.is_absolute() || text.contains(&b'\r') || text.contains(&b'\n') {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "a leftover is listed by its absolute path, on one line",
            path_text(leftover),
        ));
    }
    fs.make_directories(state_dir)?;
    text.push(b'\n');
    fs.append_durable(&state_dir.join(CLEANUP_LIST_NAME), &text)
}

// Two names or two whole paths (bare()), compared as the file system compares them. Windows folds
// case; an ASCII fold is what is done here, and a name whose case differs in a letter beyond ASCII
// is read as another name, which only ever makes the check refuse more.
#[cfg(windows)]
fn same_name(a: &Path, b: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    fn fold(unit: u16) -> u16 {
        if unit < 0x80 {
            u16::from((unit as u8).to_ascii_lowercase())
        } else {
            unit
        }
    }
    let left: Vec<u16> = a.as_os_str().encode_wide().collect();
    let right: Vec<u16> = b.as_os_str().encode_wide().collect();
    left.len() == right.len() && left.iter().zip(&right).all(|(&x, &y)| fold(x) == fold(y))
}

#[cfg(not(windows))]
fn same_name(a: &Path, b: &Path) -> bool {
    a == b
}

// How many leading names `base` has, when `child` has all of them and at least one more; None
// otherwise. Both are bare().
fn names_below(child: &Path, base: &Path) -> Option<usize> {
    if base.as_os_str().is_empty() {
        return None; // no place at all is not every place
    }
    let mut child_names = child.components();
    let mut shared = 0usize;
    for name in base.components() {
        match child_names.next() {
            Some(other) if same_name(Path::new(other.as_os_str()), Path::new(name.as_os_str())) => {
                shared += 1;
            }
            _ => return None,
        }
    }
    child_names.next()?; // equal to the base is not below it
    Some(shared)
}

// Whether a name below the first `from` of `path` is one that Windows does not read as it is
// spelled: it drops the dots and spaces at the end of a name, so "..." is the folder it is in and
// ".. " may be the one above. Such a line would pass the check that it lies under the base and
// then act on the base itself, or on what is above it. The updater never makes such a name, so
// the line is not one it wrote.
#[cfg(windows)]
fn names_something_else_than_it_says(path: &Path, from: usize) -> bool {
    use std::os::windows::ffi::OsStrExt;
    path.components().skip(from).any(|part| {
        let name: Vec<u16> = part.as_os_str().encode_wide().collect();
        matches!(name.last(), Some(&unit) if unit == u16::from(b'.') || unit == u16::from(b' '))
    })
}

// Every other system reads a name as it is spelled.
#[cfg(not(windows))]
fn names_something_else_than_it_says(_path: &Path, _from: usize) -> bool {
    false
}

// What a line names, removed through `fs`. Not the crate's remove_tree (remove_tree.rs), which
// carries on past what it cannot delete and reports it as leftovers: a line is kept on the list
// for the next start exactly when something under it could not go, so here the first refusal is
// the answer.
fn remove_tree(fs: &dyn FsOps, path: &Path, depth: i32) -> Status {
    let info = fs.stat(path)?;
    if info.kind == EntryKind::Missing {
        return Ok(());
    }
    if info.reparse_point {
        // The link itself goes; what it points at is not touched, and never walked.
        return if info.kind == EntryKind::Directory {
            fs.remove_empty_directory(path)
        } else {
            fs.remove_file(path)
        };
    }
    if info.kind != EntryKind::Directory {
        return fs.remove_file(path);
    }
    if depth >= MAX_CLEANUP_DEPTH {
        return Err(make_error(
            ErrorCode::InvalidState,
            "a folder nested too deeply to clean up",
            path_text(path),
        ));
    }
    let children = fs.list_directory(path)?;
    // Everything that can go goes; the first refusal is the one reported.
    let mut first: Option<Error> = None;
    for child in &children {
        if let Err(error) = remove_tree(fs, child, depth + 1) {
            first.get_or_insert(error);
        }
    }
    if let Some(first) = first {
        return Err(first);
    }
    fs.remove_empty_directory(path)
}

struct CleanupBase {
    /// As the caller spelled it.
    lexical: PathBuf,
    /// With every link resolved; None when that failed.
    resolved: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LineOutcome {
    Done,
    Retry,
}

// One line of the list. Done: removed, or nothing to remove, or not a place this list may name
// (never touched). Retry: it is ours and could not be removed (a locked file), or it is under a
// place that could not be examined just now: keep it.
fn clean_one(
    fs: &dyn FsOps,
    line: &[u8],
    bases: &[CleanupBase],
    list_files: &[&Path],
) -> LineOutcome {
    let given = path_from_utf8(line);
    if !given.is_absolute() {
        return LineOutcome::Done;
    }
    let path = bare(&given);
    for list_file in list_files {
        if same_name(&path, &bare(list_file)) {
            return LineOutcome::Done; // the list does not delete itself
        }
    }

    for base in bases {
        let Some(shared) = names_below(&path, &base.lexical) else {
            continue;
        };
        if names_something_else_than_it_says(&path, shared) {
            return LineOutcome::Done; // not a line this program wrote: never touched
        }
        let Some(resolved) = &base.resolved else {
            // A place that could not be resolved (access denied, a sharing violation: a missing
            // place resolves fine) is not a place without leftovers. The line is by its text one
            // of ours, so it is kept for the next start, not dropped.
            return LineOutcome::Retry;
        };
        // Each folder between the base and the path, itself excluded, must be a real folder:
        // through a link the path would name something that is not under the base at all.
        let names: Vec<Component<'_>> = path.components().collect();
        let mut walk: PathBuf = names[..shared].iter().collect();
        for name in &names[shared..names.len() - 1] {
            walk.push(name);
            match fs.stat(&walk) {
                Err(_) => return LineOutcome::Retry,
                // Missing: nothing there to remove. A link: never followed.
                Ok(info) if info.kind == EntryKind::Missing || info.reparse_point => {
                    return LineOutcome::Done;
                }
                Ok(_) => {}
            }
        }
        // And the folder it sits in, with links resolved, must be the base or below it.
        let Ok(parent) = fs.resolve(path.parent().unwrap_or(Path::new(""))) else {
            return LineOutcome::Retry;
        };
        if !same_name(&parent, resolved) && names_below(&bare(&parent), resolved).is_none() {
            return LineOutcome::Done;
        }
        return match remove_tree(fs, &path, 0) {
            Ok(()) => LineOutcome::Done,
            Err(_) => LineOutcome::Retry,
        };
    }
    LineOutcome::Done // not under any place the list may name
}

// Acts on the lines of `work` and removes it, first appending the lines that must stay to `list`.
// Nothing is dropped unless it was acted on: a crash, a full disk or a refusal anywhere leaves
// `work` in place for the next start, and a line in both is acted on twice, which is harmless.
fn clean_taken(state_dir: &Path, root: &Path, fs: &dyn FsOps, list: &Path, work: &Path) -> Status {
    let info = fs.stat(work)?;
    if info.kind == EntryKind::Missing {
        return Ok(()); // another process finished it
    }
    if info.kind != EntryKind::File {
        return Err(make_error(
            ErrorCode::InvalidState,
            format!("{CLEANUP_WORK_NAME} is not a file"),
            path_text(work),
        ));
    }
    if info.size > MAX_CLEANUP_LIST_BYTES {
        // Not a list this program wrote. Dropped whole rather than walked line by line.
        fs.remove_file(work)?;
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "cleanup.list was too long to be one this program wrote; it was discarded",
            path_text(list),
        ));
    }
    let text = fs.read_file(work)?;

    // The places a line may name: U, and R/.update. A place that cannot be resolved (it is not
    // "missing": that resolves; it is an error from the system, which may pass) is kept in the
    // list, and every line under it with it.
    let mut candidates = vec![state_dir.to_path_buf()];
    if !root.as_os_str().is_empty() {
        candidates.push(update_work_directory(root));
    }
    let bases: Vec<CleanupBase> = candidates
        .iter()
        .map(|candidate| CleanupBase {
            lexical: bare(candidate),
            resolved: fs.resolve(candidate).ok().map(|resolved| bare(&resolved)),
        })
        .collect();
    let list_files = [list, work];

    let mut kept: Vec<&[u8]> = Vec::new();
    let mut start = 0usize;
    while start < text.len() {
        let end = text[start..]
            .iter()
            .position(|&c| c == b'\n')
            .map_or(text.len(), |length| start + length);
        let mut line = &text[start..end];
        start = end + 1;
        if let Some(without) = line.strip_suffix(b"\r") {
            line = without;
        }
        if line.is_empty() || kept.contains(&line) {
            continue;
        }
        if clean_one(fs, line, &bases, &list_files) == LineOutcome::Retry {
            kept.push(line);
        }
    }

    if !kept.is_empty() {
        let mut again = Vec::new();
        for line in &kept {
            again.extend_from_slice(line);
            again.push(b'\n');
        }
        fs.append_durable(list, &again)?; // `work` stays: the lines are not lost
    }
    fs.remove_file(work)
}

/// Deletes what the list names and keeps on the list what could still not be deleted (a file
/// still locked stays listed), working on the renamed copy U/cleanup.list.work so that an append
/// that arrives meanwhile is not lost. Called by the window alone, first thing at its start: the
/// helper only appends, and katana_cli, katana_mcp and a headless window never sweep, so what is
/// listed stays until the window is next started. U is user-writable, so the list is NOT trusted:
/// a line is acted on only when its path, resolved (FsOps::resolve), lies strictly under
/// `state_dir`, or under <root>/.update, and no parent of it between that folder and the path is a
/// reparse point; every other line is dropped without being touched. `root` may be empty (no
/// install known: only `state_dir` is then a place the list may name). OK when the list is absent.
pub fn process_cleanup_list(state_dir: &Path, root: &Path, fs: &dyn FsOps) -> Status {
    require_state_directory(state_dir)?;
    let list = state_dir.join(CLEANUP_LIST_NAME);
    let work = state_dir.join(CLEANUP_WORK_NAME);

    // A run that died after it took the list left it as `work`: that one is finished first, and
    // the list (which may hold what an appender added since) waits for the next start.
    let left_work = fs.stat(&work)?;
    if left_work.kind != EntryKind::Missing {
        return clean_taken(state_dir, root, fs, &list, &work);
    }

    let info = fs.stat(&list)?;
    if info.kind == EntryKind::Missing {
        return Ok(());
    }
    if info.kind != EntryKind::File {
        return Err(make_error(
            ErrorCode::InvalidState,
            "cleanup.list is not a file",
            path_text(&list),
        ));
    }
    fs.rename_file(&list, &work)?;
    clean_taken(state_dir, root, fs, &list, &work)
}

// The replacement of a record on Windows, through the call that reports the system's own error
// number (the module's write_record).
#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "the record is renamed over its old self through the Win32 file API, confined to this adapter"
)]
mod windows {
    use super::Replaced;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::Storage::FileSystem::{GetFileAttributesW, MoveFileExW};

    // The values of the Windows SDK's headers (winerror.h, winnt.h, winbase.h).
    const ERROR_ACCESS_DENIED: u32 = 5;
    const ERROR_SHARING_VIOLATION: u32 = 32;
    const ERROR_LOCK_VIOLATION: u32 = 33;
    const INVALID_FILE_ATTRIBUTES: u32 = 0xFFFF_FFFF;
    const FILE_ATTRIBUTE_READONLY: u32 = 0x1;
    const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x8;

    // The path as the C++ hands it over (path::c_str()): the wide text as it is, terminated.
    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }

    pub(super) fn replace_file(from: &Path, to: &Path) -> Replaced {
        let from = wide(from);
        let to = wide(to);
        // SAFETY: both are NUL-terminated wide paths that outlive the call.
        let moved = unsafe {
            MoveFileExW(
                from.as_ptr(),
                to.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        } != 0;
        if moved {
            return Replaced {
                done: true,
                worth_retrying: false,
                why: String::new(),
            };
        }
        // SAFETY: GetLastError only reads this thread's last-error value.
        let error = unsafe { GetLastError() };
        // 32 sharing violation and 33 lock violation: somebody has the record open. 5 access
        // denied: the same when a scanner holds it, but also what a read-only record or a folder
        // in its place gives, which no wait cures.
        let mut worth_retrying = error == ERROR_SHARING_VIOLATION || error == ERROR_LOCK_VIOLATION;
        if error == ERROR_ACCESS_DENIED {
            // SAFETY: `to` is a NUL-terminated wide path that outlives the call.
            let attributes = unsafe { GetFileAttributesW(to.as_ptr()) };
            worth_retrying = attributes == INVALID_FILE_ATTRIBUTES
                || (attributes & (FILE_ATTRIBUTE_READONLY | FILE_ATTRIBUTE_DIRECTORY)) == 0;
        }
        Replaced {
            done: false,
            worth_retrying,
            why: format!("os_error={error}"),
        }
    }
}
