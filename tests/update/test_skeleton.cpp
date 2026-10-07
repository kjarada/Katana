// What the update skeleton itself promises, which no later task owns: the version
// reaches the library, the test keys are the RFC's and nothing a build pins, and the
// seams of the contract are interfaces a test double can implement and the apply
// services can be made of. The behaviour behind each seam is the later tasks' (their
// own test files, beside this one); these cases stay true when every stub is gone.

#include <gtest/gtest.h>

#include <array>
#include <cctype>
#include <cstdint>
#include <filesystem>
#include <memory>
#include <string>
#include <vector>

#include "katana/update/apply.hpp"
#include "katana/update/fault_fs.hpp"
#include "katana/update/journal.hpp"
#include "katana/update/version.hpp"
#include "test_key.hpp"

using namespace katana;

#ifndef KATANA_EXPECTED_VERSION
#error "tests/update/CMakeLists.txt passes the project version as KATANA_EXPECTED_VERSION"
#endif

namespace {

bool isDigit(char c)
{
    return std::isdigit(static_cast<unsigned char>(c)) != 0;
}

} // namespace

TEST(UpdateSkeleton, TheProgramVersionIsTheOneProjectWritesAndTheReleaseWorkflowCanRead)
{
    // project(Katana VERSION ...) reaches the library as KATANA_VERSION: katana_cli has no
    // definition of its own, which is why the library carries one.
    EXPECT_EQ(update::programVersionText(), std::string_view(KATANA_EXPECTED_VERSION));

    // And it has the form release.yml's sed reads, digits and dots (MAJOR.MINOR to
    // MAJOR.MINOR.PATCH.BUILD), worked out here and not by the parser under test.
    const std::string_view text = update::programVersionText();
    ASSERT_FALSE(text.empty());
    int dots = 0;
    bool digitBeforeEveryDot = true;
    char previous = '.';
    for (const char c : text) {
        if (c == '.') {
            ++dots;
            digitBeforeEveryDot = digitBeforeEveryDot && isDigit(previous);
        } else {
            EXPECT_TRUE(isDigit(c)) << "not a digit: " << c;
        }
        previous = c;
    }
    EXPECT_GE(dots, 1);
    EXPECT_LE(dots, 3);
    EXPECT_TRUE(digitBeforeEveryDot);
    EXPECT_TRUE(isDigit(text.back()));
}

TEST(UpdateSkeleton, TheTestKeysAreRfc8032VectorsOneAndTwoNotKeysAnyBuildPins)
{
    // RFC 8032, section 7.1, TEST 1 and TEST 2, public keys byte by byte as the RFC prints
    // them (and as OpenSSL derives them from the seeds in test_key.hpp).
    constexpr std::array<std::uint8_t, 32> kTest1 = {
        0xd7, 0x5a, 0x98, 0x01, 0x82, 0xb1, 0x0a, 0xb7, 0xd5, 0x4b, 0xfe,
        0xd3, 0xc9, 0x64, 0x07, 0x3a, 0x0e, 0xe1, 0x72, 0xf3, 0xda, 0xa6,
        0x23, 0x25, 0xaf, 0x02, 0x1a, 0x68, 0xf7, 0x07, 0x51, 0x1a};
    constexpr std::array<std::uint8_t, 32> kTest2 = {
        0x3d, 0x40, 0x17, 0xc3, 0xe8, 0x43, 0x89, 0x5a, 0x92, 0xb7, 0x0a,
        0xa7, 0x4d, 0x1b, 0x7e, 0xbc, 0x9c, 0x98, 0x2c, 0xcf, 0x2e, 0xc4,
        0x96, 0x8c, 0xc0, 0xcd, 0x55, 0xf1, 0x2a, 0xf4, 0x66, 0x0c};

    const update::Trust& trust = update::testTrust();
    EXPECT_FALSE(trust.placeholder);
    ASSERT_EQ(trust.keys.size(), 2u);
    EXPECT_EQ(trust.keys[0].key, kTest1);
    EXPECT_EQ(trust.keys[1].key, kTest2);
    // The key id is the first 16 hex digits of the key, as signature.hpp says.
    EXPECT_EQ(trust.keys[0].id, "d75a980182b10ab7");
    EXPECT_EQ(trust.keys[1].id, "3d4017c3e843895a");
}

namespace {

using Path = std::filesystem::path;

// The seams of the contract, each as a double a test would write: they implement every
// pure virtual and nothing else, so a function added to an interface without a thought
// for its fakes breaks this file's build.
class FakeFs final : public update::FsOps {
  public:
    core::Result<update::EntryInfo> stat(const Path&) const override
    {
        return update::EntryInfo{};
    }
    core::Result<std::vector<Path>> listDirectory(const Path&) const override
    {
        return std::vector<Path>{};
    }
    core::Result<std::string> readFile(const Path&) const override { return std::string{}; }
    core::Result<Path> resolve(const Path& path) const override { return path; }
    core::Result<std::uint64_t> freeSpace(const Path&) const override
    {
        return std::uint64_t{1} << 40;
    }
    core::Status makeDirectories(const Path&) override { return {}; }
    core::Status copyFile(const Path&, const Path&) override { return {}; }
    core::Status renameFile(const Path&, const Path&) override { return {}; }
    core::Status removeFile(const Path&) override { return {}; }
    core::Status removeEmptyDirectory(const Path&) override { return {}; }
    core::Status appendDurable(const Path&, std::string_view) override { return {}; }
    core::Status writeDurable(const Path&, std::string_view) override { return {}; }
    core::Status setExecutable(const Path&) override { return {}; }
    core::Status syncDirectory(const Path&) override { return {}; }
    core::Result<std::unique_ptr<update::FileLock>> tryLock(const Path&) override
    {
        return std::unique_ptr<update::FileLock>(std::make_unique<update::FileLock>());
    }
};

class FakeProcesses final : public update::ProcessOps {
  public:
    std::uint64_t currentPid() const override { return 1; }
    bool isAlive(std::uint64_t) const override { return false; }
    core::Result<std::vector<update::RunningProgram>> holdersUnder(const Path&) const override
    {
        return std::vector<update::RunningProgram>{};
    }
};

class FakeLauncher final : public update::Launcher {
  public:
    core::Result<update::SpawnResult> run(const update::SpawnRequest&) override
    {
        return update::SpawnResult{};
    }
    core::Status startDetached(const update::SpawnRequest&) override { return {}; }
};

class FakeRegistry final : public update::RegistryOps {
  public:
    core::Result<std::optional<Path>> installerLocation() const override
    {
        return std::optional<Path>{};
    }
    core::Status setRunOnce(std::string_view, std::string_view) override { return {}; }
    core::Status clearRunOnce(std::string_view) override { return {}; }
    core::Status setInstalledVersion(std::string_view, std::string_view) override { return {}; }
};

} // namespace

TEST(UpdateSkeleton, TheApplyServicesAreMadeOfSeamsATestCanFake)
{
    FakeFs fs;
    FakeProcesses processes;
    FakeLauncher launcher;
    FakeRegistry registry;

    // The helper's trust is a parameter: here the test key, as in katana_updater_test.
    const update::ApplyServices services{fs, processes, launcher, registry, update::testTrust()};
    EXPECT_EQ(&services.fs, &fs);
    EXPECT_EQ(&services.trust, &update::testTrust());
    // The design's waits, which a test shortens by passing its own timings.
    EXPECT_EQ(services.timings.waitForParent.count(), 60'000);
    EXPECT_EQ(services.timings.waitForHolders.count(), 30'000);
    EXPECT_EQ(services.timings.healthWait.count(), 60'000);
}

TEST(UpdateSkeleton, TheFaultInjectorAndTheJournalWrapTheSameSeamTheApplyUses)
{
    FakeFs fs;
    // Both take the FsOps they are given and keep it: the fault loops put a FaultFs
    // between the apply and the disk, and the journal writes through whatever it is
    // handed, so a crash is seen at every journal write too.
    update::FaultFs faults(fs);
    update::Journal journal(faults, Path("journal"));
    EXPECT_EQ(journal.path(), Path("journal"));
}
