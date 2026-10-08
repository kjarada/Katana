#pragma once

// What the swap's tests share (test_apply.cpp, test_recover.cpp, test_fault_loops.cpp,
// test_journal.cpp): a scratch folder, a portable FsOps over std::filesystem, doubles for the
// other seams, releases and installs built from specifications, and the whole-tree hash the
// fault loops compare.
//
// WHERE THE EXPECTATIONS COME FROM (CLAUDE.md, section 4). The tree an update must leave is
// BUILT HERE, from the specification of the release, in a separate folder, by writing each file
// the specification lists - never read back from what the swap produced. The manifests are
// signed here with the RFC 8032 test key, through OpenSSL, which is not the code under test; the
// T3 fixtures (tests/update/fixtures, made by the Python producer and signed by `openssl`) stand
// beside them as a second, independent source.
//
// StdFs is portable and does not flush: the swap's algorithm is the same on every platform and
// the loops run it hundreds of times. The real Windows file system is tested separately and the
// swap is run once through it (test_journal.cpp, test_apply.cpp, `#ifdef _WIN32`).

#include <gtest/gtest.h>

#include <algorithm>
#include <atomic>
#include <chrono>
#include <cstdint>
#include <filesystem>
#include <fstream>
#include <functional>
#include <map>
#include <memory>
#include <mutex>
#include <optional>
#include <set>
#include <sstream>
#include <string>
#include <thread>
#include <string_view>
#include <vector>

#include "katana/core/path_text.hpp"
#include "katana/core/sha256.hpp"
#include "katana/update/apply.hpp"
#include "katana/update/fault_fs.hpp"
#include "katana/update/fsops.hpp"
#include "katana/update/journal.hpp"
#include "katana/update/manifest.hpp"
#include "katana/update/process.hpp"
#include "katana/update/recover.hpp"
#include "katana/update/registry.hpp"
#include "katana/update/state.hpp"
#include "test_key.hpp"

#if __has_include(<openssl/evp.h>)
#include <openssl/evp.h>
#define KATANA_TEST_CAN_SIGN 1
#else
#define KATANA_TEST_CAN_SIGN 0
#endif

namespace katana::update::harness {

namespace fs = std::filesystem;
using Path = fs::path;

// ---- a folder that goes away --------------------------------------------------------------------

class Scratch {
  public:
    explicit Scratch(const std::string& label)
    {
        static std::atomic<unsigned> counter{0};
        const std::string seed = label + ":" +
                                 std::to_string(std::chrono::steady_clock::now().time_since_epoch().count()) +
                                 ":" + std::to_string(counter++);
        path_ = fs::temp_directory_path() / ("katana-update-" + core::sha256Hex(seed).substr(0, 12));
        std::error_code ec;
        fs::remove_all(path_, ec);
        fs::create_directories(path_);
    }
    Scratch(const Scratch&) = delete;
    Scratch& operator=(const Scratch&) = delete;
    ~Scratch()
    {
        std::error_code ec;
        fs::remove_all(path_, ec);
    }
    [[nodiscard]] const Path& path() const { return path_; }
    [[nodiscard]] Path operator/(std::string_view name) const { return path_ / std::string(name); }

  private:
    Path path_;
};

inline void writeText(const Path& path, std::string_view bytes)
{
    fs::create_directories(path.parent_path());
    std::ofstream out(path, std::ios::binary | std::ios::trunc);
    out.write(bytes.data(), static_cast<std::streamsize>(bytes.size()));
}

inline std::string readText(const Path& path)
{
    std::ifstream in(path, std::ios::binary);
    return std::string((std::istreambuf_iterator<char>(in)), std::istreambuf_iterator<char>());
}

inline bool isPresent(const Path& path)
{
    std::error_code ec;
    return fs::exists(fs::symlink_status(path, ec));
}

// Windows says "access denied" or "sharing violation" for a moment about a file a virus scanner
// is looking at - the real file system retries those (fsops.hpp), and so does the one the loops
// use, so that a scanner cannot make a loop of hundreds of iterations fail once.
inline bool transient(const std::error_code& ec)
{
#ifdef _WIN32
    return ec.category() == std::system_category() && (ec.value() == 5 || ec.value() == 32 || ec.value() == 33);
#else
    (void)ec;
    return false;
#endif
}

template <typename Operation>
std::error_code retrying(Operation operation)
{
    std::error_code ec;
    for (int attempt = 0; attempt < 150; ++attempt) {
        ec.clear();
        operation(ec);
        if (!ec || !transient(ec)) {
            return ec;
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(20));
    }
    return ec;
}

inline void removeAllWithRetry(const Path& path)
{
    for (int attempt = 0; attempt < 150; ++attempt) {
        std::error_code ec;
        fs::remove_all(path, ec);
        std::error_code probe;
        if (!fs::exists(fs::symlink_status(path, probe))) {
            return;
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(20));
    }
}

// Makes `to` a copy of `from`, quickly, for a loop that does it hundreds of times. A file is
// HARD-LINKED where it can be: the swap never changes a file in place - it moves, creates and
// deletes - so a second name for the same bytes is as good as a copy and costs no data write
// (on a machine whose virus scanner looks at every file written, the difference is a factor of
// several). The exceptions are the files that ARE appended to in place, the journal and the
// recovery log: those are copied. Where a link cannot be made (another volume, a file system
// without them) the file is copied.
inline void replaceTree(const Path& from, const Path& to)
{
    std::error_code ec;
    removeAllWithRetry(to);
    fs::create_directories(to, ec);
    ASSERT_FALSE(ec) << ec.message();
    for (fs::recursive_directory_iterator it(from, ec), end; !ec && it != end; it.increment(ec)) {
        const Path target = to / it->path().lexically_relative(from);
        std::error_code failure;
        if (it->is_directory(failure)) {
            fs::create_directories(target, failure);
        } else {
            const std::string name = it->path().filename().string();
            const bool appendedTo = name == "journal" || name == "recovery.log";
            bool linked = false;
            if (!appendedTo) {
                fs::create_hard_link(it->path(), target, failure);
                linked = !failure;
            }
            if (!linked) {
                failure.clear();
                fs::copy_file(it->path(), target, fs::copy_options::overwrite_existing, failure);
            }
        }
        ASSERT_FALSE(failure) << it->path().string() << ": " << failure.message();
    }
    ASSERT_FALSE(ec) << ec.message();
}

// ---- a file system over std::filesystem ----------------------------------------------------------

inline core::Error osError(core::ErrorCode code, std::string message, const Path& path, int os = 0)
{
    return core::makeError(code, std::move(message),
                           "os_error=" + std::to_string(os) + " " + core::pathToUtf8(path));
}

class StdFs : public FsOps {
  public:
    // The locks held by any StdFs of this process: a lock is exclusive between holders, as
    // the real one is between processes.
    class Lock final : public FileLock {
      public:
        explicit Lock(std::string key) : key_(std::move(key)) {}
        ~Lock() override
        {
            std::lock_guard guard(mutex());
            held().erase(key_);
        }

      private:
        std::string key_;
    };

    core::Result<EntryInfo> stat(const Path& path) const override
    {
        std::error_code ec;
        const fs::file_status link = fs::symlink_status(path, ec);
        if (link.type() == fs::file_type::not_found || ec == std::errc::no_such_file_or_directory ||
            ec == std::errc::not_a_directory) {
            return EntryInfo{};
        }
        if (ec) {
            return osError(core::ErrorCode::FileImportFailure, ec.message(), path, ec.value());
        }
        EntryInfo info;
        info.reparsePoint = link.type() == fs::file_type::symlink;
        const fs::file_status real = fs::status(path, ec);
        if (real.type() == fs::file_type::directory) {
            info.kind = EntryKind::Directory;
        } else if (real.type() == fs::file_type::regular) {
            info.kind = EntryKind::File;
            info.size = fs::file_size(path, ec);
        } else {
            info.kind = info.reparsePoint ? EntryKind::File : EntryKind::Other;
        }
        return info;
    }

    core::Result<std::vector<Path>> listDirectory(const Path& directory) const override
    {
        std::error_code ec;
        std::vector<Path> names;
        for (fs::directory_iterator it(directory, ec), end; !ec && it != end; it.increment(ec)) {
            names.push_back(it->path());
        }
        if (ec) {
            return osError(core::ErrorCode::NotFound, ec.message(), directory, ec.value());
        }
        return names;
    }

    core::Result<std::string> readFile(const Path& path) const override
    {
        std::error_code ec;
        if (!fs::is_regular_file(path, ec)) {
            return osError(core::ErrorCode::NotFound, "no such file", path, 2);
        }
        return readText(path);
    }

    core::Result<Path> resolve(const Path& path) const override
    {
        std::error_code ec;
        const Path resolved = fs::weakly_canonical(path, ec);
        if (ec) {
            return osError(core::ErrorCode::FileImportFailure, ec.message(), path, ec.value());
        }
        return resolved;
    }

    core::Result<std::uint64_t> freeSpace(const Path& path) const override
    {
        Path probe = path;
        std::error_code ec;
        while (!probe.empty() && !fs::exists(probe, ec)) {
            probe = probe.parent_path();
        }
        const fs::space_info info = fs::space(probe, ec);
        if (ec) {
            return osError(core::ErrorCode::FileImportFailure, ec.message(), path, ec.value());
        }
        return static_cast<std::uint64_t>(info.available);
    }

    core::Status makeDirectories(const Path& directory) override
    {
        std::error_code ec;
        fs::create_directories(directory, ec);
        if (ec) {
            return osError(core::ErrorCode::FileExportFailure, ec.message(), directory, ec.value());
        }
        return {};
    }

    core::Status copyFile(const Path& from, const Path& to) override
    {
        if (isPresent(to)) {
            return osError(core::ErrorCode::AlreadyExists, "the target exists", to, 80);
        }
        const std::error_code ec =
            retrying([&](std::error_code& e) { fs::copy_file(from, to, fs::copy_options::none, e); });
        if (ec) {
            return osError(core::ErrorCode::FileExportFailure, ec.message(), to, ec.value());
        }
        return {};
    }

    core::Status renameFile(const Path& from, const Path& to) override
    {
        if (isPresent(to)) {
            return osError(core::ErrorCode::AlreadyExists, "the target exists", to, 183);
        }
        const std::error_code ec = retrying([&](std::error_code& e) { fs::rename(from, to, e); });
        if (ec) {
            return osError(core::ErrorCode::FileExportFailure, ec.message(), from, ec.value());
        }
        return {};
    }

    core::Status removeFile(const Path& path) override
    {
        std::error_code ec;
        if (fs::is_directory(fs::symlink_status(path, ec))) {
            return osError(core::ErrorCode::FileExportFailure, "a directory", path, 5);
        }
        ec = retrying([&](std::error_code& e) { fs::remove(path, e); });
        if (ec) {
            return osError(core::ErrorCode::FileExportFailure, ec.message(), path, ec.value());
        }
        return {};
    }

    core::Status removeEmptyDirectory(const Path& path) override
    {
        std::error_code ec;
        if (!fs::exists(fs::symlink_status(path, ec))) {
            return {};
        }
        if (!fs::is_directory(fs::symlink_status(path, ec))) {
            return osError(core::ErrorCode::InvalidState, "not a directory", path, 267);
        }
        if (!fs::is_empty(path, ec)) {
            return osError(core::ErrorCode::InvalidState, "the folder is not empty", path, 145);
        }
        fs::remove(path, ec);
        if (ec) {
            return osError(core::ErrorCode::FileExportFailure, ec.message(), path, ec.value());
        }
        return {};
    }

    core::Status appendDurable(const Path& path, std::string_view bytes) override
    {
        std::ofstream out(path, std::ios::binary | std::ios::app);
        out.write(bytes.data(), static_cast<std::streamsize>(bytes.size()));
        if (!out) {
            return osError(core::ErrorCode::FileExportFailure, "write failed", path);
        }
        return {};
    }

    core::Status writeDurable(const Path& path, std::string_view bytes) override
    {
        std::ofstream out(path, std::ios::binary | std::ios::trunc);
        out.write(bytes.data(), static_cast<std::streamsize>(bytes.size()));
        if (!out) {
            return osError(core::ErrorCode::FileExportFailure, "write failed", path);
        }
        return {};
    }

    core::Status setExecutable(const Path&) override { return {}; }
    core::Status syncDirectory(const Path&) override { return {}; }

    core::Result<std::unique_ptr<FileLock>> tryLock(const Path& path) override
    {
        const std::string key = core::pathToUtf8(path.lexically_normal());
        std::lock_guard guard(mutex());
        if (!held().insert(key).second) {
            return core::makeError(core::ErrorCode::InvalidState, "held", "os_error=32 " + key);
        }
        std::error_code ec;
        fs::create_directories(path.parent_path(), ec);
        { std::ofstream touch(path, std::ios::binary | std::ios::app); }
        return std::unique_ptr<FileLock>(std::make_unique<Lock>(key));
    }

  private:
    static std::mutex& mutex()
    {
        static std::mutex m;
        return m;
    }
    static std::set<std::string>& held()
    {
        static std::set<std::string> keys;
        return keys;
    }
};

// A FsOps that passes everything to another, for a test to override the one operation it
// wants to break.
class ForwardingFs : public FsOps {
  public:
    explicit ForwardingFs(FsOps& inner) : inner_(inner) {}

    core::Result<EntryInfo> stat(const Path& p) const override { return inner_.stat(p); }
    core::Result<std::vector<Path>> listDirectory(const Path& p) const override
    {
        return inner_.listDirectory(p);
    }
    core::Result<std::string> readFile(const Path& p) const override { return inner_.readFile(p); }
    core::Result<Path> resolve(const Path& p) const override { return inner_.resolve(p); }
    core::Result<std::uint64_t> freeSpace(const Path& p) const override
    {
        return inner_.freeSpace(p);
    }
    core::Status makeDirectories(const Path& p) override { return inner_.makeDirectories(p); }
    core::Status copyFile(const Path& a, const Path& b) override { return inner_.copyFile(a, b); }
    core::Status renameFile(const Path& a, const Path& b) override { return inner_.renameFile(a, b); }
    core::Status removeFile(const Path& p) override { return inner_.removeFile(p); }
    core::Status removeEmptyDirectory(const Path& p) override
    {
        return inner_.removeEmptyDirectory(p);
    }
    core::Status appendDurable(const Path& p, std::string_view b) override
    {
        return inner_.appendDurable(p, b);
    }
    core::Status writeDurable(const Path& p, std::string_view b) override
    {
        return inner_.writeDurable(p, b);
    }
    core::Status setExecutable(const Path& p) override { return inner_.setExecutable(p); }
    core::Status syncDirectory(const Path& p) override { return inner_.syncDirectory(p); }
    core::Result<std::unique_ptr<FileLock>> tryLock(const Path& p) override
    {
        return inner_.tryLock(p);
    }

  protected:
    FsOps& inner_;
};

// Counts the renames it passes on.
class CountingFs final : public ForwardingFs {
  public:
    using ForwardingFs::ForwardingFs;
    core::Status renameFile(const Path& a, const Path& b) override
    {
        ++renames;
        return inner_.renameFile(a, b);
    }
    std::uint64_t renames = 0;
};

// A FsOps whose operations a test can intercept: each hook sees the call before it is passed
// on, and may answer for it (a Status other than "go on") or change what it reports.
class HookFs final : public ForwardingFs {
  public:
    using ForwardingFs::ForwardingFs;

    // Returning a value answers the call instead of passing it on.
    std::function<std::optional<core::Status>(const Path&, const Path&)> beforeCopy;
    std::function<void(const Path&, const Path&)> afterCopy; // after the copy was made
    std::function<std::optional<core::Status>(const Path&, const Path&)> beforeRename;
    std::function<std::optional<core::Status>(const Path&, std::string_view)> beforeAppend;
    std::function<void(const Path&, EntryInfo&)> adjustStat;
    std::function<std::optional<Path>(const Path&)> overrideResolve;
    std::optional<std::uint64_t> freeBytes;

    core::Result<EntryInfo> stat(const Path& p) const override
    {
        core::Result<EntryInfo> result = inner_.stat(p);
        if (result.ok() && adjustStat) {
            EntryInfo info = result.value();
            adjustStat(p, info);
            return info;
        }
        return result;
    }
    core::Result<Path> resolve(const Path& p) const override
    {
        if (overrideResolve) {
            if (const std::optional<Path> other = overrideResolve(p)) {
                return *other;
            }
        }
        return inner_.resolve(p);
    }
    core::Result<std::uint64_t> freeSpace(const Path& p) const override
    {
        return freeBytes ? core::Result<std::uint64_t>(*freeBytes) : inner_.freeSpace(p);
    }
    core::Status copyFile(const Path& a, const Path& b) override
    {
        if (beforeCopy) {
            if (const std::optional<core::Status> answer = beforeCopy(a, b)) {
                return *answer;
            }
        }
        const core::Status done = inner_.copyFile(a, b);
        if (afterCopy) {
            afterCopy(a, b);
        }
        return done;
    }
    core::Status renameFile(const Path& a, const Path& b) override
    {
        if (beforeRename) {
            if (const std::optional<core::Status> answer = beforeRename(a, b)) {
                return *answer;
            }
        }
        return inner_.renameFile(a, b);
    }
    core::Status appendDurable(const Path& p, std::string_view bytes) override
    {
        if (beforeAppend) {
            if (const std::optional<core::Status> answer = beforeAppend(p, bytes)) {
                return *answer;
            }
        }
        return inner_.appendDurable(p, bytes);
    }
};

inline core::Status injected(int osErrorNumber, const Path& path)
{
    return core::makeError(core::ErrorCode::FileExportFailure, "injected",
                           "os_error=" + std::to_string(osErrorNumber) + " " + core::pathToUtf8(path));
}

// Writes down every MUTATING operation it passes on, in the order FaultFs counts them (the
// eight of fsops.hpp), so that "the k-th operation" of a fault loop can be named: put it above
// the FaultFs and the log has an entry for every operation the FaultFs counted, the one it
// crashed before included.
class LogFs final : public ForwardingFs {
  public:
    using ForwardingFs::ForwardingFs;

    std::vector<std::string> log;

    core::Status makeDirectories(const Path& p) override
    {
        log.push_back("makeDirectories " + core::pathToUtf8(p));
        return inner_.makeDirectories(p);
    }
    core::Status copyFile(const Path& a, const Path& b) override
    {
        log.push_back("copyFile " + core::pathToUtf8(a) + " -> " + core::pathToUtf8(b));
        return inner_.copyFile(a, b);
    }
    core::Status renameFile(const Path& a, const Path& b) override
    {
        log.push_back("renameFile " + core::pathToUtf8(a) + " -> " + core::pathToUtf8(b));
        return inner_.renameFile(a, b);
    }
    core::Status removeFile(const Path& p) override
    {
        log.push_back("removeFile " + core::pathToUtf8(p));
        return inner_.removeFile(p);
    }
    core::Status removeEmptyDirectory(const Path& p) override
    {
        log.push_back("removeEmptyDirectory " + core::pathToUtf8(p));
        return inner_.removeEmptyDirectory(p);
    }
    core::Status appendDurable(const Path& p, std::string_view bytes) override
    {
        log.push_back("appendDurable " + core::pathToUtf8(p) + " " + std::string(bytes));
        return inner_.appendDurable(p, bytes);
    }
    core::Status writeDurable(const Path& p, std::string_view bytes) override
    {
        log.push_back("writeDurable " + core::pathToUtf8(p) + " " + std::string(bytes));
        return inner_.writeDurable(p, bytes);
    }
    core::Status setExecutable(const Path& p) override
    {
        log.push_back("setExecutable " + core::pathToUtf8(p));
        return inner_.setExecutable(p);
    }

    // The 1-based number of the first entry at or after `from` (1-based) that begins with
    // `prefix` and contains `needle`; 0 when there is none.
    [[nodiscard]] std::uint64_t find(std::string_view prefix, std::string_view needle = {},
                                     std::uint64_t from = 1) const
    {
        for (std::uint64_t i = from; i <= log.size(); ++i) {
            const std::string& entry = log[i - 1];
            if (entry.rfind(prefix, 0) == 0 && entry.find(needle) != std::string::npos) {
                return i;
            }
        }
        return 0;
    }
    [[nodiscard]] std::vector<std::uint64_t> all(std::string_view prefix) const
    {
        std::vector<std::uint64_t> found;
        for (std::uint64_t i = 1; i <= log.size(); ++i) {
            if (log[i - 1].rfind(prefix, 0) == 0) {
                found.push_back(i);
            }
        }
        return found;
    }
};

// Where the journal of the (single) transaction under R/.update stands; Empty when there is
// no folder or no complete record.
inline JournalState journalStateOf(const Path& root)
{
    std::error_code ec;
    for (fs::directory_iterator it(root / ".update", ec), end; !ec && it != end; it.increment(ec)) {
        if (it->is_directory(ec)) {
            const Path journal = it->path() / "journal";
            if (isPresent(journal)) {
                return parseJournal(readText(journal)).state;
            }
        }
    }
    return JournalState::Empty;
}

// ---- the other seams --------------------------------------------------------------------------

class FakeProcesses final : public ProcessOps {
  public:
    std::uint64_t pid = 1;
    // The parent (the window): alive for this many isAlive() questions, then gone. -1: forever.
    mutable int parentAliveFor = 0;
    // Who holds files under the root: for this many holdersUnder() questions, then nobody.
    // -1: forever.
    int holdersFor = 0;
    std::vector<RunningProgram> holders;
    // Set to let everyone go at once, whatever holdersFor says: a test that needs the helper to
    // stay waiting until another thread has done its part holds with holdersFor = -1 and
    // releases here.
    std::atomic<bool> release{false};
    bool scanFails = false;

    std::uint64_t currentPid() const override { return pid; }
    bool isAlive(std::uint64_t) const override
    {
        if (parentAliveFor < 0) {
            return true;
        }
        if (parentAliveFor == 0) {
            return false;
        }
        --parentAliveFor;
        return true;
    }
    core::Result<std::vector<RunningProgram>> holdersUnder(const Path&) const override
    {
        if (scanFails) {
            return core::makeError(core::ErrorCode::Internal, "the snapshot failed");
        }
        const int seen = ++scans;
        if (!release.load() && (holdersFor < 0 || seen <= holdersFor)) {
            return holders;
        }
        return std::vector<RunningProgram>{};
    }
    mutable std::atomic<int> scans{0};
};

class FakeLauncher final : public Launcher {
  public:
    std::vector<SpawnRequest> ran;
    std::vector<SpawnRequest> started;
    std::function<core::Result<SpawnResult>(const SpawnRequest&)> onRun;

    core::Result<SpawnResult> run(const SpawnRequest& request) override
    {
        ran.push_back(request);
        if (onRun) {
            return onRun(request);
        }
        return SpawnResult{};
    }
    core::Status startDetached(const SpawnRequest& request) override
    {
        started.push_back(request);
        return {};
    }
};

class FakeRegistry final : public RegistryOps {
  public:
    std::optional<Path> location;
    std::map<std::string, std::string> runOnce;
    std::vector<std::string> runOnceHistory; // every command ever registered
    int runOnceCleared = 0;
    std::vector<std::pair<std::string, std::string>> versions;

    core::Result<std::optional<Path>> installerLocation() const override { return location; }
    core::Status setRunOnce(std::string_view name, std::string_view command) override
    {
        runOnce[std::string(name)] = std::string(command);
        runOnceHistory.emplace_back(command);
        return {};
    }
    core::Status clearRunOnce(std::string_view name) override
    {
        runOnce.erase(std::string(name));
        ++runOnceCleared;
        return {};
    }
    core::Status setInstalledVersion(std::string_view version, std::string_view name) override
    {
        versions.emplace_back(std::string(version), std::string(name));
        return {};
    }
};

// ---- signing, with OpenSSL (not the code under test) -------------------------------------------------

inline std::string base64(const unsigned char* bytes, std::size_t size)
{
    static constexpr std::string_view alphabet =
        "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    std::string out;
    for (std::size_t i = 0; i < size; i += 3) {
        const std::uint32_t chunk = (static_cast<std::uint32_t>(bytes[i]) << 16) |
                                    (i + 1 < size ? static_cast<std::uint32_t>(bytes[i + 1]) << 8 : 0) |
                                    (i + 2 < size ? static_cast<std::uint32_t>(bytes[i + 2]) : 0);
        out += alphabet[(chunk >> 18) & 63];
        out += alphabet[(chunk >> 12) & 63];
        out += i + 1 < size ? alphabet[(chunk >> 6) & 63] : '=';
        out += i + 2 < size ? alphabet[chunk & 63] : '=';
    }
    return out;
}

inline bool canSign()
{
    return KATANA_TEST_CAN_SIGN != 0;
}

// True where a continuous-integration service runs the tests (it sets CI, and GitHub also
// GITHUB_ACTIONS).
inline bool inContinuousIntegration()
{
    return !core::environmentVariable("CI").empty() ||
           !core::environmentVariable("GITHUB_ACTIONS").empty();
}

// At the top of a test that signs. Where OpenSSL's headers are not found: skipped, and said so,
// on a developer's machine - but a FAILURE in continuous integration, because the 40-file crash
// loop, the recovery loop and the rename-failure loop are among the tests that sign, and a
// skip count cannot be told from T8's three placeholders: the central tests would stop
// running and nobody would see it.
#define REQUIRE_CAN_SIGN()                                                              \
    if (!::katana::update::harness::canSign()) {                                        \
        if (::katana::update::harness::inContinuousIntegration()) {                     \
            FAIL() << "OpenSSL headers are not available to sign the test manifests, "  \
                      "and in continuous integration the signing tests are not optional";\
        }                                                                               \
        GTEST_SKIP() << "OpenSSL headers are not available to sign the test manifests"; \
    }

// The signature line of `message` under the test key whose seed is `seedHex` (the active key
// by default): `ed25519 <keyid> <base64>` and a line feed.
inline std::string signLine(std::string_view message,
                            std::string_view seedHex = kTestActiveSeedHex,
                            std::string_view publicHex = kTestActivePublicHex)
{
#if KATANA_TEST_CAN_SIGN
    unsigned char seed[32];
    for (int i = 0; i < 32; ++i) {
        const auto nibble = [](char c) { return c <= '9' ? c - '0' : c - 'a' + 10; };
        seed[i] = static_cast<unsigned char>(nibble(seedHex[2 * i]) * 16 + nibble(seedHex[2 * i + 1]));
    }
    EVP_PKEY* key = EVP_PKEY_new_raw_private_key(EVP_PKEY_ED25519, nullptr, seed, 32);
    EVP_MD_CTX* context = EVP_MD_CTX_new();
    unsigned char signature[64];
    std::size_t length = sizeof(signature);
    const bool ok = key != nullptr && context != nullptr &&
                    EVP_DigestSignInit(context, nullptr, nullptr, nullptr, key) == 1 &&
                    EVP_DigestSign(context, signature, &length,
                                   reinterpret_cast<const unsigned char*>(message.data()),
                                   message.size()) == 1;
    EVP_MD_CTX_free(context);
    EVP_PKEY_free(key);
    if (!ok || length != 64) {
        return {};
    }
    return "ed25519 " + std::string(publicHex.substr(0, 16)) + " " + base64(signature, 64) + "\n";
#else
    (void)message;
    (void)seedHex;
    (void)publicHex;
    return {};
#endif
}

// ---- a release, from a specification ---------------------------------------------------------------

struct FileSpec {
    std::string path;
    std::string content;
    FileClass fileClass = FileClass::Data;
    bool executable = false;
};

struct ReleaseSpec {
    std::string version = "0.2.0";
    std::string tag = "v0.2.0";
    std::uint64_t sequence = 1000;
    std::string target = "win64";
    std::vector<FileSpec> files;

    [[nodiscard]] const FileSpec* find(std::string_view path) const
    {
        for (const FileSpec& file : files) {
            if (file.path == path) {
                return &file;
            }
        }
        return nullptr;
    }
};

inline std::string className(FileClass fileClass)
{
    switch (fileClass) {
    case FileClass::Program:
        return "program";
    case FileClass::Lib:
        return "lib";
    case FileClass::Data:
        return "data";
    case FileClass::Seed:
        return "seed";
    }
    return "data";
}

// The manifest as the producer writes it (manifest.hpp): sorted keys, files sorted by path.
// Where the bytes would sit in a payload is made up - the swap does not read the payload - but
// keeps the rules of parseManifest (csize at least 2, inside the payload).
inline std::string manifestJson(const ReleaseSpec& release)
{
    std::vector<const FileSpec*> sorted;
    for (const FileSpec& file : release.files) {
        sorted.push_back(&file);
    }
    std::sort(sorted.begin(), sorted.end(),
              [](const FileSpec* a, const FileSpec* b) { return a->path < b->path; });
    std::uint64_t offset = 44;
    std::string files;
    for (const FileSpec* file : sorted) {
        const std::uint64_t csize = std::max<std::uint64_t>(2, file->content.size() + 2);
        if (!files.empty()) {
            files += ",\n";
        }
        files += "{\"class\":\"" + className(file->fileClass) + "\",\"csize\":" + std::to_string(csize);
        if (file->executable) {
            files += ",\"exec\":true";
        }
        files += ",\"offset\":" + std::to_string(offset) + ",\"path\":\"" + file->path +
                 "\",\"sha256\":\"" + core::sha256Hex(file->content) +
                 "\",\"size\":" + std::to_string(file->content.size()) + "}";
        offset += csize;
    }
    const std::uint64_t payload = offset + 100;
    return "{\"applier\":1,\"channel\":\"stable\",\"expires\":\"2030-01-01T00:00:00Z\",\"files\":[\n" +
           files + "\n],\"notes_url\":\"https://github.com/kjarada/Katana/releases/tag/" +
           release.tag + "\",\"payload\":{\"codec\":\"deflate\",\"name\":\"Katana-" + release.version +
           "-" + release.target + ".update.zip\",\"size\":" + std::to_string(payload) +
           "},\"product\":\"katana\",\"published\":\"2026-10-01T00:00:00Z\",\"schema\":1,\"sequence\":" +
           std::to_string(release.sequence) + ",\"tag\":\"" + release.tag + "\",\"target\":\"" +
           release.target + "\",\"version\":\"" + release.version + "\"}\n";
}

// installed.json as the swap records it (apply.cpp, installedRecord), written out by hand here:
// sorted keys, one LF.
inline std::string installedJson(const ReleaseSpec& release)
{
    return "{\"product\":\"katana\",\"schema\":1,\"sequence\":" + std::to_string(release.sequence) +
           ",\"tag\":\"" + release.tag + "\",\"target\":\"" + release.target + "\",\"version\":\"" +
           release.version + "\"}\n";
}

inline constexpr std::string_view kInstallJson = "{\"schema\":1,\"target\":\"win64\"}";

// The files of a release at R, plus the install record that makes R an install.
inline void writeFiles(const Path& root, const ReleaseSpec& release)
{
    for (const FileSpec& file : release.files) {
        writeText(root / file.path, file.content);
    }
}

// R as the helper leaves an installed release: its files, install.json, installed.json and the
// signed manifest beside it.
inline void writeInstall(const Path& root, const ReleaseSpec& release)
{
    writeFiles(root, release);
    writeText(root / "share/katana/install.json", kInstallJson);
    writeText(root / "share/katana/installed.json", installedJson(release));
    const std::string json = manifestJson(release);
    writeText(root / "share/katana/installed.manifest.json", json);
    writeText(root / "share/katana/installed.manifest.json.sig", signLine(json));
}

// The stage folder of a release: every file at its path, and the signed manifest.
inline void writeStage(const Path& stage, const ReleaseSpec& release)
{
    writeFiles(stage, release);
    const std::string json = manifestJson(release);
    writeText(stage / "manifest.json", json);
    writeText(stage / "manifest.json.sig", signLine(json));
}

// ---- whole-tree hash ---------------------------------------------------------------------------

// Every entry under `root` except the helper's own working folder .update: a line per folder
// and per file (path, size, SHA-256), sorted. Two trees are the same when this is. A folder is
// part of it, so a rollback that leaves an empty folder the update made is a difference.
inline std::map<std::string, std::string> snapshot(const Path& root)
{
    std::map<std::string, std::string> entries;
    std::error_code ec;
    for (fs::recursive_directory_iterator it(root, ec), end; !ec && it != end; it.increment(ec)) {
        const Path relative = it->path().lexically_relative(root);
        const std::string generic = relative.generic_string();
        if (generic == ".update" || generic.rfind(".update/", 0) == 0) {
            continue;
        }
        if (it->is_directory(ec)) {
            entries[generic] = "dir";
        } else {
            const core::Result<std::string> hash = core::sha256FileHex(it->path());
            entries[generic] = "file " + std::to_string(fs::file_size(it->path(), ec)) + " " +
                               (hash.ok() ? hash.value() : std::string("unreadable"));
        }
    }
    return entries;
}

inline std::string treeHash(const Path& root)
{
    core::Sha256 hash;
    for (const auto& [name, what] : snapshot(root)) {
        hash.update(name);
        hash.update("\n");
        hash.update(what);
        hash.update("\n");
    }
    return hash.finishHex();
}

// The differences between two snapshots, one per line, for a failure message.
inline std::string difference(const std::map<std::string, std::string>& expected,
                              const std::map<std::string, std::string>& actual)
{
    std::ostringstream out;
    for (const auto& [name, what] : expected) {
        const auto found = actual.find(name);
        if (found == actual.end()) {
            out << "  missing:    " << name << "\n";
        } else if (found->second != what) {
            out << "  different:  " << name << "\n";
        }
    }
    for (const auto& [name, what] : actual) {
        if (!expected.contains(name)) {
            out << "  unexpected: " << name << "\n";
        }
    }
    return out.str();
}

// The names under R/.update other than the lock: what a finished transaction must not leave.
inline std::vector<std::string> leftoversInUpdate(const Path& root)
{
    std::vector<std::string> names;
    std::error_code ec;
    for (fs::directory_iterator it(root / ".update", ec), end; !ec && it != end; it.increment(ec)) {
        const std::string name = it->path().filename().string();
        if (name != "lock") {
            names.push_back(name);
        }
    }
    return names;
}

// ---- a scenario: a v1 install, a v2 stage, the tree v2 must leave ------------------------------

struct Scenario {
    explicit Scenario(const std::string& label) : scratch(label)
    {
        root = scratch.path() / "install";
        stage = scratch.path() / "stage";
        state = scratch.path() / "state";
        expected = scratch.path() / "expected";
        fs::create_directories(state);
    }

    Scratch scratch;
    Path root;
    Path stage;
    Path state;
    Path expected; // the tree after a good update, built from the specification
    ReleaseSpec v1;
    ReleaseSpec v2;
    std::map<std::string, std::string> v1Snapshot; // the install as it is before the update, entry by entry
    std::string v1Hash; // the install as it is before the update
    std::string v2Hash; // the tree a good update must leave
};

// File contents: deterministic, of different sizes, with a NUL and a high byte in some.
inline std::string contentOf(std::string_view path, std::string_view version)
{
    std::string content = std::string(path) + "@" + std::string(version) + "\n";
    const std::size_t repeat = core::sha256Hex(path).front() % 7 + 1;
    const std::string unit = content;
    for (std::size_t i = 1; i < repeat; ++i) {
        content += unit;
    }
    if (path.size() % 5 == 0) {
        content.push_back('\0');
        content.push_back(static_cast<char>(0xE9));
    }
    return content;
}

// The 40-file release pair of the fault loops (see the list in test_fault_loops.cpp). v1 has
// 38 files, v2 has 40; the update changes 12, adds 5, removes 3 (one has been edited by the
// person and stays), creates 2 folders and empties 1.
inline void buildFortyFileScenario(Scenario& s)
{
    s.v1.version = "0.9.0";
    s.v1.tag = "v0.9.0";
    s.v1.sequence = 1000;
    s.v2.version = "0.10.0";
    s.v2.tag = "v0.10.0";
    s.v2.sequence = 2000;

    const auto add = [&](ReleaseSpec& release, std::string path, FileClass fileClass, bool exec,
                         std::string_view version) {
        release.files.push_back({path, contentOf(path, version), fileClass, exec});
    };
    const auto both = [&](const std::string& path, FileClass fileClass, bool exec, bool changed) {
        add(s.v1, path, fileClass, exec, "1");
        add(s.v2, path, fileClass, exec, changed ? "2" : "1");
    };

    both("bin/katana.exe", FileClass::Program, true, true);
    both("bin/katana_cli.exe", FileClass::Program, true, true);
    both("bin/katana_updater.exe", FileClass::Program, true, true);
    for (int i = 1; i <= 12; ++i) {
        const std::string name = std::string("bin/lib") + (i < 10 ? "0" : "") + std::to_string(i) + ".dll";
        both(name, FileClass::Lib, false, i == 3 || i == 4 || i == 7 || i == 11);
    }
    add(s.v1, "bin/old1.dll", FileClass::Lib, false, "1");
    add(s.v1, "bin/old2.dll", FileClass::Lib, false, "1");
    add(s.v2, "bin/lib13.dll", FileClass::Lib, false, "2");
    add(s.v2, "bin/lib14.dll", FileClass::Lib, false, "2");
    for (int i = 1; i <= 16; ++i) {
        const std::string name = std::string("share/katana/data/f") + (i < 10 ? "0" : "") + std::to_string(i) + ".dat";
        both(name, FileClass::Data, false, i == 2 || i == 5 || i == 9 || i == 13);
    }
    add(s.v1, "share/katana/legacy/old.txt", FileClass::Data, false, "1");
    add(s.v2, "share/katana/more/g1.dat", FileClass::Data, false, "2");
    add(s.v2, "share/katana/more/g2.dat", FileClass::Data, false, "2");
    s.v1.files.push_back({"share/katana/empty.txt", "", FileClass::Data, false});
    s.v2.files.push_back({"share/katana/empty.txt", "", FileClass::Data, false});
    both("etc/katana/settings.ini", FileClass::Seed, false, true);
    both("lib/plugins/p1.dat", FileClass::Data, false, false);
    both("lib/plugins/p2.dat", FileClass::Data, false, true);
    add(s.v2, "lib/plugins/extra/p3.dat", FileClass::Data, false, "2");

    // The install before the update: v1, and what a person did to it.
    writeInstall(s.root, s.v1);
    writeText(s.root / "etc/katana/settings.ini", "user=edited\n");              // a seed, edited
    writeText(s.root / "share/katana/data/f05.dat", "the person's own f05\n");   // changed by v2, edited
    writeText(s.root / "bin/old2.dll", "the person's own old2\n");               // removed by v2, edited
    writeText(s.root / "share/katana/user_notes.txt", "notes the update never heard of\n");
    writeText(s.root / "bin/user_tool.dll", "a plug-in nobody listed\n");
    writeStage(s.stage, s.v2);

    // The tree a good update leaves, written out from the specification: v2's files, except
    // the seed and the edited old2 (which stay as the person had them), then the person's
    // unknown files, then the records.
    for (const FileSpec& file : s.v2.files) {
        if (file.fileClass != FileClass::Seed) {
            writeText(s.expected / file.path, file.content);
        }
    }
    writeText(s.expected / "etc/katana/settings.ini", "user=edited\n");
    writeText(s.expected / "bin/old2.dll", "the person's own old2\n");
    writeText(s.expected / "share/katana/user_notes.txt", "notes the update never heard of\n");
    writeText(s.expected / "bin/user_tool.dll", "a plug-in nobody listed\n");
    writeText(s.expected / "share/katana/install.json", kInstallJson);
    writeText(s.expected / "share/katana/installed.json", installedJson(s.v2));
    writeText(s.expected / "share/katana/installed.manifest.json", manifestJson(s.v2));
    writeText(s.expected / "share/katana/installed.manifest.json.sig", signLine(manifestJson(s.v2)));

    s.v1Hash = treeHash(s.root);
    s.v1Snapshot = snapshot(s.root);
    s.v2Hash = treeHash(s.expected);
}

// ---- the T3 fixtures: a release pair made by the Python producer and signed by openssl --------------

inline Path fixtureFolder()
{
    return Path(KATANA_UPDATE_FIXTURES);
}

inline void copyTree(const Path& from, const Path& to)
{
    std::error_code ec;
    fs::create_directories(to);
    fs::copy(from, to, fs::copy_options::recursive | fs::copy_options::overwrite_existing, ec);
    ASSERT_FALSE(ec) << from.string() << ": " << ec.message();
}

// R is the fixtures' install `installed` ("v1" or "v2") with the records of that release; the
// stage is the fixtures' v2 files with the manifest and signature of release folder
// `stageRelease` ("v2", or "bad/downgrade-version", ...). With the defaults - v1 installed, v2
// staged - `expected` is the tree the update must leave: v2's files, the seed as v1 had it,
// and v2's records.
inline void buildFixtureScenario(Scenario& s, const std::string& installed = "v1",
                                 const std::string& stageRelease = "v2")
{
    const Path fixtures = fixtureFolder();
    copyTree(fixtures / "install" / installed, s.root);
    const bool v1 = installed == "v1";
    writeText(s.root / "share/katana/installed.json",
              std::string("{\"product\":\"katana\",\"schema\":1,\"sequence\":") +
                  (v1 ? "1790000000,\"tag\":\"v0.9.0\"" : "1792312800,\"tag\":\"v0.10.0\"") +
                  ",\"target\":\"win64\",\"version\":\"" + (v1 ? "0.9.0" : "0.10.0") + "\"}\n");
    const Path installedRelease = fixtures / "release" / installed;
    writeText(s.root / "share/katana/installed.manifest.json",
              readText(installedRelease / "Katana-update-win64.json"));
    writeText(s.root / "share/katana/installed.manifest.json.sig",
              readText(installedRelease / "Katana-update-win64.json.sig"));

    copyTree(fixtures / "install" / "v2", s.stage);
    fs::remove(s.stage / "share/katana/install.json"); // the installer's mark is never in a stage
    const Path staged = fixtures / "release" / stageRelease;
    writeText(s.stage / "manifest.json", readText(staged / "Katana-update-win64.json"));
    writeText(s.stage / "manifest.json.sig", readText(staged / "Katana-update-win64.json.sig"));

    if (v1 && stageRelease == "v2") {
        copyTree(fixtures / "install" / "v2", s.expected);
        writeText(s.expected / "etc/katana/settings.ini",
                  readText(fixtures / "install/v1/etc/katana/settings.ini"));
        writeText(s.expected / "share/katana/installed.json",
                  "{\"product\":\"katana\",\"schema\":1,\"sequence\":1792312800,\"tag\":\"v0.10.0\","
                  "\"target\":\"win64\",\"version\":\"0.10.0\"}\n");
        writeText(s.expected / "share/katana/installed.manifest.json",
                  readText(fixtures / "release/v2/Katana-update-win64.json"));
        writeText(s.expected / "share/katana/installed.manifest.json.sig",
                  readText(fixtures / "release/v2/Katana-update-win64.json.sig"));
        s.v2Hash = treeHash(s.expected);
    }
    s.v1Hash = treeHash(s.root);
    s.v1Snapshot = snapshot(s.root);
}

// ---- running the swap ----------------------------------------------------------------------------------

inline ApplyTimings fastTimings()
{
    ApplyTimings timings;
    timings.waitForParent = std::chrono::milliseconds(60);
    timings.waitForHolders = std::chrono::milliseconds(60);
    timings.pollInterval = std::chrono::milliseconds(1);
    timings.healthWait = std::chrono::milliseconds(2000);
    return timings;
}

// A new version that starts: --update-health writes the marker and exits 0.
inline std::function<core::Result<SpawnResult>(const SpawnRequest&)>
healthyStart(const Path& stateDir, const std::string& transaction)
{
    return [stateDir, transaction](const SpawnRequest&) -> core::Result<SpawnResult> {
        writeText(healthMarkerPath(stateDir, transaction), "ok");
        return SpawnResult{};
    };
}

// The doubles and the request of one run of apply().
struct Rig {
    Rig(const Scenario& scenario, FsOps& fileSystem, std::string transaction = "a1b2c3d4")
        : fs(&fileSystem), tx(std::move(transaction))
    {
        request.root = scenario.root;
        request.stageDirectory = scenario.stage;
        request.stateDirectory = scenario.state;
        request.helper = scenario.state / ("run-" + tx) / "katana_updater.exe";
        request.transaction = tx;
        request.waitPid = 0;
        request.relaunch = scenario.root / "bin" / "katana.exe";
        launcher.onRun = healthyStart(scenario.state, tx);
    }

    ApplyServices services() { return ApplyServices{*fs, processes, launcher, registry, testTrust(), timings}; }
    ApplyResult run(std::stop_token stop = {}) { return apply(request, services(), stop); }

    FsOps* fs;
    std::string tx;
    ApplyRequest request;
    FakeProcesses processes;
    FakeLauncher launcher;
    FakeRegistry registry;
    ApplyTimings timings = fastTimings();
};


// ---- crashing and recovering ---------------------------------------------------------------------------

inline std::string stateName(JournalState state)
{
    switch (state) {
    case JournalState::Empty:
        return "Empty";
    case JournalState::Planned:
        return "Planned";
    case JournalState::Committing:
        return "Committing";
    case JournalState::CommittedPendingHealth:
        return "CommittedPendingHealth";
    case JournalState::RollingBack:
        return "RollingBack";
    case JournalState::RolledBack:
        return "RolledBack";
    case JournalState::Healthy:
        return "Healthy";
    }
    return "?";
}

// A scenario kept pristine on the side, put back in front of every iteration.
class Loop {
  public:
    Loop(const std::string& label, const std::function<void(Scenario&)>& build) : scenario_(label)
    {
        build(scenario_);
        pristine_ = scenario_.scratch.path() / "pristine";
        copyTree(scenario_.root, pristine_ / "root");
        copyTree(scenario_.stage, pristine_ / "stage");
    }

    void reset()
    {
        replaceTree(pristine_ / "root", scenario_.root);
        replaceTree(pristine_ / "stage", scenario_.stage);
        std::error_code ec;
        fs::remove_all(scenario_.state, ec);
        fs::create_directories(scenario_.state);
    }

    Scenario& scenario() { return scenario_; }
    [[nodiscard]] const Path& pristineRoot() const { return pristine_; }

  private:
    Scenario scenario_;
    Path pristine_;
};

// The apply, on `fs`, until it finishes or "dies". Returns the crash, if there was one.
inline std::optional<SimulatedCrash> applyUntilCrash(Scenario& s, FsOps& fs, ApplyResult* result = nullptr)
{
    Rig rig(s, fs);
    try {
        const ApplyResult ended = rig.run();
        if (result != nullptr) {
            *result = ended;
        }
    } catch (const SimulatedCrash& crash) {
        return crash;
    }
    return std::nullopt;
}

// recoverInProcess on `fs`, until it finishes or "dies".
inline std::optional<SimulatedCrash> recoverUntilCrash(Scenario& s, FsOps& fs, core::Status* status = nullptr)
{
    Rig rig(s, fs, "eeeeeeee");
    try {
        const core::Status ended = recoverInProcess(s.root, rig.services());
        if (status != nullptr) {
            *status = ended;
        }
    } catch (const SimulatedCrash& crash) {
        return crash;
    }
    return std::nullopt;
}

inline core::Status recoverClean(Scenario& s)
{
    StdFs clean;
    core::Status status;
    const std::optional<SimulatedCrash> crash = recoverUntilCrash(s, clean, &status);
    EXPECT_FALSE(crash.has_value());
    return status;
}


} // namespace katana::update::harness
