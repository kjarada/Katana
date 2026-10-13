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
//
// WHAT RECOVERY WRITES, AND WHERE. recover() is called first by katana_cli and
// katana_mcp too, and a headless program writes no per-user state
// (docs/headless.md). So the rule, which src/katana_updater/main.cpp keeps and
// tests/update/test_helper.cpp holds it to:
//   - `katana_updater --recover <R>` - what this function runs, what the RunOnce
//     entry runs and what a person types - is given no state folder and writes
//     NOTHING per-user, whoever started it: no last.json, no state.json, no
//     cleanup.list, no health marker, and KATANA_UPDATE_STATE_DIR is not read.
//     Its only record is in the install itself, R/.update/recovery.log: a line
//     appended for each run that found something to put right
//     (`<UTC time> recover found=<tx,...> result=ok|failed left=<tx,...>`, and
//     `detail=` after a failure), and one for each journal record the rollback
//     would not follow. A run that found nothing writes nothing, and where
//     R/.update is not a folder of the install itself (a link) nothing is read
//     or written through it. When everything is back it removes the RunOnce
//     entry the interrupted swap left.
//   - last.json is written by ONE program in ONE mode: the helper run for the
//     window, the swap itself (`--state U`, apply.hpp). A recovery started from
//     katana_cli or katana_mcp therefore never leaves a "the last update ..."
//     message for the window; the window learns which version is installed from
//     the install's own records (install.hpp), and from last.json only when the
//     swap's own helper got to write one.
//   - In the calling program this function READS the state folder, to find the
//     helper copy U/run-<tx>/ (it is outside R, so it still runs when R's own
//     bin/ is half swapped) - and that is all it does there. It makes, changes
//     and deletes nothing in U, whichever program calls it: a recovery started
//     from katana_cli or katana_mcp leaves the per-user folder byte for byte
//     as it found it, the helper copy it ran included, and no cleanup.list is
//     begun. What a sweep of R/.update/<tx> could not delete is found there
//     again at the next start. The copy stays: removing it is for a program
//     that may write per-user state, and none does yet, so one helper's size
//     is left in U for each update a crash interrupted.
[[nodiscard]] core::Status recover(const std::filesystem::path& root);

// Roll back every transaction under R/.update whose journal needs it, in
// process. Idempotent. OK when none needed it.
[[nodiscard]] core::Status recoverInProcess(const std::filesystem::path& root,
                                            const ApplyServices& services);

} // namespace katana::update
