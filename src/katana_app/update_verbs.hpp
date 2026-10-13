// The UPDATE verbs: checking for, downloading and applying a newer Katana
// (docs/update.md). One grammar for katana_cli, the window and its headless
// form; katana_mcp reaches the read-only half through its own tool.
//
//   UPDATE CHECK
//       -> update=available|current|unreachable|unsupported|blocked
//          installed=<v> latest=<v> files=<changed>/<total> download_bytes=<n>
//          notes_url=<url> reason=<token>
//   UPDATE STATUS
//       -> installed=<v> target=<t> kind=installer|archive|development
//          staged=<v|none> skipped=<v|none> auto=on|off last_check=<UTC|never>
//          token=present|absent last=<status>[:<reason>]
//   UPDATE DOWNLOAD CONFIRM
//       -> progress lines, then staged=<v>
//   UPDATE APPLY CONFIRM
//       -> ready=<v> transaction=<tx>; the caller then ends its session and
//          starts the helper (see UpdateReply::apply)
//   UPDATE SKIP <version> | UPDATE SKIP NONE
//   UPDATE SET AUTO ON|OFF
//   UPDATE TOKEN STATUS | UPDATE TOKEN CLEAR
//
// CONFIRM is a word of the line, not a question asked back: a line without it
// is refused and says what the line with it would do. A token is never a word
// of a line - lines are logged - so there is no UPDATE TOKEN SET; the window's
// Settings page writes the credential store itself, and a script sets
// KATANA_UPDATE_TOKEN.
//
// What each surface may do (UpdateSurface):
//   Cli             everything; APPLY CONFIRM must be the last line of the run
//                   and relaunches nothing.
//   Window          everything; APPLY CONFIRM is followed by the window's own
//                   close (its save question included) and relaunches katana.
//   HeadlessWindow  CHECK and STATUS only, and only when typed: no automatic
//                   check, no per-user state written, no dialog. DOWNLOAD and
//                   APPLY reply reason=headless.
//   Mcp             CHECK and STATUS only, through the katana_check_update
//                   tool. An UPDATE line inside katana_run_commands is refused
//                   naming that tool: an agent never downloads or applies, so
//                   it never replaces the program its client holds open.
// A copy that packaging did not write (no share/katana/install.json: a build
// tree, a plain install) is kind=development: CHECK replies unsupported and
// nothing is ever downloaded or applied to it.

#pragma once

#include <cstdint>
#include <filesystem>
#include <functional>
#include <optional>
#include <stop_token>
#include <string>
#include <string_view>

#include "katana/core/error.hpp"
#include "katana/update/apply.hpp"
#include "katana/update/check.hpp"
#include "katana/update/credential.hpp"
#include "katana/update/feed.hpp"
#include "katana/update/install.hpp"
#include "katana/update/process.hpp"
#include "katana/update/state.hpp"
#include "katana/update/trust.hpp"

namespace katana::app {

// True for a line whose verb is UPDATE.
[[nodiscard]] bool isUpdateLine(std::string_view line);

enum class UpdateSurface { Cli, Window, HeadlessWindow, Mcp };

// Everything a line needs from the program that runs it. Nothing here is
// looked up behind the caller's back, so a test builds one with a fake
// transport, a test trust and a scratch folder and runs the real verbs.
struct UpdateContext {
    UpdateSurface surface = UpdateSurface::Cli;
    // What is installed (update::readInstall of the running program's root).
    update::Install install;
    // Whose signature a manifest must carry. nullptr: update::productionTrust().
    const update::Trust* trust = nullptr;
    // Where the releases are: update::defaultFeedConfig(install.target), with
    // the token of `credentials` put in by makeUpdateContext when there is one.
    update::FeedConfig feed;
    // The https transport (update_transport.hpp, makeUpdateFetch). Empty: the
    // network verbs reply update=unreachable reason=no_transport.
    update::Fetch fetch;
    // U, the per-user update folder (update::stateDirectory()). EMPTY means
    // "write no per-user state", which is what a headless run passes: CHECK
    // then neither reads nor records skipped, blocked or last_check.
    std::filesystem::path stateDirectory;
    // TOKEN STATUS and TOKEN CLEAR. nullptr: both reply reason=no_store.
    update::CredentialStore* credentials = nullptr;
    // Runs the helper's --selftest in APPLY's preflight. nullptr: the system's.
    update::Launcher* launcher = nullptr;
    // The process the helper waits for before it swaps anything: the caller's
    // own. 0: none.
    std::uint64_t pid = 0;
    // What the helper starts once the swap is done. Empty: nothing.
    std::filesystem::path relaunch;
    // Cancels a CHECK or a DOWNLOAD between files and between requests.
    std::stop_token stop;
    // Bytes fetched so far and to fetch, for a progress bar. May be empty.
    update::ProgressFn progress;
    // DOWNLOAD's progress lines as they happen, for a log. May be empty; the
    // lines are in the reply either way.
    std::function<void(std::string_view)> progressLine;
    // The time CHECK records as last_check, UTC seconds. 0: the system clock.
    std::int64_t nowEpoch = 0;
};

// What UPDATE CHECK found, for a caller that shows it rather than prints it.
struct UpdateOffer {
    update::CheckResult check;
    std::uint64_t changedFiles = 0;  // of totalFiles; 0 unless check.status is Available
    std::uint64_t totalFiles = 0;
    std::uint64_t downloadBytes = 0; // what DOWNLOAD would fetch
};

// What UPDATE STATUS says, for the same reason.
struct UpdateStatus {
    std::string installedVersion;
    std::string target;                     // "" for a development copy
    update::InstallKind kind = update::InstallKind::Development;
    std::string stagedVersion;              // "" when nothing is staged
    update::UpdateState state;              // skipped, automatic, last check; defaults when no state is kept
    bool tokenPresent = false;
    std::optional<update::LastResult> last; // the last apply's outcome, when one is recorded
};

struct UpdateReply {
    // The reply as the grammar above gives it: what katana_cli prints and the
    // window logs.
    std::string text;
    // Set by UPDATE CHECK.
    std::optional<UpdateOffer> offer;
    // Set by UPDATE STATUS.
    std::optional<UpdateStatus> status;
    // Set by UPDATE APPLY CONFIRM when the release is staged, verified and its
    // helper has passed the preflight. The line itself changes nothing in the
    // install: the CALLER ends its session first (the window after its save
    // question; katana_cli at the end of its run) and only then calls
    // update::launchApply(*apply). A caller that does not end, or whose close
    // was cancelled, simply does not call it, and the release stays staged.
    std::optional<update::ApplyRequest> apply;
};

// Runs an UPDATE line. InvalidArgument for a line the grammar does not take;
// Unsupported for a line this surface may not run (the message names the
// surface that may); otherwise a reply, whose records say what happened - a
// feed that cannot be read is a reply (update=unreachable), not an error.
[[nodiscard]] katana::core::Result<UpdateReply> runUpdateLine(const UpdateContext& context,
                                                              std::string_view line);

// The context of the running program: its install record, the default feed,
// the system credential store's token, the https transport, and - unless the
// surface is HeadlessWindow or Mcp - the per-user state folder. `pid` and
// `relaunch` are left for the caller. The stores it makes live as long as the
// program (function-local statics).
[[nodiscard]] UpdateContext makeUpdateContext(UpdateSurface surface);

// The lines HELP UPDATE and --help print.
[[nodiscard]] const char* updateHelpText();

} // namespace katana::app
