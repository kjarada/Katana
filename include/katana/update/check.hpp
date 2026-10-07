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
// "blocked_before".

#include <cstdint>
#include <optional>
#include <stop_token>
#include <string>
#include <string_view>

#include "katana/core/error.hpp"
#include "katana/update/feed.hpp"
#include "katana/update/install.hpp"
#include "katana/update/manifest.hpp"
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
};

// `install` says what is installed and which target to ask for; `trust` is
// production in the programs and a test key in the tests. The feed's token, when
// the config has one, goes only to api.github.com (feed.hpp).
[[nodiscard]] core::Result<CheckResult> check(const Fetch& fetch, const FeedConfig& feed,
                                              const Install& install, const Trust& trust,
                                              std::stop_token stop);

} // namespace katana::update
