#pragma once

// Per-user update state, in the state directory U, and nothing else: the
// install's own records are install.hpp's.
//
//   U = %LOCALAPPDATA%\Katana\update          (Windows)
//       $XDG_STATE_HOME/katana/update         (else ~/.local/state/katana/update)
//   KATANA_UPDATE_STATE_DIR, when set, is U instead - how tests and a person
//   with a roaming profile keep it elsewhere. It moves no trust (nothing in U
//   is believed without a signature check) and is read only here.
//
//   U/state.json      {"schema":1,"last_check":<epoch seconds>,"auto":true,
//                      "skipped":"0.3.0","blocked":[<sequence>,...]}
//   U/last.json       {"schema":1,"status":"rolled_back","reason":"...",
//                      "holder":"katana_mcp.exe (4412)","version":"0.3.0","tx":"..."}
//                     what the last apply ended as, shown once at the next start and then
//                     cleared
//   U/stage-<seq>/    staged files, one per changed manifest file, at their root-relative paths
//   U/run-<tx>/       the preflighted helper copy (and its self-test output)
//   U/healthy-<tx>    the marker the new version writes after its first idle turn
//   U/cleanup.list    what the next start of the window deletes: leftovers a locked file kept
//                     from being deleted, and helper copies (U/run-<tx>) that nobody will run
//                     again - the one a helper ran from, which it cannot delete itself, and
//                     those the verbs and the window list for an apply that was prepared and
//                     never started (appendCleanup, processCleanupList below)
//   U/cleanup.list.work  the list while a run works on it: processCleanupList renames the list to
//                     this name (atomic, so a helper that appends meanwhile starts a new list),
//                     acts on it, appends what must stay to the list and removes it. A run that
//                     died leaves it, and the next start finishes it first.
//
// Headless runs read and write no per-user place (docs/headless.md): the
// callers that are headless do not call these.

#include <chrono>
#include <cstdint>
#include <filesystem>
#include <optional>
#include <string>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/update/fsops.hpp"

namespace katana::update {

struct UpdateState {
    std::int64_t lastCheckEpoch = 0;       // 0: never
    bool automatic = true;                 // "Check for updates automatically" (default on)
    std::string skippedVersion;            // "UPDATE SKIP <version>": that version is not offered
    // Releases whose health check failed here, by sequence: not offered and not applied again
    // until the person empties the list ("UPDATE BLOCKED CLEAR").
    std::vector<std::uint64_t> blockedSequences;
};

struct LastResult {
    // "committed" | "rolled_back" | "blocked" | "refused" | "rollback_incomplete"
    // (toString(ApplyOutcome), apply.hpp). The last is the half-swapped tree: the next start
    // names `katana_updater --recover <R>` and runs it.
    std::string status;
    std::string reason;  // machine-readable token
    std::string holder;  // "katana_mcp.exe (4412)" for blocked; else empty
    std::string version; // the version the apply was for
    std::string transaction;
};

// U, as above. Not created.
[[nodiscard]] std::filesystem::path stateDirectory();

[[nodiscard]] std::filesystem::path stagePath(const std::filesystem::path& stateDir,
                                              std::uint64_t sequence);
[[nodiscard]] std::filesystem::path runPath(const std::filesystem::path& stateDir,
                                            const std::string& transaction);
[[nodiscard]] std::filesystem::path healthMarkerPath(const std::filesystem::path& stateDir,
                                                     const std::string& transaction);

// state.json: absent is the defaults (not an error); unreadable or from a
// newer schema is ParseFailure / Unsupported, and the caller carries on with
// defaults and says so. Written through a temporary and a rename.
[[nodiscard]] core::Result<UpdateState> readState(const std::filesystem::path& stateDir);
[[nodiscard]] core::Status writeState(const std::filesystem::path& stateDir,
                                      const UpdateState& state);

// last.json: std::nullopt when there is none.
[[nodiscard]] core::Result<std::optional<LastResult>>
readLastResult(const std::filesystem::path& stateDir);
[[nodiscard]] core::Status writeLastResult(const std::filesystem::path& stateDir,
                                           const LastResult& result);
[[nodiscard]] core::Status clearLastResult(const std::filesystem::path& stateDir);

// ---- the automatic check's clock -----------------------------------------------------------

// An automatic check is due every 24 hours plus a jitter of 0 to 2 hours, so that a
// release does not meet every copy at the same minute. The jitter is drawn ONCE per
// process by the caller (uniformly in [0, kCheckJitterMax]) and passed in, which keeps
// checkIsDue a function of its arguments.
inline constexpr std::chrono::seconds kCheckInterval{24 * 3600};
inline constexpr std::chrono::seconds kCheckJitterMax{2 * 3600};

// Whether the window's automatic check is due at `nowEpoch` (UTC seconds):
//   - never when the person turned the check off (state.automatic == false);
//   - always when there was none yet (lastCheckEpoch <= 0), and when the last one lies in
//     the FUTURE (the clock was set back: a stored time that cannot be right must not
//     silence the check until the real time catches up);
//   - otherwise when nowEpoch - lastCheckEpoch >= kCheckInterval + jitter, `jitter` being
//     clamped to [0, kCheckJitterMax].
// A typed `UPDATE CHECK` does not ask: it checks. FINISHED, with tests, in
// src/katana_update/contract.cpp: task T6 keeps it.
[[nodiscard]] bool checkIsDue(const UpdateState& state, std::int64_t nowEpoch,
                              std::chrono::seconds jitter);

// ---- U/cleanup.list --------------------------------------------------------------------------

// Appends `leftover` (an absolute path) to U/cleanup.list, one path per line, UTF-8. Two kinds
// of thing are listed. What a held lock kept the helper from deleting: the helper copy it runs
// from, a locked file under R/.update/<tx>. And a helper copy that will never be started: the
// U/run-<tx> of an apply that was prepared and then not launched (app::abandonApply,
// src/katana_app/update_verbs.hpp: katana_cli's session, and the window when its restart is
// taken back), and the one a recovery left behind, which the window lists at its start.
// Written through `fs` so the fault loops see it.
[[nodiscard]] core::Status appendCleanup(const std::filesystem::path& stateDir,
                                         const std::filesystem::path& leftover, FsOps& fs);

// Deletes what the list names and keeps on the list what could still not be deleted (a file
// still locked stays listed), working on the renamed copy U/cleanup.list.work so that an append
// that arrives meanwhile is not lost. Called by the window alone, first thing at its start
// (src/katana_qt/update_workbench.cpp): the helper only appends, and katana_cli, katana_mcp
// and a headless window never sweep, so what is listed stays until the window is next
// started. U is user-writable, so the list is NOT trusted: a line is
// acted on only when its path, resolved (FsOps::resolve), lies strictly under `stateDir`, or
// under <root>/.update, and no parent of it between that folder and the path is a reparse
// point; every other line is dropped without being touched. `root` may be empty (no install
// known: only `stateDir` is then a place the list may name). OK when the list is absent.
[[nodiscard]] core::Status processCleanupList(const std::filesystem::path& stateDir,
                                              const std::filesystem::path& root, FsOps& fs);

} // namespace katana::update
