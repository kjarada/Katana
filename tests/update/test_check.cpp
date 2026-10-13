// check() (include/katana/update/check.hpp): asking the feed whether a newer release exists.
//
// Where the expectations come from (CLAUDE.md, section 4). The releases are the T3 fixtures
// (tests/update/fixtures/release): manifests made by the Python producer and signed by
// `openssl pkeyutl` with the published RFC 8032 test keys, so "this signature is good" is
// settled outside this program. What each fixture is - v1 is 0.9.0 (sequence 1790000000), v2 is
// 0.10.0 (sequence 1792312800), and why each bad/ folder is bad - is written in
// tools/update_make_fixtures.py. A manifest that must differ in one signed field (another
// product, an old `expires`) is the v2 text with that field changed and signed here with
// OpenSSL, the same independent signer the swap's tests use. The feed is a fake Fetch keyed by
// address that records every request it is given, so what is asked for, in what order, with
// what bound and with what token is read off the record.

#include <gtest/gtest.h>

#include <algorithm>
#include <array>
#include <map>
#include <optional>
#include <stop_token>
#include <string>
#include <string_view>
#include <vector>

#include "apply_harness.hpp"
#include "katana/update/check.hpp"
#include "katana/update/manifest.hpp"
#include "katana/update/ranges.hpp"
#include "katana/update/version.hpp"
#include "test_key.hpp"

using namespace katana;
using namespace katana::update;
namespace h = katana::update::harness;

namespace {

constexpr std::string_view kSecretToken = "ghp_SECRET-TOKEN-0123456789";

// ---- the feed ---------------------------------------------------------------------------------------

FeedConfig githubFeed()
{
    FeedConfig feed;
    feed.repo = "kjarada/Katana";
    feed.target = "win64";
    feed.userAgent = "Katana/0.9.0 (win64)";
    return feed;
}

Install installOf(std::string_view version, std::uint64_t sequence, bool record = true)
{
    Install install;
    install.root = "R";
    install.target = "win64";
    install.kind = InstallKind::Archive;
    install.version = parseVersion(version).value();
    install.sequence = sequence;
    install.hasInstalledRecord = record;
    return install;
}

// 0.9.0 as the helper recorded it.
Install installedV1()
{
    return installOf("0.9.0", 1790000000);
}

struct Release {
    std::string json;
    std::string sig;
};

Release fixture(const std::string& name)
{
    const h::Path folder = h::fixtureFolder() / "release" / name;
    Release release;
    release.json = h::readText(folder / "Katana-update-win64.json");
    release.sig = h::readText(folder / "Katana-update-win64.json.sig");
    return release;
}

// v2's text with `from` replaced by `to` (exactly once), signed by the active test key.
Release changedV2(const std::string& from, const std::string& to)
{
    Release release = fixture("v2");
    const std::size_t at = release.json.find(from);
    EXPECT_NE(at, std::string::npos) << "the fixture no longer holds: " << from;
    if (at != std::string::npos) {
        release.json.replace(at, from.size(), to);
    }
    release.sig = h::signLine(release.json);
    return release;
}

// A fake feed: addresses to bodies or errors; anything else is a 404, as on the real host.
class FakeFeed {
  public:
    void serve(const std::string& url, std::string body)
    {
        bodies_.insert_or_assign(url, std::move(body));
    }
    void fail(const std::string& url, core::Error error)
    {
        errors_.insert_or_assign(url, std::move(error));
    }
    void serveRelease(const FeedConfig& feed, const Release& release)
    {
        serve(signatureUrl(feed), release.sig);
        serve(manifestUrl(feed), release.json);
    }

    Fetch fetch()
    {
        return [this](const FetchRequest& request, std::stop_token,
                      ProgressFn) -> core::Result<std::string> {
            requests.push_back(request);
            if (const auto error = errors_.find(request.url); error != errors_.end()) {
                return error->second;
            }
            if (const auto body = bodies_.find(request.url); body != bodies_.end()) {
                return body->second;
            }
            return core::makeError(core::ErrorCode::NotFound, "nothing at this address",
                                   "status=404; " + request.url);
        };
    }

    std::vector<FetchRequest> requests;

  private:
    std::map<std::string, std::string> bodies_;
    std::map<std::string, core::Error> errors_;
};

core::Error status(core::ErrorCode code, int number)
{
    return core::makeError(code, "the server said " + std::to_string(number),
                           "status=" + std::to_string(number) + "; https://github.com/x");
}

CheckResult run(FakeFeed& feed, const FeedConfig& config, const Install& install,
                const Trust& trust = testTrust())
{
    const core::Result<CheckResult> result =
        check(feed.fetch(), config, install, trust, std::stop_token{});
    EXPECT_TRUE(result.ok()) << (result.ok() ? "" : result.error().describe());
    return result.ok() ? result.value() : CheckResult{};
}

// ---- available, current -------------------------------------------------------------------------------

TEST(Check, ANewerReleaseIsAvailableWithItsVerifiedManifest)
{
    FakeFeed feed;
    const FeedConfig config = githubFeed();
    feed.serveRelease(config, fixture("v2"));
    const CheckResult result = run(feed, config, installedV1());

    EXPECT_EQ(result.status, CheckStatus::Available);
    EXPECT_EQ(result.installedVersion, "0.9.0");
    EXPECT_EQ(result.latestVersion, "0.10.0") << "0.10 is above 0.9 as a number, not as text";
    EXPECT_EQ(result.notesUrl, "https://github.com/kjarada/Katana/releases/tag/v0.10.0");
    EXPECT_TRUE(result.reason.empty());
    EXPECT_TRUE(result.detail.empty());
    EXPECT_EQ(result.route, FeedRoute::Download);
    ASSERT_TRUE(result.manifest.has_value());
    EXPECT_EQ(result.manifest->sequence, 1792312800u);
    EXPECT_EQ(result.manifest->tag, "v0.10.0");
    // The manifest carries the exact bytes that were signed, for the stage and the helper.
    EXPECT_EQ(result.manifest->signedJson, fixture("v2").json);
    EXPECT_EQ(result.manifest->signatureLine, fixture("v2").sig);
}

TEST(Check, TheSignatureIsFetchedFirstThenTheManifestEachWithTheEnginesBound)
{
    FakeFeed feed;
    const FeedConfig config = githubFeed();
    feed.serveRelease(config, fixture("v2"));
    (void)run(feed, config, installedV1());

    ASSERT_EQ(feed.requests.size(), 2u);
    // Written out: the addresses of the Download route (feed.hpp).
    EXPECT_EQ(feed.requests[0].url,
              "https://github.com/kjarada/Katana/releases/latest/download/Katana-update-win64.json.sig");
    EXPECT_EQ(feed.requests[1].url,
              "https://github.com/kjarada/Katana/releases/latest/download/Katana-update-win64.json");
    EXPECT_EQ(feed.requests[0].maxBytes, kMaxSignatureBytes);
    EXPECT_EQ(feed.requests[1].maxBytes, kMaxManifestBytes);
    for (const FetchRequest& request : feed.requests) {
        EXPECT_NE(request.maxBytes, 0u) << "a Fetch is never given an unbounded request";
        EXPECT_EQ(request.userAgent, "Katana/0.9.0 (win64)");
        EXPECT_TRUE(request.bearerToken.empty()) << "the Download route never carries a token";
        EXPECT_EQ(request.rangeLength, 0u) << "a whole body";
    }
}

TEST(Check, NothingIsFetchedAfterASignatureThatCouldNotBeRead)
{
    FakeFeed feed;
    const FeedConfig config = githubFeed();
    feed.fail(signatureUrl(config), status(core::ErrorCode::FileImportFailure, 500));
    feed.serve(manifestUrl(config), fixture("v2").json);
    const CheckResult result = run(feed, config, installedV1());
    EXPECT_EQ(result.status, CheckStatus::Unreachable);
    EXPECT_EQ(feed.requests.size(), 1u) << "the signature gates the manifest";
}

TEST(Check, TheSameOrAnOlderReleaseIsCurrentAndNeverOffered)
{
    const FeedConfig config = githubFeed();
    struct Case {
        const char* what;
        std::string release;
        Install install;
    };
    const std::vector<Case> cases = {
        {"the installed version and sequence", "v2", installOf("0.10.0", 1792312800)},
        {"a build with a higher sequence installed", "v2", installOf("0.10.0", 1792312801)},
        {"a higher version installed", "v2", installOf("0.11.0", 1700000000)},
        // T3's bad/: a validly signed 0.9.0 with a sequence ABOVE v2's, offered to 0.10.0.
        {"a downgrade by version", "bad/downgrade-version", installOf("0.10.0", 1792312800)},
        // ... and a validly signed 0.10.0 with a sequence BELOW v2's, offered to that install.
        {"a replay of an older build of the same version", "bad/downgrade-sequence",
         installOf("0.10.0", 1792312800)},
    };
    for (const Case& test : cases) {
        FakeFeed feed;
        feed.serveRelease(config, fixture(test.release));
        const CheckResult result = run(feed, config, test.install);
        EXPECT_EQ(result.status, CheckStatus::Current) << test.what;
        EXPECT_TRUE(result.manifest.has_value()) << test.what << ": the verified manifest is kept";
        EXPECT_FALSE(result.latestVersion.empty()) << test.what;
    }
}

TEST(Check, AFreshInstallIsNotOfferedTheReleaseItCameFrom)
{
    // No installed.json: a fresh install's sequence is the placeholder 0, whatever sequence its
    // package names (install.hpp), and isNewerThan leaves it out while there is no record
    // (trust.hpp), so the same version is not newer.
    FakeFeed feed;
    const FeedConfig config = githubFeed();
    feed.serveRelease(config, fixture("v2"));
    EXPECT_EQ(run(feed, config, installOf("0.10.0", 0, /*record=*/false)).status,
              CheckStatus::Current);
    // ... but a higher version is offered to it.
    EXPECT_EQ(run(feed, config, installOf("0.9.0", 0, /*record=*/false)).status,
              CheckStatus::Available);
}

TEST(Check, AReleaseSignedByTheSpareKeyIsAsGoodAsOneSignedByTheActiveKey)
{
    FakeFeed feed;
    const FeedConfig config = githubFeed();
    feed.serveRelease(config, fixture("v2-spare-signed"));
    EXPECT_EQ(run(feed, config, installedV1()).status, CheckStatus::Available);
}

TEST(Check, ANotifyOnlyReleaseIsAvailableAndSaysSo)
{
    FeedConfig config = githubFeed();
    config.target = "macos-arm64";
    Install install = installedV1();
    install.target = "macos-arm64";
    FakeFeed feed;
    const h::Path folder = h::fixtureFolder() / "release" / "v2-macos-notify";
    feed.serve(signatureUrl(config), h::readText(folder / "Katana-update-macos-arm64.json.sig"));
    feed.serve(manifestUrl(config), h::readText(folder / "Katana-update-macos-arm64.json"));
    const CheckResult result = run(feed, config, install);
    EXPECT_EQ(result.status, CheckStatus::Available);
    ASSERT_TRUE(result.manifest.has_value());
    EXPECT_TRUE(result.manifest->notifyOnly);
    EXPECT_FALSE(result.manifest->payload.has_value());
}

TEST(Check, AnExpiredManifestIsStillOfferedExpiryIsAdvisory)
{
    // `expires` 2021-01-01, long gone: the dialog says the information is old, and the release is
    // offered all the same (manifest.hpp). The clock is the test's: isExpired takes it.
    REQUIRE_CAN_SIGN();
    FakeFeed feed;
    const FeedConfig config = githubFeed();
    feed.serveRelease(config, changedV2("\"expires\":\"2027-04-16T08:40:00Z\"",
                                        "\"expires\":\"2021-01-01T00:00:00Z\""));
    const CheckResult result = run(feed, config, installedV1());
    EXPECT_EQ(result.status, CheckStatus::Available);
    ASSERT_TRUE(result.manifest.has_value());
    EXPECT_TRUE(isExpired(*result.manifest, 1'800'000'000)) << "January 2027 is after 2021";
    EXPECT_FALSE(isExpired(*result.manifest, 1'600'000'000)) << "September 2020 is before it";
}

// ---- every way of not learning the answer is Unreachable, with a reason ------------------------------------

struct Failure {
    const char* what;
    core::Error error;
    const char* reason;
};

TEST(Check, AFeedThatCannotBeReadIsUnreachableWithTheReasonAndNeverAnErrorOfTheCall)
{
    const FeedConfig config = githubFeed();
    const std::vector<Failure> failures = {
        {"a private repository or a missing release (404)",
         status(core::ErrorCode::NotFound, 404), "http_404"},
        {"the rate limit (403)", status(core::ErrorCode::FileImportFailure, 403), "http_403"},
        {"too many requests (429)", status(core::ErrorCode::FileImportFailure, 429), "http_429"},
        {"the host in trouble (503)", status(core::ErrorCode::FileImportFailure, 503), "http_503"},
        {"no network",
         core::makeError(core::ErrorCode::FileImportFailure, "the service's host name could not be found",
                         "status=0; https://github.com/x"),
         "offline"},
        // As the transport reports a page (feed.hpp: `status=<n>; reason=html_page`).
        {"a captive portal's page",
         core::makeError(core::ErrorCode::FileImportFailure, "a web page instead of the file",
                         "status=200; reason=html_page; https://github.com/x"),
         "html_page"},
        {"a redirect out of https",
         core::makeError(core::ErrorCode::FileImportFailure, "redirected to an address that is not https",
                         "status=302; reason=bad_redirect; https://github.com/x"),
         "bad_redirect"},
        {"a body past the bound",
         core::makeError(core::ErrorCode::Unsupported, "longer than allowed", "status=200"), "too_large"},
        {"a cancelled transfer", core::makeError(core::ErrorCode::InvalidState, "cancelled"), "cancelled"},
    };
    for (const Failure& failure : failures) {
        // The signature failing, and then (the signature good) the manifest failing.
        for (const bool onSignature : {true, false}) {
            FakeFeed feed;
            feed.serve(signatureUrl(config), fixture("v2").sig);
            feed.serve(manifestUrl(config), fixture("v2").json);
            feed.fail(onSignature ? signatureUrl(config) : manifestUrl(config), failure.error);
            const core::Result<CheckResult> result =
                check(feed.fetch(), config, installedV1(), testTrust(), std::stop_token{});
            ASSERT_TRUE(result.ok()) << failure.what << ": " << result.error().describe();
            EXPECT_EQ(result.value().status, CheckStatus::Unreachable) << failure.what;
            EXPECT_EQ(result.value().reason, failure.reason) << failure.what;
            EXPECT_EQ(result.value().detail, failure.error.message) << failure.what;
            EXPECT_FALSE(result.value().manifest.has_value()) << failure.what;
        }
    }
}

TEST(Check, AWebPageWhereTheSignatureOrTheManifestShouldBeIsAPortal)
{
    const FeedConfig config = githubFeed();
    const std::string page = "<!DOCTYPE html><html><body>Sign in to the guest network</body></html>";
    for (const std::string& body : {page, "  \r\n\t" + page, std::string("\xEF\xBB\xBF") + page,
                                    std::string("<html>")}) {
        {
            FakeFeed feed;
            feed.serveRelease(config, fixture("v2"));
            feed.serve(signatureUrl(config), body);
            const CheckResult result = run(feed, config, installedV1());
            EXPECT_EQ(result.status, CheckStatus::Unreachable);
            EXPECT_EQ(result.reason, "html_page");
        }
        {
            FakeFeed feed;
            feed.serveRelease(config, fixture("v2"));
            feed.serve(manifestUrl(config), body);
            const CheckResult result = run(feed, config, installedV1());
            EXPECT_EQ(result.status, CheckStatus::Unreachable);
            EXPECT_EQ(result.reason, "html_page");
        }
    }
}

TEST(Check, ATruncatedSignatureOrManifestDoesNotVerify)
{
    const FeedConfig config = githubFeed();
    const Release good = fixture("v2");
    struct Case {
        const char* what;
        std::string sig;
        std::string json;
        const char* reason;
    };
    const std::vector<Case> cases = {
        {"a signature cut short", good.sig.substr(0, 60), good.json, "malformed"},
        {"an empty signature", "", good.json, "malformed"},
        {"a manifest cut in half", good.sig, good.json.substr(0, good.json.size() / 2), "bad_signature"},
        {"a manifest one byte short", good.sig, good.json.substr(0, good.json.size() - 1), "bad_signature"},
        {"an empty manifest", good.sig, "", "malformed"},
    };
    for (const Case& test : cases) {
        FakeFeed feed;
        feed.serve(signatureUrl(config), test.sig);
        feed.serve(manifestUrl(config), test.json);
        const CheckResult result = run(feed, config, installedV1());
        EXPECT_EQ(result.status, CheckStatus::Unreachable) << test.what;
        EXPECT_EQ(result.reason, test.reason) << test.what;
        EXPECT_FALSE(result.manifest.has_value()) << test.what;
    }
}

TEST(Check, AnythingThatDoesNotVerifyIsNeverTrusted)
{
    const FeedConfig config = githubFeed();
    struct Case {
        const char* folder;
        const char* reason;
    };
    const std::vector<Case> cases = {
        {"bad/flipped-manifest-bit", "bad_signature"},   // one bit of a hash digit
        {"bad/swapped-sig", "bad_signature"},            // v1's signature on v2's manifest
        {"bad/wrong-key", "untrusted_key"},              // signed by a key no build pins
        {"bad/wrong-key-claims-active", "bad_signature"} // another key, the active key's id
    };
    for (const Case& test : cases) {
        FakeFeed feed;
        feed.serveRelease(config, fixture(test.folder));
        const CheckResult result = run(feed, config, installedV1());
        EXPECT_EQ(result.status, CheckStatus::Unreachable) << test.folder;
        EXPECT_EQ(result.reason, test.reason) << test.folder;
        EXPECT_FALSE(result.manifest.has_value()) << test.folder;
        EXPECT_FALSE(result.detail.empty()) << test.folder;
    }
}

TEST(Check, AValidlySignedManifestWithABreakingPathIsMalformedNotAnUpdate)
{
    // T3's bad/path-traversal: signed correctly, and lists bin/../../evil.dll.
    FakeFeed feed;
    const FeedConfig config = githubFeed();
    feed.serveRelease(config, fixture("bad/path-traversal"));
    const CheckResult result = run(feed, config, installedV1());
    EXPECT_EQ(result.status, CheckStatus::Unreachable);
    EXPECT_EQ(result.reason, "malformed");
}

TEST(Check, ABuildWithPlaceholderKeysCanNeitherVerifyNorBeFooled)
{
    FakeFeed feed;
    const FeedConfig config = githubFeed();
    feed.serveRelease(config, fixture("v2"));
    Trust placeholders;
    placeholders.placeholder = true;
    const CheckResult result = run(feed, config, installedV1(), placeholders);
    EXPECT_EQ(result.status, CheckStatus::Unreachable);
    EXPECT_EQ(result.reason, "placeholder_keys");
}

TEST(Check, ATrustBuiltWronglyIsCouldNotCheckAndNeverForged)
{
    // A pinned key that is no public key (here 32 bytes of 0xFF, past the field prime, so not a
    // canonical point) cannot say anything about any signature: Internal, which check() reports as
    // "verify_unavailable" and not as a bad signature.
    PublicKey broken;
    broken.fill(0xFF);
    Trust trust;
    trust.keys.push_back(TrustedKey{keyIdOf(broken), broken});
    const std::array<unsigned char, 64> zeros{};
    const std::string line = "ed25519 " + keyIdOf(broken) + " " + h::base64(zeros.data(), zeros.size()) + "\n";
    FakeFeed feed;
    const FeedConfig config = githubFeed();
    feed.serve(signatureUrl(config), line);
    feed.serve(manifestUrl(config), fixture("v2").json);
    const CheckResult result = run(feed, config, installedV1(), trust);
    EXPECT_EQ(result.status, CheckStatus::Unreachable);
    EXPECT_EQ(result.reason, "verify_unavailable");
}

TEST(Check, ASignedManifestForAnotherProductTargetOrChannelIsNotForThisInstall)
{
    REQUIRE_CAN_SIGN();
    const FeedConfig config = githubFeed();
    struct Case {
        const char* what;
        std::string from;
        std::string to;
        const char* reason;
    };
    const std::vector<Case> cases = {
        {"another product", "\"product\":\"katana\"", "\"product\":\"other\"", "wrong_product"},
        {"another target", "\"target\":\"win64\"", "\"target\":\"win-arm64\"", "wrong_target"},
        {"another channel", "\"channel\":\"stable\"", "\"channel\":\"beta\"", "wrong_channel"},
    };
    for (const Case& test : cases) {
        FakeFeed feed;
        feed.serveRelease(config, changedV2(test.from, test.to));
        const CheckResult result = run(feed, config, installedV1());
        EXPECT_EQ(result.status, CheckStatus::Unreachable) << test.what;
        EXPECT_EQ(result.reason, test.reason) << test.what;
    }
}

// ---- cannot be asked --------------------------------------------------------------------------------------------

TEST(Check, ADevelopmentTreeIsNeverAskedAbout)
{
    FakeFeed feed;
    Install install = installedV1();
    install.kind = InstallKind::Development;
    install.target.clear();
    const CheckResult result = run(feed, githubFeed(), install);
    EXPECT_EQ(result.status, CheckStatus::Unsupported);
    EXPECT_EQ(result.reason, "development_tree");
    EXPECT_FALSE(result.detail.empty());
    EXPECT_TRUE(feed.requests.empty()) << "no network is touched";
}

TEST(Check, ABuildWithNoFeedIsUnsupportedAndAsksNothing)
{
    FakeFeed feed;
    FeedConfig config = githubFeed();
    config.repo.clear();
    const CheckResult result = run(feed, config, installedV1());
    EXPECT_EQ(result.status, CheckStatus::Unsupported);
    EXPECT_EQ(result.reason, "disabled_build");
    EXPECT_TRUE(feed.requests.empty());

    // The owner's own host instead of GitHub is a feed.
    config.feedBase = "https://updates.example.org/katana";
    feed.serveRelease(config, fixture("v2"));
    EXPECT_EQ(run(feed, config, installedV1()).status, CheckStatus::Available);
    ASSERT_FALSE(feed.requests.empty());
    EXPECT_EQ(feed.requests[0].url, "https://updates.example.org/katana/Katana-update-win64.json.sig");
}

TEST(Check, ACallThatCannotBeMadeIsAnErrorOfTheCall)
{
    FakeFeed feed;
    const Fetch fetch = feed.fetch();
    const Install install = installedV1();

    std::stop_source stopped;
    stopped.request_stop();
    const auto cancelled = check(fetch, githubFeed(), install, testTrust(), stopped.get_token());
    ASSERT_FALSE(cancelled.ok());
    EXPECT_EQ(cancelled.error().code, core::ErrorCode::InvalidState);

    FeedConfig noTarget = githubFeed();
    noTarget.target.clear();
    EXPECT_EQ(check(fetch, noTarget, install, testTrust(), {}).error().code, core::ErrorCode::InvalidArgument);

    FeedConfig beta = githubFeed();
    beta.channel = "beta";
    EXPECT_EQ(check(fetch, beta, install, testTrust(), {}).error().code, core::ErrorCode::InvalidArgument);

    FeedConfig otherTarget = githubFeed();
    otherTarget.target = "linux-x86_64";
    EXPECT_EQ(check(fetch, otherTarget, install, testTrust(), {}).error().code,
              core::ErrorCode::InvalidArgument)
        << "the feed's target is the install's";

    EXPECT_EQ(check(Fetch{}, githubFeed(), install, testTrust(), {}).error().code,
              core::ErrorCode::InvalidArgument);
    EXPECT_TRUE(feed.requests.empty());
}

// ---- the private-repository route ----------------------------------------------------------------------

constexpr std::string_view kReleaseJson = R"({
  "tag_name": "v0.10.0",
  "assets": [
    {"id": 11, "name": "Katana-update-win64.json.sig", "size": 114},
    {"id": 12, "name": "Katana-update-win64.json", "size": 2000},
    {"id": 13, "name": "Katana-0.10.0-win64.update.zip", "size": 3791}
  ]
})";

FeedConfig privateFeed()
{
    FeedConfig config = githubFeed();
    config.token = std::string(kSecretToken);
    return config;
}

// A repository that answers an anonymous read with 404 and a token holder through the API.
void serveBehindToken(FakeFeed& feed, const FeedConfig& config, const Release& release)
{
    feed.serve(releaseLookupUrl(config), std::string(kReleaseJson));
    feed.serve(assetApiUrl(config, 11), release.sig);
    feed.serve(assetApiUrl(config, 12), release.json);
}

TEST(CheckPrivateRoute, A404OnTheSignatureWithATokenRetriesOnceThroughTheApiAndSaysSo)
{
    FakeFeed feed;
    const FeedConfig config = privateFeed();
    serveBehindToken(feed, config, fixture("v2"));
    const CheckResult result = run(feed, config, installedV1());

    EXPECT_EQ(result.status, CheckStatus::Available);
    EXPECT_EQ(result.route, FeedRoute::Api);
    ASSERT_EQ(feed.requests.size(), 4u);
    // The anonymous read first; then the lookup and the two assets by their ids.
    EXPECT_EQ(feed.requests[0].url,
              "https://github.com/kjarada/Katana/releases/latest/download/Katana-update-win64.json.sig");
    EXPECT_EQ(feed.requests[1].url, "https://api.github.com/repos/kjarada/Katana/releases/latest");
    EXPECT_EQ(feed.requests[2].url, "https://api.github.com/repos/kjarada/Katana/releases/assets/11");
    EXPECT_EQ(feed.requests[3].url, "https://api.github.com/repos/kjarada/Katana/releases/assets/12");
    EXPECT_EQ(feed.requests[1].accept, "application/vnd.github+json");
    EXPECT_EQ(feed.requests[2].accept, "application/octet-stream");
    EXPECT_EQ(feed.requests[3].accept, "application/octet-stream");
    EXPECT_EQ(feed.requests[1].maxBytes, kMaxReleaseJsonBytes);
    EXPECT_EQ(feed.requests[2].maxBytes, kMaxSignatureBytes);
    EXPECT_EQ(feed.requests[3].maxBytes, kMaxManifestBytes);
    // The token: on every Api request, on no Download one.
    EXPECT_TRUE(feed.requests[0].bearerToken.empty());
    for (std::size_t i = 1; i < feed.requests.size(); ++i) {
        EXPECT_EQ(feed.requests[i].bearerToken, std::string(kSecretToken)) << feed.requests[i].url;
    }
}

TEST(CheckPrivateRoute, ATokenIsNotUsedWhenThePublicRouteWorks)
{
    FakeFeed feed;
    const FeedConfig config = privateFeed();
    feed.serveRelease(config, fixture("v2"));
    const CheckResult result = run(feed, config, installedV1());
    EXPECT_EQ(result.status, CheckStatus::Available);
    EXPECT_EQ(result.route, FeedRoute::Download);
    EXPECT_EQ(feed.requests.size(), 2u);
    for (const FetchRequest& request : feed.requests) {
        EXPECT_TRUE(request.bearerToken.empty());
    }
}

TEST(CheckPrivateRoute, NoRetryWithoutATokenOrWithAFeedBaseOrAfterAnyOtherFailure)
{
    {
        FakeFeed feed;
        FeedConfig config = githubFeed(); // no token
        serveBehindToken(feed, config, fixture("v2"));
        const CheckResult result = run(feed, config, installedV1());
        EXPECT_EQ(result.status, CheckStatus::Unreachable);
        EXPECT_EQ(result.reason, "http_404");
        EXPECT_EQ(feed.requests.size(), 1u);
        EXPECT_EQ(result.route, FeedRoute::Download);
    }
    {
        FakeFeed feed;
        FeedConfig config = privateFeed();
        config.feedBase = "https://updates.example.org/katana"; // the owner's host: the API is GitHub's
        const CheckResult result = run(feed, config, installedV1());
        EXPECT_EQ(result.reason, "http_404");
        EXPECT_EQ(feed.requests.size(), 1u);
    }
    for (const int number : {403, 429, 500}) {
        FakeFeed feed;
        const FeedConfig config = privateFeed();
        serveBehindToken(feed, config, fixture("v2"));
        feed.fail(signatureUrl(config), status(core::ErrorCode::FileImportFailure, number));
        const CheckResult result = run(feed, config, installedV1());
        EXPECT_EQ(result.reason, "http_" + std::to_string(number));
        EXPECT_EQ(feed.requests.size(), 1u) << "only a 404 sends a token holder to the API";
    }
    {
        // The signature found, the manifest not (a release being published): not a private repo.
        FakeFeed feed;
        const FeedConfig config = privateFeed();
        serveBehindToken(feed, config, fixture("v2"));
        feed.serve(signatureUrl(config), fixture("v2").sig);
        const CheckResult result = run(feed, config, installedV1());
        EXPECT_EQ(result.reason, "http_404");
        EXPECT_EQ(feed.requests.size(), 2u);
    }
}

TEST(CheckPrivateRoute, ThePrivateRouteFailsWithTheReasonOfWhatWentWrongThere)
{
    const FeedConfig config = privateFeed();
    {
        FakeFeed feed; // the token sees nothing either: the lookup is a 404 too
        const CheckResult result = run(feed, config, installedV1());
        EXPECT_EQ(result.status, CheckStatus::Unreachable);
        EXPECT_EQ(result.reason, "http_404");
        EXPECT_EQ(result.route, FeedRoute::Api);
        EXPECT_EQ(feed.requests.size(), 2u);
    }
    {
        FakeFeed feed; // a bad token
        feed.fail(releaseLookupUrl(config), status(core::ErrorCode::FileImportFailure, 401));
        EXPECT_EQ(run(feed, config, installedV1()).reason, "http_401");
    }
    {
        FakeFeed feed; // the release has no manifest asset
        feed.serve(releaseLookupUrl(config), R"({"assets":[{"id":11,"name":"Katana-update-win64.json.sig"}]})");
        const CheckResult result = run(feed, config, installedV1());
        EXPECT_EQ(result.reason, "asset_missing");
        EXPECT_EQ(feed.requests.size(), 2u);
    }
    {
        FakeFeed feed; // the release has no signature asset
        feed.serve(releaseLookupUrl(config), R"({"assets":[{"id":12,"name":"Katana-update-win64.json"}]})");
        EXPECT_EQ(run(feed, config, installedV1()).reason, "asset_missing");
    }
    {
        FakeFeed feed; // not a release at all
        feed.serve(releaseLookupUrl(config), "[1,2,3]");
        EXPECT_EQ(run(feed, config, installedV1()).reason, "malformed");
    }
    {
        FakeFeed feed; // a sign-in page in place of the release
        feed.serve(releaseLookupUrl(config), "<html>sign in</html>");
        EXPECT_EQ(run(feed, config, installedV1()).reason, "malformed");
    }
    {
        FakeFeed feed; // an asset fetched by id that is not what was signed
        serveBehindToken(feed, config, fixture("bad/flipped-manifest-bit"));
        EXPECT_EQ(run(feed, config, installedV1()).reason, "bad_signature")
            << "the bytes fetched by id are verified like any others";
    }
    {
        FakeFeed feed; // and a page in place of an asset
        serveBehindToken(feed, config, fixture("v2"));
        feed.serve(assetApiUrl(config, 12), "<!DOCTYPE html><html></html>");
        EXPECT_EQ(run(feed, config, installedV1()).reason, "html_page");
    }
}

TEST(CheckPrivateRoute, TheTokenIsInNoResultWhateverWentWrong)
{
    const FeedConfig config = privateFeed();
    std::vector<FakeFeed> feeds(4);
    serveBehindToken(feeds[0], config, fixture("v2"));
    serveBehindToken(feeds[1], config, fixture("bad/wrong-key"));
    feeds[2].fail(releaseLookupUrl(config), status(core::ErrorCode::FileImportFailure, 401));
    feeds[3].serve(releaseLookupUrl(config), "not json");
    for (FakeFeed& feed : feeds) {
        const CheckResult result = run(feed, config, installedV1());
        for (const std::string& text : {result.reason, result.detail, result.notesUrl,
                                        result.latestVersion, result.installedVersion}) {
            EXPECT_EQ(text.find(kSecretToken), std::string::npos) << text;
        }
    }
}

// ---- what the person has already decided ------------------------------------------------------------------

CheckResult availableV2()
{
    FakeFeed feed;
    const FeedConfig config = githubFeed();
    feed.serveRelease(config, fixture("v2"));
    CheckResult result = run(feed, config, installedV1());
    EXPECT_EQ(result.status, CheckStatus::Available);
    return result;
}

TEST(CheckState, ASkippedVersionIsCurrentAndABlockedSequenceIsBlocked)
{
    {
        CheckResult result = availableV2();
        UpdateState state;
        state.skippedVersion = "0.10.0";
        applyUpdateState(result, state);
        EXPECT_EQ(result.status, CheckStatus::Current);
        EXPECT_EQ(result.reason, "skipped");
        EXPECT_TRUE(result.manifest.has_value()) << "what was found is kept";
        EXPECT_EQ(result.latestVersion, "0.10.0");
    }
    {
        // Compared as versions: "0.10" is "0.10.0".
        CheckResult result = availableV2();
        UpdateState state;
        state.skippedVersion = "0.10";
        applyUpdateState(result, state);
        EXPECT_EQ(result.status, CheckStatus::Current);
    }
    {
        CheckResult result = availableV2();
        UpdateState state;
        state.blockedSequences = {1, 1792312800, 5};
        applyUpdateState(result, state);
        EXPECT_EQ(result.status, CheckStatus::Blocked);
        EXPECT_EQ(result.reason, "blocked_before");
    }
    {
        // Blocked is the stronger: it is not offered again whatever else was said.
        CheckResult result = availableV2();
        UpdateState state;
        state.blockedSequences = {1792312800};
        state.skippedVersion = "0.10.0";
        applyUpdateState(result, state);
        EXPECT_EQ(result.status, CheckStatus::Blocked);
    }
}

TEST(CheckState, AnotherVersionAnotherSequenceAndAnAnswerThatIsNotAvailableAreLeftAlone)
{
    for (const std::string skipped : {"0.9.0", "0.11.0", "", "banana", "0.10.0.1"}) {
        CheckResult result = availableV2();
        UpdateState state;
        state.skippedVersion = skipped;
        state.blockedSequences = {1792312799, 1792312801};
        applyUpdateState(result, state);
        EXPECT_EQ(result.status, CheckStatus::Available) << "skipped '" << skipped << "'";
        EXPECT_TRUE(result.reason.empty());
    }
    CheckResult unreachable;
    unreachable.status = CheckStatus::Unreachable;
    unreachable.reason = "offline";
    UpdateState everything;
    everything.skippedVersion = "0.10.0";
    everything.blockedSequences = {1792312800};
    applyUpdateState(unreachable, everything);
    EXPECT_EQ(unreachable.status, CheckStatus::Unreachable);
    EXPECT_EQ(unreachable.reason, "offline");
}

TEST(CheckState, EachStatusHasItsWord)
{
    EXPECT_EQ(toString(CheckStatus::Available), "available");
    EXPECT_EQ(toString(CheckStatus::Current), "current");
    EXPECT_EQ(toString(CheckStatus::Unreachable), "unreachable");
    EXPECT_EQ(toString(CheckStatus::Unsupported), "unsupported");
    EXPECT_EQ(toString(CheckStatus::Blocked), "blocked");
}

} // namespace
