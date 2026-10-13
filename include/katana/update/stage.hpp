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
// preflight(): copies the helper (the staged one when its manifest `applier`
// is newer than the installed helper speaks, else R/bin/katana_updater) into
// <U>/run-<tx>/ and runs `--selftest` there with the Launcher. The copy is
// what runs the swap, so at commit nothing under R is loaded and no loaded
// file is renamed; and running it now makes Defender scan it early, and proves
// it starts, before the window closes.

#include <filesystem>
#include <stop_token>
#include <string>
#include <string_view>

#include "katana/core/error.hpp"
#include "katana/update/apply.hpp"
#include "katana/update/feed.hpp"
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

// Fetches and stages every changed file of `plan` into `stageDir`, then writes the staged
// manifest and signature (see above).
// InvalidState "cancelled"; the transport's errors; InvalidArgument for a
// member that fails its hash or its size; FileExportFailure when the disk
// refuses. Progress is bytes of the compressed payload fetched over the total
// to fetch, counting what a previous run already staged as fetched.
//
// The Error of a failure carries a machine-readable token, `reason=<token>` first in
// its context (fetchFailureReason, feed.hpp, reads it back; the transport's own failures
// have theirs: http_<status>, offline, html_page, partial_downloads, cancelled):
//   staged_hash      a member inflated to bytes that are not the signed file
//   staged_corrupt   a member could not be unpacked, or is not the signed length
//   write_failed     the stage folder could not be written
//   low_disk_space   the volume has less than the plan's requiredFreeBytes (less what is staged)
//   short_range      the server sent a body that is not the range's length
//   unverified_manifest / notify_only / no_token   the call is not one stage() can serve
//   asset_missing / payload_mismatch / malformed   the private route's release listing
// preflight's: helper_missing, helper_not_started, helper_timeout, helper_failed.
[[nodiscard]] core::Status stage(const Fetch& fetch, const FeedConfig& feed,
                                 const Manifest& manifest, const Plan& plan,
                                 const std::filesystem::path& stageDir, std::stop_token stop,
                                 ProgressFn progress);

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

} // namespace katana::update
