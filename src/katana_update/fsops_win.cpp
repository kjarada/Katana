// The real file system of include/katana/update/fsops.hpp, for Windows. Compiled on Windows
// only (src/katana_update/CMakeLists.txt).
//
// What matters here, and why:
//   - A rename is MoveFileExW with NO flag. MOVEFILE_REPLACE_EXISTING is absent on purpose
//     (the apply never replaces by accident), MOVEFILE_COPY_ALLOWED is absent (a rename is
//     atomic or it fails; a cross-volume copy-and-delete is neither), and
//     MOVEFILE_DELAY_UNTIL_REBOOT is NEVER used: it completes at a reboot, by a different
//     account, with no way to roll it back (include/katana/update/fsops.hpp).
//   - Errors 5, 32 and 33 (access denied, sharing and lock violations) are what an antivirus
//     scanner holding a file for a moment causes, as much as a program that has it open, so
//     a rename retries them every RetryPolicy::interval for up to RetryPolicy::total.
//   - A file the update will read back is flushed (FlushFileBuffers) before the call returns.
//   - Directories are opened with FILE_FLAG_BACKUP_SEMANTICS, the only way to get a handle to
//     one.
//   - A path is given the \\?\ prefix when it is long (or ends in a dot or a space, which the
//     ordinary form would silently drop), so an install under a deep folder works without the
//     long-path manifest setting.
//   - "Reparse point" here means one that sends the path somewhere else (a symbolic link, a
//     junction, a mount point: the reparse tags whose "name surrogate" bit is set), which is
//     what a write through must be refused for. A OneDrive placeholder or a deduplicated file
//     has a reparse point too, but it is the file itself; refusing it would refuse an install
//     in a synchronised folder.

#include "katana/update/fsops.hpp"

#ifndef _WIN32
#error "fsops_win.cpp is compiled on Windows only"
#endif

#include <algorithm>
#include <string_view>
#include <thread>
#include <utility>
#include <vector>

#define WIN32_LEAN_AND_MEAN
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>

#include "katana/core/path_text.hpp"

namespace katana::update {

namespace {

using Path = std::filesystem::path;

constexpr DWORD kShareAll = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;
// Past this a file is not a record or a manifest, and readFile would hold it all in memory.
constexpr std::uint64_t kReadFileLimit = 256ull * 1024 * 1024;
constexpr DWORD kReparseNameSurrogate = 0x20000000;
constexpr std::size_t kLongPath = 240; // below MAX_PATH (260) with room for a file name

std::string contextOf(DWORD osError, const Path& path)
{
    return "os_error=" + std::to_string(osError) + " " + core::pathToUtf8(path);
}

core::Error osError(core::ErrorCode code, std::string message, DWORD error, const Path& path)
{
    return core::makeError(code, std::move(message), contextOf(error, path));
}

bool isMissingError(DWORD error)
{
    return error == ERROR_FILE_NOT_FOUND || error == ERROR_PATH_NOT_FOUND ||
           error == ERROR_INVALID_NAME || error == ERROR_BAD_PATHNAME ||
           error == ERROR_DIRECTORY || error == ERROR_NOT_A_REPARSE_POINT;
}

bool isRetryable(DWORD error)
{
    return std::find(std::begin(kRetryableOsErrors), std::end(kRetryableOsErrors),
                     static_cast<int>(error)) != std::end(kRetryableOsErrors);
}

Path absoluteNormal(const Path& path)
{
    std::error_code ec;
    Path absolute = std::filesystem::absolute(path, ec);
    if (ec) {
        absolute = path;
    }
    absolute = absolute.lexically_normal();
    absolute.make_preferred();
    return absolute;
}

bool endsInDotOrSpace(const std::wstring& text)
{
    // A trailing separator is not part of the name.
    std::size_t end = text.size();
    while (end > 0 && text[end - 1] == L'\\') {
        --end;
    }
    return end > 0 && (text[end - 1] == L'.' || text[end - 1] == L' ');
}

// The path as the wide string the system calls take.
std::wstring widePath(const Path& path)
{
    const Path absolute = absoluteNormal(path);
    std::wstring text = absolute.native();
    if (text.rfind(L"\\\\?\\", 0) == 0 || text.rfind(L"\\\\.\\", 0) == 0) {
        return text;
    }
    if (text.size() < kLongPath && !endsInDotOrSpace(text)) {
        return text;
    }
    if (text.rfind(L"\\\\", 0) == 0) { // \\server\share\... -> \\?\UNC\server\share\...
        return L"\\\\?\\UNC\\" + text.substr(2);
    }
    return L"\\\\?\\" + text;
}

// A handle that closes itself.
class Handle {
  public:
    Handle() = default;
    explicit Handle(HANDLE handle) : handle_(handle) {}
    Handle(const Handle&) = delete;
    Handle& operator=(const Handle&) = delete;
    Handle(Handle&& other) noexcept : handle_(std::exchange(other.handle_, INVALID_HANDLE_VALUE)) {}
    Handle& operator=(Handle&& other) noexcept
    {
        if (this != &other) {
            reset();
            handle_ = std::exchange(other.handle_, INVALID_HANDLE_VALUE);
        }
        return *this;
    }
    ~Handle() { reset(); }

    [[nodiscard]] bool valid() const { return handle_ != INVALID_HANDLE_VALUE; }
    [[nodiscard]] HANDLE get() const { return handle_; }
    void reset()
    {
        if (handle_ != INVALID_HANDLE_VALUE) {
            CloseHandle(handle_);
            handle_ = INVALID_HANDLE_VALUE;
        }
    }

  private:
    HANDLE handle_ = INVALID_HANDLE_VALUE;
};

class WindowsLock final : public FileLock {
  public:
    explicit WindowsLock(Handle handle) : handle_(std::move(handle)) {}

  private:
    Handle handle_;
};

// The reparse tag of an entry whose attributes say it has one. A failed lookup counts as a
// redirecting one: the safe answer for a caller that refuses to write through it.
bool redirects(const std::wstring& wide)
{
    WIN32_FIND_DATAW found{};
    const HANDLE search = FindFirstFileW(wide.c_str(), &found);
    if (search == INVALID_HANDLE_VALUE) {
        return true;
    }
    FindClose(search);
    return (found.dwReserved0 & kReparseNameSurrogate) != 0;
}

core::Status writeAll(HANDLE file, std::string_view bytes, const Path& path)
{
    std::size_t done = 0;
    while (done < bytes.size()) {
        const DWORD chunk = static_cast<DWORD>(std::min<std::size_t>(bytes.size() - done, 1u << 20));
        DWORD written = 0;
        if (!WriteFile(file, bytes.data() + done, chunk, &written, nullptr)) {
            return osError(core::ErrorCode::FileExportFailure, "the file could not be written",
                           GetLastError(), path);
        }
        done += written;
    }
    return {};
}

class WindowsFsOps final : public FsOps {
  public:
    explicit WindowsFsOps(const RetryPolicy& retry) : retry_(retry) {}

    // ---- looking ---------------------------------------------------------------------

    core::Result<EntryInfo> stat(const Path& path) const override
    {
        const std::wstring wide = widePath(path);
        WIN32_FILE_ATTRIBUTE_DATA data{};
        if (!GetFileAttributesExW(wide.c_str(), GetFileExInfoStandard, &data)) {
            const DWORD error = GetLastError();
            if (isMissingError(error)) {
                return EntryInfo{};
            }
            return osError(core::ErrorCode::FileImportFailure, "the entry cannot be examined",
                           error, path);
        }
        EntryInfo info;
        info.kind = (data.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) != 0 ? EntryKind::Directory
                                                                            : EntryKind::File;
        info.reparsePoint = (data.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT) != 0 &&
                            redirects(wide);
        info.size = (static_cast<std::uint64_t>(data.nFileSizeHigh) << 32) | data.nFileSizeLow;
        return info;
    }

    core::Result<std::vector<Path>> listDirectory(const Path& directory) const override
    {
        std::wstring pattern = widePath(directory);
        if (pattern.empty() || pattern.back() != L'\\') {
            pattern += L'\\';
        }
        pattern += L'*';
        WIN32_FIND_DATAW found{};
        const HANDLE search = FindFirstFileW(pattern.c_str(), &found);
        if (search == INVALID_HANDLE_VALUE) {
            const DWORD error = GetLastError();
            return osError(isMissingError(error) ? core::ErrorCode::NotFound
                                                 : core::ErrorCode::FileImportFailure,
                           "the directory cannot be listed", error, directory);
        }
        std::vector<Path> names;
        do {
            const std::wstring_view name = found.cFileName;
            if (name != L"." && name != L"..") {
                names.push_back(directory / Path(std::wstring(name)));
            }
        } while (FindNextFileW(search, &found));
        const DWORD last = GetLastError();
        FindClose(search);
        if (last != ERROR_NO_MORE_FILES) {
            return osError(core::ErrorCode::FileImportFailure, "the directory cannot be listed",
                           last, directory);
        }
        return names;
    }

    core::Result<std::string> readFile(const Path& path) const override
    {
        Handle file(CreateFileW(widePath(path).c_str(), GENERIC_READ, kShareAll, nullptr,
                                OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL | FILE_FLAG_SEQUENTIAL_SCAN,
                                nullptr));
        if (!file.valid()) {
            const DWORD error = GetLastError();
            return osError(isMissingError(error) ? core::ErrorCode::NotFound
                                                 : core::ErrorCode::FileImportFailure,
                           "the file cannot be opened", error, path);
        }
        LARGE_INTEGER size{};
        if (!GetFileSizeEx(file.get(), &size) || size.QuadPart < 0) {
            return osError(core::ErrorCode::FileImportFailure, "the file cannot be sized",
                           GetLastError(), path);
        }
        if (static_cast<std::uint64_t>(size.QuadPart) > kReadFileLimit) {
            return osError(core::ErrorCode::FileImportFailure, "the file is too large to read",
                           0, path);
        }
        std::string bytes(static_cast<std::size_t>(size.QuadPart), '\0');
        std::size_t done = 0;
        while (done < bytes.size()) {
            const DWORD want =
                static_cast<DWORD>(std::min<std::size_t>(bytes.size() - done, 1u << 20));
            DWORD got = 0;
            if (!ReadFile(file.get(), bytes.data() + done, want, &got, nullptr)) {
                return osError(core::ErrorCode::FileImportFailure, "the file cannot be read",
                               GetLastError(), path);
            }
            if (got == 0) {
                break; // it shrank while it was read
            }
            done += got;
        }
        bytes.resize(done);
        return bytes;
    }

    core::Result<Path> resolve(const Path& path) const override
    {
        const Path absolute = absoluteNormal(path);
        // The longest prefix that exists is the part a link can be in; the rest is names
        // that do not exist yet and so cannot be links.
        Path existing = absolute;
        std::vector<Path> tail;
        while (true) {
            if (GetFileAttributesW(widePath(existing).c_str()) != INVALID_FILE_ATTRIBUTES) {
                break;
            }
            const DWORD error = GetLastError();
            if (!isMissingError(error)) {
                return osError(core::ErrorCode::FileImportFailure, "the path cannot be resolved",
                               error, existing);
            }
            const Path parent = existing.parent_path();
            if (parent.empty() || parent == existing) {
                return absolute; // not even the drive exists
            }
            tail.push_back(existing.filename());
            existing = parent;
        }
        Handle handle(CreateFileW(widePath(existing).c_str(), 0, kShareAll, nullptr,
                                  OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS, nullptr));
        if (!handle.valid()) {
            return osError(core::ErrorCode::FileImportFailure, "the path cannot be resolved",
                           GetLastError(), existing);
        }
        std::wstring final(512, L'\0');
        DWORD length = GetFinalPathNameByHandleW(handle.get(), final.data(),
                                                 static_cast<DWORD>(final.size()),
                                                 FILE_NAME_NORMALIZED | VOLUME_NAME_DOS);
        if (length >= final.size()) {
            final.assign(length + 1, L'\0');
            length = GetFinalPathNameByHandleW(handle.get(), final.data(),
                                               static_cast<DWORD>(final.size()),
                                               FILE_NAME_NORMALIZED | VOLUME_NAME_DOS);
        }
        if (length == 0 || length >= final.size()) {
            return osError(core::ErrorCode::FileImportFailure, "the path cannot be resolved",
                           GetLastError(), existing);
        }
        final.resize(length);
        if (final.rfind(L"\\\\?\\UNC\\", 0) == 0) {
            final = L"\\\\" + final.substr(8);
        } else if (final.rfind(L"\\\\?\\", 0) == 0) {
            final = final.substr(4);
        }
        Path resolved(final);
        for (auto name = tail.rbegin(); name != tail.rend(); ++name) {
            resolved /= *name;
        }
        return resolved;
    }

    core::Result<std::uint64_t> freeSpace(const Path& path) const override
    {
        // The volume of the nearest folder that exists.
        Path probe = absoluteNormal(path);
        while (true) {
            const DWORD attributes = GetFileAttributesW(widePath(probe).c_str());
            if (attributes != INVALID_FILE_ATTRIBUTES) {
                if ((attributes & FILE_ATTRIBUTE_DIRECTORY) == 0) {
                    probe = probe.parent_path();
                }
                break;
            }
            const Path parent = probe.parent_path();
            if (parent.empty() || parent == probe) {
                break;
            }
            probe = parent;
        }
        ULARGE_INTEGER available{};
        ULARGE_INTEGER total{};
        ULARGE_INTEGER free{};
        if (!GetDiskFreeSpaceExW(widePath(probe).c_str(), &available, &total, &free)) {
            return osError(core::ErrorCode::FileImportFailure, "the free space cannot be read",
                           GetLastError(), probe);
        }
        return static_cast<std::uint64_t>(available.QuadPart);
    }

    // ---- changing --------------------------------------------------------------------

    core::Status makeDirectories(const Path& directory) override
    {
        const Path target = absoluteNormal(directory);
        std::vector<Path> missing;
        Path probe = target;
        while (true) {
            const DWORD attributes = GetFileAttributesW(widePath(probe).c_str());
            if (attributes != INVALID_FILE_ATTRIBUTES) {
                if ((attributes & FILE_ATTRIBUTE_DIRECTORY) == 0) {
                    return osError(core::ErrorCode::InvalidState,
                                   "a file is where a folder is needed", ERROR_DIRECTORY, probe);
                }
                break;
            }
            const DWORD error = GetLastError();
            if (!isMissingError(error)) {
                return osError(core::ErrorCode::FileExportFailure,
                               "the folder cannot be examined", error, probe);
            }
            missing.push_back(probe);
            const Path parent = probe.parent_path();
            if (parent.empty() || parent == probe) {
                break;
            }
            probe = parent;
        }
        for (auto folder = missing.rbegin(); folder != missing.rend(); ++folder) {
            if (!CreateDirectoryW(widePath(*folder).c_str(), nullptr)) {
                const DWORD error = GetLastError();
                if (error == ERROR_ALREADY_EXISTS &&
                    (GetFileAttributesW(widePath(*folder).c_str()) & FILE_ATTRIBUTE_DIRECTORY) != 0) {
                    continue; // made by someone else a moment ago
                }
                return osError(core::ErrorCode::FileExportFailure,
                               "the folder cannot be created", error, *folder);
            }
        }
        return {};
    }

    core::Status copyFile(const Path& from, const Path& to) override
    {
        Handle source(CreateFileW(widePath(from).c_str(), GENERIC_READ, kShareAll, nullptr,
                                  OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL | FILE_FLAG_SEQUENTIAL_SCAN,
                                  nullptr));
        if (!source.valid()) {
            const DWORD error = GetLastError();
            return osError(isMissingError(error) ? core::ErrorCode::NotFound
                                                 : core::ErrorCode::FileImportFailure,
                           "the file to copy cannot be opened", error, from);
        }
        // A new file with the folder's own access rules (not the source's: the stage folder is
        // writable by anyone running as the user, and a file in the install must take the
        // install's), written by hand rather than by CopyFileW, which would carry the
        // source's attributes and alternate streams along and gives no handle to flush.
        const std::wstring wideTo = widePath(to);
        Handle destination(CreateFileW(wideTo.c_str(), GENERIC_WRITE, 0, nullptr, CREATE_NEW,
                                       FILE_ATTRIBUTE_NORMAL | FILE_FLAG_SEQUENTIAL_SCAN, nullptr));
        if (!destination.valid()) {
            const DWORD error = GetLastError();
            return osError(error == ERROR_FILE_EXISTS || error == ERROR_ALREADY_EXISTS
                               ? core::ErrorCode::AlreadyExists
                               : (isMissingError(error) ? core::ErrorCode::NotFound
                                                        : core::ErrorCode::FileExportFailure),
                           "the copy cannot be created", error, to);
        }
        std::vector<char> buffer(1u << 20);
        core::Status status;
        while (status.ok()) {
            DWORD got = 0;
            if (!ReadFile(source.get(), buffer.data(), static_cast<DWORD>(buffer.size()), &got,
                          nullptr)) {
                status = osError(core::ErrorCode::FileImportFailure,
                                 "the file to copy cannot be read", GetLastError(), from);
                break;
            }
            if (got == 0) {
                break;
            }
            status = writeAll(destination.get(), std::string_view(buffer.data(), got), to);
        }
        if (status.ok() && !FlushFileBuffers(destination.get())) {
            status = osError(core::ErrorCode::FileExportFailure,
                             "the copy cannot be flushed to disk", GetLastError(), to);
        }
        if (status.ok()) {
            FILETIME written{};
            if (GetFileTime(source.get(), nullptr, nullptr, &written)) {
                SetFileTime(destination.get(), nullptr, nullptr, &written); // best effort
            }
        }
        destination.reset();
        if (!status.ok()) {
            DeleteFileW(wideTo.c_str()); // never leave half a file that looks like a copy
        }
        return status;
    }

    core::Status renameFile(const Path& from, const Path& to) override
    {
        const std::wstring wideFrom = widePath(from);
        const std::wstring wideTo = widePath(to);
        const auto deadline = std::chrono::steady_clock::now() + retry_.total;
        while (true) {
            if (MoveFileExW(wideFrom.c_str(), wideTo.c_str(), 0)) {
                return {};
            }
            const DWORD error = GetLastError();
            if (error == ERROR_ALREADY_EXISTS || error == ERROR_FILE_EXISTS) {
                return osError(core::ErrorCode::AlreadyExists, "the target exists", error, to);
            }
            if (isMissingError(error)) {
                return osError(core::ErrorCode::NotFound, "the file to move is not there", error,
                               from);
            }
            const auto now = std::chrono::steady_clock::now();
            if (!isRetryable(error) || now >= deadline) {
                return osError(core::ErrorCode::FileExportFailure,
                               "the file could not be moved", error, from);
            }
            std::this_thread::sleep_for(
                std::min<std::chrono::steady_clock::duration>(retry_.interval, deadline - now));
        }
    }

    core::Status removeFile(const Path& path) override
    {
        const std::wstring wide = widePath(path);
        if (DeleteFileW(wide.c_str())) {
            return {};
        }
        DWORD error = GetLastError();
        if (isMissingError(error)) {
            return {};
        }
        if (error == ERROR_ACCESS_DENIED) {
            const DWORD attributes = GetFileAttributesW(wide.c_str());
            if (attributes != INVALID_FILE_ATTRIBUTES && (attributes & FILE_ATTRIBUTE_READONLY) != 0 &&
                (attributes & FILE_ATTRIBUTE_DIRECTORY) == 0) {
                SetFileAttributesW(wide.c_str(), attributes & ~FILE_ATTRIBUTE_READONLY);
                if (DeleteFileW(wide.c_str())) {
                    return {};
                }
                error = GetLastError();
            }
        }
        return osError(core::ErrorCode::FileExportFailure, "the file could not be deleted", error,
                       path);
    }

    core::Status removeEmptyDirectory(const Path& path) override
    {
        if (RemoveDirectoryW(widePath(path).c_str())) {
            return {};
        }
        const DWORD error = GetLastError();
        if (isMissingError(error)) {
            return {};
        }
        if (error == ERROR_DIR_NOT_EMPTY) {
            return osError(core::ErrorCode::InvalidState, "the folder is not empty", error, path);
        }
        return osError(core::ErrorCode::FileExportFailure, "the folder could not be deleted",
                       error, path);
    }

    core::Status appendDurable(const Path& path, std::string_view bytes) override
    {
        // FILE_APPEND_DATA without FILE_WRITE_DATA: every write goes to the end, whatever
        // else is writing, and FlushFileBuffers is still allowed.
        Handle file(CreateFileW(widePath(path).c_str(), FILE_APPEND_DATA | FILE_WRITE_ATTRIBUTES | SYNCHRONIZE,
                                kShareAll, nullptr, OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr));
        if (!file.valid()) {
            return osError(core::ErrorCode::FileExportFailure, "the file cannot be opened",
                           GetLastError(), path);
        }
        core::Status status = writeAll(file.get(), bytes, path);
        if (status.ok() && !FlushFileBuffers(file.get())) {
            status = osError(core::ErrorCode::FileExportFailure,
                             "the file cannot be flushed to disk", GetLastError(), path);
        }
        return status;
    }

    core::Status writeDurable(const Path& path, std::string_view bytes) override
    {
        // Through a temporary and a rename, so a crash leaves the old file or the new one and
        // never half of one.
        Path temporary = path;
        temporary += L".katana-tmp";
        const std::wstring wideTemporary = widePath(temporary);
        {
            Handle file(CreateFileW(wideTemporary.c_str(), GENERIC_WRITE, 0, nullptr, CREATE_ALWAYS,
                                    FILE_ATTRIBUTE_NORMAL, nullptr));
            if (!file.valid()) {
                return osError(core::ErrorCode::FileExportFailure, "the file cannot be created",
                               GetLastError(), temporary);
            }
            core::Status status = writeAll(file.get(), bytes, temporary);
            if (status.ok() && !FlushFileBuffers(file.get())) {
                status = osError(core::ErrorCode::FileExportFailure,
                                 "the file cannot be flushed to disk", GetLastError(), temporary);
            }
            if (!status.ok()) {
                file.reset();
                DeleteFileW(wideTemporary.c_str());
                return status;
            }
        }
        const std::wstring wide = widePath(path);
        const auto deadline = std::chrono::steady_clock::now() + retry_.total;
        while (true) {
            if (MoveFileExW(wideTemporary.c_str(), wide.c_str(),
                            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)) {
                return {};
            }
            const DWORD error = GetLastError();
            const auto now = std::chrono::steady_clock::now();
            if (!isRetryable(error) || now >= deadline) {
                DeleteFileW(wideTemporary.c_str());
                return osError(core::ErrorCode::FileExportFailure, "the file could not be replaced",
                               error, path);
            }
            std::this_thread::sleep_for(
                std::min<std::chrono::steady_clock::duration>(retry_.interval, deadline - now));
        }
    }

    core::Status setExecutable(const Path&) override { return {}; }

    // ---- durability and exclusion --------------------------------------------------------

    core::Status syncDirectory(const Path& directory) override
    {
        // Best effort: a directory handle can be flushed on NTFS, and some file systems
        // refuse. NTFS logs a rename's metadata change in order, so a refusal costs nothing
        // the recovery does not already tolerate (any prefix of the renames).
        Handle handle(CreateFileW(widePath(directory).c_str(), GENERIC_WRITE, kShareAll, nullptr,
                                  OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS, nullptr));
        if (handle.valid()) {
            FlushFileBuffers(handle.get());
        }
        return {};
    }

    core::Result<std::unique_ptr<FileLock>> tryLock(const Path& path) override
    {
        Handle handle(CreateFileW(widePath(path).c_str(), GENERIC_READ | GENERIC_WRITE, 0, nullptr,
                                  OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr));
        if (!handle.valid()) {
            const DWORD error = GetLastError();
            if (error == ERROR_SHARING_VIOLATION || error == ERROR_LOCK_VIOLATION) {
                return osError(core::ErrorCode::InvalidState, "held", error, path);
            }
            return osError(isMissingError(error) ? core::ErrorCode::NotFound
                                                 : core::ErrorCode::FileExportFailure,
                           "the lock file cannot be opened", error, path);
        }
        return std::unique_ptr<FileLock>(std::make_unique<WindowsLock>(std::move(handle)));
    }

  private:
    RetryPolicy retry_;
};

} // namespace

std::unique_ptr<FsOps> makeSystemFsOps(const RetryPolicy& retry)
{
    return std::make_unique<WindowsFsOps>(retry);
}

} // namespace katana::update
