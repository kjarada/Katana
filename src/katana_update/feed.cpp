// Task T8 writes defaultFeedConfig and parseReleaseAssets of include/katana/update/feed.hpp
// (stubs below). The ADDRESSES are finished: they are the contract to the character, they
// are pure, and a review found the private-repository route missing from the header, so
// they are written and tested (tests/update/test_contract.cpp) rather than left as prose.
// T8 keeps them and writes the rest around them.
//
// The build-time defaults are in place (src/katana_update/CMakeLists.txt passes them to
// the library): KATANA_UPDATE_REPO, "kjarada/Katana" unless the configure says
// otherwise, and KATANA_UPDATE_FEED, empty unless it does.

#include "katana/update/feed.hpp"

#include "not_implemented.hpp"

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

FeedConfig defaultFeedConfig(const std::string&)
{
    detail::notYet("defaultFeedConfig", "T8");
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

core::Result<std::vector<ReleaseAsset>> parseReleaseAssets(std::string_view)
{
    return detail::notImplemented("parseReleaseAssets", "T8");
}

} // namespace katana::update
