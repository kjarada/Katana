// The UPDATE verbs (src/katana_app/update_verbs.hpp): each line of the grammar
// run for real over a fake feed, a scratch install and a scratch state folder.
//
// WHERE THE EXPECTATIONS COME FROM (the testing rule: never from this program's
// own output). The releases are the fixtures of tools/update_make_fixtures.py
// (tests/update/fixtures): two install trees, v1 = 0.9.0 (sequence 1790000000)
// and v2 = 0.10.0 (sequence 1792312800), and for each a manifest made by the
// Python producer and signed with `openssl` by the published RFC 8032 test
// key. What an update from v1 to v2 fetches is read off the signed v2 manifest
// and the two trees, by hand:
//
//   path                          v1                v2                     csize  offset
//   bin/katana.exe                180 bytes         207 bytes, other hash    85      44   CHANGED
//   bin/katana_cli.exe            the same hash                                           kept
//   bin/libcore-1.dll             the same hash                                           kept
//   bin/libnew-2.dll              absent            118 bytes                70     418   CHANGED
//   etc/katana/settings.ini       60 bytes          a seed: written only when absent      kept
//   share/katana/empty.txt        the same hash                                           kept
//   share/katana/help/index.txt   absent            82 bytes                 75     599   CHANGED
//   share/katana/news.txt         absent            54 bytes                 54     725   CHANGED
//   share/katana/random.bin       the same hash                                           kept
//   share/katana/readme.txt       52 bytes          55 bytes, other hash     55    2938   CHANGED
//
// So 5 of the manifest's 10 files change, and their compressed sizes add to
// 85 + 70 + 75 + 54 + 55 = 339: `files=5/10 download_bytes=339`. The changed
// members lie between offset 44 and 2938 + 55 = 2993 with holes far below the
// megabyte at which requests merge (update/ranges.hpp), so a first download is
// ONE request for bytes [44, 2993), 2949 of them. The payload is 3791 bytes.
//
// The clock is the test's: every context says it is 2026-10-19T00:00:00Z,
// which is 1792368000 (the v2 manifest's own sequence, 1792312800, is its
// `published` time 2026-10-18T08:40:00Z, so midnight that day is 1792281600
// and the next midnight 86400 later).
//
// The file system the staging goes through is the system's, which is written
// for Windows only so far (update/fsops.hpp): the tests that download or apply
// run there, and everywhere else one test holds the verbs to their refusal.

#include <gtest/gtest.h>

#include <algorithm>
#include <cstdint>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <iterator>
#include <map>
#include <memory>
#include <optional>
#include <sstream>
#include <stop_token>
#include <string>
#include <string_view>
#include <system_error>
#include <utility>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/core/path_text.hpp"
#include "katana/update/apply.hpp"
#include "katana/update/check.hpp"
#include "katana/update/credential.hpp"
#include "katana/update/feed.hpp"
#include "katana/update/fsops.hpp"
#include "katana/update/install.hpp"
#include "katana/update/process.hpp"
#include "katana/update/signature.hpp"
#include "katana/update/stage.hpp"
#include "katana/update/state.hpp"
#include "katana/update/trust.hpp"
#include "katana/update/version.hpp"
#include "mcp_tools.hpp"
#include "session.hpp"
#include "start_environment.hpp"
// The two public test keys, by their hex: the trust is built from them below.
#include "update/test_key.hpp"
#include "update_verbs.hpp"

// Last, so that no header above is read with its macros: where this test
// program is (GetModuleFileNameW) and which process it is
// (GetCurrentProcessId), for the tests of the download and the apply.
#ifdef _WIN32
#ifndef NOMINMAX
#define NOMINMAX
#endif
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>
#endif

// tests/update/fixtures. The suite is told where the survey fixtures are
// (tests/app/CMakeLists.txt), and these lie two folders from them.
#ifndef KATANA_UPDATE_FIXTURES
#define KATANA_UPDATE_FIXTURES KATANA_SURVEYIO_DATA "/../../update/fixtures"
#endif

namespace {

namespace fs = std::filesystem;
namespace core = katana::core;
namespace update = katana::update;
using katana::app::runUpdateLine;
using katana::app::UpdateContext;
using katana::app::UpdateReply;
using katana::app::UpdateSurface;

constexpr std::uint64_t kV1Sequence = 1790000000;
constexpr std::uint64_t kV2Sequence = 1792312800;
// 2026-10-19T00:00:00Z (the head of this file).
constexpr std::int64_t kNow = 1792368000;
constexpr const char* kNowText = "2026-10-19T00:00:00Z";

constexpr const char* kSignatureUrl =
    "https://github.com/kjarada/Katana/releases/latest/download/Katana-update-win64.json.sig";
constexpr const char* kManifestUrl =
    "https://github.com/kjarada/Katana/releases/latest/download/Katana-update-win64.json";
// The payload of the release the manifest names: its tag and its payload.name.
constexpr const char* kPayloadUrl =
    "https://github.com/kjarada/Katana/releases/download/v0.10.0/Katana-0.10.0-win64.update.zip";

// What UPDATE CHECK says of v2 to a v1 install (the head of this file).
const std::string kNotes = "https://github.com/kjarada/Katana/releases/tag/v0.10.0";
const std::string kAvailable =
    "update=available installed=0.9.0 latest=0.10.0 files=5/10 download_bytes=339 notes_url=" +
    kNotes;
// What UPDATE STATUS says of a v1 install with nothing kept, staged or stored.
const std::string kFreshStatus = "installed=0.9.0 target=win64 kind=archive staged=none "
                                 "skipped=none auto=on last_check=never token=absent last=none";

constexpr std::string_view kSecret = "ghp_SECRET-TOKEN-0123456789";

fs::path fixtures()
{
    return fs::path(KATANA_UPDATE_FIXTURES);
}

std::string bytesOf(const fs::path& file)
{
    std::ifstream in(file, std::ios::binary);
    EXPECT_TRUE(in.good()) << file.generic_string();
    return std::string(std::istreambuf_iterator<char>(in), std::istreambuf_iterator<char>());
}

void writeBytes(const fs::path& file, std::string_view bytes)
{
    fs::create_directories(file.parent_path());
    std::ofstream out(file, std::ios::binary | std::ios::trunc);
    out.write(bytes.data(), static_cast<std::streamsize>(bytes.size()));
    ASSERT_TRUE(out.good()) << file.generic_string();
}

// Every file under `root`, by its path from there, with its bytes: what a
// tree is, to say that it did not change.
std::map<std::string, std::string> treeOf(const fs::path& root)
{
    std::map<std::string, std::string> files;
    std::error_code error;
    for (fs::recursive_directory_iterator at(root, error), end; !error && at != end;
         at.increment(error)) {
        if (at->is_regular_file()) {
            files[fs::relative(at->path(), root).generic_string()] = bytesOf(at->path());
        }
    }
    return files;
}

std::vector<std::string> linesOf(const std::string& text)
{
    std::vector<std::string> lines;
    std::istringstream in(text);
    for (std::string line; std::getline(in, line);) {
        lines.push_back(line);
    }
    return lines;
}

// The trust of the two published test keys: who signed the fixtures. Built
// here from the keys' hex, since the suite the engine's own tests share their
// definition with is another executable.
const update::Trust& testKeys()
{
    static const update::Trust trust = [] {
        update::Trust built;
        for (const std::string_view hex :
             {update::kTestActivePublicHex, update::kTestSparePublicHex}) {
            update::TrustedKey key;
            key.key = update::parsePublicKeyHex(hex).value();
            key.id = update::keyIdOf(key.key);
            built.keys.push_back(key);
        }
        return built;
    }();
    return trust;
}

// A folder of a test's own, removed with it. Named by the test, since ctest
// runs the cases of a suite side by side, and numbered, since a test may
// have two.
class Scratch {
  public:
    Scratch()
    {
        static int made = 0;
        const ::testing::TestInfo* test = ::testing::UnitTest::GetInstance()->current_test_info();
        path_ = fs::temp_directory_path() /
                ("katana-update-verbs-" + std::string(test != nullptr ? test->name() : "none") +
                 "-" + std::to_string(++made));
        std::error_code error;
        fs::remove_all(path_, error);
        fs::create_directories(path_, error);
    }
    ~Scratch()
    {
        std::error_code error;
        fs::remove_all(path_, error);
    }
    Scratch(const Scratch&) = delete;
    Scratch& operator=(const Scratch&) = delete;

    [[nodiscard]] const fs::path& path() const { return path_; }

  private:
    fs::path path_;
};

// A feed: addresses to bodies or failures, anything else a 404 as on the real
// host; a range of a body when a range is asked for. Every request is kept.
class FakeFeed {
  public:
    void serve(const std::string& url, std::string body)
    {
        bodies_.insert_or_assign(url, std::move(body));
        failures_.erase(url);
    }
    void fail(const std::string& url, core::Error error)
    {
        failures_.insert_or_assign(url, std::move(error));
    }

    [[nodiscard]] update::Fetch fetch()
    {
        return [this](const update::FetchRequest& request, std::stop_token,
                      update::ProgressFn progress) -> core::Result<std::string> {
            requests.push_back(request);
            if (const auto failure = failures_.find(request.url); failure != failures_.end()) {
                return failure->second;
            }
            const auto body = bodies_.find(request.url);
            if (body == bodies_.end()) {
                return core::makeError(core::ErrorCode::NotFound, "nothing at this address",
                                       "status=404; " + request.url);
            }
            if (request.rangeLength == 0) {
                return body->second;
            }
            if (request.rangeStart + request.rangeLength > body->second.size()) {
                return core::makeError(core::ErrorCode::FileImportFailure,
                                       "a range past the end of the file", "status=416");
            }
            std::string part = body->second.substr(request.rangeStart, request.rangeLength);
            if (progress) {
                progress(part.size() / 2, part.size());
                progress(part.size(), part.size());
            }
            return part;
        };
    }

    [[nodiscard]] std::vector<update::FetchRequest> requestsFor(const std::string& url) const
    {
        std::vector<update::FetchRequest> found;
        for (const update::FetchRequest& request : requests) {
            if (request.url == url) {
                found.push_back(request);
            }
        }
        return found;
    }

    std::vector<update::FetchRequest> requests;

  private:
    std::map<std::string, std::string> bodies_;
    std::map<std::string, core::Error> failures_;
};

core::Error httpStatus(core::ErrorCode code, int number)
{
    return core::makeError(code, "the server said " + std::to_string(number),
                           "status=" + std::to_string(number) + "; https://github.com/x");
}

class FakeStore final : public update::CredentialStore {
  public:
    [[nodiscard]] core::Result<std::string> read() const override
    {
        if (readFailure.has_value()) {
            return *readFailure;
        }
        if (token.empty()) {
            return core::makeError(core::ErrorCode::NotFound, "no token is stored", place);
        }
        return token;
    }
    [[nodiscard]] core::Status write(std::string_view value) override
    {
        token = std::string(value);
        return {};
    }
    [[nodiscard]] core::Status clear() override
    {
        ++clears;
        if (clearFailure.has_value()) {
            return *clearFailure;
        }
        token.clear();
        return {};
    }
    [[nodiscard]] std::string_view where() const override { return place; }

    std::string token;
    std::string place = "test store";
    std::optional<core::Error> readFailure;
    std::optional<core::Error> clearFailure;
    int clears = 0;
};

// Starts nothing: it keeps what it was asked to run, and says the helper's
// self-test ended as `exitCode`.
class FakeLauncher final : public update::Launcher {
  public:
    [[nodiscard]] core::Result<update::SpawnResult>
    run(const update::SpawnRequest& request) override
    {
        runs.push_back(request);
        update::SpawnResult result;
        result.exitCode = exitCode;
        return result;
    }
    [[nodiscard]] core::Status startDetached(const update::SpawnRequest& request) override
    {
        detached.push_back(request);
        return {};
    }

    std::vector<update::SpawnRequest> runs;
    std::vector<update::SpawnRequest> detached;
    int exitCode = 0;
};

// A v1 install in a scratch folder, an empty state folder beside it, and a
// feed that serves nothing until told to.
struct Rig {
    Rig() : root(scratch.path() / "R"), state(scratch.path() / "U")
    {
        fs::copy(fixtures() / "install" / "v1", root, fs::copy_options::recursive);
        install.root = root;
        install.target = "win64";
        install.kind = update::InstallKind::Archive;
        install.version = update::parseVersion("0.9.0").value();
        install.sequence = kV1Sequence;
        install.hasInstalledRecord = true;
        feedConfig.repo = "kjarada/Katana";
        feedConfig.target = "win64";
        feedConfig.userAgent = "Katana/0.9.0 (win64)";
    }

    // The v2 release on the feed: its signature, its manifest and its payload.
    void serveV2()
    {
        const fs::path release = fixtures() / "release" / "v2";
        feed.serve(kSignatureUrl, bytesOf(release / "Katana-update-win64.json.sig"));
        feed.serve(kManifestUrl, bytesOf(release / "Katana-update-win64.json"));
        feed.serve(kPayloadUrl, bytesOf(release / "Katana-0.10.0-win64.update.zip"));
    }

    // 0.10.0 as the helper recorded it: what the v2 release is not newer than.
    void installV2()
    {
        install.version = update::parseVersion("0.10.0").value();
        install.sequence = kV2Sequence;
    }

    // What makeUpdateContext makes for `surface`, over the fakes: a state
    // folder for katana_cli and the window, none for a headless window or
    // katana_mcp.
    [[nodiscard]] UpdateContext context(UpdateSurface surface = UpdateSurface::Cli)
    {
        UpdateContext made;
        made.surface = surface;
        made.install = install;
        made.trust = &testKeys();
        made.feed = feedConfig;
        made.fetch = feed.fetch();
        if (surface == UpdateSurface::Cli || surface == UpdateSurface::Window) {
            made.stateDirectory = state;
        }
        made.credentials = &store;
        made.launcher = &launcher;
        made.nowEpoch = kNow;
        return made;
    }

    [[nodiscard]] fs::path stageDirectory() const
    {
        return state / ("stage-" + std::to_string(kV2Sequence));
    }

    Scratch scratch;
    fs::path root;
    fs::path state;
    update::Install install;
    update::FeedConfig feedConfig;
    FakeFeed feed;
    FakeStore store;
    FakeLauncher launcher;
};

// The reply's text, or the refusal's whole description.
std::string said(const core::Result<UpdateReply>& reply)
{
    return reply.ok() ? reply->text : "error: " + reply.error().describe();
}

// The first line of a reply that was given.
std::string firstLineOf(const core::Result<UpdateReply>& reply)
{
    const std::vector<std::string> lines = linesOf(said(reply));
    return lines.empty() ? std::string() : lines.front();
}

bool contains(const std::string& text, std::string_view words)
{
    return text.find(words) != std::string::npos;
}

bool startsWith(const std::string& text, std::string_view words)
{
    return std::string_view(text).starts_with(words);
}

// std::cout and std::cerr into strings while it lives: what a session prints.
class Captured {
  public:
    Captured() : cout_(std::cout.rdbuf(out_.rdbuf())), cerr_(std::cerr.rdbuf(err_.rdbuf())) {}
    ~Captured()
    {
        std::cout.rdbuf(cout_);
        std::cerr.rdbuf(cerr_);
    }
    Captured(const Captured&) = delete;
    Captured& operator=(const Captured&) = delete;

    [[nodiscard]] std::string out() const { return out_.str(); }
    [[nodiscard]] std::string err() const { return err_.str(); }

  private:
    std::ostringstream out_;
    std::ostringstream err_;
    std::streambuf* cout_;
    std::streambuf* cerr_;
};

// The v2 release downloaded and ready in the update folder `state`, made by
// hand: the two records stage() writes last (update/stage.hpp) are all that
// says a stage is complete, and they are the feed's own signed bytes. No file
// of the release is beside them, and none is looked for by the lines that
// only say what is staged.
void putACompleteStageOfV2In(const fs::path& state)
{
    const fs::path release = fixtures() / "release" / "v2";
    const fs::path stage = state / ("stage-" + std::to_string(kV2Sequence));
    writeBytes(update::stagedManifestPath(stage), bytesOf(release / "Katana-update-win64.json"));
    writeBytes(update::stagedSignaturePath(stage),
               bytesOf(release / "Katana-update-win64.json.sig"));
}

// The two records of an install in `root` (update/install.hpp): install.json
// as packaging writes it, and installed.json as the helper does for 0.9.0.
void writeInstallRecords(const fs::path& root)
{
    const fs::path records = root / "share" / "katana";
    writeBytes(records / "install.json", "{\"schema\":1,\"target\":\"win64\"}\n");
    writeBytes(records / "installed.json",
               "{\"schema\":1,\"product\":\"katana\",\"version\":\"0.9.0\","
               "\"sequence\":1790000000,\"tag\":\"v0.9.0\",\"target\":\"win64\"}\n");
}

// A katana_cli session whose UPDATE lines run on the rig instead of on this
// test program's own install (Session::setUpdateContextSource). It starts
// with no customisation, so that it prints nothing of one.
struct CliSession {
    explicit CliSession(Rig& rig)
    {
        session.setUpdateContextSource([&rig, this](UpdateSurface surface) {
            asked.push_back(surface);
            return rig.context(surface);
        });
    }

    katana::app::tests::StartEnvironment environment{"none", nullptr};
    katana::app::Session session{"katana_cli"};
    std::vector<UpdateSurface> asked; // the surface each context was asked for
};

// ---- the line -----------------------------------------------------------------------------------

TEST(UpdateVerbs, ALineIsAnUpdateLineByItsFirstWordInAnyCase)
{
    for (const char* line : {"UPDATE CHECK", "update status", "  Update  DOWNLOAD CONFIRM",
                             "\tUPDATE", "UPDATE nonsense"}) {
        EXPECT_TRUE(katana::app::isUpdateLine(line)) << line;
    }
    for (const char* line : {"", "   ", "UPDATES CHECK", "UPDATECHECK", "LINE 0,0 1,1",
                             "HELP UPDATE", "# UPDATE CHECK", "CHECK UPDATE"}) {
        EXPECT_FALSE(katana::app::isUpdateLine(line)) << line;
    }
}

TEST(UpdateVerbs, LinesTheGrammarDoesNotTakeAreInvalidArgumentsAndDoNothing)
{
    Rig rig;
    rig.serveV2();
    const UpdateContext context = rig.context();
    for (const char* line :
         {"UPDATE", "UPDATE NOW", "UPDATE CHECK NOW", "UPDATE STATUS JSON",
          "UPDATE DOWNLOAD CONFIRM NOW", "UPDATE DOWNLOAD YES", "UPDATE APPLY NOW",
          "UPDATE SKIP", "UPDATE SKIP 1", "UPDATE SKIP 1.2.3.4.5", "UPDATE SKIP v0.10.0",
          "UPDATE SKIP 0.10.0 0.11.0", "UPDATE SET", "UPDATE SET AUTO", "UPDATE SET AUTO MAYBE",
          "UPDATE SET SPEED ON", "UPDATE SET AUTO ON NOW", "UPDATE TOKEN",
          "UPDATE TOKEN STATUS NOW", "LINE 0,0 1,1"}) {
        const core::Result<UpdateReply> reply = runUpdateLine(context, line);
        ASSERT_FALSE(reply.ok()) << line << " -> " << said(reply);
        EXPECT_EQ(reply.error().code, core::ErrorCode::InvalidArgument) << line;
    }
    EXPECT_TRUE(rig.feed.requests.empty()) << "a line that is refused asks nobody anything";
    EXPECT_FALSE(fs::exists(rig.state)) << "and keeps nothing";
    EXPECT_EQ(rig.store.clears, 0);
}

TEST(UpdateVerbs, ALineWithoutItsConfirmIsRefusedSayingWhatTheLineWithItDoes)
{
    Rig rig;
    rig.serveV2();
    for (const UpdateSurface surface : {UpdateSurface::Cli, UpdateSurface::Window}) {
        const UpdateContext context = rig.context(surface);

        const core::Result<UpdateReply> download = runUpdateLine(context, "UPDATE DOWNLOAD");
        ASSERT_FALSE(download.ok());
        EXPECT_EQ(download.error().code, core::ErrorCode::InvalidArgument);
        EXPECT_TRUE(contains(download.error().message, "nothing was downloaded"));
        EXPECT_TRUE(contains(download.error().message, "UPDATE DOWNLOAD CONFIRM asks"));
        EXPECT_TRUE(contains(download.error().message, "downloads the files"));

        const core::Result<UpdateReply> apply = runUpdateLine(context, "update apply");
        ASSERT_FALSE(apply.ok());
        EXPECT_EQ(apply.error().code, core::ErrorCode::InvalidArgument);
        EXPECT_TRUE(contains(apply.error().message, "nothing was changed"));
        EXPECT_TRUE(contains(apply.error().message, "UPDATE APPLY CONFIRM ends this session"));
        EXPECT_TRUE(contains(apply.error().message, "replace the installed files"));
    }
    EXPECT_TRUE(rig.feed.requests.empty());
    EXPECT_TRUE(rig.launcher.runs.empty());
    EXPECT_FALSE(fs::exists(rig.state));
}

// ---- UPDATE CHECK -------------------------------------------------------------------------------

TEST(UpdateVerbs, CheckOffersANewerReleaseWithWhatItWouldFetchAndRecordsWhenItAsked)
{
    Rig rig;
    rig.serveV2();
    const core::Result<UpdateReply> reply = runUpdateLine(rig.context(), "UPDATE CHECK");
    ASSERT_TRUE(reply.ok()) << said(reply);
    EXPECT_EQ(reply->text, kAvailable);

    ASSERT_TRUE(reply->offer.has_value());
    EXPECT_EQ(reply->offer->check.status, update::CheckStatus::Available);
    EXPECT_EQ(reply->offer->check.installedVersion, "0.9.0");
    EXPECT_EQ(reply->offer->check.latestVersion, "0.10.0");
    EXPECT_EQ(reply->offer->changedFiles, 5u);
    EXPECT_EQ(reply->offer->totalFiles, 10u);
    EXPECT_EQ(reply->offer->downloadBytes, 339u);
    EXPECT_FALSE(reply->status.has_value());
    EXPECT_FALSE(reply->apply.has_value());

    // The signature, then the manifest, and nothing of the payload: a check
    // downloads no file of the release.
    ASSERT_EQ(rig.feed.requests.size(), 2u);
    EXPECT_EQ(rig.feed.requests[0].url, kSignatureUrl);
    EXPECT_EQ(rig.feed.requests[1].url, kManifestUrl);

    const core::Result<update::UpdateState> state = update::readState(rig.state);
    ASSERT_TRUE(state.ok());
    EXPECT_EQ(state->lastCheckEpoch, kNow);
    EXPECT_FALSE(fs::exists(rig.stageDirectory()));
}

TEST(UpdateVerbs, CheckIsAskedInAnyCaseAndWithBlanksAround)
{
    Rig rig;
    rig.serveV2();
    const core::Result<UpdateReply> reply = runUpdateLine(rig.context(), "  update   check ");
    ASSERT_TRUE(reply.ok()) << said(reply);
    EXPECT_EQ(reply->text, kAvailable);
}

TEST(UpdateVerbs, CheckSaysCurrentWhenTheFeedHoldsWhatIsInstalled)
{
    Rig rig;
    rig.serveV2();
    rig.installV2();
    const core::Result<UpdateReply> reply = runUpdateLine(rig.context(), "UPDATE CHECK");
    ASSERT_TRUE(reply.ok()) << said(reply);
    // Nothing would be fetched, so nothing is said of files or bytes.
    EXPECT_EQ(reply->text, "update=current installed=0.10.0 latest=0.10.0 notes_url=" + kNotes);
    ASSERT_TRUE(reply->offer.has_value());
    EXPECT_EQ(reply->offer->check.status, update::CheckStatus::Current);
    EXPECT_EQ(reply->offer->changedFiles, 0u);

    // The feed answered, so this was a check all the same.
    const core::Result<update::UpdateState> state = update::readState(rig.state);
    ASSERT_TRUE(state.ok());
    EXPECT_EQ(state->lastCheckEpoch, kNow);
}

TEST(UpdateVerbs, CheckSaysUnreachableWithTheTransportsReasonAndRecordsNoCheck)
{
    struct Failure {
        const char* what;
        core::Error error;
        std::string reply; // after "update=unreachable installed=0.9.0 "
    };
    // Each as update/feed.hpp says a transport reports it, with the token it
    // is read as (fetchFailureReason) and its own sentence as the detail.
    const std::vector<Failure> failures = {
        {"a private repository or no release (404)", httpStatus(core::ErrorCode::NotFound, 404),
         "reason=http_404 detail=\"the server said 404\""},
        {"the rate limit (403)", httpStatus(core::ErrorCode::FileImportFailure, 403),
         "reason=http_403 detail=\"the server said 403\""},
        {"too many requests (429)", httpStatus(core::ErrorCode::FileImportFailure, 429),
         "reason=http_429 detail=\"the server said 429\""},
        {"the host in trouble (503)", httpStatus(core::ErrorCode::FileImportFailure, 503),
         "reason=http_503 detail=\"the server said 503\""},
        {"no network",
         core::makeError(core::ErrorCode::FileImportFailure, "the host name could not be found",
                         "status=0; https://github.com/x"),
         "reason=offline detail=\"the host name could not be found\""},
        {"a captive portal's page",
         core::makeError(core::ErrorCode::FileImportFailure, "a web page instead of the file",
                         "status=200; reason=html_page; https://github.com/x"),
         "reason=html_page detail=\"a web page instead of the file\""},
        {"a redirect out of https",
         core::makeError(core::ErrorCode::FileImportFailure, "redirected to no https address",
                         "status=302; reason=bad_redirect; https://github.com/x"),
         "reason=bad_redirect detail=\"redirected to no https address\""},
        {"a body past the engine's bound",
         core::makeError(core::ErrorCode::Unsupported, "longer than allowed", "status=200"),
         "reason=too_large detail=\"longer than allowed\""},
        {"a transfer that was cancelled",
         core::makeError(core::ErrorCode::InvalidState, "cancelled"),
         "reason=cancelled detail=cancelled"},
    };
    for (const Failure& failure : failures) {
        Rig rig;
        rig.serveV2();
        rig.feed.fail(kSignatureUrl, failure.error);
        const core::Result<UpdateReply> reply = runUpdateLine(rig.context(), "UPDATE CHECK");
        ASSERT_TRUE(reply.ok()) << failure.what << ": a feed that cannot be read is a reply, "
                                << said(reply);
        EXPECT_EQ(reply->text, "update=unreachable installed=0.9.0 " + failure.reply)
            << failure.what;
        ASSERT_TRUE(reply->offer.has_value()) << failure.what;
        EXPECT_EQ(reply->offer->check.status, update::CheckStatus::Unreachable) << failure.what;
        EXPECT_EQ(rig.feed.requests.size(), 1u) << failure.what << ": the signature gates the rest";
        EXPECT_FALSE(fs::exists(rig.state / "state.json"))
            << failure.what << ": nothing answered, so no check is recorded";
    }
}

TEST(UpdateVerbs, CheckSaysUnreachableWhenTheSignatureIsNotTheReleaseKeys)
{
    // tests/update/fixtures/release/bad/wrong-key: v2, signed by a key the
    // trust does not hold.
    Rig rig;
    const fs::path release = fixtures() / "release" / "bad" / "wrong-key";
    rig.feed.serve(kSignatureUrl, bytesOf(release / "Katana-update-win64.json.sig"));
    rig.feed.serve(kManifestUrl, bytesOf(release / "Katana-update-win64.json"));
    const core::Result<UpdateReply> reply = runUpdateLine(rig.context(), "UPDATE CHECK");
    ASSERT_TRUE(reply.ok()) << said(reply);
    EXPECT_TRUE(startsWith(firstLineOf(reply),
                           "update=unreachable installed=0.9.0 reason=untrusted_key"))
        << said(reply);
    EXPECT_FALSE(contains(reply->text, "latest=")) << "nothing of an unverified manifest is said";
    EXPECT_FALSE(fs::exists(rig.state / "state.json"));
}

TEST(UpdateVerbs, CheckWithNoTransportSaysSoAndAsksNobody)
{
    Rig rig;
    rig.serveV2();
    UpdateContext context = rig.context();
    context.fetch = {};
    const core::Result<UpdateReply> reply = runUpdateLine(context, "UPDATE CHECK");
    ASSERT_TRUE(reply.ok()) << said(reply);
    EXPECT_TRUE(startsWith(reply->text,
                           "update=unreachable installed=0.9.0 reason=no_transport detail="))
        << reply->text;
    EXPECT_TRUE(rig.feed.requests.empty());
    EXPECT_FALSE(fs::exists(rig.state));
}

TEST(UpdateVerbs, ABuildWithNoNetworkIsACopyThatCannotUpdate)
{
    // What the transport of a build without an HTTP stack answers every
    // request with (src/katana_app/update_transport.hpp): Unsupported, and the
    // token no_network. No server can cure that, so the copy is unsupported,
    // not the feed unreachable.
    Rig rig;
    UpdateContext context = rig.context();
    context.fetch = [](const update::FetchRequest&, std::stop_token,
                       update::ProgressFn) -> core::Result<std::string> {
        return core::makeError(core::ErrorCode::Unsupported,
                               "this build of Katana has no network access", "reason=no_network");
    };
    const core::Result<UpdateReply> reply = runUpdateLine(context, "UPDATE CHECK");
    ASSERT_TRUE(reply.ok()) << said(reply);
    EXPECT_EQ(reply->text, "update=unsupported installed=0.9.0 reason=no_network "
                           "detail=\"this build of Katana has no network access\"");
    ASSERT_TRUE(reply->offer.has_value());
    EXPECT_EQ(reply->offer->check.status, update::CheckStatus::Unsupported);
}

TEST(UpdateVerbs, CheckDoesNotOfferAReleaseThatWasRolledBackHere)
{
    Rig rig;
    rig.serveV2();
    update::UpdateState state;
    state.blockedSequences = {kV2Sequence};
    ASSERT_TRUE(update::writeState(rig.state, state).ok());

    const core::Result<UpdateReply> reply = runUpdateLine(rig.context(), "UPDATE CHECK");
    ASSERT_TRUE(reply.ok()) << said(reply);
    EXPECT_EQ(reply->text, "update=blocked installed=0.9.0 latest=0.10.0 notes_url=" + kNotes +
                               " reason=blocked_before");
    ASSERT_TRUE(reply->offer.has_value());
    EXPECT_EQ(reply->offer->check.status, update::CheckStatus::Blocked);

    // Recording the check kept what the state already held.
    const core::Result<update::UpdateState> after = update::readState(rig.state);
    ASSERT_TRUE(after.ok());
    EXPECT_EQ(after->blockedSequences, std::vector<std::uint64_t>{kV2Sequence});
    EXPECT_EQ(after->lastCheckEpoch, kNow);
}

TEST(UpdateVerbs, CheckDoesNotOfferASkippedVersionUntilTheSkipIsTakenBack)
{
    Rig rig;
    rig.serveV2();
    const UpdateContext context = rig.context();

    const core::Result<UpdateReply> skipped = runUpdateLine(context, "UPDATE SKIP 0.10.0");
    ASSERT_TRUE(skipped.ok()) << said(skipped);
    EXPECT_EQ(skipped->text, "skipped=0.10.0");

    const core::Result<UpdateReply> checked = runUpdateLine(context, "UPDATE CHECK");
    ASSERT_TRUE(checked.ok()) << said(checked);
    EXPECT_EQ(checked->text, "update=current installed=0.9.0 latest=0.10.0 notes_url=" + kNotes +
                                 " reason=skipped");
    ASSERT_TRUE(checked->offer.has_value());
    EXPECT_EQ(checked->offer->check.status, update::CheckStatus::Current);

    const core::Result<UpdateReply> taken = runUpdateLine(context, "update skip none");
    ASSERT_TRUE(taken.ok()) << said(taken);
    EXPECT_EQ(taken->text, "skipped=none");
    const core::Result<UpdateReply> again = runUpdateLine(context, "UPDATE CHECK");
    ASSERT_TRUE(again.ok()) << said(again);
    EXPECT_EQ(again->text, kAvailable);
}

TEST(UpdateVerbs, AVersionIsSkippedAsAVersionNotAsText)
{
    // 0.10 and 0.10.0 are one release (update/version.hpp).
    Rig rig;
    rig.serveV2();
    const UpdateContext context = rig.context();
    const core::Result<UpdateReply> skipped = runUpdateLine(context, "UPDATE SKIP 0.10");
    ASSERT_TRUE(skipped.ok()) << said(skipped);
    EXPECT_EQ(skipped->text, "skipped=0.10");
    const core::Result<UpdateReply> checked = runUpdateLine(context, "UPDATE CHECK");
    ASSERT_TRUE(checked.ok()) << said(checked);
    EXPECT_TRUE(contains(checked->text, "update=current")) << checked->text;
    EXPECT_TRUE(contains(checked->text, "reason=skipped")) << checked->text;

    // Another version skipped hides nothing of this one.
    ASSERT_TRUE(runUpdateLine(context, "UPDATE SKIP 0.11.0").ok());
    const core::Result<UpdateReply> other = runUpdateLine(context, "UPDATE CHECK");
    ASSERT_TRUE(other.ok()) << said(other);
    EXPECT_EQ(other->text, kAvailable);
}

TEST(UpdateVerbs, CheckSaysWhenTheReleasesInformationIsOldAndOffersItAllTheSame)
{
    // v2's `expires` is 2027-04-16T08:40:00Z. 1900000000 is in 2030.
    Rig rig;
    rig.serveV2();
    UpdateContext context = rig.context();
    context.nowEpoch = 1900000000;
    const core::Result<UpdateReply> reply = runUpdateLine(context, "UPDATE CHECK");
    ASSERT_TRUE(reply.ok()) << said(reply);
    EXPECT_EQ(reply->text, kAvailable + " information=old");
}

TEST(UpdateVerbs, ANotifyOnlyReleaseIsAvailableAndHasNothingToFetch)
{
    // tests/update/fixtures/release/v2-macos-notify: 0.10.0 announced for a
    // target whose copies are replaced by hand. It lists no file.
    Rig rig;
    rig.install.target = "macos-arm64";
    rig.feedConfig.target = "macos-arm64";
    const fs::path release = fixtures() / "release" / "v2-macos-notify";
    const std::string base = "https://github.com/kjarada/Katana/releases/latest/download/";
    rig.feed.serve(base + "Katana-update-macos-arm64.json.sig",
                   bytesOf(release / "Katana-update-macos-arm64.json.sig"));
    rig.feed.serve(base + "Katana-update-macos-arm64.json",
                   bytesOf(release / "Katana-update-macos-arm64.json"));
    const core::Result<UpdateReply> reply = runUpdateLine(rig.context(), "UPDATE CHECK");
    ASSERT_TRUE(reply.ok()) << said(reply);
    EXPECT_EQ(reply->text, "update=available installed=0.9.0 latest=0.10.0 notes_url=" + kNotes +
                               " reason=notify_only");
    ASSERT_TRUE(reply->offer.has_value());
    EXPECT_EQ(reply->offer->check.status, update::CheckStatus::Available);
    EXPECT_EQ(reply->offer->totalFiles, 0u);
}

TEST(UpdateVerbs, ADevelopmentCopyIsNeverAskedAboutAndKeepsNothing)
{
    // A copy packaging did not write: no install.json, so no target and no
    // kind but development. The feed would offer v2; it is never asked.
    Rig rig;
    rig.serveV2();
    rig.install = update::Install{};
    rig.install.version = update::parseVersion("0.9.0").value();
    const UpdateContext context = rig.context();

    const core::Result<UpdateReply> checked = runUpdateLine(context, "UPDATE CHECK");
    ASSERT_TRUE(checked.ok()) << said(checked);
    EXPECT_EQ(checked->text.rfind("update=unsupported installed=0.9.0 reason=development_tree "
                                  "detail=",
                                  0),
              0u)
        << checked->text;
    ASSERT_TRUE(checked->offer.has_value());
    EXPECT_EQ(checked->offer->check.status, update::CheckStatus::Unsupported);
    EXPECT_TRUE(rig.feed.requests.empty()) << "a build tree stays offline";
    EXPECT_FALSE(fs::exists(rig.state)) << "and a check that asked nobody is not recorded";

    const core::Result<UpdateReply> status = runUpdateLine(context, "UPDATE STATUS");
    ASSERT_TRUE(status.ok()) << said(status);
    EXPECT_EQ(status->text, "installed=0.9.0 target=none kind=development staged=none "
                            "skipped=none auto=on last_check=never token=absent last=none");
}

TEST(UpdateVerbs, AnInstallRecordThatDoesNotReadIsSaidAndTheCopyIsNotUpdated)
{
    Rig rig;
    rig.serveV2();
    UpdateContext context = rig.context();
    context.install = update::Install{};
    context.install.version = update::parseVersion("0.9.0").value();
    context.installProblem = "ParseFailure: install.json is not a record";
    const core::Result<UpdateReply> reply = runUpdateLine(context, "UPDATE CHECK");
    ASSERT_TRUE(reply.ok()) << said(reply);
    EXPECT_EQ(reply->text, "update=unsupported installed=0.9.0 reason=install_record "
                           "detail=\"ParseFailure: install.json is not a record\"");
    EXPECT_TRUE(rig.feed.requests.empty());

    // STATUS has no word in kind= for such a copy: its record says what the
    // context holds, and a warning after it says that this is not what the
    // copy is. Without it the copy was called a development build and no more.
    const core::Result<UpdateReply> status = runUpdateLine(context, "UPDATE STATUS");
    ASSERT_TRUE(status.ok()) << said(status);
    const std::vector<std::string> lines = linesOf(status->text);
    ASSERT_EQ(lines.size(), 2u) << status->text;
    EXPECT_EQ(lines[0], "installed=0.9.0 target=none kind=development staged=none skipped=none "
                        "auto=on last_check=never token=absent last=none");
    EXPECT_EQ(lines[1].rfind("warning text=\"this copy's install record could not be read", 0),
              0u)
        << lines[1];
    EXPECT_TRUE(contains(lines[1], "ParseFailure: install.json is not a record")) << lines[1];
    EXPECT_FALSE(fs::exists(rig.state));
}

TEST(UpdateVerbs, CheckOffersAReleaseWhoseFilesItCouldNotCompareAndSaysThatItCouldNot)
{
    // update/plan.hpp: a folder where a file of the release goes is an error
    // that names the file, not a change. The release is known all the same;
    // what it would fetch is not, so the record has no files= and no
    // download_bytes= (update_verbs.hpp) and a warning says why.
    Rig rig;
    rig.serveV2();
    const fs::path readme = rig.root / "share" / "katana" / "readme.txt";
    ASSERT_TRUE(fs::remove(readme));
    ASSERT_TRUE(fs::create_directory(readme));

    const core::Result<UpdateReply> reply = runUpdateLine(rig.context(), "UPDATE CHECK");
    ASSERT_TRUE(reply.ok()) << said(reply);
    const std::vector<std::string> lines = linesOf(reply->text);
    ASSERT_EQ(lines.size(), 2u) << reply->text;
    EXPECT_EQ(lines[0], "update=available installed=0.9.0 latest=0.10.0 notes_url=" + kNotes);
    EXPECT_EQ(lines[1].rfind("warning text=\"what the update would change could not be worked out",
                             0),
              0u)
        << lines[1];
    EXPECT_TRUE(contains(lines[1], "share/katana/readme.txt")) << lines[1];
    ASSERT_TRUE(reply->offer.has_value());
    EXPECT_EQ(reply->offer->check.status, update::CheckStatus::Available);
    EXPECT_EQ(reply->offer->totalFiles, 0u);
    EXPECT_EQ(reply->offer->downloadBytes, 0u);

    // The feed answered, so it was a check.
    const core::Result<update::UpdateState> state = update::readState(rig.state);
    ASSERT_TRUE(state.ok());
    EXPECT_EQ(state->lastCheckEpoch, kNow);
}

TEST(UpdateVerbs, ACheckThatWasCancelledBeforeItBeganIsAnErrorAndAsksNobody)
{
    // update/check.hpp: the call's own error is for a check that could not be
    // made at all, and a stop already asked for is one; `cancelled` is what
    // the engine calls it everywhere (plan.hpp, stage.hpp).
    Rig rig;
    rig.serveV2();
    std::stop_source cancelled;
    cancelled.request_stop();
    UpdateContext context = rig.context();
    context.stop = cancelled.get_token();

    const core::Result<UpdateReply> reply = runUpdateLine(context, "UPDATE CHECK");
    ASSERT_FALSE(reply.ok()) << said(reply);
    EXPECT_EQ(reply.error().code, core::ErrorCode::InvalidState);
    EXPECT_EQ(reply.error().message, "cancelled");
    EXPECT_TRUE(rig.feed.requests.empty());
    EXPECT_FALSE(fs::exists(rig.state)) << "a check that asked nobody is not recorded";

    // A status asks nobody, so there is nothing of it to cancel.
    const core::Result<UpdateReply> status = runUpdateLine(context, "UPDATE STATUS");
    ASSERT_TRUE(status.ok()) << said(status);
    EXPECT_EQ(status->text, kFreshStatus);
}

TEST(UpdateVerbs, AStateFileThatDoesNotReadIsSaidAndIsNotWrittenOver)
{
    Rig rig;
    rig.serveV2();
    const std::string damaged = "this is not a state file";
    writeBytes(rig.state / "state.json", damaged);
    const UpdateContext context = rig.context();

    const core::Result<UpdateReply> checked = runUpdateLine(context, "UPDATE CHECK");
    ASSERT_TRUE(checked.ok()) << said(checked);
    const std::vector<std::string> lines = linesOf(checked->text);
    ASSERT_EQ(lines.size(), 2u) << checked->text;
    EXPECT_EQ(lines[0], kAvailable) << "the defaults are used: nothing is skipped or blocked";
    EXPECT_EQ(lines[1].rfind("warning text=\"the update settings could not be read", 0), 0u)
        << lines[1];
    EXPECT_EQ(bytesOf(rig.state / "state.json"), damaged);

    const core::Result<UpdateReply> status = runUpdateLine(context, "UPDATE STATUS");
    ASSERT_TRUE(status.ok()) << said(status);
    const std::vector<std::string> statusLines = linesOf(status->text);
    ASSERT_EQ(statusLines.size(), 2u) << status->text;
    EXPECT_EQ(statusLines[0], kFreshStatus);
    EXPECT_EQ(statusLines[1].rfind("warning text=", 0), 0u);

    for (const char* line : {"UPDATE SKIP 0.10.0", "UPDATE SET AUTO OFF"}) {
        const core::Result<UpdateReply> refused = runUpdateLine(context, line);
        ASSERT_FALSE(refused.ok()) << line << " -> " << said(refused);
        EXPECT_EQ(refused.error().code, core::ErrorCode::InvalidState) << line;
    }
    EXPECT_EQ(bytesOf(rig.state / "state.json"), damaged);
}

// ---- UPDATE STATUS ------------------------------------------------------------------------------

TEST(UpdateVerbs, StatusSaysWhatThisCopyIsAndWhatIsKept)
{
    Rig rig;
    rig.serveV2();
    const UpdateContext context = rig.context();

    const core::Result<UpdateReply> fresh = runUpdateLine(context, "UPDATE STATUS");
    ASSERT_TRUE(fresh.ok()) << said(fresh);
    EXPECT_EQ(fresh->text, kFreshStatus);
    ASSERT_TRUE(fresh->status.has_value());
    EXPECT_EQ(fresh->status->installedVersion, "0.9.0");
    EXPECT_EQ(fresh->status->target, "win64");
    EXPECT_EQ(fresh->status->kind, update::InstallKind::Archive);
    EXPECT_TRUE(fresh->status->stagedVersion.empty());
    EXPECT_TRUE(fresh->status->state.automatic);
    EXPECT_FALSE(fresh->status->tokenPresent);
    EXPECT_FALSE(fresh->status->last.has_value());
    EXPECT_FALSE(fresh->offer.has_value());
    EXPECT_TRUE(rig.feed.requests.empty()) << "a status asks nobody";
    EXPECT_FALSE(fs::exists(rig.state)) << "and writes nothing";

    // Each thing kept, changed by its own line or by what writes it, and read back.
    ASSERT_TRUE(runUpdateLine(context, "UPDATE CHECK").ok());
    ASSERT_TRUE(runUpdateLine(context, "UPDATE SKIP 0.10.0").ok());
    ASSERT_TRUE(runUpdateLine(context, "UPDATE SET AUTO OFF").ok());
    rig.store.token = std::string(kSecret);
    update::LastResult last;
    last.status = "rolled_back";
    last.reason = "health_exit";
    last.version = "0.10.0";
    ASSERT_TRUE(update::writeLastResult(rig.state, last).ok());

    const core::Result<UpdateReply> kept = runUpdateLine(context, "update status");
    ASSERT_TRUE(kept.ok()) << said(kept);
    EXPECT_EQ(kept->text, std::string("installed=0.9.0 target=win64 kind=archive staged=none "
                                      "skipped=0.10.0 auto=off last_check=") +
                              kNowText + " token=present last=rolled_back:health_exit");
    ASSERT_TRUE(kept->status.has_value());
    EXPECT_EQ(kept->status->state.skippedVersion, "0.10.0");
    EXPECT_FALSE(kept->status->state.automatic);
    EXPECT_EQ(kept->status->state.lastCheckEpoch, kNow);
    EXPECT_TRUE(kept->status->tokenPresent);
    ASSERT_TRUE(kept->status->last.has_value());
    EXPECT_EQ(kept->status->last->status, "rolled_back");

    // Reading how the last update ended does not clear it: the window shows
    // it once and clears it itself.
    EXPECT_TRUE(fs::exists(rig.state / "last.json"));
}

TEST(UpdateVerbs, StatusSaysAnInstallerKindAndALastUpdateThatWentThrough)
{
    Rig rig;
    rig.install.kind = update::InstallKind::Installer;
    update::LastResult last;
    last.status = "committed";
    last.version = "0.9.0";
    ASSERT_TRUE(update::writeLastResult(rig.state, last).ok());
    const core::Result<UpdateReply> reply = runUpdateLine(rig.context(), "UPDATE STATUS");
    ASSERT_TRUE(reply.ok()) << said(reply);
    EXPECT_EQ(reply->text, "installed=0.9.0 target=win64 kind=installer staged=none skipped=none "
                           "auto=on last_check=never token=absent last=committed");
}

TEST(UpdateVerbs, WhatIsDownloadedAndHowTheLastUpdateEndedAreAnInstallsAndNoDevelopmentCopys)
{
    // One update folder, as one user has: the v2 release complete in it, and
    // the record of an update that was put back.
    Rig rig;
    putACompleteStageOfV2In(rig.state);
    update::LastResult last;
    last.status = "rolled_back";
    last.reason = "health_exit";
    last.version = "0.10.0";
    ASSERT_TRUE(update::writeLastResult(rig.state, last).ok());

    // The installed copy says both: the release is above what it has.
    const core::Result<UpdateReply> installed = runUpdateLine(rig.context(), "UPDATE STATUS");
    ASSERT_TRUE(installed.ok()) << said(installed);
    EXPECT_EQ(installed->text, "installed=0.9.0 target=win64 kind=archive staged=0.10.0 "
                               "skipped=none auto=on last_check=never token=absent "
                               "last=rolled_back:health_exit");

    // A development copy on the same machine shares the folder, and neither
    // is its own (update_verbs.hpp): nothing is ever applied to it.
    rig.install = update::Install{};
    rig.install.version = update::parseVersion("0.9.0").value();
    const core::Result<UpdateReply> development = runUpdateLine(rig.context(), "UPDATE STATUS");
    ASSERT_TRUE(development.ok()) << said(development);
    EXPECT_EQ(development->text, "installed=0.9.0 target=none kind=development staged=none "
                                 "skipped=none auto=on last_check=never token=absent last=none");
    ASSERT_TRUE(development->status.has_value());
    EXPECT_TRUE(development->status->stagedVersion.empty());
    EXPECT_FALSE(development->status->last.has_value());
}

TEST(UpdateVerbs, AStageIsReadyByItsTwoSignedRecordsAndByNothingElse)
{
    // What STATUS calls staged (update_verbs.hpp): the folder stage-<sequence>
    // whose two records verify and name that sequence. Each folder here lacks
    // one of those things, and is passed over.
    const fs::path release = fixtures() / "release" / "v2";
    const std::string manifest = bytesOf(release / "Katana-update-win64.json");
    const std::string signature = bytesOf(release / "Katana-update-win64.json.sig");
    Rig rig;
    // No signature beside the manifest: a download that did not finish.
    writeBytes(update::stagedManifestPath(rig.stageDirectory()), manifest);
    // The records of v2 in a folder named for another release.
    writeBytes(update::stagedManifestPath(rig.state / "stage-1792312801"), manifest);
    writeBytes(update::stagedSignaturePath(rig.state / "stage-1792312801"), signature);
    // Names that are no stage-<sequence>, and a file where a folder would be.
    writeBytes(update::stagedManifestPath(rig.state / "stage-"), manifest);
    writeBytes(update::stagedSignaturePath(rig.state / "stage-"), signature);
    writeBytes(update::stagedManifestPath(rig.state / "stage-1792312800x"), manifest);
    writeBytes(update::stagedSignaturePath(rig.state / "stage-1792312800x"), signature);
    writeBytes(rig.state / "stage-17", "a file");

    const UpdateContext context = rig.context();
    const core::Result<UpdateReply> none = runUpdateLine(context, "UPDATE STATUS");
    ASSERT_TRUE(none.ok()) << said(none);
    EXPECT_EQ(none->text, kFreshStatus);

    // With its signature the first folder is the release, ready.
    writeBytes(update::stagedSignaturePath(rig.stageDirectory()), signature);
    const core::Result<UpdateReply> ready = runUpdateLine(context, "UPDATE STATUS");
    ASSERT_TRUE(ready.ok()) << said(ready);
    ASSERT_TRUE(ready->status.has_value());
    EXPECT_EQ(ready->status->stagedVersion, "0.10.0");
}

// ---- UPDATE SKIP, UPDATE SET AUTO ---------------------------------------------------------------

TEST(UpdateVerbs, SetAutoIsKeptAndChangingOneSettingKeepsTheOthers)
{
    Rig rig;
    const UpdateContext context = rig.context();
    const core::Result<UpdateReply> off = runUpdateLine(context, "update set auto off");
    ASSERT_TRUE(off.ok()) << said(off);
    EXPECT_EQ(off->text, "auto=off");
    ASSERT_TRUE(runUpdateLine(context, "UPDATE SKIP 0.10.0").ok());

    core::Result<update::UpdateState> state = update::readState(rig.state);
    ASSERT_TRUE(state.ok());
    EXPECT_FALSE(state->automatic) << "the skip did not put the check back on";
    EXPECT_EQ(state->skippedVersion, "0.10.0");

    const core::Result<UpdateReply> on = runUpdateLine(context, "UPDATE SET AUTO ON");
    ASSERT_TRUE(on.ok()) << said(on);
    EXPECT_EQ(on->text, "auto=on");
    state = update::readState(rig.state);
    ASSERT_TRUE(state.ok());
    EXPECT_TRUE(state->automatic);
    EXPECT_EQ(state->skippedVersion, "0.10.0") << "and the switch kept the skip";
}

TEST(UpdateVerbs, WithNoStateFolderNothingPerUserIsReadOrWritten)
{
    // What a caller passes to keep nothing: an empty state folder. A state
    // file lies where the program's own would be, and is neither read nor
    // touched.
    Rig rig;
    rig.serveV2();
    update::UpdateState state;
    state.skippedVersion = "0.10.0";
    state.automatic = false;
    ASSERT_TRUE(update::writeState(rig.state, state).ok());
    const std::map<std::string, std::string> before = treeOf(rig.state);

    UpdateContext context = rig.context();
    context.stateDirectory.clear();

    const core::Result<UpdateReply> checked = runUpdateLine(context, "UPDATE CHECK");
    ASSERT_TRUE(checked.ok()) << said(checked);
    EXPECT_EQ(checked->text, kAvailable) << "the skip that is kept elsewhere was not read";

    const core::Result<UpdateReply> status = runUpdateLine(context, "UPDATE STATUS");
    ASSERT_TRUE(status.ok()) << said(status);
    EXPECT_EQ(status->text, kFreshStatus) << "nor the switch, nor when the last check was";

    // Nothing was carried out, and each reply gives the reason.
    const core::Result<UpdateReply> skipped = runUpdateLine(context, "UPDATE SKIP 0.10.0");
    ASSERT_TRUE(skipped.ok()) << said(skipped);
    EXPECT_EQ(skipped->text, "skipped=none reason=no_state");
    const core::Result<UpdateReply> automatic = runUpdateLine(context, "UPDATE SET AUTO OFF");
    ASSERT_TRUE(automatic.ok()) << said(automatic);
    EXPECT_EQ(automatic->text, "auto=on reason=no_state");

    EXPECT_EQ(treeOf(rig.state), before);
}

// ---- UPDATE TOKEN -------------------------------------------------------------------------------

TEST(UpdateVerbs, TokenStatusSaysPresentOrAbsentAndNeverTheToken)
{
    Rig rig;
    const UpdateContext context = rig.context();
    const core::Result<UpdateReply> absent = runUpdateLine(context, "UPDATE TOKEN STATUS");
    ASSERT_TRUE(absent.ok()) << said(absent);
    EXPECT_EQ(absent->text, "token=absent");

    rig.store.token = std::string(kSecret);
    const core::Result<UpdateReply> present = runUpdateLine(context, "update token status");
    ASSERT_TRUE(present.ok()) << said(present);
    EXPECT_EQ(present->text, "token=present") << "the whole reply: no word of it is the token";
}

TEST(UpdateVerbs, TokenStatusSaysWhenTheStoreCannotSay)
{
    Rig rig;
    const UpdateContext context = rig.context();
    // credential.hpp: InvalidState is something stored that is no usable
    // token; Unsupported and Internal are a store that cannot be asked.
    rig.store.readFailure = core::makeError(core::ErrorCode::InvalidState,
                                            "the stored value holds a space", "test store");
    const core::Result<UpdateReply> unusable = runUpdateLine(context, "UPDATE TOKEN STATUS");
    ASSERT_TRUE(unusable.ok()) << said(unusable);
    EXPECT_EQ(unusable->text,
              "token=absent reason=token_unusable detail=\"the stored value holds a space\"");

    rig.store.readFailure = core::makeError(core::ErrorCode::Unsupported,
                                            "the store cannot be used in this session");
    const core::Result<UpdateReply> unavailable = runUpdateLine(context, "UPDATE TOKEN STATUS");
    ASSERT_TRUE(unavailable.ok()) << said(unavailable);
    EXPECT_EQ(unavailable->text, "token=absent reason=store_unavailable "
                                 "detail=\"the store cannot be used in this session\"");
}

TEST(UpdateVerbs, WithNoStoreTheTokenLinesSaySo)
{
    Rig rig;
    UpdateContext context = rig.context();
    context.credentials = nullptr;
    for (const char* line : {"UPDATE TOKEN STATUS", "UPDATE TOKEN CLEAR"}) {
        const core::Result<UpdateReply> reply = runUpdateLine(context, line);
        ASSERT_TRUE(reply.ok()) << line << " -> " << said(reply);
        EXPECT_EQ(reply->text, "token=absent reason=no_store") << line;
    }
    // A token the context was handed is one the feed would send.
    context.feed.token = std::string(kSecret);
    const core::Result<UpdateReply> handed = runUpdateLine(context, "UPDATE TOKEN STATUS");
    ASSERT_TRUE(handed.ok()) << said(handed);
    EXPECT_EQ(handed->text, "token=present reason=no_store");
}

TEST(UpdateVerbs, TokenClearRemovesTheStoredToken)
{
    Rig rig;
    rig.store.token = std::string(kSecret);
    const core::Result<UpdateReply> reply = runUpdateLine(rig.context(), "UPDATE TOKEN CLEAR");
    ASSERT_TRUE(reply.ok()) << said(reply);
    EXPECT_EQ(reply->text, "token=absent");
    EXPECT_EQ(rig.store.clears, 1);
    EXPECT_TRUE(rig.store.token.empty());

    // Clearing when none is stored is no failure.
    const core::Result<UpdateReply> again = runUpdateLine(rig.context(), "update token clear");
    ASSERT_TRUE(again.ok()) << said(again);
    EXPECT_EQ(again->text, "token=absent");
}

TEST(UpdateVerbs, TokenClearSaysWhenTheEnvironmentStillGivesOne)
{
    // A store that is the environment variable: read-only, as credential.hpp
    // says of it (clear is Unsupported), and still giving its token.
    Rig rig;
    rig.store.token = std::string(kSecret);
    rig.store.place = "environment";
    rig.store.clearFailure =
        core::makeError(core::ErrorCode::Unsupported, "the variable is not Katana's to change");
    const core::Result<UpdateReply> reply = runUpdateLine(rig.context(), "UPDATE TOKEN CLEAR");
    ASSERT_TRUE(reply.ok()) << said(reply);
    EXPECT_EQ(reply->text.rfind("token=present reason=environment detail=", 0), 0u) << reply->text;
    EXPECT_TRUE(contains(reply->text, "KATANA_UPDATE_TOKEN")) << reply->text;
    EXPECT_FALSE(contains(reply->text, kSecret));

    // Any other failure of the store is the line's failure.
    rig.store.clearFailure = core::makeError(core::ErrorCode::Internal, "the store failed (5)");
    const core::Result<UpdateReply> failed = runUpdateLine(rig.context(), "UPDATE TOKEN CLEAR");
    ASSERT_FALSE(failed.ok()) << said(failed);
    EXPECT_EQ(failed.error().code, core::ErrorCode::Internal);
    EXPECT_FALSE(contains(failed.error().describe(), kSecret));
}

TEST(UpdateVerbs, ATokenTypedOnALineIsRefusedAndNeverRepeated)
{
    Rig rig;
    for (const char* line :
         {"UPDATE TOKEN SET ghp_TYPED-ON-A-LINE", "UPDATE TOKEN ghp_TYPED-ON-A-LINE",
          "UPDATE TOKEN STATUS ghp_TYPED-ON-A-LINE"}) {
        const core::Result<UpdateReply> reply = runUpdateLine(rig.context(), line);
        ASSERT_FALSE(reply.ok()) << line;
        EXPECT_EQ(reply.error().code, core::ErrorCode::InvalidArgument) << line;
        EXPECT_FALSE(contains(reply.error().describe(), "ghp_TYPED-ON-A-LINE")) << line;
        EXPECT_TRUE(contains(reply.error().message, "KATANA_UPDATE_TOKEN")) << line;
    }
    EXPECT_TRUE(rig.store.token.empty()) << "no line stores a token";
}

TEST(UpdateVerbs, TheTokenIsInNothingThatIsSaidWhateverATransportOrAStoreSays)
{
    // A transport, and a store, that put the token in their own words - which
    // the engine's never do. What reaches a reply, an error or a window's
    // offer has it masked all the same.
    const std::string secret(kSecret);
    Rig rig;
    rig.serveV2();
    rig.store.token = secret;
    rig.feedConfig.token = secret;
    rig.feed.fail(kSignatureUrl,
                  core::makeError(core::ErrorCode::FileImportFailure,
                                  "no answer from https://api.github.com/x?access_token=" + secret,
                                  "status=0; Bearer " + secret));
    const UpdateContext context = rig.context();

    const core::Result<UpdateReply> checked = runUpdateLine(context, "UPDATE CHECK");
    ASSERT_TRUE(checked.ok()) << "(not shown: it may hold the token)";
    EXPECT_FALSE(contains(checked->text, secret));
    EXPECT_TRUE(contains(checked->text, "access_token=***")) << "masked, not dropped";
    ASSERT_TRUE(checked->offer.has_value());
    EXPECT_FALSE(contains(checked->offer->check.detail, secret));
    EXPECT_FALSE(contains(checked->offer->check.reason, secret));

    // A word of a line that is the token is not said back either.
    const core::Result<UpdateReply> typed = runUpdateLine(context, "UPDATE " + secret);
    ASSERT_FALSE(typed.ok());
    EXPECT_FALSE(contains(typed.error().describe(), secret));
    EXPECT_TRUE(contains(typed.error().message, "'***' is not an UPDATE action"));

    for (const char* line : {"UPDATE STATUS", "UPDATE TOKEN STATUS", "UPDATE SKIP 0.10.0",
                             "UPDATE SET AUTO OFF", "UPDATE TOKEN CLEAR"}) {
        const core::Result<UpdateReply> reply = runUpdateLine(context, line);
        EXPECT_FALSE(contains(said(reply), secret)) << line;
    }
}

// ---- what a surface may not run -----------------------------------------------------------------

TEST(UpdateVerbs, AHeadlessWindowChecksAndReportsWhenAskedAndKeepsNothing)
{
    Rig rig;
    rig.serveV2();
    // What the person keeps: a skip of this very release, and the check off.
    update::UpdateState state;
    state.skippedVersion = "0.10.0";
    state.automatic = false;
    ASSERT_TRUE(update::writeState(rig.state, state).ok());
    const std::map<std::string, std::string> before = treeOf(rig.state);

    const UpdateContext context = rig.context(UpdateSurface::HeadlessWindow);
    ASSERT_TRUE(context.stateDirectory.empty());

    const core::Result<UpdateReply> checked = runUpdateLine(context, "UPDATE CHECK");
    ASSERT_TRUE(checked.ok()) << said(checked);
    EXPECT_EQ(checked->text, kAvailable) << "the same answer on every machine";
    const core::Result<UpdateReply> status = runUpdateLine(context, "UPDATE STATUS");
    ASSERT_TRUE(status.ok()) << said(status);
    EXPECT_EQ(status->text, kFreshStatus);
    EXPECT_EQ(treeOf(rig.state), before) << "no per-user file was written";
}

TEST(UpdateVerbs, AHeadlessWindowsDownloadAndApplyAreAnsweredWithTheReasonAndDoNothing)
{
    Rig rig;
    rig.serveV2();
    const UpdateContext context = rig.context(UpdateSurface::HeadlessWindow);

    const core::Result<UpdateReply> download = runUpdateLine(context, "UPDATE DOWNLOAD CONFIRM");
    ASSERT_TRUE(download.ok()) << "a reply, not a refusal of the line: " << said(download);
    EXPECT_EQ(download->text.rfind("staged=none reason=headless text=", 0), 0u) << download->text;
    EXPECT_TRUE(contains(download->text, "the Katana window or katana_cli")) << download->text;

    const core::Result<UpdateReply> apply = runUpdateLine(context, "UPDATE APPLY CONFIRM");
    ASSERT_TRUE(apply.ok()) << said(apply);
    EXPECT_EQ(apply->text.rfind("ready=none reason=headless text=", 0), 0u) << apply->text;
    EXPECT_TRUE(contains(apply->text, "the Katana window or katana_cli")) << apply->text;
    EXPECT_FALSE(apply->apply.has_value());

    EXPECT_TRUE(rig.feed.requests.empty()) << "nothing was asked of the feed";
    EXPECT_TRUE(rig.launcher.runs.empty());
    EXPECT_FALSE(fs::exists(rig.state));
}

TEST(UpdateVerbs, AHeadlessWindowIsRefusedEveryLineThatKeepsOrReadsAPerUserThing)
{
    Rig rig;
    rig.serveV2();
    rig.store.token = std::string(kSecret);
    const UpdateContext context = rig.context(UpdateSurface::HeadlessWindow);
    for (const char* line : {"UPDATE SKIP 0.10.0", "UPDATE SKIP NONE", "UPDATE SET AUTO OFF",
                             "UPDATE SET AUTO ON", "UPDATE TOKEN STATUS", "UPDATE TOKEN CLEAR"}) {
        const core::Result<UpdateReply> reply = runUpdateLine(context, line);
        ASSERT_FALSE(reply.ok()) << line << " -> " << said(reply);
        EXPECT_EQ(reply.error().code, core::ErrorCode::Unsupported) << line;
        EXPECT_EQ(reply.error().context, "reason=headless") << line;
        EXPECT_TRUE(contains(reply.error().message, "the Katana window or katana_cli runs it"))
            << line << ": " << reply.error().message;
    }
    EXPECT_EQ(rig.store.clears, 0);
    EXPECT_EQ(rig.store.token, kSecret);
    EXPECT_FALSE(fs::exists(rig.state));
    EXPECT_TRUE(rig.feed.requests.empty());
}

TEST(UpdateVerbs, KatanaMcpsSurfaceChecksAndReportsAndIsRefusedEverythingElse)
{
    Rig rig;
    rig.serveV2();
    rig.store.token = std::string(kSecret);
    const UpdateContext context = rig.context(UpdateSurface::Mcp);
    ASSERT_TRUE(context.stateDirectory.empty());

    const core::Result<UpdateReply> checked = runUpdateLine(context, "UPDATE CHECK");
    ASSERT_TRUE(checked.ok()) << said(checked);
    EXPECT_EQ(checked->text, kAvailable);
    const core::Result<UpdateReply> status = runUpdateLine(context, "UPDATE STATUS");
    ASSERT_TRUE(status.ok()) << said(status);
    // The store this context was given holds a token, and that much is said.
    EXPECT_EQ(status->text, "installed=0.9.0 target=win64 kind=archive staged=none skipped=none "
                            "auto=on last_check=never token=present last=none");

    const std::size_t asked = rig.feed.requests.size();
    for (const char* line :
         {"UPDATE DOWNLOAD CONFIRM", "UPDATE APPLY CONFIRM", "UPDATE SKIP 0.10.0",
          "UPDATE SKIP NONE", "UPDATE SET AUTO OFF", "UPDATE TOKEN STATUS", "UPDATE TOKEN CLEAR"}) {
        const core::Result<UpdateReply> reply = runUpdateLine(context, line);
        ASSERT_FALSE(reply.ok()) << line << " -> " << said(reply);
        EXPECT_EQ(reply.error().code, core::ErrorCode::Unsupported) << line;
        EXPECT_EQ(reply.error().context, "reason=mcp") << line;
        EXPECT_TRUE(contains(reply.error().message, "katana_check_update")) << line;
        EXPECT_TRUE(contains(reply.error().message, "the Katana window or katana_cli runs it"))
            << line << ": " << reply.error().message;
    }
    EXPECT_EQ(rig.feed.requests.size(), asked) << "a refused line asks the feed for nothing";
    EXPECT_TRUE(rig.launcher.runs.empty());
    EXPECT_EQ(rig.store.clears, 0);
    EXPECT_FALSE(fs::exists(rig.state)) << "an agent's check keeps nothing";
}

// ---- katana_mcp's tool --------------------------------------------------------------------------

TEST(UpdateVerbs, TheCheckUpdateToolGivesTheTwoRecordsAsTextAndAsObjects)
{
    Rig rig;
    rig.serveV2();
    const katana::app::mcp::ToolReply reply =
        katana::app::mcp::checkUpdateReply(rig.context(UpdateSurface::Mcp));
    EXPECT_FALSE(reply.isError) << reply.text;
    EXPECT_EQ(reply.text, kAvailable + "\n" + kFreshStatus);

    const nlohmann::json& check = reply.structured["check"];
    EXPECT_EQ(check["update"], "available");
    EXPECT_EQ(check["installed"], "0.9.0");
    EXPECT_EQ(check["latest"], "0.10.0");
    EXPECT_EQ(check["files"], "5/10");
    EXPECT_EQ(check["download_bytes"], 339) << "a number, not text";
    EXPECT_EQ(check["notes_url"], kNotes);
    EXPECT_FALSE(check.contains("reason"));

    const nlohmann::json& status = reply.structured["status"];
    EXPECT_EQ(status["installed"], "0.9.0");
    EXPECT_EQ(status["target"], "win64");
    EXPECT_EQ(status["kind"], "archive");
    EXPECT_EQ(status["staged"], "none");
    EXPECT_EQ(status["auto"], "on");
    EXPECT_EQ(status["last_check"], "never");
    EXPECT_EQ(status["token"], "absent");
    EXPECT_FALSE(fs::exists(rig.state)) << "the tool is read-only: no check is recorded";
}

TEST(UpdateVerbs, TheCheckUpdateToolGivesAFailuresSentenceUnquoted)
{
    Rig rig;
    rig.serveV2();
    rig.feed.fail(kSignatureUrl, httpStatus(core::ErrorCode::NotFound, 404));
    const katana::app::mcp::ToolReply reply =
        katana::app::mcp::checkUpdateReply(rig.context(UpdateSurface::Mcp));
    EXPECT_FALSE(reply.isError) << "a feed that cannot be read is an answer: " << reply.text;
    EXPECT_EQ(reply.structured["check"]["update"], "unreachable");
    EXPECT_EQ(reply.structured["check"]["reason"], "http_404");
    EXPECT_EQ(reply.structured["check"]["detail"], "the server said 404");
    EXPECT_FALSE(reply.structured["check"].contains("latest"));
}

// ---- the running program's context --------------------------------------------------------------

TEST(UpdateVerbs, TheContextOfAProgramInNoInstallIsADevelopmentCopys)
{
    // This suite's executable is in no install: nothing packaging wrote lies
    // beside it. The token comes from the variable alone here, so that the
    // credential store of whoever runs the suite is not read.
    katana::app::tests::ScopedVariable token("KATANA_UPDATE_TOKEN", "ghp_from_the_environment");
    Scratch scratch;
    katana::app::tests::ScopedVariable folder("KATANA_UPDATE_STATE_DIR", scratch.path() / "U");

    for (const UpdateSurface surface : {UpdateSurface::Cli, UpdateSurface::Window,
                                        UpdateSurface::HeadlessWindow, UpdateSurface::Mcp}) {
        const UpdateContext context = katana::app::makeUpdateContext(surface);
        EXPECT_EQ(context.surface, surface);
        EXPECT_EQ(context.install.kind, update::InstallKind::Development);
        EXPECT_TRUE(context.install.target.empty());
        EXPECT_EQ(update::toString(context.install.version), update::programVersionText());
        EXPECT_TRUE(context.installProblem.empty()) << context.installProblem;
        EXPECT_TRUE(static_cast<bool>(context.fetch));
        ASSERT_NE(context.credentials, nullptr);
        EXPECT_EQ(context.feed.token, "ghp_from_the_environment");
        EXPECT_EQ(context.pid, 0u) << "left for the caller";
        EXPECT_TRUE(context.relaunch.empty());
        const bool keeps = surface == UpdateSurface::Cli || surface == UpdateSurface::Window;
        EXPECT_EQ(context.stateDirectory.empty(), !keeps);
        if (keeps) {
            EXPECT_EQ(context.stateDirectory, scratch.path() / "U");
        }

        // And the lines on it: a development copy, asked about by nobody.
        const core::Result<UpdateReply> checked = runUpdateLine(context, "UPDATE CHECK");
        ASSERT_TRUE(checked.ok()) << said(checked);
        EXPECT_TRUE(contains(checked->text, "update=unsupported")) << checked->text;
        EXPECT_TRUE(contains(checked->text, "reason=development_tree")) << checked->text;
        EXPECT_FALSE(contains(checked->text, "ghp_from_the_environment"));
        const core::Result<UpdateReply> status = runUpdateLine(context, "UPDATE STATUS");
        ASSERT_TRUE(status.ok()) << said(status);
        EXPECT_TRUE(contains(status->text, "kind=development")) << status->text;
        EXPECT_TRUE(contains(status->text, "token=present")) << status->text;
    }
    EXPECT_FALSE(fs::exists(scratch.path() / "U")) << "nothing was kept by any of them";
}

TEST(UpdateVerbs, AHeadlessContextHasNoTokenWithoutTheVariable)
{
    katana::app::tests::ScopedVariable token("KATANA_UPDATE_TOKEN", nullptr);
    for (const UpdateSurface surface : {UpdateSurface::HeadlessWindow, UpdateSurface::Mcp}) {
        const UpdateContext context = katana::app::makeUpdateContext(surface);
        EXPECT_TRUE(context.feed.token.empty());
        const core::Result<UpdateReply> status = runUpdateLine(context, "UPDATE STATUS");
        ASSERT_TRUE(status.ok()) << said(status);
        EXPECT_TRUE(contains(status->text, "token=absent")) << status->text;
    }
}

TEST(UpdateVerbs, TheContextOfAProgramInAnInstallIsThatInstallsOwnRecords)
{
    // A scratch install with the two records an installed copy has, and the
    // context of the program that lies in its bin/. The token comes from the
    // variable and the state folder is the test's own, so that neither the
    // credential store nor the settings of whoever runs the suite is read.
    katana::app::tests::ScopedVariable token("KATANA_UPDATE_TOKEN", "ghp_from_the_environment");
    Rig rig;
    katana::app::tests::ScopedVariable folder("KATANA_UPDATE_STATE_DIR", rig.state);
    writeInstallRecords(rig.root);
    const fs::path program = rig.root / "bin" / "katana_cli.exe";
    ASSERT_TRUE(fs::exists(program));

    UpdateContext context = katana::app::makeUpdateContext(UpdateSurface::Cli, program);
    EXPECT_EQ(context.surface, UpdateSurface::Cli);
    EXPECT_TRUE(context.installProblem.empty()) << context.installProblem;
    EXPECT_EQ(context.install.root, rig.root);
    EXPECT_EQ(context.install.target, "win64");
    // No installer recorded this scratch folder as the place it installed to.
    EXPECT_EQ(context.install.kind, update::InstallKind::Archive);
    EXPECT_EQ(update::toString(context.install.version), "0.9.0");
    EXPECT_EQ(context.install.sequence, kV1Sequence);
    EXPECT_TRUE(context.install.hasInstalledRecord);
    EXPECT_EQ(context.feed.target, "win64") << "the feed is asked for what the copy was packaged as";
    EXPECT_EQ(context.feed.token, "ghp_from_the_environment");
    EXPECT_EQ(context.stateDirectory, rig.state);
    EXPECT_EQ(context.pid, 0u) << "left for the caller";
    EXPECT_TRUE(context.relaunch.empty());

    // The lines on it, with the rig's feed and the fixtures' keys where the
    // network and the release keys are: an installed copy, so it IS asked
    // about. (The address is the rig's, whatever this build was configured
    // to ask.)
    context.trust = &testKeys();
    context.feed = rig.feedConfig;
    context.fetch = rig.feed.fetch();
    context.nowEpoch = kNow;
    rig.serveV2();
    const core::Result<UpdateReply> status = runUpdateLine(context, "UPDATE STATUS");
    ASSERT_TRUE(status.ok()) << said(status);
    EXPECT_EQ(status->text, "installed=0.9.0 target=win64 kind=archive staged=none skipped=none "
                            "auto=on last_check=never token=present last=none");
    const core::Result<UpdateReply> checked = runUpdateLine(context, "UPDATE CHECK");
    ASSERT_TRUE(checked.ok()) << said(checked);
    EXPECT_EQ(checked->text, kAvailable);
}

TEST(UpdateVerbs, TheContextOfAProgramWhoseInstallRecordDoesNotReadSaysSoAndIsAskedAboutByNobody)
{
    katana::app::tests::ScopedVariable token("KATANA_UPDATE_TOKEN", "ghp_from_the_environment");
    Rig rig;
    katana::app::tests::ScopedVariable folder("KATANA_UPDATE_STATE_DIR", rig.state);
    writeBytes(rig.root / "share" / "katana" / "install.json", "this is not a record");

    UpdateContext context =
        katana::app::makeUpdateContext(UpdateSurface::Cli, rig.root / "bin" / "katana_cli.exe");
    // Why, and of which file: the error of the reading names it (install.hpp).
    EXPECT_TRUE(contains(context.installProblem, "install.json")) << context.installProblem;
    EXPECT_EQ(context.install.root, rig.root);
    EXPECT_TRUE(context.install.target.empty());
    EXPECT_EQ(update::toString(context.install.version), update::programVersionText());

    context.trust = &testKeys();
    context.feed = rig.feedConfig;
    context.fetch = rig.feed.fetch();
    context.nowEpoch = kNow;
    rig.serveV2();
    const std::string version(update::programVersionText());
    const core::Result<UpdateReply> checked = runUpdateLine(context, "UPDATE CHECK");
    ASSERT_TRUE(checked.ok()) << said(checked);
    EXPECT_EQ(checked->text.rfind("update=unsupported installed=" + version +
                                      " reason=install_record detail=",
                                  0),
              0u)
        << checked->text;
    const core::Result<UpdateReply> status = runUpdateLine(context, "UPDATE STATUS");
    ASSERT_TRUE(status.ok()) << said(status);
    const std::vector<std::string> lines = linesOf(status->text);
    ASSERT_EQ(lines.size(), 2u) << status->text;
    EXPECT_EQ(lines[0], "installed=" + version +
                            " target=none kind=development staged=none skipped=none auto=on "
                            "last_check=never token=present last=none");
    EXPECT_EQ(lines[1].rfind("warning text=\"this copy's install record could not be read", 0),
              0u)
        << lines[1];
    EXPECT_TRUE(rig.feed.requests.empty()) << "nothing is asked about a copy that is not known";
    EXPECT_FALSE(fs::exists(rig.state));
}

// ---- the help, and the session ------------------------------------------------------------------

TEST(UpdateVerbs, TheHelpNamesEveryLineOfTheGrammarAndIsInTheSessionsHelp)
{
    const std::string help = katana::app::updateHelpText();
    for (const char* words :
         {"Update    UPDATE CHECK", "UPDATE STATUS", "UPDATE DOWNLOAD CONFIRM",
          "UPDATE APPLY CONFIRM", "UPDATE SKIP <version> | UPDATE SKIP NONE",
          "UPDATE SET AUTO ON|OFF", "UPDATE TOKEN STATUS | UPDATE TOKEN CLEAR",
          "update=available|current|unreachable|unsupported|blocked",
          "kind=installer|archive|development", "KATANA_UPDATE_TOKEN", "katana_check_update",
          "the last line of a katana_cli run"}) {
        EXPECT_TRUE(contains(help, words)) << words;
    }
    // The word a script fails a run on is in no help text.
    EXPECT_FALSE(contains(help, "error"));
    EXPECT_TRUE(contains(katana::app::Session::helpText(), help));
}

TEST(UpdateVerbs, HelpUpdateTypedInASessionIsTheUpdateVerbsOwnBlock)
{
    katana::app::Session session(nullptr);
    for (const char* line : {"HELP UPDATE", "help  update", "? UPDATE"}) {
        Captured captured;
        EXPECT_TRUE(session.run(line)) << line;
        EXPECT_EQ(captured.out(), katana::app::updateHelpText()) << line;
        EXPECT_TRUE(captured.err().empty()) << line;
    }
}

TEST(UpdateVerbs, ASessionOfNoProgramRefusesUpdateLinesAndLeavesNothingToApply)
{
    // The session a test makes is no program's (session.hpp): there is no
    // install for UPDATE to act on, and no user whose settings to read.
    katana::app::Session session(nullptr);
    for (const char* line : {"UPDATE STATUS", "UPDATE CHECK", "update apply confirm"}) {
        Captured captured;
        EXPECT_FALSE(session.run(line)) << line;
        EXPECT_TRUE(contains(captured.err(), "error: Unsupported: UPDATE acts on the program"))
            << captured.err();
        EXPECT_TRUE(captured.out().empty()) << captured.out();
    }
    EXPECT_FALSE(session.takePendingApply().has_value());
}

TEST(UpdateVerbs, ASessionToldItIsKatanaMcpsRefusesEveryUpdateLineNamingTheTool)
{
    katana::app::Session session(nullptr);
    session.setUpdateSurface(UpdateSurface::Mcp);
    for (const char* line : {"UPDATE CHECK", "UPDATE STATUS", "update download confirm",
                             "UPDATE APPLY CONFIRM", "UPDATE"}) {
        Captured captured;
        EXPECT_FALSE(session.run(line)) << line;
        EXPECT_TRUE(contains(captured.err(), "error: Unsupported: UPDATE lines are not run by "
                                             "katana_mcp"))
            << captured.err();
        EXPECT_TRUE(contains(captured.err(), "katana_check_update")) << captured.err();
    }
    EXPECT_FALSE(session.takePendingApply().has_value());
}

TEST(UpdateVerbs, TheLineAFrontEndMustRunLastIsUpdateApply)
{
    using katana::app::Session;
    for (const char* line : {"UPDATE APPLY CONFIRM", "update apply confirm", "  UPDATE\tAPPLY",
                             "UPDATE APPLY NOW"}) {
        EXPECT_TRUE(Session::isUpdateApply(line)) << line;
        EXPECT_TRUE(katana::app::isUpdateApplyLine(line)) << line;
    }
    for (const char* line : {"UPDATE CHECK", "UPDATE DOWNLOAD CONFIRM", "UPDATE APPLYING",
                             "APPLY", "HELP UPDATE", "", "# UPDATE APPLY CONFIRM"}) {
        EXPECT_FALSE(Session::isUpdateApply(line)) << line;
        EXPECT_FALSE(katana::app::isUpdateApplyLine(line)) << line;
    }
}

TEST(UpdateVerbs, TheLastLineRuleReadsALineAsTheVerbsDo)
{
    // A carriage return or a line feed parts two words for the verbs, as a
    // blank does, and a -c argument can hold one. Such a line IS an apply to
    // the verbs, so it is one to the rule that holds an apply to the last
    // line: read otherwise, the lines after it would be run by a session that
    // had already prepared the swap.
    Rig rig;
    const UpdateContext headless = rig.context(UpdateSurface::HeadlessWindow);
    for (const char* line : {"UPDATE\rAPPLY CONFIRM", "UPDATE\nAPPLY CONFIRM",
                             "update \r\n apply\rconfirm", "UPDATE\tAPPLY\rCONFIRM"}) {
        // What the verbs make of it: UPDATE APPLY CONFIRM, answered here as a
        // headless window answers that line on every system.
        const core::Result<UpdateReply> reply = runUpdateLine(headless, line);
        ASSERT_TRUE(reply.ok()) << said(reply);
        EXPECT_EQ(reply->text.rfind("ready=none reason=headless", 0), 0u) << reply->text;
        EXPECT_TRUE(katana::app::Session::isUpdateApply(line));
        EXPECT_TRUE(katana::app::isUpdateApplyLine(line));
    }
}

TEST(UpdateVerbs, AProgramsSessionRunsItsUpdateLinesOnTheContextItIsHandedAndPrintsTheReply)
{
    Rig rig;
    rig.serveV2();
    CliSession cli(rig);
    {
        Captured captured;
        EXPECT_TRUE(cli.session.run("UPDATE STATUS"));
        EXPECT_EQ(captured.out(), kFreshStatus + "\n");
        EXPECT_TRUE(captured.err().empty()) << captured.err();
    }
    {
        Captured captured;
        EXPECT_TRUE(cli.session.run("update check"));
        EXPECT_EQ(captured.out(), kAvailable + "\n");
        EXPECT_TRUE(captured.err().empty()) << captured.err();
    }
    {
        // A refusal is the line's failure: said on std::cerr, nothing on std::cout.
        Captured captured;
        EXPECT_FALSE(cli.session.run("UPDATE DOWNLOAD"));
        EXPECT_TRUE(captured.out().empty()) << captured.out();
        EXPECT_EQ(captured.err().rfind("error: InvalidArgument: UPDATE DOWNLOAD needs the word "
                                       "CONFIRM",
                                       0),
                  0u)
            << captured.err();
    }
    // The context is asked for as katana_cli's, anew for each line.
    ASSERT_GE(cli.asked.size(), 2u);
    for (const UpdateSurface surface : cli.asked) {
        EXPECT_EQ(surface, UpdateSurface::Cli);
    }
    EXPECT_FALSE(cli.session.takePendingApply().has_value());

    // Told that it is katana_mcp's, the session refuses the line before any
    // context is made: nothing of the install or the user is read for it.
    cli.session.setUpdateSurface(UpdateSurface::Mcp);
    cli.asked.clear();
    {
        Captured captured;
        EXPECT_FALSE(cli.session.run("UPDATE CHECK"));
        EXPECT_TRUE(contains(captured.err(), "katana_check_update")) << captured.err();
    }
    EXPECT_TRUE(cli.asked.empty());
}

// ---- UPDATE DOWNLOAD CONFIRM and UPDATE APPLY CONFIRM -------------------------------------------

#ifndef _WIN32

TEST(UpdateVerbs, DownloadAndApplyAreRefusedWhereThisVersionInstallsNoUpdate)
{
    // The swap is written for Windows only so far: elsewhere a release is
    // told of (UPDATE CHECK) and no more, and the two lines say so at once.
    Rig rig;
    rig.serveV2();
    for (const char* line : {"UPDATE DOWNLOAD CONFIRM", "UPDATE APPLY CONFIRM"}) {
        const core::Result<UpdateReply> reply = runUpdateLine(rig.context(), line);
        ASSERT_FALSE(reply.ok()) << line << " -> " << said(reply);
        EXPECT_EQ(reply.error().code, core::ErrorCode::Unsupported) << line;
        EXPECT_EQ(reply.error().context, "reason=platform") << line;
        EXPECT_TRUE(contains(reply.error().message, "UPDATE CHECK")) << reply.error().message;
    }
    EXPECT_TRUE(rig.feed.requests.empty());
    EXPECT_TRUE(rig.launcher.runs.empty());
    EXPECT_FALSE(fs::exists(rig.state));
}

#else

// The whole payload zip, as the v2 manifest signs its size.
constexpr std::uint64_t kPayloadSize = 3791;

// The files v2 changes, as the head of this file lists them.
const std::vector<std::string> kChanged = {"bin/katana.exe", "bin/libnew-2.dll",
                                           "share/katana/help/index.txt", "share/katana/news.txt",
                                           "share/katana/readme.txt"};
const std::vector<std::string> kKept = {"bin/katana_cli.exe", "bin/libcore-1.dll",
                                        "etc/katana/settings.ini", "share/katana/empty.txt",
                                        "share/katana/random.bin"};

TEST(UpdateVerbs, DownloadStagesTheChangedFilesInOneRequestAndSaysSo)
{
    Rig rig;
    rig.serveV2();
    const std::map<std::string, std::string> installed = treeOf(rig.root);
    UpdateContext context = rig.context();
    std::vector<std::string> spoken;
    context.progressLine = [&spoken](std::string_view line) { spoken.emplace_back(line); };
    std::vector<std::pair<std::uint64_t, std::uint64_t>> counted;
    context.progress = [&counted](std::uint64_t done, std::uint64_t total) {
        counted.emplace_back(done, total);
    };

    const core::Result<UpdateReply> reply = runUpdateLine(context, "UPDATE DOWNLOAD CONFIRM");
    ASSERT_TRUE(reply.ok()) << said(reply);

    // Progress lines, then what is staged.
    const std::vector<std::string> lines = linesOf(reply->text);
    ASSERT_GE(lines.size(), 2u) << reply->text;
    EXPECT_EQ(lines.back(), "staged=0.10.0");
    const std::vector<std::string> progress(lines.begin(), lines.end() - 1);
    for (const std::string& line : progress) {
        EXPECT_EQ(line.rfind("progress bytes=", 0), 0u) << line;
        EXPECT_TRUE(contains(line, "/2949 files=")) << line;
    }
    // One request for [44, 2993), and all five files there at the end.
    EXPECT_EQ(progress.back(), "progress bytes=2949/2949 files=5/5");
    EXPECT_EQ(spoken, progress) << "each line was said as it happened, and is in the reply";
    ASSERT_FALSE(counted.empty());
    EXPECT_EQ(counted.back(), (std::pair<std::uint64_t, std::uint64_t>{2949, 2949}));

    // It asked the feed first, as CHECK does, so what a window shows of the
    // release is in this reply too (update_verbs.hpp).
    ASSERT_TRUE(reply->offer.has_value());
    EXPECT_EQ(reply->offer->check.status, update::CheckStatus::Available);
    EXPECT_EQ(reply->offer->check.latestVersion, "0.10.0");
    EXPECT_EQ(reply->offer->changedFiles, 5u);
    EXPECT_EQ(reply->offer->totalFiles, 10u);
    EXPECT_EQ(reply->offer->downloadBytes, 339u);
    EXPECT_FALSE(reply->apply.has_value());

    const std::vector<update::FetchRequest> payload = rig.feed.requestsFor(kPayloadUrl);
    ASSERT_EQ(payload.size(), 1u);
    EXPECT_EQ(payload[0].rangeStart, 44u);
    EXPECT_EQ(payload[0].rangeLength, 2949u);
    EXPECT_EQ(payload[0].maxBytes, 2949u) << "a server that ignores Range cannot send more";
    EXPECT_EQ(payload[0].payloadSize, kPayloadSize);

    // The staged files are v2's, byte for byte; what did not change is not there.
    const fs::path stage = rig.stageDirectory();
    for (const std::string& path : kChanged) {
        EXPECT_EQ(bytesOf(stage / path), bytesOf(fixtures() / "install" / "v2" / path)) << path;
    }
    for (const std::string& path : kKept) {
        EXPECT_FALSE(fs::exists(stage / path)) << path;
    }
    // And the two records that say the stage is complete: the signed bytes.
    const fs::path release = fixtures() / "release" / "v2";
    EXPECT_EQ(bytesOf(update::stagedManifestPath(stage)),
              bytesOf(release / "Katana-update-win64.json"));
    EXPECT_EQ(bytesOf(update::stagedSignaturePath(stage)),
              bytesOf(release / "Katana-update-win64.json.sig"));

    EXPECT_EQ(treeOf(rig.root), installed) << "a download changes nothing in the install";

    const core::Result<UpdateReply> status = runUpdateLine(context, "UPDATE STATUS");
    ASSERT_TRUE(status.ok()) << said(status);
    EXPECT_EQ(status->text, std::string("installed=0.9.0 target=win64 kind=archive staged=0.10.0 "
                                        "skipped=none auto=on last_check=") +
                                kNowText + " token=absent last=none");
    ASSERT_TRUE(status->status.has_value());
    EXPECT_EQ(status->status->stagedVersion, "0.10.0");
}

TEST(UpdateVerbs, ASecondDownloadFetchesOnlyWhatIsMissing)
{
    Rig rig;
    rig.serveV2();
    const UpdateContext context = rig.context();
    ASSERT_TRUE(runUpdateLine(context, "UPDATE DOWNLOAD CONFIRM").ok());

    // One staged file lost (a download cut short leaves the same): the next
    // run asks for that member alone - news.txt, 54 bytes at 725 - and counts
    // the other four, 85 + 70 + 75 + 55 = 285 bytes, as already fetched.
    const fs::path stage = rig.stageDirectory();
    ASSERT_TRUE(fs::remove(stage / "share" / "katana" / "news.txt"));
    rig.feed.requests.clear();
    const core::Result<UpdateReply> resumed = runUpdateLine(context, "UPDATE DOWNLOAD CONFIRM");
    ASSERT_TRUE(resumed.ok()) << said(resumed);
    const std::vector<update::FetchRequest> payload = rig.feed.requestsFor(kPayloadUrl);
    ASSERT_EQ(payload.size(), 1u);
    EXPECT_EQ(payload[0].rangeStart, 725u);
    EXPECT_EQ(payload[0].rangeLength, 54u);
    const std::vector<std::string> lines = linesOf(resumed->text);
    ASSERT_GE(lines.size(), 2u);
    EXPECT_EQ(lines.front(), "progress bytes=285/339 files=4/5");
    EXPECT_EQ(lines[lines.size() - 2], "progress bytes=339/339 files=5/5");
    EXPECT_EQ(lines.back(), "staged=0.10.0");
    EXPECT_EQ(bytesOf(stage / "share" / "katana" / "news.txt"),
              bytesOf(fixtures() / "install" / "v2" / "share" / "katana" / "news.txt"));

    // With nothing missing, nothing of the payload is asked for at all.
    rig.feed.requests.clear();
    const core::Result<UpdateReply> again = runUpdateLine(context, "UPDATE DOWNLOAD CONFIRM");
    ASSERT_TRUE(again.ok()) << said(again);
    EXPECT_TRUE(rig.feed.requestsFor(kPayloadUrl).empty());
    EXPECT_EQ(again->text, "progress bytes=339/339 files=5/5\nstaged=0.10.0");
}

TEST(UpdateVerbs, ADownloadThatFailsIsAnErrorWithItsReasonAndLeavesNothingReady)
{
    Rig rig;
    rig.serveV2();
    rig.feed.fail(kPayloadUrl, httpStatus(core::ErrorCode::FileImportFailure, 503));
    const UpdateContext context = rig.context();
    const core::Result<UpdateReply> reply = runUpdateLine(context, "UPDATE DOWNLOAD CONFIRM");
    ASSERT_FALSE(reply.ok()) << said(reply);
    EXPECT_EQ(reply.error().message, "the server said 503");
    EXPECT_EQ(reply.error().context.rfind("reason=http_503; ", 0), 0u) << reply.error().context;

    EXPECT_FALSE(fs::exists(update::stagedManifestPath(rig.stageDirectory())));
    const core::Result<UpdateReply> status = runUpdateLine(context, "UPDATE STATUS");
    ASSERT_TRUE(status.ok()) << said(status);
    EXPECT_TRUE(contains(status->text, "staged=none")) << status->text;

    // Nothing is ready, so there is nothing to apply.
    const core::Result<UpdateReply> apply = runUpdateLine(context, "UPDATE APPLY CONFIRM");
    ASSERT_FALSE(apply.ok());
    EXPECT_EQ(apply.error().context, "reason=not_staged");
}

TEST(UpdateVerbs, ADownloadWithATokenInItsFailureSaysNoneOfIt)
{
    const std::string secret(kSecret);
    Rig rig;
    rig.serveV2();
    rig.feedConfig.token = secret;
    rig.feed.fail(kPayloadUrl,
                  core::makeError(core::ErrorCode::FileImportFailure,
                                  "refused: Authorization: Bearer " + secret,
                                  "status=403; https://h/p?token=" + secret));
    UpdateContext context = rig.context();
    std::vector<std::string> spoken;
    context.progressLine = [&spoken](std::string_view line) { spoken.emplace_back(line); };
    const core::Result<UpdateReply> reply = runUpdateLine(context, "UPDATE DOWNLOAD CONFIRM");
    ASSERT_FALSE(reply.ok());
    EXPECT_FALSE(contains(reply.error().describe(), secret));
    EXPECT_TRUE(contains(reply.error().message, "Bearer ***"));
    EXPECT_EQ(reply.error().context.rfind("reason=http_403; ", 0), 0u);
    for (const std::string& line : spoken) {
        EXPECT_FALSE(contains(line, secret)) << "a progress line";
    }
}

TEST(UpdateVerbs, DownloadWithNothingNewerSaysWhyAndFetchesNothing)
{
    {
        Rig rig;
        rig.serveV2();
        rig.installV2();
        const core::Result<UpdateReply> reply =
            runUpdateLine(rig.context(), "UPDATE DOWNLOAD CONFIRM");
        ASSERT_TRUE(reply.ok()) << said(reply);
        EXPECT_EQ(reply->text, "update=current installed=0.10.0 latest=0.10.0 notes_url=" +
                                   kNotes + "\nstaged=none");
        EXPECT_TRUE(rig.feed.requestsFor(kPayloadUrl).empty());
        EXPECT_FALSE(fs::exists(rig.stageDirectory()));
    }
    {
        Rig rig;
        rig.serveV2();
        rig.feed.fail(kSignatureUrl, httpStatus(core::ErrorCode::FileImportFailure, 429));
        const core::Result<UpdateReply> reply =
            runUpdateLine(rig.context(), "UPDATE DOWNLOAD CONFIRM");
        ASSERT_TRUE(reply.ok()) << said(reply);
        EXPECT_EQ(reply->text, "update=unreachable installed=0.9.0 reason=http_429 "
                               "detail=\"the server said 429\"\nstaged=none");
        EXPECT_FALSE(fs::exists(rig.stageDirectory()));
    }
    {
        // Skipped: what CHECK does not offer, DOWNLOAD does not fetch.
        Rig rig;
        rig.serveV2();
        const UpdateContext context = rig.context();
        ASSERT_TRUE(runUpdateLine(context, "UPDATE SKIP 0.10.0").ok());
        const core::Result<UpdateReply> reply = runUpdateLine(context, "UPDATE DOWNLOAD CONFIRM");
        ASSERT_TRUE(reply.ok()) << said(reply);
        EXPECT_EQ(reply->text, "update=current installed=0.9.0 latest=0.10.0 notes_url=" + kNotes +
                                   " reason=skipped\nstaged=none");
        EXPECT_TRUE(rig.feed.requestsFor(kPayloadUrl).empty());
    }
}

TEST(UpdateVerbs, NothingIsDownloadedToOrAppliedToADevelopmentCopy)
{
    Rig rig;
    rig.serveV2();
    rig.install = update::Install{};
    rig.install.version = update::parseVersion("0.9.0").value();
    const UpdateContext context = rig.context();

    const core::Result<UpdateReply> download = runUpdateLine(context, "UPDATE DOWNLOAD CONFIRM");
    ASSERT_TRUE(download.ok()) << said(download);
    const std::vector<std::string> lines = linesOf(download->text);
    ASSERT_EQ(lines.size(), 2u) << download->text;
    EXPECT_EQ(lines[0].rfind("update=unsupported installed=0.9.0 reason=development_tree", 0), 0u);
    EXPECT_EQ(lines[1], "staged=none");

    const core::Result<UpdateReply> apply = runUpdateLine(context, "UPDATE APPLY CONFIRM");
    ASSERT_FALSE(apply.ok()) << said(apply);
    EXPECT_EQ(apply.error().code, core::ErrorCode::Unsupported);
    EXPECT_EQ(apply.error().context, "reason=development_tree");

    EXPECT_TRUE(rig.feed.requests.empty());
    EXPECT_TRUE(rig.launcher.runs.empty());
    EXPECT_FALSE(fs::exists(rig.state));
}

TEST(UpdateVerbs, WithNoStateFolderThereIsNowhereToDownloadToAndNothingToApply)
{
    Rig rig;
    rig.serveV2();
    UpdateContext context = rig.context();
    context.stateDirectory.clear();

    const core::Result<UpdateReply> download = runUpdateLine(context, "UPDATE DOWNLOAD CONFIRM");
    ASSERT_TRUE(download.ok()) << said(download);
    EXPECT_EQ(download->text, "staged=none reason=no_state");
    EXPECT_TRUE(rig.feed.requests.empty()) << "nothing is asked for that could not be kept";

    const core::Result<UpdateReply> apply = runUpdateLine(context, "UPDATE APPLY CONFIRM");
    ASSERT_FALSE(apply.ok()) << said(apply);
    EXPECT_EQ(apply.error().context, "reason=no_state");
    EXPECT_FALSE(fs::exists(rig.state));
}

TEST(UpdateVerbs, ApplyWithoutADownloadedReleaseIsRefused)
{
    Rig rig;
    rig.serveV2();
    const core::Result<UpdateReply> reply = runUpdateLine(rig.context(), "UPDATE APPLY CONFIRM");
    ASSERT_FALSE(reply.ok()) << said(reply);
    EXPECT_EQ(reply.error().code, core::ErrorCode::InvalidState);
    EXPECT_EQ(reply.error().context, "reason=not_staged");
    EXPECT_TRUE(contains(reply.error().message, "UPDATE DOWNLOAD CONFIRM"));
    EXPECT_TRUE(rig.launcher.runs.empty()) << "no helper was copied or started";
    EXPECT_TRUE(rig.feed.requests.empty()) << "and an apply asks the feed for nothing";
}

// Where this test program is, as the system names it.
fs::path thisProgram()
{
    std::wstring name(32768, L'\0'); // the longest path Windows has
    const DWORD length =
        GetModuleFileNameW(nullptr, name.data(), static_cast<DWORD>(name.size()));
    name.resize(length);
    return fs::path(name);
}

// A program to stand where an install's helper is. A REAL one, although
// nothing here starts it - the launcher of every test is a fake: a file named
// .exe that is no program is what Windows answers with a message box the day
// something does start it, so no test makes one. It is the build's own helper
// when that is built (the test programs are in bin/tests, the helper in bin),
// and otherwise this test program, which is always there.
const fs::path& aRealProgram()
{
    static const fs::path program = [] {
        const fs::path self = thisProgram();
        const fs::path helper = self.parent_path().parent_path() / "katana_updater.exe";
        std::error_code error;
        return fs::is_regular_file(helper, error) && !error ? helper : self;
    }();
    return program;
}

// The helper of the install in `root`: R/bin/katana_updater.exe.
fs::path putAHelperIn(const fs::path& root)
{
    const fs::path helper = update::installedProgram(root, "katana_updater");
    std::error_code error;
    fs::create_directories(helper.parent_path(), error);
    fs::copy_file(aRealProgram(), helper, fs::copy_options::overwrite_existing, error);
    EXPECT_FALSE(error) << "the helper could not be put in the scratch install: "
                        << error.message();
    return helper;
}

// What `transaction=` of an APPLY's printed record says: "ready=<v> transaction=<tx>".
std::string transactionOf(const std::string& printed)
{
    constexpr std::string_view kKey = " transaction=";
    const std::size_t at = printed.find(kKey);
    if (at == std::string::npos) {
        return {};
    }
    const std::size_t from = at + kKey.size();
    return printed.substr(from, printed.find_first_of("\r\n", from) - from);
}

// The lines of U/cleanup.list as paths: what is listed for the sweep
// (update/state.hpp: one absolute path to a line, UTF-8).
std::vector<fs::path> listedForTheSweep(const fs::path& state)
{
    std::vector<fs::path> listed;
    if (!fs::exists(state / "cleanup.list")) {
        return listed;
    }
    for (const std::string& line : linesOf(bytesOf(state / "cleanup.list"))) {
        listed.push_back(core::pathFromUtf8(line));
    }
    return listed;
}

TEST(UpdateVerbs, ApplyPreparesTheHelperAndHandsBackTheRequestChangingNothingInTheInstall)
{
    Rig rig;
    rig.serveV2();
    const fs::path installedHelper = putAHelperIn(rig.root);
    UpdateContext context = rig.context();
    context.pid = 4242;
    context.relaunch = rig.root / "bin" / "katana.exe";
    ASSERT_TRUE(runUpdateLine(context, "UPDATE DOWNLOAD CONFIRM").ok());
    const std::map<std::string, std::string> installed = treeOf(rig.root);
    rig.feed.requests.clear();

    const core::Result<UpdateReply> reply = runUpdateLine(context, "UPDATE APPLY CONFIRM");
    ASSERT_TRUE(reply.ok()) << said(reply);
    ASSERT_TRUE(reply->apply.has_value());
    const update::ApplyRequest& request = *reply->apply;
    EXPECT_EQ(request.root, rig.root);
    EXPECT_EQ(request.stageDirectory, rig.stageDirectory());
    EXPECT_EQ(request.stateDirectory, rig.state);
    EXPECT_TRUE(update::isValidTransactionId(request.transaction)) << request.transaction;
    EXPECT_EQ(request.waitPid, 4242u);
    EXPECT_EQ(request.relaunch, rig.root / "bin" / "katana.exe");
    EXPECT_EQ(reply->text, "ready=0.10.0 transaction=" + request.transaction);

    // The helper that will run the swap is a copy outside the install, and it
    // was asked to prove it starts - by the launcher the context gave.
    EXPECT_EQ(request.helper,
              rig.state / ("run-" + request.transaction) / installedHelper.filename());
    EXPECT_TRUE(bytesOf(request.helper) == bytesOf(installedHelper))
        << "the copy is the install's helper, byte for byte";
    ASSERT_EQ(rig.launcher.runs.size(), 1u);
    EXPECT_EQ(rig.launcher.runs[0].executable, request.helper);
    EXPECT_EQ(rig.launcher.runs[0].arguments, std::vector<std::string>{"--selftest"});

    // The line itself starts nothing and changes nothing: the caller ends its
    // session first, and launches then.
    EXPECT_TRUE(rig.launcher.detached.empty());
    EXPECT_EQ(treeOf(rig.root), installed);
    EXPECT_FALSE(fs::exists(rig.root / ".update"));
    EXPECT_TRUE(rig.feed.requests.empty()) << "an apply asks the feed for nothing";

    // What the caller then does with the request: the helper copy, started
    // detached with the request as its command line (update/apply.hpp).
    ASSERT_TRUE(update::launchApply(request, rig.launcher).ok());
    ASSERT_EQ(rig.launcher.detached.size(), 1u);
    EXPECT_EQ(rig.launcher.detached[0].executable, request.helper);
    EXPECT_EQ(rig.launcher.detached[0].arguments, update::helperArguments(request));
}

TEST(UpdateVerbs, ARequestSaysWhoIsWaitedForAndWhatIsStartedAgainAsItsContextDid)
{
    // A context that names nobody to wait for and nothing to start again
    // makes a request that says so to the helper (update/apply.hpp).
    Rig rig;
    rig.serveV2();
    putAHelperIn(rig.root);
    const UpdateContext context = rig.context();
    ASSERT_TRUE(runUpdateLine(context, "UPDATE DOWNLOAD CONFIRM").ok());
    const core::Result<UpdateReply> reply = runUpdateLine(context, "update apply confirm");
    ASSERT_TRUE(reply.ok()) << said(reply);
    ASSERT_TRUE(reply->apply.has_value());
    EXPECT_TRUE(reply->apply->relaunch.empty());
    EXPECT_EQ(reply->apply->waitPid, 0u);
}

// ---- katana_cli's own path: the session ---------------------------------------------------------

TEST(UpdateVerbs, KatanaClisSessionPrintsADownloadsProgressOnceAndKeepsThePreparedApplyToBeTaken)
{
    Rig rig;
    rig.serveV2();
    putAHelperIn(rig.root);
    std::optional<update::ApplyRequest> request;
    {
        CliSession cli(rig);
        std::string downloaded;
        {
            Captured captured;
            ASSERT_TRUE(cli.session.run("UPDATE DOWNLOAD CONFIRM")) << captured.err();
            downloaded = captured.out();
            EXPECT_TRUE(captured.err().empty()) << captured.err();
        }
        // Progress lines, then what is staged: the verbs' reply, a line each.
        const std::vector<std::string> lines = linesOf(downloaded);
        ASSERT_GE(lines.size(), 2u) << downloaded;
        EXPECT_EQ(lines.back(), "staged=0.10.0");
        EXPECT_EQ(lines[lines.size() - 2], "progress bytes=2949/2949 files=5/5");
        for (std::size_t at = 0; at + 1 < lines.size(); ++at) {
            EXPECT_EQ(lines[at].rfind("progress bytes=", 0), 0u) << lines[at];
        }
        // Each once. A progress line is printed as it happens AND is in the
        // reply, which is printed when the line has run; and no two lines of
        // one download are the same, for one is said only when the bytes or
        // the files have moved on. So a line seen twice was printed twice.
        std::vector<std::string> sorted = lines;
        std::sort(sorted.begin(), sorted.end());
        EXPECT_TRUE(std::adjacent_find(sorted.begin(), sorted.end()) == sorted.end())
            << downloaded;

        std::string applied;
        {
            Captured captured;
            ASSERT_TRUE(cli.session.run("UPDATE APPLY CONFIRM")) << captured.err();
            applied = captured.out();
            EXPECT_TRUE(captured.err().empty()) << captured.err();
        }
        request = cli.session.takePendingApply();
        ASSERT_TRUE(request.has_value()) << "the line that succeeded left its request";
        EXPECT_EQ(applied, "ready=0.10.0 transaction=" + request->transaction + "\n");
        EXPECT_FALSE(cli.session.takePendingApply().has_value()) << "handed over once";
    }

    // What katana_cli's request tells the helper: wait for THIS process - the
    // one that ran the line - and start nothing afterwards.
    EXPECT_EQ(request->waitPid, static_cast<std::uint64_t>(GetCurrentProcessId()));
    EXPECT_TRUE(request->relaunch.empty());
    EXPECT_EQ(request->root, rig.root);
    EXPECT_EQ(request->stageDirectory, rig.stageDirectory());
    EXPECT_EQ(request->stateDirectory, rig.state);
    EXPECT_TRUE(update::isValidTransactionId(request->transaction)) << request->transaction;

    // The session started nothing: its front end does, once the session has
    // ended. And a request that was taken is the front end's to launch - the
    // session that ended did not give its helper copy up.
    EXPECT_TRUE(rig.launcher.detached.empty());
    EXPECT_TRUE(fs::exists(request->helper));
    EXPECT_TRUE(listedForTheSweep(rig.state).empty());
}

TEST(UpdateVerbs, ASessionWhoseApplyIsRefusedLeavesNothingToLaunch)
{
    Rig rig;
    rig.serveV2();
    putAHelperIn(rig.root);
    CliSession cli(rig);
    {
        // Nothing is downloaded yet.
        Captured captured;
        EXPECT_FALSE(cli.session.run("UPDATE APPLY CONFIRM"));
        EXPECT_TRUE(contains(captured.err(), "reason=not_staged")) << captured.err();
        EXPECT_TRUE(captured.out().empty()) << captured.out();
    }
    EXPECT_FALSE(cli.session.takePendingApply().has_value());

    {
        Captured captured;
        ASSERT_TRUE(cli.session.run("UPDATE DOWNLOAD CONFIRM")) << captured.err();
    }
    {
        // Downloaded, and the helper does not prove that it starts.
        rig.launcher.exitCode = 3;
        Captured captured;
        EXPECT_FALSE(cli.session.run("UPDATE APPLY CONFIRM"));
        EXPECT_TRUE(contains(captured.err(), "reason=helper_failed")) << captured.err();
        EXPECT_FALSE(contains(captured.out(), "ready=")) << captured.out();
    }
    EXPECT_FALSE(cli.session.takePendingApply().has_value());
    EXPECT_TRUE(rig.launcher.detached.empty());
}

TEST(UpdateVerbs, AnApplyNobodyLaunchesHasItsHelperCopyListedForTheSweep)
{
    Rig rig;
    rig.serveV2();
    putAHelperIn(rig.root);
    const UpdateContext context = rig.context();
    ASSERT_TRUE(runUpdateLine(context, "UPDATE DOWNLOAD CONFIRM").ok());
    const core::Result<UpdateReply> reply = runUpdateLine(context, "UPDATE APPLY CONFIRM");
    ASSERT_TRUE(reply.ok()) << said(reply);
    ASSERT_TRUE(reply->apply.has_value());
    const update::ApplyRequest& request = *reply->apply;
    const fs::path runFolder = rig.state / ("run-" + request.transaction);
    ASSERT_EQ(request.helper.parent_path(), runFolder);
    ASSERT_TRUE(fs::exists(request.helper));
    // A request that may yet be launched is listed nowhere.
    EXPECT_TRUE(listedForTheSweep(rig.state).empty());

    // Its caller will not launch it: the copy is listed (update/state.hpp),
    // not removed here, and the release stays downloaded.
    ASSERT_TRUE(katana::app::abandonApply(request).ok());
    EXPECT_EQ(listedForTheSweep(rig.state), std::vector<fs::path>{runFolder});
    EXPECT_TRUE(fs::exists(request.helper));
    const core::Result<UpdateReply> status = runUpdateLine(context, "UPDATE STATUS");
    ASSERT_TRUE(status.ok()) << said(status);
    EXPECT_TRUE(contains(status->text, "staged=0.10.0")) << status->text;

    // And the line is one the sweep of the next start takes for its own
    // (update/state.hpp): the folder goes - or, should a scanner be holding
    // the new copy open at this moment, it stays listed for the start after.
    // A line the sweep did not take for its own would be dropped, with the
    // folder still there.
    const std::unique_ptr<update::FsOps> files = update::makeSystemFsOps();
    ASSERT_TRUE(update::processCleanupList(rig.state, rig.root, *files).ok());
    const std::vector<fs::path> left = listedForTheSweep(rig.state);
    EXPECT_TRUE(!fs::exists(runFolder) || left == std::vector<fs::path>{runFolder})
        << "the folder is still there and is no longer listed";
    EXPECT_TRUE(fs::exists(update::stagedManifestPath(rig.stageDirectory())));

    // A request that is none - nothing was prepared for it - lists nothing.
    EXPECT_TRUE(katana::app::abandonApply(update::ApplyRequest{}).ok());
    EXPECT_EQ(listedForTheSweep(rig.state), left);
}

TEST(UpdateVerbs, ASessionGivesUpAPreparedApplyThatALaterOneReplacesOrThatNobodyTook)
{
    Rig rig;
    rig.serveV2();
    putAHelperIn(rig.root);
    std::string first;
    std::string second;
    {
        CliSession cli(rig);
        {
            Captured captured;
            ASSERT_TRUE(cli.session.run("UPDATE DOWNLOAD CONFIRM")) << captured.err();
        }
        {
            Captured captured;
            ASSERT_TRUE(cli.session.run("UPDATE APPLY CONFIRM")) << captured.err();
            first = transactionOf(captured.out());
        }
        ASSERT_TRUE(update::isValidTransactionId(first)) << first;
        EXPECT_TRUE(listedForTheSweep(rig.state).empty()) << "it may yet be taken and launched";

        // A second apply is prepared before anybody took the first: one is
        // launched at most, so the first's helper copy is nobody's.
        {
            Captured captured;
            ASSERT_TRUE(cli.session.run("UPDATE APPLY CONFIRM")) << captured.err();
            second = transactionOf(captured.out());
        }
        ASSERT_TRUE(update::isValidTransactionId(second)) << second;
        ASSERT_NE(first, second);
        EXPECT_EQ(listedForTheSweep(rig.state),
                  std::vector<fs::path>{rig.state / ("run-" + first)});
    }
    // The session ended and nobody took the second: that one is given up too.
    EXPECT_EQ(listedForTheSweep(rig.state),
              (std::vector<fs::path>{rig.state / ("run-" + first), rig.state / ("run-" + second)}));
    EXPECT_TRUE(rig.launcher.detached.empty());
}

TEST(UpdateVerbs, ApplyIsRefusedWhenTheStateFileDoesNotReadForItIsWhereWhatWasPutBackIsKept)
{
    Rig rig;
    rig.serveV2();
    putAHelperIn(rig.root);
    const UpdateContext context = rig.context();
    ASSERT_TRUE(runUpdateLine(context, "UPDATE DOWNLOAD CONFIRM").ok());
    // Damaged, or written by a newer Katana: either way the list of the
    // releases that were put back here cannot be read out of it.
    const std::string damaged = "this is not a state file";
    writeBytes(rig.state / "state.json", damaged);

    const core::Result<UpdateReply> reply = runUpdateLine(context, "UPDATE APPLY CONFIRM");
    ASSERT_FALSE(reply.ok()) << said(reply);
    EXPECT_EQ(reply.error().code, core::ErrorCode::InvalidState);
    EXPECT_EQ(reply.error().context, "reason=state_unreadable");
    EXPECT_TRUE(contains(reply.error().message, "Katana 0.10.0")) << reply.error().message;
    EXPECT_TRUE(rig.launcher.runs.empty()) << "no helper was copied or started";
    EXPECT_EQ(bytesOf(rig.state / "state.json"), damaged) << "and the file is left as it is";

    // With the file gone the defaults are the truth again - nothing was put
    // back here - and the release is applied.
    ASSERT_TRUE(fs::remove(rig.state / "state.json"));
    const core::Result<UpdateReply> again = runUpdateLine(context, "UPDATE APPLY CONFIRM");
    ASSERT_TRUE(again.ok()) << said(again);
    EXPECT_TRUE(again->apply.has_value());
}

TEST(UpdateVerbs, AFolderWhoseNameIsNoUnicodeTextInTheUpdateFolderIsPassedOver)
{
    // What a Windows file system lets a name be and UTF-8 cannot say: a
    // surrogate with no partner, U+D800 alone. Turning such a name into text
    // throws (core/path_text.hpp), and the update folder is the user's to
    // write: with one such folder in it, STATUS and APPLY ended in an
    // exception that nothing caught.
    Rig rig;
    rig.serveV2();
    putAHelperIn(rig.root);
    const UpdateContext context = rig.context();
    ASSERT_TRUE(runUpdateLine(context, "UPDATE DOWNLOAD CONFIRM").ok());
    std::wstring name = L"stage-";
    name.push_back(static_cast<wchar_t>(0xD800));
    std::error_code refused;
    if (!fs::create_directory(rig.state / name, refused)) {
        GTEST_SKIP() << "this file system holds no such name: " << refused.message();
    }

    const core::Result<UpdateReply> status = runUpdateLine(context, "UPDATE STATUS");
    ASSERT_TRUE(status.ok()) << said(status);
    EXPECT_TRUE(contains(status->text, "staged=0.10.0")) << status->text;
    const core::Result<UpdateReply> apply = runUpdateLine(context, "UPDATE APPLY CONFIRM");
    ASSERT_TRUE(apply.ok()) << said(apply);
    EXPECT_EQ(apply->text.rfind("ready=0.10.0 transaction=", 0), 0u) << apply->text;
}

TEST(UpdateVerbs, NothingIsDownloadedToOrAppliedToACopyWhoseInstallRecordDoesNotRead)
{
    Rig rig;
    rig.serveV2();
    UpdateContext context = rig.context();
    context.installProblem = "ParseFailure: install.json is not a record";

    const core::Result<UpdateReply> download = runUpdateLine(context, "UPDATE DOWNLOAD CONFIRM");
    ASSERT_TRUE(download.ok()) << said(download);
    EXPECT_EQ(download->text,
              "update=unsupported installed=0.9.0 reason=install_record "
              "detail=\"ParseFailure: install.json is not a record\"\nstaged=none");

    const core::Result<UpdateReply> apply = runUpdateLine(context, "UPDATE APPLY CONFIRM");
    ASSERT_FALSE(apply.ok()) << said(apply);
    EXPECT_EQ(apply.error().code, core::ErrorCode::Unsupported);
    EXPECT_EQ(apply.error().context, "reason=install_record");
    EXPECT_TRUE(contains(apply.error().message, "ParseFailure: install.json is not a record"))
        << apply.error().message;

    EXPECT_TRUE(rig.feed.requests.empty());
    EXPECT_TRUE(rig.launcher.runs.empty());
    EXPECT_FALSE(fs::exists(rig.state));
}

TEST(UpdateVerbs, ADownloadThatCannotCompareTheInstalledFilesFetchesNothingAndSaysWhy)
{
    // As for CHECK above: a folder where a file of the release goes. What to
    // fetch cannot be worked out, so nothing is.
    Rig rig;
    rig.serveV2();
    const fs::path readme = rig.root / "share" / "katana" / "readme.txt";
    ASSERT_TRUE(fs::remove(readme));
    ASSERT_TRUE(fs::create_directory(readme));

    const core::Result<UpdateReply> reply = runUpdateLine(rig.context(), "UPDATE DOWNLOAD CONFIRM");
    ASSERT_FALSE(reply.ok()) << said(reply);
    EXPECT_EQ(reply.error().context.rfind("reason=plan_failed", 0), 0u) << reply.error().context;
    EXPECT_EQ(reply.error().message.rfind("what the update would change could not be worked out, "
                                          "so nothing was downloaded: ",
                                          0),
              0u)
        << reply.error().message;
    EXPECT_TRUE(contains(reply.error().context, "share/katana/readme.txt"))
        << reply.error().context;
    EXPECT_TRUE(rig.feed.requestsFor(kPayloadUrl).empty());
    EXPECT_FALSE(fs::exists(rig.stageDirectory()));
}

TEST(UpdateVerbs, ADownloadThatWasCancelledBeforeItBeganIsAnErrorAndFetchesNothing)
{
    Rig rig;
    rig.serveV2();
    std::stop_source cancelled;
    cancelled.request_stop();
    UpdateContext context = rig.context();
    context.stop = cancelled.get_token();
    const core::Result<UpdateReply> reply = runUpdateLine(context, "UPDATE DOWNLOAD CONFIRM");
    ASSERT_FALSE(reply.ok()) << said(reply);
    EXPECT_EQ(reply.error().code, core::ErrorCode::InvalidState);
    EXPECT_EQ(reply.error().message, "cancelled");
    EXPECT_TRUE(rig.feed.requests.empty());
    EXPECT_FALSE(fs::exists(rig.state));
}

TEST(UpdateVerbs, ApplySaysWhyWhenTheHelperDoesNotProveItStarts)
{
    Rig rig;
    rig.serveV2();
    putAHelperIn(rig.root);
    const UpdateContext context = rig.context();
    ASSERT_TRUE(runUpdateLine(context, "UPDATE DOWNLOAD CONFIRM").ok());
    rig.launcher.exitCode = 3;
    const core::Result<UpdateReply> reply = runUpdateLine(context, "UPDATE APPLY CONFIRM");
    ASSERT_FALSE(reply.ok()) << said(reply);
    // stage.hpp, the preflight's tokens.
    EXPECT_EQ(reply.error().context.rfind("reason=helper_failed", 0), 0u) << reply.error().context;
    EXPECT_TRUE(rig.launcher.detached.empty());

    // And with no helper in the install at all.
    Rig bare;
    bare.serveV2();
    const UpdateContext bareContext = bare.context();
    ASSERT_TRUE(runUpdateLine(bareContext, "UPDATE DOWNLOAD CONFIRM").ok());
    const core::Result<UpdateReply> missing = runUpdateLine(bareContext, "UPDATE APPLY CONFIRM");
    ASSERT_FALSE(missing.ok()) << said(missing);
    EXPECT_EQ(missing.error().context.rfind("reason=helper_missing", 0), 0u)
        << missing.error().context;
    EXPECT_TRUE(bare.launcher.runs.empty());
}

TEST(UpdateVerbs, ApplyRefusesAReleaseThatWasRolledBackHere)
{
    Rig rig;
    rig.serveV2();
    putAHelperIn(rig.root);
    const UpdateContext context = rig.context();
    ASSERT_TRUE(runUpdateLine(context, "UPDATE DOWNLOAD CONFIRM").ok());
    // What the helper records when the new version did not start and the old
    // one was put back (update/state.hpp, `blocked`).
    core::Result<update::UpdateState> state = update::readState(rig.state);
    ASSERT_TRUE(state.ok());
    state->blockedSequences.push_back(kV2Sequence);
    ASSERT_TRUE(update::writeState(rig.state, *state).ok());

    const core::Result<UpdateReply> reply = runUpdateLine(context, "UPDATE APPLY CONFIRM");
    ASSERT_FALSE(reply.ok()) << said(reply);
    EXPECT_EQ(reply.error().context, "reason=blocked_before");
    EXPECT_TRUE(contains(reply.error().message, "Katana 0.10.0"));
    EXPECT_TRUE(rig.launcher.runs.empty());
}

TEST(UpdateVerbs, AStageThatIsNotAboveWhatIsInstalledIsNotReadyToApply)
{
    // What a finished update can leave behind: the stage of the release that
    // is now installed. It is not offered as staged, and is not applied.
    Rig rig;
    rig.serveV2();
    putAHelperIn(rig.root);
    ASSERT_TRUE(runUpdateLine(rig.context(), "UPDATE DOWNLOAD CONFIRM").ok());
    rig.installV2();
    const UpdateContext context = rig.context();
    const core::Result<UpdateReply> status = runUpdateLine(context, "UPDATE STATUS");
    ASSERT_TRUE(status.ok()) << said(status);
    EXPECT_TRUE(contains(status->text, "installed=0.10.0")) << status->text;
    EXPECT_TRUE(contains(status->text, "staged=none")) << status->text;
    const core::Result<UpdateReply> apply = runUpdateLine(context, "UPDATE APPLY CONFIRM");
    ASSERT_FALSE(apply.ok()) << said(apply);
    EXPECT_EQ(apply.error().context, "reason=not_staged");
}

TEST(UpdateVerbs, AStageWhoseManifestWasChangedIsNotReadyToApply)
{
    // The stage folder is the user's to write. One byte of the staged
    // manifest changed, and the signature no longer covers it.
    Rig rig;
    rig.serveV2();
    putAHelperIn(rig.root);
    const UpdateContext context = rig.context();
    ASSERT_TRUE(runUpdateLine(context, "UPDATE DOWNLOAD CONFIRM").ok());
    const fs::path manifest = update::stagedManifestPath(rig.stageDirectory());
    std::string bytes = bytesOf(manifest);
    const std::size_t at = bytes.find("0.10.0");
    ASSERT_NE(at, std::string::npos);
    bytes[at + 2] = '9'; // 0.90.0
    writeBytes(manifest, bytes);

    const core::Result<UpdateReply> status = runUpdateLine(context, "UPDATE STATUS");
    ASSERT_TRUE(status.ok()) << said(status);
    EXPECT_TRUE(contains(status->text, "staged=none")) << status->text;
    const core::Result<UpdateReply> apply = runUpdateLine(context, "UPDATE APPLY CONFIRM");
    ASSERT_FALSE(apply.ok()) << said(apply);
    EXPECT_EQ(apply.error().context, "reason=not_staged");
    EXPECT_TRUE(rig.launcher.runs.empty());
}

#endif // _WIN32

} // namespace
