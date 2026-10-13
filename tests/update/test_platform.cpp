// What Linux and macOS do with an update where a copy of Katana does not replace its own files:
// a download, a preflight and the start of the helper are REFUSED for the platform, nothing is
// fetched and nothing is written - and what a program may still ask of the process seam there is
// answered. Windows does not compile this file (tests/update/CMakeLists.txt adds it where the
// system is not Windows): its seams are the real ones and have tests of their own
// (test_journal.cpp, test_helper.cpp). The helper program's own refusal is held by
// check_platform_refusal.cmake, beside this file.
//
// WHERE THE EXPECTATIONS COME FROM (CLAUDE.md, section 4) - none is the program's output read
// back:
//   - what these platforms get is settled outside the code: they check for a release and tell
//     of it, and an install of it is refused as unsupported, for the platform, changing nothing.
//     The code of that refusal is core::ErrorCode::Unsupported, and its token is `platform`,
//     the one the UPDATE verbs say for a system that does not install (reason=platform). The
//     token is read with update::fetchFailureReason, the function a caller reads it with;
//   - "nothing was fetched" is a count kept by the transport this file hands over, "no program
//     was run" a count kept by its launcher, and "nothing was written" is std::filesystem asked
//     directly - never the file system under test;
//   - this process's id is the operating system's own answer, getpid(); process 1 exists on
//     every such system (init, launchd), and kill(2) says EPERM of it to anyone who is not root;
//     a child that has exited and has been waited for is gone, by waitpid(2);
//   - 0 and a number wider than a process id are not alive by the contract (process.hpp).
//
// WHEN TASK T18 WRITES THE FILE SYSTEM AND THE LAUNCHER FOR LINUX (src/katana_update/
// fsops_posix.cpp, spawn_posix.cpp), these cases part ways:
//   - PlatformWithoutApply.ADownload... and .APreflight... then fail on Linux, as they should,
//     and STAY FOR macOS (under #ifdef __APPLE__): a Mac refuses for good, whatever that file
//     answers, and these are the cases that fail if writing it turned downloads on inside a
//     sealed bundle;
//   - SeamsNotWritten.* state what a seam does while it is unwritten, and give way to the tests
//     of what was written;
//   - ProcessSeam.* hold on both systems, before and after.

#include <gtest/gtest.h>

#include <cerrno>
#include <chrono>
#include <cstdint>
#include <filesystem>
#include <memory>
#include <optional>
#include <stop_token>
#include <string>
#include <system_error>
#include <vector>

#include <sys/types.h>
#include <sys/wait.h>
#include <unistd.h>

#include "katana/core/error.hpp"
#include "katana/update/apply.hpp"
#include "katana/update/feed.hpp"
#include "katana/update/fsops.hpp"
#include "katana/update/install.hpp"
#include "katana/update/manifest.hpp"
#include "katana/update/plan.hpp"
#include "katana/update/process.hpp"
#include "katana/update/stage.hpp"

using namespace katana;
namespace fs = std::filesystem;

namespace {

// A path under the temporary folder that does NOT exist: the cases prove that it still does
// not when they end. Removed all the same, should a case fail by making it.
class Nowhere {
  public:
    Nowhere()
        : path_(fs::temp_directory_path() /
                ("katana_px_" + std::to_string(::getpid()) + "_" +
                 std::to_string(std::chrono::steady_clock::now().time_since_epoch().count())))
    {
    }
    Nowhere(const Nowhere&) = delete;
    Nowhere& operator=(const Nowhere&) = delete;
    ~Nowhere()
    {
        std::error_code ignored;
        fs::remove_all(path_, ignored);
    }
    [[nodiscard]] const fs::path& path() const { return path_; }
    [[nodiscard]] bool wasMade() const
    {
        std::error_code ignored;
        return fs::exists(fs::symlink_status(path_, ignored));
    }

  private:
    fs::path path_;
};

// The code a call was refused with; nothing when it was not refused.
template <typename T> std::optional<core::ErrorCode> refusalOf(const core::Result<T>& result)
{
    return result.ok() ? std::nullopt : std::optional<core::ErrorCode>(result.error().code);
}

// The reason token of a refusal, as a caller reads it; a word no token is when there was none.
template <typename T> std::string reasonOf(const core::Result<T>& result)
{
    return result.ok() ? std::string("not refused") : update::fetchFailureReason(result.error());
}

constexpr std::optional<core::ErrorCode> kUnsupported = core::ErrorCode::Unsupported;

// A launcher that would let anything through, and counts what it was asked to start.
class CountingLauncher final : public update::Launcher {
  public:
    [[nodiscard]] core::Result<update::SpawnResult> run(const update::SpawnRequest&) override
    {
        ++runs;
        return update::SpawnResult{};
    }
    [[nodiscard]] core::Status startDetached(const update::SpawnRequest&) override
    {
        ++runs;
        return {};
    }
    int runs = 0;
};

} // namespace

// ---- the engine: no download and no preflight -----------------------------------------------

TEST(PlatformWithoutApply, ADownloadIsRefusedForThePlatformBeforeAnythingIsFetchedOrWritten)
{
    const Nowhere nowhere;
    int requests = 0;
    const update::Fetch fetch = [&requests](const update::FetchRequest&, std::stop_token,
                                            update::ProgressFn) -> core::Result<std::string> {
        ++requests;
        return core::makeError(core::ErrorCode::Internal, "the transport was asked");
    };
    // Neither is looked at: the refusal comes before either could matter.
    const update::Manifest manifest{};
    const update::Plan plan{};

    const core::Status staged = update::stage(fetch, update::FeedConfig{}, manifest, plan,
                                              nowhere.path() / "stage-1", std::stop_token{}, {});
    EXPECT_EQ(refusalOf(staged), kUnsupported);
    EXPECT_EQ(reasonOf(staged), "platform");
    EXPECT_EQ(requests, 0);
    EXPECT_FALSE(nowhere.wasMade());
}

TEST(PlatformWithoutApply, APreflightIsRefusedForThePlatformAndRunsNoProgram)
{
    const Nowhere nowhere;
    CountingLauncher launcher;
    update::Install install;
    install.root = nowhere.path() / "Katana";

    const core::Result<update::PreparedApply> prepared =
        update::preflight(update::Manifest{}, install, nowhere.path() / "stage-1",
                          nowhere.path() / "state", launcher);
    EXPECT_EQ(refusalOf(prepared), kUnsupported);
    EXPECT_EQ(reasonOf(prepared), "platform");
    EXPECT_EQ(launcher.runs, 0);
    EXPECT_FALSE(nowhere.wasMade()) << "no run folder is left, nor begun";
}

// ---- the seams that are not written ---------------------------------------------------------

TEST(SeamsNotWritten, EveryOperationOfTheSystemFileSystemIsRefusedAndMakesNothing)
{
    const Nowhere nowhere;
    const std::unique_ptr<update::FsOps> system = update::makeSystemFsOps();
    const fs::path folder = nowhere.path() / "folder";
    const fs::path file = nowhere.path() / "file";

    EXPECT_EQ(refusalOf(system->stat(file)), kUnsupported);
    EXPECT_EQ(refusalOf(system->listDirectory(nowhere.path())), kUnsupported);
    EXPECT_EQ(refusalOf(system->readFile(file)), kUnsupported);
    EXPECT_EQ(refusalOf(system->resolve(file)), kUnsupported);
    EXPECT_EQ(refusalOf(system->freeSpace(nowhere.path())), kUnsupported);
    EXPECT_EQ(refusalOf(system->makeDirectories(folder)), kUnsupported);
    EXPECT_EQ(refusalOf(system->copyFile(file, folder / "copy")), kUnsupported);
    EXPECT_EQ(refusalOf(system->renameFile(file, folder / "moved")), kUnsupported);
    EXPECT_EQ(refusalOf(system->removeFile(file)), kUnsupported);
    EXPECT_EQ(refusalOf(system->removeEmptyDirectory(folder)), kUnsupported);
    EXPECT_EQ(refusalOf(system->appendDurable(file, "a")), kUnsupported);
    EXPECT_EQ(refusalOf(system->writeDurable(file, "w")), kUnsupported);
    EXPECT_EQ(refusalOf(system->setExecutable(file)), kUnsupported);
    EXPECT_EQ(refusalOf(system->syncDirectory(nowhere.path())), kUnsupported);
    EXPECT_EQ(refusalOf(system->tryLock(nowhere.path() / "lock")), kUnsupported);
    EXPECT_FALSE(nowhere.wasMade()) << "a refusal that made a folder, a file or a lock";
}

TEST(SeamsNotWritten, TheSystemLauncherStartsNothingAndSaysItIsThePlatform)
{
    const std::unique_ptr<update::Launcher> launcher = update::makeSystemLauncher();
    update::SpawnRequest request;
    // A program that is there on every such system: nothing is missing but the launcher.
    request.executable = "/bin/sh";
    request.arguments = {"-c", "exit 0"};
    const core::Result<update::SpawnResult> ran = launcher->run(request);
    EXPECT_EQ(refusalOf(ran), kUnsupported);
    EXPECT_EQ(reasonOf(ran), "platform");
    const core::Status started = launcher->startDetached(request);
    EXPECT_EQ(refusalOf(started), kUnsupported);
    EXPECT_EQ(reasonOf(started), "platform");
}

TEST(SeamsNotWritten, StartingTheHelperIsRefusedForThePlatformAndNotAsADownloadTooLarge)
{
    // launchApply holds a launcher and no file system, so it is the launcher's refusal that
    // reaches its caller - under the token stage() and preflight() give, and not `too_large`,
    // which is what an Unsupported with no token of its own reads as (feed.hpp).
    update::ApplyRequest apply;
    apply.helper = "/bin/sh";
    apply.transaction = "a1b2c3d4";
    const core::Status launched = update::launchApply(apply);
    EXPECT_EQ(refusalOf(launched), kUnsupported);
    EXPECT_EQ(reasonOf(launched), "platform");
}

TEST(SeamsNotWritten, TheProgramsRunningFromAnInstallAreNotGuessedAt)
{
    // "None" from a scan that did not look would be read as "nothing is running, go on".
    const core::Result<std::vector<update::RunningProgram>> running =
        update::runningPrograms(fs::temp_directory_path());
    EXPECT_EQ(refusalOf(running), kUnsupported);
    EXPECT_EQ(reasonOf(running), "platform");
    const std::unique_ptr<update::ProcessOps> processes = update::makeSystemProcessOps();
    EXPECT_EQ(refusalOf(processes->holdersUnder(fs::temp_directory_path())), kUnsupported);
}

// ---- the process seam's two answers -----------------------------------------------------------

TEST(ProcessSeam, AProgramIsToldItsOwnProcessIdAndWhichProcessesAreAlive)
{
    const std::unique_ptr<update::ProcessOps> processes = update::makeSystemProcessOps();
    EXPECT_EQ(processes->currentPid(), static_cast<std::uint64_t>(::getpid()));
    EXPECT_TRUE(processes->isAlive(processes->currentPid()));
    EXPECT_TRUE(processes->isAlive(1)) << "a process this user may not signal is alive too";
    EXPECT_FALSE(processes->isAlive(0)) << "0 names a group to kill(), not a process";
    EXPECT_FALSE(processes->isAlive(std::uint64_t{1} << 40)) << "wider than any process id";
}

TEST(ProcessSeam, AProcessThatHasEndedAndBeenWaitedForIsNotAlive)
{
    const pid_t child = ::fork();
    ASSERT_NE(child, -1) << "fork: errno " << errno;
    if (child == 0) {
        ::_exit(0); // the child: nothing of the test runs in it
    }
    const std::unique_ptr<update::ProcessOps> processes = update::makeSystemProcessOps();
    // Until it is waited for it is there: running still, or ended and not yet reaped, which
    // counts as alive (process_scan_posix.cpp says why that is left so).
    EXPECT_TRUE(processes->isAlive(static_cast<std::uint64_t>(child)));

    int status = 0;
    pid_t waited = -1;
    do {
        waited = ::waitpid(child, &status, 0);
    } while (waited == -1 && errno == EINTR);
    ASSERT_EQ(waited, child) << "waitpid: errno " << errno;
    EXPECT_FALSE(processes->isAlive(static_cast<std::uint64_t>(child)));
}
