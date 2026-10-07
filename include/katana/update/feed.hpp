#pragma once

// Where releases are and how the engine asks for them, without linking an HTTP
// library: the transport is INJECTED. katana_app adapts gis::httpFetch (the one
// HTTP stack the project has: libcurl, retries, timeouts, the CA bundle at
// <bin>/../etc/ssl/certs, a body cap) to `Fetch`
// (src/katana_app/update_transport.cpp, T8); tests pass a fake; the engine
// never sees curl.
//
// THE FEED. No index, no mutable tag, no REST call (60 requests an hour per
// IP anonymously): the newest release's fixed-name assets,
//
//   https://github.com/<repo>/releases/latest/download/Katana-update-<target>.json
//   https://github.com/<repo>/releases/latest/download/Katana-update-<target>.json.sig
//
// and the payload zip of the release the manifest names,
//
//   https://github.com/<repo>/releases/download/<tag>/<payload.name>
//
// `latest` is whichever release `gh release create --latest` flagged, so a
// bad release is undone with `gh release edit vPREV --latest`. A feed base
// of the owner's own host replaces the first part: <base>/<fixed name>, and
// <base>/<payload.name> for the payload; it must honour `Range`.
//
// Build-time defaults, set at configure (src/katana_update/CMakeLists.txt):
//   KATANA_UPDATE_REPO   "kjarada/Katana"
//   KATANA_UPDATE_FEED   ""  (empty: GitHub, from the repo; set: that base URL)
// An empty repo AND an empty feed is a build with no auto-update: check()
// answers Unsupported with the reason "disabled_build".

#include <cstdint>
#include <functional>
#include <stop_token>
#include <string>

#include "katana/core/error.hpp"
#include "katana/update/manifest.hpp"

namespace katana::update {

struct FeedConfig {
    std::string repo;        // "owner/name"
    std::string feedBase;    // "" for GitHub; else a base URL without a trailing '/'
    std::string channel = "stable";
    std::string target;      // "win64" ... (from install.json)
    std::string userAgent;   // "Katana/<version> (<target>)": the only thing a check sends
    std::string token;       // optional read-only token; see credential.hpp. Empty: none
};

// The configure-time defaults, with `target` and the user agent filled in.
[[nodiscard]] FeedConfig defaultFeedConfig(const std::string& target);

// The three URLs above.
[[nodiscard]] std::string manifestUrl(const FeedConfig& feed);
[[nodiscard]] std::string signatureUrl(const FeedConfig& feed);
[[nodiscard]] std::string payloadUrl(const FeedConfig& feed, const Manifest& manifest);

// One request. `Fetch` returns the body or an error.
struct FetchRequest {
    std::string url;
    // 0 length: the whole body. Otherwise bytes [rangeStart, rangeStart +
    // rangeLength) by `Range: bytes=a-b`, and the answer must be exactly that
    // long (206, or a 200 whose body is the whole of a file no longer than
    // the range).
    std::uint64_t rangeStart = 0;
    std::uint64_t rangeLength = 0;
    // The most bytes to accept; the transfer is abandoned the moment it is
    // exceeded. For a range this equals rangeLength, so a server that ignores
    // `Range` and starts the whole payload cannot flood the disk or memory.
    std::uint64_t maxBytes = 0;
    // Added as `Authorization: Bearer <token>` ONLY when the transport is
    // about to talk https to exactly api.github.com, and NEVER on a redirect
    // hop: the transport follows redirects itself and drops the header there.
    // The engine fills it in only for a request to that host. Never logged.
    std::string bearerToken;
    std::string userAgent;
};

using ProgressFn = std::function<void(std::uint64_t done, std::uint64_t total)>;

// The injected transport. Failure conventions the engine reads:
//   NotFound              a 404 (a private repository answers anonymous reads with it)
//   Unsupported           the body was longer than maxBytes (a server that ignored
//                         Range): reported as "this network does not allow partial downloads"
//   InvalidState          cancelled through the stop token
//   FileImportFailure     anything else; the Error's context begins `status=<n>` when an
//                         HTTP status arrived (403, 429 ...), `status=0` otherwise
// A portal's HTML page that answers 200 is the engine's to notice: it fails
// the signature, which is checked before the body is believed.
using Fetch = std::function<core::Result<std::string>(const FetchRequest&, std::stop_token,
                                                      ProgressFn)>;

} // namespace katana::update
