#pragma once

// Downloading what changed, and proving the helper can start, before the
// window closes.
//
// stage(): for each merged range of the plan, one ranged request into the
// payload; each member of it is inflated (capped at its signed size), hashed
// and renamed into the stage folder at its root-relative path
// (<U>/stage-<sequence>/<path>). A file already staged and verified is skipped,
// so an interrupted or repeated download resumes at file granularity. A member
// that does not hash to the manifest's value is not kept and the stage fails
// ("staged_hash"); nothing unverified is ever left in the folder. Needs
// plan.requiredFreeBytes free on the stage volume, checked first.
//
// WHAT stage() LEAVES BESIDE THE FILES. Last of all - after every member is staged and
// verified, so that their presence means "complete" - it writes the manifest it was given
// and its signature into the stage folder, through a temporary name and a rename:
//
//   <stageDir>/manifest.json        the exact signed bytes (Manifest::signedJson)
//   <stageDir>/manifest.json.sig    the signature line (Manifest::signatureLine)
//
// (kStagedManifestName, kStagedSignatureName: top-level names that no manifest path can
// have, because a path starts with bin, share, etc or lib - paths.hpp). The helper reads
// them to know WHAT it is applying and verifies them again with its own keys, not
// trusting that the engine did (apply.hpp, step 1); `UPDATE STATUS staged=` reads them
// back through verifyManifest to name the staged version. A stage folder without both
// is not a ready stage: it is resumed (stage() skips what is already verified), never
// applied.
//
// ONE stage() AT A TIME PER FOLDER. Two programs of one install - the window and katana_cli -
// are given the same folder for the same release, and two runs filling it at once would share
// its temporary files. So stage() holds a lock for as long as it runs: FsOps::tryLock on
// stageLockPath(stageDir), a file BESIDE the folder (U/stage-<sequence>.lock), so that the
// folder itself holds the release's files, the two records and nothing else. A second stage()
// of the same folder is refused at once with `stage_busy`: it asks the network for nothing and
// touches nothing. The lock's file stays when the lock is let go; a later release's stage()
// removes it together with the folder it belonged to.
//
// preflight(): copies the helper (the staged one when its manifest `applier`
// is newer than the installed helper speaks, else R/bin/katana_updater) into
// <U>/run-<tx>/ and runs `--selftest` there with the Launcher. The copy is
// what runs the swap, so at commit nothing under R is loaded and no loaded
// file is renamed; and running it now makes Defender scan it early, and proves
// it starts, before the window closes. A staged helper is held to its signed
// hash twice: where it lies in the stage folder, before anything is created,
// and again as the COPY in run-<tx>, which is the file that will run - so
// what is started is what the release signed, whatever happened to the staged
// file between the first look and the copy.

#include <filesystem>
#include <stop_token>
#include <string>
#include <string_view>

#include "katana/core/error.hpp"
#include "katana/update/apply.hpp"
#include "katana/update/feed.hpp"
#include "katana/update/fsops.hpp"
#include "katana/update/install.hpp"
#include "katana/update/manifest.hpp"
#include "katana/update/plan.hpp"
#include "katana/update/process.hpp"

namespace katana::update {

inline constexpr std::string_view kStagedManifestName = "manifest.json";
inline constexpr std::string_view kStagedSignatureName = "manifest.json.sig";

[[nodiscard]] inline std::filesystem::path
stagedManifestPath(const std::filesystem::path& stageDir)
{
    return stageDir / kStagedManifestName;
}
[[nodiscard]] inline std::filesystem::path
stagedSignaturePath(const std::filesystem::path& stageDir)
{
    return stageDir / kStagedSignatureName;
}

// The file stage() locks while it fills `stageDir` (see "ONE stage() AT A TIME"): the folder's
// own name with ".lock" after it, beside the folder - U/stage-<sequence>.lock.
[[nodiscard]] inline std::filesystem::path stageLockPath(const std::filesystem::path& stageDir)
{
    // "U/stage-5/" names the same folder as "U/stage-5".
    std::filesystem::path lock = stageDir.has_filename() ? stageDir : stageDir.parent_path();
    lock += ".lock";
    return lock;
}

// Fetches and stages every changed file of `plan` into `stageDir`, then writes the staged
// manifest and signature (see above).
// InvalidState "cancelled"; the transport's errors; InvalidArgument for a
// member that fails its hash or its size; FileExportFailure when the disk
// refuses; AlreadyExists when another stage() is filling the same folder.
// Progress is bytes of the compressed payload fetched over the total
// to fetch, counting what a previous run already staged as fetched.
//
// The Error of a failure carries a machine-readable token, `reason=<token>` first in
// its context (fetchFailureReason, feed.hpp, reads it back; the transport's own failures
// have theirs: http_<status>, offline, html_page, partial_downloads, cancelled):
//   staged_hash      a member inflated to bytes that are not the signed file
//   staged_corrupt   a member could not be unpacked, or is not the signed length
//   write_failed     the stage folder could not be written; the file system's own
//                    `os_error=<n>` and path follow the token (fsops.hpp)
//   stage_busy       another stage() holds the folder's lock (stageLockPath): nothing was
//                    fetched or touched; the other run finishes, or this one is tried again
//   low_disk_space   the volume has less than the plan's requiredFreeBytes (less what is staged)
//   short_range      the transport handed over a body that is not the range's length (a Fetch
//                    of makeFetch refuses that itself, as bad_range: feed.hpp)
//   unverified_manifest / notify_only / no_token   the call is not one stage() can serve
//   asset_missing / payload_mismatch / malformed   the private route's release listing
// preflight's: helper_missing, helper_not_started, helper_timeout, helper_failed, write_failed.
//
// Progress never goes backwards and never passes its total, whatever the transport reports
// (it starts a request again after a dropped connection).
//
// What is asked for is not Plan::ranges as it stands: the changed files that are not staged
// yet are merged again (mergeRanges), so a resumed stage asks for less than the plan lists.
// Folders `stage-<n>` beside `stageDir` with n below the manifest's sequence are removed (the
// stage of a release this one outranks) with their lock files `stage-<n>.lock` - unless
// another program holds that lock, which means it is filling the folder now; nothing else
// beside `stageDir` is touched.
//
// Inside `stageDir` a name on the way to a staged file is the stage's to use: a folder (or a
// link to one) that stands where a file belongs, and a file or a link that stands where a
// folder belongs, is removed - a link itself, never what it points at - and the file is staged.
// So nothing somebody left in the folder stops a stage for good, and a member is not written
// through a link that was lying there to somewhere outside it.
[[nodiscard]] core::Status stage(const Fetch& fetch, const FeedConfig& feed,
                                 const Manifest& manifest, const Plan& plan,
                                 const std::filesystem::path& stageDir, std::stop_token stop,
                                 ProgressFn progress);

// The same, through the file system given instead of the system's (makeSystemFsOps, which the
// form above uses). It is how the tests run the staging on every platform - the system's own
// file system is written for Windows only so far (fsops.hpp), so the form above answers
// write_failed "not implemented" elsewhere - and how they make a write fail. `fs` must be one
// over the real disk: a member is hashed while a stream writes it, which no FsOps call does.
[[nodiscard]] core::Status stage(const Fetch& fetch, const FeedConfig& feed,
                                 const Manifest& manifest, const Plan& plan,
                                 const std::filesystem::path& stageDir, std::stop_token stop,
                                 ProgressFn progress, FsOps& fs);

// What launchApply needs, once the helper has been copied and has proved it
// starts.
struct PreparedApply {
    std::string transaction;
    std::filesystem::path runDirectory; // U/run-<tx>
    std::filesystem::path helper;       // the copy in it
};

// The design's preflight(manifest, stageDir), with the three things it
// cannot do without: the install (for R and the installed helper), the state
// directory (for run-<tx>) and the launcher (for --selftest). Failure leaves
// no run folder behind.
[[nodiscard]] core::Result<PreparedApply>
preflight(const Manifest& manifest, const Install& install, const std::filesystem::path& stageDir,
          const std::filesystem::path& stateDir, Launcher& launcher);

// The same, through the file system given (see stage above).
[[nodiscard]] core::Result<PreparedApply>
preflight(const Manifest& manifest, const Install& install, const std::filesystem::path& stageDir,
          const std::filesystem::path& stateDir, Launcher& launcher, FsOps& fs);

} // namespace katana::update
