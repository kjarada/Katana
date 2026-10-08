// The records beside the programs (install.hpp: install.json, installed.json, the signed manifest
// kept for removals) and in the per-user state folder (state.hpp: state.json, last.json, the
// cleanup list), and the one function that finds the install root of the running program.
//
// Each expectation comes from the contract in the header, worked out by hand: the file shapes
// are the ones the headers print, the paths are joined by the rules there, and the numbers in
// the JSON are the ones a careful writer would put (2^64 - 1 is 18446744073709551615). The
// cleanup list is the one place where input is hostile - the state folder is writable by
// anyone who is the user - so its cases are the attacks: a line that leaves the folder by
// "..", by a sibling that shares a prefix, by a link; a name that is a folder, or the list.
//
// FsOps is T7's (src/katana_update/fsops_win.cpp is a stub while this is written), so the
// cleanup cases run on DiskFs below: the std::filesystem operations the interface describes,
// with reparse points reported as the interface says they are.

#include <gtest/gtest.h>

#include <algorithm>
#include <atomic>
#include <chrono>
#include <cstdint>
#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <iterator>
#include <optional>
#include <regex>
#include <set>
#include <string>
#include <thread>
#include <vector>

#include "katana/core/path_text.hpp"
#include "katana/update/install.hpp"
#include "katana/update/registry.hpp"
#include "katana/update/state.hpp"

#ifdef _WIN32
#ifndef NOMINMAX
#define NOMINMAX
#endif
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>
#else
#include <unistd.h>
#endif

using namespace katana;
namespace fs = std::filesystem;

namespace {

std::string readFile(const fs::path& path)
{
    std::ifstream in(path, std::ios::binary);
    return std::string(std::istreambuf_iterator<char>(in), std::istreambuf_iterator<char>());
}

void writeFile(const fs::path& path, const std::string& bytes)
{
    fs::create_directories(path.parent_path());
    std::ofstream out(path, std::ios::binary | std::ios::trunc);
    out.write(bytes.data(), static_cast<std::streamsize>(bytes.size()));
}

class TempDir {
  public:
    TempDir()
    {
        static std::atomic<int> counter{0};
        path_ = fs::temp_directory_path() /
                ("katana_t6_state_" +
                 std::to_string(std::chrono::steady_clock::now().time_since_epoch().count()) + "_" +
                 std::to_string(counter++));
        fs::create_directories(path_);
    }
    TempDir(const TempDir&) = delete;
    TempDir& operator=(const TempDir&) = delete;
    ~TempDir()
    {
        std::error_code ignored;
        fs::remove_all(path_, ignored);
    }
    const fs::path& path() const { return path_; }

  private:
    fs::path path_;
};

// A directory link (a junction on Windows - it needs no privilege - a symlink elsewhere), taken
// away again without touching what it points at.
class DirectoryLink {
  public:
    DirectoryLink(const fs::path& link, const fs::path& target) : link_(link)
    {
#ifdef _WIN32
        // `cmd /c "..."`: the outer quotes are removed by cmd, the inner ones stay.
        const std::string command =
            "\"mklink /J \"" + link.string() + "\" \"" + target.string() + "\" >NUL 2>&1\"";
        made_ = std::system(command.c_str()) == 0;
#else
        std::error_code error;
        fs::create_directory_symlink(target, link, error);
        made_ = !error;
#endif
    }
    DirectoryLink(const DirectoryLink&) = delete;
    DirectoryLink& operator=(const DirectoryLink&) = delete;
    ~DirectoryLink()
    {
        if (made_) {
            std::error_code ignored;
            fs::remove(link_, ignored);
        }
    }
    bool made() const { return made_; }

  private:
    fs::path link_;
    bool made_ = false;
};

// Sets an environment variable for the length of a test and puts it back. `value` nullopt
// removes it. Windows: the wide environment, which core::environmentVariable reads.
class EnvGuard {
  public:
    EnvGuard(const char* name, const std::optional<fs::path>& value) : name_(name)
    {
        previous_ = current();
        apply(value ? std::optional<fs::path::string_type>(value->native()) : std::nullopt);
    }
    EnvGuard(const EnvGuard&) = delete;
    EnvGuard& operator=(const EnvGuard&) = delete;
    ~EnvGuard() { apply(previous_); }

  private:
    using Text = fs::path::string_type;
    std::optional<Text> current() const
    {
#ifdef _WIN32
        const std::wstring name(name_.begin(), name_.end());
        const DWORD size = GetEnvironmentVariableW(name.c_str(), nullptr, 0);
        if (size == 0) {
            return GetLastError() == ERROR_ENVVAR_NOT_FOUND ? std::nullopt
                                                            : std::optional<Text>(Text());
        }
        Text value(size, L'\0');
        value.resize(GetEnvironmentVariableW(name.c_str(), value.data(), size));
        return value;
#else
        const char* value = std::getenv(name_.c_str());
        return value ? std::optional<Text>(value) : std::nullopt;
#endif
    }
    void apply(const std::optional<Text>& value) const
    {
#ifdef _WIN32
        const std::wstring name(name_.begin(), name_.end());
        SetEnvironmentVariableW(name.c_str(), value ? value->c_str() : nullptr);
#else
        if (value) {
            setenv(name_.c_str(), value->c_str(), 1);
        } else {
            unsetenv(name_.c_str());
        }
#endif
    }
    std::string name_;
    std::optional<Text> previous_;
};

// Makes `to` the working folder for the length of a test and puts the old one back.
class CwdGuard {
  public:
    explicit CwdGuard(const fs::path& to) : previous_(fs::current_path()) { fs::current_path(to); }
    CwdGuard(const CwdGuard&) = delete;
    CwdGuard& operator=(const CwdGuard&) = delete;
    ~CwdGuard()
    {
        std::error_code ignored;
        fs::current_path(previous_, ignored);
    }

  private:
    fs::path previous_;
};

// ---- the file system, as FsOps describes it -----------------------------------------------

core::Error osError(const char* what, long code, const fs::path& path)
{
    return core::makeError(core::ErrorCode::FileExportFailure, what,
                           "os_error=" + std::to_string(code) + "; " + core::pathToUtf8(path));
}

class DiskFs : public update::FsOps {
  public:
    core::Result<update::EntryInfo> stat(const fs::path& path) const override
    {
        update::EntryInfo info;
#ifdef _WIN32
        const DWORD attributes = GetFileAttributesW(path.c_str());
        if (attributes == INVALID_FILE_ATTRIBUTES) {
            const DWORD error = GetLastError();
            if (error == ERROR_FILE_NOT_FOUND || error == ERROR_PATH_NOT_FOUND) {
                return info;
            }
            return osError("stat failed", error, path);
        }
        info.reparsePoint = (attributes & FILE_ATTRIBUTE_REPARSE_POINT) != 0;
        info.kind = (attributes & FILE_ATTRIBUTE_DIRECTORY) != 0 ? update::EntryKind::Directory
                                                                 : update::EntryKind::File;
#else
        std::error_code error;
        const fs::file_status status = fs::symlink_status(path, error);
        if (status.type() == fs::file_type::not_found) {
            return info;
        }
        if (error) {
            return osError("stat failed", error.value(), path);
        }
        info.reparsePoint = status.type() == fs::file_type::symlink;
        info.kind = info.reparsePoint                           ? update::EntryKind::Other
                    : status.type() == fs::file_type::regular   ? update::EntryKind::File
                    : status.type() == fs::file_type::directory ? update::EntryKind::Directory
                                                                : update::EntryKind::Other;
#endif
        if (info.kind == update::EntryKind::File) {
            std::error_code error2;
            info.size = fs::file_size(path, error2);
        }
        return info;
    }
    core::Result<std::vector<fs::path>> listDirectory(const fs::path& directory) const override
    {
        std::error_code error;
        std::vector<fs::path> out;
        for (fs::directory_iterator it(directory, error), end; !error && it != end;
             it.increment(error)) {
            out.push_back(it->path());
        }
        if (error) {
            return osError("list failed", error.value(), directory);
        }
        return out;
    }
    core::Result<std::string> readFile(const fs::path& path) const override
    {
        std::ifstream in(path, std::ios::binary);
        if (!in) {
            return osError("read failed", 0, path);
        }
        return std::string(std::istreambuf_iterator<char>(in), std::istreambuf_iterator<char>());
    }
    core::Result<fs::path> resolve(const fs::path& path) const override
    {
        std::error_code error;
        const fs::path resolved = fs::weakly_canonical(path, error);
        if (error) {
            return osError("resolve failed", error.value(), path);
        }
        return resolved;
    }
    core::Result<std::uint64_t> freeSpace(const fs::path& path) const override
    {
        std::error_code error;
        const fs::space_info space = fs::space(path, error);
        if (error) {
            return osError("space failed", error.value(), path);
        }
        return static_cast<std::uint64_t>(space.available);
    }
    core::Status makeDirectories(const fs::path& directory) override
    {
        std::error_code error;
        fs::create_directories(directory, error);
        return error ? core::Status(osError("mkdir failed", error.value(), directory))
                     : core::Status();
    }
    core::Status copyFile(const fs::path& from, const fs::path& to) override
    {
        std::error_code error;
        fs::copy_file(from, to, error);
        return error ? core::Status(osError("copy failed", error.value(), to)) : core::Status();
    }
    core::Status renameFile(const fs::path& from, const fs::path& to) override
    {
        std::error_code error;
        fs::rename(from, to, error);
        return error ? core::Status(osError("rename failed", error.value(), to)) : core::Status();
    }
    core::Status removeFile(const fs::path& path) override
    {
        std::error_code error;
        fs::remove(path, error);
        return error ? core::Status(osError("remove failed", error.value(), path)) : core::Status();
    }
    core::Status removeEmptyDirectory(const fs::path& path) override
    {
        std::error_code error;
        fs::remove(path, error);
        return error ? core::Status(osError("rmdir failed", error.value(), path)) : core::Status();
    }
    core::Status appendDurable(const fs::path& path, std::string_view bytes) override
    {
        std::ofstream out(path, std::ios::binary | std::ios::app);
        out.write(bytes.data(), static_cast<std::streamsize>(bytes.size()));
        out.flush();
        return out ? core::Status() : core::Status(osError("append failed", 0, path));
    }
    core::Status writeDurable(const fs::path& path, std::string_view bytes) override
    {
        std::ofstream out(path, std::ios::binary | std::ios::trunc);
        out.write(bytes.data(), static_cast<std::streamsize>(bytes.size()));
        out.flush();
        return out ? core::Status() : core::Status(osError("write failed", 0, path));
    }
    core::Status setExecutable(const fs::path&) override { return {}; }
    core::Status syncDirectory(const fs::path&) override { return {}; }
    core::Result<std::unique_ptr<update::FileLock>> tryLock(const fs::path&) override
    {
        return core::makeError(core::ErrorCode::Unsupported, "not used here");
    }
};

// A DiskFs that cannot delete the files it is told are locked: Windows error 32, which the real
// implementation returns once its retries are spent.
class LockedFs : public DiskFs {
  public:
    explicit LockedFs(std::set<std::string> lockedNames) : locked_(std::move(lockedNames)) {}
    core::Status removeFile(const fs::path& path) override
    {
        if (locked_.contains(path.filename().string())) {
            return core::makeError(core::ErrorCode::FileExportFailure, "the file is in use",
                                   "os_error=32; " + core::pathToUtf8(path));
        }
        return DiskFs::removeFile(path);
    }

  private:
    std::set<std::string> locked_;
};

// Thrown in place of the k-th changing operation: the process died between k - 1 and k.
struct Crash {
    std::uint64_t operation = 0;
};

// A DiskFs that counts the operations that change the disk (fsops.hpp lists them) and dies before
// the one it is told to. T7's FaultFs does this for the apply's own loops; this is the same idea,
// local, so that these cases do not wait for it.
class CrashingFs final : public DiskFs {
  public:
    void crashBefore(std::uint64_t operation) { crashBefore_ = operation; }
    std::uint64_t operations() const { return operations_; }

    core::Status makeDirectories(const fs::path& p) override
    {
        tick();
        return DiskFs::makeDirectories(p);
    }
    core::Status copyFile(const fs::path& a, const fs::path& b) override
    {
        tick();
        return DiskFs::copyFile(a, b);
    }
    core::Status renameFile(const fs::path& a, const fs::path& b) override
    {
        tick();
        return DiskFs::renameFile(a, b);
    }
    core::Status removeFile(const fs::path& p) override
    {
        tick();
        return DiskFs::removeFile(p);
    }
    core::Status removeEmptyDirectory(const fs::path& p) override
    {
        tick();
        return DiskFs::removeEmptyDirectory(p);
    }
    core::Status appendDurable(const fs::path& p, std::string_view b) override
    {
        tick();
        return DiskFs::appendDurable(p, b);
    }
    core::Status writeDurable(const fs::path& p, std::string_view b) override
    {
        tick();
        return DiskFs::writeDurable(p, b);
    }
    core::Status setExecutable(const fs::path& p) override
    {
        tick();
        return DiskFs::setExecutable(p);
    }

  private:
    void tick()
    {
        if (++operations_ == crashBefore_) {
            throw Crash{operations_};
        }
    }
    std::uint64_t crashBefore_ = 0;
    std::uint64_t operations_ = 0;
};

// A registry that says what it is told to.
class FakeRegistry final : public update::RegistryOps {
  public:
    std::optional<fs::path> location;
    bool fails = false;
    core::Result<std::optional<fs::path>> installerLocation() const override
    {
        if (fails) {
            return core::makeError(core::ErrorCode::Internal, "the registry cannot be read");
        }
        return location;
    }
    core::Status setRunOnce(std::string_view, std::string_view) override { return {}; }
    core::Status clearRunOnce(std::string_view) override { return {}; }
    core::Status setInstalledVersion(std::string_view, std::string_view) override { return {}; }
};

// parseVersion belongs to task T4. Until it is merged the cases that need a version are skipped,
// visibly, rather than reported as failures of a function that is not this task's.
#define REQUIRE_VERSION_PARSING()                                                                  \
    do {                                                                                           \
        if (!update::parseVersion("1.2").ok()) {                                                   \
            GTEST_SKIP() << "update::parseVersion is not implemented yet (task T4)";               \
        }                                                                                          \
    } while (false)

update::Version versionOf(std::uint32_t a, std::uint32_t b, std::uint32_t c, std::uint32_t d,
                          std::uint8_t written)
{
    update::Version v;
    v.components = {a, b, c, d};
    v.written = written;
    return v;
}

// An install root with install.json (and whatever else the case adds).
struct Root {
    TempDir temp;
    fs::path root = temp.path() / "R";
    fs::path records = root / "share" / "katana";
    Root() { fs::create_directories(records); }
    void installJson(const std::string& text) { writeFile(records / "install.json", text); }
    void installedJson(const std::string& text) { writeFile(records / "installed.json", text); }
};

std::string lineOf(const fs::path& path)
{
    return core::pathToUtf8(path) + "\n";
}

} // namespace

// ---- the names of things ------------------------------------------------------------------

TEST(InstallRecords, KindsAreNamedInTheWordsOfUpdateStatus)
{
    EXPECT_EQ(update::toString(update::InstallKind::Installer), "installer");
    EXPECT_EQ(update::toString(update::InstallKind::Archive), "archive");
    EXPECT_EQ(update::toString(update::InstallKind::Development), "development");
}

TEST(InstallRecords, TheRecordsLiveInShareKatanaAndTheWorkFolderIsDotUpdate)
{
    const fs::path root = fs::path("R");
    EXPECT_EQ(update::installRecordDirectory(root), root / "share" / "katana");
    EXPECT_EQ(update::updateWorkDirectory(root), root / ".update");
}

// ---- readInstall --------------------------------------------------------------------------

TEST(InstallRecords, ATreeWithoutInstallJsonIsADevelopmentTreeWhateverElseIsInIt)
{
    REQUIRE_VERSION_PARSING();
    Root world;
    FakeRegistry registry;
    registry.location = world.root; // even a registry that claims it does not make it an install
    // installed.json and the manifest copy, left behind in a copied folder: still not an install.
    world.installedJson(R"({"schema":1,"version":"9.9.9","sequence":5})");
    writeFile(world.records / "installed.manifest.json", "{}");
    writeFile(world.records / "installed.manifest.json.sig", "x");
    const auto install = update::readInstall(world.root, registry);
    ASSERT_TRUE(install.ok()) << install.error().describe();
    EXPECT_EQ(install->kind, update::InstallKind::Development);
    EXPECT_EQ(install->root, world.root);
    EXPECT_EQ(install->target, "");
    EXPECT_EQ(install->product, "katana");
    EXPECT_EQ(install->version, *update::parseVersion(KATANA_EXPECTED_VERSION));
    EXPECT_EQ(update::toString(install->version), KATANA_EXPECTED_VERSION);
    EXPECT_EQ(install->sequence, 0u);
    EXPECT_FALSE(install->hasInstalledRecord);
    EXPECT_FALSE(install->hasInstalledManifest);
}

TEST(InstallRecords, AFolderThatDoesNotExistIsADevelopmentTreeToo)
{
    REQUIRE_VERSION_PARSING();
    TempDir temp;
    FakeRegistry registry;
    const auto install = update::readInstall(temp.path() / "nothing_here", registry);
    ASSERT_TRUE(install.ok()) << install.error().describe();
    EXPECT_EQ(install->kind, update::InstallKind::Development);
}

TEST(InstallRecords, AnInstallJsonAloneIsAFreshInstallOfTheNamedTarget)
{
    REQUIRE_VERSION_PARSING();
    Root world;
    world.installJson(R"({"schema":1,"target":"win-arm64"})");
    FakeRegistry registry;
    const auto install = update::readInstall(world.root, registry);
    ASSERT_TRUE(install.ok()) << install.error().describe();
    EXPECT_EQ(install->kind, update::InstallKind::Archive);
    EXPECT_EQ(install->target, "win-arm64");
    // No installed.json: the version this program was built as, at sequence 0, and no record.
    EXPECT_EQ(install->version, *update::parseVersion(KATANA_EXPECTED_VERSION));
    EXPECT_EQ(update::toString(install->version), KATANA_EXPECTED_VERSION);
    EXPECT_EQ(install->sequence, 0u);
    EXPECT_FALSE(install->hasInstalledRecord);
    EXPECT_FALSE(install->hasInstalledManifest);
}

TEST(InstallRecords, TheFixtureInstallTreesAreInstallsOfTargetWin64)
{
    REQUIRE_VERSION_PARSING();
    for (const char* tree : {"v1", "v2"}) {
        FakeRegistry registry;
        const auto install =
            update::readInstall(fs::path(KATANA_UPDATE_FIXTURES) / "install" / tree, registry);
        ASSERT_TRUE(install.ok()) << tree << ": " << install.error().describe();
        EXPECT_EQ(install->kind, update::InstallKind::Archive);
        EXPECT_EQ(install->target, "win64");
    }
}

TEST(InstallRecords, InstalledJsonSaysWhichBuildTheHelperPutThere)
{
    REQUIRE_VERSION_PARSING();
    Root world;
    world.installJson(R"({"schema":1,"target":"win64"})");
    world.installedJson(R"({"schema":1,"product":"katana","version":"0.9.0","sequence":1790000000,)"
                        R"("tag":"v0.9.0","target":"win64"})");
    FakeRegistry registry;
    const auto install = update::readInstall(world.root, registry);
    ASSERT_TRUE(install.ok()) << install.error().describe();
    EXPECT_EQ(install->version, versionOf(0, 9, 0, 0, 3));
    EXPECT_EQ(update::toString(install->version), "0.9.0") << "== ignores how many were written";
    EXPECT_EQ(install->sequence, 1790000000u);
    EXPECT_TRUE(install->hasInstalledRecord) << "exactly when installed.json is there";
}

TEST(InstallRecords, VersionsOfFourComponentsAndSequencesBeyondTwoToTheFiftyThirdAreRead)
{
    REQUIRE_VERSION_PARSING();
    Root world;
    world.installJson(R"({"schema":1,"target":"win64"})");
    world.installedJson(R"({"schema":1,"version":"1.2.3.4","sequence":18446744073709551615})");
    FakeRegistry registry;
    const auto install = update::readInstall(world.root, registry);
    ASSERT_TRUE(install.ok()) << install.error().describe();
    EXPECT_EQ(install->version, versionOf(1, 2, 3, 4, 4));
    EXPECT_EQ(update::toString(install->version), "1.2.3.4") << "== ignores how many were written";
    EXPECT_EQ(install->sequence, UINT64_MAX);
}

TEST(InstallRecords, TheInstalledManifestCountsOnlyWithItsSignatureBesideIt)
{
    REQUIRE_VERSION_PARSING();
    Root world;
    world.installJson(R"({"schema":1,"target":"win64"})");
    FakeRegistry registry;
    auto read = [&] { return update::readInstall(world.root, registry); };
    EXPECT_FALSE(read()->hasInstalledManifest);
    writeFile(world.records / "installed.manifest.json", "{}");
    EXPECT_FALSE(read()->hasInstalledManifest)
        << "a manifest with no signature is not one to delete by";
    writeFile(world.records / "installed.manifest.json.sig", "ed25519 k s");
    EXPECT_TRUE(read()->hasInstalledManifest);
    fs::remove(world.records / "installed.manifest.json");
    EXPECT_FALSE(read()->hasInstalledManifest);
    fs::create_directory(world.records / "installed.manifest.json"); // a folder is not a manifest
    EXPECT_FALSE(read()->hasInstalledManifest);
}

TEST(InstallRecords, TheRegistryDecidesBetweenInstallerAndArchive)
{
    REQUIRE_VERSION_PARSING();
    Root world;
    world.installJson(R"({"schema":1,"target":"win64"})");
    FakeRegistry registry;
    auto kind = [&] { return update::readInstall(world.root, registry)->kind; };

    EXPECT_EQ(kind(), update::InstallKind::Archive) << "no installer entry";
    registry.location = world.root;
    EXPECT_EQ(kind(), update::InstallKind::Installer);
    registry.location = world.root / "bin" / "..";
    EXPECT_EQ(kind(), update::InstallKind::Installer) << "the same folder spelled another way";
    registry.location = world.root.string() + "/"; // a trailing separator
    EXPECT_EQ(kind(), update::InstallKind::Installer);
    registry.location = world.temp.path() / "somewhere_else";
    EXPECT_EQ(kind(), update::InstallKind::Archive) << "the installer put another copy elsewhere";
    registry.location = world.temp.path();
    EXPECT_EQ(kind(), update::InstallKind::Archive) << "a parent folder is not the folder";
    registry.location = world.root;
    registry.fails = true;
    EXPECT_EQ(kind(), update::InstallKind::Archive)
        << "a registry that cannot be read is no reason to refuse";
}

#ifdef _WIN32
TEST(InstallRecords, OnWindowsTheInstallerFolderMayDifferInCaseFromTheRoot)
{
    REQUIRE_VERSION_PARSING();
    Root world;
    world.installJson(R"({"schema":1,"target":"win64"})");
    FakeRegistry registry;
    std::string upper = world.root.string();
    std::transform(upper.begin(), upper.end(), upper.begin(),
                   [](unsigned char c) { return std::toupper(c); });
    registry.location = fs::path(upper);
    EXPECT_EQ(update::readInstall(world.root, registry)->kind, update::InstallKind::Installer);
}
#endif

TEST(InstallRecords, AnInstallJsonThatIsNotWhatTheCpackInstallWritesIsRefused)
{
    const struct {
        const char* text;
        core::ErrorCode code;
    } cases[] = {
        {"", core::ErrorCode::ParseFailure},
        {"{", core::ErrorCode::ParseFailure},
        {"[]", core::ErrorCode::ParseFailure},
        {"null", core::ErrorCode::ParseFailure},
        {R"({"target":"win64"})", core::ErrorCode::ParseFailure},              // no schema
        {R"({"schema":"1","target":"win64"})", core::ErrorCode::ParseFailure}, // schema is text
        {R"({"schema":1.0,"target":"win64"})", core::ErrorCode::ParseFailure}, // not an integer
        {R"({"schema":1})", core::ErrorCode::ParseFailure},                    // no target
        {R"({"schema":1,"target":5})", core::ErrorCode::ParseFailure},
        {R"({"schema":1,"target":""})", core::ErrorCode::ParseFailure},
        {R"({"schema":1,"target":"Win64"})", core::ErrorCode::ParseFailure}, // upper case
        {R"({"schema":1,"target":"win 64"})", core::ErrorCode::ParseFailure},
        {R"({"schema":1,"target":"win64\n"})", core::ErrorCode::ParseFailure},
        {R"({"schema":1,"target":"../win64"})",
         core::ErrorCode::ParseFailure}, // it becomes part of a URL
        {R"({"schema":1,"target":"a/b"})", core::ErrorCode::ParseFailure},
        {R"({"schema":1,"target":"win64?x=1"})", core::ErrorCode::ParseFailure},
        {R"({"schema":1,"target":"abcdefghijklmnopqrstuvwxyz0123456"})",
         core::ErrorCode::ParseFailure}, // 33
        {R"({"schema":2,"target":"win64"})", core::ErrorCode::Unsupported},
        {R"({"schema":0,"target":"win64"})", core::ErrorCode::Unsupported},
        {R"({"schema":-1,"target":"win64"})", core::ErrorCode::Unsupported},
    };
    for (const auto& c : cases) {
        Root world;
        world.installJson(c.text);
        FakeRegistry registry;
        const auto install = update::readInstall(world.root, registry);
        ASSERT_FALSE(install.ok()) << c.text;
        EXPECT_EQ(install.error().code, c.code) << c.text;
        EXPECT_NE(install.error().context.find("install.json"), std::string::npos)
            << "the file is named: " << install.error().context;
    }
}

TEST(InstallRecords, ATargetOfThirtyTwoCharactersAndTheFiveRealOnesAreAccepted)
{
    REQUIRE_VERSION_PARSING();
    for (const char* target : {"win64", "win-arm64", "linux-x86_64", "linux-aarch64", "macos-arm64",
                               "abcdefghijklmnopqrstuvwxyz012345"}) {
        Root world;
        world.installJson(std::string(R"({"schema":1,"target":")") + target + R"("})");
        FakeRegistry registry;
        const auto install = update::readInstall(world.root, registry);
        ASSERT_TRUE(install.ok()) << target << ": " << install.error().describe();
        EXPECT_EQ(install->target, target);
    }
}

TEST(InstallRecords, MembersAnInstallJsonHasBeyondTheOnesReadAreIgnored)
{
    REQUIRE_VERSION_PARSING();
    Root world;
    world.installJson(
        R"({"schema":1,"target":"win64","written_by":"a newer packaging","extra":[1,2]})");
    FakeRegistry registry;
    EXPECT_TRUE(update::readInstall(world.root, registry).ok());
}

TEST(InstallRecords, AnInstalledJsonThatIsNotWhatTheHelperWritesIsRefused)
{
    REQUIRE_VERSION_PARSING();
    const struct {
        const char* text;
        core::ErrorCode code;
    } cases[] = {
        {"", core::ErrorCode::ParseFailure},
        {"{", core::ErrorCode::ParseFailure},
        {R"({"schema":1,"sequence":5})", core::ErrorCode::ParseFailure},      // no version
        {R"({"schema":1,"version":"0.9.0"})", core::ErrorCode::ParseFailure}, // no sequence
        {R"({"schema":1,"version":5,"sequence":5})", core::ErrorCode::ParseFailure},
        {R"({"schema":1,"version":"nine","sequence":5})", core::ErrorCode::ParseFailure},
        {R"({"schema":1,"version":"1","sequence":5})",
         core::ErrorCode::ParseFailure}, // one component
        {R"({"schema":1,"version":"0.9.0","sequence":-1})", core::ErrorCode::ParseFailure},
        {R"({"schema":1,"version":"0.9.0","sequence":"5"})", core::ErrorCode::ParseFailure},
        {R"({"schema":1,"version":"0.9.0","sequence":1.5})", core::ErrorCode::ParseFailure},
        {R"({"schema":1,"version":"0.9.0","sequence":18446744073709551616})",
         core::ErrorCode::ParseFailure},
        {R"({"version":"0.9.0","sequence":5})", core::ErrorCode::ParseFailure}, // no schema
        {R"({"schema":2,"version":"0.9.0","sequence":5})", core::ErrorCode::Unsupported},
    };
    for (const auto& c : cases) {
        Root world;
        world.installJson(R"({"schema":1,"target":"win64"})");
        world.installedJson(c.text);
        FakeRegistry registry;
        const auto install = update::readInstall(world.root, registry);
        ASSERT_FALSE(install.ok()) << c.text;
        EXPECT_EQ(install.error().code, c.code) << c.text;
        EXPECT_NE(install.error().context.find("installed.json"), std::string::npos)
            << install.error().context;
    }
}

TEST(InstallRecords, ARecordLongerThanAnyThisProgramWritesIsNotReadWhole)
{
    Root world;
    world.installJson(R"({"schema":1,"target":"win64","pad":")" + std::string(70 * 1024, 'x') +
                      R"("})");
    FakeRegistry registry;
    const auto install = update::readInstall(world.root, registry);
    ASSERT_FALSE(install.ok());
    EXPECT_EQ(install.error().code, core::ErrorCode::ParseFailure);
}

TEST(InstallRecords, ARecordThatIsAFolderIsAnErrorNotADevelopmentTree)
{
    Root world;
    fs::create_directory(world.records / "install.json");
    FakeRegistry registry;
    const auto install = update::readInstall(world.root, registry);
    ASSERT_FALSE(install.ok())
        << "an install.json that cannot be read must not turn a real install into a dev tree";
    EXPECT_EQ(install.error().code, core::ErrorCode::FileImportFailure);
}

TEST(InstallRecords, NoRootIsAnInvalidArgument)
{
    FakeRegistry registry;
    const auto install = update::readInstall(fs::path(), registry);
    ASSERT_FALSE(install.ok());
    EXPECT_EQ(install.error().code, core::ErrorCode::InvalidArgument);
}

// ---- the root of the running program ------------------------------------------------------

TEST(InstallRecords, TheRunningProgramIsNotInsideAnInstallWhenItIsATestInTheBuildTree)
{
    // The test executable is build/<preset>/bin/tests/katana_update_tests: its folder is "tests",
    // not "bin", so it is in no install (the layout rules are tested in test_contract.cpp). What
    // this holds is that the function found THIS program's path: the error names it.
    const auto root = update::installRootOfRunningProgram();
    ASSERT_FALSE(root.ok()) << "unexpectedly inside an install: " << core::pathToUtf8(*root);
    EXPECT_EQ(root.error().code, core::ErrorCode::NotFound);
    const fs::path named = core::pathFromUtf8(root.error().context);
    EXPECT_TRUE(named.is_absolute()) << root.error().context;
    EXPECT_TRUE(fs::exists(named)) << root.error().context;
    EXPECT_EQ(named.stem().string(), "katana_update_tests") << root.error().context;
}

// ---- the state folder ---------------------------------------------------------------------

TEST(StateFolder, TheEnvironmentVariableSaysWhereItIsAndNothingElseMovesIt)
{
    TempDir temp;
    const fs::path chosen = temp.path() / "chosen" / "folder";
    EnvGuard variable("KATANA_UPDATE_STATE_DIR", chosen);
    EXPECT_EQ(update::stateDirectory(), chosen);
    // Not created by asking.
    EXPECT_FALSE(fs::exists(chosen));
}

TEST(StateFolder, ARelativeValueIsMadeAbsoluteAtOnce)
{
    EnvGuard variable("KATANA_UPDATE_STATE_DIR", fs::path("relative") / "dir");
    const fs::path state = update::stateDirectory();
    EXPECT_TRUE(state.is_absolute()) << state;
    EXPECT_EQ(state, fs::absolute(fs::path("relative") / "dir"));
}

TEST(StateFolder, AnEmptyVariableIsAsGoodAsNone)
{
    EnvGuard variable("KATANA_UPDATE_STATE_DIR", fs::path(""));
    const fs::path state = update::stateDirectory();
    EXPECT_FALSE(state.empty()) << "falls back to the platform's place on any machine with a home";
    EXPECT_EQ(state.filename(), "update");
}

#ifdef _WIN32
TEST(StateFolder, OnWindowsItIsLocalAppDataKatanaUpdate)
{
    TempDir temp;
    EnvGuard none("KATANA_UPDATE_STATE_DIR", std::nullopt);
    EnvGuard local("LOCALAPPDATA", temp.path() / "Local");
    EXPECT_EQ(update::stateDirectory(), temp.path() / "Local" / "Katana" / "update");
}

TEST(StateFolder, OnWindowsWithoutLocalAppDataItIsUnderTheProfile)
{
    TempDir temp;
    EnvGuard none("KATANA_UPDATE_STATE_DIR", std::nullopt);
    EnvGuard local("LOCALAPPDATA", std::nullopt);
    EnvGuard profile("USERPROFILE", temp.path() / "profile");
    EXPECT_EQ(update::stateDirectory(),
              temp.path() / "profile" / "AppData" / "Local" / "Katana" / "update");
    EnvGuard noProfile("USERPROFILE", std::nullopt);
    EXPECT_TRUE(update::stateDirectory().empty())
        << "no place to put it: an empty path, not the working folder";
}
#else
TEST(StateFolder, OnLinuxItIsXdgStateHomeKatanaUpdate)
{
    TempDir temp;
    EnvGuard none("KATANA_UPDATE_STATE_DIR", std::nullopt);
    EnvGuard xdg("XDG_STATE_HOME", temp.path() / "state");
    EXPECT_EQ(update::stateDirectory(), temp.path() / "state" / "katana" / "update");
}

TEST(StateFolder, OnLinuxARelativeXdgValueIsIgnoredAsTheSpecificationSays)
{
    TempDir temp;
    EnvGuard none("KATANA_UPDATE_STATE_DIR", std::nullopt);
    EnvGuard xdg("XDG_STATE_HOME", fs::path("relative"));
    EnvGuard home("HOME", temp.path() / "home");
    EXPECT_EQ(update::stateDirectory(),
              temp.path() / "home" / ".local" / "state" / "katana" / "update");
}

TEST(StateFolder, OnLinuxWithoutXdgItIsUnderHomeLocalState)
{
    TempDir temp;
    EnvGuard none("KATANA_UPDATE_STATE_DIR", std::nullopt);
    EnvGuard xdg("XDG_STATE_HOME", std::nullopt);
    EnvGuard home("HOME", temp.path() / "home");
    EXPECT_EQ(update::stateDirectory(),
              temp.path() / "home" / ".local" / "state" / "katana" / "update");
    EnvGuard noHome("HOME", std::nullopt);
    EXPECT_TRUE(update::stateDirectory().empty());
}
#endif

TEST(StateFolder, TheNamesInsideItAreTheOnesTheHeaderListsAndNothingIsCreated)
{
    const fs::path u = fs::path("U");
    EXPECT_EQ(update::stagePath(u, 1792312800), u / "stage-1792312800");
    EXPECT_EQ(update::stagePath(u, 0), u / "stage-0");
    EXPECT_EQ(update::stagePath(u, UINT64_MAX), u / "stage-18446744073709551615");
    EXPECT_EQ(update::runPath(u, "3fa9"), u / "run-3fa9");
    EXPECT_EQ(update::healthMarkerPath(u, "3fa9"), u / "healthy-3fa9");
    EXPECT_FALSE(fs::exists(u));
}

TEST(StateFolder, NoStateFolderIsRefusedByEveryFunctionAndIsNeverTheWorkingFolder)
{
    // stateDirectory() is an empty path on a machine with no home. Joined with a name that is a
    // file in the working folder; the folder here holds the very files an unguarded call would
    // read, replace, remove or act on.
    TempDir temp;
    DiskFs disk;
    const fs::path victim = temp.path() / "victim.txt";
    writeFile(victim, "x");
    writeFile(temp.path() / "state.json", R"({"schema":1,"auto":false})");
    writeFile(temp.path() / "last.json", R"({"schema":1,"status":"rolled_back"})");
    writeFile(temp.path() / "cleanup.list", lineOf(victim));
    const std::string stateBefore = readFile(temp.path() / "state.json");
    const std::string lastBefore = readFile(temp.path() / "last.json");
    const std::string listBefore = readFile(temp.path() / "cleanup.list");

    CwdGuard here(temp.path());
    const fs::path none;
    const auto expectRefused = [](const core::Status& status, const char* what) {
        ASSERT_FALSE(status.ok()) << what;
        EXPECT_EQ(status.error().code, core::ErrorCode::InvalidArgument) << what;
    };
    {
        const auto state = update::readState(none);
        ASSERT_FALSE(state.ok()) << "read ./state.json as if it were the state";
        EXPECT_EQ(state.error().code, core::ErrorCode::InvalidArgument);
    }
    {
        const auto last = update::readLastResult(none);
        ASSERT_FALSE(last.ok()) << "read ./last.json as if it were the last result";
        EXPECT_EQ(last.error().code, core::ErrorCode::InvalidArgument);
    }
    expectRefused(update::writeState(none, update::UpdateState{}), "writeState");
    expectRefused(update::writeLastResult(none, update::LastResult{"committed", "", "", "", ""}),
                  "writeLastResult");
    expectRefused(update::clearLastResult(none), "clearLastResult");
    expectRefused(update::appendCleanup(none, temp.path() / "leftover.bin", disk), "appendCleanup");
    expectRefused(update::processCleanupList(none, temp.path() / "R", disk), "processCleanupList");
    expectRefused(update::processCleanupList(none, fs::path(), disk),
                  "processCleanupList with no root");

    EXPECT_TRUE(fs::exists(victim)) << "a list in the working folder was believed";
    EXPECT_EQ(readFile(temp.path() / "state.json"), stateBefore);
    EXPECT_EQ(readFile(temp.path() / "last.json"), lastBefore);
    EXPECT_EQ(readFile(temp.path() / "cleanup.list"), listBefore);
    std::size_t files = 0;
    for (const fs::directory_entry& entry : fs::directory_iterator(temp.path())) {
        (void)entry;
        ++files;
    }
    EXPECT_EQ(files, 4u) << "and nothing was created";
}

// ---- state.json ---------------------------------------------------------------------------

TEST(StateFile, NoFileIsTheDefaultsAndNotAnError)
{
    TempDir temp;
    for (const fs::path& folder : {temp.path() / "never_made", temp.path()}) {
        const auto state = update::readState(folder);
        ASSERT_TRUE(state.ok()) << state.error().describe();
        EXPECT_EQ(state->lastCheckEpoch, 0);
        EXPECT_TRUE(state->automatic) << "the automatic check is on by default";
        EXPECT_EQ(state->skippedVersion, "");
        EXPECT_TRUE(state->blockedSequences.empty());
    }
    EXPECT_FALSE(fs::exists(temp.path() / "never_made")) << "reading creates nothing";
}

TEST(StateFile, EveryFieldComesBackAsItWentIn)
{
    TempDir temp;
    const fs::path folder = temp.path() / "nested" / "deeper" / "U";
    update::UpdateState state;
    state.lastCheckEpoch = 1790000000;
    state.automatic = false;
    state.skippedVersion = "0.3.0";
    state.blockedSequences = {1792312800, 7, UINT64_MAX};
    ASSERT_TRUE(update::writeState(folder, state).ok());

    const auto back = update::readState(folder);
    ASSERT_TRUE(back.ok()) << back.error().describe();
    EXPECT_EQ(back->lastCheckEpoch, 1790000000);
    EXPECT_FALSE(back->automatic);
    EXPECT_EQ(back->skippedVersion, "0.3.0");
    EXPECT_EQ(back->blockedSequences, (std::vector<std::uint64_t>{1792312800, 7, UINT64_MAX}));
}

TEST(StateFile, TheDefaultsAndTheExtremesRoundTripToo)
{
    TempDir temp;
    update::UpdateState defaults;
    ASSERT_TRUE(update::writeState(temp.path(), defaults).ok());
    auto back = update::readState(temp.path());
    ASSERT_TRUE(back.ok());
    EXPECT_EQ(back->lastCheckEpoch, 0);
    EXPECT_TRUE(back->automatic);
    EXPECT_TRUE(back->blockedSequences.empty());

    update::UpdateState extremes;
    extremes.lastCheckEpoch = INT64_MAX;
    extremes.skippedVersion = "caf\xC3\xA9 \"quoted\" \\ back\nslash"; // UTF-8 and JSON's escapes
    ASSERT_TRUE(update::writeState(temp.path(), extremes).ok());
    back = update::readState(temp.path());
    ASSERT_TRUE(back.ok()) << back.error().describe();
    EXPECT_EQ(back->lastCheckEpoch, INT64_MAX);
    EXPECT_EQ(back->skippedVersion, extremes.skippedVersion);
    extremes.lastCheckEpoch = INT64_MIN;
    ASSERT_TRUE(update::writeState(temp.path(), extremes).ok());
    EXPECT_EQ(update::readState(temp.path())->lastCheckEpoch, INT64_MIN);
}

TEST(StateFile, TheFileIsTheDocumentedJson)
{
    TempDir temp;
    update::UpdateState state;
    state.lastCheckEpoch = 1790000000;
    state.automatic = false;
    state.skippedVersion = "0.3.0";
    state.blockedSequences = {5, 6};
    ASSERT_TRUE(update::writeState(temp.path(), state).ok());
    const std::string text = readFile(temp.path() / "state.json");
    for (const char* pattern : {R"("schema"\s*:\s*1[,\s])", R"("last_check"\s*:\s*1790000000[,\s])",
                                R"("auto"\s*:\s*false[,\s])", R"("skipped"\s*:\s*"0\.3\.0"[,\s])",
                                R"("blocked"\s*:\s*\[\s*5\s*,\s*6\s*\])"}) {
        EXPECT_TRUE(std::regex_search(text, std::regex(pattern))) << pattern << "\n" << text;
    }
}

TEST(StateFile, MembersMissingTakeTheirDefaultsAndMembersUnknownAreIgnored)
{
    TempDir temp;
    writeFile(temp.path() / "state.json", R"({"schema":1})");
    auto state = update::readState(temp.path());
    ASSERT_TRUE(state.ok()) << state.error().describe();
    EXPECT_EQ(state->lastCheckEpoch, 0);
    EXPECT_TRUE(state->automatic);

    writeFile(temp.path() / "state.json",
              R"({"schema":1,"auto":false,"added_later":{"a":[1]},"last_check":-5})");
    state = update::readState(temp.path());
    ASSERT_TRUE(state.ok()) << state.error().describe();
    EXPECT_FALSE(state->automatic);
    EXPECT_EQ(state->lastCheckEpoch, -5)
        << "a time before 1970 is kept as it is; checkIsDue treats it as never";
}

TEST(StateFile, ACorruptFileIsAParseFailureThatTheCallerAnswersWithDefaults)
{
    const char* cases[] = {
        "",
        "{",
        "null",
        "[]",
        "42",
        "\"state\"",
        "{\"schema\":1} trailing",
        "\x01\x02\xff\xfe binary",
        R"({"auto":true})",  // no schema
        R"({"schema":"1"})", // schema as text
        R"({"schema":1.0})",
        R"({"schema":1,"auto":"yes"})",
        R"({"schema":1,"auto":1})",
        R"({"schema":1,"last_check":"yesterday"})",
        R"({"schema":1,"last_check":1.5})",
        R"({"schema":1,"last_check":true})",
        R"({"schema":1,"last_check":9223372036854775808})", // INT64_MAX + 1
        R"({"schema":1,"skipped":5})",
        R"({"schema":1,"skipped":null})",
        R"({"schema":1,"blocked":5})",
        R"({"schema":1,"blocked":{"a":1}})",
        R"({"schema":1,"blocked":[-1]})",
        R"({"schema":1,"blocked":["a"]})",
        R"({"schema":1,"blocked":[1.5]})",
        R"({"schema":1,"blocked":[1,2,null]})",
    };
    for (const char* text : cases) {
        TempDir temp;
        writeFile(temp.path() / "state.json", text);
        const auto state = update::readState(temp.path());
        ASSERT_FALSE(state.ok()) << text;
        EXPECT_EQ(state.error().code, core::ErrorCode::ParseFailure) << text;
        EXPECT_NE(state.error().context.find("state.json"), std::string::npos)
            << state.error().context;
    }
}

TEST(StateFile, ASchemaFromAnotherVersionIsUnsupportedNotCorrupt)
{
    for (const char* text :
         {R"({"schema":2})", R"({"schema":0})", R"({"schema":99,"auto":false})"}) {
        TempDir temp;
        writeFile(temp.path() / "state.json", text);
        const auto state = update::readState(temp.path());
        ASSERT_FALSE(state.ok()) << text;
        EXPECT_EQ(state.error().code, core::ErrorCode::Unsupported) << text;
    }
}

TEST(StateFile, AStateFileThatIsAFolderOrFarTooLongIsAnErrorNotTheDefaults)
{
    {
        TempDir temp;
        fs::create_directory(temp.path() / "state.json");
        const auto state = update::readState(temp.path());
        ASSERT_FALSE(state.ok());
        EXPECT_EQ(state.error().code, core::ErrorCode::FileImportFailure);
    }
    {
        TempDir temp;
        writeFile(temp.path() / "state.json",
                  R"({"schema":1,"skipped":")" + std::string(1024 * 1024, 'x') + R"("})");
        const auto state = update::readState(temp.path());
        ASSERT_FALSE(state.ok())
            << "a megabyte of a version string was not written by this program";
        EXPECT_EQ(state.error().code, core::ErrorCode::ParseFailure);
    }
}

TEST(StateFile, WritingFailsOnAFolderThatCannotBeMade)
{
    TempDir temp;
    writeFile(temp.path() / "a_file", "x");
    const core::Status status =
        update::writeState(temp.path() / "a_file" / "U", update::UpdateState{});
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(status.error().code, core::ErrorCode::FileExportFailure);
}

// ---- state.json is replaced whole or not at all -------------------------------------------

TEST(StateFile, ARewriteLeavesOneFileAndNothingOfTheOldContent)
{
    TempDir temp;
    update::UpdateState big;
    big.skippedVersion = "9.9.9";
    for (std::uint64_t i = 0; i < 2000; ++i) {
        big.blockedSequences.push_back(1790000000 + i);
    }
    ASSERT_TRUE(update::writeState(temp.path(), big).ok());
    update::UpdateState small;
    small.automatic = false;
    ASSERT_TRUE(update::writeState(temp.path(), small).ok());

    const auto back = update::readState(temp.path());
    ASSERT_TRUE(back.ok()) << back.error().describe();
    EXPECT_FALSE(back->automatic);
    EXPECT_TRUE(back->blockedSequences.empty()) << "no tail of the longer old file is left";
    EXPECT_EQ(back->skippedVersion, "");

    std::vector<std::string> names;
    for (const fs::directory_entry& entry : fs::directory_iterator(temp.path())) {
        names.push_back(entry.path().filename().string());
    }
    EXPECT_EQ(names, (std::vector<std::string>{"state.json"})) << "the temporary file is gone";
}

TEST(StateFile, AWriteThatCannotCompleteLeavesTheOldFileWholeAndNoLitter)
{
    TempDir temp;
    update::UpdateState first;
    first.skippedVersion = "0.3.0";
    first.blockedSequences = {1, 2, 3};
    ASSERT_TRUE(update::writeState(temp.path(), first).ok());
    const std::string before = readFile(temp.path() / "state.json");

#ifdef _WIN32
    // The file is read-only: Windows will not replace it.
    fs::permissions(temp.path() / "state.json", fs::perms::owner_write, fs::perm_options::remove);
#else
    // The folder takes no new files.
    fs::permissions(temp.path(), fs::perms::owner_write, fs::perm_options::remove);
    {
        std::ofstream probe(temp.path() / "probe");
        if (probe.is_open()) {
            fs::permissions(temp.path(), fs::perms::owner_write, fs::perm_options::add);
            GTEST_SKIP() << "this user can create files in a folder without permission (root?)";
        }
    }
#endif
    update::UpdateState second;
    second.skippedVersion = "9.9.9";
    const core::Status status = update::writeState(temp.path(), second);
#ifdef _WIN32
    fs::permissions(temp.path() / "state.json", fs::perms::owner_write, fs::perm_options::add);
#else
    fs::permissions(temp.path(), fs::perms::owner_write, fs::perm_options::add);
#endif
    ASSERT_FALSE(status.ok()) << "the replacement was refused, and said so";
    EXPECT_EQ(status.error().code, core::ErrorCode::FileExportFailure);
    EXPECT_EQ(readFile(temp.path() / "state.json"), before) << "the old record is as it was";
    std::size_t files = 0;
    for (const fs::directory_entry& entry : fs::directory_iterator(temp.path())) {
        (void)entry;
        ++files;
    }
    EXPECT_EQ(files, 1u) << "no temporary file was left behind";
}

TEST(StateFile, AReaderDuringManyRewritesNeverSeesHalfAFile)
{
    // The control first: a file cut in half IS refused, so the check below could see a torn one.
    TempDir control;
    update::UpdateState wide;
    for (std::uint64_t i = 0; i < 3000; ++i) {
        wide.blockedSequences.push_back(1790000000 + i);
    }
    ASSERT_TRUE(update::writeState(control.path(), wide).ok());
    const std::string whole = readFile(control.path() / "state.json");
    writeFile(control.path() / "state.json", whole.substr(0, whole.size() / 2));
    ASSERT_EQ(update::readState(control.path()).error().code, core::ErrorCode::ParseFailure);

    // Now a writer that alternates a long record and a short one, and a reader that reads as fast
    // as it can.
    TempDir temp;
    update::UpdateState small;
    small.skippedVersion = "short";
    ASSERT_TRUE(update::writeState(temp.path(), wide).ok());
    std::atomic<bool> done{false};
    std::atomic<int> writeFailures{0};
    constexpr int kWrites = 40;
    std::string firstFailure; // written by the writer thread alone, read after join()
    std::thread writer([&] {
        for (int i = 0; i < kWrites; ++i) {
            const core::Status written = update::writeState(temp.path(), i % 2 == 0 ? small : wide);
            if (!written.ok()) {
                if (writeFailures++ == 0) {
                    firstFailure = written.error().describe();
                }
            }
        }
        done = true;
    });
    std::size_t reads = 0;
    std::size_t sawWide = 0;
    std::size_t sawSmall = 0;
    std::size_t torn = 0;
    while (!done) {
        const auto state = update::readState(temp.path());
        if (state.ok()) {
            ++reads;
            if (state->blockedSequences.size() == 3000 && state->skippedVersion.empty()) {
                ++sawWide;
            } else if (state->blockedSequences.empty() && state->skippedVersion == "short") {
                ++sawSmall;
            } else {
                ++torn;
            }
        } else if (state.error().code == core::ErrorCode::ParseFailure) {
            ++torn; // an unreadable half is exactly what this must never see
        }
        // FileImportFailure: the replacement was under way at that instant; try again.
        std::this_thread::sleep_for(std::chrono::microseconds(300));
    }
    writer.join();
    EXPECT_EQ(torn, 0u);
    EXPECT_GT(reads, 0u);
    EXPECT_GT(sawWide, 0u);
    EXPECT_GT(sawSmall, 0u) << "both records were seen: the check covered a change of content";
    // Windows will not rename over a record that a reader has open at that instant, even one
    // opened with every share mode: the write waits for the reader to let go (two seconds at
    // most, the cases below). This reader reopens the file every 300 microseconds without end,
    // which no real one does, so some writes wait for a long time and a few may give up; most
    // must not.
    EXPECT_LT(writeFailures.load(), kWrites / 2) << firstFailure;
}

#ifdef _WIN32
TEST(StateFile, ARecordAnotherProgramHasOpenForAWhileIsReplacedWhenItLetsGo)
{
    // A virus scanner looking at a file that was just closed holds it with no sharing for a
    // moment: 400 ms here, eight times what the first version of the retry waited. (Windows
    // answers the rename with error 5, access denied, or 32, sharing violation.)
    TempDir temp;
    update::UpdateState first;
    first.skippedVersion = "0.3.0";
    ASSERT_TRUE(update::writeState(temp.path(), first).ok());
    const fs::path file = temp.path() / "state.json";
    HANDLE held = CreateFileW(file.c_str(), GENERIC_READ, 0, nullptr, OPEN_EXISTING,
                              FILE_ATTRIBUTE_NORMAL, nullptr);
    ASSERT_NE(held, INVALID_HANDLE_VALUE);
    std::thread scanner([held] {
        std::this_thread::sleep_for(std::chrono::milliseconds(400));
        CloseHandle(held);
    });
    update::UpdateState second;
    second.skippedVersion = "9.9.9";
    const core::Status status = update::writeState(temp.path(), second);
    scanner.join();
    ASSERT_TRUE(status.ok()) << status.error().describe();
    const auto back = update::readState(temp.path());
    ASSERT_TRUE(back.ok()) << back.error().describe();
    EXPECT_EQ(back->skippedVersion, "9.9.9");
    std::size_t files = 0;
    for (const fs::directory_entry& entry : fs::directory_iterator(temp.path())) {
        (void)entry;
        ++files;
    }
    EXPECT_EQ(files, 1u) << "no temporary file was left behind";
}

TEST(StateFile, ARecordHeldOpenForGoodIsReportedAfterTheWaitAndTheOldOneIsWhole)
{
    TempDir temp;
    update::UpdateState first;
    first.skippedVersion = "0.3.0";
    ASSERT_TRUE(update::writeState(temp.path(), first).ok());
    const fs::path file = temp.path() / "state.json";
    const std::string before = readFile(file);
    HANDLE held = CreateFileW(file.c_str(), GENERIC_READ, 0, nullptr, OPEN_EXISTING,
                              FILE_ATTRIBUTE_NORMAL, nullptr);
    ASSERT_NE(held, INVALID_HANDLE_VALUE);
    update::UpdateState second;
    second.skippedVersion = "9.9.9";
    const auto begin = std::chrono::steady_clock::now();
    const core::Status status = update::writeState(temp.path(), second);
    const auto waited = std::chrono::steady_clock::now() - begin;
    CloseHandle(held);
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(status.error().code, core::ErrorCode::FileExportFailure);
    const std::string& context = status.error().context;
    EXPECT_TRUE(context.rfind("os_error=5;", 0) == 0 || context.rfind("os_error=32;", 0) == 0 ||
                context.rfind("os_error=33;", 0) == 0)
        << "the system's own number, one of the three that mean in use: " << context;
    EXPECT_GE(waited, std::chrono::milliseconds(1500)) << "it waited for the program to let go";
    EXPECT_EQ(readFile(file), before);
    std::size_t files = 0;
    for (const fs::directory_entry& entry : fs::directory_iterator(temp.path())) {
        (void)entry;
        ++files;
    }
    EXPECT_EQ(files, 1u) << "no temporary file was left behind";
}
#endif

// ---- last.json ----------------------------------------------------------------------------

TEST(LastResult, NoFileIsNoResult)
{
    TempDir temp;
    const auto result = update::readLastResult(temp.path());
    ASSERT_TRUE(result.ok()) << result.error().describe();
    EXPECT_FALSE(result->has_value());
    EXPECT_FALSE(update::readLastResult(temp.path() / "never_made")->has_value());
}

TEST(LastResult, EveryFieldComesBackIncludingTheHalfSwappedTreesStatus)
{
    TempDir temp;
    update::LastResult written;
    written.status = "rollback_incomplete";
    written.reason = "rollback_failed";
    written.holder = "katana_mcp.exe (4412)";
    written.version = "0.3.0";
    written.transaction = "3fa9c0de";
    ASSERT_TRUE(update::writeLastResult(temp.path() / "U", written).ok());
    const auto result = update::readLastResult(temp.path() / "U");
    ASSERT_TRUE(result.ok()) << result.error().describe();
    ASSERT_TRUE(result->has_value());
    EXPECT_EQ((*result)->status, "rollback_incomplete");
    EXPECT_EQ((*result)->reason, "rollback_failed");
    EXPECT_EQ((*result)->holder, "katana_mcp.exe (4412)");
    EXPECT_EQ((*result)->version, "0.3.0");
    EXPECT_EQ((*result)->transaction, "3fa9c0de");
    const std::string text = readFile(temp.path() / "U" / "last.json");
    EXPECT_TRUE(std::regex_search(text, std::regex(R"("tx"\s*:\s*"3fa9c0de")"))) << text;
}

TEST(LastResult, QuotesBackslashesLineBreaksAndNonAsciiSurvive)
{
    TempDir temp;
    update::LastResult written;
    written.status = "blocked";
    written.holder =
        "\"katana mcp\".exe (4412), C:\\Program Files\\x\nsecond line, caf\xC3\xA9 \xE2\x9C\x93";
    ASSERT_TRUE(update::writeLastResult(temp.path(), written).ok());
    const auto result = update::readLastResult(temp.path());
    ASSERT_TRUE(result.ok() && result->has_value());
    EXPECT_EQ((*result)->holder, written.holder);
    EXPECT_EQ((*result)->reason, "");
}

TEST(LastResult, AnotherResultReplacesTheFirstAndClearingRemovesIt)
{
    TempDir temp;
    update::LastResult first;
    first.status = "rolled_back";
    first.reason = "health_timeout";
    update::LastResult second;
    second.status = "committed";
    ASSERT_TRUE(update::writeLastResult(temp.path(), first).ok());
    ASSERT_TRUE(update::writeLastResult(temp.path(), second).ok());
    EXPECT_EQ((*update::readLastResult(temp.path()))->status, "committed");
    EXPECT_EQ((*update::readLastResult(temp.path()))->reason, "") << "nothing of the first is left";

    EXPECT_TRUE(update::clearLastResult(temp.path()).ok());
    EXPECT_FALSE(fs::exists(temp.path() / "last.json"));
    EXPECT_FALSE(update::readLastResult(temp.path())->has_value()) << "shown once, then cleared";
    EXPECT_TRUE(update::clearLastResult(temp.path()).ok()) << "clearing what is not there is fine";
    EXPECT_TRUE(update::clearLastResult(temp.path() / "never_made").ok());
}

TEST(LastResult, AFileThatIsNotAResultIsRefused)
{
    const struct {
        const char* text;
        core::ErrorCode code;
    } cases[] = {
        {"", core::ErrorCode::ParseFailure},
        {"{", core::ErrorCode::ParseFailure},
        {"[]", core::ErrorCode::ParseFailure},
        {R"({"status":"committed"})", core::ErrorCode::ParseFailure}, // no schema
        {R"({"schema":1})", core::ErrorCode::ParseFailure},           // no status
        {R"({"schema":1,"status":""})", core::ErrorCode::ParseFailure},
        {R"({"schema":1,"status":5})", core::ErrorCode::ParseFailure},
        {R"({"schema":1,"status":"blocked","holder":5})", core::ErrorCode::ParseFailure},
        {R"({"schema":1,"status":"blocked","reason":null})", core::ErrorCode::ParseFailure},
        {R"({"schema":1,"status":"blocked","tx":["a"]})", core::ErrorCode::ParseFailure},
        {R"({"schema":2,"status":"blocked"})", core::ErrorCode::Unsupported},
    };
    for (const auto& c : cases) {
        TempDir temp;
        writeFile(temp.path() / "last.json", c.text);
        const auto result = update::readLastResult(temp.path());
        ASSERT_FALSE(result.ok()) << c.text;
        EXPECT_EQ(result.error().code, c.code) << c.text;
    }
    // Members beyond the ones read are ignored.
    TempDir temp;
    writeFile(temp.path() / "last.json", R"({"schema":1,"status":"refused","new_member":[1,2]})");
    const auto result = update::readLastResult(temp.path());
    ASSERT_TRUE(result.ok() && result->has_value());
    EXPECT_EQ((*result)->status, "refused");
}

// ---- U/cleanup.list -----------------------------------------------------------------------

namespace {

// U and R/.update under a temporary folder, a victim outside both, and a place to make files in.
struct CleanupWorld {
    TempDir temp;
    fs::path stateDir = temp.path() / "U";
    fs::path root = temp.path() / "R";
    fs::path work = root / ".update";
    fs::path list = stateDir / "cleanup.list";
    DiskFs disk;
    CleanupWorld()
    {
        fs::create_directories(stateDir);
        fs::create_directories(work);
    }
    fs::path file(const fs::path& path, const std::string& bytes = "x") const
    {
        writeFile(path, bytes);
        return path;
    }
    void listLines(const std::vector<fs::path>& paths) const
    {
        std::string text;
        for (const fs::path& path : paths) {
            text += lineOf(path);
        }
        writeFile(list, text);
    }
};

} // namespace

TEST(CleanupList, AppendingCreatesTheFolderAndTheListAndAddsALineEachTime)
{
    TempDir temp;
    DiskFs disk;
    const fs::path stateDir = temp.path() / "not" / "made" / "yet";
    const fs::path first = temp.path() / "a" / "first.bin";
    const fs::path second =
        temp.path() / "b c" / core::pathFromUtf8("café.bin"); // a space, non-ASCII
    ASSERT_TRUE(update::appendCleanup(stateDir, first, disk).ok());
    EXPECT_EQ(readFile(stateDir / "cleanup.list"), lineOf(first));
    ASSERT_TRUE(update::appendCleanup(stateDir, second, disk).ok());
    EXPECT_EQ(readFile(stateDir / "cleanup.list"), lineOf(first) + lineOf(second));
}

TEST(CleanupList, OnlyAnAbsolutePathOnOneLineIsListed)
{
    TempDir temp;
    DiskFs disk;
    const fs::path stateDir = temp.path() / "U";
    for (const fs::path& bad : {fs::path("relative/file.bin"), fs::path(), fs::path("file.bin"),
                                temp.path() / "two\nlines", temp.path() / "carriage\rreturn"}) {
        const core::Status status = update::appendCleanup(stateDir, bad, disk);
        ASSERT_FALSE(status.ok()) << bad;
        EXPECT_EQ(status.error().code, core::ErrorCode::InvalidArgument) << bad;
    }
    EXPECT_FALSE(fs::exists(stateDir / "cleanup.list")) << "nothing was written";
}

TEST(CleanupList, AppendingGoesThroughTheSeamSoACrashAtAnyStepLeavesAWholeList)
{
    // Run once to count the changing operations, then die before each: afterwards the list is
    // absent or holds exactly the leftover, whole.
    std::uint64_t operations = 0;
    {
        TempDir temp;
        CrashingFs crashing;
        const core::Status appended =
            update::appendCleanup(temp.path() / "U", temp.path() / "leftover.bin", crashing);
        ASSERT_TRUE(appended.ok()) << appended.error().describe();
        operations = crashing.operations();
        ASSERT_GE(operations, 1u) << "the append is counted: it went through the seam";
    }
    for (std::uint64_t k = 1; k <= operations; ++k) {
        TempDir temp;
        CrashingFs crashing;
        crashing.crashBefore(k);
        const fs::path leftover = temp.path() / "leftover.bin";
        EXPECT_THROW((void)update::appendCleanup(temp.path() / "U", leftover, crashing), Crash)
            << "operation " << k;
        const fs::path list = temp.path() / "U" / "cleanup.list";
        if (fs::exists(list)) {
            EXPECT_EQ(readFile(list), lineOf(leftover)) << "operation " << k;
        }
    }
}

TEST(CleanupList, AListRewrittenAfterACrashIsStillWholeOrTheOldOne)
{
    // The rewrite is one writeDurable (or the removal of the file): a crash before it leaves the
    // old list, which names the same leftovers, so nothing is lost and the next start finishes.
    CleanupWorld world;
    LockedFs locked({"locked.bin"});
    const fs::path stuck = world.file(world.stateDir / "run-x" / "locked.bin");
    const fs::path loose = world.file(world.stateDir / "loose.tmp");
    world.listLines({world.stateDir / "run-x", loose});
    const std::string before = readFile(world.list);

    // A crash on the removal of the second line's file: the list on disk is still `before`.
    class CrashOnLoose final : public LockedFs {
      public:
        using LockedFs::LockedFs;
        core::Status removeFile(const fs::path& p) override
        {
            if (p.filename() == "loose.tmp") {
                throw Crash{1};
            }
            return LockedFs::removeFile(p);
        }
    } crashing({"locked.bin"});
    EXPECT_THROW((void)update::processCleanupList(world.stateDir, world.root, crashing), Crash);
    EXPECT_EQ(readFile(world.list), before);
    EXPECT_TRUE(fs::exists(stuck));
    EXPECT_TRUE(fs::exists(loose));
    // And the next start gets on with it.
    ASSERT_TRUE(update::processCleanupList(world.stateDir, world.root, locked).ok());
    EXPECT_FALSE(fs::exists(loose));
    EXPECT_EQ(readFile(world.list), lineOf(world.stateDir / "run-x"));
}

TEST(CleanupList, NoListIsOkAndCreatesNothing)
{
    CleanupWorld world;
    ASSERT_TRUE(update::processCleanupList(world.stateDir, world.root, world.disk).ok());
    EXPECT_FALSE(fs::exists(world.list));
    ASSERT_TRUE(
        update::processCleanupList(world.temp.path() / "never_made", world.root, world.disk).ok());
    EXPECT_FALSE(fs::exists(world.temp.path() / "never_made"));
}

TEST(CleanupList, LeftoversUnderTheStateFolderAndTheTransactionFolderAreRemovedWhole)
{
    CleanupWorld world;
    // A helper copy's folder, a staged tree, a loose file, and a transaction's old/ and new/.
    world.file(world.stateDir / "run-3fa9" / "katana_updater.exe");
    world.file(world.stateDir / "run-3fa9" / "selftest.txt");
    world.file(world.stateDir / "stage-1792312800" / "bin" / "katana.exe");
    world.file(world.stateDir / "stage-1792312800" / "share" / "katana" / "deep" / "er" / "x.txt");
    world.file(world.stateDir / "loose.tmp");
    world.file(world.work / "tx1" / "old" / "bin" / "a.dll");
    world.file(world.work / "tx1" / "new" / "bin" / "a.dll");
    fs::create_directories(world.work / "tx1" / "empty_dir");
    const std::vector<fs::path> leftovers{world.stateDir / "run-3fa9",
                                          world.stateDir / "stage-1792312800",
                                          world.stateDir / "loose.tmp", world.work / "tx1"};
    for (const fs::path& leftover : leftovers) {
        ASSERT_TRUE(update::appendCleanup(world.stateDir, leftover, world.disk).ok());
    }
    ASSERT_TRUE(update::processCleanupList(world.stateDir, world.root, world.disk).ok());
    for (const fs::path& leftover : leftovers) {
        EXPECT_FALSE(fs::exists(leftover)) << leftover;
    }
    EXPECT_FALSE(fs::exists(world.list)) << "nothing is left to list";
    EXPECT_TRUE(fs::is_directory(world.stateDir));
    EXPECT_TRUE(fs::is_directory(world.work)) << "the places themselves stay";
}

TEST(CleanupList, ALineThatIsNotInTheStateFolderOrTheTransactionFolderIsDroppedAndUntouched)
{
    CleanupWorld world;
    const fs::path outside = world.file(world.temp.path() / "victim.txt");
    const fs::path inInstall =
        world.file(world.root / "bin" / "katana.exe"); // R, but not R/.update
    const fs::path inSibling =
        world.file(world.temp.path() / "U-evil" / "x"); // shares U's name as a prefix
    const fs::path inWorkSibling = world.file(world.root / ".update-evil" / "x");
    const fs::path dotDot =
        world.stateDir / "run-a" / ".." / ".." / "victim.txt"; // lexically outside
    world.file(world.stateDir / "run-a" / "keep.txt");
    world.listLines({outside, inInstall, inSibling, inWorkSibling, dotDot});
    // Lines that are not absolute paths at all, and blank lines, written by hand.
    std::string text = readFile(world.list);
    text += "relative/victim.txt\n\n   \nvictim.txt\n";
    writeFile(world.list, text);

    ASSERT_TRUE(update::processCleanupList(world.stateDir, world.root, world.disk).ok());
    EXPECT_TRUE(fs::exists(outside));
    EXPECT_TRUE(fs::exists(inInstall)) << "R/bin is the install, not the work folder";
    EXPECT_TRUE(fs::exists(inSibling));
    EXPECT_TRUE(fs::exists(inWorkSibling));
    EXPECT_TRUE(fs::exists(world.stateDir / "run-a" / "keep.txt"));
    EXPECT_FALSE(fs::exists(world.list)) << "dropped lines are not kept";
}

TEST(CleanupList, TheStateFolderAndTheWorkFolderThemselvesAreNeverRemoved)
{
    CleanupWorld world;
    world.file(world.stateDir / "state.json", "{}");
    world.file(world.work / "lock");
    world.listLines({world.stateDir, world.work, world.stateDir / ".",
                     world.work / "tx1" / ".." / ".", world.list});
    ASSERT_TRUE(update::processCleanupList(world.stateDir, world.root, world.disk).ok());
    EXPECT_TRUE(fs::exists(world.stateDir / "state.json"));
    EXPECT_TRUE(fs::exists(world.work / "lock"));
    EXPECT_TRUE(fs::is_directory(world.work));
}

TEST(CleanupList, WithNoRootOnlyTheStateFolderIsAPlaceTheListMayName)
{
    CleanupWorld world;
    const fs::path inWork = world.file(world.work / "tx1" / "old.dll");
    const fs::path inState = world.file(world.stateDir / "run-1" / "x.exe");
    world.listLines({inWork, inState});
    ASSERT_TRUE(update::processCleanupList(world.stateDir, fs::path(), world.disk).ok());
    EXPECT_TRUE(fs::exists(inWork))
        << "R/.update is not known, so nothing there is the list's to remove";
    EXPECT_FALSE(fs::exists(inState));
}

TEST(CleanupList, ALineNamingAPathThatIsAlreadyGoneIsDroppedWithoutFuss)
{
    CleanupWorld world;
    world.listLines({world.stateDir / "run-gone", world.work / "tx-gone" / "x"});
    ASSERT_TRUE(update::processCleanupList(world.stateDir, world.root, world.disk).ok());
    EXPECT_FALSE(fs::exists(world.list));
}

TEST(CleanupList, ALineWithAWindowsLineEndingAndDuplicatesIsReadAsOneEntry)
{
    CleanupWorld world;
    const fs::path leftover = world.file(world.stateDir / "once.tmp");
    writeFile(world.list,
              core::pathToUtf8(leftover) + "\r\n" + core::pathToUtf8(leftover) + "\r\n\r\n");
    ASSERT_TRUE(update::processCleanupList(world.stateDir, world.root, world.disk).ok());
    EXPECT_FALSE(fs::exists(leftover));
    EXPECT_FALSE(fs::exists(world.list));
}

TEST(CleanupList, ALineOfBytesThatAreNotUtf8IsReadWithoutThrowingAndANonAsciiLeftoverIsRemoved)
{
    // The list is user-writable text read first thing at start-up, where nothing catches an
    // exception. The names are written as bytes: e acute (C3 A9) in a folder of the state
    // folder, two CJK characters (E5 9C B0 E5 9B B3) in one of the work folder. A line is
    // taken as UTF-8 (core::pathFromUtf8); read by the code page, "caf" + C3 A9 names another
    // folder on Windows, and the leftover stays.
    CleanupWorld world;
    const fs::path accented = world.stateDir / core::pathFromUtf8("caf\xC3\xA9");
    const fs::path japanese =
        world.work / "tx1" / core::pathFromUtf8("\xE5\x9C\xB0\xE5\x9B\xB3");
    world.file(accented / "x.bin");
    world.file(japanese / "y.bin");
    const fs::path victim = world.file(world.temp.path() / "victim.txt");

    const std::string under = core::pathToUtf8(world.stateDir / "run-1") + "/";
    std::string text;
    text += under + "\xFF\xFE\n";             // not UTF-8 at all
    text += under + "caf\xC3\n";              // a sequence cut short
    text += core::pathToUtf8(world.temp.path()) + "/\xC0\xAF\n"; // an overlong '/'
    text += "\xFF\xFE\n";                     // and not absolute either
    text += lineOf(accented);
    text += lineOf(japanese);
    writeFile(world.list, text);

    bool ok = false;
    ASSERT_NO_THROW(ok = update::processCleanupList(world.stateDir, world.root, world.disk).ok());
    EXPECT_TRUE(ok);
    EXPECT_FALSE(fs::exists(accented)) << "the folder named in UTF-8 is the one removed";
    EXPECT_FALSE(fs::exists(japanese));
    EXPECT_TRUE(fs::is_directory(world.work / "tx1")) << "only what the line named";
    EXPECT_TRUE(fs::exists(victim));
    EXPECT_FALSE(fs::exists(world.list)) << "no line is left that could be retried";
}

TEST(CleanupList, ALockedFileStaysListedAndEverythingElseIsRemoved)
{
    CleanupWorld world;
    LockedFs locked({"locked.bin"});
    const fs::path folder = world.stateDir / "run-x";
    const fs::path stuck = world.file(folder / "locked.bin");
    const fs::path free = world.file(folder / "other.bin");
    const fs::path plain = world.file(world.stateDir / "plain.tmp");
    world.listLines({folder, plain, folder, plain});
    ASSERT_TRUE(update::processCleanupList(world.stateDir, world.root, locked).ok())
        << "a file still locked is the list's business, not an error";
    EXPECT_TRUE(fs::exists(stuck));
    EXPECT_FALSE(fs::exists(free)) << "what could be removed was";
    EXPECT_FALSE(fs::exists(plain));
    EXPECT_EQ(readFile(world.list), lineOf(folder))
        << "only the folder that could not be removed, once";

    // The next start, with the lock gone: removed, and the list with it.
    ASSERT_TRUE(update::processCleanupList(world.stateDir, world.root, world.disk).ok());
    EXPECT_FALSE(fs::exists(folder));
    EXPECT_FALSE(fs::exists(world.list));
}

#ifdef _WIN32
TEST(CleanupList, AFileReallyHeldOpenByAnotherProgramStaysListedUntilItIsReleased)
{
    CleanupWorld world;
    const fs::path held = world.file(world.stateDir / "run-1" / "katana_updater.exe");
    ASSERT_TRUE(update::appendCleanup(world.stateDir, world.stateDir / "run-1", world.disk).ok());
    HANDLE handle =
        CreateFileW(held.c_str(), GENERIC_READ, FILE_SHARE_READ, nullptr, OPEN_EXISTING,
                    FILE_ATTRIBUTE_NORMAL, nullptr); // no FILE_SHARE_DELETE: like a running image
    ASSERT_NE(handle, INVALID_HANDLE_VALUE);
    const core::Status whileHeld =
        update::processCleanupList(world.stateDir, world.root, world.disk);
    EXPECT_TRUE(whileHeld.ok());
    EXPECT_TRUE(fs::exists(held));
    EXPECT_EQ(readFile(world.list), lineOf(world.stateDir / "run-1"));
    CloseHandle(handle);
    ASSERT_TRUE(update::processCleanupList(world.stateDir, world.root, world.disk).ok());
    EXPECT_FALSE(fs::exists(held.parent_path()));
    EXPECT_FALSE(fs::exists(world.list));
}
#endif

#ifdef _WIN32
TEST(CleanupList, OnWindowsANameThatTheSystemReadsAsAnotherIsNotALineThisProgramWrote)
{
    // Windows drops the dots and spaces at the end of a name: "U\..." is U itself, "U\. " too, and
    // ".. " may be the folder above. Each passes the test that the line lies under U (or
    // R/.update) when read as text. The state and the lock - the journals - are what would go.
    CleanupWorld world;
    world.file(world.stateDir / "state.json", "{}");
    world.file(world.stateDir / "run-a" / "keep.txt");
    world.file(world.work / "lock");
    world.file(world.work / "tx1" / "journal");
    std::string text;
    for (const fs::path& base : {world.stateDir, world.work}) {
        for (const char* name : {"...", "....", ". ", ".. ", "run-a.", "run-a ", "tx1."}) {
            text += core::pathToUtf8(base) + "\\" + name + "\n";
        }
        text += core::pathToUtf8(base) + "\\tx1\\...\n"; // tx1\... is tx1
        text += core::pathToUtf8(base) + "\\run-a\\keep.txt.\n";
    }
    writeFile(world.list, text);
    ASSERT_TRUE(update::processCleanupList(world.stateDir, world.root, world.disk).ok());
    EXPECT_TRUE(fs::exists(world.stateDir / "state.json"));
    EXPECT_TRUE(fs::exists(world.stateDir / "run-a" / "keep.txt"));
    EXPECT_TRUE(fs::exists(world.work / "lock"));
    EXPECT_TRUE(fs::exists(world.work / "tx1" / "journal"));
    EXPECT_TRUE(fs::is_directory(world.root));
    EXPECT_FALSE(fs::exists(world.list)) << "and the lines are dropped, not kept";
}
#endif

TEST(CleanupList, ATooLongListIsDiscardedWholeAndNothingInItIsActedOn)
{
    CleanupWorld world;
    const fs::path victim = world.file(world.stateDir / "first_line_victim.tmp");
    std::string text = lineOf(victim);
    while (text.size() < 2 * 1024 * 1024) {
        text += lineOf(world.stateDir / ("pad-" + std::to_string(text.size())));
    }
    writeFile(world.list, text);
    const core::Status status = update::processCleanupList(world.stateDir, world.root, world.disk);
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(status.error().code, core::ErrorCode::InvalidArgument);
    EXPECT_TRUE(fs::exists(victim))
        << "a list this size is not one the program wrote: none of it is believed";
    EXPECT_FALSE(fs::exists(world.list));
}

TEST(CleanupList, AListThatIsAFolderIsAnError)
{
    CleanupWorld world;
    fs::create_directory(world.list);
    const core::Status status = update::processCleanupList(world.stateDir, world.root, world.disk);
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(status.error().code, core::ErrorCode::InvalidState);
}

TEST(CleanupList, ALineThatWouldReachOutThroughALinkIsNotFollowed)
{
    CleanupWorld world;
    const fs::path outside = world.temp.path() / "outside";
    const fs::path victim = world.file(outside / "f.txt");
    world.file(outside / "sub" / "g.txt");
    fs::create_directories(world.stateDir / "run-a");
    DirectoryLink link(world.stateDir / "run-a" / "link", outside);
    if (!link.made()) {
        GTEST_SKIP() << "this machine does not let the test make a directory link";
    }
    // Written by hand: the file behind the link, and the folder that holds the link.
    world.listLines({world.stateDir / "run-a" / "link" / "f.txt",
                     world.stateDir / "run-a" / "link" / "sub" / "g.txt"});
    ASSERT_TRUE(update::processCleanupList(world.stateDir, world.root, world.disk).ok());
    EXPECT_TRUE(fs::exists(victim))
        << "a file reached through a link out of the folder is not removed";
    EXPECT_TRUE(fs::exists(outside / "sub" / "g.txt"));
    EXPECT_FALSE(fs::exists(world.list)) << "and the lines are dropped";
}

TEST(CleanupList, ALinkIsRemovedItselfAndWhatItPointsAtIsNotWalked)
{
    CleanupWorld world;
    const fs::path outside = world.temp.path() / "outside";
    const fs::path victim = world.file(outside / "f.txt");
    world.file(outside / "sub" / "g.txt");
    fs::create_directories(world.stateDir / "run-a");
    fs::create_directories(world.stateDir / "run-b" / "inner");
    DirectoryLink leaf(world.stateDir / "run-a" / "link", outside);
    DirectoryLink inside(world.stateDir / "run-b" / "inner" / "link", outside);
    if (!leaf.made() || !inside.made()) {
        GTEST_SKIP() << "this machine does not let the test make a directory link";
    }
    world.file(world.stateDir / "run-b" / "own.txt");
    // run-a/link is named itself; run-b holds a link somewhere in it.
    world.listLines({world.stateDir / "run-a" / "link", world.stateDir / "run-b"});
    ASSERT_TRUE(update::processCleanupList(world.stateDir, world.root, world.disk).ok());
    EXPECT_FALSE(fs::exists(world.stateDir / "run-a" / "link")) << "the link itself is a leftover";
    EXPECT_FALSE(fs::exists(world.stateDir / "run-b")) << "run-b, with the link in it, is gone";
    EXPECT_TRUE(fs::exists(victim)) << "the folder the links pointed at is untouched";
    EXPECT_TRUE(fs::exists(outside / "sub" / "g.txt"));
    EXPECT_FALSE(fs::exists(world.list));
}

TEST(CleanupList, AStateFolderReachedThroughALinkOfItsOwnIsStillTheStateFolder)
{
    // %LOCALAPPDATA% redirected to another disk is a link, and every path under it is spelled
    // through it: the base being a link is not what the list guards against.
    CleanupWorld world;
    const fs::path alias = world.temp.path() / "U-alias";
    DirectoryLink link(alias, world.stateDir);
    if (!link.made()) {
        GTEST_SKIP() << "this machine does not let the test make a directory link";
    }
    const fs::path leftover = world.file(world.stateDir / "run-1" / "x.exe");
    DiskFs disk;
    ASSERT_TRUE(update::appendCleanup(alias, alias / "run-1", disk).ok());
    ASSERT_TRUE(update::processCleanupList(alias, world.root, disk).ok());
    EXPECT_FALSE(fs::exists(leftover));
    EXPECT_FALSE(fs::exists(world.list));
}

TEST(CleanupList, AFolderNestedDeeperThanTheUpdaterEverMakesIsLeftAlone)
{
    CleanupWorld world;
    fs::path deep = world.stateDir / "run-deep";
    for (int i = 0; i < 70; ++i) {
        deep /= "d";
    }
    std::error_code error;
    fs::create_directories(deep, error);
    if (error) {
        GTEST_SKIP() << "this machine cannot make a path that long: " << error.message();
    }
    world.listLines({world.stateDir / "run-deep"});
    ASSERT_TRUE(update::processCleanupList(world.stateDir, world.root, world.disk).ok());
    EXPECT_TRUE(fs::exists(deep))
        << "the walk gives up past 64 levels instead of recursing without end";
    EXPECT_EQ(readFile(world.list), lineOf(world.stateDir / "run-deep")) << "and it stays listed";
}
