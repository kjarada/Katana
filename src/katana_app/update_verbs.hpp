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
//
// WHAT THE REPLIES ADD TO THE LINES ABOVE (update_verbs.cpp). A key is written
// only when it has a value, so a reply without reason= is one that was carried
// out, and a value is quoted when it holds a blank (cad::recordValue).
//   UPDATE CHECK      detail="<the failure's own sentence>" after reason=, and
//                     information=old before it when the release's `expires`
//                     has passed (it is offered all the same). files= and
//                     download_bytes= are there when the installed files were
//                     hashed, which is for update=available alone.
//                     reason=, beyond check.hpp's: no_transport (the context
//                     has no fetch), no_network (this build has no HTTP stack:
//                     update=unsupported), install_record (install.json does
//                     not read: update=unsupported), notify_only (available,
//                     and installed by hand).
//                     last_check is recorded when the feed ANSWERED - a
//                     manifest verified: available, current or blocked - and
//                     not for a check that could not read it, so that an
//                     automatic check that failed is due again; nor for a
//                     copy that asks nobody (unsupported). It is written into
//                     the state as that is at the moment of writing, so what
//                     another program of the install kept while the installed
//                     files were hashed - a skip, the switch, a release the
//                     helper blocked - is kept.
//   UPDATE STATUS     target=none for a development copy; last=none when no
//                     apply's outcome is recorded. A development copy is
//                     staged=none last=none whatever the update folder holds:
//                     those are an install's, and it is none. staged= is the
//                     newest release in the update folder whose two records
//                     (stage.hpp) verify and which is above what is installed;
//                     APPLY applies that one. A copy whose install record does
//                     not read (UpdateContext::installProblem) has no word of
//                     its own in kind=: the record says target=none
//                     kind=development, and a warning record after it says
//                     that the install record could not be read, and why.
//   UPDATE DOWNLOAD CONFIRM
//                     each progress line is
//                         progress bytes=<done>/<total> files=<done>/<total>
//                     (compressed bytes fetched; changed files staged). When
//                     nothing is staged the reply is CHECK's record - which
//                     says why - and then staged=none; with no update folder,
//                     staged=none reason=no_state. A download that fails is an
//                     error whose context begins reason=<token> (stage.hpp).
//                     It asks the feed first, as CHECK does (and records the
//                     check), so UpdateReply::offer is set by it too.
//   UPDATE APPLY CONFIRM
//                     refused, with reason= first in the error's context:
//                     not_staged, blocked_before, elevated, development_tree,
//                     install_record, no_state, platform, or the preflight's
//                     token (stage.hpp); and state_unreadable when the state
//                     file does not read - it is where the releases that were
//                     put back here are kept, so without it the line cannot
//                     know that this one was not, and applies nothing.
//                     elevated is a Katana that was started "as
//                     administrator" where that is the elevated half of a
//                     split token (UpdateContext::elevated; update/
//                     process.hpp, processIsElevated): the helper would run
//                     with those rights from a copy in the per-user update
//                     folder, which the same person's ordinary programs can
//                     write. It is the last of these refusals and comes
//                     before the preflight, so no helper copy is made and
//                     the release stays downloaded: Katana started the
//                     ordinary way applies it. DOWNLOAD is not refused.
//   UPDATE SKIP, UPDATE SET AUTO
//                     skipped=<v|none>; auto=on|off. With no update folder:
//                     the same with reason=no_state, and nothing is kept. With
//                     a state file that does not read: refused (InvalidState),
//                     and the file is left as it is.
//   UPDATE TOKEN STATUS | CLEAR
//                     token=present|absent, and never the token. reason=
//                     no_store, token_unusable or store_unavailable when the
//                     store could not say; after CLEAR, token=present
//                     reason=environment when KATANA_UPDATE_TOKEN still gives
//                     one, which is not this program's to remove.
//   any line          a record `warning text="..."` after the reply's own, for
//                     what did not stop the line: a state file that does not
//                     read (the defaults are used and it is not written over).
// A headless window's DOWNLOAD and APPLY are replies, staged=none or ready=none
// with reason=headless and text="..." naming who runs them; every other line a
// surface may not run is an Unsupported error whose context is reason=headless
// or reason=mcp. Where this version cannot install an update at all (it is
// written for Windows; elsewhere a release is told of and no more) DOWNLOAD and
// APPLY are Unsupported, reason=platform.
// Every reply, error and progress line passes through update::redactToken with
// the context's token and the store's.

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

// True for an UPDATE APPLY line, with its CONFIRM or without: the line a front
// end that runs many lines must run LAST (Session::isUpdateApply). The words
// are split as runUpdateLine splits them, so the line that is held to be the
// last one is exactly the line that would prepare an apply.
[[nodiscard]] bool isUpdateApplyLine(std::string_view line);

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
    // Why `install` is not what the install's own records say: install.json was
    // there and did not read (makeUpdateContext). Empty: it read, or there was
    // none. Such a copy is not called a development build, which it is not:
    // CHECK replies update=unsupported reason=install_record with this as its
    // detail, and nothing is downloaded or applied. Added after the members
    // above were published, and so after them.
    std::string installProblem;
    // This process holds the elevated half of a split token: it was started
    // "as administrator" (update::processIsElevated, which makeUpdateContext
    // asks). APPLY CONFIRM is then refused, reason=elevated, before a helper
    // copy is made. A test sets it; nothing here asks the system. Last in the
    // struct: it was added after every member above was published.
    bool elevated = false;
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
// the system credential store's token, the https transport, whether the
// process was started as administrator (`elevated`), and - unless the surface
// is HeadlessWindow or Mcp - the per-user state folder. `pid` and `relaunch`
// are left for the caller. The stores it makes live as long as the program
// (function-local statics).
[[nodiscard]] UpdateContext makeUpdateContext(UpdateSurface surface);

// The same for the program whose executable is at `executable` (an absolute
// path; update::installRootOfExecutable finds its install) instead of the
// running one: the install's records are that copy's, and everything else -
// the feed, the token, the transport, the state folder - is as above. What a
// test builds a scratch install for.
[[nodiscard]] UpdateContext makeUpdateContext(UpdateSurface surface,
                                              const std::filesystem::path& executable);

// What a caller does with an UpdateReply::apply it will NOT launch - its close
// was cancelled, the helper could not be started, a later APPLY took its
// place. UPDATE APPLY CONFIRM copied the helper to U/run-<tx> for it
// (update/stage.hpp, preflight), and only a helper that ran would have cleared
// that copy away: this lists the folder for the sweep the next start makes
// (update/state.hpp, cleanup.list). The release stays staged, and nothing else
// is touched. An error is the list's, which could not be written: the copy
// then stays, a program's size in the user's own folder.
[[nodiscard]] katana::core::Status abandonApply(const update::ApplyRequest& request);

// The lines HELP UPDATE and --help print.
[[nodiscard]] const char* updateHelpText();

} // namespace katana::app
