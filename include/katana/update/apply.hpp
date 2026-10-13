#pragma once

// The swap: replacing the install's files with the staged ones, and putting
// every file back if it goes wrong. It runs in the helper program
// (katana_updater), after the window has closed, and in-process in the tests
// through the seams (FsOps, ProcessOps, Launcher, RegistryOps).
//
// The algorithm (every step is a test, and the loops of fault_fs.hpp break it
// at each filesystem operation):
//
//  1. LOCK AND VERIFY. Take R/.update/lock (FsOps::tryLock; a live holder ->
//     Refused "locked"). Under it, first settle what an earlier swap left: one
//     that a crash cut off is put back, and one that is in place and was never
//     said to start (below, after step 7) is confirmed when its marker is there;
//     when either cannot be - or R/.update cannot be read to know - Refused
//     "interrupted_update" and nothing new starts.
//     Verify the installed manifest's signature and the
//     staged manifest's (stagedManifestPath(S), stagedSignaturePath(S): stage.hpp;
//     missing or unverified -> Refused "staged_manifest") - with THE HELPER'S OWN
//     KEYS, not the engine's answer: product and target equal install.json's,
//     isNewerThan (trust.hpp) over installed.json's pair, the path rules (paths.hpp),
//     the free space.
//  2. WAIT for the window (request.waitPid, 60 s), then for every process with
//     an image under R (ProcessOps::holdersUnder). After 30 s: Blocked, naming
//     them; the old version is relaunched and nothing was touched. NEVER kill.
//     The same question is asked once more in step 4, when the plan is made.
//  3. PLAN, changing nothing under R. Hash every manifest file in R. Equal:
//     KEEP. Else the staged file must exist and hash right, and it is COPIED
//     (not renamed: the stage folder is user-writable, and a file in R must
//     take R's ACL) into R/.update/<tx>/new/<path>. Every parent must not be a
//     reparse point and must resolve under R. REMOVE only a file listed in
//     installed.manifest.json, absent from the new manifest, whose disk hash
//     equals the old manifest's; a file nobody listed is never touched. Any
//     shortfall aborts with R untouched.
//  4. JOURNAL. Append the PLAN group (journal.hpp) with one flush. Then step 2's
//     SECOND LOOK: the plan hashed and copied for seconds, and a program started
//     in them - Katana opened again, a script's katana_cli - would go on running
//     old code over the new tree. One scan and no wait; a holder now is the
//     Blocked of step 2: the transaction's folder is deleted, the install is as
//     it was, and the stage (the download) stays for the Retry. Only then
//     COMMITTING, and the RunOnce entry (RegistryOps) is registered for
//     `<run copy>\katana_updater --recover R`.
//  5. COMMIT, renames only: the classes in the order data, lib, program, then
//     the helper's own file, then installed.json and installed.manifest.json(.sig).
//     Each file: R/p -> .update/<tx>/old/p, then .update/<tx>/new/p -> R/p.
//     Errors 5, 32, 33 retry inside FsOps::renameFile.
//  6. ROLLBACK (a failure, a timeout, recover()). Journal ROLLING_BACK, then
//     decide FROM THE DISK: each existing old/p goes back over R/p, files the
//     transaction added are deleted, directories it created go when empty;
//     journal ROLLED_BACK; relaunch the old version; write last.json. It is
//     idempotent after a crash anywhere, including inside itself. From
//     COMMITTED_PENDING_HEALTH - a health check that failed - NOTHING MOVES until
//     ROLLING_BACK is on disk: that state alone tells recovery to leave the tree,
//     so the journal must stop saying it before the tree stops being whole.
//  7. SUCCESS, once the new version has shown that it starts. Journal
//     COMMITTED_PENDING_HEALTH; then RUN `R/bin/katana --update-health <tx>` TO
//     ITS END, for up to 60 s. That run is a whole start of the window that
//     shows nothing: it writes U/healthy-<tx> on the first idle turn of its
//     event loop (about 3 s) and exits. Healthy is: the marker is there, AND
//     the run ended with 0 or was still running when the wait ended (it is then
//     stopped). A start that could not be made, that ends non-zero (a missing
//     entry point fails before main), or that never writes the marker - however
//     it ended - means rollback as in 6, and the sequence is added to `blocked`;
//     a marker such a start wrote is deleted BEFORE the rollback, so that it can
//     never be read as proof by whoever meets the transaction later.
//     Healthy: for an installer-kind install rewrite DisplayVersion and
//     DisplayName (only now: a release that is put back must never be the one
//     Settings > Apps names); journal HEALTHY; delete R/.update/<tx>, the stage
//     and the marker; what a lock keeps from being deleted goes to
//     U/cleanup.list for the next start. The folder and the marker go only once
//     HEALTHY is on disk: until then they are what the update is settled by.
//
//  If the ROLLBACK ITSELF fails (a file that stays locked, a full or failing disk),
//  the tree may be half swapped and nothing more can be done from here: the outcome is
//  RollbackIncomplete, last.json says so, and the by-hand command is
//  `katana_updater --recover <R>` (the RunOnce entry and the next start of any program
//  try it too: recover.hpp). It is never reported as RolledBack.
//
//  If THE HELPER DIES in step 7 - between COMMITTED_PENDING_HEALTH and HEALTHY: power, a
//  shutdown, the process ended - the tree is a whole new version that nobody has said
//  starts. No program's recover() touches it (it may be a live helper's, waiting for
//  that very program). It is settled, under the lock, by `katana_updater --recover`
//  (the RunOnce entry, or by hand), by the window of the new version at its first idle
//  turn, or by step 1 of the next swap: recover.hpp says which of them may do what.
//
// A program started during the commit (about a second) may fail to load; it
// is started again. That residual is stated in docs/update.md.

#include <chrono>
#include <cstdint>
#include <filesystem>
#include <span>
#include <stop_token>
#include <string>
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/update/fsops.hpp"
#include "katana/update/process.hpp"
#include "katana/update/registry.hpp"
#include "katana/update/trust.hpp"

namespace katana::update {

// The helper protocol this build speaks. A manifest's `applier` is the least
// protocol that can apply it; a helper older than that is replaced from the
// stage first (stage.hpp, preflight).
inline constexpr std::uint32_t kApplierProtocol = 1;

// What the helper is told (its command line). The window has already exited
// when it runs, after close() succeeded.
struct ApplyRequest {
    std::filesystem::path root;           // R
    std::filesystem::path stageDirectory; // S: U/stage-<sequence>
    std::filesystem::path stateDirectory; // U: last.json, state.json, healthy-<tx>
    std::filesystem::path helper;         // the preflighted copy, U/run-<tx>/katana_updater
    std::string transaction;              // T
    std::uint64_t waitPid = 0;            // the window's pid; 0: none to wait for
    std::filesystem::path relaunch;       // what to start afterwards; empty: relaunch nothing
};

// The helper's command line, without argv[0]:
//   --root R --stage S --state U --tx T --wait-pid P --relaunch <path|none>
// and its inverse. `helper` is not on the command line (it is argv[0]'s copy).
// The arguments arrive as UTF-8 (process.hpp, utf8Arguments). parseHelperArguments
// refuses (InvalidArgument) a `tx` for which isValidTransactionId is false - it names a
// folder under R/.update and U, and a command line is not trusted to name only that -
// a missing option, an unknown option and a repeated one.
[[nodiscard]] std::vector<std::string> helperArguments(const ApplyRequest& request);
[[nodiscard]] core::Result<ApplyRequest>
parseHelperArguments(std::span<const std::string> arguments);

// The switch `katana` is relaunched with to prove it starts:
// `katana --update-health <tx>` (src/katana_qt/main.cpp). It writes
// healthMarkerPath(U, tx) after its first idle turn and exits normally.
inline constexpr std::string_view kHealthSwitch = "--update-health";

// R/bin/<stem>, with ".exe" on Windows.
[[nodiscard]] std::filesystem::path installedProgram(const std::filesystem::path& root,
                                                     std::string_view stem);

// A fresh transaction id: lower-case hex and digits only, safe as a file name
// and in a command line, unique across runs.
[[nodiscard]] std::string newTransactionId();

// What an id must look like wherever one is read (the helper's command line, a folder
// name under R/.update or U): 1 to 64 characters, each 0-9 or a-f. Nothing else - no
// separator, dot, space or drive letter - so an id can never name a place outside the
// folders it is joined to. FINISHED, with tests, in src/katana_update/contract.cpp.
[[nodiscard]] bool isValidTransactionId(std::string_view text);

enum class ApplyOutcome {
    Committed, // the new version is in place and healthy
    // Processes under R did not exit, or one was started while the plan was made; nothing
    // was touched.
    Blocked,
    RolledBack, // a failure after the plan; the old version is back
    Refused,   // verification or planning failed; nothing was touched
    // A failure after the plan AND the rollback failed too: the tree may be half
    // swapped. reason "rollback_failed"; detail names `katana_updater --recover <R>`.
    RollbackIncomplete,
};

// The token last.json's `status` carries for an outcome (state.hpp, LastResult):
// "committed" "blocked" "rolled_back" "refused" "rollback_incomplete".
[[nodiscard]] constexpr std::string_view toString(ApplyOutcome outcome)
{
    switch (outcome) {
    case ApplyOutcome::Committed:
        return "committed";
    case ApplyOutcome::Blocked:
        return "blocked";
    case ApplyOutcome::RolledBack:
        return "rolled_back";
    case ApplyOutcome::Refused:
        return "refused";
    case ApplyOutcome::RollbackIncomplete:
        return "rollback_incomplete";
    }
    return "refused";
}

struct ApplyResult {
    ApplyOutcome outcome = ApplyOutcome::Refused;
    // A short machine-readable token. Everything apply() produces (src/katana_update/apply.cpp):
    //   before anything is read: "locked", "interrupted_update" (no new update starts over an
    //     earlier one that is not settled. Either that one was cut off and could not be put back
    //     - run katana_updater --recover - or it is in place and was never said to start, and
    //     its marker is not there: the window of that version settles it at its start, and so
    //     does katana_updater --recover, which may put it back. Or R/.update could not be read
    //     through, so that neither is known. `detail` says which),
    //     "invalid_request", "root_not_writable", "install_record", "development_tree";
    //   the staged release: "staged_manifest" (missing, unsigned, or refused by verifyManifest -
    //     which also covers another product, target or channel and the path rules, so there is no
    //     separate token for them), "notify_only", "applier_too_new", "downgrade";
    //   waiting for the window and the programs under R: "blocked" (the Blocked outcome, from
    //     step 2's wait and from its second look in step 4 alike), "scan_failed" (from either),
    //     "cancelled";
    //   the plan: "staged_missing", "staged_hash", "reparse_point", "path_conflict", "copy_failed",
    //     "space", "journal", "io_error", "transaction_exists";
    //   the commit and after: "rename_failed", "health_timeout", "health_exit", "rollback_failed";
    //   anywhere: "internal_error" (a std::exception from the file system or a conversion cut the
    //     run short; it was ended like any other failure at that point, and "rollback_failed"
    //     replaces it when putting the tree back threw as well).
    // And one token that apply() never produces and that stands in the same `reason=` line of
    // the helper's output: "platform". On a system where katana_updater swaps nothing it
    // refuses the swap, and --recover, before apply() is called (src/katana_updater/main.cpp,
    // refusedByThePlatform; stage() and preflight() refuse under the same token,
    // src/katana_update/stage.cpp).
    // A dialog switches on the token, never on `detail`.
    std::string reason;
    std::string detail;                    // one sentence for a person
    std::vector<RunningProgram> holders;   // Blocked: who holds the files
};

// Every wait the algorithm makes, injectable so a test does not sit for a
// minute. The defaults are the design's numbers.
struct ApplyTimings {
    std::chrono::milliseconds waitForParent{60'000};
    std::chrono::milliseconds waitForHolders{30'000};
    std::chrono::milliseconds pollInterval{250};
    std::chrono::milliseconds healthWait{60'000};
};

struct ApplyServices {
    FsOps& fs;
    ProcessOps& processes;
    Launcher& launcher;
    RegistryOps& registry;
    // The helper's trust: production in katana_updater, a test key in katana_updater_test.
    const Trust& trust;
    ApplyTimings timings{};
};

// Steps 1 to 7. Never throws except SimulatedCrash from a FaultFs. `stop`
// cancels before the commit; after COMMITTING it is ignored (a half swap is
// finished or rolled back, never abandoned).
[[nodiscard]] ApplyResult apply(const ApplyRequest& request, const ApplyServices& services,
                                std::stop_token stop);

// Step 6 alone: roll the transaction back from the disk. For a failure inside
// apply() and for recover. An error means the rollback did not finish (the tree may be
// half swapped): apply() then reports RollbackIncomplete.
[[nodiscard]] core::Status rollback(const std::filesystem::path& root,
                                    const std::string& transaction, const ApplyServices& services);

// Starts the preflighted helper copy detached, with helperArguments(request),
// and returns. The caller then exits: the helper is waiting for it. The second
// form takes the launcher, for tests.
[[nodiscard]] core::Status launchApply(const ApplyRequest& request);
[[nodiscard]] core::Status launchApply(const ApplyRequest& request, Launcher& launcher);

} // namespace katana::update
