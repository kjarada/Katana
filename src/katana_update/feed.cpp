// The feed of include/katana/update/feed.hpp: the addresses (finished by the skeleton and kept to
// the character, tests/update/test_contract.cpp), the configure-time defaults, the reader of a
// release's asset list, and the transport's rules - redirects, the token, the answer of a range -
// over an injected function that moves one request.
//
// The build-time defaults are in place (src/katana_update/CMakeLists.txt passes them to the
// library): KATANA_UPDATE_REPO, "kjarada/Katana" unless the configure says otherwise, and
// KATANA_UPDATE_FEED, empty unless it does.

#include "katana/update/feed.hpp"

#include <algorithm>
#include <nlohmann/json.hpp>
#include <optional>
#include <utility>

#include "katana/core/text.hpp"
#include "katana/update/credential.hpp"
#include "katana/update/ranges.hpp"
#include "katana/update/version.hpp"

#if !defined(KATANA_UPDATE_REPO) || !defined(KATANA_UPDATE_FEED)
#error "KATANA_UPDATE_REPO and KATANA_UPDATE_FEED are defined by src/katana_update/CMakeLists.txt"
#endif

namespace katana::update {

namespace {

// RFC 3986 unreserved characters pass; every other byte is %XX with upper-case hex.
std::string percentEncode(std::string_view text)
{
    constexpr std::string_view kHex = "0123456789ABCDEF";
    std::string out;
    out.reserve(text.size());
    for (const char c : text) {
        const bool unreserved = (c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z') ||
                                (c >= '0' && c <= '9') || c == '-' || c == '.' || c == '_' ||
                                c == '~';
        if (unreserved) {
            out.push_back(c);
        } else {
            const auto byte = static_cast<unsigned char>(c);
            out.push_back('%');
            out.push_back(kHex[byte >> 4]);
            out.push_back(kHex[byte & 0x0F]);
        }
    }
    return out;
}

// https://github.com/<repo>/releases (the repo is the owner's own setting, not a signed
// value; it is "owner/name", whose slash must stay).
std::string githubReleases(const FeedConfig& feed)
{
    return "https://github.com/" + feed.repo + "/releases";
}

std::string apiRepo(const FeedConfig& feed)
{
    return "https://api.github.com/repos/" + feed.repo;
}

std::string latestAsset(const FeedConfig& feed, const std::string& name)
{
    if (!feed.feedBase.empty()) {
        return feed.feedBase + "/" + name;
    }
    return githubReleases(feed) + "/latest/download/" + name;
}

} // namespace

FeedConfig defaultFeedConfig(const std::string& target)
{
    FeedConfig feed;
    feed.repo = KATANA_UPDATE_REPO;
    feed.feedBase = KATANA_UPDATE_FEED;
    // "a base URL without a trailing '/'": the addresses join with one.
    while (!feed.feedBase.empty() && feed.feedBase.back() == '/') {
        feed.feedBase.pop_back();
    }
    feed.channel = "stable";
    feed.target = target;
    // The only thing a check sends about this copy; no machine name, no user, no identifier.
    feed.userAgent = "Katana/" + std::string(programVersionText()) + " (" + target + ")";
    // The token is the caller's to fill in, from the credential store (credential.hpp): it is
    // never compiled in.
    feed.route = FeedRoute::Download;
    return feed;
}

std::string manifestAssetName(const std::string& target)
{
    return "Katana-update-" + target + ".json";
}

std::string signatureAssetName(const std::string& target)
{
    return manifestAssetName(target) + ".sig";
}

std::string manifestUrl(const FeedConfig& feed)
{
    return latestAsset(feed, manifestAssetName(feed.target));
}

std::string signatureUrl(const FeedConfig& feed)
{
    return latestAsset(feed, signatureAssetName(feed.target));
}

std::string payloadUrl(const FeedConfig& feed, const Manifest& manifest)
{
    if (!manifest.payload) {
        return {};
    }
    const std::string name = percentEncode(manifest.payload->name);
    if (!feed.feedBase.empty()) {
        return feed.feedBase + "/" + name;
    }
    return githubReleases(feed) + "/download/" + percentEncode(manifest.tag) + "/" + name;
}

std::string releaseLookupUrl(const FeedConfig& feed)
{
    return apiRepo(feed) + "/releases/latest";
}

std::string releaseByTagUrl(const FeedConfig& feed, std::string_view tag)
{
    return apiRepo(feed) + "/releases/tags/" + percentEncode(tag);
}

std::string assetApiUrl(const FeedConfig& feed, std::uint64_t assetId)
{
    return apiRepo(feed) + "/releases/assets/" + std::to_string(assetId);
}

// ---- the asset list of a release ---------------------------------------------------------------

namespace {

using Json = nlohmann::json;

// A release's JSON has the release, its author, the assets and each asset's uploader: four
// levels. Sixteen is room for GitHub to add some and stops a hostile document from nesting
// the parser's stack away (the document is capped at kMaxReleaseJsonBytes, which is room for
// a million levels).
constexpr int kMaxReleaseJsonDepth = 16;
// GitHub allows 1000 assets to a release (docs/update.md cites about-releases); twice that is
// slack, and a longer list is not a release of this system.
constexpr std::size_t kMaxReleaseAssets = 2000;

core::Error badRelease(std::string what, std::string context = {})
{
    return core::makeError(core::ErrorCode::ParseFailure, std::move(what), std::move(context));
}

} // namespace

core::Result<std::vector<ReleaseAsset>> parseReleaseAssets(std::string_view json)
{
    if (json.size() > kMaxReleaseJsonBytes) {
        return badRelease("the release description is larger than this program reads",
                          std::to_string(json.size()) + " bytes");
    }
    // The lexer takes a NUL byte for the end of its input: what follows would be ignored.
    if (json.find('\0') != std::string_view::npos) {
        return badRelease("the release description holds a NUL byte");
    }

    int depth = 0;
    struct TooDeep {};
    const Json::parser_callback_t watch = [&](int, Json::parse_event_t event, Json&) {
        if (event == Json::parse_event_t::object_start ||
            event == Json::parse_event_t::array_start) {
            if (++depth > kMaxReleaseJsonDepth) {
                throw TooDeep{};
            }
        } else if (event == Json::parse_event_t::object_end ||
                   event == Json::parse_event_t::array_end) {
            --depth;
        }
        return true;
    };

    Json root;
    try {
        root = Json::parse(json, watch, /*allow_exceptions=*/true, /*ignore_comments=*/false);
    } catch (const TooDeep&) {
        return badRelease("the release description is nested too deeply");
    } catch (const Json::parse_error& error) {
        // Not error.what(): it ends with the token the reader was in.
        return badRelease("the release description is not well-formed JSON",
                          "the reader stopped at byte " + std::to_string(error.byte));
    } catch (const std::exception&) {
        return badRelease("the release description could not be read");
    }

    if (!root.is_object()) {
        return badRelease("the release description is not a JSON object");
    }
    std::vector<ReleaseAsset> assets;
    const auto listed = root.find("assets");
    if (listed == root.end()) {
        return assets;
    }
    if (!listed->is_array()) {
        return badRelease("the release's assets are not a list", "assets");
    }
    if (listed->size() > kMaxReleaseAssets) {
        return badRelease("the release lists more assets than a release of this program has",
                          std::to_string(listed->size()) + " assets");
    }
    assets.reserve(listed->size());
    std::size_t index = 0;
    for (const Json& item : *listed) {
        const std::string where = "assets[" + std::to_string(index++) + "]";
        if (!item.is_object()) {
            return badRelease("an asset is not an object", where);
        }
        const auto name = item.find("name");
        if (name == item.end() || !name->is_string() || name->get<std::string>().empty()) {
            return badRelease("an asset has no name", where);
        }
        const auto id = item.find("id");
        // An unsigned integer as written: a float, a string or a negative number is not an id.
        if (id == item.end() || !id->is_number_unsigned() || id->get<std::uint64_t>() == 0) {
            return badRelease("an asset has no id", where);
        }
        ReleaseAsset asset;
        asset.name = name->get<std::string>();
        asset.id = id->get<std::uint64_t>();
        const auto size = item.find("size");
        if (size != item.end()) {
            if (!size->is_number_unsigned()) {
                return badRelease("an asset's size is not a whole number", where);
            }
            asset.size = size->get<std::uint64_t>();
        }
        assets.push_back(std::move(asset));
    }
    return assets;
}

// ---- the transport's rules ---------------------------------------------------------------------

namespace {

bool hasForbiddenCharacter(std::string_view url)
{
    // Control characters, a space, DEL, a backslash (a browser reads it as '/', curl does not:
    // two readers of one address) and anything past ASCII: an address that went over the wire is
    // percent-encoded.
    return std::any_of(url.begin(), url.end(), [](char c) {
        const auto byte = static_cast<unsigned char>(c);
        return byte <= 0x20 || byte >= 0x7F || c == '\\';
    });
}

struct UrlParts {
    std::string scheme;           // lower case
    std::string_view authority;   // between "//" and the first '/', '?' or '#'
    std::string_view remainder;   // from that character on; may be empty
};

std::optional<UrlParts> splitUrl(std::string_view url)
{
    const std::size_t colon = url.find("://");
    if (colon == std::string_view::npos || colon == 0 || colon > 8) {
        return std::nullopt;
    }
    UrlParts parts;
    parts.scheme = core::lowered(url.substr(0, colon));
    const std::string_view rest = url.substr(colon + 3);
    const std::size_t end = rest.find_first_of("/?#");
    parts.authority = rest.substr(0, end);
    parts.remainder = end == std::string_view::npos ? std::string_view{} : rest.substr(end);
    return parts;
}

// The host of an authority without user-info: "127.0.0.1", "localhost", "[::1]" with a port
// after it or not. nullopt when there is user-info or the port is not digits.
struct HostAndPort {
    std::string host; // lower case, brackets kept
    bool hasPort = false;
};

std::optional<HostAndPort> splitAuthority(std::string_view authority)
{
    if (authority.empty() || authority.find('@') != std::string_view::npos) {
        return std::nullopt;
    }
    HostAndPort result;
    std::string_view portText;
    if (authority.front() == '[') {
        const std::size_t close = authority.find(']');
        if (close == std::string_view::npos) {
            return std::nullopt;
        }
        result.host = core::lowered(authority.substr(0, close + 1));
        const std::string_view after = authority.substr(close + 1);
        if (!after.empty()) {
            if (after.front() != ':') {
                return std::nullopt;
            }
            portText = after.substr(1);
            result.hasPort = true;
        }
    } else {
        const std::size_t colon = authority.find(':');
        result.host = core::lowered(authority.substr(0, colon));
        if (colon != std::string_view::npos) {
            portText = authority.substr(colon + 1);
            result.hasPort = true;
        }
    }
    if (result.host.empty()) {
        return std::nullopt;
    }
    if (result.hasPort) {
        if (portText.empty() || portText.size() > 5 ||
            !std::all_of(portText.begin(), portText.end(),
                         [](char c) { return c >= '0' && c <= '9'; })) {
            return std::nullopt;
        }
    }
    return result;
}

bool isLoopbackHost(const std::string& host)
{
    return host == "127.0.0.1" || host == "localhost" || host == "[::1]";
}

core::Error badRedirect(std::string_view why)
{
    return core::makeError(core::ErrorCode::FileImportFailure,
                           "the server redirected somewhere this program does not follow",
                           "reason=bad_redirect; " + std::string(why));
}

// The Error of a request that got an HTTP answer or none, its context in the one order feed.hpp
// gives: `status=<n>`, then the failure's own token when it has one, then the address without
// its query, then whatever else is worth saying. The token comes before the address because the
// address is not this program's text: a path may spell anything.
core::Error fetchError(core::ErrorCode code, std::string message, int status,
                       std::string_view url, std::string_view reason = {},
                       std::string_view detail = {})
{
    std::string context = "status=" + std::to_string(status) + "; ";
    if (!reason.empty()) {
        context += "reason=";
        context += reason;
        context += "; ";
    }
    context += displayUrl(url);
    if (!detail.empty()) {
        context += "; ";
        context += detail;
    }
    return core::makeError(code, std::move(message), std::move(context));
}

// A server's words (a header it sent) as they may stand in an Error: printable ASCII, and no
// more of them than a person reads.
std::string shownServerText(std::string_view text)
{
    constexpr std::size_t kShown = 160;
    std::string out;
    for (const char c : text.substr(0, kShown)) {
        const auto byte = static_cast<unsigned char>(c);
        out.push_back(byte >= 0x20 && byte < 0x7F ? c : '?');
    }
    if (text.size() > kShown) {
        out += "...";
    }
    return out;
}

// What checkRangeResponse refused, as an Error of a Fetch. A server that ignored `Range` keeps
// its code and its sentence, which the dialog shows. A 206 that is not the range asked for is
// given the token bad_range: its `status=206` alone would be read as "the server said 206".
core::Error rangeRefused(const core::Error& why, const FetchRequest& request,
                         std::string_view url, std::string_view contentRange)
{
    if (why.code != core::ErrorCode::FileImportFailure) {
        return why;
    }
    // Written again here, not copied from `why`: its context quotes the header as it came.
    return fetchError(core::ErrorCode::FileImportFailure, why.message, 206, url, "bad_range",
                      "asked for " + std::to_string(request.rangeLength) + " bytes from " +
                          std::to_string(request.rangeStart) + " of " +
                          std::to_string(request.payloadSize) + "; Content-Range '" +
                          shownServerText(contentRange) + "'");
}

// The request's token is in no word of an Error the Fetch returns, whoever wrote the words: the
// hop function is the caller's, and what it says about a failure is passed on.
core::Error withoutToken(core::Error error, std::string_view token)
{
    if (!token.empty()) {
        error.message = redactToken(error.message, token);
        error.context = redactToken(error.context, token);
    }
    return error;
}

core::Error cancelled(std::string_view url)
{
    return core::makeError(core::ErrorCode::InvalidState, "cancelled", displayUrl(url));
}

bool isRedirectStatus(int status)
{
    return status == 301 || status == 302 || status == 303 || status == 307 || status == 308;
}

// A person's sentence for a status that is neither a success nor a redirect.
std::string messageForStatus(int status)
{
    switch (status) {
    case 401:
        return "the server did not accept the access token (401)";
    case 403:
        return "the server refused the request (403); it may be limiting requests, or the "
               "network may block it";
    case 429:
        return "the server is limiting requests (429); try again later";
    case 416:
        return "the server does not have the part of the file that was asked for (416); the "
               "release may have been replaced";
    default:
        break;
    }
    if (status >= 500 && status < 600) {
        return "the server had a problem answering (" + std::to_string(status) + ")";
    }
    return "the server answered with an unexpected status (" + std::to_string(status) + ")";
}

bool contentTypeIsHtml(std::string_view contentType)
{
    return core::lowered(contentType).find("text/html") != std::string::npos;
}

} // namespace

bool isApiGithubUrl(std::string_view url)
{
    if (hasForbiddenCharacter(url)) {
        return false;
    }
    const std::optional<UrlParts> parts = splitUrl(url);
    // The authority must BE the host name: nothing before it (user-info), nothing after it (a
    // port, a trailing dot), nothing longer. HTTPS only, so the token never crosses in clear.
    return parts && parts->scheme == "https" && core::lowered(parts->authority) == kApiHost;
}

bool isLoopbackHttpUrl(std::string_view url)
{
    if (hasForbiddenCharacter(url)) {
        return false;
    }
    const std::optional<UrlParts> parts = splitUrl(url);
    if (!parts || parts->scheme != "http") {
        return false;
    }
    const std::optional<HostAndPort> host = splitAuthority(parts->authority);
    return host && isLoopbackHost(host->host);
}

bool isFetchableUrl(std::string_view url, bool allowLoopbackHttp)
{
    if (hasForbiddenCharacter(url)) {
        return false;
    }
    const std::optional<UrlParts> parts = splitUrl(url);
    if (!parts) {
        return false;
    }
    if (parts->scheme == "https") {
        return splitAuthority(parts->authority).has_value();
    }
    return parts->scheme == "http" && allowLoopbackHttp && isLoopbackHttpUrl(url);
}

core::Result<std::string> resolveRedirect(std::string_view from, std::string_view location)
{
    if (location.empty()) {
        return badRedirect("the answer names no Location");
    }
    if (hasForbiddenCharacter(location)) {
        return badRedirect("the Location holds a character an address cannot hold");
    }
    // A fragment is never sent to a server.
    location = location.substr(0, location.find('#'));
    if (location.empty()) {
        return badRedirect("the Location is only a fragment");
    }
    const std::optional<UrlParts> base = splitUrl(from);
    if (!base) {
        return badRedirect("the address that was redirected is not an address");
    }

    std::string target;
    if (location.starts_with("//")) {
        target = base->scheme + ":" + std::string(location);
    } else if (location.front() == '/') {
        target = base->scheme + "://" + std::string(base->authority) + std::string(location);
    } else {
        target = std::string(location);
    }

    const std::optional<UrlParts> parts = splitUrl(target);
    if (!parts || (parts->scheme != "https" && parts->scheme != "http")) {
        return badRedirect("the Location is not an http or https address");
    }
    if (!splitAuthority(parts->authority)) {
        return badRedirect("the Location names no host, or names user-info or a bad port");
    }
    // Normalise the scheme and keep everything else as the server wrote it.
    return parts->scheme + "://" + std::string(parts->authority) + std::string(parts->remainder);
}

std::vector<std::string> hopHeaders(const FetchRequest& request, std::string_view hopUrl)
{
    std::vector<std::string> headers;
    if (!request.accept.empty()) {
        headers.push_back("Accept: " + request.accept);
    }
    if (request.rangeLength > 0) {
        if (const core::Result<std::string> range =
                rangeHeaderValue(request.rangeStart, request.rangeLength)) {
            headers.push_back("Range: " + range.value());
        }
    }
    if (isApiGithubUrl(hopUrl)) {
        headers.emplace_back("X-GitHub-Api-Version: 2022-11-28");
        if (!request.bearerToken.empty()) {
            headers.push_back("Authorization: Bearer " + request.bearerToken);
        }
    }
    return headers;
}

std::string displayUrl(std::string_view url)
{
    const std::optional<UrlParts> parts = splitUrl(url);
    if (!parts) {
        return "(not an address)";
    }
    std::string_view authority = parts->authority;
    if (const std::size_t at = authority.rfind('@'); at != std::string_view::npos) {
        authority.remove_prefix(at + 1);
    }
    std::string_view path = parts->remainder;
    path = path.substr(0, path.find_first_of("?#"));
    // Bounded: an address in a message is for a person to read.
    std::string shown = parts->scheme + "://" + std::string(authority) + std::string(path);
    constexpr std::size_t kShown = 200;
    if (shown.size() > kShown) {
        shown.resize(kShown);
        shown += "...";
    }
    return shown;
}

namespace {

// One request, its redirects followed here (feed.hpp, "the transport's rules").
core::Result<std::string> followRequest(const HopFn& hop, const FetchRequest& request,
                                        const std::stop_token& stop, const ProgressFn& progress)
{
    if (!hop) {
        return core::makeError(core::ErrorCode::Internal, "no transport was given");
    }
    // The caller's bound is what keeps an unverified answer from filling memory or disk.
    if (request.maxBytes == 0) {
        return core::makeError(core::ErrorCode::InvalidArgument,
                               "a request must name the most bytes it accepts",
                               displayUrl(request.url));
    }
    const bool ranged = request.rangeLength > 0;
    if (ranged && request.payloadSize == 0) {
        return core::makeError(core::ErrorCode::InvalidArgument,
                               "a ranged request must name the signed size of the file",
                               displayUrl(request.url));
    }
    // A feed of the owner's own may be a plain-http loopback server (a test, a local mirror); a
    // request that began on the open internet is https the whole way.
    const bool allowLoopbackHttp = isLoopbackHttpUrl(request.url);
    if (!isFetchableUrl(request.url, allowLoopbackHttp)) {
        return core::makeError(core::ErrorCode::InvalidArgument,
                               "only https addresses are fetched", displayUrl(request.url));
    }

    std::string url = request.url;
    for (int redirects = 0;; ++redirects) {
        if (stop.stop_requested()) {
            return cancelled(url);
        }
        HttpHop one;
        one.url = url;
        one.userAgent = request.userAgent;
        one.headers = hopHeaders(request, url);
        one.maxBytes = request.maxBytes;
        one.timeoutSeconds = ranged ? kRangeHopTimeoutSeconds : kHopTimeoutSeconds;

        core::Result<HttpHopReply> answer = hop(one, stop, progress);
        if (!answer) {
            const core::Error& error = answer.error();
            if (error.code == core::ErrorCode::Unsupported && ranged) {
                // The body outgrew the range: a server that ignored `Range` and began the whole
                // payload.
                return fetchError(core::ErrorCode::Unsupported,
                                  std::string(kPartialDownloadsRefused), 200, url, {},
                                  "exceeded=" + std::to_string(request.maxBytes) + " bytes");
            }
            return error;
        }
        HttpHopReply& reply = answer.value();

        if (isRedirectStatus(reply.status)) {
            if (redirects >= kMaxRedirects) {
                return fetchError(core::ErrorCode::FileImportFailure,
                                  "the server redirected too many times", reply.status, url,
                                  "too_many_redirects");
            }
            core::Result<std::string> next = resolveRedirect(url, reply.location);
            if (!next) {
                return fetchError(core::ErrorCode::FileImportFailure, next.error().message,
                                  reply.status, url, "bad_redirect");
            }
            if (!isFetchableUrl(next.value(), allowLoopbackHttp)) {
                return fetchError(core::ErrorCode::FileImportFailure,
                                  "the server redirected to an address that is not https",
                                  reply.status, url, "bad_redirect");
            }
            url = std::move(next).value();
            continue;
        }

        if (reply.status == 404) {
            return fetchError(core::ErrorCode::NotFound,
                              "the server has nothing at this address (404)", 404, url);
        }
        if (reply.status != 200 && reply.status != 206) {
            return fetchError(core::ErrorCode::FileImportFailure, messageForStatus(reply.status),
                              reply.status, url);
        }
        if (contentTypeIsHtml(reply.contentType)) {
            return fetchError(core::ErrorCode::FileImportFailure,
                              "the network answered with a web page instead of the file "
                              "(a sign-in page?)",
                              reply.status, url, "html_page");
        }
        if (reply.body.size() > request.maxBytes) {
            return fetchError(core::ErrorCode::Unsupported,
                              ranged ? std::string(kPartialDownloadsRefused)
                                     : "the answer is longer than this request allows",
                              reply.status, url);
        }
        if (ranged) {
            const core::Status held =
                checkRangeResponse(reply.status, reply.contentRange, reply.body.size(),
                                   request.rangeStart, request.rangeLength, request.payloadSize);
            if (!held.ok()) {
                return rangeRefused(held.error(), request, url, reply.contentRange);
            }
        } else if (reply.status != 200) {
            return fetchError(core::ErrorCode::FileImportFailure,
                              "the server sent part of a file that was asked for whole",
                              reply.status, url);
        }
        return std::move(reply.body);
    }
}

} // namespace

Fetch makeFetch(HopFn hop)
{
    return [hop = std::move(hop)](const FetchRequest& request, std::stop_token stop,
                                  ProgressFn progress) -> core::Result<std::string> {
        core::Result<std::string> answer = followRequest(hop, request, stop, progress);
        if (!answer.ok() && !request.bearerToken.empty()) {
            return withoutToken(answer.error(), request.bearerToken);
        }
        return answer;
    };
}

std::string fetchFailureReason(const core::Error& error)
{
    // The two leading fields this program writes, `status=<n>; ` and then `reason=<token>`, and
    // nothing after them: what follows may be an address or a server's words, and must not be
    // able to name a reason.
    std::string_view rest = error.context;
    std::optional<int> status;
    constexpr std::string_view kStatus = "status=";
    if (rest.starts_with(kStatus)) {
        rest.remove_prefix(kStatus.size());
        std::size_t digits = 0;
        int number = 0;
        while (digits < rest.size() && rest[digits] >= '0' && rest[digits] <= '9' &&
               number < 10000) {
            number = number * 10 + (rest[digits] - '0');
            ++digits;
        }
        if (digits > 0) {
            status = number;
        }
        rest.remove_prefix(digits);
        constexpr std::string_view kNext = "; ";
        // Only a number and then the separator lead to a second field: `status=200x` and a
        // `status=` with no number have none.
        rest = digits > 0 && rest.starts_with(kNext) ? rest.substr(kNext.size())
                                                     : std::string_view{};
    }
    constexpr std::string_view kReason = "reason=";
    if (rest.starts_with(kReason)) {
        rest.remove_prefix(kReason.size());
        const std::string_view token = rest.substr(0, rest.find(';'));
        if (!token.empty()) {
            return std::string(token);
        }
    }

    switch (error.code) {
    case core::ErrorCode::InvalidState:
        return "cancelled";
    case core::ErrorCode::NotFound:
        return "http_404";
    case core::ErrorCode::Unsupported:
        return error.message == kPartialDownloadsRefused ? "partial_downloads" : "too_large";
    case core::ErrorCode::FileImportFailure:
        // No status at all is a failure to get an answer, like status 0.
        return status.value_or(0) == 0 ? "offline" : "http_" + std::to_string(*status);
    default:
        return "error";
    }
}

} // namespace katana::update
