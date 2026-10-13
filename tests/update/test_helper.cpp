// The helper program (src/katana_updater): its process scan, its registry, its command line, its
// self-test, and the whole swap driven through the real katana_updater_test binary.
//
// WHERE THE EXPECTATIONS COME FROM (CLAUDE.md, section 4). Nothing here is copied from what the
// program printed:
//   - the process scan is held to processes this test starts itself, whose image path it knows,
//     and to the operating system's own answers (GetCurrentProcessId, the path it started, and
//     the file system's identity for a file where two spellings of its path must be told to be
//     one file);
//   - the registry is read back with RegGetValueW and written for the reader with RegSetKeyValueW,
//     not through the code under test, and always under a key of the test's own;
//   - the import table is read from the PE file by the code below, not by the helper;
//   - the tree an update must leave is built from the specification of the release, in another
//     folder (apply_harness.hpp), or is the T3 fixtures' (the Python producer, signed by openssl);
//   - exit codes are the table at the top of src/katana_updater/main.cpp, which is a design
//     document and not a log;
//   - the self-test's signature is RFC 8032 TEST 3, and the hostile configuration is shown to be
//     hostile by a program that does not harden (the openssl command, when it is installed).
//
// WHAT A TEST HERE NEVER DOES:
//   - start a file that is not a program. Windows answers a text file named .exe with a message
//     box that somebody has to click. Every program started is a real one: the helper, and
//     updater_stub.cpp standing in for Katana (in the T3 fixtures, whose katana.exe is text, by
//     the test helper's --stand-in);
//   - start a program other than through update::Launcher, which turns that box off for the
//     start: the stubs left running are started by it too, and say themselves which process
//     they are (Sleeper, below);
//   - write the machine's own RunOnce, Uninstall or installer keys. Every registry write goes
//     under HKCU\Software\KatanaUpdateTest-<unique>, which is deleted with the test; the helper
//     built for the tests has no way to the machine's keys at all (main.cpp).
//
// Windows only: the process scan and the registry are, and the POSIX seams are T18's. A test that
// would pass whatever the program did is a defect, so each end-to-end case also asserts the
// disk: the whole-tree hash of the install against the tree built from the specification.

#include <gtest/gtest.h>

#ifdef _WIN32

#include <algorithm>
#include <array>
#include <atomic>
#include <cctype>
#include <chrono>
#include <fstream>
#include <functional>
#include <iostream>
#include <map>
#include <mutex>
#include <optional>
#include <set>
#include <thread>

#include "apply_harness.hpp"

#define WIN32_LEAN_AND_MEAN
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>

#include <winreg.h>

#ifndef KATANA_UPDATER_EXE
#error "tests/update/CMakeLists.txt defines KATANA_UPDATER_EXE, KATANA_UPDATER_TEST_EXE and KATANA_UPDATER_STUB_EXE"
#endif

namespace katana::update {
// Defined by src/katana_updater/registry_win.cpp: the registry under HKCU\<base>.
std::unique_ptr<RegistryOps> makeRegistryOpsUnder(std::string_view base);
} // namespace katana::update

using namespace katana;
using namespace katana::update;
using namespace katana::update::harness;

namespace {

using namespace std::chrono_literals;

// ---- small helpers --------------------------------------------------------------------------------

std::wstring toWide(std::string_view utf8)
{
    if (utf8.empty()) {
        return {};
    }
    const int length = MultiByteToWideChar(CP_UTF8, 0, utf8.data(), static_cast<int>(utf8.size()), nullptr, 0);
    std::wstring wide(static_cast<std::size_t>(length), L'\0');
    MultiByteToWideChar(CP_UTF8, 0, utf8.data(), static_cast<int>(utf8.size()), wide.data(), length);
    return wide;
}

std::string toUtf8(const std::wstring& wide)
{
    return utf8FromWide(wide);
}

bool equalIgnoringCase(const std::wstring& a, const std::wstring& b)
{
    return a.size() == b.size() &&
           CompareStringOrdinal(a.data(), static_cast<int>(a.size()), b.data(), static_cast<int>(b.size()),
                                TRUE) == CSTR_EQUAL;
}

bool contains(const std::string& text, std::string_view part)
{
    return text.find(part) != std::string::npos;
}

// The value of the first `key=` line of a program's output, or "" when there is none.
std::string valueOf(const std::string& output, std::string_view key)
{
    std::size_t line = 0;
    while (line < output.size()) {
        std::size_t end = output.find('\n', line);
        if (end == std::string::npos) {
            end = output.size();
        }
        std::string_view text(output.data() + line, end - line);
        if (!text.empty() && text.back() == '\r') {
            text.remove_suffix(1);
        }
        if (text.size() > key.size() && text.substr(0, key.size()) == key && text[key.size()] == '=') {
            return std::string(text.substr(key.size() + 1));
        }
        line = end + 1;
    }
    return {};
}

std::string describe(const SpawnResult& ran)
{
    return "exit " + std::to_string(ran.exitCode) + (ran.timedOut ? " (timed out)" : "") + ", output:\n" +
           ran.output;
}

std::vector<std::string> linesOf(const std::string& text)
{
    std::vector<std::string> lines;
    std::size_t begin = 0;
    while (begin < text.size()) {
        std::size_t end = text.find('\n', begin);
        if (end == std::string::npos) {
            end = text.size();
        }
        std::string line = text.substr(begin, end - begin);
        if (!line.empty() && line.back() == '\r') {
            line.pop_back();
        }
        if (!line.empty()) {
            lines.push_back(std::move(line));
        }
        begin = end + 1;
    }
    return lines;
}

// A variable of the test process's environment for the length of a scope: the programs it starts
// inherit it. Restored afterwards, so the cases cannot affect one another (ctest also runs each in
// its own process).
class ScopedEnvironment {
  public:
    ScopedEnvironment() = default;
    ScopedEnvironment(const ScopedEnvironment&) = delete;
    ScopedEnvironment& operator=(const ScopedEnvironment&) = delete;
    ~ScopedEnvironment()
    {
        for (auto saved = saved_.rbegin(); saved != saved_.rend(); ++saved) {
            SetEnvironmentVariableW(saved->name.c_str(), saved->value ? saved->value->c_str() : nullptr);
        }
    }

    void set(const wchar_t* name, const std::wstring& value)
    {
        remember(name);
        SetEnvironmentVariableW(name, value.c_str());
    }
    void set(const wchar_t* name, const Path& value) { set(name, value.native()); }
    void unset(const wchar_t* name)
    {
        remember(name);
        SetEnvironmentVariableW(name, nullptr);
    }

  private:
    struct Saved {
        std::wstring name;
        std::optional<std::wstring> value;
    };
    void remember(const wchar_t* name)
    {
        std::wstring value(32768, L'\0');
        const DWORD size = GetEnvironmentVariableW(name, value.data(), static_cast<DWORD>(value.size()));
        Saved saved{name, std::nullopt};
        if (size > 0 && size < value.size()) {
            value.resize(size);
            saved.value = value;
        }
        saved_.push_back(std::move(saved));
    }
    std::vector<Saved> saved_;
};

// Runs a program to its end with the system Launcher: its exit code and what it printed.
SpawnResult runProgram(const Path& executable, std::vector<std::string> arguments,
                       const Path& workingDirectory = {}, std::chrono::milliseconds timeout = 120s)
{
    const std::unique_ptr<Launcher> launcher = makeSystemLauncher();
    SpawnRequest request;
    request.executable = executable;
    request.arguments = std::move(arguments);
    request.workingDirectory = workingDirectory;
    request.timeout = timeout;
    const core::Result<SpawnResult> ran = launcher->run(request);
    EXPECT_TRUE(ran.ok()) << (ran.ok() ? "" : ran.error().describe());
    if (!ran.ok()) {
        SpawnResult failed;
        failed.exitCode = -1;
        return failed;
    }
    return ran.value();
}

// A program left running: the stub, asked to sleep. Ended with the object. It is started by the
// Launcher like every other program here, so that the Launcher's precaution covers this start
// too: should the copy of the stub not be a program after all (a copy cut short), that is an
// error and never a message box of the system's. The Launcher hands back no process id, so the
// stub writes its own into a file this names for it (KATANA_STUB_PID_FILE, updater_stub.cpp),
// and the process is then opened by that id - to end it with, and so that the id stays this
// process's for as long as a case asks about it. pid() is 0 when any of that failed.
class Sleeper {
  public:
    Sleeper(const Path& executable, unsigned milliseconds) : executable_(executable)
    {
        static Scratch notes("sleepers");
        static std::atomic<unsigned> counter{0};
        const Path note = notes.path() / (std::to_string(counter++) + ".pid");
        {
            // Only the program started now is told where to write: the variable is gone again
            // before anything else is started.
            ScopedEnvironment environment;
            environment.set(L"KATANA_STUB_PID_FILE", note);
            SpawnRequest request;
            request.executable = executable;
            request.arguments = {"--sleep", std::to_string(milliseconds)};
            request.workingDirectory = executable.parent_path();
            const core::Status started = makeSystemLauncher()->startDetached(request);
            EXPECT_TRUE(started.ok()) << "could not start " << toUtf8(executable.native()) << ": "
                                      << (started.ok() ? std::string() : started.error().describe());
            if (!started.ok()) {
                return;
            }
        }
        // The stub writes the digits and a line feed in one write, first thing.
        std::string text;
        const auto end = std::chrono::steady_clock::now() + std::chrono::seconds(30);
        while (std::chrono::steady_clock::now() < end) {
            text = readText(note);
            if (!text.empty() && text.back() == '\n') {
                break;
            }
            std::this_thread::sleep_for(std::chrono::milliseconds(10));
        }
        DWORD pid = 0;
        for (const char c : text) {
            if (c >= '0' && c <= '9') {
                pid = pid * 10 + static_cast<DWORD>(c - '0');
            }
        }
        EXPECT_NE(pid, 0u) << toUtf8(executable.native()) << " was started and did not say which process it is";
        if (pid == 0) {
            return;
        }
        process_ = OpenProcess(SYNCHRONIZE | PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid);
        EXPECT_TRUE(process_ != nullptr) << "process " << pid << " could not be opened: " << GetLastError();
        if (process_ != nullptr) {
            pid_ = pid;
        }
    }
    Sleeper(const Sleeper&) = delete;
    Sleeper& operator=(const Sleeper&) = delete;
    ~Sleeper()
    {
        terminate();
        if (process_ != nullptr) {
            CloseHandle(process_);
        }
    }

    [[nodiscard]] DWORD pid() const { return pid_; }
    [[nodiscard]] const Path& executable() const { return executable_; }
    void terminate()
    {
        if (process_ == nullptr) {
            return;
        }
        if (WaitForSingleObject(process_, 0) == WAIT_TIMEOUT) {
            TerminateProcess(process_, 1);
        }
        WaitForSingleObject(process_, 10'000);
    }

  private:
    HANDLE process_ = nullptr;
    DWORD pid_ = 0;
    Path executable_;
};

void copyProgram(const Path& from, const Path& to)
{
    fs::create_directories(to.parent_path());
    std::error_code ec;
    fs::copy_file(from, to, fs::copy_options::overwrite_existing, ec);
    ASSERT_FALSE(ec) << toUtf8(from.native()) << " -> " << toUtf8(to.native()) << ": " << ec.message();
}

// The helper program, copied ONCE per test process into the temp folder, and each world's copy a
// hard link to that (a plain copy where the volume cannot link). The build tree and the temp
// folder are often on different volumes, which rules out linking to the build's own file. A
// world's helper is still a program run from a folder of its own, which is what the helper copy
// is; what the cases that make dozens of worlds save is a 10 MB copy each, scanned anew by the
// virus checker every time, which was most of their time.
void linkProgram(const Path& from, const Path& to)
{
    static Scratch shared("helper-copy");
    static std::map<std::string, Path> copies;
    static std::mutex mutex;
    Path source;
    {
        const std::lock_guard<std::mutex> lock(mutex);
        Path& copy = copies[toUtf8(from.native())];
        if (copy.empty()) {
            copy = shared.path() / from.filename();
            copyProgram(from, copy);
        }
        source = copy;
    }
    fs::create_directories(to.parent_path());
    if (!CreateHardLinkW(to.c_str(), source.c_str(), nullptr)) {
        copyProgram(source, to);
    }
}

// A folder named with a space and letters outside every ANSI code page at once: the helper must
// be able to work in a profile like that (it reads its command line as UTF-16). Short, like the
// other names below: the RunOnce command names the helper copy AND the install, both under the
// temp folder here, and a RunOnce command is at most 260 characters (registry.hpp).
const Path kUnicodeFolder = Path(L"K\u00e4 \u65e5");

// CMake gives these with forward slashes; the programs report paths with the native separator,
// and the cases compare them as text.
Path preferred(const char* path)
{
    Path result(path);
    return result.make_preferred();
}

const Path kStub = preferred(KATANA_UPDATER_STUB_EXE);
const Path kHelper = preferred(KATANA_UPDATER_EXE);
const Path kTestHelper = preferred(KATANA_UPDATER_TEST_EXE);

// The long spelling of an existing folder: Windows may hand out 8.3 names for the temp folder
// (C:\Users\RUNNER~1\... on a build service), and the paths the programs report back are
// compared with the test's as text. GetLongPathNameW and not std::filesystem::canonical, which
// with this library on Windows leaves a short name as it is.
Path longSpelling(const Path& folder)
{
    std::wstring buffer(folder.native().size() + 64, L'\0');
    for (int attempt = 0; attempt < 3; ++attempt) {
        const DWORD length = GetLongPathNameW(folder.c_str(), buffer.data(), static_cast<DWORD>(buffer.size()));
        if (length == 0) {
            return folder;
        }
        if (length < buffer.size()) {
            buffer.resize(length);
            return Path(buffer);
        }
        buffer.assign(length + 1, L'\0');
    }
    return folder;
}

// Whether two paths name ONE file, by the file system's own identity for it - the volume and the
// file's number on it - and not by how the paths are spelled. A short name against a long one, a
// junction against the folder it points to, either case: the spelling a program reports of its
// own image is the kernel's choice, and the test does not guess it.
bool sameFile(const Path& first, const Path& second)
{
    const auto identity = [](const Path& path) -> std::optional<std::array<DWORD, 3>> {
        // No access asked for, only the attributes: a program that is running can be opened so.
        const HANDLE file = CreateFileW(path.c_str(), 0, FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                                        nullptr, OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS, nullptr);
        if (file == INVALID_HANDLE_VALUE) {
            return std::nullopt;
        }
        BY_HANDLE_FILE_INFORMATION information{};
        const bool known = GetFileInformationByHandle(file, &information) != 0;
        CloseHandle(file);
        if (!known) {
            return std::nullopt;
        }
        return std::array<DWORD, 3>{information.dwVolumeSerialNumber, information.nFileIndexHigh,
                                    information.nFileIndexLow};
    };
    const std::optional<std::array<DWORD, 3>> one = identity(first);
    const std::optional<std::array<DWORD, 3>> other = identity(second);
    return one.has_value() && other.has_value() && *one == *other;
}

Path systemFolder()
{
    std::wstring folder(MAX_PATH, L'\0');
    const UINT length = GetSystemDirectoryW(folder.data(), static_cast<UINT>(folder.size()));
    folder.resize(length < folder.size() ? length : 0);
    return Path(folder);
}

// Makes `link` a junction to the folder `target` (cmd's mklink /J, which needs no privilege).
// False, with what cmd said, where the machine's policy refuses it.
bool makeJunction(const Path& link, const Path& target, std::string& said)
{
    const SpawnResult made = runProgram(systemFolder() / "cmd.exe",
                                        {"/c", "mklink", "/J", toUtf8(link.native()), toUtf8(target.native())});
    said = made.output;
    return made.exitCode == 0;
}

// ---- a registry of the test's own -----------------------------------------------------------------------

bool keyExists(const std::wstring& subkey)
{
    HKEY opened = nullptr;
    const LONG status = RegOpenKeyExW(HKEY_CURRENT_USER, subkey.c_str(), 0, KEY_READ, &opened);
    if (status == ERROR_SUCCESS) {
        RegCloseKey(opened);
    }
    return status == ERROR_SUCCESS;
}

// HKCU\Software\KatanaUpdateTest-<unique>, deleted with the object. Everything the code under
// test writes goes below it.
class TestHive {
  public:
    TestHive()
    {
        static std::atomic<unsigned> counter{0};
        const std::string seed = std::to_string(GetCurrentProcessId()) + ":" +
                                 std::to_string(std::chrono::steady_clock::now().time_since_epoch().count()) +
                                 ":" + std::to_string(counter++);
        base_ = "Software\\KatanaUpdateTest-" + core::sha256Hex(seed).substr(0, 12);
    }
    TestHive(const TestHive&) = delete;
    TestHive& operator=(const TestHive&) = delete;
    ~TestHive()
    {
        // The tree below the key, then the key: nothing of a test stays in the registry.
        const std::wstring key = toWide(base_);
        RegDeleteTreeW(HKEY_CURRENT_USER, key.c_str());
        RegDeleteKeyW(HKEY_CURRENT_USER, key.c_str());
    }

    [[nodiscard]] const std::string& base() const { return base_; }
    [[nodiscard]] std::wstring key(std::wstring_view relative) const
    {
        return toWide(base_) + L"\\" + std::wstring(relative);
    }
    [[nodiscard]] std::wstring runOnceKey() const
    {
        return key(L"Software\\Microsoft\\Windows\\CurrentVersion\\RunOnce");
    }
    [[nodiscard]] std::wstring uninstallKey() const
    {
        return key(L"Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Katana");
    }
    [[nodiscard]] std::wstring locationKey() const { return key(L"Software\\Jarada\\Katana"); }
    [[nodiscard]] bool exists() const { return keyExists(toWide(base_)); }

  private:
    std::string base_;
};

// A string value read back directly; `type` receives its registry type.
std::optional<std::wstring> readString(const std::wstring& subkey, const wchar_t* name, DWORD* type = nullptr)
{
    std::wstring value(4096, L'\0');
    DWORD bytes = static_cast<DWORD>(value.size() * sizeof(wchar_t));
    DWORD kind = 0;
    const LONG status = RegGetValueW(HKEY_CURRENT_USER, subkey.c_str(), name,
                                     RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ | RRF_NOEXPAND, &kind, value.data(), &bytes);
    if (status != ERROR_SUCCESS) {
        return std::nullopt;
    }
    if (type != nullptr) {
        *type = kind;
    }
    value.resize(wcsnlen(value.c_str(), bytes / sizeof(wchar_t)));
    return value;
}

void writeValue(const std::wstring& subkey, const wchar_t* name, DWORD type, const std::wstring& text)
{
    const LONG status = RegSetKeyValueW(HKEY_CURRENT_USER, subkey.c_str(), name, type, text.c_str(),
                                        static_cast<DWORD>((text.size() + 1) * sizeof(wchar_t)));
    ASSERT_EQ(status, ERROR_SUCCESS) << "RegSetKeyValueW " << toUtf8(subkey);
}

// The value name the helper registers (registry.hpp), and the machine's own key for it. The
// machine's key is only ever READ here - but for one line, which takes a value of this file's
// own out of it again should a redirection of HKEY_CURRENT_USER ever fail to hold
// (TheShippedRegistry..., below), and is not reached otherwise.
const std::wstring kRunOnceValue = L"KatanaUpdateRecover";
const std::wstring kMachineRunOnceKey = L"Software\\Microsoft\\Windows\\CurrentVersion\\RunOnce";

// HKEY_CURRENT_USER sent, for this process, to a key of the test's own (RegOverridePredefKey:
// what an installer uses to see what a component would write), and back when the object goes. So
// the production registry - whose key paths are relative to HKEY_CURRENT_USER - can be run as it
// is and still write nothing of the machine's.
class RedirectedCurrentUser {
  public:
    explicit RedirectedCurrentUser(const std::wstring& subkey)
    {
        if (RegCreateKeyExW(HKEY_CURRENT_USER, subkey.c_str(), 0, nullptr, 0, KEY_ALL_ACCESS, nullptr, &target_,
                            nullptr) != ERROR_SUCCESS) {
            target_ = nullptr;
            return;
        }
        active_ = RegOverridePredefKey(HKEY_CURRENT_USER, target_) == ERROR_SUCCESS;
    }
    RedirectedCurrentUser(const RedirectedCurrentUser&) = delete;
    RedirectedCurrentUser& operator=(const RedirectedCurrentUser&) = delete;
    ~RedirectedCurrentUser()
    {
        if (active_) {
            RegOverridePredefKey(HKEY_CURRENT_USER, nullptr);
        }
        if (target_ != nullptr) {
            RegCloseKey(target_);
        }
    }
    [[nodiscard]] bool active() const { return active_; }

  private:
    HKEY target_ = nullptr;
    bool active_ = false;
};

// ---- the stub, and an install built around it ------------------------------------------------------------------

const std::string& stubBytes()
{
    static const std::string bytes = readText(kStub);
    return bytes;
}

// The stub with a tag appended to the file: the program still runs (the PE loader ignores bytes
// after the last section), and reads its tag from its own file (updater_stub.cpp).
std::string taggedStub(std::string_view tag)
{
    return stubBytes() + "\nKATANA-STUB-TAG=" + std::string(tag) + "\n";
}

constexpr std::string_view kTransaction = "a1b2c3d4e5f60718";

// The test helper's --timings: wait for the window, wait for the programs under R, poll interval,
// health wait, rename retry - milliseconds.
constexpr std::string_view kTimings = "500,1500,50,20000,30000";

// A release of the stub's: v1 and v2 differ in the programs (the tag), one library, one data file,
// and each has a file the other lacks. The seed differs too, and an update must leave v1's.
ReleaseSpec stubRelease(std::string_view tag, std::string_view version, std::uint64_t sequence)
{
    const bool second = tag == "v2";
    ReleaseSpec release;
    release.version = std::string(version);
    release.tag = "v" + std::string(version);
    release.sequence = sequence;
    const auto add = [&release](std::string path, std::string content, FileClass fileClass, bool executable) {
        release.files.push_back({std::move(path), std::move(content), fileClass, executable});
    };
    add("bin/katana.exe", taggedStub(tag), FileClass::Program, true);
    add("bin/katana_cli.exe", taggedStub(tag), FileClass::Program, true);
    add("bin/libcore-1.dll", std::string("core library ") + std::string(tag) + "\n", FileClass::Lib, false);
    add(second ? "bin/libnew-2.dll" : "bin/libold-1.dll", std::string("only in ") + std::string(tag) + "\n",
        FileClass::Lib, false);
    add("share/katana/readme.txt", std::string("readme ") + std::string(tag) + "\n", FileClass::Data, false);
    add(second ? "share/katana/help/index.txt" : "share/katana/legacy/old.txt",
        std::string("only in ") + std::string(tag) + "\n", FileClass::Data, false);
    add("etc/katana/settings.ini", std::string("seed ") + std::string(tag) + "\n", FileClass::Seed, false);
    return release;
}

std::string logOf(const Path& log)
{
    return isPresent(log) ? readText(log) : std::string();
}

// The names directly inside a folder, with "/" after a folder's.
std::set<std::string> namesIn(const Path& folder)
{
    std::set<std::string> names;
    std::error_code ec;
    for (fs::directory_iterator it(folder, ec), end; !ec && it != end; it.increment(ec)) {
        names.insert(toUtf8(it->path().filename().native()) + (it->is_directory(ec) ? "/" : ""));
    }
    return names;
}

// The RunOnce command the helper registers for a helper copy and an install (apply.hpp, step 4;
// the quoting is CommandLineToArgvW's, src/katana_update/command_line.hpp): both paths hold a
// space here, so both are quoted.
std::string runOnceCommand(const Path& helperCopy, const Path& root)
{
    return "\"" + toUtf8(helperCopy.native()) + "\" --recover \"" + toUtf8(root.native()) + "\"";
}

// Everything one end-to-end case needs: a v1 install, a v2 stage, the tree v2 must leave, the
// state folder, the helper copy in it (as preflight makes it), a registry of its own, and the
// environment the stub reads.
class World {
  public:
    explicit World(const std::string& label) : scratch_(label)
    {
        top_ = longSpelling(scratch_.path()) / kUnicodeFolder;
        root = top_ / "Katana 1";
        stage = top_ / "stage";
        state = top_ / "state";
        expected = top_ / "expected";
        log = top_ / "stub.log";
        fs::create_directories(state);

        v1 = stubRelease("v1", "0.9.0", 1000);
        v2 = stubRelease("v2", "0.10.0", 2000);
        writeInstall(root, v1);
        writeStage(stage, v2);
        // The tree a good update leaves, from the specification: v2's files, the seed as v1 had
        // it, the files only v1 had gone, and v2's records.
        for (const FileSpec& file : v2.files) {
            if (file.fileClass != FileClass::Seed) {
                writeText(expected / file.path, file.content);
            }
        }
        writeText(expected / "etc/katana/settings.ini", v1.find("etc/katana/settings.ini")->content);
        writeText(expected / "share/katana/install.json", kInstallJson);
        writeText(expected / "share/katana/installed.json", installedJson(v2));
        writeText(expected / "share/katana/installed.manifest.json", manifestJson(v2));
        writeText(expected / "share/katana/installed.manifest.json.sig", signLine(manifestJson(v2)));
        v1Hash = treeHash(root);
        v1Snapshot = snapshot(root);
        v2Hash = treeHash(expected);
        installHelper(std::string(kTransaction));
    }
    World(const World&) = delete;
    World& operator=(const World&) = delete;
    ~World()
    {
        // The program the helper starts last is detached and may still be on its way out: its
        // file cannot be deleted while it runs, and the folder goes with this object.
        const std::unique_ptr<ProcessOps> processes = makeSystemProcessOps();
        const auto end = std::chrono::steady_clock::now() + 5s;
        while (std::chrono::steady_clock::now() < end) {
            const core::Result<std::vector<RunningProgram>> running = processes->holdersUnder(root);
            if (!running.ok() || running->empty()) {
                break;
            }
            std::this_thread::sleep_for(20ms);
        }
    }

    // The helper as preflight leaves it: a copy in U/run-<tx>/, named as recover() looks for it.
    void installHelper(const std::string& transaction)
    {
        linkProgram(kTestHelper, helperPath(transaction));
    }
    [[nodiscard]] Path helperPath(const std::string& transaction) const
    {
        return runPath(state, transaction) / "katana_updater.exe";
    }

    // The command line of the swap, as the engine writes it (helperArguments).
    [[nodiscard]] std::vector<std::string> engineArguments(const std::string& transaction = std::string(kTransaction),
                                                           std::uint64_t waitPid = 0) const
    {
        ApplyRequest request;
        request.root = root;
        request.stageDirectory = stage;
        request.stateDirectory = state;
        request.transaction = transaction;
        request.waitPid = waitPid;
        request.relaunch = root / "bin" / "katana.exe";
        return helperArguments(request);
    }

    // ... plus the test options: the registry of this world, and waits a test can afford.
    [[nodiscard]] std::vector<std::string> applyArguments(const std::string& transaction = std::string(kTransaction),
                                                          std::uint64_t waitPid = 0,
                                                          std::string_view timings = kTimings) const
    {
        std::vector<std::string> arguments = engineArguments(transaction, waitPid);
        arguments.push_back("--registry-base");
        arguments.push_back(hive.base());
        arguments.push_back("--timings");
        arguments.push_back(std::string(timings));
        return arguments;
    }

    [[nodiscard]] std::vector<std::string> withTestOptions(std::vector<std::string> arguments) const
    {
        arguments.push_back("--registry-base");
        arguments.push_back(hive.base());
        return arguments;
    }

    // The environment of the programs started now: the stub's switches, the state folder it writes
    // the health marker into (and recover() looks for the helper copy in), the registry the stub
    // looks in, and the registry of a test helper whose command line carries no test option.
    // Held until the world ends.
    void arrangeEnvironment(std::string_view stubMode = "healthy")
    {
        environment.set(L"KATANA_STUB_LOG", log);
        environment.set(L"KATANA_STUB_MODE", toWide(stubMode));
        environment.set(L"KATANA_UPDATE_STATE_DIR", state);
        environment.set(L"KATANA_STUB_REGISTRY_BASE", toWide(hive.base()));
        environment.set(L"KATANA_UPDATER_TEST_REGISTRY_BASE", toWide(hive.base()));
    }

    SpawnResult runHelper(std::vector<std::string> arguments, const std::string& transaction = std::string(kTransaction),
                          std::chrono::milliseconds timeout = 120s)
    {
        return runProgram(helperPath(transaction), std::move(arguments), helperPath(transaction).parent_path(),
                          timeout);
    }

    [[nodiscard]] std::string runOnce() const
    {
        const std::optional<std::wstring> value = readString(hive.runOnceKey(), kRunOnceValue.c_str());
        return value ? toUtf8(*value) : std::string();
    }
    [[nodiscard]] bool runOnceIsSet() const { return readString(hive.runOnceKey(), kRunOnceValue.c_str()).has_value(); }

    // Makes the install an installer's: the key the installer writes names this folder, and the
    // Uninstall entry (which only an installer makes) shows version 0.9.0.
    void registerAsInstaller()
    {
        writeValue(hive.locationKey(), nullptr, REG_SZ, root.native());
        writeValue(hive.uninstallKey(), L"DisplayVersion", REG_SZ, L"0.9.0");
        writeValue(hive.uninstallKey(), L"DisplayName", REG_SZ, L"Katana 0.9.0");
    }
    [[nodiscard]] std::optional<std::wstring> displayVersion() const
    {
        return readString(hive.uninstallKey(), L"DisplayVersion");
    }
    [[nodiscard]] std::optional<std::wstring> displayName() const
    {
        return readString(hive.uninstallKey(), L"DisplayName");
    }

    [[nodiscard]] std::string stubLog() const { return logOf(log); }
    [[nodiscard]] bool logHas(std::string_view part) const { return contains(stubLog(), part); }
    // The stub the helper starts last is detached: wait for its line.
    bool waitForLog(std::string_view part, std::chrono::milliseconds limit = 30s) const
    {
        const auto end = std::chrono::steady_clock::now() + limit;
        while (std::chrono::steady_clock::now() < end) {
            if (logHas(part)) {
                return true;
            }
            std::this_thread::sleep_for(20ms);
        }
        return logHas(part);
    }

    void expectV1()
    {
        EXPECT_EQ(treeHash(root), v1Hash) << "the tree must be v1\n" << difference(v1Snapshot, snapshot(root));
    }
    void expectV2()
    {
        EXPECT_EQ(treeHash(root), v2Hash) << "the tree must be v2\n" << difference(snapshot(expected), snapshot(root));
    }

    [[nodiscard]] LastResult last() const
    {
        const core::Result<std::optional<LastResult>> read = readLastResult(state);
        EXPECT_TRUE(read.ok() && read->has_value()) << "last.json was not written";
        return read.ok() && read->has_value() ? **read : LastResult{};
    }

    // Declared first, destroyed last (the hive is removed first, then the environment put back).
    Scratch scratch_;
    Path top_;
    Path root;
    Path stage;
    Path state;
    Path expected;
    Path log;
    ReleaseSpec v1;
    ReleaseSpec v2;
    std::string v1Hash;
    std::string v2Hash;
    std::map<std::string, std::string> v1Snapshot;
    TestHive hive;
    ScopedEnvironment environment;
};

// A stub left running with its image at `path` (a copy of the stub, put there).
std::unique_ptr<Sleeper> startSleeperAt(const Path& path, unsigned milliseconds = 120'000)
{
    copyProgram(kStub, path);
    return std::make_unique<Sleeper>(path, milliseconds);
}

// ---- the T3 fixtures as an install and a stage ---------------------------------------------------------------

constexpr std::string_view kFixtureTransaction = "c1c2c3c4";
// The fixtures' v2 manifest, tests/update/fixtures/release/v2/Katana-update-win64.json.
constexpr std::uint64_t kFixtureV2Sequence = 1792312800;

// The release pair the Python producer made and openssl signed (apply_harness.hpp,
// buildFixtureScenario), in a folder with a space and letters outside the code page, with the
// test helper copied where preflight puts it and a registry of its own. The fixtures' katana.exe
// is a text file: it is never started (see the top of this file).
class FixtureWorld {
  public:
    explicit FixtureWorld(const std::string& label, const std::string& installed = "v1",
                          const std::string& staged = "v2")
        : s(label)
    {
        top = longSpelling(s.scratch.path()) / kUnicodeFolder;
        s.root = top / "Katana 1";
        s.stage = top / "stage";
        s.state = top / "state";
        s.expected = top / "expected";
        log = top / "stub.log";
        fs::create_directories(s.state);
        buildFixtureScenario(s, installed, staged);
        linkProgram(kTestHelper, helper());
    }

    [[nodiscard]] Path helper() const
    {
        return runPath(s.state, std::string(kFixtureTransaction)) / "katana_updater.exe";
    }

    // The swap's command line with nothing to start at the end, and the test options.
    [[nodiscard]] std::vector<std::string> applyArguments(std::string_view timings = kTimings) const
    {
        ApplyRequest request;
        request.root = s.root;
        request.stageDirectory = s.stage;
        request.stateDirectory = s.state;
        request.transaction = std::string(kFixtureTransaction);
        std::vector<std::string> arguments = helperArguments(request);
        arguments.push_back("--registry-base");
        arguments.push_back(hive.base());
        arguments.push_back("--timings");
        arguments.push_back(std::string(timings));
        return arguments;
    }

    SpawnResult run(std::vector<std::string> arguments)
    {
        return runProgram(helper(), std::move(arguments), helper().parent_path());
    }

    [[nodiscard]] bool runOnceIsSet() const { return readString(hive.runOnceKey(), kRunOnceValue.c_str()).has_value(); }

    Scenario s;
    Path top;
    Path log;
    TestHive hive;
};

// ---- the process scan ---------------------------------------------------------------------------------------

bool listsPid(const core::Result<std::vector<RunningProgram>>& scanned, DWORD pid,
              const Path* expectedImage = nullptr)
{
    if (!scanned.ok()) {
        return false;
    }
    for (const RunningProgram& program : scanned.value()) {
        if (program.pid == pid) {
            // The image the scan names must BE the file that was started, however it is spelled
            // (the temp folder's 8.3 name, a junction): one file by the file system's identity.
            return expectedImage == nullptr || sameFile(program.image, *expectedImage);
        }
    }
    return false;
}

std::wstring upperCased(std::wstring text)
{
    CharUpperBuffW(text.data(), static_cast<DWORD>(text.size()));
    return text;
}
std::wstring lowerCased(std::wstring text)
{
    CharLowerBuffW(text.data(), static_cast<DWORD>(text.size()));
    return text;
}

} // namespace

// =====================================================================================================
// The process scan
// =====================================================================================================

TEST(HelperProcessScan, ProgramsRunningFromUnderTheRootAreFoundWhateverWayTheRootIsSpelled)
{
    Scratch scratch("scan-spellings");
    const Path root = scratch.path() / kUnicodeFolder / "Install Katana";
    const auto sleeper = startSleeperAt(root / "bin" / "katana_cli.exe");
    ASSERT_NE(sleeper->pid(), 0u);
    const std::unique_ptr<ProcessOps> processes = makeSystemProcessOps();

    std::vector<std::pair<std::string, Path>> spellings;
    spellings.emplace_back("as given", root);
    spellings.emplace_back("upper case", Path(upperCased(root.native())));
    spellings.emplace_back("lower case", Path(lowerCased(root.native())));
    std::wstring slashes = root.native();
    std::replace(slashes.begin(), slashes.end(), L'\\', L'/');
    spellings.emplace_back("forward slashes", Path(slashes));
    spellings.emplace_back("a trailing separator", Path(root.native() + L"\\"));
    spellings.emplace_back("dot and dot-dot segments", Path(root.native() + L"\\bin\\..\\share\\.."));
    spellings.emplace_back("the extended-length prefix", Path(L"\\\\?\\" + root.native()));
    // The short name of the folder, where the volume makes 8.3 names (it is the same folder).
    std::wstring shortName(MAX_PATH * 4, L'\0');
    const DWORD shortLength = GetShortPathNameW(root.c_str(), shortName.data(), static_cast<DWORD>(shortName.size()));
    if (shortLength > 0 && shortLength < shortName.size()) {
        shortName.resize(shortLength);
        if (!equalIgnoringCase(shortName, root.native())) {
            spellings.emplace_back("the 8.3 short name", Path(shortName));
        }
    }

    for (const auto& [what, spelling] : spellings) {
        const core::Result<std::vector<RunningProgram>> scanned = processes->holdersUnder(spelling);
        ASSERT_TRUE(scanned.ok()) << what << ": " << scanned.error().describe();
        EXPECT_TRUE(listsPid(scanned, sleeper->pid(), &sleeper->executable()))
            << what << ": the program started from " << toUtf8(sleeper->executable().native())
            << " was not listed under " << toUtf8(spelling.native());
    }
}

TEST(HelperProcessScan, ARootReachedThroughAJunctionFindsTheProgramStartedFromTheRealFolder)
{
    Scratch scratch("scan-junction");
    const Path root = scratch.path() / kUnicodeFolder / "Install Katana";
    const auto sleeper = startSleeperAt(root / "bin" / "katana.exe");
    const Path junction = scratch.path() / "link";
    // Where the machine's policy refuses a junction the case cannot be made - on a developer's
    // machine. A build service makes them, and a skip there would pass for a test that ran.
    std::string said;
    if (!makeJunction(junction, root, said)) {
        SKIP_UNLESS_AVAILABLE_OR_FAIL_IN_CI("a junction could not be made here: " + said);
    }
    const core::Result<std::vector<RunningProgram>> scanned = makeSystemProcessOps()->holdersUnder(junction);
    ASSERT_TRUE(scanned.ok()) << scanned.error().describe();
    EXPECT_TRUE(listsPid(scanned, sleeper->pid(), &sleeper->executable()))
        << "the junction names the same folder";
}

TEST(HelperProcessScan, AFolderThatOnlySharesTheNamePrefixIsNotUnderTheRoot)
{
    Scratch scratch("scan-prefix");
    const Path root = scratch.path() / "Katana";
    const Path sibling = scratch.path() / "Katana2";
    const auto outside = startSleeperAt(sibling / "bin" / "katana.exe");
    const auto inside = startSleeperAt(root / "bin" / "katana.exe");
    const std::unique_ptr<ProcessOps> processes = makeSystemProcessOps();

    const auto scanned = processes->holdersUnder(root);
    ASSERT_TRUE(scanned.ok()) << scanned.error().describe();
    EXPECT_TRUE(listsPid(scanned, inside->pid()));
    EXPECT_FALSE(listsPid(scanned, outside->pid())) << "C:\\...\\Katana2 is not under C:\\...\\Katana";

    const auto other = processes->holdersUnder(sibling);
    ASSERT_TRUE(other.ok());
    EXPECT_TRUE(listsPid(other, outside->pid()));
    EXPECT_FALSE(listsPid(other, inside->pid()));
}

TEST(HelperProcessScan, AProgramThatHasEndedIsNoLongerListedAndNoLongerAlive)
{
    Scratch scratch("scan-ended");
    const Path root = scratch.path() / "Install";
    auto sleeper = startSleeperAt(root / "bin" / "katana.exe");
    const std::unique_ptr<ProcessOps> processes = makeSystemProcessOps();
    const DWORD pid = sleeper->pid();

    EXPECT_TRUE(processes->isAlive(pid));
    EXPECT_TRUE(listsPid(processes->holdersUnder(root), pid));

    // Ended, with this test still holding a handle to it: the id and the image name stay
    // readable, and neither question may answer as if it still ran.
    sleeper->terminate();
    EXPECT_FALSE(processes->isAlive(pid)) << "the process object outlives the process while a handle is open";
    EXPECT_FALSE(listsPid(processes->holdersUnder(root), pid));
}

TEST(HelperProcessScan, TheProcessThatAsksIsNeverItsOwnHolderAndTheScanStillSeesItsNeighbours)
{
    // The test program and the stub share a folder: the test program's own image is under it, and
    // so is the neighbour's, so a scan that leaves this process out is excluding it, not missing it.
    wchar_t self[MAX_PATH * 4];
    const DWORD length = GetModuleFileNameW(nullptr, self, static_cast<DWORD>(std::size(self)));
    ASSERT_GT(length, 0u);
    ASSERT_TRUE(equalIgnoringCase(Path(std::wstring(self, length)).parent_path().native(), kStub.parent_path().native()))
        << "the premise: the test program and the stub are built into one folder";
    const auto neighbour = std::make_unique<Sleeper>(kStub, 120'000);
    const std::unique_ptr<ProcessOps> processes = makeSystemProcessOps();

    const auto scanned = processes->holdersUnder(kStub.parent_path());
    ASSERT_TRUE(scanned.ok()) << scanned.error().describe();
    EXPECT_TRUE(listsPid(scanned, neighbour->pid())) << "control: the scan sees a program in this folder";
    EXPECT_FALSE(listsPid(scanned, GetCurrentProcessId())) << "this process is never its own holder";
    EXPECT_EQ(processes->currentPid(), GetCurrentProcessId());
}

TEST(HelperProcessScan, AMissingRootHoldsNothingAndAnEmptyRootIsRefused)
{
    Scratch scratch("scan-missing");
    const std::unique_ptr<ProcessOps> processes = makeSystemProcessOps();
    const auto none = processes->holdersUnder(scratch.path() / "does-not-exist");
    ASSERT_TRUE(none.ok());
    EXPECT_TRUE(none->empty());
    const auto refused = processes->holdersUnder(Path());
    ASSERT_FALSE(refused.ok());
    EXPECT_EQ(refused.error().code, core::ErrorCode::InvalidArgument);
}

TEST(HelperProcessScan, ProcessIdsAreAliveOnlyWhileTheProcessRuns)
{
    const std::unique_ptr<ProcessOps> processes = makeSystemProcessOps();
    EXPECT_TRUE(processes->isAlive(GetCurrentProcessId()));
    EXPECT_FALSE(processes->isAlive(0)) << "0 means no process";
    EXPECT_FALSE(processes->isAlive(0xFFFFFFF0ull)) << "a number no process has";
    EXPECT_FALSE(processes->isAlive(0x1'0000'0000ull)) << "wider than a process id";
}

TEST(HelperProcessScan, TheEnginesRunningProgramsIsTheSameScan)
{
    Scratch scratch("scan-engine");
    const Path root = scratch.path() / "Install";
    const auto sleeper = startSleeperAt(root / "bin" / "katana_mcp.exe");
    const core::Result<std::vector<RunningProgram>> running = runningPrograms(root);
    ASSERT_TRUE(running.ok()) << running.error().describe();
    EXPECT_TRUE(listsPid(running, sleeper->pid(), &sleeper->executable()))
        << "a program a client holds open, katana_mcp, is what `blocked` must name";
}

// =====================================================================================================
// The registry
// =====================================================================================================

TEST(HelperRegistry, TheInstallersFolderIsReadFromTheKeyTheInstallerWrites)
{
    const TestHive hive;
    const std::unique_ptr<RegistryOps> registry = makeRegistryOpsUnder(hive.base());
    ASSERT_TRUE(registry != nullptr);

    auto location = registry->installerLocation();
    ASSERT_TRUE(location.ok()) << location.error().describe();
    EXPECT_FALSE(location->has_value()) << "no key: not an installer's install";

    const std::wstring folder = L"C:\\Users\\K\u00e4se\\AppData\\Local\\Programs\\Katana";
    writeValue(hive.locationKey(), nullptr, REG_SZ, folder);
    location = registry->installerLocation();
    ASSERT_TRUE(location.ok() && location->has_value());
    EXPECT_EQ((*location)->native(), folder);

    // An expandable string, as an installer may write %LOCALAPPDATA%: expanded by the reader.
    writeValue(hive.locationKey(), nullptr, REG_EXPAND_SZ, L"%SystemRoot%\\Katana");
    std::wstring systemRoot(MAX_PATH, L'\0');
    systemRoot.resize(GetEnvironmentVariableW(L"SystemRoot", systemRoot.data(), static_cast<DWORD>(systemRoot.size())));
    ASSERT_FALSE(systemRoot.empty()) << "the premise: Windows sets SystemRoot";
    location = registry->installerLocation();
    ASSERT_TRUE(location.ok()) << location.error().describe();
    ASSERT_TRUE(location->has_value());
    EXPECT_EQ((*location)->native(), systemRoot + L"\\Katana");

    // An expansion LONGER than the text stored, which is the usual case and the installer's own
    // (%LOCALAPPDATA%\Programs\Katana): the folder is read to its end. For an expandable string
    // Windows was measured to report the size of the STORED text (registry_win.cpp), and a reader
    // that believes that size cuts this folder short after 22 of its 43 characters. The variable
    // is this test's own, set for this process, which is where the expansion is done.
    ScopedEnvironment environment;
    const std::wstring profile = L"C:\\Users\\K\u00e4se\\AppData\\Local";
    environment.set(L"KT9", profile);
    writeValue(hive.locationKey(), nullptr, REG_EXPAND_SZ, L"%KT9%\\Programs\\Katana");
    location = registry->installerLocation();
    ASSERT_TRUE(location.ok()) << location.error().describe();
    ASSERT_TRUE(location->has_value());
    EXPECT_EQ((*location)->native(), profile + L"\\Programs\\Katana");

    // And an expansion longer than the reader's first buffer (260 characters) as well.
    const std::wstring deep = L"C:\\" + std::wstring(300, L'd');
    environment.set(L"KT9", deep);
    location = registry->installerLocation();
    ASSERT_TRUE(location.ok()) << location.error().describe();
    ASSERT_TRUE(location->has_value());
    EXPECT_EQ((*location)->native(), deep + L"\\Programs\\Katana");

    // A folder longer than the reader's first buffer (260 characters) is read whole.
    const std::wstring longFolder = L"C:\\" + std::wstring(300, L'k') + L"\\Katana";
    writeValue(hive.locationKey(), nullptr, REG_SZ, longFolder);
    location = registry->installerLocation();
    ASSERT_TRUE(location.ok()) << location.error().describe();
    ASSERT_TRUE(location->has_value());
    EXPECT_EQ((*location)->native(), longFolder);

    writeValue(hive.locationKey(), nullptr, REG_SZ, L"");
    location = registry->installerLocation();
    ASSERT_TRUE(location.ok());
    EXPECT_FALSE(location->has_value()) << "an empty folder is no folder";

    const DWORD number = 7;
    ASSERT_EQ(RegSetKeyValueW(HKEY_CURRENT_USER, hive.locationKey().c_str(), nullptr, REG_DWORD, &number, sizeof(number)),
              ERROR_SUCCESS);
    location = registry->installerLocation();
    ASSERT_TRUE(location.ok());
    EXPECT_FALSE(location->has_value()) << "a value that is not a string names no folder";
}

TEST(HelperRegistry, TheRunOnceEntryIsWrittenReplacedAndRemovedUnderItsOwnNameAlone)
{
    const TestHive hive;
    const std::unique_ptr<RegistryOps> registry = makeRegistryOpsUnder(hive.base());
    ASSERT_TRUE(registry != nullptr);
    EXPECT_EQ(kRunOnceName, "KatanaUpdateRecover") << "tests/update/updater_stub.cpp looks for this name";
    EXPECT_EQ(toWide(kRunOnceName), kRunOnceValue);
    writeValue(hive.runOnceKey(), L"SomebodyElse", REG_SZ, L"\"C:\\Other\\tool.exe\" --once");

    const std::string first = "\"C:\\Users\\x\\run-1\\katana_updater.exe\" --recover \"C:\\Users\\x\\Katana\"";
    ASSERT_TRUE(registry->setRunOnce(kRunOnceName, first).ok());
    DWORD type = 0;
    EXPECT_EQ(readString(hive.runOnceKey(), kRunOnceValue.c_str(), &type), toWide(first));
    EXPECT_EQ(type, static_cast<DWORD>(REG_SZ));

    const std::string second = "\"C:\\Users\\x\\run-2\\katana_updater.exe\" --recover \"C:\\Users\\x\\Katana\"";
    ASSERT_TRUE(registry->setRunOnce(kRunOnceName, second).ok());
    EXPECT_EQ(readString(hive.runOnceKey(), kRunOnceValue.c_str()), toWide(second))
        << "a second transaction replaces the first";

    ASSERT_TRUE(registry->clearRunOnce(kRunOnceName).ok());
    EXPECT_FALSE(readString(hive.runOnceKey(), kRunOnceValue.c_str()).has_value());
    EXPECT_EQ(readString(hive.runOnceKey(), L"SomebodyElse"), std::wstring(L"\"C:\\Other\\tool.exe\" --once"))
        << "another program's entry is not ours to remove";
    EXPECT_TRUE(registry->clearRunOnce(kRunOnceName).ok()) << "removing what is not there succeeds";

    // Not even the key need be there.
    const TestHive empty;
    EXPECT_TRUE(makeRegistryOpsUnder(empty.base())->clearRunOnce(kRunOnceName).ok());
    EXPECT_FALSE(empty.exists()) << "removing creates nothing";
}

TEST(HelperRegistry, ACommandWithLettersOutsideTheCodePageRoundTripsAsUtf16)
{
    const TestHive hive;
    const std::unique_ptr<RegistryOps> registry = makeRegistryOpsUnder(hive.base());
    const std::wstring wide = L"\"C:\\Users\\K\u00e4se \u65e5\u672c\\katana_updater.exe\" --recover \"C:\\Katana\"";
    ASSERT_TRUE(registry->setRunOnce(kRunOnceName, toUtf8(wide)).ok());
    EXPECT_EQ(readString(hive.runOnceKey(), kRunOnceValue.c_str()), wide);
}

TEST(HelperRegistry, ACommandOverTheDocumentedLimitIsRefusedAndWritesNothing)
{
    const TestHive hive;
    const std::unique_ptr<RegistryOps> registry = makeRegistryOpsUnder(hive.base());
    const std::string atTheLimit(260, 'a');
    ASSERT_TRUE(registry->setRunOnce(kRunOnceName, atTheLimit).ok()) << "260 characters is allowed";
    const core::Status over = registry->setRunOnce(kRunOnceName, std::string(261, 'b'));
    ASSERT_FALSE(over.ok());
    EXPECT_EQ(over.error().code, core::ErrorCode::InvalidArgument);
    EXPECT_EQ(readString(hive.runOnceKey(), kRunOnceValue.c_str()), toWide(atTheLimit)) << "the old entry stands";
    EXPECT_FALSE(registry->setRunOnce(kRunOnceName, "").ok());
    EXPECT_FALSE(registry->setRunOnce("", "x").ok());
    EXPECT_FALSE(registry->setRunOnce(kRunOnceName, "bad \xC3\x28 utf8").ok());
}

TEST(HelperRegistry, TheUninstallEntryIsRewrittenOnlyWhereTheInstallerMadeIt)
{
    const TestHive hive;
    const std::unique_ptr<RegistryOps> registry = makeRegistryOpsUnder(hive.base());

    // An archive install has no Uninstall key: nothing is done, and nothing is created.
    ASSERT_TRUE(registry->setInstalledVersion("0.3.0", "Katana 0.3.0").ok());
    EXPECT_FALSE(keyExists(hive.uninstallKey())) << "the key belongs to the installer; the updater never makes it";

    writeValue(hive.uninstallKey(), L"DisplayVersion", REG_SZ, L"0.2.0");
    writeValue(hive.uninstallKey(), L"DisplayName", REG_SZ, L"Katana 0.2.0");
    writeValue(hive.uninstallKey(), L"Publisher", REG_SZ, L"Jarada");
    ASSERT_TRUE(registry->setInstalledVersion("0.3.0", "Katana 0.3.0").ok());
    EXPECT_EQ(readString(hive.uninstallKey(), L"DisplayVersion"), std::wstring(L"0.3.0"));
    EXPECT_EQ(readString(hive.uninstallKey(), L"DisplayName"), std::wstring(L"Katana 0.3.0"));
    EXPECT_EQ(readString(hive.uninstallKey(), L"Publisher"), std::wstring(L"Jarada")) << "other values stay";
}

TEST(HelperRegistry, ABaseThatNamesNoFolderOfKeysIsRefused)
{
    EXPECT_TRUE(makeRegistryOpsUnder("") == nullptr);
    EXPECT_TRUE(makeRegistryOpsUnder("\\Software\\x") == nullptr);
    EXPECT_TRUE(makeRegistryOpsUnder("Software\\x\\") == nullptr);
    EXPECT_TRUE(makeRegistryOpsUnder("bad \xC3\x28 utf8") == nullptr);
    EXPECT_TRUE(makeRegistryOpsUnder("Software\\x") != nullptr);
}

TEST(HelperRegistry, TheShippedRegistryUsesTheKeysWindowsAndTheInstallerUseShownWithCurrentUserRedirected)
{
    // makeSystemRegistryOps() is what the shipped programs call, and its key paths are relative
    // to HKEY_CURRENT_USER. To see where they lead without writing the machine's RunOnce key,
    // HKEY_CURRENT_USER is sent to a key of this test's own while the calls are made, and the
    // result is read where Windows and the installer would look - below that key. Even so the
    // value has a name of the test's own and a command that does nothing, and the machine's
    // real key is checked to have been left alone.
    const TestHive hive;
    const std::string name = "KatanaUpdateTest-" + core::sha256Hex(hive.base()).substr(0, 12);
    const std::wstring wideName = toWide(name);
    const std::string command = "\"C:\\Windows\\System32\\cmd.exe\" /c exit 0";
    const bool wasThere = readString(kMachineRunOnceKey, wideName.c_str()).has_value();
    ASSERT_FALSE(wasThere) << "a value of this test's own name is in the machine's RunOnce key already";

    // What an installer leaves, put where the redirected paths will find it.
    writeValue(hive.locationKey(), nullptr, REG_SZ, L"C:\\Somewhere\\Katana");
    writeValue(hive.uninstallKey(), L"DisplayVersion", REG_SZ, L"0.2.0");
    writeValue(hive.uninstallKey(), L"DisplayName", REG_SZ, L"Katana 0.2.0");

    bool redirected = false;
    core::Status written;
    core::Status versioned;
    core::Result<std::optional<Path>> location = std::optional<Path>{};
    {
        const RedirectedCurrentUser redirect(toWide(hive.base()));
        redirected = redirect.active();
        if (redirected) {
            const std::unique_ptr<RegistryOps> registry = makeSystemRegistryOps();
            written = registry->setRunOnce(name, command);
            versioned = registry->setInstalledVersion("0.3.0", "Katana 0.3.0");
            location = registry->installerLocation();
        }
    }
    // HKEY_CURRENT_USER is itself again: look at where the calls landed.
    const std::optional<std::wstring> landed = readString(hive.runOnceKey(), wideName.c_str());
    const bool reachedTheMachine = readString(kMachineRunOnceKey, wideName.c_str()).has_value();
    if (redirected) {
        const RedirectedCurrentUser redirect(toWide(hive.base()));
        if (redirect.active()) {
            EXPECT_TRUE(makeSystemRegistryOps()->clearRunOnce(name).ok());
        }
    }
    // Should the redirection not have held, nothing of this test stays in the machine's key. It
    // is the one write this file ever makes there, and only to take its own value out again: the
    // key is not even opened for writing when the value did not reach it.
    if (reachedTheMachine) {
        RegDeleteKeyValueW(HKEY_CURRENT_USER, kMachineRunOnceKey.c_str(), wideName.c_str());
    }

    ASSERT_TRUE(redirected) << "HKEY_CURRENT_USER could not be redirected, so the shipped registry was not run";
    EXPECT_FALSE(reachedTheMachine) << "the redirection did not hold: the machine's RunOnce key was written";
    ASSERT_TRUE(written.ok()) << written.error().describe();
    EXPECT_EQ(landed, toWide(command)) << "Software\\Microsoft\\Windows\\CurrentVersion\\RunOnce of the user's hive";
    EXPECT_FALSE(readString(hive.runOnceKey(), wideName.c_str()).has_value()) << "and removed from there again";

    ASSERT_TRUE(versioned.ok()) << versioned.error().describe();
    EXPECT_EQ(readString(hive.uninstallKey(), L"DisplayVersion"), std::wstring(L"0.3.0"))
        << "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Katana, the key the installer makes";
    EXPECT_EQ(readString(hive.uninstallKey(), L"DisplayName"), std::wstring(L"Katana 0.3.0"));

    ASSERT_TRUE(location.ok()) << location.error().describe();
    ASSERT_TRUE(location->has_value()) << "Software\\Jarada\\Katana, the key the installer writes its folder to";
    EXPECT_EQ((*location)->native(), std::wstring(L"C:\\Somewhere\\Katana"));
}

TEST(HelperRegistry, TheKeysATestMakesAreGoneWhenItEnds)
{
    std::wstring base;
    {
        const TestHive hive;
        base = toWide(hive.base());
        const std::unique_ptr<RegistryOps> registry = makeRegistryOpsUnder(hive.base());
        ASSERT_TRUE(registry->setRunOnce(kRunOnceName, "\"C:\\x\\katana_updater.exe\" --recover C:\\Katana").ok());
        writeValue(hive.uninstallKey(), L"DisplayVersion", REG_SZ, L"0.2.0");
        ASSERT_TRUE(keyExists(base)) << "control: the keys were there";
    }
    EXPECT_FALSE(keyExists(base)) << "a test leaves nothing behind in the registry";
}

// =====================================================================================================
// The program: --version, --help, --selftest, usage
// =====================================================================================================

namespace {

const std::vector<std::pair<const char*, Path>>& builds()
{
    static const std::vector<std::pair<const char*, Path>> both = {{"katana_updater", kHelper},
                                                                    {"katana_updater_test", kTestHelper}};
    return both;
}

// A configuration that makes an OpenSSL that reads it unable to fetch ANY algorithm, Ed25519
// included: it asks for the FIPS property, which no provider here offers. (Measured with the
// openssl command: `openssl dgst -sha256` under it fails with "unsupported". A configuration that
// merely names a provider module that is missing does not break `openssl version` and is no test.)
Path writeHostileConfiguration(const Path& folder)
{
    const Path path = folder / "hostile.cnf";
    writeText(path,
              "openssl_conf = hostile_init\n"
              "[hostile_init]\n"
              "alg_section = hostile_algorithms\n"
              "[hostile_algorithms]\n"
              "default_properties = fips=yes\n");
    return path;
}

const wchar_t* const kOpenSslVariables[] = {L"OPENSSL_CONF", L"OPENSSL_MODULES", L"OPENSSL_ENGINES"};

} // namespace

TEST(HelperProgram, VersionIsTheProjectVersionAndTheProtocol)
{
    for (const auto& [name, program] : builds()) {
        const SpawnResult ran = runProgram(program, {"--version"});
        EXPECT_EQ(ran.exitCode, 0) << name << ": " << describe(ran);
        EXPECT_EQ(linesOf(ran.output), std::vector<std::string>{std::string("katana_updater ") + KATANA_EXPECTED_VERSION +
                                                                " applier=" + std::to_string(kApplierProtocol)})
            << name;
    }
}

TEST(HelperProgram, HelpPrintsTheUsageAndEveryExitCode)
{
    // The table at the top of src/katana_updater/main.cpp, where the codes are decided: each
    // code, and a word of what it means.
    const std::vector<std::pair<std::string, std::string>> documented = {
        {"0", "done"},     {"10", "blocked"},      {"11", "rolled back"}, {"12", "rollback incomplete"},
        {"13", "refused"}, {"20", "command line"}, {"21", "not trusted"}, {"30", "internal error"}};
    for (const auto& [name, program] : builds()) {
        const SpawnResult ran = runProgram(program, {"--help"});
        EXPECT_EQ(ran.exitCode, 0) << name << ": " << describe(ran);
        for (const std::string_view form : {"usage: katana_updater --root R --stage S --state U --tx T --wait-pid P "
                                            "--relaunch <path|none>",
                                            "katana_updater --recover R", "--selftest", "--version", "--help"}) {
            EXPECT_TRUE(contains(ran.output, form)) << name << ": " << form << "\n" << ran.output;
        }
        const std::vector<std::string> lines = linesOf(ran.output);
        for (const auto& [code, meaning] : documented) {
            const bool listed = std::any_of(lines.begin(), lines.end(), [&](const std::string& line) {
                const std::size_t begin = line.find_first_not_of(' ');
                if (begin == std::string::npos) {
                    return false;
                }
                const std::size_t end = line.find(' ', begin);
                return line.substr(begin, end == std::string::npos ? std::string::npos : end - begin) == code &&
                       contains(line, meaning);
            });
            EXPECT_TRUE(listed) << name << ": exit code " << code << " (" << meaning << ") is not in --help\n"
                                << ran.output;
        }
    }
    EXPECT_FALSE(contains(runProgram(kHelper, {"--help"}).output, "--die-at"))
        << "the shipped helper has no test options to tell of";
    EXPECT_TRUE(contains(runProgram(kTestHelper, {"--help"}).output, "test build"))
        << "the test helper says what it is";
}

TEST(HelperProgram, TheSelfTestVerifiesAKnownSignatureAndReportsWhatItFound)
{
    // None of the three variables is set here, whatever the machine that runs the tests has.
    ScopedEnvironment environment;
    for (const wchar_t* variable : kOpenSslVariables) {
        environment.unset(variable);
    }
    for (const auto& [name, program] : builds()) {
        const SpawnResult ran = runProgram(program, {"--selftest"});
        ASSERT_EQ(ran.exitCode, 0) << name << ": " << describe(ran);
        EXPECT_EQ(valueOf(ran.output, "selftest"), "ok") << name;
        EXPECT_EQ(valueOf(ran.output, "version"), KATANA_EXPECTED_VERSION) << name;
        EXPECT_EQ(valueOf(ran.output, "applier"), std::to_string(kApplierProtocol)) << name;
        EXPECT_EQ(valueOf(ran.output, "ed25519"), "ok") << name;
        EXPECT_EQ(valueOf(ran.output, "openssl_environment"), "cleared") << name;
        EXPECT_EQ(valueOf(ran.output, "openssl_removed"), "none") << name;
        EXPECT_FALSE(contains(ran.output, "problem=")) << name << ": " << ran.output;
    }
    // Which keys each build holds: the test build the test keys, the shipped one the pinned keys
    // (placeholders until the owner runs the key steps, docs/update.md) - never the test keys.
    EXPECT_EQ(valueOf(runProgram(kTestHelper, {"--selftest"}).output, "keys"), "test");
    const std::string shipped = valueOf(runProgram(kHelper, {"--selftest"}).output, "keys");
    EXPECT_TRUE(shipped == "pinned" || shipped == "placeholder") << shipped;
}

TEST(HelperProgram, TheSelfTestPassesUnderAHostileOpenSslConfModulesOrEnginesAndSaysWhatItRemoved)
{
    Scratch scratch("selftest-hostile");
    const Path configuration = writeHostileConfiguration(scratch.path());
    const Path missingFolder = scratch.path() / "no-such-folder";
    struct Case {
        const char* what;
        bool conf;
        bool modules;
        bool engines;
        const char* removed; // the names, in the order main.cpp lists the variables
    };
    for (const Case& hostile : {Case{"all three", true, true, true, "OPENSSL_CONF,OPENSSL_MODULES,OPENSSL_ENGINES"},
                                Case{"OPENSSL_CONF alone", true, false, false, "OPENSSL_CONF"},
                                Case{"OPENSSL_MODULES alone", false, true, false, "OPENSSL_MODULES"},
                                Case{"OPENSSL_ENGINES alone", false, false, true, "OPENSSL_ENGINES"}}) {
        ScopedEnvironment environment;
        for (const wchar_t* variable : kOpenSslVariables) {
            environment.unset(variable);
        }
        if (hostile.conf) {
            environment.set(L"OPENSSL_CONF", configuration);
        }
        if (hostile.modules) {
            environment.set(L"OPENSSL_MODULES", missingFolder);
        }
        if (hostile.engines) {
            environment.set(L"OPENSSL_ENGINES", missingFolder);
        }
        for (const auto& [name, program] : builds()) {
            const SpawnResult ran = runProgram(program, {"--selftest"});
            EXPECT_EQ(ran.exitCode, 0) << name << ", " << hostile.what << ": " << describe(ran);
            EXPECT_EQ(valueOf(ran.output, "selftest"), "ok") << name << ", " << hostile.what << ": " << ran.output;
            EXPECT_EQ(valueOf(ran.output, "ed25519"), "ok") << name << ", " << hostile.what;
            EXPECT_EQ(valueOf(ran.output, "openssl_environment"), "cleared") << name << ", " << hostile.what;
            // The variables reached the program and it took them away: "cleared" alone would also
            // be the answer of a program that was never handed them.
            EXPECT_EQ(valueOf(ran.output, "openssl_removed"), hostile.removed) << name << ", " << hostile.what;
        }
    }
}

TEST(HelperProgram, TheHostileConfigurationBreaksAProgramThatReadsIt)
{
    // The control of the case above: a program that does not harden its environment is broken by
    // this configuration, so the helper's pass is the helper's doing and not a configuration that
    // does nothing. The program is the openssl command, where one of version 3 or later is
    // installed and works; the release tools' tests need one too (tests/update/CMakeLists.txt).
    // Without one the case is skipped on a developer's machine and FAILS on a build service: a
    // control that skips there would leave the case above with nothing to show it means anything.
    Scratch scratch("selftest-control");
    const Path configuration = writeHostileConfiguration(scratch.path());
    wchar_t found[MAX_PATH * 4];
    const DWORD foundLength =
        SearchPathW(nullptr, L"openssl.exe", nullptr, static_cast<DWORD>(std::size(found)), found, nullptr);
    if (foundLength == 0 || foundLength >= std::size(found)) {
        SKIP_UNLESS_AVAILABLE_OR_FAIL_IN_CI(
            "no openssl.exe on PATH: the hostile configuration is not shown to be hostile here");
    }
    const Path openssl(std::wstring(found, foundLength));
    const std::vector<std::string> digest = {"dgst", "-sha256", toUtf8(configuration.native())};
    {
        ScopedEnvironment environment;
        for (const wchar_t* variable : kOpenSslVariables) {
            environment.unset(variable);
        }
        const SpawnResult version = runProgram(openssl, {"version"});
        const std::string text = version.output;
        const bool threeOrLater = version.exitCode == 0 && text.rfind("OpenSSL ", 0) == 0 && text.size() > 8 &&
                                  std::isdigit(static_cast<unsigned char>(text[8])) != 0 && text[8] >= '3';
        if (!threeOrLater || runProgram(openssl, digest).exitCode != 0) {
            SKIP_UNLESS_AVAILABLE_OR_FAIL_IN_CI(toUtf8(openssl.native()) + " is not a working OpenSSL 3 or later (" +
                                                text +
                                                "): the hostile configuration is not shown to be hostile here");
        }
    }
    ScopedEnvironment environment;
    environment.set(L"OPENSSL_CONF", configuration);
    const SpawnResult control = runProgram(openssl, digest);
    EXPECT_NE(control.exitCode, 0) << "the hostile configuration should break an OpenSSL that reads it: "
                                   << describe(control);
}

TEST(HelperProgram, EveryKindOfBadCommandLineIsAUsageErrorWithExitCode20)
{
    Scratch scratch("usage");
    const std::string root = toUtf8(scratch.path().native());
    const std::string folder = toUtf8((scratch.path() / "x").native());
    const std::string tx = "a1b2c3d4";
    // A full, valid swap command line, from which one thing is broken at a time.
    const auto full = [&]() {
        return std::vector<std::string>{"--root", folder, "--stage", folder, "--state", folder, "--tx", tx,
                                        "--wait-pid", "0", "--relaunch", "none"};
    };
    const auto without = [&](std::string_view option) {
        std::vector<std::string> arguments = full();
        for (std::size_t i = 0; i < arguments.size(); i += 2) {
            if (arguments[i] == option) {
                arguments.erase(arguments.begin() + static_cast<std::ptrdiff_t>(i),
                                arguments.begin() + static_cast<std::ptrdiff_t>(i) + 2);
                break;
            }
        }
        return arguments;
    };
    const auto with = [&](std::string_view option, std::string value) {
        std::vector<std::string> arguments = full();
        for (std::size_t i = 0; i < arguments.size(); i += 2) {
            if (arguments[i] == option) {
                arguments[i + 1] = std::move(value);
            }
        }
        return arguments;
    };
    std::vector<std::pair<std::string, std::vector<std::string>>> cases = {
        {"no arguments", {}},
        {"an unknown option", {"--frobnicate"}},
        {"an unknown option with a value", {"--frobnicate", "x"}},
        {"a bare word", {"update"}},
        {"--root without its value", {"--root"}},
        {"--version with another option", {"--version", "--selftest"}},
        {"--selftest with an argument", {"--selftest", "x"}},
        {"--help with an argument", {"--help", "x"}},
        {"--recover without a folder", {"--recover"}},
        {"--recover with two", {"--recover", root, root}},
        {"--recover with an empty folder", {"--recover", ""}},
        {"--recover with a folder that does not exist", {"--recover", folder}},
        {"--recover with a file", {"--recover", toUtf8((scratch.path() / "file.txt").native())}},
        {"no --root", without("--root")},
        {"no --stage", without("--stage")},
        {"no --state", without("--state")},
        {"no --tx", without("--tx")},
        {"no --wait-pid", without("--wait-pid")},
        {"no --relaunch", without("--relaunch")},
        {"a repeated option", [&] { auto a = full(); a.push_back("--tx"); a.push_back(tx); return a; }()},
        {"an unknown option among the rest", [&] { auto a = full(); a.push_back("--x"); a.push_back("y"); return a; }()},
        {"a transaction id with a separator", with("--tx", "..\\evil")},
        {"a transaction id with a parent folder", with("--tx", "../x")},
        {"a transaction id in capitals", with("--tx", "A1B2C3")},
        {"an empty transaction id", with("--tx", "")},
        {"a transaction id of 65 characters", with("--tx", std::string(65, 'a'))},
        {"a process id that is not a number", with("--wait-pid", "abc")},
        {"a negative process id", with("--wait-pid", "-1")},
        {"an empty process id", with("--wait-pid", "")},
        {"a relative install folder", with("--root", "Install")},
        {"a relative stage folder", with("--stage", "..\\stage")},
        {"a relative state folder", with("--state", "state")},
        {"a relative program to start", with("--relaunch", "bin\\katana.exe")},
        {"an empty install folder", with("--root", "")},
    };
    writeText(scratch.path() / "file.txt", "x");
    for (const auto& [name, program] : builds()) {
        for (const auto& [what, arguments] : cases) {
            const SpawnResult ran = runProgram(program, arguments);
            EXPECT_EQ(ran.exitCode, 20) << name << ": " << what << "\n" << describe(ran);
            EXPECT_TRUE(contains(ran.output, "katana_updater: ")) << name << ": " << what << "\n" << ran.output;
            EXPECT_TRUE(contains(ran.output, "usage: katana_updater")) << name << ": " << what << "\n" << ran.output;
        }
    }
    // The control: the command line the cases were broken from is not itself a usage error. Its
    // folders do not exist, so the program refuses it for what it finds (13), having read it.
    for (const auto& [name, program] : builds()) {
        const SpawnResult ran = runProgram(program, full());
        EXPECT_EQ(ran.exitCode, 13) << name << ": " << describe(ran);
        EXPECT_EQ(valueOf(ran.output, "reason"), "development_tree") << name << ": " << ran.output;
        EXPECT_FALSE(contains(ran.output, "usage: katana_updater")) << name << ": " << ran.output;
    }
}

TEST(HelperProgram, TheShippedHelperHasNoTestOnlyOptionsAndTheTestHelperRefusesThemMalformed)
{
    Scratch scratch("usage-test-options");
    const std::string folder = toUtf8(scratch.path().native());
    const std::string program = toUtf8(kStub.native());
    for (const std::vector<std::string>& arguments : std::vector<std::vector<std::string>>{
             {"--die-at", "3", "--recover", folder},
             {"--registry-base", "Software\\x", "--recover", folder},
             {"--timings", "1,2,3,4,5", "--recover", folder},
             {"--stand-in", program, "--recover", folder}}) {
        const SpawnResult ran = runProgram(kHelper, arguments);
        EXPECT_EQ(ran.exitCode, 20) << describe(ran);
        EXPECT_TRUE(contains(ran.output, "unknown option")) << "the shipped helper has no such option\n" << ran.output;
    }
    // Behind a mode the program takes no more arguments at all, so the option is refused there too.
    EXPECT_EQ(runProgram(kHelper, {"--recover", folder, "--die-at", "3"}).exitCode, 20);
    for (const std::vector<std::string>& arguments : std::vector<std::vector<std::string>>{
             {"--die-at", "0", "--recover", folder},
             {"--die-at", "x", "--recover", folder},
             {"--die-at", "-3", "--recover", folder},
             {"--die-at", "--recover", folder},
             {"--registry-base", "", "--recover", folder},
             {"--registry-base", "\\Software\\x", "--recover", folder},
             {"--timings", "1,2,3,4", "--recover", folder},
             {"--timings", "1,2,3,4,5,6", "--recover", folder},
             {"--timings", "1,2,3,x,5", "--recover", folder},
             {"--stand-in", "stub.exe", "--recover", folder},
             {"--stand-in", program, "--stand-in", program, "--recover", folder},
             {"--die-at", "1", "--die-at", "2", "--recover", folder}}) {
        const SpawnResult ran = runProgram(kTestHelper, arguments);
        EXPECT_EQ(ran.exitCode, 20) << describe(ran);
    }

    // The registry a test helper is told of by the environment is held to the same rule, and the
    // shipped helper does not read the variable at all.
    ScopedEnvironment environment;
    environment.set(L"KATANA_UPDATER_TEST_REGISTRY_BASE", std::wstring(L"\\Software\\x"));
    EXPECT_EQ(runProgram(kTestHelper, {"--version"}).exitCode, 20);
    EXPECT_EQ(runProgram(kHelper, {"--version"}).exitCode, 0);
}

// =====================================================================================================
// The import table
// =====================================================================================================

namespace {

// The DLLs a PE file imports, read from the file's import directory (not from objdump, which
// the ctest entries of CMakeLists.txt use: this is a second reading, in the test program).
std::vector<std::string> importedLibraries(const Path& executable)
{
    const std::string bytes = readText(executable);
    const auto* base = reinterpret_cast<const unsigned char*>(bytes.data());
    std::vector<std::string> names;
    if (bytes.size() < sizeof(IMAGE_DOS_HEADER)) {
        return names;
    }
    const auto* dos = reinterpret_cast<const IMAGE_DOS_HEADER*>(base);
    if (dos->e_magic != IMAGE_DOS_SIGNATURE || dos->e_lfanew < 0 ||
        static_cast<std::size_t>(dos->e_lfanew) + sizeof(IMAGE_NT_HEADERS64) > bytes.size()) {
        return names;
    }
    const auto* nt = reinterpret_cast<const IMAGE_NT_HEADERS64*>(base + dos->e_lfanew);
    if (nt->Signature != IMAGE_NT_SIGNATURE || nt->OptionalHeader.Magic != IMAGE_NT_OPTIONAL_HDR64_MAGIC) {
        return names; // x86-64 and ARM64 are both PE32+
    }
    const IMAGE_DATA_DIRECTORY imports = nt->OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_IMPORT];
    const IMAGE_SECTION_HEADER* sections = IMAGE_FIRST_SECTION(nt);
    const auto toOffset = [&](DWORD rva) -> std::optional<std::size_t> {
        for (unsigned i = 0; i < nt->FileHeader.NumberOfSections; ++i) {
            const IMAGE_SECTION_HEADER& section = sections[i];
            const DWORD size = std::max(section.Misc.VirtualSize, section.SizeOfRawData);
            if (rva >= section.VirtualAddress && rva < section.VirtualAddress + size) {
                return static_cast<std::size_t>(rva - section.VirtualAddress + section.PointerToRawData);
            }
        }
        return std::nullopt;
    };
    if (imports.VirtualAddress == 0) {
        return names;
    }
    const std::optional<std::size_t> table = toOffset(imports.VirtualAddress);
    for (std::size_t at = table.value_or(bytes.size()); at + sizeof(IMAGE_IMPORT_DESCRIPTOR) <= bytes.size();
         at += sizeof(IMAGE_IMPORT_DESCRIPTOR)) {
        const auto* descriptor = reinterpret_cast<const IMAGE_IMPORT_DESCRIPTOR*>(base + at);
        if (descriptor->Name == 0) {
            break;
        }
        if (const std::optional<std::size_t> nameAt = toOffset(descriptor->Name); nameAt && *nameAt < bytes.size()) {
            names.emplace_back(bytes.c_str() + *nameAt);
        }
    }
    return names;
}

std::string lowerAscii(std::string text)
{
    for (char& c : text) {
        c = static_cast<char>(std::tolower(static_cast<unsigned char>(c)));
    }
    return text;
}

// The DLLs of Windows itself: the names of tests/update/check_updater_imports.cmake, one for one.
// Which DLLs are Windows' is decided there; what this file adds is a second READING of the import
// table, by other code than objdump's.
bool isOperatingSystemLibrary(const std::string& name)
{
    static const std::set<std::string> system = {
        "kernel32", "kernelbase", "advapi32", "user32",  "shell32",  "ole32",    "oleaut32", "ws2_32",
        "crypt32",  "bcrypt",     "ncrypt",   "ntdll",   "rpcrt4",   "secur32",  "userenv",  "version",
        "shlwapi",  "iphlpapi",   "wintrust", "psapi",   "cfgmgr32", "sechost",  "msvcrt",   "ucrtbase"};
    const std::string lower = lowerAscii(name);
    constexpr std::string_view extension = ".dll";
    if (lower.size() <= extension.size() || lower.substr(lower.size() - extension.size()) != extension) {
        return false;
    }
    const std::string stem = lower.substr(0, lower.size() - extension.size());
    return system.contains(stem) || stem.rfind("api-ms-win-", 0) == 0;
}

} // namespace

TEST(HelperImports, EveryProgramTheHelperRunsImportsOnlyDllsOfTheOperatingSystem)
{
    // The shipped helper, the same sources with the test key, and the stub the end-to-end cases
    // start from an install: none may need a DLL of the toolchain, because the first two run from
    // a folder of their own (S1) and the third from an install whose bin/ is being replaced.
    for (const Path& program : {kHelper, kTestHelper, kStub}) {
        const std::vector<std::string> imported = importedLibraries(program);
        ASSERT_FALSE(imported.empty()) << toUtf8(program.native()) << ": no import table was read";
        std::string listing;
        for (const std::string& name : imported) {
            listing += (listing.empty() ? "" : ", ") + name;
            EXPECT_TRUE(isOperatingSystemLibrary(name))
                << toUtf8(program.filename().native()) << " imports " << name << ", which is not the operating system's";
        }
        // Every program for Windows imports its kernel: a reading that did not find it read nothing.
        EXPECT_TRUE(std::any_of(imported.begin(), imported.end(),
                                [](const std::string& name) { return lowerAscii(name) == "kernel32.dll"; }))
            << toUtf8(program.filename().native()) << ": " << listing;
        RecordProperty(("imports_" + toUtf8(program.stem().native())).c_str(), listing);
        std::cout << "[  IMPORTS ] " << toUtf8(program.filename().native()) << ": " << listing << "\n";
    }
}

TEST(HelperImports, TheCheckSeesADynamicallyLinkedProgramForWhatItIs)
{
    // The control: a program that is NOT static must be reported as importing a DLL of the
    // toolchain, or the case above would pass on a reader that cannot see it. The test program
    // itself links libstdc++ as a DLL.
    wchar_t self[MAX_PATH * 4];
    const DWORD length = GetModuleFileNameW(nullptr, self, static_cast<DWORD>(std::size(self)));
    ASSERT_GT(length, 0u);
    const std::vector<std::string> imported = importedLibraries(Path(std::wstring(self, length)));
    ASSERT_FALSE(imported.empty());
    const bool foreign = std::any_of(imported.begin(), imported.end(),
                                     [](const std::string& name) { return !isOperatingSystemLibrary(name); });
    EXPECT_TRUE(foreign) << "the test program imports only system DLLs, so it is no control for the reader";
    EXPECT_FALSE(isOperatingSystemLibrary("libstdc++-6.dll"));
    EXPECT_FALSE(isOperatingSystemLibrary("libgcc_s_seh-1.dll"));
    EXPECT_FALSE(isOperatingSystemLibrary("libwinpthread-1.dll"));
    EXPECT_FALSE(isOperatingSystemLibrary("libcrypto-3-x64.dll"));
    EXPECT_FALSE(isOperatingSystemLibrary("zlib1.dll"));
    EXPECT_FALSE(isOperatingSystemLibrary("kernel32")) << "a name, not a prefix";
    EXPECT_FALSE(isOperatingSystemLibrary("kernel32.dll.evil.dll"));
    EXPECT_TRUE(isOperatingSystemLibrary("KERNEL32.dll"));
    EXPECT_TRUE(isOperatingSystemLibrary("api-ms-win-crt-stdio-l1-1-0.dll"));
}

TEST(HelperProgram, TheCopyRunsFromAFolderOfItsOwnWithNothingOfTheToolchainOnPath)
{
    // S1 of the design: the helper never runs from bin/, so it must need nothing beside it.
    Scratch scratch("selftest-alone");
    const Path copy = scratch.path() / kUnicodeFolder / "run-1" / "katana_updater.exe";
    copyProgram(kHelper, copy);
    // Read before it is run. A helper that needed a DLL of the toolchain would not fail quietly
    // here: started with nothing of the toolchain on PATH it would put up the loader's own
    // message box ("... was not found") and sit behind it until somebody clicked - the Launcher
    // gives a program it starts the default error mode. So the regression this case is for is
    // caught from the file, and the program is only started once it can load.
    const std::vector<std::string> imported = importedLibraries(copy);
    ASSERT_FALSE(imported.empty()) << "no import table was read from the copy: it is not started";
    for (const std::string& name : imported) {
        ASSERT_TRUE(isOperatingSystemLibrary(name))
            << "katana_updater imports " << name << ", which is not the operating system's: it is not started";
    }
    wchar_t windows[MAX_PATH];
    const UINT length = GetWindowsDirectoryW(windows, MAX_PATH);
    const std::wstring systemFolders =
        std::wstring(windows, length) + L"\\System32;" + std::wstring(windows, length);
    ScopedEnvironment environment;
    environment.set(L"PATH", systemFolders);
    // A self-test takes a moment; half a minute is a virus scanner's first look at a new file.
    const SpawnResult ran = runProgram(copy, {"--selftest"}, copy.parent_path(), std::chrono::seconds(30));
    EXPECT_FALSE(ran.timedOut) << describe(ran);
    EXPECT_EQ(ran.exitCode, 0) << describe(ran);
    EXPECT_EQ(valueOf(ran.output, "selftest"), "ok");
}

// =====================================================================================================
// The swap, through the real program
// =====================================================================================================

namespace {

// One run of the real program in a fresh world, killed before its k-th change to the disk
// (--die-at, the test build's FaultFs; a crash is a process that stops).
struct CrashProbe {
    std::unique_ptr<World> world;
    SpawnResult ran;
    bool crashed = false;      // it ended by the simulated crash, not by finishing
    bool inTheCommit = false;  // COMMITTING is in the journal and the tree is neither version's
};

CrashProbe probeCrash(const std::string& label, std::uint64_t k)
{
    CrashProbe probe;
    probe.world = std::make_unique<World>(label + "-" + std::to_string(k));
    World& world = *probe.world;
    world.arrangeEnvironment();
    std::vector<std::string> arguments = world.applyArguments();
    arguments.push_back("--die-at");
    arguments.push_back(std::to_string(k));
    probe.ran = world.runHelper(arguments);
    probe.crashed = probe.ran.exitCode == 99;
    const std::string journal = readText(world.root / ".update" / std::string(kTransaction) / "journal");
    const std::string tree = treeHash(world.root);
    probe.inTheCommit = probe.crashed && contains(journal, "COMMITTING") &&
                        !contains(journal, "COMMITTED_PENDING_HEALTH") && tree != world.v1Hash &&
                        tree != world.v2Hash;
    return probe;
}

// Finds a crash in the MIDDLE of the commit, with `wanted` agreeing, and returns the world it
// happened in with the crash left on disk. The number of the first change after the journal says
// COMMITTING is found by bisection, and the changes after it are tried in turn: each is a rename
// or a journal line.
std::unique_ptr<World> crashedMidCommit(const std::string& label, const std::function<bool(World&)>& wanted,
                                        SpawnResult* crashed = nullptr, std::uint64_t* at = nullptr)
{
    // Whether the run killed before change k had got as far as COMMITTING. A bisection needs an
    // answer that, once yes, stays yes as k grows - so "COMMITTING is in the journal" is not
    // enough by itself:
    //   - not killed at all: k is past the last change, and so past the commit;
    //   - COMMITTING is in the journal;
    //   - there is no journal to read. Before the plan there is none yet. After a healthy update
    //     there is none ANY MORE: the clean-up deletes it with the transaction's folder while
    //     changes are still being counted (the staged files, the records in U), and a run killed
    //     there would read as "before the commit". The install's own record tells the two apart:
    //     installed.json is put in place by the commit, among its last files (apply.hpp, step 5),
    //     and by nothing before it, so the new version's record means the commit has begun.
    const auto reachedTheCommit = [&](std::uint64_t k) {
        const CrashProbe probe = probeCrash(label, k);
        if (!probe.crashed) {
            return true;
        }
        const World& killed = *probe.world;
        if (contains(readText(killed.root / ".update" / std::string(kTransaction) / "journal"), "COMMITTING")) {
            return true;
        }
        return readText(killed.root / "share" / "katana" / "installed.json") == installedJson(killed.v2);
    };
    // The upper end is found, not assumed: doubled until a run gets as far as the commit, so that
    // a release of more files than today's does not put the answer out of reach.
    std::uint64_t low = 1; // before the first change: nothing on disk
    std::uint64_t high = 32;
    while (!reachedTheCommit(high)) {
        low = high;
        high *= 2;
        if (high > 4096) {
            ADD_FAILURE() << "no run got as far as the commit within 4096 changes to the disk";
            return nullptr;
        }
    }
    while (low + 1 < high) {
        const std::uint64_t middle = low + (high - low) / 2;
        (reachedTheCommit(middle) ? high : low) = middle;
    }
    for (std::uint64_t k = high; k < high + 40; ++k) {
        CrashProbe probe = probeCrash(label, k);
        if (!probe.crashed) {
            break;
        }
        if (probe.inTheCommit && wanted(*probe.world)) {
            if (crashed != nullptr) {
                *crashed = probe.ran;
            }
            if (at != nullptr) {
                *at = k;
            }
            return std::move(probe.world);
        }
    }
    ADD_FAILURE() << "no crash point fell in the middle of the commit (the journal reaches COMMITTING at change "
                  << high << ")";
    return nullptr;
}

// A tree that is truly half of each: the commit moves data first and programs last (apply.hpp,
// step 5), so there is a stretch in which the readme is the new version's and katana.exe still
// the old one's.
bool isHalfSwapped(World& world)
{
    return readText(world.root / "share" / "katana" / "readme.txt") == world.v2.find("share/katana/readme.txt")->content &&
           readText(world.root / "bin" / "katana.exe") == world.v1.find("bin/katana.exe")->content;
}

// The sequences state.json holds as blocked: the releases whose health check failed on this
// machine, which are not offered again as they were (state.hpp). None when there is no file.
std::vector<std::uint64_t> blockedSequences(const Path& stateDirectory)
{
    const core::Result<UpdateState> recorded = readState(stateDirectory);
    EXPECT_TRUE(recorded.ok()) << (recorded.ok() ? std::string() : recorded.error().describe());
    return recorded.ok() ? recorded->blockedSequences : std::vector<std::uint64_t>{};
}

} // namespace

TEST(HelperSwap, ASignedUpdateIsAppliedByTheProgramAndTheTreeIsTheNewVersionsWithAHostileEnvironment)
{
    REQUIRE_CAN_SIGN();
    std::wstring hiveKey;
    {
        World world("swap-commit");
        hiveKey = toWide(world.hive.base());
        const Path hostile = writeHostileConfiguration(world.top_);
        world.arrangeEnvironment();
        world.registerAsInstaller();
        world.environment.set(L"OPENSSL_CONF", hostile);
        world.environment.set(L"OPENSSL_MODULES", world.top_ / "no-modules");
        world.environment.set(L"OPENSSL_ENGINES", world.top_ / "no-engines");

        const SpawnResult ran = world.runHelper(world.applyArguments());
        ASSERT_EQ(ran.exitCode, 0) << describe(ran);
        EXPECT_EQ(valueOf(ran.output, "outcome"), "committed");
        world.expectV2();
        EXPECT_EQ(world.displayVersion(), std::wstring(L"0.10.0")) << "Settings > Apps names the installed version";
        EXPECT_EQ(world.displayName(), std::wstring(L"Katana 0.10.0"));
        EXPECT_TRUE(leftoversInUpdate(world.root).empty()) << "a healthy update leaves nothing in R/.update";

        const LastResult last = world.last();
        EXPECT_EQ(last.status, "committed");
        EXPECT_EQ(last.version, "0.10.0");
        EXPECT_EQ(last.transaction, kTransaction);
        EXPECT_FALSE(isPresent(healthMarkerPath(world.state, std::string(kTransaction)))) << "the marker is cleaned up";
        EXPECT_FALSE(world.runOnceIsSet()) << "the RunOnce entry is removed when the transaction ends";

        // The new version was started for its health check, from the install, with the person's
        // OPENSSL_CONF handed on (the helper itself ran without it), and with the helper's RunOnce
        // entry in place at that moment: it names the helper copy and the install.
        const std::vector<std::string> lines = linesOf(world.stubLog());
        ASSERT_GE(lines.size(), 1u) << world.stubLog();
        const std::string& health = lines.front();
        EXPECT_EQ(health.rfind("health tag=v2 ", 0), 0u) << health;
        EXPECT_TRUE(contains(health, "exe=" + toUtf8((world.root / "bin" / "katana.exe").native()) + " ")) << health;
        EXPECT_TRUE(contains(health, "OPENSSL_CONF=" + toUtf8(hostile.native()))) << health;
        const std::string expectedRunOnce = runOnceCommand(world.helperPath(std::string(kTransaction)), world.root);
        EXPECT_LE(toWide(expectedRunOnce).size(), 260u) << "the premise: a command Windows would run";
        EXPECT_TRUE(contains(health, "runonce=" + expectedRunOnce + " ")) << health << "\nexpected the RunOnce command\n"
                                                                          << expectedRunOnce;
        EXPECT_TRUE(contains(health, "tx=" + std::string(kTransaction)));
        // And the new version is started at the end, the way the window's own relaunch starts it.
        ASSERT_TRUE(world.waitForLog("start tag=v2 ")) << world.stubLog();
        const std::vector<std::string> after = linesOf(world.stubLog());
        ASSERT_EQ(after.size(), 2u) << world.stubLog();
        EXPECT_TRUE(contains(after[1], "OPENSSL_CONF=" + toUtf8(hostile.native()))) << after[1];
        EXPECT_TRUE(contains(after[1], "exe=" + toUtf8((world.root / "bin" / "katana.exe").native()) + " ")) << after[1];
        EXPECT_TRUE(contains(after[1], "runonce=-")) << "the entry was gone before Katana was started again: " << after[1];
        EXPECT_FALSE(contains(after[1], " tx=")) << "the relaunch is a plain start, not another health check";
    }
    EXPECT_FALSE(keyExists(hiveKey)) << "the test's registry keys went with the test: nothing is left behind";
}

TEST(HelperSwap, ATestHelperToldOfNoRegistryWritesNoRunOnceEntryOnTheMachine)
{
    REQUIRE_CAN_SIGN();
    // The machine's own key is only READ, by the stub, at the moment a shipped helper would have
    // its entry there: during the health check. The engine's own command line is used as it is,
    // with no test option, and the environment names no registry either. A machine on which a
    // real Katana was interrupted has such an entry already, and the case cannot judge it; a
    // build service has no Katana installed, so there the entry would be a test's own leak, and
    // that is a failure.
    if (readString(kMachineRunOnceKey, kRunOnceValue.c_str()).has_value()) {
        SKIP_UNLESS_AVAILABLE_OR_FAIL_IN_CI(
            "this machine has a Katana recovery entry in its RunOnce key already; it is not this test's to judge");
    }
    World world("swap-no-registry");
    world.arrangeEnvironment();
    world.environment.unset(L"KATANA_UPDATER_TEST_REGISTRY_BASE");
    world.environment.set(L"KATANA_STUB_MACHINE_RUNONCE", std::wstring(L"1"));

    const SpawnResult ran = world.runHelper(world.engineArguments());
    ASSERT_EQ(ran.exitCode, 0) << describe(ran);
    world.expectV2();
    const std::vector<std::string> lines = linesOf(world.stubLog());
    ASSERT_GE(lines.size(), 1u) << world.stubLog();
    EXPECT_EQ(lines.front().rfind("health tag=v2 ", 0), 0u) << lines.front();
    EXPECT_TRUE(contains(lines.front(), " machine_runonce=- ")) << "the test helper wrote the machine's RunOnce key: "
                                                                << lines.front();
    EXPECT_TRUE(contains(lines.front(), " runonce=- ")) << "and it had no registry of the test's to write: "
                                                        << lines.front();
    EXPECT_FALSE(world.hive.exists()) << "nothing was written under the test's key either";
    ASSERT_TRUE(world.waitForLog("start tag=v2 ")) << world.stubLog();
}

TEST(HelperSwap, TheShippedHelperWillNotApplyAReleaseSignedWithTheTestKeyAndTouchesNothing)
{
    REQUIRE_CAN_SIGN();
    World world("swap-shipped-trust");
    world.arrangeEnvironment();
    copyProgram(kHelper, world.helperPath("b1b2"));
    // The real program with the engine's own command line, and a stage signed with a key anybody
    // holds: the pinned keys do not include it, so the answer is "not trusted". It reads the
    // machine's registry for the installer's folder and, refusing before the plan, writes none.
    const SpawnResult ran = world.runHelper(world.engineArguments("b1b2"), "b1b2");
    EXPECT_EQ(ran.exitCode, 21) << describe(ran);
    EXPECT_EQ(valueOf(ran.output, "outcome"), "refused");
    EXPECT_EQ(valueOf(ran.output, "reason"), "staged_manifest");
    world.expectV1();
    EXPECT_TRUE(leftoversInUpdate(world.root).empty());
    EXPECT_FALSE(world.runOnceIsSet());
    EXPECT_EQ(world.last().status, "refused");
    // The window had closed to let the update in: it is started again, as it was.
    ASSERT_TRUE(world.waitForLog("start tag=v1 ")) << world.stubLog();
}

TEST(HelperSwap, ADevelopmentTreeIsNeverTouchedAndTheHelperExitsWithCode13)
{
    REQUIRE_CAN_SIGN();
    // A tree with no share/katana/install.json is a build tree or a plain install, never an
    // install an update may change (install.hpp) - whatever is staged for it, however well signed.
    World world("swap-development-tree");
    world.arrangeEnvironment();
    fs::remove(world.root / "share" / "katana" / "install.json");
    const std::map<std::string, std::string> before = snapshot(world.root);

    const SpawnResult ran = world.runHelper(world.applyArguments());
    EXPECT_EQ(ran.exitCode, 13) << describe(ran);
    EXPECT_EQ(valueOf(ran.output, "outcome"), "refused");
    EXPECT_EQ(valueOf(ran.output, "reason"), "development_tree");
    EXPECT_EQ(difference(before, snapshot(world.root)), "") << "the tree was changed";
    EXPECT_FALSE(isPresent(world.root / ".update")) << "not even the work folder is made in a development tree";
    EXPECT_FALSE(world.runOnceIsSet());
    EXPECT_EQ(world.last().reason, "development_tree");
    ASSERT_TRUE(world.waitForLog("start tag=v1 ")) << world.stubLog();
}

TEST(HelperSwap, TheFixturesReleaseSignedByOpensslIsAppliedEndToEndWithARealProgramStandingInForItsKatana)
{
    // The T3 fixtures: trees and a manifest made by the Python producer, signed by the openssl
    // command with the published test key - nothing of this program made them. The tree the
    // update must leave is the fixtures' own v2 install with v1's seed and v2's records.
    FixtureWorld world("swap-fixtures");
    ScopedEnvironment environment;
    environment.set(L"KATANA_STUB_LOG", world.log);
    environment.set(L"KATANA_STUB_MODE", std::wstring(L"healthy"));
    environment.set(L"KATANA_UPDATE_STATE_DIR", world.s.state);
    environment.set(L"KATANA_STUB_REGISTRY_BASE", toWide(world.hive.base()));
    const std::string tx(kFixtureTransaction);

    std::vector<std::string> arguments = world.applyArguments();
    arguments.push_back("--stand-in");
    arguments.push_back(toUtf8(kStub.native()));
    const SpawnResult ran = world.run(arguments);
    ASSERT_EQ(ran.exitCode, 0) << describe(ran);
    EXPECT_EQ(valueOf(ran.output, "outcome"), "committed");
    EXPECT_EQ(treeHash(world.s.root), world.s.v2Hash)
        << "the tree must be the fixtures' v2\n"
        << difference(snapshot(world.s.expected), snapshot(world.s.root));
    EXPECT_TRUE(leftoversInUpdate(world.s.root).empty());
    EXPECT_FALSE(world.runOnceIsSet());

    const core::Result<std::optional<LastResult>> last = readLastResult(world.s.state);
    ASSERT_TRUE(last.ok() && last->has_value());
    EXPECT_EQ((*last)->status, "committed");
    EXPECT_EQ((*last)->version, "0.10.0");
    EXPECT_EQ((*last)->transaction, tx);

    // One start, of the stand-in, as the health check of this transaction, from the install's
    // bin folder, with the RunOnce entry standing. Nothing is started at the end (--relaunch none).
    const std::vector<std::string> lines = linesOf(logOf(world.log));
    ASSERT_EQ(lines.size(), 1u) << logOf(world.log);
    EXPECT_EQ(lines[0].rfind("health tag=none ", 0), 0u) << "the stand-in is the build's own, untagged stub: " << lines[0];
    EXPECT_TRUE(contains(lines[0], "cwd=" + toUtf8((world.s.root / "bin").native()) + " ")) << lines[0];
    EXPECT_TRUE(contains(lines[0], "runonce=" + runOnceCommand(world.helper(), world.s.root) + " ")) << lines[0];
    EXPECT_TRUE(contains(lines[0], "tx=" + tx + " ")) << lines[0];
}

TEST(HelperSwap, ANewVersionThatCannotBeStartedAtAllIsRolledBackAtOnceAndNotOfferedAgain)
{
    // The new katana is not there to start - a virus scanner took it between the commit and the
    // health check, say. (The stand-in is a path with no file: starting it is an error of the
    // operating system's, at once and with no message box. A file that is NOT a program is never
    // started by a test.)
    FixtureWorld world("swap-unstartable");
    const Path missing = world.top / "not here" / "katana.exe";
    std::vector<std::string> arguments = world.applyArguments("500,1500,50,60000,30000");
    arguments.push_back("--stand-in");
    arguments.push_back(toUtf8(missing.native()));
    const auto begin = std::chrono::steady_clock::now();
    const SpawnResult ran = world.run(arguments);
    const auto elapsed = std::chrono::steady_clock::now() - begin;
    EXPECT_EQ(ran.exitCode, 11) << describe(ran);
    EXPECT_EQ(valueOf(ran.output, "outcome"), "rolled_back");
    EXPECT_EQ(valueOf(ran.output, "reason"), "health_exit");
    EXPECT_EQ(treeHash(world.s.root), world.s.v1Hash) << difference(world.s.v1Snapshot, snapshot(world.s.root));
    EXPECT_LT(elapsed, 30s) << "the 60 s health wait must not have been spent";
    EXPECT_TRUE(leftoversInUpdate(world.s.root).empty());
    EXPECT_FALSE(world.runOnceIsSet());
    const core::Result<std::optional<LastResult>> last = readLastResult(world.s.state);
    ASSERT_TRUE(last.ok() && last->has_value());
    EXPECT_EQ((*last)->status, "rolled_back");
    EXPECT_EQ((*last)->reason, "health_exit");
    const core::Result<UpdateState> recorded = readState(world.s.state);
    ASSERT_TRUE(recorded.ok());
    EXPECT_EQ(recorded->blockedSequences, std::vector<std::uint64_t>{kFixtureV2Sequence})
        << "the sequence of the fixtures' v2 manifest";
}

TEST(HelperSwap, EveryBadReleaseOfTheFixturesIsRefusedWithExitCode21AndTheTreeIsUntouched)
{
    struct Bad {
        const char* release;
        const char* installed;
        const char* reason;
    };
    for (const Bad& bad : {Bad{"flipped-manifest-bit", "v1", "staged_manifest"}, Bad{"swapped-sig", "v1", "staged_manifest"},
                           Bad{"wrong-key", "v1", "staged_manifest"}, Bad{"wrong-key-claims-active", "v1", "staged_manifest"},
                           Bad{"path-traversal", "v1", "staged_manifest"}, Bad{"downgrade-version", "v2", "downgrade"},
                           Bad{"downgrade-sequence", "v2", "downgrade"}}) {
        FixtureWorld world(std::string("helper-bad-") + bad.release, bad.installed, std::string("bad/") + bad.release);
        const SpawnResult ran = world.run(world.applyArguments());
        EXPECT_EQ(ran.exitCode, 21) << bad.release << ": " << describe(ran);
        EXPECT_EQ(valueOf(ran.output, "outcome"), "refused") << bad.release;
        EXPECT_EQ(valueOf(ran.output, "reason"), bad.reason) << bad.release;
        EXPECT_EQ(treeHash(world.s.root), world.s.v1Hash) << bad.release << difference(world.s.v1Snapshot, snapshot(world.s.root));
        EXPECT_TRUE(leftoversInUpdate(world.s.root).empty()) << bad.release;
        EXPECT_FALSE(world.runOnceIsSet()) << bad.release;
        const core::Result<std::optional<LastResult>> last = readLastResult(world.s.state);
        ASSERT_TRUE(last.ok() && last->has_value()) << bad.release;
        EXPECT_EQ((*last)->status, "refused") << bad.release;
        EXPECT_EQ((*last)->reason, bad.reason) << bad.release;
    }
}

TEST(HelperSwap, ANewVersionThatExitsWithAnErrorIsRolledBackWithExitCode11AndTheOldOneStartsAgain)
{
    REQUIRE_CAN_SIGN();
    World world("swap-health-exit");
    world.arrangeEnvironment("exit1");
    world.registerAsInstaller();
    const SpawnResult ran = world.runHelper(world.applyArguments());
    ASSERT_EQ(ran.exitCode, 11) << describe(ran);
    EXPECT_EQ(valueOf(ran.output, "outcome"), "rolled_back");
    EXPECT_EQ(valueOf(ran.output, "reason"), "health_exit");
    world.expectV1();
    EXPECT_EQ(world.displayVersion(), std::wstring(L"0.9.0")) << "a release that did not start is never shown as installed";
    EXPECT_EQ(world.displayName(), std::wstring(L"Katana 0.9.0"));
    EXPECT_TRUE(leftoversInUpdate(world.root).empty());
    EXPECT_FALSE(world.runOnceIsSet());
    EXPECT_EQ(world.last().status, "rolled_back");
    EXPECT_EQ(world.last().reason, "health_exit");

    const core::Result<UpdateState> recorded = readState(world.state);
    ASSERT_TRUE(recorded.ok());
    EXPECT_EQ(recorded->blockedSequences, std::vector<std::uint64_t>{2000})
        << "a release that does not start is not offered again";

    // The new version tried to start, and then the OLD one was started: the person gets Katana back.
    ASSERT_TRUE(world.waitForLog("start tag=v1 ")) << world.stubLog();
    EXPECT_EQ(linesOf(world.stubLog()).front().rfind("health tag=v2 ", 0), 0u);
}

TEST(HelperSwap, ANewVersionThatSaysItStartedAndThenExitsWithAnErrorIsRolledBackToo)
{
    REQUIRE_CAN_SIGN();
    World world("swap-health-marker-then-exit");
    world.arrangeEnvironment("marker_then_exit3");
    const SpawnResult ran = world.runHelper(world.applyArguments());
    ASSERT_EQ(ran.exitCode, 11) << describe(ran);
    EXPECT_EQ(valueOf(ran.output, "reason"), "health_exit");
    world.expectV1();
    EXPECT_TRUE(world.logHas("health tag=v2 ")) << "the new version was started";
    EXPECT_FALSE(isPresent(healthMarkerPath(world.state, std::string(kTransaction))))
        << "the marker of a start that then failed is no evidence for the next one";
    EXPECT_EQ(world.last().status, "rolled_back");
    EXPECT_EQ(world.last().reason, "health_exit");
    EXPECT_EQ(blockedSequences(world.state), std::vector<std::uint64_t>{2000})
        << "v2's sequence: a release that starts and then fails is not offered again";
    ASSERT_TRUE(world.waitForLog("start tag=v1 ")) << world.stubLog();
}

TEST(HelperSwap, ANewVersionThatEndsWellWithoutSayingItStartedIsRolledBackAndNotOfferedAgain)
{
    REQUIRE_CAN_SIGN();
    // Exit code 0 and no marker: a program that was started and ended without a complaint, and
    // never got to its first idle turn - a release whose katana does not know the switch and
    // leaves at once, say. "A start that ... never writes it means rollback" (apply.hpp, step 7):
    // a clean exit alone proves nothing. It ended, so the helper has no wait to sit out: the
    // reason is the exit, not the time, and the 60 s of this run's health wait are not spent.
    World world("swap-health-exit0");
    world.arrangeEnvironment("exit0");
    world.registerAsInstaller();
    const auto begin = std::chrono::steady_clock::now();
    const SpawnResult ran =
        world.runHelper(world.applyArguments(std::string(kTransaction), 0, "500,1500,50,60000,30000"));
    const auto elapsed = std::chrono::steady_clock::now() - begin;
    ASSERT_EQ(ran.exitCode, 11) << describe(ran);
    EXPECT_EQ(valueOf(ran.output, "outcome"), "rolled_back");
    EXPECT_EQ(valueOf(ran.output, "reason"), "health_exit");
    EXPECT_LT(elapsed, 30s) << "the health wait must not have been sat out for a program that had ended";
    world.expectV1();
    EXPECT_EQ(world.displayVersion(), std::wstring(L"0.9.0")) << "a release that did not prove itself is never shown";
    EXPECT_TRUE(leftoversInUpdate(world.root).empty());
    EXPECT_FALSE(world.runOnceIsSet());
    EXPECT_FALSE(isPresent(healthMarkerPath(world.state, std::string(kTransaction)))) << "the premise: no marker";
    EXPECT_EQ(world.last().status, "rolled_back");
    EXPECT_EQ(world.last().reason, "health_exit");
    EXPECT_EQ(blockedSequences(world.state), std::vector<std::uint64_t>{2000}) << "v2's sequence";
    ASSERT_TRUE(world.waitForLog("start tag=v1 ")) << world.stubLog();
    const std::vector<std::string> lines = linesOf(world.stubLog());
    ASSERT_GE(lines.size(), 1u);
    EXPECT_EQ(lines.front().rfind("health tag=v2 ", 0), 0u) << lines.front();
    EXPECT_TRUE(contains(lines.front(), " mode=exit0")) << "the premise: the stub was in the mode asked for";
}

TEST(HelperSwap, ANewVersionThatNeverSaysItStartedIsRolledBackWhenTheHealthWaitEnds)
{
    REQUIRE_CAN_SIGN();
    World world("swap-health-timeout");
    world.arrangeEnvironment("silent");
    // The wait is 60 s in a shipped helper; the test shortens it (the fourth number).
    const SpawnResult ran = world.runHelper(world.applyArguments(std::string(kTransaction), 0, "500,1500,50,3000,30000"));
    ASSERT_EQ(ran.exitCode, 11) << describe(ran);
    EXPECT_EQ(valueOf(ran.output, "reason"), "health_timeout");
    world.expectV1();
    EXPECT_TRUE(world.logHas("health tag=v2 ")) << "the new version was started";
    EXPECT_EQ(world.last().status, "rolled_back");
    EXPECT_EQ(world.last().reason, "health_timeout");
    EXPECT_EQ(blockedSequences(world.state), std::vector<std::uint64_t>{2000})
        << "v2's sequence: a release that never says it started is not offered again";
    ASSERT_TRUE(world.waitForLog("start tag=v1 ")) << world.stubLog();
}

TEST(HelperSwap, AProgramRunningFromTheInstallBlocksTheUpdateAndNothingIsTouchedUntilRetry)
{
    REQUIRE_CAN_SIGN();
    World world("swap-blocked");
    world.arrangeEnvironment();
    // The install's own katana_cli.exe (the stub), running: the real scan must see it.
    auto holder = std::make_unique<Sleeper>(world.root / "bin" / "katana_cli.exe", 120'000);
    ASSERT_NE(holder->pid(), 0u);

    const SpawnResult blocked = world.runHelper(world.applyArguments());
    ASSERT_EQ(blocked.exitCode, 10) << describe(blocked);
    EXPECT_EQ(valueOf(blocked.output, "outcome"), "blocked");
    EXPECT_EQ(valueOf(blocked.output, "holder"), "katana_cli.exe (" + std::to_string(holder->pid()) + ")");
    world.expectV1();
    EXPECT_TRUE(leftoversInUpdate(world.root).empty()) << "a blocked update changed nothing";
    EXPECT_FALSE(world.runOnceIsSet());
    const LastResult last = world.last();
    EXPECT_EQ(last.status, "blocked");
    EXPECT_EQ(last.holder, "katana_cli.exe (" + std::to_string(holder->pid()) + ")");
    ASSERT_TRUE(world.waitForLog("start tag=v1 ")) << "the old version is started again\n" << world.stubLog();

    // Retry once the program is closed: a new transaction, the same stage.
    holder->terminate();
    holder.reset();
    world.installHelper("e1e2");
    const SpawnResult retried = world.runHelper(world.applyArguments("e1e2"), "e1e2");
    ASSERT_EQ(retried.exitCode, 0) << describe(retried);
    world.expectV2();
    ASSERT_TRUE(world.waitForLog("start tag=v2 ")) << world.stubLog();
}

TEST(HelperSwap, TheUpdateWaitsForTheWindowToExitAndGivesUpIfItNever)
{
    REQUIRE_CAN_SIGN();
    {
        // The window exits after a while: the helper waits for it and then applies.
        World world("swap-wait-parent");
        world.arrangeEnvironment();
        const auto window = startSleeperAt(world.top_ / "window" / "katana.exe", 1500);
        const SpawnResult ran = world.runHelper(world.applyArguments(std::string(kTransaction), window->pid(),
                                                                       "20000,1500,50,20000,30000"));
        EXPECT_EQ(ran.exitCode, 0) << describe(ran);
        world.expectV2();
        EXPECT_TRUE(world.waitForLog("start tag=v2 ")) << world.stubLog();
    }
    {
        // The window does not exit: blocked, naming it, and no second window is started beside it.
        World world("swap-parent-stays");
        world.arrangeEnvironment();
        const auto window = startSleeperAt(world.top_ / "window" / "katana.exe", 120'000);
        const SpawnResult ran = world.runHelper(world.applyArguments(std::string(kTransaction), window->pid()));
        EXPECT_EQ(ran.exitCode, 10) << describe(ran);
        EXPECT_EQ(valueOf(ran.output, "holder"), "Katana (" + std::to_string(window->pid()) + ")")
            << "a pid with no image to name is called Katana";
        world.expectV1();
        std::this_thread::sleep_for(500ms);
        EXPECT_FALSE(world.logHas("start tag=")) << "the window that hung is not started a second time\n" << world.stubLog();
    }
}

TEST(HelperSwap, AnUpdateRunningInAnotherHelperIsNotDisturbedAndExitsWithCode13)
{
    REQUIRE_CAN_SIGN();
    World world("swap-locked");
    world.arrangeEnvironment();
    fs::create_directories(world.root / ".update");
    // The lock as the other helper holds it: opened for writing with no sharing at all.
    const std::wstring lockPath = (world.root / ".update" / "lock").native();
    const HANDLE lock = CreateFileW(lockPath.c_str(), GENERIC_WRITE, 0, nullptr, CREATE_ALWAYS,
                                    FILE_ATTRIBUTE_NORMAL, nullptr);
    ASSERT_NE(lock, INVALID_HANDLE_VALUE);
    const SpawnResult ran = world.runHelper(world.applyArguments());
    CloseHandle(lock);
    EXPECT_EQ(ran.exitCode, 13) << describe(ran);
    EXPECT_EQ(valueOf(ran.output, "reason"), "locked");
    world.expectV1();
    EXPECT_FALSE(isPresent(world.state / "last.json")) << "the other helper owns the outcome";
    std::this_thread::sleep_for(300ms);
    EXPECT_FALSE(world.logHas("start tag=")) << "and nothing is started: the other helper starts Katana\n" << world.stubLog();
}

TEST(HelperSwap, ARollbackThatCannotFinishExitsWithCode12AndRecoverByHandPutsTheTreeBack)
{
    REQUIRE_CAN_SIGN();
    World world("swap-rollback-incomplete");
    // The new version starts, waits 1.5 s and exits 1; in that time this test holds a file of the
    // new version open with no sharing for deletion, so the rollback cannot move it back.
    world.arrangeEnvironment("delay_exit1");
    std::optional<SpawnResult> ran;
    std::thread helper([&] {
        ran = world.runHelper(world.applyArguments(std::string(kTransaction), 0, "500,1500,50,20000,1000"));
    });
    const bool started = world.waitForLog("health tag=v2 ");
    HANDLE held = INVALID_HANDLE_VALUE;
    if (started) {
        const std::wstring library = (world.root / "bin" / "libcore-1.dll").native();
        held = CreateFileW(library.c_str(), GENERIC_READ, FILE_SHARE_READ | FILE_SHARE_WRITE, nullptr, OPEN_EXISTING,
                           FILE_ATTRIBUTE_NORMAL, nullptr);
    }
    helper.join();
    ASSERT_TRUE(started) << "the new version never started: " << (ran ? describe(*ran) : std::string());
    ASSERT_NE(held, INVALID_HANDLE_VALUE) << "the new library could not be held: " << GetLastError();
    ASSERT_TRUE(ran.has_value());

    EXPECT_EQ(ran->exitCode, 12) << describe(*ran);
    EXPECT_EQ(valueOf(ran->output, "outcome"), "rollback_incomplete");
    EXPECT_EQ(valueOf(ran->output, "reason"), "rollback_failed");
    EXPECT_TRUE(contains(valueOf(ran->output, "detail"), "katana_updater --recover")) << "it names the command to run";
    EXPECT_EQ(world.last().status, "rollback_incomplete");
    EXPECT_TRUE(world.runOnceIsSet()) << "the next logon will try again: the entry is kept";
    std::this_thread::sleep_for(300ms);
    EXPECT_FALSE(world.logHas("start tag=")) << "a half-swapped tree is not started\n" << world.stubLog();

    // The lock is let go; the person (or the next start of a program, or the next logon) runs it.
    CloseHandle(held);
    const SpawnResult recovered = world.runHelper(world.withTestOptions({"--recover", toUtf8(world.root.native())}));
    EXPECT_EQ(recovered.exitCode, 0) << describe(recovered);
    EXPECT_EQ(valueOf(recovered.output, "recover"), "ok");
    world.expectV1();
    EXPECT_FALSE(world.runOnceIsSet()) << "recovery removes the entry";
    EXPECT_EQ(leftoversInUpdate(world.root), std::vector<std::string>{"recovery.log"})
        << "recovery cleared the transaction and kept its one log line";
}

TEST(HelperSwap, ACrashInTheMiddleOfTheCommitIsPutRightByRecoverAndLeavesOnlyTheRecoveryLog)
{
    REQUIRE_CAN_SIGN();
    SpawnResult crashed;
    std::uint64_t at = 0;
    const std::unique_ptr<World> world = crashedMidCommit("swap-crash", isHalfSwapped, &crashed, &at);
    ASSERT_TRUE(world != nullptr);
    SCOPED_TRACE("crashed before change " + std::to_string(at));
    EXPECT_TRUE(contains(crashed.output, "simulated crash before change " + std::to_string(at)));

    // What a crash leaves: the RunOnce entry, the lock released with the process, a half swapped tree.
    EXPECT_EQ(world->runOnce(), runOnceCommand(world->helperPath(std::string(kTransaction)), world->root));
    EXPECT_FALSE(isPresent(world->state / "last.json")) << "a process that was killed wrote nothing";
    EXPECT_FALSE(isPresent(world->root / ".update" / "recovery.log"));

    // Recovery as the RunOnce entry or a person runs it, in an environment whose
    // KATANA_UPDATE_STATE_DIR names a new empty folder: that folder must stay empty, and the
    // state folder the swap used must stay as it was, byte for byte.
    const Path elsewhere = world->top_ / "elsewhere";
    fs::create_directories(elsewhere);
    world->environment.set(L"KATANA_UPDATE_STATE_DIR", elsewhere);
    const std::map<std::string, std::string> before = snapshot(world->state);

    const std::vector<std::string> recoverArguments =
        world->withTestOptions({"--recover", toUtf8(world->root.native())});
    const SpawnResult recovered = world->runHelper(recoverArguments);
    ASSERT_EQ(recovered.exitCode, 0) << describe(recovered);
    EXPECT_EQ(valueOf(recovered.output, "recover"), "ok");
    world->expectV1();
    EXPECT_FALSE(world->runOnceIsSet()) << "recovery removes the RunOnce entry";
    EXPECT_FALSE(isPresent(world->root / ".update" / std::string(kTransaction))) << "the transaction folder is gone";

    EXPECT_TRUE(snapshot(elsewhere).empty()) << "recovery wrote into the per-user state folder";
    EXPECT_EQ(difference(before, snapshot(world->state)), "") << "recovery changed the state folder the swap used";
    EXPECT_FALSE(isPresent(world->state / "last.json")) << "no last.json: recover has no state folder to put one in";

    // The one record it does keep: a line in the install's own recovery.log.
    const Path log = world->root / ".update" / "recovery.log";
    ASSERT_TRUE(isPresent(log));
    const std::vector<std::string> lines = linesOf(readText(log));
    ASSERT_EQ(lines.size(), 1u) << readText(log);
    EXPECT_TRUE(contains(lines[0], " recover found=" + std::string(kTransaction) + " result=ok left="))
        << lines[0];
    // A UTC time first, as 2026-10-13T08:40:00Z is written.
    ASSERT_GE(lines[0].size(), 21u) << lines[0];
    EXPECT_EQ(lines[0][4], '-') << lines[0];
    EXPECT_EQ(lines[0][10], 'T') << lines[0];
    EXPECT_EQ(lines[0].substr(19, 2), "Z ") << lines[0];

    // And a second run has nothing to put right: nothing printed as work, nothing logged.
    const SpawnResult again = world->runHelper(recoverArguments);
    EXPECT_EQ(again.exitCode, 0) << describe(again);
    EXPECT_EQ(valueOf(again.output, "recover"), "nothing_to_do");
    EXPECT_EQ(linesOf(readText(log)), lines) << "a run that found nothing writes no line";
}

TEST(HelperSwap, RecoveryStartedTheWayAHeadlessProgramStartsItWritesTheRecoveryLogAndNoPerUserState)
{
    REQUIRE_CAN_SIGN();
    const std::unique_ptr<World> world = crashedMidCommit("swap-headless-recover", isHalfSwapped);
    ASSERT_TRUE(world != nullptr);
    const std::string tx(kTransaction);
    ASSERT_TRUE(namesIn(world->state).contains("run-" + tx + "/"))
        << "the premise: the helper copy the swap ran from is in U";
    // The per-user folder, entry by entry and byte by byte, as the crash left it.
    const std::map<std::string, std::string> before = snapshot(world->state);

    // katana_cli and katana_mcp call recover(R) first thing (recover.hpp); this process plays
    // their part. U is the folder the swap used (KATANA_UPDATE_STATE_DIR), so the helper copy is
    // found there and run with `--recover R` - the engine's own command line, no test option:
    // the environment names the registry of this test.
    const core::Status recovered = recover(world->root);
    ASSERT_TRUE(recovered.ok()) << recovered.error().describe();
    world->expectV1();
    EXPECT_FALSE(world->runOnceIsSet()) << "the helper removed the RunOnce entry the crash left";

    // What it wrote: one line in the install's own recovery.log.
    EXPECT_EQ(leftoversInUpdate(world->root), std::vector<std::string>{"recovery.log"});
    const std::vector<std::string> lines = linesOf(readText(world->root / ".update" / "recovery.log"));
    ASSERT_EQ(lines.size(), 1u);
    EXPECT_TRUE(contains(lines[0], " recover found=" + tx + " result=ok left=")) << lines[0];

    // What it did not write: any per-user state. No last.json - only the helper run for the
    // window writes one - no state.json, no health marker, no cleanup.list; and the rule is held
    // to the letter (recover.hpp: "writes only R/.update/recovery.log"): the per-user folder is
    // as it was, nothing in it made, changed or removed - the helper copy it ran included.
    EXPECT_FALSE(isPresent(world->state / "last.json"));
    EXPECT_FALSE(isPresent(world->state / "state.json"));
    EXPECT_FALSE(isPresent(healthMarkerPath(world->state, tx)));
    EXPECT_FALSE(isPresent(world->state / "cleanup.list"));
    EXPECT_EQ(difference(before, snapshot(world->state)), "")
        << "recover() changed the per-user state folder. A headless program writes no per-user state: "
           "recover() may read U to find the helper copy and must leave it as it is (recover.hpp)";

    // And a start after that finds nothing to do, and still writes nothing there.
    EXPECT_TRUE(recover(world->root).ok());
    EXPECT_EQ(linesOf(readText(world->root / ".update" / "recovery.log")), lines);
    EXPECT_EQ(difference(before, snapshot(world->state)), "");
}

TEST(HelperSwap, RecoverNeitherReadsNorWritesThroughAnUpdateFolderThatIsALink)
{
    REQUIRE_CAN_SIGN();
    // R/.update is a junction to a folder outside the install, and that folder holds what looks
    // like an interrupted transaction. Recovery reads nothing through a link (recover.cpp), and
    // the swap makes nothing through one, so no transaction of this install can be in there:
    // the program has nothing to do, says so, and neither lists the folder as this install's
    // nor leaves its recovery.log in it - which would be a write outside R.
    World world("swap-recover-link");
    world.arrangeEnvironment();
    const Path outside = world.top_ / "outside";
    writeText(outside / "c0ffee" / "journal", "1 PLAN c0ffee 1000 2000 00000000\n");
    writeText(outside / "note.txt", "somebody else's folder\n");
    const std::map<std::string, std::string> before = snapshot(outside);
    const Path link = world.root / ".update";
    std::string said;
    if (!makeJunction(link, outside, said)) {
        SKIP_UNLESS_AVAILABLE_OR_FAIL_IN_CI("a junction could not be made here: " + said);
    }
    ASSERT_TRUE(isPresent(link / "c0ffee" / "journal")) << "the premise: the link leads to the folder";

    const SpawnResult ran = world.runHelper(world.withTestOptions({"--recover", toUtf8(world.root.native())}));
    // The link itself goes before anything is judged (only the link: the folder it points to
    // stays), so that the tree and the folder are compared as they are and the world can be
    // cleared away.
    ASSERT_NE(RemoveDirectoryW(link.c_str()), 0) << "the junction could not be removed: " << GetLastError();

    EXPECT_EQ(ran.exitCode, 0) << describe(ran);
    EXPECT_EQ(valueOf(ran.output, "recover"), "nothing_to_do") << ran.output;
    EXPECT_FALSE(contains(ran.output, "c0ffee")) << "the folder behind the link was listed as this install's\n"
                                                 << ran.output;
    EXPECT_EQ(difference(before, snapshot(outside)), "") << "the folder behind the link was written to";
    EXPECT_FALSE(isPresent(outside / "recovery.log"));
    world.expectV1();
    EXPECT_FALSE(isPresent(world.state / "last.json"));
}

TEST(HelperSwap, ACrashInsideRecoveryLeavesAStateThatTheNextRecoverFinishes)
{
    REQUIRE_CAN_SIGN();
    const std::unique_ptr<World> world = crashedMidCommit("swap-recover-crash", isHalfSwapped);
    ASSERT_TRUE(world != nullptr);
    const std::string root = toUtf8(world->root.native());

    // Recovery itself is killed before its second change to the disk (the first is the journal's
    // ROLLING_BACK line): the tree is still mixed, the entry still stands, nothing was logged.
    std::vector<std::string> killed = world->withTestOptions({"--recover", root});
    killed.push_back("--die-at");
    killed.push_back("2");
    const SpawnResult died = world->runHelper(killed);
    ASSERT_EQ(died.exitCode, 99) << describe(died);
    EXPECT_NE(treeHash(world->root), world->v1Hash);
    EXPECT_NE(treeHash(world->root), world->v2Hash);
    EXPECT_TRUE(world->runOnceIsSet());
    EXPECT_FALSE(isPresent(world->root / ".update" / "recovery.log")) << "a process that was killed wrote no line";

    // The next one picks up from the disk and finishes.
    const SpawnResult finished = world->runHelper(world->withTestOptions({"--recover", root}));
    EXPECT_EQ(finished.exitCode, 0) << describe(finished);
    EXPECT_EQ(valueOf(finished.output, "recover"), "ok");
    world->expectV1();
    EXPECT_FALSE(world->runOnceIsSet());
    EXPECT_EQ(linesOf(readText(world->root / ".update" / "recovery.log")).size(), 1u);
}

TEST(HelperSwap, RecoverOnAnInstallThatWasNeverUpdatedHasNothingToDoAndWritesNothing)
{
    REQUIRE_CAN_SIGN();
    World world("swap-recover-clean");
    world.arrangeEnvironment();
    const SpawnResult ran = world.runHelper(world.withTestOptions({"--recover", toUtf8(world.root.native())}));
    EXPECT_EQ(ran.exitCode, 0) << describe(ran);
    EXPECT_EQ(valueOf(ran.output, "recover"), "nothing_to_do");
    world.expectV1();
    EXPECT_FALSE(isPresent(world.root / ".update")) << "not even the work folder is made";
    EXPECT_FALSE(isPresent(world.state / "last.json"));
}

TEST(HelperSwap, RecoverThatCannotPutTheTreeBackExitsWithCode12AndSaysWhy)
{
    REQUIRE_CAN_SIGN();
    // A crashed commit after the new library is in place; the library is then held, so that it
    // cannot be moved out again.
    const std::unique_ptr<World> world = crashedMidCommit("swap-recover-fails", [](World& w) {
        return readText(w.root / "bin" / "libcore-1.dll") == w.v2.find("bin/libcore-1.dll")->content;
    });
    ASSERT_TRUE(world != nullptr);
    // Held open with no sharing for deletion: it cannot be moved back, so the rollback cannot finish.
    const std::wstring library = (world->root / "bin" / "libcore-1.dll").native();
    const HANDLE held = CreateFileW(library.c_str(), GENERIC_READ, FILE_SHARE_READ | FILE_SHARE_WRITE, nullptr,
                                    OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, nullptr);
    ASSERT_NE(held, INVALID_HANDLE_VALUE) << GetLastError();
    const SpawnResult ran = world->runHelper(
        world->withTestOptions({"--recover", toUtf8(world->root.native()), "--timings", "500,500,50,500,500"}));
    CloseHandle(held);
    EXPECT_EQ(ran.exitCode, 12) << describe(ran);
    EXPECT_EQ(valueOf(ran.output, "recover"), "failed");
    EXPECT_TRUE(contains(valueOf(ran.output, "detail"), "katana_updater --recover")) << ran.output;
    EXPECT_TRUE(world->runOnceIsSet()) << "the entry stays for the next logon";
    EXPECT_FALSE(isPresent(world->state / "last.json")) << "a failed recovery writes no per-user state either";
    const std::vector<std::string> lines = linesOf(readText(world->root / ".update" / "recovery.log"));
    ASSERT_EQ(lines.size(), 1u);
    EXPECT_TRUE(contains(lines[0], "result=failed")) << lines[0];

    // Released, it finishes.
    const SpawnResult finished = world->runHelper(world->withTestOptions({"--recover", toUtf8(world->root.native())}));
    EXPECT_EQ(finished.exitCode, 0) << describe(finished);
    world->expectV1();
}

#endif // _WIN32
