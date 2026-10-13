// The contract's own behaviour: the pure functions the skeleton finished itself (see the top
// of src/katana_update/contract.cpp, feed.cpp and arguments.cpp) and the constants and
// names the headers fix. Each case is the regression test of a defect an independent review
// found in the first skeleton, and each expectation is worked out by hand from the rule the
// header states - a calendar of seconds, RFC 3986's unreserved set, UTF-8's bit layout -
// never read off the code under test (CLAUDE.md, section 4).
//
// No later task replaces this file: T4 to T8 own their own test files, and these cases stay
// true when every stub is gone.

#include <gtest/gtest.h>

#include <array>
#include <chrono>
#include <cstdint>
#include <filesystem>
#include <fstream>
#include <string>
#include <string_view>

#include "katana/core/path_text.hpp"
#include "katana/update/apply.hpp"
#include "katana/update/feed.hpp"
#include "katana/update/install.hpp"
#include "katana/update/paths.hpp"
#include "katana/update/process.hpp"
#include "katana/update/stage.hpp"
#include "katana/update/state.hpp"
#include "katana/update/trust.hpp"

using namespace katana;
namespace fs = std::filesystem;

namespace {

// A version written out by hand, so these cases do not depend on parseVersion (task T4).
update::Version versionOf(std::uint32_t major, std::uint32_t minor, std::uint32_t patch)
{
    update::Version version;
    version.components = {major, minor, patch, 0};
    version.written = 3;
    return version;
}

update::Manifest manifestOf(update::Version version, std::uint64_t sequence)
{
    update::Manifest manifest;
    manifest.version = version;
    manifest.sequence = sequence;
    return manifest;
}

update::Install installOf(update::Version version, std::uint64_t sequence, bool hasRecord)
{
    update::Install install;
    install.version = version;
    install.sequence = sequence;
    install.hasInstalledRecord = hasRecord;
    return install;
}

} // namespace

// ---- isNewerThan (trust.hpp) --------------------------------------------------------------

TEST(UpdateContract, AFreshInstallIsNotOfferedTheReleaseItWasMadeFrom)
{
    // The zip a person unpacks is 0.3.0; the feed's manifest for 0.3.0 was built at
    // sequence 1792312800. The install has no installed.json, so its sequence is the
    // placeholder 0 and, compared as a number, every fresh install would be "behind" its own
    // release by 1792312800 and the status bar would offer "Update 0.3.0" on the first start.
    const update::Manifest release = manifestOf(versionOf(0, 3, 0), 1792312800);
    EXPECT_FALSE(update::isNewerThan(release, installOf(versionOf(0, 3, 0), 0, false)));
}

TEST(UpdateContract, AFreshInstallIsStillOfferedAHigherVersion)
{
    const update::Install fresh = installOf(versionOf(0, 3, 0), 0, false);
    EXPECT_TRUE(update::isNewerThan(manifestOf(versionOf(0, 3, 1), 5), fresh));
    EXPECT_TRUE(update::isNewerThan(manifestOf(versionOf(0, 4, 0), 5), fresh));
    EXPECT_TRUE(update::isNewerThan(manifestOf(versionOf(1, 0, 0), 5), fresh));
}

TEST(UpdateContract, AnInstallWithARecordIsOfferedTheSameVersionOnlyAtAHigherSequence)
{
    const update::Version v030 = versionOf(0, 3, 0);
    const update::Install recorded = installOf(v030, 100, true);
    EXPECT_TRUE(update::isNewerThan(manifestOf(v030, 101), recorded));  // a workflow re-run
    EXPECT_FALSE(update::isNewerThan(manifestOf(v030, 100), recorded)); // the same build
    EXPECT_FALSE(update::isNewerThan(manifestOf(v030, 99), recorded));  // an older build
}

TEST(UpdateContract, AHigherVersionIsNewerWhateverTheSequencesAre)
{
    // The version decides first: a re-run of an old release (a huge sequence) cannot
    // outrank a newer version, and a newer version outranks a bigger recorded sequence.
    const update::Install recorded = installOf(versionOf(0, 3, 0), 999999, true);
    EXPECT_TRUE(update::isNewerThan(manifestOf(versionOf(0, 3, 1), 1), recorded));
}

TEST(UpdateContract, ALowerVersionIsNeverNewerWhateverItsSequence)
{
    const std::uint64_t huge = 4'000'000'000'000;
    EXPECT_FALSE(update::isNewerThan(manifestOf(versionOf(0, 2, 9), huge),
                                     installOf(versionOf(0, 3, 0), 1, true)));
    EXPECT_FALSE(update::isNewerThan(manifestOf(versionOf(0, 2, 9), huge),
                                     installOf(versionOf(0, 3, 0), 0, false)));
}

TEST(UpdateContract, VersionsAreComparedAsNumbersAndATrailingZeroIsNotADifference)
{
    // 0.10 is above 0.9 (10 > 9; as text "0.10" < "0.9").
    EXPECT_TRUE(update::isNewerThan(manifestOf(versionOf(0, 10, 0), 1),
                                    installOf(versionOf(0, 9, 0), 0, false)));
    // "0.3" and "0.3.0" are one version (version.hpp), so the shorter spelling is not newer.
    update::Version twoComponents;
    twoComponents.components = {0, 3, 0, 0};
    twoComponents.written = 2;
    EXPECT_FALSE(update::isNewerThan(manifestOf(twoComponents, 7),
                                     installOf(versionOf(0, 3, 0), 0, false)));
}

TEST(UpdateContract, AnInstallDefaultsToNoRecord)
{
    EXPECT_FALSE(update::Install{}.hasInstalledRecord);
}

// ---- installRootOfExecutable (install.hpp) --------------------------------------------------

class InstallRoot : public ::testing::Test {
  protected:
    void SetUp() override
    {
        base_ = fs::temp_directory_path() /
                (std::string("katana_update_contract_") +
                 ::testing::UnitTest::GetInstance()->current_test_info()->name());
        std::error_code ec;
        fs::remove_all(base_, ec);
        fs::create_directories(base_);
    }
    void TearDown() override
    {
        std::error_code ec;
        fs::remove_all(base_, ec);
    }
    fs::path dir(const fs::path& relative)
    {
        fs::create_directories(base_ / relative);
        return base_ / relative;
    }
    fs::path base_;
};

TEST_F(InstallRoot, ABinFolderBesideShareIsTheFlatLayoutOfWindowsAndLinux)
{
    const fs::path root = dir("Katana");
    dir("Katana/bin");
    dir("Katana/share");
    const auto found = update::installRootOfExecutable(root / "bin" / "katana.exe");
    ASSERT_TRUE(found.ok());
    EXPECT_EQ(found.value(), root);
}

TEST_F(InstallRoot, TheNameBinIsMatchedIgnoringCase)
{
    const fs::path root = dir("Katana");
    dir("Katana/BIN");
    dir("Katana/share");
    const auto found = update::installRootOfExecutable(root / "BIN" / "katana.exe");
    ASSERT_TRUE(found.ok());
    EXPECT_EQ(found.value(), root);
}

TEST_F(InstallRoot, AMacOsBundleHasItsRootInContentsResourcesWhereShareIs)
{
    // The layout cmake/KatanaPackaging.cmake gives the bundle: programs in Contents/MacOS, the
    // data - and share/katana/install.json - in Contents/Resources/share. There is no bin/.
    const fs::path resources = dir("Katana.app/Contents/Resources");
    dir("Katana.app/Contents/Resources/share/katana");
    dir("Katana.app/Contents/MacOS");
    const auto found = update::installRootOfExecutable(base_ / "Katana.app" / "Contents" /
                                                       "MacOS" / "katana");
    ASSERT_TRUE(found.ok());
    EXPECT_EQ(found.value(), resources);
}

TEST_F(InstallRoot, AnApplicationBundleWithoutShareInItsResourcesIsNotAnInstall)
{
    dir("Katana.app/Contents/Resources");
    dir("Katana.app/Contents/MacOS");
    const auto found = update::installRootOfExecutable(base_ / "Katana.app" / "Contents" /
                                                       "MacOS" / "katana");
    ASSERT_FALSE(found.ok());
    EXPECT_EQ(found.error().code, core::ErrorCode::NotFound);
}

TEST_F(InstallRoot, ABinFolderWithNoShareBesideItIsNotAnInstall)
{
    dir("Somewhere/bin");
    const auto found = update::installRootOfExecutable(base_ / "Somewhere" / "bin" / "tool");
    ASSERT_FALSE(found.ok());
    EXPECT_EQ(found.error().code, core::ErrorCode::NotFound);
}

TEST_F(InstallRoot, ShareBesideAFolderThatIsNotBinIsNotAnInstall)
{
    dir("Katana/tools");
    dir("Katana/share");
    const auto found = update::installRootOfExecutable(base_ / "Katana" / "tools" / "tool");
    ASSERT_FALSE(found.ok());
}

TEST_F(InstallRoot, AFileNamedShareIsNotTheShareFolder)
{
    dir("Katana/bin");
    std::ofstream(base_ / "Katana" / "share") << "not a directory";
    const auto found = update::installRootOfExecutable(base_ / "Katana" / "bin" / "katana.exe");
    ASSERT_FALSE(found.ok());
}

TEST_F(InstallRoot, ABareFileNameHasNoInstall)
{
    const auto found = update::installRootOfExecutable("katana.exe");
    ASSERT_FALSE(found.ok());
    EXPECT_EQ(found.error().code, core::ErrorCode::NotFound);
}

// ---- the feed's addresses (feed.hpp) ---------------------------------------------------------

namespace {

update::FeedConfig githubFeed(const std::string& target)
{
    update::FeedConfig feed;
    feed.repo = "kjarada/Katana";
    feed.target = target;
    return feed;
}

update::Manifest releaseManifest(const std::string& tag, const std::string& payloadName)
{
    update::Manifest manifest;
    manifest.tag = tag;
    update::Payload payload;
    payload.codec = "deflate";
    payload.name = payloadName;
    manifest.payload = payload;
    return manifest;
}

} // namespace

TEST(UpdateFeedAddresses, TheDownloadRouteNamesTheLatestReleasesFixedAssets)
{
    const update::FeedConfig feed = githubFeed("win64");
    EXPECT_EQ(update::manifestUrl(feed),
              "https://github.com/kjarada/Katana/releases/latest/download/"
              "Katana-update-win64.json");
    EXPECT_EQ(update::signatureUrl(feed),
              "https://github.com/kjarada/Katana/releases/latest/download/"
              "Katana-update-win64.json.sig");
    EXPECT_EQ(update::payloadUrl(feed, releaseManifest("v0.3.0", "Katana-0.3.0-win64.update.zip")),
              "https://github.com/kjarada/Katana/releases/download/v0.3.0/"
              "Katana-0.3.0-win64.update.zip");
}

TEST(UpdateFeedAddresses, AFeedBaseReplacesGithubForAllThree)
{
    update::FeedConfig feed = githubFeed("linux-x86_64");
    feed.feedBase = "https://updates.example.org/katana";
    EXPECT_EQ(update::manifestUrl(feed),
              "https://updates.example.org/katana/Katana-update-linux-x86_64.json");
    EXPECT_EQ(update::signatureUrl(feed),
              "https://updates.example.org/katana/Katana-update-linux-x86_64.json.sig");
    EXPECT_EQ(update::payloadUrl(
                  feed, releaseManifest("v0.3.0", "Katana-0.3.0-linux-x86_64.update.zip")),
              "https://updates.example.org/katana/Katana-0.3.0-linux-x86_64.update.zip");
}

TEST(UpdateFeedAddresses, ANotifyOnlyManifestHasNoPayloadAddress)
{
    update::Manifest manifest;
    manifest.notifyOnly = true;
    manifest.tag = "v0.3.0";
    EXPECT_TRUE(update::payloadUrl(githubFeed("macos-arm64"), manifest).empty());
}

TEST(UpdateFeedAddresses, ASignedNameCannotAddASegmentOrAQueryToTheAddress)
{
    // RFC 3986: only A-Z a-z 0-9 - . _ ~ pass; every other byte is %XX in upper-case hex. '/'
    // is 0x2F, ' ' is 0x20, '?' is 0x3F, '#' is 0x23, '%' itself is 0x25.
    const update::FeedConfig feed = githubFeed("win64");
    EXPECT_EQ(update::payloadUrl(feed, releaseManifest("v1/../2", "a b?c#d%e.zip")),
              "https://github.com/kjarada/Katana/releases/download/v1%2F..%2F2/"
              "a%20b%3Fc%23d%25e.zip");
    EXPECT_EQ(update::releaseByTagUrl(feed, "v1/../2"),
              "https://api.github.com/repos/kjarada/Katana/releases/tags/v1%2F..%2F2");
}

TEST(UpdateFeedAddresses, ThePrivateRepositoryRouteIsTheRestApiOnApiGithubCom)
{
    const update::FeedConfig feed = githubFeed("win64");
    EXPECT_EQ(update::releaseLookupUrl(feed),
              "https://api.github.com/repos/kjarada/Katana/releases/latest");
    EXPECT_EQ(update::releaseByTagUrl(feed, "v0.3.0"),
              "https://api.github.com/repos/kjarada/Katana/releases/tags/v0.3.0");
    EXPECT_EQ(update::assetApiUrl(feed, 7654321),
              "https://api.github.com/repos/kjarada/Katana/releases/assets/7654321");
    // The route is GitHub's: a feedBase does not move it. And the token's one destination,
    // api.github.com over https, is where every one of these goes.
    update::FeedConfig mirror = feed;
    mirror.feedBase = "https://updates.example.org/katana";
    for (const std::string& url : {update::releaseLookupUrl(mirror),
                                   update::releaseByTagUrl(mirror, "v1"),
                                   update::assetApiUrl(mirror, 1)}) {
        EXPECT_EQ(url.rfind("https://api.github.com/", 0), 0u) << url;
    }
}

TEST(UpdateFeedAddresses, NoDownloadRouteAddressIsOnTheApiHost)
{
    // The token is sent only to api.github.com, so a request the Download route makes must
    // never be one the transport could attach it to.
    const update::FeedConfig feed = githubFeed("win64");
    const update::Manifest manifest = releaseManifest("v0.3.0", "Katana-0.3.0-win64.update.zip");
    for (const std::string& url : {update::manifestUrl(feed), update::signatureUrl(feed),
                                   update::payloadUrl(feed, manifest)}) {
        EXPECT_EQ(url.rfind("https://github.com/", 0), 0u) << url;
    }
}

TEST(UpdateFeedAddresses, TheAssetNamesAreTheFixedOnesOfTheTarget)
{
    EXPECT_EQ(update::manifestAssetName("win-arm64"), "Katana-update-win-arm64.json");
    EXPECT_EQ(update::signatureAssetName("win-arm64"), "Katana-update-win-arm64.json.sig");
}

TEST(UpdateFeedAddresses, TheRouteDefaultsToDownloadSoNoTokenGoesAnywhereUnlessCheckSaysSo)
{
    EXPECT_EQ(update::FeedConfig{}.route, update::FeedRoute::Download);
    EXPECT_TRUE(update::FetchRequest{}.bearerToken.empty());
    EXPECT_TRUE(update::FetchRequest{}.accept.empty());
}

TEST(UpdateFeedAddresses, WhatIsFetchedBeforeVerificationHasABoundTheEngineSets)
{
    // The signature line is `ed25519 ` (8) + the key id (16) + ' ' (1) + base64 of 64 bytes
    // (4 * ceil(64 / 3) = 88) + CRLF (2) = 115 bytes at most.
    EXPECT_GE(update::kMaxSignatureBytes, 115u);
    // A manifest of kMaxFiles entries at about 400 bytes each (a 260-character path, a
    // 64-digit hash, the sizes and offsets and the field names) must fit.
    EXPECT_GE(update::kMaxManifestBytes, update::kMaxFiles * 400u);
    EXPECT_GT(update::kMaxReleaseJsonBytes, 0u);
    // None is unbounded.
    EXPECT_LT(update::kMaxManifestBytes, std::uint64_t{1} << 30);
}

// ---- the staged manifest (stage.hpp)  -------------------------------------------------------

TEST(UpdateStagedManifest, ItsFilesAreNamedAtTheTopOfTheStageFolder)
{
    EXPECT_EQ(update::stagedManifestPath("S"), fs::path("S") / "manifest.json");
    EXPECT_EQ(update::stagedSignaturePath("S"), fs::path("S") / "manifest.json.sig");
    // The signature file is the manifest's name and ".sig", as in the feed.
    EXPECT_EQ(std::string(update::kStagedSignatureName),
              std::string(update::kStagedManifestName) + ".sig");
}

TEST(UpdateStagedManifest, NoManifestPathCanBeAMemberOfTheStageFolderRoot)
{
    // A manifest path starts with bin, share, etc or lib (paths.hpp). The staged manifest's
    // names are at the root and are neither, so a staged file can never overwrite them.
    for (const std::string_view name :
         {update::kStagedManifestName, update::kStagedSignatureName}) {
        for (const std::string_view root : update::kInstallRoots) {
            EXPECT_NE(name, root);
        }
        EXPECT_EQ(name.find('/'), std::string_view::npos);
    }
}

// ---- how an apply ends (apply.hpp, state.hpp)  ----------------------------------------------

TEST(UpdateOutcomes, EveryOutcomeHasItsOwnLastResultStatus)
{
    EXPECT_EQ(update::toString(update::ApplyOutcome::Committed), "committed");
    EXPECT_EQ(update::toString(update::ApplyOutcome::Blocked), "blocked");
    EXPECT_EQ(update::toString(update::ApplyOutcome::RolledBack), "rolled_back");
    EXPECT_EQ(update::toString(update::ApplyOutcome::Refused), "refused");
    // A rollback that itself failed is not "rolled_back": the tree may be half swapped.
    EXPECT_EQ(update::toString(update::ApplyOutcome::RollbackIncomplete), "rollback_incomplete");
    EXPECT_NE(update::toString(update::ApplyOutcome::RollbackIncomplete),
              update::toString(update::ApplyOutcome::RolledBack));
}

// ---- transaction ids (apply.hpp)  -----------------------------------------------------------

TEST(UpdateTransactionIds, LowerCaseHexAndDigitsOfOneToSixtyFourCharactersAreValid)
{
    EXPECT_TRUE(update::isValidTransactionId("0"));
    EXPECT_TRUE(update::isValidTransactionId("0a1b2c3d"));
    EXPECT_TRUE(update::isValidTransactionId(std::string(64, 'f')));
}

TEST(UpdateTransactionIds, AnythingThatCouldNameAPlaceOutsideTheFolderItIsJoinedToIsRefused)
{
    EXPECT_FALSE(update::isValidTransactionId(""));
    EXPECT_FALSE(update::isValidTransactionId(std::string(65, 'a')));
    EXPECT_FALSE(update::isValidTransactionId("A1"));      // upper case
    EXPECT_FALSE(update::isValidTransactionId("g1"));      // not hexadecimal
    EXPECT_FALSE(update::isValidTransactionId(".."));
    EXPECT_FALSE(update::isValidTransactionId("a/b"));
    EXPECT_FALSE(update::isValidTransactionId("a\\b"));
    EXPECT_FALSE(update::isValidTransactionId("a b"));
    EXPECT_FALSE(update::isValidTransactionId("c:"));
    EXPECT_FALSE(update::isValidTransactionId(std::string("ab\0cd", 5)));
}

// ---- the automatic check's clock (state.hpp)  -----------------------------------------------

namespace {

constexpr std::int64_t kDay = 86400; // 24 * 60 * 60
constexpr std::int64_t kLast = 1'000'000;

update::UpdateState stateChecked(std::int64_t at, bool automatic = true)
{
    update::UpdateState state;
    state.lastCheckEpoch = at;
    state.automatic = automatic;
    return state;
}

} // namespace

TEST(UpdateCheckClock, TheConstantsAreADayAndTwoHours)
{
    EXPECT_EQ(update::kCheckInterval.count(), 86400);
    EXPECT_EQ(update::kCheckJitterMax.count(), 7200);
}

TEST(UpdateCheckClock, ACheckIsDueOnceADayHasPassed)
{
    using std::chrono::seconds;
    EXPECT_FALSE(update::checkIsDue(stateChecked(kLast), kLast + kDay - 1, seconds{0}));
    EXPECT_TRUE(update::checkIsDue(stateChecked(kLast), kLast + kDay, seconds{0}));
    EXPECT_FALSE(update::checkIsDue(stateChecked(kLast), kLast, seconds{0}));
}

TEST(UpdateCheckClock, TheJitterDelaysTheCheckBySoMuch)
{
    using std::chrono::seconds;
    // 24 h + 1 h = 90000 s.
    EXPECT_FALSE(update::checkIsDue(stateChecked(kLast), kLast + 89999, seconds{3600}));
    EXPECT_TRUE(update::checkIsDue(stateChecked(kLast), kLast + 90000, seconds{3600}));
}

TEST(UpdateCheckClock, AJitterBeyondTwoHoursIsClampedAndANegativeOneToZero)
{
    using std::chrono::seconds;
    // Clamped to 7200: due at 86400 + 7200 = 93600, not at 86400 + 10000.
    EXPECT_FALSE(update::checkIsDue(stateChecked(kLast), kLast + 93599, seconds{10000}));
    EXPECT_TRUE(update::checkIsDue(stateChecked(kLast), kLast + 93600, seconds{10000}));
    // Clamped to 0: due at 86400.
    EXPECT_TRUE(update::checkIsDue(stateChecked(kLast), kLast + kDay, seconds{-5}));
}

TEST(UpdateCheckClock, ACheckThatNeverHappenedIsDueAndOneTurnedOffIsNot)
{
    using std::chrono::seconds;
    EXPECT_TRUE(update::checkIsDue(stateChecked(0), 5, seconds{0}));
    EXPECT_TRUE(update::checkIsDue(stateChecked(-1), 5, seconds{0}));
    // The person turned the automatic check off: never due, whatever the clock says.
    EXPECT_FALSE(update::checkIsDue(stateChecked(0, false), kLast, seconds{0}));
    EXPECT_FALSE(update::checkIsDue(stateChecked(kLast, false), kLast + 10 * kDay, seconds{0}));
}

TEST(UpdateCheckClock, ALastCheckInTheFutureDoesNotSilenceTheCheck)
{
    // The clock was set back after the last check: a stored time that cannot be right.
    using std::chrono::seconds;
    EXPECT_TRUE(update::checkIsDue(stateChecked(kLast + 1), kLast, seconds{0}));
}

// ---- the command line as UTF-8 (process.hpp)  -----------------------------------------------

#ifdef _WIN32

TEST(UpdateArguments, AnAccentedLetterIsItsTwoByteUtf8Sequence)
{
    // U+00E9 is 110_00011 10_101001 = C3 A9.
    EXPECT_EQ(update::utf8FromWide(L"Jos\u00e9"), "Jos\xC3\xA9");
}

TEST(UpdateArguments, ACharacterOfTheBasicPlaneIsItsThreeByteSequence)
{
    // U+20AC (the euro sign) is 1110_0010 10_000010 10_101100 = E2 82 AC.
    EXPECT_EQ(update::utf8FromWide(L"\u20ac"), "\xE2\x82\xAC");
}

TEST(UpdateArguments, ASurrogatePairIsOneFourByteSequence)
{
    // U+1F600 = D83D DE00 in UTF-16, and 11110_000 10_011111 10_011000 10_000000 = F0 9F 98 80.
    EXPECT_EQ(update::utf8FromWide(std::wstring{static_cast<wchar_t>(0xD83D),
                                                static_cast<wchar_t>(0xDE00)}),
              "\xF0\x9F\x98\x80");
}

TEST(UpdateArguments, AnUnpairedSurrogateBecomesTheReplacementCharacter)
{
    // U+FFFD is EF BF BD. A high surrogate alone, a low one alone, and a high one followed
    // by an ordinary character (which must survive).
    EXPECT_EQ(update::utf8FromWide(std::wstring{static_cast<wchar_t>(0xD800)}), "\xEF\xBF\xBD");
    EXPECT_EQ(update::utf8FromWide(std::wstring{static_cast<wchar_t>(0xDC00)}), "\xEF\xBF\xBD");
    EXPECT_EQ(update::utf8FromWide(std::wstring{static_cast<wchar_t>(0xD800), L'A'}),
              "\xEF\xBF\xBD" "A");
}

TEST(UpdateArguments, AnEmptyAndAnAsciiStringConvertAsTheyAre)
{
    EXPECT_EQ(update::utf8FromWide(L""), "");
    EXPECT_EQ(update::utf8FromWide(L"--root C:\\Katana"), "--root C:\\Katana");
}

TEST(UpdateArguments, TheRunningProgramsOwnCommandLineIsRead)
{
    // The command line is the operating system's, not argv: arguments are ignored on Windows.
    const std::vector<std::string> arguments = update::utf8Arguments(0, nullptr);
    ASSERT_FALSE(arguments.empty());
    EXPECT_EQ(core::pathFromUtf8(arguments.front()).stem().string(), "katana_update_tests");
}

#else

TEST(UpdateArguments, TheArgumentsAreReturnedAsGiven)
{
    char first[] = "katana_updater";
    char second[] = "--root";
    char third[] = "/home/jos\xC3\xA9/Katana";
    char* argv[] = {first, second, third};
    const std::vector<std::string> arguments = update::utf8Arguments(3, argv);
    ASSERT_EQ(arguments.size(), 3u);
    EXPECT_EQ(arguments[0], "katana_updater");
    EXPECT_EQ(arguments[1], "--root");
    EXPECT_EQ(arguments[2], "/home/jos\xC3\xA9/Katana");
    EXPECT_TRUE(update::utf8Arguments(0, nullptr).empty());
}

#endif
