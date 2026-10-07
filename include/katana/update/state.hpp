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
//   U/cleanup.list    leftovers a locked file kept from being deleted; the next start removes them
//
// Headless runs read and write no per-user place (docs/headless.md): the
// callers that are headless do not call these.

#include <cstdint>
#include <filesystem>
#include <optional>
#include <string>
#include <vector>

#include "katana/core/error.hpp"

namespace katana::update {

struct UpdateState {
    std::int64_t lastCheckEpoch = 0;       // 0: never
    bool automatic = true;                 // "Check for updates automatically" (default on)
    std::string skippedVersion;            // "UPDATE SKIP <version>": that version is not offered
    std::vector<std::uint64_t> blockedSequences; // releases whose health check failed here
};

struct LastResult {
    std::string status;  // "committed" | "rolled_back" | "blocked" | "refused"
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

} // namespace katana::update
