// The swap (include/katana/update/apply.hpp), one case at a time. The loops that break it at
// every file system operation are in test_fault_loops.cpp; rollback and recovery are in
// test_recover.cpp.
//
// Where the expectations come from (CLAUDE.md, section 4):
//   - the tree an update must leave is built from the specification of the release, in its own
//     folder (apply_harness.hpp), or from the T3 fixtures made by the Python producer;
//   - the signatures are made with OpenSSL, or are the T3 fixtures' (`openssl pkeyutl`);
//   - the command-line quoting is checked against the operating system's own parser
//     (CommandLineToArgvW) and against the examples Microsoft publishes.

#include <gtest/gtest.h>

#include <cctype>
#include <fstream>
#include <stdexcept>
#include <thread>

#include "apply_harness.hpp"
#include "command_line.hpp"
#include "recover_internal.hpp"
#include "scripted_processes.hpp"

#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>
#include <shellapi.h>
#endif

using namespace katana;
using namespace katana::update;
using namespace katana::update::harness;

namespace {

// ---- helpers --------------------------------------------------------------------------------------

void expectUntouched(const Scenario& s, const ApplyResult& result)
{
    EXPECT_EQ(treeHash(s.root), s.v1Hash)
        << difference(s.v1Snapshot, snapshot(s.root)) << result.detail;
    EXPECT_TRUE(leftoversInUpdate(s.root).empty()) << "a refused update leaves no transaction folder";
}

void expectRefused(const Scenario& s, const ApplyResult& result, std::string_view reason)
{
    EXPECT_EQ(result.outcome, ApplyOutcome::Refused) << result.detail;
    EXPECT_EQ(result.reason, reason) << result.detail;
    expectUntouched(s, result);
}

LastResult lastOf(const Scenario& s)
{
    const core::Result<std::optional<LastResult>> read = readLastResult(s.state);
    EXPECT_TRUE(read.ok());
    EXPECT_TRUE(read.ok() && read->has_value()) << "last.json was not written";
    return read.ok() && read->has_value() ? **read : LastResult{};
}

ReleaseSpec smallRelease(const std::string& version, std::uint64_t sequence)
{
    ReleaseSpec release;
    release.version = version;
    release.tag = "v" + version;
    release.sequence = sequence;
    release.files = {
        {"bin/katana.exe", "katana " + version + "\n", FileClass::Program, true},
        {"bin/lib.dll", "lib " + version + "\n", FileClass::Lib, false},
        {"share/katana/data.txt", "data " + version + "\n", FileClass::Data, false},
    };
    return release;
}

// A scenario from two small releases written out by the harness (the fixtures are the other
// source). Everything in the install is v1's; the stage is v2's.
void buildSmallScenario(Scenario& s)
{
    s.v1 = smallRelease("0.1.0", 100);
    s.v2 = smallRelease("0.2.0", 200);
    writeInstall(s.root, s.v1);
    writeStage(s.stage, s.v2);
    s.v1Hash = treeHash(s.root);
    s.v1Snapshot = snapshot(s.root);
}

} // namespace

// ---- the helper's command line --------------------------------------------------------------------------

TEST(HelperArguments, ARequestIsTheSixOptionsInTheDocumentedOrder)
{
    ApplyRequest request;
    request.root = core::pathFromUtf8("/opt/katana");
    request.stageDirectory = core::pathFromUtf8("/var/u/stage-5");
    request.stateDirectory = core::pathFromUtf8("/var/u");
    request.transaction = "abc123";
    request.waitPid = 4412;
    request.relaunch = core::pathFromUtf8("/opt/katana/bin/katana");
    const std::vector<std::string> expected = {
        "--root",     "/opt/katana",        "--stage",    "/var/u/stage-5",
        "--state",    "/var/u",             "--tx",       "abc123",
        "--wait-pid", "4412",               "--relaunch", "/opt/katana/bin/katana"};
    EXPECT_EQ(helperArguments(request), expected);
}

TEST(HelperArguments, ARequestWithASpaceAndANonAsciiLetterInItsPathsComesBackWhole)
{
    ApplyRequest request;
    request.root = core::pathFromUtf8("C:/Users/Jos\xC3\xA9 M\xC3\xBCller/Programs/Katana Install");
    request.stageDirectory = core::pathFromUtf8("C:/Users/Jos\xC3\xA9 M\xC3\xBCller/AppData/Katana/update/stage-1792312800");
    request.stateDirectory = core::pathFromUtf8("C:/Users/Jos\xC3\xA9 M\xC3\xBCller/AppData/Katana/update");
    request.transaction = "0123456789abcdef0123456789abcdef";
    request.waitPid = 4294967295ull;
    request.relaunch = core::pathFromUtf8("C:/Users/Jos\xC3\xA9 M\xC3\xBCller/Programs/Katana Install/bin/katana.exe");

    const std::vector<std::string> arguments = helperArguments(request);
    const core::Result<ApplyRequest> parsed = parseHelperArguments(arguments);
    ASSERT_TRUE(parsed.ok()) << parsed.error().describe();
    EXPECT_EQ(parsed->root, request.root);
    EXPECT_EQ(parsed->stageDirectory, request.stageDirectory);
    EXPECT_EQ(parsed->stateDirectory, request.stateDirectory);
    EXPECT_EQ(parsed->transaction, request.transaction);
    EXPECT_EQ(parsed->waitPid, request.waitPid);
    EXPECT_EQ(parsed->relaunch, request.relaunch);
    EXPECT_TRUE(parsed->helper.empty()) << "the helper is argv[0]'s copy, not on the command line";
    // The UTF-8 is carried as it is.
    EXPECT_EQ(arguments[1], "C:/Users/Jos\xC3\xA9 M\xC3\xBCller/Programs/Katana Install");
}

TEST(HelperArguments, NoRelaunchIsTheWordNoneAndComesBackEmpty)
{
    ApplyRequest request;
    request.root = core::pathFromUtf8("/r");
    request.stageDirectory = core::pathFromUtf8("/s");
    request.stateDirectory = core::pathFromUtf8("/u");
    request.transaction = "1";
    const std::vector<std::string> arguments = helperArguments(request);
    ASSERT_EQ(arguments.size(), 12u);
    EXPECT_EQ(arguments[10], "--relaunch");
    EXPECT_EQ(arguments[11], "none");
    EXPECT_EQ(arguments[9], "0");
    const core::Result<ApplyRequest> parsed = parseHelperArguments(arguments);
    ASSERT_TRUE(parsed.ok());
    EXPECT_TRUE(parsed->relaunch.empty());
    EXPECT_EQ(parsed->waitPid, 0u);
}

namespace {

std::vector<std::string> goodArguments()
{
    return {"--root", "/r", "--stage", "/s", "--state", "/u", "--tx", "ab12", "--wait-pid", "7",
            "--relaunch", "none"};
}

} // namespace

TEST(HelperArguments, ATransactionIdThatIsNotLowerCaseHexadecimalIsRefused)
{
    // It names a folder under R/.update and one in the state folder, and a command line is
    // not trusted to name only that.
    for (const std::string& tx : {std::string(""), std::string(".."), std::string("../x"),
                                 std::string("ABC"), std::string("g1"), std::string("a b"),
                                 std::string("a/b"), std::string("a\\b"), std::string("C:"),
                                 std::string(65, 'a'), std::string("caf\xC3\xA9")}) {
        std::vector<std::string> arguments = goodArguments();
        arguments[7] = tx;
        const core::Result<ApplyRequest> parsed = parseHelperArguments(arguments);
        ASSERT_FALSE(parsed.ok()) << "accepted '" << tx << "'";
        EXPECT_EQ(parsed.error().code, core::ErrorCode::InvalidArgument);
    }
    std::vector<std::string> longest = goodArguments();
    longest[7] = std::string(64, 'f');
    EXPECT_TRUE(parseHelperArguments(longest).ok()) << "64 characters is the longest id";
}

TEST(HelperArguments, AMissingAnUnknownARepeatedAndAValuelessOptionAreRefused)
{
    const std::vector<std::string> good = goodArguments();
    ASSERT_TRUE(parseHelperArguments(good).ok());
    // Each option left out in turn.
    for (std::size_t i = 0; i < good.size(); i += 2) {
        std::vector<std::string> fewer = good;
        fewer.erase(fewer.begin() + static_cast<std::ptrdiff_t>(i), fewer.begin() + static_cast<std::ptrdiff_t>(i) + 2);
        const core::Result<ApplyRequest> parsed = parseHelperArguments(fewer);
        ASSERT_FALSE(parsed.ok()) << "without " << good[i];
        EXPECT_NE(parsed.error().message.find("missing option"), std::string::npos);
        EXPECT_EQ(parsed.error().context, good[i]);
    }
    // An option nobody defined.
    std::vector<std::string> unknown = good;
    unknown.insert(unknown.end(), {"--elevate", "yes"});
    EXPECT_FALSE(parseHelperArguments(unknown).ok());
    // The same option twice, even with the same value.
    std::vector<std::string> twice = good;
    twice.insert(twice.end(), {"--root", "/r"});
    const core::Result<ApplyRequest> repeated = parseHelperArguments(twice);
    ASSERT_FALSE(repeated.ok());
    EXPECT_NE(repeated.error().message.find("twice"), std::string::npos);
    // An option with nothing after it.
    std::vector<std::string> valueless = good;
    valueless.push_back("--root");
    EXPECT_FALSE(parseHelperArguments(valueless).ok());
    // A bare word.
    std::vector<std::string> bare = good;
    bare.insert(bare.begin(), "stray");
    EXPECT_FALSE(parseHelperArguments(bare).ok());
    EXPECT_FALSE(parseHelperArguments({}).ok());
}

TEST(HelperArguments, AProcessIdThatIsNotANumberAndAnEmptyFolderAreRefused)
{
    for (const std::string& pid : {std::string(""), std::string("abc"), std::string("-1"),
                                  std::string("12x"), std::string(" 5"), std::string("5 "),
                                  std::string("99999999999999999999999")}) {
        std::vector<std::string> arguments = goodArguments();
        arguments[9] = pid;
        EXPECT_FALSE(parseHelperArguments(arguments).ok()) << "accepted '" << pid << "'";
    }
    for (const std::size_t position : {1u, 3u, 5u}) {
        std::vector<std::string> arguments = goodArguments();
        arguments[position] = "";
        EXPECT_FALSE(parseHelperArguments(arguments).ok()) << goodArguments()[position - 1];
    }
}

TEST(HelperArguments, TheHelperIsStartedDetachedWithTheArgumentsAndFromItsOwnFolder)
{
    ApplyRequest request;
    request.root = core::pathFromUtf8("/r");
    request.stageDirectory = core::pathFromUtf8("/s");
    request.stateDirectory = core::pathFromUtf8("/u");
    request.transaction = "ab";
    request.helper = core::pathFromUtf8("/u/run-ab/katana_updater");
    FakeLauncher launcher;
    ASSERT_TRUE(launchApply(request, launcher).ok());
    ASSERT_EQ(launcher.started.size(), 1u);
    EXPECT_TRUE(launcher.ran.empty()) << "detached, not waited for";
    EXPECT_EQ(launcher.started[0].executable, request.helper);
    EXPECT_EQ(launcher.started[0].arguments, helperArguments(request));
    // Not the install folder: a program whose working directory is inside the install keeps
    // that folder from being moved.
    EXPECT_EQ(launcher.started[0].workingDirectory, request.helper.parent_path());

    ApplyRequest none = request;
    none.helper.clear();
    const core::Status refused = launchApply(none, launcher);
    EXPECT_FALSE(refused.ok());
    EXPECT_EQ(launcher.started.size(), 1u);
}

TEST(HelperArguments, InstalledProgramIsBinUnderTheRootWithTheExtensionOfTheSystem)
{
    const Path root = core::pathFromUtf8("/opt/katana");
#ifdef _WIN32
    EXPECT_EQ(installedProgram(root, "katana"), root / "bin" / "katana.exe");
#else
    EXPECT_EQ(installedProgram(root, "katana"), root / "bin" / "katana");
#endif
}

TEST(TransactionId, ANewIdIsValidAsLongAsTheDocumentationSaysAndNeverRepeats)
{
    std::set<std::string> seen;
    for (int i = 0; i < 2000; ++i) {
        const std::string id = newTransactionId();
        ASSERT_TRUE(isValidTransactionId(id)) << id;
        EXPECT_GE(id.size(), 16u) << "enough that two runs do not collide";
        EXPECT_TRUE(seen.insert(id).second) << "repeated: " << id;
    }
}

TEST(TransactionId, IdsMadeAtTheSameTimeByManyThreadsAreAllDifferent)
{
    // The counter that is part of each id is shared by the threads.
    constexpr int kThreads = 8;
    constexpr int kEach = 250;
    std::vector<std::vector<std::string>> made(kThreads);
    {
        std::vector<std::jthread> threads;
        for (int t = 0; t < kThreads; ++t) {
            threads.emplace_back([&made, t] {
                for (int i = 0; i < kEach; ++i) {
                    made[static_cast<std::size_t>(t)].push_back(newTransactionId());
                }
            });
        }
    }
    std::set<std::string> seen;
    for (const std::vector<std::string>& ids : made) {
        for (const std::string& id : ids) {
            ASSERT_TRUE(isValidTransactionId(id)) << id;
            EXPECT_TRUE(seen.insert(id).second) << "repeated: " << id;
        }
    }
    EXPECT_EQ(seen.size(), static_cast<std::size_t>(kThreads * kEach));
}

// ---- command-line quoting ---------------------------------------------------------------------------------

TEST(CommandLine, ArgumentsAreQuotedByTheRulesWindowsParsesThemWith)
{
    using detail::quoteArgument;
    // Microsoft's rules ("Parsing C++ command-line arguments"): an ordinary word is bare.
    EXPECT_EQ(quoteArgument("plain"), "plain");
    EXPECT_EQ(quoteArgument("--root"), "--root");
    // A space (or tab) wraps it in quotes; an empty argument is two quotes.
    EXPECT_EQ(quoteArgument("a b"), "\"a b\"");
    EXPECT_EQ(quoteArgument("a\tb"), "\"a\tb\"");
    EXPECT_EQ(quoteArgument(""), "\"\"");
    // A quote inside gets a backslash.
    EXPECT_EQ(quoteArgument("say \"hi\""), "\"say \\\"hi\\\"\"");
    EXPECT_EQ(quoteArgument("a\"b"), "\"a\\\"b\"");
    // Backslashes are literal unless they end at a quote: then they double.
    EXPECT_EQ(quoteArgument("C:\\dir\\file"), "C:\\dir\\file");
    EXPECT_EQ(quoteArgument("C:\\my dir\\file"), "\"C:\\my dir\\file\"");
    EXPECT_EQ(quoteArgument("C:\\my dir\\"), "\"C:\\my dir\\\\\"");   // trailing backslash doubled
    EXPECT_EQ(quoteArgument("a\\\"b"), "\"a\\\\\\\"b\"");             // one backslash before a quote: 2n+1
    EXPECT_EQ(quoteArgument("\\\\server\\share x"), "\"\\\\server\\share x\"");
}

TEST(CommandLine, AProgramIsAlwaysQuotedBecauseAPathMayHoldASpace)
{
    EXPECT_EQ(detail::commandLine("C:\\Program Files\\k\\katana_updater.exe", {"--recover", "C:\\My Dir"}),
              "\"C:\\Program Files\\k\\katana_updater.exe\" --recover \"C:\\My Dir\"");
    EXPECT_EQ(detail::commandLine("C:\\k\\u.exe", {}), "\"C:\\k\\u.exe\"");
}

#ifdef _WIN32
TEST(CommandLine, TheOperatingSystemsOwnParserReadsBackWhatWasWritten)
{
    const std::vector<std::string> arguments = {
        "plain", "a b", "", "say \"hi\"", "C:\\dir\\file", "C:\\my dir\\", "a\\\"b", "\\\\server\\share x",
        "--relaunch", "none", "tab\there", "trailing\\\\", "two  spaces", "quote\"", "\"", "\\", "\\\\\""};
    const std::string line = detail::commandLine("C:\\Program Files\\Katana\\bin\\katana.exe", arguments);
    const std::wstring wide(line.begin(), line.end()); // ASCII only
    int count = 0;
    wchar_t** parsed = CommandLineToArgvW(wide.c_str(), &count);
    ASSERT_NE(parsed, nullptr);
    ASSERT_EQ(static_cast<std::size_t>(count), arguments.size() + 1);
    EXPECT_EQ(utf8FromWide(parsed[0]), "C:\\Program Files\\Katana\\bin\\katana.exe");
    for (std::size_t i = 0; i < arguments.size(); ++i) {
        EXPECT_EQ(utf8FromWide(parsed[i + 1]), arguments[i]) << "argument " << i;
    }
    LocalFree(parsed);
}
#endif

// ---- a good update -----------------------------------------------------------------------------------------

TEST(Apply, TheFixtureReleaseReplacesTheInstallAndLeavesExactlyTheTreeOfTheNewVersion)
{
    Scenario s("apply-fixture");
    buildFixtureScenario(s);
    ASSERT_NE(s.v1Hash, s.v2Hash) << "the fixtures differ";
    StdFs disk;
    Rig rig(s, disk);
    const ApplyResult result = rig.run();

    ASSERT_EQ(result.outcome, ApplyOutcome::Committed) << result.reason << ": " << result.detail;
    EXPECT_EQ(treeHash(s.root), s.v2Hash) << difference(snapshot(s.expected), snapshot(s.root));
    EXPECT_TRUE(leftoversInUpdate(s.root).empty()) << "a healthy transaction deletes its folder";
}

TEST(Apply, TheFixtureUpdateDoesWhatTheReleaseSaysFileByFile)
{
    Scenario s("apply-fixture-facts");
    buildFixtureScenario(s);
    StdFs disk;
    Rig rig(s, disk);
    ASSERT_EQ(rig.run().outcome, ApplyOutcome::Committed);

    const Path& root = s.root;
    const Path v1 = fixtureFolder() / "install/v1";
    const Path v2 = fixtureFolder() / "install/v2";
    // Changed: the new bytes.
    EXPECT_EQ(readText(root / "bin/katana.exe"), readText(v2 / "bin/katana.exe"));
    EXPECT_NE(readText(root / "bin/katana.exe"), readText(v1 / "bin/katana.exe"));
    // Unchanged (the same hash in both releases): untouched, and still the same bytes.
    EXPECT_EQ(readText(root / "bin/katana_cli.exe"), readText(v1 / "bin/katana_cli.exe"));
    EXPECT_EQ(readText(root / "share/katana/random.bin"), readText(v2 / "share/katana/random.bin"));
    // Added, with the folder it needs.
    EXPECT_EQ(readText(root / "bin/libnew-2.dll"), readText(v2 / "bin/libnew-2.dll"));
    EXPECT_EQ(readText(root / "share/katana/help/index.txt"), readText(v2 / "share/katana/help/index.txt"));
    // Removed: v1 listed them, v2 does not, and nobody had touched them; the folder the
    // removal emptied goes too.
    EXPECT_FALSE(isPresent(root / "bin/libold-1.dll"));
    EXPECT_FALSE(isPresent(root / "share/katana/legacy/old.txt"));
    EXPECT_FALSE(isPresent(root / "share/katana/legacy"));
    // The seed was there, so it was not rewritten: v1's bytes, not v2's.
    EXPECT_EQ(readText(root / "etc/katana/settings.ini"), readText(v1 / "etc/katana/settings.ini"));
    EXPECT_NE(readText(root / "etc/katana/settings.ini"), readText(v2 / "etc/katana/settings.ini"));
    // The records: installed.json by hand, the manifest and signature byte for byte.
    EXPECT_EQ(readText(root / "share/katana/installed.json"),
              "{\"product\":\"katana\",\"schema\":1,\"sequence\":1792312800,\"tag\":\"v0.10.0\","
              "\"target\":\"win64\",\"version\":\"0.10.0\"}\n");
    EXPECT_EQ(readText(root / "share/katana/installed.manifest.json"),
              readText(fixtureFolder() / "release/v2/Katana-update-win64.json"));
    EXPECT_EQ(readText(root / "share/katana/installed.manifest.json.sig"),
              readText(fixtureFolder() / "release/v2/Katana-update-win64.json.sig"));
    // install.json is the installer's and the update never writes it.
    EXPECT_EQ(readText(root / "share/katana/install.json"), std::string(kInstallJson));
}

TEST(Apply, TheStageIsCleanedAfterAHealthyUpdateAndTheMarkerGoes)
{
    Scenario s("apply-clean");
    buildFixtureScenario(s);
    StdFs disk;
    Rig rig(s, disk);
    ASSERT_EQ(rig.run().outcome, ApplyOutcome::Committed);
    EXPECT_FALSE(isPresent(s.stage / "bin/katana.exe"));
    EXPECT_FALSE(isPresent(s.stage / "manifest.json"));
    EXPECT_FALSE(isPresent(s.stage / "manifest.json.sig"));
    EXPECT_FALSE(isPresent(s.stage)) << "the folder goes once it is empty";
    EXPECT_FALSE(isPresent(healthMarkerPath(s.state, rig.tx)));
}

TEST(Apply, OnlyFilesTheManifestNamesAreDeletedFromTheStageBecauseItsPathCameFromACommandLine)
{
    Scenario s("apply-stage-careful");
    buildFixtureScenario(s);
    writeText(s.stage / "mine.txt", "the person's file, which happens to be in the folder named as the stage");
    writeText(s.stage / "sub/also-mine.txt", "x");
    StdFs disk;
    Rig rig(s, disk);
    ASSERT_EQ(rig.run().outcome, ApplyOutcome::Committed);
    EXPECT_EQ(readText(s.stage / "mine.txt"), "the person's file, which happens to be in the folder named as the stage");
    EXPECT_EQ(readText(s.stage / "sub/also-mine.txt"), "x");
    EXPECT_FALSE(isPresent(s.stage / "bin")) << "the manifest's own folders emptied";
}

TEST(Apply, ItStartsTheNewVersionToCheckItAndThenRelaunchesTheProgramItWasToldTo)
{
    Scenario s("apply-launches");
    buildFixtureScenario(s);
    StdFs disk;
    Rig rig(s, disk);
    ASSERT_EQ(rig.run().outcome, ApplyOutcome::Committed);

    // The health check: R/bin/katana --update-health <tx>, waited for, with the health wait.
    ASSERT_EQ(rig.launcher.ran.size(), 1u);
    EXPECT_EQ(rig.launcher.ran[0].executable, installedProgram(s.root, "katana"));
    EXPECT_EQ(rig.launcher.ran[0].arguments, (std::vector<std::string>{"--update-health", rig.tx}));
    EXPECT_EQ(rig.launcher.ran[0].timeout, rig.timings.healthWait);
    // And then the relaunch, detached, exactly once.
    ASSERT_EQ(rig.launcher.started.size(), 1u);
    EXPECT_EQ(rig.launcher.started[0].executable, rig.request.relaunch);
}

TEST(Apply, NoRelaunchPathMeansNothingIsStartedAfterwards)
{
    Scenario s("apply-no-relaunch");
    buildFixtureScenario(s);
    StdFs disk;
    Rig rig(s, disk);
    rig.request.relaunch.clear();
    ASSERT_EQ(rig.run().outcome, ApplyOutcome::Committed);
    EXPECT_TRUE(rig.launcher.started.empty());
    EXPECT_EQ(rig.launcher.ran.size(), 1u) << "the health check still runs";
}

TEST(Apply, TheOutcomeIsWrittenToLastJson)
{
    Scenario s("apply-last");
    buildFixtureScenario(s);
    StdFs disk;
    Rig rig(s, disk);
    ASSERT_EQ(rig.run().outcome, ApplyOutcome::Committed);
    const LastResult last = lastOf(s);
    EXPECT_EQ(last.status, "committed");
    EXPECT_EQ(last.version, "0.10.0");
    EXPECT_EQ(last.transaction, rig.tx);
    EXPECT_TRUE(last.holder.empty());
}

TEST(Apply, TheRunOnceEntryIsRegisteredBeforeTheSwapAndClearedAfterwards)
{
    Scenario s("apply-runonce");
    buildFixtureScenario(s);
    StdFs disk;
    Rig rig(s, disk);
    std::map<std::string, std::string> during;
    rig.launcher.onRun = [&](const SpawnRequest& request) -> core::Result<SpawnResult> {
        during = rig.registry.runOnce; // as it stands while the new version is being checked
        return healthyStart(s.state, rig.tx)(request);
    };
    ASSERT_EQ(rig.run().outcome, ApplyOutcome::Committed);
    ASSERT_EQ(during.count(std::string(kRunOnceName)), 1u);
    // The helper copy re-run in recovery mode at the next logon, root quoted when it needs it.
    const std::string root = core::pathToUtf8(s.root);
    const std::string helper = core::pathToUtf8(rig.request.helper);
    EXPECT_EQ(during.at(std::string(kRunOnceName)),
              "\"" + helper + "\" --recover " + detail::quoteArgument(root));
    EXPECT_TRUE(rig.registry.runOnce.empty()) << "cleared when the transaction ended";
}

TEST(Apply, ARootWithASpaceInItIsQuotedInTheRunOnceCommand)
{
    Scenario s("apply-runonce-space");
    // Move the whole scenario under a folder with a space in its name.
    const Path spaced = s.scratch.path() / "Program Files" / "Katana Install";
    buildFixtureScenario(s);
    fs::create_directories(spaced.parent_path());
    fs::rename(s.root, spaced);
    s.root = spaced;
    StdFs disk;
    Rig rig(s, disk);
    rig.launcher.onRun = healthyStart(s.state, rig.tx);
    ASSERT_EQ(rig.run().outcome, ApplyOutcome::Committed);
    ASSERT_EQ(rig.registry.runOnceHistory.size(), 1u);
    EXPECT_EQ(rig.registry.runOnceHistory[0],
              "\"" + core::pathToUtf8(rig.request.helper) + "\" --recover \"" +
                  core::pathToUtf8(spaced) + "\"");
}

TEST(Apply, AnInstallerKindInstallHasItsUninstallEntryRewrittenAnArchiveDoesNot)
{
    {
        Scenario s("apply-registry-installer");
        buildFixtureScenario(s);
        StdFs disk;
        Rig rig(s, disk);
        rig.registry.location = s.root; // HKCU Software\Jarada\Katana equals R
        ASSERT_EQ(rig.run().outcome, ApplyOutcome::Committed);
        ASSERT_EQ(rig.registry.versions.size(), 1u);
        EXPECT_EQ(rig.registry.versions[0].first, "0.10.0");
        EXPECT_EQ(rig.registry.versions[0].second, "Katana 0.10.0");
    }
    {
        Scenario s("apply-registry-archive");
        buildFixtureScenario(s);
        StdFs disk;
        Rig rig(s, disk);
        ASSERT_EQ(rig.run().outcome, ApplyOutcome::Committed);
        EXPECT_TRUE(rig.registry.versions.empty());
    }
}

TEST(Apply, TheFortyFileUpdateKeepsWhatThePersonChangedAndMovesTheRest)
{
    REQUIRE_CAN_SIGN();
    Scenario s("apply-forty");
    buildFortyFileScenario(s);
    ASSERT_EQ(s.v2.files.size(), 40u);
    ASSERT_EQ(s.v1.files.size(), 38u);
    StdFs disk;
    Rig rig(s, disk);
    const ApplyResult result = rig.run();
    ASSERT_EQ(result.outcome, ApplyOutcome::Committed) << result.reason << ": " << result.detail;
    EXPECT_EQ(treeHash(s.root), s.v2Hash) << difference(snapshot(s.expected), snapshot(s.root));

    // The same, one fact at a time, from the specification.
    EXPECT_EQ(readText(s.root / "etc/katana/settings.ini"), "user=edited\n") << "a seed is never rewritten";
    EXPECT_EQ(readText(s.root / "bin/old2.dll"), "the person's own old2\n")
        << "removed from the release but changed by the person: left";
    EXPECT_FALSE(isPresent(s.root / "bin/old1.dll")) << "removed from the release, unchanged: removed";
    EXPECT_FALSE(isPresent(s.root / "share/katana/legacy")) << "and its emptied folder with it";
    EXPECT_EQ(readText(s.root / "share/katana/data/f05.dat"), s.v2.find("share/katana/data/f05.dat")->content)
        << "a file the release lists and the person edited is replaced";
    EXPECT_EQ(readText(s.root / "share/katana/user_notes.txt"), "notes the update never heard of\n");
    EXPECT_EQ(readText(s.root / "bin/user_tool.dll"), "a plug-in nobody listed\n");
    EXPECT_TRUE(fs::is_directory(s.root / "share/katana/more"));
    EXPECT_TRUE(fs::is_directory(s.root / "lib/plugins/extra"));
}

TEST(Apply, ARepeatedUpdateToTheSameReleaseIsRefusedAsNotNewer)
{
    Scenario s("apply-twice");
    buildFixtureScenario(s);
    StdFs disk;
    {
        Rig first(s, disk, "aaaa");
        ASSERT_EQ(first.run().outcome, ApplyOutcome::Committed);
    }
    // The install is v2 now, with a record that says so; the stage is gone, so put it back.
    const std::string installed = treeHash(s.root);
    fs::remove_all(s.stage);
    const Path fixtures = fixtureFolder();
    copyTree(fixtures / "install/v2", s.stage);
    writeText(s.stage / "manifest.json", readText(fixtures / "release/v2/Katana-update-win64.json"));
    writeText(s.stage / "manifest.json.sig", readText(fixtures / "release/v2/Katana-update-win64.json.sig"));
    Rig second(s, disk, "bbbb");
    const ApplyResult result = second.run();
    EXPECT_EQ(result.outcome, ApplyOutcome::Refused);
    EXPECT_EQ(result.reason, "downgrade") << result.detail;
    EXPECT_EQ(treeHash(s.root), installed);
}

// ---- refusals: the signature, the release, the install -----------------------------------------------------------

TEST(ApplyRefusals, ATreeThatIsNotAnInstallIsNeverUpdated)
{
    Scenario s("apply-development");
    buildFixtureScenario(s);
    fs::remove(s.root / "share/katana/install.json"); // the mark of the installer's CPack step
    s.v1Hash = treeHash(s.root);
    s.v1Snapshot = snapshot(s.root);
    StdFs disk;
    Rig rig(s, disk);
    const ApplyResult result = rig.run();
    EXPECT_EQ(result.outcome, ApplyOutcome::Refused);
    EXPECT_EQ(result.reason, "development_tree");
    EXPECT_EQ(treeHash(s.root), s.v1Hash);
    EXPECT_FALSE(isPresent(s.root / ".update")) << "not even the work folder is made";
}

TEST(ApplyRefusals, AMissingStagedManifestOrSignatureIsRefusedAndNothingIsTouched)
{
    {
        Scenario s("apply-no-manifest");
        buildFixtureScenario(s);
        fs::remove(s.stage / "manifest.json");
        StdFs disk;
        Rig rig(s, disk);
        expectRefused(s, rig.run(), "staged_manifest");
    }
    {
        Scenario s("apply-no-signature");
        buildFixtureScenario(s);
        fs::remove(s.stage / "manifest.json.sig");
        StdFs disk;
        Rig rig(s, disk);
        expectRefused(s, rig.run(), "staged_manifest");
    }
}

TEST(ApplyRefusals, TheHelperVerifiesTheStagedManifestWithItsOwnKeysAndRefusesWhatFailsThem)
{
    // Each of the fixtures' bad releases, staged as if the engine had been fooled into
    // staging it. They are the cases the engine's own tests refuse; the helper must refuse
    // them too, because it does not trust the engine's answer.
    for (const std::string bad : {"flipped-manifest-bit", "swapped-sig", "wrong-key",
                                  "wrong-key-claims-active", "path-traversal"}) {
        Scenario s("apply-bad-" + bad);
        buildFixtureScenario(s, "v1", "bad/" + bad);
        StdFs disk;
        Rig rig(s, disk);
        const ApplyResult result = rig.run();
        EXPECT_EQ(result.outcome, ApplyOutcome::Refused) << bad;
        EXPECT_EQ(result.reason, "staged_manifest") << bad << ": " << result.detail;
        expectUntouched(s, result);
        // And the person gets the program back.
        EXPECT_EQ(rig.launcher.started.size(), 1u) << bad;
    }
}

TEST(ApplyRefusals, AManifestSignedByAKeyThisBuildDoesNotTrustIsRefused)
{
    Scenario s("apply-untrusted");
    buildFixtureScenario(s);
    // The real production keys are placeholders in a development build; the test trust is
    // the one in the services. Swap in a trust that holds only the spare key: the active key
    // signed this release, so it no longer verifies.
    Trust onlySpare;
    onlySpare.keys.push_back(testTrust().keys[1]);
    StdFs disk;
    Rig rig(s, disk);
    ApplyServices services = rig.services();
    const ApplyServices narrowed{services.fs, services.processes, services.launcher,
                                 services.registry, onlySpare, services.timings};
    const ApplyResult result = apply(rig.request, narrowed, {});
    expectRefused(s, result, "staged_manifest");
}

TEST(ApplyRefusals, AReleaseThatIsNotNewerThanTheInstallIsRefusedOnVersionAndOnSequence)
{
    for (const std::string bad : {"downgrade-version", "downgrade-sequence"}) {
        Scenario s("apply-" + bad);
        buildFixtureScenario(s, "v2", "bad/" + bad); // v2 is installed
        StdFs disk;
        Rig rig(s, disk);
        const ApplyResult result = rig.run();
        EXPECT_EQ(result.outcome, ApplyOutcome::Refused) << bad;
        EXPECT_EQ(result.reason, "downgrade") << bad << ": " << result.detail;
        expectUntouched(s, result);
    }
}

TEST(ApplyRefusals, AManifestForAnotherTargetIsRefused)
{
    REQUIRE_CAN_SIGN();
    Scenario s("apply-other-target");
    buildSmallScenario(s);
    ReleaseSpec other = smallRelease("0.2.0", 200);
    other.target = "linux-x86_64";
    writeStage(s.stage, other);
    StdFs disk;
    Rig rig(s, disk);
    const ApplyResult result = rig.run();
    // verifyManifest (trust.cpp) is what asks "is this signed for this install's target", with
    // the helper's own code, so that is the token and that is the sentence.
    EXPECT_EQ(result.outcome, ApplyOutcome::Refused);
    EXPECT_EQ(result.reason, "staged_manifest");
    EXPECT_NE(result.detail.find("another target"), std::string::npos) << result.detail;
    expectUntouched(s, result);
}

TEST(ApplyRefusals, AnAnnouncementOnlyManifestIsNotAnUpdate)
{
    REQUIRE_CAN_SIGN();
    Scenario s("apply-notify-only");
    buildSmallScenario(s);
    const std::string json =
        "{\"applier\":1,\"channel\":\"stable\",\"expires\":\"2030-01-01T00:00:00Z\",\"files\":[],"
        "\"notes_url\":\"https://github.com/kjarada/Katana/releases/tag/v0.2.0\",\"notify_only\":true,"
        "\"product\":\"katana\",\"published\":\"2026-10-01T00:00:00Z\",\"schema\":1,\"sequence\":200,"
        "\"tag\":\"v0.2.0\",\"target\":\"win64\",\"version\":\"0.2.0\"}\n";
    writeText(s.stage / "manifest.json", json);
    writeText(s.stage / "manifest.json.sig", signLine(json));
    StdFs disk;
    Rig rig(s, disk);
    expectRefused(s, rig.run(), "notify_only");
}

TEST(ApplyRefusals, AManifestThatNeedsANewerHelperIsRefused)
{
    REQUIRE_CAN_SIGN();
    Scenario s("apply-applier");
    buildSmallScenario(s);
    std::string json = manifestJson(s.v2);
    const std::size_t at = json.find("\"applier\":1");
    ASSERT_NE(at, std::string::npos);
    json.replace(at, 11, "\"applier\":2");
    writeText(s.stage / "manifest.json", json);
    writeText(s.stage / "manifest.json.sig", signLine(json));
    StdFs disk;
    Rig rig(s, disk);
    expectRefused(s, rig.run(), "applier_too_new");
}

TEST(ApplyRefusals, AnInvalidTransactionIdOrNoRootIsRefusedBeforeAnythingIsMade)
{
    Scenario s("apply-bad-request");
    buildSmallScenario(s);
    StdFs disk;
    {
        Rig rig(s, disk);
        rig.request.transaction = "../../evil";
        const ApplyResult result = rig.run();
        EXPECT_EQ(result.outcome, ApplyOutcome::Refused);
        EXPECT_EQ(result.reason, "invalid_request");
        EXPECT_FALSE(isPresent(s.root / ".update"));
        EXPECT_FALSE(isPresent(s.scratch.path() / "evil"));
    }
    {
        Rig rig(s, disk);
        rig.request.root.clear();
        EXPECT_EQ(rig.run().reason, "invalid_request");
    }
}

TEST(ApplyRefusals, AnEmptyStateFolderIsRefusedBecauseTheHealthMarkerCouldNotBeLookedFor)
{
    Scenario s("apply-no-state-folder");
    buildFixtureScenario(s);
    StdFs disk;
    Rig rig(s, disk);
    rig.request.stateDirectory.clear();
    const ApplyResult result = rig.run();
    EXPECT_EQ(result.outcome, ApplyOutcome::Refused);
    EXPECT_EQ(result.reason, "invalid_request");
    EXPECT_FALSE(isPresent(s.root / ".update")) << "refused before anything is made";
    EXPECT_EQ(treeHash(s.root), s.v1Hash);
    EXPECT_TRUE(rig.launcher.ran.empty()) << "no health check without a place for its marker";
}

TEST(ApplyRefusals, ARefusedUpdateStartsTheOldVersionAgainAndSaysSoInLastJson)
{
    Scenario s("apply-refused-relaunch");
    buildFixtureScenario(s);
    fs::remove(s.stage / "manifest.json");
    StdFs disk;
    Rig rig(s, disk);
    ASSERT_EQ(rig.run().outcome, ApplyOutcome::Refused);
    ASSERT_EQ(rig.launcher.started.size(), 1u);
    EXPECT_EQ(rig.launcher.started[0].executable, rig.request.relaunch);
    EXPECT_TRUE(rig.launcher.ran.empty()) << "no health check: nothing was installed";
    const LastResult last = lastOf(s);
    EXPECT_EQ(last.status, "refused");
    EXPECT_EQ(last.reason, "staged_manifest");
}

// ---- planning: what must be in the stage -------------------------------------------------------------------------

TEST(ApplyPlan, AStagedFileThatIsMissingRefusesTheUpdateAndLeavesNothingBehind)
{
    Scenario s("apply-staged-missing");
    buildFixtureScenario(s);
    fs::remove(s.stage / "bin/libnew-2.dll");
    StdFs disk;
    Rig rig(s, disk);
    expectRefused(s, rig.run(), "staged_missing");
}

TEST(ApplyPlan, AStagedFileWithTheWrongBytesOrTheWrongSizeIsRefused)
{
    {
        Scenario s("apply-staged-hash");
        buildFixtureScenario(s);
        std::string bytes = readText(s.stage / "bin/libnew-2.dll");
        bytes[3] = static_cast<char>(bytes[3] ^ 0x01); // one bit
        writeText(s.stage / "bin/libnew-2.dll", bytes);
        StdFs disk;
        Rig rig(s, disk);
        expectRefused(s, rig.run(), "staged_hash");
    }
    {
        Scenario s("apply-staged-size");
        buildFixtureScenario(s);
        writeText(s.stage / "bin/libnew-2.dll", readText(s.stage / "bin/libnew-2.dll") + "x");
        StdFs disk;
        Rig rig(s, disk);
        expectRefused(s, rig.run(), "staged_hash");
    }
}

TEST(ApplyPlan, TheHashIsOfTheCopyNotOfWhatTheStageHoldsOnceTheCopyIsMade)
{
    // The stage folder is writable by anything running as the user. A file that is wrong only
    // while it is being copied, and right again before anyone looks, must not be installed:
    // hashing the stage file after the copy would see the right bytes and pass the wrong copy.
    // The copy is what is hashed. Here the stage file holds other bytes of the same length for
    // the length of the copy, and its own again after.
    Scenario s("apply-staged-race");
    buildFixtureScenario(s);
    StdFs disk;
    HookFs hooked(disk);
    std::string original;
    hooked.beforeCopy = [&](const Path& from, const Path&) -> std::optional<core::Status> {
        if (from.filename() == "libnew-2.dll") {
            original = readText(from);
            std::string swapped = original;
            std::reverse(swapped.begin(), swapped.end()); // same length, other bytes
            writeText(from, swapped);
        }
        return std::nullopt;
    };
    hooked.afterCopy = [&](const Path& from, const Path&) {
        if (from.filename() == "libnew-2.dll") {
            writeText(from, original);
        }
    };
    Rig rig(s, hooked);
    expectRefused(s, rig.run(), "staged_hash");
}

TEST(ApplyPlan, TheInstallIsNotTouchedWhileThePlanCopiesAndHashes)
{
    // Whatever refusal ends the plan, R is as it was: the same entries, bytes and folders.
    Scenario s("apply-plan-readonly");
    buildFixtureScenario(s);
    fs::remove(s.stage / "share/katana/news.txt"); // the last file the plan reaches fails
    StdFs disk;
    Rig rig(s, disk);
    expectRefused(s, rig.run(), "staged_missing");
    EXPECT_EQ(snapshot(s.root), s.v1Snapshot);
}

TEST(ApplyPlan, ADirectoryWhereAFileMustGoOrAFileWhereADirectoryMustGoIsRefused)
{
    {
        Scenario s("apply-conflict-dir");
        buildFixtureScenario(s);
        fs::remove(s.root / "bin/libcore-1.dll");
        fs::create_directories(s.root / "bin/libcore-1.dll"); // a folder in the file's place
        s.v1Hash = treeHash(s.root);
        s.v1Snapshot = snapshot(s.root);
        // libcore is unchanged in v2, but the folder is not the file the manifest names.
        StdFs disk;
        Rig rig(s, disk);
        expectRefused(s, rig.run(), "path_conflict");
    }
    {
        Scenario s("apply-conflict-file");
        buildFixtureScenario(s);
        writeText(s.root / "share/katana/help", "a file where the new folder must be");
        s.v1Hash = treeHash(s.root);
        s.v1Snapshot = snapshot(s.root);
        StdFs disk;
        Rig rig(s, disk);
        expectRefused(s, rig.run(), "path_conflict");
    }
}

TEST(ApplyPlan, AFolderThatIsALinkOrResolvesOutsideTheInstallIsRefused)
{
    {
        Scenario s("apply-reparse-parent");
        buildFixtureScenario(s);
        StdFs disk;
        HookFs hooked(disk);
        hooked.adjustStat = [&](const Path& p, EntryInfo& info) {
            if (p == s.root / "share/katana") {
                info.reparsePoint = true;
            }
        };
        Rig rig(s, hooked);
        expectRefused(s, rig.run(), "reparse_point");
    }
    {
        Scenario s("apply-resolves-outside");
        buildFixtureScenario(s);
        StdFs disk;
        HookFs hooked(disk);
        hooked.overrideResolve = [&](const Path& p) -> std::optional<Path> {
            if (p == s.root / "bin") {
                return s.scratch.path() / "somewhere" / "else"; // what a junction would give
            }
            return std::nullopt;
        };
        Rig rig(s, hooked);
        expectRefused(s, rig.run(), "reparse_point");
    }
    {
        Scenario s("apply-reparse-file");
        buildFixtureScenario(s);
        StdFs disk;
        HookFs hooked(disk);
        hooked.adjustStat = [&](const Path& p, EntryInfo& info) {
            if (p == s.root / "bin/katana.exe") {
                info.reparsePoint = true;
            }
        };
        Rig rig(s, hooked);
        expectRefused(s, rig.run(), "reparse_point");
    }
    {
        Scenario s("apply-reparse-update-folder");
        buildFixtureScenario(s);
        StdFs disk;
        HookFs hooked(disk);
        hooked.adjustStat = [&](const Path& p, EntryInfo& info) {
            if (p == s.root / ".update") {
                info.reparsePoint = true;
            }
        };
        Rig rig(s, hooked);
        const ApplyResult result = rig.run();
        EXPECT_EQ(result.outcome, ApplyOutcome::Refused);
        EXPECT_EQ(result.reason, "reparse_point");
        EXPECT_EQ(treeHash(s.root), s.v1Hash);
    }
}

TEST(ApplyPlan, NotEnoughFreeSpaceRefusesBeforeAnythingIsCopied)
{
    Scenario s("apply-space");
    buildFixtureScenario(s);
    StdFs disk;
    LogFs log(disk);
    HookFs hooked(log);
    hooked.freeBytes = 100; // the copies need more than a megabyte of margin alone
    Rig rig(s, hooked);
    expectRefused(s, rig.run(), "space");
    // No copy was started, and nothing was made in a transaction folder.
    EXPECT_EQ(log.find("copyFile"), 0u);
    EXPECT_EQ(log.find("renameFile"), 0u);
    EXPECT_EQ(log.find("writeDurable"), 0u);
    EXPECT_EQ(log.find("makeDirectories", "new"), 0u);
}

TEST(ApplyPlan, ATransactionFolderThatIsAlreadyThereIsRefusedAndNeverDeleted)
{
    // The same id again would be a folder some other run made - maybe a half swap waiting for
    // its recovery. It is not this run's to touch.
    Scenario s("apply-tx-exists");
    buildFixtureScenario(s);
    writeText(s.root / ".update" / "a1b2c3d4" / "old/bin/katana.exe", "somebody's old file");
    writeText(s.root / ".update" / "a1b2c3d4" / "journal", "");
    StdFs disk;
    Rig rig(s, disk);
    const ApplyResult result = rig.run();
    EXPECT_EQ(result.outcome, ApplyOutcome::Refused);
    EXPECT_EQ(result.reason, "transaction_exists");
    EXPECT_EQ(treeHash(s.root), s.v1Hash);
    EXPECT_EQ(readText(s.root / ".update" / "a1b2c3d4" / "old/bin/katana.exe"), "somebody's old file");
}

TEST(ApplyPlan, AFullDiskDuringACopyRefusesTheUpdateAndRemovesWhatWasCopied)
{
    Scenario s("apply-disk-full");
    buildFixtureScenario(s);
    StdFs disk;
    HookFs hooked(disk);
    int copies = 0;
    hooked.beforeCopy = [&](const Path&, const Path& to) -> std::optional<core::Status> {
        if (++copies == 3) {
            return injected(112, to); // ERROR_DISK_FULL
        }
        return std::nullopt;
    };
    Rig rig(s, hooked);
    const ApplyResult result = rig.run();
    expectRefused(s, result, "copy_failed");
    EXPECT_NE(result.detail.find("112"), std::string::npos) << result.detail;
}

TEST(ApplyPlan, AJournalThatCannotBeWrittenRefusesTheUpdateBeforeAnythingMoves)
{
    Scenario s("apply-journal-plan");
    buildFixtureScenario(s);
    StdFs disk;
    HookFs hooked(disk);
    hooked.beforeAppend = [&](const Path& p, std::string_view) -> std::optional<core::Status> {
        if (p.filename() == "journal") {
            return injected(112, p);
        }
        return std::nullopt;
    };
    Rig rig(s, hooked);
    expectRefused(s, rig.run(), "journal");
}

TEST(ApplyPlan, ACancelBeforeTheCommitTouchesNothing)
{
    {
        Scenario s("apply-cancel-early");
        buildFixtureScenario(s);
        StdFs disk;
        Rig rig(s, disk);
        std::stop_source stop;
        stop.request_stop();
        expectRefused(s, rig.run(stop.get_token()), "cancelled");
    }
    {
        // Asked for in the middle of the plan: after the third file has been examined.
        Scenario s("apply-cancel-mid");
        buildFixtureScenario(s);
        StdFs disk;
        HookFs hooked(disk);
        std::stop_source stop;
        int examined = 0;
        hooked.adjustStat = [&](const Path& p, EntryInfo&) {
            if (p.parent_path() == s.root / "bin" && ++examined == 3) {
                stop.request_stop();
            }
        };
        Rig rig(s, hooked);
        expectRefused(s, rig.run(stop.get_token()), "cancelled");
    }
}

TEST(ApplyPlan, AStopRequestedOnceTheCommitHasBegunIsIgnoredAndTheSwapIsFinished)
{
    Scenario s("apply-cancel-late");
    buildFixtureScenario(s);
    StdFs disk;
    HookFs hooked(disk);
    std::stop_source stop;
    hooked.beforeRename = [&](const Path&, const Path&) -> std::optional<core::Status> {
        stop.request_stop(); // at the first rename of the commit
        return std::nullopt;
    };
    Rig rig(s, hooked);
    const ApplyResult result = rig.run(stop.get_token());
    EXPECT_EQ(result.outcome, ApplyOutcome::Committed) << "a half swap is finished or rolled back, never abandoned";
    EXPECT_EQ(treeHash(s.root), s.v2Hash);
}

// ---- the lock ------------------------------------------------------------------------------------------------------

TEST(ApplyLock, AHeldLockRefusesTheUpdateAndLeavesTheOtherHelpersOutcomeAlone)
{
    Scenario s("apply-locked");
    buildFixtureScenario(s);
    StdFs disk;
    fs::create_directories(s.root / ".update");
    core::Result<std::unique_ptr<FileLock>> other = disk.tryLock(s.root / ".update" / "lock");
    ASSERT_TRUE(other.ok());

    Rig rig(s, disk);
    const ApplyResult result = rig.run();
    EXPECT_EQ(result.outcome, ApplyOutcome::Refused);
    EXPECT_EQ(result.reason, "locked");
    EXPECT_EQ(treeHash(s.root), s.v1Hash);
    EXPECT_TRUE(rig.launcher.started.empty()) << "the other helper starts the program";
    EXPECT_TRUE(rig.launcher.ran.empty());
    const core::Result<std::optional<LastResult>> last = readLastResult(s.state);
    ASSERT_TRUE(last.ok());
    EXPECT_FALSE(last->has_value()) << "it is the other helper's to write";
    EXPECT_TRUE(rig.registry.runOnce.empty());
}

TEST(ApplyLock, TwoHelpersAtOnceOneUpdatesAndTheOtherIsRefused)
{
    Scenario s("apply-contend");
    buildFixtureScenario(s);
    StdFs disk;

    Rig first(s, disk, "aaaa");
    first.processes.holders = {RunningProgram{4412, core::pathFromUtf8("katana_mcp.exe")}};
    first.processes.holdersFor = -1; // keeps the first helper waiting, lock held, until released
    first.timings.waitForHolders = std::chrono::seconds(60);
    first.timings.pollInterval = std::chrono::milliseconds(2);
    ApplyResult firstResult;
    std::thread running([&] { firstResult = first.run(); });

    // Once the first is scanning for holders it holds the lock.
    for (int i = 0; i < 5000 && first.processes.scans.load() == 0; ++i) {
        std::this_thread::sleep_for(std::chrono::milliseconds(1));
    }
    ASSERT_GT(first.processes.scans.load(), 0);
    Rig second(s, disk, "bbbb");
    const ApplyResult secondResult = second.run();
    first.processes.release = true; // only now does the first helper go on
    running.join();

    EXPECT_EQ(secondResult.outcome, ApplyOutcome::Refused);
    EXPECT_EQ(secondResult.reason, "locked");
    EXPECT_EQ(firstResult.outcome, ApplyOutcome::Committed) << firstResult.reason << firstResult.detail;
    EXPECT_EQ(treeHash(s.root), s.v2Hash);
}

TEST(ApplyLock, TheLockIsFreeAgainAfterTheUpdate)
{
    Scenario s("apply-lock-free");
    buildFixtureScenario(s);
    StdFs disk;
    Rig rig(s, disk);
    ASSERT_EQ(rig.run().outcome, ApplyOutcome::Committed);
    EXPECT_TRUE(disk.tryLock(s.root / ".update" / "lock").ok())
        << "released before the relaunch, whose first act is recover()";
}

TEST(ApplyLock, ARootThatCannotBeWrittenIsReportedAsSuch)
{
    Scenario s("apply-unwritable");
    buildFixtureScenario(s);
    StdFs disk;
    class Unwritable final : public ForwardingFs {
      public:
        using ForwardingFs::ForwardingFs;
        core::Status makeDirectories(const Path& p) override
        {
            return osError(core::ErrorCode::FileExportFailure, "access denied", p, 5);
        }
    } refusing(disk);
    Rig rig(s, refusing);
    const ApplyResult result = rig.run();
    EXPECT_EQ(result.outcome, ApplyOutcome::Refused);
    EXPECT_EQ(result.reason, "root_not_writable") << result.detail;
    EXPECT_EQ(treeHash(s.root), s.v1Hash);
}

namespace {

// Answers "held" to the first `heldFor` attempts to take the lock (always, when it is -1): what a
// virus scanner or an indexer that has the lock file open for a moment looks like to a program
// that asks for it with share mode 0.
class BusyLockFs final : public ForwardingFs {
  public:
    BusyLockFs(FsOps& inner, int heldFor) : ForwardingFs(inner), heldFor_(heldFor) {}

    core::Result<std::unique_ptr<FileLock>> tryLock(const Path& p) override
    {
        ++attempts;
        if (heldFor_ < 0 || attempts <= heldFor_) {
            return core::makeError(core::ErrorCode::InvalidState, "held",
                                   "os_error=32 " + core::pathToUtf8(p));
        }
        return inner_.tryLock(p);
    }

    int attempts = 0;

  private:
    int heldFor_;
};

} // namespace

TEST(ApplyLock, ALockThatIsHeldOnlyForAMomentIsWaitedForNotMistakenForAnotherHelper)
{
    Scenario s("apply-lock-blip");
    buildFixtureScenario(s);
    StdFs disk;
    BusyLockFs busy(disk, 3);
    Rig rig(s, busy);
    rig.timings.waitForHolders = std::chrono::seconds(5); // the grace is the shorter of this and 2 s
    const ApplyResult result = rig.run();
    EXPECT_EQ(result.outcome, ApplyOutcome::Committed) << result.reason << ": " << result.detail;
    EXPECT_EQ(busy.attempts, 4) << "three refusals, then the lock";
    EXPECT_EQ(treeHash(s.root), s.v2Hash);
}

TEST(ApplyLock, ALockThatStaysHeldIsLookedAtAgainAndThenLeftToItsHolderInSilence)
{
    Scenario s("apply-lock-stays");
    buildFixtureScenario(s);
    StdFs disk;
    BusyLockFs busy(disk, -1);
    Rig rig(s, busy);
    const ApplyResult result = rig.run();
    EXPECT_EQ(result.outcome, ApplyOutcome::Refused);
    EXPECT_EQ(result.reason, "locked");
    EXPECT_GT(busy.attempts, 1) << "it looked again before it believed the lock was held";
    EXPECT_EQ(treeHash(s.root), s.v1Hash);
    EXPECT_TRUE(rig.launcher.started.empty()) << "the holder starts the program";
    const core::Result<std::optional<LastResult>> last = readLastResult(s.state);
    ASSERT_TRUE(last.ok());
    EXPECT_FALSE(last->has_value()) << "it is the other helper's to write";
}

// ---- waiting for the window and for everyone under the root -----------------------------------------------------------

TEST(ApplyWait, ProgramsStillRunningUnderTheRootBlockTheUpdateAndNothingIsTouched)
{
    Scenario s("apply-blocked");
    buildFixtureScenario(s);
    StdFs disk;
    Rig rig(s, disk);
    rig.processes.holders = {RunningProgram{4412, core::pathFromUtf8("C:/k/bin/katana_mcp.exe")}};
    rig.processes.holdersFor = -1; // never goes away

    const ApplyResult result = rig.run();
    EXPECT_EQ(result.outcome, ApplyOutcome::Blocked);
    EXPECT_EQ(result.reason, "blocked");
    ASSERT_EQ(result.holders.size(), 1u);
    EXPECT_EQ(result.holders[0].pid, 4412u);
    EXPECT_EQ(result.detail, "Close these first: katana_mcp.exe (4412). Then press Retry.");
    expectUntouched(s, result);
    EXPECT_GT(rig.processes.scans.load(), 1) << "it waited, polling, before it gave up";

    // The old version is started again, nothing else is run, and the outcome is recorded.
    ASSERT_EQ(rig.launcher.started.size(), 1u);
    EXPECT_EQ(rig.launcher.started[0].executable, rig.request.relaunch);
    EXPECT_TRUE(rig.launcher.ran.empty());
    const LastResult last = lastOf(s);
    EXPECT_EQ(last.status, "blocked");
    EXPECT_EQ(last.holder, "katana_mcp.exe (4412)");
}

TEST(ApplyWait, EveryHolderIsNamedAndNoneIsKilled)
{
    Scenario s("apply-blocked-many");
    buildFixtureScenario(s);
    StdFs disk;
    Rig rig(s, disk);
    rig.processes.holders = {RunningProgram{10, core::pathFromUtf8("C:/k/bin/katana_cli.exe")},
                             RunningProgram{20, core::pathFromUtf8("C:/k/bin/katana_mcp.exe")}};
    rig.processes.holdersFor = -1;
    const ApplyResult result = rig.run();
    ASSERT_EQ(result.outcome, ApplyOutcome::Blocked);
    EXPECT_EQ(result.detail, "Close these first: katana_cli.exe (10), katana_mcp.exe (20). Then press Retry.");
    EXPECT_EQ(lastOf(s).holder, "katana_cli.exe (10), katana_mcp.exe (20)");
    // The seam has no way to kill: ProcessOps is "who is running" and nothing more.
}

TEST(ApplyWait, AProgramThatExitsWhileTheUpdateWaitsLetsItGoAhead)
{
    Scenario s("apply-holder-leaves");
    buildFixtureScenario(s);
    StdFs disk;
    Rig rig(s, disk);
    rig.processes.holders = {RunningProgram{4412, core::pathFromUtf8("C:/k/bin/katana_mcp.exe")}};
    rig.processes.holdersFor = 5;
    rig.timings.waitForHolders = std::chrono::milliseconds(5000);
    const ApplyResult result = rig.run();
    EXPECT_EQ(result.outcome, ApplyOutcome::Committed) << result.reason << result.detail;
    // Five answers that name the program, a sixth that names nobody and ends the wait - and one
    // more: the same question is asked again when the plan is made, before the journal says
    // COMMITTING (apply.hpp, step 4; the cases of that second look are further down).
    EXPECT_EQ(rig.processes.scans.load(), 7);
    EXPECT_EQ(treeHash(s.root), s.v2Hash);
}

TEST(ApplyWait, TheHelpersOwnProcessIsNotAHolder)
{
    Scenario s("apply-self");
    buildFixtureScenario(s);
    StdFs disk;
    Rig rig(s, disk);
    rig.processes.pid = 999;
    rig.processes.holders = {RunningProgram{999, core::pathFromUtf8("C:/k/bin/katana_updater.exe")}};
    rig.processes.holdersFor = -1;
    EXPECT_EQ(rig.run().outcome, ApplyOutcome::Committed);
}

TEST(ApplyWait, AWindowThatDoesNotExitBlocksTheUpdate)
{
    Scenario s("apply-parent-stays");
    buildFixtureScenario(s);
    StdFs disk;
    Rig rig(s, disk);
    rig.request.waitPid = 77;
    rig.processes.parentAliveFor = -1;
    const ApplyResult result = rig.run();
    EXPECT_EQ(result.outcome, ApplyOutcome::Blocked);
    ASSERT_EQ(result.holders.size(), 1u);
    EXPECT_EQ(result.holders[0].pid, 77u);
    expectUntouched(s, result);
    EXPECT_EQ(rig.processes.scans.load(), 0) << "the files are not even looked at before the window is gone";
}

TEST(ApplyWait, AWindowThatIsStillRunningIsNotStartedASecondTimeBesideItself)
{
    Scenario s("apply-parent-no-second-window");
    buildFixtureScenario(s);
    StdFs disk;
    Rig rig(s, disk);
    rig.request.waitPid = 77;
    rig.processes.parentAliveFor = -1; // hung: it never exits
    const ApplyResult result = rig.run();
    ASSERT_EQ(result.outcome, ApplyOutcome::Blocked);
    EXPECT_TRUE(rig.launcher.started.empty()) << "a second Katana would open beside the hung one";
    EXPECT_TRUE(rig.launcher.ran.empty());
    EXPECT_EQ(lastOf(s).status, "blocked") << "and the outcome is still recorded";
    expectUntouched(s, result);
}

TEST(ApplyWait, AWindowThatExitsAfterAWhileIsWaitedFor)
{
    Scenario s("apply-parent-exits");
    buildFixtureScenario(s);
    StdFs disk;
    Rig rig(s, disk);
    rig.request.waitPid = 77;
    rig.processes.parentAliveFor = 10;
    rig.timings.waitForParent = std::chrono::milliseconds(5000);
    EXPECT_EQ(rig.run().outcome, ApplyOutcome::Committed);
}

TEST(ApplyWait, AScanThatFailsIsARefusalNotAGuess)
{
    Scenario s("apply-scan-fails");
    buildFixtureScenario(s);
    StdFs disk;
    Rig rig(s, disk);
    rig.processes.scanFails = true;
    expectRefused(s, rig.run(), "scan_failed");
}

TEST(ApplyWait, ACancelWhileWaitingEndsTheWaitWithoutTouchingAnything)
{
    Scenario s("apply-cancel-waiting");
    buildFixtureScenario(s);
    StdFs disk;
    Rig rig(s, disk);
    rig.processes.holders = {RunningProgram{4412, core::pathFromUtf8("C:/k/bin/katana_mcp.exe")}};
    rig.processes.holdersFor = -1;
    rig.timings.waitForHolders = std::chrono::milliseconds(60000);
    std::stop_source stop;
    std::thread canceller([&] {
        std::this_thread::sleep_for(std::chrono::milliseconds(30));
        stop.request_stop();
    });
    const auto start = std::chrono::steady_clock::now();
    const ApplyResult result = rig.run(stop.get_token());
    canceller.join();
    EXPECT_LT(std::chrono::steady_clock::now() - start, std::chrono::seconds(10));
    expectRefused(s, result, "cancelled");
}

// ---- step 2's second look: a program started while the plan was made --------------------------------------------------
//
// The first look is over before the plan, and the plan hashes and copies every changed file. A
// program of the install started in that time ran old code over the new tree, unseen. The rule
// now (apply.hpp, step 4): the scan is asked once more, with the PLAN on disk and immediately
// before the journal goes to COMMITTING, and a holder found then is the Blocked of the first
// look - the install untouched and the download kept. The expectations below are that rule's:
// the outcome and its sentence are the ones ProgramsStillRunningUnderTheRoot... holds the first
// look to, and "untouched" and "kept" are facts about the disk.

namespace {

// One run of the swap with a process seam of the test's own in the place of the rig's: the
// harness's FakeProcesses cannot say "nobody, and then somebody" (scripted_processes.hpp).
ApplyResult runWith(Rig& rig, ProcessOps& processes)
{
    const ApplyServices services{*rig.fs, processes, rig.launcher, rig.registry, testTrust(), rig.timings};
    return apply(rig.request, services, std::stop_token{});
}

} // namespace

TEST(ApplyWait, AProgramStartedWhileThePlanWasMadeBlocksTheUpdateAndTheInstallAndTheDownloadAreAsBefore)
{
    Scenario s("apply-late-holder");
    buildFixtureScenario(s);
    const std::map<std::string, std::string> stageBefore = snapshot(s.stage);
    StdFs disk;
    Rig rig(s, disk);
    // Nobody at the first look. At the second, katana_cli: started while the files were hashed
    // and copied.
    ScriptedProcesses processes({nobody(), somebody(4412, "C:/k/bin/katana_cli.exe")});

    const ApplyResult result = runWith(rig, processes);
    EXPECT_EQ(result.outcome, ApplyOutcome::Blocked);
    EXPECT_EQ(result.reason, "blocked");
    ASSERT_EQ(result.holders.size(), 1u);
    EXPECT_EQ(result.holders[0].pid, 4412u);
    EXPECT_EQ(result.detail, "Close these first: katana_cli.exe (4412). Then press Retry.");
    EXPECT_EQ(processes.scans, 2) << "the second look is one question, not another wait";

    // The install is as it was: not a file of it moved, and no folder of the transaction left.
    expectUntouched(s, result);
    // The download is kept: every staged file is where it was, with the bytes it had.
    EXPECT_EQ(difference(stageBefore, snapshot(s.stage)), "") << "the stage was changed";
    // The journal never said COMMITTING, so no RunOnce entry was ever registered.
    EXPECT_TRUE(rig.registry.runOnceHistory.empty());
    // As after the first look: the old version is started again, nothing is run for its health,
    // and the outcome is recorded for the window to show.
    ASSERT_EQ(rig.launcher.started.size(), 1u);
    EXPECT_EQ(rig.launcher.started[0].executable, rig.request.relaunch);
    EXPECT_TRUE(rig.launcher.ran.empty());
    const LastResult last = lastOf(s);
    EXPECT_EQ(last.status, "blocked");
    EXPECT_EQ(last.holder, "katana_cli.exe (4412)");

    // "Kept" in the sense that matters: with the program closed, the Retry installs from that
    // same stage, with nothing downloaded again.
    Rig retry(s, disk, "b2b2b2b2");
    const ApplyResult again = retry.run();
    EXPECT_EQ(again.outcome, ApplyOutcome::Committed) << again.reason << ": " << again.detail;
    EXPECT_EQ(treeHash(s.root), s.v2Hash) << difference(snapshot(s.expected), snapshot(s.root));
}

TEST(ApplyWait, TheSecondLookIsTheLastThingBeforeTheJournalSaysCommitting)
{
    Scenario s("apply-late-look-order");
    buildFixtureScenario(s);
    StdFs disk;
    LogFs log(disk);
    Rig rig(s, log);
    ScriptedProcesses processes; // nobody, whenever it is asked
    std::size_t changesBefore = 0; // the changes to the disk made before the second look
    JournalState journalAtTheLook = JournalState::Empty;
    std::string treeAtTheLook;
    processes.onScan = [&](int asked) {
        if (asked == 2) {
            changesBefore = log.log.size();
            journalAtTheLook = journalStateOf(s.root);
            treeAtTheLook = treeHash(s.root);
        }
    };

    const ApplyResult result = runWith(rig, processes);
    ASSERT_EQ(result.outcome, ApplyOutcome::Committed) << result.reason << ": " << result.detail;
    EXPECT_EQ(processes.scans, 2) << "asked once before the plan and once before the commit";
    // When it was asked the second time the PLAN group was on disk and nothing had moved ...
    EXPECT_EQ(journalAtTheLook, JournalState::Planned);
    EXPECT_EQ(treeAtTheLook, s.v1Hash);
    // ... the PLAN group was the last change before it, and the COMMITTING record the first one
    // after it: nothing is done between the answer and the commit, in which a program could be
    // started and not seen.
    ASSERT_GT(changesBefore, 0u);
    ASSERT_LT(changesBefore, log.log.size());
    EXPECT_EQ(log.find("appendDurable", "PLAN"), changesBefore);
    EXPECT_EQ(log.find("appendDurable", "COMMITTING"), changesBefore + 1) << log.log[changesBefore];
    EXPECT_EQ(treeHash(s.root), s.v2Hash);
}

TEST(ApplyWait, AScanThatFailsAtTheSecondLookIsARefusalTooAndNothingIsTouched)
{
    Scenario s("apply-late-scan-fails");
    buildFixtureScenario(s);
    const std::map<std::string, std::string> stageBefore = snapshot(s.stage);
    StdFs disk;
    Rig rig(s, disk);
    // "Nobody" the first time; the second time the scan gives no answer, which is not "nobody".
    ScriptedProcesses processes({nobody(), noAnswer()});
    const ApplyResult result = runWith(rig, processes);
    expectRefused(s, result, "scan_failed");
    EXPECT_EQ(processes.scans, 2);
    EXPECT_EQ(difference(stageBefore, snapshot(s.stage)), "") << "the stage was changed";
    EXPECT_TRUE(rig.registry.runOnceHistory.empty());
}

// ---- removals and seeds, the rules ---------------------------------------------------------------------------------------

TEST(ApplyRemovals, AFirstUpdateWithNoInstalledManifestDeletesNothing)
{
    Scenario s("apply-first-update");
    buildFixtureScenario(s);
    fs::remove(s.root / "share/katana/installed.manifest.json");
    fs::remove(s.root / "share/katana/installed.manifest.json.sig");
    StdFs disk;
    Rig rig(s, disk);
    ASSERT_EQ(rig.run().outcome, ApplyOutcome::Committed);
    // v1's own files that v2 dropped are not listed in any record, so they stay.
    EXPECT_TRUE(isPresent(s.root / "bin/libold-1.dll"));
    EXPECT_TRUE(isPresent(s.root / "share/katana/legacy/old.txt"));
    // And the records are written, the first time.
    EXPECT_TRUE(isPresent(s.root / "share/katana/installed.manifest.json"));
    EXPECT_TRUE(isPresent(s.root / "share/katana/installed.manifest.json.sig"));
}

TEST(ApplyRemovals, AnInstalledManifestThatDoesNotVerifyDecidesNothing)
{
    Scenario s("apply-tampered-record");
    buildFixtureScenario(s);
    std::string json = readText(s.root / "share/katana/installed.manifest.json");
    json[json.find("libold") + 3] = 'X'; // a listed name, changed: the signature no longer fits
    writeText(s.root / "share/katana/installed.manifest.json", json);
    StdFs disk;
    Rig rig(s, disk);
    ASSERT_EQ(rig.run().outcome, ApplyOutcome::Committed);
    EXPECT_TRUE(isPresent(s.root / "bin/libold-1.dll")) << "no record can be trusted, so nothing is removed";
}

TEST(ApplyRemovals, AFileTheOldManifestListsAndThePersonChangedIsNotRemoved)
{
    Scenario s("apply-edited-removal");
    buildFixtureScenario(s);
    writeText(s.root / "share/katana/legacy/old.txt", "edited by the person");
    StdFs disk;
    Rig rig(s, disk);
    ASSERT_EQ(rig.run().outcome, ApplyOutcome::Committed);
    EXPECT_EQ(readText(s.root / "share/katana/legacy/old.txt"), "edited by the person");
    EXPECT_FALSE(isPresent(s.root / "bin/libold-1.dll")) << "the unchanged one still goes";
}

TEST(ApplyRemovals, AFileThatWasAlreadyDeletedIsNotAnError)
{
    Scenario s("apply-gone-removal");
    buildFixtureScenario(s);
    fs::remove(s.root / "bin/libold-1.dll");
    StdFs disk;
    Rig rig(s, disk);
    EXPECT_EQ(rig.run().outcome, ApplyOutcome::Committed);
}

TEST(ApplyRemovals, ARemovalThatIsALinkIsLeftAlone)
{
    Scenario s("apply-link-removal");
    buildFixtureScenario(s);
    StdFs disk;
    HookFs hooked(disk);
    hooked.adjustStat = [&](const Path& p, EntryInfo& info) {
        if (p == s.root / "bin/libold-1.dll") {
            info.reparsePoint = true;
        }
    };
    Rig rig(s, hooked);
    ASSERT_EQ(rig.run().outcome, ApplyOutcome::Committed);
    EXPECT_TRUE(isPresent(s.root / "bin/libold-1.dll"));
}

TEST(ApplyRemovals, AFolderTheRemovalEmptiedIsRemovedButNeverOneOfTheFourTopFolders)
{
    REQUIRE_CAN_SIGN();
    Scenario s("apply-prune");
    // v1 has bin/only.dll and share/katana/a/b/deep.txt; v2 has neither (and keeps bin/other.dll).
    s.v1.version = "0.1.0";
    s.v1.tag = "v0.1.0";
    s.v1.sequence = 1;
    s.v2.version = "0.2.0";
    s.v2.tag = "v0.2.0";
    s.v2.sequence = 2;
    s.v1.files = {{"bin/only.dll", "x", FileClass::Lib, false},
                  {"share/katana/a/b/deep.txt", "y", FileClass::Data, false},
                  {"share/katana/kept.txt", "z", FileClass::Data, false}};
    s.v2.files = {{"bin/other.dll", "o", FileClass::Lib, false},
                  {"share/katana/kept.txt", "z", FileClass::Data, false}};
    writeInstall(s.root, s.v1);
    writeStage(s.stage, s.v2);
    StdFs disk;
    Rig rig(s, disk);
    ASSERT_EQ(rig.run().outcome, ApplyOutcome::Committed);
    EXPECT_FALSE(isPresent(s.root / "share/katana/a")) << "a and a/b were emptied by the removal";
    EXPECT_TRUE(fs::is_directory(s.root / "share/katana")) << "kept.txt and the records are in it";
    EXPECT_TRUE(fs::is_directory(s.root / "bin")) << "bin stays though only.dll was its first file";
    EXPECT_TRUE(isPresent(s.root / "bin/other.dll"));
}

TEST(ApplySeeds, ASeedThatIsAbsentIsWrittenAndOneThatIsPresentIsNeverReplaced)
{
    {
        Scenario s("apply-seed-absent");
        buildFixtureScenario(s);
        fs::remove(s.root / "etc/katana/settings.ini");
        StdFs disk;
        Rig rig(s, disk);
        ASSERT_EQ(rig.run().outcome, ApplyOutcome::Committed);
        EXPECT_EQ(readText(s.root / "etc/katana/settings.ini"),
                  readText(fixtureFolder() / "install/v2/etc/katana/settings.ini"));
    }
    {
        Scenario s("apply-seed-edited");
        buildFixtureScenario(s);
        writeText(s.root / "etc/katana/settings.ini", "theme=mine\n");
        StdFs disk;
        Rig rig(s, disk);
        ASSERT_EQ(rig.run().outcome, ApplyOutcome::Committed);
        EXPECT_EQ(readText(s.root / "etc/katana/settings.ini"), "theme=mine\n");
    }
}

// ---- the commit: order, and what a failure does -----------------------------------------------------------------------------

TEST(ApplyCommit, ClassesAreCommittedDataThenLibsThenProgramsThenTheHelperThenTheRecords)
{
    REQUIRE_CAN_SIGN();
    Scenario s("apply-order");
    buildFortyFileScenario(s);
    StdFs disk;
    HookFs hooked(disk);
    std::vector<std::string> intoPlace; // R/<path> as each new file is moved in
    hooked.beforeRename = [&](const Path&, const Path& to) -> std::optional<core::Status> {
        const Path relative = to.lexically_relative(s.root);
        // Under the root but not in the transaction's folder: the new file in its place.
        if (!relative.empty() && *relative.begin() != ".." && *relative.begin() != ".update") {
            intoPlace.push_back(relative.generic_string());
        }
        return std::nullopt;
    };
    Rig rig(s, hooked);
    ASSERT_EQ(rig.run().outcome, ApplyOutcome::Committed);

    const auto classOf = [&](const std::string& path) {
        if (path == "bin/katana_updater.exe") {
            return 3;
        }
        if (path.rfind("share/katana/install", 0) == 0) {
            return 4; // the records
        }
        if (const FileSpec* file = s.v2.find(path)) {
            switch (file->fileClass) {
            case FileClass::Data:
            case FileClass::Seed:
                return 0;
            case FileClass::Lib:
                return 1;
            case FileClass::Program:
                return 2;
            }
        }
        return -1;
    };
    int previous = 0;
    std::size_t moved = 0;
    for (const std::string& path : intoPlace) {
        const int rank = classOf(path);
        if (rank < 0) {
            continue;
        }
        EXPECT_GE(rank, previous) << path << " committed out of order";
        previous = std::max(previous, rank);
        ++moved;
    }
    EXPECT_GE(moved, 17u);
    ASSERT_GE(intoPlace.size(), 3u);
    // installed.json, then the manifest, then its signature: the very last three.
    EXPECT_EQ(intoPlace[intoPlace.size() - 3], "share/katana/installed.json");
    EXPECT_EQ(intoPlace[intoPlace.size() - 2], "share/katana/installed.manifest.json");
    EXPECT_EQ(intoPlace[intoPlace.size() - 1], "share/katana/installed.manifest.json.sig");
}

TEST(ApplyCommit, OnlyTheCopiesOfFilesTheManifestMarksExecutableAreMadeExecutable)
{
    REQUIRE_CAN_SIGN();
    Scenario s("apply-exec-bit");
    buildFortyFileScenario(s);
    StdFs disk;
    LogFs log(disk);
    Rig rig(s, log);
    ASSERT_EQ(rig.run().outcome, ApplyOutcome::Committed);

    std::vector<std::string> marked; // the path below new/, '/'-separated
    for (std::string entry : log.log) {
        if (entry.rfind("setExecutable ", 0) != 0) {
            continue;
        }
        std::replace(entry.begin(), entry.end(), '\\', '/');
        const std::size_t at = entry.find("/new/");
        ASSERT_NE(at, std::string::npos) << entry;
        marked.push_back(entry.substr(at + 5));
    }
    std::sort(marked.begin(), marked.end());
    // From the specification (apply_harness.hpp, buildFortyFileScenario): the three programs are
    // the only files with exec set, and all three change between the releases.
    EXPECT_EQ(marked, (std::vector<std::string>{"bin/katana.exe", "bin/katana_cli.exe",
                                                "bin/katana_updater.exe"}));
}

TEST(ApplyCommit, TheStageIsCopiedFromAndNeverRenamedOutOf)
{
    Scenario s("apply-copy-not-rename");
    buildFixtureScenario(s);
    StdFs disk;
    HookFs hooked(disk);
    std::vector<Path> renamedFrom;
    hooked.beforeRename = [&](const Path& from, const Path&) -> std::optional<core::Status> {
        renamedFrom.push_back(from);
        return std::nullopt;
    };
    Rig rig(s, hooked);
    // Fail the health check so the stage is kept: a rolled-back update can be tried again.
    rig.launcher.onRun = [](const SpawnRequest&) -> core::Result<SpawnResult> {
        SpawnResult bad;
        bad.exitCode = 1;
        return bad;
    };
    const ApplyResult result = rig.run();
    ASSERT_EQ(result.outcome, ApplyOutcome::RolledBack);
    for (const Path& from : renamedFrom) {
        const Path relative = from.lexically_relative(s.stage);
        EXPECT_TRUE(relative.empty() || *relative.begin() == "..") << "renamed out of the stage: " << from;
    }
    EXPECT_TRUE(isPresent(s.stage / "bin/katana.exe")) << "the stage is whole for another try";
    EXPECT_TRUE(isPresent(s.stage / "manifest.json"));
}

TEST(ApplyCommit, ARenameThatFailsRollsTheWholeSwapBackToExactlyTheOldTree)
{
    Scenario s("apply-rename-fails");
    buildFixtureScenario(s);
    StdFs disk;
    FaultFs faults(disk);
    faults.failRenameWith(3, 32);
    Rig rig(s, faults);
    const ApplyResult result = rig.run();
    EXPECT_EQ(result.outcome, ApplyOutcome::RolledBack) << result.reason << ": " << result.detail;
    EXPECT_EQ(result.reason, "rename_failed");
    EXPECT_NE(result.detail.find("32"), std::string::npos) << result.detail;
    EXPECT_EQ(treeHash(s.root), s.v1Hash) << difference(s.v1Snapshot, snapshot(s.root));
    EXPECT_TRUE(leftoversInUpdate(s.root).empty());

    // The old version is started again, no health check ran, and the outcome says so.
    ASSERT_EQ(rig.launcher.started.size(), 1u);
    EXPECT_TRUE(rig.launcher.ran.empty());
    const LastResult last = lastOf(s);
    EXPECT_EQ(last.status, "rolled_back");
    EXPECT_EQ(last.reason, "rename_failed");
    EXPECT_TRUE(rig.registry.runOnce.empty());
    EXPECT_TRUE(rig.registry.versions.empty());
}

TEST(ApplyCommit, ARollbackThatItselfFailsIsRollbackIncompleteNeverRolledBack)
{
    Scenario s("apply-rollback-fails");
    buildFixtureScenario(s);
    StdFs disk;
    HookFs hooked(disk);
    // From the sixth rename on every rename fails with a sharing violation: the commit stops,
    // and so does the rollback that tries to move the files back.
    int renames = 0;
    hooked.beforeRename = [&](const Path& from, const Path&) -> std::optional<core::Status> {
        if (++renames >= 6) {
            return injected(32, from);
        }
        return std::nullopt;
    };
    Rig rig(s, hooked);
    const ApplyResult result = rig.run();
    EXPECT_EQ(result.outcome, ApplyOutcome::RollbackIncomplete);
    EXPECT_EQ(result.reason, "rollback_failed");
    EXPECT_NE(result.detail.find("katana_updater --recover"), std::string::npos) << result.detail;
    EXPECT_NE(result.detail.find(core::pathToUtf8(s.root)), std::string::npos)
        << "the command to run by hand names the root";

    // It is said in last.json, and nothing pretends the tree is whole.
    EXPECT_EQ(lastOf(s).status, "rollback_incomplete");
    EXPECT_EQ(lastOf(s).reason, "rollback_failed");
    EXPECT_NE(treeHash(s.root), s.v1Hash);
    // The old files are still in the transaction folder, and the RunOnce entry is still set
    // so that the next logon tries again; and a half-swapped program is not relaunched.
    EXPECT_FALSE(leftoversInUpdate(s.root).empty());
    EXPECT_EQ(rig.registry.runOnce.count(std::string(kRunOnceName)), 1u);
    EXPECT_TRUE(rig.launcher.started.empty());
}

TEST(ApplyCommit, AJournalThatCannotBeWrittenMidCommitIsRolledBack)
{
    Scenario s("apply-journal-commit");
    buildFixtureScenario(s);
    StdFs disk;
    HookFs hooked(disk);
    int appends = 0;
    hooked.beforeAppend = [&](const Path& p, std::string_view) -> std::optional<core::Status> {
        // The PLAN group is the first, COMMITTING the second, the first progress group the third.
        if (p.filename() == "journal" && ++appends == 3) {
            return injected(112, p);
        }
        return std::nullopt;
    };
    Rig rig(s, hooked);
    const ApplyResult result = rig.run();
    EXPECT_EQ(result.outcome, ApplyOutcome::RolledBack) << result.reason << result.detail;
    EXPECT_EQ(result.reason, "journal");
    EXPECT_EQ(treeHash(s.root), s.v1Hash);
}

// ---- health ------------------------------------------------------------------------------------------------------------------


TEST(ApplyHealth, ANewVersionThatExitsNonZeroWithoutItsMarkerIsRolledBackAndNotOfferedAgain)
{
    Scenario s("apply-health-exit");
    buildFixtureScenario(s);
    StdFs disk;
    Rig rig(s, disk);
    rig.launcher.onRun = [](const SpawnRequest&) -> core::Result<SpawnResult> {
        SpawnResult bad;
        bad.exitCode = 3;
        return bad;
    };
    const ApplyResult result = rig.run();
    EXPECT_EQ(result.outcome, ApplyOutcome::RolledBack);
    EXPECT_EQ(result.reason, "health_exit");
    EXPECT_NE(result.detail.find("3"), std::string::npos);
    EXPECT_EQ(treeHash(s.root), s.v1Hash) << difference(s.v1Snapshot, snapshot(s.root));
    EXPECT_TRUE(leftoversInUpdate(s.root).empty());

    // The release goes to `blocked` so the dialog does not offer it again.
    const core::Result<UpdateState> state = readState(s.state);
    ASSERT_TRUE(state.ok());
    EXPECT_EQ(state->blockedSequences, std::vector<std::uint64_t>{1792312800});
    EXPECT_EQ(lastOf(s).status, "rolled_back");
    EXPECT_EQ(lastOf(s).reason, "health_exit");
    // The old version is started.
    EXPECT_EQ(rig.launcher.started.size(), 1u);
}

TEST(ApplyHealth, ANewVersionThatNeverWritesItsMarkerIsRolledBackWhetherItExitedOrTimedOut)
{
    {
        Scenario s("apply-health-no-marker");
        buildFixtureScenario(s);
        StdFs disk;
        Rig rig(s, disk);
        rig.launcher.onRun = [](const SpawnRequest&) -> core::Result<SpawnResult> { return SpawnResult{}; };
        const ApplyResult result = rig.run();
        EXPECT_EQ(result.outcome, ApplyOutcome::RolledBack);
        EXPECT_EQ(result.reason, "health_exit") << "exit 0 without the marker is not a start";
        EXPECT_EQ(treeHash(s.root), s.v1Hash);
    }
    {
        Scenario s("apply-health-timeout");
        buildFixtureScenario(s);
        StdFs disk;
        Rig rig(s, disk);
        rig.launcher.onRun = [](const SpawnRequest&) -> core::Result<SpawnResult> {
            SpawnResult hung;
            hung.timedOut = true;
            hung.exitCode = 1;
            return hung;
        };
        const ApplyResult result = rig.run();
        EXPECT_EQ(result.outcome, ApplyOutcome::RolledBack);
        EXPECT_EQ(result.reason, "health_timeout");
        EXPECT_EQ(treeHash(s.root), s.v1Hash);
    }
}

TEST(ApplyHealth, ANewVersionThatCannotBeStartedAtAllIsRolledBack)
{
    // A missing entry point fails before main(): the program does not start.
    Scenario s("apply-health-no-start");
    buildFixtureScenario(s);
    StdFs disk;
    Rig rig(s, disk);
    rig.launcher.onRun = [](const SpawnRequest& request) -> core::Result<SpawnResult> {
        return core::makeError(core::ErrorCode::NotFound, "the program could not be started",
                               core::pathToUtf8(request.executable));
    };
    const ApplyResult result = rig.run();
    EXPECT_EQ(result.outcome, ApplyOutcome::RolledBack);
    EXPECT_EQ(result.reason, "health_exit");
    EXPECT_EQ(treeHash(s.root), s.v1Hash);
}

TEST(ApplyHealth, AMarkerFollowedByANonZeroExitIsNotHealthyButAHungExitAfterTheMarkerIs)
{
    {
        Scenario s("apply-health-marker-crash");
        buildFixtureScenario(s);
        StdFs disk;
        Rig rig(s, disk);
        rig.launcher.onRun = [&](const SpawnRequest&) -> core::Result<SpawnResult> {
            writeText(healthMarkerPath(s.state, rig.tx), "ok");
            SpawnResult crashed;
            crashed.exitCode = 139;
            return crashed;
        };
        const ApplyResult result = rig.run();
        EXPECT_EQ(result.outcome, ApplyOutcome::RolledBack);
        EXPECT_EQ(result.reason, "health_exit");
        EXPECT_EQ(treeHash(s.root), s.v1Hash);
        EXPECT_FALSE(isPresent(healthMarkerPath(s.state, rig.tx)))
            << "the marker of a start that then failed is not left in the state folder";
    }
    {
        Scenario s("apply-health-marker-hang");
        buildFixtureScenario(s);
        StdFs disk;
        Rig rig(s, disk);
        rig.launcher.onRun = [&](const SpawnRequest&) -> core::Result<SpawnResult> {
            writeText(healthMarkerPath(s.state, rig.tx), "ok");
            SpawnResult slow;
            slow.timedOut = true; // it started, said so, and did not exit: the launcher killed it
            return slow;
        };
        const ApplyResult result = rig.run();
        EXPECT_EQ(result.outcome, ApplyOutcome::Committed);
        EXPECT_EQ(treeHash(s.root), s.v2Hash);
    }
}

TEST(ApplyHealth, TheHelperCopyThatCannotDeleteItselfIsQueuedForTheNextStart)
{
    Scenario s("apply-helper-copy");
    buildFixtureScenario(s);
    StdFs disk;
    Rig rig(s, disk);
    ASSERT_EQ(rig.run().outcome, ApplyOutcome::Committed);
    const std::string list = readText(s.state / "cleanup.list");
    EXPECT_EQ(list, core::pathToUtf8(rig.request.helper.parent_path()) + "\n");
}

TEST(ApplyHealth, TheHelperCopyIsQueuedAfterEveryOutcomeButAFailedRollback)
{
    {
        Scenario s("apply-helper-copy-refused");
        buildFixtureScenario(s);
        fs::remove(s.stage / "manifest.json");
        StdFs disk;
        Rig rig(s, disk);
        ASSERT_EQ(rig.run().outcome, ApplyOutcome::Refused);
        EXPECT_EQ(readText(s.state / "cleanup.list"), core::pathToUtf8(rig.request.helper.parent_path()) + "\n");
    }
    {
        Scenario s("apply-helper-copy-rolled-back");
        buildFixtureScenario(s);
        StdFs disk;
        FaultFs faults(disk);
        faults.failRenameWith(3, 32);
        Rig rig(s, faults);
        ASSERT_EQ(rig.run().outcome, ApplyOutcome::RolledBack);
        EXPECT_EQ(readText(s.state / "cleanup.list"), core::pathToUtf8(rig.request.helper.parent_path()) + "\n");
    }
    {
        // The RunOnce entry re-runs katana_updater --recover from this copy: it stays.
        Scenario s("apply-helper-copy-incomplete");
        buildFixtureScenario(s);
        StdFs disk;
        HookFs hooked(disk);
        int renames = 0;
        hooked.beforeRename = [&](const Path& from, const Path&) -> std::optional<core::Status> {
            return ++renames >= 4 ? std::optional<core::Status>(injected(32, from)) : std::nullopt;
        };
        Rig rig(s, hooked);
        ASSERT_EQ(rig.run().outcome, ApplyOutcome::RollbackIncomplete);
        EXPECT_FALSE(isPresent(s.state / "cleanup.list"));
    }
}

TEST(ApplyHealth, WhatALockKeepsFromBeingDeletedAfterAHealthyUpdateIsQueuedForTheNextStart)
{
    Scenario s("apply-leftovers");
    buildFixtureScenario(s);
    StdFs disk;
    // A scanner or a program holds a file of the old version: it cannot be deleted.
    class Holding final : public ForwardingFs {
      public:
        using ForwardingFs::ForwardingFs;
        core::Status removeFile(const Path& path) override
        {
            if (path.filename() == "katana.exe" && path.string().find("old") != std::string::npos) {
                return osError(core::ErrorCode::FileExportFailure, "sharing violation", path, 32);
            }
            return inner_.removeFile(path);
        }
    } holding(disk);
    Rig rig(s, holding);
    const ApplyResult result = rig.run();
    EXPECT_EQ(result.outcome, ApplyOutcome::Committed) << "the update is done; only the clean-up is not";
    EXPECT_EQ(treeHash(s.root), s.v2Hash);
    // The file is still there, and written to the list the next start works through.
    const Path stuck = s.root / ".update" / rig.tx / "old" / "bin" / "katana.exe";
    EXPECT_TRUE(isPresent(stuck));
    const std::string list = readText(s.state / "cleanup.list");
    EXPECT_NE(list.find("katana.exe"), std::string::npos) << list;
    EXPECT_TRUE(isPresent(s.root / ".update" / rig.tx / "journal")) << "and the journal says what the folder was";
    // The next start sweeps the folder once the file is free.
    ASSERT_TRUE(recoverClean(s).ok());
    EXPECT_TRUE(leftoversInUpdate(s.root).empty());
    EXPECT_EQ(treeHash(s.root), s.v2Hash);
}

// ---- what is left alone, what is planned, and where a rollback must reach --------------------------------------------

TEST(Apply, FilesThatAreAlreadyRightAreNeitherCopiedNorMovedAndThePlanNamesExactlyWhatChanges)
{
    Scenario s("apply-keep");
    buildFixtureScenario(s);
    StdFs disk;
    LogFs log(disk);
    Rig rig(s, log);
    ASSERT_EQ(rig.run().outcome, ApplyOutcome::Committed);

    // Read off the T3 fixtures' two manifests by hand: katana_cli.exe, libcore-1.dll, empty.txt
    // and random.bin have the same SHA-256 in v1 and v2, and settings.ini is a seed that is
    // already there. Not one of them may be copied or moved - this is the whole point of
    // deciding by hash, and the tree comes out the same if it is not done.
    for (const char* kept : {"katana_cli.exe", "libcore-1.dll", "empty.txt", "random.bin", "settings.ini"}) {
        for (const std::string& operation : log.log) {
            if (operation.rfind("copyFile", 0) == 0 || operation.rfind("renameFile", 0) == 0) {
                EXPECT_EQ(operation.find(kept), std::string::npos) << kept << " was touched: " << operation;
            }
        }
    }

    // The PLAN group lists exactly the files that change, are added or are removed, and the
    // folder that has to be made - and the three records the helper writes.
    const std::uint64_t at = log.find("appendDurable", " 1 PLAN ");
    ASSERT_GT(at, 0u);
    const std::string& line = log.log[at - 1];
    const ParsedJournal plan = parseJournal(line.substr(line.find(" 1 PLAN ") + 1));
    std::set<std::string> planned;
    for (const JournalRecord& record : plan.records) {
        if (record.op != JournalOp::Plan) {
            ASSERT_EQ(record.args.size(), 1u);
            planned.insert(std::string(toString(record.op)) + " " + record.args[0]);
        }
    }
    const std::set<std::string> expected = {
        "REPLACE bin/katana.exe",
        "REPLACE share/katana/readme.txt",
        "ADD bin/libnew-2.dll",
        "ADD share/katana/help/index.txt",
        "ADD share/katana/news.txt",
        "REMOVE bin/libold-1.dll",
        "REMOVE share/katana/legacy/old.txt",
        "REPLACE share/katana/installed.json",
        "REPLACE share/katana/installed.manifest.json",
        "REPLACE share/katana/installed.manifest.json.sig",
        "MKDIR share/katana/help",
    };
    EXPECT_EQ(planned, expected);
    // And only those were copied from the stage: five files and nothing else (the records are
    // written, not copied).
    EXPECT_EQ(log.all("copyFile").size(), 5u);
}

TEST(Apply, AnInstallFolderGivenWithATrailingSeparatorIsTheSameFolder)
{
    // A shell's tab completion adds the separator: katana_updater --recover "C:\...\Katana\".
    Scenario s("apply-trailing-separator");
    buildFixtureScenario(s);
    StdFs disk;
    Rig rig(s, disk);
    rig.request.root = Path(core::pathToUtf8(s.root) + "/");
    const ApplyResult result = rig.run();
    ASSERT_EQ(result.outcome, ApplyOutcome::Committed) << result.reason << ": " << result.detail;
    EXPECT_EQ(treeHash(s.root), s.v2Hash) << difference(snapshot(s.expected), snapshot(s.root));
}

TEST(Apply, TheUninstallEntryNamesTheNewVersionOnlyOnceItHasProvedItStarts)
{
    {
        Scenario s("apply-registry-after-health");
        buildFixtureScenario(s);
        StdFs disk;
        Rig rig(s, disk);
        rig.registry.location = s.root;
        std::size_t writtenWhileChecking = 99;
        rig.launcher.onRun = [&](const SpawnRequest& request) -> core::Result<SpawnResult> {
            writtenWhileChecking = rig.registry.versions.size();
            return healthyStart(s.state, rig.tx)(request);
        };
        ASSERT_EQ(rig.run().outcome, ApplyOutcome::Committed);
        EXPECT_EQ(writtenWhileChecking, 0u) << "not before the new version has started";
        ASSERT_EQ(rig.registry.versions.size(), 1u);
        EXPECT_EQ(rig.registry.versions[0].first, "0.10.0");
    }
    {
        // A release that is rolled back must not be the one Settings > Apps names.
        Scenario s("apply-registry-rolled-back");
        buildFixtureScenario(s);
        StdFs disk;
        Rig rig(s, disk);
        rig.registry.location = s.root;
        rig.launcher.onRun = [](const SpawnRequest&) -> core::Result<SpawnResult> {
            SpawnResult bad;
            bad.exitCode = 3;
            return bad;
        };
        ASSERT_EQ(rig.run().outcome, ApplyOutcome::RolledBack);
        EXPECT_TRUE(rig.registry.versions.empty());
    }
}

TEST(ApplyPlan, TheFolderOfTheRecordsIsHeldToTheSameRuleAsEveryOtherFolder)
{
    REQUIRE_CAN_SIGN();
    // No file of either release lives in share/katana, so only the three records the helper
    // itself writes there would go through a link at share/katana.
    Scenario s("apply-reparse-records");
    s.v1 = smallRelease("0.1.0", 100);
    s.v2 = smallRelease("0.2.0", 200);
    for (ReleaseSpec* release : {&s.v1, &s.v2}) {
        for (FileSpec& file : release->files) {
            if (file.path == "share/katana/data.txt") {
                file.path = "share/other/data.txt";
            }
        }
    }
    writeInstall(s.root, s.v1);
    writeStage(s.stage, s.v2);
    s.v1Hash = treeHash(s.root);
    s.v1Snapshot = snapshot(s.root);
    StdFs disk;
    HookFs hooked(disk);
    hooked.adjustStat = [&](const Path& p, EntryInfo& info) {
        if (p == s.root / "share/katana") {
            info.reparsePoint = true;
        }
    };
    Rig rig(s, hooked);
    expectRefused(s, rig.run(), "reparse_point");
}

TEST(ApplyCommit, AFileWithTwoDotsInsideItsNameIsPutBackLikeAnyOther)
{
    REQUIRE_CAN_SIGN();
    // "a..b.dat" is a legal name (only "." and ".." as a whole component are not), and a
    // release may ship one. If rollback refused it, its old file would be lost with the folder.
    Scenario s("apply-dots-in-name");
    s.v1 = smallRelease("0.1.0", 100);
    s.v2 = smallRelease("0.2.0", 200);
    s.v1.files.push_back({"share/katana/a..b.dat", "a..b of the old version\n", FileClass::Data, false});
    s.v2.files.push_back({"share/katana/a..b.dat", "a..b of the new version\n", FileClass::Data, false});
    writeInstall(s.root, s.v1);
    writeStage(s.stage, s.v2);
    s.v1Hash = treeHash(s.root);
    s.v1Snapshot = snapshot(s.root);

    StdFs disk;
    FaultFs faults(disk);
    // The data class goes first, in path order: a..b.dat (renames 1 and 2), then data.txt. The
    // fourth rename, data.txt's new file into place, fails - a..b.dat is the new one by then.
    faults.failRenameWith(4, 32);
    Rig rig(s, faults);
    const ApplyResult result = rig.run();
    EXPECT_EQ(result.outcome, ApplyOutcome::RolledBack) << result.reason << ": " << result.detail;
    EXPECT_EQ(treeHash(s.root), s.v1Hash) << difference(s.v1Snapshot, snapshot(s.root));
    EXPECT_EQ(readText(s.root / "share/katana/a..b.dat"), "a..b of the old version\n");
    EXPECT_FALSE(isPresent(s.root / ".update" / "recovery.log")) << "no record was refused";
}

TEST(ApplyCommit, AFolderTheReleaseBringsForTheFirstTimeIsGoneAgainAfterARollback)
{
    REQUIRE_CAN_SIGN();
    // etc and lib are folders of their own in the journal (a bare name that checkJournalPath
    // refuses for a file); rollback must remove them, or the tree is not the old version's.
    Scenario s("apply-new-top-folders");
    s.v1 = smallRelease("0.1.0", 100);
    s.v2 = smallRelease("0.2.0", 200);
    s.v2.files.push_back({"etc/katana/new.cfg", "a new setting\n", FileClass::Data, false});
    s.v2.files.push_back({"lib/plugins/x.dat", "a new plug-in\n", FileClass::Data, false});
    writeInstall(s.root, s.v1);
    writeStage(s.stage, s.v2);
    s.v1Hash = treeHash(s.root);
    s.v1Snapshot = snapshot(s.root);
    ASSERT_FALSE(isPresent(s.root / "etc"));
    ASSERT_FALSE(isPresent(s.root / "lib"));

    StdFs disk;
    FaultFs faults(disk);
    // Data class in path order: etc/katana/new.cfg (rename 1), lib/plugins/x.dat (2), then
    // data.txt: its old file moves aside (3) - and that one fails.
    faults.failRenameWith(3, 32);
    Rig rig(s, faults);
    const ApplyResult result = rig.run();
    EXPECT_EQ(result.outcome, ApplyOutcome::RolledBack) << result.reason << ": " << result.detail;
    EXPECT_EQ(treeHash(s.root), s.v1Hash) << difference(s.v1Snapshot, snapshot(s.root));
    EXPECT_FALSE(isPresent(s.root / "etc"));
    EXPECT_FALSE(isPresent(s.root / "lib"));
    EXPECT_FALSE(isPresent(s.root / ".update" / "recovery.log")) << "no record was refused";
}

TEST(ApplyHealth, OnlyAStateFileThatIsNotOursToParseMayBeReplacedByTheDefaults)
{
    // A scanner holding state.json for a moment is a failure to READ, and the person's
    // "automatic" and "skipped" in it are not to be forgotten for the sake of one blocked
    // release. (A real hold cannot be told from a failed write by a test: the rename that
    // writes the defaults fails on a held file as well, so the rule is checked on its own.)
    EXPECT_TRUE(detail::mayReplaceUnreadableState(
        core::makeError(core::ErrorCode::ParseFailure, "the record is not a JSON object")));
    for (const core::ErrorCode code :
         {core::ErrorCode::FileImportFailure, core::ErrorCode::NotFound, core::ErrorCode::Unsupported,
          core::ErrorCode::InvalidState, core::ErrorCode::Internal}) {
        EXPECT_FALSE(detail::mayReplaceUnreadableState(core::makeError(code, "x"))) << static_cast<int>(code);
    }
}

TEST(ApplyHealth, AStateFileThatIsNotJsonIsReplacedByTheDefaultsAndTheBlockedRelease)
{
    // state.hpp: an unreadable record is carried on from with the defaults. It is already lost.
    Scenario s("apply-health-corrupt-state");
    buildFixtureScenario(s);
    writeText(s.state / "state.json", "this is not json");
    StdFs disk;
    Rig rig(s, disk);
    rig.launcher.onRun = [](const SpawnRequest&) -> core::Result<SpawnResult> {
        SpawnResult bad;
        bad.exitCode = 3;
        return bad;
    };
    ASSERT_EQ(rig.run().outcome, ApplyOutcome::RolledBack);
    const core::Result<UpdateState> state = readState(s.state);
    ASSERT_TRUE(state.ok());
    EXPECT_EQ(state->blockedSequences, std::vector<std::uint64_t>{1792312800});
    EXPECT_TRUE(state->automatic);
}

// ---- the real Windows file system -----------------------------------------------------------------------------------------------

#ifdef _WIN32

TEST(ApplyOnWindows, TheFixtureUpdateThroughTheRealFileSystemLeavesTheNewTree)
{
    Scenario s("apply-real");
    buildFixtureScenario(s);
    const auto disk = makeSystemFsOps(RetryPolicy{std::chrono::milliseconds(5), std::chrono::milliseconds(100)});
    Rig rig(s, *disk);
    const ApplyResult result = rig.run();
    ASSERT_EQ(result.outcome, ApplyOutcome::Committed) << result.reason << ": " << result.detail;
    EXPECT_EQ(treeHash(s.root), s.v2Hash) << difference(snapshot(s.expected), snapshot(s.root));
    EXPECT_TRUE(leftoversInUpdate(s.root).empty());
}

TEST(ApplyOnWindows, AFileAProgramHoldsOpenRollsTheSwapBackAfterTheRetryWindow)
{
    Scenario s("apply-real-locked");
    buildFixtureScenario(s);
    const auto disk = makeSystemFsOps(RetryPolicy{std::chrono::milliseconds(5), std::chrono::milliseconds(100)});
    // bin/katana.exe is changed by the update. Held open for reading with no delete share, as
    // a running image or a scanner holds it: it can be read (hashed) but not moved.
    HANDLE held = CreateFileW((s.root / "bin/katana.exe").c_str(), GENERIC_READ, FILE_SHARE_READ, nullptr,
                              OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, nullptr);
    ASSERT_NE(held, INVALID_HANDLE_VALUE);
    Rig rig(s, *disk);
    const ApplyResult result = rig.run();
    CloseHandle(held);

    EXPECT_EQ(result.outcome, ApplyOutcome::RolledBack) << result.reason << ": " << result.detail;
    EXPECT_EQ(result.reason, "rename_failed");
    EXPECT_NE(result.detail.find("os_error=32"), std::string::npos) << result.detail;
    EXPECT_EQ(treeHash(s.root), s.v1Hash) << difference(s.v1Snapshot, snapshot(s.root));
    EXPECT_TRUE(leftoversInUpdate(s.root).empty());
}

TEST(ApplyOnWindows, TwoHelpersContendForTheLockThroughTheRealFileSystem)
{
    Scenario s("apply-real-contend");
    buildFixtureScenario(s);
    const auto disk = makeSystemFsOps();
    Rig first(s, *disk, "aaaa");
    first.processes.holders = {RunningProgram{4412, core::pathFromUtf8("katana_mcp.exe")}};
    first.processes.holdersFor = -1;
    first.timings.waitForHolders = std::chrono::seconds(60);
    first.timings.pollInterval = std::chrono::milliseconds(3);
    ApplyResult firstResult;
    std::thread running([&] { firstResult = first.run(); });
    for (int i = 0; i < 5000 && first.processes.scans.load() == 0; ++i) {
        std::this_thread::sleep_for(std::chrono::milliseconds(1));
    }
    ASSERT_GT(first.processes.scans.load(), 0);
    Rig second(s, *disk, "bbbb");
    const ApplyResult secondResult = second.run();
    first.processes.release = true;
    running.join();
    EXPECT_EQ(secondResult.reason, "locked");
    EXPECT_EQ(firstResult.outcome, ApplyOutcome::Committed) << firstResult.reason << firstResult.detail;
}

// ---- what only the real file system can show -----------------------------------------------------------------------------

namespace {

bool makeJunction(const Path& link, const Path& target)
{
    const std::unique_ptr<Launcher> launcher = makeSystemLauncher();
    SpawnRequest request;
    request.executable = Path(core::environmentVariable("SystemRoot")) / "System32" / "cmd.exe";
    request.arguments = {"/c", "mklink", "/J", core::pathToUtf8(link), core::pathToUtf8(target)};
    const core::Result<SpawnResult> ran = launcher->run(request);
    return ran.ok() && ran->exitCode == 0;
}

std::unique_ptr<FsOps> realFs()
{
    return makeSystemFsOps(RetryPolicy{std::chrono::milliseconds(5), std::chrono::milliseconds(100)});
}

// The path with the long-path prefix, for the test's own file calls (the system's long-path
// setting must not decide whether the test can build its tree).
Path prefixed(const Path& path)
{
    return Path(L"\\\\?\\" + path.native());
}

} // namespace

TEST(ApplyOnWindows, AnInstallUnderAVeryDeepFolderIsUpdatedThroughTheLongPathPrefix)
{
    // The root is 216 characters, so that what the library reads by the plain name stays under
    // the 260 of the ordinary calls (the install's own files: 242 to 252) while a copy made
    // under R/.update/<tx>/new/ does not (share/katana/readme.txt there: 261, the new help
    // index: 265). The copy's hash must be taken through the same prefix the file system uses,
    // or the right bytes are refused as "staged_hash".
    //
    // std::filesystem cannot walk a tree by its prefixed name (probed with this library: the
    // iteration and remove_all go wrong), so the test builds and checks the install at a short
    // place and MOVES it to the deep one for the update and back afterwards.
    Scenario s("apply-long-root");
    buildFixtureScenario(s);
    const Path top = s.scratch.path() / std::string(60, 'a');
    if (top.native().size() > 150) {
        GTEST_SKIP() << "the temporary folder is too deep to build a root of 216 characters in it";
    }
    Path deep = top;
    std::size_t index = 0;
    while (deep.native().size() + 61 <= 190) {
        deep /= std::string(60, static_cast<char>('b' + index++ % 20));
    }
    deep /= std::string(216 - deep.native().size() - 1, 'z');
    ASSERT_EQ(deep.native().size(), 216u);

    std::error_code ec;
    fs::create_directories(prefixed(deep.parent_path()), ec);
    ASSERT_FALSE(ec) << ec.message();
    fs::rename(s.root, prefixed(deep), ec);
    ASSERT_FALSE(ec) << ec.message();
    // Whatever happens next, the install comes back to where the test can look at it, and the
    // empty folders it stood in are removed one by one.
    struct Restore {
        const Scenario& scenario;
        const Path& deep;
        const Path& top;
        ~Restore()
        {
            std::error_code error;
            fs::rename(prefixed(deep), scenario.root, error);
            for (Path folder = deep.parent_path(); folder.native().size() >= top.native().size();
                 folder = folder.parent_path()) {
                fs::remove(prefixed(folder), error);
            }
        }
    } restore{s, deep, top};

    const auto disk = realFs();
    Rig rig(s, *disk);
    rig.request.root = deep; // the ordinary spelling, as a command line carries it
    const ApplyResult result = rig.run();
    EXPECT_EQ(result.outcome, ApplyOutcome::Committed) << result.reason << ": " << result.detail;
    fs::rename(prefixed(deep), s.root, ec);
    ASSERT_FALSE(ec) << ec.message();
    EXPECT_EQ(treeHash(s.root), s.v2Hash) << difference(snapshot(s.expected), snapshot(s.root));
    EXPECT_TRUE(leftoversInUpdate(s.root).empty());
}

TEST(ApplyOnWindows, AnInstallUnderAFolderWithASpaceAndNonAsciiLettersIsUpdated)
{
    Scenario s("apply-non-ascii-root");
    s.root = s.scratch.path() / core::pathFromUtf8("K\xC3\xA4se \xE6\x97\xA5\xE6\x9C\xAC") / "install";
    buildFixtureScenario(s);
    const auto disk = realFs();
    Rig rig(s, *disk);
    const ApplyResult result = rig.run();
    ASSERT_EQ(result.outcome, ApplyOutcome::Committed) << result.reason << ": " << result.detail;
    EXPECT_EQ(treeHash(s.root), s.v2Hash) << difference(snapshot(s.expected), snapshot(s.root));
    EXPECT_TRUE(leftoversInUpdate(s.root).empty());
}

TEST(ApplyOnWindows, AnInstallFolderGivenInAnotherCaseIsTheSameFolder)
{
    Scenario s("apply-root-case");
    buildFixtureScenario(s);
    std::string shouting = core::pathToUtf8(s.root);
    for (char& c : shouting) {
        c = static_cast<char>(std::toupper(static_cast<unsigned char>(c)));
    }
    const auto disk = realFs();
    Rig rig(s, *disk);
    rig.request.root = core::pathFromUtf8(shouting);
    ASSERT_NE(rig.request.root.native(), s.root.native());
    const ApplyResult result = rig.run();
    ASSERT_EQ(result.outcome, ApplyOutcome::Committed) << result.reason << ": " << result.detail;
    EXPECT_EQ(treeHash(s.root), s.v2Hash) << difference(snapshot(s.expected), snapshot(s.root));
}

TEST(ApplyOnWindows, ANameThatDiffersOnlyInCaseIsOneFileAndIsNotRemovedAfterItIsInstalled)
{
    REQUIRE_CAN_SIGN();
    // v1 lists bin/Old.dll and v2 bin/old.dll: on NTFS one file. v1's manifest lists a name
    // v2's does not (to a byte-wise comparison), but removing "Old.dll" after "old.dll" was
    // installed would delete what was just put in place.
    Scenario s("apply-case-removal");
    s.v1 = smallRelease("0.1.0", 100);
    s.v2 = smallRelease("0.2.0", 200);
    s.v1.files.push_back({"bin/Old.dll", "the old Old.dll\n", FileClass::Lib, false});
    s.v2.files.push_back({"bin/old.dll", "the new old.dll\n", FileClass::Lib, false});
    writeInstall(s.root, s.v1);
    writeStage(s.stage, s.v2);
    const auto disk = realFs();
    Rig rig(s, *disk);
    const ApplyResult result = rig.run();
    ASSERT_EQ(result.outcome, ApplyOutcome::Committed) << result.reason << ": " << result.detail;
    EXPECT_EQ(readText(s.root / "bin/old.dll"), "the new old.dll\n");
    std::set<std::string> names;
    for (const auto& entry : fs::directory_iterator(s.root / "bin")) {
        names.insert(entry.path().filename().string());
    }
    EXPECT_EQ(names, (std::set<std::string>{"katana.exe", "lib.dll", "old.dll"}));
}

TEST(ApplyOnWindows, ARealJunctionInsideTheInstallRefusesTheUpdateAndNothingBeyondItIsWritten)
{
    REQUIRE_CAN_SIGN();
    Scenario s("apply-junction");
    s.v1 = smallRelease("0.1.0", 100);
    s.v2 = smallRelease("0.2.0", 200);
    s.v2.files.push_back({"share/katana/ext/new.dat", "must not leave the install\n", FileClass::Data, false});
    writeInstall(s.root, s.v1);
    writeStage(s.stage, s.v2);
    const Path outside = s.scratch.path() / "outside";
    fs::create_directories(outside);
    ASSERT_TRUE(makeJunction(s.root / "share" / "katana" / "ext", outside)) << "mklink /J";
    const std::string before = treeHash(s.root);

    const auto disk = realFs();
    Rig rig(s, *disk);
    const ApplyResult result = rig.run();
    EXPECT_EQ(result.outcome, ApplyOutcome::Refused) << result.reason << ": " << result.detail;
    EXPECT_EQ(result.reason, "reparse_point");
    EXPECT_TRUE(fs::is_empty(outside)) << "nothing was written through the junction";
    EXPECT_EQ(treeHash(s.root), before);
    EXPECT_TRUE(leftoversInUpdate(s.root).empty());
}

#endif // _WIN32

// ---- an exception from below -------------------------------------------------------------------------------------------------

namespace {

// Throws a std::exception - what a conversion, an allocation or a library does, and a crash
// injected by FaultFs does NOT (SimulatedCrash is not one) - in place of the k-th mutating
// operation - once, or (`sticky`) at that one and every one after it, as when a disk has gone.
// The operations are the eight FaultFs counts.
class ThrowingFs final : public ForwardingFs {
  public:
    ThrowingFs(FsOps& inner, std::uint64_t throwBefore, bool sticky)
        : ForwardingFs(inner), throwBefore_(throwBefore), sticky_(sticky)
    {
    }

    core::Status makeDirectories(const Path& p) override
    {
        step();
        return inner_.makeDirectories(p);
    }
    core::Status copyFile(const Path& a, const Path& b) override
    {
        step();
        return inner_.copyFile(a, b);
    }
    core::Status renameFile(const Path& a, const Path& b) override
    {
        step();
        return inner_.renameFile(a, b);
    }
    core::Status removeFile(const Path& p) override
    {
        step();
        return inner_.removeFile(p);
    }
    core::Status removeEmptyDirectory(const Path& p) override
    {
        step();
        return inner_.removeEmptyDirectory(p);
    }
    core::Status appendDurable(const Path& p, std::string_view bytes) override
    {
        step();
        return inner_.appendDurable(p, bytes);
    }
    core::Status writeDurable(const Path& p, std::string_view bytes) override
    {
        step();
        return inner_.writeDurable(p, bytes);
    }
    core::Status setExecutable(const Path& p) override
    {
        step();
        return inner_.setExecutable(p);
    }

    [[nodiscard]] bool thrown() const { return thrown_; }

  private:
    void step()
    {
        ++count_;
        if (count_ == throwBefore_ || (sticky_ && count_ > throwBefore_)) {
            thrown_ = true;
            throw std::runtime_error("injected: not a core::Error and not a crash");
        }
    }

    std::uint64_t throwBefore_;
    bool sticky_;
    std::uint64_t count_ = 0;
    bool thrown_ = false;
};

// For every operation k of a clean small update: the update runs on a file system that throws
// at k (and, if `sticky`, at every operation after it); apply() must return, never throw, and the
// tree must end as the algorithm says - then recovery on a clean disk must make it v1 or v2.
void exceptionAtEveryOperation(bool sticky)
{
    Loop loop("apply-throws", [](Scenario& s) {
        buildSmallScenario(s);
        writeInstall(s.expected, s.v2); // the tree a good update leaves, from the specification
        s.v2Hash = treeHash(s.expected);
    });
    Scenario& s = loop.scenario();

    // The operations of a clean run, and where its PLAN group and its HEALTHY record fall.
    std::uint64_t total = 0;
    std::uint64_t plan = 0;
    std::uint64_t healthy = 0;
    {
        StdFs bottom;
        LogFs log(bottom);
        Rig rig(s, log);
        ASSERT_EQ(rig.run().outcome, ApplyOutcome::Committed);
        total = log.log.size();
        plan = log.find("appendDurable", "PLAN");
        healthy = log.find("appendDurable", " HEALTHY ");
    }
    ASSERT_GT(plan, 0u);
    ASSERT_GT(healthy, plan);
    ASSERT_GT(total, healthy);

    for (std::uint64_t k = 1; k <= total; ++k) {
        SCOPED_TRACE((sticky ? "every operation from " : "exception before operation ") +
                     std::to_string(k) + " of " + std::to_string(total));
        loop.reset();
        StdFs bottom;
        ThrowingFs throwing(bottom, k, sticky);
        Rig rig(s, throwing);
        ApplyResult result;
        try {
            result = rig.run();
        } catch (...) {
            FAIL() << "an exception escaped apply()";
        }
        ASSERT_TRUE(throwing.thrown()) << "the operation was never reached";

        // From the algorithm: before the PLAN group is written nothing under the install has
        // moved; between that and the HEALTHY record the tree is put back from the disk; the
        // HEALTHY record comes after the new version proved itself, so only the clean-up is
        // lost and the update stands.
        std::string expectedAfterRecovery;
        if (k <= plan) {
            EXPECT_EQ(result.outcome, ApplyOutcome::Refused) << result.reason << ": " << result.detail;
            EXPECT_EQ(result.reason, "internal_error");
            EXPECT_EQ(treeHash(s.root), s.v1Hash) << difference(s.v1Snapshot, snapshot(s.root));
            if (!sticky) {
                EXPECT_TRUE(leftoversInUpdate(s.root).empty()) << "the folder of copies is deleted";
            }
            expectedAfterRecovery = s.v1Hash;
        } else if (k < healthy) {
            if (sticky) {
                // The disk is gone for the rollback too: said so, with the command that puts it right.
                EXPECT_EQ(result.outcome, ApplyOutcome::RollbackIncomplete)
                    << result.reason << ": " << result.detail;
                EXPECT_EQ(result.reason, "rollback_failed");
                EXPECT_NE(result.detail.find("katana_updater --recover"), std::string::npos);
            } else {
                EXPECT_EQ(result.outcome, ApplyOutcome::RolledBack)
                    << result.reason << ": " << result.detail;
                EXPECT_EQ(result.reason, "internal_error");
                EXPECT_EQ(treeHash(s.root), s.v1Hash) << difference(s.v1Snapshot, snapshot(s.root));
                EXPECT_TRUE(leftoversInUpdate(s.root).empty());
            }
            expectedAfterRecovery = s.v1Hash;
        } else {
            EXPECT_EQ(result.outcome, ApplyOutcome::Committed) << result.reason << ": " << result.detail;
            EXPECT_EQ(treeHash(s.root), s.v2Hash) << difference(snapshot(s.expected), snapshot(s.root));
            expectedAfterRecovery = s.v2Hash;
        }

        // Whatever it was, the lock is free, and recovery on a disk that works makes the tree
        // one whole version: the old one before the commit and during it, the new one after.
        StdFs after;
        EXPECT_TRUE(after.tryLock(s.root / ".update" / "lock").ok());
        ASSERT_TRUE(recoverClean(s).ok());
        EXPECT_EQ(treeHash(s.root), expectedAfterRecovery);
    }
}

} // namespace

TEST(ApplyUnexpected, AnExceptionAtAnyOperationEndsInTheOldTreeOrTheNewOneAndNeverEscapes)
{
    REQUIRE_CAN_SIGN();
    exceptionAtEveryOperation(false);
}

TEST(ApplyUnexpected, AFileSystemThatKeepsThrowingFromAnyOperationOnEndsTheSameWayOrSaysTheRollbackFailed)
{
    REQUIRE_CAN_SIGN();
    exceptionAtEveryOperation(true);
}

// ---- the Windows launcher ------------------------------------------------------------------------------------------------------

#ifdef _WIN32

namespace {

Path systemProgram(const char* relative)
{
    return Path(core::environmentVariable("SystemRoot")) / relative;
}

Path cmdExe()
{
    return systemProgram("System32/cmd.exe");
}

Path powershell()
{
    return systemProgram("System32/WindowsPowerShell/v1.0/powershell.exe");
}

SpawnRequest cmd(const std::string& command)
{
    SpawnRequest request;
    request.executable = cmdExe();
    request.arguments = {"/c", command};
    request.timeout = std::chrono::seconds(30);
    return request;
}

// Detaches the test program from its console for the length of a test, so that a console
// program it starts has no console to inherit: WITHOUT CREATE_NO_WINDOW Windows then opens a
// new console window for it (a black one, in life), and with the flag it does not. Run from a
// terminal or by the test harness the child would otherwise share the parent's console and the
// flag would make no difference to what it sees.
class WithoutConsole {
  public:
    WithoutConsole() : detached_(FreeConsole() != 0) {}
    WithoutConsole(const WithoutConsole&) = delete;
    WithoutConsole& operator=(const WithoutConsole&) = delete;
    ~WithoutConsole()
    {
        if (detached_) {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }

  private:
    bool detached_;
};

// The window handle GetConsoleWindow gives a process: 0 for a console program started
// without a window (CREATE_NO_WINDOW), the window of a console it was given otherwise.
// Asked of PowerShell, which can call it, because nothing else installed can.
const char* kConsoleWindowScript =
    "Add-Type -Namespace K -Name W -MemberDefinition "
    "'[DllImport(\"kernel32.dll\")] public static extern System.IntPtr GetConsoleWindow();'; "
    "[K.W]::GetConsoleWindow().ToInt64()";

} // namespace

TEST(Launcher, RunWaitsForTheProgramAndReportsItsExitCodeAndItsMergedOutput)
{
    const std::unique_ptr<Launcher> launcher = makeSystemLauncher();
    const core::Result<SpawnResult> result = launcher->run(cmd("echo to-stdout & echo to-stderr 1>&2 & exit 7"));
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_EQ(result->exitCode, 7);
    EXPECT_FALSE(result->timedOut);
    EXPECT_NE(result->output.find("to-stdout"), std::string::npos) << result->output;
    EXPECT_NE(result->output.find("to-stderr"), std::string::npos) << "standard error is merged in";

    const core::Result<SpawnResult> fine = launcher->run(cmd("exit 0"));
    ASSERT_TRUE(fine.ok());
    EXPECT_EQ(fine->exitCode, 0);
}

TEST(Launcher, ARunThatTakesTooLongIsKilledAndSaidToHaveTimedOut)
{
    const std::unique_ptr<Launcher> launcher = makeSystemLauncher();
    SpawnRequest request = cmd("ping -n 6 127.0.0.1 >nul");
    request.timeout = std::chrono::milliseconds(300);
    const auto start = std::chrono::steady_clock::now();
    const core::Result<SpawnResult> result = launcher->run(request);
    const auto took = std::chrono::steady_clock::now() - start;
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_TRUE(result->timedOut);
    EXPECT_GE(took, std::chrono::milliseconds(280));
    EXPECT_LT(took, std::chrono::seconds(4)) << "and it did not wait for the program to end";
}

TEST(Launcher, TheOutputOfAChattyProgramIsCappedAtSixtyFourKibiBytesWithoutBlockingIt)
{
    const std::unique_ptr<Launcher> launcher = makeSystemLauncher();
    // About 130 KB of output: twice the cap.
    const core::Result<SpawnResult> result =
        launcher->run(cmd("for /l %i in (1,1,3000) do @echo xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"));
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_EQ(result->exitCode, 0) << "the program was read to its end, not left blocked on a full pipe";
    EXPECT_FALSE(result->timedOut);
    EXPECT_EQ(result->output.size(), 64u * 1024u);
}

TEST(Launcher, AProgramThatIsNotThereIsAnErrorNotAnExitCode)
{
    const std::unique_ptr<Launcher> launcher = makeSystemLauncher();
    SpawnRequest request;
    request.executable = Path("C:/definitely/not/here/nothing.exe");
    const core::Result<SpawnResult> result = launcher->run(request);
    ASSERT_FALSE(result.ok());
    EXPECT_EQ(result.error().code, core::ErrorCode::NotFound);
    EXPECT_EQ(result.error().context.rfind("os_error=3 ", 0), 0u) << result.error().describe(); // ERROR_PATH_NOT_FOUND
    EXPECT_FALSE(launcher->startDetached(request).ok());
}

// A file that is no program (a text file named .exe, as a stub in a test tree or a truncated
// download can be) used to make Windows raise a system message box - "This app can't run on
// your PC", or "Unsupported 16-Bit Application" - and wait for a click: in an unattended helper
// that is a hang. The launcher must say it as an error and show nothing. If this test ever
// hangs, the dialog is back; the ctest timeout turns that into a failure.
TEST(Launcher, AFileThatIsNoProgramIsAnErrorAndNeverASystemDialog)
{
    const std::unique_ptr<Launcher> launcher = makeSystemLauncher();
    Scratch scratch("launcher-no-program");
    const Path notAProgram = scratch.path() / "not-a-program.exe";
    {
        std::ofstream out(notAProgram, std::ios::binary);
        out << "this is text, not a program" << '\n';
    }
    SpawnRequest request;
    request.executable = notAProgram;

    const UINT before = SetErrorMode(0);
    SetErrorMode(before); // read it without changing it
    const auto start = std::chrono::steady_clock::now();
    const core::Result<SpawnResult> waited = launcher->run(request);
    const core::Status detached = launcher->startDetached(request);
    const auto took = std::chrono::steady_clock::now() - start;
    const UINT after = SetErrorMode(0);
    SetErrorMode(after);

    ASSERT_FALSE(waited.ok());
    // ERROR_BAD_EXE_FORMAT (193) on older Windows, ERROR_EXE_MACHINE_TYPE_MISMATCH (216) on newer
    // ones for a file this short: both say "this file is no program for this machine".
    const std::string context = waited.error().context;
    EXPECT_TRUE(context.rfind("os_error=193 ", 0) == 0 || context.rfind("os_error=216 ", 0) == 0)
        << waited.error().describe();
    EXPECT_FALSE(detached.ok());
    EXPECT_LT(took, std::chrono::seconds(10)) << "it failed at once instead of waiting for a click";
    EXPECT_EQ(after, before) << "the process error mode is put back as it was";
}

TEST(Launcher, TheWorkingDirectoryIsTheOneAskedFor)
{
    const std::unique_ptr<Launcher> launcher = makeSystemLauncher();
    Scratch scratch("launcher-cwd");
    SpawnRequest request = cmd("cd");
    request.workingDirectory = scratch.path();
    const core::Result<SpawnResult> result = launcher->run(request);
    ASSERT_TRUE(result.ok());
    std::string printed = result->output;
    while (!printed.empty() && (printed.back() == '\r' || printed.back() == '\n')) {
        printed.pop_back();
    }
    EXPECT_TRUE(_stricmp(printed.c_str(), core::pathToUtf8(scratch.path()).c_str()) == 0)
        << printed << " vs " << scratch.path().string();
}

TEST(Launcher, ArgumentsReachTheProgramWhateverTheyHold)
{
    if (!fs::exists(powershell())) {
        SKIP_UNLESS_AVAILABLE_OR_FAIL_IN_CI("no powershell.exe to print the arguments it was given");
    }
    Scratch scratch("launcher-args");
    writeText(scratch.path() / "args.ps1", "$args | ForEach-Object { '[' + $_ + ']' }\n");
    SpawnRequest request;
    request.executable = powershell();
    request.arguments = {"-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File",
                         core::pathToUtf8(scratch.path() / "args.ps1"),
                         "plain", "two words", "say \"hi\"", "ends\\", "has space\\", "--root",
                         "C:\\Program Files\\Katana Install"};
    request.timeout = std::chrono::seconds(60);
    const std::unique_ptr<Launcher> launcher = makeSystemLauncher();
    const core::Result<SpawnResult> result = launcher->run(request);
    ASSERT_TRUE(result.ok()) << result.error().describe();
    ASSERT_EQ(result->exitCode, 0) << result->output;
    const std::string expected = "[plain]\r\n[two words]\r\n[say \"hi\"]\r\n[ends\\]\r\n[has space\\]\r\n[--root]\r\n"
                                 "[C:\\Program Files\\Katana Install]\r\n";
    EXPECT_EQ(result->output, expected);
}

TEST(Launcher, ARunStartsAConsoleProgramWithNoConsoleWindow)
{
    // CREATE_NO_WINDOW: katana_updater is a console program started from a program with no
    // console, and without the flag Windows opens a black one for as long as it runs.
    if (!fs::exists(powershell())) {
        SKIP_UNLESS_AVAILABLE_OR_FAIL_IN_CI("no powershell.exe to ask");
    }
    SpawnRequest request;
    request.executable = powershell();
    request.arguments = {"-NoProfile", "-NonInteractive", "-Command", kConsoleWindowScript};
    request.timeout = std::chrono::seconds(60);
    const std::unique_ptr<Launcher> launcher = makeSystemLauncher();
    const WithoutConsole noConsole;
    const core::Result<SpawnResult> result = launcher->run(request);
    ASSERT_TRUE(result.ok()) << result.error().describe();
    if (result->exitCode != 0) {
        SKIP_UNLESS_AVAILABLE_OR_FAIL_IN_CI("PowerShell cannot compile the probe here: " + result->output);
    }
    EXPECT_EQ(result->output, "0\r\n") << "a console window handle: the child was given a window";
}

TEST(Launcher, ADetachedStartReturnsAtOnceAndTheChildHasNoConsoleWindowEither)
{
    if (!fs::exists(powershell())) {
        SKIP_UNLESS_AVAILABLE_OR_FAIL_IN_CI("no powershell.exe to ask");
    }
    Scratch scratch("launcher-detached");
    const Path answer = scratch.path() / "answer.txt";
    SpawnRequest request;
    request.executable = powershell();
    request.arguments = {"-NoProfile", "-NonInteractive", "-Command",
                         std::string(kConsoleWindowScript) + " | Out-File -Encoding ascii '" +
                             core::pathToUtf8(answer) + "'"};
    const std::unique_ptr<Launcher> launcher = makeSystemLauncher();
    const WithoutConsole noConsole;
    ASSERT_TRUE(launcher->startDetached(request).ok());
    // Compiling the probe takes the child a second or more; if it has already answered, the
    // call waited for it.
    EXPECT_FALSE(isPresent(answer)) << "it must not wait for the child";
    for (int i = 0; i < 600 && !isPresent(answer); ++i) {
        std::this_thread::sleep_for(std::chrono::milliseconds(100));
    }
    if (!isPresent(answer)) {
        SKIP_UNLESS_AVAILABLE_OR_FAIL_IN_CI(
            "the child wrote no answer in a minute (PowerShell cannot compile the probe here?)");
    }
    std::this_thread::sleep_for(std::chrono::milliseconds(200));
    std::string text = readText(answer);
    while (!text.empty() && (text.back() == '\r' || text.back() == '\n')) {
        text.pop_back();
    }
    EXPECT_EQ(text, "0");
}

TEST(Launcher, ADetachedChildOutlivesTheCallThatStartedIt)
{
    Scratch scratch("launcher-outlives");
    const Path marker = scratch.path() / "done.txt";
    const std::unique_ptr<Launcher> launcher = makeSystemLauncher();
    // cmd.exe does not read the backslash-quote of the usual quoting, so the file is named
    // relative to the working directory and nothing in the command needs a quote.
    SpawnRequest request = cmd("ping -n 2 127.0.0.1 >nul & echo finished> done.txt");
    request.workingDirectory = scratch.path();
    ASSERT_TRUE(launcher->startDetached(request).ok());
    EXPECT_FALSE(isPresent(marker)) << "not done yet when startDetached returns";
    for (int i = 0; i < 100 && !isPresent(marker); ++i) {
        std::this_thread::sleep_for(std::chrono::milliseconds(100));
    }
    EXPECT_TRUE(isPresent(marker)) << "the child kept running by itself";
}

TEST(Launcher, APathWithANonAsciiLetterReachesTheChildIntact)
{
    if (!fs::exists(powershell())) {
        SKIP_UNLESS_AVAILABLE_OR_FAIL_IN_CI("no powershell.exe to print the arguments it was given");
    }
    Scratch scratch("launcher-unicode");
    writeText(scratch.path() / "args.ps1",
              "[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding $false\n"
              "$args | ForEach-Object { [Console]::Out.Write('[' + $_ + ']') }\n");
    SpawnRequest request;
    request.executable = powershell();
    request.arguments = {"-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File",
                         core::pathToUtf8(scratch.path() / "args.ps1"), "C:\\Users\\Jos\xC3\xA9 M\xC3\xBCller"};
    request.timeout = std::chrono::seconds(60);
    const std::unique_ptr<Launcher> launcher = makeSystemLauncher();
    const core::Result<SpawnResult> result = launcher->run(request);
    ASSERT_TRUE(result.ok()) << result.error().describe();
    ASSERT_EQ(result->exitCode, 0) << result->output;
    EXPECT_EQ(result->output, "[C:\\Users\\Jos\xC3\xA9 M\xC3\xBCller]");
}

#endif // _WIN32
