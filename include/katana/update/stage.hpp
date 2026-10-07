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
// preflight(): copies the helper (the staged one when its manifest `applier`
// is newer than the installed helper speaks, else R/bin/katana_updater) into
// <U>/run-<tx>/ and runs `--selftest` there with the Launcher. The copy is
// what runs the swap, so at commit nothing under R is loaded and no loaded
// file is renamed; and running it now makes Defender scan it early, and proves
// it starts, before the window closes.

#include <filesystem>
#include <stop_token>
#include <string>

#include "katana/core/error.hpp"
#include "katana/update/apply.hpp"
#include "katana/update/feed.hpp"
#include "katana/update/install.hpp"
#include "katana/update/manifest.hpp"
#include "katana/update/plan.hpp"
#include "katana/update/process.hpp"

namespace katana::update {

// Fetches and stages every changed file of `plan` into `stageDir`.
// InvalidState "cancelled"; the transport's errors; InvalidArgument for a
// member that fails its hash or its size; FileExportFailure when the disk
// refuses. Progress is bytes of the compressed payload fetched over the total
// to fetch.
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
