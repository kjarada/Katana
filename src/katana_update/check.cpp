// check() of include/katana/update/check.hpp: ask the feed whether a newer release exists.
//
// The order is the point. The signature is fetched first and the manifest second, both
// bounded by the engine (feed.hpp: a Fetch is never given maxBytes 0), and NOTHING in either is
// believed until verifyManifest (trust.hpp) has checked the signature over the manifest's exact
// bytes; only then is the manifest read, and only then compared with the install.
//
// Every way of not learning the answer - offline, a 403 or 429, a 404, a captive portal's page,
// a signature that does not verify - is the STATUS Unreachable with a reason token, never an
// error of the call: the window checks silently and says nothing when it fails, and a typed
// check says one sentence. The call itself fails only when it could not be made: a stop already
// requested, an argument that is no argument.
//
// The private-repository route is the fallback and not the default (feed.hpp): it costs REST
// quota, and it is the only place a token is used. It is tried once, only when the signature on
// the public route answered 404 (which is what a private repository says to an anonymous read),
// the config holds a token and the feed is GitHub's own.

#include "katana/update/check.hpp"

#include <algorithm>
#include <utility>

#include "katana/update/version.hpp"

namespace katana::update {

namespace {

constexpr std::string_view kStable = "stable";

// An Error of this file's own, with the reason token fetchFailureReason reads back.
core::Error failure(core::ErrorCode code, std::string message, std::string_view reason,
                    std::string_view extra = {})
{
    std::string context = "reason=" + std::string(reason);
    if (!extra.empty()) {
        context += "; ";
        context += extra;
    }
    return core::makeError(code, std::move(message), std::move(context));
}

// The first non-blank character is '<': markup where a manifest ('{') or a signature line
// ('e') should be. A captive portal answers 200 with its sign-in page for any address.
bool looksLikeMarkup(std::string_view body)
{
    std::size_t at = 0;
    if (body.starts_with("\xEF\xBB\xBF")) {
        at = 3;
    }
    while (at < body.size() && (body[at] == ' ' || body[at] == '\t' || body[at] == '\r' ||
                                body[at] == '\n')) {
        ++at;
    }
    return at < body.size() && body[at] == '<';
}

CheckResult unreachable(CheckResult result, const core::Error& error)
{
    result.status = CheckStatus::Unreachable;
    result.reason = fetchFailureReason(error);
    result.detail = error.message;
    result.manifest.reset();
    return result;
}

CheckResult unreachable(CheckResult result, std::string reason, std::string detail)
{
    result.status = CheckStatus::Unreachable;
    result.reason = std::move(reason);
    result.detail = std::move(detail);
    result.manifest.reset();
    return result;
}

// What verifyManifest's failures mean to a person, worked out from the code and, where one
// code covers several, the message (trust.hpp names the code as the contract; the message tells
// "signed for another product" from "the signature does not match", which share
// InvalidArgument). Internal is "could not check" and says nothing about the release.
std::string verifyFailureReason(const core::Error& error, const Trust& trust)
{
    switch (error.code) {
    case core::ErrorCode::Internal:
        return "verify_unavailable";
    case core::ErrorCode::Unsupported:
        return trust.placeholder ? "placeholder_keys" : "untrusted_key";
    case core::ErrorCode::ParseFailure:
        return "malformed";
    case core::ErrorCode::InvalidArgument: {
        const auto says = [&](std::string_view words) {
            return error.message.find(words) != std::string::npos;
        };
        if (says("another product")) {
            return "wrong_product";
        }
        if (says("another target")) {
            return "wrong_target";
        }
        if (says("another channel")) {
            return "wrong_channel";
        }
        if (says("the signature")) {
            return "bad_signature";
        }
        // Signed, and still not a manifest this program accepts (a path rule, a size rule), or
        // an empty or oversize file.
        return "malformed";
    }
    default:
        return "bad_signature";
    }
}

struct Files {
    std::string signature;
    std::string manifest;
};

FetchRequest wholeBody(const FeedConfig& feed, std::string url, std::uint64_t maxBytes)
{
    FetchRequest request;
    request.url = std::move(url);
    request.maxBytes = maxBytes;
    request.userAgent = feed.userAgent;
    return request;
}

// Both files from the Download route: GET .sig, then .json. `signatureNotFound` is set when
// the FIRST request answered 404, the one answer that sends a token holder to the other route.
core::Result<Files> fetchFromDownloadRoute(const Fetch& fetch, const FeedConfig& feed,
                                           std::stop_token stop, bool& signatureNotFound)
{
    Files files;
    core::Result<std::string> signature =
        fetch(wholeBody(feed, signatureUrl(feed), kMaxSignatureBytes), stop, {});
    if (!signature) {
        signatureNotFound = signature.error().code == core::ErrorCode::NotFound;
        return signature.error();
    }
    files.signature = std::move(signature).value();
    core::Result<std::string> manifest =
        fetch(wholeBody(feed, manifestUrl(feed), kMaxManifestBytes), stop, {});
    if (!manifest) {
        return manifest.error();
    }
    files.manifest = std::move(manifest).value();
    return files;
}

FetchRequest apiRequest(const FeedConfig& feed, std::string url, std::string accept,
                        std::uint64_t maxBytes)
{
    FetchRequest request = wholeBody(feed, std::move(url), maxBytes);
    request.accept = std::move(accept);
    // Every address built here is api.github.com's (feed.hpp), and makeFetch sends the token
    // there only, whatever this field says.
    request.bearerToken = feed.token;
    return request;
}

// The asset `name` of the release's list.
core::Result<ReleaseAsset> findAsset(const std::vector<ReleaseAsset>& assets,
                                     const std::string& name)
{
    const auto at = std::find_if(assets.begin(), assets.end(),
                                 [&](const ReleaseAsset& asset) { return asset.name == name; });
    if (at == assets.end()) {
        return failure(core::ErrorCode::NotFound,
                       "the latest release has no asset named " + name, "asset_missing");
    }
    return *at;
}

// releases/latest, then the two assets by id (feed.hpp). The release is the one GitHub calls
// latest at this moment; stage() looks the payload up again by the manifest's tag.
core::Result<Files> fetchFromApiRoute(const Fetch& fetch, const FeedConfig& feed,
                                      std::stop_token stop)
{
    core::Result<std::string> release = fetch(
        apiRequest(feed, releaseLookupUrl(feed), "application/vnd.github+json",
                   kMaxReleaseJsonBytes),
        stop, {});
    if (!release) {
        return release.error();
    }
    core::Result<std::vector<ReleaseAsset>> assets = parseReleaseAssets(release.value());
    if (!assets) {
        return failure(core::ErrorCode::ParseFailure, assets.error().message, "malformed");
    }
    const core::Result<ReleaseAsset> signatureAsset =
        findAsset(assets.value(), signatureAssetName(feed.target));
    if (!signatureAsset) {
        return signatureAsset.error();
    }
    const core::Result<ReleaseAsset> manifestAsset =
        findAsset(assets.value(), manifestAssetName(feed.target));
    if (!manifestAsset) {
        return manifestAsset.error();
    }

    Files files;
    core::Result<std::string> signature = fetch(
        apiRequest(feed, assetApiUrl(feed, signatureAsset.value().id), "application/octet-stream",
                   kMaxSignatureBytes),
        stop, {});
    if (!signature) {
        return signature.error();
    }
    files.signature = std::move(signature).value();
    core::Result<std::string> manifest = fetch(
        apiRequest(feed, assetApiUrl(feed, manifestAsset.value().id), "application/octet-stream",
                   kMaxManifestBytes),
        stop, {});
    if (!manifest) {
        return manifest.error();
    }
    files.manifest = std::move(manifest).value();
    return files;
}

} // namespace

std::string_view toString(CheckStatus status)
{
    switch (status) {
    case CheckStatus::Available:
        return "available";
    case CheckStatus::Current:
        return "current";
    case CheckStatus::Unreachable:
        return "unreachable";
    case CheckStatus::Unsupported:
        return "unsupported";
    case CheckStatus::Blocked:
        return "blocked";
    }
    return "unreachable";
}

core::Result<CheckResult> check(const Fetch& fetch, const FeedConfig& feed, const Install& install,
                                const Trust& trust, std::stop_token stop)
{
    if (!fetch) {
        return core::makeError(core::ErrorCode::InvalidArgument, "check was given no transport");
    }
    if (feed.target.empty()) {
        return core::makeError(core::ErrorCode::InvalidArgument,
                               "the feed names no target to ask for");
    }
    // Only the stable channel exists (a beta channel is not built: docs/update.md, "Not done"),
    // and verifyManifest holds the signed channel to it.
    if (feed.channel != kStable) {
        return core::makeError(core::ErrorCode::InvalidArgument,
                               "only the stable channel exists", feed.channel);
    }

    CheckResult result;
    result.installedVersion = toString(install.version);

    // A development tree is never asked about (install.hpp): no network is touched.
    if (install.kind == InstallKind::Development) {
        result.status = CheckStatus::Unsupported;
        result.reason = "development_tree";
        result.detail = "this copy of Katana is a development build, which is not updated in place";
        return result;
    }
    if (feed.repo.empty() && feed.feedBase.empty()) {
        result.status = CheckStatus::Unsupported;
        result.reason = "disabled_build";
        result.detail = "this build of Katana has no update feed";
        return result;
    }
    if (install.target != feed.target) {
        return core::makeError(core::ErrorCode::InvalidArgument,
                               "the feed asks for another target than the install's",
                               "feed '" + feed.target + "', install '" + install.target + "'");
    }
    if (stop.stop_requested()) {
        return core::makeError(core::ErrorCode::InvalidState, "cancelled");
    }

    FeedRoute route = FeedRoute::Download;
    bool signatureNotFound = false;
    core::Result<Files> files = fetchFromDownloadRoute(fetch, feed, stop, signatureNotFound);
    if (!files && signatureNotFound && !feed.token.empty() && feed.feedBase.empty() &&
        !stop.stop_requested()) {
        // What a private repository says to an anonymous read. Once, and only for GitHub.
        route = FeedRoute::Api;
        files = fetchFromApiRoute(fetch, feed, stop);
    }
    result.route = route;
    if (!files) {
        return unreachable(std::move(result), files.error());
    }

    if (looksLikeMarkup(files.value().signature) || looksLikeMarkup(files.value().manifest)) {
        return unreachable(std::move(result), "html_page",
                           "the network answered with a web page instead of the update "
                           "information (a sign-in page?)");
    }

    core::Result<Manifest> verified =
        verifyManifest(files.value().manifest, files.value().signature, trust, install);
    if (!verified) {
        return unreachable(std::move(result), verifyFailureReason(verified.error(), trust),
                           verified.error().message);
    }

    Manifest manifest = std::move(verified).value();
    result.latestVersion = toString(manifest.version);
    result.notesUrl = manifest.notesUrl;
    // The version and the sequence together (trust.hpp, isNewerThan): a downgrade, a replay of
    // the installed build and the release a fresh install came from are all Current.
    result.status = isNewerThan(manifest, install) ? CheckStatus::Available : CheckStatus::Current;
    result.manifest = std::move(manifest);
    return result;
}

void applyUpdateState(CheckResult& result, const UpdateState& state)
{
    if (result.status != CheckStatus::Available || !result.manifest) {
        return;
    }
    const Manifest& manifest = *result.manifest;
    if (std::find(state.blockedSequences.begin(), state.blockedSequences.end(),
                  manifest.sequence) != state.blockedSequences.end()) {
        result.status = CheckStatus::Blocked;
        result.reason = "blocked_before";
        return;
    }
    if (!state.skippedVersion.empty()) {
        // As versions and not as text: "0.3" and "0.3.0" are one release.
        const core::Result<Version> skipped = parseVersion(state.skippedVersion);
        if (skipped && skipped.value() == manifest.version) {
            result.status = CheckStatus::Current;
            result.reason = "skipped";
        }
    }
}

} // namespace katana::update
