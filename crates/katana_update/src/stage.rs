//! Downloading what changed, and proving the helper can start, before the window closes.
//!
//! stage(): for each merged range of the plan, one ranged request into the payload; each member
//! of it is inflated (capped at its signed size), hashed and renamed into the stage folder at its
//! root-relative path (<U>/stage-<sequence>/<path>). A file already staged and verified is skipped,
//! so an interrupted or repeated download resumes at file granularity. A member that does not hash
//! to the manifest's value is not kept and the stage fails ("staged_hash"); nothing unverified is
//! ever left in the folder. Needs plan.required_free_bytes free on the stage volume, checked
//! first.
//!
//! WHAT stage() LEAVES BESIDE THE FILES. Last of all - after every member is staged and verified,
//! so that their presence means "complete" - it writes the manifest it was given and its signature
//! into the stage folder, through a temporary name and a rename:
//!
//! ```text
//! <stageDir>/manifest.json        the exact signed bytes (Manifest::signed_json)
//! <stageDir>/manifest.json.sig    the signature line (Manifest::signature_line)
//! ```
//!
//! ([`STAGED_MANIFEST_NAME`], [`STAGED_SIGNATURE_NAME`]: top-level names that no manifest path can
//! have, because a path starts with bin, share, etc or lib - paths.rs). The helper reads them to
//! know WHAT it is applying and verifies them again with its own keys, not trusting that the
//! engine did (apply.rs, step 1); `UPDATE STATUS staged=` reads them back through verify_manifest
//! to name the staged version. A stage folder without both is not a ready stage: it is resumed
//! (stage() skips what is already verified), never applied.
//!
//! ONE stage() AT A TIME PER FOLDER. Two programs of one install - the window and katana_cli - are
//! given the same folder for the same release, and two runs filling it at once would share its
//! temporary files. So stage() holds a lock for as long as it runs: FsOps::try_lock on
//! [`stage_lock_path`], a file BESIDE the folder (U/stage-<sequence>.lock), so that the folder
//! itself holds the release's files, the two records and nothing else. A second stage() of the
//! same folder is refused at once with `stage_busy`: it asks the network for nothing and touches
//! nothing. The lock's file stays when the lock is let go; a later release's stage() removes it
//! together with the folder it belonged to.
//!
//! preflight(): copies the helper (the staged one when its manifest `applier` is newer than the
//! installed helper speaks, else R/bin/katana_updater) into <U>/run-<tx>/ and runs `--selftest`
//! there with the Launcher. The copy is what runs the swap, so at commit nothing under R is loaded
//! and no loaded file is renamed; and running it now makes Defender scan it early, and proves it
//! starts, before the window closes. A staged helper is held to its signed hash twice: where it
//! lies in the stage folder, before anything is created, and again as the COPY in run-<tx>, which
//! is the file that will run - so a staged file that changed between the first look and the copy
//! is caught, and not started. (Both folders are the user's own to write. This is a guard against
//! a file that changed, not against the user: the helper verifies the release again with its own
//! keys before it touches the install.)
//!
//! How it works. The plan says which manifest files differ; the ranges of the plan's members are
//! fetched one request each; every member is inflated (capped at its signed size), hashed while it
//! is written to a temporary name, and only a member whose SHA-256 is the signed one is renamed to
//! its final place under <stageDir>. So a name under the stage folder is a verified file, at every
//! instant, whatever the network, the disk or the user does; a cancelled or failed stage leaves
//! some verified files and no half file, and the next run finds them by their hash and fetches
//! only what is missing. The signed manifest and its signature are written last of all: their
//! presence is "complete".
//!
//! Nothing here trusts what it fetched: a member is believed only by its hash, which is in the
//! signed manifest, and the helper checks the staged files by hash again before it copies them
//! (apply.rs, step 3). That second check is also why a member is written by a plain stream and not
//! flushed to the disk: a staged file a power cut left short fails its hash at the swap, nothing
//! is touched, and the next download replaces it.
//!
//! The folder is worked through an FsOps (fsops.rs), the system's unless one is given. A member is
//! hashed while it is written, by a stream, so the FsOps must be one over the real disk; what it
//! adds is the real file system's long paths and its patience with a virus scanner on Windows, and
//! the tests' ability to run the same code on every platform and to break a write.
//!
//! "Hashed while it is written" is a statement about ONE writer: the hash is of the stream, and
//! what is renamed is the file. So one stage() at a time fills a folder - a lock beside it, held
//! for the whole call - and a second is refused before it touches anything, rather than sharing
//! the first one's temporary files.

use crate::apply::{APPLIER_PROTOCOL, installed_program, new_transaction_id};
use crate::feed::{
    FeedConfig, FeedRoute, Fetch, FetchRequest, MAX_RELEASE_JSON_BYTES, ProgressFn, StopToken,
    asset_api_url, parse_release_assets, payload_url, release_by_tag_url, stop_requested,
};
use crate::fsops::{EntryKind, FileLock, FsOps, RetryPolicy, make_system_fs_ops, path_text};
use crate::inflate::inflate_raw;
use crate::install::Install;
use crate::long_path::openable;
use crate::manifest::{Manifest, ManifestFile, Payload};
use crate::plan::Plan;
use crate::process::{Launcher, SpawnRequest};
use crate::ranges::{RANGE_MAX_LENGTH, RANGE_MERGE_GAP, RangeMember, merge_ranges};
use crate::remove_tree::remove_tree;
use crate::state::{append_cleanup, run_path};
use katana_core::error::make_error;
use katana_core::path_text::{path_from_utf8, path_to_utf8};
use katana_core::sha256::{Sha256, sha256_file_hex};
use katana_core::{Error, ErrorCode, Result, Status};
use std::cell::Cell;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

pub const STAGED_MANIFEST_NAME: &str = "manifest.json";
pub const STAGED_SIGNATURE_NAME: &str = "manifest.json.sig";

pub fn staged_manifest_path(stage_dir: &Path) -> PathBuf {
    stage_dir.join(STAGED_MANIFEST_NAME)
}

pub fn staged_signature_path(stage_dir: &Path) -> PathBuf {
    stage_dir.join(STAGED_SIGNATURE_NAME)
}

/// The file stage() locks while it fills `stage_dir` (see "ONE stage() AT A TIME"): the folder's
/// own name with ".lock" after it, beside the folder - U/stage-<sequence>.lock.
pub fn stage_lock_path(stage_dir: &Path) -> PathBuf {
    // "U/stage-5/" names the same folder as "U/stage-5": the path without its trailing
    // separators, spelled as it was given.
    let mut lock = stage_dir.components().as_path().as_os_str().to_os_string();
    lock.push(".lock");
    PathBuf::from(lock)
}

// Where members are written before they are known to be right. A first component no manifest path
// can have (paths.rs: bin, share, etc or lib), so it can never be the name of a file the stage is
// supposed to hold.
const SCRATCH_FOLDER: &str = ".partial";

// An Error of this file's own, with the reason token fetch_failure_reason reads back.
fn stage_error(code: ErrorCode, message: impl Into<String>, reason: &str, extra: &str) -> Error {
    let mut context = format!("reason={reason}");
    if !extra.is_empty() {
        context.push_str("; ");
        context.push_str(extra);
    }
    make_error(code, message, context)
}

fn cancelled_error() -> Error {
    stage_error(ErrorCode::InvalidState, "cancelled", "cancelled", "")
}

// The stage folder refused something. `why` is the file system's own Error: its message is joined
// to the sentence, and its context (`os_error=<n>` and the path, fsops.rs) follows the token,
// where a person looking for the cause finds the number.
fn write_failed(what: &str, why: &Error, manifest_path: &str) -> Error {
    let mut extra = String::new();
    if !manifest_path.is_empty() {
        extra = format!("path={manifest_path}");
    }
    if !why.context.is_empty() {
        if !extra.is_empty() {
            extra.push_str("; ");
        }
        extra.push_str(&why.context);
    }
    stage_error(
        ErrorCode::FileExportFailure,
        format!("{what}: {}", why.message),
        "write_failed",
        &extra,
    )
}

fn final_path_of(stage_dir: &Path, file: &ManifestFile) -> PathBuf {
    stage_dir.join(path_from_utf8(&file.path))
}

// The staged file at `path` is exactly the manifest's file: a plain file of the signed size whose
// SHA-256 is the signed one. Anything else - absent, a link, another size, another hash - is not
// staged. The hash is read from the disk every time: a file's name proves nothing.
fn is_verified(fs: &dyn FsOps, path: &Path, file: &ManifestFile) -> bool {
    let plain = matches!(
        fs.stat(path),
        Ok(info) if info.kind == EntryKind::File && !info.reparse_point && info.size == file.size
    );
    plain && sha256_file_hex(&openable(path)).is_ok_and(|hash| hash == file.sha256)
}

// What FsOps::try_lock answers when another holder has the lock (fsops.rs).
fn is_held(error: &Error) -> bool {
    error.code == ErrorCode::InvalidState && error.message == "held"
}

// `name` is "stage-<n>" with n a number below `sequence`: the folder of a release this one
// outranks.
fn is_older_stage_name(name: &[u8], sequence: u64) -> bool {
    const PREFIX: &[u8] = b"stage-";
    if !name.starts_with(PREFIX) || name.len() == PREFIX.len() || name.len() > 40 {
        return false;
    }
    let digits = &name[PREFIX.len()..];
    if !digits.iter().all(u8::is_ascii_digit) {
        return false;
    }
    // std::from_chars: a number too large for 64 bits is no number.
    std::str::from_utf8(digits)
        .ok()
        .and_then(|text| text.parse::<u64>().ok())
        .is_some_and(|older| older < sequence)
}

// The last name of a path as bytes (the C++'s pathToUtf8 of filename()).
fn name_bytes(path: &Path) -> Vec<u8> {
    path.file_name()
        .map(|name| path_to_utf8(Path::new(name)))
        .unwrap_or_default()
}

// Removes the folders `stage-<n>` beside the one being filled whose n is below this release's
// sequence - what a helper that died left, and the stage of every release this one outranks - and
// the lock file of each (stage_lock_path). A folder whose lock another program holds is being
// filled by it now, and is left. A folder of a newer sequence, or any other name, is not touched.
// Best effort.
fn sweep_older_stages(fs: &dyn FsOps, stage_dir: &Path, sequence: u64) {
    let Some(parent) = stage_dir
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    else {
        return;
    };
    let Ok(children) = fs.list_directory(parent) else {
        return;
    };
    // The folders to sweep, each once: named by themselves, or by a lock file that outlived its
    // folder (the helper deletes the folder of a release it installed, not the lock beside it).
    const LOCK_SUFFIX: &[u8] = b".lock";
    let mut folders: Vec<PathBuf> = Vec::new();
    for child in &children {
        let mut name = name_bytes(child);
        if name.ends_with(LOCK_SUFFIX) {
            name.truncate(name.len() - LOCK_SUFFIX.len());
        }
        if !is_older_stage_name(&name, sequence) {
            continue;
        }
        let folder = parent.join(path_from_utf8(&name));
        if folder != stage_dir && !folders.contains(&folder) {
            folders.push(folder);
        }
    }
    for folder in &folders {
        let lock_path = stage_lock_path(folder);
        {
            let lock = fs.try_lock(&lock_path);
            if let Err(error) = &lock
                && is_held(error)
            {
                continue;
            }
            // Only a folder: a file somebody named like one is not this program's.
            if matches!(fs.stat(folder), Ok(info) if info.kind == EntryKind::Directory) {
                let mut leftovers: Vec<PathBuf> = Vec::new();
                remove_tree(fs, folder, &mut leftovers, "");
            }
            // Let go first: a lock that is held cannot be deleted, by its holder either.
            drop(lock);
        }
        let _ = fs.remove_file(&lock_path);
    }
}

// Whatever stands in a staged file's place goes: nothing (the usual case), a file that failed its
// hash, or a folder somebody put there, with all that is in it. A link is removed, never entered
// (remove_tree).
fn clear_place(fs: &dyn FsOps, place: &Path) -> Status {
    let is_folder = matches!(fs.stat(place), Ok(info) if info.kind == EntryKind::Directory);
    if !is_folder {
        return fs.remove_file(place); // success when nothing is there
    }
    let mut leftovers: Vec<PathBuf> = Vec::new();
    remove_tree(fs, place, &mut leftovers, "");
    if let Some(first) = leftovers.first() {
        return Err(make_error(
            ErrorCode::FileExportFailure,
            "a folder in a staged file's place could not be removed",
            format!("os_error=0 {}", path_text(first)),
        ));
    }
    Ok(())
}

// The names between the stage folder and a staged file must be folders of the stage. One that is
// something else - a file, or a link, which would send the member to wherever it points - goes,
// so that the folder can be made. Only names INSIDE `stage_dir` are looked at: the walk is over
// the manifest path's own parts, never upwards from the file.
fn clear_parents(fs: &dyn FsOps, stage_dir: &Path, file: &ManifestFile) -> Status {
    let relative = path_from_utf8(&file.path);
    let parts: Vec<_> = relative.iter().collect();
    let mut folder = stage_dir.to_path_buf();
    // Every part but the file's own name.
    for part in parts.iter().take(parts.len().saturating_sub(1)) {
        folder.push(part);
        let info = match fs.stat(&folder) {
            Ok(info) if info.kind != EntryKind::Missing => info,
            // Nothing below it either; making the folders says what is wrong, if anything.
            _ => break,
        };
        if info.kind == EntryKind::Directory && !info.reparse_point {
            continue;
        }
        let mut leftovers: Vec<PathBuf> = Vec::new();
        remove_tree(fs, &folder, &mut leftovers, "");
        if let Some(first) = leftovers.first() {
            return Err(make_error(
                ErrorCode::FileExportFailure,
                "what stands where a folder of the stage belongs could not be removed",
                format!("os_error=0 {}", path_text(first)),
            ));
        }
        break;
    }
    Ok(())
}

// The scratch folder goes however stage() ends: on success it is empty, on failure it holds the
// member that was being written.
struct ScratchFolder<'f> {
    fs: &'f dyn FsOps,
    path: PathBuf,
}

impl Drop for ScratchFolder<'_> {
    fn drop(&mut self) {
        let mut leftovers: Vec<PathBuf> = Vec::new();
        remove_tree(self.fs, &self.path, &mut leftovers, "");
    }
}

fn disk_full(file: &ManifestFile) -> Error {
    stage_error(
        ErrorCode::FileExportFailure,
        "the stage folder could not be written (is the disk full?)",
        "write_failed",
        &format!("path={}", file.path),
    )
}

// Inflates `compressed` to the member's final place, if and only if it is the signed file.
fn stage_member(
    fs: &dyn FsOps,
    scratch: &Path,
    stage_dir: &Path,
    index: usize,
    file: &ManifestFile,
    compressed: &[u8],
    stop: StopToken<'_>,
) -> Status {
    let temporary = scratch.join(format!("{index}"));
    let target = final_path_of(stage_dir, file);
    if let Err(removed) = fs.remove_file(&temporary) {
        return Err(write_failed(
            "the stage folder could not be prepared",
            &removed,
            &file.path,
        ));
    }

    let Ok(created) = std::fs::File::create(openable(&temporary)) else {
        return Err(stage_error(
            ErrorCode::FileExportFailure,
            "a downloaded file could not be written to the stage folder",
            "write_failed",
            &format!("path={}", file.path),
        ));
    };
    let mut out = BufWriter::new(created);
    let mut hasher = Sha256::new();
    let inflated = {
        let mut sink = |block: &[u8]| -> Status {
            if stop_requested(stop) {
                return Err(cancelled_error());
            }
            hasher.update(block);
            if out.write_all(block).is_err() {
                return Err(disk_full(file));
            }
            Ok(())
        };
        inflate_raw(compressed, file.size, &mut sink)
    };
    // The C++'s out.close(), then out.fail(): what is still buffered is written now.
    let closed = out.into_inner().is_ok();
    if let Err(why) = inflated {
        let _ = fs.remove_file(&temporary);
        // The sink's own failures (cancelled, the disk), and the inflater failing to run at all,
        // which says nothing about the member: passed on as they are. Everything else is what the
        // inflater found wrong with the bytes.
        if why.code == ErrorCode::InvalidState
            || why.code == ErrorCode::FileExportFailure
            || why.code == ErrorCode::Internal
        {
            return Err(why);
        }
        return Err(stage_error(
            ErrorCode::InvalidArgument,
            format!(
                "a downloaded file is damaged and could not be unpacked: {}",
                why.message
            ),
            "staged_corrupt",
            &format!("path={}", file.path),
        ));
    }
    if !closed {
        let _ = fs.remove_file(&temporary);
        return Err(disk_full(file));
    }
    // The only comparison that matters: the bytes are the signed ones. Nothing unverified is ever
    // given a final name.
    if hasher.finish_hex() != file.sha256 {
        let _ = fs.remove_file(&temporary);
        return Err(stage_error(
            ErrorCode::InvalidArgument,
            "a downloaded file is not the one the release signed",
            "staged_hash",
            &format!("path={}", file.path),
        ));
    }

    if let Err(cleared) = clear_parents(fs, stage_dir, file) {
        let _ = fs.remove_file(&temporary);
        return Err(write_failed(
            "the stage folder could not be prepared",
            &cleared,
            &file.path,
        ));
    }
    if let Err(made) = fs.make_directories(target.parent().unwrap_or(Path::new(""))) {
        let _ = fs.remove_file(&temporary);
        return Err(write_failed(
            "the stage folder could not be prepared",
            &made,
            &file.path,
        ));
    }
    // A rename never replaces (fsops.rs). What is there is a file that failed its hash, or
    // something that was never the stage's - a folder, a link: whatever it is, it goes.
    if let Err(removed) = clear_place(fs, &target) {
        let _ = fs.remove_file(&temporary);
        return Err(write_failed(
            "what is in a staged file's place could not be removed",
            &removed,
            &file.path,
        ));
    }
    if let Err(moved) = fs.rename_file(&temporary, &target) {
        let _ = fs.remove_file(&temporary);
        return Err(write_failed(
            "a verified file could not be put in its place",
            &moved,
            &file.path,
        ));
    }
    if file.executable
        && let Err(marked) = fs.set_executable(&target)
    {
        // The file stays: it is the signed bytes, and the swap and the preflight each mark their
        // own copy (apply.rs, preflight below). But a folder that refuses this is not one to go
        // on writing into.
        return Err(write_failed(
            "a staged program could not be made runnable",
            &marked,
            &file.path,
        ));
    }
    Ok(())
}

// <stage_dir>/<name>, through the scratch folder and a rename.
fn write_staged_record(fs: &dyn FsOps, scratch: &Path, target: &Path, bytes: &[u8]) -> Status {
    let temporary = scratch.join(target.file_name().unwrap_or_default());
    if let Err(written) = fs.write_durable(&temporary, bytes) {
        return Err(write_failed(
            "the staged manifest could not be written",
            &written,
            "",
        ));
    }
    if let Err(removed) = clear_place(fs, target) {
        return Err(write_failed(
            "an earlier staged manifest could not be removed",
            &removed,
            "",
        ));
    }
    if let Err(moved) = fs.rename_file(&temporary, target) {
        return Err(write_failed(
            "the staged manifest could not be put in its place",
            &moved,
            "",
        ));
    }
    Ok(())
}

// Where the payload is fetched from, and with what.
#[derive(Default)]
struct PayloadSource {
    url: String,
    accept: String,
    bearer_token: String,
}

fn resolve_payload_source(
    fetch: &Fetch<'_>,
    feed: &FeedConfig,
    manifest: &Manifest,
    payload: &Payload,
    stop: StopToken<'_>,
) -> Result<PayloadSource> {
    if feed.route == FeedRoute::Download {
        return Ok(PayloadSource {
            url: payload_url(feed, manifest),
            ..PayloadSource::default()
        });
    }
    // The private route: the release the manifest names (the one that is `latest` may have moved
    // on since the check), its assets, and the payload's id among them.
    if feed.token.is_empty() {
        return Err(stage_error(
            ErrorCode::InvalidArgument,
            "the private-repository route needs an access token",
            "no_token",
            "",
        ));
    }
    let lookup = FetchRequest {
        url: release_by_tag_url(feed, &manifest.tag),
        accept: String::from("application/vnd.github+json"),
        bearer_token: feed.token.clone(),
        max_bytes: MAX_RELEASE_JSON_BYTES,
        user_agent: feed.user_agent.clone(),
        ..FetchRequest::default()
    };
    let Some(transport) = fetch else {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "stage was given no transport",
        ));
    };
    let release = transport(&lookup, stop, None)?;
    let assets = parse_release_assets(release)
        .map_err(|error| stage_error(ErrorCode::ParseFailure, error.message, "malformed", ""))?;
    let Some(asset) = assets.iter().find(|asset| asset.name == payload.name) else {
        return Err(stage_error(
            ErrorCode::NotFound,
            format!("the release has no file named {}", payload.name),
            "asset_missing",
            "",
        ));
    };
    // The size the listing gives is not signed; the signed one is what the ranges are held to. A
    // listing that disagrees says the asset is not the one the manifest describes.
    if asset.size != 0 && asset.size != payload.size {
        return Err(stage_error(
            ErrorCode::InvalidState,
            "the release's file is not the length its manifest names; the release may have \
             been replaced",
            "payload_mismatch",
            "",
        ));
    }
    Ok(PayloadSource {
        url: asset_api_url(feed, asset.id),
        accept: String::from("application/octet-stream"),
        bearer_token: feed.token.clone(),
    })
}

// WHERE A COPY OF KATANA DOES NOT REPLACE ITS OWN FILES. The forms of stage() and preflight() that
// use the system's file system refuse there under the token `platform`, before a folder is asked
// for or a request is sent. Two cases, and they do not hang on each other:
//
//   macOS, for good   A file changed inside Katana.app breaks the seal of the bundle
//                     (docs/update.md), so nothing is downloaded and no helper is prepared there
//                     WHATEVER the file system answers. fsops_posix.rs is compiled for macOS too:
//                     the day it is written for Linux it works on a Mac as well, and this refusal
//                     must still stand. So it asks the file system nothing.
//   Linux, for now    The system's file system is not written (fsops_posix.rs): every operation of
//                     it is one refusal, with the code Unsupported. Worked through, that comes
//                     back as `write_failed` from stage() - a folder that could not be written -
//                     and as `helper_missing` from preflight(), neither of which is what is wrong.
//                     So one question is put to it first and its refusal is handed on as it is.
//                     Where the file system is written stat() does not answer Unsupported, and
//                     nothing here stands in the way.
#[cfg(target_vendor = "apple")]
fn refused_by_the_platform(_system: &dyn FsOps, _folder: &Path) -> Option<Error> {
    Some(stage_error(
        ErrorCode::Unsupported,
        "Katana does not install updates itself on macOS, where a changed file would break the \
         seal of the application: it checks for a newer version, which is then downloaded from \
         the releases page and installed by hand",
        "platform",
        "",
    ))
}

#[cfg(all(not(windows), not(target_vendor = "apple")))]
fn refused_by_the_platform(system: &dyn FsOps, folder: &Path) -> Option<Error> {
    match system.stat(folder) {
        Err(asked) if asked.code == ErrorCode::Unsupported => Some(stage_error(
            ErrorCode::Unsupported,
            asked.message,
            "platform",
            "",
        )),
        _ => None,
    }
}

/// Fetches and stages every changed file of `plan` into `stage_dir`, then writes the staged
/// manifest and signature (see the top of the module), through the system's file system.
///
/// InvalidState "cancelled"; the transport's errors; InvalidArgument for a member that fails its
/// hash or its size; FileExportFailure when the disk refuses; AlreadyExists when another stage()
/// is filling the same folder; Unsupported where this system installs no update (`platform`,
/// below). Progress is bytes of the compressed payload fetched over the total to fetch, counting
/// what a previous run already staged as fetched.
///
/// The Error of a failure carries a machine-readable token, `reason=<token>` first in its context
/// (fetch_failure_reason, feed.rs, reads it back; the transport's own failures have theirs:
/// http_<status>, offline, html_page, partial_downloads, cancelled):
///
/// ```text
/// staged_hash      a member inflated to bytes that are not the signed file
/// staged_corrupt   a member could not be unpacked, or is not the signed length
/// write_failed     the stage folder could not be written; the file system's own `os_error=<n>`
///                  and path follow the token (fsops.rs)
/// stage_busy       another stage() holds the folder's lock (stage_lock_path): nothing was fetched
///                  or touched; the other run finishes, or this one is tried again
/// low_disk_space   the volume has less than the plan's required_free_bytes (less what is staged)
/// short_range      the transport handed over a body that is not the range's length (a Fetch of
///                  make_fetch refuses that itself, as bad_range: feed.rs)
/// unverified_manifest / notify_only / no_token   the call is not one stage() can serve
/// asset_missing / payload_mismatch / malformed   the private route's release listing
/// platform         this system is one where a copy of Katana does not replace its own files
///                  (macOS for good, Linux until its file system is written: the module says
///                  why). Only the forms that use the system's file system refuse so, and before
///                  a folder is asked for or a request is sent
/// ```
///
/// preflight's: helper_missing, helper_not_started, helper_timeout, helper_failed, write_failed,
/// and platform as above.
///
/// Progress never goes backwards and never passes its total, whatever the transport reports (it
/// starts a request again after a dropped connection).
///
/// What is asked for is not Plan::ranges as it stands: the changed files that are not staged yet
/// are merged again (merge_ranges), so a resumed stage asks for less than the plan lists. Folders
/// `stage-<n>` beside `stage_dir` with n below the manifest's sequence are removed (the stage of a
/// release this one outranks) with their lock files `stage-<n>.lock` - unless another program
/// holds that lock, which means it is filling the folder now; nothing else beside `stage_dir` is
/// touched.
///
/// Inside `stage_dir` a name on the way to a staged file is the stage's to use: a folder (or a link
/// to one) that stands where a file belongs, and a file or a link that stands where a folder
/// belongs, is removed - a link itself, never what it points at - and the file is staged. So
/// nothing somebody left in the folder stops a stage for good, and a member is not written through
/// a link that was lying there to somewhere outside it.
pub fn stage(
    fetch: &Fetch<'_>,
    feed: &FeedConfig,
    manifest: &Manifest,
    plan: &Plan,
    stage_dir: &Path,
    stop: StopToken<'_>,
    progress: ProgressFn<'_>,
) -> Status {
    let system = make_system_fs_ops(&RetryPolicy::default());
    #[cfg(not(windows))]
    {
        if let Some(refused) = refused_by_the_platform(system.as_ref(), stage_dir) {
            return Err(refused);
        }
    }
    stage_with(
        fetch,
        feed,
        manifest,
        plan,
        stage_dir,
        stop,
        progress,
        system.as_ref(),
    )
}

/// The same, through the file system given instead of the system's (make_system_fs_ops, which
/// [`stage`] uses). It is how the tests run the staging on every platform - the system's own file
/// system is written for Windows only so far (fsops.rs), and elsewhere [`stage`] refuses at once
/// under the token `platform`, touching nothing - and how they make a write fail. `fs` must be one
/// over the real disk: a member is hashed while a stream writes it, which no FsOps call does.
#[allow(
    clippy::too_many_arguments,
    reason = "the C++'s stage(), with the file system the other form of it supplies"
)]
pub fn stage_with(
    fetch: &Fetch<'_>,
    feed: &FeedConfig,
    manifest: &Manifest,
    plan: &Plan,
    stage_dir: &Path,
    stop: StopToken<'_>,
    progress: ProgressFn<'_>,
    fs: &dyn FsOps,
) -> Status {
    if fetch.is_none() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "stage was given no transport",
        ));
    }
    if stage_dir.as_os_str().is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "stage was given no folder",
        ));
    }
    // Only what verify_manifest returned is staged: the helper is handed these bytes to verify
    // again, and a manifest that was never verified has none (manifest.rs).
    if manifest.signed_json.is_empty() || manifest.signature_line.is_empty() {
        return Err(stage_error(
            ErrorCode::InvalidArgument,
            "only a verified manifest can be staged",
            "unverified_manifest",
            "",
        ));
    }
    let payload = match &manifest.payload {
        Some(payload) if !manifest.notify_only => payload,
        _ => {
            return Err(stage_error(
                ErrorCode::Unsupported,
                "this release is an announcement and has nothing to download",
                "notify_only",
                "",
            ));
        }
    };
    if plan.files.len() != manifest.files.len() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the plan is not the plan of this manifest",
            format!(
                "{} planned, {} listed",
                plan.files.len(),
                manifest.files.len()
            ),
        ));
    }
    if stop_requested(stop) {
        return Err(cancelled_error());
    }

    if let Err(made) = fs.make_directories(stage_dir) {
        return Err(write_failed(
            "the stage folder could not be created",
            &made,
            "",
        ));
    }
    // One stage() at a time in this folder: two would share its temporary files, and each would
    // delete the other's. Taken before anything in the folder is touched, and held to the end of
    // this function - it is declared before the scratch folder, which is therefore emptied while
    // the lock is still held.
    let _lock: Box<dyn FileLock> = match fs.try_lock(&stage_lock_path(stage_dir)) {
        Ok(lock) => lock,
        Err(error) => {
            if is_held(&error) {
                return Err(stage_error(
                    ErrorCode::AlreadyExists,
                    "another copy of Katana is downloading this update",
                    "stage_busy",
                    "",
                ));
            }
            return Err(write_failed(
                "the stage folder could not be locked",
                &error,
                "",
            ));
        }
    };
    // A staged manifest says "complete"; this run has not completed yet, so a record of an
    // earlier one must not keep saying so if this one fails.
    for stale in [
        staged_manifest_path(stage_dir),
        staged_signature_path(stage_dir),
    ] {
        if let Err(removed) = clear_place(fs, &stale) {
            return Err(write_failed(
                "an earlier staged manifest could not be removed",
                &removed,
                "",
            ));
        }
    }
    sweep_older_stages(fs, stage_dir, manifest.sequence);

    let scratch = ScratchFolder {
        fs,
        path: stage_dir.join(SCRATCH_FOLDER),
    };
    {
        let mut leftovers: Vec<PathBuf> = Vec::new();
        remove_tree(fs, &scratch.path, &mut leftovers, ""); // a crashed run's member
    }
    if let Err(made) = fs.make_directories(&scratch.path) {
        return Err(write_failed(
            "the stage folder could not be prepared",
            &made,
            "",
        ));
    }

    // What is already staged and right is skipped: the resume.
    let mut missing: Vec<RangeMember> = Vec::new();
    let mut staged_bytes: u64 = 0; // size of the files already there
    let mut staged_compressed: u64 = 0; // csize of the same
    for planned in &plan.files {
        if !planned.changed {
            continue;
        }
        let Some(file) = manifest.files.get(planned.index) else {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "the plan names a file the manifest does not list",
                format!("{}", planned.index),
            ));
        };
        if is_verified(fs, &final_path_of(stage_dir, file), file) {
            staged_bytes += file.size;
            staged_compressed += file.compressed_size;
        } else {
            missing.push(RangeMember {
                file_index: planned.index,
                offset: file.offset,
                length: file.compressed_size,
            });
        }
        if stop_requested(stop) {
            return Err(cancelled_error());
        }
    }

    // Free space first (design, section 2): 2.6 times the changed bytes, less what the stage
    // already holds, which is on the disk and so already out of the figure the volume reports.
    let needed = plan.required_free_bytes.saturating_sub(staged_bytes);
    if needed > 0
        && let Ok(free) = fs.free_space(stage_dir)
        && free < needed
    {
        return Err(stage_error(
            ErrorCode::FileExportFailure,
            "there is not enough free disk space to download the update",
            "low_disk_space",
            &format!("need={needed} bytes; free={free} bytes"),
        ));
    }
    // An unreadable figure does not stop the download: the writes will say if the disk is full.

    let ranges = merge_ranges(missing, RANGE_MERGE_GAP, RANGE_MAX_LENGTH);
    let mut total = staged_compressed;
    for range in &ranges {
        total += range.length;
    }
    // Never backwards, never past the total: the transport starts a request again after a dropped
    // connection, and its count with it.
    let shown = Cell::new(0u64);
    let report = |done: u64| {
        shown.set(shown.get().max(done.min(total)));
        if let Some(progress) = progress {
            progress(shown.get(), total);
        }
    };
    report(staged_compressed);

    if !ranges.is_empty() {
        let source = resolve_payload_source(fetch, feed, manifest, payload, stop)?;
        let Some(transport) = fetch else {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "stage was given no transport",
            ));
        };
        let mut done = staged_compressed;
        for range in &ranges {
            if stop_requested(stop) {
                return Err(cancelled_error());
            }
            let request = FetchRequest {
                url: source.url.clone(),
                range_start: range.offset,
                range_length: range.length,
                payload_size: payload.size,
                // The server may not send more than was asked for: a server that ignores `Range`
                // and starts the whole payload is cut off after this many bytes.
                max_bytes: range.length,
                accept: source.accept.clone(),
                bearer_token: source.bearer_token.clone(),
                user_agent: feed.user_agent.clone(),
            };
            let base = done;
            let received: &dyn Fn(u64, u64) =
                &|received: u64, _total: u64| report(base + received.min(range.length));
            let body = transport(&request, stop, Some(received))?;
            if body.len() as u64 != range.length {
                return Err(stage_error(
                    ErrorCode::FileImportFailure,
                    format!(
                        "the download server sent {} bytes for a range of {}",
                        body.len(),
                        range.length
                    ),
                    "short_range",
                    "",
                ));
            }
            for member in &range.members {
                if stop_requested(stop) {
                    return Err(cancelled_error());
                }
                let file = &manifest.files[member.file_index];
                // merge_ranges puts every member inside its range; checked all the same, for it is
                // the line between the server's bytes and a read past them.
                if member.offset < range.offset
                    || member.offset - range.offset > range.length
                    || member.length > range.length - (member.offset - range.offset)
                {
                    return Err(make_error(
                        ErrorCode::Internal,
                        "a file lies outside the range that holds it",
                        file.path.clone(),
                    ));
                }
                let start = (member.offset - range.offset) as usize;
                let slice = &body[start..start + member.length as usize];
                stage_member(
                    fs,
                    &scratch.path,
                    stage_dir,
                    member.file_index,
                    file,
                    slice,
                    stop,
                )?;
            }
            done += range.length;
            report(done);
        }
    }
    if stop_requested(stop) {
        return Err(cancelled_error());
    }

    // Last: the signed manifest, then its signature. Their presence is "complete".
    write_staged_record(
        fs,
        &scratch.path,
        &staged_manifest_path(stage_dir),
        &manifest.signed_json,
    )?;
    write_staged_record(
        fs,
        &scratch.path,
        &staged_signature_path(stage_dir),
        &manifest.signature_line,
    )?;
    if let Err(synced) = fs.sync_directory(stage_dir) {
        return Err(write_failed(
            "the stage folder could not be flushed to the disk",
            &synced,
            "",
        ));
    }
    report(total);
    Ok(())
}

// ---- preflight ---------------------------------------------------------------------------------

/// What launch_apply needs, once the helper has been copied and has proved it starts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PreparedApply {
    pub transaction: String,
    /// U/run-<tx>.
    pub run_directory: PathBuf,
    /// The copy in it.
    pub helper: PathBuf,
}

const OUTPUT_SHOWN: usize = 240;

// The first words of what the helper printed, for the reason: bounded, printable.
fn excerpt_of(output: &[u8]) -> String {
    output
        .iter()
        .take(OUTPUT_SHOWN)
        .map(|&byte| {
            if (0x20..0x7F).contains(&byte) {
                char::from(byte)
            } else {
                ' '
            }
        })
        .collect()
}

// The run folder goes, and what a lock keeps there is left for the next start (state.rs).
fn remove_run_folder(fs: &dyn FsOps, state_dir: &Path, run_dir: &Path) {
    let mut leftovers: Vec<PathBuf> = Vec::new();
    remove_tree(fs, run_dir, &mut leftovers, "");
    for leftover in &leftovers {
        let _ = append_cleanup(state_dir, leftover, fs);
    }
}

/// The design's preflight(manifest, stageDir), with the three things it cannot do without: the
/// install (for R and the installed helper), the state directory (for run-<tx>) and the launcher
/// (for --selftest), through the system's file system. Failure leaves no run folder behind.
pub fn preflight(
    manifest: &Manifest,
    install: &Install,
    stage_dir: &Path,
    state_dir: &Path,
    launcher: &dyn Launcher,
) -> Result<PreparedApply> {
    let system = make_system_fs_ops(&RetryPolicy::default());
    #[cfg(not(windows))]
    {
        if let Some(refused) = refused_by_the_platform(system.as_ref(), state_dir) {
            return Err(refused);
        }
    }
    preflight_with(
        manifest,
        install,
        stage_dir,
        state_dir,
        launcher,
        system.as_ref(),
    )
}

/// The same, through the file system given (see [`stage_with`]).
pub fn preflight_with(
    manifest: &Manifest,
    install: &Install,
    stage_dir: &Path,
    state_dir: &Path,
    launcher: &dyn Launcher,
    fs: &dyn FsOps,
) -> Result<PreparedApply> {
    if state_dir.as_os_str().is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "preflight was given no state folder",
        ));
    }

    // Which helper runs the swap. The installed one speaks the protocol this build speaks; a
    // manifest that needs a newer one carries it, and it is the staged copy that runs, after its
    // hash is checked against the signed one (the stage folder is user-writable).
    let installed_helper = installed_program(&install.root, "katana_updater");
    let mut source = installed_helper.clone();
    // The manifest's entry of the helper, when it is the staged one that runs: what its copy is
    // held to, below.
    let mut signed_helper: Option<&ManifestFile> = None;
    if manifest.applier > APPLIER_PROTOCOL {
        let wanted = format!(
            "bin/{}",
            String::from_utf8_lossy(&name_bytes(&installed_helper))
        );
        let Some(file) = manifest.files.iter().find(|file| file.path == wanted) else {
            return Err(stage_error(
                ErrorCode::InvalidState,
                "this release needs a newer update helper than the installed one and does not \
                 carry one",
                "helper_missing",
                "",
            ));
        };
        signed_helper = Some(file);
        source = final_path_of(stage_dir, file);
        // Before anything is created: a helper that is not there, or not the signed one, is found
        // without leaving a run folder to clear away.
        if !is_verified(fs, &source, file) {
            return Err(stage_error(
                ErrorCode::InvalidState,
                "the release's update helper is not staged, or is not the one the release signed",
                "helper_missing",
                &format!("path={}", file.path),
            ));
        }
    } else {
        let installed = matches!(fs.stat(&source), Ok(info) if info.kind == EntryKind::File);
        if !installed {
            return Err(stage_error(
                ErrorCode::NotFound,
                "the update helper is not installed",
                "helper_missing",
                &format!("path={}", path_text(&source)),
            ));
        }
    }

    let transaction = new_transaction_id();
    let run_dir = run_path(state_dir, &transaction);
    if let Err(made) = fs.make_directories(&run_dir) {
        return Err(write_failed(
            "the update helper's folder could not be created",
            &made,
            "",
        ));
    }
    let helper = run_dir.join(source.file_name().unwrap_or_default());
    if let Err(copied) = fs.copy_file(&source, &helper) {
        remove_run_folder(fs, state_dir, &run_dir);
        return Err(write_failed(
            "the update helper could not be copied",
            &copied,
            "",
        ));
    }
    // The copy is the file that will run, so the copy is what must be the signed helper. The stage
    // folder is user-writable: the file that was hashed there a moment ago and the file that was
    // just copied from there need not be the same bytes.
    if let Some(file) = signed_helper
        && !is_verified(fs, &helper, file)
    {
        remove_run_folder(fs, state_dir, &run_dir);
        return Err(stage_error(
            ErrorCode::InvalidState,
            "the release's update helper changed while it was being copied; the copy is not the \
             one the release signed",
            "helper_missing",
            &format!("path={}", file.path),
        ));
    }
    if let Err(marked) = fs.set_executable(&helper) {
        remove_run_folder(fs, state_dir, &run_dir);
        return Err(write_failed(
            "the update helper could not be made runnable",
            &marked,
            "",
        ));
    }

    // It starts: the copy is scanned now, while the window is still open, and a helper that cannot
    // run here is found before anything is closed.
    let request = SpawnRequest {
        executable: helper.clone(),
        arguments: vec![String::from("--selftest")],
        working_directory: run_dir.clone(),
        ..SpawnRequest::default()
    };
    let ran = match launcher.run(&request) {
        Ok(ran) => ran,
        Err(error) => {
            remove_run_folder(fs, state_dir, &run_dir);
            return Err(stage_error(
                ErrorCode::FileImportFailure,
                format!("the update helper could not be started: {}", error.message),
                "helper_not_started",
                "",
            ));
        }
    };
    if ran.timed_out {
        remove_run_folder(fs, state_dir, &run_dir);
        return Err(stage_error(
            ErrorCode::FileImportFailure,
            "the update helper did not finish its self-test in time",
            "helper_timeout",
            "",
        ));
    }
    if ran.exit_code != 0 {
        remove_run_folder(fs, state_dir, &run_dir);
        return Err(stage_error(
            ErrorCode::FileImportFailure,
            "the update helper's self-test failed",
            "helper_failed",
            &format!("exit={}; {}", ran.exit_code, excerpt_of(&ran.output)),
        ));
    }

    Ok(PreparedApply {
        transaction,
        run_directory: run_dir,
        helper,
    })
}
