#pragma once

// Finishing what a crash interrupted. recover(R) is the FIRST thing main() does
// in katana, katana_cli and katana_mcp, and what `katana_updater --recover R`
// does (the RunOnce entry points there too), so a machine that lost power or a
// helper that was killed in the middle of a swap is put right the next time
// anything starts - or by hand, when the program itself cannot start because
// its own files are half swapped.
//
//   - A live holder of R/.update/lock: an update is running now. Wait for it
//     up to kRecoverWait, then return (it finished, or it is not ours to touch).
//   - No holder and a journal in state Planned, Committing or RollingBack
//     (journal.hpp, needsRecovery): run the helper copy (U/run-<tx>/, else
//     R/bin/katana_updater) with `--recover R`, which calls recoverInProcess.
//   - Otherwise nothing: a cheap check (one directory listing) that costs a
//     start no measurable time when there is nothing to do.
//
// A recovery that fails does not end in silence: the Error's message names the command to
// run by hand, `katana_updater --recover <R>`, and the apply's last.json says
// "rollback_incomplete" (state.hpp) so the next start of the window shows it.
//
// recoverInProcess is the work itself, rolling every interrupted transaction
// back from the disk (apply.hpp, step 6); the helper's --recover mode calls it
// and the fault loops call it with a clean FsOps after a simulated crash, and
// again after a crash INSIDE a recovery.

#include <chrono>
#include <filesystem>

#include "katana/core/error.hpp"
#include "katana/update/apply.hpp"

namespace katana::update {

inline constexpr std::chrono::seconds kRecoverWait{20};

// The system seams. Returns OK when there was nothing to do, when recovery
// ran and succeeded, and when a live update holds the lock past the wait (the
// caller carries on: that is not a failure of recovery). An error only when a
// recovery was needed and could not be run or did not complete; the message
// says the by-hand command (`katana_updater --recover <R>`).
[[nodiscard]] core::Status recover(const std::filesystem::path& root);

// Roll back every transaction under R/.update whose journal needs it, in
// process. Idempotent. OK when none needed it.
[[nodiscard]] core::Status recoverInProcess(const std::filesystem::path& root,
                                            const ApplyServices& services);

} // namespace katana::update
