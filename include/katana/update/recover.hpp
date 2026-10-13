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
//
// AN UPDATE THAT IS INSTALLED AND WAS NEVER SAID TO START. A helper that dies while
// the new version is proving that it starts (apply.hpp, step 7: power, a shutdown,
// the process ended) leaves a journal that says COMMITTED_PENDING_HEALTH for good.
// That is none of the states above: the tree is a whole version, so recover() and
// recoverInProcess leave it exactly as it is, take no lock for it and start
// nothing - which is what keeps the health run's own recover() from waiting on,
// or starting again, the helper that started it. Three other callers settle it,
// each under the update lock, which a helper that is alive and waiting holds:
//
//   settleUnproved   `katana_updater --recover R` - the RunOnce entry at the next
//                    logon, or a person. Healthy when the marker U/healthy-<tx> is
//                    there; else PUT BACK, when the tree is still that release, its
//                    old files are all kept, U is the folder the update was applied
//                    from and no program of the install is running; else left for
//                    a start that can decide.
//   confirmStarted   the window of the new version, on its first idle turn: it IS
//                    the proof, so it writes the marker and settles it healthy.
//   the next swap    Applier::begin (apply.cpp): healthy when the marker is there;
//                    otherwise no new update is started over one that is not
//                    settled ("interrupted_update").
//
// None of them starts a program. What they may each write is said at the function.

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
//     entry runs and what a person types - writes NOTHING per-user, whoever
//     started it: no last.json, no state.json, no cleanup.list, no health
//     marker. It READS the per-user folder (stateDirectory(), so
//     KATANA_UPDATE_STATE_DIR counts) for one thing: whether the marker
//     U/healthy-<tx> of an update that was never said to start is there - and,
//     so that a marker missing from the wrong folder is not taken for one that
//     was never written, whether that update's helper copy U/run-<tx> is
//     (settleUnproved, below) - and only when the install holds such an update.
//     Its only record is in the install itself, R/.update/recovery.log: a line
//     appended for each run that found something to put right
//     (`<UTC time> recover found=<tx,...> result=ok|failed left=<tx,...>`, and
//     `detail=` after a failure), and one for each journal record the rollback
//     would not follow (`<tx> journal record refused: <record>`, the record on
//     one line). A run that found nothing writes nothing, and where
//     R/.update is not a folder of the install itself (a link) nothing is read
//     or written through it. When everything is back it removes the RunOnce
//     entry the interrupted swap left.
//     THE FORM OF THOSE LINES IS READ, by the window (the last point below
//     says what for): a transaction is named to it by a whole line whose
//     second word is `recover`, whose third is `found=<tx,...>` and whose
//     fourth begins `result=` - which the line of a refused record, its
//     second word `journal`, never is. Neither form changes without
//     updateRecoveredTransactions (src/katana_qt/update_dialog.cpp) and its
//     tests (tests/qt_widgets/test_update_dialog.cpp).
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
//     again at the next start. The copy stays, here: removing it is for the
//     program that sweeps U, and that is the window a person uses - katana_cli
//     keeps per-user state too, and sweeps nothing. Its start
//     (UpdateWorkbench::start,
//     src/katana_qt/update_workbench.cpp: on Windows, not a headless run, not
//     a development tree) reads THIS install's recovery.log, puts on
//     U/cleanup.list the copy U/run-<tx> of each transaction a recovery found
//     and whose folder R/.update/<tx> is gone, and sweeps the list
//     (listRecoveredCopies there; which transactions, and why the install's
//     log and not the names in U, is updateRecoveredTransactions',
//     src/katana_qt/update_dialog.hpp). Until that start one helper's size is
//     left in U for each update that needed a recovery - which is for as long
//     as only katana_cli and katana_mcp are started. That is an update a
//     crash interrupted (its helper died before it could list its own copy),
//     and equally one whose rollback failed and which `--recover` put right
//     later: Applier::finish (apply.cpp) keeps the copy off cleanup.list
//     after a rollback that failed, because the RunOnce entry runs
//     `--recover` from that very copy, and the recovery then lists nothing.
[[nodiscard]] core::Status recover(const std::filesystem::path& root);

// Roll back every transaction under R/.update whose journal needs it, in
// process. Idempotent. OK when none needed it. A transaction whose journal says
// COMMITTED_PENDING_HEALTH is not one of them (see the top): it is left as it
// is, and with nothing else to do no lock is taken.
[[nodiscard]] core::Status recoverInProcess(const std::filesystem::path& root,
                                            const ApplyServices& services);

// ---- settling an update that was never said to start --------------------------------------

// How settleUnproved ended. With several such updates under one install - a swap
// refuses to start beside one, so there is one at most - the answer is the first
// of RollbackIncomplete, Left, RolledBack, Healthy that fits any of them.
enum class Settled {
    // The install holds no such update, and nothing was written. Mostly without the lock being
    // asked for; also when the helper whose lock was waited for had settled its own by then.
    Nothing,
    Healthy,            // the marker was there: the update stands (its folder is deleted, or
                        // kept under a journal that says HEALTHY while a file in it is held)
    RolledBack,         // no marker, and it could be put back: the old version is in place
    // It could not be decided in this run, and it is as it was: a program of the install is
    // running; the files are no longer that release's, or the old ones it would be put back
    // from are not all kept; `evidenceDirectory` is not the folder the update was applied from
    // (it holds no U/run-<tx>), so a marker missing there proves nothing; the scan, the marker
    // or the folder could not be asked; or the lock could not be taken for a reason other than
    // a holder. In each of those the RunOnce entry is registered, so a logon comes back to it.
    // (Also the answer when the run stopped on an unexpected error, before anything was done.)
    Left,
    Busy,               // the update lock stayed held: a helper is alive. Nothing was written
    // Putting it back did not finish, or could not begin (the ROLLING_BACK record could not
    // be written, and then nothing was moved): the tree may be half swapped.
    RollbackIncomplete,
};

// What `katana_updater --recover R` does once recoverInProcess has returned OK, with
// the helper's seams. `helper` is the running helper's own file, and
// `evidenceDirectory` the per-user folder U.
//
// With no folder under R/.update whose journal says COMMITTED_PENDING_HEALTH:
// Nothing, and the lock is not asked for. Otherwise the lock is taken, polled the
// way recoverInProcess polls it (kRecoverWait, shortened by the timings); a lock
// that stays held is Busy, with nothing written, the registry included. A lock
// that cannot be opened at all is no helper's: Left, with the RunOnce entry
// registered (step 1) so that a logon comes back. Then:
//
//   1. FIRST the RunOnce entry is registered again, `<helper> --recover <R>` (the
//      text the swap registered). Windows removes an entry before it runs it, so a
//      run that lost power here would otherwise leave no logon to come back at.
//   2. Each such transaction is settled by the one rule (recover_internal.hpp,
//      settlePending), rollback allowed:
//        - U/healthy-<tx> is a file: the new version said that it starts. HEALTHY
//          is written and the folder deleted. Evidence, not assumption: it is the
//          proof the swap itself asks for - without the exit code of the health
//          run, which died with the helper.
//        - no marker, and the absence is evidence: U holds the copy the update's
//          helper ran from, U/run-<tx>, so it is the folder the marker would be
//          in. The tree is still that release, every old file the update moved
//          aside is still kept, and no program of the install is running (asked
//          before the files are hashed and again as the last thing): the update
//          is put back by rollback() (apply.hpp). Nothing is rolled back under a
//          running program.
//        - otherwise it is left, and nothing is written for it.
//   3. The RunOnce entry is removed when no transaction folder is left under
//      R/.update. Left, or a rollback that did not finish, keeps it: the next
//      logon comes here again.
//
// WHAT IT WRITES: the install (the journal, the files a rollback moves, the
// folder it deletes), the RunOnce value, and - for an installer's install that
// is settled healthy - DisplayVersion and DisplayName, the version Settings >
// Apps shows (apply.hpp, step 7: the swap writes them only once the proof is
// there, and it may have died before). U is only READ - the marker stays
// where it is - so the rule of recover() holds for the headless programs that
// start `--recover`. And what such a run CANNOT record is NOT recorded: a
// rollback made here writes no last.json and does not add the release to
// `blocked` (state.hpp), so the next check offers that release again. No
// apply-and-put-back loop follows from that: nothing applies an update but a
// person asking for it, and an update a person applies runs under a live helper,
// whose own health check blocks a release that does not start.
//
// Starts no program. Idempotent, and safe to cut off anywhere: before HEALTHY is
// on disk the next run decides again from the same evidence, after it any start
// sweeps the folder, and a rollback that was cut off is finished by every start
// (journal.hpp, needsRecovery).
[[nodiscard]] Settled settleUnproved(const std::filesystem::path& root,
                                     const std::filesystem::path& helper,
                                     const std::filesystem::path& evidenceDirectory,
                                     const ApplyServices& services);

enum class Confirmed {
    Nothing, // nothing to confirm: the install holds no update that was never said to
             // start (or its folder could not be looked at, which asking again does not mend)
    Done,    // it is settled healthy
    // Not now: the update lock is held - a helper is alive, or a scanner has the lock file
    // for a moment - or the marker or the record could not be written, or the folder could
    // not be read through. Nothing is settled, the RunOnce entry is as it was, and the caller
    // asks again later (the window: every 5 s, 12 times).
    Busy,
};

// What the window of a NEW version does on its first idle turn, when its install
// may hold an update that was never said to start. THE CALLER has made sure that
// it is that version (the install has a record of its own, and the version it
// names is the program's own: a window of the OLD files, started during the
// commit, proves nothing), that the run is interactive, and that the tree is an
// install: UpdateWorkbench::start, src/katana_qt/update_workbench.cpp.
//
// A window that has reached its first idle turn is what the health run exists to
// show, so: with no such update, Nothing. One attempt at the lock - a window does
// not wait - and Busy when it is held. Else, for each: the marker
// `stateDirectory`/healthy-<tx> is written (so a crash from here on is settled
// healthy by the next `--recover` too), the rule settles it healthy, the marker
// and the dead helper's copy U/run-<tx> are removed (what stays goes on
// U/cleanup.list), and the RunOnce entry is removed. It never rolls back, never
// starts a program, and writes per-user state: it is for an interactive window,
// never for a headless program.
[[nodiscard]] Confirmed confirmStarted(const std::filesystem::path& root,
                                       const std::filesystem::path& stateDirectory);

} // namespace katana::update
