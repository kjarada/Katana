// The feed (include/katana/update/feed.hpp): the configure-time defaults, the reader of a
// release's asset list, and the transport's rules - the one place the access token is held to
// "exactly https://api.github.com, on no redirect hop", the redirect limits, the portal and
// range answers, the time a request is given, and the guarantee that no error carries the token
// or the query of an address, whoever wrote its words.
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

    const auto api = hopHeaders(request, "https://api.github.com/repos/o/r/releases/assets/9",
                                /*redirected=*/false);
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
        // Whether the engine asked for the address itself or a server answered with it.
        for (const bool redirected : {false, true}) {
            const auto other = hopHeaders(request, url, redirected);
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
}

TEST(HopHeaders, AnAddressAServerAnsweredWithGetsNoTokenEvenWhenItIsTheApiHost)
{
    // The owner's ruling (S2) and the contract of FetchRequest::bearerToken: the token goes to
    // exactly https://api.github.com and NEVER on a redirect hop. So the very address that
    // carries it on the first hop carries none when a redirect is what named it.
    const FetchRequest request = rangedRequest();
    for (const std::string url : {
             "https://api.github.com/repos/o/r/releases/assets/9",
             "https://api.github.com/repositories/42/releases/assets/9",
             "https://api.github.com/user",
             "https://api.github.com",
         }) {
        const auto redirected = hopHeaders(request, url, /*redirected=*/true);
        EXPECT_FALSE(hasHeaderNamed(redirected, "Authorization")) << url;
        for (const std::string& header : redirected) {
            EXPECT_FALSE(contains(header, kSecretToken)) << url << ": " << header;
        }
        // The rest of what a hop to the API is sent says nothing secret, and stays.
        EXPECT_TRUE(hasHeader(redirected, "X-GitHub-Api-Version: 2022-11-28")) << url;
        EXPECT_TRUE(hasHeader(redirected, "Accept: application/octet-stream")) << url;
        EXPECT_TRUE(hasHeader(redirected, "Range: bytes=100-149")) << url;
    }
}

TEST(HopHeaders, NoTokenMeansNoAuthorizationEvenOnTheApiHost)
{
    FetchRequest request = rangedRequest();
    request.bearerToken.clear();
    const auto api = hopHeaders(request, "https://api.github.com/x", /*redirected=*/false);
    EXPECT_FALSE(hasHeaderNamed(api, "Authorization"));
}

TEST(HopHeaders, AWholeBodyRequestHasNoRangeAndAnEmptyAcceptIsOmitted)
{
    FetchRequest request;
    request.url = "https://github.com/x";
    request.maxBytes = 10;
    const auto headers = hopHeaders(request, request.url, /*redirected=*/false);
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

TEST(WithoutQueries, EveryAddressInATextLosesItsQueryAndItsFragment)
{
    // Written out by hand from the rule in feed.hpp: from "://" an address runs to the next
    // blank, control character or byte outside ASCII, and is cut at its first '?' or '#'.
    EXPECT_EQ(withoutQueries("https://h/p?sp=r&sig=S"), "https://h/p");
    EXPECT_EQ(withoutQueries("no answer from https://h/p?sig=S&se=1: timed out"),
              "no answer from https://h/p timed out");
    EXPECT_EQ(withoutQueries("https://h/p#frag end"), "https://h/p end");
    EXPECT_EQ(withoutQueries("a https://x/y b http://z/w?k=v c"), "a https://x/y b http://z/w c");
    EXPECT_EQ(withoutQueries("https://h/p?a=1\nhttps://g/q?b=2"), "https://h/p\nhttps://g/q");
    // What a list of secret parameter names does not know is cut all the same, and so is a
    // query some of whose values were already hidden.
    EXPECT_EQ(withoutQueries("https://s3.example/b/k?X-Amz-Signature=abcdef&X-Amz-Expires=300"),
              "https://s3.example/b/k");
    EXPECT_EQ(withoutQueries("https://h/p?sp=r&sig=***&se=2026 (HTTP 0): Could not resolve host"),
              "https://h/p (HTTP 0): Could not resolve host");
    // An address inside a query goes with the query; a quote or a semicolon inside a query does
    // not end the address (either may stand in one), so nothing after it survives.
    EXPECT_EQ(withoutQueries("https://a/p?next=https://b/q?sig=S tail"), "https://a/p tail");
    EXPECT_EQ(withoutQueries("'https://h/p?name='x';sig=S' refused"), "'https://h/p refused");
}

TEST(WithoutQueries, ATextWithoutAnAddressOrWithoutAQueryIsLeftAsItIs)
{
    for (const std::string text : {
             "",
             "the server did not answer within 60 s",
             "a web page instead of the file (a sign-in page?)", // a '?' that is in no address
             "status=403; https://github.com/o/r/releases/latest/download/x; detail",
             "status=206; reason=bad_range; https://github.com/x.zip; Content-Range 'bytes 1-2/3'",
             "://",
             "a://",
         }) {
        EXPECT_EQ(withoutQueries(text), text);
    }
}

// ---- the time a request is given ---------------------------------------------------------------------

TEST(Timeouts, ARangeIsGivenTheTimeItsLengthTakesOnASlowLine)
{
    // feed.hpp: 60 s to get an answer started, plus the length at 16 KiB a second rounded up;
    // never less than 600 s and never more than four hours. Each figure is worked by hand.
    EXPECT_EQ(kHopTimeoutSeconds, 60);
    EXPECT_EQ(kRangeHopTimeoutSeconds, 600);
    EXPECT_EQ(kSlowLineBytesPerSecond, 16384u);
    EXPECT_EQ(kMaxHopTimeoutSeconds, 14400);

    EXPECT_EQ(rangeHopTimeoutSeconds(0), 600);
    EXPECT_EQ(rangeHopTimeoutSeconds(50), 600);
    // The floor ends where the transfer alone takes 540 s: 540 * 16384 = 8847360 bytes.
    EXPECT_EQ(rangeHopTimeoutSeconds(8847360), 600);
    EXPECT_EQ(rangeHopTimeoutSeconds(8847361), 601) << "a second that was begun counts";
    // The largest merged range, 32 MiB = 33554432 bytes: 2048 s at 16384 a second, and 60.
    EXPECT_EQ(kRangeMaxLength, 33554432u);
    EXPECT_EQ(rangeHopTimeoutSeconds(kRangeMaxLength), 2108);
    EXPECT_EQ(rangeHopTimeoutSeconds(kRangeMaxLength + 1), 2109);
    // The cap is reached at 14340 s of transfer: 14340 * 16384 = 234946560 bytes; one second
    // less of it (14339 * 16384 = 234930176 bytes) is 14399 s.
    EXPECT_EQ(rangeHopTimeoutSeconds(234930176), 14399);
    EXPECT_EQ(rangeHopTimeoutSeconds(234946560), 14400);
    EXPECT_EQ(rangeHopTimeoutSeconds(2ull * 1024 * 1024 * 1024), 14400)
        << "the largest file a manifest may list";
    EXPECT_EQ(rangeHopTimeoutSeconds(~0ull), 14400) << "and no length makes the sum wrap";
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

TEST(Transport, EachHopIsGivenTheTimeOfItsRequest)
{
    // A whole body: 60 s. The largest merged range (32 MiB): 60 + 2048 = 2108 s, on every hop.
    {
        Wire wire;
        wire.script.push_back(answer(302, "", "", std::string(kStorage)));
        wire.script.push_back(answer(200, "ok"));
        ASSERT_TRUE(makeFetch(wire.hop())(apiAsset(), {}, {}).ok());
        ASSERT_EQ(wire.hops.size(), 2u);
        EXPECT_EQ(wire.hops[0].timeoutSeconds, 60);
        EXPECT_EQ(wire.hops[1].timeoutSeconds, 60);
    }
    {
        FetchRequest request = rangedRequest();
        request.rangeStart = 0;
        request.rangeLength = kRangeMaxLength;
        request.maxBytes = kRangeMaxLength;
        request.payloadSize = 2 * kRangeMaxLength;
        Wire wire;
        wire.script.push_back(answer(302, "", "", std::string(kStorage)));
        wire.script.push_back(answer(503, "busy"));
        ASSERT_FALSE(makeFetch(wire.hop())(request, {}, {}).ok());
        ASSERT_EQ(wire.hops.size(), 2u);
        EXPECT_EQ(wire.hops[0].timeoutSeconds, 2108);
        EXPECT_EQ(wire.hops[1].timeoutSeconds, 2108);
    }
}

TEST(Transport, ARedirectNeverCarriesTheTokenNotEvenBackToTheApiHost)
{
    // "Never to a redirect target" (the owner's ruling, S2; FetchRequest::bearerToken). The first
    // hop is the one address the engine wrote; every later one was named by a server.
    {
        // api.github.com answers a renamed repository with a 301 to itself: the hop that follows
        // it is a redirect hop, and has no token.
        Wire wire;
        wire.script.push_back(answer(301, "", "", "/repositories/42/releases/assets/9"));
        wire.script.push_back(answer(302, "", "", std::string(kStorage)));
        wire.script.push_back(answer(200, "ok"));
        const Fetch fetch = makeFetch(wire.hop());
        ASSERT_TRUE(fetch(apiAsset(), {}, {}).ok());
        ASSERT_EQ(wire.hops.size(), 3u);
        EXPECT_EQ(wire.hops[1].url, "https://api.github.com/repositories/42/releases/assets/9");
        EXPECT_TRUE(hasHeaderNamed(wire.hops[0].headers, "Authorization"));
        EXPECT_FALSE(hasHeaderNamed(wire.hops[1].headers, "Authorization"));
        EXPECT_FALSE(hasHeaderNamed(wire.hops[2].headers, "Authorization"));
    }
    {
        // The storage host (or whoever answers in its place) sends the request back to the API,
        // at a path of its own choosing: that request is not an authenticated one.
        Wire wire;
        wire.script.push_back(answer(302, "", "", std::string(kStorage)));
        wire.script.push_back(answer(302, "", "", "https://api.github.com/user"));
        wire.script.push_back(answer(200, "ok"));
        const Fetch fetch = makeFetch(wire.hop());
        ASSERT_TRUE(fetch(apiAsset(), {}, {}).ok());
        ASSERT_EQ(wire.hops.size(), 3u);
        EXPECT_EQ(wire.hops[2].url, "https://api.github.com/user");
        EXPECT_TRUE(hasHeaderNamed(wire.hops[0].headers, "Authorization"));
        EXPECT_FALSE(hasHeaderNamed(wire.hops[1].headers, "Authorization"));
        EXPECT_FALSE(hasHeaderNamed(wire.hops[2].headers, "Authorization"));
        for (std::size_t i = 1; i < wire.hops.size(); ++i) {
            for (const std::string& header : wire.hops[i].headers) {
                EXPECT_FALSE(contains(header, kSecretToken)) << wire.hops[i].url;
            }
        }
    }
    {
        // A redirect to the very address that was asked for is a redirect all the same.
        Wire wire;
        wire.script.push_back(
            answer(307, "", "", "https://api.github.com/repos/o/r/releases/assets/9"));
        wire.script.push_back(answer(200, "ok"));
        const Fetch fetch = makeFetch(wire.hop());
        ASSERT_TRUE(fetch(apiAsset(), {}, {}).ok());
        ASSERT_EQ(wire.hops.size(), 2u);
        EXPECT_EQ(wire.hops[1].url, wire.hops[0].url);
        EXPECT_TRUE(hasHeaderNamed(wire.hops[0].headers, "Authorization"));
        EXPECT_FALSE(hasHeaderNamed(wire.hops[1].headers, "Authorization"));
    }
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

    // Another part of the file, another file, no header, a short body: each is a 206 that is not
    // the range asked for, and says so with its own token - `http_206` would read as a status
    // the server refused with.
    for (const auto& wrong : {
             answer(206, part, "", "", "bytes 101-150/1000"),
             answer(206, part, "", "", "bytes 100-149/999"),
             answer(206, part, "", "", ""),
             answer(206, part, "", "", "bytes 0-49/1000"),
             answer(206, part.substr(0, 49), "", "", "bytes 100-149/1000"),
         }) {
        const auto refused = oneRequest(wrong, request);
        ASSERT_FALSE(refused.ok()) << wrong.reply.contentRange;
        EXPECT_EQ(refused.error().code, core::ErrorCode::FileImportFailure);
        EXPECT_EQ(fetchFailureReason(refused.error()), "bad_range") << wrong.reply.contentRange;
        EXPECT_TRUE(refused.error().context.starts_with("status=206; reason=bad_range; "))
            << refused.error().context;
    }
}

TEST(Transport, AServersHeaderIsShownBoundedAndPrintableAndCannotNameTheReason)
{
    // The Content-Range of a refused answer is quoted for a person, so it is the server's text
    // in this program's Error: cut to a length, with nothing but printable ASCII, and after the
    // token, where it cannot pass for one.
    FetchRequest request = rangedRequest();
    request.url = "https://github.com/o/r/releases/download/v1/x.zip";
    const std::string hostile = "reason=cancelled; \x1b[31m" + std::string(5000, 'A');
    const auto refused = oneRequest(answer(206, std::string(50, 'x'), "", "", hostile), request);
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(fetchFailureReason(refused.error()), "bad_range");
    EXPECT_LT(refused.error().context.size(), 400u);
    for (const char c : refused.error().context) {
        EXPECT_TRUE(c >= 0x20 && c < 0x7F) << "byte " << static_cast<int>(c);
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

// What a hop that abandons a transfer at maxBytes hands back when it can say what the answer was
// (katana_app's can): the status and the headers, no body, and `overran`.
Reply overrun(int status, std::string contentType = {}, std::string contentRange = {})
{
    Reply reply = answer(status, "", std::move(contentType), "", std::move(contentRange));
    reply.reply.overran = true;
    return reply;
}

TEST(Transport, AnAnswerThatRanPastItsBoundIsNamedByWhatTheAnswerSaidItWas)
{
    FetchRequest ranged = rangedRequest(); // bytes 100-149 of 1000, at most 50
    ranged.url = "https://github.com/o/r/releases/download/v1/x.zip";

    // A portal's page is a page however long it is. The signature is asked for with a bound of
    // 1024 bytes, and a sign-in page is longer than that: it is still "html_page", and not a
    // file that is too large or a network without partial downloads.
    for (const std::string type : {"text/html", "text/html; charset=utf-8"}) {
        const auto page = oneRequest(overrun(200, type));
        ASSERT_FALSE(page.ok()) << type;
        EXPECT_EQ(page.error().code, core::ErrorCode::FileImportFailure) << type;
        EXPECT_EQ(fetchFailureReason(page.error()), "html_page") << type;
        const auto inPlaceOfARange = oneRequest(overrun(200, type), ranged);
        ASSERT_FALSE(inPlaceOfARange.ok()) << type;
        EXPECT_EQ(fetchFailureReason(inPlaceOfARange.error()), "html_page") << type;
    }

    // A whole file longer than the engine reads before it has verified it.
    const auto large = oneRequest(overrun(200, "application/octet-stream"));
    ASSERT_FALSE(large.ok());
    EXPECT_EQ(large.error().code, core::ErrorCode::Unsupported);
    EXPECT_EQ(fetchFailureReason(large.error()), "too_large");
    EXPECT_TRUE(large.error().context.starts_with("status=200; ")) << large.error().context;

    // A range answered with a 200 that began the whole file: this network does not do ranges.
    const auto ignored = oneRequest(overrun(200, "application/zip"), ranged);
    ASSERT_FALSE(ignored.ok());
    EXPECT_EQ(ignored.error().code, core::ErrorCode::Unsupported);
    EXPECT_EQ(ignored.error().message, std::string(kPartialDownloadsRefused));
    EXPECT_EQ(fetchFailureReason(ignored.error()), "partial_downloads");
    EXPECT_TRUE(ignored.error().context.starts_with("status=200; ")) << ignored.error().context;

    // A range answered with a 206 that ran past the range: a part of the file, and not the
    // part asked for. Partial downloads ARE allowed here; this one is wrong (feed.hpp: a 206
    // whose body is of another length is `status=206; reason=bad_range`).
    const auto past = oneRequest(overrun(206, "application/zip", "bytes 100-149/1000"), ranged);
    ASSERT_FALSE(past.ok());
    EXPECT_EQ(past.error().code, core::ErrorCode::FileImportFailure);
    EXPECT_EQ(fetchFailureReason(past.error()), "bad_range");
    EXPECT_TRUE(past.error().context.starts_with("status=206; reason=bad_range; "))
        << past.error().context;
    // The same from a hop that does not enforce the bound and hands the long body over: 51
    // bytes where 50 were asked for.
    const auto handedOver =
        oneRequest(answer(206, std::string(51, 'x'), "", "", "bytes 100-149/1000"), ranged);
    ASSERT_FALSE(handedOver.ok());
    EXPECT_EQ(handedOver.error().code, core::ErrorCode::FileImportFailure);
    EXPECT_EQ(fetchFailureReason(handedOver.error()), "bad_range");
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

TEST(Transport, AHopThatPutsTheTokenInItsOwnErrorDoesNotGetItPastTheFetch)
{
    // The hop function is the caller's (katana_app's, over libcurl). Suppose it, or the library
    // under it, quotes a request header in a failure: the Fetch still returns no token. Every
    // code a hop may fail with (feed.hpp), with the token in the message and in the context.
    const std::string bearer = "Authorization: Bearer " + std::string(kSecretToken);
    for (const core::ErrorCode code :
         {core::ErrorCode::FileImportFailure, core::ErrorCode::InvalidState,
          core::ErrorCode::Unsupported, core::ErrorCode::InvalidArgument,
          core::ErrorCode::Internal}) {
        Wire wire;
        wire.script.push_back(fail(core::makeError(code, "could not send '" + bearer + "'",
                                                   "status=0; header " + bearer + "; " +
                                                       std::string(kSecretToken))));
        const auto body = makeFetch(wire.hop())(apiAsset(), {}, {});
        ASSERT_FALSE(body.ok());
        EXPECT_EQ(body.error().code, code) << "the failure is the hop's, less the token";
        EXPECT_FALSE(contains(body.error().describe(), kSecretToken)) << body.error().describe();
        // What was beside the token is still there to read.
        EXPECT_TRUE(contains(body.error().message, "could not send")) << body.error().message;
        EXPECT_TRUE(body.error().context.starts_with("status=0; ")) << body.error().context;
    }
    // The same on a later hop, where the header was never sent: the request still holds it.
    Wire wire;
    wire.script.push_back(answer(302, "", "", std::string(kStorage)));
    wire.script.push_back(fail(core::makeError(core::ErrorCode::FileImportFailure,
                                               "echo " + std::string(kSecretToken), "status=0")));
    const auto later = makeFetch(wire.hop())(apiAsset(), {}, {});
    ASSERT_FALSE(later.ok());
    EXPECT_FALSE(contains(later.error().describe(), kSecretToken));
}

TEST(Transport, AHopThatQuotesTheSignedAddressDoesNotGetItsQueryPastTheFetch)
{
    // The hop function is the caller's, over a library that puts the address it was given into
    // what it says about a failure - whole, or with the parameters it takes for secrets hidden
    // and the others left (a redaction by parameter name knows `sig`, and has never heard of
    // `sp` or `se`). The address a release redirects to is a signed one: all of its query is
    // the secret. So whatever a hop says, the Error of the Fetch names the address once, where
    // the Fetch writes it, without its query - and nothing else in it has a query either.
    const std::string partlyHidden =
        "https://release-assets.githubusercontent.com/github-production-release-asset/1/uuid"
        "?sp=r&sig=***&se=2026-10-20T08%3A00%3A00Z";
    struct Said {
        std::string message;
        std::string context;
    };
    const std::vector<Said> said = {
        {"the request failed",
         std::string(kStorage) + ": Could not resolve host: release-assets.githubusercontent.com"},
        {"the request failed", partlyHidden + ": SSL certificate problem"},
        {"no answer from " + std::string(kStorage), "status=0; " + std::string(kStorage)},
        {"no answer from '" + partlyHidden + "'", "status=0; reason=cancelled; " + partlyHidden},
    };
    for (const core::ErrorCode code :
         {core::ErrorCode::FileImportFailure, core::ErrorCode::InvalidState,
          core::ErrorCode::Unsupported, core::ErrorCode::InvalidArgument,
          core::ErrorCode::Internal}) {
        for (const Said& one : said) {
            Wire wire;
            wire.script.push_back(answer(302, "", "", std::string(kStorage)));
            wire.script.push_back(fail(core::makeError(code, one.message, one.context)));
            const auto body = makeFetch(wire.hop())(apiAsset(), {}, {});
            ASSERT_FALSE(body.ok());
            EXPECT_EQ(body.error().code, code) << "the failure is the hop's";
            const std::string text = body.error().describe();
            EXPECT_FALSE(contains(text, "SIGNATURE-IN-QUERY")) << text;
            EXPECT_FALSE(contains(text, "sig=")) << text;
            EXPECT_FALSE(contains(text, "sp=r")) << text;
            EXPECT_FALSE(contains(text, "se=2026")) << text;
            EXPECT_FALSE(contains(text, "?")) << text;
            EXPECT_FALSE(contains(text, kSecretToken)) << text;
            // No answer arrived, and then the address as a person may read it.
            EXPECT_TRUE(body.error().context.starts_with(
                "status=0; https://release-assets.githubusercontent.com/"
                "github-production-release-asset/1/uuid"))
                << body.error().context;
        }
    }
}

TEST(Transport, AHopsErrorKeepsItsStatusAndItsWordsAndCannotNameAReason)
{
    // feed.hpp, HopFn: the code and the sentence are the hop's; the context is the Fetch's -
    // the status the hop led with (0 when it named none), the address, then what else the hop
    // said, as text. The address here is oneRequest's.
    const std::string address = "https://github.com/o/r/releases/latest/download/x";

    const auto refused = oneRequest(fail(core::makeError(
        core::ErrorCode::FileImportFailure, "the service's certificate could not be verified",
        "status=0; SSL certificate problem: unable to get local issuer certificate")));
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, core::ErrorCode::FileImportFailure);
    EXPECT_EQ(refused.error().message, "the service's certificate could not be verified");
    EXPECT_EQ(refused.error().context,
              "status=0; " + address +
                  "; SSL certificate problem: unable to get local issuer certificate");
    EXPECT_EQ(fetchFailureReason(refused.error()), "offline");

    const auto bare = oneRequest(fail(core::makeError(core::ErrorCode::FileImportFailure, "no route")));
    ASSERT_FALSE(bare.ok());
    EXPECT_EQ(bare.error().context, "status=0; " + address);

    // A reason is this program's to give. One a hop wrote - first in its context, or after its
    // status - comes after the address, where it is words.
    const auto named = oneRequest(fail(core::makeError(core::ErrorCode::FileImportFailure, "m",
                                                       "reason=html_page; x")));
    ASSERT_FALSE(named.ok());
    EXPECT_EQ(named.error().context, "status=0; " + address + "; reason=html_page; x");
    EXPECT_EQ(fetchFailureReason(named.error()), "offline");
    const auto afterStatus = oneRequest(fail(core::makeError(
        core::ErrorCode::FileImportFailure, "m", "status=0; reason=cancelled; x")));
    ASSERT_FALSE(afterStatus.ok());
    EXPECT_EQ(afterStatus.error().context, "status=0; " + address + "; reason=cancelled; x");
    EXPECT_EQ(fetchFailureReason(afterStatus.error()), "offline");
}

TEST(Transport, AnAddressCannotNameTheReasonOfAFailure)
{
    // A path may spell anything, `reason=` and `status=` included; it comes after the fields
    // this program writes, and is not read as one of them. Here a server redirects to such an
    // address, which then refuses with 403.
    Wire wire;
    wire.script.push_back(
        answer(302, "", "", "https://evil.example/status=200;reason=cancelled;/portal=html"));
    wire.script.push_back(answer(403, "no"));
    FetchRequest request;
    request.url = "https://github.com/o/r/releases/latest/download/x";
    request.maxBytes = 100;
    const auto body = makeFetch(wire.hop())(request, {}, {});
    ASSERT_FALSE(body.ok());
    EXPECT_EQ(fetchFailureReason(body.error()), "http_403") << body.error().context;
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
    EXPECT_EQ(reasonOf(ErrorCode::FileImportFailure, "m", "status=200; reason=html_page; https://x/y"),
              "html_page");
    EXPECT_EQ(reasonOf(ErrorCode::Unsupported, std::string(kPartialDownloadsRefused), "status=200"),
              "partial_downloads");
    EXPECT_EQ(reasonOf(ErrorCode::Unsupported, "longer than allowed", "status=200"), "too_large");
    // A reason the code that made the Error gave it wins, up to the next ';': first in the
    // context, or straight after the status.
    EXPECT_EQ(reasonOf(ErrorCode::InvalidArgument, "m", "reason=staged_hash; path=bin/x"), "staged_hash");
    EXPECT_EQ(reasonOf(ErrorCode::FileExportFailure, "m", "reason=low_disk_space"), "low_disk_space");
    EXPECT_EQ(reasonOf(ErrorCode::FileImportFailure, "m", "status=302; reason=bad_redirect; https://x/y"),
              "bad_redirect");
    EXPECT_EQ(reasonOf(ErrorCode::Internal, "m", ""), "error");
    EXPECT_EQ(reasonOf(ErrorCode::ParseFailure, "m", "reason="), "error") << "an empty token is none";
    EXPECT_EQ(reasonOf(ErrorCode::FileImportFailure, "m", "status=403; reason=; https://x/y"), "http_403")
        << "an empty token is none after a status too";
}

TEST(FailureReason, AReasonIsReadOnlyWhereThisProgramWritesOne)
{
    // feed.hpp: `status=<n>; reason=<token>; <address>; <detail>`. An address and a server's
    // words come after the two fields, and whatever they spell is not a token.
    using core::ErrorCode;
    const auto reasonOf = [](ErrorCode code, std::string context) {
        return fetchFailureReason(core::makeError(code, "m", std::move(context)));
    };
    EXPECT_EQ(reasonOf(ErrorCode::FileImportFailure, "status=403; https://x/reason=cancelled;y"),
              "http_403");
    EXPECT_EQ(reasonOf(ErrorCode::FileImportFailure, "status=429; https://x/y; reason=html_page"),
              "http_429");
    EXPECT_EQ(reasonOf(ErrorCode::NotFound, "status=404; https://x/y?reason=offline"), "http_404");
    EXPECT_EQ(reasonOf(ErrorCode::FileImportFailure, "https://x/status=500; reason=cancelled"),
              "offline")
        << "a status is read only at the very start";
    EXPECT_EQ(reasonOf(ErrorCode::FileImportFailure, "status=200x; reason=cancelled"), "http_200")
        << "what follows the number must be the separator";
    EXPECT_EQ(reasonOf(ErrorCode::FileImportFailure, "status=; reason=cancelled"), "offline")
        << "a status with no number is not the leading field, so nothing follows it";
    EXPECT_EQ(reasonOf(ErrorCode::InvalidState, "path=bin/x; reason=staged_hash"), "cancelled")
        << "not first: not a token";
}

} // namespace
