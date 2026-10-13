// The Windows process scan of include/katana/update/process.hpp: who is running, whether a
// process id is alive, and which processes have their program under the install root.
//
// This file lives with the helper, but the UPDATE LIBRARY compiles it (src/katana_update/
// CMakeLists.txt), because katana, katana_cli and katana_mcp ask the same question to say
// `blocked`, and runningPrograms() is part of the engine's API. CMake compiles it on Windows only.
//
// WHAT "HOLDS" THE INSTALL. A program of the old version that stayed running through a swap would
// go on as old code among new files, and its own file can be neither replaced nor deleted while
// it runs; so the question is "which processes were started from a file under R". Toolhelp
// lists every process of the machine; QueryFullProcessImageNameW names the file each was started
// from. The modules a process has loaded are not asked: a DLL that is loaded can be renamed (it
// is opened with FILE_SHARE_DELETE), which is all the commit does, and listing the modules of
// every process fails on exactly the protected ones. A process that has ended is not a holder,
// whoever still has a handle to it.
//
// WHAT THE SCAN CANNOT SEE, AND WHAT THEN HAPPENS. A process this user may not open - another
// account's, which is also what a Katana started "as administrator" with another account's
// password is - gives no image path, and is passed over: it is NOT taken for a holder. Nothing
// later stops the swap on its account either. By every account the design found (its section
// 12) Windows lets the file of a running program be renamed and refuses only to delete or
// overwrite it, and renames are all the commit does; so the update goes through under such a
// program, which runs on as the old code among the new version's files, its own file now in
// R/.update/<tx>/old, where the clean-up cannot delete it yet and lists it for the next start.
// That is the residual, said here rather than hidden:
//   - the per-user install (design, D1) is a folder of the person's own profile, which no other
//     account runs a program from; it takes an archive unpacked where several accounts can
//     reach it, and one of them running Katana from it at that moment;
//   - the one thing known of such a process is its file's name (PROCESSENTRY32W::szExeFile), and
//     a name does not say which install it was started from: to hold the update on "somebody
//     runs a katana.exe" would block it for as long as any other account runs any Katana of its
//     own, on a machine people share;
//   - no elevation is asked for to see more: a per-user install is updated by the user.
// A snapshot that cannot be taken or read at all is another matter: that is an error, never "no
// holders" (apply() then refuses with "scan_failed" and touches nothing).
//
// PATHS ARE COMPARED AS TEXT, so both sides are first put in one form. The root can be spelled in
// many ways that name one folder - upper or lower case, "/" for "\", a trailing separator, "." and
// ".." segments, the \\?\ prefix, 8.3 short names, a junction - and the kernel reports an image's
// path as it opened the file. normalised() settles the first five lexically; the short names and
// links are settled by asking the file system for the root's long and final names, and the
// comparison then tries every spelling of the root. Case is folded by CompareStringOrdinal, which
// uses the file system's own (NTFS) upper-case table and not just ASCII.
//
// A process id can be reused. isAlive() asks about the id, and the helper asks it for the id of
// the window that started it, a few seconds after that window began to close: a recycled id inside
// that interval is possible in principle and would only make the helper wait for the wrong
// process until the 60 s limit and then report "Katana is still running"; the person presses
// Retry. The interface has no process creation time to tell the two apart.

#include "katana/update/process.hpp"

#ifndef _WIN32
#error "process_scan_win.cpp is compiled on Windows only"
#endif

#include <algorithm>
#include <string>
#include <string_view>
#include <vector>

#define WIN32_LEAN_AND_MEAN
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>
#include <tlhelp32.h>

namespace katana::update {

namespace {

class Handle {
  public:
    explicit Handle(HANDLE handle = nullptr) : handle_(handle) {}
    Handle(const Handle&) = delete;
    Handle& operator=(const Handle&) = delete;
    ~Handle()
    {
        if (handle_ != nullptr && handle_ != INVALID_HANDLE_VALUE) {
            CloseHandle(handle_);
        }
    }
    [[nodiscard]] HANDLE get() const { return handle_; }
    [[nodiscard]] bool valid() const
    {
        return handle_ != nullptr && handle_ != INVALID_HANDLE_VALUE;
    }

  private:
    HANDLE handle_;
};

// A path in one form: backslashes only, no \\?\ or \\.\ prefix, no "." or empty segment, ".."
// folded in (never above the drive or the server and share), no trailing separator - except on a
// drive root, which keeps its one. Pure text; nothing is asked of the file system.
std::wstring normalised(std::wstring text)
{
    std::replace(text.begin(), text.end(), L'/', L'\\');
    constexpr std::wstring_view kExtendedUnc = LR"(\\?\UNC\)";
    constexpr std::wstring_view kExtended = LR"(\\?\)";
    constexpr std::wstring_view kDevice = LR"(\\.\)";
    if (text.starts_with(kExtendedUnc)) {
        text = L"\\\\" + text.substr(kExtendedUnc.size());
    } else if (text.starts_with(kExtended) || text.starts_with(kDevice)) {
        text.erase(0, 4);
    }

    std::wstring anchor;
    std::size_t position = 0;
    std::size_t fixedSegments = 0; // the server and the share of a UNC path are not folded away
    bool rooted = false;
    if (text.starts_with(L"\\\\")) {
        anchor = L"\\\\";
        position = 2;
        fixedSegments = 2;
        rooted = true;
    } else if (text.size() >= 2 && text[1] == L':') {
        anchor = text.substr(0, 2);
        position = 2;
        rooted = position < text.size() && text[position] == L'\\';
    } else if (!text.empty() && text[0] == L'\\') {
        rooted = true;
    }

    std::vector<std::wstring> segments;
    while (position <= text.size()) {
        std::size_t end = text.find(L'\\', position);
        if (end == std::wstring::npos) {
            end = text.size();
        }
        const std::wstring segment = text.substr(position, end - position);
        if (segment == L"..") {
            if (segments.size() > fixedSegments) {
                segments.pop_back();
            }
        } else if (!segment.empty() && segment != L".") {
            segments.push_back(segment);
        }
        position = end + 1;
    }

    std::wstring out = anchor;
    if (rooted && !(anchor == L"\\\\")) {
        out += L'\\';
    }
    for (std::size_t i = 0; i < segments.size(); ++i) {
        if (i > 0) {
            out += L'\\';
        }
        out += segments[i];
    }
    return out;
}

// GetLongPathNameW of an existing path (8.3 short names written out); the input when it fails
// (the path is not there, or the volume has no short names to expand).
std::wstring longName(const std::wstring& path)
{
    std::wstring buffer(path.size() + 64, L'\0');
    for (int attempt = 0; attempt < 3; ++attempt) {
        const DWORD length =
            GetLongPathNameW(path.c_str(), buffer.data(), static_cast<DWORD>(buffer.size()));
        if (length == 0) {
            return path;
        }
        if (length < buffer.size()) {
            buffer.resize(length);
            return buffer;
        }
        buffer.resize(length + 1);
    }
    return path;
}

// The name the file system gives the folder after every link is followed (junctions, symbolic
// links, subst drives); empty when it cannot be opened.
std::wstring finalName(const std::wstring& path)
{
    const Handle folder(CreateFileW(path.c_str(), 0,
                                    FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                                    nullptr, OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS, nullptr));
    if (!folder.valid()) {
        return {};
    }
    std::wstring buffer(512, L'\0');
    for (int attempt = 0; attempt < 3; ++attempt) {
        const DWORD length = GetFinalPathNameByHandleW(folder.get(), buffer.data(),
                                                       static_cast<DWORD>(buffer.size()),
                                                       FILE_NAME_NORMALIZED | VOLUME_NAME_DOS);
        if (length == 0) {
            return {};
        }
        if (length < buffer.size()) {
            buffer.resize(length);
            return buffer;
        }
        buffer.resize(length + 1);
    }
    return {};
}

// True for a path that names a place without help from the current directory: a drive and a
// separator, or a UNC or extended-length prefix. std::filesystem does not call all of those
// absolute (the \\?\ form it reads as having no root directory), and then makes a mess of them.
bool namesAPlace(const std::wstring& text)
{
    if (text.size() >= 3 && text[1] == L':' && (text[2] == L'\\' || text[2] == L'/')) {
        return true;
    }
    return text.starts_with(L"\\\\") || text.starts_with(L"//");
}

// The root as absolute text: as it is, or made absolute against the current directory.
std::wstring absoluteText(const std::filesystem::path& root)
{
    const std::wstring text = root.native();
    if (namesAPlace(text)) {
        return text;
    }
    std::wstring buffer(MAX_PATH, L'\0');
    for (int attempt = 0; attempt < 3; ++attempt) {
        const DWORD length =
            GetFullPathNameW(text.c_str(), static_cast<DWORD>(buffer.size()), buffer.data(),
                             nullptr);
        if (length == 0) {
            return text;
        }
        if (length < buffer.size()) {
            buffer.resize(length);
            return buffer;
        }
        buffer.resize(length + 1);
    }
    return text;
}

// Every spelling of the root that the comparison tries, without repeats.
std::vector<std::wstring> rootSpellings(const std::filesystem::path& root)
{
    std::vector<std::wstring> spellings;
    const auto add = [&spellings](std::wstring spelling) {
        if (!spelling.empty() &&
            std::find(spellings.begin(), spellings.end(), spelling) == spellings.end()) {
            spellings.push_back(std::move(spelling));
        }
    };
    const std::wstring plain = normalised(absoluteText(root));
    add(plain);
    add(normalised(longName(plain)));
    add(normalised(finalName(plain)));
    return spellings;
}

// True when `path` is the folder `root` names or something inside it. The character after the
// root must be a separator (or the root must end in one), or "C:\Katana2" would be "under"
// "C:\Katana".
bool isUnder(const std::wstring& root, const std::wstring& path)
{
    if (root.empty() || path.size() <= root.size()) {
        return false;
    }
    if (CompareStringOrdinal(path.data(), static_cast<int>(root.size()), root.data(),
                             static_cast<int>(root.size()), TRUE) != CSTR_EQUAL) {
        return false;
    }
    return root.back() == L'\\' || path[root.size()] == L'\\';
}

// A Toolhelp snapshot of the processes; INVALID_HANDLE_VALUE and the error when it fails.
HANDLE takeSnapshot(DWORD& error)
{
    for (int attempt = 0; attempt < 5; ++attempt) {
        const HANDLE snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if (snapshot != INVALID_HANDLE_VALUE) {
            return snapshot;
        }
        error = GetLastError();
        if (error != ERROR_BAD_LENGTH) { // the one transient failure the API documents
            break;
        }
    }
    return INVALID_HANDLE_VALUE;
}

class WindowsProcessOps final : public ProcessOps {
  public:
    [[nodiscard]] std::uint64_t currentPid() const override { return GetCurrentProcessId(); }

    [[nodiscard]] bool isAlive(std::uint64_t pid) const override
    {
        if (pid == 0 || pid > 0xFFFFFFFFull) {
            return false;
        }
        const Handle process(OpenProcess(SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION, FALSE,
                                         static_cast<DWORD>(pid)));
        if (!process.valid()) {
            // No such process is "not alive"; a process we may not open is one that exists.
            return GetLastError() == ERROR_ACCESS_DENIED;
        }
        return WaitForSingleObject(process.get(), 0) == WAIT_TIMEOUT;
    }

    [[nodiscard]] core::Result<std::vector<RunningProgram>>
    holdersUnder(const std::filesystem::path& root) const override
    {
        if (root.empty()) {
            return core::makeError(core::ErrorCode::InvalidArgument,
                                   "no install folder to look for programs under");
        }
        const std::vector<std::wstring> spellings = rootSpellings(root);

        DWORD snapshotError = 0;
        const Handle snapshot(takeSnapshot(snapshotError));
        if (!snapshot.valid()) {
            return core::makeError(core::ErrorCode::Internal,
                                   "the running programs cannot be listed",
                                   "os_error=" + std::to_string(snapshotError));
        }

        const DWORD self = GetCurrentProcessId();
        std::vector<RunningProgram> holders;
        std::wstring image(32768, L'\0'); // the longest path Windows has
        PROCESSENTRY32W entry{};
        entry.dwSize = sizeof(entry);
        if (!Process32FirstW(snapshot.get(), &entry)) {
            // A snapshot holds this process at the least, so one with no first entry was not
            // read: an error, and never the answer "nobody is running" (see the top).
            return core::makeError(core::ErrorCode::Internal,
                                   "the running programs cannot be listed",
                                   "os_error=" + std::to_string(GetLastError()));
        }
        do {
            if (entry.th32ProcessID == self || entry.th32ProcessID == 0) {
                continue; // this process is never its own holder; 0 is the idle process
            }
            const Handle process(
                OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, entry.th32ProcessID));
            if (!process.valid()) {
                continue; // exited since the snapshot, or not ours to open (see the top)
            }
            // A process that has ended holds nothing, though its id and its image name stay
            // readable for as long as anybody keeps a handle to it (the program that started it
            // and has not yet collected its exit code, say). An exit code of 259 itself reads as
            // "still running", which only errs towards naming a holder.
            DWORD exitCode = 0;
            if (GetExitCodeProcess(process.get(), &exitCode) && exitCode != STILL_ACTIVE) {
                continue;
            }
            DWORD length = static_cast<DWORD>(image.size());
            if (!QueryFullProcessImageNameW(process.get(), 0, image.data(), &length) ||
                length == 0) {
                continue;
            }
            std::wstring path = normalised(std::wstring(image.data(), length));
            bool under = false;
            for (const std::wstring& spelling : spellings) {
                under = under || isUnder(spelling, path);
            }
            if (!under && path.find(L'~') != std::wstring::npos) {
                // An 8.3 name in the image's path, against a root spelled long: expand it.
                const std::wstring expanded = normalised(longName(path));
                for (const std::wstring& spelling : spellings) {
                    under = under || isUnder(spelling, expanded);
                }
                if (under) {
                    path = expanded;
                }
            }
            if (under) {
                holders.push_back({entry.th32ProcessID, std::filesystem::path(path)});
            }
        } while (Process32NextW(snapshot.get(), &entry));
        std::sort(holders.begin(), holders.end(),
                  [](const RunningProgram& a, const RunningProgram& b) { return a.pid < b.pid; });
        return holders;
    }
};

} // namespace

std::unique_ptr<ProcessOps> makeSystemProcessOps()
{
    return std::make_unique<WindowsProcessOps>();
}

core::Result<std::vector<RunningProgram>> runningPrograms(const std::filesystem::path& root)
{
    return makeSystemProcessOps()->holdersUnder(root);
}

} // namespace katana::update
