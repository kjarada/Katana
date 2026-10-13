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
// THE PRIVATE-REPOSITORY ROUTE (FeedRoute::Api), for a person who holds a read-only
// token while the repository is private. A github.com release URL does not take a
// token and answers an anonymous read of a private repository with 404, so the only
// door is the REST API, and the token goes there and nowhere else:
//
//   GET https://api.github.com/repos/<repo>/releases/latest            (releaseLookupUrl)
//   GET https://api.github.com/repos/<repo>/releases/tags/<tag>        (releaseByTagUrl)
//        Accept: application/vnd.github+json. The body is JSON; its assets[] give each
//        asset's name, numeric id and size (parseReleaseAssets).
//   GET https://api.github.com/repos/<repo>/releases/assets/<id>       (assetApiUrl)
//        Accept: application/octet-stream. The API answers 302 to the storage host;
//        the transport follows it, re-sends `Range` and drops `Authorization` there.
//
// check() starts on the Download route. Only when the signature answers NotFound, the
// config has a token and no feedBase, it retries ONCE on the Api route (releases/latest,
// then the two assets by id) and reports the route that worked in CheckResult::route;
// the caller copies it to FeedConfig::route for stage(). stage() on the Api route finds
// the payload's id through releaseByTagUrl(manifest.tag) - the release that is `latest`
// may have moved on since the check. The Api route costs REST quota (60 an hour
// anonymously, 5000 with a token), which is why it is the fallback and not the default.
// Not measured against real GitHub (redirect headers, Range through the 302): the first
// release's rehearsal does that (docs/update.md, "To verify on first release").
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
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/update/manifest.hpp"

namespace katana::update {

// Which door the engine is using; see "THE PRIVATE-REPOSITORY ROUTE".
enum class FeedRoute {
    Download, // github.com/<repo>/releases/... or the owner's feedBase: no token is ever sent
    Api,      // api.github.com: needs a token; asset ids from the release lookup
};

struct FeedConfig {
    std::string repo;        // "owner/name"
    std::string feedBase;    // "" for GitHub; else a base URL without a trailing '/'
    std::string channel = "stable";
    std::string target;      // "win64" ... (from install.json)
    std::string userAgent;   // "Katana/<version> (<target>)": the only thing a check sends
    std::string token;       // optional read-only token; see credential.hpp. Empty: none
    FeedRoute route = FeedRoute::Download; // check() sets it; stage() reads it
};

// The configure-time defaults, with `target` and the user agent filled in.
[[nodiscard]] FeedConfig defaultFeedConfig(const std::string& target);

// The three Download-route URLs above, whatever `route` says. payloadUrl is empty
// for a manifest without a payload (notify-only). Names taken from the manifest
// (the tag, the payload name) are percent-encoded, every byte outside RFC 3986's
// unreserved set (A-Z a-z 0-9 - . _ ~), so a signed value cannot add a path segment
// or a query.
[[nodiscard]] std::string manifestUrl(const FeedConfig& feed);
[[nodiscard]] std::string signatureUrl(const FeedConfig& feed);
[[nodiscard]] std::string payloadUrl(const FeedConfig& feed, const Manifest& manifest);

// The fixed asset names of one target: "Katana-update-<target>.json" and the same
// with ".sig". What the Api route looks for among a release's assets.
[[nodiscard]] std::string manifestAssetName(const std::string& target);
[[nodiscard]] std::string signatureAssetName(const std::string& target);

// The Api-route URLs (the private-repository route). They name api.github.com
// whatever `feedBase` says: the route is only taken for GitHub.
[[nodiscard]] std::string releaseLookupUrl(const FeedConfig& feed);
[[nodiscard]] std::string releaseByTagUrl(const FeedConfig& feed, std::string_view tag);
[[nodiscard]] std::string assetApiUrl(const FeedConfig& feed, std::uint64_t assetId);

// One asset of a release, as the API lists it.
struct ReleaseAsset {
    std::string name;
    std::uint64_t id = 0;
    std::uint64_t size = 0;
};

// The assets[] of a release's JSON (releaseLookupUrl / releaseByTagUrl). The JSON is
// NOT signed, so nothing in it is believed beyond locating an asset: the bytes fetched
// by id are verified like any others. ParseFailure when it is not a release object or
// an asset lacks a name or an id; an absent assets[] is an empty list.
[[nodiscard]] core::Result<std::vector<ReleaseAsset>> parseReleaseAssets(std::string_view json);

// What may be fetched before anything is verified, and so must be bounded by the
// engine and not by the server. A Fetch is never given maxBytes 0 (it answers
// InvalidArgument if it is): check() passes kMaxSignatureBytes for the signature,
// kMaxManifestBytes for the manifest and kMaxReleaseJsonBytes for a release lookup.
// kMaxManifestBytes holds kMaxFiles (paths.hpp) entries of about 400 bytes with room
// to spare; the signature line is `ed25519 <16> <88 base64>` plus a line break (115).
inline constexpr std::uint64_t kMaxSignatureBytes = 1024;
inline constexpr std::uint64_t kMaxManifestBytes = 8ull * 1024 * 1024;
inline constexpr std::uint64_t kMaxReleaseJsonBytes = 2ull * 1024 * 1024;

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
    // NEVER 0: the whole-body requests before verification use the kMax...Bytes
    // constants above. A Fetch given 0 answers InvalidArgument.
    std::uint64_t maxBytes = 0;
    // The Accept header; empty: the transport's default. The Api route sets
    // "application/vnd.github+json" for a release lookup and
    // "application/octet-stream" for an asset.
    std::string accept;
    // Added as `Authorization: Bearer <token>` ONLY when the transport is
    // about to talk https to exactly api.github.com, and NEVER on a redirect
    // hop: the transport follows redirects itself and drops the header there.
    // The engine fills it in only for a request to that host. Never logged.
    std::string bearerToken;
    std::string userAgent;
    // The signed size of the file a range is taken from (Manifest::payload->size): what a
    // `Content-Range` total and a 200 that holds the whole file are held to
    // (checkRangeResponse, ranges.hpp). Required (InvalidArgument without it) when
    // rangeLength is not 0; ignored otherwise. Last in the struct: it was added after the
    // members above were published, and a member added anywhere else would move them.
    std::uint64_t payloadSize = 0;
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

// ---- the transport's rules ---------------------------------------------------------------------
//
// What a Fetch does between the request and the bytes is the engine's, and is written here with
// no HTTP library, so that the rules that keep the token safe are tested to the letter:
// makeFetch(hop) is a Fetch over a function that moves ONE request on the wire and follows
// nothing. katana_app supplies that function from gis::httpExchange
// (src/katana_app/update_transport.cpp); a test supplies a fake that records what each hop sent.
//
//   - Redirects are followed HERE, at most kMaxRedirects, and only to https addresses (an http
//     address on the loopback only when the request itself began there: a feed of the owner's
//     own, a test). The headers are REBUILT for every hop (hopHeaders): `Range` and `Accept` on
//     every hop, and `Authorization: Bearer` on the FIRST hop alone, and there only when the
//     address the engine asked for is exactly https://api.github.com. No address a server
//     answered with is sent the token - not the signed address of the storage host, and not
//     api.github.com itself when a redirect names it: a host that is trusted with a download is
//     not thereby trusted to aim an authenticated request at a path of its choosing. (What that
//     costs: a repository that was renamed answers on api.github.com with a 301 to its new
//     address, and the hop that follows it carries no token, so a private repository then
//     answers 404. The person corrects the repository's name; whether that redirect should carry
//     the token is the owner's to rule, docs/update.md, "To verify on first release".)
//   - A 404 is NotFound; any other status that is not 200/206 is FileImportFailure with the
//     context `status=<n>`; a transport failure is `status=0` (the Fetch conventions above).
//   - An answer whose content type is HTML is a web page standing in for the file (a captive
//     portal, a proxy's sign-in): FileImportFailure, context `status=<n>; reason=html_page`,
//     however long the page is - also when it is longer than the request's maxBytes.
//   - A ranged answer is held to checkRangeResponse (ranges.hpp): 206 with the Content-Range
//     asked for, or a 200 that is the whole file when the whole file was asked for. A server that
//     ignores `Range` and starts the whole payload is cut off at maxBytes (the hop function does
//     that) and reported as Unsupported "this network does not allow partial downloads"; a 206
//     that is another part of the file, or of a file of another length, or a body of another
//     length - one that ran past the range included - is FileImportFailure, context
//     `status=206; reason=bad_range`.
//   - No address with a query string, and no header, is ever put in an Error. The Fetch writes
//     the context of every Error it returns itself, with the address through displayUrl; what
//     the hop function said about a failure is passed on only after withoutQueries, so an
//     address the hop quoted has lost its query too; and the request's token is in no word of
//     it: message and context pass through redactToken (credential.hpp).
//
// THE CONTEXT OF AN ERROR of a Fetch is written in one order, so that the token of a failure is
// read from the part this program wrote and never from an address or a server's words:
//
//   status=<n>; reason=<token>; <address without its query>; <detail>
//
// `status=` first when an HTTP answer (or none: 0) is what failed, then `reason=` when the
// failure has a token of its own, then the rest. fetchFailureReason reads those two leading
// fields and nothing after them.

inline constexpr std::string_view kApiHost = "api.github.com";
inline constexpr int kMaxRedirects = 5;
// What the hop function is given as the time for one request. A whole-body request is a few
// kilobytes: kHopTimeoutSeconds. A range is given the time its length takes on a slow line -
// mobile data at the edge of coverage, where a survey crew updates - and never less than
// kRangeHopTimeoutSeconds: rangeHopTimeoutSeconds. A transfer that has stopped moving is ended
// long before either by the hop's own low-speed limit; these bound one that only crawls.
inline constexpr int kHopTimeoutSeconds = 60;
inline constexpr int kRangeHopTimeoutSeconds = 600;
inline constexpr std::uint64_t kSlowLineBytesPerSecond = 16 * 1024;
inline constexpr int kMaxHopTimeoutSeconds = 4 * 60 * 60;

// Seconds for one ranged request of `rangeLength` bytes: kHopTimeoutSeconds to get an answer
// started plus the length at kSlowLineBytesPerSecond, rounded up - 60 + 2048 = 2108 s for the
// largest merged range (32 MiB, ranges.hpp) - and at least kRangeHopTimeoutSeconds, at most
// kMaxHopTimeoutSeconds (a member larger than a merged range is a range of its own, unsplit).
[[nodiscard]] int rangeHopTimeoutSeconds(std::uint64_t rangeLength);

// True for exactly `https://api.github.com` and a path or query after it, in any case: the
// authority is the whole text up to the first '/', '?' or '#', so a user-info prefix
// (https://api.github.com@host/), a port, a longer name (api.github.com.example.org) and a
// trailing dot are all NOT api.github.com. The one test the token passes.
[[nodiscard]] bool isApiGithubUrl(std::string_view url);

// https://..., or http://127.0.0.1, http://localhost or http://[::1] (any port) when
// `allowLoopbackHttp`. No control character, space or backslash, no user-info, a non-empty host.
[[nodiscard]] bool isFetchableUrl(std::string_view url, bool allowLoopbackHttp);
[[nodiscard]] bool isLoopbackHttpUrl(std::string_view url);

// The address a Location header names, from the address that answered with it: an absolute
// https (or http, which isFetchableUrl then judges) address, "//host/path" with the scheme of
// `from`, or "/path" on the host of `from`. The fragment is dropped. Anything else - a path
// relative to the current one, another scheme, a control character, a space, a backslash,
// user-info - is FileImportFailure "bad_redirect".
[[nodiscard]] core::Result<std::string> resolveRedirect(std::string_view from,
                                                        std::string_view location);

// The request headers for one hop to `hopUrl`, "Name: value" each. `redirected` says how the
// hop came about: false for the address the engine asked for (the first hop), true for one a
// server answered with.
//   Accept: <request.accept>                         when it is not empty
//   Range: bytes=a-b                                  for a ranged request
//   Authorization: Bearer <request.bearerToken>       only when the token is not empty AND the
//                                                     hop is not `redirected` AND
//                                                     isApiGithubUrl(hopUrl)
//   X-GitHub-Api-Version: 2022-11-28                  for a hop to api.github.com
// The token is a value in the result and nowhere else; log nothing built from it.
[[nodiscard]] std::vector<std::string> hopHeaders(const FetchRequest& request,
                                                  std::string_view hopUrl, bool redirected);

// `url` without user-info, query and fragment: scheme://host/path, for an error context or a
// log line. A signed storage address carries its signature in the query.
[[nodiscard]] std::string displayUrl(std::string_view url);

// `text` with the query and the fragment cut from every address written in it. Wherever "://"
// stands, what follows up to the next blank, control character or byte outside ASCII is taken
// for an address (none of those can be in one that went over the wire), and everything from its
// first '?' or '#' to its end is dropped:
//   "no answer from https://h/p?sig=S&se=1: timed out"  ->  "no answer from https://h/p timed out"
// It is what the words of a hop function, or of the library under it, pass through before they
// are put in an Error: they may quote the address they were given, whole or with some of its
// query hidden, and the address a release redirects to is a signed one - all of its query is
// the secret, under names no list of secret parameter names knows.
[[nodiscard]] std::string withoutQueries(std::string_view text);

// One request on the wire, redirects NOT followed.
struct HttpHop {
    std::string url;
    std::string userAgent;
    std::vector<std::string> headers; // from hopHeaders
    std::uint64_t maxBytes = 0;       // abandon the transfer past this many body bytes
    int timeoutSeconds = kHopTimeoutSeconds;
};

// What came back. Any HTTP status that arrived is a reply, 3xx and 4xx included.
struct HttpHopReply {
    int status = 0;
    std::string contentType;
    std::string location;      // the Location header, empty when none
    std::string contentRange;  // the Content-Range header, empty when none
    std::string body;
    // True when the body of a 200 or 206 passed HttpHop::maxBytes and the transfer was abandoned
    // there. `body` is then not the answer (the hop of katana_app leaves it empty) and is never
    // handed on; the status and the content type are still the answer's, and they are what tell
    // a portal's long page (html_page) from a server that ignored `Range` (a 200:
    // partial_downloads) from a 206 that ran past its range (bad_range).
    bool overran = false;
};

// Errors of a hop: InvalidState when cancelled; FileImportFailure for any failure to get an
// HTTP answer (no connection, a certificate refused, a timeout); InvalidArgument for an address
// it cannot fetch; Unsupported when the body passed maxBytes and the hop cannot say what the
// answer was (one that can returns the reply with `overran` set, which is the better report).
//
// The context of a hop's Error is the hop's own words about the failure - `status=<n>` first
// when it knows one, libcurl's sentence after it - and NOT the address: the Fetch writes the
// address itself. No context of a hop is trusted to be free of one, all the same: the Fetch
// keeps the leading status, and puts what follows it after its own address, through
// withoutQueries (and the message likewise).
using HopFn = std::function<core::Result<HttpHopReply>(const HttpHop&, std::stop_token,
                                                       ProgressFn)>;

[[nodiscard]] Fetch makeFetch(HopFn hop);

// The short machine-readable token for a failure of a Fetch (or of stage(), which adds its own
// `reason=<token>` to its contexts): the `reason=` token the Error carries (the transport's are
// "html_page", "bad_range", "bad_redirect" and "too_many_redirects"), else "cancelled",
// "http_<status>" (http_404, http_403, http_429), "offline" (status=0, or no status at all),
// "partial_downloads", "too_large"; "error" for anything else. What check()'s
// CheckResult::reason holds.
//
// A `reason=` counts only where this program writes one: at the very start of the context, or
// straight after a leading `status=<n>; `. One further on - inside an address, in a server's
// header - is text, not a token.
[[nodiscard]] std::string fetchFailureReason(const core::Error& error);

} // namespace katana::update
