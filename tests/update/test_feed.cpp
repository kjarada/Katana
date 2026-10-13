// The feed (include/katana/update/feed.hpp): the configure-time defaults, the reader of a
// release's asset list, and the transport's rules - the one place the access token is held to
// "exactly https://api.github.com, on no redirect hop", the redirect limits, the portal and
// range answers, and the guarantee that no error carries the token or a signed address.
//
// The addresses themselves are tested in test_contract.cpp (UpdateFeedAddresses), which no later
// task replaces; this file does not repeat them.
//
// Where the expectations come from (CLAUDE.md, section 4): every address, header and status here
// is written out by hand from RFC 3986 (the authority ends at the first '/', '?' or '#'; '@'
// begins a host after user-info), RFC 9110 (Location, Range, Content-Range, the redirect
// statuses) and GitHub's REST documentation (the release object's assets[]), never read back
// from the code under test. The transport runs over a fake that records each hop it is given, so
// no network is touched.

#include <gtest/gtest.h>

#include <cstdint>
#include <deque>
#include <functional>
#include <optional>
#include <stop_token>
#include <string>
#include <string_view>
#include <vector>

#include "katana/update/feed.hpp"
#include "katana/update/ranges.hpp"
#include "katana/update/version.hpp"

using namespace katana;
using namespace katana::update;

#ifndef KATANA_EXPECTED_VERSION
#error "tests/update/CMakeLists.txt passes the project version as KATANA_EXPECTED_VERSION"
#endif

namespace {

constexpr std::string_view kSecretToken = "ghp_SECRET-TOKEN-0123456789";

bool contains(const std::string& text, std::string_view needle)
{
    return text.find(needle) != std::string::npos;
}

// ---- the defaults ---------------------------------------------------------------------------------

TEST(FeedDefaults, TheDefaultFeedIsTheOwnersRepositoryOnGithubWithTheUserAgentAndNoToken)
{
    // Decision D0 (docs/update.md): kjarada/Katana, no feed of the owner's own, no token compiled
    // in. A configure that changes KATANA_UPDATE_REPO or KATANA_UPDATE_FEED changes this on
    // purpose, and so changes this test.
    const FeedConfig feed = defaultFeedConfig("win64");
    EXPECT_EQ(feed.repo, "kjarada/Katana");
    EXPECT_TRUE(feed.feedBase.empty());
    EXPECT_EQ(feed.channel, "stable");
    EXPECT_EQ(feed.target, "win64");
    EXPECT_TRUE(feed.token.empty()) << "a token is never compiled in";
    EXPECT_EQ(feed.route, FeedRoute::Download);
    // "Katana/<ver> (<target>)": the only thing a check says about this copy.
    EXPECT_EQ(feed.userAgent, std::string("Katana/") + KATANA_EXPECTED_VERSION + " (win64)");
}

TEST(FeedDefaults, TheUserAgentFollowsTheTarget)
{
    EXPECT_EQ(defaultFeedConfig("linux-aarch64").userAgent,
              std::string("Katana/") + KATANA_EXPECTED_VERSION + " (linux-aarch64)");
    EXPECT_EQ(defaultFeedConfig("linux-aarch64").target, "linux-aarch64");
}

// ---- the asset list of a release --------------------------------------------------------------------

// The shape GitHub documents for a release (REST, "Get the latest release"), trimmed to what
// matters and with the members this reader must ignore left in.
constexpr std::string_view kRelease = R"({
  "url": "https://api.github.com/repos/kjarada/Katana/releases/1001",
  "tag_name": "v0.10.0",
  "draft": false,
  "author": {"login": "someone", "id": 4},
  "assets": [
    {"url": "https://api.github.com/repos/kjarada/Katana/releases/assets/7654321",
     "id": 7654321, "name": "Katana-update-win64.json", "size": 2000, "content_type": "application/json",
     "uploader": {"login": "someone", "id": 4}},
    {"id": 7654322, "name": "Katana-update-win64.json.sig", "size": 114},
    {"id": 18446744073709551615, "name": "Katana-0.10.0-win64.update.zip", "size": 4294967296}
  ]
})";

TEST(ReleaseAssets, AssetsAreReadWithTheirNamesIdsAndSizesInOrder)
{
    const auto assets = parseReleaseAssets(kRelease);
    ASSERT_TRUE(assets.ok()) << assets.error().describe();
    ASSERT_EQ(assets.value().size(), 3u);
    EXPECT_EQ(assets.value()[0].name, "Katana-update-win64.json");
    EXPECT_EQ(assets.value()[0].id, 7654321u);
    EXPECT_EQ(assets.value()[0].size, 2000u);
    EXPECT_EQ(assets.value()[1].name, "Katana-update-win64.json.sig");
    EXPECT_EQ(assets.value()[1].id, 7654322u);
    // Past 2^53 and past 2^32: exact, not a double.
    EXPECT_EQ(assets.value()[2].id, 18446744073709551615ull);
    EXPECT_EQ(assets.value()[2].size, 4294967296ull);
}

TEST(ReleaseAssets, AReleaseWithoutAssetsOrWithAnEmptyListHasNone)
{
    for (const std::string_view json :
         {std::string_view(R"({"tag_name":"v1.0.0"})"), std::string_view(R"({"assets":[]})")}) {
        const auto assets = parseReleaseAssets(json);
        ASSERT_TRUE(assets.ok()) << assets.error().describe();
        EXPECT_TRUE(assets.value().empty());
    }
}

TEST(ReleaseAssets, AnAssetWithoutASizeHasSizeZero)
{
    const auto assets = parseReleaseAssets(R"({"assets":[{"id":5,"name":"a"}]})");
    ASSERT_TRUE(assets.ok());
    EXPECT_EQ(assets.value().at(0).size, 0u);
}

TEST(ReleaseAssets, WhatIsNotAReleaseWithWellFormedAssetsIsAParseFailure)
{
    const std::vector<std::string> broken = {
        "",                                                       // nothing
        "[]",                                                     // not an object
        "\"assets\"",                                             // not an object
        "42",                                                     // not an object
        "{",                                                      // truncated
        R"({"assets":{"id":1,"name":"a"}})",                      // assets is not a list
        R"({"assets":[7]})",                                      // an asset is not an object
        R"({"assets":[{"id":1}]})",                               // no name
        R"({"assets":[{"name":"a"}]})",                           // no id
        R"({"assets":[{"id":1,"name":""}]})",                     // an empty name
        R"({"assets":[{"id":1,"name":7}]})",                      // a name that is not text
        R"({"assets":[{"id":"1","name":"a"}]})",                  // an id written as text
        R"({"assets":[{"id":1.5,"name":"a"}]})",                  // an id that is not whole
        R"({"assets":[{"id":-1,"name":"a"}]})",                   // a negative id
        R"({"assets":[{"id":0,"name":"a"}]})",                    // no asset has id 0
        R"({"assets":[{"id":1e3,"name":"a"}]})",                  // a float, however whole
        R"({"assets":[{"id":1,"name":"a","size":"big"}]})",       // a size that is not a number
        R"({"assets":[{"id":1,"name":"a","size":-4}]})",          // a negative size
    };
    for (const std::string& json : broken) {
        const auto assets = parseReleaseAssets(json);
        ASSERT_FALSE(assets.ok()) << "accepted: " << json;
        EXPECT_EQ(assets.error().code, core::ErrorCode::ParseFailure) << json;
    }
}

TEST(ReleaseAssets, ANulByteNestingTooDeepAndAnOversizeDocumentAreRefused)
{
    // The lexer would take the NUL for the end of the text and ignore what follows.
    std::string withNul = R"({"assets":[]})";
    withNul.push_back('\0');
    withNul += R"({"assets":[{"id":1,"name":"x"}]})";
    EXPECT_FALSE(parseReleaseAssets(withNul).ok());

    // 17 levels, the first past the reader's 16.
    std::string deep;
    for (int i = 0; i < 17; ++i) {
        deep += "{\"a\":";
    }
    deep += "1";
    deep.append(17, '}');
    EXPECT_FALSE(parseReleaseAssets(deep).ok());
    // 16 levels, the last that is read: the release object is level 1.
    std::string allowed;
    for (int i = 0; i < 16; ++i) {
        allowed += "{\"a\":";
    }
    allowed += "1";
    allowed.append(16, '}');
    EXPECT_TRUE(parseReleaseAssets(allowed).ok());

    // One byte past the bound the transport reads a release lookup with.
    std::string huge(kMaxReleaseJsonBytes + 1, ' ');
    EXPECT_FALSE(parseReleaseAssets(huge).ok());
}

TEST(ReleaseAssets, MoreAssetsThanAReleaseCanHaveAreRefused)
{
    // GitHub allows 1000; the reader allows 2000 and refuses 2001.
    const auto list = [](int count) {
        std::string json = R"({"assets":[)";
        for (int i = 1; i <= count; ++i) {
            if (i > 1) {
                json += ",";
            }
            json += "{\"id\":" + std::to_string(i) + ",\"name\":\"a" + std::to_string(i) + "\"}";
        }
        return json + "]}";
    };
    EXPECT_TRUE(parseReleaseAssets(list(2000)).ok());
    EXPECT_FALSE(parseReleaseAssets(list(2001)).ok());
}

// ---- the token's one destination --------------------------------------------------------------------

TEST(ApiHost, OnlyHttpsApiGithubComIsTheApiHost)
{
    EXPECT_TRUE(isApiGithubUrl("https://api.github.com/repos/o/r/releases/latest"));
    EXPECT_TRUE(isApiGithubUrl("https://api.github.com"));
    EXPECT_TRUE(isApiGithubUrl("https://api.github.com?x=1"));
    EXPECT_TRUE(isApiGithubUrl("HTTPS://API.GITHUB.COM/repos")) << "scheme and host names ignore case";

    // The look-alikes the check exists for: RFC 3986's authority runs to the first '/', '?' or
    // '#', and a '@' puts the real host after it.
    EXPECT_FALSE(isApiGithubUrl("http://api.github.com/repos")) << "never in the clear";
    EXPECT_FALSE(isApiGithubUrl("https://api.github.com@evil.example/repos"));
    EXPECT_FALSE(isApiGithubUrl("https://api.github.com:443/repos"));
    EXPECT_FALSE(isApiGithubUrl("https://api.github.com:8443/repos"));
    EXPECT_FALSE(isApiGithubUrl("https://api.github.com./repos"));
    EXPECT_FALSE(isApiGithubUrl("https://api.github.com.evil.example/repos"));
    EXPECT_FALSE(isApiGithubUrl("https://evil-api.github.com/repos"));
    EXPECT_FALSE(isApiGithubUrl("https://github.com/api.github.com"));
    EXPECT_FALSE(isApiGithubUrl("https://evil.example/https://api.github.com/"));
    EXPECT_FALSE(isApiGithubUrl("https://evil.example?x=https://api.github.com/"));
    EXPECT_FALSE(isApiGithubUrl("https://evil.example#@api.github.com/"));
    EXPECT_FALSE(isApiGithubUrl("https://release-assets.githubusercontent.com/x"));
    EXPECT_FALSE(isApiGithubUrl("https://github.com/kjarada/Katana/releases/latest/download/x"));
    EXPECT_FALSE(isApiGithubUrl("https://api.github.com\\@evil.example/"));
    EXPECT_FALSE(isApiGithubUrl("https://api.github.com /repos"));
    EXPECT_FALSE(isApiGithubUrl("https://api.github.com\r\nHost: evil/"));
    EXPECT_FALSE(isApiGithubUrl("ftp://api.github.com/"));
    EXPECT_FALSE(isApiGithubUrl("api.github.com/repos"));
    EXPECT_FALSE(isApiGithubUrl(""));
}

TEST(FetchableAddresses, HttpsAlwaysAndHttpOnlyOnTheLoopbackWhenAllowed)
{
    EXPECT_TRUE(isFetchableUrl("https://github.com/o/r/releases/latest/download/x", false));
    EXPECT_TRUE(isFetchableUrl("https://updates.example.org:8443/dir/x", false));
    EXPECT_FALSE(isFetchableUrl("http://github.com/x", false));
    EXPECT_FALSE(isFetchableUrl("http://github.com/x", true)) << "allowed only on the loopback";

    EXPECT_TRUE(isFetchableUrl("http://127.0.0.1:8080/x", true));
    EXPECT_TRUE(isFetchableUrl("http://localhost/x", true));
    EXPECT_TRUE(isFetchableUrl("http://[::1]:9/x", true));
    EXPECT_FALSE(isFetchableUrl("http://127.0.0.1:8080/x", false));
    EXPECT_FALSE(isFetchableUrl("http://127.0.0.1.evil.example/x", true));
    EXPECT_FALSE(isFetchableUrl("http://localhost@evil.example/x", true));
    EXPECT_FALSE(isFetchableUrl("http://127.0.0.1:80a/x", true)) << "a port is digits";

    EXPECT_FALSE(isFetchableUrl("ftp://github.com/x", true));
    EXPECT_FALSE(isFetchableUrl("file:///etc/passwd", true));
    EXPECT_FALSE(isFetchableUrl("https://", true));
    EXPECT_FALSE(isFetchableUrl("https:///path", true)) << "no host";
    EXPECT_FALSE(isFetchableUrl("https://user:pass@github.com/x", true)) << "no user-info";
    EXPECT_FALSE(isFetchableUrl("https://github.com/a b", true));
    EXPECT_FALSE(isFetchableUrl("https://github.com/a\\b", true));
    EXPECT_FALSE(isFetchableUrl("https://github.com/a\nb", true));
    EXPECT_FALSE(isFetchableUrl("https://github.com/\xC3\xA9", true)) << "an address is ASCII";
    EXPECT_FALSE(isFetchableUrl("", true));

    EXPECT_TRUE(isLoopbackHttpUrl("http://127.0.0.1:1/"));
    EXPECT_FALSE(isLoopbackHttpUrl("https://127.0.0.1:1/")) << "the exception is for plain http";
}

// ---- redirects ---------------------------------------------------------------------------------------

TEST(Redirects, ALocationIsResolvedAgainstTheAddressThatAnsweredWithIt)
{
    const std::string from = "https://api.github.com/repos/o/r/releases/assets/9?x=1";
    const auto resolve = [&](std::string_view location) {
        return resolveRedirect(from, location);
    };
    // Absolute: kept as the server wrote it, query and all (a signed address needs its query).
    EXPECT_EQ(resolve("https://release-assets.githubusercontent.com/a/b?sig=Zm9v&se=1").value(),
              "https://release-assets.githubusercontent.com/a/b?sig=Zm9v&se=1");
    EXPECT_EQ(resolve("HTTPS://Example.org/P").value(), "https://Example.org/P")
        << "the scheme is lower-cased, the rest is left alone";
    // Root-relative and scheme-relative (RFC 3986, 4.2).
    EXPECT_EQ(resolve("/repositories/42/releases/assets/9").value(),
              "https://api.github.com/repositories/42/releases/assets/9");
    EXPECT_EQ(resolve("//objects.example.org/blob?x=1").value(), "https://objects.example.org/blob?x=1");
    // A fragment never goes to a server.
    EXPECT_EQ(resolve("https://example.org/p#frag").value(), "https://example.org/p");
    EXPECT_EQ(resolve("/p?q=1#frag").value(), "https://api.github.com/p?q=1");
    // A plain-http address is resolved (isFetchableUrl then judges it).
    EXPECT_EQ(resolve("http://example.org/p").value(), "http://example.org/p");
    EXPECT_EQ(resolveRedirect("http://127.0.0.1:5/a", "/b").value(), "http://127.0.0.1:5/b");
}

TEST(Redirects, ALocationThatIsNotAnAddressThisProgramFollowsIsRefusedWithAReason)
{
    const std::string from = "https://api.github.com/x";
    for (const std::string_view bad : {
             "",                                   // none
             "#only-a-fragment",                   // nothing to fetch
             "relative/path",                      // relative to the current path: not followed
             "../up",                              // likewise
             "?q=1",                               // likewise
             "ftp://example.org/x",                // another scheme
             "file:///etc/passwd",
             "javascript:alert(1)",
             "https://user:pw@example.org/x",      // user-info
             "https://api.github.com@evil.example/", // the look-alike
             "https:///no-host",
             "https://example.org:port/x",
             "https://example.org/a b",            // a space
             "https://example.org/a\\b",           // a backslash
             "https://example.org/a\r\nX: y",      // a line break
             "https://example.org/\xC3\xA9",       // raw non-ASCII
         }) {
        const auto resolved = resolveRedirect(from, bad);
        ASSERT_FALSE(resolved.ok()) << "followed: " << bad;
        EXPECT_NE(resolved.error().context.find("reason=bad_redirect"), std::string::npos) << bad;
    }
}

// ---- what a hop sends -------------------------------------------------------------------------------

FetchRequest rangedRequest()
{
    FetchRequest request;
    request.url = "https://api.github.com/repos/o/r/releases/assets/9";
    request.rangeStart = 100;
    request.rangeLength = 50;
    request.payloadSize = 1000;
    request.maxBytes = 50;
    request.accept = "application/octet-stream";
    request.bearerToken = std::string(kSecretToken);
    request.userAgent = "Katana/9.9.9 (win64)";
    return request;
}

bool hasHeader(const std::vector<std::string>& headers, std::string_view line)
{
    for (const std::string& header : headers) {
        if (header == line) {
            return true;
        }
    }
    return false;
}

bool hasHeaderNamed(const std::vector<std::string>& headers, std::string_view name)
{
    for (const std::string& header : headers) {
        if (header.size() > name.size() && header.compare(0, name.size(), name) == 0 &&
            header[name.size()] == ':') {
            return true;
        }
    }
    return false;
}

TEST(HopHeaders, TheTokenIsOnTheApiHostAndOnNoOtherAddress)
{
    const FetchRequest request = rangedRequest();
    const std::string bearer = "Authorization: Bearer " + std::string(kSecretToken);

    const auto api = hopHeaders(request, "https://api.github.com/repos/o/r/releases/assets/9");
    EXPECT_TRUE(hasHeader(api, bearer));
    EXPECT_TRUE(hasHeader(api, "Accept: application/octet-stream"));
    EXPECT_TRUE(hasHeader(api, "Range: bytes=100-149")) << "inclusive last byte, RFC 9110 14.1.2";
    EXPECT_TRUE(hasHeader(api, "X-GitHub-Api-Version: 2022-11-28"));

    for (const std::string url : {
             "https://release-assets.githubusercontent.com/a/b?sig=Zm9v",
             "https://github.com/o/r/releases/download/v1/x.zip",
             "https://api.github.com.evil.example/x",
             "https://api.github.com@evil.example/x",
             "http://api.github.com/x",
             "https://api.github.com:8443/x",
             "http://127.0.0.1:8080/x",
         }) {
        const auto other = hopHeaders(request, url);
        EXPECT_FALSE(hasHeaderNamed(other, "Authorization")) << url;
        EXPECT_FALSE(hasHeaderNamed(other, "X-GitHub-Api-Version")) << url;
        // What the next hop still needs to get the same bytes.
        EXPECT_TRUE(hasHeader(other, "Accept: application/octet-stream")) << url;
        EXPECT_TRUE(hasHeader(other, "Range: bytes=100-149")) << url;
        for (const std::string& header : other) {
            EXPECT_FALSE(contains(header, kSecretToken)) << url << ": " << header;
        }
    }
}

TEST(HopHeaders, NoTokenMeansNoAuthorizationEvenOnTheApiHost)
{
    FetchRequest request = rangedRequest();
    request.bearerToken.clear();
    const auto api = hopHeaders(request, "https://api.github.com/x");
    EXPECT_FALSE(hasHeaderNamed(api, "Authorization"));
}

TEST(HopHeaders, AWholeBodyRequestHasNoRangeAndAnEmptyAcceptIsOmitted)
{
    FetchRequest request;
    request.url = "https://github.com/x";
    request.maxBytes = 10;
    const auto headers = hopHeaders(request, request.url);
    EXPECT_TRUE(headers.empty());
}

TEST(DisplayUrl, DropsTheQueryTheFragmentAndTheUserInfo)
{
    EXPECT_EQ(displayUrl("https://release-assets.githubusercontent.com/a/b?sig=SECRET&se=1#f"),
              "https://release-assets.githubusercontent.com/a/b");
    EXPECT_EQ(displayUrl("https://user:pw@example.org/p?x"), "https://example.org/p");
    EXPECT_EQ(displayUrl("HTTPS://Example.org"), "https://Example.org");
    EXPECT_EQ(displayUrl("not an address"), "(not an address)");
    const std::string longPath = "https://example.org/" + std::string(500, 'a');
    EXPECT_LT(displayUrl(longPath).size(), 220u) << "bounded: it is for a person to read";
}

// ---- the transport, over a fake that records every hop ------------------------------------------------

// What the fake wire does with one hop.
struct Reply {
    std::optional<core::Error> error;
    HttpHopReply reply;
};

Reply answer(int status, std::string body = {}, std::string contentType = {},
             std::string location = {}, std::string contentRange = {})
{
    Reply reply;
    reply.reply.status = status;
    reply.reply.body = std::move(body);
    reply.reply.contentType = std::move(contentType);
    reply.reply.location = std::move(location);
    reply.reply.contentRange = std::move(contentRange);
    return reply;
}

Reply fail(core::Error error)
{
    Reply reply;
    reply.error = std::move(error);
    return reply;
}

// A scripted wire: the n-th hop gets the n-th reply. Every hop is kept.
struct Wire {
    std::deque<Reply> script;
    std::vector<HttpHop> hops;

    HopFn hop()
    {
        return [this](const HttpHop& one, std::stop_token, ProgressFn) -> core::Result<HttpHopReply> {
            hops.push_back(one);
            if (script.empty()) {
                return core::makeError(core::ErrorCode::Internal, "the script ran out");
            }
            Reply next = std::move(script.front());
            script.pop_front();
            if (next.error) {
                return *next.error;
            }
            return next.reply;
        };
    }
};

FetchRequest apiAsset()
{
    FetchRequest request;
    request.url = "https://api.github.com/repos/o/r/releases/assets/9";
    request.accept = "application/octet-stream";
    request.bearerToken = std::string(kSecretToken);
    request.maxBytes = 1024;
    request.userAgent = "Katana/9.9.9 (win64)";
    return request;
}

constexpr std::string_view kStorage =
    "https://release-assets.githubusercontent.com/github-production-release-asset/1/uuid"
    "?sp=r&sig=SIGNATURE-IN-QUERY&se=2026-10-20T08%3A00%3A00Z";

TEST(Transport, TheTokenReachesTheApiHostOnTheFirstHopAndNotTheStorageHostItRedirectsTo)
{
    Wire wire;
    wire.script.push_back(answer(302, "", "", std::string(kStorage)));
    wire.script.push_back(answer(200, "the file bytes", "application/octet-stream"));
    const Fetch fetch = makeFetch(wire.hop());

    const core::Result<std::string> body = fetch(apiAsset(), {}, {});
    ASSERT_TRUE(body.ok()) << body.error().describe();
    EXPECT_EQ(body.value(), "the file bytes");

    ASSERT_EQ(wire.hops.size(), 2u);
    const std::string bearer = "Authorization: Bearer " + std::string(kSecretToken);
    EXPECT_EQ(wire.hops[0].url, "https://api.github.com/repos/o/r/releases/assets/9");
    EXPECT_TRUE(hasHeader(wire.hops[0].headers, bearer));
    EXPECT_TRUE(hasHeader(wire.hops[0].headers, "Accept: application/octet-stream"));
    // The signed storage address, followed exactly as written, with no token and no X-GitHub header.
    EXPECT_EQ(wire.hops[1].url, std::string(kStorage));
    EXPECT_FALSE(hasHeaderNamed(wire.hops[1].headers, "Authorization"));
    EXPECT_FALSE(hasHeaderNamed(wire.hops[1].headers, "X-GitHub-Api-Version"));
    EXPECT_TRUE(hasHeader(wire.hops[1].headers, "Accept: application/octet-stream"));
    for (const HttpHop& hop : wire.hops) {
        EXPECT_EQ(hop.userAgent, "Katana/9.9.9 (win64)");
        EXPECT_EQ(hop.maxBytes, 1024u);
    }
}

TEST(Transport, TheRangeIsSentAgainOnEveryHop)
{
    Wire wire;
    wire.script.push_back(answer(302, "", "", std::string(kStorage)));
    wire.script.push_back(answer(206, std::string(50, 'x'), "application/octet-stream", "",
                                 "bytes 100-149/1000"));
    const Fetch fetch = makeFetch(wire.hop());
    const core::Result<std::string> body = fetch(rangedRequest(), {}, {});
    ASSERT_TRUE(body.ok()) << body.error().describe();
    ASSERT_EQ(wire.hops.size(), 2u);
    for (const HttpHop& hop : wire.hops) {
        EXPECT_TRUE(hasHeader(hop.headers, "Range: bytes=100-149")) << hop.url;
        EXPECT_EQ(hop.timeoutSeconds, kRangeHopTimeoutSeconds) << "a range gets the long timeout";
    }
}

TEST(Transport, ARedirectBackToTheApiHostCarriesTheTokenAgainAndOnlyThere)
{
    // api.github.com answers a renamed repository with a 301 to itself; the rule is the host of
    // each hop, so the token goes with it - and a hop to anywhere else in the chain has none.
    Wire wire;
    wire.script.push_back(answer(301, "", "", "/repositories/42/releases/assets/9"));
    wire.script.push_back(answer(302, "", "", std::string(kStorage)));
    wire.script.push_back(answer(200, "ok"));
    const Fetch fetch = makeFetch(wire.hop());
    ASSERT_TRUE(fetch(apiAsset(), {}, {}).ok());
    ASSERT_EQ(wire.hops.size(), 3u);
    EXPECT_EQ(wire.hops[1].url, "https://api.github.com/repositories/42/releases/assets/9");
    EXPECT_TRUE(hasHeaderNamed(wire.hops[0].headers, "Authorization"));
    EXPECT_TRUE(hasHeaderNamed(wire.hops[1].headers, "Authorization"));
    EXPECT_FALSE(hasHeaderNamed(wire.hops[2].headers, "Authorization"));
}

TEST(Transport, ADownloadRouteRequestNeverCarriesATokenAnywhere)
{
    // The engine fills bearerToken only for the API host; this holds the transport to it even
    // if a caller put one on a request to github.com.
    Wire wire;
    wire.script.push_back(answer(302, "", "", std::string(kStorage)));
    wire.script.push_back(answer(200, "ok"));
    FetchRequest request = apiAsset();
    request.url = "https://github.com/o/r/releases/latest/download/Katana-update-win64.json";
    const Fetch fetch = makeFetch(wire.hop());
    ASSERT_TRUE(fetch(request, {}, {}).ok());
    for (const HttpHop& hop : wire.hops) {
        EXPECT_FALSE(hasHeaderNamed(hop.headers, "Authorization")) << hop.url;
    }
}

TEST(Transport, ARedirectChainOfFiveIsFollowedAndOneOfSixIsRefused)
{
    const auto chain = [](int redirects) {
        Wire wire;
        for (int i = 0; i < redirects; ++i) {
            wire.script.push_back(answer(302, "", "", "https://hop" + std::to_string(i) + ".example.org/x"));
        }
        wire.script.push_back(answer(200, "end"));
        const Fetch fetch = makeFetch(wire.hop());
        FetchRequest request;
        request.url = "https://github.com/x";
        request.maxBytes = 100;
        core::Result<std::string> body = fetch(request, {}, {});
        const std::size_t made = wire.hops.size(); // after the fetch: argument order is not fixed
        return std::make_pair(std::move(body), made);
    };
    const auto five = chain(5);
    ASSERT_TRUE(five.first.ok()) << five.first.error().describe();
    EXPECT_EQ(five.second, 6u);

    const auto six = chain(6);
    ASSERT_FALSE(six.first.ok());
    EXPECT_EQ(six.first.error().code, core::ErrorCode::FileImportFailure);
    EXPECT_EQ(fetchFailureReason(six.first.error()), "too_many_redirects");
    EXPECT_EQ(six.second, 6u) << "no seventh request is made";
}

TEST(Transport, ARedirectToHttpOrToAnythingButAnAddressIsRefusedWithoutBeingFollowed)
{
    for (const std::string location : {
             "http://objects.example.org/x",       // out of https
             "ftp://objects.example.org/x",
             "relative/path",
             "",
             "https://user:pw@objects.example.org/x",
         }) {
        Wire wire;
        wire.script.push_back(answer(302, "", "", location));
        wire.script.push_back(answer(200, "must not be asked for"));
        FetchRequest request = apiAsset();
        const core::Result<std::string> body = makeFetch(wire.hop())(request, {}, {});
        ASSERT_FALSE(body.ok()) << "followed: " << location;
        EXPECT_EQ(fetchFailureReason(body.error()), "bad_redirect") << location;
        EXPECT_EQ(wire.hops.size(), 1u) << location;
    }
}

TEST(Transport, PlainHttpOnTheLoopbackIsFollowedOnlyWhenTheRequestBeganThere)
{
    // A feed of the owner's own on 127.0.0.1 (a test, a local mirror), with a redirect to a
    // second port: allowed. The same redirect from an https address is not.
    {
        Wire wire;
        wire.script.push_back(answer(302, "", "", "http://127.0.0.1:50002/target"));
        wire.script.push_back(answer(200, "local"));
        FetchRequest request;
        request.url = "http://127.0.0.1:50001/start";
        request.maxBytes = 100;
        request.bearerToken = std::string(kSecretToken);
        const core::Result<std::string> body = makeFetch(wire.hop())(request, {}, {});
        ASSERT_TRUE(body.ok()) << body.error().describe();
        ASSERT_EQ(wire.hops.size(), 2u);
        EXPECT_FALSE(hasHeaderNamed(wire.hops[0].headers, "Authorization"));
        EXPECT_FALSE(hasHeaderNamed(wire.hops[1].headers, "Authorization"));
    }
    {
        Wire wire;
        wire.script.push_back(answer(302, "", "", "http://127.0.0.1:50002/target"));
        FetchRequest request;
        request.url = "https://github.com/start";
        request.maxBytes = 100;
        EXPECT_FALSE(makeFetch(wire.hop())(request, {}, {}).ok());
        EXPECT_EQ(wire.hops.size(), 1u);
    }
    {
        // A first address that is plain http off the loopback is not fetched at all.
        Wire wire;
        FetchRequest request;
        request.url = "http://updates.example.org/start";
        request.maxBytes = 100;
        const auto body = makeFetch(wire.hop())(request, {}, {});
        ASSERT_FALSE(body.ok());
        EXPECT_EQ(body.error().code, core::ErrorCode::InvalidArgument);
        EXPECT_TRUE(wire.hops.empty());
    }
}

TEST(Transport, EveryRedirectStatusIsFollowedAndEveryOtherStatusIsAFailure)
{
    for (const int status : {301, 302, 303, 307, 308}) {
        Wire wire;
        wire.script.push_back(answer(status, "", "", "https://example.org/next"));
        wire.script.push_back(answer(200, "ok"));
        FetchRequest request;
        request.url = "https://github.com/x";
        request.maxBytes = 10;
        EXPECT_TRUE(makeFetch(wire.hop())(request, {}, {}).ok()) << status;
    }
    // 300 and 304 are not redirects this program follows; they are unexpected answers.
    for (const int status : {300, 304, 100, 204}) {
        Wire wire;
        wire.script.push_back(answer(status, "", "", "https://example.org/next"));
        FetchRequest request;
        request.url = "https://github.com/x";
        request.maxBytes = 10;
        const auto body = makeFetch(wire.hop())(request, {}, {});
        ASSERT_FALSE(body.ok()) << status;
        EXPECT_EQ(fetchFailureReason(body.error()), "http_" + std::to_string(status));
    }
}

// The failures of the Fetch contract (feed.hpp) ---------------------------------------------------------

core::Result<std::string> oneRequest(Reply reply, FetchRequest request = {})
{
    if (request.url.empty()) {
        request.url = "https://github.com/o/r/releases/latest/download/x";
        request.maxBytes = 1024;
    }
    Wire wire;
    wire.script.push_back(std::move(reply));
    return makeFetch(wire.hop())(request, {}, {});
}

TEST(Transport, ANotFoundIsNotFoundAndTheOtherStatusesCarryTheirNumber)
{
    const auto missing = oneRequest(answer(404, "<html>Not Found</html>", "text/html"));
    ASSERT_FALSE(missing.ok());
    EXPECT_EQ(missing.error().code, core::ErrorCode::NotFound);
    EXPECT_EQ(fetchFailureReason(missing.error()), "http_404");
    EXPECT_TRUE(missing.error().context.starts_with("status=404")) << missing.error().context;

    for (const int status : {400, 401, 403, 410, 416, 429, 500, 502, 503}) {
        const auto refused = oneRequest(answer(status, "body"));
        ASSERT_FALSE(refused.ok()) << status;
        EXPECT_EQ(refused.error().code, core::ErrorCode::FileImportFailure) << status;
        EXPECT_TRUE(refused.error().context.starts_with("status=" + std::to_string(status)))
            << refused.error().context;
        EXPECT_EQ(fetchFailureReason(refused.error()), "http_" + std::to_string(status));
    }
}

TEST(Transport, ATransportFailureIsStatusZeroAndIsReportedOffline)
{
    const auto offline = oneRequest(fail(core::makeError(
        core::ErrorCode::FileImportFailure, "the service's host name could not be found", "status=0; x")));
    ASSERT_FALSE(offline.ok());
    EXPECT_EQ(offline.error().code, core::ErrorCode::FileImportFailure);
    EXPECT_EQ(fetchFailureReason(offline.error()), "offline");
}

TEST(Transport, ACancelledHopIsInvalidStateAndAStopBeforeTheFirstHopMakesNoRequest)
{
    const auto cancelled = oneRequest(fail(core::makeError(core::ErrorCode::InvalidState, "cancelled")));
    ASSERT_FALSE(cancelled.ok());
    EXPECT_EQ(cancelled.error().code, core::ErrorCode::InvalidState);
    EXPECT_EQ(fetchFailureReason(cancelled.error()), "cancelled");

    Wire wire;
    wire.script.push_back(answer(200, "x"));
    std::stop_source source;
    source.request_stop();
    FetchRequest request;
    request.url = "https://github.com/x";
    request.maxBytes = 10;
    const auto body = makeFetch(wire.hop())(request, source.get_token(), {});
    ASSERT_FALSE(body.ok());
    EXPECT_EQ(body.error().code, core::ErrorCode::InvalidState);
    EXPECT_TRUE(wire.hops.empty());
}

TEST(Transport, AWebPageInPlaceOfTheFileIsAPortalWhateverItsStatus)
{
    for (const std::string type : {"text/html", "text/html; charset=utf-8", "TEXT/HTML"}) {
        const auto page = oneRequest(answer(200, "<html>Sign in to the network</html>", type));
        ASSERT_FALSE(page.ok()) << type;
        EXPECT_EQ(page.error().code, core::ErrorCode::FileImportFailure);
        EXPECT_EQ(fetchFailureReason(page.error()), "html_page") << type;
    }
    // A 206 that is a web page is no part of a file.
    FetchRequest ranged = rangedRequest();
    const auto partial = oneRequest(answer(206, std::string(50, ' '), "text/html", "", "bytes 100-149/1000"), ranged);
    ASSERT_FALSE(partial.ok());
    EXPECT_EQ(fetchFailureReason(partial.error()), "html_page");
    // JSON is not.
    EXPECT_TRUE(oneRequest(answer(200, "{}", "application/json; charset=utf-8")).ok());
}

TEST(Transport, ARangeIsAnsweredByTheRangeAskedForOrItIsRefused)
{
    const std::string part(50, 'x');
    FetchRequest request = rangedRequest();
    request.url = "https://github.com/o/r/releases/download/v1/x.zip";

    EXPECT_TRUE(oneRequest(answer(206, part, "", "", "bytes 100-149/1000"), request).ok());
    EXPECT_TRUE(oneRequest(answer(206, part, "", "", "bytes 100-149/*"), request).ok())
        << "an unknown total is allowed";

    // Another part of the file, another file, no header, a short body, a long body.
    for (const auto& wrong : {
             answer(206, part, "", "", "bytes 101-150/1000"),
             answer(206, part, "", "", "bytes 100-149/999"),
             answer(206, part, "", "", ""),
             answer(206, part, "", "", "bytes 0-49/1000"),
             answer(206, part.substr(0, 49), "", "", "bytes 100-149/1000"),
         }) {
        const auto refused = oneRequest(wrong, request);
        EXPECT_FALSE(refused.ok()) << wrong.reply.contentRange;
    }
}

TEST(Transport, AServerThatIgnoresRangeIsRefusedAsPartialDownloadsWhenItsBodyWouldFloodTheRange)
{
    // The fake hop enforces maxBytes as the real one does: past it the transfer is abandoned.
    Wire wire;
    HopFn hop = [&](const HttpHop& one, std::stop_token, ProgressFn) -> core::Result<HttpHopReply> {
        wire.hops.push_back(one);
        constexpr std::uint64_t kWholeFile = 1000; // answered whole, with a 200
        if (kWholeFile > one.maxBytes) {
            return core::makeError(core::ErrorCode::Unsupported, "the answer is larger than allowed");
        }
        HttpHopReply reply;
        reply.status = 200;
        reply.body.assign(kWholeFile, 'z');
        return reply;
    };
    FetchRequest request = rangedRequest();
    request.url = "https://github.com/o/r/releases/download/v1/x.zip";
    const auto body = makeFetch(hop)(request, {}, {});
    ASSERT_FALSE(body.ok());
    EXPECT_EQ(body.error().code, core::ErrorCode::Unsupported);
    EXPECT_EQ(body.error().message, std::string(kPartialDownloadsRefused));
    EXPECT_EQ(fetchFailureReason(body.error()), "partial_downloads");
    ASSERT_EQ(wire.hops.size(), 1u);
    EXPECT_EQ(wire.hops[0].maxBytes, 50u) << "the hop was cut off at the range length";
}

TEST(Transport, ATwoHundredThatIsNotTheWholeFileAskedForIsPartialDownloadsToo)
{
    // A server that ignores Range for a file shorter than the cut-off: the body fits maxBytes,
    // and is still not bytes 100-149.
    FetchRequest request = rangedRequest();
    request.url = "https://github.com/o/r/releases/download/v1/x.zip";
    request.rangeStart = 0;
    request.rangeLength = 40;
    request.payloadSize = 1000;
    request.maxBytes = 40;
    const auto notTheRange = oneRequest(answer(200, std::string(40, 'q')), request);
    ASSERT_FALSE(notTheRange.ok());
    EXPECT_EQ(fetchFailureReason(notTheRange.error()), "partial_downloads");

    // The whole file asked for as a range is a 200 that is all of it.
    request.rangeLength = 1000;
    request.maxBytes = 1000;
    EXPECT_TRUE(oneRequest(answer(200, std::string(1000, 'q')), request).ok());
}

TEST(Transport, AWholeBodyRequestWantsA200AndTheEngineSBound)
{
    FetchRequest request;
    request.url = "https://github.com/x";
    request.maxBytes = 10;
    EXPECT_TRUE(oneRequest(answer(200, std::string(10, 'a')), request).ok());
    EXPECT_FALSE(oneRequest(answer(206, "abc", "", "", "bytes 0-2/10"), request).ok())
        << "a part of a file that was asked for whole";
    // A hop that does not enforce the bound is held to it anyway.
    const auto over = oneRequest(answer(200, std::string(11, 'a')), request);
    ASSERT_FALSE(over.ok());
    EXPECT_EQ(over.error().code, core::ErrorCode::Unsupported);
    EXPECT_EQ(fetchFailureReason(over.error()), "too_large");
}

TEST(Transport, ARequestThatIsNotBoundedOrNotAnAddressIsRefusedBeforeAnyHop)
{
    Wire wire;
    wire.script.push_back(answer(200, "x"));
    const Fetch fetch = makeFetch(wire.hop());

    FetchRequest unbounded;
    unbounded.url = "https://github.com/x";
    unbounded.maxBytes = 0;
    EXPECT_EQ(fetch(unbounded, {}, {}).error().code, core::ErrorCode::InvalidArgument);

    FetchRequest noPayloadSize = rangedRequest();
    noPayloadSize.payloadSize = 0;
    EXPECT_EQ(fetch(noPayloadSize, {}, {}).error().code, core::ErrorCode::InvalidArgument);

    for (const std::string url : {"", "ftp://github.com/x", "github.com/x", "https://github.com/a b"}) {
        FetchRequest bad;
        bad.url = url;
        bad.maxBytes = 10;
        EXPECT_EQ(fetch(bad, {}, {}).error().code, core::ErrorCode::InvalidArgument) << url;
    }
    EXPECT_TRUE(wire.hops.empty());

    // No transport at all is a defect that says so, not a crash.
    EXPECT_EQ(makeFetch(HopFn{})(unbounded, {}, {}).error().code, core::ErrorCode::Internal);
    FetchRequest ok;
    ok.url = "https://github.com/x";
    ok.maxBytes = 10;
    EXPECT_EQ(makeFetch(HopFn{})(ok, {}, {}).error().code, core::ErrorCode::Internal);
}

TEST(Transport, ProgressIsPassedToTheHop)
{
    std::vector<std::pair<std::uint64_t, std::uint64_t>> seen;
    HopFn hop = [](const HttpHop&, std::stop_token, ProgressFn progress) -> core::Result<HttpHopReply> {
        progress(5, 10);
        progress(10, 10);
        HttpHopReply reply;
        reply.status = 200;
        reply.body = "0123456789";
        return reply;
    };
    FetchRequest request;
    request.url = "https://github.com/x";
    request.maxBytes = 10;
    ASSERT_TRUE(makeFetch(hop)(request, {}, [&](std::uint64_t done, std::uint64_t total) {
                   seen.emplace_back(done, total);
               }).ok());
    ASSERT_EQ(seen.size(), 2u);
    EXPECT_EQ(seen[1], (std::pair<std::uint64_t, std::uint64_t>{10, 10}));
}

// ---- nothing secret leaves the transport in an Error ---------------------------------------------------

TEST(Transport, NoErrorCarriesTheTokenAHeaderOrTheSignatureOfAStorageAddress)
{
    // Every way a request through the signed storage address can fail, with the token on the
    // request: the Error's text must name neither the token nor the query of the address.
    const std::vector<Reply> storageFailures = [] {
        std::vector<Reply> replies;
        replies.push_back(answer(403, "denied"));
        replies.push_back(answer(404, "gone"));
        replies.push_back(answer(429, "slow"));
        replies.push_back(answer(500, "oops"));
        replies.push_back(answer(200, "<html>", "text/html"));
        replies.push_back(answer(302, "", "", "http://downgraded.example.org/x?sig=SIGNATURE-IN-QUERY"));
        replies.push_back(answer(302, "", "", ""));
        replies.push_back(fail(core::makeError(core::ErrorCode::FileImportFailure, "no route", "status=0; x")));
        return replies;
    }();
    for (const Reply& storage : storageFailures) {
        Wire wire;
        wire.script.push_back(answer(302, "", "", std::string(kStorage)));
        wire.script.push_back(storage);
        const auto body = makeFetch(wire.hop())(apiAsset(), {}, {});
        ASSERT_FALSE(body.ok());
        const std::string text = body.error().describe();
        EXPECT_FALSE(contains(text, kSecretToken)) << text;
        EXPECT_FALSE(contains(text, "SIGNATURE-IN-QUERY")) << text;
        EXPECT_FALSE(contains(text, "sig=")) << text;
        EXPECT_FALSE(contains(text, "Bearer")) << text;
    }
    // And the failure of the first hop, to the API host with the token.
    for (const int status : {401, 403, 404, 429, 500}) {
        Wire wire;
        wire.script.push_back(answer(status, "body"));
        const auto body = makeFetch(wire.hop())(apiAsset(), {}, {});
        ASSERT_FALSE(body.ok());
        EXPECT_FALSE(contains(body.error().describe(), kSecretToken)) << status;
    }
}

// ---- the reason tokens -------------------------------------------------------------------------------------

TEST(FailureReason, EachFailureOfAFetchHasAShortMachineReadableToken)
{
    using core::ErrorCode;
    const auto reasonOf = [](ErrorCode code, std::string message, std::string context) {
        return fetchFailureReason(core::makeError(code, std::move(message), std::move(context)));
    };
    EXPECT_EQ(reasonOf(ErrorCode::InvalidState, "cancelled", ""), "cancelled");
    EXPECT_EQ(reasonOf(ErrorCode::NotFound, "nothing", "status=404; https://x/y"), "http_404");
    EXPECT_EQ(reasonOf(ErrorCode::FileImportFailure, "m", "status=403; https://x/y"), "http_403");
    EXPECT_EQ(reasonOf(ErrorCode::FileImportFailure, "m", "status=429; https://x/y"), "http_429");
    EXPECT_EQ(reasonOf(ErrorCode::FileImportFailure, "m", "status=503"), "http_503");
    EXPECT_EQ(reasonOf(ErrorCode::FileImportFailure, "m", "status=0; https://x/y; timeout"), "offline");
    EXPECT_EQ(reasonOf(ErrorCode::FileImportFailure, "m", "no status here"), "offline");
    EXPECT_EQ(reasonOf(ErrorCode::FileImportFailure, "m", "status=200; https://x/y; portal=html"), "html_page");
    EXPECT_EQ(reasonOf(ErrorCode::Unsupported, std::string(kPartialDownloadsRefused), "status=200"),
              "partial_downloads");
    EXPECT_EQ(reasonOf(ErrorCode::Unsupported, "longer than allowed", "status=200"), "too_large");
    // A reason the code that made the Error gave it wins, up to the next ';'.
    EXPECT_EQ(reasonOf(ErrorCode::InvalidArgument, "m", "reason=staged_hash; path=bin/x"), "staged_hash");
    EXPECT_EQ(reasonOf(ErrorCode::FileExportFailure, "m", "reason=low_disk_space"), "low_disk_space");
    EXPECT_EQ(reasonOf(ErrorCode::Internal, "m", ""), "error");
    EXPECT_EQ(reasonOf(ErrorCode::ParseFailure, "m", "reason="), "error") << "an empty token is none";
}

} // namespace
