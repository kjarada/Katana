#pragma once

// Asking the feed whether a newer release exists. Window only, only when
// install.json exists, by default every 24 h plus 0 to 2 h of jitter; typed
// `UPDATE CHECK` at any time.
//
// check() fetches the signature, then the manifest, verifies, and only then
// parses (trust.hpp). ANY failure to learn the answer - offline, 403, 404,
// 429, a portal's HTML, a bad signature - is the status Unreachable, not an
// error of the call: silent when the check was automatic, one sentence when a
// person asked. The Result's own error is for a call that could not even be
// made (a stop token already requested, an unusable argument).
//
// `reason` is a short machine-readable token (the verbs print it as
// `reason=`): "offline", "http_403", "http_404", "http_429", "http_<n>",
// "bad_signature", "wrong_product", "wrong_target", "malformed", "untrusted_key",
// "placeholder_keys", "partial_downloads", "cancelled", "disabled_build",
// "development_tree", "root_not_writable", "headless", "skipped",
// "blocked_before". Added by check() itself: "html_page" (a web page where the file
// should be: a captive portal), "too_large" (a file longer than the engine reads before
// it has verified it), "wrong_channel", "asset_missing" (the private route's release has no
// manifest or signature asset), "verify_unavailable" (the signature could not be checked at
// all, which says nothing about the release). fetchFailureReason (feed.hpp) makes the
// transport's tokens; `detail` carries the failure's own sentence for a person.
//
// Before the answer is known the engine asks `checkIsDue` (state.hpp) whether an
// AUTOMATIC check is due; a typed `UPDATE CHECK` asks the feed regardless.

#include <cstdint>
#include <optional>
#include <stop_token>
#include <string>
#include <string_view>

#include "katana/core/error.hpp"
#include "katana/update/feed.hpp"
#include "katana/update/install.hpp"
#include "katana/update/manifest.hpp"
#include "katana/update/state.hpp"
#include "katana/update/trust.hpp"

namespace katana::update {

enum class CheckStatus {
    Available,   // a release above the installed (version, sequence); manifest is set
    Current,     // the feed's release is not above it
    Unreachable, // the feed could not be read or did not verify; `reason` says why
    // This copy cannot update: a development tree, no feed in this build, an unwritable root.
    Unsupported,
    Blocked,     // an apply of this very release was rolled back here; not offered again
};

// "available" "current" "unreachable" "unsupported" "blocked"
[[nodiscard]] std::string_view toString(CheckStatus status);

struct CheckResult {
    CheckStatus status = CheckStatus::Unreachable;
    std::string installedVersion;
    std::string latestVersion;           // when a manifest was read, even if not newer
    std::string notesUrl;
    std::string reason;
    std::optional<Manifest> manifest;    // verified; set for Available and Current
    // The route the answer came by (feed.hpp, "THE PRIVATE-REPOSITORY ROUTE"). The
    // caller copies it into FeedConfig::route before stage(), so the payload is fetched
    // the same way the manifest was.
    FeedRoute route = FeedRoute::Download;
    // The failure's own sentence (the transport's or the verifier's message, never a URL with a
    // query and never the token), for "Could not check for updates: <detail>". Empty on success.
    // Last in the struct: it was added after the members above were published, and a member
    // added anywhere else would move them.
    std::string detail;
};

// `install` says what is installed and which target to ask for; `trust` is
// production in the programs and a test key in the tests. The feed's token, when
// the config has one, goes only to api.github.com (feed.hpp).
[[nodiscard]] core::Result<CheckResult> check(const Fetch& fetch, const FeedConfig& feed,
                                              const Install& install, const Trust& trust,
                                              std::stop_token stop);

// What the person has already decided about a release the feed offers, applied to an answer
// check() gave (check() reads no per-user file, so it is called with the state the caller read):
//   - a release whose sequence is in `state.blockedSequences` (its apply was rolled back here)
//     is Blocked, reason "blocked_before": not offered again;
//   - otherwise a release whose version is `state.skippedVersion` ("UPDATE SKIP") is Current,
//     reason "skipped". The window's automatic check applies this; a typed `UPDATE CHECK` may
//     choose to show the skipped release anyway.
// Only an Available answer changes; the manifest, latestVersion and notesUrl stay.
void applyUpdateState(CheckResult& result, const UpdateState& state);

} // namespace katana::update
